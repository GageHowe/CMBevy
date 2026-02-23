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
use common::net::quic::*;
use common::pawn::pawn::PawnPlugin;
use common::net::{
    quic::{QuicPlugin, QuicManager, InboundMessage},
    runtime::{TokioRuntime, TokioRuntimePlugin},
    message::{MsgType, SimulationState},
};
use common::physics::physics_world::*;
// use settings::settings::*;
use std::{collections::HashMap, net::UdpSocket, time::SystemTime};
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;
use common::level::level::*;


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
    .add_systems(Update, handle_inbound)
;
    println!("starting client...\n");

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
    manager.connect(&runtime, "127.0.0.1:5000".parse().unwrap());
}

fn handle_inbound(mut reader: MessageReader<InboundMessage>) {
    for msg in reader.read() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::State(state)) => {
                println!("Tick {} received", state.tick);
            }
            Ok(other) => {
                println!("Got: {other:?}");
            }
            Err(e) => {
                eprintln!("Deserialize error: {e}");
            }
        }
    }
}

// handle_udp runs on FixedPreUpdate
// step_physics runs on FixedUpdate
// flush_outgoing_udp runs on FixedPostUpdate
