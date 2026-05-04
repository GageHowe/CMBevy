use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;

use super::{Projectile, helpers, tick_projectiles};
use crate::{
    GameObject,
    health::{DamageCause, Health, LastDamageSource},
    sound::SoundEmitter,
    spawn::AppGameObjectExt,
};

pub const SPEED: f32 = 500.0;
pub const DAMAGE: f32 = 100.0;
pub const LIFETIME: u32 = 300; // ticks
const RADIUS: f32 = 0.05;

#[derive(Component, Reflect)]
pub struct HailMaryProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}
impl Default for HailMaryProjectile {
    fn default() -> Self {
        Self { shooter: None, lifetime: LIFETIME }
    }
}

impl Projectile for HailMaryProjectile {
    const KIND: GameObjectKind = GameObjectKind::HailMaryProjectile;
    const SPEED: f32 = SPEED;
    const KNOCKBACK: f32 = 1.5;
    const DAMAGE_CAUSE: DamageCause = DamageCause::Sniper;

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
        let impulse = helpers::knockback_impulse::<Self>(dir, 1.0);
        world.apply_game_impulse(shooter, impulse, None, None);
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

/// Spawns a Hail Mary projectile (local prediction on client, authoritative on server).
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
    let entity = helpers::spawn_projectile(
        GameObjectKind::HailMaryProjectile,
        (
            HailMaryProjectile { shooter, lifetime: LIFETIME },
            SoundEmitter { event: "event:/Weapons/SniperProjectileSound" },
        ),
        origin,
        velocity,
        shooter_velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    );
    helpers::queue_world_fire_sound(commands, shooter, "event:/Weapons/SniperShot", origin, velocity);
    // add point light
    let light = commands
        .spawn((
            PointLight {
                intensity: 8000.0,
                range: 50.0,
                color: Color::srgb(1.0, 0.0, 0.0),
                shadows_enabled: true,
                ..default()
            },
            Transform::default(),
        ))
        .id();
    commands.entity(entity).add_child(light);
    entity
}

/// Spawns a Hail Mary projectile when a SpawnCommand arrives (other clients receiving server broadcast).
/// starting_velocity already includes the shooter's velocity, computed server-side.
impl GameObject for HailMaryProjectile {
    const KIND: GameObjectKind = GameObjectKind::HailMaryProjectile;

    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            (
                HailMaryProjectile { shooter: None, lifetime: LIFETIME },
                SoundEmitter { event: "event:/Weapons/SniperProjectileSound" },
            ),
            RADIUS,
            "event:/Weapons/SniperShot",
        );
        let light = world
            .spawn((
                PointLight {
                    intensity: 8000.0,
                    range: 50.0,
                    color: Color::srgb(1.0, 0.0, 0.0),
                    shadows_enabled: true,
                    ..default()
                },
                Transform::default(),
            ))
            .id();
        world.entity_mut(entity).add_child(light);
    }
}

pub struct HailMaryProjectilePlugin;
impl Plugin for HailMaryProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<HailMaryProjectile>().add_systems(
            FixedUpdate,
            tick_projectiles::<HailMaryProjectile>
                .after(step_physics)
                .in_set(super::AuthoritySet::Projectile),
        );
        #[cfg(feature = "client")]
        app.add_systems(bevy::prelude::Update, add_visual);
    }
}

/// Adds a visible sphere mesh to newly spawned Hail Mary projectile entities (client-only).
#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<HailMaryProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.08));
        let mat = materials.add(StandardMaterial {
            // base_color: Color::srgb(1.0, 0.5, 0.0),
            emissive: LinearRgba::new(4.0, 3.0, 0.0, 1.0) * 3.0,
            unlit: true,
            ..default()
        });
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
