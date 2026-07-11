# Operator/Station Autocomplete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace free-text operator/station code entry with type-ahead
suggestions (matching on code or name) in every form field that takes an
ATOC operator code or CRS station code.

**Architecture:** Two new unauthenticated GET endpoints
(`/public/stations?q=`, `/public/tocs?q=`) query the existing `stations`/
`tocs` reference tables with `ILIKE` substring matching, capped at 20
results. The frontend debounces keystrokes, fetches through the existing
same-origin `/api/*` proxy (client components can't read the server-only
`API_BASE_URL`), and feeds results into Mantine's `data` prop on
`Autocomplete` (single-value fields) or `TagsInput` (multi-value fields).

**Tech Stack:** Rust/axum/sqlx (backend), Next.js/React/Mantine v9/vitest
(frontend).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md`.
- No new DB indexes — `stations`/`tocs` are ~2,500/~30 rows; a sequential
  `ILIKE` scan is fast enough (spec's Non-goals).
- No fuzzy/`pg_trgm` matching — plain substring `ILIKE` on code or name.
- New backend queries use runtime-checked `sqlx::query`/`sqlx::query_as`,
  **not** the `query!`/`query_as!` macro family — this codebase
  deliberately avoids needing a live DB or checked-in `.sqlx` cache at
  compile time (`crates/api/src/data/queries.rs` module doc). Confirmed
  during planning: no `.sqlx` dir, no `SQLX_OFFLINE` reference anywhere in
  the repo.
- **Deviation from the spec's Testing section:** the spec says "Rust unit
  tests for `search_stations`/`search_tocs` against a test DB, following
  the existing test patterns in `queries.rs`." Verified while planning
  that `queries.rs` has **no DB-integration tests** — its `#[cfg(test)]`
  module only tests the pure `incident_changed` helper, and no
  `sqlx::test`/DB-integration harness exists anywhere in this workspace
  (`custom_lines.rs` similarly only unit-tests its pure `slugify`
  function). Building a DB-test harness from scratch is out of scope for
  this feature. Task 1 below instead extracts the one piece of real,
  pure, DB-free logic (query trimming/empty-check) into a unit-testable
  function, matching the established pattern, and the SQL itself is
  verified by manual `curl` against a running `docker compose` stack in
  Task 5.
- Client Components must fetch suggestions through the existing
  same-origin `/api/*` proxy (`frontend/app/api/[...path]/route.ts`), not
  through `frontend/lib/api.ts`'s `baseUrl()` — that reads the
  server-only `API_BASE_URL` env var, which is not available in the
  browser (confirmed: `CustomLineForm.tsx`'s existing POST and
  `PinToggle.tsx` both already fetch `/api/...` directly for this reason,
  never through `lib/api.ts`). This is a intentional correction versus
  the spec's "`frontend/lib/api.ts` gains `searchStations`/`searchTocs`"
  wording — those functions live in a new `frontend/lib/suggestions.ts`
  instead, to keep `lib/api.ts` exclusively for the server-only
  `baseUrl()` pattern.

---

## File Structure

- Create: `crates/api/src/data/reference.rs` — `Suggestion` struct,
  `search_stations`/`search_tocs` query functions.
- Modify: `crates/api/src/data/mod.rs` — register the new module.
- Create: `crates/api/src/routes/reference.rs` — `GET /stations`,
  `GET /tocs` handlers, `sanitize_query` helper + its unit tests.
- Modify: `crates/api/src/routes/mod.rs` — register the module, merge its
  router into `public_router()`.
- Create: `frontend/lib/suggestions.ts` — `Suggestion` type,
  `searchStations`/`searchTocs` client-side fetch functions (via the
  `/api/*` proxy).
- Create: `frontend/lib/useSuggestions.ts` — debounced, abort-aware hook
  shared by every autocomplete field.
- Create: `frontend/lib/useSuggestions.test.ts` — hook tests.
- Modify: `frontend/app/lines/CustomLineForm.tsx` — Operators
  (`TagsInput`), Add station (`TextInput` → `Autocomplete`), Destination
  CRS filter (`TagsInput`) all wired to suggestions. Headcode prefixes
  untouched.
- Modify: `frontend/app/stations/StationSearchForm.tsx` — station lookup
  `TextInput` → `Autocomplete`.

---

### Task 1: Backend — reference search data layer

**Files:**
- Create: `crates/api/src/data/reference.rs`
- Modify: `crates/api/src/data/mod.rs:1-5`

**Interfaces:**
- Produces: `pub struct Suggestion { pub code: String, pub name: String }`
  (derives `Debug, Clone, Serialize, sqlx::FromRow`); `pub async fn
  search_stations(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<Suggestion>>`;
  `pub async fn search_tocs(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<Suggestion>>`.
  Task 2's route handlers call these directly.

- [ ] **Step 1: Write `crates/api/src/data/reference.rs`**

```rust
//! Read-only type-ahead search over the `stations`/`tocs` reference
//! tables. See
//! docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md.
//!
//! Uses runtime-checked `sqlx::query_as` rather than the `query_as!`
//! macro family, matching `queries.rs`'s established rationale: the
//! macros need a live DB or a checked-in `.sqlx` cache at compile time,
//! which this workspace deliberately doesn't carry.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Suggestion {
    pub code: String,
    pub name: String,
}

/// Matches `q` as a case-insensitive substring of either the CRS code or
/// the station name. `q` must already be trimmed and non-empty (callers
/// go through `routes::reference::sanitize_query` first).
pub async fn search_stations(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let pattern = format!("%{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT crs AS code, name FROM stations \
         WHERE crs ILIKE $1 OR name ILIKE $1 \
         ORDER BY name LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Matches `q` as a case-insensitive substring of either the ATOC code or
/// the operator name. Same trimmed/non-empty contract as
/// [`search_stations`].
pub async fn search_tocs(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Suggestion>> {
    let pattern = format!("%{q}%");
    let rows: Vec<Suggestion> = sqlx::query_as(
        "SELECT atoc_code AS code, name FROM tocs \
         WHERE atoc_code ILIKE $1 OR name ILIKE $1 \
         ORDER BY name LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 2: Register the module**

In `crates/api/src/data/mod.rs`, add `reference` alphabetically among the
existing `pub mod` lines:

```rust
pub mod config;
pub mod custom_lines;
pub mod preferences;
pub mod queries;
pub mod reference;
pub mod samples;

pub use common::{LineDefinition, Station};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p api`
Expected: no errors. `search_stations`/`search_tocs` are unused at this
point (no route calls them yet) — expect an `unused` warning, not an
error; that's resolved once Task 2 wires them in.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/reference.rs crates/api/src/data/mod.rs
git commit -m "Add reference-data search queries for stations and TOCs"
```

---

### Task 2: Backend — `/public/stations` and `/public/tocs` routes

**Files:**
- Create: `crates/api/src/routes/reference.rs`
- Modify: `crates/api/src/routes/mod.rs:7-40`

**Interfaces:**
- Consumes: `crate::data::reference::{Suggestion, search_stations, search_tocs}` (Task 1).
- Produces: `pub fn router() -> Router` merged into `public_router()`,
  exposing `GET /stations?q=` and `GET /tocs?q=` (become
  `/public/stations`, `/public/tocs` once nested in `main.rs`).

- [ ] **Step 1: Write the failing test for `sanitize_query`**

Create `crates/api/src/routes/reference.rs` with just the helper and its
tests first:

```rust
//! `/public/stations`, `/public/tocs`: type-ahead search over reference
//! data. Unauthenticated, read-only — same `public_router()` pattern as
//! `lines.rs`. See
//! docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md.

/// Trims `raw`; returns `None` if the result is empty. Used to skip
/// querying the DB entirely for a type-ahead request with no search text
/// yet (e.g. the field was just focused, or the user cleared it).
fn sanitize_query(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_query_trims_whitespace() {
        assert_eq!(sanitize_query("  wok  "), Some("wok"));
    }

    #[test]
    fn sanitize_query_rejects_empty_or_whitespace_only() {
        assert_eq!(sanitize_query(""), None);
        assert_eq!(sanitize_query("   "), None);
    }

    #[test]
    fn sanitize_query_passes_through_non_whitespace_unchanged() {
        assert_eq!(sanitize_query("SW"), Some("SW"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p api sanitize_query`
Expected: 3 passed (the function is trivial enough to write correct on
the first pass here — but run it to confirm before moving on, per the
project's TDD convention).

- [ ] **Step 3: Add the route handlers**

Extend `crates/api/src/routes/reference.rs` (above the `#[cfg(test)]`
block):

```rust
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::reference::{self, Suggestion};

/// Caps how many rows a single type-ahead request can return. 20 is
/// plenty for a dropdown the user is actively narrowing by typing more.
const SUGGESTION_LIMIT: i64 = 20;

pub fn router() -> Router {
    Router::new()
        .route("/stations", axum::routing::get(search_stations))
        .route("/tocs", axum::routing::get(search_tocs))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search_stations(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let Some(q) = sanitize_query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let results = reference::search_stations(&app.database, q, SUGGESTION_LIMIT)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

async fn search_tocs(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let Some(q) = sanitize_query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let results = reference::search_tocs(&app.database, q, SUGGESTION_LIMIT)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "reference search failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}
```

(The `sanitize_query` function and its `#[cfg(test)] mod tests` block from
Step 1 stay where they are, above this new code.)

- [ ] **Step 4: Wire the router into `public_router()`**

In `crates/api/src/routes/mod.rs`, add the module declaration:

```rust
pub mod health;
pub mod ingest;
pub mod line_status;
pub mod lines;
pub mod preferences;
pub mod reference;
pub mod samples;
```

And merge it into `public_router()`:

```rust
    Router::new()
        .merge(health::router())
        .merge(lines::router())
        .merge(preferences::router())
        .merge(reference::router())
```

- [ ] **Step 5: Verify the workspace builds and all tests pass**

Run: `cargo test -p api`
Expected: all tests pass, including the 3 `sanitize_query` tests from
Step 2.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/reference.rs crates/api/src/routes/mod.rs
git commit -m "Add GET /public/stations and /public/tocs type-ahead endpoints"
```

---

### Task 3: Frontend — suggestion fetch helpers + debounced hook

**Files:**
- Create: `frontend/lib/suggestions.ts`
- Create: `frontend/lib/useSuggestions.ts`
- Create: `frontend/lib/useSuggestions.test.ts`

**Interfaces:**
- Produces: `export interface Suggestion { code: string; name: string }`;
  `export async function searchStations(q: string, signal?: AbortSignal): Promise<Suggestion[]>`;
  `export async function searchTocs(q: string, signal?: AbortSignal): Promise<Suggestion[]>`;
  `export function useSuggestions(query: string, search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>): { suggestions: Suggestion[]; loading: boolean }`.
  Tasks 4 and 5 import `searchStations`/`searchTocs`/`useSuggestions` from
  these two modules.

- [ ] **Step 1: Write `frontend/lib/suggestions.ts`**

```ts
export interface Suggestion {
  code: string;
  name: string;
}

/** Client-side fetch through the same-origin `/api/*` proxy
 * (`app/api/[...path]/route.ts`) — Client Components can't read the
 * server-only `API_BASE_URL`, so this can't go through `lib/api.ts`'s
 * `baseUrl()` like the server-rendered fetches do. Empty/whitespace `q`
 * short-circuits without a network call, mirroring the backend's own
 * empty-query short-circuit. */
export async function searchStations(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const response = await fetch(`/api/stations?q=${encodeURIComponent(q)}`, { signal });
  if (!response.ok) return [];
  return response.json() as Promise<Suggestion[]>;
}

export async function searchTocs(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const response = await fetch(`/api/tocs?q=${encodeURIComponent(q)}`, { signal });
  if (!response.ok) return [];
  return response.json() as Promise<Suggestion[]>;
}
```

- [ ] **Step 2: Write the failing tests for `useSuggestions`**

Create `frontend/lib/useSuggestions.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { useSuggestions } from './useSuggestions';
import type { Suggestion } from './suggestions';

const sample: Suggestion[] = [{ code: 'WOK', name: 'Woking' }];

describe('useSuggestions', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns no suggestions and does not call search for an empty query', () => {
    const search = vi.fn();
    const { result } = renderHook(() => useSuggestions('', search));
    expect(result.current.suggestions).toEqual([]);
    expect(search).not.toHaveBeenCalled();
  });

  it('debounces before calling search', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { rerender } = renderHook(({ query }) => useSuggestions(query, search), {
      initialProps: { query: 'w' },
    });
    rerender({ query: 'wo' });
    rerender({ query: 'wok' });

    expect(search).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith('wok', expect.any(AbortSignal));
  });

  it('populates suggestions once the debounced search resolves', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { result } = renderHook(() => useSuggestions('wok', search));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    await waitFor(() => expect(result.current.suggestions).toEqual(sample));
  });

  it('aborts the in-flight request when the query changes again before it resolves', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { rerender } = renderHook(({ query }) => useSuggestions(query, search), {
      initialProps: { query: 'wok' },
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(search).toHaveBeenCalledTimes(1);
    const firstSignal = search.mock.calls[0][1] as AbortSignal;

    rerender({ query: 'alt' });
    expect(firstSignal.aborted).toBe(true);
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npm --prefix frontend test -- useSuggestions`
Expected: FAIL — `useSuggestions` does not exist yet (`Cannot find module
'./useSuggestions'` or similar).

- [ ] **Step 4: Write `frontend/lib/useSuggestions.ts`**

```ts
'use client';

import { useEffect, useRef, useState } from 'react';
import type { Suggestion } from './suggestions';

const DEBOUNCE_MS = 250;

/** Debounces `query` by 250ms, then calls `search(query, signal)`.
 * Aborts the in-flight request (via `signal`) whenever `query` changes
 * again before the previous call resolves, so a fast typist never has a
 * stale, slower response overwrite a newer one. Shared by every
 * operator/station autocomplete field. */
export function useSuggestions(
  query: string,
  search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>,
): { suggestions: Suggestion[]; loading: boolean } {
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!query.trim()) {
      setSuggestions([]);
      setLoading(false);
      return;
    }

    const controller = new AbortController();
    setLoading(true);
    const timer = setTimeout(() => {
      search(query, controller.signal)
        .then((results) => {
          if (!controller.signal.aborted) {
            setSuggestions(results);
          }
        })
        .catch((err: unknown) => {
          if (!(err instanceof DOMException && err.name === 'AbortError')) {
            setSuggestions([]);
          }
        })
        .finally(() => {
          if (!controller.signal.aborted) {
            setLoading(false);
          }
        });
    }, DEBOUNCE_MS);

    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query, search]);

  return { suggestions, loading };
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npm --prefix frontend test -- useSuggestions`
Expected: all 4 tests pass.

- [ ] **Step 6: Run the full frontend test suite and type check**

Run: `npm --prefix frontend test && npm --prefix frontend run build`
Expected: all existing tests still pass; `next build` succeeds (this also
type-checks the new files).

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/suggestions.ts frontend/lib/useSuggestions.ts frontend/lib/useSuggestions.test.ts
git commit -m "Add debounced station/operator suggestion fetching"
```

---

### Task 4: Frontend — wire autocomplete into `CustomLineForm`

**Files:**
- Modify: `frontend/app/lines/CustomLineForm.tsx`

**Interfaces:**
- Consumes: `useSuggestions` (Task 3), `searchStations`/`searchTocs`
  (Task 3).

- [ ] **Step 1: Add imports and per-field query state**

In `frontend/app/lines/CustomLineForm.tsx`, replace the import line and
add three new pieces of state (one query string per autocomplete field —
separate from the fields' own committed `value` state, since Mantine's
`onSearchChange` reports the live in-progress text, not the committed
tags/value):

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
```

```tsx
  const [operatorsQuery, setOperatorsQuery] = useState('');
  const { suggestions: operatorSuggestions } = useSuggestions(operatorsQuery, searchTocs);

  const { suggestions: stationSuggestions } = useSuggestions(stationInput, searchStations);

  const [destinationQuery, setDestinationQuery] = useState('');
  const { suggestions: destinationSuggestions } = useSuggestions(destinationQuery, searchStations);
```

(`stationInput` already exists as state at line 13 — reused directly,
no new query state needed for that field, since it's already a
single-value text field with nothing else it needs to track.)

- [ ] **Step 2: Wire the Operators field**

Replace:

```tsx
      <TagsInput label="Operators" placeholder="e.g. SW" value={operators} onChange={setOperators} />
```

with:

```tsx
      <TagsInput
        label="Operators"
        placeholder="e.g. SW"
        value={operators}
        onChange={setOperators}
        onSearchChange={setOperatorsQuery}
        data={operatorSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
      />
```

- [ ] **Step 3: Wire the Add station field**

Replace:

```tsx
        <TextInput
          label="Add station (CRS code)"
          placeholder="e.g. WOK"
          value={stationInput}
          onChange={(event) => setStationInput(event.currentTarget.value)}
          maxLength={3}
        />
```

with:

```tsx
        <Autocomplete
          label="Add station (CRS code)"
          placeholder="e.g. WOK"
          value={stationInput}
          onChange={setStationInput}
          maxLength={3}
          data={stationSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
        />
```

(`Autocomplete`'s `onChange` receives the string value directly, unlike
`TextInput`'s change-event — no `event.currentTarget.value` unwrapping
needed.)

- [ ] **Step 4: Wire the Destination CRS filter field**

Replace:

```tsx
          <TagsInput label="Destination CRS filter" placeholder="e.g. AON" value={destinationCrsFilter} onChange={setDestinationCrsFilter} />
```

with:

```tsx
          <TagsInput
            label="Destination CRS filter"
            placeholder="e.g. AON"
            value={destinationCrsFilter}
            onChange={setDestinationCrsFilter}
            onSearchChange={setDestinationQuery}
            data={destinationSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
          />
```

Leave the "Headcode prefixes" `TagsInput` immediately above it untouched
— it's not a station/operator code (see spec Non-goals).

- [ ] **Step 5: Manually verify in the browser**

Run: `docker compose --profile dev up` (brings up the full stack; see
`docker-compose.yml`'s usage comment), then open
`http://localhost:3000/lines` and:
- Type into "Operators" — a dropdown of matching TOCs should appear after
  ~250ms.
- Type into "Add station (CRS code)" — a dropdown of matching stations
  should appear.
- Open "advanced options" and type into "Destination CRS filter" — same
  behavior.
- Confirm typing a code with no match (e.g. "ZZZ") still lets you add it
  as a free tag/value (Operators, Destination CRS filter) or leaves the
  Add button disabled until 3 characters are typed (Add station field —
  unchanged existing behavior).

Expected: suggestions appear, selecting one fills the field with just the
code, and the existing create-line flow still works end to end.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/lines/CustomLineForm.tsx
git commit -m "Add autocomplete suggestions to CustomLineForm's operator/station fields"
```

---

### Task 5: Frontend — wire autocomplete into `StationSearchForm`

**Files:**
- Modify: `frontend/app/stations/StationSearchForm.tsx`

**Interfaces:**
- Consumes: `useSuggestions`, `searchStations` (Task 3).

- [ ] **Step 1: Rewrite the component**

Replace the full contents of
`frontend/app/stations/StationSearchForm.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, Button, Group } from '@mantine/core';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

export function StationSearchForm() {
  const router = useRouter();
  const [crs, setCrs] = useState('');
  const { suggestions } = useSuggestions(crs, searchStations);

  function handleSearch() {
    const trimmed = crs.trim().toUpperCase();
    if (!trimmed) return;
    router.push(`/stations/${trimmed}`);
  }

  return (
    <Group align="end">
      <Autocomplete
        label="Station CRS code"
        placeholder="e.g. WOK"
        value={crs}
        onChange={setCrs}
        maxLength={3}
        data={suggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
      />
      <Button onClick={handleSearch} disabled={crs.trim().length === 0}>
        Look up
      </Button>
    </Group>
  );
}
```

- [ ] **Step 2: Manually verify in the browser**

With the dev stack still running from Task 4 Step 5, open
`http://localhost:3000/stations` and type into "Station CRS code" —
confirm a dropdown of matching stations appears, selecting one fills the
field with the code, and "Look up" still navigates to
`/stations/<CODE>`.

- [ ] **Step 3: Run the full frontend test suite one more time**

Run: `npm --prefix frontend test`
Expected: all tests pass (no test file exists for `StationSearchForm`
today — confirmed during planning — so this step only guards against a
regression elsewhere).

- [ ] **Step 4: Commit**

```bash
git add frontend/app/stations/StationSearchForm.tsx
git commit -m "Add autocomplete suggestions to StationSearchForm"
```

---

### Task 6: End-to-end verification against real data

**Files:** none (verification only).

- [ ] **Step 1: Bring up the dev stack and confirm reference data is populated**

Run: `docker compose --profile dev up -d postgres api-dev`, wait for
`api-dev` to report healthy, then:

```bash
curl -s "http://localhost:8080/public/stations?q=wok" | head -c 500
curl -s "http://localhost:8080/public/tocs?q=south" | head -c 500
```

Expected: JSON arrays of `{"code": ..., "name": ...}` objects. If both
return `[]`, the `stations`/`tocs` tables are empty in this environment
(poller-stations/poller-tocs haven't run yet, or RDM credentials aren't
configured — see `.env.example`'s notes on unconfirmed RDM feeds) — that's
an environment/data issue, not a bug in this feature; re-run against a
stack with populated reference tables before concluding otherwise.

- [ ] **Step 2: Confirm the empty-query short-circuit**

```bash
curl -s "http://localhost:8080/public/stations" 
curl -s "http://localhost:8080/public/stations?q=   "
```

Expected: both return `[]` with no error, per Task 2's `sanitize_query`
short-circuit.

- [ ] **Step 3: Confirm case-insensitivity and code-vs-name matching**

```bash
curl -s "http://localhost:8080/public/stations?q=WOKING"
curl -s "http://localhost:8080/public/stations?q=wok"
curl -s "http://localhost:8080/public/tocs?q=sw"
```

Expected: the first two return the same station (name match, then code
match, both case-insensitive); the third returns TOCs whose ATOC code or
name contains "sw" (case-insensitive).

- [ ] **Step 4: Full workspace verification**

Run: `cargo test --workspace && npm --prefix frontend test && npm --prefix frontend run build`
Expected: everything passes — this is the final gate before considering
the feature done.

- [ ] **Step 5: Bring the stack down**

```bash
docker compose --profile dev down
```
