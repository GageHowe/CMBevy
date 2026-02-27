// client executable

use bevy::camera::{PerspectiveProjection, Projection};
use bevy::log::{Level, LogPlugin};
use bevy::prelude::Camera3d;
use bevy::prelude::*;
use bevy::window::PresentMode;
use common::pawn::pawn::PawnPlugin;
use common::net::{
    quic::*,
    runtime::TokioRuntime,
    message::{MsgType, SpawnCommand, ObjectType},
};
use common::ui::ui::UIPlugin;
use common::ui::window::WindowSettingsPlugin;
use common::level::level::*;
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;
use common::ui::ui::GuiState;

/// Bevy message for spawn commands received from the server.
#[derive(Message, Debug, Clone)]
struct ServerSpawnCommand(SpawnCommand);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
enum AppState {
    #[default]
    Playing,
    // MainMenu,
    // PauseMenu,
}
fn main() {
    let mut app = App::new();

    app.add_plugins(
        // defaultplugins includes asset plugin
        DefaultPlugins
            .set(AssetPlugin {
                // dev: cargo sets CWD to client/, so go up to workspace root
                // release: assets/ sits next to the exe, default path works
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
                    present_mode: PresentMode::FifoRelaxed, // vsync off
                    ..default()
                }),
                ..default()
            }),
    );

    app.add_plugins(MasterPlugin) // common required plugins
    .init_state::<AppState>() // MainMenu, etc
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin)
    .add_plugins(PawnPlugin)
    .add_message::<ServerSpawnCommand>()
    .add_systems(Startup, (spawn_camera, spawn_scene));
    // app.add_systems(FixedUpdate, increment_tick);

    // NETWORKING
    app.add_systems(Startup, connect);
    app.add_systems(Update, (on_message, handle_spawn_commands, send_chat));
    app.add_systems(FixedUpdate, (on_message, send_chat));

    println!("starting client...\n");
    app.run();
}

fn spawn_scene(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            // vertical FOV in radians
            fov: 90.0_f32.to_radians(),
            ..Default::default()
        }),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn connect(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.connect(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_message(
    mut inbound: ResMut<InboundQueue>,
    mut gui: ResMut<GuiState>,
    mut spawn_events: MessageWriter<ServerSpawnCommand>,
) {
    while let Some(msg) = inbound.0.pop_front() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::Pong(text)) => {
                println!("PONG {text}");
                gui.push_log(format!("pong: {text}"));
            }
            Ok(MsgType::ChatMessage(sender, text)) => {
                gui.push_log(format!("[{sender}] {text}"));
            }
            Ok(MsgType::State(_st)) => {
            //
            }
            Ok(MsgType::SpawnCommand(cmd)) => {
                println!("Received spawn command: {cmd:?}");
                gui.push_log(format!("Spawning {:?}", cmd.kind));
                spawn_events.write(ServerSpawnCommand(cmd));
            }
            Ok(other) => println!("Unhandled: {other:?}"),
            Err(e) => eprintln!("Deserialize error: {e}"),
        }
    }
}

fn handle_spawn_commands(
    mut events: MessageReader<ServerSpawnCommand>,
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    world: ResMut<common::physics::physics_world::PhysicsWorld>,
) {
    for evt in events.read() {
        let cmd = &evt.0;
        let location = cmd.location.map(Vec3::from).unwrap_or(Vec3::ZERO);
        let transform = Transform::from_translation(location);

        match cmd.kind {
            ObjectType::Biped => {
                common::pawn::biped::spawn(transform, commands, meshes, materials, world);
                return; // these system params are moved, handle one per frame
            }
            ObjectType::Spaceship => {
                common::pawn::spaceship::spawn(transform, commands, meshes, materials, world);
                return;
            }
            _ => {
                println!("Unhandled spawn type: {:?}", cmd.kind);
            }
        }
    }
}

fn send_chat(
    mut outbound: ResMut<OutboundQueue>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::Enter) {
        outbound.send(
            SendTarget::All,
            Channel::Ordered,
            &MsgType::ChatMessage("player".into(), "hello!".into()),
        );
    }
}