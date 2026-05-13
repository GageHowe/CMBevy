use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_common::{AuthSessionResponse, IdentityProvider, LogoutRequest, ProviderLinkRequest,
    ProviderLoginRequest, StandaloneLoginRequest, StandaloneRegisterRequest};
use serde::Deserialize;

use crate::{AppState, db};

#[derive(Clone)]
pub(crate) struct SteamAuthConfig {
    app_id: u32,
    publisher_key: Option<String>,
    identity: String,
}

impl SteamAuthConfig {
    pub(crate) fn from_env() -> Self {
        Self {
            app_id: std::env::var("STEAM_APP_ID")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(3526510),
            publisher_key: std::env::var("STEAM_PUBLISHER_KEY").ok(),
            identity: std::env::var("STEAM_WEBAPI_IDENTITY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "beacon".to_string()),
        }
    }
}

pub(crate) async fn register_standalone(
    State(state): State<AppState>,
    Json(req): Json<StandaloneRegisterRequest>,
) -> Result<Json<AuthSessionResponse>, ApiError> {
    Ok(Json(db::create_standalone_account(&state.db, &req)?))
}

pub(crate) async fn login_standalone(
    State(state): State<AppState>,
    Json(req): Json<StandaloneLoginRequest>,
) -> Result<Json<AuthSessionResponse>, ApiError> {
    Ok(Json(db::login_standalone_account(&state.db, &req)?))
}

pub(crate) async fn login_provider(
    State(state): State<AppState>,
    Json(mut req): Json<ProviderLoginRequest>,
) -> Result<Json<AuthSessionResponse>, ApiError> {
    req.provider_user_id = verify_provider_proof(&state, req.provider, &req.proof)?;
    Ok(Json(db::login_provider_account(&state.db, &req)?))
}

pub(crate) async fn link_provider(
    State(state): State<AppState>,
    Json(mut req): Json<ProviderLinkRequest>,
) -> Result<Json<AuthSessionResponse>, ApiError> {
    req.provider_user_id = verify_provider_proof(&state, req.provider, &req.proof)?;
    Ok(Json(db::link_provider_identity(&state.db, &req)?))
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    Json(req): Json<LogoutRequest>,
) -> Result<StatusCode, ApiError> {
    db::delete_session(&state.db, &req.session_token)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn session(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<AuthSessionResponse>, ApiError> {
    Ok(Json(db::session_by_token(&state.db, &token)?))
}

pub(crate) struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl ApiError {
    fn new(status: StatusCode, message: &'static str) -> Self {
        Self { status, message }
    }

    pub(crate) fn bad_request(message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub(crate) fn unauthorized(message: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub(crate) fn conflict(message: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub(crate) fn internal(message: &'static str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub(crate) fn unavailable(message: &'static str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

fn verify_provider_proof(
    state: &AppState,
    provider: IdentityProvider,
    proof: &str,
) -> Result<String, ApiError> {
    if proof.trim().is_empty() {
        return Err(ApiError::bad_request("missing provider proof"));
    }
    match provider {
        IdentityProvider::Steam => verify_steam_ticket(&state.steam, proof),
        IdentityProvider::Xbox => Err(ApiError::bad_request("xbox auth is not implemented")),
        IdentityProvider::Standalone => {
            Err(ApiError::bad_request("use standalone login for standalone accounts"))
        }
    }
}

fn verify_steam_ticket(config: &SteamAuthConfig, ticket: &str) -> Result<String, ApiError> {
    let Some(key) = &config.publisher_key else {
        return Err(ApiError::unavailable("steam auth is not configured"));
    };
    let response = ureq::get("https://partner.steam-api.com/ISteamUserAuth/AuthenticateUserTicket/v1/")
        .query("key", key)
        .query("appid", &config.app_id.to_string())
        .query("ticket", ticket.trim())
        .query("identity", &config.identity)
        .call()
        .map_err(|_| ApiError::unauthorized("steam ticket verification failed"))?;
    let payload = response
        .into_json::<SteamAuthTicketResponse>()
        .map_err(|_| ApiError::unauthorized("steam ticket verification failed"))?;
    let params = payload
        .response
        .params
        .ok_or(ApiError::unauthorized("steam ticket rejected"))?;
    if params.result.as_deref() != Some("OK") {
        return Err(ApiError::unauthorized("steam ticket rejected"));
    }
    params
        .steamid
        .filter(|steamid| !steamid.trim().is_empty())
        .ok_or(ApiError::unauthorized("steam ticket rejected"))
}

#[derive(Deserialize)]
struct SteamAuthTicketResponse {
    response: SteamAuthTicketEnvelope,
}

#[derive(Deserialize)]
struct SteamAuthTicketEnvelope {
    params: Option<SteamAuthTicketParams>,
}

#[derive(Deserialize)]
struct SteamAuthTicketParams {
    result: Option<String>,
    steamid: Option<String>,
}
