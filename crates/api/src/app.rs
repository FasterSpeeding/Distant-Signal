use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};

use crate::auth::oidc::{OidcClient, OidcConfig};
use crate::data::config::ServiceArguments;

pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    /// Deliberately a lazy `redis::Client`, NOT a live
    /// `redis::aio::ConnectionManager`. `Client::open` only parses the URL --
    /// it never opens a socket -- so an unreachable Redis cannot fail
    /// `AppState::init` and crash-loop the whole public status API. The one
    /// consumer (`data::queries::upsert_incidents`) connects at publish time
    /// and already logs-and-continues on failure, and the enricher's hourly
    /// sweep is the backstop for anything that misses the stream. A broken
    /// enrichment path must never be able to take displayed status down.
    pub redis: redis::Client,
    /// OIDC relying-party client -- see `auth::oidc`'s module doc for why
    /// discovery is lazy (not performed here in `init`).
    pub oidc: OidcClient,
    /// Verifies an incoming `/private/*` request's `Authorization: Bearer`
    /// token against Authentik's JWKS -- see
    /// `crate::auth::internal_oauth`.
    pub internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier,
    /// (Route prefix, HTTP method, required group names), built once here
    /// from config. The method dimension is load-bearing: `/stanox-crs`
    /// has two legitimate callers with DIFFERENT methods --
    /// `trust-consumer` only ever `GET`s it (a read-only reference
    /// reload) and `schedule-reference` only ever `POST`s it (its
    /// per-sequence write) -- so each caller gets its own `(prefix,
    /// method)` entry with exactly its own group, never the other
    /// caller's group. A single path-keyed-only table (no method
    /// dimension) previously let EITHER caller's token authorize BOTH
    /// methods on this route -- trust-consumer's read-only token could
    /// `POST` (corrupt the reference table), and schedule-reference's
    /// write token could `GET` -- see
    /// docs/superpowers/plans/2026-09-02-internal-service-oauth2.md's
    /// security review. Every other route in this table happens to have
    /// exactly one caller today, so its entry (or entries, for a caller
    /// that legitimately uses both `GET` and `POST` on the same path)
    /// carries only that caller's group regardless -- see
    /// `build_internal_oauth_routes`.
    pub internal_oauth_routes: Vec<(&'static str, axum::http::Method, Vec<String>)>,
    /// CRS -> candidate line_ids, built once here from `config.lines`
    /// (Decision 2 of
    /// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
    /// Consulted by `routes::train::post_track` and the periodic
    /// schedule-match sweep (`main.rs`) -- never mutated after startup,
    /// same "load once, refresh only on process restart" posture as
    /// `config.lines` itself already has.
    pub schedule_crs_line_index: std::collections::HashMap<String, Vec<String>>,
    /// The incident->lines matcher, built once here from `config.lines`
    /// (same "load once, refresh only on process restart" posture as
    /// `schedule_crs_line_index` above).
    ///
    /// `data::queries::upsert_incidents` runs it over every incoming
    /// incident so `incidents.affected_lines` records the SAME line
    /// attribution the aggregator computes for live status, rather than the
    /// archive answering "which lines?" its own, different way. See
    /// `common::matcher`'s module doc for the defect that came from the
    /// latter.
    pub line_matcher: common::matcher::LineMatcher,
}

/// Builds `AppState::internal_oauth_routes` from config. Factored out of
/// `AppState::init` so tests (`crate::auth`'s route-scoping test suite)
/// can exercise the REAL production table -- not a hand-copied stand-in
/// that could silently drift from it -- without needing every other part
/// of `AppState::init` (a live database connection, etc.).
pub(crate) fn build_internal_oauth_routes(
    config: &ServiceArguments,
) -> Vec<(&'static str, axum::http::Method, Vec<String>)> {
    use axum::http::Method;

    vec![
        (
            "/incidents",
            Method::GET,
            vec![config.internal_oauth_group_incidents.clone()],
        ),
        (
            "/incidents",
            Method::POST,
            vec![config.internal_oauth_group_incidents.clone()],
        ),
        (
            "/stations",
            Method::GET,
            vec![config.internal_oauth_group_stations.clone()],
        ),
        (
            "/stations",
            Method::POST,
            vec![config.internal_oauth_group_stations.clone()],
        ),
        (
            "/tocs",
            Method::GET,
            vec![config.internal_oauth_group_tocs.clone()],
        ),
        (
            "/tocs",
            Method::POST,
            vec![config.internal_oauth_group_tocs.clone()],
        ),
        (
            "/station-samples",
            Method::GET,
            vec![config.internal_oauth_group_ldbws.clone()],
        ),
        (
            "/station-samples",
            Method::POST,
            vec![config.internal_oauth_group_ldbws.clone()],
        ),
        // GET-only: `samples::router()` never wires a POST handler for
        // this path at all.
        (
            "/sample-stations",
            Method::GET,
            vec![config.internal_oauth_group_ldbws.clone()],
        ),
        (
            "/tfl-line-status",
            Method::GET,
            vec![config.internal_oauth_group_tfl.clone()],
        ),
        (
            "/tfl-line-status",
            Method::POST,
            vec![config.internal_oauth_group_tfl.clone()],
        ),
        // POST-only: trust-consumer's per-poll-cycle event batch --
        // `ingest::router()` never wires a GET handler for this path.
        (
            "/train-events",
            Method::POST,
            vec![config.internal_oauth_group_trust_consumer.clone()],
        ),
        // POST-only, same caller/group as /train-events: trust-consumer's
        // notifier-forwarding queue signals (Task 17) -- ingest::router()
        // never wires a GET handler for this path either.
        (
            "/train-forward-signals",
            Method::POST,
            vec![config.internal_oauth_group_trust_consumer.clone()],
        ),
        // GET-only: trust-consumer's periodic tracked-trains reload --
        // `ingest::router()` never wires a POST handler for this path.
        (
            "/tracked-trains",
            Method::GET,
            vec![config.internal_oauth_group_trust_consumer.clone()],
        ),
        // POST-only: trust-backlog-consumer's per-cycle event batch --
        // ingest::router() never wires a GET handler for this path,
        // mirroring /train-events exactly (see that entry's own comment).
        (
            "/trust-event-backlog",
            Method::POST,
            vec![config.internal_oauth_group_trust_backlog.clone()],
        ),
        // ONE reader again: schedule-ingest reading back its own last write.
        //
        // schedule-reference's read grant here was REMOVED (2026-09-25): it
        // used to seed its restart dedup marker from this route, which was
        // the bug -- this route reports when schedule-INGEST extracted a
        // delivery, not whether schedule-reference ever published it. It now
        // reads its own `/schedule-reference-publishes` marker instead (just
        // below), so it has no business reading this route at all any more,
        // and a grant no caller needs is a grant that should not exist. The
        // only cost of revoking it is during the rolling deploy that lands
        // this change: an old `reference` pod still GETting this route gets a
        // 403, which `seed_last_processed_delivery` already handles as
        // "fall back to first-run behavior" (a redundant republish of a
        // delivery it had already published -- wasteful for one cycle, never
        // data loss).
        (
            "/schedule-feed-ingests",
            Method::GET,
            vec![config.internal_oauth_group_schedule_ingest.clone()],
        ),
        (
            "/schedule-feed-ingests",
            Method::POST,
            vec![config.internal_oauth_group_schedule_ingest.clone()],
        ),
        // BOTH methods, ONE group -- schedule-reference reading back its own
        // last write, exactly the shape /full-coverage-stats below documents
        // ("this producer reading back its own last write, not a second
        // caller"), and deliberately NOT /schedule-feed-ingests' split shape
        // directly above: no other service writes or reads this marker.
        //
        // This route exists because seeding schedule-reference's restart
        // dedup from /schedule-feed-ingests (schedule-INGEST's
        // extraction-time record) made a restart mid-processing skip a
        // delivery that had never actually been published. schedule-reference
        // now writes its own completion marker here, once per delivery, only
        // after every product for that delivery has published successfully.
        // See crates/api/migrations/20260925130000_schedule_reference_publishes.sql.
        (
            "/schedule-reference-publishes",
            Method::GET,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        (
            "/schedule-reference-publishes",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // Split by method, NOT a shared two-group entry: trust-consumer,
        // full-coverage-consumer, and trust-backlog-consumer only ever GET
        // (read-only reload), schedule-reference only ever POSTs (its
        // write) -- see this field's own doc comment on
        // `AppState::internal_oauth_routes` for why a merged entry here
        // was the actual security gap this table's method dimension
        // fixes.
        //
        // full-coverage-consumer's own group was missing here entirely
        // until now, even though its config.rs has carried a
        // stanox_crs_url since Deploy A -- confirmed live: its GET
        // requests 403'd on a genuinely valid, correctly-scoped token
        // (right group for every OTHER route it calls, e.g.
        // /station-full-coverage-samples and /full-coverage-stats),
        // because this one entry never listed its group as an accepted
        // caller. trust-backlog-consumer's group was likewise missing --
        // its hourly STANOX/CRS reference-table reload 403'd on an
        // otherwise valid, correctly-scoped token for the same reason.
        // All three GET callers share this entry (the `groups.iter().any`
        // check in auth.rs) rather than getting split rows, since none
        // is granted write access here -- only /stanox-crs POST (below,
        // schedule-reference only) is a write.
        (
            "/stanox-crs",
            Method::GET,
            vec![
                config.internal_oauth_group_trust_consumer.clone(),
                config.internal_oauth_group_full_coverage.clone(),
                config.internal_oauth_group_trust_backlog.clone(),
            ],
        ),
        (
            "/stanox-crs",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // POST-only, no GET pair -- same shape as
        // /schedule-network-departures below, reusing schedule-reference's
        // EXISTING writer credential (the same one /stanox-crs's own POST
        // above already uses). Task 3's `trip_planning.rs` reads
        // `tiploc_crs` straight off `api`'s own database via
        // `queries::list_tiploc_crs`, not via a private ingest route, so
        // there is no reader here needing its own GET entry -- see
        // docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md.
        (
            "/tiploc-crs",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        (
            "/station-full-coverage-samples",
            Method::GET,
            vec![config.internal_oauth_group_full_coverage.clone()],
        ),
        (
            "/station-full-coverage-samples",
            Method::POST,
            vec![config.internal_oauth_group_full_coverage.clone()],
        ),
        // Also split by method: schedule-reference publishes (POST),
        // full-coverage-consumer reads the real rows back (GET) -- same
        // "different services, different groups" shape as /stanox-crs
        // above, per Correction 2.
        (
            "/schedule-line-population",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        (
            "/schedule-line-population",
            Method::GET,
            vec![config.internal_oauth_group_full_coverage.clone()],
        ),
        // POST-only: no GET pair, unlike /schedule-line-population -- see
        // Corrections/Task 2's own note in
        // docs/superpowers/plans/2026-09-04-whole-network-trip-search-plan.md.
        // Reuses schedule-reference's EXISTING writer credential, the same one
        // /stanox-crs and /schedule-line-population's own POST already use.
        (
            "/schedule-network-departures",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // POST-only, same as /schedule-network-departures directly above,
        // and reusing schedule-reference's EXISTING writer credential --
        // the same one /stanox-crs, /schedule-line-population and
        // /schedule-network-departures already use. A fourth product from
        // the same producer is not a fourth identity.
        (
            "/schedule-destination-departures",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // POST-only, same as /schedule-network-departures and
        // /schedule-destination-departures above, and reusing
        // schedule-reference's EXISTING writer credential -- no new config
        // field needed. No GET pair: a GET-by-CRS read route for
        // `fixed_links` belongs to Phase 2, not this ingestion task.
        (
            "/fixed-links",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // POST-only, Dynamic Trip Planning Phase 2's newest publish from
        // this same producer -- the fifth route this producer's table
        // wires overall (line-population, network-departures,
        // destination-departures, fixed-links, and now this), though the
        // FOURTH product built off the shared per-cycle `ScheduleIndex`
        // specifically -- `fixed-links` above is ALF-derived and published
        // via its own separate call, not through
        // `publish_cif_derived_products`'s loop; see that function's own
        // per-product ordinal comments in `schedule-reference/src/main.rs`.
        // Reuses schedule-reference's EXISTING writer credential, same as
        // every publish above. No GET pair: Task 3's
        // `trip_planning::fetch_calling_points_for_date` reads this table
        // directly from `api`'s own database, not via a private ingest
        // route.
        (
            "/schedule-calling-points-full",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
        // Same group, both methods -- this producer reading back its own
        // last write, not a second caller (see Correction 2).
        (
            "/full-coverage-stats",
            Method::POST,
            vec![config.internal_oauth_group_full_coverage.clone()],
        ),
        (
            "/full-coverage-stats",
            Method::GET,
            vec![config.internal_oauth_group_full_coverage.clone()],
        ),
        // Two independent producers write to each of these tables now --
        // poller-irish-rail-gtfs (RepublicOfIreland rows) and
        // poller-nir-stations (NorthernIreland rows) -- so both GET and
        // POST accept either credential. See
        // docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
        // Task 1.
        (
            "/island-of-ireland-stations",
            Method::GET,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-stations",
            Method::POST,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-lines",
            Method::GET,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-lines",
            Method::POST,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-station-samples",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_live.clone()],
        ),
        (
            "/island-of-ireland-station-samples",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_live.clone()],
        ),
    ]
}

/// Startup guard (2026-09-25 Low-severity auth-core review): the
/// user-facing SSO client and the internal-service OAuth2 client must
/// never be configured with the SAME `client_id` -- see the call site in
/// `AppState::init` for the full rationale. Factored out as its own free
/// function (rather than an inline `ensure!` in `init`) purely so this one
/// check is unit-testable without needing a live database connection --
/// everything else `AppState::init` does before reaching this point
/// (connecting to Postgres, parsing `REDIS_URL`) is not something a plain
/// `#[test]` can exercise cheaply.
fn ensure_sso_and_internal_oauth_clients_differ(
    sso_client_id: &str,
    internal_oauth_client_id: &str,
) -> Result<()> {
    ensure!(
        sso_client_id != internal_oauth_client_id,
        "sso_client_id and internal_oauth_client_id must be different values -- both are \
         configured to \"{sso_client_id}\", which would let a human SSO login and an \
         internal-service bearer token satisfy each other's audience check"
    );
    Ok(())
}

/// Hand-rolled rather than `#[derive(Debug)]`. Two independent reasons:
///
/// 1. `OidcClient` holds a `reqwest::Client` and a
///    `tokio::sync::OnceCell<CoreClient<..>>`, and `openidconnect`'s
///    `CoreClient` does not implement `Debug` -- a derived `Debug` on
///    `AppState` simply fails to compile once the `oidc` field exists.
/// 2. Even setting that aside, `config: ServiceArguments` carries
///    `sso_client_secret`/`database_url` (which itself
///    embeds the Postgres password) -- printing it via its own derived
///    `Debug` would leak both the moment anything ever
///    debug-formats an `AppState`/`App` value. Nothing in this codebase
///    does that today, but a hand-rolled impl that never touches those
///    fields is cheap insurance against a future `tracing::debug!(?app,
///    ...)` accidentally doing so.
///
/// Every field below is therefore a fixed placeholder, not a real dump of
/// the field's contents -- this exists only so `#[derive(Debug)]`-adjacent
/// tooling (e.g. `{:?}` in a panic message) doesn't itself panic or leak
/// secrets, not to make `AppState` genuinely inspectable.
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &"ServiceArguments { .. }")
            .field("database", &"PgPool { .. }")
            .field("redis", &"redis::Client { .. }")
            .field("oidc", &"OidcClient { .. }")
            .field("internal_oauth_verifier", &"ServiceTokenVerifier { .. }")
            .field("internal_oauth_routes", &self.internal_oauth_routes)
            .field("schedule_crs_line_index", &self.schedule_crs_line_index)
            .finish()
    }
}

pub type App = Arc<AppState>;
pub type Router = axum::Router<App>;

impl AppState {
    pub async fn init() -> Result<App> {
        let config = ServiceArguments::parse();

        // sqlx's own `ConnectOptions` default for `log_slow_statements` is
        // WARN at a 1-second threshold -- fine for typical interactive
        // queries, but this crate's own bulk write path
        // (`queries::upsert_schedule_destination_departures`'s `INSERT ...
        // SELECT * FROM UNNEST(...) ON CONFLICT DO NOTHING`, batching up to
        // ~250k rows and observed taking 3-8 seconds per batch in
        // production) trips that default on every single batch, drowning
        // real slow-query signal in expected noise. No crate in this
        // workspace calls `.log_slow_statements(...)` anywhere (confirmed
        // by grep), so every pool -- this one included -- has been
        // inheriting that 1-second default uniformly.
        //
        // Raised here, pool-wide, to 10 seconds -- not a second,
        // query-specific pool -- because `api` has exactly one `PgPool` for
        // the whole service (this one call site) and introducing a second
        // pool solely to scope a threshold to one query would be
        // disproportionate. The tradeoff is acceptable because every other,
        // genuinely-interactive query in this crate runs comfortably under
        // a second in practice, so a 10-second floor changes nothing for
        // them; for the bulk path it still leaves ~2-3x headroom over the
        // slowest real batches observed (3-8s) to absorb ordinary load
        // variance, while a genuinely pathological multi-minute query would
        // still trip it immediately, same as before.
        let connect_options: PgConnectOptions = config
            .database_url
            .parse()
            .context("could not parse DATABASE_URL")?;
        let connect_options =
            connect_options.log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(10));

        let db = PgPoolOptions::new()
            .max_connections(50)
            .connect_with(connect_options)
            .await
            .context("Could not connect to database")?;

        // No eager connect: only the URL is validated here. See the `redis`
        // field's doc comment on `AppState`.
        let redis =
            redis::Client::open(config.redis_url.clone()).context("Could not parse REDIS_URL")?;

        // An empty client secret would make every future confidential-client
        // token exchange fail anyway, but only after a real user has already
        // been redirected all the way to the IdP and back -- reject at
        // startup instead, matching the internal-oauth guards below.
        ensure!(
            !config.sso_client_secret.is_empty(),
            "sso_client_secret (--sso-client-secret / SSO_CLIENT_SECRET) must not be empty"
        );

        // `data::users::insert_session` binds `session_ttl_days` into
        // `make_interval(days => $3)` as an `i32` (sqlx sends a plain
        // integer `days` argument as `INT4`), but the config field itself
        // is `i64` -- clap's parser happily accepts any in-range `i64`,
        // including one past `i32::MAX` (~5.8 million days, absurd for a
        // real deployment but not something the parser itself rejects).
        // Checked here, once, at startup, rather than left to silently
        // wrap on first login: `insert_session` re-checks this same
        // conversion itself (defense in depth, see its own doc comment),
        // but a bad value should never get that far -- it should fail the
        // deploy immediately, with a message naming the actual env var,
        // the same posture every other guard in this block already takes.
        i32::try_from(config.session_ttl_days).context(
            "session_ttl_days (--session-ttl-days / SESSION_TTL_DAYS) does not fit in a \
             32-bit day count -- sessions.expires_at is computed via \
             make_interval(days => ...), which requires an i32",
        )?;

        let oidc = OidcClient::new(OidcConfig {
            issuer_url: config.sso_issuer_url.clone(),
            client_id: config.sso_client_id.clone(),
            client_secret: config.sso_client_secret.clone(),
            redirect_url: config.sso_redirect_url.clone(),
        })
        .context("failed to construct OIDC client")?;

        // `sso_client_id` (a real human's own browser-based login, verified
        // by `OidcClient`/`openidconnect`'s ID-token verifier) and
        // `internal_oauth_client_id` (the `aud` every verified `/private/*`
        // bearer token must carry, checked by `ServiceTokenVerifier::verify`
        // below) are two entirely separate identities by design --
        // `internal_oauth_issuer_url`'s own doc comment on `ServiceArguments`
        // already documents that they MAY legitimately share the same
        // Authentik instance (issuer), just under different
        // Applications/Providers, so issuer equality is never checked here.
        // client_id equality is a different matter: nothing about the two
        // verifiers' own logic prevents a token that satisfies one client
        // id's audience check from also satisfying the other if a
        // chart/secret-wiring mistake ever handed both the SAME client_id --
        // a human's own SSO-issued ID token could then pass
        // `ServiceTokenVerifier::verify`'s `aud` check (or a
        // client-credentials access token could pass the SSO side), letting
        // a real person's browser session masquerade as a trusted internal
        // service credential, or vice versa. Today's chart wires genuinely
        // distinct values for both (this is a misconfiguration-only risk),
        // but nothing before this guard actually verified that at startup --
        // fail loudly here rather than let a wiring mistake pass silently
        // into production. See `ensure_sso_and_internal_oauth_clients_differ`
        // for why this check is its own free function.
        ensure_sso_and_internal_oauth_clients_differ(
            &config.sso_client_id,
            &config.internal_oauth_client_id,
        )?;

        // An empty required-group value must never silently become "any
        // group matches" -- the same failure class the old shared-secret
        // design guarded against for its own credential (see the startup
        // guard this replaces, formerly against a now-deleted config
        // field). issuer_url/client_id are guarded too: an empty
        // issuer_url would make IssuerUrl::new("") fail inside
        // ServiceTokenVerifier::new below anyway, but failing here first
        // gives a clearer message naming the actual env var.
        for (name, value) in [
            (
                "internal_oauth_issuer_url",
                &config.internal_oauth_issuer_url,
            ),
            ("internal_oauth_client_id", &config.internal_oauth_client_id),
            (
                "internal_oauth_group_incidents",
                &config.internal_oauth_group_incidents,
            ),
            (
                "internal_oauth_group_stations",
                &config.internal_oauth_group_stations,
            ),
            (
                "internal_oauth_group_tocs",
                &config.internal_oauth_group_tocs,
            ),
            (
                "internal_oauth_group_ldbws",
                &config.internal_oauth_group_ldbws,
            ),
            ("internal_oauth_group_tfl", &config.internal_oauth_group_tfl),
            (
                "internal_oauth_group_trust_consumer",
                &config.internal_oauth_group_trust_consumer,
            ),
            (
                "internal_oauth_group_schedule_ingest",
                &config.internal_oauth_group_schedule_ingest,
            ),
            (
                "internal_oauth_group_schedule_reference",
                &config.internal_oauth_group_schedule_reference,
            ),
            (
                "internal_oauth_group_full_coverage",
                &config.internal_oauth_group_full_coverage,
            ),
            (
                "internal_oauth_group_trust_backlog",
                &config.internal_oauth_group_trust_backlog,
            ),
            (
                "internal_oauth_group_irish_rail_gtfs",
                &config.internal_oauth_group_irish_rail_gtfs,
            ),
            (
                "internal_oauth_group_irish_rail_live",
                &config.internal_oauth_group_irish_rail_live,
            ),
            (
                "internal_oauth_group_nir_stations",
                &config.internal_oauth_group_nir_stations,
            ),
        ] {
            ensure!(
                !value.is_empty(),
                "{name} must not be empty (see --{}/{})",
                name.replace('_', "-"),
                name.to_uppercase()
            );
        }

        let internal_oauth_verifier = crate::auth::internal_oauth::ServiceTokenVerifier::new(
            config.internal_oauth_issuer_url.clone(),
            config.internal_oauth_client_id.clone(),
        )
        .context("failed to construct internal-oauth verifier")?;

        let internal_oauth_routes = build_internal_oauth_routes(&config);

        let schedule_crs_line_index =
            crate::data::schedule_matching::crs_to_line_ids(&config.lines);

        let line_matcher = common::matcher::LineMatcher::new(&config.lines);

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
            internal_oauth_verifier,
            internal_oauth_routes,
            schedule_crs_line_index,
            line_matcher,
        }))
    }
}

#[cfg(test)]
mod internal_oauth_startup_guard_tests {
    use super::ensure_sso_and_internal_oauth_clients_differ;

    #[test]
    fn distinct_client_ids_pass() {
        assert!(
            ensure_sso_and_internal_oauth_clients_differ(
                "human-login-client",
                "svc-internal-client"
            )
            .is_ok()
        );
    }

    #[test]
    fn identical_client_ids_fail_loudly() {
        let err = ensure_sso_and_internal_oauth_clients_differ("same-id", "same-id")
            .expect_err("identical client ids must be rejected at startup");
        assert!(err.to_string().contains("sso_client_id"));
        assert!(err.to_string().contains("internal_oauth_client_id"));
    }
}
