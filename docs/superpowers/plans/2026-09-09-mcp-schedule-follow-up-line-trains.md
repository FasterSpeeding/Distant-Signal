# Line-Level Live-Status Route Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `GET /public/lines/{id}/trains?date=`, a public, unauthenticated
route returning every scheduled UID on a line for one rail day (from
`schedule_line_population`) paired with its live status from the shared
`trains`/`train_current_state` tables, where one already exists — closing
the N+1 gap between the schedule-only `GET /public/lines/{id}/schedule`
and the single-train `GET /Train/by-uid/{uid}/{date}`.

**Architecture:** One new batched data-layer query
(`trains::get_public_train_states_for_line`, structurally the existing
`get_public_train_state` widened to `WHERE train_uid = ANY($1)`), one new
pure JSON-shaping helper (`render::line_train_json`, following this
crate's established "opaque pass-through + hand-built camelCase overlay"
convention already used by `schedule_departure_json`/
`calling_point_departure_json`), and one new thin route handler in
`routes/lines.rs` next to its existing `/lines/{id}/schedule` sibling —
reusing that file's own `ScheduleQuery`/`resolve_schedule_date` for the
`?date=` parameter. No migration, no new table, no producer/consumer
change in any other crate.

**Tech Stack:** Rust, axum, sqlx (runtime-checked queries, not
`query!`/`query_as!` macros — this crate has no `.sqlx` query cache),
Postgres, `serde_json`.

**Spec:** `docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md`
(§5.3 is the section this plan implements directly)

## Global Constraints

- No new migration. This plan reads only tables that already exist:
  `schedule_line_population`, `trains`, `train_current_state`, `stations`.
- The new route is public and unauthenticated — no `AuthenticatedUser`
  extractor, no access-group check — matching every sibling reference-data
  route in `routes/lines.rs`/`routes/trains.rs`.
- The new route performs **no write** — it must never call
  `trains::find_or_create_train`/`find_or_create_trains_batch`. A
  `liveStatus: null` for a train with no existing `trains` row is the
  correct, honest response (spec §5.3's "no write side effect" decision).
- `api` stays undeserialized against `schedule_line_population.population`
  — pluck only the known `uid` field via `serde_json::Value::get`, never
  add a `schedule-query` crate dependency to `api` or deserialize into
  `schedule_query::LinePopulationEntry`. Same posture `get_line_schedule`
  already established, reused here.
- DB-backed tests are written `#[ignore]`d, following this crate's
  existing `db_tests` convention throughout `routes/lines.rs`/
  `data/trains.rs`. Run them with:
  `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`
  (run `sqlx migrate run` first if any migration is pending — none is
  added by this plan, but the shared local test database is used
  concurrently by other work this session).
- Every new/changed function gets a doc comment citing
  `docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md`
  §5.3, matching this codebase's existing citation convention (every file
  read while writing the spec does this).

---

### Task 1: Batched live-state query — `trains::get_public_train_states_for_line`

**Files:**
- Modify: `crates/api/src/data/trains.rs`

**Interfaces:**
- Consumes: nothing new — reuses the existing `PublicTrainState` struct
  and the same `trains`/`train_current_state`/`stations` join
  `get_public_train_state` already uses (same file, lines 302–330).
- Produces: `pub async fn get_public_train_states_for_line(pool: &PgPool, train_uids: &[String], service_date: NaiveDate) -> anyhow::Result<Vec<PublicTrainState>>` — Task 3's route handler calls this by name.

- [ ] **Step 1: Write the failing test**

Add to the `db_tests` module at the bottom of `crates/api/src/data/trains.rs`
(same module the existing `get_public_train_state_*` tests live in — insert
after `get_public_train_state_reads_the_shared_row_and_its_current_state`,
before `is_known_scheduled_train_is_false_for_an_unpublished_uid`):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids \
                -- --ignored`"]
    async fn get_public_train_states_for_line_returns_only_existing_rows_for_the_requested_uids() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-09".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-09T08:00:00Z".parse().unwrap();
        let calling_points = serde_json::json!(["EUS", "BHM"]);

        // One resolved train (has both schedule match and live state)...
        let resolved_id = find_or_create_train_with_schedule_match(
            &pool,
            "TEST-LINE-TRAINS-RESOLVED",
            service_date,
            "EUS",
            scheduled_departure,
            Some("BHM"),
            "line-a",
            &calling_points,
        )
        .await
        .expect("seed resolved trains row");
        mark_train_resolved(&pool, resolved_id, "1A11")
            .await
            .expect("mark_train_resolved");
        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, last_event_type, delay_minutes, \
                 next_calling_point, updated_at) \
             VALUES ($1, 'en_route', 'Watford Junction', 'DEPARTURE', 2, 'BHM', NOW())",
        )
        .bind(resolved_id)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        // ...and one UID that was requested but has NO trains row at all
        // (a scheduled service TRUST hasn't activated yet) -- must simply
        // be absent from the result, not an error and not a null-filled
        // placeholder row.
        let requested = vec![
            "TEST-LINE-TRAINS-RESOLVED".to_string(),
            "TEST-LINE-TRAINS-UNSEEN".to_string(),
        ];

        let states = get_public_train_states_for_line(&pool, &requested, service_date)
            .await
            .expect("get_public_train_states_for_line");

        assert_eq!(
            states.len(),
            1,
            "only the one UID with a real trains row should come back: {states:?}"
        );
        let state = &states[0];
        assert_eq!(state.train_uid, "TEST-LINE-TRAINS-RESOLVED");
        assert_eq!(state.trains_id, resolved_id);
        assert_eq!(state.train_id, Some("1A11".to_string()));
        assert_eq!(state.status, Some("en_route".to_string()));
        assert_eq!(state.delay_minutes, Some(2));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(resolved_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_public_train_states_for_line_returns_empty_for_an_empty_uid_list -- --ignored`"]
    async fn get_public_train_states_for_line_returns_empty_for_an_empty_uid_list() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-09".parse().unwrap();

        let states = get_public_train_states_for_line(&pool, &[], service_date)
            .await
            .expect("get_public_train_states_for_line with no uids");

        assert!(states.is_empty(), "an empty uid list must short-circuit to no rows, not a malformed empty-array SQL query");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api get_public_train_states_for_line -- --ignored --test-threads=1`
Expected: compile failure — `get_public_train_states_for_line` is not
defined yet (`cannot find function` error).

- [ ] **Step 3: Implement the function**

Add to `crates/api/src/data/trains.rs`, directly after `get_public_train_state`
(after line 330, before the `#[cfg(test)] mod db_tests` block):

```rust
/// Batched sibling of [`get_public_train_state`] -- one query covering
/// every `train_uid` in `train_uids` for the same `service_date`, instead
/// of one query per train. Backs `GET /public/lines/{id}/trains?date=`
/// (docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
/// §5.3) -- the whole reason that route exists is to collapse what would
/// otherwise be one `GET /Train/by-uid` call per scheduled service on a
/// line into a single round trip.
///
/// Returns only rows that actually exist. Does NOT preserve `train_uids`'
/// own order, and does NOT synthesize a placeholder for a UID with no
/// `trains` row -- a scheduled service TRUST hasn't activated yet
/// legitimately has none. Callers key the result by `train_uid` /
/// `PublicTrainState::train_uid` themselves.
///
/// Never writes: unlike `routes::train::get_by_uid_and_date`'s
/// read-triggered `find_or_create_train` upsert, this function performs
/// no insert for a UID with no existing row -- see the spec's "no write
/// side effect" decision (§5.3).
pub async fn get_public_train_states_for_line(
    pool: &PgPool,
    train_uids: &[String],
    service_date: NaiveDate,
) -> anyhow::Result<Vec<PublicTrainState>> {
    if train_uids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, PublicTrainState>(
        "SELECT tr.id AS trains_id, tr.train_uid, tr.service_date, tr.origin_crs, so.name AS origin_name, \
                tr.destination_crs, sd.name AS destination_name, tr.scheduled_departure, \
                tr.calling_points, tr.train_id, \
                cs.status, cs.last_reported_location, cs.last_event_type, cs.delay_minutes, \
                cs.next_calling_point, cs.eta_next, cs.eta_source \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         LEFT JOIN stations so ON so.crs = UPPER(tr.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tr.destination_crs) \
         WHERE tr.train_uid = ANY($1) AND tr.service_date = $2",
    )
    .bind(train_uids)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api get_public_train_states_for_line -- --ignored --test-threads=1`
Expected: both new tests PASS (2 passed; 0 failed).

- [ ] **Step 5: Run the whole crate's fast (non-DB) test suite and build to check for regressions**

Run: `cargo build -p api && cargo test -p api`
Expected: builds clean; all non-`#[ignore]`d tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/trains.rs
git commit -m "$(cat <<'EOF'
Add trains::get_public_train_states_for_line, a batched live-status read

Backs the new GET /public/lines/{id}/trains route (spec 2026-09-09):
one query for every UID on a line's schedule, instead of one
GET /Train/by-uid call per train. Read-only -- never creates a trains
row for a UID with none.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: JSON-shaping helper — `render::line_train_json`

**Files:**
- Modify: `crates/api/src/render.rs`

**Interfaces:**
- Consumes: `crate::data::trains::PublicTrainState` (Task 1's return type;
  already exists — no change needed to that struct).
- Produces: `pub(crate) fn line_train_json(entry: &serde_json::Value, live: Option<&crate::data::trains::PublicTrainState>) -> serde_json::Value` — Task 3's route handler calls this by name, once per population entry.

- [ ] **Step 1: Write the failing test**

Add to `render.rs`'s existing `#[cfg(test)] mod tests` block (append after
`schedule_departure_json_null_destination_crs_stays_null`, the last test in
the file):

```rust
    #[test]
    fn line_train_json_with_no_live_row_passes_the_population_entry_through_and_nulls_live_status()
     {
        let entry = serde_json::json!({
            "uid": "C10001",
            "calling_points": [
                {"tiploc": "EUSTON", "kind": "Origin", "booked_arrival": null, "booked_departure": "08:00:00", "is_half_minute_arrival": false, "is_half_minute_departure": false}
            ],
        });
        let json = line_train_json(&entry, None);
        assert_eq!(json["uid"], "C10001");
        assert_eq!(json["callingPoints"], entry["calling_points"]);
        assert!(json["liveStatus"].is_null());
    }

    #[test]
    fn line_train_json_with_a_live_row_attaches_live_status_in_camel_case() {
        use crate::data::trains::PublicTrainState;

        let entry = serde_json::json!({"uid": "C10002", "calling_points": []});
        let live = PublicTrainState {
            trains_id: 42,
            train_uid: "C10002".to_string(),
            service_date: "2026-09-09".parse().unwrap(),
            origin_crs: Some("EUS".to_string()),
            origin_name: Some("London Euston".to_string()),
            destination_crs: Some("BHM".to_string()),
            destination_name: Some("Birmingham New Street".to_string()),
            scheduled_departure: Some("2026-09-09T08:00:00Z".parse().unwrap()),
            calling_points: None,
            train_id: Some("1A11".to_string()),
            status: Some("en_route".to_string()),
            last_reported_location: Some("Watford Junction".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(2),
            next_calling_point: Some("BHM".to_string()),
            eta_next: None,
            eta_source: None,
            journey_stops: None,
        };

        let json = line_train_json(&entry, Some(&live));
        assert_eq!(json["uid"], "C10002");
        assert_eq!(json["liveStatus"]["trainsId"], 42);
        assert_eq!(json["liveStatus"]["trainId"], "1A11");
        assert_eq!(json["liveStatus"]["originCrs"], "EUS");
        assert_eq!(json["liveStatus"]["destinationName"], "Birmingham New Street");
        assert_eq!(json["liveStatus"]["status"], "en_route");
        assert_eq!(json["liveStatus"]["lastReportedLocation"], "Watford Junction");
        assert_eq!(json["liveStatus"]["delayMinutes"], 2);
        assert_eq!(json["liveStatus"]["nextCallingPoint"], "BHM");
        // journeyStops/callingPoints must NOT appear inside liveStatus --
        // the batched route deliberately omits per-stop overlays (spec
        // §5.3, Decision point 3) and calling points come from the raw
        // population entry, not from PublicTrainState's own field.
        assert!(json["liveStatus"].get("journeyStops").is_none());
        assert!(json["liveStatus"].get("callingPoints").is_none());
    }

    #[test]
    fn line_train_json_missing_uid_on_the_population_entry_renders_null_not_a_panic() {
        let entry = serde_json::json!({"calling_points": []});
        let json = line_train_json(&entry, None);
        assert!(json["uid"].is_null());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api line_train_json`
Expected: compile failure — `line_train_json` is not defined yet.

- [ ] **Step 3: Implement the function**

Add to `crates/api/src/render.rs`, directly after `calling_point_departure_json`
(after its closing `}`, before the `#[cfg(test)] mod tests` block):

```rust
/// One `GET /public/lines/{id}/trains?date=` result entry -- an
/// unprocessed `schedule_line_population` entry (`entry`, the exact same
/// opaque-JSONB pass-through `get_line_schedule` already returns for the
/// whole population, unchanged shape) paired with live status from the
/// shared `trains`/`train_current_state` tables, when a row already
/// exists for that train_uid/date. See
/// docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
/// §5.3.
///
/// `live` is `None` for a train nobody has ever tracked, searched via
/// `GET /Train/by-uid`, or that TRUST hasn't activated yet -- an honest,
/// expected, common gap (this function never fabricates a status), not an
/// error. Deliberately does NOT surface `PublicTrainState::calling_points`
/// or `::journey_stops` inside `liveStatus` -- `callingPoints` at the top
/// level always comes from `entry` (the population's own calling points),
/// and per-stop journey overlays are out of scope for this batched route
/// (spec §5.3, Decision point 3); a caller wanting a specific train's full
/// overlay still calls `GET /Train/by-uid/{uid}/{date}` for that one
/// train.
pub(crate) fn line_train_json(
    entry: &Value,
    live: Option<&crate::data::trains::PublicTrainState>,
) -> Value {
    json!({
        "uid": entry.get("uid").cloned().unwrap_or(Value::Null),
        "callingPoints": entry.get("calling_points").cloned().unwrap_or(Value::Null),
        "liveStatus": live.map(|s| json!({
            "trainsId": s.trains_id,
            "trainId": s.train_id,
            "originCrs": s.origin_crs,
            "originName": s.origin_name,
            "destinationCrs": s.destination_crs,
            "destinationName": s.destination_name,
            "scheduledDeparture": s.scheduled_departure,
            "status": s.status,
            "lastReportedLocation": s.last_reported_location,
            "lastEventType": s.last_event_type,
            "delayMinutes": s.delay_minutes,
            "nextCallingPoint": s.next_calling_point,
            "etaNext": s.eta_next,
            "etaSource": s.eta_source,
        })),
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api line_train_json`
Expected: all three new tests PASS.

- [ ] **Step 5: Run the whole crate's fast test suite and build to check for regressions**

Run: `cargo build -p api && cargo test -p api`
Expected: builds clean; all non-`#[ignore]`d tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "$(cat <<'EOF'
Add render::line_train_json, the JSON shape for GET /public/lines/{id}/trains

Pass-through of one schedule_line_population entry plus an optional
camelCase liveStatus overlay from PublicTrainState -- deliberately
excludes calling_points/journeyStops from liveStatus (spec 2026-09-09,
Decision point 3).

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Route handler, wiring, and HTTP-layer tests

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Consumes: `queries::get_schedule_line_population` (existing, unchanged),
  `trains::get_public_train_states_for_line` (Task 1),
  `render::line_train_json` (Task 2), `ScheduleQuery`/`resolve_schedule_date`
  (existing, same file, lines 95–109).
- Produces: `GET /public/lines/{id}/trains?date=` — the whole feature's
  externally-visible surface. No later task depends on this handler's own
  name (it is not called from anywhere else).

- [ ] **Step 1: Write the failing tests**

Add to the `db_tests` module at the bottom of `crates/api/src/routes/lines.rs`
(insert directly after the existing `schedule_a_row_only_for_a_different_date_is_still_404_today`
test, before that module's closing `}`):

```rust
    /// Issues `GET /public/lines/{id}/trains`, with an optional `?date=`
    /// query string. Mirrors `get_line_schedule`'s own request-building.
    async fn get_line_trains(
        router: axum::Router,
        id: &str,
        date: Option<&str>,
    ) -> (StatusCode, Value) {
        let uri = match date {
            Some(date) => format!("/public/lines/{id}/trains?date={date}"),
            None => format!("/public/lines/{id}/trains"),
        };
        let request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(request).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_no_row_for_the_line_and_date_is_404_naming_both -- --ignored`"]
    async fn trains_no_row_for_the_line_and_date_is_404_naming_both() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-missing").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-missing", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let body = body
            .as_str()
            .expect("404 body is a plain string")
            .to_string();
        assert!(body.contains("test-trains-3-missing"), "body: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_a_population_with_no_trains_rows_returns_every_entry_with_null_live_status \
                -- --ignored`"]
    async fn trains_a_population_with_no_trains_rows_returns_every_entry_with_null_live_status() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-no-live").await;
        sqlx::query("DELETE FROM trains WHERE train_uid IN ('TEST-TRAINS-3-A', 'TEST-TRAINS-3-B')")
            .execute(&pool)
            .await
            .ok();

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {"uid": "TEST-TRAINS-3-A", "calling_points": [{"tiploc": "PADTON", "kind": "Origin", "booked_arrival": null, "booked_departure": "08:15:00", "is_half_minute_arrival": false, "is_half_minute_departure": false}]},
            {"uid": "TEST-TRAINS-3-B", "calling_points": []},
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-no-live")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-no-live", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 2);
        for entry in entries {
            assert!(entry["liveStatus"].is_null(), "entry: {entry:?}");
        }
        assert_eq!(entries[0]["uid"], "TEST-TRAINS-3-A");
        assert_eq!(
            entries[0]["callingPoints"],
            population[0]["calling_points"],
            "callingPoints must be the raw population entry, unchanged"
        );

        delete_schedule_population_fixture(&pool, "test-trains-3-no-live").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                trains_a_uid_with_an_existing_trains_row_gets_its_live_status_attached \
                -- --ignored`"]
    async fn trains_a_uid_with_an_existing_trains_row_gets_its_live_status_attached() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-trains-3-live").await;
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-TRAINS-3-LIVE'")
            .execute(&pool)
            .await
            .ok();

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {"uid": "TEST-TRAINS-3-LIVE", "calling_points": []},
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-trains-3-live")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let trains_id: (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date, train_id) \
             VALUES ($1, $2, $3) RETURNING id",
        )
        .bind("TEST-TRAINS-3-LIVE")
        .bind(today)
        .bind("1B22")
        .fetch_one(&pool)
        .await
        .expect("seed fixture trains row");
        sqlx::query(
            "INSERT INTO train_current_state \
                (trains_id, status, last_reported_location, delay_minutes, updated_at) \
             VALUES ($1, 'en_route', 'Reading', 5, NOW())",
        )
        .bind(trains_id.0)
        .execute(&pool)
        .await
        .expect("seed fixture train_current_state row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_trains(router, "test-trains-3-live", None).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("body is a JSON array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["uid"], "TEST-TRAINS-3-LIVE");
        assert_eq!(entries[0]["liveStatus"]["trainId"], "1B22");
        assert_eq!(entries[0]["liveStatus"]["status"], "en_route");
        assert_eq!(entries[0]["liveStatus"]["lastReportedLocation"], "Reading");
        assert_eq!(entries[0]["liveStatus"]["delayMinutes"], 5);

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id.0)
            .execute(&pool)
            .await
            .ok();
        delete_schedule_population_fixture(&pool, "test-trains-3-live").await;
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api trains_ -- --ignored --test-threads=1`
Expected: compile failure — no `/lines/{id}/trains` route exists, and
`get_line_trains` (the handler) is not defined.

- [ ] **Step 3: Add the imports, the route, and the handler**

In `crates/api/src/routes/lines.rs`, change the import block near the top
(currently lines 16–24) from:

```rust
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::{
    custom_lines::{self, NewCustomLine},
    queries,
};
```

to:

```rust
use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{App, Router};
use crate::auth::{AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::{
    custom_lines::{self, NewCustomLine},
    queries,
    trains::{self, PublicTrainState},
};
use crate::render::line_train_json;
```

Change `router()` (currently lines 28–44) from:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route(
            "/lines/{id}",
            axum::routing::get(get_line)
                .put(update_line)
                .delete(delete_line),
        )
        .route(
            "/lines/{id}/definition",
            axum::routing::get(get_line_definition),
        )
        .route(
            "/lines/{id}/schedule",
            axum::routing::get(get_line_schedule),
        )
}
```

to:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route(
            "/lines/{id}",
            axum::routing::get(get_line)
                .put(update_line)
                .delete(delete_line),
        )
        .route(
            "/lines/{id}/definition",
            axum::routing::get(get_line_definition),
        )
        .route(
            "/lines/{id}/schedule",
            axum::routing::get(get_line_schedule),
        )
        .route("/lines/{id}/trains", axum::routing::get(get_line_trains))
}
```

Add the handler directly after `get_line_schedule` (after its closing `}`,
before `get_line_definition`):

```rust
/// `GET /public/lines/{id}/trains?date=`: every scheduled UID on line `id`
/// for one rail day (from `schedule_line_population`, the same source
/// `get_line_schedule` reads), each paired with its live status from the
/// shared `trains`/`train_current_state` tables where one already exists.
/// See docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
/// §5.3 for the full design.
///
/// Closes the gap between `get_line_schedule` (schedule only, one line at
/// a time) and `routes::train::get_by_uid_and_date` (schedule + live
/// status, but one train at a time): without this route, "what's running
/// on this line right now" costs one `GET /Train/by-uid` call per
/// scheduled service. This route costs exactly two queries regardless of
/// how many trains the line has: one for the population, one batched
/// `trains::get_public_train_states_for_line` covering every UID in it.
///
/// Same 404 semantics as `get_line_schedule` -- an unknown/not-yet-
/// published `(id, date)` 404s naming both. Unlike `get_by_uid_and_date`,
/// this handler never writes: a UID with no existing `trains` row simply
/// renders `liveStatus: null` (an honest, expected gap -- see the spec's
/// Open question 2), never triggering a `find_or_create_train` upsert.
async fn get_line_trains(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let service_date = resolve_schedule_date(query.date, chrono::Utc::now().date_naive());
    let Some(population) = queries::get_schedule_line_population(&app.database, &id, service_date)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    };

    let entries = population.as_array().cloned().unwrap_or_default();
    let uids: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("uid").and_then(Value::as_str).map(str::to_string))
        .collect();

    let live_states = trains::get_public_train_states_for_line(&app.database, &uids, service_date)
        .await
        .map_err(internal_error)?;
    let live_by_uid: HashMap<&str, &PublicTrainState> = live_states
        .iter()
        .map(|s| (s.train_uid.as_str(), s))
        .collect();

    let result: Vec<Value> = entries
        .iter()
        .map(|entry| {
            let live = entry
                .get("uid")
                .and_then(Value::as_str)
                .and_then(|uid| live_by_uid.get(uid).copied());
            line_train_json(entry, live)
        })
        .collect();

    Ok(Json(result))
}
```

Note: `lines.rs` already has its own module-scope `use serde_json::{...}`?
No — check first: this file currently builds its JSON responses via typed
`#[derive(Serialize)]` structs, not `serde_json::json!`/`Value` directly
(unlike `render.rs`). The `Value` import added above is new to this file;
`json!` itself is not used here (only inside `render::line_train_json`),
so no `serde_json::json` import is needed in `lines.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api trains_ -- --ignored --test-threads=1`
Expected: all three new tests PASS (3 passed; 0 failed).

- [ ] **Step 5: Run the full existing `lines.rs`/`trains.rs` DB test suites to check for regressions**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`
Expected: every test in the crate passes, including the pre-existing
`schedule_*` tests in `lines.rs` and every test in `trains.rs`/`train.rs`.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "$(cat <<'EOF'
Add GET /public/lines/{id}/trains, closing the N+1 line-level live-status gap

Combines schedule_line_population (via get_schedule_line_population,
unchanged) with the new batched trains::get_public_train_states_for_line
read, so a caller gets every scheduled train on a line plus its live
status (where known) in one request instead of one GET /Train/by-uid
call per train. Public, unauthenticated, no write side effect -- see
docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
§5.3.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Full-suite verification and code review prep

**Files:** none (verification only)

- [ ] **Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: clean build, no warnings introduced by this plan's changes.

- [ ] **Step 2: Full non-DB test suite**

Run: `cargo test --workspace`
Expected: all tests pass (this plan added no code outside `crates/api`, so
no other crate's suite is expected to change behavior).

- [ ] **Step 3: Full DB-backed `api` test suite**

Run:
```bash
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
```
Expected: all `#[ignore]`d tests pass, including this plan's six new
tests (two in `data/trains.rs`, three in `routes/lines.rs`, plus the three
pure unit tests in `render.rs` which already run under the non-DB suite in
Step 2).

- [ ] **Step 4: `cargo clippy` sanity check**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean (or note and fix any new lint this plan's code trips —
this repo's existing files pass clippy clean, so a new warning localizes
to this plan's own additions).

- [ ] **Step 5: Confirm no frontend files were touched**

Run: `git diff --stat main -- frontend/`
Expected: empty output — this plan is `crates/api`-only, so no `npx
vitest run`/`npm run build` is needed per the task's own "if you touch
frontend code" condition.

This task has no commit of its own — it's a verification gate before
requesting code review (`superpowers:requesting-code-review`).
