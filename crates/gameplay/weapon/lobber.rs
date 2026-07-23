use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::*;
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::{health::DamageCause, projectile};

pub const COOLDOWN_TICKS: u32 = 45;
pub const MAGAZINE_SIZE: u16 = 1;
pub const RESERVE_AMMO: u16 = 5;
pub const RELOAD_TICKS: u16 = 95;
const EXPLOSION: projectile::ProjectileExplosion = projectile::ProjectileExplosion {
    radius: 12.0,
    impulse: 30.0,
    impulse_mass_cap: 1000.0,
    self_damage_scale: 0.5,
    percent_max_health_damage: 0.2,
    #[cfg(feature = "client")]
    shake_radius: 10.0,
    #[cfg(feature = "client")]
    shake_scale: 0.5,
    #[cfg(feature = "client")]
    effect: particles_plugin::prelude::spawn_lobber_explosion_effect,
};
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO,
    lifetime: 240,
    radius: Some(0.16),
    contact_damage: 200.0,
    knockback: 0.0,
    damage_cause: DamageCause::Projectile,
    despawn_on_contact: true,
    explosion: Some(EXPLOSION),
};
#[derive(Component, Default, Reflect)]
pub struct LobberComponent;

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Lobber",
    model_path: "models/launcher_placeholder_2.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair028.png",
    prediction_projectile_speed: Some(projectile::LOBBER_SPEED),
    zoom_multiplier: 1.5,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS as u16,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 1.0,
    shooter_impulse: 3.0,
    mass_scaled_shooter_impulse: true,
    decorate_projectile: Some(decorate_projectile),
    projectile_behavior: Some(ProjectileBehavior {
        semi_auto: false,
        spread: 0.0,
        sound: "event:/Weapons/SniperShotLocal",
        recoil_scale: 1.0,
        kick_vertical: (8.0, 10.0),
        kick_horizontal: (-2.0, 2.0),
        kick_recovery: 8.0,
        zoomed_kick_scale: 1.0,
        shake: Some(CameraShake {
            translation: Vec3::new(0.01, 0.01, 0.08),
            rotation: Vec2::new(0.02, 0.015),
            roll: 0.01,
            duration: 0.18,
            frequency: 16.0,
        }),
    }),
};

pub fn spawn_lobber(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    let weapon = weapon_bundle(LobberComponent, CONFIG);
    helpers::insert_generic_weapon(
        entity,
        cmd,
        "lobber",
        world,
        CONFIG.display_name,
        CONFIG.model_path,
        CONFIG.crosshair_path,
        CONFIG.prediction_projectile_speed,
        weapon,
    );
    helpers::make_generic_weapon_physics(
        entity,
        cmd,
        CONFIG.collider_path,
        ColliderBuilder::cuboid(0.2, 0.06, 0.55),
        world,
    );
    crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {
    #[cfg(feature = "client")]
    {
        let mesh = _world
            .resource_mut::<Assets<Mesh>>()
            .add(bevy::math::primitives::Sphere::new(0.16));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(2.0, 6.0, 8.0, 1.0),
                base_color: Color::srgb(0.45, 0.9, 1.0),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
