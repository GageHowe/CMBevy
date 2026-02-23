use super::runtime::TokioRuntime;
use bevy::prelude::*;
use quinn::{Connection, Endpoint};
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
            .add_systems(
                Update,
                (
                    register_new_connections,
                    flush_outbound_messages,
                    collect_inbound_messages,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct ConnectionId(pub u64);

/// Write this message from any system to send data to a peer.
#[derive(Message, Clone)]
pub struct OutboundMessage {
    pub target: SendTarget,
    pub payload: Vec<u8>,
    pub reliability: Reliability,
}

#[derive(Debug, Clone)]
pub enum SendTarget {
    /// Send to one specific peer.
    One(ConnectionId),
    /// Send to every connected peer.
    All,
}

#[derive(Debug, Clone, Copy)]
pub enum Reliability {
    /// Uses a Quinn bidirectional stream. Guaranteed, ordered delivery.
    Reliable,
    /// Uses a Quinn datagram. No guarantee, no order. Max ~1200 bytes.
    Unreliable,
}

/// Read this message from any system to receive data from a peer.
#[derive(Message, Debug, Clone)]
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub reliability: Reliability,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Internal channel messages
// ---------------------------------------------------------------------------

enum InternalInbound {
    Reliable { conn_id: ConnectionId, data: Vec<u8> },
    Unreliable { conn_id: ConnectionId, data: Vec<u8> },
}

struct NewConnection {
    conn_id: ConnectionId,
    connection: Connection,
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct QuicManager {
    connections: HashMap<ConnectionId, Connection>,
    next_id: u64,

    // Tokio → Bevy: newly accepted/established connections
    new_conn_tx: mpsc::UnboundedSender<NewConnection>,
    new_conn_rx: mpsc::UnboundedReceiver<NewConnection>,

    // Tokio → Bevy: inbound data
    inbound_tx: mpsc::UnboundedSender<InternalInbound>,
    inbound_rx: mpsc::UnboundedReceiver<InternalInbound>,
}

impl Default for QuicManager {
    fn default() -> Self {
        let (new_conn_tx, new_conn_rx) = mpsc::unbounded_channel();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
        Self {
            connections: HashMap::new(),
            next_id: 0,
            new_conn_tx,
            new_conn_rx,
            inbound_tx,
            inbound_rx,
        }
    }
}

impl QuicManager {
    // -----------------------------------------------------------------------
    // Setup
    // -----------------------------------------------------------------------

    /// Start a QUIC server. Accepts connections from any peer.
    pub fn start_server(&mut self, runtime: &TokioRuntime, addr: SocketAddr) {
        let new_conn_tx = self.new_conn_tx.clone();
        let inbound_tx = self.inbound_tx.clone();

        // We hand out IDs from inside the async task. Use an atomic so we
        // don't need a mutex.
        use std::sync::atomic::{AtomicU64, Ordering};
        let counter = std::sync::Arc::new(AtomicU64::new(self.next_id));

        runtime.spawn(async move {
            let cert =
                rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
                cert.signing_key.serialize_der().into(),
            );
            let cert_der =
                rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());

            let mut server_config =
                quinn::ServerConfig::with_single_cert(vec![cert_der], key).unwrap();

            // Enable datagrams on the server side.
            let mut transport = quinn::TransportConfig::default();
            transport.datagram_receive_buffer_size(Some(1024 * 1024));
            server_config.transport_config(std::sync::Arc::new(transport));

            let endpoint = Endpoint::server(server_config, addr).unwrap();
            println!("QUIC server listening on {addr}");

            while let Some(incoming) = endpoint.accept().await {
                let connection = match incoming.await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Connection failed: {e}");
                        continue;
                    }
                };

                let conn_id = ConnectionId(counter.fetch_add(1, Ordering::SeqCst));
                println!("Peer connected: {conn_id:?}");

                // Register the connection back on the Bevy side.
                let _ = new_conn_tx.send(NewConnection {
                    conn_id,
                    connection: connection.clone(),
                });

                // Spawn tasks to read streams and datagrams for this peer.
                spawn_reader_tasks(conn_id, connection, inbound_tx.clone());
            }
        });
    }

    /// Connect to a remote QUIC server.
    pub fn connect(&mut self, runtime: &TokioRuntime, server_addr: SocketAddr) {
        let new_conn_tx = self.new_conn_tx.clone();
        let inbound_tx = self.inbound_tx.clone();
        let conn_id = ConnectionId(self.next_id);
        self.next_id += 1;

        runtime.spawn(async move {
            let crypto = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(SkipServerVerification::new())
                .with_no_client_auth();

            let mut transport = quinn::TransportConfig::default();
            transport.datagram_receive_buffer_size(Some(1024 * 1024));

            let mut client_config = quinn::ClientConfig::new(std::sync::Arc::new(
                quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
            ));
            client_config.transport_config(std::sync::Arc::new(transport));

            let mut endpoint =
                Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
            endpoint.set_default_client_config(client_config);

            let connection = match endpoint.connect(server_addr, "localhost") {
                Ok(c) => match c.await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Connection error: {e}");
                        return;
                    }
                },
                Err(e) => {
                    eprintln!("Connect failed: {e}");
                    return;
                }
            };

            println!("Connected to server as {conn_id:?}");

            let _ = new_conn_tx.send(NewConnection {
                conn_id,
                connection: connection.clone(),
            });

            spawn_reader_tasks(conn_id, connection, inbound_tx);
        });
    }
}

// ---------------------------------------------------------------------------
// Bevy systems
// ---------------------------------------------------------------------------

/// Drains the new-connection channel and stores connections in the map.
fn register_new_connections(mut manager: ResMut<QuicManager>) {
    while let Ok(NewConnection { conn_id, connection }) =
        manager.new_conn_rx.try_recv()
    {
        manager.connections.insert(conn_id, connection);
    }
}

/// Drains the `OutboundMessage` queue and sends via Tokio.
fn flush_outbound_messages(
    mut messages: MessageReader<OutboundMessage>,
    manager: Res<QuicManager>,
    runtime: Res<TokioRuntime>,
) {
    for msg in messages.read() {
        let targets: Vec<Connection> = match &msg.target {
            SendTarget::One(id) => manager
                .connections
                .get(id)
                .map(|c| vec![c.clone()])
                .unwrap_or_default(),
            SendTarget::All => manager.connections.values().cloned().collect(),
        };

        for conn in targets {
            let data = msg.payload.clone();
            match msg.reliability {
                Reliability::Reliable => {
                    runtime.spawn(async move {
                        match conn.open_bi().await {
                            Ok((mut send, _recv)) => {
                                if let Err(e) = send.write_all(&data).await {
                                    eprintln!("Write error: {e}");
                                }
                                if let Err(e) = send.finish() {
                                    eprintln!("Stream finish error: {e}");
                                }
                            }
                            Err(e) => eprintln!("open_bi error: {e}"),
                        }
                    });
                }
                Reliability::Unreliable => {
                    runtime.spawn(async move {
                        if let Err(e) = conn.send_datagram(data.into()) {
                            eprintln!("Datagram send error: {e}");
                        }
                    });
                }
            }
        }
    }
}

/// Drains the inbound channel and writes `InboundMessage` Bevy messages.
fn collect_inbound_messages(
    mut manager: ResMut<QuicManager>,
    mut writer: MessageWriter<InboundMessage>,
) {
    while let Ok(msg) = manager.inbound_rx.try_recv() {
        let (conn_id, reliability, payload) = match msg {
            InternalInbound::Reliable { conn_id, data } => {
                (conn_id, Reliability::Reliable, data)
            }
            InternalInbound::Unreliable { conn_id, data } => {
                (conn_id, Reliability::Unreliable, data)
            }
        };
        writer.write(InboundMessage {
            conn_id,
            reliability,
            payload,
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawns two Tokio tasks per connection: one reads bi-streams (reliable),
/// the other reads datagrams (unreliable).
fn spawn_reader_tasks(
    conn_id: ConnectionId,
    connection: Connection,
    inbound_tx: mpsc::UnboundedSender<InternalInbound>,
) {
    // Reliable: accept incoming bi-directional streams.
    let conn_r = connection.clone();
    let tx_r = inbound_tx.clone();
    tokio::spawn(async move {
        loop {
            match conn_r.accept_bi().await {
                Ok((_send, mut recv)) => {
                    let tx = tx_r.clone();
                    tokio::spawn(async move {
                        match recv.read_to_end(1024 * 1024).await {
                            Ok(data) => {
                                let _ = tx.send(InternalInbound::Reliable {
                                    conn_id,
                                    data,
                                });
                            }
                            Err(e) => eprintln!("Stream read error: {e}"),
                        }
                    });
                }
                Err(e) => {
                    eprintln!("accept_bi closed for {conn_id:?}: {e}");
                    break;
                }
            }
        }
    });

    // Unreliable: receive datagrams.
    let conn_u = connection;
    let tx_u = inbound_tx;
    tokio::spawn(async move {
        loop {
            match conn_u.read_datagram().await {
                Ok(data) => {
                    let _ = tx_u.send(InternalInbound::Unreliable {
                        conn_id,
                        data: data.to_vec(),
                    });
                }
                Err(e) => {
                    eprintln!("datagram read closed for {conn_id:?}: {e}");
                    break;
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// TLS helpers
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self)
    }
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