use std::collections::HashMap;

use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, step_physics};

use crate::dispatch_game_object_on_death;

const DAMAGE_ATTRIBUTION_WINDOW_SECS: f32 = 6.0;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HealthAuthoritySet;

pub struct HealthPlugin;
impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingDeathDespawns>();
        app.add_systems(
            FixedUpdate,
            (
                apply_collision_damage.after(step_physics),
                regenerate_health,
                age_last_damage_sources,
            )
                .chain()
                .in_set(HealthAuthoritySet),
        );
        app.add_systems(FixedUpdate, handle_deaths.after(step_physics).in_set(HealthAuthoritySet));
        app.add_systems(FixedLast, flush_pending_death_despawns);
    }
}

#[derive(Component, Clone, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component, Clone, Copy)]
pub struct HealthRegen {
    pub per_sec: f32,
}

#[derive(Clone, Copy, Default, Reflect)]
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
        (self.age_secs <= DAMAGE_ATTRIBUTION_WINDOW_SECS).then_some(self.attacker).flatten()
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
pub struct PendingDeathDespawns(pub Vec<Entity>);

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    pub fn apply_damage(&mut self, amount: f32) {
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
pub struct CollisionDamageConfig {
    pub threshold_per_mass: f32,
    pub min_threshold: f32,
    pub damage_scale: f32,
    pub max_damage_per_hit: Option<f32>,
}

impl Default for CollisionDamageConfig {
    fn default() -> Self {
        Self {
            // Default to a mass-scaled threshold so heavier objects don't take the same landing
            // damage as light ones from identical support/contact impulses.
            threshold_per_mass: 40.0,
            min_threshold: 40.0,
            damage_scale: 3.0,
            max_damage_per_hit: None,
        }
    }
}

impl CollisionDamageConfig {
    fn damage_from_impulse(self, impulse: f32, mass: f32) -> f32 {
        let threshold = (mass * self.threshold_per_mass).max(self.min_threshold);
        let mut damage = (impulse - threshold).max(0.0) * self.damage_scale;
        if let Some(max_damage) = self.max_damage_per_hit {
            damage = damage.min(max_damage);
        }
        damage
    }
}

/// Applies damage to any entity with Health + a rigidbody based on collision impulse.
/// Rapier's contact impulse already reflects the force imparted by the collision, so
/// damage only needs one path regardless of what the body hit.
/// Must run after step_physics.
pub fn apply_collision_damage(
    world: Res<PhysicsWorld>,
    has_health_q: Query<Option<&CollisionDamageConfig>, With<Health>>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
) {
    let mut damage_map: HashMap<Entity, f32> = HashMap::new();
    for pair in world.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() {
            continue;
        }
        let impulse: f32 =
            pair.manifolds.iter().flat_map(|m| m.points.iter()).map(|p| p.data.impulse).sum();
        if impulse <= 0.0 {
            continue;
        }
        for rb_h in [pair.collider1, pair.collider2]
            .into_iter()
            .filter_map(|collider| world.collider_set.get(collider).and_then(|c| c.parent()))
        {
            let Some(rb) = world.rigid_body_set.get(rb_h) else {
                continue;
            };
            if !rb.is_dynamic() {
                continue;
            }
            if let Some(&entity) = world.handle_to_entity.get(&rb_h) {
                let Ok(config) = has_health_q.get(entity) else {
                    continue;
                };
                let damage =
                    config.copied().unwrap_or_default().damage_from_impulse(impulse, rb.mass());
                if damage <= 0.0 {
                    continue;
                }
                info!("collision impulse: {impulse:.2}  damage: {damage:.1}");
                *damage_map.entry(entity).or_default() += damage;
            }
        }
    }

    for (entity, damage) in damage_map {
        if let Ok(mut health) = health_q.get_mut(entity) {
            if let Ok(mut last_damage) = last_damage_q.get_mut(entity) {
                last_damage.attacker = None;
                last_damage.cause = DamageCause::Collision;
                last_damage.age_secs = 0.0;
            }
            health.apply_damage(damage);
        }
    }
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

fn regenerate_health(time: Res<Time<Fixed>>, mut health_q: Query<(&mut Health, &HealthRegen)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut health, regen) in &mut health_q {
        if health.is_dead() || health.current >= health.max || regen.per_sec <= 0.0 {
            continue;
        }
        health.current = (health.current + regen.per_sec * dt).min(health.max);
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

        let should_despawn =
            kind.clone().is_none_or(|kind| run_death_callback(kind, entity, world));
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
