mod config;

use axum::{
    extract::{ConnectInfo, State},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use axum::response::Response;
use axum::http::header;
use http_common::*;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    lobbies: Arc<Mutex<HashMap<String, LobbyInfo>>>,
}

#[tokio::main]
async fn main() {
    let conn = Connection::open("data.db").unwrap();
    conn.execute_batch(include_str!("../schema.sql")).unwrap();

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        lobbies: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/", get(serve_ui))
        .route("/theme.css", get(serve_css))
        .route("/lobbies/partial", get(list_lobbies_partial))
        .route("/lobbies/register", post(register_lobby))
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    println!("Listening on http://0.0.0.0:8000");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}

async fn register_lobby(
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

async fn serve_ui() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}
async fn serve_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(include_str!("static/theme.css").into())
        .unwrap()
}

async fn list_lobbies_partial(State(state): State<AppState>) -> Html<String> {
    let lobbies = state.lobbies.lock().unwrap();

    if lobbies.is_empty() {
        return Html(r#"<div class="empty">No active lobbies right now.</div>"#.into());
    }

    let html = lobbies.values().map(|l| {
        let full = l.player_count >= l.max_players;
        let badge_class = if full { "badge full" } else { "badge open" };
        let badge_text = if full { "Full" } else { "Open" };
        format!(r#"
            <div class="lobby-card" data-name="{name}">
                <span class="lobby-name">{name}</span>
                <span class="lobby-host">hosted by {host}</span>
                <div class="lobby-footer">
                    <span class="{badge_class}">{badge_text} · {pc}/{mp}</span>
                    <button class="ghost" hx-post="/join/{id}" hx-swap="none">Join</button>
                </div>
            </div>
        "#,
                name = l.name,
                host = l.host,
                badge_class = badge_class,
                badge_text = badge_text,
                pc = l.player_count,
                mp = l.max_players,
                id = l.id,
        )
    }).collect();

    Html(html)
}
