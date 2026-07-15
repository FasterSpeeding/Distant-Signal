# Last-Updated Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a "last updated" timestamp on every line status card, and a nav-bar info icon reporting the freshness of the three global data sources (stations, TOCs, incidents feed).

**Architecture:** Backend: thread the already-stored `line_status.computed_at` timestamp through `render.rs::to_tfl_shape` (currently discarded on 3 of 4 status endpoints) as an explicit parameter, and add a new public `/public/freshness` endpoint that surfaces the existing (currently poller-only) `last_stations_fetch`/`last_tocs_fetch`/`last_incidents_fetch` queries. Frontend: a reusable `LastUpdated` client component that is hydration-safe (renders a fixed absolute time until mounted, then a live relative string) — the same `mounted`-gate pattern used to fix `ThemeToggle`'s hydration bug — used both on line cards and inside a new `DataFreshnessInfo` nav-bar tooltip.

**Tech Stack:** Rust/axum/sqlx (crates/api), Next.js App Router + Mantine + Vitest/Testing Library (frontend).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-15-last-updated-indicators-design.md`.
- Do not add a `computed_at`/`computedAt` field to `common::LineStatusReport` — it's constructed by the aggregator without a timestamp; keep this API-response-only, threaded as an explicit parameter into `render::to_tfl_shape`.
- Scope is stations/TOCs/incidents freshness only — station-samples is explicitly out of scope.
- Any frontend component rendering a value derived from `Date.now()`/locale/timezone must use the `mounted`-gate pattern (see `components/ThemeToggle.tsx`'s comment) to avoid SSR/CSR hydration mismatches.

---

### Task 1: Backend — thread `computed_at` through `render::to_tfl_shape`

**Files:**
- Modify: `crates/api/src/render.rs`
- Test: `crates/api/src/render.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `to_tfl_shape(report: &common::LineStatusReport, computed_at: chrono::DateTime<chrono::Utc>, detail: bool) -> serde_json::Value` (signature change — was `(report, detail)`, now takes `computed_at` as the 2nd positional arg). Sets `json["computedAt"]` to `computed_at.to_rfc3339()` unconditionally. Later tasks (Task 2) call this new signature.

- [ ] **Step 1: Write the failing test**

Add this test and a small helper to the existing `mod tests` block in `crates/api/src/render.rs` (the file already has `use chrono::Utc;` in that block — add `use chrono::{DateTime, TimeZone};` alongside it):

```rust
    fn sample_computed_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap()
    }

    #[test]
    fn renders_computed_at() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(json["computedAt"], "2026-07-15T09:00:00+00:00");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api renders_computed_at`
Expected: FAIL — compile error, `to_tfl_shape` takes 2 arguments but 3 were supplied (this drives the signature change in Step 3, which will also require fixing every other call site — that's fine, do it all in Step 3).

- [ ] **Step 3: Update `to_tfl_shape`'s signature and every call site in this file**

In `crates/api/src/render.rs`, change the import line and function:

```rust
use chrono::{DateTime, Utc};
use common::{LineStatus, LineStatusReport, Severity};
use serde_json::{Value, json};

pub fn to_tfl_shape(report: &LineStatusReport, computed_at: DateTime<Utc>, detail: bool) -> Value {
    json!({
        "$type": "NRStatus.LineStatusReport",
        "id": report.id,
        "name": report.name,
        "modeName": report.mode_name,
        "operators": report.operators,
        "computedAt": computed_at.to_rfc3339(),
        "lineStatuses": report.statuses.iter().map(|s| status_to_json(s, detail)).collect::<Vec<_>>(),
    })
}
```

Then update every existing test call site in the same file's `mod tests` block to pass `sample_computed_at()` as the new 2nd argument — there are 6 existing calls, all of the shape `to_tfl_shape(&report, false)` or `to_tfl_shape(&report, true)`; change each to `to_tfl_shape(&report, sample_computed_at(), false)` / `to_tfl_shape(&report, sample_computed_at(), true)`. (Do not change `sample_report()` itself — `computed_at` stays a separate parameter, not a field on `LineStatusReport`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api render`
Expected: PASS — all `render::tests::*` tests pass, including the new `renders_computed_at`. (This will still show compile errors from `crates/api/src/routes/line_status.rs`, which calls `to_tfl_shape` with the old 2-arg signature — that's expected and fixed in Task 2. If you want a clean compile at this step, `cargo test -p api render 2>&1 | grep -A3 "test result"` isolates the render-module result from the rest of the crate's errors; otherwise proceed straight to Task 2, which must land in the same commit as this one to keep the crate compiling.)

- [ ] **Step 5: Commit**

Hold off on committing — Task 2 must land in the same commit since Step 3 above leaves `crates/api` non-compiling on its own (line_status.rs still calls the old signature). Commit once Task 2 is done.

---

### Task 2: Backend — pass `computed_at` from the DB through the route handlers

**Files:**
- Modify: `crates/api/src/data/queries.rs` (`LineStatusRow`, `row_to_report`, `line_status_for_mode`, `line_status_for_ids`)
- Modify: `crates/api/src/routes/line_status.rs` (`get_mode_status`, `get_line_status`, `get_stop_point_disruption`, `get_line_status_history`)

**Interfaces:**
- Consumes: `to_tfl_shape(report, computed_at, detail)` from Task 1.
- Produces: `queries::LineStatusRow` gains a public `computed_at: chrono::DateTime<chrono::Utc>` field, populated by `line_status_for_mode`/`line_status_for_ids`. All three of `/Line/Mode/{mode}/Status`, `/Line/{ids}/Status`, `/StopPoint/{crs}/Disruption` now include `computedAt` in their JSON response, matching what `/Line/{id}/Status/{from}/to/{to}` already did.

- [ ] **Step 1: Update `LineStatusRow` and the two query functions**

In `crates/api/src/data/queries.rs`, change the struct, its row-mapping function, and both `SELECT` statements:

```rust
/// One row from `line_status`, deserialized into the shape `render.rs`
/// consumes.
pub struct LineStatusRow {
    pub id: String,
    pub name: String,
    pub mode_name: String,
    pub operators: Vec<String>,
    pub statuses: Vec<common::LineStatus>,
    pub computed_at: chrono::DateTime<chrono::Utc>,
}

fn row_to_report(row: sqlx::postgres::PgRow) -> Result<LineStatusRow> {
    use sqlx::Row;
    let statuses_json: serde_json::Value = row.try_get("statuses")?;
    Ok(LineStatusRow {
        id: row.try_get("line_id")?,
        name: row.try_get("name")?,
        mode_name: row.try_get("mode_name")?,
        operators: row.try_get("operators")?,
        statuses: serde_json::from_value(statuses_json)?,
        computed_at: row.try_get("computed_at")?,
    })
}

pub async fn line_status_for_mode(pool: &PgPool, mode: &str) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE mode_name = $1",
    )
    .bind(mode)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_report).collect()
}

pub async fn line_status_for_ids(pool: &PgPool, ids: &[String]) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE line_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_report).collect()
}
```

(Leave `line_status_history_for_range` untouched — it already selects and returns `computed_at` separately.)

- [ ] **Step 2: Update the four route handlers in `crates/api/src/routes/line_status.rs`**

`get_mode_status`:

```rust
async fn get_mode_status(
    State(app): State<App>,
    Path(mode): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if mode != "national-rail" {
        return Err((StatusCode::BAD_REQUEST, format!("unsupported mode: {mode}")));
    }

    let rows = queries::line_status_for_mode(&app.database, &mode)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let computed_at = row.computed_at;
                let report = to_report(row);
                to_tfl_shape(&report, computed_at, query.detail)
            })
            .collect(),
    ))
}
```

`get_line_status` — same reshaping:

```rust
async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();

    let rows = queries::line_status_for_ids(&app.database, &ids)
        .await
        .map_err(internal_error)?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no matching line(s): {}", ids.join(","))));
    }

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let computed_at = row.computed_at;
                let report = to_report(row);
                to_tfl_shape(&report, computed_at, query.detail)
            })
            .collect(),
    ))
}
```

`get_stop_point_disruption` — capture `computed_at` before `row`'s fields are moved into the synthetic report:

```rust
async fn get_stop_point_disruption(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let matching_line_ids: Vec<String> = app
        .config
        .lines
        .iter()
        .filter(|line| line.has_station(&crs))
        .map(|line| line.id.clone())
        .collect();

    if matching_line_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let rows = queries::line_status_for_ids(&app.database, &matching_line_ids)
        .await
        .map_err(internal_error)?;

    let disruptions: Vec<Value> = rows
        .into_iter()
        .flat_map(|row| {
            let computed_at = row.computed_at;
            let statuses: Vec<LineStatus> = row
                .statuses
                .into_iter()
                .filter(|s| s.severity != Severity::GoodService)
                .collect();
            let report = LineStatusReport {
                id: row.id,
                name: row.name,
                mode_name: row.mode_name,
                operators: row.operators,
                statuses,
            };
            if report.statuses.is_empty() {
                None
            } else {
                Some(to_tfl_shape(&report, computed_at, true))
            }
        })
        .collect();

    Ok(Json(disruptions))
}
```

`get_line_status_history` — simplify now that `to_tfl_shape` takes `computed_at` directly (drop the old manual `json["computedAt"] = ...` line):

```rust
async fn get_line_status_history(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let history = queries::line_status_history_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        history
            .into_iter()
            .map(|(computed_at, statuses)| {
                let report = LineStatusReport {
                    id: id.clone(),
                    name: String::new(),
                    mode_name: String::new(),
                    operators: vec![],
                    statuses,
                };
                to_tfl_shape(&report, computed_at, true)
            })
            .collect(),
    ))
}
```

- [ ] **Step 3: Run the full API crate test suite**

Run: `cargo test -p api`
Expected: PASS — all tests compile and pass, including `render::tests::renders_computed_at` from Task 1.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/render.rs crates/api/src/data/queries.rs crates/api/src/routes/line_status.rs
git commit -m "Include computedAt on all line-status API endpoints, not just history"
```

---

### Task 3: Backend — new public `/public/freshness` endpoint

**Files:**
- Create: `crates/api/src/routes/freshness.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Consumes: `queries::last_stations_fetch`/`last_tocs_fetch`/`last_incidents_fetch` (`crates/api/src/data/queries.rs`, already exist, unchanged).
- Produces: `GET /public/freshness` → `{ "stations": "<rfc3339>" | null, "tocs": ..., "incidents": ... }`.

- [ ] **Step 1: Write the failing test**

Create `crates/api/src/routes/freshness.rs`:

```rust
//! `/public/freshness`: how fresh the three data sources feeding the
//! aggregator are (stations reference data, TOC reference data, the raw
//! incidents feed). Unauthenticated, read-only — same `public_router()`
//! pattern as `reference.rs`. Reuses the same `last_*_fetch` queries the
//! private poller-startup endpoints already call
//! (`crates/api/src/routes/ingest.rs`) — this is a public read of the same
//! underlying data, just aimed at the frontend instead of poller backoff.
//! Station-samples is deliberately omitted: it's per-station polling data,
//! not one of the three sources this endpoint reports on.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/freshness", axum::routing::get(get_freshness))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DataFreshness {
    pub stations: Option<DateTime<Utc>>,
    pub tocs: Option<DateTime<Utc>>,
    pub incidents: Option<DateTime<Utc>>,
}

async fn get_freshness(State(app): State<App>) -> Result<Json<DataFreshness>, (StatusCode, String)> {
    let stations = queries::last_stations_fetch(&app.database).await.map_err(internal_error)?;
    let tocs = queries::last_tocs_fetch(&app.database).await.map_err(internal_error)?;
    let incidents = queries::last_incidents_fetch(&app.database).await.map_err(internal_error)?;
    Ok(Json(DataFreshness { stations, tocs, incidents }))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "data freshness query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn serializes_missing_data_as_null() {
        let freshness = DataFreshness { stations: None, tocs: None, incidents: None };
        let json = serde_json::to_value(&freshness).unwrap();
        assert!(json["stations"].is_null());
        assert!(json["tocs"].is_null());
        assert!(json["incidents"].is_null());
    }

    #[test]
    fn round_trips_a_present_timestamp() {
        let ts = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        let freshness = DataFreshness { stations: Some(ts), tocs: None, incidents: None };
        let json = serde_json::to_value(&freshness).unwrap();
        let roundtripped: DateTime<Utc> = json["stations"].as_str().unwrap().parse().unwrap();
        assert_eq!(roundtripped, ts);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api freshness`
Expected: FAIL — `crates/api/src/routes/freshness.rs` isn't wired into `mod.rs` yet, so `cargo test` won't even see this module. (If your toolchain reports "no tests ran" rather than a hard failure, that's the same signal — proceed to Step 3.)

- [ ] **Step 3: Wire the module into `routes/mod.rs` and `public_router()`**

In `crates/api/src/routes/mod.rs`, add `pub mod freshness;` to the `pub mod` list (alphabetical, between `health` and `ingest`... actually `ingest` isn't in this list, it's `health, ingest, line_status, lines, preferences, reference, samples` per the module — insert alphabetically):

```rust
pub mod freshness;
pub mod health;
pub mod ingest;
pub mod line_status;
pub mod lines;
pub mod preferences;
pub mod reference;
pub mod samples;
```

And merge its router into `public_router()`:

```rust
pub fn public_router() -> Router {
    Router::new()
        .merge(health::router())
        .merge(freshness::router())
        .merge(lines::router())
        .merge(preferences::router())
        .merge(reference::router())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api freshness`
Expected: PASS — both `freshness::tests::*` tests pass.

- [ ] **Step 5: Run the full API crate test suite**

Run: `cargo test -p api`
Expected: PASS, no regressions.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/freshness.rs crates/api/src/routes/mod.rs
git commit -m "Add public /public/freshness endpoint for stations/TOCs/incidents data freshness"
```

---

### Task 4: Frontend — `relativeTime` pure function

**Files:**
- Create: `frontend/lib/relativeTime.ts`
- Test: `frontend/lib/relativeTime.test.ts`

**Interfaces:**
- Produces: `relativeTime(from: Date, to: Date): string` — used by Task 5's `LastUpdated` component.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/relativeTime.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { relativeTime } from './relativeTime';

describe('relativeTime', () => {
  it('returns "just now" for under a minute', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T09:00:30Z');
    expect(relativeTime(from, to)).toBe('just now');
  });

  it('returns whole minutes under an hour', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T09:02:30Z');
    expect(relativeTime(from, to)).toBe('2m ago');
  });

  it('returns whole hours under a day', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T12:00:00Z');
    expect(relativeTime(from, to)).toBe('3h ago');
  });

  it('returns whole days at a day or more', () => {
    const from = new Date('2026-07-13T09:00:00Z');
    const to = new Date('2026-07-15T09:00:00Z');
    expect(relativeTime(from, to)).toBe('2d ago');
  });

  it('clamps a future "from" (clock skew) to "just now" instead of a negative value', () => {
    const from = new Date('2026-07-15T09:05:00Z');
    const to = new Date('2026-07-15T09:00:00Z');
    expect(relativeTime(from, to)).toBe('just now');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/relativeTime.test.ts`
Expected: FAIL with "Cannot find module './relativeTime'".

- [ ] **Step 3: Write the implementation**

Create `frontend/lib/relativeTime.ts`:

```ts
/** Renders the gap between two instants as "just now" / "Nm ago" / "Nh
 * ago" / "Nd ago". A negative gap (clock skew — `from` in the future
 * relative to `to`) is clamped to zero rather than shown as e.g. "-2m
 * ago", matching the same defensive clamp used for poller poll-interval
 * math (see `crates/common/src/ingest.rs`'s `duration_until_next_poll`). */
export function relativeTime(from: Date, to: Date): string {
  const diffMinutes = Math.max(0, Math.floor((to.getTime() - from.getTime()) / 60_000));

  if (diffMinutes < 1) return 'just now';
  if (diffMinutes < 60) return `${diffMinutes}m ago`;

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) return `${diffHours}h ago`;

  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run lib/relativeTime.test.ts`
Expected: PASS — all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/relativeTime.ts frontend/lib/relativeTime.test.ts
git commit -m "Add relativeTime pure function for last-updated indicators"
```

---

### Task 5: Frontend — `LastUpdated` component

**Files:**
- Create: `frontend/components/LastUpdated.tsx`
- Test: `frontend/components/LastUpdated.test.tsx`

**Interfaces:**
- Consumes: `relativeTime(from, to)` from Task 4.
- Produces: `<LastUpdated timestamp={isoString} label="Updated" withTooltip={true} />` — a Mantine `Text` (optionally `Tooltip`-wrapped) showing `"{label} {relative-or-absolute time}"`. Used by Task 6 (`LineStatusCard`, `withTooltip` defaulted true) and Task 8 (`DataFreshnessInfo`, `withTooltip={false}`).

- [ ] **Step 1: Write the failing test**

Create `frontend/components/LastUpdated.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { LastUpdated } from './LastUpdated';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe('LastUpdated', () => {
  it('server-rendered output shows a fixed absolute time, never a relative one (avoids hydration mismatch)', () => {
    // Mirrors the ThemeToggle regression test: renderToString never runs
    // effects, so this is exactly what the server sends down. It must not
    // depend on "now" (real wall-clock time at test-run time), or it can
    // never match the client's own pre-mount render.
    const html = renderToString(
      <MantineProvider>
        <LastUpdated timestamp="2026-07-15T09:00:00Z" />
      </MantineProvider>,
    );
    expect(html).toContain('Updated');
    expect(html).not.toMatch(/\d+[mhd] ago|just now/);
  });

  it('shows a relative time once mounted', () => {
    renderWithProvider(<LastUpdated timestamp="2026-07-15T09:00:00Z" />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });

  it('supports a custom label', () => {
    renderWithProvider(<LastUpdated timestamp="2026-07-15T09:00:00Z" label="Stations:" />);
    expect(screen.getByText(/^Stations:/)).toBeInTheDocument();
  });

  it('shows the exact time in a tooltip on hover by default', async () => {
    renderWithProvider(<LastUpdated timestamp="2026-07-15T09:00:00Z" />);
    // Mantine's Tooltip doesn't mount its floating content into the DOM
    // at all until actually triggered — hover it first (same pattern as
    // LineDefinitionTooltip.test.tsx).
    fireEvent.mouseEnter(screen.getByText(/^Updated/));
    expect(await screen.findByRole('tooltip', { hidden: true })).toBeInTheDocument();
  });

  it('does not show a tooltip on hover when withTooltip is false', () => {
    renderWithProvider(<LastUpdated timestamp="2026-07-15T09:00:00Z" withTooltip={false} />);
    fireEvent.mouseEnter(screen.getByText(/^Updated/));
    expect(screen.queryByRole('tooltip', { hidden: true })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/LastUpdated.test.tsx`
Expected: FAIL with "Cannot find module './LastUpdated'".

- [ ] **Step 3: Write the implementation**

Create `frontend/components/LastUpdated.tsx`:

```tsx
'use client';

import { useEffect, useState } from 'react';
import { Text, Tooltip } from '@mantine/core';
import { relativeTime } from '@/lib/relativeTime';

const EXACT_TIME_FORMAT = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
  timeStyle: 'short',
});

const RELATIVE_TIME_TICK_MS = 30_000;

/** Shows "{label} Xm ago", with the exact time in a tooltip (or plain,
 * with `withTooltip={false}`, for reuse inside another tooltip's content —
 * see `DataFreshnessInfo`, which nests three of these inside one outer
 * `Tooltip` rather than each showing its own).
 *
 * A relative "time ago" string depends on `Date.now()` at render time, so
 * it can't be computed identically during SSR and the client's
 * pre-hydration render — the same class of bug fixed in `ThemeToggle` (see
 * that component's comment). Before mount, this always shows a fixed
 * absolute time (deterministic regardless of server/client locale or
 * timezone); only after the `useEffect` below fires does it switch to the
 * live relative string, re-computed every 30s. */
export function LastUpdated({
  timestamp,
  label = 'Updated',
  withTooltip = true,
}: {
  timestamp: string;
  label?: string;
  withTooltip?: boolean;
}) {
  const date = new Date(timestamp);
  const exact = EXACT_TIME_FORMAT.format(date);
  const [now, setNow] = useState<Date | null>(null);

  useEffect(() => {
    setNow(new Date());
    const id = setInterval(() => setNow(new Date()), RELATIVE_TIME_TICK_MS);
    return () => clearInterval(id);
  }, []);

  const displayed = now === null ? exact : relativeTime(date, now);
  const text = (
    <Text size="xs" c="dimmed">
      {label} {displayed}
    </Text>
  );

  return withTooltip ? <Tooltip label={exact}>{text}</Tooltip> : text;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/LastUpdated.test.tsx`
Expected: PASS — all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/LastUpdated.tsx frontend/components/LastUpdated.test.tsx
git commit -m "Add hydration-safe LastUpdated component"
```

---

### Task 6: Frontend — per-line indicator on `LineStatusCard`

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/components/LineStatusCard.tsx`
- Modify: `frontend/components/LineStatusCard.test.tsx`
- Modify: `frontend/lib/severity.test.ts`

**Interfaces:**
- Consumes: `LastUpdated` from Task 5.
- Produces: `LineStatusReport.computedAt: string` (required field, now present on every report the API returns). `LineStatusHistoryEntry` becomes a type alias of `LineStatusReport`.

- [ ] **Step 1: Update the type definitions**

In `frontend/lib/types.ts`, add `computedAt` to `LineStatusReport` and simplify `LineStatusHistoryEntry`:

```ts
export interface LineStatusReport {
  $type: string;
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
  computedAt: string;
}

export type LineStatusHistoryEntry = LineStatusReport;
```

(This replaces the existing `LineStatusReport` interface, which currently ends at `lineStatuses: LineStatus[];`, and replaces the existing `export interface LineStatusHistoryEntry extends LineStatusReport { computedAt: string; }`.)

- [ ] **Step 2: Fix the two existing typed fixtures this breaks**

In `frontend/lib/severity.test.ts`, add `computedAt` to `baseReport`:

```ts
  const baseReport: LineStatusReport = {
    $type: 'NRStatus.LineStatusReport',
    id: 'wcml',
    name: 'West Coast Main Line',
    modeName: 'national-rail',
    operators: ['AW'],
    lineStatuses: [],
    computedAt: '2026-07-15T09:00:00Z',
  };
```

In `frontend/components/LineStatusCard.test.tsx`, add `computedAt` to `report`:

```ts
const report: LineStatusReport = {
  $type: 'NRStatus.LineStatusReport',
  id: 'wcml',
  name: 'West Coast Main Line',
  modeName: 'national-rail',
  operators: ['AW'],
  computedAt: '2026-07-15T09:00:00Z',
  lineStatuses: [
    {
      statusSeverity: 9,
      statusSeverityDescription: 'Minor Delays',
      reason: 'Signal failure',
      dataQuality: 'knowledgebase',
      validityPeriods: [{ fromDate: '2026-07-07T10:00:00Z', toDate: null, isNow: true }],
    },
  ],
};
```

- [ ] **Step 3: Write the failing test for the card itself**

Add this test to `frontend/components/LineStatusCard.test.tsx`'s `describe('LineStatusCard', ...)` block:

```tsx
  it('renders a last-updated indicator', () => {
    renderWithProvider(<LineStatusCard report={report} />);
    expect(screen.getByText(/Updated (just now|\d+[mhd] ago)/)).toBeInTheDocument();
  });
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/LineStatusCard.test.tsx`
Expected: FAIL — "renders a last-updated indicator" fails (`LastUpdated` text not present yet); the other pre-existing tests in this file still pass since they don't reference `computedAt`.

- [ ] **Step 5: Render `LastUpdated` in `LineStatusCard`**

Update `frontend/components/LineStatusCard.tsx`:

```tsx
'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

export function LineStatusCard({ report }: { report: LineStatusReport }) {
  const worst = worstStatus(report);
  return (
    <Card withBorder shadow="sm" padding="lg" component={Link} href={`/lines/${report.id}`}>
      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={600}>{report.name}</Text>
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
        <Text size="sm" c="dimmed">
          {worst.reason}
        </Text>
        <LastUpdated timestamp={report.computedAt} />
      </Stack>
    </Card>
  );
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/LineStatusCard.test.tsx lib/severity.test.ts`
Expected: PASS — all tests in both files pass.

- [ ] **Step 7: Run the full frontend test suite and typecheck**

Run: `cd frontend && npx vitest run && npx tsc --noEmit`
Expected: All test files pass. `tsc --noEmit` shows only the pre-existing, unrelated `LineDefinitionTooltip.test.tsx` `hidden` option errors (present before this plan started) — no new errors from files this plan touches.

- [ ] **Step 8: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/severity.test.ts frontend/components/LineStatusCard.tsx frontend/components/LineStatusCard.test.tsx
git commit -m "Show a last-updated indicator on every line status card"
```

---

### Task 7: Frontend — `getDataFreshness` API client function

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `DataFreshness { stations: string | null; tocs: string | null; incidents: string | null }` (type), `getDataFreshness(): Promise<DataFreshness>` (fetches `${API_BASE_URL}/public/freshness`). Used by Task 9 (`app/layout.tsx`).

- [ ] **Step 1: Write the failing test**

Add to `frontend/lib/api.test.ts` (inside the existing `describe('api client', ...)` block, alongside the other `getX fetches the correct URL` tests):

```ts
  it('getDataFreshness fetches the correct URL', async () => {
    await getDataFreshness();
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/freshness',
      expect.objectContaining({ next: { revalidate: 30 } }),
    );
  });
```

And add `getDataFreshness` to the existing import block at the top of the file:

```ts
import {
  getLineStatusForMode,
  getLineStatus,
  getStopPointDisruption,
  getLineStatusHistory,
  getPreferences,
  getAllLines,
  getCustomLine,
  getLineDefinition,
  getDataFreshness,
  ApiNotFoundError,
} from './api';
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/api.test.ts`
Expected: FAIL — `getDataFreshness` isn't exported from `./api` yet.

- [ ] **Step 3: Add the type and the function**

In `frontend/lib/types.ts`, add:

```ts
export interface DataFreshness {
  stations: string | null;
  tocs: string | null;
  incidents: string | null;
}
```

In `frontend/lib/api.ts`, add `DataFreshness` to the type import and add the function at the end of the file:

```ts
import type {
  LineStatusReport,
  LineStatusHistoryEntry,
  Preferences,
  LineSummary,
  CustomLineDetail,
  LineDefinitionSummary,
  DataFreshness,
} from './types';
```

```ts
export async function getDataFreshness(): Promise<DataFreshness> {
  return fetchJson<DataFreshness>(`${baseUrl()}/public/freshness`, {
    next: { revalidate: 30 },
  });
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run lib/api.test.ts`
Expected: PASS — all tests in the file pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add getDataFreshness API client function"
```

---

### Task 8: Frontend — `DataFreshnessInfo` nav popup

**Files:**
- Create: `frontend/components/DataFreshnessInfo.tsx`
- Test: `frontend/components/DataFreshnessInfo.test.tsx`

**Interfaces:**
- Consumes: `LastUpdated` from Task 5, `DataFreshness` type from Task 7.
- Produces: `<DataFreshnessInfo freshness={dataFreshness} />` — an info `ActionIcon` with a `Tooltip` listing Stations/TOCs/Incidents freshness. Used by Task 9 (`app/layout.tsx`).

- [ ] **Step 1: Write the failing test**

Create `frontend/components/DataFreshnessInfo.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { DataFreshnessInfo } from './DataFreshnessInfo';
import type { DataFreshness } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const freshness: DataFreshness = {
  stations: '2026-07-15T09:00:00Z',
  tocs: '2026-07-15T08:00:00Z',
  incidents: null,
};

describe('DataFreshnessInfo', () => {
  it('renders an info icon', () => {
    renderWithProvider(<DataFreshnessInfo freshness={freshness} />);
    expect(screen.getByRole('button', { name: 'Data freshness' })).toBeInTheDocument();
  });

  it('shows a last-updated row for each present timestamp', async () => {
    renderWithProvider(<DataFreshnessInfo freshness={freshness} />);
    // Mantine's Tooltip doesn't mount its floating content into the DOM
    // at all until actually triggered — hover it first (same pattern as
    // LineDefinitionTooltip.test.tsx).
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Stations:/, { hidden: true })).toBeInTheDocument();
    expect(screen.getByText(/^TOCs:/, { hidden: true })).toBeInTheDocument();
  });

  it('shows "never fetched" for a null timestamp', async () => {
    renderWithProvider(<DataFreshnessInfo freshness={freshness} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^Incidents: never fetched/, { hidden: true })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/DataFreshnessInfo.test.tsx`
Expected: FAIL with "Cannot find module './DataFreshnessInfo'".

- [ ] **Step 3: Write the implementation**

Create `frontend/components/DataFreshnessInfo.tsx`:

```tsx
'use client';

import { ActionIcon, Stack, Text, Tooltip } from '@mantine/core';
import { LastUpdated } from './LastUpdated';
import type { DataFreshness } from '@/lib/types';

function freshnessRow(label: string, timestamp: string | null) {
  if (timestamp === null) {
    return (
      <Text size="xs" c="dimmed" key={label}>
        {label}: never fetched
      </Text>
    );
  }
  return <LastUpdated key={label} timestamp={timestamp} label={`${label}:`} withTooltip={false} />;
}

/** Nav-bar info icon for the freshness of the three data sources feeding
 * the aggregator (as opposed to `LastUpdated` on each line card, which
 * shows when that line's own status was last computed). Same
 * `ActionIcon` + `Tooltip` pattern as `LineDefinitionTooltip`. Each row
 * reuses `LastUpdated` with `withTooltip={false}` — nesting a
 * `Tooltip`-wrapped element inside this outer `Tooltip`'s own `label`
 * wouldn't be hoverable (the outer tooltip closes as the pointer leaves
 * the icon), so only the outer tooltip shows on hover here. */
export function DataFreshnessInfo({ freshness }: { freshness: DataFreshness }) {
  return (
    <Tooltip
      label={
        <Stack gap={2}>
          {freshnessRow('Stations', freshness.stations)}
          {freshnessRow('TOCs', freshness.tocs)}
          {freshnessRow('Incidents', freshness.incidents)}
        </Stack>
      }
      multiline
      maw={280}
    >
      <ActionIcon variant="subtle" aria-label="Data freshness">
        ⓘ
      </ActionIcon>
    </Tooltip>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/DataFreshnessInfo.test.tsx`
Expected: PASS — all 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/DataFreshnessInfo.tsx frontend/components/DataFreshnessInfo.test.tsx
git commit -m "Add DataFreshnessInfo nav popup"
```

---

### Task 9: Frontend — wire `DataFreshnessInfo` into the nav bar

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Consumes: `getDataFreshness` (Task 7), `DataFreshnessInfo` (Task 8).

- [ ] **Step 1: Update `app/layout.tsx`**

```tsx
import '@/app/globals.css';
import { MantineProvider, ColorSchemeScript, mantineHtmlProps, Group, Text } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { ThemeToggle } from '@/components/ThemeToggle';
import { DataFreshnessInfo } from '@/components/DataFreshnessInfo';
import { getDataFreshness } from '@/lib/api';

export const metadata: Metadata = {
  title: 'National Rail Status',
  description: 'Line status for UK National Rail, TfL-style.',
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // A root layout has no route-level `error.tsx` boundary (that only
  // catches errors in child segments), so an uncaught fetch failure here
  // would take down every page rather than just one — fall back to an
  // all-"never fetched" state instead.
  const freshness = await getDataFreshness().catch(() => ({
    stations: null,
    tocs: null,
    incidents: null,
  }));

  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <MantineProvider defaultColorScheme="auto">
          <Group
            component="nav"
            justify="space-between"
            p="md"
            style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}
          >
            {/* Plain `<Link>` wrapping Mantine's `Text`, rather than
                `component={Link}` on a Mantine polymorphic prop: this file
                is a Server Component, and passing the `Link` component
                reference into a Mantine `component` prop from a Server
                Component previously broke `next build`'s Server/Client
                boundary serialization check (see LineStatusCard fix).
                `ThemeToggle` below doesn't hit this: it's imported and
                rendered as a plain JSX element (a Client Component child
                of this Server Component), not passed as a value into a
                Mantine `component` prop — a different, safe pattern. */}
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
              <DataFreshnessInfo freshness={freshness} />
              <ThemeToggle />
            </Group>
          </Group>
          {children}
        </MantineProvider>
      </body>
    </html>
  );
}
```

- [ ] **Step 2: Run the full frontend test suite and typecheck**

Run: `cd frontend && npx vitest run && npx tsc --noEmit`
Expected: All test files pass (same pre-existing `LineDefinitionTooltip.test.tsx` typecheck errors as before, unrelated to this plan — no new ones).

- [ ] **Step 3: Manually verify in the browser**

Run: `docker compose --profile dev up -d` (the repo's single `docker-compose.yml` at the repo root gates its dev-profile services — `api-dev`, `aggregator-dev`, `frontend-dev`, etc. — behind `profiles: ["dev"]`) and load `http://localhost:3000`. Confirm:
- Each line card on the dashboard and `/lines` shows a small "Updated Xm ago" line, and hovering it shows the exact time.
- The nav bar shows a new ⓘ icon next to the theme toggle; hovering it lists Stations/TOCs/Incidents freshness (or "never fetched" if the corresponding poller hasn't run yet in this dev environment).
- No hydration-mismatch warnings appear in the browser console on initial load.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Show global data-freshness info icon in the nav bar"
```

---

### Task 10: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions anywhere in the workspace.

- [ ] **Step 2: Run the full frontend test suite and typecheck**

Run: `cd frontend && npx vitest run && npx tsc --noEmit`
Expected: PASS, same pre-existing unrelated `LineDefinitionTooltip.test.tsx` typecheck errors as at the start of this plan, no new ones.

- [ ] **Step 3: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree (everything committed task-by-task above).
