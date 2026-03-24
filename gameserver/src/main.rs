// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use physics::physics_world::*;
use net::{
    quic::*,
    message::{GameObjectKind, MsgType, NetworkID, NetworkIDResource, SimulationState, SpawnCommand},
};
use net::message::PawnInputKind;
use common::tick::Ticker;
#[derive(Resource)]
struct BindAddr(SocketAddr);
use master_plugin::MasterPlugin;
use game_objects::pawn::{biped, spaceship};
use game_objects::pawn::{BipedPawnComponent, SpaceshipPawnComponent, VehicleComponent};
use game_objects::pawn::vehicle::Cockpit;
use game_objects::SpawnGameObjectCommand;
use game_objects::health::{Health, handle_deaths};
use game_objects::weapon::WeaponPlugin;
use game_objects::projectile::{rifle, hail_mary};
use game_objects::pawn::biped::WeaponSlots;
use common::debug_println;
use game_objects::level::{LevelPlugin, LevelBytes, SpawnPoint, read_and_compress_level};
use game_objects::planet::PlanetComponent;
use std::sync::{mpsc, Mutex};
use scripting::{ScriptConfig, get_script_global};

#[derive(Resource)]
struct ConsoleCommands(Mutex<mpsc::Receiver<String>>);

#[derive(Resource)]
struct ModeConfig {
    pub respawn_delay: f32,
}

fn parse_args() -> (SocketAddr, String, String) {
    let mut addr = common::config::SERVER_BIND_ADDRESS.to_string();
    let mut map = "maps/default.scn.ron".to_string(); // asset-relative; load_server_level prepends the asset dir for fs reads
    let mut gametype = "assets/gametypes/default.lua".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port"     => { if let Some(p) = args.next().and_then(|p| p.parse::<u16>().ok()) { addr = format!("0.0.0.0:{p}"); } }
            "--map"      => { if let Some(v) = args.next() { map = v; } }
            "--gametype" => { if let Some(v) = args.next() { gametype = v; } }
            _ => {}
        }
    }
    (addr.parse().unwrap(), map, gametype)
}

/// Resource holding the path to the level .scn.ron file.
#[derive(Resource)]
struct LevelPath(String);

/// Responds to UDP "discover" probes so LAN clients can find this server.
fn start_lan_discovery(quic_port: u16) {
    let port_str = quic_port.to_string();
    std::thread::spawn(move || {
        let Ok(sock) = std::net::UdpSocket::bind(format!("0.0.0.0:{}", common::config::LAN_DISCOVERY_PORT)) else { return };
        let mut buf = [0u8; 16];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
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
            file_path: if cfg!(debug_assertions) { "../assets" } else { "assets" }.to_string(),
            ..default()
        })
        .add_plugins(bevy::scene::ScenePlugin) // needed to register DynamicScene asset + RON loader
        .add_plugins(LogPlugin { level: Level::ERROR, ..default() });

    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            if let Ok(line) = line {
                let _ = cmd_tx.send(line);
            }
        }
    });

    app.add_plugins(MasterPlugin);
    app.add_plugins(LevelPlugin);
    app.add_systems(FixedUpdate, (step_physics, sync_physics_to_transforms).chain());
    app.add_systems(PostUpdate, flush_outbound);
    app.add_plugins(WeaponPlugin);
    app.insert_resource(BindAddr(bind_addr));
    app.insert_resource(LevelPath(map_path));
    app.insert_resource(ScriptConfig { path: gametype_path, is_server: true, source: None });
    app.insert_resource(ConsoleCommands(Mutex::new(cmd_rx)));
    app.init_resource::<PlayerRegistry>();
    app.init_resource::<PendingRespawns>();
    app.init_resource::<BodyHistory>();

    app.add_systems(PreUpdate, process_inbound_server);
    app.add_systems(Update, (tick_respawns, process_console_commands, assign_planet_network_ids));
    // load_server_level: read file bytes → LevelBytes, spawn DynamicSceneRoot
    app.add_systems(Startup, (load_server_level, start_server, init_mode_config).chain());
    app.add_systems(FixedUpdate, on_message.before(step_physics));
    app.add_systems(FixedUpdate, broadcast_health_updates.after(step_physics).before(broadcast_tick));
    // handle_deaths is registered by HealthPlugin; order handle_player_deaths before it
    app.add_systems(FixedUpdate, handle_player_deaths.after(step_physics).before(handle_deaths));
    app.add_systems(FixedUpdate, broadcast_tick.after(handle_deaths));

    println!("starting server...\n");
    app.run();
}

/// maps of each connected client to their spawned pawn entity and network id.
/// TODO: what to do once we have players that can switch Possessed Entities? E.g., getting into vehicles
/// TODO: how to get, say, NetworkID or ConnectionID from Entity efficiently?
/// maybe https://github.com/lun3x/multi_index_map
#[derive(Resource, Default)]
struct PlayerRegistry(HashMap<ConnectionId, (Entity, NetworkID)>);

/// Pending respawns: conn_id → (seconds_remaining, kind).
#[derive(Resource, Default)]
struct PendingRespawns(HashMap<ConnectionId, (f32, GameObjectKind)>);

/// Ring buffer of per-tick body snapshots used for tick-stamped hit replay.
/// Entries older than 128 ticks are pruned after each broadcast.
#[derive(Resource, Default)]
struct BodyHistory(HashMap<u64, SimulationState>);

/// starts the quic server
fn start_server(mut quic: ResMut<QuicManager>, mut server: ResMut<QuinnetServer>, addr: Res<BindAddr>) {
    quic.start_server(&mut server, addr.0);
}

/// Reads the level file, stores compressed bytes for client transfer, and starts async scene load.
/// LevelPath is asset-relative (e.g. "maps/default.scn.ron"); we prepend the asset dir for fs reads.
/// Bevy's FileAssetReader resolves relative paths from CARGO_MANIFEST_DIR (not CWD), so we match that.
fn load_server_level(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    level_path: Res<LevelPath>,
) {
    let asset_path = &level_path.0;
    // env! gives the compile-time manifest dir; in release, assets sit next to the binary.
    let asset_dir = if cfg!(debug_assertions) { concat!(env!("CARGO_MANIFEST_DIR"), "/../assets") } else { "assets" };
    let fs_path = format!("{asset_dir}/{asset_path}");
    commands.insert_resource(LevelBytes(read_and_compress_level(&fs_path)));
    let handle: Handle<DynamicScene> = asset_server.load(asset_path.clone());
    commands.spawn(bevy::scene::DynamicSceneRoot(handle));
}


/// Assigns NetworkIDs to planets once their physics body is ready. Server-only.
fn assign_planet_network_ids(
    query: Query<Entity, (With<PlanetComponent>, With<RigidBodyHandleComponent>, Without<NetworkID>)>,
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

fn pick_spawn_point(spawn_points: &Query<(&SpawnPoint, &Transform)>, team: u8, counter: usize) -> (Vec3, Quat) {
    let count = spawn_points.iter().filter(|(sp, _)| sp.team == team).count();
    let Some((_, t)) = spawn_points.iter().filter(|(sp, _)| sp.team == team).nth(counter % count.max(1)) else {
        return (Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY);
    };
    (t.translation, t.rotation)
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
    commands.queue(SpawnGameObjectCommand { entity, cmd: spawn_cmd.clone() });

    // Tell all currently connected players (including the new one) about the pawn.
    for (&other_conn_id, _) in registry.0.iter() {
        quic.send(SendTarget::One(other_conn_id), Channel::Ordered, &MsgType::SpawnCommand(spawn_cmd.clone()));
    }
    quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::SpawnCommand(spawn_cmd));
    // Tell the owning client to possess this pawn.
    quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::Possess(net_id.clone()));

    registry.0.insert(conn_id, (entity, net_id));
}

/// Logic for when a player leaves or dies.
/// Drops held weapons back into the world and despawns the pawn.
/// Extracts (NetworkID, Entity) pairs for all weapons currently in a pawn's slots.
fn slots_to_held(slots: &Option<&WeaponSlots>) -> Vec<(NetworkID, Entity)> {
    let Some(slots) = slots else { return vec![]; };
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
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let drop_pos = world.entity_to_handle.get(&entity)
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(rb_pos)
        .unwrap_or(Vec3::ZERO);
    for (wid, weapon_entity) in held_weapons {
        world.teleport_body(weapon_entity, drop_pos);
        world.set_body_enabled(weapon_entity, true);
        quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos));
    }
    registry.0.retain(|_, (e, _)| *e != entity);
    commands.entity(entity).despawn();
    quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(net_id));
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    level_bytes: Option<Res<LevelBytes>>,
    spawn_points: Query<(&SpawnPoint, &Transform)>,
    // non-pawn physics objects (weapons, vehicles, planets) — used to sync new clients
    non_pawn_objects: Query<(&NetworkID, &GameObjectKind, &RigidBodyHandleComponent), Without<BipedPawnComponent>>,
    // all networked entities — used to find entities by NetworkID
    all_networked: Query<(Entity, &NetworkID)>,
    mut pawn_slots: Query<&mut WeaponSlots>,
    mut bipeds: Query<&mut BipedPawnComponent>,
    mut spaceships: Query<&mut SpaceshipPawnComponent>,
    mut cockpits: Query<&mut Cockpit, With<VehicleComponent>>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                if let Some(ref lb) = level_bytes {
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered,
                        &MsgType::FileData("map.scn.ron".into(), lb.0.clone()));
                }
                if let Some(cfg) = &script_config {
                    if let Ok(src) = std::fs::read(&cfg.path) {
                        quic.send_file(SendTarget::One(msg.conn_id), "gametype.lua".into(), src);
                    }
                }
                // Tell the new client about all existing pawns (as ghosts).
                for (_, (existing_entity, existing_net_id)) in registry.0.iter() {
                    let existing_pos = world.entity_to_handle.get(existing_entity)
                        .and_then(|&h| world.rigid_body_set.get(h))
                        .map(rb_pos)
                        .unwrap_or(Vec3::ZERO);
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: existing_net_id.clone(),
                        position: existing_pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                        server_tick: tick.tick,
                        kind: GameObjectKind::Biped,
                    }));
                }

                // Tell the new client about all non-pawn physics objects (weapons, planets, etc.).
                // held weapons are excluded (not in any slot = free; in slot = skip, client learns via WeaponPickup history... TODO: send held too)
                let held_ids: std::collections::HashSet<&NetworkID> = pawn_slots.iter()
                    .flat_map(|s| [s.primary.0.as_ref(), s.pocket.0.as_ref()])
                    .flatten()
                    .collect();
                for (net_id, kind, rb) in non_pawn_objects.iter() {
                    if held_ids.contains(net_id) { continue; }
                    let pos = world.rigid_body_set.get(rb.0)
                        .map(rb_pos)
                        .unwrap_or(Vec3::ZERO);
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: net_id.clone(),
                        position: pos,
                        starting_velocity: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                        server_tick: tick.tick,
                        kind: kind.clone(),
                    }));
                }

                let num_teams = { let mut s = std::collections::HashSet::new(); for (sp, _) in spawn_points.iter() { s.insert(sp.team); } s.len().max(1) };
                let team = (registry.0.len() % num_teams) as u8;
                let (sp, sr) = pick_spawn_point(&spawn_points, team, registry.0.len());
                spawn_player(msg.conn_id, GameObjectKind::Biped, sp, sr, &mut quic, &mut registry, &mut net_ids,
                    &mut commands, tick.tick);
            }

            // if server receives a Disconnected message...
            MsgType::Disconnected => {
                pending_respawns.0.remove(&msg.conn_id);
                if let Some((entity, net_id)) = registry.0.remove(&msg.conn_id) {
                    debug_println!("GameServer: Player disconnected: entity={entity} conn={:?}", msg.conn_id);
                    let held = slots_to_held(&pawn_slots.get(entity).ok());
                    kill_player(entity, net_id, held, &mut quic, &mut registry, &mut commands, &mut world);
                }
            }
            MsgType::Input(_, kind) => {
                if let Some(&(entity, _)) = registry.0.get(&msg.conn_id) {
                    match kind {
                        PawnInputKind::Biped(input) => {
                            if let Ok(mut biped) = bipeds.get_mut(entity) {
                                // skip movement while in a vehicle; the vehicle entity moves instead
                                if biped.in_vehicle.is_none() {
                                    if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
                                        biped.look_yaw   = input.look_yaw;
                                        biped.look_pitch = input.look_pitch;
                                        biped::apply_biped_movement(&mut world, &RigidBodyHandleComponent(handle), input, &mut biped);
                                    }
                                }
                            }
                        }
                        PawnInputKind::Spaceship(input) => {
                            // route to the vehicle the biped is currently driving
                            let vehicle_entity = bipeds.get(entity).ok().and_then(|b| b.in_vehicle);
                            if let Some(ve) = vehicle_entity {
                                if let Some(handle) = world.entity_to_handle.get(&ve).copied() {
                                    if let Ok(mut ship) = spaceships.get_mut(ve) {
                                        spaceship::apply_spaceship_movement(&mut world, &RigidBodyHandleComponent(handle), input, &mut ship);
                                    }
                                    // future vehicle types: add analogous branches here
                                }
                            }
                        }
                    }
                }
            }
            MsgType::FlashlightToggle => {
                if let Some(&(entity, ref net_id)) = registry.0.get(&msg.conn_id) {
                    if let Ok(mut biped) = bipeds.get_mut(entity) {
                        biped.flashlight_on = !biped.flashlight_on;
                        let on = biped.flashlight_on;
                        quic.send(SendTarget::All, Channel::Ordered, &MsgType::FlashlightState(net_id.clone(), on));
                    }
                }
            }
            MsgType::Interact(target_net_id) => {
                let Some(&(player_entity, ref player_net_id)) = registry.0.get(&msg.conn_id) else { continue };
                let player_net_id = player_net_id.clone();
                let target_entity = all_networked.iter().find(|(_, nid)| **nid == target_net_id).map(|(e, _)| e);
                let Some(target_entity) = target_entity else { continue };

                // ---- vehicle enter / exit ----
                if let Ok(mut cockpit) = cockpits.get_mut(target_entity) {
                    if let Ok(mut biped) = bipeds.get_mut(player_entity) {
                        if biped.in_vehicle == Some(target_entity) {
                            // exit: eject biped along the vehicle's local right vector
                            let eject_pos = world.entity_to_handle.get(&target_entity)
                                .and_then(|&h| world.rigid_body_set.get(h))
                                .map(|rb| rb_pos(rb) + rb_rot(rb) * Vec3::X * 4.0)
                                .unwrap_or(Vec3::ZERO);
                            biped.in_vehicle = None;
                            cockpit.occupant = None;
                            world.set_body_enabled(player_entity, true);
                            world.teleport_body(player_entity, eject_pos);
                            quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::Possess(player_net_id));
                        } else if biped.in_vehicle.is_none() && cockpit.occupant.is_none() {
                            // enter: validate range then transfer possession
                            let pp = world.entity_to_handle.get(&player_entity).and_then(|&h| world.rigid_body_set.get(h)).map(|rb| rb.position().translation);
                            let vp = world.entity_to_handle.get(&target_entity).and_then(|&h| world.rigid_body_set.get(h)).map(|rb| rb.position().translation);
                            let in_range = matches!((pp, vp), (Some(a), Some(b)) if { let d = a - b; d.x*d.x + d.y*d.y + d.z*d.z < 36.0 });
                            if in_range {
                                biped.in_vehicle = Some(target_entity);
                                cockpit.occupant = Some(player_entity);
                                world.set_body_enabled(player_entity, false);
                                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::Possess(target_net_id));
                            }
                        }
                    }
                    continue;
                }

                // ---- weapon pickup ----
                let weapon_entity = target_entity;
                let is_free = pawn_slots.iter().all(|s| s.primary.0.as_ref() != Some(&target_net_id) && s.pocket.0.as_ref() != Some(&target_net_id));
                if !is_free { continue; }

                let player_pos = world.entity_to_handle.get(&player_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);
                let weapon_pos = world.entity_to_handle.get(&weapon_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);
                let in_range = match (player_pos, weapon_pos) {
                    (Some(pp), Some(wp)) => { let d = pp - wp; (d.x*d.x + d.y*d.y + d.z*d.z).sqrt() < 2.0 }
                    _ => false,
                };
                if !in_range { continue; }

                let Ok(mut slots) = pawn_slots.get_mut(player_entity) else { continue };
                if slots.is_full() {
                    let drop_pos = player_pos.map(|t| Vec3::new(t.x, t.y, t.z)).unwrap_or(Vec3::ZERO);
                    let active = slots.active_mut();
                    let Some(drop_id) = active.0.take() else { continue };
                    let drop_entity = active.1.take();
                    drop(slots);
                    if let Some(drop_entity) = drop_entity {
                        world.teleport_body(drop_entity, drop_pos);
                        world.set_body_enabled(drop_entity, true);
                        quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponDrop(drop_id, player_net_id.clone(), drop_pos));
                    }
                    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else { continue };
                    slots.active_mut().0 = Some(target_net_id.clone());
                    slots.active_mut().1 = Some(weapon_entity);
                } else {
                    if slots.primary.0.is_none() { slots.primary = (Some(target_net_id.clone()), Some(weapon_entity)); }
                    else                         { slots.pocket  = (Some(target_net_id.clone()), Some(weapon_entity)); }
                }
                world.set_body_enabled(weapon_entity, false);
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponPickup(target_net_id, player_net_id));
            }
            MsgType::FireRequest { weapon: weapon_net_id, kind, temp_id, origin, dir } => {
                let Some(&(shooter_entity, _)) = registry.0.get(&msg.conn_id) else { continue };
                let shooter_holds = pawn_slots.get(shooter_entity).map(|s| s.primary.0.as_ref() == Some(&weapon_net_id) || s.pocket.0.as_ref() == Some(&weapon_net_id)).unwrap_or(false);
                if !shooter_holds { continue; }
                let dir_v = dir.normalize_or_zero();
                if dir_v == Vec3::ZERO { continue; }
                // inherit shooter's velocity so the projectile is relative to the shooter
                let sv = world.entity_to_handle.get(&shooter_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(rb_vel)
                    .unwrap_or(Vec3::ZERO);
                match kind {
                    GameObjectKind::RifleProjectile => {
                        let vel = dir_v * rifle::SPEED + sv;
                        let entity = rifle::spawn(origin, vel, &mut commands, &mut world, Some(shooter_entity), 0);
                        let net_id = NetworkID(net_ids.next());
                        commands.entity(entity).insert(net_id.clone());
                        quic.send(SendTarget::AllExcept(msg.conn_id), Channel::Unordered,
                            &MsgType::SpawnCommand(SpawnCommand { net_id: net_id.clone(), position: origin, starting_velocity: vel, rotation: Quat::IDENTITY, server_tick: tick.tick, kind: GameObjectKind::RifleProjectile }));
                        quic.send(SendTarget::One(msg.conn_id), Channel::Ordered,
                            &MsgType::ProjectileConfirm { temp_id, net_id });
                    }
                    GameObjectKind::HailMaryProjectile => {
                        let vel = dir_v * hail_mary::SPEED + sv;
                        let entity = hail_mary::spawn(origin, vel, &mut commands, &mut world, Some(shooter_entity), 0);
                        let net_id = NetworkID(net_ids.next());
                        commands.entity(entity).insert(net_id.clone());
                        quic.send(SendTarget::AllExcept(msg.conn_id), Channel::Unordered,
                            &MsgType::SpawnCommand(SpawnCommand { net_id: net_id.clone(), position: origin, starting_velocity: vel, rotation: Quat::IDENTITY, server_tick: tick.tick, kind: GameObjectKind::HailMaryProjectile }));
                        quic.send(SendTarget::One(msg.conn_id), Channel::Ordered,
                            &MsgType::ProjectileConfirm { temp_id, net_id });
                    }
                    _ => {}
                }
            }
            MsgType::TimePing(bits) => {
                quic.send(SendTarget::One(msg.conn_id), Channel::Unreliable, &MsgType::TimePong(bits));
            }
            MsgType::Ping(text) => {
                debug_println!("Got a ping from conn_id {:?} with text {}", msg.conn_id, text);
                quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::Pong(text));
            }
            MsgType::ChatMessage(sender, text) => {
                debug_println!("GameServer: Got ChatMessage: [{sender}] {text}");
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::ChatMessage(sender, text));
            }
            other => debug_println!("Unhandled: {other:?}"),
        }
    }
}

fn tick_respawns(
    mut pending: ResMut<PendingRespawns>,
    time: Res<Time>,
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    tick: Res<Ticker>,
    spawn_points: Query<(&SpawnPoint, &Transform)>,
) {
    let dt = time.delta_secs();
    let ready: Vec<(ConnectionId, GameObjectKind)> = pending.0.iter_mut()
        .filter_map(|(&id, (t, k))| { *t -= dt; (*t <= 0.0).then(|| (id, k.clone())) })
        .collect();
    for (conn_id, kind) in ready {
        pending.0.remove(&conn_id);
        let (sp, sr) = pick_spawn_point(&spawn_points, 0, registry.0.len());
        spawn_player(conn_id, kind, sp, sr, &mut quic, &mut registry, &mut net_ids,
            &mut commands, tick.tick);
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
                println!("Shutting down...");
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::Disconnected);
                std::process::exit(0);
            }
            "kick" => {
                if let Some(id) = parts.next().and_then(|s| s.parse::<ConnectionId>().ok()) {
                    quic.send(SendTarget::One(id), Channel::Ordered, &MsgType::Disconnected);
                    quic.inbound.push_back(InboundMessage { conn_id: id, channel: Channel::Ordered, msg: MsgType::Disconnected });
                    println!("Kicked {id}");
                } else {
                    println!("Usage: kick <conn_id>");
                }
            }
            "say" => {
                let text = parts.next().unwrap_or("").to_string();
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::ChatMessage("[Server]".into(), text.clone()));
                println!("[Server] {text}");
            }
            "status" => {
                println!("{} player(s) connected:", registry.0.len());
                for (conn_id, (entity, net_id)) in &registry.0 {
                    println!("  conn={conn_id} entity={entity:?} net_id={net_id:?}");
                }
            }
            "" => {}
            other => println!("Unknown command: {other}. Commands: shutdown, kick <id>, say <text>, status"),
        }
    }
}

/// Handles player-specific death cleanup: drops weapons, removes from registry, queues respawn.
/// Runs before handle_deaths (which despawns the entity).
fn handle_player_deaths(
    dead_q: Query<(Entity, &Health, &NetworkID), Changed<Health>>,
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mode: Res<ModeConfig>,
    mut world: ResMut<PhysicsWorld>,
    pawn_slots: Query<&WeaponSlots>,
) {
    for (entity, health, net_id) in dead_q.iter() {
        if health.current > 0.0 { continue; }
        let conn_id = registry.0.iter().find(|(_, (e, _))| *e == entity).map(|(cid, _)| *cid);
        let Some(conn_id) = conn_id else { continue };
        let drop_pos = world.entity_to_handle.get(&entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_pos)
            .unwrap_or(Vec3::ZERO);
        for (wid, weapon_entity) in slots_to_held(&pawn_slots.get(entity).ok()) {
            world.teleport_body(weapon_entity, drop_pos);
            world.set_body_enabled(weapon_entity, true);
            quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos));
        }
        registry.0.retain(|_, (e, _)| *e != entity);
        pending_respawns.0.insert(conn_id, (mode.respawn_delay, GameObjectKind::Biped));
    }
}

/// Broadcasts any Health changes to all clients after tick_projectile_hits runs.
fn broadcast_health_updates(
    mut quic: ResMut<QuicManager>,
    health_q: Query<(&Health, &NetworkID), Changed<Health>>,
) {
    for (health, net_id) in health_q.iter() {
        quic.send(SendTarget::All, Channel::Ordered, &MsgType::HealthUpdate(net_id.clone(), health.current));
    }
}

fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &RigidBodyHandleComponent)>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    quic.send(SendTarget::All, Channel::Unreliable, &MsgType::State(state));
}
