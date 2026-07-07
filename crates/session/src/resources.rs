#[cfg(not(feature = "client"))]
use std::collections::BTreeMap;
use std::collections::HashMap;

use bevy::prelude::*;
#[cfg(not(feature = "client"))]
use gameplay::pawn::PawnInputKind;
use net::message::{NetworkID, SimulationState};
#[cfg(not(feature = "client"))]
use net::quic::ConnectionId;

#[cfg(feature = "client")]
#[derive(Resource, Clone)]
/// Selected multiplayer server address for the client runtime.
pub struct ServerAddr {
    pub addr: std::net::SocketAddr,
    pub lobby_id: Option<String>,
}

#[cfg(feature = "client")]
#[derive(Resource, Default)]
/// Last authoritative physics snapshot received from the server.
pub struct LastServerState(pub Option<SimulationState>);

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
    pub log: Vec<String>,
    pub scoreboard: Option<net::message::ScoreboardSnapshot>,
}

#[cfg(feature = "client")]
impl GuiState {
    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }
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

#[cfg(feature = "client")]
pub(crate) type JustSpawned = HashMap<NetworkID, (Entity, u64)>;

#[cfg(feature = "client")]
#[derive(Resource, Default)]
pub(crate) struct PendingWeaponPickups(pub Vec<(NetworkID, NetworkID)>);

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
    pub held: Option<PawnInputKind>,
    pub queue: BTreeMap<u64, PawnInputKind>,
}

#[cfg(not(feature = "client"))]
impl PendingInputState {
    pub fn push(&mut self, seq: u64, input: PawnInputKind) {
        if seq <= self.applied_seq {
            merge_edges(&mut self.held, input);
            return;
        }
        if let Some(existing) = self.queue.get_mut(&seq) {
            merge_biped_edges(existing, input);
            return;
        }
        self.queue.insert(seq, input);
        while self.queue.len() > 128 {
            self.queue.pop_first();
        }
    }

    pub fn next(&mut self) -> Option<(u64, PawnInputKind, bool)> {
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
fn merge_edges(held: &mut Option<PawnInputKind>, input: PawnInputKind) {
    match held {
        Some(held) => merge_biped_edges(held, input),
        None => *held = Some(input),
    }
}

#[cfg(not(feature = "client"))]
fn merge_biped_edges(current: &mut PawnInputKind, incoming: PawnInputKind) {
    let (PawnInputKind::Biped(current), PawnInputKind::Biped(incoming)) = (current, incoming)
    else {
        return;
    };
    current.item.primary_pressed |= incoming.item.primary_pressed;
    current.item.secondary_pressed |= incoming.item.secondary_pressed;
    current.item.reload_pressed |= incoming.item.reload_pressed;
    current.ability1_pressed |= incoming.ability1_pressed;
    current.melee_pressed |= incoming.melee_pressed;
    if incoming.item.primary_pressed
        || incoming.item.secondary_pressed
        || incoming.item.reload_pressed
    {
        current.item.weapon = incoming.item.weapon;
        current.item.tick = incoming.item.tick;
        current.item.origin = incoming.item.origin;
        current.item.aim_dir = incoming.item.aim_dir;
    }
}

#[cfg(not(feature = "client"))]
fn clear_biped_edges(input: &mut PawnInputKind) {
    if let PawnInputKind::Biped(input) = input {
        input.item.primary_pressed = false;
        input.item.secondary_pressed = false;
        input.item.reload_pressed = false;
        input.ability1_pressed = false;
        input.melee_pressed = false;
    }
}

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub struct PendingMeleeHits(pub HashMap<ConnectionId, NetworkID>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct LastProcessedInputSeq(pub HashMap<ConnectionId, u64>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Connections that have completed transport setup and are ready for initial world sync.
pub struct PendingConnections(pub std::collections::HashSet<ConnectionId>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Connections currently considered active by the session layer.
pub struct ActiveConnections(pub std::collections::HashSet<ConnectionId>);
