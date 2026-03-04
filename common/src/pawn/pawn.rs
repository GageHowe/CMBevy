use crate::physics::physics_world::*;
use crate::{physics::physics_world::PhysicsWorld, ring_buffer::RingBuffer};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use std::collections::HashMap;
use wincode_derive::{SchemaRead, SchemaWrite};

pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        // app.add_systems(Startup, spawn_test_pawn);
        app.add_systems(FixedPreUpdate, (gather_pawn_input, (move_bipeds, move_spaceships)).chain());
        app.add_systems(FixedPostUpdate, snap_camera_to_rig);
    }
}


// ============================================================================
// COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct BipedPawnComponent;

#[derive(Component)]
pub struct SpaceshipPawnComponent;

/// Input state consumed by movement systems each tick.
/// does this really need to be Component?
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
}

/// Camera follow configuration.
#[derive(Component)]
pub struct CameraRigComponent {
    pub offset: Vec3,
}

impl Default for CameraRigComponent {
    fn default() -> Self {
        Self { offset: Vec3::new(0.0, 1.0, 5.0) }
    }
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

/// STABLE, DO NOT CHANGE
/// gathers keyboard input for the locally possessed pawn(s)
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut Possessed>,
    egui_wants_input: Res<EguiWantsInput>,
) {
    if egui_wants_input.wants_any_input() { return; }
    let Ok(mut possessed) = pawns.single_mut() else { return };

    let mut input = PawnInput::default();
    if keyboard.pressed(KeyCode::KeyW) { input.forward -= 1.0; }
    if keyboard.pressed(KeyCode::KeyS) { input.forward += 1.0; }
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

    possessed.push(input);
}

/// Snaps the camera to the possessed pawn's rig offset.
pub fn snap_camera_to_rig(
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Possessed>)>,
    pawn: Query<(&Transform, &CameraRigComponent), (With<Possessed>, Without<Camera3d>)>,
) {
    let Ok(mut cam) = camera.single_mut() else { return };
    let Ok((pawn_t, rig)) = pawn.single() else { return };
    cam.translation = pawn_t.translation + pawn_t.rotation * rig.offset;
    cam.rotation = pawn_t.rotation;
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