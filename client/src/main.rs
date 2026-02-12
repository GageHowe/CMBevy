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
use common::net::runtime::TokioRuntimePlugin;
use common::pawn::pawn::PawnPlugin;
use common::{level::level::*, physics::physics_world::*};
// use settings::settings::*;
use std::{collections::HashMap, net::UdpSocket, time::SystemTime};
use ui::ui::UIPlugin;
use ui::window::WindowSettingsPlugin;

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
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    .init_state::<AppState>()
    // .add_plugins(AppSettingsPlugin)
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(PhysicsPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin)
    // .add_plugins(ClientNetManagerPlugin)
    .add_plugins(PawnPlugin)
    .add_plugins(TokioRuntimePlugin)
    .add_systems(Startup, spawn_camera)
    // .add_systems(Startup, connect_to_server);
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

// handle_udp runs on FixedPreUpdate
// step_physics runs on FixedUpdate
// flush_outgoing_udp runs on FixedPostUpdate
