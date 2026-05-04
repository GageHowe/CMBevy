use bevy::prelude::*;
#[cfg(feature = "client")]
use common::game_state::GameState;
use common::{GameObjectKind, PredictedCommands};
use net::message::{NetworkID, SpawnCommand};
use physics::physics_world::*;

use super::{Projectile, ProjectileState, helpers, tick_projectiles};
use crate::{
    GameObject,
    health::{Health, LastDamageSource},
    spawn::AppGameObjectExt,
};

pub const SPEED: f32 = 60.0;
pub const LIFETIME: u32 = 240;
pub const DAMAGE: f32 = 110.0;
pub const EXPLOSION_RADIUS: f32 = 10.0;
pub const EXPLOSION_IMPULSE: f32 = 30.0;
pub const EXPLOSION_IMPULSE_MAX_EFFECTIVE_MASS: f32 = 1000.0;
const RADIUS: f32 = 0.16;
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

#[derive(Component, Reflect)]
pub struct RpgProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}
impl Default for RpgProjectile {
    fn default() -> Self {
        Self { shooter: None, lifetime: LIFETIME }
    }
}

pub fn shooter_knockback(mass: f32) -> f32 {
    mass * <RpgProjectile as Projectile>::SHOOTER_KNOCKBACK
}

impl Projectile for RpgProjectile {
    const KIND: GameObjectKind = GameObjectKind::RpgProjectile;
    const SPEED: f32 = SPEED;
    const SHOOTER_KNOCKBACK: f32 = 3.0;

    fn tick(
        &mut self,
        entity: Entity,
        state: &mut ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
    ) {
        tick_inner(self, entity, state, body, world, commands, health_q, last_damage_q, None, None);
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

fn tick_inner(
    projectile: &mut RpgProjectile,
    entity: Entity,
    state: &mut ProjectileState,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    net_ids: Option<&Query<&NetworkID>>,
    predicted: Option<&mut PredictedCommands>,
) {
    helpers::tick_sphere_explosive_projectile(
        &mut projectile.lifetime,
        projectile.shooter,
        entity,
        state,
        body,
        world,
        commands,
        health_q,
        last_damage_q,
        net_ids,
        predicted,
        &CONFIG,
        #[cfg(feature = "client")]
        EXPLOSION_SHAKE_RADIUS,
        #[cfg(feature = "client")]
        SHAKE_SCALE,
    );
}

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
        GameObjectKind::RpgProjectile,
        RpgProjectile { shooter, lifetime: LIFETIME },
        origin,
        velocity,
        shooter_velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    );
    helpers::queue_world_fire_sound(commands, shooter, "event:/Weapons/SniperShot", origin, velocity);
    entity
}

impl GameObject for RpgProjectile {
    const KIND: GameObjectKind = GameObjectKind::RpgProjectile;

    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            RpgProjectile { shooter: None, lifetime: LIFETIME },
            RADIUS,
            "event:/Weapons/SniperShot",
        );
    }
}

pub struct RpgProjectilePlugin;
impl Plugin for RpgProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<RpgProjectile>().add_systems(
            FixedUpdate,
            tick_projectiles::<RpgProjectile>
                .after(step_physics)
                .in_set(super::AuthoritySet::Projectile),
        );
        #[cfg(feature = "client")]
        app.add_systems(
            FixedUpdate,
            tick_predicted_projectiles.after(step_physics).run_if(in_state(GameState::Multiplayer)),
        );
        #[cfg(feature = "client")]
        app.add_systems(bevy::prelude::Update, add_visual);
    }
}

#[cfg(feature = "client")]
fn tick_predicted_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut RpgProjectile, &RigidBodyHandleComponent, &mut ProjectileState)>,
    mut health_q: Query<&mut Health>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    net_ids: Query<&NetworkID>,
    mut predicted: ResMut<PredictedCommands>,
) {
    for (entity, mut projectile, body, mut state) in q.iter_mut() {
        if state.temp_id == 0 {
            continue;
        }
        tick_inner(
            &mut projectile,
            entity,
            &mut state,
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
        commands.entity(entity).insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
