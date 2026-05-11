use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{FireCtx, Weapon, apply_zoom, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::{GameObject, GameObjectKind, projectile::coil_launcher, spawn::AppGameObjectExt};

pub const COOLDOWN_TICKS: u32 = 60;
pub const MAGAZINE_SIZE: u16 = 4;
pub const RESERVE_AMMO: u16 = 16;
pub const RELOAD_TICKS: u16 = 180;

pub struct CoilLauncherPlugin;
impl Plugin for CoilLauncherPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<CoilLauncherComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct CoilLauncherComponent;

impl Weapon for CoilLauncherComponent {
    const MODEL_PATH: &'static str = "models/placeholder_coil_launcher.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair028.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(coil_launcher::SPEED);
    const ZOOM_MULTIPLIER: f32 = 2.0;
    const MAGAZINE_SIZE: u16 = MAGAZINE_SIZE;
    const RESERVE_AMMO: u16 = RESERVE_AMMO;
    const RELOAD_TICKS: u16 = RELOAD_TICKS;
    const FIRE_COOLDOWN_TICKS: u16 = COOLDOWN_TICKS as u16;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::CoilLauncherProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <coil_launcher::CoilLauncherProjectile as crate::projectile::Projectile>::fire_authoritative;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        apply_zoom::<Self>(ctx);
        if ctx.reload_pressed {
            super::start_reload(ctx.weapon_state, &ctx.weapon_config);
        }
        if !ctx.want_fire || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
            return;
        }

        helpers::fire_projectile(
            ctx,
            world,
            commands,
            coil_launcher::SPEED,
            coil_launcher::spawn,
        );

        #[cfg(feature = "client")]
        helpers::apply_local_predicted_impulse(
            ctx,
            world,
            -ctx.aim_dir
                * coil_launcher::shooter_knockback(helpers::shooter_mass(world, ctx.shooter)),
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
}

impl GameObject for CoilLauncherComponent {
    const KIND: GameObjectKind = GameObjectKind::CoilLauncher;
    const GC_AFTER_SECS: Option<f32> = Some(10.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(CoilLauncherComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::CoilLauncher,
            <Self as Weapon>::MODEL_PATH,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            <Self as Weapon>::COLLIDER_PATH,
            ColliderBuilder::cuboid(0.2, 0.05, 0.4),
            world,
        );
    }
}
