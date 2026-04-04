use bevy::prelude::Command;
use bevy::prelude::*;
pub use common::GameObjectKind;
use net::message::SpawnCommand;
use std::collections::HashMap;

pub mod components;
pub mod generic;
pub mod health;
pub mod level;
pub mod pawn;
pub mod projectile;
pub mod sound;
pub mod weapon;
pub use components::{atmosphere, planet};
pub use generic::{GenericShape, spawn_generic};

pub struct GameObjectsPlugin;

impl Plugin for GameObjectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetworkEntityMap>()
            .add_systems(
                PreUpdate,
                (index_added_network_ids, index_removed_network_ids),
            )
            .add_systems(
                FixedPreUpdate,
                (index_added_network_ids, index_removed_network_ids),
            );
    }
}

#[derive(Resource, Default)]
pub struct NetworkEntityMap {
    by_id: HashMap<net::message::NetworkID, Entity>,
    by_entity: HashMap<Entity, net::message::NetworkID>,
}

impl NetworkEntityMap {
    pub fn get(&self, net_id: &net::message::NetworkID) -> Option<Entity> {
        self.by_id.get(net_id).copied()
    }

    pub fn insert(&mut self, net_id: net::message::NetworkID, entity: Entity) {
        if let Some(prev_id) = self.by_entity.insert(entity, net_id.clone()) {
            self.by_id.remove(&prev_id);
        }
        if let Some(prev_entity) = self.by_id.insert(net_id.clone(), entity) {
            self.by_entity.remove(&prev_entity);
        }
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        let Some(net_id) = self.by_entity.remove(&entity) else {
            return;
        };
        self.by_id.remove(&net_id);
    }
}

fn index_added_network_ids(
    mut map: ResMut<NetworkEntityMap>,
    added: Query<(Entity, &net::message::NetworkID), Added<net::message::NetworkID>>,
) {
    for (entity, net_id) in added.iter() {
        map.insert(net_id.clone(), entity);
    }
}

fn index_removed_network_ids(
    mut map: ResMut<NetworkEntityMap>,
    mut removed: RemovedComponents<net::message::NetworkID>,
) {
    for entity in removed.read() {
        map.remove_entity(entity);
    }
}

macro_rules! for_each_game_object {
    ($m:ident $($args:tt)*) => {
        $m!(
            $($args)*
            GameObjectKind::Biped => pawn::biped::BipedPawnComponent,
            GameObjectKind::Spaceship => pawn::spaceship::SpaceshipPawnComponent,
            GameObjectKind::Rifle => weapon::rifle::RifleComponent,
            GameObjectKind::HailMary => weapon::hail_mary::HailMaryComponent,
            GameObjectKind::Rpg => weapon::rpg::RpgComponent,
            GameObjectKind::RifleProjectile => projectile::rifle::RifleProjectile,
            GameObjectKind::HailMaryProjectile => projectile::hail_mary::HailMaryProjectile,
            GameObjectKind::RpgProjectile => projectile::rpg::RpgProjectile
        )
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

/*
This module is for GameObjects, a collection of objects that can be spawned into the game world.
GameObjects can be spawned by:
* Server -> Client spawn commands,
* the Client (in the case of Singleplayer, static objects, predicted projectiles etc)
* Lua scripting

The goal is to have a clean and simple calling convention so callers can spawn a GameObject easily.

*/

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
        world
            .entity_mut(self.entity)
            .insert(self.cmd.net_id.clone());
        for_each_game_object!(dispatch_spawn_match self.cmd.kind, self.entity, &self.cmd, world;);
    }
}
