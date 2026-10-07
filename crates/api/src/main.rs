use anyhow::Context;
use axum_prometheus::PrometheusMetricLayerBuilder;
use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use api::app::{App, AppState, Router};
use api::{data, routes};

/// What the per-request tracing span records as `uri` (API-3, LEG-7).
///
/// `TraceLayer::new_for_http()`'s default span logs the full request URI, and
/// every `warn!`/`error!` inside a request prints the span's fields. Several
/// URIs carry secrets or personal data:
///
/// - bearer tokens in the path: `/Journeys/shared/{token}` (a journey share
///   link) and `/public/groups/join/{token}` (a group invite, valid for 7
///   days; the tokens are hashed at rest, so the logs were the weakest copy);
/// - the query string: `/public/auth/callback?code=&state=` (OIDC) and
///   `lat`/`lon` on the nearby-stations lookup.
///
/// So a request that matched a route is logged by its route TEMPLATE
/// (`/public/groups/join/{token}`), never its concrete path or query. A
/// request that matched nothing is logged by its path, with the query string
/// dropped, the two token prefixes above redacted (a trailing-slash variant
/// of a real token URL is unmatched), and the length capped.
fn loggable_request_uri(
    uri: &axum::http::Uri,
    matched_path: Option<&axum::extract::MatchedPath>,
) -> String {
    if let Some(matched) = matched_path {
        return matched.as_str().to_string();
    }
    redact_unmatched_path(uri.path())
}

/// Longest unmatched path logged; scanners send arbitrarily long ones.
const MAX_LOGGED_UNMATCHED_PATH: usize = 200;

fn redact_unmatched_path(path: &str) -> String {
    const SECRET_PREFIXES: [&str; 2] = ["/Journeys/shared/", "/public/groups/join/"];
    for prefix in SECRET_PREFIXES {
        if let Some(rest) = path.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return format!("{prefix}[REDACTED]");
        }
    }
    if path.len() <= MAX_LOGGED_UNMATCHED_PATH {
        return path.to_string();
    }
    let mut end = MAX_LOGGED_UNMATCHED_PATH;
    while !path.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}[...]", &path[..end])
}

/// The per-request span: method, [`loggable_request_uri`] and version.
fn request_span(request: &axum::http::Request<axum::body::Body>) -> tracing::Span {
    tracing::info_span!(
        "request",
        method = %request.method(),
        uri = %loggable_request_uri(
            request.uri(),
            request.extensions().get::<axum::extract::MatchedPath>(),
        ),
        version = ?request.version(),
    )
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

fn main() -> std::process::ExitCode {
    // `api parse-ticket <pdf|pkpass>`: the ticket-parse child process
    // (M13; see `data::ticket_subprocess`). Checked before anything else so
    // the child never starts a tokio runtime, reads `.env`, or parses the
    // server's own arguments.
    if let Some(code) = data::ticket_subprocess::maybe_run_child() {
        std::process::exit(code);
    }
    common::logging::exit_code(server_main())
}

#[tokio::main]
#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
async fn server_main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    // API-1: tracing FIRST, so nothing logged during `AppState::init` (the
    // INF-5 Postgres wait included), the migrations or a sweep's first tick
    // is dropped.
    common::logging::init("api");

    let app = AppState::init().await?;
    // CORPUS_FALLBACK_ENABLED (default off): see `data::corpus_crosswalk`.
    data::corpus_crosswalk::init_fallback_from_env()?;

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
    //      origin from a browser).
    //   3. (L4, 2026-09-27) `auth::reject_cross_origin_cookie_mutation`,
    //      layered below: the same Origin/Referer check at the api layer
    //      itself, so a deployment that exposes this service directly
    //      (`ingress.api.enabled`) isn't back to `SameSite=Lax` alone.
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

    // API-2: whole-request time limits, body reads included -- a short one
    // for everything public, a longer one for the internal ingest routes
    // (100 MB bodies, 120 s publish statements). The HTTP/1 header-read
    // timeout is on the listener itself; see `api::edge`.
    let edge_settings = api::edge::EdgeSettings::from_env()?;
    let rate_limit_settings = api::rate_limit::RateLimitSettings::from_env()?;
    tracing::info!(?rate_limit_settings, "api rate limits");
    // The MCP is recognised by an internal OAuth bearer verified with the
    // same `AppState::internal_oauth_verifier` (and so the same JWKS cache)
    // as /private/*; an empty INTERNAL_OAUTH_GROUP_MCP leaves this inert.
    let service_callers = api::rate_limit::ServiceCallerAuth::new(
        app.clone() as std::sync::Arc<dyn api::rate_limit::ServiceTokenCheck>,
        &app.config.internal_oauth_group_mcp,
    );
    if service_callers.is_some() {
        tracing::info!(
            mcp_group = %app.config.internal_oauth_group_mcp,
            "MCP service caller recognised on public rate-limited routes (own budget, key svc:mcp)"
        );
    } else {
        tracing::info!("INTERNAL_OAUTH_GROUP_MCP is empty: no MCP service-caller budget");
    }
    let rate_limiter =
        api::rate_limit::RateLimiter::with_service_callers(rate_limit_settings, service_callers);
    let mut router = Router::new()
        .merge(routes::line_status::router())
        .merge(routes::train::router())
        .merge(routes::journeys::router())
        .merge(routes::journey_templates::router())
        .merge(routes::trips::router())
        .nest("/public", routes::public_router())
        .layer(edge_settings.public_timeout_layer())
        .nest(
            "/private",
            routes::private_router(app.clone()).layer(edge_settings.private_timeout_layer()),
        );

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
        data::queries::register_schedule_publish_metrics();
        routes::auth::register_user_metrics();
        data::incident_removal::register_metrics();
        // Before the migrations, so a database that goes away while they
        // run (or right after) already shows as distant_signal_api_db_up 0.
        data::db_health::register_metrics();
        data::trust_event_backlog::register_uid_inference_metrics();
        // Key routes' request series (and the graph-cache outcomes) at 0, so
        // a rare route's first request after a restart is visible to
        // increase()/rate(). See `api::route_metrics`.
        api::route_metrics::register(&app.internal_oauth_routes);
        tokio::spawn(data::db_health::probe_loop(app.database.clone()));
    }

    // L4 (2026-09-26 review): api-layer Origin check on every
    // session-cookie-bearing mutation, so the no-body routes described in
    // the CORS comment above no longer rely on `SameSite=Lax` alone when
    // this service is reached directly rather than through the frontend
    // proxy. See `auth::reject_cross_origin_cookie_mutation`.
    let expected_browser_origin: Option<std::sync::Arc<str>> =
        api::auth::expected_browser_origin(&app.config.sso_redirect_url).map(Into::into);
    let router = router
        // 2026-10-01 outage follow-up: a route that could not reach the
        // database answers 503 + Retry-After (JSON body), not 500. See
        // `api::unavailable`.
        .layer(axum::middleware::from_fn_with_state(
            edge_settings.unavailable_retry_after(),
            api::unavailable::annotate_unavailable,
        ))
        .layer(axum::middleware::from_fn_with_state(
            expected_browser_origin,
            api::auth::reject_cross_origin_cookie_mutation,
        ))
        .layer(cors)
        // Per-client-IP limits on login, /Trips/plan, /Train/by-uid and
        // public writes; /private/* is exempt. See `api::rate_limit`.
        .layer(axum::middleware::from_fn_with_state(
            rate_limiter,
            api::rate_limit::enforce,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .with_state(app.clone());

    // Report the admin group at startup (not secret): an empty value means
    // the admin session-revocation endpoint refuses everyone.
    if app.config.admin_group.trim().is_empty() {
        tracing::info!("ADMIN_GROUP is empty: admin session revocation is disabled");
    } else {
        tracing::info!(
            admin_group = %app.config.admin_group,
            "admin session revocation enabled for this Authentik group"
        );
    }

    // MUST stay immediately before the migrations (`ds_store::migrate::run`),
    // never after.
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
    // the migration file itself. `ds-migrate run` (the chart's migrate Job)
    // runs the same two steps.
    //
    // The migrations then run on their own connection, not the request
    // pool: lock_timeout 10s and a statement_timeout under the startup
    // probe's 900s budget instead of the pool's 60s, after dropping any
    // INVALID index a failed CREATE INDEX CONCURRENTLY left behind. See
    // `ds_store::migrate`.
    let migrate = async {
        ds_store::migrate::ensure_ready_for_contract_migration(&app.database).await?;
        // MIGRATION_DATABASE_URL (the schema owner) when set, else
        // DATABASE_URL. See `ds_store::migrate::migration_url`.
        let (migration_url, migration_url_var) = ds_store::migrate::migration_url(
            &app.config.database_url,
            app.config.migration_database_url.as_deref(),
        );
        let migration_options: sqlx::postgres::PgConnectOptions = migration_url
            .parse()
            .with_context(|| format!("could not parse {migration_url_var}"))?;
        ds_store::migrate::run(
            api::app::with_dead_client_detection(migration_options),
            ds_store::migrate::MigrationSettings::from_env()?,
        )
        .await?;
        anyhow::Ok(())
    };
    let bind_url = app.config.bind_url.clone();
    let header_read_timeout = edge_settings.header_read_timeout();
    run_startup(
        migrate,
        || spawn_background_loops(&app),
        || async move {
            let listener = tokio::net::TcpListener::bind(&bind_url).await?;
            api::edge::serve(listener, router, header_read_timeout).await?;
            Ok(())
        },
    )
    .await
}

/// API-1: the startup order, isolated so it is testable. Migrations first;
/// only then the background sweeps (whose first `interval` tick fires at
/// once, so spawning them earlier ran DML against a possibly un-migrated
/// schema while the migrator held its locks); only then the listener (so
/// the startup/readiness probes only pass on a migrated schema). A failed
/// migration starts nothing.
async fn run_startup<M, S, B, BF>(migrate: M, spawn_background: S, serve: B) -> anyhow::Result<()>
where
    M: Future<Output = anyhow::Result<()>>,
    S: FnOnce(),
    B: FnOnce() -> BF,
    BF: Future<Output = anyhow::Result<()>>,
{
    migrate.await?;
    spawn_background();
    serve().await
}

/// The four background sweeps (the session-cleanup one also runs the
/// dead-link prune and the personal-data retention sweep). Only called once migrations have run -- see
/// [`run_startup`].
fn spawn_background_loops(app: &App) {
    tokio::spawn(schedule_match_sweep_loop(app.clone()));
    tokio::spawn(reconciliation_sweep_loop(app.clone()));
    tokio::spawn(backlog_match_sweep_loop(app.clone()));
    tokio::spawn(session_cleanup_sweep_loop(app.clone()));
    // One-shot: rebuilds the CORPUS crosswalk if the stored one predates the
    // newest delivery or this build's rules (one MAX() when no CORPUS), then
    // seeds the CORPUS freshness gauge.
    let pool = app.database.clone();
    tokio::spawn(async move {
        if let Err(err) = data::corpus_crosswalk::rebuild_if_stale(&pool).await {
            tracing::error!(error = ?err, "CORPUS crosswalk startup rebuild failed");
        }
        // Seeds the CORPUS freshness gauge from the durable marker, so the
        // staleness alert survives restarts between monthly deliveries.
        if let Err(err) = data::corpus::refresh_last_delivery_metric(&pool).await {
            tracing::error!(error = ?err, "CORPUS freshness gauge startup read failed");
        }
    });
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
fn spawn_metrics_listener(port: u16, metrics_handle: PrometheusHandle) {
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

/// Builds a background-sweep-loop `tokio::time::Interval`, ticking every
/// `interval_secs` -- with `MissedTickBehavior::Delay` rather than the
/// default `Burst`. Shared by all four sweep loops below (they mirror
/// each other's shape exactly, per their own doc comments).
///
/// `Burst` fires every missed tick back-to-back with zero gap once a cycle
/// overruns its own interval (a slow DB sweep query, a stuck connection) --
/// exactly when the database is already struggling, it would pile up a
/// burst of immediate follow-up sweeps instead of settling back into its
/// normal cadence. `Delay` instead waits a fresh `interval_secs` from
/// whenever the overrun tick actually completes, so a slow cycle degrades
/// to a slower cadence, never a thundering-herd burst. Same fix, same
/// rationale, as `common::poller_loop`'s own
/// `poll_interval_with_delay_on_overrun`/`aggregator`'s own
/// `cycle_interval`/`enricher`'s own `ticking_interval`. Split into its own
/// function so the configuration is directly assertable in a unit test via
/// `Interval::missed_tick_behavior()`, since the missed-tick BEHAVIOR
/// itself (skipping ticks under a real overrun) isn't practically
/// observable without a slow, flaky, real-time test.
fn sweep_interval(interval_secs: u64) -> tokio::time::Interval {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
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
    let mut interval = sweep_interval(app.config.schedule_match_interval_secs);
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
    let mut interval = sweep_interval(app.config.reconciliation_sweep_interval_secs);
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
    let mut interval = sweep_interval(app.config.backlog_match_sweep_interval_secs);
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
    let mut interval = sweep_interval(app.config.session_cleanup_interval_secs);
    let retention_policy = data::retention::RetentionPolicy {
        past_travel_days: app.config.past_travel_retention_days,
        stale_push_subscription_days: app.config.stale_push_subscription_days,
        inactive_account_days: app.config.inactive_account_retention_days,
    };
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
        // Same hourly cadence, same "cheap, idempotent, nobody is waiting
        // on it" reasoning -- see `unlisted_links::prune_dead_links`.
        match data::unlisted_links::prune_dead_links(&app.database).await {
            Ok(deleted) if deleted > 0 => {
                tracing::info!(
                    deleted,
                    "session-cleanup sweep pruned dead share/invite links"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "dead-link prune failed; will retry next interval");
            }
        }
        // Personal-data retention (UK legal audit LEG-5): past travel,
        // stale push subscriptions and (only if enabled) inactive accounts.
        // Same hourly cadence and "idempotent, retried next tick" posture.
        // See `data::retention`.
        match data::retention::prune_personal_data(
            &app.database,
            retention_policy,
            chrono::Utc::now(),
        )
        .await
        {
            Ok(outcome) if outcome.total() > 0 => {
                tracing::info!(
                    ?outcome,
                    "personal-data retention sweep pruned expired rows"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "personal-data retention sweep failed; will retry next interval");
            }
        }
    }
}

#[cfg(test)]
mod run_startup_tests {
    use std::cell::RefCell;

    use super::run_startup;

    /// API-1: migrate, then spawn the sweeps, then bind.
    #[tokio::test]
    async fn migrations_run_before_the_sweeps_and_the_listener() {
        let steps = RefCell::new(Vec::new());
        run_startup(
            async {
                steps.borrow_mut().push("migrate");
                Ok(())
            },
            || steps.borrow_mut().push("spawn sweeps"),
            || async {
                steps.borrow_mut().push("bind");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*steps.borrow(), ["migrate", "spawn sweeps", "bind"]);
    }

    #[tokio::test]
    async fn a_failed_migration_starts_nothing() {
        let steps = RefCell::new(Vec::new());
        let result = run_startup(
            async { Err(anyhow::anyhow!("migration failed")) },
            || steps.borrow_mut().push("spawn sweeps"),
            || async {
                steps.borrow_mut().push("bind");
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        assert!(steps.borrow().is_empty());
    }
}

#[cfg(test)]
mod sweep_interval_tests {
    use super::sweep_interval;

    /// Regression for the "L1 -- `MissedTickBehavior::Burst` still default"
    /// finding: every sweep loop above shares this one interval builder, so
    /// asserting it here covers all four (`schedule_match_sweep_loop`,
    /// `reconciliation_sweep_loop`, `backlog_match_sweep_loop`,
    /// `session_cleanup_sweep_loop`) -- each must opt into `Delay`, not
    /// leave `Burst` as the default, so an overrun sweep doesn't fire a
    /// burst of back-to-back catch-up cycles against the database.
    #[tokio::test]
    async fn sweep_interval_defaults_to_delay_not_burst_on_a_missed_tick() {
        let interval = sweep_interval(60);
        assert_eq!(
            interval.missed_tick_behavior(),
            tokio::time::MissedTickBehavior::Delay
        );
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
mod request_span_tests {
    use std::sync::{Arc, Mutex};

    use axum::routing::{get, post};
    use tower::ServiceExt;

    use super::{loggable_request_uri, redact_unmatched_path, request_span};

    /// Runs `uri` through a router shaped like the real one (the same
    /// `Router::layer` placement, nested `/public`) and returns what the
    /// span recorded as `uri`.
    async fn logged_uri(method: &str, uri: &str) -> String {
        let seen = Arc::new(Mutex::new(None::<String>));
        let seen_in_layer = seen.clone();
        let public = axum::Router::new()
            .route("/groups/join/{token}", get(|| async {}).post(|| async {}))
            .route("/auth/callback", get(|| async {}))
            .route("/reference/stations/nearest", get(|| async {}));
        let router = axum::Router::new()
            .route("/Journeys/shared/{token}", get(|| async {}))
            .route("/Journeys/{id}", post(|| async {}))
            .nest("/public", public)
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let seen = seen_in_layer.clone();
                    async move {
                        *seen.lock().unwrap() = Some(loggable_request_uri(
                            request.uri(),
                            request.extensions().get::<axum::extract::MatchedPath>(),
                        ));
                        // The real span builder must accept the same request.
                        let _span = request_span(&request);
                        next.run(request).await
                    }
                },
            ));
        let request = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        router.oneshot(request).await.unwrap();
        seen.lock().unwrap().clone().expect("the layer ran")
    }

    /// API-3: a group invite token never reaches the log.
    #[tokio::test]
    async fn a_group_join_token_is_logged_as_the_route_template() {
        for method in ["GET", "POST"] {
            let logged = logged_uri(method, "/public/groups/join/secret-invite-123").await;
            assert_eq!(logged, "/public/groups/join/{token}");
        }
    }

    #[tokio::test]
    async fn a_journey_share_token_is_logged_as_the_route_template() {
        let logged = logged_uri("GET", "/Journeys/shared/super-secret-token-123").await;
        assert_eq!(logged, "/Journeys/shared/{token}");
    }

    /// API-3: the OIDC code and state are in the query string, which is never
    /// logged.
    #[tokio::test]
    async fn the_oidc_callback_query_is_not_logged() {
        let logged = logged_uri("GET", "/public/auth/callback?code=abc123&state=xyz789").await;
        assert_eq!(logged, "/public/auth/callback");
    }

    /// LEG-7: coordinates in the query string are never logged.
    #[tokio::test]
    async fn coordinates_in_the_query_are_not_logged() {
        let logged = logged_uri(
            "GET",
            "/public/reference/stations/nearest?lat=51.5074&lon=-0.1278",
        )
        .await;
        assert!(!logged.contains("51.5074"), "{logged}");
        assert!(!logged.contains('?'), "{logged}");
    }

    #[tokio::test]
    async fn an_unmatched_path_is_logged_without_its_query() {
        let logged = logged_uri("GET", "/wp-login.php?user=admin").await;
        assert_eq!(logged, "/wp-login.php");
    }

    /// A trailing-slash variant of a real token URL matches no route, so it
    /// takes the unmatched branch; the token is still redacted.
    #[tokio::test]
    async fn an_unmatched_token_url_is_still_redacted() {
        let logged = logged_uri("GET", "/public/groups/join/secret-invite-123/").await;
        assert_eq!(logged, "/public/groups/join/[REDACTED]");
        assert!(!logged.contains("secret-invite-123"));
        let logged = logged_uri("GET", "/Journeys/shared/tok/extra").await;
        assert_eq!(logged, "/Journeys/shared/[REDACTED]");
    }

    #[test]
    fn a_long_unmatched_path_is_capped() {
        let long = "/".to_string() + &"x".repeat(5000);
        let logged = redact_unmatched_path(&long);
        assert!(logged.len() < 220, "{}", logged.len());
        assert!(logged.ends_with("[...]"));
    }

    #[test]
    fn the_bare_share_prefix_is_left_alone() {
        assert_eq!(
            redact_unmatched_path("/Journeys/shared/"),
            "/Journeys/shared/"
        );
    }
}
