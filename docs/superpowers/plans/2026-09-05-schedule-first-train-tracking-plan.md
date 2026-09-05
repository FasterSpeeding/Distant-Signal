# Schedule-First Resolution for Tracked-Train Pins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a tracked-train pin learn its `train_uid` (and a real,
displayable schedule) from CIF schedule data at pin-creation time (and on a
periodic retry), instead of relying solely on a single ±20-minute live
TRUST Movement window that a late-created pin can permanently miss.

**Architecture:** A new, pure `schedule_query::match_pin` function resolves
a pin's `(origin CRS, scheduled departure)` against a candidate line's
already-published `schedule_line_population` JSONB. `api` gains a new
`schedule_query` dependency (a deliberate boundary crossing, Decision 3 of
the spec) to run this match both synchronously at pin-creation time and on
a new periodic background sweep, writing `train_uid` + a schedule
snapshot and moving `resolution_status` to a new intermediate
`'schedule_matched'` value. `upsert_train_event`'s two-field guard is
relaxed so a later live TRUST Movement can still promote a schedule-matched
pin all the way to `'resolved'` without a second, independently-supplied
`resolved_train_uid`. `trust-consumer`'s own live-matching logic
(`resolve_origin_departure`) is untouched; only which rows it considers
eligible changes.

**Tech Stack:** Rust (`sqlx` runtime-checked queries, `axum`, `tokio`),
Postgres (a new migration), Next.js/TypeScript frontend (Mantine).

**Spec:** `docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md`
(read in full; every Decision/Open Question below refers to that document).

## Open Questions resolved by this plan

1. **Should `schedule_query` gain a `match_pin` helper, rather than `api`
   hand-rolling the comparison?** Yes -- Task 2. It stays a pure function
   (no `chrono-tz`, no I/O) by taking a caller-supplied `to_utc` closure
   for the CIF-local-to-UTC conversion, so `api` still owns and reuses its
   existing DST-aware `eta_blend::london_to_utc` (Decision 3 step 3's own
   instruction) without `schedule_query` growing a timezone dependency of
   its own.
2. **Test-suite audit before relaxing `upsert_train_event`'s guard**: audited
   in full (Task 9). Finding: **zero existing tests anywhere in the
   repository exercise `upsert_train_event`'s two-field guard at all** --
   no unit test, no `#[ignore]`d db test in `train_tracking.rs`'s own test
   modules, no integration test (`crates/api` has no `tests/` directory).
   The only tests referencing `resolved_train_uid`/`resolved_train_id` live
   in `crates/trust-consumer/src/process.rs`'s test module, and they assert
   what `run_once` *produces* on the outgoing `TrainMovementEventMessage`
   (a `trust-consumer`-side concern) -- never what `api::upsert_train_event`
   *does* with it. So there is nothing to break, but this is itself a real,
   pre-existing gap: Task 9 adds the first direct tests for this function.
3. **Multiple candidate lines per CRS**: trust the spec's reasoning (STP
   resolution is `(uid, date)`-keyed, not line-keyed, so two lines'
   populations must agree for the same UID) rather than add a
   cross-line reconciliation pass. Task 6's matching loop iterates
   candidate lines in a fixed order and returns the **first** line whose
   own population yields any match at all -- it does not fetch every
   candidate line's population and reconcile. This is cheaper (fewer
   JSONB fetches/deserializes per pin) and, per this reasoning, never
   produces a different real-world answer. No defensive assertion is
   added for disagreement between lines; if this reasoning is ever proven
   wrong in production it will surface as a plainly wrong schedule caption
   on a real pin, which the existing caveat copy (Task 11) already hedges
   against, and a live TRUST Movement (Task 10) still supersedes it either
   way.
4. **Ambiguity within the ±20-minute window**: **nearest-time wins**,
   confirmed as this plan's tie-break. Exact rule (Task 2): among every
   calling point (across every entry in the population passed to
   `match_pin`) whose TIPLOC matches and whose booked departure resolves
   to within tolerance, the one with the smallest `|scheduled - candidate|`
   wins; an exact tie is broken by whichever entry is encountered first in
   the `population` slice's own order (a `<=`-guarded running-best scan
   never replaces on an equal delta). This mirrors
   `resolve_origin_departure`'s own "first-match/earliest wins" posture for
   its inverted ambiguity case. Documented, deterministic limitation: this
   tie-break is stable for one call, but not guaranteed stable across a
   `schedule-reference` republish that reorders the underlying JSONB array
   -- acceptable because an exact-delta tie between two real, distinct
   services is a rare edge case already softened by the caveat copy and by
   TRUST's live Movement eventually superseding whichever guess was made.
5. **Exact new-column shape (snapshot vs. re-derive)**: a **hybrid**, not a
   blanket answer either way -- and it follows this table's own existing
   split precedent rather than picking one pattern uniformly:
   - **Snapshot** the expensive-to-recompute part: `schedule_calling_points`
     (the matched entry's full calling-point list, pulled out of a
     whole-line, whole-day JSONB blob that would otherwise need a live
     JOIN + deserialize on every `GET`) and `matched_line_id` (audit-only).
     This mirrors `train_current_state`/`train_movement_events` -- data
     computed by an expensive, external, wholesale-replaced-per-cycle
     process (`schedule_line_population` itself is upserted wholesale, per
     `upsert_schedule_line_population`'s own doc, "never merged") gets
     written once and read back verbatim, never re-derived live.
   - **Re-derive the display name** the cheap way this table already does
     for `pin_origin_name`/`pin_destination_name`: store only
     `schedule_destination_crs` (a stable code, exactly like
     `pin_destination_crs`) and resolve `schedule_destination_name` via one
     more `LEFT JOIN stations` in `TRACKED_TRAIN_STATE_SELECT`, at read
     time -- a single indexed one-row lookup, not a JSONB scan, so there is
     no cost story here that argues for a stored name column, and it keeps
     the destination station's *display* name always current if it's ever
     corrected in `stations`, matching the existing precedent's own reason
     for being read-time in the first place.
6. **Caveat copy**: finalized in Task 11 -- both the `TrainJourney.tsx`
   `schedule_matched` branch body text and a `Tooltip` (mirroring
   `EtaBadge.tsx`'s existing provenance-tooltip pattern) carrying the
   "subject to late alterations" caveat.

## Global Constraints

- `resolved`'s existing semantic (**both** `train_uid` **and** `train_id`
  bound) must not change. This plan adds an earlier waypoint
  (`schedule_matched`); it does not redefine `resolved`.
- `train_id` remains exclusively TRUST-sourced. No code path introduced by
  this plan ever writes `train_id` from schedule data.
- `resolve_origin_departure` (`crates/trust-consumer/src/matching.rs`)
  itself is not modified. Only which rows are eligible to reach it changes
  (`apply_reference_reload`'s match arm, Task 10).
- `MATCH_TOLERANCE`'s value (20 minutes) does not change; it is hoisted
  into `common` (Task 3) so both matching paths read one definition, never
  duplicated with a different value.
- `MAX_PIN_AGE` and every other existing constant's value are unchanged.
- No full national schedule database, no `schedule_network_departures`
  reuse, no `Activation.train_uid`-exact-match matching path -- all three
  are explicit Non-goals in the spec and stay out of scope here.
- Every SQL write in this plan uses runtime-checked `sqlx::query`/
  `query_as` (never the `query!` macro family), matching this crate's own
  stated convention (`crates/api/src/data/queries.rs`'s module doc).
- Every user-facing copy string introduced by this plan contains no
  snake_case identifiers (matches `validate_pin`'s own established
  guard/tests).

---

## Task 1: Migration -- `'schedule_matched'` status + schema-metadata columns

**Files:**
- Create: `crates/api/migrations/20260905150000_schedule_matched_resolution.sql`

**Interfaces:**
- Produces: the `'schedule_matched'` value now accepted by
  `tracked_trains.resolution_status`'s CHECK constraint; four new nullable
  columns: `matched_line_id TEXT`, `schedule_calling_points JSONB`,
  `schedule_destination_crs TEXT`, `schedule_matched_at TIMESTAMPTZ`.

- [ ] **Step 1: Write the migration**

```sql
-- ---------------------------------------------------------------------
-- Schedule-first resolution for tracked-train pins (Decisions 3-5 of
-- docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md):
-- a new 'schedule_matched' resolution_status waypoint between 'pending'
-- and 'resolved', plus nullable columns for the schedule-match snapshot
-- this plan's Open Question 5 resolves in favor of a hybrid
-- snapshot-the-expensive-part / re-derive-the-cheap-name-part shape (see
-- the plan's own writeup -- schedule_destination_crs is resolved to a
-- display name via a LEFT JOIN at read time, the same way
-- pin_destination_crs already is; schedule_calling_points is NOT
-- re-derived at read time, since that would mean a live JOIN into a
-- whole line's whole-day schedule_line_population JSONB on every GET).
-- ---------------------------------------------------------------------

-- `IF EXISTS`/re-add rather than a bare rename: this is the standard,
-- unnamed-inline-CHECK-constraint auto-generated name Postgres assigns
-- (`{table}_{column}_check`), confirmed against
-- crates/api/migrations/20260828120000_train_tracking.sql's original,
-- un-named `CHECK (resolution_status IN (...))` clause -- `IF EXISTS`
-- is defensive only, in case that ever proves wrong on a real database
-- (see this task's own verification step).
ALTER TABLE tracked_trains DROP CONSTRAINT IF EXISTS tracked_trains_resolution_status_check;
ALTER TABLE tracked_trains ADD CONSTRAINT tracked_trains_resolution_status_check
    CHECK (resolution_status IN ('pending', 'schedule_matched', 'resolved', 'unresolved'));

-- Which candidate line's schedule_line_population produced the match --
-- an audit/debugging column only. Nothing re-queries
-- schedule_line_population using it at read time.
ALTER TABLE tracked_trains ADD COLUMN matched_line_id TEXT;

-- Snapshot of the matched entry's calling_points at match time, already
-- camelCase-shaped on write (see schedule_matching::ScheduleCallingPointDto,
-- Task 6) so this crate can relay it to the frontend as opaque JSON with
-- no read-time conversion.
ALTER TABLE tracked_trains ADD COLUMN schedule_calling_points JSONB;

-- The matched schedule's own terminus CRS, resolved once at match time --
-- same "store the stable code, derive the display name via a read-time
-- LEFT JOIN stations" pattern pin_origin_crs/pin_destination_crs already
-- use (TRACKED_TRAIN_STATE_SELECT).
ALTER TABLE tracked_trains ADD COLUMN schedule_destination_crs TEXT;

-- Parallels resolved_at, but independent of it: this is set the moment a
-- schedule match happens (Decision 3 step 4), NOT when TRUST later
-- confirms the pin live -- resolved_at keeps meaning exactly what it
-- means today.
ALTER TABLE tracked_trains ADD COLUMN schedule_matched_at TIMESTAMPTZ;
```

- [ ] **Step 2: Run the migration against a local database and verify the constraint**

```bash
cd crates/api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres sqlx migrate run
psql "$DATABASE_URL" -c "\d tracked_trains" | grep -A1 resolution_status_check
```

Expected: the printed constraint definition reads
`CHECK (resolution_status = ANY (ARRAY['pending'::text, 'schedule_matched'::text, 'resolved'::text, 'unresolved'::text]))`
(Postgres's own canonical rendering of an `IN (...)` CHECK). If the
`DROP CONSTRAINT IF EXISTS` silently matched nothing (i.e. the original
constraint had a different auto-generated name on this Postgres version),
this step will instead show the OLD three-value constraint still present
alongside a duplicate new one, or a constraint-name-conflict error on the
`ADD CONSTRAINT` -- if so, find the real name via
`SELECT conname FROM pg_constraint WHERE conrelid = 'tracked_trains'::regclass AND contype = 'c';`
and fix the `DROP CONSTRAINT` line to match before proceeding.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260905150000_schedule_matched_resolution.sql
git commit -m "Add schedule_matched resolution status and schedule-match snapshot columns"
```

---

## Task 2: `schedule_query::match_pin` -- the pure matching function

**Files:**
- Modify: `crates/schedule-query/src/resolve.rs`
- Modify: `crates/schedule-query/src/lib.rs` (export `match_pin`)

**Interfaces:**
- Produces:
  ```rust
  pub fn match_pin<'a>(
      population: &'a [LinePopulationEntry],
      crs_tiplocs: &[&str],
      scheduled: DateTime<Utc>,
      tolerance: chrono::Duration,
      to_utc: impl Fn(NaiveTime) -> Option<DateTime<Utc>>,
  ) -> Option<&'a LinePopulationEntry>
  ```
  Consumed by Task 6's `attempt_schedule_match`.

- [ ] **Step 1: Add imports and the function to `resolve.rs`**

Add to the top of `crates/schedule-query/src/resolve.rs` (existing imports
are `use chrono::{Datelike, NaiveDate, NaiveTime};` and
`use crate::records::{CallingPoint, RawSchedule, StpIndicator};`):

```rust
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};

use crate::records::{CallingPoint, LinePopulationEntry, RawSchedule, StpIndicator};
```

Add the function itself, directly below `schedules_touching`:

```rust
/// Finds the best schedule match for a tracked-train pin (Decision 3 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md):
/// among every `population` entry with a calling point whose TIPLOC
/// (compared via [`normalize_tiploc`]) is one of `crs_tiplocs` and whose
/// `booked_departure` resolves -- via caller-supplied `to_utc`, so this
/// pure crate never grows a `chrono-tz` dependency of its own; the real
/// caller passes a closure wrapping `crates/api/src/data/eta_blend.rs`'s
/// existing DST-aware `london_to_utc` -- to within `tolerance` of
/// `scheduled`, returns the entry whose matching calling point is
/// CLOSEST in time to `scheduled`.
///
/// Tie-break (the plan's Open Question 4): on an exact equal delta
/// between two candidates, the one encountered FIRST in `population`'s
/// own order wins -- the scan below only replaces `best` on a strictly
/// smaller delta, never an equal one. This is deterministic for one call
/// but not guaranteed stable across a `schedule-reference` republish that
/// reorders the underlying JSONB array; accepted as a rare-edge-case
/// limitation, not fixed here (see the plan's own writeup).
///
/// `None` if nothing in `population` has any calling point at any of
/// `crs_tiplocs` within `tolerance` of `scheduled`.
pub fn match_pin<'a>(
    population: &'a [LinePopulationEntry],
    crs_tiplocs: &[&str],
    scheduled: DateTime<Utc>,
    tolerance: Duration,
    to_utc: impl Fn(NaiveTime) -> Option<DateTime<Utc>>,
) -> Option<&'a LinePopulationEntry> {
    let normalized_targets: Vec<&str> = crs_tiplocs.iter().map(|t| normalize_tiploc(t)).collect();

    let mut best: Option<(&'a LinePopulationEntry, Duration)> = None;
    for entry in population {
        for cp in &entry.calling_points {
            if !normalized_targets.contains(&normalize_tiploc(&cp.tiploc)) {
                continue;
            }
            let Some(booked) = cp.booked_departure else {
                continue;
            };
            let Some(candidate_utc) = to_utc(booked) else {
                continue;
            };
            let delta = (scheduled - candidate_utc).abs();
            if delta > tolerance {
                continue;
            }
            match &best {
                Some((_, best_delta)) if *best_delta <= delta => {}
                _ => best = Some((entry, delta)),
            }
        }
    }
    best.map(|(entry, _)| entry)
}
```

- [ ] **Step 2: Export it from `lib.rs`**

```rust
pub use resolve::{
    ResolvedSchedule, ScheduleIndex, departures_by_crs, match_pin, resolve_for_date,
    schedules_touching,
};
```

- [ ] **Step 3: Write the tests**

Add to `resolve.rs`'s existing `#[cfg(test)] mod tests`, reusing its
existing `calling_point_with_departure`/`tiploc_map`-style helpers:

```rust
fn population_entry(uid: &str, calling_points: Vec<CallingPoint>) -> LinePopulationEntry {
    LinePopulationEntry {
        uid: uid.to_string(),
        calling_points,
    }
}

// Identity closure: every test below constructs `booked_departure` values
// already meant to be read as UTC instants directly, so `to_utc` just
// pairs a bare NaiveTime with a fixed date -- exercising `match_pin`'s
// arithmetic without pulling in a real Europe/London conversion (that's
// `eta_blend::london_to_utc`'s own, separately-tested job).
fn utc_on(date: &str) -> impl Fn(NaiveTime) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    move |t| Some(DateTime::<Utc>::from_naive_utc_and_offset(date.and_time(t), Utc))
}

#[test]
fn match_pin_matches_a_departure_within_tolerance() {
    let population = vec![population_entry(
        "C11052",
        vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:15")],
    )];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap();
    let matched = match_pin(
        &population,
        &["EUSTON"],
        scheduled,
        Duration::minutes(20),
        utc_on("2026-09-05"),
    );
    assert_eq!(matched.map(|e| e.uid.as_str()), Some("C11052"));
}

#[test]
fn match_pin_rejects_a_departure_outside_tolerance() {
    let population = vec![population_entry(
        "C11052",
        vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:15")],
    )];
    let scheduled: DateTime<Utc> = "2026-09-05T20:00:00Z".parse().unwrap(); // 45m away
    assert_eq!(
        match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), utc_on("2026-09-05")),
        None
    );
}

#[test]
fn match_pin_rejects_a_tiploc_not_in_crs_tiplocs() {
    let population = vec![population_entry(
        "C11052",
        vec![calling_point_with_departure("CREWE  ", CallingPointKind::Origin, "19:15")],
    )];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap();
    assert_eq!(
        match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), utc_on("2026-09-05")),
        None
    );
}

#[test]
fn match_pin_ignores_a_calling_point_with_no_booked_departure() {
    let population = vec![population_entry(
        "C11052",
        vec![calling_point("EUSTON ", CallingPointKind::Terminate)], // no booked_departure
    )];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap();
    assert_eq!(
        match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), utc_on("2026-09-05")),
        None
    );
}

#[test]
fn match_pin_nearest_time_wins_between_two_in_tolerance_candidates() {
    let population = vec![
        population_entry("FAR", vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:05")]), // 10m away
        population_entry("NEAR", vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:12")]), // 3m away
    ];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap();
    let matched = match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), utc_on("2026-09-05"));
    assert_eq!(matched.map(|e| e.uid.as_str()), Some("NEAR"));
}

#[test]
fn match_pin_on_an_exact_tie_the_first_in_population_order_wins() {
    let population = vec![
        population_entry("FIRST", vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:10")]),
        population_entry("SECOND", vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:20")]),
    ];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap(); // exactly 5m from both
    let matched = match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), utc_on("2026-09-05"));
    assert_eq!(matched.map(|e| e.uid.as_str()), Some("FIRST"));
}

#[test]
fn match_pin_skips_a_candidate_whose_to_utc_conversion_fails() {
    // Simulates a nonexistent-local-time DST edge case: to_utc returns
    // None for every candidate, so nothing can match even though the
    // TIPLOC/tolerance checks would otherwise pass.
    let population = vec![population_entry(
        "C11052",
        vec![calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "19:15")],
    )];
    let scheduled: DateTime<Utc> = "2026-09-05T19:15:00Z".parse().unwrap();
    let matched = match_pin(&population, &["EUSTON"], scheduled, Duration::minutes(20), |_| None);
    assert_eq!(matched, None);
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p schedule-query match_pin
```

Expected: all 7 new tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/schedule-query/src/resolve.rs crates/schedule-query/src/lib.rs
git commit -m "Add schedule_query::match_pin, the pin-vs-schedule matching primitive"
```

---

## Task 3: Hoist `MATCH_TOLERANCE` into `common`

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/trust-consumer/src/matching.rs`

**Interfaces:**
- Produces: `pub const common::MATCH_TOLERANCE: chrono::Duration`, consumed
  by both `trust-consumer::matching::resolve_origin_departure` (updated
  here) and `api::data::schedule_matching::attempt_schedule_match` (Task 6).

- [ ] **Step 1: Add the constant to `common`**

Add near the top of `crates/common/src/lib.rs` (alongside the other
top-level constants, e.g. `CUSTOM_NAME_MAX_LENGTH`):

```rust
/// How far apart a scheduled departure and a candidate departure (a live
/// TRUST Movement, or a CIF-booked calling point) can be and still be
/// considered the same real-world service. Shared by
/// `trust_consumer::matching::resolve_origin_departure` (live TRUST
/// Movement matching) and `api`'s `schedule_matching::attempt_schedule_match`
/// (CIF schedule matching) -- hoisted here, rather than duplicated with a
/// cross-reference comment, per Decision 3 step 3 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md,
/// so the two matching paths can never silently drift onto different
/// tolerance values. The VALUE (20 minutes) is unchanged from
/// `trust-consumer`'s original constant -- this is a hoist, not a
/// behavior change.
pub const MATCH_TOLERANCE: chrono::Duration = chrono::Duration::minutes(20);
```

- [ ] **Step 2: Remove the local constant from `trust-consumer` and use `common`'s**

In `crates/trust-consumer/src/matching.rs`, delete:

```rust
const MATCH_TOLERANCE: chrono::Duration = chrono::Duration::minutes(20);
```

and change the one use site in `resolve_origin_departure` from
`MATCH_TOLERANCE` to `common::MATCH_TOLERANCE`:

```rust
pub fn resolve_origin_departure(
    loc_crs: &str,
    actual_timestamp: DateTime<Utc>,
    pending: &[PendingPin],
) -> Option<i64> {
    pending
        .iter()
        .find(|pin| {
            pin.pin_origin_crs.eq_ignore_ascii_case(loc_crs)
                && (pin.pin_scheduled_departure - actual_timestamp).abs() <= common::MATCH_TOLERANCE
        })
        .map(|pin| pin.tracked_train_id)
}
```

Update the module doc comment's own reference to "`MATCH_TOLERANCE`,
`matching.rs:23`" is unaffected (this task doesn't touch doc comments in
other files), but update this file's own top-of-module doc line that
names `MATCH_TOLERANCE` if it hardcodes the old local path -- check with:

```bash
grep -n "MATCH_TOLERANCE" crates/trust-consumer/src/matching.rs
```

and adjust any remaining reference to read `common::MATCH_TOLERANCE`.

- [ ] **Step 3: Run the existing tests unchanged**

```bash
cargo test -p trust-consumer matching::
```

Expected: all 5 existing `matching.rs` tests still pass unmodified (this
is a pure rename/relocation, no behavior change).

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/lib.rs crates/trust-consumer/src/matching.rs
git commit -m "Hoist MATCH_TOLERANCE into common so both matching paths share one definition"
```

---

## Task 4: `api` -- CRS -> line_id reverse index

**Files:**
- Create: `crates/api/src/data/schedule_matching.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/app.rs`
- Modify: `crates/api/Cargo.toml`

**Interfaces:**
- Produces: `pub fn crs_to_line_ids(lines: &[common::LineDefinition]) -> HashMap<String, Vec<String>>`
  and a new `AppState.schedule_crs_line_index: HashMap<String, Vec<String>>`
  field, computed once at startup from the ALREADY-loaded
  `app.config.lines` (`crates/api/src/data/config.rs`'s existing
  `ServiceArguments.lines: LineCatalogue`, confirmed already loaded for
  `full_coverage_enabled_for`'s own use -- no new config plumbing needed).

- [ ] **Step 1: Add `schedule-query` as an `api` dependency**

In `crates/api/Cargo.toml`, add (alphabetically, alongside `sha2`):

```toml
schedule-query = { path = "../schedule-query" }
```

- [ ] **Step 2: Create the module and the index function**

```rust
// crates/api/src/data/schedule_matching.rs
//! Schedule-first resolution of a tracked-train pin's `train_uid`, per
//! docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md.
//! Attempted once at pin-creation time (`routes::train::post_track`) and
//! again periodically for every still-`pending` row
//! (`run_schedule_match_sweep`, `main.rs`'s new background loop) -- both
//! paths funnel through `attempt_schedule_match`, the only place this
//! crate ever calls `schedule_query::match_pin`.

use std::collections::HashMap;

use common::LineDefinition;

/// `CRS -> Vec<line_id>` (Decision 2 of the design spec), built from the
/// static `lines/*.toml` catalogue -- mirrors
/// `crates/schedule-reference/src/main.rs`'s own `lines_to_publish`
/// predicate exactly (a line qualifies if it has at least one
/// `tiploc`-bearing station), then further filters to only the
/// TIPLOC-bearing stations themselves, since a station with no TIPLOC has
/// no way to ever appear in a CIF calling-point list anyway. Built once
/// at `AppState::init` from `app.config.lines` (already loaded there for
/// `full_coverage_enabled_for`'s own use -- this is a pure re-keying of
/// data already in memory, no new I/O).
pub fn crs_to_line_ids(lines: &[LineDefinition]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        if !line.stations.iter().any(|s| s.tiploc.is_some()) {
            continue;
        }
        for station in &line.stations {
            if station.tiploc.is_none() {
                continue;
            }
            let crs = station.crs.to_uppercase();
            let ids = index.entry(crs).or_default();
            if !ids.contains(&line.id) {
                ids.push(line.id.clone());
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, stations: Vec<(&str, Option<&str>)>) -> LineDefinition {
        LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "rail".to_string(),
            category: "national-rail".to_string(),
            operators: vec![],
            stations: stations
                .into_iter()
                .map(|(crs, tiploc)| common::Station {
                    crs: crs.to_string(),
                    tiploc: tiploc.map(str::to_string),
                    role: "minor".to_string(),
                    segment: None,
                })
                .collect(),
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    #[test]
    fn a_tiploc_bearing_station_maps_its_crs_to_its_line() {
        let lines = vec![line("wcml", vec![("EUS", Some("EUSTON"))])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("EUS"), Some(&vec!["wcml".to_string()]));
    }

    #[test]
    fn a_crs_with_no_tiploc_on_its_station_entry_is_not_indexed() {
        let lines = vec![line("wcml", vec![("EUS", Some("EUSTON")), ("ZZZ", None)])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("ZZZ"), None);
    }

    #[test]
    fn a_crs_shared_by_two_lines_maps_to_both() {
        let lines = vec![
            line("line-a", vec![("EUS", Some("EUSTON"))]),
            line("line-b", vec![("EUS", Some("EUSTON"))]),
        ];
        let index = crs_to_line_ids(&lines);
        let mut ids = index.get("EUS").cloned().unwrap_or_default();
        ids.sort();
        assert_eq!(ids, vec!["line-a".to_string(), "line-b".to_string()]);
    }

    #[test]
    fn a_line_with_no_tiploc_bearing_station_at_all_is_excluded_entirely() {
        let lines = vec![line("no-tiploc-line", vec![("ZZA", None), ("ZZB", None)])];
        let index = crs_to_line_ids(&lines);
        assert!(index.is_empty());
    }

    #[test]
    fn a_lowercase_crs_on_a_station_entry_is_indexed_uppercased() {
        let lines = vec![line("wcml", vec![("eus", Some("EUSTON"))])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("EUS"), Some(&vec!["wcml".to_string()]));
    }
}
```

- [ ] **Step 3: Register the module**

In `crates/api/src/data/mod.rs`, add (alphabetically):

```rust
pub mod schedule_matching;
```

- [ ] **Step 4: Wire the index into `AppState`**

In `crates/api/src/app.rs`, add a field to `AppState` (after
`internal_oauth_routes`):

```rust
    /// CRS -> candidate line_ids, built once here from `config.lines`
    /// (Decision 2 of
    /// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
    /// Consulted by `routes::train::post_track` and the periodic
    /// schedule-match sweep (`main.rs`) -- never mutated after startup,
    /// same "load once, refresh only on process restart" posture as
    /// `config.lines` itself already has.
    pub schedule_crs_line_index: std::collections::HashMap<String, Vec<String>>,
```

In `AppState::init`, immediately before the final `Ok(Arc::new(Self { ... }))`:

```rust
        let schedule_crs_line_index = crate::data::schedule_matching::crs_to_line_ids(&config.lines);

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
            internal_oauth_verifier,
            internal_oauth_routes,
            schedule_crs_line_index,
        }))
```

(`config.lines` derefs to `&[LineDefinition]` via `LineCatalogue`'s
existing `Deref` impl, and this line must come before `config` is moved
into the struct literal.)

- [ ] **Step 5: Update every existing `test_app` fixture in `api`'s route tests**

`schedule_crs_line_index` is a required field of `AppState`, so every
existing hand-built `AppState { ... }` test fixture in
`crates/api/src/routes/{chatbot,departures,stanox_crs,lines}.rs` (and any
other file constructing `AppState` directly for tests, not via
`AppState::init`) needs this field added. Search first:

```bash
grep -rln "AppState {" crates/api/src/routes/*.rs
```

For each match, add `schedule_crs_line_index: std::collections::HashMap::new(),`
to the struct literal (an empty index is the correct fixture default for
every one of these tests -- none of them exercise schedule matching).

- [ ] **Step 6: Build and run the affected tests**

```bash
cargo build -p api
cargo test -p api schedule_matching::
cargo test -p api --lib
```

Expected: `crs_to_line_ids`'s 5 new tests pass; the whole `api` crate
still compiles and its existing non-DB-gated test suite still passes.

- [ ] **Step 7: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/data/schedule_matching.rs crates/api/src/data/mod.rs crates/api/src/app.rs crates/api/src/routes/*.rs
git commit -m "Add the CRS-to-line_id reverse index for schedule-first pin resolution"
```

---

## Task 5: `api` -- stanox_crs lookups + `london_to_utc` visibility

**Files:**
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/data/eta_blend.rs`

**Interfaces:**
- Produces: `pub async fn list_stanox_crs_for_crs(pool, crs) -> Result<Vec<common::StanoxCrsRecord>>`,
  `pub async fn crs_for_tiploc(pool, tiploc) -> Result<Option<String>>`,
  and `pub(crate) fn london_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>>`
  (was private) -- all consumed by Task 6.

- [ ] **Step 1: Add the two stanox_crs queries**

In `crates/api/src/data/queries.rs`, directly below the existing
`list_stanox_crs` function (reuses its private `StanoxCrsRow` struct and
`From` impl unchanged):

```rust
/// Every `stanox_crs` row for one CRS -- the "which TIPLOCs does this
/// station's code cover" lookup Decision 3 step 3 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
/// calls for (`list_stanox_crs`'s existing `WHERE`-less shape returns
/// everything; this is its `WHERE crs = $1` sibling). `UPPER(...)` on both
/// sides, matching `TRACKED_TRAIN_STATE_SELECT`'s own established
/// convention -- `tracked_trains.pin_origin_crs` is never
/// case-normalized at write time (`validate_pin` doesn't uppercase it),
/// so a case-insensitive compare here is load-bearing, not defensive
/// tidiness.
pub async fn list_stanox_crs_for_crs(pool: &PgPool, crs: &str) -> Result<Vec<common::StanoxCrsRecord>> {
    let rows = sqlx::query_as::<_, StanoxCrsRow>(
        "SELECT stanox, crs, tiploc, station_name, source_sequence FROM stanox_crs \
         WHERE UPPER(crs) = UPPER($1)",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(common::StanoxCrsRecord::from).collect())
}

/// Reverse of the above: one CRS for a TIPLOC, or `None` if unmapped.
/// Used to resolve a matched schedule's own terminus CRS
/// (`schedule_destination_crs`) from its last calling point's TIPLOC.
/// `LIMIT 1`: a TIPLOC maps to at most one real station in practice, but
/// this doesn't assume uniqueness at the SQL level (no `UNIQUE`
/// constraint on `stanox_crs.tiploc` -- multiple STANOX rows can share a
/// TIPLOC, e.g. different platforms/areas of one physical location), so
/// this is "a plausible one," not "the guaranteed only one."
pub async fn crs_for_tiploc(pool: &PgPool, tiploc: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT crs FROM stanox_crs WHERE UPPER(tiploc) = UPPER($1) LIMIT 1",
    )
    .bind(tiploc)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(crs,)| crs))
}
```

- [ ] **Step 2: Widen `london_to_utc`'s visibility**

In `crates/api/src/data/eta_blend.rs`, change:

```rust
fn london_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
```

to:

```rust
pub(crate) fn london_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
```

Its existing doc comment and test module are otherwise unchanged --
this is a visibility-only edit.

- [ ] **Step 3: Write tests for the two new queries**

Add to `crates/api/src/data/queries.rs`'s existing `db_tests`-equivalent
module (check whether `queries.rs` already has a `#[cfg(test)] mod
db_tests` -- if not, add one following `train_tracking.rs`'s own
`db_tests` module shape exactly, including its `connect()` helper):

```rust
#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            list_stanox_crs_for_crs -- --ignored`"]
async fn list_stanox_crs_for_crs_returns_only_matching_rows_case_insensitively() {
    let pool = connect().await;
    upsert_stanox_crs(
        &pool,
        &[
            common::StanoxCrsRecord {
                stanox: "TEST-EUS".to_string(),
                crs: "EUS".to_string(),
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
                source_sequence: 1,
            },
            common::StanoxCrsRecord {
                stanox: "TEST-WAT".to_string(),
                crs: "WAT".to_string(),
                tiploc: "WATRLMN".to_string(),
                station_name: "LONDON WATERLOO".to_string(),
                source_sequence: 1,
            },
        ],
    )
    .await
    .expect("seed stanox_crs");

    let rows = list_stanox_crs_for_crs(&pool, "eus").await.expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tiploc, "EUSTON");

    sqlx::query("DELETE FROM stanox_crs WHERE stanox IN ('TEST-EUS', 'TEST-WAT')")
        .execute(&pool)
        .await
        .expect("cleanup");
}

#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            crs_for_tiploc -- --ignored`"]
async fn crs_for_tiploc_resolves_a_known_tiploc_and_none_for_an_unknown_one() {
    let pool = connect().await;
    upsert_stanox_crs(
        &pool,
        &[common::StanoxCrsRecord {
            stanox: "TEST-CRE".to_string(),
            crs: "CRE".to_string(),
            tiploc: "CREWE".to_string(),
            station_name: "CREWE".to_string(),
            source_sequence: 1,
        }],
    )
    .await
    .expect("seed stanox_crs");

    assert_eq!(crs_for_tiploc(&pool, "crewe").await.unwrap(), Some("CRE".to_string()));
    assert_eq!(crs_for_tiploc(&pool, "NOWHERE").await.unwrap(), None);

    sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-CRE'")
        .execute(&pool)
        .await
        .expect("cleanup");
}
```

- [ ] **Step 4: Run**

```bash
cargo build -p api
cargo test -p api list_stanox_crs_for_crs -- --ignored
cargo test -p api crs_for_tiploc -- --ignored
```

Expected: both pass against a live local Postgres with migrations applied.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/data/eta_blend.rs
git commit -m "Add stanox_crs CRS/TIPLOC lookups and expose london_to_utc within the crate"
```

---

## Task 6: `api` -- the schedule-matching function itself

**Files:**
- Modify: `crates/api/src/data/schedule_matching.rs`
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Consumes: `schedule_query::match_pin` (Task 2), `common::MATCH_TOLERANCE`
  (Task 3), `crate::data::queries::{get_schedule_line_population,
  list_stanox_crs_for_crs, crs_for_tiploc}`, `crate::data::eta_blend::london_to_utc`
  (Task 5), `crate::data::schedule_matching::crs_to_line_ids` (Task 4).
- Produces:
  `pub async fn attempt_schedule_match(pool, tracked_train_id, pin_origin_crs, pin_scheduled_departure, service_date, crs_line_index) -> anyhow::Result<bool>`
  and `pub async fn train_tracking::apply_schedule_match(...) -> anyhow::Result<bool>`
  -- both consumed by Task 7 (pin-creation wiring) and Task 8 (periodic
  sweep).

**Important correctness note discovered during this plan's own research**:
`schedule_query::CallingPoint`/`CallingPointKind` carry **no**
`#[serde(rename_all = "camelCase")]` -- their JSON keys are snake_case
(`booked_arrival`, not `bookedArrival`). Storing `matched.calling_points`
verbatim as the new `schedule_calling_points` JSONB column would then
relay snake_case-keyed objects nested inside an otherwise fully-camelCase
`TrackedTrainState` response -- inconsistent with every other field on
that struct. Step 2 below converts to a small camelCase DTO **before**
storing, once, at match time, so the read path (Task 11) can relay the
stored JSONB as an opaque `serde_json::Value` with no read-time
conversion.

- [ ] **Step 1: Add `apply_schedule_match` to `train_tracking.rs`**

```rust
/// Writes a successful schedule match (Decision 3 step 4 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md):
/// sets `train_uid` (never `train_id` -- that stays exclusively
/// TRUST-sourced, per this plan's Global Constraints) and moves
/// `resolution_status` to the new `'schedule_matched'` waypoint. Guarded
/// by `WHERE train_uid IS NULL AND resolution_status = 'pending'` so this
/// is safe to call from BOTH the synchronous pin-creation path and the
/// periodic sweep without a race clobbering a row that has since moved on
/// (a live TRUST Movement resolved it first, or an earlier sweep tick
/// already matched it) -- `rows_affected() == 0` in either of those cases
/// is not an error, just a no-op, which is why this returns `bool` rather
/// than erroring on zero rows affected.
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

/// Row shape for `list_pending_pins_for_schedule_match`'s query -- every
/// still-`pending`, never-schedule-matched row, the periodic sweep's own
/// input set (Decision 3's "also run this same attempt periodically").
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingSchedulePin {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_scheduled_departure: DateTime<Utc>,
}

/// Every row the periodic schedule-match sweep should retry: still
/// `pending` AND still lacking a `train_uid` -- a `schedule_matched` row
/// already has one and is excluded, same as a `resolved`/`unresolved` row.
pub async fn list_pending_pins_for_schedule_match(
    pool: &PgPool,
) -> anyhow::Result<Vec<PendingSchedulePin>> {
    let rows = sqlx::query_as::<_, PendingSchedulePin>(
        "SELECT id, service_date, pin_origin_crs, pin_scheduled_departure \
         FROM tracked_trains WHERE train_uid IS NULL AND resolution_status = 'pending'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 2: Add the camelCase calling-point DTO and `attempt_schedule_match` to `schedule_matching.rs`**

```rust
use chrono::{DateTime, NaiveDate, Utc};
use schedule_query::LinePopulationEntry;
use serde::Serialize;
use sqlx::PgPool;

use crate::data::eta_blend::london_to_utc;
use crate::data::{queries, train_tracking};

/// camelCase wire shape for one calling point, converted from
/// `schedule_query::CallingPoint` (whose own JSON keys are snake_case --
/// see this task's own note) BEFORE storage, so `schedule_calling_points`
/// is stored already camelCase and the read path can relay it verbatim.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleCallingPointDto {
    tiploc: String,
    kind: schedule_query::CallingPointKind,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    is_half_minute_arrival: bool,
    is_half_minute_departure: bool,
}

impl From<&schedule_query::CallingPoint> for ScheduleCallingPointDto {
    fn from(cp: &schedule_query::CallingPoint) -> Self {
        Self {
            tiploc: cp.tiploc.clone(),
            kind: cp.kind,
            booked_arrival: cp.booked_arrival,
            booked_departure: cp.booked_departure,
            is_half_minute_arrival: cp.is_half_minute_arrival,
            is_half_minute_departure: cp.is_half_minute_departure,
        }
    }
}

/// One pin's schedule-match attempt (Decision 3 steps 1-5), called both
/// synchronously at creation (`routes::train::post_track`, Task 7) and
/// periodically for every still-`pending` row (`run_schedule_match_sweep`,
/// Task 8). Iterates `crs_line_index`'s candidate lines for
/// `pin_origin_crs` IN A FIXED ORDER and returns on the FIRST candidate
/// line whose own population yields any match at all (this plan's Open
/// Question 3 resolution: trusts that a second candidate line, if any,
/// would resolve the same UID/date identically, so there is nothing to
/// gain from fetching every candidate and reconciling).
///
/// Returns `Ok(true)` only if a match was found AND actually written
/// (i.e. the row was still eligible -- see `apply_schedule_match`'s own
/// guard). `Ok(false)` covers every other honest "still pending" outcome
/// uniformly: no candidate line, no `stanox_crs` rows for this CRS, no
/// `schedule_line_population` published yet for any candidate, or no
/// calling point within tolerance.
pub async fn attempt_schedule_match(
    pool: &PgPool,
    tracked_train_id: i64,
    pin_origin_crs: &str,
    pin_scheduled_departure: DateTime<Utc>,
    service_date: NaiveDate,
    crs_line_index: &HashMap<String, Vec<String>>,
) -> anyhow::Result<bool> {
    let Some(candidate_lines) = crs_line_index.get(&pin_origin_crs.to_uppercase()) else {
        return Ok(false);
    };

    let origin_tiplocs = queries::list_stanox_crs_for_crs(pool, pin_origin_crs).await?;
    if origin_tiplocs.is_empty() {
        return Ok(false);
    }
    let tiplocs: Vec<&str> = origin_tiplocs.iter().map(|r| r.tiploc.as_str()).collect();

    for line_id in candidate_lines {
        let Some(json) = queries::get_schedule_line_population(pool, line_id, service_date).await?
        else {
            continue;
        };
        let entries: Vec<LinePopulationEntry> = serde_json::from_value(json)?;

        let Some(matched) = schedule_query::match_pin(
            &entries,
            &tiplocs,
            pin_scheduled_departure,
            common::MATCH_TOLERANCE,
            |t| london_to_utc(service_date.and_time(t)),
        ) else {
            continue;
        };

        let calling_points: Vec<ScheduleCallingPointDto> =
            matched.calling_points.iter().map(ScheduleCallingPointDto::from).collect();
        let calling_points_json = serde_json::to_value(&calling_points)?;

        let destination_crs = match matched.calling_points.last() {
            Some(cp) => {
                queries::crs_for_tiploc(pool, schedule_query::normalize_tiploc(&cp.tiploc)).await?
            }
            None => None,
        };

        return train_tracking::apply_schedule_match(
            pool,
            tracked_train_id,
            &matched.uid,
            line_id,
            &calling_points_json,
            destination_crs.as_deref(),
        )
        .await;
    }

    Ok(false)
}

/// The periodic sweep's own entry point (Decision 3's "also run this same
/// attempt periodically"): re-runs `attempt_schedule_match` against every
/// still-`pending`, never-matched row. A single row's failure (e.g. a
/// malformed `schedule_line_population` JSONB for one line) is logged and
/// skipped, not propagated -- one bad row must never stop the sweep from
/// making progress on every other row. Returns the count of rows this
/// call actually matched, for the caller's own logging.
pub async fn run_schedule_match_sweep(
    pool: &PgPool,
    crs_line_index: &HashMap<String, Vec<String>>,
) -> anyhow::Result<u64> {
    let rows = train_tracking::list_pending_pins_for_schedule_match(pool).await?;
    let mut matched = 0u64;
    for row in rows {
        match attempt_schedule_match(
            pool,
            row.id,
            &row.pin_origin_crs,
            row.pin_scheduled_departure,
            row.service_date,
            crs_line_index,
        )
        .await
        {
            Ok(true) => matched += 1,
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    tracked_train_id = row.id,
                    "schedule match attempt failed for this pin; will retry next sweep"
                );
            }
        }
    }
    Ok(matched)
}
```

Note `PendingSchedulePin`/`HashMap` need importing:
`use std::collections::HashMap;` and
`use crate::data::train_tracking::PendingSchedulePin;` are implied by the
`train_tracking::list_pending_pins_for_schedule_match` call above (no
explicit `use` needed since it's called qualified) -- but `HashMap` DOES
need `use std::collections::HashMap;` added to `schedule_matching.rs`'s
imports (it is already imported for `crs_to_line_ids` from Task 4, so no
duplicate is needed if that import remains).

- [ ] **Step 2: Write `#[ignore]`d db tests for `attempt_schedule_match`**

Add to a new `#[cfg(test)] mod db_tests` in `schedule_matching.rs`,
following `train_tracking.rs`'s own `db_tests` module shape (same
`connect()` helper, same `#[ignore]` reason string convention). Seed a
`schedule_line_population` row directly (bypassing `schedule-reference`
entirely) and a `stanox_crs` row for the origin, then assert the match:

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

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

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attempt_schedule_match -- --ignored --test-threads=1`"]
    async fn attempt_schedule_match_reproduces_the_eus_bug_and_now_resolves_it() {
        let pool = connect().await;
        let user_id = "TEST-SCHEDULE-MATCH-EUS";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("schedule-match@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-EUS-STANOX', 'EUS', 'EUSTON', 'LONDON EUSTON', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ('west-coast-main-line', $1, $2) \
             ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
        )
        .bind(service_date)
        .bind(population_json("C99999", "EUSTON ", "19:15"))
        .execute(&pool)
        .await
        .expect("seed schedule_line_population");

        // The exact reported bug: a pin created more than an hour after
        // its train's own origin-departure window (the pin's own
        // scheduled_departure is still 19:15 -- what changes is that no
        // live TRUST Movement for it will ever arrive within this
        // process's test window, exactly mirroring "pinned an hour late,
        // TRUST's own ±20-minute window already closed").
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-05T19:15:00+01:00".parse().unwrap(); // BST -> 18:15 UTC
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
        assert!(matched, "the pin should schedule-match against C99999");

        let state = train_tracking::get_by_tracking_id(&pool, tracked_train_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "schedule_matched");
        assert_eq!(state.train_uid, Some("C99999".to_string()));
        assert_eq!(state.train_id, None, "train_id must stay TRUST-exclusive");

        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = 'west-coast-main-line' AND service_date = $1")
            .bind(service_date)
            .execute(&pool)
            .await
            .expect("cleanup population");
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-EUS-STANOX'")
            .execute(&pool)
            .await
            .expect("cleanup stanox_crs");
        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                attempt_schedule_match -- --ignored --test-threads=1`"]
    async fn attempt_schedule_match_with_no_candidate_line_leaves_the_row_pending() {
        let pool = connect().await;
        let user_id = "TEST-SCHEDULE-MATCH-NO-CANDIDATE";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("no-candidate@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("ZZZ")
        .bind("2026-09-05T19:15:00Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap())
        .fetch_one(&pool)
        .await
        .expect("seed fixture tracked_trains row");

        let matched = attempt_schedule_match(
            &pool,
            tracked_train_id,
            "ZZZ",
            "2026-09-05T19:15:00Z".parse().unwrap(),
            service_date,
            &HashMap::new(), // no candidate lines at all
        )
        .await
        .expect("attempt schedule match");
        assert!(!matched);

        let state = train_tracking::get_by_tracking_id(&pool, tracked_train_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "pending");
        assert_eq!(state.train_uid, None);

        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("cleanup user");
    }
}
```

- [ ] **Step 3: Run**

```bash
cargo build -p api
cargo test -p api attempt_schedule_match -- --ignored --test-threads=1
```

Expected: both new db tests pass (requires local Postgres with this
plan's migrations applied, per Task 1).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/schedule_matching.rs crates/api/src/data/train_tracking.rs
git commit -m "Add the schedule-first pin-matching function and its DB write path"
```

---

## Task 7: Wire schedule matching into pin creation

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Consumes: `crate::data::schedule_matching::attempt_schedule_match` (Task 6).
- Produces: `POST /Train/track` now returns `resolutionStatus:
  "schedule_matched"` when a synchronous match succeeds, `"pending"`
  otherwise (unchanged from today).

- [ ] **Step 1: Update `post_track`**

Change:

```rust
use crate::data::{delay_repay_rules, eta_blend, ticket_extraction, train_tracking};
```

to:

```rust
use crate::data::{delay_repay_rules, eta_blend, schedule_matching, ticket_extraction, train_tracking};
```

Change `post_track` from:

```rust
async fn post_track(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(pin): Json<TrackPinRequest>,
) -> Result<Json<TrackPinResponse>, (StatusCode, String)> {
    train_tracking::validate_pin(&pin, Utc::now()).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let tracking_id = train_tracking::create_pin(&app.database, &pin, &user.id)
        .await
        .map_err(internal_error("create tracking pin"))?;

    Ok(Json(TrackPinResponse {
        tracking_id,
        resolution_status: "pending",
    }))
}
```

to:

```rust
async fn post_track(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(pin): Json<TrackPinRequest>,
) -> Result<Json<TrackPinResponse>, (StatusCode, String)> {
    train_tracking::validate_pin(&pin, Utc::now()).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let tracking_id = train_tracking::create_pin(&app.database, &pin, &user.id)
        .await
        .map_err(internal_error("create tracking pin"))?;

    // Best-effort schedule-first match, attempted synchronously in the
    // same request (Decision 3 of
    // docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
    // A failure here must never fail pin creation itself -- the periodic
    // sweep (Task 8) retries any pin this call didn't resolve, including
    // one that failed with a real error.
    let resolution_status = match schedule_matching::attempt_schedule_match(
        &app.database,
        tracking_id,
        &pin.origin_crs,
        pin.scheduled_departure,
        pin.service_date,
        &app.schedule_crs_line_index,
    )
    .await
    {
        Ok(true) => "schedule_matched",
        Ok(false) => "pending",
        Err(err) => {
            tracing::warn!(
                error = ?err,
                tracking_id,
                "schedule match attempt failed at pin creation; pin stays pending"
            );
            "pending"
        }
    };

    Ok(Json(TrackPinResponse {
        tracking_id,
        resolution_status,
    }))
}
```

- [ ] **Step 2: Write a route-level `#[ignore]`d test**

Add to `routes/train.rs`'s existing route test module (same
`test_router`/`test_app`/`seed_session` helpers already used by
`post_tracked_train_name_the_owner_can_rename_and_clear`, etc.), seeding
a `schedule_line_population` row before posting to `/Train/track`:

```rust
#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            post_track_schedule_matches -- --ignored --test-threads=1`"]
async fn post_track_schedule_matches_a_pin_whose_train_a_live_movement_would_have_missed() {
    let pool = connect().await;
    let token = seed_session(&pool, "TEST-ROUTE-SCHEDULE-MATCH").await;

    sqlx::query(
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
         VALUES ('TEST-ROUTE-EUS-STANOX', 'EUS', 'EUSTON', 'LONDON EUSTON', 1) \
         ON CONFLICT (stanox) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("seed stanox_crs");

    let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
    sqlx::query(
        "INSERT INTO schedule_line_population (line_id, service_date, population) \
         VALUES ('west-coast-main-line', $1, $2) \
         ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
    )
    .bind(service_date)
    .bind(serde_json::json!([{
        "uid": "C88888",
        "calling_points": [{
            "tiploc": "EUSTON ",
            "kind": "Origin",
            "booked_arrival": null,
            "booked_departure": "19:15",
            "is_half_minute_arrival": false,
            "is_half_minute_departure": false
        }]
    }]))
    .execute(&pool)
    .await
    .expect("seed schedule_line_population");

    let router = test_router(test_app(pool.clone()));
    let (status, body) = post_json(
        router,
        "/Train/track".to_string(),
        Some(&token),
        serde_json::json!({
            "service_date": "2026-09-05",
            "origin_crs": "EUS",
            "scheduled_departure": "2026-09-05T18:15:00Z"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "response: {body:?}");
    assert_eq!(
        body.get("resolutionStatus").and_then(Value::as_str),
        Some("schedule_matched")
    );

    sqlx::query("DELETE FROM schedule_line_population WHERE line_id = 'west-coast-main-line' AND service_date = $1")
        .bind(service_date)
        .execute(&pool)
        .await
        .expect("cleanup population");
    sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-ROUTE-EUS-STANOX'")
        .execute(&pool)
        .await
        .expect("cleanup stanox_crs");
    cleanup_user(&pool, "TEST-ROUTE-SCHEDULE-MATCH").await;
}
```

(`test_app` built here must also carry the correct `schedule_crs_line_index`
-- check whether `test_app` builds `AppState` by hand (per Task 4 Step 5)
or via `AppState::init`; if by hand, add
`schedule_crs_line_index: HashMap::from([("EUS".to_string(), vec!["west-coast-main-line".to_string()])])`
to THIS test's own fixture, not the shared default, since this is the one
route test that needs a real candidate line.)

- [ ] **Step 3: Run**

```bash
cargo test -p api post_track -- --ignored --test-threads=1
```

Expected: passes, including this new test and every pre-existing
`post_track*`/route test unmodified.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Attempt a schedule match synchronously when a train is pinned"
```

---

## Task 8: Periodic schedule-match sweep

**Files:**
- Modify: `crates/api/src/data/config.rs`
- Modify: `crates/api/src/main.rs`

**Interfaces:**
- Consumes: `crate::data::schedule_matching::run_schedule_match_sweep` (Task 6).
- Produces: a new `tokio::spawn`ed background loop in `api`'s own
  `main()`, the first of its kind for this crate (it has otherwise always
  been a pure request/response server -- `enricher`'s `sweep_loop`/
  `reclaim_loop` in `crates/enricher/src/main.rs` is the direct precedent
  this mirrors).

- [ ] **Step 1: Add the interval config field**

In `crates/api/src/data/config.rs`, add to `ServiceArguments` (after
`full_coverage_enabled_default`):

```rust
    /// How often `api`'s own background schedule-match sweep re-attempts
    /// Decision 3's schedule-first resolution against every still-`pending`
    /// tracked-train row -- the retroactive-fix mechanism for a pin
    /// created before its service's `schedule_line_population` cycle ran,
    /// or before this feature shipped at all
    /// (docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
    /// Decision 6). Same "plain interval, no jitter" shape as
    /// `schedule-reference`'s own `poll_interval_secs`
    /// (`crates/schedule-reference/src/config.rs:25`). 300s (5 minutes)
    /// default -- frequent enough that a newly-published
    /// `schedule_line_population` row is picked up within a rail day's
    /// working hours, cheap enough (a handful of still-pending rows on a
    /// typical day) not to matter at this cadence. Deliberately NOT wired
    /// into the Helm chart -- same "default suffices, override via env if
    /// an operator ever needs to" posture as several other unwired
    /// `ServiceArguments` fields in this file.
    #[arg(long, env, default_value_t = 300)]
    pub schedule_match_interval_secs: u64,
```

- [ ] **Step 2: Spawn the sweep loop in `main.rs`**

Change:

```rust
use crate::app::{AppState, Router};
```

to:

```rust
use crate::app::{App, AppState, Router};
```

Add, immediately after `let app = AppState::init().await?;`:

```rust
    tokio::spawn(schedule_match_sweep_loop(app.clone()));
```

Add the function itself (near the bottom of `main.rs`, after `fn main`):

```rust
/// Periodic retry of Decision 3's schedule-first match against every
/// still-`pending`, never-schedule-matched tracked-train row -- the
/// mechanism that makes this feature retroactive-capable for a pin
/// created before its schedule's population was published, or before
/// this feature shipped at all (Decision 6 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
/// Mirrors `crates/enricher/src/main.rs`'s own `sweep_loop` shape -- the
/// established precedent in this workspace for "a service that is mostly
/// a request/response server also runs one background interval loop."
async fn schedule_match_sweep_loop(app: App) {
    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(app.config.schedule_match_interval_secs));
    loop {
        interval.tick().await;
        match data::schedule_matching::run_schedule_match_sweep(&app.database, &app.schedule_crs_line_index)
            .await
        {
            Ok(matched) if matched > 0 => {
                tracing::info!(matched, "schedule-match sweep resolved pending pins");
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(error = ?err, "schedule-match sweep failed; will retry next interval");
            }
        }
    }
}
```

(`data::schedule_matching` resolves via `main.rs`'s existing `pub mod
data;` declaration.)

- [ ] **Step 3: Build**

```bash
cargo build -p api
```

Expected: compiles clean. This task adds no new unit tests of its own
(the sweep's real logic, `run_schedule_match_sweep`, is already tested in
Task 6; this task is pure wiring) -- verified instead by Task 12's
end-to-end scenario.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/main.rs
git commit -m "Run the schedule-match sweep as a periodic background loop in api"
```

---

## Task 9: Relax `upsert_train_event`'s two-field guard

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Produces: `upsert_train_event` now flips `resolution_status` to
  `'resolved'` and writes `train_id` whenever `resolved_train_id.is_some()`
  alone; `train_uid` is written via `COALESCE`, never blindly overwritten
  with `NULL`.

**Test-suite audit (Open Question 2), performed before writing this
task**: grepped `resolved_train_uid|resolved_train_id|resolution_status`
across `crates/api/` and `crates/trust-consumer/`, then read
`crates/api/src/data/train_tracking.rs`'s full test module (both
`#[cfg(test)] mod tests` and `mod db_tests`) end to end. Finding: **there
is no existing test anywhere that calls `upsert_train_event` at all** --
not in `train_tracking.rs`'s own test modules, not in
`crates/api/src/routes/ingest.rs` (which has no test module), and
`crates/api` has no `tests/` integration-test directory. The only tests
matching the grep live in `crates/trust-consumer/src/process.rs`'s test
module, and every one of them asserts what `run_once` *produces* on the
`TrainMovementEventMessage` it returns (a `trust-consumer`-side concern,
already covered and unaffected by this task) -- never what `api`'s
`upsert_train_event` *does* with that message once posted. **Conclusion:
this change cannot break an existing test, because none exists.** This
task therefore both makes the change and adds the first direct tests for
this function.

- [ ] **Step 1: Write the failing tests first**

Add a new `#[cfg(test)] mod db_tests` section to
`crates/api/src/data/train_tracking.rs` (or extend the existing one,
whichever the file has by the time this task runs -- Task 6 may have
already added `db_tests` entries elsewhere in this same file):

```rust
fn fixture_event(tracked_train_id: i64, dedup_key: &str) -> common::TrainMovementEventMessage {
    common::TrainMovementEventMessage {
        tracked_train_id,
        resolved_train_uid: None,
        resolved_train_id: None,
        dedup_key: dedup_key.to_string(),
        msg_type: "0003".to_string(),
        event_type: Some("DEPARTURE".to_string()),
        loc_stanox: Some("72410".to_string()),
        loc_crs: Some("EUS".to_string()),
        planned_timestamp: Some("2026-09-05T18:15:00Z".parse().unwrap()),
        actual_timestamp: Some("2026-09-05T18:15:00Z".parse().unwrap()),
        variation_status: Some("ON TIME".to_string()),
        raw_body: serde_json::json!({}),
        status: "en_route".to_string(),
        last_reported_location: Some("EUS".to_string()),
        last_event_type: Some("DEPARTURE".to_string()),
        delay_minutes: Some(0),
        next_calling_point: Some("CRE".to_string()),
        eta_next: None,
        eta_source: None,
    }
}

#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            upsert_train_event -- --ignored --test-threads=1`"]
async fn upsert_train_event_with_only_resolved_train_id_resolves_and_preserves_the_existing_train_uid() {
    let pool = connect().await;
    let user_id = "TEST-UPSERT-SCHEDULE-MATCHED";
    seed_user(&pool, user_id).await;
    let (tracked_train_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, train_uid, resolution_status) \
         VALUES ($1, $2, $3, $4, $5, 'schedule_matched') RETURNING id",
    )
    .bind(user_id)
    .bind("2026-09-05".parse::<chrono::NaiveDate>().unwrap())
    .bind("EUS")
    .bind("2026-09-05T18:15:00Z".parse::<DateTime<Utc>>().unwrap())
    .bind("C88888") // schedule-matched train_uid, no train_id yet
    .fetch_one(&pool)
    .await
    .expect("seed schedule-matched tracked_trains row");

    let mut event = fixture_event(tracked_train_id, "dedup-only-train-id");
    event.resolved_train_uid = None; // the exact gap this task closes
    event.resolved_train_id = Some("221832406".to_string());

    upsert_train_event(&pool, &event).await.expect("upsert train event");

    let state = get_by_tracking_id(&pool, tracked_train_id)
        .await
        .expect("read tracked train")
        .expect("tracked train exists");
    assert_eq!(state.resolution_status, "resolved");
    assert_eq!(state.train_id, Some("221832406".to_string()));
    assert_eq!(
        state.train_uid,
        Some("C88888".to_string()),
        "the schedule-matched train_uid must survive, COALESCE-preserved, not overwritten with NULL"
    );

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            upsert_train_event -- --ignored --test-threads=1`"]
async fn upsert_train_event_with_both_fields_still_sets_both_train_uid_and_train_id() {
    let pool = connect().await;
    let user_id = "TEST-UPSERT-BOTH-FIELDS";
    seed_user(&pool, user_id).await;
    let tracking_id = seed_tracked_train(&pool, user_id).await;

    let mut event = fixture_event(tracking_id, "dedup-both-fields");
    event.resolved_train_uid = Some("C21373".to_string());
    event.resolved_train_id = Some("221832406".to_string());

    upsert_train_event(&pool, &event).await.expect("upsert train event");

    let state = get_by_tracking_id(&pool, tracking_id)
        .await
        .expect("read tracked train")
        .expect("tracked train exists");
    assert_eq!(state.resolution_status, "resolved");
    assert_eq!(state.train_uid, Some("C21373".to_string()));
    assert_eq!(state.train_id, Some("221832406".to_string()));

    cleanup_user(&pool, user_id).await;
}

#[tokio::test]
#[ignore = "requires a live database; see this plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            upsert_train_event -- --ignored --test-threads=1`"]
async fn upsert_train_event_with_neither_field_leaves_resolution_status_and_train_uid_untouched() {
    let pool = connect().await;
    let user_id = "TEST-UPSERT-NEITHER-FIELD";
    seed_user(&pool, user_id).await;
    let tracking_id = seed_tracked_train(&pool, user_id).await;

    let event = fixture_event(tracking_id, "dedup-neither-field"); // both None, the default

    upsert_train_event(&pool, &event).await.expect("upsert train event");

    let state = get_by_tracking_id(&pool, tracking_id)
        .await
        .expect("read tracked train")
        .expect("tracked train exists");
    assert_eq!(
        state.resolution_status, "pending",
        "resolution_status must not move without at least resolved_train_id"
    );
    assert_eq!(state.train_uid, None);
    assert_eq!(state.train_id, None);
    // The movement/current-state writes still happen unconditionally --
    // this guard only ever gates the tracked_trains UPDATE.
    assert_eq!(state.status, Some("en_route".to_string()));

    cleanup_user(&pool, user_id).await;
}
```

- [ ] **Step 2: Run the tests to see them fail against the current guard**

```bash
cargo test -p api upsert_train_event_with_only_resolved_train_id -- --ignored
```

Expected: **FAIL** -- the first test's `assert_eq!(state.resolution_status, "resolved")`
fails (still `"schedule_matched"`), since the current guard requires both
fields.

- [ ] **Step 3: Relax the guard**

In `crates/api/src/data/train_tracking.rs`, change:

```rust
    if let (Some(train_uid), Some(train_id)) = (&event.resolved_train_uid, &event.resolved_train_id)
    {
        sqlx::query(
            "UPDATE tracked_trains \
             SET train_uid = $2, train_id = $3, resolution_status = 'resolved', resolved_at = NOW() \
             WHERE id = $1",
        )
        .bind(event.tracked_train_id)
        .bind(train_uid)
        .bind(train_id)
        .execute(&mut *tx)
        .await?;
    }
```

to:

```rust
    // Fires on `resolved_train_id.is_some()` ALONE now -- Decision 5 of
    // docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md,
    // the required companion to schedule-first matching: once a schedule
    // match can populate `train_uid` before ANY TRUST message arrives, the
    // resolving Movement's own `resolved_train_uid` is frequently `None`
    // (this process's `pending_activations` map is unrelated to a
    // schedule match), and the old two-field guard would leave a pin with
    // fully live, correct tracking data stuck at `schedule_matched`
    // forever. `train_uid` uses `COALESCE`, never a blind overwrite,
    // preserving whatever value a schedule match (or an earlier message)
    // already wrote. `resolved`'s own two-field INVARIANT (both
    // `train_uid` and `train_id` bound) is unchanged: this is still the
    // only write that ever sets `resolution_status = 'resolved'`, and it
    // never leaves `train_uid` NULL when it does (either freshly supplied
    // here, or already present from an earlier message/schedule match).
    if let Some(train_id) = &event.resolved_train_id {
        sqlx::query(
            "UPDATE tracked_trains \
             SET train_uid = COALESCE($2, train_uid), train_id = $3, \
                 resolution_status = 'resolved', resolved_at = NOW() \
             WHERE id = $1",
        )
        .bind(event.tracked_train_id)
        .bind(&event.resolved_train_uid)
        .bind(train_id)
        .execute(&mut *tx)
        .await?;
    }
```

- [ ] **Step 4: Run the tests again to confirm they pass**

```bash
cargo test -p api upsert_train_event -- --ignored --test-threads=1
```

Expected: **PASS**, all three new tests.

- [ ] **Step 5: Run the full existing `api`/`trust-consumer` suites to confirm nothing regressed**

```bash
cargo test -p api --lib
cargo test -p trust-consumer
```

Expected: unchanged pass count -- confirms the audit's own conclusion
(nothing existing depended on the old guard).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Relax upsert_train_event's two-field guard so resolved_train_id alone resolves a pin"
```

---

## Task 10: Widen `trust-consumer`'s `apply_reference_reload` match arm

**Files:**
- Modify: `crates/trust-consumer/src/process.rs`

**Interfaces:**
- Produces: a `schedule_matched` `TrackedTrainRef` is now treated
  identically to a `pending` one for reference-reload rehydration.
  `list_active_tracked_trains` (`crates/api/src/data/queries.rs`) needs
  **no** change -- its `WHERE tt.resolution_status != 'unresolved'` filter
  already includes `schedule_matched` rows (confirmed by inspection:
  Decision 7's "additive" backward-compat claim holds here too).

- [ ] **Step 1: Write the failing test first**

Add to `process.rs`'s existing `#[cfg(test)] mod tests`, directly below
`an_already_resolved_ref_is_rehydrated_from_the_reference_reload`:

```rust
/// The exact scenario this task exists for: a schedule-matched pin (a
/// real `train_uid` already known, no `train_id` yet) must be rehydrated
/// into `reference.pending` -- NOT `state.resolved` -- so a live TRUST
/// Movement can still claim it via the ordinary CRS+time heuristic.
#[tokio::test]
async fn a_schedule_matched_ref_is_treated_as_pending_for_rehydration_and_can_still_be_claimed() {
    let mut feed = FakeMovementFeed::new(vec![vec![ORIGIN_DEPARTURE.to_string()]]);
    let mut reference = Reference { pending: Vec::new() };
    let mut state = ProcessorState::default();

    let mut schedule_matched_ref = tracked_ref(1, "schedule_matched", None);
    schedule_matched_ref.train_uid = Some("C88888".to_string()); // known from the schedule match
    schedule_matched_ref.pin_origin_crs = "WAT".to_string();
    schedule_matched_ref.pin_scheduled_departure = "2026-08-28T18:32:00Z".parse().unwrap();

    apply_reference_reload(vec![schedule_matched_ref], &mut reference, &mut state);
    assert_eq!(
        reference.pending.len(),
        1,
        "a schedule_matched ref must be rehydrated as a matchable pending pin"
    );

    let events = run_once(&mut feed, &reference, &mut state, &TEST_STANOX_CRS)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tracked_train_id, 1);
    assert_eq!(
        events[0].resolved_train_id,
        Some("221832406".to_string()),
        "the live Movement still claims it via the ordinary heuristic, unchanged"
    );
}
```

- [ ] **Step 2: Run to confirm it fails**

```bash
cargo test -p trust-consumer a_schedule_matched_ref_is_treated_as_pending
```

Expected: **FAIL** -- `reference.pending.len()` is `0` (the current match
arm's `_ => {}` silently drops the `"schedule_matched"` status).

- [ ] **Step 3: Widen the match arm**

In `crates/trust-consumer/src/process.rs`'s `apply_reference_reload`,
change:

```rust
    for tracked in refs {
        match tracked.resolution_status.as_str() {
            "pending" => pending.push(crate::matching::PendingPin {
                tracked_train_id: tracked.id,
                pin_origin_crs: tracked.pin_origin_crs,
                pin_scheduled_departure: tracked.pin_scheduled_departure,
            }),
            "resolved" => {
                if let Some(train_id) = tracked.train_id {
                    state.resolved.entry(train_id).or_insert(tracked.id);
                }
            }
            _ => {}
        }
    }
```

to:

```rust
    for tracked in refs {
        match tracked.resolution_status.as_str() {
            // `schedule_matched` is treated exactly like `pending` here --
            // it already carries a `train_uid` (irrelevant to this
            // matching heuristic, which only ever compares CRS + time,
            // never train_uid), but it still has no `train_id`, so it
            // must stay eligible for the same live-Movement claim a plain
            // `pending` row is (Decision 3 of
            // docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md).
            "pending" | "schedule_matched" => pending.push(crate::matching::PendingPin {
                tracked_train_id: tracked.id,
                pin_origin_crs: tracked.pin_origin_crs,
                pin_scheduled_departure: tracked.pin_scheduled_departure,
            }),
            "resolved" => {
                if let Some(train_id) = tracked.train_id {
                    state.resolved.entry(train_id).or_insert(tracked.id);
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 4: Run to confirm it passes**

```bash
cargo test -p trust-consumer
```

Expected: **PASS**, including every pre-existing test in this module
unmodified.

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/process.rs
git commit -m "Rehydrate schedule_matched refs as matchable pending pins on reference reload"
```

---

## Task 11: Frontend -- types, the new `TrainJourney` branch, and copy

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/components/TrainJourney.tsx`
- Modify: `frontend/components/TrainJourney.test.tsx`
- Modify: `frontend/app/train/by-id/[trackingId]/page.test.tsx`
- Modify: `frontend/app/train/[uid]/[date]/page.test.tsx`
- Modify: `frontend/app/page.tsx`
- Modify: `frontend/app/track/mine/page.tsx`
- Modify: `crates/api/src/data/train_tracking.rs` (the `TRACKED_TRAIN_STATE_SELECT` read path)

**Interfaces:**
- Consumes: nothing new at the wire level beyond this plan's own new
  columns (Task 1) and `resolution_status` value (already produced by
  Tasks 6-9).
- Produces: `ResolutionStatus` gains `'schedule_matched'`;
  `TrackedTrainState` gains `scheduleDestinationCrs`,
  `scheduleDestinationName`, `scheduleCallingPoints`.

- [ ] **Step 1: Add the read-time destination-name JOIN in `train_tracking.rs`**

Update `TrackedTrainState` (add three fields, after `train_id`):

```rust
    pub train_id: Option<String>,
    /// The matched schedule's own terminus CRS (Decision 3 step 4 of
    /// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md),
    /// `None` until schedule-matched (or if the terminus TIPLOC never
    /// resolved to a CRS -- see `schedule_matching::attempt_schedule_match`).
    pub schedule_destination_crs: Option<String>,
    /// See `pin_origin_name`'s own doc comment -- same
    /// `LEFT JOIN stations` mechanism, joined on `schedule_destination_crs`.
    pub schedule_destination_name: Option<String>,
    /// Opaque JSONB relay of the matched entry's calling points, already
    /// camelCase-shaped at write time
    /// (`schedule_matching::ScheduleCallingPointDto`) -- this crate does
    /// not deserialize it again on the way out.
    pub schedule_calling_points: Option<serde_json::Value>,
```

Update `TRACKED_TRAIN_STATE_SELECT`:

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

`TrackedTrainListItem`/its own query are **unchanged** -- staying
lighter than the detail view is this table's own existing precedent
(Decision 1 of the original tracked-trains-list design), and the list row
already shows `resolutionStatus` (now sometimes `schedule_matched`) with
no further change needed there beyond Step 5's label below.

- [ ] **Step 2: Widen the frontend types**

In `frontend/lib/types.ts`, change:

```ts
export type ResolutionStatus = 'pending' | 'resolved' | 'unresolved';
```

to:

```ts
export type ResolutionStatus = 'pending' | 'schedule_matched' | 'resolved' | 'unresolved';
```

Add a new exported type above `TrackedTrainState`:

```ts
export type ScheduleCallingPointKind = 'Origin' | 'Intermediate' | 'Terminate';

/** One calling point of a `schedule_matched` pin's matched service, as
 * snapshotted at match time (`crates/api/src/data/schedule_matching.rs`'s
 * `ScheduleCallingPointDto`) -- already camelCase on the wire, unlike the
 * Rust `schedule_query::CallingPoint` type it's derived from. */
export interface ScheduleCallingPoint {
  tiploc: string;
  kind: ScheduleCallingPointKind;
  bookedArrival: string | null; // "HH:MM:SS"
  bookedDeparture: string | null;
  isHalfMinuteArrival: boolean;
  isHalfMinuteDeparture: boolean;
}
```

Add three fields to `TrackedTrainState` (after `trainId`):

```ts
  trainId: string | null;
  // Populated once `resolutionStatus` is `'schedule_matched'` or later
  // (a schedule match's own destination -- may differ from
  // `pinDestinationCrs`, which is only what the user typed on the
  // tracking form and is optional). `null` until matched, or if the
  // matched schedule's terminus TIPLOC never resolved to a CRS.
  scheduleDestinationCrs: string | null;
  scheduleDestinationName: string | null;
  scheduleCallingPoints: ScheduleCallingPoint[] | null;
```

Widen `TrackPinResponse`:

```ts
/** `POST /Train/track`'s response body -- camelCase, like every other
 * `crates/api` public JSON response (only the request body above is
 * snake_case). `resolutionStatus` is `'pending'` unless a synchronous
 * schedule match succeeded at creation time, in which case it's
 * `'schedule_matched'` -- see
 * docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
 * Decision 3. */
export interface TrackPinResponse {
  trackingId: number;
  resolutionStatus: ResolutionStatus;
}
```

- [ ] **Step 3: Update the three `TrackedTrainState` fixture builders**

`frontend/components/TrainJourney.test.tsx`,
`frontend/app/train/by-id/[trackingId]/page.test.tsx`, and
`frontend/app/train/[uid]/[date]/page.test.tsx` each have their own
`baseState`/`trackedTrainState` helper building a full `TrackedTrainState`
object literal. In each, add the three new fields (defaulted `null`,
matching every other nullable field's own default pattern in that
helper):

```ts
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: null,
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    ...overrides,
    customName: overrides.customName ?? null,
```

(TypeScript will refuse to compile these files without this step, since
`TrackedTrainState` is now missing three required fields from each
literal -- run `npx tsc --noEmit` after Step 2 to see this fail loudly
before making this fix, confirming the type change is actually wired
through.)

- [ ] **Step 4: Add the `schedule_matched` branch to `TrainJourney.tsx`**

Add the `Tooltip` import (mirrors `EtaBadge.tsx`'s own caveat-tooltip
pattern) and insert a new branch between the existing `pending` and
`unresolved` branches:

```tsx
import { Alert, Badge, Group, Loader, Stack, Text, Tooltip } from '@mantine/core';
```

```tsx
  if (state.resolutionStatus === 'schedule_matched') {
    const destination = state.scheduleDestinationName ?? state.scheduleDestinationCrs;
    return (
      <Stack gap="sm">
        <Group gap="xs">
          <Text fw={500}>
            Matched to a scheduled service — Train {state.trainUid}
            {destination ? ` to ${destination}` : ''}
          </Text>
          <Tooltip label="This is the booked timetable, not a live report yet. It may change if Network Rail issues a late alteration, and we'll update this automatically once live tracking begins.">
            <Badge color="gray" variant="light">
              As scheduled
            </Badge>
          </Tooltip>
        </Group>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Waiting for Network Rail&apos;s live tracking to begin.
        </Text>
      </Stack>
    );
  }
```

(Placed directly after the closing `}` of the existing `pending` branch's
`if` block, before the existing `unresolved` branch's `if` -- order
matters only for readability here, since each branch already `return`s.)

- [ ] **Step 5: Add the `schedule_matched` label to both `STATUS_LABELS` objects**

In both `frontend/app/page.tsx` and `frontend/app/track/mine/page.tsx`
(kept independently duplicated, matching this codebase's own existing
"no shared extraction" convention for these two files -- see the comment
already on `page.tsx`'s `STATUS_LABELS`), add one entry:

```ts
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  schedule_matched: 'Matched to schedule',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};
```

No other change is needed in either file: both `TrackedTrainSummaryRow`/
`RowStatusBadge`'s `resolutionStatus !== 'resolved'` gray-badge branch and
their `href`'s `resolutionStatus === 'resolved' && trainUid` canonical-link
gate already treat any non-`resolved` value uniformly and correctly
(`schedule_matched` renders gray, same bucket as `pending`, and still
routes through `/train/by-id/{id}`, never the canonical `/train/{uid}/{date}`
link -- exactly Decision 7's own backward-compat claim, now verified by
Step 6's test).

- [ ] **Step 6: Write the new `TrainJourney.tsx` tests**

Add to `frontend/components/TrainJourney.test.tsx`:

```tsx
it('schedule_matched: names the matched train and destination, with a caveat badge', () => {
  renderWithMantine(
    <TrainJourney
      state={baseState({
        resolutionStatus: 'schedule_matched',
        trainUid: 'C88888',
        scheduleDestinationCrs: 'CRE',
        scheduleDestinationName: 'Crewe',
      })}
    />,
  );
  expect(screen.getByText(/Matched to a scheduled service — Train C88888 to Crewe/)).toBeInTheDocument();
  expect(screen.getByText('As scheduled')).toBeInTheDocument();
  expect(screen.getByText(/Waiting for Network Rail's live tracking to begin/)).toBeInTheDocument();
});

it('schedule_matched: falls back to the destination CRS when no name resolved, and omits it entirely when neither did', () => {
  renderWithMantine(
    <TrainJourney
      state={baseState({
        resolutionStatus: 'schedule_matched',
        trainUid: 'C88888',
        scheduleDestinationCrs: 'CRE',
      })}
    />,
  );
  expect(screen.getByText(/Train C88888 to CRE/)).toBeInTheDocument();

  renderWithMantine(
    <TrainJourney state={baseState({ resolutionStatus: 'schedule_matched', trainUid: 'C88888' })} />,
  );
  expect(screen.getAllByText(/Train C88888/).length).toBeGreaterThan(0);
});
```

- [ ] **Step 7: Run**

```bash
cd frontend
npx tsc --noEmit
npm test -- TrainJourney
```

Expected: type-check passes; all `TrainJourney` tests pass, including the
two new ones.

- [ ] **Step 8: Manual dev-server verification**

Per this repo's own standing practice for UI changes:

```bash
cd frontend
npm run dev
```

With a local `api`/Postgres running Task 1's migration and a
`schedule_line_population` row seeded for today (or by temporarily
lowering `MATCH_TOLERANCE`'s effective window is not needed -- simplest is
to seed a population row and a `stanox_crs` row directly via `psql`, as
Task 6's own test does), track a train from `/track` whose origin CRS and
scheduled time match that seeded entry, then open
`/train/by-id/{trackingId}` and confirm:
- The page renders the new "Matched to a scheduled service — Train
  {uid}..." heading (not the old "Waiting to hear from Network Rail"
  panel).
- The gray "As scheduled" badge appears, and hovering it shows the
  late-alteration caveat tooltip text.
- `/track/mine` and the home page's tracked-trains list both show the
  "Matched to schedule" badge for the same pin, in the same gray color as
  a `pending` row.

- [ ] **Step 9: Commit**

```bash
git add frontend/lib/types.ts frontend/components/TrainJourney.tsx frontend/components/TrainJourney.test.tsx \
        "frontend/app/train/by-id/[trackingId]/page.test.tsx" "frontend/app/train/[uid]/[date]/page.test.tsx" \
        frontend/app/page.tsx frontend/app/track/mine/page.tsx crates/api/src/data/train_tracking.rs
git commit -m "Add the schedule_matched frontend branch, types, and destination read-time join"
```

---

## Task 12: End-to-end verification

**Files:** none new -- this task runs the full verification matrix and
confirms the specific bug scenario is fixed.

- [ ] **Step 1: Full Rust verification, matching CI's exact invocations (`.github/workflows/ci.yml`)**

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

Then, with a local Postgres running and migrated (`sqlx migrate run` from
`crates/api`):

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api -p aggregator -- --ignored --test-threads=1
```

Expected: every step passes clean, including every new `#[ignore]`d test
added across Tasks 5-9.

- [ ] **Step 2: Full frontend verification**

```bash
cd frontend
npx tsc --noEmit
npm test
npm run build
```

Expected: clean typecheck, full vitest suite passes, production build
succeeds.

- [ ] **Step 3: The original-bug regression scenario, run explicitly**

This is Task 6's own
`attempt_schedule_match_reproduces_the_eus_bug_and_now_resolves_it` test,
called out here specifically because it is the direct answer to "does
this fix the reported bug":

```bash
cargo test -p api attempt_schedule_match_reproduces_the_eus_bug -- --ignored
```

Expected: **PASS**. Confirm by reading the test's own assertions once
more: a pin is created for an EUS-origin, 19:15-scheduled train (the exact
reported case) with a `schedule_line_population` row already seeded for
`west-coast-main-line` (EUS's real catalogued line, confirmed at
`lines/west-coast-main-line.toml:33-34` per the spec's own Decision 1) --
`attempt_schedule_match` (simulating what would happen well after TRUST's
own ±20-minute Movement window has already closed, since no live Movement
is posted anywhere in this test at all) still resolves the pin to
`resolution_status = 'schedule_matched'` with the correct `train_uid`,
and leaves `train_id` `None` (still TRUST-exclusive, per this plan's
Global Constraints) -- proving the pin no longer stays stuck at `pending`
forever the way the original bug report described.

- [ ] **Step 4: Confirm the periodic-sweep retroactive path (Decision 6) with a second, explicit test read-through**

Re-read `run_schedule_match_sweep`'s own test coverage (Task 6, Step 2's
`attempt_schedule_match_with_no_candidate_line_leaves_the_row_pending`
plus the EUS test) and confirm by inspection that
`list_pending_pins_for_schedule_match`'s `WHERE train_uid IS NULL AND
resolution_status = 'pending'` clause is exactly the set of rows Decision
6 describes as retroactively fixable: a row stuck at `pending` from
*before* this feature shipped is indistinguishable, at that `WHERE`
clause, from one created five minutes ago -- both are picked up by the
next sweep tick once `schedule_match_interval_secs` elapses, with no
special-cased backfill script, exactly as the spec requires.

- [ ] **Step 5: helm-lint sanity check (no chart changes expected, confirm none are needed)**

```bash
helm lint charts/distant-signal
helm lint charts/distant-signal -f charts/distant-signal/values-example.yaml
```

Expected: both pass unmodified -- this plan's only new config
(`schedule_match_interval_secs`) has a `clap` default and is deliberately
not wired into the chart (Task 8), so no chart change is required or
expected.

- [ ] **Step 6: Final commit (if anything from this task's own read-throughs surfaced a fix)**

If Steps 1-5 are all clean, there is nothing to commit for this task --
it is a verification pass, not a code-producing one. If any step surfaces
a real gap, fix it as part of the task where it belongs (do not bundle an
unrelated fix into this task's own commit) and re-run this task's steps
from the top.

---

## Self-review notes (carried over from plan authoring, not a task to execute)

- **Spec coverage**: Decisions 1-8 of the spec are each implemented by a
  specific task above (Decision 1: Task 6's line-index lookup; Decision 2:
  Task 4; Decision 3: Tasks 6-8; Decision 4: Tasks 1, 11; Decision 5: Task
  9; Decision 6: Task 8 + Task 12 Step 4; Decision 7: verified by
  inspection in Tasks 10-11, no code change needed beyond the label/type
  additions; Decision 8: informed this plan's choice not to reuse
  `full-coverage-consumer`'s `correlate.rs`/`population.rs` directly, only
  their *pattern*). All six Open Questions are resolved above and threaded
  through the relevant task. Every Non-goal in the spec (national schedule
  DB, `schedule_network_departures` reuse, `Activation.train_uid`
  exact-match, changing `MATCH_TOLERANCE`'s value) is respected -- none of
  the 12 tasks touch any of them.
- **Type/name consistency check**: `attempt_schedule_match`'s signature
  (Task 6) matches every call site (Task 7's `post_track`, Task 6's own
  `run_schedule_match_sweep`, Task 8's sweep loop indirectly via
  `run_schedule_match_sweep`). `ScheduleCallingPointDto` (Rust, Task 6) and
  `ScheduleCallingPoint` (TypeScript, Task 11) are named consistently
  field-for-field. `PendingSchedulePin` (Task 6) is the one and only
  struct `list_pending_pins_for_schedule_match`/`run_schedule_match_sweep`
  reference.

## Biggest open risk for the implementer

**The first-candidate-line-wins simplification in Task 6 (Open Question
3's resolution) is trusted reasoning, not independently verified against a
real multi-line-overlap CRS in this plan.** If two catalogued lines
genuinely disagree for the same `(uid, service_date)` in production (which
the spec argues should be structurally impossible, since STP resolution is
`(uid, date)`-keyed, not line-keyed) this plan's implementation would
silently take whichever candidate line happens to come first in
`crs_to_line_ids`'s HashMap-derived, not fully deterministic, iteration
order -- there is no defensive assertion or alerting anywhere in this plan
that would surface such a disagreement if the reasoning turns out to be
wrong. The mitigating factors are real (a wrong schedule match is
caveated copy, not asserted fact, per Task 11; TRUST's own live Movement
still supersedes it via Task 10's unchanged claiming logic), but this is
the one piece of this plan resting on trusted reasoning rather than a
test against real overlapping-line schedule data.
