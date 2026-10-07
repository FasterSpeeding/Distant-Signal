//! `backfill_line_train_summaries`: derives `line_train_summaries` rows
//! for every stored line population that has no current rows -- the
//! populations published before the table existed, or derived against an
//! older catalogue.
//!
//! ```text
//!   DATABASE_URL=postgres://... LINES_DIR=./lines \
//!     cargo run -p api --bin backfill_line_train_summaries [-- --force]
//! ```
//!
//! Optional: without it the line page and timetable read the population
//! JSONB (slower, same answer) until `schedule-reference`'s next publish
//! rewrites the rows. Idempotent: rows already current are skipped (all of
//! them with `--force`). One `(line, date)` at a time, each in its own
//! transaction, serialised with a concurrent publish of the same row.
//! Uses the same catalogue as `api` (`LINES_DIR`), which decides what
//! "current" means -- run it with the deployed image's `/app/lines`.

#![expect(
    clippy::print_stdout,
    reason = "one-off CLI tool: stdout is its output"
)]

use sqlx::postgres::PgPoolOptions;

/// Same default as `api`'s own `--lines-dir`.
const DEFAULT_LINES_DIR: &str = "/app/lines";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    common::logging::init_with_filter(
        "backfill-line-train-summaries",
        common::logging::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| common::logging::EnvFilter::new("info")),
    );
    let force = std::env::args().skip(1).any(|a| a == "--force");
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let lines_dir = std::env::var("LINES_DIR").unwrap_or_else(|_| DEFAULT_LINES_DIR.to_string());
    let lines = common::config::parse_lines(&lines_dir)?;
    anyhow::ensure!(
        !lines.is_empty(),
        "no line definitions found in {lines_dir}: rows derived without the catalogue would \
         carry no on-line stops. Set LINES_DIR to the repository's lines/ directory."
    );
    println!("loaded {} line definitions from {lines_dir}", lines.len());

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;
    let keys: Vec<(String, chrono::NaiveDate)> = sqlx::query_as(
        "SELECT line_id, service_date FROM schedule_line_population ORDER BY service_date, line_id",
    )
    .fetch_all(&pool)
    .await?;

    let (mut written, mut current, mut rows) = (0_usize, 0_usize, 0_usize);
    for (line_id, service_date) in &keys {
        let line = lines.iter().find(|l| &l.id == line_id);
        let started = std::time::Instant::now();
        match api::data::line_train_summaries::rebuild_summaries(
            &pool,
            line,
            line_id,
            *service_date,
            force,
        )
        .await?
        {
            // Pruned since the key list was read.
            None => {}
            Some(None) => current += 1,
            Some(Some(n)) => {
                written += 1;
                rows += n;
                println!(
                    "{line_id} {service_date}: {n} rows in {} ms",
                    started.elapsed().as_millis()
                );
            }
        }
    }
    println!(
        "backfill complete: {} populations, {written} rewritten ({rows} rows), {current} already current",
        keys.len()
    );
    Ok(())
}
