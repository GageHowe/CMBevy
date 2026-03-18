use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::{GameObjectKind, GameObject};
use physics::debug::{draw_collider, rb_iso};
use common::interaction::Interactable;
use net::message::SpawnCommand;
use physics::physics_world::*;
use physics::convex_hull_asset::ConvexHullAsset;
use super::weapon::{Weapon, WeaponComponent, PendingHullCollider};
use crate::sound::SoundEmitter;

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

pub const DAMAGE: f32 = 100.0;
/// fixed between shots
pub const COOLDOWN_TICKS: u32 = 120;
/// Projectile speed in m/s.
pub const PROJECTILE_SPEED: f32 = 100.0;

const HULL_PATH: &str = "collision/hail_mary_placeholder_2.obj";
const SCALE: f32 = 10.0;

#[derive(Component, Default)]
pub struct HailMaryComponent {
    pub cooldown: u32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
    /// Ticks remaining for muzzle flash visibility. Set to MUZZLE_FLASH_TICKS on fire.
    pub muzzle_flash_ticks: u8,
    pub muzzle_flash_light: Option<Entity>,
}

const MUZZLE_FLASH_TICKS: u8 = 3;

impl Weapon for HailMaryComponent {
    fn fire_sound(&self) -> Option<&'static str> { Some("event:/SniperShot") }
    fn update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, origin: Vec3, aim_dir: Vec3, shooter: Option<Entity>, _tick: u64, want_fire: bool) -> bool {
        self.cooldown = self.cooldown.saturating_sub(1);
        if want_fire && self.cooldown == 0 { self.fire_requested = true; }
        if !self.fire_requested { return false; }
        self.cooldown = COOLDOWN_TICKS;
        self.fire_requested = false;
        self.muzzle_flash_ticks = MUZZLE_FLASH_TICKS;
        spawn_projectile(origin, aim_dir, commands, world, DAMAGE, shooter);
        true
    }
}

impl GameObject for HailMaryComponent {
    fn spawn_physics(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
        let light = commands.spawn((
            PointLight {
                intensity: 2_000_000.0,
                range: 15.0,
                color: Color::srgb(1.0, 0.6, 0.2),
                shadows_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -0.6),
            Visibility::Hidden,
        )).id();
        let entity = commands.spawn((
            WeaponComponent,
            HailMaryComponent { muzzle_flash_light: Some(light), ..default() },
            GameObjectKind::HailMary,
            Interactable { range: 2.0 },
            Transform::from(transform),
        )).id();
        let rb = RigidBodyBuilder::dynamic()
            .translation(transform.translation)
            .angular_damping(2.0)
            .build();
        let rb_handle = world.insert_body(entity, rb);
        commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
        {
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
            collider_set.insert_with_parent(ColliderBuilder::cuboid(0.2, 0.05, 0.4).build(), rb_handle, rigid_body_set);
        }
        commands.entity(entity).add_child(light);
        entity
    }
    fn cleanup() {}
    fn get_rigidbody() -> Option<RigidBody> {
        Some(RigidBodyBuilder::dynamic().angular_damping(2.0).build())
    }
}

impl GameObject for HailMaryProjectileState {
    fn spawn_physics(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
        spawn_projectile(transform.translation, Vec3::NEG_Z, commands, world, DAMAGE, None).0
    }
    fn cleanup() {}
    fn get_rigidbody() -> Option<RigidBody> {
        Some(RigidBodyBuilder::kinematic_velocity_based().ccd_enabled(true).build())
    }
}

pub fn spawn_from_command(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
    hull_assets: &Assets<ConvexHullAsset>,
) -> Entity {
    let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::splat(SCALE) };
    let entity = HailMaryComponent::spawn_physics(transform, commands, world);
    commands.entity(entity).insert((SceneRoot(asset_server.load("models/hail_mary_placeholder_2.glb#Scene0")), Visibility::default(), cmd.net_id));
    let s = SCALE;
    let handle = asset_server.load_with_settings(HULL_PATH, move |settings: &mut f32| *settings = s);
    if let Some(hull) = hull_assets.get(&handle) {
        if let Some(rb_handle) = world.entity_to_handle.get(&entity).copied() {
            let existing: Vec<ColliderHandle> = world.rigid_body_set.get(rb_handle)
                .map(|rb| rb.colliders().to_vec())
                .unwrap_or_default();
            for ch in existing {
                let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
                collider_set.remove(ch, island_manager, rigid_body_set, true);
            }
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
            collider_set.insert_with_parent(hull.0.clone(), rb_handle, rigid_body_set);
        }
    } else {
        commands.entity(entity).insert(PendingHullCollider(handle));
    }
    entity
}

/// Tracks damage and shooter on a live projectile entity (server-side).
#[derive(Component)]
pub struct HailMaryProjectileState {
    pub damage: f32,
    pub shooter: Option<Entity>,
}

/// Spawns a Hail Mary projectile physics body. Returns `(entity, actual_velocity)`.
/// The projectile inherits the shooter's velocity if the shooter entity is found in the world.
pub fn spawn_projectile(
    origin: Vec3,
    direction: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    damage: f32,
    shooter: Option<Entity>,
) -> (Entity, Vec3) {
    // When shooter is None (ghost), direction is treated as a pre-computed velocity.
    let vel = match shooter {
        Some(e) => {
            let sv = world.entity_to_handle.get(&e)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
                .unwrap_or(Vec3::ZERO);
            direction * PROJECTILE_SPEED + sv
        }
        None => direction,
    };
    let entity = commands.spawn((
        GameObjectKind::HailMaryProjectile,
        HailMaryProjectileState { damage, shooter },
        Transform::from_translation(origin),
        SoundEmitter { event: "event:/SniperShot" },
        // GravityScale(0.5),
    )).id();
    let rb = RigidBodyBuilder::kinematic_velocity_based()


        .translation(origin)
        .linvel(Vector::new(vel.x, vel.y, vel.z))
        .ccd_enabled(true)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    {
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        let projectile_groups = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
        collider_set.insert_with_parent(
            ColliderBuilder::ball(0.05)
                .collision_groups(projectile_groups)
                .solver_groups(projectile_groups)
                .build(),
            rb_handle, rigid_body_set,
        );
    }
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    (entity, vel)
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

pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&HailMaryProjectileState, &RigidBodyHandleComponenet)>,
    mut gizmos: Gizmos,
) {
    for (_, body_handle) in projectiles.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
        let iso = rb_iso(rb);
        for ch in rb.colliders() {
            if let Some(col) = world.collider_set.get(*ch) {
                draw_collider(col, iso, Color::srgba(1.0, 0.3, 0.1, 0.9), &mut gizmos);
            }
        }
    }
}
