// physics_world.rs
// this manages the physics simulation and syncs it with clients

// use bevy::math::VectorSpace;
use bevy::prelude::*;
// use bevy::render::
// use nalgebra::Vector3;
use crate::net::message::{BodyState, NetworkID, SimulationState};
use bevy::math::primitives::Cuboid;
use rapier3d::prelude::Vector3;
use rapier3d::prelude::*;
pub use rapier3d::prelude::RigidBodyHandle;
use std::collections::HashMap;

/// a way for entities to refer to their rigidbody
#[derive(Component)]
pub struct PhysicsBodyHandle(pub rapier3d::prelude::RigidBodyHandle);

/// new
/// this should be used on the client to decide whether or not to resimulate, and which tick/state to target if so
#[derive(Resource)]
pub struct LastRecievedServerState(pub Option<SimulationState>);

#[derive(Resource)]
pub struct PhysicsWorld {
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub global_gravity: Vector3,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub physics_hooks: (),
    pub event_handler: (),

    pub handle_to_entity: HashMap<RigidBodyHandle, Entity>,
    pub entity_to_handle: HashMap<Entity, RigidBodyHandle>,
}

impl PhysicsWorld {
    /// Create new PhysicsWorld with reasonable defaults, subject to tweaking
    pub fn new(gravity: Vector3) -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            global_gravity: gravity,
            integration_parameters: IntegrationParameters {
                dt: 1.0 / 60.0,
                min_ccd_dt: 1.0 / 60.0 / 100.0,
                contact_softness: SpringCoefficients::contact_defaults(),
                warmstart_coefficient: 1.0,
                num_internal_pgs_iterations: 1,
                num_internal_stabilization_iterations: 1,
                num_solver_iterations: 4,
                min_island_size: 128,
                normalized_allowed_linear_error: 0.001,
                normalized_max_corrective_velocity: 10.0,
                normalized_prediction_distance: 0.002,
                max_ccd_substeps: 1,
                length_unit: 1.0,
                friction_model: FrictionModel::default(),
            },
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            physics_hooks: (),
            event_handler: (),

            handle_to_entity: HashMap::new(),
            entity_to_handle: HashMap::new(),
        }
    }

    pub fn step(&mut self) {
        self.physics_pipeline.step(
            self.global_gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &self.physics_hooks,
            &self.event_handler,
        );
    }

    /// Insert a rigidbody with a rigidbody-entity relationship. It's not tracked until inserted here
    pub fn insert_body(&mut self, entity: Entity, body: RigidBody) -> RigidBodyHandle {
        let handle = self.rigid_body_set.insert(body);

        self.handle_to_entity.insert(handle, entity);
        self.entity_to_handle.insert(entity, handle);

        handle
    }

    /// Clean up the rigidbody associated with this entity.
    pub fn remove_body(&mut self, entity: Entity) {
        if let Some(handle) = self.entity_to_handle.remove(&entity) {
            self.handle_to_entity.remove(&handle);
            self.rigid_body_set.remove(
                handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
    }
}

impl PhysicsWorld {
    /// Cast a ray and return the first entity hit and the distance to impact.
    /// Optionally excludes `exclude_entity`'s colliders (e.g. the shooter).
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        exclude_entity: Option<Entity>,
    ) -> Option<(Entity, f32)> {
        let filter = match exclude_entity.and_then(|e| self.entity_to_handle.get(&e).copied()) {
            Some(handle) => QueryFilter::default().exclude_rigid_body(handle),
            None => QueryFilter::default(),
        };
        let qp = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        let ray = Ray::new(origin, direction);
        qp.cast_ray(&ray, max_distance, true)
            .and_then(|(ch, toi)| {
                let rb_handle = self.collider_set.get(ch)?.parent()?;
                let entity = self.handle_to_entity.get(&rb_handle)?;
                Some((*entity, toi))
            })
    }
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        // app.insert_resource(PhysicsWorld::new(Vector3::new(0.0, -9.81, 0.0)))
        app.insert_resource(PhysicsWorld::new(Vector3::ZERO))
            .add_systems(Startup, create_objects)
            .add_systems(
                FixedUpdate,
                (step_physics, sync_physics_to_transforms).chain(),
            );
    }
}

/// example of creating physics objects
pub fn create_objects(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {

    let plane_entity = commands.spawn_empty().id();
    let plane_rb = RigidBodyBuilder::fixed()
        .translation(Vector3::new(0.0, -10.0, 0.0))
        .build();
    let plane_handle = world.insert_body(plane_entity, plane_rb);
    let plane_collider = ColliderBuilder::cuboid(10.0, 2.0, 10.0).build();
    commands.entity(plane_entity).insert((
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(20.0, 4.0, 20.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.2, 0.2),
            metallic: 0.0,
            perceptual_roughness: 0.6,
            reflectance: 0.1,
            ..Default::default()
        })),
        Visibility::default(),
        PhysicsBodyHandle(plane_handle),
    ));

    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(plane_collider, plane_handle, rigid_body_set);
}

pub fn step_physics(mut world: ResMut<PhysicsWorld>) {
    world.step();
    // apply gravity for all planets
}

/// Snapshot the current physics state for all networked bodies.
/// Returns a `SimulationState` stamped with `tick`.
#[rustfmt::skip]
pub fn snapshot_bodies<'a>(
    world: &PhysicsWorld,
    tick: u64,
    pairs: impl Iterator<Item = (&'a NetworkID, &'a PhysicsBodyHandle)>,
) -> SimulationState {
    let mut bodies = HashMap::new();
    for (net_id, body_handle) in pairs {
        if let Some(rb) = world.rigid_body_set.get(body_handle.0) {
            let pos = rb.position();
            bodies.insert(net_id.clone(), BodyState {
                position: Vec3::new(pos.translation.x, pos.translation.y, pos.translation.z).into(),
                rotation: Quat::from_xyzw(pos.rotation.x, pos.rotation.y, pos.rotation.z, pos.rotation.w).into(),
                linvel:   Vec3::new(rb.linvel().x, rb.linvel().y, rb.linvel().z).into(),
                angvel:   Vec3::new(rb.angvel().x, rb.angvel().y, rb.angvel().z).into(),
            });
        }
    }
    SimulationState { tick, bodies }
}

/// Apply a server snapshot to the physics world.
/// `pairs` maps NetworkID → RigidBodyHandle for every networked entity.
pub fn restore_snapshot(
    world: &mut PhysicsWorld,
    snapshot: &SimulationState,
    pairs: &[(NetworkID, RigidBodyHandle)],
) {
    for (net_id, handle) in pairs {
        let Some(state) = snapshot.bodies.get(net_id) else { continue };
        let Some(rb) = world.rigid_body_set.get_mut(*handle) else { continue };
        rb.set_translation(
            Vector3::new(state.position.x, state.position.y, state.position.z),
            true,
        );
        rb.set_rotation(
            Quat::from_xyzw(
                state.rotation.x,
                state.rotation.y,
                state.rotation.z,
                state.rotation.w,
            ),
            true,
        );
        rb.set_linvel(
            Vector3::new(state.linvel.x, state.linvel.y, state.linvel.z),
            true,
        );
        rb.set_angvel(
            Vector3::new(state.angvel.x, state.angvel.y, state.angvel.z),
            true,
        );
        rb.wake_up(true);
    }
}

/// handle visual sync
/// probably could be more efficient though
fn sync_physics_to_transforms(
    world: Res<PhysicsWorld>,
    mut query: Query<(&PhysicsBodyHandle, &mut Transform)>,
) {
    for (body_handle, mut transform) in query.iter_mut() {
        if let Some(body) = world.rigid_body_set.get(body_handle.0) {
            let pos = body.position();
            let translation = pos.translation;
            let rotation = pos.rotation;

            transform.translation = Vec3::new(translation.x, translation.y, translation.z);
            transform.rotation = Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w);
        }
    }
}

// use bevy::transform::TransformSystems;
//
// fn transform_physics_tranforms_extrapolated(
//     world: Res<PhysicsWorld>,
//     fixed_time: Res<Time<Fixed>>,
//     mut query: Query<(&PhysicsBodyHandle, &mut Transform)>,
// ) {
//     let a = fixed_time.overstep_fraction(); // 0..1 between fixed steps [web:44][web:48]
//
//     // duration of one physics tick (must match IntegrationParameters::dt)
//     let dt = world.integration_parameters.dt as f32;
//
//     for (body_handle, mut transform) in &mut query {
//         if let Some(body) = world.rigid_body_set.get(body_handle.0) {
//             // current physics state
//             let pos = body.position();
//             let translation = pos.translation;
//             let rotation = pos.rotation;
//             let linvel = body.linvel();
//             let angvel = body.angvel();
//
//             // predict next-tick position using velocity (simple extrapolation) [web:29]
//             let future_translation = translation + linvel * dt;
//             // naive angular extrapolation: axis-angle from angvel * dt
//             let ang_speed = angvel.norm();
//             let future_rotation = if ang_speed > 0.0001 {
//                 let axis = angvel / ang_speed;
//                 let angle = ang_speed * dt;
//                 let delta = Quat::from_axis_angle(Vec3::new(axis.x, axis.y, axis.z), angle);
//                 Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w) * delta
//             } else {
//                 Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w)
//             };
//
//             // lerp/slerp between current and future based on overstep fraction [web:29]
//             let current = Vec3::new(translation.x, translation.y, translation.z);
//             let future = Vec3::new(future_translation.x, future_translation.y, future_translation.z);
//             transform.translation = current.lerp(future, a);
//
//             let current_rot = Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w);
//             transform.rotation = current_rot.slerp(future_rotation, a);
//         }
//     }
// }
