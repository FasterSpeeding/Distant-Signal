# Cross-Network Incident Archive/Search Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `GET /public/incidents` (a keyset-paginated, filterable list of
every Knowledgebase incident this app has ingested) and a `/incidents`
frontend page that browses/searches it, linking each result out to the
existing `/incidents/[id]` detail page.

**Architecture:** One new btree index supports the list's default order and
cursor. One new query function in `crates/api/src/data/queries.rs`
(`search_incidents`) runs a single dynamic-filter, keyset-paginated `SELECT`
against `incidents`, reusing the two existing GIN indexes for its
operator/station-overlap filters. One new handler in
`crates/api/src/routes/incidents.rs` (`search_incidents`, registered
alongside the existing by-id detail route) validates query params, resolves
`line` to a catalogue line's station list, and renders the page as camelCase
JSON. One new Client Component (`IncidentSearchForm.tsx`) mirrors
`TrainSearchForm.tsx`'s fetch/`useState`/"Load more" shape exactly, fetching
through the existing same-origin `/api/*` proxy; a thin Server Component
shell (`app/incidents/page.tsx`) fetches reference data for the filter
dropdowns and reads `searchParams` for a shareable initial filter state.

**Tech Stack:** Rust (axum, sqlx, Postgres) for the API; Next.js App Router
(Server + Client Components), Mantine, Vitest for the frontend.

**Spec:**
`docs/superpowers/specs/2026-09-12-incident-archive-design.md` (this plan
implements every decision in that spec as written; it does not re-derive
any of them). Parent spec for shared conventions:
`docs/superpowers/specs/2026-08-31-incident-detail-page-design.md`.

## Global Constraints

- **`incidents` table schema** (unchanged by this feature — no columns
  added or dropped):
  ```
  incident_id       TEXT PRIMARY KEY
  summary           TEXT NOT NULL
  description       TEXT NOT NULL
  operators         TEXT[] NOT NULL          -- ATOC codes
  affected_stations TEXT[] NOT NULL          -- CRS codes
  priority          INTEGER NOT NULL         -- raw RDM int, no documented enum
  validity_periods  JSONB NOT NULL DEFAULT '[]'
  is_planned        BOOLEAN NOT NULL DEFAULT FALSE
  is_cleared        BOOLEAN NOT NULL DEFAULT FALSE
  fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
  first_seen_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
  ```
  Existing indexes reused as-is: `incidents_operators_gin` (GIN over
  `operators`), `incidents_affected_stations_gin` (GIN over
  `affected_stations`). `incidents_active` (partial, `WHERE NOT is_cleared`)
  is NOT used by this feature — it carries no orderable column.
- **Pagination cursor shape**, reused exactly from `trains.rs::get_trains_search`
  / `queries::search_schedule_calling_point_departures`: an opaque
  base64url(no-pad)-encoded keyset cursor, a `limit` query param clamped to a
  `MAX_*_SEARCH_LIMIT` constant (never rejected for being too large, only for
  being non-positive or unparseable), an `after` query param naming the
  previous page's cursor, a `nextCursor` field in the response that is
  explicit JSON `null` on the last page (never omitted), and
  `#[serde(deny_unknown_fields)]` on the query-params struct.
- **30-day default window**: the frontend's initial filter state (no
  `searchParams` present) sets `from` to 30 days before today. This is a
  **frontend-only** default — the backend route itself has no implicit date
  floor and returns every matching incident ever ingested, newest-first, one
  page at a time, when no `from` is given at all.
- **DB-backed test convention**: every Rust test that touches a live
  database is `#[tokio::test]` + `#[ignore]`, run explicitly with:
  ```
  DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
  ```
  (`--test-threads=1` because these tests share one real database and are
  not isolated from each other.)
- **Frontend test framework**: Vitest, with `renderWithMantine` from
  `frontend/test/render.tsx` and `@testing-library/react`, colocated
  `*.test.tsx`/`*.test.ts` files.
- **The `line` filter is a station-overlap approximation, not a real line
  match** (Correction 1 / Decision 2 of the spec) — it catches the matcher's
  `StationHit`/`ExclusiveSegment`/`SharedSegment` tiers but **misses**
  `KeywordOnly` and `OperatorOnly` matches. This limitation must be visible
  in the UI copy, not just documented in code comments.
- **`priority` is a raw, unexplained integer range filter** — no
  Major/Minor enum exists or is invented here (Correction 2 / Decision 3).
  The UI must label it as a raw feed value with undocumented meaning.
- **Retention/pruning for `incidents`/`incident_history` is explicitly out
  of scope for this feature** and must not be added as a side effect of any
  task below — it is a real, already-flagged gap (see the spec's "Retention
  and unbounded growth" section) that this plan deliberately does not close.

---

## File Structure

```
crates/api/migrations/
  20260912090000_incidents_first_seen_at_id.sql   NEW -- one index

crates/api/src/data/queries.rs
  + IncidentSummaryRow, IncidentSearchCursor, IncidentSearchPage structs
  + search_incidents() query function                          (Task 2)
  + #[cfg(test)] mod incident_search_query_tests                (Task 2)

crates/api/src/routes/incidents.rs
  + IncidentSearchParams struct, DEFAULT_INCIDENT_SEARCH_LIMIT /
    MAX_INCIDENT_SEARCH_LIMIT constants, normalize_limit, normalize_rfc3339,
    encode_cursor, decode_cursor, incident_summary_json, search_incidents
    handler; router() gains a second route                     (Task 3)
  + #[cfg(test)] mod db_tests additions                         (Task 3)

frontend/lib/types.ts
  + IncidentSummary, IncidentSearchResponse                     (Task 4)

frontend/components/IncidentSearchForm.tsx      NEW             (Task 4)
frontend/components/IncidentSearchForm.test.tsx NEW             (Task 4)

frontend/app/incidents/page.tsx                 NEW             (Task 5)
frontend/app/incidents/page.test.tsx            NEW             (Task 5)
frontend/app/layout.tsx                         MODIFIED (nav link) (Task 5)
frontend/app/lines/page.tsx                     MODIFIED (nav link) (Task 5)
```

---

## Task 1: Migration — `incidents_first_seen_at_id` index

**Files:**
- Create: `crates/api/migrations/20260912090000_incidents_first_seen_at_id.sql`

**Interfaces:**
- Produces: an index named `incidents_first_seen_at_id` on
  `incidents (first_seen_at DESC, incident_id DESC)`, which Task 2's
  `search_incidents` query relies on for its default order, its keyset
  cursor comparison, and its `from`/`to` bound (leading column).

- [ ] **Step 1: Write the migration file**

```sql
-- Supports GET /public/incidents' default ordering, keyset cursor, and
-- from/to date-range filter (leading column) -- see
-- docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 5.
-- incidents_active (WHERE NOT is_cleared) carries no orderable column and
-- was built for a different, narrower check ("does this specific still-
-- active id exist"); this is the first index on first_seen_at at all.
-- incidents_operators_gin / incidents_affected_stations_gin already cover
-- this feature's operator/line filters unchanged -- no new index needed
-- for either.
CREATE INDEX incidents_first_seen_at_id
    ON incidents (first_seen_at DESC, incident_id DESC);
```

- [ ] **Step 2: Apply the migration against the local test database**

Run:
```
sqlx migrate run --source crates/api/migrations --database-url postgres://lucy@localhost:5432/distant_signal_test
```
Expected: `Applied 20260912090000/migrate incidents first seen at id ...`
(or equivalent success output), no error.

- [ ] **Step 3: Verify the index exists**

Run:
```
psql postgres://lucy@localhost:5432/distant_signal_test -c "\d incidents" | grep incidents_first_seen_at_id
```
Expected output contains:
```
"incidents_first_seen_at_id" btree (first_seen_at DESC, incident_id DESC)
```

- [ ] **Step 4: Commit**

```bash
git add crates/api/migrations/20260912090000_incidents_first_seen_at_id.sql
git commit -m "Add incidents_first_seen_at_id index for the incident archive's default order/cursor"
```

---

## Task 2: Backend data layer — `search_incidents` query function

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add after
  `lines_currently_reporting_incident`, before the
  `// --- Movement Events Queries ---` section marker)
- Test: same file, new `#[cfg(test)] mod incident_search_query_tests` block

**Interfaces:**
- Consumes: nothing from earlier tasks except Task 1's new index (used by
  Postgres's planner, not referenced by name in Rust code).
- Produces (consumed by Task 3):
  - `pub struct IncidentSummaryRow { incident_id: String, summary: String, operators: Vec<String>, affected_stations: Vec<String>, priority: i32, is_planned: bool, is_cleared: bool, first_seen_at: chrono::DateTime<chrono::Utc>, fetched_at: chrono::DateTime<chrono::Utc> }`
  - `pub struct IncidentSearchCursor { first_seen_at: chrono::DateTime<chrono::Utc>, incident_id: String }`
  - `pub struct IncidentSearchPage { results: Vec<IncidentSummaryRow>, next_cursor: Option<IncidentSearchCursor> }`
  - `pub async fn search_incidents(pool: &PgPool, operators: Option<Vec<String>>, affected_stations: Option<Vec<String>>, is_planned: Option<bool>, is_cleared: Option<bool>, priority_min: Option<i32>, priority_max: Option<i32>, first_seen_from: Option<chrono::DateTime<chrono::Utc>>, first_seen_to: Option<chrono::DateTime<chrono::Utc>>, after: Option<&IncidentSearchCursor>, limit: i64) -> Result<IncidentSearchPage>`
    — **always** returns `Ok`, never `Ok(None)`/404-shaped: an empty match
    is `Ok(IncidentSearchPage { results: vec![], next_cursor: None })`,
    matching the design spec's testing note that there is no
    "unpublished"-vs-"empty" distinction for this table (unlike
    `search_schedule_calling_point_departures`).

- [ ] **Step 1: Write the failing tests**

Add this new module directly after `lines_currently_reporting_incident`'s
closing brace (i.e. right before the `// --- Movement Events Queries ---`
comment), mirroring the existing `schedule_destination_departures_query_tests`
module's own local `test_pool()`/fixture-helper style:

```rust
/// Every fixture incident_id in this module is prefixed `archive-test-`
/// and cleaned up by prefix, rather than day-scoped like the calling-point
/// search tests (`incidents` has no natural per-test partition key the way
/// `schedule_destination_departures` has `service_date`).
#[cfg(test)]
mod incident_search_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM incidents WHERE incident_id LIKE 'archive-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incidents rows");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_incident(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_stations: &[&str],
        priority: i32,
        is_planned: bool,
        is_cleared: bool,
        first_seen_at: chrono::DateTime<chrono::Utc>,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, \
                 is_planned, is_cleared, first_seen_at) \
             VALUES ($1, $2, '', $3, $4, $5, $6, $7, $8)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_stations)
        .bind(priority)
        .bind(is_planned)
        .bind(is_cleared)
        .bind(first_seen_at)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    fn at(hour: u32) -> chrono::DateTime<chrono::Utc> {
        // Every fixture timestamp lands on a fixed far-future day so ties
        // and ordering are exact and reproducible, mirroring
        // `schedule_destination_departures_query_tests::fixture_date`'s own
        // "far future, deterministic" rationale.
        chrono::Utc
            .with_ymd_and_hms(2099, 1, 1, hour, 0, 0)
            .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_with_no_filters_returns_every_row_newest_first_ties_broken_by_incident_id_desc()
     {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-1", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        // Two incidents sharing the SAME first_seen_at -- the tiebreak this
        // test exists to prove.
        seed_incident(&pool, "archive-test-2", &["VT"], &["WAT"], 1, false, false, at(10)).await;
        seed_incident(&pool, "archive-test-3", &["VT"], &["WAT"], 1, false, false, at(10)).await;

        let page = search_incidents(
            &pool, None, None, None, None, None, None, None, None, None, 100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page.results.iter().map(|r| r.incident_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["archive-test-3", "archive-test-2", "archive-test-1"],
            "newest first_seen_at first; a tie at the same first_seen_at breaks on \
             incident_id descending: {ids:?}"
        );
        assert!(page.next_cursor.is_none());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_operator_filter_matches_on_overlap_not_exact_match() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-a", &["VT", "SW"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-b", &["GW"], &["PAD"], 1, false, false, at(9)).await;

        let page = search_incidents(
            &pool,
            Some(vec!["SW".to_string()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page.results.iter().map(|r| r.incident_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["archive-test-a"],
            "an incident with operators {{VT, SW}} matches a request for SW alone: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_affected_stations_filter_matches_on_overlap_and_excludes_no_overlap_incidents()
     {
        // This is the direct regression proving the "line" filter's
        // approximation limitation at the primitive level: an incident with
        // NO station overlap at all (the shape a real KeywordOnly/
        // OperatorOnly-only matcher hit would have -- see Correction 1 of
        // the design spec) is correctly absent from a station-overlap
        // filter's results, even though such an incident could be
        // perfectly real for that line via the matcher's other tiers.
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-c", &["VT"], &["WAT", "WOK"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-d", &["VT"], &[], 1, false, false, at(9)).await;

        let page = search_incidents(
            &pool,
            None,
            Some(vec!["WOK".to_string()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page.results.iter().map(|r| r.incident_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["archive-test-c"],
            "an incident with no affected_stations overlap must be excluded, even though it \
             shares an operator with the filtered line: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_from_to_bounds_are_inclusive() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-e", &["VT"], &["WAT"], 1, false, false, at(8)).await;
        seed_incident(&pool, "archive-test-f", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-g", &["VT"], &["WAT"], 1, false, false, at(10)).await;

        let page = search_incidents(
            &pool,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(at(8)),
            Some(at(9)),
            None,
            100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page.results.iter().map(|r| r.incident_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["archive-test-f", "archive-test-e"],
            "both bounds are inclusive; archive-test-g (hour 10) must be excluded: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_planned_and_cleared_filters() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-h", &["VT"], &["WAT"], 1, true, false, at(9)).await;
        seed_incident(&pool, "archive-test-i", &["VT"], &["WAT"], 1, false, true, at(9)).await;

        let planned_only = search_incidents(
            &pool, None, None, Some(true), None, None, None, None, None, None, 100,
        )
        .await
        .expect("search");
        assert_eq!(
            planned_only.results.iter().map(|r| r.incident_id.as_str()).collect::<Vec<_>>(),
            vec!["archive-test-h"]
        );

        let cleared_only = search_incidents(
            &pool, None, None, None, Some(true), None, None, None, None, None, 100,
        )
        .await
        .expect("search");
        assert_eq!(
            cleared_only.results.iter().map(|r| r.incident_id.as_str()).collect::<Vec<_>>(),
            vec!["archive-test-i"]
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_priority_range_is_inclusive() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-j", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-k", &["VT"], &["WAT"], 2, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-l", &["VT"], &["WAT"], 3, false, false, at(9)).await;

        let page = search_incidents(
            &pool,
            None,
            None,
            None,
            None,
            Some(2),
            Some(2),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        assert_eq!(
            page.results.iter().map(|r| r.incident_id.as_str()).collect::<Vec<_>>(),
            vec!["archive-test-k"]
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_combines_filters_with_and_semantics() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        // Matches operator AND planned AND priority range:
        seed_incident(&pool, "archive-test-m", &["VT"], &["WAT"], 5, true, false, at(9)).await;
        // Fails on operator only:
        seed_incident(&pool, "archive-test-n", &["GW"], &["WAT"], 5, true, false, at(9)).await;
        // Fails on planned only:
        seed_incident(&pool, "archive-test-o", &["VT"], &["WAT"], 5, false, false, at(9)).await;
        // Fails on priority range only:
        seed_incident(&pool, "archive-test-p", &["VT"], &["WAT"], 1, true, false, at(9)).await;

        let page = search_incidents(
            &pool,
            Some(vec!["VT".to_string()]),
            None,
            Some(true),
            None,
            Some(4),
            Some(6),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        assert_eq!(
            page.results.iter().map(|r| r.incident_id.as_str()).collect::<Vec<_>>(),
            vec!["archive-test-m"],
            "only the row matching every filter simultaneously must be returned"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_keyset_pagination_pages_without_gaps_or_repeats_and_breaks_ties_on_incident_id_desc()
     {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        // Five incidents, three of them sharing one first_seen_at, forcing
        // the tiebreak to matter mid-pagination.
        seed_incident(&pool, "archive-test-q1", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-q2", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-q3", &["VT"], &["WAT"], 1, false, false, at(9)).await;
        seed_incident(&pool, "archive-test-r", &["VT"], &["WAT"], 1, false, false, at(8)).await;
        seed_incident(&pool, "archive-test-s", &["VT"], &["WAT"], 1, false, false, at(10)).await;

        let mut cursor: Option<IncidentSearchCursor> = None;
        let mut collected: Vec<String> = Vec::new();
        loop {
            let page = search_incidents(
                &pool,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                cursor.as_ref(),
                1,
            )
            .await
            .expect("search");
            assert_eq!(page.results.len(), 1, "limit=1 must return exactly one row per page");
            collected.push(page.results[0].incident_id.clone());
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        assert_eq!(
            collected,
            vec![
                "archive-test-s",
                "archive-test-q3",
                "archive-test-q2",
                "archive-test-q1",
                "archive-test-r",
            ],
            "no gaps, no repeats, tie broken by incident_id descending: {collected:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_with_no_matches_returns_ok_with_an_empty_vec_never_none() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "archive-test-t", &["VT"], &["WAT"], 1, false, false, at(9)).await;

        let page = search_incidents(
            &pool,
            Some(vec!["ZZ".to_string()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search never fails on an unmatched filter -- there is no 404 concept here");

        assert!(page.results.is_empty());
        assert!(page.next_cursor.is_none());
        delete_fixtures(&pool).await;
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail (function does not exist yet)**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api incident_search_query_tests -- --ignored --test-threads=1
```
Expected: compile error — `search_incidents`, `IncidentSearchCursor`,
`IncidentSummaryRow` not found in this scope.

- [ ] **Step 3: Add `use chrono::TimeZone;` for the test module's `at()` helper**

Add near the top of the `incident_search_query_tests` module (needed for
`chrono::Utc.with_ymd_and_hms(...)`):

```rust
use chrono::TimeZone;
```

(Place this alongside the module's existing `use super::*; use
sqlx::postgres::PgPoolOptions;` lines from Step 1.)

- [ ] **Step 4: Write the structs and query function**

Add this directly above the test module from Step 1 (i.e. immediately after
`lines_currently_reporting_incident`'s closing brace, before
`// --- Movement Events Queries ---`):

```rust
/// Deliberately lighter than `IncidentRow` -- no `description`, no
/// `validity_periods`. See
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 7:
/// a list row that may render dozens per page has no use for either field,
/// and `description`'s raw HTML would otherwise force every list-rendering
/// call site to sanitize it for nothing.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentSummaryRow {
    pub incident_id: String,
    pub summary: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

/// Keyset cursor for `search_incidents`, matching
/// `CallingPointDepartureCursor`'s own shape and rationale exactly (see
/// that struct's doc comment) but over `(first_seen_at DESC, incident_id
/// DESC)` instead of `(scheduled, train_uid)`. `routes::incidents` encodes
/// this onto the wire and parses it back; nothing outside that module
/// should construct one from user input directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentSearchCursor {
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub incident_id: String,
}

/// One page of incident-archive search results. `next_cursor` is `Some`
/// only when there is genuinely at least one more row (the query fetches
/// `limit + 1` to know that) -- same convention as
/// `CallingPointDeparturePage`.
#[derive(Debug, Clone)]
pub struct IncidentSearchPage {
    pub results: Vec<IncidentSummaryRow>,
    pub next_cursor: Option<IncidentSearchCursor>,
}

/// The incident archive's one read: a keyset-paginated, dynamically
/// filtered scan of `incidents`, ordered newest-`first_seen_at`-first with
/// ties broken by `incident_id` descending -- see
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 4.
/// Modelled directly on `search_schedule_calling_point_departures`'s
/// "`fetch = limit + 1`, one extra row to detect `has_more`,
/// `($n::type IS NULL OR condition)` per optional filter, keyset tuple
/// comparison in `WHERE`, matching `ORDER BY`" shape.
///
/// **Unlike that function, this one never returns `Ok(None)`.** There is
/// no "has this day been published yet" concept for `incidents` -- an
/// unfiltered request that matches nothing (or a filter combination that
/// matches nothing) is `Ok` with an empty `results` Vec, always a `200`
/// with an empty array at the route layer, never a `404`.
///
/// `affected_stations` is the resolved station list for the caller's
/// `line` filter (Decision 2's approximation) when one was given, or
/// `None` for "no line filter" -- this function has no knowledge of line
/// catalogues at all; that resolution happens in `routes::incidents`
/// before this is called.
#[allow(clippy::too_many_arguments)]
pub async fn search_incidents(
    pool: &PgPool,
    operators: Option<Vec<String>>,
    affected_stations: Option<Vec<String>>,
    is_planned: Option<bool>,
    is_cleared: Option<bool>,
    priority_min: Option<i32>,
    priority_max: Option<i32>,
    first_seen_from: Option<chrono::DateTime<chrono::Utc>>,
    first_seen_to: Option<chrono::DateTime<chrono::Utc>>,
    after: Option<&IncidentSearchCursor>,
    limit: i64,
) -> Result<IncidentSearchPage> {
    let fetch = limit.saturating_add(1);

    let rows: Vec<IncidentSummaryRow> = sqlx::query_as(
        r#"
            SELECT incident_id, summary, operators, affected_stations, priority,
                   is_planned, is_cleared, first_seen_at, fetched_at
            FROM incidents
            WHERE ($1::text[]      IS NULL OR operators && $1)
              AND ($2::text[]      IS NULL OR affected_stations && $2)
              AND ($3::boolean     IS NULL OR is_planned = $3)
              AND ($4::boolean     IS NULL OR is_cleared = $4)
              AND ($5::integer     IS NULL OR priority >= $5)
              AND ($6::integer     IS NULL OR priority <= $6)
              AND ($7::timestamptz IS NULL OR first_seen_at >= $7)
              AND ($8::timestamptz IS NULL OR first_seen_at <= $8)
              AND ($9::timestamptz IS NULL
                   OR (first_seen_at, incident_id) < ($9, $10))
            ORDER BY first_seen_at DESC, incident_id DESC
            LIMIT $11
            "#,
    )
    .bind(operators)
    .bind(affected_stations)
    .bind(is_planned)
    .bind(is_cleared)
    .bind(priority_min)
    .bind(priority_max)
    .bind(first_seen_from)
    .bind(first_seen_to)
    .bind(after.map(|c| c.first_seen_at))
    .bind(after.map(|c| c.incident_id.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    let has_more = rows.len() as i64 > limit;
    let mut page_rows = rows;
    if has_more {
        page_rows.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        page_rows.last().map(|r| IncidentSearchCursor {
            first_seen_at: r.first_seen_at,
            incident_id: r.incident_id.clone(),
        })
    } else {
        None
    };

    Ok(IncidentSearchPage {
        results: page_rows,
        next_cursor,
    })
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api incident_search_query_tests -- --ignored --test-threads=1
```
Expected: all 8 tests in `incident_search_query_tests` pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add search_incidents keyset-paginated query for the incident archive"
```

---

## Task 3: Backend route — `GET /public/incidents`

**Files:**
- Modify: `crates/api/src/routes/incidents.rs`
- Test: same file's existing `#[cfg(test)]` area (add a new `mod db_tests`
  alongside the existing plain `mod tests`)

**Interfaces:**
- Consumes: `queries::IncidentSummaryRow`, `queries::IncidentSearchCursor`,
  `queries::IncidentSearchPage`, `queries::search_incidents(...)` (Task 2).
  `app.config.lines: LineCatalogue` (deref's to `Vec<common::LineDefinition>`,
  each with `.id: String`, `.stations: Vec<common::Station>` where
  `Station.crs: String`) — same field access `lines.rs::get_line_definition`
  already uses.
- Produces (consumed by Task 4): the wire response shape
  `{ "results": IncidentSummary[], "nextCursor": string | null }` where each
  `IncidentSummary` is
  `{ incidentId, summary, operators, affectedStations, priority, isPlanned, isCleared, firstSeenAt, fetchedAt }`
  (camelCase, exactly matching Task 4's `IncidentSummary` TypeScript type).
  Full path: `GET /public/incidents` (mounted via the existing
  `public_router()` merge of `incidents::router()` in
  `crates/api/src/routes/mod.rs` — no change needed there).

- [ ] **Step 1: Write the failing tests**

Add to `crates/api/src/routes/incidents.rs`, after the existing
`#[cfg(test)] mod tests { ... }` block (the plain unit tests for
`to_incident_detail_json`):

```rust
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::internal_oauth::ServiceTokenVerifier;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Local to this test module, matching `routes::trains`'s own
    /// `test_app` -- this codebase's convention is one small
    /// per-route-test-module copy of this fixture builder, not a shared
    /// helper (grepped: `stanox_crs.rs`, `departures.rs`,
    /// `station_stats.rs`, `trains.rs`, `chatbot.rs`, `groups.rs`,
    /// `ingest.rs`, `train.rs` each define their own).
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
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
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
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
            internal_oauth_verifier: ServiceTokenVerifier::new(
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

    async fn delete_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM incidents WHERE incident_id LIKE 'route-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incidents rows");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_incident(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_stations: &[&str],
        priority: i32,
        is_planned: bool,
        is_cleared: bool,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, \
                 is_planned, is_cleared) \
             VALUES ($1, $2, '', $3, $4, $5, $6, $7)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_stations)
        .bind(priority)
        .bind(is_planned)
        .bind(is_cleared)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    async fn get(pool: &PgPool, lines: Vec<common::LineDefinition>, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), lines));
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

    fn fixture_line() -> common::LineDefinition {
        common::LineDefinition {
            id: "test-line".to_string(),
            name: "Test Line".to_string(),
            mode: "train".to_string(),
            category: "main".to_string(),
            operators: vec!["VT".to_string()],
            stations: vec![
                common::Station {
                    crs: "WAT".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
            ],
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_with_no_filters_returns_every_seeded_row_as_200() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-1", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!results(&body).is_empty());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_operator_param_is_comma_parsed_and_matches_on_overlap() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-2", &["VT", "SW"], &["WAT"], 1, false, false).await;
        seed_incident(&pool, "route-test-3", &["GW"], &["PAD"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?operator=SW,XX").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows.iter().map(|r| r["incidentId"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["route-test-2"]);
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_unknown_line_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?line=does-not-exist").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("line"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_known_line_resolves_to_station_overlap_and_excludes_a_no_overlap_incident()
     {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        // Matches test-line via WOK (one of the line's own stations).
        seed_incident(&pool, "route-test-4", &["VT"], &["WOK"], 1, false, false).await;
        // Same operator as the line, but NO shared station -- the shape a
        // real OperatorOnly-only matcher hit would have. Must be excluded:
        // this is the concrete proof the line filter's approximation
        // misses that tier, per Correction 1 of the design spec.
        seed_incident(&pool, "route-test-5", &["VT"], &["ZZZ"], 1, false, false).await;

        let (status, body) = get(&pool, vec![fixture_line()], "/incidents?line=test-line").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows.iter().map(|r| r["incidentId"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            vec!["route-test-4"],
            "only the station-overlap match must be returned: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_priority_min_greater_than_max_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?priority_min=5&priority_max=1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("priority"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it()
     {
        let pool = connect().await;
        let (status, _) = get(&pool, vec![], "/incidents?operater=SW").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-6", &["VT"], &["WAT"], 1, false, false).await;

        let (status, _) = get(&pool, vec![], "/incidents?limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, vec![], "/incidents?limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, _) = get(&pool, vec![], "/incidents?limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_INCIDENT_SEARCH_LIMIT, never rejected"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-7", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?limit=100").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-8", &["VT"], &["WAT"], 1, false, false).await;
        seed_incident(&pool, "route-test-9", &["VT"], &["WAT"], 1, false, false).await;

        let (status, first) = get(&pool, vec![], "/incidents?limit=1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&first).len(), 1);
        let cursor = next_cursor(&first).expect("a second page exists");

        let (status, second) = get(&pool, vec![], &format!("/incidents?limit=1&after={cursor}")).await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_ne!(
            second_rows[0]["incidentId"], results(&first)[0]["incidentId"],
            "`after` must continue from the cursor, not restart at page 1"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_renders_camel_case_rows_with_no_leaked_snake_case_fields() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-10", &["VT"], &["WAT"], 3, true, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let row = rows
            .iter()
            .find(|r| r["incidentId"] == "route-test-10")
            .expect("fixture row present");
        assert_eq!(row["summary"], "Fixture incident route-test-10");
        assert_eq!(row["priority"], 3);
        assert_eq!(row["isPlanned"], true);
        assert_eq!(row["isCleared"], false);
        assert!(row.get("first_seen_at").is_none(), "no stray snake_case field");
        assert!(row.get("is_planned").is_none(), "no stray snake_case field");
        assert!(row.get("description").is_none(), "list rows never include description");
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_published_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?operator=ZZ_NO_SUCH_OPERATOR").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an unmatched filter is a 200 with an empty results array, never a 404 -- there is \
             no 'unpublished' concept for this table"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail (route/handler does not exist yet)**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api incident_search -- --ignored --test-threads=1
```
Expected: compile error — `search_incidents` handler, `IncidentSearchParams`,
etc. not found; `router()` only has one route.

- [ ] **Step 3: Add the new imports at the top of the file**

Change:
```rust
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
```
to:
```rust
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
```

- [ ] **Step 4: Register the new route in `router()`**

Change:
```rust
pub fn router() -> Router {
    Router::new().route("/incidents/{incidentId}", axum::routing::get(get_incident))
}
```
to:
```rust
pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::get(search_incidents))
        .route("/incidents/{incidentId}", axum::routing::get(get_incident))
}
```

- [ ] **Step 5: Add the constants, params struct, and helper functions**

Add these directly above `async fn get_incident(...)`:

```rust
/// Page size when the caller does not ask for one. Same numeric value as
/// `routes::trains::DEFAULT_SEARCH_LIMIT` -- no reason for this route's
/// page size to differ -- but declared as its own constant, not shared,
/// since the two routes have no reason to be coupled (Decision 4 of the
/// design spec).
const DEFAULT_INCIDENT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected --
/// same rationale and same numeric value as
/// `routes::trains::MAX_SEARCH_LIMIT`.
const MAX_INCIDENT_SEARCH_LIMIT: i64 = 200;

/// `#[serde(deny_unknown_fields)]` for the same reason
/// `trains.rs::TrainSearchParams` has it -- see this crate's established
/// posture: a misspelled filter name must 400, not silently no-op. See
/// Correction 3 of the design spec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentSearchParams {
    /// Optional. Comma-separated ATOC codes, e.g. `operator=SW,VT` --
    /// matches this codebase's existing multi-value convention
    /// (`GET /Line/{ids}/Status`'s `ids`), not repeated query keys.
    /// Matches if the incident's `operators` array overlaps this set at
    /// all (an "any of" filter).
    operator: Option<String>,
    /// Optional. A single catalogue line id (`app.config.lines`, never a
    /// custom line). Resolved server-side to that line's own station list,
    /// then applied as a station-overlap filter -- see this route's own
    /// doc comment on `search_incidents` for the named approximation this
    /// implies. An id that doesn't resolve in `app.config.lines` is a
    /// `400` ("unknown line"), not a 404 or a silently-empty result.
    line: Option<String>,
    /// Optional, RFC3339. Inclusive lower bound on `first_seen_at`.
    from: Option<String>,
    /// Optional, RFC3339. Inclusive upper bound on `first_seen_at`.
    to: Option<String>,
    /// Optional. `true` = planned works only, `false` = unplanned only,
    /// omitted = either.
    planned: Option<bool>,
    /// Optional. `true` = cleared only, `false` = active only, omitted =
    /// either. Deliberately not a hidden default filter.
    cleared: Option<bool>,
    /// Optional. Inclusive lower bound on the raw `priority` integer. No
    /// documented "major"/"minor" mapping exists -- this is a raw numeric
    /// range over an unexplained feed value.
    priority_min: Option<i32>,
    /// Optional. Inclusive upper bound. A `400` if both bounds are given
    /// and `priority_min > priority_max`.
    priority_max: Option<i32>,
    /// Optional page size, 1..=`MAX_INCIDENT_SEARCH_LIMIT`. Over-large is
    /// clamped, not rejected; non-positive or unparseable is a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`.
    after: Option<String>,
}

/// Parses and bounds the page size -- same shape as
/// `routes::trains::normalize_limit`, kept as its own local copy since the
/// two routes' constants are deliberately not shared.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_INCIDENT_SEARCH_LIMIT);
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
    Ok(parsed.min(MAX_INCIDENT_SEARCH_LIMIT))
}

/// Parses a caller-supplied RFC3339 timestamp for `from`/`to`.
fn normalize_rfc3339(label: &str, raw: &str) -> Result<chrono::DateTime<chrono::Utc>, (StatusCode, String)> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("{label} must be an RFC3339 timestamp"),
            )
        })
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"{RFC3339 first_seen_at}|{incident_id}"` -- same opaque-token shape as
/// `routes::trains::encode_cursor`. Not signed: it names a public row on
/// an unauthenticated route.
fn encode_cursor(cursor: &queries::IncidentSearchCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.first_seen_at.to_rfc3339(),
        cursor.incident_id
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently dropped -- dropping it would restart the caller at page 1
/// while their UI appended the response as page 2, duplicating rows.
fn decode_cursor(raw: &str) -> Result<queries::IncidentSearchCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [first_seen_at, incident_id] = parts.as_slice() else {
        return Err(invalid());
    };
    let first_seen_at = chrono::DateTime::parse_from_rfc3339(first_seen_at)
        .map_err(|_| invalid())?
        .with_timezone(&chrono::Utc);
    Ok(queries::IncidentSearchCursor {
        first_seen_at,
        incident_id: (*incident_id).to_string(),
    })
}

/// Renders one `IncidentSummaryRow` as camelCase JSON -- the `IncidentSummary`
/// shape `frontend/lib/types.ts` declares.
fn incident_summary_json(row: &queries::IncidentSummaryRow) -> Value {
    json!({
        "incidentId": row.incident_id,
        "summary": row.summary,
        "operators": row.operators,
        "affectedStations": row.affected_stations,
        "priority": row.priority,
        "isPlanned": row.is_planned,
        "isCleared": row.is_cleared,
        "firstSeenAt": row.first_seen_at.to_rfc3339(),
        "fetchedAt": row.fetched_at.to_rfc3339(),
    })
}
```

- [ ] **Step 6: Add the handler**

Add directly above `async fn get_incident(...)`:

```rust
/// `GET /public/incidents` -- see
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decisions
/// 1-5. Unauthenticated, per this file's own established public-read
/// convention. The `line` filter is a named approximation (station-overlap
/// against the resolved catalogue line's own stations) -- it catches the
/// matcher's `StationHit`/`ExclusiveSegment`/`SharedSegment` tiers but
/// misses `KeywordOnly`/`OperatorOnly`; see Correction 1 of the design spec
/// and this route's own frontend copy (Decision 6) for where that
/// limitation must stay visible to a user, not just documented here.
async fn search_incidents(
    State(app): State<App>,
    Query(params): Query<IncidentSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let operators: Option<Vec<String>> = params.operator.as_deref().and_then(|raw| {
        let list: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if list.is_empty() { None } else { Some(list) }
    });

    let affected_stations: Option<Vec<String>> = match params
        .line
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(line_id) => {
            let Some(line) = app.config.lines.iter().find(|l| l.id == line_id) else {
                return Err((StatusCode::BAD_REQUEST, "unknown line".to_string()));
            };
            Some(line.stations.iter().map(|s| s.crs.clone()).collect())
        }
        None => None,
    };

    let from = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("from", s))
        .transpose()?;
    let to = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("to", s))
        .transpose()?;

    if let (Some(min), Some(max)) = (params.priority_min, params.priority_max) {
        if min > max {
            return Err((
                StatusCode::BAD_REQUEST,
                "priority_min must not exceed priority_max".to_string(),
            ));
        }
    }

    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    let page = queries::search_incidents(
        &app.database,
        operators,
        affected_stations,
        params.planned,
        params.cleared,
        params.priority_min,
        params.priority_max,
        from,
        to,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(json!({
        "results": page.results.iter().map(incident_summary_json).collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

```

- [ ] **Step 7: Run the tests to verify they pass**

Run:
```
DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api incident_search -- --ignored --test-threads=1
```
Expected: all tests in the new `db_tests` module pass.

- [ ] **Step 8: Run the full existing test suite for this file (regression check)**

Run:
```
cargo test -p api routes::incidents::tests
```
Expected: the pre-existing `to_incident_detail_json` unit tests still pass
unchanged.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/incidents.rs
git commit -m "Add GET /public/incidents search route for the incident archive"
```

---

## Task 4: Frontend — types + `IncidentSearchForm.tsx`

**Files:**
- Modify: `frontend/lib/types.ts`
- Create: `frontend/components/IncidentSearchForm.tsx`
- Test: `frontend/components/IncidentSearchForm.test.tsx`

**Interfaces:**
- Consumes: `GET /public/incidents`'s response shape from Task 3
  (`{ results: IncidentSummary[], nextCursor: string | null }`), `LineSummary`
  (`{ id, name, category, operators, source: 'catalogue' | 'custom' | 'tfl' }`,
  already defined in `frontend/lib/types.ts`), `Suggestion`
  (`{ code, name }`, already defined).
- Produces (consumed by Task 5): `export interface IncidentSummary`,
  `export interface IncidentSearchResponse` in `frontend/lib/types.ts`;
  `export function IncidentSearchForm({ lines, tocs, initialOperator,
  initialLine, initialFrom, initialTo }: { lines: LineSummary[]; tocs:
  Suggestion[]; initialOperator?: string; initialLine?: string; initialFrom?:
  string; initialTo?: string })` in `frontend/components/IncidentSearchForm.tsx`.

- [ ] **Step 1: Add the wire types to `frontend/lib/types.ts`**

Add after the existing `IncidentDetail` interface (after its closing `}` on
line 58):

```ts
/** One row from `GET /public/incidents`. Deliberately lighter than
 * `IncidentDetail` (no description, no validityPeriods, no history, no
 * currentlyAffectsLines) — see
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 7. */
export interface IncidentSummary {
  incidentId: string;
  summary: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string; // RFC3339
}

export interface IncidentSearchResponse {
  results: IncidentSummary[];
  nextCursor: string | null;
}
```

- [ ] **Step 2: Write the failing component tests**

Create `frontend/components/IncidentSearchForm.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { IncidentSearchForm } from './IncidentSearchForm';
import type { IncidentSearchResponse, LineSummary, Suggestion } from '@/lib/types';

// Same rationale as `TrainSearchForm.test.tsx`'s identical mock: `DatePickerInput`'s
// real popover calendar has no real `<input>` `fireEvent.change` can drive.
// Kept to the same `onChange(string | null)` contract this form actually
// depends on.
vi.mock('@mantine/dates', () => ({
  DatePickerInput: ({
    label,
    value,
    onChange,
  }: {
    label: string;
    value: string | null;
    onChange: (value: string | null) => void;
  }) => (
    <div>
      <label htmlFor={`test-date-${label}`}>{label}</label>
      <input
        id={`test-date-${label}`}
        value={value ?? ''}
        onChange={(event) => onChange(event.target.value || null)}
      />
    </div>
  ),
}));

const TEST_LINES: LineSummary[] = [
  { id: 'south-western', name: 'South Western Main Line', category: 'main', operators: ['SW'], source: 'catalogue' },
  { id: 'my-custom-line', name: 'My Custom Line', category: 'main', operators: ['SW'], source: 'custom' },
];
const TEST_TOCS: Suggestion[] = [
  { code: 'SW', name: 'South Western Railway' },
  { code: 'VT', name: 'Avanti West Coast' },
];

const fetchMock = vi.fn();

beforeEach(() => {
  vi.stubGlobal('fetch', fetchMock);
  fetchMock.mockReset();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function okResponse(body: IncidentSearchResponse) {
  return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);
}

function errorResponse() {
  return Promise.resolve({ ok: false, status: 500, json: () => Promise.resolve({}) } as Response);
}

function summary(overrides: Partial<IncidentSearchResponse['results'][number]> = {}) {
  return {
    incidentId: '1',
    summary: 'Signal failure at Woking',
    operators: ['VT'],
    affectedStations: ['WOK'],
    priority: 3,
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-08-30T09:00:00Z',
    fetchedAt: '2026-08-31T10:15:00Z',
    ...overrides,
  };
}

describe('IncidentSearchForm', () => {
  it('excludes a custom line from the Line dropdown, offering only catalogue lines', () => {
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    const input = screen.getByRole('combobox', { name: /Line \(optional\)/ });
    fireEvent.click(input);
    const optionText = screen.getAllByRole('option').map((o) => o.textContent);
    expect(optionText).toEqual(['South Western Main Line']);
  });

  it('applies a 30-day default "from" floor when no initial filters are given', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
    const from = requestedUrl.searchParams.get('from');
    expect(from).not.toBeNull();
    const daysAgo = Math.round((Date.now() - new Date(from as string).getTime()) / (1000 * 60 * 60 * 24));
    expect(daysAgo).toBeGreaterThanOrEqual(29);
    expect(daysAgo).toBeLessThanOrEqual(31);
  });

  it('builds a comma-joined operator query parameter from multiple selected operators', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);

    const input = screen.getByRole('combobox', { name: /Operator \(optional\)/ });
    fireEvent.click(input);
    fireEvent.click(await screen.findByRole('option', { name: /SW/ }));
    fireEvent.click(input);
    fireEvent.click(await screen.findByRole('option', { name: /VT/ }));

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const requestedUrl = new URL(fetchMock.mock.calls[0][0], 'http://localhost');
    expect(requestedUrl.searchParams.get('operator')).toBe('SW,VT');
  });

  it('"Load more" appends rows rather than replacing them, and disappears once nextCursor is null', async () => {
    fetchMock
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '1' })], nextCursor: 'cursor-a' }))
      .mockReturnValueOnce(okResponse({ results: [summary({ incidentId: '2' })], nextCursor: null }));

    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Signal failure at Woking');
    expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(1);

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
    await waitFor(() => expect(screen.getAllByText('Signal failure at Woking')).toHaveLength(2));
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('renders the empty-results message, not a blank screen', async () => {
    fetchMock.mockReturnValue(okResponse({ results: [], nextCursor: null }));
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('No incidents match these filters.');
  });

  it('renders an error message on a failed search, not a thrown error', async () => {
    fetchMock.mockReturnValue(errorResponse());
    renderWithMantine(<IncidentSearchForm lines={TEST_LINES} tocs={TEST_TOCS} />);
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    await screen.findByText('Search failed');
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run:
```
cd frontend && npx vitest run components/IncidentSearchForm.test.tsx
```
Expected: fails to resolve `./IncidentSearchForm` (module does not exist
yet).

- [ ] **Step 4: Write the component**

Create `frontend/components/IncidentSearchForm.tsx`:

```tsx
'use client';

import { useState, type FormEvent } from 'react';
import {
  Alert,
  Badge,
  Button,
  Group,
  MultiSelect,
  NumberInput,
  ScrollArea,
  SegmentedControl,
  Select,
  Stack,
  Text,
} from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentSearchResponse, IncidentSummary, LineSummary, Suggestion } from '@/lib/types';

type DatePreset = '7d' | '30d' | '90d' | 'all';

function calendarDaysAgo(days: number): string {
  return dayjs().subtract(days, 'day').format('YYYY-MM-DD');
}

/** Exactly one of three mutually-exclusive states, mirroring
 * `TrainSearchForm.tsx`'s own `Results` type -- `nextCursor` lives INSIDE
 * the success variant for the same reason it does there: it must not
 * survive a state transition (a fresh search, or an error) it does not
 * belong to. */
type Results = { rows: IncidentSummary[]; nextCursor: string | null } | 'error' | null;

/** `/incidents`'s one interactive component: filter form plus a
 * cursor-paginated, "Load more"-driven results list over
 * `GET /public/incidents`. Mirrors `TrainSearchForm.tsx`'s client-side
 * fetch/`useState`/"Load more" shape exactly (Decision 6 of
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md), not the
 * `HistoryRangePicker`/server-searchParams shape `/lines/[id]/history` uses
 * -- this filter set (six independent optional filters) is a closer match
 * to `TrainSearchForm`'s multi-filter interactive search than to that range
 * picker's single from/to control.
 *
 * `lines`/`tocs` are fetched once, server-side, by `app/incidents/page.tsx`
 * and passed down as props -- the same "reference data fetched once by the
 * page" shape `AllLinesPage`/`AllLinesTable` already establishes for `tocs`.
 * `lines` is filtered to catalogue lines only (`source === 'catalogue'`)
 * before it is ever offered as a filter option, matching the backend's own
 * scoping (Decision 2): there is no way to even attempt filtering by a
 * private custom line from this form.
 *
 * Defaults to a 30-day `from` floor on first load when no initial filters
 * are supplied -- an unfiltered "all incidents ever ingested" default view
 * is the direct equivalent of the list-spamminess failure mode this
 * codebase's own research already diagnosed for `/lines/[id]/history`'s
 * Timeline tab, just multiplied across the whole network. "All time" stays
 * one preset click away; this is a default, not a ceiling. */
export function IncidentSearchForm({
  lines,
  tocs,
  initialOperator = '',
  initialLine = '',
  initialFrom = '',
  initialTo = '',
}: {
  lines: LineSummary[];
  tocs: Suggestion[];
  initialOperator?: string;
  initialLine?: string;
  initialFrom?: string;
  initialTo?: string;
}) {
  const catalogueLines = lines.filter((line) => line.source === 'catalogue');

  const [operators, setOperators] = useState<string[]>(
    initialOperator ? initialOperator.split(',').filter(Boolean) : [],
  );
  const [lineId, setLineId] = useState<string | null>(initialLine || null);
  const [fromDate, setFromDate] = useState<string | null>(
    initialFrom ? initialFrom.slice(0, 10) : calendarDaysAgo(30),
  );
  const [toDate, setToDate] = useState<string | null>(initialTo ? initialTo.slice(0, 10) : null);
  const [preset, setPreset] = useState<DatePreset | null>(initialFrom ? null : '30d');
  const [plannedFilter, setPlannedFilter] = useState<'all' | 'planned' | 'realtime'>('all');
  const [clearedFilter, setClearedFilter] = useState<'all' | 'active' | 'cleared'>('all');
  const [priorityMin, setPriorityMin] = useState<number | ''>('');
  const [priorityMax, setPriorityMax] = useState<number | ''>('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  const priorityValid = priorityMin === '' || priorityMax === '' || priorityMin <= priorityMax;

  function applyPreset(next: DatePreset) {
    setPreset(next);
    if (next === 'all') {
      setFromDate(null);
      setToDate(null);
      return;
    }
    const days = next === '7d' ? 7 : next === '30d' ? 30 : 90;
    setFromDate(calendarDaysAgo(days));
    setToDate(null);
  }

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParamsFor() {
    const params = new URLSearchParams();
    if (operators.length > 0) params.set('operator', operators.join(','));
    if (lineId) params.set('line', lineId);
    if (fromDate) params.set('from', new Date(fromDate).toISOString());
    if (toDate) params.set('to', new Date(toDate).toISOString());
    if (plannedFilter === 'planned') params.set('planned', 'true');
    if (plannedFilter === 'realtime') params.set('planned', 'false');
    if (clearedFilter === 'active') params.set('cleared', 'false');
    if (clearedFilter === 'cleared') params.set('cleared', 'true');
    if (priorityMin !== '') params.set('priority_min', String(priorityMin));
    if (priorityMax !== '') params.set('priority_max', String(priorityMax));
    return params;
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!priorityValid || searching) return;
    setSearching(true);
    try {
      const response = await fetch(`/api/incidents?${searchParamsFor().toString()}`);
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: IncidentSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleLoadMore() {
    if (results === null || results === 'error') return;
    if (results.nextCursor === null || loadingMore) return;
    setLoadingMore(true);
    try {
      const params = searchParamsFor();
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/incidents?${params.toString()}`);
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' ? { rows: current.rows, nextCursor: null } : current,
        );
        return;
      }
      const body: IncidentSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' ? { rows: current.rows, nextCursor: null } : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }

  function resultsContent() {
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
          Press Search to browse incidents across the network.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search incidents right now. Try again.
        </Alert>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No incidents match these filters.
        </Text>
      );
    }
    return (
      <>
        <ScrollArea mah={520} offsetScrollbars>
          <Stack gap="sm">
            {results.rows.map((row) => (
              <Stack key={row.incidentId} gap={4}>
                <Group justify="space-between" wrap="nowrap">
                  <TextLink href={`/incidents/${encodeURIComponent(row.incidentId)}`} underline="always">
                    {row.summary}
                  </TextLink>
                  <Text size="xs" c="dimmed">
                    {formatDateTime(row.firstSeenAt)}
                  </Text>
                </Group>
                <Group gap="xs">
                  <Badge color={row.isPlanned ? 'blue' : 'orange'}>
                    {row.isPlanned ? 'Planned Work' : 'Real-Time'}
                  </Badge>
                  <Badge color={row.isCleared ? 'gray' : 'green'}>{row.isCleared ? 'Cleared' : 'Active'}</Badge>
                  {row.operators.map((code) => (
                    <Badge key={code} variant="outline" color="grape">
                      {code}
                    </Badge>
                  ))}
                  {row.affectedStations.map((crs) => (
                    <Badge key={crs} variant="outline" color="gray">
                      {crs}
                    </Badge>
                  ))}
                </Group>
              </Stack>
            ))}
          </Stack>
        </ScrollArea>
        {results.nextCursor !== null && (
          <Group>
            <Button variant="default" size="xs" onClick={handleLoadMore} disabled={loadingMore} loading={loadingMore}>
              Load more
            </Button>
          </Group>
        )}
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <MultiSelect
        label="Operator (optional)"
        placeholder="Any operator"
        description="Matches an incident whose operators overlap any of these -- not 'scoped to exactly this operator.'"
        data={tocs.map((toc) => ({ value: toc.code, label: `${toc.code} — ${toc.name}` }))}
        value={operators}
        onChange={setOperators}
        searchable
        clearable
      />
      <Select
        label="Line (optional)"
        placeholder="Any line"
        description="Incidents affecting stations on this line -- a station-overlap approximation, not a real line match. It can miss incidents that only matched a line by keyword or shared operator, with no station in common."
        data={catalogueLines.map((line) => ({ value: line.id, label: line.name }))}
        value={lineId}
        onChange={setLineId}
        searchable
        clearable
      />
      <Group gap="sm">
        <Button variant={preset === '7d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('7d')}>
          7 days
        </Button>
        <Button variant={preset === '30d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('30d')}>
          30 days
        </Button>
        <Button variant={preset === '90d' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('90d')}>
          90 days
        </Button>
        <Button variant={preset === 'all' ? 'filled' : 'light'} size="xs" onClick={() => applyPreset('all')}>
          All time
        </Button>
      </Group>
      <Group align="end">
        <DatePickerInput
          label="From (optional)"
          value={fromDate}
          onChange={(value) => {
            setFromDate(value);
            setPreset(null);
          }}
          clearable
        />
        <DatePickerInput
          label="To (optional)"
          value={toDate}
          onChange={(value) => {
            setToDate(value);
            setPreset(null);
          }}
          clearable
        />
      </Group>
      <SegmentedControl
        value={plannedFilter}
        onChange={(value) => setPlannedFilter(value as 'all' | 'planned' | 'realtime')}
        data={[
          { label: 'All', value: 'all' },
          { label: 'Planned work', value: 'planned' },
          { label: 'Real-time', value: 'realtime' },
        ]}
      />
      <SegmentedControl
        value={clearedFilter}
        onChange={(value) => setClearedFilter(value as 'all' | 'active' | 'cleared')}
        data={[
          { label: 'All', value: 'all' },
          { label: 'Active', value: 'active' },
          { label: 'Cleared', value: 'cleared' },
        ]}
      />
      <Group grow align="flex-start">
        <NumberInput
          label="Priority (raw feed value — meaning undocumented)"
          description="Minimum, inclusive."
          value={priorityMin}
          onChange={(value) => setPriorityMin(typeof value === 'number' ? value : '')}
        />
        <NumberInput
          label="Priority (raw feed value — meaning undocumented)"
          description="Maximum, inclusive."
          value={priorityMax}
          onChange={(value) => setPriorityMax(typeof value === 'number' ? value : '')}
          error={!priorityValid ? 'Minimum must not exceed maximum' : null}
        />
      </Group>
      <Text size="xs" c="dimmed">
        Priority is a raw feed value from the Knowledgebase incident data with no documented
        &quot;major&quot;/&quot;minor&quot; meaning — shown as-is, not a severity scale.
      </Text>
      <Group>
        <Button type="submit" disabled={!priorityValid || searching}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
    </Stack>
  );
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```
cd frontend && npx vitest run components/IncidentSearchForm.test.tsx
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/components/IncidentSearchForm.tsx frontend/components/IncidentSearchForm.test.tsx
git commit -m "Add IncidentSearchForm client component for the incident archive"
```

---

## Task 5: Frontend — `/incidents` page shell and cross-links

**Files:**
- Create: `frontend/app/incidents/page.tsx`
- Test: `frontend/app/incidents/page.test.tsx`
- Modify: `frontend/app/layout.tsx` (nav link)
- Modify: `frontend/app/lines/page.tsx` (header link)

**Interfaces:**
- Consumes: `IncidentSearchForm` (Task 4), `getAllLines()` and
  `getAllTocs()` (both already exported from `frontend/lib/api.ts`,
  returning `Promise<LineSummary[]>` and `Promise<Suggestion[]>`
  respectively).
- Produces: the `/incidents` route, reachable from the main nav
  (`app/layout.tsx`) and from `/lines`'s header, per the design spec's
  cross-linking requirement (a page must not be built and left
  unreachable).

- [ ] **Step 1: Write the failing page test**

Create `frontend/app/incidents/page.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentsPage from './page';
import * as api from '@/lib/api';
import type { LineSummary, Suggestion } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getAllLines: vi.fn(), getAllTocs: vi.fn() };
});

const TEST_LINES: LineSummary[] = [
  { id: 'south-western', name: 'South Western Main Line', category: 'main', operators: ['SW'], source: 'catalogue' },
];
const TEST_TOCS: Suggestion[] = [{ code: 'SW', name: 'South Western Railway' }];

describe('IncidentsPage', () => {
  it('renders the heading and the search form with fetched reference data', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Search' })).toBeInTheDocument();
  });

  it('degrades to empty reference-data lists if either fetch fails, rather than crashing the page', async () => {
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('network error'));
    vi.mocked(api.getAllTocs).mockRejectedValue(new Error('network error'));

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
  });

  it('passes searchParams through as initial filter values', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(
      await IncidentsPage({
        searchParams: Promise.resolve({ operator: 'SW,VT', line: 'south-western' }),
      }),
    );

    // The MultiSelect renders its selected values as removable pills with
    // this exact label text -- confirms initialOperator was parsed and
    // passed through rather than dropped.
    expect(screen.getByText('SW — South Western Railway')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run:
```
cd frontend && npx vitest run app/incidents/page.test.tsx
```
Expected: fails to resolve `./page` (file does not exist yet).

- [ ] **Step 3: Write the page**

Create `frontend/app/incidents/page.tsx`:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { getAllLines, getAllTocs } from '@/lib/api';
import { IncidentSearchForm } from '@/components/IncidentSearchForm';

export const revalidate = 0;

/** `/incidents` -- the cross-network incident archive/search page. See
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md. Thin
 * Server Component shell, mirroring `app/trains/page.tsx`'s structure:
 * reads initial filter values out of `searchParams` (so a filtered link is
 * shareable), fetches the two reference-data lists the filter dropdowns
 * need (both already used the same way on `app/lines/page.tsx`), and
 * passes everything down into the interactive `IncidentSearchForm`. Either
 * reference-data fetch failing degrades to an empty option list rather
 * than crashing the page -- same posture `AllLinesPage` already takes for
 * `getAllTocs()`. */
export default async function IncidentsPage({
  searchParams,
}: {
  searchParams: Promise<{
    operator?: string | string[];
    line?: string | string[];
    from?: string | string[];
    to?: string | string[];
  }>;
}) {
  const { operator, line, from, to } = await searchParams;
  const operatorParam = Array.isArray(operator) ? operator[0] : operator;
  const lineParam = Array.isArray(line) ? line[0] : line;
  const fromParam = Array.isArray(from) ? from[0] : from;
  const toParam = Array.isArray(to) ? to[0] : to;

  const [lines, tocs] = await Promise.all([getAllLines().catch(() => []), getAllTocs().catch(() => [])]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Incident Archive</Title>
      <Text c="dimmed">
        Search Knowledgebase incidents across the whole network, independent of which line you were
        looking at. Defaults to the last 30 days — use &quot;All time&quot; to see everything this app
        has ever ingested.
      </Text>
      <IncidentSearchForm
        lines={lines}
        tocs={tocs}
        initialOperator={operatorParam}
        initialLine={lineParam}
        initialFrom={fromParam}
        initialTo={toParam}
      />
    </Stack>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run:
```
cd frontend && npx vitest run app/incidents/page.test.tsx
```
Expected: all 3 tests pass.

- [ ] **Step 5: Add the main nav link**

In `frontend/app/layout.tsx`, change:
```tsx
                      <TextLink href="/trains">Find a Train</TextLink>
                      <TrackedTrainsNavItem />
```
to:
```tsx
                      <TextLink href="/trains">Find a Train</TextLink>
                      <TextLink href="/incidents">Incident Archive</TextLink>
                      <TrackedTrainsNavItem />
```

- [ ] **Step 6: Add the `/lines` header link**

In `frontend/app/lines/page.tsx`, change:
```tsx
        <Group justify="space-between" align="baseline">
          <Title order={1}>All Lines</Title>
          <TextLink href="/lines/new">New custom line</TextLink>
        </Group>
```
to:
```tsx
        <Group justify="space-between" align="baseline">
          <Title order={1}>All Lines</Title>
          <Group gap="md">
            <TextLink href="/incidents">Incident Archive</TextLink>
            <TextLink href="/lines/new">New custom line</TextLink>
          </Group>
        </Group>
```

- [ ] **Step 7: Run the existing layout and lines-page tests to confirm no regression**

Run:
```
cd frontend && npx vitest run app/layout.test.tsx app/lines/page.test.tsx
```
(If either file does not exist, skip it — this step only guards against
breaking an existing test, not adding one.) Expected: no new failures.

- [ ] **Step 8: Manually verify the two new links resolve**

Run the frontend dev server and click through:
1. Main nav → "Incident Archive" → lands on `/incidents` with the heading
   and search form visible.
2. `/lines` → "Incident Archive" (next to "New custom line") → same
   destination.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/incidents/page.tsx frontend/app/incidents/page.test.tsx frontend/app/layout.tsx frontend/app/lines/page.tsx
git commit -m "Add /incidents archive page shell and cross-link it from the nav and /lines"
```

---

## Self-Review

**Spec coverage** — every decision in
`docs/superpowers/specs/2026-09-12-incident-archive-design.md` maps to a
task:

- Decision 1 (route location/registration) → Task 3, Step 4.
- Decision 2 (operator/line filters, line-is-an-approximation) → Task 3
  (params struct, line resolution, unknown-line 400) and Task 4 (Line
  `Select`'s honest description copy). The approximation's concrete
  regression (excludes a no-station-overlap incident) is tested at both the
  data layer (Task 2's `search_incidents_affected_stations_filter_...` test)
  and the route layer (Task 3's
  `incident_search_known_line_resolves_to_station_overlap_and_excludes_a_no_overlap_incident`
  test).
- Decision 3 (priority as a raw range, not a severity enum) → Task 3
  (`priority_min`/`priority_max`, 400 on `min > max`) and Task 4 (the
  `NumberInput` labels and the dimmed disclaimer text).
- Decision 4 (keyset cursor matching `trains_search` exactly) → Task 2
  (cursor struct, query shape) and Task 3 (`encode_cursor`/`decode_cursor`,
  `deny_unknown_fields`, `nextCursor` explicit null).
- Decision 5 (one new index, no others) → Task 1.
- Decision 6 (frontend `TrainSearchForm`-shaped client search, 30-day
  default, presets, results list, empty/error states) → Task 4 and Task 5.
- Decision 7 (lighter list rows, no description/validityPeriods/history) →
  Task 2's `IncidentSummaryRow` and Task 3's `incident_summary_json`
  omitting both fields entirely (asserted directly in
  `incident_search_renders_camel_case_rows_with_no_leaked_snake_case_fields`).
- Cross-linking (a page must not ship unreachable) → Task 5, Steps 5-6.
- Retention/pruning scoped out → not implemented anywhere in this plan;
  called out explicitly in Global Constraints above so no task
  accidentally reintroduces it.
- "No total result counts" / "No full-text search" / "No editing" — none of
  these appear anywhere in this plan's tasks, consistent with staying out
  of scope.

**Placeholder scan** — no `TODO`/`TBD`/"add appropriate ..."/"similar to
Task N" phrasing appears anywhere above; every step that touches code
includes the actual code, not a description of it.

**Type consistency** — traced end to end:
`queries::IncidentSummaryRow` (Task 2, snake_case Rust fields) →
`incident_summary_json` (Task 3, renders exactly `incidentId`, `summary`,
`operators`, `affectedStations`, `priority`, `isPlanned`, `isCleared`,
`firstSeenAt`, `fetchedAt`) → `IncidentSummary` (Task 4 TypeScript
interface, same nine fields, same names, same order) → consumed by
`IncidentSearchForm`'s `results.rows: IncidentSummary[]` (Task 4) → passed
into the page via `lines: LineSummary[]` / `tocs: Suggestion[]` props (Task
5), both pre-existing types left unchanged. `IncidentSearchResponse` (Task
4) matches the route's `{ "results": [...], "nextCursor": ... }` envelope
(Task 3) field-for-field. `queries::IncidentSearchCursor` (Task 2) is the
only type `encode_cursor`/`decode_cursor` (Task 3) construct or read —
never a bespoke shape.

---

## Execution Handoff

Plan complete and saved to
`docs/superpowers/plans/2026-09-12-incident-archive.md`. Two execution
options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task,
   review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using
   `executing-plans`, batch execution with checkpoints.

Which approach?
