// physics_world.rs
// this manages the physics simulation and syncs it with clients

use bevy::prelude::*;
// use bevy::render::
// use nalgebra::Vector3;
use bevy::math::primitives::Cuboid;
use rapier3d::prelude::Vector3;
use rapier3d::prelude::*;
use std::collections::HashMap;

// a way for entities to refer to their rigidbody
#[derive(Component)]
pub struct PhysicsBodyHandle(pub rapier3d::prelude::RigidBodyHandle);

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

    /// Insert a rigidbody-entity relationship. It's not tracked until inserted here
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

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PhysicsWorld::new(Vector3::new(0.0, -9.81, 0.0)))
            .add_systems(Startup, init_physics)
            .add_systems(
                FixedUpdate,
                (step_physics, sync_physics_to_transforms).chain(),
            );
    }
}

fn init_physics(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // make/init a physics cube
    let cube_rb = RigidBodyBuilder::dynamic()
        .translation(Vec3::new(10.0, 5.0, -3.0))
        .build();
    let cube_handle = world.rigid_body_set.insert(cube_rb);
    let cube_collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();

    // plane
    let plane_rb = RigidBodyBuilder::fixed()
        .translation(Vec3::new(0.0, -10.0, 0.0))
        .build();
    let plane_handle = world.rigid_body_set.insert(plane_rb);
    let plane_collider = ColliderBuilder::cuboid(10.0, 2.0, 10.0);
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(cube_collider, cube_handle, rigid_body_set);
    collider_set.insert_with_parent(plane_collider, plane_handle, rigid_body_set);

    // spawn visual cube
    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.7, 0.6),
            ..Default::default()
        })),
        Visibility::default(),
        PhysicsBodyHandle(cube_handle),
    ));

    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(20.0, 4.0, 20.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.0, 0.7, 0.0),
            ..Default::default()
        })),
        Visibility::default(),
        PhysicsBodyHandle(plane_handle),
    ));
}

fn step_physics(mut world: ResMut<PhysicsWorld>) {
    world.step();
    // print!("tick ");
}

/// Component to mark entities that should be networked
#[derive(Component)]
pub struct NetworkId(pub u32);

#[derive(Clone)]
pub struct NetworkSnapshot {
    pub tick: u64,
    // Map NetworkId -> (position, rotation, velocity)
    pub bodies: HashMap<u32, BodyState>,
}

#[derive(Clone)]
pub struct BodyState {
    pub position: Vec3,
    pub rotation: Quat,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

fn take_snapshot(
    world: Res<PhysicsWorld>,
    query: Query<(&NetworkId, &PhysicsBodyHandle)>,
) -> NetworkSnapshot {
    let mut bodies = HashMap::new();

    for (net_id, body_handle) in query.iter() {
        if let Some(rb) = world.rigid_body_set.get(body_handle.0) {
            let pos = rb.position();
            bodies.insert(
                net_id.0,
                BodyState {
                    position: Vec3::new(pos.translation.x, pos.translation.y, pos.translation.z),
                    rotation: Quat::from_xyzw(
                        pos.rotation.x,
                        pos.rotation.y,
                        pos.rotation.z,
                        pos.rotation.w,
                    ),
                    linvel: Vec3::new(rb.linvel().x, rb.linvel().y, rb.linvel().z),
                    angvel: Vec3::new(rb.angvel().x, rb.angvel().y, rb.angvel().z),
                },
            );
        }
    }

    NetworkSnapshot {
        tick: 0, // fill in actual tick
        bodies,
    }
}

fn restore_snapshot(
    mut world: ResMut<PhysicsWorld>,
    snapshot: &NetworkSnapshot,
    query: Query<(&NetworkId, &PhysicsBodyHandle)>,
) {
    for (net_id, body_handle) in query.iter() {
        if let Some(state) = snapshot.bodies.get(&net_id.0) {
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                // Restore position
                rb.set_translation(
                    vector![state.position.x, state.position.y, state.position.z],
                    true,
                );
                rb.set_rotation(
                    UnitQuaternion::from_quaternion(Quaternion::new(
                        state.rotation.w,
                        state.rotation.x,
                        state.rotation.y,
                        state.rotation.z,
                    )),
                    true,
                );

                // Restore velocities
                rb.set_linvel(
                    vector![state.linvel.x, state.linvel.y, state.linvel.z],
                    true,
                );
                rb.set_angvel(
                    vector![state.angvel.x, state.angvel.y, state.angvel.z],
                    true,
                );

                rb.wake_up(true);
            }
        }
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
