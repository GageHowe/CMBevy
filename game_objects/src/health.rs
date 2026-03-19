use bevy::prelude::*;
use std::collections::HashMap;
use common::{debug_println, NetworkID};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use net::quic::{QuicManager, SendTarget, Channel};
use net::message::MsgType;

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

/// Applies damage to any entity with Health + a rigidbody based on collision impulse.
/// p.data.impulse is Rapier's normal constraint impulse — already accounts for mass,
/// inertia, angular velocity, and contact geometry. Tune THRESHOLD above resting contact
/// levels to avoid false positives from gravity reaction forces.
/// Must run after step_physics.
pub fn apply_collision_damage(
    world: Res<PhysicsWorld>,
    mut health_q: Query<(&mut Health, &RigidBodyHandleComponent)>,
) {
    const THRESHOLD: f32 = 40.0;
    const SCALE: f32 = 3.0;

    let mut damage_map: HashMap<Entity, f32> = HashMap::new();
    for pair in world.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() { continue; }
        let impulse: f32 = pair.manifolds.iter()
            .flat_map(|m| m.points.iter())
            .map(|p| p.data.impulse)
            .sum();
        if impulse < THRESHOLD { continue; }
        let damage = (impulse - THRESHOLD) * SCALE;
        debug_println!("impulse: {impulse:.2}  damage: {damage:.1}");
        for &ch in &[pair.collider1, pair.collider2] {
            if let Some(rb_h) = world.collider_set.get(ch).and_then(|c| c.parent()) {
                if let Some(&entity) = world.handle_to_entity.get(&rb_h) {
                    *damage_map.entry(entity).or_default() += damage;
                }
            }
        }
    }

    for (entity, damage) in damage_map {
        if let Ok((mut health, _)) = health_q.get_mut(entity) {
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
        if health.current > 0.0 { continue; }
        commands.entity(entity).despawn();
        if let Some(net_id) = net_id {
            quic.send(SendTarget::All, Channel::Ordered, &MsgType::DespawnCommand(net_id.clone()));
        }
    }
}
