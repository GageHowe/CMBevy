use crate::{health::Health, GameObject};
use crate::weapon::{rifle, shotgun, hail_mary};
use crate::weapon::Weapon;
use net::message::{NetworkID, SpawnCommand};
use physics::physics_world::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use rapier3d::prelude::*;
use super::*;

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;
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
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponent, input: PawnInput) {
        apply_biped_movement(world, body, input, self);
    }
}
impl GameObject for BipedPawnComponent {
    fn initialize(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
        let entity = commands.spawn((
            WeaponSlots::default(),
            Health::new(100.0),
            Transform::from(transform),
        )).id();
        insert_biped_physics(entity, &transform, commands, world);
        commands.entity(entity).insert(BipedPawnComponent::default());
        entity
    }
    fn cleanup() {}
    fn get_rigidbody() -> Option<RigidBody> {
        Some(RigidBodyBuilder::dynamic().angular_damping(10.0).lock_rotations().build())
    }
}

pub struct BipedPlugin;
impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MouseSensitivity>();
        app.add_systems(FixedUpdate, update_slide_camera);
        app.add_systems(FixedPreUpdate, (
            move_pawns::<BipedPawnComponent>().in_set(MovePawnsSet),
            biped_fire::<rifle::RifleComponent>.run_if(resource_exists::<ButtonInput<MouseButton>>),
            biped_fire::<shotgun::ShotgunComponent>.run_if(resource_exists::<ButtonInput<MouseButton>>),
            biped_fire::<hail_mary::HailMaryComponent>.run_if(resource_exists::<ButtonInput<MouseButton>>),
        ).after(gather_pawn_input));
        app.add_systems(PostUpdate, mouse_look
            .before(TransformSystems::Propagate)
            .run_if(resource_exists::<AccumulatedMouseMotion>));
        app.add_systems(Update, switch_weapon_slot
            .run_if(resource_exists::<AccumulatedMouseScroll>));
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

fn insert_biped_physics(entity: Entity, transform: &Transform, commands: &mut Commands, world: &mut PhysicsWorld) {
    let capsule_rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(10.0)
        .lock_rotations()
        .build();
    let rb_handle = world.insert_body(entity, capsule_rb);
    // collision_groups: detect all (so projectile narrow_phase pairs are generated)
    // solver_groups: exclude projectiles so they don't physically push the player
    // let player_collision = InteractionGroups::new(GROUP_PLAYER, Group::ALL, InteractionTestMode::And);
    let player_solver = InteractionGroups::new(GROUP_PLAYER, Group::ALL & !GROUP_PROJECTILE, InteractionTestMode::And);
    let capsule_collider = ColliderBuilder::capsule_y(CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS)
        .friction(20.0)
        .restitution(0.0)
        .restitution_combine_rule(CoefficientCombineRule::Min)
        // .collision_groups(player_collision)
        .solver_groups(player_solver)
        .build();
    commands.entity(entity).insert(RigidBodyHandleComponent(rb_handle));
    let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
    collider_set.insert_with_parent(capsule_collider, rb_handle, rigid_body_set);
}


/// Adds a mesh, material, and a hidden flashlight to an existing biped entity.
/// Returns the flashlight entity so callers can re-parent it (e.g. under PitchPivot).
pub fn add_visuals(
    entity: Entity,
    color: Color,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    commands.entity(entity).insert((
        Mesh3d(meshes.add(bevy::math::primitives::Capsule3d::new(0.3, 1.0))),
        MeshMaterial3d(materials.add(color)),
        Visibility::default(),
    ));
    let light = commands.spawn((
        SpotLight {
            intensity: 20000.0,
            range: 500.0,
            outer_angle: 0.4,
            inner_angle: 0.3,
            shadows_enabled: true,
            ..default()
        },
        Transform::default(),
        Visibility::Hidden,
    )).id();
    commands.entity(entity).add_child(light);
    light
}

/// Sets up the YawPivot → PitchPivot → Camera hierarchy on an existing biped entity.
/// Pass the pre-existing Camera3d entity so it gets re-parented rather than re-spawned.
/// Pass the flashlight entity to attach it under PitchPivot so it tracks camera look direction.
/// Returns (yaw_pivot, pitch_pivot) entity IDs for caching.
pub fn setup_camera_rig(entity: Entity, camera: Option<Entity>, light: Entity, commands: &mut Commands) -> (Entity, Entity) {
    let pitch_pivot = commands.spawn((
        PitchPivot { pitch: 0.0 },
        Transform::default(),
        Visibility::default(),
    )).id();

    commands.entity(pitch_pivot).add_child(light);

    if let Some(cam) = camera {
        commands.entity(cam).insert(Transform::default());
        commands.entity(pitch_pivot).add_child(cam);
    }

    let yaw_pivot = commands.spawn((
        YawPivot { yaw: 0.0 },
        Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)),
        Visibility::default(),
    )).add_child(pitch_pivot).id();

    commands.entity(entity).add_child(yaw_pivot);
    (yaw_pivot, pitch_pivot)
}

/// Spawns a biped from a server SpawnCommand, adds visuals, and optionally attaches the camera rig.
/// Returns the entity. Caller is responsible for inserting any local-only components (e.g. Possessed).
pub fn spawn_from_command(
    cmd: &SpawnCommand,
    commands: &mut Commands,
    world: &mut PhysicsWorld,
    visual: &mut crate::VisualSpawnParams,
) -> Entity {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = BipedPawnComponent::initialize(transform, commands, world);
    commands.entity(entity).insert(cmd.net_id.clone());
    #[cfg(feature = "client")]
    {
        let color = if cmd.owned { Color::srgb(0.8, 0.8, 0.8) } else { Color::srgb(0.9, 0.4, 0.1) };
        let light = add_visuals(entity, color, commands, visual.meshes, visual.materials);
        let cam = if cmd.owned { visual.camera } else { None };
        let (yaw_pivot, pitch_pivot) = setup_camera_rig(entity, cam, light, commands);
        commands.queue(move |world: &mut World| {
            if let Some(mut biped) = world.entity_mut(entity).get_mut::<BipedPawnComponent>() {
                biped.yaw_pivot = Some(yaw_pivot);
                biped.pitch_pivot = Some(pitch_pivot);
            }
        });
    }
    entity
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
/// Called only when slide state changes, not every tick.
fn replace_capsule_collider(world: &mut PhysicsWorld, rb_handle: RigidBodyHandle, half_height: f32, friction: f32) {
    let player_solver = InteractionGroups::new(GROUP_PLAYER, Group::ALL & !GROUP_PROJECTILE, InteractionTestMode::And);
    // remove old collider
    if let Some(&old_ch) = world.rigid_body_set.get(rb_handle).and_then(|rb| rb.colliders().first()) {
        let PhysicsWorld { collider_set, island_manager, rigid_body_set, .. } = &mut *world;
        collider_set.remove(old_ch, island_manager, rigid_body_set, false);
    }
    let new_col = ColliderBuilder::capsule_y(half_height, CAPSULE_RADIUS)
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
    input: PawnInput,
    biped: &mut BipedPawnComponent,
) {
    // --- read phase ---
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

    let is_jump   = input.up > 0.5;
    let is_slide  = input.up < -0.5;
    let is_sprint = input.ability1;

    // swap collider shape when slide state changes (not every tick)
    if is_slide != biped.is_sliding {
        biped.is_sliding = is_slide;
        let (half_height, friction) = if is_slide { (SLIDE_HALF_HEIGHT, 0.0) } else { (CAPSULE_HALF_HEIGHT, 20.0) };
        replace_capsule_collider(world, body_handle.0, half_height, friction);
    }

    // use the correct capsule bottom for the current shape
    let cur_bottom = if biped.is_sliding { SLIDE_BOTTOM } else { CAPSULE_BOTTOM };

    // grounded check: ray from capsule bottom downward; also fetch surface velocity for relative movement
    let (grounded, ground_linvel) = {
        let ray_origin = capsule_pos - planet_up * cur_bottom;
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
        let air_dir = forward * input.forward + right * input.right + planet_up * input.up;
        if air_dir.length_squared() > 1e-6 {
            let impulse = air_dir.normalize() * AIR_CONTROL * capsule_mass;
            if let Some(rb) = world.rigid_body_set.get_mut(body_handle.0) {
                rb.apply_impulse(Vector::new(impulse.x, impulse.y, impulse.z), true);
            }
        }
    }
}

/// System: every FixedPreUpdate tick, fires the possessed biped's active weapon if mouse is pressed.
/// Sends MsgType::Fire when the weapon discharges (for multiplayer).
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
) {
    let want_fire = !egui_wants.map_or(false, |e| e.wants_any_input()) && mouse.pressed(MouseButton::Left);
    let Ok((pawn_entity, slots, biped)) = pawn.single() else { return };
    let Some(weapon_entity) = slots.slots[slots.active].1 else { return };
    let Ok(mut weapon) = weapons.get_mut(weapon_entity) else { return };
    let Some(pitch_e) = biped.pitch_pivot else { return };
    let Ok(gt) = pitch_pivot.get(pitch_e) else { return };
    let (_, rotation, origin) = gt.to_scale_rotation_translation();
    let aim_dir = rotation * Vec3::NEG_Z;
    if weapon.fixed_update(&mut world, &mut commands, origin, aim_dir, Some(pawn_entity), ticker.tick, want_fire) {
        if let (Some(sq), Some(event)) = (sound_queue.as_mut(), weapon.fire_sound()) {
            // shooter velocity for doppler
            let vel = world.entity_to_handle.get(&pawn_entity)
                .and_then(|&h| world.rigid_body_set.get(h))
                .map(|rb| { let v = rb.linvel(); Vec3::new(v.x, v.y, v.z) })
                .unwrap_or(Vec3::ZERO);
            // own weapon fire is 2D — no spatialization, always sounds centered
            sq.0.push(crate::sound::SoundRequest { event, position: None, velocity: Vec3::ZERO });
        }
        if let (Some(quic), Ok(net_id)) = (quic.as_mut(), net_ids.get(weapon_entity)) {
            quic.send(net::quic::SendTarget::All, net::quic::Channel::Unordered,
                      &net::message::MsgType::Fire(net_id.clone(), origin.into(), aim_dir.into(), ticker.tick));
        }
    }
}
