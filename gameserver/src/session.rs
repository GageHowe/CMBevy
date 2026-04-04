use bevy::prelude::*;
use common::tick::Ticker;
use game_objects::health::{Health, handle_deaths};
use game_objects::level::{LevelBytes, SpawnPoint, parented_world_pose, read_and_compress_level};
use game_objects::pawn::{ModeConfig, PendingRespawns, PlayerRegistry};
use game_objects::components::planet::PlanetComponent;
use net::message::{GameObjectKind, MsgType, NetworkID, NetworkIDResource};
use net::quic::{Channel, ConnectionId, InboundMessage, QuicManager, SendTarget};
use physics::physics_world::*;
use scripting::{ScriptConfig, get_script_global};
use std::collections::HashSet;
use std::sync::{Mutex, mpsc};

use crate::{
    BindAddr, BodyHistory, ConsoleCommands, LastProcessedInputSeq, LevelPath, PendingInputs,
    spawn_player,
};

pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
}

#[derive(Resource, Default)]
pub struct PendingConnections(pub HashSet<ConnectionId>);

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
    let asset_dir = if cfg!(debug_assertions) {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../assets")
    } else {
        "assets"
    };
    let fs_path = format!("{asset_dir}/{asset_path}");
    commands.insert_resource(LevelBytes(read_and_compress_level(&fs_path)));
    let handle: Handle<DynamicScene> = asset_server.load(asset_path.clone());
    commands.spawn(bevy::scene::DynamicSceneRoot(handle));
}

fn assign_planet_network_ids(
    query: Query<
        Entity,
        (
            With<PlanetComponent>,
            With<RigidBodyHandleComponent>,
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

pub(crate) fn pick_spawn_point(
    spawn_points: &Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    physics: &PhysicsWorld,
    team: u8,
    counter: usize,
) -> Option<(Vec3, Quat)> {
    let count = spawn_points
        .iter()
        .filter(|(_, sp, _, _)| sp.team == team)
        .count();
    if count == 0 {
        return None;
    }
    let Some((_, _, t, child_of)) = spawn_points
        .iter()
        .filter(|(_, sp, _, _)| sp.team == team)
        .nth(counter % count)
    else {
        return None;
    };
    Some(parented_world_pose(
        t,
        child_of,
        parent_transforms,
        parent_parents,
        parent_bodies,
        physics,
    ))
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
