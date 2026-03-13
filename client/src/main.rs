// client executable

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::log::{Level, LogPlugin};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings};
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::input::EguiWantsInput;
use camera::spawn_camera;
use common::net::{
    message::*,
    quic::*,
};
use common::game_objects::GameObjectKind;
use common::pawn::pawn::PitchPivot;
use common::pawn::biped;
use common::pawn::pawn::*;
use common::physics::physics_world::*;
use reconciliation::{PendingReconciliation, ReconciliationPlugin};
use tick_sync::{NetworkStats, TickSyncPlugin};
use common::tick::Ticker;
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;
use common::interaction::Interactable;
use common::weapon::{rifle, shotgun, fire_weapons, spawn_from_command, FireEffect, FiredWeapons, WeaponInput, WeaponPlugin};
use common::pawn::biped::WeaponSlots;
use std::net::SocketAddr;

mod camera;
mod ui;
mod reconciliation;
mod tick_sync;
mod menu;
use menu::MenuPlugin;

#[derive(Resource)]
pub(crate) struct ServerAddr(pub SocketAddr);

/// Child process handle when we spawned a local gameserver.
#[derive(Resource, Default)]
pub(crate) struct HostedServer {
    pub child: Option<std::process::Child>,
    pub stdin: Option<std::io::BufWriter<std::process::ChildStdin>>,
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
}
use settings::SettingsPlugin;
use steam::SteamworksPlugin;
use common::debug_println;
use common::health::Health;
use common::master_plugin::MasterPlugin;
use common::level::{Map, PendingHullColliders, default_level, spawn_static_colliders, spawn_hull_colliders, load_level_scene, cleanup_level};
use ui::ui::GuiState;
mod settings;
mod steam;

/// The local player's own NetworkID, set when the server's owned SpawnCommand arrives.
#[derive(Resource, Default)]
struct LocalNetworkID(Option<NetworkID>);

/// Tracks the entity for each weapon slot viewmodel on the local player's pawn.
/// Slot index matches WeaponSlots::slots.
#[derive(Component, Default)]
struct ViewmodelSlots([Option<Entity>; 2]);

/// Active hitscan beams to draw as gizmos. Each entry is (origin, end, seconds_remaining).
#[derive(Resource, Default)]
struct HitBeams(Vec<(Vec3, Vec3, f32)>);

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

    app.add_plugins(MasterPlugin)
        .add_plugins(SteamworksPlugin)
        .add_plugins(SettingsPlugin)
        .init_state::<GameState>()
        .init_state::<UiState>()
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(MenuPlugin)
        .add_plugins(PawnPlugin)
        .add_plugins(WeaponPlugin)
        .add_plugins(ReconciliationPlugin(GameState::Multiplayer, biped::apply_biped_movement))
        .add_plugins(TickSyncPlugin(GameState::Multiplayer))
        .insert_resource(ServerAddr(server_addr))
        .init_resource::<LocalNetworkID>()
        .init_resource::<HostedServer>()
        .init_resource::<HitBeams>()
        .init_resource::<PendingHullColliders>()
        .add_systems(FixedUpdate, (step_physics, sync_physics_to_transforms).chain()
            .run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))))
        .add_systems(Startup, spawn_camera);

    app.add_systems(PreUpdate, process_inbound_client.run_if(in_state(GameState::Multiplayer)));
    app.add_systems(PostUpdate, flush_outbound.run_if(in_state(GameState::Multiplayer)));

    app.add_systems(OnEnter(GameState::SinglePlayer), (load_sp_level, spawn_local_player).chain());
    app.add_systems(OnExit(GameState::SinglePlayer), (despawn_local_player, cleanup_level, remove_script).chain());
    app.add_systems(OnEnter(GameState::Multiplayer), connect);
    app.add_systems(OnExit(GameState::Multiplayer), (disconnect, cleanup_level, remove_script).chain());

    // FixedPreUpdate ordering:
    //   maybe_reconcile → gather_pawn_input → send_pawn_input → move_bipeds
    // (maybe_reconcile registered by ReconciliationPlugin)
    app.add_systems(
        FixedPreUpdate,
        send_pawn_input
            .after(gather_pawn_input)
            .before(move_pawns::<BipedPawnComponent>(biped::apply_biped_movement))
            .run_if(in_state(GameState::Multiplayer)),
    );

    // fire_weapons<T> ticks weapon cooldowns, resets WeaponInput, and appends to FiredWeapons.
    // local_hitscan_vfx raycasts against the scene mesh for immediate client-side beam VFX.
    // .chain() ensures rifle → shotgun → vfx in order, all before step_physics.
    app.add_systems(FixedUpdate, (
        fire_weapons::<rifle::RifleComponent>(rifle::apply_rifle_fire),
        fire_weapons::<shotgun::ShotgunComponent>(shotgun::apply_shotgun_fire),
        local_hitscan_vfx,
    ).chain().before(step_physics).run_if(in_state(GameState::SinglePlayer).or(in_state(GameState::Multiplayer))));

    // FixedPostUpdate:
    //   record_world_state (ReconciliationPlugin) → on_message/send_chat
    app.add_systems(FixedPostUpdate, on_message.run_if(in_state(GameState::Multiplayer)));

    // (tick increment is FixedLast)

    app.add_systems(Update, send_interact_request.run_if(in_state(GameState::Multiplayer)));
    app.add_systems(Update, fire_weapon.run_if(in_state(GameState::Multiplayer)));
    app.add_systems(Update, switch_weapon_slot);
    app.add_systems(Update, draw_hit_beams);
    app.add_systems(Update, (spawn_static_colliders, load_level_scene)
        .run_if(resource_added::<Map>));
    app.add_systems(Update, spawn_hull_colliders);
    app.add_systems(Update, draw_planet_radii);

    debug_println!("starting client...\n");
    app.run();
}

/// Drains FiredWeapons and raycasts against the scene mesh for immediate client-side beam VFX.
/// Viewmodel entities (held weapons) are excluded so the gun doesn't block its own shot.
fn local_hitscan_vfx(
    mut fired: ResMut<FiredWeapons>,
    mut hit_beams: ResMut<HitBeams>,
    mut ray_cast: MeshRayCast,
    viewmodels: Query<&ViewmodelSlots>,
) {
    let excluded: Vec<Entity> = viewmodels.iter()
        .flat_map(|slots| slots.0.iter().filter_map(|e| *e))
        .collect();
    for (_, effect) in fired.0.drain(..) {
        let FireEffect::Hitscan { origin, direction, range, .. } = effect;
        let Ok(dir) = Dir3::new(direction) else { continue };
        let hits = ray_cast.cast_ray(
            Ray3d::new(origin, dir),
            &MeshRayCastSettings { filter: &|e| !excluded.contains(&e), ..default() },
        );
        let end = hits.iter()
            .find(|(_, hit)| hit.distance <= range)
            .map(|(_, hit)| hit.point)
            .unwrap_or(origin + direction * range);
        hit_beams.0.push((origin, end, 0.3));
    }
}

fn load_sp_level(mut commands: Commands) {
    let level = Map::from_ron("assets/maps/default.ron")
        .unwrap_or_else(|_| default_level());
    commands.insert_resource(level);
}

// PLACEHOLDER: remove when LevelPlugin handles singleplayer pawn spawning
fn spawn_local_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<NetworkIDResource>,
    camera: Query<Entity, With<Camera3d>>,
) {
    let cmd = SpawnCommand {
        net_id: NetworkID(net_ids.get_next_free_id()),
        position: Vec3::new(0.0, 5.0, 0.0),
        rotation: Quat::IDENTITY,
        starting_velocity: Vec3::ZERO,
        server_tick: 0,
        kind: GameObjectKind::Biped,
        owned: true,
    };
    let entity = biped::spawn_from_command(&cmd, true, &mut commands, &mut meshes, &mut materials, &mut world, camera.single().ok());
    commands.entity(entity).insert((Possessed::new(128), ViewmodelSlots::default()));
}

// PLACEHOLDER: remove when LevelPlugin handles singleplayer pawn spawning
fn despawn_local_player(
    mut commands: Commands,
    camera: Query<Entity, With<Camera3d>>,
    pawns: Query<Entity, With<BipedPawnComponent>>,
) {
    if let Ok(cam) = camera.single() {
        commands.entity(cam).remove_parent_in_place();
    }
    for entity in pawns.iter() {
        commands.entity(entity).despawn();
    }
}

fn connect(mut quic: ResMut<QuicManager>, mut client: ResMut<QuinnetClient>, addr: Res<ServerAddr>) {
    quic.connect(&mut client, addr.0);
}

fn disconnect(
    mut commands: Commands,
    mut quic: ResMut<QuicManager>,
    mut client: ResMut<QuinnetClient>,
    mut local_net_id: ResMut<LocalNetworkID>,
    mut pending: ResMut<PendingReconciliation>,
    mut hosted: ResMut<HostedServer>,
    networked: Query<Entity, With<NetworkID>>,
    camera: Query<Entity, With<Camera3d>>,
) {
    if let Some(conn) = client.get_connection_mut() {
        let _ = conn.disconnect();
    }
    hosted.stdin = None; // close stdin first so server gets EOF
    if let Some(mut child) = hosted.child.take() {
        let _ = child.kill();
    }
    quic.inbound.clear();
    quic.client_connected = false;
    local_net_id.0 = None;
    pending.0 = None;
    if let Ok(cam) = camera.single() {
        commands.entity(cam).remove_parent_in_place();
    }
    // Detach all networked entities from their parents first.
    // Weapon viewmodels are children of PitchPivot (which is a child of the pawn).
    // Without this, despawning the pawn recursively also despawns the weapon, then
    // the explicit loop below tries to despawn it a second time → Bevy warning.
    for entity in networked.iter() {
        commands.entity(entity).remove_parent_in_place();
    }
    for entity in networked.iter() {
        commands.entity(entity).despawn();
    }
}

fn remove_script(mut commands: Commands) {
    commands.remove_resource::<common::scripting::RhaiScriptConfig>();
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
    mut possessed_q: Query<(&mut WeaponSlots, &mut ViewmodelSlots), With<Possessed>>,
    networked: Query<(Entity, &NetworkID)>,
    mut health_q: Query<(&NetworkID, &mut Health)>,
    camera: Query<Entity, With<Camera3d>>,
    pitch_pivot: Query<Entity, With<PitchPivot>>,
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
                            sp.commands.entity(entity).insert((Possessed::new(128), ViewmodelSlots::default()));
                        }
                    }
                    GameObjectKind::Rifle => {
                        spawn_from_command::<rifle::RifleComponent>(cmd, rifle::spawn, &mut sp.commands, &mut world, &sp.asset_server);
                    }
                    GameObjectKind::Shotgun => {
                        spawn_from_command::<shotgun::ShotgunComponent>(cmd, shotgun::spawn, &mut sp.commands, &mut world, &sp.asset_server);
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
                            if let Ok((mut slots, mut viewmodels)) = possessed_q.single_mut() {
                                for i in 0..2 {
                                    slots.slots[i] = None;
                                    if let Some(w) = viewmodels.0[i].take() {
                                        sp.commands.entity(w).despawn();
                                    }
                                }
                            }
                            local_net_id.0 = None;
                        } else if let Ok((mut slots, mut viewmodels)) = possessed_q.single_mut() {
                            // If this was a weapon viewmodel in a slot, clear the slot.
                            for i in 0..2 {
                                if slots.slots[i].as_ref() == Some(&net_id) {
                                    slots.slots[i] = None;
                                    viewmodels.0[i] = None;
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
                if is_local {
                    // Find first empty slot, assign weapon, attach as viewmodel.
                    let slot_result = if let Ok((mut slots, mut viewmodels)) = possessed_q.single_mut() {
                        let slot_idx = slots.slots.iter().position(|s| s.is_none());
                        if let Some(idx) = slot_idx {
                            slots.slots[idx] = Some(weapon_id.clone());
                            viewmodels.0[idx] = Some(weapon_entity);
                            Some((idx, slots.active == idx))
                        } else { None }
                    } else { None };
                    if let (Some((slot_idx, is_active)), Ok(pivot)) = (slot_result, pitch_pivot.single()) {
                        let offset = viewmodel_offset(slot_idx);
                        sp.commands.entity(weapon_entity)
                            .remove::<(PhysicsBodyHandle, Interactable)>()
                            .set_parent_in_place(pivot)
                            .insert(offset)
                            .insert(if is_active { Visibility::Inherited } else { Visibility::Hidden });
                    }
                } else {
                    sp.commands.entity(weapon_entity).despawn();
                }
            }
            MsgType::HitResult(origin, end, _hit_net_id) => {
                hit_beams.0.push((origin.into(), end.into(), 0.3));
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
                } else if name == "gametype.rhai" {
                    match zstd::stream::decode_all(compressed.as_slice()) {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(src) => {
                                sp.commands.insert_resource(common::scripting::RhaiScriptConfig {
                                    path: String::new(),
                                    is_server: false,
                                    source: Some(src),
                                });
                            }
                            Err(e) => eprintln!("FileData: gametype.rhai not valid utf8: {e}"),
                        },
                        Err(e) => eprintln!("FileData: failed to decompress gametype.rhai: {e}"),
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

/// On left-click while holding a weapon, sends a Fire message to the server and
/// sets WeaponInput on the weapon entity so fire_weapons<T> predicts the shot locally.
fn fire_weapon(
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Res<EguiWantsInput>,
    pawn: Query<(&WeaponSlots, &ViewmodelSlots), With<Possessed>>,
    pitch_pivot: Query<&GlobalTransform, With<PitchPivot>>,
    mut quic: ResMut<QuicManager>,
    mut weapon_inputs: Query<&mut WeaponInput>,
) {
    if egui_wants.wants_any_input() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok((slots, viewmodels)) = pawn.single() else { return };
    let Some(weapon_net_id) = slots.slots[slots.active].clone() else { return };
    let Some(weapon_entity) = viewmodels.0[slots.active] else { return };
    let Ok(gt) = pitch_pivot.single() else { return };

    let (_, rotation, origin) = gt.to_scale_rotation_translation();
    let direction = rotation * Vec3::NEG_Z;

    // Set input for client-side prediction (fire_weapons<T> reads this next FixedUpdate).
    if let Ok(mut w_input) = weapon_inputs.get_mut(weapon_entity) {
        w_input.fire = true;
        w_input.origin = origin;
        w_input.aim_dir = direction;
    }

    quic.send(
        SendTarget::All,
        Channel::Unreliable,
        &MsgType::Fire(weapon_net_id, origin.into(), direction.into()),
    );
}

fn draw_planet_radii(
    planets: Query<(&common::game_objects::planet::PlanetBehaviorComponent, &GlobalTransform)>,
    mut gizmos: Gizmos,
) {
    for (planet, gt) in planets.iter() {
        let pos = gt.translation();
        if planet.inner_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.inner_radius as f32, Color::srgb(0.8, 0.2, 0.2));
        }
        if planet.snap_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.snap_radius as f32, Color::srgb(0.9, 0.8, 0.1));
        }
        if planet.gravity_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.gravity_radius as f32, Color::srgb(0.2, 0.8, 0.2));
        }
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

/// On F press, finds the nearest interactable within range and sends an Interact request.
fn send_interact_request(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Res<EguiWantsInput>,
    player: Query<&Transform, With<Possessed>>,
    interactables: Query<(&Transform, &NetworkID), With<Interactable>>,
    mut quic: ResMut<QuicManager>,
) {
    if egui_wants.wants_any_input() || !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }
    let Ok(player_transform) = player.single() else { return };
    let player_pos = player_transform.translation;

    let nearest = interactables
        .iter()
        .filter(|(t, _)| t.translation.distance(player_pos) < 2.0)
        .min_by(|(a, _), (b, _)| {
            a.translation.distance(player_pos)
                .partial_cmp(&b.translation.distance(player_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some((_, net_id)) = nearest {
        quic.send(SendTarget::All, Channel::Ordered, &MsgType::Interact(net_id.clone()));
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
    mut pawn: Query<(&mut WeaponSlots, &ViewmodelSlots), With<Possessed>>,
    mut visibility: Query<&mut Visibility>,
) {
    let delta: f32 = scroll.delta.y;
    if delta == 0.0 { return; }
    let Ok((mut slots, viewmodels)) = pawn.single_mut() else { return };
    let prev = slots.active;
    slots.active = if delta > 0.0 {
        (slots.active + 1) % 2
    } else {
        slots.active.checked_sub(1).unwrap_or(1)
    };
    if slots.active == prev { return; }
    if let Some(e) = viewmodels.0[prev] {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Hidden; }
    }
    if let Some(e) = viewmodels.0[slots.active] {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Inherited; }
    }
}
