#![cfg(not(feature = "client"))]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use bevy::prelude::*;
use quinn::crypto::rustls::QuicServerConfig;
use tokio::{runtime::Builder, sync::mpsc};

use crate::{
    message::MsgType,
    quic::{
        Channel, ConnectionId, QuicManager, SendTarget, TransportEvent, drain_transport_events,
        ensure_rustls_crypto_provider, send_on_connection, spawn_connection_tasks,
    },
};

pub struct NetServerPlugin;

pub(crate) enum ServerCommand {
    Accepted(quinn::Connection),
    Send { target: SendTarget, channel: Channel, msg: MsgType },
    ConnectionClosed(ConnectionId),
}

pub(crate) struct ServerTransport {
    pub(crate) tx: mpsc::UnboundedSender<ServerCommand>,
    pub(crate) rx: Mutex<std::sync::mpsc::Receiver<TransportEvent>>,
}
struct ServerConnection {
    connection: quinn::Connection,
    ordered_tx: mpsc::UnboundedSender<MsgType>,
}

impl Plugin for NetServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuicManager>()
            .add_systems(PreUpdate, process_inbound_server)
            .add_systems(PostUpdate, flush_outbound_server);
    }
}

impl QuicManager {
    pub fn start_server(&mut self, addr: SocketAddr) {
        if self.server_transport.is_some() {
            return;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let accept_tx = tx.clone();
        std::thread::spawn(move || run_server_worker(addr, accept_tx, rx, event_tx));
        self.server_transport = Some(ServerTransport { tx, rx: Mutex::new(event_rx) });
        println!("QUIC server listening on {addr}");
    }
}

pub fn process_inbound_server(mut quic: ResMut<QuicManager>) {
    let Some(transport) = &quic.server_transport else {
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

pub fn flush_outbound_server(mut quic: ResMut<QuicManager>) {
    let Some(tx) = quic.server_transport.as_ref().map(|transport| transport.tx.clone()) else {
        quic.outbound.clear();
        return;
    };
    for (target, channel, msg) in quic.outbound.drain(..) {
        let _ = tx.send(ServerCommand::Send { target, channel, msg });
    }
}

fn run_server_worker(
    addr: SocketAddr,
    accept_tx: mpsc::UnboundedSender<ServerCommand>,
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
        let mut connections = HashMap::new();
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
                    connections.insert(conn_id, ServerConnection { connection, ordered_tx });
                    println!("Client connected: {conn_id}");
                    let _ = event_tx.send(TransportEvent::Connected(conn_id));
                }
                ServerCommand::Send { target, channel, msg } => {
                    send_to_targets(&connections, target, channel, &msg).await
                }
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

async fn send_to_targets(
    connections: &HashMap<ConnectionId, ServerConnection>,
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
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(
        QuicServerConfig::try_from(server_crypto).map_err(|e| format!("server quic tls: {e}"))?,
    ));
    if let Some(transport) = Arc::get_mut(&mut server_config.transport) {
        transport.max_concurrent_uni_streams(1024u32.into());
        transport.max_concurrent_bidi_streams(1024u32.into());
        transport.datagram_receive_buffer_size(Some(1024 * 1024));
    }
    quinn::Endpoint::server(server_config, addr).map_err(|e| format!("endpoint: {e}"))
}
