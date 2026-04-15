#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseMotion;
#[cfg(feature = "client")]
use bevy::input::mouse::AccumulatedMouseScroll;
#[cfg(feature = "client")]
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy::{prelude::*, transform::TransformSystems};
#[cfg(feature = "client")]
use bevy_egui::input::EguiWantsInput;
use net::{
    message::{MsgType, NetworkID},
    quic::{Channel, QuicManager, SendTarget},
};
use physics::physics_world::*;
use rapier3d::prelude::*;

#[cfg(feature = "client")]
use super::vehicle::{DriverSeat, VehicleComponent, enter_vehicle, ray_hits_cockpit};
use super::*;
#[cfg(feature = "client")]
use crate::weapon::{WeaponDriver, WeaponFireInput};
use crate::{
    GameObject, GameObjectKind,
    health::{Health, HealthRegen, LastDamageSource},
};

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

pub const CAPSULE_RADIUS: f32 = 0.3;
pub const CAPSULE_HALF_HEIGHT: f32 = 0.5;
const SLIDE_HALF_HEIGHT: f32 = 0.1;
const CAPSULE_BOTTOM: f32 = CAPSULE_HALF_HEIGHT + CAPSULE_RADIUS;
/// max speed gained per tick when accelerating on the ground
const GROUND_ACCEL: f32 = 0.7;
const JUMP_IMPULSE: f32 = 8.0;
const AIR_CONTROL: f32 = 0.15;
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
const BIPED_REGEN_PER_SEC: f32 = 4.0;

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
            WeaponSlots::new(2).with_delete_on_out_of_ammo(true),
            Health::new(100.0),
            HealthRegen { per_sec: BIPED_REGEN_PER_SEC },
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
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
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
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(bevy::math::primitives::Capsule3d::new(CAPSULE_RADIUS, CAPSULE_HALF_HEIGHT));
            let material =
                world.resource_mut::<Assets<StandardMaterial>>().add(Color::srgb(0.9, 0.4, 0.1));
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
                .spawn((PitchPivot { pitch: 0.0 }, Transform::default(), Visibility::default()))
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
            .map(|slots| slots.held_entities().collect())
            .unwrap_or_default();
        let weapon_drops: Vec<(NetworkID, Vec3)> = world
            .get::<WeaponSlots>(entity)
            .map(|slots| slots.held_weapons().map(|(net_id, _)| (net_id, drop_pos)).collect())
            .unwrap_or_default();
        let owner_net_id = world.get::<NetworkID>(entity).cloned();
        let killer = world
            .get::<LastDamageSource>(entity)
            .copied()
            .and_then(LastDamageSource::resolved_attacker);
        let conn_id = world
            .get_resource::<super::PlayerRegistry>()
            .and_then(|registry| registry.conn_id_for_character(entity));
        let mut physics = world.resource_mut::<PhysicsWorld>();
        for weapon_entity in held {
            crate::weapon::helpers::place_world_weapon(
                &mut physics,
                weapon_entity,
                drop_pos,
                Vec3::ZERO,
            );
        }
        drop(physics);

        if let Some(mut held_map) = world.get_resource_mut::<super::HeldWeaponMap>() {
            for (weapon_id, _) in &weapon_drops {
                held_map.0.remove(weapon_id);
            }
        }
        if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
            for (weapon_id, drop_pos) in weapon_drops.iter().cloned() {
                if let Some(ref net_id) = owner_net_id {
                    quic.send(
                        SendTarget::All,
                        Channel::Ordered,
                        &MsgType::WeaponDrop(weapon_id, net_id.clone(), drop_pos),
                    );
                }
            }
        }
        if let Some(mut pending_kills) =
            world.get_resource_mut::<crate::health::PendingPlayerKills>()
        {
            pending_kills.0.push((entity, killer));
        }
        let mut deferred_removal = false;
        if let Some(mut pending_removals) =
            world.get_resource_mut::<crate::health::PendingPlayerRemovals>()
        {
            pending_removals.0.push(entity);
            deferred_removal = true;
        }
        if !deferred_removal
            && let Some(mut registry) = world.get_resource_mut::<super::PlayerRegistry>()
        {
            let _ = registry.remove_character(entity);
        }
        if let Some(conn_id) = conn_id {
            let respawn_delay = world
                .get_resource::<crate::mode::ModeConfig>()
                .map_or(common::config::RESPAWN_DELAY_SECS, |cfg| cfg.respawn_delay);
            if let Some(mut pending_respawns) = world.get_resource_mut::<super::PendingRespawns>() {
                pending_respawns.0.insert(conn_id, (respawn_delay, common::GameObjectKind::Biped));
            }
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
            app.init_resource::<ReloadGate>().add_systems(Update, queue_reload_input).add_systems(
                FixedPreUpdate,
                (
                    gather_biped_input
                        .run_if(resource_exists::<ButtonInput<KeyCode>>)
                        .in_set(GatherInputSet),
                    move_pawns::<BipedPawnComponent>().in_set(MovePawnsSet),
                    biped_fire.run_if(resource_exists::<ButtonInput<MouseButton>>),
                    toggle_flashlight.run_if(resource_exists::<ButtonInput<KeyCode>>),
                    drop_active_weapon.run_if(resource_exists::<ButtonInput<KeyCode>>),
                    update_interaction_hint.run_if(resource_exists::<ButtonInput<KeyCode>>),
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
                mouse_look
                    .run_if(resource_exists::<AccumulatedMouseMotion>)
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
    input.ability1 = bindings.pressed(common::InputAction::Ability1, &keyboard, &mouse_buttons);

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
        let pitch = local_forward.y.clamp(-1.0, 1.0).asin().clamp(-PITCH_MAX, PITCH_MAX);
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
    let zoom = camera_fx.single().map(|fx| fx.zoom_multiplier.max(1.0)).unwrap_or(1.0);
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
fn switch_weapon_slot(
    scroll: Res<AccumulatedMouseScroll>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawn: Query<&mut WeaponSlots, With<Possessed>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    state: Res<State<common::game_state::GameState>>,
    mut quic: ResMut<net::quic::QuicManager>,
) {
    if scroll.delta.y == 0.0 || egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    let Ok(mut slots) = pawn.single_mut() else {
        return;
    };
    let old_active_primary = slots.active_primary();
    let old_active_weapon = slots.active().1;
    let switched = if scroll.delta.y > 0.0 { slots.next_weapon() } else { slots.prev_weapon() };
    if !switched {
        return;
    }
    if let Some(e) = old_active_weapon {
        crate::weapon::helpers::clear_inactive_slot_reload(weapon_states.get_mut(e).ok());
    }
    crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera);
    if matches!(state.get(), common::game_state::GameState::Multiplayer)
        && old_active_primary != slots.active_primary()
    {
        quic.send_to_server(
            net::quic::Channel::Ordered,
            &net::message::MsgType::SetActiveWeaponSlot(slots.active_primary()),
        );
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
        t.translation.y = if biped.is_sliding && grounded { -0.1 } else { 0.4 };
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
        // Projectile gravity sensors should never count as support geometry.
        !col.is_sensor() && col.parent().map_or(true, |rb| rb != body_handle)
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
        let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
        collider_set.remove(old_ch, island_manager, rigid_body_set, false);
    }
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
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
    let desired = biped_move_direction(body_rot, input);

    let is_jump = input.jump;
    let is_slide = input.slide;
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
        if desired.length_squared() > 1e-6 {
            let impulse = desired * GROUND_ACCEL * capsule_mass;
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
        let down = input.slide as i8 as f32;
        let impulse = (desired * AIR_CONTROL - planet_up * down * AIR_CONTROL) * capsule_mass;
        if impulse.length_squared() > 1e-6 {
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }
}

pub fn biped_move_direction(body_rot: Quat, input: BipedInput) -> Vec3 {
    let facing = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward = facing * Vec3::NEG_Z;
    let right = facing * Vec3::X;
    (forward * input.forward + right * input.right).normalize_or_zero()
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
    let base_fov = if let Projection::Perspective(p) = proj { p.fov.to_degrees() } else { 90.0 };
    commands.entity(cam).insert((
        Transform::default(),
        CameraEffector {
            base_translation: Vec3::ZERO,
            base_fov,
            current_fov: base_fov,
            ..default()
        },
    ));
    commands.entity(pitch_e).add_child(cam);
    crate::weapon::helpers::set_local_slot_visibility(&mut commands, slots);
}

#[cfg(feature = "client")]
fn hide_weapons_while_seated(
    seated: Query<&WeaponSlots, Added<super::SeatedInVehicle>>,
    mut commands: Commands,
) {
    for slots in seated.iter() {
        for weapon in slots.held_entities() {
            commands.entity(weapon).insert(Visibility::Hidden);
        }
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
                *vis = if *on { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
    // no-op in singleplayer (client_connected is false)
    if quic.client_connected {
        quic.send_to_server(
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
    mut pawn: Query<(Entity, &mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    pitch_pivot: Query<&GlobalTransform, With<PitchPivot>>,
    drivers: Query<&WeaponDriver>,
    weapon_states: Query<&crate::weapon::WeaponState>,
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut reload: ResMut<ReloadGate>,
    ticker: Res<common::tick::Ticker>,
) {
    let blocked = egui_wants.map_or(false, |e| e.wants_any_input());
    let want_fire = !blocked && bindings.pressed(common::InputAction::Fire, &keyboard, &mouse);
    let Ok((pawn_entity, mut slots, biped)) = pawn.single_mut() else {
        return;
    };
    if !want_fire {
        slots.block_fire_until_release = false;
    }
    if slots.block_fire_until_release {
        return;
    }
    let Some(weapon_entity) = slots.active().1 else {
        return;
    };
    let Ok(driver) = drivers.get(weapon_entity) else {
        let Some((_weapon_id, _removed_weapon_entity)) = slots.remove_active() else {
            return;
        };
        crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera_fx);
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
        quic.send_to_server(
            net::quic::Channel::Ordered,
            &net::message::MsgType::ReloadWeapon(weapon_net_id.clone()),
        );
    }
    commands.run_system_with(
        driver.fixed_update,
        WeaponFireInput {
            weapon: weapon_entity,
            want_fire,
            want_alt_fire: !blocked
                && bindings.pressed(common::InputAction::AltFire, &keyboard, &mouse),
            reload_pressed,
            origin,
            shooter: pawn_entity,
            tick: ticker.tick,
        },
    );
    let Ok(weapon_state) = weapon_states.get(weapon_entity) else {
        return;
    };
    if !slots.delete_on_out_of_ammo || !crate::weapon::is_depleted(weapon_state) {
        return;
    }
    let Some((_weapon_id, depleted_weapon_entity)) = slots.remove_active() else {
        return;
    };
    slots.block_fire_until_release = want_fire;
    commands.entity(depleted_weapon_entity).despawn();
    crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera_fx);
    crate::messages::push(&mut commands, "Out of ammo");
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
enum InteractTarget {
    Vehicle { cockpit_entity: Entity, vehicle_entity: Entity },
    Entity { hit_entity: Entity, net_id: net::message::NetworkID },
}

#[cfg(feature = "client")]
fn format_interaction_prompt(key: &str, verb: &str, kind: GameObjectKind) -> String {
    format!("Press {key} to {verb} {}", kind.interaction_name())
}

#[cfg(feature = "client")]
fn current_interact_target(
    pawn_entity: Entity,
    origin: Vec3,
    forward: Vec3,
    world: &PhysicsWorld,
    interactables: &Query<
        (&net::message::NetworkID, &crate::interaction::Interactable),
        With<crate::interaction::Interactable>,
    >,
    cockpits: &Query<(Entity, &DriverSeat, &GlobalTransform, &ChildOf)>,
) -> Option<InteractTarget> {
    let mut cockpit_target = None;
    for (cockpit_entity, cockpit, cockpit_gt, child_of) in cockpits.iter() {
        let (_, _, seat_center) = cockpit_gt.to_scale_rotation_translation();
        let Some(distance) =
            ray_hits_cockpit(origin, forward, cockpit.interact_radius + 4.0, seat_center, cockpit.interact_radius)
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
        return Some(InteractTarget::Vehicle { cockpit_entity, vehicle_entity });
    }

    let (hit_entity, _) = world.cast_ray(origin, forward, 4.0, &[pawn_entity])?;
    let (net_id, interactable) = interactables.get(hit_entity).ok()?;
    if !interactable_in_range(world, pawn_entity, hit_entity, interactable.range) {
        return None;
    }
    let net_id = net_id.clone();
    Some(InteractTarget::Entity { hit_entity, net_id })
}

#[cfg(feature = "client")]
fn interactable_in_range(
    world: &PhysicsWorld,
    pawn_entity: Entity,
    target_entity: Entity,
    range: f32,
) -> bool {
    matches!(
        (
            world.entity_to_handle.get(&pawn_entity).and_then(|&h| world.rigid_body_set.get(h)).map(rb_pos),
            world.entity_to_handle.get(&target_entity).and_then(|&h| world.rigid_body_set.get(h)).map(rb_pos),
        ),
        (Some(pawn_pos), Some(target_pos)) if pawn_pos.distance_squared(target_pos) <= range * range
    )
}

#[cfg(feature = "client")]
fn update_interaction_hint(
    egui_wants: Option<Res<EguiWantsInput>>,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    bindings: Res<common::ActiveKeyBindings>,
    interactables: Query<
        (&net::message::NetworkID, &crate::interaction::Interactable),
        With<crate::interaction::Interactable>,
    >,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    world: Res<PhysicsWorld>,
    object_kinds: Query<&GameObjectKind>,
    weapon_q: Query<(), With<crate::weapon::WeaponComponent>>,
    pickup_q: Query<(), With<crate::pawn::biped_ability::OnPickup>>,
    cockpit_q: Query<(Entity, &DriverSeat, &GlobalTransform, &ChildOf)>,
    mut hint: ResMut<InteractionHint>,
) {
    if egui_wants.as_ref().is_some_and(|e| e.wants_any_input()) {
        hint.0 = None;
        return;
    }
    let Ok((pawn_entity, biped)) = player.single() else {
        hint.0 = None;
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        hint.0 = None;
        return;
    };
    let Ok(pivot_gt) = pitch_pivots.get(pitch_e) else {
        hint.0 = None;
        return;
    };
    let (_, rot, origin) = pivot_gt.to_scale_rotation_translation();
    let forward = rot * Vec3::NEG_Z;
    let Some(target) =
        current_interact_target(pawn_entity, origin, forward, &world, &interactables, &cockpit_q)
    else {
        hint.0 = None;
        return;
    };
    let key = bindings.binding(common::InputAction::Interact).prompt_label();
    hint.0 = match target {
        InteractTarget::Vehicle { vehicle_entity, .. } => object_kinds
            .get(vehicle_entity)
            .ok()
            .map(|kind| format_interaction_prompt(&key, "enter", kind.clone())),
        InteractTarget::Entity { hit_entity, .. } if weapon_q.contains(hit_entity) => object_kinds
            .get(hit_entity)
            .ok()
            .map(|kind| format_interaction_prompt(&key, "equip", kind.clone())),
        InteractTarget::Entity { hit_entity, .. } if pickup_q.contains(hit_entity) => object_kinds
            .get(hit_entity)
            .ok()
            .map(|kind| format_interaction_prompt(&key, "equip", kind.clone())),
        _ => None,
    };
}

#[cfg(feature = "client")]
fn interact(
    state: Res<State<common::game_state::GameState>>,
    mut input: InteractInputParams,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    interactables: Query<
        (&net::message::NetworkID, &crate::interaction::Interactable),
        With<crate::interaction::Interactable>,
    >,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    mut world: ResMut<PhysicsWorld>,
    mut possessed_q: Query<&mut WeaponSlots, With<Possessed>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    mut quic: ResMut<net::quic::QuicManager>,
    vehicle_net_ids: Query<&net::message::NetworkID, With<VehicleComponent>>,
    object_kinds: Query<&GameObjectKind>,
    pickup_fns: Query<&crate::pawn::biped_ability::OnPickup>,
    mut cockpit_q: ParamSet<(
        Query<(Entity, &DriverSeat, &GlobalTransform, &ChildOf)>,
        Query<(&mut DriverSeat, &Transform, &ChildOf)>,
    )>,
) {
    use common::game_state::GameState;
    let blocked = input.egui_wants.as_ref().is_some_and(|e| e.wants_any_input());
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
    let Some(target) = current_interact_target(
        pawn_entity,
        origin,
        forward,
        &world,
        &interactables,
        &cockpit_q.p0(),
    ) else {
        return;
    };
    if !input.interaction.consume_press(
        !blocked
            && input.bindings.pressed(common::InputAction::Interact, &input.keyboard, &input.mouse),
        input.ticker.tick,
    ) {
        return;
    }
    match target {
        InteractTarget::Vehicle { cockpit_entity, vehicle_entity } => match state.get() {
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
                commands.entity(pawn_entity).insert(super::SeatedInVehicle(vehicle_entity));
                commands.entity(pawn_entity).remove::<Possessed>();
                commands.entity(vehicle_entity).insert(Possessed::new(128));
                if let Ok(kind) = object_kinds.get(vehicle_entity) {
                    crate::messages::push(
                        &mut commands,
                        format!("Entered {}", kind.interaction_name()),
                    );
                }
            }
            GameState::Multiplayer => {
                let Ok(vehicle_net_id) = vehicle_net_ids.get(vehicle_entity) else {
                    return;
                };
                quic.send_to_server(
                    net::quic::Channel::Ordered,
                    &net::message::MsgType::Interact(vehicle_net_id.clone()),
                );
            }
            _ => {}
        },
        InteractTarget::Entity { hit_entity, net_id: interact_net_id } => {
    if let Ok(&crate::pawn::biped_ability::OnPickup(f)) = pickup_fns.get(hit_entity) {
        // Always run locally for prediction (singleplayer) or immediate feedback (multiplayer).
        f(pawn_entity, hit_entity, &mut commands);
        if matches!(state.get(), GameState::Multiplayer) {
            quic.send_to_server(
                net::quic::Channel::Ordered,
                &net::message::MsgType::Interact(interact_net_id),
            );
        }
        return;
    }

    match state.get() {
        GameState::SinglePlayer => {
            let Ok(mut slots) = possessed_q.single_mut() else {
                return;
            };
            if slots.is_full()
                && let Some((_drop_id, drop_entity)) = slots.remove_active()
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
            let Some((is_primary, _prev_to_hide)) =
                slots.assign_pickup(interact_net_id.clone(), hit_entity)
            else {
                return;
            };
            crate::weapon::helpers::pickup_world_weapon(&mut world, hit_entity);
            crate::weapon::helpers::attach_local_viewmodel(
                &mut commands,
                hit_entity,
                pitch_e,
                is_primary,
            );
            crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera_fx);
            if let Ok(kind) = object_kinds.get(hit_entity) {
                crate::messages::push(
                    &mut commands,
                    format!("Picked up {}", kind.interaction_name()),
                );
            }
        }
        GameState::Multiplayer => {
            quic.send_to_server(
                net::quic::Channel::Ordered,
                &net::message::MsgType::Interact(interact_net_id),
            );
        }
        _ => {}
    }
        }
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
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
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
            quic.send_to_server(
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
            let Some((_weapon_id, weapon_entity)) = slots.remove_active() else {
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
            crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera_fx);
        }
        _ => {}
    }
}
