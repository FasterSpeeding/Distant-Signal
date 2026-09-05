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

    /// Root of the shared PVC. Each verified-stable delivery is extracted
    /// into `storage_dir/<timestamp>/` (a compact sortable UTC rendering of
    /// the delivery zip's own mtime -- see `delivery::delivery_dir_name`);
    /// retention pruning operates on this directory's immediate
    /// timestamp-shaped subdirectories.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// Comma-separated HH:MM times, Europe/London — reused directly from
    /// the (now-superseded) pull design's Scheduling section: the window
    /// describes when DTD *produces* the feed, not which party connects.
    ///
    /// No longer controls *when* `watch_dir` gets scanned (see
    /// `poll_interval_secs` for that) — a real delivery once landed outside
    /// every configured slot (mid-afternoon, at DTD SFTP account
    /// provisioning time) and sat unprocessed for hours because the old
    /// design only scanned at these ~9 daily slots. What's left of this
    /// field's job: its *last configured entry* still marks "today's final
    /// realistic chance the production window described by RSPS5046 has to
    /// deliver", used only to decide whether a still-incomplete delivery
    /// logs at `error` vs `info` severity — see `main`'s
    /// `is_final_check_of_day`.
    #[arg(
        long,
        env,
        default_value = "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
    )]
    pub check_times: String,

    /// How often to scan `watch_dir`, in seconds. `scan_incoming` (see
    /// `scan.rs`) is a cheap local `std::fs::read_dir` + per-file `stat` —
    /// no network call, no external rate limit to respect — so there is no
    /// real cost concern scanning far more often than the old design's
    /// sparse `check_times` slots did. This is what actually fixes the
    /// stuck-delivery bug described on `check_times`: every delivery,
    /// whenever it lands, is picked up within roughly one interval instead
    /// of potentially waiting until the next of ~9 daily slots (which could
    /// be many hours away).
    #[arg(long, env, default_value_t = 120)]
    pub poll_interval_secs: u64,

    /// How many complete deliveries to retain on disk (current + fallback).
    /// Renamed from the old `retention_keep_sequences` -- there is no
    /// sequence number any more, just delivery timestamps (see
    /// `main::prune_old_deliveries`). No history/retention requirement
    /// beyond this exists today -- a future "also copy elsewhere for
    /// long-term retention" need should be a separate, purposefully-called
    /// copy step, not a change to this simple keep-N-most-recent behavior.
    #[arg(long, env, default_value_t = 2)]
    pub retention_keep_deliveries: u32,

    /// How many consecutive polling cycles the delivery zip's mtime and
    /// size must be unchanged before it's treated as stable/complete —
    /// see `scan.rs`. There is no manifest-declared size any more (there is
    /// no manifest at all), so this stability check remains the only
    /// completeness signal available, not a fallback.
    ///
    /// Raised from this crate's original default of `2` alongside
    /// shrinking the scan cadence to `poll_interval_secs` (120s default).
    /// At the old sparse cadence (30 minutes apart during the busiest part
    /// of the overnight window, and up to ~14.5 hours apart between the
    /// 01:30 and 16:00 slots), "2 consecutive stable polls" was already a
    /// strong — if wildly inconsistent — time-based signal. At a 2-minute
    /// cadence, 2 consecutive stable polls is only 4 minutes, which a
    /// brief mid-transfer pause could satisfy by accident. `5` at the new
    /// default interval gives a 10-minute unchanged-on-disk window, which
    /// comfortably covers a transient pause without reintroducing anywhere
    /// near the old design's multi-hour worst-case detection latency.
    #[arg(long, env, default_value_t = 5)]
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
    /// across all 9 real callers).
    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

    /// Port for this service's Prometheus `/metrics` endpoint. Stays a
    /// plain field, not part of `MetricsArgs` -- its default differs per
    /// crate and `docker-compose.yml` relies on the code default.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,
}
