// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use common::tick::Ticker;
use net::message::PawnInputKind;
use net::{
    message::{
        GameObjectKind, MsgType, NetworkID, NetworkIDResource, SimulationState, SpawnCommand,
    },
    quic::*,
};
use physics::physics_world::*;
use std::collections::HashMap;
use std::net::SocketAddr;
#[derive(Resource)]
pub(crate) struct BindAddr(pub SocketAddr);
use common::debug_println;
use game_objects::level::{
    LevelBytes, LevelPlugin, SceneSpawner, SpawnPoint, scene_spawns_ready,
};
use game_objects::pawn::biped::WeaponSlots;
use game_objects::pawn::biped::SceneBiped;
use game_objects::pawn::vehicle::*;
use game_objects::pawn::spaceship::SceneSpaceship;
use game_objects::pawn::*;
use game_objects::weapon::hail_mary::SceneHailMary;
use game_objects::weapon::rifle::SceneRifle;
use game_objects::weapon::rpg::SceneRpg;
use game_objects::weapon::WeaponPlugin;
use game_objects::*;
use master_plugin::MasterPlugin;
use scripting::ScriptConfig;
use std::sync::{Mutex, mpsc};

mod session;
use session::{PendingConnections, ServerSessionPlugin, pick_spawn_point};

#[derive(Resource)]
pub(crate) struct ConsoleCommands(pub Mutex<mpsc::Receiver<String>>);

#[derive(bevy::ecs::system::SystemParam)]
struct ServerMessageParams<'w, 's> {
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
    pending_scene_bipeds: Query<'w, 's, (), (With<SceneBiped>, Without<SceneSpawner>)>,
    pending_scene_spaceships:
        Query<'w, 's, (), (With<SceneSpaceship>, Without<SceneSpawner>)>,
    pending_scene_rifles: Query<'w, 's, (), (With<SceneRifle>, Without<SceneSpawner>)>,
    pending_scene_hail_marys:
        Query<'w, 's, (), (With<SceneHailMary>, Without<SceneSpawner>)>,
    pending_scene_rpgs: Query<'w, 's, (), (With<SceneRpg>, Without<SceneSpawner>)>,
    scene_spawners: Query<'w, 's, &'static SceneSpawner>,
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
    net_ids: Query<'w, 's, &'static NetworkID>,
    seated_bipeds: Query<'w, 's, (&'static NetworkID, &'static SeatedInVehicle)>,
    cockpits: Query<'w, 's, (&'static mut Cockpit, &'static Transform, &'static ChildOf)>,
}

fn parse_args() -> (SocketAddr, String, String) {
    let mut addr = common::config::SERVER_BIND_ADDRESS.to_string();
    let mut map = "maps/default.scn.ron".to_string(); // asset-relative; load_server_level prepends the asset dir for fs reads
    let mut gametype = "assets/gametypes/default.lua".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                if let Some(p) = args.next().and_then(|p| p.parse::<u16>().ok()) {
                    addr = format!("0.0.0.0:{p}");
                }
            }
            "--map" => {
                if let Some(v) = args.next() {
                    map = v;
                }
            }
            "--gametype" => {
                if let Some(v) = args.next() {
                    gametype = v;
                }
            }
            _ => {}
        }
    }
    (addr.parse().unwrap(), map, gametype)
}

/// Resource holding the path to the level .scn.ron file.
#[derive(Resource)]
pub(crate) struct LevelPath(pub String);

/// Responds to UDP "discover" probes so LAN clients can find this server.
fn start_lan_discovery(quic_port: u16) {
    let port_str = quic_port.to_string();
    std::thread::spawn(move || {
        let Ok(sock) =
            std::net::UdpSocket::bind(format!("0.0.0.0:{}", common::config::LAN_DISCOVERY_PORT))
        else {
            return;
        };
        let mut buf = [0u8; 16];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf) else {
                continue;
            };
            if &buf[..n] == b"discover" {
                let _ = sock.send_to(port_str.as_bytes(), from);
            }
        }
    });
}

fn main() {
    let (bind_addr, map_path, gametype_path) = parse_args();
    println!("binding to {bind_addr}\nmap={map_path}\ngametype={gametype_path}");
    start_lan_discovery(bind_addr.port());

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: if cfg!(debug_assertions) {
                "../assets"
            } else {
                "assets"
            }
            .to_string(),
            ..default()
        })
        .add_plugins(bevy::scene::ScenePlugin) // needed to register DynamicScene asset + RON loader
        .add_plugins(LogPlugin {
            level: Level::ERROR,
            ..default()
        });

    app.insert_resource(common::IsServer);
    app.add_plugins(MasterPlugin);
    app.add_plugins(GameObjectsPlugin);
    app.init_resource::<HeldWeaponMap>();
    app.add_plugins(LevelPlugin);
    app.add_systems(FixedPreUpdate, on_message);
    app.add_systems(
        FixedUpdate,
        (step_physics, sync_physics_to_transforms).chain(),
    );
    app.add_plugins(WeaponPlugin);
    app.add_plugins(ServerSessionPlugin {
        bind_addr,
        map_path,
        gametype_path,
    });
    println!("starting server...\n");
    app.run();
}

/// Ring buffer of per-tick body snapshots used for tick-stamped hit replay.
/// Entries older than 128 ticks are pruned after each broadcast.
#[derive(Resource, Default)]
pub(crate) struct BodyHistory(pub HashMap<u64, SimulationState>);

#[derive(Resource, Default)]
pub(crate) struct PendingInputs(pub HashMap<ConnectionId, (u64, PawnInputKind)>);

#[derive(Resource, Default)]
pub(crate) struct LastProcessedInputSeq(pub HashMap<ConnectionId, u64>);

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
    let net_id = NetworkID(net_ids.next());
    let spawn_cmd = SpawnCommand {
        net_id: net_id.clone(),
        position: spawn_pos.into(),
        starting_velocity: Vec3::ZERO.into(),
        rotation: spawn_rot.into(),
        server_tick: tick,
        kind,
    };
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand {
        entity,
        cmd: spawn_cmd.clone(),
    });

    // Tell all currently connected players (including the new one) about the pawn.
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
    // Tell the owning client to possess this pawn.
    quic.send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::Possess(net_id.clone()),
    );

    registry.insert(conn_id, entity, net_id);
}

/// Logic for when a player leaves or dies.
/// Drops held weapons back into the world and despawns the pawn.
/// Extracts (NetworkID, Entity) pairs for all weapons currently in a pawn's slots.
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
        world.teleport_body(weapon_entity, drop_pos);
        world.set_body_enabled(weapon_entity, true);
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

fn on_message(
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
    let pending_conn_ids: Vec<_> = pending_connections.0.iter().copied().collect();
    for conn_id in pending_conn_ids {
        if registry.by_conn.contains_key(&conn_id) {
            pending_connections.0.remove(&conn_id);
            continue;
        }
        if !scene_spawns_ready(
            &sp.pending_scene_bipeds,
            &sp.pending_scene_spaceships,
            &sp.pending_scene_rifles,
            &sp.pending_scene_hail_marys,
            &sp.pending_scene_rpgs,
            &sp.scene_spawners,
        ) {
            continue;
        }
        if handle_connected(
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
                    &sp.seated_bipeds,
        ) {
            pending_connections.0.remove(&conn_id);
        }
    }

    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                send_connection_files(
                    msg.conn_id,
                    level_bytes.as_deref(),
                    script_config.as_deref(),
                    &mut quic,
                );
            }
            MsgType::ClientReady => {
                pending_connections.0.insert(msg.conn_id);
            }

            // if server receives a Disconnected message...
            MsgType::Disconnected => {
                handle_disconnected(
                    msg.conn_id,
                    &mut pending_respawns,
                    &mut registry,
                    &sp.pawn_slots,
                    &mut sp.held_weapons,
                    &mut quic,
                    &mut sp.commands,
                    &mut sp.world,
                );
                pending_connections.0.remove(&msg.conn_id);
            }
            MsgType::Input(input_seq, kind) => {
                handle_input(msg.conn_id, input_seq, kind, &mut pending_inputs);
            }
            MsgType::FlashlightToggle => {
                handle_flashlight_toggle(msg.conn_id, &registry, &mut sp.bipeds, &mut quic);
            }
            MsgType::Interact(target_net_id) => {
                handle_interact(
                    msg.conn_id,
                    target_net_id,
                    &mut registry,
                    &sp.all_networked,
                    &mut quic,
                    &mut sp.world,
                    &mut sp.held_weapons,
                    &mut sp.pawn_slots,
                    &mut sp.bipeds,
                    &sp.net_ids,
                    &mut sp.cockpits,
                    &mut sp.commands,
                );
            }
            MsgType::FireRequest {
                weapon: weapon_net_id,
                kind,
                temp_id,
                origin,
                dir,
            } => {
                handle_fire_request(
                    msg.conn_id,
                    weapon_net_id,
                    kind,
                    temp_id,
                    origin,
                    dir,
                    &registry,
                    &sp.pawn_slots,
                    &mut sp.commands,
                    &mut sp.world,
                    &mut net_ids,
                    &mut quic,
                    tick.tick,
                );
            }
            MsgType::TimePing(bits) => {
                quic.send(
                    SendTarget::One(msg.conn_id),
                    Channel::Unreliable,
                    &MsgType::TimePong(bits),
                );
            }
            MsgType::Ping(text) => {
                debug_println!(
                    "Got a ping from conn_id {:?} with text {}",
                    msg.conn_id,
                    text
                );
                quic.send(
                    SendTarget::One(msg.conn_id),
                    Channel::Ordered,
                    &MsgType::Pong(text),
                );
            }
            MsgType::ChatMessage(sender, text) => {
                debug_println!("GameServer: Got ChatMessage: [{sender}] {text}");
                quic.send(
                    SendTarget::All,
                    Channel::Ordered,
                    &MsgType::ChatMessage(sender, text),
                );
            }
            other => debug_println!("Unhandled: {other:?}"),
        }
    }
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
            &MsgType::FileData("map.scn.ron".into(), lb.0.clone()),
        );
    }
    if let Some(cfg) = script_config {
        if let Ok(src) = std::fs::read(&cfg.path) {
            quic.send_file(SendTarget::One(conn_id), "gametype.lua".into(), src);
        }
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
        debug_println!(
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
    bipeds: &mut Query<&mut BipedPawnComponent>,
    net_ids: &Query<&NetworkID>,
    cockpits: &mut Query<(&mut Cockpit, &Transform, &ChildOf)>,
    commands: &mut Commands,
) {
    let Some((player_entity, player_net_id)) = registry.get_by_conn(conn_id) else {
        return;
    };
    let Some(target_entity) = find_networked_entity(all_networked, &target_net_id) else {
        return;
    };
    let player_net_id = player_net_id.clone();
    if let Some((mut cockpit, seat_transform, _child_of)) = cockpits
        .iter_mut()
        .find(|(_, _, child_of)| child_of.parent() == target_entity)
    {
        if cockpit.occupant.is_some() && player_entity == target_entity {
            let Some(biped_entity) = exit_vehicle(world, target_entity, &mut cockpit, seat_transform)
            else {
                return;
            };
            let Ok(biped_net_id) = net_ids.get(biped_entity) else {
                return;
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
            return;
        }
        if cockpit.occupant.is_none() {
            let pp = world
                .entity_to_handle
                .get(&player_entity)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(|rb| rb.position().translation);
            let vp = world
                .entity_to_handle
                .get(&target_entity)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(|rb| {
                    let vehicle_pos = rb_pos(rb);
                    let vehicle_rot = rb_rot(rb);
                    seat_world_point(vehicle_pos, vehicle_rot, seat_transform.translation)
                });
            let in_range = matches!((pp, vp), (Some(a), Some(b)) if {
                let d = a - b;
                d.x * d.x + d.y * d.y + d.z * d.z
                    < (cockpit.interact_radius + 4.0) * (cockpit.interact_radius + 4.0)
            });
            if in_range
                && enter_vehicle(
                    world,
                    player_entity,
                    target_entity,
                    &mut cockpit,
                    seat_transform,
                )
            {
                let _ = bipeds;
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
                    &MsgType::Possess(target_net_id),
                );
            }
        }
        return;
    }
    if held_weapons.0.contains_key(&target_net_id) {
        return;
    }
    let player_pos = world
        .entity_to_handle
        .get(&player_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| rb.position().translation);
    let weapon_pos = world
        .entity_to_handle
        .get(&target_entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| rb.position().translation);
    let in_range = match (player_pos, weapon_pos) {
        (Some(pp), Some(wp)) => {
            let d = pp - wp;
            (d.x * d.x + d.y * d.y + d.z * d.z).sqrt() < 2.0
        }
        _ => false,
    };
    if !in_range {
        return;
    }
    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
        return;
    };
    if slots.is_full() {
        let drop_pos = player_pos
            .map(|t| Vec3::new(t.x, t.y, t.z))
            .unwrap_or(Vec3::ZERO);
        let active = slots.active_mut();
        let Some(drop_id) = active.0.take() else {
            return;
        };
        let drop_entity = active.1.take();
        drop(slots);
        if let Some(drop_entity) = drop_entity {
            held_weapons.0.remove(&drop_id);
            world.teleport_body(drop_entity, drop_pos);
            world.set_body_enabled(drop_entity, true);
            quic.send(
                SendTarget::All,
                Channel::Ordered,
                &MsgType::WeaponDrop(drop_id, player_net_id.clone(), drop_pos),
            );
        }
        let Ok(mut slots) = pawn_slots.get_mut(player_entity) else {
            return;
        };
        slots.active_mut().0 = Some(target_net_id.clone());
        slots.active_mut().1 = Some(target_entity);
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    } else if slots.primary.0.is_none() {
        slots.primary = (Some(target_net_id.clone()), Some(target_entity));
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    } else {
        slots.pocket = (Some(target_net_id.clone()), Some(target_entity));
        held_weapons.0.insert(target_net_id.clone(), player_entity);
    }
    world.set_body_enabled(target_entity, false);
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
    let Some(fired) = projectile::fire_authoritative(
        kind,
        origin,
        dir,
        shooter_entity,
        tick,
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
