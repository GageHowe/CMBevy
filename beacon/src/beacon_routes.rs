use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, OnceLock},
};

use axum::{
    Json,
    extract::{ConnectInfo, Path},
    http::StatusCode,
    response::Html,
};
use http_common::{
    JoinLobbyResponse, JoinStatusResponse, LobbyHeartbeat, LobbyInfo, PendingPeersResponse,
    RegisterRequest, RegisterResponse,
};

use crate::{
    RendezvousState,
    ui::{self, Page},
};

static LOBBIES: OnceLock<Arc<Mutex<HashMap<String, LobbyInfo>>>> = OnceLock::new();
static RENDEZVOUS: OnceLock<Arc<Mutex<RendezvousState>>> = OnceLock::new();

pub(crate) fn init_state(
    lobbies: Arc<Mutex<HashMap<String, LobbyInfo>>>,
    rendezvous: Arc<Mutex<RendezvousState>>,
) {
    let _ = LOBBIES.set(lobbies);
    let _ = RENDEZVOUS.set(rendezvous);
}

fn lobbies() -> &'static Arc<Mutex<HashMap<String, LobbyInfo>>> {
    LOBBIES.get().expect("lobbies initialized")
}

fn rendezvous() -> &'static Arc<Mutex<RendezvousState>> {
    RENDEZVOUS.get().expect("rendezvous initialized")
}

pub(crate) async fn serve_home() -> Html<String> {
    ui::page(
        "Critical Mass",
        Page::Home,
        include_str!("static/home_body.html"),
        "",
    )
}

pub(crate) async fn serve_ui() -> Html<String> {
    ui::page(
        "Critical Mass Custom Games",
        Page::Lobbies,
        include_str!("static/beacon_body.html"),
        include_str!("static/beacon.js"),
    )
}

pub(crate) async fn register(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RegisterRequest>,
) -> Json<RegisterResponse> {
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    let lobby = LobbyInfo {
        id: id.clone(),
        name: req.name,
        host: format!("{}:{}", addr.ip(), req.quic_port),
        player_count: 0,
        max_players: req.max_players,
    };
    lobbies().lock().unwrap().insert(id.clone(), lobby);
    Json(RegisterResponse { id })
}

pub(crate) async fn list_json() -> Json<Vec<LobbyInfo>> {
    Json(lobbies().lock().unwrap().values().cloned().collect())
}

pub(crate) async fn delete(Path(id): Path<String>) -> StatusCode {
    lobbies().lock().unwrap().remove(&id);
    StatusCode::NO_CONTENT
}

pub(crate) async fn heartbeat(
    Path(id): Path<String>,
    Json(req): Json<LobbyHeartbeat>,
) -> StatusCode {
    let mut lobbies = lobbies().lock().unwrap();
    let Some(lobby) = lobbies.get_mut(&id) else {
        return StatusCode::NOT_FOUND;
    };
    lobby.player_count = req.player_count.min(req.max_players);
    lobby.max_players = req.max_players.max(1);
    StatusCode::NO_CONTENT
}

pub(crate) async fn join(Path(id): Path<String>) -> Result<Json<JoinLobbyResponse>, StatusCode> {
    if !lobbies().lock().unwrap().contains_key(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let token = fastrand::u64(..).to_string();
    let mut rendezvous = rendezvous().lock().unwrap();
    let host = rendezvous.hosts.get(&id).map(ToString::to_string);
    rendezvous.tokens.insert(token.clone(), id);
    Ok(Json(JoinLobbyResponse { host, token }))
}

pub(crate) async fn pending_peers(Path(id): Path<String>) -> Json<PendingPeersResponse> {
    let peers = rendezvous()
        .lock()
        .unwrap()
        .pending
        .remove(&id)
        .unwrap_or_default()
        .into_iter()
        .map(|addr| addr.to_string())
        .collect();
    Json(PendingPeersResponse { peers })
}

pub(crate) async fn join_status(
    Path((id, token)): Path<(String, String)>,
) -> Result<Json<JoinStatusResponse>, StatusCode> {
    let rendezvous = rendezvous().lock().unwrap();
    if rendezvous
        .tokens
        .get(&token)
        .is_none_or(|value| value != &id)
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let host = rendezvous.hosts.get(&id).map(ToString::to_string);
    Ok(Json(JoinStatusResponse { host }))
}

pub(crate) async fn list_partial() -> Html<String> {
    let lobbies = lobbies().lock().unwrap();
    if lobbies.is_empty() {
        return Html(r#"<div class="empty">No active lobbies right now.</div>"#.into());
    }
    Html(
        lobbies
            .values()
            .map(|l| {
                let full = l.player_count >= l.max_players;
                let badge_class = if full { "badge full" } else { "badge open" };
                let badge_text = if full { "Full" } else { "Open" };
                format!(
                    r#"
                    <div class="lobby-card" data-name="{name}">
                        <span class="lobby-name">{name}</span>
                        <span class="lobby-host">hosted by {host}</span>
                        <div class="footer">
                            <span class="{badge_class}">{badge_text} · {pc}/{mp}</span>
                        </div>
                    </div>
                "#,
                    name = l.name,
                    host = l.host,
                    badge_class = badge_class,
                    badge_text = badge_text,
                    pc = l.player_count,
                    mp = l.max_players,
                )
            })
            .collect(),
    )
}
