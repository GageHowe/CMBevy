use crate::physics::physics_world::*;
use crate::{physics::physics_world::PhysicsWorld, ring_buffer::RingBuffer};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

pub struct PawnPlugin;

impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        // app.add_systems(Startup, spawn_test_pawn);
        app.add_systems(FixedPreUpdate, (gather_pawn_input, (move_bipeds, move_spaceships)).chain());
        app.add_systems(FixedPostUpdate, snap_camera_to_rig);
    }
}

/// example of spawning a pawn locally, deprecated
fn spawn_test_pawn(
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    world: ResMut<PhysicsWorld>,
) {
    use crate::net::message::NetworkID;
    super::spaceship::spawn(
        NetworkID(0),
        Transform::from_xyz(0.0, 2.0, 0.0),
        commands, meshes, materials, world,
    );
}

// ============================================================================
// COMPONENTS
// ============================================================================

#[derive(Component)]
pub struct BipedPawnComponent;

#[derive(Component)]
pub struct SpaceshipPawnComponent;

/// Input state consumed by movement systems each tick.
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

/// Marks a pawn as possessed and owns its input buffer.
/// Buffer lifetime is tied to possession — no manual cleanup needed.
///
/// - Client: added to the pawn the local player controls
/// - Server: added to every pawn a client is controlling
#[derive(Component)]
pub struct Possessed {
    buffer: RingBuffer<PawnInputComponent>,
}

impl Possessed {
    pub fn new(capacity: usize) -> Self {
        Self { buffer: RingBuffer::new(capacity) }
    }

    pub fn push(&mut self, input: PawnInputComponent) {
        self.buffer.push(input);
    }

    pub fn consume(&mut self) -> Option<PawnInputComponent> {
        self.buffer.pop()
    }
}

// SYSTEMS

/// gathers keyboard input for the locally possessed pawn(s)
pub fn gather_pawn_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut pawns: Query<&mut Possessed>,
    egui_wants_input: Res<EguiWantsInput>,
) {
    if egui_wants_input.wants_any_input() { return; }
    let Ok(mut possessed) = pawns.single_mut() else { return };

    let mut input = PawnInputComponent::default();
    if keyboard.pressed(KeyCode::KeyW)        { input.forward =  -1.0; }
    if keyboard.pressed(KeyCode::KeyS)        { input.forward =   1.0; }
    if keyboard.pressed(KeyCode::KeyD)        { input.right   =   1.0; }
    if keyboard.pressed(KeyCode::KeyA)        { input.right   =  -1.0; }
    if keyboard.pressed(KeyCode::Space)       { input.up      =   1.0; }
    if keyboard.pressed(KeyCode::ControlLeft) { input.up      =  -1.0; }
    if keyboard.pressed(KeyCode::ArrowUp)     { input.pitch   =   1.0; }
    if keyboard.pressed(KeyCode::ArrowDown)   { input.pitch   =  -1.0; }
    if keyboard.pressed(KeyCode::ArrowRight)  { input.yaw     =   1.0; }
    if keyboard.pressed(KeyCode::ArrowLeft)   { input.yaw     =  -1.0; }
    if keyboard.pressed(KeyCode::KeyQ)        { input.roll    =  -1.0; }
    if keyboard.pressed(KeyCode::KeyE)        { input.roll    =   1.0; }
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