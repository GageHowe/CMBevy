use bevy::prelude::*;
use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};
use bevy_renet::{
    RenetClient, RenetClientPlugin, RenetServer, RenetServerEvent, RenetServerPlugin,
    netcode::*,
    renet::{ClientId, ConnectionConfig, DefaultChannel, ServerEvent},
};
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
    .add_plugins(WindowSettingsPlugin)
    .add_plugins(PhysicsPlugin)
    .add_plugins(PlayerPlugin)
    .add_plugins(LevelPlugin)
    .add_plugins(UIPlugin);

    // renet setup
    app.add_plugins(RenetServerPlugin);

    let server = RenetServer::new(ConnectionConfig::default());
    app.insert_resource(server);

    // Transport layer setup
    app.add_plugins(NetcodeServerPlugin);
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(server_addr).unwrap();
    let server_config = ServerConfig {
        current_time: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap(),
        max_clients: 64,
        protocol_id: 0,
        public_addresses: vec![server_addr],
        authentication: ServerAuthentication::Unsecure,
    };
    let transport = NetcodeServerTransport::new(server_config, socket).unwrap();
    app.insert_resource(transport);

    app.add_systems(Startup, send_message_system);
    app.add_systems(Startup, receive_message_system);
    app.add_systems(Startup, handle_events_system);

    // :)
    app.run();
}

// use zstd here

fn send_message_system(mut server: ResMut<RenetServer>) {
    let channel_id = 0;
    // Send a text message for all clients
    // The enum DefaultChannel describe the channels used by the default configuration
    server.broadcast_message(DefaultChannel::ReliableOrdered, "server message");
}

fn receive_message_system(mut server: ResMut<RenetServer>) {
    // Receive message from all clients
    for client_id in server.clients_id() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            // Handle received message
        }
    }
}

fn handle_events_system(mut event: ServerEvent) {
    match event {
        ServerEvent::ClientConnected { client_id } => {
            println!("Client {client_id} connected");
        }
        ServerEvent::ClientDisconnected { client_id, reason } => {
            println!("Client {client_id} disconnected: {reason}");
        }
    }
}
