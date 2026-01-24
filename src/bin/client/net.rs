// client/net.rs
// This file contains a client-specific NetworkManager plugin

use bevy::prelude::*;
use cmbevy::core::net::backend::{compress, decompress};
use cmbevy::core::{
    config::{MAX_UDP_SIZE, SERVER_ADDRESS},
    net::backend::Message,
};
// networking backend
use std::io::ErrorKind;
use std::net::UdpSocket;
use wincode::{deserialize, serialize};

#[derive(Resource)]
pub struct ClientNetManager {
    /// to send/recv udp messages from server
    udp_socket: UdpSocket,

    /// listens to server reliable messages
    // tcp_listener: TcpListener,
    pub outgoing_udp: Vec<Message>,
    // pub tcp_messages: Vec<Message>,
}

impl ClientNetManager {
    pub fn new(udp_sock: UdpSocket /*, tcp_listener: TcpListener*/) -> Self {
        Self {
            udp_socket: udp_sock,
            // tcp_listener: tcp_listener, // not yet implemented
            outgoing_udp: vec![],
            // tcp_messages: vec![],
        }
    }

    /// adds a message to the vector to be sent later this tick
    pub fn enqueue(&mut self, msg: Message) {
        self.outgoing_udp.push(msg);
    }
}

// if client: client_loop_batched, else server_loop_batched, etc
pub struct ClientNetManagerPlugin;
impl Plugin for ClientNetManagerPlugin {
    fn build(&self, app: &mut App) {
        let sock = UdpSocket::bind("0.0.0.0:0").expect("failed to bind UDP socket");
        sock.set_nonblocking(true)
            .expect("failed to set UDP socket nonblocking");
        sock.connect(SERVER_ADDRESS)
            .expect("client: failed to connect socket");

        app.insert_resource(ClientNetManager::new(sock));
        app.add_systems(FixedPreUpdate, handle_udp); // should handle packets before running physics
        app.add_systems(FixedPostUpdate, flush_outgoing_udp);
    }
}

// handles incoming Message chunks from the server
pub fn handle_udp(manager: ResMut<ClientNetManager>) {
    let sock = &manager.udp_socket;
    let mut buf = [0u8; MAX_UDP_SIZE];
    loop {
        match sock.recv(&mut buf) {
            Ok(len) => {
                // decompress
                let decompressed = match decompress(&buf[..len]) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("client: decompress failed: {e}");
                        continue;
                    }
                };
                // deserialize and handle
                match deserialize::<Vec<Message>>(&decompressed) {
                    Ok(vec) => {
                        for msg in vec {
                            handle(msg);
                        }
                    }
                    Err(e) => eprintln!("client: bad packet: {e}"),
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // no more data available this frame
                break;
            }
            Err(e) => {
                eprintln!("client: recv failed: {e}");
                break;
            }
        }
    }
}

fn flush_outgoing_udp(mut manager: ResMut<ClientNetManager>) {
    if manager.outgoing_udp.is_empty() {
        return;
    }
    let bytes = serialize(&manager.outgoing_udp).unwrap();
    let compressed = compress(&bytes).unwrap();

    // if compressed.len() > MAX_UDP_SIZE {
    //     eprintln!(
    //         "client: outgoing UDP packet too large ({} > {}), dropping or splitting",
    //         compressed.len(),
    //         MAX_UDP_SIZE,
    //     );
    //     // TODO: split into smaller chunks or drop with error
    //     manager.outgoing_udp.clear();
    //     return;
    // }
    manager.udp_socket.send(&compressed).unwrap();
    manager.outgoing_udp.clear();
}

fn handle(msg: Message) {
    match msg {
        Message::Ping => {
            println!("client: got a Ping!")
        }
        Message::Pong => {
            println!("client: got a Pong!")
        }
        Message::Data(v) => {
            println!("client: got a Data({v})!")
        }
        _ => {}
    }
}
