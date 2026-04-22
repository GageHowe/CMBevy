use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_egui::input::EguiWantsInput;
use physics::physics_world::{PhysicsWorld, rb_pos};

use super::{
    BipedPawnComponent, CameraEffector, InteractionGate, InteractionHint, MouseSensitivity,
    PITCH_MAX, PitchPivot, Possessed, SeatedInVehicle, WeaponSlots, YawPivot, apply_biped_input,
    consume_fixed_press,
    vehicle::{DriverSeat, VehicleComponent, enter_vehicle, ray_hits_cockpit},
};
use crate::{
    GameObjectKind,
    weapon::{WeaponDriver, WeaponFireInput},
};

pub fn configure(app: &mut App) {
    app.init_resource::<FixedPressQueue>()
        .add_systems(Update, queue_fixed_inputs)
        .add_systems(
            FixedPreUpdate,
            (
                gather_biped_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(super::GatherInputSet),
                move_bipeds.in_set(super::MovePawnsSet),
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
        )
        .add_systems(
            PostUpdate,
            mouse_look
                .run_if(resource_exists::<AccumulatedMouseMotion>)
                .before(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            (
                reset_look_on_possess.before(attach_camera_on_possess),
                attach_camera_on_possess,
                hide_weapons_while_seated,
                switch_weapon_slot.run_if(resource_exists::<AccumulatedMouseScroll>),
            ),
        );
}

/// Gathers keyboard + look-pivot state into a BipedInput each FixedPreUpdate.
/// look_yaw/pitch are 1-frame stale (mouse_look runs in Update) — acceptable for movement.
fn gather_biped_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    mut fixed_presses: ResMut<FixedPressQueue>,
    mut pawns: Query<(&mut Possessed, &BipedPawnComponent)>,
    yaw_pivots: Query<&YawPivot>,
    pitch_pivots: Query<&PitchPivot>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        fixed_presses.clear_ability1();
        return;
    }
    let Ok((mut possessed, biped)) = pawns.single_mut() else {
        fixed_presses.clear_ability1();
        return;
    };

    let mut input = common::BipedInput::default();
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
    input.ability1_pressed = fixed_presses.consume_ability1();

    if let Some(yaw_e) = biped.yaw_pivot
        && let Ok(yp) = yaw_pivots.get(yaw_e)
    {
        input.look_yaw = yp.yaw;
    }
    if let Some(pitch_e) = biped.pitch_pivot
        && let Ok(pp) = pitch_pivots.get(pitch_e)
    {
        input.look_pitch = pp.pitch;
    }

    possessed.push(common::PawnInputKind::Biped(input));
}

/// moves the biped's yaw and pitch components on Update
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
    let zoom = camera_fx.single().map(|fx| fx.zoom_multiplier.max(1.0)).unwrap_or(1.0);
    let zoom_scale = 1.0 + (1.0 / zoom - 1.0) * sensitivity.zoom_blend;
    let s = sensitivity.base * zoom_scale;

    if let Some(yaw_e) = biped.yaw_pivot
        && let Ok((mut t, mut pivot)) = pivots.p0().get_mut(yaw_e)
    {
        pivot.yaw -= delta.x * s;
        t.rotation = Quat::from_rotation_y(pivot.yaw);
    }
    if let Some(pitch_e) = biped.pitch_pivot
        && let Ok((mut t, mut pivot)) = pivots.p1().get_mut(pitch_e)
    {
        pivot.pitch = (pivot.pitch - delta.y * s).clamp(-PITCH_MAX, PITCH_MAX);
        t.rotation = Quat::from_rotation_x(pivot.pitch);
    }
}

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

fn move_bipeds(
    mut world: ResMut<PhysicsWorld>,
    mut commands: Commands,
    mut pawns: Query<(
        Entity,
        &mut Possessed,
        &physics::physics_world::RigidBodyHandleComponent,
        &mut BipedPawnComponent,
    )>,
) {
    for (pawn_entity, mut possessed, handle, mut biped) in pawns.iter_mut() {
        let Some(common::PawnInputKind::Biped(input)) = possessed.consume() else {
            continue;
        };
        if let Some(fx) = apply_biped_input(&mut world, pawn_entity, input, handle, &mut biped) {
            crate::pawn::biped_ability::queue_fx(pawn_entity, fx, &world, &mut commands);
        }
    }
}

/// Re-parents the Camera3d under the biped's pitch pivot when Possessed is added.
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

fn reset_look_on_possess(
    mut bipeds: Query<&mut BipedPawnComponent, Added<Possessed>>,
    mut pivots: ParamSet<(
        Query<(&mut Transform, &mut YawPivot)>,
        Query<(&mut Transform, &mut PitchPivot)>,
    )>,
) {
    let Ok(mut biped) = bipeds.single_mut() else {
        return;
    };
    if let Some(yaw_e) = biped.yaw_pivot
        && let Ok((mut t, mut pivot)) = pivots.p0().get_mut(yaw_e)
    {
        pivot.yaw = 0.0;
        t.rotation = Quat::IDENTITY;
    }
    if let Some(pitch_e) = biped.pitch_pivot
        && let Ok((mut t, mut pivot)) = pivots.p1().get_mut(pitch_e)
    {
        pivot.pitch = 0.0;
        t.rotation = Quat::IDENTITY;
    }
    biped.look_yaw = 0.0;
    biped.look_pitch = 0.0;
    biped.last_look_frame_body_rot = None;
}

fn hide_weapons_while_seated(
    seated: Query<&WeaponSlots, Added<SeatedInVehicle>>,
    mut commands: Commands,
) {
    for slots in seated.iter() {
        for weapon in slots.held_entities() {
            commands.entity(weapon).insert(Visibility::Hidden);
        }
    }
}

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
    if let Ok(biped) = possessed_q.single()
        && let Some(light) = biped.flashlight
        && let Ok(mut vis) = lights.get_mut(light)
    {
        *vis = if *on { Visibility::Inherited } else { Visibility::Hidden };
    }
    if quic.client_connected {
        quic.send_to_server(net::quic::Channel::Ordered, &net::message::MsgType::FlashlightToggle);
    }
}

#[derive(Resource, Default)]
struct FixedPressQueue {
    reload: bool,
    ability1: bool,
}

impl FixedPressQueue {
    fn queue_reload(&mut self) {
        self.reload = true;
    }

    fn queue_ability1(&mut self) {
        self.ability1 = true;
    }

    fn consume_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload)
    }

    fn consume_ability1(&mut self) -> bool {
        std::mem::take(&mut self.ability1)
    }

    fn clear_ability1(&mut self) {
        self.ability1 = false;
    }
}

fn queue_fixed_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    egui_wants: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    mut fixed_presses: ResMut<FixedPressQueue>,
) {
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
    if !blocked && bindings.just_pressed(common::InputAction::Reload, &keyboard, &mouse) {
        fixed_presses.queue_reload();
    }
    if !blocked && bindings.just_pressed(common::InputAction::Ability1, &keyboard, &mouse) {
        fixed_presses.queue_ability1();
    }
}

fn biped_fire(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    egui_wants: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveKeyBindings>,
    mut pawn: Query<(Entity, &mut WeaponSlots, &BipedPawnComponent), With<Possessed>>,
    pitch_pivot: Query<&GlobalTransform, With<PitchPivot>>,
    drivers: Query<&WeaponDriver>,
    weapon_states: Query<&crate::weapon::WeaponState>,
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    mut quic: Option<ResMut<net::quic::QuicManager>>,
    mut fixed_presses: ResMut<FixedPressQueue>,
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
    let reload_pressed = !blocked && fixed_presses.consume_reload();
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

#[derive(bevy::ecs::system::SystemParam)]
struct InteractInputParams<'w> {
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    egui_wants: Option<Res<'w, EguiWantsInput>>,
    bindings: Res<'w, common::ActiveKeyBindings>,
    ticker: Res<'w, common::tick::Ticker>,
    interaction: ResMut<'w, InteractionGate>,
}

enum InteractTarget {
    Vehicle { cockpit_entity: Entity, vehicle_entity: Entity },
    Entity { hit_entity: Entity, net_id: Option<net::message::NetworkID> },
}

fn format_interaction_prompt(key: &str, verb: &str, kind: GameObjectKind) -> String {
    format!("Press {key} to {verb} {}", kind.interaction_name())
}

fn current_interact_target(
    pawn_entity: Entity,
    origin: Vec3,
    forward: Vec3,
    world: &PhysicsWorld,
    interactables: &Query<
        (Option<&net::message::NetworkID>, &crate::interaction::Interactable),
        With<crate::interaction::Interactable>,
    >,
    cockpits: &Query<(Entity, &DriverSeat, &GlobalTransform, &ChildOf)>,
) -> Option<InteractTarget> {
    let mut cockpit_target = None;
    for (cockpit_entity, cockpit, cockpit_gt, child_of) in cockpits.iter() {
        let (_, _, seat_center) = cockpit_gt.to_scale_rotation_translation();
        let Some(distance) = ray_hits_cockpit(
            origin,
            forward,
            cockpit.interact_radius + 4.0,
            seat_center,
            cockpit.interact_radius,
        ) else {
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

    let (hit_entity, _distance) = world.cast_ray(origin, forward, 4.0, &[pawn_entity])?;
    let (net_id, interactable) = interactables.get(hit_entity).ok()?;
    if !interactable_in_range(world, pawn_entity, hit_entity, interactable.range) {
        return None;
    }
    Some(InteractTarget::Entity { hit_entity, net_id: net_id.cloned() })
}

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

fn update_interaction_hint(
    egui_wants: Option<Res<EguiWantsInput>>,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    bindings: Res<common::ActiveKeyBindings>,
    interactables: Query<
        (Option<&net::message::NetworkID>, &crate::interaction::Interactable),
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

fn interact(
    state: Res<State<common::game_state::GameState>>,
    mut input: InteractInputParams,
    player: Query<(Entity, &BipedPawnComponent), With<Possessed>>,
    interactables: Query<
        (Option<&net::message::NetworkID>, &crate::interaction::Interactable),
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
                commands.entity(pawn_entity).insert(SeatedInVehicle(vehicle_entity));
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
                match state.get() {
                    GameState::SinglePlayer => f(pawn_entity, hit_entity, &mut commands),
                    GameState::Multiplayer => {
                        if let Some(interact_net_id) = interact_net_id {
                            quic.send_to_server(
                                net::quic::Channel::Ordered,
                                &net::message::MsgType::Interact(interact_net_id),
                            );
                        }
                    }
                    _ => {}
                }
                return;
            }

            match state.get() {
                GameState::SinglePlayer => {
                    let Some(interact_net_id) = interact_net_id else {
                        return;
                    };
                    let Ok(mut slots) = possessed_q.single_mut() else {
                        return;
                    };
                    if slots.is_full()
                        && let Some((_drop_id, drop_entity)) = slots.remove_active()
                    {
                        let drop_velocity = forward * 8.0
                            + crate::projectile::helpers::shooter_velocity(
                                &world,
                                Some(pawn_entity),
                            );
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
                    crate::weapon::helpers::sync_local_active_weapon(
                        &mut commands,
                        &slots,
                        &mut camera_fx,
                    );
                    if let Ok(kind) = object_kinds.get(hit_entity) {
                        crate::messages::push(
                            &mut commands,
                            format!("Picked up {}", kind.interaction_name()),
                        );
                    }
                }
                GameState::Multiplayer => {
                    if let Some(interact_net_id) = interact_net_id {
                        quic.send_to_server(
                            net::quic::Channel::Ordered,
                            &net::message::MsgType::Interact(interact_net_id),
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

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
