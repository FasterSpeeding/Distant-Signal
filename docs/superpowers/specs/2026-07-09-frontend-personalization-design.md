# Frontend Personalization — Design

Sub-project 2 of 3. Depends on sub-project 1
(`2026-07-09-custom-lines-and-blended-stats-design.md`) for the
`GET/POST/DELETE /public/lines` API. See also sub-project 3
(`2026-07-09-outage-page-redesign-design.md`).

## Goals

- National Rail lines become opt-in: the home page only shows lines the
  user has explicitly pinned, not every line by default.
- A "browse all lines" page lists every line (official + custom) briefly,
  lets the user pin/unpin, and is the entry point for creating a custom
  line.
- The home page also lists pinned stations.

## Non-goals

- No per-user scoping (single shared preference list, matches custom
  lines' unauthenticated model — see sub-project 1's Non-goals).

## Preferences storage

Two new tables, no FK to `custom_lines`/`stations` (official line ids
are compile-time TOML, not DB rows, so no single constraint can cover
both cases — the API silently drops pinned ids that no longer resolve to
a real line/station when serving `GET /public/preferences`):

```sql
pinned_lines (line_id text, pinned_at timestamptz)
pinned_stations (crs text, pinned_at timestamptz)
```

Deleting a custom line (`DELETE /public/lines/{id}`, sub-project 1)
cascades to remove its `pinned_lines` row in the same transaction.

## New API endpoints (`/public/preferences`, unauthenticated)

- `GET /public/preferences` → `{ pinnedLines: string[], pinnedStations: string[] }`
- `PUT /public/preferences/pinned-lines` — body `string[]`, replaces the
  whole list.
- `PUT /public/preferences/pinned-stations` — body `string[]`, replaces
  the whole list.

Replace-whole-list rather than per-item add/remove endpoints: the lists
are small and low-cardinality (personal use), so a toggle just PUTs the
updated array back rather than needing four endpoints
(add-line/remove-line/add-station/remove-station).

## Home page (`/`)

Two sections:

- **Your lines** — reuses the existing `LineStatusCard` grid
  (`components/LineStatusCard.tsx`), filtered to `pinnedLines` against
  `GET /Line/Mode/national-rail/Status` (already fetches every line's
  live status, no new endpoint needed here). Empty state: "You haven't
  pinned any lines yet" + link to `/lines`.
- **Your stations** — one row per pinned station, each showing a
  compact worst-severity indicator (from `GET /StopPoint/{crs}/Disruption`,
  already exists) and linking to `/stations/{crs}`. Same empty-state
  pattern, linking to `/stations`.

## All-lines browse page (`/lines`, new)

One row per line from `GET /public/lines`: name, operators, category, a
pin/star toggle (PUTs the updated `pinnedLines` array to
`/public/preferences/pinned-lines`), and a link into the existing
`/lines/{id}` detail page. Deliberately brief — no live status per row,
just identification + pin + navigate, per "lists all the lines briefly."

A "New custom line" button opens a form:
- Name (text)
- Operators (multi-select from known ATOC codes)
- Stations (ordered picker — add one at a time via the same search-by-CRS
  pattern as the existing `StationSearchForm`)
- Collapsed "Advanced" section: optional headcode-prefix / destination-CRS
  filters (plain text-array inputs — these are narrow power-user knobs,
  not worth bespoke UI)

Submits to `POST /public/lines` (sub-project 1). On success, navigate to
the new line's `/lines/{id}` page.

## Pinning stations

Add the same pin/star toggle to:
- `/stations` search results (`StationSearchForm`'s result list)
- `/stations/{crs}` detail page header

## Testing

- `lib/api.ts`: new client functions for the preferences and
  `/public/lines` endpoints, unit-tested the same way as existing
  `getLineStatus`/`getStopPointDisruption` (mocked `fetch`, per
  `lib/api.test.ts`'s existing pattern).
- Home page: server-component test (or integration-style render test)
  asserting pinned-only filtering and the empty-state fallback.
- All-lines page: pin toggle round-trips through the PUT endpoint;
  custom-line form validates required fields (name, ≥2 stations) before
  submit.
