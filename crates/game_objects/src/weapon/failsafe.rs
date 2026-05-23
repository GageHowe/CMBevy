use bevy::prelude::*;
use net::{message::NetworkID, quic::ConnectionId};
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

use super::{FireCtx, Weapon, helpers, weapon_bundle};
#[cfg(feature = "client")]
use crate::pawn::CameraShake;
use crate::{
    GameObject, GameObjectKind, NetworkEntityMap,
    pawn::{PlayerRegistry, WeaponSlots},
    projectile::failsafe,
    spawn::AppGameObjectExt,
};

pub const COOLDOWN_TICKS: u32 = 45;
pub const MAGAZINE_SIZE: u16 = 1;
pub const RESERVE_AMMO: u16 = 5;
pub const RELOAD_TICKS: u16 = 95;

pub struct FailsafePlugin;
impl Plugin for FailsafePlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<FailsafeComponent>();
    }
}

#[derive(Component, Default, Reflect)]
pub struct FailsafeComponent {
    pub trigger_down: bool,
}

impl Weapon for FailsafeComponent {
    const MODEL_PATH: &'static str = "models/launcher_placeholder_2.glb#Scene0";
    const COLLIDER_PATH: &'static str = "collision/placeholder_ar.obj";
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair028.png";
    const PREDICTION_PROJECTILE_SPEED: Option<f32> = Some(failsafe::SPEED);
    const MAGAZINE_SIZE: u16 = MAGAZINE_SIZE;
    const RESERVE_AMMO: u16 = RESERVE_AMMO;
    const RELOAD_TICKS: u16 = RELOAD_TICKS;
    const FIRE_COOLDOWN_TICKS: u16 = COOLDOWN_TICKS as u16;
    const PROJECTILE_KIND: net::message::GameObjectKind =
        net::message::GameObjectKind::FailsafeProjectile;
    const FIRE_PROJECTILE: super::FireProjectileFn =
        <failsafe::FailsafeProjectile as crate::projectile::Projectile>::fire_authoritative;

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        if self.trigger_down && !ctx.want_fire {
            request_detonate_local(commands, ctx.weapon);
            #[cfg(feature = "client")]
            if let (Some(quic), Some(weapon_net_id)) = (ctx.quic.as_deref_mut(), ctx.net_id) {
                quic.send_to_server(
                    net::quic::Channel::Ordered,
                    &net::message::MsgType::DetonateFailsafeRequest(weapon_net_id.clone()),
                );
            }
        }
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

        let velocity = crate::projectile::helpers::projectile_velocity(
            world,
            ctx.shooter,
            ctx.aim_dir,
            failsafe::SPEED,
        );
        let shooter_velocity = crate::projectile::helpers::shooter_velocity(world, ctx.shooter);
        let temp_id = crate::projectile::helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        failsafe::spawn(
            ctx.origin,
            velocity,
            shooter_velocity,
            commands,
            world,
            ctx.shooter,
            Some(ctx.weapon),
            temp_id,
        );
        #[cfg(feature = "client")]
        helpers::send_fire_request(
            ctx.quic.as_deref_mut(),
            ctx.net_id,
            ctx.weapon_config.projectile_kind.clone(),
            temp_id,
            ctx.origin,
            ctx.aim_dir,
        );

        #[cfg(feature = "client")]
        helpers::apply_local_predicted_impulse(
            ctx,
            world,
            -ctx.aim_dir * failsafe::shooter_knockback(helpers::shooter_mass(world, ctx.shooter)),
        );
        helpers::queue_fire_sound(ctx.sound.as_deref_mut(), "event:/Weapons/SniperShotLocal");
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((8.0, 10.0), (-2.0, 2.0), 8.0);
            #[cfg(feature = "client")]
            cam.add_shake(CameraShake {
                translation: Vec3::new(0.01, 0.01, 0.08),
                rotation: Vec2::new(0.02, 0.015),
                roll: 0.01,
                duration: 0.18,
                frequency: 16.0,
            });
        }
    }
}

impl GameObject for FailsafeComponent {
    const KIND: GameObjectKind = GameObjectKind::Failsafe;
    const GC_LIFETIME_SECS: Option<f32> = Some(10.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let weapon = weapon_bundle(FailsafeComponent::default(), world);
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
            ColliderBuilder::cuboid(0.2, 0.06, 0.55),
            world,
        );
    }
}

pub fn request_detonate_local(commands: &mut Commands, weapon_entity: Entity) {
    commands.queue(move |world: &mut World| {
        failsafe::detonate_latest_for_weapon(world, weapon_entity);
    });
}

pub fn handle_detonate_failsafe_request(
    conn_id: ConnectionId,
    weapon_net_id: NetworkID,
    registry: &PlayerRegistry,
    all_networked: &NetworkEntityMap,
    pawn_slots: &Query<&mut WeaponSlots>,
    commands: &mut Commands,
) {
    let Some((shooter_entity, _)) = registry.character(conn_id) else {
        return;
    };
    let shooter_holds = pawn_slots
        .get(shooter_entity)
        .map(|s| s.contains_net_id(&weapon_net_id))
        .unwrap_or(false);
    if !shooter_holds {
        return;
    }
    let Some(weapon_entity) = all_networked.get(&weapon_net_id) else {
        return;
    };
    request_detonate_local(commands, weapon_entity);
}
