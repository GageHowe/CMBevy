#[cfg(feature = "client")]
use super::*;
/// VehicleComponent is a shared marker inserted by every vehicle-type pawn (spaceship, car, etc.).
/// It does NOT implement Pawn — each vehicle type has its own component for that.
/// VehiclePlugin provides the enter/exit lifecycle and camera attachment that work
/// across all vehicle types.
use bevy::prelude::*;
use physics::physics_world::{
    PhysicsWorld, rb_angvel, rb_pos, rb_rot, rb_vel, step_physics,
};
#[cfg(feature = "client")]
use physics::physics_world::sync_physics_visual;

/// Marks an entity as a driveable vehicle.
#[derive(Component, Reflect)]
pub struct VehicleComponent {
    /// Camera position relative to the vehicle when occupied.
    pub camera_offset: Vec3,
}

#[derive(Component, Clone, Copy, Reflect)]
pub struct SeatedInVehicle(pub Entity);

/// A vehicle seat child entity. The child transform defines the seat anchor.
#[derive(Component, Reflect)]
pub struct Cockpit {
    pub occupant: Option<Entity>,
    pub interact_radius: f32,
    pub exit_offset: Vec3,
}

impl Default for Cockpit {
    fn default() -> Self {
        Self {
            occupant: None,
            interact_radius: 1.0,
            exit_offset: Vec3::X * 4.0,
        }
    }
}

pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<VehicleComponent>();
        app.register_type::<Cockpit>();
        app.add_systems(FixedUpdate, sync_seated_bipeds.before(step_physics));
        #[cfg(feature = "client")]
        {
            app.add_systems(Update, sync_seated_biped_visuals.after(sync_physics_visual));
            app.add_systems(FixedPreUpdate, attach_camera_on_possess_vehicle);
            app.add_systems(FixedPreUpdate, vehicle_exit_interact);
        }
    }
}

pub fn seat_world_point(vehicle_pos: Vec3, vehicle_rot: Quat, seat_local: Vec3) -> Vec3 {
    vehicle_pos + vehicle_rot * seat_local
}

pub fn ray_hits_cockpit(
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
    seat_center: Vec3,
    cockpit_radius: f32,
) -> Option<f32> {
    let offset = seat_center - origin;
    let along = offset.dot(dir);
    if along < 0.0 || along > max_distance {
        return None;
    }
    let closest = origin + dir * along;
    let dist_sq = seat_center.distance_squared(closest);
    if dist_sq <= cockpit_radius * cockpit_radius {
        Some(along)
    } else {
        None
    }
}

pub fn enter_vehicle(
    world: &mut PhysicsWorld,
    biped_entity: Entity,
    vehicle_entity: Entity,
    cockpit: &mut Cockpit,
    seat_transform: &Transform,
) -> bool {
    if cockpit.occupant.is_some() {
        return false;
    }

    let Some(&vehicle_handle) = world.entity_to_handle.get(&vehicle_entity) else {
        return false;
    };
    let Some(vehicle_body) = world.rigid_body_set.get(vehicle_handle) else {
        return false;
    };
    let vehicle_pos = rb_pos(vehicle_body);
    let vehicle_rot = rb_rot(vehicle_body);
    let vehicle_vel = rb_vel(vehicle_body);
    let vehicle_angvel = rb_angvel(vehicle_body);
    let seat_pos = seat_world_point(vehicle_pos, vehicle_rot, seat_transform.translation);
    let seat_rot = vehicle_rot * seat_transform.rotation;
    world.set_body_pose(
        biped_entity,
        seat_pos,
        seat_rot,
        vehicle_vel,
        vehicle_angvel,
    );
    world.set_body_enabled(biped_entity, false);

    cockpit.occupant = Some(biped_entity);
    true
}

pub fn exit_vehicle(
    world: &mut PhysicsWorld,
    vehicle_entity: Entity,
    cockpit: &mut Cockpit,
    seat_transform: &Transform,
) -> Option<Entity> {
    let biped_entity = cockpit.occupant.take()?;

    let vehicle_body = world
        .entity_to_handle
        .get(&vehicle_entity)
        .and_then(|&h| world.rigid_body_set.get(h))?;
    let vehicle_pos = rb_pos(vehicle_body);
    let vehicle_rot = rb_rot(vehicle_body);
    let vehicle_vel = rb_vel(vehicle_body);
    let vehicle_angvel = rb_angvel(vehicle_body);
    let exit_offset = seat_transform.rotation * cockpit.exit_offset + seat_transform.translation;
    let exit_pos = seat_world_point(vehicle_pos, vehicle_rot, exit_offset);
    let exit_rot = vehicle_rot * seat_transform.rotation;
    world.set_body_enabled(biped_entity, true);
    world.set_body_pose(
        biped_entity,
        exit_pos,
        exit_rot,
        vehicle_vel,
        vehicle_angvel,
    );
    Some(biped_entity)
}

fn sync_seated_bipeds(
    mut world: ResMut<PhysicsWorld>,
    seated: Query<(Entity, &SeatedInVehicle)>,
    cockpits: Query<(&Transform, &ChildOf), With<Cockpit>>,
) {
    for (biped_entity, seated_in) in seated.iter() {
        let Some(vehicle_handle) = world.entity_to_handle.get(&seated_in.0).copied() else {
            continue;
        };
        let Some(vehicle_body) = world.rigid_body_set.get(vehicle_handle) else {
            continue;
        };
        let Some((seat_transform, _)) = cockpits
            .iter()
            .find(|(_, child_of)| child_of.parent() == seated_in.0)
        else {
            continue;
        };
        let vehicle_pos = rb_pos(vehicle_body);
        let vehicle_rot = rb_rot(vehicle_body);
        let seat_pos = seat_world_point(vehicle_pos, vehicle_rot, seat_transform.translation);
        let seat_rot = vehicle_rot * seat_transform.rotation;
        let vehicle_vel = rb_vel(vehicle_body);
        let vehicle_angvel = rb_angvel(vehicle_body);
        world.set_body_pose(biped_entity, seat_pos, seat_rot, vehicle_vel, vehicle_angvel);
    }
}

#[cfg(feature = "client")]
fn sync_seated_biped_visuals(
    seated: Query<(Entity, &SeatedInVehicle)>,
    mut transforms: ParamSet<(
        Query<&Transform, With<VehicleComponent>>,
        Query<(&Transform, &ChildOf), With<Cockpit>>,
        Query<&mut Transform>,
    )>,
) {
    for (biped_entity, seated_in) in seated.iter() {
        let (vehicle_translation, vehicle_rotation) = {
            let vehicles = transforms.p0();
            let Ok(vehicle_transform) = vehicles.get(seated_in.0) else {
                continue;
            };
            (vehicle_transform.translation, vehicle_transform.rotation)
        };
        let (seat_translation, seat_rotation) = {
            let cockpits = transforms.p1();
            let Some((seat_transform, _)) = cockpits
                .iter()
                .find(|(_, child_of)| child_of.parent() == seated_in.0)
            else {
                continue;
            };
            (seat_transform.translation, seat_transform.rotation)
        };
        let mut bipeds = transforms.p2();
        let Ok(mut biped_transform) = bipeds.get_mut(biped_entity) else {
            continue;
        };
        // Seated riders are rendered from the vehicle's visual frame so interpolation keeps
        // them attached to the cockpit instead of drifting from their disabled rigid body.
        biped_transform.translation = vehicle_translation + vehicle_rotation * seat_translation;
        biped_transform.rotation = vehicle_rotation * seat_rotation;
    }
}

#[cfg(feature = "client")]
pub fn draw_cockpit_debug(cockpits: Query<(&Cockpit, &GlobalTransform)>, mut gizmos: Gizmos) {
    for (cockpit, gt) in cockpits.iter() {
        let (_, rot, center) = gt.to_scale_rotation_translation();
        let color = if cockpit.occupant.is_some() {
            Color::srgba(1.0, 0.2, 0.2, 0.9)
        } else {
            Color::srgba(0.2, 1.0, 0.8, 0.9)
        };
        gizmos.sphere(center, cockpit.interact_radius, color);
        let exit_tip = center + rot * cockpit.exit_offset;
        gizmos.line(center, exit_tip, Color::srgba(1.0, 0.8, 0.2, 0.9));
    }
}

/// Re-parents the camera into the vehicle when any vehicle type gains Possessed.
/// The offset should eventually be defined per vehicle type; a sensible default is used here.
#[cfg(feature = "client")]
pub fn attach_camera_on_possess_vehicle(
    vehicles: Query<(&VehicleComponent, Entity), Added<Possessed>>,
    camera: Query<(Entity, &Projection), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok((vehicle, vehicle_entity)) = vehicles.single() else {
        return;
    };
    let Ok((cam, proj)) = camera.single() else {
        return;
    };
    let base_fov = if let Projection::Perspective(p) = proj {
        p.fov.to_degrees()
    } else {
        90.0
    };
    commands.entity(cam).insert((
        Transform::from_translation(vehicle.camera_offset),
        CameraEffector {
            base_fov,
            current_fov: base_fov,
            ..default()
        },
    ));
    commands.entity(vehicle_entity).add_child(cam);
}

/// While driving, pressing F exits the vehicle.
/// In multiplayer the server handles the exit; in singleplayer it's handled locally.
#[cfg(feature = "client")]
fn vehicle_exit_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<common::game_state::GameState>>,
    vehicle: Query<
        (Entity, Option<&net::message::NetworkID>),
        (With<VehicleComponent>, With<Possessed>),
    >,
    mut cockpits: Query<(&mut Cockpit, &Transform, &ChildOf)>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
) {
    use common::game_state::GameState;
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }
    let Ok((vehicle_entity, net_id)) = vehicle.single() else {
        return;
    };
    match state.get() {
        GameState::Multiplayer => {
            let Some(net_id) = net_id else { return };
            quic.send(
                net::quic::SendTarget::All,
                net::quic::Channel::Ordered,
                &net::message::MsgType::Interact(net_id.clone()),
            );
        }
        GameState::SinglePlayer => {
            let Some((mut cockpit, seat_transform, _)) = cockpits
                .iter_mut()
                .find(|(_, _, child_of)| child_of.parent() == vehicle_entity)
            else {
                return;
            };
            let Some(biped_entity) =
                exit_vehicle(&mut world, vehicle_entity, &mut cockpit, seat_transform)
            else {
                return;
            };
            commands.entity(vehicle_entity).remove::<Possessed>();
            commands
                .entity(biped_entity)
                .remove::<SeatedInVehicle>()
                .insert(Possessed::new(128));
        }
        _ => {}
    }
}
