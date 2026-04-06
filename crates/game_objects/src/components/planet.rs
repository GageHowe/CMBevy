use crate::pawn::{BipedPawnComponent, SeatedInVehicle};
use bevy::prelude::*;
use physics::physics_world::{self, *};
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

#[deprecated]
pub const GRAVITY_STRENGTH: f32 = 9.81;

#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component, Default)]
pub enum GravityProfile {
    InverseSquare(f32),
    Linear(f32),
    Constant(f32),
}

impl Default for GravityProfile {
    fn default() -> Self {
        Self::Constant(0.0)
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Reflect, Default)]
#[reflect(Component, Default)]
pub struct PlanetComponent {
    pub inner_radius: u32,
    pub snap_radius: u32,
    pub gravity_radius: u32,
    pub gravity_profile: GravityProfile,
    pub scene: Option<String>,
}

pub fn spawn(
    planet: PlanetComponent,
    transform: Transform,
    commands: &mut Commands,
    _world: &mut PhysicsWorld,
) -> Entity {
    commands.spawn((planet, transform)).id()
}

pub fn apply_gravity_impulses(
    world: &mut PhysicsWorld,
    planets: &Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
    gravity_scales: &Query<&physics_world::GravityScale>,
    seated: &Query<&SeatedInVehicle>,
) {
    let dt = world.integration_parameters.dt;

    let planet_data: Vec<(Vec3, &PlanetComponent, RigidBodyHandle)> = planets
        .iter()
        .filter_map(|(planet, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            Some((rb_pos(rb), planet, handle.0))
        })
        .collect();

    let mut impulses: Vec<(RigidBodyHandle, Vector)> = Vec::new();
    let mut vel_deltas: Vec<(RigidBodyHandle, Vector)> = Vec::new();

    for (planet_center, planet, planet_handle) in &planet_data {
        let gravity_radius = planet.gravity_radius as f32;
        let inner_radius = planet.inner_radius as f32;

        let affected_handles: Vec<RigidBodyHandle> = if planet.gravity_radius == 0 {
            world
                .rigid_body_set
                .iter()
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
            let collider_handles: Vec<ColliderHandle> = qp
                .intersect_shape(shape_pos, &shape)
                .map(|(ch, _)| ch)
                .collect();
            collider_handles
                .iter()
                .filter_map(|ch| world.collider_set.get(*ch).and_then(|c| c.parent()))
                .collect()
        };

        for rb_handle in affected_handles {
            if world
                .handle_to_entity
                .get(&rb_handle)
                .and_then(|e| seated.get(*e).ok())
                .is_some()
            {
                continue;
            }
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            if !rb.is_enabled() {
                continue;
            }

            let to_planet = *planet_center - rb_pos(rb);
            let dist = to_planet.length();
            if dist < inner_radius || dist < 0.001 {
                continue;
            }

            let strength = match planet.gravity_profile {
                GravityProfile::InverseSquare(s) => s / (dist * dist),
                GravityProfile::Linear(s) => {
                    s * if gravity_radius > 0.0 {
                        1.0 - (dist / gravity_radius).min(1.0)
                    } else {
                        1.0
                    }
                }
                GravityProfile::Constant(s) => s,
            };

            let scale = world
                .handle_to_entity
                .get(&rb_handle)
                .and_then(|e| gravity_scales.get(*e).ok())
                .map_or(1.0, |gs| gs.0);
            if scale == 0.0 {
                continue;
            }

            let dir = to_planet / dist;
            if rb.is_dynamic() {
                impulses.push((rb_handle, dir * strength * scale * rb.mass() * dt));
            } else if rb.is_kinematic() {
                vel_deltas.push((rb_handle, dir * strength * scale * dt));
            }
        }
    }

    for (handle, impulse) in impulses {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            rb.apply_impulse(impulse, true);
        }
    }
    for (handle, dv) in vel_deltas {
        if let Some(rb) = world.rigid_body_set.get_mut(handle) {
            let v = rb.linvel();
            rb.set_linvel(Vector::new(v.x + dv.x, v.y + dv.y, v.z + dv.z), true);
        }
    }
}

pub fn apply_gravity(
    mut world: ResMut<PhysicsWorld>,
    planets: Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
    gravity_scales: Query<&physics_world::GravityScale>,
    seated: Query<&SeatedInVehicle>,
) {
    apply_gravity_impulses(&mut world, &planets, &gravity_scales, &seated);
}

const ORIENT_SPEED: f32 = 3.0;

pub fn orient_bipeds_to_planets(
    mut world: ResMut<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponent, (With<BipedPawnComponent>, Without<SeatedInVehicle>)>,
    planets: Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
) {
    orient_bipeds_to_planets_impulses(&mut world, &bipeds, &planets);
}

pub fn orient_bipeds_to_planets_impulses(
    world: &mut PhysicsWorld,
    bipeds: &Query<
        &RigidBodyHandleComponent,
        (With<BipedPawnComponent>, Without<SeatedInVehicle>),
    >,
    planets: &Query<(&PlanetComponent, &RigidBodyHandleComponent)>,
) {
    let dt = world.integration_parameters.dt;

    let planet_data: Vec<(Vec3, f32)> = planets
        .iter()
        .filter_map(|(planet, handle)| {
            let rb = world.rigid_body_set.get(handle.0)?;
            Some((rb_pos(rb), planet.snap_radius as f32))
        })
        .collect();

    let biped_handles: Vec<RigidBodyHandle> = bipeds.iter().map(|h| h.0).collect();

    for rb_handle in biped_handles {
        let (pos, current_rot) = {
            let Some(rb) = world.rigid_body_set.get(rb_handle) else {
                continue;
            };
            (rb_pos(rb), rb_rot(rb))
        };

        let nearest = planet_data
            .iter()
            .filter_map(|(center, snap_radius)| {
                let dist = pos.distance(*center);
                if *snap_radius == 0.0 || dist <= *snap_radius {
                    Some((*center, dist))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let Some(rb) = world.rigid_body_set.get_mut(rb_handle) else {
            continue;
        };
        let Some((planet_center, _)) = nearest else {
            continue;
        };

        rb.lock_rotations(true, false);

        let desired_up = (pos - planet_center).normalize();
        let current_forward = current_rot * Vec3::NEG_Z;
        let forward_proj = {
            let proj = current_forward - current_forward.dot(desired_up) * desired_up;
            if proj.length_squared() > 1e-6 {
                proj.normalize()
            } else {
                let alt = if desired_up.abs().x < 0.9 {
                    Vec3::X
                } else {
                    Vec3::Z
                };
                (alt - alt.dot(desired_up) * desired_up).normalize()
            }
        };

        let right = forward_proj.cross(desired_up).normalize();
        let back = right.cross(desired_up).normalize();
        let target_rot = Quat::from_mat3(&Mat3::from_cols(right, desired_up, back));

        let rot = current_rot.slerp(target_rot, (ORIENT_SPEED * dt).min(1.0));
        rb.set_rotation(rot, false);
    }
}

pub fn draw_planet_radii(planets: Query<(&PlanetComponent, &GlobalTransform)>, mut gizmos: Gizmos) {
    for (planet, gt) in planets.iter() {
        let pos = gt.translation();
        if planet.inner_radius > 0 {
            gizmos.sphere(
                Isometry3d::from_translation(pos),
                planet.inner_radius as f32,
                Color::srgba(0.8, 0.2, 0.2, 0.15),
            );
        }
        if planet.snap_radius > 0 {
            gizmos.sphere(
                Isometry3d::from_translation(pos),
                planet.snap_radius as f32,
                Color::srgba(0.9, 0.8, 0.1, 0.15),
            );
        }
        if planet.gravity_radius > 0 {
            gizmos.sphere(
                Isometry3d::from_translation(pos),
                planet.gravity_radius as f32,
                Color::srgba(0.2, 0.8, 0.2, 0.15),
            );
        }
    }
}

#[cfg(feature = "client")]
fn spawn_planet_visuals(
    mut commands: Commands,
    planets: Query<(Entity, &PlanetComponent), Added<PlanetComponent>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, planet) in planets.iter() {
        let Some(scene) = &planet.scene else {
            continue;
        };
        commands.entity(entity).insert((
            SceneRoot(asset_server.load(crate::asset_path::resolve_asset_path(scene))),
            Visibility::default(),
        ));
    }
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<GravityProfile>()
            .register_type::<PlanetComponent>()
            .add_systems(
                FixedUpdate,
                (apply_gravity, orient_bipeds_to_planets).before(step_physics),
            );
        #[cfg(feature = "client")]
        app.add_systems(Update, spawn_planet_visuals);
    }
}
