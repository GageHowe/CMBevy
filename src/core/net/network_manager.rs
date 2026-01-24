// // file contains the NetworkManager Resource struct

// use super::backend::{Message, get_udp_socket, send_udp_batch};
// use bevy::prelude::*;
// use std::net::{TcpListener, UdpSocket};

// #[derive(Resource)]
// pub struct NetworkManager {
//     udp_socket: UdpSocket,
//     // tcp_connection: TcpStream,
//     tcp_listener: TcpListener,
//     pub udp_messages: Vec<Message>,
//     pub tcp_messages: Vec<Message>,
//     pub clients: Vec<String>,
// }

// impl NetworkManager {
//     pub fn new(udp_sock: UdpSocket, tcp_listener: TcpListener) -> Self {
//         Self {
//             udp_socket: udp_sock,
//             tcp_listener: tcp_listener,
//             udp_messages: vec![],
//             tcp_messages: vec![],
//             clients: vec![],
//         }
//     }

//     /// adds a message to the vector to be sent later
//     pub fn add_message() {}
// }

// // if client: client_loop_batched, else server_loop_batched, etc
// pub struct NetManagerPlugin {}
// impl Plugin for NetManagerPlugin {
//     fn build(&self, app: &mut App) {}
// }
