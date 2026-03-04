use super::super::physics::physics_world::*;
use super::pawn::*;
use bevy::prelude::*;
use bevy::prelude::{Assets, Mesh, StandardMaterial, Mesh3d, MeshMaterial3d, Visibility, Color}; // weirdly, this errors in RustRover's lsp
use rapier3d::prelude::*;

/// Spawns a physics-only biped on the server (no mesh or material).
pub fn spawn_server(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        BipedPawnComponent,
        Transform::from(transform),
    )).id();
    let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    entity
}

pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands
        .spawn((
            BipedPawnComponent,
            CameraRigComponent { offset: Vec3::new(0.0, 1.0, 3.0) },
            Transform::from(transform),
            Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.8, 0.8, 0.8))),
            Visibility::default(),
        ))
        .id();

    let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    entity
}

/// Spawns another player's pawn on the client: physics body + visible mesh, no Possessed/camera.
/// Participates fully in the local physics simulation so reconciliation replay is correct.
pub fn spawn_ghost(
    transform: Transform,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands
        .spawn((
            BipedPawnComponent,
            Transform::from(transform),
            Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.9, 0.4, 0.1))),
            Visibility::default(),
        ))
        .id();

    let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    entity
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &PhysicsBodyHandle,
    input: PawnInput,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };

    let rotation = body.rotation();
    let local_right   = rotation * Vector3::new(1.0, 0.0, 0.0);
    let local_up      = rotation * Vector3::new(0.0, 1.0, 0.0);
    let local_forward = rotation * Vector3::new(0.0, 0.0, 1.0);

    let impulse = (local_right * input.right + local_up * input.up + local_forward * input.forward) * 0.2;

    body.apply_impulse(impulse, true);
}