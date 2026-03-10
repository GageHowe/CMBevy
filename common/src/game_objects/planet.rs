use bevy::prelude::*;
use rapier3d::prelude::*;
use crate::physics::physics_world::*;

pub const GRAVITY_STRENGTH: f32 = 9.81;

#[derive(Component, Default)]
pub struct PlanetComponent {
    /// radius when pawns should no longer snap to planet surface,
    /// e.g., center of a hollow planet. If zero, no effect.
    pub inner_radius: u32,
    pub snap_radius: u32,
    pub gravity_radius: u32,
}

impl PlanetComponent {
    pub fn with_inner_radius(mut self, inner_radius: u32) -> Self {
        self.inner_radius = inner_radius;
        self
    }
    pub fn with_snap_radius(mut self, snap_radius: u32) -> Self {
        self.snap_radius = snap_radius;
        self
    }
    pub fn with_gravity_radius(mut self, gravity_radius: u32) -> Self {
        self.gravity_radius = gravity_radius;
        self
    }
}

/// Spawns a planet with a fixed sphere collider.
pub fn spawn(
    transform: Transform,
    planet: PlanetComponent,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
) -> Entity {
    let snap_radius = planet.snap_radius as f32;
    let rb = RigidBodyBuilder::fixed()
        .translation(transform.translation)
        .build();
    let entity = commands.spawn((planet, transform)).id();
    let rb_handle = world.insert_body(entity, rb);
    let collider = ColliderBuilder::ball(snap_radius).build();
    commands.entity(entity).insert(PhysicsBodyHandle(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
    entity
}

/// Adds a debug sphere mesh to an existing planet entity.
pub fn add_visuals(
    entity: Entity,
    snap_radius: f32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.entity(entity).insert((
        Mesh3d(meshes.add(Sphere::new(snap_radius))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.6, 0.9))),
        Visibility::default(),
    ));
}

/// Applies planet gravity to all dynamic bodies within each planet's `gravity_radius`.
/// Equal and opposite impulses are applied to both bodies (Newton's 3rd law).
pub fn apply_gravity(
    mut world: ResMut<PhysicsWorld>,
    planets: Query<(&PlanetComponent, &PhysicsBodyHandle)>,
) {
    let planet_data: Vec<(Vec3, f32, RigidBodyHandle)> = planets.iter()
        .filter_map(|(planet, handle)| {
            let t = world.rigid_body_set.get(handle.0)?.position().translation;
            Some((Vec3::new(t.x, t.y, t.z), planet.gravity_radius as f32, handle.0))
        })
        .collect();

    let dt = world.integration_parameters.dt;
    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (planet_center, gravity_radius, planet_handle) in &planet_data {
        let shape = Ball::new(*gravity_radius);
        let shape_pos = Pose::translation(planet_center.x, planet_center.y, planet_center.z);
        let filter = QueryFilter::default().exclude_rigid_body(*planet_handle);
        let qp = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            filter,
        );
        // Collect before the loop so the qp borrow is released before mutating rigid_body_set.
        let affected: Vec<ColliderHandle> = qp.intersect_shape(shape_pos, &shape)
            .map(|(ch, _)| ch)
            .collect();

        for collider_handle in affected {
            let Some(rb_handle) = world.collider_set.get(collider_handle).and_then(|c| c.parent()) else { continue };
            let Some(rb) = world.rigid_body_set.get(rb_handle) else { continue };
            if !rb.is_dynamic() { continue; }

            // calculate and apply force
            let t = rb.position().translation;
            let to_planet = *planet_center - Vec3::new(t.x, t.y, t.z);
            let dist_sq = to_planet.length_squared();
            if dist_sq < 0.001 { continue; }
            let impulse = to_planet.normalize() * GRAVITY_STRENGTH * rb.mass() * dt;
            impulses.push((rb_handle, impulse));
            impulses.push((*planet_handle, -impulse));
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
