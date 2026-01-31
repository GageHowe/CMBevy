use crate::core::ring_buffer::RingBuffer;
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

/// Plugin that handles pawn movement, input, and camera systems
pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InputBufferConfig::default())
            .add_systems(
                FixedUpdate,
                (gather_pawn_input, move_pawn, attach_camera_to_pawn).chain(),
            )
            .add_systems(Update, (on_controlled_add_buffer, clean_buffers))
            .add_systems(Startup, spawn_test_pawn);
    }
}

// ============================================================================
// COMPONENTS
// ============================================================================

/// Marks an entity as a pawn that can be controlled
#[derive(Component)]
pub struct Pawn;

/// Input state for a pawn. Gets consumed by movement systems.
/// All values default to 0.0 or false each frame and are set by input gathering.
#[derive(Component, Default, Clone, Copy)]
pub struct PawnInput {
    pub forward: f32,  // -1.0 to 1.0
    pub right: f32,    // -1.0 to 1.0
    pub up: f32,       // -1.0 to 1.0
    pub pitch: f32,    // -1.0 to 1.0
    pub yaw: f32,      // -1.0 to 1.0
    pub roll: f32,     // -1.0 to 1.0
    pub ability: bool, // special ability key (shift)
}

/// Camera offset configuration for a pawn.
/// Camera will follow the pawn at this offset.
#[derive(Component)]
pub struct CameraRig {
    pub offset: Vec3,      // Offset from pawn position (e.g., eye height)
    pub look_offset: Vec3, // Additional look offset (currently unused)
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            offset: Vec3::new(0.0, 1.6, 0.0), // Default eye height
            look_offset: Vec3::ZERO,
        }
    }
}

/// Ring buffer of inputs for rollback/prediction.
/// Only present on pawns that need input history (controlled pawns).
#[derive(Component)]
pub struct InputBuffer {
    pub inputs: RingBuffer<PawnInput>,
}

impl InputBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inputs: RingBuffer::new(capacity),
        }
    }
}

/// Marks a pawn as currently possessed by the local player.
/// The camera will follow this pawn and input will be gathered for it.
#[derive(Component)]
pub struct Possessed;

/// Marks a pawn as being controlled (locally or remotely).
/// Triggers addition of InputBuffer for rollback/prediction.
///
/// Usage:
/// - Client: Add to pawns the local player controls (usually same as Possessed)
/// - Server: Add to all pawns that any client is controlling
#[derive(Component)]
pub struct Controlled;

/// Type of pawn, determines movement behavior
#[derive(Component)]
pub enum PawnKind {
    FpsBiped,
    Spaceship,
    Car,
}

// ============================================================================
// RESOURCES
// ============================================================================

/// Global configuration for input buffer size.
/// Buffers are only created for pawns marked as Controlled.
#[derive(Resource)]
pub struct InputBufferConfig {
    pub capacity: usize,
}

impl Default for InputBufferConfig {
    fn default() -> Self {
        Self {
            capacity: 60, // ~1 second at 60fps
        }
    }
}

// ============================================================================
// SYSTEMS
// ============================================================================

/// Gathers keyboard input for the currently possessed pawn.
/// Creates input and stores it directly in the buffer.
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut InputBuffer, With<Possessed>>,
    egui_wants_input: Res<EguiWantsInput>,
) {
    if egui_wants_input.wants_any_input() {
        return;
    }

    let Ok(mut buffer) = pawns.single_mut() else {
        return;
    };

    // grab all inputs
    let mut input = PawnInput::default();
    if keyboard.pressed(KeyCode::KeyW) {
        input.forward = 1.0;
    }
    if keyboard.pressed(KeyCode::KeyW) {
        input.forward = 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        input.forward = -1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        input.right = 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        input.right = -1.0;
    }
    if keyboard.pressed(KeyCode::Space) {
        input.up = 1.0;
    }
    if keyboard.pressed(KeyCode::ControlLeft) {
        input.up = -1.0;
    }
    if keyboard.pressed(KeyCode::ArrowUp) {
        input.pitch = 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowDown) {
        input.pitch = -1.0;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        input.yaw = 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowLeft) {
        input.yaw = -1.0;
    }
    if keyboard.pressed(KeyCode::KeyQ) {
        input.roll = -1.0;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        input.roll = 1.0;
    }
    input.ability = keyboard.pressed(KeyCode::ShiftLeft);

    buffer.inputs.push(input);
}

/// Attaches the camera to follow the currently possessed pawn.
/// Updates camera position and rotation to match pawn + camera rig offset.
pub fn attach_camera_to_pawn(
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Possessed>)>,
    pawn: Query<(&Transform, &CameraRig), (With<Possessed>, Without<Camera3d>)>,
) {
    let Ok(mut cam_transform) = camera.single_mut() else {
        return;
    };
    let Ok((pawn_transform, rig)) = pawn.single() else {
        return;
    };

    cam_transform.translation = pawn_transform.translation + rig.offset;
    cam_transform.rotation = pawn_transform.rotation;
}

/// Switches possession from one pawn to another when F is pressed.
/// This is a placeholder - actual implementation would take a target entity parameter.
pub fn possess_pawn(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    pawns: Query<Entity, With<Pawn>>,
    current: Query<Entity, With<Possessed>>,
    target: Entity,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }

    let Ok(current_entity) = current.single() else {
        return;
    };

    commands.entity(current_entity).remove::<Possessed>();
    commands.entity(target).insert(Possessed);
}

/// Basic movement system for pawns. TODO: split this into different functions per pawn type.
pub fn move_pawn(mut pawns: Query<(&InputBuffer, &mut Transform), With<Possessed>>) {
    let Ok((buffer, mut transform)) = pawns.single_mut() else {
        return;
    };

    let Some(input) = buffer.inputs.get_newest() else {
        return;
    };

    let speed = 0.2;
    let rotation_speed = 0.01;

    transform.rotate_y(-input.yaw * rotation_speed);

    let forward = transform.forward() * input.forward;
    let right = transform.right() * input.right;
    let up = Vec3::Y * input.up;

    let movement_delta = (forward + right + up) * speed;
    transform.translation += movement_delta;
}

/// Adds InputBuffer when a pawn becomes Controlled.
/// This happens when:
/// - Client: local player takes control of a pawn
/// - Server: any client takes control of a pawn
pub fn on_controlled_add_buffer(
    mut commands: Commands,
    newly_controlled: Query<Entity, Added<Controlled>>,
    config: Res<InputBufferConfig>,
) {
    for entity in &newly_controlled {
        commands
            .entity(entity)
            .insert(InputBuffer::new(config.capacity));
    }
}

/// Saves memory by removing InputBuffer when a pawn is no longer Controlled.
pub fn clean_buffers(mut commands: Commands, mut removed: RemovedComponents<Controlled>) {
    for entity in removed.read() {
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<InputBuffer>();
        }
    }
}

// ============================================================================
// SPAWN FUNCTIONS
// ============================================================================

/// Spawns a test pawn for development.
/// Creates a simple red capsule-shaped pawn that is immediately possessed.
fn spawn_test_pawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Pawn,
        PawnKind::FpsBiped,
        CameraRig::default(),
        Possessed,
        Controlled, // Triggers InputBuffer creation
        Transform::from_xyz(0.0, 1.0, 0.0),
        Mesh3d(meshes.add(Cuboid::new(0.5, 1.8, 0.5))),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
    ));
}
