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
/// fixed between shots
pub const COOLDOWN_TICKS: u32 = 120;
/// Projectile speed in m/s.
pub const PROJECTILE_SPEED: f32 = 100.0;

// didnt know impls could have consts lol
impl WeaponKind for HailMaryComponent {
    const MODEL_PATH: &'static str = "models/hail_mary_placeholder_2.glb#Scene0";
    const HULL_PATH: &'static str = "collision/hail_mary_placeholder_2.obj";
    const SCALE: f32 = 10.0;
}

#[derive(Component, Default)]
pub struct HailMaryComponent {
    pub cooldown: u32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    /// Ticks remaining for muzzle flash visibility. Set to MUZZLE_FLASH_TICKS on fire.
    pub muzzle_flash_ticks: u8,
    pub muzzle_flash_light: Option<Entity>,
}

const MUZZLE_FLASH_TICKS: u8 = 3;

/// Spawns a Hail Mary weapon entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let light = commands.spawn((
        PointLight {
            intensity: 2_000_000.0,
            range: 15.0,
            color: Color::srgb(1.0, 0.6, 0.2),
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -0.6),
        Visibility::Hidden,
    )).id();
    let entity = commands.spawn((
        WeaponComponent,
        HailMaryComponent { muzzle_flash_light: Some(light), ..default() },
        WeaponInput::default(),
        GameObjectKind::HailMary,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    commands.entity(entity).add_child(light);
    entity
}

/// Tracks damage and shooter on a live projectile entity (server-side).
#[derive(Component)]
pub struct HailMaryProjectileState {
    pub damage: f32,
    pub shooter: Option<Entity>,
}

/// Spawns a Hail Mary projectile physics body. Returns `(entity, actual_velocity)`.
/// The projectile inherits the shooter's velocity if the shooter entity is found in the world.
pub fn spawn_projectile(
    origin: Vec3,
    direction: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    damage: f32,
    shooter: Option<Entity>,
) -> (Entity, Vec3) {
    let shooter_vel = shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
        .unwrap_or(Vec3::ZERO);
    let vel = direction * PROJECTILE_SPEED + shooter_vel;
    let entity = commands.spawn((
        GameObjectKind::HailMaryProjectile,
        HailMaryProjectileState { damage, shooter },
        Transform::from_translation(origin),
        GravityScale(0.0),
    )).id();
    let rb = RigidBodyBuilder::kinematic_velocity_based()


        .translation(origin)
        .linvel(Vector::new(vel.x, vel.y, vel.z))
        .ccd_enabled(true)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    {
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        let projectile_groups = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
        collider_set.insert_with_parent(
            ColliderBuilder::ball(0.05)
                .collision_groups(projectile_groups)
                .solver_groups(projectile_groups)
                .build(),
            rb_handle, rigid_body_set,
        );
    }
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    (entity, vel)
}

/// Ticks down muzzle flash and toggles the PointLight child accordingly.
pub fn tick_muzzle_flash(
    mut weapons: Query<&mut HailMaryComponent>,
    mut lights: Query<&mut Visibility, With<PointLight>>,
) {
    for mut weapon in weapons.iter_mut() {
        let Some(light) = weapon.muzzle_flash_light else { continue };
        if let Ok(mut vis) = lights.get_mut(light) {
            if weapon.muzzle_flash_ticks > 0 {
                weapon.muzzle_flash_ticks -= 1;
                *vis = Visibility::Inherited;
            } else {
                *vis = Visibility::Hidden;
            }
        }
    }
}

pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&HailMaryProjectileState, &RigidBodyHandleComponenet)>,
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
    hail_mary: &mut HailMaryComponent,
) -> Option<FireEffect> {
    hail_mary.cooldown = hail_mary.cooldown.saturating_sub(1);
    // Only latch fire_requested when the weapon is ready; discard clicks during cooldown.
    if input.fire && hail_mary.cooldown == 0 { hail_mary.fire_requested = true; }
    if !hail_mary.fire_requested { return None; }
    hail_mary.cooldown = COOLDOWN_TICKS;
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
