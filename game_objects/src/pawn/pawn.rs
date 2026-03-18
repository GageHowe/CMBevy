use physics::physics_world::*;
use physics::physics_world::PhysicsWorld;
use common::ring_buffer::RingBuffer;
pub use common::PawnInput;
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use std::collections::HashMap;

pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(super::biped::BipedPlugin);
        app.add_systems(FixedPreUpdate, (
            gather_pawn_input.run_if(resource_exists::<ButtonInput<KeyCode>>),
            move_pawns::<SpaceshipPawnComponent>().in_set(MovePawnsSet),
        ).chain());
    }
}

// COMPONENTS

pub use super::biped::BipedPawnComponent;

#[derive(Component)]
pub struct SpaceshipPawnComponent;

// CAMERA

/// Rotates around the pawn's local Y axis (yaw). Child of the pawn entity.
/// used by biped
#[derive(Component)]
pub struct YawPivot {
    pub yaw: f32,
}
/// Rotates around its local X axis (pitch). Child of YawPivot.
/// used by biped
#[derive(Component)]
pub struct PitchPivot {
    pub pitch: f32,
}

pub const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

/// Runtime mouse sensitivity, set from the Settings resource by SettingsPlugin.
/// Defaults to 0.002 so the server (which never sets it) doesn't need it at all.
#[derive(Resource)]
pub struct MouseSensitivity(pub f32);

impl Default for MouseSensitivity {
    fn default() -> Self { Self(0.002) }
}

// CORE

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: added to every pawn a client is controlling
#[derive(Component)]
pub struct Possessed {
    buffer: RingBuffer<PawnInput>,
    /// tick -> input, kept for reconciliation replay
    /// should this be a ringbuffer?
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
    /// peek at the most recently pushed input without consuming it.
    pub fn peek_newest(&self) -> Option<&PawnInput> {
        self.buffer.get_newest()
    }
    /// record input for the given tick (used by client for reconciliation replay).
    pub fn record_input(&mut self, tick: u64, input: PawnInput) {
        self.input_history.insert(tick, input);
    }
    /// look up the recorded input for a tick.
    pub fn get_input(&self, tick: u64) -> Option<&PawnInput> {
        self.input_history.get(&tick)
    }
    /// drop input history older than `before_tick` to bound memory.
    /// (what is this even for?)
    pub fn prune_input_history(&mut self, before_tick: u64) {
        self.input_history.retain(|&t, _| t >= before_tick);
    }

}

// SYSTEMS

/// gathers keyboard input for the locally possessed pawn(s)
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<(&mut Possessed, Option<&BipedPawnComponent>)>,
    yaw_pivots: Query<&YawPivot>,
    pitch_pivots: Query<&PitchPivot>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) { return; }
    let Ok((mut possessed, biped)) = pawns.single_mut() else { return };

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

    if let Some(biped) = biped {
        if let Some(yaw_e) = biped.yaw_pivot {
            if let Ok(yp) = yaw_pivots.get(yaw_e) { input.look_yaw = yp.yaw; }
        }
        if let Some(pitch_e) = biped.pitch_pivot {
            if let Ok(pp) = pitch_pivots.get(pitch_e) { input.look_pitch = pp.pitch; }
        }
    }

    possessed.push(input);
}

/// transfers possession from the current pawn to a new target
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

/// System set covering all `move_pawns` systems. Use for ordering against pawn movement.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePawnsSet;

/// Per-pawn movement logic. Implement on each pawn component.
pub trait Pawn: Component<Mutability = bevy::ecs::component::Mutable> {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponenet, input: PawnInput);
}

/// generic input consumption function for all pawn types
pub fn move_pawns<T: Pawn>() -> impl Fn(ResMut<PhysicsWorld>, Query<(&mut Possessed, &RigidBodyHandleComponenet, &mut T)>) {
    |mut world, mut pawns| {
        for (mut possessed, handle, mut component) in pawns.iter_mut() {
            let Some(input) = possessed.consume() else { continue };
            component.apply_input(&mut world, handle, input);
        }
    }
}
