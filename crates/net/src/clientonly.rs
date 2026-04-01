#![cfg(feature = "client")]

use bevy::prelude::*;
use quinn::crypto::rustls::QuicClientConfig;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Builder;
use tokio::sync::mpsc;

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
    pub fn connect(&mut self, server_addr: SocketAddr) {
        self.disconnect();
        let (tx, rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || run_client_worker(server_addr, rx, event_tx));
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
