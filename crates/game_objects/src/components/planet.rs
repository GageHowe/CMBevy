use crate::pawn::{BipedPawnComponent, SeatedInVehicle};
use bevy::prelude::*;
use physics::physics_world::{self, *};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

#[deprecated]
pub const BASE_GRAVITY_STRENGTH: f32 = 9.81; // m/s^2

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

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct GravitySource {
    pub inner_radius: u32,
    pub radius: u32,
    pub gravity_profile: GravityProfile,
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct SnapSource {
    pub inner_radius: u32,
    pub radius: u32,
}

pub fn apply_gravity_impulses(
    world: &mut PhysicsWorld,
    gravity_sources: &Query<(&GravitySource, &RigidBodyHandleComponent)>,
    gravity_scales: &Query<&physics_world::GravityScale>,
    seated: &Query<&SeatedInVehicle>,
) {
    let dt = world.integration_parameters.dt;

    let source_data: Vec<(Vec3, &GravitySource, RigidBodyHandle)> = gravity_sources
        .iter()
        .filter_map(|(source, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            Some((rb_pos(rb), source, handle.0))
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();
    let mut vel_deltas: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (source_center, source, source_handle) in &source_data {
        let radius = source.radius as f32;
        let inner_radius = source.inner_radius as f32;

        let affected_handles: Vec<RigidBodyHandle> = if source.radius == 0 {
            world
                .rigid_body_set
                .iter()
                .filter(|&(h, _)| h != *source_handle)
                .map(|(h, _)| h)
                .collect()
        } else {
            let shape = Ball::new(radius);
            let shape_pos = Pose::translation(source_center.x, source_center.y, source_center.z);
            let filter = QueryFilter::default().exclude_rigid_body(*source_handle);
            let qp = world.broad_phase.as_query_pipeline(
                world.narrow_phase.query_dispatcher(),
                &world.rigid_body_set,
                &world.collider_set,
                filter,
            );
            let mut handles = Vec::new();
            for (ch, _) in qp.intersect_shape(shape_pos, &shape) {
                let Some(rb_handle) = world.collider_set.get(ch).and_then(|c| c.parent()) else {
                    continue;
                };
                if !handles.contains(&rb_handle) {
                    handles.push(rb_handle);
                }
            }
            handles
        };

        for rb_handle in affected_handles {
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

            let to_source = *source_center - rb_pos(rb);
            let dist = to_source.length();
            if dist < inner_radius || dist < 0.001 {
                continue;
            }

            let strength = match source.gravity_profile {
                GravityProfile::InverseSquare(s) => s / (dist * dist),
                GravityProfile::Linear(s) => {
                    s * if radius > 0.0 {
                        1.0 - (dist / radius).min(1.0)
                    } else {
                        1.0
                    }
                }
                GravityProfile::Constant(s) => s,
            };

            let scale = world
                .handle_to_entity
                .get(&rb_handle)
                .and_then(|e| gravity_scales.get(*e).ok())
                .map_or(1.0, |gs| gs.0);
            if scale == 0.0 {
                continue;
            }

            let dir = to_source / dist;
            if rb.is_dynamic() {
                impulses.push((rb_handle, dir * strength * scale * rb.mass() * dt));
            } else if rb.is_kinematic() {
                vel_deltas.push((rb_handle, dir * strength * scale * dt));
            }
        }
    }

    for (handle, impulse) in impulses {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            rb.apply_impulse(impulse, true);
        }
    }
    for (handle, dv) in vel_deltas {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let v = rb.linvel();
            rb.set_linvel(Vector::new(v.x + dv.x, v.y + dv.y, v.z + dv.z), true);
        }
    }
}

pub fn apply_gravity(
    mut world: ResMut<PhysicsWorld>,
    gravity_sources: Query<(&GravitySource, &RigidBodyHandleComponent)>,
    gravity_scales: Query<&physics_world::GravityScale>,
    seated: Query<&SeatedInVehicle>,
) {
    apply_gravity_impulses(&mut world, &gravity_sources, &gravity_scales, &seated);
}

const ORIENT_SPEED: f32 = 3.0;

pub fn orient_bipeds_to_planets(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<
        (&RigidBodyHandleComponent, &mut BipedPawnComponent),
        Without<SeatedInVehicle>,
    >,
    snap_sources: Query<(Entity, &SnapSource, &RigidBodyHandleComponent)>,
) {
    let dt = world.integration_parameters.dt;
    let snap_data: Vec<(Entity, Vec3, f32, f32)> = snap_sources
        .iter()
        .filter_map(|(entity, source, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            Some((
                entity,
                rb_pos(rb),
                source.inner_radius as f32,
                source.radius as f32,
            ))
        })
        .collect();

    for (body_handle, mut biped) in bipeds.iter_mut() {
        let rb_handle = body_handle.0;
        let (pos, current_rot) = {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            (rb_pos(rb), rb_rot(rb))
        };

        let nearest = snap_data
            .iter()
            .filter_map(|(entity, center, inner_radius, radius)| {
                let dist = pos.distance(*center);
                if dist >= *inner_radius && (*radius == 0.0 || dist <= *radius) {
                    Some((*entity, *center, dist))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        let Some(rb) = world.rigid_body_set.get_mut(rb_handle) else {
            continue;
        };
        let Some((snap_entity, snap_center, _)) = nearest else {
            biped.snap_target = None;
            continue;
        };
        biped.snap_target = Some(snap_entity);

        rb.lock_rotations(true, false);

        let desired_up = (pos - snap_center).normalize();
        orient_body_to_up(rb, current_rot, desired_up, dt);
    }
}

pub fn orient_bipeds_to_planets_impulses(
    world: &mut PhysicsWorld,
    bipeds: &Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<SeatedInVehicle>)>,
    snap_sources: &Query<(&SnapSource, &RigidBodyHandleComponent)>,
) {
    let dt = world.integration_parameters.dt;

    let snap_data: Vec<(Vec3, f32, f32)> = snap_sources
        .iter()
        .filter_map(|(source, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            Some((rb_pos(rb), source.inner_radius as f32, source.radius as f32))
        })
        .collect();

    let biped_handles: Vec<RigidBodyHandle> = bipeds.iter().map(|h| h.0).collect();

    for rb_handle in biped_handles {
        let (pos, current_rot) = {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            (rb_pos(rb), rb_rot(rb))
        };

        let nearest = snap_data
            .iter()
            .filter_map(|(center, inner_radius, radius)| {
                let dist = pos.distance(*center);
                if dist >= *inner_radius && (*radius == 0.0 || dist <= *radius) {
                    Some((*center, dist))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let Some(rb) = world.rigid_body_set.get_mut(rb_handle) else {
            continue;
        };
        let Some((snap_center, _)) = nearest else {
            continue;
        };

        rb.lock_rotations(true, false);

        let desired_up = (pos - snap_center).normalize();
        orient_body_to_up(rb, current_rot, desired_up, dt);
    }
}

fn orient_body_to_up(rb: &mut RigidBody, current_rot: Quat, desired_up: Vec3, dt: f32) {
    let current_forward = current_rot * Vec3::NEG_Z;
    let forward_proj = {
        let proj = current_forward - current_forward.dot(desired_up) * desired_up;
        if proj.length_squared() > 1e-6 {
            proj.normalize()
        } else {
            let alt = if desired_up.abs().x < 0.9 {
                Vec3::X
            } else {
                Vec3::Z
            };
            (alt - alt.dot(desired_up) * desired_up).normalize()
        }
    };

    let right = forward_proj.cross(desired_up).normalize();
    let back = right.cross(desired_up).normalize();
    let target_rot = Quat::from_mat3(&Mat3::from_cols(right, desired_up, back));

    let rot = current_rot.slerp(target_rot, (ORIENT_SPEED * dt).min(1.0));
    rb.set_rotation(rot, false);
}

#[cfg(feature = "client")]
pub fn draw_planet_radii(
    sources: Query<
        (
            Option<&GravitySource>,
            Option<&SnapSource>,
            &GlobalTransform,
        ),
        Or<(With<GravitySource>, With<SnapSource>)>,
    >,
    mut gizmos: Gizmos,
) {
    for (gravity, snap, gt) in sources.iter() {
        let pos = gt.translation();
        if let Some(gravity) = gravity {
            if gravity.inner_radius > 0 {
                gizmos.sphere(
                    Isometry3d::from_translation(pos),
                    gravity.inner_radius as f32,
                    Color::srgba(1.0, 0.0, 0.5, 0.1),
                );
            }
            if gravity.radius > 0 {
                gizmos.sphere(
                    Isometry3d::from_translation(pos),
                    gravity.radius as f32,
                    Color::srgba(0.0, 0.8, 0.0, 0.1),
                );
            }
        }
        if let Some(snap) = snap {
            if snap.inner_radius > 0 {
                gizmos.sphere(
                    Isometry3d::from_translation(pos),
                    snap.inner_radius as f32,
                    Color::srgba(1.0, 0.0, 0.5, 0.1),
                );
            }
            if snap.radius > 0 {
                gizmos.sphere(
                    Isometry3d::from_translation(pos),
                    snap.radius as f32,
                    Color::srgba(0.9, 0.9, 0.0, 0.1),
                );
            }
        }
    }
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<GravityProfile>()
            .register_type::<GravitySource>()
            .register_type::<SnapSource>()
            .add_systems(
                FixedUpdate,
                (apply_gravity, orient_bipeds_to_planets).before(step_physics),
            );
    }
}
