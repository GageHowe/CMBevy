use std::collections::HashMap;

use bevy::prelude::*;
use common::PredictedCommands;
use net::message::{NetworkID, SpawnCommand};
use physics::physics_world::*;
use rapier3d::prelude::{Ball, Collider, ColliderHandle, Pose, QueryFilter};

use crate::GameObject;
use crate::health::{Health, LastDamageSource, attribute_damage};

#[cfg(feature = "client")]
use super::ProjectileState;
use super::{Projectile, helpers, tick_projectiles};
use common::GameObjectKind;

pub const SPEED: f32 = 60.0;
pub const LIFETIME: u32 = 240;
pub const DAMAGE: f32 = 110.0;
pub const EXPLOSION_RADIUS: f32 = 5.0;
pub const EXPLOSION_IMPULSE: f32 = 30.0;
const EXPLOSION_MASS_BLEND: f32 = 0.25;
const RADIUS: f32 = 0.16;
const DIRECT_HIT_BONUS: f32 = 20.0;
const SELF_DAMAGE_SCALE: f32 = 0.5;

#[derive(Component, Reflect)]
pub struct RpgProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}
impl Default for RpgProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            lifetime: LIFETIME,
        }
    }
}

pub fn shooter_knockback(mass: f32) -> f32 {
    mass * <RpgProjectile as Projectile>::SHOOTER_KNOCKBACK
}

fn explosion_mass_scale(mass: f32) -> f32 {
    1.0 + (mass - 1.0).max(0.0) * EXPLOSION_MASS_BLEND
}

impl Projectile for RpgProjectile {
    const KIND: GameObjectKind = GameObjectKind::RpgProjectile;
    const SPEED: f32 = SPEED;
    const SHOOTER_KNOCKBACK: f32 = 3.0;

    fn tick(
        &mut self,
        entity: Entity,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
    ) {
        tick_inner(
            self,
            entity,
            body,
            world,
            commands,
            health_q,
            last_damage_q,
            None,
            None,
        );
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        let Some(&rb_handle) = world.entity_to_handle.get(&shooter) else {
            return;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            return;
        };
        // Match the predicted launcher recoil so server and client stay on the same path.
        let impulse = -dir * shooter_knockback(rb.mass());
        world.apply_game_impulse(shooter, impulse, None, None);
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

fn tick_inner(
    projectile: &mut RpgProjectile,
    entity: Entity,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<&mut PredictedCommands>,
) {
    projectile.lifetime = projectile.lifetime.saturating_sub(1);
    let Some(rb) = world.rigid_body_set.get(body.0) else {
        return;
    };
    let vel = rb_vel(rb);
    let curr = rb_pos(rb);
    let dt = world.integration_parameters.dt;
    let step = vel.length() * dt;
    if projectile.lifetime == 0 {
        explode(
            curr,
            None,
            entity,
            projectile.shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            net_ids,
            predicted,
            None,
        );
        return;
    }
    if step < 0.001 {
        return;
    }
    let prev = curr - vel * dt;
    let exclude = [entity, projectile.shooter.unwrap_or(entity)];
    let dir = vel.normalize();
    if let Some((hit, toi, normal)) = world.cast_sphere(prev, dir, RADIUS, step, &exclude) {
        let hit_point = prev + dir * toi;
        let impulse_dir = normal.normalize_or_zero();
        explode(
            hit_point,
            Some(hit),
            entity,
            projectile.shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            net_ids,
            predicted,
            Some((hit, impulse_dir, hit_point)),
        );
    }
}

fn explode(
    center: Vec3,
    direct_hit: Option<Entity>,
    projectile: Entity,
    shooter: Option<Entity>,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<&mut PredictedCommands>,
    direct_hit_impulse: Option<(Entity, Vec3, Vec3)>,
) {
    let mut affected: HashMap<Entity, f32> = HashMap::new();
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
    let shape = Ball::new(EXPLOSION_RADIUS);
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
            (1.0 - dist / EXPLOSION_RADIUS).clamp(0.0, 1.0)
        };
        if falloff > affected.get(&entity).copied().unwrap_or(0.0) {
            affected.insert(entity, falloff);
        }
    }

    let mut predicted = predicted;
    for (entity, falloff) in affected {
        let Some(&rb_handle) = world.entity_to_handle.get(&entity) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get(rb_handle) else {
            continue;
        };
        let radial_dir = (rb_pos(rb) - center).normalize_or_zero();
        let impulse_dir = if let Some((hit, dir, _)) = direct_hit_impulse
            && hit == entity
        {
            dir
        } else if radial_dir == Vec3::ZERO {
            Vec3::Y
        } else {
            radial_dir
        };
        let impulse = impulse_dir * EXPLOSION_IMPULSE * falloff * explosion_mass_scale(rb.mass());
        let net_id = net_ids.and_then(|net_ids| net_ids.get(entity).ok());
        let point = direct_hit_impulse.and_then(
            |(hit, _, hit_point)| {
                if hit == entity { Some(hit_point) } else { None }
            },
        );
        if world.apply_game_impulse_at(entity, impulse, point, net_id, predicted.as_deref_mut()) {
            if predicted.is_none() {
                if let Ok(mut health) = health_q.get_mut(entity) {
                    let mut damage = DAMAGE * falloff;
                    if direct_hit == Some(entity) {
                        damage += DIRECT_HIT_BONUS;
                    }
                    if shooter == Some(entity) {
                        damage *= SELF_DAMAGE_SCALE;
                    }
                    attribute_damage(last_damage_q, entity, shooter);
                    health.apply_damage(damage);
                }
            }
        }
    }

    commands.entity(projectile).despawn();
}

pub fn spawn(
    origin: Vec3,
    velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    shooter: Option<Entity>,
    temp_id: u32,
) -> Entity {
    helpers::spawn_projectile(
        GameObjectKind::RpgProjectile,
        RpgProjectile {
            shooter,
            lifetime: LIFETIME,
        },
        origin,
        velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    )
}

impl GameObject for RpgProjectile {
    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            RpgProjectile {
                shooter: None,
                lifetime: LIFETIME,
            },
            RADIUS,
            "event:/Weapons/SniperShot",
        );
    }
}

pub struct RpgProjectilePlugin;
impl Plugin for RpgProjectilePlugin {
    fn build(&self, app: &mut App) {
        use common::game_state::GameState;
        app.add_systems(
            FixedUpdate,
            tick_projectiles::<RpgProjectile>
                .after(step_physics)
                .run_if(|state: Option<Res<State<GameState>>>| {
                    state.map_or(true, |s| *s.get() == GameState::SinglePlayer)
                }),
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

#[cfg(feature = "client")]
fn tick_predicted_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &mut RpgProjectile,
        &RigidBodyHandleComponent,
        &ProjectileState,
    )>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    net_ids: Query<&NetworkID>,
    mut predicted: ResMut<PredictedCommands>,
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
            Some(&net_ids),
            Some(&mut predicted),
        );
    }
}

#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<RpgProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.18));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.45, 0.1),
            emissive: LinearRgba::new(5.0, 1.8, 0.3, 1.0),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
