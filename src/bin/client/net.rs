// client/net.rs
// This file contains a client-specific NetworkManager plugin

use bevy::prelude::*;
use cmbevy::core::{
    config::SERVER_ADDRESS,
    net::backend::{Message, get_udp_socket},
}; // networking backend
use std::net::{TcpListener, UdpSocket};

#[derive(Resource)]
pub struct ClientNetManager {
    /// to send/recv udp messages from server
    udp_socket: UdpSocket,

    /// listens to server reliable messages
    // tcp_listener: TcpListener,
    pub udp_messages: Vec<Message>,

    pub tcp_messages: Vec<Message>,
}

impl ClientNetManager {
    pub fn new(udp_sock: UdpSocket /*, tcp_listener: TcpListener*/) -> Self {
        Self {
            udp_socket: udp_sock,
            // tcp_listener: tcp_listener, // not yet implemented
            udp_messages: vec![],
            tcp_messages: vec![],
        }
    }

    /// adds a message to the vector to be sent later this tick
    pub fn add_message() {}
}

// if client: client_loop_batched, else server_loop_batched, etc
pub struct ClientNetManagerPlugin;
impl Plugin for ClientNetManagerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClientNetManager::new(get_udp_socket("0.0.0.0:0")));
        app.add_systems(schedule, systems)
    }
}
