//! `schedule-reference`: a sibling container in the `schedulefeed` Pod.
//! Once `schedule-ingest` has moved a verified-complete delivery into
//! `storage_dir/<n>/`, reads that sequence's `RJTTF<n>MCA.txt` (`TI`
//! records) and `RJTTF<n>MSN.txt` (`A` records) directly off the
//! already-local, read-only-mounted PVC, resolves a STANOX->CRS table, and
//! POSTs it to `api`'s `/private/stanox-crs`. See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md.

mod config;
mod parser;
mod sequence;

use std::time::Duration;

use clap::Parser;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut last_processed_sequence: Option<u32> = None;

    loop {
        interval.tick().await;
        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &mut last_processed_sequence).await;
        metrics::histogram!(common::metrics::metric_name("schedule_reference_cycle_duration_seconds"))
            .record(cycle_start.elapsed().as_secs_f64());
        if let Err(err) = result {
            tracing::error!(error = ?err, "schedule-reference cycle failed; will retry next interval");
        }
    }
}

/// Filled in by this plan's Task 4, once `crates/schedule-reference/src/parser.rs`
/// exists: scan for the highest complete sequence, skip if unchanged since
/// `last_processed_sequence`, else read+parse+POST and update it only on
/// a successful POST.
async fn poll_once(
    _client: &Client,
    config: &Config,
    last_processed_sequence: &mut Option<u32>,
) -> anyhow::Result<()> {
    let Some(sequence) = sequence::highest_complete_sequence(&config.storage_dir)? else {
        tracing::debug!("no complete MCA+MSN sequence directory found yet");
        return Ok(());
    };
    if Some(sequence) == *last_processed_sequence {
        tracing::debug!(sequence, "no new sequence since last successful parse");
        return Ok(());
    }
    tracing::info!(sequence, "TODO(Task 4): parse and POST this sequence");
    Ok(())
}
