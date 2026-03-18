use bevy::prelude::*;
use zstd::bulk::compress;
use zstd::stream::{decode_all, encode_all};
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
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use crate::message::MsgType;

pub use bevy_quinnet::client::QuinnetClient;
pub use bevy_quinnet::server::QuinnetServer;
pub use bevy_quinnet::shared::ClientId;

const ZSTD_LEVEL: i32 = 3;
const ZSTD_FILE_LEVEL: i32 = 9;

/// Leave headroom for QUIC/UDP framing (~200 bytes).
const MTU_THRESHOLD: usize = 1000;
/// seq(4) + fragment_index(1) + total_fragments(1)
const FRAG_HEADER: usize = 6;

pub(crate) const ORDERED_CHANNEL: ChannelId = 0;
pub(crate) const UNORDERED_CHANNEL: ChannelId = 1;
pub(crate) const UNRELIABLE_CHANNEL: ChannelId = 2;

pub(crate) fn channels_config() -> SendChannelsConfiguration {
    SendChannelsConfiguration::from_configs(vec![
        ChannelConfig::default_ordered_reliable(),
        ChannelConfig::default_unordered_reliable(),
        ChannelConfig::default_unreliable(),
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

pub type ConnectionId = ClientId;
pub const SERVER_CONN_ID: ConnectionId = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Ordered,
    Unordered,
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

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub channel: Channel,
    pub msg: MsgType,
}

#[derive(Debug, Clone)]
pub enum SendTarget {
    One(ConnectionId),
    All,
    /// Send to all connected clients except the given one.
    AllExcept(ConnectionId),
}

// ---------------------------------------------------------------------------
// Reassembly
// ---------------------------------------------------------------------------

/// Holds in-progress fragment reassembly for one unreliable sequence.
struct ReassemblySlot {
    seq: u32,
    total: u8,
    fragments: Vec<Option<Vec<u8>>>,
    received: u8,
}

impl ReassemblySlot {
    fn new(seq: u32, total: u8) -> Self {
        Self { seq, total, fragments: vec![None; total as usize], received: 0 }
    }

    /// Insert a fragment. Returns the reassembled payload if complete.
    fn insert(&mut self, index: u8, data: Vec<u8>) -> Option<Vec<u8>> {
        if index as usize >= self.fragments.len() || self.fragments[index as usize].is_some() {
            return None;
        }
        self.fragments[index as usize] = Some(data);
        self.received += 1;
        if self.received == self.total {
            Some(self.fragments.iter().flat_map(|f| f.as_deref().unwrap_or(&[])).cloned().collect())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct QuicManager {
    pub inbound: VecDeque<InboundMessage>,
    pub(crate) clients: HashSet<ConnectionId>,
    pub client_connected: bool,
    outbound: VecDeque<(SendTarget, Channel, MsgType)>,
    /// Sequence counter for outbound unreliable fragments.
    unreliable_seq: u32,
    /// Per-connection reassembly slot for inbound unreliable fragments.
    /// Client side uses SERVER_CONN_ID as the key.
    reassembly: HashMap<ConnectionId, ReassemblySlot>,
}

impl QuicManager {
    /// Queue a message for delivery. Messages sharing the same (target, channel)
    /// within a tick are batched into one compressed packet at flush time.
    pub fn send(&mut self, target: SendTarget, channel: Channel, msg: &MsgType) {
        self.outbound.push_back((target, channel, msg.clone()));
    }

    /// Queue a file for delivery over the ordered channel.
    /// The file bytes are stream-compressed at level 9 before sending.
    pub fn send_file(&mut self, target: SendTarget, name: String, data: Vec<u8>) {
        match encode_all(data.as_slice(), ZSTD_FILE_LEVEL) {
            Ok(compressed) => self.outbound.push_back((target, Channel::Ordered, MsgType::FileData(name, compressed))),
            Err(e) => eprintln!("QuicManager::send_file compress error: {e}"),
        }
    }

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

    pub fn connect(&mut self, client: &mut QuinnetClient, server_addr: SocketAddr) {
        // Reuse an existing (disconnected) connection via reconnect() so the
        // default_connection_id doesn't drift to a stale entry.
        if let Some(conn) = client.get_connection_mut() {
            match conn.reconnect() {
                Ok(_) => println!("Connecting to QUIC server at {server_addr}"),
                Err(e) => eprintln!("Failed to reconnect: {e}"),
            }
            return;
        }
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
// Encode / decode helpers
// ---------------------------------------------------------------------------

fn encode_batch(msgs: &[MsgType]) -> Result<Vec<u8>, String> {
    let payload = postcard::to_allocvec(msgs).map_err(|e| format!("serialize: {e}"))?;
    compress(&payload, ZSTD_LEVEL).map_err(|e| format!("compress: {e}"))
}

fn decode_batch(bytes: &[u8]) -> Result<Vec<MsgType>, String> {
    let decompressed = decode_all(bytes).map_err(|e| format!("decompress: {e}"))?;
    postcard::from_bytes::<Vec<MsgType>>(&decompressed).map_err(|e| format!("deserialize: {e}"))
}

/// Split `data` into MTU-sized fragments with a 6-byte header each.
fn make_fragments(data: Vec<u8>, seq: u32) -> Vec<Vec<u8>> {
    let chunk_size = MTU_THRESHOLD - FRAG_HEADER;
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    let total = chunks.len() as u8;
    chunks.into_iter().enumerate().map(|(i, chunk)| {
        let mut frag = Vec::with_capacity(FRAG_HEADER + chunk.len());
        frag.extend_from_slice(&seq.to_le_bytes());
        frag.push(i as u8);
        frag.push(total);
        frag.extend_from_slice(chunk);
        frag
    }).collect()
}

/// Parse a fragment header and return (seq, index, total, payload).
fn parse_fragment(bytes: &[u8]) -> Option<(u32, u8, u8, &[u8])> {
    if bytes.len() < FRAG_HEADER { return None; }
    let seq = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    Some((seq, bytes[4], bytes[5], &bytes[FRAG_HEADER..]))
}

/// Feed a fragment into the reassembly table. Returns decoded messages if complete.
fn try_reassemble(
    reassembly: &mut HashMap<ConnectionId, ReassemblySlot>,
    conn_id: ConnectionId,
    bytes: &[u8],
) -> Option<Vec<MsgType>> {
    let (seq, index, total, payload) = parse_fragment(bytes)?;

    // Fast path: single-fragment packet.
    if total == 1 {
        return match decode_batch(payload) {
            Ok(msgs) => Some(msgs),
            Err(e) => { eprintln!("[conn {conn_id}] decode error: {e}"); None }
        };
    }

    let slot = reassembly.entry(conn_id).or_insert_with(|| ReassemblySlot::new(seq, total));

    // Newer sequence arrived — discard the old reassembly.
    if seq > slot.seq {
        *slot = ReassemblySlot::new(seq, total);
    } else if seq < slot.seq {
        return None; // stale fragment, drop
    }

    let complete = slot.insert(index, payload.to_vec())?;
    reassembly.remove(&conn_id);

    match decode_batch(&complete) {
        Ok(msgs) => Some(msgs),
        Err(e) => { eprintln!("[conn {conn_id}] reassemble decode error: {e}"); None }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

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
        quic.reassembly.remove(&id);
    }

    for &client_id in &current {
        if let Some(conn) = endpoint.connection_mut(client_id) {
            while let Ok((ch_id, bytes)) = conn.dequeue_undispatched_bytes_from_peer() {
                let channel = channel_from_id(ch_id);
                let msgs = if channel == Channel::Unreliable {
                    try_reassemble(&mut quic.reassembly, client_id, &bytes)
                } else {
                    match decode_batch(&bytes) {
                        Ok(m) => Some(m),
                        Err(e) => { eprintln!("[client {client_id}] decode error: {e}"); None }
                    }
                };
                if let Some(msgs) = msgs {
                    for msg in msgs {
                        quic.inbound.push_back(InboundMessage { conn_id: client_id, channel, msg });
                    }
                }
            }
        }
    }

    quic.clients = current;
}

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
            quic.reassembly.remove(&SERVER_CONN_ID);
        }
        _ => {}
    }
    quic.client_connected = now_connected;

    if let Some(conn) = client.get_connection_mut() {
        while let Ok((ch_id, bytes)) = conn.dequeue_undispatched_bytes_from_peer() {
            let channel = channel_from_id(ch_id);
            let msgs = if channel == Channel::Unreliable {
                try_reassemble(&mut quic.reassembly, SERVER_CONN_ID, &bytes)
            } else {
                match decode_batch(&bytes) {
                    Ok(m) => Some(m),
                    Err(e) => { eprintln!("[server] decode error: {e}"); None }
                }
            };
            if let Some(msgs) = msgs {
                for msg in msgs {
                    quic.inbound.push_back(InboundMessage { conn_id: SERVER_CONN_ID, channel, msg });
                }
            }
        }
    }
}

pub fn flush_outbound(
    mut quic: ResMut<QuicManager>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
) {
    // Group by (target, channel). AllExcept is pre-expanded into One sends per client.
    let outbound: Vec<_> = quic.outbound.drain(..).collect();
    let all_clients: Vec<ConnectionId> = quic.clients.iter().copied().collect();
    let mut batches: HashMap<(Option<ConnectionId>, ChannelId), Vec<MsgType>> = HashMap::new();
    for (target, channel, msg) in outbound {
        let ch_id = ChannelId::from(channel);
        match target {
            SendTarget::All => { batches.entry((None, ch_id)).or_default().push(msg); }
            SendTarget::One(id) => { batches.entry((Some(id), ch_id)).or_default().push(msg); }
            SendTarget::AllExcept(excluded) => {
                for &id in all_clients.iter().filter(|&&id| id != excluded) {
                    batches.entry((Some(id), ch_id)).or_default().push(msg.clone());
                }
            }
        }
    }

    if let Some(endpoint) = server.get_endpoint_mut() {
        for ((target_id, ch_id), msgs) in batches {
            let data = match encode_batch(&msgs) {
                Ok(d) => d,
                Err(e) => { eprintln!("flush_outbound encode error: {e}"); continue; }
            };

            if ch_id == UNRELIABLE_CHANNEL {
                let seq = quic.unreliable_seq;
                quic.unreliable_seq = quic.unreliable_seq.wrapping_add(1);
                for frag in make_fragments(data, seq) {
                    match target_id {
                        None => { endpoint.try_broadcast_payload_on(ch_id, frag); }
                        Some(id) => { endpoint.try_send_payload_on(id, ch_id, frag); }
                    }
                }
            } else {
                match target_id {
                    None => { endpoint.try_broadcast_payload_on(ch_id, data); }
                    Some(id) => { endpoint.try_send_payload_on(id, ch_id, data); }
                }
            }
        }
    } else if let Some(conn) = client.get_connection_mut() {
        for ((_, ch_id), msgs) in batches {
            let data = match encode_batch(&msgs) {
                Ok(d) => d,
                Err(e) => { eprintln!("flush_outbound encode error: {e}"); continue; }
            };

            if ch_id == UNRELIABLE_CHANNEL {
                let seq = quic.unreliable_seq;
                quic.unreliable_seq = quic.unreliable_seq.wrapping_add(1);
                for frag in make_fragments(data, seq) {
                    conn.try_send_payload_on(ch_id, frag);
                }
            } else {
                conn.try_send_payload_on(ch_id, data);
            }
        }
    }
}
