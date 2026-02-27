// server executable

use bevy::log::{Level, LogPlugin};
use bevy::prelude::*;
use std::collections::HashMap;
use common::net::{
    quic::*,
    runtime::TokioRuntime,
    message::{MsgType, NetworkID, NetworkIDResource, ObjectType, SimulationState, SpawnCommand},
};
use common::tick::{increment_tick, Ticker};
use common::config::SERVER_BIND_ADDRESS;
use common::master_plugin::MasterPlugin;

/// Tracks which connection owns which pawn entity on the server.
#[derive(Resource, Default)]
struct ClientPawnMap(HashMap<ConnectionId, Entity>);

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
    app.insert_resource(ClientPawnMap::default());

    app.add_systems(Startup, start_server)
    .add_systems(Update, (on_connect, on_disconnect, on_message))
    .add_systems(FixedUpdate, on_message);
    app.add_systems(FixedUpdate, (increment_tick, broadcast_tick).chain());

    println!("starting server...\n");
    app.run();
}

fn start_server(mut manager: ResMut<QuicManager>, runtime: Res<TokioRuntime>) {
    manager.start_server(&runtime, SERVER_BIND_ADDRESS.parse().unwrap());
}

fn on_connect(
    mut conn_events: ResMut<ConnectionEvents>,
    mut outbound: ResMut<OutboundQueue>,
    mut net_ids: ResMut<NetworkIDResource>,
    mut pawn_map: ResMut<ClientPawnMap>,
    mut commands: Commands,
) {
    while let Some(conn_id) = conn_events.0.pop_front() {
        println!("Client connected: {:?}", conn_id);

        // assign a networked biped pawn to the new client
        let net_id = NetworkID(net_ids.get_next_id());
        let cmd = SpawnCommand {
            net_id: net_id.clone(),
            kind: ObjectType::Biped,
            location: Some(bevy::math::Vec3::new(0.0, 2.0, 0.0).into()),
            velocity: None,
            rotation: None,
        };

        // spawn a server-side entity to track the pawn
        let entity = commands.spawn(net_id).id();
        pawn_map.0.insert(conn_id, entity);
        println!("Assigned pawn entity {:?} to client {:?}", entity, conn_id);

        outbound.send(
            SendTarget::One(conn_id),
            Channel::Ordered,
            &MsgType::SpawnCommand(cmd),
        );
    }
}

fn on_disconnect(
    mut disconn_events: ResMut<DisconnectionEvents>,
    mut pawn_map: ResMut<ClientPawnMap>,
    mut commands: Commands,
) {
    while let Some(conn_id) = disconn_events.0.pop_front() {
        println!("Client disconnected: {:?}", conn_id);
        if let Some(entity) = pawn_map.0.remove(&conn_id) {
            println!("Despawning pawn entity {:?}", entity);
            commands.entity(entity).despawn();
        }
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