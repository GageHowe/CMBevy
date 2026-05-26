use bevy::{
    ecs::component::{Component, Mutable},
    prelude::*,
};
use common::NetworkID;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ComponentUpdate {
    pub net_id: NetworkID,
    pub component_type_path: String,
    pub payload: Vec<u8>,
}

struct ReplicatedComponent {
    component_type_path: &'static str,
    collect_changed: Box<dyn Fn(&mut World, &mut Vec<ComponentUpdate>) + Send + Sync>,
    collect_for_entity:
        Box<dyn Fn(bevy::ecs::world::EntityRef<'_>, &NetworkID, &mut Vec<ComponentUpdate>) + Send + Sync>,
    apply: Box<dyn Fn(Entity, &mut World, &[u8]) + Send + Sync>,
}

#[derive(Resource, Default)]
pub struct ReplicationRegistry {
    components: Vec<ReplicatedComponent>,
}

pub trait ReplicationAppExt {
    fn replicate_component<T>(&mut self) -> &mut Self
    where
        T: Component<Mutability = Mutable>
            + Clone
            + Serialize
            + DeserializeOwned
            + Send
            + Sync
            + 'static;

    fn replicate_component_with<T, S>(
        &mut self,
        extract: fn(&T) -> S,
        apply_existing: fn(&mut T, S),
        insert: fn(S) -> T,
    ) -> &mut Self
    where
        T: Component<Mutability = Mutable> + Send + Sync + 'static,
        S: Clone + Serialize + DeserializeOwned + Send + Sync + 'static;
}

impl ReplicationAppExt for App {
    fn replicate_component<T>(&mut self) -> &mut Self
    where
        T: Component<Mutability = Mutable>
            + Clone
            + Serialize
            + DeserializeOwned
            + Send
            + Sync
            + 'static,
    {
        self.replicate_component_with::<T, T>(
            |value| value.clone(),
            |slot, value| *slot = value,
            |value| value,
        )
    }

    fn replicate_component_with<T, S>(
        &mut self,
        extract: fn(&T) -> S,
        apply_existing: fn(&mut T, S),
        insert: fn(S) -> T,
    ) -> &mut Self
    where
        T: Component<Mutability = Mutable> + Send + Sync + 'static,
        S: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        let component_type_path = component_type_path::<T>();
        let mut registry = self
            .world_mut()
            .get_resource_or_init::<ReplicationRegistry>();
        if registry
            .components
            .iter()
            .any(|component| component.component_type_path == component_type_path)
        {
            return self;
        }
        registry.components.push(ReplicatedComponent {
            component_type_path,
            collect_changed: collect_changed_impl::<T, S>(extract),
            collect_for_entity: collect_for_entity_impl::<T, S>(extract),
            apply: apply_impl::<T, S>(apply_existing, insert),
        });
        self
    }
}

pub fn component_type_path<T>() -> &'static str {
    std::any::type_name::<T>()
}

pub fn component_update_for<T, S>(net_id: NetworkID, payload: &S) -> Option<ComponentUpdate>
where
    T: 'static,
    S: Serialize,
{
    Some(ComponentUpdate {
        net_id,
        component_type_path: component_type_path::<T>().to_string(),
        payload: serde_json::to_vec(payload).ok()?,
    })
}

pub fn collect_changed_component_updates(world: &mut World) -> Vec<ComponentUpdate> {
    let mut updates = Vec::new();
    world.resource_scope(|world, registry: Mut<ReplicationRegistry>| {
        for component in &registry.components {
            (component.collect_changed)(world, &mut updates);
        }
    });
    updates
}

pub fn collect_entity_component_updates(
    entity: bevy::ecs::world::EntityRef<'_>,
    net_id: &NetworkID,
    registry: &ReplicationRegistry,
) -> Vec<ComponentUpdate> {
    let mut updates = Vec::new();
    for component in &registry.components {
        (component.collect_for_entity)(entity, net_id, &mut updates);
    }
    updates
}

pub fn apply_component_update(entity: Entity, update: &ComponentUpdate, world: &mut World) {
    world.resource_scope(|world, registry: Mut<ReplicationRegistry>| {
        let Some(component) = registry
            .components
            .iter()
            .find(|component| component.component_type_path == update.component_type_path)
        else {
            return;
        };
        (component.apply)(entity, world, &update.payload);
    });
}

fn collect_changed_impl<T, S>(
    extract: fn(&T) -> S,
) -> Box<dyn Fn(&mut World, &mut Vec<ComponentUpdate>) + Send + Sync>
where
    T: Component<Mutability = Mutable> + Send + Sync + 'static,
    S: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    Box::new(move |world, updates| {
        let mut query = world.query::<(&NetworkID, Ref<T>)>();
        for (net_id, component) in query.iter(world) {
            if !component.is_changed() {
                continue;
            }
            let payload = extract(&component);
            let Some(update) = component_update_for::<T, _>(net_id.clone(), &payload) else {
                continue;
            };
            updates.push(update);
        }
    })
}

fn collect_for_entity_impl<T, S>(
    extract: fn(&T) -> S,
) -> Box<
    dyn Fn(bevy::ecs::world::EntityRef<'_>, &NetworkID, &mut Vec<ComponentUpdate>) + Send + Sync,
>
where
    T: Component<Mutability = Mutable> + Send + Sync + 'static,
    S: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    Box::new(move |entity, net_id, updates| {
        let Some(component) = entity.get::<T>() else {
            return;
        };
        let payload = extract(component);
        let Some(update) = component_update_for::<T, _>(net_id.clone(), &payload) else {
            return;
        };
        updates.push(update);
    })
}

fn apply_impl<T, S>(
    apply_existing: fn(&mut T, S),
    insert: fn(S) -> T,
) -> Box<dyn Fn(Entity, &mut World, &[u8]) + Send + Sync>
where
    T: Component<Mutability = Mutable> + Send + Sync + 'static,
    S: Clone + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    Box::new(move |entity, world, bytes| {
        let Ok(value) = serde_json::from_slice::<S>(bytes) else {
            return;
        };
        if let Some(mut component) = world.get_mut::<T>(entity) {
            apply_existing(&mut component, value);
        } else if world.entities().contains(entity) {
            world.entity_mut(entity).insert(insert(value));
        }
    })
}
