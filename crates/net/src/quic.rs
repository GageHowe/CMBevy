use bevy::prelude::*;
use bytes::Bytes;
#[cfg(feature = "client")]
use quinn::crypto::rustls::QuicClientConfig;
#[cfg(feature = "server")]
use quinn::crypto::rustls::QuicServerConfig;
use std::collections::{HashMap, VecDeque};
#[cfg(feature = "server")]
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, Once};
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use zstd::bulk::compress;
use zstd::stream::{decode_all, encode_all};

use crate::message::MsgType;

const ZSTD_LEVEL: i32 = 3;
const ZSTD_FILE_LEVEL: i32 = 9;
const MTU_THRESHOLD: usize = 1000;
const FRAG_HEADER: usize = 6;
const STREAM_KIND_ORDERED: u8 = 0;
const STREAM_KIND_UNORDERED: u8 = 1;
const MAX_PACKET_SIZE: usize = 64 * 1024 * 1024;

pub type ConnectionId = u64;
pub const SERVER_CONN_ID: ConnectionId = 0;

static RUSTLS_PROVIDER_INIT: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Ordered,
    Unordered,
    Unreliable,
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
    AllExcept(ConnectionId),
}

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

    fn insert(&mut self, index: u8, data: Vec<u8>) -> Option<Vec<u8>> {
        if index as usize >= self.fragments.len() || self.fragments[index as usize].is_some() {
            return None;
        }
        self.fragments[index as usize] = Some(data);
        self.received += 1;
        if self.received == self.total {
            Some(
                self.fragments
                    .iter()
                    .flat_map(|f| f.as_deref().unwrap_or(&[]))
                    .cloned()
                    .collect(),
            )
        } else {
            None
        }
    }
}

enum TransportEvent {
    Connected(ConnectionId),
    Disconnected(ConnectionId),
    Packet {
        conn_id: ConnectionId,
        channel: Channel,
        bytes: Vec<u8>,
    },
}

#[cfg(feature = "client")]
enum ClientCommand {
    Send {
        channel: Channel,
        bytes: Vec<u8>,
    },
    Shutdown,
}

#[cfg(feature = "server")]
enum ServerCommand {
    Accepted(quinn::Connection),
    Send {
        target: SendTarget,
        channel: Channel,
        bytes: Vec<u8>,
    },
    ConnectionClosed(ConnectionId),
    Shutdown,
}

#[cfg(feature = "client")]
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

#[cfg(feature = "client")]
impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

#[cfg(feature = "client")]
impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(feature = "client")]
struct ClientTransport {
    tx: mpsc::UnboundedSender<ClientCommand>,
    rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}

#[cfg(feature = "server")]
struct ServerTransport {
    tx: mpsc::UnboundedSender<ServerCommand>,
    rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}

#[derive(Resource)]
pub struct QuicManager {
    pub inbound: VecDeque<InboundMessage>,
    #[cfg(feature = "server")]
    pub(crate) clients: HashSet<ConnectionId>,
    #[cfg(feature = "client")]
    pub client_connected: bool,
    outbound: VecDeque<(SendTarget, Channel, MsgType)>,
    unreliable_seq: u32,
    reassembly: HashMap<ConnectionId, ReassemblySlot>,
    #[cfg(feature = "client")]
    client_transport: Option<ClientTransport>,
    #[cfg(feature = "server")]
    server_transport: Option<ServerTransport>,
}

impl Default for QuicManager {
    fn default() -> Self {
        Self {
            inbound: VecDeque::new(),
            #[cfg(feature = "server")]
            clients: HashSet::new(),
            #[cfg(feature = "client")]
            client_connected: false,
            outbound: VecDeque::new(),
            unreliable_seq: 0,
            reassembly: HashMap::new(),
            #[cfg(feature = "client")]
            client_transport: None,
            #[cfg(feature = "server")]
            server_transport: None,
        }
    }
}

#[cfg(feature = "server")]
pub struct NetServerPlugin;

#[cfg(feature = "server")]
impl Plugin for NetServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_systems(PreUpdate, process_inbound_server)
            .add_systems(PostUpdate, flush_outbound_server);
    }
}

#[cfg(feature = "client")]
pub struct NetClientPlugin;

#[cfg(feature = "client")]
impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_systems(PreUpdate, process_inbound_client)
            .add_systems(PostUpdate, flush_outbound_client);
    }
}

impl QuicManager {
    pub fn send(&mut self, target: SendTarget, channel: Channel, msg: &MsgType) {
        self.outbound.push_back((target, channel, msg.clone()));
    }

    pub fn send_file(&mut self, target: SendTarget, name: String, data: Vec<u8>) {
        match encode_all(data.as_slice(), ZSTD_FILE_LEVEL) {
            Ok(compressed) => self
                .outbound
                .push_back((target, Channel::Ordered, MsgType::FileData(name, compressed))),
            Err(e) => eprintln!("QuicManager::send_file compress error: {e}"),
        }
    }

    #[cfg(feature = "server")]
    pub fn start_server(&mut self, addr: SocketAddr) {
        if self.server_transport.is_some() {
            return;
        }
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let worker_tx = cmd_tx.clone();
        std::thread::spawn(move || run_server_worker(addr, worker_tx, cmd_rx, event_tx));
        self.server_transport = Some(ServerTransport {
            tx: cmd_tx,
            rx: Mutex::new(event_rx),
        });
        println!("QUIC server listening on {addr}");
    }

    #[cfg(feature = "client")]
    pub fn connect(&mut self, server_addr: SocketAddr) {
        self.disconnect();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || run_client_worker(server_addr, cmd_rx, event_tx));
        self.client_transport = Some(ClientTransport {
            tx: cmd_tx,
            rx: Mutex::new(event_rx),
        });
        println!("Connecting to QUIC server at {server_addr}");
    }

    #[cfg(feature = "client")]
    pub fn disconnect(&mut self) {
        if let Some(transport) = self.client_transport.take() {
            let _ = transport.tx.send(ClientCommand::Shutdown);
        }
        self.client_connected = false;
        self.reassembly.remove(&SERVER_CONN_ID);
    }
}

fn encode_batch(msgs: &[MsgType]) -> Result<Vec<u8>, String> {
    let payload = postcard::to_allocvec(msgs).map_err(|e| format!("serialize: {e}"))?;
    compress(&payload, ZSTD_LEVEL).map_err(|e| format!("compress: {e}"))
}

fn decode_batch(bytes: &[u8]) -> Result<Vec<MsgType>, String> {
    let decompressed = decode_all(bytes).map_err(|e| format!("decompress: {e}"))?;
    postcard::from_bytes::<Vec<MsgType>>(&decompressed).map_err(|e| format!("deserialize: {e}"))
}

fn make_fragments(data: Vec<u8>, seq: u32) -> Vec<Vec<u8>> {
    let chunk_size = MTU_THRESHOLD - FRAG_HEADER;
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    let total = chunks.len() as u8;
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut frag = Vec::with_capacity(FRAG_HEADER + chunk.len());
            frag.extend_from_slice(&seq.to_le_bytes());
            frag.push(i as u8);
            frag.push(total);
            frag.extend_from_slice(chunk);
            frag
        })
        .collect()
}

fn parse_fragment(bytes: &[u8]) -> Option<(u32, u8, u8, &[u8])> {
    if bytes.len() < FRAG_HEADER {
        return None;
    }
    let seq = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    Some((seq, bytes[4], bytes[5], &bytes[FRAG_HEADER..]))
}

fn try_reassemble(
    reassembly: &mut HashMap<ConnectionId, ReassemblySlot>,
    conn_id: ConnectionId,
    bytes: &[u8],
) -> Option<Vec<MsgType>> {
    let (seq, index, total, payload) = parse_fragment(bytes)?;
    if total == 1 {
        return match decode_batch(payload) {
            Ok(msgs) => Some(msgs),
            Err(e) => {
                eprintln!("[conn {conn_id}] decode error: {e}");
                None
            }
        };
    }

    let slot = reassembly
        .entry(conn_id)
        .or_insert_with(|| ReassemblySlot::new(seq, total));

    if seq > slot.seq {
        *slot = ReassemblySlot::new(seq, total);
    } else if seq < slot.seq {
        return None;
    }

    let complete = slot.insert(index, payload.to_vec())?;
    reassembly.remove(&conn_id);

    match decode_batch(&complete) {
        Ok(msgs) => Some(msgs),
        Err(e) => {
            eprintln!("[conn {conn_id}] reassemble decode error: {e}");
            None
        }
    }
}

fn drain_transport_events(quic: &mut QuicManager, events: Vec<TransportEvent>) {
    for event in events {
        match event {
            TransportEvent::Connected(conn_id) => {
                #[cfg(feature = "server")]
                {
                    quic.clients.insert(conn_id);
                    println!("Client connected: {conn_id}");
                }
                #[cfg(feature = "client")]
                if conn_id == SERVER_CONN_ID {
                    quic.client_connected = true;
                    println!("Connected to server");
                }
                quic.inbound.push_back(InboundMessage {
                    conn_id,
                    channel: Channel::Ordered,
                    msg: MsgType::Connected,
                });
            }
            TransportEvent::Disconnected(conn_id) => {
                #[cfg(feature = "server")]
                {
                    quic.clients.remove(&conn_id);
                    println!("Client disconnected: {conn_id}");
                }
                #[cfg(feature = "client")]
                if conn_id == SERVER_CONN_ID {
                    quic.client_connected = false;
                    println!("Disconnected from server");
                }
                quic.reassembly.remove(&conn_id);
                quic.inbound.push_back(InboundMessage {
                    conn_id,
                    channel: Channel::Ordered,
                    msg: MsgType::Disconnected,
                });
            }
            TransportEvent::Packet {
                conn_id,
                channel,
                bytes,
            } => {
                let msgs = if channel == Channel::Unreliable {
                    try_reassemble(&mut quic.reassembly, conn_id, &bytes)
                } else {
                    match decode_batch(&bytes) {
                        Ok(msgs) => Some(msgs),
                        Err(e) => {
                            eprintln!("[conn {conn_id}] decode error: {e}");
                            None
                        }
                    }
                };
                if let Some(msgs) = msgs {
                    for msg in msgs {
                        quic.inbound.push_back(InboundMessage {
                            conn_id,
                            channel,
                            msg,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(feature = "server")]
pub fn process_inbound_server(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.server_transport else {
        return;
    };
    let Ok(rx) = transport.rx.lock() else {
        return;
    };
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    drop(rx);
    drain_transport_events(&mut quic, events);
}

#[cfg(feature = "client")]
pub fn process_inbound_client(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.client_transport else {
        return;
    };
    let Ok(rx) = transport.rx.lock() else {
        return;
    };
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    drop(rx);
    drain_transport_events(&mut quic, events);
}

#[cfg(feature = "server")]
pub fn flush_outbound_server(mut quic: ResMut<QuicManager>) {
    let Some(tx) = quic.server_transport.as_ref().map(|transport| transport.tx.clone()) else {
        quic.outbound.clear();
        return;
    };
    let outbound: Vec<_> = quic.outbound.drain(..).collect();
    let all_clients: Vec<ConnectionId> = quic.clients.iter().copied().collect();
    let mut batches: HashMap<(Option<ConnectionId>, Channel), Vec<MsgType>> = HashMap::new();
    for (target, channel, msg) in outbound {
        match target {
            SendTarget::All => {
                batches.entry((None, channel)).or_default().push(msg);
            }
            SendTarget::One(id) => {
                batches.entry((Some(id), channel)).or_default().push(msg);
            }
            SendTarget::AllExcept(excluded) => {
                for &id in all_clients.iter().filter(|&&id| id != excluded) {
                    batches.entry((Some(id), channel)).or_default().push(msg.clone());
                }
            }
        }
    }

    for ((target, channel), msgs) in batches {
        let data = match encode_batch(&msgs) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("flush_outbound_server encode error: {e}");
                continue;
            }
        };
        let payloads = if channel == Channel::Unreliable {
            let seq = quic.unreliable_seq;
            quic.unreliable_seq = quic.unreliable_seq.wrapping_add(1);
            make_fragments(data, seq)
        } else {
            vec![data]
        };
        for bytes in payloads {
            let _ = tx.send(ServerCommand::Send {
                target: target.map_or(SendTarget::All, SendTarget::One),
                channel,
                bytes,
            });
        }
    }
}

#[cfg(feature = "client")]
pub fn flush_outbound_client(mut quic: ResMut<QuicManager>) {
    let Some(tx) = quic.client_transport.as_ref().map(|transport| transport.tx.clone()) else {
        quic.outbound.clear();
        return;
    };
    let outbound: Vec<_> = quic.outbound.drain(..).collect();
    let mut batches: HashMap<Channel, Vec<MsgType>> = HashMap::new();
    for (_, channel, msg) in outbound {
        batches.entry(channel).or_default().push(msg);
    }

    for (channel, msgs) in batches {
        let data = match encode_batch(&msgs) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("flush_outbound_client encode error: {e}");
                continue;
            }
        };
        let payloads = if channel == Channel::Unreliable {
            let seq = quic.unreliable_seq;
            quic.unreliable_seq = quic.unreliable_seq.wrapping_add(1);
            make_fragments(data, seq)
        } else {
            vec![data]
        };
        for bytes in payloads {
            let _ = tx.send(ClientCommand::Send { channel, bytes });
        }
    }
}

#[cfg(feature = "server")]
fn run_server_worker(
    addr: SocketAddr,
    cmd_tx: mpsc::UnboundedSender<ServerCommand>,
    mut cmd_rx: mpsc::UnboundedReceiver<ServerCommand>,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    let runtime = match Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Failed to build server runtime: {e}");
            return;
        }
    };
    runtime.block_on(async move {
        let endpoint = match make_server_endpoint(addr) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("Failed to start QUIC server: {e}");
                return;
            }
        };
        let accept_tx = cmd_tx.clone();
        let endpoint_for_accept = endpoint.clone();
        tokio::spawn(async move {
            while let Some(incoming) = endpoint_for_accept.accept().await {
                let accept_tx = accept_tx.clone();
                tokio::spawn(async move {
                    if let Ok(connection) = incoming.await {
                        let _ = accept_tx.send(ServerCommand::Accepted(connection));
                    }
                });
            }
        });

        let mut next_conn_id = 1u64;
        let mut connections: HashMap<ConnectionId, ServerConnection> = HashMap::new();
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ServerCommand::Accepted(connection) => {
                    let conn_id = next_conn_id;
                    next_conn_id = next_conn_id.wrapping_add(1);
                    let ordered_tx =
                        spawn_connection_tasks(
                            conn_id,
                            connection.clone(),
                            event_tx.clone(),
                            ClientOrServer::Server(cmd_tx.clone()),
                        );
                    connections.insert(conn_id, ServerConnection { connection, ordered_tx });
                    let _ = event_tx.send(TransportEvent::Connected(conn_id));
                }
                ServerCommand::Send {
                    target,
                    channel,
                    bytes,
                } => send_to_targets(&connections, target, channel, bytes).await,
                ServerCommand::ConnectionClosed(conn_id) => {
                    connections.remove(&conn_id);
                    let _ = event_tx.send(TransportEvent::Disconnected(conn_id));
                }
                ServerCommand::Shutdown => {
                    endpoint.close(0u32.into(), b"shutdown");
                    break;
                }
            }
        }
        endpoint.wait_idle().await;
    });
}

#[cfg(feature = "client")]
fn run_client_worker(
    server_addr: SocketAddr,
    mut cmd_rx: mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    let runtime = match Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Failed to build client runtime: {e}");
            return;
        }
    };
    runtime.block_on(async move {
        let mut endpoint = match quinn::Endpoint::client("0.0.0.0:0".parse().expect("valid bind")) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("Failed to bind client endpoint: {e}");
                return;
            }
        };
        endpoint.set_default_client_config(make_client_config());
        let connection = match endpoint.connect(server_addr, "localhost") {
            Ok(connecting) => match connecting.await {
                Ok(connection) => connection,
                Err(e) => {
                    eprintln!("Failed to open QUIC connection: {e}");
                    return;
                }
            },
            Err(e) => {
                eprintln!("Failed to start QUIC connect: {e}");
                return;
            }
        };

        let ordered_tx =
            spawn_connection_tasks(SERVER_CONN_ID, connection.clone(), event_tx.clone(), ClientOrServer::Client);
        let _ = event_tx.send(TransportEvent::Connected(SERVER_CONN_ID));

        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ClientCommand::Send { channel, bytes } => {
                    send_on_connection(&connection, &ordered_tx, channel, bytes).await;
                }
                ClientCommand::Shutdown => {
                    connection.close(0u32.into(), b"shutdown");
                    break;
                }
            }
        }
        endpoint.wait_idle().await;
    });
}

#[cfg(feature = "server")]
struct ServerConnection {
    connection: quinn::Connection,
    ordered_tx: mpsc::UnboundedSender<Vec<u8>>,
}

#[cfg(feature = "server")]
async fn send_to_targets(
    connections: &HashMap<ConnectionId, ServerConnection>,
    target: SendTarget,
    channel: Channel,
    bytes: Vec<u8>,
) {
    match target {
        SendTarget::All => {
            for connection in connections.values() {
                send_on_connection(&connection.connection, &connection.ordered_tx, channel, bytes.clone())
                    .await;
            }
        }
        SendTarget::One(conn_id) => {
            if let Some(connection) = connections.get(&conn_id) {
                send_on_connection(&connection.connection, &connection.ordered_tx, channel, bytes).await;
            }
        }
        SendTarget::AllExcept(excluded) => {
            for (&conn_id, connection) in connections.iter().filter(|(id, _)| **id != excluded) {
                let _ = conn_id;
                send_on_connection(&connection.connection, &connection.ordered_tx, channel, bytes.clone())
                    .await;
            }
        }
    }
}

enum ClientOrServer {
    #[cfg(feature = "client")]
    Client,
    #[cfg(feature = "server")]
    Server(mpsc::UnboundedSender<ServerCommand>),
}

fn spawn_connection_tasks(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
    side: ClientOrServer,
) -> mpsc::UnboundedSender<Vec<u8>> {
    let (ordered_tx, ordered_rx) = mpsc::unbounded_channel();
    tokio::spawn(ordered_sender_task(connection.clone(), ordered_rx));
    tokio::spawn(uni_receiver_task(conn_id, connection.clone(), event_tx.clone()));
    tokio::spawn(datagram_receiver_task(conn_id, connection.clone(), event_tx.clone()));
    tokio::spawn(async move {
        let _ = connection.closed().await;
        match side {
            #[cfg(feature = "client")]
            ClientOrServer::Client => {
                let _ = event_tx.send(TransportEvent::Disconnected(conn_id));
            }
            #[cfg(feature = "server")]
            ClientOrServer::Server(cmd_tx) => {
                let _ = cmd_tx.send(ServerCommand::ConnectionClosed(conn_id));
            }
        }
    });
    ordered_tx
}

async fn ordered_sender_task(
    connection: quinn::Connection,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let mut stream = match connection.open_uni().await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to open ordered stream: {e}");
            return;
        }
    };
    if stream.write_all(&[STREAM_KIND_ORDERED]).await.is_err() {
        return;
    }
    while let Some(bytes) = rx.recv().await {
        let len = (bytes.len() as u32).to_le_bytes();
        if stream.write_all(&len).await.is_err() || stream.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = stream.finish();
}

async fn uni_receiver_task(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    while let Ok(mut stream) = connection.accept_uni().await {
        let mut kind = [0u8; 1];
        if stream.read_exact(&mut kind).await.is_err() {
            continue;
        }
        match kind[0] {
            STREAM_KIND_ORDERED => loop {
                let mut len = [0u8; 4];
                if stream.read_exact(&mut len).await.is_err() {
                    break;
                }
                let size = u32::from_le_bytes(len) as usize;
                let mut payload = vec![0; size];
                if stream.read_exact(&mut payload).await.is_err() {
                    break;
                }
                let _ = event_tx.send(TransportEvent::Packet {
                    conn_id,
                    channel: Channel::Ordered,
                    bytes: payload,
                });
            },
            STREAM_KIND_UNORDERED => {
                if let Ok(payload) = stream.read_to_end(MAX_PACKET_SIZE).await {
                    let _ = event_tx.send(TransportEvent::Packet {
                        conn_id,
                        channel: Channel::Unordered,
                        bytes: payload,
                    });
                }
            }
            _ => {}
        }
    }
}

async fn datagram_receiver_task(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    while let Ok(bytes) = connection.read_datagram().await {
        let _ = event_tx.send(TransportEvent::Packet {
            conn_id,
            channel: Channel::Unreliable,
            bytes: bytes.to_vec(),
        });
    }
}

async fn send_on_connection(
    connection: &quinn::Connection,
    ordered_tx: &mpsc::UnboundedSender<Vec<u8>>,
    channel: Channel,
    bytes: Vec<u8>,
) {
    match channel {
        Channel::Ordered => {
            let _ = ordered_tx.send(bytes);
        }
        Channel::Unordered => {
            if let Ok(mut stream) = connection.open_uni().await {
                let _ = stream.write_all(&[STREAM_KIND_UNORDERED]).await;
                let _ = stream.write_all(&bytes).await;
                let _ = stream.finish();
            }
        }
        Channel::Unreliable => {
            let _ = connection.send_datagram(Bytes::from(bytes));
        }
    }
}

fn ensure_rustls_crypto_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(feature = "server")]
fn make_server_endpoint(addr: SocketAddr) -> Result<quinn::Endpoint, String> {
    ensure_rustls_crypto_provider();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into(), addr.ip().to_string()])
        .map_err(|e| format!("generate cert: {e}"))?;
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    let cert_der = cert.cert.der().clone();
    let server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key.into())
        .map_err(|e| format!("server tls: {e}"))?;
    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_crypto).map_err(
            |e| format!("server quic tls: {e}"),
        )?));
    if let Some(transport) = Arc::get_mut(&mut server_config.transport) {
        transport.max_concurrent_uni_streams(1024u32.into());
        transport.datagram_receive_buffer_size(Some(1024 * 1024));
        transport.max_concurrent_bidi_streams(0u8.into());
    }
    quinn::Endpoint::server(server_config, addr).map_err(|e| format!("endpoint: {e}"))
}

#[cfg(feature = "client")]
fn make_client_config() -> quinn::ClientConfig {
    ensure_rustls_crypto_provider();
    let client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    quinn::ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(client_crypto).expect("valid client quic config"),
    ))
}
