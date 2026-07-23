use std::{
    collections::VecDeque,
    io::Read,
    net::SocketAddr,
    sync::{Mutex, Once},
};

use bevy::prelude::*;
use bytes::Bytes;
use common::config::MAX_UDP_SIZE;
use tokio::sync::mpsc;
use zstd::stream::encode_all;

use crate::net::message::MsgType;

/// TODO: make this a setting for performance/net ratio
const ZSTD_LEVEL: i32 = 3;
const ZSTD_FILE_LEVEL: i32 = 9;
const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
const MAX_DECOMPRESSED_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
const ALPN_PROTOCOL_PREFIX: &str = "critical-mass/";

#[cfg(feature = "client")]
const CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(feature = "client")]
const CONNECT_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(feature = "client")]
const RENDEZVOUS_POLL_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(feature = "client")]
const RENDEZVOUS_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(feature = "client")]
const PUNCH_ATTEMPTS: usize = 100;

#[cfg(not(feature = "client"))]
const HOST_ANNOUNCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
#[cfg(not(feature = "client"))]
const HOST_PUNCH_ATTEMPTS: usize = 50;

/// Opaque transport-level connection identifier assigned by the net layer.
pub type ConnectionId = u64;
/// Synthetic connection id used by the client for the one authoritative server.
pub const SERVER_CONN_ID: ConnectionId = 0;

static RUSTLS_PROVIDER_INIT: Once = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Reliability/ordering mode for a network packet.
pub enum Channel {
    Ordered,
    Unordered,
    Unreliable,
}

#[derive(Debug, Clone)]
/// A decoded message delivered by the transport into the game layer.
pub struct InboundMessage {
    pub conn_id: ConnectionId,
    pub channel: Channel,
    pub packet_size: usize,
    pub msg: MsgType,
}

/// destination set for an outgoing message
#[derive(Debug, Clone)]
pub enum SendTarget {
    One(ConnectionId), // hey you!
    All,
    /// e.g., for replicating animations or weapon state to other clients
    AllExcept(ConnectionId),
}

#[cfg(feature = "client")]
pub(crate) enum ClientCommand {
    Send {
        channel: Channel,
        msg: MsgType,
    },
    Shutdown,
}
#[cfg(feature = "client")]
pub(crate) struct ClientTransport {
    pub(crate) tx: mpsc::UnboundedSender<ClientCommand>,
    pub(crate) rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}

#[cfg(not(feature = "client"))]
pub(crate) enum ServerCommand {
    Accepted(quinn::Connection),
    EnablePunch(String),
    Punch(SocketAddr),
    Send {
        target: SendTarget,
        channel: Channel,
        msg: MsgType,
    },
    ConnectionClosed(ConnectionId),
}
#[cfg(not(feature = "client"))]
pub(crate) struct ServerTransport {
    pub(crate) tx: mpsc::UnboundedSender<ServerCommand>,
    pub(crate) rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}

#[derive(Resource, Default)]
/// Shared QUIC transport resource used by both client and server runtimes.
pub struct QuicManager {
    pub inbound: VecDeque<InboundMessage>,
    pub notices: VecDeque<String>,
    pub(crate) outbound: VecDeque<(SendTarget, Channel, MsgType)>,

    #[cfg(feature = "client")]
    pub client_connected: bool,
    #[cfg(feature = "client")]
    pub(crate) client_transport: Option<ClientTransport>,

    #[cfg(not(feature = "client"))]
    pub(crate) server_transport: Option<ServerTransport>,
}

impl QuicManager {
    pub fn send(&mut self, target: SendTarget, channel: Channel, msg: &MsgType) {
        self.outbound.push_back((target, channel, msg.clone()));
    }

    pub fn send_file(&mut self, target: SendTarget, name: String, data: Vec<u8>) {
        match encode_all(data.as_slice(), ZSTD_FILE_LEVEL) {
            Ok(data) => self.send(target, Channel::Ordered, &MsgType::FileData(name, data)),
            Err(e) => eprintln!("send_file compress error: {e}"),
        }
    }

    #[cfg(feature = "client")]
    pub fn send_to_server(&mut self, channel: Channel, msg: &MsgType) {
        self.outbound.push_back((
            SendTarget::One(SERVER_CONN_ID),
            channel,
            msg.clone(),
        ));
    }

    #[cfg(feature = "client")]
    pub fn connect(&mut self, server_addr: SocketAddr, lobby_id: Option<String>) {
        self.disconnect();
        let (tx, rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || run_client_worker(server_addr, lobby_id, rx, event_tx));
        self.client_transport = Some(ClientTransport {
            tx,
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
    }

    #[cfg(not(feature = "client"))]
    pub fn start_server(&mut self, addr: SocketAddr) {
        if self.server_transport.is_some() {
            return;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let accept_tx = tx.clone();
        std::thread::spawn(move || run_server_worker(addr, accept_tx, rx, event_tx));
        self.server_transport = Some(ServerTransport {
            tx,
            rx: Mutex::new(event_rx),
        });
        println!("Starting QUIC server on {addr}");
    }

    #[cfg(not(feature = "client"))]
    pub fn enable_punch(&self, lobby_id: String) {
        let Some(transport) = &self.server_transport else {
            return;
        };
        let _ = transport.tx.send(ServerCommand::EnablePunch(lobby_id));
    }

    #[cfg(not(feature = "client"))]
    pub fn punch_peer(&self, addr: SocketAddr) {
        let Some(transport) = &self.server_transport else {
            return;
        };
        let _ = transport.tx.send(ServerCommand::Punch(addr));
    }
}

pub struct NetPlugin;
impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>();

        #[cfg(feature = "client")]
        app.add_systems(PreUpdate, process_inbound_client)
            .add_systems(PostUpdate, flush_outbound_client);

        #[cfg(not(feature = "client"))]
        app.add_systems(PreUpdate, process_inbound_server)
            .add_systems(PostUpdate, flush_outbound_server);
    }
}

pub(crate) enum TransportEvent {
    Connected(ConnectionId),
    Disconnected(ConnectionId),
    Message(InboundMessage),
    #[cfg(feature = "client")]
    Notice(String),
}

pub(crate) fn encode_message(msg: &MsgType) -> Result<Vec<u8>, String> {
    let bytes = postcard::to_allocvec(msg).map_err(|e| format!("serialize: {e}"))?;
    encode_all(bytes.as_slice(), ZSTD_LEVEL).map_err(|e| format!("compress: {e}"))
}

fn runtime() -> Option<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            eprintln!("Failed to build transport runtime: {e}");
            e
        })
        .ok()
}

fn alpn_protocol() -> Vec<u8> {
    format!(
        "{ALPN_PROTOCOL_PREFIX}{}",
        common::config::CRITICAL_MASS_VERSION
    )
    .into_bytes()
}

fn tokio_udp(socket: std::net::UdpSocket) -> Option<tokio::net::UdpSocket> {
    let _ = socket.set_nonblocking(true);
    tokio::net::UdpSocket::from_std(socket).ok()
}

fn transport_events(rx: &Mutex<std::sync::mpsc::Receiver<TransportEvent>>) -> Vec<TransportEvent> {
    let Ok(rx) = rx.lock() else { return Vec::new() };
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

#[cfg(feature = "client")]
fn process_inbound_client(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.client_transport else {
        return;
    };
    let events = transport_events(&transport.rx);
    drain_transport_events(&mut quic, events);
}

#[cfg(feature = "client")]
fn flush_outbound_client(mut quic: ResMut<QuicManager>) {
    let Some(tx) = quic
        .client_transport
        .as_ref()
        .map(|transport| transport.tx.clone())
    else {
        quic.outbound.clear();
        return;
    };
    for (_, channel, msg) in quic.outbound.drain(..) {
        let _ = tx.send(ClientCommand::Send { channel, msg });
    }
}

#[cfg(not(feature = "client"))]
fn process_inbound_server(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.server_transport else {
        return;
    };
    let events = transport_events(&transport.rx);
    drain_transport_events(&mut quic, events);
}

#[cfg(not(feature = "client"))]
fn flush_outbound_server(mut quic: ResMut<QuicManager>) {
    let Some(tx) = quic
        .server_transport
        .as_ref()
        .map(|transport| transport.tx.clone())
    else {
        quic.outbound.clear();
        return;
    };
    for (target, channel, msg) in quic.outbound.drain(..) {
        let _ = tx.send(ServerCommand::Send {
            target,
            channel,
            msg,
        });
    }
}

#[cfg(feature = "client")]
#[derive(Debug)]
struct SkipServerVerification(std::sync::Arc<rustls::crypto::CryptoProvider>);

#[cfg(feature = "client")]
impl SkipServerVerification {
    fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        )))
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
fn run_client_worker(
    server_addr: SocketAddr,
    lobby_id: Option<String>,
    mut cmd_rx: mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    let Some(runtime) = runtime() else {
        return;
    };
    runtime.block_on(async move {
        notice(&event_tx, "Binding local UDP socket...");
        let socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(socket) => socket,
            Err(e) => {
                eprintln!("Failed to bind client socket: {e}");
                return;
            }
        };
        let punch_socket = socket.try_clone().ok();
        let mut endpoint = match quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            None,
            socket,
            quinn::default_runtime().expect("quinn runtime"),
        ) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("Failed to bind client endpoint: {e}");
                return;
            }
        };
        endpoint.set_default_client_config(make_client_config());
        let mut server_addr = server_addr;
        if let Some(lobby_id) = lobby_id {
            notice(&event_tx, "Contacting beacon...");
            if let Some((host_addr, token)) = start_rendezvous_join(&event_tx, &lobby_id).await {
                server_addr = host_addr;
                if let Some(socket) = punch_socket {
                    notice(&event_tx, "Punching through NAT...");
                    tokio::spawn(run_client_punch_loop(socket, lobby_id, token, host_addr));
                }
            }
        }
        notice(&event_tx, format!("Opening connection to {server_addr}..."));
        let Some(connection) =
            connect_with_retry(&endpoint, server_addr, &mut cmd_rx, &event_tx).await
        else {
            return;
        };
        notice(&event_tx, "Connected. Loading world...");

        let close_tx = event_tx.clone();
        let ordered_tx = spawn_connection_tasks(
            SERVER_CONN_ID,
            connection.clone(),
            event_tx.clone(),
            move |conn_id| {
                let _ = close_tx.send(TransportEvent::Disconnected(conn_id));
            },
        );
        let _ = event_tx.send(TransportEvent::Connected(SERVER_CONN_ID));

        while let Some(command) = cmd_rx.recv().await {
            match command {
                ClientCommand::Send { channel, msg } => {
                    send_on_connection(&connection, &ordered_tx, channel, &msg).await
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

#[cfg(feature = "client")]
async fn start_rendezvous_join(
    event_tx: &std::sync::mpsc::Sender<TransportEvent>,
    lobby_id: &str,
) -> Option<(SocketAddr, String)> {
    let response = ureq::post(&format!(
        "{}/lobbies/{lobby_id}/join",
        common::config::BEACON_URL
    ))
    .call()
    .ok()?
    .into_json::<http_common::JoinLobbyResponse>()
    .ok()?;
    if let Some(host) = response
        .host
        .as_deref()
        .and_then(|addr| addr.parse::<SocketAddr>().ok())
    {
        notice(event_tx, format!("Found host at {host}."));
        return Some((host, response.token));
    }
    notice(event_tx, "Waiting for host NAT rendezvous...");
    let deadline = tokio::time::Instant::now() + RENDEZVOUS_WAIT_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(RENDEZVOUS_POLL_DELAY).await;
        let Ok(status) = ureq::get(&format!(
            "{}/lobbies/{}/join/{}",
            common::config::BEACON_URL,
            lobby_id,
            response.token
        ))
        .call() else {
            continue;
        };
        let Ok(status) = status.into_json::<http_common::JoinStatusResponse>() else {
            continue;
        };
        if let Some(host) = status
            .host
            .as_deref()
            .and_then(|addr| addr.parse::<SocketAddr>().ok())
        {
            notice(event_tx, format!("Host replied from {host}."));
            return Some((host, response.token));
        }
    }
    notice(
        event_tx,
        "Host NAT rendezvous timed out. Trying direct address...",
    );
    None
}

#[cfg(feature = "client")]
async fn run_client_punch_loop(
    socket: std::net::UdpSocket,
    lobby_id: String,
    token: String,
    host_addr: SocketAddr,
) {
    let Some(socket) = tokio_udp(socket) else {
        return;
    };
    let beacon_addr = common::config::beacon_rendezvous_addr();
    let join = format!("join:{lobby_id}:{token}");
    for _ in 0..PUNCH_ATTEMPTS {
        let _ = socket.send_to(join.as_bytes(), &beacon_addr).await;
        let _ = socket.send_to(b"cm", host_addr).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(feature = "client")]
async fn connect_once(
    endpoint: &quinn::Endpoint,
    server_addr: SocketAddr,
) -> Result<quinn::Connection, String> {
    endpoint
        .connect(server_addr, "localhost")
        .map_err(|e| format!("start connect: {e}"))?
        .await
        .map_err(|e| format!("open connection: {e}"))
}

#[cfg(feature = "client")]
async fn connect_with_retry(
    endpoint: &quinn::Endpoint,
    server_addr: SocketAddr,
    cmd_rx: &mut mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: &std::sync::mpsc::Sender<TransportEvent>,
) -> Option<quinn::Connection> {
    let mut warned_retry = false;
    loop {
        match cmd_rx.try_recv() {
            Ok(ClientCommand::Shutdown) | Err(mpsc::error::TryRecvError::Disconnected) => {
                return None;
            }
            Ok(ClientCommand::Send { .. }) | Err(mpsc::error::TryRecvError::Empty) => {}
        }
        match tokio::time::timeout(CONNECT_ATTEMPT_TIMEOUT, connect_once(endpoint, server_addr))
            .await
        {
            Ok(Ok(connection)) => return Some(connection),
            Ok(Err(e)) => eprintln!("Failed to open QUIC connection: {e}; retrying..."),
            Err(_) => eprintln!("Timed out opening QUIC connection; retrying..."),
        }
        if !warned_retry {
            warned_retry = true;
            notice(event_tx, "Connection not open yet. Retrying...");
        }
        tokio::time::sleep(CONNECT_RETRY_DELAY).await;
    }
}

#[cfg(feature = "client")]
fn notice(event_tx: &std::sync::mpsc::Sender<TransportEvent>, text: impl Into<String>) {
    let _ = event_tx.send(TransportEvent::Notice(text.into()));
}

#[cfg(feature = "client")]
fn make_client_config() -> quinn::ClientConfig {
    ensure_rustls_crypto_provider();
    let mut client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![alpn_protocol()];
    quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
            .expect("valid client quic config"),
    ))
}

#[cfg(not(feature = "client"))]
struct ServerConnection {
    connection: quinn::Connection,
    ordered_tx: mpsc::UnboundedSender<MsgType>,
}

#[cfg(not(feature = "client"))]
fn run_server_worker(
    addr: SocketAddr,
    accept_tx: mpsc::UnboundedSender<ServerCommand>,
    mut cmd_rx: mpsc::UnboundedReceiver<ServerCommand>,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    let Some(runtime) = runtime() else {
        return;
    };
    runtime.block_on(async move {
        let socket = match std::net::UdpSocket::bind(addr) {
            Ok(socket) => socket,
            Err(e) => {
                eprintln!("Failed to bind server socket: {e}");
                return;
            }
        };
        let punch_socket = socket.try_clone().ok();
        let endpoint = match make_server_endpoint(socket) {
            Ok(endpoint) => endpoint,
            Err(e) => {
                eprintln!("Failed to start QUIC server: {e}");
                return;
            }
        };
        let close_tx = accept_tx.clone();
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

        let mut next_conn_id: ConnectionId = 1;
        let mut connections = std::collections::HashMap::new();
        while let Some(command) = cmd_rx.recv().await {
            match command {
                ServerCommand::Accepted(connection) => {
                    let conn_id = next_conn_id;
                    next_conn_id = next_conn_id.wrapping_add(1);
                    let tx = close_tx.clone();
                    let ordered_tx = spawn_connection_tasks(
                        conn_id,
                        connection.clone(),
                        event_tx.clone(),
                        move |conn_id| {
                            let _ = tx.send(ServerCommand::ConnectionClosed(conn_id));
                        },
                    );
                    connections.insert(
                        conn_id,
                        ServerConnection {
                            connection,
                            ordered_tx,
                        },
                    );
                    println!("Client connected: {conn_id}");
                    let _ = event_tx.send(TransportEvent::Connected(conn_id));
                }
                ServerCommand::EnablePunch(lobby_id) => {
                    if let Some(socket) = cloned_socket(&punch_socket) {
                        tokio::spawn(run_host_announce_loop(socket, lobby_id));
                    }
                }
                ServerCommand::Punch(addr) => {
                    if let Some(socket) = cloned_socket(&punch_socket) {
                        tokio::spawn(send_punch(socket, addr));
                    }
                }
                ServerCommand::Send {
                    target,
                    channel,
                    msg,
                } => send_to_targets(&connections, target, channel, &msg).await,
                ServerCommand::ConnectionClosed(conn_id) => {
                    connections.remove(&conn_id);
                    println!("Client disconnected: {conn_id}");
                    let _ = event_tx.send(TransportEvent::Disconnected(conn_id));
                }
            }
        }
        endpoint.wait_idle().await;
    });
}

#[cfg(not(feature = "client"))]
async fn send_to_targets(
    connections: &std::collections::HashMap<ConnectionId, ServerConnection>,
    target: SendTarget,
    channel: Channel,
    msg: &MsgType,
) {
    match target {
        SendTarget::All => {
            for connection in connections.values() {
                send_on_connection(&connection.connection, &connection.ordered_tx, channel, msg)
                    .await;
            }
        }
        SendTarget::One(conn_id) => {
            if let Some(connection) = connections.get(&conn_id) {
                send_on_connection(&connection.connection, &connection.ordered_tx, channel, msg)
                    .await;
            }
        }
        SendTarget::AllExcept(excluded) => {
            for (&conn_id, connection) in connections.iter() {
                if conn_id != excluded {
                    send_on_connection(
                        &connection.connection,
                        &connection.ordered_tx,
                        channel,
                        msg,
                    )
                    .await;
                }
            }
        }
    }
}

#[cfg(not(feature = "client"))]
fn make_server_endpoint(socket: std::net::UdpSocket) -> Result<quinn::Endpoint, String> {
    ensure_rustls_crypto_provider();
    let addr = socket
        .local_addr()
        .map_err(|e| format!("local addr: {e}"))?;
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into(), addr.ip().to_string()])
        .map_err(|e| format!("generate cert: {e}"))?;
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    let cert_der = cert.cert.der().clone();
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key.into())
        .map_err(|e| format!("server tls: {e}"))?;
    server_crypto.alpn_protocols = vec![alpn_protocol()];
    let mut server_config = quinn::ServerConfig::with_crypto(std::sync::Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
            .map_err(|e| format!("server quic tls: {e}"))?,
    ));
    if let Some(transport) = std::sync::Arc::get_mut(&mut server_config.transport) {
        transport.max_concurrent_uni_streams(1024u32.into());
        transport.max_concurrent_bidi_streams(1024u32.into());
        transport.datagram_receive_buffer_size(Some(1024 * 1024));
    }
    quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        quinn::default_runtime().expect("quinn runtime"),
    )
    .map_err(|e| format!("endpoint: {e}"))
}

#[cfg(not(feature = "client"))]
async fn run_host_announce_loop(socket: std::net::UdpSocket, lobby_id: String) {
    let Some(socket) = tokio_udp(socket) else {
        return;
    };
    let beacon_addr = common::config::beacon_rendezvous_addr();
    let announce = format!("host:{lobby_id}");
    loop {
        let _ = socket.send_to(announce.as_bytes(), &beacon_addr).await;
        tokio::time::sleep(HOST_ANNOUNCE_INTERVAL).await;
    }
}

#[cfg(not(feature = "client"))]
async fn send_punch(socket: std::net::UdpSocket, addr: SocketAddr) {
    let Some(socket) = tokio_udp(socket) else {
        return;
    };
    for _ in 0..HOST_PUNCH_ATTEMPTS {
        let _ = socket.send_to(b"cm", addr).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn decode_message(bytes: &[u8]) -> Result<MsgType, String> {
    if bytes.len() > MAX_MESSAGE_SIZE {
        return Err("compressed message too large".into());
    }
    let mut decoder =
        zstd::stream::read::Decoder::new(bytes).map_err(|e| format!("decompress: {e}"))?;
    let mut bytes = Vec::new();
    decoder
        .by_ref()
        .take((MAX_DECOMPRESSED_MESSAGE_SIZE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("decompress: {e}"))?;
    if bytes.len() > MAX_DECOMPRESSED_MESSAGE_SIZE {
        return Err("decompressed message too large".into());
    }
    postcard::from_bytes(&bytes).map_err(|e| format!("deserialize: {e}"))
}

pub(crate) fn drain_transport_events(quic: &mut QuicManager, events: Vec<TransportEvent>) {
    for event in events {
        match event {
            TransportEvent::Connected(conn_id) => {
                #[cfg(feature = "client")]
                update_client_connection_state(quic, conn_id, true);
                push_status_message(quic, conn_id, MsgType::Connected);
            }
            TransportEvent::Disconnected(conn_id) => {
                #[cfg(feature = "client")]
                update_client_connection_state(quic, conn_id, false);
                push_status_message(quic, conn_id, MsgType::Disconnected);
            }
            TransportEvent::Message(message) => quic.inbound.push_back(message),
            #[cfg(feature = "client")]
            TransportEvent::Notice(message) => quic.notices.push_back(message),
        }
    }
}

fn push_status_message(quic: &mut QuicManager, conn_id: ConnectionId, msg: MsgType) {
    quic.inbound.push_back(InboundMessage {
        conn_id,
        channel: Channel::Ordered,
        packet_size: 0,
        msg,
    });
}

#[cfg(feature = "client")]
fn update_client_connection_state(quic: &mut QuicManager, conn_id: ConnectionId, connected: bool) {
    if conn_id != SERVER_CONN_ID {
        return;
    }
    quic.client_connected = connected;
    println!(
        "{} server",
        if connected {
            "Connected to"
        } else {
            "Disconnected from"
        }
    );
}

pub(crate) fn forward_decoded_message(
    conn_id: ConnectionId,
    channel: Channel,
    bytes: Vec<u8>,
    event_tx: &std::sync::mpsc::Sender<TransportEvent>,
) {
    let packet_size = bytes.len();
    match decode_message(&bytes) {
        Ok(msg) => {
            let _ = event_tx.send(TransportEvent::Message(InboundMessage {
                conn_id,
                channel,
                packet_size,
                msg,
            }));
        }
        Err(e) => eprintln!("[conn {conn_id}] decode error: {e}"),
    }
}

pub(crate) fn spawn_connection_tasks<F>(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
    on_close: F,
) -> mpsc::UnboundedSender<MsgType>
where
    F: FnOnce(ConnectionId) + Send + 'static,
{
    let (ordered_tx, ordered_rx) = mpsc::unbounded_channel();
    tokio::spawn(ordered_sender_task(connection.clone(), ordered_rx));
    tokio::spawn(bidi_receiver_task(
        conn_id,
        connection.clone(),
        event_tx.clone(),
    ));
    tokio::spawn(uni_receiver_task(
        conn_id,
        connection.clone(),
        event_tx.clone(),
    ));
    tokio::spawn(datagram_receiver_task(
        conn_id,
        connection.clone(),
        event_tx.clone(),
    ));
    tokio::spawn(async move {
        let _ = connection.closed().await;
        on_close(conn_id);
    });
    ordered_tx
}

pub(crate) async fn send_on_connection(
    connection: &quinn::Connection,
    ordered_tx: &mpsc::UnboundedSender<MsgType>,
    channel: Channel,
    msg: &MsgType,
) {
    match channel {
        Channel::Ordered => _ = ordered_tx.send(msg.clone()),
        Channel::Unordered => {
            let Some(bytes) = encoded_bytes(msg, "unordered") else {
                return;
            };
            if let Ok(mut stream) = connection.open_uni().await {
                let _ = stream.write_all(&bytes).await;
                let _ = stream.finish();
            }
        }
        Channel::Unreliable => {
            let Some(bytes) = encoded_bytes(msg, "unreliable") else {
                return;
            };
            if bytes.len() > MAX_UDP_SIZE {
                eprintln!(
                    "unreliable packet too large: {} bytes > {} for {:?}",
                    bytes.len(),
                    MAX_UDP_SIZE,
                    msg
                );
                return;
            }
            let _ = connection.send_datagram(Bytes::from(bytes));
        }
    }
}

fn encoded_bytes(msg: &MsgType, label: &str) -> Option<Vec<u8>> {
    match encode_message(msg) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            eprintln!("{label} encode error: {e}");
            None
        }
    }
}

pub(crate) fn ensure_rustls_crypto_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(not(feature = "client"))]
fn cloned_socket(socket: &Option<std::net::UdpSocket>) -> Option<std::net::UdpSocket> {
    socket.as_ref().and_then(|sock| sock.try_clone().ok())
}

async fn ordered_sender_task(
    connection: quinn::Connection,
    mut rx: mpsc::UnboundedReceiver<MsgType>,
) {
    let (mut send, _) = match connection.open_bi().await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to open ordered stream: {e}");
            return;
        }
    };
    while let Some(msg) = rx.recv().await {
        let bytes = match encode_message(&msg) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("ordered encode error: {e}");
                continue;
            }
        };
        let len = (bytes.len() as u32).to_le_bytes();
        if send.write_all(&len).await.is_err() || send.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = send.finish();
}

async fn bidi_receiver_task(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    while let Ok((_, mut recv)) = connection.accept_bi().await {
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            loop {
                let mut len = [0u8; 4];
                if recv.read_exact(&mut len).await.is_err() {
                    break;
                }
                let len = u32::from_le_bytes(len) as usize;
                if len > MAX_MESSAGE_SIZE {
                    break;
                }
                let mut bytes = vec![0; len];
                if recv.read_exact(&mut bytes).await.is_err() {
                    break;
                }
                forward_decoded_message(conn_id, Channel::Ordered, bytes, &event_tx);
            }
        });
    }
}

async fn uni_receiver_task(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    while let Ok(mut stream) = connection.accept_uni().await {
        let event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Ok(bytes) = stream.read_to_end(MAX_MESSAGE_SIZE).await {
                forward_decoded_message(conn_id, Channel::Unordered, bytes, &event_tx);
            }
        });
    }
}

async fn datagram_receiver_task(
    conn_id: ConnectionId,
    connection: quinn::Connection,
    event_tx: std::sync::mpsc::Sender<TransportEvent>,
) {
    while let Ok(bytes) = connection.read_datagram().await {
        forward_decoded_message(conn_id, Channel::Unreliable, bytes.to_vec(), &event_tx);
    }
}
