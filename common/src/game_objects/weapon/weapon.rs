use bevy::prelude::*;
use rapier3d::prelude::*;

use crate::net::message::SpawnCommand;
use crate::physics::physics_world::*;

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
/// - `Projectile` (future): server spawns an authoritative physics body and broadcasts
///   a `SpawnCommand`; client spawns a predicted local body in its processing system.
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
    // Future: Projectile { origin, direction, speed, damage, ... }
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
    apply: fn(&mut PhysicsWorld, WeaponInput, f32, &mut T) -> Option<FireEffect>,
) -> impl Fn(ResMut<PhysicsWorld>, Res<Time<Fixed>>, Query<(Entity, &mut WeaponInput, &mut T)>, ResMut<FiredWeapons>) {
    move |mut world, time, mut weapons, mut fired| {
        let dt = time.delta_secs();
        for (entity, mut input, mut component) in weapons.iter_mut() {
            if let Some(effect) = apply(&mut world, *input, dt, &mut component) {
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
}

/// Spawns a weapon from a network SpawnCommand: physics body + scene visuals + net_id.
/// Client-only in practice (requires AssetServer for the GLB scene).
pub fn spawn_from_command<W: WeaponKind>(
    cmd: SpawnCommand,
    spawn: fn(Transform, &mut Commands, &mut PhysicsWorld) -> Entity,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
) -> Entity {
    let transform = Transform { translation: cmd.position, rotation: cmd.rotation, ..default() };
    let entity = spawn(transform, commands, world);
    commands.entity(entity).insert((SceneRoot(asset_server.load(W::MODEL_PATH)), Visibility::default(), cmd.net_id));
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
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
}
