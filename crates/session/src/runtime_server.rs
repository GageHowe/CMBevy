use bevy::{
    ecs::system::{Command, SystemState},
    prelude::*,
};
use common::{LeaderboardScope, ScoringOption};
use game_objects::{
    SpawnGameObjectCommand,
    health::Health,
    level::{PendingMapScene, SpawnPoint, default_asset_dir, load_level_source},
    mode::{MatchPhase, MatchState, ModeConfig, PlayerNumbers, Team, TeamNumbers},
    pawn::{PendingRespawns, PlayerRegistry, SeatedInVehicle},
};
use net::{message::*, quic::*};
use physics::physics_world::*;
use scripting::{ScriptConfig, get_script_global};

use crate::{
    replication::{broadcast_health_updates, broadcast_scoreboard, broadcast_tick, spawn_player},
    resources::*,
};

pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
}

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
                game_objects::health::HealthAuthoritySet.run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::projectile::ProjectileAuthoritySet
                    .run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                FixedUpdate,
                game_objects::level::LevelAuthoritySet.run_if(crate::runtime::has_authority),
            )
            .configure_sets(
                common::slow_update::SlowUpdate,
                game_objects::level::LevelAuthoritySet.run_if(crate::runtime::has_authority),
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
            .add_systems(FixedUpdate, crate::bots::run_bots.before(step_physics))
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

fn start_server(mut quic: ResMut<QuicManager>, addr: Res<BindAddr>) {
    quic.start_server(addr.0);
}

fn advance_match_state_time(mut match_state: ResMut<MatchState>, time: Res<Time<Fixed>>) {
    match_state.phase_elapsed_secs += time.delta_secs();
}

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
    let ready: Vec<(ConnectionId, GameObjectKind, Team)> = pending
        .0
        .iter_mut()
        .filter_map(|(&id, (t, k, team))| {
            *t -= dt;
            (*t <= 0.0).then(|| (id, k.clone(), *team))
        })
        .collect();
    for (conn_id, kind, team) in ready {
        pending.0.remove(&conn_id);
        let Some((sp, sr, sv)) = game_objects::lifecycle::pick_spawn_point_with_velocity(
            &spawn_points,
            &parent_transforms,
            &parent_parents,
            &parent_bodies,
            &physics,
            team.0,
            registry.controlled_count(),
        ) else {
            continue;
        };
        spawn_player(
            conn_id,
            kind,
            team,
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

fn process_console_commands(
    cmds: Res<ConsoleCommands>,
    mut quic: ResMut<QuicManager>,
    registry: Res<PlayerRegistry>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut commands: Commands,
    tick: Res<common::tick::Ticker>,
    spawn_points: Query<(Entity, &SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
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
                        packet_size: 0,
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
                println!("{} player(s) connected:", registry.controlled_count());
                for (conn_id, (entity, net_id)) in registry.controlled_entries() {
                    println!("  conn={conn_id} entity={entity:?} net_id={net_id:?}");
                }
            }
            "bot" => {
                let team = parts.next().and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
                if let Some((pos, rot, vel)) =
                    game_objects::lifecycle::pick_spawn_point_with_velocity(
                        &spawn_points,
                        &parent_transforms,
                        &parent_parents,
                        &parent_bodies,
                        &physics,
                        team,
                        tick.tick as usize,
                    )
                {
                    let (entity, _, spawn_cmd) = game_objects::lifecycle::spawn_game_object(
                        GameObjectKind::Biped,
                        pos,
                        rot,
                        vel,
                        tick.tick,
                        &mut commands,
                        &mut net_ids,
                    );
                    commands.entity(entity).insert((
                        Team(team),
                        game_objects::bot::BotController::new(
                            Team(team),
                            game_objects::bot::HeuristicKillerBot,
                        ),
                    ));
                    quic.send(SendTarget::All, Channel::Ordered, &MsgType::SpawnCommand(spawn_cmd));
                }
                println!("spawned bot on team {}", team + 1);
            }
            "" => {}
            other => println!(
                "Unknown command: {other}. Commands: shutdown, kick <id>, say <text>, status, bot [team]"
            ),
        }
    }
}

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

    for (conn_id, team, spawn_pos, spawn_rot, spawn_vel) in collect_restart_spawns(world) {
        let existing = world.get_resource::<PlayerRegistry>().and_then(|registry| {
            registry.character(conn_id).map(|(entity, net_id)| (entity, net_id.clone()))
        });

        if let Some((character_entity, character_net_id)) = existing {
            reset_existing_player(
                world,
                conn_id,
                character_entity,
                character_net_id,
                team,
                spawn_pos,
                spawn_rot,
                spawn_vel,
            );
            continue;
        }

        spawn_restarted_player(world, conn_id, team, spawn_pos, spawn_rot, spawn_vel);
    }
}

fn collect_restart_spawns(world: &mut World) -> Vec<(ConnectionId, Team, Vec3, Quat, Vec3)> {
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
            .map(|(spawn_pos, spawn_rot, spawn_vel)| {
                (conn_id, Team(team), spawn_pos, spawn_rot, spawn_vel)
            })
        })
        .collect()
}

fn reset_existing_player(
    world: &mut World,
    conn_id: ConnectionId,
    character_entity: Entity,
    character_net_id: NetworkID,
    team: Team,
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
        game_objects::pawn::broadcast_seat_state(
            &mut world.resource_mut::<QuicManager>(),
            &character_net_id,
            None,
        );
    }

    if let Some(mut health) = world.get_mut::<Health>(character_entity) {
        health.current = health.max;
    }
    if let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
        registry.set_controlled_pawn(conn_id, character_entity, character_net_id.clone());
    }
    world.entity_mut(character_entity).insert(team);
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

fn spawn_restarted_player(
    world: &mut World,
    conn_id: ConnectionId,
    team: Team,
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
    world.entity_mut(entity).insert(team);

    let existing_conn_ids = world
        .get_resource::<PlayerRegistry>()
        .map(|registry| registry.controlled_conn_ids().collect::<Vec<_>>())
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
    }
    if let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
        registry.register_character(conn_id, entity, net_id.clone());
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            quic.send(SendTarget::One(conn_id), Channel::Ordered, &MsgType::Possess(net_id));
        }
    }
}

fn apply_inputs(
    pending_inputs: Res<PendingInputs>,
    mut last_input_seq: ResMut<LastProcessedInputSeq>,
    registry: Res<PlayerRegistry>,
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<QuicManager>,
    mut bipeds: Query<&mut game_objects::pawn::biped::BipedPawnComponent>,
    mut spaceships: Query<&mut game_objects::pawn::spaceship::SpaceshipPawnComponent>,
) {
    for (&conn_id, (input_seq, kind)) in pending_inputs.0.iter() {
        let Some((entity, net_id)) = registry.controlled_pawn(conn_id) else {
            continue;
        };
        let (applied, fx) = game_objects::pawn::apply_server_input(
            entity,
            kind.clone(),
            &mut world,
            &mut bipeds,
            &mut spaceships,
        );
        if applied {
            last_input_seq.0.insert(conn_id, *input_seq);
        }
        if let Some(fx) = fx {
            let channel = game_objects::pawn::biped_ability::fx_channel(fx);
            let msg = game_objects::pawn::biped_ability::fx_message(net_id.clone(), fx);
            quic.send(SendTarget::AllExcept(conn_id), channel, &msg);
        }
    }
}
