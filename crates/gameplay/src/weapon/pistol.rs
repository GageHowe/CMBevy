use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
use crate::{health::DamageCause, projectile};

pub const COOLDOWN_TICKS: u32 = 10;
pub const MAGAZINE_SIZE: u16 = 12;
pub const RESERVE_AMMO: u16 = 48;
pub const RELOAD_TICKS: u16 = 50;
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO, // wtf is this
    lifetime: 60,
    radius: None,
    contact_damage: 60.0,
    knockback: 0.2,
    damage_cause: DamageCause::Projectile,
    despawn_on_contact: true,
    explosion: None,
};
#[derive(Component, Default, Reflect)]
pub struct PistolComponent;

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Pistol",
    model_path: "models/placeholder_pistol.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair007.png",
    prediction_projectile_speed: Some(projectile::PISTOL_SPEED),
    zoom_multiplier: 1.0,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS as u16,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 1.0,
    shooter_impulse: PROJECTILE.knockback,
    mass_scaled_shooter_impulse: false,
    decorate_projectile: Some(decorate_projectile),
    projectile_behavior: Some(ProjectileBehavior {
        semi_auto: true,
        spread: 0.0,
        sound: "event:/Weapons/AssaultRifle/RifleShotLocal",
        recoil_scale: 2.0,
        kick_vertical: (1.2, 0.3),
        kick_horizontal: (-0.6, 0.6),
        kick_recovery: 22.0,
        zoomed_kick_scale: 1.0,
        shake: None,
    }),
};

impl crate::archetype::SpawnArchetypeTrait for crate::archetype::Pistol {
    fn spawn(self, entity: Entity, bundle: crate::archetype::SpawnBundle, world: &mut World) {
        let weapon = weapon_bundle(PistolComponent::default(), CONFIG);
        helpers::insert_generic_weapon(
            entity,
            &bundle,
            "pistol",
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
            ColliderBuilder::cuboid(0.12, 0.04, 0.22),
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
            // .add(bevy::math::primitives::Sphere::new(0.04));
            .add(bevy::math::primitives::Capsule3d::new(1.0, 10.0));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(6.0, 4.8, 1.2, 1.0),
                base_color: Color::srgb(1.0, 0.9, 0.4),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
