use bevy::{
    ecs::system::{Command, SystemState},
    prelude::*,
};
use common::{LeaderboardScope, ScoringOption};
use gameplay::{
    SpawnGameObjectCommand,
    health::Health,
    level::{PendingMapScene, SpawnPoint, default_asset_dir, load_level_source},
    mode::{MatchPhase, MatchState, ModeConfig, PlayerNumbers, Team, TeamNumbers},
    pawn::{Mounted, PendingRespawns, PlayerRegistry, Possessed},
};
use http_common::{LobbyHeartbeat, RegisterRequest, RegisterResponse};
use net::{message::*, quic::*};
use physics::physics_world::*;
use scripting::{ScriptConfig, get_script_global};

use crate::{
    messages_server::apply_melee_hit_requests,
    replication::{broadcast_scoreboard, broadcast_tick, spawn_player},
    resources::*,
};

pub struct ServerSessionPlugin {
    pub bind_addr: std::net::SocketAddr,
    pub map_path: String,
    pub gametype_path: String,
    pub advertise: Option<RegisterRequest>,
}

#[derive(Resource)]
struct HostedLobby {
    req: RegisterRequest,
    id: Option<String>,
    timer: Timer,
    poll_timer: Timer,
    max_players: u8,
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

        let bind_addr = self.bind_addr;
        let map_path = self.map_path.clone();
        let gametype_path = self.gametype_path.clone();
        crate::runtime::configure_authority_sets(app);
        app.insert_resource(ScriptConfig {
            path: gametype_path.clone(),
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
        .init_resource::<PendingMeleeHits>()
        .init_resource::<LastProcessedInputSeq>()
        .init_resource::<BodyHistory>()
        .add_systems(Update, (tick_respawns, process_console_commands))
        .add_systems(Update, restart_round)
        .add_systems(
            Startup,
            (
                move |mut commands: Commands| load_server_level(&map_path, &mut commands),
                move |mut quic: ResMut<QuicManager>| quic.start_server(bind_addr),
                init_mode_config,
            )
                .chain(),
        )
        .add_systems(
            FixedPreUpdate,
            (
                apply_inputs.before(gameplay::pawn::MovePawnsSet),
                gameplay::bot::run_bots.before(gameplay::pawn::MovePawnsSet),
            ),
        )
        .add_systems(FixedUpdate, apply_melee_hit_requests.before(step_physics))
        .add_systems(FixedUpdate, advance_match_state_time)
        .add_systems(
            FixedUpdate,
            (
                gameplay::health::broadcast_dirty_health,
                gameplay::weapon::broadcast_dirty_weapon_states,
            )
                .after(gameplay::health::handle_deaths)
                .before(broadcast_tick),
        )
        .add_systems(FixedUpdate, broadcast_scoreboard.before(broadcast_tick))
        .add_systems(
            FixedUpdate,
            broadcast_tick.after(gameplay::health::handle_deaths),
        )
        .add_systems(
            FixedPreUpdate,
            crate::messages_server::flush_pending_connections.after(crate::on_message),
        );
        if let Some(advertise) = &self.advertise {
            app.insert_resource(HostedLobby {
                req: advertise.clone(),
                id: None,
                timer: Timer::from_seconds(3.0, TimerMode::Repeating),
                poll_timer: Timer::from_seconds(0.25, TimerMode::Repeating),
                max_players: advertise.max_players,
            })
            .add_systems(Startup, register_hosted_lobby)
            .add_systems(Update, (heartbeat_hosted_lobby, poll_hosted_lobby_peers));
        }
    }
}

fn register_hosted_lobby(world: &mut World) {
    let Some(req) = world
        .get_resource::<HostedLobby>()
        .map(|lobby| lobby.req.clone())
    else {
        return;
    };
    let id = ureq::post(&format!("{}/lobbies/register", common::config::BEACON_URL))
        .send_json(&req)
        .ok()
        .and_then(|resp| resp.into_json::<RegisterResponse>().ok())
        .map(|resp| resp.id);
    if let Some(id) = id {
        if let Some(quic) = world.get_resource::<QuicManager>() {
            quic.enable_punch(id.clone());
        }
        if let Some(mut lobby) = world.get_resource_mut::<HostedLobby>() {
            lobby.id = Some(id);
        }
    }
}

fn heartbeat_hosted_lobby(
    time: Res<Time>,
    mut lobby: ResMut<HostedLobby>,
    active_connections: Res<ActiveConnections>,
) {
    if !lobby.timer.tick(time.delta()).just_finished() {
        return;
    }
    let Some(id) = lobby.id.as_ref() else {
        return;
    };
    let player_count = active_connections.0.len().min(u8::MAX as usize) as u8;
    let _ = ureq::post(&format!(
        "{}/lobbies/{}/heartbeat",
        common::config::BEACON_URL,
        id
    ))
    .send_json(LobbyHeartbeat {
        player_count,
        max_players: lobby.max_players,
    });
}

fn poll_hosted_lobby_peers(
    time: Res<Time>,
    mut lobby: ResMut<HostedLobby>,
    quic: Res<QuicManager>,
) {
    if !lobby.poll_timer.tick(time.delta()).just_finished() {
        return;
    }
    let Some(id) = lobby.id.as_ref() else {
        return;
    };
    let Ok(peers) = fetch_pending_lobby_peers(id) else {
        return;
    };
    for addr in peers {
        quic.punch_peer(addr);
    }
}

fn fetch_pending_lobby_peers(lobby_id: &str) -> Result<Vec<std::net::SocketAddr>, String> {
    let response: http_common::PendingPeersResponse = ureq::get(&format!(
        "{}/lobbies/{}/punch",
        common::config::BEACON_URL,
        lobby_id
    ))
    .call()
    .map_err(|e| e.to_string())?
    .into_json()
    .map_err(|e| e.to_string())?;
    response
        .peers
        .into_iter()
        .map(|addr| {
            addr.parse()
                .map_err(|e| format!("invalid peer addr '{addr}': {e}"))
        })
        .collect()
}

fn advance_match_state_time(mut match_state: ResMut<MatchState>, time: Res<Time<Fixed>>) {
    match_state.phase_elapsed_secs += time.delta_secs();
}

fn load_server_level(level_path: &str, commands: &mut Commands) {
    match load_level_source(level_path, &default_asset_dir()) {
        Ok(level) => {
            commands.insert_resource(PendingMapScene(level.compressed.clone()));
            commands.insert_resource(level);
        }
        Err(err) => {
            gameplay::messages::push(commands, err);
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
                eprintln!("unknown SCORING mode '{other}', keeping default");
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
                eprintln!("unknown LEADERBOARD_SCOPE '{other}', keeping default");
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
    let ready: Vec<(ConnectionId, String, Team)> = pending
        .0
        .iter_mut()
        .filter_map(|(&id, (t, k, team))| {
            *t -= dt;
            (*t <= 0.0).then(|| (id, k.clone(), *team))
        })
        .collect();
    for (conn_id, kind, team) in ready {
        pending.0.remove(&conn_id);
        let Some((sp, sr, sv)) = gameplay::lifecycle::pick_spawn_point_with_velocity(
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
            kind.as_str(),
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
                    quic.send(
                        SendTarget::One(id),
                        Channel::Ordered,
                        &MsgType::Disconnected,
                    );
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
                if let Some((pos, rot, vel)) = gameplay::lifecycle::pick_spawn_point_with_velocity(
                    &spawn_points,
                    &parent_transforms,
                    &parent_parents,
                    &parent_bodies,
                    &physics,
                    team,
                    tick.tick as usize,
                ) {
                    let (entity, _, spawn_cmd) = gameplay::lifecycle::spawn_game_object(
                        "biped",
                        Some(pos),
                        Some(rot),
                        Some(vel),
                        None,
                        tick.tick,
                        &mut commands,
                        &mut net_ids,
                    );
                    commands.entity(entity).insert((
                        Team(team),
                        Possessed::new(128),
                        gameplay::bot::BotController::new(
                            Team(team),
                            gameplay::bot::HeuristicKillerBot,
                        ),
                    ));
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &MsgType::SpawnCommand(spawn_cmd),
                    );
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
    let restart_requested = world
        .get_resource::<MatchState>()
        .is_some_and(|state| state.restart_requested);
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
            registry
                .character(conn_id)
                .map(|(entity, net_id)| (entity, net_id.clone()))
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
            gameplay::lifecycle::pick_spawn_point_with_velocity(
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
    if let Some(mounted) = world.get::<Mounted>(character_entity).copied() {
        let _ = gameplay::pawn::mount::handle_mount_parent_death(mounted.0, world);
        gameplay::pawn::send_mount_state(
            &mut world.resource_mut::<QuicManager>(),
            SendTarget::All,
            &character_net_id,
            None,
        );
    }

    if let Some(mut health) = world.get_mut::<Health>(character_entity) {
        health.restore_full();
    }
    world.entity_mut(character_entity).insert(team);
    world
        .entity_mut(character_entity)
        .insert(Possessed::new(128));
    {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        physics.set_body_enabled(character_entity, true);
        physics.set_body_pose(
            character_entity,
            spawn_pos,
            spawn_rot,
            spawn_vel,
            Vec3::ZERO,
        );
    }
    world.resource_scope(|world, mut registry: Mut<PlayerRegistry>| {
        let Some(mut quic) = world.get_resource_mut::<QuicManager>() else {
            return;
        };
        gameplay::pawn::possess_pawn(
            conn_id,
            character_entity,
            &character_net_id,
            &mut registry,
            &mut quic,
        );
    });
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
    let spawn_cmd = SpawnCommand::new(net_id.clone(), "biped", tick)
        .position(spawn_pos)
        .rotation(spawn_rot)
        .velocity(spawn_vel);
    let entity = world.spawn_empty().id();
    SpawnGameObjectCommand {
        entity,
        cmd: spawn_cmd.clone(),
    }
    .apply(world);
    world.entity_mut(entity).insert((team, Possessed::new(128)));

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
        quic.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::SpawnCommand(spawn_cmd),
        );
    }
    world.resource_scope(|world, mut registry: Mut<PlayerRegistry>| {
        let Some(mut quic) = world.get_resource_mut::<QuicManager>() else {
            return;
        };
        registry.register_character(conn_id, entity, net_id.clone());
        gameplay::pawn::send_possess(&mut quic, conn_id, &net_id);
    });
}

fn apply_inputs(
    mut pending_inputs: ResMut<PendingInputs>,
    mut last_input_seq: ResMut<LastProcessedInputSeq>,
    registry: Res<PlayerRegistry>,
    ticker: Res<common::tick::Ticker>,
    weapon_slots: Query<&gameplay::pawn::WeaponSlots>,
    body_handles: Query<&RigidBodyHandleComponent>,
    networked: Res<gameplay::NetworkEntityMap>,
    physics: Res<PhysicsWorld>,
    mut commands: Commands,
    mut possessed: Query<&mut Possessed>,
) {
    for (&conn_id, pending) in pending_inputs.0.iter_mut() {
        let Some((entity, _)) = registry.controlled_pawn(conn_id) else {
            continue;
        };
        let Some((input_seq, kind, advanced)) = pending.next() else {
            continue;
        };
        if let (Some(command_weapon), Ok(slots)) = (kind.item.weapon, weapon_slots.get(entity))
            && slots
                .active()
                .0
                .as_ref()
                .is_some_and(|id| id.0 == command_weapon)
            && let Some(weapon) = networked.get(&NetworkID(command_weapon))
            && let Ok(body_handle) = body_handles.get(entity)
            && let Some((origin, fallback_aim_dir)) = gameplay::pawn::biped::aim_pose(
                &physics,
                body_handle,
                kind.look_yaw,
                kind.look_pitch,
            )
        {
            let input_aim_dir = kind.item.aim_dir.normalize_or_zero();
            let aim_dir = if input_aim_dir == Vec3::ZERO {
                fallback_aim_dir
            } else {
                input_aim_dir
            };
            commands
                .entity(weapon)
                .insert(gameplay::weapon::WeaponFireInput {
                    want_fire: kind.item.primary,
                    fire_pressed: kind.item.primary_pressed,
                    want_alt_fire: kind.item.secondary,
                    alt_fire_pressed: kind.item.secondary_pressed,
                    reload_pressed: kind.item.reload_pressed,
                    origin,
                    aim_dir,
                    shooter: entity,
                    tick: ticker.tick,
                    prediction_id: if advanced {
                        kind.item.tick
                    } else {
                        ticker.tick
                    } as u32,
                });
        }
        let Ok(mut possessed) = possessed.get_mut(entity) else {
            continue;
        };
        possessed.push(kind);
        if advanced {
            last_input_seq.0.insert(conn_id, input_seq);
        }
        pending.clear_edges();
    }
}
