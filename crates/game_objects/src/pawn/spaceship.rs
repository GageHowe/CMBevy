use super::vehicle::{DriverSeat, VehicleComponent, VehiclePawn, spawn_driver_seat};
use super::*;
use crate::generic::attach_hull_collider;
use crate::weapon::AimReticle;
use crate::{GameObject, GameObjectKind};
#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use physics::physics_world::*;
use rapier3d::prelude::*;

const HULL_PATH: &str = "collision/placeholder_carrier.obj";
#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/placeholder_carrier.glb#Scene0";
const THRUST: f32 = 2000.0;
const ROLL_SPEED: f32 = 500.0;
const BASE_SENSITIVITY: f32 = 100000.0;
const MAX_TORQUE: f32 = 40000.0;

pub struct SpaceshipPlugin;
impl Plugin for SpaceshipPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(feature = "client"))]
        let _ = app;
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_spaceship_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                move_pawns::<SpaceshipPawnComponent>().in_set(MovePawnsSet),
            )
                .chain(),
        );
    }
}

#[derive(Component, Default, Reflect)]
pub struct SpaceshipPawnComponent;

impl Pawn for SpaceshipPawnComponent {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::Spaceship(i) = input {
            apply_spaceship_movement(world, body, i, self);
        }
    }
}

impl VehiclePawn for SpaceshipPawnComponent {
    const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 15.0, 30.0);
    const DRIVER_SEAT_OFFSET: Vec3 = Vec3::new(0.0, 0.6, -2.0);
    const DRIVER_EXIT_OFFSET: Vec3 = Vec3::new(2.0, 0.0, 0.0);
    const DRIVER_INTERACT_RADIUS: f32 = 0.8;
}

impl GameObject for SpaceshipPawnComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        let driver_seat = spawn_driver_seat::<SpaceshipPawnComponent>(entity, world);
        world.entity_mut(entity).insert((
            SpaceshipPawnComponent,
            VehicleComponent::for_vehicle::<SpaceshipPawnComponent>(driver_seat),
            AimReticle("textures/crosshairs/crosshair001.png", None),
            GameObjectKind::Spaceship,
            Transform::from(transform),
            cmd.net_id.clone(),
        ));
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb = RigidBodyBuilder::dynamic()
                .translation(transform.translation)
                .linvel(Vector3::new(
                    cmd.starting_velocity.x,
                    cmd.starting_velocity.y,
                    cmd.starting_velocity.z,
                ))
                .angular_damping(0.5)
                .build();
            physics.insert_body(entity, rb)
        };
        attach_hull_collider(
            entity,
            rb_handle,
            HULL_PATH,
            1.0,
            ColliderBuilder::cuboid(1.5, 1.0, 3.0),
            world,
        );
        world
            .entity_mut(entity)
            .insert(RigidBodyHandleComponent(rb_handle));
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load(MODEL_PATH);
            world
                .entity_mut(entity)
                .insert((SceneRoot(scene), Visibility::default()));
        }
    }

    fn on_death(entity: Entity, world: &mut World) -> bool {
        let Some((driver_seat_entity, seat_transform)) =
            world.get::<VehicleComponent>(entity).and_then(|vehicle| {
                world
                    .get::<Transform>(vehicle.driver_seat)
                    .cloned()
                    .map(|transform| (vehicle.driver_seat, transform))
            })
        else {
            return true;
        };

        let Some(biped_entity) = world.resource_scope(|world, mut physics: Mut<PhysicsWorld>| {
            let Some(mut cockpit) = world.get_mut::<DriverSeat>(driver_seat_entity) else {
                return None;
            };
            super::vehicle::exit_vehicle(&mut physics, entity, &mut cockpit, &seat_transform)
        }) else {
            return true;
        };

        world
            .entity_mut(biped_entity)
            .remove::<super::SeatedInVehicle>();
        true
    }
}

#[cfg(feature = "client")]
fn gather_spaceship_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawns: Query<&mut Possessed, With<SpaceshipPawnComponent>>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    if cursor_q.grab_mode == CursorGrabMode::None {
        return;
    }
    let Ok(mut possessed) = pawns.single_mut() else {
        return;
    };

    let mut input = common::SpaceshipInput::default();
    if keyboard.pressed(KeyCode::KeyW) {
        input.forward += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        input.forward -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        input.right += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        input.right -= 1.0;
    }
    if keyboard.pressed(KeyCode::Space) {
        input.up += 1.0;
    }
    if keyboard.pressed(KeyCode::ControlLeft) {
        input.up -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyQ) {
        input.roll -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        input.roll += 1.0;
    }
    input.ability1 = keyboard.pressed(KeyCode::ShiftLeft);
    let s = sensitivity.vehicle_pitch_yaw;
    input.yaw = -mouse.delta.x * s;
    input.pitch = -mouse.delta.y * s;

    possessed.push(PawnInputKind::Spaceship(input));
}

pub fn apply_spaceship_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::SpaceshipInput,
    _spaceship: &mut SpaceshipPawnComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };
    if !body.is_enabled() {
        return;
    }
    let rotation = body.rotation();
    let local_right = rotation * Vector3::new(1.0, 0.0, 0.0);
    let local_up = rotation * Vector3::new(0.0, 1.0, 0.0);
    let local_forward = rotation * Vector3::new(0.0, 0.0, -1.0);

    let impulse =
        (local_right * input.right + local_up * input.up + local_forward * input.forward) * THRUST;
    body.apply_impulse(impulse, true);

    let mut torque = local_up * input.yaw;
    torque += local_right * input.pitch;
    torque *= BASE_SENSITIVITY;
    torque += local_forward * input.roll * ROLL_SPEED;
    let torque_mag = torque.length();
    if torque_mag > MAX_TORQUE {
        torque *= MAX_TORQUE / torque_mag;
    }
    body.apply_torque_impulse(torque, true);
}
