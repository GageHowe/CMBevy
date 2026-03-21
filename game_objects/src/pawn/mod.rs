// pub mod kinds;
pub mod biped;
pub mod spaceship;
// pub mod dep;

use physics::physics_world::*;
use physics::physics_world::PhysicsWorld;
use common::ring_buffer::RingBuffer;
use net::message::MsgType;
use bevy::prelude::*;
use std::collections::HashMap;
use crate::GameObject;

pub use biped::BipedPawnComponent;
pub use biped::{YawPivot, PitchPivot};
pub use spaceship::SpaceshipPawnComponent;
pub use common::{BipedInput, SpaceshipInput, PawnInputKind};

pub struct PawnPlugin;
impl Plugin for PawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(biped::BipedPlugin);
        app.add_plugins(spaceship::SpaceshipPlugin);
    }
}

/// all pawns implement this; defines input and movement
pub trait Pawn: Component<Mutability = bevy::ecs::component::Mutable> + GameObject {
    fn apply_input(&mut self, world: &mut PhysicsWorld, body: &RigidBodyHandleComponent, input: PawnInputKind);
}

// CAMERA

/// runtime mouse sensitivity, set from the Settings resource by SettingsPlugin. Only needed by client.
#[derive(Resource)]
pub struct MouseSensitivity(pub f32);
impl Default for MouseSensitivity {
    fn default() -> Self { Self(0.002) }
}

/// Marks a pawn as possessed and owns its input history for prediction + reconciliation.
///
/// - Client: added to the pawn the local player controls
/// - Server: not used (server applies inputs directly from network messages)
#[derive(Component)]
pub struct Possessed {
    input_buffer: RingBuffer<PawnInputKind>,
    input_history: HashMap<u64, PawnInputKind>,
}
impl Possessed {
    pub fn new(capacity: usize) -> Self {
        Self {
            input_buffer: RingBuffer::new(capacity),
            input_history: HashMap::new(),
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
    /// record input for the given tick (used by client for reconciliation replay).
    pub fn record_input(&mut self, tick: u64, input: PawnInputKind) {
        self.input_history.insert(tick, input);
    }
    /// look up the recorded input for a tick.
    pub fn get_input(&self, tick: u64) -> Option<&PawnInputKind> {
        self.input_history.get(&tick)
    }
    /// drop input history older than `before_tick` to bound memory.
    pub fn prune_input_history(&mut self, before_tick: u64) {
        self.input_history.retain(|&t, _| t >= before_tick);
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
pub fn move_pawns<T: Pawn>() -> impl Fn(ResMut<PhysicsWorld>, Query<(&mut Possessed, &RigidBodyHandleComponent, &mut T)>) {
    |mut world, mut pawns| {
        for (mut possessed, handle, mut component) in pawns.iter_mut() {
            let Some(input) = possessed.consume() else { continue };
            component.apply_input(&mut world, handle, input);
        }
    }
}

/// Peeks the newest buffered input, stamps it with the current tick,
/// records it for replay, and sends it serialized over the unreliable channel.
/// Register in client/main.rs after GatherInputSet, before MovePawnsSet, gated on multiplayer.
pub fn send_pawn_input(
    quic: Option<ResMut<net::quic::QuicManager>>,
    tick: Res<common::tick::Ticker>,
    mut pawns: Query<&mut Possessed>,
) {
    let Some(mut quic) = quic else { return };
    let Ok(mut possessed) = pawns.single_mut() else { return };
    let Some(input) = possessed.peek_newest().cloned() else { return };
    let t = tick.tick;
    possessed.record_input(t, input.clone());
    // keep ~2 seconds of history
    possessed.prune_input_history(t.saturating_sub(128));
    quic.send(
        net::quic::SendTarget::All,
        net::quic::Channel::Unreliable,
        &MsgType::Input(t, input),
    );
}
