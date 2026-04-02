// pub mod kinds;
pub mod biped;
pub mod spaceship;
pub mod vehicle;

/// spring-damping recoil + procedural shake + zoom applied on top of gameplay aim.
/// Placed on the Camera3d entity by biped possession; any pawn system can write to it.
#[derive(bevy::prelude::Component)]
pub struct CameraEffector {
    pub pitch_offset: f32,
    pub pitch_vel: f32,
    pub yaw_offset: f32,
    pub yaw_vel: f32,
    /// Shake intensity; decays toward zero each frame.
    pub shake: f32,
    /// for recoil recovery
    pub recovery_speed: f32,
    /// User's base FOV in degrees. Set at possession; updated when settings change.
    pub base_fov: f32,
    /// Zoom multiplier for this tick (1.0 = no zoom). Weapons write this; no auto-reset.
    pub zoom_multiplier: f32,
    /// Smoothly lerped FOV in degrees, written to Projection each frame.
    pub current_fov: f32,
}
impl Default for CameraEffector {
    fn default() -> Self {
        Self {
            pitch_offset: 0.0,
            pitch_vel: 0.0,
            yaw_offset: 0.0,
            yaw_vel: 0.0,
            shake: 0.0,
            recovery_speed: 18.0,
            base_fov: 90.0,
            zoom_multiplier: 1.0,
            current_fov: 90.0,
        }
    }
}
impl CameraEffector {
    pub fn add_kick(&mut self, vertical: (f32, f32), horizontal: (f32, f32), recovery_speed: f32) {
        self.pitch_vel += vertical.0 + fastrand::f32() * (vertical.1 - vertical.0);
        self.yaw_vel += horizontal.0 + fastrand::f32() * (horizontal.1 - horizontal.0);
        self.recovery_speed = recovery_speed;
    }
    pub fn add_shake(&mut self, amount: f32) {
        self.shake += amount;
    }
}
// pub mod dep;

use crate::GameObject;
use bevy::prelude::*;
use common::PredictedCommands;
use common::ring_buffer::RingBuffer;
use net::message::MsgType;
use physics::physics_world::PhysicsWorld;
use physics::physics_world::*;

pub use biped::BipedPawnComponent;
pub use biped::{PitchPivot, YawPivot};
pub use common::{BipedInput, PawnInputKind, SpaceshipInput};
pub use spaceship::SpaceshipPawnComponent;
pub use vehicle::VehicleComponent;

pub struct PawnPlugin;
impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(biped::BipedPlugin);
        app.add_plugins(spaceship::SpaceshipPlugin);
        app.add_plugins(vehicle::VehiclePlugin);
    }
}

/// all pawns implement this; defines input and movement
pub trait Pawn: Component<Mutability = bevy::ecs::component::Mutable> + GameObject {
    fn apply_input(
        &mut self,
        world: &mut PhysicsWorld,
        body: &RigidBodyHandleComponent,
        input: PawnInputKind,
    );
}

macro_rules! for_each_pawn_input_type {
    ($m:ident $($args:tt)*) => {
        $m!(
            $($args)*
            PawnInputKind::Biped => apply_biped_server_input,
            PawnInputKind::Spaceship => apply_spaceship_server_input
        )
    };
}

macro_rules! apply_server_input_match {
    ($input:expr, $entity:expr, $world:expr, $bipeds:expr, $spaceships:expr; $($kind:path => $handler:path),+ $(,)?) => {
        match $input {
            $(
                $kind(input) => $handler($entity, input, $world, $bipeds, $spaceships),
            )+
        }
    };
}

fn apply_biped_server_input(
    entity: Entity,
    input: BipedInput,
    world: &mut PhysicsWorld,
    bipeds: &mut Query<&mut biped::BipedPawnComponent>,
    _spaceships: &mut Query<&mut spaceship::SpaceshipPawnComponent>,
) -> bool {
    let Ok(mut biped) = bipeds.get_mut(entity) else {
        return false;
    };
    if biped.in_vehicle.is_some() {
        return false;
    }
    let Some(handle) = world.entity_to_handle.get(&entity).copied() else {
        return false;
    };
    biped.look_yaw = input.look_yaw;
    biped.look_pitch = input.look_pitch;
    biped::apply_biped_movement(world, &RigidBodyHandleComponent(handle), input, &mut biped);
    true
}

fn apply_spaceship_server_input(
    entity: Entity,
    input: SpaceshipInput,
    world: &mut PhysicsWorld,
    bipeds: &mut Query<&mut biped::BipedPawnComponent>,
    spaceships: &mut Query<&mut spaceship::SpaceshipPawnComponent>,
) -> bool {
    let vehicle_entity = bipeds.get(entity).ok().and_then(|b| b.in_vehicle);
    let Some(vehicle_entity) = vehicle_entity else {
        return false;
    };
    let Some(handle) = world.entity_to_handle.get(&vehicle_entity).copied() else {
        return false;
    };
    let Ok(mut ship) = spaceships.get_mut(vehicle_entity) else {
        return false;
    };
    spaceship::apply_spaceship_movement(world, &RigidBodyHandleComponent(handle), input, &mut ship);
    true
}

pub fn apply_server_input(
    entity: Entity,
    input: PawnInputKind,
    world: &mut PhysicsWorld,
    bipeds: &mut Query<&mut biped::BipedPawnComponent>,
    spaceships: &mut Query<&mut spaceship::SpaceshipPawnComponent>,
) -> bool {
    for_each_pawn_input_type!(apply_server_input_match input, entity, world, bipeds, spaceships;)
}

// CAMERA

/// runtime mouse sensitivity, set from the Settings resource by SettingsPlugin. Only needed by client.
#[derive(Resource)]
pub struct MouseSensitivity {
    pub base: f32,
    pub zoom_blend: f32,
}
impl Default for MouseSensitivity {
    fn default() -> Self {
        Self {
            base: 0.002,
            zoom_blend: 1.0,
        }
    }
}

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: not used (server applies inputs directly from network messages)
#[derive(Component)]
#[component(storage = "SparseSet")]
pub struct Possessed {
    input_buffer: RingBuffer<PawnInputKind>,
}
impl Possessed {
    pub fn new(capacity: usize) -> Self {
        Self {
            input_buffer: RingBuffer::new(capacity),
        }
    }
    pub fn push(&mut self, input: PawnInputKind) {
        self.input_buffer.push(input);
    }
    pub fn consume(&mut self) -> Option<PawnInputKind> {
        self.input_buffer.pop()
    }
    /// peek at the most recently pushed input without consuming it.
    pub fn peek_newest(&self) -> Option<&PawnInputKind> {
        self.input_buffer.get_newest()
    }
}

// SYSTEMS

/// System set covering all gather-input systems. Reconciliation runs before this.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatherInputSet;

/// System set covering all `move_pawns` systems. Use for ordering against pawn movement.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MovePawnsSet;

/// generic input consumption function for all pawn types
pub fn move_pawns<T: Pawn>()
-> impl Fn(ResMut<PhysicsWorld>, Query<(&mut Possessed, &RigidBodyHandleComponent, &mut T)>) {
    |mut world, mut pawns| {
        for (mut possessed, handle, mut component) in pawns.iter_mut() {
            let Some(input) = possessed.consume() else {
                continue;
            };
            component.apply_input(&mut world, handle, input);
        }
    }
}

/// Peeks the newest buffered input, stamps it with the current tick,
/// records it for replay, and sends it serialized over the unreliable channel.
/// Register in client/main.rs after GatherInputSet, before MovePawnsSet, gated on multiplayer.
pub fn send_pawn_input(
    quic: Option<ResMut<net::quic::QuicManager>>,
    pawns: Query<&Possessed>,
    mut predicted: ResMut<PredictedCommands>,
) {
    let Some(mut quic) = quic else { return };
    let Ok(possessed) = pawns.single() else {
        return;
    };
    let Some(input) = possessed.peek_newest().cloned() else {
        return;
    };
    let seq = predicted.record_input(input.clone());
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Unreliable,
        &MsgType::Input(seq, input),
    );
}
