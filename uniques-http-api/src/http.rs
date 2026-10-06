pub(crate) mod admin;
pub mod api;
pub mod extract;
pub mod state;

use axum::Router;
use tower_http::CompressionLevel;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;

pub use extract::IndexSnapshot;
pub use state::{AppState, QuerySnapshot, ServerState};

pub fn app(server: ServerState) -> Router {
    let collections = server.settings.collections.clone();
    Router::new()
        .merge(admin::router())
        .merge(api::router(&collections))
        .layer(CorsLayer::permissive())
        // gzip when the client sends `Accept-Encoding: gzip` (JSON bodies are large and repetitive).
        // Fastest level: per-request bodies are small enough that CPU matters more than ratio.
        // Responses that already carry `Content-Encoding` (pre-gzipped effects list) pass through.
        .layer(CompressionLayer::new().quality(CompressionLevel::Fastest))
        .with_state(server)
}
