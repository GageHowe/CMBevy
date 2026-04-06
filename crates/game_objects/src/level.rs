use bevy::prelude::*;
use bevy::scene::DynamicSceneRoot;
use bevy::scene::serde::SceneDeserializer;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::{
    InitialVelocity, PhysicsWorld, RigidBodyHandleComponent, SceneRigidBody, rb_angvel, rb_pos,
    rb_rot, rb_vel,
};
use rapier3d::prelude::*;
use serde::de::DeserializeSeed;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

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
    /// Local asset path or `sha256:...` remote ref to an OBJ file containing VHACD convex hulls.
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

#[derive(Component, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct SceneSpawn {
    pub kind: common::GameObjectKind,
    pub respawn_delay_secs: f32,
}

impl Default for SceneSpawn {
    fn default() -> Self {
        Self {
            kind: common::GameObjectKind::Biped,
            respawn_delay_secs: 10.0,
        }
    }
}

#[derive(Component)]
struct SceneSpawner {
    kind: common::GameObjectKind,
    starting_velocity: Vec3,
    respawn_delay_secs: f32,
    respawn_timer_secs: f32,
    active_entity: Option<Entity>,
}

impl SceneSpawner {
    fn is_ready(&self) -> bool {
        self.active_entity.is_some()
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct SceneSpawnState<'w, 's> {
    pending_markers: Query<'w, 's, (), (With<SceneSpawn>, Without<SceneSpawner>)>,
    spawners: Query<'w, 's, &'static SceneSpawner>,
}

impl SceneSpawnState<'_, '_> {
    pub fn ready(&self) -> bool {
        self.pending_markers.is_empty() && self.spawners.iter().all(SceneSpawner::is_ready)
    }
}

pub fn parented_world_pose(
    local_transform: &Transform,
    child_of: Option<&ChildOf>,
    parent_transforms: &Query<&Transform>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
    physics: &PhysicsWorld,
) -> (Vec3, Quat) {
    let mut translation = local_transform.translation;
    let mut rotation = local_transform.rotation;
    let mut current_parent = child_of.map(ChildOf::parent);

    while let Some(parent) = current_parent {
        if let Ok(handle) = parent_bodies.get(parent) {
            if let Some(body) = physics.rigid_body_set.get(handle.0) {
                translation = rb_pos(body) + rb_rot(body) * translation;
                rotation = rb_rot(body) * rotation;
                break;
            }
        }
        let Ok(parent_transform) = parent_transforms.get(parent) else {
            break;
        };
        translation = parent_transform.translation + parent_transform.rotation * translation;
        rotation = parent_transform.rotation * rotation;
        current_parent = parent_parents.get(parent).ok().map(ChildOf::parent);
    }

    (translation, rotation)
}

// ── pending hull collider queue ───────────────────────────────────────────────

/// Pending convex-hull colliders for static level geometry, waiting for the mesh asset to load.
#[derive(Resource, Default)]
pub struct PendingHullColliders(pub Vec<(Entity, Vec3, Quat, Handle<ConvexHullAsset>)>);

// ── network transfer helper ───────────────────────────────────────────────────

/// Compressed raw .scn.ron bytes to send to new clients on connect.
/// Server-only; set in the level-load startup system.
#[derive(Resource, Clone)]
pub struct LevelBytes {
    pub hash: String,
    pub compressed: Vec<u8>,
}

pub fn load_level_source(path: &str, asset_dir: &str) -> LevelBytes {
    if path.starts_with("sha256:") {
        return load_remote_level(path);
    }
    let fs_path = format!("{asset_dir}/{path}");
    read_and_compress_level(&fs_path)
}

/// Reads a .scn.ron file and returns it as compressed bytes for network transfer.
pub fn read_and_compress_level(path: &str) -> LevelBytes {
    let raw =
        std::fs::read(path).unwrap_or_else(|e| panic!("Failed to read level \"{path}\": {e}"));
    compress_level_bytes(&raw)
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

pub fn map_cache_dir() -> PathBuf {
    if std::path::Path::new("map_cache").exists() || !cfg!(debug_assertions) {
        PathBuf::from("map_cache")
    } else {
        PathBuf::from("../map_cache")
    }
}

pub fn map_cache_path(hash: &str) -> PathBuf {
    map_cache_dir().join(format!("{}.zst", sanitize_hash(hash)))
}

pub fn read_cached_map(hash: &str) -> Option<Vec<u8>> {
    std::fs::read(map_cache_path(hash)).ok()
}

pub fn write_cached_map(hash: &str, compressed: &[u8]) {
    std::fs::create_dir_all(map_cache_dir()).expect("map cache dir create failed");
    std::fs::write(map_cache_path(hash), compressed).expect("map cache write failed");
}

pub fn compressed_level_hash(compressed: &[u8]) -> Option<String> {
    let bytes = zstd::stream::decode_all(compressed).ok()?;
    Some(format!("sha256:{}", hex_sha256(&bytes)))
}

fn load_remote_level(hash: &str) -> LevelBytes {
    if let Some(compressed) = read_cached_map(hash) {
        return LevelBytes {
            hash: hash.to_string(),
            compressed,
        };
    }
    let url = format!("{}/assets/{}", common::config::BEACON_URL, hash);
    let response = ureq::get(&url)
        .call()
        .unwrap_or_else(|err| panic!("failed to fetch level {hash}: {err}"));
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)
        .unwrap_or_else(|err| panic!("failed to read level {hash}: {err}"));
    let level = compress_level_bytes(&bytes);
    write_cached_map(&level.hash, &level.compressed);
    level
}

fn compress_level_bytes(raw: &[u8]) -> LevelBytes {
    LevelBytes {
        hash: format!("sha256:{}", hex_sha256(raw)),
        compressed: zstd::stream::encode_all(raw, 3).expect("level: zstd compress failed"),
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn sanitize_hash(hash: &str) -> String {
    hash.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | ':' => ch,
            _ => '_',
        })
        .collect()
}

// ── plugin ────────────────────────────────────────────────────────────────────

pub struct LevelPlugin;
impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ColliderShape>();
        app.register_type::<StaticCollider>();
        app.register_type::<SpawnPoint>();
        app.register_type::<SceneSpawn>();
        app.register_type::<MapMeta>();
        app.init_resource::<PendingHullColliders>();
        // react to scene-spawned components — works on both client and server
        app.add_systems(Update, (spawn_static_colliders, spawn_hull_colliders));

        // Keep authored scene data as small marker components and route all runtime setup
        // through the existing imperative GameObject spawn path.
        app.add_systems(Update, init_scene_spawners);
        app.add_systems(FixedUpdate, tick_scene_spawners);
    }
}

// ── systems ───────────────────────────────────────────────────────────────────

fn queue_scene_spawn(
    spawn_entity: Entity,
    position: Vec3,
    rotation: Quat,
    kind: common::GameObjectKind,
    starting_velocity: Vec3,
    commands: &mut Commands,
    net_id_res: &mut ResMut<net::message::NetworkIDResource>,
) -> net::message::SpawnCommand {
    let net_id = net::message::NetworkID(net_id_res.next());
    let spawn_cmd = net::message::SpawnCommand {
        net_id,
        position,
        rotation,
        starting_velocity,
        server_tick: 0,
        kind,
    };
    commands.queue(crate::SpawnGameObjectCommand {
        entity: spawn_entity,
        cmd: spawn_cmd.clone(),
    });
    spawn_cmd
}

fn init_scene_spawners(
    query: Query<
        (Entity, &SceneSpawn, Option<&InitialVelocity>),
        (Added<SceneSpawn>, Without<SceneSpawner>),
    >,
    mut commands: Commands,
) {
    for (entity, scene_spawn, initial_velocity) in query.iter() {
        commands.entity(entity).insert(SceneSpawner {
            kind: scene_spawn.kind.clone(),
            starting_velocity: initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            respawn_delay_secs: scene_spawn.respawn_delay_secs,
            respawn_timer_secs: 0.0,
            active_entity: None,
        });
    }
}

fn tick_scene_spawners(
    spawners_exist: Query<(), With<SceneSpawner>>,
    existing: Query<(), ()>,
    mut spawners: Query<(Entity, &Transform, Option<&ChildOf>, &mut SceneSpawner)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    time: Res<Time<Fixed>>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    let should_spawn = is_server.is_some()
        || state.is_some_and(|s| *s.get() == common::game_state::GameState::SinglePlayer);
    if spawners_exist.is_empty() || !should_spawn {
        return;
    }

    for (_spawner_entity, local_transform, child_of, mut spawner) in spawners.iter_mut() {
        if let Some(active_entity) = spawner.active_entity {
            if existing.get(active_entity).is_ok() {
                continue;
            }
            spawner.active_entity = None;
            spawner.respawn_timer_secs = spawner.respawn_delay_secs;
        }

        if spawner.respawn_timer_secs > 0.0 {
            spawner.respawn_timer_secs = (spawner.respawn_timer_secs - time.delta_secs()).max(0.0);
            if spawner.respawn_timer_secs > 0.0 {
                continue;
            }
        }

        let (position, rotation) = parented_world_pose(
            local_transform,
            child_of,
            &parent_transforms,
            &parent_parents,
            &parent_bodies,
            &physics,
        );
        let inherited_velocity = child_of
            .and_then(|child_of| parent_bodies.get(child_of.parent()).ok())
            .and_then(|handle| physics.rigid_body_set.get(handle.0))
            .map(|rb| {
                let linear = rb_vel(rb);
                let angular = rb_angvel(rb);
                let offset = position - rb_pos(rb);
                linear + angular.cross(offset)
            })
            .unwrap_or(Vec3::ZERO);
        let spawn_entity = commands.spawn_empty().id();
        let spawn_cmd = queue_scene_spawn(
            spawn_entity,
            position,
            rotation,
            spawner.kind.clone(),
            spawner.starting_velocity + inherited_velocity,
            &mut commands,
            &mut net_id_res,
        );
        spawner.active_entity = Some(spawn_entity);

        if is_server.is_some() {
            if let Some(quic) = quic.as_mut() {
                quic.send(
                    net::quic::SendTarget::All,
                    net::quic::Channel::Ordered,
                    &net::message::MsgType::SpawnCommand(spawn_cmd),
                );
            }
        }
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
            // Hash refs download into a persistent local cache, so Bevy still loads a normal file path.
            let path = crate::asset_path::resolve_asset_path(path);
            let handle =
                asset_server.load_with_settings(path, move |settings: &mut f32| *settings = s);
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
            .spawn((
                SceneRoot(asset_server.load(crate::asset_path::resolve_asset_path(scene_path))),
                *transform,
            ))
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
