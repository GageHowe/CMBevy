use bevy::prelude::*;
use common::tick::Ticker;
use physics::physics_world::*;

use crate::{
    lifecycle::spawn_game_object,
    mode::ModeConfig,
    net::{message::*, quic::*},
    pawn::{Controller, PlayerRegistry},
    session::resources::*,
    *,
};

pub(super) fn spawn_player(
    conn_id: ConnectionId,
    spawn_name: &str,
    team: Team,
    spawn_pos: Vec3,
    spawn_rot: Quat,
    spawn_vel: Vec3,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    net_ids: &mut NetworkIDResource,
    commands: &mut Commands,
    tick: u64,
) {
    let kind_debug = spawn_name.to_string();
    let (entity, net_id, spawn_cmd) = spawn_game_object(
        spawn_name,
        Some(spawn_pos),
        Some(spawn_rot),
        Some(spawn_vel),
        None,
        tick,
        commands,
        net_ids,
    );
    commands
        .entity(entity)
        .insert((team, Controller::for_client(conn_id)));

    for other_conn_id in registry.controlled_conn_ids() {
        quic.send(
            SendTarget::One(other_conn_id),
            Channel::Ordered,
            &MsgType::SpawnCommand(spawn_cmd.clone()),
        );
    }
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::SpawnCommand(spawn_cmd),
    );
    registry.register_character(conn_id, entity, net_id.clone());
    crate::pawn::send_possess(quic, conn_id, &net_id);
    eprintln!("GameServer: spawned {kind_debug} for conn {conn_id}");
}

/// Broadcasts the authoritative physics snapshot and any dirty pawn look state for the current tick.
pub fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &RigidBodyHandleComponent)>,
    mut biped_looks: Query<(&NetworkID, &mut crate::pawn::biped::BipedPawnComponent)>,
    registry: Res<PlayerRegistry>,
    last_input_seq: Res<LastProcessedInputSeq>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    for conn_id in registry.controlled_conn_ids() {
        let mut state_for_client = state.clone();
        state_for_client.last_input_seq = *last_input_seq.0.get(&conn_id).unwrap_or(&0);
        quic.send(
            SendTarget::One(conn_id),
            Channel::Unreliable,
            &MsgType::State(State(state_for_client)),
        );
    }
    crate::pawn::broadcast_dirty_look_updates(&mut quic, &mut biped_looks);
}
