use bevy::prelude::*;
// use bevy_renet::renet::*;
use bevy_renet::netcode::*;
use bevy_renet::renet::{ConnectionConfig, DefaultChannel};
use bevy_renet::*;
use cmbevy::core::level::level::*;
use cmbevy::core::physics::physics_world::*;
use cmbevy::core::player::player::*;
use cmbevy::core::ui::ui::UIPlugin;
use cmbevy::core::window::*;
use std::net::UdpSocket;
use std::time::SystemTime;

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
    app.add_plugins(RenetClientPlugin);

    let client = RenetClient::new(ConnectionConfig::default());
    app.insert_resource(client);

    // Setup the transport layer
    app.add_plugins(NetcodeClientPlugin);

    let authentication = ClientAuthentication::Unsecure {
        server_addr: "127.0.0.1:5000".parse().unwrap(),
        client_id: 0,
        user_data: None,
        protocol_id: 0,
    };
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
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
    }
}
