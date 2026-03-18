use bevy::prelude::{Commands, Entity, Transform};
use rapier3d::prelude::RigidBody;
pub use common::GameObjectKind;

pub mod health;
pub mod sound;
pub mod pawn;
pub mod weapon;
pub mod planet;
pub mod generic;
pub mod level;
pub mod scripting;
pub mod master_plugin;

pub use generic::{spawn_generic, GenericShape};

use physics::physics_world::PhysicsWorld;

pub trait GameObject {
    fn spawn_physics(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity;
    fn cleanup();
    fn get_rigidbody() -> Option<RigidBody>;
}
