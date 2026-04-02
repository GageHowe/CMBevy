// generic.rs — spawns arbitrary physics objects with an optional mesh and network ID.
use bevy::prelude::*;
use common::NetworkID;
use physics::convex_hull_asset::ConvexHullAsset;
use physics::physics_world::*;
use rapier3d::prelude::*;

/// Attached to an entity when its convex hull collider is still loading.
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct PendingHullCollider(pub Handle<ConvexHullAsset>);

/// Attaches a convex hull if it's ready; otherwise inserts a temporary collider and
/// marks the entity so the real hull can replace it once the asset finishes loading.
pub fn attach_hull_collider(
    entity: Entity,
    body_handle: RigidBodyHandle,
    path: &'static str,
    scale: f32,
    fallback: ColliderBuilder,
    world: &mut World,
) {
    let handle = world
        .resource::<AssetServer>()
        .load_with_settings(path, move |settings: &mut f32| *settings = scale);
    let collider = world
        .resource::<Assets<ConvexHullAsset>>()
        .get(&handle)
        .map(|hull| hull.0.clone())
        .unwrap_or_else(|| {
            world
                .entity_mut(entity)
                .insert(PendingHullCollider(handle.clone()));
            fallback.build()
        });
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
pub enum GenericShape<'a> {
    Primitive(ColliderBuilder),
    Hull {
        path: &'static str,
        scale: f32,
        asset_server: &'a AssetServer,
        hull_assets: &'a Assets<ConvexHullAsset>,
    },
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
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .build();
    let rb_handle = world.insert_body(entity, rb);
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
        GenericShape::Hull {
            path,
            scale,
            asset_server,
            hull_assets,
        } => {
            let s = scale;
            let handle =
                asset_server.load_with_settings(path, move |settings: &mut f32| *settings = s);
            if let Some(hull) = hull_assets.get(&handle) {
                let PhysicsWorld {
                    collider_set,
                    rigid_body_set,
                    ..
                } = &mut *world;
                collider_set.insert_with_parent(hull.0.clone(), rb_handle, rigid_body_set);
            } else {
                commands.entity(entity).insert(PendingHullCollider(handle));
            }
        }
    }

    if let Some((mesh_h, mat_h)) = mesh {
        commands.entity(entity).insert((
            Mesh3d(mesh_h),
            MeshMaterial3d(mat_h),
            Visibility::default(),
        ));
    }
    if let Some(id) = net_id {
        commands.entity(entity).insert(id);
    }

    entity
}

pub fn swap_hull_colliders(
    mut commands: Commands,
    mut physics: ResMut<PhysicsWorld>,
    hull_assets: Res<Assets<ConvexHullAsset>>,
    pending: Query<(Entity, &PendingHullCollider, &RigidBodyHandleComponent)>,
) {
    let ready: Vec<_> = pending
        .iter()
        .filter_map(|(entity, pending, body)| {
            hull_assets
                .get(&pending.0)
                .map(|asset| (entity, body.0, asset.0.clone()))
        })
        .collect();

    for (entity, body_handle, collider) in ready {
        let Some(body) = physics.rigid_body_set.get(body_handle) else {
            commands.entity(entity).remove::<PendingHullCollider>();
            continue;
        };
        let old_colliders: Vec<_> = body.colliders().to_vec();
        let PhysicsWorld {
            collider_set,
            rigid_body_set,
            island_manager,
            ..
        } = &mut *physics;
        for collider_handle in old_colliders {
            collider_set.remove(collider_handle, island_manager, rigid_body_set, true);
        }
        collider_set.insert_with_parent(collider, body_handle, rigid_body_set);
        commands.entity(entity).remove::<PendingHullCollider>();
    }
}
