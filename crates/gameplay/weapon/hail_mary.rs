use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
#[cfg(feature = "client")]
use crate::weapon::weapon_flash;
use crate::{health::DamageCause, projectile};

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO,
    lifetime: 300,
    radius: None,
    contact_damage: 100.0,
    knockback: 1.5,
    damage_cause: DamageCause::Sniper,
    despawn_on_contact: true,
    explosion: None,
};
pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "Hail Mary",
    model_path: "models/hail_mary_placeholder_2.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair010.png",
    prediction_projectile_speed: Some(projectile::HAIL_MARY_SPEED),
    zoom_multiplier: 5.0,
    magazine_size: 3,
    reserve_ammo: 9,
    reload_ticks: 100,
    fire_cooldown_ticks: 60,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 0.0,
    shooter_impulse: 1.5,
    mass_scaled_shooter_impulse: false,
    decorate_projectile: Some(decorate_projectile),
    projectile_behavior: Some(ProjectileBehavior {
        semi_auto: false,
        spread: 0.0,
        sound: "event:/Weapons/SniperShotLocal",
        recoil_scale: 1.0,
        kick_vertical: (5.0, 4.0),
        kick_horizontal: (-1.0, 1.0),
        kick_recovery: 10.0,
        zoomed_kick_scale: 1.0,
        shake: None,
    }),
};

#[derive(Component, Default, Reflect)]
pub struct HailMaryComponent {
    pub muzzle_flash: Option<Entity>,
}

pub fn spawn_hail_mary(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    #[cfg(feature = "client")]
    let muzzle_flash = Some(weapon_flash::spawn_weapon_flash(
        world,
        entity,
        Vec3::new(0.0, 0.0, -2.0),
        0.18,
        Color::srgb(1.0, 0.6, 0.2),
        8.0,
        28.0,
        20_000.0,
        20.0,
        false,
    ));
    #[cfg(not(feature = "client"))]
    let muzzle_flash = None;
    let weapon = weapon_bundle(
        HailMaryComponent {
            muzzle_flash,
            ..default()
        },
        CONFIG,
    );
    helpers::insert_generic_weapon(
        entity,
        cmd,
        "hail_mary",
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
        ColliderBuilder::cuboid(0.2, 0.05, 0.4),
        world,
    );
    crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {
    #[cfg(feature = "client")]
    {
        let mesh = _world
            .resource_mut::<Assets<Mesh>>()
            .add(bevy::math::primitives::Sphere::new(0.06));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(10.0, 8.0, 2.5, 1.0),
                base_color: Color::srgb(1.0, 0.85, 0.55),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
