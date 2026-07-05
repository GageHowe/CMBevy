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
};

#[derive(Component, Default, Reflect)]
pub struct HailMaryComponent {
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    pub muzzle_flash: Option<Entity>,
}

#[cfg(feature = "client")]
fn update_hail_mary(
    weapon: &mut HailMaryComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
) {
    apply_zoom(ctx);
    if ctx.reload_pressed {
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
    }
    if ctx.want_fire {
        weapon.fire_requested = true;
    }
    if !weapon.fire_requested {
        return;
    }
    if !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
        weapon.fire_requested = false;
        return;
    }
    weapon.fire_requested = false;
    #[cfg(feature = "client")]
    if let Some(flash) = weapon.muzzle_flash {
        commands.queue(move |world: &mut World| {
            weapon_flash::trigger_weapon_flash(world, flash);
        });
    }

    helpers::fire_projectile(ctx, world, commands);
    #[cfg(feature = "client")]
    projectile::apply_recoil(1.5, false, ctx, world, 1.0);
    helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
    if let Some(cam) = ctx.camera.as_mut() {
        cam.add_kick((5.0, 4.0), (-1.0, 1.0), 10.0);
    }
}

#[cfg(feature = "client")]
pub fn drive_hail_marys(
    mut weapons: Query<(
        Entity,
        &mut HailMaryComponent,
        &mut WeaponState,
        &WeaponConfig,
        &PendingWeaponInput,
    )>,
    net_ids: Query<&net::message::NetworkID>,
    world: ResMut<PhysicsWorld>,
    commands: Commands,
    quic: Option<ResMut<net::quic::QuicManager>>,
    sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    possessed: Query<Entity, With<crate::pawn::Possessed>>,
    camera_fx: Query<(&mut crate::pawn::CameraEffector, &GlobalTransform), With<Camera3d>>,
    id_counter: Option<ResMut<crate::projectile::ProjectileIdCounter>>,
    predicted: Option<ResMut<common::PredictedCommands>>,
) {
    super::drive_weapon_inputs(
        &mut weapons,
        net_ids,
        world,
        commands,
        quic,
        sound_queue,
        possessed,
        camera_fx,
        id_counter,
        predicted,
        update_hail_mary,
    );
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
