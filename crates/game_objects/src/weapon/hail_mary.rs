use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, apply_zoom, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::projectile::helpers as projectile_helpers;
#[cfg(feature = "client")]
use crate::weapon::weapon_flash;
use crate::{GameObject, GameObjectKind, projectile::hail_mary, spawn::AppGameObjectExt};

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

pub struct HailMaryPlugin;
impl Plugin for HailMaryPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<HailMaryComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct HailMaryComponent {
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    pub muzzle_flash: Option<Entity>,
}

impl Weapon for HailMaryComponent {
    const MODEL_PATH: &'static str = "models/hail_mary_placeholder_2.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair010.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(hail_mary::SPEED);
    const ZOOM_MULTIPLIER: f32 = 5.0;
    const MAGAZINE_SIZE: u16 = 3;
    const RESERVE_AMMO: u16 = 9;
    const RELOAD_TICKS: u16 = 100;
    const FIRE_COOLDOWN_TICKS: u16 = 60;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::HailMaryProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <hail_mary::HailMaryProjectile as crate::projectile::Projectile>::fire_authoritative;

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
        if ctx.want_fire {
            self.fire_requested = true;
        }
        if !self.fire_requested {
            return;
        }
        if !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
            self.fire_requested = false;
            return;
        }
        self.fire_requested = false;
        #[cfg(feature = "client")]
        if let Some(flash) = self.muzzle_flash {
            commands.queue(move |world: &mut World| {
                weapon_flash::trigger_weapon_flash(world, flash);
            });
        }

        helpers::fire_projectile(ctx, world, commands, hail_mary::SPEED, hail_mary::spawn);
        #[cfg(feature = "client")]
        projectile_helpers::apply_recoil::<hail_mary::HailMaryProjectile>(ctx, world, 1.0);
        helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((5.0, 4.0), (-1.0, 1.0), 10.0);
        }
    }
}

impl GameObject for HailMaryComponent {
    const KIND: GameObjectKind = GameObjectKind::HailMary;
    const GC_AFTER_SECS: Option<f32> = Some(10.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
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
            world,
        );
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::HailMary,
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
