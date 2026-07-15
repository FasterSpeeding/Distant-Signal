# Last-updated indicators — design

## Purpose

Users currently have no way to tell how fresh the line-status data or the
underlying reference data (stations, TOCs, incidents feed) is. This adds:

1. A small "Updated Xm ago" indicator on every line status card, reflecting
   when the aggregator last computed that line's status.
2. An info icon in the nav bar giving the freshness of the three global data
   sources that feed the aggregator: stations reference data, TOC reference
   data, and the raw incidents feed.

## Background (what already exists)

- `line_status` (Postgres table) has a `computed_at` column, set by the
  aggregator (`crates/aggregator/src/queries.rs`) on every write. It's
  already selected and returned as `computedAt` on the history endpoint
  (`GET /Line/{id}/Status/{from}/to/{to}`, see `routes/line_status.rs:162`)
  but discarded (`LineStatusRow` doesn't carry it) on the three endpoints
  the frontend actually uses for cards: `/Line/Mode/{mode}/Status`,
  `/Line/{ids}/Status`, and `/StopPoint/{crs}/Disruption`.
- `queries::last_stations_fetch`/`last_tocs_fetch`/`last_incidents_fetch`
  (`crates/api/src/data/queries.rs`) already compute `MAX(fetched_at)` per
  table. They're currently only exposed via the *private*,
  poller-auth-gated GET counterparts in `routes/ingest.rs` (used for poller
  startup backoff, not by the frontend).
- The frontend has no existing relative-time/"time ago" component, but
  `dayjs` is already a dependency (unused in source so far).

## Backend changes

### 1. `computedAt` on the three main status endpoints

- `LineStatusRow` (`crates/api/src/data/queries.rs`) gains a
  `computed_at: chrono::DateTime<chrono::Utc>` field; `line_status_for_mode`
  and `line_status_for_ids` add `computed_at` to their `SELECT` list.
- `get_mode_status`, `get_line_status`, `get_stop_point_disruption`
  (`crates/api/src/routes/line_status.rs`) each inject
  `json["computedAt"] = Value::String(row.computed_at.to_rfc3339())` onto
  the rendered JSON, mirroring the existing pattern at line 162. This is
  done at the route layer (not by adding a field to `common::LineStatusReport`)
  since that struct is also constructed by the aggregator without a
  timestamp — keeping `computed_at` API-response-only avoids touching the
  aggregator or the shared `common` crate.
- For `/StopPoint/{crs}/Disruption`, which can return multiple synthetic
  reports per stop point line, use each source row's own `computed_at`.

### 2. New public data-freshness endpoint

- New `crates/api/src/routes/freshness.rs`, mounted in `public_router()`
  (`routes/mod.rs`) as `GET /public/freshness`.
- Reuses the existing `queries::last_stations_fetch`/`last_tocs_fetch`/
  `last_incidents_fetch` — no new query logic.
- Response shape:
  ```json
  { "stations": "2026-07-15T09:00:00Z", "tocs": null, "incidents": "2026-07-15T09:29:00Z" }
  ```
  (`null` when a table has never been populated — same semantics as the
  existing private `LastFetchedResponse`.)

## Frontend changes

### 3. `LastUpdated` component (new, reusable)

`components/LastUpdated.tsx` — a small client component:
`<LastUpdated timestamp={isoString} label="Updated" />` renders dimmed text
`"{label} Xm ago"`, with the exact time shown in a `Tooltip` on hover.

**Hydration safety.** A relative "time ago" string depends on `Date.now()`
at render time, so it cannot be computed identically during SSR and the
client's pre-hydration render — this is the same class of bug just fixed in
`components/ThemeToggle.tsx` (see the comment on that component).
`LastUpdated` uses the same `mounted` gate:

- Before mount (SSR output and the client's first render): show a fixed
  absolute time via `Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/London',
  dateStyle: 'medium', timeStyle: 'short' })` — deterministic regardless of
  server/client locale or timezone, so server and client agree.
- After mount (`useEffect` sets `mounted = true`): switch to a live
  relative string (`relativeTime(from, now)`, a pure exported function),
  recomputed every 30s via `setInterval`.
- The Tooltip always shows the same fixed absolute-time string, mount or
  not.

### 4. Per-line indicator

- `lib/types.ts`: add `computedAt: string` to `LineStatusReport`. Simplify
  `LineStatusHistoryEntry` from `extends LineStatusReport { computedAt: string }`
  to `export type LineStatusHistoryEntry = LineStatusReport;` (now
  redundant since the field lives on the base type).
- `components/LineStatusCard.tsx`: render
  `<LastUpdated timestamp={report.computedAt} />` under the line name.

### 5. Global freshness info icon

- `lib/api.ts`: add `getDataFreshness(): Promise<DataFreshness>` hitting
  `/public/freshness` with `next: { revalidate: 30 }` (matches the
  aggregation cadence, same convention as `getLineStatusForMode`).
- `lib/types.ts`: add
  `interface DataFreshness { stations: string | null; tocs: string | null; incidents: string | null }`.
- `app/layout.tsx` (Server Component) calls `getDataFreshness()` and passes
  the result into a new `components/DataFreshnessInfo.tsx` Client Component,
  placed in the nav `Group` next to `ThemeToggle` — following the same
  "Server Component fetches, passes props to Client Component" split
  documented in `LineDefinitionTooltip.tsx`'s comment.
- `DataFreshnessInfo` renders an ⓘ `ActionIcon` (same visual pattern as
  `LineDefinitionTooltip`) with a `Tooltip` listing three rows — Stations,
  TOCs, Incidents — each using `LastUpdated`, or "Never" text when `null`.

## Testing

- Rust: extend `render.rs`/`line_status.rs` tests asserting `computedAt`
  appears on all three main endpoints' JSON (not just history); a
  serialization test for the new freshness response's field names.
- Frontend:
  - Unit tests for the pure `relativeTime(from, to)` function (boundary
    cases: <1min, minutes, hours, days).
  - A `renderToString`-based regression test for `LastUpdated` (mirroring
    the one added for `ThemeToggle`) proving the pre-mount/SSR output is
    time-independent (fixed absolute time, not "Xm ago") regardless of
    when the test runs.
  - Component tests for `LineStatusCard` (shows the indicator) and
    `DataFreshnessInfo` (shows all three rows, handles `null`).

## Out of scope

- Station-samples freshness (not requested — only stations/TOCs/incidents).
- Live-updating without a page reload beyond the 30s relative-time tick
  (no websocket/polling for fresh `computedAt`/`freshness` values within a
  single page view — relies on Next's existing `revalidate: 30` + normal
  navigation/reload).
