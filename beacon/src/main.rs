mod assets;
mod beacon_routes;
mod db;
mod ui;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    http::header,
    response::Response,
    routing::{delete, get, post},
};
use http_common::LobbyInfo;
use rusqlite::Connection;

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

    let bind_addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    println!("Listening on http://{bind_addr}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

async fn serve_css() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(include_str!("static/theme.css").into())
        .unwrap()
}
