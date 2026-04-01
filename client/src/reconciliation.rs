use bevy::prelude::*;
use rapier3d::prelude::{RigidBodyHandle, Vector};
use std::collections::{HashMap, HashSet};

use common::tick::{NetworkStats, Ticker};
use game_objects::pawn::Pawn;
use game_objects::atmosphere::{AtmosphereComponent, apply_wind_resistance_impulses};
use game_objects::pawn::biped::BipedPawnComponent;
use game_objects::pawn::spaceship::SpaceshipPawnComponent;
use game_objects::pawn::{GatherInputSet, Possessed};
use game_objects::planet::{
    PlanetComponent, apply_gravity_impulses, orient_bipeds_to_planets_impulses,
};
use net::message::{NetworkID, SimulationState};
use physics::physics_world::{
    GravityScale, PhysicsWorld, RigidBodyHandleComponent, rb_angvel, rb_pos, rb_rot, rb_vel,
    restore_snapshot, snapshot_bodies, step_world,
};
/// manages client-side rollback/correction, like in Rocket League
pub struct ReconciliationPlugin<S: States + Copy>(pub S);
impl<S: States + Copy> ReconciliationPlugin<S> {
    pub fn new(state: S) -> Self {
        Self(state)
    }
}

impl<S: States + Copy> Plugin for ReconciliationPlugin<S> {
    fn build(&self, app: &mut App) {
        let state = self.0;
        app.init_resource::<PendingReconciliation>()
            .init_resource::<PhysicsErrors>()
            .add_systems(
                FixedPreUpdate,
                (apply_physics_corrections, maybe_reconcile)
                    .chain()
                    .before(GatherInputSet)
                    .run_if(in_state(state)),
            );
    }
}

/// like BodyState, but stores errors, not state
#[derive(Default)]
struct BodyError {
    pos: Vec3,
    rot: Vec3, // axis * angle
    linvel: Vec3,
    angvel: Vec3,
}
impl BodyError {
    fn is_nearly_zero(&self) -> bool {
        self.pos.length_squared() < 1e-4 && self.linvel.length_squared() < 1e-2
    }
}

/// accumulated per-body physics errors, exponentially drained each tick.
#[derive(Resource, Default)]
pub struct PhysicsErrors(HashMap<NetworkID, BodyError>);

/// Holds the most recent server snapshot waiting to be consumed by `maybe_reconcile`.
/// Set by `on_message` when a `MsgType::State` arrives.
#[derive(Resource, Default)]
pub struct PendingReconciliation(pub Option<SimulationState>);

/// Exponentially drains per-body physics errors by applying a fraction each tick.
/// Alpha = 0.1 → ~90% corrected after ~22 ticks (~0.37 s at 60 Hz).
pub fn apply_physics_corrections(
    mut errors: ResMut<PhysicsErrors>,
    mut world: ResMut<PhysicsWorld>,
    bodies: Query<(
        &NetworkID,
        &RigidBodyHandleComponent,
        Option<&BipedPawnComponent>,
    )>,
) {
    const ALPHA: f32 = 0.2;
    if errors.0.is_empty() {
        return;
    }
    let handles: HashMap<NetworkID, (RigidBodyHandle, bool)> = bodies
        .iter()
        .map(|(nid, h, biped)| (nid.clone(), (h.0, biped.is_some())))
        .collect();
    errors.0.retain(|net_id, error| {
        if error.is_nearly_zero() {
            return false;
        }
        let Some(&(handle, is_biped)) = handles.get(net_id) else {
            return false;
        };
        let Some(rb) = world.rigid_body_set.get_mut(handle) else {
            return false;
        };

        let dp = error.pos * ALPHA;
        let dv = error.linvel * ALPHA;
        let dav = error.angvel * ALPHA;
        let dr = error.rot * ALPHA;

        let t = rb_pos(rb) + dp;
        rb.set_translation(Vector::new(t.x, t.y, t.z), true);

        // Biped rotation is owned by orient_bipeds_to_planets — skip rotation/angvel corrections
        // to prevent stale server look_yaw from leaking into the camera.
        if !is_biped {
            let ang = dr.length();
            if ang > 1e-6 {
                let cur = rb_rot(rb);
                let delta = Quat::from_axis_angle(dr / ang, ang);
                rb.set_rotation(delta * cur, true);
            }
            let av = rb_angvel(rb) + dav;
            rb.set_angvel(Vector::new(av.x, av.y, av.z), true);
        }

        let v = rb_vel(rb) + dv;
        rb.set_linvel(Vector::new(v.x, v.y, v.z), true);

        let keep = 1.0 - ALPHA;
        error.pos *= keep;
        error.rot *= keep;
        error.linvel *= keep;
        error.angvel *= keep;

        true
    });
}

pub fn maybe_reconcile(
    mut pending: ResMut<PendingReconciliation>,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    _net_stats: Res<NetworkStats>,
    bodies: Query<(&NetworkID, &RigidBodyHandleComponent, Option<&Possessed>)>,
    bipeds: Query<&RigidBodyHandleComponent, With<BipedPawnComponent>>,
    planets: Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
    atmospheres: Query<(&AtmosphereComponent, &Transform, Option<&RigidBodyHandleComponent>)>,
    gravity_scales: Query<&GravityScale>,
    mut errors: ResMut<PhysicsErrors>,
    mut biped_q: Query<&mut BipedPawnComponent, With<Possessed>>,
    mut spaceship_q: Query<&mut SpaceshipPawnComponent, With<Possessed>>,
) {
    let Some(snapshot) = pending.0.take() else {
        return;
    };

    let Some((_, our_handle, possessed)) = bodies
        .iter()
        .find_map(|(nid, h, p)| p.map(|poss| (nid, h, poss)))
    else {
        return;
    };

    let pairs: Vec<(NetworkID, RigidBodyHandle)> = bodies
        .iter()
        .map(|(nid, h, _)| (nid.clone(), h.0))
        .collect();

    let current_state =
        snapshot_bodies(&world, tick.tick, bodies.iter().map(|(nid, h, _)| (nid, h)));

    restore_snapshot(&mut world, &snapshot, &pairs);

    let tracked: HashSet<RigidBodyHandle> = pairs.iter().map(|(_, h)| *h).collect();
    let to_freeze: Vec<RigidBodyHandle> = world
        .rigid_body_set
        .iter()
        .filter(|(h, rb)| !tracked.contains(h) && !rb.is_fixed() && rb.is_enabled())
        .map(|(h, _)| h)
        .collect();
    for &h in &to_freeze {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            rb.set_enabled(false);
        }
    }

    let our_rb = our_handle.0;
    for replay_seq in (snapshot.last_input_seq + 1)..=possessed.latest_input_seq() {
        if let Some(input) = possessed.get_input(replay_seq).cloned() {
            let handle = RigidBodyHandleComponent(our_rb);
            if let Ok(mut b) = biped_q.single_mut() {
                b.apply_input(&mut world, &handle, input);
            } else if let Ok(mut s) = spaceship_q.single_mut() {
                s.apply_input(&mut world, &handle, input);
            }
        }
        apply_wind_resistance_impulses(&mut world, &atmospheres);
        apply_gravity_impulses(&mut world, &planets, &gravity_scales);
        orient_bipeds_to_planets_impulses(&mut world, &bipeds, &planets);
        step_world(&mut world);
    }

    for &h in &to_freeze {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            rb.set_enabled(true);
        }
    }

    let resim_state = snapshot_bodies(&world, tick.tick, bodies.iter().map(|(nid, h, _)| (nid, h)));
    restore_snapshot(&mut world, &current_state, &pairs);

    for (net_id, resim) in &resim_state.bodies {
        let Some(cur) = current_state.bodies.get(net_id) else {
            continue;
        };

        let rot_err = resim.rotation * cur.rotation.conjugate();
        let (axis, angle) = rot_err.to_axis_angle();

        let entry = errors.0.entry(net_id.clone()).or_default();
        entry.pos = resim.position - cur.position;
        entry.rot = axis * angle;
        entry.linvel = resim.linvel - cur.linvel;
        entry.angvel = resim.angvel - cur.angvel;
    }
}
