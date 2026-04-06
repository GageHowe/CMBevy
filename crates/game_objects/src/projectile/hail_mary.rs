use super::{
    Projectile, ProjectileState, insert_generic_remote_projectile, make_generic_projectile_physics,
    tick_projectiles,
};
use crate::GameObject;
use crate::health::Health;
use crate::sound::SoundEmitter;
use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;
use rapier3d::prelude::*;

pub const SPEED: f32 = 600.0;
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
        Self {
            shooter: None,
            lifetime: LIFETIME,
        }
    }
}

impl Projectile for HailMaryProjectile {
    const KIND: GameObjectKind = GameObjectKind::HailMaryProjectile;
    const SPEED: f32 = SPEED;

    fn tick(
        &mut self,
        entity: Entity,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
    ) {
        self.lifetime = self.lifetime.saturating_sub(1);
        if self.lifetime == 0 {
            commands.entity(entity).despawn();
            return;
        }
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            return;
        };
        let vel = rb_vel(rb);
        let dt = world.integration_parameters.dt;
        let step = vel.length() * dt;
        if step < 0.001 {
            return;
        }
        let curr = rb_pos(rb);
        let prev = curr - vel * dt;
        // exclude both self and shooter so the ray isn't blocked by the shooter's capsule on spawn
        let exclude = [entity, self.shooter.unwrap_or(entity)];
        let Some((hit, _)) = world.cast_ray(prev, vel.normalize(), step, &exclude) else {
            return;
        };
        commands.entity(entity).despawn();
        if let Ok(mut health) = health_q.get_mut(hit) {
            health.apply_damage(DAMAGE);
        }
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        _weapon: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn(origin, velocity, commands, world, shooter, temp_id)
    }
}

/// Spawns a Hail Mary projectile (local prediction on client, authoritative on server).
/// velocity = pre-computed velocity (SPEED * dir + shooter_vel).
pub fn spawn(
    origin: Vec3,
    velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    temp_id: u32,
) -> Entity {
    let entity = commands
        .spawn((
            GameObjectKind::HailMaryProjectile,
            HailMaryProjectile {
                shooter,
                lifetime: LIFETIME,
            },
            ProjectileState { temp_id },
            Transform::from_translation(origin),
            SoundEmitter {
                event: "event:/Weapons/SniperProjectileSound",
            },
        ))
        .id();
    // no solver contacts here because projectile hit detection is manual via cast_ray.
    let rb_handle =
        make_generic_projectile_physics(entity, origin, velocity, RADIUS, Group::NONE, world);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
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
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        insert_generic_remote_projectile(
            entity,
            cmd,
            world,
            (
                HailMaryProjectile {
                    shooter: None,
                    lifetime: LIFETIME,
                },
                SoundEmitter {
                    event: "event:/Weapons/SniperProjectileSound",
                },
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
        use common::game_state::GameState;
        // run on the server (no GameState resource) and in singleplayer; skip on multiplayer client
        app.add_systems(
            FixedUpdate,
            tick_projectiles::<HailMaryProjectile>
                .after(step_physics)
                .run_if(|state: Option<Res<State<GameState>>>| {
                    state.map_or(true, |s| *s.get() == GameState::SinglePlayer)
                }),
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
            base_color: Color::srgb(1.0, 0.5, 0.0),
            emissive: LinearRgba::new(4.0, 2.0, 0.0, 1.0),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
