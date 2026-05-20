//! Central spawn dispatch for the reflected game object types shared across the game.

use bevy::prelude::{App, Command, Resource, *};
use common::GameObjectKind;
use net::message::SpawnCommand;

use crate::gc::WorldObjectGc;

type SpawnGameObjectFn = fn(Entity, &SpawnCommand, &mut World);
type GameObjectDeathFn = fn(Entity, &mut World) -> bool;

#[derive(Clone, Copy)]
struct GameObjectRegistration {
    spawn: SpawnGameObjectFn,
    on_death: GameObjectDeathFn,
    gc_lifetime_secs: Option<f32>,
    collision_sound: Option<&'static str>,
    splash_damage_uses_center_of_mass: bool,
}

impl GameObjectRegistration {
    fn of<T: GameObject>() -> Self {
        Self {
            spawn: T::spawn,
            on_death: T::on_death,
            gc_lifetime_secs: T::gc_lifetime_secs(),
            collision_sound: T::COLLISION_SOUND,
            splash_damage_uses_center_of_mass: T::SPLASH_DAMAGE_USES_CENTER_OF_MASS,
        }
    }
}

#[derive(Component)]
pub struct CenterOfMassSplashDamage;

#[derive(Resource, Default)]
pub struct GameObjectRegistry(Vec<(GameObjectKind, GameObjectRegistration)>);

impl GameObjectRegistry {
    pub fn register<T: GameObject>(&mut self) {
        if self.0.iter().any(|(kind, _)| *kind == T::KIND) {
            panic!("GameObjectKind::{:?} registered more than once", T::KIND);
        }
        self.0.push((T::KIND, GameObjectRegistration::of::<T>()));
    }

    fn get(&self, kind: GameObjectKind) -> GameObjectRegistration {
        *self
            .0
            .iter()
            .find(|(registered_kind, _)| *registered_kind == kind)
            .map(|(_, registration)| registration)
            .unwrap_or_else(|| panic!("GameObjectKind::{kind:?} is not registered"))
    }

    pub fn collision_sound(&self, kind: GameObjectKind) -> Option<&'static str> {
        self.get(kind).collision_sound
    }
}

pub trait AppGameObjectExt {
    fn register_game_object<T: GameObject>(&mut self) -> &mut Self;
}

impl AppGameObjectExt for App {
    fn register_game_object<T: GameObject>(&mut self) -> &mut Self {
        self.init_resource::<GameObjectRegistry>();
        self.world_mut()
            .resource_mut::<GameObjectRegistry>()
            .register::<T>();
        self
    }
}

pub trait GameObject: Default + Reflect {
    const KIND: GameObjectKind;
    const GC_LIFETIME_SECS: Option<f32> = None;
    const SPLASH_DAMAGE_USES_CENTER_OF_MASS: bool = true;

    /// the sound this plays when colliing with things.
    const COLLISION_SOUND: Option<&'static str> = None;
    /// responsible for enacting all side effects that spawn this entity.
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World);
    /// callback that is called when entities' health drops to 0, before they are despawned.
    /// responsible for particle effects, debris, cleanup, etc.
    fn on_death(_entity: Entity, _world: &mut World) -> bool {
        true
    }

    fn gc_lifetime_secs() -> Option<f32> {
        Self::GC_LIFETIME_SECS
    }
}

pub fn dispatch_game_object_on_death(
    kind: GameObjectKind,
    entity: Entity,
    world: &mut World,
) -> bool {
    let registration = {
        let registry = world.resource::<GameObjectRegistry>();
        registry.get(kind)
    };
    (registration.on_death)(entity, world)
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
        let registration = {
            let registry = world.resource::<GameObjectRegistry>();
            registry.get(self.cmd.kind.clone())
        };
        (registration.spawn)(self.entity, &self.cmd, world);
        if registration.splash_damage_uses_center_of_mass {
            world
                .entity_mut(self.entity)
                .insert(CenterOfMassSplashDamage);
        } else {
            world
                .entity_mut(self.entity)
                .remove::<CenterOfMassSplashDamage>();
        }
        if let Some(reset_secs) = registration.gc_lifetime_secs
            && world.get::<WorldObjectGc>(self.entity).is_none()
        {
            world
                .entity_mut(self.entity)
                .insert(WorldObjectGc::new(reset_secs));
        }
    }
}
