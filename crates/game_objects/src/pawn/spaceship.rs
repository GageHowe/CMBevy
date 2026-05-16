#[cfg(feature = "client")]
use bevy::input::{gamepad::Gamepad, mouse::AccumulatedMouseMotion};
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
#[cfg(feature = "client")]
use bevy_hanabi_plugin::prelude::spawn_spaceship_death_explosion_effect;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{
    vehicle::{VehicleComponent, VehiclePawn, spawn_driver_mount},
    *,
};
#[cfg(feature = "client")]
use crate::flash::spawn_flash;
#[cfg(feature = "client")]
use crate::shield::spawn_box_shield_visual;
use crate::{
    GameObject, GameObjectKind,
    collision::CollisionFxMaterial,
    generic::attach_hull_collider,
    health::{CollisionDamageConfig, Health, LastDamageSource},
    reticle::AimReticle,
    shield::attach_shield_collider,
    spawn::AppGameObjectExt,
};

const HULL_PATH: &str = "collision/placeholder_carrier.obj";
#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/placeholder_carrier.glb#Scene0";
const THRUST: f32 = 2000.0;
const ROLL_SPEED: f32 = 500.0;
const BASE_SENSITIVITY: f32 = 100000.0;
const MAX_TORQUE: f32 = 40000.0;
const SPACESHIP_MAX_HEALTH: f32 = 1500.0;
const SHIELD_MAX_HEALTH: f32 = 300.0;
const SHIELD_REGEN_PER_SEC: f32 = 60.0;
const SHIELD_REGEN_DELAY_SECS: f32 = 5.0;

pub struct SpaceshipPlugin;
impl Plugin for SpaceshipPlugin {
    fn build(&self, _app: &mut App) {
        _app.register_game_object::<SpaceshipPawnComponent>();
        #[cfg(feature = "client")]
        _app.add_systems(
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
    const DRIVER_MOUNT_OFFSET: Vec3 = Vec3::new(0.0, 0.6, -2.0);
    const DRIVER_INTERACT_RADIUS: f32 = 0.8;
}

impl GameObject for SpaceshipPawnComponent {
    const KIND: GameObjectKind = GameObjectKind::Spaceship;
    const GC_AFTER_SECS: Option<f32> = Some(300.0);
    const SPLASH_DAMAGE_USES_CENTER_OF_MASS: bool = false;

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        spawn_driver_mount::<SpaceshipPawnComponent>(entity, world);
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
            let rb_handle = physics.insert_body(entity, rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(transform.rotation, true);
            }
            rb_handle
        };
        #[allow(unused_mut)]
        let mut shield = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            attach_shield_collider(
                rb_handle,
                ColliderBuilder::cuboid(1.9, 1.4, 3.4).build(),
                SHIELD_MAX_HEALTH,
                SHIELD_REGEN_PER_SEC,
                SHIELD_REGEN_DELAY_SECS,
                &mut physics,
            )
        };
        #[cfg(feature = "client")]
        {
            shield.visual = Some(spawn_box_shield_visual(world, entity, Vec3::new(1.9, 1.4, 3.4)));
        }
        world.entity_mut(entity).insert((
            SpaceshipPawnComponent,
            Health::new(SPACESHIP_MAX_HEALTH, 0.0, 0.0),
            CollisionDamageConfig {
                threshold_per_mass: 120.0,
                min_threshold: 400.0,
                damage_scale: 0.5,
            },
            LastDamageSource::default(),
            VehicleComponent::for_vehicle::<SpaceshipPawnComponent>(),
            CollisionFxMaterial::Sparks,
            AimReticle("textures/crosshairs/crosshair001.png", None),
            GameObjectKind::Spaceship,
            shield,
            Transform::from(transform),
            cmd.net_id.clone(),
        ));
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
        #[cfg(feature = "client")]
        {
            let (position, velocity) = world
                .resource::<PhysicsWorld>()
                .entity_to_handle
                .get(&entity)
                .and_then(|&handle| {
                    let physics = world.resource::<PhysicsWorld>();
                    let rb = physics.rigid_body_set.get(handle)?;
                    Some((rb_pos(rb), rb_vel(rb)))
                })
                .or_else(|| {
                    world
                        .get::<Transform>(entity)
                        .map(|t| (t.translation, Vec3::ZERO))
                })
                .unwrap_or((Vec3::ZERO, Vec3::ZERO));
            spawn_spaceship_death_explosion_effect(world, position, velocity);
            spawn_flash(
                world,
                position,
                18.0,
                Color::srgb(1.0, 0.9, 0.1),
                true, // make the light visible even when not in frustum
                10000.0,
                10.0, // mesh brightness decay speed
                200000000.0,
                3.0, // light decay speed
                true,
                velocity,
            );
        }
        super::vehicle::handle_vehicle_death(entity, world);
        true
    }
}

#[cfg(feature = "client")]
fn gather_spaceship_input(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    gamepads: Query<&Gamepad>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
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
    let gamepad = common::active_gamepad(gamepads.iter());
    let move_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.left_stick(), sensitivity.gamepad_move_deadzone)
        })
        .unwrap_or(Vec2::ZERO);
    let look_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.right_stick(), sensitivity.gamepad_look_deadzone)
        })
        .unwrap_or(Vec2::ZERO);

    let mut input = common::SpaceshipInput::default();
    if bindings.pressed(
        common::InputAction::MoveForward,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.forward += 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveBackward,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.forward -= 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveRight,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.right += 1.0;
    }
    if bindings.pressed(
        common::InputAction::MoveLeft,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.right -= 1.0;
    }
    input.forward = (input.forward + move_stick.y).clamp(-1.0, 1.0);
    input.right = (input.right + move_stick.x).clamp(-1.0, 1.0);
    if bindings.pressed(
        common::InputAction::Jump,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.up += 1.0;
    }
    if bindings.pressed(
        common::InputAction::Crouch,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.up -= 1.0;
    }
    if bindings.pressed(
        common::InputAction::RollLeft,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.roll -= 1.0;
    }
    if bindings.pressed(
        common::InputAction::RollRight,
        &keyboard,
        &mouse_buttons,
        gamepad,
    ) {
        input.roll += 1.0;
    }
    input.ability1 = bindings.pressed(
        common::InputAction::Ability1,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    let s = sensitivity.vehicle_pitch_yaw;
    input.yaw = -mouse.delta.x * s + look_stick.x * sensitivity.gamepad_look * time.delta_secs();
    input.pitch = -mouse.delta.y * s
        + look_stick.y
            * sensitivity.gamepad_look
            * time.delta_secs()
            * if sensitivity.gamepad_invert_y {
                -1.0
            } else {
                1.0
            };

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
