use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::*;
use crate::{health::DamageCause, projectile};

pub const COOLDOWN_TICKS: u32 = 4;
pub const MAGAZINE_SIZE: u16 = 36;
pub const RESERVE_AMMO: u16 = 144;
pub const RELOAD_TICKS: u16 = 68;
pub const SPREAD_RADIANS: f32 = 0.03;
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
pub fn spread_dir(aim_dir: Vec3) -> Vec3 {
    helpers::apply_spread(aim_dir, SPREAD_RADIANS)
}

#[derive(Component, Default, Reflect)]
pub struct SmgComponent;

pub const CONFIG: WeaponConfig = WeaponConfig {
    display_name: "SMG",
    model_path: "models/placeholder_smg.glb#Scene0",
    collider_path: "collision/placeholder_ar.obj",
    crosshair_path: "textures/crosshairs/crosshair007.png",
    prediction_projectile_speed: Some(projectile::RIFLE_SPEED),
    zoom_multiplier: 1.0,
    magazine_size: MAGAZINE_SIZE,
    reserve_ammo: RESERVE_AMMO,
    reload_ticks: RELOAD_TICKS,
    fire_cooldown_ticks: COOLDOWN_TICKS as u16,
    projectile: Some(PROJECTILE),
    projectile_gravity_scale: 0.0,
    shooter_impulse: 0.1,
    mass_scaled_shooter_impulse: false,
    decorate_projectile: Some(decorate_projectile),
};

#[cfg(feature = "client")]
fn update_smg(
    _weapon: &mut SmgComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
) {
    if ctx.reload_pressed {
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
    }
    if !ctx.want_fire || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
        return;
    }
    let temp_id = crate::projectile::next_temp_id(ctx.id_counter.as_deref_mut());
    let shot_dir = spread_dir(ctx.aim_dir);
    helpers::fire_projectile_with_dir(ctx, world, commands, temp_id, shot_dir);
    #[cfg(feature = "client")]
    projectile::apply_recoil(0.1, false, ctx, world, 0.45);
    helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/RifleShotLocal");
    if let Some(cam) = ctx.camera.as_mut() {
        cam.add_kick((1.1, 0.35), (-0.6, 0.6), 24.0);
    }
}

#[cfg(feature = "client")]
pub fn drive_smgs(
    mut weapons: Query<(
        Entity,
        &mut SmgComponent,
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
        update_smg,
    );
}

pub fn spawn_smg(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
    let weapon = weapon_bundle(SmgComponent, CONFIG);
    helpers::insert_generic_weapon(
        entity,
        cmd,
        "smg",
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
        ColliderBuilder::cuboid(0.18, 0.05, 0.35),
        world,
    );
    crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {
    #[cfg(feature = "client")]
    {
        let mesh = _world
            .resource_mut::<Assets<Mesh>>()
            .add(bevy::math::primitives::Sphere::new(0.03));
        let material = _world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                emissive: LinearRgba::new(7.0, 2.2, 0.8, 1.0),
                base_color: Color::srgb(1.0, 0.5, 0.25),
                unlit: true,
                ..default()
            });
        let visual = _world
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()))
            .id();
        _world.entity_mut(_entity).add_child(visual);
    }
}
