use axum::middleware;

use crate::app::{App, Router};
use crate::auth::require_internal_token;

pub mod health;
pub mod ingest;

pub fn public_router() -> Router {
    Router::new().nest("/health", health::router())
}

/// Takes the app state directly (rather than picking it up later via
/// `Router::with_state`) because the internal-auth layer needs a concrete
/// token value at the point it's constructed: `axum::middleware::from_fn`
/// fixes its handler's state to `()`, so a stateful check has to go through
/// `from_fn_with_state`, which takes the state by value up front.
pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .layer(middleware::from_fn_with_state(app, require_internal_token))
}
