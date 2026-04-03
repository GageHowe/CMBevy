use bevy::prelude::*;
use common::{NetworkID, debug_println};
use net::message::MsgType;
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::{PhysicsWorld, step_physics};
use std::collections::HashMap;

pub struct HealthPlugin;
impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, apply_collision_damage.after(step_physics));
        #[cfg(not(feature = "client"))]
        app.add_systems(FixedUpdate, handle_deaths.after(step_physics));
    }
}

#[derive(Component, Clone, Copy)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }

    /// Apply damage; returns `true` if this damage killed the entity.
    pub fn apply_damage(&mut self, amount: f32) -> bool {
        self.current = (self.current - amount).max(0.0);
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
) {
    let mut damage_map: HashMap<Entity, f32> = HashMap::new();
    for pair in world.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() {
            continue;
        }
        let impulse: f32 = pair
            .manifolds
            .iter()
            .flat_map(|m| m.points.iter())
            .map(|p| p.data.impulse)
            .sum();
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
                let damage = config
                    .copied()
                    .unwrap_or_default()
                    .damage_from_impulse(impulse, rb.mass());
                if damage <= 0.0 {
                    continue;
                }
                debug_println!("collision impulse: {impulse:.2}  damage: {damage:.1}");
                *damage_map.entry(entity).or_default() += damage;
            }
        }
    }

    for (entity, damage) in damage_map {
        if let Ok(mut health) = health_q.get_mut(entity) {
            health.apply_damage(damage);
        }
    }
}

/// Despawns any entity whose Health just hit zero and broadcasts DespawnCommand.
/// Server-only. Chain after handle_player_deaths for player-specific cleanup first.
pub fn handle_deaths(
    mut commands: Commands,
    mut quic: ResMut<QuicManager>,
    dead_q: Query<(Entity, &Health, Option<&NetworkID>), Changed<Health>>,
) {
    for (entity, health, net_id) in dead_q.iter() {
        if health.current > 0.0 {
            continue;
        }
        commands.entity(entity).despawn();
        if let Some(net_id) = net_id {
            quic.send(
                SendTarget::All,
                Channel::Ordered,
                &MsgType::DespawnCommand(net_id.clone()),
            );
        }
    }
}
