#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use physics::physics_world::PhysicsWorld;

#[cfg(feature = "client")]
use super::*;
use super::{PlayerRegistry, mount};
#[cfg(feature = "client")]
use crate::interaction::InteractionName;
use crate::net::{
    message::NetworkID,
    quic::{QuicManager, SendTarget},
};

/// Marker shared by drivable vehicles.
#[derive(Component)]
pub struct VehicleComponent {
    /// Third-person local-space camera offset used while this vehicle is possessed.
    pub camera_offset: Vec3,
    pub apply_input: fn(&mut PhysicsWorld, Entity, common::PawnInput),
}

impl VehicleComponent {
    pub fn for_vehicle<T: VehiclePawn>() -> Self {
        Self {
            camera_offset: T::CAMERA_OFFSET,
            apply_input: T::apply_input,
        }
    }
}

/// Vehicle-specific tuning required by the generic driver-mount helper.
pub trait VehiclePawn: Component {
    const CAMERA_OFFSET: Vec3;
    const DRIVER_MOUNT_OFFSET: Vec3;
    const DRIVER_INTERACT_RADIUS: f32 = 1.0; // what is this? todo remove
    const EXIT_OFFSET: Vec3 = Vec3::ZERO;
    fn apply_input(world: &mut PhysicsWorld, entity: Entity, input: common::PawnInput);
}

/// Shared plugin for generic vehicle driver-mount behavior.
pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(feature = "client"))]
        let _ = app;
        #[cfg(feature = "client")]
        {
            app.add_systems(
                FixedPreUpdate,
                attach_camera_on_possess_vehicle.in_set(common::game_state::SimulationSystems),
            );
            app.add_systems(
                FixedPreUpdate,
                vehicle_exit_interact.in_set(common::game_state::SimulationSystems),
            );
        }
    }
}

pub fn spawn_driver_mount<T: VehiclePawn>(vehicle_entity: Entity, world: &mut World) -> Entity {
    let anchor = mount::spawn_mount_anchor(vehicle_entity, T::DRIVER_MOUNT_OFFSET, world);
    world
        .entity_mut(vehicle_entity)
        .insert(mount::CharacterMount {
            occupant: None,
            anchor,
            interact_radius: T::DRIVER_INTERACT_RADIUS,
            exit_offset: T::EXIT_OFFSET,
        });
    anchor
}

pub fn handle_vehicle_death(vehicle_entity: Entity, world: &mut World) {
    let biped_net_id = world
        .get::<mount::CharacterMount>(vehicle_entity)
        .and_then(|mount| mount.occupant)
        .and_then(|biped_entity| world.get::<NetworkID>(biped_entity).cloned());
    let Some(biped_entity) = mount::handle_mount_parent_death(vehicle_entity, world) else {
        return;
    };
    crate::health::copy_last_damage_source(world, vehicle_entity, biped_entity);

    #[cfg(feature = "client")]
    mount::clear_mount_possession(vehicle_entity, biped_entity, world);

    let conn_id = world
        .get_resource::<PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(biped_entity));
    if let (Some(conn_id), Some(biped_net_id)) = (conn_id, biped_net_id) {
        world
            .entity_mut(vehicle_entity)
            .remove::<super::Controller>();
        world
            .entity_mut(biped_entity)
            .insert(super::Controller::for_client(conn_id));
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            super::possess_pawn(conn_id, &biped_net_id, &mut quic);
            super::send_mount_state(&mut quic, SendTarget::All, &biped_net_id, None);
        }
    }
}

#[cfg(feature = "client")]
pub fn attach_camera_on_possess_vehicle(
    vehicles: Query<(&VehicleComponent, Entity), Added<Controller>>,
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
    let pivot = commands
        .spawn((
            Transform::default(),
            Visibility::Inherited,
            crate::spring_arm::SpringArm::new(vehicle.camera_offset, 0.2, 5.0),
            crate::spring_arm::SpringArmPivot,
        ))
        .id();
    commands.entity(vehicle_entity).add_child(pivot);
    commands.entity(cam).insert((
        Transform::default(),
        CameraEffector {
            base_translation: Vec3::ZERO,
            base_fov,
            current_fov: base_fov,
            ..default()
        },
    ));
    commands.entity(pivot).add_child(cam);
}

#[cfg(feature = "client")]
fn vehicle_exit_interact(
    state: Res<State<common::game_state::GameState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    ui_wants: Option<Res<common::UiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    vehicle: Query<
        (Entity, Option<&crate::net::message::NetworkID>),
        (With<VehicleComponent>, With<Controller>),
    >,
    mut mounts: Query<&mut mount::CharacterMount>,
    anchor_transforms: Query<&Transform>,
    mut world: ResMut<physics::physics_world::PhysicsWorld>,
    mut commands: Commands,
    mut quic: ResMut<crate::net::quic::QuicManager>,
    mut interaction: ResMut<InteractionGate>,
    ticker: Res<common::tick::Ticker>,
    interaction_names: Query<&InteractionName>,
) {
    use common::game_state::GameState;
    let blocked = ui_wants.is_some_and(|ui| ui.keyboard || ui.pointer);
    let Ok((vehicle_entity, net_id)) = vehicle.single() else {
        return;
    };
    if !interaction.consume_press(
        !blocked
            && bindings.pressed(
                common::InputAction::Interact,
                &keyboard,
                &mouse,
                common::active_gamepad(gamepads.iter()),
            ),
        ticker.tick,
    ) {
        return;
    }
    match state.get() {
        GameState::Multiplayer => {
            let Some(net_id) = net_id else { return };
            quic.send_to_server(
                crate::net::quic::Channel::Ordered,
                &crate::net::message::MsgType::Interact(crate::net::message::Interact(
                    net_id.clone(),
                )),
            );
        }
        GameState::SinglePlayer => {
            let Ok(mut driver_mount) = mounts.get_mut(vehicle_entity) else {
                return;
            };
            let Some(biped_entity) = mount::try_unmount_character(
                &mut world,
                vehicle_entity,
                &mut driver_mount,
                &anchor_transforms,
            ) else {
                return;
            };
            commands.entity(vehicle_entity).remove::<Controller>();
            commands
                .entity(biped_entity)
                .remove::<mount::Mounted>()
                .insert(Controller::new(128));
            if let Ok(name) = interaction_names.get(vehicle_entity) {
                crate::messages::push(&mut commands, format!("Exited {}", name.0));
            }
        }
        _ => {}
    }
}
