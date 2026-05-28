//! Central spawn dispatch for replicated game object kinds.

use std::collections::HashMap;

use bevy::prelude::{Command, *};
use net::message::SpawnCommand;

use crate::gc::WorldObjectGc;

pub type SpawnFn = fn(Entity, &SpawnCommand, &mut World);

#[derive(Resource, Default)]
pub struct SpawnRegistry(pub HashMap<&'static str, SpawnFn>);

#[derive(Component, Clone, Copy)]
pub struct SpawnReplicated(pub &'static str);

#[derive(Component)]
pub struct CenterOfMassSplashDamage;

#[derive(Component, Clone, Copy)]
pub struct DespawnOnDeath;

#[derive(Component, Clone, Copy)]
pub struct CollisionSound(pub &'static str);

pub fn register_spawnable(
    app: &mut App,
    spawn_name: &'static str,
    spawn: SpawnFn,
) {
    let mut registry = app.world_mut().get_resource_or_insert_with(SpawnRegistry::default);
    registry.0.insert(spawn_name, spawn);
}

pub fn insert_spawn_metadata(
    entity: Entity,
    world: &mut World,
    gc_lifetime_secs: Option<f32>,
    despawn_on_death: bool,
    collision_sound: Option<&'static str>,
    use_center_of_mass_splash_damage: bool,
) {
    let mut entity = world.entity_mut(entity);
    if despawn_on_death {
        entity.insert(DespawnOnDeath);
    } else {
        entity.remove::<DespawnOnDeath>();
    }
    if let Some(sound) = collision_sound {
        entity.insert(CollisionSound(sound));
    } else {
        entity.remove::<CollisionSound>();
    }
    if use_center_of_mass_splash_damage {
        entity.insert(CenterOfMassSplashDamage);
    } else {
        entity.remove::<CenterOfMassSplashDamage>();
    }
    if let Some(reset_secs) = gc_lifetime_secs {
        if entity.get::<WorldObjectGc>().is_none() {
            entity.insert(WorldObjectGc::new(reset_secs));
        }
    } else {
        entity.remove::<WorldObjectGc>();
    }
}

pub fn find_entity_by_net_id(
    world: &mut World,
    net_id: &net::message::NetworkID,
) -> Option<Entity> {
    if let Some(networked) = world.get_resource::<crate::NetworkEntityMap>()
        && let Some(entity) = networked.get_entity(net_id)
    {
        return Some(entity);
    }
    world
        .query::<(Entity, &net::message::NetworkID)>()
        .iter(world)
        .find_map(|(entity, entity_net_id)| (entity_net_id == net_id).then_some(entity))
}

fn spawn_game_object(spawn_name: &str, entity: Entity, cmd: &SpawnCommand, world: &mut World) {
    let Some(spawn) = world
        .get_resource::<SpawnRegistry>()
        .and_then(|registry| registry.0.get(spawn_name))
        .copied()
    else {
        panic!("unknown spawn '{spawn_name}'");
    };
    spawn(entity, cmd, world);
}

/// Spawns any game object described by a SpawnCommand onto a pre-allocated entity.
/// Queue via `commands.queue(SpawnGameObjectCommand { entity, cmd })`.
pub struct SpawnGameObjectCommand {
    pub entity: Entity,
    pub cmd: SpawnCommand,
}

impl Command for SpawnGameObjectCommand {
    fn apply(self, world: &mut World) {
        world.entity_mut(self.entity).insert(self.cmd.net_id.clone());
        spawn_game_object(self.cmd.spawn_name.as_str(), self.entity, &self.cmd, world);
        if let Some(parent_net_id) = &self.cmd.parent_net_id
            && let Some(parent) = find_entity_by_net_id(world, parent_net_id)
        {
            world.entity_mut(parent).add_child(self.entity);
        }
    }
}
