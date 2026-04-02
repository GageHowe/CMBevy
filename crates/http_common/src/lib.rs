use serde::{Deserialize, Serialize};

// types used by the beacon, accessible to all

#[derive(Serialize, Deserialize, Clone)]
pub struct LobbyInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub player_count: u8,
    pub max_players: u8,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterRequest {
    pub quic_port: u16,
    pub name: String,
    pub max_players: u8,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub id: String,
}
