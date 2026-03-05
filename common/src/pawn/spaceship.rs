use super::super::physics::physics_world::*;
use super::pawn::*;
// use bevy::prelude::*;
use rapier3d::prelude::*;
// use bevy::prelude::Cuboid;

// also should i make a pawn trait so there's some more structure, so we know that a pawn needs to implement spawn,
//   consume_inputs (movement), etc?

pub fn apply_spaceship_movement(
    world: &mut PhysicsWorld,
    body_handle: &PhysicsBodyHandle,
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
