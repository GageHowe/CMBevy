use bevy::prelude::*;
use common::{LocalControl, PredictedImpulses};
#[cfg(feature = "client")]
use common::game_state::GameState;
use net::message::*;
use physics::{
    collider_flags::{ColliderFlags, collider_flags},
    physics_world::*,
};
use rapier3d::prelude::{
    Ball, Collider, ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode,
    Pose, QueryFilter, RigidBodyBuilder, Vector,
};

use crate::{
    AuthoritySystems,
    health::{DamageCause, Health, LastDamageSource, attribute_damage},
    shield::Shield,
    spawn::CenterOfMassSplashDamage,
};

pub const PISTOL_SPEED: f32 = 600.0;
pub const RIFLE_SPEED: f32 = 600.0;
pub const HAIL_MARY_SPEED: f32 = 500.0;
pub const LOBBER_SPEED: f32 = 60.0;
pub const THUMPER_SPEED: f32 = 120.0;
pub const COIL_LAUNCHER_SPEED: f32 = 100.0;

const SHIELD_EXIT_EPSILON: f32 = 0.001;
const DEFAULT_SENSOR_RADIUS: f32 = 0.03;

pub struct FiredProjectile {
    pub entity: Entity,
    pub net_id: NetworkID,
    pub position: Vec3,
    pub starting_velocity: Vec3,
    pub shooter_velocity: Vec3,
}

#[derive(Component, Clone, Copy)]
pub struct Projectile {
    pub shooter: Option<Entity>,
    pub last_position: Vec3,
    pub inherited_launch_velocity: Vec3,
    pub lifetime: u32,
    pub radius: Option<f32>,
    pub contact_damage: f32,
    pub knockback: f32,
    pub damage_cause: DamageCause,
    pub despawn_on_contact: bool,
    pub explosion: Option<ProjectileExplosion>,
}

#[derive(Component, Clone, Copy)]
pub struct ProjectileTempId(pub u32);

#[derive(Clone, Copy)]
pub struct ProjectileExplosion {
    pub radius: f32,
    pub impulse: f32,
    pub impulse_mass_cap: f32,
    pub self_damage_scale: f32,
    pub percent_max_health_damage: f32,
    #[cfg(feature = "client")]
    pub shake_radius: f32,
    #[cfg(feature = "client")]
    pub shake_scale: f32,
    #[cfg(feature = "client")]
    pub effect: fn(&mut World, Vec3, Vec3),
}

#[derive(Clone, Copy)]
struct ProjectileHit {
    entity: Entity,
    collider: ColliderHandle,
    dir: Vec3,
    point: Vec3,
}

pub struct ProjectilePlugin;
impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.init_resource::<PredictedProjectileMap>().add_systems(
            FixedPostUpdate,
            (
                index_added_predicted_projectiles,
                index_removed_predicted_projectiles,
            )
                .in_set(TrackPredictedProjectilesSet),
        );
        app.add_systems(
            FixedUpdate,
            tick_projectiles
                .after(step_physics)
                .in_set(ProjectileDamageSet)
                .in_set(AuthoritySystems),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            tick_predicted_projectiles
                .after(step_physics)
                .run_if(in_state(GameState::Multiplayer)),
        );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectileDamageSet;

pub fn shooter_velocity(world: &PhysicsWorld, shooter: Option<Entity>) -> Vec3 {
    shooter
        .and_then(|entity| world.entity_to_handle.get(&entity).copied())
        .and_then(|handle| world.rigid_body_set.get(handle))
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

pub fn fire_authoritative(
    projectile: Projectile,
    projectile_speed: f32,
    gravity_scale: f32,
    shooter_impulse: f32,
    mass_scaled_shooter_impulse: bool,
    origin: Vec3,
    dir: Vec3,
    shooter: Entity,
    temp_id: Option<u32>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut NetworkIDResource,
) -> Option<FiredProjectile> {
    let dir = dir.normalize_or_zero();
    let starting_velocity = projectile_velocity(world, Some(shooter), dir, projectile_speed);
    let inherited_launch_velocity = shooter_velocity(world, Some(shooter));
    if dir != Vec3::ZERO && shooter_impulse != 0.0 {
        let impulse = if mass_scaled_shooter_impulse {
            -dir * shooter_mass(world, shooter) * shooter_impulse
        } else {
            -dir * shooter_impulse
        };
        world.apply_game_impulse(shooter, impulse, None, None);
    }
    let entity = spawn(
        projectile,
        gravity_scale,
        origin,
        starting_velocity,
        inherited_launch_velocity,
        commands,
        world,
        Some(shooter),
        temp_id,
    );
    let net_id = NetworkID(net_ids.next());
    commands.entity(entity).insert(net_id.clone());
    Some(FiredProjectile {
        entity,
        net_id: net_id.clone(),
        position: origin,
        starting_velocity,
        shooter_velocity: inherited_launch_velocity,
    })
}

pub fn spawn(
    mut projectile: Projectile,
    gravity_scale: f32,
    origin: Vec3,
    velocity: Vec3,
    // pretty sure this is needed to accurately replicate shooter velocity for raycast handling
    inherited_launch_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    // optionally add an ID for client-predicted projectiles, used for projectile spawn confirmation. Bots submit None.
    temp_id: Option<u32>,
) -> Entity {
    projectile.shooter = shooter;
    projectile.last_position = origin;
    projectile.inherited_launch_velocity = inherited_launch_velocity;
    let entity = commands
        .spawn((projectile, Transform::from_translation(origin)))
        .id();
    if let Some(temp_id) = temp_id {
        commands.entity(entity).insert(ProjectileTempId(temp_id));
    }

    if let Some(gravity_scale) = projectile_gravity(gravity_scale) {
        commands.entity(entity).insert(gravity_scale);
    }
    let rb_handle = make_kinematic_body(entity, origin, velocity, projectile.radius, world);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    entity
}

pub fn spawn_remote(
    entity: Entity,
    mut projectile: Projectile,
    gravity_scale: f32,
    position: Vec3,
    starting_velocity: Vec3,
    shooter_velocity: Vec3,
    world: &mut World,
) {
    projectile.last_position = position;
    projectile.inherited_launch_velocity = shooter_velocity;
    world
        .entity_mut(entity)
        .insert((projectile, Transform::from_translation(position)));
    if let Some(gravity_scale) = projectile_gravity(gravity_scale) {
        world.entity_mut(entity).insert(gravity_scale);
    }
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        make_kinematic_body(
            entity,
            position,
            starting_velocity,
            projectile.radius,
            &mut physics,
        )
    };
    world
        .entity_mut(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    crate::insert_spawn_metadata(entity, world, None, true, None, true);
}

fn projectile_gravity(gravity_scale: f32) -> Option<GravityScale> {
    (gravity_scale != 1.0).then_some(GravityScale(gravity_scale))
}

fn tick_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Projectile, &RigidBodyHandleComponent)>,
    mut health_q: Query<&mut Health, Without<Shield>>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    mut shield_q: Query<(Entity, &Shield, &mut Health)>,
    splash_q: Query<(), With<CenterOfMassSplashDamage>>,
) {
    for (entity, mut projectile, body) in &mut q {
        tick_projectile(
            entity,
            &mut projectile,
            body,
            &mut world,
            &mut commands,
            &mut health_q,
            &mut last_damage_q,
            &mut shield_q,
            &splash_q,
            None,
            None,
        );
    }
}

#[cfg(feature = "client")]
fn tick_predicted_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Projectile, &RigidBodyHandleComponent), With<ProjectileTempId>>,
    mut health_q: Query<&mut Health, Without<Shield>>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    mut shield_q: Query<(Entity, &Shield, &mut Health)>,
    splash_q: Query<(), With<CenterOfMassSplashDamage>>,
    net_ids: Query<&NetworkID>,
    control: Res<LocalControl>,
    mut impulses: ResMut<PredictedImpulses>,
) {
    for (entity, mut projectile, body) in &mut q {
        tick_projectile(
            entity,
            &mut projectile,
            body,
            &mut world,
            &mut commands,
            &mut health_q,
            &mut last_damage_q,
            &mut shield_q,
            &splash_q,
            Some(&net_ids),
            Some((&control, &mut impulses)),
        );
    }
}

fn tick_projectile(
    entity: Entity,
    projectile: &mut Projectile,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health, Without<Shield>>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
    splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<(&LocalControl, &mut PredictedImpulses)>,
) {
    let Some(hit) = cast_projectile(
        projectile,
        entity,
        body,
        world,
        commands,
        &shield_q.as_readonly(),
    ) else {
        return;
    };
    // handle explosions if any
    if let Some(explosion) = projectile.explosion {
        explode(
            hit.point,
            Some(hit.entity),
            entity,
            projectile.shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            shield_q,
            splash_q,
            net_ids,
            predicted,
            Some((hit.entity, hit.collider, hit.dir, hit.point)),
            projectile.contact_damage,
            explosion,
        );
        return;
    }
    // handle direct hit damage
    apply_direct_hit(
        projectile,
        entity,
        hit,
        world,
        commands,
        health_q,
        last_damage_q,
        shield_q,
    );
}

fn cast_projectile(
    projectile: &mut Projectile,
    entity: Entity,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    shield_q: &Query<(Entity, &Shield, &Health)>,
) -> Option<ProjectileHit> {
    projectile.lifetime = projectile.lifetime.saturating_sub(1);
    if projectile.lifetime == 0 {
        commands.entity(entity).despawn();
        return None;
    }
    let rb = world.rigid_body_set.get(body.0)?;
    let curr = rb_pos(rb);
    let dt = world.integration_parameters.dt;
    let prev = projectile.last_position + projectile.inherited_launch_velocity * dt;
    let cast_delta = curr - prev;
    let step = cast_delta.length();
    if step < 0.001 {
        projectile.last_position = curr;
        return None;
    }
    let dir = cast_delta / step;
    let exclude = [entity, projectile.shooter.unwrap_or(entity)];
    let origin = prev;
    let remaining = step;
    let hit = if let Some(radius) = projectile.radius {
        let hit = world
            .cast_sphere(origin, dir, radius, remaining, &exclude)
            .map(|(entity, collider, toi, _)| (entity, collider, toi));
        if let Some((_, collider, _)) = hit
            && shield_should_skip_inside_hit(world, shield_q, collider, origin, radius)
        {
            world
                .cast_sphere_ignoring_shields(origin, dir, radius, remaining, &exclude)
                .map(|(entity, collider, toi, _)| (entity, collider, toi))
        } else {
            hit
        }
    } else {
        world
            .cast_ray_hits(origin, dir, remaining, &exclude)
            .into_iter()
            .find(|hit| !shield_should_skip_inside_hit(world, shield_q, hit.collider, origin, 0.0))
            .map(|hit| (hit.entity, hit.collider, hit.toi))
    };
    let Some((hit_entity, hit_collider, toi)) = hit else {
        projectile.last_position = curr;
        return None;
    };
    Some(ProjectileHit {
        entity: hit_entity,
        collider: hit_collider,
        dir,
        point: origin + dir * toi,
    })
}

fn apply_direct_hit(
    projectile: &Projectile,
    projectile_entity: Entity,
    hit: ProjectileHit,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health, Without<Shield>>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
) {
    if apply_direct_shield_hit(world, hit.collider, projectile.contact_damage, shield_q) {
        if projectile.despawn_on_contact {
            commands.entity(projectile_entity).despawn();
        }
        return;
    }
    if projectile.despawn_on_contact {
        commands.entity(projectile_entity).despawn();
    }
    if projectile.knockback != 0.0 {
        world.apply_game_impulse_at(
            hit.entity,
            hit.dir.normalize_or_zero() * projectile.knockback,
            Some(hit.point),
            None,
            None,
        );
    }
    apply_entity_damage(
        hit.entity,
        projectile.shooter,
        projectile.contact_damage,
        0.0,
        projectile.damage_cause,
        health_q,
        last_damage_q,
    );
}

fn explode(
    center: Vec3,
    direct_hit: Option<Entity>,
    projectile_entity: Entity,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health, Without<Shield>>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
    splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<(&LocalControl, &mut PredictedImpulses)>,
    direct_hit_impulse: Option<(Entity, ColliderHandle, Vec3, Vec3)>,
    contact_damage: f32,
    explosion: ProjectileExplosion,
) {
    #[cfg(feature = "client")]
    queue_explosion_fx(
        commands,
        center,
        explosion_inherit_velocity(world, direct_hit, direct_hit_impulse),
        explosion.shake_radius,
        explosion.shake_scale,
        explosion.effect,
    );
    let mut affected = std::collections::HashMap::<Entity, f32>::new();
    let excluded: Vec<RigidBodyHandle> = world
        .entity_to_handle
        .get(&projectile_entity)
        .copied()
        .into_iter()
        .collect();
    let pred = |_: ColliderHandle, col: &Collider| {
        col.parent()
            .is_none_or(|rb_handle| !excluded.contains(&rb_handle))
    };
    let filter = QueryFilter::new().predicate(&pred);
    let qp = world.broad_phase.as_query_pipeline(
        world.narrow_phase.query_dispatcher(),
        &world.rigid_body_set,
        &world.collider_set,
        filter,
    );
    let shape = Ball::new(explosion.radius);
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
            (1.0 - rb_pos(rb).distance(center) / explosion.radius).clamp(0.0, 1.0)
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
        let impulse_dir = if let Some((hit, _, dir, _)) = direct_hit_impulse {
            if hit == entity { dir } else { radial_dir }
        } else if radial_dir == Vec3::ZERO {
            Vec3::Y
        } else {
            radial_dir
        };
        let impulse =
            impulse_dir * explosion.impulse * rb.mass().min(explosion.impulse_mass_cap) * falloff;
        let net_id = net_ids.and_then(|ids| ids.get(entity).ok());
        let point =
            direct_hit_impulse.and_then(|(hit, _, _, point)| (hit == entity).then_some(point));
        let prediction = predicted
            .as_mut()
            .map(|(control, impulses)| (*control, &mut **impulses));
        if world.apply_game_impulse_at(entity, impulse, point, net_id, prediction)
            && predicted.is_none()
        {
            if let Some((hit, collider, _, _)) = direct_hit_impulse
                && hit == entity
                && apply_direct_shield_hit(world, collider, contact_damage, shield_q)
            {
                continue;
            }
            let mut damage = contact_damage * falloff;
            if shooter == Some(entity) {
                damage *= explosion.self_damage_scale;
            }
            apply_entity_damage(
                entity,
                shooter,
                damage,
                explosion.percent_max_health_damage * falloff,
                DamageCause::Explosion,
                health_q,
                last_damage_q,
            );
        }
    }
    commands.entity(projectile_entity).despawn();
}

fn apply_direct_shield_hit(
    world: &PhysicsWorld,
    collider: ColliderHandle,
    damage: f32,
    shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
) -> bool {
    let flags = world
        .collider_set
        .get(collider)
        .map(|collider| collider_flags(collider.user_data))
        .unwrap_or_else(ColliderFlags::empty);
    if !flags.contains(ColliderFlags::SHIELD) {
        return false;
    }
    let Some((_, _, mut charge)) = shield_q
        .iter_mut()
        .find(|(_, shield, _)| shield.collider == Some(collider))
    else {
        return false;
    };
    if charge.is_dead() {
        return false;
    }
    charge.apply_damage(damage);
    true
}

fn shield_should_skip_inside_hit(
    world: &PhysicsWorld,
    shield_q: &Query<(Entity, &Shield, &Health)>,
    collider_handle: ColliderHandle,
    center: Vec3,
    radius: f32,
) -> bool {
    let Some(collider) = world.collider_set.get(collider_handle) else {
        return false;
    };
    if !collider_flags(collider.user_data).contains(ColliderFlags::SHIELD) {
        return false;
    }
    let Some((_, shield, charge)) = shield_q
        .iter()
        .find(|(_, shield, _)| shield.collider == Some(collider_handle))
    else {
        return false;
    };
    if charge.is_dead() || shield.double_sided {
        return false;
    }
    collider.shape().distance_to_point(
        collider.position(),
        Vector::new(center.x, center.y, center.z),
        true,
    ) <= radius + SHIELD_EXIT_EPSILON
}

// does projectile damage to a non-shield entity if they have a Health component
fn apply_entity_damage(
    entity: Entity,
    attacker: Option<Entity>,
    damage: f32,
    percent_max_health_damage: f32,
    cause: DamageCause,
    health_q: &mut Query<&mut Health, Without<Shield>>,
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

fn make_kinematic_body(
    entity: Entity,
    origin: Vec3,
    velocity: Vec3,
    radius: Option<f32>,
    world: &mut PhysicsWorld,
) -> RigidBodyHandle {
    let handle = world.insert_body(
        entity,
        RigidBodyBuilder::kinematic_velocity_based()
            .translation(origin)
            .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
            .build(),
    );
    let projectile_groups =
        InteractionGroups::new(GROUP_PROJECTILE, Group::NONE, InteractionTestMode::And);
    let collider = ColliderBuilder::ball(radius.unwrap_or(DEFAULT_SENSOR_RADIUS))
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

#[cfg(feature = "client")]
fn shooter_mass(world: &PhysicsWorld, shooter: Entity) -> f32 {
    world
        .entity_to_handle
        .get(&shooter)
        .and_then(|&handle| world.rigid_body_set.get(handle))
        .map(|rb| rb.mass())
        .unwrap_or(0.0)
}

#[cfg(not(feature = "client"))]
fn shooter_mass(world: &PhysicsWorld, shooter: Entity) -> f32 {
    world
        .entity_to_handle
        .get(&shooter)
        .and_then(|&handle| world.rigid_body_set.get(handle))
        .map(|rb| rb.mass())
        .unwrap_or(0.0)
}

#[cfg(feature = "client")]
fn queue_explosion_fx(
    commands: &mut Commands,
    center: Vec3,
    inherit_velocity: Vec3,
    shake_radius: f32,
    shake_scale: f32,
    effect: fn(&mut World, Vec3, Vec3),
) {
    commands.queue(move |world: &mut World| {
        add_explosion_camera_shake(world, center, shake_radius, shake_scale);
        effect(world, center, inherit_velocity);
    });
}

#[cfg(feature = "client")]
fn add_explosion_camera_shake(world: &mut World, center: Vec3, radius: f32, scale: f32) {
    let mut camera_q = world
        .query_filtered::<(&GlobalTransform, &mut crate::pawn::CameraEffector), With<Camera3d>>();
    let Ok((camera_gt, mut camera_fx)) = camera_q.single_mut(world) else {
        return;
    };
    let falloff = (1.0 - camera_gt.translation().distance(center) / radius).clamp(0.0, 1.0);
    if falloff <= 0.0 {
        return;
    }
    camera_fx.add_shake(
        crate::pawn::CameraShake {
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
fn explosion_inherit_velocity(
    world: &PhysicsWorld,
    direct_hit: Option<Entity>,
    direct_hit_impulse: Option<(Entity, ColliderHandle, Vec3, Vec3)>,
) -> Vec3 {
    direct_hit
        .and_then(|entity| {
            let hit_point =
                direct_hit_impulse.and_then(|(hit, _, _, point)| (hit == entity).then_some(point));
            let handle = world.entity_to_handle.get(&entity).copied()?;
            let rb = world.rigid_body_set.get(handle)?;
            Some(match hit_point {
                Some(point) => rb_point_vel(rb, point),
                None => rb_vel(rb),
            })
        })
        .unwrap_or(Vec3::ZERO)
}

#[cfg(feature = "client")]
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrackPredictedProjectilesSet;

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub struct PredictedProjectileMap {
    by_temp_id: std::collections::HashMap<u32, Entity>,
    by_entity: std::collections::HashMap<Entity, u32>,
}

#[cfg(feature = "client")]
impl PredictedProjectileMap {
    pub fn get(&self, temp_id: u32) -> Option<Entity> {
        self.by_temp_id.get(&temp_id).copied()
    }

    pub fn insert(&mut self, temp_id: u32, entity: Entity) {
        if let Some(old_temp_id) = self.by_entity.insert(entity, temp_id) {
            self.by_temp_id.remove(&old_temp_id);
        }
        if let Some(old_entity) = self.by_temp_id.insert(temp_id, entity) {
            self.by_entity.remove(&old_entity);
        }
    }

    pub fn remove_temp_id(&mut self, temp_id: u32) {
        let Some(entity) = self.by_temp_id.remove(&temp_id) else {
            return;
        };
        self.by_entity.remove(&entity);
    }

    pub fn remove_entity(&mut self, entity: Entity) {
        let Some(temp_id) = self.by_entity.remove(&entity) else {
            return;
        };
        self.by_temp_id.remove(&temp_id);
    }
}

#[cfg(feature = "client")]
fn index_added_predicted_projectiles(
    mut map: ResMut<PredictedProjectileMap>,
    added: Query<(Entity, &ProjectileTempId), Added<ProjectileTempId>>,
) {
    for (entity, temp_id) in &added {
        map.insert(temp_id.0, entity);
    }
}

#[cfg(feature = "client")]
fn index_removed_predicted_projectiles(
    mut map: ResMut<PredictedProjectileMap>,
    mut removed: RemovedComponents<ProjectileTempId>,
) {
    for entity in removed.read() {
        map.remove_entity(entity);
    }
}

#[cfg(feature = "client")]
pub fn confirm_projectile(
    temp_id: u32,
    net_id: NetworkID,
    predicted_projectiles: &mut PredictedProjectileMap,
    projectile_q: &Query<(Entity, &ProjectileTempId)>,
    commands: &mut Commands,
) {
    if let Some(projectile_entity) = predicted_projectiles.get(temp_id) {
        predicted_projectiles.remove_temp_id(temp_id);
        if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
            entity_commands.insert(net_id);
        }
        return;
    }
    for (projectile_entity, projectile_temp_id) in projectile_q.iter() {
        if projectile_temp_id.0 == temp_id {
            predicted_projectiles.remove_temp_id(temp_id);
            if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
                entity_commands.insert(net_id.clone());
            }
            break;
        }
    }
}

#[cfg(feature = "client")]
pub fn draw_projectile_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<&RigidBodyHandleComponent, With<Projectile>>,
    mut gizmos: Gizmos,
) {
    use physics::debug::draw_body_colliders;
    for body_handle in &projectiles {
        draw_body_colliders(
            &world,
            body_handle,
            Color::srgba(1.0, 0.8, 0.2, 0.9),
            &mut gizmos,
        );
    }
}

#[cfg(feature = "client")]
pub fn draw_projectile_raycast_debug(
    world: Res<PhysicsWorld>,
    projectiles: Query<(&Projectile, &RigidBodyHandleComponent)>,
    mut gizmos: Gizmos,
) {
    let dt = world.integration_parameters.dt;
    for (projectile, body_handle) in &projectiles {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        let start = projectile.last_position + projectile.inherited_launch_velocity * dt;
        gizmos.line(start, rb_pos(body), Color::srgba(0.2, 1.0, 1.0, 0.9));
    }
}
