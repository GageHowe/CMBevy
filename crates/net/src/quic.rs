use std::{collections::VecDeque, sync::Once};

use bevy::prelude::*;
use bytes::Bytes;
use common::config::MAX_UDP_SIZE;
use tokio::sync::mpsc;
use zstd::stream::{decode_all, encode_all};

use crate::message::MsgType;

const ZSTD_LEVEL: i32 = 3;
const ZSTD_FILE_LEVEL: i32 = 9;
const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

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

#[derive(Debug, Clone)]
/// Destination set for an outgoing message.
pub enum SendTarget {
    One(ConnectionId),
    All,
    AllExcept(ConnectionId),
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
    pub(crate) client_transport: Option<crate::clientonly::ClientTransport>,
    #[cfg(not(feature = "client"))]
    pub(crate) server_transport: Option<crate::serveronly::ServerTransport>,
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

fn decode_message(bytes: &[u8]) -> Result<MsgType, String> {
    let bytes = decode_all(bytes).map_err(|e| format!("decompress: {e}"))?;
    postcard::from_bytes(&bytes).map_err(|e| format!("deserialize: {e}"))
}

pub(crate) fn drain_transport_events(quic: &mut QuicManager, events: Vec<TransportEvent>) {
    for event in events {
        match event {
            TransportEvent::Connected(conn_id) => {
                #[cfg(feature = "client")]
                if conn_id == SERVER_CONN_ID {
                    quic.client_connected = true;
                    println!("Connected to server");
                }
                quic.inbound.push_back(InboundMessage {
                    conn_id,
                    channel: Channel::Ordered,
                    packet_size: 0,
                    msg: MsgType::Connected,
                });
            }
            TransportEvent::Disconnected(conn_id) => {
                #[cfg(feature = "client")]
                if conn_id == SERVER_CONN_ID {
                    quic.client_connected = false;
                    println!("Disconnected from server");
                }
                quic.inbound.push_back(InboundMessage {
                    conn_id,
                    channel: Channel::Ordered,
                    packet_size: 0,
                    msg: MsgType::Disconnected,
                });
            }
            TransportEvent::Message(message) => quic.inbound.push_back(message),
            #[cfg(feature = "client")]
            TransportEvent::Notice(message) => quic.notices.push_back(message),
        }
    }
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
    tokio::spawn(bidi_receiver_task(conn_id, connection.clone(), event_tx.clone()));
    tokio::spawn(uni_receiver_task(conn_id, connection.clone(), event_tx.clone()));
    tokio::spawn(datagram_receiver_task(conn_id, connection.clone(), event_tx.clone()));
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
        Channel::Ordered => {
            let _ = ordered_tx.send(msg.clone());
        }
        Channel::Unordered => {
            let bytes = match encode_message(msg) {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("unordered encode error: {e}");
                    return;
                }
            };
            if let Ok(mut stream) = connection.open_uni().await {
                let _ = stream.write_all(&bytes).await;
                let _ = stream.finish();
            }
        }
        Channel::Unreliable => {
            let bytes = match encode_message(msg) {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("unreliable encode error: {e}");
                    return;
                }
            };
            if bytes.len() > MAX_UDP_SIZE {
                bevy::log::warn!(
                    "unreliable packet too large: {} bytes > {} for {:?}",
                    bytes.len(),
                    MAX_UDP_SIZE,
                    msg
                );
            }
            let _ = connection.send_datagram(Bytes::from(bytes));
        }
    }
}

pub(crate) fn ensure_rustls_crypto_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
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
                let mut bytes = vec![0; u32::from_le_bytes(len) as usize];
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
