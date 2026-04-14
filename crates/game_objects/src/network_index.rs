//! Maintains fast lookups between network ids, entities, and rigid bodies.

use std::collections::HashMap;

use bevy::prelude::*;
use physics::physics_world::{RigidBodyHandle, RigidBodyHandleComponent};

#[derive(Resource, Default)]
pub struct NetworkEntityMap {
    netid_to_entity: HashMap<net::message::NetworkID, Entity>,
    entity_to_netid: HashMap<Entity, net::message::NetworkID>,
    netid_to_rigidbody: HashMap<net::message::NetworkID, RigidBodyHandle>,
}

impl NetworkEntityMap {
    pub fn get(&self, net_id: &net::message::NetworkID) -> Option<Entity> {
        self.get_entity(net_id)
    }

    pub fn get_entity(&self, net_id: &net::message::NetworkID) -> Option<Entity> {
        self.netid_to_entity.get(net_id).copied()
    }

    pub fn get_net_id_for_entity(&self, entity: Entity) -> Option<&net::message::NetworkID> {
        self.entity_to_netid.get(&entity)
    }

    pub fn get_body(&self, net_id: &net::message::NetworkID) -> Option<RigidBodyHandle> {
        self.netid_to_rigidbody.get(net_id).copied()
    }

    pub fn get_entity_and_body(
        &self,
        net_id: &net::message::NetworkID,
    ) -> Option<(Entity, RigidBodyHandle)> {
        Some((self.get_entity(net_id)?, self.get_body(net_id)?))
    }

    pub fn body_pairs(&self) -> impl Iterator<Item = (&net::message::NetworkID, &RigidBodyHandle)> {
        self.netid_to_rigidbody.iter()
    }

    pub fn body_pairs_vec(&self) -> Vec<(net::message::NetworkID, RigidBodyHandle)> {
        self.netid_to_rigidbody.iter().map(|(net_id, handle)| (net_id.clone(), *handle)).collect()
    }

    pub fn insert(&mut self, net_id: net::message::NetworkID, entity: Entity) {
        if let Some(prev_id) = self.entity_to_netid.insert(entity, net_id.clone()) {
            self.netid_to_entity.remove(&prev_id);
            self.netid_to_rigidbody.remove(&prev_id);
        }
        if let Some(prev_entity) = self.netid_to_entity.insert(net_id.clone(), entity) {
            self.entity_to_netid.remove(&prev_entity);
        }
    }

    pub fn insert_body(&mut self, net_id: net::message::NetworkID, handle: RigidBodyHandle) {
        self.netid_to_rigidbody.insert(net_id, handle);
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        let Some(net_id) = self.entity_to_netid.remove(&entity) else {
            return;
        };
        self.netid_to_entity.remove(&net_id);
        self.netid_to_rigidbody.remove(&net_id);
    }

    pub fn remove_body_for_entity(&mut self, entity: Entity) {
        let Some(net_id) = self.entity_to_netid.get(&entity) else {
            return;
        };
        self.netid_to_rigidbody.remove(net_id);
    }
}

pub(crate) fn index_added_network_ids(
    mut map: ResMut<NetworkEntityMap>,
    added: Query<
        (Entity, &net::message::NetworkID, Option<&RigidBodyHandleComponent>),
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
        Or<(Added<RigidBodyHandleComponent>, Changed<RigidBodyHandleComponent>)>,
    >,
) {
    for (entity, body) in bodies.iter() {
        let Some(net_id) = map.entity_to_netid.get(&entity).cloned() else {
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
