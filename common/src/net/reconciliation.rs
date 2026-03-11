use bevy::prelude::*;
use rapier3d::prelude::RigidBodyHandle;

use crate::net::message::{NetworkID, SimulationState};
use crate::physics::physics_world::{PhysicsBodyHandle, PhysicsWorld, restore_snapshot, snapshot_bodies};
use crate::game_objects::pawn::biped;
use crate::game_objects::pawn::pawn::{gather_pawn_input, BipedPawnComponent, Possessed};
use crate::ring_buffer::RingBuffer;
use crate::tick::Ticker;

pub const RECONCILE_POS_THRESHOLD: f32 = 0.2;
pub const RECONCILE_VEL_THRESHOLD: f32 = 1.0;

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

pub struct ReconciliationPlugin<S: States + Copy>(pub S);

impl<S: States + Copy> Plugin for ReconciliationPlugin<S> {
    fn build(&self, app: &mut App) {
        let state = self.0;
        app.init_resource::<PendingReconciliation>()
            .init_resource::<LocalStateHistory>()
            .add_systems(FixedPreUpdate, maybe_reconcile.before(gather_pawn_input).run_if(in_state(state)))
            .add_systems(FixedPostUpdate, record_world_state.run_if(in_state(state)));
    }
}

/// Runs after `step_physics`. Snapshots all networked bodies into the local history so
/// `maybe_reconcile` can compare and restore them against the authoritative server state.
pub fn record_world_state(
    world: Res<PhysicsWorld>,
    tick: Res<Ticker>,
    mut history: ResMut<LocalStateHistory>,
    query: Query<(&NetworkID, &PhysicsBodyHandle)>,
) {
    history.0.push(snapshot_bodies(&world, tick.tick, query.iter()));
}

/// Runs at the start of `FixedPreUpdate`, before input is gathered.
///
/// If a server snapshot is pending:
///   1. Compare it against our locally predicted state at that tick.
///   2. If the error exceeds threshold, restore physics state and replay all buffered
///      inputs from `snapshot_tick+1` up to (not including) the current tick.
///      Bodies in the server snapshot are restored to server-authoritative state.
///      Networked bodies the server didn't mention are restored to local predicted state.
pub fn maybe_reconcile(
    mut pending: ResMut<PendingReconciliation>,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    history: Res<LocalStateHistory>,
    bodies: Query<(&NetworkID, &PhysicsBodyHandle, Option<&Possessed>)>,
) {
    let Some(snapshot) = pending.0.take() else { return };

    let Some((our_net_id, our_handle, possessed)) =
        bodies.iter().find_map(|(nid, h, p)| p.map(|poss| (nid, h, poss)))
    else {
        return;
    };

    let local_at_tick = history.0.iter().find(|s| s.tick == snapshot.tick);

    let needs_reconcile = match (
        local_at_tick.and_then(|s| s.bodies.get(our_net_id)),
        snapshot.bodies.get(our_net_id),
    ) {
        (Some(predicted), Some(server)) => {
            let pos_err = (server.position - predicted.position).length();
            let vel_err = (server.linvel - predicted.linvel).length();
            pos_err > RECONCILE_POS_THRESHOLD || vel_err > RECONCILE_VEL_THRESHOLD
        }
        // No history for this tick — always reconcile to stay correct.
        _ => true,
    };

    if !needs_reconcile {
        return;
    }

    let pairs: Vec<(NetworkID, RigidBodyHandle)> = bodies
        .iter()
        .map(|(nid, h, _)| (nid.clone(), h.0))
        .collect();

    restore_snapshot(&mut world, &snapshot, &pairs);

    if let Some(local) = local_at_tick {
        let unmentioned: Vec<(NetworkID, RigidBodyHandle)> = pairs.iter()
            .filter(|(nid, _)| !snapshot.bodies.contains_key(nid))
            .cloned()
            .collect();
        restore_snapshot(&mut world, local, &unmentioned);
    }

    let current = tick.tick;
    for replay_tick in (snapshot.tick + 1)..current {
        if let Some(&input) = possessed.get_input(replay_tick) {
            // TODO: generalise when Possessed can apply to non-biped pawns
            biped::apply_biped_movement(&mut world, our_handle, input, &mut BipedPawnComponent);
        }
        world.step();
    }
}
