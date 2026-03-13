use bevy::prelude::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use crate::physics::physics_world::{PhysicsBodyHandle, PhysicsWorld};
use crate::physics::convex_hull_asset::ConvexHullAsset;
use crate::game_objects::GameObjectKind;
use crate::game_objects::planet::PlanetBehaviorComponent;

#[derive(Clone, Serialize, Deserialize)]
pub enum ColliderShape {
    Cuboid(Vec3),
    Ball(f32),
    Capsule { half_height: f32, radius: f32 },
    /// Path to an OBJ file (relative to assets/) containing VHACD convex hulls.
    ConvexHulls(String),
}

#[derive(Resource, Default)]
pub struct PendingHullColliders(pub Vec<(Vec3, Quat, Handle<ConvexHullAsset>)>);

/// Marks every entity spawned by level loading (static colliders, scene root).
/// Used to identify and despawn them on level unload.
#[derive(Component)]
pub struct LevelEntity;

fn one() -> f32 { 1.0 }
fn default_scale() -> Vec3 { Vec3::ONE }

#[derive(Clone, Serialize, Deserialize)]
pub struct StaticCollider {
    pub position: Vec3,
    pub rotation: Quat,
    pub shape: ColliderShape,
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LevelSpawnRequest {
    pub kind: GameObjectKind,
    pub position: Vec3,
    pub rotation: Quat,
    #[serde(default)]
    pub planet_params: Option<PlanetBehaviorComponent>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub position: Vec3,
    pub rotation: Quat,
    pub team: u8,
}

/// Describes everything needed to load a level.
/// Serializable — can be loaded from RON and in the future sent over the network.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct Map {
    /// Path to the GLB scene file loaded on the client for visuals.
    pub scene_path: String,
    #[serde(default = "default_scale")]
    pub scene_scale: Vec3,
    /// Static (fixed) physics bodies. Spawned on both client and server.
    pub static_colliders: Vec<StaticCollider>,
    /// Objects to spawn at level start. The server processes these on startup.
    pub initial_spawns: Vec<LevelSpawnRequest>,
    /// Player spawn points, grouped by team id.
    pub spawn_points: Vec<SpawnPoint>,
}

impl Map {
    pub fn from_ron(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(ron::from_str(&content)?)
    }

    pub fn to_compressed_ron(&self) -> Option<Vec<u8>> {
        let s = ron::to_string(self).ok()?;
        zstd::stream::encode_all(s.as_bytes(), 9).ok()
    }

    pub fn from_compressed_ron(bytes: &[u8]) -> Option<Self> {
        let data = zstd::stream::decode_all(bytes).ok()?;
        ron::from_str(std::str::from_utf8(&data).ok()?).ok()
    }
}

pub struct LevelPlugin(pub Map);

impl LevelPlugin {
    /// Load from a RON file, falling back to `default_level()` on error.
    pub fn load(path: &str) -> Self {
        let level = Map::from_ron(path).unwrap_or_else(|e| {
            eprintln!("Failed to load level from \"{path}\": {e}. Using default.");
            default_level()
        });
        LevelPlugin(level)
    }
}

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone());
        app.init_resource::<PendingHullColliders>();
        app.add_systems(Startup, spawn_static_colliders);
        app.add_systems(Update, spawn_hull_colliders);
    }
}

pub fn spawn_static_colliders(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    asset_server: Res<AssetServer>,
    level: Res<Map>,
) {
    for sc in &level.static_colliders {
        let s = sc.scale;
        if let ColliderShape::ConvexHulls(path) = &sc.shape {
            let handle = asset_server.load_with_settings(path.clone(), move |settings: &mut f32| *settings = s);
            pending.0.push((sc.position, sc.rotation, handle));
            continue;
        }
        let collider = match &sc.shape {
            ColliderShape::Cuboid(he) => ColliderBuilder::cuboid(he.x * s, he.y * s, he.z * s).build(),
            ColliderShape::Ball(r) => ColliderBuilder::ball(r * s).build(),
            ColliderShape::Capsule { half_height, radius } => {
                ColliderBuilder::capsule_y(half_height * s, radius * s).build()
            }
            ColliderShape::ConvexHulls(_) => unreachable!(),
        };
        spawn_fixed_body(sc.position, sc.rotation, collider, &mut commands, &mut world);
    }
}

pub fn spawn_hull_colliders(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
) {
    let ready: Vec<_> = pending.0.iter()
        .filter_map(|(pos, rot, h)| hull_assets.get(h).map(|a| (*pos, *rot, a.0.clone())))
        .collect();
    pending.0.retain(|(_, _, h)| hull_assets.get(h).is_none());
    for (pos, rot, collider) in ready {
        spawn_fixed_body(pos, rot, collider, &mut commands, &mut world);
    }
}

fn spawn_fixed_body(
    position: Vec3,
    rotation: Quat,
    collider: Collider,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) {
    let entity = commands.spawn_empty().id();
    let rb = RigidBodyBuilder::fixed()
        .translation(Vector3::new(position.x, position.y, position.z))
        .build();
    let handle = world.insert_body(entity, rb);
    if let Some(rb) = world.rigid_body_set.get_mut(handle) {
        rb.set_rotation(rotation, true);
    }
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, handle, rigid_body_set);
    commands.entity(entity).insert((PhysicsBodyHandle(handle), LevelEntity));
}

/// Spawns planet entities from the map's initial_spawns. Run on the client when
/// the Map resource is first added (both singleplayer and multiplayer).
pub fn spawn_level_planets(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    level: Res<Map>,
) {
    for req in &level.initial_spawns {
        if req.kind != GameObjectKind::Planet { continue; }
        let Some(params) = req.planet_params.clone() else { continue; };
        let transform = Transform::from_translation(req.position).with_rotation(req.rotation);
        crate::game_objects::planet::spawn(params, transform, &mut commands, &mut world);
    }
}

pub fn load_level_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    level: Res<Map>,
) {
    commands.spawn((
        SceneRoot(asset_server.load(level.scene_path.clone())),
        Transform::from_scale(level.scene_scale),
        LevelEntity,
    ));
}

pub fn cleanup_level(
    mut commands: Commands,
    level_entities: Query<Entity, With<LevelEntity>>,
    mut pending: ResMut<PendingHullColliders>,
) {
    for entity in level_entities.iter() {
        commands.entity(entity).despawn();
    }
    pending.0.clear();
    commands.remove_resource::<Map>();
}

pub fn default_level() -> Map {
    Map {
        scene_path: "models/companion_cube.glb#Scene0".into(),
        scene_scale: Vec3::ONE,
        static_colliders: vec![
            StaticCollider {
                position: Vec3::new(0.0, -10.0, 0.0),
                rotation: Quat::IDENTITY,
                shape: ColliderShape::Cuboid(Vec3::new(10.0, 2.0, 10.0)),
                scale: 1.0,
            },
        ],
        initial_spawns: vec![
            LevelSpawnRequest {
                kind: GameObjectKind::Rifle,
                position: Vec3::new(3.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
                planet_params: None,
            },
            LevelSpawnRequest {
                kind: GameObjectKind::Shotgun,
                position: Vec3::new(-3.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
                planet_params: None,
            },
        ],
        spawn_points: vec![
            SpawnPoint { position: Vec3::new(0.0, 5.0, 0.0), rotation: Quat::IDENTITY, team: 0 },
        ],
    }
}
