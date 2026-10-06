//! "Ended (no longer listed)": inferring that an incident has left RDM's
//! Knowledgebase feed without RDM ever clearing it (user decision,
//! 2026-10-06; docs/superpowers/specs/2026-10-06-incident-source-removal-design.md).
//!
//! The KB feed purges incidents nightly (~22:57-23:00 UTC) whatever their
//! state, and never clears planned ones, so "active" as `NOT is_cleared`
//! alone left rows active forever. `is_cleared` stays RDM's own fact; this
//! module maintains OUR observation that the feed stopped listing a row:
//! `incidents.source_missing_polls` and `incidents.source_removed_at`.
//!
//! # The guard
//!
//! Absence is evidence only when the snapshot is the whole feed, so
//! inference runs once per POST, after every chunk of
//! `queries::upsert_incident_snapshot` has committed, and only when ALL of:
//!
//! 1. the poller says the snapshot is complete (`IncidentSnapshot::complete`:
//!    no malformed `<PtIncident>` skipped, document not truncated). An older
//!    poller's bare array is never complete;
//! 2. it lists at least one incident;
//! 3. the previous complete snapshot was at least [`MIN_INFERENCE_GAP_SECS`]
//!    ago: a retried POST whose first attempt already committed (the
//!    response was lost) must not count the same poll twice;
//! 4. a previous complete snapshot exists and this one is not more than 50%
//!    smaller than it.
//!
//! Every complete, non-empty snapshot that is not "too soon" becomes the new
//! baseline for (4) whether or not inference ran, so a real large purge
//! delays inference by one poll instead of blocking it until the feed
//! regrows.
//!
//! When the guard passes, every not-cleared, not-yet-removed row missing
//! from this snapshot gets `source_missing_polls + 1`; a row reaching
//! [`MISSES_TO_REMOVE`] gets `source_removed_at = fetched_at`. Rows the
//! snapshot DOES list were already reset (counter 0, `source_removed_at`
//! NULL) by the chunk upserts, complete snapshot or not: presence is
//! positive evidence on its own. Planned and unplanned rows are treated the
//! same.
//!
//! `source_removed_at` is the row's `fetched_at`, the last time the feed
//! listed it, rather than `now()`: it is the best bound we have on when the
//! row left (some time after that), it does not depend on when the second
//! miss happened to be confirmed, and it is what the one-off backfill
//! (scripts/backfill-2026-10-06-incident-source-removed.sql) writes too, so
//! old and new removals read the same.

use anyhow::Result;
use sqlx::PgPool;

/// Consecutive complete snapshots an incident must be missing from before
/// it is marked removed (user decision 2026-10-06: 2).
pub const MISSES_TO_REMOVE: i16 = 2;

/// Inference is skipped for a complete snapshot arriving less than this long
/// after the previous one (guard 3 in the module docs). The poller polls
/// every 300s and gives up retrying a POST after a quarter of that
/// (`common::poller_loop::post_retry_budget`), plus its 30s request timeout,
/// so a retry of an already-committed POST lands well inside 120s while the
/// next real poll does not. A poller configured to poll faster than this
/// only gets every other poll counted: slower, never wrong.
pub const MIN_INFERENCE_GAP_SECS: f64 = 120.0;

/// Suffix of `distant_signal_api_incident_removal_inference_total{outcome}`:
/// one increment per incidents POST, labelled with [`Inference::label`].
pub const INFERENCE_METRIC: &str = "api_incident_removal_inference_total";

/// Suffix of `distant_signal_api_incidents_marked_removed_total`: rows newly
/// given a `source_removed_at` (the per-poll removal count).
pub const MARKED_REMOVED_METRIC: &str = "api_incidents_marked_removed_total";

/// Every [`Inference::label`], for zero-registration at startup.
const OUTCOMES: [&str; 6] = [
    "applied",
    "incomplete",
    "empty",
    "too_soon",
    "no_baseline",
    "shrink",
];

/// Registers both metrics at 0 at startup, so the chart's
/// `DistantSignalIncidentRemovalInferenceStalled` alert sees the first
/// increment of every series.
pub fn register_metrics() {
    for outcome in OUTCOMES {
        metrics::counter!(common::metrics::metric_name(INFERENCE_METRIC), "outcome" => outcome)
            .increment(0);
    }
    metrics::counter!(common::metrics::metric_name(MARKED_REMOVED_METRIC)).increment(0);
}

/// What one snapshot's inference did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inference {
    /// The guard passed. `missing`: rows whose counter advanced this poll;
    /// `removed`: of those, the ones that reached [`MISSES_TO_REMOVE`].
    Applied { missing: u64, removed: u64 },
    /// The poller did not vouch for the snapshot being the whole feed.
    Incomplete,
    /// A complete snapshot listing nothing: an upstream outage looks
    /// exactly like this, so it is never evidence.
    Empty,
    /// Within [`MIN_INFERENCE_GAP_SECS`] of the previous complete snapshot.
    TooSoon,
    /// No previous complete snapshot to compare sizes with (first poll
    /// after deploy). Recorded as the baseline.
    NoBaseline,
    /// More than 50% smaller than the previous complete snapshot. Recorded
    /// as the new baseline.
    Shrink { previous: i64, current: i64 },
}

impl Inference {
    /// The metric's `outcome` label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "applied",
            Self::Incomplete => "incomplete",
            Self::Empty => "empty",
            Self::TooSoon => "too_soon",
            Self::NoBaseline => "no_baseline",
            Self::Shrink { .. } => "shrink",
        }
    }
}

/// The pure part of the guard (rules 2-4 of the module docs; rule 1 is
/// decided before any database work). `previous` is the stored baseline as
/// `(size, seconds since it was recorded)`. Returns the verdict and whether
/// this snapshot becomes the new baseline.
pub(crate) fn judge(current: i64, previous: Option<(i64, f64)>) -> (Inference, bool) {
    if current == 0 {
        return (Inference::Empty, false);
    }
    let Some((previous_size, age_secs)) = previous else {
        return (Inference::NoBaseline, true);
    };
    if age_secs < MIN_INFERENCE_GAP_SECS {
        return (Inference::TooSoon, false);
    }
    // "More than 50% smaller": current < previous / 2, in integers.
    if current.saturating_mul(2) < previous_size {
        return (
            Inference::Shrink {
                previous: previous_size,
                current,
            },
            true,
        );
    }
    (
        Inference::Applied {
            missing: 0,
            removed: 0,
        },
        true,
    )
}

/// Runs the guard and, if it passes, advances the miss counters, all in one
/// transaction. `present_ids` is every incident id the snapshot listed;
/// the caller has already committed their upserts (and so their resets).
///
/// The baseline row is read `FOR UPDATE`, so two overlapping POSTs (a slow
/// request and its retry) run their inference one after the other, and the
/// second sees the first's baseline and is skipped as too soon.
#[expect(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a feed has at most a few thousand incidents; counts are never negative"
)]
pub async fn infer_removals(
    pool: &PgPool,
    present_ids: &[&str],
    complete: bool,
) -> Result<Inference> {
    if !complete {
        return Ok(record(Inference::Incomplete));
    }
    let mut distinct: Vec<&str> = present_ids.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    let current = distinct.len() as i64;

    let mut tx = pool.begin().await?;
    let previous: Option<(i32, f64)> = sqlx::query_as(
        "SELECT last_complete_size, \
                EXTRACT(EPOCH FROM (now() - last_complete_at))::float8 \
           FROM incident_feed_state WHERE singleton FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let (verdict, new_baseline) =
        judge(current, previous.map(|(size, age)| (i64::from(size), age)));

    if new_baseline {
        sqlx::query(
            "INSERT INTO incident_feed_state (singleton, last_complete_at, last_complete_size) \
             VALUES (TRUE, now(), $1) \
             ON CONFLICT (singleton) DO UPDATE SET \
                 last_complete_at = EXCLUDED.last_complete_at, \
                 last_complete_size = EXCLUDED.last_complete_size",
        )
        .bind(i32::try_from(current).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    }

    let verdict = if let Inference::Applied { .. } = verdict {
        let (missing, removed): (i64, i64) = sqlx::query_as(
            "WITH missed AS ( \
                 UPDATE incidents SET \
                     source_missing_polls = source_missing_polls + 1, \
                     source_removed_at = CASE WHEN source_missing_polls + 1 >= $2 \
                                              THEN fetched_at END \
                  WHERE NOT is_cleared \
                    AND source_removed_at IS NULL \
                    AND NOT (incident_id = ANY($1)) \
                 RETURNING source_removed_at \
             ) \
             SELECT count(*), count(source_removed_at) FROM missed",
        )
        .bind(&distinct)
        .bind(MISSES_TO_REMOVE)
        .fetch_one(&mut *tx)
        .await?;
        Inference::Applied {
            missing: missing as u64,
            removed: removed as u64,
        }
    } else {
        verdict
    };

    tx.commit().await?;
    Ok(record(verdict))
}

/// Logs and counts one verdict, returning it.
fn record(verdict: Inference) -> Inference {
    metrics::counter!(
        common::metrics::metric_name(INFERENCE_METRIC),
        "outcome" => verdict.label()
    )
    .increment(1);
    match verdict {
        Inference::Applied { missing, removed } => {
            metrics::counter!(common::metrics::metric_name(MARKED_REMOVED_METRIC))
                .increment(removed);
            if missing > 0 {
                tracing::info!(
                    missing,
                    removed,
                    "incidents missing from a complete feed snapshot; those missing twice are now ended (no longer listed)"
                );
            }
        }
        Inference::Incomplete => tracing::warn!(
            "incident snapshot not complete (malformed elements skipped, a truncated body, or an older poller); skipping removal inference"
        ),
        Inference::Empty => tracing::warn!(
            "complete incident snapshot lists no incidents; skipping removal inference"
        ),
        Inference::TooSoon => tracing::info!(
            min_gap_secs = MIN_INFERENCE_GAP_SECS,
            "incident snapshot arrived too soon after the previous complete one (a retried POST?); skipping removal inference"
        ),
        Inference::NoBaseline => tracing::info!(
            "first complete incident snapshot: recorded its size as the baseline; skipping removal inference"
        ),
        Inference::Shrink { previous, current } => tracing::warn!(
            previous,
            current,
            "complete incident snapshot is more than 50% smaller than the previous one; skipping removal inference this poll"
        ),
    }
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_snapshot_is_never_evidence_and_never_a_baseline() {
        assert_eq!(judge(0, None), (Inference::Empty, false));
        assert_eq!(judge(0, Some((900, 300.0))), (Inference::Empty, false));
    }

    #[test]
    fn the_first_complete_snapshot_only_records_a_baseline() {
        assert_eq!(judge(900, None), (Inference::NoBaseline, true));
    }

    #[test]
    fn a_retry_inside_the_gap_counts_nothing_and_keeps_the_baseline() {
        assert_eq!(judge(900, Some((900, 30.0))), (Inference::TooSoon, false));
    }

    #[test]
    fn more_than_half_smaller_is_skipped_but_becomes_the_baseline() {
        assert_eq!(
            judge(449, Some((900, 300.0))),
            (
                Inference::Shrink {
                    previous: 900,
                    current: 449
                },
                true
            )
        );
        // Exactly half is not MORE than 50% smaller.
        assert!(matches!(
            judge(450, Some((900, 300.0))),
            (Inference::Applied { .. }, true)
        ));
    }

    #[test]
    fn labels_match_the_registered_outcomes() {
        let verdicts = [
            Inference::Applied {
                missing: 0,
                removed: 0,
            },
            Inference::Incomplete,
            Inference::Empty,
            Inference::TooSoon,
            Inference::NoBaseline,
            Inference::Shrink {
                previous: 2,
                current: 0,
            },
        ];
        let labels: Vec<&str> = verdicts.iter().map(Inference::label).collect();
        assert_eq!(labels, OUTCOMES);
    }

    #[test]
    fn a_normal_poll_applies() {
        assert!(matches!(
            judge(964, Some((965, 300.0))),
            (Inference::Applied { .. }, true)
        ));
        // Growth is never suspicious.
        assert!(matches!(
            judge(2000, Some((965, 300.0))),
            (Inference::Applied { .. }, true)
        ));
    }
}

/// Against a real database through the real write path
/// (`queries::upsert_incident_snapshot`). Inference is global -- it ages
/// every listed-elsewhere row in the table -- so these tests reset
/// `incident_feed_state` first, assert only on their own `TEST-REMOVAL-`
/// rows, and need `--test-threads=1`.
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::data::queries::{self, IncidentSnapshotOutcome};
    use common::IncidentMessage;
    use sqlx::postgres::PgPoolOptions;

    const PREFIX: &str = "TEST-REMOVAL-";

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn reset(pool: &PgPool) {
        for sql in [
            "DELETE FROM incident_history WHERE incident_id LIKE 'TEST-REMOVAL-%'",
            "DELETE FROM incidents WHERE incident_id LIKE 'TEST-REMOVAL-%'",
            "DELETE FROM incident_feed_state",
        ] {
            sqlx::query(sql).execute(pool).await.expect(sql);
        }
    }

    fn incident(suffix: &str, planned: bool, cleared: bool) -> IncidentMessage {
        IncidentMessage {
            incident_id: format!("{PREFIX}{suffix}"),
            summary: format!("{suffix} summary"),
            description: format!("{suffix} description"),
            operators: vec!["ZZ".to_string()],
            affected_stations: vec![],
            priority: 2,
            validity: vec![],
            is_planned: planned,
            is_cleared: cleared,
        }
    }

    fn unplanned(suffix: &str) -> IncidentMessage {
        incident(suffix, false, false)
    }

    fn planned(suffix: &str) -> IncidentMessage {
        incident(suffix, true, false)
    }

    /// One poll, as if [`MIN_INFERENCE_GAP_SECS`] had passed since the
    /// previous one (the stored baseline is backdated first).
    async fn poll(pool: &PgPool, batch: &[IncidentMessage], complete: bool) -> Inference {
        sqlx::query(
            "UPDATE incident_feed_state SET last_complete_at = now() - interval '10 minutes'",
        )
        .execute(pool)
        .await
        .expect("backdate the baseline");
        poll_now(pool, batch, complete).await
    }

    /// One poll with no backdating.
    async fn poll_now(pool: &PgPool, batch: &[IncidentMessage], complete: bool) -> Inference {
        // Port 1: a publish fails fast and is only logged.
        let redis = redis::Client::open("redis://127.0.0.1:1").expect("redis url");
        let matcher = common::matcher::LineMatcher::new(&[]);
        let IncidentSnapshotOutcome { inference, .. } =
            queries::upsert_incident_snapshot(pool, &redis, &matcher, batch, complete)
                .await
                .expect("upsert snapshot");
        inference
    }

    /// `(source_missing_polls, ended?)`, checking on the way that an ended
    /// row's `source_removed_at` is its `fetched_at`.
    async fn state(pool: &PgPool, suffix: &str) -> (i16, bool) {
        let (missing, removed_at, fetched_at): (
            i16,
            Option<chrono::DateTime<chrono::Utc>>,
            chrono::DateTime<chrono::Utc>,
        ) = sqlx::query_as(
            "SELECT source_missing_polls, source_removed_at, fetched_at \
               FROM incidents WHERE incident_id = $1",
        )
        .bind(format!("{PREFIX}{suffix}"))
        .fetch_one(pool)
        .await
        .expect("row exists");
        if let Some(removed_at) = removed_at {
            assert_eq!(
                removed_at, fetched_at,
                "{suffix}: source_removed_at is when the feed last listed it"
            );
        }
        (missing, removed_at.is_some())
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_removal::db_tests -- --ignored --test-threads=1`"]
    async fn missing_from_two_complete_polls_ends_planned_and_unplanned_alike() {
        let pool = test_pool().await;
        reset(&pool).await;

        // Three always listed, so dropping two is not a >50% shrink.
        let everything = [
            unplanned("A"),
            unplanned("A2"),
            unplanned("A3"),
            unplanned("B"),
            planned("P"),
        ];
        assert_eq!(
            poll(&pool, &everything, true).await,
            Inference::NoBaseline,
            "the first complete snapshot only records its size"
        );

        let only_a = [unplanned("A"), unplanned("A2"), unplanned("A3")];
        let first_miss = poll(&pool, &only_a, true).await;
        assert!(
            matches!(first_miss, Inference::Applied { removed: 0, .. }),
            "{first_miss:?}"
        );
        assert_eq!(
            state(&pool, "B").await,
            (1, false),
            "one miss is not enough"
        );
        assert_eq!(state(&pool, "P").await, (1, false));
        assert_eq!(state(&pool, "A").await, (0, false));

        let second_miss = poll(&pool, &only_a, true).await;
        assert!(
            matches!(second_miss, Inference::Applied { removed, .. } if removed >= 2),
            "{second_miss:?}"
        );
        assert_eq!(state(&pool, "B").await, (2, true), "unplanned: ended");
        assert_eq!(
            state(&pool, "P").await,
            (2, true),
            "planned: ended the same way"
        );
        assert_eq!(state(&pool, "A").await, (0, false), "still listed: active");

        // Ended rows are not counted any further.
        poll(&pool, &only_a, true).await;
        assert_eq!(state(&pool, "B").await, (2, true));

        // The read side agrees.
        let row = queries::incident_by_id(&pool, &format!("{PREFIX}P"))
            .await
            .expect("lookup")
            .expect("exists");
        assert!(row.source_removed_at.is_some());
        assert!(!row.is_cleared, "ended is our observation, not RDM's clear");
        reset(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_removal::db_tests -- --ignored --test-threads=1`"]
    async fn incomplete_empty_shrunken_and_retried_snapshots_infer_nothing() {
        let pool = test_pool().await;
        reset(&pool).await;
        let five: Vec<IncidentMessage> = ["A", "B", "C", "D", "E"]
            .into_iter()
            .map(unplanned)
            .collect();
        assert_eq!(poll(&pool, &five, true).await, Inference::NoBaseline);

        // Incomplete (a skipped element, or an older poller's bare array):
        // never evidence, however often it repeats.
        for _ in 0..3 {
            assert_eq!(poll(&pool, &five[..3], false).await, Inference::Incomplete);
        }
        assert_eq!(state(&pool, "D").await, (0, false));
        assert_eq!(state(&pool, "E").await, (0, false));

        // Empty: an upstream outage looks exactly like this.
        assert_eq!(poll(&pool, &[], true).await, Inference::Empty);
        assert_eq!(state(&pool, "A").await, (0, false));

        // More than 50% smaller than the previous complete snapshot (2 of 5).
        assert_eq!(
            poll(&pool, &five[..2], true).await,
            Inference::Shrink {
                previous: 5,
                current: 2
            }
        );
        assert_eq!(state(&pool, "E").await, (0, false));

        // ...but it became the baseline, so a real purge only costs a poll.
        let applied = poll(&pool, &five[..2], true).await;
        assert!(matches!(applied, Inference::Applied { .. }), "{applied:?}");
        assert_eq!(state(&pool, "E").await, (1, false));

        // A retry of a POST that already committed (no time has passed):
        // must not count the same poll twice.
        assert_eq!(poll_now(&pool, &five[..2], true).await, Inference::TooSoon);
        assert_eq!(state(&pool, "E").await, (1, false));
        reset(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_removal::db_tests -- --ignored --test-threads=1`"]
    async fn reappearing_resets_the_count_and_un_ends() {
        let pool = test_pool().await;
        reset(&pool).await;
        let both = [unplanned("A"), planned("B")];
        let only_a = [unplanned("A")];
        poll(&pool, &both, true).await;

        // Missed once, back, missed once: never two CONSECUTIVE misses.
        poll(&pool, &only_a, true).await;
        assert_eq!(state(&pool, "B").await, (1, false));
        poll(&pool, &both, false).await; // listed again, even if incomplete
        assert_eq!(state(&pool, "B").await, (0, false));
        poll(&pool, &only_a, true).await;
        assert_eq!(state(&pool, "B").await, (1, false));

        // Ended, then listed again: active again at once.
        poll(&pool, &only_a, true).await;
        assert_eq!(state(&pool, "B").await, (2, true));
        poll(&pool, &both, true).await;
        assert_eq!(state(&pool, "B").await, (0, false));
        reset(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_removal::db_tests -- --ignored --test-threads=1`"]
    async fn a_cleared_incident_is_never_ended_and_its_clear_is_in_history() {
        let pool = test_pool().await;
        reset(&pool).await;
        let a = unplanned("A");
        let mut c = unplanned("C");
        poll(&pool, &[a.clone(), c.clone()], true).await;

        // RDM clears C by flipping the flag alone: a history row records it.
        c.is_cleared = true;
        poll(&pool, &[a.clone(), c.clone()], true).await;
        let history: Vec<bool> = sqlx::query_scalar(
            "SELECT is_cleared FROM incident_history WHERE incident_id = $1 \
             ORDER BY recorded_at, id",
        )
        .bind(&c.incident_id)
        .fetch_all(&pool)
        .await
        .expect("history");
        assert_eq!(history, vec![false, true], "the clear is its own snapshot");

        // Then C drops out of the feed: it stays cleared, never "ended".
        poll(&pool, std::slice::from_ref(&a), true).await;
        poll(&pool, std::slice::from_ref(&a), true).await;
        assert_eq!(state(&pool, "C").await, (0, false));
        reset(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_removal::db_tests -- --ignored --test-threads=1`"]
    async fn the_backfill_script_marks_only_long_unlisted_rows() {
        let pool = test_pool().await;
        reset(&pool).await;
        let path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("../../scripts/backfill-2026-10-06-incident-source-removed.sql");
        let script = std::fs::read_to_string(&path).expect("read the backfill script");
        let apply = script
            .split("-- BEGIN APPLY")
            .nth(1)
            .and_then(|rest| rest.split("-- END APPLY").next())
            .expect("the script has an APPLY block");

        // With no complete snapshot recorded yet, it matches nothing.
        poll(&pool, &[unplanned("OLD")], false).await;
        sqlx::query(
            "UPDATE incidents SET fetched_at = now() - interval '3 days' \
             WHERE incident_id = 'TEST-REMOVAL-OLD'",
        )
        .execute(&pool)
        .await
        .expect("age OLD");
        sqlx::raw_sql(apply).execute(&pool).await.expect("apply");
        assert_eq!(state(&pool, "OLD").await, (0, false));

        // After one: rows unlisted for days are ended; a row listed in that
        // snapshot, a cleared row and a row 10 minutes stale are not.
        let mut cleared = unplanned("CLEARED");
        cleared.is_cleared = true;
        poll(&pool, &[cleared], false).await;
        poll(&pool, &[unplanned("LISTED")], true).await;
        sqlx::query(
            "UPDATE incidents SET fetched_at = now() - interval '3 days' \
             WHERE incident_id IN ('TEST-REMOVAL-OLD', 'TEST-REMOVAL-CLEARED')",
        )
        .execute(&pool)
        .await
        .expect("age rows");
        poll(&pool, &[planned("OLD-PLANNED")], false).await;
        poll(&pool, &[unplanned("RECENT")], false).await;
        sqlx::raw_sql(
            "UPDATE incidents SET fetched_at = now() - interval '2 days' \
             WHERE incident_id = 'TEST-REMOVAL-OLD-PLANNED'; \
             UPDATE incidents SET fetched_at = now() - interval '10 minutes' \
             WHERE incident_id = 'TEST-REMOVAL-RECENT'",
        )
        .execute(&pool)
        .await
        .expect("age rows");

        sqlx::raw_sql(apply).execute(&pool).await.expect("apply");
        assert_eq!(state(&pool, "OLD").await, (2, true));
        assert_eq!(state(&pool, "OLD-PLANNED").await, (2, true));
        assert_eq!(state(&pool, "LISTED").await, (0, false));
        assert_eq!(state(&pool, "RECENT").await, (0, false));
        assert_eq!(state(&pool, "CLEARED").await, (0, false));

        // Idempotent, and undone by the next listing.
        sqlx::raw_sql(apply)
            .execute(&pool)
            .await
            .expect("apply again");
        assert_eq!(state(&pool, "OLD").await, (2, true));
        poll(&pool, &[unplanned("OLD")], false).await;
        assert_eq!(state(&pool, "OLD").await, (0, false));
        reset(&pool).await;
    }
}
