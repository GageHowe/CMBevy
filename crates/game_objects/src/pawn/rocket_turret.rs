#[cfg(feature = "client")]
use bevy::input::gamepad::Gamepad;
#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use net::quic::{Channel, QuicManager, SendTarget};
use physics::physics_world::*;
use rapier3d::prelude::*;

#[cfg(feature = "client")]
use super::{CameraEffector, GatherInputSet, MovePawnsSet, Possessed};
use super::{CharacterMount, Pawn, PawnInputKind, RocketTurretInput, mount};
use crate::{
    GameObject, GameObjectKind,
    collision::CollisionFxMaterial,
    health::{CollisionDamageConfig, Health, LastDamageSource},
    projectile::{
        Projectile,
        lobber::{LobberProjectile, SPEED as RPG_SPEED},
    },
    reticle::{AimOrigin, AimReticle},
    spawn::AppGameObjectExt,
};

#[cfg(feature = "client")]
const MODEL_PATH: &str = "models/kenney-prototypes/shape-cylinder.glb#Scene0";
const HALF_EXTENTS: Vec3 = Vec3::new(0.6, 0.6, 1.2);
const TURRET_MAX_HEALTH: f32 = 500.0;
#[cfg(feature = "client")]
const YAW_SPEED: f32 = 0.002;
const PITCH_MIN: f32 = -0.3;
const PITCH_MAX: f32 = 0.8;
const FIRE_COOLDOWN_TICKS: u16 = 45;
#[cfg(feature = "client")]
const CAMERA_OFFSET: Vec3 = Vec3::new(0.0, 0.8, 4.0);
const MOUNT_OFFSET: Vec3 = Vec3::new(0.0, 0.9, 0.0);
const PITCH_PIVOT_OFFSET: Vec3 = Vec3::new(0.0, 0.55, 0.2);
const MUZZLE_LENGTH: f32 = 2.6;

/// Plugin that registers the mounted rocket turret pawn and its runtime systems.
pub struct RocketTurretPlugin;
impl Plugin for RocketTurretPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<RocketTurretPawnComponent>()
            .add_systems(FixedUpdate, tick_rocket_turret_cooldowns)
            .add_systems(FixedUpdate, fire_queued_rocket_turrets.before(step_physics))
            .add_systems(Update, sync_pitch_pivots);
        #[cfg(feature = "client")]
        app.add_systems(
            FixedPreUpdate,
            (
                gather_rocket_turret_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(GatherInputSet),
                super::move_pawns::<RocketTurretPawnComponent>().in_set(MovePawnsSet),
            )
                .chain(),
        )
        .add_systems(Update, attach_camera_on_possess_turret);
    }
}

/// A possessable mounted turret pawn with its own look state, camera, and fire control.
#[derive(Component, Reflect)]
pub struct RocketTurretPawnComponent {
    /// Current local yaw relative to the parent body.
    pub yaw: f32,
    /// Current local pitch applied on the child pitch pivot.
    pub pitch: f32,
    /// Dirty flag used to replicate look state to remote clients.
    #[reflect(ignore)]
    pub look_sync_dirty: bool,
    /// Remaining fire cooldown in fixed ticks.
    pub cooldown_ticks: u16,
    /// Latched one-shot fire request consumed by the fire system.
    pub fire_queued: bool,
    /// Child pivot entity used to animate pitch separately from yaw.
    pub pitch_pivot: Option<Entity>,
}

impl Default for RocketTurretPawnComponent {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            look_sync_dirty: false,
            cooldown_ticks: 0,
            fire_queued: false,
            pitch_pivot: None,
        }
    }
}

impl Pawn for RocketTurretPawnComponent {
    fn apply_input(
        &mut self,
        _world: &mut PhysicsWorld,
        _body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::RocketTurret(input) = input {
            apply_rocket_turret_input(self, input);
        }
    }
}

impl GameObject for RocketTurretPawnComponent {
    const KIND: GameObjectKind = GameObjectKind::RocketTurret;
    const GC_AFTER_SECS: Option<f32> = Some(300.0);

    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let mount_anchor = mount::spawn_mount_anchor(entity, MOUNT_OFFSET, world);
        let pitch_pivot = world
            .spawn((
                Transform::from_translation(PITCH_PIVOT_OFFSET),
                Visibility::default(),
            ))
            .id();
        world.entity_mut(entity).add_child(pitch_pivot);
        world.entity_mut(entity).insert((
            RocketTurretPawnComponent {
                pitch_pivot: Some(pitch_pivot),
                ..default()
            },
            CharacterMount {
                occupant: None,
                anchor: mount_anchor,
                interact_radius: 1.4,
                exit_offset: Vec3::new(-1.5, 0.0, 0.0),
            },
            Health::new(TURRET_MAX_HEALTH),
            CollisionDamageConfig {
                threshold_per_mass: 120.0,
                min_threshold: 250.0,
                damage_scale: 0.4,
            },
            LastDamageSource::default(),
            CollisionFxMaterial::Sparks,
            AimOrigin(pitch_pivot),
            AimReticle("textures/crosshairs/crosshair001.png", Some(RPG_SPEED)),
            GameObjectKind::RocketTurret,
            Transform::from_translation(cmd.position),
            cmd.net_id.clone(),
        ));
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let rb = RigidBodyBuilder::kinematic_position_based()
                .translation(cmd.position)
                .build();
            let rb_handle = physics.insert_body(entity, rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(cmd.rotation, true);
            }
            let collider =
                ColliderBuilder::cuboid(HALF_EXTENTS.x, HALF_EXTENTS.y, HALF_EXTENTS.z).build();
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
        let biped_net_id = world
            .get::<CharacterMount>(entity)
            .and_then(|mount| mount.occupant)
            .and_then(|biped_entity| world.get::<net::message::NetworkID>(biped_entity).cloned());
        let Some(biped_entity) = mount::handle_mount_parent_death(entity, world) else {
            return true;
        };
        world.entity_mut(biped_entity).remove::<mount::Mounted>();
        #[cfg(feature = "client")]
        mount::clear_mount_possession(entity, biped_entity, world);
        let conn_id = world
            .get_resource::<super::PlayerRegistry>()
            .and_then(|registry| registry.conn_id_for_character(biped_entity));
        if let (Some(conn_id), Some(biped_net_id)) = (conn_id, biped_net_id) {
            if let Some(mut registry) = world.get_resource_mut::<super::PlayerRegistry>() {
                registry.set_controlled_pawn(conn_id, biped_entity, biped_net_id.clone());
            }
            if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
                quic.send(
                    SendTarget::One(conn_id),
                    Channel::Ordered,
                    &net::message::MsgType::Possess(biped_net_id.clone()),
                );
                super::broadcast_mount_state(&mut quic, &biped_net_id, None);
            }
        }
        true
    }
}

pub fn apply_rocket_turret_input(turret: &mut RocketTurretPawnComponent, input: RocketTurretInput) {
    if (turret.yaw - input.yaw).abs() > 0.0001 || (turret.pitch - input.pitch).abs() > 0.0001 {
        turret.look_sync_dirty = true;
    }
    turret.yaw += input.yaw;
    turret.pitch = (turret.pitch + input.pitch).clamp(PITCH_MIN, PITCH_MAX);
    if input.fire_pressed {
        turret.fire_queued = true;
    }
}

fn tick_rocket_turret_cooldowns(mut turrets: Query<&mut RocketTurretPawnComponent>) {
    for mut turret in &mut turrets {
        turret.cooldown_ticks = turret.cooldown_ticks.saturating_sub(1);
    }
}

fn sync_pitch_pivots(
    turrets: Query<&RocketTurretPawnComponent>,
    mut pivots: Query<&mut Transform>,
) {
    for turret in turrets.iter() {
        let Some(pitch_pivot) = turret.pitch_pivot else {
            continue;
        };
        let Ok(mut transform) = pivots.get_mut(pitch_pivot) else {
            continue;
        };
        transform.rotation = Quat::from_rotation_x(turret.pitch);
    }
}

fn fire_queued_rocket_turrets(
    #[cfg(feature = "client")] state: Res<State<common::game_state::GameState>>,
    tick: Res<common::tick::Ticker>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut net_ids: ResMut<net::message::NetworkIDResource>,
    mut quic: Option<ResMut<QuicManager>>,
    pitch_pivots: Query<&GlobalTransform>,
    mut turrets: Query<(Entity, &mut RocketTurretPawnComponent)>,
) {
    #[cfg(feature = "client")]
    if matches!(state.get(), common::game_state::GameState::Multiplayer) {
        return;
    }

    for (entity, mut turret) in &mut turrets {
        if !turret.fire_queued || turret.cooldown_ticks > 0 {
            turret.fire_queued = false;
            continue;
        }
        turret.fire_queued = false;
        turret.cooldown_ticks = FIRE_COOLDOWN_TICKS;
        let Some(pitch_pivot) = turret.pitch_pivot else {
            continue;
        };
        let Ok(pivot_gt) = pitch_pivots.get(pitch_pivot) else {
            continue;
        };
        let (_, rot, origin) = pivot_gt.to_scale_rotation_translation();
        let dir = rot * Vec3::NEG_Z;
        let fire_origin = origin + dir * MUZZLE_LENGTH;
        let Some(fired) = <LobberProjectile as Projectile>::fire_authoritative(
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
                SendTarget::All,
                Channel::Unordered,
                &net::message::MsgType::SpawnCommand(fired.spawn_cmd),
            );
        }
    }
}

#[cfg(feature = "client")]
fn gather_rocket_turret_input(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    gamepads: Query<&Gamepad>,
    sensitivity: Res<super::MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut turrets: Query<&mut Possessed, With<RocketTurretPawnComponent>>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    if cursor_q.grab_mode == CursorGrabMode::None {
        return;
    }
    let Ok(mut possessed) = turrets.single_mut() else {
        return;
    };
    let gamepad = common::active_gamepad(gamepads.iter());
    let look_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.right_stick(), sensitivity.gamepad_look_deadzone)
        })
        .unwrap_or(Vec2::ZERO);
    let mut input = RocketTurretInput::default();
    input.yaw =
        -mouse.delta.x * YAW_SPEED + look_stick.x * sensitivity.gamepad_look * time.delta_secs();
    input.pitch = -mouse.delta.y * YAW_SPEED
        + look_stick.y
            * sensitivity.gamepad_look
            * time.delta_secs()
            * if sensitivity.gamepad_invert_y {
                -1.0
            } else {
                1.0
            };
    input.fire = bindings.pressed(
        common::InputAction::Fire,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    input.fire_pressed = bindings.just_pressed(
        common::InputAction::Fire,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    possessed.push(PawnInputKind::RocketTurret(input));
}

#[cfg(feature = "client")]
fn attach_camera_on_possess_turret(
    turrets: Query<&RocketTurretPawnComponent, Added<Possessed>>,
    camera: Query<(Entity, &Projection), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok(turret) = turrets.single() else {
        return;
    };
    let Ok((cam, proj)) = camera.single() else {
        return;
    };
    let Some(pitch_pivot) = turret.pitch_pivot else {
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
            crate::spring_arm::SpringArm::new(CAMERA_OFFSET, 0.2, 5.0),
            crate::spring_arm::SpringArmPivot,
        ))
        .id();
    commands.entity(pitch_pivot).add_child(pivot);
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
