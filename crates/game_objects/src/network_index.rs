//! Maintains fast lookups between network ids, entities, and rigid bodies.

use bevy::prelude::*;
use physics::physics_world::{RigidBodyHandle, RigidBodyHandleComponent};
use std::collections::HashMap;

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

    pub fn get_net_id_for_entity(&self, entity: Entity) -> Option<&net::message::NetworkID> {
        self.by_entity.get(&entity)
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

pub(crate) fn index_added_network_ids(
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

pub(crate) fn index_added_or_changed_rigid_bodies(
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

pub(crate) fn index_removed_network_ids(
    mut map: ResMut<NetworkEntityMap>,
    mut removed: RemovedComponents<net::message::NetworkID>,
) {
    for entity in removed.read() {
        map.remove_entity(entity);
    }
}

pub(crate) fn index_removed_rigid_bodies(
    mut map: ResMut<NetworkEntityMap>,
    mut removed: RemovedComponents<RigidBodyHandleComponent>,
) {
    for entity in removed.read() {
        map.remove_body_for_entity(entity);
    }
}
