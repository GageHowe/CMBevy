// physics_world.rs
// this manages the physics simulation and syncs it with clients

use std::collections::HashMap;

use bevy::prelude::*;
use common::{BodyState, NetworkID, PredictedCommands, SimulationState};
pub use rapier3d::prelude::{RigidBodyHandle, Vector3};
use rapier3d::{parry::query::ShapeCastOptions, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    collider_flags::{ColliderFlags, collider_flags},
    collider_shape::AuthoredColliderShape,
};

/// Collision group for player bodies (capsule + foot sphere).
pub const GROUP_PLAYER: Group = Group::GROUP_1;
/// Collision group for projectile sensor colliders. These exist for overlap-based queries like
/// gravity, not for collision or solver participation.
pub const GROUP_PROJECTILE: Group = Group::GROUP_2;

// cheap accessors for getting pos/rot/etc from a &RigidBody
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
#[inline]
pub fn rb_point_vel(rb: &RigidBody, point: Vec3) -> Vec3 {
    let offset = point - rb_pos(rb);
    rb_vel(rb) + rb_angvel(rb).cross(offset)
}

/// scales how strongly planetary gravity affects this body. Defaults to 1.0 if absent.
#[derive(Component, Clone, Copy)]
pub struct GravityScale(pub f32);

/// Scene-authored initial linear velocity for objects that spawn through map data.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
#[component(storage = "SparseSet")]
pub struct InitialVelocity(pub Vec3);

/// Scene-authored initial angular velocity for objects that spawn through map data.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
#[component(storage = "SparseSet")]
pub struct InitialAngularVelocity(pub Vec3);

/// enables specifying RigidBody type in .ron map files
#[derive(Component, Clone, Copy, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
pub enum SceneRigidBody {
    #[default]
    Fixed,
    Dynamic,
    /// Kinematic velocity-based: moves physics objects but is unaffected by forces.
    Kinematic,
}

/// a way for entities to refer to their rigidbody
#[derive(Component)]
pub struct RigidBodyHandleComponent(pub RigidBodyHandle);

/// struct that contains native Rapier world, maps, and bookkeeping
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

#[derive(Clone, Copy)]
pub struct RayHit {
    pub entity: Entity,
    pub collider: ColliderHandle,
    /// time of impact; 0 is immediate hit, 1 is hit at very tip of ray. maybe use this for damage scaling or something
    pub toi: f32,
    /// direction to
    pub normal: Option<Vec3>,
    /// world-space position of hit
    pub point_of_impact: Option<Vec3>,
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
                num_solver_iterations: 3, // prefer speed over accuracy, subject to tuning
                min_island_size: 128,
                normalized_allowed_linear_error: 0.001,
                normalized_max_corrective_velocity: 10.0, // maybe make this higher...
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
            eprintln!("tried to remove_rigidbody but entity {entity} is not in entity_to_handle")
        }
    }

    // /// Disable or re-enable a body without removing it from the world.
    // pub fn set_body_enabled(&mut self, entity: Entity, enabled: bool) {
    //     if let Some(&handle) = self.entity_to_handle.get(&entity) {
    //         if let Some(rb) = self.rigid_body_set.get_mut(handle) {
    //             rb.set_enabled(enabled);
    //         }
    //     }
    // }

    // this is bad because rarely do we want to set the velocity of an entity to 0.
    // pub fn teleport_body(&mut self, entity: Entity, pos: Vec3) {
    //     if let Some(&handle) = self.entity_to_handle.get(&entity) {
    //         if let Some(rb) = self.rigid_body_set.get_mut(handle) {
    //             rb.set_translation(Vector3::new(pos.x, pos.y, pos.z), true);
    //             rb.set_linvel(Vector3::ZERO, true);
    //             rb.set_angvel(Vector3::ZERO, true);
    //         }
    //     }
    // }

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
                // Don't wake disabled bodies — waking a body that isn't in any island
                // corrupts Rapier's island manager (active_island_id becomes usize::MAX).
                let wake = rb.is_enabled();
                rb.set_translation(Vector3::new(pos.x, pos.y, pos.z), wake);
                rb.set_rotation(rot, wake);
                rb.set_linvel(Vector3::new(linvel.x, linvel.y, linvel.z), wake);
                rb.set_angvel(Vector3::new(angvel.x, angvel.y, angvel.z), wake);
                if wake {
                    rb.wake_up(true);
                }
            }
        }
    }

    // gross wrapper
    // pub fn predicted_body_point(
    //     &self,
    //     entity: Entity,
    //     local_point: Vec3,
    // ) -> Option<(Vec3, Quat, Vec3, Vec3)> {
    //     self.predicted_body_point_after(entity, local_point, self.integration_parameters.dt)
    // }

    // used by pawn; but is there a better way?
    pub fn predicted_body_point_after(
        &self,
        entity: Entity,
        local_point: Vec3,
        dt: f32,
    ) -> Option<(Vec3, Quat, Vec3, Vec3)> {
        let rb = self
            .entity_to_handle
            .get(&entity)
            .and_then(|&handle| self.rigid_body_set.get(handle))?;
        let linvel = rb_vel(rb);
        let angvel = rb_angvel(rb);
        let predicted = rb.predict_position_using_velocity(dt.max(0.0));
        let predicted_pos = Vec3::new(
            predicted.translation.x,
            predicted.translation.y,
            predicted.translation.z,
        );
        let predicted_rot = Quat::from_xyzw(
            predicted.rotation.x,
            predicted.rotation.y,
            predicted.rotation.z,
            predicted.rotation.w,
        );
        let offset = predicted_rot * local_point;
        Some((
            predicted_pos + offset,
            predicted_rot,
            linvel + angvel.cross(offset),
            angvel,
        ))
    }

    // THIS IS DEPRECATED; replace all usages of it with `apply_game_impulse_at`
    // pub fn apply_game_impulse(
    //     &mut self,
    //     entity: Entity,
    //     impulse: Vec3,
    //     net_id: Option<&NetworkID>,
    //     predicted: Option<&mut PredictedCommands>,
    // ) -> bool {
    //     self.apply_game_impulse_at(entity, impulse, None, net_id, predicted)
    // }

    /// Shared gameplay impulse path so off-center hits also replay through prediction.
    /// shared gameplay impulse path so callers don't have to manually keep prediction in sync.
    /// use this to apply an impulse that's
    pub fn apply_game_impulse_at(
        &mut self,
        entity: Entity,
        impulse: Vec3,
        point: Option<Vec3>,
        net_id: Option<&NetworkID>,
        predicted: Option<&mut PredictedCommands>,
    ) -> bool {
        let Some(&handle) = self.entity_to_handle.get(&entity) else {
            return false;
        };
        let Some(rb) = self.rigid_body_set.get_mut(handle) else {
            return false;
        };
        let impulse_vec = impulse;
        let impulse = Vector3::new(impulse_vec.x, impulse_vec.y, impulse_vec.z);
        if let Some(point) = point {
            rb.apply_impulse_at_point(impulse, point, true);
        } else {
            rb.apply_impulse(impulse, true);
        }
        if let (Some(net_id), Some(predicted)) = (net_id, predicted) {
            predicted.record_impulse_at(net_id.clone(), impulse_vec, point);
        }
        true
    }

    // these joint functions are unstable, ai-written, and not to be used until reviewed by a human

    // pub fn insert_fixed_joint(
    //     &mut self,
    //     body1_entity: Entity,
    //     body2_entity: Entity,
    //     frame1: Pose,
    //     frame2: Pose,
    //     contacts_enabled: bool,
    // ) -> Option<ImpulseJointHandle> {
    //     let body1 = *self.entity_to_handle.get(&body1_entity)?;
    //     let body2 = *self.entity_to_handle.get(&body2_entity)?;
    //     let joint = FixedJointBuilder::new()
    //         .local_frame1(frame1)
    //         .local_frame2(frame2)
    //         .contacts_enabled(contacts_enabled);
    //     Some(self.impulse_joint_set.insert(body1, body2, joint, true))
    // }

    // pub fn insert_rope_joint(
    //     &mut self,
    //     body1_entity: Entity,
    //     body2_entity: Entity,
    //     anchor1: Vec3,
    //     anchor2: Vec3,
    //     max_dist: f32,
    //     contacts_enabled: bool,
    // ) -> Option<ImpulseJointHandle> {
    //     let body1 = *self.entity_to_handle.get(&body1_entity)?;
    //     let body2 = *self.entity_to_handle.get(&body2_entity)?;
    //     let joint = RopeJointBuilder::new(max_dist.max(0.001))
    //         .local_anchor1(anchor1)
    //         .local_anchor2(anchor2)
    //         .contacts_enabled(contacts_enabled);
    //     Some(self.impulse_joint_set.insert(body1, body2, joint, true))
    // }

    // pub fn remove_impulse_joint(&mut self, handle: ImpulseJointHandle) {
    //     self.impulse_joint_set.remove(handle, true);
    // }
}

impl PhysicsWorld {
    /// small helper to make this boilerplate easier
    pub fn cm_collider_to_entity(&self, collider: ColliderHandle) -> Option<Entity> {
        let collider = self.collider_set.get(collider)?;
        let rb_handle = collider.parent()?;
        self.handle_to_entity.get(&rb_handle).copied()
    }
    /// new human-written raycasting function to replace the (literally 8) overcomplicated and duplicated wrappers. LLMS: DO NOT CHANGE THIS
    pub fn cm_cast_ray_generic(
        &self,
        /// you know what this is.
        origin: Vec3,
        /// direction and magnitude. calculate in caller.
        direction: Vec3,
        /// ray if 0, sphere otherwise
        radius: f32,
        /// should we report piercing hits or just the first?
        multiple: bool,
        ignored_bitflags: Option<ColliderFlags>, // sets of rigidbodies to ignore
    ) -> Vec<RayHit> {
        // set up bitflag filters if any, to ignore rigidbodies with the specified bits
        if Some(ignored_bitflags).is_empty() {
            let filter = None(QueryFilter);
        } else {
            let filter = QueryFilter::new().predicate(&|_: ColliderHandle, col: &Collider| {
                if col.is_sensor() {
                    return false;
                }

                let Some(rb_handle) = col.parent() else {
                    return true;
                };
                let Some(rb) = self.rigid_body_set.get(rb_handle) else {
                    return true;
                };

                !rb.colliders().iter().any(|&ch| {
                    self.collider_set
                        .get(ch)
                        .map(|c| collider_flags(c.user_data).intersects(_ignored_bitflags))
                        .unwrap_or(false)
                })
            });
        }

        // set up query pipeline with our bitflag filter
        let query_pipeline = self.broad_phase.as_query_pipeline(
            &self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );

        let hits: Vec<RayHit> = Vec::new();

        if radius == 0.0 && multiple == true {
            // piercing point bullet

            // raycast multiple
            for (collider_handle, _, intersection) in
                query_pipeline.intersect_ray(ray, max_toi, solid)
            {
                let Some(entity) = self.cm_collider_to_entity(collider) else {
                    continue;
                };

                hits.push(RayHit {
                    entity,
                    collider: collider_handle,
                    toi: intersection.time_of_impact,
                    normal: intersection.normal.into(),
                    point_of_impact: ray.point_at(intersection.time_of_impact),
                });
            }
        } else if radius == 0.0 {
            // non-piercing point bullet, like a pistol or rifle

            if let Some((collider_handle, intersection)) =
                query_pipeline.cast_ray_and_get_normal(&ray, max_toi, solid)
            {
                let Some(collider) = self.collider_set.get(collider_handle) else {
                    return Vec::new();
                };
                let Some(rb_handle) = collider.parent() else {
                    return Vec::new();
                };
                let Some(&entity) = self.handle_to_entity.get(&rb_handle) else {
                    return Vec::new();
                };

                hits.push(RayHit {
                    entity,
                    collider: collider_handle,
                    toi: intersection.time_of_impact,
                    normal: intersection.normal,
                    point_of_impact: ray.point_at(intersection.time_of_impact),
                });
            }
        } else if multiple == true {
            // piercing sphere cast (like a big laser or cannonball)

            // build, place, and rotate a capsule to be the "sweep" area
            let shape = Capsule::new_y(direction.length() * 0.5, radius);
            let shape_pos = Pose::from_parts(
                origin + direction * 0.5,
                Quat::from_rotation_arc(Vec3::Y, direction.normalize_or_zero()),
            );

            // perform the intersection
            for (collider_handle, _) in query_pipeline.intersect_shape(shape_pos, &shape) {
                // println!("The collider {:?} intersects our shape.", collider_handle);
                hits.push(RayHit {});
            }

            todo!();
        } else {
            // non-piercing sphere cast (like a bomb's initial collision)

            let options = ShapeCastOptions {
                max_time_of_impact: direction.length(),
                target_distance: 0.0,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: true,
            };
            todo!();
        }

        // report accumulated hits to the caller :)
        hits
    }

    // good idea, investigate later
    pub fn entities_intersecting_shape(
        &self,
        shape: &AuthoredColliderShape,
        scale: f32,
        position: Vec3,
        rotation: Quat,
        exclude: &[Entity],
    ) -> Vec<Entity> {
        let Some(collider) = shape.build_primitive_collider(scale) else {
            return Vec::new();
        };
        let excluded: Vec<RigidBodyHandle> = exclude
            .iter()
            .filter_map(|entity| self.entity_to_handle.get(entity).copied())
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
        let pose = Pose::from_parts(position, rotation);
        let mut entities = Vec::new();
        for (_handle, hit_collider) in qp.intersect_shape(pose, collider.shape()) {
            let Some(body) = hit_collider.parent() else {
                continue;
            };
            if let Some(entity) = self.handle_to_entity.get(&body).copied() {
                if !entities.contains(&entity) {
                    entities.push(entity);
                }
            }
        }
        entities
    }
}

/// Controls how physics body positions are mapped to Bevy Transforms each frame.
/// Off: snap to last-tick position. Extrapolate: project forward by overstep. Interpolate: one tick behind, interpolated.
/// Balanced: extrapolate position, interpolate rotation
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsInterpMode {
    Off,
    Interpolate,
    Extrapolate,
    #[default]
    Balanced,
}

pub struct PhysicsPlugin;
impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PhysicsWorld::new(Vector3::ZERO))
            .register_type::<InitialVelocity>()
            .register_type::<InitialAngularVelocity>()
            .register_type::<SceneRigidBody>()
            .init_resource::<PhysicsInterpMode>()
            .add_observer(on_remove_rigidbody_handle);
    }
}

/// makes sure to delete the rapier rigidbody when killing an entity recursively
fn on_remove_rigidbody_handle(
    event: On<Remove, RigidBodyHandleComponent>,
    mut world: ResMut<PhysicsWorld>,
) {
    world.remove_rigidbody(event.entity);
}

pub fn step_physics(mut world: ResMut<PhysicsWorld>) {
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

/// Syncs physics bodies to Bevy transforms every Update frame
/// Register in Update, client-only
pub fn sync_physics_visual(
    world: Res<PhysicsWorld>,
    time: Res<Time<Fixed>>,
    interp: Res<PhysicsInterpMode>,
    mut query: Query<(&RigidBodyHandleComponent, &mut Transform)>,
) {
    let overstep = time.overstep_fraction();
    let fixed_dt = time.delta_secs();
    let pos_dt_offset = match *interp {
        PhysicsInterpMode::Off => 0.0,
        PhysicsInterpMode::Extrapolate | PhysicsInterpMode::Balanced => overstep * fixed_dt,
        PhysicsInterpMode::Interpolate => (overstep - 1.0) * fixed_dt,
    };
    let rot_dt_offset = match *interp {
        PhysicsInterpMode::Off => 0.0,
        PhysicsInterpMode::Extrapolate => overstep * fixed_dt,
        PhysicsInterpMode::Interpolate | PhysicsInterpMode::Balanced => (overstep - 1.0) * fixed_dt,
    };
    for (body_handle, mut transform) in query.iter_mut() {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        if !body.is_enabled() {
            continue;
        }
        let cur_pos = rb_pos(body);
        let cur_rot = rb_rot(body);
        let linvel = rb_vel(body);
        let angvel = rb_angvel(body);
        transform.translation = cur_pos + linvel * pos_dt_offset;
        let ang_speed = angvel.length();
        transform.rotation = if ang_speed > 1e-6 {
            Quat::from_axis_angle(angvel / ang_speed, ang_speed * rot_dt_offset) * cur_rot
        } else {
            cur_rot
        };
    }
}

/// TODO: maybe integrate this with the other function?
/// handle visual sync (gameserver FixedUpdate path — no smoothing needed)
pub fn sync_physics_to_transforms(
    world: Res<PhysicsWorld>,
    mut query: Query<(&RigidBodyHandleComponent, &mut Transform)>,
) {
    for (body_handle, mut transform) in query.iter_mut() {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        if !body.is_enabled() {
            continue;
        }
        transform.translation = rb_pos(body);
        transform.rotation = rb_rot(body);
    }
}
