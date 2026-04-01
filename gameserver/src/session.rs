use bevy::prelude::*;
use common::tick::Ticker;
use game_objects::health::{handle_deaths, Health};
use game_objects::level::{read_and_compress_level, LevelBytes, SpawnPoint};
use game_objects::pawn::biped::WeaponSlots;
use game_objects::planet::PlanetComponent;
use net::message::{GameObjectKind, MsgType, NetworkID, NetworkIDResource, PawnInputKind};
use net::quic::{Channel, ConnectionId, InboundMessage, QuicManager, SendTarget};
use physics::physics_world::*;
use scripting::{get_script_global, ScriptConfig};
use std::sync::{mpsc, Mutex};

use crate::{BindAddr, BodyHistory, ConsoleCommands, LastProcessedInputSeq, LevelPath, ModeConfig, PendingInputs, PendingRespawns, PlayerRegistry, spawn_player, slots_to_held};

pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
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
            .insert_resource(ScriptConfig { path: self.gametype_path.clone(), is_server: true, source: None })
            .insert_resource(ConsoleCommands(Mutex::new(cmd_rx)))
            .init_resource::<PlayerRegistry>()
            .init_resource::<PendingRespawns>()
            .init_resource::<PendingInputs>()
            .init_resource::<LastProcessedInputSeq>()
            .init_resource::<BodyHistory>()
            .add_systems(Update, (tick_respawns, process_console_commands, assign_planet_network_ids))
            .add_systems(Startup, (load_server_level, start_server, init_mode_config).chain())
            .add_systems(FixedUpdate, apply_inputs.before(step_physics))
            .add_systems(FixedUpdate, broadcast_health_updates.after(step_physics).before(broadcast_tick))
            .add_systems(FixedUpdate, handle_player_deaths.after(step_physics).before(handle_deaths))
            .add_systems(FixedUpdate, broadcast_tick.after(handle_deaths));
    }
}

fn start_server(mut quic: ResMut<QuicManager>, addr: Res<BindAddr>) {
    quic.start_server(addr.0);
}

fn load_server_level(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    level_path: Res<LevelPath>,
) {
    let asset_path = &level_path.0;
    let asset_dir = if cfg!(debug_assertions) { concat!(env!("CARGO_MANIFEST_DIR"), "/../assets") } else { "assets" };
    let fs_path = format!("{asset_dir}/{asset_path}");
    commands.insert_resource(LevelBytes(read_and_compress_level(&fs_path)));
    let handle: Handle<DynamicScene> = asset_server.load(asset_path.clone());
    commands.spawn(bevy::scene::DynamicSceneRoot(handle));
}

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

pub(crate) fn pick_spawn_point(
    spawn_points: &Query<(&SpawnPoint, &Transform)>,
    team: u8,
    counter: usize,
) -> (Vec3, Quat) {
    let count = spawn_points.iter().filter(|(sp, _)| sp.team == team).count();
    let Some((_, t)) = spawn_points.iter().filter(|(sp, _)| sp.team == team).nth(counter % count.max(1)) else {
        return (Vec3::new(0.0, 5.0, 0.0), Quat::IDENTITY);
    };
    (t.translation, t.rotation)
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
        spawn_player(conn_id, kind, sp, sr, &mut quic, &mut registry, &mut net_ids, &mut commands, tick.tick);
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
                    quic.send(SendTarget::One(id), Channel::Ordered, &MsgType::Disconnected);
                    quic.inbound.push_back(InboundMessage { conn_id: id, channel: Channel::Ordered, msg: MsgType::Disconnected });
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
        if health.current > 0.0 {
            continue;
        }
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
    registry: Res<PlayerRegistry>,
    last_input_seq: Res<LastProcessedInputSeq>,
    mut history: ResMut<BodyHistory>,
) {
    let state = snapshot_bodies(&world, tick.tick, query.iter());
    history.0.insert(tick.tick, state.clone());
    history.0.retain(|&t, _| tick.tick.saturating_sub(t) <= 128);
    for &conn_id in registry.0.keys() {
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
        let Some(&(entity, _)) = registry.0.get(&conn_id) else {
            continue;
        };

        match kind.clone() {
            PawnInputKind::Biped(input) => {
                if let Ok(mut biped) = bipeds.get_mut(entity) {
                    if biped.in_vehicle.is_none() {
                        if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
                            biped.look_yaw = input.look_yaw;
                            biped.look_pitch = input.look_pitch;
                            game_objects::pawn::biped::apply_biped_movement(
                                &mut world,
                                &RigidBodyHandleComponent(handle),
                                input,
                                &mut biped,
                            );
                            last_input_seq.0.insert(conn_id, *input_seq);
                        }
                    }
                }
            }
            PawnInputKind::Spaceship(input) => {
                let vehicle_entity = bipeds.get(entity).ok().and_then(|b| b.in_vehicle);
                if let Some(ve) = vehicle_entity {
                    if let Some(handle) = world.entity_to_handle.get(&ve).copied() {
                        if let Ok(mut ship) = spaceships.get_mut(ve) {
                            game_objects::pawn::spaceship::apply_spaceship_movement(
                                &mut world,
                                &RigidBodyHandleComponent(handle),
                                input,
                                &mut ship,
                            );
                            last_input_seq.0.insert(conn_id, *input_seq);
                        }
                    }
                }
            }
        }
    }
}
