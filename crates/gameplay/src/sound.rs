pub use audio::{SoundEmitter, SoundQueue, SoundRequest};
use bevy::prelude::*;
use physics::physics_world::{PhysicsWorld, rb_vel};

#[cfg(feature = "client")]
use crate::{
    CollisionSound,
    collision::{CollisionImpactSet, CollisionImpacts},
};

pub fn entity_velocity(world: &PhysicsWorld, entity: Option<Entity>) -> Vec3 {
    entity
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(rb_vel)
        .unwrap_or(Vec3::ZERO)
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
        sound_queue.play_3d(
            sound.0,
            impact.position,
            entity_velocity(&world, Some(impact.entity)),
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
