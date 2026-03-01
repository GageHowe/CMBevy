mod config;

use axum::{
    extract::State,
    response::Html,
    routing::get,
    Router,
};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use axum::response::Response;
use axum::http::header;

#[derive(Serialize, Deserialize, Clone)]
struct LobbyInfo {
    id: String,
    name: String,
    host: String,
    player_count: u8,
    max_players: u8,
}

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
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    println!("Listening on http://0.0.0.0:8000");
    axum::serve(listener, app).await.unwrap();
}

async fn serve_ui() -> Html<&'static str> {
    Html(include_str!("web/index.html"))
}
async fn serve_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(include_str!("web/theme.css").into())
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
