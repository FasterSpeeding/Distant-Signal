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
- Operators — free-text `TagsInput`, not a multi-select against known ATOC
  codes: there's no backend endpoint enumerating operator codes, and adding
  one is out of scope here. Consistent with how the headcode-prefix/
  destination-CRS filters below are already treated as "narrow power-user
  knobs, not worth bespoke UI."
- Stations (ordered picker — add one at a time via the same search-by-CRS
  pattern as the existing `StationSearchForm`)
- Collapsed "Advanced" section: optional headcode-prefix / destination-CRS
  filters (`TagsInput`, same free-text treatment as operators)

Submits to `POST /public/lines` (sub-project 1). On success, navigate to
the new line's `/lines/{id}` page.

`POST /public/lines`'s request struct (`CreateLineRequest` in
`crates/api/src/routes/lines.rs`) was originally built without
`#[serde(rename_all = "camelCase")]`, so it silently expected snake_case
`headcode_prefixes`/`destination_crs_filter` — inconsistent with every
other JSON key this API uses. Fixed as part of this sub-project (before
this form became the endpoint's first real consumer) rather than baking a
snake_case workaround into the frontend.

## Browser-initiated writes: same-origin proxy

Client Components (the pin toggle, the custom-line form) run in the
browser and cannot read `API_BASE_URL` — Next.js only inlines
`NEXT_PUBLIC_`-prefixed env vars into the client bundle. A single
catch-all Route Handler, `frontend/app/api/[...path]/route.ts`, proxies
same-origin `/api/*` requests to `${API_BASE_URL}/public/*` server-side.
Client Components call this proxy directly (plain `fetch('/api/...')`),
not `lib/api.ts` (whose functions assume the server-only env var and are
used by Server Components for initial-render reads only). This also means
the `api` service's CORS policy never needs to allow POST/PUT/DELETE from
a browser origin — the browser only ever talks to the Next.js origin.

## Pinning stations

Add the pin/star toggle to the `/stations/{crs}` detail page header only.
The design originally also called for one on `/stations` search results,
but `StationSearchForm` has no results list — it's a direct CRS-code
lookup that navigates straight to `/stations/{crs}` on submit — so there
is nothing there to attach a toggle to.

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
