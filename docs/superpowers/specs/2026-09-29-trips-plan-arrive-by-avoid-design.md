# `/Trips/plan`: arrive-by, avoid lists, whole-journey waypoints and constraint-specific errors

Date: 2026-09-29. Status: implemented on branch `trips-plan-arrive-by-avoid`.

## 1. Why

Before this change, `/Trips/plan`:

- could only search forward from `departAfter`;
- could not route around a station;
- gave no reason when a segment came back empty;
- planned waypoints one segment at a time.

The MCP team's `Distant-Signal-MCP` (`src/tools/plan-journey.ts`,
`dsTripPlanEligible`) sends every `arriveBy`, `via` or `avoid` query to its own
local planner for these reasons. Skye's `train-mcp` has these features.

The MCP team reviewed the waypoint chaining of `d5d8acfe` and found three
problems:

- it charged a change at a waypoint even when the traveller stays on a
  through train (a phantom change);
- `maxChanges` applied per segment, not to the whole journey;
- DS allowed 8 waypoints where the MCP allows 20.

The user approved porting from `train-mcp` and `Distant-Signal-MCP` on
2026-09-29.

## 2. API (additive)

```
GET /Trips/plan?origin=&destination=&date=
    [&waypoints=CRS,...]                          (at most 20, configurable)
    [&departAfter=HH:MM | &arriveBy=HH:MM]
    [&avoid=CRS,...][&avoidStop=CRS,...][&avoidChange=CRS,...]
    [&results=fastest|options][&maxChanges=0..4][&live=true|false]
```

### Arrive-by

- `arriveBy` is the latest acceptable arrival at the destination. Sending it
  together with `departAfter` is a 400.
- `fastest` returns the latest-departing itinerary that still arrives in
  time. If several leave at that same minute, it returns the one that
  arrives first.
- `options` returns one itinerary per change count up to `maxChanges`. Each
  is the latest departure that arrives in time with that many changes. They
  are ordered fewest changes first, and each leaves strictly later than the
  one before.
- `cappedByMaxChanges` means a later departure exists that needs more
  changes than `maxChanges`.

### Avoid lists

There are three lists. Each takes at most 8 codes.

| Parameter | The traveller never... | Origin |
|---|---|---|
| `avoid` | rides a train that calls at the station or runs through it without stopping; boards, alights or walks there | `train-mcp`'s `avoid` |
| `avoidStop` | rides a train that calls at the station (running through without stopping is fine) | `train-mcp`'s `avoidStop` |
| `avoidChange` | boards, alights, changes or walks there (staying on a train that calls there is fine) | new; the loosest of the three |

These are 400s:

- an unknown code, with a message naming the list
  (`avoid: 'ZZZ' is not a recognised station CRS code`);
- an avoided station that is also the origin, the destination or a waypoint;
- more than 8 codes in one list.

### Waypoints

Waypoints are now planned as one whole journey.

- **Satisfied by a call.** A waypoint is satisfied when a train calls there
  or the traveller walks in. A train that runs through without stopping does
  not count. This is `train-mcp`'s `viaStop`.
- **No phantom change.** Staying on a train through a waypoint is not a
  change. That segment's itinerary has `continuesPreviousTrain: true`, and
  no change time is charged.
- **Change time.** Changing trains at a waypoint costs that station's
  minimum change time. At a `NoInterchange` sentinel it costs 5 minutes, as
  before.
- **`maxChanges` covers the whole journey.** A change at a waypoint counts
  only when the traveller really changes trains.
  - `options`: a hard limit on the whole journey.
  - `fastest`: every part of the journey gets `exceedsRecommendedChanges`,
    computed from the whole journey's change count.
- **Aligned itineraries.** `segments[s].itineraries[j]` is part `s` of
  journey `j`. The new top-level `journeys[j]` summarises journey `j`. A
  client must pick a journey, not one itinerary per segment. The frontend
  now selects all parts together.
- **Rejected waypoints.** A waypoint equal to the origin, the destination or
  the waypoint before it is a 400. It used to produce an empty segment.
- **Limit.** Up to 20 waypoints, the MCP's limit. `api.tripPlanMaxWaypoints`
  (`TRIP_PLAN_MAX_WAYPOINTS`, clamped to 1-20) can lower it. §5 has the
  cost.

### New response fields

| Where | Field | Meaning |
|---|---|---|
| top level | `arriveBy` | `{time, dayOffset}` or `null` |
| top level | `avoid`, `avoidStop`, `avoidChange` | The lists as applied: uppercased, deduplicated, always present. |
| top level | `journeys` | `[{changeCount, departure, arrival, totalDurationMinutes, exceedsRecommendedChanges?, liveFeasible?}]`, aligned with every segment's `itineraries`. |
| itinerary | `continuesPreviousTrain` | Always present. `true` when the first leg continues the previous segment's train through the waypoint. |
| segment | `arriveBy` | The deadline this segment was effectively searched for (arrive-by only), else `null`. |
| segment | `departAfter` | For segment 0, the request's `departAfter`. For later segments, the earliest time any journey could leave the waypoint. `null` for arrive-by. |
| segment | `noResultReason` | `{constraint, values, message}` when `itineraries` is empty, else `null`. |

### `noResultReason.constraint` values

| Value | Meaning |
|---|---|
| `maxChanges` | Itineraries exist, but all need more changes than the cap (with waypoints, counted over the whole journey). |
| `avoid`, `avoidStop`, `avoidChange` | Removing that list alone gives an itinerary. |
| `avoidCombined` | Removing all the avoid lists gives an itinerary, but removing any single list does not. |
| `arriveBy` | Nothing arrives by the deadline. The message gives the earliest arrival. |
| `departAfter` | Nothing leaves after the start time. The message gives the last departure that still gets there. |
| `noRoute` | Nothing reaches the destination that day. |
| `previousSegment`, `nextSegment` | Another segment, named in `values`, is the one with no itinerary. |

An empty result is still a 200, as before.

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
      "changeCount": 0, "totalDurationMinutes": 50, "continuesPreviousTrain": false,
      "exceedsRecommendedChanges": false, "liveFeasible": true}]
  }],
  "journeys": [{"changeCount": 0, "departure": {"time": "09:40:00", "dayOffset": 0},
    "arrival": {"time": "10:30:00", "dayOffset": 0}, "totalDurationMinutes": 50,
    "exceedsRecommendedChanges": false, "liveFeasible": true}],
  "live": {"applied": true, "reason": null, "replans": 0, ...}
}

GET /Trips/plan?origin=ZJA&waypoints=ZJB&destination=ZJC&date=2026-10-05&departAfter=09:45
  (TWJT1 calls at ZJB 10:30-10:32; a change there needs 5 minutes)
  segments[0].itineraries[0]: TWJT1 10:00 -> 10:30, continuesPreviousTrain: false
  segments[1].itineraries[0]: TWJT1 10:32 -> 11:00, continuesPreviousTrain: true
  journeys[0].changeCount: 0

GET /Trips/plan?origin=ZMD&waypoints=ZME,ZMF&destination=ZMG&...&results=options&maxChanges=1
  (a direct train per segment: two changes in all)
  every segment: itineraries [], cappedByMaxChanges true,
    noResultReason.constraint "maxChanges"; journeys []

GET /Trips/plan?origin=ZVA&destination=ZVC&date=2026-10-05&arriveBy=10:00
  segments[0].noResultReason: {"constraint": "arriveBy", "values": ["10:00"],
    "message": "No itinerary from ZVA arrives at ZVC by 10:00 on 2026-10-05;
                the earliest arrival is 10:30, leaving at 09:40."}

GET /Trips/plan?...&departAfter=09:45&avoid=ZVP&avoidStop=ZVB
  segments[0].noResultReason: {"constraint": "avoid", "values": ["ZVP"],
    "message": "No itinerary from ZVA to ZVC departing after 09:45 on 2026-10-05
                while avoiding ZVP (not even passing through); one exists
                without that restriction."}
```

These shapes are asserted by the DB tests in `crates/api/src/routes/trips.rs`:
`arrive_by_and_avoid_end_to_end_live_on_and_off`,
`live_a_delay_past_arrive_by_replans_onto_an_earlier_train`,
`a_through_train_at_a_waypoint_is_not_a_change_end_to_end`,
`max_changes_caps_the_whole_journey_across_two_waypoints` and
`the_maximum_number_of_waypoints_plans_as_one_journey`.

### Mapping from `train-mcp` / DS-MCP `plan_journey`

| `plan_journey` | `/Trips/plan` |
|---|---|
| `arriveBy: true` with `time` | `arriveBy=time` |
| `avoid` | `avoid` |
| `avoidStop` | `avoidStop` |
| `viaStop` | `waypoints` (same meaning: a call, a train continuing through at no change, and the whole-journey change cap) |
| `via` (pass through) | not supported (see §6) |
| `LON` group code | not expanded: a 400, because `LON` is not a routable CRS |
| up to 20 via points | up to 20 `waypoints` |

## 3. Design

### 3.1 Restrictions inside the searches (`crates/trip-planner/src/restrictions.rs`)

`Restrictions` holds three sets, all keyed by normalized TIPLOC. Each CRS code
is expanded to all of its TIPLOCs.

- **`no_interchange`**: from `avoidChange`, and implied by the other two.
  Checked in:
  - `ready_source_at`: no fresh boarding;
  - `relax`: no alighting, no walking in or out;
  - the backward scan, symmetrically.
- **`no_call`**: from `avoidStop` and `avoid`. Any connection that starts or
  ends at the station is blocked.
- **`pass_legs`**: from `avoid`. For each train that runs through an avoided
  station without calling, its base connections in order, with the spanning
  ones marked blocked. A live-overlay replacement connection is blocked if it
  spans a blocked base leg. It is also blocked, conservatively, if it cannot
  be placed in the base order.

A blocked connection is unusable, and it also ends any ride on its train. Both
forward searches decide "already aboard" by UID alone. Merely skipping the
connection would therefore let someone who boarded before the avoided station
stay aboard past it.

`train-mcp` hit the same problem (see the comment on `pruneConnections`). It
fixes it by relabelling each surviving run of a schedule with a synthetic
`scheduleId`. Here the searches drop the UID from their "aboard" set instead.
Legs therefore keep their real UID, which the live overlay and
`trip_leg_details` rely on.

Boarding the same train again further on then became possible. That exposed
a reconstruction bug: CSA looked the boarding up by UID, so it rebuilt an
earlier ride from the later boarding. Each arrival now records its ride, and
in RAPTOR also the round it was ridden in. The fix is its own commit, with the
regression test `a_train_boarded_again_after_a_block_keeps_its_first_ride`.

Pass-through data: a connection names only its two calls. The untimed pass
rows were already in `schedule_calling_points_full`, but `build_connections`
walked over them. `schedule_query::build_connections_with_passes` now also
returns a `PassIndex`: for each TIPLOC, the indices of the connections whose
span includes it as a pass row.

- On a production weekday (2026-09-29) there are about 194k untimed rows out
  of 488k, so the index is about 0.8 MB.
- It is built once per cached graph.
- `build_restrictions` turns `avoid` into `pass_legs` with one pass over the
  day's connections, once per request.

### 3.2 Arrive-by (`crates/trip-planner/src/reverse.rs`)

The core is a backward Connection Scan. It walks the connections that depart
by the deadline in reverse, with the overlay applied. For each stop and
waypoint stage it keeps the latest time a traveller may have arrived there
and still make the deadline.

Every rule mirrors `csa.rs`:

- A fresh boarding pays the boarding stop's `minimum_change_time`. A
  same-CRS sibling arrival counts. At a waypoint just reached, the waypoint
  fallback applies.
- The origin is free.
- Staying on the same UID is free.
- Fixed links are relaxed backwards. A walk is accepted only if the link
  `fixed_links_from` would choose at the walk's start time arrives in time.
- Restrictions apply as they do going forward.
- The scan stops at the best origin departure found so far.

`latest_departures_by_trips` is the round-based version: round *k* gives the
latest departure using at most *k* trains.

The journey itself comes from the ordinary forward search (CSA/RAPTOR, or the
staged search with waypoints). It is run from the latest departure found,
over the connections departing by the deadline. So an arrive-by itinerary is
built by the same code as a depart-after one.

If the forward search ever arrived late, the code falls back to `train-mcp`'s
`latestDepartureFor` bisection. That fallback is correct because the earliest
arrival never decreases as the departure time increases. Brute-force tests
check the backward scan against the forward searches, with and without
waypoints and restrictions, for CSA and for every RAPTOR round count.

### 3.3 Waypoints as one journey (`crates/trip-planner/src/staged.rs`)

Every label carries a stage: stage *s* means "the first *s* waypoints have
been called at".

- Arriving at waypoint *s* at stage *s*, by train or on foot, is also
  arriving there at stage *s + 1*.
- A traveller on a train that calls at waypoint *s* is also on it at stage
  *s + 1*. The ride continues, and the next part starts with
  `continues_previous_train`.
- The destination is reached at the last stage.

CSA and RAPTOR share one sweep. RAPTOR's rounds count trains over the whole
journey, and a ride through a waypoint counts once. That is what makes
`maxChanges` an end-to-end cap. Changes are counted as trains ridden minus
one.

**Dominance pruning.**

- Forward: a label at (stop, stage *s*, time *t*) is useless if the same
  stop was reached as early at a later stage. Any continuation from stage
  *s* calls at the later waypoints in order, so it also serves the later
  stage.
- Backward, the mirror: a label at stage *s* is useless if a lower stage
  already has a label at least as late at that stop.
- Per connection, only the most-advanced stage aboard (forward) or the
  least-advanced stage that works (backward) is processed.

Pruning cut the 8-waypoint CSA from 539 ms to 170 ms, and the 20-waypoint CSA
from 3.7 s to 0.33 s.

**Correctness checks.**

- With no waypoints, the staged searches match `csa.rs`/`raptor.rs` on random
  networks.
- With waypoints, they match an independent oracle: a fixpoint over "board
  any catchable train, ride it, alight anywhere" with no scan order and no
  pruning. Disabling the through-train continuation makes that test fail.

**How the API uses it.** `plan_trip` (`trip_planning_itinerary.rs`) takes the
staged path whenever there are waypoints. It then:

- splits each journey into per-segment itineraries;
- aligns the segments' itineraries by journey;
- derives each segment's `departAfter`/`arriveBy`: the earliest ready time
  at, or latest deadline for, the waypoint over all journeys;
- builds the `journeys` summaries.

Without waypoints, `plan_trip` behaves exactly as before.

The old per-segment chained planner (`plan_via_waypoints`) is kept for one
job: explaining an infeasible joint plan (§3.4).

### 3.4 Constraint-specific errors (`explain_empty_segment`)

This is adapted from `train-mcp`'s `attributeFailure`. It runs only for an
empty segment and costs a few extra CSA searches. The checks, in order:

1. `options` mode and `cappedByMaxChanges`: the reason is `maxChanges`.
2. The avoid lists are active and an unrestricted search for the same time
   finds something: the reason is the first list whose removal alone is
   enough, or `avoidCombined` if no single list is.
3. Arrive-by: a search from 00:00 finds an arrival, so the reason is
   `arriveBy`, with the earliest arrival in the message.
   Depart-after: the backward scan with an end-of-service deadline finds the
   last departure, so the reason is `departAfter`, with that departure in the
   message.
4. Otherwise, `noRoute`.

When a whole-journey plan with waypoints finds nothing, the chained planner
is re-run to find the segment that fails on its own:

- that segment gets its own reason;
- every other segment names it (`previousSegment` / `nextSegment`);
- if every segment plans on its own, the whole-journey cap was the problem,
  and every segment gets `maxChanges`.

### 3.5 Live overlay

- An itinerary whose live arrival is after its segment's `arriveBy` becomes
  `liveFeasible: false`. That triggers a re-plan within the existing budget
  (3 re-plans for `fastest`, 1 for `options`). The backward scan reads the
  overlay, so the re-plan chooses an earlier train.
- For joint plans, `plan_invalidated` checks each journey's own change at
  each waypoint on live times. Staying aboard through a waypoint always
  passes.
- `journeys[j].liveFeasible` is true only if every part is live-feasible and
  every waypoint change still works.
- No origin look-back seeds are used in arrive-by mode, because a late train
  only arrives later.
- `live=false` is unchanged.

## 4. Provenance

| Idea / code | Source | How it was adapted |
|---|---|---|
| The meanings of `avoid` (passes through) and `avoidStop` (calls only), and their names | `train-mcp` `src/timetable/plan/constraints.ts`, top doc and `RouteConstraints`, at commit `88043594a221b7b497fe6c52ddc8c3a52cde4e74`; the same file in `Distant-Signal-MCP` at `ec1aa720e1e5a67df746ad75b856909e3ba1e06a` | Same semantics. DS adds `avoidChange`. |
| Restrictions inside the search; a blocked connection must end the ride | `constraints.ts` `pruneConnections` | The UID continuity break happens in the searches instead of a `scheduleId` relabel, so real UIDs are kept. |
| Pass-through detection | `constraints.ts` `connectionTouches` / `buildPassingIndex` | A `PassIndex` built from the untimed rows DS already stored, plus `pass_legs` covering live replacements. |
| Constraint-specific failures | `constraints.ts` `attributeFailure`, `ConstraintFailure` | A per-segment `noResultReason` in a 200. Narrowed to the single responsible list. Adds time-bound, chain and `maxChanges` reasons. |
| Arrive-by by bisection over departure time | `train-mcp` `src/tools/plan-journey.ts` `latestDepartureFor` (same in `Distant-Signal-MCP`) | Kept only as a fallback. The primary method is a new backward scan. |
| Refusing unenforceable codes | `constraints.ts` FIX 1 (`expandAndValidate`, `isRoutable`) | A 400 for codes outside DS's CRS-to-TIPLOC crosswalk. The `LON` expansion (FIX 2) was not ported. |
| A via waypoint satisfied by a call; no interchange charged when the onward leg is the same working; one ride, not two legs | `constraints.ts` `searchLeg` (same-`scheduleId` exemption), `mergeAdjacentLegs`, and `viaStop` semantics | Built into the search as stages, so the through train is found even when a fresh change would be too tight, and the change cap covers the whole journey. `train-mcp` fixes this up after the fact. Parts stay split per segment, with `continuesPreviousTrain`; the frontend merges them when tracking. |
| 20 waypoints | `Distant-Signal-MCP` `src/tools/plan-journey.ts` `MAX_WAYPOINTS = 20` | DS's ceiling and default, after measuring (§5). |
| The interchange rules the new searches mirror | DS's own `csa.rs`/`raptor.rs`, ported from `Distant-Signal-MCP` `src/timetable/plan/csa.ts`/`raptor.ts` | Mirrored. |

No code was copied verbatim: both sources are TypeScript. The
`Distant-Signal-MCP` checkout was only read. `train-mcp` was cloned read-only
into a scratch directory and deleted afterwards.

## 5. Cost

All numbers are release builds on a synthetic day in
`crates/trip-planner/tests/bench_arrive_by.rs`: 27.9k trains and 401k
connections, against about 290k connections in production. Production data
could not be copied locally in this session.

**Without waypoints** (`bench_arrive_by_and_restrictions`, 10 OD pairs,
median of 5):

| Search | Median | Max |
|---|---|---|
| CSA depart 08:00 | 11.8 ms | 121 ms (unreachable pair) |
| CSA arriveBy 12:00 | 17.2 ms | 30 ms |
| CSA avoidChange / avoidStop | 10.5 / 13.0 ms | 148 / 165 ms |
| CSA arriveBy + avoidStop | 24.7 ms | 34 ms |
| RAPTOR depart 08:00 (4 rounds) | 534 ms | 577 ms |
| RAPTOR arriveBy 12:00 | 141 ms | 266 ms |
| RAPTOR avoidStop | 678 ms | 875 ms |
| RAPTOR arriveBy + avoidStop | 163 ms | 318 ms |

- Arrive-by is cheaper than depart-after in `options` mode. The backward
  rounds are bounded by the deadline and stop at the best origin departure.
- The restriction check adds about 20-25% to a sweep.

**With waypoints** (`bench_waypoints`, 3 OD pairs through hubs, median):

| Waypoints | CSA | CSA arriveBy | RAPTOR, 6 rounds | arriveBy rounds | old chained CSA |
|---|---|---|---|---|---|
| 0 | 10 ms | 15 ms | 0.85 s | 96 ms | 11 ms |
| 2 | 50 ms | 77 ms | 1.2 s | 0.54 s | 22 ms |
| 4 | 86 ms | 141 ms | 1.7 s | 1.1 s | 30 ms |
| 8 | 170 ms | 250 ms | 1.6 s | 1.2 s | 53 ms |
| 12 | 197 ms | 370 ms | 1.5 s | 1.4 s | 84 ms |
| 20 | 332 ms | 269 ms | 1.9 s | 1.8 s | 133 ms |

`fastest` now costs about 3× the old chained CSA. `options` costs less than
before: the chained planner ran one full RAPTOR per segment, roughly 5-7 s at
8 waypoints, and the joint search takes about 1.6 s. At 20 waypoints the
worst cases are:

- `fastest` with 3 live re-plans: about 1.3 s;
- `options` with 1 re-plan: about 4 s.

Both are within what 8 waypoints already cost under the old planner. The
planning-slot semaphore (4 permits) bounds concurrency as before.

## 6. Not done, and decisions for the user

1. **Pass-through `via`.** A waypoint is satisfied by a call, as in
   `viaStop`. `train-mcp`'s pass-through `via` needs its decoy retries
   (`legsTouchTarget`); it could be built on the pass index later.
2. **Aligned itineraries are a semantic change for multi-segment clients.**
   `segments[s].itineraries[j]` must now be read as part of journey `j`. The
   DS frontend is updated. DS-MCP never sent `waypoints` before.
3. **`LON` and other group codes** are a 400, not expanded.
4. **Waypoint default of 20.** It is configurable down to 1 with
   `api.tripPlanMaxWaypoints`. The measured cost is in §5.
5. **Stricter waypoint validation.** A waypoint equal to the origin, the
   destination or the previous waypoint is now a 400. It used to produce an
   empty segment.
