// client executable

// mod client;
mod ui;
// use crate::client::{ClientNetManager, ClientNetManagerPlugin};
use bevy::camera::{PerspectiveProjection, Projection};
use bevy::log::{Level, LogPlugin};
use bevy::prelude::Camera3d;
use bevy::prelude::*;
use bevy::window::PresentMode;
// use common::net::net::MsgType;
// use common::net::quic::*;
use common::pawn::pawn::PawnPlugin;
use common::net::{
    quic::{QuicPlugin, QuicManager, OutboundMessage, InboundMessage,
           ConnectionEstablished, ConnectionLost, SendTarget, Channel},
    runtime::{TokioRuntime, TokioRuntimePlugin},
    message::MsgType,
};
use common::physics::physics_world::*;
// use settings::settings::*;
use std::{collections::HashMap, net::UdpSocket, time::SystemTime};
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;
use common::level::level::*;
use common::config::SERVER_BIND_ADDRESS;


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
    .insert_resource(Time::<Fixed>::from_hz(64.0))
    .init_state::<AppState>()
    // .add_plugins(AppSettingsPlugin)
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(PhysicsPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin)
    // .add_plugins(ClientNetManagerPlugin)
    .add_plugins(PawnPlugin)

    // NETWORKING

    .add_plugins(TokioRuntimePlugin)
    .add_plugins(QuicPlugin)
    .add_systems(Startup, connect)
    .add_systems(Update, (on_connect, on_disconnect, on_message, send_chat))

    ; println!("starting client...\n");

    app.run();
}

// fn connect_to_server(mut manager: ResMut<ClientNetManager>) {
//     manager.enqueue(MsgType::Error("HELLO".to_string()));
// }

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

fn on_connect(mut reader: MessageReader<ConnectionEstablished>) {
    for evt in reader.read() {
        println!("Connected to server as {:?}", evt.conn_id);
    }
}

fn on_disconnect(mut reader: MessageReader<ConnectionLost>) {
    for _evt in reader.read() {
        println!("Disconnected from server");
        // return to main menu, etc.
    }
}

fn on_message(mut reader: MessageReader<InboundMessage>) {
    for msg in reader.read() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::State(_state)) => {
                // apply world state from server
            }
            Ok(MsgType::ChatMessage(sender, msg)) => {
                println!("Got message: {sender}, {msg}")
            }
            Ok(other) => println!("Unhandled: {other:?}"),
            Err(e) => eprintln!("Deserialize error: {e}"),
        }
    }
}

// Example: send a chat message reliably and in order
fn send_chat(
    mut writer: MessageWriter<OutboundMessage>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if input.just_pressed(KeyCode::Enter) {
        let msg = MsgType::ChatMessage("player".into(), "hello!".into());
        writer.write(OutboundMessage {
            target: SendTarget::All,
            channel: Channel::Ordered,   // chat must arrive in order
            payload: wincode::serialize(&msg).unwrap(),
        });
    }
}
