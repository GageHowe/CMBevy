use std::collections::{HashMap, HashSet};

use bevy::{ecs::system::SystemState, prelude::*};
use common::{LocalControl, NetworkID, PredictedImpulse, PredictedImpulses};
use gameplay::{
    NetworkEntityMap,
    components::{
        gravity::{GravitySource, apply_gravity_impulses},
        snap::{SnapSource, orient_bipeds_to_snap_sources_impulses},
    },
    pawn::{Controller, GatherInputSet, Mounted, PawnInputApplier, biped::BipedPawnComponent},
    session::PendingReconciliation,
};
use physics::physics_world::{
    GravityScale, PhysicsWorld, RigidBodyHandleComponent, restore_snapshot,
};
use rapier3d::prelude::{RigidBodyHandle, Vector};

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
            .add_systems(OnExit(state), reset_prediction)
            .add_systems(
                FixedPreUpdate,
                reconcile
                    .in_set(common::game_state::SimulationSystems)
                    .before(GatherInputSet)
                    .run_if(in_state(state)),
            );
    }
}

fn reset_prediction(mut control: ResMut<LocalControl>, mut impulses: ResMut<PredictedImpulses>) {
    *control = LocalControl::default();
    *impulses = PredictedImpulses::default();
}

fn reconcile(world: &mut World) {
    let Some(snapshot) = world
        .resource_mut::<PendingReconciliation>()
        .snapshot
        .take()
    else {
        return;
    };
    let mut controlled = world.query_filtered::<&NetworkID, With<Controller>>();
    let Ok(our_net_id) = controlled.single(world).cloned() else {
        return;
    };
    let (pairs, our_entity) = {
        let networked = world.resource::<NetworkEntityMap>();
        let Some(our_entity) = networked.get_entity(&our_net_id) else {
            return;
        };
        (networked.body_pairs_vec(), our_entity)
    };

    let (latest_seq, first_seq) = {
        let control = world.resource::<LocalControl>();
        (control.latest_seq(), control.first_seq())
    };
    let replay_start = snapshot.last_input_seq + 1;
    if replay_start <= latest_seq && first_seq.is_none_or(|first| replay_start < first) {
        return;
    }

    restore_snapshot(
        world.resource_mut::<PhysicsWorld>().as_mut(),
        &snapshot,
        &pairs,
    );

    let frozen = freeze_untracked(world, &pairs);
    let replay_handles = pairs.iter().cloned().collect::<HashMap<_, _>>();
    for seq in replay_start..=latest_seq {
        let Some(input) = world.resource::<LocalControl>().get(seq).cloned() else {
            continue;
        };
        if let Some(applier) = world.get::<PawnInputApplier>(our_entity).copied() {
            applier.apply(world, our_entity, input);
        }
        let impulses = world
            .resource::<PredictedImpulses>()
            .get(seq)
            .map(<[_]>::to_vec);
        if let Some(impulses) = impulses {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            for impulse in impulses {
                apply_predicted_impulse(&mut physics, &replay_handles, impulse);
            }
        }
        step_replay(world);
    }
    unfreeze(world, &frozen);
}

fn freeze_untracked(
    world: &mut World,
    pairs: &[(NetworkID, RigidBodyHandle)],
) -> Vec<RigidBodyHandle> {
    let tracked = pairs
        .iter()
        .map(|(_, handle)| *handle)
        .collect::<HashSet<_>>();
    let mut physics = world.resource_mut::<PhysicsWorld>();
    let frozen = physics
        .rigid_body_set
        .iter()
        .filter(|(handle, body)| !tracked.contains(handle) && !body.is_fixed() && body.is_enabled())
        .map(|(handle, _)| handle)
        .collect::<Vec<_>>();
    for handle in &frozen {
        if let Some(body) = physics.rigid_body_set.get_mut(*handle) {
            body.set_enabled(false);
        }
    }
    frozen
}

fn unfreeze(world: &mut World, frozen: &[RigidBodyHandle]) {
    let mut physics = world.resource_mut::<PhysicsWorld>();
    for handle in frozen {
        if let Some(body) = physics.rigid_body_set.get_mut(*handle) {
            body.set_enabled(true);
        }
    }
}

fn step_replay(world: &mut World) {
    let mut state: SystemState<(
        ResMut<PhysicsWorld>,
        Query<(&GravitySource, &RigidBodyHandleComponent)>,
        Query<&GravityScale>,
        Query<&Mounted>,
        Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<Mounted>)>,
        Query<(&SnapSource, &RigidBodyHandleComponent)>,
    )> = SystemState::new(world);
    let Ok((mut physics, gravity, gravity_scales, seated, bipeds, snap)) = state.get_mut(world)
    else {
        return;
    };
    apply_gravity_impulses(&mut physics, &gravity, &gravity_scales, &seated);
    orient_bipeds_to_snap_sources_impulses(&mut physics, &bipeds, &snap);
    physics.step();
}

fn apply_predicted_impulse(
    world: &mut PhysicsWorld,
    replay_handles: &HashMap<NetworkID, RigidBodyHandle>,
    predicted: PredictedImpulse,
) {
    let Some(handle) = replay_handles.get(&predicted.target).copied() else {
        return;
    };
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        let impulse = Vector::new(
            predicted.impulse.x,
            predicted.impulse.y,
            predicted.impulse.z,
        );
        if let Some(point) = predicted.point {
            rb.apply_impulse_at_point(impulse, point, true);
        } else {
            rb.apply_impulse(impulse, true);
        }
    }
}
