use super::{FireCtx, Weapon, helpers};
use crate::projectile::hail_mary;
use crate::{GameObject, GameObjectKind};
use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::ColliderBuilder;

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

const MUZZLE_FLASH_TICKS: u8 = 3;
pub const COOLDOWN_TICKS: u32 = 120; // fixed ticks between shots

const HULL_PATH: &str = "collision/placeholder_ar.obj";
#[cfg(feature = "client")]
const SCENE_PATH: &str = "models/hail_mary_placeholder_2.glb#Scene0";

pub struct HailMaryPlugin;
impl Plugin for HailMaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, tick_muzzle_flash);
        #[cfg(feature = "client")]
        app.add_systems(Update, update_impact_indicator);
        #[cfg(feature = "client")]
        app.add_systems(Startup, spawn_impact_indicator);
    }
}

#[derive(Component, Default, Reflect)]
pub struct HailMaryComponent {
    pub cooldown: u32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    /// Ticks remaining for muzzle flash visibility. Set to MUZZLE_FLASH_TICKS on fire.
    pub muzzle_flash_ticks: u8,
    pub muzzle_flash_light: Option<Entity>,
}
impl Weapon for HailMaryComponent {
    const CROSSHAIR_PATH: &'static str = "textures/crosshairs/crosshair010.png";

    fn fixed_update(
        &mut self,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        ctx: &mut FireCtx,
    ) {
        // hold right-click to scope in at 5x
        if let Some(cam) = ctx.camera.as_mut() {
            cam.zoom_multiplier = if ctx.want_alt_fire { 5.0 } else { 1.0 };
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        if ctx.want_fire && self.cooldown == 0 {
            self.fire_requested = true;
        }
        if !self.fire_requested {
            return;
        }
        self.cooldown = COOLDOWN_TICKS;
        self.fire_requested = false;
        self.muzzle_flash_ticks = MUZZLE_FLASH_TICKS;

        let velocity =
            helpers::projectile_velocity(world, ctx.shooter, ctx.aim_dir, hail_mary::SPEED);
        let temp_id = helpers::next_temp_id(ctx.id_counter.as_deref_mut());
        hail_mary::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);
        helpers::queue_fire_sound(
            ctx.sound.as_deref_mut(),
            ctx.camera.is_some(),
            "event:/Weapons/SniperShotLocal",
            "event:/Weapons/SniperShot",
            ctx.origin,
        );
        if let Some(cam) = ctx.camera.as_mut() {
            cam.add_kick((5.0, 4.0), (-1.0, 1.0), 10.0);
        }
        helpers::send_fire_request(
            ctx.quic.as_deref_mut(),
            ctx.net_id,
            net::message::GameObjectKind::HailMaryProjectile,
            temp_id,
            ctx.origin,
            ctx.aim_dir,
        );
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
        helpers::insert_generic_weapon(
            entity,
            cmd,
            world,
            GameObjectKind::HailMary,
            <Self as Weapon>::CROSSHAIR_PATH,
            HailMaryComponent {
                muzzle_flash_light: Some(light),
                ..default()
            },
        );
        helpers::make_generic_weapon_physics(
            entity,
            cmd.position,
            HULL_PATH,
            ColliderBuilder::cuboid(0.2, 0.05, 0.4),
            world,
        );
        world.entity_mut(entity).add_child(light);
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load(SCENE_PATH);
            world
                .entity_mut(entity)
                .insert((SceneRoot(scene), Visibility::default()));
        }
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

/// Marker for the Hail Mary impact indicator UI node.
#[cfg(feature = "client")]
#[derive(Component)]
pub struct ImpactIndicator;

/// Spawns the persistent impact indicator UI node (hidden until Hail Mary is active).
#[cfg(feature = "client")]
fn spawn_impact_indicator(mut commands: Commands, asset_server: Res<AssetServer>) {
    helpers::spawn_screen_indicator(
        &mut commands,
        asset_server.load("textures/ui/impact_indicator.png"),
    );
}

/// Projects the Hail Mary's predicted impact point to screen space and moves the indicator UI node.
/// Raycasts along the actual projectile travel direction (aim + shooter velocity), matching spawn_projectile.
#[cfg(feature = "client")]
fn update_impact_indicator(
    pawn: Query<(Entity, &crate::pawn::biped::WeaponSlots), With<crate::pawn::Possessed>>,
    weapons: Query<&HailMaryComponent>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: Res<PhysicsWorld>,
    mut indicator: Query<(&mut Node, &mut Visibility), With<ImpactIndicator>>,
) {
    let Ok((mut node, mut vis)) = indicator.single_mut() else {
        return;
    };
    let show = (|| -> Option<Vec2> {
        let (pawn_entity, slots) = pawn.single().ok()?;
        let weapon_entity = slots.active().1?;
        weapons.get(weapon_entity).ok()?;
        let (cam, cam_gt) = camera.single().ok()?;
        let origin = cam_gt.translation();
        let aim_dir = *cam_gt.forward();
        let shooter_vel = world
            .entity_to_handle
            .get(&pawn_entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let actual_dir = (aim_dir * hail_mary::SPEED + shooter_vel).normalize_or_zero();
        const MAX_RANGE: f32 = 500.0;
        let hit_dist = world
            .cast_ray(origin, actual_dir, MAX_RANGE, &[pawn_entity])
            .map(|(_, t)| t)
            .unwrap_or(MAX_RANGE);
        cam.world_to_viewport(cam_gt, origin + actual_dir * hit_dist)
            .ok()
    })();
    helpers::set_screen_indicator_position(&mut node, &mut vis, show);
}
