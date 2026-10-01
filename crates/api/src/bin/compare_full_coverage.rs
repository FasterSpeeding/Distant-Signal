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
//! `--windows` (2026-09-27) reports on the windowed full-coverage stats
//! instead -- the shadow-mode evidence for turning `enforce` on, and for
//! picking its pilot lines (see `api::data::full_coverage_window_report`):
//!
//! ```text
//!   DATABASE_URL=postgres://... LINES_DIR=./lines \
//!     cargo run -p api --bin compare_full_coverage -- --windows --all-lines --days 7 \
//!       [--min-rank 4] [--csv ./report-dir]
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
    /// The catalogue line id to compare, e.g. `tfw-conwy-valley`. Required
    /// unless `--windows --all-lines`.
    #[arg(long)]
    line_id: Option<String>,
    /// Report on the windowed full-coverage stats (recent / day-to-date
    /// windows, the aggregator's verdicts) instead of the daily rollups.
    #[arg(long)]
    windows: bool,
    /// With `--windows`: every line, not just `--line-id`.
    #[arg(long)]
    all_lines: bool,
    /// With `--windows`: the enforced tier's `severity_rank` gate (4 =
    /// Severe Delays / Part Suspended), to split escalations into enforced
    /// and would-escalate-only.
    #[arg(long, default_value_t = common::full_coverage_window::FULL_COVERAGE_WINDOW_DEFAULT_MIN_ESCALATION_RANK)]
    min_rank: u8,
    /// With `--windows`: also write escalations.csv, ldbws_pairs.csv and
    /// line_volumes.csv to this directory.
    #[arg(long)]
    csv: Option<std::path::PathBuf>,
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
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

async fn run() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    common::logging::init_with_filter(
        "compare-full-coverage",
        common::logging::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| common::logging::EnvFilter::new("info")),
    );

    let args = Args::parse();
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let pool = PgPoolOptions::new().connect(&database_url).await?;

    let to = args.to.unwrap_or_else(|| Utc::now().date_naive());
    let from = args
        .from
        .unwrap_or_else(|| to - Duration::days(args.days.max(0)));

    if args.windows {
        return windows_report::run(&pool, &args, from, to).await;
    }
    let Some(line_id) = args.line_id.clone() else {
        anyhow::bail!("--line-id is required (or use --windows --all-lines)");
    };

    let severity_threshold = severity_threshold_for(&line_id, &args.lines_dir);

    let report = full_coverage_comparison::compare_line(
        &pool,
        &line_id,
        from,
        to,
        severity_threshold,
        args.large_gap_threshold,
    )
    .await?;

    print_report(&report, severity_threshold);

    let pass_events =
        full_coverage_comparison::find_pass_events_for_line_best_effort(&pool, &line_id).await?;
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
             partial={} total={} delayed={} cancelled={} skipped={} avg_delay_minutes={:.1}\n",
            snapshot.service_date,
            snapshot.availability,
            snapshot.partial,
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

    if !report.full_coverage_history.is_empty() {
        println!("full_coverage_line_stats by day (a partial day is not clean signal):");
        for row in &report.full_coverage_history {
            println!(
                "  {} availability={} partial={} total={} delayed={} cancelled={} skipped={}",
                row.service_date,
                row.availability,
                row.partial,
                row.stats.total,
                row.stats.delayed,
                row.stats.cancelled,
                row.stats.skipped
            );
        }
        println!();
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

/// `--windows`: formatting only; see `api::data::full_coverage_window_report`.
mod windows_report {
    use std::collections::HashMap;
    use std::io::Write;

    use api::data::full_coverage_window_report as report;
    use chrono::{DateTime, NaiveDate, Utc};

    use super::Args;

    fn pct(v: f64) -> String {
        format!("{:.1}%", v * 100.0)
    }

    pub async fn run(
        pool: &sqlx::PgPool,
        args: &Args,
        from: NaiveDate,
        to: NaiveDate,
    ) -> anyhow::Result<()> {
        let line_id = match (&args.line_id, args.all_lines) {
            (_, true) => None,
            (Some(line), false) => Some(line.as_str()),
            (None, false) => anyhow::bail!("--windows needs --line-id or --all-lines"),
        };
        let start: DateTime<Utc> = from.and_hms_opt(0, 0, 0).expect("midnight").and_utc();
        let end: DateTime<Utc> = (to + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc();
        let defaults = common::Defaults::default();
        let per_line: HashMap<String, common::Defaults> = match common::config::parse_lines(
            &args.lines_dir,
        ) {
            Ok(lines) => lines
                .iter()
                .map(|l| {
                    (
                        l.id.clone(),
                        common::thresholds_for(&defaults, &l.severity_overrides),
                    )
                })
                .collect(),
            Err(err) => {
                tracing::warn!(error = ?err, "could not load the line catalogue; using default thresholds");
                HashMap::new()
            }
        };

        let inputs = report::load(pool, line_id, start, end).await?;
        println!(
            "windowed full-coverage report, {} to {} (UTC days), {}; enforced tier: severity_rank >= {}",
            from,
            to,
            line_id.map_or("all lines".to_string(), |l| format!("line {l:?}")),
            args.min_rank
        );
        println!(
            "note: full coverage counts a train delayed at 3+ minutes late at its first call on the \
             line; LDBWS at 5+. Full-coverage late rates run higher by construction.\n"
        );
        if inputs.windows.is_empty() {
            println!(
                "no full_coverage_line_window_stats rows in this range -- is \
                 FULL_COVERAGE_WINDOWED_STATS=true on full-coverage-consumer?"
            );
            return Ok(());
        }

        // 1. Health.
        let health = report::health(&inputs, &per_line, start, end.min(Utc::now()));
        println!("== 1. health (recent buckets) ==");
        println!(
            "{:<40} {:>8} {:>9} {:>8}  ineligible (below_threshold/partial/feed_stale/stale_row)",
            "line", "present", "expected", "eligible"
        );
        for h in &health {
            let reasons: Vec<String> = report::INELIGIBLE_REASONS
                .iter()
                .map(|r| {
                    h.ineligible
                        .get(r.as_str())
                        .copied()
                        .unwrap_or(0)
                        .to_string()
                })
                .collect();
            println!(
                "{:<40} {:>8} {:>9} {:>8}  {}  ({} present)",
                h.line_id,
                h.buckets_present,
                h.buckets_expected,
                h.eligible,
                reasons.join("/"),
                pct(h.presence())
            );
        }

        // 2. Would-escalate log.
        let escalations = report::escalations(&inputs, &per_line, args.min_rank);
        let flapping = report::flapping_lines(&inputs, &per_line, args.min_rank);
        let often = report::often_escalated_lines(&inputs, &per_line, &escalations);
        println!("\n== 2. would-escalate log (vs the severity each line was showing) ==");
        for e in &escalations {
            println!(
                "{} {:<40} {} -> {}{}  total={} late={} cancelled={}+{} skipped={}  {}",
                e.at.format("%Y-%m-%d %H:%MZ"),
                e.line_id,
                e.from.description(),
                e.to.description(),
                if e.below_min_rank {
                    " [below gate: recorded only]"
                } else {
                    " [enforced tier]"
                },
                e.counts.total,
                e.counts.delayed,
                e.counts.cancelled_explicit,
                e.counts.cancelled_presumed,
                e.counts.skipped,
                e.reason
            );
        }
        let (by_severity, by_hour) = report::escalation_totals(&escalations);
        println!("totals by severity:");
        for ((severity, enforced), n) in &by_severity {
            println!(
                "  {severity:<20} {:<22} {n}",
                if *enforced {
                    "enforced tier"
                } else {
                    "would-escalate only"
                }
            );
        }
        println!(
            "by London hour: {}",
            by_hour
                .iter()
                .map(|(h, n)| format!("{h:02}h={n}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "flapping (>= {} transitions within 2 h): {flapping:?}",
            report::FLAP_TRANSITIONS
        );
        println!("escalated in > 20% of daytime buckets: {often:?}");

        // 3. Against LDBWS.
        let pairs = report::ldbws_pairs(&inputs, &per_line);
        let both: Vec<&report::LdbwsPair> = pairs
            .iter()
            .filter(|p| p.fc_eligible && p.ldbws_total >= report::LDBWS_MIN_TOTAL)
            .collect();
        let late: Vec<(f64, f64)> = both
            .iter()
            .map(|p| (p.fc_late_rate(), p.ldbws_late_rate()))
            .collect();
        let cancel: Vec<(f64, f64)> = both
            .iter()
            .map(|p| (p.fc_cancel_rate(), p.ldbws_cancel_rate()))
            .collect();
        println!(
            "\n== 3. against LDBWS (recent bucket vs the half-hours covering its due range) =="
        );
        println!(
            "paired buckets with an eligible window and >= {} LDBWS services: {}",
            report::LDBWS_MIN_TOTAL,
            both.len()
        );
        let mean = |v: &[(f64, f64)], first: bool| {
            if v.is_empty() {
                0.0
            } else {
                v.iter()
                    .map(|(a, b)| if first { *a } else { *b })
                    .sum::<f64>()
                    / v.len() as f64
            }
        };
        println!(
            "late rate: full coverage mean {} vs LDBWS {}; correlation {:?}",
            pct(mean(&late, true)),
            pct(mean(&late, false)),
            report::correlation(&late)
        );
        println!(
            "cancel rate: full coverage mean {} vs LDBWS {}; correlation {:?}",
            pct(mean(&cancel, true)),
            pct(mean(&cancel, false)),
            report::correlation(&cancel)
        );
        println!("severity confusion (full coverage, LDBWS) -> buckets:");
        for ((fc, ld), n) in report::confusion(&pairs) {
            println!("  {fc:<20} {ld:<20} {n}");
        }
        let (agree, of) = report::agreement_when_ldbws_sees_trouble(&pairs);
        println!(
            "when LDBWS shows >= Minor Delays, the eligible window does too: {agree} of {of} ({})",
            pct(if of == 0 {
                0.0
            } else {
                agree as f64 / of as f64
            })
        );

        // 4. Against the closed day.
        let checks = report::closed_day_checks(&inputs);
        println!("\n== 4. closed-day audit rows ==");
        for version in [2u16, 1] {
            println!(
                "-- stats_version {version}{} --",
                if version == 1 {
                    " (legacy whole-population method: not comparable with the windows)"
                } else {
                    ""
                }
            );
            for c in checks.iter().filter(|c| c.stats_version == version) {
                println!(
                    "{} {:<40} {} partial={} total={} last-day-to-date={:?} agree(2%)={:?} \
                     late={} cancel={} | LDBWS late={} cancel={}",
                    c.service_date,
                    c.line_id,
                    c.availability,
                    c.partial,
                    c.closed_total,
                    c.last_day_to_date_total,
                    c.totals_agree(),
                    pct(c.closed_late_rate),
                    pct(c.closed_cancel_rate),
                    c.ldbws_late_rate.map_or("-".to_string(), pct),
                    c.ldbws_cancel_rate.map_or("-".to_string(), pct)
                );
            }
        }

        // 5. The aggregator's own record.
        let summary = report::verdict_summary(&inputs.verdicts);
        println!("\n== 5. aggregator verdicts (full_coverage_window_verdicts) ==");
        if summary.evaluated == 0 {
            println!("none -- FULL_COVERAGE_WINDOW_MODE is off (or the api migration is missing)");
        } else {
            println!("{} bucket verdicts", summary.evaluated);
            for (verdict, n) in &summary.by_verdict {
                println!("  {verdict:<30} {n}");
            }
            println!("escalations (severity, enforced / would-escalate / below gate):");
            for ((severity, kind), n) in &summary.escalations {
                println!("  {severity:<20} {kind:<16} {n}");
            }
        }

        // 6. Volume, for picking the pilot lines.
        let volumes = report::line_volumes(&inputs, &health, &escalations, &flapping);
        println!("\n== 6. per-line volume (for picking the enforce pilot lines) ==");
        println!(
            "{:<40} {:>10} {:>6} {:>9} {:>9} {:>8} {:>9} {:>9} {:>5}",
            "line",
            "day-median",
            "p90",
            "max-daily",
            "eligible",
            "present",
            "enforced",
            "below-gate",
            "flap"
        );
        let mut sorted = volumes.clone();
        sorted.sort_by_key(|v| std::cmp::Reverse(v.daytime_median));
        for v in &sorted {
            println!(
                "{:<40} {:>10} {:>6} {:>9} {:>9} {:>8} {:>9} {:>9} {:>5}",
                v.line_id,
                v.daytime_median,
                v.daytime_p90,
                v.max_daily,
                pct(v.eligible_share),
                pct(v.bucket_presence),
                v.escalations_enforced_tier,
                v.escalations_below_gate,
                v.flapping
            );
        }
        println!(
            "suggested pilot lines (daytime median trains per window near each target, clean health):"
        );
        for (target, pick) in report::suggest_pilots(&volumes) {
            println!(
                "  ~{target:>2}: {}",
                pick.as_deref().unwrap_or("(no clean candidate)")
            );
        }

        if let Some(dir) = &args.csv {
            write_csv(dir, &escalations, &pairs, &volumes)?;
            println!("\nCSV written to {}", dir.display());
        }
        Ok(())
    }

    fn write_csv(
        dir: &std::path::Path,
        escalations: &[report::Escalation],
        pairs: &[report::LdbwsPair],
        volumes: &[report::LineVolume],
    ) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut f = std::fs::File::create(dir.join("escalations.csv"))?;
        writeln!(
            f,
            "computed_at,line_id,from,to,enforced_tier,total,delayed,cancelled_explicit,cancelled_presumed,skipped,reason"
        )?;
        for e in escalations {
            writeln!(
                f,
                "{},{},{},{},{},{},{},{},{},{},{}",
                e.at.to_rfc3339(),
                e.line_id,
                quote(e.from.description()),
                quote(e.to.description()),
                !e.below_min_rank,
                e.counts.total,
                e.counts.delayed,
                e.counts.cancelled_explicit,
                e.counts.cancelled_presumed,
                e.counts.skipped,
                quote(&e.reason)
            )?;
        }
        let mut f = std::fs::File::create(dir.join("ldbws_pairs.csv"))?;
        writeln!(
            f,
            "bucket_start,line_id,fc_eligible,fc_total,fc_late_rate,fc_cancel_rate,fc_severity,ldbws_total,ldbws_late_rate,ldbws_cancel_rate,ldbws_severity"
        )?;
        for p in pairs {
            writeln!(
                f,
                "{},{},{},{},{:.4},{:.4},{},{},{:.4},{:.4},{}",
                p.bucket_start.to_rfc3339(),
                p.line_id,
                p.fc_eligible,
                p.fc.total,
                p.fc_late_rate(),
                p.fc_cancel_rate(),
                quote(p.fc_severity.map_or("", |s| s.description())),
                p.ldbws_total,
                p.ldbws_late_rate(),
                p.ldbws_cancel_rate(),
                quote(p.ldbws_severity.map_or("", |s| s.description()))
            )?;
        }
        let mut f = std::fs::File::create(dir.join("line_volumes.csv"))?;
        writeln!(
            f,
            "line_id,daytime_median,daytime_p90,max_daily,eligible_share,bucket_presence,escalations_enforced_tier,escalations_below_gate,flapping"
        )?;
        for v in volumes {
            writeln!(
                f,
                "{},{},{},{},{:.4},{:.4},{},{},{}",
                v.line_id,
                v.daytime_median,
                v.daytime_p90,
                v.max_daily,
                v.eligible_share,
                v.bucket_presence,
                v.escalations_enforced_tier,
                v.escalations_below_gate,
                v.flapping
            )?;
        }
        Ok(())
    }
}
