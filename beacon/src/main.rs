mod assets;
mod beacon_routes;
mod db;
mod ui;

use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    http::header,
    response::Response,
    routing::{delete, get, post},
};
use rusqlite::Connection;

const BEACON_RENDEZVOUS_PORT: u16 = 42072;

#[derive(Default)]
pub(crate) struct RendezvousState {
    pub(crate) hosts: HashMap<String, SocketAddr>,
    pub(crate) tokens: HashMap<String, String>,
    pub(crate) pending: HashMap<String, Vec<SocketAddr>>,
}

#[derive(Clone)]
pub(crate) struct AssetState {
    pub(crate) db: Arc<Mutex<Connection>>,
}

#[tokio::main]
async fn main() {
    let db_path = env_path("BEACON_DB_PATH").unwrap_or_else(|| PathBuf::from("data.db"));
    let asset_dir = assets::asset_dir();
    let db = db::open(&db_path);
    db::sync_assets(&db, &asset_dir);

    let assets = AssetState { db };
    let lobbies = Arc::new(Mutex::new(HashMap::new()));
    let rendezvous = Arc::new(Mutex::new(RendezvousState::default()));
    beacon_routes::init_state(lobbies.clone(), rendezvous.clone());
    tokio::spawn(run_rendezvous_udp(rendezvous));

    let asset_routes = Router::<AssetState>::new()
        .route("/assets", get(assets::serve_ui).post(assets::upload))
        .route("/assets/list", get(assets::list_partial))
        .route("/assets/{hash}", get(assets::get).put(assets::put))
        .route("/assets/{hash}/vote/{vote}", post(assets::vote))
        .with_state(assets);

    let lobby_routes = Router::new()
        .route("/lobbies", get(beacon_routes::list_json))
        .route("/lobbies/partial", get(beacon_routes::list_partial))
        .route("/lobbies/register", post(beacon_routes::register))
        .route("/lobbies/{id}/heartbeat", post(beacon_routes::heartbeat))
        .route("/lobbies/{id}/join", post(beacon_routes::join))
        .route(
            "/lobbies/{id}/join/{token}",
            get(beacon_routes::join_status),
        )
        .route("/lobbies/{id}/punch", get(beacon_routes::pending_peers))
        .route("/lobbies/{id}", delete(beacon_routes::delete));

    let app = Router::new()
        .route("/", get(beacon_routes::serve_home))
        .route("/theme.css", get(serve_css))
        .route("/health", get(|| async { "OK" }))
        .route("/beacon", get(beacon_routes::serve_ui))
        .merge(asset_routes)
        .merge(lobby_routes);

    let bind_addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    println!("Listening on http://{bind_addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

async fn serve_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(include_str!("static/theme.css").into())
        .unwrap()
}

async fn run_rendezvous_udp(state: Arc<Mutex<RendezvousState>>) {
    let Ok(sock) =
        tokio::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, BEACON_RENDEZVOUS_PORT))
            .await
    else {
        return;
    };
    let mut buf = [0u8; 256];
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf).await else {
            continue;
        };
        let Ok(msg) = std::str::from_utf8(&buf[..n]) else {
            continue;
        };
        let mut rendezvous = state.lock().unwrap();
        if let Some(id) = msg.strip_prefix("host:") {
            rendezvous.hosts.insert(id.to_string(), from);
            continue;
        }
        let mut parts = msg.split(':');
        if parts.next() != Some("join") {
            continue;
        }
        let Some(id) = parts.next() else {
            continue;
        };
        let Some(token) = parts.next() else {
            continue;
        };
        if rendezvous.tokens.get(token).is_none_or(|value| value != id) {
            continue;
        }
        let peers = rendezvous.pending.entry(id.to_string()).or_default();
        if !peers.contains(&from) {
            peers.push(from);
        }
    }
}
