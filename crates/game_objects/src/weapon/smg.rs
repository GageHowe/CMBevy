use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::projectile::helpers as projectile_helpers;
use crate::{GameObject, GameObjectKind, projectile::rifle, spawn::AppGameObjectExt};

pub const COOLDOWN_TICKS: u32 = 4;
pub const MAGAZINE_SIZE: u16 = 36;
pub const RESERVE_AMMO: u16 = 144;
pub const RELOAD_TICKS: u16 = 68;
pub const SPREAD_RADIANS: f32 = 0.03;

pub fn spread_dir(aim_dir: Vec3) -> Vec3 {
    helpers::apply_spread(aim_dir, SPREAD_RADIANS)
}

pub struct SmgPlugin;

impl Plugin for SmgPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<SmgComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct SmgComponent;

impl Weapon for SmgComponent {
    const MODEL_PATH: &'static str = "models/placeholder_smg.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);
    const MAGAZINE_SIZE: u16 = MAGAZINE_SIZE;
    const RESERVE_AMMO: u16 = RESERVE_AMMO;
    const RELOAD_TICKS: u16 = RELOAD_TICKS;
    const FIRE_COOLDOWN_TICKS: u16 = COOLDOWN_TICKS as u16;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::RifleProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <rifle::RifleProjectile as crate::projectile::Projectile>::fire_authoritative;

    fn fixed_update(
        &mut self,
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

        let temp_id = crate::projectile::helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        let shot_dir = spread_dir(ctx.aim_dir);
        helpers::fire_projectile_with_dir(
            ctx,
            world,
            commands,
            rifle::SPEED,
            temp_id,
            shot_dir,
            rifle::spawn,
        );
        #[cfg(feature = "client")]
        projectile_helpers::apply_recoil::<rifle::RifleProjectile>(ctx, world, 0.45);
        helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/RifleShotLocal");
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((1.1, 0.35), (-0.6, 0.6), 24.0);
        }
    }
}

impl GameObject for SmgComponent {
    const KIND: GameObjectKind = GameObjectKind::Smg;
    const GC_LIFETIME_SECS: Option<f32> = Some(10.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(SmgComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            <Self as Weapon>::MODEL_PATH,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            <Self as Weapon>::COLLIDER_PATH,
            ColliderBuilder::cuboid(0.18, 0.05, 0.35),
            world,
        );
    }
}
