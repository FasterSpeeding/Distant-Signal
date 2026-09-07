# TfL Incident Page (v1, Option B Live Snapshot) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a TfL-sourced disruption a real destination page — reachable
from `DisruptionDetail.tsx` the same way a Knowledgebase incident already
links to `/incidents/[id]` — that honestly shows "what TfL is reporting
right now for this line": current severity badge(s), the raw compound
status text verbatim, and a last-updated timestamp. No new table, no
migration, no history timeline, no segment-level severity parsing.

**Architecture:** A new, minimal `GET /public/tfl-lines/{lineId}` route
(`crates/api/src/routes/tfl_lines.rs`, mirroring `routes/incidents.rs`'s
`Path(String)` + 404-on-unknown shape) reads the current `line_status` row
for one `source = 'tfl'` line id via a new query function,
`queries::tfl_line_status_by_id`, and renders it through the
**already-existing** `render::to_tfl_shape` — the exact function
`GET /Line/{ids}/Status` already uses, so the response is a plain
`LineStatusReport`-shaped JSON object the frontend already has a type for.
A new frontend route, `frontend/app/tfl-lines/[id]/page.tsx`, fetches it
through a new `getTflLine` client function and renders a flat,
`/incidents/[id]`-style detail page (no accordion/filter chrome — that's
`IssueList`'s job elsewhere and does not fit a single-line, single-purpose
detail view): line name, one `StatusBadge` + verbatim `reason` text per
simultaneous `LineStatus` entry, each entry's validity period, and a
"Last updated" timestamp from `computedAt`. A new frontend helper,
`tflLineIdFromSource`, mirrors `incidentIdFromSource`'s shape to extract a
TfL line id from a `tfl-line-status-{lineId}` `Disruption.source` string
and wires a new link into `DisruptionDetail.tsx`, alongside (never
replacing) the existing Knowledgebase-incident link logic.

**Tech Stack:** Rust (`axum`, `sqlx` runtime-checked queries, `anyhow`),
Next.js/React (Server Component page, Mantine UI), Vitest +
`@testing-library/react` for frontend tests, `cargo test` (with a
`#[tokio::test] #[ignore]`-gated live-database test) for the backend query.

**Spec:** `docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md`
(read in full; every reference to "the design doc" below means this
document). Its Section 2 recommendation (Option B: live snapshot, no
synthetic incident identity) and Section 3 recommendation (defer
segment-level severity parsing; render the raw compound status string
verbatim under one badge) are **settled decisions this plan does not
re-litigate** — see the task prompt that produced this plan for the exact
wording.

## Decisions this plan resolves (the design doc's own Section 7, Task 1)

The design doc's Section 7 "Rough task breakdown" flagged two open
product questions as blocking precise scoping ("Task 1"). This plan
resolves both, so every task below is concrete:

1. **Dedicated page vs. extending `/lines/[id]`/`/lines/[id]/history`:
   dedicated.** `/lines/[id]` already renders a TfL line's current
   statuses (confirmed by reading `frontend/app/lines/[id]/page.tsx:163`,
   `IssueList` over `report.lineStatuses`) and, for a bare TfL line id
   like `tfl-victoria` (not merged into an NR line the way Elizabeth line
   is), that page already works end-to-end via the existing
   `GET /Line/{id}/Status?detail=true` route
   (`crates/api/src/routes/line_status.rs:227-279`,
   `queries::line_status_for_ids`, `crates/api/src/data/queries.rs:1048-1056`
   — it filters only on `line_id = ANY($1)`, so a `tfl-`-prefixed id
   matches its own `source = 'tfl'` row with no special-casing needed).
   But that page also carries an Edit/Delete-line management header,
   half-hourly trend charts, and a full `IssueList` filter/accordion UI —
   all real, useful chrome for *managing a line*, none of it appropriate
   as the destination for "tell me what this one disruption is." A
   dedicated page, deliberately narrower, is the better fit for the same
   reason `/incidents/[id]` itself is not just "`/lines/[id]` with an
   incident ID in the URL." This plan builds `frontend/app/tfl-lines/[id]/page.tsx`
   as a new, separate route, backed by a new, separate (but very thin)
   backend route — not an extension of either existing line page.
2. **History section: out of scope for v1.** Ship "current status only."
   `line_status_history` remains fully available for a later iteration
   (no schema work needed to add it — `queries::line_status_history_for_range`,
   `crates/api/src/data/queries.rs:1058-1082`, already exists and is
   already generic over any `line_id`), but this plan does not wire it
   into the new page. This matches the design doc's own Section 5 framing
   ("if Task 1 keeps it in v1") and the parent task's explicit "No history
   timeline (explicitly out of scope per the design doc)" instruction.

## Global Constraints

- **No new migration, no new table.** Every task reads/writes only the
  existing `line_status`/`line_status_history` tables, exactly as
  `queries::upsert_tfl_line_status` (`crates/api/src/data/queries.rs:387-457`)
  already writes them. If any task in this plan seems to need a schema
  change, stop and re-read the design doc's Section 4 — it doesn't.
- **Raw TfL `reason` text is rendered completely verbatim, as plain
  text, under one severity badge.** No client-side splitting, no
  re-wording, no per-segment styling — this is Section 3's deferred-not-wrong
  v1 decision. Do not add `sanitizeDescription` (TfL sends no HTML in
  `reason`, unlike Knowledgebase's `description` — the design doc's
  Section 5 states this explicitly).
- **`incidentIdFromSource` (`frontend/lib/incidents.ts:11-14`) keeps its
  exact current behavior for every source string, including TfL ones
  (still returns `null` for `tfl-line-status-*`).** The new
  `tflLineIdFromSource` helper is additive, in the same file, not a
  change to that function's contract — the existing test
  `frontend/lib/incidents.test.ts:21-23` (`incidentIdFromSource('tfl-line-status-northern')`
  returns `null`) must keep passing unmodified.
- **New JSON responses stay camelCase**, produced the same way every
  other hand-built response in this crate is (`serde_json::json!()`
  field-by-field via `render::to_tfl_shape`, not a `#[derive(Serialize)]
  #[serde(rename_all = "camelCase")]` struct with nested nested types —
  see `crates/api/src/routes/incidents.rs:52-60`'s own documented
  rationale for why the derive approach is a trap here).
- **No "currently affects [lines]" section, no "first seen" field, no
  history timeline** on the new page — all three are explicit
  Section 5/6 non-goals for Option B, not omissions to fix later.
- **Backend query tests that hit a real database** follow this crate's
  existing convention exactly: a `#[cfg(test)] mod <name>_query_tests`
  block, a private `test_pool()` reading `DATABASE_URL` via
  `PgPoolOptions`, `#[tokio::test]` + `#[ignore = "requires a live
  database; run with \`cargo test -p api <test_name> -- --ignored\`"]`,
  seed via raw `sqlx::query` INSERT with `ON CONFLICT DO UPDATE`, and an
  explicit `DELETE` cleanup at the end of the test body (see
  `crates/api/src/data/queries.rs:2094-2123`, the `incident_by_id` test,
  for the exact pattern to copy). Do **not** use `#[sqlx::test]` — it is
  not used anywhere in this crate today.
- **Frontend tests** use Vitest + `@testing-library/react` +
  `renderWithMantine` (`frontend/test/render`), run via `npm test -- <file>`
  from the `frontend/` directory (`frontend/package.json`'s `"test":
  "vitest run"`). Page-level tests mock `@/lib/api` with `vi.mock` and
  `next/navigation`'s `notFound` with a no-op `vi.fn()`, exactly as
  `frontend/app/incidents/[id]/page.test.tsx:1-21` does.

---

## Task 1: Backend query — `queries::tfl_line_status_by_id`

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add function after
  `line_status_history_for_range`, which ends at line 1082; add a new
  test module at the end of the file, after line 2457)

**Interfaces:**
- Produces: `pub async fn tfl_line_status_by_id(pool: &PgPool, line_id:
  &str) -> Result<Option<LineStatusRow>>` — consumed by Task 2's route
  handler. `LineStatusRow` is the existing struct at
  `crates/api/src/data/queries.rs:1012-1019` (`id`, `name`, `mode_name`,
  `operators: Vec<String>`, `statuses: Vec<common::LineStatus>`,
  `computed_at: chrono::DateTime<chrono::Utc>`) — unchanged, reused as-is.

- [ ] **Step 1: Write the failing test**

Add this new module at the very end of `crates/api/src/data/queries.rs`
(after the existing `crs_for_tiploc_resolves_a_known_tiploc_and_none_for_an_unknown_one`
test, which is the last item in the file):

```rust
// Tested at the query level rather than through a full route/router
// harness -- same posture as `schedule_feed_ingest_query_tests` above:
// `routes/tfl_lines.rs`'s handler is a thin serialize wrapper around this
// function plus the already-tested `render::to_tfl_shape`, so the real
// SQL (and its `source = 'tfl'` filter, which is the whole reason this
// isn't just a call to the existing `line_status_for_ids`) is what needs
// live-database coverage.
#[cfg(test)]
mod tfl_line_query_tests {
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api tfl_line_status_by_id -- --ignored`"]
    async fn tfl_line_status_by_id_finds_a_seeded_tfl_row_and_none_for_an_unknown_id() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) VALUES \
                ('TEST-TFL-LINE', 'Test Victoria', 'tube', '{TfL}', \
                 '[{\"severity\":6,\"reason\":\"Severe delays due to a signal failure\", \
                    \"validity\":{\"from_date\":\"2026-01-01T00:00:00Z\",\"to_date\":null,\"is_now\":true}, \
                    \"data_quality\":\"tfl\"}]', \
                 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let found = tfl_line_status_by_id(&pool, "TEST-TFL-LINE")
            .await
            .expect("query")
            .expect("row should exist");
        assert_eq!(found.name, "Test Victoria");
        assert_eq!(found.statuses.len(), 1);
        assert_eq!(
            found.statuses[0].reason,
            "Severe delays due to a signal failure"
        );

        let missing = tfl_line_status_by_id(&pool, "TEST-TFL-LINE-DOES-NOT-EXIST")
            .await
            .expect("query");
        assert!(missing.is_none());

        sqlx::query("DELETE FROM line_status WHERE line_id = 'TEST-TFL-LINE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api tfl_line_status_by_id_ignores_non_tfl -- --ignored`"]
    async fn tfl_line_status_by_id_ignores_a_row_with_a_non_tfl_source() {
        // A national-rail (aggregator-written) row sharing this function's
        // lookup id must never be returned here -- this route exists
        // specifically for TfL-sourced rows, and `source` is the only
        // column that distinguishes them (line_id alone is not enough:
        // see the `source` column's own migration,
        // `20260822120000_line_status_source.sql`).
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) VALUES \
                ('TEST-NR-LINE', 'Test NR Line', 'national-rail', '{VT}', '[]', 'aggregator') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let result = tfl_line_status_by_id(&pool, "TEST-NR-LINE")
            .await
            .expect("query");
        assert!(
            result.is_none(),
            "a non-TfL-sourced row must not be returned by this lookup"
        );

        sqlx::query("DELETE FROM line_status WHERE line_id = 'TEST-NR-LINE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p api tfl_line_status_by_id -- --ignored`
Expected: a compile error — `tfl_line_status_by_id` is not yet defined.

- [ ] **Step 3: Implement the function**

Insert immediately after `line_status_history_for_range`
(`crates/api/src/data/queries.rs:1058-1082`), before `pub struct
DailyStatsRow`:

```rust
/// The current `line_status` row for one TfL-sourced line, or `None` if
/// no row exists for `line_id`, or a row exists but was written by the
/// aggregator (`source = 'tfl'` is required) rather than `poller-tfl`.
/// Backs `GET /public/tfl-lines/{lineId}` (`routes/tfl_lines.rs`) --
/// deliberately its own query rather than a reuse of `line_status_for_ids`
/// (which has no `source` filter at all): a caller of this specific route
/// asked for a *TfL* line's status, and a same-named non-TfL row (there
/// is no such collision today, since `TFL_LINE_ID_PREFIX` namespaces every
/// TfL id, but nothing stops that changing) must 404, not silently render
/// as if it were a TfL row.
pub async fn tfl_line_status_by_id(pool: &PgPool, line_id: &str) -> Result<Option<LineStatusRow>> {
    let row = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at \
         FROM line_status WHERE line_id = $1 AND source = 'tfl'",
    )
    .bind(line_id)
    .fetch_optional(pool)
    .await?;

    row.map(row_to_report).transpose()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Requires a local Postgres reachable at `DATABASE_URL` with this crate's
migrations applied. Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api tfl_line_status_by_id -- --ignored --nocapture`
Expected: both new tests (`tfl_line_status_by_id_finds_a_seeded_tfl_row_and_none_for_an_unknown_id`,
`tfl_line_status_by_id_ignores_a_row_with_a_non_tfl_source`) pass.

- [ ] **Step 5: Run the full crate test suite to confirm no regression**

Run: `cargo test -p api`
Expected: all non-`--ignored` tests still pass (the two new tests are
`#[ignore]`d, so they do not run here — that's expected, matching every
other live-database test in this file).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add tfl_line_status_by_id query for the new TfL line status route"
```

---

## Task 2: Backend route — `GET /public/tfl-lines/{lineId}`

**Files:**
- Create: `crates/api/src/routes/tfl_lines.rs`
- Modify: `crates/api/src/routes/mod.rs:7-24` (add `pub mod tfl_lines;`),
  `crates/api/src/routes/mod.rs:48-63` (add `.merge(tfl_lines::router())`
  to `public_router()`)

**Interfaces:**
- Consumes: `queries::tfl_line_status_by_id` (Task 1);
  `render::to_tfl_shape(report: &common::LineStatusReport, computed_at:
  chrono::DateTime<chrono::Utc>, detail: bool) -> serde_json::Value`
  (existing, `crates/api/src/render.rs:14-24`, unmodified).
- Produces: `GET /public/tfl-lines/{lineId}` — 200 with a
  `LineStatusReport`-shaped JSON body (same shape `GET
  /Line/{ids}/Status?detail=true` already returns per element, but here a
  single object, not an array — matching `GET /public/incidents/{incidentId}`'s
  own single-object convention, not the list-route convention) on a known
  TfL line id; 404 with a plain-text body otherwise. Consumed by Task 3's
  `getTflLine`.

- [ ] **Step 1: Write the route module**

```rust
//! `GET /public/tfl-lines/{lineId}` -- a live snapshot of what TfL is
//! currently reporting for one line: current severity/reason/validity per
//! simultaneous status, rendered through the same `render::to_tfl_shape`
//! every other TfL-shaped endpoint in this crate uses. Unauthenticated,
//! matching every other read in `public_router()`.
//!
//! Deliberately NOT a history view: per
//! docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md's Option B
//! recommendation, TfL's feed carries no stable per-disruption identity, so
//! this route (and the frontend page it backs) show only "what TfL is
//! reporting right now for this line," not a `/incidents/[id]`-style
//! timeline. No new table, no migration -- this reads `line_status` exactly
//! as `queries::upsert_tfl_line_status` already writes it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use common::LineStatusReport;
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::queries;
use crate::render::to_tfl_shape;

pub fn router() -> Router {
    Router::new().route("/tfl-lines/{lineId}", axum::routing::get(get_tfl_line))
}

async fn get_tfl_line(
    State(app): State<App>,
    Path(line_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(row) = queries::tfl_line_status_by_id(&app.database, &line_id)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no current TfL status for line: {line_id}"),
        ));
    };

    let computed_at = row.computed_at;
    let report = LineStatusReport {
        id: row.id,
        name: row.name,
        mode_name: row.mode_name,
        operators: row.operators,
        statuses: row.statuses,
    };
    // detail=true unconditionally: this route's entire purpose is a
    // per-status detail view, unlike the list-shaped `/Line/Mode/{mode}/Status`
    // family, where `detail` is caller-controlled to keep the default list
    // payload small.
    Ok(Json(to_tfl_shape(&report, computed_at, true)))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "tfl line status lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}
```

- [ ] **Step 2: Wire the module into `routes/mod.rs`**

In `crates/api/src/routes/mod.rs`, add the module declaration alphabetically
among the existing ones:

```rust
pub mod stanox_crs;
pub mod station_stats;
pub mod tfl_lines;
pub mod train;
```

And add it to `public_router()`'s merge chain (same function used by
`incidents::router()` — this route is a detail-page read, not a
TfL-compatibility-shaped route like `line_status::router()`, so it belongs
here, nested under `/public`, not merged unprefixed in `main.rs`):

```rust
        .merge(station_stats::router())
        .merge(departures::router())
        .merge(stanox_crs::router())
        .merge(tfl_lines::router())
}
```

- [ ] **Step 3: Build and run the crate's existing test suite**

Run: `cargo build -p api && cargo test -p api`
Expected: clean build, no test regressions (this task adds no new
`#[test]`s of its own — the query it depends on is already covered by
Task 1's live-database tests, and the JSON shape it emits is already
covered by `render.rs`'s existing `to_tfl_shape` tests, e.g.
`renders_status_fields`/`disruption_included_with_detail_flag` in
`crates/api/src/render.rs`. This mirrors `routes/incidents.rs`'s own
precedent exactly: it has zero HTTP-layer tests of its own for the same
reason).

- [ ] **Step 4: Manually verify against a running local stack**

```bash
docker compose up -d --build api
# Wait for the api container to report healthy, then:
curl -s http://localhost:8080/public/tfl-lines/tfl-tram | jq .
```

Expected: if `poller-tfl` has already populated a `tfl-tram` row, a JSON
object with `"id": "tfl-tram"`, `"name"`, `"lineStatuses"` (a non-empty
array, each entry carrying `statusSeverity`/`reason`/`validityPeriods`),
and `"computedAt"`. If no such row exists yet, a 404 with a body naming
the line id — either result confirms the route and its 404 branch are
both reachable.

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/public/tfl-lines/definitely-not-a-real-line
```

Expected: `404`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/tfl_lines.rs crates/api/src/routes/mod.rs
git commit -m "Add GET /public/tfl-lines/{lineId}, a live-snapshot TfL line status route"
```

---

## Task 3: Frontend API client — `getTflLine`

**Files:**
- Modify: `frontend/lib/api.ts` (add function near `getIncident`,
  currently at lines 559-563)
- Test: `frontend/lib/api.test.ts` (add tests near the existing
  `getIncident` tests, currently at lines 779-806)

**Interfaces:**
- Consumes: `GET /public/tfl-lines/{lineId}` (Task 2).
- Produces: `export async function getTflLine(lineId: string):
  Promise<LineStatusReport>` — consumed by Task 6's page component.
  `LineStatusReport` is the existing type at `frontend/lib/types.ts:113-128`,
  unmodified.

- [ ] **Step 1: Write the failing tests**

Add `getTflLine` to the import list near the top of
`frontend/lib/api.test.ts` (alongside the existing `getIncident` import),
then add these tests near the existing `getIncident` tests:

```typescript
  it('getTflLine fetches the correct URL with no caching', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ id: 'tfl-victoria' }), { status: 200 })));
    await getTflLine('tfl-victoria');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/tfl-lines/tfl-victoria',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getTflLine throws ApiNotFoundError on a 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getTflLine('does-not-exist')).rejects.toThrow(ApiNotFoundError);
  });

  it('getTflLine still throws on a non-404 failure', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
    await expect(getTflLine('tfl-victoria')).rejects.toThrow(/500/);
  });

  // `lineId` ultimately originates from `Disruption.source`, external feed
  // data (see `tflLineIdFromSource`, Task 4), so it must be percent-encoded
  // before being interpolated into the URL -- same rationale as the
  // existing `getIncident percent-encodes the incident id in the URL` test.
  it('getTflLine percent-encodes the line id in the URL', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ id: 'tfl-victoria' }), { status: 200 })));
    await getTflLine('tfl-victoria/../lines');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/tfl-lines/tfl-victoria%2F..%2Flines',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: FAIL — `getTflLine` is not exported from `./api`.

- [ ] **Step 3: Implement `getTflLine`**

Add to `frontend/lib/api.ts`, immediately after `getIncident`
(currently lines 559-563):

```typescript
export async function getTflLine(lineId: string): Promise<LineStatusReport> {
  return fetchJson<LineStatusReport>(`${baseUrl()}/public/tfl-lines/${encodeURIComponent(lineId)}`, {
    cache: 'no-store',
  });
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: PASS, including the four new `getTflLine` tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add getTflLine API client function"
```

---

## Task 4: Frontend helper — `tflLineIdFromSource`

**Files:**
- Modify: `frontend/lib/incidents.ts`
- Test: `frontend/lib/incidents.test.ts`

**Interfaces:**
- Produces: `export function tflLineIdFromSource(source: string | null |
  undefined): string | null` — consumed by Task 5's `DisruptionDetail.tsx`
  change.

- [ ] **Step 1: Write the failing tests**

Append to `frontend/lib/incidents.test.ts`:

```typescript
describe('tflLineIdFromSource', () => {
  it('strips the known prefix and returns the raw TfL line id', () => {
    expect(tflLineIdFromSource('tfl-line-status-tfl-victoria')).toBe('tfl-victoria');
  });

  it('returns null for null', () => {
    expect(tflLineIdFromSource(null)).toBeNull();
  });

  it('returns null for undefined', () => {
    expect(tflLineIdFromSource(undefined)).toBeNull();
  });

  it('returns null for the shared LDBWS-inferred literal constant', () => {
    expect(tflLineIdFromSource('ldbws-sampling')).toBeNull();
  });

  it('returns null for a Knowledgebase incident source, even though it is a real, unrelated prefix', () => {
    expect(tflLineIdFromSource('knowledgebase-incident-12345')).toBeNull();
  });

  it('returns null for an empty string', () => {
    expect(tflLineIdFromSource('')).toBeNull();
  });
});
```

Add `tflLineIdFromSource` to the existing `import { incidentIdFromSource }
from './incidents';` line at the top of the file.

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- incidents.test.ts`
Expected: FAIL — `tflLineIdFromSource` is not exported from `./incidents`.

- [ ] **Step 3: Implement `tflLineIdFromSource`**

Append to `frontend/lib/incidents.ts`:

```typescript
const TFL_LINE_STATUS_PREFIX = 'tfl-line-status-';

/** Mirrors `incidentIdFromSource` above, for TfL disruptions instead of
 * Knowledgebase ones. Extracts the TfL line id (already `tfl-`-prefixed,
 * e.g. `tfl-victoria` -- the exact string `line_status.line_id` stores for
 * that row, see `crates/poller-tfl/src/schema.rs`'s `to_report`) from a
 * `tfl-line-status-{lineId}` source string, for linking to
 * `/tfl-lines/{lineId}`. `null` for anything else, including the LDBWS
 * `ldbws-sampling` constant and Knowledgebase sources (handled by
 * `incidentIdFromSource` instead) -- this function's contract is
 * deliberately independent of that one's; changing one must never change
 * the other's behavior. See
 * docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md Section 5. */
export function tflLineIdFromSource(source: string | null | undefined): string | null {
  if (!source || !source.startsWith(TFL_LINE_STATUS_PREFIX)) return null;
  return source.slice(TFL_LINE_STATUS_PREFIX.length);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- incidents.test.ts`
Expected: PASS, all 12 tests (6 existing `incidentIdFromSource` +
6 new `tflLineIdFromSource`).

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/incidents.ts frontend/lib/incidents.test.ts
git commit -m "Add tflLineIdFromSource, the TfL-disruption analog of incidentIdFromSource"
```

---

## Task 5: Wire the link into `DisruptionDetail.tsx`

**Files:**
- Modify: `frontend/components/DisruptionDetail.tsx`
- Test: `frontend/components/DisruptionDetail.test.tsx`

**Interfaces:**
- Consumes: `tflLineIdFromSource` (Task 4).

- [ ] **Step 1: Write the failing tests**

Append to `frontend/components/DisruptionDetail.test.tsx`:

```typescript
  it('renders a link to the TfL line status page when source names a TfL line', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, source: 'tfl-line-status-tfl-victoria' }} />,
    );
    const link = screen.getByRole('link', { name: 'View live TfL line status' });
    expect(link).toHaveAttribute('href', '/tfl-lines/tfl-victoria');
  });

  it('renders no TfL-line-status link when source names a Knowledgebase incident', () => {
    renderWithMantine(<DisruptionDetail disruption={sample} />);
    expect(screen.queryByRole('link', { name: 'View live TfL line status' })).not.toBeInTheDocument();
  });

  it('renders no TfL-line-status link when source is the LDBWS-inferred literal', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: 'ldbws-sampling' }} />);
    expect(screen.queryByRole('link', { name: 'View live TfL line status' })).not.toBeInTheDocument();
  });

  it('renders no TfL-line-status link when source is null', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: null }} />);
    expect(screen.queryByRole('link', { name: 'View live TfL line status' })).not.toBeInTheDocument();
  });

  // Same path-like-value rationale as the existing
  // 'percent-encodes a path-like incident id in the link href' test.
  it('percent-encodes a path-like TfL line id in the link href', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, source: 'tfl-line-status-tfl-victoria/extra' }} />,
    );
    const link = screen.getByRole('link', { name: 'View live TfL line status' });
    expect(link).toHaveAttribute('href', '/tfl-lines/tfl-victoria%2Fextra');
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- DisruptionDetail.test.tsx`
Expected: FAIL — no element with the role/name `'View live TfL line
status'` is rendered yet.

- [ ] **Step 3: Wire the link into the component**

In `frontend/components/DisruptionDetail.tsx`, add the import:

```typescript
import { incidentIdFromSource, tflLineIdFromSource } from '@/lib/incidents';
```

Compute the new value alongside the existing `incidentId` (near the top
of the component body):

```typescript
  const incidentId = incidentIdFromSource(disruption.source);
  const tflLineId = tflLineIdFromSource(disruption.source);
```

Render the new link immediately after the existing `{incidentId && (...)}`
block (the two are mutually exclusive per each helper's own prefix check,
so both blocks can render unconditionally side by side with no `else`):

```tsx
      {incidentId && (
        <TextLink href={`/incidents/${encodeURIComponent(incidentId)}`} underline="always">
          View full incident details
        </TextLink>
      )}
      {tflLineId && (
        <TextLink href={`/tfl-lines/${encodeURIComponent(tflLineId)}`} underline="always">
          View live TfL line status
        </TextLink>
      )}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- DisruptionDetail.test.tsx`
Expected: PASS, all tests including the 5 new ones and every pre-existing
one (in particular the two already-present tests that assert TfL/LDBWS
sources render **no incident-detail link** must still pass unchanged —
this task adds a new link, it does not touch that logic).

- [ ] **Step 5: Commit**

```bash
git add frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx
git commit -m "Link TfL disruptions to their new live-snapshot status page"
```

---

## Task 6: Frontend page — `frontend/app/tfl-lines/[id]/page.tsx`

**Files:**
- Create: `frontend/app/tfl-lines/[id]/page.tsx`
- Create: `frontend/app/tfl-lines/[id]/not-found.tsx`

**Interfaces:**
- Consumes: `getTflLine` (Task 3); `ApiNotFoundError` (existing,
  `frontend/lib/api.ts:31`); `LineStatusReport`/`LineStatus`/`ValidityPeriod`
  (existing, `frontend/lib/types.ts`); `StatusBadge` (existing,
  `frontend/components/StatusBadge.tsx`); `ShareButton`, `formatDateTime`
  (existing, used identically by `frontend/app/incidents/[id]/page.tsx`).

- [ ] **Step 1: Write the page component**

```tsx
import { notFound } from 'next/navigation';
import { Badge, Divider, Group, Stack, Text, Title } from '@mantine/core';
import { ApiNotFoundError, getTflLine } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { ShareButton } from '@/components/ShareButton';
import { formatDateTime } from '@/lib/dateFormat';
import type { ValidityPeriod } from '@/lib/types';

// Same rationale as every dynamic `[param]` route in this app (see
// `frontend/app/incidents/[id]/page.tsx`'s identical comment): without
// this, `next build` may try to prerender against a database that only
// exists on the compose network at runtime.
export const revalidate = 0;

function formatValidityPeriod(period: ValidityPeriod): string {
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}

export default async function TflLineStatusPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let report;
  try {
    report = await getTflLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Group gap="sm">
          <Title order={1}>{report.name}</Title>
          <Badge variant="outline" color="gray">
            {report.modeName}
          </Badge>
        </Group>
        <ShareButton />
      </Group>

      {/* Sets expectations up front: this is Option B from
          docs/superpowers/specs/2026-09-07-tfl-incident-page-design.md --
          a live snapshot, not a tracked, bookmarkable incident with a
          history a user could expect to still resolve later. */}
      <Text c="dimmed" size="sm">
        This is a live snapshot of what TfL is currently reporting for this line — not a
        tracked incident with a history. Reopening this page later shows whatever TfL is
        reporting at that time, which may be a different disruption or no disruption at all.
      </Text>

      <Stack gap="lg">
        {report.lineStatuses.map((status, i) => (
          <Stack key={i} gap={4}>
            <StatusBadge severity={status.statusSeverity} />
            {/* Raw compound TfL text rendered verbatim, as plain text --
                TfL sends no HTML in `reason`, unlike Knowledgebase's
                `description`, so no sanitizeDescription call is needed
                here. A single string covering several differently-affected
                segments at different severities (Section 3 of the design
                doc) renders here exactly as TfL sent it, unstyled, under
                this one severity badge -- a deliberate, honestly-incomplete
                v1 scope line, not a bug. */}
            <Text size="sm">{status.reason}</Text>
            {status.validityPeriods.map((period, j) => (
              <Text key={j} size="xs" c="dimmed">
                {formatValidityPeriod(period)}
              </Text>
            ))}
          </Stack>
        ))}
      </Stack>

      <Divider />

      <Text size="xs" c="dimmed">
        Last updated: {formatDateTime(report.computedAt)}
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 2: Write the not-found page**

```tsx
import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function TflLineNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1} size="h2">
        No current TfL status for this line
      </Title>
      <Text c="dimmed">
        TfL is not currently reporting this line, or the id is unrecognized. TfL's own feed
        carries no history beyond what this app has already polled — see
        `line_status_history` via that line&apos;s own history page for what was last
        recorded, if anything.
      </Text>
      <Group gap="lg">
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
```

- [ ] **Step 3: Manually verify against a running local stack**

```bash
cd frontend
npm run build
```

Expected: the build succeeds with no TypeScript errors (in particular, no
"variable used before its type is narrowed" errors from the `try`/`catch`
around `getTflLine`, mirroring `IncidentDetailPage`'s identical pattern).

- [ ] **Step 4: Commit**

```bash
git add frontend/app/tfl-lines/[id]/page.tsx frontend/app/tfl-lines/[id]/not-found.tsx
git commit -m "Add the TfL line live-snapshot status page"
```

---

## Task 7: Frontend page tests

**Files:**
- Create: `frontend/app/tfl-lines/[id]/page.test.tsx`

**Interfaces:**
- Consumes: `TflLineStatusPage` (Task 6); `getTflLine` (Task 3, mocked).

- [ ] **Step 1: Write the failing tests**

```typescript
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TflLineStatusPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import { notFound } from 'next/navigation';
import { formatDateTime } from '@/lib/dateFormat';
import type { LineStatusReport } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getTflLine: vi.fn() };
});
// Same rationale as `frontend/app/incidents/[id]/page.test.tsx`'s
// identical mock: this page renders no client component that needs
// useRouter, and notFound() is a plain no-op here -- the page's own
// unconditional `throw err;` after calling it is what makes the promise
// actually reject in this mocked environment.
vi.mock('next/navigation', () => ({ notFound: vi.fn() }));

function report(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    id: 'tfl-victoria',
    name: 'Victoria',
    modeName: 'tube',
    operators: ['TfL'],
    computedAt: '2026-09-07T09:00:00Z',
    lineStatuses: [
      {
        statusSeverity: 6,
        statusSeverityDescription: 'Severe Delays',
        reason: 'Severe delays due to a signal failure at Oxford Circus',
        dataQuality: 'tfl',
        validityPeriods: [{ fromDate: '2026-09-07T08:00:00Z', toDate: null, isNow: true }],
        sampleAvailability: { state: 'no-coverage' },
        fullCoverageAvailability: { state: 'not-enabled' },
      },
    ],
    ...overrides,
  };
}

describe('TflLineStatusPage', () => {
  it('calls notFound() when getTflLine throws ApiNotFoundError', async () => {
    vi.mocked(api.getTflLine).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(TflLineStatusPage({ params: Promise.resolve({ id: 'does-not-exist' }) })).rejects.toThrow();
    expect(vi.mocked(notFound)).toHaveBeenCalled();
  });

  it('renders the line name and mode', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(report());
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.getByText('Victoria')).toBeInTheDocument();
    expect(screen.getByText('tube')).toBeInTheDocument();
  });

  it('renders the severity badge and the raw reason text verbatim', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(report());
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
    expect(screen.getByText('Severe delays due to a signal failure at Oxford Circus')).toBeInTheDocument();
  });

  // The concrete regression test for Section 3's "raw text as-is" v1
  // decision: a real, production-shaped compound multi-segment TfL string
  // (from the design doc's own worked example) must render completely
  // unstyled and unsplit, under the one severity badge for the whole entry.
  it('renders a compound multi-segment status string completely verbatim, with no per-segment styling', async () => {
    const compound =
      'Severe delays between Heathrow Terminals 2&3 and Heathrow Terminal 5 due to an earlier ' +
      'signal failure. MINOR DELAYS between Hayes & Harlington and Reading. MINOR DELAYS between ' +
      'Shenfield and Whitechapel. GOOD SERVICE on the rest of the line.';
    vi.mocked(api.getTflLine).mockResolvedValue(
      report({
        lineStatuses: [
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: compound,
            dataQuality: 'tfl',
            validityPeriods: [{ fromDate: '2026-09-07T08:00:00Z', toDate: null, isNow: true }],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
          },
        ],
      }),
    );
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-elizabeth' }) }));
    expect(screen.getByText(compound)).toBeInTheDocument();
    // Only one severity badge for the whole compound entry -- not one per
    // segment (segment-level severity parsing is explicitly deferred).
    expect(screen.getAllByText('Severe Delays')).toHaveLength(1);
  });

  it('renders every simultaneous status when a line reports more than one at once', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(
      report({
        lineStatuses: [
          {
            statusSeverity: 4,
            statusSeverityDescription: 'Planned Closure',
            reason: 'No service between Seven Sisters and Walthamstow Central',
            dataQuality: 'tfl',
            validityPeriods: [{ fromDate: '2026-09-07T00:00:00Z', toDate: '2026-09-08T00:00:00Z', isNow: true }],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
          },
          {
            statusSeverity: 6,
            statusSeverityDescription: 'Severe Delays',
            reason: 'Signal failure at Oxford Circus',
            dataQuality: 'tfl',
            validityPeriods: [{ fromDate: '2026-09-07T02:30:00Z', toDate: null, isNow: true }],
            sampleAvailability: { state: 'no-coverage' },
            fullCoverageAvailability: { state: 'not-enabled' },
          },
        ],
      }),
    );
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.getByText('No service between Seven Sisters and Walthamstow Central')).toBeInTheDocument();
    expect(screen.getByText('Signal failure at Oxford Circus')).toBeInTheDocument();
    expect(screen.getByText('Planned Closure')).toBeInTheDocument();
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('renders the validity period', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(report());
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.getByText(`${formatDateTime('2026-09-07T08:00:00Z')} – ongoing`)).toBeInTheDocument();
  });

  it('renders the last-updated timestamp', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(report());
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.getByText(`Last updated: ${formatDateTime('2026-09-07T09:00:00Z')}`)).toBeInTheDocument();
  });

  it('renders no "currently affects" or history section', async () => {
    vi.mocked(api.getTflLine).mockResolvedValue(report());
    renderWithMantine(await TflLineStatusPage({ params: Promise.resolve({ id: 'tfl-victoria' }) }));
    expect(screen.queryByText(/Currently affects/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/^History$/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/First seen/i)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- page.test.tsx --dir app/tfl-lines`
(or, if that path filter doesn't resolve on this Vitest config, `npm test
-- frontend/app/tfl-lines/\[id\]/page.test.tsx`)
Expected: FAIL — `./page` does not yet default-export a component with
this behavior (it doesn't exist as a module resolvable from this path
until Task 6's `page.tsx` is present; if Task 6 already landed, this
instead fails on the specific assertions not yet matching, e.g. no
"Last updated" text).

- [ ] **Step 3: Confirm the implementation from Task 6 makes them pass**

Run (from `frontend/`): `npm test -- page.test.tsx --dir app/tfl-lines`
Expected: PASS, all 8 tests. If any fail, adjust `page.tsx` from Task 6
(not this test file) — in particular, double-check Mantine's `Badge`
rendering puts the mode-name text (`report.modeName`, e.g. `"tube"`) in a
`getByText`-findable node, and that `StatusBadge`'s label
(`frontend/lib/severity.ts`'s `severityLabel`) renders the exact strings
`'Severe Delays'`/`'Planned Closure'` this test expects.

- [ ] **Step 4: Run the full frontend test suite to confirm no regression**

Run (from `frontend/`): `npm test`
Expected: PASS, no regressions in any other suite.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/tfl-lines/[id]/page.test.tsx
git commit -m "Add tests for the TfL line live-snapshot status page"
```

---

## Self-review notes (performed on this plan before handoff)

- **Placeholder scan:** no "TBD"/"TODO"/"handle appropriately" found; every
  step above contains literal code, not a description of code.
- **Type/signature consistency across tasks:** `tfl_line_status_by_id`
  (Task 1) returns `Result<Option<LineStatusRow>>`, matching exactly what
  Task 2's handler destructures with `let Some(row) = ... else`.
  `getTflLine` (Task 3) returns `Promise<LineStatusReport>`, matching the
  type Task 6's `page.tsx` treats `report` as (no explicit annotation
  needed — inferred). `tflLineIdFromSource` (Task 4) returns `string |
  null`, matching the `{tflLineId && (...)}` conditional Task 5 adds.
  Route path segment name is `{lineId}` consistently between Task 2's
  Rust route registration and this document's own prose; the Express-style
  `{lineId}` axum path-param name is never referenced in code (axum
  extracts positionally into `Path<String>`), so no naming drift is
  possible there.
- **Spec coverage check**, against the design doc's Sections 2, 3, 5, 6, 7:
  - Section 2 (Option B, no synthetic identity, reuse `line_status`/
    `line_status_history` with zero migration): satisfied — no migration
    task exists anywhere in this plan; Task 1's query reads the existing
    table only.
  - Section 3 (raw compound text verbatim, no segment parsing): satisfied
    by Task 6's plain-text `{status.reason}` render and Task 7's explicit
    compound-string regression test.
  - Section 5's concrete page-content bullets: severity badge(s) per
    simultaneous status (Task 6), raw reason text with no HTML
    sanitization (Task 6), validity period(s) (Task 6), no "currently
    affects" section (Task 6 omits it; Task 7 asserts its absence), no
    "first seen" field (same). The one bullet **not** carried over is the
    optional history section — deliberately, per this plan's own
    "Decisions this plan resolves" section above (v1 ships current-status-only).
  - Section 5's two sub-bullets on linking: `tflLineIdFromSource` (Task 4)
    and its wiring into `DisruptionDetail.tsx` (Task 5) directly implement
    the design doc's own suggested mechanism, without altering
    `incidentIdFromSource`'s contract (Global Constraints, and Task 5's
    Step 4 explicitly re-asserts the pre-existing TfL/LDBWS-return-null
    tests still pass).
  - Section 6 non-goals (Option A, segment parsing, Naptan/CRS
    reconciliation, a browse-all index, retroactive backfill, changing
    `/lines/[id]`'s existing rendering, DLR pilot integration): none of
    these appear as tasks, matching the design doc's explicit scoping.
- **Global Constraints self-check:** no migration file is created or
  modified by any task; every new Rust JSON response goes through the
  existing hand-built `render::to_tfl_shape`, not a new derived struct;
  the two live-DB tests (Task 1) follow the exact `#[tokio::test]
  #[ignore]` + manual `test_pool()` pattern already used throughout
  `queries.rs`, not `#[sqlx::test]`.
