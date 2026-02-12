// use bevy::prelude::*;
// // use common::net::net::{compress, decompress};
// use common::{
//     config::{CLIENT_CONNECT_ADDRESS, MAX_UDP_SIZE},
//     net::net::MsgType,
// };
// use std::collections::HashMap;
// use std::io::ErrorKind;
// use std::net::UdpSocket;
// use wincode::{deserialize, serialize};

// #[derive(Resource)]
// pub struct ClientNetManager {
//     udp_socket: UdpSocket,
//     pub outgoing_udp: Vec<MsgType>,

//     current_tick: u64,
//     msg_seq: u64, // unique identifier for messages sent
// }

// impl ClientNetManager {
//     pub fn new(udp_sock: UdpSocket) -> Self {
//         Self {
//             udp_socket: udp_sock,
//             outgoing_udp: vec![],

//             current_tick: 0,
//             msg_seq: 0,
//         }
//     }

//     /// prep a unreliable message for sending
//     pub fn enqueue(&mut self, msg: MsgType) {
//         self.outgoing_udp.push(msg);
//     }
// }

// pub struct ClientNetManagerPlugin;
// impl Plugin for ClientNetManagerPlugin {
//     fn build(&self, app: &mut App) {
//         let sock = UdpSocket::bind("0.0.0.0:0").expect("failed to bind UDP socket");
//         sock.set_nonblocking(true)
//             .expect("failed to set UDP socket nonblocking");
//         sock.connect(CLIENT_CONNECT_ADDRESS)
//             .expect("CLIENT: failed to connect socket");

//         app.insert_resource(ClientNetManager::new(sock));
//         // app.add_systems(FixedPreUpdate, handle_messages);
//         app.add_systems(FixedUpdate, increment_tick);
//         // app.add_systems(FixedPostUpdate, flush_outgoing_udp);
//     }
// }

// fn increment_tick(mut man: ResMut<ClientNetManager>) {
//     man.current_tick += 1;
//     // println!("client tick: {}", man.current_tick)
// }

// // /// Handle incoming messages from server
// // pub fn handle_messages(mut manager: ResMut<ClientNetManager>) {
// //     let mut buf = [0u8; MAX_UDP_SIZE];
// //     loop {
// //         let recv_result = manager.udp_socket.recv(&mut buf);

// //         match recv_result {
// //             Ok(len) => {
// //                 let decompressed = match decompress(&buf[..len]) {
// //                     Ok(d) => d,
// //                     Err(e) => {
// //                         eprintln!("CLIENT: decompress failed: {e}");
// //                         continue;
// //                     }
// //                 };
// //                 match deserialize::<Vec<MsgType>>(&decompressed) {
// //                     Ok(msgs) => {
// //                         for msg in msgs {
// //                             handle(&mut manager, msg);
// //                         }
// //                     }
// //                     Err(e) => eprintln!("CLIENT: bad packet: {e}"),
// //                 }
// //             }
// //             Err(e) if e.kind() == ErrorKind::WouldBlock => break,
// //             Err(_) => break,
// //         }
// //     }
// // }

// // fn flush_outgoing_udp(mut manager: ResMut<ClientNetManager>) {
// //     if manager.outgoing_udp.is_empty() {
// //         return;
// //     }
// //     let bytes = serialize(&manager.outgoing_udp).unwrap();
// //     let compressed = compress(&bytes).unwrap();
// //     manager.udp_socket.send(&compressed).unwrap();
// //     manager.outgoing_udp.clear();
// // }

// // fn handle(manager: &mut ClientNetManager, msg: MsgType) {
// //     match msg {
// //         // MsgType::Ping => println!("CLIENT: got a Ping!"),
// //         // MsgType::Pong => println!("CLIENT: got a Pong!"),
// //         // MsgType::Data(v) => println!("CLIENT: got a Data({v})!"),
// //         // MsgType::State(s) => println!("CLIENT: got a State: {:?}", s),
// //         _ => {}
// //     }
// // }
