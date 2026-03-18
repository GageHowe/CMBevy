use crate::GameObject;
use physics::physics_world::*;
use super::pawn::*;
use bevy::prelude::*;
use rapier3d::prelude::*;

impl Pawn for SpaceshipPawnComponent {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponenet, input: PawnInput) {
        apply_spaceship_movement(world, body, input, self);
    }
}

impl GameObject for SpaceshipPawnComponent {
    fn spawn_physics(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
        let entity = commands.spawn((SpaceshipPawnComponent, Transform::from(transform))).id();
        let rb = RigidBodyBuilder::dynamic().translation(transform.translation).build();
        let rb_handle = world.insert_body(entity, rb);
        commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
        entity
    }
    fn cleanup() {}
    fn get_rigidbody() -> Option<RigidBody> {
        Some(RigidBodyBuilder::dynamic().build())
    }
}

pub fn apply_spaceship_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponenet,
    input: PawnInput,
    _spaceship: &mut SpaceshipPawnComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };

    // get the pawn's current orientation from rapier
    let rotation = body.rotation();
    let local_right   = rotation * Vector3::new(1.0, 0.0, 0.0);
    let local_up      = rotation * Vector3::new(0.0, 1.0, 0.0);
    let local_forward = rotation * Vector3::new(0.0, 0.0, 1.0);

    let impulse = (local_right * input.right + local_up * input.up + local_forward * input.forward) * 0.2;

    body.apply_impulse(impulse, true);
}
