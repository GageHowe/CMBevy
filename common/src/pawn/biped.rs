use super::super::physics::physics_world::*;
use super::pawn::*;
use bevy::prelude::*;
use rapier3d::prelude::*;

pub fn spawn(
    transform: Transform,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world: ResMut<PhysicsWorld>,
) {
    let pawn_entity = commands
        .spawn((
            BipedPawnComponent,
            CameraRigComponent {
                offset: Vec3::new(0.0, 1.0, 3.0),
            },
            Possessed::new(60),
            Transform::from_translation(transform.translation),
            Mesh3d(meshes.add(Cuboid::new(
                transform.scale.x,
                transform.scale.y,
                transform.scale.z,
            ))),
            MeshMaterial3d(materials.add(Color::srgb(1.0, 1.0, 1.0))),
            Visibility::default(),
        ))
        .id();

    let rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .build();
    let rb_handle = world.insert_body(pawn_entity, rb);

    let collider = ColliderBuilder::cuboid(
        transform.scale.x * 0.5,
        transform.scale.y * 0.5,
        transform.scale.z * 0.5,
    )
        .build();

    commands
        .entity(pawn_entity)
        .insert(PhysicsBodyHandle(rb_handle));

    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &PhysicsBodyHandle,
    input: PawnInputComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };

    // get the pawn's current orientation from rapier
    let rotation = body.rotation();
    let local_right   = rotation * Vector3::new(1.0, 0.0, 0.0);
    let local_up      = rotation * Vector3::new(0.0, 1.0, 0.0);
    let local_forward = rotation * Vector3::new(0.0, 0.0, 1.0);

    let impulse = local_right   * input.right
        + local_up      * input.up
        + local_forward * input.forward;

    body.apply_impulse(impulse, true);
}