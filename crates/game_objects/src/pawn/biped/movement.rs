use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::*;

pub(super) fn ground_state(
    world: &PhysicsWorld,
    body_handle: RigidBodyHandle,
    capsule_pos: Vec3,
    planet_up: Vec3,
) -> (bool, Vec3, Option<Entity>) {
    let ray_origin = capsule_pos - planet_up * CAPSULE_BOTTOM;
    let exclude = |_ch: ColliderHandle, col: &rapier3d::prelude::Collider| {
        !col.is_sensor() && col.parent().map_or(true, |rb| rb != body_handle)
    };
    let filter = QueryFilter::new().predicate(&exclude);
    let qp = world.broad_phase.as_query_pipeline(
        world.narrow_phase.query_dispatcher(),
        &world.rigid_body_set,
        &world.collider_set,
        filter,
    );
    let ray = Ray::new(ray_origin, -planet_up);
    if let Some((ch, _)) = qp.cast_ray(&ray, GROUND_DIST, true) {
        let support_body = world.collider_set.get(ch).and_then(|col| col.parent());
        let vel = support_body
            .and_then(|rb_h| world.rigid_body_set.get(rb_h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let support_entity =
            support_body.and_then(|rb_h| world.handle_to_entity.get(&rb_h).copied());
        (true, vel, support_entity)
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
    if let Some(old_ch) = old_ch {
        let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
        collider_set.remove(old_ch, island_manager, rigid_body_set, false);
    }
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(
        make_biped_capsule_collider(half_height, friction, feet_planted),
        rb_handle,
        rigid_body_set,
    )
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
    let (grounded, ground_linvel, support_entity) =
        ground_state(world, body_handle.0, capsule_pos, planet_up);

    let slide_feet_planted = grounded;
    if input.slide != biped.is_sliding
        || (input.slide && slide_feet_planted != biped.slide_feet_planted)
    {
        biped.is_sliding = input.slide;
        biped.slide_feet_planted = slide_feet_planted;
        let (half_height, friction, feet_planted) = if input.slide {
            (SLIDE_HALF_HEIGHT, SLIDE_FRICTION, slide_feet_planted)
        } else {
            (CAPSULE_HALF_HEIGHT, MAIN_FRICTION, true)
        };
        biped.collider = Some(replace_capsule_collider(
            world,
            body_handle.0,
            biped.collider,
            half_height,
            friction,
            feet_planted,
        ));
    }

    biped.jump_cooldown = biped.jump_cooldown.saturating_sub(1);

    // if grounded and not sliding
    if grounded && !input.slide && desired.length_squared() > 1e-6 {
        let rel_vel = capsule_linvel - ground_linvel;
        let rel_vel_planar = rel_vel - planet_up * rel_vel.dot(planet_up);
        let forward_speed = rel_vel_planar.dot(desired).max(0.0);
        let impulse = desired
            * (1.0 - (forward_speed * GROUND_SPEED_FALLOFF).tanh())
            * GROUND_ACCEL
            * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
    }

    if input.jump && grounded && biped.jump_cooldown == 0 {
        biped.jump_cooldown = JUMP_COOLDOWN;
        let impulse = planet_up * JUMP_IMPULSE * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
        if let Some(support_entity) = support_entity
            && let Some(&support_handle) = world.entity_to_handle.get(&support_entity)
            && let Some(rb) = world.rigid_body_set.get_mut(support_handle)
            && rb.is_dynamic()
        {
            rb.apply_impulse(Vector::new(-impulse.x, -impulse.y, -impulse.z), true);
        }
    }

    // air control
    if !grounded {
        let down = input.slide as i8 as f32;
        let impulse = (desired * AIR_CONTROL - planet_up * down * AIR_CONTROL) * capsule_mass;
        if impulse.length_squared() > 1e-6
            && let Some(rb) = world.rigid_body_set.get_mut(body_handle.0)
        {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
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
    crate::pawn::biped_ability::apply_input(owner, input, world, biped)
}

pub fn biped_move_direction(body_rot: Quat, input: BipedInput) -> Vec3 {
    let facing = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward = facing * Vec3::NEG_Z;
    let right = facing * Vec3::X;
    (forward * input.forward + right * input.right).normalize_or_zero()
}

pub fn viewmodel_offset(_is_primary: bool) -> Transform {
    Transform::from_xyz(0.4, -0.3, 0.0)
}
