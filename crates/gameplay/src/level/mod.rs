use std::path::PathBuf;

use bevy::asset::AssetApp;
#[allow(unused_imports)]
use bevy::light::{AmbientLight, CascadeShadowConfig, DirectionalLight};
use bevy::{
    camera::visibility::Visibility,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
    world_serialization::{
        DynamicWorld, DynamicWorldRoot, WorldAssetRoot, serde::WorldDeserializer,
    },
};
use physics::{
    collider_shape::AuthoredColliderShape as Shape,
    convex_hull_asset::load_convex_hull_blocking,
    physics_world::{
        InitialAngularVelocity, InitialVelocity, PhysicsWorld, RigidBodyHandleComponent,
        SceneRigidBody, rb_angvel, rb_pos, rb_rot, rb_vel,
    },
};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize, de::DeserializeSeed};
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

// ── plugin ────────────────────────────────────────────────────────────────────

pub struct LevelPlugin;
impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        // TODO: figure out what this actually does
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
        app.register_type::<Visibility>();
        app.register_type::<DirectionalLight>();
        app.register_type::<CascadeShadowConfig>();
        app.register_type::<Mesh3d>();
        app.register_type::<MeshMaterial3d<StandardMaterial>>();
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.register_asset_reflect::<Mesh>();
        app.register_asset_reflect::<StandardMaterial>();

        // maybe we could make these systems observer/trigger-driven or manual
        // rather than on Update? seems wasteful to have them run so often.
        // we'll only load a new level infrequently TODO

        app.add_systems(Update, apply_pending_map_scene);
        // react to scene-spawned components — works on both client and server
        app.add_systems(Update, (spawn_static_colliders, assign_scene_network_ids));
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
        // through the existing imperative spawn path.
        app.add_systems(Update, init_spawners);
        app.add_systems(FixedUpdate, tick_spawners.in_set(AuthoritySystems));
    }
}

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

// script zones don't work yet

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

/// Marks the entity that owns the loaded DynamicWorld.
/// The visual GLB scene is spawned as a child, so despawning this entity cleans everything up.
#[derive(Component)]
pub struct LevelSceneRoot;

#[derive(Component, Clone, Reflect, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct Spawner {
    #[serde(alias = "kind")]
    pub spawn_name: String,
    pub respawn_delay_secs: f32,
}
impl Default for Spawner {
    fn default() -> Self {
        Self {
            spawn_name: "biped".into(),
            respawn_delay_secs: 10.0,
        }
    }
}

#[derive(Component)]
pub struct SpawnerRuntime {
    pub(crate) spawn_name: String,
    pub(crate) velocity: Vec3,
    pub(crate) respawn_delay_secs: f32,
    pub(crate) respawn_timer_secs: f32,
    pub(crate) active_entity: Option<Entity>,
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
    Ok(compress_level_bytes(&raw))
}

/// Compressed .scn.ron bytes received from the server, pending scene spawn.
/// can we make this just a Vec<u8>? inline it or make this a state
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
    let mut asset_server = world.resource::<AssetServer>().clone();
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry_guard = registry.read();
    let scene_de = WorldDeserializer {
        type_registry: &registry_guard,
        load_from_path: &mut asset_server,
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
    let handle = world.resource_mut::<Assets<DynamicWorld>>().add(scene);
    world.spawn((DynamicWorldRoot(handle), LevelSceneRoot));
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
            spawn_name: spawner.spawn_name.clone(),
            velocity: initial_velocity.map_or(Vec3::ZERO, |v| v.0),
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
            Option<&crate::net::message::NetworkID>,
        ),
        Added<RigidBodyHandleComponent>,
    >,
    mut commands: Commands,
    mut net_ids: ResMut<crate::net::message::NetworkIDResource>,
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
            commands
                .entity(entity)
                .insert(crate::net::message::NetworkID(id));
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
    mut net_id_res: ResMut<crate::net::message::NetworkIDResource>,
    mut quic: Option<ResMut<crate::net::quic::QuicManager>>,
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

        let parent_body = parent_body_handle(child_of, &parent_parents, &parent_bodies);
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
            spawner.spawn_name.as_str(),
            Some(position),
            Some(rotation),
            Some(spawner.velocity + inherited_velocity),
            None,
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
                quic.send(
                    crate::net::quic::SendTarget::All,
                    crate::net::quic::Channel::Ordered,
                    &crate::net::message::MsgType::SpawnCommand(spawn_cmd),
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
            let Some(mut collider) =
                load_convex_hull_blocking(common::config::asset_dir().join(path), s)
            else {
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

#[cfg(feature = "client")]
pub fn spawn_scene_models(
    mut commands: Commands,
    models: Query<(Entity, &SceneModel), Added<SceneModel>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, model) in &models {
        commands.entity(entity).insert((
            WorldAssetRoot(asset_server.load(model.path.clone())),
            Visibility::default(),
        ));
    }
}

#[cfg(feature = "client")]
/// Draws a lightweight debug marker for script zones so proof-of-concept objectives such as
/// Script zones are visible without dedicated art.
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
                WorldAssetRoot(asset_server.load(scene_path.clone())),
                *transform,
            ))
            .id();
        // parent to the scene root so it despawns with it
        commands.entity(root).add_child(entity);
    }
}

/// Despawns the level scene and removes level resources.
/// Bevy's DynamicWorldRoot component hook handles scene-entity cleanup on entity despawn.
pub fn cleanup_level(mut commands: Commands, scene_roots: Query<Entity, With<LevelSceneRoot>>) {
    for entity in scene_roots.iter() {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<MapMeta>();
}
