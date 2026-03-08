use bevy::prelude::*;
use rapier3d::prelude::*;

use crate::physics::physics_world::*;

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// Input state for a weapon, set by whoever controls it before `fire_weapons<T>` runs.
#[derive(Component, Default, Clone, Copy)]
pub struct WeaponInput {
    pub fire: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
}

/// Generic fire dispatch — analogous to `move_pawns<T>`.
/// Called every tick; `apply` is responsible for its own cooldown gating.
/// `input.fire` is reset to false after each call.
pub fn fire_weapons<T: Component<Mutability = bevy::ecs::component::Mutable>>(
    apply: fn(&mut PhysicsWorld, WeaponInput, f32, &mut T),
) -> impl Fn(ResMut<PhysicsWorld>, Res<Time<Fixed>>, Query<(&mut WeaponInput, &mut T)>) {
    move |mut world, time, mut weapons| {
        let dt = time.delta_secs();
        for (mut input, mut component) in weapons.iter_mut() {
            apply(&mut world, *input, dt, &mut component);
            input.fire = false;
        }
    }
}

/// Inserts a dynamic physics body for a weapon entity.
/// Eventually we'll make unique rigidbodies for each, but it's ok to have one function for now
pub fn insert_weapon_physics(
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
