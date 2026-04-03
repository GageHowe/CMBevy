use bevy::prelude::*;
use bevy::scene::DynamicSceneRoot;
use bevy::scene::serde::SceneDeserializer;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::{
    InitialVelocity, PhysicsWorld, RigidBodyHandleComponent, SceneRigidBody,
};
use rapier3d::prelude::*;
use serde::de::DeserializeSeed;
use crate::pawn::biped::SceneBiped;
use crate::pawn::spaceship::SceneSpaceship;
use crate::weapon::hail_mary::SceneHailMary;
use crate::weapon::rifle::SceneRifle;
use crate::weapon::rpg::SceneRpg;

// ── component / resource types ────────────────────────────────────────────────

// TODO: can we make this a generic rapier type instead of declaring our own?
#[derive(Clone, Reflect)]
#[reflect(Default)]
pub enum ColliderShape {
    Ball(f32),
    Cuboid(Vec3),
    Capsule {
        half_height: f32,
        radius: f32,
    },
    /// Path to an OBJ file (relative to assets/) containing VHACD convex hulls.
    ConvexHulls(String),
}

impl Default for ColliderShape {
    fn default() -> Self {
        Self::Ball(1.0)
    }
}

/// Static (fixed) collider placed in the scene. Position/rotation come from Transform.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct StaticCollider {
    pub shape: ColliderShape,
    pub scale: f32,
}

/// Player/bot spawn point placed in the scene. Position/rotation come from Transform.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct SpawnPoint {
    pub team: u8,
}

/// Level-wide metadata inserted as a Resource by the .scn.ron file.
#[derive(Resource, Clone, Reflect, Default)]
#[reflect(Resource, Default)]
pub struct MapMeta {
    /// Visual GLB scenes to load for this level. Client-only.
    pub visuals: Vec<(String, Transform)>,
    /// Optional cubemap path (e.g. KTX2) for the skybox. Client-only.
    pub skybox: Option<String>,
    /// Rendered skybox background brightness.
    pub skybox_brightness: f32,
    /// EnvironmentMapLight intensity (scene PBR lighting from the skybox). Client-only.
    pub env_light_intensity: f32,
}

// ── scene-root marker ─────────────────────────────────────────────────────────

/// Marks the entity that owns the loaded DynamicScene.
/// The visual GLB scene is spawned as a child, so despawning this entity cleans everything up.
#[derive(Component)]
pub struct LevelSceneRoot;

// ── pending hull collider queue ───────────────────────────────────────────────

/// Pending convex-hull colliders for static level geometry, waiting for the mesh asset to load.
#[derive(Resource, Default)]
pub struct PendingHullColliders(pub Vec<(Entity, Vec3, Quat, Handle<ConvexHullAsset>)>);

// ── network transfer helper ───────────────────────────────────────────────────

/// Compressed raw .scn.ron bytes to send to new clients on connect.
/// Server-only; set in the level-load startup system.
#[derive(Resource)]
pub struct LevelBytes(pub Vec<u8>);

/// Reads a .scn.ron file and returns it as compressed bytes for network transfer.
pub fn read_and_compress_level(path: &str) -> Vec<u8> {
    let raw =
        std::fs::read(path).unwrap_or_else(|e| panic!("Failed to read level \"{path}\": {e}"));
    zstd::stream::encode_all(raw.as_slice(), 3).expect("level: zstd compress failed")
}

/// Compressed .scn.ron bytes received from the server, pending scene spawn.
#[derive(Resource)]
pub struct PendingMapScene(pub Vec<u8>);

/// Decompresses scene bytes from the server, deserializes in-memory, and spawns the scene.
/// Exclusive system — runs on the client whenever PendingMapScene exists.
pub fn apply_pending_map_scene(world: &mut World) {
    let Some(pending) = world.remove_resource::<PendingMapScene>() else {
        return;
    };
    let bytes = match zstd::stream::decode_all(pending.0.as_slice()) {
        Ok(b) => b,
        Err(e) => {
            error!("map decompress: {e}");
            return;
        }
    };
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry_guard = registry.read();
    let scene_de = SceneDeserializer {
        type_registry: &registry_guard,
    };
    let mut ron_de = match ron::Deserializer::from_bytes(&bytes) {
        Ok(d) => d,
        Err(e) => {
            error!("map ron: {e}");
            return;
        }
    };
    let scene = match scene_de.deserialize(&mut ron_de) {
        Ok(s) => s,
        Err(e) => {
            error!("map deserialize: {e}");
            return;
        }
    };
    drop(registry_guard);
    let handle = world.resource_mut::<Assets<DynamicScene>>().add(scene);
    world.spawn((DynamicSceneRoot(handle), LevelSceneRoot));
}

// ── plugin ────────────────────────────────────────────────────────────────────

pub struct LevelPlugin;
impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ColliderShape>()
            .register_type::<StaticCollider>()
            .register_type::<SpawnPoint>()
            .register_type::<SceneBiped>()
            .register_type::<SceneSpaceship>()
            .register_type::<SceneRifle>()
            .register_type::<SceneHailMary>()
            .register_type::<SceneRpg>()
            .register_type::<MapMeta>()
            .init_resource::<PendingHullColliders>()
            // react to scene-spawned components — works on both client and server
            .add_systems(Update, (spawn_static_colliders, spawn_hull_colliders));

        // Keep authored scene data as small marker components and route all runtime setup
        // through the existing imperative GameObject spawn path.
        app.add_systems(
            Update,
            (
                spawn_scene_bipeds,
                spawn_scene_spaceships,
                spawn_scene_rifles,
                spawn_scene_hail_marys,
                spawn_scene_rpgs,
            ),
        );
    }
}

// ── systems ───────────────────────────────────────────────────────────────────

fn scene_runtime_spawns_enabled(
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) -> bool {
    is_server.is_some()
        || state.is_some_and(|s| *s.get() == common::game_state::GameState::SinglePlayer)
}

fn queue_scene_spawn(
    entity: Entity,
    transform: &Transform,
    kind: common::GameObjectKind,
    starting_velocity: Vec3,
    commands: &mut Commands,
    net_id_res: &mut ResMut<net::message::NetworkIDResource>,
) {
    let net_id = net::message::NetworkID(net_id_res.next());
    let new_entity = commands.spawn_empty().id();
    commands.queue(crate::SpawnGameObjectCommand {
        entity: new_entity,
        cmd: net::message::SpawnCommand {
            net_id,
            position: transform.translation,
            rotation: transform.rotation,
            starting_velocity,
            server_tick: 0,
            kind,
        },
    });
    commands.entity(entity).despawn();
}

fn spawn_scene_bipeds(
    query: Query<
        (Entity, &Transform, Option<&InitialVelocity>),
        (Added<SceneBiped>, With<ChildOf>, Without<net::message::NetworkID>),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    if !scene_runtime_spawns_enabled(state, is_server) {
        return;
    }
    for (entity, transform, initial_velocity) in query.iter() {
        queue_scene_spawn(
            entity,
            transform,
            common::GameObjectKind::Biped,
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut net_id_res,
        );
    }
}

fn spawn_scene_spaceships(
    query: Query<
        (Entity, &Transform, Option<&InitialVelocity>),
        (
            Added<SceneSpaceship>,
            With<ChildOf>,
            Without<net::message::NetworkID>,
        ),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    if !scene_runtime_spawns_enabled(state, is_server) {
        return;
    }
    for (entity, transform, initial_velocity) in query.iter() {
        queue_scene_spawn(
            entity,
            transform,
            common::GameObjectKind::Spaceship,
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut net_id_res,
        );
    }
}

fn spawn_scene_rifles(
    query: Query<
        (Entity, &Transform, Option<&InitialVelocity>),
        (Added<SceneRifle>, With<ChildOf>, Without<net::message::NetworkID>),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    if !scene_runtime_spawns_enabled(state, is_server) {
        return;
    }
    for (entity, transform, initial_velocity) in query.iter() {
        queue_scene_spawn(
            entity,
            transform,
            common::GameObjectKind::Rifle,
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut net_id_res,
        );
    }
}

fn spawn_scene_hail_marys(
    query: Query<
        (Entity, &Transform, Option<&InitialVelocity>),
        (
            Added<SceneHailMary>,
            With<ChildOf>,
            Without<net::message::NetworkID>,
        ),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    if !scene_runtime_spawns_enabled(state, is_server) {
        return;
    }
    for (entity, transform, initial_velocity) in query.iter() {
        queue_scene_spawn(
            entity,
            transform,
            common::GameObjectKind::HailMary,
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut net_id_res,
        );
    }
}

fn spawn_scene_rpgs(
    query: Query<
        (Entity, &Transform, Option<&InitialVelocity>),
        (Added<SceneRpg>, With<ChildOf>, Without<net::message::NetworkID>),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    if !scene_runtime_spawns_enabled(state, is_server) {
        return;
    }
    for (entity, transform, initial_velocity) in query.iter() {
        queue_scene_spawn(
            entity,
            transform,
            common::GameObjectKind::Rpg,
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut net_id_res,
        );
    }
}

/// Inserts a fixed physics body on each entity that has a StaticCollider component.
/// Reacts to Added<StaticCollider>, so it works regardless of how the entity was spawned.
pub fn spawn_static_colliders(
    new_colliders: Query<
        (
            Entity,
            &StaticCollider,
            &Transform,
            Option<&RigidBodyHandleComponent>,
            Option<&SceneRigidBody>,
            Option<&InitialVelocity>,
        ),
        Added<StaticCollider>,
    >,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    asset_server: Res<AssetServer>,
) {
    for (entity, sc, transform, body_handle, scene_body, initial_velocity) in new_colliders.iter() {
        let s = sc.scale;
        let pos = transform.translation;
        let rot = transform.rotation;
        if let ColliderShape::ConvexHulls(path) = &sc.shape {
            if body_handle.is_none() {
                ensure_body(
                    entity,
                    pos,
                    rot,
                    scene_body.copied().unwrap_or_default(),
                    initial_velocity.map_or(Vec3::ZERO, |v| v.0),
                    &mut commands,
                    &mut world,
                );
            }
            let handle = asset_server
                .load_with_settings(path.clone(), move |settings: &mut f32| *settings = s);
            pending.0.push((entity, pos, rot, handle));
            continue;
        }
        let collider = match &sc.shape {
            ColliderShape::Cuboid(he) => {
                ColliderBuilder::cuboid(he.x * s, he.y * s, he.z * s).build()
            }
            ColliderShape::Ball(r) => ColliderBuilder::ball(r * s).build(),
            ColliderShape::Capsule {
                half_height,
                radius,
            } => ColliderBuilder::capsule_y(half_height * s, radius * s).build(),
            ColliderShape::ConvexHulls(_) => unreachable!(),
        };
        attach_collider_to_body(
            entity,
            pos,
            rot,
            collider,
            body_handle.map(|h| h.0),
            scene_body.copied().unwrap_or_default(),
            initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            &mut commands,
            &mut world,
        );
    }
}

pub fn spawn_hull_colliders(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
) {
    let ready: Vec<_> = pending
        .0
        .iter()
        .filter_map(|(entity, pos, rot, h)| {
            hull_assets
                .get(h)
                .map(|a| (*entity, *pos, *rot, a.0.clone()))
        })
        .collect();
    pending
        .0
        .retain(|(_, _, _, h)| hull_assets.get(h).is_none());
    for (entity, pos, rot, collider) in ready {
        let existing = world.entity_to_handle.get(&entity).copied();
        attach_collider_to_body(
            entity,
            pos,
            rot,
            collider,
            existing,
            SceneRigidBody::Fixed,
            Vec3::ZERO,
            &mut commands,
            &mut world,
        );
    }
}

fn attach_collider_to_body(
    entity: Entity,
    position: Vec3,
    rotation: Quat,
    collider: Collider,
    existing_handle: Option<RigidBodyHandle>,
    scene_body: SceneRigidBody,
    initial_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let handle = existing_handle.unwrap_or_else(|| {
        ensure_body(
            entity,
            position,
            rotation,
            scene_body,
            initial_velocity,
            commands,
            world,
        )
    });
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        rb.set_rotation(rotation, true);
    }
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(collider, handle, rigid_body_set);
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(handle));
}

fn ensure_body(
    entity: Entity,
    position: Vec3,
    rotation: Quat,
    scene_body: SceneRigidBody,
    initial_velocity: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> RigidBodyHandle {
    if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
        return handle;
    }
    let builder = match scene_body {
        SceneRigidBody::Fixed => RigidBodyBuilder::fixed(),
        SceneRigidBody::Dynamic => RigidBodyBuilder::dynamic(),
    };
    let rb = builder
        .translation(Vector3::new(position.x, position.y, position.z))
        .linvel(Vector3::new(
            initial_velocity.x,
            initial_velocity.y,
            initial_velocity.z,
        ))
        .build();
    let handle = world.insert_body(entity, rb);
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        rb.set_rotation(rotation, true);
    }
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(handle));
    handle
}

/// Spawns the GLB visual scene when MapMeta is available. Client-only.
pub fn load_level_scene(
    scene_root: Query<Entity, With<LevelSceneRoot>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    meta: Res<MapMeta>,
) {
    let Ok(root) = scene_root.single() else {
        return;
    };
    for (scene_path, transform) in &meta.visuals {
        let entity = commands
            .spawn((SceneRoot(asset_server.load(scene_path.clone())), *transform))
            .id();
        // parent to the scene root so it despawns with it
        commands.entity(root).add_child(entity);
    }
}

/// Despawns the level scene and removes level resources.
/// Bevy's DynamicSceneRoot component hook handles scene-entity cleanup on entity despawn.
pub fn cleanup_level(
    mut commands: Commands,
    scene_roots: Query<Entity, With<LevelSceneRoot>>,
    mut pending: ResMut<PendingHullColliders>,
) {
    for entity in scene_roots.iter() {
        commands.queue(move |world: &mut World| {
            if let Ok(entity) = world.get_entity_mut(entity) {
                entity.despawn();
            }
        });
    }
    pending.0.clear();
    commands.remove_resource::<MapMeta>();
}
