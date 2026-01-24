// use crate::core::config::*;
use std::io;
use std::io::Cursor;
use std::net::UdpSocket;
use wincode::{deserialize, serialize};
use wincode_derive::{SchemaRead, SchemaWrite};
use zstd::{Decoder, Encoder};
use zstd::{decode_all, encode_all};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq)]
enum Message {
    Ping,
    Pong,
    Data(i32),
    MoreData(String),
}

impl Message {
    fn handle_on_server(self, src: &str) {
        match self {
            Message::Ping => { /* server behavior */ }
            Message::Pong => { /* server behavior */ }
            Message::Data(v) => { /* server behavior */ }
            _ => {}
        }
    }

    fn handle_on_client(self) {
        match self {
            Message::Ping => { /* client behavior */ }
            Message::Pong => { /* client behavior */ }
            Message::Data(v) => { /* client behavior */ }
            _ => {}
        }
    }
}

// the following functions might be unnecessary and may be removed

// "0.0.0.0:0" for client, SERVER_ADDRESS for server
fn get_udp_socket(addr: &str) -> io::Result<UdpSocket> {
    UdpSocket::bind(addr)
}
fn compress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    encode_all(Cursor::new(bytes), 3)
}
fn decompress(bytes: &[u8]) -> io::Result<Vec<u8>> {
    decode_all(Cursor::new(bytes))
}

/// TODO: deprecate in favor of send_pkt_batch
fn send_single_pkt(sock: &UdpSocket, dst: &str, msg: Message) -> io::Result<()> {
    let bytes = serialize(&msg).map_err(|e| {
        eprintln!("serialize failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize failed")
    })?;

    let compressed = compress(&bytes)?;

    sock.send_to(&compressed, dst)?;
    Ok(())
}

/// serialize, compress, and send a vector of Messages
fn send_pkt_batch(sock: &UdpSocket, dst: &str, msgs: Vec<Message>) -> io::Result<()> {
    let bytes = serialize(&msgs).map_err(|e| {
        eprintln!("serialize batch failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize batch failed")
    })?;

    let compressed = compress(&bytes)?;

    sock.send_to(&compressed, dst)?;
    Ok(())
}

// TODO: deprecate this in favor of server_loop_batched
fn server_loop(sock: &UdpSocket) -> io::Result<()> {
    let mut buf = [0u8; 1500];

    loop {
        let (len, src) = sock.recv_from(&mut buf)?;
        match deserialize::<Message>(&buf[..len]) {
            Ok(msg) => msg.handle_on_server(&src.to_string()),
            Err(e) => eprintln!("bad packet from {src}: {e}"),
        }
    }
}

fn server_loop_batched(sock: &UdpSocket) -> io::Result<()> {
    let mut buf = [0u8; 1500];

    loop {
        let (len, src) = sock.recv_from(&mut buf)?;
        let decompressed = match decompress(&buf[..len]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("decompress failed from {src}: {e}");
                continue;
            }
        };

        match deserialize::<Vec<Message>>(&decompressed) {
            Ok(vec) => {
                for msg in vec {
                    msg.handle_on_server(&src.to_string());
                }
            }
            Err(e) => eprintln!("bad packet from {src}: {e}"),
        }
    }
}

fn client_loop_batched(sock: &UdpSocket) -> io::Result<()> {
    let mut buf = [0u8; 1500];

    loop {
        let (len, src) = sock.recv_from(&mut buf)?;
        let decompressed = match decompress(&buf[..len]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("client: decompress failed: {e}");
                continue;
            }
        };

        match deserialize::<Vec<Message>>(&decompressed) {
            Ok(vec) => {
                for msg in vec {
                    msg.handle_on_client();
                }
            }
            Err(e) => eprintln!("client: bad packet: {e}"),
        }
    }
}

fn client_loop(sock: &UdpSocket) -> io::Result<()> {
    let mut buf = [0u8; 1500];
    loop {
        let (len, _src) = sock.recv_from(&mut buf)?;
        match deserialize::<Message>(&buf[..len]) {
            Ok(msg) => msg.handle_on_client(),
            Err(e) => eprintln!("bad packet from server: {e}"),
        }
    }
}
