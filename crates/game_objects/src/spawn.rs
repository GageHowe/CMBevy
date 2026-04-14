//! Central spawn dispatch for the reflected game object types shared across the game.

use bevy::prelude::{Command, *};
use common::GameObjectKind;
use net::message::SpawnCommand;
use pawn::biped_ability::implementors::JetpackPickup;

use crate::{pawn, projectile, weapon};

macro_rules! for_each_game_object {
    ($m:ident $($args:tt)*) => {
        $m!(
            $($args)*
            GameObjectKind::Biped => pawn::biped::BipedPawnComponent,
            GameObjectKind::Spaceship => pawn::spaceship::SpaceshipPawnComponent,
            GameObjectKind::Pistol => weapon::pistol::PistolComponent,
            GameObjectKind::Rifle => weapon::rifle::RifleComponent,
            GameObjectKind::HailMary => weapon::hail_mary::HailMaryComponent,
            GameObjectKind::Rpg => weapon::rpg::RpgComponent,
            GameObjectKind::PistolProjectile => projectile::rifle::PistolProjectile,
            GameObjectKind::RifleProjectile => projectile::rifle::RifleProjectile,
            GameObjectKind::HailMaryProjectile => projectile::hail_mary::HailMaryProjectile,
            GameObjectKind::RpgProjectile => projectile::rpg::RpgProjectile,
            GameObjectKind::Jetpack => JetpackPickup
        )
    };
}

macro_rules! dispatch_game_object_match {
    (
        $method:ident,
        $kind:expr,
        $entity:expr,
        $world:expr;
        $($kind_path:path => $ty:path),+ $(,)?
    ) => {
        match $kind.clone() {
            $(
                $kind_path => <$ty as GameObject>::$method($entity, $world),
            )+
            _ => panic!("GameObjectKind::{:?} is not registered for SpawnGameObjectCommand", $kind),
        }
    };
}

macro_rules! dispatch_spawn_match {
    (
        $kind:expr,
        $entity:expr,
        $cmd:expr,
        $world:expr;
        $($kind_path:path => $ty:path),+ $(,)?
    ) => {
        match $kind.clone() {
            $(
                $kind_path => <$ty as GameObject>::spawn($entity, $cmd, $world),
            )+
            _ => panic!("GameObjectKind::{:?} is not registered for SpawnGameObjectCommand", $kind),
        }
    };
}

/// Runtime constructor for a spawnable game object.
pub trait GameObject: Default + Reflect {
    /// responsible for enacting all side effects that spawn this entity
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World);
    /// callback that is called when entities' health drops to 0, before they are despawned.
    /// responsible for particle effects, debris, cleanup, etc.
    fn on_death(_entity: Entity, _world: &mut World) -> bool {
        true
    }
}

pub fn dispatch_game_object_on_death(
    kind: GameObjectKind,
    entity: Entity,
    world: &mut World,
) -> bool {
    for_each_game_object!(dispatch_game_object_match on_death, kind, entity, world;)
}

/// Spawns any game object described by a SpawnCommand onto a pre-allocated entity.
/// Queue via `commands.queue(SpawnGameObjectCommand { entity, cmd })`.
pub struct SpawnGameObjectCommand {
    pub entity: Entity,
    pub cmd: SpawnCommand,
}

impl Command for SpawnGameObjectCommand {
    fn apply(self, world: &mut World) {
        // Insert NetworkID before type-specific spawn so the on_add hook for GameObjectKind
        // can use its presence as a guard to skip already-spawned entities.
        world.entity_mut(self.entity).insert(self.cmd.net_id.clone());
        for_each_game_object!(dispatch_spawn_match self.cmd.kind, self.entity, &self.cmd, world;);
    }
}
