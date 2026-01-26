// client/net.rs
// This file contains a client-specific NetworkManager plugin

use bevy::prelude::*;
use cmbevy::core::net::backend::{compress, decompress};
use cmbevy::core::{
    config::{CLIENT_CONNECT_ADDRESS, MAX_UDP_SIZE},
    net::backend::{Message, MessageWrapper /*, compress, decompress */},
};
// networking backend
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
    pending_reliable: HashMap<u64, (MessageWrapper, f32)>,
}

impl ClientNetManager {
    pub fn new(udp_sock: UdpSocket) -> Self {
        Self {
            udp_socket: udp_sock,
            outgoing_udp: vec![],
            next_reliable_id: 0,
            pending_reliable: HashMap::new(),
        }
    }

    /// Queue a regular (unreliable) message
    pub fn enqueue(&mut self, msg: Message) {
        self.outgoing_udp.push(MessageWrapper::regular(msg));
    }

    /// Queue a reliable message (will be resent until acked)
    pub fn enqueue_reliable(&mut self, msg: Message, time: f32) {
        let id = self.next_reliable_id;
        self.next_reliable_id += 1;

        let wrapper = MessageWrapper::reliable(id, msg);

        // Queue for immediate send
        self.outgoing_udp.push(wrapper.clone());

        // Track for resending
        self.pending_reliable.insert(id, (wrapper, time));
    }

    /// Resend unacked reliable messages
    pub fn resend_unacked(&mut self, current_time: f32) {
        for (_id, (wrapper, sent_time)) in self.pending_reliable.iter_mut() {
            if current_time - *sent_time > 0.5 {
                // Resend after 500ms
                self.outgoing_udp.push(wrapper.clone());
                *sent_time = current_time;
            }
        }
    }

    /// Remove acked message from pending
    fn handle_ack(&mut self, id: u64) {
        if self.pending_reliable.remove(&id).is_some() {
            println!("client: received ack {id}");
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
            .expect("client: failed to connect socket");

        app.insert_resource(ClientNetManager::new(sock));
        app.add_systems(FixedPreUpdate, handle_udp);
        app.add_systems(
            FixedPostUpdate,
            (resend_reliable_client, flush_outgoing_udp).chain(),
        );
    }
}
pub fn resend_reliable_client(mut manager: ResMut<ClientNetManager>, time: Res<Time>) {
    manager.resend_unacked(time.elapsed_secs());
}

/// Handle incoming messages from server
/// Handle incoming messages from server
pub fn handle_udp(mut manager: ResMut<ClientNetManager>) {
    let mut buf = [0u8; MAX_UDP_SIZE];
    loop {
        // Borrow socket only for this recv call
        let recv_result = manager.udp_socket.recv(&mut buf);

        match recv_result {
            Ok(len) => {
                let decompressed = match decompress(&buf[..len]) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("client: decompress failed: {e}");
                        continue;
                    }
                };
                match deserialize::<Vec<MessageWrapper>>(&decompressed) {
                    Ok(wrappers) => {
                        for wrapper in wrappers {
                            // If this message needs an ack, send it
                            if let Some(id) = wrapper.reliable_id {
                                manager.enqueue(Message::Ack(id));
                            }

                            handle(&mut manager, wrapper.message);
                        }
                    }
                    Err(e) => eprintln!("client: bad packet: {e}"),
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => break,
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
