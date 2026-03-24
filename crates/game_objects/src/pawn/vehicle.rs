/// VehicleComponent is a shared marker inserted by every vehicle-type pawn (spaceship, car, etc.).
/// It does NOT implement Pawn — each vehicle type has its own component for that.
/// VehiclePlugin provides the enter/exit lifecycle and camera attachment that work
/// across all vehicle types.
use bevy::prelude::*;
#[cfg(feature = "client")]
use super::*;

/// Marks an entity as a driveable vehicle.
/// Automatically inserts a `Cockpit` on the same entity.
#[derive(Component, Reflect)]
#[require(Cockpit)]
pub struct VehicleComponent {
    /// Camera position relative to the vehicle when occupied.
    pub camera_offset: Vec3,
}

/// The cockpit of a vehicle — the interactable attach point where a biped sits.
#[derive(Component, Default, Reflect)]
pub struct Cockpit {
    pub occupant: Option<Entity>,
}

pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<VehicleComponent>();
        app.register_type::<Cockpit>();
        #[cfg(feature = "client")]
        {
            app.add_systems(FixedPreUpdate, attach_camera_on_possess_vehicle);
            app.add_systems(FixedPreUpdate, vehicle_exit_interact);
        }
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
    let Ok((vehicle, vehicle_entity)) = vehicles.single() else { return };
    let Ok((cam, proj)) = camera.single() else { return };
    let base_fov = if let Projection::Perspective(p) = proj { p.fov.to_degrees() } else { 90.0 };
    commands.entity(cam).insert((
        Transform::from_translation(vehicle.camera_offset),
        CameraEffector { base_fov, current_fov: base_fov, ..default() },
    ));
    commands.entity(vehicle_entity).add_child(cam);
}

/// While driving, pressing F exits the vehicle.
/// In multiplayer the server handles the exit; in singleplayer it's handled locally.
#[cfg(feature = "client")]
fn vehicle_exit_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    state: Res<State<common::game_state::GameState>>,
    mut vehicle: Query<(Entity, Option<&net::message::NetworkID>, &mut Cockpit, &GlobalTransform), (With<VehicleComponent>, With<Possessed>)>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
) {
    use common::game_state::GameState;
    if !keyboard.just_pressed(KeyCode::KeyF) { return; }
    let Ok((vehicle_entity, net_id, mut cockpit, vehicle_gt)) = vehicle.single_mut() else { return };
    match state.get() {
        GameState::Multiplayer => {
            let Some(net_id) = net_id else { return };
            quic.send(net::quic::SendTarget::All, net::quic::Channel::Ordered, &net::message::MsgType::Interact(net_id.clone()));
        }
        GameState::SinglePlayer => {
            let Some(biped_entity) = cockpit.occupant.take() else { return };
            let (_, rot, pos) = vehicle_gt.to_scale_rotation_translation();
            let eject_pos = pos + rot * Vec3::X * 4.0;
            world.set_body_enabled(biped_entity, true);
            world.teleport_body(biped_entity, eject_pos);
            commands.entity(vehicle_entity).remove::<Possessed>();
            commands.entity(biped_entity).insert(Possessed::new(128));
        }
        _ => {}
    }
}
