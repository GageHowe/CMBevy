mod assets;
mod beacon_routes;
mod db;
mod ui;

use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    http::header,
    response::Response,
    routing::{delete, get, post},
};
use axum_server::tls_rustls::RustlsConfig;
use http_common::LobbyInfo;
use rcgen::generate_simple_self_signed;
use rusqlite::Connection;

const TLS_HOSTNAME: &str = "criticalmass.dev";

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) db: Arc<Mutex<Connection>>,
    pub(crate) lobbies: Arc<Mutex<HashMap<String, LobbyInfo>>>,
}

#[tokio::main]
async fn main() {
    let db = db::open("data.db");
    db::sync_assets(&db, &assets::asset_dir());

    let state = AppState {
        db,
        lobbies: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/", get(beacon_routes::serve_home))
        .route("/theme.css", get(serve_css))
        .route("/health", get(|| async { "OK" }))
        .route("/assets", get(assets::serve_ui).post(assets::upload))
        .route("/assets/list", get(assets::list_partial))
        .route("/assets/{hash}", get(assets::get).put(assets::put))
        .route("/assets/{hash}/vote/{vote}", post(assets::vote))
        .route("/beacon", get(beacon_routes::serve_ui))
        .route("/lobbies", get(beacon_routes::list_json))
        .route("/lobbies/partial", get(beacon_routes::list_partial))
        .route("/lobbies/register", post(beacon_routes::register))
        .route("/lobbies/{id}/heartbeat", post(beacon_routes::heartbeat))
        .route("/lobbies/{id}", delete(beacon_routes::delete))
        .with_state(state);

    let service = app.into_make_service_with_connect_info::<SocketAddr>();

    if let Some(tls) = tls_config().await {
        let bind_addr = SocketAddr::from(([0, 0, 0, 0], 443));
        println!("Listening on https://{bind_addr}");
        axum_server::bind_rustls(bind_addr, tls)
            .serve(service)
            .await
            .unwrap();
        return;
    }

    let bind_addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    eprintln!("TLS not supported, listening on http://{bind_addr}");
    axum::serve(listener, service).await.unwrap();
}

async fn serve_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(include_str!("static/theme.css").into())
        .unwrap()
}

async fn tls_config() -> Option<RustlsConfig> {
    let cert_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let cert_path = cert_dir.join("fullchain.pem");
    let key_path = cert_dir.join("privkey.pem");
    if (!cert_path.is_file() || !key_path.is_file())
        && generate_tls_files(&cert_path, &key_path).is_err()
    {
        return None;
    }
    let tls = RustlsConfig::from_pem_file(cert_path, key_path)
        .await
        .unwrap();
    Some(tls)
}

fn generate_tls_files(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let cert = generate_simple_self_signed(vec![TLS_HOSTNAME.to_string()])?;
    fs::write(cert_path, cert.cert.pem())?;
    fs::write(key_path, cert.key_pair.serialize_pem())?;
    Ok(())
}
