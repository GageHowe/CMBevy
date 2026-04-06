use bevy::math::{Quat, Vec3};
use bevy::prelude::*;
pub use common::{
    BodyState, GameObjectKind, NetworkID, NetworkIDResource, PawnInputKind, SimulationState,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    pub net_id: NetworkID,
    pub position: Vec3,
    pub starting_velocity: Vec3,
    pub rotation: Quat,
    pub server_tick: u64,
    pub kind: GameObjectKind,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum MsgType {
    Connected,
    ClientReady,
    RequestMap,
    Disconnected,
    ChatMessage(String, String),
    Ping(String),
    Pong(String),
    Input(u64, PawnInputKind),
    State(SimulationState),
    SpawnCommand(SpawnCommand),
    DespawnCommand(NetworkID),
    Possess(NetworkID),
    SeatState(NetworkID, Option<NetworkID>),
    Interact(NetworkID),
    WeaponPickup(NetworkID, NetworkID),
    WeaponDrop(NetworkID, NetworkID, Vec3),
    FireRequest {
        weapon: NetworkID,
        kind: GameObjectKind,
        temp_id: u32,
        origin: Vec3,
        dir: Vec3,
    },
    ProjectileConfirm {
        temp_id: u32,
        net_id: NetworkID,
    },
    HitResult(Vec3, Vec3, Option<NetworkID>),
    HealthUpdate(NetworkID, f32),
    TimePing(u64),
    TimePong(u64),
    FlashlightToggle,
    FlashlightState(NetworkID, bool),
    MapHash(String),
    FileData(String, Vec<u8>),
}
