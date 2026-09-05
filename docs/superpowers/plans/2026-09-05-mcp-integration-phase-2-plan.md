# MCP Integration Phase 2 (2a + 2b) — Two New Public Read Routes

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement exactly Phase 2a and Phase 2b of
`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md`
("the design doc") — Decisions 3 and 4 — inside `crates/api` only: two new,
narrow, unauthenticated `GET` routes over data that already exists and is
already correctly computed. No new persistence, no microservice change, no
MCP-repo (`distant-signal-mcp`) work of any kind.

**Architecture:** `GET /public/lines/{id}/schedule?date=` (Task 1) is added
to the existing `routes/lines.rs` — the file that already owns every
`/lines/{id}/...` path — and reads `queries::get_schedule_line_population`
directly, relaying its stored `population` JSONB value completely
unprocessed (the `api` crate has no dependency on `schedule-query`, the
crate that defines `LinePopulationEntry`, so there is no Rust type to
deserialize into even if this route wanted to). `GET /public/stanox-crs`
(Task 2) is a brand-new file, `routes/stanox_crs.rs`, mirroring
`island_of_ireland.rs`'s "small, dedicated, whole-table" shape, and reads
the exact same `queries::list_stanox_crs` the existing
internal-oauth-gated `GET /private/stanox-crs` already uses — same query,
different (public, no-credential) mount point, `search_stations`/
`data/reference.rs` untouched. Task 3 is end-to-end verification of both
routes together, including a real running-instance smoke test.

**Tech Stack:** Rust (axum, sqlx, serde_json), no new crate dependency, no
new migration.

**Spec:**
`docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md`
(Decisions 3 and 4 specifically; Decisions 1, 2, 5, 6, 7 and Phases 0/1/3
are explicitly out of scope for this plan — see Non-goals).

## Global Constraints

- **Both new routes are genuinely public and unauthenticated.** Neither is
  mounted through `private_router()`/`require_internal_oauth`
  (`crates/api/src/routes/mod.rs:68-85`) or any `AuthenticatedUser`/
  `OptionalAuthenticatedUser` extractor — same posture as every other
  reference-data route in `public_router()` (`/public/lines`,
  `/public/stations`, `/public/island-of-ireland/*`).
- **No new access group, anywhere, in either repo.** Nothing in this plan
  touches `internal_oauth_group_*`, `AccessGroupClaims`, or any MCP-side
  gate.
- **No microservice changes.** `schedule-reference`, `full-coverage-consumer`,
  `trust-consumer` are untouched — this plan only adds new `api`-side `GET`
  handlers over tables those services already write.
- **No new Postgres tables, columns, or migrations.** `schedule_line_population`
  (migration `20260904090000`) and `stanox_crs` (migration `20260901150000`)
  already exist with everything this plan needs.
- **`crates/api/src/data/reference.rs` and `crates/api/src/routes/reference.rs`
  (`search_stations`/`GET /public/stations?q=`) are not modified by this
  plan at all.** Phase 2b's bulk mirror is a fully independent route, read
  path, and file — this is the design doc's own Decision 4 reasoning
  (avoid coupling the hot-path type-ahead query to a second,
  independently-refreshed table), and Task 2 below adds no import, call,
  or reference to either file.
- **File scope.** Modified: `crates/api/src/routes/lines.rs`,
  `crates/api/src/routes/mod.rs`. Created:
  `crates/api/src/routes/stanox_crs.rs`. No other file in this repo
  changes — no `Cargo.toml`, no migration, no chart, no frontend file.
- **Testing.** `cargo build --workspace`; `cargo clippy --workspace
  --all-features` (the exact invocation `.github/workflows/ci.yml:98-102`'s
  `clippy` job runs — no `--all-targets`, no explicit `-D warnings` flag,
  since that job's own `auguwu/clippy-action` step provides the
  fail-the-check behavior, not a cargo-level flag); `cargo test --workspace`
  (ignored tests skipped, `.github/workflows/ci.yml:219-220`); then
  `cargo test -p api -- --ignored --test-threads=1` against a live,
  migrated Postgres (`.github/workflows/ci.yml:225-230`'s real CI sequence:
  `sqlx migrate run` from `crates/api`, then `cargo test -p api -p
  aggregator -- --ignored --test-threads=1` — this plan touches only `api`,
  so `-p api` alone is sufficient locally). Every new HTTP-layer test
  follows this crate's own established `#[ignore]`d `db_tests` convention
  (seed fixture rows in a reserved test-only key namespace, assert, delete
  the fixture) — no new test infrastructure invented.
- **Response casing is deliberately inconsistent between the two new
  routes, and that inconsistency is correct, not a bug to fix.**
  `GET /public/lines/{id}/schedule` returns `schedule_query::LinePopulationEntry`'s
  own snake_case field names (`uid`, `calling_points`, `booked_arrival`,
  ...) verbatim — `api` cannot re-shape it into this crate's usual
  camelCase convention without adding a `schedule-query` dependency it
  doesn't have today, and the design doc's own Decision 3 explicitly chose
  "pass straight through" over that. `GET /public/stanox-crs` returns
  `common::StanoxCrsRecord`'s fields, which are also snake_case (no
  `#[serde(rename_all = "camelCase")]` on that struct,
  `crates/common/src/lib.rs:761`) — same as `GET /private/stanox-crs`
  already does today, so this is not a new inconsistency Task 2
  introduces, only one Task 1 newly introduces for a different reason.

## Non-goals

- **Phase 0 and Phase 1** (wiring the five already-public Section B routes
  into new MCP tools; finishing the TRUST-corroboration `DsApiClient` fifth
  method) — both are `distant-signal-mcp`-repo-only work with zero `api`
  changes, per the design doc's own Decisions 1/2. Not touched here.
- **Phase 3a** (`train_movement_events.raw_body` actually carrying data)
  and **Phase 3b** (persisting/exposing per-train state for every
  full-coverage-enabled line via a new `full_coverage_train_state` table).
  The design doc's own Recommended phased scope explicitly defers 3b to
  "its own dedicated follow-up design pass," and this plan does not design
  or implement any part of either 3a or 3b.
- **Any `distant-signal-mcp` (the separate MCP adapter repo) work at all** —
  no new tool, no new client method, no new resource. The design doc's own
  "Unlocks" column names `get_line_timetable(lineId, date?)` and
  `resolve_station`'s tiploc-join improvement / `resolve_tiploc` as what
  Phase 2a/2b make possible on the MCP side — building those is separate,
  later work in that other repository, out of scope for this document.
- **Any access-group, OAuth, or MCP-side gating change.** Both new routes
  are fully public; Decision 7's `mcp-users`-vs-`mcp-live-boards`
  discussion in the design doc applies to Phase 1/3b tooling, not to
  anything built here.
- **Widening, refactoring, or adding fields to `GET /public/stations?q=`/
  `search_stations`.** Explicitly named and rejected as Phase 2b's own
  shape (design doc Decision 4, option (a) considered and not chosen).
- **A retention/pruning job for `schedule_line_population` or `stanox_crs`.**
  Both tables are already wholesale-replaced per producer cycle
  (`upsert_schedule_line_population`/`upsert_stanox_crs`'s own doc
  comments) — reading them publicly doesn't change their existing
  lifecycle, and this plan adds no new unbounded-growth table (contrast
  the design doc's Open question/risk 6, which is about `tracked_trains`/
  `train_movement_events`, an unrelated table Phase 3 territory).
- **Any change to `full-coverage-consumer`'s rollout scope, or measuring
  how many lines have `full_coverage_enabled: true` today.** Named in the
  design doc's Open question/risk 2 as relevant to Phase 3b specifically;
  irrelevant to Phase 2a, which reads `schedule_line_population` (written
  by `schedule-reference` for every line it processes, not gated by
  full-coverage enablement at all).
- **Measuring `stanox_crs`'s real row count/response size.** Named
  unmeasured in the design doc's Open question/risk 5. This plan's Task 3
  smoke test observes the real count once a local stack is up (cheap,
  already part of verification) but does not commission separate
  load-testing or pagination — `GET /public/island-of-ireland/stations`/
  `/lines` already established the "no pagination, the whole catalogue is
  fine to return in one response" precedent this route follows.

---

## Task 1: `GET /public/lines/{id}/schedule?date=` (Phase 2a)

**Files:** modify `crates/api/src/routes/lines.rs`.

Independent of Task 2 — different file, different table, no shared code.
Closes design doc Decision 3.

**Collision check (done this session):** `crates/api/src/routes/lines.rs`'s
current `router()` (lines 28-41) registers exactly `/lines`, `/lines/{id}`,
and `/lines/{id}/definition`. No existing route matches
`/lines/{id}/schedule`. Confirmed via direct read of the file and a
workspace grep for `schedule` under `crates/api/src/routes/` — the only
other `schedule*` paths in this crate are `/private/schedule-line-population`,
`/private/schedule-network-departures`, and
`/public/stations/{crs}/schedule-departures` (`routes/departures.rs`), none
of which share this route's path shape.

- [ ] **Step 1: Add `Query` to this file's axum imports.** Current
  (`lines.rs:16-19`):

```rust
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
```

  Change line 17 to:

```rust
use axum::extract::{Path, Query, State};
```

- [ ] **Step 2: Add the route registration.** Current `router()`
  (`lines.rs:28-41`):

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
}
```

  Change to:

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

- [ ] **Step 3: Add the handler and its pure date-resolution helper.**
  Insert immediately before `async fn get_line_definition(` (`lines.rs:91`
  in the file as currently read), i.e. right after the `LineDefinitionSummary`
  struct definition:

```rust
#[derive(Debug, Deserialize)]
struct ScheduleQuery {
    date: Option<chrono::NaiveDate>,
}

/// Resolves the effective service date for `GET /lines/{id}/schedule`:
/// the caller's explicit `?date=`, or `today` if omitted. Factored out as
/// a pure function so the "default to today" decision is testable without
/// a clock or a database -- same rationale as
/// `routes::reference::sanitize_query`.
fn resolve_schedule_date(
    requested: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> chrono::NaiveDate {
    requested.unwrap_or(today)
}

/// `GET /public/lines/{id}/schedule?date=`: the full CIF-derived stopping
/// pattern for every service on line `id`, for one rail day -- read
/// straight off `schedule_line_population` (`queries::get_schedule_line_population`).
/// See docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md
/// Decision 3.
///
/// Deliberately does NOT check `app.config.lines`/`custom_lines` first the
/// way `get_line_definition` does: `schedule_line_population` is keyed
/// purely by whatever `line_id` string `schedule-reference` published
/// under, with no foreign key to either catalogue or custom lines, so
/// there is nothing to disambiguate here -- an unknown, custom, or
/// not-yet-published catalogue `id` alike simply 404 for the same reason
/// ("no row for this key"), which is the same honesty split
/// `get_station_schedule_departures` already draws for
/// `schedule_network_departures`.
///
/// The response body is `schedule_line_population.population` relayed
/// completely unprocessed: `api` has no dependency on the `schedule-query`
/// crate (the crate that defines `LinePopulationEntry`/`CallingPoint`) at
/// all, so its JSON keys are that crate's own snake_case field names
/// (`uid`, `calling_points`, `booked_arrival`, `booked_departure`,
/// `is_half_minute_arrival`, `is_half_minute_departure`, `tiploc`, `kind`),
/// NOT this crate's usual camelCase convention -- see this plan's Global
/// Constraints for why that's a deliberate, not accidental, difference
/// from `get_station_schedule_departures`.
async fn get_line_schedule(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<ScheduleQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let service_date = resolve_schedule_date(query.date, chrono::Utc::now().date_naive());
    let Some(population) =
        queries::get_schedule_line_population(&app.database, &id, service_date)
            .await
            .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule population for line {id} on {service_date}"),
        ));
    };

    Ok(Json(population))
}
```

- [ ] **Step 4: Unit tests for `resolve_schedule_date`.** Add to the
  existing `#[cfg(test)] mod tests` block (`lines.rs`, after
  `a_tfl_line_with_no_nr_counterpart_is_not_suppressed`):

```rust
    #[test]
    fn resolve_schedule_date_uses_the_explicit_date_when_given() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        let requested = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        assert_eq!(resolve_schedule_date(Some(requested), today), requested);
    }

    #[test]
    fn resolve_schedule_date_defaults_to_today_when_absent() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap();
        assert_eq!(resolve_schedule_date(None, today), today);
    }
```

- [ ] **Step 5: Run the unit tests.**

```bash
cargo test -p api resolve_schedule_date
```

  Expected: 2 passed.

- [ ] **Step 6: HTTP-layer `db_tests`.** This crate's `lines.rs` already
  has a `db_tests` module with `test_app`/`test_router`/`cleanup_user`
  helpers (used by `get_line`/`get_line_definition`/`list_lines` tests) —
  reuse them rather than duplicating. Add a fixture-cleanup helper and a
  request helper alongside the existing `get_line_definition` request
  helper, then four tests, all appended at the end of the `db_tests`
  module:

```rust
    async fn delete_schedule_population_fixture(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM schedule_line_population WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_line_population rows");
    }

    /// Issues `GET /public/lines/{id}/schedule`, with an optional
    /// `?date=` query string. Mirrors `get_line_definition`'s own
    /// request-building/body-shape handling.
    async fn get_line_schedule(
        router: axum::Router,
        id: &str,
        date: Option<&str>,
    ) -> (StatusCode, Value) {
        let uri = match date {
            Some(date) => format!("/public/lines/{id}/schedule?date={date}"),
            None => format!("/public/lines/{id}/schedule"),
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
                schedule_no_row_for_the_line_and_date_is_404_naming_both -- --ignored`"]
    async fn schedule_no_row_for_the_line_and_date_is_404_naming_both() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-missing").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) =
            get_line_schedule(router, "test-schedule-2a-missing", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let body = body.as_str().expect("404 body is a plain string").to_string();
        assert!(body.contains("test-schedule-2a-missing"), "body: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_a_row_for_today_returns_the_raw_population_json_unchanged -- --ignored`"]
    async fn schedule_a_row_for_today_returns_the_raw_population_json_unchanged() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-today").await;

        let today = chrono::Utc::now().date_naive();
        let population = serde_json::json!([
            {
                "uid": "C12345",
                "calling_points": [
                    {
                        "tiploc": "WATRLMN",
                        "kind": "origin",
                        "booked_arrival": null,
                        "booked_departure": "08:15:00",
                        "is_half_minute_arrival": false,
                        "is_half_minute_departure": false
                    }
                ]
            }
        ]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-schedule-2a-today")
        .bind(today)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_schedule(router, "test-schedule-2a-today", None).await;

        assert_eq!(status, StatusCode::OK);
        // Byte-for-byte the same JSON that was stored -- including its
        // snake_case keys, unchanged -- proving this route is a true
        // pass-through, not a re-shaping.
        assert_eq!(body, population);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-today").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_an_explicit_date_query_param_selects_that_date_not_today -- --ignored`"]
    async fn schedule_an_explicit_date_query_param_selects_that_date_not_today() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-explicit-date").await;

        let requested = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let population = serde_json::json!([{"uid": "C99999", "calling_points": []}]);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3)",
        )
        .bind("test-schedule-2a-explicit-date")
        .bind(requested)
        .bind(&population)
        .execute(&pool)
        .await
        .expect("seed fixture population row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = get_line_schedule(
            router,
            "test-schedule-2a-explicit-date",
            Some("2026-01-02"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, population);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-explicit-date").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                schedule_a_row_only_for_a_different_date_is_still_404_today -- --ignored`"]
    async fn schedule_a_row_only_for_a_different_date_is_still_404_today() {
        let pool = connect().await;
        delete_schedule_population_fixture(&pool, "test-schedule-2a-stale").await;

        let yesterday = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, '[]')",
        )
        .bind("test-schedule-2a-stale")
        .bind(yesterday)
        .execute(&pool)
        .await
        .expect("seed a stale fixture row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, _) = get_line_schedule(router, "test-schedule-2a-stale", None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        delete_schedule_population_fixture(&pool, "test-schedule-2a-stale").await;
    }
```

  These four tests use `test_app(pool, vec![])`/`test_router` exactly as
  the existing `get_line_definition_*` tests do (an empty catalogue is
  fine — `get_line_schedule` never consults `app.config.lines`).

- [ ] **Step 7: Run the new DB-backed tests.**

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api schedule_ -- --ignored --test-threads=1
```

  (Requires a live, migrated Postgres — `sqlx migrate run` from
  `crates/api` first, same as CI's own sequence, if not already applied.)
  Expected: 4 passed (the four `schedule_*` tests added above; this
  filter also happens to match none of `departures.rs`'s
  `schedule_departures_*` tests, since cargo's substring filter is applied
  per-binary and this run targets `-p api`'s lib target specifically —
  if it does pick up `departures.rs`'s tests too, that's fine, they should
  also pass unmodified).

- [ ] **Step 8: Full local `api` check.**

```bash
cargo build --workspace
cargo clippy --workspace --all-features
cargo test -p api
```

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "api: add GET /public/lines/{id}/schedule, a public read-through to schedule_line_population"
```

---

## Task 2: `GET /public/stanox-crs` (Phase 2b)

**Files:** create `crates/api/src/routes/stanox_crs.rs`, modify
`crates/api/src/routes/mod.rs`.

Independent of Task 1 — different file, different table. Closes design
doc Decision 4.

**Collision check (done this session):** the only existing `stanox-crs`
route anywhere in this crate is `GET /private/stanox-crs`
(`crates/api/src/routes/ingest.rs:314-321`), mounted under `/private` via
`private_router()` (`routes/mod.rs:68`), gated by `require_internal_oauth`.
`public_router()` (`routes/mod.rs:26-61`) has no route at `/stanox-crs`
today, and `/public/stanox-crs` and `/private/stanox-crs` are different
paths by construction (`main.rs:61-62` nests them under different
prefixes) — no collision.

**Naming justification:** `GET /public/stanox-crs`, not
`/public/stations/all` or `/public/stations/{crs}/tiploc` (the design
doc's own alternate placeholders). Reasoning: (a) this is a bulk mirror of
one specific table, `stanox_crs` — naming it after that table, the same
way `GET /private/stanox-crs` already does, keeps the public and private
names symmetric and makes "this is the public version of the private
ingest-side route" obvious from the path alone; (b) nesting it under
`/stations/...` would misleadingly imply a relationship to
`GET /public/stations`/`search_stations` that the design doc explicitly
rejected (Decision 4 option (a), not chosen) — this route reads a
completely different table with no join to `stations` at all, and should
not look coupled to it in its URL either.

- [ ] **Step 1: Create the route file.**

```rust
//! `GET /public/stanox-crs`: a bulk, unauthenticated public mirror of the
//! live STANOX->CRS->TIPLOC->station-name reference table. Entirely
//! independent of `/public/stations`/`search_stations`
//! (`crates/api/src/routes/reference.rs`, `crates/api/src/data/reference.rs`)
//! -- different table (`stanox_crs`, not `stations`), different query,
//! different file; this route adds no coupling between the two. See
//! docs/superpowers/specs/2026-09-05-mcp-deeper-api-integration-design.md
//! Decision 4.
//!
//! Reads the exact same `queries::list_stanox_crs` the existing
//! internal-oauth-gated `GET /private/stanox-crs`
//! (`crates/api/src/routes/ingest.rs`) already uses -- same query, same
//! "the full current table, not a freshness timestamp" shape, different
//! (public, no-credential) mount point. No pagination, matching
//! `routes::island_of_ireland`'s own "the whole catalogue is a few hundred
//! rows at most" precedent for an unauthenticated whole-table GET.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/stanox-crs", axum::routing::get(list_stanox_crs))
}

async fn list_stanox_crs(
    State(app): State<App>,
) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "public stanox-crs mirror query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards against a future `#[serde(rename_all = "camelCase")]`
    /// silently being added to `common::StanoxCrsRecord` (which would
    /// change this route's response shape without anyone touching this
    /// file) -- this route deliberately mirrors `GET /private/stanox-crs`'s
    /// existing snake_case shape unchanged, per this plan's Global
    /// Constraints.
    #[test]
    fn stanox_crs_record_serializes_as_snake_case_unchanged() {
        let record = common::StanoxCrsRecord {
            stanox: "12345".to_string(),
            crs: "WAT".to_string(),
            tiploc: "WATRLMN".to_string(),
            station_name: "London Waterloo".to_string(),
            source_sequence: 7,
        };
        let value = serde_json::to_value(&record).expect("serialize StanoxCrsRecord");
        assert_eq!(
            value,
            serde_json::json!({
                "stanox": "12345",
                "crs": "WAT",
                "tiploc": "WATRLMN",
                "station_name": "London Waterloo",
                "source_sequence": 7
            })
        );
    }
}

/// HTTP-layer tests exercised against a live database -- mirrors
/// `routes::departures::db_tests`'s seed/assert/delete pattern and its
/// `test_app` helper (copied here rather than shared, matching this
/// crate's own established convention of colocating this helper per-file
/// until a shared module is actually warranted -- see
/// `routes::departures::db_tests::test_app`'s own doc comment for the
/// same reasoning stated the first time this was copied).
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
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
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
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

    async fn delete_fixture(pool: &PgPool, stanox: &str) {
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = $1")
            .bind(stanox)
            .execute(pool)
            .await
            .expect("cleanup fixture stanox_crs row");
    }

    async fn seed_fixture(pool: &PgPool, stanox: &str, crs: &str, tiploc: &str, name: &str) {
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ($1, $2, $3, $4, 1) \
             ON CONFLICT (stanox) DO UPDATE SET \
                crs = EXCLUDED.crs, tiploc = EXCLUDED.tiploc, station_name = EXCLUDED.station_name",
        )
        .bind(stanox)
        .bind(crs)
        .bind(tiploc)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed fixture stanox_crs row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                stanox_crs_route_is_unauthenticated_and_returns_seeded_rows \
                -- --ignored --test-threads=1`"]
    async fn stanox_crs_route_is_unauthenticated_and_returns_seeded_rows() {
        let pool = connect().await;
        delete_fixture(&pool, "9ZTEST01").await;
        delete_fixture(&pool, "9ZTEST02").await;

        seed_fixture(&pool, "9ZTEST01", "ZTA", "ZTESTTPA", "Test Alpha").await;
        seed_fixture(&pool, "9ZTEST02", "ZTB", "ZTESTTPB", "Test Beta").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        // No Authorization/Cookie header at all -- proves this route needs
        // no credential, per this plan's Global Constraints.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stanox-crs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let rows = json.as_array().expect("response is a JSON array");

        let find = |stanox: &str| {
            rows.iter()
                .find(|r| r.get("stanox").and_then(Value::as_str) == Some(stanox))
        };
        let alpha = find("9ZTEST01").expect("fixture 9ZTEST01 present in response");
        assert_eq!(alpha["crs"], "ZTA");
        assert_eq!(alpha["tiploc"], "ZTESTTPA");
        assert_eq!(alpha["station_name"], "Test Alpha");
        let beta = find("9ZTEST02").expect("fixture 9ZTEST02 present in response");
        assert_eq!(beta["crs"], "ZTB");

        delete_fixture(&pool, "9ZTEST01").await;
        delete_fixture(&pool, "9ZTEST02").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                stanox_crs_route_reflects_an_update_to_an_existing_row \
                -- --ignored --test-threads=1`"]
    async fn stanox_crs_route_reflects_an_update_to_an_existing_row() {
        // Proves this is a live read of the current table, not a cached or
        // point-in-time snapshot -- matching `queries::list_stanox_crs`'s
        // own doc comment ("the full current table").
        let pool = connect().await;
        delete_fixture(&pool, "9ZTEST03").await;
        seed_fixture(&pool, "9ZTEST03", "ZTC", "ZTESTTPC", "Test Gamma Old Name").await;
        seed_fixture(&pool, "9ZTEST03", "ZTC", "ZTESTTPC", "Test Gamma New Name").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/stanox-crs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let rows = json.as_array().expect("response is a JSON array");
        let gamma = rows
            .iter()
            .find(|r| r.get("stanox").and_then(Value::as_str) == Some("9ZTEST03"))
            .expect("fixture 9ZTEST03 present in response");
        assert_eq!(gamma["station_name"], "Test Gamma New Name");

        delete_fixture(&pool, "9ZTEST03").await;
    }
}
```

- [ ] **Step 2: Register the new module and merge its router.** Modify
  `crates/api/src/routes/mod.rs`. Current (`mod.rs:7-23`):

```rust
pub mod auth;
pub mod chatbot;
pub mod departures;
pub mod freshness;
pub mod health;
pub mod history_retention;
pub mod incidents;
pub mod ingest;
pub mod island_of_ireland;
pub mod line_status;
pub mod lines;
pub mod notifications;
pub mod preferences;
pub mod reference;
pub mod samples;
pub mod station_stats;
pub mod train;
```

  Add `pub mod stanox_crs;` alphabetically (after `station_stats`, before
  `train`):

```rust
pub mod auth;
pub mod chatbot;
pub mod departures;
pub mod freshness;
pub mod health;
pub mod history_retention;
pub mod incidents;
pub mod ingest;
pub mod island_of_ireland;
pub mod line_status;
pub mod lines;
pub mod notifications;
pub mod preferences;
pub mod reference;
pub mod samples;
pub mod stanox_crs;
pub mod station_stats;
pub mod train;
```

  Current `public_router()` (`mod.rs:25-61`) ends:

```rust
        .merge(station_stats::router())
        .merge(departures::router())
}
```

  Change to:

```rust
        .merge(station_stats::router())
        .merge(departures::router())
        .merge(stanox_crs::router())
}
```

- [ ] **Step 3: Run the new tests.**

```bash
cargo test -p api stanox_crs_record_serializes_as_snake_case_unchanged
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api stanox_crs_route -- --ignored --test-threads=1
```

  Expected: 1 passed (unit test), then 2 passed (the two `db_tests`
  above).

- [ ] **Step 4: Full local `api` check.**

```bash
cargo build --workspace
cargo clippy --workspace --all-features
cargo test -p api
```

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/stanox_crs.rs crates/api/src/routes/mod.rs
git commit -m "api: add GET /public/stanox-crs, a public bulk mirror of the stanox_crs table"
```

---

## Task 3: End-to-end verification (both routes together)

**Files:** none (verification only).

Depends on Tasks 1 and 2 both being complete. Confirms the whole workspace
still builds/tests cleanly with both new routes present, and that a real
running `api` instance serves both correctly — including the one
regression this plan must not cause: `search_stations`/
`GET /public/stations?q=` staying completely unaffected.

- [ ] **Step 1: Full workspace check, matching CI's real sequence
  (`.github/workflows/ci.yml`).**

```bash
cargo fmt --all
cargo build --workspace
cargo clippy --workspace --all-features
cargo test --workspace
```

  `cargo fmt --all` (not `--check`) is run here, not merely checked: CI's
  own `rustfmt` job is `continue-on-error: true` today because of
  pre-existing, unrelated drift elsewhere in the workspace
  (`.github/workflows/ci.yml:118-124`'s own comment) — this task still
  keeps the two new/changed files themselves clean rather than relying on
  that non-blocking check.

- [ ] **Step 2: DB-backed ignored tests, the real CI incantation.**

```bash
# Start a local Postgres if one isn't already running, e.g.:
docker run --rm -d --name mcp-phase2-pg -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=postgres -p 5432:5432 postgres:16

cargo install sqlx-cli --version 0.8.6 --no-default-features --features postgres,rustls --locked
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  bash -c 'cd crates/api && sqlx migrate run'

DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api -- --ignored --test-threads=1
```

  Expected: every existing `#[ignore]`d `api` test still passes (no
  regression to `departures.rs`/`reference.rs`/`lines.rs`'s existing
  suites), plus the 4 new `schedule_*` tests (Task 1) and 2 new
  `stanox_crs_*` tests (Task 2).

```bash
docker stop mcp-phase2-pg   # if started above
```

- [ ] **Step 3: Real running-instance smoke test.** Bring up the full dev
  stack (`docker-compose.dev.yml`'s documented workflow — see
  `dev.env.example` for the required `POSTGRES_*`/`SSO_*`/
  `INTERNAL_OAUTH_*` values this compose file needs):

```bash
cp dev.env.example dev.env   # fill in real values per that file's own comments
docker compose --env-file dev.env up -d --build
```

  Wait for `api`'s healthcheck to pass (`docker compose --env-file dev.env
  ps` shows `api` as `healthy`; it curls `http://localhost:8080/public/health`
  internally, per `docker-compose.yml:155-157`), then seed one fixture row
  per new route directly against the running Postgres container (reads
  `POSTGRES_USER`/`POSTGRES_DB` back out of `dev.env` rather than
  hardcoding them, since a real deployment's `dev.env` may differ from the
  defaults in `dev.env.example`):

```bash
set -a; source dev.env; set +a

docker compose --env-file dev.env exec -T postgres \
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "INSERT INTO schedule_line_population (line_id, service_date, population) \
   VALUES ('test-e2e-schedule', CURRENT_DATE, \
   '[{\"uid\":\"C00001\",\"calling_points\":[{\"tiploc\":\"WATRLMN\",\"kind\":\"origin\",\"booked_arrival\":null,\"booked_departure\":\"08:00:00\",\"is_half_minute_arrival\":false,\"is_half_minute_departure\":false}]}]'::jsonb) \
   ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population;"

docker compose --env-file dev.env exec -T postgres \
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
   VALUES ('9ZE2E01', 'ZTE', 'ZTESTE2E', 'Test E2E Station', 1) \
   ON CONFLICT (stanox) DO UPDATE SET station_name = EXCLUDED.station_name;"
```

  Then verify both new routes, and that `search_stations` is untouched:

```bash
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/public/lines/test-e2e-schedule/schedule
# expect 200

curl -s http://localhost:8080/public/lines/test-e2e-schedule/schedule | jq '.'
# expect the exact fixture array above, snake_case keys unchanged
# (uid, calling_points, booked_arrival, ...)

curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/public/lines/test-e2e-schedule/schedule?date=2000-01-01
# expect 404 -- no row for that date

curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/public/stanox-crs
# expect 200

curl -s http://localhost:8080/public/stanox-crs | jq 'map(select(.stanox == "9ZE2E01"))'
# expect one object: {"stanox":"9ZE2E01","crs":"ZTE","tiploc":"ZTESTE2E","station_name":"Test E2E Station","source_sequence":1}

curl -s http://localhost:8080/public/stanox-crs | jq 'length'
# record this number -- answers the design doc's own unmeasured Open
# question/risk 5 ("likely small, one row per GB TIPLOC, not verified")
# for the real deployment this is run against; not a pass/fail gate

# Regression check: search_stations/GET /public/stations?q= untouched
curl -s -o /dev/null -w '%{http_code}\n' "http://localhost:8080/public/stations?q=london"
# expect 200, exactly as before this plan
```

  Clean up the fixture rows and tear the stack down:

```bash
docker compose --env-file dev.env exec -T postgres \
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c \
  "DELETE FROM schedule_line_population WHERE line_id = 'test-e2e-schedule'; \
   DELETE FROM stanox_crs WHERE stanox = '9ZE2E01';"
docker compose --env-file dev.env down
```

No commit for this task (verification only) — if any step fails, fix the
relevant earlier task and re-verify before considering this plan complete.

---

## Self-review

- **Spec coverage.** Design doc Decision 3 (Phase 2a) is Task 1 in full,
  including its exact response-shape reasoning (opaque pass-through, no
  deserialization) and its path-shape choice (`line_id` in the path,
  `service_date` as an optional query param defaulting to today). Decision
  4 (Phase 2b) is Task 2 in full, including both naming and the explicit
  "don't touch `search_stations`" requirement (verified: Task 2's diff
  touches only a new file plus `routes/mod.rs`'s module list and
  `public_router()`'s merge chain — zero lines of `reference.rs`/
  `data/reference.rs` change). Decision 7 and Phases 0/1/3 are explicitly
  Non-goals, matching the task brief's own scoping.
- **Placeholder scan.** Every task step above contains complete,
  copy-pasteable code (handlers, tests, migrations-free SQL, `curl`/`jq`
  commands) — no "TBD"/"similar to Task N"/unshown test bodies.
- **Type/name consistency.** `get_line_schedule`/`resolve_schedule_date`
  (Task 1) and `list_stanox_crs`/`internal_error` (Task 2) are named and
  used consistently between their definition (Steps 1-3) and their test
  call sites (Steps 4-7 / Step 1-3 of Task 2). Both new routes' response
  types (`serde_json::Value` for Task 1, `Vec<common::StanoxCrsRecord>`
  for Task 2) match what their respective `queries::*` functions already
  return today, confirmed by direct reads of `crates/api/src/data/queries.rs`
  lines 713-724 and 760-776 this session.
