//! `trust-consumer`: persistent Kafka consumer for Network Rail's TRUST
//! Train Movements feed (via RDM), filtered to exactly the currently
//! user-tracked `(train_uid, date)` set. NOT a cron-style poller -- see
//! docs/superpowers/plans/2026-08-28-train-tracking.md's Global
//! Constraints for why this crate isn't named `poller-trust`.

mod config;
mod feed;
mod health;
mod schema;
mod matching;
mod journey;
mod eta;
mod dedup;
mod process;
mod queries;

use std::time::Duration;

use clap::Parser;
use config::Config;
use feed::MovementFeed;
use feed::kafka::KafkaMovementFeed;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();

    let mut feed = KafkaMovementFeed::connect(&config, connection_state)?;

    let mut reference = process::Reference { pending: Vec::new() };
    let reload_interval = Duration::from_secs(config.reference_reload_secs);
    let mut last_reference_reload = tokio::time::Instant::now() - reload_interval;

    // Owned here, for the whole life of the process: TRUST spreads one
    // train's Activation, origin departure, later movements and any
    // cancellation across many batches, so this state must survive every
    // `run_once` call, not be rebuilt per cycle. See
    // `process::ProcessorState`'s docs.
    let mut state = process::ProcessorState::default();

    loop {
        if last_reference_reload.elapsed() >= reload_interval {
            match queries::fetch_active_tracked_trains(
                &http,
                &config.api_tracked_trains_url,
                &config.internal_token,
            )
            .await
            {
                Ok(refs) => {
                    reference.pending = refs
                        .into_iter()
                        .filter(|r| r.resolution_status == "pending")
                        .map(|r| crate::matching::PendingPin {
                            tracked_train_id: r.id,
                            pin_origin_crs: r.pin_origin_crs,
                            pin_scheduled_departure: r.pin_scheduled_departure,
                        })
                        .collect();
                    last_reference_reload = tokio::time::Instant::now();
                }
                Err(err) => {
                    tracing::error!(error = ?err, "failed to reload active tracked trains; retrying next cycle");
                }
            }
        }

        match process::run_once(&mut feed, &reference, &mut state).await {
            Ok(events) => {
                if let Err(err) = queries::post_train_events(
                    &http,
                    &config.api_ingest_url,
                    &config.internal_token,
                    &events,
                )
                .await
                {
                    tracing::error!(error = ?err, "failed to post train events; not committing this batch's offsets");
                    continue; // do NOT commit -- at-least-once redelivery will retry, dedup_key makes it safe
                }
                if let Err(err) = feed.commit().await {
                    tracing::error!(error = ?err, "failed to commit Kafka offsets");
                }
            }
            Err(err) => {
                tracing::error!(error = ?err, "error processing movement feed batch");
            }
        }
    }
}
