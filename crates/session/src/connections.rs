use bevy::prelude::*;
use game_objects::{
    level::{LevelBytes, SpawnPoint},
    pawn::{HeldWeaponMap, PendingRespawns, PlayerRegistry, SeatedInVehicle, WeaponSlots},
    weapon::{WeaponConfig, WeaponState},
};
use net::{message::*, quic::*};
use physics::physics_world::*;
use scripting::ScriptConfig;

use crate::{
    replication::{kill_player, slots_to_held, spawn_player},
    resources::*,
};

pub(super) fn handle_connected(
    conn_id: ConnectionId,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    net_ids: &mut NetworkIDResource,
    commands: &mut Commands,
    world: &PhysicsWorld,
    tick: u64,
    spawn_points: &Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    spawnables: &Query<(Entity, &NetworkID, &GameObjectKind, &RigidBodyHandleComponent)>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    pawn_slots: &Query<&mut WeaponSlots>,
    entity_net_ids: &Query<&NetworkID>,
    seated_bipeds: &Query<(&NetworkID, &SeatedInVehicle)>,
) -> bool {
    let mut teams = spawn_points.iter().map(|(_, sp, _, _)| sp.team).collect::<Vec<_>>();
    teams.sort();
    teams.dedup();
    let team = teams.get(registry.controlled_count() % teams.len().max(1)).copied().unwrap_or(0);
    let Some((sp, sr, sv)) = game_objects::lifecycle::pick_spawn_point_with_velocity(
        spawn_points,
        parent_transforms,
        parent_parents,
        parent_bodies,
        world,
        team,
        registry.controlled_count(),
    ) else {
        warn!("conn {conn_id}: server ready but no spawn point resolved");
        return false;
    };

    let held_ids: std::collections::HashSet<&NetworkID> = pawn_slots
        .iter()
        .flat_map(|s| s.slots.iter().map(|slot| slot.0.as_ref()))
        .flatten()
        .collect();
    for (entity, net_id, kind, rb) in spawnables.iter() {
        if held_ids.contains(net_id) {
            continue;
        }
        let Some(body) = world.rigid_body_set.get(rb.0) else {
            continue;
        };
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::SpawnCommand(SpawnCommand {
                net_id: net_id.clone(),
                position: rb_pos(body),
                starting_velocity: rb_vel(body),
                shooter_velocity: Vec3::ZERO,
                rotation: rb_rot(body),
                server_tick: tick,
                kind: kind.clone(),
            }),
        );
        if game_objects::weapon::is_weapon_kind(kind)
            && let Ok((state, _)) = weapon_runtime.get_mut(entity)
        {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::WeaponState(net_id.clone(), *state),
            );
        }
    }
    for (biped_net_id, seated_in) in seated_bipeds.iter() {
        let Ok(vehicle_net_id) = entity_net_ids.get(seated_in.0) else {
            continue;
        };
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::SeatState(biped_net_id.clone(), Some(vehicle_net_id.clone())),
        );
    }
    spawn_player(
        conn_id,
        GameObjectKind::Biped,
        game_objects::Team(team),
        sp,
        sr,
        sv,
        quic,
        registry,
        net_ids,
        commands,
        tick,
    );
    true
}

pub(super) fn send_connection_files(
    conn_id: ConnectionId,
    level_bytes: Option<&LevelBytes>,
    script_config: Option<&ScriptConfig>,
    quic: &mut QuicManager,
) {
    if let Some(lb) = level_bytes {
        quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::MapHash(lb.hash.clone()));
    }
    if let Some(cfg) = script_config
        && let Ok(src) = std::fs::read(&cfg.path)
    {
        quic.send_file(SendTarget::One(conn_id), "gametype.lua".into(), src);
    }
}

pub(super) fn send_map_file(
    conn_id: ConnectionId,
    level_bytes: Option<&LevelBytes>,
    quic: &mut QuicManager,
) {
    if let Some(lb) = level_bytes {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::FileData("map.scn.ron".into(), lb.compressed.clone()),
        );
    }
}

pub(super) fn handle_disconnected(
    conn_id: ConnectionId,
    pending_respawns: &mut PendingRespawns,
    registry: &mut PlayerRegistry,
    pawn_slots: &Query<&mut WeaponSlots>,
    held_weapons: &mut HeldWeaponMap,
    quic: &mut QuicManager,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    pending_respawns.0.remove(&conn_id);
    if let Some((entity, net_id)) = registry.remove_character_for_conn(conn_id) {
        info!("GameServer: Player disconnected: entity={entity} conn={:?}", conn_id);
        let held = slots_to_held(&pawn_slots.get(entity).ok());
        kill_player(entity, net_id, held, quic, registry, held_weapons, commands, world);
    }
}

pub(super) fn flush_pending_connections(
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    net_ids: &mut NetworkIDResource,
    sp: &mut ServerMessageParams<'_, '_>,
    tick: u64,
) {
    let pending_conn_ids: Vec<_> = pending_connections.0.iter().copied().collect();
    for conn_id in pending_conn_ids {
        if registry.controlled_pawn(conn_id).is_some() {
            pending_connections.0.remove(&conn_id);
            continue;
        }
        if let Some(reason) = sp.level_ready.reason() {
            info!("pending conn {conn_id}: server world not ready: {reason}");
            continue;
        }
        if handle_connected(
            conn_id,
            quic,
            registry,
            net_ids,
            &mut sp.commands,
            &sp.world,
            tick,
            &sp.spawn_points,
            &sp.parent_transforms,
            &sp.parent_parents,
            &sp.parent_bodies,
            &sp.spawnables,
            &mut sp.weapon_runtime,
            &sp.pawn_slots,
            &sp.net_ids,
            &sp.seated_bipeds,
        ) {
            pending_connections.0.remove(&conn_id);
        }
    }
}
