use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::projectile::helpers as projectile_helpers;
use crate::{GameObject, GameObjectKind, projectile::rifle, spawn::AppGameObjectExt};

pub const COOLDOWN_TICKS: u32 = 10;
pub const MAGAZINE_SIZE: u16 = 12;
pub const RESERVE_AMMO: u16 = 48;
pub const RELOAD_TICKS: u16 = 50;

pub struct PistolPlugin;
impl Plugin for PistolPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<PistolComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct PistolComponent {
    pub trigger_down: bool,
}

impl Weapon for PistolComponent {
    const MODEL_PATH: &'static str = "models/placeholder_ar.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);
    const MAGAZINE_SIZE: u16 = MAGAZINE_SIZE;
    const RESERVE_AMMO: u16 = RESERVE_AMMO;
    const RELOAD_TICKS: u16 = RELOAD_TICKS;
    const FIRE_COOLDOWN_TICKS: u16 = COOLDOWN_TICKS as u16;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::PistolProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <rifle::PistolProjectile as crate::projectile::Projectile>::fire_authoritative;

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

        helpers::fire_projectile(ctx, world, commands, rifle::SPEED, rifle::spawn_pistol);
        #[cfg(feature = "client")]
        projectile_helpers::apply_recoil::<rifle::PistolProjectile>(ctx, world, 0.6);
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
            world,
            ctx.shooter,
            ctx.camera.is_some(),
            "event:/Weapons/RifleShotLocal",
            "event:/Weapons/RifleShot",
            ctx.origin,
        );
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((1.2, 0.3), (-0.6, 0.6), 22.0);
        }
    }
}

impl GameObject for PistolComponent {
    const KIND: GameObjectKind = GameObjectKind::Pistol;

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(PistolComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Pistol,
            <Self as Weapon>::MODEL_PATH,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            <Self as Weapon>::COLLIDER_PATH,
            ColliderBuilder::cuboid(0.12, 0.04, 0.22),
            world,
        );
    }
}
