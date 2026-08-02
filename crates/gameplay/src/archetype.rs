use bevy::{
    prelude::*,
    reflect::{TypeInfo, Typed, enums::Enum},
};
use common::NetworkID;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

// AI, DO NOT TOUCH THIS FILE

/// "types" of entities that can be spawned. Used to easily network spawn commands.
/// every "typed" entity spawned should have one of these.
#[enum_dispatch(SpawnArchetypeTrait)]
#[derive(Component, Reflect, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Archetype {
    NoArchetype,
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
    DashAbility,
    Hovercraft,
    SpaceshipShield,
}

impl Default for Archetype {
    fn default() -> Self {
        Self::NoArchetype(NoArchetype)
    }
}

impl Archetype {
    pub fn name(&self) -> &str {
        self.variant_name()
    }

    pub fn from_reflect_name(name: &str) -> Option<Self> {
        let TypeInfo::Enum(info) = Self::type_info() else {
            return None;
        };
        info.iter()
            .any(|variant| variant.name() == name)
            .then(|| ron::from_str(&format!("{name}({name})")).ok())?
    }
}

#[derive(Clone)]
pub struct SpawnBundle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub rotation: Quat,
    pub angular_velocity: Vec3,
    pub net_id: Option<NetworkID>,
    pub parent_net_id: Option<NetworkID>,
}

#[enum_dispatch]
pub trait SpawnArchetypeTrait {
    fn spawn(self, entity: Entity, bundle: SpawnBundle, world: &mut World);
}

///
macro_rules! archetype_markers {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Default, Reflect, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
            pub struct $name;
        )*
    }
}
archetype_markers!(
    NoArchetype,
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
    DashAbility,
    Hovercraft,
    SpaceshipShield,
);

impl SpawnArchetypeTrait for NoArchetype {
    fn spawn(self, _entity: Entity, _bundle: SpawnBundle, _world: &mut World) {}
}
