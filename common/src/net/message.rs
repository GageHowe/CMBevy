use crate::types::{CMQuat, CMVec3};
use bevy::prelude::*;
use std::collections::HashMap;
use wincode_derive::{SchemaRead, SchemaWrite};
use std::str::FromStr;
use crate::pawn::pawn::PawnInput;

/// component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Eq, Clone, Component, Hash)]
pub struct NetworkID(pub u64);

/// Like the Ticker resource, keeps track of the next available NetworkID to use
#[derive(Resource, Default)]
pub struct NetworkIDResource {
    pub last_id: u64
}
impl NetworkIDResource {
    /// should be used when spawning a new networked entity
    pub fn get_next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
}

/// Discriminates what kind of entity a SpawnCommand creates.
#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum SpawnKind {
    /// A biped pawn. `bool` is true when this is the local player's own pawn.
    Biped(bool),
    Rifle,
    Shotgun,
    // Add new weapon types here — each routes to its own spawn + add_visuals on the client.
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    pub net_id: NetworkID,
    pub position: CMVec3,
    pub starting_velocity: CMVec3,
    pub rotation: CMQuat,
    /// Server tick at spawn time — client uses this to synchronize its clock.
    pub server_tick: u64,
    pub kind: SpawnKind,
}

#[derive(SchemaWrite, SchemaRead, Debug, Clone, PartialEq)]
pub struct PawnInputMessage {
    pub input: PawnInput,
    pub tick: u64,
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub enum MsgType {
    // --- connection lifecycle (local only, never sent over the wire) ---
    Connected,
    Disconnected,
    // networked messages
    ChatMessage(String, String),
    /// probably will be displayed in the player's console / chatbox
    Error(String),
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
    /// Client → Server: (weapon_net_id, tick, origin, direction). Fire the held weapon.
    Fire(NetworkID, u64, CMVec3, CMVec3),
    /// Server → All: (origin, end, hit_net_id). Hitscan result for visual effects.
    HitResult(CMVec3, CMVec3, Option<NetworkID>),
}
impl FromStr for MsgType {
    type Err = String;

    /// try to recognize a user-provided command for runtime debugging purposes
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (cmd, rest) = s.split_once(' ').unwrap_or((s, ""));

        match cmd {
            "chat" => {
                let (sender, msg) = rest.split_once(' ')
                    .ok_or("usage: chat <sender> <message>")?;
                Ok(MsgType::ChatMessage(sender.to_string(), msg.to_string()))
            }
            "error" => Ok(MsgType::Error(rest.to_string())),
            "ping" => Ok(MsgType::Ping(rest.to_string())),
            "pong" => Ok(MsgType::Pong(rest.to_string())),
            _ => Err(format!("unknown command: {cmd}")),
        }
    }
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: CMVec3,
    pub rotation: CMQuat,
    pub linvel: CMVec3,
    pub angvel: CMVec3,
}

#[derive(SchemaWrite, SchemaRead, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    pub bodies: HashMap<NetworkID, BodyState>,
}
