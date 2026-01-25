// Backend.rs

// This file includes types and functions
// needed by client/net.rs and server/net.rs

use std::io;
use std::io::Cursor;
use std::net::{TcpStream, UdpSocket};
use wincode::serialize;
use wincode_derive::{SchemaRead, SchemaWrite};
// use zstd::{Decoder, Encoder};
use zstd::{decode_all, encode_all};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq)]
pub enum Message {
    Ping,
    Pong,
    Data(i32),
    MoreData(String),
    /// address, message
    ChatMessage(String, String),
    // Test(Vec<String>),
}

// the following functions might be unnecessary and may be removed

// "0.0.0.0:0" for client, SERVER_ADDRESS for server
pub fn get_udp_socket(addr: &str) -> UdpSocket {
    let sock = UdpSocket::bind(addr).expect("failed to bind UDP socket");
    sock.set_nonblocking(true)
        .expect("failed to set UDP socket nonblocking");
    sock
}
pub fn get_tcp_stream(addr: &str) -> TcpStream {
    TcpStream::connect(addr).expect("failed to connect TCP stream")
}
pub fn compress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    encode_all(Cursor::new(bytes), 3)
}
pub fn decompress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    decode_all(Cursor::new(bytes))
}

/// TODO: deprecate in favor of send_pkt_batch
pub fn send_single_pkt(sock: &UdpSocket, dst: &str, msg: Message) -> io::Result<()> {
    let bytes = serialize(&msg).map_err(|e| {
        eprintln!("serialize failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize failed")
    })?;

    let compressed = compress(&bytes)?;

    sock.send_to(&compressed, dst)?;
    Ok(())
}

/// Serialize, compress, and send a vector of Messages
pub fn send_udp_batch(sock: &UdpSocket, dst: &str, msgs: Vec<Message>) -> io::Result<()> {
    let bytes = serialize(&msgs).map_err(|e| {
        eprintln!("serialize batch failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize batch failed")
    })?;

    let compressed = compress(&bytes)?;

    sock.send_to(&compressed, dst)?;
    Ok(())
}

/// Sends a collection of Messages to a list of clients. Typically used by the server
pub fn broadcast_udp(sock: &UdpSocket, clients: &[String], msgs: &[Message]) -> io::Result<()> {
    // Serialize the whole batch once
    let bytes = serialize(msgs).map_err(|e| {
        eprintln!("serialize multicast failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize multicast failed")
    })?;

    let compressed = compress(&bytes)?;

    for client in clients {
        sock.send_to(&compressed, client)?;
    }

    Ok(())
}
