use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// component to mark entities that should be networked.
/// NetworkID is managed by the server.
#[derive(
    Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Component, Hash, Reflect, Default,
)]
#[reflect(Component, Default)]
pub struct NetworkID(pub u64);

/// server-side resource that keeps track of the next available NetworkID to use
#[derive(Resource, Default)]
pub struct NetworkIDResource {
    last_id: u64,
}
impl NetworkIDResource {
    /// should be used when spawning a new networked entity
    pub fn next(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    pub fn reserve(&mut self, id: u64) {
        self.last_id = self.last_id.max(id);
    }
}

/// networked state of a dynamic rigidbody.
/// stable, do not touch.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BodyState {
    pub position: Vec3,
    pub rotation: Quat,
    pub linvel: Vec3,
    pub angvel: Vec3,
}

/// networked message for a set of rigidbodies.
/// stable, do not touch.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SimulationState {
    pub tick: u64,
    pub last_input_seq: u64,
    pub bodies: HashMap<NetworkID, BodyState>,
}

#[derive(
    Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Component, Reflect, Default,
)]
#[reflect(Component, Default)]
/// Replicated weapon state shared by client prediction, authority, and HUD.
pub struct WeaponState {
    /// Ammo currently loaded and ready to fire.
    pub ammo_in_mag: u16,
    /// Spare ammo available for reloads.
    pub reserve_ammo: u16,
    /// Remaining reload time in fixed ticks.
    pub reload_ticks: u16,
    /// Remaining fire cooldown in fixed ticks.
    pub cooldown_ticks: u16,
}

/// Match scoring policy shared between authoritative game state and the HUD.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Reflect)]
pub enum ScoringOption {
    Unscored,
    ScoreToWin(i32),
}

impl Default for ScoringOption {
    fn default() -> Self {
        Self::ScoreToWin(50)
    }
}

/// Chooses which counter set the HUD leaderboard should render for the active mode.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Reflect)]
pub enum LeaderboardScope {
    None,
    Player,
    Team,
}

impl Default for LeaderboardScope {
    fn default() -> Self {
        Self::Player
    }
}

/// update this as needed; it's a "Master List" defines types of game objects that can be spawned
/// this needs to stay in common since both net and game_objects access it
#[derive(Debug, PartialEq, Clone, Component, Serialize, Deserialize, Reflect, Default)]
#[reflect(Component, Default)]
/// Enumerates all spawnable replicated gameplay objects shared across binaries.
pub enum GameObjectKind {
    #[default]
    Biped,
    Spaceship,
    Fighter,
    Truck,
    RocketTurret,
    Planet,
    Pistol,
    Rifle,
    Shotgun,
    HailMary,
    HailMaryProjectile,
    Thumper,
    ThumperProjectile,
    PistolProjectile,
    RifleProjectile,
    Lobber,
    LobberProjectile,
    CoilLauncher,
    CoilLauncherProjectile,
    GrenadeLauncher,
    GrenadeLauncherProjectile,
    FighterRocketProjectile,
    Jetpack,
    Dash,
}

impl GameObjectKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "biped" => Self::Biped,
            "spaceship" => Self::Spaceship,
            "fighter" => Self::Fighter,
            "truck" => Self::Truck,
            "rocket_turret" => Self::RocketTurret,
            "planet" => Self::Planet,
            "pistol" => Self::Pistol,
            "rifle" => Self::Rifle,
            "shotgun" => Self::Shotgun,
            "hail_mary" => Self::HailMary,
            "thumper" => Self::Thumper,
            "rpg" => Self::Lobber,
            "coil_launcher" => Self::CoilLauncher,
            "grenade_launcher" => Self::GrenadeLauncher,
            "jetpack" => Self::Jetpack,
            "dash" => Self::Dash,
            _ => return None,
        })
    }

    /// splits CamelCase name -> "Camel Case"
    pub fn interaction_name(&self) -> String {
        let debug = format!("{self:?}");
        let mut out = String::with_capacity(debug.len() + 4);
        for (i, ch) in debug.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() {
                out.push(' ');
            }
            out.push(ch);
        }
        out
    }
}
