// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use common::{physics::physics_world::*};
use common::net::{
    quic::*,
    runtime::{TokioRuntime, TokioRuntimePlugin},
    message::{MsgType, SimulationState},
};
use common::tick::{increment_tick, Ticker};
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;
use common::net::message::NetworkIDResource;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(LogPlugin {
                level: Level::ERROR,
                ..default()
            })
    );
    // .add_plugins(LevelPlugin)

    app.add_plugins(MasterPlugin);


    // NETWORKING

    // keeps track of current incrementing NetworkID number
    app.insert_resource(NetworkIDResource::default());

    app.add_systems(Startup, start_server)
    .add_systems(Update, on_message)
    .add_systems(FixedUpdate, on_message);
    app.add_systems(FixedUpdate, (increment_tick, broadcast_tick).chain());

    println!("starting server...\n");
    app.run();
}

fn start_server(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.start_server(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}
//
// fn on_connect(mut reader: MessageReader<ConnectionEstablished>) {
//     for evt in reader.read() {
//         println!("Client connected: {:?}", evt.conn_id);
//         // spawn player entity, assign NetworkID, etc.
//     }
// }
//
// fn on_disconnect(mut reader: MessageReader<ConnectionLost>) {
//     for evt in reader.read() {
//         println!("Client disconnected: {:?}", evt.conn_id);
//         // despawn player entity, etc.
//     }
// }

fn on_message(
    mut inbound: ResMut<InboundQueue>,
    mut outbound: ResMut<OutboundQueue>,
) {
    while let Some(msg) = inbound.0.pop_front() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::Ping(text)) => {
                println!("Got ping: {text}");
                outbound.send(
                    SendTarget::One(msg.conn_id),
                    Channel::Ordered,
                    &MsgType::Pong(text),
                );
            }
            Ok(MsgType::ChatMessage(sender, text)) => println!("[{sender}] {text}"),
            Ok(other) => println!("Unhandled: {other:?}"),
            Err(e) => eprintln!("Deserialize error: {e}"),
        }
    }
}

fn broadcast_tick(
    mut outbound: ResMut<OutboundQueue>,
    tick: Res<Ticker>
) {
    let msg = MsgType::State(SimulationState { tick: tick.tick, bodies: Default::default() });
    outbound.send(SendTarget::All, Channel::Unreliable, &msg);
}