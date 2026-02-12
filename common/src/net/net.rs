// This file includes types and functions
// needed net/client.rs and net/server.rs

use crate::types::{CMQuat, CMVec3};
use bevy::prelude::*;
use quinn::{ClientConfig, Endpoint, ServerConfig}; // for QUIC
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer}; // for TLS certs
use std::collections::HashMap;
use std::io;
use std::io::Cursor;
// use std::net::{TcpStream, UdpSocket};
use std::{error::Error, net::SocketAddr, sync::Arc};
use wincode::serialize;
use wincode_derive::{SchemaRead, SchemaWrite};
use zstd::{decode_all, encode_all};

// #[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
// pub struct Packet {
//     msgs: Vec<MsgType>,
// }

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum MsgType {
    /// address, message
    ChatMessage(String, String),
    // HitReport()
    // Test(Vec<String>),
    /// A message the recipient will display in messagebar
    Error(String),

    BodyState(BodyState),
    State(SimulationState),
}

// pub struct Message {
//     /// The data this Message contains
//     data: MsgType,
//     /// The unique sequence number of this message
//     seq: i32,
// }

/// Component to mark entities that should be networked
#[derive(Component)]
pub struct NetworkId(pub u32);

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: CMVec3,
    pub rotation: CMQuat,
    pub linvel: CMVec3,
    pub angvel: CMVec3,
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    // Map NetworkId -> (position, rotation, velocity)
    pub bodies: HashMap<u32, BodyState>,
}

/// simple parsing function
pub fn str_to_message(s: &str) -> MsgType {
    match s {
        // "Ping" => MsgType::Ping,
        // "Pong" => MsgType::Pong,
        _ => MsgType::Error("unimplemented".to_string()),
    }
}

// pub fn get_tcp_stream(addr: &str) -> TcpStream {
//     TcpStream::connect(addr).expect("failed to connect TCP stream")
// }
pub fn compress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    encode_all(Cursor::new(bytes), 3)
}
pub fn decompress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    decode_all(Cursor::new(bytes))
}

// /// TODO: deprecate in favor of send_pkt_batch
// pub fn send_single_pkt(sock: &UdpSocket, dst: &str, msg: MsgType) -> io::Result<()> {
//     let bytes = serialize(&msg).map_err(|e| {
//         eprintln!("serialize failed: {e}");
//         io::Error::new(io::ErrorKind::Other, "serialize failed")
//     })?;

//     let compressed = compress(&bytes)?;

//     sock.send_to(&compressed, dst)?;
//     Ok(())
// }

// /// Serialize, compress, and send a vector of Messages
// pub fn send_udp_batch(sock: &UdpSocket, dst: &str, msgs: Vec<MsgType>) -> io::Result<()> {
//     let bytes = serialize(&msgs).map_err(|e| {
//         eprintln!("serialize batch failed: {e}");
//         io::Error::new(io::ErrorKind::Other, "serialize batch failed")
//     })?;

//     let compressed = compress(&bytes)?;

//     sock.send_to(&compressed, dst)?;
//     Ok(())
// }

// /// Sends a collection of Messages to a list of clients. Typically used by the server
// pub fn broadcast_udp(sock: &UdpSocket, clients: &[String], msgs: &[MsgType]) -> io::Result<()> {
//     // Serialize the whole batch once
//     let bytes = serialize(msgs).map_err(|e| {
//         eprintln!("serialize multicast failed: {e}");
//         io::Error::new(io::ErrorKind::Other, "serialize multicast failed")
//     })?;

//     let compressed = compress(&bytes)?;

//     for client in clients {
//         sock.send_to(&compressed, client)?;
//     }

//     Ok(())
// }
