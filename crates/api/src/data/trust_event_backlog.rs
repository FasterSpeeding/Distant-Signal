// crates/api/src/data/trust_event_backlog.rs
//! Storage for `trust_event_backlog`
//! (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md). Write
//! side only -- Task 5 (`schedule_matching.rs` or a new sibling module)
//! owns the read/consumption side.

use common::TrustBacklogEventMessage;
use sqlx::PgPool;

/// Blind, at-least-once-safe batch insert -- `ON CONFLICT DO NOTHING` on
/// `dedup_key` (the same posture `train_movement_events` already uses for
/// the same reason: Redis Streams' own at-least-once delivery means a
/// redelivered batch after a crash-before-XACK is expected, not
/// exceptional). Returns how many rows this call actually inserted (for
/// the caller's own logging), not the batch length -- a redelivered batch
/// legitimately inserts 0.
pub async fn upsert_trust_event_backlog_batch(
    pool: &PgPool,
    events: &[TrustBacklogEventMessage],
) -> anyhow::Result<u64> {
    let mut inserted = 0u64;
    let mut tx = pool.begin().await?;
    for event in events {
        let result = sqlx::query(
            "INSERT INTO trust_event_backlog \
                (crs, train_uid, train_id, service_date, msg_type, event_type, \
                 planned_timestamp, actual_timestamp, variation_status, delay_minutes, dedup_key) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (dedup_key) DO NOTHING",
        )
        .bind(&event.crs)
        .bind(&event.train_uid)
        .bind(&event.train_id)
        .bind(event.service_date)
        .bind(&event.msg_type)
        .bind(&event.event_type)
        .bind(event.planned_timestamp)
        .bind(event.actual_timestamp)
        .bind(&event.variation_status)
        .bind(event.delay_minutes)
        .bind(&event.dedup_key)
        .execute(&mut *tx)
        .await?;
        inserted += result.rows_affected();
    }
    tx.commit().await?;
    Ok(inserted)
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

    fn fixture_event(train_id: &str, dedup_key: &str) -> TrustBacklogEventMessage {
        TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("C11052".to_string()),
            train_id: train_id.to_string(),
            service_date: "2026-09-05".parse().unwrap(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-05T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-05T19:16:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(1),
            dedup_key: dedup_key.to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                upsert_trust_event_backlog_batch -- --ignored`"]
    async fn a_fresh_batch_inserts_every_row() {
        let pool = connect().await;
        let events = vec![
            fixture_event("TEST-TRUST-BACKLOG-1", "test-dedup-key-1"),
            fixture_event("TEST-TRUST-BACKLOG-2", "test-dedup-key-2"),
        ];

        let inserted = upsert_trust_event_backlog_batch(&pool, &events)
            .await
            .expect("insert");
        assert_eq!(inserted, 2);

        sqlx::query("DELETE FROM trust_event_backlog WHERE dedup_key LIKE 'test-dedup-key-%'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_redelivered_batch_inserts_nothing_twice -- --ignored`"]
    async fn a_redelivered_batch_inserts_nothing_twice() {
        let pool = connect().await;
        let event = fixture_event("TEST-TRUST-BACKLOG-3", "test-dedup-key-3");

        let first = upsert_trust_event_backlog_batch(&pool, &[event.clone()])
            .await
            .expect("first insert");
        assert_eq!(first, 1);

        let redelivered = upsert_trust_event_backlog_batch(&pool, &[event])
            .await
            .expect("redelivered insert");
        assert_eq!(redelivered, 0, "same dedup_key must not insert twice");

        sqlx::query("DELETE FROM trust_event_backlog WHERE dedup_key = 'test-dedup-key-3'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
