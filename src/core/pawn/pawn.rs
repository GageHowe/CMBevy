use crate::core::physics::physics_world::*;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use rapier3d::dynamics::RigidBody;
use rapier3d::prelude::*;

// COMPONENTS

/// any entity that can be possessed
#[derive(Component)]
pub struct Pawn;

/// inputs to be consumed by the different kinds of pawns
#[derive(Component, Default, Clone, Copy)]
pub struct PawnInput {
    forward: f32,
    right: f32,
    up: f32,
    pitch: f32,
    yaw: f32,
    roll: f32,
    ability: bool, // shift key etc
}

/// a buffer of inputs owned by each pawn
#[derive(Component)]
struct InputBuffer {
    inputs: Vec<PawnInput>,
}

// not sure which of these I'll use yet
// #[derive(Component)]
// pub struct Possessed;
// #[derive(Component)]
// struct ControlledBy(ClientId);

#[derive(Component)]
pub enum PawnKind {
    FpsBiped,
    Spaceship,
    Car,
    // Car, Turret, etc.
}

// pub fn gather_pawn_input(
//     keyboard: Res<ButtonInput<KeyCode>>,
//     mut pawns: Query<&mut PawnInput, With<Possessed>>,
//     egui_wants_input: Res<EguiWantsInput>,
//     // mut mouse_motion_events: MessageReader<MouseMotion>,
// ) {
//     if egui_wants_input.wants_any_input() {
//         return;
//     }
//     let Ok(mut input) = pawns.single_mut() else {
//         return;
//     };

//     if keyboard.pressed(KeyCode::KeyW) {
//         input.forward += 1.0;
//     }
//     if keyboard.pressed(KeyCode::KeyS) {
//         input.forward -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::KeyD) {
//         input.right += 1.0;
//     }
//     if keyboard.pressed(KeyCode::KeyA) {
//         input.right -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::Space) {
//         input.up += 1.0;
//     }
//     if keyboard.pressed(KeyCode::ControlLeft) {
//         input.up -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::ArrowUp) {
//         input.pitch += 1.0;
//     }
//     if keyboard.pressed(KeyCode::ArrowDown) {
//         input.pitch -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::ArrowRight) {
//         input.yaw += 1.0;
//     }
//     if keyboard.pressed(KeyCode::ArrowLeft) {
//         input.yaw -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::KeyQ) {
//         input.roll -= 1.0;
//     }
//     if keyboard.pressed(KeyCode::KeyE) {
//         input.roll += 1.0;
//     }
//     input.ability = keyboard.pressed(KeyCode::ShiftLeft);
// }

// pub fn possess_pawn(
//     mut commands: Commands,
//     keyboard: Res<ButtonInput<KeyCode>>,
//     mut pawns: Query<(Entity, &PawnKind), With<Pawn>>,
//     current: Query<Entity, With<Possessed>>,
//     target: Entity,
// ) {
//     if !keyboard.just_pressed(KeyCode::KeyF) {
//         return;
//     }

//     let current_entity = current.single().ok();
//     let Some(current_entity) = current_entity else {
//         return;
//     };

//     commands.entity(current_entity).remove::<Possessed>();
//     commands.entity(target).insert(Possessed);
// }

// pub struct PawnPlugin;
// impl Plugin for PawnPlugin {
//     fn build(&self, app: &mut App) {
//         app.add_systems(FixedUpdate, gather_local_pawn_input);
//     }
// }
