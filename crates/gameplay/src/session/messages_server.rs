use bevy::{ecs::system::SystemState, prelude::*};
use common::tick::Ticker;
use crate::{
    NetworkEntityMap,
    level::{LevelBytes, SpawnPoint},
    pawn::{
        Controller, HeldWeaponMap, PendingRespawns, PlayerRegistry, VehicleComponent, WeaponSlots,
        biped_ability::AbilityPickup,
    },
    weapon::{WeaponConfig, WeaponState},
};
use crate::net::{message::*, quic::*};
use physics::physics_world::*;
use crate::scripting::ScriptConfig;

use crate::session::{connections::*, resources::*};

pub fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut active_connections: ResMut<ActiveConnections>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_connections: ResMut<PendingConnections>,
    mut pending_inputs: ResMut<PendingInputs>,
    mut pending_melee_hits: ResMut<PendingMeleeHits>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut world_params: (Commands, ResMut<PhysicsWorld>, ResMut<HeldWeaponMap>),
    mut gameplay_params: (
        Res<NetworkEntityMap>,
        Query<&mut WeaponSlots>,
        Query<&VehicleComponent>,
        Query<&crate::interaction::Interactable>,
        Query<&NetworkID>,
        Query<(&NetworkID, &crate::pawn::Mounted)>,
        Query<&mut crate::pawn::CharacterMount>,
        Query<&Transform>,
        Query<(&mut WeaponState, &WeaponConfig)>,
        Query<&AbilityPickup>,
        Query<(Entity, &Controller)>,
    ),
    level_bytes: Option<Res<LevelBytes>>,
) {
    let (ref mut commands, ref mut world, ref mut held_weapons) = world_params;
    let (
        ref all_networked,
        ref mut pawn_slots,
        ref vehicles,
        ref interactables,
        ref net_id_q,
        ref mounted_bipeds,
        ref mut mounts,
        ref mount_anchor_transforms,
        ref mut weapon_runtime,
        ref ability_pickups,
        ref controllers,
    ) = gameplay_params;
    crate::session::helpers::drain_inbound(&mut quic, |msg, quic| {
        if matches!(msg.msg, MsgType::Disconnected) {
            handle_server_disconnect(
                msg.conn_id,
                &mut active_connections,
                &mut registry,
                &mut pending_connections,
                &mut pending_inputs,
                &mut pending_melee_hits,
                &mut pending_respawns,
                commands,
                world,
                held_weapons,
                pawn_slots,
                mounted_bipeds,
                mounts,
                quic,
            );
            return;
        }
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
            commands,
            world,
            held_weapons,
            all_networked,
            pawn_slots,
            vehicles,
            interactables,
            net_id_q,
            mounts,
            mount_anchor_transforms,
            weapon_runtime,
            ability_pickups,
            controllers,
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
            (Commands<'_, '_>, ResMut<PhysicsWorld>),
            (
                Query<
                    '_,
                    '_,
                    (
                        Entity,
                        &'static SpawnPoint,
                        &'static Transform,
                        Option<&'static ChildOf>,
                    ),
                >,
                Query<'_, '_, &'static Transform>,
                Query<'_, '_, &'static ChildOf>,
                Query<'_, '_, &'static RigidBodyHandleComponent>,
                Option<Res<'_, crate::level::PendingMapScene>>,
                Query<'_, '_, (), With<crate::level::LevelSceneRoot>>,
                Query<
                    '_,
                    '_,
                    (),
                    (
                        With<crate::level::Spawner>,
                        Without<crate::level::SpawnerRuntime>,
                    ),
                >,
                Query<
                    '_,
                    '_,
                    &'static SceneRigidBody,
                    (With<RigidBodyHandleComponent>, Without<NetworkID>),
                >,
                Query<
                    '_,
                    '_,
                    (
                        Entity,
                        &'static NetworkID,
                        &'static crate::SpawnReplicated,
                        Option<&'static RigidBodyHandleComponent>,
                        Option<&'static ChildOf>,
                        Option<&'static Transform>,
                    ),
                >,
                Query<'_, '_, &'static mut WeaponSlots>,
                Query<'_, '_, &'static NetworkID>,
                Query<'_, '_, (&'static NetworkID, &'static crate::pawn::Mounted)>,
            ),
            Res<Ticker>,
        )> = SystemState::new(world);
        let (
            mut quic,
            mut registry,
            mut pending_connections,
            mut net_ids,
            (mut commands, physics),
            (
                spawn_points,
                parent_transforms,
                parent_parents,
                parent_bodies,
                pending_map,
                roots,
                pending_markers,
                scene_bodies,
                spawnables,
                pawn_slots,
                net_id_q,
                mounted_bipeds,
            ),
            tick,
        ) = state.get_mut(world);
        let pending_conn_ids: Vec<_> = pending_connections.0.iter().copied().collect();
        for conn_id in pending_conn_ids {
            if registry.character(conn_id).is_some() {
                pending_connections.0.remove(&conn_id);
                continue;
            }
            if let Some(reason) = level_ready_reason(
                pending_map.as_deref(),
                &roots,
                &pending_markers,
                &scene_bodies,
            ) {
                eprintln!("pending conn {conn_id}: server world not ready: {reason}");
                continue;
            }
            let Some(entity_snapshots) = handle_connected(
                conn_id,
                &mut quic,
                &mut registry,
                &mut net_ids,
                &mut commands,
                &physics,
                tick.tick,
                &spawn_points,
                &parent_transforms,
                &parent_parents,
                &parent_bodies,
                &spawnables,
                &pawn_slots,
                &net_id_q,
                &mounted_bipeds,
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
    let snapshots = snapshots
        .into_iter()
        .map(|(conn_id, entity, net_id)| {
            let entity = world.entity(entity);
            (
                conn_id,
                net_id,
                entity.get::<common::WeaponState>().copied(),
                entity
                    .get::<crate::health::Health>()
                    .map(|health| health.pool),
            )
        })
        .collect::<Vec<_>>();
    let mut quic = world.resource_mut::<QuicManager>();
    for (conn_id, net_id, weapon_state, health) in snapshots {
        if let Some(weapon_state) = weapon_state {
            crate::weapon::send_weapon_state(
                &mut quic,
                SendTarget::One(conn_id),
                &net_id,
                weapon_state,
            );
        }
        if let Some(health) = health {
            crate::health::send_health(&mut quic, SendTarget::One(conn_id), &net_id, health);
        }
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
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    held_weapons: &mut HeldWeaponMap,
    all_networked: &NetworkEntityMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    vehicles: &Query<&VehicleComponent>,
    interactables: &Query<&crate::interaction::Interactable>,
    net_id_q: &Query<&NetworkID>,
    mounts: &mut Query<&mut crate::pawn::CharacterMount>,
    mount_anchor_transforms: &Query<&Transform>,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    ability_pickups: &Query<&AbilityPickup>,
    controllers: &Query<(Entity, &Controller)>,
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
        MsgType::Disconnected => {}
        MsgType::Input(input_seq, kind) => {
            if !kind.is_valid() {
                return;
            }
            pending_inputs
                .0
                .entry(conn_id)
                .or_default()
                .push(input_seq, kind);
        }
        MsgType::MeleeHitRequest(target_net_id) => {
            pending_melee_hits.0.insert(conn_id, target_net_id);
        }
        MsgType::Interact(target_net_id) => {
            handle_interact(
                conn_id,
                target_net_id,
                registry,
                pending_inputs,
                all_networked,
                quic,
                world,
                weapon_runtime,
                held_weapons,
                pawn_slots,
                net_id_q,
                vehicles,
                interactables,
                mounts,
                mount_anchor_transforms,
                commands,
                ability_pickups,
                controllers,
            );
        }
        MsgType::DropWeapon(drop_dir) => crate::weapon::handle_drop_request(
            conn_id,
            registry,
            pawn_slots,
            held_weapons,
            world,
            weapon_runtime,
            commands,
            quic,
            drop_dir,
        ),
        MsgType::DropAbility(drop_dir) => crate::pawn::biped_ability::handle_drop_request(
            conn_id, registry, commands, drop_dir,
        ),
        MsgType::SetActiveWeaponSlot(active_primary) => {
            crate::weapon::handle_set_active_slot_request(
                conn_id,
                active_primary,
                registry,
                pawn_slots,
                weapon_runtime,
                quic,
            )
        }
        MsgType::StartBeamCharge(_)
        | MsgType::StartBeam { .. }
        | MsgType::BeamHitReport { .. }
        | MsgType::EndBeam(_) => {}
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

fn handle_server_disconnect(
    conn_id: ConnectionId,
    active_connections: &mut ActiveConnections,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    pending_inputs: &mut PendingInputs,
    pending_melee_hits: &mut PendingMeleeHits,
    pending_respawns: &mut PendingRespawns,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    mounted_bipeds: &Query<(&NetworkID, &crate::pawn::Mounted)>,
    mounts: &mut Query<&mut crate::pawn::CharacterMount>,
    quic: &mut QuicManager,
) {
    active_connections.0.remove(&conn_id);
    pending_connections.0.remove(&conn_id);
    pending_inputs.0.remove(&conn_id);
    pending_melee_hits.0.remove(&conn_id);
    handle_disconnected(
        conn_id,
        pending_respawns,
        registry,
        pawn_slots,
        held_weapons,
        quic,
        commands,
        world,
        mounted_bipeds,
        mounts,
    );
}

fn level_ready_reason(
    pending_map: Option<&crate::level::PendingMapScene>,
    roots: &Query<(), With<crate::level::LevelSceneRoot>>,
    pending_markers: &Query<
        (),
        (
            With<crate::level::Spawner>,
            Without<crate::level::SpawnerRuntime>,
        ),
    >,
    scene_bodies: &Query<&SceneRigidBody, (With<RigidBodyHandleComponent>, Without<NetworkID>)>,
) -> Option<&'static str> {
    if pending_map.is_some() {
        Some("map pending")
    } else if roots.is_empty() {
        Some("level root missing")
    } else if !pending_markers.is_empty() {
        Some("scene spawners initializing")
    } else if scene_bodies
        .iter()
        .any(|scene_body| matches!(scene_body, SceneRigidBody::Dynamic))
    {
        Some("scene network ids pending")
    } else {
        None
    }
}

fn handle_interact(
    conn_id: ConnectionId,
    target_net_id: NetworkID,
    registry: &mut PlayerRegistry,
    pending_inputs: &PendingInputs,
    all_networked: &NetworkEntityMap,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    weapon_runtime: &mut Query<(&mut WeaponState, &WeaponConfig)>,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    interactables: &Query<&crate::interaction::Interactable>,
    mounts: &mut Query<&mut crate::pawn::CharacterMount>,
    mount_anchor_transforms: &Query<&Transform>,
    commands: &mut Commands,
    ability_pickups: &Query<&AbilityPickup>,
    controllers: &Query<(Entity, &Controller)>,
) {
    let Some((controlled, _)) = controllers
        .iter()
        .find(|(_, controller)| controller.client == Some(conn_id))
    else {
        return;
    };
    let Some((character, character_net_id)) = registry.character(conn_id) else {
        return;
    };
    let character_net_id = character_net_id.clone();
    let Some(target) = all_networked.get(&target_net_id) else {
        return;
    };
    let aim_dir = pending_inputs
        .0
        .get(&conn_id)
        .and_then(|pending| pending.held.as_ref())
        .and_then(|input| crate::pawn::aim_dir(world, character, Some(input)))
        .unwrap_or_else(|| {
            world
                .body(character)
                .map(|rb| rb_rot(rb) * Vec3::NEG_Z)
                .unwrap_or(Vec3::NEG_Z)
        });

    if vehicles.contains(target) || mounts.contains(target) {
        crate::pawn::mount::handle_server_interact(
            conn_id,
            controlled,
            character,
            &character_net_id,
            target,
            &target_net_id,
            registry,
            quic,
            world,
            net_ids,
            mounts,
            mount_anchor_transforms,
            commands,
        );
        return;
    }

    if crate::pawn::biped_ability::interact_pickup(
        character,
        target,
        world,
        interactables,
        ability_pickups,
        commands,
    ) {
        return;
    }

    crate::weapon::handle_interact_pickup_request(
        character,
        character_net_id,
        target,
        target_net_id,
        quic,
        world,
        weapon_runtime,
        held_weapons,
        pawn_slots,
        commands,
        interactables,
        aim_dir,
    );
}

pub fn apply_melee_hit_requests(
    mut pending_melee_hits: ResMut<PendingMeleeHits>,
    registry: Res<PlayerRegistry>,
    networked: Res<NetworkEntityMap>,
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<&mut crate::pawn::BipedPawnComponent>,
    mut health_q: Query<&mut crate::health::Health>,
    mut last_damage_q: Query<&mut crate::health::LastDamageSource>,
) {
    let requests = std::mem::take(&mut pending_melee_hits.0);
    for (conn_id, target_net_id) in requests {
        let Some((attacker, _)) = registry.character(conn_id) else {
            continue;
        };
        let Some(target) = networked.get(&target_net_id) else {
            continue;
        };
        let Ok(mut biped) = bipeds.get_mut(attacker) else {
            continue;
        };
        if biped.melee_debug_ticks == 0 {
            continue;
        }
        let start = biped.melee_debug_start;
        let end = biped.melee_debug_end;
        if !crate::pawn::biped::validate_melee_target(&mut world, attacker, target, start, end) {
            continue;
        }
        let impulse = crate::pawn::biped::melee_impulse(start, end);
        world.apply_game_impulse(attacker, -impulse, None, None);
        if let Ok(mut health) = health_q.get_mut(target) {
            crate::health::attribute_damage(
                &mut last_damage_q,
                target,
                Some(attacker),
                crate::health::DamageCause::Unknown,
            );
            health.apply_damage(crate::pawn::biped::MELEE_DAMAGE);
        }
        world.apply_game_impulse(target, impulse, None, None);
        biped.melee_debug_ticks = 0;
    }
}
