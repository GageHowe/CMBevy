use std::{
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
};

use axum::{
    Json,
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AppState, db,
    ui::{self, Page},
};

const FILENAME_HEADER: &str = "x-asset-filename";

#[derive(Deserialize)]
pub(crate) struct AssetListQuery {
    q: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AssetUploadResponse {
    hash: String,
    file_name: String,
    size_bytes: usize,
}

pub(crate) async fn serve_ui() -> Html<String> {
    ui::page(
        "Critical Mass Assets",
        Page::Assets,
        include_str!("static/assets_body.html"),
        include_str!("static/assets.js"),
    )
}

pub(crate) async fn upload(
    State(state): State<AppState>,
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
    write_asset(&hash, &file_name, &body)?;
    db::upsert_asset(&state.db, &hash, &file_name, body.len() as u64);
    Ok(Json(AssetUploadResponse {
        hash,
        file_name,
        size_bytes: body.len(),
    }))
}

pub(crate) async fn list_partial(
    State(state): State<AppState>,
    Query(query): Query<AssetListQuery>,
) -> Html<String> {
    let entries = db::list_assets(&state.db, query.q.as_deref());
    if entries.is_empty() {
        return Html(r#"<div class="empty">No matching assets found.</div>"#.to_string());
    }
    Html(
        entries
            .into_iter()
            .map(|entry| {
                let total = entry.upvotes + entry.downvotes;
                let positive = if total == 0 {
                    "No votes".to_string()
                } else {
                    format!("{}% positive", entry.upvotes * 100 / total)
                };
                format!(
                    r#"
                    <article class="asset-card">
                        <div class="stack">
                            <h3>{name}</h3>
                            <p>{hash}</p>
                        </div>
                        <div class="meta">
                            <span>{size}</span>
                            <span>{positive}</span>
                            <span>{up}/{total} upvotes</span>
                        </div>
                        <div class="footer">
                            <div class="row">
                                <button class="ghost" onclick="voteAsset('{hash}', 'up')">Upvote</button>
                                <button class="ghost" onclick="voteAsset('{hash}', 'down')">Downvote</button>
                            </div>
                            <div class="row">
                                <button class="ghost" onclick="copyAssetHash('{hash}')">Copy hash</button>
                                <a class="button" href="/assets/{hash}" target="_blank" rel="noreferrer">Open file</a>
                            </div>
                        </div>
                    </article>
                "#,
                    name = escape_html(&entry.file_name),
                    hash = escape_html(&entry.hash),
                    size = format_size(entry.size_bytes),
                    positive = positive,
                    up = entry.upvotes,
                    total = total,
                )
            })
            .collect(),
    )
}

pub(crate) async fn vote(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Path((hash, vote)): Path<(String, String)>,
) -> StatusCode {
    let Some(value) = parse_vote(&vote) else {
        return StatusCode::BAD_REQUEST;
    };
    db::vote(&state.db, &hash, &addr.ip().to_string(), value);
    StatusCode::NO_CONTENT
}

pub(crate) async fn get(Path(hash): Path<String>) -> Result<Response, StatusCode> {
    let bytes = std::fs::read(asset_path(&hash)).map_err(|_| StatusCode::NOT_FOUND)?;
    let file_name = std::fs::read_to_string(asset_meta_path(&hash)).ok();
    let mut builder = Response::builder();
    if let Some(file_name) = file_name.as_deref() {
        builder = builder.header(FILENAME_HEADER, file_name.trim());
    }
    builder
        .body(bytes.into())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn put(
    State(state): State<AppState>,
    Path(hash): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let file_name = headers
        .get(FILENAME_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(sanitize_file_name)
        .unwrap_or_else(|| fallback_file_name(&hash));
    match write_asset(&hash, &file_name, &body) {
        Ok(()) => {
            db::upsert_asset(&state.db, &hash, &file_name, body.len() as u64);
            StatusCode::CREATED
        }
        Err(code) => code,
    }
}

fn write_asset(hash: &str, file_name: &str, body: &[u8]) -> Result<(), StatusCode> {
    let path = asset_path(hash);
    let Some(dir) = path.parent() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    std::fs::create_dir_all(dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(&path, body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(asset_meta_path(hash), file_name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

fn parse_vote(vote: &str) -> Option<bool> {
    match vote {
        "up" => Some(true),
        "down" => Some(false),
        _ => None,
    }
}

pub(crate) fn asset_dir() -> PathBuf {
    if FsPath::new("asset_blobs").exists() || !cfg!(debug_assertions) {
        PathBuf::from("asset_blobs")
    } else {
        PathBuf::from("../asset_blobs")
    }
}

fn asset_path(hash: &str) -> PathBuf {
    asset_dir().join(sanitize_hash(hash))
}

fn asset_meta_path(hash: &str) -> PathBuf {
    asset_dir().join(format!("{}.name", sanitize_hash(hash)))
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

fn fallback_file_name(hash: &str) -> String {
    format!("{}.bin", sanitize_hash(hash))
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
