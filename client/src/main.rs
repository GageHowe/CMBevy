// client executable

use bevy::camera::{PerspectiveProjection, Projection};
use bevy::log::{Level, LogPlugin};
use bevy::prelude::Camera3d;
use bevy::prelude::*;
use bevy::window::PresentMode;
use common::pawn::pawn::PawnPlugin;
use common::net::{
    quic::*,
    runtime::{TokioRuntime, TokioRuntimePlugin},
    message::MsgType,
};
use common::ui::ui::UIPlugin;
use common::ui::window::WindowSettingsPlugin;
use common::level::level::*;
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;
use common::tick::increment_tick;

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
        DefaultPlugins
            .set(LogPlugin {
                level: Level::WARN,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "window".into(),
                    present_mode: PresentMode::FifoRelaxed, // vsync off
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins(MasterPlugin) // common required plugins
    .init_state::<AppState>() // MainMenu, etc
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin)
    .add_plugins(PawnPlugin)
    .add_systems(Startup, spawn_camera);
    app.add_systems(FixedUpdate, increment_tick);

    // NETWORKING
    app.add_systems(Startup, connect)
    .add_systems(Update, (on_message, send_chat))
        .add_systems(FixedUpdate, (on_message, send_chat))        // client
    ; println!("starting client...\n");

    app.run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            // vertical FOV in radians
            fov: 110.0_f32.to_radians(),
            ..Default::default()
        }),
        Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn connect(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.connect(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_message(mut inbound: ResMut<InboundQueue>) {
    while let Some(msg) = inbound.0.pop_front() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::State(state)) => { /* apply world state */ }
            Ok(MsgType::ChatMessage(sender, text)) => println!("[{sender}] {text}"),
            Ok(other) => println!("Unhandled: {other:?}"),
            Err(e) => eprintln!("Deserialize error: {e}"),
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