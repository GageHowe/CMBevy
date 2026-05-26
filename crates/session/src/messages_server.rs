use bevy::{ecs::system::SystemState, prelude::*};
use common::tick::Ticker;
use game_objects::{
    level::LevelBytes,
    pawn::{PendingRespawns, PlayerRegistry},
};
use net::{message::*, quic::*};
use scripting::ScriptConfig;

use crate::{actions::*, connections::*, resources::*};

pub fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut active_connections: ResMut<ActiveConnections>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_connections: ResMut<PendingConnections>,
    mut pending_inputs: ResMut<PendingInputs>,
    mut pending_melee_hits: ResMut<PendingMeleeHits>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut sp: ServerMessageParams<'_, '_>,
    tick: Res<Ticker>,
    level_bytes: Option<Res<LevelBytes>>,
) {
    crate::helpers::drain_inbound(&mut quic, |msg, quic| {
        process_server_message(
            msg.conn_id,
            msg.msg,
            level_bytes.as_deref(),
            script_config.as_deref(),
            quic,
            &mut active_connections,
            &mut registry,
            &mut pending_connections,
            &mut pending_inputs,
            &mut pending_melee_hits,
            &mut pending_respawns,
            &mut net_ids,
            &mut sp,
            tick.tick,
        );
    });
}

pub fn flush_pending_connections(world: &mut World) {
    let mut snapshots = Vec::new();
    {
        let mut state: SystemState<(
            ResMut<QuicManager>,
            ResMut<PlayerRegistry>,
            ResMut<PendingConnections>,
            ResMut<NetworkIDResource>,
            ServerMessageParams<'_, '_>,
            Res<Ticker>,
        )> = SystemState::new(world);
        let (mut quic, mut registry, mut pending_connections, mut net_ids, mut sp, tick) =
            state.get_mut(world);
        let pending_conn_ids: Vec<_> = pending_connections.0.iter().copied().collect();
        for conn_id in pending_conn_ids {
            if registry.controlled_pawn(conn_id).is_some() {
                pending_connections.0.remove(&conn_id);
                continue;
            }
            if let Some(reason) = sp.level_ready.reason() {
                eprintln!("pending conn {conn_id}: server world not ready: {reason}");
                continue;
            }
            let Some(entity_snapshots) = handle_connected(
                conn_id,
                &mut quic,
                &mut registry,
                &mut net_ids,
                &mut sp.commands,
                &sp.world,
                tick.tick,
                &sp.spawn_points,
                &sp.parent_transforms,
                &sp.parent_parents,
                &sp.parent_bodies,
                &sp.spawnables,
                &sp.pawn_slots,
                &sp.net_ids,
                &sp.mounted_bipeds,
                &sp.biped_spawnables,
                &sp.spaceship_spawnables,
                &sp.fighter_spawnables,
                &sp.truck_spawnables,
                &sp.hovercraft_spawnables,
                &sp.shield_spawnables,
                &sp.pistol_spawnables,
                &sp.beamer_spawnables,
                &sp.rifle_spawnables,
                &sp.smg_spawnables,
                &sp.hail_mary_spawnables,
                &sp.thumper_spawnables,
                &sp.lobber_spawnables,
                &sp.coil_launcher_spawnables,
                &sp.interaction_name_spawnables,
            ) else {
                continue;
            };
            snapshots.extend(
                entity_snapshots
                    .into_iter()
                    .map(|(entity, net_id)| (conn_id, entity, net_id)),
            );
            pending_connections.0.remove(&conn_id);
        }
        state.apply(world);
    }
    if snapshots.is_empty() {
        return;
    }
    let replication = world.resource::<net::replication::ReplicationRegistry>();
    let updates = snapshots
        .into_iter()
        .flat_map(|(conn_id, entity, net_id)| {
            net::replication::collect_entity_component_updates(
                world.entity(entity),
                &net_id,
                &replication,
            )
            .into_iter()
            .map(move |update| (conn_id, update))
        })
        .collect::<Vec<_>>();
    let mut quic = world.resource_mut::<QuicManager>();
    for (conn_id, update) in updates {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::ComponentUpdate(update),
        );
    }
}

fn process_server_message(
    conn_id: ConnectionId,
    msg: MsgType,
    level_bytes: Option<&LevelBytes>,
    script_config: Option<&ScriptConfig>,
    quic: &mut QuicManager,
    active_connections: &mut ActiveConnections,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    pending_inputs: &mut PendingInputs,
    pending_melee_hits: &mut PendingMeleeHits,
    pending_respawns: &mut PendingRespawns,
    net_ids: &mut NetworkIDResource,
    sp: &mut ServerMessageParams<'_, '_>,
    tick: u64,
) {
    match msg {
        MsgType::Connected => {
            active_connections.0.insert(conn_id);
            eprintln!("GameServer: conn {conn_id} connected; sending map metadata");
            send_connection_files(conn_id, level_bytes, script_config, quic);
        }
        MsgType::RequestMap => send_map_file(conn_id, level_bytes, quic),
        MsgType::ClientReady => {
            eprintln!("GameServer: conn {conn_id} sent ClientReady");
            pending_connections.0.insert(conn_id);
        }
        MsgType::Disconnected => {
            active_connections.0.remove(&conn_id);
            handle_disconnected(
                conn_id,
                pending_respawns,
                registry,
                &sp.pawn_slots,
                &mut sp.held_weapons,
                quic,
                &mut sp.commands,
                &mut sp.world,
            );
            pending_connections.0.remove(&conn_id);
        }
        MsgType::Input(input_seq, kind) => handle_input(conn_id, input_seq, kind, pending_inputs),
        MsgType::MeleeHitRequest(target_net_id) => {
            handle_melee_hit_request(conn_id, target_net_id, pending_melee_hits)
        }
        MsgType::Interact(target_net_id) => {
            handle_interact(
                conn_id,
                target_net_id,
                registry,
                pending_inputs,
                &sp.all_networked,
                quic,
                &mut sp.world,
                &mut sp.weapon_runtime,
                &mut sp.held_weapons,
                &mut sp.pawn_slots,
                &sp.net_ids,
                &sp.vehicles,
                &sp.interactables,
                &mut sp.mounts,
                &sp.mount_anchor_transforms,
                &mut sp.commands,
                &sp.on_pickup_q,
            );
        }
        MsgType::DropWeapon(drop_dir) => game_objects::weapon::handle_drop_request(
            conn_id,
            registry,
            &mut sp.pawn_slots,
            &mut sp.held_weapons,
            &mut sp.world,
            &mut sp.weapon_runtime,
            &mut sp.commands,
            quic,
            drop_dir,
        ),
        MsgType::DropAbility(drop_dir) => {
            handle_drop_ability(conn_id, registry, &mut sp.commands, drop_dir);
        }
        MsgType::SetActiveWeaponSlot(active_primary) => {
            game_objects::weapon::handle_set_active_slot_request(
                conn_id,
                active_primary,
                registry,
                &mut sp.pawn_slots,
                &mut sp.weapon_runtime,
                quic,
            )
        }
        MsgType::ReloadWeapon(weapon_net_id) => game_objects::weapon::handle_reload_request(
            conn_id,
            weapon_net_id,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.weapon_runtime,
            quic,
        ),
        MsgType::FireRequest {
            weapon: weapon_net_id,
            temp_id,
            origin,
            dir,
        } => game_objects::weapon::handle_fire_request(
            conn_id,
            weapon_net_id,
            temp_id,
            origin,
            dir,
            registry,
            &sp.all_networked,
            &mut sp.pawn_slots,
            &mut sp.weapon_runtime,
            &mut sp.held_weapons,
            &mut sp.commands,
            &mut sp.world,
            net_ids,
            quic,
        ),
        MsgType::StartBeamCharge(weapon_net_id) => {
            game_objects::weapon::beamer::handle_start_charge_request(
                conn_id,
                weapon_net_id,
                registry,
                &sp.all_networked,
                &sp.pawn_slots,
                &mut sp.beamers,
                &mut sp.weapon_runtime,
                quic,
                tick,
            )
        }
        MsgType::StartBeam {
            weapon: weapon_net_id,
            origin,
            dir,
        } => game_objects::weapon::beamer::handle_start_beam_request(
            conn_id,
            weapon_net_id,
            origin,
            dir,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.beamers,
            quic,
            tick,
        ),
        MsgType::BeamHitReport {
            weapon: weapon_net_id,
            origin,
            dir,
            target,
        } => game_objects::weapon::beamer::handle_beam_hit_report(
            conn_id,
            weapon_net_id,
            origin,
            dir,
            target,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.beamers,
            &mut sp.weapon_runtime,
            &mut sp.health_q,
            &mut sp.last_damage_q,
            quic,
            tick,
        ),
        MsgType::EndBeam(weapon_net_id) => game_objects::weapon::beamer::handle_end_beam_request(
            conn_id,
            weapon_net_id,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.beamers,
            &mut sp.weapon_runtime,
            quic,
        ),
        MsgType::DetonateFailsafeRequest(_) => {}
        MsgType::TimePing(bits) => {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Unreliable,
                &MsgType::TimePong(bits),
            );
        }
        MsgType::Ping(text) => {
            eprintln!("Got a ping from conn_id {:?} with text {}", conn_id, text);
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::Pong(text),
            );
        }
        MsgType::ChatMessage(sender, text) => {
            eprintln!("GameServer: Got ChatMessage: [{sender}] {text}");
            quic.send(
                SendTarget::All,
                Channel::Ordered,
                &MsgType::ChatMessage(sender, text),
            );
        }
        other => eprintln!("Unhandled: {other:?}"),
    }
}
