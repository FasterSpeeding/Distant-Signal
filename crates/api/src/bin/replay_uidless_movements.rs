//! `replay_uidless_movements`: writes `train_movement_events` /
//! `train_current_state` for the uid-less TRUST rows a trust-backlog-consumer
//! restart left out of them, now that api infers a uid-less event's train
//! from its Activation in `trust_event_backlog` (2026-10-01 outage review).
//!
//! ```text
//!   DATABASE_URL=postgres://... \
//!     cargo run -p api --bin replay_uidless_movements -- 2026-10-01T00:00:00Z
//! ```
//!
//! The argument is the earliest `received_at` to replay (default: 24 hours
//! ago, the backlog's retention). Idempotent: every shared write is keyed by
//! the event's dedup key and guarded by event time, so re-running it
//! changes nothing. Logic:
//! `api::data::trust_event_backlog::replay_uidless_backlog`.

#![expect(
    clippy::print_stdout,
    reason = "one-off CLI tool: stdout is its output"
)]

use sqlx::postgres::PgPoolOptions;

/// Rows per `ingest_shared_movements_batch` call (about the size of the
/// consumer's own POSTs).
const CHUNK: i64 = 500;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

async fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    common::logging::init_with_filter(
        "replay-uidless-movements",
        common::logging::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| common::logging::EnvFilter::new("info")),
    );
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let since = match std::env::args().nth(1) {
        Some(raw) => raw
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|err| anyhow::anyhow!("{raw:?} is not an RFC 3339 timestamp: {err}"))?,
        None => chrono::Utc::now() - chrono::Duration::hours(24),
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    let report =
        api::data::trust_event_backlog::replay_uidless_backlog(&pool, since, CHUNK).await?;
    println!(
        "replayed {} uid-less backlog rows received since {since} ({} failed; re-run to retry them)",
        report.rows, report.failed
    );
    anyhow::ensure!(report.failed == 0, "{} rows failed", report.failed);
    Ok(())
}
