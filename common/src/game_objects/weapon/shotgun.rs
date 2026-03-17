use bevy::prelude::*;
use crate::game_objects::GameObjectKind;
use crate::interaction::Interactable;
use crate::net::message::SpawnCommand;
use crate::physics::physics_world::*;
use crate::physics::convex_hull_asset::ConvexHullAsset;
use rapier3d::prelude::*;
use super::weapon::{Weapon, WeaponComponent};

pub const RANGE: f32 = 25.0;
/// Total damage split evenly across all pellets on a direct hit.
pub const DAMAGE: f32 = 80.0;
/// Ticks between shots (1 shot/sec at 60 Hz).
pub const COOLDOWN_TICKS: u32 = 60;
/// Number of pellets fired per shot (client-side spread prediction only).
pub const PELLETS: usize = 8;
/// Half-angle spread in radians per pellet offset.
pub const SPREAD: f32 = 0.08;

/// Per-instance state for the shotgun weapon type.
#[derive(Component, Default)]
pub struct ShotgunComponent {
    pub cooldown: u32,
    /// Latched when fire is requested; cleared after the shot fires.
    pub fire_requested: bool,
}

impl Weapon for ShotgunComponent {
    fn update(&mut self, _world: &mut PhysicsWorld, _commands: &mut Commands, _origin: Vec3, _aim_dir: Vec3, _shooter: Option<Entity>, _tick: u64, want_fire: bool) -> bool {
        if want_fire { self.fire_requested = true; }
        self.cooldown = self.cooldown.saturating_sub(1);
        if !self.fire_requested || self.cooldown > 0 { return false; }
        self.cooldown = COOLDOWN_TICKS;
        self.fire_requested = false;
        true
    }
}

/// Spawns a shotgun entity with physics. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        WeaponComponent,
        ShotgunComponent::default(),
        GameObjectKind::Shotgun,
        Interactable { range: 2.0 },
        Transform::from(transform),
    )).id();
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(2.0)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(ColliderBuilder::cuboid(0.2, 0.05, 0.4).build(), rb_handle, rigid_body_set);
    entity
}

pub fn spawn_from_command(
    cmd: SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    asset_server: &AssetServer,
    _hull_assets: &Assets<ConvexHullAsset>,
) -> Entity {
    let transform = Transform { translation: cmd.position, rotation: cmd.rotation, scale: Vec3::ONE };
    let entity = spawn(transform, commands, world);
    commands.entity(entity).insert((SceneRoot(asset_server.load("models/shotgun.glb#Scene0")), Visibility::default(), cmd.net_id));
    entity
}
