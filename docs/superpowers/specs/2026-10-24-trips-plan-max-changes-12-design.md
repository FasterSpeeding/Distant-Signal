# `/Trips/plan`: `maxChanges` up to 12, default 4

DS-85, 2026-10-24. Supersedes the change cap and default of
[2026-10-06-trips-plan-via-and-max-changes-design](2026-10-06-trips-plan-via-and-max-changes-design.md)
(ceiling 6, default 2). DS-85's first pass (2026-10-21) kept 6. Since then
fixed links are read both ways, a Tube stop can be an end (DS-135), the
live planner applies delays inside the search (DS-140), and waypoint and
via plans count changes end to end. All of these can add changes, so it
was measured again.

## Decision

| | Before | Now |
|---|---|---|
| `MAX_CHANGES_LIMIT` | 6 | 12 |
| `DEFAULT_MAX_CHANGES` (omitted `maxChanges`) | 2 | 4, or less down to 2 where the options search-size guard needs it |
| `MIN_OPTIONS_SEARCH_SIZE` (chart floor) | 8 | 14 (`max_rounds(12)`) |
| `DEFAULT_OPTIONS_SEARCH_SIZE` | 252 | 252 (unchanged) |
| `exceedsRecommendedChanges` (fastest) | over `maxChanges` | unchanged rule, so over 4 by default |

- **Ceiling 12.** No measured plan needed more than 6, but via and
  waypoint plans already reach exactly 6, with only the headroom round
  left to see a 7th. Cost is near zero: RAPTOR stops at the first round
  that improves nothing, and every real search had converged within 8
  rounds. 16 bought nothing over 12.
- **Default 4.** At 2, 14 of 35 depart-after and 13 of 35 arrive-by
  `options` searches found nothing. Their fewest-change route needs 3 or
  4 changes: rural and cross-country trips, and Tube-stop ends to rural
  stations. At 4 that drops to 3 of 36 and 1 of 34. Each change the
  Pareto set added beyond 2 was 21 to 175 minutes faster, except one (6
  minutes). London crossings and Tube-stop trips got the same answer at 2
  as at 12: the Pareto set never adds a change that isn't faster.
- **Not adaptive.** We considered searching up to N and showing extra
  changes only when they save at least 15 minutes each. Of the 13 options
  measured where a third or fourth change bought an earlier answer, only
  one fell below that threshold. The filter would add a second rule to
  drop one result. Pareto dominance already does
  most of that work.
- **Compatibility.** An omitted `maxChanges` with many waypoints and vias
  steps down from 4 towards 2 until the search fits the guard
  (`routes::trips::default_max_changes`). Every request that fitted at
  the old default still fits. The echoed top-level `maxChanges` is the
  value used.
- **Fastest mode.** The flag still compares against the `maxChanges` in
  effect. With 2 it flagged 37 of 71 fastest answers; with 4 it flags 11.
  In 19 of 71 cases CSA's earliest-arrival path used more changes than
  RAPTOR needed for the same time: STR-CMR 6 against 4, WTB-PWL via
  SHF,CRE 8 against 6. So some flags are about CSA's choice of path, not
  the trip. That is an open follow-up (prefer fewer changes on ties); it
  does not change the threshold.

## Method

Prod's timetable for Tuesday 2026-10-27 was copied read-only (`COPY ...
TO STDOUT`) into a scratch Postgres: about 482k calling points, plus the
reference tables. A local release api ran against it with `live=false`;
the service date is in the future, so no live data applies. 36 trips:

- long cross-country: PNZ-INV, KYL-LWT, AYW-NRW, STR-CMR, MLG-FAL,
  KLM-SKG, OBN-BNP, WCK-PLY;
- rural to rural: PWL-WTB, SIV-SKG, GSL-CMR, LOO-HXM, LLO-HBB, BRM-STV,
  MCN-ALM;
- London crossings: CBW-MKC, DVP-OXF, HGS-ELY, SOV-RDG, SAL-IPS,
  DMS-CBG, MAL-HUN;
- Tube and DLR stop ends: Uxbridge to MCN and WTB, Cockfosters to PNZ,
  Epping to BRM, Morden to LWT, GSL to Stanmore, Beckton to OBN;
- vias and waypoints: PNZ-INV via YRK, AYW-NRW via waypoint BHM,
  BTN-ELY via `group:LON`, WTB-PWL via SHF,CRE, SIV-SKG via waypoint EXD
  and via BHM, Uxbridge-WTB via YRK, CMR-FAL via waypoints CBG,OXF.

Each trip ran depart-after and arrive-by, in fastest and options mode.
Wall time was dominated by other builds on the host (load average 70 to
200), so CPU seconds of the api process per request is the cost metric.

## Results

Run 1 (caps 6, 8, 10, 12 and 16; 2 repetitions; 1,440 requests):

| | cap 6 | 8 | 10 | 12 | 16 |
|---|---|---|---|---|---|
| options answers different from cap 6 | - | 0 | 0 | 0 | 0 |
| most changes in any options answer | 6 | 6 | 6 | 6 | 6 |
| options depart-after CPU, median / p95 (s) | 1.68 / 3.32 | 1.69 / 3.76 | 1.63 / 3.53 | 1.64 / 3.57 | 1.65 / 3.48 |
| options arrive-by CPU, median / p95 (s) | 1.07 / 3.26 | 0.94 / 2.91 | 0.91 / 3.37 | 1.02 / 4.12 | 0.91 / 3.36 |
| fastest depart-after CPU, median / p95 (s) | 0.13 / 0.53 | 0.13 / 0.44 | 0.12 / 0.40 | 0.12 / 0.44 | 0.13 / 0.48 |
| fastest arrive-by CPU, median / p95 (s) | 0.22 / 0.71 | 0.21 / 0.63 | 0.20 / 0.75 | 0.19 / 0.61 | 0.22 / 0.61 |

Run 2 (caps 2, 4, 6 and 12 plus an omitted `maxChanges`, paired; one
repetition; options only):

| | cap 2 | 4 (= omitted) | 6 | 12 |
|---|---|---|---|---|
| depart-after with no itinerary | 14/35 | 3/36 | 1/36 | 1/36 |
| arrive-by with no itinerary | 13/35 | 1/34 | 0/34 | 0/34 |
| depart-after CPU, median / p95 (s) | 0.80 / 1.45 | 1.36 / 1.92 | 1.56 / 2.07 | 1.68 / 2.97 |
| arrive-by CPU, median / p95 (s) | 0.36 / 1.54 | 0.69 / 1.77 | 0.79 / 2.55 | 0.81 / 2.42 |
| total CPU against the column to its left | - | x1.60 dep, x1.55 arr | x1.13, x1.28 | x1.11, x1.01 |

The omitted-`maxChanges` requests matched cap 4 exactly and echoed
`maxChanges: 4`. The remaining empty searches find nothing at any cap:
KYL-LWT departing after 06:00 can't arrive the same service day (the
multi-day gap, DS-108). CMR-FAL via waypoints CBG,OXF and WTB-PWL via
SHF,CRE need 5.

Four plan slots, four of the heaviest searches at once (CMR-FAL via 2
waypoints, WTB-PWL via 2 vias, PNZ-INV via YRK, SIV-SKG via a waypoint
and a via), two repetitions:

| cap | depart-after CPU (s) | arrive-by CPU (s) | peak RSS over the api's ~0.5 GB baseline |
|---|---|---|---|
| 2 | 4.8-5.0 | 3.3-3.4 | +11 to +44 MB |
| 4 | 6.5-6.8 | 4.2-4.8 | +11 to +36 MB |
| 6 | 5.8-6.7 | 6.6-7.5 | +28 to +38 MB |
| 12 | 8.2-8.4 | 7.1-7.7 | +65 to +67 MB |

The day's graph dominates memory. Per-search labels at 12 add about 65 MB
across four slots.

## Cost of the new default

The higher default is the material cost, not the ceiling. An `options`
search with `maxChanges` omitted costs about 1.6x the CPU it did at 2:
median 0.8 s to 1.4 s depart-after, and a plan slot is held that much
longer. In exchange it answers about a third more trips. `fastest` (the
web app's default results mode) does no rounds and is unaffected. The
synthetic bench (`bench_max_changes_cap` in
`crates/trip-planner/tests/bench_arrive_by.rs`) shows the same pattern.
The algorithm is unchanged, so any given cap costs what it did before.

## DS-MCP

Its default of 4 now matches DS. Its schema allows up to 24, and DS
rejects 13 to 24 with a 400. It should lower its ceiling to 12 (no change
made here).
