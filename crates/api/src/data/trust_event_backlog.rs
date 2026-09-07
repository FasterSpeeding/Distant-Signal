// crates/api/src/data/trust_event_backlog.rs
//! Storage for `trust_event_backlog`
//! (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md). Write
//! side only -- Task 5 (`schedule_matching.rs` or a new sibling module)
//! owns the read/consumption side.

use common::TrustBacklogEventMessage;
use sqlx::PgPool;
use trust_schema::journey::{self, DerivedState};
use trust_schema::schema::Movement;

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

/// Mirrors one `TrustBacklogEventMessage` onto the shared `trains`/
/// `train_movement_events`/`train_current_state` tables -- the concrete
/// wiring behind "trust-backlog-consumer becomes the primary movement-event
/// writer" (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
/// §3). A deliberate no-op, not an error, whenever `event.train_uid` is
/// `None` -- this process never saw the Activation for this train_id
/// (Task 13's own named, accepted gap), so there is no natural key to
/// create or find a `trains` row by. Independent of, and additional to,
/// this same batch's existing `upsert_trust_event_backlog_batch` write --
/// `trust_event_backlog` itself is a separate, parallel system (this
/// plan's Global Constraints).
pub async fn ingest_shared_movement(
    pool: &PgPool,
    event: &TrustBacklogEventMessage,
) -> anyhow::Result<()> {
    let Some(train_uid) = &event.train_uid else {
        return Ok(());
    };

    let trains_id =
        crate::data::trains::find_or_create_train(pool, train_uid, event.service_date).await?;
    crate::data::trains::mark_train_resolved(pool, trains_id, &event.train_id).await?;

    let previous = fetch_previous_derived_state(pool, trains_id).await?;
    let derived = match event.msg_type.as_str() {
        "0003" => {
            let movement = Movement {
                train_id: event.train_id.clone(),
                event_type: event.event_type.clone().unwrap_or_default(),
                gbtt_timestamp: None,
                planned_timestamp: event
                    .planned_timestamp
                    .map(|t| t.timestamp_millis().to_string()),
                actual_timestamp: event
                    .actual_timestamp
                    .map(|t| t.timestamp_millis().to_string()),
                reporting_stanox: None,
                loc_stanox: None,
                toc_id: None,
                variation_status: event.variation_status.clone(),
            };
            let mut derived = journey::apply_movement(&previous, &movement, event.crs.as_deref());
            if let (Some(p), Some(a), Some("LATE")) = (
                event.planned_timestamp,
                event.actual_timestamp,
                event.variation_status.as_deref(),
            ) {
                derived.delay_minutes = Some((a - p).num_minutes() as i32);
            }
            derived
        }
        "0002" => journey::apply_cancellation(&previous),
        // "0001" (Activation) carries no derivable state of its own --
        // find_or_create_train/mark_train_resolved above already did
        // everything an Activation contributes to the shared row.
        _ => return Ok(()),
    };

    let movement_event = common::TrainMovementEventMessage {
        tracked_train_id: 0, // unused by upsert_train_movement -- see its own doc comment
        resolved_train_uid: None,
        resolved_train_id: None,
        dedup_key: event.dedup_key.clone(),
        msg_type: event.msg_type.clone(),
        event_type: event.event_type.clone(),
        loc_stanox: None,
        loc_crs: event.crs.clone(),
        planned_timestamp: event.planned_timestamp,
        actual_timestamp: event.actual_timestamp,
        variation_status: event.variation_status.clone(),
        raw_body: serde_json::json!({}),
        status: derived.status,
        last_reported_location: derived.last_reported_location,
        last_event_type: derived.last_event_type,
        delay_minutes: derived.delay_minutes,
        next_calling_point: derived.next_calling_point,
        eta_next: None,
        eta_source: None,
    };
    crate::data::train_tracking::upsert_train_movement(pool, trains_id, &movement_event).await
}

async fn fetch_previous_derived_state(
    pool: &PgPool,
    trains_id: i64,
) -> anyhow::Result<DerivedState> {
    let row: Option<(
        String,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT status, last_reported_location, last_event_type, delay_minutes, next_calling_point \
             FROM train_current_state WHERE trains_id = $1",
    )
    .bind(trains_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((
            status,
            last_reported_location,
            last_event_type,
            delay_minutes,
            next_calling_point,
        )) => DerivedState {
            status,
            last_reported_location,
            last_event_type,
            delay_minutes,
            next_calling_point,
        },
        None => DerivedState::awaiting_activation(),
    })
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

        let first = upsert_trust_event_backlog_batch(&pool, std::slice::from_ref(&event))
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movement_writes_the_shared_tables_when_train_uid_is_known \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movement_writes_the_shared_tables_when_train_uid_is_known() {
        let pool = connect().await;
        let event = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: Some("TEST-INGEST-UID".to_string()),
            train_id: "TEST-INGEST-TRAIN-ID".to_string(),
            service_date: "2026-09-06".parse().unwrap(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:16:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(1),
            dedup_key: "test-ingest-shared-dedup".to_string(),
        };

        ingest_shared_movement(&pool, &event)
            .await
            .expect("ingest_shared_movement");

        let (trains_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-INGEST-UID'")
                .fetch_one(&pool)
                .await
                .expect("a shared trains row must have been created");

        let (status, delay_minutes): (String, Option<i32>) = sqlx::query_as(
            "SELECT status, delay_minutes FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("a current-state row must exist");
        assert_eq!(status, "en_route");
        assert_eq!(delay_minutes, Some(1));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movement_is_a_no_op_with_no_known_train_uid -- --ignored`"]
    async fn ingest_shared_movement_is_a_no_op_with_no_known_train_uid() {
        let pool = connect().await;
        let event = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: None, // the accepted gap -- this process never saw the Activation
            train_id: "TEST-INGEST-NO-UID-TRAIN-ID".to_string(),
            service_date: "2026-09-06".parse().unwrap(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-ingest-shared-no-uid-dedup".to_string(),
        };
        ingest_shared_movement(&pool, &event)
            .await
            .expect("must not error, just no-op");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM trains WHERE train_id = 'TEST-INGEST-NO-UID-TRAIN-ID'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(
            count, 0,
            "no trains row should be created with no known train_uid"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movement_twice_with_the_same_event_writes_exactly_one_row_each \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movement_twice_with_the_same_event_writes_exactly_one_row_each() {
        // Same real code path (ingest_shared_movement) called twice against
        // non-reset state -- not a duplicated inline copy, not a reset in
        // between -- proving the composed find_or_create_train (ON CONFLICT
        // DO UPDATE ... RETURNING id), mark_train_resolved (plain overwrite
        // UPDATE), and upsert_train_movement (ON CONFLICT DO NOTHING /
        // DO UPDATE) are genuinely safe against Redis Streams' at-least-once
        // redelivery of the same trust-backlog batch. Tasks 7 and 10 both had
        // to be fixed in review because their "idempotency" tests didn't
        // actually re-invoke the same code path against non-reset state --
        // this test exists specifically so that mistake can't recur silently
        // here.
        let pool = connect().await;
        let event = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: Some("TEST-INGEST-TWICE-UID".to_string()),
            train_id: "TEST-INGEST-TWICE-TRAIN-ID".to_string(),
            service_date: "2026-09-06".parse().unwrap(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:16:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(1),
            dedup_key: "test-ingest-shared-twice-dedup".to_string(),
        };

        ingest_shared_movement(&pool, &event)
            .await
            .expect("first ingest_shared_movement");
        ingest_shared_movement(&pool, &event)
            .await
            .expect("second ingest_shared_movement, same event, non-reset state");

        let trains_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM trains WHERE train_uid = 'TEST-INGEST-TWICE-UID'",
        )
        .fetch_one(&pool)
        .await
        .expect("count trains");
        assert_eq!(
            trains_count, 1,
            "exactly one trains row after two identical calls"
        );

        let (trains_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-INGEST-TWICE-UID'")
                .fetch_one(&pool)
                .await
                .expect("trains id");

        let movement_events_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count train_movement_events");
        assert_eq!(
            movement_events_count, 1,
            "exactly one train_movement_events row after two identical calls"
        );

        let current_state_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count train_current_state");
        assert_eq!(
            current_state_count, 1,
            "exactly one train_current_state row after two identical calls"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
