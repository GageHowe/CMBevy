use bevy::prelude::*;
use bevy::scene::DynamicSceneRoot;
use bevy::scene::serde::SceneDeserializer;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::{PhysicsWorld, RigidBodyHandleComponent};
use rapier3d::prelude::*;
use serde::de::DeserializeSeed;

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
    /// Path to the GLB visual scene file. Client-only.
    pub scene_path: String,
    pub scene_scale: Vec3,
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
            .register_type::<common::GameObjectKind>()
            .register_type::<MapMeta>()
            .init_resource::<PendingHullColliders>()
            // react to scene-spawned components — works on both client and server
            .add_systems(Update, (spawn_static_colliders, spawn_hull_colliders));

        // Convert scene-placeholder GameObjectKind entities into real physics objects.
        // Runs as a system (not a hook) so it executes after scene_spawner_system finishes —
        // using a hook caused scene_spawner_system to panic when it accessed an entity that
        // the hook had already despawned mid-write. SpawnGameObjectCommand inserts NetworkID
        // before GameObjectKind, so those entities are excluded by Without<NetworkID>.
        app.add_systems(Update, spawn_scene_objects);
    }
}

// ── systems ───────────────────────────────────────────────────────────────────

/// Converts scene-placeholder GameObjectKind entities into real physics objects.
/// On the server: always. On the client: only in SinglePlayer.
fn spawn_scene_objects(
    query: Query<
        (Entity, &common::GameObjectKind, &Transform),
        (
            Added<common::GameObjectKind>,
            Without<net::message::NetworkID>,
        ),
    >,
    mut commands: Commands,
    mut net_id_res: ResMut<net::message::NetworkIDResource>,
    state: Option<Res<State<common::game_state::GameState>>>,
    is_server: Option<Res<common::IsServer>>,
) {
    // On the client: only run in SinglePlayer
    if is_server.is_none() {
        if state.map_or(true, |s| {
            *s.get() != common::game_state::GameState::SinglePlayer
        }) {
            return;
        }
    }

    for (entity, kind, transform) in query.iter() {
        match kind {
            common::GameObjectKind::Biped
            | common::GameObjectKind::Rifle
            | common::GameObjectKind::Shotgun
            | common::GameObjectKind::HailMary
            | common::GameObjectKind::Spaceship => {}
            _ => continue,
        }
        let net_id = net::message::NetworkID(net_id_res.next());
        let new_entity = commands.spawn_empty().id();
        commands.queue(crate::SpawnGameObjectCommand {
            entity: new_entity,
            cmd: net::message::SpawnCommand {
                net_id,
                position: transform.translation,
                rotation: transform.rotation,
                starting_velocity: Vec3::ZERO,
                server_tick: 0,
                kind: kind.clone(),
            },
        });
        commands.entity(entity).despawn();
    }
}

/// Inserts a fixed physics body on each entity that has a StaticCollider component.
/// Reacts to Added<StaticCollider>, so it works regardless of how the entity was spawned.
pub fn spawn_static_colliders(
    new_colliders: Query<(Entity, &StaticCollider, &Transform), Added<StaticCollider>>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    asset_server: Res<AssetServer>,
) {
    for (entity, sc, transform) in new_colliders.iter() {
        let s = sc.scale;
        let pos = transform.translation;
        let rot = transform.rotation;
        if let ColliderShape::ConvexHulls(path) = &sc.shape {
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
        attach_fixed_body(entity, pos, rot, collider, &mut commands, &mut world);
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
        attach_fixed_body(entity, pos, rot, collider, &mut commands, &mut world);
    }
}

fn attach_fixed_body(
    entity: Entity,
    position: Vec3,
    rotation: Quat,
    collider: Collider,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let rb = RigidBodyBuilder::fixed()
        .translation(Vector3::new(position.x, position.y, position.z))
        .build();
    let handle = world.insert_body(entity, rb);
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
    let visual = commands
        .spawn((
            SceneRoot(asset_server.load(meta.scene_path.clone())),
            Transform::from_scale(meta.scene_scale),
        ))
        .id();
    // parent to the scene root so it despawns with it
    commands.entity(root).add_child(visual);
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
