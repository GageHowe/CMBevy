//! Gameplay/authored debug drawing helpers.
//!
//! Keep this module generic over authored shapes and simple gizmo patterns used by game object
//! systems. Raw physics collider/body drawing stays in `physics::debug` so dependencies continue
//! to point the right direction.

#[cfg(feature = "client")]
use bevy::{math::primitives::Cuboid as BevyCuboid, prelude::*};
#[cfg(feature = "client")]
use physics::collider_shape::AuthoredColliderShape;

#[cfg(feature = "client")]
pub fn draw_radius_spheres(
    gizmos: &mut Gizmos,
    pos: Vec3,
    inner_r: f32,
    outer_r: f32,
    inner_color: Color,
    outer_color: Color,
) {
    if inner_r > 0.0 {
        gizmos.sphere(Isometry3d::from_translation(pos), inner_r, inner_color);
    }
    if outer_r > 0.0 {
        gizmos.sphere(Isometry3d::from_translation(pos), outer_r, outer_color);
    }
}

#[cfg(feature = "client")]
pub fn draw_authored_shape(
    gizmos: &mut Gizmos,
    shape: &AuthoredColliderShape,
    position: Vec3,
    rotation: Quat,
    color: Color,
) {
    let iso = Isometry3d::new(position, rotation);
    match shape {
        AuthoredColliderShape::Ball(radius) => {
            gizmos.sphere(iso, *radius, color);
        }
        AuthoredColliderShape::Cuboid(half_extents) => {
            gizmos.primitive_3d(
                &BevyCuboid::new(
                    half_extents.x * 2.0,
                    half_extents.y * 2.0,
                    half_extents.z * 2.0,
                ),
                iso,
                color,
            );
        }
        AuthoredColliderShape::Capsule {
            half_height,
            radius,
        } => {
            gizmos.primitive_3d(&Capsule3d::new(*radius, half_height * 2.0), iso, color);
        }
        AuthoredColliderShape::ConvexHulls(_) => {}
    }
}
