use std::cmp::Ordering;

use bevy::prelude::*;
use physics::physics_world::{
    ForceApplication, PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot,
};
use rapier3d::prelude::{RigidBody, RigidBodyHandle};
use serde::{Deserialize, Serialize};

use super::{in_ring_zone, in_spherical_zone};
use crate::pawn::{BipedPawnComponent, Mounted};

#[derive(Serialize, Deserialize, Clone, Reflect, Default)]
pub enum SnapKind {
    /// Biped up = away from center point. Standing on a planet's outer surface.
    #[default]
    Point,
    /// Biped up = toward the axis. Standing on a ring world's inner wall.
    /// Direction is in the body's LOCAL space.
    Axis(Vec3),
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct SnapSource {
    pub kind: SnapKind,
    pub inner_radius: u32,
    pub radius: u32,
    /// Half-length along the axis for ring zones (0 = infinite).
    #[serde(default)]
    pub half_width: u32,
}

// ---- internal ----

/// Returns (desired_up, distance) for snap selection. Negates the outward vector for rings
/// since biped "up" on an inner ring wall points toward the axis, not away.
fn snap_eval(
    center: Vec3,
    axis: Option<Vec3>,
    inner_r: f32,
    outer_r: f32,
    half_width: f32,
    pos: Vec3,
) -> Option<(Vec3, f32)> {
    match axis {
        None => in_spherical_zone(center, inner_r, outer_r, pos),
        Some(ax) => {
            in_ring_zone(center, ax, inner_r, outer_r, half_width, pos).map(|(d, dist)| (-d, dist))
        }
    }
}

const ORIENT_SPEED: f32 = 3.0;

fn orient_body_to_up(rb: &mut RigidBody, current_rot: Quat, desired_up: Vec3, dt: f32) {
    let fwd = current_rot * Vec3::NEG_Z;
    let fwd_proj = {
        let p = fwd - fwd.dot(desired_up) * desired_up;
        if p.length_squared() > 1e-6 {
            p.normalize()
        } else {
            let alt = if desired_up.abs().x < 0.9 {
                Vec3::X
            } else {
                Vec3::Z
            };
            (alt - alt.dot(desired_up) * desired_up).normalize()
        }
    };
    let right = fwd_proj.cross(desired_up).normalize();
    let back = right.cross(desired_up).normalize();
    let target = Quat::from_mat3(&Mat3::from_cols(right, desired_up, back));
    rb.set_rotation(
        current_rot.slerp(target, (ORIENT_SPEED * dt).min(1.0)),
        false,
    );
}

// ---- systems ----

pub fn orient_bipeds_to_snap_sources(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<(&RigidBodyHandleComponent, &mut BipedPawnComponent), Without<Mounted>>,
    sources: Query<(Entity, &SnapSource, &RigidBodyHandleComponent)>,
) {
    let dt = world.integration_parameters.dt;

    // Pre-collect world-space source data to free the immutable borrow before &mut below.
    let entries: Vec<(Entity, Vec3, Option<Vec3>, f32, f32, f32)> = sources
        .iter()
        .filter_map(|(entity, src, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            let axis = match src.kind {
                SnapKind::Axis(dir) => Some(rb_rot(rb) * dir.normalize_or_zero()),
                SnapKind::Point => None,
            };
            Some((
                entity,
                rb_pos(rb),
                axis,
                src.inner_radius as f32,
                src.radius as f32,
                src.half_width as f32,
            ))
        })
        .collect();

    for (handle, mut biped) in bipeds.iter_mut() {
        let (pos, rot) = {
            let Some(rb) = world.rigid_body_set.get(handle.0) else {
                continue;
            };
            (rb_pos(rb), rb_rot(rb))
        };
        let best = entries
            .iter()
            .filter_map(|&(e, c, ax, ir, or_, hw)| {
                snap_eval(c, ax, ir, or_, hw, pos).map(|(up, d)| (e, up, d))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal));
        let Some(rb) = world.rigid_body_set.get_mut(handle.0) else {
            continue;
        };
        match best {
            None => biped.snap_target = None,
            Some((entity, up, _)) => {
                biped.snap_target = Some(entity);
                rb.lock_rotations(true, false);
                orient_body_to_up(rb, rot, up, dt);
            }
        }
    }
}

pub fn orient_bipeds_to_snap_sources_impulses(
    world: &mut PhysicsWorld,
    bipeds: &Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<Mounted>)>,
    sources: &Query<(&SnapSource, &RigidBodyHandleComponent)>,
) {
    let dt = world.integration_parameters.dt;

    let entries: Vec<(Vec3, Option<Vec3>, f32, f32, f32)> = sources
        .iter()
        .filter_map(|(src, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            let axis = match src.kind {
                SnapKind::Axis(dir) => Some(rb_rot(rb) * dir.normalize_or_zero()),
                SnapKind::Point => None,
            };
            Some((
                rb_pos(rb),
                axis,
                src.inner_radius as f32,
                src.radius as f32,
                src.half_width as f32,
            ))
        })
        .collect();

    let handles: Vec<RigidBodyHandle> = bipeds.iter().map(|h| h.0).collect();

    for rb_handle in handles {
        let (pos, rot) = {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            (rb_pos(rb), rb_rot(rb))
        };
        let best = entries
            .iter()
            .filter_map(|&(c, ax, ir, or_, hw)| snap_eval(c, ax, ir, or_, hw, pos))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        let Some(rb) = world.rigid_body_set.get_mut(rb_handle) else {
            continue;
        };
        if let Some((up, _)) = best {
            rb.lock_rotations(true, false);
            orient_body_to_up(rb, rot, up, dt);
        }
    }
}

// ---- visualization ----

#[cfg(feature = "client")]
pub fn draw_snap_radii(sources: Query<(&SnapSource, &GlobalTransform)>, mut gizmos: Gizmos) {
    for (src, gt) in sources.iter() {
        let pos = gt.translation();
        match src.kind {
            SnapKind::Point => crate::debug_draw::draw_radius_spheres(
                &mut gizmos,
                pos,
                src.inner_radius as f32,
                src.radius as f32,
                Color::srgba(1.0, 0.0, 0.5, 0.1),
                Color::srgba(0.9, 0.9, 0.0, 0.1),
            ),
            SnapKind::Axis(dir) => {
                let normal = gt.affine().transform_vector3(dir).normalize_or_zero();
                if normal.length_squared() < 0.5 {
                    continue;
                }
                super::draw_zone_circles(
                    &mut gizmos,
                    pos,
                    normal,
                    src.inner_radius as f32,
                    src.radius as f32,
                    src.half_width as f32,
                    Color::srgba(0.4, 0.8, 1.0, 0.6),
                );
            }
        }
    }
}

pub struct SnapPlugin;
impl Plugin for SnapPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SnapKind>()
            .register_type::<SnapSource>()
            .add_systems(
                FixedUpdate,
                orient_bipeds_to_snap_sources.in_set(ForceApplication),
            );
    }
}
