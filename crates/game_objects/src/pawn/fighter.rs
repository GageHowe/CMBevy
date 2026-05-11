#[cfg(feature = "client")]
use bevy::input::{gamepad::Gamepad, mouse::AccumulatedMouseMotion};
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
    projectile::{Projectile, fighter_rocket::FighterRocketProjectile},
    reticle::AimReticle,
    spawn::AppGameObjectExt,
};

const HULL_PATH: &str = "collision/placeholder_carrier.obj";
#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/placeholder_carrier.glb#Scene0";
const MODEL_SCALE: f32 = 0.45;
const FORWARD_THRUST: f32 = 2800.0;
const REVERSE_THRUST: f32 = 900.0;
const STRAFE_THRUST: f32 = 800.0;
const VERTICAL_THRUST: f32 = 800.0;
const ROLL_SPEED: f32 = 500.0;
const BASE_SENSITIVITY: f32 = 100000.0;
const MAX_TORQUE: f32 = 40000.0;
const FIGHTER_MAX_HEALTH: f32 = 900.0;
const FIRE_COOLDOWN_TICKS: u16 = 10;
const MUZZLE_LENGTH: f32 = 2.0;

pub struct FighterPlugin;
impl Plugin for FighterPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<FighterPawnComponent>()
            .add_systems(FixedUpdate, tick_fighter_cooldowns)
            .add_systems(FixedUpdate, fire_fighters.after(step_physics));
        #[cfg(not(feature = "client"))]
        let _ = app;
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_fighter_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                move_pawns::<FighterPawnComponent>().in_set(MovePawnsSet),
            )
                .chain(),
        );
    }
}

#[derive(Component, Default, Reflect)]
pub struct FighterPawnComponent {
    pub cooldown_ticks: u16,
    #[reflect(ignore)]
    pub want_fire: bool,
}

impl Pawn for FighterPawnComponent {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::Spaceship(i) = input {
            self.want_fire = i.ability1;
            apply_fighter_movement(world, body, i);
        }
    }
}

impl VehiclePawn for FighterPawnComponent {
    const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 6.0, 15.0);
    const DRIVER_MOUNT_OFFSET: Vec3 = Vec3::new(0.0, 0.35, -0.9);
    const DRIVER_INTERACT_RADIUS: f32 = 0.6;
}

impl GameObject for FighterPawnComponent {
    const KIND: GameObjectKind = GameObjectKind::Fighter;
    const GC_AFTER_SECS: Option<f32> = Some(300.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        spawn_driver_mount::<FighterPawnComponent>(entity, world);
        world.entity_mut(entity).insert((
            FighterPawnComponent::default(),
            Health::new(FIGHTER_MAX_HEALTH),
            CollisionDamageConfig {
                threshold_per_mass: 120.0,
                min_threshold: 250.0,
                damage_scale: 0.5,
            },
            LastDamageSource::default(),
            VehicleComponent::for_vehicle::<FighterPawnComponent>(),
            CollisionFxMaterial::Sparks,
            AimReticle(
                "textures/crosshairs/crosshair001.png",
                Some(crate::projectile::fighter_rocket::SPEED),
            ),
            GameObjectKind::Fighter,
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
            MODEL_SCALE,
            ColliderBuilder::cuboid(0.7 * MODEL_SCALE, 0.45 * MODEL_SCALE, 1.4 * MODEL_SCALE),
            world,
        );
        world
            .entity_mut(entity)
            .insert(RigidBodyHandleComponent(rb_handle));
        #[cfg(feature = "client")]
        {
            let scene = world.resource::<AssetServer>().load(MODEL_PATH);
            let visual = world
                .spawn((
                    SceneRoot(scene),
                    Transform::from_scale(Vec3::splat(MODEL_SCALE)),
                    Visibility::default(),
                ))
                .id();
            world.entity_mut(entity).add_child(visual);
        }
    }

    fn on_death(entity: Entity, world: &mut World) -> bool {
        super::vehicle::handle_vehicle_death(entity, world);
        true
    }
}

#[cfg(feature = "client")]
fn gather_fighter_input(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    gamepads: Query<&Gamepad>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut pawns: Query<&mut Possessed, With<FighterPawnComponent>>,
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
        common::InputAction::Fire,
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

fn tick_fighter_cooldowns(mut fighters: Query<&mut FighterPawnComponent>) {
    for mut fighter in &mut fighters {
        fighter.cooldown_ticks = fighter.cooldown_ticks.saturating_sub(1);
    }
}

fn fire_fighters(
    #[cfg(feature = "client")] state: Res<State<common::game_state::GameState>>,
    tick: Res<common::tick::Ticker>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<net::message::NetworkIDResource>,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut fighters: Query<(Entity, &mut FighterPawnComponent, &RigidBodyHandleComponent)>,
) {
    #[cfg(feature = "client")]
    if matches!(state.get(), common::game_state::GameState::Multiplayer) {
        return;
    }

    for (entity, mut fighter, body_handle) in &mut fighters {
        if !fighter.want_fire || fighter.cooldown_ticks > 0 {
            continue;
        }
        fighter.cooldown_ticks = FIRE_COOLDOWN_TICKS;
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        let dir = rb_rot(rb) * Vec3::NEG_Z;
        let fire_origin = rb_pos(rb) + dir * MUZZLE_LENGTH;
        let Some(fired) = <FighterRocketProjectile as Projectile>::fire_authoritative(
            fire_origin,
            dir,
            entity,
            tick.tick,
            entity,
            0,
            &mut commands,
            &mut world,
            &mut net_ids,
        ) else {
            continue;
        };
        if let Some(quic) = quic.as_deref_mut() {
            quic.send(
                net::quic::SendTarget::All,
                net::quic::Channel::Unordered,
                &net::message::MsgType::SpawnCommand(fired.spawn_cmd),
            );
        }
    }
}

pub fn apply_fighter_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::SpaceshipInput,
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

    let forward_thrust = if input.forward >= 0.0 {
        FORWARD_THRUST
    } else {
        REVERSE_THRUST
    };
    let impulse = local_forward * input.forward * forward_thrust
        + local_right * input.right * STRAFE_THRUST
        + local_up * input.up * VERTICAL_THRUST;
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
