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
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Reflect, Default)]
pub enum LeaderboardScope {
    None,
    #[default]
    Player,
    Team,
}
