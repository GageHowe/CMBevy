use crate::core::config::*;
use serde::{Deserialize, Serialize};
use std::io;
use std::net::UdpSocket;
use wincode::*;
use wincode::{deserialize, serialize};
use wincode_derive::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq)]
enum Message {
    Ping,
    Pong,
    Data(i32),
}

impl Message {
    fn handle_on_server(self, src: &str) {
        match self {
            Message::Ping => { /* server behavior */ }
            Message::Pong => { /* server behavior */ }
            Message::Data(v) => { /* server behavior */ }
        }
    }

    fn handle_on_client(self) {
        match self {
            Message::Ping => { /* client behavior */ }
            Message::Pong => { /* client behavior */ }
            Message::Data(v) => { /* client behavior */ }
        }
    }
}

// "0.0.0.0:0" for client, SERVER_ADDRESS for server
fn get_client_udp_socket(addr: &str) -> io::Result<UdpSocket> {
    UdpSocket::bind(addr)
}

fn send_single_pkt(sock: &UdpSocket, dst: &str, msg: Message) -> io::Result<()> {
    let bytes = serialize(&msg).map_err(|e| {
        eprintln!("serialize failed: {e}");
        io::Error::new(io::ErrorKind::Other, "serialize failed")
    })?;

    sock.send_to(&bytes, dst)?;
    Ok(())
}

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
