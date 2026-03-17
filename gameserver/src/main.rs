// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use common::physics::physics_world::*;
use common::net::{
    quic::*,
    message::{GameObjectKind, MsgType, NetworkID, NetworkIDResource, SimulationState, SpawnCommand},
};
use common::tick::Ticker;
#[derive(Resource)]
struct BindAddr(SocketAddr);
use common::master_plugin::MasterPlugin;
use common::pawn::biped;
use common::pawn::pawn::BipedPawnComponent;
use common::health::Health;
use common::weapon::{rifle, shotgun, hail_mary, WeaponPlugin};
use common::pawn::biped::WeaponSlots;
use common::debug_println;
use common::level::{Map, LevelPlugin, SpawnPoint};
use std::sync::{mpsc, Mutex};
use common::game_objects::planet::PlanetBehaviorComponent;
use common::scripting::{ScriptConfig, call_script_fn, get_script_global};

#[derive(Resource)]
struct ConsoleCommands(Mutex<mpsc::Receiver<String>>);

#[derive(Resource)]
struct ModeConfig {
    pub respawn_delay: f32,
}

fn parse_args() -> (SocketAddr, String, String) {
    let mut addr = common::config::SERVER_BIND_ADDRESS.to_string();
    let mut map = "assets/maps/default.ron".to_string();
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

fn main() {
    let (bind_addr, map_path, gametype_path) = parse_args();
    println!("binding to {bind_addr}\nmap={map_path}\ngametype={gametype_path}");
    let level = Map::from_ron(&map_path).unwrap_or_else(|e| panic!("Failed to load map \"{map_path}\": {e}"));

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::asset::AssetPlugin {
            file_path: if cfg!(debug_assertions) { "../assets" } else { "assets" }.to_string(),
            ..default()
        })
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
    app.add_plugins(LevelPlugin(level));
    app.add_systems(FixedUpdate, (step_physics, sync_physics_to_transforms).chain());
    app.add_systems(PostUpdate, flush_outbound);
    app.add_plugins(WeaponPlugin);
    app.insert_resource(BindAddr(bind_addr));
    app.insert_resource(ScriptConfig { path: gametype_path, is_server: true, source: None });
    app.insert_resource(ConsoleCommands(Mutex::new(cmd_rx)));
    app.init_resource::<PlayerRegistry>();
    app.init_resource::<WeaponRegistry>();
    app.init_resource::<PendingRespawns>();
    app.init_resource::<BodyHistory>();

    app.add_systems(PreUpdate, process_inbound_server);
    app.add_systems(Update, (tick_respawns, process_console_commands));
    app.add_systems(Startup, (start_server, spawn_level_objects, init_mode_config).chain());
    app.add_systems(FixedUpdate, on_message.before(step_physics));
    app.add_systems(FixedUpdate, broadcast_tick.after(step_physics));

    println!("starting server...\n");
    app.run();
}

/// Maps each connected client to their spawned pawn entity and network id.
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
#[derive(Resource, Default)]
struct WeaponRegistry {
    free: HashMap<NetworkID, Entity>,
    held: HashMap<NetworkID, (Entity, Entity)>,
}

fn start_server(mut quic: ResMut<QuicManager>, mut server: ResMut<QuinnetServer>, addr: Res<BindAddr>) {
    quic.start_server(&mut server, addr.0);
}

/// this is fairly hard-coded, we need to make it more modular
fn spawn_level_objects(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut weapon_registry: ResMut<WeaponRegistry>,
    level: Res<Map>,
) {
    for req in &level.initial_spawns {
        let transform = Transform::from_translation(req.position).with_rotation(req.rotation);
        let net_id = NetworkID(net_ids.get_next_free_id());
        let entity = match req.kind {
            GameObjectKind::Rifle     => rifle::spawn(transform, &mut commands, &mut world),
            GameObjectKind::Shotgun   => shotgun::spawn(transform, &mut commands, &mut world),
            GameObjectKind::HailMary  => hail_mary::spawn(transform, &mut commands, &mut world),
            GameObjectKind::Planet  => {
                let params = req.planet_params.clone().unwrap_or_else(|| {
                    eprintln!("Planet spawn request missing planet_params, using defaults");
                    PlanetBehaviorComponent { inner_radius: 5, snap_radius: 0, gravity_radius: 0,
                        gravity_profile: common::game_objects::planet::GravityProfile::Constant(9.81) }
                });
                common::game_objects::planet::spawn(params, transform, &mut commands, &mut world)
            }
            _ => continue,
        };
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

fn pick_spawn_point(spawn_points: &[SpawnPoint], team: u8, counter: usize) -> (Vec3, Quat) {
    let pts: Vec<_> = spawn_points.iter().filter(|p| p.team == team).collect();
    if pts.is_empty() {
        return (Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY);
    }
    let p = &pts[counter % pts.len()];
    (p.position, p.rotation)
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
    world: &mut PhysicsWorld,
    tick: u64,
) {
    let net_id = NetworkID(net_ids.get_next_free_id());
    let entity = match kind {
        GameObjectKind::Biped => biped::spawn(Transform::from_translation(spawn_pos).with_rotation(spawn_rot), commands, world),
        _ => unreachable!("spawn_player called with non-pawn kind"),
    };
    commands.entity(entity).insert(net_id.clone());

    let spawn_cmd = |owned: bool| SpawnCommand {
        net_id: net_id.clone(),
        position: spawn_pos.into(),
        starting_velocity: Vec3::ZERO.into(),
        rotation: spawn_rot.into(),
        server_tick: tick,
        kind: kind.clone(),
        owned,
    };

    // Tell all currently connected players about the new pawn (not owned by them).
    for (&other_conn_id, _) in registry.0.iter() {
        quic.send(SendTarget::One(other_conn_id), Channel::Ordered,
            &MsgType::SpawnCommand(spawn_cmd(false)));
    }
    // Tell the client it owns this pawn.
    quic.send(SendTarget::One(conn_id), Channel::Ordered,
        &MsgType::SpawnCommand(spawn_cmd(true)));

    registry.0.insert(conn_id, (entity, net_id));
}

/// Shared logic for when a player leaves the game (disconnect or death).
/// Drops held weapons back into the world and despawns the pawn.
fn remove_player(
    entity: Entity,
    net_id: NetworkID,
    quic: &mut QuicManager,
    registry: &mut PlayerRegistry,
    weapon_registry: &mut WeaponRegistry,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    tick: u64,
) {
    let drop_pos = world.entity_to_handle.get(&entity)
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

#[derive(bevy::ecs::system::SystemParam)]
struct HitscanParams<'w, 's> {
    networked: Query<'w, 's, (Entity, &'static NetworkID)>,
    body_query: Query<'w, 's, (&'static NetworkID, &'static RigidBodyHandleComponenet)>,
    health_q: Query<'w, 's, (&'static mut Health, &'static NetworkID)>,
    history: Res<'w, BodyHistory>,
    mode: Res<'w, ModeConfig>,
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
    level: Res<Map>,
    weapon_kinds: Query<&GameObjectKind>,
    mut pawn_slots: Query<&mut WeaponSlots>,
    mut bipeds: Query<&mut BipedPawnComponent>,
    mut hs: HitscanParams,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::Connected => {
                if let Some(data) = level.to_compressed_ron() {
                    quic.send(SendTarget::One(msg.conn_id), Channel::Ordered,
                        &MsgType::FileData("map.ron".into(), data));
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
                        owned: false,
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
                        owned: false,
                    }));
                }

                let num_teams = { let mut s = std::collections::HashSet::new(); for p in &level.spawn_points { s.insert(p.team); } s.len().max(1) };
                let team = (registry.0.len() % num_teams) as u8;
                let (sp, sr) = pick_spawn_point(&level.spawn_points, team, registry.0.len());
                spawn_player(msg.conn_id, GameObjectKind::Biped, sp, sr, &mut quic, &mut registry, &mut net_ids,
                    &mut commands, &mut world, tick.tick);
            }
            MsgType::Disconnected => {
                pending_respawns.0.remove(&msg.conn_id);
                if let Some((entity, net_id)) = registry.0.remove(&msg.conn_id) {
                    debug_println!("GameServer: Player disconnected: entity={entity} conn={:?}", msg.conn_id);
                    remove_player(entity, net_id, &mut quic, &mut registry, &mut weapon_registry,
                        &mut commands, &mut world, tick.tick);
                }
            }
            MsgType::Input(pawn_input) => {
                if let Some(&(entity, _)) = registry.0.get(&msg.conn_id) {
                    if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
                        if let Ok(mut biped) = bipeds.get_mut(entity) {
                            biped.look_yaw   = pawn_input.input.look_yaw;
                            biped.look_pitch = pawn_input.input.look_pitch;
                            biped::apply_biped_movement(&mut world, &RigidBodyHandleComponenet(handle), pawn_input.input, &mut biped);
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

                    if slots.slots.iter().all(|s| s.0.is_some()) {
                        let active = slots.active;
                        let drop_id = slots.slots[active].0.take().unwrap();
                        if let Some((drop_entity, _)) = weapon_registry.held.remove(&drop_id) {
                            let drop_pos = player_pos.map(|t| Vec3::new(t.x, t.y, t.z)).unwrap_or(Vec3::ZERO);
                            world.teleport_body(drop_entity, drop_pos);
                            world.set_body_enabled(drop_entity, true);
                            weapon_registry.free.insert(drop_id.clone(), drop_entity);
                            quic.send(SendTarget::All, Channel::Ordered,
                                &MsgType::WeaponDrop(drop_id, player_net_id.clone(), drop_pos));
                        }
                    }

                    let slot_idx = slots.slots.iter().position(|s| s.0.is_none()).unwrap();
                    slots.slots[slot_idx].0 = Some(target_net_id.clone());
                    drop(slots);

                    weapon_registry.free.remove(&target_net_id);
                    weapon_registry.held.insert(target_net_id.clone(), (weapon_entity, player_entity));
                    world.set_body_enabled(weapon_entity, false);
                    quic.send(SendTarget::All, Channel::Ordered,
                        &MsgType::WeaponPickup(target_net_id, player_net_id));
                }
            }
            MsgType::Fire(weapon_net_id, origin, direction, fire_tick) => {
                let Some(&(shooter_entity, _)) = registry.0.get(&msg.conn_id) else { continue };
                let weapon_entity = match weapon_registry.held.get(&weapon_net_id) {
                    Some(&(we, carrier)) if carrier == shooter_entity => we,
                    _ => continue,
                };
                let dir_v = Vec3::from(direction).normalize_or_zero();
                if dir_v == Vec3::ZERO { continue; }

                let is_projectile = weapon_kinds.get(weapon_entity).map(|k| matches!(k, GameObjectKind::HailMary)).unwrap_or(false);
                if is_projectile {
                    quic.send(SendTarget::AllExcept(msg.conn_id), Channel::Unordered, &MsgType::Fire(weapon_net_id, origin, direction, fire_tick));
                } else {
                    // Hitscan: lag-comp raycast inline.
                    let origin_v: Vec3 = origin.into();
                    let pairs: Vec<(NetworkID, RigidBodyHandle)> = hs.body_query.iter()
                        .map(|(nid, rbh)| (nid.clone(), rbh.0))
                        .collect();
                    let current = snapshot_bodies(&world, tick.tick, hs.body_query.iter());
                    if let Some(historical) = hs.history.0.get(&fire_tick) {
                        restore_snapshot(&mut world, historical, &pairs);
                    }
                    let (range, damage) = match weapon_kinds.get(weapon_entity) {
                        Ok(GameObjectKind::Rifle)   => (rifle::RANGE, rifle::DAMAGE),
                        Ok(GameObjectKind::Shotgun) => (shotgun::RANGE, shotgun::DAMAGE),
                        _ => { restore_snapshot(&mut world, &current, &pairs); continue; }
                    };
                    let hit = world.cast_ray(origin_v, dir_v, range, Some(shooter_entity));
                    restore_snapshot(&mut world, &current, &pairs);
                    let (end, hit_net_id) = match hit {
                        Some((hit_entity, toi)) => {
                            let hit_net_id = hs.networked.iter().find(|(e, _)| *e == hit_entity).map(|(_, nid)| nid.clone());
                            (origin_v + dir_v * toi, hit_net_id)
                        }
                        None => (origin_v + dir_v * range, None),
                    };
                    quic.send(SendTarget::All, Channel::Unreliable,
                        &MsgType::HitResult(origin_v.into(), end.into(), hit_net_id.clone()));
                    if let Some(hit_nid) = hit_net_id {
                        if let Some((mut health, _)) = hs.health_q.iter_mut().find(|(_, nid)| **nid == hit_nid) {
                            let died = health.apply_damage(damage);
                            quic.send(SendTarget::All, Channel::Ordered,
                                &MsgType::HealthUpdate(hit_nid.clone(), health.current));
                            if died {
                                if let Some((dead_entity, _)) = hs.networked.iter().find(|(_, nid)| **nid == hit_nid) {
                                    let player_entry = registry.0.iter()
                                        .find(|(_, (e, _))| *e == dead_entity)
                                        .map(|(cid, (_, nid))| (*cid, nid.clone()));
                                    if let Some((conn_id, player_net_id)) = player_entry {
                                        remove_player(dead_entity, player_net_id, &mut quic, &mut registry,
                                            &mut weapon_registry, &mut commands, &mut world, tick.tick);
                                        pending_respawns.0.insert(conn_id, (hs.mode.respawn_delay, GameObjectKind::Biped));
                                    } else {
                                        commands.entity(dead_entity).despawn();
                                        quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(hit_nid));
                                    }
                                }
                            }
                        }
                    }
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
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    level: Res<Map>,
    mode: Res<ModeConfig>,
) {
    let dt = time.delta_secs();
    let ready: Vec<(ConnectionId, GameObjectKind)> = pending.0.iter_mut()
        .filter_map(|(&id, (t, k))| { *t -= dt; (*t <= 0.0).then(|| (id, k.clone())) })
        .collect();
    for (conn_id, kind) in ready {
        pending.0.remove(&conn_id);
        let (sp, sr) = pick_spawn_point(&level.spawn_points, 0, registry.0.len());
        spawn_player(conn_id, kind, sp, sr, &mut quic, &mut registry, &mut net_ids,
            &mut commands, &mut world, tick.tick);
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

fn broadcast_tick(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkID, &RigidBodyHandleComponenet)>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    quic.send(SendTarget::All, Channel::Unreliable, &MsgType::State(state));
}
