# Design: the line page's "Trains on this line"

Status: phases 1 and 2 implemented 2026-10-06; phase 3 implemented
2026-10-07 (§5, as built).
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
- `serviceMode` (and `liveTracking`): as the default view -- the
  `schedule_services` mode for the listed trains, else the population's
  CIF Train Status (changed at integration with the bus/ferry work).

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

## 5. Phase 3: the full-day timetable (implemented 2026-10-07)

`/lines/[id]/timetable?date=&dir=&from=&to=&at=&scope=` -- the full day,
cursor-paged, on a table derived at publish time instead of the
population JSONB per request.

**Table** `line_train_summaries` (migration `20261008100000`, one
transactional file; the index is in it because the table is new):
`(line_id, service_date, uid)` key; `scope`, `direction`, `due_minute`
(`lineDue`, minutes after the date's midnight), `end_minute` (last
on-line public arrival), `operator_atoc`, `train_status`, `origin_crs`,
`destination_crs`, `on_line_stops` (`[{crs, minute, arrival}]`),
`has_scope`, `derivation`; index `(line_id, service_date, scope,
due_minute, uid)`. Deviations from the plan:

- `train_status`, not `service_mode`: the mode is resolved at read time
  from `schedule_services` (it knows the Train Category) with the status
  as fallback, as on every schedule surface. Storing the mode would have
  frozen a pre-`schedule_services` answer into the row.
- `has_scope` (the population carries membership) and `derivation`
  (`DERIVATION_VERSION` + a hash of the line's catalogue stations and
  aliases) are new. A reader uses only rows whose `derivation` matches
  its own catalogue, so a catalogue change or a derivation change falls
  back to the JSONB rather than serving stale on-line stops.
- No CHECK constraints on population values: the rows share the
  population's transaction, and a new value must not fail its publish.

**Writer** (`data::line_train_summaries::upsert_population_with_summaries`,
called by `POST /private/schedule-line-population`): the population's
upsert (unchanged statement, now `RETURNING`) and, in the same
transaction, a delete-and-insert of the date's rows -- only when the
population changed or the stored `derivation` differs. The population
text is decoded on a blocking thread into slim entries (only the fields
the summary reads), the crosswalk is read once, and every row comes from
`derive_row`, the same function the JSONB fallback uses. A population
the table cannot hold (a uid listed twice, or a shape the slim decode
rejects) leaves no rows: readers fall back. Pruned by the aggregator with
`schedule_line_population`'s retention. Backfill: the next publish, or
`backfill_line_train_summaries` (in the api image; idempotent).

**Readers.** `view=summary` reads the table when it has current rows for
the line and date, else the JSONB, both through `SummaryRow` (a DB test
compares the two bodies for ten summary and eleven timetable queries).
`GET /public/lines/{id}/timetable` pages in SQL with a keyset cursor
`(time, uid)` -- `time` is the departure from `from` when given, else
`lineDue` -- and falls back to the same page worked out in memory from
the JSONB (`timetable_page_in_memory`, also the SQL's test reference).
Live state and modes are read for the page only.

**Running now** (changed with phase 3): a train is a running candidate
when it is due on the line at or before `at` and its last on-line arrival
is at most the 3 h delay grace before `at` -- its whole span, so a long
run no longer drops out after 6 h. The JSONB path ships calling points
for those candidates using the last calling point as an upper bound of
the run's end.

**Page.** `/lines/[id]/timetable` (server component): a GET filter form
(date: three days back to tomorrow; from; to; from time; this line's
trains or other trains along it), direction chips with the whole day's
counts, the line page's row component (time at `from` or on the line,
arrival at `to`, service-mode badge), and "Load more"
(`LoadMoreControl`, appending in place through the `/api` proxy) with a
`<noscript>` next-page link. The line page links to it ("Full day's
timetable →") with its direction, stations and window start.

**Line page follow-ups (2026-10-07).** Between 00:00 and 03:00 the line
page also fetches the previous service date (its window and `at` plus
24 h) and merges those trains, linked to their own date. On a phone the
hub links show the termini first, then majors, then junctions, three at
most, the rest behind "More stations".

## 6. Risks

- The table follows the crosswalk only at publish: a `tiploc_crs` change
  reaches the rows with the next population publish (or the backfill).
- Rows are written per changed publish: about 300k rows per service date
  network-wide (see the phase 3 measurements in the commit history).
- The JSONB fallback (before the first publish after deploy, or after a
  catalogue change) is as slow as phase 2; the timetable's fallback ships
  every calling point of the day.
- Hubs come from catalogue roles, not measured touch counts.
- Station-pair rows on the line page come from `/public/trains/search`
  (WTT `from`/`to` bounds, any operator) and have live status only when
  the summary also lists the train; the timetable's `from`/`to` are the
  line's own stations and public times.
