# `/Trips/plan` from and to a TfL stop (2026-10-23)

Status: design, implemented on the branch that adds this file. Work log
DS-135 (a subtask of DS-134).

The user's decision (2026-10-22): the planner accepts a Tube, DLR or tram
stop with no National Rail station of its own, such as Uxbridge
(`940GZZLUUXB`), as a start or an end. It plans from or to a nearby
National Rail station and adds a Tube or walk leg for the last part. The
stops are the `tfl_stops` and `tfl_stop_crs` tables of DS-79
(`2026-10-20-tfl-stop-lookup-design.md`).

## 1. Inputs

`origin` and `destination` of `GET /Trips/plan` take, besides a CRS and a
`tiploc:` code, a TfL stop id: a NaPTAN metro or tram stop area, `940G`
and 4 to 16 upper-case letters or digits (`data::tfl_access::
is_tfl_stop_id`). A CRS is three letters and a `tiploc:` code has its
prefix, so the forms never overlap and CRS requests are untouched: they
do no new work at all (no read, no copy). Waypoints, vias and the avoid
lists still take stations only.

Errors, all `400`, before any graph is built: a stop id not in
`tfl_stops` ("'940G...' is not a known TfL stop"), a stop with no
candidate station ("no National Rail station could be found near ..."),
the same stop at both ends. A candidate whose station has no TIPLOC in the
timetable is dropped; a stop left with none is the second error.

## 2. Access candidates

`data::tfl_access::access_candidates`, over every `tfl_stops` row (about
350) and the `stations` within 3 km. Each station once, quickest first
(until DS-167 the first rule that found any won, so a hub stop had only
its hub walk):

1. **Hub.** The stop's own National Rail station: its `tfl_stop_crs`
   rows, or a `tfl_stops::CRS_OVERRIDES` entry naming it (Euston's Tube
   stop is EUS). A walk of 5 minutes; a hub station is never offered
   another way.
2. **Line and walk**, for a hub stop too (DS-167: Old Street's station,
   `OLD`, has a few trains to Moorgate; its Northern line ride to Euston
   is the journey). For each of the stop's lines, the nearest 5 stops
   along the line that have a hub station (rule 1 there), every station
   of each hub (King's Cross St. Pancras gives KGX, STP and SPL), by a
   ride of at most 90 minutes; plus every station within 1 km in a
   straight line, on foot. At most 16 stations with the hub's, each once
   (the quicker way when two lines reach it).
3. **Nearest**, only when rules 1 and 2 find none. The 3 nearest stations
   within 3 km, on foot.
4. **Station ride** (DS-167). When the other end of the plan is a
   station whose own stop (rule 1 there: `CRS_OVERRIDES` or
   `tfl_stop_crs`) shares a line with this stop, that station is a
   candidate by the ride between the two stops at the direct ride's
   minutes (below: no walk at the station end, where the journey starts or
   ends), whatever rule 2's cuts, in place of a slower candidate for it.
   So Euston to Old Street (`940GZZLUODS`) is the Northern line ride
   (about 3.4 km, 12 minutes), not the Tube link to Moorgate, a train
   to Old Street and the hub walk (38 minutes).

## 3. Estimates

DS has no TfL timetables or journey times, so each time is a model,
conservative and stated in the response (`access.basis`,
`distanceMetres`, and since DS-151 `stopCount` and `changeAt`):

- **Ride**: 5 minutes to walk in and wait (the average wait), the
  distance along the line at the line's speed, and 3 minutes from the
  platform to the National Rail station. Speeds: Tube 30 km/h (TfL quotes
  about 33 km/h for the Underground, stops included), DLR 25, tram 18.
  The two walks follow the curated links' convention
  (`reference-data/supplementary-fixed-links.csv`: a link's minutes
  include the walks at both ends); the planner still adds the station's
  minimum change time before the train, as after any link.
- **Distance along a line** (DS-151): along the line's routes, TfL's
  route sequences (below). A ride's distance is the sum of the straight
  lines between consecutive stops of a route, and `stopCount` the stops
  it rides. The rule:
  1. One route with the start and then the end (either may come twice,
     as on the Circle; the shortest pair counts): a ride with no change.
  2. Otherwise a route from the start and a route on to the end that
     share a stop: one ride that changes trains at the shared stop making
     it shortest, the branch junction (Harrow-on-the-Hill for Uxbridge to
     Rickmansworth, Acton Town for Uxbridge to Heathrow). It costs 5
     minutes to change (`BRANCH_CHANGE_MINUTES`) and 5 more waiting
     (`BRANCH_CHANGE_WAIT_MINUTES`, half a branch's typical 10-minute
     headway, as the second train is waited for again), and names the
     junction as `changeAt`.
  3. Otherwise (it would take two changes on one line): no ride; the
     candidate is not offered.
  A line with no routes stored, or a start or end no route of its line
  has, keeps the pre-DS-151 estimate: the path in a minimum spanning tree
  of the line's stops by straight-line distance (Prim's), which is close
  to the track but blind to changes between branches. A route stop that
  `tfl_stops` does not have (or has no position for) is passed over.
  Rides into a stop (a destination) run the routes the other way, so a
  direction-only route is honoured. A ride stays on one line; changes
  between lines are not modelled. The rule-2 candidates per line are now
  the quickest 5, not the nearest 5, so a branch change can drop out.
- **Walk**: the straight-line distance times 1.25, at 4.5 km/h.
- **Hours**: rides can be started from 05:30 to 00:30 every day (two
  link windows); the Night Tube is not modelled. Walks at any time.
- **Two stops on one line** (origin and destination both stops): also a
  ride straight between them (5 minutes in, then the ride; no walk at the
  far end), up to 150 minutes.

### Route sequences (DS-151)

poller-tfl's route-sequences job (`TFL_ROUTES_ENABLED`, chart
`pollers.tfl.stops.routes`, off by default, only beside the stops job)
reads `GET /Line/Mode/tube,dlr,tram`, then each line's `GET
/Line/{id}/Route/Sequence/{inbound,outbound}` (about 27 calls), and keeps
each sequence's `orderedLineRoutes` of service type `Regular`: a route end
to end (`Uxbridge ↔ Aldgate`), its NaPTAN stop areas in running order. The
ordered routes, not the `stopPointSequences` branches, because a route is
what one train runs: "one route has both stops in order" is exactly "no
change". A route with an id that is not a stop area, fewer than 2 or more
than 200 stops is dropped whole (a missing stop would measure short). The
run publishes `tfl-line-routes/1` (about 80 routes, one part) on
`ds:ingest:tfl-stops` with its own producer and cursor (`cycle =
"tfl-routes"`), at the stops job's interval and under its feed guard: an
empty run, a line with no routes, or a route count down more than
`maxDropFraction` on the last snapshot is refused
(`distant_signal_tfl_routes_refused_total{reason}`) and the stored routes
stay. The ingest-writer (`tfl-stops: apply` covers both schemas) replaces
`tfl_line_routes` (`line_id, direction, route_index, name, stop_ids
TEXT[], fetched_at`; writer SID, api S; freshness `tfl-line-routes`),
skipping an older snapshot. The api reads the table with `tfl_stops` for
each stop request.

## 4. Search

`trip_planner::access::with_access` copies the cached graph's
`InterchangeData` for the request and adds the stop as a virtual stop (its
id as both its CRS and its only TIPLOC) with one fixed link per candidate:
out of a virtual origin, into a virtual destination. Every search already
seeds fixed links from its origins and follows them into its
destinations, so this is the multi-source, multi-target search with no
change to Connection Scan, RAPTOR, the arrive-by search, waypoints, vias
or the avoid lists (an avoided station is never walked into, so its
candidate is unusable). Links run one way only, so no journey passes
through a virtual stop. The graph cache is never changed; the copy costs a
few milliseconds, and only for a stop request.

The in-search live snapshot (DS-140) and the re-plans run on the same
copy (`PlanRequest::interchange`). A stop origin has no "late train from
the origin" seeds (no train leaves the stop itself).

## 5. Output

The access leg is a `kind: "transfer"` leg (mode `TUBE`, `METRO` or
`WALK`), dressed by `AccessEnds::finish_legs` after the TfL stops and
lines are attached:

- the stop end's code becomes `originCrs`/`destinationCrs` `null`, with
  the stop as `originStop`/`destinationStop`;
- a ride names its line and the stop it reaches at the station end
  (`Harrow-on-the-Hill`);
- `access: {end, basis, distanceMetres}`.

So the existing ride merge (DS-139), saving (`tripPlanSave`'s
`transferAsUntracked`: the stop's name and id) and journey creation work
unchanged. A depart-after itinerary that starts with an access leg leaves
as late as still makes its first train (its departure less the legs
before it and the minimum change time there): the search itself starts
the leg at `departAfter`. Arrive-by needs nothing: its search already
leaves as late as it can.

The response gains a top-level `access` (`{origin, destination}`, each
`{stop, candidates}` or `null`) only when an end is a stop.

## 6. Uxbridge

On a copy of production's timetable for Wednesday 2026-10-28 and its 356
TfL stops, Uxbridge (Metropolitan and Piccadilly, no station within 1 km)
gets these candidates:

| Station | Line | Along the line | Estimate |
| --- | --- | --- | --- |
| Harrow-on-the-Hill (HOH) | Metropolitan | 11.7 km | 32 min |
| Rickmansworth (RIC) | Metropolitan | 21.5 km | 52 min |
| Chorleywood (CLW) | Metropolitan | 25.0 km | 58 min |
| Chalfont & Latimer (CFO) | Metropolitan | 28.3 km | 65 min |
| King's Cross, St Pancras (KGX, STP, SPL) | Metropolitan | 29.1 km | 67 min |
| Heathrow Terminal 4 (HAF) | Piccadilly | 31.6 km | 72 min |
| Heathrow Terminals 2 and 3 (HXX) | Piccadilly | 33.1 km | 75 min |
| Heathrow Terminal 5 (HWV) | Piccadilly | 35.8 km | 80 min |
| Finsbury Park (FPK) | Piccadilly | 37.4 km | 83 min |

A Metropolitan line train takes roughly 20 minutes from Uxbridge to
Harrow, so 32 with the walk in, the wait and the walk out is on the safe
side. Plans leaving at 08:00:

- to Aylesbury: Tube to Harrow-on-the-Hill, the 08:39 Chiltern, 09:25
  (leave 08:04);
- to Reading: Tube to Harrow, the 08:44 to Marylebone, the Bakerloo line
  to Paddington, the 09:23, 09:46 (leave 08:09); `options` adds the
  Metropolitan to King's Cross, the Circle line to Paddington and the
  09:48, 10:11, with no change of train;
- to Birmingham New Street: the Metropolitan to King's Cross, the Tube
  to Euston and the 09:36, 10:55 (leave 08:09);
- Birmingham New Street to Uxbridge, arrive by 18:00: the 15:17 to
  Euston, then the Tube via King's Cross, 17:52;
- Uxbridge to Cockfosters (both stops): one Piccadilly line ride, 104
  minutes, 49 km.

The suggestion of Ealing Broadway and the Piccadilly line does not apply:
Ealing Broadway is on the Central and District lines; the Piccadilly's
nearest stop to it is Ealing Common, about 800 m away, which is not a
hub, so no rule offers it.

Latency on the same copy (release build, graph cached, median of 5): CRS
plans 8 to 47 ms (`fastest`) and 540 to 570 ms (`options`), as before;
Uxbridge plans 43 to 58 ms and 610 to 620 ms. The extra is reading
`tfl_stops` and copying the interchange data.

### After DS-151

The same 356 stops (a read-only copy of production's `tfl_stops` on
2026-10-23) with TfL's route sequences of that day (82 routes). Uxbridge's
candidates, before (spanning tree) and after (routes):

| Station | Line | Before | After |
| --- | --- | --- | --- |
| Harrow-on-the-Hill (HOH) | Metropolitan | 11.7 km, 32 min | 11.0 km, 8 stops, 30 min |
| King's Cross, St Pancras (KGX, STP, SPL) | Metropolitan | 29.1 km, 67 min | 28.2 km, 16 stops, 65 min |
| Rickmansworth (RIC) | Metropolitan | 21.5 km, 52 min | 23.3 km, 14 stops, change at Harrow-on-the-Hill, 65 min |
| Farringdon (FDX, ZFD) | Metropolitan | not offered | 30.0 km, 17 stops, 68 min |
| Moorgate (MOG) | Metropolitan | not offered | 31.1 km, 19 stops, 71 min |
| Chorleywood (CLW), Chalfont & Latimer (CFO) | Metropolitan | 58, 65 min | not offered (change at Harrow; not in the quickest 5) |
| Finsbury Park (FPK) | Piccadilly | 37.4 km, 83 min | 37.4 km, 33 stops, 83 min |
| Heathrow Terminal 4 (HAF) | Piccadilly | 31.6 km, 72 min | 32.8 km, 23 stops, change at Acton Town, 84 min |
| Heathrow Terminals 2 and 3 (HXX) | Piccadilly | 33.1 km, 75 min | 33.1 km, 23 stops, change at Acton Town, 85 min |
| Heathrow Terminal 5 (HWV) | Piccadilly | 35.8 km, 80 min | 35.7 km, 24 stops, change at Acton Town, 90 min |

Uxbridge's Piccadilly trains run to Cockfosters, and its Metropolitan
trains to Aldgate, so Rickmansworth and Heathrow each take a change, now
charged. Stop to stop, Uxbridge to Cockfosters is still one ride (49.0 km,
41 stops, 104 minutes), and Uxbridge to Rickmansworth is one ride with a
change at Harrow-on-the-Hill, 62 minutes (49 before).

## 7. Frontend

The planner's From and To search (`lib/plannerEnds.ts`) adds TfL stops
after the stations, bus stops and ferry terminals, ranked as `/stations`
ranks them, named `Uxbridge (Tube)` with their lines. Picking one writes
its stop id; the field then says what it is and that the route starts (or
ends) with the Tube or a walk to a station nearby. The results name the
stop and say each access leg's time is an estimate ("Estimated time for
about 11.7 km along the line"). Saving keeps the access leg as an
untracked leg with the stop's name and id, as for any Tube transfer.

## 8. Open questions

- TfL's own journey times (`/Journey/JourneyResults` or the line
  timetables) would replace the model, at the cost of a new feed.
- Line sequences now measure rides and charge a change between branches
  (DS-151); they could also model a change between lines.
- Buses to a station (Uxbridge's buses to West Drayton or Denham) are not
  offered; DS-77 is buses.
