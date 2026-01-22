use bevy::prelude::*;
use rapier3d::prelude::*;

#[derive(Component)]
pub struct RigidBodyDesc {
    pub body_type: RigidBodyType,
}

#[derive(Component)]
pub struct ColliderDesc {
    pub shape: ColliderShape, // enum you define; see below
}

#[derive(Component)]
pub struct RigidBodyHandleComp {
    pub handle: RigidBodyHandle,
}

pub enum ColliderShape {
    CapsuleY { half_height: f32, radius: f32 },
    Cuboid { hx: f32, hy: f32, hz: f32 },
    // Add others as needed
}

// a bundle is a collection of components that can be inserted into an entity at once
#[derive(Bundle)]
pub struct PhysicsMeshBundle {
    pub transform: Transform,
    pub rb: RigidBodyDesc,
    pub col: ColliderDesc,
}
