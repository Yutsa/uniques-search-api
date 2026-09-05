pub mod cards;
pub mod collections;
pub(crate) mod error;
pub mod effects;
pub mod family;
pub mod search;

use axum::Router;

use crate::config::CollectionsSettings;
use crate::http::ServerState;

pub fn router(collections: &CollectionsSettings) -> Router<ServerState> {
    Router::new()
        .merge(cards::router())
        .merge(collections::router(collections))
        .merge(effects::router())
        .merge(family::router())
        .merge(search::router())
}
