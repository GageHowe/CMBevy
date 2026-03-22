// physics_world.rs
// this manages the physics simulation and syncs it with clients

use bevy::prelude::*;
use common::{BodyState, NetworkID, SimulationState};
use rapier3d::prelude::Vector3;
use rapier3d::prelude::*;
pub use rapier3d::prelude::RigidBodyHandle;
use std::collections::HashMap;
use common::debug_println;

/// Collision group for player bodies (capsule + foot sphere).
pub const GROUP_PLAYER: Group = Group::GROUP_1;
/// Collision group for projectiles. Excluded from player-group solver contacts.
pub const GROUP_PROJECTILE: Group = Group::GROUP_2;

/// scales how strongly planetary gravity affects this body. Defaults to 1.0 if absent.
#[derive(Component, Clone, Copy)]
pub struct GravityScale(pub f32);

/// a way for entities to refer to their rigidbody
#[derive(Component)]
pub struct RigidBodyHandleComponent(pub RigidBodyHandle);

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
                dt: 1.0 / common::config::FIXED_TICK_RATE as f32,
                min_ccd_dt: 1.0 / common::config::FIXED_TICK_RATE as f32 / 100.0,
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

    /// insert a rigidbody with a rigidbody-entity relationship. it's not tracked until inserted here.
    pub fn insert_body(&mut self, entity: Entity, body: RigidBody) -> RigidBodyHandle {
        let handle = self.rigid_body_set.insert(body);

        self.handle_to_entity.insert(handle, entity);
        self.entity_to_handle.insert(entity, handle);

        handle
    }

    /// clean up the rigidbody associated with this entity.
    pub fn remove_rigidbody(&mut self, entity: Entity) {
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
        } else {
            debug_println!("Warning: tried to remove_rigidbody but entity {entity} is not in entity_to_handle")
        }
    }

    /// Disable or re-enable a body without removing it from the world.
    pub fn set_body_enabled(&mut self, entity: Entity, enabled: bool) {
        if let Some(&handle) = self.entity_to_handle.get(&entity) {
            if let Some(rb) = self.rigid_body_set.get_mut(handle) {
                rb.set_enabled(enabled);
            }
        }
    }

    /// Teleport a body to `pos` and zero its velocities.
    pub fn teleport_body(&mut self, entity: Entity, pos: Vec3) {
        if let Some(&handle) = self.entity_to_handle.get(&entity) {
            if let Some(rb) = self.rigid_body_set.get_mut(handle) {
                rb.set_translation(Vector3::new(pos.x, pos.y, pos.z), true);
                rb.set_linvel(Vector3::ZERO, true);
                rb.set_angvel(Vector3::ZERO, true);
            }
        }
    }
}

impl PhysicsWorld {
    /// Cast a sphere and return the first entity hit.
    pub fn cast_sphere(
        &self,
        origin: Vec3,
        direction: Vec3,
        radius: f32,
        max_distance: f32,
        exclude_entity: Option<Entity>,
    ) -> Option<Entity> {
        use rapier3d::parry::query::ShapeCastOptions;
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
        let shape = Ball::new(radius);
        let iso = Pose::translation(origin.x, origin.y, origin.z);
        let vel = Vector::new(direction.x, direction.y, direction.z);
        qp.cast_shape(&iso, vel, &shape, ShapeCastOptions::with_max_time_of_impact(max_distance))
            .and_then(|(ch, _)| {
                let rb_handle = self.collider_set.get(ch)?.parent()?;
                Some(*self.handle_to_entity.get(&rb_handle)?)
            })
    }

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

/// Controls how physics body positions are mapped to Bevy Transforms each frame.
/// Off: snap to last-tick position. Extrapolate: project forward by overstep. Interpolate: one tick behind, interpolated.
/// RotationOnly: same as Extrapolate but skips writing translation (e.g. for bodies whose position is owned by the hierarchy).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsInterpMode {
    Off,
    Interpolate,
    Extrapolate,
    #[default]
    RotationOnly,
}

pub struct PhysicsPlugin;
impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PhysicsWorld::new(Vector3::ZERO))
            .init_resource::<PhysicsInterpMode>()
            .add_observer(on_remove_physics_body);
    }
}

fn on_remove_physics_body(event: On<Remove, RigidBodyHandleComponent>, mut world: ResMut<PhysicsWorld>) {
    world.remove_rigidbody(event.entity);
}

pub fn step_physics(mut world: ResMut<PhysicsWorld>) {
    step_world(&mut world);
}

/// call this when stepping and reconciling
pub fn step_world(world: &mut ResMut<PhysicsWorld>) {
    // do anything that needs to be done physics-wise
    world.step();
}

/// Snapshot the current physics state for all networked bodies.
/// Returns a `SimulationState` stamped with `tick`.
#[rustfmt::skip]
pub fn snapshot_bodies<'a>(
    world: &PhysicsWorld,
    tick: u64,
    pairs: impl Iterator<Item = (&'a NetworkID, &'a RigidBodyHandleComponent)>,
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

/// Syncs physics bodies to Bevy transforms every frame, decoupled from the fixed tick.
/// Register in Update (client-only); for server-side exact sync use sync_physics_to_transforms.
pub fn sync_physics_visual(
    world: Res<PhysicsWorld>,
    time: Res<Time<Fixed>>,
    interp: Res<PhysicsInterpMode>,
    mut query: Query<(&RigidBodyHandleComponent, &mut Transform)>,
) {
    let overstep = time.overstep_fraction();
    let fixed_dt = time.delta_secs();
    let dt_offset = match *interp {
        PhysicsInterpMode::Off => 0.0,
        PhysicsInterpMode::Extrapolate | PhysicsInterpMode::RotationOnly => overstep * fixed_dt,
        PhysicsInterpMode::Interpolate => (overstep - 1.0) * fixed_dt,
    };
    for (body_handle, mut transform) in query.iter_mut() {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else { continue };
        let pos = body.position();
        let cur_pos = Vec3::new(pos.translation.x, pos.translation.y, pos.translation.z);
        let cur_rot = Quat::from_xyzw(pos.rotation.x, pos.rotation.y, pos.rotation.z, pos.rotation.w);
        let linvel = Vec3::new(body.linvel().x, body.linvel().y, body.linvel().z);
        let angvel = Vec3::new(body.angvel().x, body.angvel().y, body.angvel().z);
        // RotationOnly: write last-known position (no extrapolation), extrapolate rotation only
        transform.translation = if *interp == PhysicsInterpMode::RotationOnly {
            cur_pos
        } else {
            cur_pos + linvel * dt_offset
        };
        let ang_speed = angvel.length();
        transform.rotation = if ang_speed > 1e-6 {
            Quat::from_axis_angle(angvel / ang_speed, ang_speed * dt_offset) * cur_rot
        } else {
            cur_rot
        };
    }
}

/// handle visual sync (gameserver FixedUpdate path — no smoothing needed)
pub fn sync_physics_to_transforms(
    world: Res<PhysicsWorld>,
    mut query: Query<(&RigidBodyHandleComponent, &mut Transform)>,
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
