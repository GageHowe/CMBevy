use bevy::prelude::*;

pub mod weapon;
pub mod rifle;
pub mod shotgun;

// Re-export shared types so callers don't have to reach into `weapon::weapon` directly.
pub use weapon::{
    fire_all_weapons, fire_weapons,
    FireEffect, FiredWeapons,
    WeaponComponent, WeaponInput, WeaponState,
    insert_weapon_physics,
};

/// Registers shared weapon resources.
/// Each binary registers `fire_all_weapons` itself with its own ordering
/// (server: after on_message, before step_physics; client: before step_physics).
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FiredWeapons>();
    }
}
