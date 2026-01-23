use bevy::prelude::*;
use bevy_renet::{
    RenetClient, RenetClientPlugin, RenetServer, RenetServerEvent, RenetServerPlugin,
    netcode::*,
    renet::{ClientId, ConnectionConfig, DefaultChannel, ServerEvent},
};
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

#[derive(Debug, Default, Resource)]
pub struct ServerLobby {
    pub players: HashMap<ClientId, Entity>,
}

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
    app.add_plugins(RenetClientPlugin);

    let client = RenetClient::new(ConnectionConfig::default());
    app.insert_resource(client);

    // Setup the transport layer
    app.add_plugins(NetcodeClientPlugin);

    let authentication = ClientAuthentication::Unsecure {
        server_addr: SERVER_ADDRESS.parse().unwrap(),
        client_id: 0,
        user_data: None,
        protocol_id: 0,
    };
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    let mut transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

    app.insert_resource(transport);

    app.add_systems(Startup, send_message_system);
    app.add_systems(Startup, receive_message_system);
    // :)
    app.run();
}

// use ruzstd here

fn send_message_system(mut client: ResMut<RenetClient>) {
    // Send a text message to the server
    client.send_message(DefaultChannel::ReliableOrdered, "server message");
}

fn receive_message_system(mut client: ResMut<RenetClient>) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        // Handle received message
        println!("client recieved message: {:?}", message)
    }
}
