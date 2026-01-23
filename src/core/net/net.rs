// use bevy_renet::*;

// this module will contain information about packets, synchronization, etc
// use serde::{Deserialize, Serialize};
// use serde::{Deserialize, Serialize};
use std::io;
use std::net::UdpSocket;
// use wincode::
use serde::{Deserialize, Serialize};
use wincode::*;
use wincode::{deserialize, serialize};
use wincode_derive::{SchemaRead, SchemaWrite};

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p = Point { x: 10, y: 20 };

    // Serialize to Vec<u8>
    let bytes: Vec<u8> = serialize(&p).unwrap();

    // Deserialize back to Point
    let decoded: Point = deserialize(&bytes).unwrap();

    assert_eq!(p, decoded);
    println!("Got point back: {:?}", decoded);
}

// https://www.youtube.com/watch?v=fBHO0yptg1Y

// // #[repr(transparent)]
// #[derive(Clone, Serialize, Deserialize, SchemaWrite, SchemaRead)]
// #[wincode(from = "Pkt")]
// pub enum Pkt {
//     Ping,
//     Pong,
//     Msg(String),
// }

// let socket = UdpSocket::bind("0.0.0.0:0")?;

// let target_address = "127.0.0.1:8080"; // The destination IP and port
// let data_to_send = b"Hello from Rust UDP client!"; // A byte slice (&[u8])

// // Send the byte slice to the target address
// match socket.send_to(data_to_send, target_address) {
//     Ok(bytes_sent) => {
//         println!("Sent {} bytes to {}", bytes_sent, target_address);
//     }
//     Err(e) => {
//         eprintln!("Failed to send data: {}", e);
//     }
// }

// let dst = "127.0.0.1:8080";
fn send_pkt(dst: &str) -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    // let encoded_result =
    // wincode::encode(data_to_send, wincode::Options::new().with_native_endian())?;

    // match socket.send_to(data_to_send, dst) {
    //     Ok(bytes_sent) => {
    //         println!("Sent {} bytes to {}", bytes_sent, dst);
    //     }
    //     Err(e) => {
    //         eprintln!("Failed to send data: {}", e);
    //     }
    // }

    Ok(())
}
