// atmosphere.rs
/*
an atmosphere:
* damps velocity of objects inside it relative to its velocity
* atmosphere entity may or may not be a rigidbody (e.g. a moving planet)
*/

use bevy::prelude::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use physics::physics_world::{self, *};

pub struct AtmospherePlugin;
impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, apply_wind_resistance.before(step_physics));
    }
}

#[derive(Component, Serialize, Deserialize, Clone)]
pub struct AtmosphereComponent {
    /// radius of influence
    pub radius: u32,
    /// drag coefficient
    pub strength: f32,
}

/// Inner function callable during reconciliation replay (mirrors apply_gravity_impulses).
pub fn apply_wind_resistance_impulses(
    world: &mut PhysicsWorld,
    atmospheres: &Query<(&AtmosphereComponent, &Transform, Option<&RigidBodyHandleComponent>)>,
) {
    let dt = world.integration_parameters.dt;

    // collect atmosphere centers and velocities up front to avoid borrow issues
    let atmo_data: Vec<(Vec3, Vec3, &AtmosphereComponent)> = atmospheres.iter()
        .filter_map(|(atmo, transform, handle)| {
            let (center, vel) = if let Some(h) = handle {
                let rb = world.rigid_body_set.get(h.0)?;
                let t = rb.position().translation;
                let v = rb.linvel();
                (Vec3::new(t.x, t.y, t.z), Vec3::new(v.x, v.y, v.z))
            } else {
                (transform.translation, Vec3::ZERO)
            };
            Some((center, vel, atmo))
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (center, atmo_vel, atmo) in &atmo_data {
        let radius = atmo.radius as f32;

        // spatial query: find all colliders within the atmosphere sphere
        let shape = Ball::new(radius);
        let shape_pos = Pose::translation(center.x, center.y, center.z);
        let qp = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            QueryFilter::default(),
        );
        let affected: Vec<RigidBodyHandle> = qp.intersect_shape(shape_pos, &shape)
            .filter_map(|(ch, _)| world.collider_set.get(ch).and_then(|c| c.parent()))
            .collect();

        for rb_handle in affected {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else { continue };
            if !rb.is_dynamic() || !rb.is_enabled() { continue; }

            let v = rb.linvel();
            let body_vel = Vec3::new(v.x, v.y, v.z);
            // relative velocity of the body with respect to the atmosphere
            let rel_vel = body_vel - *atmo_vel;
            // drag impulse opposes relative motion
            let impulse = -rel_vel * atmo.strength * rb.mass() * dt;
            impulses.push((rb_handle, Vector::new(impulse.x, impulse.y, impulse.z)));
        }
    }

    for (handle, impulse) in impulses {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            rb.apply_impulse(impulse, true);
        }
    }
}

/// FixedUpdate system that applies wind resistance.
/// Should also be called during reconciliation (call apply_wind_resistance_impulses directly).
pub fn apply_wind_resistance(
    mut world: ResMut<PhysicsWorld>,
    atmospheres: Query<(&AtmosphereComponent, &Transform, Option<&RigidBodyHandleComponent>)>,
) {
    apply_wind_resistance_impulses(&mut world, &atmospheres);
}

