/// VehicleComponent is a shared marker inserted by every vehicle-type pawn (spaceship, car, etc.).
/// It does NOT implement Pawn — each vehicle type has its own component for that.
/// VehiclePlugin provides the enter/exit lifecycle and camera attachment that work
/// across all vehicle types.
use bevy::prelude::*;
#[cfg(feature = "client")]
use super::*;

/// Marks an entity as a driveable vehicle.
/// Inserted by every vehicle-type pawn during spawn alongside its type-specific component.
#[derive(Component, Default, Reflect)]
pub struct VehicleComponent {
    /// biped entity currently at the controls; None = unoccupied.
    pub driver: Option<Entity>,
}

pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<VehicleComponent>();
        #[cfg(feature = "client")]
        {
            use common::game_state::GameState;
            app.add_systems(Update, attach_camera_on_possess_vehicle);
            app.add_systems(Update, vehicle_exit_interact
                .run_if(in_state(GameState::Multiplayer)));
        }
    }
}

/// Re-parents the camera into the vehicle when any vehicle type gains Possessed.
/// The offset should eventually be defined per vehicle type; a sensible default is used here.
#[cfg(feature = "client")]
pub fn attach_camera_on_possess_vehicle(
    vehicles: Query<Entity, (With<VehicleComponent>, Added<Possessed>)>,
    camera: Query<(Entity, &Projection), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok(vehicle_entity) = vehicles.single() else { return };
    let Ok((cam, proj)) = camera.single() else { return };
    let base_fov = if let Projection::Perspective(p) = proj { p.fov.to_degrees() } else { 90.0 };
    commands.entity(cam).insert((
        Transform::from_xyz(0.0, 0.5, -2.5),
        CameraEffector { base_fov, current_fov: base_fov, ..default() },
    ));
    commands.entity(vehicle_entity).add_child(cam);
}

/// While driving, pressing F sends Interact(vehicle_net_id) → server ejects the player.
#[cfg(feature = "client")]
fn vehicle_exit_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    vehicle: Query<&net::message::NetworkID, (With<VehicleComponent>, With<Possessed>)>,
    mut quic: ResMut<net::quic::QuicManager>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) { return; }
    let Ok(net_id) = vehicle.single() else { return };
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Ordered,
        &net::message::MsgType::Interact(net_id.clone()),
    );
}
