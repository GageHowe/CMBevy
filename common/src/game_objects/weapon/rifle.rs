use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, FireEffect, WeaponComponent, WeaponInput, WeaponKind};

pub const RANGE: f32 = 500.0;
pub const DAMAGE: f32 = 25.0;
/// Ticks between shots (10 rounds/sec at 60 Hz).
pub const COOLDOWN_TICKS: u32 = 6;

impl WeaponKind for RifleComponent {
    const MODEL_PATH: &'static str = "models/ar.glb#Scene0";
}

/// Per-instance state for the rifle weapon type.
#[derive(Component, Default)]
pub struct RifleComponent {
    pub cooldown: u32,
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
    rifle: &mut RifleComponent,
) -> Option<FireEffect> {
    if input.fire { rifle.fire_requested = true; }
    rifle.cooldown = rifle.cooldown.saturating_sub(1);
    if !rifle.fire_requested || rifle.cooldown > 0 { return None; }
    rifle.cooldown = COOLDOWN_TICKS;
    rifle.fire_requested = false;
    Some(FireEffect::Hitscan { origin: input.origin, direction: input.aim_dir, range: RANGE, damage: DAMAGE, shooter: input.shooter })
}

