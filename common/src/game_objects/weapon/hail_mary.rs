use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::physics::debug::{draw_collider, rb_iso};
use crate::interaction::Interactable;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, FireEffect, WeaponComponent, WeaponInput, WeaponKind};

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

pub const DAMAGE: f32 = 100.0;
pub const COOLDOWN: f32 = 2.5;
/// Projectile speed in m/s.
pub const PROJECTILE_SPEED: f32 = 300.0;

// didnt know impls could have consts lol
impl WeaponKind for HailMaryComponent {
    const MODEL_PATH: &'static str = "models/hail_mary_placeholder_2.glb#Scene0";
    const HULL_PATH: &'static str = "collision/hail_mary_placeholder_2.obj";
    const SCALE: f32 = 10.0;
}

#[derive(Component, Default)]
pub struct HailMaryComponent {
    pub cooldown: f32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    /// Ticks remaining for muzzle flash visibility. Set to MUZZLE_FLASH_TICKS on fire.
    pub muzzle_flash_ticks: u8,
}

const MUZZLE_FLASH_TICKS: u8 = 3;

/// Spawns a Hail Mary entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        HailMaryComponent::default(),
        WeaponInput::default(),
        GameObjectKind::HailMary,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    entity
}

/// Marks a locally-predicted projectile spawned before server confirmation.
/// Cleaned up when a `SpawnCommand(HailMaryProjectile)` arrives (adoption) or age exceeds the
/// timeout (server rejection / lost packet).
#[derive(Component, Default)]
pub struct PredictedProjectile {
    pub age_ticks: u32,
}

/// Tracks damage and shooter on a live projectile entity (server-side).
#[derive(Component)]
pub struct HailMaryProjectileState {
    pub damage: f32,
    pub shooter: Option<Entity>,
}

/// Spawns a Hail Mary projectile physics body. Returns the entity.
pub fn spawn_projectile(
    origin: Vec3,
    direction: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    damage: f32,
    shooter: Option<Entity>,
) -> Entity {
    let entity = commands.spawn((
        GameObjectKind::HailMaryProjectile,
        HailMaryProjectileState { damage, shooter },
        Transform::from_translation(origin),
    )).id();
    let rb = RigidBodyBuilder::kinematic_velocity_based()
        .translation(origin)
        .linvel(Vector::new(direction.x * PROJECTILE_SPEED, direction.y * PROJECTILE_SPEED, direction.z * PROJECTILE_SPEED))
        .ccd_enabled(true)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    {
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        collider_set.insert_with_parent(ColliderBuilder::ball(0.05).build(), rb_handle, rigid_body_set);
    }
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    entity
}

/// Spawns a hidden PointLight child on the weapon entity for muzzle flash.
/// Call this client-side after spawning the weapon entity.
pub fn add_muzzle_flash(entity: Entity, commands: &mut Commands) {
    let light = commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            range: 6.0,
            color: Color::srgb(1.0, 0.6, 0.2),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -0.6),
        Visibility::Hidden,
    )).id();
    commands.entity(entity).add_child(light);
}

/// Ticks down muzzle flash and toggles the PointLight child accordingly.
pub fn tick_muzzle_flash(
    mut weapons: Query<(&mut HailMaryComponent, &Children)>,
    mut lights: Query<&mut Visibility, With<PointLight>>,
) {
    for (mut weapon, children) in weapons.iter_mut() {
        for &child in children {
            if let Ok(mut vis) = lights.get_mut(child) {
                if weapon.muzzle_flash_ticks > 0 {
                    weapon.muzzle_flash_ticks -= 1;
                    *vis = Visibility::Inherited;
                } else {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&HailMaryProjectileState, &PhysicsBodyHandle)>,
    mut gizmos: Gizmos,
) {
    for (_, body_handle) in projectiles.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
        let iso = rb_iso(rb);
        for ch in rb.colliders() {
            if let Some(col) = world.collider_set.get(*ch) {
                draw_collider(col, iso, Color::srgba(1.0, 0.3, 0.1, 0.9), &mut gizmos);
            }
        }
    }
}

pub fn apply_hail_mary_fire(
    _world: &mut PhysicsWorld,
    input: WeaponInput,
    dt: f32,
    hail_mary: &mut HailMaryComponent,
) -> Option<FireEffect> {
    if input.fire { hail_mary.fire_requested = true; }
    hail_mary.cooldown = (hail_mary.cooldown - dt).max(0.0);
    if !hail_mary.fire_requested || hail_mary.cooldown > 0.0 { return None; }
    hail_mary.cooldown = COOLDOWN;
    hail_mary.fire_requested = false;
    hail_mary.muzzle_flash_ticks = MUZZLE_FLASH_TICKS;
    Some(FireEffect::Projectile {
        origin: input.origin,
        direction: input.aim_dir,
        speed: PROJECTILE_SPEED,
        damage: DAMAGE,
        shooter: input.shooter,
    })
}
