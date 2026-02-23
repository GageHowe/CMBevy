// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use common::{physics::physics_world::*};
use common::net::{
    quic::{QuicPlugin, QuicManager, OutboundMessage, InboundMessage,
           ConnectionEstablished, ConnectionLost, SendTarget, Channel},
    runtime::{TokioRuntime, TokioRuntimePlugin},
    message::{MsgType, SimulationState},
};
use common::config::SERVER_BIND_ADDRESS;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(LogPlugin {
                level: Level::ERROR,
                ..default()
            })
    )
    .insert_resource(Time::<Fixed>::from_hz(64.0))
    .add_plugins(PhysicsPlugin)
    // .add_plugins(LevelPlugin)

    // NETWORKING

    .add_plugins(TokioRuntimePlugin)
    .add_plugins(QuicPlugin)
    .add_systems(Startup, start_server)
    .add_systems(Update, (on_connect, on_disconnect, on_message, broadcast_tick))






    ;
    println!("starting server...\n");
    app.run();
}



fn start_server(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.start_server(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_connect(mut reader: MessageReader<ConnectionEstablished>) {
    for evt in reader.read() {
        println!("Client connected: {:?}", evt.conn_id);
        // spawn player entity, assign NetworkID, etc.
    }
}

fn on_disconnect(mut reader: MessageReader<ConnectionLost>) {
    for evt in reader.read() {
        println!("Client disconnected: {:?}", evt.conn_id);
        // despawn player entity, etc.
    }
}

fn on_message(mut reader: MessageReader<InboundMessage>) {
    for msg in reader.read() {
        match wincode::deserialize::<MsgType>(&msg.payload) {
            Ok(MsgType::ChatMessage(addr, text)) => {
                println!("[{addr}] {text}");
            }
            Ok(MsgType::BodyState(_state)) => {
                // apply incoming body state from this client
                // debug_assert!(_sta);

            }
            Ok(other) => println!("Unhandled: {other:?}"),
            Err(e) => eprintln!("Deserialize error: {e}"),
        }
    }
}

fn broadcast_tick(
    mut writer: MessageWriter<OutboundMessage>,
    mut tick: Local<u64>,
) {
    *tick += 1;
    let msg = MsgType::State(SimulationState { tick: *tick, bodies: Default::default() });

    writer.write(OutboundMessage {
        target: SendTarget::All,
        channel: Channel::Unreliable,  // high frequency, don't need ordering
        payload: wincode::serialize(&msg).unwrap(),
    });
}