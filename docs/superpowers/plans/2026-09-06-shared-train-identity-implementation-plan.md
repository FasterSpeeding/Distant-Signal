# Shared Train Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a train a shared, public entity keyed by real-world identity (`train_uid`/`service_date`) rather than a per-user pin, so any subscriber's live status and any unauthenticated caller's `GET /Train/by-uid/{uid}/{date}` lookup read the same row.

**Architecture:** A new `trains` table becomes the shared parent row; `tracked_trains` (later renamed `train_subscriptions`) shrinks to a private per-user join table pointing at it via a nullable `trains_id` FK (`ON DELETE SET NULL`), while `train_movement_events`/`train_current_state` re-point from per-subscription to per-train (`ON DELETE CASCADE`). The cutover is staged expand/contract (Steps A-D) so every intermediate state keeps existing callers working, `trust-backlog-consumer` becomes the primary writer for the new shared movement tables, and `trust-consumer` is repurposed into a lighter, faster notifier-forwarding signal.

**Tech Stack:** Rust, `sqlx` (runtime-checked `sqlx::query`/`query_as`, no `query!` macro), PostgreSQL 16, `axum`, `tokio`, `chrono`, `serde_json`, Redis Streams (`movement-feed` crate).

**Spec:** docs/superpowers/specs/2026-09-06-shared-train-identity-design.md

## Global Constraints

- New FK columns pointing at `trains.id` are always named `trains_id`, never `train_id` — `train_id` already means TRUST's own daily identifier string on `tracked_trains`/`trust_event_backlog`, and the two must never collide under one column name.
- `train_subscriptions.trains_id` (the renamed `tracked_trains.trains_id`) uses `ON DELETE SET NULL` — a pruned `trains` row must never cascade away a user's subscription, custom name, tickets, or notification history.
- `train_movement_events.trains_id` and `train_current_state.trains_id` use `ON DELETE CASCADE` — neither table has any private, user-owned data worth preserving past its train's retention window.
- `trains` has no `resolution_status` enum. Status is derived from column presence: `calling_points IS NOT NULL` means "has schedule data," `train_id IS NOT NULL` means "has live data." These are independent booleans, never a single ordered state.
- `crates/api` uses runtime-checked `sqlx::query`/`sqlx::query_as` exclusively — no `query!`/`query_as!`, no `.sqlx` query cache. Every new query in this plan follows that convention.
- A `trains` row's `train_id` column is TRUST-sourced only, never written from a schedule match — this mirrors `tracked_trains.train_uid`/`train_id`'s existing separation.
- `trust_event_backlog` is kept, not retired — it stays scoped to the legacy CRS+time fallback lookup; the new `trains`/`train_movement_events`/`train_current_state` tables are a separate, parallel system.
- `trust_event_backlog_retention_days` keeps its own cautious default-1-day-until-licence-confirmed posture (`crates/aggregator/src/config.rs`) — this plan does not touch that knob. The **new** `trains` retention knob defaults straight to 30 days, since the RDM licensing question that gates `trust_event_backlog`'s own retention has already been confirmed clear by the repo owner.
- `trains` retention is 30 days (`prune_trains`), reusing the past-dates sibling design's own figure rather than inventing a second number for a structurally similar concern.
- Step D's final schema keeps `train_movement_events.trains_id` and `train_current_state.trains_id` nullable — some rows can never be re-pointed (no natural `train_uid` key to backfill by) and must not force a `NOT NULL`/hard failure.
- `GET /Train/by-uid/{train_uid}/{date}` becomes a public, unauthenticated, unscoped read once this plan's API-surface tasks land — a deliberate, reviewed API-contract change from today's "your own tracked trains only, 404 otherwise."
- Every step in the migration backbone (Steps A-C) is additive/read-only-flip and independently revertible; Step D's final, irreversible act (dropping `tracked_train_id`) is gated on a verified dry-run row-count comparison, never bundled into the same migration that adds `trains_id`.

---

## Task 1: Step A migration — create `trains`, add `tracked_trains.trains_id`

**Files:**
- Create: `crates/api/migrations/20260906100000_trains.sql`
- Test: `crates/api/migrations/20260906100000_trains.sql` (verified by running the migration against a live dev database; no separate `.rs` test file — this repo has no migration-testing framework beyond "the crate's own `sqlx::migrate!` startup path applies it, and every later task's own `db_tests` prove the resulting schema shape")

**Interfaces:**
- Consumes: nothing (first task).
- Produces: table `trains` with columns `id BIGSERIAL PRIMARY KEY, train_uid TEXT NOT NULL, service_date DATE NOT NULL, origin_crs TEXT, scheduled_departure TIMESTAMPTZ, destination_crs TEXT, calling_points JSONB, matched_line_id TEXT, schedule_matched_at TIMESTAMPTZ, train_id TEXT, resolved_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`, `UNIQUE (train_uid, service_date)`; column `tracked_trains.trains_id BIGINT REFERENCES trains(id) ON DELETE SET NULL`; column `tracked_trains.notifications_enabled BOOLEAN NOT NULL DEFAULT TRUE`. Every later task that reads/writes `trains` or `tracked_trains.trains_id` depends on this migration having applied.

- [ ] **Step 1: Write the failing test**

There is no unit test to write for a bare schema migration in this repo's own convention (confirmed: no `.sql` file in `crates/api/migrations` has a co-located `_test.rs`; migrations are proven by the next Rust code that queries the new shape). Instead, write a throwaway verification query and run it manually against a scratch database to prove the migration is syntactically valid and applies cleanly:

```bash
psql "$DATABASE_URL" -c "SELECT to_regclass('public.trains');"
```

Expected: this returns `NULL` (table does not exist yet) before Step 3, confirming there is something real to build.

- [ ] **Step 2: Run test to verify it fails**

Run: `psql "$DATABASE_URL" -c "SELECT to_regclass('public.trains');"`
Expected: prints `NULL` — the `trains` table does not exist yet.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906100000_trains.sql
-- -------------------------------------------------------------------------
-- Shared Train Identity, Step A (expand): a train becomes a shared, public
-- entity keyed by real-world identity (train_uid, service_date) rather than
-- by whichever user pinned it first. See
-- docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §1-2.
--
-- This migration is purely additive: it creates the new `trains` table and
-- adds a nullable FK column to the existing `tracked_trains` table. No
-- existing column is touched, no existing read path changes behavior.
-- -------------------------------------------------------------------------

CREATE TABLE trains (
    id                  BIGSERIAL PRIMARY KEY,
    train_uid           TEXT NOT NULL,
    service_date        DATE NOT NULL,

    -- Schedule-derived (mirrors tracked_trains' own schedule-match columns,
    -- 20260905150000_schedule_matched_resolution.sql).
    origin_crs          TEXT,
    scheduled_departure TIMESTAMPTZ,
    destination_crs     TEXT,
    calling_points      JSONB,
    matched_line_id     TEXT,
    schedule_matched_at TIMESTAMPTZ,

    -- Live-TRUST-derived. train_id is TRUST's own daily identifier string --
    -- NOT the same concept as this table's own surrogate `id` column (see
    -- this plan's Global Constraints on the trains_id naming convention).
    train_id            TEXT,
    resolved_at         TIMESTAMPTZ,

    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (train_uid, service_date)
);

ALTER TABLE tracked_trains
    ADD COLUMN trains_id BIGINT REFERENCES trains(id) ON DELETE SET NULL;

CREATE INDEX tracked_trains_trains_id ON tracked_trains (trains_id);

-- New product surface, schema-only in this migration: no per-subscription
-- mute/opt-in flag exists anywhere in this schema today. Wiring this into
-- notifier's send decision needs explicit product-owner sign-off before it
-- ships (see the design spec's §1) -- this column is added now only so the
-- Step D rename (a later task) doesn't need its own separate migration for
-- it. DEFAULT TRUE preserves today's implicit "every subscription notifies"
-- behavior for every existing and new row.
ALTER TABLE tracked_trains
    ADD COLUMN notifications_enabled BOOLEAN NOT NULL DEFAULT TRUE;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `psql "$DATABASE_URL" -c "SELECT to_regclass('public.trains');"` (the migration is applied automatically on `api`'s next startup via `sqlx::migrate!`, or manually via `sqlx migrate run --source crates/api/migrations`)
Expected: prints `trains` (the table now exists). Also run `cargo test -p api` to confirm nothing else in the crate broke.

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906100000_trains.sql
git commit -m "Add trains table and tracked_trains.trains_id/notifications_enabled (Step A)"
```

## Task 2: `trains` identity primitives (`find_or_create_train`, `mark_train_resolved`)

**Files:**
- Create: `crates/api/src/data/trains.rs`
- Modify: `crates/api/src/data/mod.rs:1-18` (add `pub mod trains;`)
- Test: `crates/api/src/data/trains.rs` (co-located `#[cfg(test)] mod db_tests`, matching this crate's own convention — see `crates/api/src/data/trust_event_backlog.rs`'s `db_tests`)

**Interfaces:**
- Consumes: table `trains` from Task 1.
- Produces: `pub async fn find_or_create_train(pool: &PgPool, train_uid: &str, service_date: chrono::NaiveDate) -> anyhow::Result<i64>`; `pub async fn mark_train_resolved(pool: &PgPool, trains_id: i64, train_id: &str) -> anyhow::Result<()>`. Both are called by Tasks 3, 4, 5, 6, 21.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/trains.rs
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
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api find_or_create_train_returns_the_same_id_on_a_repeat_call -- --ignored`
Expected: FAIL with a compile error — `find_or_create_train` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/api/src/data/trains.rs
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
```

```rust
// crates/api/src/data/mod.rs -- add one line to the existing module list
pub mod trains;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api find_or_create_train_returns_the_same_id_on_a_repeat_call mark_train_resolved_sets_train_id_and_resolved_at -- --ignored --test-threads=1`
Expected: PASS (both tests).

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/trains.rs crates/api/src/data/mod.rs
git commit -m "Add find_or_create_train/mark_train_resolved identity primitives"
```

## Task 3: Dual-write on schedule match (`attempt_schedule_match`)

**Files:**
- Modify: `crates/api/src/data/schedule_matching.rs:94-155` (`attempt_schedule_match`)
- Test: `crates/api/src/data/schedule_matching.rs` (extends the existing `db_tests` module)

**Interfaces:**
- Consumes: `trains::find_or_create_train_with_schedule_match(pool, train_uid, service_date, origin_crs, scheduled_departure, destination_crs, matched_line_id, calling_points) -> anyhow::Result<i64>` (Task 2); existing `train_tracking::apply_schedule_match` (unchanged signature).
- Produces: after this task, a successful schedule match also leaves `tracked_trains.trains_id` set to the shared `trains` row's id, and that `trains` row carries `train_uid`/`origin_crs`/`scheduled_departure`/`destination_crs`/`matched_line_id`/`calling_points`/`schedule_matched_at`. No other task depends on a new function name here — this is a body-only change.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/schedule_matching.rs -- add to the existing `db_tests` module
#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            attempt_schedule_match_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`"]
async fn attempt_schedule_match_also_dual_writes_the_shared_trains_row() {
    let pool = connect().await;
    let user_id = "TEST-SCHEDULE-MATCH-DUAL-WRITE";
    sqlx::query(
        "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind("dual-write@example.com")
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("seed fixture user");

    sqlx::query(
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
         VALUES ('TEST-DW-STANOX', 'EUS', 'EUSTON', 'LONDON EUSTON', 1) \
         ON CONFLICT (stanox) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("seed stanox_crs");

    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    sqlx::query(
        "INSERT INTO schedule_line_population (line_id, service_date, population) \
         VALUES ('west-coast-main-line', $1, $2) \
         ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
    )
    .bind(service_date)
    .bind(population_json("TEST-DW-UID", "EUSTON ", "19:15"))
    .execute(&pool)
    .await
    .expect("seed schedule_line_population");

    let scheduled_departure: chrono::DateTime<chrono::Utc> =
        "2026-09-06T19:15:00+01:00".parse().unwrap();
    let (tracked_train_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(user_id)
    .bind(service_date)
    .bind("EUS")
    .bind(scheduled_departure)
    .fetch_one(&pool)
    .await
    .expect("seed fixture tracked_trains row");

    let mut crs_line_index = HashMap::new();
    crs_line_index.insert("EUS".to_string(), vec!["west-coast-main-line".to_string()]);

    let matched = attempt_schedule_match(
        &pool,
        tracked_train_id,
        "EUS",
        scheduled_departure,
        service_date,
        &crs_line_index,
    )
    .await
    .expect("attempt schedule match");
    assert!(matched);

    let (trains_id,): (Option<i64>,) =
        sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
            .bind(tracked_train_id)
            .fetch_one(&pool)
            .await
            .expect("read back trains_id");
    let trains_id = trains_id.expect("a successful schedule match must set trains_id");

    let (train_uid, matched_line_id): (String, Option<String>) =
        sqlx::query_as("SELECT train_uid, matched_line_id FROM trains WHERE id = $1")
            .bind(trains_id)
            .fetch_one(&pool)
            .await
            .expect("read back the shared trains row");
    assert_eq!(train_uid, "TEST-DW-UID");
    assert_eq!(matched_line_id, Some("west-coast-main-line".to_string()));

    sqlx::query("DELETE FROM schedule_line_population WHERE line_id = 'west-coast-main-line' AND service_date = $1")
        .bind(service_date)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-DW-STANOX'")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-DW-UID'")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api attempt_schedule_match_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: FAIL — `trains_id` on `tracked_trains` stays `NULL` after a successful match, since `attempt_schedule_match` doesn't write it yet.

- [ ] **Step 3: Write minimal implementation**

Replace the `return train_tracking::apply_schedule_match(...)` tail of `attempt_schedule_match` (`crates/api/src/data/schedule_matching.rs:143-151`) with:

```rust
        let matched_ok = train_tracking::apply_schedule_match(
            pool,
            tracked_train_id,
            &matched.uid,
            line_id,
            &calling_points_json,
            destination_crs.as_deref(),
        )
        .await?;

        if matched_ok {
            // Step A dual-write (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
            // §2 Step A): mirror this schedule match onto the shared `trains`
            // row too, and point this subscription at it.
            let trains_id = crate::data::trains::find_or_create_train_with_schedule_match(
                pool,
                &matched.uid,
                service_date,
                pin_origin_crs,
                pin_scheduled_departure,
                destination_crs.as_deref(),
                line_id,
                &calling_points_json,
            )
            .await?;
            sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
                .bind(tracked_train_id)
                .bind(trains_id)
                .execute(pool)
                .await?;
        }

        return Ok(matched_ok);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api attempt_schedule_match_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: PASS. Also re-run the two pre-existing tests in this module (`attempt_schedule_match_reproduces_the_eus_bug_and_now_resolves_it`, `attempt_schedule_match_with_no_candidate_line_leaves_the_row_pending`) to confirm no regression: `DATABASE_URL=... cargo test -p api attempt_schedule_match -- --ignored --test-threads=1`.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/schedule_matching.rs
git commit -m "Dual-write schedule matches onto the shared trains row"
```

## Task 4: Dual-write on backlog match (`attempt_backlog_match`)

**Files:**
- Modify: `crates/api/src/data/trust_event_backlog_match.rs:338-358` (`attempt_backlog_match`)
- Test: `crates/api/src/data/trust_event_backlog_match.rs` (extends the existing `db_tests` module)

**Interfaces:**
- Consumes: `trains::find_or_create_train(pool, train_uid, service_date) -> anyhow::Result<i64>` (Task 2).
- Produces: after this task, a successful backlog match that also found an Activation's `train_uid` leaves `tracked_trains.trains_id` set on the shared `trains` row. A backlog match with no `train_uid` found (Activation missing from the retention window) leaves `trains_id` `NULL`, matching the accepted gap.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/trust_event_backlog_match.rs -- add to the existing `db_tests` module
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            a_backlog_match_with_an_activation_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`"]
async fn a_backlog_match_with_an_activation_also_dual_writes_the_shared_trains_row() {
    let pool = connect().await;
    let user_id = "TEST-BACKLOG-DUAL-WRITE-USER";
    sqlx::query(
        "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind("backlog-dual-write@example.com")
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("seed fixture user");

    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let scheduled: DateTime<Utc> = "2026-09-06T18:15:00Z".parse().unwrap();

    sqlx::query(
        "INSERT INTO trust_event_backlog \
            (crs, train_uid, train_id, service_date, msg_type, event_type, \
             planned_timestamp, actual_timestamp, variation_status, dedup_key) \
         VALUES (NULL, $1, $2, $3, '0001', NULL, NULL, NULL, NULL, $4), \
                ($5, NULL, $2, $3, '0003', 'DEPARTURE', $6, $6, 'ON TIME', $7)",
    )
    .bind("TEST-DW-BACKLOG-UID")
    .bind("TEST-DW-BACKLOG-TRAIN-ID")
    .bind(service_date)
    .bind("test-dw-backlog-dedup-activation")
    .bind("EUS")
    .bind(scheduled)
    .bind("test-dw-backlog-dedup-movement")
    .execute(&pool)
    .await
    .expect("seed backlog rows");

    let (tracked_train_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(user_id)
    .bind(service_date)
    .bind("EUS")
    .bind(scheduled)
    .fetch_one(&pool)
    .await
    .expect("seed tracked_trains row");

    let matched = attempt_backlog_match(&pool, tracked_train_id, "EUS", scheduled, service_date)
        .await
        .expect("attempt_backlog_match");
    assert!(matched);

    let (trains_id,): (Option<i64>,) =
        sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
            .bind(tracked_train_id)
            .fetch_one(&pool)
            .await
            .expect("read back trains_id");
    let trains_id = trains_id.expect("a backlog match with a found Activation must set trains_id");

    let (train_uid,): (String,) = sqlx::query_as("SELECT train_uid FROM trains WHERE id = $1")
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("read back the shared trains row");
    assert_eq!(train_uid, "TEST-DW-BACKLOG-UID");

    sqlx::query("DELETE FROM tracked_trains WHERE id = $1").bind(tracked_train_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-DW-BACKLOG-UID'").execute(&pool).await.ok();
    sqlx::query("DELETE FROM trust_event_backlog WHERE train_id = 'TEST-DW-BACKLOG-TRAIN-ID'").execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api a_backlog_match_with_an_activation_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: FAIL — `trains_id` stays `NULL` after a successful backlog match.

- [ ] **Step 3: Write minimal implementation**

Replace the tail of `attempt_backlog_match` (`crates/api/src/data/trust_event_backlog_match.rs:344-357`) with:

```rust
    let Some((train_id, train_uid)) =
        find_backlog_match(pool, pin_origin_crs, pin_scheduled_departure).await?
    else {
        return Ok(false);
    };

    let history = fetch_backlog_history(pool, &train_id, service_date).await?;
    if history.is_empty() {
        return Ok(false);
    }

    replay_backlog_history(pool, tracked_train_id, train_uid.as_deref(), history).await?;

    // Step A dual-write (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
    // §2 Step A): only possible when this backlog carried an Activation for
    // this train_id (train_uid is Some) -- a Movement/Cancellation-only
    // backfill has no natural key to create a trains row against, matching
    // Step B's own accepted gap.
    if let Some(train_uid) = &train_uid {
        let trains_id = crate::data::trains::find_or_create_train(pool, train_uid, service_date).await?;
        crate::data::trains::mark_train_resolved(pool, trains_id, &train_id).await?;
        sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
            .bind(tracked_train_id)
            .bind(trains_id)
            .execute(pool)
            .await?;
    }

    Ok(true)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api a_backlog_match_with_an_activation_also_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: PASS. Also re-run the two pre-existing tests in this module: `DATABASE_URL=... cargo test -p api attempt_backlog_match -- --ignored --test-threads=1`.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/trust_event_backlog_match.rs
git commit -m "Dual-write backlog matches onto the shared trains row"
```

## Task 5: Dual-write on live-TRUST resolution (`upsert_train_event`)

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs:394-477` (`upsert_train_event`)
- Test: `crates/api/src/data/train_tracking.rs` (new `#[cfg(test)] mod db_tests`, matching this crate's own convention)

**Interfaces:**
- Consumes: `trains::find_or_create_train`, `trains::mark_train_resolved` (Task 2).
- Produces: `upsert_train_event`'s signature is unchanged (`pool: &PgPool, event: &TrainMovementEventMessage) -> anyhow::Result<()>`); after this task, a live TRUST Movement/Cancellation that resolves a pin **and** carries a `resolved_train_uid` also leaves `tracked_trains.trains_id` set on the shared `trains` row. A resolution with no `train_uid` known (this process never saw the Activation) leaves `trains_id` `NULL`, matching Step B's accepted gap.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/train_tracking.rs -- new module, appended at the end of the file
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
                a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`"]
    async fn a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row() {
        let pool = connect().await;
        let user_id = "TEST-LIVE-RESOLUTION-DUAL-WRITE";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("live-resolution@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("WAT")
        .bind(service_date.and_hms_opt(18, 32, 0).unwrap().and_utc())
        .fetch_one(&pool)
        .await
        .expect("seed tracked_trains row");

        let event = common::TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: Some("TEST-LIVE-UID".to_string()),
            resolved_train_id: Some("221832406".to_string()),
            dedup_key: "test-live-dual-write-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        };

        upsert_train_event(&pool, &event).await.expect("upsert_train_event");

        let (trains_id,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        let trains_id = trains_id.expect("a resolution with a known train_uid must set trains_id");

        let (train_uid, train_id): (String, Option<String>) =
            sqlx::query_as("SELECT train_uid, train_id FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back the shared trains row");
        assert_eq!(train_uid, "TEST-LIVE-UID");
        assert_eq!(train_id, Some("221832406".to_string()));

        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1").bind(user_id).execute(&pool).await.ok();
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-LIVE-UID'").execute(&pool).await.ok();
        sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&pool).await.ok();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: FAIL — `trains_id` stays `NULL` after `upsert_train_event` resolves the pin.

- [ ] **Step 3: Write minimal implementation**

Replace `upsert_train_event`'s resolution branch and closing (`crates/api/src/data/train_tracking.rs:415-427` and `:475-477`) so the full function reads:

```rust
pub async fn upsert_train_event(
    pool: &PgPool,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    // Captured here (inside the transaction, via RETURNING) so the Step A
    // dual-write below can run after commit without re-deriving what this
    // UPDATE already computed.
    let mut freshly_resolved: Option<(Option<String>, chrono::NaiveDate, String)> = None;

    if let Some(train_id) = &event.resolved_train_id {
        let row: Option<(Option<String>, chrono::NaiveDate)> = sqlx::query_as(
            "UPDATE tracked_trains \
             SET train_uid = COALESCE($2, train_uid), train_id = $3, \
                 resolution_status = 'resolved', resolved_at = NOW() \
             WHERE id = $1 \
             RETURNING train_uid, service_date",
        )
        .bind(event.tracked_train_id)
        .bind(&event.resolved_train_uid)
        .bind(train_id)
        .fetch_optional(&mut *tx)
        .await?;
        freshly_resolved = row.map(|(train_uid, service_date)| (train_uid, service_date, train_id.clone()));
    }

    sqlx::query(
        "INSERT INTO train_movement_events \
            (tracked_train_id, dedup_key, msg_type, event_type, loc_stanox, loc_crs, \
             planned_timestamp, actual_timestamp, variation_status, raw_body) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING",
    )
    .bind(event.tracked_train_id)
    .bind(&event.dedup_key)
    .bind(&event.msg_type)
    .bind(&event.event_type)
    .bind(&event.loc_stanox)
    .bind(&event.loc_crs)
    .bind(event.planned_timestamp)
    .bind(event.actual_timestamp)
    .bind(&event.variation_status)
    .bind(&event.raw_body)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO train_current_state \
            (tracked_train_id, status, last_reported_location, last_event_type, \
             delay_minutes, next_calling_point, eta_next, eta_source, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW()) \
         ON CONFLICT (tracked_train_id) DO UPDATE SET \
            status                  = EXCLUDED.status, \
            last_reported_location  = EXCLUDED.last_reported_location, \
            last_event_type         = EXCLUDED.last_event_type, \
            delay_minutes            = EXCLUDED.delay_minutes, \
            next_calling_point       = EXCLUDED.next_calling_point, \
            eta_next                 = EXCLUDED.eta_next, \
            eta_source               = EXCLUDED.eta_source, \
            updated_at               = NOW()",
    )
    .bind(event.tracked_train_id)
    .bind(&event.status)
    .bind(&event.last_reported_location)
    .bind(&event.last_event_type)
    .bind(event.delay_minutes)
    .bind(&event.next_calling_point)
    .bind(event.eta_next)
    .bind(&event.eta_source)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Step A dual-write (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
    // §2 Step A), best-effort outside the transaction above -- every write
    // here is itself an idempotent upsert, so atomicity with the legacy
    // write isn't required (a failure here just retries cleanly on the
    // next event for this train).
    if let Some((Some(train_uid), service_date, train_id)) = freshly_resolved {
        let trains_id = crate::data::trains::find_or_create_train(pool, &train_uid, service_date).await?;
        crate::data::trains::mark_train_resolved(pool, trains_id, &train_id).await?;
        sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
            .bind(event.tracked_train_id)
            .bind(trains_id)
            .execute(pool)
            .await?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Dual-write live-TRUST pin resolution onto the shared trains row"
```

## Task 6: Backfill diagnostic — count the un-repointable edge case before Step B runs

**This task deliberately does not follow the TDD template below it.** There
is no code to write: this is the pre-implementation-planning decision the
design spec's Open Question 1 and §2 Step B both name explicitly — "someone
should run `SELECT count(*) ...` against real production data, so the size
of the gap being accepted is known rather than assumed" — made into a real,
executable step, per the writing-plans skill's own rule that a decision
only the repo owner can make blocks a task until it's turned into one.

**Files:** none changed.

**Interfaces:** produces one number, consumed by a human decision before
Task 7 runs: whether "leave these rows permanently un-repointed" (the
spec's own recommendation) is safe to treat as final, or whether the count
is large enough to justify a different backfill strategy first. This task
does not change that recommendation itself — it only supplies the number
the recommendation was conditioned on.

- [ ] **Step 1: Run the diagnostic query against production**

```bash
psql "$DATABASE_URL" -c "SELECT count(*) FROM tracked_trains WHERE train_id IS NOT NULL AND train_uid IS NULL;"
```

This is the exact query named by both the design spec's §2 Step B and its
Open Question 1 — rows resolved via a live TRUST Movement alone, with no
schedule match ever having run, which therefore have no natural
`(train_uid, service_date)` key for Step B's `find_or_create_train`-based
backfill (Task 7) to key off.

- [ ] **Step 2: Record and report the count**

Report the number back into this plan's execution record (e.g. as a
comment on the tracking issue/PR for this plan, or a note appended to this
file's own checklist) before Task 7 is executed. Two outcomes:

- **Small (low tens or fewer) relative to total tracked-train volume**:
  proceed with Task 7 exactly as written — leave these rows' `trains_id`
  permanently `NULL`, matching the spec's own recommendation.
- **Large enough to be a real fraction of all resolved rows**: stop before
  Task 7 and flag this back to the repo owner — a different backfill
  strategy (e.g. synthesizing a placeholder `trains` identity keyed on
  `train_id` alone) would need its own design pass, which is out of scope
  for this plan to invent unprompted.

- [ ] **Step 3: No commit for this task**

Nothing was changed in the repository — there is no commit. Proceed to
Task 7 once the count has been recorded and the "leave it permanently
un-repointed" posture is confirmed acceptable.

## Task 7: Step B — one-off idempotent backfill of existing resolved rows

**Files:**
- Modify: `crates/api/src/data/trains.rs` (new `#[ignore]`d test in the
  existing `db_tests` module — this repo's own established convention for
  a one-off, human-triggered database job; confirmed directly that no
  `bin/`-style one-shot job target exists anywhere in this workspace, per
  the design spec's own §2 Step B note)
- Test: same file, same test (this task's "test" IS the job — there is no
  separate implementation to write afterward, since the job's entire
  purpose is a one-time production side effect, not a piece of
  reusable library code)

**Interfaces:**
- Consumes: `trains::find_or_create_train` (Task 2).
- Produces: every existing `tracked_trains` row with `train_uid IS NOT
  NULL` gets `trains_id` set to its matching `trains` row's id (creating
  that row if it doesn't exist yet). Rows with `train_id IS NOT NULL AND
  train_uid IS NULL` (Task 6's counted edge case) are left with `trains_id
  = NULL`, permanently, per the spec's own accepted-gap recommendation.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/trains.rs -- appended to the existing `db_tests` module
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
```

- [ ] **Step 2: Run test to verify it fails**

This test cannot "fail" in the usual red/green sense before any
implementation exists, since the loop body above IS the implementation,
inline. Instead, confirm the PRECONDITION assertion
(`assert_eq!(trains_id_before, None, ...)`) is the only thing exercised by
running the test body up to that point manually (e.g. temporarily
`return`ing right after it) — this is the "something real to build" proof
the template's Step 2 exists to establish, adapted for a one-off job that
has no separate not-yet-implemented function to call.

- [ ] **Step 3: This job's implementation is what Step 1 already wrote**

No separate step: unlike every other task in this plan, there is no
library function to extract this loop into (nothing else in this codebase
will ever call it again after this job has run once against production).
The `#[ignore]`d test body above is the complete implementation.

- [ ] **Step 4: Run the real job against production and verify it passes**

Run: `DATABASE_URL=<production> cargo test -p api run_step_b_backfill_of_existing_resolved_rows -- --ignored --test-threads=1 --nocapture`
Expected: PASS. Then independently verify the migration's own exit
condition directly: `psql "$DATABASE_URL" -c "SELECT count(*) FROM tracked_trains WHERE train_uid IS NOT NULL AND trains_id IS NULL;"` returns `0` — this is Step C's own precondition (Task 8), so confirming it here unblocks that task.

- [ ] **Step 5: Commit**

Nothing in the repository changes as a result of running this job against
a real database — there is no application code to commit. If Step 1's test
code itself is kept in the tree (recommended, so a fresh environment or a
disaster-recovery restore can re-run the exact same backfill), commit it:
```bash
git add crates/api/src/data/trains.rs
git commit -m "Add Step B one-off backfill job for pre-existing resolved tracked_trains rows"
```

## Task 8: Step C — cut identity/schedule reads over to the shared `trains` row

**Precondition (verify before starting this task):**
```bash
psql "$DATABASE_URL" -c "SELECT count(*) FROM tracked_trains WHERE train_uid IS NOT NULL AND trains_id IS NULL;"
```
must return `0` (Task 7 must have completed, and every dual-write from
Tasks 3-5 must have been running in production long enough to catch every
row created since). If this is non-zero, do not proceed — re-run Task 7's
backfill first.

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs` (`TRACKED_TRAIN_STATE_SELECT`, `list_tracked_trains_for_user`)
- Modify: `crates/api/src/routes/train.rs` (`get_by_uid_and_date`'s own appended `WHERE` clause)
- Test: `crates/api/src/data/train_tracking.rs` (new `#[cfg(test)] mod db_tests`, matching Task 5's convention of adding this module fresh to this file)

**Interfaces:**
- Consumes: `tracked_trains.trains_id` (Task 1), the `trains` table (Task 1).
- Produces: `get_by_tracking_id`, `get_by_uid_and_date` (still ownership-scoped at this point — Task 19 changes that separately), and `list_tracked_trains_for_user` now read `train_uid`/`train_id`/`schedule_destination_crs`/`schedule_calling_points` from the joined `trains` row instead of `tracked_trains`' own duplicate columns. `TrackedTrainState`/`TrackedTrainListItem`'s own public shapes are unchanged — this is a body-only, read-path-only change, trivially revertible by reverting the query text alone (per the design spec's own Rollback posture for Step C).

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/train_tracking.rs -- new module, appended at the end of the file
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
                get_by_tracking_id_reads_identity_from_the_joined_trains_row -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_reads_identity_from_the_joined_trains_row_not_tracked_trains_own_stale_column()
    {
        let pool = connect().await;
        let user_id = "TEST-STEP-C-CUTOVER";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("step-c-cutover@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "STEPC-UID", service_date)
            .await
            .expect("find_or_create_train");

        // The row's OWN train_uid column is deliberately wrong -- proving
        // the read below trusts the joined `trains` row, not this column.
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, train_uid) \
             VALUES ($1, $2, 'EUS', $3, $4, 'STALE-WRONG-UID') RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(service_date.and_hms_opt(19, 15, 0).unwrap().and_utc())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed tracked_trains row with a stale own train_uid");

        let state = get_by_tracking_id(&pool, tracked_train_id)
            .await
            .expect("get_by_tracking_id")
            .expect("row exists");
        assert_eq!(
            state.train_uid,
            Some("STEPC-UID".to_string()),
            "must read train_uid from the joined trains row, not tracked_trains' own stale column"
        );

        sqlx::query("DELETE FROM tracked_trains WHERE id = $1")
            .bind(tracked_train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api get_by_tracking_id_reads_identity_from_the_joined_trains_row -- --ignored --test-threads=1`
Expected: FAIL — today's `TRACKED_TRAIN_STATE_SELECT` still selects `tt.train_uid` directly, so the assertion sees `"STALE-WRONG-UID"` instead of `"STEPC-UID"`.

- [ ] **Step 3: Write minimal implementation**

Before (`crates/api/src/data/train_tracking.rs:598-611`):
```rust
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tt.train_uid, tt.train_id, \
           tt.schedule_destination_crs, ssd.name AS schedule_destination_name, \
           tt.schedule_calling_points, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM tracked_trains tt \
    LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
    LEFT JOIN stations ssd ON ssd.crs = UPPER(tt.schedule_destination_crs)";
```

After:
```rust
// `LEFT JOIN trains tr`: Step C of
// docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §2 --
// train_uid/train_id/schedule_destination_crs/schedule_calling_points now
// come from the shared trains row, not tracked_trains' own duplicate
// columns (those columns still physically exist and are still written by
// Tasks 3-5's dual-write and by apply_schedule_match until Task 21 retires
// those writes -- this is a READ-only flip). `cs` still joins on
// `tracked_train_id` here -- train_current_state isn't re-pointed to
// trains_id until Step D (Task 11).
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tr.train_uid, tr.train_id, \
           tr.destination_crs AS schedule_destination_crs, ssd.name AS schedule_destination_name, \
           tr.calling_points AS schedule_calling_points, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM tracked_trains tt \
    LEFT JOIN trains tr ON tr.id = tt.trains_id \
    LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
    LEFT JOIN stations ssd ON ssd.crs = UPPER(tr.destination_crs)";
```

`get_by_uid_and_date` (`crates/api/src/data/train_tracking.rs:696-709`) uses this same const via `format!`, so its `SELECT` list is already fixed by the change above; only its own appended `WHERE` clause needs updating, from `tt.train_uid = $1` to `tr.train_uid = $1`:
```rust
pub async fn get_by_uid_and_date(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> anyhow::Result<Option<TrackedTrainState>> {
    let row = sqlx::query_as::<_, TrackedTrainState>(&format!(
        "{TRACKED_TRAIN_STATE_SELECT} WHERE tr.train_uid = $1 AND tt.service_date = $2"
    ))
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

`list_tracked_trains_for_user` (`crates/api/src/data/train_tracking.rs:663-679`): same join added, `tt.train_uid` becomes `tr.train_uid`:
```rust
pub async fn list_tracked_trains_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<TrackedTrainListItem>> {
    let rows = sqlx::query_as::<_, TrackedTrainListItem>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                tt.pin_scheduled_departure, tt.resolution_status, tr.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at, tt.custom_name \
         FROM tracked_trains tt \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
         WHERE tt.user_id = $1 \
         ORDER BY tt.tracked_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api get_by_tracking_id_reads_identity_from_the_joined_trains_row -- --ignored --test-threads=1`
Expected: PASS. Also re-run every existing test in `schedule_matching.rs`,
`trust_event_backlog_match.rs`, and `routes/train.rs` touching
`get_by_tracking_id`/`get_by_uid_and_date` to confirm no regression (they
still assert against `state.train_uid` etc., which now resolves through
the join and must still match, since Tasks 3-5's dual-write keeps both
locations in sync).

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Step C: read train identity/schedule columns from the shared trains row"
```

## Task 9: Step D migration — add `trains_id` to `train_movement_events`/`train_current_state`

**Files:**
- Create: `crates/api/migrations/20260906110000_train_movement_trains_id.sql`

**Interfaces:**
- Consumes: table `trains` (Task 1).
- Produces: nullable `train_movement_events.trains_id`, a partial unique
  index `(trains_id, dedup_key) WHERE trains_id IS NOT NULL` alongside the
  existing `(tracked_train_id, dedup_key)` one; nullable
  `train_current_state.trains_id` with its own partial unique index; a new
  surrogate `train_current_state.id` primary key, since `tracked_train_id`
  stops being able to serve as this table's `NOT NULL` primary key once a
  row can exist for a `trains_id` with zero subscribers. Every later task
  in this plan that writes or reads either table via `trains_id` depends
  on this migration.

- [ ] **Step 1: Write the failing test**

```bash
psql "$DATABASE_URL" -c "SELECT column_name FROM information_schema.columns WHERE table_name = 'train_movement_events' AND column_name = 'trains_id';"
```
Expected: zero rows (the column does not exist yet).

- [ ] **Step 2: Run test to verify it fails**

Run the command above. Expected: empty result.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906110000_train_movement_trains_id.sql
-- -------------------------------------------------------------------------
-- Shared Train Identity, Step D (re-point), part 1 -- schema only. See
-- docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §2
-- Step D.
-- -------------------------------------------------------------------------

-- train_movement_events: purely additive. The existing NOT NULL
-- tracked_train_id column and its UNIQUE (tracked_train_id, dedup_key)
-- constraint are untouched -- both the old and the new dedup constraint
-- coexist until Task 22's separately-gated final drop removes
-- tracked_train_id entirely.
ALTER TABLE train_movement_events
    ADD COLUMN trains_id BIGINT REFERENCES trains(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX train_movement_events_trains_id_dedup
    ON train_movement_events (trains_id, dedup_key)
    WHERE trains_id IS NOT NULL;

CREATE INDEX train_movement_events_trains_id
    ON train_movement_events (trains_id, received_at)
    WHERE trains_id IS NOT NULL;

-- train_current_state: a genuine PK restructuring, not merely an additive
-- column. Today's PK, tracked_train_id, is NOT NULL by construction (every
-- PRIMARY KEY is) -- but this design's whole point is a row that can exist
-- for a trains_id with ZERO subscribers (the design spec's own "today,
-- train_current_state literally cannot answer 'where is this train' for a
-- train nobody has pinned" framing), which requires inserting a row with
-- no tracked_train_id value at all. A NOT NULL PK can never accommodate
-- that, so tracked_train_id stops being this table's PK here; a new
-- surrogate `id` column takes over, and tracked_train_id becomes a plain
-- nullable column, still unique when present (via its own partial index),
-- so any code not yet migrated to the trains_id path keeps working
-- unchanged against it.
--
-- Deliberate, reasoned deviation from this design spec's own §1 text
-- ("trains_id BIGINT PRIMARY KEY"), reconciled in favor of this plan's own
-- binding Global Constraint ("Step D's final schema keeps ...
-- train_current_state.trains_id nullable ... must not force a NOT
-- NULL/hard failure"): a PRIMARY KEY is always NOT NULL, and Step B's own
-- named edge case (a row with no natural train_uid key to backfill by,
-- confirmed and sized by Task 6) means trains_id can never be guaranteed
-- non-null for every row this table will ever hold. A partial UNIQUE index
-- (WHERE trains_id IS NOT NULL) delivers the same "one row per physical
-- train" guarantee for every row that DOES have one, without requiring the
-- column to be total.
--
-- `IF EXISTS`/explicit-name rather than assumed -- same defensive posture
-- 20260905150000_schedule_matched_resolution.sql already took for an
-- auto-generated constraint name: Postgres names an inline
-- `PRIMARY KEY` constraint `{table}_pkey` by default, confirmed against
-- this table's own original, un-named `tracked_train_id BIGINT PRIMARY KEY`
-- declaration (20260828120000_train_tracking.sql).
ALTER TABLE train_current_state ADD COLUMN id BIGSERIAL;
ALTER TABLE train_current_state DROP CONSTRAINT IF EXISTS train_current_state_pkey;
ALTER TABLE train_current_state ADD PRIMARY KEY (id);
ALTER TABLE train_current_state ALTER COLUMN tracked_train_id DROP NOT NULL;
CREATE UNIQUE INDEX train_current_state_tracked_train_id
    ON train_current_state (tracked_train_id)
    WHERE tracked_train_id IS NOT NULL;

ALTER TABLE train_current_state
    ADD COLUMN trains_id BIGINT REFERENCES trains(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX train_current_state_trains_id
    ON train_current_state (trains_id)
    WHERE trains_id IS NOT NULL;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `psql "$DATABASE_URL" -c "SELECT column_name FROM information_schema.columns WHERE table_name = 'train_movement_events' AND column_name = 'trains_id';"`
Expected: one row, `trains_id`. Also `cargo test -p api` to confirm nothing
else in the crate broke (the old `tracked_train_id`-keyed upsert shape in
today's `upsert_train_event` is untouched by this migration alone — Task
11 is what changes the Rust code).

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906110000_train_movement_trains_id.sql
git commit -m "Step D: add nullable trains_id to train_movement_events/train_current_state"
```

## Task 10: Step D backfill — populate `trains_id` on existing movement rows

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs` (new `#[ignore]`d job
  test, appended to the `db_tests` module Task 8 just added)

**Interfaces:**
- Consumes: `tracked_trains.trains_id` (already backfilled by Task 7),
  `train_movement_events`/`train_current_state.trains_id` (Task 9).
- Produces: every `train_movement_events`/`train_current_state` row whose
  owning `tracked_trains.trains_id` is set gets its own `trains_id`
  populated to match. Rows whose owning subscription's `trains_id` is
  itself `NULL` (Task 6/7's named edge case) stay `NULL` permanently, per
  the design spec's own Step D text.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/train_tracking.rs -- appended to db_tests (Task 8)
#[tokio::test]
#[ignore = "one-off Step D production backfill job, not a repeatable unit test; \
            run manually, exactly once per environment, with \
            `DATABASE_URL=... cargo test -p api run_step_d_backfill_of_movement_tables \
            -- --ignored --test-threads=1 --nocapture` -- run AFTER Task 7's backfill"]
async fn run_step_d_backfill_of_movement_tables() {
    let pool = connect().await;
    let user_id = "TEST-STEP-D-BACKFILL";
    sqlx::query(
        "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind("step-d-backfill@example.com")
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("seed fixture user");

    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let trains_id = crate::data::trains::find_or_create_train(&pool, "STEPD-UID", service_date)
        .await
        .expect("find_or_create_train");
    let (tracked_train_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id) \
         VALUES ($1, $2, 'EUS', $3, $4) RETURNING id",
    )
    .bind(user_id)
    .bind(service_date)
    .bind(service_date.and_hms_opt(19, 15, 0).unwrap().and_utc())
    .bind(trains_id)
    .fetch_one(&pool)
    .await
    .expect("seed a resolved, already-repointed subscription");

    sqlx::query(
        "INSERT INTO train_movement_events (tracked_train_id, dedup_key, msg_type, raw_body) \
         VALUES ($1, 'test-step-d-dedup', '0003', '{}'::jsonb)",
    )
    .bind(tracked_train_id)
    .execute(&pool)
    .await
    .expect("seed a pre-existing movement row with no trains_id yet");
    sqlx::query(
        "INSERT INTO train_current_state (tracked_train_id, status) VALUES ($1, 'en_route')",
    )
    .bind(tracked_train_id)
    .execute(&pool)
    .await
    .expect("seed a pre-existing current-state row with no trains_id yet");

    // --- the backfill job itself, run inline in this test ---
    loop {
        let result = sqlx::query(
            "WITH batch AS ( \
                SELECT tme.id, tt.trains_id AS new_trains_id \
                FROM train_movement_events tme \
                JOIN tracked_trains tt ON tt.id = tme.tracked_train_id \
                WHERE tme.trains_id IS NULL AND tt.trains_id IS NOT NULL \
                LIMIT 500 \
             ) \
             UPDATE train_movement_events tme SET trains_id = batch.new_trains_id \
             FROM batch WHERE tme.id = batch.id",
        )
        .execute(&pool)
        .await
        .expect("backfill train_movement_events batch");
        if result.rows_affected() == 0 {
            break;
        }
    }
    loop {
        let result = sqlx::query(
            "WITH batch AS ( \
                SELECT cs.id, tt.trains_id AS new_trains_id \
                FROM train_current_state cs \
                JOIN tracked_trains tt ON tt.id = cs.tracked_train_id \
                WHERE cs.trains_id IS NULL AND tt.trains_id IS NOT NULL \
                LIMIT 500 \
             ) \
             UPDATE train_current_state cs SET trains_id = batch.new_trains_id \
             FROM batch WHERE cs.id = batch.id",
        )
        .execute(&pool)
        .await
        .expect("backfill train_current_state batch");
        if result.rows_affected() == 0 {
            break;
        }
    }

    let (event_trains_id,): (Option<i64>,) =
        sqlx::query_as("SELECT trains_id FROM train_movement_events WHERE dedup_key = 'test-step-d-dedup'")
            .fetch_one(&pool)
            .await
            .expect("read back trains_id");
    assert_eq!(event_trains_id, Some(trains_id));

    let (state_trains_id,): (Option<i64>,) = sqlx::query_as(
        "SELECT trains_id FROM train_current_state WHERE tracked_train_id = $1",
    )
    .bind(tracked_train_id)
    .fetch_one(&pool)
    .await
    .expect("read back trains_id");
    assert_eq!(state_trains_id, Some(trains_id));

    sqlx::query("DELETE FROM tracked_trains WHERE id = $1").bind(tracked_train_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api run_step_d_backfill_of_movement_tables -- --ignored --test-threads=1`
Expected: FAIL at the two `assert_eq!` calls — both rows still have `trains_id = NULL` before the batched `UPDATE`s run (which, per Step 1, are written inline as the test body itself, so this "failure" is best verified by commenting out the two loops temporarily, matching Task 7's own adapted Step 2).

- [ ] **Step 3: Write minimal implementation**

Same note as Task 7: the loop bodies in Step 1 ARE the implementation —
there is no separate library function this backfill needs, since nothing
else in this codebase will ever call it again after it has run once
against production.

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=<production> cargo test -p api run_step_d_backfill_of_movement_tables -- --ignored --test-threads=1 --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Add Step D one-off backfill job for train_movement_events/train_current_state"
```

## Task 11: Split `upsert_train_event` into a shared, `trains_id`-keyed write plus a thin legacy resolution flip

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs` (`upsert_train_event`, `list_active_tracked_trains`)
- Test: `crates/api/src/data/train_tracking.rs` (extends the `db_tests` module)

**Interfaces:**
- Consumes: `trains::find_or_create_train`, `trains::mark_train_resolved` (Task 2); `train_movement_events`/`train_current_state.trains_id` (Task 9).
- Produces: `pub async fn upsert_train_movement(pool: &PgPool, trains_id: i64, event: &TrainMovementEventMessage) -> anyhow::Result<()>` — writes `train_movement_events`/`train_current_state` keyed by `trains_id` alone, callable for **any** `trains_id` regardless of whether a subscription exists at all. Consumed by Task 14 (trust-backlog-consumer's ingest route). `upsert_train_event`'s own public signature is unchanged, but it no longer writes `train_movement_events`/`train_current_state` directly — it now resolves (or looks up) the event's `trains_id` and delegates to `upsert_train_movement`. `list_active_tracked_trains`'s `train_current_state` join moves from `tracked_train_id` to `trains_id`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/train_tracking.rs -- appended to db_tests (Tasks 8, 10)
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all \
            -- --ignored --test-threads=1`"]
async fn upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all() {
    let pool = connect().await;
    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let trains_id = crate::data::trains::find_or_create_train(&pool, "NOSUB-UID", service_date)
        .await
        .expect("find_or_create_train");
    // Deliberately: no tracked_trains row is ever created for this trains_id.

    let event = common::TrainMovementEventMessage {
        tracked_train_id: 0, // unused by upsert_train_movement -- see its own doc comment
        resolved_train_uid: None,
        resolved_train_id: None,
        dedup_key: "test-nosub-dedup".to_string(),
        msg_type: "0003".to_string(),
        event_type: Some("DEPARTURE".to_string()),
        loc_stanox: Some("87212".to_string()),
        loc_crs: Some("WAT".to_string()),
        planned_timestamp: None,
        actual_timestamp: None,
        variation_status: None,
        raw_body: serde_json::json!({}),
        status: "en_route".to_string(),
        last_reported_location: Some("WAT".to_string()),
        last_event_type: Some("DEPARTURE".to_string()),
        delay_minutes: Some(0),
        next_calling_point: None,
        eta_next: None,
        eta_source: None,
    };

    upsert_train_movement(&pool, trains_id, &event)
        .await
        .expect("upsert_train_movement for an unsubscribed train");

    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM train_current_state WHERE trains_id = $1")
            .bind(trains_id)
            .fetch_one(&pool)
            .await
            .expect("a current-state row must exist for this trains_id even with zero subscribers");
    assert_eq!(status, "en_route");

    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `upsert_train_movement` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

Replace `upsert_train_event`'s full body (`crates/api/src/data/train_tracking.rs`, the version Task 5 left it in) with:

```rust
/// Writes one TRUST-derived event into the SHARED, per-physical-train
/// tables. Callable for ANY `trains_id`, regardless of whether any
/// `tracked_trains` row (subscription) references it at all -- this is
/// the primary write path once trust-backlog-consumer becomes the primary
/// movement-event writer (Task 14), and it's also what `upsert_train_event`
/// below now delegates to for the legacy per-subscription path.
/// `event.tracked_train_id` is ignored here on purpose -- this function's
/// entire point is to not require one.
pub async fn upsert_train_movement(
    pool: &PgPool,
    trains_id: i64,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO train_movement_events \
            (trains_id, dedup_key, msg_type, event_type, loc_stanox, loc_crs, \
             planned_timestamp, actual_timestamp, variation_status, raw_body) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (trains_id, dedup_key) WHERE trains_id IS NOT NULL DO NOTHING",
    )
    .bind(trains_id)
    .bind(&event.dedup_key)
    .bind(&event.msg_type)
    .bind(&event.event_type)
    .bind(&event.loc_stanox)
    .bind(&event.loc_crs)
    .bind(event.planned_timestamp)
    .bind(event.actual_timestamp)
    .bind(&event.variation_status)
    .bind(&event.raw_body)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO train_current_state \
            (trains_id, status, last_reported_location, last_event_type, \
             delay_minutes, next_calling_point, eta_next, eta_source, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW()) \
         ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET \
            status                  = EXCLUDED.status, \
            last_reported_location  = EXCLUDED.last_reported_location, \
            last_event_type         = EXCLUDED.last_event_type, \
            delay_minutes            = EXCLUDED.delay_minutes, \
            next_calling_point       = EXCLUDED.next_calling_point, \
            eta_next                 = EXCLUDED.eta_next, \
            eta_source               = EXCLUDED.eta_source, \
            updated_at               = NOW()",
    )
    .bind(trains_id)
    .bind(&event.status)
    .bind(&event.last_reported_location)
    .bind(&event.last_event_type)
    .bind(event.delay_minutes)
    .bind(&event.next_calling_point)
    .bind(event.eta_next)
    .bind(&event.eta_source)
    .execute(pool)
    .await?;

    Ok(())
}

/// Legacy per-subscription resolution flip only -- as of this task, it no
/// longer writes `train_movement_events`/`train_current_state` itself
/// (that's `upsert_train_movement`'s job now). Advances
/// `tracked_trains.resolution_status`, mirrors the resolution onto the
/// shared `trains` row (same dual-write Task 5 introduced), and returns
/// the resolved `trains_id` so the caller can feed the same event into
/// `upsert_train_movement`. Returns `None` only in the accepted-gap case:
/// no `trains_id` was already known AND this call carries no
/// `resolved_train_uid` either (this process never saw the Activation) --
/// the pin still flips to `'resolved'` for this user's own tracking
/// purposes, but no shared `trains` row can be created or updated without
/// a known identity.
async fn flip_legacy_resolution(
    pool: &PgPool,
    tracked_train_id: i64,
    resolved_train_uid: Option<&str>,
    resolved_train_id: &str,
) -> anyhow::Result<Option<i64>> {
    let row: Option<(Option<i64>, chrono::NaiveDate)> = sqlx::query_as(
        "UPDATE tracked_trains SET resolution_status = 'resolved', resolved_at = NOW() \
         WHERE id = $1 RETURNING trains_id, service_date",
    )
    .bind(tracked_train_id)
    .fetch_optional(pool)
    .await?;
    let Some((existing_trains_id, service_date)) = row else {
        return Ok(None);
    };

    let trains_id = match (existing_trains_id, resolved_train_uid) {
        (Some(id), _) => Some(id),
        (None, Some(train_uid)) => {
            let id = crate::data::trains::find_or_create_train(pool, train_uid, service_date).await?;
            sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
                .bind(tracked_train_id)
                .bind(id)
                .execute(pool)
                .await?;
            Some(id)
        }
        (None, None) => None,
    };
    if let Some(id) = trains_id {
        crate::data::trains::mark_train_resolved(pool, id, resolved_train_id).await?;
    }
    Ok(trains_id)
}

/// Idempotent, same overall contract as before this task: resolves the pin
/// (if `resolved_train_id` is `Some`) and writes the shared movement/
/// current-state tables. As of this task, that write ALWAYS goes through
/// [`upsert_train_movement`], keyed on `trains_id` -- never directly on
/// `tracked_train_id` -- so an event for a subscription whose identity is
/// still entirely unknown (no schedule/backlog match ever ran, and this
/// call itself carries no `resolved_train_uid`) has nothing to key a
/// shared-table write on and is dropped with a warning, matching the
/// accepted gap `flip_legacy_resolution` documents.
pub async fn upsert_train_event(
    pool: &PgPool,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    let resolved_trains_id = match &event.resolved_train_id {
        Some(train_id) => {
            flip_legacy_resolution(
                pool,
                event.tracked_train_id,
                event.resolved_train_uid.as_deref(),
                train_id,
            )
            .await?
        }
        None => None,
    };

    let trains_id = match resolved_trains_id {
        Some(id) => Some(id),
        None => {
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT trains_id FROM tracked_trains WHERE id = $1",
            )
            .bind(event.tracked_train_id)
            .fetch_optional(pool)
            .await?
            .flatten()
        }
    };

    match trains_id {
        Some(trains_id) => upsert_train_movement(pool, trains_id, event).await?,
        None => {
            tracing::warn!(
                tracked_train_id = event.tracked_train_id,
                "no trains_id known yet for this subscription; movement event dropped \
                 from the shared store until its identity is resolved"
            );
        }
    }

    Ok(())
}
```

Also re-point `list_active_tracked_trains`'s `train_current_state` join
(`crates/api/src/data/train_tracking.rs:371-383`) from `tracked_train_id`
to `trains_id`, since new writes no longer keep the old join's data fresh:

```rust
pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRow>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, tt.train_id \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TrackedTrainRef::from).collect())
}
```

And `TRACKED_TRAIN_STATE_SELECT`/`list_tracked_trains_for_user` (Task 8):
their `cs` join changes from `cs.tracked_train_id = tt.id` to
`cs.trains_id = tt.trains_id`, same reasoning.

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all -- --ignored --test-threads=1`
Expected: PASS. Also re-run Task 5's own
`a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row`
test and every `get_by_tracking_id`/`list_tracked_trains_for_user`-touching
test in `routes/train.rs` to confirm the `cs` join re-point didn't regress
an already-resolved pin's status/delay display.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Split upsert_train_event into a trains_id-keyed write plus a thin legacy resolution flip"
```

## Task 12: `notifier`'s fan-out becomes one-to-many via `trains_id`

**Files:**
- Modify: `crates/notifier/src/queries.rs` (`TrainCandidate`, `poll_train_candidates`)
- Modify: `crates/notifier/src/main.rs` (`current_train_state`'s one call site)
- Test: `crates/notifier/src/queries.rs` (extends whatever `#[cfg(test)]`/`db_tests` module already covers `poll_train_candidates` there)

**Interfaces:**
- Consumes: `train_movement_events.trains_id`, `train_current_state.trains_id` (Task 9, now populated going forward by Task 11's `upsert_train_movement`).
- Produces: `TrainCandidate` gains a `trains_id: i64` field. `poll_train_candidates`'s own return shape (`(Vec<TrainCandidate>, i64)`) and every other field on `TrainCandidate` are unchanged, so `main.rs`'s notification-sending body (URL, tag, `upsert_train_notification_state` call) needs no change beyond the one `current_train_state` call site below.

- [ ] **Step 1: Write the failing test**

```rust
// crates/notifier/src/queries.rs -- new test, alongside however this
// module's own existing tests are organised (mirror their own db_tests
// connect() helper if one already exists in this file; otherwise add one
// matching the exact shape used throughout this plan's other db_tests).
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p notifier \
            poll_train_candidates_fans_out_one_trains_id_to_every_subscriber \
            -- --ignored --test-threads=1`"]
async fn poll_train_candidates_fans_out_one_trains_id_to_every_subscriber() {
    let pool = connect().await;
    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let trains_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO trains (train_uid, service_date) VALUES ('TEST-FANOUT-UID', $1) \
         RETURNING id",
    )
    .bind(service_date)
    .fetch_one(&pool)
    .await
    .expect("seed trains row");

    for user_id in ["TEST-FANOUT-USER-A", "TEST-FANOUT-USER-B"] {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");
        sqlx::query(
            "INSERT INTO tracked_trains \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, resolution_status) \
             VALUES ($1, $2, 'EUS', $3, $4, 'resolved')",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(service_date.and_hms_opt(19, 15, 0).unwrap().and_utc())
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed a subscription pointing at the shared train");
    }

    let (event_id,): (i64,) = sqlx::query_as(
        "INSERT INTO train_movement_events (trains_id, dedup_key, msg_type, raw_body) \
         VALUES ($1, 'test-fanout-dedup', '0003', '{}'::jsonb) RETURNING id",
    )
    .bind(trains_id)
    .fetch_one(&pool)
    .await
    .expect("seed a movement event for the shared train");
    sqlx::query(
        "INSERT INTO train_current_state (trains_id, status, delay_minutes) VALUES ($1, 'en_route', 20)",
    )
    .bind(trains_id)
    .execute(&pool)
    .await
    .expect("seed current state showing a real delay");

    let (candidates, max_id) = poll_train_candidates(&pool, event_id - 1, 15)
        .await
        .expect("poll_train_candidates");
    assert_eq!(max_id, event_id);
    assert_eq!(
        candidates.len(),
        2,
        "one physical train's delay must notify BOTH of its independent subscribers"
    );
    assert!(candidates.iter().all(|c| c.trains_id == trains_id));

    sqlx::query("DELETE FROM tracked_trains WHERE trains_id = $1").bind(trains_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
    for user_id in ["TEST-FANOUT-USER-A", "TEST-FANOUT-USER-B"] {
        sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&pool).await.ok();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p notifier poll_train_candidates_fans_out_one_trains_id_to_every_subscriber -- --ignored --test-threads=1`
Expected: FAIL — today's query does `SELECT DISTINCT tracked_train_id FROM train_movement_events`, so it only ever discovers exactly one `tracked_train_id` per row, never fanning out to a second subscriber pointing at the same `trains_id`; `candidates.len()` is `1`, not `2`, and `TrainCandidate` has no `trains_id` field to compile the assertion against yet.

- [ ] **Step 3: Write minimal implementation**

Before (`crates/notifier/src/queries.rs:122-198`):
```rust
pub struct TrainCandidate {
    pub tracked_train_id: i64,
    pub user_id: String,
    pub new_rank: u8,
    pub previous_rank: u8,
}

pub async fn poll_train_candidates(
    pool: &PgPool,
    since_id: i64,
    delay_threshold_minutes: i32,
) -> anyhow::Result<(Vec<TrainCandidate>, i64)> {
    let touched = sqlx::query("SELECT DISTINCT tracked_train_id FROM train_movement_events WHERE id > $1")
        .bind(since_id)
        .fetch_all(pool)
        .await?;
    // ... (per-tracked_train_id current-state + notification-state lookup, unchanged shape)
}
```

After:
```rust
pub struct TrainCandidate {
    pub tracked_train_id: i64,
    pub trains_id: i64,
    pub user_id: String,
    pub new_rank: u8,
    pub previous_rank: u8,
}

/// Watermark now advances over `train_movement_events.trains_id` (Task 9),
/// the shared, per-physical-train identity -- NOT `tracked_train_id`, which
/// stops being written by new events as of Task 11. One `trains_id` can
/// have MANY independent subscribers (`tracked_trains` rows); this fans out
/// to every one of them, each judged by its OWN cooldown/escalation state
/// in `train_notification_state` (still keyed by `(user_id, tracked_train_id)`
/// -- unchanged, since that's still each user's own private escalation
/// history for their own subscription row).
pub async fn poll_train_candidates(
    pool: &PgPool,
    since_id: i64,
    delay_threshold_minutes: i32,
) -> anyhow::Result<(Vec<TrainCandidate>, i64)> {
    let touched: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT trains_id FROM train_movement_events \
         WHERE id > $1 AND trains_id IS NOT NULL",
    )
    .bind(since_id)
    .fetch_all(pool)
    .await?;

    if touched.is_empty() {
        return Ok((Vec::new(), since_id));
    }

    let max_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM train_movement_events WHERE id > $1")
        .bind(since_id)
        .fetch_one(pool)
        .await?;

    let mut candidates = Vec::new();
    for trains_id in touched {
        let current = sqlx::query(
            "SELECT status, delay_minutes FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_optional(pool)
        .await?;
        let Some(current) = current else { continue }; // no current-state row yet -- nothing to compare

        let status: String = current.try_get("status")?;
        let delay_minutes: Option<i32> = current.try_get("delay_minutes")?;
        let new_rank = train_severity_rank(&status, delay_minutes, delay_threshold_minutes);

        let subscribers = sqlx::query("SELECT id, user_id FROM tracked_trains WHERE trains_id = $1")
            .bind(trains_id)
            .fetch_all(pool)
            .await?;
        for subscriber in subscribers {
            let tracked_train_id: i64 = subscriber.try_get("id")?;
            let user_id: String = subscriber.try_get("user_id")?;

            let previous = sqlx::query(
                "SELECT last_notified_status, last_notified_delay_minutes \
                 FROM train_notification_state WHERE user_id = $1 AND tracked_train_id = $2",
            )
            .bind(&user_id)
            .bind(tracked_train_id)
            .fetch_optional(pool)
            .await?;
            let previous_rank = match previous {
                None => 0,
                Some(previous) => {
                    let previous_status: String = previous.try_get("last_notified_status")?;
                    let previous_delay: Option<i32> = previous.try_get("last_notified_delay_minutes")?;
                    train_severity_rank(&previous_status, previous_delay, delay_threshold_minutes)
                }
            };

            if crate::decision::decide_train_notification(previous_rank, new_rank)
                == crate::decision::NotifyDecision::NotifyNow
            {
                candidates.push(TrainCandidate {
                    tracked_train_id,
                    trains_id,
                    user_id,
                    new_rank,
                    previous_rank,
                });
            }
        }
    }
    Ok((candidates, max_id))
}
```

`crates/notifier/src/main.rs`'s one call site (`current_train_state`,
`main.rs:112,132-135`) switches from `tracked_train_id` to `trains_id`:
```rust
let (status, delay_minutes) = current_train_state(pool, candidate.trains_id).await?;
// ...
async fn current_train_state(pool: &PgPool, trains_id: i64) -> anyhow::Result<(String, Option<i32>)> {
    let row = sqlx::query("SELECT status, delay_minutes FROM train_current_state WHERE trains_id = $1")
        .bind(trains_id)
        // ... unchanged from here
}
```
Every other use of `candidate.tracked_train_id` in `main.rs` (the
notification URL/tag, `upsert_train_notification_state`'s call) is
unchanged — those stay per-subscription, deliberately.

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p notifier poll_train_candidates_fans_out_one_trains_id_to_every_subscriber -- --ignored --test-threads=1`
Expected: PASS. Also `cargo build -p notifier` to confirm `main.rs`'s call
site compiles against the new field.

- [ ] **Step 5: Commit**
```bash
git add crates/notifier/src/queries.rs crates/notifier/src/main.rs
git commit -m "notifier: fan out one trains_id's movement event to every one of its subscribers"
```

## Task 13: `trust-backlog-consumer` — close the live-ingest-time `train_uid` correlation gap

**Files:**
- Modify: `crates/trust-backlog-consumer/src/process.rs` (`ProcessorState`, `process_message`)
- Test: `crates/trust-backlog-consumer/src/process.rs` (extends the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing new (uses the existing `common::TrustBacklogEventMessage.train_uid: Option<String>` field, already present).
- Produces: every outgoing Movement/Cancellation `TrustBacklogEventMessage` for a `train_id` whose Activation this process has seen now carries a real `train_uid`, closing the gap the design spec's §3 names explicitly. Consumed by Task 14 (`api`'s ingest handler needs a real `train_uid` to key the shared-store write on).

- [ ] **Step 1: Write the failing test**

```rust
// crates/trust-backlog-consumer/src/process.rs -- appended to the existing `mod tests`
#[test]
fn a_movement_after_a_parked_activation_carries_the_real_train_uid() {
    let activation_msg = TrustMessage::Activation(activation("221832406", "C21373", "2026-09-05"));
    let mut state = ProcessorState::default();
    process_message(
        &activation_msg,
        &mut state,
        &stanox_table(),
        &crs_index_with(&["WAT"]),
        today(),
    );

    let movement_msg = TrustMessage::Movement(movement(
        "221832406",
        "DEPARTURE",
        Some("87212"),
        Some("ON TIME"),
    ));
    let result = process_message(
        &movement_msg,
        &mut state,
        &stanox_table(),
        &crs_index_with(&["WAT"]),
        today(),
    )
    .unwrap();
    assert_eq!(result.train_uid, Some("C21373".to_string()));
}

#[test]
fn a_movement_with_no_parked_activation_still_carries_no_train_uid() {
    // The accepted, unavoidable gap this task's own doc comment names --
    // an Activation this process never saw leaves nothing to attach.
    let message = TrustMessage::Movement(movement(
        "999999999",
        "DEPARTURE",
        Some("87212"),
        Some("ON TIME"),
    ));
    let mut state = ProcessorState::default();
    let result = process_message(
        &message,
        &mut state,
        &stanox_table(),
        &crs_index_with(&["WAT"]),
        today(),
    )
    .unwrap();
    assert_eq!(result.train_uid, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-backlog-consumer a_movement_after_a_parked_activation_carries_the_real_train_uid`
Expected: FAIL — today's `TrustMessage::Movement` arm hard-codes `train_uid: None` unconditionally (`process.rs:122`).

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/trust-backlog-consumer/src/process.rs
#[derive(Debug, Default)]
pub struct ProcessorState {
    pub pending_service_dates: HashMap<String, NaiveDate>,
    /// `train_id -> train_uid`, populated identically to
    /// `pending_service_dates` (same Activation message, same lifetime --
    /// see this module's own doc comment). Closes the gap named in
    /// docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §3:
    /// this consumer is now the PRIMARY writer for the shared trains/
    /// train_movement_events tables, and a real train_uid on every event
    /// is what lets `api` key a Movement into the right `trains` row at
    /// all. Never removed on read (unlike trust-consumer's own
    /// one-shot-claim `pending_activations`) -- a train's whole
    /// Activation-to-Cancellation lifetime may span many Movements, every
    /// one of which needs the same train_uid, not just the first.
    pub pending_train_uids: HashMap<String, String>,
}
```

In `process_message`'s `TrustMessage::Activation` arm, add alongside the
existing `pending_service_dates` insert:
```rust
state
    .pending_train_uids
    .insert(activation.train_id.clone(), activation.train_uid.clone());
```

In the `TrustMessage::Movement` arm, replace the hard-coded field:
```rust
Some(common::TrustBacklogEventMessage {
    crs: Some(loc_crs),
    train_uid: state.pending_train_uids.get(&movement.train_id).cloned(),
    train_id: movement.train_id.clone(),
    // ... every other field unchanged
})
```

In the `TrustMessage::Cancellation` arm, same replacement:
```rust
Some(common::TrustBacklogEventMessage {
    crs: None,
    train_uid: state.pending_train_uids.get(&cancellation.train_id).cloned(),
    train_id: cancellation.train_id.clone(),
    // ... every other field unchanged
})
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p trust-backlog-consumer a_movement_after_a_parked_activation_carries_the_real_train_uid a_movement_with_no_parked_activation_still_carries_no_train_uid`
Expected: PASS. Also `cargo test -p trust-backlog-consumer` in full to
confirm the existing 7 tests in this module still pass unchanged (none of
them assert `train_uid` at all today, so none should regress).

- [ ] **Step 5: Commit**
```bash
git add crates/trust-backlog-consumer/src/process.rs
git commit -m "trust-backlog-consumer: carry the real train_uid on every Movement/Cancellation"
```

## Task 14: Wire trust-backlog-consumer's ingest route as the shared movement-event writer

**Files:**
- Modify: `crates/api/src/data/trust_event_backlog.rs` (new `ingest_shared_movement` function)
- Modify: `crates/api/src/routes/ingest.rs` (`post_trust_event_backlog`)
- Test: `crates/api/src/data/trust_event_backlog.rs` (extends the existing `db_tests` module)

**Interfaces:**
- Consumes: `trains::find_or_create_train`, `trains::mark_train_resolved` (Task 2); `train_tracking::upsert_train_movement` (Task 11); `TrustBacklogEventMessage.train_uid` (now real for correlated events, Task 13).
- Produces: `pub async fn ingest_shared_movement(pool: &PgPool, event: &TrustBacklogEventMessage) -> anyhow::Result<()>`, called once per event from `post_trust_event_backlog` alongside its existing `upsert_trust_event_backlog_batch` call. This is the concrete code behind the design spec's "trust-backlog-consumer becomes the primary movement-event writer" (§3) — `trust_event_backlog` itself is untouched by this task; this is a second, parallel write off the same incoming batch.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/trust_event_backlog.rs -- appended to db_tests
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

    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
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
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM trains WHERE train_id = 'TEST-INGEST-NO-UID-TRAIN-ID'")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 0, "no trains row should be created with no known train_uid");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api ingest_shared_movement_writes_the_shared_tables_when_train_uid_is_known -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `ingest_shared_movement` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/api/src/data/trust_event_backlog.rs -- appended, after upsert_trust_event_backlog_batch
use trust_schema::journey::{self, DerivedState};
use trust_schema::schema::Movement;

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
                planned_timestamp: event.planned_timestamp.map(|t| t.timestamp_millis().to_string()),
                actual_timestamp: event.actual_timestamp.map(|t| t.timestamp_millis().to_string()),
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

async fn fetch_previous_derived_state(pool: &PgPool, trains_id: i64) -> anyhow::Result<DerivedState> {
    let row: Option<(String, Option<String>, Option<String>, Option<i32>, Option<String>)> =
        sqlx::query_as(
            "SELECT status, last_reported_location, last_event_type, delay_minutes, next_calling_point \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some((status, last_reported_location, last_event_type, delay_minutes, next_calling_point)) => {
            DerivedState {
                status,
                last_reported_location,
                last_event_type,
                delay_minutes,
                next_calling_point,
            }
        }
        None => DerivedState::awaiting_activation(),
    })
}
```

Wire it into the ingest route (`crates/api/src/routes/ingest.rs:250-259`):
```rust
async fn post_trust_event_backlog(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrustBacklogEventMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let inserted =
        crate::data::trust_event_backlog::upsert_trust_event_backlog_batch(&app.database, &events)
            .await
            .map_err(internal_error)?;

    // Additional, parallel write onto the shared trains/train_movement_events/
    // train_current_state tables -- see ingest_shared_movement's own doc
    // comment. A per-event failure here is logged and skipped, never
    // propagated: this route's own contract (backlog archival) must not
    // start failing because of a problem in the newer, separate shared-store
    // write path.
    for event in &events {
        if let Err(err) =
            crate::data::trust_event_backlog::ingest_shared_movement(&app.database, event).await
        {
            tracing::warn!(error = ?err, train_id = %event.train_id, "failed to ingest shared movement");
        }
    }

    Ok(Json(UpsertResponse { upserted: inserted }))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api ingest_shared_movement_writes_the_shared_tables_when_train_uid_is_known ingest_shared_movement_is_a_no_op_with_no_known_train_uid -- --ignored --test-threads=1`
Expected: PASS. Also re-run `cargo test -p api post_trust_event_backlog` and the existing `upsert_trust_event_backlog_batch` tests to confirm the backlog-archival contract itself is unchanged.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/trust_event_backlog.rs crates/api/src/routes/ingest.rs
git commit -m "Wire trust-backlog-consumer's ingest route as the shared movement-event writer"
```

## Task 15: `trust_event_backlog` — add a `train_uid` read path for the legacy fallback

**Files:**
- Create: `crates/api/migrations/20260906111500_trust_event_backlog_train_uid_index.sql`
- Modify: `crates/api/src/data/trust_event_backlog_match.rs` (new `find_train_id_by_uid` function)
- Test: `crates/api/src/data/trust_event_backlog_match.rs` (extends the existing `db_tests` module)

**Interfaces:**
- Consumes: table `trust_event_backlog` (unchanged shape, per this plan's Global Constraints — this table is kept, not retired).
- Produces: `pub async fn find_train_id_by_uid(pool: &PgPool, train_uid: &str, service_date: NaiveDate) -> anyhow::Result<Option<String>>` — resolves a bare `train_uid`/`service_date` to TRUST's own `train_id` via the one Activation row that ever carries a `train_uid` in this table, per the design spec's §3. Not called by any other task in this plan directly (this closes a read-path gap the design spec names, ahead of the eventual search/lookup sub-project that will need it — see the spec's §6 non-goals on the UID-first schedule reverse-index it deliberately does NOT build).

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/trust_event_backlog_match.rs -- appended to db_tests
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            find_train_id_by_uid_resolves_via_the_activation_row -- --ignored --test-threads=1`"]
async fn find_train_id_by_uid_resolves_via_the_activation_row() {
    let pool = connect().await;
    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    sqlx::query(
        "INSERT INTO trust_event_backlog \
            (crs, train_uid, train_id, service_date, msg_type, dedup_key) \
         VALUES (NULL, $1, $2, $3, '0001', $4)",
    )
    .bind("TEST-FIND-BY-UID")
    .bind("TEST-FIND-BY-UID-TRAIN-ID")
    .bind(service_date)
    .bind("test-find-by-uid-dedup-activation")
    .execute(&pool)
    .await
    .expect("seed an Activation row");

    let train_id = find_train_id_by_uid(&pool, "TEST-FIND-BY-UID", service_date)
        .await
        .expect("find_train_id_by_uid");
    assert_eq!(train_id, Some("TEST-FIND-BY-UID-TRAIN-ID".to_string()));

    let miss = find_train_id_by_uid(&pool, "TEST-FIND-BY-UID-NO-SUCH-ROW", service_date)
        .await
        .expect("find_train_id_by_uid miss");
    assert_eq!(miss, None);

    sqlx::query("DELETE FROM trust_event_backlog WHERE train_id = 'TEST-FIND-BY-UID-TRAIN-ID'")
        .execute(&pool)
        .await
        .ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api find_train_id_by_uid_resolves_via_the_activation_row -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `find_train_id_by_uid` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906111500_trust_event_backlog_train_uid_index.sql
-- Adds the train_uid read path named by
-- docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §3.
-- Confirmed directly against 20260905160000_trust_event_backlog.sql: no
-- index on train_uid exists on this table today (only dedup_key,
-- (crs, planned_timestamp), and (train_id, service_date)).
CREATE INDEX trust_event_backlog_train_uid
    ON trust_event_backlog (train_uid, service_date)
    WHERE train_uid IS NOT NULL;
```

```rust
// crates/api/src/data/trust_event_backlog_match.rs -- appended, after find_backlog_match
/// Resolves a bare `(train_uid, service_date)` to TRUST's own `train_id`,
/// via the one row type in this table that ever carries a `train_uid` at
/// all -- an Activation (`msg_type = '0001'`). Unlike `find_backlog_match`
/// above (a CRS+time lookup that discovers an unknown `train_id`), this is
/// the inverse direction: identity is already known, and the caller wants
/// TRUST's own daily identifier to key a `fetch_backlog_history`-style
/// lookup by. `None` covers both "no Activation for this identity is in
/// the backlog's retention window" and "never existed" uniformly -- this
/// table has no way to distinguish them, same posture as every other
/// lookup in this module.
pub async fn find_train_id_by_uid(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT train_id FROM trust_event_backlog \
         WHERE train_uid = $1 AND service_date = $2 AND msg_type = '0001' \
         LIMIT 1",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(train_id,)| train_id))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `psql "$DATABASE_URL" -c "SELECT to_regclass('public.trust_event_backlog_train_uid');"` (confirms the index exists, applied via `sqlx::migrate!` on `api`'s next startup), then `DATABASE_URL=... cargo test -p api find_train_id_by_uid_resolves_via_the_activation_row -- --ignored --test-threads=1`
Expected: index name printed; test PASSes.

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906111500_trust_event_backlog_train_uid_index.sql crates/api/src/data/trust_event_backlog_match.rs
git commit -m "Add a train_uid read path to trust_event_backlog"
```

## Task 16: `trust-consumer` — direct `train_uid` match on Activation, bypassing the CRS+time heuristic

**Files:**
- Modify: `crates/trust-consumer/src/process.rs` (`Reference`, `ProcessorState`, `apply_reference_reload`, `process_message`)
- Test: `crates/trust-consumer/src/process.rs` (extends the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `common::TrackedTrainRef.train_uid` (already exists — populated whenever a ref is schedule-matched, or created via Task 20's new NR-primary endpoint).
- Produces: `Reference` gains `pub by_train_uid: HashMap<String, i64>` (train_uid -> tracked_train_id); `ProcessorState` gains `pub activation_matched_awaiting_movement: HashSet<String>` (train_id). An Activation whose `train_uid` is already known to a pin now resolves `state.resolved` immediately, without waiting for a Movement to run the ±20-minute CRS+time heuristic at all — strictly more reliable, per the design spec's §3. Every existing `Reference { pending: ... }` struct literal in this file's own test module needs a second field added (`by_train_uid: HashMap::new()`) to keep compiling — a mechanical fixup, not a behavioral one.

- [ ] **Step 1: Write the failing test**

```rust
// crates/trust-consumer/src/process.rs -- appended to the existing `mod tests`
#[tokio::test]
async fn an_activation_with_a_known_train_uid_resolves_the_pin_immediately_without_waiting_for_a_movement()
{
    let activation = r#"[{"header":{"msg_type":"0001"},"body":{
        "train_id":"221832406","train_uid":"C88888","toc_id":"SW",
        "train_service_code":"22345000","schedule_wtt_id":"WTT1",
        "schedule_start_date":"2026-08-28","schedule_end_date":"2026-08-28"
    }}]"#;
    let mut feed = FakeMovementFeed::new(vec![vec![activation.to_string()]]);
    let mut reference = Reference {
        pending: Vec::new(),
        by_train_uid: HashMap::new(),
    };
    reference.by_train_uid.insert("C88888".to_string(), 1);
    let mut state = ProcessorState::default();

    let events = run_once(&mut feed, &reference, &mut state, &TEST_STANOX_CRS)
        .await
        .unwrap();
    assert!(events.is_empty(), "an Activation never posts an event of its own");
    assert_eq!(
        state.resolved.get("221832406"),
        Some(&1),
        "resolved immediately on Activation, before any Movement at all"
    );
}

#[tokio::test]
async fn the_first_movement_after_an_activation_direct_match_still_carries_resolved_train_id_for_the_db_flip()
{
    let activation = r#"[{"header":{"msg_type":"0001"},"body":{
        "train_id":"221832406","train_uid":"C88888","toc_id":"SW",
        "train_service_code":"22345000","schedule_wtt_id":"WTT1",
        "schedule_start_date":"2026-08-28","schedule_end_date":"2026-08-28"
    }}]"#;
    let later_arrival = r#"[{"header":{"msg_type":"0003"},"body":{
        "train_id":"221832406","event_type":"ARRIVAL",
        "planned_timestamp":"1787943000000","actual_timestamp":"1787943000000",
        "loc_stanox":"86031","variation_status":"ON TIME"
    }}]"#;
    let mut feed = FakeMovementFeed::new(vec![
        vec![activation.to_string()],
        vec![later_arrival.to_string()],
    ]);
    let mut reference = Reference {
        pending: Vec::new(),
        by_train_uid: HashMap::new(),
    };
    reference.by_train_uid.insert("C88888".to_string(), 1);
    let mut state = ProcessorState::default();

    run_once(&mut feed, &reference, &mut state, &TEST_STANOX_CRS)
        .await
        .unwrap();
    let events = run_once(&mut feed, &reference, &mut state, &TEST_STANOX_CRS)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tracked_train_id, 1);
    assert_eq!(events[0].resolved_train_uid, Some("C88888".to_string()));
    assert_eq!(events[0].resolved_train_id, Some("221832406".to_string()));

    // A SECOND movement for the same train_id must not re-report resolution.
    let second_arrival = r#"[{"header":{"msg_type":"0003"},"body":{
        "train_id":"221832406","event_type":"ARRIVAL",
        "planned_timestamp":"1787943100000","actual_timestamp":"1787943100000",
        "loc_stanox":"86031","variation_status":"ON TIME"
    }}]"#;
    let mut feed2 = FakeMovementFeed::new(vec![vec![second_arrival.to_string()]]);
    let second_events = run_once(&mut feed2, &reference, &mut state, &TEST_STANOX_CRS)
        .await
        .unwrap();
    assert_eq!(second_events[0].resolved_train_id, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-consumer an_activation_with_a_known_train_uid_resolves_the_pin_immediately`
Expected: FAIL with a compile error — `Reference` has no `by_train_uid` field yet.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/trust-consumer/src/process.rs
pub struct Reference {
    pub pending: Vec<crate::matching::PendingPin>,
    /// `train_uid -> tracked_train_id`, for every active ref whose
    /// identity is already known (a schedule match, or an NR-primary
    /// subscription created via `POST /Train/by-uid/.../track`, Task 20).
    /// Checked FIRST on every Activation
    /// (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §3)
    /// -- strictly more reliable than the ±20-minute CRS+time heuristic
    /// `matching::resolve_origin_departure` still exists for pins that
    /// genuinely lack this.
    pub by_train_uid: HashMap<String, i64>,
}

#[derive(Debug, Default)]
pub struct ProcessorState {
    pub resolved: HashMap<String, i64>,
    pub pending_activations: HashMap<String, PendingActivation>,
    pub last_derived: HashMap<String, DerivedState>,
    /// `train_id`s resolved via `by_train_uid`'s direct-match fast path
    /// (this task) but not yet confirmed by a live Movement. `api`'s own
    /// `upsert_train_event` only flips `resolution_status` on a message
    /// that carries `resolved_train_id` -- since the direct match happens
    /// on the Activation itself (which never posts an event), this set
    /// defers that one-time "freshly resolved" signal to the FIRST
    /// Movement this process sees for the train_id, exactly once.
    pub activation_matched_awaiting_movement: HashSet<String>,
}
```

`apply_reference_reload` (populate `by_train_uid` alongside the existing `pending` rebuild):
```rust
pub fn apply_reference_reload(
    refs: Vec<common::TrackedTrainRef>,
    reference: &mut Reference,
    state: &mut ProcessorState,
) {
    let mut pending = Vec::new();
    let mut by_train_uid = HashMap::new();

    for tracked in refs {
        match tracked.resolution_status.as_str() {
            "pending" | "schedule_matched" => {
                if let Some(train_uid) = &tracked.train_uid {
                    by_train_uid.insert(train_uid.clone(), tracked.id);
                }
                pending.push(crate::matching::PendingPin {
                    tracked_train_id: tracked.id,
                    pin_origin_crs: tracked.pin_origin_crs,
                    pin_scheduled_departure: tracked.pin_scheduled_departure,
                });
            }
            "resolved" => {
                if let Some(train_id) = tracked.train_id {
                    state.resolved.entry(train_id).or_insert(tracked.id);
                }
            }
            _ => {}
        }
    }

    reference.pending = pending;
    reference.by_train_uid = by_train_uid;
}
```

`process_message`'s `Activation` arm:
```rust
TrustMessage::Activation(activation) => {
    if let Some(&tracked_train_id) = reference.by_train_uid.get(&activation.train_uid)
        && !state.resolved.contains_key(&activation.train_id)
    {
        state.resolved.insert(activation.train_id.clone(), tracked_train_id);
        state
            .activation_matched_awaiting_movement
            .insert(activation.train_id.clone());
    }
    state.pending_activations.insert(
        activation.train_id.clone(),
        PendingActivation {
            train_uid: activation.train_uid.clone(),
            schedule_end_date: activation.schedule_end_date.parse::<NaiveDate>().ok(),
        },
    );
    None
}
```

`process_message`'s `Movement` arm — only the `Some` branch of the
existing `match state.resolved.get(&movement.train_id).copied()` changes:
```rust
let (tracked_train_id, freshly_resolved) =
    match state.resolved.get(&movement.train_id).copied() {
        Some(tracked_train_id) => {
            let freshly_resolved = state
                .activation_matched_awaiting_movement
                .remove(&movement.train_id);
            (tracked_train_id, freshly_resolved)
        }
        None => {
            // ... unchanged CRS+time heuristic branch below this point
        }
    };
```

Finally, add `use std::collections::HashSet;` if not already imported (it
already is, for `claimed`/`unclaimed` a few lines below), and update every
existing `Reference { pending: ... }` construction in this file's test
module (both direct struct literals and the `reference_with_one_pending`
helper) to also set `by_train_uid: HashMap::new()`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p trust-consumer an_activation_with_a_known_train_uid_resolves_the_pin_immediately the_first_movement_after_an_activation_direct_match`
Expected: PASS. Then `cargo test -p trust-consumer` in full to confirm all
25 pre-existing tests in this module still pass against the mechanically
updated `Reference` literals.

- [ ] **Step 5: Commit**
```bash
git add crates/trust-consumer/src/process.rs
git commit -m "trust-consumer: resolve a pin directly on Activation when its train_uid is already known"
```

## Task 17: New notifier-forwarding queue table, plus `trust-consumer`'s write side

**Files:**
- Create: `crates/api/migrations/20260906120000_notifier_forward_queue.sql`
- Modify: `crates/common/src/lib.rs` (`TrackedTrainRef` gains `trains_id`; new `TrainForwardSignalMessage`)
- Modify: `crates/api/src/data/train_tracking.rs` (`TrackedTrainRow`/`list_active_tracked_trains` select `trains_id` too)
- Create: `crates/api/src/data/notifier_forward_queue.rs` (write side, mirroring `trust_event_backlog.rs`'s own storage-module shape)
- Modify: `crates/api/src/data/mod.rs` (add `pub mod notifier_forward_queue;`)
- Modify: `crates/api/src/routes/ingest.rs` (new `/train-forward-signals` route)
- Modify: `crates/trust-consumer/src/process.rs` (`Reference.trains_id_by_tracked_train_id`, new pure `build_forward_signals`)
- Modify: `crates/trust-consumer/src/queries.rs`, `crates/trust-consumer/src/main.rs` (post the signals)
- Test: `crates/trust-consumer/src/process.rs` (new unit test for `build_forward_signals`); `crates/api/src/data/notifier_forward_queue.rs` (new `db_tests`)

**Interfaces:**
- Consumes: `trains.id` (Task 1); `TrainMovementEventMessage` (unchanged).
- Produces: table `notifier_forward_queue (id BIGSERIAL PRIMARY KEY, trains_id BIGINT NOT NULL REFERENCES trains(id) ON DELETE CASCADE, event_summary TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())`; `pub fn build_forward_signals(events: &[TrainMovementEventMessage], trains_id_by_tracked_train_id: &HashMap<i64, i64>) -> Vec<common::TrainForwardSignalMessage>` (pure, no signature change to `run_once` — kept out of that function entirely so its own 25+ existing tests are untouched); `pub async fn insert_forward_signals(pool: &PgPool, signals: &[common::TrainForwardSignalMessage]) -> anyhow::Result<u64>`. Consumed by Task 18 (`notifier`'s read/poll side).

- [ ] **Step 1: Write the failing test**

```rust
// crates/trust-consumer/src/process.rs -- appended to `mod tests`
#[test]
fn build_forward_signals_only_forwards_events_with_a_known_trains_id() {
    let mut trains_id_by_tracked_train_id = HashMap::new();
    trains_id_by_tracked_train_id.insert(1i64, 42i64);

    let events = vec![
        common::TrainMovementEventMessage {
            tracked_train_id: 1,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "d1".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: None,
            loc_crs: None,
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(3),
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        },
        common::TrainMovementEventMessage {
            tracked_train_id: 2, // no trains_id known for this one
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "d2".to_string(),
            msg_type: "0003".to_string(),
            event_type: None,
            loc_stanox: None,
            loc_crs: None,
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: None,
            last_event_type: None,
            delay_minutes: None,
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        },
    ];

    let signals = build_forward_signals(&events, &trains_id_by_tracked_train_id);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].trains_id, 42);
    assert!(signals[0].event_summary.contains("WAT"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trust-consumer build_forward_signals_only_forwards_events_with_a_known_trains_id`
Expected: FAIL with a compile error — `build_forward_signals` and `common::TrainForwardSignalMessage` don't exist yet.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906120000_notifier_forward_queue.sql
-- The lightweight notifier-forwarding queue named by
-- docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §3 --
-- a forwarding SIGNAL, not a data store: notifier polls this on a second,
-- faster-cadence query (Task 18), still deciding whether to actually send
-- a push via its own unchanged cooldown/escalation logic. No consumed-row
-- bookkeeping column -- notifier tracks its own read position via a
-- second `notifier_cursor` row (Task 18), the same watermark-cursor
-- pattern `poll_train_candidates` already uses.
CREATE TABLE notifier_forward_queue (
    id           BIGSERIAL PRIMARY KEY,
    trains_id    BIGINT NOT NULL REFERENCES trains(id) ON DELETE CASCADE,
    event_summary TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX notifier_forward_queue_trains_id ON notifier_forward_queue (trains_id);
```

```rust
// crates/common/src/lib.rs -- extend TrackedTrainRef, add TrainForwardSignalMessage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedTrainRef {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub train_id: Option<String>,
    /// The shared `trains` row this subscription points at, if any (Task
    /// 1's `tracked_trains.trains_id`). Lets trust-consumer key its new
    /// forwarding-queue writes (Task 17) without a second round-trip.
    pub trains_id: Option<i64>,
}

/// A lightweight forwarding signal from trust-consumer to notifier
/// (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §3)
/// -- deliberately minimal, a signal not a data store: notifier's own
/// unchanged cooldown/escalation logic (`train_current_state`,
/// `train_notification_state`) is still the sole gatekeeper for whether a
/// push is actually sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainForwardSignalMessage {
    pub trains_id: i64,
    pub event_summary: String,
}
```

`crates/api/src/data/train_tracking.rs`'s `TrackedTrainRow`/`list_active_tracked_trains` (extended from Task 11's own version):
```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct TrackedTrainRow {
    id: i64,
    service_date: chrono::NaiveDate,
    pin_origin_crs: String,
    pin_scheduled_departure: DateTime<Utc>,
    resolution_status: String,
    train_uid: Option<String>,
    train_id: Option<String>,
    trains_id: Option<i64>,
}

impl From<TrackedTrainRow> for TrackedTrainRef {
    fn from(row: TrackedTrainRow) -> Self {
        TrackedTrainRef {
            id: row.id,
            service_date: row.service_date,
            pin_origin_crs: row.pin_origin_crs,
            pin_scheduled_departure: row.pin_scheduled_departure,
            resolution_status: row.resolution_status,
            train_uid: row.train_uid,
            train_id: row.train_id,
            trains_id: row.trains_id,
        }
    }
}

pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRow>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, tt.train_id, tt.trains_id \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TrackedTrainRef::from).collect())
}
```

```rust
// crates/api/src/data/notifier_forward_queue.rs
//! Write side of the notifier-forwarding queue (Task 17). Read/poll side
//! lives in `crates/notifier` (Task 18).

use common::TrainForwardSignalMessage;
use sqlx::PgPool;

pub async fn insert_forward_signals(
    pool: &PgPool,
    signals: &[TrainForwardSignalMessage],
) -> anyhow::Result<u64> {
    let mut inserted = 0u64;
    for signal in signals {
        sqlx::query("INSERT INTO notifier_forward_queue (trains_id, event_summary) VALUES ($1, $2)")
            .bind(signal.trains_id)
            .bind(&signal.event_summary)
            .execute(pool)
            .await?;
        inserted += 1;
    }
    Ok(inserted)
}
```

Add `pub mod notifier_forward_queue;` to `crates/api/src/data/mod.rs`, and a
new route in `crates/api/src/routes/ingest.rs` (alongside `post_train_events`):
```rust
async fn post_train_forward_signals(
    State(app): State<App>,
    Json(signals): Json<Vec<common::TrainForwardSignalMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let inserted =
        crate::data::notifier_forward_queue::insert_forward_signals(&app.database, &signals)
            .await
            .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: inserted }))
}
```
registered as `.route("/train-forward-signals", axum::routing::post(post_train_forward_signals))`, same router as `/train-events`.

`crates/trust-consumer/src/process.rs` — extend `Reference` (Task 16's own version) and add the pure builder:
```rust
pub struct Reference {
    pub pending: Vec<crate::matching::PendingPin>,
    pub by_train_uid: HashMap<String, i64>,
    /// `tracked_train_id -> trains_id`, for every active ref that has one
    /// (regardless of resolution_status -- an already-`resolved`
    /// subscription still needs its later movements forwarded). Feeds
    /// `build_forward_signals`, below.
    pub trains_id_by_tracked_train_id: HashMap<i64, i64>,
}
```
`apply_reference_reload` gains one line per `tracked` in its existing loop,
before the `match tracked.resolution_status.as_str()`:
```rust
if let Some(trains_id) = tracked.trains_id {
    trains_id_by_tracked_train_id.insert(tracked.id, trains_id);
}
```
(with a `let mut trains_id_by_tracked_train_id = HashMap::new();` declared
alongside `pending`/`by_train_uid`, and `reference.trains_id_by_tracked_train_id = trains_id_by_tracked_train_id;` set at the end, mirroring the other two fields exactly.)

```rust
/// Pure by design (see this module's own doc comment on why `run_once`
/// itself returns only `Vec<TrainMovementEventMessage>`, untouched by this
/// task): building a forwarding signal is a separate concern from
/// resolving/deriving movement state, and keeping it out of `run_once`
/// means none of that function's own 25+ existing tests need updating for
/// this feature. Filters out any event whose `tracked_train_id` has no
/// known `trains_id` yet -- exactly the same accepted gap named throughout
/// this plan (a subscription whose identity, and therefore trains_id, is
/// still unknown has nothing to forward a signal about).
pub fn build_forward_signals(
    events: &[common::TrainMovementEventMessage],
    trains_id_by_tracked_train_id: &HashMap<i64, i64>,
) -> Vec<common::TrainForwardSignalMessage> {
    events
        .iter()
        .filter_map(|event| {
            let trains_id = *trains_id_by_tracked_train_id.get(&event.tracked_train_id)?;
            Some(common::TrainForwardSignalMessage {
                trains_id,
                event_summary: format!(
                    "{} at {}",
                    event.status,
                    event
                        .last_reported_location
                        .as_deref()
                        .unwrap_or("an unknown location")
                ),
            })
        })
        .collect()
}
```

`crates/trust-consumer/src/queries.rs` — one more thin wrapper, mirroring `post_train_events` exactly:
```rust
pub async fn post_train_forward_signals(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
    signals: &[common::TrainForwardSignalMessage],
) -> anyhow::Result<()> {
    if signals.is_empty() {
        return Ok(());
    }
    common::ingest::post_batch(client, url, tokens, signals, "train forward signals").await
}
```

`crates/trust-consumer/src/main.rs` — after the existing `post_train_events` call site, build and post the signals from the SAME `events` value and the current `reference`:
```rust
let signals = process::build_forward_signals(events, &reference.trains_id_by_tracked_train_id);
if let Err(err) = queries::post_train_forward_signals(&http, &config.forward_signals_url, &internal_oauth, &signals).await {
    tracing::warn!(error = ?err, "failed to post train forward signals");
}
```
(`config.forward_signals_url`: a new `#[arg(long, env)]` field on this
crate's own `Config`, mirroring `config.api_ingest_url`'s own shape,
pointing at the new `/train-forward-signals` route.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p trust-consumer build_forward_signals_only_forwards_events_with_a_known_trains_id`
Expected: PASS. Also `cargo build -p api -p trust-consumer -p common` to confirm every crate touched by the `TrackedTrainRef` field addition still compiles (`full-coverage-consumer`/`trust-backlog-consumer` don't construct this struct, only consume it via JSON, so they're unaffected).

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906120000_notifier_forward_queue.sql crates/common/src/lib.rs \
        crates/api/src/data/train_tracking.rs crates/api/src/data/notifier_forward_queue.rs \
        crates/api/src/data/mod.rs crates/api/src/routes/ingest.rs \
        crates/trust-consumer/src/process.rs crates/trust-consumer/src/queries.rs crates/trust-consumer/src/main.rs
git commit -m "Add the notifier-forwarding queue table and trust-consumer's write side"
```

## Task 18: `notifier`'s read/poll side for the forwarding queue

**Files:**
- Modify: `crates/notifier/src/queries.rs` (extract `candidates_for_trains_id`; new `poll_forward_queue`)
- Modify: `crates/notifier/src/main.rs` (new faster-cadence cycle; extract `notify_train_candidates`)
- Modify: `crates/notifier/src/config.rs` (new `forward_queue_poll_interval_secs`)
- Test: `crates/notifier/src/queries.rs` (extends the `poll_train_candidates`-covering test module)

**Interfaces:**
- Consumes: `notifier_forward_queue` (Task 17).
- Produces: `pub async fn candidates_for_trains_id(pool: &PgPool, trains_id: i64, delay_threshold_minutes: i32) -> anyhow::Result<Vec<TrainCandidate>>` (the per-`trains_id` body Task 12's `poll_train_candidates` already has, extracted so this task can reuse it without duplicating cooldown/escalation logic); `pub async fn poll_forward_queue(pool: &PgPool, since_id: i64) -> anyhow::Result<(Vec<i64>, i64)>` (distinct `trains_id`s touched since the watermark, plus the new watermark). `run_cycle`'s own train section and the new forward-queue cycle both funnel through the SAME `notify_train_candidates`/cooldown logic — this task adds a second, faster INPUT into that logic, never a second decision path (per the design spec's own §6 non-goal on redesigning escalation logic).

- [ ] **Step 1: Write the failing test**

```rust
// crates/notifier/src/queries.rs -- alongside Task 12's poll_train_candidates test
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p notifier \
            poll_forward_queue_returns_distinct_touched_trains_ids -- --ignored --test-threads=1`"]
async fn poll_forward_queue_returns_distinct_touched_trains_ids() {
    let pool = connect().await;
    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let trains_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO trains (train_uid, service_date) VALUES ('TEST-FORWARD-QUEUE-UID', $1) RETURNING id",
    )
    .bind(service_date)
    .fetch_one(&pool)
    .await
    .expect("seed trains row");

    let (queue_id,): (i64,) = sqlx::query_as(
        "INSERT INTO notifier_forward_queue (trains_id, event_summary) VALUES ($1, 'en_route at WAT') RETURNING id",
    )
    .bind(trains_id)
    .fetch_one(&pool)
    .await
    .expect("seed a forward-queue row");

    let (touched, max_id) = poll_forward_queue(&pool, queue_id - 1)
        .await
        .expect("poll_forward_queue");
    assert_eq!(touched, vec![trains_id]);
    assert_eq!(max_id, queue_id);

    let (touched_again, max_id_again) = poll_forward_queue(&pool, max_id)
        .await
        .expect("poll_forward_queue again from the new watermark");
    assert!(touched_again.is_empty());
    assert_eq!(max_id_again, max_id);

    sqlx::query("DELETE FROM notifier_forward_queue WHERE id = $1").bind(queue_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p notifier poll_forward_queue_returns_distinct_touched_trains_ids -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `poll_forward_queue` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

`crates/notifier/src/queries.rs` — extract the per-`trains_id` body out of
Task 12's `poll_train_candidates` and add the new poll function:
```rust
/// The per-`trains_id` candidate-building body Task 12's `poll_train_candidates`
/// already has, extracted so Task 18's forward-queue cycle can reuse it
/// without a second, divergent copy of the cooldown/escalation lookup.
pub async fn candidates_for_trains_id(
    pool: &PgPool,
    trains_id: i64,
    delay_threshold_minutes: i32,
) -> anyhow::Result<Vec<TrainCandidate>> {
    let current = sqlx::query("SELECT status, delay_minutes FROM train_current_state WHERE trains_id = $1")
        .bind(trains_id)
        .fetch_optional(pool)
        .await?;
    let Some(current) = current else { return Ok(Vec::new()) };

    let status: String = current.try_get("status")?;
    let delay_minutes: Option<i32> = current.try_get("delay_minutes")?;
    let new_rank = train_severity_rank(&status, delay_minutes, delay_threshold_minutes);

    let subscribers = sqlx::query("SELECT id, user_id FROM tracked_trains WHERE trains_id = $1")
        .bind(trains_id)
        .fetch_all(pool)
        .await?;
    let mut candidates = Vec::new();
    for subscriber in subscribers {
        let tracked_train_id: i64 = subscriber.try_get("id")?;
        let user_id: String = subscriber.try_get("user_id")?;

        let previous = sqlx::query(
            "SELECT last_notified_status, last_notified_delay_minutes \
             FROM train_notification_state WHERE user_id = $1 AND tracked_train_id = $2",
        )
        .bind(&user_id)
        .bind(tracked_train_id)
        .fetch_optional(pool)
        .await?;
        let previous_rank = match previous {
            None => 0,
            Some(previous) => {
                let previous_status: String = previous.try_get("last_notified_status")?;
                let previous_delay: Option<i32> = previous.try_get("last_notified_delay_minutes")?;
                train_severity_rank(&previous_status, previous_delay, delay_threshold_minutes)
            }
        };

        if crate::decision::decide_train_notification(previous_rank, new_rank)
            == crate::decision::NotifyDecision::NotifyNow
        {
            candidates.push(TrainCandidate { tracked_train_id, trains_id, user_id, new_rank, previous_rank });
        }
    }
    Ok(candidates)
}

pub async fn poll_train_candidates(
    pool: &PgPool,
    since_id: i64,
    delay_threshold_minutes: i32,
) -> anyhow::Result<(Vec<TrainCandidate>, i64)> {
    let touched: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT trains_id FROM train_movement_events WHERE id > $1 AND trains_id IS NOT NULL",
    )
    .bind(since_id)
    .fetch_all(pool)
    .await?;
    if touched.is_empty() {
        return Ok((Vec::new(), since_id));
    }
    let max_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM train_movement_events WHERE id > $1")
        .bind(since_id)
        .fetch_one(pool)
        .await?;

    let mut candidates = Vec::new();
    for trains_id in touched {
        candidates.extend(candidates_for_trains_id(pool, trains_id, delay_threshold_minutes).await?);
    }
    Ok((candidates, max_id))
}

/// The forward queue's own watermark poll -- same shape as
/// `poll_train_candidates`'s own `train_movement_events` watermark, over
/// `notifier_forward_queue` instead. Advanced via its own, separate
/// `notifier_cursor` row (name `"notifier_forward_queue"`), independent of
/// the `"train_movement_events"` cursor `poll_train_candidates` advances.
pub async fn poll_forward_queue(pool: &PgPool, since_id: i64) -> anyhow::Result<(Vec<i64>, i64)> {
    let touched: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT trains_id FROM notifier_forward_queue WHERE id > $1")
            .bind(since_id)
            .fetch_all(pool)
            .await?;
    if touched.is_empty() {
        return Ok((Vec::new(), since_id));
    }
    let max_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM notifier_forward_queue WHERE id > $1")
        .bind(since_id)
        .fetch_one(pool)
        .await?;
    Ok((touched, max_id))
}
```

`crates/notifier/src/config.rs` — new field:
```rust
/// Cadence for the forwarding-queue poll (Task 17/18) -- deliberately
/// faster than `poll_interval_secs`, since the whole point of
/// trust-consumer's forwarding signal is a quicker path to a push than
/// waiting for train_movement_events' own slower-polled cycle. The exact
/// value is a judgment call, not a researched figure -- see the design
/// spec's own Open Question 3 on this cadence needing "concrete design
/// during implementation planning."
#[arg(long, env, default_value_t = 15)]
pub forward_queue_poll_interval_secs: u64,
```

`crates/notifier/src/main.rs` — extract the train-notification-sending
body into a shared function, add the second interval:
```rust
async fn notify_train_candidates(
    pool: &PgPool,
    candidates: &[queries::TrainCandidate],
    vapid_private_key: &str,
    vapid_subject: &str,
    now: chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    for candidate in candidates {
        tracing::info!(
            tracked_train_id = candidate.tracked_train_id,
            trains_id = candidate.trains_id,
            previous_rank = candidate.previous_rank,
            new_rank = candidate.new_rank,
            "train notification candidate"
        );
        let (status, delay_minutes) = current_train_state(pool, candidate.trains_id).await?;
        let payload = NotificationPayload {
            title: if status == "cancelled" { "Your train was cancelled".to_string() } else { "Your train is delayed".to_string() },
            body: match delay_minutes {
                Some(minutes) if status != "cancelled" => format!("Now running about {minutes} minutes late."),
                _ => "Check the latest status.".to_string(),
            },
            url: format!("/track/{}", candidate.tracked_train_id),
            tag: format!("train-{}", candidate.tracked_train_id),
        };
        if send_to_all_subscriptions(pool, &candidate.user_id, &payload, vapid_private_key, vapid_subject).await? {
            queries::upsert_train_notification_state(pool, &candidate.user_id, candidate.tracked_train_id, &status, delay_minutes, now)
                .await?;
        }
    }
    Ok(())
}

async fn run_forward_queue_cycle(
    pool: &PgPool,
    train_delay_threshold_minutes: i32,
    vapid_private_key: &str,
    vapid_subject: &str,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let cursor_start = queries::read_cursor(pool, "notifier_forward_queue").await?;
    let (touched_trains_ids, max_id) = queries::poll_forward_queue(pool, cursor_start).await?;
    for trains_id in touched_trains_ids {
        let candidates =
            queries::candidates_for_trains_id(pool, trains_id, train_delay_threshold_minutes).await?;
        notify_train_candidates(pool, &candidates, vapid_private_key, vapid_subject, now).await?;
    }
    queries::advance_cursor(pool, "notifier_forward_queue", max_id).await?;
    Ok(())
}
```
`run_cycle`'s own train section (`main.rs:105-126`) now calls the shared
function instead of its own inline loop:
```rust
let (train_candidates, train_max_id) =
    queries::poll_train_candidates(pool, train_cursor_start, train_delay_threshold_minutes).await?;
notify_train_candidates(pool, &train_candidates, vapid_private_key, vapid_subject, now).await?;
queries::advance_cursor(pool, "train_movement_events", train_max_id).await?;
```
and `main`'s own loop gains the second interval, selected concurrently:
```rust
let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
let mut forward_interval = tokio::time::interval(Duration::from_secs(config.forward_queue_poll_interval_secs));
loop {
    tokio::select! {
        _ = interval.tick() => {
            let result = run_cycle(&pool, cooldown, config.train_delay_threshold_minutes, &config.vapid_private_key, &config.vapid_subject).await;
            if let Err(err) = result {
                tracing::error!(error = ?err, "notifier cycle failed; will retry next interval");
            }
        }
        _ = forward_interval.tick() => {
            let result = run_forward_queue_cycle(&pool, config.train_delay_threshold_minutes, &config.vapid_private_key, &config.vapid_subject).await;
            if let Err(err) = result {
                tracing::error!(error = ?err, "notifier forward-queue cycle failed; will retry next interval");
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p notifier poll_forward_queue_returns_distinct_touched_trains_ids -- --ignored --test-threads=1`
Expected: PASS. Also `cargo build -p notifier` to confirm `main.rs`'s `tokio::select!` loop and the extracted `notify_train_candidates` compile, and re-run Task 12's own `poll_train_candidates_fans_out_one_trains_id_to_every_subscriber` test to confirm the extraction didn't change its behavior.

- [ ] **Step 5: Commit**
```bash
git add crates/notifier/src/queries.rs crates/notifier/src/main.rs crates/notifier/src/config.rs
git commit -m "notifier: poll the forwarding queue on a second, faster cadence"
```

## Task 19: `GET /Train/by-uid/{uid}/{date}` — drop the ownership check, read `trains` directly

**Files:**
- Modify: `crates/api/src/data/trains.rs` (new `PublicTrainState`, `get_public_train_state`)
- Modify: `crates/api/src/routes/train.rs` (`get_by_uid_and_date`)
- Test: `crates/api/src/data/trains.rs` (extends `db_tests`); `crates/api/src/routes/train.rs` (replaces the existing ownership-scoped tests for this route)

**Interfaces:**
- Consumes: table `trains` (Task 1), `train_current_state.trains_id` (Task 9, populated going forward by Tasks 11/14).
- Produces: `pub async fn get_public_train_state(pool: &PgPool, train_uid: &str, service_date: NaiveDate) -> anyhow::Result<Option<PublicTrainState>>`, and the route handler drops its `AuthenticatedUser` extractor entirely. **This is the real, reviewed API-contract change this plan's Global Constraints call out explicitly**: today, "your own tracked trains only, 404 for anyone else's"; after this task, "anyone can look up any known train." Supersedes Step C's own flip of this one route (Task 8) — this task moves it off `tracked_trains` entirely, rather than merely changing which columns it joins through.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/routes/train.rs -- replaces the three existing
// get_by_uid_and_date_* tests (get_by_uid_and_date_no_session_is_401,
// get_by_uid_and_date_a_non_owner_session_gets_the_same_404_as_unresolved,
// get_by_uid_and_date_an_unresolved_pair_is_404_with_the_unchanged_message)
// -- their own 401-for-anonymous / 404-for-non-owner assertions describe
// exactly the ownership contract this task deliberately removes.
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            get_by_uid_and_date_is_public_and_unscoped -- --ignored --test-threads=1`"]
async fn get_by_uid_and_date_is_public_and_unscoped() {
    let pool = connect().await; // however this file's own db-backed route tests connect
    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let trains_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO trains (train_uid, service_date, origin_crs) \
         VALUES ('TEST-PUBLIC-BY-UID', $1, 'EUS') RETURNING id",
    )
    .bind(service_date)
    .fetch_one(&pool)
    .await
    .expect("seed a trains row with NO subscriber at all");

    let router = crate::app::Router::new()
        .route(
            "/Train/by-uid/{train_uid}/{date}",
            axum::routing::get(super::get_by_uid_and_date),
        )
        .with_state(test_app(pool.clone()));

    // No Authorization header at all -- this must succeed, not 401.
    let response = request(router, format!("/Train/by-uid/TEST-PUBLIC-BY-UID/{service_date}"), None).await;
    assert_eq!(response.status(), StatusCode::OK);

    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api get_by_uid_and_date_is_public_and_unscoped -- --ignored --test-threads=1`
Expected: FAIL — today's handler requires `AuthenticatedUser`, so an
unauthenticated request never reaches the ownership check at all and comes
back `401`, not `200`.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/api/src/data/trains.rs -- appended, after mark_train_resolved
use serde::Serialize;

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
```

```rust
// crates/api/src/routes/train.rs
async fn get_by_uid_and_date(
    State(app): State<App>,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<crate::data::trains::PublicTrainState>, (StatusCode, String)> {
    let state = crate::data::trains::get_public_train_state(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("read public train state"))?;
    match state {
        Some(state) => Ok(Json(state)),
        None => Err((
            StatusCode::NOT_FOUND,
            "no known train for that uid/date".to_string(),
        )),
    }
}
```
(Deletes the `AuthenticatedUser` parameter and the `tracked_train_owner`
ownership check the old handler had — see the design spec's §4 for why
this is a deliberate, reviewed removal, not an oversight.)

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api get_by_uid_and_date_is_public_and_unscoped -- --ignored --test-threads=1`
Expected: PASS. Delete the three now-contradictory ownership tests named in
Step 1's comment (they assert behavior this task deliberately removes).

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/trains.rs crates/api/src/routes/train.rs
git commit -m "Make GET /Train/by-uid/{uid}/{date} public and unscoped, reading trains directly"
```

## Task 20: New `POST /Train/by-uid/{uid}/{date}/track` — the NR-primary tracking entry point

**Files:**
- Create: `crates/api/migrations/20260906130000_nullable_pin_columns.sql`
- Modify: `crates/api/src/data/train_tracking.rs` (new `create_subscription_for_train`)
- Modify: `crates/api/src/routes/train.rs` (new `post_track_by_uid` route + handler)
- Test: `crates/api/src/data/train_tracking.rs` (extends `db_tests`)

**Interfaces:**
- Consumes: `trains::find_or_create_train` (Task 2).
- Produces: `pub async fn create_subscription_for_train(pool: &PgPool, trains_id: i64, user_id: &str) -> anyhow::Result<i64>`, and `POST /Train/by-uid/{train_uid}/{date}/track`. For a `trains` row with schedule data already known, the new subscription inherits it immediately (`pin_origin_crs`/`pin_scheduled_departure`/`pin_destination_crs` sourced live from the `trains` row); for a bare `train_uid` with no schedule match yet (the design spec's own accepted §1 gap), these come back `NULL` — which requires relaxing `tracked_trains.pin_origin_crs`/`pin_scheduled_departure` from `NOT NULL`, since this plan's Global Constraints (and `validate_pin`'s existing MAX_PIN_AGE check) only ever applied to the LEGACY `POST /Train/track` path, never to an identity-already-known NR-primary one.

- [ ] **Step 1: Write the failing test**

```rust
// crates/api/src/data/train_tracking.rs -- appended to db_tests
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            create_subscription_for_train_inherits_known_schedule_data \
            -- --ignored --test-threads=1`"]
async fn create_subscription_for_train_inherits_known_schedule_data() {
    let pool = connect().await;
    let user_id = "TEST-NR-PRIMARY-TRACK";
    sqlx::query(
        "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(user_id)
    .bind("nr-primary-track@example.com")
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("seed fixture user");

    let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
    let scheduled_departure: chrono::DateTime<chrono::Utc> = "2026-09-06T19:15:00Z".parse().unwrap();
    let trains_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure) \
         VALUES ('TEST-NR-PRIMARY-UID', $1, 'EUS', $2) RETURNING id",
    )
    .bind(service_date)
    .bind(scheduled_departure)
    .fetch_one(&pool)
    .await
    .expect("seed a trains row with known schedule data");

    let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
        .await
        .expect("create_subscription_for_train");

    let (row_trains_id, pin_origin_crs, pin_scheduled_departure): (Option<i64>, String, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as("SELECT trains_id, pin_origin_crs, pin_scheduled_departure FROM tracked_trains WHERE id = $1")
            .bind(tracking_id)
            .fetch_one(&pool)
            .await
            .expect("read back the new subscription");
    assert_eq!(row_trains_id, Some(trains_id));
    assert_eq!(pin_origin_crs, "EUS");
    assert_eq!(pin_scheduled_departure, scheduled_departure);

    sqlx::query("DELETE FROM tracked_trains WHERE id = $1").bind(tracking_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api create_subscription_for_train_inherits_known_schedule_data -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `create_subscription_for_train` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906130000_nullable_pin_columns.sql
-- An NR-primary subscription (Task 20) may point at a bare train_uid with
-- no schedule match yet (the design spec's own §1 accepted gap) -- there
-- is no pin_origin_crs/pin_scheduled_departure to store in that case.
-- These two columns were NOT NULL only because every subscription used to
-- be created via the legacy CRS+time pin flow, which always supplies them
-- (validate_pin enforces that upstream, unchanged). Relaxing them here is
-- additive/safe: no existing row has a NULL value in either column today.
ALTER TABLE tracked_trains ALTER COLUMN pin_origin_crs DROP NOT NULL;
ALTER TABLE tracked_trains ALTER COLUMN pin_scheduled_departure DROP NOT NULL;
```

```rust
// crates/api/src/data/train_tracking.rs -- appended, near create_pin
/// Creates a subscription for an identity that is ALREADY fully known
/// (the NR-primary path, docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
/// §4) -- no `pending`/`schedule_matched` waypoint at all, unlike
/// `create_pin`'s legacy CRS+time flow. `pin_*` columns are sourced live
/// from the `trains` row itself via `INSERT ... SELECT`, and come back
/// `NULL` if that row has no schedule data yet (the accepted gap named in
/// the design spec's §1) -- safe since Task 20's own migration relaxed
/// their `NOT NULL` constraint.
pub async fn create_subscription_for_train(
    pool: &PgPool,
    trains_id: i64,
    user_id: &str,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains \
            (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs) \
         SELECT $1, tr.id, tr.service_date, tr.origin_crs, tr.scheduled_departure, tr.destination_crs \
         FROM trains tr WHERE tr.id = $2 \
         RETURNING id",
    )
    .bind(user_id)
    .bind(trains_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}
```

```rust
// crates/api/src/routes/train.rs -- new route + handler
// .route(
//     "/Train/by-uid/{train_uid}/{date}/track",
//     axum::routing::post(post_track_by_uid),
// )

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackByUidResponse {
    tracking_id: i64,
}

async fn post_track_by_uid(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<TrackByUidResponse>, (StatusCode, String)> {
    let trains_id = crate::data::trains::find_or_create_train(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("find or create train"))?;
    let tracking_id = train_tracking::create_subscription_for_train(&app.database, trains_id, &user.id)
        .await
        .map_err(internal_error("create subscription"))?;
    Ok(Json(TrackByUidResponse { tracking_id }))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api create_subscription_for_train_inherits_known_schedule_data -- --ignored --test-threads=1`
Expected: PASS. Also `cargo build -p api` to confirm the new route registers cleanly.

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906130000_nullable_pin_columns.sql crates/api/src/data/train_tracking.rs crates/api/src/routes/train.rs
git commit -m "Add POST /Train/by-uid/{uid}/{date}/track, the NR-primary tracking entry point"
```

## Task 21: `POST /Train/track` internal rewire — retire `tracked_trains`' own legacy schedule/identity column writes

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs` (`apply_schedule_match`, `list_pending_pins_for_schedule_match`, `list_active_tracked_trains`)
- Modify: `crates/api/src/data/schedule_matching.rs` (`attempt_schedule_match`'s call site)
- Test: `crates/api/src/data/schedule_matching.rs` (re-run existing `db_tests`, no new test file needed — see Step 1)

**Interfaces:**
- Consumes: `tracked_trains.trains_id` (Task 1), `trains::find_or_create_train_with_schedule_match` (Task 2, already wired by Task 3).
- Produces: `apply_schedule_match`'s signature narrows to `pub async fn apply_schedule_match(pool: &PgPool, tracked_train_id: i64) -> anyhow::Result<bool>` — it now ONLY flips `resolution_status`, guarded on `trains_id IS NULL` instead of `train_uid IS NULL`. After this task, **no code path anywhere in this crate ever writes** `tracked_trains.{train_uid, train_id, matched_line_id, schedule_calling_points, schedule_destination_crs, schedule_matched_at, resolved_at's schedule-adjacent siblings}` — every one of Task 8's read-flips already stopped trusting them, so this is a pure write-side contraction, symmetric with Step C. This is the precondition Task 22's final drop depends on.

- [ ] **Step 1: Write the failing test**

This task's own correctness is best proven by re-running Task 3's own,
already-passing test — its assertions must keep passing even though the
column being asserted on (`state.train_uid`) is no longer written by the
function under test:

Run first, to confirm the CURRENT (pre-Task-21) behavior:
```bash
DATABASE_URL=... cargo test -p api attempt_schedule_match_reproduces_the_eus_bug_and_now_resolves_it -- --ignored --test-threads=1
```
Expected: PASS (this is the baseline — Task 21 must not break it). Then
make the change below and confirm it STILL passes — proving the read
(via Task 8's `trains` join) and the write (now only via
`find_or_create_train_with_schedule_match`) agree with each other even
with `tracked_trains.train_uid` itself no longer touched.

- [ ] **Step 2: Run test to verify it fails**

There is no NEW test to fail here — Step 1's existing test already passes
before this change and must continue to. Instead, confirm the thing this
task removes actually still happens today: `psql "$DATABASE_URL" -c
"SELECT proname FROM pg_proc"` is unnecessary — simpler,
`grep -n "SET train_uid" crates/api/src/data/train_tracking.rs` shows
`apply_schedule_match`'s current `UPDATE` still sets it (line ~501),
confirming there's something real to remove.

- [ ] **Step 3: Write minimal implementation**

Before (`crates/api/src/data/train_tracking.rs:491-513`):
```rust
pub async fn apply_schedule_match(
    pool: &PgPool,
    tracked_train_id: i64,
    train_uid: &str,
    matched_line_id: &str,
    schedule_calling_points: &serde_json::Value,
    schedule_destination_crs: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_trains \
         SET train_uid = $2, resolution_status = 'schedule_matched', matched_line_id = $3, \
             schedule_calling_points = $4, schedule_destination_crs = $5, schedule_matched_at = NOW() \
         WHERE id = $1 AND train_uid IS NULL AND resolution_status = 'pending'",
    )
    .bind(tracked_train_id)
    .bind(train_uid)
    .bind(matched_line_id)
    .bind(schedule_calling_points)
    .bind(schedule_destination_crs)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

After:
```rust
/// As of this task, this ONLY flips `resolution_status` -- every schedule
/// column this used to also write now lives exclusively on the shared
/// `trains` row (`schedule_matching::attempt_schedule_match`'s own
/// `find_or_create_train_with_schedule_match` call, Task 3). Guarded on
/// `trains_id IS NULL` rather than the old `train_uid IS NULL` -- since
/// Task 8's read cutover, `tracked_trains.train_uid` is no longer the
/// signal anything trusts for "has this pin been schedule-matched yet."
pub async fn apply_schedule_match(
    pool: &PgPool,
    tracked_train_id: i64,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_trains SET resolution_status = 'schedule_matched' \
         WHERE id = $1 AND trains_id IS NULL AND resolution_status = 'pending'",
    )
    .bind(tracked_train_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

`list_pending_pins_for_schedule_match` (`crates/api/src/data/train_tracking.rs:529-539`) — same guard-column swap:
```rust
pub async fn list_pending_pins_for_schedule_match(
    pool: &PgPool,
) -> anyhow::Result<Vec<PendingSchedulePin>> {
    let rows = sqlx::query_as::<_, PendingSchedulePin>(
        "SELECT id, service_date, pin_origin_crs, pin_scheduled_departure \
         FROM tracked_trains WHERE trains_id IS NULL AND resolution_status = 'pending'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

`list_active_tracked_trains` (Task 11/17's own cumulative version) — its
last remaining read of `tt.train_uid`/`tt.train_id` now moves to the
joined `trains` row, since those two columns on `tracked_trains` itself
stop being written as of this task:
```rust
pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRow>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tr.train_uid, tr.train_id, tt.trains_id \
         FROM tracked_trains tt \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TrackedTrainRef::from).collect())
}
```

`crates/api/src/data/schedule_matching.rs`'s `attempt_schedule_match` call site (`:143-151`) drops the now-removed arguments:
```rust
let matched_ok = train_tracking::apply_schedule_match(pool, tracked_train_id).await?;

if matched_ok {
    let trains_id = crate::data::trains::find_or_create_train_with_schedule_match(
        pool,
        &matched.uid,
        service_date,
        pin_origin_crs,
        pin_scheduled_departure,
        destination_crs.as_deref(),
        line_id,
        &calling_points_json,
    )
    .await?;
    sqlx::query("UPDATE tracked_trains SET trains_id = $2 WHERE id = $1")
        .bind(tracked_train_id)
        .bind(trains_id)
        .execute(pool)
        .await?;
}

return Ok(matched_ok);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p api attempt_schedule_match -- --ignored --test-threads=1`
Expected: PASS — both of Task 3's own pre-existing tests in this module
still pass unchanged, proving the read (Task 8) and write (this task) sides
agree. Also `cargo test -p trust-consumer` in full, to confirm
`apply_reference_reload`'s `by_train_uid` population (Task 16) still works
correctly now that `list_active_tracked_trains` sources `train_uid` from
`trains` rather than `tracked_trains` directly.

- [ ] **Step 5: Commit**
```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/data/schedule_matching.rs
git commit -m "Retire tracked_trains' own legacy schedule/identity column writes"
```

## Task 22: Final Step D cutover — dry-run verification, then drop every retired legacy column

**Files:**
- Create: `crates/api/migrations/20260906140000_drop_legacy_columns.sql`
- Modify: `crates/api/src/data/train_tracking.rs` (`flip_legacy_resolution`'s `UPDATE` drops its own now-dead `resolved_at` write)

**Interfaces:**
- Consumes: every prior task in this Step D sequence (Tasks 9-11, 21) having already stopped reading AND writing the columns this migration drops.
- Produces: `train_movement_events`/`train_current_state` lose `tracked_train_id` entirely — `trains_id` is now their only train-identity column. `tracked_trains` loses `train_uid, train_id, matched_line_id, schedule_calling_points, schedule_destination_crs, schedule_matched_at, resolved_at`. This is the plan's own Global Constraints' "final, irreversible act" — gated on a dry-run row-count comparison, never bundled into the migration that added `trains_id` (Task 9).

- [ ] **Step 1: Dry-run verification (run BEFORE writing/applying the migration)**

```bash
psql "$DATABASE_URL" -c "SELECT count(*) FROM train_movement_events WHERE tracked_train_id IS NOT NULL AND trains_id IS NULL;"
psql "$DATABASE_URL" -c "SELECT count(*) FROM train_current_state WHERE tracked_train_id IS NOT NULL AND trains_id IS NULL;"
```
Both counts should be small and explainable entirely by Task 6's own
accepted-gap figure (rows whose owning subscription never had a `train_uid`
to backfill by). If either count is unexpectedly large — bigger than what
Task 6 already accepted — STOP: do not proceed with this migration until
the discrepancy is understood, since dropping `tracked_train_id` at that
point would silently and permanently lose those rows' only identity link.

- [ ] **Step 2: Run test to verify it fails**

```bash
psql "$DATABASE_URL" -c "SELECT column_name FROM information_schema.columns WHERE table_name = 'tracked_trains' AND column_name = 'train_uid';"
```
Expected: one row, `train_uid` — confirming the column still exists before
this migration removes it.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906140000_drop_legacy_columns.sql
-- Step D's final, irreversible act
-- (docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §2) --
-- gated on Task 22's own Step 1 dry-run row-count comparison, run manually
-- BEFORE this migration is ever applied to a real database. Bundles both
-- drops the design spec names together in its own closing paragraph: the
-- movement-table tracked_train_id columns, and tracked_trains' own
-- fully-retired legacy schedule/identity columns (Tasks 11/21 already
-- stopped writing every one of them; Task 8 already stopped reading them).

-- train_movement_events: drop the OLD dedup constraint before the column
-- it references, then the column itself. The new trains_id-keyed dedup
-- index (Task 9) is untouched -- it becomes this table's ONLY dedup
-- constraint from here on.
ALTER TABLE train_movement_events
    DROP CONSTRAINT IF EXISTS train_movement_events_tracked_train_id_dedup_key_key;
ALTER TABLE train_movement_events DROP COLUMN tracked_train_id;

-- train_current_state: drop the OLD partial-unique index (Task 9) before
-- the column it references, then the column itself. trains_id's own
-- partial unique index (Task 9) is untouched.
DROP INDEX IF EXISTS train_current_state_tracked_train_id;
ALTER TABLE train_current_state DROP COLUMN tracked_train_id;

-- tracked_trains: drop the OLD resolved-identity index before train_uid
-- (the column it references), then every retired legacy column.
DROP INDEX IF EXISTS tracked_trains_resolved_identity;
ALTER TABLE tracked_trains
    DROP COLUMN train_uid,
    DROP COLUMN train_id,
    DROP COLUMN matched_line_id,
    DROP COLUMN schedule_calling_points,
    DROP COLUMN schedule_destination_crs,
    DROP COLUMN schedule_matched_at,
    DROP COLUMN resolved_at;
```

`flip_legacy_resolution` (Task 11) still writes `tracked_trains.resolved_at`
as part of its resolution `UPDATE` — this must stop too, in the SAME task
that drops the column, or every subsequent live-TRUST resolution would
start erroring:
```rust
// crates/api/src/data/train_tracking.rs -- flip_legacy_resolution's UPDATE
let row: Option<(Option<i64>, chrono::NaiveDate)> = sqlx::query_as(
    "UPDATE tracked_trains SET resolution_status = 'resolved' \
     WHERE id = $1 RETURNING trains_id, service_date",
)
.bind(tracked_train_id)
.fetch_optional(pool)
.await?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `psql "$DATABASE_URL" -c "SELECT column_name FROM information_schema.columns WHERE table_name = 'tracked_trains' AND column_name = 'train_uid';"`
Expected: empty result (the column is gone). Then `cargo build -p api -p notifier -p trust-consumer -p trust-backlog-consumer` in full, to catch any remaining reference to a dropped column across the workspace (there should be none, per every prior task in this sequence), and `cargo test -p api` (unit tests only, no `DATABASE_URL` needed) to confirm nothing else broke at compile time.

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906140000_drop_legacy_columns.sql crates/api/src/data/train_tracking.rs
git commit -m "Step D final cutover: drop tracked_train_id and every retired legacy column"
```

## Task 23: `prune_trains` — retention job in `crates/aggregator`

**Files:**
- Modify: `crates/aggregator/src/queries.rs` (`prune_trains`)
- Modify: `crates/aggregator/src/config.rs` (`trains_retention_days`)
- Modify: `crates/aggregator/src/main.rs` (`run_cycle`'s param list and its call site)
- Test: `crates/aggregator/src/queries.rs` (extends the existing `#[cfg(test)]` module covering `prune_trust_event_backlog`/the other prune jobs)

**Interfaces:**
- Consumes: table `trains` (Task 1); `ON DELETE CASCADE` from `train_movement_events`/`train_current_state` (Task 9) does the rest inside the same `DELETE` statement; `ON DELETE SET NULL` from `tracked_trains.trains_id` (Task 1) is what makes a pruned train's subscription survive as a historical record.
- Produces: `pub async fn prune_trains(pool: &PgPool, retention_days: i64) -> Result<u64>`, wired into `run_cycle`'s existing per-cycle prune-job list, alongside `prune_history`/`prune_trust_event_backlog`/the daily/half-hourly stats prune jobs. Default `30` days, per this plan's Global Constraints — no licence-confirmation caution needed here, unlike `trust_event_backlog_retention_days`'s own cautious default-1 posture, since the licensing question is already closed for this data.

- [ ] **Step 1: Write the failing test**

```rust
// crates/aggregator/src/queries.rs -- appended, alongside prune_trust_event_backlog_deletes_only_rows_older_than_the_retention_window
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            prune_trains_deletes_only_rows_older_than_the_retention_window -- --ignored --test-threads=1`"]
async fn prune_trains_deletes_only_rows_older_than_the_retention_window() {
    let pool = connect().await;
    let old_date: chrono::NaiveDate = (chrono::Utc::now().date_naive() - chrono::Duration::days(40));
    let recent_date: chrono::NaiveDate = (chrono::Utc::now().date_naive() - chrono::Duration::days(5));

    let (old_id,): (i64,) = sqlx::query_as(
        "INSERT INTO trains (train_uid, service_date) VALUES ('TEST-PRUNE-TRAINS-OLD', $1) RETURNING id",
    )
    .bind(old_date)
    .fetch_one(&pool)
    .await
    .expect("seed an old trains row");
    let (recent_id,): (i64,) = sqlx::query_as(
        "INSERT INTO trains (train_uid, service_date) VALUES ('TEST-PRUNE-TRAINS-RECENT', $1) RETURNING id",
    )
    .bind(recent_date)
    .fetch_one(&pool)
    .await
    .expect("seed a recent trains row");

    let pruned = prune_trains(&pool, 30).await.expect("prune_trains");
    assert!(pruned >= 1);

    let old_still_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trains WHERE id = $1)")
        .bind(old_id)
        .fetch_one(&pool)
        .await
        .expect("check old row");
    assert!(!old_still_exists, "a 40-day-old trains row must be pruned under a 30-day retention window");

    let recent_still_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trains WHERE id = $1)")
        .bind(recent_id)
        .fetch_one(&pool)
        .await
        .expect("check recent row");
    assert!(recent_still_exists, "a 5-day-old trains row must survive a 30-day retention window");

    sqlx::query("DELETE FROM trains WHERE id = $1").bind(recent_id).execute(&pool).await.ok();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=... cargo test -p aggregator prune_trains_deletes_only_rows_older_than_the_retention_window -- --ignored --test-threads=1`
Expected: FAIL with a compile error — `prune_trains` is not defined yet.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/aggregator/src/queries.rs -- appended, after prune_trust_event_backlog
/// Prunes `trains` rows older than `retention_days`, per
/// docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §5.
/// A single, backward-only predicate on the parent row -- `ON DELETE
/// CASCADE` on `train_movement_events`/`train_current_state` (Task 9)
/// does the rest inside this same statement; `tracked_trains.trains_id`'s
/// `ON DELETE SET NULL` (Task 1) is what makes a pruned train's per-user
/// subscription survive this delete as a no-live-data historical record.
pub async fn prune_trains(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM trains WHERE service_date < CURRENT_DATE - ($1 || ' days')::interval",
    )
    .bind(retention_days.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

```rust
// crates/aggregator/src/config.rs -- new field, alongside trust_event_backlog_retention_days
/// How long to keep `trains` (and, via CASCADE,
/// `train_movement_events`/`train_current_state`) rows before pruning
/// them. Reuses the past-dates sibling design's own 30-day figure
/// (docs/superpowers/specs/2026-09-06-schedule-line-population-past-dates-design.md)
/// rather than inventing a second number for a structurally similar
/// concern -- see this plan's Global Constraints. Unlike
/// `trust_event_backlog_retention_days`'s cautious default-1-until-licence-
/// confirmed posture, this can default straight to 30 from day one: the
/// RDM licensing question that caution exists to enforce has already been
/// confirmed clear by the repo owner for this data.
#[arg(long, env, default_value_t = 30)]
pub trains_retention_days: i64,
```

`crates/aggregator/src/main.rs` — `run_cycle` gains one more parameter and
one more prune call, alongside the existing `trust_event_backlog` one:
```rust
async fn run_cycle(
    pool: &sqlx::PgPool,
    static_lines: &HashMap<String, LineDefinition>,
    defaults: &Defaults,
    retention_days: i64,
    daily_stats_retention_days: i64,
    half_hourly_stats_retention_hours: i64,
    trust_event_backlog_retention_days: i64,
    trains_retention_days: i64,
    dedup_ledger: &mut SeenServiceLedger,
    full_coverage_enabled_default: bool,
) -> anyhow::Result<()> {
    // ... unchanged body up to the trust_event_backlog prune block ...

    let trains_pruned = queries::prune_trains(pool, trains_retention_days).await?;
    metrics::counter!(common::metrics::metric_name("aggregator_trains_rows_pruned_total"))
        .increment(trains_pruned);

    // ... rest of the function unchanged, with trains_pruned added to the
    // final tracing::info! call alongside trust_event_backlog_pruned ...
}
```
and its one call site:
```rust
let result = run_cycle(
    &pool,
    &static_lines,
    &defaults,
    config.history_retention_days,
    config.daily_stats_retention_days,
    config.half_hourly_stats_retention_hours,
    config.trust_event_backlog_retention_days,
    config.trains_retention_days,
    &mut dedup_ledger,
    config.full_coverage_enabled_default,
)
.await;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=... cargo test -p aggregator prune_trains_deletes_only_rows_older_than_the_retention_window -- --ignored --test-threads=1`
Expected: PASS. Also `cargo test -p aggregator` (unit tests) to confirm `run_cycle`'s new parameter didn't break its own existing tests.

- [ ] **Step 5: Commit**
```bash
git add crates/aggregator/src/queries.rs crates/aggregator/src/config.rs crates/aggregator/src/main.rs
git commit -m "Add prune_trains retention job, wired into aggregator's per-cycle prune list"
```

## Task 24: Rename `tracked_trains` → `train_subscriptions` (final, cosmetic)

**Sequenced deliberately last**, per the design spec's own explicit instruction (§1): "Sequence this rename as a late, purely cosmetic step, after every functional change below has landed and been verified — never as an early or combined step." Every task before this one already works against whatever the table is named at the time; this task changes only the name, not any behavior.

**Files:**
- Create: `crates/api/migrations/20260906150000_rename_tracked_trains.sql`
- Modify: every file a `grep -rln "tracked_trains" crates/` turns up with a raw SQL string reference — confirmed directly, this is exactly: `crates/api/src/data/{schedule_matching,trust_event_backlog_match,queries,train_tracking}.rs`, `crates/api/src/routes/{ingest,train}.rs`, `crates/notifier/src/queries.rs`. `crates/common/src/lib.rs`/`crates/common/src/ingest.rs` and `crates/trust-consumer/src/{config,queries,main,process}.rs` also match that grep, but only in doc-comment prose (e.g. describing `TrackedTrainRef`'s purpose) or in historical citations of an old migration's literal filename — neither is a live SQL string, so neither needs changing; a citation of `20260828120000_train_tracking.sql`'s own filename must stay exactly as written, since renaming it would misdescribe what that file was actually named at the time it was authored.

**Interfaces:**
- Consumes: nothing new.
- Produces: the table is named `train_subscriptions` everywhere in the schema and in every live SQL string in this codebase. **Rust identifiers are explicitly out of scope for this task** — `tracked_train_id` (the FK column name, still used by `tracked_train_tickets`/`train_notification_state`), `TrackedTrainRef`/`TrackedTrainState`/`TrackedTrainListItem`/`TrackedTrainRow`/`TrackedTrainTicket`, and function names like `tracked_train_owner`/`delete_tracked_train`/`rename_tracked_train`/`list_active_tracked_trains` are all left exactly as they are — the design spec's own §1 names only the TABLE's rename, and grepping for the exact string `tracked_trains` (plural) never matches any of those (all singular: `tracked_train_id`, `TrackedTrainRef`, etc.), which is precisely why that grep is this task's own correct verification, not an incidental side effect.

- [ ] **Step 1: Write the failing test**

```bash
grep -rn "tracked_trains" crates/api/src/data/*.rs crates/api/src/routes/*.rs crates/notifier/src/queries.rs
```
Expected: non-empty — every raw SQL string in these files still says
`tracked_trains` (e.g. `"FROM tracked_trains tt"`, `"UPDATE tracked_trains
SET ..."`, `"INSERT INTO tracked_trains ..."`).

- [ ] **Step 2: Run test to verify it fails**

Run the same command as Step 1. Expected: it prints every remaining
occurrence — confirming there's real, mechanical work left to do.

- [ ] **Step 3: Write minimal implementation**

```sql
-- crates/api/migrations/20260906150000_rename_tracked_trains.sql
-- Final, purely cosmetic step of
-- docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §1/§2 --
-- every functional change in this plan has already landed and been
-- verified by this point (Tasks 1-23). Index renames are optional/cosmetic
-- (Postgres does not require them -- a foreign key `REFERENCES
-- tracked_trains(id)` from another table follows a table rename
-- automatically, by OID, with no SQL text change needed anywhere else),
-- included here anyway for one consistent naming convention across the
-- whole schema.
ALTER TABLE tracked_trains RENAME TO train_subscriptions;
ALTER INDEX tracked_trains_user_id RENAME TO train_subscriptions_user_id;
ALTER INDEX tracked_trains_trains_id RENAME TO train_subscriptions_trains_id;
ALTER INDEX tracked_trains_resolution_status RENAME TO train_subscriptions_resolution_status;
```

Then, in each of `crates/api/src/data/schedule_matching.rs`,
`crates/api/src/data/trust_event_backlog_match.rs`,
`crates/api/src/data/queries.rs`, `crates/api/src/data/train_tracking.rs`,
`crates/api/src/routes/ingest.rs`, `crates/api/src/routes/train.rs`, and
`crates/notifier/src/queries.rs`: replace every raw SQL string's
`tracked_trains` table reference with `train_subscriptions` — `FROM
tracked_trains`, `UPDATE tracked_trains`, `INSERT INTO tracked_trains`,
`JOIN tracked_trains`, and any `REFERENCES tracked_trains` inside a
migration-adjacent inline comment quoting SQL — leaving every Rust
identifier (table aliases like `tt` are fine to keep, since they're just a
local SQL alias, not the table name itself) untouched. This is inherently
repetitive/mechanical rather than needing bespoke logic per call site — a
careful find/replace of the quoted string `tracked_trains` (not
`tracked_train_id`, not `TrackedTrain...`) inside each file's SQL string
literals is the entire task.

Each test file's own fixture SQL (e.g. `INSERT INTO tracked_trains (...)`
inside a `#[cfg(test)] mod db_tests` block) is included in this same
sweep — a test seeding a row through the OLD table name would simply fail
against the renamed schema, so these are not optional to skip.

- [ ] **Step 4: Run test to verify it passes**

Run: `grep -rn "tracked_trains" crates/`
Expected: **zero hits**, anywhere in the workspace — the sweep is
complete. Then run the full test suite for every crate this task touched:
`DATABASE_URL=... cargo test -p api -p notifier -- --ignored --test-threads=1`
(plus each crate's own non-`--ignored` unit tests,
`cargo test -p api -p notifier -p trust-consumer -p trust-backlog-consumer -p aggregator`),
to confirm every query still resolves against the renamed table.

- [ ] **Step 5: Commit**
```bash
git add crates/api/migrations/20260906150000_rename_tracked_trains.sql \
        crates/api/src/data/schedule_matching.rs crates/api/src/data/trust_event_backlog_match.rs \
        crates/api/src/data/queries.rs crates/api/src/data/train_tracking.rs \
        crates/api/src/routes/ingest.rs crates/api/src/routes/train.rs \
        crates/notifier/src/queries.rs
git commit -m "Rename tracked_trains to train_subscriptions (final, cosmetic)"
```

---

Plan complete and saved to `docs/superpowers/plans/2026-09-06-shared-train-identity-implementation-plan.md`. Two execution options:

1. **Subagent-Driven (recommended)** - dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** - execute tasks in this session using executing-plans, batch execution with checkpoints
