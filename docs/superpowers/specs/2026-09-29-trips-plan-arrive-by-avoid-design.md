# `/Trips/plan`: arrive-by, avoid lists and constraint-specific errors

Date: 2026-09-29. Status: implemented on branch `trips-plan-arrive-by-avoid`.

## 1. Why

`/Trips/plan` could only search forward from `departAfter`. It had no way to
steer a route away from a station, and an empty segment gave no reason. The
MCP team's `Distant-Signal-MCP` (`src/tools/plan-journey.ts`,
`dsTripPlanEligible`) sends every `arriveBy` or `via`/`avoid` query to its own
local planner for exactly these reasons. Skye's `train-mcp` has all three
features. The user approved porting from `train-mcp` and `Distant-Signal-MCP`
on 2026-09-29.

## 2. API (additive)

```
GET /Trips/plan?origin=&destination=&date=
    [&waypoints=CRS,...]
    [&departAfter=HH:MM | &arriveBy=HH:MM]
    [&avoid=CRS,...][&avoidStop=CRS,...][&avoidChange=CRS,...]
    [&results=fastest|options][&maxChanges=0..4][&live=true|false]
```

- **`arriveBy`**: the latest acceptable arrival at the destination. Giving it
  together with `departAfter` is a 400. `fastest` returns the single
  latest-departing itinerary that arrives in time; if several leave at that
  minute, the earliest-arriving one. `options` returns, for each number of
  changes up to `maxChanges`, the latest departure that arrives in time. That
  is one itinerary per change count, fewest changes first, each leaving
  strictly later than the one before. `cappedByMaxChanges` is true when a
  later departure with more changes exists.
- **`avoid`**: never ride a train that calls at the station or runs through
  it without stopping, and never board, alight or walk there. This is
  `train-mcp`'s `avoid`.
- **`avoidStop`**: never ride a train that calls at the station. A train that
  runs through without stopping is fine. This is `train-mcp`'s `avoidStop`.
- **`avoidChange`**: never board, alight, change or walk there. Staying on a
  train that calls there is fine. This one is new to Distant Signal and is
  the loosest of the three.
- Each list takes at most 8 codes; more is a 400. An unknown code is a 400
  naming the list (`avoid: 'ZZZ' is not a recognised station CRS code`). An
  avoided origin, destination or waypoint is also a 400.

New response fields (all additive):

| Where | Field | Meaning |
|---|---|---|
| top level | `arriveBy` | `{time, dayOffset}` or `null` |
| top level | `avoid`, `avoidStop`, `avoidChange` | the lists as applied: uppercased, deduplicated, always present |
| segment | `arriveBy` | the deadline this segment was searched for (arrive-by only), else `null` |
| segment | `departAfter` | unchanged; `null` for an arrive-by request |
| segment | `noResultReason` | `{constraint, values, message}` when `itineraries` is empty, else `null` |

`noResultReason.constraint` is one of:

- `maxChanges`: itineraries exist but every one needs more changes than the
  cap.
- `avoid`, `avoidStop` or `avoidChange`: removing that list alone gives an
  itinerary.
- `avoidCombined`: dropping all the avoid lists gives an itinerary, but
  dropping any single list does not.
- `arriveBy`: nothing arrives by the deadline. The message gives the
  earliest arrival.
- `departAfter`: nothing leaves after the start time. The message gives the
  last departure that still gets there.
- `noRoute`: nothing reaches the destination that service day.
- `previousSegment` or `nextSegment`: this waypoint segment was not searched,
  because the segment it chains from found nothing.

An empty segment is still a 200, as before.

### Examples (for the MCP team)

```
GET /Trips/plan?origin=ZVA&destination=ZVC&date=2026-10-05&arriveBy=10:45&avoid=ZVP
{
  "results": "fastest", "maxChanges": 2,
  "arriveBy": {"time": "10:45:00", "dayOffset": 0},
  "avoid": ["ZVP"], "avoidStop": [], "avoidChange": [],
  "segments": [{
    "originCrs": "ZVA", "destinationCrs": "ZVC",
    "departAfter": null, "arriveBy": {"time": "10:45:00", "dayOffset": 0},
    "cappedByMaxChanges": false, "noResultReason": null,
    "itineraries": [{"legs": [{"kind": "train", "trainUid": "TAVE1",
        "scheduledDeparture": "09:40:00", "scheduledArrival": "10:30:00", ...}],
      "changeCount": 0, "totalDurationMinutes": 50,
      "exceedsRecommendedChanges": false, "liveFeasible": true}]
  }],
  "live": {"applied": true, "reason": null, "replans": 0, ...}
}

GET /Trips/plan?origin=ZVA&destination=ZVC&date=2026-10-05&arriveBy=10:00
  segments[0].itineraries: []
  segments[0].noResultReason: {"constraint": "arriveBy", "values": ["10:00"],
    "message": "No itinerary from ZVA arrives at ZVC by 10:00 on 2026-10-05;
                the earliest arrival is 10:30, leaving at 09:40."}

GET /Trips/plan?...&departAfter=09:45&avoid=ZVP&avoidStop=ZVB
  segments[0].noResultReason: {"constraint": "avoid", "values": ["ZVP"],
    "message": "No itinerary from ZVA to ZVC departing after 09:45 on 2026-10-05
                while avoiding ZVP (not even passing through); one exists
                without that restriction."}
```

These shapes come from the DB tests `arrive_by_and_avoid_end_to_end_live_on_and_off`
and `live_a_delay_past_arrive_by_replans_onto_an_earlier_train` in
`crates/api/src/routes/trips.rs`.

Mapping from `train-mcp`/DS-MCP `plan_journey`:

| `plan_journey` | `/Trips/plan` |
|---|---|
| `arriveBy: true` with `time` | `arriveBy=time` |
| `avoid` | `avoid` |
| `avoidStop` | `avoidStop` |
| `viaStop` | `waypoints`, with one difference: DS always charges the waypoint's change time, even when staying on the same train (see §6) |
| `via` (pass through) | not supported (see §6) |
| `LON` group code | not expanded: it is a 400, because it is not a routable CRS |

## 3. Design

### 3.1 Restrictions inside the searches (`crates/trip-planner/src/restrictions.rs`)

`Restrictions` holds three sets, all keyed by normalized TIPLOC. The route
expands each CRS code to all of its TIPLOCs.

- `no_interchange` (from `avoidChange`, and implied by the other two):
  - `ready_source_at` refuses a fresh boarding;
  - `relax` refuses an arrival, so there is no alighting and no walk in or
    out;
  - the backward scan refuses both, symmetrically.
- `no_call` (from `avoidStop` and `avoid`): any connection that starts or
  ends at the station is blocked.
- `pass_legs` (from `avoid`): for each train that runs through an avoided
  station without calling, its base connections in order, with the spanning
  ones marked blocked. A live-overlay replacement connection counts as
  blocked if it spans a blocked base leg. It is also blocked if it cannot be
  placed in the base order at all, which is conservative.

A blocked connection is unusable, and it also ends any ride on its train. Both
forward searches decide "already aboard" by UID alone. If they only skipped
the connection, a passenger who boarded before the avoided station would carry
on after it. `train-mcp` hits the same problem (`pruneConnections`' long
comment, found on a real CrossCountry working). It fixes it by relabelling
each surviving run of a schedule with a synthetic `scheduleId`. Here the
searches instead remove the UID from their "aboard" set when they meet a
blocked connection. Legs therefore keep their real UID, which the live overlay
and `trip_leg_details` key on. The test
`the_forward_searches_never_carry_a_ride_across_a_blocked_connection` fails
without this.

Pass-through data: connections name only their two calls. The untimed pass
rows were already in `schedule_calling_points_full`, but
`build_connections` skipped over them. `schedule_query::build_connections_with_passes`
now also returns a `PassIndex`: for each TIPLOC, the indices of the
connections whose span includes it as a pass row. On a production weekday
(2026-09-29) there are about 194k untimed rows out of 488k, so the index is
about 0.8 MB of `u32`s plus one key per TIPLOC. It is built once per cached
graph. `build_restrictions` turns the `avoid` list into `pass_legs` with one
pass over the day's connections, once per request, on the blocking pool under
the plan permit.

### 3.2 Arrive-by (`crates/trip-planner/src/reverse.rs`)

The core is a backward Connection Scan. It walks the day's connections
(overlay applied, departing no later than the deadline) in reverse. For each
stop it keeps the latest time a traveller may have arrived there and still
make the deadline. Every rule mirrors `csa.rs`:

- A fresh boarding at a non-origin stop costs that stop's
  `minimum_change_time` (`NoInterchange` forbids it). An arrival at any
  same-CRS sibling may use it, charged at the boarding stop's own figure.
- The origin needs no change time. Staying on the same UID is free.
- Fixed links are relaxed backwards. A walk starting at `t` is accepted only
  if the link `fixed_links_from` would pick at `t` (the shortest valid one)
  arrives in time, which is the same choice the forward search makes.
- Restrictions apply as they do going forward.
- The scan stops as soon as a connection departs at or before the best origin
  departure found so far.

`latest_departures_by_trips` is the round-based version. Round *k* answers
"the latest departure using at most *k* trains", reading the previous round's
labels to decide where alighting works. It stops early once a round changes
nothing.

The journey itself comes from the ordinary forward search, run from the
latest departure the backward scan found, over the connections that depart by
the deadline. So an arrive-by itinerary is built by the same code, with the
same legs, as a depart-after one. For `options`, a forward RAPTOR capped at
*k* trains runs from each departure that strictly improves on fewer trains.

If the forward search ever disagreed and arrived late, `scan_connections_arrive_by`
falls back to `train-mcp`'s `latestDepartureFor`. That is a binary search over
departure minutes with the forward search as the probe. It is correct because
earliest arrival never decreases as the departure time increases. A
brute-force differential test checks the backward scan against the forward
searches: 60 random networks, three deadlines, with and without restrictions,
for both CSA and each RAPTOR round count. It found no disagreement, so the
fallback is a safety net only.

Why not just port `train-mcp`'s approach (about 11 forward probes by
bisection)? The backward scan is one pass bounded by the deadline, and it
gives each change count's answer in the same rounds. Bisection would need a
RAPTOR run per probe for `options`.

### 3.3 Waypoints and time (`trip_planning_itinerary::plan_trip`)

- Depart-after chains forwards, unchanged.
- Arrive-by chains backwards. The last segment must arrive by `arriveBy`.
  Each earlier segment must arrive by `chain_deadline_min` of the segment
  after it: that segment's latest departure less the minimum change time at
  the TIPLOC it leaves from (the fallback is 5 minutes, as for
  `chain_ready_min`). This is the mirror of the forward chain.
- When a segment finds nothing, the segments it would chain into are
  validated but not searched, and get `previousSegment` or `nextSegment` as
  their reason.
- Every segment's CRS codes are validated before any search. A bad code is
  therefore a 400 naming the first bad segment, in either direction.
- The avoid lists apply to every segment ("chained in time" as waypoints
  are). Every segment is searched with the same restrictions, at its own
  chained time.

### 3.4 Constraint-specific errors (`explain_empty_segment`)

This is adapted from `train-mcp`'s `attributeFailure`. It runs only for an
empty segment and costs a few extra CSA searches. The checks, in order:

1. `options` and `cappedByMaxChanges`: the reason is `maxChanges`.
2. The avoid lists are active and an unrestricted search for the same time
   finds something. The reason is the first list whose removal alone is
   enough, or `avoidCombined` if no single list is. `train-mcp` always blamed
   `avoid` first; here the list actually responsible is named.
3. Arrive-by: a search from 00:00 finds an arrival. The reason is `arriveBy`,
   and the message gives the earliest arrival.
   Depart-after: the backward scan with an end-of-service deadline finds the
   last departure that reaches the destination. The reason is `departAfter`,
   and the message gives that departure.
4. Otherwise the reason is `noRoute`.

### 3.5 Live overlay

- An itinerary whose live arrival is after its segment's `arriveBy` is marked
  `liveFeasible: false` in `trip_plan_live::annotate`. This makes
  `plan_invalidated` true, and the existing loop re-plans with the live times
  in the overlay, within the existing per-mode budget (3 re-plans for
  `fastest`, 1 for `options`). The backward scan reads the overlay, so the
  re-plan picks an earlier train.
- For an earlier waypoint segment, the deadline is the latest arrival that
  still makes an onward itinerary, so being late for it is a real miss.
- There are no origin look-back seeds in arrive-by mode: a late train only
  arrives later.
- `live=false` is unchanged: no live keys.
- The test `live_a_delay_past_arrive_by_replans_onto_an_earlier_train` covers
  this in both modes.

## 4. Provenance

| Idea / code | Source | How it was adapted |
|---|---|---|
| The three meanings of "avoid" (`avoid` passes through; `avoidStop` calls only) and their names | `train-mcp` `src/timetable/plan/constraints.ts` (top doc, `RouteConstraints`), commit `88043594a221b7b497fe6c52ddc8c3a52cde4e74`; same file in `Distant-Signal-MCP` at `ec1aa720e1e5a67df746ad75b856909e3ba1e06a` | Same semantics and names. DS adds `avoidChange`. |
| Restrictions applied inside the search, not by filtering results; a blocked connection must end the ride | `train-mcp` `constraints.ts` `pruneConnections` | UID continuity is broken inside CSA/RAPTOR and the backward scan instead of relabelling `scheduleId`, so reported UIDs stay real. |
| Pass-through detection from passing points | `train-mcp` `constraints.ts` `connectionTouches` / `buildPassingIndex` | DS had no pass data in `Connection`. A `PassIndex` is built from the untimed rows `build_connections` already skipped, plus per-train `pass_legs` that also cover live-overlay replacements. |
| Constraint-specific failure attribution | `train-mcp` `constraints.ts` `attributeFailure`, `ConstraintFailure` | Returned as a per-segment `noResultReason` in a 200, not an error. Narrowed to the single responsible list. Adds time-bound and chain reasons, `maxChanges`, earliest-arrival and last-departure hints. |
| Arrive-by by bisection over departure time with a forward probe (monotonicity argument) | `train-mcp` `src/tools/plan-journey.ts` `latestDepartureFor` (same in `Distant-Signal-MCP` `src/tools/plan-journey.ts`) | Kept only as the fallback in `scan_connections_arrive_by`. The primary method is a new backward CSA / per-round scan. |
| Rejecting constraint codes that cannot be enforced | `train-mcp` `constraints.ts` FIX 1 (`expandAndValidate`, `isRoutable`) | A code absent from DS's CRS-to-TIPLOC crosswalk is a 400. The `LON` → London Terminals expansion (FIX 2) was not ported. |
| CSA/RAPTOR interchange rules the backward scan mirrors | DS's own `csa.rs`/`raptor.rs` (ported from `Distant-Signal-MCP` `src/timetable/plan/csa.ts`/`raptor.ts`) | Mirrored, not shared, like the existing CSA/RAPTOR duplication. |

No code was copied verbatim: both sources are TypeScript. The
`Distant-Signal-MCP` checkout was read only. `train-mcp` was cloned read-only
into a scratch directory and deleted afterwards.

## 5. Cost

- Bounds: each avoid list is capped at 8 codes. Restrictions are built once
  per request. `noResultReason` costs up to about 5 extra CSA searches, and
  only for an empty segment.
- Arrive-by `options` runs at most `maxChanges + 2` backward rounds, plus one
  capped forward RAPTOR per improving round, all within `[0, deadline]`.
- Benchmark: `crates/trip-planner/tests/bench_arrive_by.rs`, release build,
  synthetic day of 27.9k trains and 401k connections (production is about
  290k), 10 random OD pairs, median of 5 each:

| Search | Median | Max |
|---|---|---|
| CSA depart 08:00 | 11.8 ms | 142 ms (unreachable pair: full scan) |
| CSA arriveBy 12:00 | 18.1 ms | 25 ms |
| CSA avoidChange / avoidStop | 12.4 / 13.8 ms | 222 / 186 ms (the unreachable pair) |
| CSA arriveBy + avoidStop | 21.9 ms | 36 ms |
| RAPTOR depart 08:00 (4 rounds) | 617 ms | 702 ms |
| RAPTOR arriveBy 12:00 | 124 ms | 275 ms |
| RAPTOR avoidStop | 768 ms | 1.1 s |
| RAPTOR arriveBy + avoidStop | 171 ms | 376 ms |

  Arrive-by is cheaper than depart-after in `options` mode. Depart-after
  RAPTOR sweeps from the departure time to the end of the day on every round,
  while the arrive-by rounds are bounded by the deadline and stop at the best
  origin departure.

  The restriction check adds about 20-25% to a sweep (two hash lookups per
  connection). This was measured on a synthetic network because copying a
  production snapshot into a local database was not permitted in this
  session. The route-level overheads (graph cache, leg details, live reads)
  do not change with these features; the live-overlay design doc measures
  them.

## 6. Not done, and decisions for the user

1. **`via` (pass-through waypoint)**. `train-mcp`'s `via` accepts a station
   passed without stopping, and needs its "decoy" retry logic
   (`legsTouchTarget`, `MAX_VIA_RETRIES`). DS `waypoints` already mean
   `viaStop`. `via` was not added. The pass index would make it possible
   later.
2. **Same-train continuation at a waypoint**. `train-mcp`'s `searchLeg`
   waives the change time when the onward leg is the same working, and its
   `mergeAdjacentLegs` joins the two halves. DS waypoints still always charge
   the waypoint's change time, in both directions. Porting this would change
   existing waypoint behaviour, so it is left for a decision.
3. **`LON` and other group codes** are a 400, not expanded.
4. **Defaults**: `avoid` has `train-mcp`'s strict meaning, including pass
   through. The frontend's new `avoidCrs` query field sends `avoid`. No form
   control was added.
