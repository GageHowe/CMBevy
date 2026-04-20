use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;
#[cfg(feature = "client")]
use crate::projectile::helpers as projectile_helpers;

use super::{FireCtx, Weapon, apply_zoom, helpers, weapon_bundle};
use crate::{
    GameObject, GameObjectKind,
    projectile::hail_mary,
};

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

const MUZZLE_FLASH_TICKS: u8 = 3;

pub struct HailMaryPlugin;
impl Plugin for HailMaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, tick_muzzle_flash);
    }
}

#[derive(Component, Default, Reflect)]
pub struct HailMaryComponent {
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    /// Ticks remaining for muzzle flash visibility. Set to MUZZLE_FLASH_TICKS on fire.
    pub muzzle_flash_ticks: u8,
    pub muzzle_flash_light: Option<Entity>,
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
        self.muzzle_flash_ticks = MUZZLE_FLASH_TICKS;

        helpers::fire_projectile(ctx, world, commands, hail_mary::SPEED, hail_mary::spawn);
        #[cfg(feature = "client")]
        projectile_helpers::apply_recoil::<hail_mary::HailMaryProjectile>(ctx, world, 1.0);
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
            cam.add_kick((5.0, 4.0), (-1.0, 1.0), 10.0);
        }
    }
}

impl GameObject for HailMaryComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let light = world
            .spawn((
                PointLight {
                    intensity: 20000.0,
                    range: 15.0,
                    color: Color::srgb(1.0, 0.6, 0.2),
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, -0.6),
                Visibility::Hidden,
            ))
            .id();
        let weapon = weapon_bundle(
            HailMaryComponent { muzzle_flash_light: Some(light), ..default() },
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
        world.entity_mut(entity).add_child(light);
    }
}

/// Ticks down muzzle flash and toggles the PointLight child accordingly.
pub fn tick_muzzle_flash(
    mut weapons: Query<&mut HailMaryComponent>,
    mut lights: Query<&mut Visibility, With<PointLight>>,
) {
    for mut weapon in weapons.iter_mut() {
        let Some(light) = weapon.muzzle_flash_light else {
            continue;
        };
        if let Ok(mut vis) = lights.get_mut(light) {
            if weapon.muzzle_flash_ticks > 0 {
                weapon.muzzle_flash_ticks -= 1;
                *vis = Visibility::Inherited;
            } else {
                *vis = Visibility::Hidden;
            }
        }
    }
}
