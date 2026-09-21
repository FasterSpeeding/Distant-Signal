//! `compare_full_coverage`: reports whether `full-coverage-consumer`'s
//! TRUST-vs-schedule output agrees with LDBWS-sample-derived output for one
//! line over a time window -- built to validate the `tfw-conwy-valley`
//! full-coverage pilot (see that file's own "Full-coverage pilot" comment)
//! before any second line is ever opted in.
//!
//! ```text
//!   DATABASE_URL=postgres://... LINES_DIR=./lines \
//!     cargo run -p api --bin compare_full_coverage -- --line-id tfw-conwy-valley --days 30
//! ```
//!
//! Read-only: this never writes to any table. Exits non-zero only on a
//! real database or catalogue-loading error -- a "no data yet" or
//! "healthy" or "concerning" result all still exit 0, since those are
//! informational findings, not tool failures. Redirect its output
//! (`> report.txt`) to keep a dated record while comparing across pilot
//! weeks.
//!
//! All the comparison logic, the reasoning for rate-based (not raw-count)
//! comparison, and the exact meaning of "concerning" for each of the two
//! heuristics this task was asked to check live in
//! `api::data::full_coverage_comparison`'s module doc -- read that first.
//! This file is deliberately nothing but argument parsing and
//! human-readable formatting.

use chrono::{Duration, Utc};
use clap::Parser;
use sqlx::postgres::PgPoolOptions;

use api::data::full_coverage_comparison::{
    self, CancellationFinding, ComparisonReport, SkippedFinding,
};

#[derive(Debug, Parser)]
#[command(
    about = "Compares full-coverage-consumer output against LDBWS-sample-derived output \
                    for one line, over the same time window."
)]
struct Args {
    /// The catalogue line id to compare, e.g. `tfw-conwy-valley`.
    #[arg(long)]
    line_id: String,
    /// How many trailing days (ending today) to compare. Ignored if
    /// `--from` is also given.
    #[arg(long, default_value_t = 30)]
    days: i64,
    /// Explicit start date (`YYYY-MM-DD`), overriding `--days`.
    #[arg(long)]
    from: Option<chrono::NaiveDate>,
    /// Explicit end date (`YYYY-MM-DD`), defaulting to today.
    #[arg(long)]
    to: Option<chrono::NaiveDate>,
    /// Directory of `lines/*.toml` files, used only to read this line's
    /// own (possibly overridden) `reduced_service_pct` severity threshold
    /// -- the operationally meaningful cutoff `classify_cancellation` uses
    /// (see that function's own doc comment). Falls back to
    /// `common::Defaults`'s plain default (0.25) if the line or directory
    /// can't be found, rather than failing the whole comparison over a
    /// missing catalogue mount.
    #[arg(long, env = "LINES_DIR", default_value = "lines")]
    lines_dir: String,
    /// Absolute rate-difference threshold (percentage points, as a
    /// fraction) above which a gap is flagged even when neither side
    /// crosses the severity threshold. See
    /// `full_coverage_comparison::classify_cancellation`'s own doc comment.
    #[arg(long, default_value_t = 0.15)]
    large_gap_threshold: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let to = args.to.unwrap_or_else(|| Utc::now().date_naive());
    let from = args
        .from
        .unwrap_or_else(|| to - Duration::days(args.days.max(0)));

    let severity_threshold = severity_threshold_for(&args.line_id, &args.lines_dir);

    let report = full_coverage_comparison::compare_line(
        &pool,
        &args.line_id,
        from,
        to,
        severity_threshold,
        args.large_gap_threshold,
    )
    .await?;

    print_report(&report, severity_threshold);

    let pass_events =
        full_coverage_comparison::find_pass_events_for_line_best_effort(&pool, &args.line_id)
            .await?;
    print_pass_event_section(&pass_events);

    Ok(())
}

/// This line's own configured `reduced_service_pct` (a
/// `[severity_overrides]` entry in `lines/*.toml`, falling back to
/// `common::Defaults::default().reduced_service_pct` if the catalogue
/// can't be loaded or doesn't contain this line) -- see `Args::lines_dir`'s
/// own doc comment for why this matters to the comparison.
fn severity_threshold_for(line_id: &str, lines_dir: &str) -> f64 {
    let defaults = common::Defaults::default();
    match common::config::parse_lines(lines_dir) {
        Ok(lines) => match lines.iter().find(|l| l.id == line_id) {
            Some(line) => {
                common::thresholds_for(&defaults, &line.severity_overrides).reduced_service_pct
            }
            None => {
                tracing::warn!(
                    line_id,
                    lines_dir,
                    "line not found in catalogue; using the plain default reduced_service_pct"
                );
                defaults.reduced_service_pct
            }
        },
        Err(err) => {
            tracing::warn!(
                error = ?err,
                lines_dir,
                "could not load line catalogue; using the plain default reduced_service_pct"
            );
            defaults.reduced_service_pct
        }
    }
}

fn print_report(report: &ComparisonReport, severity_threshold: f64) {
    println!(
        "full-coverage vs. LDBWS-sample comparison for line {:?}, {} to {}",
        report.line_id, report.from, report.to
    );
    println!(
        "(this line's reduced_service_pct severity threshold: {:.0}%)\n",
        severity_threshold * 100.0
    );

    match &report.live_snapshot {
        Some(snapshot) => println!(
            "current live full_coverage_line_stats row: service_date={} availability={} \
             total={} delayed={} cancelled={} skipped={} avg_delay_minutes={:.1}\n",
            snapshot.service_date,
            snapshot.availability,
            snapshot.stats.total,
            snapshot.stats.delayed,
            snapshot.stats.cancelled,
            snapshot.stats.skipped,
            snapshot.stats.avg_delay_minutes
        ),
        None => println!(
            "current live full_coverage_line_stats row: none yet -- full-coverage-consumer has \
             not published anything for this line, or this line isn't in its shadow_lines scope\n"
        ),
    }

    if report.days.is_empty() {
        println!(
            "no line_status_daily_stats / line_status_daily_coverage_stats rows for this line \
             in this window -- nothing to compare yet. This is EXPECTED immediately after \
             flipping full_coverage_enabled for a new pilot line: line_status_daily_coverage_stats \
             only gets written once full-coverage-consumer's own row for this line flips \
             `available` (see aggregation::merge_full_coverage's gate) for at least one \
             aggregator cycle. Re-run this tool after the pilot line has run for a few days."
        );
        return;
    }

    let mut concerning = 0usize;
    let mut insufficient = 0usize;
    for day in &report.days {
        let (sample_str, coverage_str) = (
            day.sample
                .as_ref()
                .map(|r| {
                    format!(
                        "cancelled={:.1}% delayed={:.1}% skipped={:.1}% ({} cycles)",
                        r.cancelled_rate * 100.0,
                        r.delayed_rate * 100.0,
                        r.skipped_rate * 100.0,
                        r.cycles
                    )
                })
                .unwrap_or_else(|| "no LDBWS-sample data".to_string()),
            day.coverage
                .as_ref()
                .map(|r| {
                    format!(
                        "cancelled={:.1}% delayed={:.1}% skipped={:.1}% ({} resolved windows)",
                        r.cancelled_rate * 100.0,
                        r.delayed_rate * 100.0,
                        r.skipped_rate * 100.0,
                        r.cycles
                    )
                })
                .unwrap_or_else(|| "no full-coverage data".to_string()),
        );
        println!("{}:", day.day);
        println!("  LDBWS sample:   {sample_str}");
        println!("  full-coverage:  {coverage_str}");
        match &day.cancellation {
            CancellationFinding::Healthy { .. } => {
                println!(
                    "  cancellation:   healthy -- rates agree, neither crosses severity threshold"
                );
            }
            CancellationFinding::ConcerningCrossesSeverityThreshold {
                sample_rate,
                coverage_rate,
                threshold,
            } => {
                concerning += 1;
                println!(
                    "  cancellation:   CONCERNING -- full-coverage ({:.1}%) crosses this line's \
                     {:.0}% severity threshold while LDBWS ({:.1}%) does not. Possible Decision \
                     2d (unconfirmed-by-window-close = cancelled) inflation -- check \
                     full-coverage-consumer's own logs/metrics for this line/day for a coverage \
                     gap (missed Activation, untranslatable STANOX) before trusting this rate.",
                    coverage_rate * 100.0,
                    threshold * 100.0,
                    sample_rate * 100.0
                );
            }
            CancellationFinding::ConcerningButCorroborated {
                sample_rate,
                coverage_rate,
            } => {
                concerning += 1;
                println!(
                    "  cancellation:   concerning, but corroborated -- both LDBWS ({:.1}%) and \
                     full-coverage ({:.1}%) cross the severity threshold; a real disruption day, \
                     not (by itself) evidence of Decision 2d inflation.",
                    sample_rate * 100.0,
                    coverage_rate * 100.0
                );
            }
            CancellationFinding::ConcerningLargeGap {
                sample_rate,
                coverage_rate,
            } => {
                concerning += 1;
                println!(
                    "  cancellation:   concerning -- large gap (LDBWS {:.1}% vs. full-coverage \
                     {:.1}%) even though neither crosses the severity threshold; worth a look.",
                    sample_rate * 100.0,
                    coverage_rate * 100.0
                );
            }
            CancellationFinding::InsufficientData => {
                insufficient += 1;
                println!(
                    "  cancellation:   insufficient data (one or both sides missing this day)"
                );
            }
        }
        match &day.skipped {
            SkippedFinding::NotYetImplemented { sample_rate } => {
                println!(
                    "  skipped:        full-coverage always reports 0 here today (Decision 2g's \
                     PASS->skipped mapping is not implemented in \
                     crates/full-coverage-consumer/src/stats.rs -- see this tool's module doc). \
                     LDBWS's own skipped rate this day was {:.1}%; not a discrepancy, just data \
                     full-coverage doesn't produce yet.",
                    sample_rate * 100.0
                );
            }
            SkippedFinding::InsufficientData => {
                println!(
                    "  skipped:        insufficient data (one or both sides missing this day)"
                );
            }
        }
        println!();
    }

    println!(
        "summary: {} day(s) compared, {concerning} flagged concerning, {insufficient} with \
         insufficient data.",
        report.days.len()
    );
    println!(
        "\nA HEALTHY pilot result: most/all days classify as Healthy or ConcerningButCorroborated \
         (real disruption both sources agree on), with ConcerningCrossesSeverityThreshold rare or \
         explained by a confirmed full-coverage-consumer coverage gap for that specific day.\n\
         A CONCERNING pilot result: ConcerningCrossesSeverityThreshold or ConcerningLargeGap \
         recurring across many days with no corresponding LDBWS signal -- that is the signature \
         Decision 2d's own flagged accuracy risk would produce, and is grounds to flip \
         full_coverage_enabled back to false for this line pending investigation."
    );
}

fn print_pass_event_section(pass_events: &[full_coverage_comparison::PassEventCandidate]) {
    println!(
        "\n--- Decision 2g event-level check (best-effort; see module doc for why this is \
         narrow) ---"
    );
    if pass_events.is_empty() {
        println!(
            "no PASS-type train_movement_events rows found for a train matched to this line. \
             This is EXPECTED today, not a failed check -- see \
             api::data::full_coverage_comparison's module doc, \"Event-level verification of \
             Decision 2g\": no table this app writes currently retains a real TRUST PASS event \
             in a form this tool can cross-reference against LDBWS's skipped_stations."
        );
        return;
    }
    println!(
        "found {} PASS event(s) for a pin-tracked train on this line (most recent first) -- \
         cross-reference these manually against LDBWS station_samples for the same service/day \
         to look for a real skipped_stations match:",
        pass_events.len()
    );
    for event in pass_events {
        println!(
            "  train_movement_events.id={} train_uid={} loc_crs={:?} actual_timestamp={:?}",
            event.train_movement_event_id, event.train_uid, event.loc_crs, event.actual_timestamp
        );
    }
}
