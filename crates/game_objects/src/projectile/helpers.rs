use super::{Projectile, ProjectileState};
use crate::health::{Health, LastDamageSource, attribute_damage};
use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;
use rapier3d::prelude::{Group, RigidBodyBuilder, Vector};

#[derive(Clone, Copy)]
pub struct RayProjectileHit {
    pub entity: Entity,
    pub dir: Vec3,
    pub point: Vec3,
}

pub fn shooter_velocity(world: &PhysicsWorld, shooter: Option<Entity>) -> Vec3 {
    shooter
        .and_then(|e| world.entity_to_handle.get(&e).copied())
        .and_then(|h| world.rigid_body_set.get(h))
        .map(rb_vel)
        .unwrap_or(Vec3::ZERO)
}

pub fn projectile_velocity(
    world: &PhysicsWorld,
    shooter: Option<Entity>,
    aim_dir: Vec3,
    speed: f32,
) -> Vec3 {
    aim_dir * speed + shooter_velocity(world, shooter)
}

pub fn next_temp_id(id_counter: Option<&mut u32>) -> u32 {
    id_counter
        .map(|counter| {
            *counter = counter.wrapping_add(1);
            *counter
        })
        .unwrap_or(0)
}

pub fn make_projectile_physics(
    entity: Entity,
    origin: Vec3,
    velocity: Vec3,
    _radius: f32,
    _solver_memberships: Group,
    world: &mut PhysicsWorld,
) -> RigidBodyHandle {
    world.insert_body(
        entity,
        RigidBodyBuilder::kinematic_velocity_based()
            .translation(origin)
            .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
            .build(),
    )
}

pub fn spawn_projectile(
    kind: GameObjectKind,
    projectile: impl Bundle,
    origin: Vec3,
    velocity: Vec3,
    radius: f32,
    temp_id: u32,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands
        .spawn((
            kind,
            projectile,
            ProjectileState { temp_id },
            Transform::from_translation(origin),
        ))
        .id();
    // Projectile collision is resolved by casts so contacts do not push the shooter.
    let rb_handle = make_projectile_physics(entity, origin, velocity, radius, Group::NONE, world);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    entity
}

pub fn insert_remote_projectile(
    entity: Entity,
    cmd: &SpawnCommand,
    world: &mut World,
    projectile: impl Bundle,
    radius: f32,
    fire_sound: &'static str,
) {
    world.entity_mut(entity).insert((
        cmd.kind.clone(),
        projectile,
        ProjectileState { temp_id: 0 },
        Transform::from_translation(cmd.position),
        cmd.net_id.clone(),
    ));
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        make_projectile_physics(
            entity,
            cmd.position,
            cmd.starting_velocity,
            radius,
            Group::ALL & !GROUP_PLAYER,
            &mut physics,
        )
    };
    world
        .entity_mut(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    if let Some(mut sq) = world.get_resource_mut::<crate::sound::SoundQueue>() {
        sq.play_3d(fire_sound, cmd.position, cmd.starting_velocity);
    }
}

pub fn tick_raycast_projectile(
    lifetime: &mut u32,
    shooter: Option<Entity>,
    entity: Entity,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
) -> Option<RayProjectileHit> {
    *lifetime = lifetime.saturating_sub(1);
    if *lifetime == 0 {
        commands.entity(entity).despawn();
        return None;
    }
    let rb = world.rigid_body_set.get(body.0)?;
    let vel = rb_vel(rb);
    let step = vel.length() * world.integration_parameters.dt;
    if step < 0.001 {
        return None;
    }
    let dir = vel.normalize();
    let prev = rb_pos(rb) - vel * world.integration_parameters.dt;
    let exclude = [entity, shooter.unwrap_or(entity)];
    let (hit, toi) = world.cast_ray(prev, dir, step, &exclude)?;
    commands.entity(entity).despawn();
    Some(RayProjectileHit {
        entity: hit,
        dir,
        point: prev + dir * toi,
    })
}

pub fn apply_raycast_hit<P: Projectile>(
    hit: RayProjectileHit,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    damage: f32,
) {
    apply_hit_impulse::<P>(world, hit.entity, hit.dir, Some(hit.point));
    if let Ok(mut health) = health_q.get_mut(hit.entity) {
        attribute_damage(last_damage_q, hit.entity, shooter);
        health.apply_damage(damage);
    }
}

pub fn knockback_impulse<P: Projectile>(dir: Vec3, scale: f32) -> Vec3 {
    -dir.normalize_or_zero() * P::KNOCKBACK * scale
}

pub fn shooter_knockback_impulse<P: Projectile>(dir: Vec3, scale: f32) -> Vec3 {
    -dir.normalize_or_zero() * P::SHOOTER_KNOCKBACK * scale
}

pub fn apply_hit_impulse<P: Projectile>(
    world: &mut PhysicsWorld,
    entity: Entity,
    dir: Vec3,
    hit_point: Option<Vec3>,
) {
    world.apply_game_impulse_at(
        entity,
        dir.normalize_or_zero() * P::KNOCKBACK,
        hit_point,
        None,
        None,
    );
}

#[cfg(feature = "client")]
pub fn apply_recoil<P: Projectile>(
    ctx: &mut crate::weapon::FireCtx,
    world: &mut PhysicsWorld,
    scale: f32,
) {
    let (Some(shooter), Some(shooter_net_id)) = (ctx.shooter, ctx.shooter_net_id) else {
        return;
    };
    world.apply_game_impulse(
        shooter,
        shooter_knockback_impulse::<P>(ctx.aim_dir, scale),
        Some(shooter_net_id),
        ctx.predicted.as_deref_mut(),
    );
}
