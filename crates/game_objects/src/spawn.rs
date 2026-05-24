//! Central spawn dispatch for replicated game object kinds.

use bevy::prelude::{Command, *};
use common::GameObjectKind;
use net::message::SpawnCommand;

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

pub fn dispatch_game_object_on_death(kind: GameObjectKind, entity: Entity, world: &mut World) {
    match kind {
        GameObjectKind::Biped => crate::pawn::biped::on_biped_death(entity, world),
        GameObjectKind::Spaceship => crate::pawn::spaceship::on_spaceship_death(entity, world),
        GameObjectKind::Fighter => crate::pawn::fighter::on_fighter_death(entity, world),
        GameObjectKind::Truck => crate::pawn::truck::on_truck_death(entity, world),
        GameObjectKind::Hovercraft => crate::pawn::hovercraft::on_hovercraft_death(entity, world),
        _ => {}
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

fn spawn_game_object(kind: GameObjectKind, entity: Entity, cmd: &SpawnCommand, world: &mut World) {
    match kind {
        GameObjectKind::Biped => crate::pawn::biped::spawn_biped(entity, cmd, world),
        GameObjectKind::Spaceship => crate::pawn::spaceship::spawn_spaceship(entity, cmd, world),
        GameObjectKind::SpaceshipShield => {
            crate::shield::spawn_spaceship_shield(entity, cmd, world)
        }
        GameObjectKind::Fighter => crate::pawn::fighter::spawn_fighter(entity, cmd, world),
        GameObjectKind::Truck => crate::pawn::truck::spawn_truck(entity, cmd, world),
        GameObjectKind::Hovercraft => crate::pawn::hovercraft::spawn_hovercraft(entity, cmd, world),
        GameObjectKind::Planet | GameObjectKind::Shotgun => {
            panic!("GameObjectKind::{kind:?} has no spawn implementation");
        }
        GameObjectKind::Pistol => crate::weapon::pistol::spawn_pistol(entity, cmd, world),
        GameObjectKind::Beamer => crate::weapon::beamer::spawn_beamer(entity, cmd, world),
        GameObjectKind::Rifle => crate::weapon::rifle::spawn_rifle(entity, cmd, world),
        GameObjectKind::Smg => crate::weapon::smg::spawn_smg(entity, cmd, world),
        GameObjectKind::Failsafe => crate::weapon::failsafe::spawn_failsafe(entity, cmd, world),
        GameObjectKind::HailMary => crate::weapon::hail_mary::spawn_hail_mary(entity, cmd, world),
        GameObjectKind::Thumper => crate::weapon::thumper::spawn_thumper(entity, cmd, world),
        GameObjectKind::Lobber => crate::weapon::lobber::spawn_lobber(entity, cmd, world),
        GameObjectKind::CoilLauncher => {
            crate::weapon::coil_launcher::spawn_coil_launcher(entity, cmd, world)
        }
        GameObjectKind::HailMaryProjectile => {
            crate::projectile::hail_mary::spawn_remote_hail_mary(entity, cmd, world)
        }
        GameObjectKind::ThumperProjectile => {
            crate::projectile::thumper::spawn_remote_thumper(entity, cmd, world)
        }
        GameObjectKind::FailsafeProjectile => {
            crate::projectile::failsafe::spawn_remote_failsafe(entity, cmd, world)
        }
        GameObjectKind::PistolProjectile => {
            crate::projectile::pistol::spawn_remote_pistol(entity, cmd, world)
        }
        GameObjectKind::RifleProjectile => {
            crate::projectile::rifle::spawn_remote_rifle(entity, cmd, world)
        }
        GameObjectKind::LobberProjectile => {
            crate::projectile::lobber::spawn_remote_lobber(entity, cmd, world)
        }
        GameObjectKind::CoilLauncherProjectile => {
            crate::projectile::coil_launcher::spawn_remote_coil_launcher(entity, cmd, world)
        }
        GameObjectKind::FighterRocketProjectile => {
            crate::projectile::fighter_rocket::spawn_remote_fighter_rocket(entity, cmd, world)
        }
        GameObjectKind::Jetpack => {
            crate::pawn::biped_ability::spawn_jetpack_pickup(entity, cmd, world)
        }
        GameObjectKind::Dash => crate::pawn::biped_ability::spawn_dash_pickup(entity, cmd, world),
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
        world
            .entity_mut(self.entity)
            .insert((self.cmd.net_id.clone(), self.cmd.kind.clone()));
        spawn_game_object(self.cmd.kind.clone(), self.entity, &self.cmd, world);
        if let Some(parent_net_id) = &self.cmd.parent_net_id
            && let Some(parent) = find_entity_by_net_id(world, parent_net_id)
        {
            world.entity_mut(parent).add_child(self.entity);
        }
    }
}
