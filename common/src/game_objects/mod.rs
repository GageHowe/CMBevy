use serde::{Deserialize, Serialize};

pub mod health;
pub mod pawn;
pub mod weapon;
pub mod planet;

/// update this as needed; it defines types of game objects that can be spawned
#[derive(Debug, PartialEq, Clone, bevy::ecs::component::Component, Serialize, Deserialize)]
pub enum GameObjectKind {
    Biped,
    Spaceship,
    Rifle,
    Shotgun,
    HailMary,
    HailMaryProjectile,
    Planet,
}

impl GameObjectKind {
    pub fn is_projectile(&self) -> bool {
        matches!(self, GameObjectKind::HailMary)
    }
}

/// A component holding "tags", labels that can be applied
/// currently unused
#[derive(Debug, PartialEq, Clone, bevy::ecs::component::Component, Serialize, Deserialize)]
pub struct Tags(Vec<String>);
