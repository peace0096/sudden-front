mod game;
mod hub;
mod net;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/game.v1.rs"));
}

use axum::{routing::get, Router};
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    // Every room's ECS world is owned by this one thread, ticking at 30 Hz.
    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("game-hub".into())
        .spawn(move || hub::run(inbound_rx))
        .expect("spawn game thread");

    let dist = std::env::var("CLIENT_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../client/dist"));
    let static_files = ServeDir::new(&dist).fallback(ServeFile::new(dist.join("index.html")));

    let app = Router::new()
        .route("/ws", get(net::ws_handler))
        .fallback_service(static_files)
        .with_state(inbound_tx);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind port");
    println!("Sudden Front server: http://localhost:{port}");
    axum::serve(listener, app).await.expect("server error");
}
