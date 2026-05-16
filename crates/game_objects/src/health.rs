use bevy::prelude::*;
#[cfg(feature = "client")]
use net::message::NetworkID;
use physics::physics_world::PhysicsWorld;

use crate::{
    AuthoritySystems,
    collision::{CollisionImpactSet, CollisionImpacts},
    dispatch_game_object_on_death,
};

const DAMAGE_ATTRIBUTION_WINDOW_SECS: f32 = 6.0;

/// Registers shared health, regen, damage, and death handling systems.
pub struct HealthPlugin;
impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingDeathDespawns>();
        app.add_systems(
            FixedUpdate,
            (
                apply_collision_damage,
                regenerate_health,
                age_last_damage_sources,
            )
                .chain()
                .after(CollisionImpactSet)
                .in_set(AuthoritySystems),
        );
        app.add_systems(FixedUpdate, handle_deaths.in_set(AuthoritySystems));
        app.add_systems(FixedLast, flush_pending_death_despawns);
    }
}

#[derive(Component, Clone, Copy)]
/// Current and maximum hit points for a damageable entity.
pub struct Health {
    pub current: f32,
    pub max: f32,
    pub regen_per_sec: f32,
    pub regen_delay_secs: f32,
    pub regen_delay_remaining_secs: f32,
}

#[derive(Clone, Copy, Default, Reflect)]
/// High-level reason a damaging event occurred.
pub enum DamageCause {
    #[default]
    Unknown,
    Collision,
    Projectile,
    Sniper,
    Explosion,
}

/// Tracks the most recent gameplay-owned attacker and damage cause for death handling.
#[derive(Component, Clone, Copy, Default, Reflect)]
pub struct LastDamageSource {
    pub attacker: Option<Entity>,
    pub cause: DamageCause,
    pub age_secs: f32,
}

impl LastDamageSource {
    pub fn resolved_attacker(self) -> Option<Entity> {
        (self.age_secs <= DAMAGE_ATTRIBUTION_WINDOW_SECS)
            .then_some(self.attacker)
            .flatten()
    }
}

/// Deferred script kill callbacks drained after authoritative death handling has finished.
#[derive(Resource, Default)]
pub struct PendingPlayerKills(pub Vec<(Entity, Option<Entity>)>);

/// Dead player entities stay in the registry until kill callbacks run so scripts can still
/// mutate their player numbers during `on_player_killed`.
#[derive(Resource, Default)]
pub struct PendingPlayerRemovals(pub Vec<Entity>);

#[derive(Resource, Default)]
/// Deferred despawn queue used to avoid despawning mid-death-processing.
pub struct PendingDeathDespawns(pub Vec<Entity>);

impl Health {
    pub fn new(max: f32, regen_per_sec: f32, regen_delay_secs: f32) -> Self {
        Self {
            current: max,
            max,
            regen_per_sec,
            regen_delay_secs,
            regen_delay_remaining_secs: 0.0,
        }
    }

    pub fn apply_damage(&mut self, amount: f32) {
        if amount <= 0.0 || self.is_dead() {
            return;
        }
        self.regen_delay_remaining_secs = self.regen_delay_secs;
        self.current = (self.current - amount).max(0.0);
    }

    /// Deals damage equal to `fraction` of current max health (e.g. 0.2 = 20%).
    pub fn apply_percent_damage(&mut self, fraction: f32) {
        self.apply_damage(self.max * fraction);
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }
}

#[derive(Component, Clone, Copy)]
/// Tuning for converting collision impulses into gameplay damage.
pub struct CollisionDamageConfig {
    /// Per-mass impulse threshold before damage starts applying.
    pub threshold_per_mass: f32,
    /// Absolute minimum impulse threshold regardless of mass.
    pub min_threshold: f32,
    /// Scale factor from post-threshold impulse to damage.
    pub damage_scale: f32,
}

impl Default for CollisionDamageConfig {
    fn default() -> Self {
        Self {
            // Default to a mass-scaled threshold so heavier objects don't take the same landing
            // damage as light ones from identical support/contact impulses.
            threshold_per_mass: 40.0,
            min_threshold: 40.0,
            damage_scale: 3.0,
        }
    }
}

impl CollisionDamageConfig {
    fn damage_from_impulse(self, impulse: f32, mass: f32) -> f32 {
        let threshold = (mass * self.threshold_per_mass).max(self.min_threshold);
        (impulse - threshold).max(0.0) * self.damage_scale
    }
}

/// Applies collision damage from the generic impact queue.
pub fn apply_collision_damage(
    impacts: Res<CollisionImpacts>,
    has_health_q: Query<Option<&CollisionDamageConfig>, With<Health>>,
    world: Res<PhysicsWorld>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
) {
    for impact in &impacts.0 {
        let Ok(config) = has_health_q.get(impact.entity) else {
            continue;
        };
        let Some(mass) = world
            .entity_to_handle
            .get(&impact.entity)
            .and_then(|&handle| world.rigid_body_set.get(handle))
            .filter(|rb| rb.is_dynamic())
            .map(|rb| rb.mass())
        else {
            continue;
        };
        let damage = config
            .copied()
            .unwrap_or_default()
            .damage_from_impulse(impact.impulse, mass);
        if damage <= 0.0 {
            continue;
        }
        eprintln!(
            "collision impulse: {:.2}  damage: {:.1}",
            impact.impulse, damage
        );
        if let Ok(mut health) = health_q.get_mut(impact.entity) {
            if let Ok(mut last_damage) = last_damage_q.get_mut(impact.entity) {
                last_damage.attacker = None;
                last_damage.cause = DamageCause::Collision;
                last_damage.age_secs = 0.0;
            }
            health.apply_damage(damage);
        }
    }
}

#[cfg(feature = "client")]
pub fn apply_health_update(
    net_id: &NetworkID,
    current: f32,
    networked: &crate::NetworkEntityMap,
    health_q: &mut Query<&mut Health>,
) {
    let Some(entity) = networked.get(net_id) else {
        return;
    };
    let Ok(mut health) = health_q.get_mut(entity) else {
        return;
    };
    health.current = current;
}

fn age_last_damage_sources(time: Res<Time<Fixed>>, mut q: Query<&mut LastDamageSource>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut last_damage in &mut q {
        if last_damage.attacker.is_none() && matches!(last_damage.cause, DamageCause::Unknown) {
            continue;
        }
        last_damage.age_secs += dt;
        if last_damage.age_secs > DAMAGE_ATTRIBUTION_WINDOW_SECS {
            last_damage.attacker = None;
            last_damage.cause = DamageCause::Unknown;
        }
    }
}

fn regenerate_health(time: Res<Time<Fixed>>, mut health_q: Query<&mut Health>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut health in &mut health_q {
        health.regen_delay_remaining_secs = (health.regen_delay_remaining_secs - dt).max(0.0);
        if health.is_dead() || health.current >= health.max || health.regen_per_sec <= 0.0 {
            continue;
        }
        if health.regen_delay_remaining_secs > 0.0 {
            continue;
        }
        health.current = (health.current + health.regen_per_sec * dt).min(health.max);
    }
}

/// calls GameObject::on_death for objects that have been killed, and despawns if it returns true
pub fn handle_deaths(world: &mut World) {
    let dead: Vec<(Entity, Option<common::GameObjectKind>)> = {
        let mut q = world
            .query_filtered::<(Entity, &Health, Option<&common::GameObjectKind>), Changed<Health>>(
            );
        q.iter(world)
            .filter(|(_, health, _)| health.is_dead())
            .map(|(entity, _, kind)| (entity, kind.cloned()))
            .collect()
    };

    for (entity, kind) in dead {
        if !world.entities().contains(entity) {
            continue;
        }

        let should_despawn = kind
            .clone()
            .is_none_or(|kind| run_death_callback(kind, entity, world));
        if !should_despawn || !world.entities().contains(entity) {
            continue;
        }
        if let Some(mut pending) = world.get_resource_mut::<PendingDeathDespawns>() {
            pending.0.push(entity);
        } else {
            world.entity_mut(entity).despawn();
        }
    }
}

fn flush_pending_death_despawns(world: &mut World) {
    let dead = world
        .get_resource_mut::<PendingDeathDespawns>()
        .map(|mut pending| std::mem::take(&mut pending.0))
        .unwrap_or_default();
    for entity in dead {
        if world.entities().contains(entity) {
            world.entity_mut(entity).despawn();
        }
    }
}

fn run_death_callback(kind: common::GameObjectKind, entity: Entity, world: &mut World) -> bool {
    dispatch_game_object_on_death(kind, entity, world)
}

pub fn attribute_damage(
    last_damage_q: &mut Query<&mut LastDamageSource>,
    victim: Entity,
    attacker: Option<Entity>,
    cause: DamageCause,
) {
    let Ok(mut last_damage) = last_damage_q.get_mut(victim) else {
        return;
    };
    last_damage.attacker = attacker;
    last_damage.cause = cause;
    last_damage.age_secs = 0.0;
}

pub fn copy_last_damage_source(world: &mut World, from: Entity, to: Entity) {
    let Some(source) = world.get::<LastDamageSource>(from).copied() else {
        return;
    };
    let Some(mut target) = world.get_mut::<LastDamageSource>(to) else {
        return;
    };
    *target = source;
}
