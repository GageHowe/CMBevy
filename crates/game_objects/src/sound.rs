use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_vel};

/// Push to SoundQueue for positional one-shots (fire, impact, etc).
/// FMOD creates, starts, and releases the instance immediately — no ownership needed.
pub struct SoundRequest {
    pub event: &'static str,
    /// None = 2D (no spatialization). Use for local player sounds like own weapon fire.
    pub position: Option<Vec3>,
    pub velocity: Vec3,
}

/// One-shot sound queue. Push requests; the sound system drains and plays them each PostUpdate.
#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<SoundRequest>);

impl SoundQueue {
    pub fn play_2d(&mut self, event: &'static str) {
        self.0.push(SoundRequest {
            event,
            position: None,
            velocity: Vec3::ZERO,
        });
    }

    pub fn play_3d(&mut self, event: &'static str, position: Vec3, velocity: Vec3) {
        self.0.push(SoundRequest {
            event,
            position: Some(position),
            velocity,
        });
    }
}

pub fn entity_velocity(world: &PhysicsWorld, entity: Option<Entity>) -> Vec3 {
    entity
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(rb_vel)
        .unwrap_or(Vec3::ZERO)
}

/// Attach to any entity (with Transform) for a persistent spatial sound tied to entity lifetime.
/// The instance plays until the component or entity is removed. Velocity is sourced from the
/// entity's RigidBodyHandleComponenet if present, otherwise zero.
#[derive(Component)]
pub struct SoundEmitter {
    pub event: &'static str,
}
