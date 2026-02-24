use crate::physics::physics_world::*;
use crate::{physics::physics_world::PhysicsWorld, ring_buffer::RingBuffer};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

/// Plugin that handles pawn movement, input, and camera systems
pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        // eventually, we'll remove this and request pawns from server
        app.add_systems(Startup, spawn_test_pawn);

        // before FixedUpdate, gather and apply inputs
        app.add_systems(FixedPreUpdate, (gather_pawn_input, (move_bipeds, move_spaceships)).chain());

        // after FixedUpdate, update camera and do cleanup
        app.add_systems(
            FixedPostUpdate, (snap_camera_to_rig, on_controlled_add_buffer, clean_buffers).chain(),
        );
    }
}

fn spawn_test_pawn(
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    world: ResMut<PhysicsWorld>,
) {
    super::spaceship::spawn(
        Transform::from_xyz(0.0, 2.0, 0.0),
        commands,
        meshes,
        materials,
        world,
    );
}

// COMPONENTS
// these mark the pawn types for systems to act on later

#[derive(Component)]
pub struct BipedPawnComponent;
#[derive(Component)]
pub struct SpaceshipPawnComponent;

// INPUT AND CONTROL

/// input state used by all pawn types and which gets consumed by movement systems.
#[derive(Component, Default, Clone, Copy)]
pub struct PawnInputComponent {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub ability1: bool,
    pub ability2: bool,
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

/// ring buffer of inputs for rollback/prediction.
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

/// marks a pawn as currently possessed by the local player.
/// the camera will follow this pawn and input will be gathered for it.
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
    input.ability1 = keyboard.pressed(KeyCode::ShiftLeft);
    input.ability2 = keyboard.pressed(KeyCode::KeyE);

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

/// switches possession component from one pawn to another.
pub fn possess_pawn(
    mut commands: Commands,
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

/// adds an InputBuffer when a pawn becomes Controlled.
/// This happens when:
/// on client: local player takes control of a pawn
/// on server: any client takes control of a pawn
pub fn on_controlled_add_buffer(
    mut commands: Commands,
    newly_controlled: Query<Entity, Added<Controlled>>,
) {
    for entity in &newly_controlled {
        commands
            .entity(entity)
            .insert(InputBufferComponent::new(60));
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

// FUNCTIONS FOR MOVING PAWN TYPES

pub fn move_bipeds(
    mut world: ResMut<PhysicsWorld>,
    mut bipeds: Query<(&mut InputBufferComponent, &PhysicsBodyHandle), With<BipedPawnComponent>>,
) {
    for (mut buffer, body_handle) in bipeds.iter_mut() {
        let Some(input) = buffer.consume() else { continue };
        super::biped::apply_biped_movement(&mut world, body_handle, input);
    }
}

pub fn move_spaceships(
    mut world: ResMut<PhysicsWorld>,
    mut spaceships: Query<(&mut InputBufferComponent, &PhysicsBodyHandle), With<SpaceshipPawnComponent>>,
) {
    for (mut buffer, body_handle) in spaceships.iter_mut() {
        let Some(input) = buffer.consume() else { continue };
        super::spaceship::apply_spaceship_movement(&mut world, body_handle, input);
    }
}
