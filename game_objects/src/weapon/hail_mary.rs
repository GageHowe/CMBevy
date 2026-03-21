use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::{GameObjectKind, GameObject};
use crate::health::Health;
use physics::debug::{draw_collider, rb_iso};
use common::interaction::Interactable;
use physics::physics_world::*;
use physics::convex_hull_asset::ConvexHullAsset;
use super::{Weapon, WeaponComponent, PendingHullCollider, FireCtx};
use crate::sound::SoundEmitter;

// the Hail Mary is a projectile sniper. One shot, one kill.
// we use KinematicVelocityBased as the projectile with CCD.

const MUZZLE_FLASH_TICKS: u8 = 3;
pub const DAMAGE: f32 = 100.0;
pub const COOLDOWN_TICKS: u32 = 120; // fixed ticks between shots
pub const PROJECTILE_SPEED: f32 = 300.0; // projectile speed in m/s
pub const PROJECTILE_LIFETIME: u32 = 300; // in ticks

const HULL_PATH: &str = "collision/hail_mary_placeholder_2.obj";
const SCALE: f32 = 10.0;

pub struct HailMaryPlugin;
impl Plugin for HailMaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, tick_projectile_hits.after(step_physics));
        app.add_systems(FixedUpdate, tick_muzzle_flash);
        #[cfg(feature = "client")]
        app.add_systems(Update, add_projectile_visual);
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
        spawn_projectile(ctx.origin, ctx.aim_dir, commands, world, DAMAGE, ctx.shooter);
        if let Some(sq) = ctx.sound.as_mut() { sq.0.push(crate::sound::SoundRequest { event: "event:/SniperShot", position: None, velocity: Vec3::ZERO }); }
        if let Some(cam) = ctx.camera.as_mut() { cam.add_kick(0.5); }
        if let (Some(q), Some(id)) = (ctx.quic.as_mut(), ctx.net_id) {
            q.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                   &net::message::MsgType::Fire(id.clone(), ctx.origin.into(), ctx.aim_dir.into(), ctx.tick));
        }
    }
}

impl GameObject for HailMaryComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::splat(SCALE) };
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
            let rb_handle = physics.insert_body(entity, rb);
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(ColliderBuilder::cuboid(0.2, 0.05, 0.4).build(), rb_handle, rigid_body_set);
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        world.entity_mut(entity).add_child(light);
        #[cfg(feature = "client")]
        {
            let s = SCALE;
            let (scene, handle) = {
                let server = world.resource::<AssetServer>();
                (server.load("models/hail_mary_placeholder_2.glb#Scene0"),
                 server.load_with_settings(HULL_PATH, move |settings: &mut f32| *settings = s))
            };
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
            // swap cuboid for convex hull immediately if already loaded, else defer
            let hull = world.resource::<Assets<ConvexHullAsset>>().get(&handle).map(|h| h.0.clone());
            if let Some(hull) = hull {
                let existing: Vec<ColliderHandle> = world.resource::<PhysicsWorld>()
                    .rigid_body_set.get(rb_handle).map(|rb| rb.colliders().to_vec()).unwrap_or_default();
                let mut physics = world.resource_mut::<PhysicsWorld>();
                for ch in existing {
                    let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *physics;
                    collider_set.remove(ch, island_manager, rigid_body_set, true);
                }
                let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
                collider_set.insert_with_parent(hull, rb_handle, rigid_body_set);
            } else {
                world.entity_mut(entity).insert(PendingHullCollider(handle));
            }
        }
    }
}

/// Tracks damage and shooter on a live projectile entity.
#[derive(Component, Default, Reflect)]
pub struct HailMaryProjectileState {
    pub damage: f32,
    pub shooter: Option<Entity>,
    /// ticks remaining before auto-despawn
    pub lifetime: u32,
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
        HailMaryProjectileState { damage, shooter, lifetime: PROJECTILE_LIFETIME },
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
        // collision_groups: detect against all groups (needed for narrow_phase contact pairs)
        // solver_groups: exclude players so projectiles don't physically push them
        let proj_collision = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL, InteractionTestMode::And);
        let proj_solver    = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
        collider_set.insert_with_parent(
            ColliderBuilder::ball(0.05)
                .collision_groups(proj_collision)
                .solver_groups(proj_solver)
                .build(),
            rb_handle, rigid_body_set,
        );
    }
    commands.entity(entity).insert(RigidBodyHandleComponent(rb_handle));
    let light = commands.spawn((
        PointLight { intensity: 8000.0, range: 50.0, color: Color::srgb(1.0, 0.0, 0.0), shadows_enabled: true, ..default() },
        Transform::default(),
    )).id();
    commands.entity(entity).add_child(light);
    (entity, vel)
}

/// Detects narrow_phase contacts for live projectiles, applies damage, and despawns on hit or expiry.
pub fn tick_projectile_hits(
    world: Res<PhysicsWorld>,
    mut commands: Commands,
    mut projectiles: Query<(Entity, &mut HailMaryProjectileState, &RigidBodyHandleComponent)>,
    mut health_q: Query<&mut Health>,
) {
    // collect hits first to avoid reborrowing world
    let mut hits: Vec<(Entity, Entity, f32)> = Vec::new();

    for (proj_entity, mut state, rb_handle) in projectiles.iter_mut() {
        state.lifetime = state.lifetime.saturating_sub(1);
        if state.lifetime == 0 { commands.entity(proj_entity).despawn(); continue; }

        let Some(rb) = world.rigid_body_set.get(rb_handle.0) else { continue };
        'outer: for &ch in rb.colliders() {
            for pair in world.narrow_phase.contact_pairs_with(ch) {
                if !pair.has_any_active_contact() { continue; }
                let other_ch = if pair.collider1 == ch { pair.collider2 } else { pair.collider1 };
                let Some(other_rb) = world.collider_set.get(other_ch).and_then(|c| c.parent()) else { continue };
                let Some(&hit_entity) = world.handle_to_entity.get(&other_rb) else { continue };
                if state.shooter == Some(hit_entity) { continue; }
                hits.push((proj_entity, hit_entity, state.damage));
                break 'outer;
            }
        }
    }

    for (proj, target, damage) in hits {
        commands.entity(proj).despawn();
        if let Ok(mut health) = health_q.get_mut(target) {
            health.apply_damage(damage);
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

/// Adds a visible sphere mesh to newly spawned projectile entities (client-only).
#[cfg(feature = "client")]
fn add_projectile_visual(
    q: Query<Entity, Added<HailMaryProjectileState>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.08));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.5, 0.0),
            emissive: LinearRgba::new(4.0, 2.0, 0.0, 1.0),
            unlit: true,
            ..default()
        });
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}

pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&HailMaryProjectileState, &RigidBodyHandleComponent)>,
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
