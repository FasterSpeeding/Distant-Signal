use axum_prometheus::PrometheusMetricLayerBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::app::{App, AppState, Router};

pub mod app;
pub mod auth;
pub mod data;
pub mod render;
pub mod routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let app = AppState::init().await?;

    tokio::spawn(schedule_match_sweep_loop(app.clone()));

    // Permissive ORIGIN, deliberately non-credentialed. The four
    // line-status endpoints and /public/health are intentionally public,
    // and /private/* is gated by internal-service OAuth2
    // (require_internal_oauth, crates/api/src/auth.rs) — a header check
    // CORS doesn't bypass.
    //
    // READ THIS BEFORE CHANGING THE TWO LINES BELOW. /public/* now also
    // carries cookie-based session auth (the `distant_signal_session` cookie, see
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
        .with_prefix("distant_signal")
        .with_default_metrics()
        .build_pair();

    let mut router = Router::new()
        .merge(routes::line_status::router())
        .merge(routes::train::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()));

    // Unlike the other seven binaries, api's own listener stays up either
    // way -- metrics_enabled only decides whether /metrics is registered
    // and whether requests are counted at all. See `metrics_enabled` in
    // crates/api/src/data/config.rs.
    if app.config.metrics_enabled {
        router = router
            // Deliberately NOT gated by require_internal_oauth. Read-only,
            // and NetworkPolicy-gated (from the monitoring namespace
            // specifically) when NetworkPolicy is enabled (see
            // docs/superpowers/specs/2026-08-29-metrics-design.md's Open
            // Question 3, and the metrics plan's Task 10). NOTE: unlike the
            // other seven binaries' dedicated metrics ports, this route
            // shares api's own HTTP port -- so if `ingress.api.enabled=true`
            // (default false), it IS reachable through that public Ingress
            // alongside the rest of the public API, exactly like api's own
            // /private/* caveat already documented in the chart README. The
            // exposure is read-only request-count/latency telemetry, not
            // secrets, and metrics.enabled=false removes this route
            // entirely.
            .route(
                "/metrics",
                axum::routing::get(move || async move { metrics_handle.render() }),
            )
            .layer(metrics_layer);
    }

    let router = router
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

/// Periodic retry of Decision 3's schedule-first match against every
/// still-`pending`, never-schedule-matched tracked-train row -- the
/// mechanism that makes this feature retroactive-capable for a pin
/// created before its schedule's population was published, or before
/// this feature shipped at all (Decision 6 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
/// Mirrors `crates/enricher/src/main.rs`'s own `sweep_loop` shape -- the
/// established precedent in this workspace for "a service that is mostly
/// a request/response server also runs one background interval loop."
async fn schedule_match_sweep_loop(app: App) {
    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(app.config.schedule_match_interval_secs));
    loop {
        interval.tick().await;
        match data::schedule_matching::run_schedule_match_sweep(&app.database, &app.schedule_crs_line_index)
            .await
        {
            Ok(matched) if matched > 0 => {
                tracing::info!(matched, "schedule-match sweep resolved pending pins");
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "schedule-match sweep failed; will retry next interval");
            }
        }
    }
}
