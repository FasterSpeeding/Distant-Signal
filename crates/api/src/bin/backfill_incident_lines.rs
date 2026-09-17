//! `backfill_incident_lines`: fills `incidents.affected_lines` for rows that
//! were ingested before the column existed (or before a `lines/*.toml`
//! change), so the incident archive's Line filter can find them.
//!
//! ```text
//!   DATABASE_URL=postgres://... LINES_DIR=./lines \
//!     cargo run -p api --bin backfill_incident_lines
//! ```
//!
//! Idempotent and safe to re-run: it writes only `affected_lines`, only for
//! rows whose recomputed value differs from what is stored, so a second run
//! reports zero updates. Exits non-zero only on a real database or
//! catalogue-loading error.
//!
//! All the logic and the reasoning for why this is a binary rather than a
//! migration live in `api::data::incident_line_backfill`'s module doc; the
//! operational runbook is `docs/incident-affected-lines-backfill.md`. This
//! file is deliberately nothing but wiring.

use common::matcher::LineMatcher;
use sqlx::postgres::PgPoolOptions;

/// Same default as `api`'s own `--lines-dir` (`/app/lines`, baked into the
/// Docker image), so running this inside the `api` container needs no
/// arguments at all.
const DEFAULT_LINES_DIR: &str = "/app/lines";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let lines_dir = std::env::var("LINES_DIR").unwrap_or_else(|_| DEFAULT_LINES_DIR.to_string());

    let lines = common::config::parse_lines(&lines_dir)?;
    // A missing/empty catalogue directory parses successfully as zero lines
    // (see `common::config::parse_lines`'s own test), which would quietly
    // blank every row's `affected_lines` instead of filling it. Refuse.
    anyhow::ensure!(
        !lines.is_empty(),
        "no line definitions found in {lines_dir} -- refusing to run, since an empty catalogue \
         would clear affected_lines on every row. Set LINES_DIR to the repository's lines/ \
         directory."
    );
    tracing::info!(count = lines.len(), lines_dir, "loaded line catalogue");

    let matcher = LineMatcher::new(&lines);
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let report = api::data::incident_line_backfill::run_backfill(&pool, &matcher).await?;

    tracing::info!(
        rows_examined = report.rows_examined,
        rows_updated = report.rows_updated,
        rows_matching_no_line = report.rows_matching_no_line,
        "incident affected_lines backfill complete"
    );
    println!(
        "backfill complete:\n  \
         incidents examined:            {}\n  \
         incidents updated:             {}\n  \
         incidents matching no line:    {}",
        report.rows_examined, report.rows_updated, report.rows_matching_no_line,
    );
    if report.rows_matching_no_line > 0 {
        println!(
            "\nAn incident matching no catalogue line is expected, not an error: the feed \
             carries incidents for operators and routes this catalogue has no lines/*.toml \
             entry for. Those rows are reachable through the archive's Operator filter."
        );
    }
    Ok(())
}
