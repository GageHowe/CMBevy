use super::super::physics::physics_world::*;
use super::pawn::*;
use bevy::prelude::*;
use bevy::prelude::{Assets, Mesh, StandardMaterial, Mesh3d, MeshMaterial3d, Visibility, Color}; // weirdly, this errors in RustRover's lsp
use rapier3d::prelude::*;

fn insert_biped_physics(entity: Entity, transform: &Transform, commands: &mut Commands, world: &mut PhysicsWorld) {
    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(10.0)
        .build();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
}

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
    insert_biped_physics(entity, &transform, commands, world);
    entity
}

/// Spawns the locally-possessed biped on the client with the camera pivot hierarchy:
///   pawn → YawPivot → PitchPivot → (existing camera entity)
///
/// Pass the pre-existing Camera3d entity so it gets re-parented rather than re-spawned.
/// Its Transform is reset to identity so it sits at the pivot origin.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
    camera: Option<Entity>,
) -> Entity {
    let pitch_pivot = commands.spawn((
        PitchPivot { pitch: 0.0 },
        Transform::default(),
        Visibility::default(),
    )).id();

    if let Some(cam) = camera {
        // Reset the camera's local transform — its world position is now driven by the hierarchy.
        commands.entity(cam).insert(Transform::default());
        commands.entity(pitch_pivot).add_child(cam);
    }

    let yaw_pivot = commands.spawn((
        YawPivot,
        // Eye height in pawn-local space. Always "up" relative to the pawn surface.
        Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)),
        Visibility::default(),
    )).add_child(pitch_pivot).id();

    let entity = commands
        .spawn((
            BipedPawnComponent,
            Transform::from(transform),
            Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(Color::srgb(0.8, 0.8, 0.8))),
            Visibility::default(),
        ))
        .add_child(yaw_pivot)
        .id();

    insert_biped_physics(entity, &transform, commands, world);
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

    insert_biped_physics(entity, &transform, commands, world);
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

    // Reconstruct world-space facing: body_rotation * local_yaw.
    // Using body rotation from physics means the server always has consistent state
    // without needing to trust the client's world-space transform.
    let facing = *body.rotation() * bevy::math::Quat::from_rotation_y(input.look_yaw);
    let right   = facing * bevy::math::Vec3::X;
    let up      = facing * bevy::math::Vec3::Y;
    let forward = facing * bevy::math::Vec3::NEG_Z;

    let v = (right * input.right + up * input.up + forward * input.forward) * 0.2;
    let impulse = Vector::new(v.x, v.y, v.z);

    body.apply_impulse(impulse, true);
}
