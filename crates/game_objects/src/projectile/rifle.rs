use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;

use super::{Projectile, helpers, tick_projectiles};
use crate::{
    GameObject,
    health::{Health, LastDamageSource},
};

pub const SPEED: f32 = 600.0;
pub const DAMAGE: f32 = 25.0;
pub const PISTOL_DAMAGE: f32 = 15.0;
pub const LIFETIME: u32 = 120; // 2 seconds at 60 Hz
const RADIUS: f32 = 0.03;

#[derive(Component, Reflect)]
pub struct RifleProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}

#[derive(Component, Reflect)]
pub struct PistolProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}

impl Default for RifleProjectile {
    fn default() -> Self {
        Self { shooter: None, lifetime: LIFETIME }
    }
}

impl Default for PistolProjectile {
    fn default() -> Self {
        Self { shooter: None, lifetime: LIFETIME }
    }
}

impl Projectile for RifleProjectile {
    const KIND: GameObjectKind = GameObjectKind::RifleProjectile;
    const SPEED: f32 = SPEED;
    const KNOCKBACK: f32 = 0.1;

    fn tick(
        &mut self,
        entity: Entity,
        state: &mut super::ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
    ) {
        let Some(hit) = helpers::tick_raycast_projectile(
            &mut self.lifetime,
            self.shooter,
            entity,
            state,
            body,
            world,
            commands,
        ) else {
            return;
        };
        helpers::apply_raycast_hit::<Self>(
            hit,
            self.shooter,
            world,
            health_q,
            last_damage_q,
            DAMAGE,
        );
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        world.apply_game_impulse(shooter, helpers::knockback_impulse::<Self>(dir, 1.0), None, None);
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        shooter_velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        _weapon: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn(origin, velocity, shooter_velocity, commands, world, shooter, temp_id)
    }
}

impl Projectile for PistolProjectile {
    const KIND: GameObjectKind = GameObjectKind::PistolProjectile;
    const SPEED: f32 = SPEED;
    const KNOCKBACK: f32 = 1.5;

    fn tick(
        &mut self,
        entity: Entity,
        state: &mut super::ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
    ) {
        let Some(hit) = helpers::tick_raycast_projectile(
            &mut self.lifetime,
            self.shooter,
            entity,
            state,
            body,
            world,
            commands,
        ) else {
            return;
        };
        helpers::apply_raycast_hit::<Self>(
            hit,
            self.shooter,
            world,
            health_q,
            last_damage_q,
            PISTOL_DAMAGE,
        );
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        world.apply_game_impulse(shooter, helpers::knockback_impulse::<Self>(dir, 1.0), None, None);
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        shooter_velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        _weapon: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn_pistol(origin, velocity, shooter_velocity, commands, world, shooter, temp_id)
    }
}

/// Spawns a rifle projectile (local prediction on client, authoritative on server).
/// velocity = pre-computed velocity (SPEED * dir + shooter_vel).
pub fn spawn(
    origin: Vec3,
    velocity: Vec3,
    shooter_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    temp_id: u32,
) -> Entity {
    helpers::spawn_projectile(
        GameObjectKind::RifleProjectile,
        RifleProjectile { shooter, lifetime: LIFETIME },
        origin,
        velocity,
        shooter_velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    )
}

pub fn spawn_pistol(
    origin: Vec3,
    velocity: Vec3,
    shooter_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    temp_id: u32,
) -> Entity {
    helpers::spawn_projectile(
        GameObjectKind::PistolProjectile,
        PistolProjectile { shooter, lifetime: LIFETIME },
        origin,
        velocity,
        shooter_velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    )
}

/// Spawns a rifle projectile when a SpawnCommand arrives (other clients receiving server broadcast).
/// starting_velocity already includes the shooter's velocity, computed server-side.
impl GameObject for RifleProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            RifleProjectile { shooter: None, lifetime: LIFETIME },
            RADIUS,
            "event:/Weapons/RifleShot",
        );
    }
}

impl GameObject for PistolProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            PistolProjectile { shooter: None, lifetime: LIFETIME },
            RADIUS,
            "event:/Weapons/RifleShot",
        );
    }
}

pub struct RifleProjectilePlugin;
impl Plugin for RifleProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (tick_projectiles::<RifleProjectile>, tick_projectiles::<PistolProjectile>)
                .after(step_physics)
                .in_set(super::ProjectileAuthoritySet),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            bevy::prelude::Update,
            (add_visual::<RifleProjectile>, add_visual::<PistolProjectile>),
        );
    }
}

/// Adds a visible mesh to newly spawned rifle-style projectile entities (client-only).
#[cfg(feature = "client")]
fn add_visual<P: Component>(
    q: Query<Entity, Added<P>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.04));
        let mat = materials.add(StandardMaterial {
            emissive: LinearRgba::new(6.0, 5.0, 0.5, 1.0) * 3.0,
            unlit: true,
            ..default()
        });
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
