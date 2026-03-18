use crate::game_objects::{health::Health, GameObject};
use crate::game_objects::weapon::{rifle, shotgun, hail_mary};
use crate::game_objects::weapon::weapon::Weapon;
use crate::net::message::{NetworkID, SpawnCommand};
use crate::physics::physics_world::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use rapier3d::prelude::*;
use super::pawn::*;

#[derive(Component, Default)]
pub struct BipedPawnComponent {
    pub flashlight_on: bool,
    /// handle to the foot-sphere rigid body. None until physics are inserted.
    pub foot_sphere: Option<RigidBodyHandle>,
    /// ticks remaining before another jump is allowed.
    pub jump_cooldown: u8,
    /// look yaw/pitch (radians). Set from input; used for server-side movement simulation.
    pub look_yaw: f32,
    pub look_pitch: f32,
    /// cached pivot entities set by setup_camera_rig; None on the server.
    pub yaw_pivot: Option<Entity>,
    pub pitch_pivot: Option<Entity>,
}

pub struct BipedPlugin;

impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_remove_biped);
        app.init_resource::<MouseSensitivity>();
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

pub fn on_remove_biped(
    trigger: On<Remove, BipedPawnComponent>,
    bipeds: Query<&BipedPawnComponent>,
    mut world: ResMut<PhysicsWorld>,
) {
    let Ok(biped) = bipeds.get(trigger.entity) else { return };
    let Some(sphere_handle) = biped.foot_sphere else { return };
    let PhysicsWorld { rigid_body_set, island_manager, collider_set, impulse_joint_set, multibody_joint_set, .. } = &mut *world;
    rigid_body_set.remove(sphere_handle, island_manager, collider_set, impulse_joint_set, multibody_joint_set, true);
}

/// Two weapon slots on a biped pawn. Stored on the entity, not globally.
/// Each slot holds the NetworkID and (client-only) the local weapon entity for the viewmodel.
#[derive(Component, Default)]
pub struct WeaponSlots {
    pub slots: [(Option<NetworkID>, Option<Entity>); 2],
    pub active: usize,
}

const SPHERE_RADIUS: f32 = 0.25;
/// offset from capsule center to its bottom (half-height + radius)
const CAPSULE_BOTTOM: f32 = 0.8;

fn insert_biped_physics(entity: Entity, transform: &Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> RigidBodyHandle {
    let capsule_rb = RigidBodyBuilder::dynamic()
        .translation(transform.translation)
        .angular_damping(10.0)
        .lock_rotations()
        .build();
    let rb_handle = world.insert_body(entity, capsule_rb);
    let player_groups = InteractionGroups::new(GROUP_PLAYER, Group::ALL & !GROUP_PROJECTILE, InteractionTestMode::And);
    let capsule_collider = ColliderBuilder::capsule_y(0.5, 0.3)
        .friction(0.0)
        .restitution(0.0)
        .restitution_combine_rule(CoefficientCombineRule::Min)
        .collision_groups(player_groups)
        .solver_groups(player_groups)
        .build();
    commands.entity(entity).insert(RigidBodyHandleComponenet(rb_handle));
    {
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        collider_set.insert_with_parent(capsule_collider, rb_handle, rigid_body_set);
    }

    // foot sphere: high friction, zero external torque influence (overridden each tick)
    let sphere_pos = transform.translation - Vec3::Y * CAPSULE_BOTTOM;
    let sphere_rb = RigidBodyBuilder::dynamic()
        .translation(sphere_pos)
        .lock_rotations()
        .gravity_scale(0.0)
        .build();
    let sphere_handle = world.rigid_body_set.insert(sphere_rb);
    {
        let PhysicsWorld { collider_set, rigid_body_set, .. } = &mut *world;
        let sphere_collider = ColliderBuilder::ball(SPHERE_RADIUS)
            .friction(3.0)
            .restitution(0.0)
            .restitution_combine_rule(CoefficientCombineRule::Min)
            .collision_groups(player_groups)
            .solver_groups(player_groups)
            .build();
        collider_set.insert_with_parent(sphere_collider, sphere_handle, rigid_body_set);
    }

    // SphericalJoint: anchors at capsule bottom (body1) and sphere center (body2),
    // contacts disabled so capsule and sphere don't collide with each other
    let joint = SphericalJointBuilder::new()
        .local_anchor1(Vector::new(0.0, -CAPSULE_BOTTOM, 0.0))
        .local_anchor2(Vector::new(0.0, 0.0, 0.0))
        .contacts_enabled(false)
        .build();
    world.impulse_joint_set.insert(rb_handle, sphere_handle, joint, true);

    sphere_handle
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
            intensity: 2_000_000.0,
            range: 30.0,
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
    owned: bool,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &mut PhysicsWorld,
    camera: Option<Entity>,
) -> Entity {
    let transform = Transform {
        translation: cmd.position.into(),
        rotation: cmd.rotation.into(),
        ..default()
    };
    let entity = BipedPawnComponent::spawn_physics(transform, commands, world);
    let color = if owned { Color::srgb(0.8, 0.8, 0.8) } else { Color::srgb(0.9, 0.4, 0.1) };
    let light = add_visuals(entity, color, commands, meshes, materials);
    let cam = if owned { camera } else { None };
    let (yaw_pivot, pitch_pivot) = setup_camera_rig(entity, cam, light, commands);
    commands.entity(entity).insert(cmd.net_id.clone());
    commands.queue(move |world: &mut World| {
        if let Some(mut biped) = world.entity_mut(entity).get_mut::<BipedPawnComponent>() {
            biped.yaw_pivot = Some(yaw_pivot);
            biped.pitch_pivot = Some(pitch_pivot);
        }
    });
    entity
}

impl Pawn for BipedPawnComponent {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponenet, input: PawnInput) {
        apply_biped_movement(world, body, input, self);
    }
}

impl GameObject for BipedPawnComponent {
    fn spawn_physics(transform: Transform, commands: &mut Commands, world: &mut PhysicsWorld) -> Entity {
        let entity = commands.spawn((
            WeaponSlots::default(),
            Health::new(100.0),
            Transform::from(transform),
        )).id();
        let sphere_handle = insert_biped_physics(entity, &transform, commands, world);
        commands.entity(entity).insert(BipedPawnComponent { foot_sphere: Some(sphere_handle), ..default() });
        entity
    }
    fn cleanup() {}
    fn get_rigidbody() -> Option<RigidBody> {
        Some(RigidBodyBuilder::dynamic().angular_damping(10.0).lock_rotations().build())
    }
}

pub fn draw_biped_debug(
    world: Res<PhysicsWorld>,
    bipeds: Query<(&BipedPawnComponent, &RigidBodyHandleComponenet)>,
    mut gizmos: Gizmos,
) {
    use crate::physics::debug::{draw_collider, rb_iso};
    for (biped, body_handle) in bipeds.iter() {
        let Some(sphere_handle) = biped.foot_sphere else { continue };

        if let Some(rb) = world.rigid_body_set.get(body_handle.0) {
            let iso = rb_iso(rb);
            for ch in rb.colliders() {
                if let Some(col) = world.collider_set.get(*ch) {
                    draw_collider(col, iso, Color::srgba(0.3, 0.6, 1.0, 0.1), &mut gizmos);
                }
            }
        }

        if let Some(rb) = world.rigid_body_set.get(sphere_handle) {
            let iso = rb_iso(rb);
            for ch in rb.colliders() {
                if let Some(col) = world.collider_set.get(*ch) {
                    draw_collider(col, iso, Color::srgba(0.2, 0.9, 0.3, 0.1), &mut gizmos);
                }
            }
            if let Some(cap_rb) = world.rigid_body_set.get(body_handle.0) {
                let cap_t = cap_rb.position().translation;
                gizmos.line(Vec3::new(cap_t.x, cap_t.y, cap_t.z), iso.translation.into(), Color::srgba(0.2, 0.9, 0.3, 0.4));
            }
        }
    }
}

const WALK_ANGULAR:   f32 = 80.0;   // rad/s → friction drives capsule ~2 m/s
const SPRINT_ANGULAR: f32 = 160.0;
const JUMP_IMPULSE:      f32 = 30.0;
const AIR_CONTROL:       f32 = 0.5;
const GROUND_DIST:    f32 = 0.01;  // must be nearly touching to count as grounded
const JUMP_COOLDOWN:  u8  = 25;    // ticks (~0.4 s at 60 Hz) before another jump

pub fn apply_biped_movement(
    world: &mut PhysicsWorld,
    body_handle: &RigidBodyHandleComponenet,
    input: PawnInput,
    biped: &mut BipedPawnComponent,
) {
    let Some(sphere_handle) = biped.foot_sphere else { return };

    // --- read phase ---
    let (body_rot, capsule_mass) = {
        let Some(body) = world.rigid_body_set.get(body_handle.0) else { return };
        let r = body.rotation();
        (Quat::from_xyzw(r.x, r.y, r.z, r.w), body.mass())
    };

    let planet_up = body_rot * Vec3::Y;
    let facing    = body_rot * Quat::from_rotation_y(input.look_yaw);
    let forward   = facing * Vec3::NEG_Z;
    let right     = facing * Vec3::X;

    let is_jump   = input.up > 0.5;
    let is_slide  = input.up < -0.5;
    let is_sprint = input.ability1;

    // grounded check: ray downward from sphere center
    let grounded = {
        let sphere_t = {
            let Some(rb) = world.rigid_body_set.get(sphere_handle) else { return };
            rb.position().translation
        };
        let capsule_handle = body_handle.0;
        let exclude = |_ch: ColliderHandle, col: &rapier3d::prelude::Collider| {
            col.parent().map_or(true, |rb| rb != capsule_handle && rb != sphere_handle)
        };
        let filter = QueryFilter::new().predicate(&exclude);
        let qp = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            filter,
        );
        let ray = Ray::new(
            Vec3::new(sphere_t.x, sphere_t.y, sphere_t.z),
            -planet_up,
        );
        qp.cast_ray(&ray, SPHERE_RADIUS + GROUND_DIST, true).is_some()
    };

    // --- write phase ---

    let sphere_collider_h = world.rigid_body_set.get(sphere_handle)
        .and_then(|rb| rb.colliders().first().copied());

    if grounded {
        // sphere angular velocity drives friction-based movement
        let desired = forward * input.forward + right * input.right;
        let speed = if is_sprint && input.forward >= 0.0 { SPRINT_ANGULAR } else { WALK_ANGULAR };
        let angvel = if !is_slide && desired.length_squared() > 1e-6 {
            let axis = planet_up.cross(desired.normalize());
            axis * speed
        } else {
            Vec3::ZERO
        };

        // disable sphere when sliding so the capsule's zero-friction collider takes over
        let was_sliding = sphere_collider_h
            .and_then(|ch| world.collider_set.get(ch))
            .map(|col| !col.is_enabled())
            .unwrap_or(false);
        if let Some(ch) = sphere_collider_h {
            if let Some(col) = world.collider_set.get_mut(ch) {
                col.set_enabled(!is_slide);
            }
        }

        let capsule_linvel = world.rigid_body_set.get(body_handle.0)
            .map(|rb| { let v = rb.linvel(); Vector::new(v.x, v.y, v.z) })
            .unwrap_or(Vector::ZERO);

        if let Some(rb) = world.rigid_body_set.get_mut(sphere_handle) {
            if is_slide || (was_sliding && !is_slide) {
                rb.set_linvel(capsule_linvel, false);
            }
            rb.set_angvel(Vector::new(angvel.x, angvel.y, angvel.z), true);
        }
    } else {
        // re-enable sphere collider in case we left the ground while sliding
        if let Some(ch) = sphere_collider_h {
            if let Some(col) = world.collider_set.get_mut(ch) {
                col.set_enabled(true);
            }
        }
        if let Some(rb) = world.rigid_body_set.get_mut(sphere_handle) {
            rb.set_angvel(Vector::ZERO, true);
        }
    }

    biped.jump_cooldown = biped.jump_cooldown.saturating_sub(1);

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
    mut quic: Option<ResMut<crate::net::quic::QuicManager>>,
    ticker: Res<crate::tick::Ticker>,
) {
    let want_fire = !egui_wants.map_or(false, |e| e.wants_any_input()) && mouse.pressed(MouseButton::Left);
    let Ok((pawn_entity, slots, biped)) = pawn.single() else { return };
    let Some(weapon_entity) = slots.slots[slots.active].1 else { return };
    let Ok(mut weapon) = weapons.get_mut(weapon_entity) else { return };
    let Some(pitch_e) = biped.pitch_pivot else { return };
    let Ok(gt) = pitch_pivot.get(pitch_e) else { return };
    let (_, rotation, origin) = gt.to_scale_rotation_translation();
    let aim_dir = rotation * Vec3::NEG_Z;
    if weapon.update(&mut world, &mut commands, origin, aim_dir, Some(pawn_entity), ticker.tick, want_fire) {
        if let (Some(quic), Ok(net_id)) = (quic.as_mut(), net_ids.get(weapon_entity)) {
            quic.send(crate::net::quic::SendTarget::All, crate::net::quic::Channel::Unordered,
                &crate::net::message::MsgType::Fire(net_id.clone(), origin.into(), aim_dir.into(), ticker.tick));
        }
    }
}
