use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;
use net::message::SpawnCommand;
use crate::health::Health;
use crate::sound::SoundEmitter;
use crate::GameObject;
use super::{Projectile, ProjectileState, tick_projectiles};
use common::GameObjectKind;

pub const SPEED:    f32 = 600.0;
pub const DAMAGE:   f32 = 100.0;
pub const LIFETIME: u32 = 300; // ticks
const RADIUS: f32 = 0.05;

#[derive(Component, Reflect)]
pub struct HailMaryProjectile {
    pub shooter:  Option<Entity>,
    pub lifetime: u32,
}
impl Default for HailMaryProjectile {
    fn default() -> Self { Self { shooter: None, lifetime: LIFETIME } }
}

impl Projectile for HailMaryProjectile {
    const KIND: GameObjectKind = GameObjectKind::HailMaryProjectile;

    fn tick(&mut self, entity: Entity, body: &RigidBodyHandleComponent, world: &PhysicsWorld, commands: &mut Commands, health_q: &mut Query<&mut Health>) {
        self.lifetime = self.lifetime.saturating_sub(1);
        if self.lifetime == 0 { commands.entity(entity).despawn(); return; }
        let Some(rb) = world.rigid_body_set.get(body.0) else { return };
        let vel = rb_vel(rb);
        let dt = world.integration_parameters.dt;
        let step = vel.length() * dt;
        if step < 0.001 { return; }
        let curr = rb_pos(rb);
        let prev = curr - vel * dt;
        let Some((hit, _)) = world.cast_ray(prev, vel.normalize(), step, Some(entity)) else { return };
        if Some(hit) != self.shooter {
            commands.entity(entity).despawn();
            if let Ok(mut health) = health_q.get_mut(hit) { health.apply_damage(DAMAGE); }
        }
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
    let entity = commands.spawn((
        GameObjectKind::HailMaryProjectile,
        HailMaryProjectile { shooter, lifetime: LIFETIME },
        ProjectileState { temp_id },
        Transform::from_translation(origin),
        SoundEmitter { event: "event:/Weapons/SniperProjectileSound" },
    )).id();
    let rb_handle = world.insert_body(entity, RigidBodyBuilder::kinematic_velocity_based()
        .translation(origin)
        .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
        .ccd_enabled(true)
        .build());
    {
        let proj_collision = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL, InteractionTestMode::And);
        let proj_solver    = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        collider_set.insert_with_parent(
            ColliderBuilder::ball(RADIUS).collision_groups(proj_collision).solver_groups(proj_solver).build(),
            rb_handle, rigid_body_set,
        );
    }
    commands.entity(entity).insert(RigidBodyHandleComponent(rb_handle));
    // add point light
    let light = commands.spawn((
        PointLight { intensity: 8000.0, range: 50.0, color: Color::srgb(1.0, 0.0, 0.0), shadows_enabled: true, ..default() },
        Transform::default(),
    )).id();
    commands.entity(entity).add_child(light);
    entity
}

/// Spawns a Hail Mary projectile when a SpawnCommand arrives (other clients receiving server broadcast).
/// starting_velocity already includes the shooter's velocity, computed server-side.
impl GameObject for HailMaryProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        world.entity_mut(entity).insert((
            GameObjectKind::HailMaryProjectile,
            HailMaryProjectile { shooter: None, lifetime: LIFETIME },
            ProjectileState { temp_id: 0 },
            Transform::from_translation(cmd.position),
            SoundEmitter { event: "event:/Weapons/SniperProjectileSound" },
            cmd.net_id.clone(),
        ));
        let vel = cmd.starting_velocity;
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb_handle = physics.insert_body(entity, RigidBodyBuilder::kinematic_velocity_based()
                .translation(cmd.position)
                .linvel(Vector::new(vel.x, vel.y, vel.z))
                .ccd_enabled(true)
                .build());
            let proj_collision = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL, InteractionTestMode::And);
            let proj_solver    = InteractionGroups::new(GROUP_PROJECTILE, Group::ALL & !GROUP_PLAYER, InteractionTestMode::And);
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(
                ColliderBuilder::ball(RADIUS).collision_groups(proj_collision).solver_groups(proj_solver).build(),
                rb_handle, rigid_body_set,
            );
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        let light = world.spawn((
            PointLight { intensity: 8000.0, range: 50.0, color: Color::srgb(1.0, 0.0, 0.0), shadows_enabled: true, ..default() },
            Transform::default(),
        )).id();
        world.entity_mut(entity).add_child(light);
        // play spatial fire sound for the shooter we're watching (we are not the shooter)
        if let Some(mut sq) = world.get_resource_mut::<crate::sound::SoundQueue>() {
            sq.0.push(crate::sound::SoundRequest { event: "event:/Weapons/SniperShot", position: Some(cmd.position), velocity: Vec3::ZERO });
        }
    }
}

pub struct HailMaryProjectilePlugin;
impl Plugin for HailMaryProjectilePlugin {
    fn build(&self, app: &mut App) {
        // both systems are server-only; client registers them for singleplayer in main.rs
        #[cfg(not(feature = "client"))]
        app.add_systems(FixedUpdate, tick_projectiles::<HailMaryProjectile>.after(step_physics));
        #[cfg(feature = "client")]
        {
            use common::game_state::GameState;
            app.add_systems(FixedUpdate, tick_projectiles::<HailMaryProjectile>
                .after(step_physics).run_if(bevy::prelude::in_state(GameState::SinglePlayer)));
            app.add_systems(bevy::prelude::Update, add_visual);
        }
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
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
