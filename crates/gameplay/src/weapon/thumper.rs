use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::{health::DamageCause, projectile};

pub const COOLDOWN_TICKS: u32 = 18;
pub const MAGAZINE_SIZE: u16 = 6;
pub const RESERVE_AMMO: u16 = 24;
pub const RELOAD_TICKS: u16 = 80;
const EXPLOSION: projectile::ProjectileExplosion = projectile::ProjectileExplosion {
    radius: 5.0,
    impulse: 6.0,
    impulse_mass_cap: 1000.0,
    self_damage_scale: 0.2,
    percent_max_health_damage: 0.0,
    #[cfg(feature = "client")]
    shake_radius: 10.0,
    #[cfg(feature = "client")]
    shake_scale: 1.0,
    #[cfg(all(feature = "client", feature = "particles"))]
    effect: particles_plugin::prelude::spawn_thumper_explosion_effect,
};
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO,
    lifetime: 180,
    radius: Some(0.16),
    contact_damage: 30.0,
    knockback: 0.0,
    damage_cause: DamageCause::Projectile,
    despawn_on_contact: true,
    explosion: Some(EXPLOSION),
};

#[derive(Component, Default, Reflect)]
pub struct ThumperComponent;

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Thumper",
    model_path: "models/thumper_placeholder.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair028.png",
    prediction_projectile_speed: Some(projectile::THUMPER_SPEED),
    zoom_multiplier: 1.0,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS as u16,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 5.0,
    shooter_impulse: 0.8,
    mass_scaled_shooter_impulse: true,
    decorate_projectile: Some(decorate_projectile),
    projectile_behavior: Some(ProjectileBehavior {
        semi_auto: true,
        spread: 0.0,
        sound: "event:/Weapons/SniperShotLocal",
        recoil_scale: 1.0,
        kick_vertical: (2.5, 3.0),
        kick_horizontal: (-0.5, 0.5),
        kick_recovery: 10.0,
        zoomed_kick_scale: 1.0,
        shake: Some(CameraShake {
            translation: Vec3::new(0.006, 0.006, 0.035),
            rotation: Vec2::new(0.008, 0.006),
            roll: 0.004,
            duration: 0.12,
            frequency: 14.0,
        }),
    }),
};

impl crate::archetype::SpawnArchetypeTrait for crate::archetype::Thumper {
    fn spawn(self, entity: Entity, bundle: crate::archetype::SpawnBundle, world: &mut World) {
        let weapon = weapon_bundle(ThumperComponent::default(), CONFIG);
        helpers::insert_generic_weapon(
            entity,
            &bundle,
            "thumper",
            world,
            CONFIG.display_name,
            CONFIG.model_path,
            CONFIG.crosshair_path,
            CONFIG.prediction_projectile_speed,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            &bundle,
            CONFIG.collider_path,
            ColliderBuilder::cuboid(0.2, 0.05, 0.4),
            world,
        );
        crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
    }
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {
    #[cfg(feature = "client")]
    {
        let mesh = _world
            .resource_mut::<Assets<Mesh>>()
            .add(bevy::math::primitives::Sphere::new(0.12));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(3.0, 7.0, 1.5, 1.0),
                base_color: Color::srgb(0.7, 1.0, 0.45),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
