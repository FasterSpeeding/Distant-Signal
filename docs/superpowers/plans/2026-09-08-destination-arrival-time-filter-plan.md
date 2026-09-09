# Destination-arrival time filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an optional, independent time-of-arrival filter on `GET /public/trains/search`'s `destination` field: `destination_from`/`destination_to`, bounding a new `destination_arrival` column, distinct from the existing `from`/`to` (which stay scoped to `station`).

**Architecture:** Mirror the existing `true_origin_crs` plumbing end to end: one new nullable column on `schedule_destination_departures`, computed once per schedule from the Terminate calling point's `booked_arrival` in `schedule-query`'s resolve pass, carried through the `schedule-reference` publish, the `api` upsert/search query, the route's params/validation, the JSON render, and the frontend form.

**Tech Stack:** Rust (axum, sqlx/Postgres), Next.js/React/TypeScript (Mantine, vitest).

**Spec:** docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md

## Global Constraints

- New column: `destination_arrival` (nullable `TIME`) on `schedule_destination_departures`.
- New query params: `destination_from` / `destination_to`, `"HH:MM"`, inclusive bounds, parsed with the existing `normalize_time` helper.
- Setting either without `destination` is a `400`.
- No new index (see spec's Index section).
- New JSON field: `destinationArrival` (`"HH:MM"` or `null`) in `calling_point_departure_json`'s output.
- Every touch point mirrors the existing `true_origin_crs` pattern in the same file, so diff review is a direct side-by-side.

---

### Task 1: Migration -- add the column

**Files:**
- Create: `crates/api/migrations/20260908130000_schedule_destination_departures_destination_arrival.sql`

**Interfaces:**
- Produces: `schedule_destination_departures.destination_arrival` (nullable `TIME`), consumed by Task 4's SQL and Task 8's live-DB tests.

- [ ] **Step 1: Write the migration**

```sql
-- Adds the destination's own arrival time as a second, independent
-- optional time filter on GET /public/trains/search, distinct from the
-- existing from/to (which stay scoped to the searched `station`'s own
-- `scheduled` time). See
-- docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
--
-- Mirrors true_origin_crs's own addition in the prior migration exactly:
-- computed once per schedule from the Terminate calling point (`.last()`),
-- here using its booked_arrival rather than booked_departure, since
-- CallingPointKind::Terminate is "arrival only, no departure"
-- (crates/schedule-query/src/records.rs). Nullable for the same reason
-- true_origin_crs is: the terminating calling point's own booked_arrival
-- can be absent from a real published schedule, and that degrades this
-- filter field to NULL rather than dropping the row.
ALTER TABLE schedule_destination_departures ADD COLUMN destination_arrival TIME;

-- No new index. destination_from/destination_to become two more AND
-- predicates evaluated against rows already narrowed by
-- schedule_destination_departures_calling_point_idx's equality/range scan
-- on (service_date, origin_crs, scheduled, train_uid) -- the same
-- after-the-narrowing-scan shape destination_crs already has. This filter
-- is also only ever usable together with destination (enforced at the API
-- layer as a 400 otherwise), which is itself already a non-leading
-- predicate on that same scan. Per the prior migration's own comment ("Do
-- not add a second index without a measured reason"), this doesn't add
-- one speculatively; a future measurement can revisit this.
```

- [ ] **Step 2: Run it against the local test database**

Run: `sqlx migrate run --database-url postgres://lucy@localhost:5432/distant_signal_test --source crates/api/migrations`

Expected: the new migration applies cleanly with no errors.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260908130000_schedule_destination_departures_destination_arrival.sql
git commit -m "Add destination_arrival column to schedule_destination_departures"
```

---

### Task 2: `schedule-query` -- carry `destination_arrival` on `DestinationDeparture`

**Files:**
- Modify: `crates/schedule-query/src/records.rs` (the `DestinationDeparture` struct, ~line 227)
- Modify: `crates/schedule-query/src/resolve.rs` (`departures_by_destination_crs`, ~line 263-319, and its test module)

**Interfaces:**
- Consumes: `crate::resolve::ResolvedSchedule.calling_points` (`Vec<CallingPoint>`, each with `kind: CallingPointKind`, `booked_arrival: Option<NaiveTime>`, `booked_departure: Option<NaiveTime>`, `tiploc: String`) -- already in scope in this file.
- Produces: `DestinationDeparture.destination_arrival: Option<NaiveTime>`, consumed by Task 3's `schedule_destination_departures_rows` and Task 4's `ScheduleDestinationDeparturesRow`.

- [ ] **Step 1: Write the failing tests in `resolve.rs`**

Add a `calling_point_with_arrival` test helper next to the existing `calling_point`/`calling_point_with_departure` ones (~line 380-404):

```rust
fn calling_point_with_arrival(tiploc: &str, kind: CallingPointKind, arrival: &str) -> CallingPoint {
    CallingPoint {
        tiploc: tiploc.to_string(),
        kind,
        booked_arrival: Some(NaiveTime::parse_from_str(arrival, "%H:%M").unwrap()),
        booked_departure: None,
        is_half_minute_arrival: false,
        is_half_minute_departure: false,
    }
}
```

Then add these tests after `departures_by_destination_crs_never_buckets_the_terminating_calling_point_itself` (~line 968):

```rust
#[test]
fn departures_by_destination_crs_attaches_the_terminating_calling_points_arrival_to_every_entry() {
    // The load-bearing mirror of
    // departures_by_destination_crs_attaches_the_schedules_true_origin_to_every_one_of_its_entries,
    // but for the LAST calling point's booked_arrival instead of the
    // FIRST's booked_departure.
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![
            calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
            calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
            calling_point_with_arrival("MNCRPIC", CallingPointKind::Terminate, "11:30"),
        ],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

    let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

    let manchester = &by_destination["MAN"];
    assert_eq!(manchester.len(), 2);
    for entry in manchester {
        assert_eq!(
            entry.destination_arrival,
            Some(NaiveTime::from_hms_opt(11, 30, 0).unwrap()),
            "every entry for this schedule must carry the SAME terminating arrival time"
        );
    }
}

#[test]
fn departures_by_destination_crs_degrades_destination_arrival_to_none_when_the_terminating_calling_point_has_no_booked_arrival() {
    // A Terminate calling point built with the plain `calling_point`
    // helper (no booked_arrival) -- a real-world gap in the CIF data, not
    // a test bug. Must degrade this filter field to None, not drop the
    // row or panic.
    let raw = vec![RawSchedule {
        basic: basic(
            "C11052",
            StpIndicator::Permanent,
            "2026-05-18",
            "2026-12-11",
            WEEKDAYS,
        ),
        calling_points: vec![
            calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
            calling_point("CREWE  ", CallingPointKind::Terminate),
        ],
    }];
    let index = ScheduleIndex::build(raw);
    let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
    let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

    let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

    assert_eq!(by_destination["CRE"].len(), 1);
    assert_eq!(by_destination["CRE"][0].destination_arrival, None);
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test -p schedule-query departures_by_destination_crs_attaches_the_terminating`

Expected: FAIL to compile -- `destination_arrival` doesn't exist yet on `DestinationDeparture`.

- [ ] **Step 3: Add the field and compute it**

In `crates/schedule-query/src/records.rs`, add to `DestinationDeparture` (~line 227-232), with a doc comment mirroring `true_origin_crs`'s own:

```rust
    /// `destination_arrival` is the schedule's REAL final calling point's
    /// (the `Terminate` one) `booked_arrival` -- the mirror of
    /// `true_origin_crs` above, but reading the LAST calling point's
    /// arrival instead of the FIRST's departure, because
    /// `CallingPointKind::Terminate` is "arrival only, no departure"
    /// (this crate's own `records.rs`). Computed once per schedule and
    /// IDENTICAL across every entry that schedule contributes, exactly
    /// like `true_origin_crs`. `None` when the terminating calling
    /// point's own `booked_arrival` is absent from a real published
    /// schedule -- a plain filter-field degrade, not a dropped row.
    /// Backs the OPTIONAL "arriving between" filter on
    /// `GET /public/trains/search?destination_from=&destination_to=`,
    /// which only applies when `destination` is also set -- see
    /// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
    pub destination_arrival: Option<NaiveTime>,
```

Update the struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationDeparture {
    pub uid: String,
    pub origin_crs: String,
    pub scheduled: NaiveTime,
    pub true_origin_crs: Option<String>,
    pub destination_arrival: Option<NaiveTime>,
}
```

In `crates/schedule-query/src/resolve.rs`'s `departures_by_destination_crs` (~line 286-315), add the mirrored once-per-schedule computation right after `true_origin_crs` is computed:

```rust
        // The mirror of true_origin_crs directly above, but from the
        // LAST calling point's booked_arrival (Terminate: arrival only,
        // no departure) instead of the FIRST's booked_departure.
        // Computed once per schedule, attached unchanged to every entry.
        let destination_arrival = resolved
            .calling_points
            .last()
            .and_then(|last| last.booked_arrival);
```

And add it to the `DestinationDeparture` construction:

```rust
                .push(crate::records::DestinationDeparture {
                    uid: resolved.uid.clone(),
                    origin_crs: origin_crs.clone(),
                    scheduled: departure,
                    true_origin_crs: true_origin_crs.clone(),
                    destination_arrival,
                });
```

(`destination_arrival` is `Copy` (`Option<NaiveTime>`), so no `.clone()` needed, unlike `true_origin_crs`.)

- [ ] **Step 4: Fix every other `DestinationDeparture` literal in this crate to compile**

Run: `cargo build -p schedule-query --tests 2>&1 | grep "missing field"` and add `destination_arrival: None` (or the real value where the test already asserts on it) to each reported literal.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p schedule-query departures_by_destination_crs`

Expected: PASS, including the two new tests and every pre-existing `departures_by_destination_crs_*` test unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-query/src/records.rs crates/schedule-query/src/resolve.rs
git commit -m "Compute destination_arrival alongside true_origin_crs in departures_by_destination_crs"
```

---

### Task 3: `schedule-reference` -- publish `destination_arrival`

**Files:**
- Modify: `crates/schedule-reference/src/main.rs` (`schedule_destination_departures_rows`, ~line 378-400, and its tests ~line 700-900)

**Interfaces:**
- Consumes: `schedule_query::DestinationDeparture.destination_arrival: Option<NaiveTime>` (Task 2).
- Produces: one more key, `"destination_arrival"`, in each `serde_json::Value` this function emits -- consumed by Task 4's `ScheduleDestinationDeparturesRow` deserialization.

- [ ] **Step 1: Find every `DestinationDeparture` literal in this file's tests and add the new field**

Run: `grep -n "true_origin_crs: " crates/schedule-reference/src/main.rs` to find every test fixture (there are ~6, per the earlier grep in this task's research: lines ~728, 734, 744, 808, 814, 856, 888, 894). Add `destination_arrival: None,` next to each `true_origin_crs: ...,` for now (Step 3 below adds one real-value case).

- [ ] **Step 2: Write the failing test**

Find `schedule_destination_departures_rows_includes_the_true_origin_crs_field` (~line 798) and add a sibling test right after it:

```rust
#[test]
fn schedule_destination_departures_rows_includes_the_destination_arrival_field() {
    let mut by_destination = std::collections::HashMap::new();
    by_destination.insert(
        "MAN".to_string(),
        vec![
            schedule_query::DestinationDeparture {
                uid: "C11052".to_string(),
                origin_crs: "EUS".to_string(),
                scheduled: NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                true_origin_crs: Some("EUS".to_string()),
                destination_arrival: Some(NaiveTime::from_hms_opt(11, 30, 0).unwrap()),
            },
            schedule_query::DestinationDeparture {
                uid: "C99999".to_string(),
                origin_crs: "CRE".to_string(),
                scheduled: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                true_origin_crs: None,
                destination_arrival: None,
            },
        ],
    );
    let today = "2026-09-08".parse().unwrap();

    let rows = schedule_destination_departures_rows(by_destination, today);

    let c11052_row = rows.iter().find(|r| r["train_uid"] == "C11052").unwrap();
    assert_eq!(c11052_row["destination_arrival"], "11:30:00");
    let c99999_row = rows.iter().find(|r| r["train_uid"] == "C99999").unwrap();
    assert!(
        c99999_row["destination_arrival"].is_null(),
        "a None destination_arrival must serialize as JSON null, not be omitted"
    );
}
```

(Match this test's exact literal construction style, including imports, to whatever the existing `schedule_destination_departures_rows_includes_the_true_origin_crs_field` test already uses in this file -- read it first and copy its exact `use`/helper pattern rather than guessing at `NaiveTime`'s import path.)

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p schedule-reference schedule_destination_departures_rows_includes_the_destination_arrival_field`

Expected: FAIL to compile (`destination_arrival` unknown field) until Task 2 is merged into this branch, then FAIL on the assertion (`destination_arrival` key not emitted).

- [ ] **Step 4: Add the field to the emitted JSON**

In `schedule_destination_departures_rows` (~line 389-397):

```rust
                serde_json::json!({
                    "service_date": today,
                    "destination_crs": destination_crs,
                    "scheduled": d.scheduled,
                    "train_uid": d.uid,
                    "origin_crs": d.origin_crs,
                    "true_origin_crs": d.true_origin_crs,
                    "destination_arrival": d.destination_arrival,
                })
```

Update this function's doc comment's "Budget ~90 bytes per entry" line to also mention `destination_arrival` (~10 more bytes), matching how it already calls out `true_origin_crs`'s cost.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p schedule-reference schedule_destination_departures`

Expected: PASS, including every pre-existing test in this file that touches `DestinationDeparture`/`schedule_destination_departures_rows`.

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-reference/src/main.rs
git commit -m "Publish destination_arrival on each schedule-destination-departures row"
```

---

### Task 4: `api` data layer -- storage, upsert, and search

**Files:**
- Modify: `crates/api/src/data/queries.rs`:
  - `ScheduleDestinationDeparturesRow` struct (~line 916-923)
  - `upsert_schedule_destination_departures` (~line 993-1041)
  - `search_schedule_calling_point_departures` (~line 1096-1181)
  - `schedule_destination_departures_query_tests` module (~line 2756+)

**Interfaces:**
- Consumes: `schedule_destination_departures.destination_arrival` column (Task 1); the JSON `"destination_arrival"` key (Task 3, though `api` never depends on `schedule-reference` directly -- it only deserializes the wire shape via `ScheduleDestinationDeparturesRow`'s own `Deserialize`).
- Produces:
  - `ScheduleDestinationDeparturesRow.destination_arrival: Option<chrono::NaiveTime>`, consumed by Task 6 (route -> query call, though the route doesn't construct this type -- it's `routes/ingest.rs`'s deserialize target, unaffected by this plan).
  - `search_schedule_calling_point_departures(..., destination_arrival_from: Option<NaiveTime>, destination_arrival_to: Option<NaiveTime>, ...)` -- new trailing-but-one params (before `after`, `limit` -- see exact signature below), consumed by Task 6's route handler.
  - Each result row's `serde_json::Value` gains a `"destination_arrival"` key (`"HH:MM:SS"` string or `Value::Null`), consumed by Task 7's `calling_point_departure_json`.

- [ ] **Step 1: Update `ScheduleDestinationDeparturesRow` and the upsert function**

In `crates/api/src/data/queries.rs`, add to the struct (~line 916-923):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDestinationDeparturesRow {
    pub service_date: chrono::NaiveDate,
    pub destination_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
    pub origin_crs: String,
    pub true_origin_crs: Option<String>,
    pub destination_arrival: Option<chrono::NaiveTime>,
}
```

In `upsert_schedule_destination_departures` (~line 993-1041), add a parallel `Vec` next to `true_origin_crs`'s:

```rust
    let destination_arrival: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.destination_arrival).collect();
```

Extend the `INSERT ... SELECT * FROM UNNEST(...)` to seven columns:

```rust
    let result = sqlx::query(
        "INSERT INTO schedule_destination_departures \
            (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::time[], $4::text[], $5::text[], $6::text[], $7::time[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&destination_crs)
    .bind(&scheduled)
    .bind(&train_uids)
    .bind(&origin_crs)
    .bind(&true_origin_crs)
    .bind(&destination_arrival)
    .execute(&mut *tx)
    .await?;
```

- [ ] **Step 2: Write the failing round-trip test**

Add next to `upsert_round_trips_true_origin_crs_including_a_null_value` (~line 2936-2969) in `schedule_destination_departures_query_tests`. First update the module's `row(...)` test helper (~line 2793-2809) to take and set the new field:

```rust
    fn row(
        service_date: chrono::NaiveDate,
        destination_crs: &str,
        scheduled: chrono::NaiveTime,
        train_uid: &str,
        origin_crs: &str,
        true_origin_crs: Option<&str>,
        destination_arrival: Option<chrono::NaiveTime>,
    ) -> ScheduleDestinationDeparturesRow {
        ScheduleDestinationDeparturesRow {
            service_date,
            destination_crs: destination_crs.to_string(),
            scheduled,
            train_uid: train_uid.to_string(),
            origin_crs: origin_crs.to_string(),
            true_origin_crs: true_origin_crs.map(str::to_string),
            destination_arrival,
        }
    }
```

Update every existing call site of `row(...)` in this module to pass a trailing `None` (or a real `Some(time(..))` where the new test below needs one) -- there are calls in `fixture_rows`, `upsert_wholesale_replaces_the_whole_service_date`, `upsert_round_trips_true_origin_crs_including_a_null_value`, `calling_point_fixture_rows`, `search_calling_point_only_returns_rows_for_the_requested_station`, `search_calling_point_is_scoped_to_the_requested_service_date_only`, and `search_calling_point_keyset_cursor_pages_without_gaps_or_repeats_and_breaks_ties_on_train_uid`.

Then add:

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            schedule_destination_departures -- --ignored --test-threads=1`"]
async fn upsert_round_trips_destination_arrival_including_a_null_value() {
    let pool = test_pool().await;
    let date = fixture_date(31);
    delete_day(&pool, date).await;

    upsert_schedule_destination_departures(
        &pool,
        &[
            row(date, "ZRD", time(8, 0), "C70001", "EUS", Some("EUS"), Some(time(11, 30))),
            row(date, "ZRD", time(9, 0), "C70002", "CRE", None, None),
        ],
    )
    .await
    .expect("seed rows");

    let stored: Vec<(String, Option<chrono::NaiveTime>)> = sqlx::query_as(
        "SELECT train_uid, destination_arrival FROM schedule_destination_departures \
         WHERE service_date = $1 ORDER BY train_uid",
    )
    .bind(date)
    .fetch_all(&pool)
    .await
    .expect("read back");

    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0], ("C70001".to_string(), Some(time(11, 30))));
    assert_eq!(
        stored[1],
        ("C70002".to_string(), None),
        "an absent destination_arrival must round-trip as SQL NULL, not a fabricated time"
    );

    delete_day(&pool, date).await;
}
```

- [ ] **Step 3: Run it, confirm it fails to compile, then run migrations and re-test**

Run: `cargo test -p api upsert_round_trips_destination_arrival -- --ignored --test-threads=1`

Expected: compile error until Task 1's migration is applied to the test DB (already done in Task 1 Step 2) and this task's Step 1 lands; then PASS.

- [ ] **Step 4: Update `search_schedule_calling_point_departures`'s signature and SQL**

Replace the function signature (~line 1096-1106):

```rust
#[allow(clippy::too_many_arguments)]
pub async fn search_schedule_calling_point_departures(
    pool: &PgPool,
    station_crs: &str,
    service_date: chrono::NaiveDate,
    scheduled_from: chrono::NaiveTime,
    true_origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    to_time: Option<chrono::NaiveTime>,
    destination_arrival_from: Option<chrono::NaiveTime>,
    destination_arrival_to: Option<chrono::NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>> {
```

Update the doc comment above it to add a paragraph mirroring the `scheduled_from`/`to_time` one:

```rust
/// `destination_arrival_from`/`destination_arrival_to` are a SEPARATE
/// inclusive bound pair on `destination_arrival`, independent of
/// `scheduled_from`/`to_time` above -- the former is "when does the train
/// reach `destination_crs`", the latter is "when is the train at
/// `station_crs`". Both pairs may be supplied at once; neither widens or
/// implies the other. The route layer (not this function) rejects either
/// being set without `destination_crs` -- this function applies whatever
/// it is given, filter-shaped, with no cross-field validation of its own,
/// matching how it already treats every other Option argument here.
```

Update the query body and bind order (~line 1109-1135):

```rust
    let rows: Vec<(String, String, Option<String>, chrono::NaiveTime, Option<chrono::NaiveTime>)> =
        sqlx::query_as(
            r#"
            SELECT train_uid, destination_crs, true_origin_crs, scheduled, destination_arrival
            FROM schedule_destination_departures
            WHERE service_date = $1
              AND origin_crs = $2
              AND scheduled >= $3
              AND ($4::text IS NULL OR true_origin_crs = $4)
              AND ($5::text IS NULL OR destination_crs = $5)
              AND ($6::time IS NULL OR scheduled <= $6)
              AND ($7::time IS NULL OR destination_arrival >= $7)
              AND ($8::time IS NULL OR destination_arrival <= $8)
              AND ($9::time IS NULL
                   OR (scheduled, train_uid) > ($9, $10))
            ORDER BY scheduled, train_uid
            LIMIT $11
            "#,
        )
        .bind(service_date)
        .bind(station_crs)
        .bind(scheduled_from)
        .bind(true_origin_crs)
        .bind(destination_crs)
        .bind(to_time)
        .bind(destination_arrival_from)
        .bind(destination_arrival_to)
        .bind(after.map(|c| c.scheduled))
        .bind(after.map(|c| c.train_uid.as_str()))
        .bind(fetch)
        .fetch_all(pool)
        .await?;
```

Update the row-tuple destructuring in the `has_more`/`next_cursor`/`departures` blocks below (~line 1147-1180) to the new 5-tuple shape, e.g.:

```rust
    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|(train_uid, _, _, scheduled, _)| CallingPointDepartureCursor {
                scheduled: *scheduled,
                train_uid: train_uid.clone(),
            })
    } else {
        None
    };

    let departures = page_rows
        .iter()
        .map(|(train_uid, destination_crs, true_origin_crs, scheduled, destination_arrival)| {
            serde_json::json!({
                "uid": train_uid,
                "destination_crs": destination_crs,
                "true_origin_crs": true_origin_crs,
                "scheduled": scheduled.format("%H:%M:%S").to_string(),
                "destination_arrival": destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
            })
        })
        .collect();
```

- [ ] **Step 5: Update every existing call site of `search_schedule_calling_point_departures` to compile**

Run: `cargo build -p api --tests 2>&1 | grep "search_schedule_calling_point_departures"` and insert `None, None,` (both new params, in that order) between the `to_time`/`Some(time(12,0))`-style argument and the `after`/cursor argument at every call site in `crates/api/src/data/queries.rs`'s test module (there are ~11, per this task's earlier research) and in `crates/api/src/routes/trains.rs` (Task 6 handles that one for real -- for now, if you reach it first, pass `None, None` there too as a placeholder Task 6 will replace).

- [ ] **Step 6: Write the failing filter test**

Add after `search_calling_point_filters_by_an_inclusive_time_range_and_excludes_before_now` (~line 3306-3332):

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            search_calling_point -- --ignored --test-threads=1`"]
async fn search_calling_point_filters_by_destination_arrival_independent_of_the_station_time_range() {
    let pool = test_pool().await;
    let date = fixture_date(32);
    delete_day(&pool, date).await;
    // Two trains both callable at RDG within the SAME scheduled (station)
    // time window, but arriving at WAT 40 minutes apart -- so only the
    // destination_arrival bound, not scheduled/to_time, can tell them
    // apart.
    upsert_schedule_destination_departures(
        &pool,
        &[
            row(date, "WAT", time(8, 0), "C80001", "RDG", None, Some(time(8, 40))),
            row(date, "WAT", time(8, 5), "C80002", "RDG", None, Some(time(9, 20))),
        ],
    )
    .await
    .expect("seed fixture");

    let page = search_schedule_calling_point_departures(
        &pool,
        "RDG",
        date,
        any_time(),
        None,
        Some("WAT"),
        None,
        Some(time(9, 0)),
        Some(time(9, 30)),
        None,
        100,
    )
    .await
    .expect("search")
    .expect("the day is published");

    assert_eq!(page.departures.len(), 1);
    assert_eq!(page.departures[0]["uid"], "C80002");
    assert_eq!(page.departures[0]["destination_arrival"], "09:20:00");

    delete_day(&pool, date).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            search_calling_point -- --ignored --test-threads=1`"]
async fn search_calling_point_with_no_destination_arrival_bounds_ignores_a_null_destination_arrival() {
    let pool = test_pool().await;
    let date = fixture_date(33);
    delete_day(&pool, date).await;
    upsert_schedule_destination_departures(
        &pool,
        &[row(date, "WAT", time(8, 0), "C80003", "RDG", None, None)],
    )
    .await
    .expect("seed fixture");

    let page = search_schedule_calling_point_departures(
        &pool, "RDG", date, any_time(), None, None, None, None, None, None, 100,
    )
    .await
    .expect("search")
    .expect("the day is published");

    assert_eq!(page.departures.len(), 1, "no destination_from/to means the NULL row is still returned");
    assert!(page.departures[0]["destination_arrival"].is_null());

    delete_day(&pool, date).await;
}
```

- [ ] **Step 7: Run the whole module and confirm everything passes**

Run: `cargo test -p api schedule_destination_departures -- --ignored --test-threads=1` and `cargo test -p api search_calling_point -- --ignored --test-threads=1` (requires `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test` in the environment, and Task 1's migration already applied).

Expected: PASS, including every pre-existing test in this module.

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add destination_arrival storage, upsert, and search filtering to the calling-point search query"
```

---

### Task 5: `api` ingest route test fixture

**Files:**
- Modify: `crates/api/src/routes/ingest.rs` (`post_schedule_destination_departures_upserts_the_rows`, ~line 1240-1300)

**Interfaces:**
- Consumes: `ScheduleDestinationDeparturesRow` (Task 4) -- `Option<T>` fields deserialize as `None` when the JSON key is absent (serde's standard behavior for self-describing formats), so this task's only obligation is to prove the field round-trips when it IS present, not to touch every other test in this file.

- [ ] **Step 1: Extend the existing test's JSON body**

In `post_schedule_destination_departures_upserts_the_rows`, add `"destination_arrival": "11:30:00"` to the first posted object and leave the second one without the key (proving both "present" and "absent" deserialize correctly):

```rust
let body = serde_json::json!([
    {
        "service_date": "2099-02-01",
        "destination_crs": "ZRB",
        "scheduled": "08:22:00",
        "train_uid": "C10001",
        "origin_crs": "EUS",
        "true_origin_crs": "PAD",
        "destination_arrival": "11:30:00"
    },
    {
        "service_date": "2099-02-01",
        "destination_crs": "ZRB",
        "scheduled": "10:05:00",
        "train_uid": "C10002",
        "origin_crs": "CRE",
        "true_origin_crs": null
    }
]);
```

Extend the read-back query and assertions:

```rust
let stored: Vec<(String, chrono::NaiveTime, String, String, Option<String>, Option<chrono::NaiveTime>)> =
    sqlx::query_as(
        "SELECT destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival \
         FROM schedule_destination_departures \
         WHERE service_date = '2099-02-01' \
         ORDER BY scheduled",
    )
    .fetch_all(&pool)
    .await
    .expect("read back the upserted rows");
```

and after the existing assertions add:

```rust
assert_eq!(stored[0].5, Some(chrono::NaiveTime::from_hms_opt(11, 30, 0).unwrap()));
assert_eq!(
    stored[1].5, None,
    "an absent destination_arrival key must deserialize as None, not fail or default to a real time"
);
```

- [ ] **Step 2: Run it**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api post_schedule_destination_departures_upserts_the_rows -- --ignored --test-threads=1`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/api/src/routes/ingest.rs
git commit -m "Extend the schedule-destination-departures ingest test to cover destination_arrival"
```

---

### Task 6: `api` route -- params, validation, and wiring

**Files:**
- Modify: `crates/api/src/routes/trains.rs` (`TrainSearchParams` ~line 79-106, `get_trains_search` ~line 195-275, and `db_tests` ~line 285+)

**Interfaces:**
- Consumes: `queries::search_schedule_calling_point_departures(..., destination_arrival_from, destination_arrival_to, ...)` (Task 4).
- Produces: `destination_from`/`destination_to` query params; a `400` when either is set without `destination`.

- [ ] **Step 1: Add the new params to `TrainSearchParams`**

```rust
    /// Optional, `"HH:MM"`, inclusive lower bound on the time the train
    /// ARRIVES at `destination` -- a SEPARATE filter from `from`/`to`
    /// above, which stay scoped to `station`. Requires `destination` to
    /// be set; see this route's own validation for why an arrival-time
    /// filter with nothing named to arrive at 400s instead of being
    /// silently ignored.
    destination_from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound. Same `destination`
    /// requirement as `destination_from`.
    destination_to: Option<String>,
```

Add these two fields to the struct, after `to` and before `limit`.

- [ ] **Step 2: Parse and validate them in `get_trains_search`**

After the existing `to_time` parsing block (~line 218-223), add:

```rust
    let destination_from_time = params
        .destination_from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("destination_from", s))
        .transpose()?;
    let destination_to_time = params
        .destination_to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("destination_to", s))
        .transpose()?;
    if (destination_from_time.is_some() || destination_to_time.is_some()) && destination.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "destination_from and destination_to require destination to be set".to_string(),
        ));
    }
```

- [ ] **Step 3: Pass the new values through to the query call**

Update the `queries::search_schedule_calling_point_departures(...)` call (~line 247-257) to insert the two new arguments in the same position Task 4 put them in the function signature (after `to_time`, before `after`):

```rust
    let Some(page) = queries::search_schedule_calling_point_departures(
        &app.database,
        &station,
        today,
        scheduled_from,
        origin.as_deref(),
        destination.as_deref(),
        to_time,
        destination_from_time,
        destination_to_time,
        after.as_ref(),
        limit,
    )
```

- [ ] **Step 4: Write the failing route-level tests**

Add to `db_tests`, after `trains_search_malformed_time_is_a_400` (~line 476-485):

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            trains_search -- --ignored --test-threads=1`"]
async fn trains_search_destination_from_without_destination_is_a_400() {
    let pool = connect().await;
    let (status, body) =
        get(&pool, "/trains/search?station=ZRB&destination_from=09:00").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("destination"), "400 body should name the field: {body}");
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            trains_search -- --ignored --test-threads=1`"]
async fn trains_search_destination_to_without_destination_is_a_400() {
    let pool = connect().await;
    let (status, _) = get(&pool, "/trains/search?station=ZRB&destination_to=09:00").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            trains_search -- --ignored --test-threads=1`"]
async fn trains_search_malformed_destination_from_is_a_400() {
    let pool = connect().await;
    let (status, body) = get(
        &pool,
        "/trains/search?station=ZRB&destination=WAT&destination_from=teatime",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("destination_from"), "400 body should name the field: {body}");
}
```

Add a live-filter test after `trains_search_applies_origin_destination_and_time_filters_together` (~line 620-635) -- extend `seed_today` won't work cleanly here since it doesn't set `destination_arrival`; write a self-contained fixture like `trains_search_origin_and_destination_are_independent_of_each_other` does:

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            trains_search -- --ignored --test-threads=1`"]
async fn trains_search_filters_by_destination_arrival_independent_of_from_to() {
    let pool = connect().await;
    delete_today(&pool).await;
    let today = chrono::Utc::now().date_naive();
    let (_, soon, later) = relative_times();
    // Both rows are at the SAME scheduled (station) time, "soon" -- so
    // only destination_from/destination_to, not from/to, can tell them
    // apart. Their destination_arrival values are `soon`+30m and
    // `later`+30m respectively, both still comfortably inside this
    // test's "at least an hour before midnight" guarantee from
    // relative_times().
    let arrival_a = soon + chrono::Duration::minutes(30);
    let arrival_b = later + chrono::Duration::minutes(30);
    for (train_uid, destination_arrival) in [("C90001", arrival_a), ("C90002", arrival_b)] {
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(today)
        .bind("WAT")
        .bind(soon)
        .bind(train_uid)
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .bind(destination_arrival)
        .execute(&pool)
        .await
        .expect("seed fixture row");
    }

    let uri = format!(
        "/trains/search?station=ZRB&destination=WAT&destination_from={}&destination_to={}",
        arrival_b.format("%H:%M"),
        arrival_b.format("%H:%M"),
    );
    let (status, body) = get(&pool, &uri).await;
    assert_eq!(status, StatusCode::OK);
    let rows = results(&body);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["uid"], "C90002");

    delete_today(&pool).await;
}
```

- [ ] **Step 5: Run them to verify they fail, then implement, then pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api trains_search -- --ignored --test-threads=1`

Expected: compile/assert failures before Steps 1-3 land, PASS after.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/trains.rs
git commit -m "Add destination_from/destination_to params to GET /public/trains/search"
```

---

### Task 7: `api` render layer -- `destinationArrival` on the wire

**Files:**
- Modify: `crates/api/src/render.rs` (`calling_point_departure_json`, ~line 208-220, and its tests ~line 658-685)

**Interfaces:**
- Consumes: the `serde_json::Value` row shape's `"destination_arrival"` key (Task 4).
- Produces: `destinationArrival` (`"HH:MM"` string or `null`) in the JSON this function returns, consumed by Task 8's frontend.

- [ ] **Step 1: Write the failing tests**

Add after `calling_point_departure_json_renders_a_null_origin_as_json_null_not_a_missing_key` (~line 673-684):

```rust
#[test]
fn calling_point_departure_json_renders_destination_arrival_trimmed_to_hh_mm() {
    let row = serde_json::json!({
        "uid": "C10001",
        "destination_crs": "WAT",
        "true_origin_crs": "PAD",
        "scheduled": "08:22:00",
        "destination_arrival": "11:30:00",
    });
    let json = calling_point_departure_json(&row, "RDG");
    assert_eq!(json["destinationArrival"], "11:30");
}

#[test]
fn calling_point_departure_json_renders_a_null_destination_arrival_as_json_null_not_a_missing_key() {
    let row = serde_json::json!({
        "uid": "C10002",
        "destination_crs": "WAT",
        "true_origin_crs": "PAD",
        "scheduled": "10:05:00",
        "destination_arrival": null,
    });
    let json = calling_point_departure_json(&row, "RDG");
    assert!(json["destinationArrival"].is_null());
    assert!(json.get("destinationArrival").is_some(), "must be explicit null, not omitted");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p api calling_point_departure_json_renders_destination_arrival`

Expected: FAIL (`destinationArrival` key absent).

- [ ] **Step 3: Implement**

```rust
pub(crate) fn calling_point_departure_json(d: &Value, station_crs: &str) -> Value {
    let scheduled = d
        .get("scheduled")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    let destination_arrival = d
        .get("destination_arrival")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "stationCrs": station_crs,
        "originCrs": d.get("true_origin_crs").cloned().unwrap_or(Value::Null),
        "destinationCrs": d.get("destination_crs").cloned().unwrap_or(Value::Null),
        "destinationArrival": destination_arrival,
    })
}
```

Note: `d.get("destination_arrival").and_then(Value::as_str)` returns `None` both when the key is absent and when it is JSON `null` -- either way `destination_arrival` ends up `None`, and `json!({"destinationArrival": None::<String>, ...})` serializes that as an explicit `null` (not an omitted key), matching `originCrs`'s existing null-handling.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api calling_point_departure_json`

Expected: PASS, including every pre-existing test in this function's module.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "Render destinationArrival on each calling-point search result row"
```

---

### Task 8: Full backend test suite and DB-backed suite

**Files:** none (verification only)

- [ ] **Step 1: Build the whole workspace**

Run: `cargo build --workspace`

Expected: no errors.

- [ ] **Step 2: Run the non-DB workspace test suite**

Run: `cargo test --workspace`

Expected: all pass (the `--ignored` DB tests are skipped by default, as today).

- [ ] **Step 3: Apply the new migration to the test database, if not already done in Task 1**

Run: `sqlx migrate run --database-url postgres://lucy@localhost:5432/distant_signal_test --source crates/api/migrations`

- [ ] **Step 4: Run the DB-backed API test suite**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`

Expected: all pass, including every test this plan added and every pre-existing one.

- [ ] **Step 5: No commit** (verification-only task).

---

### Task 9: Frontend -- `TrainSearchForm`

**Files:**
- Modify: `frontend/components/TrainSearchForm.tsx`
- Modify: `frontend/components/TrainSearchForm.test.tsx`

**Interfaces:**
- Consumes: `GET /public/trains/search`'s new `destinationArrival` result field (Task 7) and new `destination_from`/`destination_to` query params (Task 6).
- Produces: no new exports; this is a leaf component.

- [ ] **Step 1: Write the failing tests**

In `TrainSearchForm.test.tsx`, add `destinationArrival: string | null` to the `searchBody` row type and to `PAGE_ONE`/`PAGE_TWO`/`PAGE_THREE` fixtures (set to `null` throughout is fine -- no existing test asserts on it, and this plan's new tests build their own inline fixtures where they need a real value):

```ts
function searchBody(
  rows: Array<{
    uid: string;
    scheduled: string;
    stationCrs: string;
    originCrs: string | null;
    destinationCrs: string | null;
    destinationArrival: string | null;
  }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({ results: rows, nextCursor });
}
```

```ts
const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', destinationArrival: null },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT', destinationArrival: null },
];
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT', destinationArrival: null },
];
const PAGE_THREE = [
  { uid: 'C10004', scheduled: '13:15', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT', destinationArrival: null },
];
```

Fix every other inline `searchBody([{ ... }])` call in the file the same way (grep for `stationCrs:` in this test file to find them all -- there are two more, in the "?" placeholder test and the "distinguishes... 404" test doesn't call `searchBody` with rows).

Then add new tests at the end of the `describe` block, before the closing `});`:

```ts
it('does not render the arrival-time filter until a destination is entered', () => {
  vi.stubGlobal('fetch', mockFetchByUrl());
  renderWithMantine(<TrainSearchForm initialStation="MAN" />);

  expect(screen.queryByLabelText('Arrival from (optional)')).not.toBeInTheDocument();
  expect(screen.queryByLabelText('Arrival to (optional)')).not.toBeInTheDocument();
});

it('renders the arrival-time filter once a destination is entered', () => {
  vi.stubGlobal('fetch', mockFetchByUrl());
  renderWithMantine(<TrainSearchForm initialStation="MAN" initialDestination="WAT" />);

  expect(screen.getByLabelText('Arrival from (optional)')).toBeInTheDocument();
  expect(screen.getByLabelText('Arrival to (optional)')).toBeInTheDocument();
});

it('sends destination_from/destination_to only when a destination is set', async () => {
  const fetchMock = mockFetchByUrl();
  vi.stubGlobal('fetch', fetchMock);
  renderWithMantine(<TrainSearchForm initialStation="man" initialDestination="wat" />);

  fireEvent.change(screen.getByLabelText('Arrival from (optional)'), { target: { value: '09:00' } });
  fireEvent.change(screen.getByLabelText('Arrival to (optional)'), { target: { value: '09:30' } });
  fireEvent.click(screen.getByRole('button', { name: 'Search' }));

  await waitFor(() =>
    expect(searchCallUrl(fetchMock)).toBe(
      '/api/trains/search?station=MAN&destination=WAT&destination_from=09%3A00&destination_to=09%3A30',
    ),
  );
});

it('drops any previously-entered arrival-time filter once destination is cleared', async () => {
  const fetchMock = mockFetchByUrl();
  vi.stubGlobal('fetch', fetchMock);
  renderWithMantine(<TrainSearchForm initialStation="MAN" initialDestination="WAT" />);

  fireEvent.change(screen.getByLabelText('Arrival from (optional)'), { target: { value: '09:00' } });
  fireEvent.change(screen.getByLabelText('Destination (optional)') ?? screen.getByLabelText('Terminating at (optional)'), {
    target: { value: '' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Search' }));

  await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
});
```

(For the last test, use whichever exact label `Terminating at (optional)` -- the Autocomplete already carries that `label` prop in the current file, per this plan's own research; drop the `?? screen.getByLabelText(...)` fallback and just use `screen.getByLabelText('Terminating at (optional)')` directly once you've confirmed that's the real label in the file you're editing.)

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run TrainSearchForm`

Expected: FAIL (no such label in the DOM yet; `searchParams()` doesn't send the new params yet).

- [ ] **Step 3: Implement in `TrainSearchForm.tsx`**

Add to the `TrainSearchRow` interface:

```ts
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  stationCrs: string;
  originCrs: string | null;
  destinationCrs: string | null;
  destinationArrival: string | null;
}
```

Add two new pieces of state next to `fromTime`/`toTime` (~line 94-95):

```ts
  const [destinationArrivalFrom, setDestinationArrivalFrom] = useState('');
  const [destinationArrivalTo, setDestinationArrivalTo] = useState('');
```

Add validity checks next to `fromValid`/`toValid` (~line 110-113):

```ts
  const destinationArrivalFromValid =
    destinationArrivalFrom.trim() === '' || TIME_PATTERN.test(destinationArrivalFrom.trim());
  const destinationArrivalToValid =
    destinationArrivalTo.trim() === '' || TIME_PATTERN.test(destinationArrivalTo.trim());
  const canSearch =
    stationValid &&
    originValid &&
    destinationValid &&
    fromValid &&
    toValid &&
    destinationArrivalFromValid &&
    destinationArrivalToValid &&
    !searching;
```

Update `searchParams()` (~line 125-132) to send the new params only when `destinationCrs` is set -- this is what makes "clearing destination drops any stale arrival filter" true by construction, without needing to clear the state itself:

```ts
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
    if (destinationCrs.trim()) {
      params.set('destination', destinationCrs.trim().toUpperCase());
      if (destinationArrivalFrom.trim()) params.set('destination_from', destinationArrivalFrom.trim());
      if (destinationArrivalTo.trim()) params.set('destination_to', destinationArrivalTo.trim());
    }
    if (fromTime.trim()) params.set('from', fromTime.trim());
    if (toTime.trim()) params.set('to', toTime.trim());
    return params;
  }
```

Add the new inputs to the JSX, right after the existing `Group grow` block for `From`/`To` (~line 325-340), rendered only when `destinationCrs` is non-empty -- matching the file's existing `results.nextCursor !== null && (...)` conditional-render convention:

```tsx
      {destinationCrs.trim() !== '' && (
        <Group grow align="flex-start">
          <TextInput
            label="Arrival from (optional)"
            placeholder="09:00"
            description="When the train reaches the destination above -- separate from From/To, which are about the Station above."
            value={destinationArrivalFrom}
            onChange={(event) => setDestinationArrivalFrom(event.currentTarget.value)}
            error={
              destinationArrivalFrom.length > 0 && !destinationArrivalFromValid
                ? 'Must be a time like 09:00'
                : null
            }
          />
          <TextInput
            label="Arrival to (optional)"
            placeholder="09:30"
            value={destinationArrivalTo}
            onChange={(event) => setDestinationArrivalTo(event.currentTarget.value)}
            error={
              destinationArrivalTo.length > 0 && !destinationArrivalToValid ? 'Must be a time like 09:30' : null
            }
          />
        </Group>
      )}
```

Update the component's own top-of-file doc comment (~line 56-79) to mention the new filter pair and why it is conditional, mirroring how it already explains the origin/destination/from/to set.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run TrainSearchForm`

Expected: PASS, including every pre-existing test in this file.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrainSearchForm.tsx frontend/components/TrainSearchForm.test.tsx
git commit -m "Add the destination-arrival time filter to TrainSearchForm"
```

---

### Task 10: Full frontend verification and whole-branch review

**Files:** none (verification only)

- [ ] **Step 1: Run the full frontend test suite**

Run: `cd frontend && npx vitest run`

Expected: all pass.

- [ ] **Step 2: Run the frontend build**

Run: `cd frontend && npm run build`

Expected: succeeds with no type errors.

- [ ] **Step 3: Re-run the full backend suites one more time (they may have drifted from earlier tasks' partial states)**

Run: `cargo build --workspace && cargo test --workspace && DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`

Expected: all pass.

- [ ] **Step 4: Request code review**

Use `superpowers:requesting-code-review` against the full diff on this branch versus `main`.

- [ ] **Step 5: Address findings, push the branch, and report back**

Push to `origin` (never force-merge into `main` from this worktree). Report the branch name, the design decisions (column/param names, index choice), and the exact test commands run with their results.
