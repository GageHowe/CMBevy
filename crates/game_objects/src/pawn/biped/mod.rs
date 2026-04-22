use bevy::{prelude::*, transform::TransformSystems};
use net::{
    message::{MsgType, NetworkID},
    quic::{Channel, QuicManager, SendTarget},
};
use physics::physics_world::*;
use rapier3d::prelude::*;

use super::*;
use crate::{
    GameObject, GameObjectKind,
    health::{DamageCause, Health, HealthRegen, LastDamageSource},
    spawn::AppGameObjectExt,
};

#[cfg(feature = "client")]
mod client;

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
    #[reflect(ignore)]
    pub ability: Option<crate::pawn::biped_ability::EquippedAbility>,
    #[cfg(feature = "client")]
    #[reflect(ignore)]
    pub jetpack_fx_entity: Option<Entity>,
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
    const KIND: GameObjectKind = GameObjectKind::Biped;

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
        #[cfg(feature = "client")]
        if let Some(fx_entity) =
            world.get::<BipedPawnComponent>(entity).and_then(|biped| biped.jetpack_fx_entity)
        {
            if let Ok(fx) = world.get_entity_mut(fx_entity) {
                fx.despawn();
            }
        }
        let owner_net_id = world.get::<NetworkID>(entity).cloned();
        let last_damage = world.get::<LastDamageSource>(entity).copied().unwrap_or_default();
        let killer = last_damage.resolved_attacker();
        let conn_id = world
            .get_resource::<super::PlayerRegistry>()
            .and_then(|registry| registry.conn_id_for_character(entity));
        push_death_message(world, entity, killer, last_damage.cause);
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
            let team = world.get::<crate::Team>(entity).copied().unwrap_or(crate::Team(0));
            if let Some(mut pending_respawns) = world.get_resource_mut::<super::PendingRespawns>() {
                pending_respawns
                    .0
                    .insert(conn_id, (respawn_delay, common::GameObjectKind::Biped, team));
            }
        }
        true
    }
}

fn push_death_message(
    world: &mut World,
    victim: Entity,
    killer: Option<Entity>,
    cause: DamageCause,
) {
    let victim_name = player_name(world, victim);
    let killer_name = killer.map(|killer| player_name(world, killer));
    let text = match (killer, killer_name, cause) {
        (Some(killer), Some(_), DamageCause::Explosion) if killer == victim => {
            format!("{victim_name} blew themselves up")
        }
        (Some(killer), Some(_), _) if killer == victim => {
            format!("{victim_name} committed suicide")
        }
        (Some(_), Some(killer_name), DamageCause::Sniper) => {
            format!("{killer_name} sniped {victim_name}")
        }
        (Some(_), Some(killer_name), DamageCause::Explosion) => {
            format!("{killer_name} blew up {victim_name}")
        }
        (Some(_), Some(killer_name), _) => format!("{killer_name} killed {victim_name}"),
        (_, _, DamageCause::Explosion) => format!("{victim_name} blew up"),
        (_, _, DamageCause::Collision) => format!("{victim_name} is gone... reduced to atoms..."),
        _ => format!("{victim_name} died"),
    };

    #[cfg(not(feature = "client"))]
    if let Some(mut quic) = world.get_resource_mut::<QuicManager>() {
        quic.send(SendTarget::All, Channel::Ordered, &MsgType::OnscreenMessage(text));
        return;
    }
    crate::messages::push_world(world, text);
}

fn player_name(world: &World, entity: Entity) -> String {
    world
        .get_resource::<super::PlayerRegistry>()
        .and_then(|registry| registry.conn_id_for_character(entity))
        .map(|conn_id| format!("Player {conn_id}"))
        .unwrap_or_else(|| "Player".to_string())
}

pub struct BipedPlugin;
impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.register_game_object::<BipedPawnComponent>().init_resource::<MouseSensitivity>();
        app.add_systems(FixedUpdate, update_slide_camera);
        #[cfg(feature = "client")]
        client::configure(app);
        app.add_systems(
            PostUpdate,
            preserve_look_across_body_rotation.before(TransformSystems::Propagate),
        );
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

pub fn apply_biped_input(
    world: &mut PhysicsWorld,
    owner: Entity,
    input: BipedInput,
    body_handle: &RigidBodyHandleComponent,
    biped: &mut BipedPawnComponent,
) -> Option<crate::pawn::biped_ability::AbilityFx> {
    biped.look_yaw = input.look_yaw;
    biped.look_pitch = input.look_pitch;
    apply_biped_movement(world, body_handle, input, biped);
    crate::pawn::biped_ability::apply_input(owner, input, world, biped)
}

pub fn biped_move_direction(body_rot: Quat, input: BipedInput) -> Vec3 {
    let facing = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward = facing * Vec3::NEG_Z;
    let right = facing * Vec3::X;
    (forward * input.forward + right * input.right).normalize_or_zero()
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
