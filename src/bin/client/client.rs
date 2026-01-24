// client executable

mod net;
use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use cmbevy::core::{
    level::level::*,
    physics::{components::*, physics_world::*},
    player::player::*,
    settings::settings::*,
    ui::ui::UIPlugin,
    window::*,
};

use std::{collections::HashMap, net::UdpSocket, time::SystemTime};

use crate::net::ClientNetManagerPlugin;
// use ruzstd::decoding::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, States, Default)]
enum AppState {
    #[default]
    MainMenu,
    // PauseMenu,
    // Playing,
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(LogPlugin {
        level: Level::WARN,
        ..default()
    }))
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    .init_state::<AppState>()
    .add_plugins(AppSettingsPlugin)
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(PhysicsPlugin)
    .add_plugins(PlayerPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin)
    .add_plugins(ClientNetManagerPlugin);

    // :)
    println!("starting server...\n");
    app.run();
}

// handle_udp runs on FixedPreUpdate
// step_physics runs on FixedUpdate
// flush_outgoing_udp runs on FixedPostUpdate
