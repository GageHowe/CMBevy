use bevy::prelude::*;
use common::net::backend::MsgType;
use common::physics::physics_world::*;
use common::{
    config::{MAX_UDP_SIZE, SERVER_BIND_ADDRESS},
    net::backend::*,
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

    pub outgoing_udp: HashMap<SocketAddr, Vec<MsgType>>,
    pub current_tick: u64,
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
    pub fn enqueue(&mut self, client: SocketAddr, msg: MsgType) {
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
        app.add_systems(FixedPreUpdate, recv_udp);
        app.add_systems(FixedUpdate, increment_tick);
        app.add_systems(
            FixedPostUpdate,
            (send_physics_state, flush_outgoing_udp).chain(),
        );
    }
}

fn increment_tick(mut man: ResMut<ServerNetManager>) {
    man.current_tick += 1;
    // println!("tick: {}", man.current_tick)
}

/// gather the current simulation state and queue it for sending
pub fn send_physics_state(
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkId, &PhysicsBodyHandle)>,
    mut manager: ResMut<ServerNetManager>,
) {
    if manager.clients.is_empty() {
        return;
    };
    let clients: Vec<_> = manager.clients.iter().copied().collect();

    let data = MsgType::State(take_snapshot(world, query));
    for client in clients {
        manager.enqueue(client, data.clone());
        // println!("Sent state to client {client}")
    }
}

/// Receive loop on the server
pub fn recv_udp(mut manager: ResMut<ServerNetManager>) {
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
        match deserialize::<Vec<MsgType>>(&decompressed) {
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

fn handle_from_client(manager: &mut ServerNetManager, src: SocketAddr, msg: MsgType) {
    match msg {
        MsgType::Ping => {
            println!("server: got Ping from {src}, queuing Pong");
            manager.enqueue(src, MsgType::Pong);
        }
        MsgType::Pong => {
            println!("server: got Pong from {src}");
        }
        MsgType::Data(v) => {
            println!("server: got Data({v}) from {src}");
            manager.enqueue(src, MsgType::Data(v));
        }
        x => {
            println!("server: got {:?} from {src}", x)
        }
    }
}
