//! Identity primitives for the shared `trains` table
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md).
//! `find_or_create_train` is the one idempotent upsert every dual-write
//! path in this plan funnels through -- Step B's own one-off backfill uses
//! the exact same `ON CONFLICT ... DO UPDATE ... RETURNING id` shape.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// Finds or creates the `trains` row for `(train_uid, service_date)`,
/// returning its surrogate `id`. `DO UPDATE` (never `DO NOTHING`) is what
/// makes `RETURNING id` reliable on a re-run against a row that already
/// exists -- safe to call more than once for the same identity.
pub async fn find_or_create_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO trains (train_uid, service_date) \
         VALUES ($1, $2) \
         ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid \
         RETURNING id",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Same idempotent shape as [`find_or_create_train`], but also mirrors a
/// successful schedule match's own result onto the shared row (Step A's
/// "attempt_schedule_match... mirror their result onto the shared trains
/// row too"). `COALESCE`d against the existing value on every schedule
/// column so a second subscriber's independent match against the same
/// physical train never clobbers data an earlier one already wrote.
#[allow(clippy::too_many_arguments)]
pub async fn find_or_create_train_with_schedule_match(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
    origin_crs: &str,
    scheduled_departure: DateTime<Utc>,
    destination_crs: Option<&str>,
    matched_line_id: &str,
    calling_points: &serde_json::Value,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO trains \
            (train_uid, service_date, origin_crs, scheduled_departure, destination_crs, \
             matched_line_id, calling_points, schedule_matched_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) \
         ON CONFLICT (train_uid, service_date) DO UPDATE SET \
            train_uid            = EXCLUDED.train_uid, \
            origin_crs           = COALESCE(trains.origin_crs, EXCLUDED.origin_crs), \
            scheduled_departure  = COALESCE(trains.scheduled_departure, EXCLUDED.scheduled_departure), \
            destination_crs      = COALESCE(trains.destination_crs, EXCLUDED.destination_crs), \
            matched_line_id      = COALESCE(trains.matched_line_id, EXCLUDED.matched_line_id), \
            calling_points       = COALESCE(trains.calling_points, EXCLUDED.calling_points), \
            schedule_matched_at  = COALESCE(trains.schedule_matched_at, EXCLUDED.schedule_matched_at) \
         RETURNING id",
    )
    .bind(train_uid)
    .bind(service_date)
    .bind(origin_crs)
    .bind(scheduled_departure)
    .bind(destination_crs)
    .bind(matched_line_id)
    .bind(calling_points)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Mirrors a live-TRUST resolution onto the shared row's own
/// Live-TRUST-derived columns (`train_id`, `resolved_at`) -- never
/// clobbers `train_uid`/schedule columns, which this function doesn't
/// touch at all. Safe to call more than once for the same `trains_id`
/// (a later Movement re-supplying the same `train_id` is a harmless
/// no-op overwrite of identical values).
pub async fn mark_train_resolved(
    pool: &PgPool,
    trains_id: i64,
    train_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE trains SET train_id = $2, resolved_at = NOW() WHERE id = $1")
        .bind(trains_id)
        .bind(train_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Step B's one-off backfill job: for every `tracked_trains` row with
/// `train_uid IS NOT NULL AND trains_id IS NULL`, finds-or-creates the
/// matching `trains` row and links it. Batches 500 rows at a time until
/// none remain. Safe to call more than once -- a later call against
/// already-backfilled data selects zero rows on its very first batch and
/// returns immediately without touching `trains` or `tracked_trains` again.
pub async fn backfill_trains_id_for_resolved_rows(pool: &PgPool) -> anyhow::Result<()> {
    loop {
        let rows: Vec<(i64, String, NaiveDate)> = sqlx::query_as(
            "SELECT id, train_uid, service_date FROM tracked_trains \
             WHERE train_uid IS NOT NULL AND trains_id IS NULL \
             LIMIT 500",
        )
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }
        for (id, train_uid, row_service_date) in &rows {
            let trains_id = find_or_create_train(pool, train_uid, *row_service_date).await?;
            sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
                .bind(id)
                .bind(trains_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// The public, unscoped read-model for `GET /Train/by-uid/{uid}/{date}`
/// (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §4).
/// Unlike `train_tracking::TrackedTrainState` (which this route used to
/// return), this carries no `custom_name`/pin fields at all -- those are
/// per-subscriber private data with no place on a shared, public row.
/// Darwin ETA blending (`crate::data::eta_blend`, wired into the legacy
/// `TrackedTrainState` read paths) is deliberately NOT applied here --
/// that helper operates on `TrackedTrainState`'s own pin-shaped input;
/// wiring it into this new public shape is left as a fast-follow, not
/// part of this task's own scope (only the ownership-check removal).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicTrainState {
    pub id: i64,
    pub train_uid: String,
    pub service_date: NaiveDate,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub calling_points: Option<serde_json::Value>,
    pub train_id: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
}

/// Public, unscoped read for `(train_uid, service_date)` -- no
/// `AuthenticatedUser`/ownership check anywhere in this call path. Reads
/// `trains` directly (joined with the re-pointed `train_current_state` via
/// `trains_id`, Task 9/11/14), never touches `tracked_trains` at all, so
/// there is no code path here that could reach a subscriber's own
/// `custom_name`/ticket/notification data.
pub async fn get_public_train_state(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<PublicTrainState>> {
    let row = sqlx::query_as::<_, PublicTrainState>(
        "SELECT tr.id, tr.train_uid, tr.service_date, tr.origin_crs, so.name AS origin_name, \
                tr.destination_crs, sd.name AS destination_name, tr.scheduled_departure, \
                tr.calling_points, tr.train_id, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs) \
         WHERE tr.train_uid = $1 AND tr.service_date = $2",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_or_create_train_returns_the_same_id_on_a_repeat_call -- --ignored"]
    async fn find_or_create_train_returns_the_same_id_on_a_repeat_call() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let first = find_or_create_train(&pool, "TEST-TRAINS-UID-1", service_date)
            .await
            .expect("first find_or_create_train");
        let second = find_or_create_train(&pool, "TEST-TRAINS-UID-1", service_date)
            .await
            .expect("second find_or_create_train");
        assert_eq!(first, second, "the same (train_uid, service_date) must resolve to one row");

        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-UID-1'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                mark_train_resolved_sets_train_id_and_resolved_at -- --ignored"]
    async fn mark_train_resolved_sets_train_id_and_resolved_at() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = find_or_create_train(&pool, "TEST-TRAINS-UID-2", service_date)
            .await
            .expect("find_or_create_train");

        mark_train_resolved(&pool, trains_id, "221832406")
            .await
            .expect("mark_train_resolved");

        let (train_id, resolved_at): (Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as("SELECT train_id, resolved_at FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains row");
        assert_eq!(train_id, Some("221832406".to_string()));
        assert!(resolved_at.is_some());

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                find_or_create_train_with_schedule_match_never_clobbers_an_earlier_match -- --ignored"]
    async fn find_or_create_train_with_schedule_match_never_clobbers_an_earlier_match() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-06T12:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["PAD", "RDG"]);

        let first_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-TRAINS-UID-3",
            service_date,
            "PAD",
            scheduled_departure,
            None,
            "line-a",
            &calling_points,
        )
        .await
        .expect("first find_or_create_train_with_schedule_match");

        let second_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-TRAINS-UID-3",
            service_date,
            "ZZZ",
            scheduled_departure,
            None,
            "line-b",
            &calling_points,
        )
        .await
        .expect("second find_or_create_train_with_schedule_match");

        assert_eq!(
            first_id, second_id,
            "the same (train_uid, service_date) must resolve to one row"
        );

        let (origin_crs, matched_line_id): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT origin_crs, matched_line_id FROM trains WHERE id = $1",
        )
        .bind(first_id)
        .fetch_one(&pool)
        .await
        .expect("read back trains row");
        assert_eq!(
            origin_crs,
            Some("PAD".to_string()),
            "a later independent match must not clobber the first match's origin_crs"
        );
        assert_eq!(
            matched_line_id,
            Some("line-a".to_string()),
            "a later independent match must not clobber the first match's matched_line_id"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(first_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "one-off Step B production backfill job, not a repeatable unit test; \
                run manually, exactly once per environment, with \
                `DATABASE_URL=... cargo test -p api run_step_b_backfill_of_existing_resolved_rows \
                -- --ignored --test-threads=1 --nocapture` -- see Task 6 for the pre-run diagnostic"]
    async fn run_step_b_backfill_of_existing_resolved_rows() {
        let pool = connect().await;
        let user_id = "TEST-STEP-B-BACKFILL";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("step-b-backfill@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        // A resolved row exactly as Tasks 3-5 would have left one BEFORE this
        // plan's dual-write landed -- train_uid set, trains_id still NULL.
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, \
                 train_uid, train_id, resolution_status) \
             VALUES ($1, $2, 'EUS', $3, 'TEST-STEP-B-UID', 'TEST-STEP-B-TRAIN-ID', 'resolved') \
             RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(service_date.and_hms_opt(19, 15, 0).unwrap().and_utc())
        .fetch_one(&pool)
        .await
        .expect("seed a pre-existing resolved row with no trains_id yet");

        let (trains_id_before,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        assert_eq!(trains_id_before, None, "precondition: not yet backfilled");

        // --- Run 1: the backfill job against genuinely unbackfilled data ---
        backfill_trains_id_for_resolved_rows(&pool)
            .await
            .expect("first backfill run");

        let (trains_id_after_first_run,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id after first run");
        assert!(
            trains_id_after_first_run.is_some(),
            "the row must now point at a trains row"
        );

        let (matched_trains_id,): (i64,) = sqlx::query_as(
            "SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2",
        )
        .bind("TEST-STEP-B-UID")
        .bind(service_date)
        .fetch_one(&pool)
        .await
        .expect("exactly one trains row must exist for this identity after the first run");
        assert_eq!(trains_id_after_first_run, Some(matched_trains_id));

        // --- Run 2: re-run the SAME job, in the SAME test, with NO cleanup
        // or reset in between -- this is what actually proves the job's own
        // `SELECT ... WHERE trains_id IS NULL` + `UPDATE` loop is safe to
        // run again against data it already backfilled. (Merely re-calling
        // `find_or_create_train` directly, as the old version of this test
        // did, only re-proves Task 2's upsert idempotency -- it never
        // exercises this job's own selection query a second time.)
        backfill_trains_id_for_resolved_rows(&pool)
            .await
            .expect("second backfill run, against already-backfilled data");

        let (trains_id_after_second_run,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id after second run");
        assert_eq!(
            trains_id_after_second_run, trains_id_after_first_run,
            "re-running the backfill job against already-backfilled data must leave trains_id \
             pointing at the exact same trains row"
        );

        let (trains_row_count_after_second_run,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM trains WHERE train_uid = $1 AND service_date = $2",
        )
        .bind("TEST-STEP-B-UID")
        .bind(service_date)
        .fetch_one(&pool)
        .await
        .expect("count trains rows for this identity after the second run");
        assert_eq!(
            trains_row_count_after_second_run, 1,
            "re-running the backfill job must not create a duplicate trains row for the same \
             (train_uid, service_date) identity"
        );

        sqlx::query("DELETE FROM tracked_trains WHERE id = $1")
            .bind(tracked_train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-STEP-B-UID'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_state_returns_none_for_no_matching_row -- --ignored"]
    async fn get_public_train_state_returns_none_for_no_matching_row() {
        let pool = connect().await;
        let result = get_public_train_state(&pool, "NOSUCHUID", "2026-09-06".parse().unwrap())
            .await
            .expect("get_public_train_state");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_state_reads_the_shared_row_and_its_current_state -- --ignored"]
    async fn get_public_train_state_reads_the_shared_row_and_its_current_state() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-06T12:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "MKC"]);

        let trains_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-PUBLIC-STATE-UID",
            service_date,
            "EUS",
            scheduled_departure,
            Some("MKC"),
            "line-a",
            &calling_points,
        )
        .await
        .expect("seed a trains row via schedule match");
        mark_train_resolved(&pool, trains_id, "1A23")
            .await
            .expect("mark_train_resolved");

        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'Watford Junction', 'DEPARTURE', 4, 'MKC', NOW())",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        let state = get_public_train_state(&pool, "TEST-PUBLIC-STATE-UID", service_date)
            .await
            .expect("get_public_train_state")
            .expect("row should be found");

        assert_eq!(state.id, trains_id);
        assert_eq!(state.train_uid, "TEST-PUBLIC-STATE-UID");
        assert_eq!(state.origin_crs, Some("EUS".to_string()));
        assert_eq!(state.destination_crs, Some("MKC".to_string()));
        assert_eq!(state.train_id, Some("1A23".to_string()));
        assert_eq!(state.status, Some("en_route".to_string()));
        assert_eq!(
            state.last_reported_location,
            Some("Watford Junction".to_string())
        );
        assert_eq!(state.delay_minutes, Some(4));
        assert_eq!(state.next_calling_point, Some("MKC".to_string()));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
