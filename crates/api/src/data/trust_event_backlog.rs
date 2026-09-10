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

    // Step 3.5: destination_crs, batched the same way as Step 3 -- feeds
    // confirmed-terminus-ARRIVAL detection below
    // (`trust_schema::journey::apply_movement`'s `destination_crs` param).
    // A read failure here is non-fatal and NOT threaded into `results`:
    // this data only refines status detection, it never gates whether the
    // event itself is written, so a lookup miss degrades gracefully to
    // "destination unknown" (the same as if no schedule had ever matched)
    // rather than failing the whole batch.
    let destination_map =
        crate::data::trains::destination_crs_for_trains_batch(pool, &distinct_trains_ids)
            .await
            .unwrap_or_default();

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
                let destination_crs = destination_map.get(&trains_id).map(String::as_str);
                let mut derived = journey::apply_movement(
                    &previous,
                    &movement,
                    event.crs.as_deref(),
                    destination_crs,
                );
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

/// Test-only, one-shot fault injector for [`fetch_previous_derived_states_batch`]'s
/// batched SELECT, compiled out of every non-test build. It exists because,
/// unlike `find_or_create_trains_batch`/`mark_trains_resolved_batch` (whose
/// batched fallback IS reachable via a real, per-row-attributable DB failure
/// -- a value violating a genuine column constraint), this function's only
/// bound parameter is `trains_ids: &[i64]` -- internally generated
/// surrogate keys from a PRIOR successful step, never raw/untrusted event
/// data -- so there is no "bad row" shape that can make this particular
/// SELECT fail for one id but not another. `ingest_shared_movements_batch`'s
/// own fallback here is reachable in production, though: real-world
/// failures like a dropped connection or statement timeout hit the whole
/// query regardless of row content. This flag lets a test force exactly
/// that kind of failure once, deterministically, so the REAL fallback loop
/// (this function's caller retrying one `trains_id` at a time via
/// [`fetch_previous_derived_state`]) can be exercised against a real
/// database rather than mocked out.
#[cfg(test)]
static FORCE_FETCH_PREVIOUS_DERIVED_STATES_BATCH_FAILURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
    #[cfg(test)]
    if FORCE_FETCH_PREVIOUS_DERIVED_STATES_BATCH_FAILURE
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(anyhow::anyhow!(
            "test-injected fetch_previous_derived_states_batch failure"
        ));
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

    /// Corroborating proof, for THIS call path specifically, of the guard
    /// documented on `upsert_train_movement`'s own doc comment (see
    /// `train_tracking.rs`'s `an_out_of_order_event_does_not_regress_current_state`
    /// for the direct, same-technique proof against that function itself).
    /// `ingest_shared_movement` requires no production code change of its
    /// own -- it already funnels every write through `upsert_train_movement`
    /// (`ingest_shared_movements_batch`'s final write, above), so the guard
    /// applies automatically; this test exists only to confirm that's
    /// actually true end-to-end through THIS module's own public entry
    /// point, not merely inferred from reading the call graph.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                an_out_of_order_ingest_shared_movement_call_does_not_regress_current_state \
                -- --ignored --test-threads=1`"]
    async fn an_out_of_order_ingest_shared_movement_call_does_not_regress_current_state() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let newer_movement = TrustBacklogEventMessage {
            crs: Some("MKC".to_string()),
            train_uid: Some("TEST-BACKLOG-OUT-OF-ORDER-UID".to_string()),
            train_id: "TEST-BACKLOG-OUT-OF-ORDER-TID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("ARRIVAL".to_string()),
            planned_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            delay_minutes: Some(0),
            dedup_key: "test-backlog-out-of-order-newer-dedup".to_string(),
        };
        ingest_shared_movement(&pool, &newer_movement)
            .await
            .expect("the newer movement's own ingest must succeed");

        let (trains_id,): (i64,) = sqlx::query_as(
            "SELECT id FROM trains WHERE train_uid = 'TEST-BACKLOG-OUT-OF-ORDER-UID'",
        )
        .fetch_one(&pool)
        .await
        .expect("a shared trains row must have been created");

        // A STALE Cancellation, arriving SECOND, timestamped BEFORE the
        // movement above -- exactly the design doc's §1.3 scenario: this
        // consumer's own batch redelivering an old message out of
        // real-world order, or racing the live trust-consumer path for the
        // same trains_id. Without the guard, `apply_cancellation` would
        // flip a train that has already progressed (and, in reality, may
        // never have been cancelled at all) to `status = 'cancelled'`.
        let stale_cancellation = TrustBacklogEventMessage {
            crs: None,
            train_uid: Some("TEST-BACKLOG-OUT-OF-ORDER-UID".to_string()),
            train_id: "TEST-BACKLOG-OUT-OF-ORDER-TID".to_string(),
            service_date,
            msg_type: "0002".to_string(),
            event_type: None,
            planned_timestamp: None,
            actual_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-backlog-out-of-order-stale-cancel-dedup".to_string(),
        };
        ingest_shared_movement(&pool, &stale_cancellation)
            .await
            .expect("the stale cancellation's call must succeed (a guarded no-op is not an error)");

        let (status, last_reported_location, delay_minutes): (String, Option<String>, Option<i32>) =
            sqlx::query_as(
                "SELECT status, last_reported_location, delay_minutes \
                 FROM train_current_state WHERE trains_id = $1",
            )
            .bind(trains_id)
            .fetch_one(&pool)
            .await
            .expect("a current-state row must exist");
        assert_eq!(
            status, "en_route",
            "the stale cancellation must not have regressed status to 'cancelled'"
        );
        assert_eq!(
            last_reported_location,
            Some("MKC".to_string()),
            "the stale cancellation must not have touched the newer movement's location"
        );
        assert_eq!(
            delay_minutes,
            Some(0),
            "the stale cancellation must not have regressed delay_minutes"
        );

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
        // writing it twice), plus a no-known-train_uid event (proving the
        // existing no-op path still no-ops from inside a batch, not just
        // when called standalone).
        //
        // NOTE: this test does NOT prove intra-batch causality for
        // fetch_previous_derived_state, despite an earlier version of this
        // comment claiming it did -- `journey::apply_movement` derives
        // every field this test asserts on (status, last_event_type,
        // delay_minutes) from the CURRENT movement alone, never from
        // `previous`, so those assertions would pass identically whether or
        // not `previous` state were threaded correctly between A's two
        // events. See
        // `ingest_shared_movements_batch_preserves_intra_batch_causality_for_a_movement_then_cancellation_pair`
        // below for a test that actually discriminates on this (using a
        // Movement -> Cancellation pair, since `apply_cancellation` DOES
        // copy fields straight from `previous`).
        //
        // Before this task's batching change, this
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movements_batch_falls_back_to_per_pair_find_or_create_when_the_batched_insert_fails \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movements_batch_falls_back_to_per_pair_find_or_create_when_the_batched_insert_fails()
     {
        // Forces `find_or_create_trains_batch`'s batched `INSERT ...
        // UNNEST(...)` to fail for the WHOLE batch by giving one event's
        // train_uid an embedded NUL byte -- Postgres genuinely rejects any
        // TEXT value containing one (`invalid byte sequence for encoding
        // "UTF8": 0x00`, confirmed against this same live database before
        // writing this test), so this isn't a contrived/mocked failure.
        // Asserts the fallback then retries one pair at a time, so the
        // GOOD event still succeeds and the BAD event's failure is
        // reported against only itself -- not silently swallowed, and not
        // misattributed to the good event.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let good = TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("TEST-FALLBACK-STEP1-GOOD".to_string()),
            train_id: "TEST-FALLBACK-STEP1-GOOD-TID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:16:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            delay_minutes: Some(0),
            dedup_key: "test-fallback-step1-good-dedup".to_string(),
        };
        let bad = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            // The embedded NUL byte is what trips the batched INSERT.
            train_uid: Some("TEST-FALLBACK-STEP1-BAD\u{0}".to_string()),
            train_id: "TEST-FALLBACK-STEP1-BAD-TID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-fallback-step1-bad-dedup".to_string(),
        };

        let results = ingest_shared_movements_batch(&pool, &[good, bad]).await;
        assert_eq!(results.len(), 2);
        assert!(
            results[0].is_ok(),
            "the good event must still succeed: {:?}",
            results[0]
        );
        let bad_err = results[1]
            .as_ref()
            .expect_err("the bad event must fail, not be silently swallowed");
        assert!(
            bad_err.to_string().contains("find_or_create_train failed"),
            "the bad event's error must be attributed to the find_or_create_train fallback step, \
             got: {bad_err}"
        );

        let (trains_id, status): (i64, String) = sqlx::query_as(
            "SELECT tr.id, cs.status FROM trains tr \
             JOIN train_current_state cs ON cs.trains_id = tr.id \
             WHERE tr.train_uid = 'TEST-FALLBACK-STEP1-GOOD'",
        )
        .fetch_one(&pool)
        .await
        .expect("the good event's trains/current_state rows must exist");
        assert_eq!(status, "en_route");

        let bad_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM trains WHERE train_id = 'TEST-FALLBACK-STEP1-BAD-TID'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(
            bad_count, 0,
            "the bad event's identity must never have been created"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movements_batch_falls_back_to_per_pair_mark_resolved_when_the_batched_update_fails \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movements_batch_falls_back_to_per_pair_mark_resolved_when_the_batched_update_fails()
     {
        // Same technique as the find_or_create_trains_batch fallback test
        // above, but the NUL byte is on `train_id` instead of `train_uid`,
        // so BOTH events resolve a real trains_id via Step 1 (proving this
        // failure is genuinely isolated to Step 2, `mark_trains_resolved_batch`'s
        // batched `UPDATE ... FROM UNNEST(...)`), which then fails only for
        // the bad event's pair. Asserts the good event still gets its
        // train_id resolved and its current_state written, while the bad
        // event's failure is attributed to only itself and its identity
        // row is left un-resolved (train_id still NULL) rather than ending
        // up with a wrong/partial value.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let good = TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("TEST-FALLBACK-STEP2-GOOD".to_string()),
            train_id: "TEST-FALLBACK-STEP2-GOOD-TID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:16:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            delay_minutes: Some(0),
            dedup_key: "test-fallback-step2-good-dedup".to_string(),
        };
        let bad = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: Some("TEST-FALLBACK-STEP2-BAD".to_string()),
            // The embedded NUL byte is what trips the batched UPDATE, not
            // the (perfectly valid) train_uid this time.
            train_id: "TEST-FALLBACK-STEP2-BAD-TID\u{0}".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-fallback-step2-bad-dedup".to_string(),
        };

        let results = ingest_shared_movements_batch(&pool, &[good, bad]).await;
        assert_eq!(results.len(), 2);
        assert!(
            results[0].is_ok(),
            "the good event must still succeed: {:?}",
            results[0]
        );
        let bad_err = results[1]
            .as_ref()
            .expect_err("the bad event must fail, not be silently swallowed");
        assert!(
            bad_err.to_string().contains("mark_train_resolved failed"),
            "the bad event's error must be attributed to the mark_train_resolved fallback step, \
             got: {bad_err}"
        );

        let (good_trains_id, good_train_id, good_status): (i64, Option<String>, String) =
            sqlx::query_as(
                "SELECT tr.id, tr.train_id, cs.status FROM trains tr \
                 JOIN train_current_state cs ON cs.trains_id = tr.id \
                 WHERE tr.train_uid = 'TEST-FALLBACK-STEP2-GOOD'",
            )
            .fetch_one(&pool)
            .await
            .expect("the good event's trains/current_state rows must exist");
        assert_eq!(
            good_train_id,
            Some("TEST-FALLBACK-STEP2-GOOD-TID".to_string())
        );
        assert_eq!(good_status, "en_route");

        // Step 1 (identity resolution) succeeded for the bad event too --
        // only Step 2 (resolving train_id) failed for it -- so a trains
        // row exists, but must have been left un-resolved rather than
        // ending up with a truncated/garbage train_id.
        let (bad_trains_id, bad_train_id): (i64, Option<String>) = sqlx::query_as(
            "SELECT id, train_id FROM trains WHERE train_uid = 'TEST-FALLBACK-STEP2-BAD'",
        )
        .fetch_one(&pool)
        .await
        .expect("the bad event's trains row must still have been created by Step 1");
        assert_eq!(
            bad_train_id, None,
            "a failed mark_train_resolved must leave train_id un-resolved, not partially written"
        );
        let bad_current_state_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM train_current_state WHERE trains_id = $1")
                .bind(bad_trains_id)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(
            bad_current_state_count, 0,
            "an event whose resolution failed must not reach the derive/write step"
        );

        sqlx::query("DELETE FROM trains WHERE id IN ($1, $2)")
            .bind(good_trains_id)
            .bind(bad_trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movements_batch_falls_back_to_per_id_fetch_when_the_batched_previous_state_select_fails \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movements_batch_falls_back_to_per_id_fetch_when_the_batched_previous_state_select_fails()
     {
        // Unlike Steps 1/2, `fetch_previous_derived_states_batch`'s only
        // bound parameter is a `Vec<i64>` of ALREADY-resolved surrogate
        // trains_ids, not raw/untrusted event data -- there is no "bad
        // row" shape a caller can supply that makes this SELECT fail for
        // one id but not another. What IS real and reachable in
        // production is the batched SELECT failing outright for reasons
        // unrelated to row content (a dropped connection, a statement
        // timeout) while the DATABASE ITSELF stays healthy -- so this test
        // uses `FORCE_FETCH_PREVIOUS_DERIVED_STATES_BATCH_FAILURE`, a
        // one-shot, #[cfg(test)]-only fault injector (compiled out of
        // every non-test build -- see its own doc comment), to force
        // exactly that once, and then exercises the REAL per-row fallback
        // (`fetch_previous_derived_state`) against real data.
        //
        // Two trains each get a real Movement first (establishing REAL,
        // non-default previous state), then, with the fault armed, both
        // get a Cancellation in the SAME batch call. `apply_cancellation`
        // copies `last_reported_location`/`last_event_type`/`delay_minutes`
        // straight from `previous` -- so this only passes if the fallback
        // actually fetched each train's REAL prior state from the
        // database, not a default/lost one.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let movement_a = TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("TEST-FALLBACK-STEP3-UID-A".to_string()),
            train_id: "TEST-FALLBACK-STEP3-TID-A".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:20:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(5),
            dedup_key: "test-fallback-step3-movement-a-dedup".to_string(),
        };
        let movement_b = TrustBacklogEventMessage {
            crs: Some("WAT".to_string()),
            train_uid: Some("TEST-FALLBACK-STEP3-UID-B".to_string()),
            train_id: "TEST-FALLBACK-STEP3-TID-B".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("ARRIVAL".to_string()),
            planned_timestamp: Some("2026-09-06T20:00:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T20:03:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(3),
            dedup_key: "test-fallback-step3-movement-b-dedup".to_string(),
        };
        let seed_results = ingest_shared_movements_batch(&pool, &[movement_a, movement_b]).await;
        assert!(
            seed_results.iter().all(|r| r.is_ok()),
            "seeding movements must succeed: {seed_results:?}"
        );

        let (trains_a_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-FALLBACK-STEP3-UID-A'")
                .fetch_one(&pool)
                .await
                .expect("trains row for A");
        let (trains_b_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-FALLBACK-STEP3-UID-B'")
                .fetch_one(&pool)
                .await
                .expect("trains row for B");

        // Arm the one-shot fault injector, then send both trains a
        // Cancellation in the SAME batch call.
        FORCE_FETCH_PREVIOUS_DERIVED_STATES_BATCH_FAILURE
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let cancel_a = TrustBacklogEventMessage {
            crs: None,
            train_uid: Some("TEST-FALLBACK-STEP3-UID-A".to_string()),
            train_id: "TEST-FALLBACK-STEP3-TID-A".to_string(),
            service_date,
            msg_type: "0002".to_string(),
            event_type: None,
            planned_timestamp: None,
            // A real Cancellation always carries TRUST's own `canx_timestamp`
            // in this field (see `upsert_train_movement`'s own doc comment,
            // and `crates/trust-backlog-consumer/src/process.rs`'s
            // Cancellation construction) -- `None` here would be an
            // unrealistic fixture as of the event-time monotonicity guard
            // (Option C of the write-race design doc): this event's
            // `event_time` must be at or after movement_a's (19:20) for its
            // write to apply at all.
            actual_timestamp: Some("2026-09-06T19:25:00Z".parse().unwrap()),
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-fallback-step3-cancel-a-dedup".to_string(),
        };
        let cancel_b = TrustBacklogEventMessage {
            crs: None,
            train_uid: Some("TEST-FALLBACK-STEP3-UID-B".to_string()),
            train_id: "TEST-FALLBACK-STEP3-TID-B".to_string(),
            service_date,
            msg_type: "0002".to_string(),
            event_type: None,
            planned_timestamp: None,
            // Same reasoning as cancel_a's own comment -- at or after
            // movement_b's actual_timestamp (20:03).
            actual_timestamp: Some("2026-09-06T20:05:00Z".parse().unwrap()),
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-fallback-step3-cancel-b-dedup".to_string(),
        };
        let results = ingest_shared_movements_batch(&pool, &[cancel_a, cancel_b]).await;

        assert!(
            !FORCE_FETCH_PREVIOUS_DERIVED_STATES_BATCH_FAILURE
                .load(std::sync::atomic::Ordering::SeqCst),
            "the fault must actually have fired during this call (one-shot flag consumed), \
             otherwise this test isn't exercising the fallback at all"
        );
        assert!(
            results.iter().all(|r| r.is_ok()),
            "both cancellations must still succeed via the per-id fallback: {results:?}"
        );

        let (status_a, last_loc_a, last_event_a, delay_a): (
            String,
            Option<String>,
            Option<String>,
            Option<i32>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, last_event_type, delay_minutes \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_a_id)
        .fetch_one(&pool)
        .await
        .expect("current state for A");
        assert_eq!(status_a, "cancelled");
        assert_eq!(
            last_loc_a,
            Some("EUS".to_string()),
            "must carry over A's REAL previous location, fetched via the per-id fallback"
        );
        assert_eq!(last_event_a, Some("DEPARTURE".to_string()));
        assert_eq!(delay_a, Some(5));

        let (status_b, last_loc_b, last_event_b, delay_b): (
            String,
            Option<String>,
            Option<String>,
            Option<i32>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, last_event_type, delay_minutes \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_b_id)
        .fetch_one(&pool)
        .await
        .expect("current state for B");
        assert_eq!(status_b, "cancelled");
        assert_eq!(
            last_loc_b,
            Some("WAT".to_string()),
            "must carry over B's REAL previous location, fetched via the per-id fallback"
        );
        assert_eq!(last_event_b, Some("ARRIVAL".to_string()));
        assert_eq!(delay_b, Some(3));

        sqlx::query("DELETE FROM trains WHERE id IN ($1, $2)")
            .bind(trains_a_id)
            .bind(trains_b_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                ingest_shared_movements_batch_preserves_intra_batch_causality_for_a_movement_then_cancellation_pair \
                -- --ignored --test-threads=1`"]
    async fn ingest_shared_movements_batch_preserves_intra_batch_causality_for_a_movement_then_cancellation_pair()
     {
        // The discriminating test the flagship
        // `ingest_shared_movements_batch_collapses_round_trips_across_a_real_batch`
        // test above claimed to be, but isn't: `journey::apply_movement`
        // derives every field a same-train Movement-then-Movement pair
        // could assert on purely from the CURRENT movement, never from
        // `previous` -- so that test would pass identically even if a
        // broken implementation read every event in a batch from the SAME
        // stale pre-batch snapshot instead of the causally-updated
        // in-memory state map.
        //
        // `journey::apply_cancellation`, by contrast, copies
        // `last_reported_location`/`last_event_type`/`delay_minutes`
        // straight from `previous` -- so a same-train Movement followed by
        // a Cancellation, in ONE batch call, for a train with NO prior
        // database row (previous == DerivedState::awaiting_activation()
        // before this call), actually discriminates: a correct
        // implementation has the Cancellation observe the Movement's
        // JUST-COMPUTED state (real location/event_type/delay); a broken
        // one that re-reads a pre-batch snapshot would have it observe the
        // pre-batch default instead (None/None/None), since there was no
        // database row before this call started.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let movement = TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("TEST-CAUSALITY-UID".to_string()),
            train_id: "TEST-CAUSALITY-TID".to_string(),
            service_date,
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:20:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(5),
            dedup_key: "test-causality-movement-dedup".to_string(),
        };
        let cancellation = TrustBacklogEventMessage {
            crs: None,
            train_uid: Some("TEST-CAUSALITY-UID".to_string()),
            train_id: "TEST-CAUSALITY-TID".to_string(),
            service_date,
            msg_type: "0002".to_string(),
            event_type: None,
            planned_timestamp: None,
            // A real Cancellation always carries TRUST's own `canx_timestamp`
            // here (see `upsert_train_movement`'s own doc comment) -- `None`
            // would be unrealistic as of the event-time monotonicity guard
            // (Option C of the write-race design doc): this event's
            // `event_time` must be at or after the movement's (19:20) for
            // its write to apply at all.
            actual_timestamp: Some("2026-09-06T19:25:00Z".parse().unwrap()),
            variation_status: None,
            delay_minutes: None,
            dedup_key: "test-causality-cancellation-dedup".to_string(),
        };

        let results = ingest_shared_movements_batch(&pool, &[movement, cancellation]).await;
        assert!(
            results.iter().all(|r| r.is_ok()),
            "both events should succeed: {results:?}"
        );

        let (trains_id,): (i64,) =
            sqlx::query_as("SELECT id FROM trains WHERE train_uid = 'TEST-CAUSALITY-UID'")
                .fetch_one(&pool)
                .await
                .expect("trains row");

        let (status, last_reported_location, last_event_type, delay_minutes): (
            String,
            Option<String>,
            Option<String>,
            Option<i32>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, last_event_type, delay_minutes \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("current state");

        assert_eq!(status, "cancelled");
        assert_eq!(
            last_reported_location,
            Some("EUS".to_string()),
            "the Cancellation must have observed the Movement's just-computed location, not a \
             stale pre-batch default -- this is what proves intra-batch causality survived \
             batching"
        );
        assert_eq!(
            last_event_type,
            Some("DEPARTURE".to_string()),
            "same causality proof, for last_event_type"
        );
        assert_eq!(
            delay_minutes,
            Some(5),
            "same causality proof, for delay_minutes"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
