use bevy::prelude::*;
use gameplay::{
    level::{LevelBytes, SpawnPoint},
    pawn::{HeldWeaponMap, Mounted, PendingRespawns, PlayerRegistry, WeaponSlots},
};
use net::{message::*, quic::*};
use physics::physics_world::*;
use scripting::ScriptConfig;

use crate::replication::{kill_player, slots_to_held, spawn_player};

fn send_existing_spawnable(
    conn_id: ConnectionId,
    net_id: &NetworkID,
    spawn_name: &str,
    rb: Option<&RigidBodyHandleComponent>,
    child_of: Option<&ChildOf>,
    transform: Option<&Transform>,
    entity_net_ids: &Query<&NetworkID>,
    quic: &mut QuicManager,
    world: &PhysicsWorld,
    tick: u64,
) {
    let Some(spawn_cmd) = (|| {
        let (parent_net_id, position, velocity, rotation, angular_velocity) = if let Some(rb) = rb {
            let body = world.rigid_body_set.get(rb.0)?;
            (
                None,
                rb_pos(body),
                rb_vel(body),
                rb_rot(body),
                rb_angvel(body),
            )
        } else {
            let transform = transform?;
            let parent_net_id =
                child_of.and_then(|child_of| entity_net_ids.get(child_of.parent()).ok().cloned());
            (
                parent_net_id,
                transform.translation,
                Vec3::ZERO,
                transform.rotation,
                Vec3::ZERO,
            )
        };
        let mut cmd = SpawnCommand::new(net_id.clone(), spawn_name, tick)
            .position(position)
            .rotation(rotation)
            .velocity(velocity)
            .angular_velocity(angular_velocity);
        if let Some(parent_net_id) = parent_net_id {
            cmd = cmd.parent(parent_net_id);
        }
        Some(cmd)
    })() else {
        return;
    };
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::SpawnCommand(spawn_cmd),
    );
}

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
    spawnables: &Query<(
        Entity,
        &NetworkID,
        &gameplay::SpawnReplicated,
        Option<&RigidBodyHandleComponent>,
        Option<&ChildOf>,
        Option<&Transform>,
    )>,
    pawn_slots: &Query<&mut WeaponSlots>,
    entity_net_ids: &Query<&NetworkID>,
    mounted_bipeds: &Query<(&NetworkID, &Mounted)>,
) -> Option<Vec<(Entity, NetworkID)>> {
    let mut teams = spawn_points
        .iter()
        .map(|(_, sp, _, _)| sp.team)
        .collect::<Vec<_>>();
    teams.sort();
    teams.dedup();
    let team = teams
        .get(registry.controlled_count() % teams.len().max(1))
        .copied()
        .unwrap_or(0);
    let Some((sp, sr, sv)) = gameplay::lifecycle::pick_spawn_point_with_velocity(
        spawn_points,
        parent_transforms,
        parent_parents,
        parent_bodies,
        world,
        team,
        registry.controlled_count(),
    ) else {
        eprintln!("conn {conn_id}: server ready but no spawn point resolved");
        return None;
    };

    let held_ids: std::collections::HashSet<&NetworkID> = pawn_slots
        .iter()
        .flat_map(|s| s.slots.iter().map(|slot| slot.0.as_ref()))
        .flatten()
        .collect();
    let mut snapshots = Vec::new();
    for child_pass in [false, true] {
        for (entity, net_id, spawn_rep, rb, child_of, transform) in spawnables.iter() {
            if held_ids.contains(net_id) {
                continue;
            }
            if child_of.is_some() != child_pass {
                continue;
            }
            send_existing_spawnable(
                conn_id,
                net_id,
                spawn_rep.0,
                rb,
                child_of,
                transform,
                entity_net_ids,
                quic,
                world,
                tick,
            );
            snapshots.push((entity, net_id.clone()));
        }
    }
    for (biped_net_id, mounted) in mounted_bipeds.iter() {
        let Ok(parent_net_id) = entity_net_ids.get(mounted.0) else {
            continue;
        };
        gameplay::pawn::send_mount_state(
            quic,
            SendTarget::One(conn_id),
            biped_net_id,
            Some(parent_net_id),
        );
    }
    spawn_player(
        conn_id,
        "biped",
        gameplay::Team(team),
        sp,
        sr,
        sv,
        quic,
        registry,
        net_ids,
        commands,
        tick,
    );
    Some(snapshots)
}

pub(super) fn send_connection_files(
    conn_id: ConnectionId,
    level_bytes: Option<&LevelBytes>,
    script_config: Option<&ScriptConfig>,
    quic: &mut QuicManager,
) {
    if let Some(lb) = level_bytes {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::MapHash(lb.hash.clone()),
        );
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
        eprintln!(
            "GameServer: Player disconnected: entity={entity} conn={:?}",
            conn_id
        );
        let held = slots_to_held(&pawn_slots.get(entity).ok());
        kill_player(
            entity,
            net_id,
            held,
            quic,
            registry,
            held_weapons,
            commands,
            world,
        );
    }
}
