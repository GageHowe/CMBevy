// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use common::net::{
    quic::*,
    runtime::TokioRuntime,
    message::{MsgType, NetworkID, NetworkIDResource, ObjectType, SimulationState, SpawnCommand},
};
use common::tick::{increment_tick, Ticker};
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;

fn main() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(LogPlugin {
                level: Level::ERROR,
                ..default()
            })
    );

    app.add_plugins(MasterPlugin);

    // NETWORKING

    // keeps track of current incrementing NetworkID number
    app.insert_resource(NetworkIDResource::default());

    app.add_systems(Startup, start_server)
    .add_systems(Update, (on_connect, on_message))
    .add_systems(FixedUpdate, on_message);
    app.add_systems(FixedUpdate, (increment_tick, broadcast_tick).chain());

    println!("starting server...\n");
    app.run();
}

fn start_server(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.start_server(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_connect(
    mut events: MessageReader<ConnectionEvent>,
    mut outbound: ResMut<OutboundQueue>,
    mut net_ids: ResMut<NetworkIDResource>,
) {
    for evt in events.read() {
        println!("Client connected: {:?}", evt.0);

        // assign a networked biped pawn to the new client
        let net_id = NetworkID(net_ids.get_next_id());
        let cmd = SpawnCommand {
            net_id,
            kind: ObjectType::Biped,
            location: Some(bevy::math::Vec3::new(0.0, 2.0, 0.0).into()),
            velocity: None,
            rotation: None,
        };
        outbound.send(
            SendTarget::One(evt.0),
            Channel::Ordered,
            &MsgType::SpawnCommand(cmd),
        );
    }
}

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