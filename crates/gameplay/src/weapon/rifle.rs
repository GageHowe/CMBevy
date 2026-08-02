use bevy::prelude::*;
#[allow(unused_imports)]
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
use crate::{health::DamageCause, projectile};

pub const COOLDOWN_TICKS: u32 = 8;
pub const MAGAZINE_SIZE: u16 = 30;
pub const RESERVE_AMMO: u16 = 120;
pub const RELOAD_TICKS: u16 = 70;
const ZOOMED_KICK_SCALE: f32 = 0.3;
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO,
    lifetime: 60,
    radius: None,
    contact_damage: 40.0,
    knockback: 0.1,
    damage_cause: DamageCause::Projectile,
    despawn_on_contact: true,
    explosion: None,
};

#[derive(Component, Default, Reflect)]
pub struct RifleComponent;

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Rifle",
    model_path: "models/placeholder_ar.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair007.png",
    prediction_projectile_speed: Some(projectile::RIFLE_SPEED),
    zoom_multiplier: 2.5,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS as u16,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 1.0,
    shooter_impulse: 0.1,
    mass_scaled_shooter_impulse: false,
    decorate_projectile: Some(decorate_projectile),
    projectile_behavior: Some(ProjectileBehavior {
        semi_auto: false,
        spread: 0.0,
        sound: "event:/Weapons/AssaultRifle/RifleShotLocal",
        recoil_scale: 1.0,
        kick_vertical: (2.0, 0.5),
        kick_horizontal: (-1.0, 1.0),
        kick_recovery: 20.0,
        zoomed_kick_scale: ZOOMED_KICK_SCALE,
        shake: None,
    }),
};

impl crate::archetype::SpawnArchetypeTrait for crate::archetype::AssaultRifle {
    fn spawn(self, entity: Entity, bundle: crate::archetype::SpawnBundle, world: &mut World) {
        let weapon = weapon_bundle(RifleComponent, CONFIG);
        helpers::insert_generic_weapon(
            entity,
            &bundle,
            "rifle",
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
            .add(bevy::math::primitives::Sphere::new(0.035));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(8.0, 2.0, 0.6, 1.0),
                base_color: Color::srgb(1.0, 0.45, 0.2),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
