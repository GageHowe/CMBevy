use bevy::prelude::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use crate::physics::physics_world::{PhysicsBodyHandle, PhysicsWorld};
use crate::physics::convex_hull_asset::{ConvexHullAsset, ConvexHullPlugin};
use crate::game_objects::GameObjectKind;

#[derive(Clone, Serialize, Deserialize)]
pub enum ColliderShape {
    Cuboid(Vec3),
    Ball(f32),
    Capsule { half_height: f32, radius: f32 },
    /// Path to an OBJ file (relative to assets/) containing VHACD convex hulls.
    ConvexHulls(String),
}

#[derive(Resource, Default)]
struct PendingHullColliders(Vec<(Vec3, Quat, Handle<ConvexHullAsset>)>);

#[derive(Clone, Serialize, Deserialize)]
pub struct StaticCollider {
    pub position: Vec3,
    pub rotation: Quat,
    pub shape: ColliderShape,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LevelSpawnRequest {
    pub kind: GameObjectKind,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Describes everything needed to load a level.
/// Serializable — can be loaded from JSON and in the future sent over the network.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct LevelDescription {
    /// Path to the GLB scene file loaded on the client for visuals.
    pub scene_path: String,
    /// Static (fixed) physics bodies. Spawned on both client and server.
    pub static_colliders: Vec<StaticCollider>,
    /// Objects to spawn at level start. The server processes these on startup.
    pub spawn_requests: Vec<LevelSpawnRequest>,
}

impl LevelDescription {
    pub fn from_json(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }
}

pub struct LevelPlugin(pub LevelDescription);

impl LevelPlugin {
    /// Load from a JSON file, falling back to `default_level()` on error.
    pub fn load(path: &str) -> Self {
        let level = LevelDescription::from_json(path).unwrap_or_else(|e| {
            eprintln!("Failed to load level from \"{path}\": {e}. Using default.");
            default_level()
        });
        LevelPlugin(level)
    }
}

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ConvexHullPlugin);
        app.insert_resource(self.0.clone());
        app.init_resource::<PendingHullColliders>();
        app.add_systems(Startup, spawn_static_colliders);
        app.add_systems(Update, spawn_hull_colliders);
        app.add_systems(Startup, load_level_scene.run_if(resource_exists::<AssetServer>));
    }
}

fn spawn_static_colliders(
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut pending: ResMut<PendingHullColliders>,
    asset_server: Res<AssetServer>,
    level: Res<LevelDescription>,
) {
    for sc in &level.static_colliders {
        if let ColliderShape::ConvexHulls(path) = &sc.shape {
            pending.0.push((sc.position, sc.rotation, asset_server.load(path.clone())));
            continue;
        }
        let collider = match &sc.shape {
            ColliderShape::Cuboid(he) => ColliderBuilder::cuboid(he.x, he.y, he.z).build(),
            ColliderShape::Ball(r) => ColliderBuilder::ball(*r).build(),
            ColliderShape::Capsule { half_height, radius } => {
                ColliderBuilder::capsule_y(*half_height, *radius).build()
            }
            ColliderShape::ConvexHulls(_) => unreachable!(),
        };
        spawn_fixed_body(sc.position, sc.rotation, collider, &mut commands, &mut world);
    }
}

fn spawn_hull_colliders(
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
    commands.entity(entity).insert(PhysicsBodyHandle(handle));
}

fn load_level_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    level: Res<LevelDescription>,
) {
    commands.spawn((
        SceneRoot(asset_server.load(level.scene_path.clone())),
        Transform::default(),
    ));
}

pub fn default_level() -> LevelDescription {
    LevelDescription {
        scene_path: "models/companion_cube.glb#Scene0".into(),
        static_colliders: vec![
            StaticCollider {
                position: Vec3::new(0.0, -10.0, 0.0),
                rotation: Quat::IDENTITY,
                shape: ColliderShape::Cuboid(Vec3::new(10.0, 2.0, 10.0)),
            },
        ],
        spawn_requests: vec![
            LevelSpawnRequest {
                kind: GameObjectKind::Rifle,
                position: Vec3::new(3.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
            },
            LevelSpawnRequest {
                kind: GameObjectKind::Shotgun,
                position: Vec3::new(-3.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
            },
        ],
    }
}
