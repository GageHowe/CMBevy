use bevy::prelude::*;
// use nalgebra::Vector3;
use rapier3d::prelude::Vector3;
use rapier3d::prelude::*;
#[derive(Resource)] // means this is a singleton
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
}

impl PhysicsWorld {
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
                // what is the optimal value for min_island_size?
                // It should not be too big so that we don't end up with
                // huge islands that don't fit in cache.
                // However we don't want it to be too small and end up with
                // tons of islands, reducing SIMD parallelism opportunities.
                // TODO: benchmark
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

    pub fn spawn_capsule_player(&mut self, start: Vector3) -> RigidBodyHandle {
        let rb = RigidBodyBuilder::dynamic()
            .translation(start)
            .lock_rotations()
            .build();

        let handle = self.rigid_body_set.insert(rb);

        let collider = ColliderBuilder::capsule_y(0.9, 0.4) // height, radius
            .friction(0.0)
            .build();

        self.collider_set
            .insert_with_parent(collider, handle, &mut self.rigid_body_set);

        handle
    }
}

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PhysicsWorld::new(Vector3::new(0.0, -9.81, 0.0)))
            .add_systems(Startup, init_physics)
            .add_systems(FixedUpdate, step_physics);
    }
}

fn init_physics(mut world: ResMut<PhysicsWorld>) {
    let cube_rb = RigidBodyBuilder::dynamic()
        .translation(Vector::new(10.0, 5.0, -3.0))
        .build();

    let cube_handle = world.rigid_body_set.insert(cube_rb);

    let cube_collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();

    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;

    collider_set.insert_with_parent(cube_collider, cube_handle, rigid_body_set);
}

fn step_physics(mut world: ResMut<PhysicsWorld>) {
    world.step();
    print!("tick ");
}
