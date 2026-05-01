#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{
    rocket_turret,
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
const SUSPENSION_REST: f32 = 0.5;
const WHEEL_RADIUS: f32 = 0.45;
const SPRING_STIFFNESS: f32 = 22000.0;
const SPRING_DAMPING: f32 = 2800.0;
const DRIVE_FORCE: f32 = 9000.0;
const BRAKE_FORCE: f32 = 5500.0;
const LATERAL_GRIP: f32 = 4200.0;
const MAX_STEER_ANGLE: f32 = 0.45;

const WHEELS: [Wheel; 4] = [
    Wheel::new(Vec3::new(-0.95, -0.2, 1.35), true, true),
    Wheel::new(Vec3::new(0.95, -0.2, 1.35), true, true),
    Wheel::new(Vec3::new(-0.95, -0.2, -1.35), false, true),
    Wheel::new(Vec3::new(0.95, -0.2, -1.35), false, true),
];

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

#[derive(Clone, Copy)]
struct Wheel {
    anchor: Vec3,
    steer: bool,
    drive: bool,
}

impl Wheel {
    const fn new(anchor: Vec3, steer: bool, drive: bool) -> Self {
        Self { anchor, steer, drive }
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
                max_damage_per_hit: Some(90.0),
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
                .linear_damping(0.15)
                .angular_damping(1.8)
                .build();
            let rb_handle = physics.insert_body(entity, rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(transform.rotation, true);
            }
            let collider = ColliderBuilder::cuboid(HALF_EXTENTS.x, HALF_EXTENTS.y, HALF_EXTENTS.z)
                .friction(1.0)
                .build();
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(collider, rb_handle, rigid_body_set);
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        rocket_turret::spawn_attached_to_truck(entity, Vec3::new(0.0, 1.1, -2.2), world);
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load(MODEL_PATH);
            world.entity_mut(entity).insert((SceneRoot(scene), Visibility::default()));
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
        .map(|gamepad| common::stick_with_deadzone(gamepad.left_stick(), sensitivity.gamepad_move_deadzone))
        .unwrap_or(Vec2::ZERO);

    let mut input = common::TruckInput::default();
    if bindings.pressed(common::InputAction::MoveForward, &keyboard, &mouse_buttons, gamepad) {
        input.throttle += 1.0;
    }
    if bindings.pressed(common::InputAction::MoveBackward, &keyboard, &mouse_buttons, gamepad) {
        input.throttle -= 1.0;
    }
    if bindings.pressed(common::InputAction::MoveRight, &keyboard, &mouse_buttons, gamepad) {
        input.steer += 1.0;
    }
    if bindings.pressed(common::InputAction::MoveLeft, &keyboard, &mouse_buttons, gamepad) {
        input.steer -= 1.0;
    }
    input.throttle = (input.throttle + move_stick.y).clamp(-1.0, 1.0);
    input.steer = (input.steer + move_stick.x).clamp(-1.0, 1.0);
    if bindings.pressed(common::InputAction::Crouch, &keyboard, &mouse_buttons, gamepad) {
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
    let Some(entity) = world.handle_to_entity.get(&body_handle.0).copied() else {
        return;
    };
    let dt = world.integration_parameters.dt;
    let mut wheel_impulses = Vec::with_capacity(WHEELS.len() * 3);

    {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            return;
        };
        if !body.is_enabled() {
            return;
        }

        let position = rb_pos(body);
        let rotation = rb_rot(body);
        let up = rotation * Vec3::Y;
        let down = -up;
        let forward = rotation * -Vec3::Z;
        let ray_dir = down.normalize_or_zero();

        for wheel in WHEELS {
            let anchor = position + rotation * wheel.anchor;
            let Some((_hit, toi)) =
                world.cast_ray(anchor, ray_dir, SUSPENSION_REST + WHEEL_RADIUS, &[entity])
            else {
                continue;
            };

            let suspension = (toi - WHEEL_RADIUS).clamp(0.0, SUSPENSION_REST);
            let compression = SUSPENSION_REST - suspension;
            if compression <= 0.0 {
                continue;
            }

            let contact = anchor + ray_dir * toi;
            let point_velocity = rb_point_vel(body, contact);
            let suspension_speed = point_velocity.dot(ray_dir);
            let spring_force =
                (compression * SPRING_STIFFNESS - suspension_speed * SPRING_DAMPING).max(0.0);
            wheel_impulses.push((contact, -ray_dir * spring_force * dt));

            let steer_angle = if wheel.steer { input.steer * MAX_STEER_ANGLE } else { 0.0 };
            let wheel_forward = Quat::from_axis_angle(up, steer_angle) * forward;
            let wheel_right = wheel_forward.cross(up).normalize_or_zero();
            let forward_speed = point_velocity.dot(wheel_forward);
            let lateral_speed = point_velocity.dot(wheel_right);

            if wheel.drive {
                wheel_impulses.push((contact, wheel_forward * (input.throttle * DRIVE_FORCE * dt)));
            }
            wheel_impulses.push((
                contact,
                -wheel_forward * (forward_speed * input.brake * BRAKE_FORCE * dt),
            ));
            wheel_impulses.push((contact, -wheel_right * (lateral_speed * LATERAL_GRIP * dt)));
        }
    }

    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };
    for (point, impulse) in wheel_impulses {
        if impulse != Vec3::ZERO {
            body.apply_impulse_at_point(impulse, point, true);
        }
    }
}
