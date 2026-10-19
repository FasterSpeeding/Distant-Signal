# Arrive-by on the plan page, and overnight deadlines on `/Trips/plan`

Date: 2026-10-19. Status: implemented on branch `arrive-by-planner`.

## 1. Starting point

The engine and the API already plan arrive-by journeys. That was built on
2026-09-29 (`2026-09-29-trips-plan-arrive-by-avoid-design.md` §3.2) and
extended by the via and max-changes spec (2026-10-06) and the OR-group spec
(2026-10-07). In particular:

- `GET /Trips/plan?...&arriveBy=HH:MM` returns the latest-departing journeys
  that arrive by the deadline. Sending it with `departAfter` is a 400.
- `fastest` returns one journey: the latest departure that arrives in time,
  and of those leaving at that minute, the earliest arrival.
- `options` returns one journey per change count up to `maxChanges`,
  fewest changes first. Each leaves strictly later than the one before.
- Waypoints, OR-group waypoints and vias, `maxChanges`, avoid lists,
  minimum change times and the live overlay all apply. They are covered by
  the brute-force agreement tests in `crates/trip-planner/src/reverse.rs`
  and `staged.rs`, by `modal_change_buffer.rs`, and by the API DB tests.

Two things were missing:

1. **The plan page could not ask for it.** `PlanTripForm` only had "Depart
   after". There was also no way to page to earlier or later journeys in
   either mode.
2. **A deadline after midnight could not be expressed.** `arriveBy` is a
   clock time on the service date `date`. "Arrive by 00:30" on Saturday
   morning is served by Friday's late trains: service date Friday, deadline
   24:30. The API had no way to say that, and paging past midnight hit the
   same wall.

This spec covers those two gaps. It does not add a second engine.

## 2. Algorithm (unchanged, restated)

Arrive-by uses the backward Connection Scan in
`trip_planner::reverse`. It walks the day's connections that depart by the
deadline in reverse order, keeping for each stop (and waypoint or via stage)
the latest time a traveller could be there and still make the deadline.
Every interchange rule mirrors the forward CSA. `latest_departures_by_trips`
is the round-based (RAPTOR-style) variant that gives the latest departure
for each number of trains, for `options`.

The itinerary itself is then rebuilt by the ordinary forward search from
that latest departure. So an arrive-by itinerary comes from the same code
as a depart-after one.

This is the standard reverse search. It reuses the engine's connection
arrays, interchange data, restrictions and staged labels rather than
building a separate engine. Nothing in it changes here.

**Overnight.** The engine works in minutes from the service date's
midnight, and a late train's post-midnight calls are already stored past
1440, for example 00:20 the next morning is 1460. A deadline of 1470 is
therefore just a larger number to the scan. The only change is letting the
request carry it (§3).

## 3. API (additive)

### 3.1 `dayOffset`

```
GET /Trips/plan?...&(departAfter|arriveBy)=HH:MM[&dayOffset=0|1]
```

- `dayOffset=1` puts the given time on the day after `date`, so
  `date=2026-10-23&arriveBy=00:30&dayOffset=1` means "by 00:30 on
  24 Oct, on 23 Oct's trains". The search deadline is `1440 + 30`.
- With `departAfter` it means the same: depart after that time on the next
  morning, on `date`'s trains. That is mainly for paging (§4.3).
- Absent or empty means 0, so every existing request is unchanged.
- These are 400s: a value other than 0 or 1, and `dayOffset` without
  `departAfter` or `arriveBy`.
- The top-level `arriveBy` echo already has the shape
  `{time, dayOffset}` and now carries the offset. A new top-level
  `departAfter` echoes `{time, dayOffset}` for a depart-after request.
  It is the effective start, so 00:00 when none was given, and `null` for
  arrive-by.

### 3.2 Unchanged

- The `departAfter` / `arriveBy` exclusivity 400 and every other
  validation.
- **Order.** Results keep the existing convention, which is the one
  ds-mcp already reads. `fastest` returns one journey. `options` lists
  fewest changes first, and in arrive-by mode each one departs strictly
  later than the one before. A client wanting latest departure first can
  sort `journeys` by `departure`. The plan page does not re-sort, so it
  matches the API.
- **Tie-break.** Among journeys leaving at the same latest minute,
  `fastest` takes the one that arrives first: the forward search rebuilt
  from that departure is an earliest-arrival search. Beyond that, the
  choice is the forward search's, the same as for depart-after.
- **No result.** It stays a 200 with an empty `itineraries` and a
  `noResultReason`. `constraint: "arriveBy"` says nothing arrives in time
  and gives the earliest arrival.

ds-mcp needs no change. It never sends `dayOffset`, and it gains only the
extra `departAfter` echo, which is a new key.

## 4. Plan page

### 4.1 The form

- A `SegmentedControl` labelled "Search by", with "Depart after" and
  "Arrive by", sits next to the time field. The field is labelled "Time".
  It is optional for depart-after, where an empty time searches from
  00:00 as before, and required for arrive-by, where Find routes stays
  disabled until there is a time.
- The copy stays "Depart after", not "Depart at", because the search
  returns trains leaving at or after the time, and "at" would promise an
  exact match. The style guide asks us to say what is true.
- Switching modes keeps the time the visitor typed.
- **URL state.** `/plan` writes `departAfter=HH:MM` or `arriveBy=HH:MM`
  into its query string, never both. That is the calendar date and the
  time the visitor sees. On reload the form restores the mode and the
  time. A hand-edited URL carrying both keeps `arriveBy`. The API would
  400 on the pair, and the form never sends both.

### 4.2 Overnight mapping

The form's date is the calendar date the visitor means. When the mode is
"Arrive by" and the time is before 03:00, the request goes to the
**previous** service date with `dayOffset=1`. Saturday 00:30 becomes
`date=Friday&arriveBy=00:30&dayOffset=1`. Those are the trains that can
get someone there by then.

03:00 is `OVERNIGHT_CUTOFF` in `lib/tripPlan.ts`. It is after the last
regular late-evening arrivals and before the first morning trains. It
misses trains timetabled on Saturday itself that leave between 00:00 and
the deadline. Those are rare.

Depart-after keeps the calendar date as the service date. Moving it would
lose the next morning's trains, because a search on Friday's service ends
with Friday's last train.

### 4.3 Earlier and later

Under the results there are two buttons, "Earlier routes" and "Later
routes". Each runs one request.

- **Later routes**: depart after the earliest departure shown, plus one minute.
- **Earlier routes**: arrive by the latest arrival shown, minus one minute.

So "Later routes" always switches to depart-after and "Earlier routes"
always switches to arrive-by, in either mode. That is the only one-request way to page
backwards from a depart-after result, and forwards from an arrive-by
result. The form, the summary line and the URL are updated to the paged
search, so what the visitor sees, what the URL reopens and what was asked
for always agree.

It always moves on. Every journey on a Later page leaves after the
earliest departure of the page before, and every journey on an Earlier
page arrives before the latest arrival of the page before. In `options`
mode a page can repeat a journey from the previous page. That beats
skipping one.

Times past midnight are anchored on `journeys[j]` with its `dayOffset`,
then turned back into a calendar date and time, so they go through §4.2's
mapping like a typed time. With a waypoint, `journeys` is the whole-journey
summary, so paging moves the whole journey.

The buttons show once a search has found a journey. When a page has
nothing, both buttons page from the last plan that had a journey, so the
visitor can step back the other way. A new search from the form starts
afresh. The buttons are disabled while a search runs.

### 4.4 Results copy

A summary line above the segments says what was asked for: "Arrive by
09:00, 23 Oct 2026" or "Depart after 08:30, 23 Oct 2026". The date is
the calendar date the visitor chose, not the service date of an overnight
mapping. Each itinerary card is unchanged.

`noResultHint` gains two hints:

- `arriveBy`: "Try a later arrival time, or search by departure time."
- `departAfter`: "Try an earlier departure time, or search by arrival
  time."

All copy follows `docs/style-guide.md`: sentence case, no em dashes,
ellipses or arrows.

## 5. Edge cases

| Case | Behaviour |
|---|---|
| Arrive by 00:30, service on the previous evening | Previous service date with `dayOffset=1` (§4.2). Found. |
| Arrive by 02:00, nothing that late | `noResultReason.arriveBy` with the earliest arrival. "Earlier routes" still works from the last plan that had a journey. |
| Later from a 23:58 departure | Depart after 23:59. Then from 23:59 to the following calendar day's 00:00, which searches that calendar date. A train from the previous service date leaving after midnight is not offered on that page. Known limitation, see §8. |
| Same origin and destination | The same answer as depart-after: a 200 with no journeys and `noResultReason.constraint: "noRoute"`. A DB test pins it (§7). |
| Via, OR-group via, waypoints | Unchanged engine paths (`staged_arrive_by`). The summary's anchors come from `journeys`, so paging is whole-journey. |
| Live overlay | Unchanged: a live arrival past the deadline makes the itinerary `liveFeasible: false` and triggers a re-plan within the budget. |
| Both times in a hand-edited URL | The form keeps `arriveBy`. The API would 400, but the form never sends both. |

## 6. Performance

Arrive-by is the existing code path. Its cost was measured in the
2026-09-29 spec §5 and re-measured for this spec with
`crates/trip-planner/tests/bench_arrive_by.rs`. That is a release build on
the synthetic day: 27.9k trains and 401k connections, against about 290k in
production. The numbers are in §6.1.

- `dayOffset` adds nothing: it is an addition to the deadline.
- The overnight mapping can read the previous date's graph. The graph
  cache keeps two dates (`TRIP_PLAN_GRAPH_CACHE_DATES`), so a late-night
  search may cost one graph build. That is the same as any first search
  for a date.
- Paging is one request per click, the same as a new search. It counts
  against the existing `trip_plan` rate limit (20 a minute, burst 10,
  anonymous). Nobody pages that fast.
- The live overlay's look-back seeds are already skipped in arrive-by
  mode.

### 6.1 Measured

`cargo test --release -p trip-planner --test bench_arrive_by -- --ignored
--nocapture`, on 2026-10-19, 10 origin-destination pairs, median of 5. The
bench's five tests run concurrently, so the absolute numbers are a little
high. The ratios are what matter.

| Search | Depart after 08:00 | Arrive by 12:00 |
|---|---|---|
| CSA (`fastest`), median | 14.6 ms | 16.0 ms |
| CSA (`fastest`), max | 120.2 ms (an unreachable pair) | 36.0 ms |
| RAPTOR rounds (`options`), median | 575.1 ms | 131.4 ms |
| RAPTOR rounds (`options`), max | 692.7 ms | 339.7 ms |
| CSA with `avoidStop`, median | 13.9 ms | 24.2 ms |

Without waypoints, arrive-by `fastest` costs about the same as
depart-after, and arrive-by `options` is cheaper, because the backward
rounds only search up to the deadline. With 4 waypoints and 3 vias,
arrive-by `fastest` costs about twice depart-after: 402 ms against 192 ms
by CSA. Arrive-by `options` is again cheaper: 2.9 s against 4.3 s at six
rounds. Both are below the same rows in the 2026-10-06 spec §4.2 (CSA
0.80 s and 1.3 s), so nothing here moves an existing budget. A page of
"Earlier routes" or "Later routes" is one of these searches.

## 7. Tests

- Engine (Rust unit tests): the latest-departing direct journey, with vias
  and `maxChanges`, and an overnight deadline past 1440. Some of these
  exist already. New tests add the overnight deadline and, alongside the
  existing via test, a `maxChanges` cap on arrive-by.
- API (DB tests): `dayOffset=1` reaches a post-midnight arrival. The
  `departAfter` echo. The `dayOffset` 400s. Arrive-by with the same
  origin and destination answers as depart-after does.
- Frontend (vitest): the toggle, URL round trip, overnight mapping,
  paging anchors and the summary line.
- Playwright: an arrive-by plan end to end against a seeded network, and
  visual baselines of the plan form in both modes.

## 8. Open questions

- A deadline or start near midnight could search both service dates and
  merge. That would remove §5's paging gap and §4.2's rare miss, at the
  cost of two graph reads. Not done here.
- "Depart at" was the suggested label. This spec keeps "Depart after" for
  accuracy (§4.1).
