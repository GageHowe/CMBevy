use bevy::prelude::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use crate::pawn::pawn::BipedPawnComponent;
use physics::physics_world::{self, *};

#[deprecated]
/// this should depend on the individual GravityRadiusComponent
pub const GRAVITY_STRENGTH: f32 = 9.81;

#[derive(Component, Serialize, Deserialize, Clone)]
pub enum GravityProfile {
    InverseSquare(f32),
    Linear(f32),
    Constant(f32),
}

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct PlanetBehaviorComponent {
    /// skips gravity application completely when within this radius
    pub inner_radius: u32,
    /// outer radius where bipeds' feet should point towards the center of the planet.
    /// has no effect when entity is inside inner_radius
    pub snap_radius: u32,
    /// max radius for the spatial query. If zero, all objects are affected.
    pub gravity_radius: u32,
    /// defines how strong gravity is over time
    pub gravity_profile: GravityProfile
}

pub fn spawn(planet: PlanetBehaviorComponent, transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
    let collider_radius = planet.inner_radius as f32;
    let pos = transform.translation;
    let entity = commands.spawn((planet, transform)).id();
    let rb_handle = world.insert_body(entity, RigidBodyBuilder::fixed().translation(Vector3::new(pos.x, pos.y, pos.z)).build());
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(ColliderBuilder::ball(collider_radius).friction(3.0).restitution(0.0).build(), rb_handle, rigid_body_set);
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    entity
}

/// Applies a gravity impulse from each planet to all dynamic bodies within its radius.
/// Called before every physics step, including during reconciliation replay.
pub fn apply_gravity_impulses(
    world: &mut PhysicsWorld,
    planets: &Query<(&PlanetBehaviorComponent, &RigidBodyHandleComponenet)>,
    gravity_scales: &Query<&physics_world::GravityScale>,
) {
    let dt = world.integration_parameters.dt;

    let planet_data: Vec<(Vec3, &PlanetBehaviorComponent, RigidBodyHandle)> = planets.iter()
        .filter_map(|(planet, handle)| {
            let t = world.rigid_body_set.get(handle.0)?.position().translation;
            Some((Vec3::new(t.x, t.y, t.z), planet, handle.0))
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();
    let mut vel_deltas: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (planet_center, planet, planet_handle) in &planet_data {
        let gravity_radius = planet.gravity_radius as f32;
        let inner_radius = planet.inner_radius as f32;

        let affected_handles: Vec<RigidBodyHandle> = if planet.gravity_radius == 0 {
            world.rigid_body_set.iter()
                .filter(|&(h, _)| h != *planet_handle)
                .map(|(h, _)| h)
                .collect()
        } else {
            let shape = Ball::new(gravity_radius);
            let shape_pos = Pose::translation(planet_center.x, planet_center.y, planet_center.z);
            let filter = QueryFilter::default().exclude_rigid_body(*planet_handle);
            let qp = world.broad_phase.as_query_pipeline(
                world.narrow_phase.query_dispatcher(),
                &world.rigid_body_set,
                &world.collider_set,
                filter,
            );
            let collider_handles: Vec<ColliderHandle> = qp.intersect_shape(shape_pos, &shape)
                .map(|(ch, _)| ch)
                .collect();
            collider_handles.iter()
                .filter_map(|ch| world.collider_set.get(*ch).and_then(|c| c.parent()))
                .collect()
        };

        for rb_handle in affected_handles {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else { continue };
            if !rb.is_enabled() { continue; }

            let t = rb.position().translation;
            let to_planet = *planet_center - Vec3::new(t.x, t.y, t.z);
            let dist = to_planet.length();
            if dist < inner_radius || dist < 0.001 { continue; }

            let strength = match planet.gravity_profile {
                GravityProfile::InverseSquare(s) => s / (dist * dist),
                GravityProfile::Linear(s)        => s * if gravity_radius > 0.0 { 1.0 - (dist / gravity_radius).min(1.0) } else { 1.0 },
                GravityProfile::Constant(s)      => s,
            };

            let scale = world.handle_to_entity.get(&rb_handle)
                .and_then(|e| gravity_scales.get(*e).ok())
                .map_or(1.0, |gs| gs.0);
            if scale == 0.0 { continue; }

            let dir = to_planet / dist;
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
    planets: Query<(&PlanetBehaviorComponent, &RigidBodyHandleComponenet)>,
    gravity_scales: Query<&physics_world::GravityScale>,
) {
    apply_gravity_impulses(&mut world, &planets, &gravity_scales);
}

/// Smoothly orients bipeds upright relative to the nearest planet using an
/// orthonormal basis rebuild each tick. Avoids the roll drift that accumulates
/// when composing rotation arcs, since the basis is reconstructed from scratch
/// using the current forward vector projected onto the plane perpendicular to
/// the planet's "up". snap_radius == 0 is treated as unlimited.
/// Smoothly orients bipeds upright relative to the nearest planet using an
/// orthonormal basis rebuild each tick. Avoids the roll drift that accumulates
/// when composing rotation arcs, since the basis is reconstructed from scratch
/// using the current forward vector projected onto the plane perpendicular to
/// the planet's "up". snap_radius == 0 is treated as unlimited.
pub fn orient_bipeds_to_planets(
    mut world: ResMut<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponenet, With<BipedPawnComponent>>,
    planets: Query<(&PlanetBehaviorComponent, &RigidBodyHandleComponenet)>,
) {
    let planet_data: Vec<(Vec3, f32)> = planets.iter()
        .filter_map(|(planet, handle)| {
            let t = world.rigid_body_set.get(handle.0)?.position().translation;
            Some((Vec3::new(t.x, t.y, t.z), planet.snap_radius as f32))
        })
        .collect();

    let biped_handles: Vec<RigidBodyHandle> = bipeds.iter().map(|h| h.0).collect();

    for rb_handle in biped_handles {
        let (pos, current_rot) = {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else { continue };
            let t = rb.position().translation;
            let r = rb.rotation();
            (Vec3::new(t.x, t.y, t.z), Quat::from_xyzw(r.x, r.y, r.z, r.w))
        };

        let nearest = planet_data.iter()
            .filter_map(|(center, snap_radius)| {
                let dist = pos.distance(*center);
                if *snap_radius == 0.0 || dist <= *snap_radius { Some((*center, dist)) } else { None }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let Some(rb) = world.rigid_body_set.get_mut(rb_handle) else { continue };
        let Some((planet_center, _)) = nearest else { continue };

        rb.lock_rotations(true, false);

        let desired_up = (pos - planet_center).normalize();
        let current_forward = current_rot * Vec3::NEG_Z;

        // Gram-Schmidt: project current forward onto the plane perpendicular to desired_up.
        // This preserves yaw without accumulating roll across ticks.
        let forward_proj = {
            let proj = current_forward - current_forward.dot(desired_up) * desired_up;
            if proj.length_squared() > 1e-6 {
                proj.normalize()
            } else {
                let alt = if desired_up.abs().x < 0.9 { Vec3::X } else { Vec3::Z };
                (alt - alt.dot(desired_up) * desired_up).normalize()
            }
        };

        // Rebuild orthonormal basis: X=right, Y=up, Z=back
        let right = forward_proj.cross(desired_up).normalize();
        let back  = right.cross(desired_up).normalize();
        rb.set_rotation(Quat::from_mat3(&Mat3::from_cols(right, desired_up, back)), false);
    }
}

pub fn draw_planet_radii(
    planets: Query<(&PlanetBehaviorComponent, &GlobalTransform)>,
    mut gizmos: Gizmos,
) {
    for (planet, gt) in planets.iter() {
        let pos = gt.translation();
        if planet.inner_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.inner_radius as f32, Color::srgba(0.8, 0.2, 0.2, 0.15));
        }
        if planet.snap_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.snap_radius as f32, Color::srgba(0.9, 0.8, 0.1, 0.15));
        }
        if planet.gravity_radius > 0 {
            gizmos.sphere(Isometry3d::from_translation(pos), planet.gravity_radius as f32, Color::srgba(0.2, 0.8, 0.2, 0.15));
        }
    }
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (apply_gravity, orient_bipeds_to_planets).before(step_physics));
    }
}
