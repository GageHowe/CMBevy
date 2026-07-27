use bevy::{
    input::{
        gamepad::Gamepad,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    },
    prelude::*,
    window::*,
};
use bevy_egui::input::EguiWantsInput;
use physics::physics_world::PhysicsWorld;

use super::{
    mount::{CharacterMount, Mounted, ray_hits_mount},
    *,
};
use crate::{interaction::InteractionName, weapon::WeaponFireInput};

pub(super) fn configure(app: &mut App) {
    app.init_resource::<FixedPressQueue>()
        .add_systems(
            Update,
            queue_fixed_inputs.in_set(common::game_state::SimulationSystems),
        )
        .add_systems(
            FixedPreUpdate,
            (
                gather_biped_input
                    .run_if(resource_exists::<ButtonInput<KeyCode>>)
                    .in_set(super::GatherInputSet),
                biped_fire
                    .run_if(resource_exists::<ButtonInput<MouseButton>>)
                    .in_set(super::GatherInputSet),
                drop_active_weapon.run_if(resource_exists::<ButtonInput<KeyCode>>),
                update_interaction_hint.run_if(resource_exists::<ButtonInput<KeyCode>>),
                interact
                    .run_if(
                        in_state(common::game_state::GameState::SinglePlayer)
                            .or_else(in_state(common::game_state::GameState::Multiplayer)),
                    )
                    .run_if(resource_exists::<ButtonInput<KeyCode>>),
            )
                .chain()
                .in_set(common::game_state::SimulationSystems),
        )
        .add_systems(
            PostUpdate,
            mouse_look
                .in_set(common::game_state::SimulationSystems)
                .run_if(resource_exists::<AccumulatedMouseMotion>)
                .before(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            (
                reset_look_on_possess.before(attach_camera_on_possess),
                attach_camera_on_possess,
                clear_zoom_without_active_weapon,
                hide_weapons_while_seated,
                switch_weapon_slot.run_if(resource_exists::<AccumulatedMouseScroll>),
            )
                .in_set(common::game_state::SimulationSystems),
        );
}

fn clear_zoom_without_active_weapon(
    pawn: Query<&WeaponSlots, With<Controller>>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
) {
    let Ok(slots) = pawn.single() else {
        return;
    };
    if slots.active().1.is_some() {
        return;
    }
    if let Ok(mut camera) = camera.single_mut() {
        camera.reset_zoom();
    }
}

fn gather_biped_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    sensitivity: Res<MouseSensitivity>,
    mut fixed_presses: ResMut<FixedPressQueue>,
    pawns: Query<&BipedPawnComponent, With<Controller>>,
    mut control: ResMut<common::LocalControl>,
    yaw_pivots: Query<&YawPivot>,
    pitch_pivots: Query<&PitchPivot>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        fixed_presses.clear_ability1();
        fixed_presses.clear_melee();
        return;
    }
    let Ok(biped) = pawns.single() else {
        fixed_presses.clear_ability1();
        fixed_presses.clear_melee();
        return;
    };
    let gamepad = common::active_gamepad(gamepads.iter());
    let move_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.left_stick(), sensitivity.gamepad_move_deadzone)
        })
        .unwrap_or(Vec2::ZERO);
    let mut input = common::BipedInput::default();
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
    input.jump = bindings.pressed(
        common::InputAction::Jump,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    input.slide = bindings.pressed(
        common::InputAction::Crouch,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    input.ability1 = bindings.pressed(
        common::InputAction::Ability1,
        &keyboard,
        &mouse_buttons,
        gamepad,
    );
    input.ability1_pressed = fixed_presses.consume_ability1();
    input.melee_pressed = fixed_presses.consume_melee();
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
    control.push(input);
}

fn mouse_look(
    time: Res<Time>,
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    gamepads: Query<&Gamepad>,
    cursor_q: Single<&CursorOptions, With<PrimaryWindow>>,
    possessed: Query<&BipedPawnComponent, With<Controller>>,
    camera_fx: Query<&CameraEffector, With<Camera3d>>,
    mut pivots: ParamSet<(
        Query<(&mut Transform, &mut YawPivot)>,
        Query<(&mut Transform, &mut PitchPivot)>,
    )>,
) {
    let gamepad = common::active_gamepad(gamepads.iter());
    let look_stick = gamepad
        .map(|gamepad| {
            common::stick_with_deadzone(gamepad.right_stick(), sensitivity.gamepad_look_deadzone)
        })
        .unwrap_or(Vec2::ZERO);
    if cursor_q.grab_mode == CursorGrabMode::None
        || (mouse.delta == Vec2::ZERO && look_stick == Vec2::ZERO)
    {
        return;
    }
    let Ok(biped) = possessed.single() else {
        return;
    };
    let zoom = camera_fx
        .single()
        .map(|fx| fx.zoom_multiplier.max(1.0))
        .unwrap_or(1.0);
    let zoom_scale = 1.0 + (1.0 / zoom - 1.0) * sensitivity.zoom_blend;
    let s = sensitivity.base * zoom_scale;
    let gamepad_delta = Vec2::new(
        look_stick.x * sensitivity.gamepad_look * time.delta_secs(),
        look_stick.y
            * sensitivity.gamepad_look
            * time.delta_secs()
            * if sensitivity.gamepad_invert_y {
                -1.0
            } else {
                1.0
            },
    );
    let look_delta = Vec2::new(
        mouse.delta.x * s + gamepad_delta.x,
        mouse.delta.y * s - gamepad_delta.y,
    );
    if let Some(yaw_e) = biped.yaw_pivot
        && let Ok((mut t, mut pivot)) = pivots.p0().get_mut(yaw_e)
    {
        pivot.yaw -= look_delta.x;
        t.rotation = Quat::from_rotation_y(pivot.yaw);
    }
    if let Some(pitch_e) = biped.pitch_pivot
        && let Ok((mut t, mut pivot)) = pivots.p1().get_mut(pitch_e)
    {
        pivot.pitch = (pivot.pitch - look_delta.y).clamp(-PITCH_MAX, PITCH_MAX);
        t.rotation = Quat::from_rotation_x(pivot.pitch);
    }
}

fn switch_weapon_slot(
    scroll: Res<AccumulatedMouseScroll>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
    mut pawn: Query<&mut WeaponSlots, With<Controller>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut camera: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    state: Res<State<common::game_state::GameState>>,
    mut quic: ResMut<crate::net::quic::QuicManager>,
) {
    if scroll.delta.y == 0.0 || egui_wants_input.map_or(false, |e| e.wants_any_input()) {
        return;
    }
    let Ok(mut slots) = pawn.single_mut() else {
        return;
    };
    let old_active_primary = slots.active_primary();
    let old_active_weapon = slots.active().1;
    let switched = if scroll.delta.y > 0.0 {
        slots.next_weapon()
    } else {
        slots.prev_weapon()
    };
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
            crate::net::quic::Channel::Ordered,
            &crate::net::message::MsgType::SetActiveWeaponSlot(
                crate::net::message::SetActiveWeaponSlot(slots.active_primary()),
            ),
        );
    }
}

fn attach_camera_on_possess(
    bipeds: Query<(&BipedPawnComponent, &WeaponSlots), Added<Controller>>,
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
    let base_fov = if let Projection::Perspective(p) = proj {
        p.fov.to_degrees()
    } else {
        90.0
    };
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
    mut bipeds: Query<&mut BipedPawnComponent, Added<Controller>>,
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
}

fn hide_weapons_while_seated(seated: Query<&WeaponSlots, Added<Mounted>>, mut commands: Commands) {
    for slots in seated.iter() {
        for weapon in slots.held_entities() {
            commands.entity(weapon).insert(Visibility::Hidden);
        }
    }
}

#[derive(Resource, Default)]
struct FixedPressQueue {
    fire: bool,
    reload: bool,
    alt_fire: bool,
    ability1: bool,
    melee: bool,
}

impl FixedPressQueue {
    fn consume_fire(&mut self) -> bool {
        std::mem::take(&mut self.fire)
    }
    fn queue_reload(&mut self) {
        self.reload = true;
    }
    fn queue_alt_fire(&mut self) {
        self.alt_fire = true;
    }
    fn queue_ability1(&mut self) {
        self.ability1 = true;
    }
    fn queue_melee(&mut self) {
        self.melee = true;
    }
    fn consume_reload(&mut self) -> bool {
        std::mem::take(&mut self.reload)
    }
    fn consume_alt_fire(&mut self) -> bool {
        std::mem::take(&mut self.alt_fire)
    }
    fn consume_ability1(&mut self) -> bool {
        std::mem::take(&mut self.ability1)
    }
    fn consume_melee(&mut self) -> bool {
        std::mem::take(&mut self.melee)
    }
    fn clear_ability1(&mut self) {
        self.ability1 = false;
    }
    fn clear_melee(&mut self) {
        self.melee = false;
    }
}

fn queue_fixed_inputs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    egui_wants: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut fixed_presses: ResMut<FixedPressQueue>,
) {
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
    let gamepad = common::active_gamepad(gamepads.iter());
    if !blocked && bindings.just_pressed(common::InputAction::Fire, &keyboard, &mouse, gamepad) {
        fixed_presses.fire = true;
    }
    if !blocked && bindings.just_pressed(common::InputAction::Reload, &keyboard, &mouse, gamepad) {
        fixed_presses.queue_reload();
    }
    if !blocked && bindings.just_pressed(common::InputAction::AltFire, &keyboard, &mouse, gamepad) {
        fixed_presses.queue_alt_fire();
    }
    if !blocked && bindings.just_pressed(common::InputAction::Ability1, &keyboard, &mouse, gamepad)
    {
        fixed_presses.queue_ability1();
    }
    if !blocked && bindings.just_pressed(common::InputAction::Melee, &keyboard, &mouse, gamepad) {
        fixed_presses.queue_melee();
    }
}

fn biped_fire(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    egui_wants: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    mut pawn: Query<
        (
            Entity,
            &mut WeaponSlots,
            &BipedPawnComponent,
            &physics::physics_world::RigidBodyHandleComponent,
        ),
        With<Controller>,
    >,
    world: Res<PhysicsWorld>,
    camera_gt: Query<&GlobalTransform, With<Camera3d>>,
    weapon_states: Query<&crate::weapon::WeaponState>,
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    mut fixed_presses: ResMut<FixedPressQueue>,
    ticker: Res<common::tick::Ticker>,
    mut control: ResMut<common::LocalControl>,
) {
    let blocked = egui_wants.is_some_and(|e| e.wants_any_input());
    let gamepad = common::active_gamepad(gamepads.iter());
    let want_fire =
        !blocked && bindings.pressed(common::InputAction::Fire, &keyboard, &mouse, gamepad);
    let Ok((pawn_entity, mut slots, biped, body_handle)) = pawn.single_mut() else {
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
    if weapon_states.get(weapon_entity).is_err() {
        let Some((_weapon_id, _removed_weapon_entity)) = slots.remove_active() else {
            return;
        };
        crate::weapon::helpers::sync_local_active_weapon(&mut commands, &slots, &mut camera_fx);
        return;
    }
    let Some((origin, fallback_aim_dir)) =
        super::aim_pose(&world, body_handle, biped.look_yaw, biped.look_pitch)
    else {
        return;
    };
    // Keep projectile spawn anchored to Rapier, but let the local camera rotation steer aim so
    // recoil / camera kick still affects shots. Do not derive origin from camera/pivot transforms.
    let aim_dir = camera_gt
        .single()
        .map(|gt| gt.to_scale_rotation_translation().1 * Vec3::NEG_Z)
        .unwrap_or(fallback_aim_dir);
    let fire_pressed = !blocked && fixed_presses.consume_fire();
    let reload_pressed = !blocked && fixed_presses.consume_reload();
    let alt_fire_pressed = !blocked && fixed_presses.consume_alt_fire();
    let input = WeaponFireInput {
        want_fire,
        fire_pressed,
        want_alt_fire: !blocked
            && bindings.pressed(common::InputAction::AltFire, &keyboard, &mouse, gamepad),
        alt_fire_pressed,
        reload_pressed,
        origin,
        aim_dir,
        shooter: pawn_entity,
        tick: ticker.tick,
        prediction_id: ticker.tick as u32,
    };
    if let Some(biped_input) = control.newest_mut() {
        biped_input.item = common::ItemInput {
            weapon: slots.active().0.as_ref().map(|id| id.0),
            primary: input.want_fire,
            primary_pressed: input.fire_pressed,
            secondary: input.want_alt_fire,
            secondary_pressed: input.alt_fire_pressed,
            reload_pressed: input.reload_pressed,
            tick: input.tick,
            origin: input.origin,
            aim_dir: input.aim_dir,
        };
    }
    commands.entity(weapon_entity).insert(input);
}

enum InteractTarget {
    Mount(Entity),
    Entity {
        hit_entity: Entity,
        net_id: Option<crate::net::message::NetworkID>,
    },
}

#[derive(bevy::ecs::system::SystemParam)]
struct InteractWorldParams<'w, 's> {
    interactables: Query<
        'w,
        's,
        (
            Option<&'static crate::net::message::NetworkID>,
            &'static crate::interaction::Interactable,
        ),
        With<crate::interaction::Interactable>,
    >,
    possessed_q: Query<'w, 's, &'static mut WeaponSlots, With<Controller>>,
    weapon_states: Query<'w, 's, &'static mut crate::weapon::WeaponState>,
    camera_fx: Query<'w, 's, &'static mut CameraEffector, With<Camera3d>>,
    mount_net_ids: Query<'w, 's, &'static crate::net::message::NetworkID, With<CharacterMount>>,
    interaction_names: Query<'w, 's, &'static InteractionName>,
    ability_pickups: Query<'w, 's, &'static crate::pawn::biped_ability::AbilityPickup>,
    mounts: ParamSet<
        'w,
        's,
        (
            Query<'w, 's, (Entity, &'static CharacterMount)>,
            Query<'w, 's, &'static mut CharacterMount>,
        ),
    >,
    mount_anchor_visuals: Query<'w, 's, &'static GlobalTransform>,
    mount_anchor_transforms: Query<'w, 's, &'static Transform>,
    commands: Commands<'w, 's>,
}

fn format_interaction_prompt(key: &str, verb: &str, name: &InteractionName) -> String {
    format!("Press {key} to {verb} {}", name.0)
}

fn current_interact_target(
    pawn_entity: Entity,
    origin: Vec3,
    forward: Vec3,
    world: &PhysicsWorld,
    interactables: &Query<
        (
            Option<&crate::net::message::NetworkID>,
            &crate::interaction::Interactable,
        ),
        With<crate::interaction::Interactable>,
    >,
    mounts: &Query<(Entity, &CharacterMount)>,
    mount_anchors: &Query<&GlobalTransform>,
) -> Option<InteractTarget> {
    let mut mount_target = None;
    for (parent_entity, mount) in mounts.iter() {
        let Ok(anchor_gt) = mount_anchors.get(mount.anchor) else {
            continue;
        };
        let (_, _, mount_center) = anchor_gt.to_scale_rotation_translation();
        let Some(distance) = ray_hits_mount(
            origin,
            forward,
            mount.interact_radius + 4.0,
            mount_center,
            mount.interact_radius,
        ) else {
            continue;
        };
        if mount.occupant.is_some() {
            continue;
        }
        let target = (distance, parent_entity);
        if mount_target.is_none_or(|best: (f32, Entity)| distance < best.0) {
            mount_target = Some(target);
        }
    }
    if let Some((_, parent_entity)) = mount_target {
        return Some(InteractTarget::Mount(parent_entity));
    }

    let (hit_entity, _distance) =
        world.cast_ray_ignoring_shields(origin, forward, 4.0, &[pawn_entity])?;
    let (net_id, interactable) = interactables.get(hit_entity).ok()?;
    if !interactable_in_range(world, pawn_entity, hit_entity, interactable.range) {
        return None;
    }
    Some(InteractTarget::Entity {
        hit_entity,
        net_id: net_id.cloned(),
    })
}

fn interactable_in_range(
    world: &PhysicsWorld,
    pawn_entity: Entity,
    target_entity: Entity,
    range: f32,
) -> bool {
    world.entities_within_range(pawn_entity, target_entity, range)
}

fn update_interaction_hint(
    egui_wants: Option<Res<EguiWantsInput>>,
    player: Query<
        (
            Entity,
            &BipedPawnComponent,
            &physics::physics_world::RigidBodyHandleComponent,
        ),
        With<Controller>,
    >,
    bindings: Res<common::ActiveBindings>,
    prompt_device: Option<Res<common::PromptDevicePreference>>,
    interactables: Query<
        (
            Option<&crate::net::message::NetworkID>,
            &crate::interaction::Interactable,
        ),
        With<crate::interaction::Interactable>,
    >,
    world: Res<PhysicsWorld>,
    interaction_names: Query<&InteractionName>,
    weapon_q: Query<(), With<crate::weapon::WeaponComponent>>,
    pickup_q: Query<(), With<crate::pawn::biped_ability::AbilityPickup>>,
    mount_q: Query<(Entity, &CharacterMount)>,
    mount_anchor_q: Query<&GlobalTransform>,
    mut hint: ResMut<InteractionHint>,
) {
    if egui_wants.as_ref().is_some_and(|e| e.wants_any_input()) {
        hint.0 = None;
        return;
    }
    let Ok((pawn_entity, biped, body_handle)) = player.single() else {
        hint.0 = None;
        return;
    };
    let Some((origin, forward)) =
        super::aim_pose(&world, body_handle, biped.look_yaw, biped.look_pitch)
    else {
        hint.0 = None;
        return;
    };
    let Some(target) = current_interact_target(
        pawn_entity,
        origin,
        forward,
        &world,
        &interactables,
        &mount_q,
        &mount_anchor_q,
    ) else {
        hint.0 = None;
        return;
    };
    let key = bindings.prompt_label_for(
        common::InputAction::Interact,
        prompt_device.map_or(common::PromptDeviceMode::Both, |mode| mode.0),
    );
    hint.0 = match target {
        InteractTarget::Mount(parent_entity) => interaction_names
            .get(parent_entity)
            .ok()
            .map(|name| format_interaction_prompt(&key, "enter", name)),
        InteractTarget::Entity { hit_entity, .. } if weapon_q.contains(hit_entity) => {
            interaction_names
                .get(hit_entity)
                .ok()
                .map(|name| format_interaction_prompt(&key, "equip", name))
        }
        InteractTarget::Entity { hit_entity, .. } if pickup_q.contains(hit_entity) => {
            interaction_names
                .get(hit_entity)
                .ok()
                .map(|name| format_interaction_prompt(&key, "equip", name))
        }
        _ => None,
    };
}

fn interact(
    state: Res<State<common::game_state::GameState>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    egui_wants: Option<Res<EguiWantsInput>>,
    bindings: Res<common::ActiveBindings>,
    ticker: Res<common::tick::Ticker>,
    mut interaction: ResMut<InteractionGate>,
    player: Query<
        (
            Entity,
            &BipedPawnComponent,
            &physics::physics_world::RigidBodyHandleComponent,
        ),
        With<Controller>,
    >,
    mut world: ResMut<PhysicsWorld>,
    mut sp: InteractWorldParams,
    mut quic: ResMut<crate::net::quic::QuicManager>,
) {
    use common::game_state::GameState;
    let blocked = egui_wants.as_ref().is_some_and(|e| e.wants_any_input());
    let Ok((pawn_entity, biped, body_handle)) = player.single() else {
        return;
    };
    let Some(pitch_e) = biped.pitch_pivot else {
        return;
    };
    let Some((origin, forward)) =
        super::aim_pose(&world, body_handle, biped.look_yaw, biped.look_pitch)
    else {
        return;
    };
    let Some(target) = current_interact_target(
        pawn_entity,
        origin,
        forward,
        &world,
        &sp.interactables,
        &sp.mounts.p0(),
        &sp.mount_anchor_visuals,
    ) else {
        return;
    };
    if !interaction.consume_press(
        !blocked
            && bindings.pressed(
                common::InputAction::Interact,
                &keyboard,
                &mouse,
                common::active_gamepad(gamepads.iter()),
            ),
        ticker.tick,
    ) {
        return;
    }
    match target {
        InteractTarget::Mount(parent_entity) => match state.get() {
            GameState::SinglePlayer => {
                let mut mount_query = sp.mounts.p1();
                let Ok(mut mount) = mount_query.get_mut(parent_entity) else {
                    return;
                };
                if let Some(crate::pawn::mount::MountInteractResult::Mounted) =
                    crate::pawn::mount::handle_mount_interact(
                        pawn_entity,
                        pawn_entity,
                        parent_entity,
                        &mut world,
                        &mut mount,
                        &sp.mount_anchor_transforms,
                    )
                {
                    sp.commands
                        .entity(pawn_entity)
                        .insert(Mounted(parent_entity));
                    sp.commands.entity(pawn_entity).remove::<Controller>();
                    sp.commands
                        .entity(parent_entity)
                        .insert(Controller::new(128));
                    if let Ok(name) = sp.interaction_names.get(parent_entity) {
                        crate::messages::push(&mut sp.commands, format!("Entered {}", name.0));
                    }
                }
            }
            GameState::Multiplayer => {
                let Ok(parent_net_id) = sp.mount_net_ids.get(parent_entity) else {
                    return;
                };
                quic.send_to_server(
                    crate::net::quic::Channel::Ordered,
                    &crate::net::message::MsgType::Interact(crate::net::message::Interact(
                        parent_net_id.clone(),
                    )),
                );
            }
            _ => {}
        },
        InteractTarget::Entity {
            hit_entity,
            net_id: interact_net_id,
        } => {
            if let Ok(pickup) = sp.ability_pickups.get(hit_entity) {
                match state.get() {
                    GameState::SinglePlayer => sp.commands.queue({
                        let pickup = *pickup;
                        move |world: &mut World| {
                            let _ = crate::pawn::biped_ability::set_ability_kind(
                                pawn_entity,
                                pickup.0.spawn_name,
                                world,
                            );
                            if let Ok(entity) = world.get_entity_mut(hit_entity) {
                                entity.despawn();
                            }
                        }
                    }),
                    GameState::Multiplayer => {
                        if let Some(interact_net_id) = interact_net_id {
                            quic.send_to_server(
                                crate::net::quic::Channel::Ordered,
                                &crate::net::message::MsgType::Interact(
                                    crate::net::message::Interact(interact_net_id),
                                ),
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
                    let Ok(mut slots) = sp.possessed_q.single_mut() else {
                        return;
                    };
                    let drop_velocity = forward * 8.0
                        + crate::projectile::shooter_velocity(&world, Some(pawn_entity));
                    if !crate::weapon::helpers::pickup_local_world_weapon(
                        pawn_entity,
                        hit_entity,
                        &interact_net_id,
                        pitch_e,
                        &mut slots,
                        origin + forward,
                        drop_velocity,
                        &mut sp.weapon_states,
                        &mut sp.commands,
                        &mut world,
                        &mut sp.camera_fx,
                        &sp.interaction_names,
                    ) {
                        return;
                    }
                }
                GameState::Multiplayer => {
                    if let Some(interact_net_id) = interact_net_id {
                        quic.send_to_server(
                            crate::net::quic::Channel::Ordered,
                            &crate::net::message::MsgType::Interact(crate::net::message::Interact(
                                interact_net_id,
                            )),
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
    gamepads: Query<&Gamepad>,
    egui_wants: Res<EguiWantsInput>,
    bindings: Res<common::ActiveBindings>,
    state: Res<State<common::game_state::GameState>>,
    player: Query<(Entity, &BipedPawnComponent), With<Controller>>,
    pitch_pivots: Query<&GlobalTransform, With<PitchPivot>>,
    mut slots_q: Query<&mut WeaponSlots, With<Controller>>,
    mut weapon_states: Query<&mut crate::weapon::WeaponState>,
    mut camera_fx: Query<&mut CameraEffector, With<Camera3d>>,
    mut commands: Commands,
    mut world: ResMut<PhysicsWorld>,
    mut quic: ResMut<crate::net::quic::QuicManager>,
    mut drop_pressed: Local<bool>,
) {
    use common::game_state::GameState;
    if egui_wants.wants_any_input()
        || !consume_fixed_press(
            bindings.pressed(
                common::InputAction::DropWeapon,
                &keyboard,
                &mouse,
                common::active_gamepad(gamepads.iter()),
            ),
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
                crate::net::quic::Channel::Ordered,
                &crate::net::message::MsgType::DropWeapon(crate::net::message::DropWeapon(
                    rot * Vec3::NEG_Z,
                )),
            );
        }
        GameState::SinglePlayer => {
            let Ok((pawn_entity, biped)) = player.single() else {
                return;
            };
            let Ok(mut slots) = slots_q.single_mut() else {
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
            let drop_velocity =
                forward * 8.0 + crate::projectile::shooter_velocity(&world, Some(pawn_entity));
            crate::weapon::helpers::drop_local_active_weapon(
                &mut slots,
                origin + forward,
                drop_velocity,
                &mut weapon_states,
                &mut commands,
                &mut world,
                &mut camera_fx,
            );
        }
        _ => {}
    }
}

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
