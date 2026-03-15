use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::net::message::SpawnCommand;
use crate::physics::physics_world::*;
use super::weapon::{insert_weapon_physics, WeaponComponent, WeaponInput, WeaponState};

pub const RANGE: f32 = 25.0;
/// Total damage split evenly across all pellets on a direct hit.
pub const DAMAGE: f32 = 80.0;
/// Seconds between shots (1 shot/sec).
pub const COOLDOWN: f32 = 1.0;
/// Number of pellets fired per shot (client-side spread prediction only).
pub const PELLETS: usize = 8;
/// Half-angle spread in radians per pellet offset.
pub const SPREAD: f32 = 0.08;

/// Marker component for the shotgun weapon type.
#[derive(Component, Default)]
pub struct ShotgunComponent;

/// Spawns a shotgun entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        ShotgunComponent,
        WeaponInput::default(),
        WeaponState::new(RANGE, DAMAGE, COOLDOWN),
        GameObjectKind::Shotgun,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    insert_weapon_physics(entity, &transform, commands, world);
    entity
}

/// Spawns a shotgun from a network SpawnCommand. Handles physics, visuals, and net_id insertion.
/// Client-only: requires AssetServer for the GLB model.
pub fn spawn_from_command(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
) -> Entity {
    let transform = Transform { translation: cmd.position, rotation: cmd.rotation, ..default() };
    let entity = spawn(transform, commands, world);
    add_visuals(entity, commands, asset_server);
    commands.entity(entity).insert(cmd.net_id);
    entity
}

/// Adds a scene (GLB model) to an existing shotgun entity.
pub fn add_visuals(
    entity: Entity,
    commands: &mut Commands,
    asset_server: &bevy::asset::AssetServer,
) {
    commands.entity(entity).insert((
        SceneRoot(asset_server.load("models/shotgun.glb#Scene0")),
        Visibility::default(),
    ));
}
