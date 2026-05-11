use bevy::prelude::*;
use common::GameObjectKind;
#[cfg(feature = "client")]
use common::game_state::GameState;
use net::message::{NetworkID, SpawnCommand};
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{FiredProjectile, Projectile, ProjectileState, helpers, tick_projectiles};
use crate::{
    GameObject,
    health::{Health, LastDamageSource},
    spawn::AppGameObjectExt,
};

pub const SPEED: f32 = 42.0;
pub const LIFETIME: u32 = 320;
pub const DAMAGE: f32 = 110.0;
pub const EXPLOSION_RADIUS: f32 = 10.0;
pub const EXPLOSION_IMPULSE: f32 = 30.0;
pub const EXPLOSION_IMPULSE_MAX_EFFECTIVE_MASS: f32 = 1000.0;
const RADIUS: f32 = 0.25;
const GRAVITY_SCALE: f32 = 1.0;
const RESTITUTION: f32 = 0.8;
const FRICTION: f32 = 0.6;
const SELF_DAMAGE_SCALE: f32 = 0.5;
#[cfg(feature = "client")]
pub const EXPLOSION_SHAKE_RADIUS: f32 = 30.0;
#[cfg(feature = "client")]
const SHAKE_SCALE: f32 = 1.0;
const CONFIG: helpers::ExplosiveProjectileConfig = helpers::ExplosiveProjectileConfig {
    projectile_radius: RADIUS,
    damage: DAMAGE,
    explosion_radius: EXPLOSION_RADIUS,
    explosion_impulse: EXPLOSION_IMPULSE,
    explosion_impulse_max_effective_mass: EXPLOSION_IMPULSE_MAX_EFFECTIVE_MASS,
    self_damage_scale: SELF_DAMAGE_SCALE,
};

#[derive(Component)]
struct PendingDetonation;

#[derive(Component, Reflect)]
pub struct GrenadeLauncherProjectile {
    pub shooter: Option<Entity>,
    pub weapon: Option<Entity>,
    pub lifetime: u32,
}
impl Default for GrenadeLauncherProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            weapon: None,
            lifetime: LIFETIME,
        }
    }
}

pub fn shooter_knockback(mass: f32) -> f32 {
    mass * <GrenadeLauncherProjectile as Projectile>::SHOOTER_KNOCKBACK
}

impl Projectile for GrenadeLauncherProjectile {
    const KIND: GameObjectKind = GameObjectKind::GrenadeLauncherProjectile;
    const SPEED: f32 = SPEED;
    const SHOOTER_KNOCKBACK: f32 = 3.0;

    fn tick(
        &mut self,
        entity: Entity,
        _state: &mut ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
    ) {
        tick_inner(self, entity, body, world, commands, health_q, last_damage_q);
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        let Some(&rb_handle) = world.entity_to_handle.get(&shooter) else {
            return;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            return;
        };
        let impulse = -dir * shooter_knockback(rb.mass());
        world.apply_game_impulse(shooter, impulse, None, None);
    }

    fn fire_authoritative(
        origin: Vec3,
        dir: Vec3,
        shooter: Entity,
        tick: u64,
        weapon: Entity,
        temp_id: u32,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        net_ids: &mut net::message::NetworkIDResource,
    ) -> Option<FiredProjectile> {
        Self::on_authoritative_fire(dir, shooter, world);
        let velocity = helpers::projectile_velocity(world, Some(shooter), dir, Self::SPEED);
        let shooter_velocity = helpers::shooter_velocity(world, Some(shooter));
        let entity = spawn(
            origin,
            velocity,
            shooter_velocity,
            commands,
            world,
            Some(shooter),
            Some(weapon),
            temp_id,
        );
        let net_id = NetworkID(net_ids.next());
        commands.entity(entity).insert(net_id.clone());
        Some(FiredProjectile {
            net_id: net_id.clone(),
            spawn_cmd: SpawnCommand {
                net_id,
                position: origin,
                starting_velocity: velocity,
                shooter_velocity,
                rotation: Quat::IDENTITY,
                server_tick: tick,
                kind: <Self as Projectile>::KIND,
            },
        })
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        shooter_velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        weapon: Option<Entity>,
        temp_id: u32,
    ) -> Entity {
        spawn(
            origin,
            velocity,
            shooter_velocity,
            commands,
            world,
            shooter,
            weapon,
            temp_id,
        )
    }
}

fn tick_inner(
    projectile: &mut GrenadeLauncherProjectile,
    entity: Entity,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
) {
    projectile.lifetime = projectile.lifetime.saturating_sub(1);
    if projectile.lifetime > 0 {
        return;
    }
    let Some(rb) = world.rigid_body_set.get(body.0) else {
        return;
    };
    explode_at(
        rb_pos(rb),
        entity,
        projectile.shooter,
        world,
        commands,
        health_q,
        last_damage_q,
    );
}

pub fn spawn(
    origin: Vec3,
    velocity: Vec3,
    shooter_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    weapon: Option<Entity>,
    temp_id: u32,
) -> Entity {
    let entity = commands
        .spawn((
            GameObjectKind::GrenadeLauncherProjectile,
            GrenadeLauncherProjectile {
                shooter,
                weapon,
                lifetime: LIFETIME,
            },
            ProjectileState {
                temp_id,
                shooter_velocity,
            },
            Transform::from_translation(origin),
            GravityScale(GRAVITY_SCALE),
        ))
        .id();
    let handle = world.insert_body(
        entity,
        RigidBodyBuilder::dynamic()
            .translation(origin)
            .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
            .ccd_enabled(true)
            .linear_damping(0.05)
            .angular_damping(0.4)
            .can_sleep(false)
            .build(),
    );
    let collider = ColliderBuilder::ball(RADIUS)
        .restitution(RESTITUTION)
        .friction(FRICTION)
        .restitution_combine_rule(CoefficientCombineRule::Max)
        .build();
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(collider, handle, rigid_body_set);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(handle));
    helpers::queue_world_fire_sound(
        commands,
        shooter,
        "event:/Weapons/SniperShot",
        origin,
        velocity,
    );
    entity
}

impl GameObject for GrenadeLauncherProjectile {
    const KIND: GameObjectKind = GameObjectKind::GrenadeLauncherProjectile;

    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        world.entity_mut(entity).insert((
            cmd.kind.clone(),
            GrenadeLauncherProjectile::default(),
            ProjectileState {
                temp_id: 0,
                shooter_velocity: cmd.shooter_velocity,
            },
            Transform::from_translation(cmd.position),
            cmd.net_id.clone(),
            GravityScale(GRAVITY_SCALE),
        ));
        let handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let handle = physics.insert_body(
                entity,
                RigidBodyBuilder::dynamic()
                    .translation(cmd.position)
                    .linvel(Vector::new(
                        cmd.starting_velocity.x,
                        cmd.starting_velocity.y,
                        cmd.starting_velocity.z,
                    ))
                    .ccd_enabled(true)
                    .linear_damping(0.05)
                    .angular_damping(0.4)
                    .can_sleep(false)
                    .build(),
            );
            let collider = ColliderBuilder::ball(RADIUS)
                .restitution(RESTITUTION)
                .friction(FRICTION)
                .restitution_combine_rule(CoefficientCombineRule::Max)
                .build();
            let PhysicsWorld {
                collider_set,
                rigid_body_set,
                ..
            } = &mut *physics;
            collider_set.insert_with_parent(collider, handle, rigid_body_set);
            handle
        };
        world
            .entity_mut(entity)
            .insert(RigidBodyHandleComponent(handle));
    }
}

pub fn detonate_latest_for_weapon(world: &mut World, weapon: Entity) {
    let mut detonate = None;
    {
        let mut q = world.query::<(Entity, &GrenadeLauncherProjectile)>();
        for (entity, projectile) in q.iter(world) {
            if projectile.weapon != Some(weapon) {
                continue;
            }
            if detonate.is_none_or(|(_, lifetime)| projectile.lifetime > lifetime) {
                detonate = Some((entity, projectile.lifetime));
            }
        }
    }
    let Some((entity, _)) = detonate else {
        return;
    };
    world.entity_mut(entity).insert(PendingDetonation);
}

fn explode_at(
    center: Vec3,
    entity: Entity,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
) {
    helpers::explode_sphere_explosive_projectile(
        center,
        None,
        entity,
        shooter,
        world,
        commands,
        health_q,
        last_damage_q,
        None,
        None,
        None,
        &CONFIG,
        #[cfg(feature = "client")]
        EXPLOSION_SHAKE_RADIUS,
        #[cfg(feature = "client")]
        SHAKE_SCALE,
    );
}

pub struct GrenadeLauncherProjectilePlugin;
impl Plugin for GrenadeLauncherProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<GrenadeLauncherProjectile>()
            .add_systems(
                FixedUpdate,
                (
                    tick_projectiles::<GrenadeLauncherProjectile>,
                    detonate_requested_projectiles,
                )
                    .chain()
                    .after(step_physics)
                    .in_set(super::AuthoritySystems),
            );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            tick_predicted_projectiles
                .after(step_physics)
                .run_if(in_state(GameState::Multiplayer)),
        );
        #[cfg(feature = "client")]
        app.add_systems(bevy::prelude::Update, add_visual);
    }
}

fn detonate_requested_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    q: Query<
        (
            Entity,
            &GrenadeLauncherProjectile,
            &RigidBodyHandleComponent,
        ),
        With<PendingDetonation>,
    >,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
) {
    for (entity, projectile, body) in q.iter() {
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            continue;
        };
        explode_at(
            rb_pos(rb),
            entity,
            projectile.shooter,
            &mut world,
            &mut commands,
            &mut health_q,
            &mut last_damage_q,
        );
    }
}

#[cfg(feature = "client")]
fn tick_predicted_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &mut GrenadeLauncherProjectile,
        &RigidBodyHandleComponent,
        &mut ProjectileState,
    )>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
) {
    for (entity, mut projectile, body, state) in q.iter_mut() {
        if state.temp_id == 0 {
            continue;
        }
        tick_inner(
            &mut projectile,
            entity,
            body,
            &mut world,
            &mut commands,
            &mut health_q,
            &mut last_damage_q,
        );
    }
}

#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<GrenadeLauncherProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.28));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.7, 0.2),
            emissive: LinearRgba::new(0.3, 1.2, 0.3, 1.0),
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
