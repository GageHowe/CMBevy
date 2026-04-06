use super::{
    Projectile, ProjectileState, insert_generic_remote_projectile, make_generic_projectile_physics,
    tick_projectiles,
};
use crate::GameObject;
use crate::health::Health;
use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;
use rapier3d::prelude::Group;

pub const SPEED: f32 = 600.0;
pub const DAMAGE: f32 = 25.0;
pub const LIFETIME: u32 = 120; // 2 seconds at 60 Hz
const RADIUS: f32 = 0.03;
const HIT_SPEED: f32 = 1.5;
const RECOIL_SPEED: f32 = 0.4;

#[derive(Component, Reflect)]
pub struct RifleProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}
impl Default for RifleProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            lifetime: LIFETIME,
        }
    }
}

pub fn weapon_recoil_impulse(mass: f32) -> f32 {
    mass * RECOIL_SPEED
}

impl Projectile for RifleProjectile {
    const KIND: GameObjectKind = GameObjectKind::RifleProjectile;
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
        let dir = vel.normalize();
        let Some((hit, toi)) = world.cast_ray(prev, dir, step, &exclude) else {
            return;
        };
        let hit_point = prev + dir * toi;
        commands.entity(entity).despawn();
        if let Some(&rb_handle) = world.entity_to_handle.get(&hit) {
            if let Some(rb) = world.rigid_body_set.get(rb_handle) {
                let impulse = dir * HIT_SPEED * rb.mass();
                world.apply_game_impulse_at(hit, impulse, Some(hit_point), None, None);
            }
        }
        if let Ok(mut health) = health_q.get_mut(hit) {
            health.apply_damage(DAMAGE);
        }
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        let Some(&rb_handle) = world.entity_to_handle.get(&shooter) else {
            return;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            return;
        };
        let impulse = -dir * weapon_recoil_impulse(rb.mass());
        world.apply_game_impulse(shooter, impulse, None, None);
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn(origin, velocity, commands, world, shooter, temp_id)
    }
}

/// Spawns a rifle projectile (local prediction on client, authoritative on server).
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
            GameObjectKind::RifleProjectile,
            RifleProjectile {
                shooter,
                lifetime: LIFETIME,
            },
            ProjectileState { temp_id },
            Transform::from_translation(origin),
        ))
        .id();
    // no solver contacts here because projectile hit detection is manual via cast_ray.
    let rb_handle =
        make_generic_projectile_physics(entity, origin, velocity, RADIUS, Group::NONE, world);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    entity
}

/// Spawns a rifle projectile when a SpawnCommand arrives (other clients receiving server broadcast).
/// starting_velocity already includes the shooter's velocity, computed server-side.
impl GameObject for RifleProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        insert_generic_remote_projectile(
            entity,
            cmd,
            world,
            RifleProjectile {
                shooter: None,
                lifetime: LIFETIME,
            },
            RADIUS,
            "event:/Weapons/RifleShot",
        );
    }
}

pub struct RifleProjectilePlugin;
impl Plugin for RifleProjectilePlugin {
    fn build(&self, app: &mut App) {
        use common::game_state::GameState;
        // run on the server (no GameState resource) and in singleplayer; skip on multiplayer client
        app.add_systems(
            FixedUpdate,
            tick_projectiles::<RifleProjectile>
                .after(step_physics)
                .run_if(|state: Option<Res<State<GameState>>>| {
                    state.map_or(true, |s| *s.get() == GameState::SinglePlayer)
                }),
        );
        #[cfg(feature = "client")]
        app.add_systems(bevy::prelude::Update, add_visual);
    }
}

/// Adds a visible mesh to newly spawned rifle projectile entities (client-only).
#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<RifleProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.04));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.2),
            emissive: LinearRgba::new(6.0, 5.0, 0.5, 1.0),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
