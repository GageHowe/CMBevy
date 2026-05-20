use bevy::{
    math::{Quat, Vec3},
    prelude::*,
};
pub use common::{
    BodyState, GameObjectKind, LeaderboardScope, NetworkID, NetworkIDResource, PawnInputKind,
    ScoringOption, SimulationState, WeaponState,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JoinPlatform {
    Guest,
    Steam,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ClientHello {
    pub display_name: String,
    pub platform: JoinPlatform,
    pub proof: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct JoinAccepted {
    pub display_name: String,
    pub platform: JoinPlatform,
    pub verified_platform_user_id: Option<String>,
}

/// Type-erased spawn payload used for all replicated game objects.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnCommand {
    /// Network id the spawned object should own once it exists on the receiver.
    pub net_id: NetworkID,
    /// Initial world-space translation.
    pub position: Vec3,
    /// Initial world-space linear velocity of the spawned object itself.
    pub starting_velocity: Vec3,
    /// Inherited platform/shooter velocity used by some projectile logic.
    pub shooter_velocity: Vec3,
    /// Initial world-space rotation.
    pub rotation: Quat,
    /// Authoritative server tick the spawn occurred on.
    pub server_tick: u64,
    /// Concrete game object type to instantiate.
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
/// Transport-level message enum shared by client and server.
pub enum MsgType {
    Connected,
    JoinChallenge(String),
    ClientHello(ClientHello),
    JoinAccepted(JoinAccepted),
    JoinRejected(String),
    ClientReady,
    RequestMap,
    Disconnected,
    ChatMessage(String, String),
    Ping(String),
    Pong(String),
    Input(u64, PawnInputKind),
    /// map of NetworkID to rigidbody state
    State(SimulationState),
    SpawnCommand(SpawnCommand),
    DespawnCommand(NetworkID),
    Possess(NetworkID),
    MountState(NetworkID, Option<NetworkID>),
    Interact(NetworkID),
    DropWeapon(Vec3),
    DropAbility(Vec3),
    SetActiveWeaponSlot(bool),
    WeaponPickup(NetworkID, NetworkID),
    WeaponDrop(NetworkID, NetworkID, Vec3),
    /// client-authoritative: "I am looking with this yaw and pitch"
    PawnLook(NetworkID, f32, f32),
    ReloadWeapon(NetworkID),
    MeleeHitRequest(NetworkID),
    FireRequest {
        weapon: NetworkID,
        kind: GameObjectKind,
        temp_id: u32,
        origin: Vec3,
        dir: Vec3,
    },
    StartBeamCharge(NetworkID),
    StartBeam {
        weapon: NetworkID,
        origin: Vec3,
        dir: Vec3,
    },
    BeamHitReport {
        weapon: NetworkID,
        origin: Vec3,
        dir: Vec3,
        target: Option<NetworkID>,
    },
    EndBeam(NetworkID),
    DetonateFailsafeRequest(NetworkID),
    ProjectileConfirm {
        temp_id: u32,
        net_id: NetworkID,
    },
    HitResult(Vec3, Vec3, Option<NetworkID>),
    /// server -> client: "this entity has this health"
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
