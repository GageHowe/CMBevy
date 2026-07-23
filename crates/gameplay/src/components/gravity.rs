use bevy::prelude::*;
use physics::physics_world::{
    self, ForceApplication, PhysicsWorld, RigidBodyHandleComponent, rb_pos, rb_rot,
};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use super::{in_ring_zone, in_spherical_zone};
use crate::pawn::Mounted;

#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component, Default)]
pub enum GravityProfile {
    InverseSquare(f32),
    Linear(f32),
    Constant(f32),
}

impl Default for GravityProfile {
    fn default() -> Self {
        Self::Constant(0.0)
    }
}

#[derive(Serialize, Deserialize, Clone, Reflect, Default)]
pub enum GravityKind {
    /// Pulls toward the center point. Planet-style.
    #[default]
    Point,
    /// Pushes away from the axis. Ring-world centrifugal style.
    /// Direction is in the body's LOCAL space.
    Axis(Vec3),
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct GravitySource {
    pub kind: GravityKind,
    pub inner_radius: u32,
    pub radius: u32,
    /// Half-length along the axis for ring zones (0 = infinite).
    #[serde(default)]
    pub half_width: u32,
    pub profile: GravityProfile,
}

// ---- impulse application ----

pub fn apply_gravity_impulses(
    world: &mut PhysicsWorld,
    sources: &Query<(&GravitySource, &RigidBodyHandleComponent)>,
    gravity_scales: &Query<&physics_world::GravityScale>,
    seated: &Query<&Mounted>,
) {
    let dt = world.integration_parameters.dt;

    struct Entry {
        center: Vec3,
        axis: Option<Vec3>,
        inner_r: f32,
        outer_r: f32,
        half_width: f32,
        profile: GravityProfile,
        source_handle: RigidBodyHandle,
    }

    let entries: Vec<Entry> = sources
        .iter()
        .filter_map(|(src, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            let axis = match src.kind {
                GravityKind::Axis(dir) => Some(rb_rot(rb) * dir.normalize_or_zero()),
                GravityKind::Point => None,
            };
            Some(Entry {
                center: rb_pos(rb),
                axis,
                inner_r: src.inner_radius as f32,
                outer_r: src.radius as f32,
                half_width: src.half_width as f32,
                profile: src.profile.clone(),
                source_handle: handle.0,
            })
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();
    let mut vel_deltas: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for entry in &entries {
        let affected: Vec<RigidBodyHandle> = match entry.axis {
            // Ring: full iteration (no tight sphere query for cylindrical range).
            Some(_) => world
                .rigid_body_set
                .iter()
                .filter(|&(h, _)| h != entry.source_handle)
                .map(|(h, _)| h)
                .collect(),
            // Sphere: use spatial query when radius is bounded.
            None if entry.outer_r > 0.0 => {
                let shape = Ball::new(entry.outer_r);
                let pos = Pose::translation(entry.center.x, entry.center.y, entry.center.z);
                let filter = QueryFilter::default().exclude_rigid_body(entry.source_handle);
                let qp = world.broad_phase.as_query_pipeline(
                    world.narrow_phase.query_dispatcher(),
                    &world.rigid_body_set,
                    &world.collider_set,
                    filter,
                );
                let mut handles = Vec::new();
                for (ch, _) in qp.intersect_shape(pos, &shape) {
                    if let Some(h) = world.collider_set.get(ch).and_then(|c| c.parent()) {
                        if !handles.contains(&h) {
                            handles.push(h);
                        }
                    }
                }
                handles
            }
            None => world
                .rigid_body_set
                .iter()
                .filter(|&(h, _)| h != entry.source_handle)
                .map(|(h, _)| h)
                .collect(),
        };

        for rb_handle in affected {
            if world
                .handle_to_entity
                .get(&rb_handle)
                .and_then(|e| seated.get(*e).ok())
                .is_some()
            {
                continue;
            }
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            if !rb.is_enabled() {
                continue;
            }

            let pos = rb_pos(rb);
            // Spherical pulls toward center; ring pushes outward from axis.
            let Some((outward, dist)) = (match entry.axis {
                None => in_spherical_zone(entry.center, entry.inner_r, entry.outer_r, pos),
                Some(ax) => in_ring_zone(
                    entry.center,
                    ax,
                    entry.inner_r,
                    entry.outer_r,
                    entry.half_width,
                    pos,
                ),
            }) else {
                continue;
            };
            let dir = match entry.axis {
                None => -outward,
                Some(_) => outward,
            };

            let strength = eval_profile(&entry.profile, dist, entry.outer_r);
            let scale = world
                .handle_to_entity
                .get(&rb_handle)
                .and_then(|e| gravity_scales.get(*e).ok())
                .map_or(1.0, |gs| gs.0);
            if scale == 0.0 {
                continue;
            }

            let f = dir * strength * scale * dt;
            if rb.is_dynamic() {
                impulses.push((rb_handle, Vector::new(f.x, f.y, f.z) * rb.mass()));
            } else if rb.is_kinematic() {
                vel_deltas.push((rb_handle, Vector::new(f.x, f.y, f.z)));
            }
        }
    }

    for (h, imp) in impulses {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            rb.apply_impulse(imp, true);
        }
    }
    for (h, dv) in vel_deltas {
        if let Some(rb) = world.rigid_body_set.get_mut(h) {
            let v = rb.linvel();
            rb.set_linvel(Vector::new(v.x + dv.x, v.y + dv.y, v.z + dv.z), true);
        }
    }
}

pub fn apply_gravity(
    mut world: ResMut<PhysicsWorld>,
    sources: Query<(&GravitySource, &RigidBodyHandleComponent)>,
    gravity_scales: Query<&physics_world::GravityScale>,
    seated: Query<&Mounted>,
) {
    apply_gravity_impulses(&mut world, &sources, &gravity_scales, &seated);
}

fn eval_profile(profile: &GravityProfile, dist: f32, outer_r: f32) -> f32 {
    match profile {
        GravityProfile::InverseSquare(s) => s / (dist * dist),
        GravityProfile::Linear(s) => {
            s * if outer_r > 0.0 {
                1.0 - (dist / outer_r).min(1.0)
            } else {
                1.0
            }
        }
        GravityProfile::Constant(s) => *s,
    }
}

// ---- visualization ----

#[cfg(feature = "client")]
pub fn draw_gravity_radii(sources: Query<(&GravitySource, &GlobalTransform)>, mut gizmos: Gizmos) {
    for (src, gt) in sources.iter() {
        let pos = gt.translation();
        match src.kind {
            GravityKind::Point => crate::debug_draw::draw_radius_spheres(
                &mut gizmos,
                pos,
                src.inner_radius as f32,
                src.radius as f32,
                Color::srgba(1.0, 0.0, 0.5, 0.1),
                Color::srgba(0.0, 0.8, 0.0, 0.1),
            ),
            GravityKind::Axis(dir) => {
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
                    Color::srgba(0.2, 1.0, 0.4, 0.5),
                );
            }
        }
    }
}

pub struct GravityPlugin;
impl Plugin for GravityPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<GravityProfile>()
            .register_type::<GravityKind>()
            .register_type::<GravitySource>()
            .add_systems(FixedUpdate, apply_gravity.in_set(ForceApplication));
    }
}
