# Edit/Delete Custom Lines Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user edit an existing custom line (name/operators/stations/
headcode prefixes/destination filter) and delete a custom line, both from
the line detail page.

**Architecture:** Reuse the existing create form (`CustomLineForm`) in an
"edit mode" driven by an optional `existingLine` prop, backed by two new
backend endpoints (`GET`/`PUT /lines/{id}`) that reuse the existing
create validation and request/response shapes. Delete wires a new
confirmation-modal component to the `DELETE /lines/{id}` endpoint that
already exists but has no UI today.

**Tech Stack:** Rust/axum/sqlx (backend), Next.js/React/Mantine v9/vitest
(frontend).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-12-edit-custom-lines-design.md`.
- A custom line's `id` never changes on edit, even if the name does —
  it's derived once at creation (`custom_lines::slugify`) and pinned-line
  references / bookmarked URLs depend on it staying stable.
- `PUT /lines/{id}`'s request body reuses `CreateLineRequest` (same shape
  `POST /lines` already accepts) — no new request struct.
- `GET /lines/{id}`'s response is a new `CustomLineDetail` struct
  (`#[serde(rename_all = "camelCase")]`) — **not** `common::CustomLine`
  directly, which has no `rename_all` and would serialize
  `headcode_prefixes`/`destination_crs_filter` in snake_case, breaking
  the camelCase convention every other endpoint in this file uses.
- **Deviation from the spec's Testing section:** the spec's frontend
  testing paragraph describes coverage for "the detail page's edit/delete
  buttons... gating logic." Verified while planning that no `app/*/page.tsx`
  file in this codebase has automated test coverage (confirmed against
  the full list of existing `*.test.tsx` files — all are `components/*`
  or `lib/*`), so there's no established pattern for testing an async
  Next.js Server Component page directly. Task 5 below instead puts full
  automated coverage on the new `DeleteLineButton` — a Client Component,
  directly testable via the same `render`/`fireEvent` pattern
  `PinToggle.test.tsx` already uses — and the detail page's `isCustom`
  gating (Task 6) is verified manually in Task 7, consistent with
  `CustomLineForm`'s own already-untested precedent.
- This repo's `docker-compose.yml` (as of this plan's authoring) has no
  `--profile dev`/`--profile prod` split — that's separate, unrelated,
  still-uncommitted work sitting in the main checkout. Task 7's
  `docker compose` commands use plain (unprefixed) service names.

---

## File Structure

- Modify: `crates/api/src/data/custom_lines.rs` — add `get_custom_line`,
  `update_custom_line`.
- Modify: `crates/api/src/routes/lines.rs` — add `CustomLineDetail`
  struct, `get_line`/`update_line` handlers, extend the `/lines/{id}`
  route with `GET`/`PUT`.
- Modify: `frontend/lib/types.ts` — add `CustomLineDetail`.
- Modify: `frontend/lib/api.ts` — add `getCustomLine`.
- Modify: `frontend/lib/api.test.ts` — test `getCustomLine`.
- Modify: `frontend/app/lines/CustomLineForm.tsx` — optional
  `existingLine` prop switches create→edit mode.
- Create: `frontend/app/lines/[id]/edit/page.tsx` — fetches the line,
  renders `CustomLineForm` in edit mode.
- Create: `frontend/components/DeleteLineButton.tsx` — confirm-then-DELETE
  button.
- Create: `frontend/components/DeleteLineButton.test.tsx`.
- Modify: `frontend/app/lines/[id]/page.tsx` — show Edit/Delete for
  custom lines only.

---

### Task 1: Backend — get/update custom line data layer + routes

**Files:**
- Modify: `crates/api/src/data/custom_lines.rs`
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Produces: `pub async fn get_custom_line(pool: &PgPool, id: &str) -> anyhow::Result<Option<CustomLine>>`;
  `pub async fn update_custom_line(pool: &PgPool, id: &str, new: NewCustomLine) -> anyhow::Result<Option<CustomLine>>`
  (both in `crate::data::custom_lines`). Route-level: `GET /lines/{id}` →
  `CustomLineDetail` JSON (404 if not a custom line); `PUT /lines/{id}` →
  `LineSummary` JSON (400 for a catalogue id or invalid body, 404 if the
  id doesn't exist). Task 2-6 (frontend) consume these two routes.

- [ ] **Step 1: Add `get_custom_line` to `crates/api/src/data/custom_lines.rs`**

Find this in the file:

```rust
        .collect()
}

/// Inserts a new custom line, deriving its id from `new.name` via
```

Replace with:

```rust
        .collect()
}

/// Fetches one custom line by id, or `None` if no custom line has that id
/// (including catalogue-line ids, which are never rows in this table).
pub async fn get_custom_line(pool: &PgPool, id: &str) -> Result<Option<CustomLine>> {
    let row = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(CustomLine {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        operators: row.try_get("operators")?,
        stations: row.try_get("stations")?,
        headcode_prefixes: row.try_get("headcode_prefixes")?,
        destination_crs_filter: row.try_get("destination_crs_filter")?,
    }))
}

/// Inserts a new custom line, deriving its id from `new.name` via
```

- [ ] **Step 2: Add `update_custom_line` to the same file**

Find this (the end of `insert_custom_line`, right before `delete_custom_line`'s doc comment):

```rust
        destination_crs_filter: new.destination_crs_filter,
    })
}

/// Deletes a custom line by id, and any `pinned_lines` row referencing it,
```

Replace with:

```rust
        destination_crs_filter: new.destination_crs_filter,
    })
}

/// Updates an existing custom line's editable fields in place. The `id`
/// itself is never changed — it was derived once at creation time and
/// pinned-line references / bookmarked URLs depend on it staying stable,
/// even if the line is later renamed. Returns `None` if no custom line
/// has that id (mirrors [`delete_custom_line`]'s `bool` — `Option` here
/// instead since the caller needs the updated row back on success).
pub async fn update_custom_line(pool: &PgPool, id: &str, new: NewCustomLine) -> Result<Option<CustomLine>> {
    let result = sqlx::query(
        r#"
        UPDATE custom_lines
        SET name = $2, operators = $3, stations = $4, headcode_prefixes = $5, destination_crs_filter = $6
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(&new.name)
    .bind(&new.operators)
    .bind(&new.stations)
    .bind(&new.headcode_prefixes)
    .bind(&new.destination_crs_filter)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(Some(CustomLine {
        id: id.to_string(),
        name: new.name,
        operators: new.operators,
        stations: new.stations,
        headcode_prefixes: new.headcode_prefixes,
        destination_crs_filter: new.destination_crs_filter,
    }))
}

/// Deletes a custom line by id, and any `pinned_lines` row referencing it,
```

- [ ] **Step 3: Verify the data layer compiles**

Run: `cargo check -p api`
Expected: no errors (the two new functions are unused until Step 4 wires
them in — that's fine, same as every prior task in this project's
data-layer-then-routes pattern).

- [ ] **Step 4: Add `CustomLineDetail` to `crates/api/src/routes/lines.rs`**

Find:

```rust
struct LineSummary {
    id: String,
    name: String,
    category: String,
    operators: Vec<String>,
    source: &'static str,
}

async fn list_lines(
```

Replace with:

```rust
struct LineSummary {
    id: String,
    name: String,
    category: String,
    operators: Vec<String>,
    source: &'static str,
}

/// Full custom-line record, returned by `GET /lines/{id}` to pre-populate
/// an edit form. `LineSummary` (above) is deliberately a smaller
/// projection used by the list endpoint for both catalogue and custom
/// lines — it lacks `stations`/`headcodePrefixes`/`destinationCrsFilter`,
/// which only exist for custom lines and are exactly what an edit form
/// needs to pre-fill.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomLineDetail {
    id: String,
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    headcode_prefixes: Vec<String>,
    destination_crs_filter: Vec<String>,
}

async fn list_lines(
```

- [ ] **Step 5: Add `get_line` and `update_line` handlers**

Find:

```rust
async fn delete_line(
```

Replace with:

```rust
async fn get_line(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<CustomLineDetail>, (StatusCode, String)> {
    let line = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;

    // No separate catalogue-id check needed here (unlike `update_line`/
    // `delete_line`): `get_custom_line` only ever queries the
    // `custom_lines` table, so a catalogue id naturally comes back `None`
    // and 404s the same way an unknown id would — there's no distinct
    // error message worth giving for "that's a catalogue line" on a
    // read-only lookup the way there is for a rejected write.
    let Some(line) = line else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };

    Ok(Json(CustomLineDetail {
        id: line.id,
        name: line.name,
        operators: line.operators,
        stations: line.stations,
        headcode_prefixes: line.headcode_prefixes,
        destination_crs_filter: line.destination_crs_filter,
    }))
}

async fn update_line(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    if app.config.lines.iter().any(|l| l.id == id) {
        return Err((
            StatusCode::BAD_REQUEST,
            "cannot edit a catalogue line".to_string(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "name must not be empty".to_string(),
        ));
    }
    if req.stations.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "a line needs at least 2 stations".to_string(),
        ));
    }

    let updated = custom_lines::update_custom_line(
        &app.database,
        &id,
        NewCustomLine {
            name: req.name,
            operators: req.operators,
            stations: req.stations,
            headcode_prefixes: req.headcode_prefixes,
            destination_crs_filter: req.destination_crs_filter,
        },
    )
    .await
    .map_err(internal_error)?;

    let Some(updated) = updated else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };

    Ok(Json(LineSummary {
        id: updated.id,
        name: updated.name,
        category: "custom".to_string(),
        operators: updated.operators,
        source: "custom",
    }))
}

async fn delete_line(
```

- [ ] **Step 6: Wire `GET`/`PUT` into the router**

Find:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route("/lines/{id}", axum::routing::delete(delete_line))
}
```

Replace with:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route(
            "/lines/{id}",
            axum::routing::get(get_line).put(update_line).delete(delete_line),
        )
}
```

- [ ] **Step 7: Verify the workspace builds and all existing tests pass**

Run: `cargo test --workspace`
Expected: all tests pass (this task adds no new automated tests — see
this plan's Global Constraints on why; correctness is verified manually
via `curl` in Task 7).

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/data/custom_lines.rs crates/api/src/routes/lines.rs
git commit -m "Add GET and PUT /public/lines/{id} for editing custom lines"
```

---

### Task 2: Frontend — CustomLineDetail type + getCustomLine client

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Consumes: `GET /public/lines/{id}` (Task 1).
- Produces: `export interface CustomLineDetail { id: string; name: string; operators: string[]; stations: string[]; headcodePrefixes: string[]; destinationCrsFilter: string[] }`;
  `export async function getCustomLine(id: string): Promise<CustomLineDetail>`.
  Tasks 3 and 4 import both from `@/lib/types` and `@/lib/api`
  respectively.

- [ ] **Step 1: Add `CustomLineDetail` to `frontend/lib/types.ts`**

Find:

```ts
export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom';
}
```

Replace with:

```ts
export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom';
}

export interface CustomLineDetail {
  id: string;
  name: string;
  operators: string[];
  stations: string[];
  headcodePrefixes: string[];
  destinationCrsFilter: string[];
}
```

- [ ] **Step 2: Write the failing test for `getCustomLine`**

In `frontend/lib/api.test.ts`, find:

```ts
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

Replace with:

```ts
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
  getPreferences,
  getAllLines,
  getCustomLine,
  ApiNotFoundError,
} from './api';
```

Then find:

```ts
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

Replace with:

```ts
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

  it('getCustomLine fetches the correct URL with no caching', async () => {
    const sampleLine = {
      id: 'custom-my-commute',
      name: 'My Commute',
      operators: ['SW'],
      stations: ['WOK', 'WAT'],
      headcodePrefixes: [],
      destinationCrsFilter: [],
    };
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify(sampleLine), { status: 200 })),
    );
    await getCustomLine('custom-my-commute');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/custom-my-commute',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm --prefix frontend test -- api.test`
Expected: FAIL — `getCustomLine` is not exported by `./api` yet.

- [ ] **Step 4: Add `getCustomLine` to `frontend/lib/api.ts`**

Find:

```ts
import type { LineStatusReport, LineStatusHistoryEntry, Preferences, LineSummary } from './types';
```

Replace with:

```ts
import type { LineStatusReport, LineStatusHistoryEntry, Preferences, LineSummary, CustomLineDetail } from './types';
```

Then find:

```ts
export async function getAllLines(): Promise<LineSummary[]> {
  return fetchJson<LineSummary[]>(`${baseUrl()}/public/lines`, { cache: 'no-store' });
}
```

Replace with:

```ts
export async function getAllLines(): Promise<LineSummary[]> {
  return fetchJson<LineSummary[]>(`${baseUrl()}/public/lines`, { cache: 'no-store' });
}

export async function getCustomLine(id: string): Promise<CustomLineDetail> {
  return fetchJson<CustomLineDetail>(`${baseUrl()}/public/lines/${id}`, { cache: 'no-store' });
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npm --prefix frontend test -- api.test`
Expected: all tests in `api.test.ts` pass, including the new one.

- [ ] **Step 6: Run the full frontend test suite and type check**

Run: `npm --prefix frontend test && npm --prefix frontend run build`
Expected: all tests pass, build succeeds.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add CustomLineDetail type and getCustomLine client"
```

---

### Task 3: Frontend — CustomLineForm edit-mode support

**Files:**
- Modify: `frontend/app/lines/CustomLineForm.tsx`

**Interfaces:**
- Consumes: `CustomLineDetail` (Task 2).
- Produces: `CustomLineForm` now accepts an optional
  `existingLine?: CustomLineDetail` prop. Task 4's edit page passes it.

- [ ] **Step 1: Replace the full contents of `frontend/app/lines/CustomLineForm.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import type { CustomLineDetail } from '@/lib/types';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * With `existingLine` set, edits that line via PUT instead of creating a
 * new one via POST. */
export function CustomLineForm({ existingLine }: { existingLine?: CustomLineDetail }) {
  const router = useRouter();
  const [name, setName] = useState(existingLine?.name ?? '');
  const [operators, setOperators] = useState<string[]>(existingLine?.operators ?? []);
  const [stationInput, setStationInput] = useState('');
  const [stations, setStations] = useState<string[]>(existingLine?.stations ?? []);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [headcodePrefixes, setHeadcodePrefixes] = useState<string[]>(existingLine?.headcodePrefixes ?? []);
  const [destinationCrsFilter, setDestinationCrsFilter] = useState<string[]>(existingLine?.destinationCrsFilter ?? []);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const [operatorsQuery, setOperatorsQuery] = useState('');
  const { suggestions: operatorSuggestions } = useSuggestions(operatorsQuery, searchTocs);

  const { suggestions: stationSuggestions } = useSuggestions(stationInput, searchStations);

  const [destinationQuery, setDestinationQuery] = useState('');
  const { suggestions: destinationSuggestions } = useSuggestions(destinationQuery, searchStations);

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
      const url = existingLine ? `/api/lines/${existingLine.id}` : '/api/lines';
      const method = existingLine ? 'PUT' : 'POST';
      const response = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, operators, stations, headcodePrefixes, destinationCrsFilter }),
      });
      if (!response.ok) {
        const message = await response.text();
        setError(message || `Request failed: ${response.status}`);
        setSubmitting(false);
        return;
      }
      router.push(existingLine ? `/lines/${existingLine.id}` : '/lines');
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="sm" maw={480}>
      <TextInput label="Name" value={name} onChange={(event) => setName(event.currentTarget.value)} />
      <TagsInput
        label="Operators"
        placeholder="e.g. SW"
        value={operators}
        onChange={setOperators}
        onSearchChange={setOperatorsQuery}
        data={operatorSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
      />
      <Group align="end">
        <Autocomplete
          label="Add station (CRS code)"
          placeholder="e.g. WOK"
          value={stationInput}
          onChange={setStationInput}
          data={stationSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
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
      <Collapse expanded={advancedOpen}>
        <Stack gap="sm">
          <TagsInput label="Headcode prefixes" placeholder="e.g. 1P" value={headcodePrefixes} onChange={setHeadcodePrefixes} />
          <TagsInput
            label="Destination CRS filter"
            placeholder="e.g. AON"
            value={destinationCrsFilter}
            onChange={setDestinationCrsFilter}
            onSearchChange={setDestinationQuery}
            data={destinationSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
          />
        </Stack>
      </Collapse>
      {error && <Text c="red">{error}</Text>}
      <Button onClick={handleSubmit} loading={submitting}>
        {existingLine ? 'Save changes' : 'Create line'}
      </Button>
    </Stack>
  );
}
```

- [ ] **Step 2: Verify build and existing tests**

Run: `npm --prefix frontend run build && npm --prefix frontend test`
Expected: build succeeds, all tests pass (no test file exists for
`CustomLineForm` — see this plan's Global Constraints).

- [ ] **Step 3: Commit**

```bash
git add frontend/app/lines/CustomLineForm.tsx
git commit -m "Add edit mode to CustomLineForm via optional existingLine prop"
```

---

### Task 4: Frontend — edit page

**Files:**
- Create: `frontend/app/lines/[id]/edit/page.tsx`

**Interfaces:**
- Consumes: `getCustomLine` (Task 2), `CustomLineForm` with `existingLine`
  (Task 3).

- [ ] **Step 1: Write `frontend/app/lines/[id]/edit/page.tsx`**

```tsx
import { notFound } from 'next/navigation';
import { ApiNotFoundError, getCustomLine } from '@/lib/api';
import { CustomLineForm } from '../../CustomLineForm';

export default async function EditCustomLinePage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let line;
  try {
    line = await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return <CustomLineForm existingLine={line} />;
}
```

(Relative import `'../../CustomLineForm'`: this file is at
`app/lines/[id]/edit/page.tsx`, and `CustomLineForm.tsx` is at
`app/lines/CustomLineForm.tsx` — two directories up. Matches the
relative-import convention `app/lines/page.tsx` already uses for the same
component.)

- [ ] **Step 2: Verify build and existing tests**

Run: `npm --prefix frontend run build && npm --prefix frontend test`
Expected: build succeeds (this also type-checks the new file), all tests
pass.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/lines/[id]/edit/page.tsx
git commit -m "Add custom line edit page"
```

---

### Task 5: Frontend — DeleteLineButton component

**Files:**
- Create: `frontend/components/DeleteLineButton.tsx`
- Create: `frontend/components/DeleteLineButton.test.tsx`

**Interfaces:**
- Produces: `export function DeleteLineButton({ id }: { id: string })`.
  Task 6 renders it on the line detail page.

- [ ] **Step 1: Write `frontend/components/DeleteLineButton.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * The confirm button inside the modal carries `aria-label="Confirm delete"`
 * so it has a distinct accessible name from this component's own trigger
 * button once both are simultaneously in the DOM (both read "Delete" as
 * their visible text, matching typical confirm-dialog UX). */
export function DeleteLineButton({ id }: { id: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    try {
      const response = await fetch(`/api/lines/${id}`, { method: 'DELETE' });
      if (!response.ok) {
        const message = await response.text();
        setError(message || `Request failed: ${response.status}`);
        setDeleting(false);
        return;
      }
      router.push('/lines');
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this line?">
        <Text>This cannot be undone.</Text>
        {error && <Text c="red">{error}</Text>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm delete">
            Delete
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

- [ ] **Step 2: Write the failing tests**

Create `frontend/components/DeleteLineButton.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { DeleteLineButton } from './DeleteLineButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
}));

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe('DeleteLineButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call DELETE until the confirmation modal is confirmed', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithProvider(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DELETEs the line and redirects to /lines on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithProvider(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/lines/custom-my-commute', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/lines'));
  });

  it('shows an error and does not redirect on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('custom line not found', { status: 404 }));

    renderWithProvider(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(screen.getByText('custom line not found')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npm --prefix frontend test -- DeleteLineButton`
Expected: FAIL — `./DeleteLineButton` module doesn't exist yet.

- [ ] **Step 4: Confirm the component from Step 1 is in place, then run the tests to verify they pass**

Run: `npm --prefix frontend test -- DeleteLineButton`
Expected: all 3 tests pass.

- [ ] **Step 5: Run the full frontend test suite and build**

Run: `npm --prefix frontend test && npm --prefix frontend run build`
Expected: all tests pass, build succeeds.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/DeleteLineButton.tsx frontend/components/DeleteLineButton.test.tsx
git commit -m "Add DeleteLineButton with confirmation modal"
```

---

### Task 6: Frontend — wire Edit/Delete into the line detail page

**Files:**
- Modify: `frontend/app/lines/[id]/page.tsx`

**Interfaces:**
- Consumes: `getCustomLine` (Task 2), `DeleteLineButton` (Task 5).

- [ ] **Step 1: Replace the full contents of `frontend/app/lines/[id]/page.tsx`**

```tsx
import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group, Button } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus, getCustomLine } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { worstStatus } from '@/lib/severity';

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let reports;
  try {
    reports = await getLineStatus([id], true);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const report = reports[0];
  const worst = worstStatus(report);

  // `getCustomLine` 404s for a catalogue-line id (the endpoint only ever
  // reads the `custom_lines` table) — that expected 404 is how this page
  // tells a custom line apart from a catalogue one, without needing a
  // second "is this custom" field on the status endpoint itself.
  let isCustom = true;
  try {
    await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      isCustom = false;
    } else {
      throw err;
    }
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>{report.name}</Title>
        <Group gap="sm">
          {isCustom && (
            <>
              {/* Plain `<Link>` wrapping `Button`, not `component={Link}`
                  on a Mantine polymorphic prop — this page is a Server
                  Component, and that pattern previously broke
                  `next build`'s Server/Client boundary check (see
                  LineStatusCard's fix). */}
              <Link href={`/lines/${id}/edit`} style={{ textDecoration: 'none' }}>
                <Button variant="outline" size="xs">Edit</Button>
              </Link>
              <DeleteLineButton id={id} />
            </>
          )}
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
      </Group>
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      <Link href={`/lines/${id}/history`} style={{ textDecoration: 'none' }}>
        <Text c="blue">View history</Text>
      </Link>
      <RepresentativeInfo statuses={report.lineStatuses} />
      <IssueList statuses={report.lineStatuses} />
    </Stack>
  );
}
```

- [ ] **Step 2: Verify build and existing tests**

Run: `npm --prefix frontend run build && npm --prefix frontend test`
Expected: build succeeds, all tests pass (this page has no existing test
file — see this plan's Global Constraints on why no new one is added
here; verified manually in Task 7).

- [ ] **Step 3: Commit**

```bash
git add frontend/app/lines/[id]/page.tsx
git commit -m "Show Edit/Delete on the line detail page for custom lines"
```

---

### Task 7: End-to-end verification against real data

**Files:** none (verification only).

- [ ] **Step 1: Bring up the stack**

Run: `docker compose up -d postgres api`, wait for `api` to report
healthy.

- [ ] **Step 2: Create a custom line to edit/delete against**

```bash
curl -s -X POST http://localhost:8080/public/lines \
  -H 'Content-Type: application/json' \
  -d '{"name":"E2E Test Line","operators":["SW"],"stations":["WOK","WAT"],"headcodePrefixes":[],"destinationCrsFilter":[]}'
```

Expected: `201`-equivalent JSON body with `"id":"custom-e2e-test-line"` (or
similar slug), `"source":"custom"`. Note the `id` for the next steps.

- [ ] **Step 3: Confirm `GET /lines/{id}` returns the full record**

```bash
curl -s http://localhost:8080/public/lines/custom-e2e-test-line
```

Expected: JSON with `id`, `name`, `operators`, `stations`,
`headcodePrefixes`, `destinationCrsFilter` (camelCase) — all matching
what was created in Step 2.

- [ ] **Step 4: Confirm `GET /lines/{id}` 404s for a catalogue line**

```bash
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/public/lines/<any-catalogue-line-id>
```

(Pick any id from `curl -s http://localhost:8080/public/lines | jq -r '.[] | select(.source == "catalogue") | .id' | head -1`.)
Expected: `404`.

- [ ] **Step 5: Confirm `PUT /lines/{id}` updates the line without changing its id**

```bash
curl -s -X PUT http://localhost:8080/public/lines/custom-e2e-test-line \
  -H 'Content-Type: application/json' \
  -d '{"name":"E2E Test Line (renamed)","operators":["SW"],"stations":["WOK","WAT","AON"],"headcodePrefixes":[],"destinationCrsFilter":[]}'
```

Expected: `200` with `"id":"custom-e2e-test-line"` (unchanged) and
`"name":"E2E Test Line (renamed)"`. Re-run Step 3's `GET` to confirm
`stations` now has 3 entries.

- [ ] **Step 6: Confirm `PUT /lines/{id}` rejects a catalogue id**

```bash
curl -s -o /dev/null -w "%{http_code}\n" -X PUT http://localhost:8080/public/lines/<catalogue-id-from-step-4> \
  -H 'Content-Type: application/json' \
  -d '{"name":"x","operators":[],"stations":["WOK","WAT"],"headcodePrefixes":[],"destinationCrsFilter":[]}'
```

Expected: `400`.

- [ ] **Step 7: Confirm `DELETE /lines/{id}` still works, then the id is gone**

```bash
curl -s -o /dev/null -w "%{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-e2e-test-line
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/public/lines/custom-e2e-test-line
```

Expected: `204` then `404`.

- [ ] **Step 8: Manual browser check of the frontend wiring**

Bring up the frontend too: `docker compose up -d frontend`. Create a
custom line at `http://localhost:3000/lines`, open its detail page, and
confirm:
- Edit and Delete buttons appear (custom line).
- Opening any catalogue line's detail page shows neither button.
- Edit navigates to `/lines/{id}/edit`, pre-filled with the line's
  current values, and "Save changes" updates it and returns to the
  detail page.
- Delete opens the confirmation modal, Cancel closes it with no request
  sent, and confirming deletes the line and redirects to `/lines`.

- [ ] **Step 9: Full workspace verification**

Run: `cargo test --workspace && npm --prefix frontend test && npm --prefix frontend run build`
Expected: everything passes — final gate before considering the feature
done.

- [ ] **Step 10: Bring the stack down**

```bash
docker compose down
```
