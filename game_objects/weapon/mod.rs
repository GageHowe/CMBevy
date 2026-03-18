use bevy::prelude::*;

pub mod weapon;
pub mod rifle;
pub mod shotgun;
pub mod hail_mary;

pub use weapon::{Weapon, WeaponComponent, PendingHullCollider};

/// Shared weapon plugin. Currently empty; each binary registers its own systems.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, _app: &mut App) {}
}

// TODO: make a weapon that's KinematicVelocityBased like a plasma launcher
// can this be affected by add_impulse?
