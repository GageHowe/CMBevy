//! Central spawn dispatch for replicated game object kinds.

use bevy::prelude::{Command, *};
use net::message::{SpawnCommand, SpawnType};

use crate::gc::WorldObjectGc;

#[derive(Component)]
pub struct CenterOfMassSplashDamage;

#[derive(Component, Clone, Copy)]
pub struct DespawnOnDeath;

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

fn spawn_game_object(spawn_type: SpawnType, entity: Entity, cmd: &SpawnCommand, world: &mut World) {
    match spawn_type {
        SpawnType::Biped => crate::pawn::biped::spawn_biped(entity, cmd, world),
        SpawnType::Spaceship => crate::pawn::spaceship::spawn_spaceship(entity, cmd, world),
        SpawnType::SpaceshipShield => {
            crate::shield::spawn_spaceship_shield(entity, cmd, world)
        }
        SpawnType::Fighter => crate::pawn::fighter::spawn_fighter(entity, cmd, world),
        SpawnType::Truck => crate::pawn::truck::spawn_truck(entity, cmd, world),
        SpawnType::Hovercraft => crate::pawn::hovercraft::spawn_hovercraft(entity, cmd, world),
        SpawnType::Planet | SpawnType::Shotgun => {
            panic!("SpawnType::{spawn_type:?} has no spawn implementation");
        }
        SpawnType::Pistol => crate::weapon::pistol::spawn_pistol(entity, cmd, world),
        SpawnType::Beamer => crate::weapon::beamer::spawn_beamer(entity, cmd, world),
        SpawnType::Rifle => crate::weapon::rifle::spawn_rifle(entity, cmd, world),
        SpawnType::Smg => crate::weapon::smg::spawn_smg(entity, cmd, world),
        SpawnType::Failsafe => {
            panic!("SpawnType::Failsafe has no spawn implementation")
        }
        SpawnType::HailMary => crate::weapon::hail_mary::spawn_hail_mary(entity, cmd, world),
        SpawnType::Thumper => crate::weapon::thumper::spawn_thumper(entity, cmd, world),
        SpawnType::Lobber => crate::weapon::lobber::spawn_lobber(entity, cmd, world),
        SpawnType::CoilLauncher => {
            crate::weapon::coil_launcher::spawn_coil_launcher(entity, cmd, world)
        }
        SpawnType::TetherGun => {
            panic!("SpawnType::{spawn_type:?} has no spawn implementation")
        }
        SpawnType::Jetpack => {
            crate::pawn::biped_ability::spawn_jetpack_pickup(entity, cmd, world)
        }
        SpawnType::Dash => crate::pawn::biped_ability::spawn_dash_pickup(entity, cmd, world),
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
        world.entity_mut(self.entity).insert(self.cmd.net_id.clone());
        spawn_game_object(self.cmd.spawn_type, self.entity, &self.cmd, world);
        if let Some(parent_net_id) = &self.cmd.parent_net_id
            && let Some(parent) = find_entity_by_net_id(world, parent_net_id)
        {
            world.entity_mut(parent).add_child(self.entity);
        }
    }
}
