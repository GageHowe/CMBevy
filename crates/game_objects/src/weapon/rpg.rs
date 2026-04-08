use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use crate::projectile::{helpers as projectile_helpers, rpg};
use crate::{GameObject, GameObjectKind};

use super::{FireCtx, Weapon, apply_zoom, helpers, weapon_bundle};

pub const COOLDOWN_TICKS: u32 = 45;

pub struct RpgPlugin;
impl Plugin for RpgPlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Default, Reflect)]
pub struct RpgComponent {
    pub cooldown: u32,
}

impl Weapon for RpgComponent {
    const MODEL_PATH: &'static str = "models/launcher_placeholder_2.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair028.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rpg::SPEED);
    const ZOOM_MULTIPLIER: f32 = 1.5;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        apply_zoom::<Self>(ctx);
        self.cooldown = self.cooldown.saturating_sub(1);
        if !ctx.want_fire || self.cooldown > 0 {
            return;
        }
        self.cooldown = COOLDOWN_TICKS;

        let velocity =
            projectile_helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, rpg::SPEED);
        let temp_id = projectile_helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        rpg::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);

        // Keep the local launcher recoil on the same path the server uses for authoritative fire.
        #[cfg(feature = "client")]
        helpers::apply_local_predicted_impulse(
            ctx,
            world,
            -ctx.aim_dir * rpg::shooter_knockback(helpers::shooter_mass(world, ctx.shooter)),
        );
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
            world,
            ctx.shooter,
            ctx.camera.is_some(),
            "event:/Weapons/SniperShotLocal",
            "event:/Weapons/SniperShot",
            ctx.origin,
        );
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((8.0, 10.0), (-2.0, 2.0), 8.0);
            cam.add_shake(0.6);
        }
        #[cfg(feature = "client")]
        helpers::send_fire_request(
            ctx.quic.as_deref_mut(),
            ctx.net_id,
            net::message::GameObjectKind::RpgProjectile,
            temp_id,
            ctx.origin,
            ctx.aim_dir,
        );
    }
}

impl GameObject for RpgComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(RpgComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Rpg,
            <Self as Weapon>::MODEL_PATH,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            weapon,
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            <Self as Weapon>::COLLIDER_PATH,
            ColliderBuilder::cuboid(0.2, 0.06, 0.55),
            world,
        );
    }
}
