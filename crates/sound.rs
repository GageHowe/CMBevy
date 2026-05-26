use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_vel};

#[cfg(feature = "client")]
use crate::{
    CollisionSound,
    collision::{CollisionImpactSet, CollisionImpacts},
};

/// Push to SoundQueue for positional one-shots (fire, impact, etc).
/// FMOD creates, starts, and releases the instance immediately — no ownership needed.
#[derive(Clone, Copy)]
pub struct SoundRequest {
    pub event: &'static str,
    /// None = 2D (no spatialization). Use for local player sounds like own weapon fire.
    pub position: Option<Vec3>,
    pub velocity: Vec3,
    pub gain: f32,
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
            gain: 1.0,
        });
    }

    pub fn play_3d(&mut self, event: &'static str, position: Vec3, velocity: Vec3) {
        self.play_3d_with_gain(event, position, velocity, 1.0);
    }

    pub fn play_3d_with_gain(
        &mut self,
        event: &'static str,
        position: Vec3,
        velocity: Vec3,
        gain: f32,
    ) {
        self.0.push(SoundRequest {
            event,
            position: Some(position),
            velocity,
            gain,
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
/// Marker for a looping/persistent spatial sound source attached to an entity.
pub struct SoundEmitter {
    pub event: &'static str,
}

#[cfg(feature = "client")]
pub fn play_collision_sounds(
    impacts: Res<CollisionImpacts>,
    sounds: Query<&CollisionSound>,
    world: Res<PhysicsWorld>,
    mut sound_queue: Option<ResMut<SoundQueue>>,
) {
    let Some(sound_queue) = sound_queue.as_mut() else {
        warn!("play_collision_sounds: Sound queue is None!");
        return;
    };
    for impact in &impacts.0 {
        if !impact.is_new {
            continue;
        }
        let Ok(sound) = sounds.get(impact.entity) else {
            continue;
        };
        let gain = collision_sound_gain(impact.impulse);
        if gain <= 0.0 {
            continue;
        }
        sound_queue.play_3d_with_gain(
            sound.0,
            impact.position,
            entity_velocity(&world, Some(impact.entity)),
            gain,
        );
    }
}

#[cfg(feature = "client")]
pub fn configure_collision_sound_system(app: &mut App) {
    app.add_systems(FixedUpdate, play_collision_sounds.after(CollisionImpactSet));
}

#[cfg(feature = "client")]
fn collision_sound_gain(impulse: f32) -> f32 {
    const MIN_IMPULSE: f32 = 20.0;
    const FULL_VOLUME_IMPULSE: f32 = 250.0;

    if impulse <= MIN_IMPULSE {
        return 0.0;
    }
    ((impulse - MIN_IMPULSE) / (FULL_VOLUME_IMPULSE - MIN_IMPULSE)).clamp(0.0, 1.0)
}
