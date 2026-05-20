use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::{GameObject, GameObjectKind, projectile::thumper, spawn::AppGameObjectExt};

pub const COOLDOWN_TICKS: u32 = 18;
pub const MAGAZINE_SIZE: u16 = 6;
pub const RESERVE_AMMO: u16 = 24;
pub const RELOAD_TICKS: u16 = 80;

pub struct ThumperPlugin;

impl Plugin for ThumperPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<ThumperComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct ThumperComponent {
    pub trigger_down: bool,
}

impl Weapon for ThumperComponent {
    const MODEL_PATH: &'static str = "models/thumper_placeholder.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair028.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(thumper::SPEED);
    const MAGAZINE_SIZE: u16 = MAGAZINE_SIZE;
    const RESERVE_AMMO: u16 = RESERVE_AMMO;
    const RELOAD_TICKS: u16 = RELOAD_TICKS;
    const FIRE_COOLDOWN_TICKS: u16 = COOLDOWN_TICKS as u16;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::ThumperProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <thumper::ThumperProjectile as crate::projectile::Projectile>::fire_authoritative;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        if ctx.reload_pressed {
            super::start_reload(ctx.weapon_state, &ctx.weapon_config);
        }
        if !ctx.want_fire {
            self.trigger_down = false;
            return;
        }
        if self.trigger_down || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
            return;
        }
        self.trigger_down = true;

        helpers::fire_projectile(ctx, world, commands, thumper::SPEED, thumper::spawn);

        #[cfg(feature = "client")]
        helpers::apply_local_predicted_impulse(
            ctx,
            world,
            -ctx.aim_dir * thumper::shooter_knockback(helpers::shooter_mass(world, ctx.shooter)),
        );
        helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((2.5, 3.0), (-0.5, 0.5), 10.0);
            #[cfg(feature = "client")]
            cam.add_shake(CameraShake {
                translation: Vec3::new(0.006, 0.006, 0.035),
                rotation: Vec2::new(0.008, 0.006),
                roll: 0.004,
                duration: 0.12,
                frequency: 14.0,
            });
        }
    }
}

impl GameObject for ThumperComponent {
    const KIND: GameObjectKind = GameObjectKind::Thumper;
    const GC_LIFETIME_SECS: Option<f32> = Some(10.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(ThumperComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Thumper,
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
