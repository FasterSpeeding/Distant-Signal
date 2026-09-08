//! Periodic reconciliation for tracked-train state that a one-shot,
//! in-process-only attempt failed to advance. See
//! docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md.

use sqlx::PgPool;

/// Stall 1's fix: a `train_subscriptions` row can stay `'pending'` forever
/// even after `train_movement_events` proves the train it points at was
/// genuinely tracked -- see the design doc's §1.1 for the two independent,
/// confirmed ways this happens (a redeployed `trust-consumer` that never
/// observed the Activation in-process, and a partial-batch-post-failure
/// race in the multi-subscriber fan-out). Writes only `resolution_status`,
/// replicating `train_tracking::flip_legacy_resolution`'s own
/// post-Task-22 invariant exactly -- every other column that function used
/// to also write now lives on the shared `trains` row and is already
/// correctly set by whichever subscriber's write succeeded first, since
/// `mark_train_resolved` operates per-`trains_id`, not per-subscription.
pub async fn reconcile_stuck_resolution_status(pool: &PgPool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "UPDATE train_subscriptions \
         SET resolution_status = 'resolved' \
         WHERE resolution_status = 'pending' \
           AND trains_id IS NOT NULL \
           AND EXISTS ( \
               SELECT 1 FROM train_movement_events tme \
               WHERE tme.trains_id = train_subscriptions.trains_id \
           )",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
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

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    async fn seed_train(pool: &PgPool, train_uid: &str, service_date: chrono::NaiveDate) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2) RETURNING id",
        )
        .bind(train_uid)
        .bind(service_date)
        .fetch_one(pool)
        .await
        .expect("seed fixture trains row");
        id
    }

    async fn seed_pending_subscription(
        pool: &PgPool,
        user_id: &str,
        service_date: chrono::NaiveDate,
        trains_id: i64,
    ) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions (user_id, service_date, trains_id, resolution_status) \
             VALUES ($1, $2, $3, 'pending') RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(trains_id)
        .fetch_one(pool)
        .await
        .expect("seed fixture pending subscription");
        id
    }

    async fn seed_movement_event(pool: &PgPool, trains_id: i64, dedup_key: &str) {
        sqlx::query(
            "INSERT INTO train_movement_events (trains_id, dedup_key, msg_type, raw_body) \
             VALUES ($1, $2, '0003', '{}'::jsonb)",
        )
        .bind(trains_id)
        .bind(dedup_key)
        .execute(pool)
        .await
        .expect("seed fixture movement event");
    }

    async fn read_resolution_status(pool: &PgPool, id: i64) -> String {
        let status: String =
            sqlx::query_scalar("SELECT resolution_status FROM train_subscriptions WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .expect("read back resolution_status");
        status
    }

    async fn cleanup(pool: &PgPool, user_id: &str, train_uid: &str) {
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                reconcile_stuck_resolution_status_flips_a_pending_row_with_movement_events_to_resolved \
                -- --ignored`"]
    async fn reconcile_stuck_resolution_status_flips_a_pending_row_with_movement_events_to_resolved()
    {
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL1-A";
        let train_uid = "TEST-RECON-STALL1-UID-A";
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        seed_user(&pool, user_id).await;
        let trains_id = seed_train(&pool, train_uid, service_date).await;
        let subscription_id = seed_pending_subscription(&pool, user_id, service_date, trains_id).await;
        seed_movement_event(&pool, trains_id, "TEST-RECON-DEDUP-A").await;

        let flipped = reconcile_stuck_resolution_status(&pool)
            .await
            .expect("reconcile_stuck_resolution_status");
        assert!(flipped >= 1, "at least this fixture's row must be counted");

        let status = read_resolution_status(&pool, subscription_id).await;
        assert_eq!(
            status, "resolved",
            "movement events exist for this trains_id, so the stuck subscription must be resolved"
        );

        cleanup(&pool, user_id, train_uid).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                reconcile_stuck_resolution_status_leaves_a_pending_row_with_no_movement_events_untouched \
                -- --ignored`"]
    async fn reconcile_stuck_resolution_status_leaves_a_pending_row_with_no_movement_events_untouched()
    {
        // The L78659 live-example variant: a genuinely missed Activation,
        // zero train_movement_events rows. This must NOT be flipped --
        // there is no honest evidence the train ever ran (design doc
        // Decision 1).
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL1-B";
        let train_uid = "TEST-RECON-STALL1-UID-B";
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        seed_user(&pool, user_id).await;
        let trains_id = seed_train(&pool, train_uid, service_date).await;
        let subscription_id = seed_pending_subscription(&pool, user_id, service_date, trains_id).await;

        reconcile_stuck_resolution_status(&pool)
            .await
            .expect("reconcile_stuck_resolution_status");

        let status = read_resolution_status(&pool, subscription_id).await;
        assert_eq!(
            status, "pending",
            "no movement events exist for this trains_id, so the row must stay pending"
        );

        cleanup(&pool, user_id, train_uid).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                reconcile_stuck_resolution_status_never_touches_a_row_that_isnt_pending \
                -- --ignored`"]
    async fn reconcile_stuck_resolution_status_never_touches_a_row_that_isnt_pending() {
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL1-C";
        let train_uid = "TEST-RECON-STALL1-UID-C";
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        seed_user(&pool, user_id).await;
        let trains_id = seed_train(&pool, train_uid, service_date).await;
        let (subscription_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions (user_id, service_date, trains_id, resolution_status) \
             VALUES ($1, $2, $3, 'schedule_matched') RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed fixture schedule_matched subscription");
        seed_movement_event(&pool, trains_id, "TEST-RECON-DEDUP-C").await;

        reconcile_stuck_resolution_status(&pool)
            .await
            .expect("reconcile_stuck_resolution_status");

        let status = read_resolution_status(&pool, subscription_id).await;
        assert_eq!(
            status, "schedule_matched",
            "only a 'pending' row may be reconciled by this function"
        );

        cleanup(&pool, user_id, train_uid).await;
    }
}
