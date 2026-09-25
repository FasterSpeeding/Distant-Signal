use axum_prometheus::PrometheusMetricLayerBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use api::app::{App, AppState, Router};
use api::{data, routes};

/// Redacts an unlisted-link share token out of a request URI before it
/// reaches `tracing`'s per-request span. 2026-09 Signal Box Audit Low
/// finding: `TraceLayer::new_for_http()`'s default `make_span_with` logs
/// the full request URI verbatim as a span field -- for `GET
/// /Journeys/shared/{token}` (`routes::journeys::get_journey_by_share_token`)
/// that means the raw, unauthenticated-bearer share token travels into
/// every trace/log line this request produces, stacked on top of the
/// exposure this codebase does NOT control (ingress access logs, browser
/// history, a `Referer` header on any outbound link/asset request the
/// shared page makes) -- the token is the entire access control for that
/// route, so leaking it anywhere is equivalent to leaking the resource
/// itself.
///
/// A full TTL mechanism for journey share links already exists at the
/// data layer -- `unlisted_links::rotate_link`'s own `ttl: Option<Duration>`
/// parameter, backed by `unlisted_links.expires_at` and honored by
/// `resolve_link`/`get_active_link` -- but journeys deliberately pass
/// `None` today, a DOCUMENTED product decision
/// (`docs/superpowers/specs/2026-09-23-unlisted-links-design.md` §5: a
/// journey's link grants read-only access to one already-bounded
/// resource, not an ever-growing membership boundary, so explicit
/// revoke/regenerate are its only two owner-facing levers). Silently
/// overriding that as a side effect of a Low-severity logging fix would be
/// a bigger, product-level change than this finding calls for -- forcing
/// every existing share link to start expiring is a UX change worth its
/// own decision, not something to sneak in here. Redacting the token from
/// tracing output is the smaller, purely-defensive fix instead: it closes
/// the log-exposure channel without changing the feature's behavior at
/// all, and if a TTL mechanism is wanted later, the plumbing is already
/// there waiting for it.
fn redact_share_token_uri(uri: &axum::http::Uri) -> String {
    let path = uri.path();
    if let Some(token) = path.strip_prefix("/Journeys/shared/")
        && !token.is_empty()
        && !token.contains('/')
    {
        return "/Journeys/shared/[REDACTED]".to_string();
    }
    uri.to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let app = AppState::init().await?;

    tokio::spawn(schedule_match_sweep_loop(app.clone()));
    tokio::spawn(reconciliation_sweep_loop(app.clone()));
    tokio::spawn(backlog_match_sweep_loop(app.clone()));
    tokio::spawn(session_cleanup_sweep_loop(app.clone()));

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
        .merge(routes::journeys::router())
        .merge(routes::journey_templates::router())
        .merge(routes::trips::router())
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
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<axum::body::Body>| {
                tracing::info_span!(
                    "request",
                    method = %request.method(),
                    uri = %redact_share_token_uri(request.uri()),
                    version = ?request.version(),
                )
            },
        ))
        .with_state(app.clone());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // MUST stay immediately before `sqlx::migrate!()`, never after.
    // `migrations/20260906140000_drop_legacy_columns.sql` IRREVERSIBLY drops
    // `train_movement_events.tracked_train_id`, `train_current_state.tracked_train_id`
    // and `tracked_trains.train_uid` -- the only columns from which a
    // pre-existing row's shared-train identity can still be recovered. On a
    // database that has not yet applied that migration and still has rows
    // whose `trains_id` was never backfilled, this refuses to start and
    // names the fix (`cargo run -p api --bin backfill_trains`), rather than
    // letting the migration run and silently lose the link. On every
    // already-contracted database -- which is every environment this plan
    // has already touched -- it is a single `_sqlx_migrations` lookup that
    // returns immediately. See
    // `crates/api/src/data/legacy_backfill.rs`'s module doc for the full
    // required deploy sequence and for why this check cannot live inside
    // the migration file itself.
    data::legacy_backfill::ensure_ready_for_contract_migration(&app.database).await?;

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
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        app.config.schedule_match_interval_secs,
    ));
    loop {
        interval.tick().await;
        match data::schedule_matching::run_schedule_match_sweep(
            &app.database,
            &app.schedule_crs_line_index,
        )
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

/// Periodic retry of two independent, confirmed stalls in tracked-train
/// state -- see
/// docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md.
/// Mirrors `schedule_match_sweep_loop`'s own shape exactly: same "a
/// request/response server also runs a background interval loop" pattern
/// this workspace already established.
async fn reconciliation_sweep_loop(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        app.config.reconciliation_sweep_interval_secs,
    ));
    let grace_period = chrono::Duration::minutes(app.config.schedule_enrichment_grace_minutes);
    loop {
        interval.tick().await;
        match data::reconciliation::run_reconciliation_sweep(
            &app.database,
            &app.schedule_crs_line_index,
            grace_period,
        )
        .await
        {
            Ok(result)
                if result.resolution_status_reconciled > 0
                    || result.schedule_enrichment_matched > 0 =>
            {
                tracing::info!(
                    resolution_status_reconciled = result.resolution_status_reconciled,
                    schedule_enrichment_matched = result.schedule_enrichment_matched,
                    "reconciliation sweep made progress on stuck tracked-train state"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "reconciliation sweep failed; will retry next interval");
            }
        }
    }
}

/// Periodic retry of `attempt_backlog_match` for every still-`pending` pin
/// it hasn't yet resolved -- the fix for a confirmed gap named in full on
/// `data::trust_event_backlog_match::run_backlog_match_sweep`'s own doc
/// comment: that function was previously only ever invoked once,
/// synchronously, at pin-creation time, with no retry for a train whose
/// real departure fell outside `common::MATCH_TOLERANCE` of its scheduled
/// time (routine under disruption). Mirrors `schedule_match_sweep_loop`'s
/// own shape exactly -- same "a request/response server also runs a
/// background interval loop" pattern this workspace already established
/// for both sibling sweeps above.
async fn backlog_match_sweep_loop(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        app.config.backlog_match_sweep_interval_secs,
    ));
    loop {
        interval.tick().await;
        match data::trust_event_backlog_match::run_backlog_match_sweep(&app.database).await {
            Ok(matched) if matched > 0 => {
                tracing::info!(matched, "backlog-match sweep resolved pending pins");
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "backlog-match sweep failed; will retry next interval");
            }
        }
    }
}

/// Periodic sweep deleting `sessions` rows past their `expires_at`
/// (`data::users::prune_expired_sessions`) -- part of the fix for "no
/// server-side session revocation" (2026-09-25 security review): an
/// expired row was already excluded from every lookup
/// (`get_session_with_user`'s own `WHERE expires_at > NOW()`), but
/// nothing ever actually deleted it, so the table only ever grew.
/// Mirrors `schedule_match_sweep_loop`'s own shape exactly -- same "a
/// request/response server also runs a background interval loop" pattern
/// this workspace already established, and the same one this crate's own
/// three sibling sweeps above already follow. Unlike those three, this
/// sweep never resolves anything a user is waiting on, so its own
/// `session_cleanup_interval_secs` defaults to a much coarser cadence
/// (1 hour) than theirs (5 minutes).
async fn session_cleanup_sweep_loop(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        app.config.session_cleanup_interval_secs,
    ));
    loop {
        interval.tick().await;
        match data::users::prune_expired_sessions(&app.database).await {
            Ok(deleted) if deleted > 0 => {
                tracing::info!(deleted, "session-cleanup sweep pruned expired sessions");
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "session-cleanup sweep failed; will retry next interval");
            }
        }
    }
}

#[cfg(test)]
mod redact_share_token_uri_tests {
    use super::redact_share_token_uri;

    #[test]
    // 2026-09 Signal Box Audit Low finding regression: the whole point of
    // this function is that the raw token never reaches the returned
    // string.
    fn redacts_a_journey_share_token() {
        let uri: axum::http::Uri = "/Journeys/shared/super-secret-token-123"
            .parse()
            .expect("valid uri");
        let redacted = redact_share_token_uri(&uri);
        assert_eq!(redacted, "/Journeys/shared/[REDACTED]");
        assert!(!redacted.contains("super-secret-token-123"));
    }

    #[test]
    fn leaves_an_ordinary_request_uri_untouched() {
        let uri: axum::http::Uri = "/Journeys/mine".parse().expect("valid uri");
        assert_eq!(redact_share_token_uri(&uri), "/Journeys/mine");
    }

    #[test]
    fn leaves_a_journey_detail_uri_untouched() {
        // Only the PUBLIC share-token route carries a bare secret in the
        // path -- an ordinary `/Journeys/{id}` uses an opaque-but-not-
        // secret numeric id behind session auth, nothing to redact.
        let uri: axum::http::Uri = "/Journeys/42".parse().expect("valid uri");
        assert_eq!(redact_share_token_uri(&uri), "/Journeys/42");
    }

    #[test]
    fn does_not_redact_the_bare_shared_prefix_with_no_token() {
        let uri: axum::http::Uri = "/Journeys/shared/".parse().expect("valid uri");
        assert_eq!(redact_share_token_uri(&uri), "/Journeys/shared/");
    }

    #[test]
    fn preserves_the_query_string_shape_for_non_matching_paths() {
        let uri: axum::http::Uri = "/Journeys/mine?foo=bar".parse().expect("valid uri");
        assert_eq!(redact_share_token_uri(&uri), "/Journeys/mine?foo=bar");
    }
}
