use bevy::prelude::*;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use crate::physics::physics_world::*;

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
    collider_set.insert_with_parent(ColliderBuilder::ball(collider_radius).build(), rb_handle, rigid_body_set);
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    entity
}

/// Applies planet gravity to all dynamic bodies within each planet's `gravity_radius`.
pub fn apply_gravity(
    mut world: ResMut<PhysicsWorld>,
    planets: Query<(&PlanetBehaviorComponent, &PhysicsBodyHandle)>,
) {
    let planet_data: Vec<(Vec3, &PlanetBehaviorComponent, RigidBodyHandle)> = planets.iter()
        .filter_map(|(planet, handle)| {
            let t = world.rigid_body_set.get(handle.0)?.position().translation;
            Some((Vec3::new(t.x, t.y, t.z), planet, handle.0))
        })
        .collect();

    let dt = world.integration_parameters.dt;
    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();

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
            if !rb.is_dynamic() { continue; }

            let t = rb.position().translation;
            let to_planet = *planet_center - Vec3::new(t.x, t.y, t.z);
            let dist = to_planet.length();
            if dist < inner_radius || dist < 0.001 { continue; }

            let strength = match planet.gravity_profile {
                GravityProfile::InverseSquare(s) => s / (dist * dist),
                GravityProfile::Linear(s)        => s * if gravity_radius > 0.0 { 1.0 - (dist / gravity_radius).min(1.0) } else { 1.0 },
                GravityProfile::Constant(s)      => s,
            };

            let impulse = to_planet / dist * strength * rb.mass() * dt;
            impulses.push((rb_handle, impulse));
        }
    }

    for (handle, impulse) in impulses {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            rb.apply_impulse(impulse, true);
        }
    }
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, apply_gravity.before(step_physics));
    }
}
