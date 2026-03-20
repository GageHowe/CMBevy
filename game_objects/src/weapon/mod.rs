use bevy::prelude::*;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};

pub mod rifle;
pub mod shotgun;
pub mod hail_mary;

/// Shared weapon plugin.
pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(hail_mary::HailMaryPlugin);
        // swap placeholder cuboid colliders for convex hulls once assets load (client-only)
        #[cfg(feature = "client")]
        app.add_systems(Update, swap_weapon_hull_colliders);
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

/// Replaces a weapon entity's placeholder cuboid collider with its convex hull once the asset loads.
/// Triggered when spawn attaches a PendingHullCollider; removed once the swap is complete.
#[cfg(feature = "client")]
fn swap_weapon_hull_colliders(
    mut commands: Commands,
    pending: Query<(Entity, &PendingHullCollider, &RigidBodyHandleComponent)>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
    mut world: ResMut<PhysicsWorld>,
) {
    for (entity, hull_handle, body_handle) in pending.iter() {
        let Some(hull) = hull_assets.get(&hull_handle.0) else { continue };
        let hull_collider = hull.0.clone();
        let existing: Vec<_> = world.rigid_body_set.get(body_handle.0)
            .map(|rb| rb.colliders().to_vec())
            .unwrap_or_default();
        for ch in existing {
            let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
            collider_set.remove(ch, island_manager, rigid_body_set, true);
        }
        {
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
            collider_set.insert_with_parent(hull_collider, body_handle.0, rigid_body_set);
        }
        commands.entity(entity).try_remove::<PendingHullCollider>();
    }
}

/// Per-weapon-type firing logic. Implement on each weapon component.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    /// Called every FixedUpdate tick when this weapon is active.
    /// `want_fire` is true when the player is pressing the fire button.
    /// Returns true if the weapon actually discharged this tick.
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, origin: Vec3, aim_dir: Vec3, shooter: Option<Entity>, tick: u64, want_fire: bool) -> bool;
    /// FMOD event path for the fire sound. None = silent.
    fn fire_sound(&self) -> Option<&'static str> { None }
}