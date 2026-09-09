# Multi-Day `GET /public/trains/search` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `GET /public/trains/search` search a caller-chosen day within a
7-day-forward / 7-day-backward window of today, instead of always today.

**Architecture:** Extend `schedule-reference`'s existing, already
date-parametric `publish_schedule_destination_departures` call into a loop
over `today..=today+7`; raise `aggregator`'s
`schedule_destination_departures_retention_days` default from 2 to 8 so 7
days of backward search data survive with a 1-day safety margin; add one
new optional `date` query parameter to the route, which selects
`service_date` (defaulting to today) and gates the existing `now`-forward
default so it only applies when searching today; extend
`TrainSearchForm.tsx` with a `DatePickerInput` bounded to the same window.
No schema change, no index change, no cursor change — `search_schedule_calling_point_departures`
already takes `service_date` as a parameter.

**Tech Stack:** Rust (axum, sqlx, chrono, chrono-tz), Next.js/React
(TypeScript, Mantine `@mantine/dates`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md`

## Global Constraints

- Forward search/publish window: exactly **7 days** (today, today+1, …,
  today+7).
- Backward search window: exactly **7 days** (today, today−1, …, today−7),
  backed by a retention default of **8** days (§1.2 of the spec — 1 extra
  day of safety margin, mirroring this codebase's existing convention for
  this exact config field).
- `date` query param format: `"YYYY-MM-DD"`, parsed with
  `chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")`.
- A malformed `date`, or a syntactically valid `date` outside the supported
  window, is a `400`, never a `404` or a silent clamp.
- `from`/`to`'s `now`-forward default (`max(now, from)`) applies **only**
  when the resolved `service_date == today`; for any other date, `from`/`to`
  are plain inclusive bounds with no implicit `now` floor.
- Exactly one `chrono::Utc::now()` call per request in `get_trains_search`
  — `today` and `now` both come from that single
  `Utc::now().with_timezone(&chrono_tz::Europe::London)` read. Never add a
  second `Utc::now()`/`chrono::Utc::now()` call anywhere in this handler or
  its helpers.
- No changes to `crates/api/src/data/queries.rs`,
  `crates/schedule-query/src/resolve.rs`, the migrations, the cursor
  encoding, or the index — all already support an arbitrary `service_date`.
- Every new Rust `#[tokio::test] #[ignore]` DB test in
  `crates/api/src/routes/trains.rs` follows this file's existing pattern:
  `#[ignore = "requires a live database; run with \`DATABASE_URL=... cargo
  test -p api trains_search -- --ignored --test-threads=1\`"]`.
- Run `cargo build --workspace` and `cargo test --workspace` after every
  backend task; run `npx vitest run` after every frontend task.

---

## Task 1: `schedule-reference` — publish the forward window

**Files:**
- Modify: `crates/schedule-reference/src/main.rs:176-226`
  (`publish_cif_derived_products`)
- Test: `crates/schedule-reference/src/main.rs` (existing `#[cfg(test)] mod
  tests` block, starting line 538)

**Interfaces:**
- Consumes: `publish_schedule_destination_departures(client, config, index,
  today: chrono::NaiveDate, stanox_crs_records, internal_oauth)` — existing,
  unmodified, already accepts an arbitrary date.
- Produces: `forward_publish_dates(today: chrono::NaiveDate, forward_days:
  i64) -> Vec<chrono::NaiveDate>` — a new pure helper, used only inside
  `publish_cif_derived_products`. No other task depends on this function's
  name or signature.

- [ ] **Step 1: Write the failing test for the new pure helper**

Add to the existing `#[cfg(test)] mod tests` block in
`crates/schedule-reference/src/main.rs` (near the other pure-function tests
like `lines_to_publish_includes_a_line_with_at_least_one_tiploc_bearing_station`):

```rust
    #[test]
    fn forward_publish_dates_returns_today_through_today_plus_n_inclusive() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        let dates = forward_publish_dates(today, 3);
        assert_eq!(
            dates,
            vec![
                chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 11).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 12).unwrap(),
            ],
            "today plus 0..=3 days, in order, today first"
        );
    }

    #[test]
    fn forward_publish_dates_with_zero_forward_days_is_just_today() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        assert_eq!(forward_publish_dates(today, 0), vec![today]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-reference forward_publish_dates`
Expected: FAIL — `forward_publish_dates` is not defined.

- [ ] **Step 3: Implement `forward_publish_dates` and wire it into `publish_cif_derived_products`**

Add this new function directly above `publish_cif_derived_products`
(`crates/schedule-reference/src/main.rs:176`):

```rust
/// Forward publish window, in days, for `schedule_destination_departures`:
/// how many days beyond today this service also computes and publishes on
/// every cycle. See
/// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1.2.
/// The route-side search window
/// (`crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`) must be
/// kept in sync with this value by hand -- there is no shared constant
/// across the `api`/`schedule-reference` crate boundary, matching this
/// codebase's existing per-crate-constant convention (e.g.
/// `MAX_DEPARTURES_PER_STATION` here vs. `MAX_SEARCH_LIMIT` in `api`).
const DESTINATION_DEPARTURES_FORWARD_DAYS: i64 = 7;

/// `today..=today+forward_days`, inclusive, today first. Pure and
/// unit-testable without a mock HTTP server or a `ScheduleIndex`, same
/// convention as `lines_to_publish` just below it in this file.
fn forward_publish_dates(today: chrono::NaiveDate, forward_days: i64) -> Vec<chrono::NaiveDate> {
    (0..=forward_days)
        .map(|offset| today + chrono::Duration::days(offset))
        .collect()
}
```

Then change `publish_cif_derived_products`'s body
(`crates/schedule-reference/src/main.rs:204-225`) from:

```rust
    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    // Third CIF-derived product off the SAME one-per-cycle ScheduleIndex
    // and the SAME `today` -- the design doc's Approach B is explicit that
    // this must not trigger a second parse or a resident index.
    publish_schedule_destination_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
}
```

to:

```rust
    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    // Third CIF-derived product off the SAME one-per-cycle ScheduleIndex --
    // the design doc's Approach B is explicit that this must not trigger a
    // second parse or a resident index. Unlike the two products above,
    // this one publishes a WINDOW of dates, not just `today`: see
    // docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md
    // §1/§2. `publish_schedule_destination_departures` itself is
    // unmodified -- it already accepts an arbitrary date; only the number
    // of times it's called per cycle changes.
    for date in forward_publish_dates(today, DESTINATION_DEPARTURES_FORWARD_DAYS) {
        publish_schedule_destination_departures(
            client,
            config,
            &index,
            date,
            stanox_crs_records,
            internal_oauth,
        )
        .await;
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p schedule-reference forward_publish_dates`
Expected: PASS (both new tests).

- [ ] **Step 5: Run the full crate's test suite**

Run: `cargo test -p schedule-reference`
Expected: PASS, no regressions (existing tests for
`publish_schedule_destination_departures`, `schedule_destination_departures_rows`,
etc. are untouched by this change since that function's own signature and
body are unmodified).

- [ ] **Step 6: Build the whole workspace**

Run: `cargo build --workspace`
Expected: succeeds.

- [ ] **Step 7: Commit**

```bash
git add crates/schedule-reference/src/main.rs
git commit -m "$(cat <<'EOF'
Publish schedule_destination_departures for a 7-day forward window

Extends the existing single-date publish into a loop over today..=today+7,
reusing the same per-cycle ScheduleIndex and the unmodified, already
date-parametric publish_schedule_destination_departures. See
docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1-2.
EOF
)"
```

---

## Task 2: `aggregator` — retention default 2 → 8

**Files:**
- Modify: `crates/aggregator/src/config.rs:115-132`
- Modify: `charts/distant-signal/values.yaml:668` (the
  `scheduleDestinationDeparturesRetentionDays: 2` line, plus its own
  preceding comment block)
- Modify: `charts/distant-signal/templates/aggregator-deployment.yaml:91-98`
  (the comment above `SCHEDULE_DESTINATION_DEPARTURES_RETENTION_DAYS`, which
  currently states "the default is 2")
- Modify: `crates/aggregator/src/queries.rs` (the doc comment on
  `prune_schedule_destination_departures`, `:513-528`, which currently
  argues "2 rather than 1... nothing reads a past service date... no
  reason to raise it further" — no longer accurate once the API route can
  ask for a past date)

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing new — this task only changes default values and their
  justifying doc comments, no signatures change.

- [ ] **Step 1: Change the Rust default**

In `crates/aggregator/src/config.rs`, replace the doc comment and default on
`schedule_destination_departures_retention_days`
(currently lines 105-132; read the file first to get exact current line
numbers, since Task 1 doesn't touch this file) from:

```rust
    /// to enforce an RDM licensing safeguard for TRUST Train Movements
    /// data -- a constraint that does not apply to CIF SCHEDULE timetable
    /// data at all. Copying the number would copy a restriction that isn't
    /// real here while giving up the margin that is: `service_date` is a
    /// RAIL day, which crosses midnight, and a CIF delivery can land late,
    /// so a 1-day window can delete the only published day shortly before
    /// its replacement arrives. 2 covers both edges.
    ///
    /// Nothing reads a past service date -- every read computes `today`
    /// server-side -- so this window protects the producer's edges, not a
    /// consumer, and there is no reason to raise it further. At ~377,000
    /// rows per day, 2 days is ~750,000 rows and ~80-120MB with the index.
    ///
    /// Unlike `trust_event_backlog_retention_days` there is deliberately NO
    /// warning emitted when this is configured higher: nothing legal is at
    /// stake, only disk.
    #[arg(long, env, default_value_t = 2)]
    pub schedule_destination_departures_retention_days: i64,
```

to:

```rust
    /// to enforce an RDM licensing safeguard for TRUST Train Movements
    /// data -- a constraint that does not apply to CIF SCHEDULE timetable
    /// data at all. Copying the number would copy a restriction that isn't
    /// real here while giving up the margin that is: `service_date` is a
    /// RAIL day, which crosses midnight, and a CIF delivery can land late,
    /// so a too-tight window can delete a still-searchable day shortly
    /// before its replacement arrives.
    ///
    /// `GET /public/trains/search` can now search up to 7 days INTO THE
    /// PAST (`crates/api/src/routes/trains.rs::SEARCH_WINDOW_BACKWARD_DAYS`)
    /// -- unlike the "nothing reads a past service date" reasoning this
    /// default used to be justified by, this window now has a real reader.
    /// 8, not 7: one extra day of safety margin beyond the search window,
    /// the same reasoning this field's default has always used (previously
    /// "2 rather than 1" for the identical reason), so a boundary date
    /// can't flake into a 404 if `aggregator`'s prune cycle runs against
    /// that date moments before a request for it lands. See
    /// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md
    /// §1.2/§3.
    ///
    /// At ~377,000 rows per day, 8 days is ~3,016,000 rows and roughly
    /// 600-675MB with the index (§1.3 of the design doc above).
    ///
    /// Unlike `trust_event_backlog_retention_days` there is deliberately NO
    /// warning emitted when this is configured higher: nothing legal is at
    /// stake, only disk.
    #[arg(long, env, default_value_t = 8)]
    pub schedule_destination_departures_retention_days: i64,
```

- [ ] **Step 2: Update the existing retention test's expectations, if any hardcode `2`**

Run: `grep -n "schedule_destination_departures_retention_days" crates/aggregator/src/config.rs crates/aggregator/src/main.rs`

If any test or default-args fixture in `crates/aggregator/src/main.rs` or
`crates/aggregator/src/config.rs` hardcodes the literal `2` for this field
(e.g. a `ServiceArguments`/`Config` test fixture constructor), update it to
`8` so it matches the new default. (The `prune_schedule_destination_departures`
tests in `crates/aggregator/src/queries.rs:3229-3320` already pass an
explicit `retention_days` argument per-test and do not depend on this
default — confirm this by reading those two tests before assuming no change
is needed there.)

- [ ] **Step 3: Update `prune_schedule_destination_departures`'s own doc comment**

In `crates/aggregator/src/queries.rs`, the doc comment on
`prune_schedule_destination_departures` (currently `:513-528`) says:

```rust
/// Nothing reads a past service date: every read of this table computes
/// `today` server-side, so the window exists to protect the PRODUCER's
/// edges, not a consumer -- see `Config::schedule_destination_departures_retention_days`
/// for why the default is 2 rather than 1.
```

Replace with:

```rust
/// `GET /public/trains/search` now reads past service dates too (up to 7
/// days back, `crates/api/src/routes/trains.rs::SEARCH_WINDOW_BACKWARD_DAYS`)
/// -- this window used to exist only to protect the PRODUCER's edges, and
/// now also has to keep a real consumer's supported range intact. See
/// `Config::schedule_destination_departures_retention_days` for why the
/// default is 8, one day more than that 7-day window strictly needs.
```

- [ ] **Step 4: Update the Helm chart's default value and comment**

In `charts/distant-signal/values.yaml`, find the
`scheduleDestinationDeparturesRetentionDays: 2` line (currently `:668`) and
its preceding comment block, and change to:

```yaml
  # -- Retention for schedule_destination_departures. NOT a licensing
  # safeguard (unlike trustEventBacklogRetentionDays above) -- purely disk
  # and keeping GET /public/trains/search's 7-day backward search window
  # intact with a 1-day safety margin. See
  # Config::schedule_destination_departures_retention_days
  # (crates/aggregator/src/config.rs) and
  # docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md.
  scheduleDestinationDeparturesRetentionDays: 8
```

In `charts/distant-signal/templates/aggregator-deployment.yaml`, find the
comment directly above `SCHEDULE_DESTINATION_DEPARTURES_RETENTION_DAYS`
(currently `:91-97`, currently reads "for why the default is 2 rather than
1") and change it to:

```yaml
            # NOT a licensing safeguard, unlike the setting directly above --
            # keeps GET /public/trains/search's 7-day backward search
            # window intact with a 1-day safety margin. See
            # Config::schedule_destination_departures_retention_days
            # (crates/aggregator/src/config.rs) for why the default is 8.
            - name: SCHEDULE_DESTINATION_DEPARTURES_RETENTION_DAYS
              value: {{ .Values.aggregator.scheduleDestinationDeparturesRetentionDays | quote }}
```

- [ ] **Step 5: Build and test**

Run: `cargo build --workspace && cargo test -p aggregator`
Expected: succeeds, no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/aggregator/src/config.rs crates/aggregator/src/queries.rs \
        charts/distant-signal/values.yaml \
        charts/distant-signal/templates/aggregator-deployment.yaml
git commit -m "$(cat <<'EOF'
Raise schedule_destination_departures retention default 2 -> 8

GET /public/trains/search can now search up to 7 days into the past, so
this table's retention window has a real reader for the first time -- not
just a producer-side safety margin. 8 keeps the same +1-day margin
convention this field already used, applied against the new 7-day search
window instead of the old "nothing reads the past" baseline. See
docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1/§3.
EOF
)"
```

---

## Task 3: `api` route — the `date` parameter

**Files:**
- Modify: `crates/api/src/routes/trains.rs` (whole file: module doc, params
  struct, handler, `db_tests`)

**Interfaces:**
- Consumes: `queries::search_schedule_calling_point_departures(pool,
  station_crs, service_date: chrono::NaiveDate, scheduled_from, ...)` —
  existing, unmodified; `service_date` was always a parameter, just always
  called with `today` before this task.
- Produces: two new `pub(crate)`-visibility-equivalent route-local
  constants, `SEARCH_WINDOW_FORWARD_DAYS: i64 = 7` and
  `SEARCH_WINDOW_BACKWARD_DAYS: i64 = 7`, referenced by Task 2's updated doc
  comments (no other code depends on them; they are `const`, not `pub`).

- [ ] **Step 1: Write the failing tests**

Add to `crates/api/src/routes/trains.rs`'s existing `#[cfg(test)] mod
db_tests` block (after `trains_search_hides_a_departure_inside_the_utc_vs_london_gap`,
which ends the file today):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_date_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&date=not-a-date").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("date"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_date_outside_the_supported_window() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let too_far_future = today + chrono::Duration::days(8);
        let too_far_past = today - chrono::Duration::days(8);

        let (status, body) = get(
            &pool,
            &format!("/trains/search?station=ZRB&date={}", too_far_future.format("%Y-%m-%d")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("date"), "400 body should name the field: {body}");

        let (status, _) = get(
            &pool,
            &format!("/trains/search?station=ZRB&date={}", too_far_past.format("%Y-%m-%d")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_accepts_a_date_exactly_at_the_edge_of_the_window() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let edge_future = today + chrono::Duration::days(7);
        let edge_past = today - chrono::Duration::days(7);

        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE service_date IN ($1, $2)",
        )
        .bind(edge_future)
        .bind(edge_past)
        .execute(&pool)
        .await
        .expect("cleanup edge-date fixtures");

        for (date, uid) in [(edge_future, "C30001"), (edge_past, "C30002")] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(date)
            .bind("WAT")
            .bind(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap())
            .bind(uid)
            .bind("ZRB")
            .bind(Option::<&str>::None)
            .execute(&pool)
            .await
            .expect("seed edge-date fixture row");

            let (status, body) = get(
                &pool,
                &format!("/trains/search?station=ZRB&date={}", date.format("%Y-%m-%d")),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "exactly 7 days out must be inside the window: {body}");
            let rows = results(&body);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["uid"], uid);
        }

        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE service_date IN ($1, $2)",
        )
        .bind(edge_future)
        .bind(edge_past)
        .execute(&pool)
        .await
        .expect("cleanup edge-date fixtures");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_now_forward_only_when_date_is_today() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let tomorrow = today + chrono::Duration::days(1);

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");

        // A row scheduled at the very start of tomorrow -- long "in the
        // past" relative to today's current clock time, which is exactly
        // the case that must NOT be now-forward-filtered once `date` picks
        // a day other than today.
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tomorrow)
        .bind("WAT")
        .bind(chrono::NaiveTime::from_hms_opt(0, 5, 0).unwrap())
        .bind("C30003")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed tomorrow's early-morning fixture row");

        let (status, body) = get(
            &pool,
            &format!("/trains/search?station=ZRB&date={}", tomorrow.format("%Y-%m-%d")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();
        assert!(
            uids.contains(&"C30003".to_string()),
            "a 00:05 row on a FUTURE date must not be hidden by today's now-forward filter: {uids:?}"
        );

        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(tomorrow)
            .execute(&pool)
            .await
            .expect("cleanup tomorrow's fixture");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_omitting_date_still_defaults_to_today() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            results(&body).len(),
            2,
            "identical to the existing today-only behavior when `date` is absent"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_404_for_an_unpublished_in_window_date_names_that_date() {
        let pool = connect().await;
        let today = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .date_naive();
        let target = today + chrono::Duration::days(3);
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(target)
            .execute(&pool)
            .await
            .expect("ensure target date has no rows");

        let (status, body) = get(
            &pool,
            &format!("/trains/search?station=ZRB&date={}", target.format("%Y-%m-%d")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains(&target.format("%Y-%m-%d").to_string()),
            "the 404 should name the actually-requested date, not always say 'today': {body}"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api trains_search -- --ignored --test-threads=1`
Expected: the new tests FAIL (compile error first, since `date` isn't a
field on `TrainSearchParams` yet — fix compilation, then confirm the
behavioral failures, particularly `trains_search_404_for_an_unpublished_in_window_date_names_that_date`,
which will fail against the *current* 404 body text even once it compiles).

- [ ] **Step 3: Add the `date` field, the window constants, and parsing/validation**

In `crates/api/src/routes/trains.rs`, add two new constants near
`MAX_SEARCH_LIMIT` (`:83`):

```rust
/// Forward search window, in days: the furthest future `date` this route
/// will accept. Must be kept in sync by hand with `schedule-reference`'s
/// own forward-publish loop
/// (`crates/schedule-reference/src/main.rs::DESTINATION_DEPARTURES_FORWARD_DAYS`)
/// -- there is no shared constant across the crate boundary, matching this
/// codebase's existing per-crate-constant convention. See
/// docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §1.2.
const SEARCH_WINDOW_FORWARD_DAYS: i64 = 7;

/// Backward search window, in days. Must not exceed
/// `Config::schedule_destination_departures_retention_days`
/// (`crates/aggregator/src/config.rs`, currently 8, one more than this
/// value) or a date this route claims to support could 404 anyway because
/// its rows have already been pruned.
const SEARCH_WINDOW_BACKWARD_DAYS: i64 = 7;
```

Add a `date` field to `TrainSearchParams` (`:85-124`), directly after
`station`:

```rust
    /// Optional, `"YYYY-MM-DD"`. Selects which `service_date` this search
    /// runs against; defaults to today (London-local, computed from the
    /// same single `Utc::now()` read this handler already uses for
    /// everything else -- see this file's own module doc comment on
    /// timezone handling and git history `baa4e75`/`8250a9a`). Must be
    /// within `SEARCH_WINDOW_BACKWARD_DAYS` days ago and
    /// `SEARCH_WINDOW_FORWARD_DAYS` days from today, inclusive, or this
    /// 400s -- a date outside the supported window is a request this
    /// deployment has already decided it can never answer, not a "nothing
    /// found" case, so it is NOT a 404.
    date: Option<String>,
```

Add a parsing helper directly below `normalize_time` (`:130-138`):

```rust
/// Parses and window-bounds a caller-supplied `"YYYY-MM-DD"` `date`
/// against `today`. `today` is always the caller's single
/// `Utc::now().with_timezone(&Europe::London)`-derived value -- see this
/// function's own call site for why a second `Utc::now()` read must never
/// be introduced here.
fn normalize_date(
    raw: &str,
    today: chrono::NaiveDate,
) -> Result<chrono::NaiveDate, (StatusCode, String)> {
    let parsed = chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| (StatusCode::BAD_REQUEST, "date must be YYYY-MM-DD".to_string()))?;
    let earliest = today - chrono::Duration::days(SEARCH_WINDOW_BACKWARD_DAYS);
    let latest = today + chrono::Duration::days(SEARCH_WINDOW_FORWARD_DAYS);
    if parsed < earliest || parsed > latest {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "date must be within {SEARCH_WINDOW_BACKWARD_DAYS} days ago and \
                 {SEARCH_WINDOW_FORWARD_DAYS} days from today"
            ),
        ));
    }
    Ok(parsed)
}
```

- [ ] **Step 4: Wire `date` into the handler and gate the now-forward default**

In `get_trains_search` (`:213-319`), change this block:

```rust
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
```

to:

```rust
    // `today` and `now` are deliberately read from ONE
    // `Utc::now().with_timezone(...)` call rather than two independent
    // `Utc::now()` calls -- see this same reasoning's original writeup in
    // this route's git history (baa4e75) for why two independent reads can
    // disagree about which calendar day it is around the UTC/London
    // midnight boundary during British Summer Time. `date` (if supplied)
    // is validated against THIS SAME `today`, never a second, independently
    // computed one -- see
    // docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §6.
    let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
    let today = london_now.date_naive();
    let now = london_now.time();

    let service_date = match params.date.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => normalize_date(raw, today)?,
        None => today,
    };

    // The `now`-forward default only makes sense when searching TODAY --
    // for any other date, forward or backward, there is no "now" to be
    // forward of, and applying today's clock time to a different date's
    // rows would silently and incorrectly filter them by the wrong day's
    // clock. See the design doc's §5.
    let scheduled_from = if service_date == today {
        match from_time {
            Some(from) => std::cmp::max(now, from),
            None => now,
        }
    } else {
        from_time.unwrap_or(chrono::NaiveTime::MIN)
    };

    let Some(page) = queries::search_schedule_calling_point_departures(
        &app.database,
        &station,
        service_date,
        scheduled_from,
```

- [ ] **Step 5: Make the 404 message name the requested date**

Directly below that call, change:

```rust
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for today".to_string(),
        ));
    };
```

to:

```rust
    else {
        let day_description = if service_date == today {
            "today".to_string()
        } else {
            service_date.format("%Y-%m-%d").to_string()
        };
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule data has been published for {day_description}"),
        ));
    };
```

- [ ] **Step 6: Update the module doc comment's "always today" claim**

In this file's module doc comment (`:34-35`), change:

```rust
//! There is deliberately NO date parameter: like
//! `get_station_schedule_departures`, this is "always today, server-side".
```

to:

```rust
//! `date` (`"YYYY-MM-DD"`, optional) selects which `service_date` this
//! search runs against, defaulting to today -- but only within a bounded
//! window (`SEARCH_WINDOW_BACKWARD_DAYS`/`SEARCH_WINDOW_FORWARD_DAYS`
//! below), not the whole published timetable: a date outside that window
//! is a `400`. See
//! docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md.
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api trains_search -- --ignored --test-threads=1`
Expected: PASS — all new tests, plus all 19 pre-existing tests in this file
(they all omit `date`, so they exercise the unchanged default-to-today
path).

- [ ] **Step 8: Run the full non-DB test suite and the workspace build**

Run: `cargo build --workspace && cargo test --workspace`
Expected: succeeds, no regressions (the DB-gated tests are `#[ignore]`d by
default and don't run here; they were already verified in Step 7).

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/trains.rs
git commit -m "$(cat <<'EOF'
Add an optional date parameter to GET /public/trains/search

Defaults to today, unchanged from existing behavior. Bounded to a 7-day
forward / 7-day backward window; anything outside it is a 400, not a 404.
The existing now-forward default on from/to now only applies when the
resolved date is today -- browsing a different day shows the whole day.
Reuses the existing single-Utc::now()-read pattern from baa4e75/8250a9a
for both the default and the window-bounds check. See
docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §5-6.
EOF
)"
```

---

## Task 4: Frontend — date picker on `TrainSearchForm`

**Files:**
- Modify: `frontend/components/TrainSearchForm.tsx`
- Modify: `frontend/components/TrainSearchForm.test.tsx`
- Modify: `frontend/app/trains/page.tsx`

**Interfaces:**
- Consumes: `GET /api/trains/search` now additionally accepts `?date=YYYY-MM-DD`
  (Task 3); absent behaves identically to before.
- Produces: `TrainSearchForm` gains a new optional prop, `initialDate?:
  string`, alongside its existing `initialStation`/`initialOrigin`/
  `initialDestination`.

- [ ] **Step 1: Add the `@mantine/dates` mock and the failing tests**

`TrainSearchForm.test.tsx` does not import or mock `@mantine/dates` today
(confirmed: grep the file for `@mantine/dates` first — it has no hits).
`TrackTrainForm.test.tsx:62-97` already solved exactly this problem for
`DateTimePicker`, with a documented reason (`fireEvent.change` needs a real
`<input>`, and `DatePickerInput`'s real popover-calendar control isn't
one). Add the same style of stand-in, adapted for `DatePickerInput`'s props
(`value: string | null`, `onChange: (value: string | null) => void`, same
shape `DateTimePicker` already uses), near the top of
`TrainSearchForm.test.tsx`, after the existing `next/navigation` mock:

```tsx
// See TrackTrainForm.test.tsx's identical mock (lines 62-97) for why a
// thin stand-in is used instead of driving the real popover calendar:
// fireEvent.change needs a real <input>, and DatePickerInput's real
// control isn't one. Keeps the same onChange(string | null) contract
// TrainSearchForm actually depends on.
vi.mock('@mantine/dates', () => ({
  DatePickerInput: ({
    label,
    value,
    onChange,
    description,
  }: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
    description?: string;
  }) => (
    <div>
      <label htmlFor="test-search-date">{label}</label>
      <input
        id="test-search-date"
        value={value ?? ''}
        onChange={(event) => onChange(event.target.value || null)}
      />
      {description && <p>{description}</p>}
    </div>
  ),
}));
```

Then add two new tests inside the existing `describe('TrainSearchForm', ...)`
block, next to `'sends only the station when no optional filter is set'`:

```tsx
  it('includes the selected date in the search request', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.change(screen.getByLabelText('Date (optional)'), { target: { value: '2026-09-16' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN&date=2026-09-16'),
    );
  });

  it('omits date from the search request when no date is picked', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialStation="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?station=MAN'));
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run TrainSearchForm`
Expected: FAIL — `DatePickerInput` isn't imported/rendered by
`TrainSearchForm` yet, so no element labelled `'Date (optional)'` exists.

- [ ] **Step 3: Add the `DatePickerInput` and wire it into `searchParams()`**

In `frontend/components/TrainSearchForm.tsx`, add the import alongside the
existing `Autocomplete`/etc. import:

```tsx
import { DatePickerInput } from '@mantine/dates';
```

Add `initialDate` to the component's prop destructuring and add new state,
directly after the existing `destinationCrs`/`setDestinationCrs` state:

```tsx
export function TrainSearchForm({
  initialStation = '',
  initialOrigin = '',
  initialDestination = '',
  initialDate = '',
  attachTicketId,
}: {
  initialStation?: string;
  initialOrigin?: string;
  initialDestination?: string;
  initialDate?: string;
  attachTicketId?: number;
}) {
  const [stationCrs, setStationCrs] = useState(initialStation);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [dateValue, setDateValue] = useState<string | null>(initialDate || null);
```

Add window bounds directly above the component (module scope, mirroring
`CRS_PATTERN`/`TIME_PATTERN`):

```tsx
/** Mirrors the backend's exact window --
 * `crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`/
 * `SEARCH_WINDOW_BACKWARD_DAYS` -- so the picker can never construct a
 * request the server will 400. Computed once per render from `dayjs()`,
 * consistent with this file's existing `today` computation just below. */
function dateWindow() {
  return {
    minDate: dayjs().subtract(7, 'day').format('YYYY-MM-DD'),
    maxDate: dayjs().add(7, 'day').format('YYYY-MM-DD'),
  };
}
```

In `searchParams()`, add `date` right after the required `station` param:

```tsx
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (dateValue) params.set('date', dateValue);
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
```

Add the `DatePickerInput` to the form's JSX, directly after the `Station`
`Autocomplete` and before `Departing from`:

```tsx
      <DatePickerInput
        label="Date (optional)"
        placeholder="Today"
        description="Search a different day, up to a week either side of today."
        value={dateValue}
        onChange={setDateValue}
        minDate={dateWindow().minDate}
        maxDate={dateWindow().maxDate}
        clearable
      />
```

- [ ] **Step 4: Update the "today" link/copy that assumed the search was always today**

`resultsContent()`'s "unpublished" branch currently reads:

```tsx
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          Today&apos;s scheduled timetable data isn&apos;t available yet — it may not have been
          published, or that station may not be one this feed covers.
        </Text>
      );
    }
```

Change to make the copy date-aware:

```tsx
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          {dateValue
            ? `Scheduled timetable data for ${dateValue} isn't available yet — it may not have been published, or that station may not be one this feed covers.`
            : "Today's scheduled timetable data isn't available yet — it may not have been published, or that station may not be one this feed covers."}
        </Text>
      );
    }
```

Also, the row-list link to `/train/{uid}/{today}` (`:141` `const today =
dayjs().format('YYYY-MM-DD')` and its use at `:282`) must link to the
*searched* date, not always today's date, once a date is picked. Change:

```tsx
  const today = dayjs().format('YYYY-MM-DD');
```

to:

```tsx
  // Every result links to the DATE THAT WAS ACTUALLY SEARCHED -- not
  // always today, now that a search can target a different day.
  const searchedDate = dateValue || dayjs().format('YYYY-MM-DD');
```

and its one use site:

```tsx
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${today}`}>
```

to:

```tsx
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${searchedDate}`}>
```

and the `TrackThisTrainButton` call directly below it, which also passes
`date={today}`, to `date={searchedDate}`.

- [ ] **Step 5: Wire `?date=` prefill in `frontend/app/trains/page.tsx`**

Following the existing pattern for `station`/`origin`/`destination`
(`:16-31`), add `date` to the `searchParams` type, destructure it, handle
the array case, and pass it through:

```tsx
  searchParams: Promise<{
    station?: string | string[];
    origin?: string | string[];
    destination?: string | string[];
    date?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { station, origin, destination, date, ticketId } = await searchParams;
  const stationParam = Array.isArray(station) ? station[0] : station;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const dateParam = Array.isArray(date) ? date[0] : date;
```

and pass `initialDate={dateParam}` to `<TrainSearchForm>` alongside the
existing `initialStation`/`initialOrigin`/`initialDestination` props.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `npx vitest run TrainSearchForm`
Expected: PASS — both new tests, plus all pre-existing `TrainSearchForm`
tests (they never set a date, so `dateValue` stays `null` and `date` is
never sent, matching prior behavior byte-for-byte).

- [ ] **Step 7: Run the full frontend test suite and build**

Run: `npx vitest run && npm run build`
Expected: both succeed, no regressions.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/TrainSearchForm.tsx \
        frontend/components/TrainSearchForm.test.tsx \
        frontend/app/trains/page.tsx
git commit -m "$(cat <<'EOF'
Add a date picker to TrainSearchForm

Wires GET /public/trains/search's new optional date parameter through the
UI: a DatePickerInput bounded to the same 7-day-either-side window the
backend enforces, omitted from the request entirely when left blank
(identical wire behavior to before this change). Every result row's link
now points at the date that was actually searched, not always today.
See docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md §8.
EOF
)"
```

---

## Task 5: Full verification pass

**Files:** none (verification only).

- [ ] **Step 1: Run the full Rust workspace build and unit tests**

Run: `cargo build --workspace && cargo test --workspace`
Expected: all succeed.

- [ ] **Step 2: Run the DB-backed API test suite**

```bash
sqlx migrate run --database-url postgres://lucy@localhost:5432/distant_signal_test
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
```

Expected: all pass, including every `trains_search_*` test added in Task 3
and the 19 pre-existing ones. If `sqlx migrate info`/`run` reports a
checksum mismatch on a migration this plan did not touch, investigate
before assuming it's this work's fault — the shared local test database may
be concurrently used by another worktree's migration, per this repo's known
shared-test-DB caveat.

- [ ] **Step 3: Run the aggregator's DB-backed tests**

```bash
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p aggregator -- --ignored --test-threads=1
```

Expected: pass, including the pre-existing
`prune_schedule_destination_departures_*` tests (unaffected — they pass an
explicit `retention_days` per test).

- [ ] **Step 4: Run the frontend test suite and build**

```bash
cd frontend && npx vitest run && npm run build
```

Expected: both succeed.

- [ ] **Step 5: Record results**

No commit for this task — it's verification only. Report the exact
commands run and their pass/fail outcome in the final summary handed back
to the user, per this pipeline's own reporting requirement.
