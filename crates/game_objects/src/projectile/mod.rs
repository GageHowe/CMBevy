use bevy::prelude::*;
pub use common::GameObjectKind;
use net::message::*;
use physics::physics_world::*;

use crate::{
    AuthoritySystems,
    health::{DamageCause, Health, LastDamageSource},
    lifecycle::make_spawn_command,
    shield::Shield,
    spawn::CenterOfMassSplashDamage,
};

pub mod coil_launcher;
pub mod failsafe;
pub mod fighter_rocket;
pub mod hail_mary;
pub mod helpers;
pub mod lobber;
pub mod pistol;
pub mod rifle;
pub mod thumper;

pub struct FiredProjectile {
    pub net_id: NetworkID,
    pub spawn_cmd: SpawnCommand,
}

pub struct ProjectilePlugin;
impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.init_resource::<ProjectileIdCounter>()
            .init_resource::<PredictedProjectileMap>()
            .add_systems(
                FixedPostUpdate,
                (
                    index_added_predicted_projectiles,
                    index_removed_predicted_projectiles,
                )
                    .in_set(TrackPredictedProjectilesSet),
            );
        app.add_plugins((
            pistol::PistolProjectilePlugin,
            rifle::RifleProjectilePlugin,
            hail_mary::HailMaryProjectilePlugin,
            fighter_rocket::FighterRocketProjectilePlugin,
            failsafe::FailsafeProjectilePlugin,
            lobber::LobberProjectilePlugin,
            coil_launcher::CoilLauncherProjectilePlugin,
            thumper::ThumperProjectilePlugin,
        ));
    }
}

/// Per-projectile-type behavior. Analogous to Weapon / Pawn.
/// The implementing type IS the component (fields: shooter, lifetime, etc.).
pub trait Projectile: Component<Mutability = bevy::ecs::component::Mutable> {
    /// the GameObjectKind enum member this type corresponds to
    const KIND: GameObjectKind;
    /// relative fire speed of projectile
    const SPEED: f32;
    const KNOCKBACK: f32 = 0.0;
    const SHOOTER_KNOCKBACK: f32 = Self::KNOCKBACK;
    const DAMAGE_CAUSE: DamageCause = DamageCause::Projectile;
    /// Handles lifetime, hit detection, and on-hit effects.
    /// Server-side / singleplayer only — caller registers with appropriate run_if.
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
    );

    fn on_authoritative_fire(_dir: Vec3, _shooter: Entity, _world: &mut PhysicsWorld) {}

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
        let starting_velocity =
            helpers::projectile_velocity(world, Some(shooter), dir, Self::SPEED);
        let shooter_velocity = helpers::shooter_velocity(world, Some(shooter));
        let entity = Self::spawn_predicted(
            origin,
            starting_velocity,
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
            spawn_cmd: make_spawn_command(
                net_id,
                <Self as Projectile>::KIND,
                None,
                origin,
                starting_velocity,
                shooter_velocity,
                Quat::IDENTITY,
                tick,
            ),
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
    ) -> Entity;
}

/// Thin shared component — only temp_id is cross-cutting (ProjectileConfirm matching on client).
#[derive(Component, Default, Reflect)]
pub struct ProjectileState {
    pub temp_id: u32,
    pub shooter_velocity: Vec3,
}

#[derive(Component)]
pub struct ProjectileRaycastDebug {
    pub start: Vec3,
    pub end: Vec3,
}

/// Client resource: monotonically increasing counter for temp projectile ids.
#[derive(Resource, Default)]
pub struct ProjectileIdCounter {
    pub count: u32,
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
    added: Query<(Entity, &ProjectileState), Added<ProjectileState>>,
) {
    for (entity, state) in added.iter() {
        if state.temp_id != 0 {
            map.insert(state.temp_id, entity);
        }
    }
}

#[cfg(feature = "client")]
fn index_removed_predicted_projectiles(
    mut map: ResMut<PredictedProjectileMap>,
    mut removed: RemovedComponents<ProjectileState>,
) {
    for entity in removed.read() {
        map.remove_entity(entity);
    }
}

/// Returns a system that draws all projectiles of type P using gizmos.
/// Color is captured at registration time. Not all projectile types need to use this.
#[cfg(feature = "client")]
pub fn draw_projectile_debug<P: Component>(
    color: Color,
) -> impl Fn(Res<PhysicsWorld>, Query<&RigidBodyHandleComponent, With<P>>, Gizmos) {
    move |world, projectiles, mut gizmos| {
        use physics::debug::draw_body_colliders;
        for body_handle in projectiles.iter() {
            draw_body_colliders(&world, body_handle, color, &mut gizmos);
        }
    }
}

/// Generic tick system for projectile components.
/// Server-only; each projectile plugin registers with run_if(in_state(GameState::SinglePlayer)) on client.
pub fn tick_projectiles<P: Projectile>(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(
        Entity,
        &mut P,
        &mut ProjectileState,
        &RigidBodyHandleComponent,
    )>,
    mut health_q: Query<&mut Health, Without<Shield>>,
    mut last_damage_q: Query<&mut LastDamageSource>,
    mut shield_q: Query<(Entity, &Shield, &mut Health)>,
    splash_q: Query<(), With<CenterOfMassSplashDamage>>,
) {
    for (entity, mut proj, mut state, body) in q.iter_mut() {
        proj.tick(
            entity,
            &mut state,
            body,
            &mut world,
            &mut commands,
            &mut health_q,
            &mut last_damage_q,
            &mut shield_q,
            &splash_q,
        );
    }
}

#[cfg(feature = "client")]
pub fn confirm_projectile(
    temp_id: u32,
    net_id: NetworkID,
    predicted_projectiles: &mut PredictedProjectileMap,
    projectile_q: &Query<(Entity, &ProjectileState)>,
    commands: &mut Commands,
) {
    if let Some(projectile_entity) = predicted_projectiles.get(temp_id) {
        predicted_projectiles.remove_temp_id(temp_id);
        if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
            entity_commands.insert(net_id);
        }
        return;
    }
    for (projectile_entity, state) in projectile_q.iter() {
        if state.temp_id == temp_id {
            predicted_projectiles.remove_temp_id(temp_id);
            if let Ok(mut entity_commands) = commands.get_entity(projectile_entity) {
                entity_commands.insert(net_id.clone());
            }
            break;
        }
    }
}

#[cfg(feature = "client")]
pub fn draw_projectile_raycast_debug(
    segments: Query<(Entity, &ProjectileRaycastDebug)>,
    mut gizmos: Gizmos,
    mut commands: Commands,
) {
    for (entity, segment) in &segments {
        gizmos.line(segment.start, segment.end, Color::srgba(0.2, 1.0, 1.0, 0.9));
        commands.entity(entity).remove::<ProjectileRaycastDebug>();
    }
}
