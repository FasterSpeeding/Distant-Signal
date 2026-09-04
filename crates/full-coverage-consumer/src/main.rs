//! `full-coverage-consumer`: a second, independent Kafka consumer against
//! the same RDM Train Movements feed `trust-consumer` reads, correlating
//! every event against the FULL scheduled population of every
//! shadow-computed line (not a small pinned-train set) -- see
//! docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md and
//! docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md.
//! SHADOW MODE ONLY: writes real per-line/per-station stats, but nothing
//! reads them into a real line's severity/DataQuality while
//! `LineDefinition.full_coverage_enabled` stays false everywhere (see the
//! design doc's binding condition).

mod config;
mod correlate;
mod health;
mod population;
mod queries;
mod stanox_tiploc;
// mod station_correlate; -- Task 12
// mod stats;            -- Task 11

use clap::Parser;
use config::Config;

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
    let _connection_state = health::spawn(config.health_bind_url.clone());
    tracing::info!(
        "full-coverage-consumer scaffolding booted; no correlation logic yet (Task 8 of the implementation plan)"
    );
    // Task 13 replaces this with the real loop.
    std::future::pending::<()>().await;
    Ok(())
}
