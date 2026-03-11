use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::game_objects::pawn::pawn::PawnInput;
pub use crate::game_objects::GameObjectKind;

/// component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Component, Hash)]
pub struct NetworkID(pub u64);

/// Like the Ticker resource, keeps track of the next available NetworkID to use
#[derive(Resource, Default)]
pub struct NetworkIDResource {
    pub last_id: u64
}
impl NetworkIDResource {
    /// should be used when spawning a new networked entity
    pub fn get_next_free_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    pub net_id: NetworkID,
    pub position: Vec3,
    pub starting_velocity: Vec3,
    pub rotation: Quat,
    /// server tick at spawn time; client uses this to synchronize its clock.
    /// do we actually need this?
    pub server_tick: u64,
    pub kind: GameObjectKind,
    /// True only for the single recipient that owns/possesses this object.
    pub owned: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PawnInputMessage {
    pub input: PawnInput,
    pub tick: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum MsgType {
    // --- connection lifecycle (local only, never sent over the wire) ---
    Connected,
    Disconnected,
    // networked messages
    ChatMessage(String, String),
    Ping(String),
    Pong(String),
    Input(PawnInputMessage),
    /// collection of BodyStates with corresponding network ids
    State(SimulationState),
    SpawnCommand(SpawnCommand),
    DespawnCommand(NetworkID),
    /// Client → Server: request to interact with the entity identified by NetworkID.
    Interact(NetworkID),
    /// Server → All: (weapon_id, carrier_net_id). Clients remove the weapon entity;
    /// the carrier client records it as their held weapon.
    WeaponPickup(NetworkID, NetworkID),
    /// Client → Server: (weapon_net_id, origin, direction). Fire the held weapon.
    Fire(NetworkID, Vec3, Vec3),
    /// Server → All: (origin, end, hit_net_id). Hitscan result for visual effects.
    /// wtf? why vfx? this will be outdated and since clients move very fast this will not be a good solution.
    /// instead, send the shooter entity (the gun) and the direction/magnitude vector, plus the target.
    HitResult(Vec3, Vec3, Option<NetworkID>),
    /// Server → All: current health for the given entity.
    HealthUpdate(NetworkID, f32),
    /// Client → Server: timestamp echo request. Payload is the bits of an f64 elapsed time.
    TimePing(u64),
    /// Server → Client: echoes the TimePing payload unchanged.
    TimePong(u64),
    /// Server → Client: file transfer. `data` is zstd-compressed at level 9;
    /// decompress with `zstd::stream::decode_all` to recover the original bytes.
    FileData(String, Vec<u8>),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: Vec3,
    pub rotation: Quat,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    pub bodies: HashMap<NetworkID, BodyState>,
}
