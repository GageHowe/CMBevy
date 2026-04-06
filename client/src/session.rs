use bevy::app::AppExit;
use bevy::core_pipeline::Skybox;
use bevy::prelude::*;
use common::GameObjectKind;
use common::debug_println;
use common::tick::{NetworkStats, Ticker};
use game_objects::health::Health;
use game_objects::interaction::Interactable;
use game_objects::level::{
    LevelSceneRoot, MapMeta, PendingMapScene, SpawnPoint, compressed_level_hash,
    load_level_source, parented_world_pose, read_cached_map, write_cached_map,
};
use game_objects::pawn::biped::{BipedPawnComponent, WeaponSlots};
use game_objects::pawn::{Possessed, SeatedInVehicle};
use game_objects::projectile::{PredictedProjectileMap, ProjectileState};
use game_objects::weapon::helpers as weapon_helpers;
use game_objects::{NetworkEntityMap, SpawnGameObjectCommand};
use http_common::{LobbyInfo, RegisterRequest, RegisterResponse};
use net::message::{MsgType, NetworkID, NetworkIDResource, SimulationState, SpawnCommand};
use net::quic::QuicManager;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

use crate::GameState;
use crate::reconciliation::PendingReconciliation;
use crate::settings::Settings;
use crate::ui::ui::GuiState;

#[derive(Resource)]
pub(crate) struct ServerAddr(pub std::net::SocketAddr);

#[derive(Resource, Default)]
pub(crate) struct PendingExit(pub bool);

/// Child process handle when we spawned a local gameserver.
#[derive(Resource, Default)]
pub(crate) struct HostedServer {
    pub child: Option<std::process::Child>,
    pub stdin: Option<std::io::BufWriter<std::process::ChildStdin>>,
    /// Filled by the beacon register thread once registration succeeds; cleared on deregister.
    pub beacon_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl HostedServer {
    pub fn send_command(&mut self, cmd: &str) {
        use std::io::Write;
        if let Some(w) = &mut self.stdin {
            let _ = writeln!(w, "{cmd}");
            let _ = w.flush();
        }
    }
}

/// Map and gametype selected in the singleplayer setup screen.
#[derive(Resource, Default)]
pub(crate) struct SinglePlayerConfig {
    pub map: String,
    pub gametype: String,
}

pub struct ClientSessionPlugin;
impl Plugin for ClientSessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LastServerState>()
            .init_resource::<LastAckedInputSeq>()
            .init_resource::<PendingWorldReady>()
            .add_systems(
                OnEnter(GameState::SinglePlayer),
                (load_sp_level, spawn_local_player).chain(),
            )
            .add_systems(
                OnExit(GameState::SinglePlayer),
                (cleanup_world, remove_script).chain(),
            )
            .add_systems(
                FixedUpdate,
                respawn_singleplayer.run_if(in_state(GameState::SinglePlayer)),
            )
            .add_systems(OnEnter(GameState::Multiplayer), connect)
            .add_systems(
                OnExit(GameState::Multiplayer),
                (cleanup_world, disconnect, remove_script).chain(),
            )
            .add_systems(
                Update,
                send_world_ready.run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(
                Update,
                mark_world_ready_after_level_load.run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(Update, load_skybox.run_if(resource_added::<MapMeta>))
            .add_systems(
                Update,
                draw_server_state
                    .run_if(debug_render_on)
                    .run_if(in_state(GameState::Multiplayer)),
            )
            .add_systems(Last, cleanup_before_app_exit)
            .add_systems(
                Update,
                exit_after_returning_to_menu.run_if(in_state(GameState::MainMenu)),
            );
    }
}

#[derive(Resource, Default)]
pub struct LastServerState(pub Option<SimulationState>);

#[derive(Resource, Default)]
pub struct LastAckedInputSeq(pub u64);

#[derive(Resource, Default)]
pub struct PendingWorldReady(pub bool);

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct SpawnParams<'w, 's> {
    commands: Commands<'w, 's>,
    entity_children: Query<'w, 's, &'static Children>,
    lights: Query<'w, 's, &'static mut Visibility, With<SpotLight>>,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ClientMessageParams<'w, 's> {
    spawn: SpawnParams<'w, 's>,
    world: ResMut<'w, PhysicsWorld>,
    biped_q: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, (&'static mut WeaponSlots, &'static BipedPawnComponent), With<Possessed>>,
            Query<'w, 's, &'static BipedPawnComponent>,
        ),
    >,
    networked: Res<'w, NetworkEntityMap>,
    health_q: Query<'w, 's, &'static mut Health>,
    camera: Query<'w, 's, Entity, With<Camera3d>>,
    projectile_q: Query<'w, 's, (Entity, &'static ProjectileState)>,
    predicted_projectiles: ResMut<'w, PredictedProjectileMap>,
}

fn load_sp_level(mut commands: Commands, sp: Res<SinglePlayerConfig>) {
    let map = if sp.map.is_empty() {
        "maps/default.ron".to_string()
    } else {
        sp.map.clone()
    };
    let asset_dir = if cfg!(debug_assertions) {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../assets")
    } else {
        "assets"
    };
    if !sp.gametype.is_empty() {
        commands.insert_resource(scripting::ScriptConfig {
            path: sp.gametype.clone(),
            is_server: false,
            source: None,
        });
    }
    game_objects::messages::push(&mut commands, "Loading map...");
    match load_level_source(&map, asset_dir) {
        Ok(level) => {
            commands.insert_resource(PendingMapScene(level.compressed));
        }
        Err(err) => game_objects::messages::push(&mut commands, format!("Map load failed: {err}")),
    }
}

fn spawn_local_player(mut commands: Commands, mut net_ids: ResMut<NetworkIDResource>) {
    let cmd = SpawnCommand {
        net_id: NetworkID(net_ids.next()),
        position: Vec3::new(0.0, 800.0, 0.0),
        rotation: Quat::IDENTITY,
        starting_velocity: Vec3::ZERO,
        server_tick: 0,
        kind: GameObjectKind::Biped,
    };
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand { entity, cmd });
    commands.entity(entity).insert(Possessed::new(128));
}

fn respawn_singleplayer(
    time: Res<Time>,
    possessed: Query<(), With<Possessed>>,
    pending_map: Option<Res<PendingMapScene>>,
    mut timer: Local<Option<f32>>,
    mut commands: Commands,
    mut net_ids: ResMut<NetworkIDResource>,
    spawn_points: Query<(&SpawnPoint, &Transform, Option<&ChildOf>)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
) {
    if !possessed.is_empty() || pending_map.is_some() {
        *timer = None;
        return;
    }
    let remaining = timer.get_or_insert(common::config::RESPAWN_DELAY_SECS);
    *remaining -= time.delta_secs();
    if *remaining > 0.0 {
        return;
    }
    *timer = None;
    let (position, rotation) = spawn_points
        .iter()
        .find(|(sp, _, _)| sp.team == 0)
        .map(|(_, transform, child_of)| {
            parented_world_pose(
                transform,
                child_of,
                &parent_transforms,
                &parent_parents,
                &parent_bodies,
                &physics,
            )
        })
        .unwrap_or((Vec3::new(0.0, 800.0, 0.0), Quat::IDENTITY));
    let cmd = SpawnCommand {
        net_id: NetworkID(net_ids.next()),
        position,
        rotation,
        starting_velocity: Vec3::ZERO,
        server_tick: 0,
        kind: GameObjectKind::Biped,
    };
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand { entity, cmd });
    commands.entity(entity).insert(Possessed::new(128));
}

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
        Skybox {
            image: image.clone(),
            brightness: meta.skybox_brightness,
            ..default()
        },
        EnvironmentMapLight {
            diffuse_map: image.clone(),
            specular_map: image,
            intensity: meta.env_light_intensity,
            affects_lightmapped_mesh_diffuse: true,
            ..default()
        },
    ));
}

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
    pending.0 = false;
}

fn mark_world_ready_after_level_load(
    loaded_levels: Query<(), Added<LevelSceneRoot>>,
    mut pending: ResMut<PendingWorldReady>,
) {
    if !loaded_levels.is_empty() {
        pending.0 = true;
    }
}

fn request_map(quic: &mut QuicManager) {
    quic.send(
        net::quic::SendTarget::One(net::quic::SERVER_CONN_ID),
        net::quic::Channel::Ordered,
        &MsgType::RequestMap,
    );
}

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

fn remove_script(mut commands: Commands) {
    commands.remove_resource::<scripting::ScriptConfig>();
}

pub fn snapshot_server_state(
    pending: Res<PendingReconciliation>,
    mut last: ResMut<LastServerState>,
) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}

fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
    let Some(state) = &last.0 else { return };
    for body in state.bodies.values() {
        let pos: Vec3 = body.position.into();
        gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
    }
}

fn debug_render_on(settings: Res<Settings>) -> bool {
    settings.debug_render
}

pub(crate) fn available_maps() -> Vec<String> {
    scan_dir(&format!("{}/maps", asset_base()), "ron")
}

pub(crate) fn available_gametypes() -> Vec<String> {
    scan_dir(&format!("{}/gametypes", asset_base()), "lua")
}

pub(crate) fn gametype_path(name: &str) -> String {
    format!("{}/gametypes/{name}.lua", asset_base())
}

pub(crate) fn fetch_remote_lobbies() -> Result<Vec<LobbyInfo>, String> {
    ureq::get(&format!("{}/lobbies", common::config::BEACON_URL))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())
}

pub(crate) fn fetch_lan_lobbies() -> Result<Vec<LobbyInfo>, String> {
    use std::net::UdpSocket;
    use std::time::Duration;
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;
    let _ = sock.send_to(
        b"discover",
        format!("255.255.255.255:{}", common::config::LAN_DISCOVERY_PORT),
    );
    let _ = sock.send_to(
        b"discover",
        format!("127.0.0.1:{}", common::config::LAN_DISCOVERY_PORT),
    );
    let mut lobbies = Vec::new();
    let mut buf = [0u8; 16];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Ok(port) = std::str::from_utf8(&buf[..n]).unwrap_or("").parse::<u16>() {
                    lobbies.push(LobbyInfo {
                        id: String::new(),
                        name: format!("LAN @ {}", from.ip()),
                        host: format!("{}:{}", from.ip(), port),
                        player_count: 0,
                        max_players: 0,
                    });
                }
            }
            Err(_) => break,
        }
    }
    Ok(lobbies)
}

pub(crate) fn start_hosted_server(
    hosted: &mut HostedServer,
    port: u16,
    map: &str,
    gametype: &str,
    advertise: Option<RegisterRequest>,
) -> std::io::Result<()> {
    let mut child = spawn_gameserver(port, map, gametype)?;
    hosted.stdin = child.stdin.take().map(std::io::BufWriter::new);
    hosted.child = Some(child);
    if let Some(req) = advertise {
        beacon_register(req, std::sync::Arc::clone(&hosted.beacon_id));
    }
    Ok(())
}

pub(crate) fn cleanup_before_app_exit(
    mut exits: MessageReader<AppExit>,
    mut quic: Option<ResMut<QuicManager>>,
    mut pending: Option<ResMut<PendingReconciliation>>,
    mut hosted: ResMut<HostedServer>,
) {
    if exits.read().next().is_none() {
        return;
    }
    shutdown_session(quic.as_deref_mut(), pending.as_deref_mut(), &mut hosted);
}

pub(crate) fn exit_after_returning_to_menu(
    pending_exit: Res<PendingExit>,
    mut exit: MessageWriter<AppExit>,
) {
    if pending_exit.0 {
        exit.write(AppExit::Success);
    }
}

pub(crate) fn shutdown_session(
    quic: Option<&mut QuicManager>,
    pending: Option<&mut PendingReconciliation>,
    hosted: &mut HostedServer,
) {
    if let Some(quic) = quic {
        quic.disconnect();
        quic.inbound.clear();
    }
    if let Some(pending) = pending {
        pending.0 = None;
    }
    hosted.stdin = None;
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(id) = hosted.beacon_id.lock().unwrap().take() {
        std::thread::spawn(move || {
            let _ = ureq::delete(&format!("{}/lobbies/{id}", common::config::BEACON_URL)).call();
        });
    }
}

fn beacon_register(
    req: RegisterRequest,
    id_slot: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    std::thread::spawn(move || {
        if let Ok(resp) =
            ureq::post(&format!("{}/lobbies/register", common::config::BEACON_URL)).send_json(&req)
        {
            if let Ok(r) = resp.into_json::<RegisterResponse>() {
                *id_slot.lock().unwrap() = Some(r.id);
            }
        }
    });
}

fn asset_base() -> &'static str {
    if std::path::Path::new("assets").exists() {
        "assets"
    } else {
        "../assets"
    }
}

fn scan_dir(dir: &str, ext: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == ext))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

fn gameserver_exe() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    let name = if cfg!(windows) {
        "gameserver.exe"
    } else {
        "gameserver"
    };
    dir.join(name)
}

fn spawn_gameserver(port: u16, map: &str, gametype: &str) -> std::io::Result<std::process::Child> {
    let port = port.to_string();
    let mut command = std::process::Command::new(gameserver_exe());
    command
        .args(["--port", &port, "--map", map, "--gametype", gametype])
        .stdin(std::process::Stdio::piped());
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(|| linux::set_parent_death_signal());
    }
    command.spawn()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io;

    const PR_SET_PDEATHSIG: i32 = 1;
    const SIGTERM: i32 = 15;

    unsafe extern "C" {
        fn prctl(option: i32, arg2: i32, arg3: usize, arg4: usize, arg5: usize) -> i32;
        fn getppid() -> i32;
    }

    pub(super) fn set_parent_death_signal() -> io::Result<()> {
        unsafe {
            if prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            if getppid() == 1 {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
        }
        Ok(())
    }
}

pub(crate) fn on_message(
    quic: Option<ResMut<QuicManager>>,
    mut gui: ResMut<GuiState>,
    mut mp: ClientMessageParams<'_, '_>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut net_stats: ResMut<NetworkStats>,
    mut last_acked_input_seq: ResMut<LastAckedInputSeq>,
    time: Res<Time>,
    possessed_q: Query<(Entity, &NetworkID), With<Possessed>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let Some(mut quic) = quic else {
        return;
    };
    let mut just_spawned: std::collections::HashMap<NetworkID, (Entity, u64)> = Default::default();
    let mut local_net_id: Option<NetworkID> = possessed_q.single().ok().map(|(_, nid)| nid.clone());
    while let Some(msg) = quic.inbound.pop_front() {
        process_client_message(
            msg.msg,
            &mut quic,
            &mut gui,
            &mut mp,
            &mut ticker,
            &mut pending,
            &mut net_stats,
            &mut last_acked_input_seq,
            &time,
            &possessed_q,
            &mut next_state,
            &mut just_spawned,
            &mut local_net_id,
        );
    }
}

fn process_client_message(
    msg: MsgType,
    quic: &mut QuicManager,
    gui: &mut GuiState,
    mp: &mut ClientMessageParams<'_, '_>,
    ticker: &mut Ticker,
    pending: &mut PendingReconciliation,
    net_stats: &mut NetworkStats,
    last_acked_input_seq: &mut LastAckedInputSeq,
    time: &Time,
    possessed_q: &Query<(Entity, &NetworkID), With<Possessed>>,
    next_state: &mut NextState<GameState>,
    just_spawned: &mut std::collections::HashMap<NetworkID, (Entity, u64)>,
    local_net_id: &mut Option<NetworkID>,
) {
    match msg {
        MsgType::Connected => {}
        MsgType::MapHash(hash) => handle_map_hash(hash, quic, &mut mp.spawn.commands),
        MsgType::SpawnCommand(cmd) => {
            handle_spawn_command(&mut mp.spawn.commands, just_spawned, cmd)
        }
        MsgType::Possess(net_id) => handle_possess(
            net_id,
            local_net_id,
            just_spawned,
            &mp.networked,
            possessed_q,
            &mut mp.spawn.commands,
            ticker,
        ),
        MsgType::SeatState(biped_net_id, vehicle_net_id) => handle_seat_state(
            &biped_net_id,
            vehicle_net_id.as_ref(),
            just_spawned,
            &mp.networked,
            &mut mp.spawn.commands,
            &mut mp.world,
        ),
        MsgType::DespawnCommand(net_id) => handle_despawn(
            &net_id,
            local_net_id,
            &mp.networked,
            &mp.camera,
            &mut mp.biped_q,
            &mut mp.spawn.commands,
        ),
        MsgType::Disconnected => {
            game_objects::messages::push(&mut mp.spawn.commands, "Disconnected.");
            next_state.set(GameState::MainMenu);
        }
        MsgType::WeaponPickup(weapon_id, carrier_net_id) => handle_weapon_pickup(
            &weapon_id,
            &carrier_net_id,
            local_net_id.as_ref(),
            &mp.networked,
            &mp.camera,
            &mut mp.biped_q,
            &mut mp.spawn.commands,
            &mut mp.world,
        ),
        MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => handle_weapon_drop(
            &weapon_id,
            &carrier_id,
            drop_pos,
            local_net_id.as_ref(),
            &mp.networked,
            &mut mp.biped_q,
            &mut mp.spawn.commands,
            &mut mp.world,
        ),
        MsgType::HitResult(_, _, _) => {}
        MsgType::ProjectileConfirm { temp_id, net_id } => handle_projectile_confirm(
            temp_id,
            net_id,
            &mut mp.predicted_projectiles,
            &mp.projectile_q,
            &mut mp.spawn.commands,
        ),
        MsgType::HealthUpdate(net_id, current) => {
            handle_health_update(&net_id, current, &mp.networked, &mut mp.health_q);
        }
        MsgType::Pong(text) => {
            debug_println!("Client: Got PONG \"{text}\"");
            gui.push_log(format!("pong: {text}"));
        }
        MsgType::ChatMessage(sender, text) => gui.push_log(format!("[{sender}] {text}")),
        MsgType::TimePong(bits) => net_stats.record_pong(bits, time.elapsed_secs_f64()),
        MsgType::State(st) => {
            if st.last_input_seq >= last_acked_input_seq.0 {
                last_acked_input_seq.0 = st.last_input_seq;
                pending.0 = Some(st);
            }
        }
        MsgType::FileData(name, compressed) => {
            handle_file_data(name, compressed, &mut mp.spawn.commands)
        }
        MsgType::FlashlightState(net_id, on) => handle_flashlight_state(
            &net_id,
            on,
            local_net_id.as_ref(),
            &mp.networked,
            &mp.spawn.entity_children,
            &mut mp.spawn.lights,
        ),
        other => debug_println!("Client: Got unhandled message: {other:?}"),
    }
}

fn find_networked_entity(networked: &NetworkEntityMap, net_id: &NetworkID) -> Option<Entity> {
    networked.get(net_id)
}

fn handle_spawn_command(
    commands: &mut Commands,
    just_spawned: &mut std::collections::HashMap<NetworkID, (Entity, u64)>,
    cmd: SpawnCommand,
) {
    let net_id = cmd.net_id.clone();
    let server_tick = cmd.server_tick;
    let entity = commands.spawn_empty().id();
    commands.queue(SpawnGameObjectCommand { entity, cmd });
    just_spawned.insert(net_id, (entity, server_tick));
}

fn handle_possess(
    net_id: NetworkID,
    local_net_id: &mut Option<NetworkID>,
    just_spawned: &std::collections::HashMap<NetworkID, (Entity, u64)>,
    networked: &NetworkEntityMap,
    possessed_q: &Query<(Entity, &NetworkID), With<Possessed>>,
    commands: &mut Commands,
    ticker: &mut Ticker,
) {
    *local_net_id = Some(net_id.clone());
    let result = just_spawned
        .get(&net_id)
        .copied()
        .or_else(|| find_networked_entity(networked, &net_id).map(|entity| (entity, ticker.tick)));
    let Some((entity, server_tick)) = result else {
        return;
    };
    for (old, _) in possessed_q.iter() {
        if old != entity {
            commands.entity(old).remove::<Possessed>();
        }
    }
    ticker.tick = server_tick;
    commands.entity(entity).insert(Possessed::new(128));
}

fn handle_seat_state(
    biped_net_id: &NetworkID,
    vehicle_net_id: Option<&NetworkID>,
    just_spawned: &std::collections::HashMap<NetworkID, (Entity, u64)>,
    networked: &NetworkEntityMap,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let biped_entity = just_spawned
        .get(biped_net_id)
        .map(|(entity, _)| *entity)
        .or_else(|| find_networked_entity(networked, biped_net_id));
    let Some(biped_entity) = biped_entity else {
        return;
    };
    match vehicle_net_id.and_then(|id| find_networked_entity(networked, id)) {
        Some(vehicle_entity) => {
            world.set_body_enabled(biped_entity, false);
            commands
                .entity(biped_entity)
                .insert(SeatedInVehicle(vehicle_entity));
        }
        None => {
            world.set_body_enabled(biped_entity, true);
            commands.entity(biped_entity).remove::<SeatedInVehicle>();
        }
    }
}

fn handle_despawn(
    net_id: &NetworkID,
    local_net_id: &mut Option<NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let is_local = local_net_id.as_ref() == Some(net_id);
    if is_local {
        if let Ok(cam) = camera.single() {
            if let Ok(mut entity) = commands.get_entity(cam) {
                entity.remove_parent_in_place();
            }
        }
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            for ent in [slots.primary.1.take(), slots.pocket.1.take()] {
                if let Some(w) = ent {
                    if let Ok(mut entity) = commands.get_entity(w) {
                        entity.despawn();
                    }
                }
            }
            slots.primary.0 = None;
            slots.pocket.0 = None;
        }
        *local_net_id = None;
    } else if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
        slots.remove_by_net_id(net_id);
    }
    if let Ok(mut entity_commands) = commands.get_entity(entity) {
        entity_commands.despawn();
    }
}

fn handle_weapon_pickup(
    weapon_id: &NetworkID,
    carrier_net_id: &NetworkID,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    camera: &Query<Entity, With<Camera3d>>,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    weapon_helpers::pickup_world_weapon(world, weapon_entity);
    if local_net_id == Some(carrier_net_id) {
        let (slot_result, pivot_e) = if let Ok((mut slots, biped)) = biped_q.p0().single_mut() {
            (
                weapon_helpers::assign_pickup_slot(&mut slots, weapon_id.clone(), weapon_entity),
                biped.pitch_pivot,
            )
        } else {
            (None, None)
        };
        if let Some((is_primary, prev_to_hide)) = slot_result {
            if let Some(prev) = prev_to_hide {
                commands.entity(prev).insert(Visibility::Hidden);
            }
            if let Some(parent) = camera.single().ok().or(pivot_e) {
                weapon_helpers::attach_local_viewmodel(commands, weapon_entity, parent, is_primary);
            }
        }
        return;
    }
    let carrier = find_networked_entity(networked, carrier_net_id);
    let pivot_e = {
        let q = biped_q.p1();
        carrier.and_then(|entity| q.get(entity).ok().and_then(|b| b.pitch_pivot))
    };
    if let Some(pivot) = pivot_e {
        weapon_helpers::attach_remote_viewmodel(commands, weapon_entity, pivot);
    }
}

fn handle_weapon_drop(
    weapon_id: &NetworkID,
    carrier_id: &NetworkID,
    drop_pos: Vec3,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    biped_q: &mut ParamSet<(
        Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
        Query<&BipedPawnComponent>,
    )>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let Some(weapon_entity) = find_networked_entity(networked, weapon_id) else {
        return;
    };
    weapon_helpers::place_world_weapon(world, weapon_entity, drop_pos);
    if local_net_id == Some(carrier_id) {
        if let Ok((mut slots, _)) = biped_q.p0().single_mut() {
            slots.remove_by_net_id(weapon_id);
        }
        weapon_helpers::detach_viewmodel(commands, world, weapon_entity);
    } else {
        commands
            .entity(weapon_entity)
            .insert((Interactable { range: 2.0 }, Visibility::Inherited));
    }
}

fn handle_projectile_confirm(
    temp_id: u32,
    net_id: NetworkID,
    predicted_projectiles: &mut PredictedProjectileMap,
    projectile_q: &Query<(Entity, &ProjectileState)>,
    commands: &mut Commands,
) {
    if let Some(entity) = predicted_projectiles.get(temp_id) {
        predicted_projectiles.remove_temp_id(temp_id);
        commands.entity(entity).insert(net_id);
        return;
    }
    for (entity, state) in projectile_q.iter() {
        if state.temp_id == temp_id {
            predicted_projectiles.remove_temp_id(temp_id);
            commands.entity(entity).insert(net_id);
            break;
        }
    }
}

fn handle_health_update(
    net_id: &NetworkID,
    current: f32,
    networked: &NetworkEntityMap,
    health_q: &mut Query<&mut Health>,
) {
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(mut health) = health_q.get_mut(entity) else {
        return;
    };
    health.current = current;
}

fn handle_file_data(name: String, compressed: Vec<u8>, commands: &mut Commands) {
    if name == "map.scn.ron" {
        let Some(hash) = compressed_level_hash(&compressed) else {
            eprintln!("FileData: failed to hash map.scn.ron");
            return;
        };
        write_cached_map(&hash, &compressed);
        commands.insert_resource(PendingMapScene(compressed));
        return;
    }
    if name != "gametype.lua" {
        return;
    }
    match zstd::stream::decode_all(compressed.as_slice()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(src) => {
                commands.insert_resource(scripting::ScriptConfig {
                    path: String::new(),
                    is_server: false,
                    source: Some(src),
                });
            }
            Err(e) => eprintln!("FileData: gametype.lua not valid utf8: {e}"),
        },
        Err(e) => eprintln!("FileData: failed to decompress gametype.lua: {e}"),
    }
}

fn handle_map_hash(hash: String, quic: &mut QuicManager, commands: &mut Commands) {
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

fn handle_flashlight_state(
    net_id: &NetworkID,
    on: bool,
    local_net_id: Option<&NetworkID>,
    networked: &NetworkEntityMap,
    entity_children: &Query<&Children>,
    lights: &mut Query<&mut Visibility, With<SpotLight>>,
) {
    if local_net_id == Some(net_id) {
        return;
    }
    let Some(entity) = find_networked_entity(networked, net_id) else {
        return;
    };
    let Ok(children) = entity_children.get(entity) else {
        return;
    };
    for child in children.iter() {
        if let Ok(mut vis) = lights.get_mut(child) {
            *vis = if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}
