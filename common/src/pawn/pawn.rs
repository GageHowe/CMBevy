use crate::physics::physics_world::*;
use crate::{physics::physics_world::PhysicsWorld, ring_buffer::RingBuffer};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_egui::input::EguiWantsInput;
use std::collections::HashMap;
use wincode_derive::{SchemaRead, SchemaWrite};

pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedPreUpdate, (gather_pawn_input, (move_bipeds, move_spaceships)).chain());
        app.add_systems(PostUpdate, mouse_look.before(TransformSystems::Propagate));
    }
}


// ============================================================================
// COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct BipedPawnComponent;

#[derive(Component)]
pub struct SpaceshipPawnComponent;

/// Rotates around the pawn's local Y axis (yaw). Child of the pawn entity.
#[derive(Component)]
pub struct YawPivot;

/// Rotates around its local X axis (pitch). Child of YawPivot.
#[derive(Component)]
pub struct PitchPivot {
    pub pitch: f32,
}

pub const MOUSE_SENSITIVITY: f32 = 0.002;
pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

/// Input state consumed by movement systems each tick.
#[derive(Component, Default, Clone, Copy, SchemaRead, SchemaWrite, Debug, PartialEq)]
pub struct PawnInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub ability1: bool,
    pub ability2: bool,
    /// Pawn-local yaw angle (radians) from the YawPivot at input time.
    /// Server reconstructs world-space facing as: body_rotation * Quat::from_rotation_y(look_yaw).
    pub look_yaw: f32,
}

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: added to every pawn a client is controlling
#[derive(Component)]
pub struct Possessed {
    buffer: RingBuffer<PawnInput>,
    /// tick → input, kept for reconciliation replay
    input_history: HashMap<u64, PawnInput>,
}

impl Possessed {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: RingBuffer::new(capacity),
            input_history: HashMap::new(),
        }
    }

    pub fn push(&mut self, input: PawnInput) {
        self.buffer.push(input);
    }

    pub fn consume(&mut self) -> Option<PawnInput> {
        self.buffer.pop()
    }

    /// Peek at the most recently pushed input without consuming it.
    pub fn peek_newest(&self) -> Option<&PawnInput> {
        self.buffer.get_newest()
    }

    /// Record input for the given tick (used by client for reconciliation replay).
    pub fn record_input(&mut self, tick: u64, input: PawnInput) {
        self.input_history.insert(tick, input);
    }

    /// Look up the recorded input for a tick.
    pub fn get_input(&self, tick: u64) -> Option<&PawnInput> {
        self.input_history.get(&tick)
    }

    /// Drop input history older than `before_tick` to bound memory.
    pub fn prune_input_history(&mut self, before_tick: u64) {
        self.input_history.retain(|&t, _| t >= before_tick);
    }

}

// SYSTEMS

/// Runs every frame in PostUpdate, before transform propagation.
/// Near-zero latency: camera transforms are always current when the scene is rendered.
pub fn mouse_look(
    mouse: Res<AccumulatedMouseMotion>,
    mut yaw_q: Query<&mut Transform, (With<YawPivot>, Without<PitchPivot>)>,
    mut pitch_q: Query<(&mut Transform, &mut PitchPivot)>,
) {
    let delta = mouse.delta;
    if delta == Vec2::ZERO { return; }

    if let Ok(mut t) = yaw_q.single_mut() {
        t.rotate_local_y(-delta.x * MOUSE_SENSITIVITY);
    }
    if let Ok((mut t, mut pivot)) = pitch_q.single_mut() {
        pivot.pitch = (pivot.pitch - delta.y * MOUSE_SENSITIVITY).clamp(-PITCH_MAX, PITCH_MAX);
        t.rotation = Quat::from_rotation_x(pivot.pitch);
    }
}

/// STABLE, DO NOT CHANGE
/// gathers keyboard input for the locally possessed pawn(s)
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut Possessed>,
    yaw_pivot: Query<&Transform, With<YawPivot>>,
    egui_wants_input: Res<EguiWantsInput>,
) {
    if egui_wants_input.wants_any_input() { return; }
    let Ok(mut possessed) = pawns.single_mut() else { return };

    let mut input = PawnInput::default();
    if keyboard.pressed(KeyCode::KeyW) { input.forward += 1.0; }
    if keyboard.pressed(KeyCode::KeyS) { input.forward -= 1.0; }
    if keyboard.pressed(KeyCode::KeyD) { input.right += 1.0; }
    if keyboard.pressed(KeyCode::KeyA) { input.right -= 1.0; }
    if keyboard.pressed(KeyCode::Space) { input.up += 1.0; }
    if keyboard.pressed(KeyCode::ControlLeft) { input.up -= 1.0; }
    if keyboard.pressed(KeyCode::ArrowUp) { input.pitch += 1.0; }
    if keyboard.pressed(KeyCode::ArrowDown) { input.pitch -= 1.0; }
    if keyboard.pressed(KeyCode::ArrowRight) { input.yaw += 1.0; }
    if keyboard.pressed(KeyCode::ArrowLeft) { input.yaw -= 1.0; }
    if keyboard.pressed(KeyCode::KeyQ) { input.roll -= 1.0; }
    if keyboard.pressed(KeyCode::KeyE) { input.roll += 1.0; }
    input.ability1 = keyboard.pressed(KeyCode::ShiftLeft);
    input.ability2 = keyboard.pressed(KeyCode::KeyE);

    // Local Transform is written by mouse_look (last PostUpdate), so one frame stale.
    // Acceptable at 64 Hz. We want local (pawn-relative) yaw, not world-space.
    if let Ok(t) = yaw_pivot.single() {
        input.look_yaw = t.rotation.to_euler(EulerRot::YXZ).0;
    }

    possessed.push(input);
}

/// Transfers possession from the current pawn to a new target.
pub fn possess_pawn(
    mut commands: Commands,
    current: Query<Entity, With<Possessed>>,
    target: Entity,
) {
    if let Ok(entity) = current.single() {
        commands.entity(entity).remove::<Possessed>();
    }
    commands.entity(target).insert(Possessed::new(60));
}

// TODO: if possible, make a better implementation to handle these
// maybe a trait + reflection?

pub fn move_bipeds(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<(&mut Possessed, &PhysicsBodyHandle), With<BipedPawnComponent>>,
) {
    for (mut possessed, body_handle) in bipeds.iter_mut() {
        let Some(input) = possessed.consume() else { continue };
        super::biped::apply_biped_movement(&mut world, body_handle, input);
    }
}

pub fn move_spaceships(
    mut world: ResMut<PhysicsWorld>,
    mut spaceships: Query<(&mut Possessed, &PhysicsBodyHandle), With<SpaceshipPawnComponent>>,
) {
    for (mut possessed, body_handle) in spaceships.iter_mut() {
        let Some(input) = possessed.consume() else { continue };
        super::spaceship::apply_spaceship_movement(&mut world, body_handle, input);
    }
}
