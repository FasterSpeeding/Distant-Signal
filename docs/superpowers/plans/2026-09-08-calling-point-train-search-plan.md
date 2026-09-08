# Calling-Point Train Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize `GET /public/trains/search` (and its one caller,
`TrainSearchForm`) from "destination required, origin optional" to "any
calling point (station) required, origin and destination both optional
filters."

**Architecture:** `schedule_destination_departures` is already one row per
departure-bearing calling point (confirmed by direct code reading, see the
spec's §0). No new table. Add one nullable column (`true_origin_crs`, the
schedule's real first calling point, computed the same way `destination_crs`
already is via `.last()`, just via `.first()`) and one new index leading on
`origin_crs` (already "the calling point of this row"). Replace the
destination-keyed query/route/render/frontend with a calling-point-keyed
equivalent, in place — no external consumer to keep the old contract for.

**Tech Stack:** Rust (axum, sqlx runtime-checked queries only, no `query!`
macro), Postgres, Next.js/React/TypeScript, Mantine, vitest.

**Spec:** `docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`

## Global Constraints

- No `sqlx::query!`/`query_as!` macros anywhere — this crate is
  runtime-checked-only (existing convention, unchanged).
- No new table. One additive migration only (nullable column + index).
- No change to publish cadence, "no resident index," "no synchronous
  cross-service call," "no operator filter," "no date filter," "404 means
  no publish for today" — all inherited unchanged from the predecessor specs.
- `GET /public/trains/search`'s query-param and response contract changes
  are BREAKING and deliberate (§6 of the spec) — do not add
  backward-compatibility shims for the old `destination`-required shape.
- Every DB-backed test in this repo is `#[ignore]`d with the standard
  comment format (`"requires a live database; run with \`DATABASE_URL=...
  cargo test -p <crate> <name> -- --ignored --test-threads=1\`"`) and reads
  `DATABASE_URL` from the environment via `test_pool()`/`connect()` helpers
  already present in each file — do not invent a new DB-connection
  convention.
- Local test database: `postgres://lucy@localhost:5432/distant_signal_test`
  (no password; role `lucy` is a local superuser) — already migrated up to
  `20260907130000_schedule_destination_departures.sql` as of this plan.
  Confirm connectivity before relying on it (Task 1, Step 3).
- Commit after every task using this repo's existing commit style (see
  `git log --oneline -15` for tone — short, imperative, no ticket numbers).

---

## Task 1: Migration — `true_origin_crs` column and the calling-point index

**Files:**
- Create: `crates/api/migrations/20260908120000_schedule_destination_departures_calling_point_search.sql`

**Interfaces:**
- Produces: a nullable `true_origin_crs TEXT` column and a
  `schedule_destination_departures_calling_point_idx` btree index on
  `schedule_destination_departures`, both of which every later task's SQL
  depends on.

- [ ] **Step 1: Confirm the local test database is reachable**

Run:
```
psql "postgres://lucy@localhost:5432/distant_signal_test" -c "\dt schedule_destination_departures"
```
Expected: the table is listed. If this fails, stop and report — every later
task's `#[ignore]`d DB tests depend on this database being reachable at
`DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test`.

- [ ] **Step 2: Write the migration**

```sql
-- Generalizes the destination-first search
-- (2026-09-07-train-listing-destination-search-sizing-design.md) into a
-- calling-point-first one
-- (2026-09-08-calling-point-train-search-design.md). See that document's
-- §0 for why this is an additive column + index, not a new table: the
-- table is already one-row-per-departure-bearing-calling-point; only the
-- QUERY's leading equality column changes, from `destination_crs` to
-- `origin_crs` (already exactly "the calling point of this row" -- see
-- `schedule_query::DestinationDeparture`'s own doc comment), plus one new
-- column to carry the schedule's TRUE origin independently of which
-- calling point a row represents.

ALTER TABLE schedule_destination_departures ADD COLUMN true_origin_crs TEXT;

-- New leading index for the new primary query shape: equality on
-- (service_date, origin_crs) -- "calls at this station" -- then a range
-- scan on scheduled, with train_uid as the keyset cursor's tiebreaker.
-- Does NOT replace the existing primary key, which remains required for
-- upsert idempotency (ON CONFLICT DO NOTHING targets it) and still serves
-- destination_crs as a real, if no-longer-leading, filter column.
CREATE INDEX schedule_destination_departures_calling_point_idx
    ON schedule_destination_departures (service_date, origin_crs, scheduled, train_uid);
```

Save this as the file named above.

- [ ] **Step 3: Apply the migration to the local test database**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo sqlx migrate run --source crates/api/migrations
```

If `sqlx-cli` is not installed, apply it directly instead:
```
psql "postgres://lucy@localhost:5432/distant_signal_test" -f crates/api/migrations/20260908120000_schedule_destination_departures_calling_point_search.sql
```

Expected: no error.

- [ ] **Step 4: Verify the column and index exist**

Run:
```
psql "postgres://lucy@localhost:5432/distant_signal_test" -c "\d schedule_destination_departures"
```
Expected: `true_origin_crs` listed as a nullable `text` column, and
`schedule_destination_departures_calling_point_idx` listed among the
indexes.

- [ ] **Step 5: Commit**

```bash
git add crates/api/migrations/20260908120000_schedule_destination_departures_calling_point_search.sql
git commit -m "Add true_origin_crs column and calling-point index to schedule_destination_departures"
```

---

## Task 2: `schedule-query` — compute and attach the schedule's true origin

**Files:**
- Modify: `crates/schedule-query/src/records.rs:190-215` (`DestinationDeparture`)
- Modify: `crates/schedule-query/src/resolve.rs:260-305` (`departures_by_destination_crs`)
- Test: `crates/schedule-query/src/resolve.rs` (`#[cfg(test)] mod tests`, near the existing `departures_by_destination_crs_*` tests, `:650-873`)

**Interfaces:**
- Produces: `DestinationDeparture.true_origin_crs: Option<String>` — the
  schedule's real first calling point's CRS, identical across every row one
  schedule contributes, `None` when that TIPLOC doesn't resolve via
  `tiploc_to_crs`. Later tasks (`schedule-reference`'s flatten function,
  `crates/api`'s row struct) read this field.

- [ ] **Step 1: Write the failing tests**

Add to `crates/schedule-query/src/resolve.rs`'s `#[cfg(test)] mod tests`,
directly after
`departures_by_destination_crs_buckets_every_departure_bearing_calling_point_under_one_destination`
(around line 726):

```rust
    #[test]
    fn departures_by_destination_crs_attaches_the_schedules_true_origin_to_every_one_of_its_entries()
    {
        // The load-bearing distinction from `origin_crs`: EVERY entry of
        // this schedule carries the SAME true_origin_crs (EUS), even the
        // entry whose own origin_crs (the calling point it represents) is
        // CRE, not EUS.
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
                calling_point("MNCRPIC", CallingPointKind::Terminate),
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
            assert_eq!(entry.true_origin_crs, Some("EUS".to_string()));
        }
        let mut origins: Vec<&str> = manchester.iter().map(|d| d.origin_crs.as_str()).collect();
        origins.sort();
        assert_eq!(origins, vec!["CRE", "EUS"]);
    }

    #[test]
    fn departures_by_destination_crs_keeps_a_row_with_true_origin_crs_none_when_the_schedules_first_calling_point_is_unresolved()
    {
        // Contrast with departures_by_destination_crs_drops_a_schedule_whose_destination_tiploc_is_unresolved
        // (a bucket-KEY unresolved -> drop the whole schedule). true_origin_crs
        // is a plain FILTER field, so it follows departures_by_crs's own
        // softer "degrade to None, keep the row" precedent instead.
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
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        // EUSTON (the schedule's true origin) deliberately absent; CREWE
        // and MNCRPIC both resolve.
        let tiploc_to_crs = tiploc_map(&[("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(
            by_destination["MAN"].len(),
            1,
            "EUSTON's own row is still dropped -- its origin_crs can't resolve either, same as \
             departures_by_crs_drops_a_calling_point_whose_own_tiploc_is_unresolved"
        );
        assert_eq!(by_destination["MAN"][0].origin_crs, "CRE");
        assert_eq!(
            by_destination["MAN"][0].true_origin_crs, None,
            "the schedule's true origin TIPLOC never resolved, so this filter field degrades to \
             None -- it does NOT drop the row the way an unresolved destination_crs would"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p schedule-query departures_by_destination_crs`
Expected: a compile error — `DestinationDeparture` has no field
`true_origin_crs` yet.

- [ ] **Step 3: Add the field to `DestinationDeparture`**

In `crates/schedule-query/src/records.rs`, replace lines 190-215 (the
`DestinationDeparture` doc comment and struct) with:

```rust
/// One departure-bearing calling point of a schedule that TERMINATES at
/// some destination CRS, as bucketed by
/// [`crate::resolve::departures_by_destination_crs`]. The destination CRS
/// itself is deliberately absent from this struct: it is the bucket key
/// (identical for every entry in a bucket), exactly as the origin CRS is
/// the bucket key for [`ScheduleDeparture`]/`departures_by_crs`.
///
/// `origin_crs` means "the station this train departs FROM", which is the
/// calling point's own CRS -- an `Origin` calling point for the first
/// entry, an `Intermediate` one for every later entry of the same
/// schedule. It is NOT necessarily the schedule's own first station, and
/// is deliberately not the same concept as `trains.origin_crs` in
/// `crates/api`, which always is. This is the field the calling-point-first
/// train search (`GET /public/trains/search?station=`) filters its
/// PRIMARY, required key on.
///
/// `true_origin_crs` is the schedule's REAL first calling point's CRS --
/// the one this struct's own `origin_crs` field doc explicitly says
/// `origin_crs` is NOT. It is computed once per schedule (via
/// `resolved.calling_points.first()`, the exact mirror of how the bucket
/// key is computed via `.last()`) and is IDENTICAL across every entry that
/// schedule contributes, unlike `origin_crs` which varies per entry. `None`
/// when the schedule's first calling point's TIPLOC doesn't resolve via
/// `tiploc_to_crs` -- a plain filter-field degrade, not a dropped row (see
/// [`crate::resolve::departures_by_destination_crs`]'s own doc comment for
/// why this differs from how an unresolved bucket-key destination is
/// treated). Backs the OPTIONAL "originating at" filter on
/// `GET /public/trains/search?origin=`, which is deliberately independent
/// of `station=`/`origin_crs` above -- see
/// docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md
/// §0.2 for why these needed to become two different columns.
///
/// `scheduled` is Europe/London LOCAL civil time, straight off the CIF
/// body, same as [`ScheduleDeparture::scheduled`] -- never UTC. See
/// `crates/schedule-reference/src/main.rs`'s `london_local_time_at` for
/// the one place that distinction is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationDeparture {
    pub uid: String,
    pub origin_crs: String,
    pub scheduled: NaiveTime,
    pub true_origin_crs: Option<String>,
}
```

- [ ] **Step 4: Compute and attach `true_origin_crs` in `departures_by_destination_crs`**

In `crates/schedule-query/src/resolve.rs`, replace the body of
`departures_by_destination_crs` (lines 265-305) with:

```rust
pub fn departures_by_destination_crs(
    index: &ScheduleIndex,
    date: NaiveDate,
    now: NaiveTime,
    tiploc_to_crs: &HashMap<String, String>,
) -> HashMap<String, Vec<crate::records::DestinationDeparture>> {
    let mut by_destination: HashMap<String, Vec<crate::records::DestinationDeparture>> =
        HashMap::new();

    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        let Some(destination_crs) = resolved
            .calling_points
            .last()
            .and_then(|last| tiploc_to_crs.get(normalize_tiploc(&last.tiploc)))
        else {
            continue;
        };
        // Computed once per schedule, exactly like destination_crs above,
        // and attached unchanged to every entry this schedule contributes
        // -- NOT recomputed per calling point, which is what would make it
        // just a duplicate of `origin_crs` instead of the schedule's own
        // true first stop.
        let true_origin_crs = resolved
            .calling_points
            .first()
            .and_then(|first| tiploc_to_crs.get(normalize_tiploc(&first.tiploc)))
            .cloned();
        for cp in &resolved.calling_points {
            let Some(departure) = cp.booked_departure else {
                continue;
            };
            if departure < now {
                continue;
            }
            let Some(origin_crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else {
                continue;
            };
            by_destination
                .entry(destination_crs.clone())
                .or_default()
                .push(crate::records::DestinationDeparture {
                    uid: resolved.uid.clone(),
                    origin_crs: origin_crs.clone(),
                    scheduled: departure,
                    true_origin_crs: true_origin_crs.clone(),
                });
        }
    }

    by_destination
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p schedule-query`
Expected: all pass, including the two new tests and all seven pre-existing
`departures_by_destination_crs_*` tests (they don't construct
`DestinationDeparture` literals directly, so the new field doesn't break
them).

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-query/src/records.rs crates/schedule-query/src/resolve.rs
git commit -m "Add true_origin_crs to DestinationDeparture, computed once per schedule"
```

---

## Task 3: `crates/api` data layer — plumb `true_origin_crs` through the row struct and upsert

**Files:**
- Modify: `crates/api/src/data/queries.rs:915-1039` (`ScheduleDestinationDeparturesRow`, `upsert_schedule_destination_departures`)
- Modify: `crates/api/src/data/queries.rs` (test module: `row()` helper and `fixture_rows()`, around line 2801-2827)
- Modify: `crates/api/src/routes/ingest.rs` (test module: `post_schedule_destination_departures_upserts_the_rows`, around line 1240-1309)
- Test: same files (`#[ignore]`d DB tests)

**Interfaces:**
- Consumes: Task 1's `true_origin_crs` column and its migration having been
  applied to the test database.
- Produces: `ScheduleDestinationDeparturesRow.true_origin_crs: Option<String>`,
  and `upsert_schedule_destination_departures` persisting it. Task 4's new
  search function reads this column back out.
- Deliberately does NOT touch `search_schedule_destination_departures` or
  its cursor/page types yet — that function still exists, unmodified, and
  its own pre-existing tests must still compile and pass after this task
  (they don't reference the new column at all). Task 4 replaces it wholesale.

- [ ] **Step 1: Add the field to `ScheduleDestinationDeparturesRow`**

In `crates/api/src/data/queries.rs`, replace lines 915-922 with:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDestinationDeparturesRow {
    pub service_date: chrono::NaiveDate,
    pub destination_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
    pub origin_crs: String,
    pub true_origin_crs: Option<String>,
}
```

- [ ] **Step 2: Bind the new column in the upsert**

In the same file, inside `upsert_schedule_destination_departures`
(currently lines 994-1039), add a sixth parallel `Vec` right after the
existing `origin_crs` one, and extend the INSERT/UNNEST/bind chain to six
columns:

```rust
pub async fn upsert_schedule_destination_departures(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let service_dates: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.service_date).collect();
    let destination_crs: Vec<&str> = rows.iter().map(|r| r.destination_crs.as_str()).collect();
    let scheduled: Vec<chrono::NaiveTime> = rows.iter().map(|r| r.scheduled).collect();
    let train_uids: Vec<&str> = rows.iter().map(|r| r.train_uid.as_str()).collect();
    let origin_crs: Vec<&str> = rows.iter().map(|r| r.origin_crs.as_str()).collect();
    let true_origin_crs: Vec<Option<&str>> =
        rows.iter().map(|r| r.true_origin_crs.as_deref()).collect();

    // Normally exactly one date. Handled as a set anyway so a batch that
    // straddles a rail-day boundary replaces both days rather than half of
    // one -- and so the DELETE can never be wider than what is being
    // written.
    let mut distinct_dates = service_dates.clone();
    distinct_dates.sort_unstable();
    distinct_dates.dedup();

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = ANY($1::date[])")
        .bind(&distinct_dates)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query(
        "INSERT INTO schedule_destination_departures \
            (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::time[], $4::text[], $5::text[], $6::text[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&destination_crs)
    .bind(&scheduled)
    .bind(&train_uids)
    .bind(&origin_crs)
    .bind(&true_origin_crs)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 3: Update the test `row()` helper and `fixture_rows()`**

In the `#[cfg(test)]` module of the same file, replace the `row()` helper
(around line 2801-2815) with:

```rust
    fn row(
        service_date: chrono::NaiveDate,
        destination_crs: &str,
        scheduled: chrono::NaiveTime,
        train_uid: &str,
        origin_crs: &str,
        true_origin_crs: Option<&str>,
    ) -> ScheduleDestinationDeparturesRow {
        ScheduleDestinationDeparturesRow {
            service_date,
            destination_crs: destination_crs.to_string(),
            scheduled,
            train_uid: train_uid.to_string(),
            origin_crs: origin_crs.to_string(),
            true_origin_crs: true_origin_crs.map(str::to_string),
        }
    }
```

Every existing call site of `row(...)` in this test module (in
`fixture_rows`, `upsert_wholesale_replaces_the_whole_service_date`,
`upsert_with_an_empty_batch_does_not_wipe_the_day`'s call to `seed`, and
`search_is_scoped_to_the_requested_service_date_only`) now needs a sixth
argument. Add `None` to every existing call site EXCEPT `fixture_rows`,
which should give each of its three rows a real value so Task 4 can build
tests against a fixture that already discriminates `origin_crs` from
`true_origin_crs`:

```rust
    fn fixture_rows(service_date: chrono::NaiveDate) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(service_date, "ZRD", time(8, 22), "C10001", "EUS", Some("PAD")),
            row(service_date, "ZRD", time(10, 5), "C10002", "CRE", Some("SWA")),
            row(service_date, "ZRD", time(18, 40), "C10003", "EUS", Some("PAD")),
        ]
    }
```

For the other call sites (`row(date, "ZRB", time(8, 0), "OLD1", "EUS")`,
`row(date, "ZRC", time(9, 0), "OLD2", "CRE")`,
`row(date, "ZRB", time(9, 30), "NEW1", "CRE")`,
`row(yesterday, "ZRD", time(8, 0), "STALE", "EUS")`), append `, None` as the
sixth argument to each.

- [ ] **Step 4: Write a new failing test proving `true_origin_crs` round-trips (both `Some` and `None`)**

Add directly after `upsert_with_an_empty_batch_does_not_wipe_the_day` (around
line 2916):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_round_trips_true_origin_crs_including_a_null_value() {
        let pool = test_pool().await;
        let date = fixture_date(20);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "ZRD", time(8, 0), "C30001", "EUS", Some("PAD")),
                row(date, "ZRD", time(9, 0), "C30002", "CRE", None),
            ],
        )
        .await
        .expect("seed rows");

        let stored: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT train_uid, true_origin_crs FROM schedule_destination_departures \
             WHERE service_date = $1 ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0], ("C30001".to_string(), Some("PAD".to_string())));
        assert_eq!(
            stored[1],
            ("C30002".to_string(), None),
            "an absent true_origin_crs must round-trip as SQL NULL, not an empty string"
        );

        delete_day(&pool, date).await;
    }
```

- [ ] **Step 5: Update the ingest route's own db_test fixture**

In `crates/api/src/routes/ingest.rs`, in
`post_schedule_destination_departures_upserts_the_rows` (around line
1240-1309), the POST body and the readback both need the new field. Replace
the `body` value with:

```rust
        let body = serde_json::json!([
            {
                "service_date": "2099-02-01",
                "destination_crs": "ZRB",
                "scheduled": "08:22:00",
                "train_uid": "C10001",
                "origin_crs": "EUS",
                "true_origin_crs": "PAD"
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

And extend the readback query/assertions:

```rust
        let stored: Vec<(String, chrono::NaiveTime, String, String, Option<String>)> = sqlx::query_as(
            "SELECT destination_crs, scheduled, train_uid, origin_crs, true_origin_crs \
             FROM schedule_destination_departures \
             WHERE service_date = '2099-02-01' \
             ORDER BY scheduled",
        )
        .fetch_all(&pool)
        .await
        .expect("read back the upserted rows");

        assert_eq!(stored.len(), 2, "one stored row per posted departure");
        assert_eq!(stored[0].0, "ZRB");
        assert_eq!(
            stored[0].1,
            chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap()
        );
        assert_eq!(stored[0].2, "C10001");
        assert_eq!(stored[0].3, "EUS");
        assert_eq!(stored[0].4, Some("PAD".to_string()));
        assert_eq!(stored[1].2, "C10002");
        assert_eq!(stored[1].3, "CRE");
        assert_eq!(stored[1].4, None);
```

- [ ] **Step 6: Run all the affected tests**

Run: `cargo build -p api` (compile check first — no `query!` macros are used
so this never needs a live database).
Expected: builds clean.

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api schedule_destination_departures -- --ignored --test-threads=1
```
Expected: all pass, including the new `upsert_round_trips_true_origin_crs_including_a_null_value`.

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api post_schedule_destination_departures -- --ignored --test-threads=1
```
Expected: passes with the updated fixture.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs
git commit -m "Plumb true_origin_crs through ScheduleDestinationDeparturesRow and its upsert"
```

---

## Task 4: `crates/api` data layer — replace destination-first search with calling-point-first search

**Files:**
- Modify: `crates/api/src/data/queries.rs:924-1189` (removes
  `DestinationDepartureCursor`, `DestinationDeparturePage`,
  `search_schedule_destination_departures`; adds
  `CallingPointDepartureCursor`, `CallingPointDeparturePage`,
  `search_schedule_calling_point_departures`)
- Modify: `crates/api/src/data/queries.rs` (test module: removes the
  `search_*` tests for the old function around lines 2918-3336, adds
  equivalents for the new one)

**Interfaces:**
- Consumes: Task 1's index, Task 3's `true_origin_crs` column and its
  `row()` test helper/`fixture_rows()`.
- Produces:
  ```rust
  pub struct CallingPointDepartureCursor {
      pub scheduled: chrono::NaiveTime,
      pub train_uid: String,
  }
  pub struct CallingPointDeparturePage {
      pub departures: Vec<serde_json::Value>,
      pub next_cursor: Option<CallingPointDepartureCursor>,
  }
  pub async fn search_schedule_calling_point_departures(
      pool: &PgPool,
      station_crs: &str,
      service_date: chrono::NaiveDate,
      scheduled_from: chrono::NaiveTime,
      true_origin_crs: Option<&str>,
      destination_crs: Option<&str>,
      to_time: Option<chrono::NaiveTime>,
      after: Option<&CallingPointDepartureCursor>,
      limit: i64,
  ) -> Result<Option<CallingPointDeparturePage>>
  ```
  Task 5 (render.rs) and Task 6 (routes/trains.rs) consume this directly.
  Element shape of `departures`: `{"uid", "destination_crs", "true_origin_crs", "scheduled": "HH:MM:SS"}`.

- [ ] **Step 1: Delete the old cursor/page types and search function**

In `crates/api/src/data/queries.rs`, three separate deletions, precisely
scoped (do NOT delete anything between them — that range holds
`upsert_schedule_destination_departures`, which Task 3 already modified,
and `schedule_destination_departures_published_for`, both of which stay
exactly as they are):

1. Delete the `DestinationDepartureCursor` doc comment and struct
   definition in full (the block starting `/// An opaque-to-the-caller
   position in one destination's ordered results...` through the closing
   `}` of `pub struct DestinationDepartureCursor { ... }`).
2. Delete the `DestinationDeparturePage` doc comment and struct definition
   in full (the block starting `/// One page of destination-search
   results...` through its closing `}`), which sits directly after #1.
3. Leave `upsert_schedule_destination_departures` and
   `schedule_destination_departures_published_for` (the day-scoped
   existence probe, doc comment starts "Cheap, day-scoped existence
   probe...") completely untouched — Step 2 below reuses the probe
   unchanged, and the upsert function is Task 3's work.
4. Delete the entire `search_schedule_destination_departures` function,
   including its full doc comment (the block starting `/// The
   destination-first train search's one read...` through the closing `}`
   of the function body) — this is the last item in the file before the
   `upsert_full_coverage_line_stats` function.

Run `grep -n "DestinationDepartureCursor\|DestinationDeparturePage\|pub async fn search_schedule_destination_departures\|pub async fn upsert_schedule_destination_departures\|async fn schedule_destination_departures_published_for" crates/api/src/data/queries.rs`
first to confirm the exact current line numbers before editing (Task 3 will
have shifted them slightly from this plan's own line numbers).

- [ ] **Step 2: Write the new types and function**

Insert in the same location:

```rust
/// An opaque-to-the-caller position in one station's ordered results: the
/// last row of the page just returned. The next page is everything
/// strictly after it under `ORDER BY scheduled, train_uid`.
///
/// Two components, not three like the destination-keyed predecessor this
/// replaces: `origin_crs` (the calling point / station being searched) is
/// now the FIXED equality filter for the whole query, constant across
/// every row of one response, so it carries no ordering information and
/// would be a redundant cursor component. `train_uid` alone is a
/// sufficient tiebreaker on `scheduled` because a schedule's `train_uid`
/// is unique per `(service_date, origin_crs)` under normal CIF data (see
/// the calling-point-search design doc's Open Question 2 for the one
/// theoretical exception this doesn't try to rule out).
///
/// `routes::trains` encodes this onto the wire and parses it back; nothing
/// outside that module should construct one from user input without going
/// through that parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallingPointDepartureCursor {
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
}

/// One page of calling-point-search results.
///
/// `departures` elements are `serde_json::Value` in the
/// `{"uid", "destination_crs", "true_origin_crs", "scheduled": "HH:MM:SS"}`
/// shape -- the exact element shape `crate::render::calling_point_departure_json`
/// reads. `next_cursor` is `Some` only when there is genuinely at least one
/// more row (the query fetches `limit + 1` to know that).
#[derive(Debug, Clone)]
pub struct CallingPointDeparturePage {
    pub departures: Vec<serde_json::Value>,
    pub next_cursor: Option<CallingPointDepartureCursor>,
}

/// The calling-point-first train search's one read: a bounded index range
/// scan over `schedule_destination_departures_calling_point_idx`, with a
/// keyset cursor. Replaces `search_schedule_destination_departures`
/// (destination-first) in place -- see
/// docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
///
/// `station_crs` is REQUIRED and matches the `origin_crs` column -- already
/// "the calling point of this row" (`schedule_query::DestinationDeparture`'s
/// own doc comment), so no data-model change was needed for the primary
/// key, only a new leading index. `true_origin_crs` and `destination_crs`
/// are BOTH optional filters layered on top, independent of each other and
/// of `station_crs`.
///
/// `scheduled_from` is an INCLUSIVE lower bound, the caller's already-
/// combined `max(now, from)`. `to_time` is an INCLUSIVE upper bound. Both
/// carry the exact same reasoning as the predecessor query.
///
/// `Ok(None)` means no CIF publish has landed for `service_date` at all
/// (maps to a 404). `Ok(Some(page))` with an empty `page.departures` means
/// the day IS published and the filters matched nothing (a 200 with an
/// empty `results` array). Reuses
/// `schedule_destination_departures_published_for` unchanged -- that probe
/// was already day-scoped, not destination-scoped, so it needs no change
/// for the new leading column.
#[allow(clippy::too_many_arguments)]
pub async fn search_schedule_calling_point_departures(
    pool: &PgPool,
    station_crs: &str,
    service_date: chrono::NaiveDate,
    scheduled_from: chrono::NaiveTime,
    true_origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    to_time: Option<chrono::NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>> {
    let fetch = limit.saturating_add(1);

    let rows: Vec<(String, String, Option<String>, chrono::NaiveTime)> = sqlx::query_as(
        r#"
        SELECT train_uid, destination_crs, true_origin_crs, scheduled
        FROM schedule_destination_departures
        WHERE service_date = $1
          AND origin_crs = $2
          AND scheduled >= $3
          AND ($4::text IS NULL OR true_origin_crs = $4)
          AND ($5::text IS NULL OR destination_crs = $5)
          AND ($6::time IS NULL OR scheduled <= $6)
          AND ($7::time IS NULL
               OR (scheduled, train_uid) > ($7, $8))
        ORDER BY scheduled, train_uid
        LIMIT $9
        "#,
    )
    .bind(service_date)
    .bind(station_crs)
    .bind(scheduled_from)
    .bind(true_origin_crs)
    .bind(destination_crs)
    .bind(to_time)
    .bind(after.map(|c| c.scheduled))
    .bind(after.map(|c| c.train_uid.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        if !schedule_destination_departures_published_for(pool, service_date).await? {
            return Ok(None);
        }
        return Ok(Some(CallingPointDeparturePage {
            departures: Vec::new(),
            next_cursor: None,
        }));
    }

    let has_more = rows.len() as i64 > limit;
    let page_rows = if has_more { &rows[..limit as usize] } else { &rows[..] };

    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|(train_uid, _, _, scheduled)| CallingPointDepartureCursor {
                scheduled: *scheduled,
                train_uid: train_uid.clone(),
            })
    } else {
        None
    };

    let departures = page_rows
        .iter()
        .map(|(train_uid, destination_crs, true_origin_crs, scheduled)| {
            serde_json::json!({
                "uid": train_uid,
                "destination_crs": destination_crs,
                "true_origin_crs": true_origin_crs,
                "scheduled": scheduled.format("%H:%M:%S").to_string(),
            })
        })
        .collect();

    Ok(Some(CallingPointDeparturePage {
        departures,
        next_cursor,
    }))
}
```

- [ ] **Step 3: Delete the old function's tests and write new ones**

Delete every test from `search_with_nothing_published_for_the_day_is_none_not_an_empty_page`
through `search_cursor_breaks_ties_on_train_uid_then_origin_crs` (the whole
block currently at lines 2918-3336) — all of them reference
`search_schedule_destination_departures`/`DestinationDepartureCursor`, both
now deleted.

Replace with (using `fixture_rows`/`seed` from Task 3, which already seed
three rows all with `origin_crs` values `EUS`/`CRE`/`EUS` under destination
`ZRD` — for the NEW station-first shape we need a fixture where multiple
rows share the SAME `origin_crs` (the new required key) with DIFFERING
`destination_crs`/`true_origin_crs`, so add a second fixture builder rather
than reusing `fixture_rows` for these):

```rust
    /// Three trains all callable-at "RDG" (the new required search key),
    /// to two different destinations, from two different true origins --
    /// exactly what's needed to prove `origin`/`destination` are
    /// independent optional filters layered on a FIXED station, not the
    /// primary key.
    fn calling_point_fixture_rows(
        service_date: chrono::NaiveDate,
    ) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(service_date, "WAT", time(8, 22), "C40001", "RDG", Some("PAD")),
            row(service_date, "WAT", time(10, 5), "C40002", "RDG", Some("SWA")),
            row(service_date, "BRI", time(18, 40), "C40003", "RDG", Some("PAD")),
        ]
    }

    async fn seed_calling_point(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &calling_point_fixture_rows(service_date))
            .await
            .expect("seed calling-point fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_nothing_published_for_the_day_is_none() {
        let pool = test_pool().await;
        let date = fixture_date(21);
        delete_day(&pool, date).await;

        let result = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, None, None, None, 100,
        )
        .await
        .expect("search");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_no_filters_returns_every_row_for_that_station() {
        let pool = test_pool().await;
        let date = fixture_date(22);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, None, None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 3);
        assert_eq!(
            page.departures[0],
            serde_json::json!({
                "uid": "C40001",
                "destination_crs": "WAT",
                "true_origin_crs": "PAD",
                "scheduled": "08:22:00",
            }),
            "element shape is exactly what render::calling_point_departure_json reads"
        );
        assert_eq!(page.departures[1]["uid"], "C40002");
        assert_eq!(page.departures[2]["uid"], "C40003");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_only_returns_rows_for_the_requested_station() {
        let pool = test_pool().await;
        let date = fixture_date(23);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(8, 0), "C50001", "RDG", None),
                row(date, "WAT", time(8, 5), "C50002", "SLO", None),
            ],
        )
        .await
        .expect("seed two-station fixture");

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, None, None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C50001");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_true_origin_independent_of_destination() {
        let pool = test_pool().await;
        let date = fixture_date(24);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), Some("PAD"), None, None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let uids: Vec<&str> = page.departures.iter().map(|d| d["uid"].as_str().unwrap()).collect();
        assert_eq!(
            uids,
            vec!["C40001", "C40003"],
            "PAD-origin filter matches trains to TWO different destinations"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_destination_independent_of_true_origin() {
        let pool = test_pool().await;
        let date = fixture_date(25);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, Some("WAT"), None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let uids: Vec<&str> = page.departures.iter().map(|d| d["uid"].as_str().unwrap()).collect();
        assert_eq!(
            uids,
            vec!["C40001", "C40002"],
            "WAT-destination filter matches trains from TWO different true origins"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_combines_both_optional_filters() {
        let pool = test_pool().await;
        let date = fixture_date(26);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), Some("PAD"), Some("WAT"), None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C40001");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_is_scoped_to_the_requested_service_date_only() {
        // Ported from the deleted search_is_scoped_to_the_requested_service_date_only
        // (destination-first predecessor) -- this coverage must not be lost
        // just because the old test block was deleted wholesale. Proves the
        // "always today, server-side" scoping: a stale day's rows must
        // never leak through, and must not even make the existence probe
        // say "published".
        let pool = test_pool().await;
        let date = fixture_date(29);
        let yesterday = date - chrono::Duration::days(1);
        delete_day(&pool, date).await;
        delete_day(&pool, yesterday).await;

        upsert_schedule_destination_departures(
            &pool,
            &[row(yesterday, "WAT", time(8, 0), "STALE", "RDG", None)],
        )
        .await
        .expect("seed a stale day");

        let result = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, None, None, None, 100,
        )
        .await
        .expect("search");
        assert!(
            result.is_none(),
            "yesterday's rows must not answer today's query, nor satisfy today's existence probe"
        );

        delete_day(&pool, yesterday).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_an_inclusive_time_range_and_excludes_before_now() {
        let pool = test_pool().await;
        let date = fixture_date(27);
        seed_calling_point(&pool, date).await;

        // Lower bound 10:05 is inclusive and matches exactly; upper bound
        // 12:00 excludes the 18:40 row.
        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, time(10, 5), None, None, Some(time(12, 0)), None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C40002");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_keyset_cursor_pages_without_gaps_or_repeats_and_breaks_ties_on_train_uid()
    {
        // Two rows share one `scheduled` (09:00) at the SAME station, to
        // prove train_uid alone is a sufficient tiebreaker now that
        // origin_crs is fixed per query, not part of the ordering.
        let pool = test_pool().await;
        let date = fixture_date(28);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(9, 0), "C60002", "RDG", None),
                row(date, "WAT", time(9, 0), "C60001", "RDG", None),
                row(date, "BRI", time(11, 0), "C60003", "RDG", None),
            ],
        )
        .await
        .expect("seed tied-time fixture");

        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<CallingPointDepartureCursor> = None;
        for _ in 0..5 {
            let page = search_schedule_calling_point_departures(
                &pool, "RDG", date, any_time(), None, None, None, cursor.as_ref(), 1,
            )
            .await
            .expect("search")
            .expect("the day is published");
            for departure in &page.departures {
                seen.push(departure["uid"].as_str().unwrap().to_string());
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(
            seen,
            vec!["C60001", "C60002", "C60003"],
            "the 09:00 tie is broken by train_uid, and every row appears exactly once"
        );
        assert!(cursor.is_none(), "the last page must not hand back a cursor");

        delete_day(&pool, date).await;
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo build -p api`
Expected: builds clean (confirms `search_schedule_destination_departures`
and its old cursor/page types are gone from every call site — Task 6 hasn't
run yet, so `routes/trains.rs` will still fail to compile at this point;
that's expected and gets fixed in Task 6, so run `cargo build -p api
--lib --tests -- --no-run` is NOT expected to fully succeed until Task 6 —
skip this compile check here and rely on Task 6's own build step instead).

Run just the new tests directly against the queries module instead:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api --lib search_calling_point -- --ignored --test-threads=1
```
This will fail to compile until `routes/trains.rs` (Task 6) is also fixed,
because `queries.rs` is compiled as part of the same crate as
`routes/trains.rs`. **Given that dependency, do Step 4 as a read-through
correctness check of the SQL and Rust here, and defer the actual `cargo
test` run to Task 6's Step, which exercises this task's code together with
the fixed route.** Note this explicitly in the commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Replace destination-first search with calling-point-first search in the data layer"
```

---

## Task 5: `crates/api/src/render.rs` — `calling_point_departure_json`

**Files:**
- Modify: `crates/api/src/render.rs:183-211` (replaces `destination_departure_json`)
- Test: `crates/api/src/render.rs` (`#[cfg(test)] mod tests` — locate and
  replace the existing tests for `destination_departure_json`, identifiable
  by name/content around the assertions on `destinationCrs`/`originCrs`
  quoted in this plan's research, e.g. near lines 560-670 based on this
  plan's own earlier reading of the file — confirm exact line numbers with
  `grep -n "destination_departure_json" crates/api/src/render.rs` before
  editing, since Task 4 did not touch this file and line numbers here are
  unchanged from before this plan)

**Interfaces:**
- Consumes: the `{"uid", "destination_crs", "true_origin_crs", "scheduled"}`
  element shape Task 4's `search_schedule_calling_point_departures` returns.
- Produces: `pub(crate) fn calling_point_departure_json(d: &Value, station_crs: &str) -> Value`,
  consumed by Task 6's route handler.

- [ ] **Step 1: Find the exact current location**

Run: `grep -n "destination_departure_json" crates/api/src/render.rs`
Note the line numbers of the function and its tests.

- [ ] **Step 2: Write the failing test**

Add this test to the `#[cfg(test)] mod tests` block in
`crates/api/src/render.rs` (near wherever the old
`destination_departure_json` tests are — put the new ones directly in their
place, per Step 4 below):

```rust
    #[test]
    fn calling_point_departure_json_renders_camel_case_with_station_attached_and_time_trimmed() {
        let row = serde_json::json!({
            "uid": "C10001",
            "destination_crs": "WAT",
            "true_origin_crs": "PAD",
            "scheduled": "08:22:00",
        });
        let json = calling_point_departure_json(&row, "RDG");
        assert_eq!(json["uid"], "C10001");
        assert_eq!(json["scheduled"], "08:22");
        assert_eq!(json["stationCrs"], "RDG");
        assert_eq!(json["originCrs"], "PAD");
        assert_eq!(json["destinationCrs"], "WAT");
    }

    #[test]
    fn calling_point_departure_json_renders_a_null_origin_as_json_null_not_a_missing_key() {
        let row = serde_json::json!({
            "uid": "C10002",
            "destination_crs": "WAT",
            "true_origin_crs": null,
            "scheduled": "10:05:00",
        });
        let json = calling_point_departure_json(&row, "RDG");
        assert!(json["originCrs"].is_null());
        assert!(json.get("originCrs").is_some(), "must be explicit null, not omitted");
    }
```

- [ ] **Step 3: Run the test to verify it fails to compile**

Run: `cargo test -p api calling_point_departure_json`
Expected: compile error — no such function yet.

- [ ] **Step 4: Replace `destination_departure_json` with `calling_point_departure_json`**

Replace the whole `destination_departure_json` function (found in Step 1)
with:

```rust
/// One `GET /public/trains/search` result row. Sibling of
/// `schedule_departure_json` above, same "hand-built camelCase over an
/// opaque JSONB element" convention. Replaces `destination_departure_json`
/// (destination-first search) in place -- see
/// docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
///
/// * `stationCrs` is the caller-supplied, normalized required search
///   parameter (the calling point being searched), echoed onto every row
///   the same way `destinationCrs` used to be under the old contract --
///   it's constant for the whole response, so it's attached here rather
///   than re-selected from every row.
/// * `originCrs` is read from `d`'s `true_origin_crs` field (nullable --
///   `null` when the schedule's own true origin TIPLOC never resolved).
///   This is a BREAKING rename in meaning from the old `originCrs`, which
///   used to mean "the calling point of this row" -- that role moves to
///   `stationCrs`. There is exactly one consumer of this route in this
///   repository (`TrainSearchForm.tsx`), updated in lockstep, so this is
///   not a compatibility concern.
/// * `destinationCrs` keeps its name and meaning (the schedule's true
///   destination) but is no longer caller-supplied and fixed -- it now
///   varies per row and is read out of `d`, the same shape switch
///   `schedule_departure_json` already uses for its own destination field.
///
/// `scheduled` is trimmed from the stored `"HH:MM:SS"` to `"HH:MM"`,
/// identical to `schedule_departure_json`.
pub(crate) fn calling_point_departure_json(d: &Value, station_crs: &str) -> Value {
    let scheduled = d
        .get("scheduled")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "stationCrs": station_crs,
        "originCrs": d.get("true_origin_crs").cloned().unwrap_or(Value::Null),
        "destinationCrs": d.get("destination_crs").cloned().unwrap_or(Value::Null),
    })
}
```

- [ ] **Step 5: Remove the old tests for `destination_departure_json`**

Delete every test in the `#[cfg(test)] mod tests` block that references
`destination_departure_json` (found in Step 1) — they test a function that
no longer exists.

- [ ] **Step 6: Confirm the new code is correct by inspection; defer running it**

Because Task 4 already removed `search_schedule_destination_departures`/
`DestinationDepartureCursor` from `queries.rs` while `routes/trains.rs`
still imports them (Task 6 hasn't run yet), the WHOLE `api` crate currently
fails to compile — including any filtered `cargo test -p api
calling_point_departure_json` invocation, since Cargo must compile every
file in the crate before running any test binary, filter or not. This is
expected and is not something to fix in this task.

Do NOT attempt to run `cargo test -p api calling_point_departure_json` and
report it as passing — it cannot pass yet. Instead, re-read the two new
tests from Step 2 against the `calling_point_departure_json` function from
Step 4 by hand and confirm the field names and JSON key lookups line up
exactly (`d.get("true_origin_crs")` for `originCrs`, `d.get("destination_crs")`
for `destinationCrs`, `station_crs` param for `stationCrs`). Record in your
report that this task's tests are verified by inspection here and will be
executed for real as part of Task 6's Step 3 full-crate test run — this is
a deliberate, plan-mandated deferral, not a skipped verification.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "Replace destination_departure_json with calling_point_departure_json"
```

---

## Task 6: `crates/api/src/routes/trains.rs` — the route itself

**Files:**
- Modify: `crates/api/src/routes/trains.rs` (whole file — production code
  lines 1-339, `db_tests` module lines 341-917)

**Interfaces:**
- Consumes: Task 4's `search_schedule_calling_point_departures`,
  `CallingPointDepartureCursor`; Task 5's `calling_point_departure_json`.
- Produces: the actual new `GET /public/trains/search?station=&origin=&destination=&from=&to=&limit=&after=`
  contract Task 8 (frontend) consumes: response envelope
  `{"results": [{"uid","scheduled","stationCrs","originCrs","destinationCrs"}], "nextCursor"}`.

This task's production-code and test changes are one coordinated contract
change across a single file (every test in this file exercises the same
param/response shape being replaced), so — unlike the smaller, additive
tasks above — this task is structured as one coherent write-then-verify
step rather than per-assertion red/green. That is a deliberate exception to
this plan's usual granularity, matching this plan's "Task Right-Sizing"
guidance: a reviewer would accept or reject this whole file's change
together, not one test at a time.

- [ ] **Step 1: Replace the whole file**

Replace the full contents of `crates/api/src/routes/trains.rs` with:

```rust
//! `GET /public/trains/search` -- calling-point-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page.
//! Generalizes the earlier destination-first search
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md) into a
//! calling-point-first one -- see
//! docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly
//! (`queries::search_schedule_calling_point_departures`) as a bounded index
//! range scan (over `schedule_destination_departures_calling_point_idx`)
//! with a keyset cursor. This is a publish-then-poll read of a table
//! `schedule-reference` writes when a CIF delivery lands -- never a
//! synchronous call into that service.
//!
//! **v1 filter set, and why it stops here.** `station` (any calling
//! point -- boarding or alighting) is required; `origin` (the schedule's
//! TRUE first calling point) and `destination` (the schedule's TRUE final
//! calling point) are both optional, independent filters, along with the
//! `from`/`to` time range. There is deliberately NO operator filter: the
//! CIF SCHEDULE feed's operator field is parsed-but-undecoded everywhere in
//! this codebase, so a CIF-derived row has no operator to filter on at all.
//! There is deliberately NO date parameter: like
//! `get_station_schedule_departures`, this is "always today, server-side".
//!
//! **This route owns the `now`-forward boundary**, which is the whole point
//! of the storage shape behind it. The publish stores the entire rail day
//! uncapped, because it fires once per CIF delivery -- roughly daily -- so
//! a publish-time filter would freeze at whatever the clock read when the
//! delivery landed. Evaluating `now` here means a search at 18:00 is
//! correct at 18:00.
//!
//! **Pagination is a keyset cursor, not an offset.** `limit` bounds one
//! page; `after` carries the last row of the previous page.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::CallingPointDepartureCursor;
use crate::render::calling_point_departure_json;

/// Page size when the caller does not ask for one.
const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected.
///
/// This is an unauthenticated, unmetered public route over a table with
/// ~377,000 rows per day. Without a ceiling, `?limit=1000000` is a free
/// full-table scan and a large response for any anonymous caller. 200 is
/// four default pages -- generous for any real client.
///
/// An over-large `limit` is clamped, not rejected -- the honest answer is
/// "here are 200, with a cursor for the rest." A `limit` that is zero,
/// negative or unparseable IS malformed and does 400.
const MAX_SEARCH_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on ANY station a
    /// train calls at -- boarding or alighting, including where it starts
    /// or ends -- not just where it departs from or terminates.
    station: String,
    /// Optional. Filters to schedules whose TRUE origin (their first
    /// calling point) is this CRS -- NOT "any calling point along the
    /// route", which is what `station` above already answers. See
    /// `schedule_query::DestinationDeparture`'s own doc comment for the
    /// `origin_crs`-vs-`true_origin_crs` distinction this filters on.
    origin: Option<String>,
    /// Optional. Filters to schedules whose TRUE destination (their final
    /// calling point) is this CRS.
    destination: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    /// Narrows the `now`-forward window; it can never widen it backwards.
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Parses a caller-supplied `"HH:MM"` into a real `NaiveTime`.
fn normalize_time(label: &str, raw: &str) -> Result<chrono::NaiveTime, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("{label} must be a time of day in HH:MM form"),
        )
    })
}

/// Validates and uppercases a CRS code.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter CRS code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Parses and bounds the page size.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let parsed: i64 = raw.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        )
    })?;
    if parsed < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        ));
    }
    Ok(parsed.min(MAX_SEARCH_LIMIT))
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"HH:MM:SS|train_uid"`. Two parts, not three: `origin_crs` (the station
/// being searched) is now the fixed equality filter for the whole query,
/// not a value that varies within one page, so it carries no ordering
/// information and doesn't belong in the cursor.
///
/// Base64 makes the value visibly OPAQUE, matching this crate's established
/// posture for other cursors in this codebase. Not signed: the cursor names
/// a public timetable row on an unauthenticated route.
fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently ignored -- ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row.
fn decode_cursor(raw: &str) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [scheduled, train_uid] = parts.as_slice() else {
        return Err(invalid());
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(CallingPointDepartureCursor {
        scheduled,
        train_uid: (*train_uid).to_string(),
    })
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let station = normalize_crs("station", &params.station)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    let destination = params
        .destination
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("destination", s))
        .transpose()?;
    let from_time = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("to", s))
        .transpose()?;
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // "Always today, server-side" -- no date parameter exists on this route
    // by design. `today` and `now` are deliberately read from ONE
    // `Utc::now().with_timezone(...)` call rather than two independent
    // `Utc::now()` calls -- see this same reasoning's original writeup in
    // this route's git history (baa4e75) for why two independent reads can
    // disagree about which calendar day it is around the UTC/London
    // midnight boundary during British Summer Time.
    let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
    let today = london_now.date_naive();
    let now = london_now.time();
    let scheduled_from = match from_time {
        Some(from) => std::cmp::max(now, from),
        None => now,
    };

    let Some(page) = queries::search_schedule_calling_point_departures(
        &app.database,
        &station,
        today,
        scheduled_from,
        origin.as_deref(),
        destination.as_deref(),
        to_time,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for today".to_string(),
        ));
    };

    Ok(Json(json!({
        "results": page
            .departures
            .iter()
            .map(|row| calling_point_departure_json(row, &station))
            .collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_today(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(chrono::Utc::now().date_naive())
            .execute(pool)
            .await
            .expect("cleanup today's schedule_destination_departures rows");
    }

    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        use chrono::Timelike;

        let now = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .time();
        let now = chrono::NaiveTime::from_hms_opt(now.hour(), now.minute(), 0)
            .expect("valid time from valid hour/minute");
        let past = chrono::NaiveTime::MIN;
        let (soon, soon_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(30));
        let (later, later_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(60));
        assert!(
            soon_wrapped == 0 && later_wrapped == 0 && now > past,
            "these tests need at least an hour before midnight and a moment after it; \
             re-run outside 23:00-00:01 Europe/London"
        );
        (past, soon, later)
    }

    /// Seeds today with rows all sharing ONE `origin_crs` (`station_crs`,
    /// the new required search key) but varying `destination_crs` and
    /// `true_origin_crs`, so the two optional filters can be exercised
    /// independently of the fixed station. One already-departed row (which
    /// the route must hide), and two future rows.
    async fn seed_today(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (past, soon, later) = relative_times();
        for (scheduled, train_uid, destination_crs, true_origin_crs) in [
            (past, "C10000", "WAT", Some("PAD")),
            (soon, "C10001", "WAT", Some("PAD")),
            (later, "C10002", "BRI", Some("SWA")),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind(station_crs)
            .bind(true_origin_crs)
            .execute(pool)
            .await
            .expect("seed fixture row");
        }
    }

    async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn results(body: &str) -> Vec<Value> {
        let json: Value = serde_json::from_str(body).unwrap();
        assert!(
            json.is_object() && json.get("results").is_some() && json.get("nextCursor").is_some(),
            "the body is an envelope with exactly `results` and `nextCursor`: {json}"
        );
        json["results"].as_array().cloned().unwrap()
    }

    fn next_cursor(body: &str) -> Option<String> {
        let json: Value = serde_json::from_str(body).unwrap();
        json["nextCursor"].as_str().map(str::to_string)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_missing_station_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "station is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_station_is_a_400_not_a_404() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("station"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");

        let (status, _) = get(&pool, "/trains/search?station=ZRB&after=bm9uc2Vuc2U").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, _) = get(&pool, "/trains/search?station=ZRB&limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_SEARCH_LIMIT, never rejected"
        );
        assert_eq!(results(&body).len(), 2, "the fixture only has two future rows");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_nothing_published_for_today_is_a_404() {
        let pool = connect().await;
        delete_today(&pool).await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("today"),
            "the 404 is about today's publish, not about the station: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_unknown_station_on_a_published_day_is_200_and_empty() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRF").await;
        assert_eq!(status, StatusCode::OK);
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&origin=ZZZ").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "published-but-unmatched is a 200 with an empty results array, never a 404"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_station_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=zrb").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let (_, soon, _) = relative_times();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(
            rows[0]["scheduled"],
            soon.format("%H:%M").to_string(),
            "seconds trimmed"
        );
        assert_eq!(
            rows[0]["stationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert_eq!(rows[0]["originCrs"], "PAD");
        assert_eq!(rows[0]["destinationCrs"], "WAT");
        assert!(rows[0].get("origin_crs").is_none(), "no stray snake_case field");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        assert!(
            !uids.contains(&"C10000"),
            "an already-departed row must not be returned: {uids:?}"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_origin_destination_and_time_filters_together() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (_, soon, later) = relative_times();
        let uri = format!(
            "/trains/search?station=ZRB&origin=SWA&destination=BRI&from={}&to={}",
            soon.format("%H:%M"),
            later.format("%H:%M")
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C10002");
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_origin_and_destination_are_independent_of_each_other() {
        // The load-bearing new-behavior test: origin=PAD alone matches
        // BOTH remaining rows (both true-originate at PAD), even though
        // they go to different destinations -- proving `origin` doesn't
        // silently also constrain `destination`.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&origin=PAD").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1, "only C10001 is both future AND PAD-originated");
        assert_eq!(rows[0]["uid"], "C10001");
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&body).len(), 2);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, first) = get(&pool, "/trains/search?station=ZRB&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let first_rows = results(&first);
        assert_eq!(first_rows.len(), 1);
        assert_eq!(first_rows[0]["uid"], "C10001");
        let cursor = next_cursor(&first).expect("a second page exists, so a cursor is returned");

        let (status, second) = get(
            &pool,
            &format!("/trains/search?station=ZRB&limit=1&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_eq!(
            second_rows[0]["uid"], "C10002",
            "`after` must continue from the cursor, not restart at page 1"
        );
        assert!(
            next_cursor(&second).is_none(),
            "the last page must not hand back a cursor"
        );

        delete_today(&pool).await;
    }

    fn london_is_currently_ahead_of_utc() -> bool {
        use chrono::Offset;

        chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .offset()
            .fix()
            .local_minus_utc()
            != 0
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_inside_the_utc_vs_london_gap() {
        let pool = connect().await;
        delete_today(&pool).await;

        let is_bst = london_is_currently_ahead_of_utc();
        let today = chrono::Utc::now().date_naive();
        let london_time = {
            use chrono::Timelike;

            let t = chrono::Utc::now()
                .with_timezone(&chrono_tz::Europe::London)
                .time();
            chrono::NaiveTime::from_hms_opt(t.hour(), t.minute(), 0)
                .expect("valid time from valid hour/minute")
        };
        let (gap_time, wrapped) =
            london_time.overflowing_sub_signed(chrono::Duration::minutes(20));
        assert!(
            wrapped == 0 && london_time >= chrono::NaiveTime::from_hms_opt(0, 20, 0).unwrap(),
            "this test needs at least 20 minutes since London midnight; re-run outside \
             00:00-00:20 Europe/London"
        );

        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(today)
        .bind("WAT")
        .bind(gap_time)
        .bind("C10099")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed the gap-row fixture");

        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();

        if is_bst {
            assert!(
                !uids.contains(&"C10099".to_string()),
                "during BST, a row scheduled 20 minutes before the correct London-local `now` \
                 has already departed and must be excluded; if this fails, `now` has regressed \
                 to bare UTC time: {uids:?}"
            );
        } else {
            // Not currently observing BST: no gap to pin this fixture
            // inside; see this test's history for why skipping here is
            // honest rather than a coverage gap.
        }

        delete_today(&pool).await;
    }
}
```

- [ ] **Step 2: Build the whole `api` crate**

Run: `cargo build -p api`
Expected: builds clean. This is the first point since Task 4 where the
whole crate (queries.rs, render.rs, routes/trains.rs, routes/ingest.rs all
together) compiles again — confirms Tasks 3, 4, 5 and 6 are mutually
consistent.

- [ ] **Step 3: Run the full `api` test suite, including ignored DB tests**

Run: `cargo test -p api`
Expected: all non-`#[ignore]`d tests pass.

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
```
Expected: every `#[ignore]`d test in the crate passes, including all of
Task 3's, Task 4's, and this task's new/updated tests.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/trains.rs
git commit -m "Rewrite GET /public/trains/search as calling-point-first"
```

---

## Task 7: `crates/schedule-reference` — publish `true_origin_crs`

**Files:**
- Modify: `crates/schedule-reference/src/main.rs:376-397` (`schedule_destination_departures_rows`)
- Test: `crates/schedule-reference/src/main.rs` (`#[cfg(test)] mod tests`,
  around lines 711-848 per `grep -n "schedule_destination_departures_rows"`)

**Interfaces:**
- Consumes: Task 2's `DestinationDeparture.true_origin_crs`.
- Produces: each flattened JSON row gains a `"true_origin_crs"` key, which
  Task 3's `ScheduleDestinationDeparturesRow` (already deployed) deserializes.

- [ ] **Step 1: Find the exact current test content**

Run: `grep -n "schedule_destination_departures_rows" crates/schedule-reference/src/main.rs`

- [ ] **Step 2: Write/update the failing test**

Find the test
`schedule_destination_departures_rows_produces_one_flat_row_per_departure_carrying_its_destination`
and add an assertion that the new key is present and correctly sourced. Add
this new test directly after it:

```rust
    #[test]
    fn schedule_destination_departures_rows_includes_the_true_origin_crs_field() {
        let mut by_destination: std::collections::HashMap<String, Vec<schedule_query::DestinationDeparture>> =
            std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "C11052".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                    true_origin_crs: Some("EUS".to_string()),
                },
                schedule_query::DestinationDeparture {
                    uid: "C11052".to_string(),
                    origin_crs: "CRE".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(10, 5, 0).unwrap(),
                    true_origin_crs: None,
                },
            ],
        );
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 8).unwrap();

        let rows = schedule_destination_departures_rows(by_destination, today);

        let eus_row = rows
            .iter()
            .find(|r| r["origin_crs"] == "EUS")
            .expect("EUS row present");
        assert_eq!(eus_row["true_origin_crs"], "EUS");

        let cre_row = rows
            .iter()
            .find(|r| r["origin_crs"] == "CRE")
            .expect("CRE row present");
        assert!(
            cre_row["true_origin_crs"].is_null(),
            "a None true_origin_crs must serialize as JSON null, not be omitted"
        );
    }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p schedule-reference schedule_destination_departures_rows_includes_the_true_origin_crs_field`
Expected: FAIL — the emitted JSON has no `"true_origin_crs"` key yet.

- [ ] **Step 4: Add the field to the flatten function**

In `crates/schedule-reference/src/main.rs`, in
`schedule_destination_departures_rows` (lines 376-397), add
`"true_origin_crs": d.true_origin_crs` to the `json!` macro:

```rust
fn schedule_destination_departures_rows(
    mut by_destination: std::collections::HashMap<
        String,
        Vec<schedule_query::DestinationDeparture>,
    >,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_destination
        .drain()
        .flat_map(|(destination_crs, departures)| {
            departures.into_iter().map(move |d| {
                serde_json::json!({
                    "service_date": today,
                    "destination_crs": destination_crs,
                    "scheduled": d.scheduled,
                    "train_uid": d.uid,
                    "origin_crs": d.origin_crs,
                    "true_origin_crs": d.true_origin_crs,
                })
            })
        })
        .collect()
}
```

Also update this function's doc comment (directly above it) to note the
new field is included in the per-row byte budget (~10 more bytes per row
than previously documented, still comfortably inside the router's body
limit).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p schedule-reference`
Expected: all pass, including the new test and the pre-existing
`schedule_destination_departures_rows_*` tests (they don't assert on the
FULL JSON object shape exhaustively enough to break — confirm by running;
if any pre-existing test does an exact-equality assertion against a
`json!({...})` literal without the new field, add `"true_origin_crs":
null` or the appropriate value to that literal so it matches).

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-reference/src/main.rs
git commit -m "Publish true_origin_crs on each schedule-destination-departures row"
```

---

## Task 8: Frontend — calling-point search UI

**Files:**
- Modify: `frontend/components/TrainSearchForm.tsx` (whole file)
- Modify: `frontend/components/TrainSearchForm.test.tsx` (whole file)
- Modify: `frontend/app/trains/page.tsx` (whole file)

**Interfaces:**
- Consumes: Task 6's `GET /public/trains/search?station=&origin=&destination=&from=&to=&limit=&after=`
  contract and its `{results: [{uid, scheduled, stationCrs, originCrs, destinationCrs}], nextCursor}`
  response envelope.

- [ ] **Step 1: Write the failing tests**

Replace the full contents of `frontend/components/TrainSearchForm.test.tsx`
with:

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Builds a `GET /public/trains/search` response body. The route returns an
 * ENVELOPE, not a bare array: `results` plus a `nextCursor` that is an
 * explicit `null` on the last page. */
function searchBody(
  rows: Array<{ uid: string; scheduled: string; stationCrs: string; originCrs: string | null; destinationCrs: string | null }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({ results: rows, nextCursor });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
];
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', stationCrs: 'MAN', originCrs: 'EUS', destinationCrs: 'WAT' },
];
const PAGE_THREE = [
  { uid: 'C10004', scheduled: '13:15', stationCrs: 'MAN', originCrs: 'CRE', destinationCrs: 'WAT' },
];

function mockFetchByUrl(
  options: { search?: (url: string) => Response; track?: () => Response } = {},
) {
  const {
    search = () => new Response(searchBody(PAGE_ONE), { status: 200 }),
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/search')) return Promise.resolve(search(url));
    if (url.startsWith('/api/stations?')) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url))
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }));
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

function searchCallUrls(fetchMock: ReturnType<typeof vi.fn>): string[] {
  return fetchMock.mock.calls
    .map((args: unknown[]) => String(args[0]))
    .filter((url: string) => url.startsWith('/api/trains/search'));
}

function searchCallUrl(fetchMock: ReturnType<typeof vi.fn>): string {
  const urls = searchCallUrls(fetchMock);
  if (urls.length === 0) throw new Error('no /api/trains/search call recorded');
  return urls[0];
}

describe('TrainSearchForm', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('does not search until a valid station CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(
      screen.getByText('Enter a station above to search for trains that call there.'),
    ).toBeInTheDocument();
  });

  it('sends only the station when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" initialDestination="wat" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?station=MAN&origin=EUS&destination=WAT&from=09%3A00&to=12%3A00',
      ),
    );
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN → WAT')).toBeInTheDocument();
  });

  it('renders a "?" placeholder when origin or destination is unknown', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () =>
          new Response(
            searchBody([
              { uid: 'C99999', scheduled: '09:00', stationCrs: 'MAN', originCrs: null, destinationCrs: 'WAT' },
            ]),
            { status: 200 },
          ),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('09:00 · ? → MAN → WAT')).toBeInTheDocument();
  });

  it('links each row to the public train page for today', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const links = await screen.findAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a Track this train action on every row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    fireEvent.click(buttons[0]);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      ),
    );
  });

  it('distinguishes "nothing published for today" from "no matches"', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) }));
    renderWithMantine(<TrainSearchForm initialStation="ZZZ" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(
        /Today's scheduled timetable data isn't available yet/,
      ),
    ).toBeInTheDocument();
  });

  it('says so when the search succeeds but matches nothing', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText('No scheduled trains match those filters right now.'),
    ).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("Couldn't search for trains right now. Try again."),
    ).toBeInTheDocument();
  });

  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(/scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers the manual /track fallback, carrying any ticketId through', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm attachTicketId={7} />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  it('offers the manual /track fallback with no query string when there is no ticketId', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
  });

  it('renders no operator filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/Operator/i)).not.toBeInTheDocument();
  });

  it('renders no date filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/^Date/i)).not.toBeInTheDocument();
  });

  it('does not offer Load more when the response has no nextCursor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('offers Load more when the response carries a nextCursor', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () => new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
  });

  it('appends the next page rather than replacing the rows, and sends after=', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=CURSOR1')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();
    expect(
      screen.getByText('08:22 · EUS → MAN → WAT'),
      'page 1 must still be on screen -- Load more appends, it does not replace',
    ).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN → WAT')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(2);
    expect(urls[0]).toBe('/api/trains/search?station=MAN');
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1');

    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument(),
    );
  });

  it('uses the NEW cursor on a second Load more, not the first one again', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) => {
        if (url.includes('after=CURSOR2'))
          return new Response(searchBody(PAGE_THREE, null), { status: 200 });
        if (url.includes('after=CURSOR1'))
          return new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 });
        return new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 });
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('13:15 · CRE → MAN → WAT')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(3);
    expect(urls[1]).toBe('/api/trains/search?station=MAN&after=CURSOR1');
    expect(
      urls[2],
      'the second Load more must use the cursor from the SECOND response',
    ).toBe('/api/trains/search?station=MAN&after=CURSOR2');
    expect(screen.getAllByText('11:40 · EUS → MAN → WAT')).toHaveLength(1);
  });

  it('keeps the original filters on a Load more request', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(2));
    expect(searchCallUrls(fetchMock)[1]).toBe(
      '/api/trains/search?station=MAN&origin=EUS&from=09%3A00&to=12%3A00&after=CURSOR1',
    );
  });

  it('starts a fresh search over rather than appending to the previous one', async () => {
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN → WAT')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(screen.queryByText('11:40 · EUS → MAN → WAT')).not.toBeInTheDocument(),
    );
    expect(screen.getByText('08:22 · EUS → MAN → WAT')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run TrainSearchForm.test.tsx`
Expected: FAIL — `TrainSearchForm` doesn't accept `initialStation` yet and
still requires `destinationCrs`.

- [ ] **Step 3: Rewrite `TrainSearchForm.tsx`**

Replace the full contents of `frontend/components/TrainSearchForm.tsx`
with:

```tsx
'use client';

import { useState, type FormEvent } from 'react';
import { Alert, Autocomplete, Button, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::calling_point_departure_json`).
 * `stationCrs` is the required calling-point search key, echoed back on
 * every row. `originCrs`/`destinationCrs` are both nullable: they are the
 * schedule's TRUE origin/destination, independent optional filters, and
 * either can be unresolved for a real published schedule (see
 * `schedule_query::DestinationDeparture`'s own doc comment). Like every
 * CIF-derived row in this app it carries NO operator and NO live running
 * status. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  stationCrs: string;
  originCrs: string | null;
  destinationCrs: string | null;
}

/** The envelope `GET /public/trains/search` returns. Not a bare array: it
 * has to carry `nextCursor`, because the backend publishes and stores the
 * whole day uncapped and hands it back a page at a time. `nextCursor` is an
 * explicit `null` on the last page, never omitted. */
interface TrainSearchResponse {
  results: TrainSearchRow[];
  nextCursor: string | null;
}

/** Exactly one of five mutually-exclusive states, checked top to bottom by
 * `resultsContent` below. `'unpublished'` and an empty `rows` array are
 * genuinely different facts and get different copy -- the backend route
 * draws that 404-vs-empty-results distinction on purpose and collapsing it
 * here would waste it.
 *
 * `nextCursor` lives INSIDE the success variant rather than in its own
 * `useState`, so it cannot survive a state transition it does not belong
 * to: a fresh search, an error, or an unpublished response all discard it
 * automatically. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Calling-point-first, whole-network train search -- the `/trains` page's
 * one interactive component. Generalizes the earlier destination-first
 * search into "any station the train calls at" as the primary, required
 * key, with "departing from" (the schedule's TRUE origin) and "terminating
 * at" (the schedule's TRUE destination) as independent, optional filters
 * layered on top -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by calling point and
 * cannot see a train that isn't in it (a station missing from
 * `stanox_crs`, a same-day amendment landing after the last CIF delivery).
 * `/track`'s manual-entry form remains the honest fallback for exactly
 * those gaps, and this component links to it explicitly.
 *
 * Filter set, and why it stops here: Station is required (it is the
 * server-side search key). Origin, Destination and a From/To time range
 * are all optional. There is no Operator filter -- CIF rows carry no
 * operator field at all. There is no Date filter -- both of this app's
 * schedule sources are "today only, server-side". Both are explicit
 * non-goals, not omissions to fill in later.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  initialStation = '',
  initialOrigin = '',
  initialDestination = '',
  attachTicketId,
}: {
  initialStation?: string;
  initialOrigin?: string;
  initialDestination?: string;
  attachTicketId?: number;
}) {
  const [stationCrs, setStationCrs] = useState(initialStation);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [fromTime, setFromTime] = useState('');
  const [toTime, setToTime] = useState('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  // Separate from `searching` on purpose: a "Load more" in flight must not
  // blank the rows already on screen the way `resultsContent`'s
  // `searching` branch does, and must not re-disable the Search button.
  const [loadingMore, setLoadingMore] = useState(false);

  const { suggestions: stationSuggestions } = useSuggestions(stationCrs, searchStations);
  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);

  const stationValid = CRS_PATTERN.test(stationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const destinationValid = destinationCrs.trim() === '' || CRS_PATTERN.test(destinationCrs.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const canSearch =
    stationValid && originValid && destinationValid && fromValid && toValid && !searching;

  // Computed once per render rather than once per row: every result links
  // to the same calendar date, because this search is always "today"
  // (server-side).
  const today = dayjs().format('YYYY-MM-DD');

  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
    if (destinationCrs.trim()) params.set('destination', destinationCrs.trim().toUpperCase());
    if (fromTime.trim()) params.set('from', fromTime.trim());
    if (toTime.trim()) params.set('to', toTime.trim());
    return params;
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSearch) return;
    setSearching(true);
    try {
      const response = await fetch(`/api/trains/search?${searchParams().toString()}`);
      if (response.status === 404) {
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleLoadMore() {
    if (results === null || results === 'error' || results === 'unpublished') return;
    if (results.nextCursor === null || loadingMore) return;

    setLoadingMore(true);
    try {
      const params = searchParams();
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/trains/search?${params.toString()}`);
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' && current !== 'unpublished'
            ? { rows: current.rows, nextCursor: null }
            : current,
        );
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: current.rows, nextCursor: null }
          : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }

  function resultsContent() {
    if (!stationValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter a station above to search for trains that call there.
        </Text>
      );
    }
    if (searching) {
      return (
        <Text size="sm" c="dimmed">
          Searching…
        </Text>
      );
    }
    if (results === null) {
      return (
        <Text size="sm" c="dimmed">
          Press Search to find trains that call at this station.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search for trains right now. Try again.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          Today&apos;s scheduled timetable data isn&apos;t available yet — it may not have been
          published, or that station may not be one this feed covers.
        </Text>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled trains match those filters right now.
        </Text>
      );
    }
    return (
      <>
        <Text size="sm" c="dimmed">
          These are from the scheduled timetable, not live running information, and may be up to 30
          minutes out of date. Open a train to see its live status.
        </Text>
        <ScrollArea mah={420} offsetScrollbars>
          <Stack gap="xs">
            {results.rows.map((row) => (
              <Group key={`${row.uid}-${row.scheduled}`} justify="space-between" wrap="nowrap">
                <Text size="sm">
                  {row.scheduled} · {row.originCrs ?? '?'} → {row.stationCrs} → {row.destinationCrs ?? '?'}
                </Text>
                <Group gap="sm" wrap="nowrap">
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${today}`}>
                    View live status
                  </TextLink>
                  <TrackThisTrainButton
                    uid={row.uid}
                    date={today}
                    attachTicketId={attachTicketId}
                    size="xs"
                  />
                </Group>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
        {results.nextCursor !== null && (
          <Group>
            <Button
              variant="default"
              size="xs"
              onClick={handleLoadMore}
              disabled={loadingMore}
              loading={loadingMore}
            >
              Load more
            </Button>
          </Group>
        )}
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Station"
        placeholder="e.g. Reading or RDG"
        description="Any station this train calls at, including where it starts or ends."
        value={stationCrs}
        onChange={setStationCrs}
        data={stationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = stationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={stationCrs.length > 0 && !stationValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Paddington or PAD"
        description="Where the journey actually begins."
        value={originCrs}
        onChange={setOriginCrs}
        data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Autocomplete
        label="Terminating at (optional)"
        placeholder="e.g. Manchester or MAN"
        description="Where the journey ends."
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Group grow align="flex-start">
        <TextInput
          label="From (optional)"
          placeholder="09:00"
          value={fromTime}
          onChange={(event) => setFromTime(event.currentTarget.value)}
          error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
        />
        <TextInput
          label="To (optional)"
          placeholder="12:00"
          value={toTime}
          onChange={(event) => setToTime(event.currentTarget.value)}
          error={toTime.length > 0 && !toValid ? 'Must be a time like 12:00' : null}
        />
      </Group>
      <Group>
        <Button type="submit" disabled={!canSearch}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
      <Text size="sm" c="dimmed" component="div">
        Can&apos;t find your train? <TextLink href={manualHref}>Track it manually</TextLink> by
        entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the component tests**

Run: `cd frontend && npx vitest run TrainSearchForm.test.tsx`
Expected: all pass.

- [ ] **Step 5: Update `frontend/app/trains/page.tsx`**

Replace its full contents with:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** `/trains` -- the primary train-discovery surface. Generalized from a
 * destination-first search into a calling-point-first one -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close.
 *
 * Query params mirror `/track`'s own convention: `?station=` pre-fills the
 * required search key, `?origin=`/`?destination=` pre-fill the optional
 * filters (so a filtered search is a shareable link), and `?ticketId=`
 * carries a standalone ticket through so the row-level "Track this train"
 * action can attach it. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    station?: string | string[];
    origin?: string | string[];
    destination?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { station, origin, destination, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param -- fall back
  // to the first value rather than letting `.toUpperCase()` throw on an
  // array. Same handling as `app/track/page.tsx:10-13`.
  const stationParam = Array.isArray(station) ? station[0] : station;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Find a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for — it'll be attached automatically once you track it."
          : 'Search the whole network by any station a train calls at. Open any result for its live status, or track it to get updates.'}
      </Text>
      <TrainSearchForm
        initialStation={stationParam?.toUpperCase()}
        initialOrigin={originParam?.toUpperCase()}
        initialDestination={destinationParam?.toUpperCase()}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
```

- [ ] **Step 6: Run the whole frontend test suite and the build**

Run: `cd frontend && npx vitest run`
Expected: all pass.

Run: `cd frontend && npm run build` (or `pnpm build`/`yarn build` — check
`frontend/package.json`'s `scripts` for the exact command this repo uses)
Expected: builds clean, no TypeScript errors.

- [ ] **Step 7: Commit**

```bash
git add frontend/components/TrainSearchForm.tsx frontend/components/TrainSearchForm.test.tsx frontend/app/trains/page.tsx
git commit -m "Generalize TrainSearchForm to calling-point-first search"
```

---

## Task 9: Whole-workspace verification

**Files:** none (verification only)

- [ ] **Step 1: Build the whole Rust workspace**

Run: `cargo build --workspace`
Expected: clean build, no warnings treated as errors beyond whatever this
repo's existing baseline already has.

- [ ] **Step 2: Run the whole Rust workspace test suite (non-ignored)**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 3: Run every ignored DB test touched by this plan**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
```
Expected: all pass.

- [ ] **Step 4: Run the frontend test suite and build**

Run: `cd frontend && npx vitest run`
Run: `cd frontend && npm run build`
Expected: both clean.

- [ ] **Step 5: Grep for any leftover reference to the old contract**

Run:
```
grep -rn "search_schedule_destination_departures\|DestinationDepartureCursor\|DestinationDeparturePage\|destination_departure_json" crates/ frontend/ --include="*.rs" --include="*.tsx" --include="*.ts"
```
Expected: no matches — everything was replaced, nothing left half-migrated.

Run:
```
grep -rn "initialDestination=\"MAN\"\|?destination=.*required\|Enter a destination station" frontend/
```
Expected: no matches in production code (test/doc mentions of the OLD
required-destination copy string would indicate an incomplete rename).

- [ ] **Step 6: No commit for this task** — it is a verification pass only.
  If any step fails, fix the specific regression it surfaces (in the file/
  task where it belongs) and re-run from Step 1.

---

## Post-plan: spec conformance self-check

Every section of `docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`
maps to a task:

- §0 (confirmed data model) → grounds Tasks 1-4, no code of its own.
- §2 (schema change) → Task 1.
- §3 (`schedule-query` changes) → Task 2.
- §4 (`crates/api` data layer) → Tasks 3 and 4.
- §5 (`render.rs`) → Task 5.
- §6 (`routes/trains.rs`) → Task 6.
- §7 (`schedule-reference`) → Task 7.
- §8 (frontend) → Task 8.
- §9/§10 (out of scope / open questions) → deliberately not built; Task 9's
  Step 5 greps confirm nothing from the old contract was left half-migrated.
