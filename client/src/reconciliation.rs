use bevy::prelude::*;
use rapier3d::prelude::{RigidBodyHandle, Vector};
use std::collections::{HashMap, HashSet};

use common::game_objects::planet::{apply_gravity_impulses, PlanetBehaviorComponent};
use common::net::message::{NetworkID, SimulationState};
use common::physics::physics_world::{GravityScale, RigidBodyHandleComponenet, PhysicsWorld, restore_snapshot, snapshot_bodies, step_world};
use common::pawn::pawn::{gather_pawn_input, PawnInput, Possessed};
use common::ring_buffer::RingBuffer;
use common::tick::Ticker;

/// Per-body accumulated physics error to smooth out over time.
/// Each field is the delta (target − current) at the moment it was set.
#[derive(Default)]
struct BodyError {
    pos:    Vec3,
    rot:    Vec3, // axis * angle
    linvel: Vec3,
    angvel: Vec3,
}

impl BodyError {
    fn is_nearly_zero(&self) -> bool {
        self.pos.length_squared() < 1e-4 && self.linvel.length_squared() < 1e-2
    }
}

/// Accumulated per-body physics errors, exponentially drained each tick.
#[derive(Resource, Default)]
pub struct PhysicsErrors(HashMap<NetworkID, BodyError>);

/// Holds the most recent server snapshot waiting to be consumed by `maybe_reconcile`.
/// Set by `on_message` when a `MsgType::State` arrives.
#[derive(Resource, Default)]
pub struct PendingReconciliation(pub Option<SimulationState>);

/// Ring buffer of every local physics snapshot, one per tick, for all networked bodies.
/// Used by reconciliation to restore bodies the server didn't mention.
#[derive(Resource)]
pub struct LocalStateHistory(pub RingBuffer<SimulationState>);
impl Default for LocalStateHistory {
    fn default() -> Self {
        Self(RingBuffer::new(128))
    }
}

pub struct ReconciliationPlugin<S: States + Copy, T: Component<Mutability = bevy::ecs::component::Mutable>>(
    pub S,
    pub fn(&mut PhysicsWorld, &RigidBodyHandleComponenet, PawnInput, &mut T),
);

impl<S: States + Copy, T: Component<Mutability = bevy::ecs::component::Mutable>> Plugin for ReconciliationPlugin<S, T> {
    fn build(&self, app: &mut App) {
        let state = self.0;
        let apply = self.1;
        app.init_resource::<PendingReconciliation>()
            .init_resource::<LocalStateHistory>()
            .init_resource::<PhysicsErrors>()
            .add_systems(FixedPreUpdate,
                (apply_physics_corrections, maybe_reconcile(apply)).chain()
                    .before(gather_pawn_input)
                    .run_if(in_state(state)))
            .add_systems(FixedPostUpdate, record_world_state.run_if(in_state(state)));
    }
}

/// Runs after `step_physics`. Snapshots all networked bodies into the local history so
/// `maybe_reconcile` can compare and restore them against the authoritative server state.
pub fn record_world_state(
    world: Res<PhysicsWorld>,
    tick: Res<Ticker>,
    mut history: ResMut<LocalStateHistory>,
    query: Query<(&NetworkID, &RigidBodyHandleComponenet)>,
) {
    history.0.push(snapshot_bodies(&world, tick.tick, query.iter()));
}

/// Exponentially drains per-body physics errors by applying a fraction each tick.
/// Alpha = 0.1 → ~90% corrected after ~22 ticks (~0.37 s at 60 Hz).
pub fn apply_physics_corrections(
    mut errors: ResMut<PhysicsErrors>,
    mut world: ResMut<PhysicsWorld>,
    bodies: Query<(&NetworkID, &RigidBodyHandleComponenet)>,
) {
    const ALPHA: f32 = 0.2; // how quickly it corrects
    if errors.0.is_empty() { return; }
    let handles: HashMap<NetworkID, RigidBodyHandle> = bodies.iter().map(|(nid, h)| (nid.clone(), h.0)).collect();
    errors.0.retain(|net_id, error| {
        if error.is_nearly_zero() { return false; }
        let Some(&handle) = handles.get(net_id) else { return false };
        let Some(rb) = world.rigid_body_set.get_mut(handle) else { return false };

        let dp = error.pos * ALPHA;
        let dv = error.linvel * ALPHA;
        let dav = error.angvel * ALPHA;
        let dr = error.rot * ALPHA;

        let t = rb.position().translation;
        rb.set_translation(Vector::new(t.x + dp.x, t.y + dp.y, t.z + dp.z), true);

        let ang = dr.length();
        if ang > 1e-6 {
            let r = rb.rotation();
            let cur = Quat::from_xyzw(r.x, r.y, r.z, r.w);
            let delta = Quat::from_axis_angle(dr / ang, ang);
            let new_rot = delta * cur;
            rb.set_rotation(new_rot, true);
        }

        let v = rb.linvel();
        rb.set_linvel(Vector::new(v.x + dv.x, v.y + dv.y, v.z + dv.z), true);
        let av = rb.angvel();
        rb.set_angvel(Vector::new(av.x + dav.x, av.y + dav.y, av.z + dav.z), true);

        // Decay the remaining error
        let keep = 1.0 - ALPHA;
        error.pos    *= keep;
        error.rot    *= keep;
        error.linvel *= keep;
        error.angvel *= keep;

        true
    });
}

/// Runs at the start of `FixedPreUpdate`, after apply_physics_corrections.
///
/// For every pending server snapshot:
///   1. Snapshot current physics state.
///   2. Fast-forward: restore to server state, replay buffered inputs from snapshot.tick+1..current.
///   3. Snapshot the resim result.
///   4. Compute per-body error = resim_result − current_state, store in PhysicsErrors.
///   5. Restore physics to current state — corrections are applied gradually by apply_physics_corrections.
pub fn maybe_reconcile<T: Component<Mutability = bevy::ecs::component::Mutable>>(
    apply: fn(&mut PhysicsWorld, &RigidBodyHandleComponenet, PawnInput, &mut T),
) -> impl Fn(ResMut<PendingReconciliation>, ResMut<PhysicsWorld>, Res<Ticker>, Res<LocalStateHistory>, Query<(&NetworkID, &RigidBodyHandleComponenet, Option<&Possessed>)>, Query<&mut T, With<Possessed>>, Query<(&PlanetBehaviorComponent, &RigidBodyHandleComponenet)>, Query<&GravityScale>, ResMut<PhysicsErrors>) {
    move |mut pending, mut world, tick, history, bodies, mut pawn_query, planets, gravity_scales, mut errors| {
        let Some(snapshot) = pending.0.take() else { return };

        let Some((_, our_handle, possessed)) =
            bodies.iter().find_map(|(nid, h, p)| p.map(|poss| (nid, h, poss)))
        else {
            return;
        };

        let pairs: Vec<(NetworkID, RigidBodyHandle)> = bodies
            .iter()
            .map(|(nid, h, _)| (nid.clone(), h.0))
            .collect();

        // Snapshot what physics currently looks like (pre-correction baseline)
        let current_state = snapshot_bodies(&world, tick.tick, bodies.iter().map(|(nid, h, _)| (nid, h)));

        // Fast-forward: restore to server authoritative state and replay inputs
        restore_snapshot(&mut world, &snapshot, &pairs);

        if let Some(local) = history.0.iter().find(|s| s.tick == snapshot.tick) {
            let unmentioned: Vec<(NetworkID, RigidBodyHandle)> = pairs.iter()
                .filter(|(nid, _)| !snapshot.bodies.contains_key(nid))
                .cloned()
                .collect();
            restore_snapshot(&mut world, local, &unmentioned);
        }

        // Freeze non-networked bodies (e.g. projectiles) so they aren't stepped during replay.
        let tracked: HashSet<RigidBodyHandle> = pairs.iter().map(|(_, h)| *h).collect();
        let to_freeze: Vec<RigidBodyHandle> = world.rigid_body_set.iter()
            .filter(|(h, rb)| !tracked.contains(h) && !rb.is_fixed() && rb.is_enabled())
            .map(|(h, _)| h)
            .collect();
        for &h in &to_freeze {
            if let Some(rb) = world.rigid_body_set.get_mut(h) { rb.set_enabled(false); }
        }

        let our_rb = our_handle.0;
        let current_tick = tick.tick;
        for replay_tick in (snapshot.tick + 1)..current_tick {
            if let Some(&input) = possessed.get_input(replay_tick) {
                if let Ok(mut component) = pawn_query.single_mut() {
                    let handle = RigidBodyHandleComponenet(our_rb);
                    apply(&mut world, &handle, input, &mut component);
                }
            }
            apply_gravity_impulses(&mut world, &planets, &gravity_scales);
            step_world(&mut world);
        }

        // Re-enable frozen non-networked bodies.
        for &h in &to_freeze {
            if let Some(rb) = world.rigid_body_set.get_mut(h) { rb.set_enabled(true); }
        }

        // Snapshot resim result, then restore current state
        let resim_state = snapshot_bodies(&world, tick.tick, bodies.iter().map(|(nid, h, _)| (nid, h)));
        restore_snapshot(&mut world, &current_state, &pairs);

        // Compute and store per-body errors (resim − current)
        for (net_id, resim) in &resim_state.bodies {
            let Some(cur) = current_state.bodies.get(net_id) else { continue };

            let rot_err = resim.rotation * cur.rotation.conjugate();
            let (axis, angle) = rot_err.to_axis_angle();

            let entry = errors.0.entry(net_id.clone()).or_default();
            entry.pos    = resim.position - cur.position;
            entry.rot    = axis * angle;
            entry.linvel = resim.linvel - cur.linvel;
            entry.angvel = resim.angvel - cur.angvel;
        }
    }
}
