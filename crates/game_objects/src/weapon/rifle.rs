use super::apply_zoom;
use super::{FireCtx, Weapon, helpers, weapon_bundle};
use crate::projectile::{helpers as projectile_helpers, rifle};
use crate::{GameObject, GameObjectKind};
use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

pub const COOLDOWN_TICKS: u32 = 8;
const ZOOMED_KICK_SCALE: f32 = 0.3;

pub struct RiflePlugin;
impl Plugin for RiflePlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Default, Reflect)]
pub struct RifleComponent {
    pub cooldown: u32,
}

impl Weapon for RifleComponent {
    const MODEL_PATH: &'static str = "models/placeholder_ar.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);
    const ZOOM_MULTIPLIER: f32 = 2.5;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        let zoom_blend = apply_zoom::<Self>(ctx);
        let kick_scale = 1.0 + (ZOOMED_KICK_SCALE - 1.0) * zoom_blend;
        self.cooldown = self.cooldown.saturating_sub(1);
        if !ctx.want_fire || self.cooldown > 0 {
            return;
        }
        self.cooldown = COOLDOWN_TICKS;

        fire_rifle_projectile(world, commands, ctx, kick_scale);
    }
}

pub fn fire_rifle_projectile(
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    ctx: &mut FireCtx,
    kick_scale: f32,
) {
    let velocity =
        projectile_helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, rifle::SPEED);
    let temp_id = projectile_helpers::next_temp_id(ctx.id_counter.as_deref_mut());
    rifle::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);
    #[cfg(feature = "client")]
    projectile_helpers::apply_recoil::<rifle::RifleProjectile>(ctx, world, kick_scale);
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
        cam.add_kick(
            (2.0 * kick_scale, 0.5 * kick_scale),
            (-kick_scale, kick_scale),
            20.0,
        );
    }
    #[cfg(feature = "client")]
    helpers::send_fire_request(
        ctx.quic.as_deref_mut(),
        ctx.net_id,
        net::message::GameObjectKind::RifleProjectile,
        temp_id,
        ctx.origin,
        ctx.aim_dir,
    );
}

impl GameObject for RifleComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(RifleComponent::default(), world);
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Rifle,
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
