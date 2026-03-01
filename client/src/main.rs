// client executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::window::PresentMode;
use common::camera::spawn_camera;

use common::pawn::pawn::{PawnPlugin, Possessed};
use common::pawn::biped;
use common::physics::physics_world::PhysicsWorld;
use common::net::{
    quic::*,
    runtime::TokioRuntime,
    message::{MsgType, SpawnCommand},
};
use common::ui::ui::UIPlugin;
use common::ui::window::WindowSettingsPlugin;
use common::level::level::*;
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;
use common::ui::ui::GuiState;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
enum AppState {
    #[default]
    Playing,
}

fn main() {
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
        .add_systems(Startup, (spawn_camera, spawn_scene));

    app.add_systems(Startup, connect);
    app.add_systems(Update, (on_message, send_chat));
    app.add_systems(FixedUpdate, (on_message, send_chat));

    println!("starting client...\n");
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

fn connect(mut quic: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    quic.connect(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_message(
    mut quic: ResMut<QuicManager>,
    mut gui: ResMut<GuiState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world: ResMut<PhysicsWorld>,
) {
    while let Some(msg) = quic.inbound.pop_front() {
        match msg.msg {
            MsgType::SpawnCommand(cmd) => spawn_pawn_client(cmd, &mut commands, &mut meshes, &mut materials, &mut world),
            MsgType::Pong(text) => {
                println!("PONG {text}");
                gui.push_log(format!("pong: {text}"));
            }
            MsgType::ChatMessage(sender, text) => {
                gui.push_log(format!("[{sender}] {text}"));
            }
            MsgType::State(_st) => {}
            other => println!("Unhandled: {other:?}"),
        }
    }
}

/// Spawns a pawn for the local player from a server SpawnCommand.
/// Change the pawn type here when needed.
fn spawn_pawn_client(
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
    let entity = biped::spawn(transform, commands, meshes, materials, world);
    commands.entity(entity).insert((cmd.net_id, Possessed::new(60)));
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
