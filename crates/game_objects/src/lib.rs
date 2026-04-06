use bevy::prelude::Command;
use bevy::prelude::*;
pub use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::{RigidBodyHandle, RigidBodyHandleComponent};
use std::collections::HashMap;

pub mod asset_path;
pub mod components;
pub mod generic;
pub mod health;
pub mod interaction;
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
                (
                    index_added_network_ids,
                    index_added_or_changed_rigid_bodies,
                    index_removed_network_ids,
                    index_removed_rigid_bodies,
                ),
            )
            .add_systems(
                FixedPreUpdate,
                (
                    index_added_network_ids,
                    index_added_or_changed_rigid_bodies,
                    index_removed_network_ids,
                    index_removed_rigid_bodies,
                ),
            );
    }
}

#[derive(Resource, Default)]
pub struct NetworkEntityMap {
    by_id: HashMap<net::message::NetworkID, Entity>,
    by_entity: HashMap<Entity, net::message::NetworkID>,
    bodies_by_id: HashMap<net::message::NetworkID, RigidBodyHandle>,
}

impl NetworkEntityMap {
    pub fn get(&self, net_id: &net::message::NetworkID) -> Option<Entity> {
        self.get_entity(net_id)
    }

    pub fn get_entity(&self, net_id: &net::message::NetworkID) -> Option<Entity> {
        self.by_id.get(net_id).copied()
    }

    pub fn get_body(&self, net_id: &net::message::NetworkID) -> Option<RigidBodyHandle> {
        self.bodies_by_id.get(net_id).copied()
    }

    pub fn get_entity_and_body(
        &self,
        net_id: &net::message::NetworkID,
    ) -> Option<(Entity, RigidBodyHandle)> {
        Some((self.get_entity(net_id)?, self.get_body(net_id)?))
    }

    pub fn body_pairs(&self) -> impl Iterator<Item = (&net::message::NetworkID, &RigidBodyHandle)> {
        self.bodies_by_id.iter()
    }

    pub fn body_pairs_vec(&self) -> Vec<(net::message::NetworkID, RigidBodyHandle)> {
        self.bodies_by_id
            .iter()
            .map(|(net_id, handle)| (net_id.clone(), *handle))
            .collect()
    }

    pub fn insert(&mut self, net_id: net::message::NetworkID, entity: Entity) {
        if let Some(prev_id) = self.by_entity.insert(entity, net_id.clone()) {
            self.by_id.remove(&prev_id);
            self.bodies_by_id.remove(&prev_id);
        }
        if let Some(prev_entity) = self.by_id.insert(net_id.clone(), entity) {
            self.by_entity.remove(&prev_entity);
        }
    }

    pub fn insert_body(&mut self, net_id: net::message::NetworkID, handle: RigidBodyHandle) {
        self.bodies_by_id.insert(net_id, handle);
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        let Some(net_id) = self.by_entity.remove(&entity) else {
            return;
        };
        self.by_id.remove(&net_id);
        self.bodies_by_id.remove(&net_id);
    }

    pub fn remove_body_for_entity(&mut self, entity: Entity) {
        let Some(net_id) = self.by_entity.get(&entity) else {
            return;
        };
        self.bodies_by_id.remove(net_id);
    }
}

fn index_added_network_ids(
    mut map: ResMut<NetworkEntityMap>,
    added: Query<
        (
            Entity,
            &net::message::NetworkID,
            Option<&RigidBodyHandleComponent>,
        ),
        Added<net::message::NetworkID>,
    >,
) {
    for (entity, net_id, body) in added.iter() {
        map.insert(net_id.clone(), entity);
        if let Some(body) = body {
            map.insert_body(net_id.clone(), body.0);
        }
    }
}

fn index_added_or_changed_rigid_bodies(
    mut map: ResMut<NetworkEntityMap>,
    bodies: Query<
        (Entity, &RigidBodyHandleComponent),
        Or<(
            Added<RigidBodyHandleComponent>,
            Changed<RigidBodyHandleComponent>,
        )>,
    >,
) {
    for (entity, body) in bodies.iter() {
        let Some(net_id) = map.by_entity.get(&entity).cloned() else {
            continue;
        };
        map.insert_body(net_id, body.0);
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

fn index_removed_rigid_bodies(
    mut map: ResMut<NetworkEntityMap>,
    mut removed: RemovedComponents<RigidBodyHandleComponent>,
) {
    for entity in removed.read() {
        map.remove_body_for_entity(entity);
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
        world
            .entity_mut(self.entity)
            .insert(self.cmd.net_id.clone());
        for_each_game_object!(dispatch_spawn_match self.cmd.kind, self.entity, &self.cmd, world;);
    }
}
