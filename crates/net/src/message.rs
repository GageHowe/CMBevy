use bevy::{
    math::{Quat, Vec3},
    prelude::*,
};
pub use common::{
    BodyState, LeaderboardScope, NetworkID, NetworkIDResource, PawnInput, ScoringOption,
    SimulationState, WeaponState,
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
    /// Optional network id of the parent entity this object should attach under.
    pub parent_net_id: Option<NetworkID>,
    pub position: Option<Vec3>,
    /// Initial world-space rotation.
    pub rotation: Option<Quat>,
    #[serde(alias = "starting_velocity")]
    pub velocity: Option<Vec3>,
    pub angular_velocity: Option<Vec3>,
    /// Authoritative server tick the spawn occurred on.
    pub server_tick: u64,
    /// Concrete game object name to instantiate.
    pub spawn_name: String,
}

impl SpawnCommand {
    pub fn new(net_id: NetworkID, spawn_name: impl Into<String>, server_tick: u64) -> Self {
        Self {
            net_id,
            parent_net_id: None,
            position: None,
            rotation: None,
            velocity: None,
            angular_velocity: None,
            server_tick,
            spawn_name: spawn_name.into(),
        }
    }

    pub fn parent(mut self, parent_net_id: NetworkID) -> Self {
        self.parent_net_id = Some(parent_net_id);
        self
    }

    pub fn position(mut self, position: Vec3) -> Self {
        self.position = Some(position);
        self
    }

    pub fn rotation(mut self, rotation: Quat) -> Self {
        self.rotation = Some(rotation);
        self
    }

    pub fn velocity(mut self, velocity: Vec3) -> Self {
        self.velocity = Some(velocity);
        self
    }

    pub fn angular_velocity(mut self, angular_velocity: Vec3) -> Self {
        self.angular_velocity = Some(angular_velocity);
        self
    }

    pub fn position_or_zero(&self) -> Vec3 {
        self.position.unwrap_or(Vec3::ZERO)
    }

    pub fn rotation_or_identity(&self) -> Quat {
        self.rotation.unwrap_or(Quat::IDENTITY)
    }

    pub fn velocity_or_zero(&self) -> Vec3 {
        self.velocity.unwrap_or(Vec3::ZERO)
    }

    pub fn angular_velocity_or_zero(&self) -> Vec3 {
        self.angular_velocity.unwrap_or(Vec3::ZERO)
    }
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum AbilityFx {
    Jetpack(bool),
    Dash(Vec3),
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
    Input(u64, PawnInput),
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
    MeleeHitRequest(NetworkID),
    ProjectileSpawn {
        weapon: NetworkID,
        net_id: NetworkID,
        position: Vec3,
        starting_velocity: Vec3,
        shooter_velocity: Vec3,
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
    ProjectileConfirm {
        temp_id: u32,
        net_id: NetworkID,
    },
    HitResult(Vec3, Vec3, Option<NetworkID>),
    TimePing(u64),
    TimePong(u64),
    OnscreenMessage(String),
    AbilityFx(NetworkID, AbilityFx),
    AbilityState(NetworkID, Option<String>),
    WeaponState(NetworkID, WeaponState),
    Health(NetworkID, i32, i32, i32, u16, u16, i32, i32),
    Scoreboard(ScoreboardSnapshot),
    MapHash(String),
    FileData(String, Vec<u8>),
}
