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
/// Requests a clean client exit back to the shell.
pub struct PendingExit(pub bool);

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
/// Gates the final "world ready" acknowledgement until map load finishes.
pub struct PendingWorldReady(pub bool);

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
pub(crate) struct BindAddr(pub std::net::SocketAddr);

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct ConsoleCommands(pub std::sync::Mutex<std::sync::mpsc::Receiver<String>>);

#[cfg(not(feature = "client"))]
#[derive(Resource)]
pub(crate) struct LevelPath(pub String);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
pub(crate) struct BodyHistory(pub HashMap<u64, SimulationState>);

#[cfg(not(feature = "client"))]
#[derive(Resource, Default)]
/// Latest input per connection waiting to be applied on the server tick.
pub struct PendingInputs(pub HashMap<ConnectionId, (u64, PawnInputKind)>);

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
