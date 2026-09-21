//! Compares `full-coverage-consumer`'s TRUST-vs-schedule output against
//! LDBWS-sample-derived output for the same line over the same time
//! window. Built for the `tfw-conwy-valley` full-coverage pilot (this
//! branch's own `lines/tfw-conwy-valley.toml` change -- see that file's
//! "Full-coverage pilot" comment), but works for any line. Run via
//! `crates/api/src/bin/compare_full_coverage.rs`.
//!
//! ```text
//!   DATABASE_URL=postgres://... LINES_DIR=./lines \
//!     cargo run -p api --bin compare_full_coverage -- --line-id tfw-conwy-valley --days 30
//! ```
//!
//! # What this compares, and why RATES, not raw counts
//!
//! `line_status_daily_stats` (LDBWS-sample-derived, written by
//! `aggregator::queries::record_daily_stats`) and
//! `line_status_daily_coverage_stats` (full-coverage-derived, written by
//! `aggregator::queries::record_daily_coverage_stats`) are NOT directly
//! comparable on their raw `total`/`delayed`/`cancelled`/`skipped` columns,
//! for two independent reasons confirmed by reading the actual write
//! paths, not assumed:
//!
//!  1. **Different populations, by design.** LDBWS's `total` counts
//!     distinct Darwin `service_id`s seen at this line's own handful of
//!     `sample_stations`, deduped once per calendar day
//!     (`aggregator::dedup::dedup_new_sample_stats`). Full-coverage's
//!     `total` counts the line's ENTIRE CIF-scheduled population for the
//!     day (`schedule_query::schedules_touching`), whether or not TRUST
//!     ever reported on it. Full-coverage's raw total being much larger
//!     than LDBWS's is expected and healthy -- that asymmetry is the whole
//!     point of full coverage, not a discrepancy to flag.
//!  2. **Different accumulation semantics** (found while grounding this
//!     tool against the real write paths -- documented bluntly here
//!     because it isn't spelled out this concretely anywhere else):
//!     `record_daily_stats` accumulates only NEWLY-deduped services each
//!     aggregator cycle, so `line_status_daily_stats`'s sums are a genuine
//!     running total across the day. `record_daily_coverage_stats` (see
//!     its own "Decision 4 scaffolding" module doc in
//!     `crates/aggregator/src/queries.rs`) instead accumulates WHATEVER
//!     `LineStatus.full_coverage_stats` reads that cycle, with no
//!     per-service dedup analog -- so once a line's full-coverage row
//!     flips `Available` (Decision 2e: once, near end of the rail day) and
//!     stays `Available` for the rest of it, EVERY remaining aggregator
//!     cycle that day re-adds the SAME snapshot into
//!     `line_status_daily_coverage_stats`. Its raw `total`/`delayed`/
//!     `cancelled`/`skipped` for that day are therefore inflated by
//!     however many cycles ran after the flip (`resolved_windows`), not a
//!     real per-day count.
//!
//! Both problems cancel out if every count is turned into a RATE (X /
//! total) before comparing: LDBWS's rate is already the genuine per-day
//! rate among the trains it sampled, and full-coverage's rate is
//! `(resolved_windows * real_x) / (resolved_windows * real_total) =
//! real_x / real_total` regardless of how many cycles inflated the raw
//! sums, as long as the per-cycle snapshot was constant across every
//! contributing cycle (true here, since Decision 2e only recomputes the
//! row once per rail day). This is why every comparison below is
//! rate-based, never a raw count diff -- and why `resolved_windows`/
//! `sample_cycles` are still surfaced alongside each rate as a confidence
//! signal (a `resolved_windows` of 1 means the row only just flipped
//! `Available`, not that the rate itself is wrong).
//!
//! # The two heuristics this tool specifically checks
//!
//! - **Decision 2d, "unconfirmed-by-window-close = cancelled"**
//!   (`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`):
//!   a population UID `full-coverage-consumer` never saw a single TRUST
//!   event for by rail-day close is counted `cancelled`
//!   (`crates/full-coverage-consumer/src/stats.rs::build_line_row`'s `None`
//!   arm, confirmed by reading that file directly). This can only ever
//!   OVER-count cancellations relative to LDBWS, never under -- so
//!   [`classify_cancellation`] specifically watches for full-coverage's
//!   cancellation rate running persistently and substantially higher than
//!   LDBWS's for the same days, using this line's own configured
//!   `reduced_service_pct` severity threshold as the operationally
//!   meaningful cutoff (crossing it is what would actually change what a
//!   user sees, once merged).
//! - **Decision 2g, PASS -> `skipped` mapping**: confirmed by reading
//!   `crates/full-coverage-consumer/src/stats.rs` directly -- this mapping
//!   is NOT implemented, not merely "unconfirmed against a real case" as
//!   the design doc's own Open Question 3 phrased it. `synthesize_departure`
//!   and the "no derived state" fallback both hard-code
//!   `skipped_stations: vec![]` unconditionally (see that file's own
//!   Decision-2g comment on both call sites). So full-coverage's `skipped`
//!   count is ALWAYS zero today, by construction, regardless of how many
//!   real PASS-at-an-intermediate-stop events actually happened.
//!   [`classify_skipped`] reports that fact plainly rather than presenting
//!   a 0-vs-nonzero comparison as if it were a meaningful finding.
//!
//! # Event-level verification of Decision 2g: attempted, found not
//!   currently possible from this app's own persisted data
//!
//! Checked directly against this repo (2026-09-21, this branch): no table
//! this app writes retains a real TRUST `PASS` movement event in a form
//! this tool could cross-reference against LDBWS's `skipped_stations`.
//! `trust_event_backlog` explicitly drops `PASS` before insert
//! (`crates/trust-backlog-consumer/src/process.rs`: "Only a real calling
//! point -- never PASS"; its `event_type` column also carries a DB `CHECK`
//! that would reject `'PASS'` outright even if a caller tried).
//! `train_movement_events` (no such `CHECK`) is scoped to already
//! pin-tracked trains only -- a small, opt-in subset, not this line's full
//! scheduled population -- and carries no headcode/service-identity field
//! that would let a `PASS` row there be matched to an LDBWS
//! `skipped_stations` entry with any real confidence.
//! `full-coverage-consumer` itself never persists which `PASS` events it
//! saw or what it would have mapped them to (Decision 2g is unimplemented,
//! not merely unverified -- see above). [`find_pass_events_for_line_best_effort`]
//! still runs the closest available check (any `PASS`-type
//! `train_movement_events` row for a train matched to this line, via
//! `trains.matched_line_id`) for completeness and because a future schema
//! change could make it useful, but it is expected to return nothing
//! today -- that is not a bug in this tool, it is the honest state of the
//! data.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::NaiveDate;
use sqlx::PgPool;

use crate::data::queries::{self, DailyCoverageStatsRow, DailyStatsRow};

/// One day's LDBWS-sample-derived or full-coverage-derived rates, already
/// normalized (see module doc for why rates, not raw counts). `cycles` is
/// `sample_cycles` (LDBWS side) or `resolved_windows` (full-coverage
/// side) -- a confidence signal, not part of the comparison itself.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyRates {
    pub day: NaiveDate,
    pub cycles: i64,
    /// Raw total -- context only. See module doc point 1: full-coverage's
    /// total is expected to be much larger than LDBWS's by design (full
    /// scheduled population vs. a handful of sample stations), so this is
    /// never compared directly between the two sides.
    pub total: i64,
    pub cancelled_rate: f64,
    pub delayed_rate: f64,
    pub skipped_rate: f64,
    pub avg_delay_minutes: f64,
}

fn rate(numerator: i64, denominator: i64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn avg_delay(delay_minutes_sum: f64, running_count: i64) -> f64 {
    if running_count == 0 {
        0.0
    } else {
        delay_minutes_sum / running_count as f64
    }
}

pub fn sample_rates(row: &DailyStatsRow) -> DailyRates {
    DailyRates {
        day: row.day,
        cycles: row.sample_cycles,
        total: row.total,
        cancelled_rate: rate(row.cancelled, row.total),
        delayed_rate: rate(row.delayed, row.total),
        skipped_rate: rate(row.skipped, row.total),
        avg_delay_minutes: avg_delay(row.delay_minutes_sum, row.running_count),
    }
}

pub fn coverage_rates(row: &DailyCoverageStatsRow) -> DailyRates {
    DailyRates {
        day: row.day,
        cycles: row.resolved_windows,
        total: row.total,
        cancelled_rate: rate(row.cancelled, row.total),
        delayed_rate: rate(row.delayed, row.total),
        skipped_rate: rate(row.skipped, row.total),
        avg_delay_minutes: avg_delay(row.delay_minutes_sum, row.running_count),
    }
}

/// Decision 2d's own flagged risk, made concrete: full-coverage's
/// cancellation rate can only ever be inflated relative to LDBWS's (never
/// deflated), because an unconfirmed-by-close UID is always counted
/// cancelled, never dropped or left running. See module doc.
#[derive(Debug, Clone, PartialEq)]
pub enum CancellationFinding {
    /// Full-coverage's cancellation rate is not meaningfully higher than
    /// LDBWS's -- no sign of Decision 2d's inflation risk today.
    Healthy {
        sample_rate: f64,
        coverage_rate: f64,
    },
    /// Full-coverage's cancellation rate crosses this line's own
    /// configured `reduced_service_pct` severity threshold while LDBWS's
    /// does not -- the operationally meaningful case, since this is
    /// exactly the gap that would change what a user sees once this line
    /// is genuinely relying on full-coverage data.
    ConcerningCrossesSeverityThreshold {
        sample_rate: f64,
        coverage_rate: f64,
        threshold: f64,
    },
    /// Both sides agree the line is under real strain (both cross the
    /// threshold, or both sit close together) -- full-coverage is telling
    /// the same story LDBWS is, just from a different population.
    ConcerningButCorroborated {
        sample_rate: f64,
        coverage_rate: f64,
    },
    /// A large absolute gap even though neither side crosses the severity
    /// threshold -- worth a human look, less urgent than the
    /// threshold-crossing case above.
    ConcerningLargeGap {
        sample_rate: f64,
        coverage_rate: f64,
    },
    /// One or both sides have no rollup for this day yet.
    InsufficientData,
}

/// `large_gap_threshold` is an absolute rate difference (e.g. `0.15` for
/// 15 percentage points) above which a gap is worth flagging even when
/// neither side crosses `severity_threshold` -- deliberately a parameter,
/// not a hardcoded constant, so a caller (or a test) can reason about it
/// explicitly rather than this module silently encoding a "magic number"
/// tuned for one specific line's traffic level.
pub fn classify_cancellation(
    sample: Option<&DailyRates>,
    coverage: Option<&DailyRates>,
    severity_threshold: f64,
    large_gap_threshold: f64,
) -> CancellationFinding {
    let (Some(sample), Some(coverage)) = (sample, coverage) else {
        return CancellationFinding::InsufficientData;
    };
    let sample_rate = sample.cancelled_rate;
    let coverage_rate = coverage.cancelled_rate;
    let sample_crosses = sample_rate >= severity_threshold;
    let coverage_crosses = coverage_rate >= severity_threshold;
    let gap = coverage_rate - sample_rate;

    if coverage_crosses && !sample_crosses {
        CancellationFinding::ConcerningCrossesSeverityThreshold {
            sample_rate,
            coverage_rate,
            threshold: severity_threshold,
        }
    } else if coverage_crosses && sample_crosses {
        CancellationFinding::ConcerningButCorroborated {
            sample_rate,
            coverage_rate,
        }
    } else if gap > large_gap_threshold {
        CancellationFinding::ConcerningLargeGap {
            sample_rate,
            coverage_rate,
        }
    } else {
        CancellationFinding::Healthy {
            sample_rate,
            coverage_rate,
        }
    }
}

/// Decision 2g's status for one day -- deterministic given today's
/// `crates/full-coverage-consumer/src/stats.rs` implementation (always
/// `NotYetImplemented` whenever a coverage row exists at all), not a
/// judgment call based on the actual numbers. See module doc.
#[derive(Debug, Clone, PartialEq)]
pub enum SkippedFinding {
    /// Full-coverage has no skipped-mapping at all yet (Decision 2g is
    /// unimplemented) -- this is expected, not evidence either way about
    /// whether the eventual mapping will be correct.
    NotYetImplemented {
        sample_rate: f64,
    },
    InsufficientData,
}

pub fn classify_skipped(
    sample: Option<&DailyRates>,
    coverage: Option<&DailyRates>,
) -> SkippedFinding {
    match (sample, coverage) {
        (Some(sample), Some(_)) => SkippedFinding::NotYetImplemented {
            sample_rate: sample.skipped_rate,
        },
        _ => SkippedFinding::InsufficientData,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailyComparison {
    pub day: NaiveDate,
    pub sample: Option<DailyRates>,
    pub coverage: Option<DailyRates>,
    pub cancellation: CancellationFinding,
    pub skipped: SkippedFinding,
}

#[derive(Debug, Clone)]
pub struct ComparisonReport {
    pub line_id: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: Vec<DailyComparison>,
    /// The current live `full_coverage_line_stats` row for this line, if
    /// `full-coverage-consumer` has ever published one -- context only,
    /// not folded into `days` (that table is a live snapshot, not a
    /// per-day history; see `crates/api/migrations/*_full_coverage_line_stats.sql`'s
    /// own "one row per line" doc comment).
    pub live_snapshot: Option<common::FullCoverageLineStatsRow>,
}

/// Builds the full comparison for one line over `[from, to]` (inclusive),
/// joining `line_status_daily_stats` and `line_status_daily_coverage_stats`
/// by day. A day present on only one side still appears in the report
/// (`sample`/`coverage` is `None` on the missing side) -- this is the
/// expected, normal state for the pilot line until its own full-coverage
/// data accumulates, not an error.
pub async fn compare_line(
    pool: &PgPool,
    line_id: &str,
    from: NaiveDate,
    to: NaiveDate,
    severity_threshold: f64,
    large_gap_threshold: f64,
) -> Result<ComparisonReport> {
    let sample_rows = queries::daily_stats_for_range(pool, line_id, from, to).await?;
    let coverage_rows = queries::daily_coverage_stats_for_range(pool, line_id, from, to).await?;
    let live_snapshot = queries::get_full_coverage_line_stats(pool, line_id).await?;

    let mut by_day: BTreeMap<NaiveDate, (Option<DailyRates>, Option<DailyRates>)> = BTreeMap::new();
    for row in &sample_rows {
        by_day.entry(row.day).or_default().0 = Some(sample_rates(row));
    }
    for row in &coverage_rows {
        by_day.entry(row.day).or_default().1 = Some(coverage_rates(row));
    }

    let days = by_day
        .into_iter()
        .map(|(day, (sample, coverage))| {
            let cancellation = classify_cancellation(
                sample.as_ref(),
                coverage.as_ref(),
                severity_threshold,
                large_gap_threshold,
            );
            let skipped = classify_skipped(sample.as_ref(), coverage.as_ref());
            DailyComparison {
                day,
                sample,
                coverage,
                cancellation,
                skipped,
            }
        })
        .collect();

    Ok(ComparisonReport {
        line_id: line_id.to_string(),
        from,
        to,
        days,
        live_snapshot,
    })
}

/// One `train_movement_events` row this best-effort check surfaced --
/// always empty today (see module doc's "Event-level verification of
/// Decision 2g" section for exactly why), kept as a real query rather than
/// a stub so it starts finding rows the moment either `train_movement_events`
/// gains a service-identity field or this line's trains start getting
/// pin-tracked in numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct PassEventCandidate {
    pub train_movement_event_id: i64,
    pub train_uid: String,
    pub loc_crs: Option<String>,
    pub actual_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

/// Best-effort search for a real, observed TRUST `PASS` event on a train
/// matched to `line_id` -- the closest thing to "verify Decision 2g
/// against a real observed case" this app's current schema allows. Joins
/// `train_movement_events` (which does carry `PASS` rows -- unlike
/// `trust_event_backlog`, it has no `event_type` `CHECK` constraint) to
/// `trains` on `trains_id`, filtered to `trains.matched_line_id = line_id`.
/// Expected to return empty on a database with little pin-tracking
/// activity for this line, or if this line's pinned trains simply haven't
/// passed through an intermediate stop while pinned -- an empty result is
/// NOT proof PASS-at-an-intermediate-stop never happens on this line, only
/// that this narrow, best-effort check didn't catch an instance. See
/// module doc for why a stronger check isn't currently possible.
pub async fn find_pass_events_for_line_best_effort(
    pool: &PgPool,
    line_id: &str,
) -> Result<Vec<PassEventCandidate>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT tme.id, t.train_uid, tme.loc_crs, tme.actual_timestamp
         FROM train_movement_events tme
         JOIN trains t ON t.id = tme.trains_id
         WHERE t.matched_line_id = $1 AND tme.event_type = 'PASS'
         ORDER BY tme.actual_timestamp DESC NULLS LAST
         LIMIT 50",
    )
    .bind(line_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(PassEventCandidate {
                train_movement_event_id: row.try_get("id")?,
                train_uid: row.try_get("train_uid")?,
                loc_crs: row.try_get("loc_crs")?,
                actual_timestamp: row.try_get("actual_timestamp")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates(day: &str, cycles: i64, total: i64, cancelled_rate: f64) -> DailyRates {
        DailyRates {
            day: day.parse().unwrap(),
            cycles,
            total,
            cancelled_rate,
            delayed_rate: 0.0,
            skipped_rate: 0.0,
            avg_delay_minutes: 0.0,
        }
    }

    #[test]
    fn sample_rates_divides_each_count_by_total() {
        let row = DailyStatsRow {
            day: "2026-09-04".parse().unwrap(),
            sample_cycles: 10,
            total: 20,
            delayed: 5,
            cancelled: 2,
            skipped: 1,
            running_count: 18,
            delay_minutes_sum: 90.0,
        };
        let rates = sample_rates(&row);
        assert_eq!(rates.cancelled_rate, 0.1);
        assert_eq!(rates.delayed_rate, 0.25);
        assert_eq!(rates.skipped_rate, 0.05);
        assert_eq!(rates.avg_delay_minutes, 5.0);
    }

    #[test]
    fn coverage_rates_normalizes_away_the_resolved_windows_multiplier() {
        // Same underlying day (total 20, cancelled 2) observed for 6
        // resolved_windows -- the raw accumulated sums are 6x, but the
        // rate must come out identical to a single snapshot's rate. This
        // is the module doc's central claim, made concrete as a test.
        let one_window = DailyCoverageStatsRow {
            day: "2026-09-04".parse().unwrap(),
            resolved_windows: 1,
            total: 20,
            delayed: 5,
            cancelled: 2,
            skipped: 0,
            running_count: 18,
            delay_minutes_sum: 90.0,
        };
        let six_windows = DailyCoverageStatsRow {
            day: "2026-09-04".parse().unwrap(),
            resolved_windows: 6,
            total: 120,
            delayed: 30,
            cancelled: 12,
            skipped: 0,
            running_count: 108,
            delay_minutes_sum: 540.0,
        };
        assert_eq!(
            coverage_rates(&one_window).cancelled_rate,
            coverage_rates(&six_windows).cancelled_rate
        );
        assert_eq!(
            coverage_rates(&one_window).avg_delay_minutes,
            coverage_rates(&six_windows).avg_delay_minutes
        );
    }

    #[test]
    fn classify_cancellation_is_healthy_when_rates_agree_and_stay_below_threshold() {
        let sample = rates("2026-09-04", 10, 20, 0.05);
        let coverage = rates("2026-09-04", 3, 100, 0.06);
        let finding = classify_cancellation(Some(&sample), Some(&coverage), 0.35, 0.15);
        assert!(matches!(finding, CancellationFinding::Healthy { .. }));
    }

    #[test]
    fn classify_cancellation_flags_a_threshold_crossing_sampling_would_have_missed() {
        // The exact Decision 2d risk: full-coverage alone would have
        // escalated this line's severity (crosses reduced_service_pct)
        // while LDBWS sampling saw nothing of the sort.
        let sample = rates("2026-09-04", 10, 20, 0.05);
        let coverage = rates("2026-09-04", 3, 100, 0.40);
        let finding = classify_cancellation(Some(&sample), Some(&coverage), 0.35, 0.15);
        assert!(matches!(
            finding,
            CancellationFinding::ConcerningCrossesSeverityThreshold {
                threshold: 0.35,
                ..
            }
        ));
    }

    #[test]
    fn classify_cancellation_is_corroborated_when_both_sides_cross_the_threshold() {
        let sample = rates("2026-09-04", 10, 20, 0.45);
        let coverage = rates("2026-09-04", 3, 100, 0.50);
        let finding = classify_cancellation(Some(&sample), Some(&coverage), 0.35, 0.15);
        assert!(matches!(
            finding,
            CancellationFinding::ConcerningButCorroborated { .. }
        ));
    }

    #[test]
    fn classify_cancellation_flags_a_large_gap_even_below_threshold() {
        let sample = rates("2026-09-04", 10, 20, 0.02);
        let coverage = rates("2026-09-04", 3, 100, 0.25);
        let finding = classify_cancellation(Some(&sample), Some(&coverage), 0.35, 0.15);
        assert!(matches!(
            finding,
            CancellationFinding::ConcerningLargeGap { .. }
        ));
    }

    #[test]
    fn classify_cancellation_reports_insufficient_data_when_either_side_is_missing() {
        let sample = rates("2026-09-04", 10, 20, 0.05);
        assert_eq!(
            classify_cancellation(Some(&sample), None, 0.35, 0.15),
            CancellationFinding::InsufficientData
        );
        assert_eq!(
            classify_cancellation(None, None, 0.35, 0.15),
            CancellationFinding::InsufficientData
        );
    }

    #[test]
    fn classify_skipped_is_always_not_yet_implemented_when_both_sides_have_data() {
        let sample = rates("2026-09-04", 10, 20, 0.0);
        let coverage = rates("2026-09-04", 3, 100, 0.0);
        assert_eq!(
            classify_skipped(Some(&sample), Some(&coverage)),
            SkippedFinding::NotYetImplemented { sample_rate: 0.0 }
        );
    }

    #[test]
    fn classify_skipped_reports_insufficient_data_when_either_side_is_missing() {
        assert_eq!(
            classify_skipped(None, None),
            SkippedFinding::InsufficientData
        );
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// End-to-end smoke test: seeds one day of each rollup table for a
    /// reserved fixture line, runs `compare_line`, and asserts the
    /// resulting report reads back the right rates and classification --
    /// proving this module's SQL (not just its pure classify functions
    /// above) works against a real Postgres, per this branch's own
    /// verification pass.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                full_coverage_comparison -- --ignored --test-threads=1`"]
    async fn compare_line_reads_back_a_seeded_day_from_both_rollup_tables() {
        let pool = connect().await;
        let line_id = "TEST-FULL-COVERAGE-COMPARISON";
        let day: NaiveDate = "2026-09-10".parse().unwrap();

        sqlx::query(
            "INSERT INTO line_status_daily_stats \
             (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES ($1, $2, 10, 20, 5, 1, 0, 19, 95.0)",
        )
        .bind(line_id)
        .bind(day)
        .execute(&pool)
        .await
        .expect("seed line_status_daily_stats");

        sqlx::query(
            "INSERT INTO line_status_daily_coverage_stats \
             (line_id, day, resolved_windows, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES ($1, $2, 4, 400, 40, 20, 0, 380, 1900.0)",
        )
        .bind(line_id)
        .bind(day)
        .execute(&pool)
        .await
        .expect("seed line_status_daily_coverage_stats");

        let report = compare_line(&pool, line_id, day, day, 0.35, 0.15)
            .await
            .expect("compare_line");

        assert_eq!(report.days.len(), 1);
        let comparison = &report.days[0];
        assert_eq!(comparison.day, day);
        let sample = comparison.sample.as_ref().expect("sample rates present");
        let coverage = comparison
            .coverage
            .as_ref()
            .expect("coverage rates present");
        assert_eq!(sample.cancelled_rate, 0.05);
        assert_eq!(coverage.cancelled_rate, 0.05); // 20/400, same underlying rate as 1/20
        assert!(matches!(
            comparison.cancellation,
            CancellationFinding::Healthy { .. }
        ));
        assert!(matches!(
            comparison.skipped,
            SkippedFinding::NotYetImplemented { .. }
        ));

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id = $1")
            .bind(line_id)
            .execute(&pool)
            .await
            .expect("cleanup line_status_daily_stats");
        sqlx::query("DELETE FROM line_status_daily_coverage_stats WHERE line_id = $1")
            .bind(line_id)
            .execute(&pool)
            .await
            .expect("cleanup line_status_daily_coverage_stats");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_pass_events_for_line_best_effort -- --ignored --test-threads=1`"]
    async fn find_pass_events_for_line_best_effort_returns_empty_for_an_unknown_line() {
        let pool = connect().await;
        let found = find_pass_events_for_line_best_effort(&pool, "TEST-NO-SUCH-LINE")
            .await
            .expect("query should succeed even with no matching rows");
        assert!(found.is_empty());
    }
}
