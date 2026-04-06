use bevy::prelude::*;
use rapier3d::prelude::{RigidBodyHandle, Vector};
use std::collections::{HashMap, HashSet};

use common::tick::{NetworkStats, Ticker};
use common::{NetworkID, PredictedCommand, PredictedCommands};
use game_objects::NetworkEntityMap;
use game_objects::components::atmosphere::{
    AtmosphericDragComponent, apply_wind_resistance_impulses,
};
use game_objects::components::planet::{
    PlanetComponent, apply_gravity_impulses, orient_bipeds_to_planets_impulses,
};
use game_objects::pawn::Pawn;
use game_objects::pawn::SeatedInVehicle;
use game_objects::pawn::biped::BipedPawnComponent;
use game_objects::pawn::spaceship::SpaceshipPawnComponent;
use game_objects::pawn::{GatherInputSet, Possessed};
use net::message::SimulationState;
use physics::physics_world::{
    GravityScale, PhysicsWorld, RigidBodyHandleComponent, rb_angvel, rb_pos, rb_rot, rb_vel,
    restore_snapshot, snapshot_body_handles, step_world,
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
            .init_resource::<PredictedCommands>()
            .init_resource::<BipedStateHistory>()
            .init_resource::<PhysicsErrors>()
            .add_systems(
                FixedPreUpdate,
                (apply_physics_corrections, maybe_reconcile)
                    .chain()
                    .before(GatherInputSet)
                    .run_if(in_state(state)),
            )
            .add_systems(FixedPostUpdate, record_biped_state.run_if(in_state(state)));
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

#[derive(Clone, Copy)]
struct BipedReplayState {
    jump_cooldown: u8,
    is_sliding: bool,
}

#[derive(Resource, Default)]
pub struct BipedStateHistory(HashMap<u64, BipedReplayState>);

fn record_biped_state(
    pawns: Query<&BipedPawnComponent, With<Possessed>>,
    predicted: Res<PredictedCommands>,
    mut history: ResMut<BipedStateHistory>,
) {
    let Ok(biped) = pawns.single() else {
        return;
    };
    let seq = predicted.latest_seq();
    if seq == 0 {
        return;
    }
    history.0.insert(
        seq,
        BipedReplayState {
            jump_cooldown: biped.jump_cooldown,
            is_sliding: biped.is_sliding,
        },
    );
    history.0.retain(|&old_seq, _| old_seq + 128 >= seq);
}

/// Exponentially drains per-body physics errors by applying a fraction each tick.
/// Alpha = 0.1 → ~90% corrected after ~22 ticks (~0.37 s at 60 Hz).
pub fn apply_physics_corrections(
    mut errors: ResMut<PhysicsErrors>,
    mut world: ResMut<PhysicsWorld>,
    networked: Res<NetworkEntityMap>,
    bipeds: Query<(), With<BipedPawnComponent>>,
) {
    const ALPHA: f32 = 0.2;
    if errors.0.is_empty() {
        return;
    }
    let handles: HashMap<NetworkID, (RigidBodyHandle, bool)> = networked
        .body_pairs()
        .filter_map(|(net_id, handle)| {
            networked
                .get_entity(net_id)
                .map(|entity| (net_id.clone(), (*handle, bipeds.get(entity).is_ok())))
        })
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
    networked: Res<NetworkEntityMap>,
    possessed: Query<&NetworkID, With<Possessed>>,
    bipeds: Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<SeatedInVehicle>)>,
    seated: Query<&SeatedInVehicle>,
    planets: Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
    atmospheres: Query<(
        &AtmosphericDragComponent,
        &Transform,
        Option<&RigidBodyHandleComponent>,
    )>,
    gravity_scales: Query<&GravityScale>,
    predicted: Res<PredictedCommands>,
    history: Res<BipedStateHistory>,
    mut errors: ResMut<PhysicsErrors>,
    mut biped_q: Query<&mut BipedPawnComponent, With<Possessed>>,
    mut spaceship_q: Query<&mut SpaceshipPawnComponent, With<Possessed>>,
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
    let current_biped_state = biped_q.single().ok().map(|biped| BipedReplayState {
        jump_cooldown: biped.jump_cooldown,
        is_sliding: biped.is_sliding,
    });

    restore_snapshot(&mut world, &snapshot, &pairs);
    if let (Some(saved), Ok(mut biped)) = (
        history.0.get(&snapshot.last_input_seq),
        biped_q.single_mut(),
    ) {
        biped.jump_cooldown = saved.jump_cooldown;
        biped.is_sliding = saved.is_sliding;
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
    for replay_seq in (snapshot.last_input_seq + 1)..=predicted.latest_seq() {
        if let Some(command) = predicted.get(replay_seq).cloned() {
            apply_predicted_command(
                &mut world,
                &pairs,
                our_net_id,
                our_rb,
                command,
                &mut biped_q,
                &mut spaceship_q,
            );
        }
        apply_wind_resistance_impulses(&mut world, &atmospheres, &seated);
        apply_gravity_impulses(&mut world, &planets, &gravity_scales, &seated);
        orient_bipeds_to_planets_impulses(&mut world, &bipeds, &planets);
        step_world(&mut world);
    }

    for &h in &to_freeze {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            rb.set_enabled(true);
        }
    }

    let resim_state =
        snapshot_body_handles(&world, tick.tick, pairs.iter().map(|(nid, h)| (nid, *h)));
    restore_snapshot(&mut world, &current_state, &pairs);
    if let (Some(saved), Ok(mut biped)) = (current_biped_state, biped_q.single_mut()) {
        biped.jump_cooldown = saved.jump_cooldown;
        biped.is_sliding = saved.is_sliding;
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

fn apply_predicted_command(
    world: &mut PhysicsWorld,
    pairs: &[(NetworkID, RigidBodyHandle)],
    our_net_id: &NetworkID,
    our_rb: RigidBodyHandle,
    command: PredictedCommand,
    biped_q: &mut Query<&mut BipedPawnComponent, With<Possessed>>,
    spaceship_q: &mut Query<&mut SpaceshipPawnComponent, With<Possessed>>,
) {
    match command {
        PredictedCommand::Input(input) => {
            let handle = RigidBodyHandleComponent(our_rb);
            if let Ok(mut b) = biped_q.single_mut() {
                b.apply_input(world, &handle, input);
            } else if let Ok(mut s) = spaceship_q.single_mut() {
                s.apply_input(world, &handle, input);
            }
        }
        PredictedCommand::Impulse { target, impulse } => {
            let handle = if &target == our_net_id {
                Some(our_rb)
            } else {
                pairs
                    .iter()
                    .find(|(net_id, _)| *net_id == target)
                    .map(|(_, handle)| *handle)
            };
            let Some(handle) = handle else {
                return;
            };
            if let Some(rb) = world.rigid_body_set.get_mut(handle) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }
}
