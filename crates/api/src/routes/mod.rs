use axum::middleware;

use crate::app::{App, Router};
use crate::auth::require_internal_token;

pub mod health;
pub mod ingest;

pub fn public_router() -> Router {
    // `health::router()` already declares its own `/health` route, so this
    // must `merge` (mount directly under `/public`) rather than `nest`
    // another `/health` prefix on top of it — nesting here previously
    // produced `/public/health/health` instead of the intended
    // `/public/health`, discovered while wiring up the docker-compose
    // healthcheck in Task 6's end-to-end verification.
    Router::new().merge(health::router())
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
