use bevy::prelude::*;
// use std::io;
use super::network_manager;
use std::net::UdpSocket;
pub struct ClientNetworkPlugin;

impl Plugin for ClientNetworkPlugin {
    fn build(&self, app: &mut App) {}
}
