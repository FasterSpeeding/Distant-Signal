//! `backfill_trains`: the operational, re-runnable backfill that MUST be
//! run against a database with pre-existing `tracked_trains` data BEFORE it
//! is upgraded to a build containing
//! `crates/ds-store/migrations/20260906140000_drop_legacy_columns.sql`.
//!
//! ```text
//!   DATABASE_URL=postgres://... cargo run -p api --bin backfill_trains
//! ```
//!
//! Idempotent and safe to run repeatedly, at any point in the
//! expand/contract sequence, including against an already-contracted
//! database (where it reports zero work and exits 0). Exits non-zero only
//! on a real database error.
//!
//! All of the logic, the full deploy sequence, and the reasoning for why
//! this is a binary rather than a migration live in
//! `crates/api/src/data/legacy_backfill.rs`'s module doc. This file is
//! deliberately nothing but argument-free wiring, so there is exactly one
//! copy of the SQL and `api`'s own startup guard
//! (`ensure_ready_for_contract_migration`) is checking the very same
//! conditions this binary clears.

#![expect(
    clippy::print_stdout,
    reason = "one-off CLI tool: stdout is its output"
)]

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    common::logging::init_with_filter(
        "backfill-trains",
        common::logging::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| common::logging::EnvFilter::new("info")),
    );

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let report = api::data::legacy_backfill::run_backfill(&pool).await?;

    tracing::info!(
        subscriptions_linked = report.subscriptions_linked,
        movement_events_linked = report.movement_events_linked,
        current_state_linked = report.current_state_linked,
        "backfill complete"
    );
    println!(
        "backfill complete:\n  \
         subscriptions linked to a trains row: {}\n  \
         train_movement_events linked:         {}\n  \
         train_current_state linked:           {}\n  \
         remaining gaps (accepted, no legacy identity to recover): {}",
        report.subscriptions_linked,
        report.movement_events_linked,
        report.current_state_linked,
        report.accepted_gaps,
    );
    if report.accepted_gaps > 0 {
        println!(
            "\nThose remaining rows carry no legacy identity at all (a subscription that never \
             resolved, or a movement row beneath one) -- there is nothing for the drop \
             migration to lose. It is safe to proceed."
        );
    }
    println!(
        "\nIt is now safe to deploy a build including \
         migrations/20260906140000_drop_legacy_columns.sql."
    );
    Ok(())
}
