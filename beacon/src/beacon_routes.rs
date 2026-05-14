use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::Html,
};
use http_common::{LobbyHeartbeat, LobbyInfo, RegisterRequest, RegisterResponse};

use crate::{
    AppState,
    ui::{self, Page},
};

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
    State(state): State<AppState>,
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
    state.lobbies.lock().unwrap().insert(id.clone(), lobby);
    Json(RegisterResponse { id })
}

pub(crate) async fn list_json(State(state): State<AppState>) -> Json<Vec<LobbyInfo>> {
    Json(state.lobbies.lock().unwrap().values().cloned().collect())
}

pub(crate) async fn delete(Path(id): Path<String>, State(state): State<AppState>) -> StatusCode {
    state.lobbies.lock().unwrap().remove(&id);
    StatusCode::NO_CONTENT
}

pub(crate) async fn heartbeat(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<LobbyHeartbeat>,
) -> StatusCode {
    let mut lobbies = state.lobbies.lock().unwrap();
    let Some(lobby) = lobbies.get_mut(&id) else {
        return StatusCode::NOT_FOUND;
    };
    lobby.player_count = req.player_count.min(req.max_players);
    lobby.max_players = req.max_players.max(1);
    StatusCode::NO_CONTENT
}

pub(crate) async fn list_partial(State(state): State<AppState>) -> Html<String> {
    let lobbies = state.lobbies.lock().unwrap();
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
