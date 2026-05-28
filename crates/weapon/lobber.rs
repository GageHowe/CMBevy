use bevy::prelude::*;
use rapier3d::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::*;

use super::*;
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::health::DamageCause;
use crate::projectile;

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
    effect: bevy_hanabi_plugin::prelude::spawn_lobber_explosion_effect,
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
const PROJECTILE_GRAVITY_SCALE: f32 = 0.0;
const SHOOTER_IMPULSE: f32 = 3.0;
const MASS_SCALED_SHOOTER_IMPULSE: bool = true;

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
    projectile_gravity_scale: PROJECTILE_GRAVITY_SCALE,
    shooter_impulse: SHOOTER_IMPULSE,
    mass_scaled_shooter_impulse: MASS_SCALED_SHOOTER_IMPULSE,
    decorate_projectile,
};

#[cfg(feature = "client")]
fn update_lobber(
    _weapon: &mut LobberComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
) {
    apply_zoom(ctx);
    if ctx.reload_pressed {
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
    }
    if !ctx.want_fire || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
        return;
    }
    helpers::fire_projectile(ctx, world, commands);
    #[cfg(feature = "client")]
    helpers::apply_local_predicted_impulse(
        ctx,
        world,
        -ctx.aim_dir * 3.0 * helpers::shooter_mass(world, ctx.shooter),
    );
    helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
    if let Some(cam) = ctx.camera.as_mut() {
        cam.add_kick((8.0, 10.0), (-2.0, 2.0), 8.0);
        #[cfg(feature = "client")]
        cam.add_shake(CameraShake {
            translation: Vec3::new(0.01, 0.01, 0.08),
            rotation: Vec2::new(0.02, 0.015),
            roll: 0.01,
            duration: 0.18,
            frequency: 16.0,
        });
    }
}

#[cfg(feature = "client")]
pub fn drive_lobbers(
    mut weapons: Query<(Entity, &mut LobberComponent, &mut WeaponState, &WeaponConfig, &PendingWeaponInput)>,
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
    super::drive_weapon_inputs(&mut weapons, net_ids, world, commands, quic, sound_queue, possessed, camera_fx, id_counter, predicted, update_lobber);
}

pub fn spawn_lobber(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(LobberComponent::default(), CONFIG);
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
