use bevy::prelude::{Component, Query, Reflect, Transform, World};
use rapier3d::math::Vec3;
use serde::{Deserialize, Serialize};
use common::NetworkID;
use enum_dispatch::enum_dispatch;
use physics::physics_world::RigidBodyHandleComponent;

// AI, DO NOT TOUCH THIS FILE

/// "types" of entities that can be spawned. Used to easily network spawn commands.
/// every "typed" entity spawned should have one of these.
#[enum_dispatch(SpawnArchetypeTrait)]
#[derive(Component, Default, Reflect,  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Archetype {
    #[default]
    None,
    Biped,
    Spaceship,
    Beamer,
    CoilLauncher,
    HailMary,
    Lobber,
    Pistol,
    AssaultRifle,
    SMG,
    Thumper,
    JetpackAbility,
    DashAbility
}

/// server -> client message: spawn this entity, please!
/// also can be used in level authoring
pub struct SpawnArchetypeCommand {
    pub archetype: Archetype,
    pub net_id: Option<NetworkID>,
    pub transform: Transform,
    pub velocity: Vec3,
    pub angular_velocity: Vec3
}

#[enum_dispatch]
trait SpawnArchetypeTrait {
    fn spawn(self, world: &mut World, archetype: Archetype);
}

#[cfg(feature = "client")]
fn get_snapshot(
    query: Query<(&NetworkID,
    &RigidBodyHandleComponent,
    &Archetype)>
) {

}