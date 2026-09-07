// crates/api/src/data/trust_event_backlog.rs
//! Storage for `trust_event_backlog`
//! (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md). Write
//! side only -- Task 5 (`schedule_matching.rs` or a new sibling module)
//! owns the read/consumption side.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
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
    // Thin wrapper over the batch-shaped implementation, called with a
    // batch of one -- see `ingest_shared_movements_batch`'s own doc
    // comment for why that function exists and how it preserves this
    // function's exact single-event behaviour. `results` always has
    // exactly one entry: `ingest_shared_movements_batch` never shrinks or
    // grows its output relative to its input.
    let mut results = ingest_shared_movements_batch(pool, std::slice::from_ref(event)).await;
    results.remove(0)
}

/// Batch-shaped sibling of [`ingest_shared_movement`] -- same per-event
/// behaviour (including the `event.train_uid == None` no-op documented on
/// that function), but collapses the once-per-event
/// `find_or_create_train`/`mark_train_resolved`/`fetch_previous_derived_state`
/// round trips into ONE multi-row query each for the WHOLE incoming
/// `events` slice, rather than one query per event. For `N` events over
/// `M` distinct `(train_uid, service_date)` pairs, the old per-event loop
/// cost `5*N` sequential round trips (`find_or_create_train`,
/// `mark_train_resolved`, `fetch_previous_derived_state`, and
/// `upsert_train_movement`'s own two writes -- all five, every event, even
/// when M events share an identity and each hits the SAME upsert target
/// again); this costs at most `3 + 2*N` in the common, fully-successful
/// case: 3 batched round trips total for identity resolution and the
/// previous-state fetch (regardless of N or M), plus the two final writes
/// (`train_movement_events`/`train_current_state`) still done once per
/// event, deliberately -- see below.
///
/// Returns one `Result` per input event, same length and order as
/// `events`. This is what lets `post_trust_event_backlog` keep its
/// existing "one bad event doesn't kill the whole batch" contract: it
/// iterates this `Vec` exactly the way it used to iterate the result of
/// calling `ingest_shared_movement` once per event in a loop, logging and
/// skipping any `Err` without aborting the rest.
///
/// The one place collapsing these round trips changes that isolation
/// story: the two batched *write* queries
/// (`trains::find_or_create_trains_batch`, `trains::mark_trains_resolved_batch`)
/// are each ONE SQL statement covering every event's identity in this
/// batch, so a single malformed row (a `train_uid` violating some column
/// constraint, say) can fail that whole statement, where the old per-event
/// `INSERT`/`UPDATE` would only ever have failed for that one event. Both
/// calls are wrapped in a fallback here: on error, this function retries
/// that step the OLD way, one row at a time, so a genuinely bad row still
/// only fails the event(s) that reference it -- full isolation is
/// preserved, just at the cost of the old per-event round trips again, and
/// only on this already-abnormal path. The final two writes are left
/// per-event exactly as before (via [`crate::data::train_tracking::upsert_train_movement`]),
/// since isolation was never at risk there to begin with.
///
/// Intra-batch causality for `fetch_previous_derived_state` is preserved
/// too, not merely approximated: the one batched SELECT only seeds an
/// in-memory map with each distinct `trains_id`'s state as it exists in
/// the database BEFORE this call; as events are then processed one at a
/// time in their original order, each event's derived state is both read
/// from AND written back into that same in-memory map before the next
/// event is considered -- so a second event in this batch for the same
/// `trains_id` sees the FIRST event's just-computed state, exactly as it
/// would if `fetch_previous_derived_state` had been re-run against the
/// database after the first event's write (which is what the old
/// sequential loop actually did).
pub async fn ingest_shared_movements_batch(
    pool: &PgPool,
    events: &[TrustBacklogEventMessage],
) -> Vec<anyhow::Result<()>> {
    let mut results: Vec<anyhow::Result<()>> = (0..events.len()).map(|_| Ok(())).collect();

    let known_indices: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.train_uid.as_ref().map(|_| i))
        .collect();
    if known_indices.is_empty() {
        return results;
    }

    // Step 1: find_or_create_train, batched across every DISTINCT
    // (train_uid, service_date) pair this POST's batch actually needs.
    let mut pair_order: Vec<(String, NaiveDate)> = Vec::new();
    let mut seen_pairs: HashSet<(String, NaiveDate)> = HashSet::new();
    for &i in &known_indices {
        let key = (
            events[i]
                .train_uid
                .clone()
                .expect("known_indices only contains Some(train_uid) events"),
            events[i].service_date,
        );
        if seen_pairs.insert(key.clone()) {
            pair_order.push(key);
        }
    }

    let id_map = match crate::data::trains::find_or_create_trains_batch(pool, &pair_order).await {
        Ok(map) => map,
        Err(_) => {
            // Fallback: the batched INSERT failed outright (e.g. one bad
            // row in an otherwise-fine batch) -- retry one pair at a time,
            // the old way, so only the pair(s) that actually fail take
            // down the event(s) that reference them.
            let mut map = HashMap::new();
            for pair in &pair_order {
                match crate::data::trains::find_or_create_train(pool, &pair.0, pair.1).await {
                    Ok(id) => {
                        map.insert(pair.clone(), id);
                    }
                    Err(err) => {
                        let message = format!("find_or_create_train failed: {err}");
                        for &i in &known_indices {
                            if events[i].train_uid.as_deref() == Some(pair.0.as_str())
                                && events[i].service_date == pair.1
                            {
                                results[i] = Err(anyhow::anyhow!("{message}"));
                            }
                        }
                    }
                }
            }
            map
        }
    };

    // Every known-train_uid event whose identity resolved to a trains_id.
    let mut resolved: Vec<(usize, i64)> = Vec::new();
    for &i in &known_indices {
        if results[i].is_err() {
            continue;
        }
        let key = (
            events[i]
                .train_uid
                .clone()
                .expect("known_indices only contains Some(train_uid) events"),
            events[i].service_date,
        );
        match id_map.get(&key) {
            Some(&id) => resolved.push((i, id)),
            None => {
                results[i] = Err(anyhow::anyhow!(
                    "find_or_create_train did not resolve an id for this event's identity"
                ));
            }
        }
    }

    // Step 2: mark_train_resolved, batched -- deduped to one row per
    // trains_id, keeping the LAST event's train_id in batch order (the
    // same final value the old sequential-overwrite loop would leave,
    // since each call plainly overwrites the column).
    let mut resolve_map: HashMap<i64, String> = HashMap::new();
    for &(i, trains_id) in &resolved {
        resolve_map.insert(trains_id, events[i].train_id.clone());
    }
    let resolve_pairs: Vec<(i64, String)> = resolve_map.into_iter().collect();
    if crate::data::trains::mark_trains_resolved_batch(pool, &resolve_pairs)
        .await
        .is_err()
    {
        // Fallback: same reasoning as Step 1's -- retry one pair at a
        // time so a single bad row doesn't fail every event in the batch.
        for (trains_id, train_id) in &resolve_pairs {
            if let Err(err) =
                crate::data::trains::mark_train_resolved(pool, *trains_id, train_id).await
            {
                let message = format!("mark_train_resolved failed: {err}");
                for &(i, tid) in &resolved {
                    if tid == *trains_id {
                        results[i] = Err(anyhow::anyhow!("{message}"));
                    }
                }
            }
        }
    }

    // Only events whose identity resolution AND resolution-mark both
    // succeeded continue past this point -- matching the original
    // `find_or_create_train(...).await?; mark_train_resolved(...).await?;`
    // early-return-on-error shape.
    let active: Vec<(usize, i64)> = resolved
        .into_iter()
        .filter(|&(i, _)| results[i].is_ok())
        .collect();
    if active.is_empty() {
        return results;
    }

    // Step 3: fetch_previous_derived_state, batched into one SELECT
    // covering every DISTINCT trains_id this batch will touch.
    let mut distinct_trains_ids: Vec<i64> = Vec::new();
    let mut seen_ids: HashSet<i64> = HashSet::new();
    for &(_, trains_id) in &active {
        if seen_ids.insert(trains_id) {
            distinct_trains_ids.push(trains_id);
        }
    }
    let mut state_map = match fetch_previous_derived_states_batch(pool, &distinct_trains_ids).await
    {
        Ok(map) => map,
        Err(_) => {
            // Fallback: same reasoning as Steps 1-2's -- this is a read,
            // not a write, so there's no data-shape reason it should ever
            // fail differently per row, but the fallback costs nothing to
            // keep for symmetry and defense-in-depth.
            let mut map = HashMap::new();
            for &id in &distinct_trains_ids {
                match fetch_previous_derived_state(pool, id).await {
                    Ok(state) => {
                        map.insert(id, state);
                    }
                    Err(err) => {
                        let message = format!("fetch_previous_derived_state failed: {err}");
                        for &(i, tid) in &active {
                            if tid == id {
                                results[i] = Err(anyhow::anyhow!("{message}"));
                            }
                        }
                    }
                }
            }
            map
        }
    };

    // Step 4: derive + write, one event at a time, in original batch
    // order -- preserving both intra-batch causality (see this function's
    // own doc comment) and the existing per-event isolation for the two
    // final writes.
    for &(i, trains_id) in &active {
        if results[i].is_err() {
            continue;
        }
        let event = &events[i];
        let previous = state_map
            .get(&trains_id)
            .cloned()
            .unwrap_or_else(DerivedState::awaiting_activation);

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
                let mut derived =
                    journey::apply_movement(&previous, &movement, event.crs.as_deref());
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
            _ => continue,
        };
        state_map.insert(trains_id, derived.clone());

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
        if let Err(err) =
            crate::data::train_tracking::upsert_train_movement(pool, trains_id, &movement_event)
                .await
        {
            results[i] = Err(err);
        }
    }

    results
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

/// Batch-shaped sibling of [`fetch_previous_derived_state`] -- one SELECT
/// covering every DISTINCT `trains_id` in `trains_ids`, rather than one
/// SELECT per id. A `trains_id` with no `train_current_state` row yet
/// (never seen a Movement/Cancellation before) is simply absent from the
/// returned map -- callers must apply the same
/// `DerivedState::awaiting_activation()` fallback
/// [`fetch_previous_derived_state`] applies for that case.
async fn fetch_previous_derived_states_batch(
    pool: &PgPool,
    trains_ids: &[i64],
) -> anyhow::Result<HashMap<i64, DerivedState>> {
    if trains_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<i32>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                next_calling_point \
         FROM train_current_state WHERE trains_id = ANY($1)",
    )
    .bind(trains_ids)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                trains_id,
                status,
                last_reported_location,
                last_event_type,
                delay_minutes,
                next_calling_point,
            )| {
                (
                    trains_id,
                    DerivedState {
                        status,
                        last_reported_location,
                        last_event_type,
                        delay_minutes,
                        next_calling_point,
                    },
                )
            },
        )
        .collect())
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movements_batch_collapses_round_trips_across_a_real_batch \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movements_batch_collapses_round_trips_across_a_real_batch() {
        // A real, non-trivial batch: two distinct trains (A, B), with A
        // seeing TWO events (proving the batched find_or_create_train/
        // mark_train_resolved calls dedup a repeated identity rather than
        // writing it twice, and that intra-batch causality for
        // fetch_previous_derived_state survives being collapsed into one
        // SELECT), plus a no-known-train_uid event (proving the existing
        // no-op path still no-ops from inside a batch, not just when
        // called standalone). Before this task's batching change, this
        // would have cost 5 sequential round trips per event (15 total);
        // after, it costs 3 batched round trips for identity resolution/
        // previous-state-fetch plus 2 per event that actually needs a
        // shared-table write (the no-uid event needs none) -- 3 + 2*3 = 9,
        // and NEITHER count grows with how many events happen to share an
        // identity, unlike the old per-event loop.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let event_a1 = TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("TEST-BATCH-UID-A".to_string()),
            train_id: "TEST-BATCH-TRAIN-ID-A1".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:16:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(1),
            dedup_key: "test-batch-dedup-a1".to_string(),
        };
        let event_a2 = TrustBacklogEventMessage {
            crs: Some("MKC".to_string()),
            train_uid: Some("TEST-BATCH-UID-A".to_string()),
            // A later Movement re-supplying a DIFFERENT train_id than
            // event_a1's -- proves mark_trains_resolved_batch keeps the
            // LAST event's value, same as the old sequential-overwrite
            // loop would.
            train_id: "TEST-BATCH-TRAIN-ID-A2".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("ARRIVAL".to_string()),
            planned_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            delay_minutes: Some(0),
            dedup_key: "test-batch-dedup-a2".to_string(),
        };
        let event_b = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: Some("TEST-BATCH-UID-B".to_string()),
            train_id: "TEST-BATCH-TRAIN-ID-B".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T20:00:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T20:00:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            delay_minutes: Some(0),
            dedup_key: "test-batch-dedup-b".to_string(),
        };
        let event_no_uid = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: None, // the accepted gap -- see ingest_shared_movement's doc comment
            train_id: "TEST-BATCH-NO-UID-TRAIN-ID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-batch-dedup-no-uid".to_string(),
        };

        let events = vec![event_a1, event_a2, event_b, event_no_uid];
        let results = ingest_shared_movements_batch(&pool, &events).await;
        assert_eq!(results.len(), 4, "one result per input event");
        for (i, result) in results.iter().enumerate() {
            assert!(
                result.is_ok(),
                "event {i} should have succeeded: {result:?}"
            );
        }

        // Exactly one `trains` row per distinct train_uid -- the whole
        // point of deduping (train_uid, service_date) pairs before the
        // batched find_or_create_train call.
        let trains_a_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM trains WHERE train_uid = 'TEST-BATCH-UID-A'")
                .fetch_one(&pool)
                .await
                .expect("count trains for A");
        assert_eq!(trains_a_count, 1);

        let (trains_a_id, train_id_a): (i64, Option<String>) =
            sqlx::query_as("SELECT id, train_id FROM trains WHERE train_uid = 'TEST-BATCH-UID-A'")
                .fetch_one(&pool)
                .await
                .expect("trains row for A");
        assert_eq!(
            train_id_a,
            Some("TEST-BATCH-TRAIN-ID-A2".to_string()),
            "mark_train_resolved must keep the LAST event's train_id, same as the old \
             sequential-overwrite loop"
        );

        let (trains_b_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-BATCH-UID-B'")
                .fetch_one(&pool)
                .await
                .expect("trains row for B");

        // Both of A's movement events were written -- the batched identity
        // writes did not swallow the second event for the same train.
        let movement_events_a: i64 =
            sqlx::query_scalar("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_a_id)
                .fetch_one(&pool)
                .await
                .expect("count train_movement_events for A");
        assert_eq!(movement_events_a, 2);

        // A's current_state reflects the SECOND event, not the first --
        // proving intra-batch causality (event_a2's derived state was
        // computed from event_a1's just-written state, not a stale
        // pre-batch DB read) survived collapsing fetch_previous_derived_state
        // into one SELECT.
        let (status_a, last_event_type_a, delay_a): (String, Option<String>, Option<i32>) =
            sqlx::query_as(
                "SELECT status, last_event_type, delay_minutes FROM train_current_state \
                 WHERE trains_id = $1",
            )
            .bind(trains_a_id)
            .fetch_one(&pool)
            .await
            .expect("current state for A");
        assert_eq!(status_a, "en_route");
        assert_eq!(last_event_type_a, Some("ARRIVAL".to_string()));
        assert_eq!(delay_a, Some(0));

        let (status_b,): (String,) =
            sqlx::query_as("SELECT status FROM train_current_state WHERE trains_id = $1")
                .bind(trains_b_id)
                .fetch_one(&pool)
                .await
                .expect("current state for B");
        assert_eq!(status_b, "en_route");

        let no_uid_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM trains WHERE train_id = 'TEST-BATCH-NO-UID-TRAIN-ID'",
        )
        .fetch_one(&pool)
        .await
        .expect("count no-uid trains");
        assert_eq!(
            no_uid_count, 0,
            "the no-known-train_uid event must still no-op inside a batch"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_a_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_b_id)
            .execute(&pool)
            .await
            .ok();
    }
}
