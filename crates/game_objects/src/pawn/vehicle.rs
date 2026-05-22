#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use net::{
    message::{MsgType, NetworkID},
    quic::{Channel, QuicManager, SendTarget},
};

#[cfg(feature = "client")]
use super::*;
use super::{Pawn, PlayerRegistry, mount};
#[cfg(feature = "client")]
use crate::GameObjectKind;

/// Marker shared by drivable vehicles.
#[derive(Component, Reflect)]
pub struct VehicleComponent {
    /// Third-person local-space camera offset used while this vehicle is possessed.
    pub camera_offset: Vec3,
}

impl VehicleComponent {
    pub fn for_vehicle<T: VehiclePawn>() -> Self {
        Self {
            camera_offset: T::CAMERA_OFFSET,
        }
    }
}

/// Vehicle-specific tuning required by the generic driver-mount helper.
pub trait VehiclePawn: Pawn {
    const CAMERA_OFFSET: Vec3;
    const DRIVER_MOUNT_OFFSET: Vec3;
    const DRIVER_INTERACT_RADIUS: f32 = 1.0;
    const EXIT_OFFSET: Vec3 = Vec3::ZERO;
}

/// Shared plugin for generic vehicle driver-mount behavior.
pub struct VehiclePlugin;
impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<VehicleComponent>();
        #[cfg(feature = "client")]
        {
            app.add_systems(FixedPreUpdate, attach_camera_on_possess_vehicle);
            app.add_systems(FixedPreUpdate, vehicle_exit_interact);
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

    world.entity_mut(biped_entity).remove::<mount::Mounted>();
    crate::health::copy_last_damage_source(world, vehicle_entity, biped_entity);

    #[cfg(feature = "client")]
    mount::clear_mount_possession(vehicle_entity, biped_entity, world);

    let conn_id = world
        .get_resource::<PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(biped_entity));
    if let (Some(conn_id), Some(biped_net_id)) = (conn_id, biped_net_id) {
        if let Some(mut registry) = world.get_resource_mut::<PlayerRegistry>() {
            registry.set_controlled_pawn(conn_id, biped_entity, biped_net_id.clone());
        }
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            quic.send(
                SendTarget::One(conn_id),
                Channel::Ordered,
                &MsgType::Possess(biped_net_id.clone()),
            );
            super::broadcast_mount_state(&mut quic, &biped_net_id, None);
        }
    }
}

pub fn handle_server_interact(
    conn_id: net::quic::ConnectionId,
    controlled: Entity,
    character: Entity,
    character_net_id: &NetworkID,
    target: Entity,
    target_net_id: &NetworkID,
    registry: &mut PlayerRegistry,
    quic: &mut QuicManager,
    world: &mut physics::physics_world::PhysicsWorld,
    net_ids: &Query<&NetworkID>,
    vehicles: &Query<&VehicleComponent>,
    mounts: &mut Query<&mut mount::CharacterMount>,
    anchor_transforms: &Query<&Transform>,
    commands: &mut Commands,
) {
    if !vehicles.contains(target) {
        return;
    }
    let Ok(mut driver_mount) = mounts.get_mut(target) else {
        return;
    };

    match mount::handle_mount_interact(
        controlled,
        character,
        target,
        world,
        &mut driver_mount,
        anchor_transforms,
    ) {
        Some(mount::MountInteractResult::Unmounted(biped_entity)) => {
            let Ok(biped_net_id) = net_ids.get(biped_entity) else {
                return;
            };
            commands.entity(biped_entity).remove::<mount::Mounted>();
            super::possess_pawn(conn_id, biped_entity, biped_net_id, registry, quic);
            super::broadcast_mount_state(quic, biped_net_id, None);
        }
        Some(mount::MountInteractResult::Mounted) => {
            commands.entity(character).insert(mount::Mounted(target));
            super::possess_pawn(conn_id, target, target_net_id, registry, quic);
            super::broadcast_mount_state(quic, character_net_id, Some(target_net_id));
        }
        None => {}
    }
}

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
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    vehicle: Query<
        (Entity, Option<&net::message::NetworkID>),
        (With<VehicleComponent>, With<Possessed>),
    >,
    mut mounts: Query<&mut mount::CharacterMount>,
    anchor_transforms: Query<&Transform>,
    mut world: ResMut<physics::physics_world::PhysicsWorld>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
    mut interaction: ResMut<InteractionGate>,
    ticker: Res<common::tick::Ticker>,
    object_kinds: Query<&GameObjectKind>,
) {
    use common::game_state::GameState;
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
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
                net::quic::Channel::Ordered,
                &net::message::MsgType::Interact(net_id.clone()),
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
            commands.entity(vehicle_entity).remove::<Possessed>();
            commands
                .entity(biped_entity)
                .remove::<mount::Mounted>()
                .insert(Possessed::new(128));
            if let Ok(kind) = object_kinds.get(vehicle_entity) {
                crate::messages::push(&mut commands, format!("Exited {}", kind.interaction_name()));
            }
        }
        _ => {}
    }
}
