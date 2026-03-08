use crate::physics::physics_world::*;
use super::pawn::*;
use crate::net::message::NetworkID;
use bevy::prelude::*;
use rapier3d::prelude::*;

/// Two weapon slots on a biped pawn. Stored on the entity, not globally.
#[derive(Component, Default)]
pub struct WeaponSlots {
    pub slots: [Option<NetworkID>; 2],
    pub active: usize,
}

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

/// Spawns a biped with physics only. Used by both server and client.
pub fn spawn(
    transform: Transform,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let entity = commands.spawn((
        BipedPawnComponent,
        WeaponSlots::default(),
        Transform::from(transform),
    )).id();
    insert_biped_physics(entity, &transform, commands, world);
    entity
}

/// Adds a mesh and material to an existing biped entity.
#[cfg(feature = "client")]
pub fn add_visuals(
    entity: Entity,
    color: Color,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.entity(entity).insert((
        Mesh3d(meshes.add(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(color)),
        Visibility::default(),
    ));
}

/// Sets up the YawPivot → PitchPivot → Camera hierarchy on an existing biped entity.
/// Pass the pre-existing Camera3d entity so it gets re-parented rather than re-spawned.
#[cfg(feature = "client")]
pub fn setup_camera_rig(entity: Entity, camera: Option<Entity>, commands: &mut Commands) {
    let pitch_pivot = commands.spawn((
        PitchPivot { pitch: 0.0 },
        Transform::default(),
        Visibility::default(),
    )).id();

    if let Some(cam) = camera {
        commands.entity(cam).insert(Transform::default());
        commands.entity(pitch_pivot).add_child(cam);
    }

    let yaw_pivot = commands.spawn((
        YawPivot { yaw: 0.0 },
        Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)),
        Visibility::default(),
    )).add_child(pitch_pivot).id();

    commands.entity(entity).add_child(yaw_pivot);
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &PhysicsBodyHandle,
    input: PawnInput,
    _biped: &mut BipedPawnComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };

    let facing = *body.rotation() * bevy::math::Quat::from_rotation_y(input.look_yaw);
    let right   = facing * bevy::math::Vec3::X;
    let up      = facing * bevy::math::Vec3::Y;
    let forward = facing * bevy::math::Vec3::NEG_Z;

    let v = (right * input.right + up * input.up + forward * input.forward) * 0.2;
    let impulse = Vector::new(v.x, v.y, v.z);

    body.apply_impulse(impulse, true);
}
