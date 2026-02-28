use super::runtime::TokioRuntime;
use bevy::prelude::*;
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct QuicPlugin;

impl Plugin for QuicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .init_resource::<InboundQueue>()
            .init_resource::<OutboundQueue>()
            .add_systems(
                Update,
                (
                    process_internal_events,
                    flush_outbound_queue,
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
    Ordered,
    Unordered,
    Unreliable,
}

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub channel: Channel,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum SendTarget {
    One(ConnectionId),
    All,
}

#[derive(Resource, Default)]
pub struct InboundQueue(pub VecDeque<InboundMessage>);

#[derive(Resource, Default)]
pub struct OutboundQueue(VecDeque<(SendTarget, Channel, Vec<u8>)>);

impl OutboundQueue {
    /// This is what other modules should use to send messages
    pub fn send(&mut self, target: SendTarget, channel: Channel, msg: &crate::net::message::MsgType) {
        self.0.push_back((target, channel, wincode::serialize(msg).unwrap()));
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct PeerState {
    connection: Connection,
    ordered_tx: mpsc::Sender<Vec<u8>>,
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
    pub clients: HashSet<ConnectionId>,
    peers: HashMap<ConnectionId, PeerState>,
    next_id: u64,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
    event_rx: mpsc::UnboundedReceiver<InternalEvent>,
}

impl Default for QuicManager {
    fn default() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            clients: HashSet::new(),
            peers: HashMap::new(),
            next_id: 0,
            event_tx,
            event_rx,
        }
    }
}

impl QuicManager {
    pub fn disconnect(&mut self, conn_id: ConnectionId) {
        if let Some(peer) = self.peers.remove(&conn_id) {
            peer.connection.close(0u32.into(), b"disconnect");
        }
    }

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
                    handle_new_connection(conn_id, connection, event_tx, false).await;
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

            handle_new_connection(conn_id, connection, event_tx, true).await;
        });
    }
}

// ---------------------------------------------------------------------------
// Connection setup
// ---------------------------------------------------------------------------

async fn handle_new_connection(
    conn_id: ConnectionId,
    connection: Connection,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
    is_initiator: bool,
) {
    let (ordered_send, ordered_recv) = if is_initiator {
        match connection.open_bi().await {
            Ok(s) => s,
            Err(e) => { eprintln!("[{conn_id:?}] open_bi failed: {e}"); return; }
        }
    } else {
        match connection.accept_bi().await {
            Ok(s) => s,
            Err(e) => { eprintln!("[{conn_id:?}] accept_bi failed: {e}"); return; }
        }
    };

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
// Reader/writer tasks
// ---------------------------------------------------------------------------

fn spawn_ordered_reader(
    conn_id: ConnectionId,
    mut recv: RecvStream,
    event_tx: mpsc::UnboundedSender<InternalEvent>,
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
        loop {
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
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
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
    handle: tokio::runtime::Handle,
) {
    handle.spawn(async move {
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

fn spawn_ordered_writer(
    mut send: SendStream,
    handle: tokio::runtime::Handle,
) -> mpsc::Sender<Vec<u8>> {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);

    handle.spawn(async move {
        while let Some(data) = rx.recv().await {
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

    tx
}

// ---------------------------------------------------------------------------
// Bevy systems
// ---------------------------------------------------------------------------

fn process_internal_events(
    mut manager: ResMut<QuicManager>,
    mut inbound: ResMut<InboundQueue>,
    runtime: Res<TokioRuntime>,
) {
    let handle = runtime.handle();

    while let Ok(event) = manager.event_rx.try_recv() {
        match event {
            InternalEvent::NewConnection {
                conn_id,
                connection,
                ordered_recv,
                ordered_send,
            } => {
                let ordered_tx = spawn_ordered_writer(ordered_send, handle.clone());
                spawn_ordered_reader(conn_id, ordered_recv, manager.event_tx.clone(), handle.clone());
                spawn_unordered_reader(conn_id, connection.clone(), manager.event_tx.clone(), handle.clone());
                spawn_datagram_reader(conn_id, connection.clone(), manager.event_tx.clone(), handle.clone());

                manager.peers.insert(conn_id, PeerState {
                    connection,
                    ordered_tx,
                });

                manager.clients.insert(conn_id);
                println!("Connected: {conn_id:?}");
            }

            InternalEvent::Disconnected { conn_id } => {
                manager.peers.remove(&conn_id);
                manager.clients.remove(&conn_id);
                println!("Disconnected: {conn_id:?}");
            }

            InternalEvent::Inbound { conn_id, channel, data } => {
                inbound.0.push_back(InboundMessage { conn_id, channel, payload: data });
            }
        }
    }
}

fn flush_outbound_queue(
    mut queue: ResMut<OutboundQueue>,
    manager: Res<QuicManager>,
    runtime: Res<TokioRuntime>,
) {
    let handle = runtime.handle();

    while let Some((target, channel, payload)) = queue.0.pop_front() {
        let peers: Vec<(ConnectionId, &PeerState)> = match &target {
            SendTarget::One(id) => manager
                .peers
                .get(id)
                .map(|p| vec![(*id, p)])
                .unwrap_or_default(),
            SendTarget::All => manager.peers.iter().map(|(id, p)| (*id, p)).collect(),
        };

        for (conn_id, peer) in peers {
            let data = payload.clone();
            match channel {
                Channel::Ordered => {
                    if let Err(e) = peer.ordered_tx.try_send(data) {
                        eprintln!("[{conn_id:?}] Ordered queue full or closed: {e}");
                    }
                }
                Channel::Unordered => {
                    let conn = peer.connection.clone();
                    handle.spawn(async move {
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
                    handle.spawn(async move {
                        if let Err(e) = conn.send_datagram(data.into()) {
                            eprintln!("[{conn_id:?}] Datagram error: {e}");
                        }
                    });
                }
            }
        }
    }
}

// ENDPOINT CONSTRUCTION

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