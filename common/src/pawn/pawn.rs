use crate::physics::physics_world::*;
use crate::{physics::physics_world::PhysicsWorld, ring_buffer::RingBuffer};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

/// Plugin that handles pawn movement, input, and camera systems
pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedPreUpdate,
            (
                gather_pawn_input,
                (do_movement/*, super::spaceship::movement*/),
            )
                .chain(),
        ) // grab inputs before simulation is stepped in FixedUpdate
        .add_systems(
            FixedPostUpdate, // before physics tick and syncing
            (snap_camera_to_rig, on_controlled_add_buffer, clean_buffers).chain(),
        )
        .add_systems(Startup, spawn_test_pawn);
    }
}

fn spawn_test_pawn(
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    world: ResMut<PhysicsWorld>,
) {
    super::biped::spawn(
        Transform::from_xyz(0.0, 2.0, 0.0),
        commands,
        meshes,
        materials,
        world,
    );
}

// ============================================================================
// COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct BipedPawnComponent;
#[derive(Component)]
pub struct SpaceshipPawnComponent;

/// Input state for a pawn. Gets consumed by movement systems.
/// All values default to 0.0 or false each frame and are set by input gathering.
#[derive(Component, Default, Clone, Copy)]
pub struct PawnInputComponent {
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
pub struct CameraRigComponent {
    pub offset: Vec3,
}

impl Default for CameraRigComponent {
    fn default() -> Self {
        Self {
            offset: Vec3::new(0.0, 1.0, 5.0), // Default eye height
        }
    }
}

/// Ring buffer of inputs for rollback/prediction.
/// Only present on pawns that need input history (controlled pawns).
#[derive(Component)]
pub struct InputBufferComponent {
    pub inputs: RingBuffer<PawnInputComponent>,
}
impl InputBufferComponent {
    pub fn new(capacity: usize) -> Self {
        Self {
            inputs: RingBuffer::new(capacity),
        }
    }

    pub fn consume(&mut self) -> Option<PawnInputComponent> {
        self.inputs.pop()
    }
}

/// Marks a pawn as currently possessed by the local player.
/// The camera will follow this pawn and input will be gathered for it.
#[derive(Component)]
pub struct PossesssionComponent;

/// Marks a pawn as being controlled (locally or remotely).
/// Triggers addition of InputBuffer for rollback/prediction.
///
/// Usage:
/// - Client: Add to pawns the local player controls (usually same as Possessed)
/// - Server: Add to all pawns that any client is controlling
#[derive(Component)]
pub struct Controlled;

// ============================================================================
// SYSTEMS
// ============================================================================

/// Gathers keyboard input for the currently possessed pawn.
/// Creates input and stores it directly in the buffer.
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut InputBufferComponent, With<PossesssionComponent>>,
    egui_wants_input: Res<EguiWantsInput>,
) {
    if egui_wants_input.wants_any_input() {
        return;
    }

    let Ok(mut buffer) = pawns.single_mut() else {
        return;
    };

    // grab all inputs
    let mut input = PawnInputComponent::default();
    if keyboard.pressed(KeyCode::KeyW) {
        input.forward = -1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        input.forward = 1.0;
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

/// Updates camera position and rotation to match pawn + camera rig offset.
pub fn snap_camera_to_rig(
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<PossesssionComponent>)>,
    pawn: Query<(&Transform, &CameraRigComponent), (With<PossesssionComponent>, Without<Camera3d>)>,
) {
    let Ok(mut cam_transform) = camera.single_mut() else {
        return;
    };
    let Ok((pawn_transform, rig)) = pawn.single() else {
        return;
    };

    cam_transform.translation = pawn_transform.translation + pawn_transform.rotation * rig.offset;

    cam_transform.rotation = pawn_transform.rotation;
}

/// Switches possession from one pawn to another.
pub fn possess_pawn(
    mut commands: Commands,
    // keyboard: Res<ButtonInput<KeyCode>>,
    // pawns: Query<Entity, With<Pawn>>,
    current: Query<Entity, With<PossesssionComponent>>,
    target: Entity,
) {
    let Ok(current_entity) = current.single() else {
        return;
    };

    commands
        .entity(current_entity)
        .remove::<PossesssionComponent>();
    commands.entity(target).insert(PossesssionComponent);
}

/// Adds InputBuffer when a pawn becomes Controlled.
/// This happens when:
/// - Client: local player takes control of a pawn
/// - Server: any client takes control of a pawn
pub fn on_controlled_add_buffer(
    mut commands: Commands,
    newly_controlled: Query<Entity, Added<Controlled>>,
) {
    for entity in &newly_controlled {
        commands
            .entity(entity)
            .insert(InputBufferComponent::new(60)); // 60 slots
    }
}

/// Saves memory by removing InputBuffer component when a pawn is no longer Controlled.
pub fn clean_buffers(mut commands: Commands, mut removed: RemovedComponents<Controlled>) {
    for entity in removed.read() {
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<InputBufferComponent>();
        }
    }
}

pub fn do_movement(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<(&mut InputBufferComponent, &PhysicsBodyHandle), With<BipedPawnComponent>>,
    mut spaceships: Query<
        (&mut InputBufferComponent, &PhysicsBodyHandle),
        (With<SpaceshipPawnComponent>, Without<BipedPawnComponent>), // jank, why is this needed
    >,
) {
    // bipeds
    for (mut buffer, body_handle) in bipeds.iter_mut() {
        let Some(input) = buffer.consume() else {
            continue;
        };
        super::biped::apply_biped_movement(&mut world, body_handle, input);
    }

    // spaceships
    for (mut buffer, body_handle) in spaceships.iter_mut() {
        let Some(input) = buffer.consume() else {
            continue;
        };
        apply_spaceship_movement(&mut world, body_handle, input);
    }
}

fn apply_spaceship_movement(
    _world: &mut PhysicsWorld,
    _body_handle: &PhysicsBodyHandle,
    _input: PawnInputComponent,
) {
    // let Some(body) = world.rigid_body_set.get_mut(body_handle.0) else {
        return;
    // };

    // Different movement logic for spaceships
    // e.g., 6DOF movement, rotation based on pitch/yaw/roll, etc.
}
