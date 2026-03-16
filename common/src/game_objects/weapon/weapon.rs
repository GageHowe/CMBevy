use bevy::prelude::*;
use rapier3d::prelude::*;

use crate::net::message::SpawnCommand;
use crate::physics::physics_world::*;
use crate::physics::convex_hull_asset::ConvexHullAsset;

/// Attached to a weapon entity when its convex hull is still loading.
/// Removed by `swap_weapon_hull_colliders` once the asset is ready.
#[derive(Component)]
pub struct PendingHullCollider(pub Handle<ConvexHullAsset>);

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// Input state for a weapon, set by whoever controls it before `fire_weapons<T>` runs.
/// On the server this is set by `on_message` when a `Fire` packet arrives.
/// On the client this is set by the fire_weapon input system before the next FixedUpdate.
#[derive(Component, Default, Clone, Copy)]
pub struct WeaponInput {
    pub fire: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    /// Server-only: the entity that pulled the trigger.
    /// Used to exclude the shooter from raycasts. Reset to None after each tick.
    pub shooter: Option<Entity>,
}

/// Produced by `apply_*_fire` when a weapon successfully fires this tick.
///
/// Each variant represents a fundamentally different fire mechanic:
/// - `Hitscan`: instant-travel ray; server raycasts and broadcasts `HitResult`.
/// - `Projectile`: client-authoritative; server relays the Fire event to all clients.
#[derive(Clone, Copy)]
pub enum FireEffect {
    Hitscan {
        origin: Vec3,
        direction: Vec3,
        range: f32,
        damage: f32,
        /// Shooter entity, excluded from the raycast.
        shooter: Option<Entity>,
    },
    Projectile {
        origin: Vec3,
        direction: Vec3,
        speed: f32,
        damage: f32,
        shooter: Option<Entity>,
    },
}

/// Resource that accumulates `FireEffect`s produced by `fire_weapons<T>` each tick.
/// The server's `handle_fired_weapons` drains it to perform raycasts + broadcast HitResult.
/// The client should drain it after reading (e.g. for VFX) or discard it.
#[derive(Resource, Default)]
pub struct FiredWeapons(pub Vec<(Entity, FireEffect)>);

/// Generic fire dispatch — analogous to `move_pawns<T>`.
/// Calls `apply` each tick; returns `Some(FireEffect)` only when the weapon fires.
/// Appends to `FiredWeapons` and resets `WeaponInput` regardless.
pub fn fire_weapons<T: Component<Mutability = bevy::ecs::component::Mutable>>(
    apply: fn(&mut PhysicsWorld, WeaponInput, &mut T) -> Option<FireEffect>,
) -> impl Fn(ResMut<PhysicsWorld>, Query<(Entity, &mut WeaponInput, &mut T)>, ResMut<FiredWeapons>) {
    move |mut world, mut weapons, mut fired| {
        for (entity, mut input, mut component) in weapons.iter_mut() {
            if let Some(effect) = apply(&mut world, *input, &mut component) {
                fired.0.push((entity, effect));
            }
            input.fire = false;
            input.shooter = None;
        }
    }
}

/// Shared interface for weapon types. Implement this to get `spawn_from_command` for free.
pub trait WeaponKind: Component + Default {
    const MODEL_PATH: &'static str;
    /// Optional path to a .obj convex hull asset. Empty string means use the default cuboid.
    const HULL_PATH: &'static str = "";
    /// Uniform scale applied to both the GLB visual and the convex hull collider.
    const SCALE: f32 = 1.0;
}

/// Spawns a weapon from a network SpawnCommand: physics body + scene visuals + net_id.
/// If `W::HULL_PATH` is set, replaces the default cuboid collider with the convex hull.
/// If the asset is already cached the swap is immediate; otherwise a `PendingHullCollider`
/// is attached and `swap_weapon_hull_colliders` finishes the job next frame.
/// Client-only in practice (requires AssetServer for the GLB scene).
pub fn spawn_from_command<W: WeaponKind>(
    cmd: SpawnCommand,
    spawn: fn(Transform, &mut Commands, &mut PhysicsWorld) -> Entity,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
    hull_assets: &Assets<ConvexHullAsset>,
) -> Entity {
    let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::splat(W::SCALE) };
    let entity = spawn(transform, commands, world);
    commands.entity(entity).insert((SceneRoot(asset_server.load(W::MODEL_PATH)), Visibility::default(), cmd.net_id));
    if !W::HULL_PATH.is_empty() {
        let s = W::SCALE;
        let handle = asset_server.load_with_settings(W::HULL_PATH, move |settings: &mut f32| *settings = s);
        if let Some(hull) = hull_assets.get(&handle) {
            // Asset already cached — swap colliders immediately.
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
    }
    entity
}

/// Inserts a dynamic physics body for a weapon entity.
pub fn insert_weapon_physics(
    entity: Entity,
    transform: &Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(2.0)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.2, 0.05, 0.4).build();
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
}
