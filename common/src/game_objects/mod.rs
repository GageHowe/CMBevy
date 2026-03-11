use serde::{Deserialize, Serialize};

pub mod health;
pub mod pawn;
pub mod weapon;
mod planet;

/// update this as needed; it defines types of game objects that can be spawned
#[derive(Debug, PartialEq, Clone, bevy::ecs::component::Component, Serialize, Deserialize)]
pub enum GameObjectKind {
    Biped,
    Spaceship,
    Rifle,
    Shotgun,
    Planet,
}

