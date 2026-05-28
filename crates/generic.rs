// generic.rs — spawns arbitrary physics objects with an optional mesh and network ID.
use bevy::prelude::*;
use common::NetworkID;
use physics::{convex_hull_asset::load_convex_hull_blocking, physics_world::*};
use rapier3d::prelude::*;

#[cfg(feature = "client")]
pub type GenericMesh = (Handle<Mesh>, Handle<StandardMaterial>);
#[cfg(not(feature = "client"))]
pub type GenericMesh = ();

pub fn attach_hull_collider(
    _entity: Entity,
    body_handle: RigidBodyHandle,
    path: &'static str,
    scale: f32,
    fallback: ColliderBuilder,
    world: &mut World,
) {
    let collider = load_convex_hull_blocking(crate::asset_path::resolve_asset_file_path(path), scale)
        .unwrap_or_else(|| fallback.build());
    let mut physics = world.resource_mut::<PhysicsWorld>();
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *physics;
    collider_set.insert_with_parent(collider, body_handle, rigid_body_set);
}

/// Collider source for `spawn_generic`. Either a primitive rapier shape (with friction/restitution
/// set directly on the builder) or a convex hull .obj loaded via the asset system.
pub enum GenericShape {
    Primitive(ColliderBuilder),
    Hull {
        path: &'static str,
        scale: f32,
    },
}

/// Spawns a dynamic physics body with an optional mesh and optional NetworkID.
/// Pass `net_id: Some(...)` on the server so `broadcast_tick` picks it up automatically.
/// Pass `mesh: Some((mesh, material))` on the client for visuals; omit on the server.
pub fn spawn_generic(
    transform: Transform,
    shape: GenericShape,
    mesh: Option<GenericMesh>,
    net_id: Option<NetworkID>,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn(Transform::from(transform)).id();
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    if let Some(rb) = world.rigid_body_set.get_mut(rb_handle) {
        rb.set_rotation(transform.rotation, true);
    }
    commands
        .entity(entity)
        .insert(RigidBodyHandleComponent(rb_handle));

    match shape {
        GenericShape::Primitive(builder) => {
            let PhysicsWorld {
                collider_set,
                rigid_body_set,
                ..
            } = &mut *world;
            collider_set.insert_with_parent(builder.build(), rb_handle, rigid_body_set);
        }
        GenericShape::Hull { path, scale } => {
            let PhysicsWorld {
                collider_set,
                rigid_body_set,
                ..
            } = &mut *world;
            let collider =
                load_convex_hull_blocking(crate::asset_path::resolve_asset_file_path(path), scale)
                    .unwrap_or_else(|| ColliderBuilder::ball(0.5).build());
            collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
        }
    }

    #[cfg(feature = "client")]
    if let Some((mesh_h, mat_h)) = mesh {
        commands.entity(entity).insert((
            Mesh3d(mesh_h),
            MeshMaterial3d(mat_h),
            Visibility::default(),
        ));
    }
    #[cfg(not(feature = "client"))]
    let _ = mesh;
    if let Some(id) = net_id {
        commands.entity(entity).insert(id);
    }

    entity
}
