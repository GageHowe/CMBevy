pub mod atmosphere;
pub mod gravity;
pub mod snap;

use bevy::prelude::*;

#[cfg(feature = "client")]
pub fn draw_zone_circles(
    gizmos: &mut Gizmos,
    pos: Vec3,
    normal: Vec3,
    inner_r: f32,
    outer_r: f32,
    half_width: f32,
    color: Color,
) {
    let rot = Quat::from_rotation_arc(Vec3::Z, normal);
    let offsets: &[f32] = if half_width > 0.0 {
        &[-half_width, half_width]
    } else {
        &[0.0]
    };
    for &off in offsets {
        let iso = Isometry3d::new(pos + normal * off, rot);
        if inner_r > 0.0 {
            gizmos.circle(iso, inner_r, color);
        }
        if outer_r > 0.0 {
            gizmos.circle(iso, outer_r, color);
        }
    }
}

/// Returns `(outward_dir, distance)` if `pos` is within `[inner_r, outer_r]` of `center`.
/// `outward_dir` points from center toward `pos`.
pub fn in_spherical_zone(
    center: Vec3,
    inner_r: f32,
    outer_r: f32,
    pos: Vec3,
) -> Option<(Vec3, f32)> {
    let d = pos - center;
    let dist = d.length();
    (dist >= 0.001 && dist >= inner_r && (outer_r == 0.0 || dist <= outer_r))
        .then(|| (d / dist, dist))
}

/// Returns `(outward_dir, perp_dist)` if `pos` is within `[inner_r, outer_r]` perpendicular
/// distance from the line through `center` along `axis` (must be unit length), and within
/// `half_width` along the axis (0 = infinite).
/// `outward_dir` points from the nearest axis point toward `pos`.
pub fn in_ring_zone(
    center: Vec3,
    axis: Vec3,
    inner_r: f32,
    outer_r: f32,
    half_width: f32,
    pos: Vec3,
) -> Option<(Vec3, f32)> {
    let axial = (pos - center).dot(axis);
    if half_width > 0.0 && axial.abs() > half_width {
        return None;
    }
    let nearest = center + axis * axial;
    let d = pos - nearest;
    let dist = d.length();
    (dist >= 0.001 && dist >= inner_r && (outer_r == 0.0 || dist <= outer_r))
        .then(|| (d / dist, dist))
}
