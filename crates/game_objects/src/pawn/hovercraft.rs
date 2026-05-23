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
    collision::CollisionFxMaterial,
    generic::attach_hull_collider,
    health::{CollisionDamageConfig, Health, LastDamageSource},
    spawn::AppGameObjectExt,
};

const HULL_PATH: &str = "collision/hovercraft.obj";
#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/hovercraft.glb#Scene0";
const HALF_EXTENTS: Vec3 = Vec3::new(1.5, 0.35, 2.4);
const HOVER_HEIGHT: f32 = 1.6;
const HOVER_RAY_LENGTH: f32 = 2.6;
const HOVER_FORCE: f32 = 170.0;
const HOVER_DAMPING: f32 = 22.0;
const DRIVE_FORCE: f32 = 140.0;
const BRAKE_FORCE: f32 = 180.0;
const SIDEWAYS_GRIP: f32 = 28.0;
const STEER_TORQUE: f32 = 18.0;
const UPRIGHT_TORQUE: f32 = 110.0;
const HOVERCRAFT_MAX_HEALTH: f32 = 1000.0;
const HOVER_POINTS: [Vec3; 4] = [
    Vec3::new(-1.1, 0.0, -1.8),
    Vec3::new(1.1, 0.0, -1.8),
    Vec3::new(-1.1, 0.0, 1.8),
    Vec3::new(1.1, 0.0, 1.8),
];

pub struct HovercraftPlugin;
impl Plugin for HovercraftPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<HovercraftPawnComponent>();
        #[cfg(not(feature = "client"))]
        let _ = app;
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_hovercraft_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                move_pawns::<HovercraftPawnComponent>().in_set(MovePawnsSet),
            )
                .chain(),
        );
    }
}

#[derive(Component, Default, Reflect)]
pub struct HovercraftPawnComponent;

impl Pawn for HovercraftPawnComponent {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::Truck(input) = input {
            apply_hovercraft_movement(world, body, input);
        }
    }
}

impl VehiclePawn for HovercraftPawnComponent {
    const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 4.5, 9.0);
    const DRIVER_MOUNT_OFFSET: Vec3 = Vec3::new(0.0, 0.9, -0.2);
    const DRIVER_INTERACT_RADIUS: f32 = 1.2;
    const EXIT_OFFSET: Vec3 = Vec3::new(-1.6, 0.0, 0.0);
}

impl GameObject for HovercraftPawnComponent {
    const KIND: GameObjectKind = GameObjectKind::Hovercraft;
    const GC_LIFETIME_SECS: Option<f32> = Some(300.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        spawn_driver_mount::<HovercraftPawnComponent>(entity, world);
        world.entity_mut(entity).insert((
            HovercraftPawnComponent,
            Health::new(HOVERCRAFT_MAX_HEALTH, 0.0, 0.0),
            CollisionDamageConfig {
                threshold_per_mass: 90.0,
                min_threshold: 250.0,
                damage_scale: 0.45,
            },
            LastDamageSource::default(),
            VehicleComponent::for_vehicle::<HovercraftPawnComponent>(),
            CollisionFxMaterial::Sparks,
            Transform::from(transform),
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
                .angular_damping(2.8)
                .build();
            let rb_handle = physics.insert_body(entity, rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(transform.rotation, true);
            }
            rb_handle
        };
        attach_hull_collider(
            entity,
            rb_handle,
            HULL_PATH,
            1.0,
            ColliderBuilder::cuboid(HALF_EXTENTS.x, HALF_EXTENTS.y, HALF_EXTENTS.z),
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

    fn on_death(entity: Entity, world: &mut World) {
        super::vehicle::handle_vehicle_death(entity, world);
    }
}

#[cfg(feature = "client")]
fn gather_hovercraft_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    sensitivity: Res<super::MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut pawns: Query<&mut Possessed, With<HovercraftPawnComponent>>,
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

pub fn apply_hovercraft_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::TruckInput,
) {
    let entity = world.handle_to_entity.get(&body_handle.0).copied();
    let Some(body) = world.rigid_body_set.get(body_handle.0) else {
        return;
    };
    if !body.is_enabled() {
        return;
    }

    let rotation = body.rotation();
    let forward = rotation * -Vector3::Z;
    let up = rotation * Vector3::Y;
    let velocity = body.linvel();
    let angvel = body.angvel();
    let center = rb_pos(body);
    let body_rotation = rb_rot(body);

    let mut hit_count = 0.0;
    let mut normal_sum = Vec3::ZERO;
    let mut hover_impulses = Vec::with_capacity(HOVER_POINTS.len());
    for point in HOVER_POINTS {
        let world_point = center + body_rotation * point;
        let Some(hit) = entity.and_then(|entity| {
            world.cast_ray_detailed_ignoring_shields(world_point, -up, HOVER_RAY_LENGTH, &[entity])
        }) else {
            continue;
        };
        let compression = ((HOVER_HEIGHT - hit.toi) / HOVER_HEIGHT).clamp(0.0, 1.0);
        if compression <= 0.0 {
            continue;
        }
        let point_vel = Vec3::new(velocity.x, velocity.y, velocity.z)
            + Vec3::new(angvel.x, angvel.y, angvel.z).cross(world_point - center);
        let vertical_speed = point_vel.dot(Vec3::new(up.x, up.y, up.z));
        let lift = compression * HOVER_FORCE - vertical_speed * HOVER_DAMPING;
        if lift <= 0.0 {
            continue;
        }
        hover_impulses.push((world_point, Vector3::new(up.x, up.y, up.z) * lift));
        normal_sum += hit.normal;
        hit_count += 1.0;
    }

    let support_normal = if hit_count > 0.0 {
        normal_sum / hit_count
    } else {
        Vec3::new(up.x, up.y, up.z)
    }
    .normalize_or_zero();
    let plane_forward = (Vec3::new(forward.x, forward.y, forward.z)
        - support_normal * Vec3::new(forward.x, forward.y, forward.z).dot(support_normal))
    .normalize_or_zero();
    let plane_right = support_normal.cross(plane_forward).normalize_or_zero();
    let body_vel = Vec3::new(velocity.x, velocity.y, velocity.z);
    let forward_speed = body_vel.dot(plane_forward);
    let sideways_speed = body_vel.dot(plane_right);
    let steer_axis = Vector3::new(support_normal.x, support_normal.y, support_normal.z);
    let steer_dir = if forward_speed.abs() > 0.5 {
        forward_speed.signum()
    } else {
        input.throttle.signum()
    };
    let upright_axis = Vec3::new(up.x, up.y, up.z).cross(support_normal);
    let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    };
    for (point, impulse) in hover_impulses {
        body.apply_impulse_at_point(impulse, Vector3::new(point.x, point.y, point.z), true);
    }
    body.apply_impulse(
        Vector3::new(plane_forward.x, plane_forward.y, plane_forward.z) * (input.throttle * DRIVE_FORCE),
        true,
    );
    body.apply_impulse(
        -Vector3::new(plane_right.x, plane_right.y, plane_right.z) * (sideways_speed * SIDEWAYS_GRIP),
        true,
    );
    body.apply_impulse(
        -(Vector3::new(plane_forward.x, plane_forward.y, plane_forward.z) * forward_speed
            + Vector3::new(plane_right.x, plane_right.y, plane_right.z) * sideways_speed)
            * (input.brake * BRAKE_FORCE),
        true,
    );
    if steer_dir != 0.0 {
        body.apply_torque_impulse(steer_axis * (input.steer * steer_dir * STEER_TORQUE), true);
    }
    if upright_axis != Vec3::ZERO {
        body.apply_torque_impulse(
            Vector3::new(upright_axis.x, upright_axis.y, upright_axis.z) * UPRIGHT_TORQUE,
            true,
        );
    }
}
