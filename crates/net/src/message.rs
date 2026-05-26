use bevy::{
    math::{Quat, Vec3},
    prelude::*,
};
pub use common::{
    BodyState, LeaderboardScope, NetworkID, NetworkIDResource, PawnInputKind, ScoringOption,
    SimulationState, WeaponState,
};
use serde::{Deserialize, Serialize};

pub use crate::replication::ComponentUpdate;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Hash, Default, Reflect)]
#[serde(rename_all = "snake_case")]
#[reflect(Default)]
pub enum SpawnType {
    #[default]
    Biped,
    Spaceship,
    SpaceshipShield,
    Fighter,
    Truck,
    Hovercraft,
    Planet,
    Shotgun,
    Pistol,
    Beamer,
    Rifle,
    Smg,
    Failsafe,
    HailMary,
    Thumper,
    Lobber,
    CoilLauncher,
    TetherGun,
    Jetpack,
    Dash,
}

impl SpawnType {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "biped" => Self::Biped,
            "spaceship" => Self::Spaceship,
            "spaceship_shield" | "shield" => Self::SpaceshipShield,
            "fighter" => Self::Fighter,
            "truck" => Self::Truck,
            "hovercraft" => Self::Hovercraft,
            "planet" => Self::Planet,
            "shotgun" => Self::Shotgun,
            "pistol" => Self::Pistol,
            "beamer" => Self::Beamer,
            "rifle" => Self::Rifle,
            "smg" => Self::Smg,
            "failsafe" => Self::Failsafe,
            "hail_mary" | "hailmary" => Self::HailMary,
            "thumper" => Self::Thumper,
            "lobber" => Self::Lobber,
            "coil_launcher" | "coillauncher" => Self::CoilLauncher,
            "tether_gun" | "tethergun" => Self::TetherGun,
            "jetpack" => Self::Jetpack,
            "dash" => Self::Dash,
            _ => return None,
        })
    }
}

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
    pub position: Vec3,
    pub starting_velocity: Vec3,
    /// Inherited platform/shooter velocity used by some projectile logic. (TODO: remove this if possible, just have callers add it to velocity, or get velocity from local)
    pub shooter_velocity: Vec3,
    /// Initial world-space rotation.
    pub rotation: Quat,
    /// Authoritative server tick the spawn occurred on.
    pub server_tick: u64,
    /// Concrete game object type to instantiate.
    pub spawn_type: SpawnType,
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
        temp_id: u32,
        origin: Vec3,
        dir: Vec3,
    },
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
    DetonateFailsafeRequest(NetworkID),
    ProjectileConfirm {
        temp_id: u32,
        net_id: NetworkID,
    },
    HitResult(Vec3, Vec3, Option<NetworkID>),
    TimePing(u64),
    TimePong(u64),
    OnscreenMessage(String),
    JetpackFx(NetworkID, bool),
    DashFx(NetworkID, Vec3),
    AbilityPickup(NetworkID, NetworkID),
    ComponentUpdate(ComponentUpdate),
    Scoreboard(ScoreboardSnapshot),
    MapHash(String),
    FileData(String, Vec<u8>),
}
