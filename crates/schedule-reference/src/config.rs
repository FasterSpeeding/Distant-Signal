use std::path::PathBuf;

use clap::Parser;

/// CLI/env configuration for the `schedule-reference` service.
///
/// Mounts the same PVC `schedule-ingest` writes to, READ-ONLY -- see
/// docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md's
/// Decision 1(c). Never writes to `storage_dir`.
#[derive(Debug, Parser)]
pub struct Config {
    /// Root of the shared PVC -- same path `schedule-ingest`'s own
    /// `--storage-dir` writes into (`crates/schedule-ingest/src/config.rs`),
    /// mounted read-only in this container.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// How often to check `storage_dir` for a new complete sequence.
    /// Independent of the underlying daily delivery cadence -- see
    /// Decision 4: most checks find nothing new, since a fresh sequence
    /// only lands roughly once a day, but reading an already-local
    /// directory listing is cheap.
    #[arg(long, env, default_value_t = 1800)]
    pub poll_interval_secs: u64,

    /// The `api` crate's ingestion endpoint for resolved STANOX/CRS rows.
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub api_ingest_url: String,

    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 8 real callers) -- see
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    /// This service's own Authentik service-account credential --
    /// per-service, distinct from every other caller's. `username` is
    /// identifying, not itself the secret; `password` (an Authentik
    /// app-password) is the actual secret.
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    /// Port for this service's Prometheus `/metrics` endpoint. MUST differ
    /// from the `ingest` sibling container's own metrics port -- both
    /// containers share one Pod network namespace (see this plan's Global
    /// Constraints).
    #[arg(long, env, default_value_t = 9092)]
    pub metrics_port: u16,

    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
