use bevy::prelude::*;
use common::tick::Ticker;
use game_objects::components::planet::GravitySource;
use game_objects::health::{Health, handle_deaths};
use game_objects::level::{
    LevelBytes, PendingMapScene, SpawnPoint, default_asset_dir, load_level_source,
};
use game_objects::lifecycle::{pick_spawn_point, spawn_game_object};
use game_objects::pawn::biped::{BipedPawnComponent, WeaponSlots};
use game_objects::pawn::vehicle::*;
use game_objects::pawn::{
    HeldWeaponMap, ModeConfig, PawnInputKind, PendingRespawns, PlayerRegistry, SeatedInVehicle,
};
use game_objects::weapon::tether::{TetherEndpoint, TetherGunComponent};
use game_objects::*;
use net::message::{
    GameObjectKind, MsgType, NetworkID, NetworkIDResource, SimulationState, SpawnCommand,
};
use net::quic::{Channel, ConnectionId, InboundMessage, QuicManager, SendTarget};
use physics::physics_world::*;
use scripting::{ScriptConfig, get_script_global};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, mpsc};

pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
}

#[derive(Resource)]
pub(crate) struct BindAddr(pub std::net::SocketAddr);

#[derive(Resource)]
pub(crate) struct ConsoleCommands(pub Mutex<mpsc::Receiver<String>>);

#[derive(Resource)]
pub(crate) struct LevelPath(pub String);

/// Ring buffer of per-tick body snapshots used for tick-stamped hit replay.
/// Entries older than 128 ticks are pruned after each broadcast.
#[derive(Resource, Default)]
pub(crate) struct BodyHistory(pub HashMap<u64, SimulationState>);

#[derive(Resource, Default)]
pub(crate) struct PendingInputs(pub HashMap<ConnectionId, (u64, PawnInputKind)>);

#[derive(Resource, Default)]
pub(crate) struct LastProcessedInputSeq(pub HashMap<ConnectionId, u64>);

#[derive(Resource, Default)]
pub struct PendingConnections(pub HashSet<ConnectionId>);

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ServerMessageParams<'w, 's> {
    commands: Commands<'w, 's>,
    world: ResMut<'w, PhysicsWorld>,
    held_weapons: ResMut<'w, HeldWeaponMap>,
    spawn_points: Query<
        'w,
        's,
        (
            Entity,
            &'static SpawnPoint,
            &'static Transform,
            Option<&'static ChildOf>,
        ),
    >,
    parent_transforms: Query<'w, 's, &'static Transform>,
    parent_parents: Query<'w, 's, &'static ChildOf>,
    parent_bodies: Query<'w, 's, &'static RigidBodyHandleComponent>,
    scene_spawn_state: game_objects::level::SceneSpawnState<'w, 's>,
    spawnables: Query<
        'w,
        's,
        (
            &'static NetworkID,
            &'static GameObjectKind,
            &'static RigidBodyHandleComponent,
        ),
    >,
    all_networked: Res<'w, NetworkEntityMap>,
    pawn_slots: Query<'w, 's, &'static mut WeaponSlots>,
    bipeds: Query<'w, 's, &'static mut BipedPawnComponent>,
    vehicles: Query<'w, 's, &'static VehicleComponent>,
    net_ids: Query<'w, 's, &'static NetworkID>,
    seated_bipeds: Query<'w, 's, (&'static NetworkID, &'static SeatedInVehicle)>,
    driver_seats: Query<'w, 's, (&'static mut DriverSeat, &'static Transform)>,
}

impl Plugin for ServerSessionPlugin {
    fn build(&self, app: &mut App) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                if let Ok(line) = line {
                    let _ = cmd_tx.send(line);
                }
            }
        });

        app.insert_resource(BindAddr(self.bind_addr))
            .insert_resource(LevelPath(self.map_path.clone()))
            .insert_resource(ScriptConfig {
                path: self.gametype_path.clone(),
                is_server: true,
                source: None,
            })
            .insert_resource(ConsoleCommands(Mutex::new(cmd_rx)))
            .init_resource::<PlayerRegistry>()
            .init_resource::<PendingRespawns>()
            .init_resource::<PendingConnections>()
            .init_resource::<PendingInputs>()
            .init_resource::<LastProcessedInputSeq>()
            .init_resource::<BodyHistory>()
            .add_systems(
                Update,
                (
                    tick_respawns,
                    process_console_commands,
                    assign_planet_network_ids,
                ),
            )
            .add_systems(
                Startup,
                (load_server_level, start_server, init_mode_config).chain(),
            )
            .add_systems(FixedUpdate, apply_inputs.before(step_physics))
            .add_systems(
                FixedUpdate,
                broadcast_health_updates
                    .after(step_physics)
                    .before(broadcast_tick),
            )
            .add_systems(
                FixedUpdate,
                broadcast_tether_updates
                    .after(step_physics)
                    .before(broadcast_tick),
            )
            .add_systems(FixedUpdate, broadcast_tick.after(handle_deaths));
    }
}

fn tether_endpoint_net(
    endpoint: TetherEndpoint,
    net_ids: &Query<&NetworkID>,
) -> Option<(NetworkID, Vec3)> {
    Some((net_ids.get(endpoint.entity).ok()?.clone(), endpoint.local_anchor))
}

fn broadcast_tether_updates(
    mut quic: ResMut<QuicManager>,
    weapons: Query<(&NetworkID, &TetherGunComponent), Changed<TetherGunComponent>>,
    net_ids: Query<&NetworkID>,
) {
    for (weapon_id, weapon) in &weapons {
        quic.send(
            SendTarget::All,
            Channel::Unordered,
            &MsgType::TetherState {
                weapon: weapon_id.clone(),
                left: weapon.left.and_then(|endpoint| tether_endpoint_net(endpoint, &net_ids)),
                right: weapon.right.and_then(|endpoint| tether_endpoint_net(endpoint, &net_ids)),
            },
        );
    }
}

fn spawn_player(
    conn_id: ConnectionId,
    kind: GameObjectKind,
    spawn_pos: Vec3,
    spawn_rot: Quat,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    net_ids: &mut NetworkIDResource,
    commands: &mut Commands,
    tick: u64,
) {
    let (entity, net_id, spawn_cmd) = spawn_game_object(
        kind,
        spawn_pos,
        spawn_rot,
        Vec3::ZERO,
        tick,
        commands,
        net_ids,
    );

    for &other_conn_id in registry.by_conn.keys() {
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
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::Possess(net_id.clone()),
    );

    registry.insert(conn_id, entity, net_id);
}

fn slots_to_held(slots: &Option<&WeaponSlots>) -> Vec<(NetworkID, Entity)> {
    let Some(slots) = slots else {
        return vec![];
    };
    [&slots.primary, &slots.pocket]
        .iter()
        .filter_map(|(nid, ent)| nid.as_ref().zip(*ent))
        .map(|(nid, ent)| (nid.clone(), ent))
        .collect()
}

fn kill_player(
    entity: Entity,
    net_id: NetworkID,
    held_weapons: Vec<(NetworkID, Entity)>,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    held_weapon_map: &mut HeldWeaponMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let drop_pos = world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos)
        .unwrap_or(Vec3::ZERO);
    for (wid, weapon_entity) in held_weapons {
        held_weapon_map.0.remove(&wid);
        game_objects::weapon::helpers::place_world_weapon(world, weapon_entity, drop_pos);
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos),
        );
    }
    registry.remove_by_entity(entity);
    commands.entity(entity).despawn();
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::DespawnCommand(net_id),
    );
}

fn find_networked_entity(all_networked: &NetworkEntityMap, net_id: &NetworkID) -> Option<Entity> {
    all_networked.get(net_id)
}

fn handle_connected(
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
    spawnables: &Query<(&NetworkID, &GameObjectKind, &RigidBodyHandleComponent)>,
    pawn_slots: &Query<&mut WeaponSlots>,
    entity_net_ids: &Query<&NetworkID>,
    seated_bipeds: &Query<(&NetworkID, &SeatedInVehicle)>,
) -> bool {
    let num_teams = {
        let mut teams = std::collections::HashSet::new();
        for (_, sp, _, _) in spawn_points.iter() {
            teams.insert(sp.team);
        }
        teams.len().max(1)
    };
    let team = (registry.by_conn.len() % num_teams) as u8;
    let Some((sp, sr)) = pick_spawn_point(
        spawn_points,
        parent_transforms,
        parent_parents,
        parent_bodies,
        world,
        team,
        registry.by_conn.len(),
    ) else {
        return false;
    };

    let held_ids: std::collections::HashSet<&NetworkID> = pawn_slots
        .iter()
        .flat_map(|s| [s.primary.0.as_ref(), s.pocket.0.as_ref()])
        .flatten()
        .collect();
    for (net_id, kind, rb) in spawnables.iter() {
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
                rotation: rb_rot(body),
                server_tick: tick,
                kind: kind.clone(),
            }),
        );
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
        sp,
        sr,
        quic,
        registry,
        net_ids,
        commands,
        tick,
    );
    true
}

fn send_connection_files(
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
    if let Some(cfg) = script_config {
        if let Ok(src) = std::fs::read(&cfg.path) {
            quic.send_file(SendTarget::One(conn_id), "gametype.lua".into(), src);
        }
    }
}

fn send_map_file(conn_id: ConnectionId, level_bytes: Option<&LevelBytes>, quic: &mut QuicManager) {
    if let Some(lb) = level_bytes {
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::FileData("map.scn.ron".into(), lb.compressed.clone()),
        );
    }
}

fn handle_disconnected(
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
    if let Some((entity, net_id)) = registry.remove_by_conn(conn_id) {
        common::debug_println!(
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

fn handle_input(
    conn_id: ConnectionId,
    input_seq: u64,
    kind: PawnInputKind,
    pending_inputs: &mut PendingInputs,
) {
    let newest_seen = pending_inputs
        .0
        .get(&conn_id)
        .map(|(seq, _)| *seq)
        .unwrap_or(0);
    if input_seq > newest_seen {
        pending_inputs.0.insert(conn_id, (input_seq, kind));
    }
}

fn handle_flashlight_toggle(
    conn_id: ConnectionId,
    registry: &PlayerRegistry,
    bipeds: &mut Query<&mut BipedPawnComponent>,
    quic: &mut QuicManager,
) {
    let Some((entity, net_id)) = registry.get_character_by_conn(conn_id) else {
        return;
    };
    let Ok(mut biped) = bipeds.get_mut(entity) else {
        return;
    };
    biped.flashlight_on = !biped.flashlight_on;
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::FlashlightState(net_id.clone(), biped.flashlight_on),
    );
}

fn handle_interact(
    conn_id: ConnectionId,
    target_net_id: NetworkID,
    registry: &mut PlayerRegistry,
    all_networked: &NetworkEntityMap,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
) {
    let Some((player_entity, player_net_id)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let Some(target_entity) = find_networked_entity(all_networked, &target_net_id) else {
        return;
    };
    let player_net_id = player_net_id.clone();
    if try_vehicle_interact(
        conn_id,
        player_entity,
        &player_net_id,
        target_entity,
        &target_net_id,
        registry,
        quic,
        world,
        net_ids,
        vehicles,
        driver_seats,
        commands,
    ) {
        return;
    }
    try_weapon_interact(
        player_entity,
        player_net_id,
        target_entity,
        target_net_id,
        quic,
        world,
        held_weapons,
        pawn_slots,
    );
}

fn body_position(world: &PhysicsWorld, entity: Entity) -> Option<Vec3> {
    world
        .entity_to_handle
        .get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos)
}

fn try_vehicle_interact(
    conn_id: ConnectionId,
    player_entity: Entity,
    player_net_id: &NetworkID,
    target_entity: Entity,
    target_net_id: &NetworkID,
    registry: &mut PlayerRegistry,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    driver_seats: &mut Query<(&mut DriverSeat, &Transform)>,
    commands: &mut Commands,
) -> bool {
    let Ok(vehicle) = vehicles.get(target_entity) else {
        return false;
    };
    let Ok((mut cockpit, seat_transform)) = driver_seats.get_mut(vehicle.driver_seat) else {
        return false;
    };

    if cockpit.occupant.is_some() && player_entity == target_entity {
        let Some(biped_entity) = exit_vehicle(world, target_entity, &mut cockpit, seat_transform)
        else {
            return true;
        };
        let Ok(biped_net_id) = net_ids.get(biped_entity) else {
            return true;
        };
        commands.entity(biped_entity).remove::<SeatedInVehicle>();
        registry.set_controlled(conn_id, biped_entity, biped_net_id.clone());
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(biped_net_id.clone(), None),
        );
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::Possess(biped_net_id.clone()),
        );
        return true;
    }

    if cockpit.occupant.is_some()
        || !vehicle_in_range(
            world,
            player_entity,
            target_entity,
            &cockpit,
            seat_transform,
        )
    {
        return true;
    }

    if enter_vehicle(
        world,
        player_entity,
        target_entity,
        &mut cockpit,
        seat_transform,
    ) {
        commands
            .entity(player_entity)
            .insert(SeatedInVehicle(target_entity));
        registry.set_controlled(conn_id, target_entity, target_net_id.clone());
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(player_net_id.clone(), Some(target_net_id.clone())),
        );
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::Possess(target_net_id.clone()),
        );
    }
    true
}

fn vehicle_in_range(
    world: &PhysicsWorld,
    player_entity: Entity,
    target_entity: Entity,
    cockpit: &DriverSeat,
    seat_transform: &Transform,
) -> bool {
    let player_pos = body_position(world, player_entity);
    let seat_pos = world
        .entity_to_handle
        .get(&target_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| seat_world_point(rb_pos(rb), rb_rot(rb), seat_transform.translation));
    matches!((player_pos, seat_pos), (Some(a), Some(b)) if {
        let d = a - b;
        d.x * d.x + d.y * d.y + d.z * d.z
            < (cockpit.interact_radius + 4.0) * (cockpit.interact_radius + 4.0)
    })
}

fn try_weapon_interact(
    player_entity: Entity,
    player_net_id: NetworkID,
    target_entity: Entity,
    target_net_id: NetworkID,
    quic: &mut QuicManager,
    world: &mut PhysicsWorld,
    held_weapons: &mut HeldWeaponMap,
    pawn_slots: &mut Query<&mut WeaponSlots>,
) {
    if held_weapons.0.contains_key(&target_net_id) {
        return;
    }
    let player_pos = body_position(world, player_entity);
    let weapon_pos = body_position(world, target_entity);
    if !matches!((player_pos, weapon_pos), (Some(pp), Some(wp)) if pp.distance(wp) < 2.0) {
        return;
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    if slots.is_full() {
        let drop_pos = player_pos.unwrap_or(Vec3::ZERO);
        let Some((drop_id, drop_entity)) =
            game_objects::weapon::helpers::drop_active_slot(&mut slots)
        else {
            return;
        };
        drop(slots);
        held_weapons.0.remove(&drop_id);
        game_objects::weapon::helpers::place_world_weapon(world, drop_entity, drop_pos);
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::WeaponDrop(drop_id, player_net_id.clone(), drop_pos),
        );
        let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
            return;
        };
        let _ = game_objects::weapon::helpers::assign_pickup_slot(
            &mut slots,
            target_net_id.clone(),
            target_entity,
        );
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    } else {
        let _ = game_objects::weapon::helpers::assign_pickup_slot(
            &mut slots,
            target_net_id.clone(),
            target_entity,
        );
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    }
    game_objects::weapon::helpers::pickup_world_weapon(world, target_entity);
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::WeaponPickup(target_net_id, player_net_id),
    );
}

fn handle_fire_request(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    kind: GameObjectKind,
    temp_id: u32,
    origin: Vec3,
    dir: Vec3,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
    quic: &mut QuicManager,
    tick: u64,
) {
    let Some((shooter_entity, _)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| {
            s.primary.0.as_ref() == Some(&weapon_net_id)
                || s.pocket.0.as_ref() == Some(&weapon_net_id)
        })
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = find_networked_entity(all_networked, &weapon_net_id) else {
        return;
    };
    let Some(fired) = projectile::fire_authoritative(
        kind,
        origin,
        dir,
        shooter_entity,
        tick,
        weapon_entity,
        temp_id,
        commands,
        world,
        net_ids,
    ) else {
        return;
    };
    quic.send(
        SendTarget::AllExcept(conn_id),
        Channel::Unordered,
        &MsgType::SpawnCommand(fired.spawn_cmd),
    );
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::ProjectileConfirm {
            temp_id,
            net_id: fired.net_id,
        },
    );
}

pub(crate) fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_connections: ResMut<PendingConnections>,
    mut pending_inputs: ResMut<PendingInputs>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut sp: ServerMessageParams<'_, '_>,
    tick: Res<Ticker>,
    level_bytes: Option<Res<LevelBytes>>,
) {
    flush_pending_connections(
        &mut quic,
        &mut registry,
        &mut pending_connections,
        &mut net_ids,
        &mut sp,
        tick.tick,
    );

    while let Some(msg) = quic.inbound.pop_front() {
        process_server_message(
            msg.conn_id,
            msg.msg,
            level_bytes.as_deref(),
            script_config.as_deref(),
            &mut quic,
            &mut registry,
            &mut pending_connections,
            &mut pending_inputs,
            &mut pending_respawns,
            &mut net_ids,
            &mut sp,
            tick.tick,
        );
    }
}

fn flush_pending_connections(
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    net_ids: &mut NetworkIDResource,
    sp: &mut ServerMessageParams<'_, '_>,
    tick: u64,
) {
    let pending_conn_ids: Vec<_> = pending_connections.0.iter().copied().collect();
    for conn_id in pending_conn_ids {
        if registry.by_conn.contains_key(&conn_id) {
            pending_connections.0.remove(&conn_id);
            continue;
        }
        if !sp.scene_spawn_state.ready() {
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
            &sp.pawn_slots,
            &sp.net_ids,
            &sp.seated_bipeds,
        ) {
            pending_connections.0.remove(&conn_id);
        }
    }
}

fn process_server_message(
    conn_id: ConnectionId,
    msg: MsgType,
    level_bytes: Option<&LevelBytes>,
    script_config: Option<&ScriptConfig>,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    pending_connections: &mut PendingConnections,
    pending_inputs: &mut PendingInputs,
    pending_respawns: &mut PendingRespawns,
    net_ids: &mut NetworkIDResource,
    sp: &mut ServerMessageParams<'_, '_>,
    tick: u64,
) {
    match msg {
        MsgType::Connected => send_connection_files(conn_id, level_bytes, script_config, quic),
        MsgType::RequestMap => send_map_file(conn_id, level_bytes, quic),
        MsgType::ClientReady => {
            pending_connections.0.insert(conn_id);
        }
        MsgType::Disconnected => {
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
        MsgType::FlashlightToggle => {
            handle_flashlight_toggle(conn_id, registry, &mut sp.bipeds, quic);
        }
        MsgType::Interact(target_net_id) => {
            handle_interact(
                conn_id,
                target_net_id,
                registry,
                &sp.all_networked,
                quic,
                &mut sp.world,
                &mut sp.held_weapons,
                &mut sp.pawn_slots,
                &sp.net_ids,
                &sp.vehicles,
                &mut sp.driver_seats,
                &mut sp.commands,
            );
        }
        MsgType::FireRequest {
            weapon: weapon_net_id,
            kind,
            temp_id,
            origin,
            dir,
        } => handle_fire_request(
            conn_id,
            weapon_net_id,
            kind,
            temp_id,
            origin,
            dir,
            registry,
            &sp.all_networked,
            &sp.pawn_slots,
            &mut sp.commands,
            &mut sp.world,
            net_ids,
            quic,
            tick,
        ),
        MsgType::TimePing(bits) => {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Unreliable,
                &MsgType::TimePong(bits),
            );
        }
        MsgType::Ping(text) => {
            common::debug_println!("Got a ping from conn_id {:?} with text {}", conn_id, text);
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::Pong(text),
            );
        }
        MsgType::ChatMessage(sender, text) => {
            common::debug_println!("GameServer: Got ChatMessage: [{sender}] {text}");
            quic.send(
                SendTarget::All,
                Channel::Ordered,
                &MsgType::ChatMessage(sender, text),
            );
        }
        other => common::debug_println!("Unhandled: {other:?}"),
    }
}

fn start_server(mut quic: ResMut<QuicManager>, addr: Res<BindAddr>) {
    quic.start_server(addr.0);
}

fn load_server_level(mut commands: Commands, level_path: Res<LevelPath>) {
    let asset_path = &level_path.0;
    match load_level_source(asset_path, default_asset_dir()) {
        Ok(level) => {
            commands.insert_resource(PendingMapScene(level.compressed.clone()));
            commands.insert_resource(level);
        }
        Err(err) => {
            game_objects::messages::push(&mut commands, err);
        }
    }
}

fn assign_planet_network_ids(
    query: Query<
        Entity,
        (
            With<RigidBodyHandleComponent>,
            With<GravitySource>,
            Without<NetworkID>,
        ),
    >,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
) {
    for entity in query.iter() {
        commands.entity(entity).insert(NetworkID(net_ids.next()));
    }
}

fn init_mode_config(world: &mut World) {
    let respawn_delay = get_script_global::<f64>(world, "RESPAWN_DELAY")
        .map(|d| d as f32)
        .unwrap_or(common::config::RESPAWN_DELAY_SECS);
    world.insert_resource(ModeConfig { respawn_delay });
}

fn tick_respawns(
    mut pending: ResMut<PendingRespawns>,
    time: Res<Time>,
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    tick: Res<Ticker>,
    spawn_points: Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
) {
    let dt = time.delta_secs();
    let ready: Vec<(ConnectionId, GameObjectKind)> = pending
        .0
        .iter_mut()
        .filter_map(|(&id, (t, k))| {
            *t -= dt;
            (*t <= 0.0).then(|| (id, k.clone()))
        })
        .collect();
    for (conn_id, kind) in ready {
        pending.0.remove(&conn_id);
        let Some((sp, sr)) = pick_spawn_point(
            &spawn_points,
            &parent_transforms,
            &parent_parents,
            &parent_bodies,
            &physics,
            0,
            registry.by_conn.len(),
        ) else {
            continue;
        };
        spawn_player(
            conn_id,
            kind,
            sp,
            sr,
            &mut quic,
            &mut registry,
            &mut net_ids,
            &mut commands,
            tick.tick,
        );
    }
}

fn process_console_commands(
    cmds: Res<ConsoleCommands>,
    mut quic: ResMut<QuicManager>,
    registry: Res<PlayerRegistry>,
) {
    while let Ok(line) = cmds.0.lock().unwrap().try_recv() {
        let mut parts = line.trim().splitn(2, ' ');
        match parts.next().unwrap_or("") {
            "shutdown" | "quit" => {
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::Disconnected);
                std::process::exit(0);
            }
            "kick" => {
                if let Some(id) = parts.next().and_then(|s| s.parse::<ConnectionId>().ok()) {
                    quic.send(
                        SendTarget::One(id),
                        Channel::Ordered,
                        &MsgType::Disconnected,
                    );
                    quic.inbound.push_back(InboundMessage {
                        conn_id: id,
                        channel: Channel::Ordered,
                        msg: MsgType::Disconnected,
                    });
                } else {
                    println!("Usage: kick <conn_id>");
                }
            }
            "say" => {
                let text = parts.next().unwrap_or("").to_string();
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::ChatMessage("[Server]".into(), text.clone()),
                );
                println!("[Server] {text}");
            }
            "status" => {
                println!("{} player(s) connected:", registry.by_conn.len());
                for (conn_id, (entity, net_id)) in &registry.by_conn {
                    println!("  conn={conn_id} entity={entity:?} net_id={net_id:?}");
                }
            }
            "" => {}
            other => println!(
                "Unknown command: {other}. Commands: shutdown, kick <id>, say <text>, status"
            ),
        }
    }
}

fn broadcast_health_updates(
    mut quic: ResMut<QuicManager>,
    health_q: Query<(&Health, &NetworkID), Changed<Health>>,
) {
    for (health, net_id) in health_q.iter() {
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::HealthUpdate(net_id.clone(), health.current),
        );
    }
}

fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &RigidBodyHandleComponent)>,
    registry: Res<PlayerRegistry>,
    last_input_seq: Res<LastProcessedInputSeq>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    for &conn_id in registry.by_conn.keys() {
        let mut state_for_client = state.clone();
        state_for_client.last_input_seq = *last_input_seq.0.get(&conn_id).unwrap_or(&0);
        quic.send(
            SendTarget::One(conn_id),
            Channel::Unreliable,
            &MsgType::State(state_for_client),
        );
    }
}

fn apply_inputs(
    pending_inputs: Res<PendingInputs>,
    mut last_input_seq: ResMut<LastProcessedInputSeq>,
    registry: Res<PlayerRegistry>,
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<&mut game_objects::pawn::biped::BipedPawnComponent>,
    mut spaceships: Query<&mut game_objects::pawn::spaceship::SpaceshipPawnComponent>,
) {
    for (&conn_id, (input_seq, kind)) in pending_inputs.0.iter() {
        let Some((entity, _)) = registry.get_by_conn(conn_id) else {
            continue;
        };
        if game_objects::pawn::apply_server_input(
            entity,
            kind.clone(),
            &mut world,
            &mut bipeds,
            &mut spaceships,
        ) {
            last_input_seq.0.insert(conn_id, *input_seq);
        }
    }
}
