use bevy::prelude::*;
use rapier3d::prelude::*;

use crate::physics::physics_world::*;

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// Input state for a weapon, set by whoever controls it before fire systems run.
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

/// Per-instance fire state shared by all standard hitscan weapons.
/// Set once at spawn with the weapon's static parameters; `cooldown` and
/// `fire_requested` are updated each tick by `fire_all_weapons`.
#[derive(Component)]
pub struct WeaponState {
    pub range: f32,
    pub damage: f32,
    /// Duration (seconds) between shots.
    pub max_cooldown: f32,
    /// Countdown timer; weapon fires when this reaches zero.
    pub cooldown: f32,
    /// Fire request latched until the weapon is off cooldown, allowing
    /// a tap while on cooldown to queue the next shot.
    pub fire_requested: bool,
}

impl WeaponState {
    pub fn new(range: f32, damage: f32, max_cooldown: f32) -> Self {
        Self { range, damage, max_cooldown, cooldown: 0.0, fire_requested: false }
    }
}

/// Produced by fire systems when a weapon successfully fires this tick.
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

/// Resource that accumulates `FireEffect`s produced by fire systems each tick.
/// The server's `handle_fired_weapons` drains it to perform raycasts + broadcast HitResult.
/// The client should drain it after reading (e.g. for VFX) or discard it.
#[derive(Resource, Default)]
pub struct FiredWeapons(pub Vec<(Entity, FireEffect)>);

/// Fires all standard hitscan weapons. Ticks cooldowns, latches fire requests, and
/// appends a `FireEffect` to `FiredWeapons` whenever a weapon is ready to shoot.
/// Resets `WeaponInput` flags after processing regardless of whether the weapon fired.
///
/// Register this system in both server and client instead of per-type `fire_weapons<T>` calls.
pub fn fire_all_weapons(
    time: Res<Time<Fixed>>,
    mut weapons: Query<(Entity, &mut WeaponInput, &mut WeaponState)>,
    mut fired: ResMut<FiredWeapons>,
) {
    let dt = time.delta_secs();
    for (entity, mut input, mut state) in weapons.iter_mut() {
        if input.fire { state.fire_requested = true; }
        state.cooldown = (state.cooldown - dt).max(0.0);
        if state.fire_requested && state.cooldown == 0.0 {
            state.cooldown = state.max_cooldown;
            state.fire_requested = false;
            fired.0.push((entity, FireEffect::Hitscan {
                origin: input.origin,
                direction: input.aim_dir,
                range: state.range,
                damage: state.damage,
                shooter: input.shooter,
            }));
        }
        input.fire = false;
        input.shooter = None;
    }
}

/// Generic fire dispatch for weapons that need custom per-type behavior beyond
/// `WeaponState` (e.g. future projectile weapons with physics interactions).
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
