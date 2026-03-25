use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::{GameObjectKind, GameObject};
use common::interaction::Interactable;
use physics::physics_world::*;
use crate::generic::hull_or;
use super::{Weapon, WeaponComponent, FireCtx};
use crate::projectile::hail_mary;

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

const MUZZLE_FLASH_TICKS: u8 = 3;
pub const COOLDOWN_TICKS: u32 = 120; // fixed ticks between shots

const HULL_PATH: &str = "collision/placeholder_ar.obj";


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
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, ctx: &mut FireCtx) {
        // hold right-click to scope in at 5x
        if let Some(cam) = ctx.camera.as_mut() {
            cam.zoom_multiplier = if ctx.want_alt_fire { 5.0 } else { 1.0 };
        }
        self.cooldown = self.cooldown.saturating_sub(1);
        if ctx.want_fire && self.cooldown == 0 { self.fire_requested = true; }
        if !self.fire_requested { return; }
        self.cooldown = COOLDOWN_TICKS;
        self.fire_requested = false;
        self.muzzle_flash_ticks = MUZZLE_FLASH_TICKS;

        let sv = ctx.shooter
            .and_then(|e| world.entity_to_handle.get(&e).copied())
            .and_then(|h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let velocity = ctx.aim_dir * hail_mary::SPEED + sv;
        let temp_id = ctx.id_counter.as_mut().map(|c| { **c = c.wrapping_add(1); **c }).unwrap_or(0);
        hail_mary::spawn(ctx.origin, velocity, commands, world, ctx.shooter, temp_id);

        if let Some(sq) = ctx.sound.as_mut() {
            // local player: 2D event (no spatialization); remote: 3D at their position
            if ctx.camera.is_some() {
                sq.0.push(crate::sound::SoundRequest { event: "event:/Weapons/SniperShotLocal", position: None, velocity: Vec3::ZERO });
            } else {
                sq.0.push(crate::sound::SoundRequest { event: "event:/Weapons/SniperShot", position: Some(ctx.origin), velocity: Vec3::ZERO });
            }
        }
        if let Some(cam) = ctx.camera.as_mut() { cam.add_kick((5.0, 4.0), (-1.0, 1.0), 10.0); }
        if let (Some(q), Some(id)) = (ctx.quic.as_mut(), ctx.net_id) {
            q.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                &net::message::MsgType::FireRequest { weapon: id.clone(), kind: net::message::GameObjectKind::HailMaryProjectile, temp_id, origin: ctx.origin, dir: ctx.aim_dir });
        }
    }
}

impl GameObject for HailMaryComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position, rotation: cmd.rotation, ..default() };
        let light = world.spawn((
            PointLight { intensity: 20000.0, range: 15.0, color: Color::srgb(1.0, 0.6, 0.2), shadows_enabled: false, ..default() },
            Transform::from_xyz(0.0, 0.0, -0.6),
            Visibility::Hidden,
        )).id();
        world.entity_mut(entity).insert((
            WeaponComponent,
            HailMaryComponent { muzzle_flash_light: Some(light), ..default() },
            GameObjectKind::HailMary,
            Interactable { range: 2.0 },
            Transform::from(transform),
            cmd.net_id.clone(),
        ));
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb = RigidBodyBuilder::dynamic().translation(transform.translation).angular_damping(2.0).build();
            physics.insert_body(entity, rb)
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        let col = hull_or(HULL_PATH, ColliderBuilder::cuboid(0.2, 0.05, 0.4), world);
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
        collider_set.insert_with_parent(col, rb_handle, rigid_body_set);
        world.entity_mut(entity).add_child(light);
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load("models/hail_mary_placeholder_2.glb#Scene0");
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
        }
    }
}

/// Ticks down muzzle flash and toggles the PointLight child accordingly.
pub fn tick_muzzle_flash(
    mut weapons: Query<&mut HailMaryComponent>,
    mut lights: Query<&mut Visibility, With<PointLight>>,
) {
    for mut weapon in weapons.iter_mut() {
        let Some(light) = weapon.muzzle_flash_light else { continue };
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
    commands.spawn((
        ImpactIndicator,
        ImageNode::new(asset_server.load("textures/ui/impact_indicator.png")),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(24.0),
            height: Val::Px(24.0),
            ..default()
        },
        ZIndex(10),
        Visibility::Hidden,
    ));
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
    let Ok((mut node, mut vis)) = indicator.single_mut() else { return };
    let show = (|| -> Option<Vec2> {
        let (pawn_entity, slots) = pawn.single().ok()?;
        let weapon_entity = slots.active().1?;
        weapons.get(weapon_entity).ok()?;
        let (cam, cam_gt) = camera.single().ok()?;
        let origin = cam_gt.translation();
        let aim_dir = *cam_gt.forward();
        let shooter_vel = world.entity_to_handle.get(&pawn_entity)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let actual_dir = (aim_dir * hail_mary::SPEED + shooter_vel).normalize_or_zero();
        const MAX_RANGE: f32 = 500.0;
        let hit_dist = world.cast_ray(origin, actual_dir, MAX_RANGE, &[pawn_entity])
            .map(|(_, t)| t)
            .unwrap_or(MAX_RANGE);
        cam.world_to_viewport(cam_gt, origin + actual_dir * hit_dist).ok()
    })();
    match show {
        Some(pos) => {
            // offset by half the node size to center it on the impact point
            node.left = Val::Px(pos.x - 12.0);
            node.top  = Val::Px(pos.y - 12.0);
            *vis = Visibility::Inherited;
        }
        None => *vis = Visibility::Hidden,
    }
}

