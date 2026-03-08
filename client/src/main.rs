// client executable

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::input::EguiWantsInput;
use common::camera::spawn_camera;
use common::net::{
    message::{self, *},
    quic::*,
};
use common::pawn::pawn::PitchPivot;
use common::pawn::biped;
use common::pawn::pawn::*;
use common::pawn::biped::apply_biped_movement;
use common::physics::physics_world::*;
use common::ring_buffer::RingBuffer;
use common::tick::Ticker;
use common::ui::ui::UIPlugin;
use common::ui::window::WindowSettingsPlugin;
use common::interaction::Interactable;
use common::weapon::{rifle, shotgun, fire_weapons, FiredWeapons, WeaponInput, WeaponPlugin};
use common::pawn::biped::WeaponSlots;
use std::net::SocketAddr;

#[derive(Resource)]
struct ServerAddr(SocketAddr);
use common::debug_println;
use common::health::Health;
use common::master_plugin::MasterPlugin;
use common::ui::ui::GuiState;

// these measure max tolerance for how much error we allow before reconciling
const RECONCILE_POS_THRESHOLD: f32 = 0.2;
const RECONCILE_VEL_THRESHOLD: f32 = 1.0;

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

/// holds the most recent server snapshot waiting to be consumed by `maybe_reconcile`.
#[derive(Resource, Default)]
struct PendingReconciliation(Option<SimulationState>);

/// Ring buffer of every local physics snapshot, one per tick, for all networked bodies.
/// Used by reconciliation to restore bodies the server didn't mention.
#[derive(Resource)]
struct LocalStateHistory(RingBuffer<SimulationState>);
impl Default for LocalStateHistory {
    fn default() -> Self {
        Self(RingBuffer::new(128))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
enum AppState {
    #[default]
    Playing,
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
        .init_state::<AppState>()
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(PawnPlugin)
        .add_plugins(WeaponPlugin)
        .insert_resource(ServerAddr(server_addr))
        .init_resource::<PendingReconciliation>()
        .init_resource::<LocalStateHistory>()
        .init_resource::<LocalNetworkID>()
        .init_resource::<HitBeams>()
        .add_systems(Startup, (spawn_camera, spawn_scene));

    app.add_systems(PreUpdate, process_inbound_client);
    app.add_systems(Startup, connect);

    // FixedPreUpdate ordering:
    //   maybe_reconcile → gather_pawn_input → send_pawn_input → move_bipeds
    app.add_systems(FixedPreUpdate, maybe_reconcile.before(gather_pawn_input));
    app.add_systems(
        FixedPreUpdate,
        send_pawn_input.after(gather_pawn_input).before(move_pawns::<BipedPawnComponent>(apply_biped_movement)),
    );

    // fire_weapons<T> ticks weapon cooldowns, resets WeaponInput, and appends to FiredWeapons.
    // drain_fired_weapons discards them (VFX system can replace it in the future).
    // .chain() ensures rifle → shotgun → drain in order, all before step_physics.
    app.add_systems(FixedUpdate, (
        fire_weapons::<rifle::RifleComponent>(rifle::apply_rifle_fire),
        fire_weapons::<shotgun::ShotgunComponent>(shotgun::apply_shotgun_fire),
        drain_fired_weapons,
    ).chain().before(step_physics));

    // FixedPostUpdate:
    //   record_world_state → on_message/send_chat
    app.add_systems(FixedPostUpdate, record_world_state);
    app.add_systems(FixedPostUpdate, (on_message, send_chat));

    // (tick increment is FixedLast)

    app.add_systems(Update, send_interact_request);
    app.add_systems(Update, fire_weapon);
    app.add_systems(Update, switch_weapon_slot);
    app.add_systems(Update, draw_hit_beams);

    debug_println!("starting client...\n");
    app.run();
}

/// Discards this tick's accumulated fire effects.
/// Replace with a real VFX/audio system when ready.
fn drain_fired_weapons(mut fired: ResMut<FiredWeapons>) {
    fired.0.clear();
}

fn spawn_scene(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        SceneRoot(asset_server.load("models/companion_cube.glb#Scene0")),
        Transform::default(),
    ));
    // commands.spawn((
    //     DirectionalLight { shadows_enabled: true, ..default() },
    //     Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    // ));
}

fn connect(mut quic: ResMut<QuicManager>, mut client: ResMut<QuinnetClient>, addr: Res<ServerAddr>) {
    quic.connect(&mut client, addr.0);
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    mut gui: ResMut<GuiState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world: ResMut<PhysicsWorld>,
    mut ticker: ResMut<Ticker>,
    mut pending: ResMut<PendingReconciliation>,
    mut local_net_id: ResMut<LocalNetworkID>,
    mut hit_beams: ResMut<HitBeams>,
    mut possessed_q: Query<(&mut WeaponSlots, &mut ViewmodelSlots), With<Possessed>>,
    networked: Query<(Entity, &NetworkID)>,
    mut health_q: Query<(&NetworkID, &mut Health)>,
    camera: Query<Entity, With<Camera3d>>,
    pitch_pivot: Query<Entity, With<PitchPivot>>,
    asset_server: Res<AssetServer>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::SpawnCommand(cmd) => {
                match cmd.kind {
                    SpawnKind::Pawn(message::PawnKind::Biped { owned }) => {
                        if owned {
                            ticker.tick = cmd.server_tick;
                            local_net_id.0 = Some(cmd.net_id.clone());
                        }
                        let cam = if owned { camera.single().ok() } else { None };
                        spawn_biped_client(cmd, owned, &mut commands, &mut meshes, &mut materials, &mut world, cam);
                    }
                    SpawnKind::Weapon(message::WeaponKind::Rifle) => {
                        spawn_rifle(cmd, &mut commands, &mut world, &asset_server);
                    }
                    SpawnKind::Weapon(message::WeaponKind::Shotgun) => {
                        spawn_shotgun(cmd, &mut commands, &mut world, &asset_server);
                    }
                }
            }
            MsgType::DespawnCommand(net_id) => {
                let is_local = local_net_id.0.as_ref() == Some(&net_id);
                for (entity, nid) in networked.iter() {
                    if *nid == net_id {
                        world.remove_body(entity);
                        if is_local {
                            // Detach the camera before despawning so the hierarchy
                            // doesn't take it with it. The server will re-spawn us.
                            if let Ok(cam) = camera.single() {
                                commands.entity(cam).remove_parent_in_place();
                            }
                            local_net_id.0 = None;
                        }
                        commands.entity(entity).despawn();
                        // If this was a viewmodel in a weapon slot, clear it.
                        if let Ok((mut slots, mut viewmodels)) = possessed_q.single_mut() {
                            for i in 0..2 {
                                if slots.slots[i].as_ref() == Some(&net_id) {
                                    slots.slots[i] = None;
                                    viewmodels.0[i] = None;
                                }
                            }
                        }
                        break;
                    }
                }
            }
            MsgType::WeaponPickup(weapon_id, carrier_net_id) => {
                let is_local = local_net_id.0.as_ref() == Some(&carrier_net_id);
                let weapon_entity = networked.iter().find(|(_, nid)| *nid == &weapon_id).map(|(e, _)| e);
                let Some(weapon_entity) = weapon_entity else { continue };
                world.remove_body(weapon_entity);
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
                        commands.entity(weapon_entity)
                            .remove::<(PhysicsBodyHandle, Interactable)>()
                            .set_parent_in_place(pivot)
                            .insert(offset)
                            .insert(if is_active { Visibility::Inherited } else { Visibility::Hidden });
                    }
                } else {
                    commands.entity(weapon_entity).despawn();
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
            // Keep only the newest snapshot; reconciliation happens next FixedPreUpdate.
            MsgType::State(st) => {
                pending.0 = Some(st);
            }
            other => debug_println!("Client: Got unhandled message: {other:?}"),
        }
    }
}

/// Spawns a biped pawn from a server SpawnCommand.
/// `owned` adds Possessed/ViewmodelSlots and attaches the camera rig.
fn spawn_biped_client(
    cmd: SpawnCommand,
    owned: bool,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
    camera: Option<Entity>,
) {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = biped::spawn(transform, commands, world);
    let color = if owned { Color::srgb(0.8, 0.8, 0.8) } else { Color::srgb(0.9, 0.4, 0.1) };
    biped::add_visuals(entity, color, commands, meshes, materials);
    if owned {
        biped::setup_camera_rig(entity, camera, commands);
        commands.entity(entity).insert((cmd.net_id, Possessed::new(128), ViewmodelSlots::default()));
    } else {
        commands.entity(entity).insert(cmd.net_id);
    }
}

/// Spawns a rifle entity on the client (physics body + mesh).
fn spawn_rifle(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
) {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = rifle::spawn(transform, commands, world);
    rifle::add_visuals(entity, commands, asset_server);
    commands.entity(entity).insert(cmd.net_id);
}

fn spawn_shotgun(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
) {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = shotgun::spawn(transform, commands, world);
    shotgun::add_visuals(entity, commands, asset_server);
    commands.entity(entity).insert(cmd.net_id);
}

fn send_chat(mut quic: ResMut<QuicManager>, input: Res<ButtonInput<KeyCode>>) {
    if input.just_pressed(KeyCode::Enter) {
        quic.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::ChatMessage("player".into(), "hello!".into()),
        );
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

/// Runs after `step_physics`. Snapshots all networked bodies into the local history so that
/// `maybe_reconcile` can compare and restore them against the authoritative server state.
fn record_world_state(
    world: Res<PhysicsWorld>,
    tick: Res<Ticker>,
    mut history: ResMut<LocalStateHistory>,
    query: Query<(&NetworkID, &PhysicsBodyHandle)>,
) {
    history.0.push(snapshot_bodies(&world, tick.tick, query.iter()));
}

/// Runs at the start of FixedPreUpdate, before input is gathered.
///
/// If a server snapshot is pending:
///   1. Compare it against our locally predicted state at that tick.
///   2. If the error exceeds a threshold, restore physics state and replay all buffered
///      inputs from snapshot_tick+1 up to (but not including) the current tick.
///      Bodies in the server snapshot are restored to server-authoritative state.
///      Networked bodies the server didn't mention are restored to local predicted state.
fn maybe_reconcile(
    mut pending: ResMut<PendingReconciliation>,
    mut world: ResMut<PhysicsWorld>,
    tick: Res<Ticker>,
    history: Res<LocalStateHistory>,
    bodies: Query<(&NetworkID, &PhysicsBodyHandle, Option<&Possessed>)>,
) {
    let Some(snapshot) = pending.0.take() else { return };

    // find the locally possessed pawn, gotta find a better way to do this
    let Some((our_net_id, our_handle, possessed)) =
        bodies.iter().find_map(|(nid, h, p)| p.map(|poss| (nid, h, poss)))
    else {
        return;
    };

    // look up local prediction at the snapshot's tick
    let local_at_tick = history.0.iter().find(|s| s.tick == snapshot.tick);

    // compare locally predicted state against the server's.
    let needs_reconcile = match (
        local_at_tick.and_then(|s| s.bodies.get(our_net_id)),
        snapshot.bodies.get(our_net_id),
    ) {
        (Some(predicted), Some(server)) => {
            let pos_err = (Vec3::new(server.position.x, server.position.y, server.position.z)
                - Vec3::new(predicted.position.x, predicted.position.y, predicted.position.z))
                .length();
            let vel_err = (Vec3::new(server.linvel.x, server.linvel.y, server.linvel.z)
                - Vec3::new(predicted.linvel.x, predicted.linvel.y, predicted.linvel.z))
                .length();
            pos_err > RECONCILE_POS_THRESHOLD || vel_err > RECONCILE_VEL_THRESHOLD
        }
        // no history for this tick, always reconcile to stay correct.
        _ => true,
    };

    if !needs_reconcile {
        return;
    }

    let pairs: Vec<(NetworkID, RigidBodyHandle)> = bodies
        .iter()
        .map(|(nid, h, _)| (nid.clone(), h.0))
        .collect();

    // restore rigidbodies mentioned by the server to the authoritative server state.
    restore_snapshot(&mut world, &snapshot, &pairs);

    // 2. Restore networked bodies the server didn't mention to their local predicted state.
    if let Some(local) = local_at_tick {
        let unmentioned: Vec<(NetworkID, RigidBodyHandle)> = pairs.iter()
            .filter(|(nid, _)| !snapshot.bodies.contains_key(nid))
            .cloned()
            .collect();
        restore_snapshot(&mut world, local, &unmentioned);
    }

    // 3. Replay our inputs from snapshot_tick+1 up to (not including) current_tick.
    //    The current tick's input will be applied normally by move_bipeds right after.
    let current = tick.tick;
    for replay_tick in (snapshot.tick + 1)..current {
        if let Some(&input) = possessed.get_input(replay_tick) {
            // TODO: pass actual BipedPawnComponent when it holds state worth replaying
            biped::apply_biped_movement(&mut world, our_handle, input, &mut BipedPawnComponent);
        }
        world.step();
    }
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
