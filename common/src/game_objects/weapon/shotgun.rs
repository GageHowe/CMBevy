use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, FireEffect, WeaponComponent, WeaponInput};

pub const RANGE: f32 = 25.0;
/// Total damage split evenly across all pellets on a direct hit.
pub const DAMAGE: f32 = 80.0;
/// Seconds between shots (1 shot/sec).
pub const COOLDOWN: f32 = 1.0;
/// Number of pellets fired per shot (client-side spread prediction only).
pub const PELLETS: usize = 8;
/// Half-angle spread in radians per pellet offset.
pub const SPREAD: f32 = 0.08;

/// Per-instance state for the shotgun weapon type.
#[derive(Component, Default)]
pub struct ShotgunComponent {
    pub cooldown: f32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
}

/// Spawns a shotgun entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        ShotgunComponent::default(),
        WeaponInput::default(),
        GameObjectKind::Shotgun,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    entity
}

/// Adds a scene (GLB model) to an existing shotgun entity.
pub fn add_visuals(
    entity: Entity,
    commands: &mut Commands,
    asset_server: &bevy::asset::AssetServer,
) {
    commands.entity(entity).insert((
        SceneRoot(asset_server.load("models/shotgun.glb#Scene0")),
        Visibility::default(),
    ));
}

pub fn apply_shotgun_fire(
    _world: &mut PhysicsWorld,
    input: WeaponInput,
    dt: f32,
    shotgun: &mut ShotgunComponent,
) -> Option<FireEffect> {
    if input.fire { shotgun.fire_requested = true; }
    shotgun.cooldown = (shotgun.cooldown - dt).max(0.0);
    if !shotgun.fire_requested || shotgun.cooldown > 0.0 { return None; }
    shotgun.cooldown = COOLDOWN;
    shotgun.fire_requested = false;
    // Client-side: caller can fan out PELLETS rays with SPREAD for VFX using the direction.
    Some(FireEffect::Hitscan { origin: input.origin, direction: input.aim_dir, range: RANGE, damage: DAMAGE, shooter: input.shooter })
}
