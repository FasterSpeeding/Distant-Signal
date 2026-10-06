# `/Trips/plan`: pass-through `via`, and `maxChanges` up to 6

Date: 2026-10-06. Status: implemented on branch
`worktree-agent-aa58878773080f9da`. This follows
`2026-09-29-trips-plan-arrive-by-avoid-design.md` and closes its §6.1.

## 1. Why

The Distant-Signal-MCP `plan_journey` tool (`src/tools/plan-journey.ts`,
`dsTripPlanEligible`) still sends two kinds of request to its own local
engine instead of DS:

- any request with `via`, a station the route must pass through, in order,
  "stopping there or not";
- any `results: "options"` request with `maxChanges` above 4, DS's old
  ceiling (`DS_TRIP_PLAN_MAX_CHANGES = 4`).

DS now serves both.

## 2. API (additive)

```
GET /Trips/plan?origin=&destination=&date=
    [&waypoints=CRS,...][&via=CRS,...]
    [&departAfter=HH:MM | &arriveBy=HH:MM]
    [&avoid=CRS,...][&avoidStop=CRS,...][&avoidChange=CRS,...]
    [&results=fastest|options][&maxChanges=0..6][&live=true|false]
```

### `via`

- A comma-separated list of CRS codes, **in order**, at most 3. The name
  was not used before. It is the MCP's `via`, the same as `train-mcp`'s.
- **What satisfies a via.** The journey physically passes through the
  station. Any of these count:
  - staying aboard a train that runs through it without stopping;
  - staying aboard a train that calls there;
  - boarding, alighting or changing there;
  - walking (or taking a fixed link) into it.
- **Order.** The vias are passed in the order given. A station may appear
  twice, but not twice in a row. Within one stretch between two calls, the
  order of the passing points is the order CIF records them in.
- **Vias and waypoints.** These are two independent ordered lists.
  `waypoints` must be called at, in order. `via` must be passed, in order.
  A via may fall before, between or after any waypoint, so it can be
  satisfied in any segment. A via at a waypoint station is allowed; calling
  at the waypoint satisfies it.
- **With the avoid lists.**
  - `avoid` (never even pass through) on a via station is a contradiction:
    a 400.
  - `avoidStop` on a via station means "pass through without calling".
  - `avoidChange` on a via station means "pass through without changing
    there".
  - Both are allowed and enforced inside the search.
- **With `maxChanges`, arrive-by and the live overlay.** These are
  unchanged. The whole-journey change count, the latest-departure
  semantics and the live re-plans all apply as before. A live replacement
  connection over a cancelled call at the via still passes it: the train
  runs through.
- **400s, before any database read:**
  - more than 3 vias;
  - the same via twice in a row;
  - a via equal to the origin or the destination;
  - a via also in `avoid`.
- **Also 400s:**
  - an unknown code: `via: 'ZZZ' is not a recognised station CRS code`;
  - a group code such as `LON`, which has no TIPLOC in DS's crosswalk. It
    is not expanded, consistent with `waypoints` and the avoid lists.

### New response fields

| Where | Field | Meaning |
|---|---|---|
| top level | `via` | The vias as applied: uppercased, in order. Always present (`[]`). |
| `journeys[j]` | `viaSatisfiedBy` | With `via` only: `[{crs, segment, leg, how}]`, one per via in order. `segments[segment].itineraries[j].legs[leg]` is the leg that first passed it. `how` is `call`, `pass` or `walk` (below). |

`how` values:

- `call`: the train called there. The traveller stayed aboard, boarded or
  alighted.
- `pass`: the train ran through without calling, past a CIF passing
  point, or past a call the live overlay cancelled.
- `walk`: a transfer leg into the station.

### New `noResultReason.constraint` value

| Value | Meaning |
|---|---|
| `via` | An itinerary exists without the vias, none with them. `values` is the single via whose removal alone is enough, else every via. Every segment carries it. |

The order of the checks follows §3.4 of the 2026-09-29 spec:

1. `maxChanges`, when `options` and capped.
2. `via`: a fastest (CSA) search with the same waypoints, avoid lists,
   time and overlay, but without the vias, finds a journey. Then each via
   is dropped in turn to name one.
3. Otherwise, the existing explanation of the plan without vias:
   per-segment `avoid*`, `arriveBy`/`departAfter`, `noRoute`,
   `previousSegment`/`nextSegment`, and `maxChanges` for the whole-journey
   cap.

### `maxChanges`

- The ceiling is now 6. It was 4. `max_rounds = maxChanges + 2`, so
  RAPTOR runs up to 8 rounds.
- Out of range is a 400: `maxChanges must be a whole number from 0 to 6
  (default 2), not '7'`.
- **Guard (new).** With `results=options`, a 400 is returned when
  `(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)` is over the bound,
  252 by default (`api.tripPlanMaxOptionsSearchSize`): `results=options
  with 4 waypoints, 3 vias and maxChanges=6 is too large a search (... =
  280, at most 252); use fewer waypoints or vias, a lower maxChanges, or
  results=fastest`. §4.4 explains the limit.

### Examples (for the MCP team)

```
GET /Trips/plan?origin=ZVA&destination=ZVC&date=2026-10-06&departAfter=09:45&via=ZVP
  (TWVF1 runs ZVA 10:00 -> ZVC 11:00 through ZVP without calling;
   TWVN1 is faster and does not pass ZVP)
  "via": ["ZVP"],
  segments[0].itineraries[0].legs[0].trainUid: "TWVF1"
  journeys[0].viaSatisfiedBy: [{"crs": "ZVP", "segment": 0, "leg": 0, "how": "pass"}]

GET /Trips/plan?...&via=ZVX           (no train passes ZVX)
  segments[0].itineraries: []
  segments[0].noResultReason: {"constraint": "via", "values": ["ZVX"],
    "message": "No itinerary from ZVA to ZVC departing after 09:45 on
                2026-10-06 passes through ZVX (calling there or not); one
                exists without that via."}

GET /Trips/plan?origin=ZKA&destination=ZKZ&...&results=options&maxChanges=6
  (the only route has six changes)
  segments[0].itineraries[0].changeCount: 6; with maxChanges=5:
  itineraries [], cappedByMaxChanges true, noResultReason.constraint "maxChanges"
```

These shapes are asserted by the DB tests
`via_passes_through_without_calling_end_to_end` and
`max_changes_five_and_six_end_to_end` in `crates/api/src/routes/trips.rs`.

## 3. Design

### 3.1 Why search state, not decoy retries

`train-mcp` and the DS-MCP build `via` as a chain of sub-searches. Each one
runs "to wherever a train that touched the via next stops"
(`passThroughDestinations`). That needs `legsTouchTarget` and up to 20
retries (`MAX_VIA_RETRIES`). A faster train reaching the same next stop by
another branch is a decoy: it satisfies the sub-search without passing the
via. The DS-MCP also documents a gap: two vias between the same pair of
calls cannot be split.

DS instead adds the vias to the search state, as the waypoints already
are (`trip_planner::staged`, 2026-09-29). This is the label-setting
approach the brief suggested. It is ordered, so it carries a counter, not
the bitmask the brief mentioned:

- A state is the pair (waypoint stage `s`, via progress `v`). `v` means
  "the first `v` vias have been passed".
- Every label (earliest arrival) and every ride ("aboard this train") is
  kept per state.
- Riding a connection advances `v` over everything the connection passes:
  its departure call, then the passing points between its two calls in
  order, then its arrival call (`Vias::advance`). Arriving at a station on
  foot advances `v` over the station.
- A ride whose progress advances mid-train stays one leg
  (`Source::Carried`). Only a waypoint starts a new part.
- The destination is reached at the last stage with every via passed.

Progress is tracked per ride, so only the train actually ridden can
satisfy a via. A decoy cannot, and there is nothing to retry. Two vias
between the same pair of calls advance together, in their recorded order.

**Ordered, not a bitmask.** The MCP's `via` is ordered, so a counter
(`v + 1` values) is enough. A bitmask would be `2^v`, and it would lose
the order.

**Dominance** generalises from stages to the grid. (s, v) dominates
(s', v') when `s >= s'` and `v >= v'`. Anything a journey can still do from
the lesser state, it can do from the greater one.

- Forward: a label is pruned when a dominating state already reached the
  stop as early. A ride is pruned when a dominating ride is aboard.
- Backward (arrive-by): labels and "aboard" sets are up-closed, so a
  lesser state dominates. The states before a connection are found as the
  least `v` whose advance over it reaches the state after. That is not
  "equal to" it: a connection can skip progress values.

### 3.2 Where the passing points come from

`schedule_query::PassIndex` already lists, per TIPLOC, the connections
whose span includes an untimed row there. It now also records each row's
position in its span (`PassRow`), so the order of two passing points
between the same calls is known.

`build_vias` (in `data::trip_planning_itinerary`) runs once per request,
on the blocking pool, next to `build_restrictions`:

- It maps each CRS to every TIPLOC it covers.
- It gives each train that calls at or passes a via its base connections
  in order, as `PassSpan`s. Each span has its departure and the via
  TIPLOCs it passes, in order.
- Trains that only call there are included too. A live replacement
  connection over a cancelled call at the via must still count as passing
  it.

`Vias::advance` matches a base connection exactly (from, to, departure).
A train can make the same pair of calls twice, so the departure is part
of the match. A replacement connection's times have moved, so it is
matched as the shortest run of spans between its two calls. One that
cannot be placed passes only its own two calls (conservative).

### 3.3 The timing-point limitation

CIF records a passing row only at a timing point: a junction, or a
station the timetable times trains through. A train running through a
station that is not a timing point for it leaves no row there, so DS
cannot see it pass. Such a train satisfies a via at that station only by
calling there, or by the traveller changing or walking there.

- **Effect.** A via at a small intermediate station will usually be
  satisfied only by stopping trains. A via at a major station or junction
  (Crewe, Rugby, Clapham Junction) is a timing point for almost every
  train and works fully.
- **No silent wrong answers.** DS never claims a train passed a station
  without a row. The failure mode is "no itinerary" (`noResultReason.via`)
  or a stopping train where a non-stopping one exists.
- **Same as the MCP.** The MCP's local engine reads the same CIF passing
  points (`resolvedPassingPointsForDate`), so it has the same limit.
- **Tested.** `a_via_at_a_non_timing_point_is_seen_only_when_a_train_calls_there`
  (`data::trip_planning_itinerary`): a train with no row at Stafford does
  not satisfy `via=STA`, and the reason is `via`. The same train with an
  untimed row there satisfies it with `pass`; with a call there, `call`.
- **Production data.** 2026-09-29 production weekday: about 194k untimed
  rows out of 488k (§3.1 of the 2026-09-29 spec). These are the rows the
  index holds.

### 3.4 Integration

- `plan_trip` takes the joint staged path whenever there are waypoints or
  vias. That covers fastest/CSA (`scan_staged`), options/RAPTOR
  (`raptor_staged`), arrive-by (`staged_arrive_by`,
  `latest_departures_by_trips` + `staged_raptor_arrive_by_from_latest`),
  the restrictions and the overlay.
- Without either, `plan_trip` is the old single-segment planner,
  unchanged.
- Each `StagedJourney` reports `via_legs`: for each via, the part and leg
  that first advanced past it, and how. The API serves these as
  `viaSatisfiedBy`.

### 3.5 Correctness checks

- **Unit scenarios** (`trip_planner::staged`):
  - staying aboard through a passed via is one leg;
  - a decoy branch to the same next stop does not count;
  - via with `avoidStop` (pass without calling), with `avoidChange` (stay
    aboard a calling train) and with `avoid` (nothing);
  - vias keep their order and interleave with waypoints;
  - an unsatisfiable via finds nothing either way;
  - arrive-by takes the latest train that passes the via.
- **Differential test** (`with_vias_csa_raptor_the_oracle_and_the_backward_scan_agree`),
  over 120 random networks with passing points, with 1 to 3 vias and 0 to
  1 waypoints:
  - staged CSA and RAPTOR both equal an independent fixpoint oracle;
  - every reported via leg really calls at or passes its via, in order;
  - the backward scan equals brute force over the forward searches (CSA,
    and RAPTOR per train count).
- **Existing tests.** All the waypoint, restriction and arrive-by tests
  pass unchanged on the restructured search. That includes the oracle
  test and the "no waypoints equals `csa.rs`/`raptor.rs`" test.

## 4. Cost

### 4.1 Method

The bench is `bench_max_changes_and_vias` in
`crates/trip-planner/tests/bench_arrive_by.rs`. It is a release build on
the same synthetic day as the 2026-09-29 numbers: 27.9k trains and 401k
connections. Each row has 3 OD pairs, and the median and maximum are
given. Waypoints are hubs. The vias are three hubs, each also passed
(without calling) by every 7th line.

The host was heavily loaded: load average 45-70 on 6 cores, from other
agents' builds. The 2026-09-29 spec had a quiet machine. Absolute times
are therefore 2-5x those figures and vary run to run. Compare ratios
within a run, not times across runs.

- The same session's `bench_waypoints` ran 1.9-2.8 s for RAPTOR (6
  rounds), against 0.85-1.9 s in the 2026-09-29 spec.
- CSA with 20 waypoints ran in 615 ms, against 332 ms.

### 4.2 Raising `maxChanges` from 4 to 6 (RAPTOR 6 to 8 rounds)

Run A (whole seconds) and run B (one decimal), both after the
optimisation in §4.4:

| waypoints | vias | RAPTOR mc4 median / max | RAPTOR mc6 median / max | arrive-by rounds mc4 | arrive-by rounds mc6 |
|---|---|---|---|---|---|
| 0 | 0 | A 2 / 4 s; B 6.9 / 8.3 s | A 3 / 4 s; B 9.1 / 9.4 s | B 0.52 / 0.68 s | B 0.48 / 0.52 s |
| 0 | 3 | A 5 / 10 s; B 10.2 / 11.8 s | A 8 / 11 s; B 9.3 / 14.0 s | B 3.1 / 3.9 s | B 3.7 / 3.9 s |
| 4 | 0 | A 4 / 4 s; B 5.0 / 7.8 s | A 8 / 9 s; B 6.0 / 9.0 s | B 1.9 / 2.0 s | B 2.4 / 2.5 s |
| 4 | 3 | A 7 / 7 s; B 17.9 / 23.5 s | A 12 / 14 s; B 19.4 / 22.4 s | B 7.2 / 13.7 s | B 11.8 / 19.1 s |
| 20 | 0 | A 3 / 7 s; B 5.1 / 5.3 s | A 7 / 10 s; B 5.4 / 7.7 s | B 2.1 / 3.0 s | B 3.2 / 4.8 s |
| 20 | 3 | A 5 / 11 s; B 5.1 / 6.1 s | A 10 / 11 s; B 7.7 / 8.5 s | B 2.9 / 4.4 s | B 4.7 / 5.3 s |

For comparison, CSA (`fastest`, independent of `maxChanges`) in run B:

| waypoints | vias | CSA median / max | CSA arrive-by median / max |
|---|---|---|---|
| 0 | 0 | 25 / 30 ms | 59 / 74 ms |
| 0 | 3 | 217 / 476 ms | 399 / 907 ms |
| 4 | 0 | 177 / 283 ms | 287 / 496 ms |
| 4 | 3 | 0.80 / 0.90 s | 1.3 / 1.8 s |
| 20 | 0 | 0.99 / 1.5 s | 0.50 / 0.88 s |
| 20 | 3 | 1.1 / 1.2 s | 0.67 / 0.73 s |

### 4.3 Reading the tables

- **Rounds.** Two more rounds cost 1.0-2x, typically about the 8/6 = 1.33
  the extra sweeps imply. RAPTOR stops early when a round improves
  nothing, so the extra rounds often cost less than a full sweep.
- **Vias.** A via state costs about 2-3x a waypoint state:
  - 4 waypoints and 3 vias at mc4 (20 states) cost 2-3.5x what 20
    waypoints alone (21 states) do;
  - 0 waypoints and 3 vias cost about 1.5-2.5x no vias.
- **Why vias cost more.** Via progress fills in across the whole network,
  because any train passing a via advances it. A waypoint stage fills in
  only once the waypoint is reached.
- **Worst case without a guard.** 20 waypoints and 3 vias at mc6 in
  options with one live re-plan is about 2 x 8-10 s under this load, about
  4-5 s on a quiet host. That is too much for one unauthenticated request.

### 4.4 The guard (`OPTIONS_SEARCH_SIZE_LIMIT`, `routes::trips`)

`results=options` requires
`(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2) <= bound`. The via
weight of 2 is the measured via-to-waypoint state cost.

**Decision, 2026-10-06 (user).** The bound is 252: twice what 20
waypoints at `maxChanges=4` (the most allowed before the raise) already
cost. It was first set at 126, exactly that cost. It is configurable:

- chart `api.tripPlanMaxOptionsSearchSize`;
- env `TRIP_PLAN_MAX_OPTIONS_SEARCH_SIZE`;
- default 252, clamped to 8-504. 8 still admits a direct plan (no
  waypoints, no vias) at `maxChanges=6`. 504 is four times the old worst
  case.

The resulting limits at the default bound (also capped at 20 waypoints):

| max waypoints | maxChanges 2 | 4 | 5 | 6 |
|---|---|---|---|---|
| 0 vias | 20 | 20 | 20 | 20 (30 by the formula) |
| 1 via | 20 | 13 | 11 | 9 |
| 2 vias | 11 | 7 | 6 | 5 |
| 3 vias | 8 | 5 | 4 | 3 |

**Expected worst case.** The largest admitted options searches, such as
5 waypoints and 3 vias at `maxChanges=4`, or 20 waypoints at
`maxChanges=6`, should cost about twice the old worst case. That is
roughly 4 s on a quiet host (20 waypoints at `maxChanges=4` was 1.9 s in
the 2026-09-29 spec), and about 8 s with the one live re-plan `options`
allows. The planning-slot semaphore (4 permits) still bounds concurrency.

- The guard is a 400 before any database read. The message names the
  numbers and the ways out: fewer waypoints or vias, a lower
  `maxChanges`, or `results=fastest`.
- `fastest` is not limited. It runs one CSA sweep, at most about 1 s at
  20 waypoints and 3 vias under this load, and up to 3 live re-plans.
- **Rejected alternative.** A flat cut of the waypoint limit when
  `maxChanges > 4` was not used. It would ignore the vias, which are the
  larger cost.

### 4.5 The restructure's own cost

Putting the vias in the state changed the staged sweep, as reported:

- labels are now kept per stop with their states;
- fresh boardings come from the labels at the stop and its siblings.

An interleaved A/B of `bench_waypoints` (no vias) against `c32285b3`, run
concurrently on the same loaded host:

- **First version:** about 1.5x slower forward. It computed every fresh
  boarding, even for a train already ridden at a dominating state, which
  the old sweep never did.
- **After the fix** (commit "perf(trip-planner): skip fresh boardings a
  ride already aboard dominates"): at parity within noise. RAPTOR was
  0.9-1.4x baseline (about 1.1x), CSA 0.6-1.7x, and the backward rounds
  0.5-1.3x.

`perf` is not permitted in this sandbox (`perf_event_paranoid=3`), so the
fix came from reading the code, not from a profile.

## 5. Provenance

| Idea | Source | How it was adapted |
|---|---|---|
| `via` semantics: pass through in order, stopping or not | `Distant-Signal-MCP` `src/tools/plan-journey.ts` `routeConstraintInputShape.via`; `src/timetable/plan/constraints.ts` (`via` doc, `connectionTouches`) | Same semantics. Implemented as search state, not a chain with decoy retries (`passThroughDestinations`, `legsTouchTarget`, `MAX_VIA_RETRIES`). |
| Passing points from CIF | `constraints.ts` `buildPassingIndex` over `resolvedPassingPointsForDate` | DS's existing `PassIndex`, now with in-span positions. |
| The `maxChanges` gap | `plan-journey.ts` `DS_TRIP_PLAN_MAX_CHANGES = 4`, `dsTripPlanEligible` | DS's ceiling is raised to 6. |

No code was copied. The `Distant-Signal-MCP` checkout was only read.

## 6. For the MCP team

- `dsTripPlanEligible` can stop excluding `hasVia`. Send `via` as `?via=`,
  comma-separated and in order. DS takes at most 3; keep the local engine
  for more.
- `DS_TRIP_PLAN_MAX_CHANGES` can become 6.
- `results=options` requests over the size guard (§4.4; default bound
  252, so e.g. up to 20 waypoints without vias at any `maxChanges`, and
  8/5/3 waypoints with 3 vias at `maxChanges` 2/4/6) get a 400
  mentioning `too large a search`. Fall back to the local engine for
  those, as for any other DS failure.
- A via at a station that is not a CIF timing point for the trains
  concerned is satisfied only by calling there (§3.3). The local engine
  has the same limit.
- `journeys[j].viaSatisfiedBy[k].how` tells `pass` from `call`, if the
  tool wants to say "passes through X without stopping".
- `noResultReason.constraint: "via"` names the via to blame.
