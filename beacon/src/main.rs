use axum::http::header;
use axum::response::Response;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::{Html, Redirect},
    routing::{delete, get, post},
};
use http_common::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};

const FILENAME_HEADER: &str = "x-asset-filename";

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    lobbies: Arc<Mutex<HashMap<String, LobbyInfo>>>,
}

#[derive(Deserialize)]
struct AssetListQuery {
    q: Option<String>,
}

#[derive(Serialize)]
struct AssetUploadResponse {
    hash: String,
    file_name: String,
    size_bytes: usize,
}

struct AssetEntry {
    hash: String,
    file_name: String,
    size_bytes: u64,
    updated_at: std::time::SystemTime,
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
        .route("/", get(|| async { Redirect::to("/assets") }))
        .route("/assets", get(serve_assets_ui).post(upload_asset))
        .route("/assets/list", get(list_assets_partial))
        .route("/theme.css", get(serve_css))
        .route("/beacon", get(serve_beacon_ui))
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

async fn serve_assets_ui() -> Html<&'static str> {
    Html(include_str!("static/assets.html"))
}

async fn serve_beacon_ui() -> Html<&'static str> {
    Html(include_str!("static/beacon.html"))
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

async fn upload_asset(
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AssetUploadResponse>, StatusCode> {
    if body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let file_name = headers
        .get(FILENAME_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(sanitize_file_name)
        .unwrap_or_else(|| "upload.bin".to_string());
    let hash = format!("sha256:{}", hex_sha256(&body));
    let path = asset_path(&hash);
    let Some(dir) = path.parent() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    std::fs::create_dir_all(dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(&path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(asset_meta_path(&hash), &file_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(AssetUploadResponse {
        hash,
        file_name,
        size_bytes: body.len(),
    }))
}

async fn list_assets_partial(query: axum::extract::Query<AssetListQuery>) -> Html<String> {
    let entries = load_asset_entries(query.q.as_deref());
    if entries.is_empty() {
        return Html(r#"<div class="empty">No matching assets found.</div>"#.to_string());
    }

    let html = entries
        .into_iter()
        .map(|entry| {
            let updated = format_system_time(entry.updated_at);
            let size = format_size(entry.size_bytes);
            format!(
                r#"
            <article class="asset-card" data-name="{name}" data-hash="{hash}">
                <div class="asset-card-head">
                    <div>
                        <h3>{name}</h3>
                        <p>{hash}</p>
                    </div>
                    <a class="ghost-link" href="/assets/{hash}" target="_blank" rel="noreferrer">Open</a>
                </div>
                <div class="asset-meta">
                    <span>{size}</span>
                    <span>{updated}</span>
                </div>
            </article>
        "#,
                name = escape_html(&entry.file_name),
                hash = escape_html(&entry.hash),
                size = escape_html(&size),
                updated = escape_html(&updated),
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
    builder
        .body(bytes.into())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
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
            if let Some(file_name) = headers
                .get(FILENAME_HEADER)
                .and_then(|value| value.to_str().ok())
            {
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
            'a'..='z' | 'A'..='Z' | '0'..='9' | ':' => ch,
            _ => '_',
        })
        .collect()
}

fn sanitize_file_name(name: &str) -> Option<String> {
    let file_name = FsPath::new(name).file_name()?.to_string_lossy();
    let sanitized: String = file_name
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '_',
        })
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn load_asset_entries(query: Option<&str>) -> Vec<AssetEntry> {
    let filter = query
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let mut entries = Vec::new();
    let Ok(dir) = std::fs::read_dir(asset_dir()) else {
        return entries;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "name") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let hash = entry.file_name().to_string_lossy().into_owned();
        let file_name = std::fs::read_to_string(asset_meta_path(&hash))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| hash.clone());
        if let Some(filter) = filter.as_deref() {
            let haystack = format!(
                "{} {}",
                file_name.to_ascii_lowercase(),
                hash.to_ascii_lowercase()
            );
            if !haystack.contains(filter) {
                continue;
            }
        }
        entries.push(AssetEntry {
            hash,
            file_name,
            size_bytes: meta.len(),
            updated_at: meta.modified().unwrap_or(std::time::UNIX_EPOCH),
        });
    }
    entries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    entries
}

fn format_system_time(time: std::time::SystemTime) -> String {
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => format!("updated {}s ago", elapsed_seconds(duration.as_secs())),
        Err(_) => "updated just now".to_string(),
    }
}

fn elapsed_seconds(timestamp: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(timestamp)
}

fn format_size(size_bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if size_bytes >= MIB {
        format!("{:.1} MiB", size_bytes as f64 / MIB as f64)
    } else if size_bytes >= KIB {
        format!("{:.1} KiB", size_bytes as f64 / KIB as f64)
    } else {
        format!("{size_bytes} B")
    }
}

fn escape_html(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            _ => vec![ch],
        })
        .collect()
}
