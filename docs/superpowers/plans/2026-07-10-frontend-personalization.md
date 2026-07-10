# Frontend Personalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** National Rail lines become opt-in (home page only shows pinned lines/stations), a `/lines` browse page lists every line and lets users pin/unpin and create custom lines, and station pages gain the same pin control.

**Architecture:** Two new small Postgres tables (`pinned_lines`, `pinned_stations`) back three new unauthenticated `api` endpoints (`GET /public/preferences`, `PUT /public/preferences/pinned-lines`, `PUT /public/preferences/pinned-stations`), consumed by Next.js Server Components for initial render. Since Client Components cannot read the server-only `API_BASE_URL` env var, a single catch-all Next.js Route Handler (`app/api/[...path]/route.ts`) proxies same-origin `/api/*` browser requests to `${API_BASE_URL}/public/*` server-side — this is what pin toggles and the custom-line form actually call, so no CORS relaxation is needed on the `api` service.

**Tech Stack:** Rust (axum, sqlx, Postgres) for the two new endpoints; Next.js App Router (Server + Client Components), Mantine, TypeScript for the frontend.

## Global Constraints

- No auth/ownership — unauthenticated, single shared preference list, matching `custom_lines`' model (spec: Non-goals).
- `PUT` endpoints replace the whole list (no per-item add/remove endpoints) — a toggle reads current state, computes the new array, PUTs it back.
- `GET /public/preferences` silently drops any pinned line/station id that no longer resolves to a real line (catalogue or custom) or station — filtering happens on read, not on write.
- Deleting a custom line (`DELETE /public/lines/{id}`, already implemented) must cascade-delete its `pinned_lines` row in the same transaction.
- No new frontend abstraction for "search stations" — the existing `/stations` lookup pattern (type a 3-letter CRS code) is reused as-is for both the custom-line station picker and pinning; there's no live station-search/autocomplete endpoint to build against.
- The design doc says "multi-select from known ATOC codes" for the custom-line form's operators field — there's no backend endpoint listing known operator codes (out of scope to add one for this plan), so this plan uses a free-text `TagsInput` instead, consistent with how the same design doc already treats the headcode-prefix/destination-CRS filters as "narrow power-user knobs, not worth bespoke UI."

---

### Task 1: Migration — `pinned_lines` and `pinned_stations` tables

**Files:**
- Create: `crates/api/migrations/20260710090000_preferences.sql`

**Interfaces:**
- Produces: tables `pinned_lines(line_id TEXT PK, pinned_at TIMESTAMPTZ)`, `pinned_stations(crs CHAR(3) PK, pinned_at TIMESTAMPTZ)`, read/written by Tasks 2–4.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- User preferences: which lines and stations are pinned to the home page.
-- No FK to custom_lines/stations: official line ids are compile-time TOML,
-- not DB rows, so no single constraint can cover both catalogue and
-- custom lines. The API filters out stale ids on read instead (see
-- `crates/api/src/routes/preferences.rs`). No owner column — unauthenticated
-- for now, same rationale as `custom_lines` (see
-- docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md's
-- Non-goals).
-- -------------------------------------------------------------------------

CREATE TABLE pinned_lines (
    line_id    TEXT        PRIMARY KEY,
    pinned_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE pinned_stations (
    crs        CHAR(3)     PRIMARY KEY,
    pinned_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 2: Apply and verify the migration**

The `api` service applies migrations automatically at startup. From the repo root:

```bash
docker compose build --no-cache api
docker compose up -d --no-build api
docker compose logs --tail 20 api
```

Expected: no migration errors, container reaches `healthy`. Then:

```bash
docker compose exec postgres psql -U postgres -d nr_status -c '\d pinned_lines'
docker compose exec postgres psql -U postgres -d nr_status -c '\d pinned_stations'
```

(Check `DATABASE_URL` in `.env` first if the db/user names differ.) Expected: column lists matching Step 1.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260710090000_preferences.sql
git commit -m "Add pinned_lines and pinned_stations table migration"
```

---

### Task 2: `api` — preferences queries

**Files:**
- Create: `crates/api/src/data/preferences.rs`
- Modify: `crates/api/src/data/mod.rs`

**Interfaces:**
- Produces:
  - `pub async fn list_pinned_line_ids(pool: &PgPool) -> Result<Vec<String>>`
  - `pub async fn list_pinned_station_crs(pool: &PgPool) -> Result<Vec<String>>`
  - `pub async fn replace_pinned_lines(pool: &PgPool, ids: &[String]) -> Result<()>`
  - `pub async fn replace_pinned_stations(pool: &PgPool, crs_codes: &[String]) -> Result<()>`
  - `pub async fn filter_existing_station_crs(pool: &PgPool, candidates: &[String]) -> Result<Vec<String>>`
  - All consumed by Task 3's route handlers.

This task has no unit test — consistent with every other `sqlx`-query module in this codebase (`custom_lines.rs`, `queries.rs`), which don't test DB-touching functions directly (confirmed repeatedly during the previous plan's task reviews). Verification is `cargo build` plus Task 3's live `curl` check.

- [ ] **Step 1: Implement**

Create `crates/api/src/data/preferences.rs`:

```rust
//! Queries for user preferences: pinned lines and pinned stations. See
//! `docs/superpowers/specs/2026-07-09-frontend-personalization-design.md`.

use anyhow::Result;
use sqlx::{PgPool, Row};

pub async fn list_pinned_line_ids(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT line_id FROM pinned_lines ORDER BY pinned_at")
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("line_id")?)).collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT crs FROM pinned_stations ORDER BY pinned_at")
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("crs")?)).collect()
}

/// Filters `candidates` down to only those that exist in `stations` —
/// used to drop stale pinned-station ids on read.
pub async fn filter_existing_station_crs(pool: &PgPool, candidates: &[String]) -> Result<Vec<String>> {
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    let rows = sqlx::query("SELECT crs FROM stations WHERE crs = ANY($1)")
        .bind(candidates)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("crs")?)).collect()
}

/// Replaces the entire pinned-lines set with `ids`, in one transaction
/// (delete-all then insert-all) so a PUT is atomic — concurrent readers
/// never see a partially-updated list.
pub async fn replace_pinned_lines(pool: &PgPool, ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines").execute(&mut *tx).await?;
    for id in ids {
        sqlx::query("INSERT INTO pinned_lines (line_id, pinned_at) VALUES ($1, NOW())")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Same replace-whole-set semantics as `replace_pinned_lines`, for stations.
pub async fn replace_pinned_stations(pool: &PgPool, crs_codes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations").execute(&mut *tx).await?;
    for crs in crs_codes {
        sqlx::query("INSERT INTO pinned_stations (crs, pinned_at) VALUES ($1, NOW())")
            .bind(crs)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
```

In `crates/api/src/data/mod.rs`, add the new module (alphabetical, matching the existing convention):

```rust
pub mod config;
pub mod custom_lines;
pub mod preferences;
pub mod queries;
pub mod samples;

pub use common::{LineDefinition, Station};
```

- [ ] **Step 2: Build**

Run: `cargo build -p api`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add crates/api/src/data/preferences.rs crates/api/src/data/mod.rs
git commit -m "Add preferences CRUD queries to the api crate"
```

---

### Task 3: `api` — `GET/PUT /public/preferences` routes

**Files:**
- Create: `crates/api/src/routes/preferences.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Consumes: `crate::data::preferences::*` (Task 2), `crate::data::custom_lines::list_custom_lines` (existing).
- Produces: `pub fn router() -> Router` merged into `public_router()`.

No automated test — same HTTP-route convention gap as `routes/lines.rs`. Verified with `curl` against the running stack.

- [ ] **Step 1: Implement**

Create `crates/api/src/routes/preferences.rs`:

```rust
//! `/public/preferences`: which lines/stations are pinned to the home
//! page. Unauthenticated, same rationale as `/public/lines` — see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::{custom_lines, preferences};

pub fn router() -> Router {
    Router::new()
        .route("/preferences", axum::routing::get(get_preferences))
        .route("/preferences/pinned-lines", axum::routing::put(put_pinned_lines))
        .route("/preferences/pinned-stations", axum::routing::put(put_pinned_stations))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesResponse {
    pinned_lines: Vec<String>,
    pinned_stations: Vec<String>,
}

async fn get_preferences(
    State(app): State<App>,
) -> Result<Json<PreferencesResponse>, (StatusCode, String)> {
    let pinned_line_ids = preferences::list_pinned_line_ids(&app.database)
        .await
        .map_err(internal_error)?;
    let custom = custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let known_line_ids: HashSet<String> = app
        .config
        .lines
        .iter()
        .map(|l| l.id.clone())
        .chain(custom.into_iter().map(|c| c.id))
        .collect();
    let pinned_lines: Vec<String> = pinned_line_ids
        .into_iter()
        .filter(|id| known_line_ids.contains(id))
        .collect();

    let pinned_station_candidates = preferences::list_pinned_station_crs(&app.database)
        .await
        .map_err(internal_error)?;
    let pinned_stations = preferences::filter_existing_station_crs(&app.database, &pinned_station_candidates)
        .await
        .map_err(internal_error)?;

    Ok(Json(PreferencesResponse { pinned_lines, pinned_stations }))
}

async fn put_pinned_lines(
    State(app): State<App>,
    Json(ids): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_lines(&app.database, &ids)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_pinned_stations(
    State(app): State<App>,
    Json(crs_codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_stations(&app.database, &crs_codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "preferences operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "operation failed".to_string())
}
```

In `crates/api/src/routes/mod.rs`, add the module and merge it into `public_router()`:

```rust
pub mod health;
pub mod ingest;
pub mod line_status;
pub mod lines;
pub mod preferences;
pub mod samples;
```

```rust
pub fn public_router() -> Router {
    // [existing doc comment — do not remove]
    Router::new()
        .merge(health::router())
        .merge(lines::router())
        .merge(preferences::router())
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p api`
Expected: builds clean.

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache api
docker compose up -d --no-build api
```

```bash
curl -s http://localhost:8080/public/preferences | python3 -m json.tool
```
Expected: `{"pinnedLines":[],"pinnedStations":[]}`.

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:8080/public/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '["wcml"]'
curl -s http://localhost:8080/public/preferences | python3 -m json.tool
```
Expected: second call returns `{"pinnedLines":["wcml"],"pinnedStations":[]}`.

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:8080/public/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '["wcml","not-a-real-line"]'
curl -s http://localhost:8080/public/preferences | python3 -m json.tool
```
Expected: `pinnedLines` returns only `["wcml"]` — `not-a-real-line` is silently dropped, confirming the filter-on-read behavior.

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:8080/public/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '[]'
```
Clean up (empties the list again).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/preferences.rs crates/api/src/routes/mod.rs
git commit -m "Add GET/PUT /public/preferences routes"
```

---

### Task 4: `api` — cascade-delete `pinned_lines` when a custom line is deleted

**Files:**
- Modify: `crates/api/src/data/custom_lines.rs`

**Interfaces:**
- Changes `delete_custom_line`'s implementation only; signature (`pub async fn delete_custom_line(pool: &PgPool, id: &str) -> Result<bool>`) is unchanged, so `crates/api/src/routes/lines.rs`'s `delete_line` handler needs no changes.

This must run after Task 1 (needs the `pinned_lines` table to exist).

- [ ] **Step 1: Implement**

In `crates/api/src/data/custom_lines.rs`, replace the existing `delete_custom_line`:

```rust
/// Deletes a custom line by id, and any `pinned_lines` row referencing it,
/// in one transaction — without this, unpinning would be impossible for a
/// line that no longer exists, and the stale pin would sit forever (no FK
/// exists to catch it, since `pinned_lines` intentionally has none — see
/// the preferences migration). Returns `true` if a custom line was
/// deleted, `false` if no custom line had that id (a no-op either way for
/// `pinned_lines`, since a non-custom-line id was never insertable there
/// through normal use, but the DELETE is harmless if it somehow was).
pub async fn delete_custom_line(pool: &PgPool, id: &str) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query("DELETE FROM custom_lines WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let deleted = result.rows_affected() > 0;
    if deleted {
        sqlx::query("DELETE FROM pinned_lines WHERE line_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(deleted)
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p api && cargo test -p api`
Expected: builds clean, all existing tests still pass (this function has no direct unit test, per the established convention — the 3 `slugify` tests and the rest of the crate's suite are unaffected).

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache api
docker compose up -d --no-build api

curl -s -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"Cascade Test","operators":["SW"],"stations":["WOK","AON"]}'
# -> {"id":"custom-cascade-test", ...}

curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:8080/public/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '["custom-cascade-test"]'
curl -s http://localhost:8080/public/preferences | python3 -m json.tool
# -> pinnedLines: ["custom-cascade-test"]

curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-cascade-test
curl -s http://localhost:8080/public/preferences | python3 -m json.tool
# -> pinnedLines: [] — confirms the cascade worked without even needing
# the read-time filter in Task 3 to hide it
```

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/custom_lines.rs
git commit -m "Cascade-delete pinned_lines row when a custom line is deleted"
```

---

### Task 5: `api` — accept camelCase JSON in `POST /public/lines`

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:** No signature changes — this only changes which JSON keys `CreateLineRequest` accepts.

`CreateLineRequest` (added when `/public/lines` was first built) has no `#[serde(rename_all = "camelCase")]`, so it currently expects snake_case `headcode_prefixes`/`destination_crs_filter` in the request body — inconsistent with every other JSON key this API produces or accepts elsewhere (`statusSeverity`, `avgDelayMinutes`, `pinnedLines`, etc.), and never actually exercised by a real client until this plan's Task 9 (the custom-line creation form) becomes the first one. Fixing it now, before a frontend caches in the snake_case workaround, is cheaper than fixing it later.

- [ ] **Step 1: Implement**

In `crates/api/src/routes/lines.rs`, add the attribute to `CreateLineRequest`:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateLineRequest {
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    #[serde(default)]
    headcode_prefixes: Vec<String>,
    #[serde(default)]
    destination_crs_filter: Vec<String>,
}
```

- [ ] **Step 2: Build**

Run: `cargo build -p api`
Expected: builds clean.

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache api
docker compose up -d --no-build api

curl -s -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"Camel Test","operators":["SW"],"stations":["WOK","AON"],"headcodePrefixes":["1P"],"destinationCrsFilter":["AON"]}'
```
Expected: `HTTP 200` with a created `custom-camel-test` line (not a 400/422 deserialization error).

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-camel-test
```
Clean up.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "Accept camelCase field names in POST /public/lines"
```

---

### Task 6: frontend — same-origin proxy for browser-initiated API writes

**Files:**
- Create: `frontend/app/api/[...path]/route.ts`

**Interfaces:**
- Produces: `/api/*` (any method, any path) proxies to `${API_BASE_URL}/public/*` server-side. Consumed by Task 8 (`PinToggle`) and Task 9 (`CustomLineForm`).

**Why this exists:** `lib/api.ts`'s `baseUrl()` reads `process.env.API_BASE_URL`, a server-only env var Next.js does not inline into the browser bundle (only `NEXT_PUBLIC_`-prefixed vars are). Client Components (the pin toggle, the custom-line form) run in the browser and therefore cannot read it, so they cannot call the `api` service directly. This route handler runs server-side and proxies same-origin browser requests through to the real service, where the env var is available — avoiding any need to expose the `api` service's port to the browser or relax its CORS policy for POST/PUT/DELETE.

No automated test for this file — same HTTP-route-level convention gap as the backend's `routes/lines.rs`/`routes/preferences.rs` (no route-handler test harness exists in this codebase). Verified with `curl` against the running Next.js dev/prod server in Step 2.

- [ ] **Step 1: Implement**

Create `frontend/app/api/[...path]/route.ts`:

```typescript
import { NextRequest, NextResponse } from 'next/server';

// Client Components can't read `API_BASE_URL` (server-only env var, not
// inlined into the browser bundle unless prefixed `NEXT_PUBLIC_`), so
// browser-initiated mutations (pinning, creating a custom line) can't call
// the `api` service directly. This catch-all proxies same-origin `/api/*`
// requests from the browser to `${API_BASE_URL}/public/*` server-side —
// since the browser only ever talks to this Next.js origin, no CORS
// relaxation on the `api` service is needed for these write endpoints.
async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  const url = `${process.env.API_BASE_URL}/public/${path.join('/')}${req.nextUrl.search}`;
  const init: RequestInit = { method: req.method, headers: { 'Content-Type': 'application/json' } };
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    init.body = await req.text();
  }
  const response = await fetch(url, init);
  const body = await response.text();
  return new NextResponse(body, {
    status: response.status,
    headers: { 'Content-Type': response.headers.get('Content-Type') ?? 'application/json' },
  });
}

export async function GET(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function POST(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function PUT(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}

export async function DELETE(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return proxy(req, (await params).path);
}
```

- [ ] **Step 2: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

```bash
curl -s http://localhost:3000/api/preferences | python3 -m json.tool
```
Expected: same shape as `curl http://localhost:8080/public/preferences` directly (`{"pinnedLines":[...],"pinnedStations":[...]}`) — confirms the proxy correctly forwards to the `api` service and returns its response.

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:3000/api/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '["wcml"]'
curl -s http://localhost:3000/api/preferences | python3 -m json.tool
```
Expected: `pinnedLines: ["wcml"]` — confirms PUT with a body proxies correctly too.

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X PUT http://localhost:3000/api/preferences/pinned-lines \
  -H "Content-Type: application/json" -d '[]'
```
Clean up.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/api/[...path]/route.ts
git commit -m "Add same-origin API proxy for browser-initiated preference/line writes"
```

---

### Task 7: frontend — `Preferences`/`LineSummary` types and server-side read functions

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `Preferences`, `LineSummary` types; `getPreferences(): Promise<Preferences>`, `getAllLines(): Promise<LineSummary[]>`. Consumed by Task 9's `/lines` page and Task 10's home page and Task 11's station page (all Server Components, using the existing `API_BASE_URL`-based fetch pattern — these are reads, unlike Task 6's proxy which is for browser-initiated writes).

- [ ] **Step 1: Write the failing tests**

Add to `frontend/lib/api.test.ts`, inside the existing `describe('api client', ...)` block (after the `getLineStatusHistory` test, before the error-handling tests):

```typescript
  it('getPreferences fetches the correct URL with no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ pinnedLines: ['wcml'], pinnedStations: ['WOK'] }), { status: 200 })),
    );
    await getPreferences();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/preferences',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getAllLines fetches the correct URL with no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getAllLines();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
```

Add `getPreferences` and `getAllLines` to the existing import at the top of the file:

```typescript
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
  getPreferences,
  getAllLines,
  ApiNotFoundError,
} from './api';
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run lib/api.test.ts`
Expected: FAIL — `getPreferences`/`getAllLines` don't exist yet in `./api`.

- [ ] **Step 3: Implement**

In `frontend/lib/types.ts`, add (anywhere, e.g. after `LineStatusHistoryEntry`):

```typescript
export interface Preferences {
  pinnedLines: string[];
  pinnedStations: string[];
}

export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom';
}
```

In `frontend/lib/api.ts`, add the import and the two functions:

```typescript
import type { LineStatusReport, LineStatusHistoryEntry, Preferences, LineSummary } from './types';
```

```typescript
export async function getPreferences(): Promise<Preferences> {
  return fetchJson<Preferences>(`${baseUrl()}/public/preferences`, { cache: 'no-store' });
}

export async function getAllLines(): Promise<LineSummary[]> {
  return fetchJson<LineSummary[]>(`${baseUrl()}/public/lines`, { cache: 'no-store' });
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run lib/api.test.ts`
Expected: PASS — all tests in the file, including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add Preferences/LineSummary types and getPreferences/getAllLines"
```

---

### Task 8: frontend — `PinToggle` component

**Files:**
- Create: `frontend/components/PinToggle.tsx`
- Create: `frontend/components/PinToggle.test.tsx`

**Interfaces:**
- Produces: `PinToggle({ kind: 'line' | 'station', id: string, initiallyPinned: boolean })` — a Client Component. Consumed by Task 9 (`/lines` page rows) and Task 11 (station page header).

This component fetches the *current full* pinned list before PUTting an update (since PUT replaces the whole array, toggling one id requires knowing all the others) — it calls Task 6's `/api/*` proxy directly via relative `fetch()`, not `lib/api.ts` (whose functions assume the server-only `API_BASE_URL`, unusable from a Client Component).

- [ ] **Step 1: Write the failing test**

Create `frontend/components/PinToggle.test.tsx`:

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { PinToggle } from './PinToggle';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe('PinToggle', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a filled star when initially pinned, outline when not', () => {
    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={true} />);
    expect(screen.getByLabelText('Unpin')).toBeInTheDocument();
  });

  it('pinning fetches current preferences then PUTs the id appended', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: ['swr-alton'], pinnedStations: [] }), { status: 200 });
      }
      return new Response(null, { status: 204 });
    });

    renderWithProvider(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/preferences/pinned-lines',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify(['swr-alton', 'wcml']),
        }),
      );
    });
  });

  it('unpinning fetches current preferences then PUTs the id removed', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: [], pinnedStations: ['WOK', 'AON'] }), { status: 200 });
      }
      return new Response(null, { status: 204 });
    });

    renderWithProvider(<PinToggle kind="station" id="WOK" initiallyPinned={true} />);
    fireEvent.click(screen.getByLabelText('Unpin'));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/preferences/pinned-stations',
        expect.objectContaining({
          method: 'PUT',
          body: JSON.stringify(['AON']),
        }),
      );
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/PinToggle.test.tsx`
Expected: FAIL — `./PinToggle` doesn't exist yet.

- [ ] **Step 3: Implement**

Create `frontend/components/PinToggle.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon } from '@mantine/core';
import type { Preferences } from '@/lib/types';

type PinKind = 'line' | 'station';

/** Calls the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * rather than `lib/api.ts` — this is a Client Component, which cannot read
 * the server-only `API_BASE_URL` env var `lib/api.ts`'s functions rely on. */
export function PinToggle({ kind, id, initiallyPinned }: { kind: PinKind; id: string; initiallyPinned: boolean }) {
  const router = useRouter();
  const [pinned, setPinned] = useState(initiallyPinned);
  const [busy, setBusy] = useState(false);

  async function toggle() {
    setBusy(true);
    try {
      const prefsResponse = await fetch('/api/preferences');
      const prefs: Preferences = await prefsResponse.json();
      const key = kind === 'line' ? 'pinnedLines' : 'pinnedStations';
      const endpoint = kind === 'line' ? '/api/preferences/pinned-lines' : '/api/preferences/pinned-stations';
      const current = prefs[key];
      const next = pinned ? current.filter((existing) => existing !== id) : [...current, id];
      await fetch(endpoint, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      setPinned(!pinned);
      router.refresh();
    } finally {
      setBusy(false);
    }
  }

  return (
    <ActionIcon
      variant={pinned ? 'filled' : 'outline'}
      color="yellow"
      onClick={toggle}
      disabled={busy}
      aria-label={pinned ? 'Unpin' : 'Pin'}
    >
      {pinned ? '★' : '☆'}
    </ActionIcon>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/PinToggle.test.tsx`
Expected: PASS — all 3 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/PinToggle.tsx frontend/components/PinToggle.test.tsx
git commit -m "Add PinToggle component"
```

---

### Task 9: frontend — `/lines` browse page and custom-line creation form

**Files:**
- Create: `frontend/app/lines/page.tsx`
- Create: `frontend/app/lines/CustomLineForm.tsx`

**Interfaces:**
- Consumes: `getAllLines`, `getPreferences` (Task 7), `PinToggle` (Task 8).
- Produces: the `/lines` route. Coexists with the existing `app/lines/[id]/page.tsx` dynamic route (Next.js resolves `/lines` to this static page and `/lines/anything-else` to the dynamic one).

No test for `CustomLineForm`'s submit flow (form-submission-triggering-navigation is awkward to unit test meaningfully here, and this codebase's existing form, `StationSearchForm`, likewise has no test file) — verified manually in Step 3 against the running stack, consistent with that existing precedent.

- [ ] **Step 1: Implement the browse page**

Create `frontend/app/lines/page.tsx`:

```tsx
import { Stack, Title, Table, Text } from '@mantine/core';
import Link from 'next/link';
import { getAllLines, getPreferences } from '@/lib/api';
import { PinToggle } from '@/components/PinToggle';
import { CustomLineForm } from './CustomLineForm';

export const revalidate = 0;

export default async function AllLinesPage() {
  const [lines, preferences] = await Promise.all([getAllLines(), getPreferences()]);
  const pinnedSet = new Set(preferences.pinnedLines);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Title order={1}>All Lines</Title>
        <Table>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>Name</Table.Th>
              <Table.Th>Category</Table.Th>
              <Table.Th>Operators</Table.Th>
              <Table.Th>Pin</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {lines.map((line) => (
              <Table.Tr key={line.id}>
                <Table.Td>
                  {/* Plain `<Link>` wrapping `Text`, not `component={Link}`
                      on a Mantine polymorphic prop — this page is a Server
                      Component, and that pattern previously broke
                      `next build`'s Server/Client boundary check (see
                      LineStatusCard's fix). */}
                  <Link href={`/lines/${line.id}`} style={{ textDecoration: 'none' }}>
                    <Text c="blue">{line.name}</Text>
                  </Link>
                </Table.Td>
                <Table.Td>{line.category}</Table.Td>
                <Table.Td>{line.operators.join(', ')}</Table.Td>
                <Table.Td>
                  <PinToggle kind="line" id={line.id} initiallyPinned={pinnedSet.has(line.id)} />
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      </Stack>

      <Stack gap="md">
        <Title order={2}>New Custom Line</Title>
        <CustomLineForm />
      </Stack>
    </Stack>
  );
}
```

- [ ] **Step 2: Implement the custom-line form**

Create `frontend/app/lines/CustomLineForm.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse } from '@mantine/core';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly. */
export function CustomLineForm() {
  const router = useRouter();
  const [name, setName] = useState('');
  const [operators, setOperators] = useState<string[]>([]);
  const [stationInput, setStationInput] = useState('');
  const [stations, setStations] = useState<string[]>([]);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [headcodePrefixes, setHeadcodePrefixes] = useState<string[]>([]);
  const [destinationCrsFilter, setDestinationCrsFilter] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  function addStation() {
    const crs = stationInput.trim().toUpperCase();
    if (crs.length !== 3 || stations.includes(crs)) return;
    setStations([...stations, crs]);
    setStationInput('');
  }

  function removeStation(crs: string) {
    setStations(stations.filter((s) => s !== crs));
  }

  async function handleSubmit() {
    setError(null);
    if (name.trim().length === 0) {
      setError('Name is required.');
      return;
    }
    if (stations.length < 2) {
      setError('Add at least 2 stations.');
      return;
    }
    setSubmitting(true);
    try {
      const response = await fetch('/api/lines', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, operators, stations, headcodePrefixes, destinationCrsFilter }),
      });
      if (!response.ok) {
        const message = await response.text();
        setError(message || `Request failed: ${response.status}`);
        setSubmitting(false);
        return;
      }
      const created: { id: string } = await response.json();
      router.push(`/lines/${created.id}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="sm" maw={480}>
      <TextInput label="Name" value={name} onChange={(event) => setName(event.currentTarget.value)} />
      <TagsInput label="Operators" placeholder="e.g. SW" value={operators} onChange={setOperators} />
      <Group align="end">
        <TextInput
          label="Add station (CRS code)"
          placeholder="e.g. WOK"
          value={stationInput}
          onChange={(event) => setStationInput(event.currentTarget.value)}
          maxLength={3}
        />
        <Button variant="outline" onClick={addStation} disabled={stationInput.trim().length !== 3}>
          Add
        </Button>
      </Group>
      <Group gap="xs">
        {stations.map((crs) => (
          <Badge key={crs} rightSection={<CloseButton size="xs" onClick={() => removeStation(crs)} />}>
            {crs}
          </Badge>
        ))}
      </Group>
      <Button variant="subtle" onClick={() => setAdvancedOpen((open) => !open)}>
        {advancedOpen ? 'Hide' : 'Show'} advanced options
      </Button>
      <Collapse in={advancedOpen}>
        <Stack gap="sm">
          <TagsInput label="Headcode prefixes" placeholder="e.g. 1P" value={headcodePrefixes} onChange={setHeadcodePrefixes} />
          <TagsInput label="Destination CRS filter" placeholder="e.g. AON" value={destinationCrsFilter} onChange={setDestinationCrsFilter} />
        </Stack>
      </Collapse>
      {error && <Text c="red">{error}</Text>}
      <Button onClick={handleSubmit} loading={submitting}>
        Create line
      </Button>
    </Stack>
  );
}
```

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Open `http://localhost:3000/lines` in a browser (or `curl -s http://localhost:3000/lines | grep -o '<title>[^<]*'` to at least confirm the page renders without a server error). Confirm:
- The table lists the 5 catalogue lines (plus any custom lines already created in earlier tasks' verification).
- Clicking a pin toggle updates it and (via `router.refresh()`) the page reflects the change without a manual reload.
- Filling in the form (name "Verify Line", operators `SW`, stations `WOK`, `AON`) and submitting navigates to `/lines/custom-verify-line`.

Clean up the test line:
```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-verify-line
```

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/page.tsx frontend/app/lines/CustomLineForm.tsx
git commit -m "Add /lines browse page and custom-line creation form"
```

---

### Task 10: frontend — home page redesign (pinned lines + pinned stations)

**Files:**
- Modify: `frontend/app/page.tsx`

**Interfaces:**
- Consumes: `getLineStatusForMode`, `getPreferences`, `getStopPointDisruption` (all existing/Task 7), `LineStatusCard`, `StatusBadge` (existing).

A local `worstSeverityAcrossReports` helper is defined in this file rather than imported from a shared module: the worst-status logic currently lives privately inside `LineStatusCard.tsx` and operates on *one* report, while this page needs the worst severity across *multiple* `LineStatusReport`s (a pinned station can sit on more than one line) for its pinned-stations list. A later, separate plan (outage page redesign) extracts a shared `lib/severity.ts` helper for the single-report case — this task doesn't depend on that landing first, and can be revisited then.

No automated test for this page: no `app/**/*.test.tsx` file exists anywhere in this codebase today (confirmed by search) — Server Component pages with data fetching aren't unit tested here, only `lib/`/`components/` units are. Verified manually in Step 2 instead, consistent with that convention.

- [ ] **Step 1: Implement**

Replace `frontend/app/page.tsx` entirely with:

```tsx
import { Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import { getLineStatusForMode, getPreferences, getStopPointDisruption } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

// See app/lines/[id]/page.tsx-adjacent history page and this repo's other
// dynamic routes for the same `revalidate = 0` rationale: without it,
// Next.js treats this route as eligible for static generation and tries to
// prerender it during `next build`, which fails since the `api` service
// only exists on the compose network at runtime.
export const revalidate = 0;

function worstSeverityAcrossReports(reports: LineStatusReport[]): number {
  let worst = 10; // Good Service
  for (const report of reports) {
    for (const status of report.lineStatuses) {
      if (severityRank(status.statusSeverity) > severityRank(worst)) {
        worst = status.statusSeverity;
      }
    }
  }
  return worst;
}

export default async function DashboardPage() {
  const preferences = await getPreferences();

  const allReports = await getLineStatusForMode('national-rail');
  const pinnedLineReports = allReports.filter((report) => preferences.pinnedLines.includes(report.id));

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => ({
      crs,
      reports: await getStopPointDisruption(crs),
    })),
  );

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Group justify="space-between">
          <Title order={1}>Your Lines</Title>
          <Link href="/lines" style={{ textDecoration: 'none' }}>
            <Text c="blue">Browse all lines</Text>
          </Link>
        </Group>
        {pinnedLineReports.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any lines yet. <Link href="/lines">Browse all lines</Link> to pin some.
          </Text>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedLineReports.map((report) => (
              <LineStatusCard key={report.id} report={report} />
            ))}
          </SimpleGrid>
        )}
      </Stack>

      <Stack gap="md">
        <Group justify="space-between">
          <Title order={2}>Your Stations</Title>
          <Link href="/stations" style={{ textDecoration: 'none' }}>
            <Text c="blue">Look up a station</Text>
          </Link>
        </Group>
        {pinnedStationEntries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any stations yet. <Link href="/stations">Look up a station</Link> to pin one.
          </Text>
        ) : (
          <Stack gap="xs">
            {pinnedStationEntries.map(({ crs, reports }) => (
              <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                <Card withBorder>
                  <Group justify="space-between">
                    <Text fw={600}>{crs}</Text>
                    <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                  </Group>
                </Card>
              </Link>
            ))}
          </Stack>
        )}
      </Stack>
    </Stack>
  );
}
```

- [ ] **Step 2: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

With no pins set, `http://localhost:3000/` should show both empty states. Pin a line and a station via `/lines` and `/stations/{crs}` (Task 11), reload `/`, confirm both sections now show the pinned items with correct status badges.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/page.tsx
git commit -m "Redesign home page around pinned lines and stations"
```

---

### Task 11: frontend — pin toggle on the station page, nav link to `/lines`

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Consumes: `getPreferences` (Task 7), `PinToggle` (Task 8).

- [ ] **Step 1: Add the pin toggle to the station page**

Replace `frontend/app/stations/[crs]/page.tsx` entirely with:

```tsx
import { Stack, Title, Text, Divider, Group } from '@mantine/core';
import { getStopPointDisruption, getPreferences } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { DisruptionDetail } from '@/components/DisruptionDetail';
import { PinToggle } from '@/components/PinToggle';

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;
  const [reports, preferences] = await Promise.all([getStopPointDisruption(crs), getPreferences()]);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {crs}</Title>
        <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
      </Group>
      {reports.length === 0 && <Text c="dimmed">No disruptions affecting this station.</Text>}
      {reports.map((report) => (
        <div key={report.id}>
          <Divider my="sm" />
          <Text fw={600}>{report.name}</Text>
          {report.lineStatuses.map((status, i) => (
            <Stack key={i} gap="xs">
              <StatusBadge severity={status.statusSeverity} />
              <Text>{status.reason}</Text>
              {status.disruption && <DisruptionDetail disruption={status.disruption} />}
            </Stack>
          ))}
        </div>
      ))}
    </Stack>
  );
}
```

- [ ] **Step 2: Add a nav link to `/lines`**

In `frontend/app/layout.tsx`, add a link next to the existing "Station Lookup" one:

```tsx
            <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
              <Text fw={700}>National Rail Line Status</Text>
            </Link>
            <Group gap="lg">
              <Link href="/lines" style={{ textDecoration: 'none' }}>
                <Text c="blue">All Lines</Text>
              </Link>
              <Link href="/stations" style={{ textDecoration: 'none' }}>
                <Text c="blue">Station Lookup</Text>
              </Link>
            </Group>
```

This replaces the existing `<Link href="/stations">...</Link>` (previously a direct sibling of the title link inside the outer `Group`) with a nested `Group` containing both nav links, so the outer `Group justify="space-between"` still puts the title on the left and both nav links together on the right.

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Visit `http://localhost:3000/stations/WOK`, confirm the pin toggle appears next to the title and toggling it works (check `curl -s http://localhost:8080/public/preferences` reflects the change). Confirm the nav bar shows both "All Lines" and "Station Lookup" links and both navigate correctly.

- [ ] **Step 4: Run the frontend test suite**

Run: `cd frontend && npm test`
Expected: all tests pass (existing suite plus Tasks 7/8's new tests).

- [ ] **Step 5: Commit**

```bash
git add frontend/app/stations/\[crs\]/page.tsx frontend/app/layout.tsx
git commit -m "Add pin toggle to station page and All Lines nav link"
```
