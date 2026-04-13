#[cfg(feature = "client")]
use super::vehicle::{DriverSeat, VehicleComponent, enter_vehicle, ray_hits_cockpit};
use super::*;
#[cfg(feature = "client")]
use crate::weapon::{WeaponDriver, WeaponFireInput};
use crate::{
    GameObject, GameObjectKind,
    health::{Health, LastDamageSource},
};
#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseMotion;
#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use net::message::NetworkID;
use physics::physics_world::*;
use rapier3d::prelude::*;

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

#[cfg(feature = "client")]
use super::CameraEffector;
pub const CAPSULE_RADIUS: f32 = 0.3;
pub const CAPSULE_HALF_HEIGHT: f32 = 0.5;
const SLIDE_HALF_HEIGHT: f32 = 0.1;
const CAPSULE_BOTTOM: f32 = CAPSULE_HALF_HEIGHT + CAPSULE_RADIUS;
/// max speed gained per tick when accelerating on the ground
const GROUND_ACCEL: f32 = 0.7;
const SPRINT_ACCEL: f32 = 1.0;
const JUMP_IMPULSE: f32 = 8.0;
const AIR_CONTROL: f32 = 0.2;
const AIR_UP_CONTROL: f32 = 0.45;
const GROUND_DIST: f32 = 0.05; // must be nearly touching to count as grounded
const JUMP_COOLDOWN: u8 = 20; // ticks before another jump
const MAIN_RESTITUTION: f32 = 0.0;
const MAIN_FRICTION: f32 = 3.0;
const SLIDE_FRICTION: f32 = 0.1;
#[cfg(feature = "client")]
const FLASHLIGHT_INTENSITY: f32 = 1000000.0;
#[cfg(feature = "client")]
const FLASHLIGHT_RANGE: f32 = 20000000.0;
#[cfg(feature = "client")]
const FLASHLIGHT_OUTER_ANGLE: f32 = 0.08;
#[cfg(feature = "client")]
const FLASHLIGHT_INNER_ANGLE: f32 = 0.01;

#[derive(Component, Default, Reflect)]
pub struct BipedPawnComponent {
    pub flashlight_on: bool,
    /// ticks remaining before another jump is allowed.
    pub jump_cooldown: u8,
    /// Look yaw/pitch (radians). Set from input; used for server-side movement simulation.
    pub look_yaw: f32,
    pub look_pitch: f32,
    /// Ccached pivot entities set by setup_camera_rig; None on the server.
    pub yaw_pivot: Option<Entity>,
    pub pitch_pivot: Option<Entity>,
    #[reflect(ignore)]
    pub flashlight: Option<Entity>,
    #[reflect(ignore)]
    pub collider: Option<ColliderHandle>,
    pub is_sliding: bool,
    /// When crouched in the air, keep the camera/head fixed and lift the feet instead.
    pub slide_feet_planted: bool,
    pub snap_target: Option<Entity>,
    /// Client-only cached body rotation used to preserve world look across body rotation.
    pub last_look_frame_body_rot: Option<Quat>,
}

impl Pawn for BipedPawnComponent {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    ) {
        if let PawnInputKind::Biped(i) = input {
            apply_biped_movement(world, body, i, self);
        }
    }
}
impl GameObject for BipedPawnComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform {
            translation: cmd.position.into(),
            rotation: cmd.rotation.into(),
            ..default()
        };
        world.entity_mut(entity).insert((
            WeaponSlots::default(),
            Health::new(100.0),
            LastDamageSource::default(),
            GameObjectKind::Biped,
            Transform::from(transform),
            BipedPawnComponent::default(),
            cmd.net_id.clone(),
        ));

        // physics
        let (rb_handle, collider_handle) = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let capsule_rb = RigidBodyBuilder::dynamic()
                .translation(transform.translation)
                .linvel(Vector3::new(
                    cmd.starting_velocity.x,
                    cmd.starting_velocity.y,
                    cmd.starting_velocity.z,
                ))
                // .angular_damping(5.0)
                .lock_rotations()
                // .ccd_enabled(true) was causing issues with relative velocity
                .build();
            let rb_handle = physics.insert_body(entity, capsule_rb);
            if let Some(rb) = physics.rigid_body_set.get_mut(rb_handle) {
                rb.set_rotation(transform.rotation, true);
            }
            let capsule_collider =
                make_biped_capsule_collider(CAPSULE_HALF_HEIGHT, MAIN_FRICTION, true);
            let PhysicsWorld {
                collider_set,
                rigid_body_set,
                ..
            } = &mut *physics;
            let collider_handle =
                collider_set.insert_with_parent(capsule_collider, rb_handle, rigid_body_set);
            (rb_handle, collider_handle)
        };
        {
            let mut entity = world.entity_mut(entity);
            entity.insert(RigidBodyHandleComponent(rb_handle));
            if let Some(mut biped) = entity.get_mut::<BipedPawnComponent>() {
                biped.collider = Some(collider_handle);
            }
        }
        #[cfg(feature = "client")]
        {
            // placeholder — matches physics capsule dimensions exactly; swap for a real model later
            let mesh =
                world
                    .resource_mut::<Assets<Mesh>>()
                    .add(bevy::math::primitives::Capsule3d::new(
                        CAPSULE_RADIUS,
                        CAPSULE_HALF_HEIGHT,
                    ));
            let material = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(Color::srgb(0.9, 0.4, 0.1));
            world.entity_mut(entity).insert((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Visibility::default(),
            ));
            let light = world
                .spawn((
                    SpotLight {
                        intensity: FLASHLIGHT_INTENSITY,
                        range: FLASHLIGHT_RANGE,
                        outer_angle: FLASHLIGHT_OUTER_ANGLE,
                        inner_angle: FLASHLIGHT_INNER_ANGLE,
                        shadows_enabled: true,
                        ..default()
                    },
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            let pitch_pivot = world
                .spawn((
                    PitchPivot { pitch: 0.0 },
                    Transform::default(),
                    Visibility::default(),
                ))
                .id();
            world.entity_mut(pitch_pivot).add_child(light);
            let yaw_pivot = world
                .spawn((
                    YawPivot { yaw: 0.0 },
                    Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)),
                    Visibility::default(),
                ))
                .id();
            world.entity_mut(yaw_pivot).add_child(pitch_pivot);
            world.entity_mut(entity).add_child(yaw_pivot);
            // cache pivot entities so input and camera logic can find them
            if let Some(mut biped) = world.entity_mut(entity).get_mut::<BipedPawnComponent>() {
                biped.yaw_pivot = Some(yaw_pivot);
                biped.pitch_pivot = Some(pitch_pivot);
                biped.flashlight = Some(light);
            }
        }
    }

    fn on_death(entity: Entity, world: &mut World) -> bool {
        #[cfg(feature = "client")]
        if world.get::<Possessed>(entity).is_some() {
            let mut camera_q = world.query_filtered::<Entity, With<Camera3d>>();
            let camera = camera_q.single(world).ok();
            if let Some(camera) = camera {
                if let Ok(mut entity) = world.get_entity_mut(camera) {
                    entity.remove_parent_in_place();
                }
            }
        }
        let drop_pos = {
            let physics = world.resource::<PhysicsWorld>();
            physics
                .entity_to_handle
                .get(&entity)
                .and_then(|&h| physics.rigid_body_set.get(h))
                .map(rb_pos)
                .unwrap_or(Vec3::ZERO)
        };
        let held: Vec<Entity> = world
            .get::<WeaponSlots>(entity)
            .map(|slots| {
                [slots.primary.1, slots.pocket.1]
                    .into_iter()
                    .flatten()
                    .collect()
            })
            .unwrap_or_default();
        let mut physics = world.resource_mut::<PhysicsWorld>();
        for weapon_entity in held {
            crate::weapon::helpers::place_world_weapon(
                &mut physics,
                weapon_entity,
                drop_pos,
                Vec3::ZERO,
            );
        }
        true
    }
}

pub struct BipedPlugin;
impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MouseSensitivity>();
        app.add_systems(FixedUpdate, update_slide_camera);
        #[cfg(feature = "client")]
        {
            app.init_resource::<ReloadGate>()
                .add_systems(Update, queue_reload_input)
                .add_systems(
                    FixedPreUpdate,
                    (
                        gather_biped_input
                            .run_if(resource_exists::<ButtonInput<KeyCode>>)
                            .in_set(GatherInputSet),
                        move_pawns::<BipedPawnComponent>().in_set(MovePawnsSet),
                        biped_fire.run_if(resource_exists::<ButtonInput<MouseButton>>),
                        toggle_flashlight.run_if(resource_exists::<ButtonInput<KeyCode>>),
                        drop_active_weapon.run_if(resource_exists::<ButtonInput<KeyCode>>),
                        interact
                            .run_if(
                                in_state(common::game_state::GameState::SinglePlayer)
                                    .or(in_state(common::game_state::GameState::Multiplayer)),
                            )
                            .run_if(resource_exists::<ButtonInput<KeyCode>>),
                    )
                        .chain(),
                );
        }
        app.add_systems(
            PostUpdate,
            preserve_look_across_body_rotation.before(TransformSystems::Propagate),
        );
        #[cfg(feature = "client")]
        {
            app.add_systems(
                PostUpdate,
                (
                    mouse_look.run_if(resource_exists::<AccumulatedMouseMotion>),
                    apply_camera_effects,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            );
            // re-parent camera under pitch pivot when a biped is possessed
            app.add_systems(
                Update,
                (
                    attach_camera_on_possess, // Added<Possession>
                    hide_weapons_while_seated,
                    switch_weapon_slot.run_if(resource_exists::<AccumulatedMouseScroll>),
                ),
            );
        }
    }
}

/// rotates around the pawn's local yaw. child of the pawn entity.
#[derive(Component)]
pub struct YawPivot {
    pub yaw: f32,
}
/// rotates around its local pitch. child of YawPivot.
#[derive(Component)]
pub struct PitchPivot {
    pub pitch: f32,
}

/// Gathers keyboard + look-pivot state into a BipedInput each FixedPreUpdate.
/// look_yaw/pitch are 1-frame stale (mouse_look runs in Update) — acceptable for movement.
#[cfg(feature = "client")]
fn gather_biped_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    mut pawns: Query<(&mut Possessed, &BipedPawnComponent)>,
    yaw_pivots: Query<&YawPivot>,
    pitch_pivots: Query<&PitchPivot>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    let Ok((mut possessed, biped)) = pawns.single_mut() else {
        return;
    };

    let mut input = BipedInput::default();
    if bindings.pressed(common::InputAction::MoveForward, &keyboard, &mouse_buttons) {
        input.forward += 1.0;
    }
    if bindings.pressed(common::InputAction::MoveBackward, &keyboard, &mouse_buttons) {
        input.forward -= 1.0;
    }
    if bindings.pressed(common::InputAction::MoveRight, &keyboard, &mouse_buttons) {
        input.right += 1.0;
    }
    if bindings.pressed(common::InputAction::MoveLeft, &keyboard, &mouse_buttons) {
        input.right -= 1.0;
    }
    input.jump = bindings.pressed(common::InputAction::Jump, &keyboard, &mouse_buttons);
    input.slide = bindings.pressed(common::InputAction::Crouch, &keyboard, &mouse_buttons);
    input.ability1 = bindings.pressed(common::InputAction::Sprint, &keyboard, &mouse_buttons);
    input.ability2 = bindings.pressed(common::InputAction::Ability2, &keyboard, &mouse_buttons);

    if let Some(yaw_e) = biped.yaw_pivot {
        if let Ok(yp) = yaw_pivots.get(yaw_e) {
            input.look_yaw = yp.yaw;
        }
    }
    if let Some(pitch_e) = biped.pitch_pivot {
        if let Ok(pp) = pitch_pivots.get(pitch_e) {
            input.look_pitch = pp.pitch;
        }
    }

    possessed.push(PawnInputKind::Biped(input));
}

/// moves the biped's yaw and pitch components on Update
fn preserve_look_across_body_rotation(
    snap_comp: Option<Res<super::LookSnapCompensation>>,
    mut possessed: Query<(&RigidBodyHandleComponent, &mut BipedPawnComponent), With<Possessed>>,
    mut pivots: ParamSet<(
        Query<&Transform, (With<Possessed>, Without<YawPivot>, Without<PitchPivot>)>,
        Query<(&mut Transform, &mut YawPivot), Without<Possessed>>,
        Query<(&mut Transform, &mut PitchPivot), Without<Possessed>>,
    )>,
) {
    let Ok((_body_handle, mut biped)) = possessed.single_mut() else {
        return;
    };
    let body_rot;
    {
        let body_transforms = pivots.p0();
        let Ok(body_transform) = body_transforms.single() else {
            return;
        };
        body_rot = body_transform.rotation;
    }
    if !snap_comp.map_or(true, |enabled| enabled.0) {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    }

    let Some(prev_body_rot) = biped.last_look_frame_body_rot else {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    };
    if body_rot.dot(prev_body_rot).abs() > 0.999_999 {
        return;
    }

    let (Some(yaw_e), Some(pitch_e)) = (biped.yaw_pivot, biped.pitch_pivot) else {
        biped.last_look_frame_body_rot = Some(body_rot);
        return;
    };
    let yaw = {
        let mut yaw_query = pivots.p1();
        let Ok((_, yaw_pivot)) = yaw_query.get_mut(yaw_e) else {
            biped.last_look_frame_body_rot = Some(body_rot);
            return;
        };
        yaw_pivot.yaw
    };
    let pitch = {
        let mut pitch_query = pivots.p2();
        let Ok((_, pitch_pivot)) = pitch_query.get_mut(pitch_e) else {
            biped.last_look_frame_body_rot = Some(body_rot);
            return;
        };
        pitch_pivot.pitch
    };
    let world_forward =
        prev_body_rot * Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch) * Vec3::NEG_Z;
    let local_forward = (body_rot.inverse() * world_forward).normalize_or_zero();
    if local_forward != Vec3::ZERO {
        let yaw = f32::atan2(-local_forward.x, -local_forward.z);
        let pitch = local_forward
            .y
            .clamp(-1.0, 1.0)
            .asin()
            .clamp(-PITCH_MAX, PITCH_MAX);
        {
            let mut yaw_query = pivots.p1();
            if let Ok((mut yaw_t, mut yaw_pivot)) = yaw_query.get_mut(yaw_e) {
                yaw_pivot.yaw = yaw;
                yaw_t.rotation = Quat::from_rotation_y(yaw);
            }
        }
        {
            let mut pitch_query = pivots.p2();
            if let Ok((mut pitch_t, mut pitch_pivot)) = pitch_query.get_mut(pitch_e) {
                pitch_pivot.pitch = pitch;
                pitch_t.rotation = Quat::from_rotation_x(pitch);
            }
        }
    }
    biped.last_look_frame_body_rot = Some(body_rot);
}

/// moves the biped's yaw and pitch components on Update
#[cfg(feature = "client")]
fn mouse_look(
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    possessed: Query<&BipedPawnComponent, With<Possessed>>,
    camera_fx: Query<&CameraEffector, With<Camera3d>>,
    mut pivots: ParamSet<(
        Query<(&mut Transform, &mut YawPivot)>,
        Query<(&mut Transform, &mut PitchPivot)>,
    )>,
) {
    if cursor_q.grab_mode == CursorGrabMode::None {
        return;
    }
    let delta = mouse.delta;
    if delta == Vec2::ZERO {
        return;
    }
    let Ok(biped) = possessed.single() else {
        return;
    };
    // Scale look sensitivity with zoom so scoped weapons stay usable without
    // needing per-weapon sensitivity code.
    let zoom = camera_fx
        .single()
        .map(|fx| fx.zoom_multiplier.max(1.0))
        .unwrap_or(1.0);
    let zoom_scale = 1.0 + (1.0 / zoom - 1.0) * sensitivity.zoom_blend;
    let s = sensitivity.base * zoom_scale;

    if let Some(yaw_e) = biped.yaw_pivot {
        if let Ok((mut t, mut pivot)) = pivots.p0().get_mut(yaw_e) {
            pivot.yaw -= delta.x * s;
            t.rotation = Quat::from_rotation_y(pivot.yaw);
        }
    }
    if let Some(pitch_e) = biped.pitch_pivot {
        if let Ok((mut t, mut pivot)) = pivots.p1().get_mut(pitch_e) {
            pivot.pitch = (pivot.pitch - delta.y * s).clamp(-PITCH_MAX, PITCH_MAX);
            t.rotation = Quat::from_rotation_x(pivot.pitch);
        }
    }
}

#[cfg(feature = "client")]
const KICK_DAMPING: f32 = 0.88; // velocity multiplier per tick at 60 Hz
#[cfg(feature = "client")]
const SHAKE_DECAY: f32 = 6.0; // intensity units per second
#[cfg(feature = "client")]
const FOV_LERP_SPEED: f32 = 15.0; // how fast zoom eases in/out

/// Integrates recoil, shake, and FOV zoom. Writes Camera3d local Transform and Projection.
#[cfg(feature = "client")]
fn apply_camera_effects(
    time: Res<Time>,
    mut camera_q: Query<(&mut Transform, &mut CameraEffector, &mut Projection), With<Camera3d>>,
) {
    let Ok((mut transform, mut fx, mut proj)) = camera_q.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    // velocity contributes to offset, then both decay — no spring force so no overshoot
    let damp = KICK_DAMPING.powf(dt * 60.0);
    let decay = (-fx.recovery_speed * dt).exp();
    fx.pitch_vel *= damp;
    fx.pitch_offset = (fx.pitch_offset + fx.pitch_vel * dt) * decay;
    fx.yaw_vel *= damp;
    fx.yaw_offset = (fx.yaw_offset + fx.yaw_vel * dt) * decay;

    // shake decays over time; harmonics approximate random without pulling in rand
    fx.shake = (fx.shake - SHAKE_DECAY * dt).max(0.0);
    let t = time.elapsed_secs();
    let (sp, sy) = if fx.shake > 0.001 {
        let s = fx.shake * 0.015;
        (s * (t * 53.1).sin(), s * (t * 37.7).cos())
    } else {
        (0.0, 0.0)
    };

    transform.rotation =
        Quat::from_euler(EulerRot::XYZ, fx.pitch_offset + sp, fx.yaw_offset + sy, 0.0);

    // FOV zoom: target = 2 * atan(tan(base/2) / multiplier) — correct optics
    let target_fov = ((fx.base_fov / 2.0).to_radians().tan() / fx.zoom_multiplier)
        .atan()
        .to_degrees()
        * 2.0;
    fx.current_fov += (target_fov - fx.current_fov) * (1.0 - (-FOV_LERP_SPEED * dt).exp());
    if let Projection::Perspective(ref mut p) = *proj {
        p.fov = fx.current_fov.to_radians();
    }
}

#[cfg(feature = "client")]
fn switch_weapon_slot(
    scroll: Res<AccumulatedMouseScroll>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawn: Query<&mut WeaponSlots, With<Possessed>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut visibility: Query<&mut Visibility>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
    state: Res<State<common::game_state::GameState>>,
    mut quic: ResMut<net::quic::QuicManager>,
) {
    if scroll.delta.y == 0.0 || egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    let Ok(mut slots) = pawn.single_mut() else {
        return;
    };
    let old_active_primary = slots.active_primary;
    if let Some(e) = slots.active().1 {
        crate::weapon::helpers::clear_inactive_slot_reload(weapon_states.get_mut(e).ok());
        if let Ok(mut vis) = visibility.get_mut(e) {
            *vis = Visibility::Hidden;
        }
    }
    slots.active_primary = !slots.active_primary;
    if let Some(e) = slots.active().1 {
        if let Ok(mut vis) = visibility.get_mut(e) {
            *vis = Visibility::Inherited;
        }
    }
    if let Ok(mut cc) = camera.single_mut() {
        cc.zoom_multiplier = 1.0;
    }
    if matches!(state.get(), common::game_state::GameState::Multiplayer)
        && old_active_primary != slots.active_primary
    {
        quic.send(
            net::quic::SendTarget::All,
            net::quic::Channel::Ordered,
            &net::message::MsgType::SetActiveWeaponSlot(slots.active_primary),
        );
    }
}

/// Two weapon slots on a biped pawn. Each slot: (NetworkID, client-only viewmodel Entity).
/// primary = right-hand slot, pocket = left-hand slot.
#[derive(Component)]
pub struct WeaponSlots {
    pub primary: (Option<NetworkID>, Option<Entity>),
    pub pocket: (Option<NetworkID>, Option<Entity>),
    /// true = primary active, false = pocket active.
    pub active_primary: bool,
}
impl Default for WeaponSlots {
    fn default() -> Self {
        Self {
            primary: (None, None),
            pocket: (None, None),
            active_primary: true,
        }
    }
}
impl WeaponSlots {
    pub fn active(&self) -> &(Option<NetworkID>, Option<Entity>) {
        if self.active_primary {
            &self.primary
        } else {
            &self.pocket
        }
    }
    pub fn active_mut(&mut self) -> &mut (Option<NetworkID>, Option<Entity>) {
        if self.active_primary {
            &mut self.primary
        } else {
            &mut self.pocket
        }
    }
    pub fn is_full(&self) -> bool {
        self.primary.0.is_some() && self.pocket.0.is_some()
    }
    /// Clears whichever slot holds this id (both NetworkID and Entity).
    pub fn remove_by_net_id(&mut self, id: &NetworkID) {
        if self.primary.0.as_ref() == Some(id) {
            self.primary = (None, None);
        }
        if self.pocket.0.as_ref() == Some(id) {
            self.pocket = (None, None);
        }
    }
}

#[cfg(feature = "client")]
pub fn draw_biped_debug(
    world: Res<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponent, With<BipedPawnComponent>>,
    mut gizmos: Gizmos,
) {
    use physics::debug::{draw_collider, rb_iso};
    for body_handle in bipeds.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else {
            continue;
        };
        let iso = rb_iso(rb);
        for ch in rb.colliders() {
            if let Some(col) = world.collider_set.get(*ch) {
                draw_collider(col, iso, Color::srgba(0.3, 0.6, 1.0, 0.1), &mut gizmos);
            }
        }
    }
}

/// Adjusts the YawPivot Y position to match the current slide state.
/// No-op on the server (yaw_pivot is None).
fn update_slide_camera(
    world: Res<PhysicsWorld>,
    bipeds: Query<(&BipedPawnComponent, &RigidBodyHandleComponent)>,
    mut pivots: Query<&mut Transform, With<YawPivot>>,
) {
    for (biped, body) in bipeds.iter() {
        let Some(yaw_e) = biped.yaw_pivot else {
            continue;
        };
        let Ok(mut t) = pivots.get_mut(yaw_e) else {
            continue;
        };
        let Some(rb) = world.rigid_body_set.get(body.0) else {
            continue;
        };
        let planet_up = rb_rot(rb) * Vec3::Y;
        let grounded = ground_state(&world, body.0, rb_pos(rb), planet_up).0;
        t.translation.y = if biped.is_sliding && grounded {
            -0.1
        } else {
            0.4
        };
    }
}

fn ground_state(
    world: &PhysicsWorld,
    body_handle: RigidBodyHandle,
    capsule_pos: Vec3,
    planet_up: Vec3,
) -> (bool, Vec3, Option<Entity>) {
    let ray_origin = capsule_pos - planet_up * CAPSULE_BOTTOM;
    let exclude = |_ch: ColliderHandle, col: &rapier3d::prelude::Collider| {
        col.parent().map_or(true, |rb| rb != body_handle)
    };
    let filter = QueryFilter::new().predicate(&exclude);
    let qp = world.broad_phase.as_query_pipeline(
        world.narrow_phase.query_dispatcher(),
        &world.rigid_body_set,
        &world.collider_set,
        filter,
    );
    let ray = Ray::new(ray_origin, -planet_up);
    if let Some((ch, _)) = qp.cast_ray(&ray, GROUND_DIST, true) {
        let support_body = world.collider_set.get(ch).and_then(|col| col.parent());
        let vel = support_body
            .and_then(|rb_h| world.rigid_body_set.get(rb_h))
            .map(rb_vel)
            .unwrap_or(Vec3::ZERO);
        let support_entity =
            support_body.and_then(|rb_h| world.handle_to_entity.get(&rb_h).copied());
        (true, vel, support_entity)
    } else {
        (false, Vec3::ZERO, None)
    }
}

fn make_biped_capsule_collider(half_height: f32, friction: f32, feet_planted: bool) -> Collider {
    let player_collision =
        InteractionGroups::new(GROUP_PLAYER, Group::ALL, InteractionTestMode::And);
    let player_solver = InteractionGroups::new(
        GROUP_PLAYER,
        Group::ALL & !GROUP_PROJECTILE,
        InteractionTestMode::And,
    );
    // Grounded crouch keeps feet planted. Airborne crouch keeps the head/camera fixed instead.
    let y_offset = if feet_planted {
        (half_height + CAPSULE_RADIUS) - CAPSULE_BOTTOM
    } else {
        CAPSULE_HALF_HEIGHT - half_height
    };
    ColliderBuilder::capsule_y(half_height, CAPSULE_RADIUS)
        .translation(Vector3::new(0.0, y_offset, 0.0))
        .friction(friction)
        .friction_combine_rule(CoefficientCombineRule::Min)
        .restitution(MAIN_RESTITUTION)
        .restitution_combine_rule(CoefficientCombineRule::Min)
        .collision_groups(player_collision)
        .solver_groups(player_solver)
        .build()
}

/// Replaces the capsule collider on a biped rigid body.
fn replace_capsule_collider(
    world: &mut PhysicsWorld,
    rb_handle: RigidBodyHandle,
    old_ch: Option<ColliderHandle>,
    half_height: f32,
    friction: f32,
    feet_planted: bool,
) -> ColliderHandle {
    if let Some(old_ch) = old_ch {
        let PhysicsWorld {
            collider_set,
            island_manager,
            rigid_body_set,
            ..
        } = &mut *world;
        collider_set.remove(old_ch, island_manager, rigid_body_set, false);
    }
    let PhysicsWorld {
        collider_set,
        rigid_body_set,
        ..
    } = &mut *world;
    collider_set.insert_with_parent(
        make_biped_capsule_collider(half_height, friction, feet_planted),
        rb_handle,
        rigid_body_set,
    )
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: BipedInput,
    biped: &mut BipedPawnComponent,
) {
    let (body_rot, capsule_pos, capsule_linvel, capsule_mass) = {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else {
            return;
        };
        if !body.is_enabled() {
            return;
        }
        (rb_rot(body), rb_pos(body), rb_vel(body), body.mass())
    };

    let planet_up = body_rot * Vec3::Y;
    let facing = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward = facing * Vec3::NEG_Z;
    let right = facing * Vec3::X;

    let is_jump = input.jump;
    let is_slide = input.slide;
    let is_sprint = input.ability1;

    // body center is always CAPSULE_BOTTOM above foot level; cast ray from foot position
    let (grounded, ground_linvel, support_entity) =
        ground_state(world, body_handle.0, capsule_pos, planet_up);

    // Air crouch should not drag the camera down with it. Swap the capsule anchor whenever
    // crouch state changes or when a crouched biped transitions between ground and air.
    let slide_feet_planted = grounded;
    if is_slide != biped.is_sliding || (is_slide && slide_feet_planted != biped.slide_feet_planted)
    {
        biped.is_sliding = is_slide;
        biped.slide_feet_planted = slide_feet_planted;
        let (half_height, friction, feet_planted) = if is_slide {
            (SLIDE_HALF_HEIGHT, SLIDE_FRICTION, slide_feet_planted)
        } else {
            (CAPSULE_HALF_HEIGHT, MAIN_FRICTION, true)
        };
        biped.collider = Some(replace_capsule_collider(
            world,
            body_handle.0,
            biped.collider,
            half_height,
            friction,
            feet_planted,
        ));
    }

    biped.jump_cooldown = biped.jump_cooldown.saturating_sub(1);

    let _ = (capsule_linvel, ground_linvel);

    if grounded && !is_slide {
        let desired = (forward * input.forward + right * input.right).normalize_or_zero();
        if desired.length_squared() > 1e-6 {
            let accel = if is_sprint && input.forward >= 0.0 {
                SPRINT_ACCEL
            } else {
                GROUND_ACCEL
            };
            let impulse = desired * accel * capsule_mass;
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }

    if is_jump && grounded && biped.jump_cooldown == 0 {
        biped.jump_cooldown = JUMP_COOLDOWN;
        let impulse = planet_up * JUMP_IMPULSE * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
        if let Some(support_entity) = support_entity
            && let Some(&support_handle) = world.entity_to_handle.get(&support_entity)
            && let Some(rb) = world.rigid_body_set.get_mut(support_handle)
            && rb.is_dynamic()
        {
            rb.apply_impulse(Vector::new(-impulse.x, -impulse.y, -impulse.z), true);
        }
    }

    if !grounded {
        let up = input.jump as i8 as f32 - input.slide as i8 as f32;
        let lateral = (forward * input.forward + right * input.right).normalize_or_zero();
        let vertical_control = if up > 0.0 {
            AIR_UP_CONTROL
        } else {
            AIR_CONTROL
        };
        let impulse = (lateral * AIR_CONTROL + planet_up * up * vertical_control) * capsule_mass;
        if impulse.length_squared() > 1e-6 {
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }
}

/// Re-parents the Camera3d under the biped's pitch pivot when Possessed is added.
/// Runs in Update so commands from OnEnter/FixedPostUpdate have already flushed.
#[cfg(feature = "client")]
fn attach_camera_on_possess(
    bipeds: Query<(&BipedPawnComponent, &WeaponSlots), Added<Possessed>>,
    camera: Query<(Entity, &Projection), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok((biped, slots)) = bipeds.single() else {
        return;
    };
    let Ok((cam, proj)) = camera.single() else {
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        return;
    };
    // read current projection FOV so CameraEffects starts in sync with settings
    let base_fov = if let Projection::Perspective(p) = proj {
        p.fov.to_degrees()
    } else {
        90.0
    };
    commands.entity(cam).insert((
        Transform::default(),
        CameraEffector {
            base_fov,
            current_fov: base_fov,
            ..default()
        },
    ));
    commands.entity(pitch_e).add_child(cam);
    set_weapon_slot_visibility(&mut commands, slots);
}

#[cfg(feature = "client")]
fn hide_weapons_while_seated(
    seated: Query<&WeaponSlots, Added<super::SeatedInVehicle>>,
    mut commands: Commands,
) {
    for slots in seated.iter() {
        for weapon in [slots.primary.1, slots.pocket.1].into_iter().flatten() {
            commands.entity(weapon).insert(Visibility::Hidden);
        }
    }
}

#[cfg(feature = "client")]
fn set_weapon_slot_visibility(commands: &mut Commands, slots: &WeaponSlots) {
    if let Some(weapon) = slots.primary.1 {
        commands.entity(weapon).insert(if slots.active_primary {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    if let Some(weapon) = slots.pocket.1 {
        commands.entity(weapon).insert(if slots.active_primary {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// Y key toggles the local player's flashlight. Sends FlashlightToggle to server when connected.
#[cfg(feature = "client")]
fn toggle_flashlight(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Res<EguiWantsInput>,
    bindings: Res<common::ActiveKeyBindings>,
    possessed_q: Query<&BipedPawnComponent, With<Possessed>>,
    mut lights: Query<&mut Visibility, With<SpotLight>>,
    mut quic: ResMut<net::quic::QuicManager>,
    mut on: Local<bool>,
    mut toggle_pressed: Local<bool>,
) {
    if egui_wants.wants_any_input()
        || !consume_fixed_press(
            bindings.pressed(common::InputAction::ToggleFlashlight, &keyboard, &mouse),
            &mut toggle_pressed,
        )
    {
        return;
    }
    *on = !*on;
    if let Ok(biped) = possessed_q.single() {
        if let Some(light) = biped.flashlight {
            if let Ok(mut vis) = lights.get_mut(light) {
                *vis = if *on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
    // no-op in singleplayer (client_connected is false)
    if quic.client_connected {
        quic.send(
            net::quic::SendTarget::All,
            net::quic::Channel::Ordered,
            &net::message::MsgType::FlashlightToggle,
        );
    }
}

/// Forwards input to the possessed biped's active weapon each FixedPreUpdate tick.
/// All fire logic (projectiles, sound, camera kick, networking) is handled by the weapon.
#[cfg(feature = "client")]
#[derive(Resource, Default)]
struct ReloadGate {
    queued: bool,
}

#[cfg(feature = "client")]
impl ReloadGate {
    fn queue(&mut self) {
        self.queued = true;
    }

    fn consume(&mut self) -> bool {
        std::mem::take(&mut self.queued)
    }
}

#[cfg(feature = "client")]
fn queue_reload_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    mut reload: ResMut<ReloadGate>,
) {
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
    if !blocked && bindings.just_pressed(common::InputAction::Reload, &keyboard, &mouse) {
        reload.queue();
    }
}

#[cfg(feature = "client")]
fn biped_fire(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    pawn: Query<(Entity, &WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    pitch_pivot: Query<&GlobalTransform, With<PitchPivot>>,
    drivers: Query<&WeaponDriver>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut reload: ResMut<ReloadGate>,
    ticker: Res<common::tick::Ticker>,
) {
    let blocked = egui_wants.map_or(false, |e| e.wants_any_input());
    let Ok((pawn_entity, slots, biped)) = pawn.single() else {
        return;
    };
    let Some(weapon_entity) = slots.active().1 else {
        return;
    };
    let Ok(driver) = drivers.get(weapon_entity) else {
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        return;
    };
    let Ok(pivot_gt) = pitch_pivot.get(pitch_e) else {
        return;
    };
    let (_, _, origin) = pivot_gt.to_scale_rotation_translation();
    let reload_pressed = !blocked && reload.consume();
    if reload_pressed
        && let (Some(quic), Some(weapon_net_id)) = (quic.as_deref_mut(), slots.active().0.as_ref())
        && quic.client_connected
    {
        quic.send(
            net::quic::SendTarget::All,
            net::quic::Channel::Ordered,
            &net::message::MsgType::ReloadWeapon(weapon_net_id.clone()),
        );
    }
    commands.run_system_with(
        driver.fixed_update,
        WeaponFireInput {
            weapon: weapon_entity,
            want_fire: !blocked && bindings.pressed(common::InputAction::Fire, &keyboard, &mouse),
            want_alt_fire: !blocked
                && bindings.pressed(common::InputAction::AltFire, &keyboard, &mouse),
            reload_pressed,
            origin,
            shooter: pawn_entity,
            tick: ticker.tick,
        },
    );
}

/// Viewmodel transform offset relative to the camera/pitch pivot.
pub fn viewmodel_offset(_is_primary: bool) -> Transform {
    Transform::from_xyz(0.4 /* right */, -0.3 /* up */, 0.0)
}

#[cfg(feature = "client")]
pub(crate) fn consume_fixed_press(is_down: bool, latched: &mut Local<bool>) -> bool {
    if !is_down {
        **latched = false;
        return false;
    }
    if **latched {
        return false;
    }
    **latched = true;
    true
}

#[cfg(feature = "client")]
#[derive(bevy::ecs::system::SystemParam)]
struct InteractInputParams<'w> {
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    egui_wants: Option<Res<'w, EguiWantsInput>>,
    bindings: Res<'w, common::ActiveKeyBindings>,
    ticker: Res<'w, common::tick::Ticker>,
    interaction: ResMut<'w, InteractionGate>,
}

#[cfg(feature = "client")]
fn interact(
    state: Res<State<common::game_state::GameState>>,
    mut input: InteractInputParams,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    interactables: Query<&net::message::NetworkID, With<crate::interaction::Interactable>>,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    mut world: ResMut<PhysicsWorld>,
    mut possessed_q: Query<&mut WeaponSlots, With<Possessed>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
    vehicle_net_ids: Query<&net::message::NetworkID, With<VehicleComponent>>,
    object_kinds: Query<&GameObjectKind>,
    mut cockpit_q: ParamSet<(
        Query<(Entity, &DriverSeat, &GlobalTransform, &ChildOf)>,
        Query<(&mut DriverSeat, &Transform, &ChildOf)>,
    )>,
) {
    use common::game_state::GameState;
    let blocked = input
        .egui_wants
        .as_ref()
        .is_some_and(|e| e.wants_any_input());
    let Ok((pawn_entity, biped)) = player.single() else {
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        return;
    };
    let Ok(pivot_gt) = pitch_pivots.get(pitch_e) else {
        return;
    };
    let (_, rot, origin) = pivot_gt.to_scale_rotation_translation();
    let forward = rot * Vec3::NEG_Z;
    let mut cockpit_target = None;
    for (cockpit_entity, cockpit, cockpit_gt, child_of) in cockpit_q.p0().iter() {
        let (_, _, seat_center) = cockpit_gt.to_scale_rotation_translation();
        let Some(distance) =
            ray_hits_cockpit(origin, forward, 4.0, seat_center, cockpit.interact_radius)
        else {
            continue;
        };
        if cockpit.occupant.is_some() {
            continue;
        }
        let vehicle_entity = child_of.parent();
        let target = (distance, cockpit_entity, vehicle_entity);
        if cockpit_target.is_none_or(|best: (f32, Entity, Entity)| distance < best.0) {
            cockpit_target = Some(target);
        }
    }

    if let Some((_, cockpit_entity, vehicle_entity)) = cockpit_target {
        if !input.interaction.consume_press(
            !blocked
                && input.bindings.pressed(
                    common::InputAction::Interact,
                    &input.keyboard,
                    &input.mouse,
                ),
            input.ticker.tick,
        ) {
            return;
        }
        match state.get() {
            GameState::SinglePlayer => {
                let mut cockpits = cockpit_q.p1();
                let Ok((mut cockpit, seat_transform, child_of)) = cockpits.get_mut(cockpit_entity)
                else {
                    return;
                };
                if child_of.parent() != vehicle_entity {
                    return;
                }
                if !enter_vehicle(
                    &mut world,
                    pawn_entity,
                    vehicle_entity,
                    &mut cockpit,
                    seat_transform,
                ) {
                    return;
                }
                commands
                    .entity(pawn_entity)
                    .insert(super::SeatedInVehicle(vehicle_entity));
                commands.entity(pawn_entity).remove::<Possessed>();
                commands.entity(vehicle_entity).insert(Possessed::new(128));
                if let Ok(kind) = object_kinds.get(vehicle_entity) {
                    crate::messages::push(&mut commands, format!("Entered {kind:?}"));
                }
                return;
            }
            GameState::Multiplayer => {
                let Ok(vehicle_net_id) = vehicle_net_ids.get(vehicle_entity) else {
                    return;
                };
                quic.send(
                    net::quic::SendTarget::All,
                    net::quic::Channel::Ordered,
                    &net::message::MsgType::Interact(vehicle_net_id.clone()),
                );
                return;
            }
            _ => {}
        }
    }

    let Some((hit_entity, _)) = world.cast_ray(origin, forward, 4.0, &[pawn_entity]) else {
        return;
    };
    let Ok(interact_net_id) = interactables.get(hit_entity) else {
        return;
    };
    let interact_net_id = interact_net_id.clone();
    if !input.interaction.consume_press(
        !blocked
            && input
                .bindings
                .pressed(common::InputAction::Interact, &input.keyboard, &input.mouse),
        input.ticker.tick,
    ) {
        return;
    }

    match state.get() {
        GameState::SinglePlayer => {
            let Ok(mut slots) = possessed_q.single_mut() else {
                return;
            };
            if slots.is_full()
                && let Some((_drop_id, drop_entity)) =
                    crate::weapon::helpers::drop_active_slot(&mut slots)
            {
                let drop_velocity = forward * 8.0
                    + crate::projectile::helpers::shooter_velocity(&world, Some(pawn_entity));
                crate::weapon::helpers::drop_or_despawn_weapon(
                    &mut commands,
                    &mut world,
                    drop_entity,
                    weapon_states.get_mut(drop_entity).ok(),
                    origin + forward,
                    drop_velocity,
                );
            }
            let Some((is_primary, prev_to_hide)) = crate::weapon::helpers::assign_pickup_slot(
                &mut slots,
                interact_net_id.clone(),
                hit_entity,
            ) else {
                return;
            };
            if let Some(prev) = prev_to_hide {
                commands.entity(prev).insert(Visibility::Hidden);
            }
            crate::weapon::helpers::pickup_world_weapon(&mut world, hit_entity);
            crate::weapon::helpers::attach_local_viewmodel(
                &mut commands,
                hit_entity,
                pitch_e,
                is_primary,
            );
            if let Ok(kind) = object_kinds.get(hit_entity) {
                crate::messages::push(&mut commands, format!("Picked up {kind:?}"));
            }
        }
        GameState::Multiplayer => {
            quic.send(
                net::quic::SendTarget::All,
                net::quic::Channel::Ordered,
                &net::message::MsgType::Interact(interact_net_id),
            );
        }
        _ => {}
    }
}

#[cfg(feature = "client")]
fn drop_active_weapon(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Res<EguiWantsInput>,
    bindings: Res<common::ActiveKeyBindings>,
    state: Res<State<common::game_state::GameState>>,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    mut slots_q: Query<&mut WeaponSlots, With<Possessed>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<net::quic::QuicManager>,
    mut drop_pressed: Local<bool>,
) {
    use common::game_state::GameState;
    if egui_wants.wants_any_input()
        || !consume_fixed_press(
            bindings.pressed(common::InputAction::DropWeapon, &keyboard, &mouse),
            &mut drop_pressed,
        )
    {
        return;
    }
    match state.get() {
        GameState::Multiplayer => {
            let Ok((_pawn_entity, biped)) = player.single() else {
                return;
            };
            let Some(pitch_e) = biped.pitch_pivot else {
                return;
            };
            let Ok(pivot_gt) = pitch_pivots.get(pitch_e) else {
                return;
            };
            let (_, rot, _) = pivot_gt.to_scale_rotation_translation();
            quic.send(
                net::quic::SendTarget::All,
                net::quic::Channel::Ordered,
                &net::message::MsgType::DropWeapon(rot * Vec3::NEG_Z),
            );
        }
        GameState::SinglePlayer => {
            let Ok((pawn_entity, biped)) = player.single() else {
                return;
            };
            let Ok(mut slots) = slots_q.single_mut() else {
                return;
            };
            let Some((_weapon_id, weapon_entity)) =
                crate::weapon::helpers::drop_active_slot(&mut slots)
            else {
                return;
            };
            let Some(pitch_e) = biped.pitch_pivot else {
                return;
            };
            let Ok(pivot_gt) = pitch_pivots.get(pitch_e) else {
                return;
            };
            let (_, rot, origin) = pivot_gt.to_scale_rotation_translation();
            let forward = rot * Vec3::NEG_Z;
            let drop_velocity = forward * 8.0
                + crate::projectile::helpers::shooter_velocity(&world, Some(pawn_entity));
            crate::weapon::helpers::drop_or_despawn_weapon(
                &mut commands,
                &mut world,
                weapon_entity,
                weapon_states.get_mut(weapon_entity).ok(),
                origin + forward,
                drop_velocity,
            );
        }
        _ => {}
    }
}
