# Design: the line page's "Trains on this line"

Status: phases 1 and 2 implemented 2026-10-06; phase 3 planned below.
Builds on `2026-10-06-line-membership-design.md` (`scope`, `direction`,
`lineDue`).

## 1. Problem (measured 2026-10-06)

`/lines/[id]` rendered every train of the day: 2,890 rows on the South West
Main Line. Each view fetched the default `/public/lines/{id}/trains`
(~21 MB, uncached) and produced a 7.7 MB page of ~27k elements; rows were
sorted by origin working time, running trains fell into "earlier", and 379
rows had no destination. Only `callingPoints[0]` of the ~23 calling points
per train was used.

## 2. Decisions (user, 2026-10-06)

- Main list: `line` trains. `shared` trains in a collapsed "Also running
  along part of this line (N)" group by operator and route. `touch` trains
  not listed; "Other trains at <hub> →" links to station timetables.
- Window now−30 min to now+2 h (desktop), +1 h (phone); plain
  Earlier/Later links; the window stated in text; a "Running now" section.
- Direction tabs labelled by terminus; "Loop" only when loop trains exist.
- API: default `/trains` unchanged; an opt-in `view=summary`.
- Phase 2: From/To picker on `/public/trains/search`; pattern grouping with
  a frequency summary; a key-station strip with a text alternative.
- Phase 3: full-day `/lines/[id]/timetable` on a precomputed table.
- URL: `dir`, `at`, `from`, `to` (and `view`); times at the on-line
  station (public); always a destination; mobile single-line rows, the
  whole row a link; a "Bus" badge when `serviceMode` says so.

## 3. API: `view=summary`

Contract in `docs/api-changelog.md` (2026-10-06). Implementation
`crates/api/src/routes/line_trains_summary.rs`,
`queries::list_line_train_summary_rows`.

- One SQL pass over the population returns every entry's small fields
  (for counts) and calling points only for entries whose `lineDue` is in
  the window or within 6 h before `at` (running candidates).
- The population's has-scope test and array are computed once in a
  materialized CTE. Naming `p.population` in a per-element expression
  re-detoasts the whole population per element (~26 ms each on the SWML):
  the merged membership work's default `/trains` timed out at 30 s on a
  production-shaped copy; fixed for both queries.
- On-line stops: public calls whose TIPLOC maps (crosswalk + `crs_aliases`)
  to a catalogue station. Destination/origin: the nearest calling point
  from that end resolving to a bookable CRS; the last/first on-line call
  otherwise.
- Running at `at`: `lineDue ≤ at ≤ last on-line arrival + max(delay, 0)`,
  not cancelled, not `completed`.
- Live state: `get_public_train_states_for_line` for the listed and
  running-candidate UIDs only.
- `serviceMode`: the population's `service_mode` when a later
  `schedule-reference` publishes it, else CIF Train Status.

## 4. Frontend

`app/lines/[id]/LineTrainsResults.tsx` (server component),
`LineTrainRow.tsx`, `LineTrains.module.css`, `lib/lineTrains.ts`.

- The page fetches the desktop window once; rows past the phone window
  carry `data-beyond-phone` and are hidden below 48em by CSS, and the
  window text and Earlier/Later links exist in a desktop and a phone
  variant. Everything works without JavaScript (links, GET form,
  `<details>`).
- Station picker: `?from=&to=` → `/public/trains/search?station=&stops_at=`
  for the window (times are the departure from `from`); live status is
  borrowed from the summary for trains it also lists.
- "By route" (`view=routes`): groups by origin, destination and on-line
  stops; fast/semi-fast/stopping by stop count among patterns of one route;
  "every N min · xx:MM" when evenly spaced (±1 min).
- Hubs: catalogue stations with role `terminus` or `junction`.

## 5. Phase 3 plan (not implemented)

Goal: `/lines/[id]/timetable?date=&dir=&from=&to=&at=&scope=` -- the full
day, cursor-paged, without expanding the population JSONB per request
(today's floor is ~0.3-0.6 s of SQL for the SWML even for a 1-minute
window).

**Table** (migration in its own timestamp range, transactional, starting
`SET LOCAL lock_timeout = '5s';`; the index in the same file is fine
because the table is new):

```sql
CREATE TABLE line_train_summaries (
    line_id      TEXT     NOT NULL,
    service_date DATE     NOT NULL,
    uid          TEXT     NOT NULL,
    scope        TEXT,            -- line | shared | touch | NULL (pre-membership)
    direction    TEXT,            -- up | down | loop
    due_minute   INTEGER,         -- lineDue as minutes after the service date's midnight
    end_minute   INTEGER,         -- last on-line public arrival
    operator     TEXT,
    service_mode TEXT NOT NULL DEFAULT 'train',
    origin_crs   TEXT,
    destination_crs TEXT,
    on_line_stops JSONB NOT NULL DEFAULT '[]', -- [{crs, minute}]
    PRIMARY KEY (line_id, service_date, uid)
);
CREATE INDEX line_train_summaries_window_idx
    ON line_train_summaries (line_id, service_date, scope, due_minute, uid);
```

**Writer.** Populate it in `api` inside `post_schedule_line_population`'s
upsert transaction (delete the `(line_id, service_date)` rows, insert the
derived rows), reusing `line_trains_summary::on_line_stops`/`endpoint_crs`
with the crosswalk read once per publish. This needs no new
`schedule-reference` endpoint; the approved schedule-reference change
would only be needed if `api` should not do the derivation (alternative:
`schedule-reference` sends the slim rows alongside the population, version
-skew-safe as an optional field). Prune with the population
(`schedule_line_population` retention). Backfill: the next CIF publish
rewrites every row; a one-off `api` admin task can derive existing dates.

**Reader.** `view=summary` switches to the table when rows exist for the
date (falls back to the JSONB path otherwise), and a new
`GET /public/lines/{id}/timetable?date&scope&direction&from&to&limit&after`
pages with a keyset cursor `(due_minute, uid)`, the same shape as
`/public/trains/search`. Live state stays per page.

**Page.** `/lines/[id]/timetable` server component with the same row
component, URL-param filters (`date`, `dir`, `from`, `to`, `at`,
`scope=line|shared`), `LoadMoreControl`-style "Later" cursor links, and a
link from the line page ("Full day's timetable →").

**Tests.** Migration checks (`migration_index_locking`,
`migration_checksums`, `check-migration-order.py`); DB tests that the
upsert writes the derived rows and replaces them, that the reader matches
the JSONB path train for train on a fixture, and the cursor paging;
vitest for the page's filters and paging.

## 6. Risks

- Calling-point SQL cost remains (phase 3 removes it).
- Early-morning London hours: the page asks for today's service date
  only, so yesterday's trains running past midnight are not shown before
  03:00.
- "Running now" looks back 6 h (very long cross-country runs drop out).
- Hubs come from catalogue roles, not measured touch counts.
- Station-pair rows come from `/public/trains/search` (WTT `from`/`to`
  bounds, any operator) and have live status only when the summary also
  lists the train.
