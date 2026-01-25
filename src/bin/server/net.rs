// server/net.rs
use bevy::prelude::*;
use cmbevy::core::{
    config::{MAX_UDP_SIZE, SERVER_BIND_ADDRESS},
    net::backend::{Message, compress, decompress},
};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use wincode::{deserialize, serialize};

#[derive(Resource)]
pub struct ServerNetManager {
    /// Shared UDP socket for all clients
    udp_socket: UdpSocket,
    /// Known clients (could be discovered from incoming packets)
    pub clients: Vec<SocketAddr>,
    /// Outgoing messages per client for this tick
    pub outgoing_udp: HashMap<SocketAddr, Vec<Message>>,
}

impl ServerNetManager {
    pub fn new(sock: UdpSocket) -> Self {
        Self {
            udp_socket: sock,
            clients: Vec::new(),
            outgoing_udp: HashMap::new(),
        }
    }

    /// Queue a message to be sent to a specific client this tick
    pub fn enqueue(&mut self, client: SocketAddr, msg: Message) {
        self.outgoing_udp.entry(client).or_default().push(msg);
        if !self.clients.contains(&client) {
            self.clients.push(client);
        }
    }
}

pub struct ServerNetManagerPlugin;
impl Plugin for ServerNetManagerPlugin {
    fn build(&self, app: &mut App) {
        let sock = UdpSocket::bind(SERVER_BIND_ADDRESS).expect("server: failed to bind UDP socket");
        sock.set_nonblocking(true)
            .expect("server: failed to set UDP socket nonblocking");

        app.insert_resource(ServerNetManager::new(sock));
        app.add_systems(FixedPreUpdate, handle_udp_server); // read before physics
        app.add_systems(FixedPostUpdate, flush_outgoing_udp); // send after physics
    }
}

/// Receive loop on the server: handles incoming Message batches from any client
pub fn handle_udp_server(mut manager: ResMut<ServerNetManager>) {
    let mut buf = [0u8; MAX_UDP_SIZE];

    loop {
        // take a short-lived borrow of the socket
        let (len, src) = match manager.udp_socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                eprintln!("server: recv_from failed: {e}");
                break;
            }
        };

        // now we can freely use &mut manager again
        let decompressed = match decompress(&buf[..len]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("server: decompress failed from {src}: {e}");
                continue;
            }
        };

        match deserialize::<Vec<Message>>(&decompressed) {
            Ok(vec) => {
                for msg in vec {
                    handle_from_client(&mut manager, src, msg);
                }
            }
            Err(e) => eprintln!("server: bad packet from {src}: {e}"),
        }
    }
}

/// Send all queued messages to each client.
/// Each client has its own Vec of Messages to send
fn flush_outgoing_udp(mut manager: ResMut<ServerNetManager>) {
    if manager.outgoing_udp.is_empty() {
        return;
    }
    let items: Vec<_> = manager.outgoing_udp.drain().collect();
    let sock = &manager.udp_socket;
    for (client, msgs) in items {
        if msgs.is_empty() {
            continue;
        }
        let bytes = match serialize(&msgs) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("server: serialize failed for {client}: {e}");
                continue;
            }
        };
        let compressed = match compress(&bytes) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("server: compress failed for {client}: {e}");
                continue;
            }
        };
        if let Err(e) = sock.send_to(&compressed, client) {
            eprintln!("server: send_to {client} failed: {e}");
        }
    }
}

/// Server-side handling of one message from one client
fn handle_from_client(manager: &mut ServerNetManager, src: SocketAddr, msg: Message) {
    match msg {
        Message::Ping => {
            println!("server: got Ping from {src}, queuing Pong");
            manager.enqueue(src, Message::Pong);
        }
        Message::Pong => {
            println!("server: got Pong from {src}");
        }
        Message::Data(v) => {
            println!("server: got Data({v}) from {src}");

            // test, echo the data back
            manager.enqueue(src, Message::Data(v));
        }
        _ => {}
    }
}
