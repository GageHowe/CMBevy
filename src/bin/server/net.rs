use bevy::prelude::*;
use cmbevy::core::{
    config::{MAX_UDP_SIZE, SERVER_BIND_ADDRESS},
    net::backend::{Message, MessageWrapper, compress, decompress},
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

    pub outgoing_udp: HashMap<SocketAddr, Vec<MessageWrapper>>,

    next_reliable_id: HashMap<SocketAddr, u64>,
    pending_reliable: HashMap<SocketAddr, HashMap<u64, (MessageWrapper, u64)>>, // (message, tick_sent)
    current_tick: u64,
    last_received_reliable: HashMap<SocketAddr, Option<u64>>,
}

impl ServerNetManager {
    pub fn new(sock: UdpSocket) -> Self {
        Self {
            udp_socket: sock,
            clients: HashSet::new(),
            outgoing_udp: HashMap::new(),
            next_reliable_id: HashMap::new(),
            pending_reliable: HashMap::new(),
            current_tick: 0,
            last_received_reliable: HashMap::new(),
        }
    }

    /// Queue a regular (unreliable) message
    pub fn enqueue(&mut self, client: SocketAddr, msg: Message) {
        self.outgoing_udp
            .entry(client)
            .or_default()
            .push(MessageWrapper::regular(msg));
        if !self.clients.contains(&client) {
            self.clients.insert(client);
        }
    }

    /// Queue a reliable message (will be resent until acked)
    pub fn enqueue_reliable(&mut self, client: SocketAddr, msg: Message) {
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
            .insert(id, (wrapper, 0)); // tick 0 means "send immediately"

        if !self.clients.contains(&client) {
            self.clients.insert(client);
        }
    }

    /// Resend unacked reliable messages
    pub fn resend_unacked(&mut self) {
        self.current_tick += 1;

        for (client, pending) in &mut self.pending_reliable {
            let mut to_remove = Vec::new();

            for (id, (wrapper, sent_tick)) in pending.iter_mut() {
                if *sent_tick == 0 {
                    // First resend
                    self.outgoing_udp
                        .entry(*client)
                        .or_default()
                        .push(wrapper.clone());
                    *sent_tick = self.current_tick;
                } else if self.current_tick - *sent_tick >= 30 {
                    // Been 30 ticks since resend, give up
                    to_remove.push(*id);
                }
            }

            for id in to_remove {
                pending.remove(&id);
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

pub fn resend_reliable_server(mut manager: ResMut<ServerNetManager>) {
    manager.resend_unacked();
}

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

        match deserialize::<Vec<MessageWrapper>>(&decompressed) {
            Ok(wrappers) => {
                for wrapper in wrappers {
                    if let Some(id) = wrapper.reliable_id {
                        // Check for duplicate (read-only check)
                        let is_duplicate = manager
                            .last_received_reliable
                            .get(&src)
                            .and_then(|&last| last)
                            .map(|last| id <= last)
                            .unwrap_or(false);

                        if is_duplicate {
                            continue; // Skip duplicate
                        }

                        // New message, send ack
                        manager.enqueue_reliable(src, Message::Ack(id));

                        // Update last received
                        manager.last_received_reliable.insert(src, Some(id));
                    }

                    handle_from_client(&mut manager, src, wrapper.message);
                }
            }
            Err(e) => eprintln!("server: bad packet from {src}: {e}"),
        }
    }
}

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
