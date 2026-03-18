use bevy::prelude::*;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::PhysicsWorld;

pub mod rifle;
pub mod shotgun;
pub mod hail_mary;

/// Shared weapon plugin. Currently empty; each binary registers its own systems.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(hail_mary::HailMaryPlugin);
    }
}

// TODO: make a weapon that's KinematicVelocityBased like a plasma launcher
// can this be affected by add_impulse?

/// Attached to a weapon entity when its convex hull is still loading.
/// Removed by `swap_weapon_hull_colliders` once the asset is ready.
#[derive(Component)]
pub struct PendingHullCollider(pub Handle<ConvexHullAsset>);

/// Marker component present on every weapon entity regardless of type.
#[derive(Component)]
pub struct WeaponComponent;

/// Per-weapon-type firing logic. Implement on each weapon component.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    /// Called every FixedUpdate tick when this weapon is active.
    /// `want_fire` is true when the player is pressing the fire button.
    /// Returns true if the weapon actually discharged this tick.
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, origin: Vec3, aim_dir: Vec3, shooter: Option<Entity>, tick: u64, want_fire: bool) -> bool;
    /// FMOD event path for the fire sound. None = silent.
    fn fire_sound(&self) -> Option<&'static str> { None }
}