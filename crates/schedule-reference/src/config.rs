use std::path::PathBuf;

use clap::Parser;
use common::config::{LineCatalogue, parse_lines};

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

    /// How often to check `storage_dir` for a new complete delivery.
    /// Independent of the underlying daily delivery cadence -- see
    /// Decision 4: most checks find nothing new, since a fresh delivery
    /// only lands roughly once a day, but reading an already-local
    /// directory listing is cheap.
    #[arg(long, env, default_value_t = 1800)]
    pub poll_interval_secs: u64,

    /// The `api` crate's ingestion endpoint for resolved STANOX/CRS rows.
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub api_ingest_url: String,

    /// The `api` crate's ingestion endpoint for this service's second
    /// responsibility (Task 7): per-line CIF SCHEDULE population publish.
    /// See docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
    /// Decision 2a/2b.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-line-population"
    )]
    pub schedule_line_population_url: String,

    /// The `api` crate's ingestion endpoint for this service's third
    /// responsibility: the whole-network trip-search fallback's per-CRS,
    /// CIF-derived "next 10 scheduled departures" publish. See
    /// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
    /// Decision 1. POST-only, no GET pair -- see that decision's own note on
    /// why this differs from `schedule_line_population_url`'s route shape.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-network-departures"
    )]
    pub schedule_network_departures_url: String,

    /// The static line catalogue -- same `--lines-dir`/`LINES_DIR`
    /// value_parser pattern as `crates/aggregator/src/config.rs`'s own
    /// field of the same name. Used to build the per-line TIPLOC set this
    /// service's own `schedules_touching` query needs (Task 7) -- a
    /// responsibility this crate did not have before Task 7.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,

    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 9 real callers).
    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

    /// Port for this service's Prometheus `/metrics` endpoint. MUST differ
    /// from the `ingest` sibling container's own metrics port -- both
    /// containers share one Pod network namespace (see this plan's Global
    /// Constraints).
    #[arg(long, env, default_value_t = 9092)]
    pub metrics_port: u16,

    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,
}
