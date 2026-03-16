use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, FireEffect, WeaponComponent, WeaponInput, WeaponKind};

pub const RANGE: f32 = 25.0;
/// Total damage split evenly across all pellets on a direct hit.
pub const DAMAGE: f32 = 80.0;
/// Ticks between shots (1 shot/sec at 60 Hz).
pub const COOLDOWN_TICKS: u32 = 60;
/// Number of pellets fired per shot (client-side spread prediction only).
pub const PELLETS: usize = 8;
/// Half-angle spread in radians per pellet offset.
pub const SPREAD: f32 = 0.08;

impl WeaponKind for ShotgunComponent {
    const MODEL_PATH: &'static str = "models/shotgun.glb#Scene0";
}

/// Per-instance state for the shotgun weapon type.
#[derive(Component, Default)]
pub struct ShotgunComponent {
    pub cooldown: u32,
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

pub fn apply_shotgun_fire(
    _world: &mut PhysicsWorld,
    input: WeaponInput,
    shotgun: &mut ShotgunComponent,
) -> Option<FireEffect> {
    if input.fire { shotgun.fire_requested = true; }
    shotgun.cooldown = shotgun.cooldown.saturating_sub(1);
    if !shotgun.fire_requested || shotgun.cooldown > 0 { return None; }
    shotgun.cooldown = COOLDOWN_TICKS;
    shotgun.fire_requested = false;
    // Client-side: caller can fan out PELLETS rays with SPREAD for VFX using the direction.
    Some(FireEffect::Hitscan { origin: input.origin, direction: input.aim_dir, range: RANGE, damage: DAMAGE, shooter: input.shooter, tick: input.tick })
}
