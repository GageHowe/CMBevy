use bevy::prelude::*;
use cmbevy::core::net::backend::{compress, decompress};
use cmbevy::core::{
    config::{CLIENT_CONNECT_ADDRESS, MAX_UDP_SIZE},
    net::backend::{Message, MessageWrapper},
};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::UdpSocket;
use wincode::{deserialize, serialize};

#[derive(Resource)]
pub struct ClientNetManager {
    udp_socket: UdpSocket,
    pub outgoing_udp: Vec<MessageWrapper>,

    // Reliable message tracking
    next_reliable_id: u64,
    pending_reliable: HashMap<u64, (MessageWrapper, u64)>, // (message, tick_sent)
    current_tick: u64,
    last_received_reliable: Option<u64>,
}

impl ClientNetManager {
    pub fn new(udp_sock: UdpSocket) -> Self {
        Self {
            udp_socket: udp_sock,
            outgoing_udp: vec![],
            next_reliable_id: 0,
            pending_reliable: HashMap::new(),
            current_tick: 0,
            last_received_reliable: None,
        }
    }

    /// Queue a regular (unreliable) message
    pub fn enqueue(&mut self, msg: Message) {
        self.outgoing_udp.push(MessageWrapper::regular(msg));
    }

    /// Queue a reliable message (will be resent until acked)
    pub fn enqueue_reliable(&mut self, msg: Message) {
        let id = self.next_reliable_id;
        self.next_reliable_id += 1;

        let wrapper = MessageWrapper::reliable(id, msg);

        // Queue for immediate send
        self.outgoing_udp.push(wrapper.clone());

        // Track for resending (tick 0 means "send immediately")
        self.pending_reliable.insert(id, (wrapper, 0));
    }

    /// Resend unacked reliable messages. This implementation only retries once.
    pub fn resend_unacked(&mut self) {
        self.current_tick += 1;
        let mut to_remove = Vec::new();

        for (id, (wrapper, sent_tick)) in self.pending_reliable.iter_mut() {
            if *sent_tick == 0 {
                // First resend
                self.outgoing_udp.push(wrapper.clone());
                *sent_tick = self.current_tick;
            } else if self.current_tick - *sent_tick >= 30 {
                to_remove.push(*id);
            }
        }

        for id in to_remove {
            self.pending_reliable.remove(&id);
        }
    }

    /// Remove acked message from pending
    fn handle_ack(&mut self, id: u64) {
        if self.pending_reliable.remove(&id).is_some() {
            println!("CLIENT: received ack {id}");
        }
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
        app.add_systems(
            FixedPostUpdate,
            (resend_reliable_client, flush_outgoing_udp).chain(),
        );
    }
}

pub fn resend_reliable_client(mut manager: ResMut<ClientNetManager>) {
    manager.resend_unacked();
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
                match deserialize::<Vec<MessageWrapper>>(&decompressed) {
                    Ok(wrappers) => {
                        for wrapper in wrappers {
                            if let Some(id) = wrapper.reliable_id {
                                // Check for duplicate
                                if let Some(last_id) = manager.last_received_reliable {
                                    if id <= last_id {
                                        continue; // Skip duplicate
                                    }
                                }

                                // New message, send ack and update
                                manager.enqueue(Message::Ack(id));
                                manager.last_received_reliable = Some(id);
                            }

                            handle(&mut manager, wrapper.message);
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
        Message::Ack(id) => {
            manager.handle_ack(id);
        }
        Message::Ping => println!("CLIENT: got a Ping!"),
        Message::Pong => println!("CLIENT: got a Pong!"),
        Message::Data(v) => println!("CLIENT: got a Data({v})!"),
        _ => {}
    }
}
