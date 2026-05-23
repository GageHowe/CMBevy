use std::path::PathBuf;

#[cfg(feature = "client")]
use bevy::light::AmbientLight;
use bevy::{
    prelude::*,
    scene::{DynamicSceneRoot, serde::SceneDeserializer},
};
use physics::{
    collider_shape::AuthoredColliderShape as Shape,
    convex_hull_asset::ConvexHullAsset,
    physics_world::{
        InitialAngularVelocity, InitialVelocity, PhysicsWorld, RigidBodyHandleComponent,
        SceneRigidBody, rb_angvel, rb_pos, rb_rot, rb_vel,
    },
};
use rapier3d::prelude::*;
use serde::de::DeserializeSeed;
use sha2::{Digest, Sha256};

#[cfg(feature = "client")]
use crate::debug_draw::draw_authored_shape;
#[cfg(feature = "client")]
use crate::zone_effects::{ZoneEffect, ZoneEffectKind};
use crate::{
    AuthoritySystems,
    gc::{SpawnerGc, WorldObjectGc},
    lifecycle::spawn_game_object,
};

mod preprocess;

// ── component / resource types ────────────────────────────────────────────────

/// Static (fixed) collider placed in the scene. Position/rotation come from Transform.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct StaticCollider {
    pub shape: Shape,
    pub scale: f32,
}

/// Scene-authored surface material for level colliders.
#[derive(Component, Clone, Copy, Reflect, Default)]
#[reflect(Component, Default)]
pub struct ColliderMaterial {
    pub friction: f32,
    pub restitution: f32,
}

/// Client-only GLB scene attached to an authored map entity.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct SceneModel {
    pub path: String,
}

/// Player/bot spawn point placed in the scene. Position/rotation come from Transform.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct SpawnPoint {
    pub team: u8,
}

/// Map-authored string tags visible to scripts. Use these to label important entities
/// (flag bases, hills, doors, spawn groups) so gametype scripts can look them up.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct ScriptTags {
    pub tags: Vec<String>,
}

/// Map-authored script-visible trigger volume. These are evaluated by querying the physics world
/// with the authored shape at the entity's current world pose, so they can be parented in scenes
/// without needing their own rigid body.
#[derive(Component, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct ScriptZone {
    pub shape: Shape,
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
    /// Ambient fill light color applied to the main camera. Client-only.
    pub ambient_light_color: Color,
    /// Ambient fill light brightness applied to the main camera. Client-only.
    pub ambient_light_brightness: f32,
    /// Preferred count where GC starts getting more aggressive. Clamped to engine limits.
    pub gc_soft_cap: Option<usize>,
}

// ── scene-root marker ─────────────────────────────────────────────────────────

/// Marks the entity that owns the loaded DynamicScene.
/// The visual GLB scene is spawned as a child, so despawning this entity cleans everything up.
#[derive(Component)]
pub struct LevelSceneRoot;

#[derive(Component, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct Spawner {
    pub kind: common::GameObjectKind,
    pub respawn_delay_secs: f32,
}
impl Default for Spawner {
    fn default() -> Self {
        Self {
            kind: common::GameObjectKind::Biped,
            respawn_delay_secs: 10.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct SpawnerRuntime {
    pub(crate) kind: common::GameObjectKind,
    pub(crate) starting_velocity: Vec3,
    pub(crate) respawn_delay_secs: f32,
    pub(crate) respawn_timer_secs: f32,
    pub(crate) active_entity: Option<Entity>,
}

#[derive(bevy::ecs::system::SystemParam)]
pub struct LevelReadyState<'w, 's> {
    pending_map: Option<Res<'w, PendingMapScene>>,
    roots: Query<'w, 's, (), With<LevelSceneRoot>>,
    pending_hulls: Res<'w, PendingHullColliders>,
    pending_markers: Query<'w, 's, (), (With<Spawner>, Without<SpawnerRuntime>)>,
    scene_bodies: Query<
        'w,
        's,
        &'static SceneRigidBody,
        (
            With<RigidBodyHandleComponent>,
            Without<net::message::NetworkID>,
        ),
    >,
}

impl LevelReadyState<'_, '_> {
    pub fn ready(&self) -> bool {
        self.pending_map.is_none()
            && !self.roots.is_empty()
            && self.pending_hulls.0.is_empty()
            && self.pending_markers.is_empty()
            && !self.pending_scene_network_ids()
    }

    pub fn reason(&self) -> Option<&'static str> {
        if self.pending_map.is_some() {
            Some("map pending")
        } else if self.roots.is_empty() {
            Some("level root missing")
        } else if !self.pending_hulls.0.is_empty() {
            Some("hull colliders pending")
        } else if !self.pending_markers.is_empty() {
            Some("scene spawners initializing")
        } else if self.pending_scene_network_ids() {
            Some("scene network ids pending")
        } else {
            None
        }
    }

    fn pending_scene_network_ids(&self) -> bool {
        self.scene_bodies
            .iter()
            .any(|scene_body| matches!(scene_body, SceneRigidBody::Dynamic))
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

pub fn parent_body_handle(
    child_of: Option<&ChildOf>,
    parent_parents: &Query<&ChildOf>,
    parent_bodies: &Query<&RigidBodyHandleComponent>,
) -> Option<RigidBodyHandle> {
    let mut current_parent = child_of.map(ChildOf::parent);
    while let Some(parent) = current_parent {
        if let Ok(handle) = parent_bodies.get(parent) {
            return Some(handle.0);
        }
        current_parent = parent_parents.get(parent).ok().map(ChildOf::parent);
    }
    None
}

// ── pending hull collider queue ───────────────────────────────────────────────

/// Pending convex-hull colliders for static level geometry, waiting for the mesh asset to load.
#[derive(Resource, Default)]
pub struct PendingHullColliders(pub Vec<PendingHullCollider>);

/// Deferred convex-hull collider attachment waiting for the asset loader to finish parsing OBJ data.
pub struct PendingHullCollider {
    pub entity: Entity,
    pub position: Vec3,
    pub rotation: Quat,
    pub hull: Handle<ConvexHullAsset>,
    pub body_type: SceneRigidBody,
    pub initial_velocity: Vec3,
    pub initial_angvel: Vec3,
}

// ── network transfer helper ───────────────────────────────────────────────────

/// Compressed raw .scn.ron bytes to send to new clients on connect.
/// Server-only; set in the level-load startup system.
#[derive(Resource, Clone)]
pub struct LevelBytes {
    /// Content hash of the uncompressed scene bytes.
    pub hash: String,
    /// Zstd-compressed scene bytes transferred over the network.
    pub compressed: Vec<u8>,
}

pub fn load_level_source(path: &str, asset_dir: &std::path::Path) -> Result<LevelBytes, String> {
    if path.starts_with("sha256:") {
        return load_remote_level(path);
    }
    read_and_compress_level(asset_dir.join(path))
}

pub fn default_asset_dir() -> std::path::PathBuf {
    common::config::asset_dir()
}

/// Reads a .scn.ron file and returns it as compressed bytes for network transfer.
pub fn read_and_compress_level(path: impl AsRef<std::path::Path>) -> Result<LevelBytes, String> {
    let path = path.as_ref();
    let raw = std::fs::read(path)
        .map_err(|e| format!("Failed to read level \"{}\": {e}", path.display()))?;
    let preprocessed = preprocess::preprocess_level_bytes(&raw)?;
    Ok(compress_level_bytes(&preprocessed))
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
            eprintln!("map decompress: {e}");
            crate::messages::push_world(world, format!("Map load failed: {e}"));
            return;
        }
    };
    let bytes = match preprocess::preprocess_level_bytes(&bytes) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("map preprocess: {e}");
            crate::messages::push_world(world, format!("Map load failed: {e}"));
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
            eprintln!("map ron: {e}");
            crate::messages::push_world(world, format!("Map load failed: {e}"));
            return;
        }
    };
    let scene = match scene_de.deserialize(&mut ron_de) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("map deserialize: {e}");
            crate::messages::push_world(world, format!("Map load failed: {e}"));
            return;
        }
    };
    drop(registry_guard);
    let handle = world.resource_mut::<Assets<DynamicScene>>().add(scene);
    world.spawn((DynamicSceneRoot(handle), LevelSceneRoot));
    crate::messages::push_world(world, "Map loaded");
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

fn load_remote_level(hash: &str) -> Result<LevelBytes, String> {
    if let Some(compressed) = read_cached_map(hash) {
        return Ok(LevelBytes {
            hash: hash.to_string(),
            compressed,
        });
    }
    let url = format!("{}/assets/{}", common::config::BEACON_URL, hash);
    let response = ureq::get(&url)
        .call()
        .map_err(|err| format!("failed to fetch level {hash}: {err}"))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)
        .map_err(|err| format!("failed to read level {hash}: {err}"))?;
    let level = compress_level_bytes(&bytes);
    write_cached_map(&level.hash, &level.compressed);
    Ok(level)
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
        app.register_type::<ChildOf>();
        app.register_type::<Shape>();
        app.register_type::<StaticCollider>();
        app.register_type::<ColliderMaterial>();
        app.register_type::<SceneModel>();
        app.register_type::<SpawnPoint>();
        app.register_type::<ScriptTags>();
        app.register_type::<ScriptZone>();
        app.register_type::<Spawner>();
        app.register_type::<MapMeta>();
        app.init_resource::<PendingHullColliders>();
        app.add_systems(Update, apply_pending_map_scene);
        // react to scene-spawned components — works on both client and server
        app.add_systems(
            Update,
            (
                spawn_static_colliders,
                spawn_hull_colliders,
                assign_scene_network_ids,
            ),
        );
        #[cfg(feature = "client")]
        {
            app.add_systems(
                Update,
                (
                    spawn_scene_models,
                    load_level_scene.run_if(resource_added::<MapMeta>),
                ),
            );
        }

        // Keep authored scene data as small marker components and route all runtime setup
        // through the existing imperative GameObject spawn path.
        app.add_systems(Update, init_spawners);
        app.add_systems(FixedUpdate, tick_spawners.in_set(AuthoritySystems));
    }
}

// ── systems ───────────────────────────────────────────────────────────────────

fn init_spawners(
    query: Query<
        (Entity, &Spawner, Option<&InitialVelocity>),
        (Added<Spawner>, Without<SpawnerRuntime>),
    >,
    mut commands: Commands,
) {
    for (entity, spawner, initial_velocity) in query.iter() {
        commands.entity(entity).insert(SpawnerRuntime {
            kind: spawner.kind.clone(),
            starting_velocity: initial_velocity.map_or(Vec3::ZERO, |v| v.0),
            respawn_delay_secs: spawner.respawn_delay_secs,
            respawn_timer_secs: 0.0,
            active_entity: None,
        });
    }
}

fn assign_scene_network_ids(
    query: Query<
        (
            Entity,
            &SceneRigidBody,
            &Transform,
            Option<&net::message::NetworkID>,
        ),
        Added<RigidBodyHandleComponent>,
    >,
    mut commands: Commands,
    mut net_ids: ResMut<net::message::NetworkIDResource>,
) {
    // TODO: Replace this transform-order derived id assignment with explicit authored ids or a
    // deterministic scene hashing scheme. Sorting by translation is a fragile hidden contract.
    let mut bodies: Vec<_> = query
        .iter()
        .filter(|(_, scene_body, _, _)| matches!(scene_body, SceneRigidBody::Dynamic))
        .collect();
    bodies.sort_by_key(|(_, _, transform, _)| {
        let t = transform.translation;
        (t.x.to_bits(), t.y.to_bits(), t.z.to_bits())
    });
    for (index, (entity, _, _, current_id)) in bodies.into_iter().enumerate() {
        let id = (index + 1) as u64;
        net_ids.reserve(id);
        if current_id.is_none() {
            commands.entity(entity).insert(net::message::NetworkID(id));
        }
    }
}

fn tick_spawners(
    spawners_exist: Query<(), With<SpawnerRuntime>>,
    existing: Query<(), ()>,
    mut spawners: Query<(Entity, &Transform, Option<&ChildOf>, &mut SpawnerRuntime)>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    time: Res<Time<Fixed>>,
) {
    if spawners_exist.is_empty() {
        return;
    }

    #[cfg(feature = "client")]
    let _ = &mut quic;
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

        // Parented spawns authored on moving bodies must wait for the parent body
        // or they lose inherited velocity and start from the wrong frame.
        let parent_body = parent_body_handle(child_of, &parent_parents, &parent_bodies);
        if child_of.is_some() && parent_body.is_none() {
            continue;
        }
        let (position, rotation) = parented_world_pose(
            local_transform,
            child_of,
            &parent_transforms,
            &parent_parents,
            &parent_bodies,
            &physics,
        );
        let inherited_velocity = parent_body
            .and_then(|handle| physics.rigid_body_set.get(handle))
            .map(|rb| {
                let linear = rb_vel(rb);
                let angular = rb_angvel(rb);
                let offset = position - rb_pos(rb);
                linear + angular.cross(offset)
            })
            .unwrap_or(Vec3::ZERO);
        let (spawn_entity, _, spawn_cmd) = spawn_game_object(
            spawner.kind.clone(),
            position,
            rotation,
            spawner.starting_velocity + inherited_velocity,
            0,
            &mut commands,
            &mut net_id_res,
        );
        commands.queue(move |world: &mut World| {
            if world.get::<WorldObjectGc>(spawn_entity).is_some() {
                world.entity_mut(spawn_entity).insert(SpawnerGc {
                    spawner: _spawner_entity,
                });
            }
        });
        spawner.active_entity = Some(spawn_entity);

        #[cfg(feature = "client")]
        let _ = &spawn_cmd;
        #[cfg(not(feature = "client"))]
        {
            if let Some(quic) = quic.as_mut() {
                crate::lifecycle::send_spawn_command(
                    quic,
                    net::quic::SendTarget::All,
                    net::quic::Channel::Ordered,
                    spawn_cmd,
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
            Option<&ColliderMaterial>,
            &Transform,
            Option<&RigidBodyHandleComponent>,
            Option<&SceneRigidBody>,
            Option<&InitialVelocity>,
            Option<&InitialAngularVelocity>,
        ),
        Added<StaticCollider>,
    >,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    asset_server: Res<AssetServer>,
) {
    for (
        entity,
        sc,
        material,
        transform,
        body_handle,
        scene_body,
        initial_velocity,
        initial_angvel,
    ) in new_colliders.iter()
    {
        let s = sc.scale;
        let pos = transform.translation;
        let rot = transform.rotation;
        let linvel = initial_velocity.map_or(Vec3::ZERO, |v| v.0);
        let angvel = initial_angvel.map_or(Vec3::ZERO, |v| v.0);
        let body_type = scene_body.copied().unwrap_or_default();
        if let Shape::ConvexHulls(path) = &sc.shape {
            if body_handle.is_none() {
                ensure_body(
                    entity,
                    pos,
                    rot,
                    body_type,
                    linvel,
                    angvel,
                    &mut commands,
                    &mut world,
                );
            }
            // Hash refs download into a persistent local cache, so Bevy still loads a normal file path.
            let path = crate::asset_path::resolve_asset_path(path);
            let handle =
                asset_server.load_with_settings(path, move |settings: &mut f32| *settings = s);
            pending.0.push(PendingHullCollider {
                entity,
                position: pos,
                rotation: rot,
                hull: handle,
                body_type,
                initial_velocity: linvel,
                initial_angvel: angvel,
            });
            continue;
        }
        let Some(mut collider) = sc.shape.build_primitive_collider(s) else {
            continue;
        };
        apply_collider_material(&mut collider, material.copied());
        attach_collider_to_body(
            entity,
            pos,
            rot,
            collider,
            body_handle.map(|h| h.0),
            body_type,
            linvel,
            angvel,
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
    materials: Query<&ColliderMaterial>,
) {
    let ready: Vec<_> = pending
        .0
        .iter()
        .filter_map(|pending| {
            hull_assets.get(&pending.hull).map(|asset| {
                (
                    pending.entity,
                    pending.position,
                    pending.rotation,
                    asset.0.clone(),
                    materials.get(pending.entity).ok().copied(),
                    pending.body_type,
                    pending.initial_velocity,
                    pending.initial_angvel,
                )
            })
        })
        .collect();
    pending
        .0
        .retain(|pending| hull_assets.get(&pending.hull).is_none());
    for (entity, pos, rot, mut collider, material, body_type, linvel, angvel) in ready {
        let existing = world.entity_to_handle.get(&entity).copied();
        apply_collider_material(&mut collider, material);
        attach_collider_to_body(
            entity,
            pos,
            rot,
            collider,
            existing,
            body_type,
            linvel,
            angvel,
            &mut commands,
            &mut world,
        );
    }
}

#[cfg(feature = "client")]
pub fn spawn_scene_models(
    mut commands: Commands,
    models: Query<(Entity, &SceneModel), Added<SceneModel>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, model) in &models {
        commands.entity(entity).insert((
            SceneRoot(asset_server.load(crate::asset_path::resolve_asset_path(&model.path))),
            Visibility::default(),
        ));
    }
}

#[cfg(feature = "client")]
/// Draws a lightweight debug marker for script zones so proof-of-concept objectives such as
/// KOTH hills are visible without dedicated art.
pub fn draw_script_zone_debug(
    zones: Query<(
        &ScriptZone,
        Option<&ZoneEffect>,
        &Transform,
        Option<&ChildOf>,
    )>,
    parent_transforms: Query<&Transform>,
    parent_parents: Query<&ChildOf>,
    parent_bodies: Query<&RigidBodyHandleComponent>,
    physics: Res<PhysicsWorld>,
    mut gizmos: Gizmos,
) {
    for (zone, effect, transform, child_of) in &zones {
        let (center, _rotation) = parented_world_pose(
            transform,
            child_of,
            &parent_transforms,
            &parent_parents,
            &parent_bodies,
            &physics,
        );
        let color = match effect.map(|effect| &effect.kind) {
            Some(ZoneEffectKind::Safe) => Color::srgba(0.2, 1.0, 0.35, 0.95),
            Some(ZoneEffectKind::OutOfBounds) => Color::srgba(1.0, 0.25, 0.2, 0.95),
            None => Color::srgba(0.15, 0.85, 0.95, 0.95),
        };
        draw_authored_shape(&mut gizmos, &zone.shape, center, _rotation, color);
    }
}

fn apply_collider_material(collider: &mut Collider, material: Option<ColliderMaterial>) {
    let Some(material) = material else {
        return;
    };
    collider.set_friction(material.friction);
    collider.set_restitution(material.restitution);
}

fn attach_collider_to_body(
    entity: Entity,
    position: Vec3,
    rotation: Quat,
    collider: Collider,
    existing_handle: Option<RigidBodyHandle>,
    scene_body: SceneRigidBody,
    initial_velocity: Vec3,
    initial_angvel: Vec3,
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
            initial_angvel,
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
    initial_angvel: Vec3,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> RigidBodyHandle {
    if let Some(handle) = world.entity_to_handle.get(&entity).copied() {
        return handle;
    }
    let builder = match scene_body {
        SceneRigidBody::Fixed => RigidBodyBuilder::fixed(),
        SceneRigidBody::Dynamic => RigidBodyBuilder::dynamic(),
        SceneRigidBody::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
    };
    let rb = builder
        .translation(Vector3::new(position.x, position.y, position.z))
        .linvel(Vector3::new(
            initial_velocity.x,
            initial_velocity.y,
            initial_velocity.z,
        ))
        .angvel(Vector3::new(
            initial_angvel.x,
            initial_angvel.y,
            initial_angvel.z,
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

#[cfg(feature = "client")]
pub fn load_level_scene(
    scene_root: Query<Entity, With<LevelSceneRoot>>,
    mut cameras: Query<&mut AmbientLight, With<Camera3d>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    meta: Res<MapMeta>,
) {
    let Ok(root) = scene_root.single() else {
        return;
    };
    if let Ok(mut ambient) = cameras.single_mut() {
        ambient.color = meta.ambient_light_color;
        ambient.brightness = meta.ambient_light_brightness;
    }
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
