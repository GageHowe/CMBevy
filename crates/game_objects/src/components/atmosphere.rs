/*
atmosphere-adjacent zones:
* atmospheric drag damps velocity of objects inside it relative to the zone's velocity
* area reverb drives client audio based on listener proximity
* either zone may or may not be attached to a rigidbody (e.g. a moving planet)
*/

use bevy::prelude::*;
use physics::physics_world::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

pub struct AtmospherePlugin;
impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<AtmosphericDragComponent>()
            .register_type::<AreaReverbComponent>()
            .add_systems(FixedUpdate, apply_wind_resistance.before(step_physics));
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct AtmosphericDragComponent {
    /// radius of influence
    pub radius: u32,
    /// drag coefficient
    pub strength: f32,
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct AreaReverbComponent {
    /// full-effect radius
    pub min_distance: f32,
    /// fade-out radius
    pub max_distance: f32,
}

/// Inner function callable during reconciliation replay (mirrors apply_gravity_impulses).
pub fn apply_wind_resistance_impulses(
    world: &mut PhysicsWorld,
    atmospheres: &Query<(
        &AtmosphericDragComponent,
        &Transform,
        Option<&RigidBodyHandleComponent>,
    )>,
) {
    let dt = world.integration_parameters.dt;

    // collect atmosphere centers and velocities up front to avoid borrow issues
    let atmo_data: Vec<(Vec3, Vec3, &AtmosphericDragComponent)> = atmospheres
        .iter()
        .filter_map(|(atmo, transform, handle)| {
            let (center, vel) = if let Some(h) = handle {
                let rb = world.rigid_body_set.get(h.0)?;
                (rb_pos(rb), rb_vel(rb))
            } else {
                (transform.translation, Vec3::ZERO)
            };
            Some((center, vel, atmo))
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (center, atmo_vel, atmo) in &atmo_data {
        let radius = atmo.radius as f32;

        let shape = Ball::new(radius);
        let shape_pos = Pose::translation(center.x, center.y, center.z);
        let qp = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            QueryFilter::default(),
        );
        let affected: Vec<RigidBodyHandle> = qp
            .intersect_shape(shape_pos, &shape)
            .filter_map(|(ch, _)| world.collider_set.get(ch).and_then(|c| c.parent()))
            .collect();

        for rb_handle in affected {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            if !rb.is_dynamic() || !rb.is_enabled() {
                continue;
            }

            let body_vel = rb_vel(rb);
            let rel_vel = body_vel - *atmo_vel;
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

pub fn apply_wind_resistance(
    mut world: ResMut<PhysicsWorld>,
    atmospheres: Query<(
        &AtmosphericDragComponent,
        &Transform,
        Option<&RigidBodyHandleComponent>,
    )>,
) {
    apply_wind_resistance_impulses(&mut world, &atmospheres);
}
