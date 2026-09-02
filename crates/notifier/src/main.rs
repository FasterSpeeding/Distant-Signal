//! `notifier`: polls line_status_history/train_movement_events by
//! watermark and sends Web Push notifications for real severity/status
//! transitions on a user's pinned lines/tracked trains. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md.

mod config;
mod decision;
mod queries;
mod send;

use std::time::Duration;

use clap::Parser;
use config::Config;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    let config = Config::parse();

    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level)).init();

    let pool = PgPoolOptions::new().max_connections(5).connect(&config.database_url).await?;

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    loop {
        interval.tick().await;
        tracing::debug!("notifier cycle placeholder -- queries.rs and send.rs wired in Tasks 4/6");
        let _ = &pool; // silences unused-var warning until Task 4 uses it
    }
}
