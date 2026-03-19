use bevy::prelude::*;
use rapier3d::prelude::RigidBody;
pub use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::PhysicsWorld;

pub mod health;
pub mod sound;
pub mod pawn;
pub mod weapon;
pub mod planet;
pub mod generic;
pub mod level;
pub mod atmosphere;
pub use generic::{spawn_generic, GenericShape};

/*
This module is for GameObjects, a collection of objects that can bwe spawned into the game world.
GameObjects can be spawned by:
* Server -> Client spawn commands,
* the Client (in the case of Singleplayer, static objects, predicted projectiles etc)
* Lua scripting

The goal is to have a clean and simple calling convention so callers can spawn a GameObject easily.

*/

/// everything that appears in a map needs to implement this.
/// Requires FromWorld, Reflect, Default in order to instantiate these objects from scene ron file.
/// FromWorld is automatically implemented for any type implementing Default
pub trait GameObject : Default + Reflect {
    fn initialize(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity;
    fn cleanup();
    fn get_rigidbody() -> Option<RigidBody>;
}

/// Client-only visual parameters passed into spawn functions.
/// On the server (no `client` feature) this is a zero-size unit struct — zero cost.
#[cfg(feature = "client")]
pub struct VisualSpawnParams<'a> {
    pub meshes:    &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    /// The Camera3d entity to attach to an owned biped. None for ghosts / non-bipeds.
    pub camera: Option<Entity>,
}

#[cfg(not(feature = "client"))]
pub struct VisualSpawnParams;

/// Spawns any game object described by a SpawnCommand.
/// Returns the spawned Entity, or None if the kind is unrecognised.
/// The single match lives here; add new kinds by implementing their module and one arm below.
pub fn spawn_game_object(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
    hull_assets: &Assets<ConvexHullAsset>,
    visual: &mut VisualSpawnParams,
) -> Option<Entity> {
    match cmd.kind {
        GameObjectKind::Biped    => Some(pawn::biped::spawn_from_command(&cmd, commands, world, visual)),
        GameObjectKind::Rifle    => Some(weapon::rifle::spawn_from_command(cmd, commands, world, asset_server, hull_assets)),
        GameObjectKind::Shotgun  => Some(weapon::shotgun::spawn_from_command(cmd, commands, world, asset_server, hull_assets)),
        GameObjectKind::HailMary => Some(weapon::hail_mary::spawn_from_command(cmd, commands, world, asset_server, hull_assets)),
        _ => None,
    }
}
