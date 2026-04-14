#[cfg(feature = "client")]
use bevy::core_pipeline::Skybox;
#[cfg(not(feature = "client"))]
use bevy::ecs::system::{Command, SystemState};
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::state::state::FreelyMutableState;
#[cfg(feature = "client")]
use common::GameObjectKind;
use common::game_state::GameState;
#[cfg(not(feature = "client"))]
use common::{LeaderboardScope, ScoringOption};
#[cfg(not(feature = "client"))]
use game_objects::SpawnGameObjectCommand;
#[cfg(not(feature = "client"))]
use game_objects::health::Health;
#[cfg(feature = "client")]
use game_objects::level::{
    LevelSceneRoot, MapMeta, PendingMapScene, SpawnPoint, compressed_level_hash, default_asset_dir,
    load_level_source, read_cached_map, write_cached_map,
};
#[cfg(not(feature = "client"))]
use game_objects::level::{PendingMapScene, SpawnPoint, default_asset_dir, load_level_source};
#[cfg(feature = "client")]
use game_objects::lifecycle::{pick_spawn_point_with_velocity, spawn_game_object};
#[cfg(not(feature = "client"))]
use game_objects::mode::{MatchPhase, MatchState, ModeConfig, PlayerNumbers, TeamNumbers};
#[cfg(feature = "client")]
use game_objects::pawn::Possessed;
#[cfg(not(feature = "client"))]
use game_objects::pawn::{PendingRespawns, PlayerRegistry, SeatedInVehicle};
#[cfg(not(feature = "client"))]
use net::message::*;
#[cfg(feature = "client")]
use net::message::{MsgType, NetworkIDResource};
#[cfg(feature = "client")]
use net::quic::QuicManager;
#[cfg(not(feature = "client"))]
use net::quic::*;
#[cfg(not(feature = "client"))]
use physics::physics_world::*;
#[cfg(feature = "client")]
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
#[cfg(not(feature = "client"))]
use scripting::{ScriptConfig, get_script_global};

#[cfg(feature = "client")]
use crate::hosted::{cleanup_before_app_exit, exit_after_returning_to_menu, shutdown_session};
#[cfg(feature = "client")]
use crate::messages;
#[cfg(not(feature = "client"))]
use crate::replication::{
    broadcast_health_updates, broadcast_scoreboard, broadcast_tick, spawn_player,
};
use crate::resources::*;

#[cfg(feature = "client")]
pub struct ClientSessionPlugin<S: States + FreelyMutableState + Copy> {
    pub main_menu: S,
    pub single_player: S,
    pub multiplayer: S,
}

#[cfg(feature = "client")]
impl<S: States + FreelyMutableState + Copy> Plugin for ClientSessionPlugin<S> {
    fn build(&self, app: &mut App) {
        let main_menu = self.main_menu;
        let single_player = self.single_player;
        let multiplayer = self.multiplayer;
        app.insert_resource(GuiState::default())
            .configure_sets(
                FixedUpdate,
                game_objects::health::HealthAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::projectile::ProjectileAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::level::LevelAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                common::slow_update::SlowUpdate,
                game_objects::level::LevelAuthoritySet.run_if(has_authority),
            )
            .init_resource::<LastServerState>()
            .init_resource::<LastAckedInputSeq>()
            .init_resource::<PendingWorldReady>()
            .init_resource::<PendingReconciliation>()
            .insert_resource(ClientSessionState { main_menu })
            .add_systems(
                OnEnter(single_player),
                (reset_singleplayer_spawn_state, load_sp_level::<S>).chain(),
            )
            .add_systems(OnExit(single_player), (cleanup_world, remove_script).chain())
            .add_systems(FixedUpdate, respawn_singleplayer.run_if(in_state(single_player)))
            .add_systems(OnEnter(multiplayer), connect)
            .add_systems(OnExit(multiplayer), (cleanup_world, disconnect, remove_script).chain())
            .add_systems(Update, send_world_ready.run_if(in_state(multiplayer)))
            .add_systems(Update, mark_world_ready_after_level_load.run_if(in_state(multiplayer)))
            .add_systems(Update, load_skybox.run_if(resource_added::<MapMeta>))
            .add_systems(Last, cleanup_before_app_exit)
            .add_systems(Update, exit_after_returning_to_menu.run_if(in_state(main_menu)))
            .add_systems(FixedPostUpdate, messages::on_message::<S>)
            .add_systems(
                FixedLast,
                snapshot_server_state
                    .run_if(in_state(multiplayer).and(resource_changed::<PendingReconciliation>)),
            );
    }
}

#[cfg(feature = "client")]
#[derive(Resource, Clone, Copy)]
pub(crate) struct ClientSessionState<S: States + Copy> {
    pub main_menu: S,
}

pub fn has_authority(state: Option<Res<State<GameState>>>) -> bool {
    #[cfg(feature = "client")]
    {
        state.is_some_and(|s| *s.get() == GameState::SinglePlayer)
    }
    #[cfg(not(feature = "client"))]
    {
        let _ = state;
        true
    }
}

#[cfg(feature = "client")]
fn reset_singleplayer_spawn_state(mut sp: ResMut<SinglePlayerConfig>) {
    sp.timer = None;
    sp.spawned_once = false;
}

#[cfg(feature = "client")]
fn load_sp_level<S: States + FreelyMutableState + Copy>(
    mut commands: Commands,
    sp: Res<SinglePlayerConfig>,
) {
    let _ = std::marker::PhantomData::<S>;
    if sp.map.is_empty() || sp.gametype.is_empty() {
        game_objects::messages::push(&mut commands, "No map or mode selected.");
        return;
    }
    commands.insert_resource(scripting::ScriptConfig {
        path: sp.gametype.clone(),
        is_server: false,
        source: None,
    });
    game_objects::messages::push(&mut commands, "Loading map...");
    match load_level_source(&sp.map, &default_asset_dir()) {
        Ok(level) => commands.insert_resource(PendingMapScene(level.compressed)),
        Err(err) => game_objects::messages::push(&mut commands, format!("Map load failed: {err}")),
    };
}

#[cfg(feature = "client")]
fn respawn_singleplayer(
    time: Res<Time>,
    possessed: Query<(), With<Possessed>>,
    pending_map: Option<Res<PendingMapScene>>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    spawn_points: Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
    mut sp: ResMut<SinglePlayerConfig>,
) {
    if !possessed.is_empty() || pending_map.is_some() {
        sp.timer = None;
        return;
    }
    let respawn_delay = if sp.spawned_once { common::config::RESPAWN_DELAY_SECS } else { 0.0 };
    let remaining = sp.timer.get_or_insert(respawn_delay);
    *remaining -= time.delta_secs();
    if *remaining > 0.0 {
        return;
    }
    sp.timer = None;
    let Some((position, rotation, velocity)) = pick_spawn_point_with_velocity(
        &spawn_points,
        &parent_transforms,
        &parent_parents,
        &parent_bodies,
        &physics,
        0,
        0,
    ) else {
        return;
    };
    let (entity, _, _) = spawn_game_object(
        GameObjectKind::Biped,
        position,
        rotation,
        velocity,
        0,
        &mut commands,
        &mut net_ids,
    );
    commands.entity(entity).insert(Possessed::new(128));
    sp.spawned_once = true;
}

#[cfg(feature = "client")]
fn load_skybox(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    meta: Res<MapMeta>,
    camera: Query<Entity, With<Camera3d>>,
) {
    let (Some(path), Ok(cam)) = (&meta.skybox, camera.single()) else {
        return;
    };
    let image: Handle<Image> =
        asset_server.load(game_objects::asset_path::resolve_asset_path(path));
    commands.entity(cam).insert((
        Skybox { image: image.clone(), brightness: meta.skybox_brightness, ..default() },
        EnvironmentMapLight {
            diffuse_map: image.clone(),
            specular_map: image,
            intensity: meta.env_light_intensity,
            affects_lightmapped_mesh_diffuse: true,
            ..default()
        },
    ));
}

#[cfg(feature = "client")]
pub fn cleanup_world(
    mut commands: Commands,
    camera: Query<(Entity, Option<&Children>), With<Camera3d>>,
    roots: Query<Entity, (With<Transform>, Without<Camera3d>, Without<ChildOf>)>,
) {
    if let Ok((cam, children)) = camera.single() {
        if let Some(ch) = children {
            for child in ch.iter() {
                commands.queue(move |world: &mut World| {
                    if let Ok(entity) = world.get_entity_mut(child) {
                        entity.despawn();
                    }
                });
            }
        }
        commands.queue(move |world: &mut World| {
            if let Ok(mut entity) = world.get_entity_mut(cam) {
                entity.remove_parent_in_place();
            }
        });
    }
    for entity in roots.iter() {
        commands.queue(move |world: &mut World| {
            if let Ok(entity) = world.get_entity_mut(entity) {
                entity.despawn();
            }
        });
    }
}

#[cfg(feature = "client")]
fn connect(
    mut commands: Commands,
    mut quic: ResMut<QuicManager>,
    addr: Res<ServerAddr>,
    mut pending: ResMut<PendingWorldReady>,
) {
    pending.0 = false;
    game_objects::messages::push(&mut commands, "Connecting...");
    quic.connect(addr.0);
}

#[cfg(feature = "client")]
fn send_world_ready(
    mut quic: ResMut<QuicManager>,
    mut pending: ResMut<PendingWorldReady>,
    pending_map: Option<Res<PendingMapScene>>,
) {
    if !pending.0 || pending_map.is_some() {
        return;
    }
    quic.send(
        net::quic::SendTarget::One(net::quic::SERVER_CONN_ID),
        net::quic::Channel::Ordered,
        &MsgType::ClientReady,
    );
    info!("Client: sent ClientReady");
    pending.0 = false;
}

#[cfg(feature = "client")]
fn mark_world_ready_after_level_load(
    loaded_levels: Query<(), Added<LevelSceneRoot>>,
    mut pending: ResMut<PendingWorldReady>,
) {
    if !loaded_levels.is_empty() {
        pending.0 = true;
    }
}

#[cfg(feature = "client")]
fn request_map(quic: &mut QuicManager) {
    quic.send(
        net::quic::SendTarget::One(net::quic::SERVER_CONN_ID),
        net::quic::Channel::Ordered,
        &MsgType::RequestMap,
    );
}

#[cfg(feature = "client")]
fn disconnect(
    mut quic: ResMut<QuicManager>,
    mut pending: ResMut<PendingReconciliation>,
    mut last_acked: ResMut<LastAckedInputSeq>,
    mut pending_world_ready: ResMut<PendingWorldReady>,
    mut hosted: ResMut<HostedServer>,
) {
    last_acked.0 = 0;
    pending_world_ready.0 = false;
    shutdown_session(Some(&mut quic), Some(&mut pending), &mut hosted);
}

#[cfg(feature = "client")]
fn remove_script(mut commands: Commands) {
    commands.remove_resource::<scripting::ScriptConfig>();
}

#[cfg(feature = "client")]
pub fn snapshot_server_state(
    pending: Res<PendingReconciliation>,
    mut last: ResMut<LastServerState>,
) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}

#[cfg(feature = "client")]
pub(crate) fn handle_map_hash(hash: String, quic: &mut QuicManager, commands: &mut Commands) {
    if let Some(compressed) = read_cached_map(&hash) {
        if compressed_level_hash(&compressed).as_deref() == Some(hash.as_str()) {
            game_objects::messages::push(commands, "Using cached map.");
            commands.insert_resource(PendingMapScene(compressed));
            return;
        }
        game_objects::messages::push(commands, "Cached map invalid. Redownloading.");
    }
    game_objects::messages::push(commands, "Downloading map...");
    request_map(quic);
}

#[cfg(feature = "client")]
pub(crate) fn handle_file_data(name: String, compressed: Vec<u8>, commands: &mut Commands) {
    if name == "map.scn.ron" {
        let Some(hash) = compressed_level_hash(&compressed) else {
            eprintln!("FileData: failed to hash map.scn.ron");
            return;
        };
        info!("Client: received map.scn.ron {hash}");
        write_cached_map(&hash, &compressed);
        commands.insert_resource(PendingMapScene(compressed));
        return;
    }
    if name != "gametype.lua" {
        return;
    }
    match zstd::stream::decode_all(compressed.as_slice()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(src) => commands.insert_resource(scripting::ScriptConfig {
                path: String::new(),
                is_server: false,
                source: Some(src),
            }),
            Err(e) => eprintln!("FileData: gametype.lua not valid utf8: {e}"),
        },
        Err(e) => eprintln!("FileData: failed to decompress gametype.lua: {e}"),
    };
}

#[cfg(not(feature = "client"))]
pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
}

#[cfg(not(feature = "client"))]
impl Plugin for ServerSessionPlugin {
    fn build(&self, app: &mut App) {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                if let Ok(line) = line {
                    let _ = cmd_tx.send(line);
                }
            }
        });

        app.insert_resource(BindAddr(self.bind_addr))
            .configure_sets(
                FixedUpdate,
                game_objects::health::HealthAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::projectile::ProjectileAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::level::LevelAuthoritySet.run_if(has_authority),
            )
            .configure_sets(
                common::slow_update::SlowUpdate,
                game_objects::level::LevelAuthoritySet.run_if(has_authority),
            )
            .insert_resource(LevelPath(self.map_path.clone()))
            .insert_resource(ScriptConfig {
                path: self.gametype_path.clone(),
                is_server: true,
                source: None,
            })
            .insert_resource(ConsoleCommands(std::sync::Mutex::new(cmd_rx)))
            .init_resource::<MatchState>()
            .init_resource::<PlayerNumbers>()
            .init_resource::<TeamNumbers>()
            .init_resource::<PlayerRegistry>()
            .init_resource::<PendingRespawns>()
            .init_resource::<PendingConnections>()
            .init_resource::<ActiveConnections>()
            .init_resource::<PendingInputs>()
            .init_resource::<LastProcessedInputSeq>()
            .init_resource::<BodyHistory>()
            .add_systems(Update, (tick_respawns, process_console_commands))
            .add_systems(Update, restart_round)
            .add_systems(Startup, (load_server_level, start_server, init_mode_config).chain())
            .add_systems(FixedUpdate, apply_inputs.before(step_physics))
            .add_systems(FixedUpdate, advance_match_state_time)
            .add_systems(
                FixedUpdate,
                broadcast_health_updates.after(step_physics).before(broadcast_tick),
            )
            .add_systems(FixedUpdate, broadcast_scoreboard.before(broadcast_tick))
            .add_systems(FixedUpdate, broadcast_tick.after(game_objects::health::handle_deaths));
    }
}

#[cfg(not(feature = "client"))]
fn start_server(mut quic: ResMut<QuicManager>, addr: Res<BindAddr>) {
    quic.start_server(addr.0);
}

#[cfg(not(feature = "client"))]
fn advance_match_state_time(mut match_state: ResMut<MatchState>, time: Res<Time<Fixed>>) {
    match_state.phase_elapsed_secs += time.delta_secs();
}

#[cfg(not(feature = "client"))]
fn load_server_level(mut commands: Commands, level_path: Res<LevelPath>) {
    let asset_path = &level_path.0;
    match load_level_source(asset_path, &default_asset_dir()) {
        Ok(level) => {
            commands.insert_resource(PendingMapScene(level.compressed.clone()));
            commands.insert_resource(level);
        }
        Err(err) => {
            game_objects::messages::push(&mut commands, err);
        }
    }
}

#[cfg(not(feature = "client"))]
fn init_mode_config(world: &mut World) {
    let mut config = ModeConfig::default();
    if let Some(value) = get_script_global::<f64>(world, "RESPAWN_DELAY") {
        config.respawn_delay = value as f32;
    }
    if let Some(value) = get_script_global::<bool>(world, "TEAMS_ENABLED") {
        config.teams_enabled = value;
    }
    if let Some(value) = get_script_global::<String>(world, "SCORING") {
        config.scoring = match value.as_str() {
            "unscored" => ScoringOption::Unscored,
            "score_to_win" => config.scoring,
            other => {
                warn!("unknown SCORING mode '{other}', keeping default");
                config.scoring
            }
        };
    }
    if let Some(value) = get_script_global::<i64>(world, "SCORE_TO_WIN") {
        config.scoring = ScoringOption::ScoreToWin(value as i32);
    }
    if let Some(value) = get_script_global::<f64>(world, "TIME_LIMIT_SECS") {
        config.time_limit_secs = value as f32;
    }
    if let Some(value) = get_script_global::<String>(world, "LEADERBOARD_SCOPE") {
        config.leaderboard_scope = match value.as_str() {
            "none" => LeaderboardScope::None,
            "team" => LeaderboardScope::Team,
            "player" => LeaderboardScope::Player,
            other => {
                warn!("unknown LEADERBOARD_SCOPE '{other}', keeping default");
                config.leaderboard_scope
            }
        };
    }
    if let Some(value) = get_script_global::<i64>(world, "LEADERBOARD_NUMBER_INDEX") {
        config.leaderboard_number_index = value.max(0) as usize;
    }
    if let Some(value) = get_script_global::<i64>(world, "TEAM_COUNT") {
        config.team_count = value.clamp(0, u8::MAX as i64) as u8;
    }
    if let Some(value) = get_script_global::<String>(world, "LEADERBOARD_LABEL") {
        config.leaderboard_label = value;
    }
    if let Some(value) = get_script_global::<String>(world, "PRIMARY_OBJECTIVE_LABEL") {
        config.primary_objective_label = value;
    }
    world.insert_resource(config);
}

#[cfg(not(feature = "client"))]
fn tick_respawns(
    mut pending: ResMut<PendingRespawns>,
    time: Res<Time>,
    mut quic: ResMut<QuicManager>,
    mut registry: ResMut<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    tick: Res<common::tick::Ticker>,
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
        let Some((sp, sr, sv)) = game_objects::lifecycle::pick_spawn_point_with_velocity(
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
            sv,
            &mut quic,
            &mut registry,
            &mut net_ids,
            &mut commands,
            tick.tick,
        );
    }
}

#[cfg(not(feature = "client"))]
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

#[cfg(not(feature = "client"))]
fn restart_round(world: &mut World) {
    let restart_requested =
        world.get_resource::<MatchState>().is_some_and(|state| state.restart_requested);
    if !restart_requested {
        return;
    }
    {
        let Some(mut match_state) = world.get_resource_mut::<MatchState>() else {
            return;
        };
        match_state.restart_requested = false;
        match_state.phase = MatchPhase::Playing;
        match_state.phase_elapsed_secs = 0.0;
        match_state.winner_player = None;
        match_state.winner_team = None;
    }
    if let Some(mut pending_respawns) = world.get_resource_mut::<PendingRespawns>() {
        pending_respawns.0.clear();
    }
    if let Some(mut player_numbers) = world.get_resource_mut::<PlayerNumbers>() {
        player_numbers.0.clear();
    }
    if let Some(mut team_numbers) = world.get_resource_mut::<TeamNumbers>() {
        team_numbers.0.clear();
    }

    for (conn_id, spawn_pos, spawn_rot, spawn_vel) in collect_restart_spawns(world) {
        let existing = world.get_resource::<PlayerRegistry>().and_then(|registry| {
            registry.get_character_by_conn(conn_id).map(|(entity, net_id)| (entity, net_id.clone()))
        });

        if let Some((character_entity, character_net_id)) = existing {
            reset_existing_player(
                world,
                conn_id,
                character_entity,
                character_net_id,
                spawn_pos,
                spawn_rot,
                spawn_vel,
            );
            continue;
        }

        spawn_restarted_player(world, conn_id, spawn_pos, spawn_rot, spawn_vel);
    }
}

#[cfg(not(feature = "client"))]
fn collect_restart_spawns(world: &mut World) -> Vec<(ConnectionId, Vec3, Quat, Vec3)> {
    let mut state: SystemState<(
        Res<ActiveConnections>,
        Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
        Query<&Transform>,
        Query<&ChildOf>,
        Query<&RigidBodyHandleComponent>,
        Res<PhysicsWorld>,
    )> = SystemState::new(world);
    let (
        active_connections,
        spawn_points,
        parent_transforms,
        parent_parents,
        parent_bodies,
        physics,
    ) = state.get(world);

    let num_teams = spawn_points
        .iter()
        .map(|(_, spawn, _, _)| spawn.team)
        .collect::<std::collections::HashSet<_>>()
        .len()
        .max(1);

    active_connections
        .0
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, conn_id)| {
            let team = (index % num_teams) as u8;
            game_objects::lifecycle::pick_spawn_point_with_velocity(
                &spawn_points,
                &parent_transforms,
                &parent_parents,
                &parent_bodies,
                &physics,
                team,
                index,
            )
            .map(|(spawn_pos, spawn_rot, spawn_vel)| (conn_id, spawn_pos, spawn_rot, spawn_vel))
        })
        .collect()
}

#[cfg(not(feature = "client"))]
fn reset_existing_player(
    world: &mut World,
    conn_id: ConnectionId,
    character_entity: Entity,
    character_net_id: NetworkID,
    spawn_pos: Vec3,
    spawn_rot: Quat,
    spawn_vel: Vec3,
) {
    if let Some(seated_in) = world.get::<SeatedInVehicle>(character_entity).copied() {
        let driver_seat = world
            .get::<game_objects::pawn::vehicle::VehicleComponent>(seated_in.0)
            .map(|vehicle| vehicle.driver_seat);
        if let Some(driver_seat) = driver_seat {
            world.resource_scope(|world, mut physics: Mut<PhysicsWorld>| {
                if let Some(seat_transform) = world.get::<Transform>(driver_seat).cloned()
                    && let Some(mut seat) =
                        world.get_mut::<game_objects::pawn::vehicle::DriverSeat>(driver_seat)
                {
                    let _ = game_objects::pawn::vehicle::exit_vehicle(
                        &mut physics,
                        seated_in.0,
                        &mut seat,
                        &seat_transform,
                    );
                }
            });
        }
        world.entity_mut(character_entity).remove::<SeatedInVehicle>();
        world.resource_mut::<QuicManager>().send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::SeatState(character_net_id.clone(), None),
        );
    }

    if let Some(mut health) = world.get_mut::<Health>(character_entity) {
        health.current = health.max;
    }
    if let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
        registry.set_controlled(conn_id, character_entity, character_net_id.clone());
    }
    world.resource_scope(|_, mut physics: Mut<PhysicsWorld>| {
        physics.set_body_enabled(character_entity, true);
        physics.set_body_pose(character_entity, spawn_pos, spawn_rot, spawn_vel, Vec3::ZERO);
    });
    world.resource_mut::<QuicManager>().send(
        SendTarget::One(conn_id),
        Channel::Ordered,
        &MsgType::Possess(character_net_id),
    );
}

#[cfg(not(feature = "client"))]
fn spawn_restarted_player(
    world: &mut World,
    conn_id: ConnectionId,
    spawn_pos: Vec3,
    spawn_rot: Quat,
    spawn_vel: Vec3,
) {
    let tick = world.resource::<common::tick::Ticker>().tick;
    let net_id = {
        let Some(mut net_ids) = world.get_resource_mut::<NetworkIDResource>() else {
            return;
        };
        NetworkID(net_ids.next())
    };
    let spawn_cmd = SpawnCommand {
        net_id: net_id.clone(),
        position: spawn_pos,
        starting_velocity: spawn_vel,
        shooter_velocity: Vec3::ZERO,
        rotation: spawn_rot,
        server_tick: tick,
        kind: GameObjectKind::Biped,
    };
    let entity = world.spawn_empty().id();
    SpawnGameObjectCommand { entity, cmd: spawn_cmd.clone() }.apply(world);

    let existing_conn_ids = world
        .get_resource::<PlayerRegistry>()
        .map(|registry| registry.by_conn.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        for other_conn_id in existing_conn_ids {
            quic.send(
                SendTarget::One(other_conn_id),
                Channel::Ordered,
                &MsgType::SpawnCommand(spawn_cmd.clone()),
            );
        }
        quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::SpawnCommand(spawn_cmd));
        quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::Possess(net_id.clone()));
    }
    if let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
        registry.insert(conn_id, entity, net_id);
    }
}

#[cfg(not(feature = "client"))]
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
