use axum_prometheus::PrometheusMetricLayerBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::app::{AppState, Router};

pub mod app;
pub mod auth;
pub mod data;
pub mod render;
pub mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let app = AppState::init().await?;

    // Permissive ORIGIN, deliberately non-credentialed. The four
    // line-status endpoints and /public/health are intentionally public,
    // and /private/* is gated by the shared-secret X-Internal-Token header
    // (crates/api/src/auth.rs) — a header check CORS doesn't bypass.
    //
    // READ THIS BEFORE CHANGING THE TWO LINES BELOW. /public/* now also
    // carries cookie-based session auth (the `nr_session` cookie, see
    // crates/api/src/auth.rs), including endpoints that mutate a user's
    // data. What keeps `allow_origin(Any)` from being a cross-origin
    // request-forgery hole is exactly two things, both load-bearing:
    //
    //   1. `allow_credentials(true)` is NOT set. Without it a browser
    //      refuses to attach cookies to a cross-origin XHR/fetch at all,
    //      and refuses to expose the response — so a hostile page can
    //      neither read a victim's preferences nor act as them here. Note
    //      that setting it alongside `allow_origin(Any)` is not even
    //      legal per the CORS spec, and tower-http panics on the
    //      combination; do not "fix" that panic by pinning an origin
    //      allowlist and enabling credentials without re-deriving this
    //      whole comment.
    //   2. Only GET is allowed. Every session-authenticated mutation is a
    //      POST/PUT/DELETE, so each is a non-simple request whose
    //      preflight this config rejects outright. Adding a method here
    //      moves that endpoint inside the permissive-origin envelope.
    //
    // The session cookie is additionally `SameSite=Lax`, which blocks
    // cross-site cookie attachment on exactly those non-GET requests
    // (`auth::set_cookie_header`) — a second, independent barrier, not a
    // substitute for either of the above.
    let cors = CorsLayer::new()
        .allow_methods([axum::http::Method::GET])
        .allow_origin(Any);

    let (metrics_layer, metrics_handle) = PrometheusMetricLayerBuilder::new()
        .with_prefix("nr_status")
        .with_default_metrics()
        .build_pair();

    let router = Router::new()
        .merge(routes::line_status::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()))
        // Deliberately NOT gated by require_internal_token or CORS-scoped
        // -- see docs/superpowers/specs/2026-08-29-metrics-design.md's
        // Open Question 3: read-only, cluster-internal only (no Ingress
        // route, ever), NetworkPolicy-gated when NetworkPolicy is enabled
        // (this plan's Task 10).
        .route("/metrics", axum::routing::get(move || async move { metrics_handle.render() }))
        .layer(metrics_layer)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(app.clone());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    sqlx::migrate!().run(&app.database).await?;

    let listener = tokio::net::TcpListener::bind(&app.config.bind_url).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
