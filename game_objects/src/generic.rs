// generic.rs — spawns arbitrary physics objects with an optional mesh and network ID.
use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::weapon::PendingHullCollider;
use common::NetworkID;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::*;

/// Collider source for `spawn_generic`. Either a primitive rapier shape (with friction/restitution
/// set directly on the builder) or a convex hull .obj loaded via the asset system.
pub enum GenericShape<'a> {
    Primitive(ColliderBuilder),
    Hull { path: &'static str, scale: f32, asset_server: &'a AssetServer, hull_assets: &'a Assets<ConvexHullAsset> },
}

/// Spawns a dynamic physics body with an optional mesh and optional NetworkID.
/// Pass `net_id: Some(...)` on the server so `broadcast_tick` picks it up automatically.
/// Pass `mesh: Some((mesh, material))` on the client for visuals; omit on the server.
pub fn spawn_generic(
    transform: Transform,
    shape: GenericShape<'_>,
    mesh: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    net_id: Option<NetworkID>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn(Transform::from(transform)).id();
    let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
    let rb_handle = world.insert_body(entity, rb);
    commands.entity(entity).insert(RigidBodyHandleComponent(rb_handle));

    match shape {
        GenericShape::Primitive(builder) => {
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
            collider_set.insert_with_parent(builder.build(), rb_handle, rigid_body_set);
        }
        GenericShape::Hull { path, scale, asset_server, hull_assets } => {
            let s = scale;
            let handle = asset_server.load_with_settings(path, move |settings: &mut f32| *settings = s);
            if let Some(hull) = hull_assets.get(&handle) {
                let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
                collider_set.insert_with_parent(hull.0.clone(), rb_handle, rigid_body_set);
            } else {
                commands.entity(entity).insert(PendingHullCollider(handle));
            }
        }
    }

    if let Some((mesh_h, mat_h)) = mesh {
        commands.entity(entity).insert((Mesh3d(mesh_h), MeshMaterial3d(mat_h), Visibility::default()));
    }
    if let Some(id) = net_id {
        commands.entity(entity).insert(id);
    }

    entity
}
