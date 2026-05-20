#![cfg(feature = "client")]

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use bevy::prelude::*;
use http_common::{JoinLobbyResponse, JoinStatusResponse};
use quinn::crypto::rustls::QuicClientConfig;
use tokio::{runtime::Builder, sync::mpsc};

const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(250);
const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
const RENDEZVOUS_POLL_DELAY: Duration = Duration::from_millis(250);
const RENDEZVOUS_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const PUNCH_ATTEMPTS: usize = 100;
const ALPN_PROTOCOL_PREFIX: &str = "critical-mass/";

use crate::quic::{
    Channel, QuicManager, SERVER_CONN_ID, TransportEvent, drain_transport_events,
    ensure_rustls_crypto_provider, send_on_connection, spawn_connection_tasks,
};

pub struct NetClientPlugin;

pub(crate) enum ClientCommand {
    Send {
        channel: Channel,
        msg: crate::message::MsgType,
    },
    Shutdown,
}
pub(crate) struct ClientTransport {
    pub(crate) tx: mpsc::UnboundedSender<ClientCommand>,
    pub(crate) rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

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

impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_systems(PreUpdate, process_inbound_client)
            .add_systems(PostUpdate, flush_outbound_client);
    }
}

impl QuicManager {
    /// Client-side send path. `SendTarget` is meaningless on the client.
    pub fn send_to_server(&mut self, channel: Channel, msg: &crate::message::MsgType) {
        self.outbound.push_back((
            crate::quic::SendTarget::One(SERVER_CONN_ID),
            channel,
            msg.clone(),
        ));
    }

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

    pub fn disconnect(&mut self) {
        if let Some(transport) = self.client_transport.take() {
            let _ = transport.tx.send(ClientCommand::Shutdown);
        }
        self.client_connected = false;
    }
}

pub fn process_inbound_client(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.client_transport else {
        return;
    };
    let Ok(rx) = transport.rx.lock() else { return };
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    drop(rx);
    drain_transport_events(&mut quic, events);
}

pub fn flush_outbound_client(mut quic: ResMut<QuicManager>) {
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

fn run_client_worker(
    server_addr: SocketAddr,
    lobby_id: Option<String>,
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
        let mut warned_retry = false;
        let connection = loop {
            match cmd_rx.try_recv() {
                Ok(ClientCommand::Shutdown) | Err(mpsc::error::TryRecvError::Disconnected) => {
                    return;
                }
                Ok(ClientCommand::Send { .. }) | Err(mpsc::error::TryRecvError::Empty) => {}
            }
            match tokio::time::timeout(
                CONNECT_ATTEMPT_TIMEOUT,
                connect_once(&endpoint, server_addr),
            )
            .await
            {
                Ok(Ok(connection)) => break connection,
                Ok(Err(e)) => {
                    let message = format!("Failed to open QUIC connection: {e}; retrying...");
                    eprintln!("{message}");
                    if !warned_retry {
                        warned_retry = true;
                        notice(&event_tx, "Connection not open yet. Retrying...");
                    }
                }
                Err(_) => {
                    let message = "Timed out opening QUIC connection; retrying...".to_string();
                    eprintln!("{message}");
                    if !warned_retry {
                        warned_retry = true;
                        notice(&event_tx, "Connection not open yet. Retrying...");
                    }
                }
            }
            tokio::time::sleep(CONNECT_RETRY_DELAY).await;
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
    .into_json::<JoinLobbyResponse>()
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
        let Ok(status) = status.into_json::<JoinStatusResponse>() else {
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

async fn run_client_punch_loop(
    socket: std::net::UdpSocket,
    lobby_id: String,
    token: String,
    host_addr: SocketAddr,
) {
    let _ = socket.set_nonblocking(true);
    let Ok(socket) = tokio::net::UdpSocket::from_std(socket) else {
        return;
    };
    let beacon_addr = common::config::beacon_rendezvous_addr();
    let join = format!("join:{lobby_id}:{token}");
    for _ in 0..PUNCH_ATTEMPTS {
        let _ = socket.send_to(join.as_bytes(), &beacon_addr).await;
        let _ = socket.send_to(b"cm", host_addr).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

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

fn notice(event_tx: &std::sync::mpsc::Sender<TransportEvent>, text: impl Into<String>) {
    let _ = event_tx.send(TransportEvent::Notice(text.into()));
}

fn make_client_config() -> quinn::ClientConfig {
    ensure_rustls_crypto_provider();
    let mut client_crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    client_crypto.alpn_protocols = vec![
        format!(
            "{ALPN_PROTOCOL_PREFIX}{}",
            common::config::CRITICAL_MASS_VERSION
        )
        .into_bytes(),
    ];
    quinn::ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(client_crypto).expect("valid client quic config"),
    ))
}
