use bevy::prelude::*;

pub mod weapon;
pub mod rifle;
pub mod shotgun;

// Re-export shared types and the fire_weapons dispatcher so callers don't have to
// reach into `weapon::weapon` directly.
pub use weapon::{fire_weapons, spawn_from_command, FireEffect, FiredWeapons, WeaponComponent, WeaponInput, WeaponKind, insert_weapon_physics};

/// Registers shared weapon resources.
/// Each binary registers the `fire_weapons<T>` systems itself with its own ordering
/// (server: after on_message, before step_physics; client: before step_physics).
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FiredWeapons>();
    }
}
