// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};
use common::{level::level::*, physics::physics_world::*};
// use std::{collections::HashMap, net::UdpSocket, time::SystemTime};
use common::net::{
    quic::{QuicPlugin, QuicManager, OutboundMessage, SendTarget, Reliability},
    runtime::{TokioRuntimePlugin, TokioRuntime},
    message::{MsgType, SimulationState}
};

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
    .insert_resource(Time::<Fixed>::from_hz(64.0))
    .add_plugins(PhysicsPlugin)
    // .add_plugins(LevelPlugin)

    // NETWORKING

    .add_plugins(TokioRuntimePlugin)
    .add_plugins(QuicPlugin)
    .add_systems(Startup, start_server)
    .add_systems(Update, broadcast_tick)





    ;
    println!("starting server...\n");
    app.run();
}


fn start_server(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.start_server(&runtime, "127.0.0.1:5000".parse().unwrap());
}

fn broadcast_tick(
    mut writer: MessageWriter<OutboundMessage>,
    mut tick: Local<u64>,
) {
    *tick += 1;

    let msg = MsgType::State(SimulationState {
        tick: *tick,
        bodies: Default::default(),
    });

    writer.write(OutboundMessage {
        target: SendTarget::All,
        payload: wincode::serialize(&msg).unwrap(),
        reliability: Reliability::Reliable,
    });
}