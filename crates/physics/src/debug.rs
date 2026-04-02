use bevy::math::primitives::Cuboid as BevyCuboid;
use bevy::prelude::*;
use common::debug_println;
use rapier3d::prelude::*;

/// Extracts a Bevy Isometry3d from a Rapier rigid body's current position.
pub fn rb_iso(rb: &RigidBody) -> Isometry3d {
    use crate::physics_world::{rb_pos, rb_rot};
    Isometry3d::new(rb_pos(rb), rb_rot(rb))
}

/// Draws a Rapier collider's shape at the given world isometry using Bevy gizmos.
/// Supports Ball, Capsule, and Cuboid; silently skips unsupported shapes.
/// Gate the calling system with debug_render_on (or equivalent) rather than checking here.
pub fn draw_collider(collider: &Collider, iso: Isometry3d, color: Color, gizmos: &mut Gizmos) {
    let shape = collider.shape();
    if let Some(ball) = shape.as_ball() {
        gizmos.sphere(iso, ball.radius, color);
    } else if let Some(cap) = shape.as_capsule() {
        gizmos.primitive_3d(
            &Capsule3d::new(cap.radius, cap.half_height() * 2.0),
            iso,
            color,
        );
    } else if let Some(cub) = shape.as_cuboid() {
        let he = cub.half_extents;
        gizmos.primitive_3d(
            &BevyCuboid::new(he.x * 2.0, he.y * 2.0, he.z * 2.0),
            iso,
            color,
        );
    } else {
        debug_println!("draw_collider: No matching type!")
    }
}
