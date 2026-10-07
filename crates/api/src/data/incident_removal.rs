//! "Ended (no longer listed)": inferring that an incident has left RDM's
//! Knowledgebase feed without RDM ever clearing it. The inference itself
//! lives in `ds_store::incidents::removal` (see its module docs for the
//! guard); this file keeps the shim and the DB tests that drive it through
//! the api's write path and read it back through the api's readers
//! (`queries::incident_by_id` stays in the api, spec §5.2).

// Moved to ds_store::incidents::removal (ingest architecture plan 1A.8)
pub use ds_store::incidents::removal::{
    INFERENCE_METRIC, Inference, MARKED_REMOVED_METRIC, MIN_INFERENCE_GAP_SECS, MISSES_TO_REMOVE,
    infer_removals, register_metrics,
};

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
    use sqlx::PgPool;
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
