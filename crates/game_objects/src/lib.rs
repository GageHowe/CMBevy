use bevy::prelude::*;
use bevy::prelude::Command;
pub use common::GameObjectKind;
use net::message::SpawnCommand;

pub mod health;
pub mod sound;
pub mod pawn;
pub mod weapon;
pub mod projectile;
pub mod planet;
pub mod generic;
pub mod level;
pub mod atmosphere;
pub use generic::{spawn_generic, GenericShape};

/*
This module is for GameObjects, a collection of objects that can be spawned into the game world.
GameObjects can be spawned by:
* Server -> Client spawn commands,
* the Client (in the case of Singleplayer, static objects, predicted projectiles etc)
* Lua scripting

The goal is to have a clean and simple calling convention so callers can spawn a GameObject easily.

*/

/// everything that appears in a map needs to implement this.
/// Requires Reflect, Default in order to instantiate these objects from scene ron file.
/// FromWorld is automatically implemented for any type implementing Default
pub trait GameObject: Default + Reflect {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World);
}

/// Spawns any game object described by a SpawnCommand onto a pre-allocated entity.
/// Queue via `commands.queue(SpawnGameObjectCommand { entity, cmd })`.
pub struct SpawnGameObjectCommand {
    pub entity: Entity,
    pub cmd: SpawnCommand,
}

impl Command for SpawnGameObjectCommand {
    fn apply(self, world: &mut World) {
        match self.cmd.kind {
            GameObjectKind::Biped              => pawn::biped::BipedPawnComponent::spawn(self.entity, &self.cmd, world),
            GameObjectKind::Spaceship          => pawn::spaceship::SpaceshipPawnComponent::spawn(self.entity, &self.cmd, world),
            GameObjectKind::Rifle              => weapon::rifle::RifleComponent::spawn(self.entity, &self.cmd, world),
            GameObjectKind::HailMary           => weapon::hail_mary::HailMaryComponent::spawn(self.entity, &self.cmd, world),
            GameObjectKind::RifleProjectile    => projectile::rifle::RifleProjectile::spawn(self.entity, &self.cmd, world),
            GameObjectKind::HailMaryProjectile => projectile::hail_mary::HailMaryProjectile::spawn(self.entity, &self.cmd, world),
            _ => { world.entity_mut(self.entity).despawn(); }
        }
    }
}
