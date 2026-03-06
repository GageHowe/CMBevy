use bevy::prelude::*;
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, WeaponComponent, WeaponStats};

/// Marker component for the rifle weapon type.
#[derive(Component)]
pub struct RifleComponent;

use super::STATS;

/// Spawns a rifle entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        RifleComponent,
        STATS,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    entity
}

/// Adds a mesh and material to an existing rifle entity.
#[cfg(feature = "client")]
pub fn add_visuals(
    entity: Entity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.entity(entity).insert((
        Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(0.4, 0.1, 0.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.15, 0.15, 0.15))),
        Visibility::default(),
    ));
}
