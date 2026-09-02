use std::path::PathBuf;

use clap::Parser;

/// CLI/env configuration for the `schedule-ingest` service.
///
/// Unlike the now-superseded pull design's equivalent `Config`, this crate
/// makes no outbound SFTP connection at all — it only scans a local mounted
/// directory that the sibling `schedule-sftp` (SFTPGo) container writes
/// into. See
/// docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md.
#[derive(Debug, Parser)]
pub struct Config {
    /// Where the SFTP daemon writes incoming files. Scanned each check time
    /// via `std::fs::read_dir` — see `src/scan.rs`.
    #[arg(long, env, default_value = "/data/schedule-feed/incoming")]
    pub watch_dir: PathBuf,

    /// Root of the shared PVC. Verified-complete sequences move to
    /// `storage_dir/<nnn>/`; retention pruning operates on this directory's
    /// immediate numeric subdirectories.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// Comma-separated HH:MM times, Europe/London — reused directly from
    /// the (now-superseded) pull design's Scheduling section: the window
    /// describes when DTD *produces* the feed, not which party connects.
    #[arg(
        long,
        env,
        default_value = "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
    )]
    pub check_times: String,

    /// How many complete sequences to retain on disk (current + fallback).
    #[arg(long, env, default_value_t = 2)]
    pub retention_keep_sequences: u32,

    /// How many consecutive polling cycles a manifest-listed file's mtime
    /// and size must be unchanged before it's treated as a completeness
    /// candidate — see Task 3. RSPS5046's manifest carries no per-file size
    /// field (confirmed directly against the real sample in this plan's
    /// own research), so this stability check is the only completeness
    /// signal available, not a fallback.
    #[arg(long, env, default_value_t = 2)]
    pub stability_cycles: u32,

    /// The `api` crate's ingestion endpoint for completed schedule feed
    /// sequences.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-feed-ingests"
    )]
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

    /// Port for this service's Prometheus `/metrics` endpoint. See
    /// docs/superpowers/plans/2026-08-29-metrics.md's Global Constraints
    /// for why this differs from api.service.port -- api reuses its
    /// existing HTTP listener, this service has none, so it needs a new one.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    /// Whether to start this service's Prometheus `/metrics` listener at
    /// all. Distinct from `metrics_port` (which port to use IF started) --
    /// this is what actually satisfies "metrics.enabled=false leaves the
    /// service working exactly as it does today" (see the Helm chart's
    /// `metrics.enabled` value and prior branches' final whole-branch
    /// review): omitting the containerPort/env/annotations in the chart
    /// alone does not stop the process from listening, since Kubernetes
    /// container ports are purely declarative.
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
