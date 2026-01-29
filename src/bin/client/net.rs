use bevy::prelude::*;
use cmbevy::core::net::backend::{compress, decompress};
use cmbevy::core::{
    config::{CLIENT_CONNECT_ADDRESS, MAX_UDP_SIZE},
    net::backend::Message,
};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::UdpSocket;
use wincode::{deserialize, serialize};

#[derive(Resource)]
pub struct ClientNetManager {
    udp_socket: UdpSocket,
    pub outgoing_udp: Vec<Message>,

    current_tick: u64,
}

impl ClientNetManager {
    pub fn new(udp_sock: UdpSocket) -> Self {
        Self {
            udp_socket: udp_sock,
            outgoing_udp: vec![],

            current_tick: 0,
        }
    }

    /// prep a unreliable message for sending
    pub fn enqueue(&mut self, msg: Message) {
        self.outgoing_udp.push(msg);
    }
}

pub struct ClientNetManagerPlugin;
impl Plugin for ClientNetManagerPlugin {
    fn build(&self, app: &mut App) {
        let sock = UdpSocket::bind("0.0.0.0:0").expect("failed to bind UDP socket");
        sock.set_nonblocking(true)
            .expect("failed to set UDP socket nonblocking");
        sock.connect(CLIENT_CONNECT_ADDRESS)
            .expect("CLIENT: failed to connect socket");

        app.insert_resource(ClientNetManager::new(sock));
        app.add_systems(FixedPreUpdate, handle_udp);
        app.add_systems(FixedPostUpdate, flush_outgoing_udp);
    }
}

/// Handle incoming messages from server
pub fn handle_udp(mut manager: ResMut<ClientNetManager>) {
    let mut buf = [0u8; MAX_UDP_SIZE];
    loop {
        let recv_result = manager.udp_socket.recv(&mut buf);

        match recv_result {
            Ok(len) => {
                let decompressed = match decompress(&buf[..len]) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("CLIENT: decompress failed: {e}");
                        continue;
                    }
                };
                match deserialize::<Vec<Message>>(&decompressed) {
                    Ok(msgs) => {
                        for msg in msgs {
                            handle(&mut manager, msg);
                        }
                    }
                    Err(e) => eprintln!("CLIENT: bad packet: {e}"),
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

fn flush_outgoing_udp(mut manager: ResMut<ClientNetManager>) {
    if manager.outgoing_udp.is_empty() {
        return;
    }
    let bytes = serialize(&manager.outgoing_udp).unwrap();
    let compressed = compress(&bytes).unwrap();
    manager.udp_socket.send(&compressed).unwrap();
    manager.outgoing_udp.clear();
}

fn handle(manager: &mut ClientNetManager, msg: Message) {
    match msg {
        Message::Ping => println!("CLIENT: got a Ping!"),
        Message::Pong => println!("CLIENT: got a Pong!"),
        Message::Data(v) => println!("CLIENT: got a Data({v})!"),
        Message::State(s) => println!("CLIENT: got a State: {:?}", s),
        _ => {}
    }
}
