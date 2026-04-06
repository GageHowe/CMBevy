use crate::GameObject;
use crate::health::Health;
use bevy::prelude::*;
pub use common::GameObjectKind;
use net::message::*;
use net::quic::*;
use physics::physics_world::*;
use rapier3d::prelude::{Group, RigidBodyBuilder, Vector};

pub mod hail_mary;
pub mod rifle;
pub mod rpg;

pub struct FiredProjectile {
    pub net_id: NetworkID,
    pub spawn_cmd: SpawnCommand,
}

macro_rules! for_each_projectile_type {
    ($m:ident $($args:tt)*) => {
        $m!(
            $($args)*
            rifle::RifleProjectile,
            hail_mary::HailMaryProjectile,
            rpg::RpgProjectile
        )
    };
}

macro_rules! fire_authoritative_match {
    (
        $kind:expr,
        $origin:expr,
        $dir:expr,
        $shooter:expr,
        $tick:expr,
        $commands:expr,
        $world:expr,
        $net_ids:expr;
        $($ty:path),+ $(,)?
    ) => {
        match $kind {
            $(
                <$ty as Projectile>::KIND => Some(<$ty as Projectile>::fire_authoritative(
                    $origin, $dir, $shooter, $tick, $commands, $world, $net_ids,
                )),
            )+
            _ => None,
        }
    };
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
        app.add_observer(on_remove_projectile);
        app.add_plugins((
            rifle::RifleProjectilePlugin,
            hail_mary::HailMaryProjectilePlugin,
            rpg::RpgProjectilePlugin,
        ));
    }
}

pub fn fire_authoritative(
    kind: GameObjectKind,
    origin: Vec3,
    dir: Vec3,
    shooter: Entity,
    tick: u64,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    net_ids: &mut net::message::NetworkIDResource,
) -> Option<FiredProjectile> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    for_each_projectile_type!(fire_authoritative_match kind, origin, dir, shooter, tick, commands, world, net_ids;)
}

pub fn make_generic_projectile_physics(
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

pub fn insert_generic_remote_projectile(
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
        make_generic_projectile_physics(
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
        sq.0.push(crate::sound::SoundRequest {
            event: fire_sound,
            position: Some(cmd.position),
            velocity: Vec3::ZERO,
        });
    }
}

/// Per-projectile-type behavior. Analogous to Weapon / Pawn.
/// The implementing type IS the component (fields: shooter, lifetime, etc.).
/// Requires GameObject so spawn-from-SpawnCommand is also defined per type.
pub trait Projectile: Component<Mutability = bevy::ecs::component::Mutable> + GameObject {
    const KIND: GameObjectKind;
    const SPEED: f32;
    /// Handles lifetime, hit detection, and on-hit effects.
    /// Server-side / singleplayer only — caller registers with appropriate run_if.
    fn tick(
        &mut self,
        entity: Entity,
        body: &RigidBodyHandleComponent,
        world: &mut PhysicsWorld,
        commands: &mut Commands,
        health_q: &mut Query<&mut Health>,
    );

    fn on_authoritative_fire(_dir: Vec3, _shooter: Entity, _world: &mut PhysicsWorld) {}

    fn fire_authoritative(
        origin: Vec3,
        dir: Vec3,
        shooter: Entity,
        tick: u64,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        net_ids: &mut net::message::NetworkIDResource,
    ) -> FiredProjectile {
        let shooter_velocity = world
            .entity_to_handle
            .get(&shooter)
            .and_then(|&h| world.rigid_body_set.get(h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        Self::on_authoritative_fire(dir, shooter, world);
        let starting_velocity = dir * Self::SPEED + shooter_velocity;
        let entity =
            Self::spawn_predicted(origin, starting_velocity, commands, world, Some(shooter), 0);
        let net_id = NetworkID(net_ids.next());
        commands.entity(entity).insert(net_id.clone());
        FiredProjectile {
            net_id: net_id.clone(),
            spawn_cmd: SpawnCommand {
                net_id,
                position: origin,
                starting_velocity,
                rotation: Quat::IDENTITY,
                server_tick: tick,
                kind: Self::KIND,
            },
        }
    }

    fn spawn_predicted(
        origin: Vec3,
        velocity: Vec3,
        commands: &mut Commands,
        world: &mut PhysicsWorld,
        shooter: Option<Entity>,
        temp_id: u32,
    ) -> Entity;
}

/// Thin shared component — only temp_id is cross-cutting (ProjectileConfirm matching on client).
#[derive(Component, Default, Reflect)]
pub struct ProjectileState {
    pub temp_id: u32,
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
        use physics::debug::{draw_collider, rb_iso};
        for body_handle in projectiles.iter() {
            let Some(rb) = world.rigid_body_set.get(body_handle.0) else {
                continue;
            };
            let iso = rb_iso(rb);
            for ch in rb.colliders() {
                if let Some(col) = world.collider_set.get(*ch) {
                    draw_collider(col, iso, color, &mut gizmos);
                }
            }
        }
    }
}

/// Generic tick system. Analogous to move_pawns<T>.
/// Server-only; each projectile plugin registers with run_if(in_state(GameState::SinglePlayer)) on client.
pub fn tick_projectiles<P: Projectile>(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut P, &RigidBodyHandleComponent)>,
    mut health_q: Query<&mut Health>,
) {
    for (entity, mut proj, body) in q.iter_mut() {
        proj.tick(entity, body, &mut world, &mut commands, &mut health_q);
    }
}

#[cfg(feature = "client")]
fn on_remove_projectile(
    event: On<Remove, ProjectileState>,
    net_ids: Query<&NetworkID>,
    quic: Option<ResMut<QuicManager>>,
) {
    let _ = (event, net_ids, quic);
}

#[cfg(not(feature = "client"))]
fn on_remove_projectile(
    event: On<Remove, ProjectileState>,
    net_ids: Query<&NetworkID>,
    mut quic: Option<ResMut<QuicManager>>,
) {
    let (Some(quic), Ok(net_id)) = (quic.as_mut(), net_ids.get(event.entity)) else {
        return;
    };
    quic.send(
        SendTarget::All,
        Channel::Ordered,
        &MsgType::DespawnCommand(net_id.clone()),
    );
}
