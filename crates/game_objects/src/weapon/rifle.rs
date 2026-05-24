use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, apply_zoom, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::projectile::helpers as projectile_helpers;
use crate::projectile::rifle;

pub const COOLDOWN_TICKS: u32 = 8;
pub const MAGAZINE_SIZE: u16 = 30;
pub const RESERVE_AMMO: u16 = 120;
pub const RELOAD_TICKS: u16 = 70;
const ZOOMED_KICK_SCALE: f32 = 0.3;

pub struct RiflePlugin;
impl Plugin for RiflePlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Default, Reflect)]
pub struct RifleComponent;

impl Weapon for RifleComponent {
    const MODEL_PATH: &'static str = "models/placeholder_ar.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);
    const ZOOM_MULTIPLIER: f32 = 2.5;
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
        let zoom_blend = apply_zoom::<Self>(ctx);
        let kick_scale = 1.0 + (ZOOMED_KICK_SCALE - 1.0) * zoom_blend;
        if ctx.reload_pressed {
            super::start_reload(ctx.weapon_state, &ctx.weapon_config);
        }
        if !ctx.want_fire || !super::consume_round(ctx.weapon_state, &ctx.weapon_config) {
            return;
        }

        fire_rifle_projectile(world, commands, ctx, kick_scale);
    }
}

pub fn fire_rifle_projectile(
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
    kick_scale: f32,
) {
    helpers::fire_projectile(ctx, world, commands, rifle::SPEED, rifle::spawn);
    #[cfg(feature = "client")]
    projectile_helpers::apply_recoil::<rifle::RifleProjectile>(ctx, world, kick_scale);
    helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/RifleShotLocal");
    if let Some(cam) = ctx.camera.as_mut() {
        cam.add_kick(
            (2.0 * kick_scale, 0.5 * kick_scale),
            (-kick_scale, kick_scale),
            20.0,
        );
    }
}

pub fn spawn_rifle(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(RifleComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            <RifleComponent as Weapon>::MODEL_PATH,
            <RifleComponent as Weapon>::CROSSHAIR_PATH,
            <RifleComponent as Weapon>::PREDICTION_PROJECTILE_SPEED,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            <RifleComponent as Weapon>::COLLIDER_PATH,
            ColliderBuilder::cuboid(0.2, 0.05, 0.4),
            world,
        );
        crate::insert_spawn_metadata(entity, world, Some(10.0), true, None, true);
}
