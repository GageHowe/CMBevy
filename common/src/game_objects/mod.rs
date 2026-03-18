use bevy::prelude::{Commands, Entity, Transform};
use rapier3d::prelude::RigidBody;
use serde::{Deserialize, Serialize};
use crate::physics::physics_world::PhysicsWorld;

pub mod health;
pub mod pawn;
pub mod weapon;
pub mod planet;
pub mod generic;
pub use generic::{spawn_generic, GenericShape};

/// update this as needed; it defines types of game objects that can be spawned
#[derive(Debug, PartialEq, Clone, bevy::ecs::component::Component, Serialize, Deserialize)]
pub enum GameObjectKind {
    Biped,
    Spaceship,
    Planet,
    Rifle,
    Shotgun,
    HailMary,
    HailMaryProjectile,
}

impl GameObjectKind {
    // this is stupid, don't do this
    // pub fn is_projectile(&self) -> bool {
    //     matches!(self, GameObjectKind::HailMary)
    // }
}

/// new trait that defines functions all GameObjects should have
pub trait GameObject {
    /// handles spawning this GameObject's rigidbody into the world.
    fn spawn_physics(
        transform: Transform,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
    ) -> Entity;
    /// call when despawning, in case this object needs to clean up
    /// basically a destructor
    fn cleanup();
    fn get_rigidbody() -> Option<RigidBody>;
}



// /// A component holding "tags", labels that can be applied and used by the lua gametype code
// /// currently unused, to integrate later
// #[derive(Debug, PartialEq, Clone, bevy::ecs::component::Component, Serialize, Deserialize)]
// pub struct Tags(Vec<String>);
