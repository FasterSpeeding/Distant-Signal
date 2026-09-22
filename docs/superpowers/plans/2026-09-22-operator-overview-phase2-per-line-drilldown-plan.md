# Plan: Operator Overview Phase 2 — Per-Line Drill-Down Completion

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** close section B's two concrete gaps against `/lines/[id]`, per
`docs/superpowers/specs/2026-09-22-operator-overview-design.md`'s Phase 2
scope: (1) confirm whether `Disruption.affectedStops`/`affectedRoutes`
segment/station detail is already surfaced by `DisruptionDetail`, and (2) add
a "Trains running today" panel backed by the already-existing
`GET /public/lines/{id}/trains?date=` route. **Headline finding, verified by
reading the real code before writing a single task below: gap (1) does not
exist.** `frontend/components/DisruptionDetail.tsx:33-46` already renders
both `affectedStops` (as gray outline badges, one per CRS) and
`affectedRoutes` (as `{from} → {to}` dimmed text lines), and
`frontend/components/DisruptionDetail.test.tsx:27-36` already has passing
regression tests for exactly that (`renders each affected stop`,
`renders the affected route range`). This plan's Task 1 is therefore a
**verification task, not a fix** — it re-runs that existing test file and
records the result, and touches no source. Every other task in this plan is
about gap (2), a genuine, real gap: nothing in the frontend calls
`GET /public/lines/{id}/trains` today (confirmed — `grep -n
"trains" frontend/lib/api.ts` matches nothing before this plan), so this is
new frontend code, wired against a backend route that already exists
unchanged.

**Architecture:** frontend-only, no backend/migration work, matching the
spec's own "Complexity: low" characterization of this phase and the
`get_line_trains` route's own doc comment (`crates/api/src/routes/lines.rs:241-260`),
which describes a route that already does everything this task needs. Task 1
verifies gap (1) is closed. Tasks 2-3 add the wire types
(`frontend/lib/types.ts`) and the client function (`frontend/lib/api.ts`,
`getLineTrains`) the new panel needs — modeled directly on the sibling
`getLineDefinition`/`getLineHalfHourlyStats` functions already in that file.
Task 4 builds the panel itself, a new Server Component
(`frontend/app/lines/[id]/LineTrainsResults.tsx`) modeled directly on the
existing `HalfHourlyTrendsResults.tsx` (same file, same page, same
"async Server Component behind a `<Suspense>` boundary, never throws, has
its own honest empty/error states" idiom already established on this exact
page). Task 5 wires it into `frontend/app/lines/[id]/page.tsx`, gated off
for the two kinds of line id verified to have **no** CIF schedule data at
all (TfL lines and custom lines — see Judgment Call 2), and updates that
page's existing test file's mocks so the new fetch doesn't break every
existing test in it. Task 6 is the final cross-cutting verify + commit.

**Tech stack:** Next.js/React/Mantine (`frontend`) only. No Rust, no
migration, no new route.

**Spec:** `docs/superpowers/specs/2026-09-22-operator-overview-design.md`
— authoritative for every architectural decision this plan implements
(this is section B, "Per-line granular pages", and the Phase 2 bullet of
its "Phased delivery plan"). This plan does not re-litigate anything that
spec already settled, and does not touch Phase 1 (A, all-lines dashboard),
Phase 3 (C/E, operators list + pinning), or Phase 4 (D, historical
views for operators/network) — those are explicitly out of scope, being
covered by other work in parallel.

---

## Judgment calls this plan makes (read before Task 1)

1. **Gap 1 (segment/station detail) needs verification only, not a fix —
   confirmed by reading the actual component and its test file.**
   `frontend/components/DisruptionDetail.tsx:33-41` renders
   `disruption.affectedStops.map((crs) => <Badge ...>{crs}</Badge>)` inside a
   `Group`, and lines 42-46 render
   `disruption.affectedRoutes.map((route, i) => <Text ...>{route.from} → {route.to}</Text>)`
   directly below the sanitized description. `DisruptionDetail.test.tsx:27-36`
   already exercises both (`'renders each affected stop'`,
   `'renders the affected route range'`), and
   `frontend/lib/types.ts:12-19`'s `Disruption` interface confirms the exact
   wire shape those tests assume (`affectedStops: string[]`,
   `affectedRoutes: AffectedRoute[]`, `AffectedRoute` at `types.ts:7` being
   `{ from: string; to: string }`). There is nothing here to "make more
   prominent" that would justify new code without inventing a requirement the
   spec didn't actually make — the spec's own wording ("may not be
   prominently surfaced... worth verifying") already hedges exactly this
   way. **Task 1 below re-runs the existing test file and stops there.**

2. **The new "Trains running today" panel must not render at all for a
   custom line or a TfL line — both are guaranteed 404s from
   `get_line_trains`, verified from two independent places, not assumed:**
   - `crates/api/src/routes/lines.rs:143-151`'s doc comment on the sibling
     `get_line_schedule` route (`get_line_trains` reads the exact same
     `schedule_line_population` table via the exact same
     `readable_line_id`/`resolve_schedule_date` helpers, `lines.rs:268-284`)
     states plainly: "Deliberately does NOT check `app.config.lines`/
     `custom_lines` first... an unknown, **custom**, or not-yet-published
     catalogue `id` alike simply 404s for the same reason."
   - `crates/schedule-reference/src/main.rs:676`'s `lines_to_publish`
     (the only writer of `schedule_line_population`) iterates
     `config.lines` — the static catalogue TOML — only; there is no TfL
     input to it at all (TfL lines come from `queries::tfl_line_summaries`,
     an entirely separate source per the design spec's own §C finding, never
     fed into `schedule-reference`). TfL line ids are namespaced with the
     `tfl-` prefix (`crates/common/src/lib.rs:257`'s `TFL_LINE_ID_PREFIX`,
     mirrored in the frontend's own literal `'tfl-...'` ids —
     `frontend/lib/modes.ts:36-42`).

   Rendering the panel anyway would cost a guaranteed round-trip per page
   load for every custom and TfL line, resolving only to the
   already-correct-but-pointless "no scheduled data" empty state Task 4
   builds for the genuinely-unpublished case. `frontend/app/lines/[id]/page.tsx`
   already computes `isCustom` (line 310 onward) for its own Edit/Delete
   gating, so this reuses an existing value rather than adding a new probe;
   the TfL check is one new `id.startsWith('tfl-')` (Task 5), the same
   literal convention `lib/modes.ts` already uses for its own TfL-adjacent
   line list, since there is no Rust→TypeScript constant bridge in this
   codebase to import `TFL_LINE_ID_PREFIX` from (same gap the custom-names
   plan's own Judgment Call 1 already documents and accepts for a different
   constant).

3. **The panel computes and passes an explicit `date` to `getLineTrains`,
   rather than relying on the backend's own UTC-default, and reuses that
   same string to build every row's `/train/{uid}/{date}` link.**
   `resolve_schedule_date` (`crates/api/src/routes/lines.rs:130-135`)
   defaults to `chrono::Utc::now().date_naive()` — a bare UTC calendar day —
   when `?date=` is omitted. This app's own stated timezone rule
   (`frontend/lib/dateFormat.ts:1-26`'s module doc: "this is a UK rail
   product... London wall-clock" for every network-time value) is UK-local,
   not UTC, and `frontend/lib/dateFormat.ts:119-121` already has
   `londonDayKey(value: string | Date): string` for exactly this
   "London calendar day" computation (used today by
   `frontend/lib/history.ts:164` for grouping status history). Rather than
   let the panel's fetch silently use one day-boundary convention (UTC) while
   its own outbound links to `/train/{uid}/{date}` used a different one
   (whatever `londonDayKey`/`dayjs` would compute), Task 5 computes
   `londonDayKey(new Date(now))` once (reusing the page's own existing `now`
   stamp, `page.tsx:360`) and threads that single string through both the
   fetch and every link — the two can no longer disagree, and no new
   date-boundary concept is introduced beyond one this codebase already
   uses.

4. **New TypeScript types, not a reuse of `ScheduleCallingPoint`/
   `PublicTrainState` — because the wire shape genuinely differs and reusing
   either would silently mismatch it.** `crates/api/src/render.rs:288-311`'s
   `line_train_json` sets `"callingPoints": entry.get("calling_points").cloned()...`
   — a **raw, unprocessed pass-through** of the population entry, camelCase
   only at the outer key, never touching the array elements' own field
   names. Proven directly by that file's own test,
   `line_train_json_with_no_live_row_passes_the_population_entry_through_and_nulls_live_status`
   (`render.rs:901-914`): its fixture `entry` has
   `"calling_points": [{"tiploc": ..., "kind": "Origin", "booked_arrival": null, "booked_departure": "08:00:00", ...}]`
   (snake_case), and the test asserts `json["callingPoints"] == entry["calling_points"]`
   **verbatim** — i.e. the *elements* inside `callingPoints` are still
   snake_case, only the envelope key is camelCase. This is a different shape
   from `frontend/lib/types.ts:451-458`'s `ScheduleCallingPoint` (fully
   camelCase, backed by a completely different Rust type,
   `crates/api/src/data/schedule_matching.rs`'s `ScheduleCallingPointDto`,
   per that interface's own doc comment) — reusing it here would be silently
   wrong (a `tiploc`/`booked_departure`-shaped object read through fields
   named `bookedDeparture` would just be `undefined`, not a compile error,
   since both are plain interfaces with no runtime validation). Task 2
   defines a new `LineTrainCallingPoint` with the real, snake_case field
   names and says so explicitly in its doc comment, rather than
   perpetuating this by silent omission. `liveStatus`, separately, only
   overlaps `PublicTrainState` partially (`render.rs:295-310` sets 13 named
   fields, explicitly omitting `journeyStops`/`callingPoints` — confirmed by
   `render.rs`'s other test,
   `line_train_json_with_a_live_row_attaches_live_status_in_camel_case`,
   lines 916-965, which asserts both are absent) — a new `LineTrainLiveStatus`
   type is used rather than the full `PublicTrainState`, so the type itself
   documents what is and isn't there instead of relying on every caller to
   remember which of `PublicTrainState`'s fields don't actually apply here.

5. **Sort key: every row is ordered by its own schedule-side first calling
   point's booked time, never by `liveStatus`'s (optional, RFC3339) scheduled
   departure.** The schedule (`callingPoints`) is the backbone of every
   entry this route returns — the array comes from
   `schedule_line_population`, not from `trains` — so `callingPoints[0]` is
   always present (barring a malformed population, its own pre-existing
   condition, not one this plan introduces or degrades) whether or not
   `liveStatus` is `null`. Sorting by `liveStatus?.scheduledDeparture` would
   silently misorder every schedule-only row (no live status yet) to the
   back regardless of its real time; sorting uniformly by the schedule's own
   `booked_departure`/`booked_arrival` (a bare `"HH:MM:SS"` CIF-local string,
   lexically sortable on its own terms, `crates/schedule-query/src/records.rs:138-167`)
   avoids ever comparing two different timestamp formats against each other.

6. **No pagination, no date picker on the new panel — it renders exactly
   one rail day ("today"), matching `StationTimetable.tsx`'s own explicit
   scope decision for its analogous "scheduled departures for this station"
   panel** ("there is no date picker on this stripped-down view" —
   `StationTimetable.tsx:56-58`'s `today()` helper comment). Unlike
   `GET /public/trains/search` (which `StationTimetable` reads, and which
   is genuinely paginated — `nextCursor`/`LoadMoreControl`),
   `GET /public/lines/{id}/trains` returns the **whole day's population in
   one response**, with no cursor of its own (`lines.rs:307-316`'s handler
   returns one flat `Vec<Value>`, no pagination fields) — so there is
   nothing to add a "Load more" control for even if this plan wanted one.

7. **`getLineTrains` defaults to an empty-array mock at the top of
   `page.test.tsx`'s existing `vi.mock('@/lib/api', ...)` factory, the same
   way `getAllTocs` already is** (`page.test.tsx:30`'s own comment: "most
   tests in this file don't care..."). Every existing `describe` block in
   that file renders the whole page and would otherwise need its own
   `beforeEach` update merely to keep working — defaulting once, centrally,
   is both less invasive and matches the file's own established precedent
   for exactly this situation.

8. **Panel placement: directly after the "TfL also reports" block, before
   "Recent trends (last 24 hours)".** Not specified by the spec. Rationale:
   "what's running right now" is current, actionable information in the same
   register as the `IssueList`/TfL-status block immediately above it, not a
   historical/statistical readout like the trend charts below — grouping the
   two "what is happening today" sections together, before the "how has this
   been performing" section, keeps the page's existing top-to-bottom
   "current state, then history" shape (name → status → issues → **trains**
   → trends → history link) rather than interleaving them.

---

## Non-goals

- **No backend changes of any kind.** `GET /public/lines/{id}/trains` is
  complete and correct as-is (`crates/api/src/routes/lines.rs:241-319`); no
  task in this plan modifies `crates/`.
- **No fix to `DisruptionDetail.tsx`** — Judgment Call 1 established there is
  nothing to fix. Resolving CRS codes in `affectedStops`/route endpoints in
  `affectedRoutes` to full station names (rather than bare codes) is a real,
  small, separable future enhancement (`stationLabel`/`routeLabel`,
  `frontend/lib/stationLabel.ts`, both require a `name` alongside the code,
  which `Disruption` doesn't carry) — explicitly left for later, since the
  spec's actual ask ("surfaced... at all") is already satisfied and adding a
  new station-name lookup dependency to this component would be new scope,
  not a gap-closing fix.
- **No date picker or "search a different day" link on the new panel** — see
  Judgment Call 6.
- **No pagination/"Load more" on the new panel** — see Judgment Call 6.
- **No TIPLOC→CRS/station-name resolution for schedule-only (no-`liveStatus`)
  rows.** Those rows render their raw scheduled time and UID only, with no
  attempt to decode `LineTrainCallingPoint.tiploc` into a station name —
  doing so would need a new TIPLOC lookup this plan does not add (the
  existing `GET /public/stanox-crs` route, mentioned only in passing by
  `lines.rs`'s own doc comments, is a separate, unrelated dataset this plan
  does not touch).
- **No change to `/lines/[id]/history`** (Timeline/Trends tabs) — that is
  already complete per the spec's §0/D findings and is not part of section B.
- **No change to `frontend/app/api/[...path]/route.ts`** — it is a
  catch-all proxy (`resolveTargetPath`, `route.ts:39-41`) that already
  forwards any `/public/lines/...` path, including its query string
  (`route.ts:55`'s `${req.nextUrl.search}`), with zero changes needed; this
  plan's new fetch is server-side only (via `lib/api.ts`, like every other
  line-scoped read on this page), so it never even goes through that proxy.

## Global Constraints

- **File scope.** Modified/created, frontend only:
  `frontend/lib/types.ts`,
  `frontend/lib/api.ts`,
  `frontend/lib/api.test.ts`,
  `frontend/app/lines/[id]/LineTrainsResults.tsx` (new),
  `frontend/app/lines/[id]/LineTrainsResults.test.tsx` (new),
  `frontend/app/lines/[id]/page.tsx`,
  `frontend/app/lines/[id]/page.test.tsx`.
  No other file changes, and no `crates/` changes at all.
- **Testing.** This phase touches no Rust, so no `cargo` commands are
  required by this plan. Frontend, matching `.github/workflows/ci.yml`'s
  `frontend` job exactly (lines ~237-268 of that file — "Typecheck
  (tsc --noEmit)", "npm test (vitest)", "npm run build (next build)"): run
  from `frontend/`,
  - `npx tsc --noEmit` after every task that touches a `.ts`/`.tsx` file;
  - `npm test -- <file>` (vitest — `package.json`'s `"test": "vitest run"`)
    for each changed/new test file, immediately after writing it;
  - a full `npm test` and `npm run build` once, at the end (Task 6), matching
    CI's own two frontend steps exactly.
  **UI verification**: per this repo's standing practice for a change with
  no automated end-to-end coverage (no `frontend/e2e` spec exists for this
  page — CI's own comment on the `frontend` job notes `frontend/e2e` "does
  not exist yet"), start the dev stack and manually verify in a real
  browser: a catalogue line with published CIF schedule data shows a
  populated "Trains running today" panel with working `/train/{uid}/{date}`
  links; a custom line and a TfL line (e.g. an id starting `tfl-`) show no
  panel at all; a catalogue line with no schedule population yet (or outside
  CIF coverage) shows the honest "No scheduled train data is available for
  this line today" message, not an error or a blank gap. Folded into Task 5's
  own Verify step, not a separate task.

---

## Task 1: Verify gap 1 (segment/station detail) is already closed — no code change

**Files:** none modified. Read-only verification, recorded here so this
plan's own history shows the check was actually run, not assumed from the
spec.

- [ ] **Step 1: Re-confirm by reading the component directly** (already done
  once while writing this plan, repeat before starting Task 2+ in case the
  worktree has moved):

```bash
sed -n '1,67p' frontend/components/DisruptionDetail.tsx
```

  Expected: lines 33-41 render `disruption.affectedStops` as a `Group` of
  `Badge`s; lines 42-46 render `disruption.affectedRoutes` as
  `{route.from} → {route.to}` `Text` lines. If either block is missing in
  the worktree this plan actually runs against, stop here — that would mean
  the spec's premise (and this plan's own Judgment Call 1) no longer holds,
  and Task 1 would need to become a real fix instead. Do not proceed past
  this step on an assumption.

- [ ] **Step 2: Run the existing regression tests for both fields**

```bash
cd frontend && npm test -- DisruptionDetail.test.tsx
```

  Expected: all tests pass, including `'renders each affected stop'` and
  `'renders the affected route range'` (`DisruptionDetail.test.tsx:27-36`).
  A passing run here is the actual evidence gap 1 is closed — record the
  result, do not just cite the source.

- [ ] **Step 3: No commit** — this task changes nothing. Proceed directly to
  Task 2.

---

## Task 2: `frontend/lib/types.ts` — wire types for `GET /public/lines/{id}/trains`

**Files:** modify `frontend/lib/types.ts`.

Independent of every other task; nothing else compiles against these types
until Task 3.

- [ ] **Step 1: Add the three new types**, near the existing
  `PublicTrainState`/`ScheduleCallingPoint` types (after `TicketListItem`,
  end of file, `types.ts:648` today — appended there rather than interleaved
  with the tracked-train types above it, since this is a different route
  family):

```typescript
export type LineTrainCallingPointKind = 'Origin' | 'Intermediate' | 'Terminate';

/** One calling point inside a `GET /public/lines/{id}/trains?date=` entry's
 * `callingPoints` array (`crates/api/src/routes/lines.rs`'s `get_line_trains`,
 * rendered by `crates/api/src/render.rs`'s `line_train_json`). NOT the same
 * shape as `ScheduleCallingPoint` above (that one is fully camelCase,
 * backed by a different Rust type, `schedule_matching::ScheduleCallingPointDto`).
 * `line_train_json` passes the population entry's `calling_points` array
 * through UNPROCESSED -- only the outer `callingPoints` envelope key is
 * camelCase; see `render.rs`'s own
 * `line_train_json_with_no_live_row_passes_the_population_entry_through_and_nulls_live_status`
 * test, which asserts `json["callingPoints"] == entry["calling_points"]`
 * verbatim. Field names below are therefore this crate's usual accidental
 * exception, not a typo: the real `schedule_query::CallingPoint` snake_case
 * names, the same documented wart `get_line_schedule`'s own doc comment
 * accepts for this identical underlying data. */
export interface LineTrainCallingPoint {
  tiploc: string;
  kind: LineTrainCallingPointKind;
  booked_arrival: string | null; // "HH:MM:SS", CIF/UK-local, no date component
  booked_departure: string | null;
  is_half_minute_arrival: boolean;
  is_half_minute_departure: boolean;
  day_offset: number;
}

/** The `liveStatus` field of a `GET /public/lines/{id}/trains?date=` entry
 * -- `null` whenever no live `trains`/`train_current_state` row exists yet
 * for this UID on this date (an expected, honest gap: this route never
 * triggers a `find_or_create_train` upsert the way
 * `GET /Train/by-uid/{uid}/{date}` does -- see `get_line_trains`'s own doc
 * comment). Deliberately its own type, not a reuse of `PublicTrainState`:
 * `line_train_json` (`render.rs:295-310`) includes only these 13 fields
 * inside `liveStatus`, explicitly omitting `journeyStops`/`callingPoints`/
 * `trainUid`/`serviceDate`/`mayHaveArrived` -- confirmed by that file's
 * `line_train_json_with_a_live_row_attaches_live_status_in_camel_case`
 * test, which asserts both omitted fields are absent. */
export interface LineTrainLiveStatus {
  trainsId: number;
  trainId: string | null;
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  scheduledDeparture: string | null; // RFC3339
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
}

/** One `GET /public/lines/{id}/trains?date=` response entry
 * (`crates/api/src/routes/lines.rs`'s `get_line_trains`) -- every scheduled
 * UID on one line for one rail day, paired with live status where one
 * already exists. See that route's own doc comment and
 * docs/superpowers/specs/2026-09-09-mcp-schedule-data-follow-up-design.md
 * §5.3. The full response is `LineTrainEntry[]`, not wrapped in an
 * envelope -- unlike `GET /public/trains/search`, this route has no cursor
 * of its own; it always returns the whole day's population in one call. */
export interface LineTrainEntry {
  uid: string;
  callingPoints: LineTrainCallingPoint[] | null;
  liveStatus: LineTrainLiveStatus | null;
}
```

  `JourneyStatus`/`EtaSource` are already defined above in this same file
  (`types.ts:442-443`) — no new import needed, this file is self-contained.

- [ ] **Step 2: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: builds clean (a bare new `export interface` addition cannot
  break any existing caller — nothing references these types yet).

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "frontend: add LineTrainEntry/LineTrainCallingPoint/LineTrainLiveStatus wire types"
```

---

## Task 3: `frontend/lib/api.ts` — `getLineTrains` client function

**Files:** modify `frontend/lib/api.ts`, `frontend/lib/api.test.ts`.

Depends on Task 2's types.

- [ ] **Step 1: Add `LineTrainEntry` to this file's existing type-only
  import block** (`api.ts:2-36`), alongside `PublicTrainState`:

```typescript
  PublicTrainState,
  LineTrainEntry,
```

- [ ] **Step 2: Add `getLineTrains`**, directly below `getLineDefinition`
  (`api.ts:407-413`), as its closest sibling (same `/public/lines/{id}/...`
  family, same no-store/cookie-forwarding shape):

```typescript
/** `GET /public/lines/{id}/trains?date=` -- every scheduled UID on line
 * `id` for one rail day, each paired with its live status where one
 * already exists (`crates/api/src/routes/lines.rs`'s `get_line_trains`).
 * `date` is `"YYYY-MM-DD"`; when omitted the backend defaults to its own
 * UTC "today" (`resolve_schedule_date`) -- callers that build a link from
 * this response (e.g. `/train/{uid}/{date}`) should always pass `date`
 * explicitly instead, so the fetched day and the link's day can never
 * disagree (see `LineTrainsResults`'s own use of `londonDayKey`, this
 * app's stated London-calendar-day convention, `lib/dateFormat.ts`).
 * 404s (`ApiNotFoundError`) when there is no CIF-derived schedule
 * population for this `(id, date)` -- an unpublished catalogue line, a
 * custom line, a TfL line (neither ever has one at all -- see
 * `get_line_schedule`'s own doc comment), or a rail day not yet published,
 * all indistinguishable from here, same as `getLineDefinition`'s sibling
 * route just above. */
export async function getLineTrains(id: string, date?: string): Promise<LineTrainEntry[]> {
  const query = date ? `?date=${encodeURIComponent(date)}` : '';
  const url = `${baseUrl()}/public/lines/${id}/trains${query}`;
  return fetchJson<LineTrainEntry[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}
```

- [ ] **Step 3: Add tests**, in `frontend/lib/api.test.ts` directly below
  the existing `getLineDefinition` tests (after `api.test.ts:609`), modeled
  on those three tests exactly (URL, cookie-forwarding, no-cookie cases),
  plus the `date`-param and 404 cases those don't need to cover:

```typescript
  it('getLineTrains fetches the correct URL with no date and no caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getLineTrains('swr-alton');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/swr-alton/trains',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineTrains includes an explicit ?date= when given', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getLineTrains('swr-alton', '2026-09-22');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/swr-alton/trains?date=2026-09-22',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineTrains forwards the incoming request cookies to the backend', async () => {
    incomingCookies.header = 'distant_signal_session=abc123';
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })),
    );
    await getLineTrains('swr-alton');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/lines/swr-alton/trains',
      expect.objectContaining({ headers: { Cookie: 'distant_signal_session=abc123' } }),
    );
  });

  it('getLineTrains throws ApiNotFoundError on a 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getLineTrains('swr-alton')).rejects.toBeInstanceOf(ApiNotFoundError);
  });
```

  Add `getLineTrains` to this test file's own top-of-file import list
  (alongside `getLineDefinition`, `api.test.ts:17`). Check `incomingCookies`
  is the same shared mutable fixture the existing `getLineDefinition` cookie
  test uses (`api.test.ts:589`) rather than a new one — reuse it as-is.

- [ ] **Step 4: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- api.test.ts
```

  Expected: typecheck clean, all `api.test.ts` tests (existing + 4 new)
  pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "frontend: add getLineTrains client function for GET /public/lines/{id}/trains"
```

---

## Task 4: `LineTrainsResults` — the panel's Server Component

**Files:** create `frontend/app/lines/[id]/LineTrainsResults.tsx`,
`frontend/app/lines/[id]/LineTrainsResults.test.tsx`.

Depends on Task 3 (`getLineTrains`). Modeled directly on
`frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx` — same
"async Server Component, never throws past its own boundary, has distinct
honest empty/error states" idiom already established on this exact page
(`page.tsx`'s own `TrendsLoadingFallback` doc comment, lines 35-43, states
the same rule this component follows).

- [ ] **Step 1: Write the component**

```typescript
import { Group, Paper, Stack, Text } from '@mantine/core';
import { ApiNotFoundError, getLineTrains } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';
import { formatTime } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import { TextLink } from '@/components/TextLink';

/** Every entry's schedule-side first calling point's own booked time --
 * used as the sort key for every row uniformly, whether or not that row
 * has a `liveStatus` yet. Deliberately NOT `entry.liveStatus?.scheduledDeparture`
 * (an RFC3339 instant, only present once a live `trains` row exists): mixing
 * that format with the schedule's own bare "HH:MM:SS" CIF time within one
 * sort would compare two different string shapes against each other. The
 * schedule (`callingPoints`) is this route's backbone -- present on every
 * entry regardless of live-status coverage -- so it is the only sort key
 * that treats every row the same way. */
function firstScheduledTime(entry: LineTrainEntry): string {
  const first = entry.callingPoints?.[0];
  return first?.booked_departure ?? first?.booked_arrival ?? '';
}

/** `id`/`date` are both required (not defaulted here) -- `date` in
 * particular is deliberately the caller's own, already-computed
 * `londonDayKey`, not left to `getLineTrains`'s own UTC default, so the
 * date this panel fetches and the date every row's `/train/{uid}/{date}`
 * link points at can never disagree (see `LineDetailPage`'s own comment on
 * this). Rendered inside a `<Suspense>` on `/lines/[id]` (`page.tsx`) --
 * same rationale as `HalfHourlyTrendsResults`: Suspense catches
 * *suspension*, not errors, so every failure branch below must resolve to
 * real markup rather than throw, or a backend outage would blank the whole
 * line page instead of just this panel. */
export async function LineTrainsResults({ id, date }: { id: string; date: string }) {
  let trains: LineTrainEntry[];
  try {
    trains = await getLineTrains(id, date);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      // The expected, common case for a custom or TfL line if this
      // component is ever reached for one despite page.tsx's own gate, and
      // the equally expected case for a catalogue line with no CIF
      // schedule population for this exact rail day yet (see
      // get_line_schedule's own 404 semantics) -- not a claim that
      // something is broken.
      return (
        <Paper withBorder p="md">
          <Text c="dimmed">No scheduled train data is available for this line today.</Text>
        </Paper>
      );
    }
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Today&apos;s trains aren&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (trains.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">No trains are scheduled on this line today.</Text>
      </Paper>
    );
  }

  const sorted = [...trains].sort((a, b) => firstScheduledTime(a).localeCompare(firstScheduledTime(b)));

  return (
    <Stack gap="xs">
      {sorted.map((train) => {
        const live = train.liveStatus;
        const scheduledTime = live?.scheduledDeparture
          ? formatTime(live.scheduledDeparture)
          : firstScheduledTime(train).slice(0, 5) || '?';
        return (
          <Group key={train.uid} justify="space-between" wrap="nowrap">
            <Text size="sm">
              {scheduledTime}
              {' · '}
              {live ? (
                routeLabel(live.originCrs, live.originName, live.destinationCrs, live.destinationName)
              ) : (
                <Text span c="dimmed">
                  Scheduled — not live yet
                </Text>
              )}
              {live?.delayMinutes != null && live.delayMinutes > 0 && (
                <Text span c="dimmed">
                  {' '}
                  · {live.delayMinutes}m late
                </Text>
              )}
            </Text>
            <TextLink href={`/train/${encodeURIComponent(train.uid)}/${date}`}>View live status</TextLink>
          </Group>
        );
      })}
    </Stack>
  );
}
```

- [ ] **Step 2: Write the test**, modeled on
  `history/HalfHourlyTrendsResults.test.tsx`'s own structure (`vi.mock('@/lib/api')`,
  render the async component directly with `await`):

```typescript
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LineTrainsResults } from './LineTrainsResults';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';

vi.mock('@/lib/api');

function entry(overrides: Partial<LineTrainEntry> = {}): LineTrainEntry {
  return {
    uid: 'C12345',
    callingPoints: [
      {
        tiploc: 'WATRLMN',
        kind: 'Origin',
        booked_arrival: null,
        booked_departure: '08:00:00',
        is_half_minute_arrival: false,
        is_half_minute_departure: false,
        day_offset: 0,
      },
    ],
    liveStatus: null,
    ...overrides,
  };
}

describe('LineTrainsResults', () => {
  it('renders the "not available" state on a 404', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new ApiNotFoundError('not found'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('No scheduled train data is available for this line today.')).toBeInTheDocument();
  });

  it('renders the honest outage state on a non-404 failure', async () => {
    vi.mocked(api.getLineTrains).mockRejectedValue(new Error('connect ECONNREFUSED'));
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText("Today's trains aren't available right now.")).toBeInTheDocument();
  });

  it('renders the empty state for a real 200 [] response', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('No trains are scheduled on this line today.')).toBeInTheDocument();
  });

  it('renders a schedule-only row without a route, with the not-live copy', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([entry()]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText('Scheduled — not live yet')).toBeInTheDocument();
    expect(screen.getByText(/08:00/)).toBeInTheDocument();
  });

  it('renders a live row\'s resolved route and links to /train/{uid}/{date}', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({
        liveStatus: {
          trainsId: 1,
          trainId: '1A11',
          originCrs: 'WAT',
          originName: 'London Waterloo',
          destinationCrs: 'ALT',
          destinationName: 'Alton',
          scheduledDeparture: '2026-09-22T08:00:00Z',
          status: 'en_route',
          lastReportedLocation: 'Woking',
          lastEventType: 'DEPARTURE',
          delayMinutes: 4,
          nextCallingPoint: 'ALT',
          etaNext: null,
          etaSource: null,
        },
      }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    expect(screen.getByText(/London Waterloo \(WAT\) → Alton \(ALT\)/)).toBeInTheDocument();
    expect(screen.getByText(/4m late/)).toBeInTheDocument();
    const link = screen.getByRole('link', { name: 'View live status' });
    expect(link).toHaveAttribute('href', '/train/C12345/2026-09-22');
  });

  it('sorts rows by their own scheduled time, not response order', async () => {
    vi.mocked(api.getLineTrains).mockResolvedValue([
      entry({ uid: 'LATER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '10:00:00' }] }),
      entry({ uid: 'EARLIER', callingPoints: [{ ...entry().callingPoints![0], booked_departure: '06:00:00' }] }),
    ]);
    renderWithMantine(await LineTrainsResults({ id: 'swr-alton', date: '2026-09-22' }));
    const links = screen.getAllByRole('link', { name: 'View live status' });
    expect(links[0]).toHaveAttribute('href', '/train/EARLIER/2026-09-22');
    expect(links[1]).toHaveAttribute('href', '/train/LATER/2026-09-22');
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- LineTrainsResults.test.tsx
```

  Expected: typecheck clean, all 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/\[id\]/LineTrainsResults.tsx frontend/app/lines/\[id\]/LineTrainsResults.test.tsx
git commit -m "frontend(lines): add LineTrainsResults, the Trains running today panel"
```

---

## Task 5: Wire the panel into `/lines/[id]` and its tests

**Files:** modify `frontend/app/lines/[id]/page.tsx`,
`frontend/app/lines/[id]/page.test.tsx`.

Depends on Task 4. Resolves Judgment Calls 2, 3, 7, 8.

- [ ] **Step 1: Add two imports** to `page.tsx` (alongside its existing
  `history`/history-tab imports, `page.tsx:24-33`):

```typescript
import { londonDayKey } from '@/lib/dateFormat';
import { LineTrainsResults } from './LineTrainsResults';
```

- [ ] **Step 2: Add a `TrainsLoadingFallback`**, directly below the
  existing `TrendsLoadingFallback` (`page.tsx:44-50`) — same shape, own
  copy:

```typescript
function TrainsLoadingFallback() {
  return (
    <Paper withBorder p="md" role="status" aria-busy="true">
      <Text c="dimmed">Loading today&apos;s trains…</Text>
    </Paper>
  );
}
```

- [ ] **Step 3: Compute `isTflLine`** right after `const { id } = await
  params;` (`page.tsx:253`):

```typescript
  // No CIF-derived schedule population is ever published for a TfL line
  // (`schedule-reference`'s own `lines_to_publish` only ever iterates the
  // catalogue TOML, `app.config.lines` -- TfL lines come from
  // `queries::tfl_line_summaries` instead, never fed into that writer at
  // all). A guaranteed 404 from `getLineTrains` is harmless (LineTrainsResults
  // renders its own honest "not available" state for it) but pointless --
  // skip the fetch and the panel entirely for this id shape. Same literal
  // `'tfl-'` prefix convention `lib/modes.ts` already uses for its own
  // TfL-adjacent line list; there is no shared Rust->TypeScript constant to
  // import `common::TFL_LINE_ID_PREFIX` from.
  const isTflLine = id.startsWith('tfl-');
```

- [ ] **Step 4: Compute `showTrainsPanel`** right after the existing
  `isCustom`/`viewerOwnsLine`/`customLine` try/catch block resolves
  (`page.tsx`, immediately after line 320's closing `}` of that `catch`):

```typescript
  // A custom line has exactly the same "no schedule population, ever"
  // property as a TfL line, per the same get_line_schedule doc-comment
  // finding above ("an unknown, custom, or not-yet-published catalogue id
  // alike simply 404s") -- skip the panel for both, for the same reason.
  const showTrainsPanel = !isCustom && !isTflLine;
```

- [ ] **Step 5: Compute the shared `date` string**, right alongside the
  existing `trendsRange` computation (`page.tsx:362-367`, reusing the same
  `now` already stamped there):

```typescript
  // The same rail day used both to fetch LineTrainsResults' data and to
  // build every one of its rows' `/train/{uid}/{date}` links -- computed
  // once so the two can never disagree (see LineTrainsResults' own doc
  // comment). London-day, not a bare UTC day: this app's stated
  // network-time convention (lib/dateFormat.ts's own module doc).
  const trainsDate = londonDayKey(new Date(now));
```

- [ ] **Step 6: Render the panel**, as a new `Stack` section inserted
  directly after the "TfL also reports" conditional block and before the
  "Recent trends (last 24 hours)" `Stack` (i.e. immediately after
  `page.tsx:491`'s closing `)}` and before line 492's
  `<Stack gap="xs">`):

```tsx
      {showTrainsPanel && (
        <Stack gap="xs">
          <Title order={2} size="h4">
            Trains running today
          </Title>
          <Suspense fallback={<TrainsLoadingFallback />}>
            <LineTrainsResults id={id} date={trainsDate} />
          </Suspense>
        </Stack>
      )}
```

- [ ] **Step 7: Update `page.test.tsx`'s mocks.** Add `getLineTrains` to the
  existing `vi.mock('@/lib/api', ...)` factory (`page.test.tsx:16-34`),
  defaulted the same way `getAllTocs` already is (Judgment Call 7), so every
  existing `describe`/`it` in this file keeps working unmodified:

```typescript
    getAllTocs: vi.fn().mockResolvedValue([]),
    getLineHalfHourlyStats: vi.fn(),
    getLineHalfHourlyCoverageStats: vi.fn(),
    getLineTrains: vi.fn().mockResolvedValue([]),
```

  Add `LineTrainEntry` to this file's own `import type { ... } from
  '@/lib/types'` block (`page.test.tsx:8-14`) for the two new tests below.

- [ ] **Step 8: Add new tests** to `page.test.tsx`, in a new `describe`
  block alongside the existing ones:

```typescript
describe('LineDetailPage Trains running today panel', () => {
  beforeEach(() => {
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getLineDefinition).mockResolvedValue({ stations: ['WOK', 'CLJ'], operators: ['SW'] });
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([]);
    vi.mocked(api.getLineHalfHourlyCoverageStats).mockResolvedValue([]);
    vi.mocked(api.getCustomLine).mockRejectedValue(new ApiNotFoundError('not found'));
  });

  it('renders the panel and passes it the line id for a catalogue line', async () => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('swr-alton', 'Alton Line')]);
    vi.mocked(api.getLineTrains).mockResolvedValue([]);
    await renderPage('swr-alton');
    expect(screen.getByRole('heading', { name: 'Trains running today' })).toBeInTheDocument();
    expect(api.getLineTrains).toHaveBeenCalledWith('swr-alton', expect.any(String));
  });

  it('does not render the panel for a TfL line id, and never calls getLineTrains', async () => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('tfl-victoria', 'Victoria line')]);
    await renderPage('tfl-victoria');
    expect(screen.queryByRole('heading', { name: 'Trains running today' })).not.toBeInTheDocument();
    expect(api.getLineTrains).not.toHaveBeenCalled();
  });

  it('does not render the panel for a custom line, and never calls getLineTrains', async () => {
    vi.mocked(api.getLineStatus).mockResolvedValue([report('custom-my-commute', 'My Commute')]);
    vi.mocked(api.getCustomLine).mockResolvedValue(customLine({ isOwner: true }));
    await renderPage('custom-my-commute');
    expect(screen.queryByRole('heading', { name: 'Trains running today' })).not.toBeInTheDocument();
    expect(api.getLineTrains).not.toHaveBeenCalled();
  });
});
```

  `report('swr-alton', ...)`/`customLine(...)` reuse the file's own existing
  helpers (`page.test.tsx:63-91`) — no new fixtures needed. `cleanup()` from
  `@testing-library/react`'s own import at the top already runs
  between tests via this file's existing setup (confirmed by the file's own
  `import { cleanup, screen } from '@testing-library/react'` at line 2 being
  already in use — if this project's vitest setup doesn't already call
  `cleanup()` automatically between tests, add an explicit `afterEach(cleanup)`
  here rather than assume).

- [ ] **Step 9: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- page.test.tsx
```

  Expected: typecheck clean, every existing test in `page.test.tsx` still
  passes (the `getLineTrains` default keeps them all working unmodified),
  plus the 3 new tests.

- [ ] **Step 10: Manual browser verification** (this repo's standing
  practice for a change with no e2e coverage — see Global Constraints):
  start the dev stack, open `/lines/{a real catalogue line id with CIF
  coverage}`, confirm the "Trains running today" panel renders a populated,
  time-sorted list whose "View live status" links open real
  `/train/{uid}/{date}` pages; open a TfL line's own `/lines/{tfl-...}`
  page and a custom line's `/lines/{custom-...}` page and confirm neither
  shows the panel at all; if a catalogue line with no CIF coverage is
  available, confirm it shows the "No scheduled train data is available for
  this line today" message rather than an error or a blank gap.

- [ ] **Step 11: Commit**

```bash
git add frontend/app/lines/\[id\]/page.tsx frontend/app/lines/\[id\]/page.test.tsx
git commit -m "feat(lines): add Trains running today panel to /lines/[id]"
```

---

## Task 6: Final cross-cutting verify

**Files:** none (verification only).

- [ ] **Step 1: Full frontend test suite**

```bash
cd frontend && npm test
```

  Expected: all tests pass, including every file touched by Tasks 1-5.

- [ ] **Step 2: Full typecheck**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: clean.

- [ ] **Step 3: Production build** (matches CI's own final frontend step)

```bash
cd frontend && npm run build
```

  Expected: builds clean. `/lines/[id]` stays `revalidate = 0` (already set,
  `page.tsx:55`, unchanged by this plan) so this new section is dynamically
  rendered per request like the rest of the page — no static-generation
  concerns for the new `Suspense`/fetch.

- [ ] **Step 4: Confirm no backend files changed** (this phase is
  frontend-only by design — see Global Constraints):

```bash
git diff --stat main -- crates/
```

  Expected: empty output.

- [ ] **Step 5: No further commit** — Tasks 1-5 already committed their own
  work; this task is verification-only.
