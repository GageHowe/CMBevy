use bevy::prelude::*;
use std::collections::HashMap;
use common::{debug_println, NetworkID};
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent, RigidBodyHandle, step_physics};
use net::quic::{QuicManager, SendTarget, Channel};
use net::message::MsgType;

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

/// Applies damage to any entity with Health + a rigidbody based on collision impulse.
/// p.data.impulse is Rapier's normal constraint impulse — already accounts for mass,
/// inertia, angular velocity, and contact geometry. Tune THRESHOLD above resting contact
/// levels to avoid false positives from gravity reaction forces.
/// Must run after step_physics.
pub fn apply_collision_damage(
    world: Res<PhysicsWorld>,
    mut health_q: Query<(&mut Health, &RigidBodyHandleComponent)>,
) {
    // dynamic vs dynamic: raw impulse threshold (player-vs-player, object-vs-object)
    const FORCE_THRESHOLD: f32 = 40.0;
    const FORCE_SCALE:     f32 = 3.0;
    // dynamic vs static: delta-v threshold (fall damage). filters out wall-pressing and
    // penetration-correction forces (both capped at ~10 m/s by Rapier's corrective velocity)
    const VELOCITY_THRESHOLD: f32 = 35.0;
    const VELOCITY_SCALE:     f32 = 1.5;

    let mut damage_map: HashMap<Entity, f32> = HashMap::new();
    for pair in world.narrow_phase.contact_pairs() {
        if !pair.has_any_active_contact() { continue; }
        let impulse: f32 = pair.manifolds.iter()
            .flat_map(|m| m.points.iter())
            .map(|p| p.data.impulse)
            .sum();
        if impulse <= 0.0 { continue; }

        let rb1 = world.collider_set.get(pair.collider1).and_then(|c| c.parent());
        let rb2 = world.collider_set.get(pair.collider2).and_then(|c| c.parent());
        let dyn1 = rb1.and_then(|h| world.rigid_body_set.get(h)).map_or(false, |rb| rb.is_dynamic());
        let dyn2 = rb2.and_then(|h| world.rigid_body_set.get(h)).map_or(false, |rb| rb.is_dynamic());

        if dyn1 && dyn2 {
            // both dynamic: impulse-based
            if impulse < FORCE_THRESHOLD { continue; }
            let damage = (impulse - FORCE_THRESHOLD) * FORCE_SCALE;
            debug_println!("dyn-dyn impulse: {impulse:.2}  damage: {damage:.1}");
            for rb_h in [rb1, rb2].into_iter().flatten() {
                if let Some(&entity) = world.handle_to_entity.get(&rb_h) {
                    *damage_map.entry(entity).or_default() += damage;
                }
            }
        } else {

            // TODO: clean this up.
            // i decided I don't want collision damage with static objects

            // // one static: delta-v on the dynamic body only
            // let Some(rb_h) = (if dyn1 { rb1 } else if dyn2 { rb2 } else { continue }) else { continue };
            // let Some(rb) = world.rigid_body_set.get(rb_h) else { continue };
            // let mass = rb.mass();
            // if mass < 1e-3 { continue; }
            // let delta_v = impulse / mass;
            // if delta_v < VELOCITY_THRESHOLD { continue; }
            // let damage = (delta_v - VELOCITY_THRESHOLD) * VELOCITY_SCALE;
            // debug_println!("dyn-static delta_v: {delta_v:.1}  damage: {damage:.1}");
            // if let Some(&entity) = world.handle_to_entity.get(&rb_h) {
            //     *damage_map.entry(entity).or_default() += damage;
            // }
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
