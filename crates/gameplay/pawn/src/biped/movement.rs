use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::*;

pub(super) fn ground_state(
    world: &PhysicsWorld,
    body_handle: RigidBodyHandle,
    capsule_pos: Vec3,
    planet_up: Vec3,
    is_sliding: bool,
) -> (bool, Vec3, Option<Entity>) {
    // The crouched capsule is top-aligned, so its bottom is at the body origin (SLIDE_CAPSULE_BOTTOM = 0).
    let bottom_offset = if is_sliding {
        SLIDE_CAPSULE_BOTTOM
    } else {
        CAPSULE_BOTTOM
    };
    let sphere_origin = capsule_pos - planet_up * (bottom_offset - CAPSULE_RADIUS * 0.9);
    let Some(self_entity) = world.handle_to_entity.get(&body_handle).copied() else {
        return (false, Vec3::ZERO, None);
    };
    if let Some((support_entity, ch, toi, normal)) = world.cast_sphere_ignoring_shields(
        sphere_origin,
        -planet_up,
        CAPSULE_RADIUS * 0.9,
        GROUND_DIST,
        &[self_entity],
    ) {
        let support_body = world.collider_set.get(ch).and_then(|col| col.parent());
        let vel = support_body
            .and_then(|rb_h| world.rigid_body_set.get(rb_h))
            .map(|rb| {
                let contact = sphere_origin + (-planet_up * toi) - normal * (CAPSULE_RADIUS * 0.9);
                rb_vel(rb) + rb_angvel(rb).cross(contact - rb_pos(rb))
            })
            .unwrap_or(Vec3::ZERO);
        (true, vel, Some(support_entity))
    } else {
        (false, Vec3::ZERO, None)
    }
}

pub(super) fn make_biped_capsule_collider(
    half_height: f32,
    friction: f32,
    feet_planted: bool,
) -> Collider {
    let player_collision =
        InteractionGroups::new(GROUP_PLAYER, Group::ALL, InteractionTestMode::And);
    let player_solver = InteractionGroups::new(
        GROUP_PLAYER,
        Group::ALL & !GROUP_PROJECTILE,
        InteractionTestMode::And,
    );
    let y_offset = if feet_planted {
        (half_height + CAPSULE_RADIUS) - CAPSULE_BOTTOM
    } else {
        CAPSULE_HALF_HEIGHT - half_height
    };
    ColliderBuilder::capsule_y(half_height, CAPSULE_RADIUS)
        .translation(Vector3::new(0.0, y_offset, 0.0))
        .friction(friction)
        .friction_combine_rule(CoefficientCombineRule::Min)
        .restitution(MAIN_RESTITUTION)
        .restitution_combine_rule(CoefficientCombineRule::Min)
        .collision_groups(player_collision)
        .solver_groups(player_solver)
        .build()
}

fn replace_capsule_collider(
    world: &mut PhysicsWorld,
    rb_handle: RigidBodyHandle,
    old_ch: Option<ColliderHandle>,
    half_height: f32,
    friction: f32,
    feet_planted: bool,
) -> ColliderHandle {
    let prototype = make_biped_capsule_collider(half_height, friction, feet_planted);
    // Mutate the existing collider in-place when possible: avoids the remove→insert cycle
    // which can leave the island manager with a stale active_island_id and panic in step_physics.
    if let Some(ch) = old_ch
        && let Some(col) = world.collider_set.get_mut(ch)
    {
        let y_offset = if feet_planted {
            (half_height + CAPSULE_RADIUS) - CAPSULE_BOTTOM
        } else {
            CAPSULE_HALF_HEIGHT - half_height
        };
        col.set_shape(prototype.shared_shape().clone());
        col.set_friction(prototype.friction());
        col.set_friction_combine_rule(prototype.friction_combine_rule());
        col.set_translation_wrt_parent(Vector3::new(0.0, y_offset, 0.0));
        return ch;
    }
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(prototype, rb_handle, rigid_body_set)
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: BipedInput,
    biped: &mut BipedPawnComponent,
) {
    let (body_rot, capsule_pos, capsule_linvel, capsule_mass) = {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            return;
        };
        if !body.is_enabled() {
            return;
        }
        (rb_rot(body), rb_pos(body), rb_vel(body), body.mass())
    };
    let planet_up = body_rot * Vec3::Y;
    let desired = biped_move_direction(body_rot, input);
    let (grounded, ground_linvel, support_entity) = ground_state(
        world,
        body_handle.0,
        capsule_pos,
        planet_up,
        biped.is_sliding,
    );

    // Air control: small directional force while airborne
    if !grounded {
        let impulse = (desired + planet_up * (input.jump as i8 as f32 - input.slide as i8 as f32))
            * AIR_CONTROL
            * capsule_mass;
        if impulse.length_squared() > 1e-6
            && let Some(rb) = world.rigid_body_set.get_mut(body_handle.0)
        {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
    }

    // Swap collider when slide state changes. The crouched capsule is always top-aligned:
    // its top stays at the same height as the standing capsule so the camera never moves
    // due to a collider change. In mid-air this means the body is unchanged; on the ground
    // physics lets the body sink until the shorter capsule rests on the surface.
    if input.slide != biped.is_sliding {
        let just_crouched = input.slide && !biped.is_sliding;
        biped.is_sliding = input.slide;
        let (half_height, friction) = if input.slide {
            (SLIDE_HALF_HEIGHT, SLIDE_FRICTION)
        } else {
            (CAPSULE_HALF_HEIGHT, MAIN_FRICTION)
        };
        biped.collider = Some(replace_capsule_collider(
            world,
            body_handle.0,
            biped.collider,
            half_height,
            friction,
            false, // always top-aligned
        ));
        // push the body down immediately when crouching on the ground instead of waiting for gravity.
        if just_crouched && grounded {
            let impulse = -planet_up * CROUCH_DOWN_IMPULSE * capsule_mass;
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }

    biped.jump_cooldown = biped.jump_cooldown.saturating_sub(1);

    // Grounded movement: apply an impulse that tapers to zero as the player approaches max speed.
    // We only consider the velocity component in the direction of desired motion so that
    // strafing or reversing direction always feels responsive.
    if grounded && !input.slide && desired.length_squared() > 1e-6 {
        // Velocity relative to the surface we're standing on (handles moving platforms).
        let rel_vel = capsule_linvel - ground_linvel;
        // Strip the vertical component so we only look at planar speed.
        let planar_vel = rel_vel - planet_up * rel_vel.dot(planet_up);
        let forward_speed = planar_vel.dot(desired).max(0.0);
        // tanh maps [0, MAX_GROUND_SPEED] → [0, ~1], so the impulse smoothly falls to zero
        // at max speed rather than cutting off abruptly.
        let speed_t = forward_speed / MAX_GROUND_SPEED;
        let impulse = desired * (1.0 - speed_t.tanh()) * GROUND_ACCEL * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
    }

    // Jump: apply an upward impulse and push the support body down to conserve momentum.
    // Crouching gives a higher jump.
    if input.jump && grounded && biped.jump_cooldown == 0 {
        biped.jump_cooldown = JUMP_COOLDOWN;
        let jump_strength = if biped.is_sliding {
            JUMP_IMPULSE_CROUCHED
        } else {
            JUMP_IMPULSE
        };
        let impulse = planet_up * jump_strength * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
        // Push the surface we jumped off (e.g. a vehicle) in the opposite direction.
        if let Some(support_entity) = support_entity
            && let Some(&support_handle) = world.entity_to_handle.get(&support_entity)
            && let Some(rb) = world.rigid_body_set.get_mut(support_handle)
            && rb.is_dynamic()
        {
            rb.apply_impulse(Vector::new(-impulse.x, -impulse.y, -impulse.z), true);
        }
    }
}

pub fn apply_biped_input(
    world: &mut PhysicsWorld,
    owner: Entity,
    input: BipedInput,
    body_handle: &RigidBodyHandleComponent,
    biped: &mut BipedPawnComponent,
) -> Option<crate::pawn::biped_ability::AbilityFx> {
    if (biped.look_yaw - input.look_yaw).abs() > 0.0001
        || (biped.look_pitch - input.look_pitch).abs() > 0.0001
    {
        biped.look_sync_dirty = true;
    }
    biped.look_yaw = input.look_yaw;
    biped.look_pitch = input.look_pitch;
    apply_biped_movement(world, body_handle, input, biped);
    super::melee::tick_melee(world, body_handle, input, biped);
    crate::pawn::biped_ability::apply_input(owner, input, world, biped)
}

pub fn biped_move_direction(body_rot: Quat, input: BipedInput) -> Vec3 {
    let facing = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward = facing * Vec3::NEG_Z;
    let right = facing * Vec3::X;
    (forward * input.forward + right * input.right).normalize_or_zero()
}

pub fn aim_pose(
    world: &PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    look_yaw: f32,
    look_pitch: f32,
) -> Option<(Vec3, Vec3)> {
    let rb = world.rigid_body_set.get(body_handle.0)?;
    let body_rot = rb_rot(rb);
    Some((
        // Fire origin must come from the physics body, not the visually smoothed transform.
        // Otherwise interpolate/extrapolate shifts projectile spawn sideways while strafing.
        rb_pos(rb) + body_rot * VIEW_PIVOT_OFFSET,
        body_rot
            * Quat::from_rotation_y(look_yaw)
            * Quat::from_rotation_x(look_pitch)
            * Vec3::NEG_Z,
    ))
}

pub fn viewmodel_offset(_is_primary: bool) -> Transform {
    Transform::from_xyz(0.4, -0.3, 0.0)
}
