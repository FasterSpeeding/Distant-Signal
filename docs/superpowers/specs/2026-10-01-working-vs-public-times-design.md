# Working vs public timetable times: evaluation and recommendation

Date: 2026-10-01. Status: phases 1 and 2 implemented (§10, §11). A
characterisation test was committed with it
(`set_down_and_pick_up_only_characterisation` in
`crates/schedule-reference/src/main.rs`).

Code references are to main at `b84e97de`. Measurements come from the
production RJTTF975 full extract (delivered 2026-09-30) and read-only
production queries made on 2026-10-01.

## 0. Summary

Distant Signal (DS) is a public-facing tool, but almost everything it shows
or computes from the timetable uses **working timetable (WTT)** times:

- The CIF parser decodes the public times (`public_arrival`,
  `public_departure`) and the Activity codes. Nothing outside the parser
  reads the public times, and every product except one JSONB blob drops
  them.
- The half-minute is truncated. DS shows a WTT time of `20:50H` as 20:50,
  while the public timetable says 20:51.
- TRUST's `gbtt_timestamp` (the public time of each movement) is
  deserialised and thrown away. Every TRUST delay is measured against the
  WTT time.
- The one public-time source DS uses is LDBWS (`std`/`etd`). It feeds the
  live board and the line-severity statistics.

How often the two kinds of time differ, for one weekday (Thu 2026-10-01;
22,880 resolved passenger schedules):

| Where | Public ≠ WTT | DS's displayed minute ≠ public | Typical difference |
|---|---|---|---|
| Origin departure | 2.6% | **0.9%** (198 services) | public 0.5–3 min *earlier* |
| Intermediate departure | 48% | **1.5%** | public = WTT rounded down, so truncation mostly matches |
| Intermediate arrival | 51% | **50%** (120,673 calls; 21,205 services) | public = WTT rounded *up*: DS shows the arrival 1 min early |
| Terminating arrival | 22% | **22%** (5,064 services) | public later by 0.5–4 min (recovery margin); **10.9% of services by ≥ 2 min** |

The Saturday figures (2026-10-03) are within one point of these.

Two problems matter most:

1. **Delay and Delay Repay are measured against the wrong time.** The
   destination padding means a train arriving "2 min late" against the WTT
   is on time against the public timetable for about 1 in 9 services.
   - The Delay Repay estimate is worse than that. It uses the train's
     *latest* TRUST delay at *any* location, including passes, and not the
     arrival at the ticket's destination against the public timetable,
     which is what Delay Repay pays on.
   - 27% of trains currently showing ≥ 15 min late sit in the 15–17 min
     band, where a 1–3 min baseline error flips eligibility.
   - Production has 0 tickets today, so no user has seen a wrong estimate
     yet.
2. **Set-down-only and pick-up-only stops are handled wrongly. This is
   confirmed by test, not inferred.**
   - **D (set down only):** 481 calls on 316 services per weekday are
     dropped from the calling-points feed. The trip planner cannot route a
     passenger *to* those stops, and the train page omits them.
   - **U (pick up only):** 323 calls on 287 services per weekday stay in
     the feed with no direction flag. The planner will route a passenger
     *off* a train where alighting is not allowed. The commonest case is
     South Western Railway (SWR) at Clapham Junction (113 calls a day), so
     Waterloo → Clapham Junction gets a train you cannot get off at.

**Recommendation:** use public times for everything shown to users and
everything computed for them: display, delay, Delay Repay, planning,
connections and notifications. Keep WTT internally for matching, identity
and ordering. Store both, and expose both on the API (`public*` plus
`working*`), so the MCP and the frontend can tell them apart.

## 1. Background: the two kinds of time

| Source | Working (WTT) | Public (GBTT) |
|---|---|---|
| CIF `LO` | scheduled departure `10..15` (`HHMM` + `H`) | public departure `15..19` |
| CIF `LI` | scheduled arrival `10..15`, departure `15..20`, **pass** `20..25` | public arrival `25..29`, departure `29..33` (`0000` means none) |
| CIF `LT` | scheduled arrival `10..15` | public arrival `15..19` |
| TRUST 0003 | `planned_timestamp`, `timetable_variation`, `variation_status` | `gbtt_timestamp` (empty for passes and non-public events) |
| LDBWS / Darwin | — (the staff-only LDBSVWS has working times) | `std`/`sta`/`etd`/`eta`, calling points `st`/`et`/`at` |

WTT is what signallers and TRUST work to. It has half-minutes, passing
times and pathing and performance allowances. The public timetable is what
is sold and advertised, and Delay Repay and the National Rail performance
measures are judged against it:

- **On Time:** within 59 s of the public time at each recorded station.
- **PPM:** within 5 or 10 min of the public arrival at the destination.

The data shows the public time is derived from the WTT time in a consistent
way:

- **Departures are rounded down.** `int_dep` public − WTT is `0` in 52% of
  calls and `−0.5` in 46%.
- **Intermediate arrivals are rounded up.** `+0.5` in 46% of calls.
- **Terminating arrivals carry extra minutes of margin** (§3).

The Activity field encodes the direction of passenger use:

- `T` means set down and pick up;
- `D` means set down only;
- `U` means pick up only;
- `R` means request stop;
- `N` means not advertised;
- `TB`/`TF` mean train begins and train finishes.

The public times encode the same thing: a `D` stop has a public arrival and
a `0000` public departure, and a `U` stop has the reverse. See the real
lines in §4.

## 2. What DS ingests and stores

### 2.1 CIF (schedule-query parser, schedule-reference products)

- **The parser decodes everything except the pass time.** It is
  `parse_calling_point` at `crates/schedule-query/src/parse.rs:530-610`:
  - WTT `booked_arrival`/`booked_departure` (`:547-564`); `parse_time_field`
    builds `HH:MM:00`, so the half-minute is dropped at `:467-474`;
  - the `H` flags as booleans (`:548`, `:556`);
  - public times (`:570-577`; `0000` maps to `None` at `:481-486`);
  - platform and Activity.
- **The scheduled pass time is never decoded** (layout diagram at `:520`;
  `connections.rs:89-93`). A passing point arrives as an `LI` with both
  times `None`.
- **The struct.** `CallingPoint` is at
  `crates/schedule-query/src/records.rs:243-327`. Its public-time doc
  comment, at `:304-314`, reads "what a passenger timetable shows, as
  opposed to the working time".
- **The direction rule covers boarding only.** `is_public_pickup()`
  (`records.rs:342-403`) answers "can a passenger board here?" using
  T/TB/U/R, with N always meaning no. There is **no matching "can alight"
  predicate**.

What each product keeps (`crates/schedule-reference/src/main.rs`):

| Product (table) | Times kept | Public times | Half-minute | Activity / direction |
|---|---|---|---|---|
| `schedule_calling_points_full` (`:1894-1931`; migration `20260923100000`) | WTT arr/dep | **dropped** | dropped | Used to *filter*: `is_public_pickup() \|\| Terminate` (`:1903-1905`), so `D` rows are deleted |
| `schedule_network_departures` (`:1644-1655`) | WTT dep (`resolve.rs:398,427`) | dropped | dropped | Filtered on pick-up (correct for a departure list) |
| `schedule_destination_departures` (`:1716-1738`) | WTT dep, `calling_point_arrival`, `destination_arrival` | dropped | dropped | Filtered on pick-up |
| `schedule_line_population` JSONB (`:1421-1446`) | full `CallingPoint` | **kept** | kept | kept |
| `trains.calling_points` JSONB (`ScheduleCallingPointDto`, `crates/api/src/data/schedule_matching.rs:66-104`) | WTT | dropped | kept (as flags) | dropped |

In production `trains.calling_points` is set on only **238 of 428,722**
`trains` rows. The train page therefore almost always rebuilds its stops
from `schedule_calling_points_full` (`crates/api/src/data/journey.rs:1-8`,
`:677-700`), which has neither public times nor `D` stops.

### 2.2 TRUST

- **Movement fields.** `Movement` is at
  `crates/trust-schema/src/schema.rs:99-123`. `gbtt_timestamp` is
  `#[allow(dead_code)]` with the note "no consumer yet".
  `planned_event_type` is not deserialised at all.
- **Live delay is computed against WTT.** It is `actual − planned`, taken
  from the *latest* movement of any type when `LATE`. See
  `refine_late_delay_minutes` at
  `crates/trust-consumer/src/process.rs:1775-1786` and
  `common::trust_timestamp::plausible_delay_minutes`.
- **Full-coverage uses TRUST's own figure.** It reads `timetable_variation`,
  which TRUST also measures against the WTT (`schema.rs:116-117`).
- **The raw body is not kept.** `train_movement_events.raw_body` is written
  as `{}` (`process.rs:1489`). Production has 5.4 M rows and all of them
  are empty, so **past public times cannot be recovered (no backfill)**.
- **The backlog keeps no public times either.** `trust_event_backlog` has
  only planned, actual, variation and delay, and keeps no PASS events.

### 2.3 LDBWS

- **What is parsed.** `std`/`etd` and calling points `st`/`et`/`at` are
  parsed; `sta`, `eta` and `isPassing` are not
  (`crates/poller-ldbws/src/schema.rs:22-88`).
- **Delay.** `delay_minutes = etd − std`, clamped at 0 (`:124-137`).
- **Storage.** Stored in `station_samples.departures` JSONB.

**This is the only public-time data DS uses.**

## 3. Usage by feature

"Right?" means "right for a member of the public".

| Feature | Time used today | Right? | Evidence |
|---|---|---|---|
| Live departure board `GET /public/stations/{crs}/departures` | LDBWS `std`/`etd` (public) | **Yes** | `crates/api/src/routes/departures.rs:90-162`, `poller-ldbws/src/schema.rs:214-217` |
| Timetable board `…/schedule-departures` and `/public/trains/search` | CIF WTT departure, truncated | **No.** Differs from public for 0.9% of origins and 1.5% of intermediates. `destinationArrival` is WTT and differs for 22% | `resolve.rs:398,427,548,569,599`; `render.rs:279-285,387-397` |
| Train page `/Train/by-uid/{uid}/{date}` → `journeyStops[].scheduled*` | CIF WTT (`journey.rs:370-375`), falling back to TRUST planned (WTT) | **No.** Intermediate arrival 1 min early in half of calls; terminating arrival early in 22% | `frontend/components/JourneyTimeline.tsx:328` shows departure ?? arrival |
| Train page `scheduledDeparture` (header) | WTT origin | Mostly fine (0.9% differ) | `schedule_matching.rs:705-711` |
| Per-stop `delayMinutes` | TRUST actual − planned (WTT), truncated | **No** at padded destinations | `journey.rs:958-961` |
| `estimated*` / `status` / `lateMinutes` | WTT + train delay; late if ≥ 1 | **No** | `journey.rs:1472-1485`, `stop_live_status.rs:75-94` |
| Train-level delay (badges, journeys list, notifier, Delay Repay) | `train_current_state.delay_minutes`: latest TRUST event at **any** location (including PASS), against WTT | **No** | `trust-consumer/src/process.rs:1427-1432,1775-1786` |
| Frontend "On time" | `delayMinutes <= 0` | Threshold fine; baseline wrong | `TrainJourney.tsx:339-340`, `lib/journeyStatus.ts:60`, `reliabilityDigest.ts:44-45,81` |
| **Delay Repay estimate** `GET /Train/{id}/tickets/{id}/delay-repay` | `state.delay_minutes` as above; the ticket's `destination_crs` is ignored | **No.** Wrong station *and* wrong baseline (§5.1) | `crates/api/src/routes/train.rs:427-480`; bands at `data/delay_repay_rules.rs:158-174` |
| Trip planner `/Trips/plan` (CSA/RAPTOR) | WTT `booked_*` from `schedule_calling_points_full`, truncated | **No.** Displayed and connected on WTT; `D`/`U` handled wrongly (§4) | `crates/api/src/data/trip_planning.rs:47-48`; `schedule-query/src/connections.rs:206-207` |
| Interchange / MCT | MSN change time compared on truncated WTT minutes | **Should be public.** Advertised connections are made on public times | `trip-planner/src/csa.rs:140-200` |
| Arrive-by, waypoints, avoid | the same WTT graph | As above | `trip-planner/src/reverse.rs`, `staged.rs` |
| Live overlay delays | TRUST actual − **CIF WTT** (truncated), clamped ≥ 0 | **No.** It also mixes timestamp bases, which `journey.rs:923-957` deliberately avoids | `crates/api/src/data/trip_plan_live.rs:160-162,268-286` |
| Journeys / legs candidate search | `schedule_destination_departures` WTT windows; only boardable rows | Windows are fine in WTT; a leg ending at a `D` stop **cannot be found** | `crates/api/src/data/queries.rs:4012-4098` |
| Notifications | train delay ≥ 15 (`notifier/src/config.rs:24-27`), latest event against WTT | **No.** Should be the forecast or actual at the user's alighting stop, against public | `notifier/src/decision.rs:66-78`, `queries.rs:520-530` |
| TRUST ↔ schedule matching | WTT `planned_timestamp`, 5-min "GBTT vs WTT" slack | **Keep WTT** (it is the identity key) | `trust-consumer/src/matching.rs:38-56` |
| Pin → CIF match, `GET /public/trains/resolve`, ETA blend, stop board | LDBWS public `std` against CIF WTT, ±2/±5/20 min | Keep WTT for identity, but compare like with like once public is stored (removes the half-minute fudge) | `train_resolve.rs:25-36`, `eta_blend.rs:15-30`, `stop_board.rs:17-35` |
| Line severity, operator trends, station stats | LDBWS `etd − std` ≥ 5 (public) | **Yes** (departures only) | `aggregator/src/aggregation.rs:1000-1010`, `common/src/lib.rs:2055-2084` |
| Full-coverage windows / station correlation | TRUST `timetable_variation` (WTT) ≥ 3/5 | Acceptable as an operational measure; label it as such | `full-coverage-consumer/src/windows.rs:90-118` |
| Incidents | no time arithmetic | n/a | — |
| Frontend formatting | `HH:MM`; no `½`; `isHalfMinute*` typed but unused | Fine for public times; WTT needs `½` if ever shown | `frontend/lib/dateFormat.ts:57,131`, `lib/types.ts:516-517` |
| Passing points | not decoded, so untimed; filtered from the train page (`JourneyTimeline.tsx:202-204`) | **Correct** not to show them as stops | `journey.rs:134-143` |
| MCP (DS-MCP) | local engine uses **public** times (`src/timetable/cif/assemble.ts:80-101`; `plan/connections.ts:63-69`); the DS path relays DS WTT in the same `departure`/`arrival` fields | **Inconsistent.** The same tool returns different time bases depending on which engine answered. Notes to the LLM never say "working time" | `plan-journey.ts:960-976,1949-1950`; `find-services.ts:760-761` says "Public departure" |

## 4. Set-down-only and pick-up-only stops (tested)

The journey-planning comparison artifact (DS vs train-mcp) says: "Using
working times, and dropping set-down-only rows, can make DS times differ
slightly from the public timetable and can leave out stops passengers can
use (inferred, not tested)." **Both halves are confirmed by test.**

The fixtures are real lines from RJTTF975:

```
UID C01372  Avanti 9S65 Euston → Glasgow Central
LIMOTHRWL 1700H1702      170100002        D      public arr 17:01, public dep 0000
UID C01355  Avanti 9G44 Euston → Wolverhampton
LIWATFDJ  2029H2031      000020316  FL FL U      public arr 0000, public dep 20:31
LIMKNSCEN 2050H2052H     205120526  FL FL T      WTT arr 20:50H, public arr 20:51
```

`set_down_and_pick_up_only_characterisation` runs these lines through
`schedule_calling_points_full_rows` (the planner's and train page's feed),
then `schedule_query::build_connections`, then the real
`trip_planner::scan_connections`. All six tests pass. The two marked
KNOWN WRONG are written to be flipped when the bug is fixed:

| Test | Today | Correct |
|---|---|---|
| `known_wrong_a_set_down_only_stop_is_dropped_from_the_calling_points_feed` | Motherwell is absent from 9S65's rows | present, flagged "set down only" |
| `known_wrong_the_planner_cannot_alight_at_a_set_down_only_stop` | Carlisle → Motherwell finds **no** journey | 16:02 → 17:01 on 9S65 |
| `known_wrong_the_planner_alights_at_a_pick_up_only_stop` | Euston → Watford Jn is planned on 9G44, arriving at **20:29 (WTT)** | not allowed: you cannot alight at Watford Jn from 9G44 |
| `correct_the_planner_boards_at_a_pick_up_only_stop` | Watford Jn → Milton Keynes OK, arriving **20:50** | allowed, but should show **20:51** (public) |
| `correct_departure_boards_respect_both_directions` | no departure at Motherwell; departure at Watford Jn | as today |
| `the_parser_keeps_both_stops_and_their_public_times` | the parser holds both stops with their public times | as today |

Scale, per day, for passenger trains (status P or 1), excluding passes:

| | Thu 2026-10-01 | Sat 2026-10-03 |
|---|---|---|
| `D`-only calls (dropped; cannot alight; missing from train page) | 481 calls, 316 services, 182 TIPLOCs | 562 calls, 384 services |
| `U`-only calls (kept; planner wrongly allows alighting) | 323 calls, 287 services, 60 TIPLOCs | 379 calls, 351 services |
| Request stops `R` (treated as ordinary; no "request" label) | 1,391 calls, 424 services | 445 services |
| `N` (not advertised; excluded, which is correct) | 162 calls | — |

The busiest affected stations are:

- `D`: Stratford, Haymarket, Stevenage, Aberdare, Watford Jn, Allerton,
  Bolton, Motherwell;
- `U`: **Clapham Junction (SWR, 113 a day)**, Stevenage, Watford Jn,
  Stratford.

Wrong answers that are easy to hit include Waterloo → Clapham Junction and
Euston → Watford Junction. The existing test
`a_non_public_intermediate_stop_is_excluded_while_a_public_one_is_retained`
(`schedule-reference/src/main.rs`) asserts the `D` drop as correct. It
should be rewritten when the fix lands.

Other places this matters:

- **Departure boards.** The CIF board and `/public/trains/search` are
  correct here, because they list departures only and filter on pick-up.
- **Journeys.** The leg search cannot end a leg at a `D` stop (§3).
- **LDBWS.** The live board comes from Darwin, which applies its own
  rules.

## 5. Mismatch symptoms

### 5.1 Delay Repay (highest priority)

- **The rule.** Operators pay Delay Repay against the **public timetable
  arrival at the passenger's destination station** (the National Rail
  Conditions of Travel and operators' schemes).
- **What DS does instead.** It uses `train_current_state.delay_minutes`,
  which has three faults:
  1. **Wrong location.** It is the latest TRUST event anywhere on the run,
     PASS events included. A train 16 min late at Rugby that recovers to
     12 min at the ticket's destination shows DR15 (25%). Before the train
     reaches the destination, the estimate is whatever the last report
     said, with nothing to say it is provisional.
  2. **Wrong baseline.** WTT instead of public. At a padded terminus
     (10.9% of services with ≥ 2 min margin; up to 4 min) the WTT
     overstates lateness, so DS can say "eligible" when the operator will
     say "not eligible".
  3. **Truncation and missing bands.** Delays are truncated to the minute
     (`num_minutes`), and there is no 120-min band. Both are minor.
- **How sensitive the bands are.** In production right now, 2,713 of the
  9,869 en-route trains at ≥ 15 min sit in the 15–17 band, so a 1–3 min
  baseline error flips the DR15 answer for about a quarter of
  borderline-eligible trains.
- **Exposure is zero today.** `tracked_train_tickets` has 0 rows.
- **Legal risk is low.** `ds-review/uk-legal-compliance-2026-09-27.md`
  LEG-14 rates the consumer-law risk low (DS is probably not a "trader"
  under the DMCC Act 2024 Pt 4, and the copy is hedged).
- **It is still a correctness defect** in the one feature tied to a legal
  entitlement, and it should be fixed before the feature gets users.

### 5.2 Padding at destinations makes trains look late

Terminating arrivals, public minus WTT, Thu 2026-10-01 (n = 22,880):

- 0: 77.9%
- +0.5: 3.7%
- +1: 6.5%
- +1.5: 0.9%
- +2: 8.4%
- +3: 2.1%
- +4: 0.2%

Examples:

- London Overground 2C01 arrives at Euston at WTT 06:19, public 06:22.
- Avanti 9G43 arrives at Wolverhampton at WTT 22:03, public 22:04.

A train arriving at Euston at 06:21 is shown "2m late" by DS and is on time
for National Rail, Realtime Trains and the operator. The reverse, where
DS shows on time and the public timetable shows late, is rare (0.1%).

### 5.3 Origin departures differ from public

These are rare (0.9% after truncation). Examples:

- Avanti 1A51 from Manchester Piccadilly: WTT 14:35, public 14:34.
- ScotRail 1R07 from Edinburgh: WTT 18:18, public 18:15.

The board shows a departure time the passenger will not see on the
station screens. The usual case, public 10:00 against WTT 10:00H, already
displays correctly because of truncation.

### 5.4 Intermediate arrivals are 1 minute early

This affects 50% of calls, because the public time is rounded up and DS
truncates. It matters for:

- planner arrival times when alighting mid-route;
- the interchange margin: a planned change can look like `MCT + 0` on WTT
  and be `MCT − 1` on public times, or the reverse;
- the train page whenever a stop shows only an arrival.

### 5.5 Passing points and set-down/pick-up-only stops

Passing points are correctly hidden. For `D`/`U` stops see §4.

### 5.6 MCP mismatch

DS-MCP's local engine answers in public times. The DS path answers in WTT,
in the same fields with the same description. An LLM comparing two plans,
or a `find_services` result against a `plan_journey` result, will see
unexplained 1–3 min differences.

## 6. Recommendation

**The principle:**

- Public times for everything shown to users or computed for them.
- WTT internally for matching, identity, ordering, day-rollover and
  dedup.
- Store both, label both on the wire, and never compare across bases
  without saying so.

### 6.1 Priorities

| # | Change | Priority | Effort | Risk | Schema / backfill | API / MCP impact |
|---|---|---|---|---|---|---|
| P1 | **Delay Repay: compute against the public arrival at the ticket's (or subscription's) destination.** Use the TRUST ARRIVAL actual at `destination_crs` against the CIF `public_arrival` for that call (or TRUST `gbtt_timestamp` once stored). With no arrival yet, show "not yet known" instead of the latest delay. Add the 120-min band. | **High** (correctness, entitlement) | M | Low (0 users today) | Needs public arrival in `schedule_calling_points_full` (P3). No backfill needed: estimates are computed live | `delayMinutes` on the delay-repay response changes meaning; add `measuredAt: {crs, basis: "public-arrival"}` |
| P2 | **Direction-aware calls.** Keep `D` stops. Publish `can_board`/`can_alight` (or the raw activity) per call. The planner refuses boarding where `!can_board` and alighting where `!can_alight`. The train page labels "set down only", "pick up only" and "request stop". Rewrite the test that blesses the `D` drop and flip the two KNOWN WRONG tests. | **High** (wrong plans, missing stops) | M | Medium: touches the connection model shared by CSA, RAPTOR, reverse, staged and the overlay; the differential tests cover it | Add columns to `schedule_calling_points_full` (republished every cycle for 8 dates, so it **self-backfills** within one publish); `ScheduleCallingPointDto` fields with `serde(default)` | Additive: `JourneyStop.canBoard/canAlight/requestStop`. MCP: same, and use them in the local engine too |
| P3 | **Store public times end to end.** Add `public_arrival`/`public_departure` to `schedule_calling_points_full`, `schedule_destination_departures` and the `schedule_network_departures` JSON, and to `ScheduleCallingPointDto`. Store `gbtt_timestamp` on `train_movement_events` (and on `TrainMovementEventMessage`/the backlog). | **High** (enables everything else) | M | Low (additive, nullable) | Nullable `ADD COLUMN` (metadata-only); schedule tables self-backfill on the next publish; **TRUST gbtt cannot be backfilled** (`raw_body` is `{}`), forward only | none by itself |
| P4 | **Display public times.** Train page stops, timetable board, search, journeys and trip plans show public arrival and departure. Stops with no public time in a direction show "—" for that side. | Medium | M | Low | uses P3 | Additive `publicArrival`/`publicDeparture` (local `HH:MM`, nullable) alongside existing fields. Keep `scheduled*` as WTT for a deprecation window, then document it as `working*` |
| P5 | **Delay against public.** Per-stop `delayMinutes`, `lateMinutes`/status, journey and trip-plan live delay, notifications: `actual − public` at the *user's* stop. Keep the train-level WTT delay as an operational figure, named as such. Use `gbtt_timestamp` when present; otherwise CIF public. Stop the live overlay diffing TRUST actual against CIF (use TRUST's own planned/gbtt pair). | Medium | M | Medium: thresholds (15-min notifications, "On time") change meaning slightly; announce it | uses P3; forward only | Add `publicDelayMinutes`; or switch `delayMinutes` and add `workingDelayMinutes`. Decision for the user (Q2) |
| P6 | **Plan on public times.** Connections use public departure/arrival (falling back to WTT only where a call has no public time but is boardable or alightable, e.g. a missing `0000` field); MCT on public minutes. This matches DS-MCP's local engine and train-mcp. | Medium | S (once P2/P3 land) | Low/Medium: plans shift by ±1 min; the differential tests still apply | uses P3 | `scheduledDeparture`/`scheduledArrival` on `PlannedLeg` become public. Document it, or add `public*` + `working*` |
| P7 | **MCP contract.** Say which basis each field uses in the tool descriptions and `JOURNEY_FRAME_NOTE`/`DS_TRIP_PLAN_NOTE`; map DS `public*` into `departure`/`arrival`; optionally expose `workingDeparture` with `½`. | Medium | S | Low | — | MCP output schema text; add optional fields |
| P8 | **Keep WTT for identity.** TRUST↔schedule matching, pin → CIF match, `/public/trains/resolve`, ETA blend, day-offset assignment, dedup keys. Once public is stored, compare LDBWS `std` against CIF *public* (exact minute) instead of WTT ± 2 min. This tightens matching at busy termini. | Low | S | Low | — | none |
| P9 | **Half-minutes and passes, if WTT is ever shown.** Render `½` from the existing `isHalfMinute*` flags; decode the CIF pass time (`LI 20..25`) for an optional "detailed" view (Realtime Trains style). Not needed for the public view. | Low | S/M | Low | `is_half_minute_*` columns on `schedule_calling_points_full` if wanted | additive |
| P10 | **Keep LDBWS-based line severity and stats** (already public). Label full-coverage TRUST stats as working-timetable lateness in any user-facing copy. | Low | S | — | — | copy only |

Suggested order: P3 → P2 → P1 → P4/P5/P6 together → P7 → P8. P2 and P3 can
share one schedule-reference republish and one migration pair. Under the
migration rules each nullable `ADD COLUMN` is a transactional migration with
`SET LOCAL lock_timeout`, and no new index is needed.

### 6.2 API field naming

For each stop or leg, prefer explicit pairs over reinterpreting
`scheduled*`:

```
publicArrival / publicDeparture       "HH:MM" local or ISO instant, null = no public call in that direction
workingArrival / workingDeparture     ISO instant with seconds (":30" for half-minutes)
canBoard / canAlight / requestStop    booleans from Activity
```

`scheduled*` should stay as it is for one release with a deprecation note,
then become an alias of `public*`. The frontend and DS-MCP move to
`public*` straight away. The MCP's `get_train_status` already relays
`bookedArrival` and `isHalfMinute*`, so it can show both.

## 7. What others do

These points come from general knowledge of those services and were not
re-verified in this session.

- **National Rail Enquiries / LDBWS:**
  - shows public times only;
  - departure boards leave out set-down-only calls;
  - service details list public calling points only;
  - passing points appear only in the staff version (LDBSVWS);
  - "On time" means the estimate equals the public time.
- **Realtime Trains:**
  - the default (simple) view shows public times and public calls;
  - the "detailed" view shows WTT with `½`, passing points and
    allowances, and measures lateness against WTT;
  - calls are annotated with their activity (for example set-down or
    pick-up only, request stops).
- **Network Rail performance measures** (On Time, Time to 3/15, PPM) all
  use the public timetable.
- **train-mcp and DS-MCP's local engine** plan on public times: a
  connection needs a public departure and a public arrival.

DS is the outlier: it shows WTT-derived times without saying so, and
measures lateness against them.

## 8. Questions for the user

1. Should `scheduled*` change meaning to public in place, with WTT moved to
   `working*`, or should the API stay additive (`public*` added,
   `scheduled*` kept as WTT)? Additive is safer for DS-MCP; in-place is
   simpler for the frontend.
2. Should `delayMinutes` switch to the public basis, or should a separate
   `publicDelayMinutes` be added? This decides whether the 15-min
   notification threshold changes meaning.
3. Should Delay Repay show "not yet known" until the train has arrived at
   the ticket's destination, or keep showing a provisional figure based on
   the current delay?
4. Is a "detailed / staff" view (WTT with `½`, passing points) wanted, or
   should WTT never be shown?
5. Should request stops be flagged to users ("request stop: tell the
   conductor")?
6. Is it acceptable to give up the history of public delays? TRUST public
   times can only be stored from now on, because past raw bodies were not
   kept.

## 9. Decisions

The user answered §8 on 2026-10-01:

1. **API: additive.** Add `publicArrival`/`publicDeparture`,
   `workingArrival`/`workingDeparture` and `canBoard`/`canAlight`/
   `requestStop` per stop now. `scheduled*` keeps its WTT meaning for one
   release, documented as deprecated-soon, and is then switched to public
   or removed.
2. **`delayMinutes` switches to the public basis** at the user's own stop.
   This changes the meaning of the 15-minute notification threshold. The
   WTT delay is kept internally, for matching only.
3. **Delay Repay before arrival: provisional.** Before the train reaches
   the ticket's destination, show a band from the current public-time delay
   projected to the destination, clearly labelled provisional. It becomes
   final once the train has arrived at the destination. Measure against the
   public arrival at the ticket's `destination_crs`, and add the 120-minute
   band.
4. **A detailed WTT view is wanted**, as an optional view on the train page
   (half-minutes and passing points).
5. **Request stops are flagged to users.**
6. **No backfill of public delays.** Public-time delay history starts now;
   TRUST raw bodies were not stored.

## 10. Phase 1 implementation (2026-10-01)

Implemented: P3 (store public times), P2 (direction-aware stops), the
additive API fields, the frontend display and labels, and the detailed WTT
view (P9).

- **Storage.** Migrations `20261001160000` (`schedule_calling_points_full`:
  `public_arrival`, `public_departure`, `working_arrival`,
  `working_departure`, `working_pass`, `can_board`, `can_alight`,
  `request_stop`), `20261001160100` (`schedule_destination_departures`:
  `public_departure`, `public_calling_point_arrival`,
  `public_destination_arrival`) and `20261001160200`
  (`train_movement_events.gbtt_timestamp`). All nullable, catalog-only.
  The schedule tables refill on the next schedule-reference publish; the
  `schedule_network_departures` JSON carries `public_departure`;
  `trains.calling_points` carries the same fields for newly matched
  trains. The CIF pass time (`LI` `20..25`) is now decoded.
- **Direction.** `D` stops are published (`can_board = false`); only `OP`
  and `N` stops stay out. CSA, RAPTOR, the staged (waypoint) searches, the
  arrive-by backward scan and the live overlay never board where
  `!can_board` and never alight where `!can_alight`, but ride through.
  The `known_wrong_*` tests now assert the correct behaviour.
- **API.** `JourneyStop`: `publicArrival`, `publicDeparture`,
  `workingArrival`, `workingDeparture`, `workingPass`, `canBoard`,
  `canAlight`, `requestStop`. Schedule-departure board rows:
  `publicDeparture`. `/public/trains/search` rows: `publicDeparture`,
  `publicDestinationArrival`. Trip-plan train legs: `publicDeparture`,
  `publicArrival`, `publicDepartureDayOffset`, `publicArrivalDayOffset`.
  `scheduled*` keep their WTT meaning for one release.
- **Frontend.** Public times are shown first on boards, the train page and
  trip plans; intermediate stops are labelled "Set down only", "Pick up
  only" and "Request stop"; train pages have a collapsed "Detailed
  (working timetable)" view with `½` and passing points.

**Left for phase 2, and the seams for it:**

- **P5, `delayMinutes` on public.** `train_movement_events.gbtt_timestamp`
  (TRUST's own public time) and `JourneyStop.publicArrival`/
  `publicDeparture` are the baselines; `journey.rs`'s
  `overlay_movement_events` still diffs against WTT. The notifier's 15-min
  threshold changes meaning with it. `trust_event_backlog` does not carry
  `gbtt_timestamp` yet, so a backlog-matched movement stores NULL.
- **P1, Delay Repay.** Read `schedule_calling_points_full.public_arrival`
  at the ticket's `destination_crs` (and `gbtt_timestamp` once there);
  provisional band before arrival, final after; add the 120-minute band.
- **P6, plan on public minutes.** `schedule_query::Connection` carries
  only the WTT minutes and the direction flags; add public minutes there
  (from `CallingPointForConnections`, fed by
  `trip_planning::fetch_calling_points_for_date`) and change MCT on them.
  Until then the planner searches on WTT and `trip_leg_details` attaches
  the public times after the search.
- **Live overlay arithmetic** (`trip_plan_live.rs`) still diffs TRUST
  actual against the CIF WTT minute.
- **P7, MCP mapping** of `public*`/`working*`/`can*`.
- **Journeys leg search.** `schedule_destination_departures` still lists
  only boardable departures, so a journey leg cannot yet END at a `D`
  stop; the leg-candidate search does not carry public times yet.

## 11. Phase 2 implementation (2026-10-01)

Implemented: P5, P1, P6, the live-overlay arithmetic, the journeys leg
search and the passing-point date gap. P7 (the MCP's mapping) is the MCP
session's; `docs/api-changelog.md` lists every field and meaning change for
it.

- **The arithmetic** lives in `common::public_delay`, pure, with one shared
  read (`public_delay::db::stop_delays`) that api and notifier both use.
  - A reported TRUST movement is measured against its own `gbtt_timestamp`
    (`delayBasis: public`).
  - Without one, TRUST's `planned_timestamp` is moved by the CIF schedule's
    `public - working` gap at that call (`publicSchedule`). Movements
    matched from the backlog have no gbtt, nor do those stored before
    phase 1. No TRUST instant is ever diffed against a CIF one, which
    would reintroduce the hour-skew bug.
  - Failing both, the working time (`working`).
  - A call not yet reported is forecast: its working time plus TRUST's
    running delay, against its public time (so a terminus's recovery
    margin counts).
  - `plausible_delay_minutes` guards every measurement.
- **P5.**
  - `JourneyStop.delayMinutes` is on public times and gains `delayBasis`.
    `status`/`lateMinutes` compare the estimate with the public time.
  - Every train-level `delayMinutes` (tracked train, `/Train/mine`,
    journeys, groups, the public train page and line trains) is the delay
    at the passenger's own stop (`api::data::stop_delay`), with
    `delayBasis` and `delayProvisional`. On the public pages it is the
    latest reported call.
  - The notifier ranks each subscriber on the delay at their own stop.
  - `train_current_state.delay_minutes` (WTT, latest event anywhere) stays
    internal: the input to forecasts and per-stop estimates.
  - Station boards, line status and network statistics already used LDBWS
    public times and are unchanged. Full-coverage stays an operational
    WTT measure.
- **P1.**
  - Delay Repay is measured against the public arrival at the ticket's
    destination (else the pin destination, else the terminus).
  - It is final once the train has arrived there, and provisional before
    (`provisional`, plus a disclaimer that leads with it).
  - 120-minute band (100% of the return fare).
  - `delay_repay_rules` stays pure; `estimate_for` is the one place the
    route and the ticket list turn a delay into an estimate.
- **P6.**
  - `schedule_query::Connection::departure_min`/`arrival_min` are public
    times, falling back per call and direction to WTT.
    `working_departure_min`/`working_arrival_min` (also on `TrainLeg`)
    keep the WTT ones for `scheduled*`.
  - Every search and the minimum change times use the public minutes.
- **Live overlay.**
  - A reported call uses the delay the journey overlay measured from that
    TRUST report's own fields.
  - Darwin's `etd - std` is unchanged.
  - Estimates are compared with the public time.
  - Facts are looked up by WTT minutes, and plans shifted by public ones.
- **Journeys leg search.**
  - `schedule_destination_departures` also publishes set-down-only calls,
    with `can_board`/`can_alight` (migration `20261001170000`).
  - Boarding readers require `can_board`, destination sides `can_alight`.
  - Leg candidates carry `publicDeparture`, `publicDestinationArrival` and
    `legPublicDestinationArrival`.
- **Passing points after midnight.**
  - `assign_day_offsets` orders passes by their own (truncated) pass
    time.
  - The journey stops re-date a stored pass that reads more than 12 hours
    before the previous working time.

**Not done, deliberately:**
- `trust_event_backlog` still does not carry `gbtt_timestamp`, so a
  backlog-matched movement uses the `publicSchedule` fallback.
- `scheduled*` stay WTT for this release (§9 decision 1).

## Appendix: method

- **The extract.** The production extract
  `/data/schedule-feed/20260930T195959Z/RJTTF975MCA.txt` was copied
  read-only from the schedulefeed pod.
- **The analysis.** A standalone script resolved each UID for the date
  (C > N/O > P, valid days and date range) and then compared fields:
  - WTT arrival/departure (half-minute aware) against public
    arrival/departure, for passenger trains (status `P`/`1`);
  - passes (`LI` with a pass time; 194,590 on the Thursday) excluded;
  - "displayed minute" means the WTT time truncated, which is what DS
    shows.
- **Production queries** were `SELECT` only:
  - `trains.calling_points` population;
  - `train_movement_events.raw_body` (all `{}`);
  - `train_current_state` delay bands;
  - `tracked_train_tickets` (0 rows).
