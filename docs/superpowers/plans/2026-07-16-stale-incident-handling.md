# Stale Incident Handling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop incidents that are cleared, temporally expired, or simply stale (still nominally "active" per RDM but past the next UK rail traffic-day boundary) from continuing to drive a line's displayed status.

**Architecture:** A new `incidents.first_seen_at` column, stamped once on first insert and never touched again, gives the aggregator its own clock for incident age — independent of RDM's own (sometimes-stale) `is_cleared`/`validity_periods` fields. `aggregator::queries::load_incidents` filters cleared rows at the SQL layer and returns `first_seen_at` alongside each incident; a new pure `is_active` predicate in `aggregator::aggregation` then filters on validity-window expiry and a rail-day-based age cutoff (02:00 Europe/London, exempting planned engineering work) before an incident is allowed to produce a `LineStatus`.

**Tech Stack:** Rust/sqlx/axum (`crates/api`, `crates/aggregator`), PostgreSQL, `chrono` + new `chrono-tz` dependency.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md`.
- `common::IncidentMessage`'s wire shape (poller ↔ API contract) does not change — `first_seen_at` is a database-only fact, never sent by or read by a poller.
- No pruning/deletion of `incidents` rows, and no "hasn't been refreshed in N poll cycles" signal — both explicitly out of scope per the spec's Non-goals.
- The rail-day boundary hour (02:00 Europe/London) is a hardcoded constant, not configurable.
- This codebase has no existing pattern for DB-backed automated tests of query functions (confirmed: `upsert_stations`/`upsert_tocs`/`upsert_station_samples`/the original `load_incidents` have none; only pure helpers extracted from them, like `incident_changed`/`normalize_for_diff`, are unit tested). Task 4 below introduces one `#[ignore]`d integration test as a deliberate, minimal exception — not a new general pattern to extend elsewhere in this plan.

---

### Task 1: `first_seen_at` column, wired through `upsert_incidents`

**Files:**
- Create: `crates/api/migrations/20260716180000_incident_first_seen.sql`
- Modify: `crates/api/src/data/queries.rs:71-100` (`upsert_incidents`'s `INSERT` statement)

**Interfaces:**
- Produces: `incidents.first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`, set once on insert, preserved across every later update. Consumed by Task 4's `load_incidents`.

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260716180000_incident_first_seen.sql`:

```sql
-- -------------------------------------------------------------------------
-- Incidents: track when we first saw each incident_id, independent of
-- anything RDM reports. `upsert_incidents` (crates/api/src/data/queries.rs)
-- sets this once on INSERT and never touches it again on UPDATE -- it's
-- our own clock for incident age, immune to RDM leaving is_cleared/
-- validity_periods stale after an edit. See
-- docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md.
-- -------------------------------------------------------------------------

ALTER TABLE incidents
    ADD COLUMN first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
```

- [ ] **Step 2: Update `upsert_incidents`'s INSERT to set `first_seen_at` once**

In `crates/api/src/data/queries.rs`, change the `INSERT` statement inside `upsert_incidents` (currently lines 71-100):

```rust
        sqlx::query(
            r#"
            INSERT INTO incidents (
                incident_id, summary, description, operators, affected_stations,
                priority, validity_periods, is_planned, is_cleared, fetched_at,
                first_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
            ON CONFLICT (incident_id) DO UPDATE SET
                summary           = EXCLUDED.summary,
                description       = EXCLUDED.description,
                operators         = EXCLUDED.operators,
                affected_stations = EXCLUDED.affected_stations,
                priority          = EXCLUDED.priority,
                validity_periods  = EXCLUDED.validity_periods,
                is_planned        = EXCLUDED.is_planned,
                is_cleared        = EXCLUDED.is_cleared,
                fetched_at        = NOW()
            "#,
        )
        .bind(&incident.incident_id)
        .bind(&incident.summary)
        .bind(&incident.description)
        .bind(&incident.operators)
        .bind(&incident.affected_stations)
        .bind(incident.priority)
        .bind(&validity_json)
        .bind(incident.is_planned)
        .bind(incident.is_cleared)
        .execute(&mut *tx)
        .await?;
```

(Only the column list, `VALUES` list, and the added trailing `, NOW()` changed — no bind calls change, since `first_seen_at` is set via a literal `NOW()` in `VALUES`, not a bound parameter. `first_seen_at` is deliberately **absent** from `ON CONFLICT DO UPDATE SET` — that omission is what preserves the original insert-time value forever after.)

- [ ] **Step 3: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS — this change doesn't touch any tested pure function (`incident_changed` is unaffected), so this just confirms the crate still compiles and the existing suite is unaffected.

- [ ] **Step 4: Commit**

```bash
git add crates/api/migrations/20260716180000_incident_first_seen.sql crates/api/src/data/queries.rs
git commit -m "Track first_seen_at per incident, set once and never updated"
```

---

### Task 2: Rail-day boundary math

**Files:**
- Modify: `crates/aggregator/Cargo.toml` (add `chrono-tz` dependency)
- Modify: `crates/aggregator/src/aggregation.rs` (new `next_rail_day_boundary` function + imports)
- Test: `crates/aggregator/src/aggregation.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `fn next_rail_day_boundary(first_seen_at: DateTime<Utc>) -> DateTime<Utc>` — the next UK rail traffic-day boundary (02:00 Europe/London) strictly after `first_seen_at`. Consumed by Task 3's `is_active`.

- [ ] **Step 1: Add the `chrono-tz` dependency**

```bash
cd crates/aggregator
cargo add chrono-tz@0.10
cd ../..
```

Expected: `crates/aggregator/Cargo.toml` gains a `chrono-tz = "0.10"` line under `[dependencies]`.

- [ ] **Step 2: Write the failing tests**

Add to `crates/aggregator/src/aggregation.rs`'s existing `mod tests` block (the file already has `use super::*;` there, which brings in whatever top-level imports Step 3 below adds):

```rust
    #[test]
    fn next_rail_day_boundary_on_a_plain_midweek_day() {
        // 2026-07-15 13:00 UTC is 14:00 BST (July is daylight saving) --
        // still well before that rail day's 02:00-the-next-day end, so the
        // boundary is 2026-07-16 02:00 BST = 2026-07-16 01:00 UTC.
        let first_seen_at: DateTime<Utc> = "2026-07-15T13:00:00Z".parse().unwrap();
        let boundary = next_rail_day_boundary(first_seen_at);
        assert_eq!(boundary, "2026-07-16T01:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn next_rail_day_boundary_just_before_local_0200_stays_in_the_earlier_rail_day() {
        // 2026-07-16 00:30 UTC is 01:30 BST -- still inside the rail day
        // that started 2026-07-15 02:00 BST, so the boundary is only 30
        // local minutes away: 2026-07-16 02:00 BST = 2026-07-16 01:00 UTC.
        let first_seen_at: DateTime<Utc> = "2026-07-16T00:30:00Z".parse().unwrap();
        let boundary = next_rail_day_boundary(first_seen_at);
        assert_eq!(boundary, "2026-07-16T01:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn next_rail_day_boundary_just_after_local_0200_rolls_to_the_next_rail_day() {
        // 2026-07-16 01:05 UTC is 02:05 BST -- just past that day's 02:00,
        // so it belongs to the rail day that just started, and the next
        // boundary is a full rail day away: 2026-07-17 02:00 BST = 01:00 UTC.
        let first_seen_at: DateTime<Utc> = "2026-07-16T01:05:00Z".parse().unwrap();
        let boundary = next_rail_day_boundary(first_seen_at);
        assert_eq!(boundary, "2026-07-17T01:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn next_rail_day_boundary_across_the_spring_forward_transition() {
        // UK clocks spring forward at 01:00 GMT -> 02:00 BST on the last
        // Sunday in March (2026-03-29). 2026-03-29 01:30 GMT (=01:30 UTC)
        // is before that day's local 02:00 (which, at the exact instant of
        // the jump, is already BST) -- so the boundary is that same day's
        // 02:00 BST = 2026-03-29 01:00 UTC.
        let first_seen_at: DateTime<Utc> = "2026-03-29T01:30:00Z".parse().unwrap();
        let boundary = next_rail_day_boundary(first_seen_at);
        assert_eq!(boundary, "2026-03-29T01:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn next_rail_day_boundary_across_the_autumn_fallback_transition() {
        // UK clocks fall back at 02:00 BST -> 01:00 GMT on the last Sunday
        // in October (2026-10-25). 2026-10-25 00:30 UTC is 01:30 BST --
        // before that day's local 02:00, which (after the fallback
        // completes) resolves as GMT -- so the boundary is 2026-10-25
        // 02:00 GMT = 2026-10-25 02:00 UTC.
        let first_seen_at: DateTime<Utc> = "2026-10-25T00:30:00Z".parse().unwrap();
        let boundary = next_rail_day_boundary(first_seen_at);
        assert_eq!(boundary, "2026-10-25T02:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p aggregator next_rail_day_boundary`
Expected: FAIL — compile error, `next_rail_day_boundary` is not defined yet.

- [ ] **Step 4: Implement `next_rail_day_boundary`**

In `crates/aggregator/src/aggregation.rs`, change the top-of-file `chrono` import and add the new function (a good spot is right after `validity_for_output`, since both are "pick/derive a time-related fact" helpers):

```rust
use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
```

(Replaces the existing `use chrono::Utc;`.)

```rust
/// The next UK rail "traffic day" boundary after `first_seen_at` -- 02:00
/// Europe/London, per Network Rail's timetable convention (a traffic day
/// runs 02:00-01:59, not a midnight-to-midnight calendar day). If
/// `first_seen_at`'s local time-of-day is before 02:00, it belongs to the
/// previous calendar day's rail day, so the boundary is that same calendar
/// day's 02:00; otherwise it's the next calendar day's 02:00.
///
/// UK clocks change exactly at the 01:00/02:00 boundary in both directions
/// (spring: 01:00 GMT -> 02:00 BST; autumn: 02:00 BST -> 01:00 GMT), so
/// local 02:00 itself is never ambiguous or missing on a transition day --
/// only 01:00-01:59 is. `LocalResult::Single` is therefore the only case
/// expected for real UK dates; anything else is treated as a defensive
/// failure rather than left to a confusing bare-unwrap panic.
fn next_rail_day_boundary(first_seen_at: DateTime<Utc>) -> DateTime<Utc> {
    let local = first_seen_at.with_timezone(&chrono_tz::Europe::London);
    let boundary_time = NaiveTime::from_hms_opt(2, 0, 0).expect("2:00:00 is a valid time");

    let boundary_date = if local.time() < boundary_time {
        local.date_naive()
    } else {
        local.date_naive() + Duration::days(1)
    };
    let boundary_naive = boundary_date.and_time(boundary_time);

    match chrono_tz::Europe::London.from_local_datetime(&boundary_naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        other => panic!(
            "unexpected {other:?} resolving rail-day boundary {boundary_naive} in Europe/London; \
             02:00 local should never be ambiguous or missing"
        ),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p aggregator next_rail_day_boundary`
Expected: PASS — all 5 new tests pass.

- [ ] **Step 6: Run the full aggregator crate test suite**

Run: `cargo test -p aggregator`
Expected: PASS, no regressions (the `Utc` import change is additive — `DateTime`/`Duration`/`TimeZone` were not previously imported by name but are now available; existing code that referred to them via fully-qualified paths like `chrono::Duration::days(2)` still compiles unchanged).

- [ ] **Step 7: Commit**

```bash
git add crates/aggregator/Cargo.toml crates/aggregator/Cargo.lock crates/aggregator/src/aggregation.rs
git commit -m "Add rail-day boundary calculation for incident staleness"
```

---

### Task 3: `is_active` filtering predicate

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs`
- Test: `crates/aggregator/src/aggregation.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `next_rail_day_boundary` (Task 2).
- Produces: `fn is_active(incident: &IncidentMessage, first_seen_at: DateTime<Utc>, now: DateTime<Utc>) -> bool`. Consumed by Task 5's `aggregate()`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/aggregator/src/aggregation.rs`'s `mod tests` block (reuses the existing `incident(...)` test helper already defined there):

```rust
    #[test]
    fn is_active_true_for_fresh_incident_with_no_validity_periods() {
        let inc = incident("T1", "Delay", "Delay description", &[], &[]);
        let now = Utc::now();
        assert!(is_active(&inc, now, now));
    }

    #[test]
    fn is_active_false_when_the_only_validity_period_has_elapsed() {
        let mut inc = incident("T2", "Delay", "Delay description", &[], &[]);
        let now = Utc::now();
        inc.validity = vec![ValidityPeriod {
            from_date: now - Duration::days(2),
            to_date: Some(now - Duration::days(1)),
            is_now: false,
        }];
        assert!(!is_active(&inc, now - Duration::days(2), now));
    }

    #[test]
    fn is_active_true_when_a_validity_period_covers_now() {
        let mut inc = incident("T3", "Delay", "Delay description", &[], &[]);
        let now = Utc::now();
        inc.validity = vec![ValidityPeriod { from_date: now - Duration::hours(1), to_date: None, is_now: true }];
        assert!(is_active(&inc, now - Duration::hours(1), now));
    }

    #[test]
    fn is_active_false_for_non_planned_incident_aged_past_the_rail_day_boundary() {
        let inc = incident("T4", "Delay", "Delay description", &[], &[]);
        let now = Utc::now();
        let first_seen_at = now - Duration::days(2);
        assert!(!is_active(&inc, first_seen_at, now));
    }

    #[test]
    fn is_active_true_for_planned_incident_aged_past_the_rail_day_boundary() {
        let mut inc = incident("T5", "Engineering work", "Planned engineering work", &[], &[]);
        inc.is_planned = true;
        let now = Utc::now();
        let first_seen_at = now - Duration::days(2);
        assert!(is_active(&inc, first_seen_at, now));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aggregator is_active`
Expected: FAIL — compile error, `is_active` is not defined yet.

- [ ] **Step 3: Implement `is_active`**

In `crates/aggregator/src/aggregation.rs`, add (right after `next_rail_day_boundary`):

```rust
/// Whether an incident should still contribute a `LineStatus` to any line
/// it matches. `is_cleared` isn't rechecked here -- `queries::load_incidents`
/// already excludes cleared rows at the SQL layer, so by the time an
/// incident reaches this function it's already known not to be cleared.
fn is_active(incident: &IncidentMessage, first_seen_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let validity_ok = incident.validity.is_empty()
        || incident
            .validity
            .iter()
            .any(|p| p.from_date <= now && p.to_date.map(|to| to > now).unwrap_or(true));
    let age_ok = incident.is_planned || now < next_rail_day_boundary(first_seen_at);
    validity_ok && age_ok
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aggregator is_active`
Expected: PASS — all 5 new tests pass.

- [ ] **Step 5: Run the full aggregator crate test suite**

Run: `cargo test -p aggregator`
Expected: PASS, no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/aggregator/src/aggregation.rs
git commit -m "Add is_active predicate for incident staleness"
```

---

### Task 4: `LoadedIncident` + filter cleared rows in `load_incidents`

**Files:**
- Modify: `crates/aggregator/src/queries.rs`
- Test: `crates/aggregator/src/queries.rs` (new `#[ignore]`d integration test)

**Interfaces:**
- Consumes: `incidents.first_seen_at` (Task 1).
- Produces: `pub struct LoadedIncident { pub message: IncidentMessage, pub first_seen_at: DateTime<Utc> }`; `load_incidents`'s return type changes from `Result<Vec<IncidentMessage>>` to `Result<Vec<LoadedIncident>>`. Consumed by Task 5's `aggregate()` (via `main.rs`'s existing call site, which needs no change of its own — see Task 5).

- [ ] **Step 1: Write the failing test**

Add to `crates/aggregator/src/queries.rs`'s existing `mod tests` block (currently only tests `normalize_for_diff`; add the new imports alongside the existing `use super::*;`):

```rust
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
                load_incidents_excludes_cleared_rows -- --ignored` against docker compose's postgres"]
    async fn load_incidents_excludes_cleared_rows() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, validity_periods, is_planned, is_cleared) \
             VALUES \
                ('TEST-ACTIVE', 'active', 'active incident', '{}', '{}', 0, '[]', false, false), \
                ('TEST-CLEARED', 'cleared', 'cleared incident', '{}', '{}', 0, '[]', false, true) \
             ON CONFLICT (incident_id) DO UPDATE SET is_cleared = EXCLUDED.is_cleared",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let loaded = load_incidents(&pool).await.expect("load_incidents");
        let ids: Vec<&str> = loaded.iter().map(|i| i.message.incident_id.as_str()).collect();

        sqlx::query("DELETE FROM incidents WHERE incident_id IN ('TEST-ACTIVE', 'TEST-CLEARED')")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        assert!(ids.contains(&"TEST-ACTIVE"), "non-cleared incident should be loaded");
        assert!(!ids.contains(&"TEST-CLEARED"), "cleared incident should be excluded");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p aggregator load_incidents_excludes_cleared_rows -- --ignored`
Expected: FAIL — compile error (`load_incidents` doesn't yet return something with a `.message` field) once Step 3 below hasn't happened yet; if run before Task 1's migration is applied to your local dev database, it would separately fail at runtime with a missing-column error — apply Task 1's migration first (`sqlx::migrate!()` runs automatically when the `api` crate boots, e.g. via `docker compose --profile dev up -d api-dev`, or run it manually with `sqlx migrate run` from `crates/api`).

- [ ] **Step 3: Change `load_incidents`'s return shape and add the `WHERE NOT is_cleared` filter**

In `crates/aggregator/src/queries.rs`, change the top-of-file import and `load_incidents`:

```rust
use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{IncidentMessage, LineStatusReport, StationSample};
use sqlx::{PgPool, Row};
```

```rust
/// One incident loaded from the `incidents` table for this aggregation
/// cycle, paired with our own `first_seen_at` clock. Deliberately not part
/// of `common::IncidentMessage` -- the wire type pollers/the API share --
/// since `first_seen_at` is a fact only this crate's staleness check cares
/// about. See docs/superpowers/specs/2026-07-16-stale-incident-handling-design.md.
pub struct LoadedIncident {
    pub message: IncidentMessage,
    pub first_seen_at: DateTime<Utc>,
}

pub async fn load_incidents(pool: &PgPool) -> Result<Vec<LoadedIncident>> {
    let rows = sqlx::query(
        "SELECT incident_id, summary, description, operators, affected_stations, \
                priority, validity_periods, is_planned, is_cleared, first_seen_at \
         FROM incidents \
         WHERE NOT is_cleared",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let validity_json: serde_json::Value = row.try_get("validity_periods")?;
            let message = IncidentMessage {
                incident_id: row.try_get("incident_id")?,
                summary: row.try_get("summary")?,
                description: row.try_get("description")?,
                operators: row.try_get("operators")?,
                affected_stations: row.try_get("affected_stations")?,
                priority: row.try_get("priority")?,
                validity: serde_json::from_value(validity_json)?,
                is_planned: row.try_get("is_planned")?,
                is_cleared: row.try_get("is_cleared")?,
            };
            Ok(LoadedIncident { message, first_seen_at: row.try_get("first_seen_at")? })
        })
        .collect()
}
```

- [ ] **Step 4: Run the ignored test against a live database to verify it passes**

`docker-compose.yml`'s `postgres` service doesn't publish its port to the
host (only the `api`/`aggregator` containers reach it, over the compose
network), so running `cargo test` from the host needs a temporary port
mapping. Create a local, uncommitted override rather than editing
`docker-compose.yml` itself:

```bash
cat > docker-compose.override.yml <<'EOF'
services:
  postgres:
    ports:
      - "5432:5432"
EOF
docker compose -p nr-status-v2 --profile dev up -d postgres
```

Then, with `POSTGRES_USER`/`POSTGRES_PASSWORD`/`POSTGRES_DB` from your local
`.env`:

```bash
source .env
DATABASE_URL="postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@localhost:5432/${POSTGRES_DB}" \
  cargo test -p aggregator load_incidents_excludes_cleared_rows -- --ignored --nocapture
```

Expected: PASS.

Clean up afterward so this doesn't linger as an unintended local change:

```bash
rm docker-compose.override.yml
docker compose -p nr-status-v2 --profile dev up -d --force-recreate postgres
```

- [ ] **Step 5: Run the full aggregator crate test suite (non-ignored tests)**

Run: `cargo test -p aggregator`
Expected: **FAIL at this point** — `crates/aggregator/src/main.rs:68` and `crates/aggregator/src/aggregation.rs` still expect `load_incidents` to return `Vec<IncidentMessage>`. This is expected; Task 5 fixes the remaining call sites and must land in the same commit as this task's change to keep the crate compiling, exactly like Task 1+2 of the last-updated-indicators feature did for a similar signature-threading change. Proceed directly to Task 5 without committing yet.

---

### Task 5: Wire `is_active`/`LoadedIncident` into `aggregate()`

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs`
- Test: `crates/aggregator/src/aggregation.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `LoadedIncident` (Task 4), `is_active` (Task 3).
- Produces: `aggregate`'s signature changes from `fn aggregate(lines: &HashMap<String, LineDefinition>, incidents: &[IncidentMessage], samples: &HashMap<String, StationSample>, registry: &SegmentRegistry, defaults: &Defaults) -> HashMap<String, LineStatusReport>` to the same signature with `incidents: &[LoadedIncident]`. `main.rs`'s call site (`aggregation::aggregate(&lines, &incidents, &samples, &registry, defaults)`) needs **no source change** — `incidents` there is already whatever `load_incidents` returns, and its type just changed upstream in Task 4.

- [ ] **Step 1: Update the `use` import and `aggregate`'s signature/body**

In `crates/aggregator/src/aggregation.rs`, add to the top-of-file imports:

```rust
use crate::queries::LoadedIncident;
```

Change `aggregate`'s signature and the incident loop (currently lines 45-74):

```rust
pub fn aggregate(
    lines: &HashMap<String, LineDefinition>,
    incidents: &[LoadedIncident],
    samples: &HashMap<String, StationSample>,
    registry: &SegmentRegistry,
    defaults: &Defaults,
) -> HashMap<String, LineStatusReport> {
    let mut reports: HashMap<String, LineStatusReport> = lines
        .values()
        .map(|line| {
            (
                line.id.clone(),
                LineStatusReport {
                    id: line.id.clone(),
                    name: line.name.clone(),
                    mode_name: line.mode.clone(),
                    operators: line.operators.clone(),
                    statuses: vec![],
                },
            )
        })
        .collect();

    // Layer 1: incidents. Filtered through `is_active` first -- a cleared,
    // temporally-expired, or stale-past-the-rail-day-cutoff incident never
    // reaches the matcher, so its line falls through to Layer 2 exactly as
    // if the incident didn't exist.
    let now = Utc::now();
    for loaded in incidents.iter().filter(|loaded| is_active(&loaded.message, loaded.first_seen_at, now)) {
        for m in lines_affected_by(&loaded.message, lines, registry) {
            let status = status_from_incident(&m, &loaded.message);
            reports.get_mut(&m.line.id).unwrap().statuses.push(status);
        }
    }
```

(The rest of `aggregate` -- Layer 2, sample-derived stats -- is unchanged.)

- [ ] **Step 2: Update the test helper and the one direct call site in the test module**

In `crates/aggregator/src/aggregation.rs`'s `mod tests` block, change `aggregate_with_defaults` (currently lines 431-438) to wrap plain `IncidentMessage`s into fresh `LoadedIncident`s, so every existing test that calls it (`aggregator_propagates_severity_through_shared_trunk`, `aggregator_isolates_exclusive_incident`, `operator_only_match_is_demoted_to_minor`, `no_incident_no_samples_yields_good_service`) needs **no changes of its own**:

```rust
    fn aggregate_with_defaults(
        lines: &HashMap<String, LineDefinition>,
        incidents: &[IncidentMessage],
    ) -> HashMap<String, LineStatusReport> {
        let registry = SegmentRegistry::new(lines);
        let defaults = Defaults::default();
        let loaded: Vec<LoadedIncident> = incidents
            .iter()
            .cloned()
            .map(|message| LoadedIncident { message, first_seen_at: Utc::now() })
            .collect();
        aggregate(lines, &loaded, &HashMap::new(), &registry, &defaults)
    }
```

Then find the one test that calls `aggregate(...)` directly rather than through the helper (the sample-stats-blending test, currently around line 882: `let reports = aggregate(&lines, &[inc], &samples, &registry, &defaults);`) and wrap its incident the same way:

```rust
        let loaded = LoadedIncident { message: inc, first_seen_at: Utc::now() };
        let reports = aggregate(&lines, &[loaded], &samples, &registry, &defaults);
```

- [ ] **Step 3: Run the full aggregator crate test suite to confirm existing tests still pass**

Run: `cargo test -p aggregator`
Expected: PASS — every existing test passes unchanged, since `aggregate_with_defaults` now internally wraps with a fresh `Utc::now()` `first_seen_at`, which is always within the current rail day (`is_active` returns `true`) and has empty `validity` (also `is_active`-true).

- [ ] **Step 4: Write tests for the staleness/exemption behavior**

`aggregate`'s signature change in Step 1 is atomic across the whole file
(the same reason Task 4 Step 5 couldn't be split into its own red/green
cycle) — the filtering it enables is already live by the time this step
runs, so there's no red phase available for these two tests specifically;
they exist to pin down the exact behavior Steps 1-3 already implemented,
not to drive new implementation. Add to the same `mod tests` block:

```rust
    #[test]
    fn stale_non_planned_incident_falls_back_to_good_service() {
        let lines = load_all_lines();
        let inc = incident(
            "SWR-STALE",
            "Signal failure at Woking",
            "Residual delays continue.",
            &["SW"],
            &["WOK"],
        );
        let registry = SegmentRegistry::new(&lines);
        let defaults = Defaults::default();
        let loaded = LoadedIncident { message: inc, first_seen_at: Utc::now() - Duration::days(5) };
        let reports = aggregate(&lines, &[loaded], &HashMap::new(), &registry, &defaults);
        for line_id in ["swr-south-west-main", "swr-portsmouth-direct", "swr-alton"] {
            assert_eq!(
                reports[line_id].worst_severity(),
                Severity::GoodService,
                "{line_id} should fall back to Good Service once the incident is stale"
            );
        }
    }

    #[test]
    fn planned_work_is_exempt_from_the_rail_day_cutoff() {
        let lines = load_all_lines();
        let mut inc = incident(
            "SWR-PLANNED",
            "Engineering work at Woking",
            "Planned engineering work.",
            &["SW"],
            &["WOK"],
        );
        inc.is_planned = true;
        let registry = SegmentRegistry::new(&lines);
        let defaults = Defaults::default();
        let loaded = LoadedIncident { message: inc, first_seen_at: Utc::now() - Duration::days(5) };
        let reports = aggregate(&lines, &[loaded], &HashMap::new(), &registry, &defaults);
        assert_eq!(reports["swr-alton"].worst_severity(), Severity::PlannedClosure);
    }
```

- [ ] **Step 5: Run the full aggregator crate test suite**

Run: `cargo test -p aggregator`
Expected: PASS — all tests in the crate, including the two new ones, pass. A failure on either new test means Step 1's wiring has a bug (most likely: `is_active` not actually being applied as a `.filter()` before the matching loop) — go back and fix Step 1 rather than adjusting the test.

- [ ] **Step 6: Commit**

```bash
git add crates/aggregator/src/aggregation.rs crates/aggregator/src/queries.rs
git commit -m "Filter stale/cleared/expired incidents out of aggregation"
```

---

### Task 6: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions anywhere in the workspace. (The new `load_incidents_excludes_cleared_rows` test is `#[ignore]`d, so it's skipped here by default -- that's expected.)

- [ ] **Step 2: Run the ignored DB integration test against a live database**

Bring up the dev stack and rebuild the two touched services so they pick up this branch's code (following the same pattern used to verify the last-updated-indicators feature):

```bash
docker compose -p nr-status-v2 --profile dev build api-dev aggregator-dev
docker compose -p nr-status-v2 --env-file .env --profile dev up -d --force-recreate postgres api-dev aggregator-dev
```

Then run the ignored test against that database, using the same temporary
port-mapping override described in Task 4 Step 4 (`docker-compose.yml`'s
`postgres` service has no published port by default):

```bash
cat > docker-compose.override.yml <<'EOF'
services:
  postgres:
    ports:
      - "5432:5432"
EOF
docker compose -p nr-status-v2 --profile dev up -d postgres

source .env
DATABASE_URL="postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@localhost:5432/${POSTGRES_DB}" \
  cargo test -p aggregator load_incidents_excludes_cleared_rows -- --ignored --nocapture
```

Expected: PASS. `docker exec` (used in Step 3 below) reaches the container directly and doesn't need the port published, so remove the override once this step's test passes:

```bash
rm docker-compose.override.yml
docker compose -p nr-status-v2 --profile dev up -d --force-recreate postgres
```

- [ ] **Step 3: Manually verify end-to-end staleness behavior**

With the dev stack from Step 2 still running:

```bash
source .env
curl -s -X POST http://localhost:8080/private/incidents \
  -H "x-internal-token: $INTERNAL_TOKEN" -H "Content-Type: application/json" \
  -d '[{"incident_id":"MANUAL-STALE-TEST","summary":"Signal failure at Woking","description":"Test incident","operators":["SW"],"affected_stations":["WOK"],"priority":0,"validity":[],"is_planned":false,"is_cleared":false}]'
```

Wait one aggregator poll cycle (check `AGGREGATOR_POLL_INTERVAL_SECS`/the aggregator's configured interval in `.env`, or just watch `docker logs nr-status-v2-aggregator-dev-1 --follow` for the next "aggregation cycle complete" line), then confirm the incident is currently showing:

```bash
curl -s http://localhost:8080/Line/swr-alton/Status | grep -o '"reason":"[^"]*"'
```

Expected: shows `"Signal failure at Woking"` (or similar) as the reason, not Good Service.

Now backdate `first_seen_at` directly to simulate staleness (this is the one piece an end-to-end test can't easily wait for in real time -- the whole point of the feature is a boundary that's typically many hours away):

```bash
docker exec nr-status-v2-postgres-1 psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "UPDATE incidents SET first_seen_at = NOW() - INTERVAL '5 days' WHERE incident_id = 'MANUAL-STALE-TEST';"
```

Wait one more aggregator poll cycle, then re-check:

```bash
curl -s http://localhost:8080/Line/swr-alton/Status | grep -o '"reason":"[^"]*"'
```

Expected: shows `"Good Service"` -- the incident no longer contributes to the line's status.

Clean up the test fixture:

```bash
docker exec nr-status-v2-postgres-1 psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "DELETE FROM incidents WHERE incident_id = 'MANUAL-STALE-TEST';"
```

- [ ] **Step 4: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree (everything committed task-by-task above).
