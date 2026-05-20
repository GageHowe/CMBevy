use bevy::prelude::*;
use common::GameObjectKind;
use net::message::SpawnCommand;
use physics::physics_world::*;

// for sounds etc, will remove when possible
#[cfg(feature = "client")]
use super::rifle;
use super::{Projectile, helpers, tick_projectiles};
use crate::{
    GameObject,
    health::{Health, LastDamageSource},
    shield::Shield,
    spawn::AppGameObjectExt,
    spawn::CenterOfMassSplashDamage,
};

pub const SPEED: f32 = 600.0;
pub const DAMAGE: f32 = 60.0;
pub const LIFETIME: u32 = 60;
const RADIUS: f32 = 0.03;

#[derive(Component, Reflect)]
pub struct PistolProjectile {
    pub shooter: Option<Entity>,
    pub lifetime: u32,
}

impl Default for PistolProjectile {
    fn default() -> Self {
        Self {
            shooter: None,
            lifetime: LIFETIME,
        }
    }
}

impl Projectile for PistolProjectile {
    const KIND: GameObjectKind = GameObjectKind::PistolProjectile;
    const SPEED: f32 = SPEED;
    const KNOCKBACK: f32 = 0.1;

    fn tick(
        &mut self,
        entity: Entity,
        state: &mut super::ProjectileState,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
        last_damage_q: &mut Query<&mut LastDamageSource>,
        shield_q: &mut Query<&mut Shield>,
        _splash_q: &Query<(), With<CenterOfMassSplashDamage>>,
    ) {
        let Some(hit) = helpers::tick_raycast_projectile(
            &mut self.lifetime,
            self.shooter,
            entity,
            state,
            body,
            world,
            commands,
            &shield_q.as_readonly(),
        ) else {
            return;
        };
        helpers::apply_raycast_hit::<Self>(
            entity,
            hit,
            self.shooter,
            world,
            commands,
            health_q,
            last_damage_q,
            shield_q,
            DAMAGE,
        );
    }

    fn on_authoritative_fire(dir: Vec3, shooter: Entity, world: &mut PhysicsWorld) {
        world.apply_game_impulse(
            shooter,
            helpers::knockback_impulse::<Self>(dir, 1.0),
            None,
            None,
        );
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
        GameObjectKind::PistolProjectile,
        PistolProjectile {
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
    helpers::queue_world_fire_sound(
        commands,
        shooter,
        "event:/Weapons/RifleShot",
        origin,
        velocity,
    );
    entity
}

impl GameObject for PistolProjectile {
    const KIND: GameObjectKind = GameObjectKind::PistolProjectile;

    fn spawn(entity: Entity, cmd: &SpawnCommand, world: &mut World) {
        helpers::insert_remote_projectile(
            entity,
            cmd,
            world,
            PistolProjectile {
                shooter: None,
                lifetime: LIFETIME,
            },
            RADIUS,
            "event:/Weapons/RifleShot",
        );
    }
}

pub struct PistolProjectilePlugin;
impl Plugin for PistolProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<PistolProjectile>().add_systems(
            FixedUpdate,
            tick_projectiles::<PistolProjectile>
                .after(step_physics)
                .in_set(super::AuthoritySystems),
        );
        #[cfg(feature = "client")]
        app.add_systems(bevy::prelude::Update, rifle::add_visual::<PistolProjectile>);
    }
}
