//! Identity primitives for the shared `trains` table
//! (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md).
//! `find_or_create_train` is the one idempotent upsert every dual-write
//! path in this plan funnels through -- Step B's own one-off backfill uses
//! the exact same `ON CONFLICT ... DO UPDATE ... RETURNING id` shape.

use chrono::{DateTime, NaiveDate, Utc};
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

        // --- the backfill job itself, run inline in this test ---
        loop {
            let rows: Vec<(i64, String, chrono::NaiveDate)> = sqlx::query_as(
                "SELECT id, train_uid, service_date FROM tracked_trains \
                 WHERE train_uid IS NOT NULL AND trains_id IS NULL \
                 LIMIT 500",
            )
            .fetch_all(&pool)
            .await
            .expect("select backfill batch");
            if rows.is_empty() {
                break;
            }
            for (id, train_uid, row_service_date) in &rows {
                let trains_id = find_or_create_train(&pool, train_uid, *row_service_date)
                    .await
                    .expect("find_or_create_train");
                sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
                    .bind(id)
                    .bind(trains_id)
                    .execute(&pool)
                    .await
                    .expect("set trains_id");
            }
        }

        let (trains_id_after,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        assert!(trains_id_after.is_some(), "the row must now point at a trains row");

        // Re-running the whole loop must be a no-op -- proves idempotency.
        let trains_id_second_run = find_or_create_train(&pool, "TEST-STEP-B-UID", service_date)
            .await
            .expect("re-run find_or_create_train");
        assert_eq!(Some(trains_id_second_run), trains_id_after);

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
}
