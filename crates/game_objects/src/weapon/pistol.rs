use super::{FireCtx, Weapon, helpers};
use crate::projectile::{helpers as projectile_helpers, rifle};
use crate::{GameObject, GameObjectKind};
use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

pub const COOLDOWN_TICKS: u32 = 10;

pub struct PistolPlugin;
impl Plugin for PistolPlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Default, Reflect)]
pub struct PistolComponent {
    pub cooldown: u32,
    pub trigger_down: bool,
}

impl Weapon for PistolComponent {
    const MODEL_PATH: &'static str = "models/placeholder_ar.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        self.cooldown = self.cooldown.saturating_sub(1);
        if !ctx.want_fire {
            self.trigger_down = false;
            return;
        }
        if self.trigger_down || self.cooldown > 0 {
            return;
        }
        self.trigger_down = true;
        self.cooldown = COOLDOWN_TICKS;

        let velocity =
            projectile_helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, rifle::SPEED);
        let temp_id = projectile_helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        rifle::spawn_pistol(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);
        #[cfg(feature = "client")]
        projectile_helpers::apply_recoil::<rifle::PistolProjectile>(ctx, world, 0.6);
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
            ctx.camera.is_some(),
            "event:/Weapons/RifleShotLocal",
            "event:/Weapons/RifleShot",
            ctx.origin,
        );
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((1.2, 0.3), (-0.6, 0.6), 22.0);
        }
        #[cfg(feature = "client")]
        helpers::send_fire_request(
            ctx.quic.as_deref_mut(),
            ctx.net_id,
            net::message::GameObjectKind::PistolProjectile,
            temp_id,
            ctx.origin,
            ctx.aim_dir,
        );
    }
}

impl GameObject for PistolComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Pistol,
            <Self as Weapon>::MODEL_PATH,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            PistolComponent::default(),
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
