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
        app.add_systems(FixedPreUpdate, (
            gather_pawn_input.run_if(resource_exists::<ButtonInput<KeyCode>>),
            (
                move_pawns::<BipedPawnComponent>(super::biped::apply_biped_movement),
                move_pawns::<SpaceshipPawnComponent>(super::spaceship::apply_spaceship_movement),
            ),
        ).chain());
        app.init_resource::<MouseSensitivity>()
            .add_systems(PostUpdate, mouse_look
                .before(TransformSystems::Propagate)
                .run_if(resource_exists::<AccumulatedMouseMotion>));
    }
}

// COMPONENTS

#[derive(Component)]
pub struct BipedPawnComponent;

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

/// runs every frame in PostUpdate, before transform propagation
pub fn mouse_look(
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    mut yaw_q: Query<(&mut Transform, &mut YawPivot), Without<PitchPivot>>,
    mut pitch_q: Query<(&mut Transform, &mut PitchPivot)>,
) {
    let delta = mouse.delta;
    if delta == Vec2::ZERO { return; }
    let s = sensitivity.0;

    if let Ok((mut t, mut pivot)) = yaw_q.single_mut() {
        pivot.yaw -= delta.x * s;
        t.rotation = Quat::from_rotation_y(pivot.yaw);
    }
    if let Ok((mut t, mut pivot)) = pitch_q.single_mut() {
        pivot.pitch = (pivot.pitch - delta.y * s).clamp(-PITCH_MAX, PITCH_MAX);
        t.rotation = Quat::from_rotation_x(pivot.pitch);
    }
}

/// gathers keyboard input for the locally possessed pawn(s)
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut Possessed>,
    yaw_pivot: Query<&YawPivot>,
    egui_wants_input: Option<Res<EguiWantsInput>>,
) {
    if egui_wants_input.map_or(false, |e| e.wants_any_input()) { return; }
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

    if let Ok(pivot) = yaw_pivot.single() {
        input.look_yaw = pivot.yaw;
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

/// generic input consumption function for all pawn types
pub fn move_pawns<T: Component<Mutability = bevy::ecs::component::Mutable>>(
    apply: fn(&mut PhysicsWorld, &PhysicsBodyHandle, PawnInput, &mut T),
) -> impl Fn(ResMut<PhysicsWorld>, Query<(&mut Possessed, &PhysicsBodyHandle, &mut T)>) {
    move |mut world, mut pawns| {
        for (mut possessed, handle, mut component) in pawns.iter_mut() {
            let Some(input) = possessed.consume() else { continue };
            apply(&mut world, handle, input, &mut component);
        }
    }
}
