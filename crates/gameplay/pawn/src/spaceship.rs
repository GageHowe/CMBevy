#[cfg(feature = "client")]
use bevy::input::{gamepad::Gamepad, mouse::AccumulatedMouseMotion};
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::window::*;
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
#[cfg(feature = "client")]
use particles_plugin::prelude::spawn_spaceship_death_explosion_effect;
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::{
    Controller, MovePawnsSet,
    vehicle::{VehicleComponent, VehiclePawn, spawn_driver_mount},
};
#[cfg(feature = "client")]
use super::{GatherInputSet, MouseSensitivity};
#[cfg(feature = "client")]
use crate::flash::spawn_flash;
use crate::{
    collision::CollisionFxMaterial,
    generic::attach_hull_collider,
    health::{
        CollisionDamageConfig, Health, LastDamageSource, copy_last_damage_source, queue_damage,
    },
    interaction::InteractionName,
    reticle::AimReticle,
    shield::spawn_attached_spaceship_shield,
    weak_point::WeakPointOf,
};

const HULL_PATH: &str = "collision/placeholder_carrier.obj";
#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/placeholder_carrier.glb#Scene0";
const THRUST: f32 = 2000.0;
const ROLL_SPEED: f32 = 500.0;
const BASE_SENSITIVITY: f32 = 100000.0;
const MAX_TORQUE: f32 = 40000.0;
const SPACESHIP_MAX_HEALTH: i32 = 1500;
const SPACESHIP_WEAK_POINT_HEALTH: i32 = 200;
const SPACESHIP_WEAK_POINT_DAMAGE: f32 = 5000.0;
const SPACESHIP_WEAK_POINT_POS: Vec3 = Vec3::new(0.0, 0.0, -10.0);
const SPACESHIP_WEAK_POINT_RADIUS: f32 = 0.5;

pub struct SpaceshipPlugin;
impl Plugin for SpaceshipPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_spaceship_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                move_spaceships.in_set(MovePawnsSet),
            )
                .chain(),
        );
        #[cfg(not(feature = "client"))]
        app.add_systems(FixedPreUpdate, move_spaceships.in_set(MovePawnsSet));
    }
}

#[derive(Component, Default, Reflect)]
pub struct SpaceshipPawnComponent;

impl VehiclePawn for SpaceshipPawnComponent {
    const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 15.0, 30.0);
    const DRIVER_MOUNT_OFFSET: Vec3 = Vec3::new(0.0, 0.6, -2.0);
    const DRIVER_INTERACT_RADIUS: f32 = 0.8;

    fn apply_input(world: &mut PhysicsWorld, entity: Entity, input: common::PawnInput) {
        let Some(&handle) = world.entity_to_handle.get(&entity) else {
            return;
        };
        apply_spaceship_movement(world, &RigidBodyHandleComponent(handle), input);
    }
}

pub fn spawn_spaceship(entity: Entity, cmd: &crate::net::message::SpawnCommand, world: &mut World) {
    let position = cmd.position_or_zero();
    let rotation = cmd.rotation_or_identity();
    let velocity = cmd.velocity_or_zero();
    let angular_velocity = cmd.angular_velocity_or_zero();
    let transform = Transform {
        translation: position,
        rotation,
        ..default()
    };
    spawn_driver_mount::<SpaceshipPawnComponent>(entity, world);
    let rb_handle = {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let rb = RigidBodyBuilder::dynamic()
            .translation(transform.translation)
            .linvel(Vector3::new(velocity.x, velocity.y, velocity.z))
            .angular_damping(0.5)
            .build();
        let rb_handle = physics.insert_body(entity, rb);
        if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
            rb.set_rotation(transform.rotation, true);
            rb.set_angvel(
                Vector3::new(angular_velocity.x, angular_velocity.y, angular_velocity.z),
                true,
            );
        }
        rb_handle
    };
    world.entity_mut(entity).insert((
        crate::SpawnReplicated("spaceship"),
        SpaceshipPawnComponent,
        Health::new(
            SPACESHIP_MAX_HEALTH,
            20,
            common::config::FIXED_TICK_RATE as u16 * 5,
        )
        .with_death(on_spaceship_death),
        CollisionDamageConfig {
            threshold_per_mass: 120.0,
            min_threshold: 400.0,
            damage_scale: 0.5,
        },
        LastDamageSource::default(),
        VehicleComponent::for_vehicle::<SpaceshipPawnComponent>(),
        InteractionName("Spaceship"),
        CollisionFxMaterial::Sparks,
        AimReticle("textures/crosshairs/crosshair001.png", None),
        Transform::from(transform),
    ));
    attach_hull_collider(
        entity,
        rb_handle,
        HULL_PATH,
        1.0,
        ColliderBuilder::cuboid(1.5, 1.0, 3.0),
        world,
    );
    let weak_point = world.spawn_empty().id();
    let collider = ColliderBuilder::ball(SPACESHIP_WEAK_POINT_RADIUS)
        .translation(Vector3::new(
            SPACESHIP_WEAK_POINT_POS.x,
            SPACESHIP_WEAK_POINT_POS.y,
            SPACESHIP_WEAK_POINT_POS.z,
        ))
        .build();
    let collider = world
        .resource_mut::<PhysicsWorld>()
        .insert_collider_with_parent(weak_point, collider, rb_handle);
    world.entity_mut(weak_point).insert((
        WeakPointOf(entity),
        PhysicsColliderHandle(collider),
        Health::new(SPACESHIP_WEAK_POINT_HEALTH, 0, 0).with_death(on_spaceship_weak_point_death),
        Transform::from_translation(SPACESHIP_WEAK_POINT_POS),
        Visibility::default(),
        crate::DespawnOnDeath,
    ));
    world.entity_mut(entity).add_child(weak_point);
    #[cfg(feature = "client")]
    {
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Sphere::new(SPACESHIP_WEAK_POINT_RADIUS));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(Color::srgb(1.0, 0.05, 0.05));
        let visual = world
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        world.entity_mut(weak_point).add_child(visual);
    }
    world
        .entity_mut(entity)
        .insert(RigidBodyHandleComponent(rb_handle));
    let _ = spawn_attached_spaceship_shield(&cmd.net_id, world);
    #[cfg(feature = "client")]
    {
        let scene = world.resource::<AssetServer>().load(MODEL_PATH);
        world
            .entity_mut(entity)
            .insert((WorldAssetRoot(scene), Visibility::default()));
    }
    crate::insert_spawn_metadata(entity, world, Some(300.0), true, None, false);
}

fn on_spaceship_weak_point_death(entity: Entity, world: &mut World) {
    let Some(target) = world
        .get::<WeakPointOf>(entity)
        .map(|weak_point| weak_point.0)
    else {
        return;
    };
    copy_last_damage_source(world, entity, target);
    queue_damage(world, target, SPACESHIP_WEAK_POINT_DAMAGE);
}

pub fn on_spaceship_death(entity: Entity, world: &mut World) {
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
    pawns: Query<(), (With<Controller>, With<SpaceshipPawnComponent>)>,
    mut control: ResMut<common::LocalControl>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    if cursor_q.grab_mode == CursorGrabMode::None {
        return;
    }
    if pawns.single().is_err() {
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

    let mut input = common::PawnInput::default();
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

    control.push(input);
}

pub fn apply_spaceship_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: common::PawnInput,
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

fn move_spaceships(
    mut world: ResMut<PhysicsWorld>,
    mut pawns: Query<(&mut Controller, &RigidBodyHandleComponent), With<SpaceshipPawnComponent>>,
) {
    for (mut possessed, handle) in &mut pawns {
        let Some(input) = possessed.consume() else {
            continue;
        };
        apply_spaceship_movement(&mut world, handle, input);
    }
}
