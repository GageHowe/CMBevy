use super::runtime::TokioRuntime;
use bevy::prelude::*;
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct QuicPlugin;

impl Plugin for QuicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_message::<OutboundMessage>()
            .add_message::<InboundMessage>()
            .add_message::<ConnectionEstablished>()
            .add_message::<ConnectionLost>()
            .add_systems(
                Update,
                (
                    process_internal_events,
                    flush_outbound_messages,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct ConnectionId(pub u64);

#[derive(Debug, Clone, Copy)]
pub enum Channel {
    /// Reliable, ordered. Persistent stream with length-prefix framing.
    /// Use for: chat, game events, state transitions.
    Ordered,
    /// Reliable, unordered. One stream per message.
    /// Use for: spawn/despawn, one-shot reliable messages where order doesn't matter.
    Unordered,
    /// Unreliable, unordered. QUIC datagrams. Max ~1200 bytes.
    /// Use for: position, rotation, anything high-frequency.
    Unreliable,
}

#[derive(Message, Clone)]
pub struct OutboundMessage {
    pub target: SendTarget,
    pub channel: Channel,
    pub payload: Vec<u8>,
}

#[derive(Message, Clone, Debug)]
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub channel: Channel,
    pub payload: Vec<u8>,
}

#[derive(Message, Clone, Debug)]
pub struct ConnectionEstablished {
    pub conn_id: ConnectionId,
}

#[derive(Message, Clone, Debug)]
pub struct ConnectionLost {
    pub conn_id: ConnectionId,
}

#[derive(Debug, Clone)]
pub enum SendTarget {
    One(ConnectionId),
    All,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct OrderedSender {
    tx: mpsc::Sender<Vec<u8>>,
}

struct PeerState {
    connection: Connection,
    ordered: OrderedSender,
}

enum InternalEvent {
    NewConnection {
        conn_id: ConnectionId,
        connection: Connection,
        ordered_recv: RecvStream,
        ordered_send: SendStream,
    },
    Disconnected {
        conn_id: ConnectionId,
    },
    Inbound {
        conn_id: ConnectionId,
        channel: Channel,
        data: Vec<u8>,
    },
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct QuicManager {
    peers: HashMap<ConnectionId, PeerState>,
    next_id: u64,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
    event_rx: mpsc::UnboundedReceiver<InternalEvent>,
}

impl Default for QuicManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            peers: HashMap::new(),
            next_id: 0,
            event_tx,
            event_rx,
        }
    }
}

impl QuicManager {
    pub fn start_server(&mut self, runtime: &TokioRuntime, addr: SocketAddr) {
        let event_tx = self.event_tx.clone();

        use std::sync::atomic::{AtomicU64, Ordering};
        let counter = std::sync::Arc::new(AtomicU64::new(self.next_id));

        runtime.spawn(async move {
            let endpoint = match make_server_endpoint(addr) {
                Ok(e) => e,
                Err(e) => { eprintln!("Failed to start server: {e}"); return; }
            };
            println!("QUIC server listening on {addr}");

            while let Some(incoming) = endpoint.accept().await {
                let connection = match incoming.await {
                    Ok(c) => c,
                    Err(e) => { eprintln!("Incoming connection failed: {e}"); continue; }
                };

                let conn_id = ConnectionId(counter.fetch_add(1, Ordering::SeqCst));
                let event_tx = event_tx.clone();

                tokio::spawn(async move {
                    handle_new_connection(conn_id, connection, event_tx).await;
                });
            }
        });
    }

    pub fn connect(&mut self, runtime: &TokioRuntime, server_addr: SocketAddr) {
        let event_tx = self.event_tx.clone();
        let conn_id = ConnectionId(self.next_id);
        self.next_id += 1;

        runtime.spawn(async move {
            let endpoint = match make_client_endpoint() {
                Ok(e) => e,
                Err(e) => { eprintln!("Failed to create client endpoint: {e}"); return; }
            };

            let connection = match endpoint.connect(server_addr, "localhost") {
                Ok(c) => match c.await {
                    Ok(c) => c,
                    Err(e) => { eprintln!("Connection failed: {e}"); return; }
                },
                Err(e) => { eprintln!("Connect error: {e}"); return; }
            };

            handle_new_connection(conn_id, connection, event_tx).await;
        });
    }
}

// ---------------------------------------------------------------------------
// Connection setup
// ---------------------------------------------------------------------------

/// Both sides call this. Each side opens one ordered bi-stream toward the
/// other (for sending), and accepts one from the other (for receiving).
/// open_bi and accept_bi must be called in the right order on both sides —
/// both open first, then both accept, so neither side deadlocks waiting.
async fn handle_new_connection(
    conn_id: ConnectionId,
    connection: Connection,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
) {
    // Open our ordered stream toward the peer.
    let (ordered_send, _unused_recv) = match connection.open_bi().await {
        Ok(s) => s,
        Err(e) => { eprintln!("[{conn_id:?}] open_bi failed: {e}"); return; }
    };

    // Accept the ordered stream the peer opened toward us.
    let (_unused_send, ordered_recv) = match connection.accept_bi().await {
        Ok(s) => s,
        Err(e) => { eprintln!("[{conn_id:?}] accept_bi failed: {e}"); return; }
    };

    println!("Connection established: {conn_id:?}");

    spawn_unordered_reader(conn_id, connection.clone(), event_tx.clone());
    spawn_datagram_reader(conn_id, connection.clone(), event_tx.clone());

    // Monitor for connection close.
    let event_tx_clone = event_tx.clone();
    let connection_clone = connection.clone();
    tokio::spawn(async move {
        let reason = connection_clone.closed().await;
        eprintln!("[{conn_id:?}] Closed: {reason}");
        let _ = event_tx_clone.send(InternalEvent::Disconnected { conn_id });
    });

    let _ = event_tx.send(InternalEvent::NewConnection {
        conn_id,
        connection,
        ordered_recv,
        ordered_send,
    });
}

// ---------------------------------------------------------------------------
// Reader tasks
// ---------------------------------------------------------------------------

fn spawn_ordered_reader(
    conn_id: ConnectionId,
    mut recv: RecvStream,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
) {
    tokio::spawn(async move {
        loop {
            // Read 4-byte little-endian length prefix.
            let mut len_buf = [0u8; 4];
            if let Err(e) = recv.read_exact(&mut len_buf).await {
                eprintln!("[{conn_id:?}] Ordered read closed: {e}");
                break;
            }
            let len = u32::from_le_bytes(len_buf) as usize;

            if len == 0 || len > 4 * 1024 * 1024 {
                eprintln!("[{conn_id:?}] Bad ordered frame length: {len}");
                break;
            }

            let mut buf = vec![0u8; len];
            if let Err(e) = recv.read_exact(&mut buf).await {
                eprintln!("[{conn_id:?}] Ordered frame read error: {e}");
                break;
            }

            let _ = event_tx.send(InternalEvent::Inbound {
                conn_id,
                channel: Channel::Ordered,
                data: buf,
            });
        }
    });
}

fn spawn_unordered_reader(
    conn_id: ConnectionId,
    connection: Connection,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
) {
    tokio::spawn(async move {
        loop {
            match connection.accept_uni().await {
                Ok(mut recv) => {
                    let tx = event_tx.clone();
                    tokio::spawn(async move {
                        match recv.read_to_end(1024 * 1024).await {
                            Ok(data) => {
                                let _ = tx.send(InternalEvent::Inbound {
                                    conn_id,
                                    channel: Channel::Unordered,
                                    data,
                                });
                            }
                            Err(e) => eprintln!("[{conn_id:?}] Unordered read error: {e}"),
                        }
                    });
                }
                Err(e) => { eprintln!("[{conn_id:?}] accept_uni closed: {e}"); break; }
            }
        }
    });
}

fn spawn_datagram_reader(
    conn_id: ConnectionId,
    connection: Connection,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
) {
    tokio::spawn(async move {
        loop {
            match connection.read_datagram().await {
                Ok(data) => {
                    let _ = event_tx.send(InternalEvent::Inbound {
                        conn_id,
                        channel: Channel::Unreliable,
                        data: data.to_vec(),
                    });
                }
                Err(e) => { eprintln!("[{conn_id:?}] Datagram read closed: {e}"); break; }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Ordered writer task
// ---------------------------------------------------------------------------

/// Owns the ordered SendStream. Receives payloads via channel and writes
/// length-prefixed frames. Keeping the stream alive preserves ordering.
fn spawn_ordered_writer(mut send: SendStream) -> OrderedSender {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);

    tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            // Write 4-byte little-endian length prefix.
            let len = (data.len() as u32).to_le_bytes();
            if let Err(e) = send.write_all(&len).await {
                eprintln!("Ordered writer error (len): {e}");
                break;
            }
            if let Err(e) = send.write_all(&data).await {
                eprintln!("Ordered writer error (data): {e}");
                break;
            }
        }
        let _ = send.finish();
    });

    OrderedSender { tx }
}

// ---------------------------------------------------------------------------
// Bevy systems
// ---------------------------------------------------------------------------

fn process_internal_events(
    mut manager: ResMut<QuicManager>,
    mut established: MessageWriter<ConnectionEstablished>,
    mut lost: MessageWriter<ConnectionLost>,
    mut inbound: MessageWriter<InboundMessage>,
) {
    while let Ok(event) = manager.event_rx.try_recv() {
        match event {
            InternalEvent::NewConnection {
                conn_id,
                connection,
                ordered_recv,
                ordered_send,
            } => {
                let ordered_writer = spawn_ordered_writer(ordered_send);
                spawn_ordered_reader(conn_id, ordered_recv, manager.event_tx.clone());

                manager.peers.insert(conn_id, PeerState {
                    connection,
                    ordered: ordered_writer,
                });

                established.write(ConnectionEstablished { conn_id });
            }

            InternalEvent::Disconnected { conn_id } => {
                manager.peers.remove(&conn_id);
                lost.write(ConnectionLost { conn_id });
            }

            InternalEvent::Inbound { conn_id, channel, data } => {
                inbound.write(InboundMessage {
                    conn_id,
                    channel,
                    payload: data,
                });
            }
        }
    }
}

fn flush_outbound_messages(
    mut messages: MessageReader<OutboundMessage>,
    manager: Res<QuicManager>,
    runtime: Res<TokioRuntime>,
) {
    for msg in messages.read() {
        let peers: Vec<(ConnectionId, &PeerState)> = match &msg.target {
            SendTarget::One(id) => manager
                .peers
                .get(id)
                .map(|p| vec![(*id, p)])
                .unwrap_or_default(),
            SendTarget::All => manager.peers.iter().map(|(id, p)| (*id, p)).collect(),
        };

        for (conn_id, peer) in peers {
            let data = msg.payload.clone();
            match msg.channel {
                Channel::Ordered => {
                    if let Err(e) = peer.ordered.tx.try_send(data) {
                        eprintln!("[{conn_id:?}] Ordered queue full or closed: {e}");
                    }
                }
                Channel::Unordered => {
                    let conn = peer.connection.clone();
                    runtime.spawn(async move {
                        match conn.open_uni().await {
                            Ok(mut send) => {
                                if let Err(e) = send.write_all(&data).await {
                                    eprintln!("[{conn_id:?}] Unordered write error: {e}");
                                    return;
                                }
                                if let Err(e) = send.finish() {
                                    eprintln!("[{conn_id:?}] Unordered finish error: {e}");
                                }
                            }
                            Err(e) => eprintln!("[{conn_id:?}] open_uni error: {e}"),
                        }
                    });
                }
                Channel::Unreliable => {
                    let conn = peer.connection.clone();
                    runtime.spawn(async move {
                        if let Err(e) = conn.send_datagram(data.into()) {
                            eprintln!("[{conn_id:?}] Datagram error: {e}");
                        }
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Endpoint construction
// ---------------------------------------------------------------------------

fn make_server_endpoint(addr: SocketAddr) -> anyhow::Result<Endpoint> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
    let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());

    let mut transport = quinn::TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(2 * 1024 * 1024));

    let mut server_config = quinn::ServerConfig::with_single_cert(vec![cert_der], key)?;
    server_config.transport_config(std::sync::Arc::new(transport));

    Ok(Endpoint::server(server_config, addr)?)
}

fn make_client_endpoint() -> anyhow::Result<Endpoint> {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    let mut transport = quinn::TransportConfig::default();
    transport.datagram_receive_buffer_size(Some(2 * 1024 * 1024));

    let mut client_config = quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?,
    ));
    client_config.transport_config(std::sync::Arc::new(transport));

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);

    Ok(endpoint)
}

// ---------------------------------------------------------------------------
// TLS: skip verification (dev only — replace with real certs for production)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> std::sync::Arc<Self> { std::sync::Arc::new(Self) }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
