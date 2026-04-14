//! Authoritative match-level state shared by gametypes, scoring, and HUD replication.

use std::collections::HashMap;

use bevy::prelude::*;
use common::{LeaderboardScope, ScoringOption};
use net::quic::ConnectionId;

/// Script-defined presentation and rules metadata for the active mode.
#[derive(Resource, Clone)]
pub struct ModeConfig {
    pub respawn_delay: f32,
    pub teams_enabled: bool,
    pub scoring: ScoringOption,
    pub leaderboard_scope: LeaderboardScope,
    pub leaderboard_number_index: usize,
    pub time_limit_secs: f32,
    pub team_count: u8,
    pub leaderboard_label: String,
    pub primary_objective_label: String,
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            respawn_delay: common::config::RESPAWN_DELAY_SECS,
            teams_enabled: false,
            scoring: ScoringOption::ScoreToWin(50),
            leaderboard_scope: LeaderboardScope::Player,
            leaderboard_number_index: 0,
            time_limit_secs: 600.0,
            team_count: 2,
            leaderboard_label: "Score".to_string(),
            primary_objective_label: "Eliminate enemies".to_string(),
        }
    }
}

/// Per-connection number vectors owned by the authoritative match/session.
#[derive(Resource, Default)]
pub struct PlayerNumbers(pub HashMap<ConnectionId, Vec<i32>>);

/// Per-team number vectors owned by the authoritative match/session.
#[derive(Resource, Default)]
pub struct TeamNumbers(pub HashMap<u8, Vec<i32>>);

/// Coarse authoritative round phase controlled by script-visible match hooks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    Playing,
    PostGame,
}

/// Minimal server-owned round state. Scripts decide when a game ends; Rust only tracks
/// the current phase and applies restart requests authoritatively.
#[derive(Resource)]
pub struct MatchState {
    pub phase: MatchPhase,
    pub phase_elapsed_secs: f32,
    pub winner_player: Option<ConnectionId>,
    pub winner_team: Option<u8>,
    pub restart_requested: bool,
}

impl Default for MatchState {
    fn default() -> Self {
        Self {
            phase: MatchPhase::Playing,
            phase_elapsed_secs: 0.0,
            winner_player: None,
            winner_team: None,
            restart_requested: false,
        }
    }
}
