use std::path::PathBuf;

use anyhow::Result;
use clap::ValueHint;
use serde::de::DeserializeOwned;

use crate::data::LineDefinition;

pub use common::Defaults;

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn parse_lines(path: &str) -> Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}

/// Newtype around the parsed line catalogue.
///
/// `clap_derive` infers the type it downcasts an `ArgMatches` entry to from
/// the field's *syntactic* shape, not from the `value_parser`'s `Value`
/// type: a bare `Vec<LineDefinition>` field is always treated as "one
/// `LineDefinition` per CLI occurrence, collected via `ArgAction::Append`" —
/// confirmed by a runtime panic ("Mismatch between definition and access of
/// `lines`") the moment `--lines-dir`/`LINES_DIR`/`default_value` actually
/// supplied a value, which nothing did before this field had a default.
/// `parse_lines` instead produces the *entire* vec from a single
/// `--lines-dir` occurrence, so the field type must not look like `Vec<T>`
/// to the derive macro. This newtype (plus `Deref`) sidesteps that:
/// `app.config.lines` still coerces to `&[LineDefinition]` at every existing
/// call site (`crate::routes::samples`, `data::samples::dedup_sample_stations`)
/// with no changes needed there.
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, clap::Parser)]
pub struct ServiceArguments {
    #[arg(short, long, env, default_value = "0.0.0.0:8080")]
    pub bind_url: String,
    #[arg(short, long, env)]
    pub database_url: String,
    #[arg(long, env)]
    pub redis_url: String,
    /// OIDC issuer base URL for the internal-service OAuth2 provider
    /// (Authentik) -- JWKS endpoint is learned via standard OIDC
    /// discovery against this URL, same mechanism as `sso_issuer_url`
    /// below. May be the same Authentik instance as `sso_issuer_url` (a
    /// different Application/Provider under it) or a different one --
    /// operator's call.
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_issuer_url: String,

    /// Expected `aud` claim on a verified internal-service access token --
    /// the shared Authentik OAuth2 Provider's own client_id (Decision 1:
    /// one provider, 8 service accounts underneath it). Must match every
    /// real caller's own `internal_oauth_client_id` (its own config) --
    /// same value, independently configured on each side.
    #[arg(long, env)]
    pub internal_oauth_client_id: String,

    /// Required Authentik group name per real caller (Decision 3) --
    /// gates which /private/* routes each caller's verified token may
    /// reach. Not secret (a group name isn't confidential). Suggested
    /// defaults only -- an operator's actual Authentik group names are
    /// not mandated by this design.
    #[arg(long, env, default_value = "svc-poller-incidents")]
    pub internal_oauth_group_incidents: String,
    #[arg(long, env, default_value = "svc-poller-stations")]
    pub internal_oauth_group_stations: String,
    #[arg(long, env, default_value = "svc-poller-tocs")]
    pub internal_oauth_group_tocs: String,
    #[arg(long, env, default_value = "svc-poller-ldbws")]
    pub internal_oauth_group_ldbws: String,
    #[arg(long, env, default_value = "svc-poller-tfl")]
    pub internal_oauth_group_tfl: String,
    #[arg(long, env, default_value = "svc-trust-consumer")]
    pub internal_oauth_group_trust_consumer: String,
    #[arg(long, env, default_value = "svc-schedule-ingest")]
    pub internal_oauth_group_schedule_ingest: String,
    #[arg(long, env, default_value = "svc-schedule-reference")]
    pub internal_oauth_group_schedule_reference: String,
    /// Gates every route the (now real, merged) `full-coverage-consumer`
    /// producer surface uses: `POST`/`GET /private/station-full-coverage-samples`
    /// (per-station, `docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`
    /// Open Question #4) and `POST`/`GET /private/full-coverage-stats`
    /// (per-line, this plan's own Task 6) -- one producer service, one
    /// credential, both endpoints it writes to. See
    /// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
    /// Decision 5.
    #[arg(long, env, default_value = "svc-full-coverage-consumer")]
    pub internal_oauth_group_full_coverage: String,
    /// OIDC issuer base URL (e.g. `https://sso.example.com/realms/rail`).
    /// `crates/api` discovers every other endpoint (authorization, token,
    /// JWKS) from this single URL's `.well-known/openid-configuration`
    /// document -- see the design doc's OIDC-over-SAML research for why.
    /// No default: every deployment must point this at its own
    /// operator-run/subscribed SSO server. Discovery itself is lazy (see
    /// this plan's Global Constraints) -- this field is only syntactically
    /// validated at startup, not dereferenced over the network.
    #[arg(long, env)]
    pub sso_issuer_url: String,

    /// OIDC client id this app is registered as with the issuer above.
    #[arg(long, env)]
    pub sso_client_id: String,

    /// OIDC client secret paired with `sso_client_id`. A genuinely new
    /// *kind* of secret for this crate -- every other credential this
    /// crate's own config used to hold (the removed shared-secret field
    /// this design retired; the RDM API keys living in sibling pollers'
    /// own configs) is a single shared/bearer token, not a paired OAuth2
    /// confidential-client secret -- but handled with the same posture:
    /// env-only, required, never
    /// logged. `ServiceArguments` derives `Debug`; avoid ever logging
    /// `app.config` wholesale (nothing in this codebase does today) --
    /// log individual non-secret fields instead if a future debug log
    /// needs to reference config.
    #[arg(long, env)]
    pub sso_client_secret: String,

    /// The exact redirect URI registered with the SSO server for the
    /// authorization-code callback. Deliberately NOT this service's own
    /// origin -- it must be the *frontend's* public origin plus
    /// `/api/auth/callback` (e.g.
    /// `https://rail.example.com/api/auth/callback`), proxied through to
    /// this crate's `/public/auth/callback` by
    /// `frontend/app/api/[...path]/route.ts` (Task 8). If this pointed at
    /// `crates/api`'s own origin instead, the `Set-Cookie` the callback
    /// handler issues would be scoped to `api`'s origin, not the origin
    /// the browser subsequently talks to for every other request -- the
    /// session cookie would never come back. See the design doc's Session
    /// architecture section.
    #[arg(long, env)]
    pub sso_redirect_url: String,

    /// Where `/auth/callback` and `/auth/logout` send the browser once
    /// they're done, WHEN no per-attempt return path was captured or the
    /// one captured failed validation -- the frontend's own root URL (e.g.
    /// `https://rail.example.com/`). No longer the sole destination for
    /// every successful login: see routes::auth::callback and
    /// auth::validate_return_to for the per-login-attempt `return_to`
    /// this now falls back from. docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md.
    #[arg(long, env)]
    pub sso_post_login_redirect_url: String,

    /// Session lifetime in days: a FIXED expiry stamped once at sign-in,
    /// not a sliding window. `sessions.expires_at` is set to
    /// `NOW() + this` by `data::users::insert_session` and never touched
    /// again -- no code path anywhere extends it on activity, so a session
    /// dies exactly this many days after login however heavily it was
    /// used. (The design doc's "Expiry and refresh" section describes a
    /// sliding window instead; that is unimplemented, and this doc comment
    /// used to claim it. If you implement it, the write goes in
    /// `auth::AuthenticatedUser::from_request_parts`, the one place every
    /// authenticated request resolves its session.) Design doc proposes 14
    /// as a starting figure, not researched further there; kept
    /// configurable since it's a product/ops tuning knob, not a protocol
    /// constant.
    #[arg(long, env, default_value_t = 14)]
    pub session_ttl_days: i64,

    /// How many days of `line_status_history` rows the aggregator actually
    /// keeps before `queries::prune_history` (`crates/aggregator`) deletes
    /// them. This crate never reads or prunes that table itself -- the
    /// only reason this field exists here is so `/public/history-retention`
    /// (`routes/history_retention.rs`) can hand the frontend's history
    /// range picker the real ceiling, instead of the frontend guessing or
    /// hardcoding a number that could silently drift from what's actually
    /// configured. Deployments MUST set this to the same value they give
    /// the aggregator's own `HISTORY_RETENTION_DAYS` -- `docker-compose.yml`
    /// and the Helm chart (`values.yaml`'s `aggregator.historyRetentionDays`)
    /// both source both services' env vars from the one value, but nothing
    /// in this crate enforces the two staying in sync beyond that
    /// convention.
    #[arg(long, env, default_value_t = 7)]
    pub history_retention_days: i64,

    /// How many days of `line_status_daily_stats` rows the aggregator
    /// actually keeps before `queries::prune_daily_stats` deletes them.
    /// This crate never reads or prunes that table itself -- the only
    /// reason this field exists here is so `/public/history-retention`
    /// (`routes/history_retention.rs`) can hand the frontend's Trends-tab
    /// granularity control the real ceiling. Deployments MUST set this to
    /// the same value they give the aggregator's own
    /// `DAILY_STATS_RETENTION_DAYS` -- same convention as
    /// `history_retention_days`, above.
    #[arg(long, env, default_value_t = 300)]
    pub daily_stats_retention_days: i64,

    /// How many hours of `line_status_half_hourly_stats` rows the
    /// aggregator actually keeps before `queries::prune_half_hourly_stats`
    /// deletes them. Same "static config echo, never enforced here"
    /// posture as `history_retention_days`/`daily_stats_retention_days`
    /// above. Deployments MUST set this to the same value they give the
    /// aggregator's own `HALF_HOURLY_STATS_RETENTION_HOURS`.
    #[arg(long, env, default_value_t = 840)]
    pub half_hourly_stats_retention_hours: i64,

    /// Whether to expose the `/metrics` route and its request-metrics
    /// middleware. Unlike the other 7 binaries, `api`'s own HTTP listener
    /// stays up regardless (it's the main service) -- this only controls
    /// whether `/metrics` is registered and whether requests are counted.
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
    #[arg(long, value_parser = parse_toml_path::<Defaults>, value_hint = ValueHint::FilePath, value_name = "FILE")]
    pub defaults_file: Option<Defaults>,
    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Defaults to `/app/lines` (baked into the Docker image — see
    /// `docker/api.Dockerfile`), overridable via `LINES_DIR` for local
    /// (non-Docker) runs.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: LineCatalogue,

    /// The VAPID public key `crates/notifier` signs push messages with —
    /// handed to the browser's `PushManager.subscribe({ applicationServerKey })`
    /// call unchanged. The matching PRIVATE key lives only in `crates/notifier`'s
    /// own config -- `api` never needs it, since `api` only stores
    /// subscriptions, it never sends to them.
    #[arg(long, env)]
    pub vapid_public_key: String,

    /// Global override for `LineDefinition.full_coverage_enabled`
    /// (Decision 3's per-line TOML rollout gate, `crates/common/src/lib.rs`).
    /// When `true`, `data::station_stats::full_coverage_enabled_for`
    /// treats EVERY catalogued line as full-coverage-enabled, regardless
    /// of what its own `lines/*.toml` entry sets -- the SAME global flag
    /// name/semantics as `crates/aggregator`'s own
    /// `full_coverage_enabled_default` (both services independently gate
    /// on `LineDefinition.full_coverage_enabled`, so both need the
    /// override). Default `false` is deliberate: this flag must never
    /// silently change behavior for a deployment that doesn't explicitly
    /// set it, and `true` is never baked in here as the default (that
    /// would require a rebuild to ever revert) -- an operator opts in via
    /// this env var / the Helm chart's `api.fullCoverageEnabledDefault`
    /// value.
    #[arg(long, env, default_value_t = false)]
    pub full_coverage_enabled_default: bool,
}
