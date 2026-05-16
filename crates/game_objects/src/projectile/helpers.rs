use bevy::prelude::*;
use common::{GameObjectKind, PredictedCommands};
use net::message::{NetworkID, SpawnCommand};
use physics::collider_flags::{ColliderFlags, collider_flags};
use physics::physics_world::*;
use rapier3d::prelude::{
    Ball, Collider, ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode,
    Pose, QueryFilter, RigidBodyBuilder, Vector,
};

use super::{Projectile, ProjectileState};
use crate::health::{DamageCause, Health, LastDamageSource, attribute_damage};
#[cfg(feature = "client")]
use crate::pawn::{CameraEffector, CameraShake};
use crate::spawn::CenterOfMassSplashDamage;
use crate::shield::Shield;

#[derive(Clone, Copy)]
pub struct RayProjectileHit {
    pub entity: Entity,
    pub collider: ColliderHandle,
    pub dir: Vec3,
    pub normal: Vec3,
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
    radius: f32,
    world: &mut PhysicsWorld,
) -> RigidBodyHandle {
    let handle = world.insert_body(
        entity,
        RigidBodyBuilder::kinematic_velocity_based()
            .translation(origin)
            .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
            .build(),
    );
    // Projectiles use raycasts for hits, but they still need a collider so planet gravity queries
    // can "see" them. Make it a sensor in a non-interacting group so it never creates contacts.
    let projectile_groups =
        InteractionGroups::new(GROUP_PROJECTILE, Group::NONE, InteractionTestMode::And);
    let collider = ColliderBuilder::ball(radius)
        .sensor(true)
        .collision_groups(projectile_groups)
        .solver_groups(projectile_groups)
        .build();
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(collider, handle, rigid_body_set);
    handle
}

pub fn spawn_projectile(
    kind: GameObjectKind,
    projectile: impl Bundle,
    origin: Vec3,
    velocity: Vec3,
    shooter_velocity: Vec3,
    radius: f32,
    temp_id: u32,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands
        .spawn((
            kind,
            projectile,
            ProjectileState {
                temp_id,
                shooter_velocity,
            },
            Transform::from_translation(origin),
        ))
        .id();
    // Projectile collision is resolved by casts so contacts do not push the shooter.
    let rb_handle = make_projectile_physics(entity, origin, velocity, radius, world);
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
        ProjectileState {
            temp_id: 0,
            shooter_velocity: cmd.shooter_velocity,
        },
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
            &mut physics,
        )
    };
    world
        .entity_mut(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    play_world_fire_sound(world, None, fire_sound, cmd.position, cmd.starting_velocity);
}

#[cfg(feature = "client")]
pub fn queue_world_fire_sound(
    commands: &mut Commands,
    shooter: Option<Entity>,
    fire_sound: &'static str,
    position: Vec3,
    velocity: Vec3,
) {
    commands.queue(move |world: &mut World| {
        play_world_fire_sound(world, shooter, fire_sound, position, velocity);
    });
}

#[cfg(not(feature = "client"))]
pub fn queue_world_fire_sound(
    _commands: &mut Commands,
    _shooter: Option<Entity>,
    _fire_sound: &'static str,
    _position: Vec3,
    _velocity: Vec3,
) {
}

fn play_world_fire_sound(
    world: &mut World,
    #[cfg(feature = "client")] shooter: Option<Entity>,
    #[cfg(not(feature = "client"))] _shooter: Option<Entity>,
    fire_sound: &'static str,
    position: Vec3,
    velocity: Vec3,
) {
    #[cfg(feature = "client")]
    if let Some(local_shooter) = world
        .query_filtered::<Entity, With<crate::pawn::Possessed>>()
        .single(world)
        .ok()
        && shooter == Some(local_shooter)
    {
        return;
    }
    if let Some(mut sq) = world.get_resource_mut::<crate::sound::SoundQueue>() {
        sq.play_3d(fire_sound, position, velocity);
    }
}

pub fn tick_raycast_projectile(
    lifetime: &mut u32,
    shooter: Option<Entity>,
    entity: Entity,
    state: &mut ProjectileState,
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
    let dt = world.integration_parameters.dt;
    let cast_vel = vel - state.shooter_velocity;
    let step = cast_vel.length() * dt;
    if step < 0.001 {
        return None;
    }
    let dir = cast_vel.normalize();
    let prev = rb_pos(rb) - cast_vel * dt;
    #[cfg(feature = "client")]
    {
        commands
            .entity(entity)
            .insert(super::ProjectileRaycastDebug {
                start: prev,
                end: prev + dir * step,
            });
    }
    let exclude = [entity, shooter.unwrap_or(entity)];
    let hit = world.cast_ray_detailed(prev, dir, step, &exclude)?;
    Some(RayProjectileHit {
        entity: hit.entity,
        collider: hit.collider,
        dir,
        normal: hit.normal,
        point: prev + dir * hit.toi,
    })
}

pub struct ExplosiveProjectileConfig {
    pub projectile_radius: f32,
    pub damage: f32,
    pub explosion_radius: f32,
    pub explosion_impulse: f32,
    pub explosion_impulse_max_effective_mass: f32,
    pub self_damage_scale: f32,
    pub percent_max_health_damage: f32,
    #[cfg(feature = "client")]
    pub spawn_explosion_effect: fn(&mut World, Vec3, Vec3),
}

#[cfg(feature = "client")]
pub fn rocket_explosion_inherit_velocity(
    world: &PhysicsWorld,
    direct_hit: Option<Entity>,
    direct_hit_impulse: Option<(Entity, ColliderHandle, Vec3, Vec3)>,
) -> Vec3 {
    direct_hit
        .and_then(|entity| {
            let hit_point = direct_hit_impulse
                .and_then(
                    |(hit, _, _, hit_point)| if hit == entity { Some(hit_point) } else { None },
                );
            let handle = world.entity_to_handle.get(&entity).copied()?;
            let rb = world.rigid_body_set.get(handle)?;
            Some(match hit_point {
                Some(hit_point) => rb_point_vel(rb, hit_point),
                None => rb_vel(rb),
            })
        })
        .unwrap_or(Vec3::ZERO)
}

#[cfg(feature = "client")]
pub fn add_explosion_camera_shake(world: &mut World, center: Vec3, radius: f32, scale: f32) {
    let mut camera_q =
        world.query_filtered::<(&GlobalTransform, &mut CameraEffector), With<Camera3d>>();
    let Ok((camera_gt, mut camera_fx)) = camera_q.single_mut(world) else {
        return;
    };
    let falloff = (1.0 - camera_gt.translation().distance(center) / radius).clamp(0.0, 1.0);
    if falloff <= 0.0 {
        return;
    }
    camera_fx.add_shake(
        CameraShake {
            translation: Vec3::new(0.2, 0.2, 0.3),
            rotation: Vec2::new(0.1, 0.1),
            roll: 0.2,
            duration: 1.0,
            frequency: 10.0,
        }
        .scaled(falloff * scale),
    );
}

#[cfg(feature = "client")]
pub fn queue_rocket_explosion_fx(
    commands: &mut Commands,
    center: Vec3,
    inherit_velocity: Vec3,
    shake_radius: f32,
    shake_scale: f32,
    spawn_effect: fn(&mut World, Vec3, Vec3),
) {
    commands.queue(move |world: &mut World| {
        add_explosion_camera_shake(world, center, shake_radius, shake_scale);
        spawn_effect(world, center, inherit_velocity);
    });
}

pub fn tick_sphere_explosive_projectile(
    lifetime: &mut u32,
    shooter: Option<Entity>,
    entity: Entity,
    state: &mut ProjectileState,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<&mut Shield>,
    splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<&mut PredictedCommands>,
    config: &ExplosiveProjectileConfig,
    #[cfg(feature = "client")] shake_radius: f32,
    #[cfg(feature = "client")] shake_scale: f32,
) {
    *lifetime = lifetime.saturating_sub(1);
    let Some(rb) = world.rigid_body_set.get(body.0) else {
        return;
    };
    let vel = rb_vel(rb);
    let curr = rb_pos(rb);
    let dt = world.integration_parameters.dt;
    let cast_vel = vel - state.shooter_velocity;
    let step = cast_vel.length() * dt;
    if *lifetime == 0 {
        explode_sphere_explosive_projectile(
            curr,
            None,
            entity,
            shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            shield_q,
            splash_q,
            net_ids,
            predicted,
            None,
            config,
            #[cfg(feature = "client")]
            shake_radius,
            #[cfg(feature = "client")]
            shake_scale,
        );
        return;
    }
    if step < 0.001 {
        return;
    }
    let prev = curr - cast_vel * dt;
    let exclude = [entity, shooter.unwrap_or(entity)];
    let dir = cast_vel.normalize();
    #[cfg(feature = "client")]
    {
        commands
            .entity(entity)
            .insert(super::ProjectileRaycastDebug {
                start: prev,
                end: prev + dir * step,
            });
    }
    if let Some((hit, hit_collider, toi, normal)) =
        world.cast_sphere(prev, dir, config.projectile_radius, step, &exclude)
    {
        let hit_point = prev + dir * toi;
        explode_sphere_explosive_projectile(
            hit_point,
            Some(hit),
            entity,
            shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            shield_q,
            splash_q,
            net_ids,
            predicted,
            Some((hit, hit_collider, normal.normalize_or_zero(), hit_point)),
            config,
            #[cfg(feature = "client")]
            shake_radius,
            #[cfg(feature = "client")]
            shake_scale,
        );
    }
}

pub fn explode_sphere_explosive_projectile(
    center: Vec3,
    direct_hit: Option<Entity>,
    projectile: Entity,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<&mut Shield>,
    splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<&mut PredictedCommands>,
    direct_hit_impulse: Option<(Entity, ColliderHandle, Vec3, Vec3)>,
    config: &ExplosiveProjectileConfig,
    #[cfg(feature = "client")] shake_radius: f32,
    #[cfg(feature = "client")] shake_scale: f32,
) {
    #[cfg(feature = "client")]
    queue_rocket_explosion_fx(
        commands,
        center,
        rocket_explosion_inherit_velocity(world, direct_hit, direct_hit_impulse),
        shake_radius,
        shake_scale,
        config.spawn_explosion_effect,
    );

    let mut affected = std::collections::HashMap::<Entity, f32>::new();
    let excluded: Vec<RigidBodyHandle> = [Some(projectile)]
        .into_iter()
        .flatten()
        .filter_map(|e| world.entity_to_handle.get(&e).copied())
        .collect();
    let pred = |_: ColliderHandle, col: &Collider| {
        col.parent().map_or(true, |rb_h| !excluded.contains(&rb_h))
    };
    let filter = QueryFilter::new().predicate(&pred);
    let qp = world.broad_phase.as_query_pipeline(
        world.narrow_phase.query_dispatcher(),
        &world.rigid_body_set,
        &world.collider_set,
        filter,
    );
    let shape = Ball::new(config.explosion_radius);
    let iso = Pose::translation(center.x, center.y, center.z);
    for (_, collider) in qp.intersect_shape(iso, &shape) {
        let Some(rb_handle) = collider.parent() else {
            continue;
        };
        let Some(&entity) = world.handle_to_entity.get(&rb_handle) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            continue;
        };
        let falloff = if direct_hit == Some(entity) {
            1.0
        } else {
            let offset = rb_pos(rb) - center;
            let dist = offset.length();
            (1.0 - dist / config.explosion_radius).clamp(0.0, 1.0)
        };
        if falloff > affected.get(&entity).copied().unwrap_or(0.0) {
            affected.insert(entity, falloff);
        }
    }

    let mut predicted = predicted;
    for (entity, falloff) in affected {
        if direct_hit != Some(entity) && !splash_q.contains(entity) {
            continue;
        }
        let Some(&rb_handle) = world.entity_to_handle.get(&entity) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            continue;
        };
        let radial_dir = (rb_pos(rb) - center).normalize_or_zero();
        let impulse_dir = if let Some((hit, _, dir, _)) = direct_hit_impulse
            && hit == entity
        {
            dir
        } else if radial_dir == Vec3::ZERO {
            Vec3::Y
        } else {
            radial_dir
        };
        let effective_mass = rb.mass().min(config.explosion_impulse_max_effective_mass);
        let impulse = impulse_dir * config.explosion_impulse * effective_mass * falloff;
        let net_id = net_ids.and_then(|net_ids| net_ids.get(entity).ok());
        let point = direct_hit_impulse.and_then(
            |(hit, _, _, hit_point)| {
                if hit == entity { Some(hit_point) } else { None }
            },
        );
        if world.apply_game_impulse_at(entity, impulse, point, net_id, predicted.as_deref_mut()) {
            if predicted.is_none() {
                if let Some((hit, hit_collider, hit_normal, _)) = direct_hit_impulse
                    && hit == entity
                    && apply_direct_shield_collider_damage(
                        world,
                        hit,
                        hit_collider,
                        impulse_dir,
                        hit_normal,
                        shield_q,
                        config.damage,
                    )
                {
                    continue;
                }
                let mut damage = config.damage * falloff;
                if shooter == Some(entity) {
                    damage *= config.self_damage_scale;
                }
                apply_entity_damage(
                    entity,
                    shooter,
                    damage,
                    config.percent_max_health_damage * falloff,
                    DamageCause::Explosion,
                    health_q,
                    last_damage_q,
                );
            }
        }
    }

    commands.entity(projectile).despawn();
}

pub fn apply_raycast_hit<P: Projectile>(
    projectile: Entity,
    hit: RayProjectileHit,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<&mut Shield>,
    damage: f32,
) {
    if let Some(blocked) = apply_direct_shield_hit(world, hit, shield_q, damage) {
        if blocked {
            commands.entity(projectile).despawn();
            return;
        }
    }
    commands.entity(projectile).despawn();
    apply_hit_impulse::<P>(world, hit.entity, hit.dir, Some(hit.point));
    apply_entity_damage(
        hit.entity,
        shooter,
        damage,
        0.0,
        P::DAMAGE_CAUSE,
        health_q,
        last_damage_q,
    );
}

fn apply_direct_shield_hit(
    world: &PhysicsWorld,
    hit: RayProjectileHit,
    shield_q: &mut Query<&mut Shield>,
    damage: f32,
) -> Option<bool> {
    let flags = world
        .collider_set
        .get(hit.collider)
        .map(|collider| collider_flags(collider.user_data))
        .unwrap_or_else(ColliderFlags::empty);
    if !flags.contains(ColliderFlags::SHIELD) {
        return Some(false);
    }
    if hit.dir.dot(hit.normal) >= 0.0 {
        return Some(false);
    }
    let Ok(mut shield) = shield_q.get_mut(hit.entity) else {
        return Some(true);
    };
    shield.apply_damage(damage);
    Some(true)
}

fn apply_direct_shield_collider_damage(
    world: &PhysicsWorld,
    entity: Entity,
    collider: ColliderHandle,
    _dir: Vec3,
    _normal: Vec3,
    shield_q: &mut Query<&mut Shield>,
    damage: f32,
) -> bool {
    let flags = world
        .collider_set
        .get(collider)
        .map(|collider| collider_flags(collider.user_data))
        .unwrap_or_else(ColliderFlags::empty);
    if !flags.contains(ColliderFlags::SHIELD) {
        return false;
    }
    let Ok(mut shield) = shield_q.get_mut(entity) else {
        return true;
    };
    shield.apply_damage(damage);
    true
}

fn apply_entity_damage(
    entity: Entity,
    attacker: Option<Entity>,
    damage: f32,
    percent_max_health_damage: f32,
    cause: DamageCause,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
) {
    if damage <= 0.0 && percent_max_health_damage <= 0.0 {
        return;
    }
    if let Ok(mut health) = health_q.get_mut(entity) {
        attribute_damage(last_damage_q, entity, attacker, cause);
        health.apply_damage(damage);
        health.apply_percent_damage(percent_max_health_damage);
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
