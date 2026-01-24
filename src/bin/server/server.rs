// server executable

pub mod net;
use crate::net::ServerNetManagerPlugin;
use bevy::prelude::*;
use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};

// use cmbevy::core::config::SERVER_ADDRESS;
use cmbevy::core::{
    level::level::*,
    physics::{/*components::*,*/ physics_world::*},
    player::player::*,
};
// use std::{collections::HashMap, net::UdpSocket, time::SystemTime};

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(RenderPlugin {
        // nasty windowless workaround
        synchronous_pipeline_compilation: true,
        render_creation: RenderCreation::Automatic(WgpuSettings {
            backends: None,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    .add_plugins(PhysicsPlugin)
    .add_plugins(PlayerPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(ServerNetManagerPlugin);

    // :)
    app.run();
}
/*
mod net;

use bevy::prelude::*;
use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};
use cmbevy::core::{
    level::level::*,
    physics::physics_world::*,
    player::player::*,
};
use crate::net::ServerNetManagerPlugin;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins.set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            render_creation: RenderCreation::Automatic(WgpuSettings {
                backends: None,
                ..default()
            }),
            ..default()
        }),
    )
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    .add_plugins(PhysicsPlugin)
    .add_plugins(PlayerPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(ServerNetManagerPlugin);

    app.run();
}

*/
