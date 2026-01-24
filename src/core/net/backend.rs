// Backend.rs

// This file includes types and functions
// needed by client/net.rs and server/net.rs

use std::io;
use std::io::Cursor;
use std::net::{TcpStream, UdpSocket};
use wincode::{deserialize, serialize};
use wincode_derive::{SchemaRead, SchemaWrite};
// use zstd::{Decoder, Encoder};
use zstd::{decode_all, encode_all};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq)]
pub enum Message {
    Ping,
    Pong,
    Data(i32),
    MoreData(String),
}

// impl Message {
//     pub fn handle_on_server(self, src: &str) {
//         match self {
//             Message::Ping => { /* server behavior */ }
//             Message::Pong => { /* server behavior */ }
//             Message::Data(v) => { /* server behavior */ }
//             _ => {}
//         }
//     }

//     pub fn handle_on_client(self) {
//         match self {
//             Message::Ping => { /* client behavior */ }
//             Message::Pong => { /* client behavior */ }
//             Message::Data(v) => { /* client behavior */ }
//             _ => {}
//         }
//     }
// }

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

// /// Recieve loop for the server, handles incoming messages from clients
// pub fn server_loop_batched(sock: &UdpSocket) -> io::Result<()> {
//     let mut buf = [0u8; 1500];

//     loop {
//         let (len, src) = sock.recv_from(&mut buf)?;
//         let decompressed = match decompress(&buf[..len]) {
//             Ok(d) => d,
//             Err(e) => {
//                 eprintln!("decompress failed from {src}: {e}");
//                 continue;
//             }
//         };

//         match deserialize::<Vec<Message>>(&decompressed) {
//             Ok(vec) => {
//                 for msg in vec {
//                     msg.handle_on_server(&src.to_string());
//                 }
//             }
//             Err(e) => eprintln!("bad packet from {src}: {e}"),
//         }
//     }
// }

// /// Receive loop for the client, handles incoming messages from the server
// pub fn client_loop_batched(sock: &UdpSocket) -> io::Result<()> {
//     let mut buf = [0u8; 1500];

//     loop {
//         let (len, src) = sock.recv_from(&mut buf)?;
//         let decompressed = match decompress(&buf[..len]) {
//             Ok(d) => d,
//             Err(e) => {
//                 eprintln!("client: decompress failed: {e}");
//                 continue;
//             }
//         };

//         match deserialize::<Vec<Message>>(&decompressed) {
//             Ok(vec) => {
//                 for msg in vec {
//                     msg.handle_on_client();
//                 }
//             }
//             Err(e) => eprintln!("client: bad packet: {e}"),
//         }
//     }
// }
