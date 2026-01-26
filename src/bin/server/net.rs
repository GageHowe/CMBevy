// server/net.rs
use bevy::prelude::*;
use cmbevy::core::{
    config::{MAX_UDP_SIZE, SERVER_BIND_ADDRESS},
    net::backend::{Message, MessageWrapper, compress, decompress},
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
    pub outgoing_udp: HashMap<SocketAddr, Vec<MessageWrapper>>,

    next_reliable_id: HashMap<SocketAddr, u64>,
    pending_reliable: HashMap<SocketAddr, HashMap<u64, (MessageWrapper, f32)>>,
}

impl ServerNetManager {
    pub fn new(sock: UdpSocket) -> Self {
        Self {
            udp_socket: sock,
            clients: Vec::new(),
            outgoing_udp: HashMap::new(),
            next_reliable_id: HashMap::new(),
            pending_reliable: HashMap::new(),
        }
    }

    /// queue a regular (unreliable) message
    pub fn enqueue(&mut self, client: SocketAddr, msg: Message) {
        self.outgoing_udp
            .entry(client)
            .or_default()
            .push(MessageWrapper::regular(msg));
        if !self.clients.contains(&client) {
            self.clients.push(client);
        }
    }

    /// Queue a reliable message (will be resent until acked)
    pub fn enqueue_reliable(&mut self, client: SocketAddr, msg: Message, time: f32) {
        let id = *self.next_reliable_id.entry(client).or_insert(0);
        self.next_reliable_id.insert(client, id + 1);

        let wrapper = MessageWrapper::reliable(id, msg);

        self.outgoing_udp
            .entry(client)
            .or_default()
            .push(wrapper.clone());

        self.pending_reliable
            .entry(client)
            .or_default()
            .insert(id, (wrapper, time));

        if !self.clients.contains(&client) {
            self.clients.push(client);
        }
    }

    /// Resend unacked reliable messages (call every frame with current time)
    pub fn resend_unacked(&mut self, current_time: f32) {
        for (client, pending) in &mut self.pending_reliable {
            for (id, (wrapper, sent_time)) in pending.iter_mut() {
                if current_time - *sent_time > 0.5 {
                    // Resend after 500ms
                    self.outgoing_udp
                        .entry(*client)
                        .or_default()
                        .push(wrapper.clone());
                    *sent_time = current_time; // Update time
                }
            }
        }
    }

    /// Remove acked message from pending
    fn handle_ack(&mut self, client: SocketAddr, id: u64) {
        if let Some(pending) = self.pending_reliable.get_mut(&client) {
            if pending.remove(&id).is_some() {
                println!("server: received ack {id} from {client}");
            }
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
            (resend_reliable_server, flush_outgoing_udp).chain(),
        );
    }
}

pub fn resend_reliable_server(mut manager: ResMut<ServerNetManager>, time: Res<Time>) {
    manager.resend_unacked(time.elapsed_secs());
}

/// Receive loop on the server: handles incoming Message batches from any client
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

        match deserialize::<Vec<MessageWrapper>>(&decompressed) {
            Ok(wrappers) => {
                for wrapper in wrappers {
                    // If this message needs an ack, send it
                    if let Some(id) = wrapper.reliable_id {
                        manager.enqueue(src, Message::Ack(id));
                    }

                    handle_from_client(&mut manager, src, wrapper.message);
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
        Message::Ack(id) => {
            manager.handle_ack(src, id);
        }
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
        _ => {}
    }
}
