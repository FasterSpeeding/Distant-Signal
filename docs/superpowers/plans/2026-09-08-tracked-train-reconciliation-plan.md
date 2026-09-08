# Tracked-Train Reconciliation Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a periodic background sweep in `crates/api` that (1) flips a
`train_subscriptions` row stuck at `resolution_status = 'pending'` to
`'resolved'` once `train_movement_events` proves the train was actually
tracked, and (2) retries schedule enrichment for an NR-primary tracked
train whose `trains` row has no schedule data yet, sourcing it from
`schedule_destination_departures` instead of TRUST backlog.

**Architecture:** One new module, `crates/api/src/data/reconciliation.rs`,
with two independent public functions plus a small combinator, wired into
a new `tokio::spawn`ed background loop in `crates/api/src/main.rs`,
mirroring the existing `schedule_match_sweep_loop` exactly. No schema
change — every column this plan reads or writes already exists and is
already nullable where it needs to be.

**Tech Stack:** Rust, `sqlx` (runtime-checked `sqlx::query`/`query_as`, no
`query!` macro), PostgreSQL 16, `tokio`, `chrono`.

**Spec:** docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md

## Global Constraints

- No migration in this plan. `train_subscriptions.pin_origin_crs`/
  `pin_scheduled_departure` are already nullable
  (`20260906130000_nullable_pin_columns.sql`); every column this plan
  writes (`train_subscriptions.resolution_status`,
  `trains.{origin_crs,destination_crs,calling_points,matched_line_id,schedule_matched_at}`)
  already exists.
- `crates/api` uses runtime-checked `sqlx::query`/`sqlx::query_as`
  exclusively — no `query!`/`query_as!`, no `.sqlx` query cache.
- Stall 1's fix (`reconcile_stuck_resolution_status`) writes **only**
  `resolution_status`, replicating `flip_legacy_resolution`'s exact
  post-Task-22 invariant (`crates/api/src/data/train_tracking.rs:700-735`)
  — never write `trains.train_id`/`resolved_at` from this path; those
  belong to `mark_train_resolved` alone.
- Stall 2's fix (`retry_schedule_enrichment_for_nr_primary_trains`) never
  hand-builds a `calling_points` JSON value. It sources only
  `(origin_crs, scheduled_departure)` from `schedule_destination_departures`
  and hands them to the existing, already-tested
  `schedule_matching::attempt_schedule_match_for_shared_train`, which does
  the actual matching (against `schedule_line_population`, which alone
  carries the full TIPLOC/kind/arrival-time fidelity
  `ScheduleCallingPointDto` requires) and the actual write (via
  `trains::find_or_create_train_with_schedule_match`, already
  `COALESCE`-safe against a concurrent earlier match).
- Stall 2's candidate query is scoped to `trains` rows referenced by at
  least one `train_subscriptions.trains_id` — never every `trains` row in
  existence (a `trains` row can have zero subscribers, from broad
  ingestion elsewhere in this codebase).
- Grace period: 30 minutes past a candidate's true origin departure
  (converted London-local → UTC via `crate::data::eta_blend::london_to_utc`,
  the same helper `schedule_matching.rs` already uses), before Stall 2's
  fix will touch a row — comfortably past `common::MATCH_TOLERANCE`
  (±20 minutes), so the live/backlog paths already had their normal window
  to resolve it first.
- Sweep interval: 300 seconds, reusing `schedule_match_interval_secs`'s own
  default and reasoning for the identical class of concern.
- `retry_schedule_enrichment_for_nr_primary_trains` takes `now:
  DateTime<Utc>` as an explicit parameter rather than calling
  `chrono::Utc::now()` internally — same "inject the clock" convention
  `train_tracking::validate_pin(pin, now)` already establishes in this
  codebase, and what makes this function's grace-period gate testable with
  fixed, deterministic timestamps rather than real wall-clock time.
- Every `#[tokio::test]` DB-gated test in this plan is `#[ignore]`d with the
  same message shape this codebase already uses, and requires
  `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test` (run
  `sqlx migrate run` first if the schema is not already current — this
  plan adds no migration, so this is only needed if the test database is
  genuinely behind).

---

## Task 1: `reconcile_stuck_resolution_status` — Stall 1's fix

**Files:**
- Create: `crates/api/src/data/reconciliation.rs`
- Modify: `crates/api/src/data/mod.rs` (register the new module, alphabetically between `queries` and `reference`)
- Test: `crates/api/src/data/reconciliation.rs` (co-located `#[cfg(test)] mod db_tests`, same convention as `trains.rs`/`schedule_matching.rs`)

**Interfaces:**
- Consumes: nothing new — plain `sqlx::PgPool` queries against
  `train_subscriptions`/`train_movement_events`, both existing tables.
- Produces: `pub async fn reconcile_stuck_resolution_status(pool: &sqlx::PgPool) -> anyhow::Result<u64>`,
  returning the number of rows flipped. Task 2 and Task 3 both depend on
  this exact name and signature.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/api/src/data/reconciliation.rs
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
    unimplemented!()
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api reconcile_stuck_resolution_status -- --ignored --test-threads=1`
Expected: compile failure or panic from `unimplemented!()` — the function
body has not been written yet.

- [ ] **Step 3: Write minimal implementation**

Replace the `unimplemented!()` body:

```rust
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
```

Register the module:

```rust
// crates/api/src/data/mod.rs -- insert alphabetically
pub mod queries;
pub mod reconciliation;
pub mod reference;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api reconcile_stuck_resolution_status -- --ignored --test-threads=1`
Expected: all three tests `PASS`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/reconciliation.rs crates/api/src/data/mod.rs
git commit -m "Add reconcile_stuck_resolution_status: reconcile pending subscriptions with real movement events"
```

---

## Task 2: `retry_schedule_enrichment_for_nr_primary_trains` and `run_reconciliation_sweep` — Stall 2's fix

**Files:**
- Modify: `crates/api/src/data/reconciliation.rs`
- Test: same file, extending `db_tests`

**Interfaces:**
- Consumes: `reconcile_stuck_resolution_status` (Task 1, same module);
  `crate::data::schedule_matching::attempt_schedule_match_for_shared_train(pool, train_uid, origin_crs, scheduled_departure, service_date, crs_line_index) -> anyhow::Result<bool>`
  (existing, `crates/api/src/data/schedule_matching.rs:253-296`);
  `crate::data::eta_blend::london_to_utc(naive: chrono::NaiveDateTime) -> Option<chrono::DateTime<chrono::Utc>>`
  (existing, `pub(crate)`, `crates/api/src/data/eta_blend.rs:53`).
- Produces:
  `pub async fn retry_schedule_enrichment_for_nr_primary_trains(pool: &sqlx::PgPool, crs_line_index: &std::collections::HashMap<String, Vec<String>>, grace_period: chrono::Duration, now: chrono::DateTime<chrono::Utc>) -> anyhow::Result<u64>`,
  returning the number of `trains` rows actually schedule-matched this
  call.
  `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)] pub struct ReconciliationSweepResult { pub resolution_status_reconciled: u64, pub schedule_enrichment_matched: u64 }`.
  `pub async fn run_reconciliation_sweep(pool: &sqlx::PgPool, crs_line_index: &std::collections::HashMap<String, Vec<String>>, grace_period: chrono::Duration) -> anyhow::Result<ReconciliationSweepResult>`.
  Task 3 depends on `run_reconciliation_sweep`'s exact name and signature.

- [ ] **Step 1: Write the failing tests**

Add to the top of `crates/api/src/data/reconciliation.rs` (below the existing `use`/Task 1 function):

```rust
use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::data::eta_blend::london_to_utc;
use crate::data::schedule_matching;

/// One `trains` row that has at least one subscriber but no schedule data
/// yet -- Stall 2's candidate set (design doc Decision 2). Scoped to
/// subscriber-referenced rows only: a `trains` row can exist with zero
/// subscribers (broad ingestion elsewhere in this codebase), and this
/// sweep's whole purpose is fixing what a TRACKED train's page shows, not
/// backfilling schedule data for the whole network.
#[derive(Debug, Clone, sqlx::FromRow)]
struct EnrichmentCandidate {
    id: i64,
    train_uid: String,
    service_date: NaiveDate,
}

async fn list_trains_needing_schedule_enrichment(
    pool: &PgPool,
) -> anyhow::Result<Vec<EnrichmentCandidate>> {
    let rows = sqlx::query_as::<_, EnrichmentCandidate>(
        "SELECT DISTINCT tr.id, tr.train_uid, tr.service_date \
         FROM trains tr \
         JOIN train_subscriptions ts ON ts.trains_id = tr.id \
         WHERE tr.schedule_matched_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The one row of `schedule_destination_departures` for `(train_uid,
/// service_date)` that represents the schedule's OWN origin departure --
/// `origin_crs = true_origin_crs` is what picks that row out from the
/// several this train may have (one per departure-bearing calling point).
/// See the design doc §3 Decision 2.
async fn true_origin_departure(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<(String, chrono::NaiveTime)>> {
    let row: Option<(String, chrono::NaiveTime)> = sqlx::query_as(
        "SELECT origin_crs, scheduled FROM schedule_destination_departures \
         WHERE train_uid = $1 AND service_date = $2 AND origin_crs = true_origin_crs \
         LIMIT 1",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Stall 2's fix: an NR-primary tracked train
/// (`train_tracking::create_subscription_for_train`) never acquires
/// schedule data (origin, destination, calling points) unless TRUST backlog
/// happened to have it at track-creation time
/// (`routes::train::enrich_shared_train`). CIF SCHEDULE data is available
/// independent of TRUST, in `schedule_destination_departures` -- this
/// retries against it for every still-unmatched, subscriber-referenced
/// `trains` row whose true origin departure has passed by `grace_period`
/// (a courtesy to the live/backlog paths' own normal resolution window, not
/// a data-availability requirement -- see the design doc §3 Decision 3).
/// `now` is injected, not read from the clock, so this stays testable with
/// fixed timestamps -- the same convention
/// `train_tracking::validate_pin(pin, now)` already establishes.
pub async fn retry_schedule_enrichment_for_nr_primary_trains(
    pool: &PgPool,
    crs_line_index: &HashMap<String, Vec<String>>,
    grace_period: chrono::Duration,
    now: DateTime<Utc>,
) -> anyhow::Result<u64> {
    let candidates = list_trains_needing_schedule_enrichment(pool).await?;
    let mut matched = 0u64;

    for candidate in candidates {
        let origin = match true_origin_departure(pool, &candidate.train_uid, candidate.service_date).await
        {
            Ok(Some(origin)) => origin,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    trains_id = candidate.id,
                    "schedule-enrichment lookup failed for this train; will retry next sweep"
                );
                continue;
            }
        };
        let (origin_crs, scheduled) = origin;

        let Some(scheduled_departure) = london_to_utc(candidate.service_date.and_time(scheduled))
        else {
            tracing::warn!(
                trains_id = candidate.id,
                "scheduled departure did not resolve to a real London local time; skipping"
            );
            continue;
        };

        if now - scheduled_departure < grace_period {
            continue;
        }

        match schedule_matching::attempt_schedule_match_for_shared_train(
            pool,
            &candidate.train_uid,
            &origin_crs,
            scheduled_departure,
            candidate.service_date,
            crs_line_index,
        )
        .await
        {
            Ok(true) => matched += 1,
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    trains_id = candidate.id,
                    "schedule-enrichment retry failed for this train; will retry next sweep"
                );
            }
        }
    }

    Ok(matched)
}

/// Both halves of the reconciliation sweep, combined for
/// `main.rs`'s single background loop. See the design doc §3 Decision 5
/// for why one shared interval governs both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconciliationSweepResult {
    pub resolution_status_reconciled: u64,
    pub schedule_enrichment_matched: u64,
}

pub async fn run_reconciliation_sweep(
    pool: &PgPool,
    crs_line_index: &HashMap<String, Vec<String>>,
    grace_period: chrono::Duration,
) -> anyhow::Result<ReconciliationSweepResult> {
    let resolution_status_reconciled = reconcile_stuck_resolution_status(pool).await?;
    let schedule_enrichment_matched = retry_schedule_enrichment_for_nr_primary_trains(
        pool,
        crs_line_index,
        grace_period,
        Utc::now(),
    )
    .await?;
    Ok(ReconciliationSweepResult {
        resolution_status_reconciled,
        schedule_enrichment_matched,
    })
}
```

Add to `db_tests` (same `mod db_tests` block as Task 1):

```rust
    fn population_json(uid: &str, tiploc: &str, departure: &str) -> serde_json::Value {
        serde_json::json!([{
            "uid": uid,
            "calling_points": [{
                "tiploc": tiploc,
                "kind": "Origin",
                "booked_arrival": null,
                "booked_departure": departure,
                "is_half_minute_arrival": false,
                "is_half_minute_departure": false
            }]
        }])
    }

    /// Fixture for Stall 2's tests: a subscribed, unmatched `trains` row,
    /// plus everything `attempt_schedule_match_for_shared_train` needs to
    /// successfully match it (a `stanox_crs` translation, a
    /// `schedule_line_population` entry) and a `schedule_destination_departures`
    /// row for `true_origin_departure` to find. Returns
    /// `(trains_id, subscription_id, crs_line_index)`.
    async fn seed_enrichment_fixture(
        pool: &PgPool,
        user_id: &str,
        train_uid: &str,
        service_date: chrono::NaiveDate,
        origin_crs: &str,
        stanox: &str,
        line_id: &str,
        scheduled: &str,
    ) -> (i64, i64, HashMap<String, Vec<String>>) {
        seed_user(pool, user_id).await;
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ($1, $2, 'EUSTON', 'LONDON EUSTON', 1) ON CONFLICT (stanox) DO NOTHING",
        )
        .bind(stanox)
        .bind(origin_crs)
        .execute(pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
        )
        .bind(line_id)
        .bind(service_date)
        .bind(population_json(train_uid, "EUSTON ", scheduled))
        .execute(pool)
        .await
        .expect("seed schedule_line_population");
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, 'WAT', $2::time, $3, $4, $4)",
        )
        .bind(service_date)
        .bind(scheduled)
        .bind(train_uid)
        .bind(origin_crs)
        .execute(pool)
        .await
        .expect("seed schedule_destination_departures");

        let trains_id = seed_train(pool, train_uid, service_date).await;
        let subscription_id = seed_pending_subscription(pool, user_id, service_date, trains_id).await;

        let mut crs_line_index = HashMap::new();
        crs_line_index.insert(origin_crs.to_string(), vec![line_id.to_string()]);

        (trains_id, subscription_id, crs_line_index)
    }

    async fn read_schedule_matched_at(pool: &PgPool, trains_id: i64) -> Option<chrono::DateTime<chrono::Utc>> {
        let schedule_matched_at: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar("SELECT schedule_matched_at FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(pool)
                .await
                .expect("read back schedule_matched_at");
        schedule_matched_at
    }

    async fn cleanup_enrichment_fixture(
        pool: &PgPool,
        user_id: &str,
        train_uid: &str,
        stanox: &str,
        line_id: &str,
        service_date: chrono::NaiveDate,
    ) {
        cleanup(pool, user_id, train_uid).await;
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = $1")
            .bind(train_uid)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = $1 AND service_date = $2")
            .bind(line_id)
            .bind(service_date)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = $1")
            .bind(stanox)
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                retry_schedule_enrichment_matches_a_subscribed_trains_row_past_the_grace_period \
                -- --ignored`"]
    async fn retry_schedule_enrichment_matches_a_subscribed_trains_row_past_the_grace_period() {
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL2-A";
        let train_uid = "TEST-RECON-STALL2-UID-A";
        let service_date: chrono::NaiveDate = "2020-01-01".parse().unwrap();
        let (trains_id, _subscription_id, crs_line_index) = seed_enrichment_fixture(
            &pool,
            user_id,
            train_uid,
            service_date,
            "EUS",
            "TEST-RECON-STANOX-A",
            "test-recon-line-a",
            "09:00",
        )
        .await;

        // 1 hour after the 09:00 scheduled departure -- comfortably past
        // the 30-minute grace period.
        let now: chrono::DateTime<chrono::Utc> = "2020-01-01T10:00:00Z".parse().unwrap();
        let matched = retry_schedule_enrichment_for_nr_primary_trains(
            &pool,
            &crs_line_index,
            chrono::Duration::minutes(30),
            now,
        )
        .await
        .expect("retry_schedule_enrichment_for_nr_primary_trains");
        assert_eq!(matched, 1);

        assert!(
            read_schedule_matched_at(&pool, trains_id).await.is_some(),
            "the trains row must now carry schedule data"
        );

        cleanup_enrichment_fixture(
            &pool,
            user_id,
            train_uid,
            "TEST-RECON-STANOX-A",
            "test-recon-line-a",
            service_date,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                retry_schedule_enrichment_skips_a_row_still_inside_the_grace_period \
                -- --ignored`"]
    async fn retry_schedule_enrichment_skips_a_row_still_inside_the_grace_period() {
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL2-B";
        let train_uid = "TEST-RECON-STALL2-UID-B";
        let service_date: chrono::NaiveDate = "2020-01-01".parse().unwrap();
        let (trains_id, _subscription_id, crs_line_index) = seed_enrichment_fixture(
            &pool,
            user_id,
            train_uid,
            service_date,
            "EUS",
            "TEST-RECON-STANOX-B",
            "test-recon-line-b",
            "09:00",
        )
        .await;

        // Only 10 minutes after the 09:00 scheduled departure -- inside a
        // 30-minute grace period.
        let now: chrono::DateTime<chrono::Utc> = "2020-01-01T09:10:00Z".parse().unwrap();
        let matched = retry_schedule_enrichment_for_nr_primary_trains(
            &pool,
            &crs_line_index,
            chrono::Duration::minutes(30),
            now,
        )
        .await
        .expect("retry_schedule_enrichment_for_nr_primary_trains");
        assert_eq!(matched, 0, "still inside the grace period; must not be touched yet");

        assert!(read_schedule_matched_at(&pool, trains_id).await.is_none());

        cleanup_enrichment_fixture(
            &pool,
            user_id,
            train_uid,
            "TEST-RECON-STANOX-B",
            "test-recon-line-b",
            service_date,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                retry_schedule_enrichment_skips_a_trains_row_with_no_subscriber \
                -- --ignored`"]
    async fn retry_schedule_enrichment_skips_a_trains_row_with_no_subscriber() {
        let pool = connect().await;
        let train_uid = "TEST-RECON-STALL2-UID-C";
        let service_date: chrono::NaiveDate = "2020-01-01".parse().unwrap();
        // A trains row with real schedule data available, but no
        // train_subscriptions row referencing it at all -- Decision 2's
        // scoping must exclude it.
        let trains_id = seed_train(&pool, train_uid, service_date).await;
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, 'WAT', '09:00'::time, $2, 'EUS', 'EUS')",
        )
        .bind(service_date)
        .bind(train_uid)
        .execute(&pool)
        .await
        .expect("seed schedule_destination_departures");

        let now: chrono::DateTime<chrono::Utc> = "2020-01-01T12:00:00Z".parse().unwrap();
        let matched = retry_schedule_enrichment_for_nr_primary_trains(
            &pool,
            &HashMap::new(),
            chrono::Duration::minutes(30),
            now,
        )
        .await
        .expect("retry_schedule_enrichment_for_nr_primary_trains");
        assert_eq!(matched, 0, "an unsubscribed trains row must never be a candidate");

        assert!(read_schedule_matched_at(&pool, trains_id).await.is_none());

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                retry_schedule_enrichment_skips_a_trains_row_with_no_schedule_destination_departures_data \
                -- --ignored`"]
    async fn retry_schedule_enrichment_skips_a_trains_row_with_no_schedule_destination_departures_data()
    {
        let pool = connect().await;
        let user_id = "TEST-RECON-STALL2-D";
        let train_uid = "TEST-RECON-STALL2-UID-D";
        let service_date: chrono::NaiveDate = "2020-01-01".parse().unwrap();
        seed_user(&pool, user_id).await;
        let trains_id = seed_train(&pool, train_uid, service_date).await;
        seed_pending_subscription(&pool, user_id, service_date, trains_id).await;

        let now: chrono::DateTime<chrono::Utc> = "2020-01-01T12:00:00Z".parse().unwrap();
        let matched = retry_schedule_enrichment_for_nr_primary_trains(
            &pool,
            &HashMap::new(),
            chrono::Duration::minutes(30),
            now,
        )
        .await
        .expect("retry_schedule_enrichment_for_nr_primary_trains");
        assert_eq!(matched, 0, "no CIF data exists for this train; nothing to enrich from");

        assert!(read_schedule_matched_at(&pool, trains_id).await.is_none());

        cleanup(&pool, user_id, train_uid).await;
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api retry_schedule_enrichment -- --ignored --test-threads=1`
Expected: compile failure (the new functions/types referenced by the tests
do not exist yet until Step 3 is applied — write the test code first,
confirm it fails to compile, exactly mirroring Task 1's cycle).

- [ ] **Step 3: Write minimal implementation**

Apply the `retry_schedule_enrichment_for_nr_primary_trains`,
`list_trains_needing_schedule_enrichment`, `true_origin_departure`,
`ReconciliationSweepResult`, and `run_reconciliation_sweep` code shown in
Step 1 above (it is the real, complete implementation, not a stub — add it
to `crates/api/src/data/reconciliation.rs` below Task 1's function).

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api retry_schedule_enrichment -- --ignored --test-threads=1`
Expected: all four tests `PASS`.

Also run the full crate's fast (non-DB) test suite to confirm nothing else
broke: `cargo test -p api`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/reconciliation.rs
git commit -m "Add retry_schedule_enrichment_for_nr_primary_trains and run_reconciliation_sweep"
```

---

## Task 3: Wire the sweep into `crates/api/src/main.rs`

**Files:**
- Modify: `crates/api/src/data/config.rs` (two new `ServiceArguments` fields)
- Modify: `crates/api/src/main.rs` (new background loop, spawned alongside `schedule_match_sweep_loop`)

**Interfaces:**
- Consumes: `data::reconciliation::run_reconciliation_sweep` (Task 2);
  `app.database: sqlx::PgPool`, `app.schedule_crs_line_index: HashMap<String, Vec<String>>`,
  `app.config: ServiceArguments` (all pre-existing `App`/`AppState` fields,
  already used identically by `schedule_match_sweep_loop`).
- Produces: nothing further downstream depends on this task — it is the
  final integration point.

- [ ] **Step 1: Add the two config fields**

```rust
// crates/api/src/data/config.rs -- add immediately after
// schedule_match_interval_secs's own field
    /// How often the reconciliation sweep re-attempts (1) flipping a
    /// `train_subscriptions` row stuck at `resolution_status = 'pending'`
    /// once `train_movement_events` proves the train was tracked, and (2)
    /// schedule-enriching an NR-primary tracked train from
    /// `schedule_destination_departures` when TRUST backlog had nothing at
    /// track-creation time. See
    /// docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md
    /// Decision 5. 300s default, reusing `schedule_match_interval_secs`'s
    /// own reasoning for the identical class of concern -- both halves of
    /// this sweep are cheap enough at this cadence not to matter, and
    /// frequent enough that a stuck row is fixed within a rail day's
    /// working hours.
    #[arg(long, env, default_value_t = 300)]
    pub reconciliation_sweep_interval_secs: u64,

    /// How long past a candidate's true origin departure the schedule-
    /// enrichment half of the reconciliation sweep waits before attempting
    /// a CIF-only match -- a courtesy to the live TRUST/backlog paths' own
    /// normal resolution window (`common::MATCH_TOLERANCE`, ±20 minutes),
    /// not a data-availability requirement. See the design doc's Decision
    /// 3. 30 minutes default, at the upper (more conservative) end of that
    /// document's own suggested 15-30 minute range.
    #[arg(long, env, default_value_t = 30)]
    pub schedule_enrichment_grace_minutes: i64,
```

- [ ] **Step 2: Add the background loop**

```rust
// crates/api/src/main.rs -- spawn alongside schedule_match_sweep_loop
    tokio::spawn(schedule_match_sweep_loop(app.clone()));
    tokio::spawn(reconciliation_sweep_loop(app.clone()));
```

```rust
// crates/api/src/main.rs -- new function, alongside schedule_match_sweep_loop
/// Periodic retry of two independent, confirmed stalls in tracked-train
/// state -- see
/// docs/superpowers/specs/2026-09-08-tracked-train-reconciliation-design.md.
/// Mirrors `schedule_match_sweep_loop`'s own shape exactly: same "a
/// request/response server also runs a background interval loop" pattern
/// this workspace already established.
async fn reconciliation_sweep_loop(app: App) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        app.config.reconciliation_sweep_interval_secs,
    ));
    let grace_period = chrono::Duration::minutes(app.config.schedule_enrichment_grace_minutes);
    loop {
        interval.tick().await;
        match data::reconciliation::run_reconciliation_sweep(
            &app.database,
            &app.schedule_crs_line_index,
            grace_period,
        )
        .await
        {
            Ok(result)
                if result.resolution_status_reconciled > 0 || result.schedule_enrichment_matched > 0 =>
            {
                tracing::info!(
                    resolution_status_reconciled = result.resolution_status_reconciled,
                    schedule_enrichment_matched = result.schedule_enrichment_matched,
                    "reconciliation sweep made progress on stuck tracked-train state"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "reconciliation sweep failed; will retry next interval");
            }
        }
    }
}
```

- [ ] **Step 3: Verify the crate builds and every existing test still passes**

There is no dedicated unit test for the loop itself in this codebase's own
convention — `schedule_match_sweep_loop` has none either; its correctness
rests entirely on `run_schedule_match_sweep`'s own tests (mirrored here by
Task 1/2's tests covering `run_reconciliation_sweep`'s two halves).

Run: `cargo build -p api`
Expected: builds cleanly, no warnings about the two new fields or the new
function being unused.

Run: `cargo test -p api`
Expected: every existing (non-DB-gated) test still passes.

- [ ] **Step 4: Manual smoke check against a real dev database**

```bash
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test \
REDIS_URL=redis://localhost:6379 \
  cargo run -p api 2>&1 | grep -i reconciliation
```

Expected: no crash on startup; once `reconciliation_sweep_interval_secs`
(300s, or set a shorter value via env for a faster manual check) elapses,
either silence (nothing to reconcile — expected on a clean dev database) or
an `info`-level "reconciliation sweep made progress" line if a fixture row
from Task 1/2's own tests was left behind (clean those up first with the
`cleanup*` helpers' own `DELETE` statements if so).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/main.rs
git commit -m "Wire the reconciliation sweep into api's background loops"
```

---

## Task 4: Full workspace verification

**Files:** none (verification only).

**Interfaces:** none.

- [ ] **Step 1: Build the whole workspace**

Run: `cargo build --workspace`
Expected: clean build, no errors, no new warnings.

- [ ] **Step 2: Run the whole workspace's fast test suite**

Run: `cargo test --workspace`
Expected: all pass (this does not include the `#[ignore]`d DB-gated tests
from Tasks 1-2, which Step 3 below covers).

- [ ] **Step 3: Run the DB-gated `api` tests, including this plan's new ones**

```bash
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test \
  cargo test -p api -- --ignored --test-threads=1
```

Expected: all pass, including every test added in Tasks 1-2 and every
pre-existing DB-gated test in `crates/api` (confirming this plan introduced
no regression in `schedule_matching`, `train_tracking`, or `trains`).

- [ ] **Step 4: Commit (if Step 1-3 surfaced any fix)**

Only if a genuine fix was needed to make the workspace build/test clean —
if everything already passed, there is nothing to commit here.

```bash
git add -A
git commit -m "Fix workspace verification findings for tracked-train reconciliation"
```
