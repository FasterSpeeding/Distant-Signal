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

/// Collapses every request that didn't match a registered route onto one
/// constant Prometheus `endpoint` label, instead of `axum_prometheus`'s
/// default behaviour of reporting the raw, verbatim request path.
///
/// **The bug this closes.** `axum_prometheus`'s `EndpointLabel::MatchedPath`
/// (the crate's own default, still in effect here until this function is
/// wired in below) tries `axum::extract::MatchedPath` first, but --
/// unavoidably, since a request that matches no route has no `MatchedPath`
/// at all -- falls back to `EndpointLabel::Exact`: the exact,
/// attacker/caller-controlled request URI, verbatim, as the `endpoint`
/// label on THREE metric families at once
/// (`distant_signal_http_requests_total`,
/// `distant_signal_http_requests_pending`,
/// `distant_signal_http_requests_duration_seconds`). `api`'s own listener
/// sits behind the chart's Ingress under a catch-all `path: /` rule (see
/// `spawn_metrics_listener`'s own doc comment above, which cites this same
/// fact for a different, already-fixed exposure), so it is reachable by
/// the ordinary background noise every public HTTP endpoint on the
/// internet receives -- scanners and bots probing `/wp-login.php`,
/// `/.env`, `/.git/config`, `/actuator/health`, random exploit paths, and
/// so on, none of which match any route this app registers. Each ONE of
/// those is a distinct string, so `metrics_exporter_prometheus`'s
/// in-process registry -- which never evicts a label set once created --
/// grows one brand-new permanent time series (three, actually: one per
/// metric family above, the two histograms carrying a full bucket array
/// each) for every distinct junk path the pod has ever been probed with,
/// for the rest of that process's life. That is unbounded memory growth
/// with no natural ceiling, driven entirely by traffic this app has zero
/// control over -- a textbook Prometheus/axum cardinality footgun, and a
/// highly plausible match for a live incident where `api` (1) sits over
/// its 1536Mi chart limit and (2) shows a restart cadence consistent with
/// "grows until OOM-killed, restarts, registry resets to empty, repeats."
///
/// `api` is the only one of this workspace's eight binaries that wires up
/// `axum_prometheus`'s per-HTTP-request auto-instrumentation at all (the
/// other seven have no comparable HTTP surface, per
/// docs/superpowers/specs/2026-08-29-metrics-design.md's own binary
/// table) -- so this is the only place in the workspace this footgun can
/// fire, and nothing in that spec's otherwise cardinality-conscious
/// review (it explicitly calls out and rejects per-line/per-station
/// labels elsewhere) considered THIS source of cardinality.
///
/// **The fix.** `axum_prometheus::EndpointLabel::MatchedPathWithFallbackFn`
/// makes the fallback a caller-supplied function instead of "verbatim
/// URI." Returning the same constant string for every input, regardless
/// of what was requested, bounds the `endpoint` label's cardinality to
/// "one entry per real route this app registers, plus exactly one more
/// for everything that didn't match" -- closing the leak without losing
/// any per-route granularity for legitimate traffic. A single shared
/// bucket for all unmatched paths is also strictly more useful for an
/// operator than either alternative: unlike `EndpointLabel::Exact`'s
/// per-junk-path explosion, "how many requests per second are hitting
/// nothing at all" is exactly the aggregate scanner-noise signal worth
/// having, and it costs three fixed time series total instead of three
/// per distinct probe.
fn unmatched_route_endpoint_label(_exact_path: &str) -> String {
    "/{unmatched}".to_string()
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
    //   2. Only GET is allowed. This does NOT mean every session-
    //      authenticated mutation's preflight is rejected outright, though
    //      an earlier version of this comment claimed exactly that — it's
    //      only true for a mutation whose body forces a genuinely
    //      "non-simple" request (per the Fetch spec's CORS-safelisted
    //      request-header rules), e.g. `Content-Type: application/json`.
    //      A handful of real routes — group promote/demote
    //      (`routes::groups::promote_member`/`demote_member`), group join
    //      (`routes::groups::post_join`), invite-link create/revoke
    //      (`routes::groups::create_invite_link`/`revoke_invite_link`), and
    //      journey share-link create/revoke
    //      (`routes::journeys::create_journey_share_link`/
    //      `revoke_journey_share_link`) — are POST/DELETE with NO JSON (or
    //      any other) body at all, sent with no extra headers, which makes
    //      each one a "simple request" under the CORS spec: the browser
    //      never sends a preflight for it in the first place, so a method
    //      allowlist here has nothing to intercept, and this config would
    //      let the ACTUAL request straight through to the handler
    //      regardless of Origin (only withholding `Access-Control-Allow-*`
    //      response headers, which stops a cross-origin page from reading
    //      the response body, not from having already caused the mutation).
    //      Adding a method here does still widen the permissive-origin
    //      envelope for the genuinely non-simple (JSON-body) mutations, so
    //      the caution stands — just not for the reason originally given.
    //
    // What actually protects those no-body routes is two independent
    // layers, neither of which is this CORS config:
    //   1. The session cookie is `SameSite=Lax`
    //      (`auth::set_cookie_header`), which stops the browser from
    //      attaching it to a cross-SITE (not just cross-origin) POST/PUT/
    //      DELETE at all — a genuinely cross-site attacker's request
    //      reaches `api` with no session, so it 401s regardless of what
    //      CORS would have allowed through.
    //   2. `frontend/app/api/[...path]/route.ts`'s own
    //      `hasAcceptableOriginForMutation` Origin check, added because
    //      `SameSite=Lax` alone doesn't cover a same-SITE sibling
    //      subdomain or a future XSS — either can still issue a same-site
    //      request that carries the cookie. Every one of the no-body
    //      routes above is only ever called by the frontend through that
    //      proxy (never fetched directly against this service's own
    //      origin from a browser), so that Origin check is the real
    //      barrier for them, not anything in this file.
    let cors = CorsLayer::new()
        .allow_methods([axum::http::Method::GET])
        .allow_origin(Any);

    let (metrics_layer, metrics_handle) = PrometheusMetricLayerBuilder::new()
        .with_prefix("distant_signal")
        .with_default_metrics()
        // Unbounded Prometheus label cardinality fix (found investigating a
        // live 2026-09-26 OOM incident: the `api` pod running over its
        // 1536Mi chart limit and cycling through repeated restarts). See
        // `unmatched_route_endpoint_label`'s own doc comment for the full
        // mechanism -- this line is what actually installs the bounded
        // fallback instead of `axum_prometheus`'s default
        // `EndpointLabel::MatchedPath`, which silently falls back to
        // `EndpointLabel::Exact` (the raw, unbounded request URI) for any
        // request that doesn't match a route at all.
        .with_endpoint_label_type(axum_prometheus::EndpointLabel::MatchedPathWithFallbackFn(
            unmatched_route_endpoint_label,
        ))
        .build_pair();

    let mut router = Router::new()
        .merge(routes::line_status::router())
        .merge(routes::train::router())
        .merge(routes::journeys::router())
        .merge(routes::journey_templates::router())
        .merge(routes::trips::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()));

    // Unlike the other seven binaries, api's own PUBLIC listener stays up
    // either way -- metrics_enabled only decides whether requests are
    // counted at all and whether the separate internal-only /metrics
    // listener below is started. See `metrics_enabled`/`metrics_port` in
    // crates/api/src/data/config.rs for the full "why a second listener,
    // not a route on this router" rationale (2026-09-25 Signal Box Audit
    // Low finding: /metrics used to be a route on THIS router, sharing
    // api's public port -- and therefore the public Ingress's catch-all
    // `path: /` rule -- with no authentication of its own).
    if app.config.metrics_enabled {
        router = router.layer(metrics_layer);
        spawn_metrics_listener(app.config.metrics_port, metrics_handle);
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

/// Starts api's own internal-only `/metrics` listener on a SEPARATE port
/// from the public one `bind_url` binds -- see `metrics_port`'s own doc
/// comment in crates/api/src/data/config.rs for the full "why a second
/// listener" rationale (2026-09-25 Signal Box Audit Low finding: /metrics
/// used to be a route on the public router, reachable through the chart's
/// Ingress with no auth of its own whenever both `metrics_enabled` and
/// `ingress.api.enabled` were set).
///
/// `metrics_handle` is the SAME `PrometheusHandle` `PrometheusMetricLayerBuilder::build_pair`
/// handed back to `main` -- the request-counting `metrics_layer` stays on
/// the public router (that's what actually observes real traffic); this
/// listener only renders the shared recorder's current text exposition.
///
/// Best-effort, matching `crates/health-http::spawn_with_state`'s own
/// posture for its (also best-effort, also internal-only) `/healthz`
/// listener: a bind failure here logs and returns rather than taking down
/// the whole process -- `/metrics` is a scrape target, not something the
/// rest of `api` depends on to function.
fn spawn_metrics_listener(
    port: u16,
    metrics_handle: axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
) {
    tokio::spawn(async move {
        let metrics_router = axum::Router::new().route(
            "/metrics",
            axum::routing::get(move || {
                let metrics_handle = metrics_handle.clone();
                async move { metrics_handle.render() }
            }),
        );
        let bind_addr = format!("0.0.0.0:{port}");
        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => listener,
            Err(err) => {
                tracing::error!(
                    error = ?err,
                    bind_addr,
                    "failed to bind api's internal /metrics listener"
                );
                return;
            }
        };
        if let Err(err) = axum::serve(listener, metrics_router).await {
            tracing::error!(error = ?err, "api's internal /metrics listener stopped");
        }
    });
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
mod unmatched_route_endpoint_label_tests {
    use super::unmatched_route_endpoint_label;

    // 2026-09-26 unbounded-metric-cardinality regression: the whole point
    // of this function is that its OUTPUT never varies with its input --
    // that's what caps the `endpoint` label to one extra value instead of
    // one per distinct request path a caller can make up.
    #[test]
    fn always_returns_the_same_constant_label() {
        let inputs = [
            "/",
            "/wp-login.php",
            "/.env",
            "/.git/config",
            "/actuator/health",
            "/Journeys/shared/some-real-looking-token",
            "",
        ];
        let labels: std::collections::HashSet<String> = inputs
            .iter()
            .map(|path| unmatched_route_endpoint_label(path))
            .collect();
        assert_eq!(
            labels.len(),
            1,
            "every distinct unmatched path must collapse to the same label, got {labels:?}"
        );
    }

    #[test]
    fn does_not_echo_a_long_adversarial_path_into_the_label() {
        // A caller can make the raw request path arbitrarily long (up to
        // whatever axum/hyper's own URI-length limits allow) -- proving
        // this stays a short constant, not something sized by the input,
        // is the other half of "bounded," not just "same for every input."
        let adversarial = "/".to_string() + &"x".repeat(8192);
        let label = unmatched_route_endpoint_label(&adversarial);
        assert_eq!(label, "/{unmatched}");
        assert!(label.len() < 32);
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
