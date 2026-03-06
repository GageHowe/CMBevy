use bevy::prelude::*;
use rapier3d::prelude::*;

use crate::physics::physics_world::*;

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// Per-weapon stats used by the server when processing fire requests.
#[derive(Component, Clone, Copy)]
pub struct WeaponStats {
    pub damage: f32,
    pub range: f32,
}

/// Inserts a dynamic physics body for a weapon entity.
/// Shared by all weapon type spawn functions.
pub(crate) fn insert_weapon_physics(
    entity: Entity,
    transform: &Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(2.0)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.2, 0.05, 0.4).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
}
