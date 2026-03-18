// client executable

use bevy::core_pipeline::Skybox;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::input::EguiWantsInput;
use camera::spawn_camera;
use net::{
    message::*,
    quic::*,
};
use common::GameObjectKind;
use game_objects::pawn::pawn::PitchPivot;
use game_objects::pawn::biped;
use game_objects::pawn::pawn::*;
use physics::physics_world::*;
use reconciliation::{PendingReconciliation, ReconciliationPlugin};
use tick_sync::{NetworkStats, TickSyncPlugin};
use common::tick::Ticker;
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;
use common::interaction::Interactable;
use game_objects::weapon::{rifle, shotgun, hail_mary, WeaponPlugin, PendingHullCollider};
use game_objects::pawn::biped::WeaponSlots;
use std::net::SocketAddr;

mod camera;
mod outline;
mod ui;
mod reconciliation;
mod tick_sync;
mod menu;
use menu::MenuPlugin;
use outline::OutlinePlugin;

#[derive(Resource)]
pub(crate) struct ServerAddr(pub SocketAddr);

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

/// Bundles spawn-related parameters to stay within Bevy's 16-param SystemParam limit.
/// ... bad job claude
#[derive(bevy::ecs::system::SystemParam)]
struct SpawnParams<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    asset_server: Res<'w, AssetServer>,
    entity_children: Query<'w, 's, &'static Children>,
    lights: Query<'w, 's, &'static mut Visibility, With<SpotLight>>,
    hull_assets: Res<'w, Assets<ConvexHullAsset>>,
}
use settings::{Settings, SettingsPlugin};
use steam::SteamworksPlugin;
use common::debug_println;
use game_objects::health::Health;
use game_objects::master_plugin::MasterPlugin;
use game_objects::level::{Map, PendingHullColliders, spawn_static_colliders, spawn_hull_colliders, load_level_scene, spawn_level_planets, cleanup_level};
use physics::convex_hull_asset::ConvexHullAsset;
use game_objects::planet::draw_planet_radii;
use game_objects::pawn::biped::draw_biped_debug;
use ui::ui::GuiState;
mod settings;
mod steam;
mod sound;
use sound::SoundPlugin;

/// The local player's own NetworkID, set when the server's owned SpawnCommand arrives.
#[derive(Resource, Default)]
struct LocalNetworkID(Option<NetworkID>);

/// Active hitscan beams to draw as gizmos. Each entry is (origin, end, seconds_remaining).
#[derive(Resource, Default)]
struct HitBeams(Vec<(Vec3, Vec3, f32)>);

/// Most recent server SimulationState, retained for debug visualization.
#[derive(Resource, Default)]
struct LastServerState(Option<net::message::SimulationState>);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub(crate) enum GameState {
    /// the game starts into this. has Quit
    #[default]
    MainMenu,
    SinglePlayer,
    Multiplayer,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
pub(crate) enum UiState {
    #[default]
    Playing,
    Paused,
    Settings,
}

/// Syncs physics bodies to Bevy transforms every frame using velocity, decoupled from the fixed tick.
/// EXTRAPOLATE=true: projects forward from the last step by the accumulated overstep time.
/// EXTRAPOLATE=false: interpolates — steps back one fixed dt then forward by overstep, keeping
///   the visual one tick behind but never overshooting.
fn sync_physics_visual(
    world: Res<PhysicsWorld>,
    time: Res<Time<Fixed>>,
    settings: Res<Settings>,
    mut query: Query<(&RigidBodyHandleComponenet, &mut Transform)>,
) {
    use settings::PhysicsInterp;
    let overstep = time.overstep_fraction();
    let fixed_dt = time.delta_secs();
    let dt_offset = match settings.physics_interp {
        PhysicsInterp::Off => 0.0,
        PhysicsInterp::Extrapolate => overstep * fixed_dt,
        PhysicsInterp::Interpolate => (overstep - 1.0) * fixed_dt,
    };

    for (body_handle, mut transform) in query.iter_mut() {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else { continue };
        let pos = body.position();
        let cur_pos = Vec3::new(pos.translation.x, pos.translation.y, pos.translation.z);
        let cur_rot = Quat::from_xyzw(pos.rotation.x, pos.rotation.y, pos.rotation.z, pos.rotation.w);
        let linvel = Vec3::new(body.linvel().x, body.linvel().y, body.linvel().z);
        let angvel = Vec3::new(body.angvel().x, body.angvel().y, body.angvel().z);

        transform.translation = cur_pos + linvel * dt_offset;
        let ang_speed = angvel.length();
        transform.rotation = if ang_speed > 1e-6 {
            Quat::from_axis_angle(angvel / ang_speed, ang_speed * dt_offset) * cur_rot
        } else {
            cur_rot
        };
    }
}

fn parse_server_addr() -> SocketAddr {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--server" {
            if let Some(addr) = args.next().and_then(|a| a.parse().ok()) {
                return addr;
            }
        }
    }
    common::config::SERVER_BIND_ADDRESS.parse().unwrap()
}

fn main() {
    let server_addr = parse_server_addr();
    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: if cfg!(debug_assertions) { "../assets" } else { "assets" }.to_string(),
                ..default()
            })
            .set(LogPlugin {
                level: Level::WARN,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "client".into(),
                    present_mode: PresentMode::FifoRelaxed,
                    ..default()
                }),
                ..default()
            }),
    );

    app.add_plugins(OutlinePlugin)
        .add_plugins(MasterPlugin)
        .add_plugins(SteamworksPlugin)
        .add_plugins(SettingsPlugin)
        .init_state::<GameState>()
        .init_state::<UiState>()
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(PawnPlugin)
        .add_plugins(WeaponPlugin)
        .add_plugins(SoundPlugin)
        .add_plugins(ReconciliationPlugin(GameState::Multiplayer, biped::apply_biped_movement))
        .add_plugins(TickSyncPlugin(GameState::Multiplayer))
        .insert_resource(ServerAddr(server_addr))
        .init_resource::<LocalNetworkID>()
        .init_resource::<HostedServer>()
        .init_resource::<HitBeams>()
        .init_resource::<LastServerState>()
        .init_resource::<PendingHullColliders>()
        .add_systems(FixedUpdate, step_physics
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))))
        .add_systems(FixedUpdate, despawn_projectiles
            .after(step_physics)
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))))
        .add_systems(Update, sync_physics_visual
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))))
        .add_systems(Startup, spawn_camera);

    app.add_systems(PreUpdate, process_inbound_client.run_if(in_state(GameState::Multiplayer)));
    app.add_systems(PostUpdate, flush_outbound.run_if(in_state(GameState::Multiplayer)));

    app.add_systems(OnEnter(GameState::SinglePlayer), (load_sp_level, spawn_local_player, spawn_sp_weapons).chain());
    app.add_systems(OnExit(GameState::SinglePlayer), (cleanup_world, cleanup_level, remove_script).chain());
    app.add_systems(OnEnter(GameState::Multiplayer), connect);
    app.add_systems(OnExit(GameState::Multiplayer), (cleanup_world, disconnect, cleanup_level, remove_script).chain());

    // FixedPreUpdate ordering:
    //   maybe_reconcile → gather_pawn_input → send_pawn_input → move_bipeds
    // (maybe_reconcile registered by ReconciliationPlugin)
    app.add_systems(
        FixedPreUpdate,
        send_pawn_input
            .after(gather_pawn_input)
            .before(MovePawnsSet)
            // AND OTHER PAWNS
            .run_if(in_state(GameState::Multiplayer)),
    );


    // FixedPostUpdate:
    //   record_world_state (ReconciliationPlugin) → on_message/send_chat
    app.add_systems(FixedPostUpdate, on_message.run_if(in_state(GameState::Multiplayer)));

    app.add_systems(FixedPostUpdate, snapshot_server_state.after(on_message).run_if(in_state(GameState::Multiplayer).and(resource_changed::<PendingReconciliation>)));


    // (tick increment is FixedLast)

    app.add_systems(Update, interact.run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))));
    app.add_systems(Update, toggle_flashlight.run_if(in_state(GameState::Multiplayer).or(in_state(GameState::SinglePlayer))));
    app.add_systems(Update, draw_hit_beams);
    app.add_systems(Update, draw_server_state.run_if(in_state(GameState::Multiplayer)));
    app.add_systems(Update, (spawn_static_colliders, load_level_scene, spawn_level_planets)
        .run_if(resource_added::<Map>));
    app.add_systems(Update, load_skybox.run_if(resource_added::<Map>));

    app.add_systems(Update, spawn_hull_colliders);
    app.add_systems(Update, swap_weapon_hull_colliders);
    app.add_systems(Update, draw_planet_radii);
    app.add_systems(Update, draw_biped_debug);
    app.add_systems(FixedUpdate, hail_mary::draw_projectile_debug
        .after(step_physics)
        .before(despawn_projectiles)
        .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))));
    app.add_systems(FixedUpdate, hail_mary::tick_muzzle_flash);

    debug_println!("starting client...\n");
    app.run();
}

fn despawn_projectiles(
    mut commands: Commands,
    world: Res<PhysicsWorld>,
    projectiles: Query<(Entity, &RigidBodyHandleComponenet), With<hail_mary::HailMaryProjectileState>>,
) {
    for (entity, body_handle) in projectiles.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
        if rb.colliders().iter().any(|&ch| world.narrow_phase.contact_pairs_with(ch).any(|p| p.has_any_active_contact())) {
            commands.entity(entity).despawn();
        }
    }
}

fn load_sp_level(mut commands: Commands) {
    let level = Map::from_ron("assets/maps/default.ron").expect("failed to load assets/maps/default.ron");
    commands.insert_resource(level);
}

fn spawn_sp_weapons(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    asset_server: Res<AssetServer>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
    mut net_ids: ResMut<NetworkIDResource>,
    level: Res<Map>,
) {
    for req in &level.initial_spawns {
        let cmd = SpawnCommand {
            net_id: NetworkID(net_ids.next()),
            position: req.position,
            rotation: req.rotation,
            starting_velocity: Vec3::ZERO,
            server_tick: 0,
            kind: req.kind.clone(),
            owned: false,
        };
        match req.kind {
            GameObjectKind::HailMary => {
                hail_mary::spawn_from_command(cmd, &mut commands, &mut world, &asset_server, &hull_assets);
            }
            GameObjectKind::Rifle => {
                rifle::spawn_from_command(cmd, &mut commands, &mut world, &asset_server, &hull_assets);
            }
            GameObjectKind::Shotgun => {
                shotgun::spawn_from_command(cmd, &mut commands, &mut world, &asset_server, &hull_assets);
            }
            _ => {}
        }
    }
}

// PLACEHOLDER: remove when LevelPlugin handles singleplayer pawn spawning
fn spawn_local_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<NetworkIDResource>,
    camera: Query<Entity, With<Camera3d>>,
    level: Res<Map>,
) {
    let spawn = level.spawn_points.first();
    let cmd = SpawnCommand {
        net_id: NetworkID(net_ids.next()),
        position: spawn.map_or(Vec3::new(0.0, 5.0, 0.0), |s| s.position),
        rotation: spawn.map_or(Quat::IDENTITY, |s| s.rotation),
        starting_velocity: Vec3::ZERO,
        server_tick: 0,
        kind: GameObjectKind::Biped,
        owned: true,
    };
    let entity = biped::spawn_from_command(&cmd, true, &mut commands, &mut meshes, &mut materials, &mut world, camera.single().ok());
    commands.entity(entity).insert(Possessed::new(128));
}

fn load_skybox(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    level: Res<Map>,
    camera: Query<Entity, With<Camera3d>>,
) {
    let (Some(path), Ok(cam)) = (&level.skybox, camera.single()) else { return };
    commands.entity(cam).insert(Skybox {
        image: asset_server.load(path.clone()),
        brightness: level.skybox_brightness,
        ..default()
    });
}

fn cleanup_world(
    mut commands: Commands,
    camera: Query<Entity, With<Camera3d>>,
    roots: Query<Entity, (With<Transform>, Without<Camera3d>, Without<ChildOf>)>,
) {
    if let Ok(cam) = camera.single() {
        commands.entity(cam).remove_parent_in_place();
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}

fn connect(mut quic: ResMut<QuicManager>, mut client: ResMut<QuinnetClient>, addr: Res<ServerAddr>) {
    quic.connect(&mut client, addr.0);
}

fn disconnect(
    mut quic: ResMut<QuicManager>,
    mut client: ResMut<QuinnetClient>,
    mut local_net_id: ResMut<LocalNetworkID>,
    mut pending: ResMut<PendingReconciliation>,
    mut hosted: ResMut<HostedServer>,
) {
    if let Some(conn) = client.get_connection_mut() {
        let _ = conn.disconnect();
    }
    hosted.stdin = None; // close stdin first so server gets EOF
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
    }
    if let Some(id) = hosted.beacon_id.lock().unwrap().take() {
        std::thread::spawn(move || {
            let _ = ureq::delete(&format!("{}/lobbies/{id}", common::config::BEACON_URL)).call();
        });
    }
    quic.inbound.clear();
    quic.client_connected = false;
    local_net_id.0 = None;
    pending.0 = None;
}

fn remove_script(mut commands: Commands) {
    commands.remove_resource::<game_objects::scripting::ScriptConfig>();
}


/// handles messages coming in from the server
/// called by quic on FixedPostUpdate
fn on_message(
    mut quic: ResMut<QuicManager>,
    mut gui: ResMut<GuiState>,
    mut sp: SpawnParams<'_, '_>,
    mut world: ResMut<PhysicsWorld>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut net_stats: ResMut<NetworkStats>,
    time: Res<Time>,
    mut local_net_id: ResMut<LocalNetworkID>,
    mut hit_beams: ResMut<HitBeams>,
    mut possessed_q: Query<(&mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    networked: Query<(Entity, &NetworkID)>,
    mut health_q: Query<(&NetworkID, &mut Health)>,
    camera: Query<Entity, With<Camera3d>>,
    pitch_pivot_q: Query<Entity, With<PitchPivot>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::SpawnCommand(cmd) => {
                match cmd.kind {
                    GameObjectKind::Biped => {
                        let owned = cmd.owned;
                        if owned {
                            ticker.tick = cmd.server_tick;
                            local_net_id.0 = Some(cmd.net_id.clone());
                        }
                        let cam = if owned { camera.single().ok() } else { None };
                        let entity = biped::spawn_from_command(&cmd, owned, &mut sp.commands, &mut sp.meshes, &mut sp.materials, &mut world, cam);
                        if owned {
                            sp.commands.entity(entity).insert(Possessed::new(128));
                        }
                    }
                    GameObjectKind::Rifle => {
                        rifle::spawn_from_command(cmd, &mut sp.commands, &mut world, &sp.asset_server, &sp.hull_assets);
                    }
                    GameObjectKind::Shotgun => {
                        shotgun::spawn_from_command(cmd, &mut sp.commands, &mut world, &sp.asset_server, &sp.hull_assets);
                    }
                    GameObjectKind::HailMary => {
                        hail_mary::spawn_from_command(cmd, &mut sp.commands, &mut world, &sp.asset_server, &sp.hull_assets);
                    }
                    GameObjectKind::Spaceship => {
                        warn!("Spaceship spawn not yet implemented on client");
                    }
                    _ => {
                        warn!("client: type not implemented");
                    }
                }
            }
            MsgType::DespawnCommand(net_id) => {
                let is_local = local_net_id.0.as_ref() == Some(&net_id);
                for (entity, nid) in networked.iter() {
                    if *nid == net_id {
                        if is_local {
                            // Detach the camera before despawning so the hierarchy
                            // doesn't take it with it. The server will re-spawn us.
                            if let Ok(cam) = camera.single() {
                                sp.commands.entity(cam).remove_parent_in_place();
                            }
                            // Despawn held weapon viewmodels explicitly so the
                            // recursive pawn despawn doesn't hit them a second time.
                            if let Ok((mut slots, _)) = possessed_q.single_mut() {
                                for i in 0..2 {
                                    let ent = slots.slots[i].1.take();
                                    slots.slots[i].0 = None;
                                    if let Some(w) = ent {
                                        sp.commands.entity(w).despawn();
                                    }
                                }
                            }
                            local_net_id.0 = None;
                        } else if let Ok((mut slots, _)) = possessed_q.single_mut() {
                            // If this was a weapon viewmodel in a slot, clear the slot.
                            for i in 0..2 {
                                if slots.slots[i].0.as_ref() == Some(&net_id) {
                                    slots.slots[i] = (None, None);
                                }
                            }
                        }
                        sp.commands.entity(entity).despawn();
                        break;
                    }
                }
            }
            MsgType::Disconnected => {
                next_state.set(GameState::MainMenu);
            }
            MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
                let is_local = local_net_id.0.as_ref() == Some(&carrier_net_id);
                let weapon_entity = networked.iter().find(|(_, nid)| *nid == &weapon_id).map(|(e, _)| e);
                let Some(weapon_entity) = weapon_entity else { continue };
                world.set_body_enabled(weapon_entity, false);
                if is_local {
                    let (slot_result, pivot_e) = if let Ok((mut slots, biped)) = possessed_q.single_mut() {
                        let slot_result = slots.slots.iter().position(|s| s.0.is_none()).map(|idx| {
                            slots.slots[idx] = (Some(weapon_id.clone()), Some(weapon_entity));
                            (idx, slots.active == idx)
                        });
                        (slot_result, biped.pitch_pivot)
                    } else { (None, None) };
                    if let (Some((slot_idx, is_active)), Some(pivot)) = (slot_result, pivot_e) {
                        sp.commands.entity(weapon_entity)
                            .remove::<(RigidBodyHandleComponenet, Interactable)>()
                            .set_parent_in_place(pivot)
                            .insert(viewmodel_offset(slot_idx))
                            .insert(if is_active { Visibility::Inherited } else { Visibility::Hidden });
                    }
                } else {
                    sp.commands.entity(weapon_entity)
                        .remove::<Interactable>()
                        .insert(Visibility::Hidden);
                }
            }
            MsgType::WeaponDrop(weapon_id, carrier_id, drop_pos) => {
                let is_carrier = local_net_id.0.as_ref() == Some(&carrier_id);
                let weapon_entity = networked.iter().find(|(_, nid)| *nid == &weapon_id).map(|(e, _)| e);
                let Some(weapon_entity) = weapon_entity else { continue };
                world.teleport_body(weapon_entity, drop_pos);
                world.set_body_enabled(weapon_entity, true);
                if is_carrier {
                    if let Ok((mut slots, _)) = possessed_q.single_mut() {
                        for i in 0..2 {
                            if slots.slots[i].0.as_ref() == Some(&weapon_id) {
                                slots.slots[i] = (None, None);
                            }
                        }
                    }
                    let handle = world.entity_to_handle.get(&weapon_entity).copied();
                    sp.commands.entity(weapon_entity)
                        .remove_parent_in_place()
                        .insert((Interactable { range: 2.0 }, Visibility::Inherited));
                    if let Some(h) = handle {
                        sp.commands.entity(weapon_entity).insert(RigidBodyHandleComponenet(h));
                    }
                } else {
                    sp.commands.entity(weapon_entity).insert((Interactable { range: 2.0 }, Visibility::Inherited));
                }
            }
            MsgType::HitResult(origin, end, _hit_net_id) => {
                hit_beams.0.push((origin.into(), end.into(), 0.3));
            }
            MsgType::Fire(_, origin, velocity, _tick) => {
                hail_mary::spawn_projectile(Vec3::from(origin), Vec3::from(velocity), &mut sp.commands, &mut world, 0.0, None);
            }
            MsgType::HealthUpdate(net_id, current) => {
                for (nid, mut health) in health_q.iter_mut() {
                    if *nid == net_id {
                        health.current = current;
                        break;
                    }
                }
            }
            MsgType::Pong(text) => {
                debug_println!("Client: Got PONG \"{text}\"");
                gui.push_log(format!("pong: {text}"));
            }
            MsgType::ChatMessage(sender, text) => {
                gui.push_log(format!("[{sender}] {text}"));
            }
            MsgType::TimePong(bits) => {
                net_stats.record_pong(bits, time.elapsed_secs_f64());
            }
            // Keep only the newest snapshot; reconciliation happens next FixedPreUpdate.
            MsgType::State(st) => {
                net_stats.record_state_tick(ticker.tick, st.tick, common::config::FIXED_TICK_RATE);
                pending.0 = Some(st);
            }
            MsgType::FileData(name, compressed) => {
                if name == "map.ron" {
                    match Map::from_compressed_ron(&compressed) {
                        Some(level) => { sp.commands.insert_resource(level); }
                        None => eprintln!("FileData: failed to parse map.ron"),
                    }
                } else if name == "gametype.lua" {
                    match zstd::stream::decode_all(compressed.as_slice()) {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(src) => {
                                sp.commands.insert_resource(game_objects::scripting::ScriptConfig {
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
            }
            MsgType::FlashlightState(net_id, on) => {
                // Local player is updated immediately by toggle_flashlight; skip to avoid flicker.
                if local_net_id.0.as_ref() == Some(&net_id) { continue; }
                if let Some((entity, _)) = networked.iter().find(|(_, nid)| *nid == &net_id) {
                    if let Ok(children) = sp.entity_children.get(entity) {
                        for child in children.iter() {
                            if let Ok(mut vis) = sp.lights.get_mut(child) {
                                *vis = if on { Visibility::Inherited } else { Visibility::Hidden };
                            }
                        }
                    }
                }
            }
            other => debug_println!("Client: Got unhandled message: {other:?}"),
        }
    }
}



/// Runs after `gather_pawn_input` (which pushed the input into Possessed.buffer) but before
/// `move_bipeds` (which consumes it). Peeks at the newest buffered input, stamps it with the
/// current tick, records it in the history for later replay, and sends it to the server.
fn send_pawn_input(
    mut quic: ResMut<QuicManager>,
    tick: Res<Ticker>,
    mut pawns: Query<&mut Possessed>,
) {
    let Ok(mut possessed) = pawns.single_mut() else { return };
    let Some(&input) = possessed.peek_newest() else { return };
    let t = tick.tick;
    possessed.record_input(t, input);
    // Keep ~2 seconds of history
    possessed.prune_input_history(t.saturating_sub(128));
    quic.send(
        SendTarget::All,
        Channel::Unreliable,
        &MsgType::Input(PawnInputMessage { input, tick: t }),
    );
}


/// Replaces a weapon entity's default cuboid collider with its convex hull once the asset loads.
/// Triggered by `spawn_from_command` attaching a `Handle<ConvexHullAsset>` to the entity.
fn swap_weapon_hull_colliders(
    mut commands: Commands,
    pending: Query<(Entity, &PendingHullCollider, &RigidBodyHandleComponenet)>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
    mut world: ResMut<PhysicsWorld>,
) {
    for (entity, hull_handle, body_handle) in pending.iter() {
        let Some(hull) = hull_assets.get(&hull_handle.0) else { continue };
        let hull_collider = hull.0.clone();
        let existing: Vec<rapier3d::prelude::ColliderHandle> = world.rigid_body_set.get(body_handle.0)
            .map(|rb| rb.colliders().to_vec())
            .unwrap_or_default();
        for ch in existing {
            let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
            collider_set.remove(ch, island_manager, rigid_body_set, true);
        }
        {
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
            collider_set.insert_with_parent(hull_collider, body_handle.0, rigid_body_set);
        }
        commands.entity(entity).remove::<PendingHullCollider>();
    }
}

/// Copies the pending server snapshot into LastServerState before reconcile consumes it.
fn snapshot_server_state(pending: Res<PendingReconciliation>, mut last: ResMut<LastServerState>) {
    if let Some(st) = &pending.0 {
        last.0 = Some(st.clone());
    }
}


/// Draws a point gizmo at each body position from the latest server state.
fn draw_server_state(last: Res<LastServerState>, mut gizmos: Gizmos) {
    let Some(state) = &last.0 else { return };
    for body in state.bodies.values() {
        let pos: Vec3 = body.position.into();
        gizmos.sphere(pos, 0.15, Color::srgb(1.0, 0.2, 0.2));
    }
}

/// Draws active hitscan beams as gizmos and ticks down their lifetime.
fn draw_hit_beams(
    mut beams: ResMut<HitBeams>,
    mut gizmos: Gizmos,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    beams.0.retain_mut(|(origin, end, remaining)| {
        gizmos.line(*origin, *end, Color::srgb(1.0, 0.8, 0.0));
        *remaining -= dt;
        *remaining > 0.0
    });
}

/// On Y press, toggles the local player's flashlight and sends FlashlightToggle to the server.
fn toggle_flashlight(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Res<EguiWantsInput>,
    local_net_id: Res<LocalNetworkID>,
    possessed_q: Query<&BipedPawnComponent, With<Possessed>>,
    pitch_pivot: Query<&Children, With<PitchPivot>>,
    mut lights: Query<&mut Visibility, With<SpotLight>>,
    mut quic: ResMut<QuicManager>,
    mut on: Local<bool>,
) {
    if egui_wants.wants_any_input() || !keyboard.just_pressed(KeyCode::KeyY) { return; }
    *on = !*on;
    if let Ok(biped) = possessed_q.single() {
        if let Some(pitch_e) = biped.pitch_pivot {
            if let Ok(children) = pitch_pivot.get(pitch_e) {
                for child in children.iter() {
                    if let Ok(mut vis) = lights.get_mut(child) {
                        *vis = if *on { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
        }
    }
    if local_net_id.0.is_some() {
        quic.send(SendTarget::All, Channel::Ordered, &MsgType::FlashlightToggle);
    }
}

fn nearest_interactable(
    player_pos: Vec3,
    interactables: &Query<(Entity, &RigidBodyHandleComponenet, &NetworkID), With<Interactable>>,
    world: &PhysicsWorld,
) -> Option<(Entity, NetworkID)> {
    interactables.iter()
        .filter_map(|(e, handle, net_id)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            let t = rb.position().translation;
            let dist = Vec3::new(t.x, t.y, t.z).distance(player_pos);
            (dist < 3.0).then_some((e, net_id.clone(), dist))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(e, net_id, _)| (e, net_id))
}

fn interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Res<EguiWantsInput>,
    state: Res<State<GameState>>,
    player: Query<(&RigidBodyHandleComponenet, &Transform, &BipedPawnComponent), With<Possessed>>,
    interactables: Query<(Entity, &RigidBodyHandleComponenet, &NetworkID), With<Interactable>>,
    mut world: ResMut<PhysicsWorld>,
    mut possessed_q: Query<&mut WeaponSlots, With<Possessed>>,
    mut commands: Commands,
    mut quic: ResMut<QuicManager>,
) {
    if egui_wants.wants_any_input() || !keyboard.just_pressed(KeyCode::KeyF) { return; }
    let Ok((h, t, biped)) = player.single() else { return };
    let player_pos = world.rigid_body_set.get(h.0)
        .map(|rb| { let p = rb.position().translation; Vec3::new(p.x, p.y, p.z) })
        .unwrap_or(t.translation);
    let Some((weapon_entity, weapon_net_id)) = nearest_interactable(player_pos, &interactables, &world) else { return };
    match state.get() {
        GameState::SinglePlayer => {
            let Some(pivot_entity) = biped.pitch_pivot else { return };
            let Ok(mut slots) = possessed_q.single_mut() else { return };
            let Some(slot_idx) = slots.slots.iter().position(|s| s.0.is_none()) else { return };
            slots.slots[slot_idx] = (Some(weapon_net_id), Some(weapon_entity));
            let is_active = slots.active == slot_idx;
            world.set_body_enabled(weapon_entity, false);
            commands.entity(weapon_entity)
                .remove::<(RigidBodyHandleComponenet, Interactable)>()
                .set_parent_in_place(pivot_entity)
                .insert(viewmodel_offset(slot_idx))
                .insert(if is_active { Visibility::Inherited } else { Visibility::Hidden });
        }
        GameState::Multiplayer => {
            quic.send(SendTarget::All, Channel::Ordered, &MsgType::Interact(weapon_net_id));
        }
        _ => {}
    }
}

/// Local-space transform offset for the viewmodel depending on which slot it's in.
fn viewmodel_offset(slot_idx: usize) -> Transform {
    match slot_idx {
        0 => Transform::from_xyz(0.3, -0.25, -0.5),
        _ => Transform::from_xyz(-0.3, -0.25, -0.5),
    }
}

/// Scroll wheel switches the active weapon slot and toggles viewmodel visibility.
fn switch_weapon_slot(
    scroll: Res<AccumulatedMouseScroll>,
    mut pawn: Query<&mut WeaponSlots, With<Possessed>>,
    mut visibility: Query<&mut Visibility>,
) {
    let delta: f32 = scroll.delta.y;
    if delta == 0.0 { return; }
    let Ok(mut slots) = pawn.single_mut() else { return };
    let prev = slots.active;
    slots.active = if delta > 0.0 {
        (slots.active + 1) % 2
    } else {
        slots.active.checked_sub(1).unwrap_or(1)
    };
    if slots.active == prev { return; }
    if let Some(e) = slots.slots[prev].1 {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Hidden; }
    }
    if let Some(e) = slots.slots[slots.active].1 {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Inherited; }
    }
}
