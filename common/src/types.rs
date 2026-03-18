use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Component, Hash)]
pub struct NetworkID(pub u64);

/// server-side resource that keeps track of the next available NetworkID to use
#[derive(Resource, Default)]
pub struct NetworkIDResource {
    last_id: u64
}
impl NetworkIDResource {
    /// should be used when spawning a new networked entity
    pub fn next(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
}

/// networked state of a dynamic rigidbody.
/// stable, do not touch.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: Vec3,
    pub rotation: Quat,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

/// networked message for a set of rigidbodies.
/// stable, do not touch.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    pub bodies: HashMap<NetworkID, BodyState>,
}

/// Input state consumed by movement systems each tick.
#[derive(Component, Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct PawnInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub ability1: bool,
    pub ability2: bool,
    /// Pawn-local yaw angle (radians) from the YawPivot at input time.
    pub look_yaw: f32,
    /// Camera pitch (radians) from the PitchPivot at input time.
    pub look_pitch: f32,
}

/// update this as needed; it defines types of game objects that can be spawned
#[derive(Debug, PartialEq, Clone, Component, Serialize, Deserialize)]
pub enum GameObjectKind {
    Biped,
    Spaceship,
    Planet,
    Rifle,
    Shotgun,
    HailMary,
    HailMaryProjectile,
}
