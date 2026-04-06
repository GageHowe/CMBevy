use axum::{
    body::Bytes,
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::Html,
    routing::{delete, get, post},
};
use rusqlite::Connection;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
// use serde::{Deserialize, Serialize};
use axum::http::header;
use axum::response::Response;
use http_common::*;

const FILENAME_HEADER: &str = "x-asset-filename";

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
        .route("/lobbies", get(list_lobbies_json))
        .route("/lobbies/partial", get(list_lobbies_partial))
        .route("/lobbies/register", post(register_lobby))
        .route("/lobbies/{id}", delete(delete_lobby))
        .route("/assets/{hash}", get(get_asset).put(put_asset))
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    println!("Listening on http://0.0.0.0:8000");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
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

async fn list_lobbies_json(State(state): State<AppState>) -> Json<Vec<LobbyInfo>> {
    Json(state.lobbies.lock().unwrap().values().cloned().collect())
}

async fn delete_lobby(Path(id): Path<String>, State(state): State<AppState>) -> StatusCode {
    state.lobbies.lock().unwrap().remove(&id);
    StatusCode::NO_CONTENT
}

async fn list_lobbies_partial(State(state): State<AppState>) -> Html<String> {
    let lobbies = state.lobbies.lock().unwrap();

    if lobbies.is_empty() {
        return Html(r#"<div class="empty">No active lobbies right now.</div>"#.into());
    }

    let html = lobbies
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
        })
        .collect();

    Html(html)
}

async fn get_asset(Path(hash): Path<String>) -> Result<Response, StatusCode> {
    let path = asset_path(&hash);
    let bytes = std::fs::read(path).map_err(|_| StatusCode::NOT_FOUND)?;
    let file_name = std::fs::read_to_string(asset_meta_path(&hash)).ok();
    let mut builder = Response::builder();
    if let Some(file_name) = file_name.as_deref() {
        builder = builder.header(FILENAME_HEADER, file_name.trim());
    }
    builder.body(bytes.into()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn put_asset(Path(hash): Path<String>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let path = asset_path(&hash);
    let Some(dir) = path.parent() else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    match std::fs::write(&path, body) {
        Ok(_) => {
            if let Some(file_name) = headers.get(FILENAME_HEADER).and_then(|value| value.to_str().ok()) {
                let _ = std::fs::write(asset_meta_path(&hash), file_name);
            }
            StatusCode::CREATED
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn asset_path(hash: &str) -> PathBuf {
    asset_dir().join(sanitize_hash(hash))
}

fn asset_meta_path(hash: &str) -> PathBuf {
    asset_dir().join(format!("{}.name", sanitize_hash(hash)))
}

fn asset_dir() -> PathBuf {
    if FsPath::new("asset_blobs").exists() || !cfg!(debug_assertions) {
        PathBuf::from("asset_blobs")
    } else {
        PathBuf::from("../asset_blobs")
    }
}

fn sanitize_hash(hash: &str) -> String {
    hash.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '_',
        })
        .collect()
}
