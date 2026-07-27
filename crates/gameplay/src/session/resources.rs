#[cfg(not(feature = "client"))]
use std::collections::BTreeMap;
use std::collections::HashMap;

use bevy::prelude::*;

use crate::net::message::{ChatMessage, NetworkID, SimulationState};
#[cfg(not(feature = "client"))]
use crate::net::quic::ConnectionId;
#[cfg(not(feature = "client"))]
use crate::pawn::PawnInput;

#[cfg(feature = "client")]
#[derive(Resource, Clone)]
/// Selected multiplayer server address for the client runtime.
pub struct ServerAddr {
    pub addr: std::net::SocketAddr,
    pub lobby_id: Option<String>,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Highest input sequence acknowledged by the authoritative server.
pub struct LastAckedInputSeq(pub u64);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Stable local player character id used by UI that should not follow vehicle/turret possession.
pub struct LocalCharacterNetId(pub Option<NetworkID>);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Snapshot waiting to be applied by the reconciliation system.
pub struct PendingReconciliation(pub Option<SimulationState>);

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Small bag of UI/runtime state owned by the client session layer.
pub struct GuiState {
    pub command_input: String,
    pub chat: Vec<ChatMessage>,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Single-player session configuration selected from the menu.
pub struct SinglePlayerConfig {
    pub map: String,
    pub gametype: String,
    pub(crate) timer: Option<f32>,
    pub(crate) spawned_once: bool,
}

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct ConsoleCommands(pub std::sync::Mutex<std::sync::mpsc::Receiver<String>>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct BodyHistory(pub HashMap<u64, SimulationState>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Ordered inputs plus the last held input per connection.
pub struct PendingInputs(pub HashMap<ConnectionId, PendingInputState>);

#[cfg(not(feature = "client"))]
#[derive(Default)]
pub struct PendingInputState {
    pub applied_seq: u64,
    pub held: Option<PawnInput>,
    pub queue: BTreeMap<u64, PawnInput>,
}

#[cfg(not(feature = "client"))]
impl PendingInputState {
    pub fn next(&mut self) -> Option<(u64, PawnInput, bool)> {
        if let Some((seq, input)) = self.queue.pop_first() {
            self.applied_seq = seq;
            self.held = Some(input.clone());
            return Some((seq, input, true));
        }
        self.held
            .clone()
            .map(|input| (self.applied_seq, input, false))
    }

    pub fn clear_edges(&mut self) {
        if let Some(input) = &mut self.held {
            clear_biped_edges(input);
        }
    }
}

#[cfg(not(feature = "client"))]
fn clear_biped_edges(input: &mut PawnInput) {
    input.item.primary_pressed = false;
    input.item.secondary_pressed = false;
    input.item.reload_pressed = false;
    input.ability1_pressed = false;
    input.melee_pressed = false;
}

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct LastProcessedInputSeq(pub HashMap<ConnectionId, u64>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Connections currently considered active by the session layer.
pub struct ActiveConnections(pub std::collections::HashSet<ConnectionId>);
