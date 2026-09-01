//! `schedule-ingest`: watches a locally mounted directory for a pushed CIF
//! SCHEDULE feed delivery from Network Rail/RDG, verifies completeness
//! against the delivery's own manifest, and forwards completed sequences to
//! the `api` crate's ingestion endpoint.
//!
//! See `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` for
//! the full design and `docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md`
//! for the implementation plan this crate is built against. This service
//! never dials out itself — a sibling SFTPGo container receives the push
//! and writes into `watch_dir`; this crate only reads what lands there (see
//! `config.rs`).

mod config;

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

    // The real check-time scheduling loop (manifest parsing, stability
    // tracking, and ingest-on-completion) is implemented in a later task —
    // see Task 5 of the implementation plan referenced in this module's
    // doc comment. This scaffold only needs to compile and match this
    // repo's other `main.rs` shapes.
    tracing::info!(watch_dir = ?config.watch_dir, "schedule-ingest scaffolded; scan loop not yet implemented");
    todo!("Task 5: implement the check-time scheduling loop")
}
