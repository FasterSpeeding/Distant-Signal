use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use sqlx::postgres::PgConnectOptions;
use sqlx::{ConnectOptions, PgPool};

use crate::auth::oidc::{OidcClient, OidcConfig};
use crate::data::config::ServiceArguments;

pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    /// OIDC relying-party client -- see `auth::oidc`'s module doc for why
    /// discovery is lazy (not performed here in `init`).
    pub oidc: OidcClient,
    /// Verifies the Distant-Signal-MCP's `Authorization: Bearer` service
    /// token against Authentik's JWKS, for its own rate-limit budget
    /// (`crate::rate_limit`) -- see `crate::auth::internal_oauth`.
    pub internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier,
    /// CRS -> candidate `line_ids`, built once here from `config.lines`
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
    /// `routes::incidents` checks a requested line filter against it; the
    /// writers (poller-incidents, through `ds_store::incidents`) fill
    /// `incidents.affected_lines` with the same matcher. See
    /// `common::matcher`'s module doc.
    pub line_matcher: common::matcher::LineMatcher,
}

/// Dead-client detection (`client_connection_check_interval` and TCP
/// keepalives), now shared with every pool through `common::pg`; kept here
/// for the migration connection, which is not built by `PoolSettings`.
const DEAD_CLIENT_DETECTION_SETTINGS: [(&str, &str); 4] =
    common::pg::DEAD_CLIENT_DETECTION_SETTINGS;

/// `pg_stat_activity.application_name` of every `api` pool connection.
pub const API_APPLICATION_NAME: &str = "distant-signal-api";

pub fn with_dead_client_detection(options: PgConnectOptions) -> PgConnectOptions {
    options.options(DEAD_CLIENT_DETECTION_SETTINGS)
}

/// Startup guard (2026-09-25 Low-severity auth-core review): the
/// user-facing SSO client and the internal-service `OAuth2` client must
/// never be configured with the SAME `client_id` -- see the call site in
/// `AppState::init` for the full rationale. Factored out as its own free
/// function (rather than an inline `ensure!` in `init`) purely so this one
/// check is unit-testable without needing a live database connection --
/// everything else `AppState::init` does before reaching this point
/// (connecting to Postgres) is not something a plain
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
#[expect(
    clippy::missing_fields_in_debug,
    reason = "deliberately prints placeholders and omits clients and secrets; see above"
)]
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("config", &"ServiceArguments { .. }")
            .field("database", &"PgPool { .. }")
            .field("oidc", &"OidcClient { .. }")
            .field("internal_oauth_verifier", &"ServiceTokenVerifier { .. }")
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
        let connect_options = with_dead_client_detection(
            connect_options.log_slow_statements(log::LevelFilter::Warn, Duration::from_secs(10)),
        );

        // INF-5: wait for Postgres (e.g. still in crash recovery after a
        // node reboot) rather than exiting into CrashLoopBackOff. The
        // listener isn't bound until after this and the migrations, so the
        // startup/readiness probes stay failing meanwhile.
        common::startup::retry_until_ready(
            "Postgres",
            common::startup::CONNECT_BACKOFF,
            None,
            || async {
                use sqlx::Connection;
                sqlx::PgConnection::connect_with(&connect_options)
                    .await?
                    .close()
                    .await
            },
        )
        .await;

        // application_name, statement_timeout (60s), idle-in-transaction
        // timeout (30s) and a 5s acquire_timeout, all overridable by env --
        // see `common::pg`. Statements that legitimately run longer (the
        // schedule publish chunks) raise the timeout with `SET LOCAL`.
        // `ds_store::pool` builds the same pool and samples it into
        // `distant_signal_db_pool_*` (plan task 1A.11).
        let pool_settings = ds_store::pool::PoolSettings::from_env(API_APPLICATION_NAME, 50)?;
        let db = pool_settings
            .connect_with(connect_options)
            .await
            .context("Could not connect to database")?;

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
        // `internal_oauth_client_id` (the `aud` every verified service
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

        // An empty issuer_url would make IssuerUrl::new("") fail inside
        // ServiceTokenVerifier::new below anyway, but failing here first
        // gives a clearer message naming the actual env var; an empty
        // client_id would accept any audience.
        for (name, value) in [
            (
                "internal_oauth_issuer_url",
                &config.internal_oauth_issuer_url,
            ),
            ("internal_oauth_client_id", &config.internal_oauth_client_id),
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

        let schedule_crs_line_index =
            crate::data::schedule_matching::crs_to_line_ids(&config.lines);

        let line_matcher = common::matcher::LineMatcher::new(&config.lines);

        Ok(Arc::new(Self {
            config,
            database: db,
            oidc,
            internal_oauth_verifier,
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

    /// Release B (ingest phase 5) removed the `/private` routes and their
    /// groups: `INTERNAL_OAUTH_GROUP_MCP` (default `srv-ds-mcp`) is the only
    /// `INTERNAL_OAUTH_GROUP_*` left.
    #[test]
    fn the_mcp_group_is_the_only_internal_oauth_group() {
        use clap::CommandFactory;
        let command = super::ServiceArguments::command();
        let defaults: Vec<(String, String)> = command
            .get_arguments()
            .filter_map(|arg| {
                let env = arg.get_env()?.to_str()?.to_string();
                let default = arg.get_default_values().first()?.to_str()?.to_string();
                env.starts_with("INTERNAL_OAUTH_GROUP_")
                    .then_some((env, default))
            })
            .collect();
        assert_eq!(
            defaults,
            vec![(
                "INTERNAL_OAUTH_GROUP_MCP".to_string(),
                "srv-ds-mcp".to_string()
            )]
        );
    }
}

#[cfg(test)]
mod dead_client_detection_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// The settings actually reach the session (and are accepted by the
    /// server -- an unknown one would fail the connection outright).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                dead_client_detection -- --ignored --test-threads=1`"]
    async fn pool_connections_carry_the_dead_client_detection_settings() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let options: PgConnectOptions = database_url.parse().expect("parse DATABASE_URL");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(with_dead_client_detection(options))
            .await
            .expect("connect with the settings");
        for (name, expected) in [
            ("client_connection_check_interval", "10s"),
            ("tcp_keepalives_count", "6"),
        ] {
            let value: String = sqlx::query_scalar(&format!("SHOW {name}"))
                .fetch_one(&pool)
                .await
                .expect("SHOW setting");
            assert_eq!(value, expected, "{name}");
        }
    }

    /// The pool `AppState::init` builds: the `common::pg` settings stack on
    /// top of the dead-client detection options rather than replacing them.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                dead_client_detection -- --ignored --test-threads=1`"]
    async fn pool_settings_and_dead_client_detection_both_apply() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let options: PgConnectOptions = database_url.parse().expect("parse DATABASE_URL");
        let settings = ds_store::pool::PoolSettings::new(API_APPLICATION_NAME, 1);
        let pool = settings
            .connect_with(with_dead_client_detection(options))
            .await
            .expect("connect with the settings");
        for (name, expected) in [
            ("client_connection_check_interval", "10s"),
            ("statement_timeout", "1min"),
            ("idle_in_transaction_session_timeout", "30s"),
            ("application_name", API_APPLICATION_NAME),
        ] {
            let value: String = sqlx::query_scalar(&format!("SHOW {name}"))
                .fetch_one(&pool)
                .await
                .expect("SHOW setting");
            assert_eq!(value, expected, "{name}");
        }
    }
}
