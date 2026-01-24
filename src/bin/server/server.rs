// server executable

pub mod net;
use crate::net::ServerNetManagerPlugin;
use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};
use cmbevy::core::{
    level::level::*,
    physics::{/*components::*,*/ physics_world::*},
    player::player::*,
};
// use std::{collections::HashMap, net::UdpSocket, time::SystemTime};

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(LogPlugin {
                level: Level::ERROR,
                ..default()
            })
            .set(RenderPlugin {
                // nasty windowless workaround
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

    // :)
    println!("starting server...\n");
    app.run();
}
