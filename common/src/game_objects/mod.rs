use wincode_derive::{SchemaRead, SchemaWrite};

pub mod health;
pub mod pawn;
pub mod weapon;

/// update this as needed; it defines types of game objects that can be spawned
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum GameObjectKind {
    Biped,
    Spaceship,
    Rifle,
    Shotgun,
}

