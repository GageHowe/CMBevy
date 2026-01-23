use super::backend::*;
use super::net::*;
use bevy::prelude::*;
use std::io;
use std::net::UdpSocket;

pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        // app.insert_resource(PhysicsWorld::new(Vector3::new(0.0, -9.81, 0.0)))
        //     .add_systems(Startup, init_physics)
        //     .add_systems(
        //         FixedUpdate,
        //         (step_physics, sync_physics_to_transforms).chain(),
        //     );
    }
}

pub struct NetworkManager {
    udp_socket: UdpSocket,
}

impl NetworkManager {
    fn new() {
        // let maybe = get_udp_
    }
}
