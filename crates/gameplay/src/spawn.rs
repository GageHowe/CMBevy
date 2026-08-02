//! Central spawn dispatch for replicated game object kinds.

use bevy::prelude::{Command, *};

use crate::{
    archetype::{SpawnArchetypeTrait, SpawnBundle},
    gc::WorldObjectGc,
    net::message::SpawnCommand,
};

/// what is this?
#[derive(Component, Clone, Copy)]
pub struct SpawnReplicated(pub &'static str);

/// i think this determines if an object should have splash damage dealt to it
#[derive(Component)]
pub struct CenterOfMassSplashDamage;

#[derive(Component, Clone, Copy)]
pub struct DespawnOnDeath;

/// tells the sound engine to play a fmod sound reference when this object collides with something else
#[derive(Component, Clone, Copy)]
pub struct CollisionSound(pub &'static str);

pub fn insert_spawn_metadata(
    entity: Entity,
    world: &mut World,
    gc_lifetime_secs: Option<f32>,
    despawn_on_death: bool,
    collision_sound: Option<&'static str>,
    use_center_of_mass_splash_damage: bool,
) {
    let mut entity = world.entity_mut(entity);

    // should this entity die when health reaches zero?
    if despawn_on_death {
        entity.insert(DespawnOnDeath);
    } else {
        entity.remove::<DespawnOnDeath>();
    }

    // collision sound
    if let Some(sound) = collision_sound {
        entity.insert(CollisionSound(sound));
    } else {
        entity.remove::<CollisionSound>();
    }

    // splash damage
    if use_center_of_mass_splash_damage {
        entity.insert(CenterOfMassSplashDamage);
    } else {
        entity.remove::<CenterOfMassSplashDamage>();
    }

    // if this object should despawn after some time, add a WorldObjectGc component
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
    net_id: &crate::net::message::NetworkID,
) -> Option<Entity> {
    if let Some(networked) = world.get_resource::<crate::NetworkEntityMap>()
        && let Some(entity) = networked.get_entity(net_id)
    {
        return Some(entity);
    }
    world
        .query::<(Entity, &crate::net::message::NetworkID)>()
        .iter(world)
        .find_map(|(entity, entity_net_id)| (entity_net_id == net_id).then_some(entity))
}

/// Spawns any game object described by a SpawnCommand onto a pre-allocated entity.
/// Queue via `commands.queue(SpawnGameObjectCommand { entity, cmd })`.
pub struct SpawnGameObjectCommand {
    pub entity: Entity,
    pub cmd: SpawnCommand,
}
impl Command for SpawnGameObjectCommand {
    type Out = ();

    fn apply(self, world: &mut World) {
        world
            .entity_mut(self.entity)
            .insert((self.cmd.net_id.clone(), self.cmd.archetype.clone()));
        self.cmd.archetype.clone().spawn(
            self.entity,
            SpawnBundle {
                position: self.cmd.position_or_zero(),
                velocity: self.cmd.velocity_or_zero(),
                rotation: self.cmd.rotation_or_identity(),
                angular_velocity: self.cmd.angular_velocity_or_zero(),
                net_id: Some(self.cmd.net_id.clone()),
                parent_net_id: self.cmd.parent_net_id.clone(),
            },
            world,
        );
        let body = world
            .get::<physics::physics_world::RigidBodyHandleComponent>(self.entity)
            .map(|body| body.0);
        if let Some(mut map) = world.get_resource_mut::<crate::NetworkEntityMap>() {
            map.insert(self.cmd.net_id.clone(), self.entity);
            if let Some(body) = body {
                map.insert_body(self.cmd.net_id.clone(), body);
            }
        }
        if let Some(parent_net_id) = &self.cmd.parent_net_id
            && let Some(parent) = find_entity_by_net_id(world, parent_net_id)
        {
            world.entity_mut(parent).add_child(self.entity);
        }
    }
}

impl crate::net::message::Message for SpawnCommand {
    fn handle(self, world: &mut World) {
        let entity = crate::find_entity_by_net_id(world, &self.net_id)
            .unwrap_or_else(|| world.spawn_empty().id());
        SpawnGameObjectCommand { entity, cmd: self }.apply(world);
    }
}
