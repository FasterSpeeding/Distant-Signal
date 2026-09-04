use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::{PgPool, postgres::PgPoolOptions};

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
        // GET-only: trust-consumer's periodic tracked-trains reload --
        // `ingest::router()` never wires a POST handler for this path.
        (
            "/tracked-trains",
            Method::GET,
            vec![config.internal_oauth_group_trust_consumer.clone()],
        ),
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
        // Split by method, NOT a shared two-group entry: trust-consumer
        // only ever GETs (read-only reload), schedule-reference only
        // ever POSTs (its write) -- see this field's own doc comment on
        // `AppState::internal_oauth_routes` for why a merged entry here
        // was the actual security gap this table's method dimension
        // fixes.
        (
            "/stanox-crs",
            Method::GET,
            vec![config.internal_oauth_group_trust_consumer.clone()],
        ),
        (
            "/stanox-crs",
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
    ]
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
            .finish()
    }
}

pub type App = Arc<AppState>;
pub type Router = axum::Router<App>;

impl AppState {
    pub async fn init() -> Result<App> {
        let config = ServiceArguments::parse();

        let db = PgPoolOptions::new()
            .max_connections(50)
            .connect(&config.database_url)
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

        let oidc = OidcClient::new(OidcConfig {
            issuer_url: config.sso_issuer_url.clone(),
            client_id: config.sso_client_id.clone(),
            client_secret: config.sso_client_secret.clone(),
            redirect_url: config.sso_redirect_url.clone(),
        })
        .context("failed to construct OIDC client")?;

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

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
            internal_oauth_verifier,
            internal_oauth_routes,
        }))
    }
}
