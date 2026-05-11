#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{
    vehicle::{VehicleComponent, VehiclePawn, spawn_driver_mount},
    *,
};
use crate::{
    GameObject, GameObjectKind,
    health::{CollisionDamageConfig, Health, LastDamageSource},
    spawn::AppGameObjectExt,
};

#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/kenney-prototypes/shape-cube-recentered.glb#Scene0";
const HALF_EXTENTS: Vec3 = Vec3::new(1.2, 0.45, 2.0);
const TRUCK_MAX_HEALTH: f32 = 1200.0;
const DRIVE_FORCE: f32 = 220.0;
const BRAKE_FORCE: f32 = 320.0;
const SIDEWAYS_GRIP: f32 = 45.0;
const STEER_TORQUE: f32 = 28.0;

pub struct TruckPlugin;
impl Plugin for TruckPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<TruckPawnComponent>();
        #[cfg(not(feature = "client"))]
        let _ = app;
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_truck_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                move_pawns::<TruckPawnComponent>().in_set(MovePawnsSet),
            )
                .chain(),
        );
    }
}

#[derive(Component, Default, Reflect)]
pub struct TruckPawnComponent;

impl Pawn for TruckPawnComponent {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::Truck(input) = input {
            apply_truck_movement(world, body, input, self);
        }
    }
}

impl VehiclePawn for TruckPawnComponent {
    const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 4.0, 8.0);
    const DRIVER_MOUNT_OFFSET: Vec3 = Vec3::new(-0.45, 0.75, 0.2);
    const DRIVER_INTERACT_RADIUS: f32 = 1.2;
    const EXIT_OFFSET: Vec3 = Vec3::new(-1.4, 0.0, 0.0);
}

impl GameObject for TruckPawnComponent {
    const KIND: GameObjectKind = GameObjectKind::Truck;
    const GC_AFTER_SECS: Option<f32> = Some(300.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        spawn_driver_mount::<TruckPawnComponent>(entity, world);
        world.entity_mut(entity).insert((
            TruckPawnComponent,
            Health::new(TRUCK_MAX_HEALTH),
            CollisionDamageConfig {
                threshold_per_mass: 90.0,
                min_threshold: 250.0,
                damage_scale: 0.45,
            },
            LastDamageSource::default(),
            VehicleComponent::for_vehicle::<TruckPawnComponent>(),
            GameObjectKind::Truck,
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
                .linear_damping(0.2)
                .angular_damping(2.2)
                .build();
            let rb_handle = physics.insert_body(entity, rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(transform.rotation, true);
            }
            let collider = ColliderBuilder::cuboid(HALF_EXTENTS.x, HALF_EXTENTS.y, HALF_EXTENTS.z)
                .friction(1.0)
                .build();
            let PhysicsWorld {
                collider_set,
                rigid_body_set,
                ..
            } = &mut *physics;
            collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
            rb_handle
        };
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
        super::vehicle::handle_vehicle_death(entity, world);
        true
    }
}

#[cfg(feature = "client")]
fn gather_truck_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    sensitivity: Res<super::MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut pawns: Query<&mut Possessed, With<TruckPawnComponent>>,
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
    let gamepad = common::active_gamepad(gamepads.iter());
    let move_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.left_stick(), sensitivity.gamepad_move_deadzone)
        })
        .unwrap_or(Vec2::ZERO);

    let mut input = common::TruckInput::default();
    if bindings.pressed(
        common::InputAction::MoveForward,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.throttle += 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveBackward,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.throttle -= 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveRight,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.steer += 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveLeft,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.steer -= 1.0;
    }
    input.throttle = (input.throttle + move_stick.y).clamp(-1.0, 1.0);
    input.steer = (input.steer + move_stick.x).clamp(-1.0, 1.0);
    if bindings.pressed(
        common::InputAction::Crouch,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.brake = 1.0;
    }
    possessed.push(PawnInputKind::Truck(input));
}

pub fn apply_truck_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::TruckInput,
    _truck: &mut TruckPawnComponent,
) {
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };
    if !body.is_enabled() {
        return;
    }

    let rotation = body.rotation();
    let forward = rotation * -Vector3::Z;
    let right = rotation * Vector3::X;
    let up = rotation * Vector3::Y;
    let velocity = body.linvel();
    if !forward.is_finite()
        || !right.is_finite()
        || !up.is_finite()
        || !Vec3::new(velocity.x, velocity.y, velocity.z).is_finite()
    {
        body.set_linvel(Vector3::ZERO, true);
        body.set_angvel(Vector3::ZERO, true);
        return;
    }
    let forward_speed = velocity.dot(forward);
    let sideways_speed = velocity.dot(right);
    body.apply_impulse(forward * (input.throttle * DRIVE_FORCE), true);
    body.apply_impulse(-right * (sideways_speed * SIDEWAYS_GRIP), true);
    body.apply_impulse(
        -(forward * forward_speed + right * sideways_speed) * (input.brake * BRAKE_FORCE),
        true,
    );
    let steer_dir = if forward_speed.abs() > 0.5 {
        forward_speed.signum()
    } else {
        input.throttle.signum()
    };
    if steer_dir != 0.0 {
        body.apply_torque_impulse(up * (input.steer * STEER_TORQUE * steer_dir), true);
    }
}
