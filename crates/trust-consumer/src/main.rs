//! `trust-consumer`: persistent Kafka consumer for Network Rail's TRUST
//! Train Movements feed (via RDM), filtered to exactly the currently
//! user-tracked `(train_uid, date)` set. NOT a cron-style poller -- see
//! docs/superpowers/plans/2026-08-28-train-tracking.md's Global
//! Constraints for why this crate isn't named `poller-trust`.

mod config;
mod feed;
mod health;
mod schema;

use clap::Parser;
use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let _connection_state = health::spawn(config.health_bind_url.clone());

    tracing::info!("trust-consumer scaffold up; Kafka consumer loop lands in later tasks");

    // Placeholder -- Task 14 replaces this with the real consume loop.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        tracing::info!("trust-consumer heartbeat");
    }
}
