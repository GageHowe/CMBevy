use bevy::prelude::*;
use bevy_quinnet::{
    client::{
        certificate::CertificateVerificationMode,
        connection::ClientAddrConfiguration,
        ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables,
    },
    server::{
        certificate::CertificateRetrievalMode,
        EndpointAddrConfiguration,
        ServerEndpointConfiguration, ServerEndpointConfigurationDefaultables,
    },
    shared::channels::{ChannelConfig, ChannelId, SendChannelsConfiguration},
};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use crate::net::message::MsgType;

// Re-export quinnet resources so callers can use them without a direct bevy_quinnet dep.
pub use bevy_quinnet::client::QuinnetClient;
pub use bevy_quinnet::server::QuinnetServer;
pub use bevy_quinnet::shared::ClientId;

// TODO: add compression with zstd

pub const ORDERED_CHANNEL: ChannelId = 0;
pub const UNORDERED_CHANNEL: ChannelId = 1;
pub const UNRELIABLE_CHANNEL: ChannelId = 2;

pub(crate) fn channels_config() -> SendChannelsConfiguration {
    SendChannelsConfiguration::from_configs(vec![
        ChannelConfig::default_ordered_reliable(),   // id 0 → Channel::Ordered
        ChannelConfig::default_unordered_reliable(), // id 1 → Channel::Unordered
        ChannelConfig::default_unreliable(),         // id 2 → Channel::Unreliable
    ])
    .expect("channel count is within limits")
}

fn channel_from_id(id: ChannelId) -> Channel {
    match id {
        0 => Channel::Ordered,
        1 => Channel::Unordered,
        _ => Channel::Unreliable,
    }
}

/// Opaque identifier for a connected peer.
/// On the server this equals bevy_quinnet's ClientId; on the client side
/// SERVER_CONN_ID is used for messages received from the server.
/// does this even need to be here?
pub type ConnectionId = ClientId;

/// A fixed ConnectionId used on the client side to represent the server.
pub const SERVER_CONN_ID: ConnectionId = 0;

/// Logical send channel with delivery guarantees.
#[derive(Debug, Clone, Copy)]
pub enum Channel {
    /// Ordered, reliable (stream-based)
    Ordered,
    /// Unordered, reliable
    Unordered,
    /// Unreliable datagram
    Unreliable,
}

impl From<Channel> for ChannelId {
    fn from(c: Channel) -> Self {
        match c {
            Channel::Ordered => ORDERED_CHANNEL,
            Channel::Unordered => UNORDERED_CHANNEL,
            Channel::Unreliable => UNRELIABLE_CHANNEL,
        }
    }
}

/// A decoded message plus the connection it arrived on.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub channel: Channel,
    pub msg: MsgType,
}

/// Who to deliver an outbound message to.
#[derive(Debug, Clone)]
pub enum SendTarget {
    One(ConnectionId),
    All,
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// High-level networking manager.  Drain `inbound` each frame; call `send`
/// to queue outbound messages; call `start_server` or `connect` once at startup.
#[derive(Resource, Default)]
pub struct QuicManager {
    /// Messages received this frame — drain these in your systems.
    pub inbound: VecDeque<InboundMessage>,
    /// Currently connected client IDs (server side only).
    pub clients: HashSet<ConnectionId>,
    /// Tracks the client connection state for edge-detection (client side only).
    client_connected: bool,
    outbound: VecDeque<(SendTarget, Channel, Vec<u8>)>,
}

impl QuicManager {
    /// Serialize and queue a message for delivery.
    pub fn send(&mut self, target: SendTarget, channel: Channel, msg: &MsgType) {
        match wincode::serialize(msg) {
            Ok(payload) => self.outbound.push_back((target, channel, payload)),
            Err(e) => eprintln!("QuicManager::send serialize error: {e}"),
        }
    }

    /// Start a QUIC server endpoint.  Call once from a `Startup` system.
    pub fn start_server(&mut self, server: &mut QuinnetServer, addr: SocketAddr) {
        let result = server.start_endpoint(ServerEndpointConfiguration {
            addr_config: EndpointAddrConfiguration::from_addr(addr),
            cert_mode: CertificateRetrievalMode::GenerateSelfSigned {
                server_hostname: addr.ip().to_string(),
            },
            defaultables: ServerEndpointConfigurationDefaultables {
                send_channels_cfg: channels_config(),
                ..Default::default()
            },
        });
        match result {
            Ok(_) => println!("QUIC server listening on {addr}"),
            Err(e) => eprintln!("Failed to start QUIC server: {e}"),
        }
    }

    /// Open a connection to a server.  Call once from a `Startup` system.
    pub fn connect(&mut self, client: &mut QuinnetClient, server_addr: SocketAddr) {
        let result = client.open_connection(ClientConnectionConfiguration {
            addr_config: ClientAddrConfiguration::from_addrs(
                server_addr,
                "0.0.0.0:0".parse().expect("failed to parse client bind address \"0.0.0.0:0\""),
            ),
            cert_mode: CertificateVerificationMode::SkipVerification,
            defaultables: ClientConnectionConfigurationDefaultables {
                send_channels_cfg: channels_config(),
                ..Default::default()
            },
        });
        match result {
            Ok(_) => println!("Connecting to QUIC server at {server_addr}"),
            Err(e) => eprintln!("Failed to open QUIC connection: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Systems (registered directly in MasterPlugin)
// ---------------------------------------------------------------------------

/// Server-side inbound processing. Register in `PreUpdate` on the server binary only.
/// Handles connection lifecycle (connect/disconnect edges) and message receipt from all clients.
pub fn process_inbound_server(
    mut quic: ResMut<QuicManager>,
    mut server: ResMut<QuinnetServer>,
) {
    let Some(endpoint) = server.get_endpoint_mut() else { return };
    let current: HashSet<ConnectionId> = endpoint.clients().into_iter().collect();

    let connected: Vec<ConnectionId> = current.difference(&quic.clients).cloned().collect();
    let disconnected: Vec<ConnectionId> = quic.clients.difference(&current).cloned().collect();

    for id in connected {
        println!("Client connected: {id}");
        quic.inbound.push_back(InboundMessage { conn_id: id, channel: Channel::Ordered, msg: MsgType::Connected });
    }
    for id in disconnected {
        println!("Client disconnected: {id}");
        quic.inbound.push_back(InboundMessage { conn_id: id, channel: Channel::Ordered, msg: MsgType::Disconnected });
    }

    // Iterate `current` (local var) to avoid holding an immutable borrow on `quic.clients`
    // while also pushing to `quic.inbound`.
    for &client_id in &current {
        if let Some(conn) = endpoint.connection_mut(client_id) {
            while let Ok((ch_id, bytes)) = conn.dequeue_undispatched_bytes_from_peer() {
                match wincode::deserialize::<MsgType>(&bytes) {
                    Ok(msg) => quic.inbound.push_back(InboundMessage {
                        conn_id: client_id,
                        channel: channel_from_id(ch_id),
                        msg,
                    }),
                    Err(e) => eprintln!("[client {client_id}] deserialize error: {e}"),
                }
            }
        }
    }

    quic.clients = current;
}

/// Client-side inbound processing. Register in `PreUpdate` on the client binary only.
/// Handles connect/disconnect edges and message receipt from the server.
pub fn process_inbound_client(
    mut quic: ResMut<QuicManager>,
    mut client: ResMut<QuinnetClient>,
) {
    let now_connected = client.is_connected();
    match (quic.client_connected, now_connected) {
        (false, true) => {
            println!("Connected to server");
            quic.inbound.push_back(InboundMessage { conn_id: SERVER_CONN_ID, channel: Channel::Ordered, msg: MsgType::Connected });
        }
        (true, false) => {
            println!("Disconnected from server");
            quic.inbound.push_back(InboundMessage { conn_id: SERVER_CONN_ID, channel: Channel::Ordered, msg: MsgType::Disconnected });
        }
        _ => {}
    }
    quic.client_connected = now_connected;

    if let Some(conn) = client.get_connection_mut() {
        while let Ok((ch_id, bytes)) = conn.dequeue_undispatched_bytes_from_peer() {
            match wincode::deserialize::<MsgType>(&bytes) {
                Ok(msg) => quic.inbound.push_back(InboundMessage {
                    conn_id: SERVER_CONN_ID,
                    channel: channel_from_id(ch_id),
                    msg,
                }),
                Err(e) => eprintln!("[server] deserialize error: {e}"),
            }
        }
    }
}

pub fn flush_outbound(
    mut quic: ResMut<QuicManager>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
) {
    while let Some((target, channel, data)) = quic.outbound.pop_front() {
        let ch_id: ChannelId = channel.into();

        if let Some(endpoint) = server.get_endpoint_mut() {
            match target {
                SendTarget::One(conn_id) => {
                    endpoint.try_send_payload_on(conn_id, ch_id, data);
                }
                SendTarget::All => {
                    endpoint.try_broadcast_payload_on(ch_id, data);
                }
            }
        } else if let Some(conn) = client.get_connection_mut() {
            conn.try_send_payload_on(ch_id, data);
        }
    }
}
