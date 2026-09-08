# Journey Timetable Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the train detail page (`components/TrainJourney.tsx`) show
the full scheduled calling-point timetable as its primary, always-shown
structure once a train's `train_uid` is known, with live actual-time/delay
data overlaid per stop as it arrives — replacing today's single
"last reported location + one delay figure" summary.

**Architecture:** A new backend module (`crates/api/src/data/journey.rs`)
merges two possible scheduled-timetable sources (`trains.calling_points`
when present, else a reconstruction from `schedule_destination_departures`)
with the latest `train_movement_events` row per location, into one
`JourneyStop[]` array attached to both existing read responses
(`TrackedTrainState`, `PublicTrainState`) as a new `journeyStops` field.
The frontend renders that array via a new `JourneyTimeline` component,
moved to the top level of `TrainJourney.tsx` so it renders across every
resolution/journey status that has a `train_uid`, not just `en_route`.

**Tech Stack:** Rust (axum, sqlx/Postgres), Next.js App Router +
TypeScript + Mantine v9, Vitest.

**Spec:** `docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md`

## Global Constraints

- Every scheduled time reaching the wire for `JourneyStop` is a real UTC
  instant (`DateTime<Utc>` / RFC3339), converted via the existing
  `crate::data::eta_blend::london_to_utc` helper — never a bare
  `"HH:MM(:SS)"` local string. This is load-bearing: see the spec's §2 for
  the exact UTC/London bug class this avoids.
- `crs` matching between sources is always case-insensitive
  (`UPPER(...)` in SQL, `.to_uppercase()` in Rust), matching every existing
  CRS comparison in this codebase (`TRACKED_TRAIN_STATE_SELECT`,
  `crs_for_tiploc`, `list_stanox_crs_for_crs`).
- No new ETA mechanism. Only the existing `etaNext`/`etaSource` (rendered
  by the existing `EtaBadge`) is used; no per-stop ETA is computed.
- DB-backed Rust tests are `#[ignore]`d and run with
  `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo
  test -p api -- --ignored --test-threads=1`, matching every other test in
  this crate.
- `journeyStops: null` is a normal, valid response value (not an error) —
  every consumer must handle it as "no timetable available for this train
  today," never assume non-null once `train_uid` is known.
- Additive only: no existing field, column, or route is removed or
  renamed. `scheduleCallingPoints`/`callingPoints` stay exactly as they are.

---

## Task 1: Backend queries — `crates/api/src/data/queries.rs`

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add four new functions near
  the existing `crs_for_tiploc`/`list_stanox_crs_for_crs`/
  `schedule_destination_departures` sections, `:762` and `:1058-1189`
  respectively, plus a new section near the end for the movement-events
  query)
- Test: same file's `#[cfg(test)] mod db_tests` (existing convention —
  every function in this file has its test colocated in this module)

**Interfaces:**
- Consumes: `sqlx::PgPool`; the existing `stanox_crs`,
  `schedule_destination_departures`, `train_movement_events`, `stations`
  tables (no schema change needed — every column this task reads already
  exists).
- Produces (used by Task 2):
  - `pub async fn crs_for_tiplocs_batch(pool: &PgPool, tiplocs: &[String]) -> anyhow::Result<std::collections::HashMap<String, String>>` — keys are `UPPER(tiploc)`.
  - `pub struct CallingPointDepartureRow { pub origin_crs: String, pub scheduled: chrono::NaiveTime, pub true_origin_crs: Option<String>, pub destination_crs: Option<String> }`
  - `pub async fn list_calling_point_departures_for_train(pool: &PgPool, train_uid: &str, service_date: chrono::NaiveDate) -> anyhow::Result<Vec<CallingPointDepartureRow>>`
  - `pub struct MovementEventRow { pub loc_crs: String, pub event_type: Option<String>, pub planned_timestamp: Option<chrono::DateTime<chrono::Utc>>, pub actual_timestamp: Option<chrono::DateTime<chrono::Utc>>, pub variation_status: Option<String> }`
  - `pub async fn latest_movement_event_per_location(pool: &PgPool, trains_id: i64) -> anyhow::Result<Vec<MovementEventRow>>`
  - `pub async fn station_names_for_crs_batch(pool: &PgPool, crs_codes: &[String]) -> anyhow::Result<std::collections::HashMap<String, String>>` — keys are `UPPER(crs)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/api/src/data/queries.rs`'s `db_tests` module (find the
`use super::*;` + `connect()`/`test_pool()` helpers already in that
module and match their exact style — this file already has dozens of
near-identical fixtures, e.g. `crs_for_tiploc_resolves_a_known_tiploc_and_none_for_an_unknown_one`
at `:2715`):

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            crs_for_tiplocs_batch_resolves_every_known_tiploc_and_omits_unknown_ones \
            -- --ignored --test-threads=1`"]
async fn crs_for_tiplocs_batch_resolves_every_known_tiploc_and_omits_unknown_ones() {
    let pool = test_pool().await;
    upsert_stanox_crs(
        &pool,
        &[
            common::StanoxCrsRecord {
                stanox: "TEST-JS-CRE".to_string(),
                crs: "CRE".to_string(),
                tiploc: "TEST-JS-CREWE".to_string(),
                station_name: "CREWE".to_string(),
                source_sequence: 1,
            },
            common::StanoxCrsRecord {
                stanox: "TEST-JS-EUS".to_string(),
                crs: "EUS".to_string(),
                tiploc: "TEST-JS-EUSTON".to_string(),
                station_name: "EUSTON".to_string(),
                source_sequence: 1,
            },
        ],
    )
    .await
    .expect("seed stanox_crs");

    let result = crs_for_tiplocs_batch(
        &pool,
        &[
            "test-js-crewe".to_string(),
            "TEST-JS-EUSTON".to_string(),
            "TEST-JS-UNKNOWN".to_string(),
        ],
    )
    .await
    .expect("crs_for_tiplocs_batch");

    assert_eq!(result.get("TEST-JS-CREWE"), Some(&"CRE".to_string()));
    assert_eq!(result.get("TEST-JS-EUSTON"), Some(&"EUS".to_string()));
    assert_eq!(result.get("TEST-JS-UNKNOWN"), None);
    assert_eq!(result.len(), 2);

    sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JS-%'")
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            list_calling_point_departures_for_train_returns_rows_ordered_by_scheduled_time \
            -- --ignored --test-threads=1`"]
async fn list_calling_point_departures_for_train_returns_rows_ordered_by_scheduled_time() {
    let pool = test_pool().await;
    let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
    sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JS-CPD'")
        .execute(&pool)
        .await
        .ok();

    upsert_schedule_destination_departures(
        &pool,
        &[
            ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "10:15:00".parse().unwrap(),
                train_uid: "TEST-JS-CPD".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
            },
            ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "10:32:00".parse().unwrap(),
                train_uid: "TEST-JS-CPD".to_string(),
                origin_crs: "SLO".to_string(),
                true_origin_crs: Some("RDG".to_string()),
            },
        ],
    )
    .await
    .expect("seed schedule_destination_departures");

    let rows = list_calling_point_departures_for_train(&pool, "TEST-JS-CPD", service_date)
        .await
        .expect("list_calling_point_departures_for_train");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].origin_crs, "RDG");
    assert_eq!(rows[0].true_origin_crs.as_deref(), Some("RDG"));
    assert_eq!(rows[0].destination_crs.as_deref(), Some("WAT"));
    assert_eq!(rows[1].origin_crs, "SLO");

    sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JS-CPD'")
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            latest_movement_event_per_location_dedups_to_the_most_recently_received_event \
            -- --ignored --test-threads=1`"]
async fn latest_movement_event_per_location_dedups_to_the_most_recently_received_event() {
    let pool = test_pool().await;
    let trains_id = crate::data::trains::find_or_create_train(
        &pool,
        "TEST-JS-MOVE",
        "2026-09-08".parse().unwrap(),
    )
    .await
    .expect("find_or_create_train");

    sqlx::query(
        "INSERT INTO train_movement_events \
            (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
             actual_timestamp, variation_status, raw_body, received_at) \
         VALUES \
            ($1, 'k1', '0003', 'ARRIVAL', 'rdg', '2026-09-08T09:15:00Z', '2026-09-08T09:17:00Z', \
             'LATE', '{}'::jsonb, NOW() - interval '2 minutes'), \
            ($1, 'k2', '0003', 'DEPARTURE', 'RDG', '2026-09-08T09:20:00Z', '2026-09-08T09:23:00Z', \
             'LATE', '{}'::jsonb, NOW())",
    )
    .bind(trains_id)
    .execute(&pool)
    .await
    .expect("seed train_movement_events");

    let rows = latest_movement_event_per_location(&pool, trains_id)
        .await
        .expect("latest_movement_event_per_location");

    assert_eq!(rows.len(), 1, "one location, dedup to its latest event");
    assert_eq!(rows[0].loc_crs, "RDG");
    assert_eq!(rows[0].event_type.as_deref(), Some("DEPARTURE"));

    sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
        .bind(trains_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM trains WHERE id = $1")
        .bind(trains_id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            station_names_for_crs_batch_resolves_known_codes_and_omits_unknown_ones \
            -- --ignored --test-threads=1`"]
async fn station_names_for_crs_batch_resolves_known_codes_and_omits_unknown_ones() {
    let pool = test_pool().await;

    let names = station_names_for_crs_batch(&pool, &["KGX".to_string(), "ZZZ-UNKNOWN".to_string()])
        .await
        .expect("station_names_for_crs_batch");

    assert!(names.contains_key("KGX"), "KGX is seeded reference data");
    assert!(!names.contains_key("ZZZ-UNKNOWN"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api crs_for_tiplocs_batch_resolves -- --ignored --test-threads=1`
Expected: FAIL to compile — `crs_for_tiplocs_batch` etc. not defined. (Same
expected-failure shape for the other three new tests.)

- [ ] **Step 3: Implement the four functions**

Add near `crs_for_tiploc` (`:762`):

```rust
/// Batched sibling of `crs_for_tiploc` -- one `WHERE UPPER(tiploc) =
/// ANY($1)` query resolving every distinct TIPLOC in a calling-point list,
/// instead of one query per TIPLOC. Mirrors the existing single/batch
/// pairing convention `trains::find_or_create_train`/
/// `find_or_create_trains_batch` already establishes. Keys are
/// `UPPER(tiploc)`; a TIPLOC with no `stanox_crs` row is simply absent from
/// the map (degrade, don't fabricate -- same posture `crs_for_tiploc`
/// already has for a single lookup).
pub async fn crs_for_tiplocs_batch(
    pool: &PgPool,
    tiplocs: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    if tiplocs.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let upper: Vec<String> = tiplocs.iter().map(|t| t.to_uppercase()).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT UPPER(tiploc), UPPER(crs) FROM stanox_crs WHERE UPPER(tiploc) = ANY($1)",
    )
    .bind(&upper)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}
```

Add near the `schedule_destination_departures` section (`:1058`):

```rust
/// One `schedule_destination_departures` row for one train_uid/service_date,
/// used to reconstruct a scheduled stop list when `trains.calling_points`
/// hasn't been populated by schedule-matching (`crates/api/src/data/journey.rs`'s
/// fallback source -- see
/// docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §0.2).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CallingPointDepartureRow {
    pub origin_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub true_origin_crs: Option<String>,
    pub destination_crs: Option<String>,
}

/// Every departure-bearing calling point of `train_uid`'s schedule on
/// `service_date`, chronological. See `CallingPointDepartureRow`'s doc
/// comment for why this exists; see the design doc §0.2 for why the
/// schedule's own terminus is NOT among these rows (no `booked_departure`
/// for a `Terminate` calling point) -- the caller appends it separately.
pub async fn list_calling_point_departures_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> Result<Vec<CallingPointDepartureRow>> {
    let rows = sqlx::query_as::<_, CallingPointDepartureRow>(
        "SELECT origin_crs, scheduled, true_origin_crs, destination_crs \
         FROM schedule_destination_departures \
         WHERE train_uid = $1 AND service_date = $2 \
         ORDER BY scheduled",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

Add a new section at the end of the file (before the `#[cfg(test)]` module
starts, or inside an existing "movement events" heading if one exists —
grep the file first; if none exists, add a short `-- ---` banner comment
matching this file's existing section-header style):

```rust
/// One `train_movement_events` row, already collapsed to the latest
/// (`received_at`-DESC) event per distinct `loc_crs` for one `trains_id` --
/// the per-stop live overlay source
/// (docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md
/// §0.4/§3.3). `loc_crs` is never `NULL` here (`WHERE loc_crs IS NOT NULL`
/// below) -- a message whose STANOX never translated to a CRS has nothing
/// to key an overlay row on and is dropped, same "degrade, don't attach to
/// the wrong stop" posture as everywhere else in this data model.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MovementEventRow {
    pub loc_crs: String,
    pub event_type: Option<String>,
    pub planned_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub actual_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub variation_status: Option<String>,
}

/// `DISTINCT ON (UPPER(loc_crs))` keeps only the most-recently-`received_at`
/// event for each location -- so a location visited with an ARRIVAL then
/// later a DEPARTURE collapses to the DEPARTURE (the more complete, more
/// recent report), matching this app's existing "last reported" framing
/// (`train_current_state.last_reported_location`/`last_event_type`)
/// extended to a per-location granularity.
pub async fn latest_movement_event_per_location(
    pool: &PgPool,
    trains_id: i64,
) -> Result<Vec<MovementEventRow>> {
    let rows = sqlx::query_as::<_, MovementEventRow>(
        "SELECT DISTINCT ON (UPPER(loc_crs)) UPPER(loc_crs) AS loc_crs, event_type, \
                planned_timestamp, actual_timestamp, variation_status \
         FROM train_movement_events \
         WHERE trains_id = $1 AND loc_crs IS NOT NULL \
         ORDER BY UPPER(loc_crs), received_at DESC",
    )
    .bind(trains_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `crs -> name` for every code in `crs_codes` that has a `stations` row --
/// batched sibling of the `LEFT JOIN stations` pattern used everywhere else
/// in this data model (`pin_origin_name`, etc.), for a stop list built from
/// several separate CRS codes rather than one join target. A code with no
/// reference row is simply absent from the map.
pub async fn station_names_for_crs_batch(
    pool: &PgPool,
    crs_codes: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    if crs_codes.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let upper: Vec<String> = crs_codes.iter().map(|c| c.to_uppercase()).collect();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT UPPER(crs), name FROM stations WHERE UPPER(crs) = ANY($1)")
            .bind(&upper)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
}
```

Check the top of `queries.rs` for its existing `Result` alias (this file
uses `type Result<T> = anyhow::Result<T>;` or imports `anyhow::Result`
directly — match whichever convention is already there rather than
introducing a second one).

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api crs_for_tiplocs_batch_resolves -- --ignored --test-threads=1`
Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api list_calling_point_departures_for_train -- --ignored --test-threads=1`
Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api latest_movement_event_per_location -- --ignored --test-threads=1`
Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api station_names_for_crs_batch -- --ignored --test-threads=1`
Expected: all PASS. Also run `cargo build --workspace` and
`cargo test -p api` (non-ignored) to confirm nothing else broke.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "feat(api): add batched CRS/movement-event queries for the journey timetable overlay"
```

---

## Task 2: `crates/api/src/data/journey.rs` — the merge module

**Files:**
- Create: `crates/api/src/data/journey.rs`
- Modify: `crates/api/src/data/mod.rs` (add `pub mod journey;`, alongside
  the existing alphabetical `pub mod` list at `:1-20`)
- Test: colocated `#[cfg(test)] mod db_tests` in `journey.rs`, same
  convention as `queries.rs`/`trains.rs`

**Interfaces:**
- Consumes: `queries::crs_for_tiplocs_batch`, `queries::CallingPointDepartureRow`,
  `queries::list_calling_point_departures_for_train`,
  `queries::MovementEventRow`, `queries::latest_movement_event_per_location`,
  `queries::station_names_for_crs_batch` (all from Task 1);
  `eta_blend::london_to_utc` (already `pub(crate)`, existing);
  `schedule_query::CallingPointKind` (already a dependency, existing).
- Produces (used by Task 3):
  - `#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "camelCase")] pub struct JourneyStop { pub crs: Option<String>, pub name: Option<String>, pub tiploc: Option<String>, pub kind: Option<schedule_query::CallingPointKind>, pub scheduled_arrival: Option<chrono::DateTime<chrono::Utc>>, pub scheduled_departure: Option<chrono::DateTime<chrono::Utc>>, pub actual_arrival: Option<chrono::DateTime<chrono::Utc>>, pub actual_departure: Option<chrono::DateTime<chrono::Utc>>, pub last_event_type: Option<String>, pub variation_status: Option<String>, pub delay_minutes: Option<i32> }`
  - `pub async fn build_journey_stops(pool: &PgPool, trains_id: i64, train_uid: &str, service_date: chrono::NaiveDate, calling_points_json: Option<&serde_json::Value>) -> anyhow::Result<Option<Vec<JourneyStop>>>`

- [ ] **Step 1: Write the failing tests**

```rust
// crates/api/src/data/journey.rs

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
                build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "TEST-JRN-CP", service_date)
            .await
            .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-1".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JRN-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-2".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-JRN-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-EUSTON",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-CREWE",
                "kind": "Terminate",
                "bookedArrival": "10:30:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-CP", service_date, Some(&calling_points))
            .await
            .expect("build_journey_stops")
            .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("EUS"));
        assert_eq!(stops[0].kind, Some(schedule_query::CallingPointKind::Origin));
        assert!(stops[0].scheduled_departure.is_some());
        assert_eq!(stops[1].crs.as_deref(), Some("CRE"));
        assert_eq!(stops[1].kind, Some(schedule_query::CallingPointKind::Terminate));
        assert!(stops[1].scheduled_arrival.is_some());

        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "TEST-JRN-FB", service_date)
            .await
            .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:00:00".parse().unwrap(),
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                },
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:20:00".parse().unwrap(),
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "SLO".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                },
            ],
        )
        .await
        .expect("seed schedule_destination_departures");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-FB", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops from the fallback source");

        assert_eq!(stops.len(), 3, "RDG + SLO + synthetic WAT terminus");
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(stops[0].kind, Some(schedule_query::CallingPointKind::Origin));
        assert_eq!(stops[1].crs.as_deref(), Some("SLO"));
        assert_eq!(stops[1].kind, Some(schedule_query::CallingPointKind::Intermediate));
        assert_eq!(stops[2].crs.as_deref(), Some("WAT"));
        assert_eq!(stops[2].kind, Some(schedule_query::CallingPointKind::Terminate));
        assert!(stops[2].scheduled_arrival.is_none(), "no arrival time known from this source yet");

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_returns_none_when_neither_source_has_anything \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_returns_none_when_neither_source_has_anything() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NONE", service_date)
            .await
            .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NONE'")
            .execute(&pool)
            .await
            .ok();

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-NONE", service_date, None)
            .await
            .expect("build_journey_stops");

        assert!(stops.is_none());

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_a_departure_event_with_correct_delay_sign \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_a_departure_event_with_correct_delay_sign() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "TEST-JRN-OV", service_date)
            .await
            .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                train_uid: "TEST-JRN-OV".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k1', '0003', 'DEPARTURE', 'RDG', '2026-09-08T07:00:00Z', \
                     '2026-09-08T07:04:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-OV", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops");

        assert_eq!(stops.len(), 1, "no true_origin != destination gap here, only RDG itself");
        assert_eq!(stops[0].actual_departure, "2026-09-08T07:04:00Z".parse().ok());
        assert_eq!(stops[0].last_event_type.as_deref(), Some("DEPARTURE"));
        assert_eq!(stops[0].delay_minutes, Some(4), "actual 4 minutes after this event's own planned time");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
```

Note on the third assertion (`stops.len(), 1`): this fixture has only one
`schedule_destination_departures` row (RDG, the true origin), and
`destination_crs = "WAT"` differs from `RDG`, so `build_journey_stops`
would normally append a synthetic WAT terminus too. **Write the test
exactly as above first** — if it fails with `stops.len() == 2` instead of
`1`, that's expected (the synthetic-terminus logic in Step 3 always
appends when `destination_crs` differs from the last row's `crs`); fix the
assertion to `2` and add `assert_eq!(stops[1].crs.as_deref(), Some("WAT"))`
once you see the real Step-3 behavior, rather than changing Step 3 to
match a guessed count. (This is flagged rather than silently "fixed" in
this plan because the exact interaction of "one-row fallback list" +
"synthetic terminus" is exactly the kind of off-by-one a fresh
implementer should verify against real output, not trust a plan's
prediction of it.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api build_journey_stops -- --ignored --test-threads=1`
Expected: FAIL to compile (`journey` module / `build_journey_stops` don't
exist yet).

- [ ] **Step 3: Implement `journey.rs`**

```rust
//! Merges a train's scheduled timetable (from `trains.calling_points` when
//! schedule-matching has populated it, else reconstructed from
//! `schedule_destination_departures`) with the latest reported movement
//! event per location, into one ordered `JourneyStop[]` -- the primary
//! data source for the train detail page's timeline. See
//! docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::eta_blend::london_to_utc;
use crate::data::queries;

/// Mirrors `schedule_matching::ScheduleCallingPointDto`'s exact camelCase
/// wire shape (the format `trains.calling_points` is stored in) -- a
/// separate, `Deserialize`-only type rather than importing that module's
/// private struct, matching this codebase's "each layer owns its own wire
/// shape" posture (the same relationship `frontend/lib/types.ts`'s
/// `ScheduleCallingPoint` already has to it, just on the Rust side).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCallingPoint {
    tiploc: String,
    kind: schedule_query::CallingPointKind,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
}

/// One calling point of a train's journey, booked schedule merged with the
/// latest reported live data for that location -- see this module's own
/// doc comment and the design doc §2/§3.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyStop {
    pub crs: Option<String>,
    pub name: Option<String>,
    pub tiploc: Option<String>,
    pub kind: Option<schedule_query::CallingPointKind>,
    pub scheduled_arrival: Option<DateTime<Utc>>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    pub last_event_type: Option<String>,
    pub variation_status: Option<String>,
    pub delay_minutes: Option<i32>,
}

impl JourneyStop {
    fn from_calling_point(cp: &RawCallingPoint, crs: Option<String>, service_date: NaiveDate) -> Self {
        Self {
            crs,
            name: None, // filled in by a batch station-name pass in `build_journey_stops`
            tiploc: Some(cp.tiploc.clone()),
            kind: Some(cp.kind),
            scheduled_arrival: cp.booked_arrival.and_then(|t| london_to_utc(service_date.and_time(t))),
            scheduled_departure: cp.booked_departure.and_then(|t| london_to_utc(service_date.and_time(t))),
            actual_arrival: None,
            actual_departure: None,
            last_event_type: None,
            variation_status: None,
            delay_minutes: None,
        }
    }
}

/// Builds the ordered stop list for `(train_uid, service_date)`, or `None`
/// if neither the primary (`calling_points_json`) nor fallback
/// (`schedule_destination_departures`) source has anything -- see the
/// design doc §1 for when this is/isn't called, and §0.2/§3.2 for the
/// fallback's synthetic-terminus construction.
pub async fn build_journey_stops(
    pool: &PgPool,
    trains_id: i64,
    train_uid: &str,
    service_date: NaiveDate,
    calling_points_json: Option<&serde_json::Value>,
) -> anyhow::Result<Option<Vec<JourneyStop>>> {
    let mut stops: Vec<JourneyStop> = match calling_points_json {
        Some(json) => {
            let raw: Vec<RawCallingPoint> = serde_json::from_value(json.clone())?;
            let tiplocs: Vec<String> = raw.iter().map(|cp| cp.tiploc.clone()).collect();
            let tiploc_to_crs = queries::crs_for_tiplocs_batch(pool, &tiplocs).await?;
            raw.iter()
                .map(|cp| {
                    let crs = tiploc_to_crs.get(&cp.tiploc.to_uppercase()).cloned();
                    JourneyStop::from_calling_point(cp, crs, service_date)
                })
                .collect()
        }
        None => {
            let rows = queries::list_calling_point_departures_for_train(pool, train_uid, service_date).await?;
            if rows.is_empty() {
                return Ok(None);
            }
            let mut built: Vec<JourneyStop> = rows
                .iter()
                .map(|row| JourneyStop {
                    crs: Some(row.origin_crs.clone()),
                    name: None,
                    tiploc: None,
                    kind: Some(if Some(row.origin_crs.as_str()) == row.true_origin_crs.as_deref() {
                        schedule_query::CallingPointKind::Origin
                    } else {
                        schedule_query::CallingPointKind::Intermediate
                    }),
                    scheduled_arrival: None,
                    scheduled_departure: london_to_utc(service_date.and_time(row.scheduled)),
                    actual_arrival: None,
                    actual_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                })
                .collect();

            if let Some(destination_crs) = rows.last().and_then(|r| r.destination_crs.clone())
                && built
                    .last()
                    .and_then(|s| s.crs.as_deref())
                    .is_none_or(|last_crs| !last_crs.eq_ignore_ascii_case(&destination_crs))
            {
                built.push(JourneyStop {
                    crs: Some(destination_crs),
                    name: None,
                    tiploc: None,
                    kind: Some(schedule_query::CallingPointKind::Terminate),
                    scheduled_arrival: None,
                    scheduled_departure: None,
                    actual_arrival: None,
                    actual_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                });
            }
            built
        }
    };

    if stops.is_empty() {
        return Ok(None);
    }

    // Station names, batched over every distinct CRS this stop list has.
    let stop_crs: Vec<String> = stops.iter().filter_map(|s| s.crs.clone()).collect();
    let names = queries::station_names_for_crs_batch(pool, &stop_crs).await?;
    for stop in &mut stops {
        if let Some(crs) = &stop.crs {
            stop.name = names.get(&crs.to_uppercase()).cloned();
        }
    }

    // Live overlay.
    let events = queries::latest_movement_event_per_location(pool, trains_id).await?;
    let events_by_crs: HashMap<String, queries::MovementEventRow> =
        events.into_iter().map(|e| (e.loc_crs.clone(), e)).collect();

    for stop in &mut stops {
        let Some(crs) = &stop.crs else { continue };
        let Some(event) = events_by_crs.get(&crs.to_uppercase()) else {
            continue;
        };
        stop.last_event_type = event.event_type.clone();
        stop.variation_status = event.variation_status.clone();

        match event.event_type.as_deref() {
            Some("ARRIVAL") => {
                stop.actual_arrival = event.actual_timestamp;
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
            }
            Some("DEPARTURE") => {
                stop.actual_departure = event.actual_timestamp;
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            Some("PASS") => {
                stop.actual_arrival = event.actual_timestamp;
                stop.actual_departure = event.actual_timestamp;
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            _ => {}
        }

        // Same delay formula trust-consumer's own forward-propagation uses
        // (`process.rs`: `(a - p).num_minutes()`) -- positive means late.
        let scheduled_reference = stop.scheduled_departure.or(stop.scheduled_arrival);
        let actual_reference = stop.actual_departure.or(stop.actual_arrival);
        stop.delay_minutes = match (actual_reference, scheduled_reference) {
            (Some(a), Some(s)) => Some((a - s).num_minutes() as i32),
            _ => None,
        };
    }

    Ok(Some(stops))
}
```

Add `pub mod journey;` to `crates/api/src/data/mod.rs`'s `pub mod` list
(alphabetically, between `island_of_ireland` and `legacy_backfill`).

Double check `schedule_query::CallingPointKind` derives `PartialEq` (it
does, per `crates/schedule-query/src/records.rs:98`: `#[derive(Debug,
Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]`) so the
`assert_eq!(stops[0].kind, Some(schedule_query::CallingPointKind::Origin))`
assertions in Step 1 compile.

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api build_journey_stops -- --ignored --test-threads=1`
Expected: all four PASS (adjusting the third test's exact assertions per
the Step 1 note if the synthetic-terminus row does append there — verify
against real output, don't guess).
Also run: `cargo build --workspace` to confirm the new module compiles
cleanly workspace-wide.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/journey.rs crates/api/src/data/mod.rs
git commit -m "feat(api): add build_journey_stops, merging scheduled timetable with live movement events"
```

---

## Task 3: Wire `journeyStops` into both read routes

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs` (`TrackedTrainState`
  struct `:923-972`, `TRACKED_TRAIN_STATE_SELECT` `:1011-1025`)
- Modify: `crates/api/src/data/trains.rs` (`PublicTrainState` struct
  `:220-251`)
- Modify: `crates/api/src/routes/train.rs` (`get_by_tracking_id` `:552-580`,
  `get_by_uid_and_date` `:686-716`, plus the `blend_darwin_eta` helper's
  neighborhood `:879-916` for the new sibling helper)
- Test: `crates/api/src/routes/train.rs`'s existing `db_tests` module

**Interfaces:**
- Consumes: `journey::build_journey_stops` (Task 2).
- Produces: `TrackedTrainState.trains_id: Option<i64>` (internal, not
  serialized), `TrackedTrainState.journey_stops: Option<Vec<journey::JourneyStop>>`
  (serialized as `journeyStops`), `PublicTrainState.journey_stops: Option<Vec<journey::JourneyStop>>`
  (serialized as `journeyStops`) — both consumed by Task 5 (frontend types).

- [ ] **Step 1: Write the failing tests**

Add to `crates/api/src/routes/train.rs`'s `db_tests` module, near the
existing `get_by_tracking_id`/`get_by_uid_and_date` tests (`:2146` /
similarly for the by-uid ones — grep `async fn get_by_uid_and_date` in
that test module to find its neighborhood):

```rust
#[tokio::test]
#[ignore = "requires a live database; see the plan's Global Constraints for the \
            DATABASE_URL incantation, then run with `cargo test -p api \
            get_by_tracking_id_includes_journey_stops -- --ignored --test-threads=1`"]
async fn get_by_tracking_id_includes_journey_stops_once_a_schedule_match_exists() {
    let pool = connect().await;
    let user_id = "TEST-JS-ROUTE-OWNER";
    seed_session(&pool, user_id).await;
    let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();

    // `find_or_create_train_with_schedule_match` seeds a `trains` row with
    // real `calling_points`, exactly like a successful schedule match would.
    let calling_points = serde_json::json!([
        {
            "tiploc": "TEST-JSR-ORIGIN",
            "kind": "Origin",
            "bookedArrival": null,
            "bookedDeparture": "09:00:00",
            "isHalfMinuteArrival": false,
            "isHalfMinuteDeparture": false
        }
    ]);
    let trains_id = crate::data::trains::find_or_create_train_with_schedule_match(
        &pool,
        "TEST-JSR-UID",
        service_date,
        "KGX",
        service_date.and_hms_opt(9, 0, 0).unwrap().and_utc(),
        None,
        "line-a",
        &calling_points,
    )
    .await
    .expect("seed a schedule-matched trains row");

    let (tracking_id,): (i64,) = sqlx::query_as(
        "INSERT INTO train_subscriptions \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, resolution_status, trains_id) \
         VALUES ($1, $2, 'KGX', $3, 'schedule_matched', $4) RETURNING id",
    )
    .bind(user_id)
    .bind(service_date)
    .bind(service_date.and_hms_opt(9, 0, 0).unwrap().and_utc())
    .bind(trains_id)
    .fetch_one(&pool)
    .await
    .expect("seed fixture train_subscriptions row");

    let token = seed_session(&pool, user_id).await;
    let router = test_router(test_app(pool.clone()));
    let (status, body) = request(router, format!("/Train/{tracking_id}"), Some(&token)).await;

    assert_eq!(status, StatusCode::OK);
    let stops = body.get("journeyStops").expect("journeyStops present").as_array().expect("array");
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0]["crs"], Value::Null, "TEST-JSR-ORIGIN has no stanox_crs row in this fixture");
    assert_eq!(stops[0]["kind"], Value::String("Origin".to_string()));

    cleanup_user(&pool, user_id).await;
}
```

(Follow this file's existing `seed_session` returning a token used twice
above only because `seed_session` is idempotent per user — check its real
implementation at `:1347`; if it errors on a second call for the same
`user_id`, call it once and reuse the returned token, matching every other
test in this file's own style.)

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api get_by_tracking_id_includes_journey_stops -- --ignored --test-threads=1`
Expected: FAIL — response has no `journeyStops` key yet (or compile error
if the struct field doesn't exist yet; either is the correct starting
failure).

- [ ] **Step 3: Implement the wiring**

In `crates/api/src/data/train_tracking.rs`, add to `TrackedTrainState`
(after `pub custom_name: Option<String>,` at `:971`):

```rust
    /// The shared `trains` row's own surrogate key, needed to read
    /// `train_movement_events` for this train's live overlay -- internal
    /// plumbing for `routes::train`'s journey-stops attachment, never sent
    /// to the frontend (this struct already uses a *different* `id` for
    /// the tracking id, so exposing a second, differently-scoped `id`-like
    /// field on the wire would repeat exactly the confusion
    /// `PublicTrainState::trains_id`'s own doc comment describes).
    #[serde(skip_serializing)]
    pub trains_id: Option<i64>,
    /// The merged scheduled-timetable + live-overlay stop list -- `None`
    /// until `train_uid` is known, or if neither backing source has
    /// anything for this train (see
    /// docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md
    /// §1). Populated by `routes::train`'s handlers AFTER this struct is
    /// read from the DB (same "read row, then overlay a computed field"
    /// pattern `blend_darwin_eta` already uses for `eta_next`/`eta_source`
    /// on this same struct) -- never selected directly by
    /// `TRACKED_TRAIN_STATE_SELECT`, hence `#[sqlx(default)]`.
    #[sqlx(default)]
    pub journey_stops: Option<Vec<crate::data::journey::JourneyStop>>,
```

Update `TRACKED_TRAIN_STATE_SELECT` to add `tr.id AS trains_id` (additive
column on the existing `LEFT JOIN trains tr`):

```rust
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tr.train_uid, tr.train_id, \
           tr.destination_crs AS schedule_destination_crs, ssd.name AS schedule_destination_name, \
           tr.calling_points AS schedule_calling_points, \
           tr.id AS trains_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM train_subscriptions tt \
    LEFT JOIN trains tr ON tr.id = tt.trains_id \
    LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
    LEFT JOIN stations ssd ON ssd.crs = UPPER(tr.destination_crs)";
```

In `crates/api/src/data/trains.rs`, add to `PublicTrainState` (after
`pub eta_source: Option<String>,` at `:250`):

```rust
    /// See `train_tracking::TrackedTrainState::journey_stops`'s doc
    /// comment -- same contract, populated the same "read row, then
    /// overlay" way by `routes::train::get_by_uid_and_date`. This struct
    /// already carries `trains_id` on the wire (unlike `TrackedTrainState`,
    /// where it's an internal-only addition), so no extra field is needed
    /// to know which `trains_id` to key the overlay query on.
    #[sqlx(default)]
    pub journey_stops: Option<Vec<crate::data::journey::JourneyStop>>,
```

In `crates/api/src/routes/train.rs`, add a new helper near
`blend_darwin_eta` (`:879`):

```rust
/// Attaches `journey_stops` to an already-fetched `TrackedTrainState`,
/// mirroring `blend_darwin_eta`'s own "read row, then overlay a computed
/// field" shape on the same struct. Only attempted once `train_uid`/
/// `trains_id` are both known (§1 of the design doc) -- a `pending`/
/// `unresolved` state has neither, and this returns `state` unchanged for
/// it, same as `blend_darwin_eta`'s own early-return branches. A DB error
/// building the overlay degrades to `journey_stops: None` rather than
/// failing the whole request -- the same best-effort posture
/// `blend_darwin_eta` already has for its own overlay.
async fn attach_journey_stops(
    app: &App,
    mut state: train_tracking::TrackedTrainState,
) -> train_tracking::TrackedTrainState {
    let (Some(trains_id), Some(train_uid)) = (state.trains_id, state.train_uid.clone()) else {
        return state;
    };
    match crate::data::journey::build_journey_stops(
        &app.database,
        trains_id,
        &train_uid,
        state.service_date,
        state.schedule_calling_points.as_ref(),
    )
    .await
    {
        Ok(stops) => state.journey_stops = stops,
        Err(err) => {
            tracing::warn!(error = ?err, trains_id, "could not build journey stops");
        }
    }
    state
}

/// Public-route sibling of `attach_journey_stops`, for `PublicTrainState`.
async fn attach_journey_stops_public(
    app: &App,
    mut state: crate::data::trains::PublicTrainState,
) -> crate::data::trains::PublicTrainState {
    let Some(train_uid_for_journey) = Some(state.train_uid.clone()).filter(|_| state.train_id.is_some() || state.calling_points.is_some())
    else {
        return state;
    };
    match crate::data::journey::build_journey_stops(
        &app.database,
        state.trains_id,
        &train_uid_for_journey,
        state.service_date,
        state.calling_points.as_ref(),
    )
    .await
    {
        Ok(stops) => state.journey_stops = stops,
        Err(err) => {
            tracing::warn!(error = ?err, trains_id = state.trains_id, "could not build journey stops");
        }
    }
    state
}
```

Read that `attach_journey_stops_public` gate carefully before wiring it
in: `PublicTrainState.train_uid` is a bare `String` (always present once
any `trains` row exists at all, per `get_public_train_state`'s `SELECT
tr.train_uid`), so it cannot itself signal "train_uid unknown" the way
`TrackedTrainState.train_uid: Option<String>` can. The real gate for
`PublicTrainState` is the SAME one `app/train/[uid]/[date]/page.tsx`'s own
`toJourneyState` already uses to derive `resolutionStatus` (`train.trainId
? 'resolved' : train.originCrs ? 'schedule_matched' : 'pending'`) — i.e.
"is there a schedule match OR a live resolution," not "is `train_uid`
non-null." Rewrite the gate to match that exactly:

```rust
async fn attach_journey_stops_public(
    app: &App,
    mut state: crate::data::trains::PublicTrainState,
) -> crate::data::trains::PublicTrainState {
    if state.train_id.is_none() && state.origin_crs.is_none() {
        // Neither a live resolution nor a schedule match has happened yet
        // -- same "pending" gate `toJourneyState` (frontend) already uses.
        return state;
    }
    match crate::data::journey::build_journey_stops(
        &app.database,
        state.trains_id,
        &state.train_uid,
        state.service_date,
        state.calling_points.as_ref(),
    )
    .await
    {
        Ok(stops) => state.journey_stops = stops,
        Err(err) => {
            tracing::warn!(error = ?err, trains_id = state.trains_id, "could not build journey stops");
        }
    }
    state
}
```

Wire both into the route handlers:

```rust
// get_by_tracking_id, replacing the existing match arm:
    match state {
        Some(state) => Ok(Json(attach_journey_stops(&app, blend_darwin_eta(&app, state).await).await)),
        None => Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        )),
    }
```

```rust
// get_by_uid_and_date, replacing the existing match arm:
    match state {
        Some(state) => Ok(Json(attach_journey_stops_public(&app, state).await)),
        None => Err((
            StatusCode::NOT_FOUND,
            "no known train for that uid/date".to_string(),
        )),
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api get_by_tracking_id_includes_journey_stops -- --ignored --test-threads=1`
Expected: PASS.
Run: `cargo build --workspace`
Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`
Expected: every existing DB-backed test in the workspace still passes (in
particular, every existing `get_by_tracking_id`/`get_by_uid_and_date` test
that asserts an exact JSON body — check whether any use a strict
`assert_eq!(body, json!({...}))` full-object comparison rather than
per-field assertions; if so, those need `"journeyStops": null` (or the
real array) added to their expected literal, since the new field will now
always be present on the wire once these routes return `Json<...>`, so an
exact-object comparison without the new key would fail even though
nothing about the *field it was actually testing* changed).
Run: `cargo test -p api` (non-DB tests) and `cargo test --workspace`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/data/trains.rs crates/api/src/routes/train.rs
git commit -m "feat(api): attach journeyStops to both tracked-train read routes"
```

---

## Task 4: Frontend types — `frontend/lib/types.ts`

**Files:**
- Modify: `frontend/lib/types.ts` (`TrainJourneyState` interface `:402-443`,
  `PublicTrainState` interface `:458-476`)
- Modify: `frontend/app/train/[uid]/[date]/page.tsx` (`toJourneyState`
  `:34-57`)
- Test: `frontend/app/train/[uid]/[date]/page.test.tsx` if it exists
  (grep for it first — if there's a `toJourneyState`-specific test, extend
  it; the by-id page's own `page.test.tsx` at
  `frontend/app/train/by-id/[trackingId]/page.test.tsx:63` already has a
  `scheduleCallingPoints: null` fixture entry to extend similarly)

**Interfaces:**
- Consumes: nothing new — this is the TypeScript mirror of Task 3's Rust
  wire shapes.
- Produces (used by Tasks 5-6): `JourneyStop`, `JourneyStopKind` types;
  `TrainJourneyState.journeyStops: JourneyStop[] | null`;
  `PublicTrainState.journeyStops: JourneyStop[] | null`.

- [ ] **Step 1: Write the failing test**

In `frontend/app/train/[uid]/[date]/page.test.tsx` (create it if it
doesn't exist yet — check first; if `toJourneyState` is currently
untested, this is a real gap this task also closes), add:

```tsx
import { describe, expect, it } from 'vitest';
// adjust the import to match this file's actual export shape once you've
// read it -- `toJourneyState` may need exporting from page.tsx first if
// it isn't already (it currently is a private, non-exported function per
// the file as read for this plan; export it as `toJourneyState` for
// testability, matching this repo's convention of exporting pure mapping
// functions from otherwise-page-only files when a test needs them).

describe('toJourneyState', () => {
  it('carries journeyStops through unchanged from PublicTrainState', () => {
    const stop = {
      crs: 'RDG',
      name: 'Reading',
      tiploc: null,
      kind: 'Origin' as const,
      scheduledArrival: null,
      scheduledDeparture: '2026-09-08T08:00:00Z',
      actualArrival: null,
      actualDeparture: null,
      lastEventType: null,
      variationStatus: null,
      delayMinutes: null,
    };
    const result = toJourneyState({
      trainsId: 1,
      trainUid: 'X12345',
      serviceDate: '2026-09-08',
      originCrs: 'RDG',
      originName: 'Reading',
      destinationCrs: 'WAT',
      destinationName: 'London Waterloo',
      scheduledDeparture: '2026-09-08T08:00:00Z',
      callingPoints: null,
      trainId: null,
      status: null,
      lastReportedLocation: null,
      lastEventType: null,
      delayMinutes: null,
      nextCallingPoint: null,
      etaNext: null,
      etaSource: null,
      journeyStops: [stop],
    });

    expect(result.journeyStops).toEqual([stop]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run app/train/[uid]/[date]/page.test.tsx`
Expected: FAIL — `journeyStops` doesn't exist on either type yet (a
TypeScript compile error under vitest, or the property is simply `undefined`
if types aren't checked strictly at test time — either is an acceptable
starting failure).

- [ ] **Step 3: Add the types and wire the mapping**

In `frontend/lib/types.ts`, after `export type ScheduleCallingPointKind = ...`
(`:361`):

```ts
export type JourneyStopKind = 'Origin' | 'Intermediate' | 'Terminate';

/** One calling point of a train's journey, booked schedule merged with the
 * latest reported live data for that location --
 * `crates/api/src/data/journey.rs`'s `JourneyStop`, camelCase on the wire.
 * `null` fields mean "not yet known" (a stop not yet reached has no
 * `actual*`/`delayMinutes`), never a fabricated value -- see
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §2. */
export interface JourneyStop {
  crs: string | null;
  name: string | null;
  tiploc: string | null;
  kind: JourneyStopKind | null;
  scheduledArrival: string | null; // RFC3339
  scheduledDeparture: string | null; // RFC3339
  actualArrival: string | null; // RFC3339
  actualDeparture: string | null; // RFC3339
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  variationStatus: string | null;
  delayMinutes: number | null;
}
```

In `TrainJourneyState` (`:402-443`), add after `pinScheduledDeparture?: string | null;`:

```ts
  // The merged scheduled-timetable + live-overlay stop list -- `null`
  // until `trainUid` is known, or if neither backing source has anything
  // for this train. See `components/JourneyTimeline.tsx` and
  // docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §1.
  journeyStops: JourneyStop[] | null;
```

In `PublicTrainState` (`:458-476`), add after `pub eta_source`'s TS
mirror (`etaSource: EtaSource | null;`):

```ts
  journeyStops: JourneyStop[] | null;
```

In `frontend/app/train/[uid]/[date]/page.tsx`'s `toJourneyState`
(`:34-57`), export the function (change `function toJourneyState` to
`export function toJourneyState`) and add to the returned object, after
`etaSource: train.etaSource,`:

```ts
    journeyStops: train.journeyStops,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run app/train/[uid]/[date]/page.test.tsx`
Expected: PASS.
Run: `cd frontend && npx tsc --noEmit` (or whatever this repo's exact
typecheck command is — check `frontend/package.json`'s `scripts` first)
to confirm no other call site broke from the now-required `journeyStops`
field. Fix the two existing test fixtures that construct a
`TrainJourneyState`/`TrackedTrainState` object literal without it
(`frontend/app/train/by-id/[trackingId]/page.test.tsx:63` and
`frontend/components/TrainJourney.test.tsx:27`, both currently setting
`scheduleCallingPoints: null` — add `journeyStops: null` alongside each).

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/app/train/[uid]/[date]/page.tsx frontend/app/train/by-id/[trackingId]/page.test.tsx frontend/components/TrainJourney.test.tsx
git commit -m "feat(frontend): add JourneyStop type and wire journeyStops through TrainJourneyState"
```

(If `frontend/app/train/[uid]/[date]/page.test.tsx` didn't exist before
this task, also `git add` it as a new file.)

---

## Task 5: `frontend/components/JourneyTimeline.tsx`

**Files:**
- Create: `frontend/components/JourneyTimeline.tsx`
- Test: `frontend/components/JourneyTimeline.test.tsx`

**Interfaces:**
- Consumes: `JourneyStop`, `JourneyStopKind` (Task 4);
  `formatTime` (`frontend/lib/dateFormat.ts`, existing).
- Produces: `JourneyTimeline({ stops }: { stops: JourneyStop[] })` —
  consumed by Task 6.

- [ ] **Step 1: Write the failing test**

First read `frontend/test/render.tsx` (the `renderWithMantine` helper) and
one existing colocated component test (e.g.
`frontend/components/EtaBadge.test.tsx` if it exists, else
`TrainJourney.test.tsx`) to match this repo's exact test-file conventions
(imports, `describe`/`it` vs `test`, assertion style) before writing this
file.

```tsx
// frontend/components/JourneyTimeline.test.tsx
import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyTimeline } from './JourneyTimeline';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'RDG',
    name: 'Reading',
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    ...overrides,
  };
}

describe('JourneyTimeline', () => {
  it('renders a station name for every stop, in order', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Terminate' }),
        ]}
      />,
    );
    const names = screen.getAllByText(/Reading|London Waterloo/);
    expect(names[0]).toHaveTextContent('Reading');
    expect(names[1]).toHaveTextContent('London Waterloo');
  });

  it('falls back to the CRS code when no station name is known', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ crs: 'ZZZ', name: null })]} />);
    expect(screen.getByText('ZZZ')).toBeInTheDocument();
  });

  it('shows only the scheduled time for a stop with no actual time yet', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ scheduledDeparture: '2026-09-08T08:00:00Z', actualDeparture: null })]}
      />,
    );
    expect(screen.queryByText(/late|early|on time/i)).not.toBeInTheDocument();
  });

  it('shows a late badge for a positive delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:04:00Z',
            delayMinutes: 4,
          }),
        ]}
      />,
    );
    expect(screen.getByText('4m late')).toBeInTheDocument();
  });

  it('shows an early badge for a negative delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: '2026-09-08T07:59:00Z',
            delayMinutes: -1,
          }),
        ]}
      />,
    );
    expect(screen.getByText('1m early')).toBeInTheDocument();
  });

  it('shows an on-time badge for a zero delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:00:00Z',
            delayMinutes: 0,
          }),
        ]}
      />,
    );
    expect(screen.getByText('On time')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/JourneyTimeline.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `JourneyTimeline.tsx`**

```tsx
import { Badge, Group, Stack, Text } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { JourneyStop } from '@/lib/types';

/** Renders the full scheduled timetable as the primary structure of the
 * train detail page, with live actual-vs-scheduled data overlaid per stop
 * once available -- see
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §4.
 * Rendered whenever `TrainJourneyState.journeyStops` is non-null,
 * independent of `resolutionStatus`/`status` -- this is the restructuring
 * the design doc's §4 describes: the timeline is no longer nested inside
 * one branch of the status switch. */
export function JourneyTimeline({ stops }: { stops: JourneyStop[] }) {
  return (
    <Stack gap={4} role="list" aria-label="Journey timeline">
      {stops.map((stop, index) => (
        <JourneyStopRow key={`${stop.crs ?? 'unknown'}-${index}`} stop={stop} />
      ))}
    </Stack>
  );
}

function delayBadge(delayMinutes: number | null) {
  if (delayMinutes === null) return null;
  if (delayMinutes === 0) {
    return (
      <Badge color="green" variant="light">
        On time
      </Badge>
    );
  }
  if (delayMinutes > 0) {
    return (
      <Badge color="orange" variant="light">
        {delayMinutes}m late
      </Badge>
    );
  }
  return (
    <Badge color="teal" variant="light">
      {Math.abs(delayMinutes)}m early
    </Badge>
  );
}

function JourneyStopRow({ stop }: { stop: JourneyStop }) {
  const label = stop.name ?? stop.crs ?? 'Unknown location';
  const scheduled = stop.scheduledDeparture ?? stop.scheduledArrival;
  const actual = stop.actualDeparture ?? stop.actualArrival;
  const reached = actual !== null;

  return (
    <Group gap="xs" role="listitem" wrap="nowrap">
      <Text fw={stop.kind === 'Origin' || stop.kind === 'Terminate' ? 700 : 400} c={reached ? undefined : 'dimmed'}>
        {label}
      </Text>
      {scheduled && (
        <Text size="sm" c="dimmed">
          {formatTime(scheduled)}
        </Text>
      )}
      {actual && (
        <Text size="sm">{formatTime(actual)}</Text>
      )}
      {delayBadge(stop.delayMinutes)}
    </Group>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/JourneyTimeline.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/JourneyTimeline.tsx frontend/components/JourneyTimeline.test.tsx
git commit -m "feat(frontend): add JourneyTimeline, rendering the per-stop scheduled/actual overlay"
```

---

## Task 6: Restructure `TrainJourney.tsx`

**Files:**
- Modify: `frontend/components/TrainJourney.tsx` (whole file, `:1-181`)
- Modify: `frontend/components/TrainJourney.test.tsx`

**Interfaces:**
- Consumes: `JourneyTimeline` (Task 5); `state.journeyStops` (Task 4).
- Produces: `TrainJourney({ state }: { state: TrainJourneyState })` —
  same public signature as today, no caller (`app/train/[uid]/[date]/page.tsx`,
  `app/train/by-id/[trackingId]/page.tsx`) needs to change.

- [ ] **Step 1: Update the existing tests first**

Read `frontend/components/TrainJourney.test.tsx` in full before editing
it — it has one test per row of the old Decision-3 state table. Add
`journeyStops: null` to every existing fixture object (matching Task 4's
`scheduleCallingPoints: null` sibling addition already made there), then
add these new cases:

```tsx
it('renders the JourneyTimeline when journeyStops is present, even while status is awaiting_activation', () => {
  renderWithMantine(
    <TrainJourney
      state={{
        ...baseState, // however this file's existing fixture base object is named -- reuse it
        resolutionStatus: 'resolved',
        status: 'awaiting_activation',
        trainUid: 'X12345',
        journeyStops: [
          {
            crs: 'RDG',
            name: 'Reading',
            tiploc: null,
            kind: 'Origin',
            scheduledArrival: null,
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualArrival: null,
            actualDeparture: null,
            lastEventType: null,
            variationStatus: null,
            delayMinutes: null,
          },
        ],
      }}
    />,
  );
  expect(screen.getByRole('list', { name: 'Journey timeline' })).toBeInTheDocument();
  expect(screen.getByText('Reading')).toBeInTheDocument();
});

it('renders the JourneyTimeline for schedule_matched, not just resolved', () => {
  renderWithMantine(
    <TrainJourney
      state={{
        ...baseState,
        resolutionStatus: 'schedule_matched',
        status: null,
        trainUid: 'X12345',
        journeyStops: [
          {
            crs: 'RDG',
            name: 'Reading',
            tiploc: null,
            kind: 'Origin',
            scheduledArrival: null,
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualArrival: null,
            actualDeparture: null,
            lastEventType: null,
            variationStatus: null,
            delayMinutes: null,
          },
        ],
      }}
    />,
  );
  expect(screen.getByRole('list', { name: 'Journey timeline' })).toBeInTheDocument();
});

it('renders no JourneyTimeline for pending, even if journeyStops were somehow non-null', () => {
  renderWithMantine(
    <TrainJourney
      state={{
        ...baseState,
        resolutionStatus: 'pending',
        status: null,
        trainUid: null,
        journeyStops: null,
      }}
    />,
  );
  expect(screen.queryByRole('list', { name: 'Journey timeline' })).not.toBeInTheDocument();
  expect(screen.getByText('Waiting to hear from Network Rail')).toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests to verify the new ones fail**

Run: `cd frontend && npx vitest run components/TrainJourney.test.tsx`
Expected: the two new "renders the JourneyTimeline..." tests FAIL (no
`role="list"` rendered yet); the pre-existing tests should still PASS
unchanged (only a new always-`null` field was added to their fixtures).

- [ ] **Step 3: Restructure `TrainJourney.tsx`**

```tsx
import { Alert, Badge, Group, Loader, Stack, Text, Tooltip } from '@mantine/core';
import { EtaBadge } from './EtaBadge';
import { JourneyTimeline } from './JourneyTimeline';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { TrainJourneyState } from '@/lib/types';

/** Renders one train's journey through every state the backend can
 * return, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 3's original table, revised by
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md:
 * the scheduled timetable (`state.journeyStops`) is now the PRIMARY,
 * always-shown structure whenever it's available (i.e. whenever
 * `trainUid` is known -- `schedule_matched` or any `resolved` sub-state),
 * rendered once at the top level via `JourneyTimeline`, rather than nested
 * inside only the `resolved`+`en_route` branch the way the old
 * `JourneyDetails` denormalized summary was. `StatusMessage` below is the
 * original per-state switch, kept for its status copy/alerts, MINUS the
 * old `JourneyDetails` call (superseded by `JourneyTimeline` for any state
 * that has `journeyStops`). `JourneyDetails` itself is kept as a fallback
 * for the one state where `journeyStops` can still be `null` despite a
 * known `trainUid` -- a real train that isn't itself a CIF-published
 * schedule that day (see the design doc §1's named gap). */
export function TrainJourney({ state }: { state: TrainJourneyState }) {
  return (
    <Stack gap="sm">
      <StatusMessage state={state} />
      {state.journeyStops ? (
        <JourneyTimeline stops={state.journeyStops} />
      ) : (
        state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />
      )}
    </Stack>
  );
}

function StatusMessage({ state }: { state: TrainJourneyState }) {
  const pinSummary = (
    <Text size="sm" c="dimmed">
      {trackedTrainDisplayName(state)}
    </Text>
  );

  if (state.resolutionStatus === 'pending') {
    return (
      <Stack gap="sm" role="status">
        <Group gap="sm">
          <Loader size="sm" />
          <Text fw={500}>Waiting to hear from Network Rail</Text>
        </Group>
        {pinSummary}
        <Text size="sm" c="dimmed">
          This train hasn&apos;t been matched to a live service yet — that&apos;s normal if it hasn&apos;t
          started running. Network Rail typically doesn&apos;t report a service until shortly before it
          departs. This page updates automatically.
        </Text>
      </Stack>
    );
  }

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
      </Stack>
    );
  }

  if (state.resolutionStatus === 'unresolved') {
    return (
      <Stack gap="sm">
        <Text fw={500} c="red">
          Couldn&apos;t be matched to a live service
        </Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Network Rail never reported a matching service for this pin. This won&apos;t resolve on its own
          — try tracking the train again if it was a genuine mistake.
        </Text>
      </Stack>
    );
  }

  // resolutionStatus === 'resolved' from here on -- trainUid is non-null
  // per the backend's own resolution invariant (a tracked train is only
  // ever set to 'resolved' in the same write that sets train_uid), even
  // though the TypeScript type can't express that correlation across two
  // separate optional fields.
  if (state.status === 'awaiting_activation' || state.status === null) {
    return (
      <Stack gap="sm">
        <Text fw={500}>Matched to train {state.trainUid}</Text>
        {pinSummary}
        <Text size="sm" c="dimmed">
          Waiting for its first movement report.
        </Text>
      </Stack>
    );
  }

  if (state.status === 'cancelled') {
    return (
      <Stack gap="sm">
        <Alert color="red" title="Cancelled">
          This service was cancelled.
        </Alert>
        <Text fw={500}>Train {state.trainUid}</Text>
        {pinSummary}
      </Stack>
    );
  }

  const mayHaveFinished =
    state.status === 'completed' || (state.status === 'en_route' && state.nextCallingPoint === null);

  return (
    <Stack gap="sm">
      <Text fw={500}>Train {state.trainUid}</Text>
      {pinSummary}
      {mayHaveFinished && (
        <Alert color="yellow" title="May have finished" variant="light">
          {/* Provisional heuristic, not a confirmed backend status -- see
              this plan's Global Constraints and
              docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
              Open Question 2. Deliberately worded as an inference
              ("may have"), never asserted as fact. */}
          No further calling points have been reported. This journey may have finished, but this is an
          inference, not a confirmed status from Network Rail.
        </Alert>
      )}
    </Stack>
  );
}

/** Fallback for the one gap the design doc's §1 names: a resolved train
 * with no `journeyStops` at all (not itself a CIF-published schedule that
 * day). Unchanged from the pre-restructuring version, minus its own
 * now-redundant "no movement data" early return duplicating what
 * `TrainJourney` above already gates on via `resolutionStatus === 'resolved'`. */
function JourneyDetails({ state }: { state: TrainJourneyState }) {
  const hasMovementData =
    state.lastReportedLocation !== null ||
    state.delayMinutes !== null ||
    state.nextCallingPoint !== null ||
    state.etaNext !== null;

  if (!hasMovementData) {
    return (
      <Stack gap={4}>
        <Text size="sm" c="dimmed">
          No movement data reported yet.
        </Text>
      </Stack>
    );
  }

  return (
    <Stack gap={4}>
      {state.lastReportedLocation && (
        <Text size="sm">
          Last reported: {state.lastReportedLocation}
          {state.lastEventType ? ` (${state.lastEventType.toLowerCase()})` : ''}
        </Text>
      )}
      {state.delayMinutes !== null && (
        <Group gap={6}>
          <Text size="sm">Delay:</Text>
          <Badge color={state.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
            {state.delayMinutes > 0 ? `${state.delayMinutes}m late` : 'On time'}
          </Badge>
        </Group>
      )}
      {state.nextCallingPoint && <Text size="sm">Next calling point: {state.nextCallingPoint}</Text>}
      <EtaBadge etaNext={state.etaNext} etaSource={state.etaSource} />
    </Stack>
  );
}
```

Note the one deliberate behavior change from the literal old code: the old
`resolved`+`schedule_matched` branches never called `JourneyDetails` at
all (only the final `resolved`+non-`awaiting_activation` fall-through
did) — so the ternary in the new top-level `TrainJourney` guards
`JourneyDetails` with `state.resolutionStatus === 'resolved'` specifically
to preserve that: `schedule_matched` with a (hypothetically) `null`
`journeyStops` renders neither `JourneyTimeline` nor `JourneyDetails`,
same "nothing but the status message" behavior it always had. Verify this
against the Step 1 test fixtures once written — if a `schedule_matched`
fixture with `journeyStops: null` is asserted anywhere to show
`JourneyDetails`-shaped content, that assertion is wrong per the original
component's own behavior and should be corrected, not the guard.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run components/TrainJourney.test.tsx`
Expected: all PASS, including the two new cases from Step 1.
Run: `cd frontend && npx vitest run components/JourneyTimeline.test.tsx frontend/app/train`
Run: `cd frontend && npx tsc --noEmit`

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrainJourney.tsx frontend/components/TrainJourney.test.tsx
git commit -m "refactor(frontend): move the journey timeline to the top level of TrainJourney, across every train_uid-known state"
```

---

## Task 7: Full verification and push

**Files:** none new — this task runs the full test/build matrix named in
the spec's §6 and the top-level task's own Deliverable requirement, fixes
anything it turns up, then pushes.

- [ ] **Step 1: Run migrations against the test database**

```bash
cd crates/api && DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test sqlx migrate run
```

Expected: no pending migrations from this plan (Task 1-3 added no new
migration — every table/column this feature reads already exists), so
this should report "no migrations to run" or equivalent. Run it anyway,
it's a required step per the top-level task's own instructions, and
confirms the test DB is caught up with `main` before the next step.

- [ ] **Step 2: Full Rust build and test suite**

```bash
cargo build --workspace
cargo test --workspace
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
```

Expected: all green. If the DB-backed run turns up a pre-existing test
this plan's changes broke (most likely: an exact-JSON-body comparison
test in `routes/train.rs` that didn't anticipate the new `journeyStops`
key — flagged already in Task 3 Step 4), fix it there rather than
reverting the new field.

- [ ] **Step 3: Full frontend build and test suite**

```bash
cd frontend
npx vitest run
npm run build
```

Expected: all green. `npm run build` in particular catches any TypeScript
error across files this plan didn't explicitly enumerate (e.g. another
`TrainJourneyState`/`TrackedTrainState` object-literal fixture somewhere
this plan's grep didn't surface) — fix any such fixture by adding
`journeyStops: null` (or a real value, if the test is about journey data
specifically), matching Task 4/6's own pattern.

- [ ] **Step 4: Manual smoke read**

Re-read the final `frontend/components/TrainJourney.tsx` and
`crates/api/src/routes/train.rs` diffs in full (`git diff main -- frontend/components/TrainJourney.tsx crates/api/src/routes/train.rs`)
against §4 and §3 of the spec, confirming: the timeline renders at top
level (not nested in one status branch), `pending`/`unresolved` are
visually unchanged from before this plan, and both route handlers degrade
`journeyStops` to `null` on a `build_journey_stops` error rather than
500ing the whole request.

- [ ] **Step 5: Request code review**

Use `superpowers:requesting-code-review` against the full diff on this
branch vs. `main`, covering both the spec's stated intent and the plan's
own Global Constraints. Address findings per
`superpowers:receiving-code-review` (verify before applying — this plan's
own author is not infallible, especially on the exact
Step-3/synthetic-terminus row count flagged in Task 2, and the
`attach_journey_stops_public` gate rewrite flagged in Task 3).

- [ ] **Step 6: Push the branch**

```bash
git push -u origin HEAD
```

Do NOT merge into `main` from this worktree (per the top-level task's own
instruction — a prior pipeline already confirmed this fails from here).
Report the branch name and the pushed commit back to the requester.

---

## Self-Review Notes (for the plan author, not a task to execute)

- **Spec coverage:** §0 (source selection) → Tasks 1-2. §1 (when a stop
  list is available) → Task 2's `build_journey_stops` gating +
  `attach_journey_stops`/`attach_journey_stops_public`'s own gates in
  Task 3. §2 (wire shape) → Tasks 2 (Rust) and 4 (TypeScript). §3 (backend
  merge mechanics) → Task 2. §4 (frontend restructuring) → Tasks 5-6. §5
  (decisions) → reflected in Task 2/6's code and doc comments. §6
  (testing) → one test block per task, plus Task 7's full-suite run. §7/§8
  (out of scope / open questions) → no task attempts any of them,
  confirmed by omission.
- **Type consistency check:** `JourneyStop`'s field names/types match
  exactly across Task 2 (Rust struct), Task 3 (both routes' attachment
  points), Task 4 (TypeScript interface), Task 5 (`JourneyTimeline`'s
  prop usage), and Task 6 (`TrainJourney`'s conditional). `build_journey_stops`'s
  signature is identical everywhere it's called (Task 3's two call sites).
  `queries::` function names/signatures introduced in Task 1 are used
  verbatim (no renaming) in Task 2.
