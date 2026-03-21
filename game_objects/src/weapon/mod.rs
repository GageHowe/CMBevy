use bevy::prelude::*;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use net::message::NetworkID;
use crate::sound::SoundQueue;
use crate::pawn::biped::CameraEffects;

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
    /// Push to play a one-shot sound this frame.
    pub sound: Option<&'a mut SoundQueue>,
    /// Local player camera; None on server or before possession.
    pub camera: Option<&'a mut CameraEffects>,
    /// QUIC manager for sending Fire messages; None in singleplayer.
    pub quic: Option<&'a mut net::quic::QuicManager>,
}


/// Per-weapon-type firing logic. Implement on each weapon component.
/// Weapons own their complete fire behavior: cooldowns, projectiles, sounds, camera kick, networking.
pub trait Weapon: Component<Mutability = bevy::ecs::component::Mutable> + Default {
    /// Called every FixedPreUpdate tick when this weapon is the active slot.
    /// The weapon reads input from ctx, spawns projectiles/effects, and calls ctx helpers as needed.
    fn fixed_update(&mut self, world: &mut PhysicsWorld, commands: &mut Commands, ctx: &mut FireCtx);
}

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
