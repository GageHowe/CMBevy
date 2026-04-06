// physics_world.rs
// this manages the physics simulation and syncs it with clients

use bevy::prelude::*;
use common::debug_println;
use common::{BodyState, NetworkID, PredictedCommands, SimulationState};
pub use rapier3d::prelude::RigidBodyHandle;
pub use rapier3d::prelude::Vector3;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Collision group for player bodies (capsule + foot sphere).
pub const GROUP_PLAYER: Group = Group::GROUP_1;
/// Collision group for projectiles. Excluded from player-group solver contacts.
pub const GROUP_PROJECTILE: Group = Group::GROUP_2;

#[inline]
pub fn rb_pos(rb: &RigidBody) -> Vec3 {
    let t = rb.position().translation;
    Vec3::new(t.x, t.y, t.z)
}
#[inline]
pub fn rb_rot(rb: &RigidBody) -> Quat {
    let r = rb.rotation();
    Quat::from_xyzw(r.x, r.y, r.z, r.w)
}
#[inline]
pub fn rb_vel(rb: &RigidBody) -> Vec3 {
    let v = rb.linvel();
    Vec3::new(v.x, v.y, v.z)
}
#[inline]
pub fn rb_angvel(rb: &RigidBody) -> Vec3 {
    let v = rb.angvel();
    Vec3::new(v.x, v.y, v.z)
}

/// scales how strongly planetary gravity affects this body. Defaults to 1.0 if absent.
#[derive(Component, Clone, Copy)]
pub struct GravityScale(pub f32);

/// Scene-authored initial linear velocity for objects that spawn through map data.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
pub struct InitialVelocity(pub Vec3);

#[derive(Component, Clone, Copy, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
pub enum SceneRigidBody {
    #[default]
    Fixed,
    Dynamic,
}

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
            debug_println!(
                "Warning: tried to remove_rigidbody but entity {entity} is not in entity_to_handle"
            )
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

    /// Teleport a body to `pos`/`rot` and set its velocities explicitly.
    pub fn set_body_pose(
        &mut self,
        entity: Entity,
        pos: Vec3,
        rot: Quat,
        linvel: Vec3,
        angvel: Vec3,
    ) {
        if let Some(&handle) = self.entity_to_handle.get(&entity) {
            if let Some(rb) = self.rigid_body_set.get_mut(handle) {
                rb.set_translation(Vector3::new(pos.x, pos.y, pos.z), true);
                rb.set_rotation(rot, true);
                rb.set_linvel(Vector3::new(linvel.x, linvel.y, linvel.z), true);
                rb.set_angvel(Vector3::new(angvel.x, angvel.y, angvel.z), true);
                rb.wake_up(true);
            }
        }
    }

    /// Shared gameplay impulse path so callers don't have to manually keep prediction in sync.
    pub fn apply_game_impulse(
        &mut self,
        entity: Entity,
        impulse: Vec3,
        net_id: Option<&NetworkID>,
        predicted: Option<&mut PredictedCommands>,
    ) -> bool {
        let Some(&handle) = self.entity_to_handle.get(&entity) else {
            return false;
        };
        let Some(rb) = self.rigid_body_set.get_mut(handle) else {
            return false;
        };
        rb.apply_impulse(Vector3::new(impulse.x, impulse.y, impulse.z), true);
        if let (Some(net_id), Some(predicted)) = (net_id, predicted) {
            predicted.record_impulse(net_id.clone(), impulse);
        }
        true
    }

    pub fn insert_fixed_joint(
        &mut self,
        body1_entity: Entity,
        body2_entity: Entity,
        frame1: Pose,
        frame2: Pose,
        contacts_enabled: bool,
    ) -> Option<ImpulseJointHandle> {
        let body1 = *self.entity_to_handle.get(&body1_entity)?;
        let body2 = *self.entity_to_handle.get(&body2_entity)?;
        let joint = FixedJointBuilder::new()
            .local_frame1(frame1)
            .local_frame2(frame2)
            .contacts_enabled(contacts_enabled);
        Some(self.impulse_joint_set.insert(body1, body2, joint, true))
    }

    pub fn remove_impulse_joint(&mut self, handle: ImpulseJointHandle) {
        self.impulse_joint_set.remove(handle, true);
    }
}

impl PhysicsWorld {
    /// Cast a sphere and return the first entity hit.
    /// `exclude` lists entities whose colliders are skipped (e.g. shooter + self).
    pub fn cast_sphere(
        &self,
        origin: Vec3,
        direction: Vec3,
        radius: f32,
        max_distance: f32,
        exclude: &[Entity],
    ) -> Option<Entity> {
        use rapier3d::parry::query::ShapeCastOptions;
        let excluded: Vec<RigidBodyHandle> = exclude
            .iter()
            .filter_map(|e| self.entity_to_handle.get(e).copied())
            .collect();
        let pred = |_: ColliderHandle, col: &Collider| {
            col.parent().map_or(true, |rb_h| !excluded.contains(&rb_h))
        };
        let filter = QueryFilter::new().predicate(&pred);
        let qp = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        let shape = Ball::new(radius);
        let iso = Pose::translation(origin.x, origin.y, origin.z);
        let vel = Vector::new(direction.x, direction.y, direction.z);
        qp.cast_shape(
            &iso,
            vel,
            &shape,
            ShapeCastOptions::with_max_time_of_impact(max_distance),
        )
        .and_then(|(ch, _)| {
            let rb_handle = self.collider_set.get(ch)?.parent()?;
            Some(*self.handle_to_entity.get(&rb_handle)?)
        })
    }

    /// Cast a ray and return the first entity hit and the distance to impact.
    /// `exclude` lists entities whose colliders are skipped (e.g. shooter + projectile self).
    pub fn cast_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        exclude: &[Entity],
    ) -> Option<(Entity, f32)> {
        let excluded: Vec<RigidBodyHandle> = exclude
            .iter()
            .filter_map(|e| self.entity_to_handle.get(e).copied())
            .collect();
        let pred = |_: ColliderHandle, col: &Collider| {
            col.parent().map_or(true, |rb_h| !excluded.contains(&rb_h))
        };
        let filter = QueryFilter::new().predicate(&pred);
        let qp = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        let ray = Ray::new(origin, direction);
        qp.cast_ray(&ray, max_distance, true).and_then(|(ch, toi)| {
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
            .register_type::<InitialVelocity>()
            .register_type::<SceneRigidBody>()
            .init_resource::<PhysicsInterpMode>()
            .add_observer(on_remove_physics_body);
    }
}

fn on_remove_physics_body(
    event: On<Remove, RigidBodyHandleComponent>,
    mut world: ResMut<PhysicsWorld>,
) {
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
pub fn snapshot_bodies<'a>(
    world: &PhysicsWorld,
    tick: u64,
    pairs: impl Iterator<Item = (&'a NetworkID, &'a RigidBodyHandleComponent)>,
) -> SimulationState {
    let mut bodies = HashMap::new();
    for (net_id, body_handle) in pairs {
        if let Some(rb) = world.rigid_body_set.get(body_handle.0) {
            bodies.insert(
                net_id.clone(),
                BodyState {
                    position: rb_pos(rb).into(),
                    rotation: rb_rot(rb).into(),
                    linvel: rb_vel(rb).into(),
                    angvel: rb_angvel(rb).into(),
                },
            );
        }
    }
    SimulationState {
        tick,
        last_input_seq: 0,
        bodies,
    }
}

pub fn snapshot_body_handles<'a>(
    world: &PhysicsWorld,
    tick: u64,
    pairs: impl Iterator<Item = (&'a NetworkID, RigidBodyHandle)>,
) -> SimulationState {
    let mut bodies = HashMap::new();
    for (net_id, handle) in pairs {
        if let Some(rb) = world.rigid_body_set.get(handle) {
            bodies.insert(
                net_id.clone(),
                BodyState {
                    position: rb_pos(rb).into(),
                    rotation: rb_rot(rb).into(),
                    linvel: rb_vel(rb).into(),
                    angvel: rb_angvel(rb).into(),
                },
            );
        }
    }
    SimulationState {
        tick,
        last_input_seq: 0,
        bodies,
    }
}

/// Apply a server snapshot to the physics world.
/// `pairs` maps NetworkID → RigidBodyHandle for every networked entity.
pub fn restore_snapshot(
    world: &mut PhysicsWorld,
    snapshot: &SimulationState,
    pairs: &[(NetworkID, RigidBodyHandle)],
) {
    for (net_id, handle) in pairs {
        let Some(state) = snapshot.bodies.get(net_id) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get_mut(*handle) else {
            continue;
        };
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
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        let cur_pos = rb_pos(body);
        let cur_rot = rb_rot(body);
        let linvel = rb_vel(body);
        let angvel = rb_angvel(body);
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
            transform.translation = rb_pos(body);
            transform.rotation = rb_rot(body);
        }
    }
}
