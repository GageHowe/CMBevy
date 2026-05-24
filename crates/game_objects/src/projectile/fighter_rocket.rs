use bevy::prelude::*;
#[cfg(feature = "client")]
use common::game_state::GameState;
use common::{GameObjectKind, PredictedCommands};
use net::message::{NetworkID, SpawnCommand};
use physics::physics_world::*;

use super::{Projectile, ProjectileState, helpers, tick_projectiles};
use crate::{
    health::{Health, LastDamageSource},
    shield::Shield,
    spawn::CenterOfMassSplashDamage,
};

pub const SPEED: f32 = 140.0;
pub const LIFETIME: u32 = 240;
pub const DAMAGE: f32 = 110.0;
pub const EXPLOSION_RADIUS: f32 = 10.0;
pub const EXPLOSION_IMPULSE: f32 = 30.0;
pub const EXPLOSION_IMPULSE_MAX_EFFECTIVE_MASS: f32 = 1000.0;
const RADIUS: f32 = 0.16;
const SELF_DAMAGE_SCALE: f32 = 0.5;
const GRAVITY_SCALE: f32 = 0.1;
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
    percent_max_health_damage: 0.0,
    #[cfg(feature = "client")]
    spawn_explosion_effect: bevy_hanabi_plugin::prelude::spawn_lobber_explosion_effect,
};

#[derive(Component, Reflect)]
pub struct FighterRocketProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}

impl Default for FighterRocketProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            lifetime: LIFETIME,
        }
    }
}

impl Projectile for FighterRocketProjectile {
    const KIND: GameObjectKind = GameObjectKind::FighterRocketProjectile;
    const SPEED: f32 = SPEED;
    const SHOOTER_KNOCKBACK: f32 = 3.0;

    fn tick(
        &mut self,
        entity: Entity,
        state: &mut ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health, Without<Shield>>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
        shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
        splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    ) {
        tick_inner(
            self,
            entity,
            state,
            body,
            world,
            commands,
            health_q,
            last_damage_q,
            shield_q,
            splash_q,
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
        let impulse = -dir * rb.mass() * <Self as Projectile>::SHOOTER_KNOCKBACK;
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
        spawn(
            origin,
            velocity,
            shooter_velocity,
            commands,
            world,
            shooter,
            temp_id,
        )
    }
}

fn tick_inner(
    projectile: &mut FighterRocketProjectile,
    entity: Entity,
    state: &mut ProjectileState,
    body: &RigidBodyHandleComponent,
    world: &mut PhysicsWorld,
    commands: &mut Commands,
    health_q: &mut Query<&mut Health, Without<Shield>>,
    last_damage_q: &mut Query<&mut LastDamageSource>,
    shield_q: &mut Query<(Entity, &Shield, &mut Health)>,
    splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
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
        shield_q,
        splash_q,
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
        GameObjectKind::FighterRocketProjectile,
        FighterRocketProjectile {
            shooter,
            lifetime: LIFETIME,
        },
        origin,
        velocity,
        shooter_velocity,
        RADIUS,
        temp_id,
        commands,
        world,
    );
    commands.entity(entity).insert(GravityScale(GRAVITY_SCALE));
    helpers::queue_world_fire_sound(
        commands,
        shooter,
        "event:/Weapons/SniperShot",
        origin,
        velocity,
    );
    entity
}

pub fn spawn_remote_fighter_rocket(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            FighterRocketProjectile {
                shooter: None,
                lifetime: LIFETIME,
            },
            RADIUS,
            "event:/Weapons/SniperShot",
        );
        world.entity_mut(entity).insert(GravityScale(GRAVITY_SCALE));
        crate::insert_spawn_metadata(entity, world, None, true, None, true);
}

pub struct FighterRocketProjectilePlugin;
impl Plugin for FighterRocketProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
                FixedUpdate,
                tick_projectiles::<FighterRocketProjectile>
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

#[cfg(feature = "client")]
fn tick_predicted_projectiles(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &mut FighterRocketProjectile,
        &RigidBodyHandleComponent,
        &mut ProjectileState,
    )>,
    mut health_q: Query<&mut Health, Without<Shield>>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    mut shield_q: Query<(Entity, &Shield, &mut Health)>,
    splash_q: Query<(), With<CenterOfMassSplashDamage>>,
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
            &mut shield_q,
            &splash_q,
            Some(&net_ids),
            Some(&mut predicted),
        );
    }
}

#[cfg(feature = "client")]
fn add_visual(
    q: Query<Entity, Added<FighterRocketProjectile>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &q {
        let mesh = meshes.add(bevy::math::primitives::Sphere::new(0.14));
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.75, 0.2),
            emissive: LinearRgba::new(6.0, 3.0, 0.5, 1.0),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}
