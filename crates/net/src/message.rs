use bevy::{
    math::{Quat, Vec3},
    prelude::*,
};
pub use common::{
    BodyState, GameObjectKind, LeaderboardScope, NetworkID, NetworkIDResource, PawnInputKind,
    ScoringOption, SimulationState, WeaponState,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    pub net_id: NetworkID,
    pub position: Vec3,
    pub starting_velocity: Vec3,
    pub shooter_velocity: Vec3,
    pub rotation: Quat,
    pub server_tick: u64,
    pub kind: GameObjectKind,
}

/// Compact replicated scoreboard data used by the client HUD.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ScoreboardEntry {
    pub net_id: NetworkID,
    pub label: String,
    pub team: u8,
    pub value: i32,
}

/// Match-level scoreboard metadata plus either per-player or per-team rows.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ScoreboardSnapshot {
    pub teams_enabled: bool,
    pub scoring: ScoringOption,
    pub leaderboard_scope: LeaderboardScope,
    pub time_limit_secs: f32,
    pub leaderboard_label: String,
    pub primary_objective_label: String,
    pub players: Vec<ScoreboardEntry>,
    pub teams: Vec<ScoreboardEntry>,
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
    DropWeapon(Vec3),
    DropAbility(Vec3),
    SetActiveWeaponSlot(bool),
    WeaponPickup(NetworkID, NetworkID),
    WeaponDrop(NetworkID, NetworkID, Vec3),
    BipedLook(NetworkID, f32, f32),
    ReloadWeapon(NetworkID),
    FireRequest { weapon: NetworkID, kind: GameObjectKind, temp_id: u32, origin: Vec3, dir: Vec3 },
    ProjectileConfirm { temp_id: u32, net_id: NetworkID },
    HitResult(Vec3, Vec3, Option<NetworkID>),
    HealthUpdate(NetworkID, f32),
    TimePing(u64),
    TimePong(u64),
    OnscreenMessage(String),
    JetpackFx(NetworkID, bool),
    DashFx(NetworkID, Vec3),
    AbilityPickup(NetworkID, NetworkID),
    WeaponState(NetworkID, WeaponState),
    Scoreboard(ScoreboardSnapshot),
    MapHash(String),
    FileData(String, Vec<u8>),
}
