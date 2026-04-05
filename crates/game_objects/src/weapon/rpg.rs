use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use crate::projectile::rpg;
use crate::{GameObject, GameObjectKind};

use super::{FireCtx, Weapon, helpers};

const HULL_PATH: &str = "collision/placeholder_ar.obj";
#[cfg(feature = "client")]
const SCENE_PATH: &str = "models/Low Poly Firearms Bundle-glb/RPG Launcher.glb#Scene0";

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
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair028.png";

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

        let velocity = helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, rpg::SPEED);
        let temp_id = helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        rpg::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);

        // Keep the local launcher recoil on the same path the server uses for authoritative fire.
        #[cfg(feature = "client")]
        helpers::apply_local_predicted_impulse(
            ctx,
            world,
            -ctx.aim_dir * rpg::weapon_recoil_impulse(helpers::shooter_mass(world, ctx.shooter)),
        );
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
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
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::Rpg,
            <Self as Weapon>::CROSSHAIR_PATH,
            RpgComponent::default(),
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd,
            HULL_PATH,
            ColliderBuilder::cuboid(0.2, 0.06, 0.55),
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
