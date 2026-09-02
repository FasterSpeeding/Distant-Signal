use axum::extract::DefaultBodyLimit;
use axum::middleware;

use crate::app::{App, Router};
use crate::auth::require_internal_oauth;

pub mod auth;
pub mod freshness;
pub mod health;
pub mod history_retention;
pub mod incidents;
pub mod ingest;
pub mod line_status;
pub mod lines;
pub mod preferences;
pub mod reference;
pub mod samples;
pub mod train;

pub fn public_router() -> Router {
    // `health::router()` already declares its own `/health` route, so this
    // must `merge` (mount directly under `/public`) rather than `nest`
    // another `/health` prefix on top of it — nesting here previously
    // produced `/public/health/health` instead of the intended
    // `/public/health`, discovered while wiring up the docker-compose
    // healthcheck in Task 6's end-to-end verification.
    //
    // `line_status::router()` is deliberately NOT merged in here. This
    // function's output is always nested under `/public` in `main.rs`
    // (load-bearing: `docker-compose.yml`'s healthcheck hits
    // `/public/health` and `crates/api/Dockerfile`'s HEALTHCHECK comment
    // says the same), but the four line-status endpoints must be
    // reachable at the unprefixed paths DESIGN.md specifies
    // (`GET /Line/Mode/national-rail/Status`, `GET /StopPoint/{crs}/Disruption`,
    // etc.) so that clients already built against TfL's own API work
    // unchanged — that's the entire point of mimicking TfL's response
    // shape. Nesting them under `/public` like `health` would silently
    // break that compatibility. `main.rs` merges `line_status::router()`
    // directly onto the top-level router instead; it's still
    // unauthenticated (no `require_internal_oauth` layer applied), just
    // not routed through this particular function.
    Router::new()
        .merge(health::router())
        .merge(freshness::router())
        .merge(history_retention::router())
        .merge(incidents::router())
        .merge(lines::router())
        .merge(preferences::router())
        .merge(reference::router())
        .merge(auth::router())
}

/// Takes the app state directly (rather than picking it up later via
/// `Router::with_state`) because the internal-auth layer needs a concrete
/// token value at the point it's constructed: `axum::middleware::from_fn`
/// fixes its handler's state to `()`, so a stateful check has to go through
/// `from_fn_with_state`, which takes the state by value up front.
pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .merge(samples::router())
        .layer(middleware::from_fn_with_state(app, require_internal_oauth))
        // Axum's `Json` extractor enforces an implicit 2MB body-read limit
        // unless overridden. `StationReference::accessibility` (see
        // crates/common) is a `#[serde(flatten)]` passthrough that carries
        // *every* unmodeled per-station field from the RDM feed verbatim
        // (carParks, ticketBuying, lifts, transportLinks, address, ...),
        // not just accessibility data — so the full ~2,600-station feed
        // measures ~55MB raw, which is what actually surfaced as a 413 on
        // poller-stations' ingest POST (a prior fix here that assumed
        // ~20MB was itself too low; verified directly against the live RDM
        // feed rather than guessed). 100MB leaves ~2x headroom over
        // today's measured size for feed growth.
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
}
