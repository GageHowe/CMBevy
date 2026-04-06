use super::{FireCtx, Weapon, helpers};
use crate::projectile::rifle;
use crate::{GameObject, GameObjectKind};
use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

const HULL_PATH: &str = "collision/placeholder_ar.obj";
#[cfg(feature = "client")]
const SCENE_PATH: &str = "models/placeholder_ar.glb#Scene0";

pub const COOLDOWN_TICKS: u32 = 6; // 10 rounds/sec at 60 Hz

pub struct RiflePlugin;
impl Plugin for RiflePlugin {
    fn build(&self, _app: &mut App) {}
}

#[derive(Component, Default, Reflect)]
pub struct RifleComponent {
    pub cooldown: u32,
}

impl Weapon for RifleComponent {
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair007.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(rifle::SPEED);

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        self.cooldown = self.cooldown.saturating_sub(1);
        if !ctx.want_fire || self.cooldown > 0 {
            return;
        }
        self.cooldown = COOLDOWN_TICKS;

        let velocity = helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, rifle::SPEED);
        let temp_id = helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        rifle::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
            ctx.camera.is_some(),
            "event:/Weapons/RifleShotLocal",
            "event:/Weapons/RifleShot",
            ctx.origin,
        );
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((2.0, 0.5), (-1.0, 1.0), 20.0);
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
}

impl GameObject for RifleComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Rifle,
            <Self as Weapon>::CROSSHAIR_PATH,
            <Self as Weapon>::PREDICTION_PROJECTILE_SPEED,
            RifleComponent::default(),
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            HULL_PATH,
            ColliderBuilder::cuboid(0.2, 0.05, 0.4),
            world,
        );
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load(SCENE_PATH);
            world
                .entity_mut(entity)
                .insert((SceneRoot(scene), Visibility::default()));
        }
    }
}
