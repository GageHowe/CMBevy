use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, FireEffect, WeaponComponent, WeaponInput};

pub const RANGE: f32 = 500.0;
pub const DAMAGE: f32 = 25.0;
/// Seconds between shots (10 rounds/sec).
pub const COOLDOWN: f32 = 0.1;

/// Per-instance state for the rifle weapon type.
#[derive(Component, Default)]
pub struct RifleComponent {
    pub cooldown: f32,
    /// Latched when fire is requested; cleared after the shot fires.
    /// Allows tapping fire while on cooldown to queue the next shot.
    pub fire_requested: bool,
}

/// Spawns a rifle entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        RifleComponent::default(),
        WeaponInput::default(),
        GameObjectKind::Rifle,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    entity
}

pub fn apply_rifle_fire(
    _world: &mut PhysicsWorld,
    input: WeaponInput,
    dt: f32,
    rifle: &mut RifleComponent,
) -> Option<FireEffect> {
    if input.fire { rifle.fire_requested = true; }
    rifle.cooldown = (rifle.cooldown - dt).max(0.0);
    if !rifle.fire_requested || rifle.cooldown > 0.0 { return None; }
    rifle.cooldown = COOLDOWN;
    rifle.fire_requested = false;
    Some(FireEffect::Hitscan { origin: input.origin, direction: input.aim_dir, range: RANGE, damage: DAMAGE, shooter: input.shooter })
}

/// Adds a scene (GLB model) to an existing rifle entity.
#[cfg(feature = "client")]
pub fn add_visuals(
    entity: Entity,
    commands: &mut Commands,
    asset_server: &bevy::asset::AssetServer,
) {
    commands.entity(entity).insert((
        SceneRoot(asset_server.load("models/ar.glb#Scene0")),
        Visibility::default(),
    ));
}
