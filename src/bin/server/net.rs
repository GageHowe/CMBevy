use bevy::prelude::*;
use cmbevy::core::{
    config::{MAX_UDP_SIZE, SERVER_BIND_ADDRESS},
    net::backend::{Message, compress, decompress},
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use wincode::{deserialize, serialize};

#[derive(Resource)]
pub struct ServerNetManager {
    udp_socket: UdpSocket,
    pub clients: HashSet<SocketAddr>,

    pub outgoing_udp: HashMap<SocketAddr, Vec<Message>>,
    current_tick: u64,
}

impl ServerNetManager {
    pub fn new(sock: UdpSocket) -> Self {
        Self {
            udp_socket: sock,
            clients: HashSet::new(),
            outgoing_udp: HashMap::new(),
            current_tick: 0,
        }
    }

    /// Queue a regular (unreliable) message
    pub fn enqueue(&mut self, client: SocketAddr, msg: Message) {
        self.outgoing_udp.entry(client).or_default().push(msg);
        if !self.clients.contains(&client) {
            self.clients.insert(client);
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
        app.add_systems(FixedPreUpdate, handle_udp_server);
        app.add_systems(
            FixedPostUpdate,
            (send_physics_state, flush_outgoing_udp).chain(),
        );
    }
}

/// gather the current simulation state and queue it for sending
pub fn send_physics_state() {}

/// Receive loop on the server
pub fn handle_udp_server(mut manager: ResMut<ServerNetManager>) {
    let mut buf = [0u8; MAX_UDP_SIZE];
    loop {
        let (len, src) = match manager.udp_socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                eprintln!("server: recv_from failed: {e}");
                break;
            }
        };
        let decompressed = match decompress(&buf[..len]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("server: decompress failed from {src}: {e}");
                continue;
            }
        };
        match deserialize::<Vec<Message>>(&decompressed) {
            Ok(msgs) => {
                for msg in msgs {
                    handle_from_client(&mut manager, src, msg);
                }
            }
            Err(e) => eprintln!("server: bad packet from {src}: {e}"),
        }
    }
}

/// finalize and send queued messages via UDP
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
            manager.enqueue(src, Message::Data(v));
        }
        x => {
            println!("server: got {:?} from {src}", x)
        }
    }
}
