# Live Departure Picker for `TrackTrainForm` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Task 1 is backend, lands first. Task 2 is frontend, depends on Task
> 1's route being live** — do not start Task 2 against a route that
> hasn't shipped; there is no mock/stub layer for a route that doesn't
> exist server-side yet (`frontend/components/TrackTrainForm.test.tsx`
> mocks `global.fetch` directly, so it can be written against Task 1's
> *documented* wire shape without the route running, but the manual smoke
> check in Task 3 needs the real route deployed).

**Goal:** implement
`docs/superpowers/specs/2026-09-03-trip-search-design.md` end to end — a
new `GET /public/stations/{crs}/departures` endpoint returning the raw,
already-stored `station_samples` departure board for a CRS (no new table,
no new poller, no CIF parsing), plus an inline picker on the existing
`/track` form that lets a user browse today's live departures from a
sampled origin station and click one to auto-fill Destination, Operator,
and Scheduled departure. Every station outside `poller-ldbws`'s current
~286-station scope keeps today's unchanged manual-entry form.

**Scale note, stated plainly per this plan's own scoping brief:** this is
a genuinely small slice — one new read-only route (a straight pass-through
of data this app already collects and stores), one new frontend picker on
an existing form, zero schema changes, zero new services. It is
intentionally **not** the CIF-schedule-based "trip search" the earlier
research document's Part 2 explored (that remains out of scope, sized at
roughly a month of work, and is not what this plan builds — see the
design doc's "Re-examining..." section for why). Scoped to land in a
single small PR.

**Architecture:** one new route file (`crates/api/src/routes/departures.rs`),
one new extracted JSON helper in `crates/api/src/render.rs`
(`station_departure_json`), one route registration in
`crates/api/src/routes/mod.rs`, and one new picker block plus supporting
state/effect in `frontend/components/TrackTrainForm.tsx`. No other files
change.

**Tech Stack:** Rust (axum, sqlx — reuses the existing
`queries::latest_station_sample`, no new query), Next.js 16 App Router +
TypeScript, Vitest 2 + Testing Library (`TrackTrainForm.test.tsx`'s
existing `global.fetch` mock pattern).

**Design doc:**
`docs/superpowers/specs/2026-09-03-trip-search-design.md` — its Decisions
section is authoritative for every type/route/wire shape below; this plan
does not repeat the reasoning, only the concrete steps.

---

## Non-goals

- **No CIF SCHEDULE-feed parsing, STP-overlay resolution, or new
  schedules/calling-points schema.** Re-examined and reconfirmed
  out of scope by the design doc; not touched by this plan at all.
- **No broadening of `poller-ldbws`'s polling scope.** Stays scoped to
  whichever CRS codes are already in `station_samples` today.
- **No new database table, migration, or write path anywhere.** Every
  task below reads `station_samples` at request time via the existing
  `latest_station_sample`; nothing is written by this plan.
- **No change to `TrackPinRequest`, `validate_pin`, or `POST
  /Train/track`.** The picker only fills the existing form's existing
  state; submission is unchanged.
- **No standalone departure-board page/component on `/stations/[crs]`.**
  Named as plausible future reuse of the same route in the design doc,
  not built here.
- **No headcode search, future-date browsing, or pagination.** Not
  buildable from this data source / not needed for this slice — see
  design doc's Explicitly out of scope.
- **No sorting of `station_samples.departures`.** Preserved in storage
  order.

## Global Constraints

- **Testing:** Rust — `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests), plus
  `DATABASE_URL=<url> cargo test -p api -- --ignored --test-threads=1`
  for the DB-backed tests this plan adds (mirrors
  `.github/workflows/ci.yml`'s existing CI invocation for this crate's
  DB tests). Frontend — `npm test` (`frontend/package.json`'s `"test":
  "vitest run"`) and `npm run build` (`next build`) from `frontend/`.
- **File scope.** Modified: `crates/api/src/render.rs`,
  `crates/api/src/routes/mod.rs`,
  `frontend/components/TrackTrainForm.tsx`,
  `frontend/components/TrackTrainForm.test.tsx`. Created:
  `crates/api/src/routes/departures.rs` (including its own `#[cfg(test)]`
  module, following `station_stats.rs`'s convention of colocated route
  tests rather than a separate test file).
- **No `internal_oauth_routes` entry needed** — the new route is
  unauthenticated, mounted via `public_router()` only, identical to
  `station_stats::router()`. Do not add anything to
  `crates/api/src/app.rs::build_internal_oauth_routes`.
- **No proxy changes needed.** `frontend/app/api/[...path]/route.ts`'s
  existing catch-all already prepends `/public/` to any non-`Train`
  path segment — confirm this by reading `resolveTargetPath` before
  starting Task 2, but do not modify that file as part of this plan.
- **CRS case handling:** no new normalization. The new route passes
  whatever `crs` the client sends straight to `latest_station_sample`,
  exactly like `station_stats.rs` already does — do not add a
  `.to_uppercase()` call the existing sibling route doesn't have.

---

### Task 1: Backend — `GET /public/stations/{crs}/departures` (Decision 2)

**Files:**
- Create: `crates/api/src/routes/departures.rs`
- Modify: `crates/api/src/render.rs`
- Modify: `crates/api/src/routes/mod.rs`

Independent of Task 2 in terms of code, but Task 2's manual smoke check
depends on this being deployed.

- [ ] **Step 1: Add `station_departure_json` to `crates/api/src/render.rs`**

Place it near the existing `sample_stats_json`/`sample_availability_json`
helpers. Use the exact field mapping from the design doc's Decision 2
code sketch (reproduced here, copy verbatim, do not re-derive):

```rust
pub(crate) fn station_departure_json(d: &common::StationDeparture) -> serde_json::Value {
    serde_json::json!({
        "serviceId": d.service_id,
        "operator": d.operator,
        "destinationCrs": d.destination_crs,
        "scheduled": d.scheduled,
        "estimated": d.estimated,
        "isCancelled": d.is_cancelled,
        "delayMinutes": d.delay_minutes,
        "cancelReason": d.cancel_reason,
        "delayReason": d.delay_reason,
        "skippedStations": d.skipped_stations,
    })
}
```

- [ ] **Step 2: Add a unit test for `station_departure_json`**

In `render.rs`'s existing `#[cfg(test)] mod tests` block. Construct one
`common::StationDeparture` with every optional field populated and one
with `cancel_reason`/`delay_reason: None`, and assert:
- Every field name in the output is the exact camelCase name above (no
  stray snake_case field survives, e.g. no `destination_crs` alongside
  `destinationCrs`).
- `None` fields serialize to JSON `null`, not an omitted key (unlike
  `TrackPinRequest`'s own `skip_serializing_if`, this is a plain
  hand-built `json!`, so `None` → `Value::Null` by `serde_json`'s default
  `Option` behavior — assert this explicitly, since a future reader
  might assume it's omitted the way the request-side type is).
- `skipped_stations: vec![]` serializes to `[]`, not omitted.

- [ ] **Step 3: Create `crates/api/src/routes/departures.rs`**

Use the design doc's Decision 2 code sketch verbatim for `router()` and
`get_station_departures` — copy it exactly (module doc comment included),
following `station_stats.rs`'s own file shape (imports, `router()`,
handler, `internal_error`) as the structural template.

- [ ] **Step 4: Register the route in `crates/api/src/routes/mod.rs`**

Add `pub mod departures;` next to the existing `pub mod station_stats;`,
and `.merge(departures::router())` inside `public_router()`'s builder
chain, next to `.merge(station_stats::router())`.

- [ ] **Step 5: Add DB-backed route tests in `departures.rs`**

Following `station_stats.rs`'s exact fixture pattern (`connect()`,
`delete_fixture`, `INSERT INTO station_samples ... ON CONFLICT`,
`#[ignore = "requires a live database; run with \`cargo test -p api \
departures -- --ignored --test-threads=1\`"]`, `router().oneshot(...)`).
Use CRS codes that don't collide with `station_stats.rs`'s own fixture
codes (`ZQQ`/`ZQR`/`ZQS` are taken — use `ZQT`/`ZQU` or similar), since
both files' tests may run against the same test database.

Cover:
- No row for the CRS at all → `404`, body names the CRS (mirror
  `station_sample_stats_no_row_is_404`'s shape).
- Row exists, `departures: '[]'` → `200 []`.
- Row exists with two departures, one cancelled with a `cancelReason` set
  and one on-time with `cancelReason: null` → `200`, asserting: exact
  camelCase field names on both entries; array order matches insertion
  order (proving no re-sort was introduced, per design doc Decision 2);
  the cancelled entry's `isCancelled: true` and `cancelReason` populated;
  the on-time entry's `cancelReason: null`.

- [ ] **Step 6: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api departures -- --ignored --test-threads=1
```

Expected: all PASS, zero clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/render.rs crates/api/src/routes/mod.rs crates/api/src/routes/departures.rs
git commit -m "Add GET /public/stations/{crs}/departures, a raw pass-through of the live station_samples board"
```

---

### Task 2: Frontend — inline departure picker on `TrackTrainForm` (Decisions 3-4)

**Files:**
- Modify: `frontend/components/TrackTrainForm.tsx`
- Modify: `frontend/components/TrackTrainForm.test.tsx`

Depends on Task 1's wire shape being final (this task's mocked JSON
shapes must match Task 1's actual field names exactly) but not on Task
1 being deployed — `global.fetch` is mocked in tests, per this file's
existing pattern for `searchStations`/`searchTocs`-driven suggestions.

- [ ] **Step 1: Add a `DepartureRow` type and picker state**

At the top of `TrackTrainForm.tsx`, near `CRS_PATTERN`:

```tsx
interface DepartureRow {
  serviceId: string;
  operator: string;
  destinationCrs: string;
  scheduled: string;
  estimated: string;
  isCancelled: boolean;
  delayMinutes: number;
  cancelReason: string | null;
  delayReason: string | null;
  skippedStations: string[];
}
```

Inside the component, add:

```tsx
const [departures, setDepartures] = useState<DepartureRow[] | 'not-sampled' | null>(null);
```

- [ ] **Step 2: Add the fetch effect**

Use the design doc's Decision 4 code sketch verbatim for the `useEffect`
body (fetch through `/api/stations/{crs}/departures`, branch on
`res.status === 404` → `'not-sampled'`, `!res.ok` → `null`, otherwise
`res.json()`). Depends on `[originCrs, originValid]` exactly as sketched
— `originValid` already exists in this file (`CRS_PATTERN.test(originCrs.trim())`).

- [ ] **Step 3: Render the three-state picker block**

Directly below the Origin `Autocomplete`, before the `Group` containing
the `DateTimePicker`. Three states per the design doc's Decision 4:
- `departures === null`: render nothing.
- `departures === 'not-sampled'`: a single dimmed `Text`, the exact copy
  from the design doc ("Live departures aren't available to browse for
  this station — enter the details below.").
- `departures` is `[]`: a single dimmed `Text` ("No live departures
  currently on the board for this station right now.").
- `departures` is a non-empty array: a scrollable list (a plain `Stack`
  with a bounded `mah`/`ScrollArea`, or Mantine's `ScrollArea` component —
  implementer's call on exact Mantine primitive, not specified further
  here), one row per departure: `scheduled` time, `destinationCrs` (bare
  code is fine — do not add a new station-name lookup call for this;
  the existing `originSuggestions`/`destinationSuggestions` arrays are
  query-scoped to what's been typed, not a general CRS→name index, so
  resolving every departure's destination name would need a new fetch
  this task does not add), `operator`, and a badge: `isCancelled` →
  red "Cancelled" badge, row not clickable; else `delayMinutes > 0` →
  amber `+{delayMinutes} min` badge, row clickable; else green "On time"
  badge, row clickable.

- [ ] **Step 4: Add `pickDeparture`**

Use the design doc's Decision 4 code sketch verbatim — combines
`row.scheduled`'s `HH:MM` with `dayjs().format('YYYY-MM-DD')` into the
exact `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture` already
expects (matching the existing "Now" button's construction, immediately
above this block in the same file). Wire it as the `onClick` for each
non-cancelled row.

- [ ] **Step 5: Add tests in `TrackTrainForm.test.tsx`**

Following this file's existing `global.fetch` mock pattern (check how
the existing suggestion-fetch tests mock multiple distinct URLs — the
mock will need to branch on whether the request is
`/api/stations?q=...` (suggestions) vs. `/api/stations/{crs}/departures`
(this task), since both can now be in flight). Cover:
- Typing a valid origin CRS triggers a `departures` fetch to
  `/api/stations/{ORIGIN}/departures`.
- A `404` response renders the "not available to browse" text.
- A `200 []` response renders the "no live departures right now" text.
- A `200` response with one cancelled and one on-time departure renders
  both rows, the cancelled one non-clickable (assert no click handler
  fires / the field values don't change on a click attempt, or that the
  row has no button role — implementer's call on the exact assertion
  shape, matching how this file already asserts disabled-state behavior
  elsewhere if such a precedent exists in this file).
- Clicking a non-cancelled row sets `destinationCrs`/`operator`/
  `scheduledDeparture` to the expected values, and the resulting
  `scheduledDeparture` string matches the `'YYYY-MM-DD HH:mm:ss'` shape
  (reuse this file's existing "Now"-button test's technique for
  controlling/reading "today's date" deterministically, per that test at
  `TrackTrainForm.test.tsx:336-348` — do not introduce a second,
  different way of mocking "now" in the same file).
- Changing `originCrs` away from a previously-picked value does **not**
  clear the already-filled Destination/Operator/Scheduled-departure
  fields (design doc's Error handling section, last bullet).

- [ ] **Step 6: Test and build**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 7: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "Add a live-departures picker to the /track form for already-sampled origin stations"
```

---

### Task 3: Final verification

- [ ] **Step 1: Full workspace verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -- --ignored --test-threads=1
```

- [ ] **Step 2: Full frontend verification**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 3: Manual smoke check against a real deployment (if available)**

`GET /public/stations/WAT/departures` (or any known-sampled station, e.g.
one from the per-station-stats research doc's measured list — `EDB`,
`LIV`, `NCL`, or `WAT`) — confirm a populated array with correct camelCase
throughout. `GET /public/stations/ZZZ/departures` (an unsampled code) —
confirm `404`. Load `/track?origin=WAT` (or another known-sampled
station) in a browser and confirm the picker renders, a cancelled row (if
one happens to be on the board) is shown but not clickable, and clicking
an on-time/delayed row fills the three fields correctly, leaving the form
still submittable by hand afterward.

- [ ] **Step 4: Confirm no stray edits outside this plan's file scope**

```bash
git diff --stat main...HEAD
```

Compare against this plan's Global Constraints "File scope" list — flag
anything unexpected before considering the branch done.

## Testing

Summarized (see each task's own steps for the authoritative detail):

- **`crates/api`**: a unit test for the new `station_departure_json`
  helper (pure, no DB); a `#[ignore]`-gated DB-backed test trio for the
  new route (404 / empty / populated-with-cancelled), following
  `station_stats.rs`'s exact fixture-and-`oneshot` convention.
- **`frontend`**: new tests in `TrackTrainForm.test.tsx` covering all
  three picker states, the cancelled-row-not-clickable behavior, the
  exact fill-on-click values (including the date-string shape), and the
  no-auto-clear-on-origin-change behavior — via this file's existing
  `global.fetch` mock pattern, no new test infrastructure.
- **CI**: the new DB-backed route tests run under the existing `api`
  crate's `--ignored` CI job — no new CI job needed.
