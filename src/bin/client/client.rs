use bevy::prelude::*;
use cmbevy::core::config::SERVER_ADDRESS;
use cmbevy::core::{
    level::level::*,
    physics::{components::*, physics_world::*},
    player::player::*,
    ui::ui::UIPlugin,
    window::*,
};

use std::{collections::HashMap, net::UdpSocket, time::SystemTime};
// use ruzstd::decoding::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_plugins(WindowSettingsPlugin)
        .add_plugins(PhysicsPlugin)
        .add_plugins(PlayerPlugin)
        .add_plugins(LevelPlugin)
        .add_plugins(UIPlugin);

    // renet setup

    // :)
    app.run();
}

// use ruzstd here
