use bevy::prelude::*;
use game_objects::{
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
    spawn_type: SpawnType,
    rb: Option<&RigidBodyHandleComponent>,
    child_of: Option<&ChildOf>,
    transform: Option<&Transform>,
    entity_net_ids: &Query<&NetworkID>,
    quic: &mut QuicManager,
    world: &PhysicsWorld,
    tick: u64,
) {
    let Some(spawn_cmd) = (|| {
        let (parent_net_id, position, starting_velocity, rotation) = if let Some(rb) = rb {
            let body = world.rigid_body_set.get(rb.0)?;
            (None, rb_pos(body), rb_vel(body), rb_rot(body))
        } else {
            let transform = transform?;
            let parent_net_id =
                child_of.and_then(|child_of| entity_net_ids.get(child_of.parent()).ok().cloned());
            (parent_net_id, transform.translation, Vec3::ZERO, transform.rotation)
        };
        Some(game_objects::lifecycle::make_spawn_command(
            net_id.clone(),
            spawn_type,
            parent_net_id,
            position,
            starting_velocity,
            Vec3::ZERO,
            rotation,
            tick,
        ))
    })() else {
        return;
    };
    game_objects::lifecycle::send_spawn_command(
        quic,
        SendTarget::One(conn_id),
        Channel::Ordered,
        spawn_cmd,
    );
}

fn spawn_type_for_entity(
    entity: Entity,
    biped_spawnables: &Query<(), With<game_objects::pawn::BipedPawnComponent>>,
    spaceship_spawnables: &Query<(), With<game_objects::pawn::SpaceshipPawnComponent>>,
    fighter_spawnables: &Query<(), With<game_objects::pawn::FighterPawnComponent>>,
    truck_spawnables: &Query<(), With<game_objects::pawn::TruckPawnComponent>>,
    hovercraft_spawnables: &Query<(), With<game_objects::pawn::HovercraftPawnComponent>>,
    shield_spawnables: &Query<(), With<game_objects::shield::Shield>>,
    pistol_spawnables: &Query<(), With<game_objects::weapon::pistol::PistolComponent>>,
    beamer_spawnables: &Query<(), With<game_objects::weapon::beamer::BeamerComponent>>,
    rifle_spawnables: &Query<(), With<game_objects::weapon::rifle::RifleComponent>>,
    smg_spawnables: &Query<(), With<game_objects::weapon::smg::SmgComponent>>,
    hail_mary_spawnables: &Query<(), With<game_objects::weapon::hail_mary::HailMaryComponent>>,
    thumper_spawnables: &Query<(), With<game_objects::weapon::thumper::ThumperComponent>>,
    lobber_spawnables: &Query<(), With<game_objects::weapon::lobber::LobberComponent>>,
    coil_launcher_spawnables: &Query<(), With<game_objects::weapon::coil_launcher::CoilLauncherComponent>>,
    interaction_names: &Query<&game_objects::interaction::InteractionName>,
) -> Option<SpawnType> {
    if biped_spawnables.contains(entity) {
        return Some(SpawnType::Biped);
    }
    if spaceship_spawnables.contains(entity) {
        return Some(SpawnType::Spaceship);
    }
    if fighter_spawnables.contains(entity) {
        return Some(SpawnType::Fighter);
    }
    if truck_spawnables.contains(entity) {
        return Some(SpawnType::Truck);
    }
    if hovercraft_spawnables.contains(entity) {
        return Some(SpawnType::Hovercraft);
    }
    if shield_spawnables.contains(entity) {
        return Some(SpawnType::SpaceshipShield);
    }
    if pistol_spawnables.contains(entity) {
        return Some(SpawnType::Pistol);
    }
    if beamer_spawnables.contains(entity) {
        return Some(SpawnType::Beamer);
    }
    if rifle_spawnables.contains(entity) {
        return Some(SpawnType::Rifle);
    }
    if smg_spawnables.contains(entity) {
        return Some(SpawnType::Smg);
    }
    if hail_mary_spawnables.contains(entity) {
        return Some(SpawnType::HailMary);
    }
    if thumper_spawnables.contains(entity) {
        return Some(SpawnType::Thumper);
    }
    if lobber_spawnables.contains(entity) {
        return Some(SpawnType::Lobber);
    }
    if coil_launcher_spawnables.contains(entity) {
        return Some(SpawnType::CoilLauncher);
    }
    match interaction_names.get(entity).ok().map(|name| name.0) {
        Some("Jetpack") => Some(SpawnType::Jetpack),
        Some("Dash") => Some(SpawnType::Dash),
        _ => None,
    }
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
        Option<&RigidBodyHandleComponent>,
        Option<&ChildOf>,
        Option<&Transform>,
    )>,
    pawn_slots: &Query<&mut WeaponSlots>,
    entity_net_ids: &Query<&NetworkID>,
    mounted_bipeds: &Query<(&NetworkID, &Mounted)>,
    biped_spawnables: &Query<(), With<game_objects::pawn::BipedPawnComponent>>,
    spaceship_spawnables: &Query<(), With<game_objects::pawn::SpaceshipPawnComponent>>,
    fighter_spawnables: &Query<(), With<game_objects::pawn::FighterPawnComponent>>,
    truck_spawnables: &Query<(), With<game_objects::pawn::TruckPawnComponent>>,
    hovercraft_spawnables: &Query<(), With<game_objects::pawn::HovercraftPawnComponent>>,
    shield_spawnables: &Query<(), With<game_objects::shield::Shield>>,
    pistol_spawnables: &Query<(), With<game_objects::weapon::pistol::PistolComponent>>,
    beamer_spawnables: &Query<(), With<game_objects::weapon::beamer::BeamerComponent>>,
    rifle_spawnables: &Query<(), With<game_objects::weapon::rifle::RifleComponent>>,
    smg_spawnables: &Query<(), With<game_objects::weapon::smg::SmgComponent>>,
    hail_mary_spawnables: &Query<(), With<game_objects::weapon::hail_mary::HailMaryComponent>>,
    thumper_spawnables: &Query<(), With<game_objects::weapon::thumper::ThumperComponent>>,
    lobber_spawnables: &Query<(), With<game_objects::weapon::lobber::LobberComponent>>,
    coil_launcher_spawnables: &Query<(), With<game_objects::weapon::coil_launcher::CoilLauncherComponent>>,
    interaction_names: &Query<&game_objects::interaction::InteractionName>,
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
    let Some((sp, sr, sv)) = game_objects::lifecycle::pick_spawn_point_with_velocity(
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
        for (entity, net_id, rb, child_of, transform) in spawnables.iter() {
            if held_ids.contains(net_id) {
                continue;
            }
            if child_of.is_some() != child_pass {
                continue;
            }
            let Some(spawn_type) = spawn_type_for_entity(
                entity,
                biped_spawnables,
                spaceship_spawnables,
                fighter_spawnables,
                truck_spawnables,
                hovercraft_spawnables,
                shield_spawnables,
                pistol_spawnables,
                beamer_spawnables,
                rifle_spawnables,
                smg_spawnables,
                hail_mary_spawnables,
                thumper_spawnables,
                lobber_spawnables,
                coil_launcher_spawnables,
                interaction_names,
            ) else {
                continue;
            };
            send_existing_spawnable(
                conn_id,
                net_id,
                spawn_type,
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
        game_objects::pawn::send_mount_state(
            quic,
            SendTarget::One(conn_id),
            biped_net_id,
            Some(parent_net_id),
        );
    }
    spawn_player(
        conn_id,
        SpawnType::Biped,
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
