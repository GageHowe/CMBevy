use bevy::prelude::*;
use rapier3d::prelude::ColliderBuilder;
#[cfg(feature = "client")]
use physics::physics_world::*;

use super::*;
use crate::health::DamageCause;
use crate::projectile;

pub const COOLDOWN_TICKS: u32 = 10;
pub const MAGAZINE_SIZE: u16 = 12;
pub const RESERVE_AMMO: u16 = 48;
pub const RELOAD_TICKS: u16 = 50;
const PROJECTILE: projectile::Projectile = projectile::Projectile {
    shooter: None,
    last_position: Vec3::ZERO,
    inherited_launch_velocity: Vec3::ZERO,
    lifetime: 60,
    radius: None,
    contact_damage: 60.0,
    knockback: 0.1,
    damage_cause: DamageCause::Projectile,
    despawn_on_contact: true,
    explosion: None,
};
const PROJECTILE_GRAVITY_SCALE: f32 = 0.0;
const SHOOTER_IMPULSE: f32 = 0.1;
const MASS_SCALED_SHOOTER_IMPULSE: bool = false;

#[derive(Component, Default, Reflect)]
pub struct PistolComponent {
    pub trigger_down: bool,
}

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
    projectile_gravity_scale: PROJECTILE_GRAVITY_SCALE,
    shooter_impulse: SHOOTER_IMPULSE,
    mass_scaled_shooter_impulse: MASS_SCALED_SHOOTER_IMPULSE,
    decorate_projectile,
};

#[cfg(feature = "client")]
fn update_pistol(
    weapon: &mut PistolComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
) {
    if ctx.reload_pressed {
        super::start_reload(ctx.weapon_state, &ctx.weapon_config);
    }
    if !ctx.want_fire {
        weapon.trigger_down = false;
        return;
    }
    if weapon.trigger_down || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
        return;
    }
    weapon.trigger_down = true;
    helpers::fire_projectile(ctx, world, commands);
    #[cfg(feature = "client")]
    projectile::apply_recoil(SHOOTER_IMPULSE, MASS_SCALED_SHOOTER_IMPULSE, ctx, world, 0.6);
    helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/RifleShotLocal");
    if let Some(cam) = ctx.camera.as_mut() {
        cam.add_kick((1.2, 0.3), (-0.6, 0.6), 22.0);
    }
}

#[cfg(feature = "client")]
pub fn drive_pistols(
    mut weapons: Query<(Entity, &mut PistolComponent, &mut WeaponState, &WeaponConfig, &PendingWeaponInput)>,
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
        update_pistol,
    );
}

pub fn spawn_pistol(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(PistolComponent::default(), CONFIG);
        helpers::insert_generic_weapon(
            entity,
            cmd,
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
            cmd,
            CONFIG.collider_path,
            ColliderBuilder::cuboid(0.12, 0.04, 0.22),
            world,
        );
        crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}

fn decorate_projectile(_entity: Entity, _world: &mut World) {
    #[cfg(feature = "client")]
    {
        let mesh = _world
            .resource_mut::<Assets<Mesh>>()
            .add(bevy::math::primitives::Sphere::new(0.04));
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
