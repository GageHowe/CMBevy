use bevy::prelude::*;

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

/// Attach to any entity (with Transform) for a persistent spatial sound tied to entity lifetime.
/// The instance plays until the component or entity is removed. Velocity is sourced from the
/// entity's RigidBodyHandleComponenet if present, otherwise zero.
#[derive(Component)]
pub struct SoundEmitter {
    pub event: &'static str,
}
