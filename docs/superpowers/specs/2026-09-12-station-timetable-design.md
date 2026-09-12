# Design: Station Timetable (collapsed CIF-schedule departures on the station page)

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline) — same posture
this repo's other same-day design docs already take.**

## Required reading consumed in full before this document was written

`frontend/app/stations/[crs]/page.tsx` (whole file); `crates/api/src/routes/lines.rs`
(`get_line_schedule`, `get_line_trains`, whole file including `db_tests`);
`crates/api/src/routes/trains.rs` (whole file, including its module doc
comment and every `db_tests` case); `crates/api/src/data/queries.rs`
(`get_schedule_line_population`, `CallingPointDepartureRow`,
`list_calling_point_departures_for_train`, `search_schedule_calling_point_departures`,
and the `schedule_destination_departures_query_tests` module); every
`schedule_destination_departures` migration
(`20260907130000_schedule_destination_departures.sql` through
`20260910100000_schedule_destination_departures_calling_point_arrival.sql`,
nine files, all read in full); `crates/api/src/render.rs`
(`calling_point_departure_json`, `schedule_departure_json`,
`station_departure_json`); `crates/schedule-query/src/records.rs`
(`CallingPoint`, `CallingPointKind`, `LinePopulationEntry`); `frontend/app/trains/page.tsx`;
`frontend/components/TrainSearchForm.tsx` (whole file, including its own
doc comment); `frontend/components/IssueList.tsx` (the `Accordion`
section, lines ~300-417); `frontend/app/api/[...path]/route.ts`; and the
following prior specs: `2026-09-07-train-listing-page-design.md`,
`2026-09-07-train-listing-destination-search-sizing-design.md`,
`2026-09-08-calling-point-train-search-design.md`,
`2026-09-08-destination-arrival-time-filter-design.md`,
`2026-09-08-journey-timetable-overlay-design.md`,
`2026-09-09-stops-at-search-filter-design.md`,
`2026-09-09-trains-search-multi-day-design.md`,
`2026-09-09-mcp-schedule-data-follow-up-design.md`.

## 0. Corrections to the brief's assumptions — this is the load-bearing section

The brief's framing needs three corrections before any UI work is designed
against it. All three were confirmed by reading real code, not assumed.

### 0.1 `schedule_line_population` is the wrong source, and not just "line-scoped instead of station-scoped"

`schedule_line_population` (backing `get_line_schedule`/`get_line_trains`)
is keyed by `(line_id, service_date)` and stores, per UID, the *entire*
resolved calling-point list for every service on one **catalogued line**
(`lines/*.toml`). Deriving "every scheduled calling point at station X"
from it would mean: enumerate every catalogued line whose `stations` list
contains X, fetch each line's whole-day population, deserialize every
entry's `calling_points` JSON, and filter each one down to the rows whose
TIPLOC resolves to X — a fan-out over an a-priori unbounded set of lines,
repeated on every station-page view, for data this app already stores in
directly queryable form elsewhere (§0.2). Worse, it only covers stations
on a catalogued line at all — most CRS codes are not (this is exactly the
gap `schedule_destination_departures` was built network-wide to close;
see `2026-09-07-train-listing-page-design.md`). `schedule_line_population`
is the wrong table for this feature, full stop, not merely a less
convenient one.

### 0.2 `schedule_destination_departures` is the right source, and it is already indexed for exactly this query

Confirmed by reading `20260908120000_schedule_destination_departures_calling_point_search.sql`'s
own header comment and the index it creates:

```sql
CREATE INDEX schedule_destination_departures_calling_point_idx
    ON schedule_destination_departures (service_date, origin_crs, scheduled, train_uid);
```

`origin_crs` is already, per that migration's own comment, "the calling
point of this row" — one row exists per departure-bearing calling point of
every non-cancelled CIF schedule, network-wide, regardless of line
catalogue membership. "Every scheduled calling point at station X" is
therefore already exactly `WHERE service_date = ? AND origin_crs = 'X'
ORDER BY scheduled` — a bounded index range scan, not a fan-out. This is
not a coincidence: this index and column were added specifically to
support "calls at this station" lookups (`2026-09-08-calling-point-train-search-design.md`),
and this feature is squarely that same question asked from the station
page instead of the `/trains` search form.

### 0.3 The query AND the route already exist — this feature needs zero new backend code

This is the corollary of §0.2 that changes the shape of the whole
deliverable. `crates/api/src/data/queries.rs::search_schedule_calling_point_departures`
already implements exactly this lookup — service-date-scoped, keyset-paginated,
`now`-forward-by-default on today, with the honest 404 (nothing published
for that day) vs. `200` empty-results (published, but no matches)
distinction already built in. It is wired up, publicly and
unauthenticated, as `GET /public/trains/search?station=<crs>&date=&from=&to=&limit=&after=`
(`crates/api/src/routes/trains.rs`), and the frontend already has a
same-origin proxy for it (`frontend/app/api/[...path]/route.ts`) and a
consuming component (`TrainSearchForm.tsx`) that calls it exactly this
way today.

**Decision 1: no new backend route, no new query parameters, no new
migration.** The station-timetable feature is a new *frontend* surface —
a stripped-down, always-`station`-fixed consumer of the existing `GET
/public/trains/search` — not a new backend capability. This is the
"least-new-code path" investigation point 3 asked for, taken as far as it
goes: the honest answer is that the path has zero new backend code on it
at all. `get_schedule_line_population`/`get_line_schedule`/`get_line_trains`
are not touched by this design and are not fit for this feature (§0.1).

### 0.4 A genuine, inherited scope gap: this shows departures, not a full arrivals-and-departures board

`schedule_destination_departures` stores one row per **departure-bearing**
calling point (`CallingPointKind::Origin` or `::Intermediate` — see
`schedule_query::records::CallingPoint`). A `Terminate` calling point has
no `booked_departure` and, per the table's own migration comment, "never
gets its own row here." **Consequence: a train that terminates at the
station this page is showing will not appear in this timetable at all** —
it has no departure-bearing row at this CRS to match on. This is not a bug
introduced by this design; it is the exact same limitation
`GET /public/trains/search` and `TrainSearchForm.tsx` already carry and
already document (`"stops_at": a value naming a schedule's true
terminating calling point never matches`, `2026-09-09-stops-at-search-filter-design.md`)
and the same one `2026-09-08-journey-timetable-overlay-design.md` §0.2
names for the *train*-scoped read of this same table ("Departure time
only, never arrival ... Acceptable for a schedule-first structural
display"). This document inherits that limitation rather than re-solving
it, and the UI copy must say "departures," not "timetable" or "every
train calling here," so it does not overclaim what the data actually
covers. See Non-goals.

### 0.5 No headcode, no operator — CIF-derived rows never carry either in this codebase

Confirmed at two independent points: `render.rs::station_departure_json`'s
own comment ("`headcode` is deliberately omitted — always `None` at the
source") for the LDBWS side, and `render.rs`'s calling-point/journey-stop
tests (`headcode: None`, asserting `json.get("headcode").is_none()`) for
the CIF side — there is no code path anywhere in this repo that resolves
a real headcode for a CIF schedule row. Operator is explicitly out of
scope for the exact same reason `trains.rs`'s own module doc comment gives
for `/public/trains/search`: "the CIF SCHEDULE feed's operator field is
parsed-but-undecoded everywhere in this codebase." **The brief's candidate
field list ("time, destination, calling points, train_uid/headcode,
operator") is corrected to: time, destination, origin, train_uid. No
headcode, no operator, ever, for this data source.** This matches
`TrainSearchForm.tsx`'s own row rendering exactly (`row.originCrs ?? '?'} →
{row.stationCrs} → {row.destinationCrs ?? '?'}`, no headcode/operator
column) — not a new gap this feature introduces, the same one every
sibling CIF-schedule surface already lives with.

### 0.6 Cross-linking precedent already exists and this design reuses it unchanged

`TrainSearchForm.tsx` already renders, per result row, `<TextLink
href={/train/${uid}/${date}}>View live status</TextLink>` next to a
`TrackThisTrainButton`. `GET /Train/by-uid/{uid}/{date}` is public,
unauthenticated, and (per `2026-09-09-mcp-schedule-data-follow-up-design.md`
§3) already backed by shared train identity — a UID with no prior
`trains` row yet renders an honest "not yet resolved" state on that page
rather than erroring. **Decision 2: the station timetable cross-links each
row to `/train/{uid}/{date}` exactly the same way**, deliberately not
inventing a second pattern for the same fact. A schedule row and a live
departure-board row are never merged into one entry on this page — they
stay two separate sources (schedule timetable in the new accordion; LDBWS
live departures wherever this station's live board already lives, if it
has one), consistent with this feature description's own framing
("collapsed, so it doesn't compete with ... the primary live-departures
view"). See Non-goals for the explicit "no live-board" of it.

## 1. Goal and scope

Add a collapsed-by-default "Scheduled departures" section to
`frontend/app/stations/[crs]/page.tsx`, showing the CIF-schedule-derived
list of trains that depart from this station (§0.4), for the rest of
today by default, cross-linked to each train's live status page. Backed
entirely by the existing `GET /public/trains/search?station=<crs>` route
(§0.3) — no backend change.

## 2. Backend

**None.** `search_schedule_calling_point_departures`,
`calling_point_departure_json`, and the `GET /public/trains/search` route
are reused completely unchanged — same query, same response envelope
(`{ "results": [...], "nextCursor": string | null }`), same row shape
(`uid`, `scheduled`, `stationCrs`, `originCrs`, `destinationCrs`,
`destinationArrival`, `destinationArrivalDayOffset`), same 404-for-
unpublished-day vs. 200-empty-for-no-matches distinction, same
`now`-forward-on-today default, same keyset pagination
(`limit`/`after`/`nextCursor`), same `SEARCH_WINDOW_FORWARD_DAYS`/
`SEARCH_WINDOW_BACKWARD_DAYS` window. The station page passes exactly one
query parameter the form-based `/trains` page also supports:
`station=<crs>` (uppercased, matching `normalize_crs`). No `date`, `from`,
`to`, `origin`, or `stops_at` are sent by this consumer in v1 (see
Non-goals) — omitting them exercises the API's own existing "no explicit
`from`, date is today" defaulting path unchanged.

## 3. Frontend

### 3.1 New component: `frontend/components/StationTimetable.tsx`

A `'use client'` component, `<StationTimetable crs="RDG" />`, rendered
from the (server) `StationDisruptionPage` alongside the existing sections,
near the bottom of the page (after the sample-stats block, as the least
load-bearing section on the page — see the brief's own framing that this
must not compete with the primary live-departures content above it).

**Decision 3: reuse `Accordion`/`AccordionItem`/`AccordionControl`/
`AccordionPanel` from `@mantine/core`, the exact same import shape
`IssueList.tsx` already uses**, rather than a bespoke disclosure widget.
One `AccordionItem` (not `multiple` — there is only one section, unlike
`IssueList`'s per-status accordion), collapsed by default (Mantine's own
default — no `defaultValue` is set).

**Decision 4: `keepMounted={false}` on the `AccordionPanel`**, matching
`IssueList.tsx`'s own documented reasoning verbatim: Mantine v9 keeps a
collapsed panel's content mounted (via the Activity API) purely
visually hidden by default, which means `screen.queryByText` (and a
screen reader in "not visible" mode, inconsistently) can still find
supposedly-hidden content. `keepMounted={false}` makes "collapsed by
default" actually mean "not rendered" until first expanded, which is the
literal ask ("collapsed/hidden by default").

**Decision 5: fetch on every expand, no cross-expand cache.** Because the
panel unmounts on collapse (Decision 4), its component-local state
(`results`, `nextCursor`, pagination) is naturally discarded on collapse
and a fresh `GET /api/trains/search?station=<crs>` fires again on the next
expand. This is deliberate, not an oversight: the data is a live,
`now`-forward window (§2) that goes stale the moment real time passes it,
a station timetable is opened rarely enough per visit that a repeat fetch
costs nothing meaningful, and it avoids a second, parallel caching
mechanism next to the one-`AccordionItem`-owns-its-own-`useState` model
`IssueList.tsx`'s sibling code already uses for its own per-item detail
fetch-free rendering. No `localStorage`/SWR/react-query layer is
introduced for this.

**Decision 6: default window is "now-forward, today, paginated with Load
more" — not a fixed "next 2 hours."** The brief asked for this to be
decided and justified. Rejected: a fixed 2-hour window, because (a) it
requires new client-side wall-clock arithmetic across a UK
midnight/DST boundary that `TrainSearchForm.tsx`'s own module comment
already flags as an unsolved, accepted imprecision for browser-local time
math in this codebase (no `frontend/`-side timezone library exists, per
that comment); and (b) it produces an ambiguous empty state — "nothing in
the next two hours" reads as "nothing scheduled" to a rider even when the
next departure is 15 minutes past the cutoff, at a quiet station this is
actively misleading. **Chosen: omit `from`/`to` entirely and let the
existing route's own `now`-forward default apply** (§2), rendering
`DEFAULT_SEARCH_LIMIT` (50) rows with a "Load more" button reusing the
existing `nextCursor`/`after` keyset — the exact mechanism
`TrainSearchForm.tsx::handleLoadMore` already implements, copied with the
`station` value fixed to this page's own `crs` and no other filter fields
exposed. A single indexed range scan bounded by `LIMIT 51` per page (see
`search_schedule_calling_point_departures`'s own `fetch =
limit.saturating_add(1)`) means even a very busy terminus costs the same
per page as a quiet request stop; there is no station-specific cap logic
to write.

### 3.2 What's shown once expanded

Per row (one `Group` per result, mirroring `TrainSearchForm.tsx`'s own
row layout): scheduled departure time (`HH:MM`, already trimmed
server-side), origin CRS (`originCrs`, the schedule's true first calling
point — `?` when unresolved, same null-handling as the search form),
destination CRS (`destinationCrs`), and `uid` as a `<TextLink
href={/train/${uid}/${date}}>` — "View live status" — using today's date
(`dayjs().format('YYYY-MM-DD')`, computed once at first render; there is
no date picker on this stripped-down view, see Non-goals). No station
name resolution (bare CRS codes only, matching `TrainSearchForm.tsx`
exactly — deliberately not a new enhancement bundled into this feature).
No calling-points list, headcode, or operator column (§0.4, §0.5). A
one-line disclaimer identical in spirit to `TrainSearchForm.tsx`'s own
("These are from the scheduled timetable, not live running information
...") sits above the row list.

### 3.3 Empty/error/unpublished states

Reuses `TrainSearchForm.tsx`'s own three-way split, since it is fetching
through the identical route:

- **Loading** — "Loading scheduled departures…"
- **Error** (non-2xx, non-404, or a thrown fetch) — an inline `Alert`,
  "Couldn't load the scheduled departures right now."
- **404 (unpublished)** — "Today's scheduled timetable data isn't
  available yet." Distinguished from the next case; collapsing the two
  would repeat exactly the mistake `StationDisruptionPage`'s own
  `fetchStationDisruptions`/`fetchStationSampleStats` helpers were written
  to avoid (this page's own file, lines 23-33 and 81-89) — a station this
  feed simply doesn't cover must not read the same as "nothing's running
  right now."
- **200 with an empty `results`** — "No scheduled departures found for the
  rest of today." (Genuinely possible: a minor request stop late at
  night, or a station whose only services all terminate here — §0.4.)
- **200 with rows** — the list from §3.2, plus "Load more" while
  `nextCursor !== null`.

### 3.4 Escape hatch to the full search

A trailing link, "Search a different day or filter →
`/trains?station=<crs>`", reusing `TrainsPage`'s existing `?station=`
prefill convention (`frontend/app/trains/page.tsx`) unchanged. This gives
a rider who wants a different date, an origin/stops-at filter, or "Track
this train" a path to the full-featured page without this component
reimplementing any of it — consistent with `TrainSearchForm.tsx`'s own
"Track it manually" escape hatch to `/track` for the gap it can't close.

## 4. Non-goals (explicit)

- **No live updates inside the expanded panel.** No polling, no
  `AutoRefresh` integration. A rider who wants to see whether a specific
  service is now running late clicks through to `/train/{uid}/{date}`
  (§0.6), which already has its own live-status machinery. Re-expanding
  the accordion (Decision 5) is the only way this panel's data refreshes.
- **No filtering or search inside the panel** (no origin/stops-at/time
  fields). The `station` value is fixed to the page's own CRS; anyone
  wanting `origin`/`stops_at`/a different date uses the escape hatch
  (§3.4) to the already-fully-featured `/trains` page rather than this
  page re-implementing a second copy of that form.
- **No date picker.** Always "today, now-forward." A different day is
  reachable only via §3.4.
- **No arrivals, no full arrivals-and-departures board.** §0.4 — a train
  terminating at this station will not appear. Not fixable within this
  design without a schema/producer change to `schedule_destination_departures`
  (out of scope; would need its own dedicated design, same as any other
  change to that table per its own migrations' "do not add ... without a
  measured reason" posture).
- **No headcode, no operator column**, ever, for this data source (§0.5).
- **No merging with the live LDBWS departure board.** The two data
  sources stay visually and structurally separate on this page — this is
  the brief's own explicit ask, not a limitation this design is working
  around.
- **No station-name resolution for origin/destination CRS codes.** Bare
  CRS codes only, matching `TrainSearchForm.tsx`.
- **No new backend route, query parameter, index, or migration** (§0.3).
- **No caching layer (SWR/react-query/localStorage) for the fetched
  rows** (Decision 5).

## 5. Testing

### 5.1 Backend

**No new backend tests are required, because no backend code changes.**
The exact request shape this feature issues (`GET
/public/trains/search?station=<crs>`, no other parameters) is already
covered by existing, passing `db_tests` in `crates/api/src/routes/trains.rs`:
`trains_search_with_no_explicit_from_still_defaults_to_the_now_floor_on_today`
and `trains_search_omitting_date_still_defaults_to_today` together prove
the exact "station-only, defaults apply" path this component relies on;
`trains_search_renders_camel_case_rows_with_trimmed_time_and_station_attached`
proves the row shape this component's `TrainSearchRow` type parses;
`trains_search_nothing_published_for_today_is_a_404` and
`trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array`
prove the two empty states §3.3 distinguishes; `trains_search_paginates_with_a_cursor_and_after_continues_from_it`
proves the "Load more" mechanics this component reuses. If a future
change to this route's contract (new field, changed default) is made, its
own `#[ignore]`d `db_tests` (run via `cargo test -p api trains_search --
--ignored --test-threads=1` per this crate's convention) already gate it
independent of this feature.

### 5.2 Frontend (Vitest, `frontend/components/StationTimetable.test.tsx`)

Following `IssueList.test.tsx`'s `screen.queryByText(...).not.toBeInTheDocument()`
convention for asserting genuine non-rendering (not just visual hiding),
and `TrainSearchForm.test.tsx`'s `vi.stubGlobal('fetch', ...)` /
`searchCallUrl` helpers for asserting the exact request URL:

1. **Collapsed by default**: renders the accordion control ("Scheduled
   departures") but the row list/loading text is not in the document, and
   `fetch` is never called, before the control is clicked.
2. **First expand triggers exactly one fetch** to
   `/api/trains/search?station=<CRS>` (uppercased), with no other query
   parameters.
3. **Loading state** renders between click and the mocked fetch resolving.
4. **Success with rows** renders each row's time/origin/destination and a
   `View live status` link whose `href` is `/train/<uid>/<today's
   YYYY-MM-DD>` for each `uid` in the mocked response.
5. **Success with an empty `results` array** renders the "No scheduled
   departures found" copy, not the unpublished or error copy.
6. **404 response** renders the "isn't available yet" copy, distinct from
   case 5 (regression coverage for the 0.4/3.3 distinction).
7. **Non-2xx/non-404 response, and a thrown fetch**, both render the
   error `Alert`.
8. **"Load more" behavior**: given a first response with a non-null
   `nextCursor`, clicking "Load more" issues a second fetch with
   `after=<that cursor>` and `station` unchanged, and appends its rows to
   the existing list rather than replacing them; a `null` `nextCursor`
   renders no "Load more" button.
9. **Collapse then re-expand issues a fresh fetch** (Decision 5) rather
   than reusing the previous result set — assert `fetch` call count is 2
   after collapse + re-expand, and that a changed mocked response between
   the two calls is reflected on screen.
10. **The `/trains?station=<crs>` escape-hatch link** (§3.4) is present
    with the correct `href` regardless of expand state.

## 6. Summary of decisions

1. **No new backend route, query parameter, or migration** — `GET
   /public/trains/search?station=<crs>` already answers this question
   exactly (§0.3).
2. **`schedule_destination_departures`, not `schedule_line_population`,
   confirmed as the correct source** — the latter is line-scoped,
   catalogue-gated, and requires a fan-out; the former already has a
   station-keyed index built for this exact shape (§0.1-0.2).
3. **This is a departures list, not a full timetable** — trains
   terminating at this station are absent, an inherited, documented
   limitation shared with every other consumer of this table, not a new
   gap (§0.4).
4. **No headcode, no operator** — neither exists for CIF-derived rows
   anywhere in this codebase (§0.5).
5. **Cross-links to `/train/{uid}/{date}` for live status**, reusing
   `TrainSearchForm.tsx`'s existing pattern unchanged; schedule and live
   data stay visually separate on the station page itself (§0.6).
6. **Collapsed via Mantine `Accordion`/`keepMounted={false}`**, matching
   `IssueList.tsx`'s existing, documented convention (Decision 3-4).
7. **Default window is "now-forward, today, paginated," not a fixed
   time cutoff** — reuses the existing route default and keyset
   pagination rather than introducing new client-side clock arithmetic
   (Decision 6).
8. **No caching across collapse/expand** — a fresh fetch every time the
   panel is opened (Decision 5).
