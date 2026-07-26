use std::collections::{HashMap, HashSet};

use bevy::{ecs::system::SystemParam, prelude::*};
use common::{
    LocalControl, NetworkID, PredictedImpulse, PredictedImpulses,
    game_state::SimulationSystems,
    tick::{NetworkStats, Ticker},
};
use gameplay::{
    NetworkEntityMap,
    components::{
        gravity::{GravitySource, apply_gravity_impulses},
        snap::{SnapSource, orient_bipeds_to_snap_sources_impulses},
    },
    pawn::{GatherInputSet, Mounted, Possessed, biped::BipedPawnComponent},
    session::PendingReconciliation,
};
use physics::physics_world::{
    GravityScale, PhysicsWorld, RigidBodyHandleComponent, rb_angvel, rb_pos, rb_rot, rb_vel,
    restore_snapshot, snapshot_body_handles,
};
use rapier3d::prelude::{RigidBodyHandle, Vector};

#[derive(Clone, Copy)]
struct BipedReplayState {
    jump_cooldown: u8,
    is_sliding: bool,
}
/// manages client-side rollback/correction
pub struct ReconciliationPlugin<S: States + Copy>(pub S);
impl<S: States + Copy> ReconciliationPlugin<S> {
    pub fn new(state: S) -> Self {
        Self(state)
    }
}

impl<S: States + Copy> Plugin for ReconciliationPlugin<S> {
    fn build(&self, app: &mut App) {
        let state = self.0;
        app.init_resource::<LocalControl>()
            .init_resource::<PredictedImpulses>()
            .init_resource::<ReplayStateHistory>()
            .init_resource::<PhysicsErrors>()
            .add_systems(
                FixedPreUpdate,
                (apply_physics_corrections, maybe_reconcile)
                    .chain()
                    .in_set(SimulationSystems)
                    .before(GatherInputSet)
                    .run_if(in_state(state)),
            )
            .add_systems(
                FixedPostUpdate,
                record_biped_state
                    .in_set(SimulationSystems)
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

#[derive(Resource, Default)]
pub struct ReplayStateHistory(HashMap<u64, ReplayState>);

#[derive(Clone, Default)]
struct ReplayState {
    biped: Option<BipedReplayState>,
    ability: Option<gameplay::pawn::biped_ability::EquippedAbility>,
}

#[derive(SystemParam)]
struct ReplayPhysicsEnv<'w, 's> {
    gravity_sources: Query<'w, 's, (&'static GravitySource, &'static RigidBodyHandleComponent)>,
    snap_sources: Query<'w, 's, (&'static SnapSource, &'static RigidBodyHandleComponent)>,
    gravity_scales: Query<'w, 's, &'static GravityScale>,
}

fn record_biped_state(
    bipeds: Query<&BipedPawnComponent, With<Possessed>>,
    control: Res<LocalControl>,
    mut history: ResMut<ReplayStateHistory>,
) {
    let seq = control.latest_seq();
    if seq == 0 {
        return;
    }
    let biped = bipeds.single().ok();
    history.0.insert(
        seq,
        ReplayState {
            biped: biped.map(|biped| BipedReplayState {
                jump_cooldown: biped.jump_cooldown,
                is_sliding: biped.is_sliding,
            }),
            ability: biped.and_then(|biped| biped.ability.clone()),
        },
    );
    history.0.retain(|&old_seq, _| old_seq + 128 >= seq);
}

/// Drains per-body physics errors each tick.
/// Position/rotation lerp smoothly to avoid visible snapping.
/// Velocity/angular velocity are corrected instantly so subsequent physics steps
/// simulate on the right trajectory rather than compounding drift.
pub fn apply_physics_corrections(
    mut errors: ResMut<PhysicsErrors>,
    mut world: ResMut<PhysicsWorld>,
    networked: Res<NetworkEntityMap>,
) {
    // ~90% corrected after ~10 ticks at 64Hz ≈ 0.16s
    const POS_ALPHA: f32 = 0.2;

    if errors.0.is_empty() {
        return;
    }
    errors.0.retain(|net_id, error| {
        if error.is_nearly_zero() {
            return false;
        }
        let Some(handle) = networked.get_body(net_id) else {
            return false;
        };
        let Some(rb) = world.rigid_body_set.get_mut(handle) else {
            return false;
        };

        // Smooth positional correction.
        let dp = error.pos * POS_ALPHA;
        let t = rb_pos(rb) + dp;
        rb.set_translation(Vector::new(t.x, t.y, t.z), true);

        let dr = error.rot * POS_ALPHA;
        let ang = dr.length();
        if ang > 1e-6 {
            let cur = rb_rot(rb);
            let delta = Quat::from_axis_angle(dr / ang, ang);
            rb.set_rotation(delta * cur, true);
        }

        // Velocity is corrected instantly: a wrong velocity compounds into position error on
        // every subsequent tick, so lerping it just makes the position correction larger.
        let v = rb_vel(rb) + error.linvel;
        rb.set_linvel(Vector::new(v.x, v.y, v.z), true);
        let av = rb_angvel(rb) + error.angvel;
        rb.set_angvel(Vector::new(av.x, av.y, av.z), true);

        error.pos *= 1.0 - POS_ALPHA;
        error.rot *= 1.0 - POS_ALPHA;
        error.linvel = Vec3::ZERO;
        error.angvel = Vec3::ZERO;

        true
    });
}

fn maybe_reconcile(
    mut pending: ResMut<PendingReconciliation>,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    _net_stats: Res<NetworkStats>,
    networked: Res<NetworkEntityMap>,
    possessed: Query<&NetworkID, With<Possessed>>,
    bipeds: Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<Mounted>)>,
    vehicles: Query<&gameplay::pawn::vehicle::VehicleComponent, With<Possessed>>,
    seated: Query<&Mounted>,
    env: ReplayPhysicsEnv,
    control: Res<LocalControl>,
    impulses: Res<PredictedImpulses>,
    history: Res<ReplayStateHistory>,
    mut errors: ResMut<PhysicsErrors>,
    mut possessed_bipeds: Query<&mut BipedPawnComponent, With<Possessed>>,
) {
    let Some(snapshot) = pending.0.take() else {
        return;
    };

    let Ok(our_net_id) = possessed.single() else {
        return;
    };
    let Some(our_handle) = networked.get_body(our_net_id) else {
        return;
    };

    let pairs = networked.body_pairs_vec();

    let current_state =
        snapshot_body_handles(&world, tick.tick, pairs.iter().map(|(nid, h)| (nid, *h)));
    let current_biped_state = possessed_bipeds
        .single()
        .ok()
        .map(|biped| BipedReplayState {
            jump_cooldown: biped.jump_cooldown,
            is_sliding: biped.is_sliding,
        });
    let current_ability = possessed_bipeds
        .single()
        .ok()
        .and_then(|biped| biped.ability.clone());
    let possessed_entity = networked.get_entity(our_net_id);

    restore_snapshot(&mut world, &snapshot, &pairs);
    if let (Some(Some(saved)), Ok(mut biped)) = (
        history
            .0
            .get(&snapshot.last_input_seq)
            .map(|saved| saved.biped),
        possessed_bipeds.single_mut(),
    ) {
        biped.jump_cooldown = saved.jump_cooldown;
        biped.is_sliding = saved.is_sliding;
    }
    if let (Some(saved), Ok(mut biped)) = (
        history.0.get(&snapshot.last_input_seq),
        possessed_bipeds.single_mut(),
    ) {
        biped.ability = saved.ability.clone();
    }

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

    let our_rb = our_handle;
    let replay_handles: HashMap<NetworkID, RigidBodyHandle> = pairs
        .iter()
        .map(|(net_id, handle)| (net_id.clone(), *handle))
        .collect();
    for replay_seq in (snapshot.last_input_seq + 1)..=control.latest_seq() {
        if let Some(input) = control.get(replay_seq) {
            let handle = RigidBodyHandleComponent(our_rb);
            if let Some(owner_entity) = possessed_entity {
                if let Ok(mut biped) = possessed_bipeds.single_mut() {
                    gameplay::pawn::biped::apply_biped_input(
                        &mut world,
                        owner_entity,
                        input.clone(),
                        &handle,
                        &mut biped,
                    );
                    let _ = gameplay::pawn::biped_ability::apply_input(
                        owner_entity,
                        input.clone(),
                        &mut world,
                        &mut biped,
                    );
                }
                if let Ok(vehicle) = vehicles.get(owner_entity) {
                    (vehicle.apply_input)(&mut world, owner_entity, input.clone());
                }
            }
            for impulse in impulses.get(replay_seq).into_iter().flatten().cloned() {
                apply_predicted_impulse(&mut world, &replay_handles, our_net_id, our_rb, impulse);
            }
        }
        apply_gravity_impulses(
            &mut world,
            &env.gravity_sources,
            &env.gravity_scales,
            &seated,
        );
        orient_bipeds_to_snap_sources_impulses(&mut world, &bipeds, &env.snap_sources);
        world.step();
    }

    for &h in &to_freeze {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            rb.set_enabled(true);
        }
    }

    let resim_state =
        snapshot_body_handles(&world, tick.tick, pairs.iter().map(|(nid, h)| (nid, *h)));
    restore_snapshot(&mut world, &current_state, &pairs);
    if let (Some(saved), Ok(mut biped)) = (current_biped_state, possessed_bipeds.single_mut()) {
        biped.jump_cooldown = saved.jump_cooldown;
        biped.is_sliding = saved.is_sliding;
    }
    if let Ok(mut biped) = possessed_bipeds.single_mut() {
        biped.ability = current_ability;
    }

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

fn apply_predicted_impulse(
    world: &mut PhysicsWorld,
    replay_handles: &HashMap<NetworkID, RigidBodyHandle>,
    our_net_id: &NetworkID,
    our_rb: RigidBodyHandle,
    impulse: PredictedImpulse,
) {
    let handle = if &impulse.target == our_net_id {
        Some(our_rb)
    } else {
        replay_handles.get(&impulse.target).copied()
    };
    let Some(handle) = handle else {
        return;
    };
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        let vec = Vector::new(impulse.impulse.x, impulse.impulse.y, impulse.impulse.z);
        if let Some(point) = impulse.point {
            rb.apply_impulse_at_point(vec, point, true);
        } else {
            rb.apply_impulse(vec, true);
        }
    }
}
