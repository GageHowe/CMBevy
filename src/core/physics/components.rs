use bevy::prelude::*;
use rapier3d::prelude::*;

// a way for entities to refer to their rigidbody
#[derive(Component)]
pub struct PhysicsBodyHandle(pub rapier3d::prelude::RigidBodyHandle);
