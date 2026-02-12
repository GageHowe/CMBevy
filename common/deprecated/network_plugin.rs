// // Simple Bevy Network Plugin using Quinn (QUIC)
// // Handles reliable and unreliable messaging between client and server

// use bevy::prelude::*;
// use quinn::{Connection, Endpoint, RecvStream, SendStream, ServerConfig};
// use std::net::SocketAddr;
// use std::sync::Arc;
// use tokio::sync::mpsc;

// // Types

// pub struct NetworkPlugin {
//     pub is_server: bool,
//     pub server_addr: SocketAddr,
// }

// impl Plugin for NetworkPlugin {
//     fn build(&self, app: &mut App) {
//         app.insert_resource(NetworkConfig {
//             is_server: self.is_server,
//             server_addr: self.server_addr,
//         })
//         .add_systems(Startup, setup_network)
//         // .add_systems(Update, (send_messages, receive_messages));
//     }
// }

// // Resources

// #[derive(Resource)]
// struct NetworkConfig {
//     is_server: bool,
//     server_addr: SocketAddr,
// }

// #[derive(Resource)]
// struct NetworkState {
//     connection: Option<Connection>,
//     tx: mpsc::UnboundedSender<NetworkMessage>,
//     rx: mpsc::UnboundedReceiver<NetworkMessage>,
// }

// // Message types
// #[derive(Message, Debug, Clone)]
// pub enum NetworkMessage {
//     Reliable(Vec<u8>),
//     Unreliable(Vec<u8>),
// }
// #[derive(Message, Event, Debug)]
// pub struct MessageReceived {
//     pub data: Vec<u8>,
//     pub is_reliable: bool,
// }
// #[derive(Message, Event, Debug)]
// pub struct SendMessage {
//     pub data: Vec<u8>,
//     pub reliable: bool,
// }

// // Functions

// fn setup_network(mut commands: Commands, config: Res<NetworkConfig>) {
//     let (tx, rx) = mpsc::unbounded_channel();

//     commands.insert_resource(NetworkState {
//         connection: None,
//         tx,
//         rx,
//     });

//     // Spawn async task to handle network initialization
//     let is_server = config.is_server;
//     let server_addr = config.server_addr;

//     std::thread::spawn(move || {
//         let runtime = tokio::runtime::Runtime::new().unwrap();
//         runtime.block_on(async {
//             if is_server {
//                 println!("Starting server on {}", server_addr);
//                 // Server setup would go here
//                 // Note: Full implementation needs certificate generation
//             } else {
//                 println!("Connecting to server at {}", server_addr);
//                 // Client connection would go here
//             }
//         });
//     });
// }

// // ============================================================================
// // Send System
// // ============================================================================

// fn send_messages(
//     mut event_reader: MessageReader<SendMessage>,
//     net_state: Option<ResMut<NetworkState>>,
// ) {
//     let Some(mut state) = net_state else { return };
//     let Some(connection) = &state.connection else {
//         return;
//     };

//     for event in event_reader.read() {
//         let msg = if event.reliable {
//             NetworkMessage::Reliable(event.data.clone())
//         } else {
//             NetworkMessage::Unreliable(event.data.clone())
//         };

//         let _ = state.tx.send(msg);
//     }
// }

// // ============================================================================
// // Receive System
// // ============================================================================

// // fn receive_messages(
// //     mut event_writer: EventWriter<MessageReceived>,
// //     mut net_state: Option<ResMut<NetworkState>>,
// // ) {
// //     let Some(mut state) = net_state else { return };

// //     // Poll for received messages
// //     while let Ok(msg) = state.rx.try_recv() {
// //         match msg {
// //             NetworkMessage::Reliable(data) => {
// //                 event_writer.send(MessageReceived {
// //                     data,
// //                     is_reliable: true,
// //                 });
// //             }
// //             NetworkMessage::Unreliable(data) => {
// //                 event_writer.send(MessageReceived {
// //                     data,
// //                     is_reliable: false,
// //                 });
// //             }
// //         }
// //     }
// // }

// // ============================================================================
// // Example Usage
// // ============================================================================

// // #[cfg(test)]
// // mod example {
// //     use super::*;

// //     fn example_server() {
// //         App::new()
// //             .add_plugins(NetworkPlugin {
// //                 is_server: true,
// //                 server_addr: "127.0.0.1:5000".parse().unwrap(),
// //             })
// //             .add_systems(Update, handle_received_messages)
// //             .run();
// //     }

// //     fn example_client() {
// //         App::new()
// //             .add_plugins(NetworkPlugin {
// //                 is_server: false,
// //                 server_addr: "127.0.0.1:5000".parse().unwrap(),
// //             })
// //             // .add_systems(Update, (handle_received_messages, send_example_messages))
// //             .run();
// //     }

// //     fn handle_received_messages(mut events: EventReader<MessageReceived>) {
// //         for msg in events.read() {
// //             println!(
// //                 "Received {} message: {:?}",
// //                 if msg.is_reliable {
// //                     "reliable"
// //                 } else {
// //                     "unreliable"
// //                 },
// //                 String::from_utf8_lossy(&msg.data)
// //             );
// //         }
// //     }

// //     fn send_example_messages(mut events: EventWriter<SendMessage>, time: Res<Time>) {
// //         // Send reliable message every 2 seconds
// //         if time.elapsed_seconds() as u32 % 2 == 0 {
// //             events.send(SendMessage {
// //                 data: b"Reliable message".to_vec(),
// //                 reliable: true,
// //             });
// //         }

// //         // Send unreliable message every frame
// //         events.send(SendMessage {
// //             data: b"Unreliable update".to_vec(),
// //             reliable: false,
// //         });
// //     }
// // }
