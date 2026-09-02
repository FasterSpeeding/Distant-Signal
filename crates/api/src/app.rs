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
    /// Startup-built token-hash -> identity lookup for `/private/*`
    /// (Decision 1/2). Built once in `init`, immutable afterward -- no
    /// runtime credential add/remove without a redeploy, per Decision 2's
    /// explicit rejection of a DB-backed, dynamically-editable table.
    pub internal_services: crate::auth::InternalServiceRegistry,
}

/// Hand-rolled rather than `#[derive(Debug)]`. Two independent reasons:
///
/// 1. `OidcClient` holds a `reqwest::Client` and a
///    `tokio::sync::OnceCell<CoreClient<..>>`, and `openidconnect`'s
///    `CoreClient` does not implement `Debug` -- a derived `Debug` on
///    `AppState` simply fails to compile once the `oidc` field exists.
/// 2. Even setting that aside, `config: ServiceArguments` carries
///    `sso_client_secret`/`internal_token`/`database_url` (which itself
///    embeds the Postgres password) -- printing it via its own derived
///    `Debug` would leak all three the moment anything ever
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
            .field("internal_services", &"InternalServiceRegistry { .. }")
            .finish()
    }
}

pub type App = Arc<AppState>;
pub type Router = axum::Router<App>;

impl AppState {
    pub async fn init() -> Result<App> {
        let config = ServiceArguments::parse();

        // An empty token would let InternalServiceRegistry::resolve match
        // an empty raw token (hashed the same as anything else) against an
        // empty `X-Internal-Token` header -- reject that at startup rather
        // than silently running an unauthenticated `private_router()`.
        ensure!(
            !config.internal_token.is_empty(),
            "internal_token (--internal-token / INTERNAL_TOKEN) must not be empty"
        );

        // Same failure class as the legacy internal_token guard above,
        // applied to each of the seven per-service tokens: an empty value
        // would let InternalServiceRegistry::resolve match an empty
        // X-Internal-Token header against it.
        for (name, value) in [
            ("internal_token_poller_incidents", &config.internal_token_poller_incidents),
            ("internal_token_poller_stations", &config.internal_token_poller_stations),
            ("internal_token_poller_tocs", &config.internal_token_poller_tocs),
            ("internal_token_poller_ldbws", &config.internal_token_poller_ldbws),
            ("internal_token_poller_tfl", &config.internal_token_poller_tfl),
            ("internal_token_trust_consumer", &config.internal_token_trust_consumer),
            ("internal_token_schedule_ingest", &config.internal_token_schedule_ingest),
        ] {
            ensure!(
                !value.is_empty(),
                "{name} (--{}/{}) must not be empty",
                name.replace('_', "-"),
                name.to_uppercase()
            );
        }

        let internal_services = crate::auth::InternalServiceRegistry::from_tokens(&[
            (crate::auth::InternalService::Legacy, config.internal_token.as_str()),
            (crate::auth::InternalService::PollerIncidents, config.internal_token_poller_incidents.as_str()),
            (crate::auth::InternalService::PollerStations, config.internal_token_poller_stations.as_str()),
            (crate::auth::InternalService::PollerTocs, config.internal_token_poller_tocs.as_str()),
            (crate::auth::InternalService::PollerLdbws, config.internal_token_poller_ldbws.as_str()),
            (crate::auth::InternalService::PollerTfl, config.internal_token_poller_tfl.as_str()),
            (crate::auth::InternalService::TrustConsumer, config.internal_token_trust_consumer.as_str()),
            (crate::auth::InternalService::ScheduleIngest, config.internal_token_schedule_ingest.as_str()),
        ]);

        let db = PgPoolOptions::new()
            .max_connections(50)
            .connect(&config.database_url)
            .await
            .context("Could not connect to database")?;

        // No eager connect: only the URL is validated here. See the `redis`
        // field's doc comment on `AppState`.
        let redis = redis::Client::open(config.redis_url.clone())
            .context("Could not parse REDIS_URL")?;

        // An empty client secret would make every future confidential-client
        // token exchange fail anyway, but only after a real user has already
        // been redirected all the way to the IdP and back -- reject at
        // startup instead, matching `internal_token`'s posture above.
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

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
            internal_services,
        }))
    }
}
