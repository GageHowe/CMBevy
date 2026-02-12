use super::runtime::{TokioRuntime, TokioRuntimePlugin};
use bevy::prelude::*;
use quinn::{Connection, Endpoint};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc;

pub struct QuicPlugin;

impl Plugin for QuicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_systems(Update, handle_incoming_messages);
    }
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct ConnectionId(pub u64);

#[derive(Resource)]
pub struct QuicManager {
    pub endpoint: Option<Endpoint>,
    pub connections: HashMap<ConnectionId, Connection>,
    pub rx: mpsc::UnboundedReceiver<(ConnectionId, Vec<u8>)>,
    tx: mpsc::UnboundedSender<(ConnectionId, Vec<u8>)>,
}

impl Default for QuicManager {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            endpoint: None,
            connections: HashMap::new(),
            rx,
            tx,
        }
    }
}

impl QuicManager {
    /// Start a server
    pub fn start_server(&mut self, runtime: &TokioRuntime, addr: SocketAddr) {
        let tx = self.tx.clone();

        runtime.spawn(async move {
            // Create self-signed cert (insecure, but simple)
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let key =
                rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
            let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());

            let server_config = quinn::ServerConfig::with_single_cert(vec![cert_der], key).unwrap();

            let endpoint = Endpoint::server(server_config, addr).unwrap();
            println!("Server listening on {}", addr);

            let mut next_id = 0u64;

            while let Some(conn) = endpoint.accept().await {
                let connection = conn.await.unwrap();
                let conn_id = ConnectionId(next_id);
                next_id += 1;

                println!("Client connected: {:?}", conn_id);

                let tx = tx.clone();

                tokio::spawn(async move {
                    while let Ok((_send, mut recv)) = connection.accept_bi().await {
                        let data = recv.read_to_end(1024 * 1024).await.unwrap();
                        let _ = tx.send((conn_id, data));
                    }
                });
            }
        });
    }

    /// Connect as client
    pub fn connect_client(&mut self, runtime: &TokioRuntime, server_addr: SocketAddr) {
        let tx = self.tx.clone();

        runtime.spawn(async move {
            let crypto = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(SkipServerVerification::new())
                .with_no_client_auth();

            let client_config = quinn::ClientConfig::new(std::sync::Arc::new(
                quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
            ));

            let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
            endpoint.set_default_client_config(client_config);

            let connection = endpoint
                .connect(server_addr, "localhost")
                .unwrap()
                .await
                .unwrap();
            println!("Connected to server");

            // For client, use connection ID 0
            let conn_id = ConnectionId(0);

            while let Ok((_send, mut recv)) = connection.accept_bi().await {
                let data = recv.read_to_end(1024 * 1024).await.unwrap();
                let _ = tx.send((conn_id, data));
            }
        });
    }

    /// Send data to a specific connection
    pub fn send_to(&self, runtime: &TokioRuntime, conn_id: ConnectionId, data: Vec<u8>) {
        if let Some(conn) = self.connections.get(&conn_id) {
            let conn = conn.clone();
            runtime.spawn(async move {
                if let Ok((mut send, _recv)) = conn.open_bi().await {
                    let _ = send.write_all(&data).await;
                    send.finish();
                }
            });
        }
    }

    /// Broadcast to all connected clients
    pub fn broadcast(&self, runtime: &TokioRuntime, data: Vec<u8>) {
        for conn in self.connections.values() {
            let conn = conn.clone();
            let data = data.clone();
            runtime.spawn(async move {
                if let Ok((mut send, _recv)) = conn.open_bi().await {
                    let _ = send.write_all(&data).await;
                    send.finish();
                }
            });
        }
    }
}

// System to handle incoming messages in Bevy
fn handle_incoming_messages(mut manager: ResMut<QuicManager>) {
    while let Ok((conn_id, data)) = manager.rx.try_recv() {
        println!("Received {} bytes from {:?}", data.len(), conn_id);
        // Handle your message here
    }
}

// Skip cert verification helper
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
