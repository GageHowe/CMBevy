// use super::backend::*;
use bevy::prelude::*;
// use std::io;
use super::network_manager;
use std::net::UdpSocket;

pub struct ServerNetworkPlugin;

impl Plugin for ServerNetworkPlugin {
    fn build(&self, app: &mut App) {}
}
