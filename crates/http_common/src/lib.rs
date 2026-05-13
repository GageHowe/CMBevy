use serde::{Deserialize, Serialize};

// types used by the beacon, accessible to all

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityProvider {
    Steam,
    Xbox,
    Standalone,
}

impl IdentityProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Xbox => "xbox",
            Self::Standalone => "standalone",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountIdentity {
    pub provider: IdentityProvider,
    pub provider_user_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountProfile {
    pub account_id: String,
    pub display_name: String,
    pub identities: Vec<AccountIdentity>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AuthSessionResponse {
    pub session_token: String,
    pub account: AccountProfile,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StandaloneRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StandaloneLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProviderLoginRequest {
    pub provider: IdentityProvider,
    pub provider_user_id: String,
    pub display_name: String,
    pub proof: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ProviderLinkRequest {
    pub session_token: String,
    pub provider: IdentityProvider,
    pub provider_user_id: String,
    pub proof: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LogoutRequest {
    pub session_token: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LobbyInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub player_count: u8,
    pub max_players: u8,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RegisterRequest {
    pub quic_port: u16,
    pub name: String,
    pub max_players: u8,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub id: String,
}
