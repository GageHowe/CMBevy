// client executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use common::camera::spawn_camera;

use common::level::level::*;
use common::net::{
    message::{MsgType, NetworkID, PawnInputMessage, SimulationState, SpawnCommand},
    quic::*,
};
use common::pawn::biped;
use common::pawn::pawn::{gather_pawn_input, move_bipeds, PawnPlugin, Possessed};
use common::physics::physics_world::{
    restore_snapshot, snapshot_bodies, step_physics, PhysicsBodyHandle, PhysicsWorld, RigidBodyHandle,
};
use common::ring_buffer::RingBuffer;
use common::tick::Ticker;
use common::ui::ui::UIPlugin;
use common::ui::window::WindowSettingsPlugin;
use std::net::SocketAddr;

#[derive(Resource)]
struct ServerAddr(SocketAddr);
use common::debug_println;
use common::master_plugin::MasterPlugin;
use common::ui::ui::GuiState;

/// How far our predicted position may drift from the server before we reconcile.
const RECONCILE_POS_THRESHOLD: f32 = 0.2;
/// How far our predicted velocity may drift from the server before we reconcile.
const RECONCILE_VEL_THRESHOLD: f32 = 1.0;

/// Holds the most recent server snapshot waiting to be consumed by `maybe_reconcile`.
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
        .add_plugins(LevelPlugin)
        .add_plugins(UIPlugin)
        .add_plugins(PawnPlugin)
        .insert_resource(ServerAddr(server_addr))
        .init_resource::<PendingReconciliation>()
        .init_resource::<LocalStateHistory>()
        .add_systems(Startup, (spawn_camera, spawn_scene));

    app.add_systems(Startup, connect);

    // FixedPreUpdate ordering:
    //   maybe_reconcile → gather_pawn_input → send_pawn_input → move_bipeds
    app.add_systems(FixedPreUpdate, maybe_reconcile.before(gather_pawn_input));
    app.add_systems(
        FixedPreUpdate,
        send_pawn_input.after(gather_pawn_input).before(move_bipeds),
    );

    // FixedUpdate ordering (step_physics comes from PhysicsPlugin):
    //   step_physics → record_world_state → on_message/send_chat
    app.add_systems(FixedUpdate, record_world_state.after(step_physics));
    app.add_systems(FixedUpdate, (on_message, send_chat).after(step_physics));

    debug_println!("starting client...\n");
    app.run();
}

fn spawn_scene(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        SceneRoot(asset_server.load("models/companion_cube.glb#Scene0")),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight { shadows_enabled: true, ..default() },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
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
    networked: Query<(Entity, &NetworkID)>,
    camera: Query<Entity, With<Camera3d>>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::SpawnCommand(cmd) => {
                if cmd.is_owned {
                    ticker.tick = cmd.server_tick;
                    let cam = camera.single().ok();
                    spawn_pawn_client(cmd, &mut commands, &mut meshes, &mut materials, &mut world, cam);
                } else {
                    spawn_ghost_client(cmd, &mut commands, &mut meshes, &mut materials, &mut world);
                }
            }
            MsgType::DespawnCommand(net_id) => {
                for (entity, nid) in networked.iter() {
                    if *nid == net_id {
                        world.remove_body(entity);
                        commands.entity(entity).despawn();
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

/// Spawns a pawn for the local player from a server SpawnCommand.
fn spawn_pawn_client(
    cmd: SpawnCommand,
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
    let entity = biped::spawn(transform, commands, meshes, materials, world, camera);
    commands.entity(entity).insert((cmd.net_id, Possessed::new(128)));
}

/// Spawns another player's pawn as a ghost (physics body + mesh, no Possessed/camera).
fn spawn_ghost_client(
    cmd: SpawnCommand,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
) {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = biped::spawn_ghost(transform, commands, meshes, materials, world);
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

    // 1. Restore server-known bodies to the authoritative server state.
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
            biped::apply_biped_movement(&mut world, our_handle, input);
        }
        world.step();
    }
}
