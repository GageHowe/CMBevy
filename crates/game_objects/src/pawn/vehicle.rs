/// VehicleComponent is a shared marker inserted by every vehicle-type pawn (spaceship, car, etc.).
/// It does NOT implement Pawn — each vehicle type has its own component for that.
/// VehiclePlugin provides the enter/exit lifecycle and camera attachment that work
/// across all vehicle types.
use bevy::prelude::*;
#[cfg(feature = "client")]
use physics::physics_world::sync_physics_visual;
use physics::physics_world::{PhysicsWorld, rb_angvel, rb_pos, rb_rot, rb_vel, step_physics};

use super::Pawn;
#[cfg(feature = "client")]
use super::*;

/// Marks an entity as a driveable vehicle.
#[derive(Component, Reflect)]
pub struct VehicleComponent {
    /// Camera position relative to the vehicle when occupied.
    pub camera_offset: Vec3,
    pub driver_seat: Entity,
}

impl VehicleComponent {
    pub fn for_vehicle<T: VehiclePawn>(driver_seat: Entity) -> Self {
        Self { camera_offset: T::CAMERA_OFFSET, driver_seat }
    }
}

#[derive(Component, Clone, Copy, Reflect)]
pub struct SeatedInVehicle(pub Entity);

pub trait VehiclePawn: Pawn {
    const CAMERA_OFFSET: Vec3;
    const DRIVER_SEAT_OFFSET: Vec3;
    const DRIVER_INTERACT_RADIUS: f32 = 1.0;
}

/// A vehicle seat child entity. The child transform defines the driver anchor.
#[derive(Component, Reflect)]
pub struct DriverSeat {
    pub occupant: Option<Entity>,
    pub interact_radius: f32,
    pub exit_offset: Vec3,
}

impl Default for DriverSeat {
    fn default() -> Self {
        Self { occupant: None, interact_radius: 1.0, exit_offset: Vec3::ZERO }
    }
}

pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<VehicleComponent>();
        app.register_type::<DriverSeat>();
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
    if dist_sq <= cockpit_radius * cockpit_radius { Some(along) } else { None }
}

pub fn spawn_driver_seat<T: VehiclePawn>(vehicle_entity: Entity, world: &mut World) -> Entity {
    let seat = world
        .spawn((
            DriverSeat { interact_radius: T::DRIVER_INTERACT_RADIUS, ..default() },
            Transform::from_translation(T::DRIVER_SEAT_OFFSET),
            Visibility::default(),
        ))
        .id();
    world.entity_mut(vehicle_entity).add_child(seat);
    seat
}

pub fn enter_vehicle(
    world: &mut PhysicsWorld,
    biped_entity: Entity,
    vehicle_entity: Entity,
    seat: &mut DriverSeat,
    seat_transform: &Transform,
) -> bool {
    if seat.occupant.is_some() {
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
    world.set_body_pose(biped_entity, seat_pos, seat_rot, vehicle_vel, vehicle_angvel);
    world.set_body_enabled(biped_entity, false);

    seat.occupant = Some(biped_entity);
    true
}

pub fn exit_vehicle(
    world: &mut PhysicsWorld,
    vehicle_entity: Entity,
    seat: &mut DriverSeat,
    seat_transform: &Transform,
) -> Option<Entity> {
    let biped_entity = seat.occupant.take()?;

    let exit_offset = seat_transform.rotation * seat.exit_offset + seat_transform.translation;
    // let (exit_pos, vehicle_rot, exit_vel, vehicle_angvel) =
    //     world.predicted_body_point(vehicle_entity, exit_offset)?;
    let vehicle_body =
        world.entity_to_handle.get(&vehicle_entity).and_then(|&h| world.rigid_body_set.get(h))?;
    let vehicle_pos = rb_pos(vehicle_body);
    let vehicle_rot = rb_rot(vehicle_body);
    let exit_vel = rb_vel(vehicle_body);
    let vehicle_angvel = rb_angvel(vehicle_body);
    let exit_pos = seat_world_point(vehicle_pos, vehicle_rot, exit_offset);
    let exit_rot = vehicle_rot * seat_transform.rotation;
    world.set_body_enabled(biped_entity, true);
    world.set_body_pose(biped_entity, exit_pos, exit_rot, exit_vel, vehicle_angvel);
    Some(biped_entity)
}

fn sync_seated_bipeds(
    mut world: ResMut<PhysicsWorld>,
    seated: Query<(Entity, &SeatedInVehicle)>,
    vehicles: Query<&VehicleComponent>,
    driver_seats: Query<&Transform, With<DriverSeat>>,
) {
    for (biped_entity, seated_in) in seated.iter() {
        let Some(vehicle_handle) = world.entity_to_handle.get(&seated_in.0).copied() else {
            continue;
        };
        let Some(vehicle_body) = world.rigid_body_set.get(vehicle_handle) else {
            continue;
        };
        let Ok(vehicle) = vehicles.get(seated_in.0) else {
            continue;
        };
        let Ok(seat_transform) = driver_seats.get(vehicle.driver_seat) else {
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
        Query<(&Transform, &VehicleComponent)>,
        Query<&Transform, With<DriverSeat>>,
        Query<&mut Transform>,
    )>,
) {
    for (biped_entity, seated_in) in seated.iter() {
        let (vehicle_translation, vehicle_rotation, driver_seat) = {
            let vehicles = transforms.p0();
            let Ok((vehicle_transform, vehicle)) = vehicles.get(seated_in.0) else {
                continue;
            };
            (vehicle_transform.translation, vehicle_transform.rotation, vehicle.driver_seat)
        };
        let (seat_translation, seat_rotation) = {
            let seats = transforms.p1();
            let Ok(seat_transform) = seats.get(driver_seat) else {
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
pub fn draw_driver_seat_debug(seats: Query<(&DriverSeat, &GlobalTransform)>, mut gizmos: Gizmos) {
    for (seat, gt) in seats.iter() {
        let (_, rot, center) = gt.to_scale_rotation_translation();
        let color = if seat.occupant.is_some() {
            Color::srgba(1.0, 0.2, 0.2, 0.9)
        } else {
            Color::srgba(0.2, 1.0, 0.8, 0.9)
        };
        gizmos.sphere(center, seat.interact_radius, color);
        let exit_tip = center + rot * seat.exit_offset;
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
    let base_fov = if let Projection::Perspective(p) = proj { p.fov.to_degrees() } else { 90.0 };
    commands.entity(cam).insert((
        Transform::from_translation(vehicle.camera_offset),
        CameraEffector {
            base_translation: vehicle.camera_offset,
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
    state: Res<State<common::game_state::GameState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    vehicle: Query<
        (Entity, &VehicleComponent, Option<&net::message::NetworkID>),
        (With<VehicleComponent>, With<Possessed>),
    >,
    mut driver_seats: Query<(&mut DriverSeat, &Transform)>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
    mut interaction: ResMut<InteractionGate>,
    ticker: Res<common::tick::Ticker>,
    object_kinds: Query<&crate::GameObjectKind>,
) {
    use common::game_state::GameState;
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
    let Ok((vehicle_entity, vehicle, net_id)) = vehicle.single() else {
        return;
    };
    if !interaction.consume_press(
        !blocked && bindings.pressed(common::InputAction::Interact, &keyboard, &mouse),
        ticker.tick,
    ) {
        return;
    }
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
            let Ok((mut seat, seat_transform)) = driver_seats.get_mut(vehicle.driver_seat) else {
                return;
            };
            let Some(biped_entity) =
                exit_vehicle(&mut world, vehicle_entity, &mut seat, seat_transform)
            else {
                return;
            };
            commands.entity(vehicle_entity).remove::<Possessed>();
            commands.entity(biped_entity).remove::<SeatedInVehicle>().insert(Possessed::new(128));
            if let Ok(kind) = object_kinds.get(vehicle_entity) {
                crate::messages::push(&mut commands, format!("Exited {kind:?}"));
            }
        }
        _ => {}
    }
}
