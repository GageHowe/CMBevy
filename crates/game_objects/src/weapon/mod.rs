use bevy::prelude::*;
use physics::physics_world::PhysicsWorld;
use net::message::NetworkID;
use crate::sound::SoundQueue;
use crate::pawn::CameraEffector;

pub mod rifle;
pub mod hail_mary;

/// Shared weapon plugin.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            rifle::RiflePlugin,
            hail_mary::HailMaryPlugin,
            crate::projectile::ProjectilePlugin,
        ));
    }
}

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// All context a weapon's fixed_update may need: input buttons and output channels.
/// Fields are optional so weapons compile and behave correctly on the server (no sound/camera).
pub struct FireCtx<'a> {
    pub want_fire: bool,
    pub want_alt_fire: bool,
    pub origin: Vec3,
    pub aim_dir: Vec3,
    pub shooter: Option<Entity>,
    pub tick: u64,
    /// NetworkID of the weapon entity — used to tell the server what fired.
    pub net_id: Option<&'a NetworkID>,
    /// NetworkID of the pawn carrying the weapon — included in fire packets so other clients can find the ghost.
    pub shooter_net_id: Option<&'a NetworkID>,
    /// Push to play a one-shot sound this frame.
    pub sound: Option<&'a mut SoundQueue>,
    /// Local player camera; None on server or before possession.
    pub camera: Option<&'a mut CameraEffector>,
    /// QUIC manager for sending Fire messages; None in singleplayer.
    pub quic: Option<&'a mut net::quic::QuicManager>,
    /// Projectile id counter for client-side prediction; None in singleplayer / on server.
    pub id_counter: Option<&'a mut u32>,
}

/// Per-weapon-type firing logic. Implement on each weapon component.
/// Weapons own their complete fire behavior: cooldowns, projectiles, sounds, camera kick, networking.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    /// Called every FixedPreUpdate tick when this weapon is the active slot.
    /// The weapon reads input from ctx, spawns projectiles/effects, and calls ctx helpers as needed.
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, ctx: &mut FireCtx);
}
