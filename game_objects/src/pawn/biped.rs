use crate::{health::Health, GameObject};
use crate::weapon::{rifle, hail_mary};
use crate::weapon::{Weapon, FireCtx};
use net::message::NetworkID;
use physics::physics_world::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::input::EguiWantsInput;
use rapier3d::prelude::*;
use super::*;

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

use super::CameraEffects;
const CAPSULE_RADIUS:      f32 = 0.3;
/// half-height of the standing capsule (total height = 2*(0.5+0.3) = 1.6 m)
const CAPSULE_HALF_HEIGHT: f32 = 0.5;
/// half-height of the sliding capsule (total height = 2*(0.1+0.3) = 0.8 m)
const SLIDE_HALF_HEIGHT:   f32 = 0.1;
const CAPSULE_BOTTOM: f32 = CAPSULE_HALF_HEIGHT + CAPSULE_RADIUS; // 0.8
const SLIDE_BOTTOM:   f32 = SLIDE_HALF_HEIGHT   + CAPSULE_RADIUS; // 0.4
const MAX_WALK_SPEED:  f32 = 40.0;
const MAX_SPRINT_SPEED: f32 = 60.0;
/// max speed gained per tick when accelerating on the ground
const GROUND_ACCEL:    f32 = 10.0;
const JUMP_IMPULSE:    f32 = 30.0;
const AIR_CONTROL:     f32 = 0.5;
const GROUND_DIST:     f32 = 0.01;  // must be nearly touching to count as grounded
const JUMP_COOLDOWN:   u8  = 25;    // ticks (~0.4 s at 60 Hz) before another jump


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
    pub is_sliding: bool,
}
impl Pawn for BipedPawnComponent {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponent, input: PawnInputKind) {
        if let PawnInputKind::Biped(i) = input { apply_biped_movement(world, body, i, self); }
    }
}
impl GameObject for BipedPawnComponent {
    fn spawn(entity: Entity, cmd: &net::message::SpawnCommand, world: &mut World) {
        let transform = Transform { translation: cmd.position.into(), rotation: cmd.rotation.into(), ..default() };
        world.entity_mut(entity).insert((
            WeaponSlots::default(),
            Health::new(100.0),
            Transform::from(transform),
            BipedPawnComponent::default(),
            cmd.net_id.clone(),
        ));
        // physics
        let rb_handle = {
            let mut physics = world.resource_mut::<PhysicsWorld>();
            let capsule_rb = RigidBodyBuilder::dynamic()
                .translation(transform.translation)
                .angular_damping(10.0)
                .lock_rotations()
                .ccd_enabled(true)
                .build();
            let rb_handle = physics.insert_body(entity, capsule_rb);
            let player_solver = InteractionGroups::new(GROUP_PLAYER, Group::ALL & !GROUP_PROJECTILE, InteractionTestMode::And);
            let capsule_collider = ColliderBuilder::capsule_y(CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS)
                .friction(5.0)
                .restitution(0.0)
                .restitution_combine_rule(CoefficientCombineRule::Min)
                .solver_groups(player_solver)
                .build();
            let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *physics;
            collider_set.insert_with_parent(capsule_collider, rb_handle, rigid_body_set);
            rb_handle
        };
        world.entity_mut(entity).insert(RigidBodyHandleComponent(rb_handle));
        #[cfg(feature = "client")]
        {
            // placeholder — matches physics capsule dimensions exactly; swap for a real model later
            let mesh = world.resource_mut::<Assets<Mesh>>().add(bevy::math::primitives::Capsule3d::new(CAPSULE_RADIUS, CAPSULE_HALF_HEIGHT));
            let material = world.resource_mut::<Assets<StandardMaterial>>().add(Color::srgb(0.9, 0.4, 0.1));
            world.entity_mut(entity).insert((Mesh3d(mesh), MeshMaterial3d(material), Visibility::default()));
            let light = world.spawn((
                SpotLight { intensity: 20000.0, range: 500.0, outer_angle: 0.4, inner_angle: 0.3, shadows_enabled: true, ..default() },
                Transform::default(),
                Visibility::Hidden,
            )).id();
            let pitch_pivot = world.spawn((PitchPivot { pitch: 0.0 }, Transform::default(), Visibility::default())).id();
            world.entity_mut(pitch_pivot).add_child(light);
            let yaw_pivot = world.spawn((
                YawPivot { yaw: 0.0 },
                Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)),
                Visibility::default(),
            )).id();
            world.entity_mut(yaw_pivot).add_child(pitch_pivot);
            world.entity_mut(entity).add_child(yaw_pivot);
            // cache pivot entities so input and camera logic can find them
            if let Some(mut biped) = world.entity_mut(entity).get_mut::<BipedPawnComponent>() {
                biped.yaw_pivot = Some(yaw_pivot);
                biped.pitch_pivot = Some(pitch_pivot);
            }
        }
    }
}

pub struct BipedPlugin;
impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MouseSensitivity>();
        app.add_systems(FixedUpdate, update_slide_camera);
        app.add_systems(FixedPreUpdate, (
            gather_biped_input
                .run_if(resource_exists::<ButtonInput<KeyCode>>)
                .in_set(GatherInputSet),
            move_pawns::<BipedPawnComponent>().in_set(MovePawnsSet),
            biped_fire::<rifle::RifleComponent>.run_if(resource_exists::<ButtonInput<MouseButton>>),
            biped_fire::<hail_mary::HailMaryComponent>.run_if(resource_exists::<ButtonInput<MouseButton>>),
        ).chain());
        app.add_systems(PostUpdate, (
            mouse_look.run_if(resource_exists::<AccumulatedMouseMotion>),
            apply_camera_effects,
        ).chain().before(TransformSystems::Propagate));
        app.add_systems(Update, switch_weapon_slot
            .run_if(resource_exists::<AccumulatedMouseScroll>));
        #[cfg(feature = "client")]
        {
            // re-parent camera under pitch pivot when a biped is possessed
            app.add_systems(Update, attach_camera_on_possess);
            // Y key toggles flashlight; sends FlashlightToggle to server when connected
            app.add_systems(Update, toggle_flashlight);
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
fn gather_biped_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawns: Query<(&mut Possessed, &BipedPawnComponent)>,
    yaw_pivots: Query<&YawPivot>,
    pitch_pivots: Query<&PitchPivot>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) { return; }
    let Ok((mut possessed, biped)) = pawns.single_mut() else { return };

    let mut input = BipedInput::default();
    if keyboard.pressed(KeyCode::KeyW) { input.forward += 1.0; }
    if keyboard.pressed(KeyCode::KeyS) { input.forward -= 1.0; }
    if keyboard.pressed(KeyCode::KeyD) { input.right += 1.0; }
    if keyboard.pressed(KeyCode::KeyA) { input.right -= 1.0; }
    input.jump = keyboard.pressed(KeyCode::Space);
    input.slide = keyboard.pressed(KeyCode::ControlLeft);
    input.ability1 = keyboard.pressed(KeyCode::ShiftLeft);
    input.ability2 = keyboard.pressed(KeyCode::KeyE);

    if let Some(yaw_e) = biped.yaw_pivot {
        if let Ok(yp) = yaw_pivots.get(yaw_e) { input.look_yaw = yp.yaw; }
    }
    if let Some(pitch_e) = biped.pitch_pivot {
        if let Ok(pp) = pitch_pivots.get(pitch_e) { input.look_pitch = pp.pitch; }
    }

    possessed.push(PawnInputKind::Biped(input));
}

/// moves the biped's yaw and pitch components on Update
fn mouse_look(
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    possessed: Query<&BipedPawnComponent, With<Possessed>>,
    mut pivots: ParamSet<(
        Query<(&mut Transform, &mut YawPivot)>,
        Query<(&mut Transform, &mut PitchPivot)>,
    )>,
) {
    if cursor_q.grab_mode == CursorGrabMode::None { return; }
    let delta = mouse.delta;
    if delta == Vec2::ZERO { return; }
    let Ok(biped) = possessed.single() else { return };
    let s = sensitivity.0;

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

const KICK_DAMPING: f32 = 0.88;  // velocity multiplier per tick at 60 Hz
const SHAKE_DECAY:   f32 = 6.0;   // intensity units per second
const FOV_LERP_SPEED: f32 = 8.0;  // how fast zoom eases in/out

/// Integrates recoil, shake, and FOV zoom. Writes Camera3d local Transform and Projection.
fn apply_camera_effects(
    time: Res<Time>,
    mut camera_q: Query<(&mut Transform, &mut CameraEffects, &mut Projection), With<Camera3d>>,
) {
    let Ok((mut transform, mut fx, mut proj)) = camera_q.single_mut() else { return };
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

    transform.rotation = Quat::from_euler(EulerRot::XYZ, fx.pitch_offset + sp, fx.yaw_offset + sy, 0.0);

    // FOV zoom: target = 2 * atan(tan(base/2) / multiplier) — correct optics
    let target_fov = ((fx.base_fov / 2.0).to_radians().tan() / fx.zoom_multiplier).atan().to_degrees() * 2.0;
    fx.current_fov += (target_fov - fx.current_fov) * (1.0 - (-FOV_LERP_SPEED * dt).exp());
    if let Projection::Perspective(ref mut p) = *proj { p.fov = fx.current_fov.to_radians(); }
}

fn switch_weapon_slot(
    scroll: Res<AccumulatedMouseScroll>,
    mut pawn: Query<&mut WeaponSlots, With<Possessed>>,
    mut visibility: Query<&mut Visibility>,
) {
    let delta: f32 = scroll.delta.y;
    if delta == 0.0 { return; }
    let Ok(mut slots) = pawn.single_mut() else { return };
    let prev = slots.active;
    slots.active = if delta > 0.0 { (slots.active + 1) % 2 } else { slots.active.checked_sub(1).unwrap_or(1) };
    if slots.active == prev { return; }
    if let Some(e) = slots.slots[prev].1 {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Hidden; }
    }
    if let Some(e) = slots.slots[slots.active].1 {
        if let Ok(mut vis) = visibility.get_mut(e) { *vis = Visibility::Inherited; }
    }
}

/// Two weapon slots on a biped pawn, stored on the entity.
/// Each slot holds the NetworkID and (client-only) the local weapon entity for the viewmodel.
#[derive(Component, Default)]
pub struct WeaponSlots {
    pub slots: [(Option<NetworkID>, Option<Entity>); 2],
    pub active: usize,
}

#[cfg(feature = "client")]
pub fn draw_biped_debug(
    world: Res<PhysicsWorld>,
    bipeds: Query<&RigidBodyHandleComponent, With<BipedPawnComponent>>,
    mut gizmos: Gizmos,
) {
    use physics::debug::{draw_collider, rb_iso};
    for body_handle in bipeds.iter() {
        let Some(rb) = world.rigid_body_set.get(body_handle.0) else { continue };
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
    bipeds: Query<&BipedPawnComponent>,
    mut pivots: Query<&mut Transform, With<YawPivot>>,
) {
    for biped in bipeds.iter() {
        let Some(yaw_e) = biped.yaw_pivot else { continue };
        let Ok(mut t) = pivots.get_mut(yaw_e) else { continue };
        t.translation.y = if biped.is_sliding { -0.1 } else { 0.4 };
    }
}

/// Replaces the capsule collider on a biped rigid body.
/// does this play nicely with reconciliation and the global map?
fn replace_capsule_collider(world: &mut PhysicsWorld, rb_handle: RigidBodyHandle, half_height: f32, friction: f32) {
    let player_solver = InteractionGroups::new(GROUP_PLAYER, Group::ALL & !GROUP_PROJECTILE, InteractionTestMode::And);
    // remove old collider
    if let Some(&old_ch) = world.rigid_body_set.get(rb_handle).and_then(|rb| rb.colliders().first()) {
        let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
        collider_set.remove(old_ch, island_manager, rigid_body_set, false);
    }
    // offset the collider so its bottom stays at foot level (body center is always CAPSULE_BOTTOM above ground)
    let y_offset = (half_height + CAPSULE_RADIUS) - CAPSULE_BOTTOM;
    let new_col = ColliderBuilder::capsule_y(half_height, CAPSULE_RADIUS)
        .translation(Vector3::new(0.0, y_offset, 0.0))
        .friction(friction)
        .restitution(0.0)
        .restitution_combine_rule(CoefficientCombineRule::Min)
        .solver_groups(player_solver)
        .build();
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(new_col, rb_handle, rigid_body_set);
}

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponent,
    input: BipedInput,
    biped: &mut BipedPawnComponent,
) {
    let (body_rot, capsule_pos, capsule_linvel, capsule_mass) = {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else { return };
        let r = body.rotation();
        let t = body.position().translation;
        let v = body.linvel();
        (
            Quat::from_xyzw(r.x, r.y, r.z, r.w),
            Vec3::new(t.x, t.y, t.z),
            Vec3::new(v.x, v.y, v.z),
            body.mass(),
        )
    };

    let planet_up = body_rot * Vec3::Y;
    let facing    = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward   = facing * Vec3::NEG_Z;
    let right     = facing * Vec3::X;

    let is_jump  = input.jump;
    let is_slide = input.slide;
    let is_sprint = input.ability1;

    // swap collider shape when slide state changes (not every tick)
    if is_slide != biped.is_sliding {
        biped.is_sliding = is_slide;
        let (half_height, friction) = if is_slide { (SLIDE_HALF_HEIGHT, 0.0) } else { (CAPSULE_HALF_HEIGHT, 20.0) };
        replace_capsule_collider(world, body_handle.0, half_height, friction);
    }

    // body center is always CAPSULE_BOTTOM above foot level; cast ray from foot position
    let (grounded, ground_linvel) = {
        let ray_origin = capsule_pos - planet_up * CAPSULE_BOTTOM;
        let capsule_handle = body_handle.0;
        let exclude = |_ch: ColliderHandle, col: &rapier3d::prelude::Collider| {
            col.parent().map_or(true, |rb| rb != capsule_handle)
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
            // surface velocity — zero for static geometry, nonzero for moving planets/platforms
            let vel = world.collider_set.get(ch)
                .and_then(|col| col.parent())
                .and_then(|rb_h| world.rigid_body_set.get(rb_h))
                .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
                .unwrap_or(Vec3::ZERO);
            (true, vel)
        } else {
            (false, Vec3::ZERO)
        }
    };

    biped.jump_cooldown = biped.jump_cooldown.saturating_sub(1);

    // relative horizontal velocity — used for speed cap so movement is correct on moving planets/platforms
    let horiz_vel    = capsule_linvel - planet_up * planet_up.dot(capsule_linvel);
    let ground_horiz = ground_linvel  - planet_up * planet_up.dot(ground_linvel);
    let rel_horiz    = horiz_vel - ground_horiz;

    if grounded && !is_slide {
        let desired = (forward * input.forward + right * input.right).normalize_or_zero();
        let max_speed = if is_sprint && input.forward >= 0.0 { MAX_SPRINT_SPEED } else { MAX_WALK_SPEED };

        if desired.length_squared() > 1e-6 {
            // accelerate toward desired direction up to max_speed relative to surface
            let cur = rel_horiz.dot(desired);
            if cur < max_speed {
                let delta = (max_speed - cur).min(GROUND_ACCEL);
                let impulse = desired * delta * capsule_mass;
                if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                    rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
                }
            }
        }
    }

    if is_jump && grounded && biped.jump_cooldown == 0 {
        biped.jump_cooldown = JUMP_COOLDOWN;
        let impulse = planet_up * JUMP_IMPULSE * capsule_mass;
        if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
            rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
        }
    }

    if !grounded {
        let up = input.jump as i8 as f32 - input.slide as i8 as f32;
        let air_dir = forward * input.forward + right * input.right + planet_up * up;
        if air_dir.length_squared() > 1e-6 {
            let impulse = air_dir.normalize() * AIR_CONTROL * capsule_mass;
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
    bipeds: Query<&BipedPawnComponent, Added<Possessed>>,
    camera: Query<(Entity, &Projection), With<Camera3d>>,
    mut commands: Commands,
) {
    let Ok(biped) = bipeds.single() else { return };
    let Ok((cam, proj)) = camera.single() else { return };
    let Some(pitch_e) = biped.pitch_pivot else { return };
    // read current projection FOV so CameraEffects starts in sync with settings
    let base_fov = if let Projection::Perspective(p) = proj { p.fov.to_degrees() } else { 90.0 };
    commands.entity(cam).insert((Transform::default(), CameraEffects { base_fov, current_fov: base_fov, ..default() }));
    commands.entity(pitch_e).add_child(cam);
}

/// Y key toggles the local player's flashlight. Sends FlashlightToggle to server when connected.
#[cfg(feature = "client")]
fn toggle_flashlight(
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Res<EguiWantsInput>,
    possessed_q: Query<&BipedPawnComponent, With<Possessed>>,
    pitch_pivot: Query<&Children, With<PitchPivot>>,
    mut lights: Query<&mut Visibility, With<SpotLight>>,
    mut quic: ResMut<net::quic::QuicManager>,
    mut on: Local<bool>,
) {
    if egui_wants.wants_any_input() || !keyboard.just_pressed(KeyCode::KeyY) { return; }
    *on = !*on;
    if let Ok(biped) = possessed_q.single() {
        if let Some(pitch_e) = biped.pitch_pivot {
            if let Ok(children) = pitch_pivot.get(pitch_e) {
                for child in children.iter() {
                    if let Ok(mut vis) = lights.get_mut(child) {
                        *vis = if *on { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
        }
    }
    // no-op in singleplayer (client_connected is false)
    if quic.client_connected {
        quic.send(net::quic::SendTarget::All, net::quic::Channel::Ordered, &net::message::MsgType::FlashlightToggle);
    }
}

/// Forwards input to the possessed biped's active weapon each FixedPreUpdate tick.
/// All fire logic (projectiles, sound, camera kick, networking) is handled by the weapon.
pub fn biped_fire<W: Weapon>(
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Option<Res<bevy_egui::input::EguiWantsInput>>,
    pawn: Query<(Entity, &WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    pitch_pivot: Query<&GlobalTransform, With<PitchPivot>>,
    mut weapons: Query<&mut W>,
    net_ids: Query<&NetworkID>,
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut sound_queue: Option<ResMut<crate::sound::SoundQueue>>,
    ticker: Res<common::tick::Ticker>,
    mut camera_fx: Query<(&mut CameraEffects, &GlobalTransform), With<Camera3d>>,
) {
    let blocked = egui_wants.map_or(false, |e| e.wants_any_input());
    let Ok((pawn_entity, slots, biped)) = pawn.single() else { return };
    let Some(weapon_entity) = slots.slots[slots.active].1 else { return };
    let Ok(mut weapon) = weapons.get_mut(weapon_entity) else { return };
    let Some(pitch_e) = biped.pitch_pivot else { return };
    let Ok(pivot_gt) = pitch_pivot.get(pitch_e) else { return };
    let (_, _, origin) = pivot_gt.to_scale_rotation_translation();

    // use camera's GlobalTransform for aim so kick offsets affect projectile direction
    let Ok((mut cam_fx, cam_gt)) = camera_fx.single_mut() else { return };
    let (_, cam_rot, _) = cam_gt.to_scale_rotation_translation();
    let mut ctx = FireCtx {
        want_fire:     !blocked && mouse.pressed(MouseButton::Left),
        want_alt_fire: !blocked && mouse.pressed(MouseButton::Right),
        origin,
        aim_dir: cam_rot * Vec3::NEG_Z,
        shooter: Some(pawn_entity),
        tick: ticker.tick,
        net_id: net_ids.get(weapon_entity).ok(),
        shooter_net_id: net_ids.get(pawn_entity).ok(),
        sound: sound_queue.as_deref_mut(),
        camera: Some(&mut *cam_fx),
        quic: quic.as_deref_mut(),
    };
    weapon.fixed_update(&mut world, &mut commands, &mut ctx);
}
