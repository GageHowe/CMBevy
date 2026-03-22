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
use game_objects::pawn::{BipedPawnComponent, SpaceshipPawnComponent};
use game_objects::SpawnGameObjectCommand;
use game_objects::health::{Health, handle_deaths};
use game_objects::weapon::{rifle, hail_mary, WeaponPlugin};
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

fn main() {
    let (bind_addr, map_path, gametype_path) = parse_args();
    println!("binding to {bind_addr}\nmap={map_path}\ngametype={gametype_path}");

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
    app.init_resource::<WeaponRegistry>();
    app.init_resource::<PendingRespawns>();
    app.init_resource::<BodyHistory>();

    app.add_systems(PreUpdate, process_inbound_server);
    app.add_systems(Update, (tick_respawns, process_console_commands, spawn_scene_weapons, assign_planet_network_ids));
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

/// Tracks weapons.
/// `free`: net_id → entity (lying in world, has physics body).
/// `held`: net_id → (weapon_entity, carrier_entity) (carried, no physics body).
/// TODO: this seems kinda scuffed, justify why this makes sense
#[derive(Resource, Default)]
struct WeaponRegistry {
    free: HashMap<NetworkID, Entity>,
    held: HashMap<NetworkID, (Entity, Entity)>,
}

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

/// Spawns physics bodies for GameObjectKind scene entities (weapons placed in the level).
/// Assigns NetworkIDs and registers in WeaponRegistry. Server-only.
fn spawn_scene_weapons(
    // Without<NetworkID>: scene placeholders never have a network ID; real spawned entities always do
    query: Query<(Entity, &GameObjectKind, &Transform), (Added<GameObjectKind>, Without<NetworkID>)>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    mut weapon_registry: ResMut<WeaponRegistry>,
) {
    for (scene_entity, kind, transform) in query.iter() {
        match kind {
            GameObjectKind::Rifle | GameObjectKind::Shotgun | GameObjectKind::HailMary => {}
            _ => { commands.entity(scene_entity).despawn(); continue; }
        }
        let net_id = NetworkID(net_ids.next());
        let entity = commands.spawn_empty().id();
        commands.queue(SpawnGameObjectCommand { entity, cmd: SpawnCommand {
            net_id: net_id.clone(),
            position: transform.translation,
            rotation: transform.rotation,
            starting_velocity: Vec3::ZERO,
            server_tick: 0,
            kind: kind.clone(),
        }});
        weapon_registry.free.insert(net_id, entity);
        commands.entity(scene_entity).despawn();
    }
}

/// Assigns NetworkIDs to planets once their physics body is ready. Server-only.
fn assign_planet_network_ids(
    query: Query<Entity, (With<PlanetComponent>, With<RigidBodyHandleComponent>, Without<NetworkID>)>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    mut weapon_registry: ResMut<WeaponRegistry>,
) {
    for entity in query.iter() {
        let net_id = NetworkID(net_ids.next());
        commands.entity(entity).insert(net_id.clone());
        weapon_registry.free.insert(net_id, entity);
    }
}

fn init_mode_config(world: &mut World) {
    let respawn_delay = get_script_global::<f64>(world, "RESPAWN_DELAY")
        .map(|d| d as f32)
        .unwrap_or(common::config::RESPAWN_DELAY_SECS);
    world.insert_resource(ModeConfig { respawn_delay });
}

fn pick_spawn_point(spawn_points: &Query<(&SpawnPoint, &Transform)>, team: u8, counter: usize) -> (Vec3, Quat) {
    let pts: Vec<_> = spawn_points.iter().filter(|(sp, _)| sp.team == team).collect();
    if pts.is_empty() {
        return (Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY);
    }
    let (_, t) = pts[counter % pts.len()];
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
fn kill_player (
    entity: Entity,
    net_id: NetworkID,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    weapon_registry: &mut WeaponRegistry,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    _tick: u64,
) {
    let drop_pos = world.entity_to_handle.get(&entity)
        // looks stupid but too lazy to look into it
        .and_then(|&h| world.rigid_body_set.get(h))
        .map(|rb| { let t = rb.position().translation; Vec3::new(t.x, t.y, t.z) })
        .unwrap_or(Vec3::ZERO);

    // Drop every held weapon back into the world.
    let held: Vec<NetworkID> = weapon_registry.held.iter()
        .filter(|&(_, &(_, carrier))| carrier == entity)
        .map(|(wid, _)| wid.clone())
        .collect();
    for wid in held {
        if let Some((weapon_entity, _)) = weapon_registry.held.remove(&wid) {
            world.teleport_body(weapon_entity, drop_pos);
            world.set_body_enabled(weapon_entity, true);
            weapon_registry.free.insert(wid.clone(), weapon_entity);
            quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos));
        }
    }

    registry.0.retain(|_, (e, _)| *e != entity);
    commands.entity(entity).despawn();
    quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(net_id));
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    script_config: Option<Res<ScriptConfig>>,
    mut registry: ResMut<PlayerRegistry>,
    mut weapon_registry: ResMut<WeaponRegistry>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    level_bytes: Option<Res<LevelBytes>>,
    spawn_points: Query<(&SpawnPoint, &Transform)>,
    weapon_kinds: Query<&GameObjectKind>,
    mut pawn_slots: Query<&mut WeaponSlots>,
    mut bipeds: Query<&mut BipedPawnComponent>,
    mut spaceships: Query<&mut SpaceshipPawnComponent>,
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
                        .map(|rb| { let t = rb.position().translation; Vec3::new(t.x, t.y, t.z) })
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

                // Tell the new client about all free weapons.
                for (weapon_net_id, &weapon_entity) in weapon_registry.free.iter() {
                    let weapon_pos = world.entity_to_handle.get(&weapon_entity)
                        .and_then(|&h| world.rigid_body_set.get(h))
                        .map(|rb| { let t = rb.position().translation; Vec3::new(t.x, t.y, t.z) })
                        .unwrap_or(Vec3::ZERO);
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered, &MsgType::SpawnCommand(SpawnCommand {
                        net_id: weapon_net_id.clone(),
                        position: weapon_pos.into(),
                        starting_velocity: Vec3::ZERO.into(),
                        rotation: Quat::IDENTITY.into(),
                        server_tick: tick.tick,
                        kind: weapon_kinds.get(weapon_entity).cloned().unwrap_or(GameObjectKind::Rifle),
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
                    kill_player(entity, net_id, &mut quic, &mut registry, &mut weapon_registry,
                                &mut commands, &mut world, tick.tick);
                }
            }
            MsgType::Input(_, kind) => {
                if let Some(&(entity, _)) = registry.0.get(&msg.conn_id) {
                    if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
                        match kind {
                            PawnInputKind::Biped(input) => {
                                if let Ok(mut biped) = bipeds.get_mut(entity) {
                                    biped.look_yaw   = input.look_yaw;
                                    biped.look_pitch = input.look_pitch;
                                    biped::apply_biped_movement(&mut world, &RigidBodyHandleComponent(handle), input, &mut biped);
                                }
                            }
                            PawnInputKind::Spaceship(input) => {
                                if let Ok(mut ship) = spaceships.get_mut(entity) {
                                    spaceship::apply_spaceship_movement(&mut world, &RigidBodyHandleComponent(handle), input, &mut ship);
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
                let weapon_entity = match weapon_registry.free.get(&target_net_id) {
                    Some(&e) => e,
                    None => continue,
                };

                let player_pos = world.entity_to_handle.get(&player_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);
                let weapon_pos = world.entity_to_handle.get(&weapon_entity)
                    .and_then(|&h| world.rigid_body_set.get(h))
                    .map(|rb| rb.position().translation);

                let in_range = match (player_pos, weapon_pos) {
                    (Some(pp), Some(wp)) => {
                        let d = pp - wp;
                        (d.x*d.x + d.y*d.y + d.z*d.z).sqrt() < 2.0
                    }
                    _ => false,
                };

                if in_range {
                    let Ok(mut slots) = pawn_slots.get_mut(player_entity) else { continue };

                    if slots.is_full() {
                        let drop_id = slots.active_mut().0.take().unwrap();
                        if let Some((drop_entity, _)) = weapon_registry.held.remove(&drop_id) {
                            let drop_pos = player_pos.map(|t| Vec3::new(t.x, t.y, t.z)).unwrap_or(Vec3::ZERO);
                            world.teleport_body(drop_entity, drop_pos);
                            world.set_body_enabled(drop_entity, true);
                            weapon_registry.free.insert(drop_id.clone(), drop_entity);
                            quic.send(SendTarget::All, Channel::Ordered,
                                &MsgType::WeaponDrop(drop_id, player_net_id.clone(), drop_pos));
                        }
                    }

                    // put the new weapon in the first empty slot (active if we just dropped)
                    if slots.primary.0.is_none() { slots.primary.0 = Some(target_net_id.clone()); }
                    else                         { slots.pocket.0  = Some(target_net_id.clone()); }
                    drop(slots);

                    weapon_registry.free.remove(&target_net_id);
                    weapon_registry.held.insert(target_net_id.clone(), (weapon_entity, player_entity));
                    world.set_body_enabled(weapon_entity, false);
                    quic.send(SendTarget::All, Channel::Ordered,
                        &MsgType::WeaponPickup(target_net_id, player_net_id));
                }
            }
            MsgType::RifleFire { weapon: weapon_net_id, shooter: _, origin, dir, tick: fire_tick } => {
                let Some(&(shooter_entity, _)) = registry.0.get(&msg.conn_id) else { continue };
                if !weapon_registry.held.get(&weapon_net_id).map(|&(_, c)| c == shooter_entity).unwrap_or(false) { continue; }
                let dir_v = dir.normalize_or_zero();
                if dir_v == Vec3::ZERO { continue; }
                // spawn authoritative projectile on server for hit detection
                rifle::spawn_rifle_projectile(origin, dir_v, &mut commands, &mut world, Some(shooter_entity));
                quic.send(SendTarget::AllExcept(msg.conn_id), Channel::Unordered,
                    &MsgType::RifleFire { weapon: weapon_net_id, shooter: registry.0[&msg.conn_id].1.clone(), origin, dir: dir_v, tick: fire_tick });
            }
            MsgType::HailMaryFire { weapon: weapon_net_id, shooter: _, origin, dir, tick: fire_tick, zoomed } => {
                let Some(&(shooter_entity, _)) = registry.0.get(&msg.conn_id) else { continue };
                if !weapon_registry.held.get(&weapon_net_id).map(|&(_, c)| c == shooter_entity).unwrap_or(false) { continue; }
                let dir_v = dir.normalize_or_zero();
                if dir_v == Vec3::ZERO { continue; }
                // spawn authoritative projectile on server for hit detection
                hail_mary::spawn_projectile(origin, dir_v, &mut commands, &mut world, Some(shooter_entity));
                quic.send(SendTarget::AllExcept(msg.conn_id), Channel::Unordered,
                    &MsgType::HailMaryFire { weapon: weapon_net_id, shooter: registry.0[&msg.conn_id].1.clone(), origin, dir: dir_v, tick: fire_tick, zoomed });
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
    mut weapon_registry: ResMut<WeaponRegistry>,
    mut pending_respawns: ResMut<PendingRespawns>,
    mode: Res<ModeConfig>,
    mut world: ResMut<PhysicsWorld>,
) {
    for (entity, health, net_id) in dead_q.iter() {
        if health.current > 0.0 { continue; }
        let conn_id = registry.0.iter()
            .find(|(_, (e, _))| *e == entity)
            .map(|(cid, _)| *cid);
        let Some(conn_id) = conn_id else { continue };

        let drop_pos = world.entity_to_handle.get(&entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(|rb| { let t = rb.position().translation; Vec3::new(t.x, t.y, t.z) })
            .unwrap_or(Vec3::ZERO);
        let held: Vec<NetworkID> = weapon_registry.held.iter()
            .filter(|&(_, &(_, carrier))| carrier == entity)
            .map(|(wid, _)| wid.clone())
            .collect();
        for wid in held {
            if let Some((weapon_entity, _)) = weapon_registry.held.remove(&wid) {
                world.teleport_body(weapon_entity, drop_pos);
                world.set_body_enabled(weapon_entity, true);
                weapon_registry.free.insert(wid.clone(), weapon_entity);
                quic.send(SendTarget::All, Channel::Ordered, &MsgType::WeaponDrop(wid, net_id.clone(), drop_pos));
            }
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
