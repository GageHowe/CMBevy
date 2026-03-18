use bevy::math::{Quat, Vec3};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
pub use common::{NetworkID, NetworkIDResource, BodyState, SimulationState, GameObjectKind};
use common::PawnInput;

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
    /// let's refactor this so that Owner is a NetworkID
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
    /// Depending on the entity's implementation(s), this could be equipping a weapon,
    /// getting into a vehicle, etc.
    Interact(NetworkID),
    /// Server → All: (weapon_id, carrier_net_id). The weapon's physics body is disabled;
    /// the carrier client attaches it as a viewmodel.
    WeaponPickup(NetworkID, NetworkID),
    /// reliably server -> all (weapon_id, carrier_net_id, drop_position)
    /// TODO: add velocity, should inherit the velocity of the player who dropped or was killed
    WeaponDrop(NetworkID, NetworkID, Vec3),
    /// Client → Server: (weapon_net_id, origin, direction, client_tick). Fire the held weapon.
    /// TODO: this should be refactored; one MsgType variant per weapon type.
    Fire(NetworkID, Vec3, Vec3, u64),
    /// Server → All: (origin, end, hit_net_id). Hitscan result for visual effects.
    /// wtf? why vfx? this will be outdated and since clients move very fast this will not be a good solution.
    /// instead, send the shooter entity (the gun) and the direction/magnitude vector, plus the target.
    HitResult(Vec3, Vec3, Option<NetworkID>),

    /// updates clients with the current health for the given entity.
    HealthUpdate(NetworkID, f32),
    /// Client → Server: timestamp echo request. Payload is the bits of an f64 elapsed time.
    TimePing(u64),
    /// Server → Client: echoes the TimePing payload unchanged.
    TimePong(u64),
    /// Client → Server: toggle my flashlight.
    FlashlightToggle,
    /// Server → All: flashlight state for the given entity.
    FlashlightState(NetworkID, bool),
    /// Server → Client: file transfer. `data` is zstd-compressed at level 9;
    /// decompress with `zstd::stream::decode_all` to recover the original bytes.
    FileData(String, Vec<u8>),
}
