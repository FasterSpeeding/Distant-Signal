# Transit networks in the trip planner: Metrolink, London Trams and after

> **Status note, 2026-10-30.** §3.3's London counts come from the BODS
> London GTFS, which merges TfL's engineering-work files with their base
> files, so London Trams (998 weekday trips) and the Underground (9,814)
> are inflated. §12's `london-trams` network and §13.2's DLR are
> superseded by one `tfl` network from TfL's own timetables. See
> [the TfL transit design](2026-10-30-tfl-transit-design.md), §16.

Design, 2026-10-24. Status: **accepted**. Work log DS-162 (a subtask of
DS-153). The user accepted every proposed default of
[§16](#16-open-questions-for-the-user) on 2026-10-24; the decisions are
recorded there.

This is the "trip planner" item of the transit networks spec's later work
([2026-10-09-transit-networks-design.md](2026-10-09-transit-networks-design.md)
§15.2, "the transit spec" below): "Tram patterns would go into the RAPTOR
connections, with footpaths from `transit_rail_links` and NaPTAN. It needs
a common stop identity across rail and tram, and a policy on
frequency-based trips." It also brings London Trams (Croydon) into scope
(the user's request of 2026-10-24), which today reach the planner only as
the modelled access legs of `crates/api/src/data/tfl_access.rs`
([2026-10-23-trips-plan-tfl-stop-ends-design.md](2026-10-23-trips-plan-tfl-stop-ends-design.md),
"the TfL stop ends spec").

## Contents

1. [Summary](#1-summary)
2. [Scope](#2-scope)
3. [Measurements](#3-measurements)
4. [Connections or access legs](#4-connections-or-access-legs)
5. [Stop identity](#5-stop-identity)
6. [Building the tram connections](#6-building-the-tram-connections)
7. [Scheduled, not live](#7-scheduled-not-live)
8. [Footpaths and interchange times](#8-footpaths-and-interchange-times)
9. [Arrive-by and the other searches](#9-arrive-by-and-the-other-searches)
10. [Memory and latency budget](#10-memory-and-latency-budget)
11. [API and frontend](#11-api-and-frontend)
12. [London Trams](#12-london-trams)
13. [Other networks: NET, DLR and after](#13-other-networks-net-dlr-and-after)
14. [Rollout](#14-rollout)
15. [Tests, phases and effort](#15-tests-phases-and-effort)
16. [Open questions for the user](#16-open-questions-for-the-user)
17. [Implementation notes (DS-164)](#17-implementation-notes-ds-164)

## 1. Summary

**Recommendation: tram trips become ordinary connections in the planner's
day graph** (option A, §4), not access legs like the TfL model (option B).
The measured cost is small: a full Metrolink weekday adds 27,614
connections to the National Rail day's ~457,000 (6%), and London Trams add
17,585 (4%). In return a tram can be ridden anywhere in a journey (rail to
Piccadilly, tram to Victoria, rail on), changes between tram lines work,
legs carry real scheduled departure times, and Connection Scan, RAPTOR,
arrive-by, waypoints and vias need no algorithm change.

- **Stop identity** (§5). A tram station is a planner node named by its
  NaPTAN code. Its platforms are its TIPLOC-like members, so the existing
  "same-CRS sibling" rules charge platform changes. Metrolink stations keep
  their public `9400ZZ…` ids, London tram stops keep their TfL `940G…` ids,
  and the planner accepts either spelling of the same NaPTAN stop area.
- **Scheduled, not live** (§7). Every tram leg says `realtime: false` and
  `liveTracking: false`, never gets a `live` block, and is shown with
  "Scheduled times, not live" and an "about every N min" hint taken from
  the timetable.
- **Footpaths** (§8). Rail-to-tram walks come from `transit_rail_links`,
  with the minutes worked out from NaPTAN and station coordinates. Each
  tram platform has a change time from its network file.
- **London Trams** (§12). The BODS London GTFS carries London Tramlink
  (NOC `TRAM`) with no key, under the OGL. So London Trams can be a
  `transit_*` network, `london-trams`, that feeds the planner only: no
  `/transit` pages and no transit line status, because TfL's stop pages
  and its `tfl-tram` status already cover it.
- **Rollout** (§14). A new list, `api.tripPlanTransitNetworks` (empty by
  default), plus a request opt-in (`transit=true`). With the list empty, or
  without the opt-in, every response is byte-identical to today's.
- **Effort** (§15). Metrolink 11–15 engineer-days. London Trams 6–8 more,
  after the Metrolink work and the Old Street fix to `tfl_access.rs`.

## 2. Scope

**In scope:**

- tram connections from the active `transit_*` feed of each enabled network
  in the planner's day graph;
- tram stations as `/Trips/plan` origins and destinations;
- rail and tram walks and change times;
- a new `kind: "transit"` leg, and how it is shown and saved;
- London Trams as a planner-only network from the BODS London GTFS;
- what carries over to networks without a timetable;
- the switch, the tests and the effort.

**Out of scope:**

- **Live tram times.** Metrolink has none (the transit spec's D1 and Q7).
  London Trams have TfL arrivals on the TfL stop page (DS-136), but the
  planner does not use them (§7.4).
- **The Tube timetable.** The BODS London GTFS also has London Underground
  (§3.3), but that adds half again to the graph, and it is a separate
  decision (§13.3).
- **Tram stations as waypoints, vias or avoids.** These still take
  National Rail stations only (Q9).
- **Journey alternatives and re-plans restricted to "same operator"**
  (`allowed_uids`) ride no trams (§9).
- **Bus networks** (DS-77).

## 3. Measurements

The Metrolink figures were measured on 2026-10-24 with read-only queries on
production's active feed (`20261022_034821-919c3aa5…`, horizon
2026-10-22 to 2026-12-22). The London figures come from the BODS London
GTFS downloaded the same day (`feed_version 20261022_032716`) into the
session scratchpad. Only counts appear here. No feed rows are committed.

### 3.1 Metrolink per service day

| Day | Trips | of which replacement bus | Connections (stop pairs) |
|---|---|---|---|
| Wed 2026-11-04 (all seven lines) | 1,529 | 0 | **27,614** |
| Sat 2026-11-07 | 1,390 | 0 | 25,390 |
| Sun 2026-11-08 | 1,147 | 0 | 18,547 |
| Wed 2026-10-28 (closure week: Green, Red and Yellow lines not running) | 1,676 | 789 | 17,223 (14,388 tram) |

- **The feed version.** 7 lines, 99 stations, 200 platforms (all with
  coordinates), 137 patterns (1,684 pattern stops, at most 33 stops long)
  and 12,932 trips. Trip ids are 42 characters long.
- **Times.**
  - 688 of the 1,684 pattern-stop offsets (41%) and 2,859 of the 12,932 trip
    starts (22%) are not whole minutes.
  - No stop has a dwell: arrival equals departure everywhere.
  - 137 pattern stops are set-down only and 137 are pick-up only (the
    first and last stops).
- **After midnight.** No trip has `start_s ≥ 86400`: a tram after midnight
  is a 00:xx trip of the next service date.
- **Frequency.** Between 08:00 and 09:00 on 2026-11-04 each line runs 4 or
  8 trips an hour in each direction (every 15 or 7.5 minutes). The busiest
  stops have 72 departures an hour (St Peter's Square) and 56 (Victoria).
  Piccadilly has 32 and the Airport 8.
- **Gaps.** One Monday in the horizon (2026-11-30) has no service day in
  the active feed. A planner must not read that as "no trams" (§7.3).
- **Replacement buses** call at tram platform codes (31 platforms and one
  station code in the closure week), so they need no extra stops.
- **Rail links.** `transit_rail_links` has 10 rows: 8 `interchange` and 2
  `nearby` (Deansgate-Castlefield for DGT, Eccles for ECC).

### 3.2 National Rail, for scale

Wednesday 2026-10-28 has 483,008 `schedule_calling_points_full` rows on
26,455 schedules, so about 457,000 connections. `GraphCache` puts one
built date at "on the order of 100 MB" (§10). The ALF already has a
generic Manchester Piccadilly to Victoria `TRANSFER` link of 22 minutes,
which stays (§8.1).

### 3.3 The BODS London GTFS

`https://data.bus-data.dft.gov.uk/timetable/download/gtfs-file/london/` is
anonymous (HTTP 200) and under the OGL, like the North West file. The zip
is 321 MB and unzips to 1.65 GB, of which `stop_times.txt` is 1.58 GB. The
calendar runs to 2027-08-01.

| Agency (NOC) | Routes | Day | Trips | Connections | vs rail |
|---|---|---|---|---|---|
| London Tramlink (`TRAM`) | 2, both `route_short_name` "Tram", `route_type` 0 | Wed 2026-11-04 | 998 | **17,585** | 3.9% |
| | | Sat 2026-11-07 | 659 | 12,044 | |
| | | Sun 2026-11-08 | 515 | 8,860 | |
| London Docklands Light Railway (`LDLR`) | 4, "DLR", `route_type` 2 | Wed 2026-11-04 | 1,584 | 18,990 | 4.2% |
| London Underground (`LULD`) | 26, one per line name, `route_type` 1 | Wed 2026-11-04 | 9,814 | 226,430 | 49.6% |

- **Tram stops.** 39 stations and 66 platforms. `parent_station` is the
  NaPTAN stop area (`940GZZCRWMB`) and `stop_id` the platform
  (`9400ZZCRWMB1`). This is the shape the Metrolink adapter already maps
  (`940G` to `9400`, transit spec §6.3).
- **TfL's tables.** `tfl_stops` has the same 39 tram stops, keyed by the
  `940G` ids. `tfl_stop_crs` links 7 of them to National Rail stations:
  BKJ, BIK, ECR, ELE and MIJ by TIPLOC, and WCY and WIM as hubs.
- **No frequencies.** `frequencies.txt` has 81 rows, none for tram, DLR or
  the Underground. Every trip is scheduled.
- **The trips at 08:00 on 2026-11-04** split 38 and 16 between the two
  "Tram" routes.

## 4. Connections or access legs

### 4.1 Option A: tram trips as connections (recommended)

Each tram trip of the date becomes `Connection`s in the same array as the
trains (§6). Tram stations become nodes, joined to rail stations by walks
(§8).

- **For.**
  - A tram can be ridden anywhere in a journey: rail into Piccadilly, the
    tram to Victoria, rail out. Rail into East Croydon, the tram to
    Wimbledon, rail out. Neither is possible today. The TfL model rides
    only at the ends and on one line.
  - Changes between tram lines work (Altrincham to the Airport via
    Cornbrook).
  - Legs carry the timetable's departure and arrival times, not a modelled
    "minutes" figure.
  - The arrive-by search, waypoints, vias, the change cap and the
    `options` Pareto set all apply unchanged, because they work on the
    array.
  - No search algorithm in `crates/trip-planner` changes.
- **Against.**
  - Every leg consumer must handle a non-CIF service: leg details, the
    live overlay, the ride merge, saving and DS-MCP (§11).
  - The graph grows by 6% (Metrolink) or 10% (with London Trams). Every
    sweep grows by the same amount (§10).
  - The graph cache must also key on the transit feed (§6.4).

### 4.2 Option B: access and egress legs, as for TfL

A Metrolink stop end becomes a virtual stop, as a `940G` TfL stop does
today (`trip_planner::access::with_access`). It gets fixed links to the
rail stations of `transit_rail_links`. Their minutes come from the
timetable's own run times (the pattern offsets) plus a wait of half the
headway.

- **For.**
  - About 3–4 days of work, mostly reusing `tfl_access`'s shape.
  - The graph and the cache do not change.
- **Against.**
  - Trams only at the ends of a journey, and only to or from a linked rail
    station. Ten Metrolink stations have one; the other 89 can only start
    or end a journey.
  - No departure times. A tram leg would read "Tram, 23 min", which is
    worse than the timetable DS already holds.
  - It cannot model the cross-city Piccadilly to Victoria change that most
    Manchester rail journeys need. The 22-minute ALF transfer would remain
    the only way.
  - Every candidate set has to be computed per request.

### 4.3 Decision

**Option A.** Option B is a step backwards from the data DS already
ingests. It is only worth considering as a stopgap for a network with no
timetable (§12.4). A tram stop end under option A is an ordinary node. It
needs no virtual stop and no `with_access` copy.

## 5. Stop identity

### 5.1 Nodes

The planner's `InterchangeData` names a station by a "CRS" key that lists
its TIPLOCs (`crs_to_tiplocs`), and maps each TIPLOC back
(`tiploc_to_crs`). A tram network maps onto that as follows:

| Planner concept | Rail | Tram |
|---|---|---|
| station ("CRS" key) | `MAN` | the station's NaPTAN access-node code, `9400ZZMAPIC` (the `transit_stops` `station` row) |
| member ("TIPLOC") | `MNCRPIC` | each platform, `9400ZZMAPIC1`, `9400ZZMAPIC2`, … (`platform` rows, by `parent_stop_id`); a stop called at directly with no platform row (one replacement-bus stop, §3.1) is its own member |
| change time | MSN, per TIPLOC | the network file's `change_minutes`, per platform (§8.2) |

The code spaces cannot collide. A CRS is three letters, a TIPLOC at most
seven characters, a `tiploc:` end has its prefix, a TfL virtual end is
`940G…`, and a transit node is `9400…` and at least eleven characters. The
`transit_stops.stop_id` CHECK (`^[A-Z0-9]{1,16}$`) and NaPTAN's national
uniqueness keep tram codes unique across networks, so the node needs no
network prefix. `normalize_tiploc` only trims, so long codes pass through
unchanged.

### 5.2 Names on the wire and in URLs

One NaPTAN stop area has two spellings: the stop-area code (`940GZZMAPIC`,
what GTFS `parent_station` and TfL use) and the access-node code
(`9400ZZMAPIC`, what Metrolink's pages and SIRI-SX use). The `940G` to
`9400` prefix rule is a bijection and is already tested against NaPTAN
(transit spec §6.3).

- **Accepted by `/Trips/plan`** as `origin` or `destination`: either
  spelling of a station of an enabled planner network, and a platform code
  (folded to its station). Matching is case-insensitive.
- **Echoed back** in each network's public spelling, the one its stop page
  uses:
  - Metrolink: `9400ZZMAPIC`, so `/transit/metrolink/stops/9400ZZMAPIC`;
  - London Trams: `940GZZCRWMB`, so `/tfl/stops/940GZZCRWMB`.

  The network file gains `public_stop_id = "access-node" | "stop-area"`.
- **URLs.**
  - The plan page uses `/plan?origin=9400ZZMAPIC&destination=MAN`.
    `lib/tripPlanUrl.ts` gains the two NaPTAN forms, as it already allows
    `tiploc:` codes and `group:` codes.
  - A transit stop page gains "Plan a journey from here", as station pages
    have.
- **Resolution order.** A CRS. Then a `tiploc:` code. Then a transit node
  of an enabled planner network. Then a TfL stop (`is_tfl_stop_id`) for the
  access model. So a London tram stop is timetabled when `london-trams` is
  enabled, and modelled as today when it is not (§12.2).
- **Errors** (400, before any graph is built, in the TfL stop ends spec's
  style): "'9400ZZXXXX' is not a stop the planner knows". The same station
  at both ends is refused, as for TfL stops.

### 5.3 What `transit_rail_links` is for here

The table links a tram station to a National Rail station (`crs`,
`tiploc`, `interchange` or `nearby`). The planner turns each row into walks
between the two nodes (§8.1). It is still never used to merge identities:
Piccadilly the tram station and MAN the rail station stay separate nodes
joined by a walk, because they have separate change times and separate
legs. The CORPUS pseudo-CRS rows for Metrolink stops (`AMM`, `AMO`, …)
stay excluded, as the transit spec §3.4 requires.

## 6. Building the tram connections

### 6.1 From patterns to connections

A new module, `crates/api/src/data/transit_planning.rs`, builds a date's
tram connections for each network in the switch (§14) whose active feed's
horizon covers the date:

1. **Read** the active version's patterns and pattern stops (137 and
   1,684 rows) and the day's trips (`transit_trips` joined to
   `transit_service_days` on the date: about 1,500). That is three small
   indexed reads, not 27k rows.
2. **Expand** each trip, in memory: for pattern stops *k* and *k+1*, one
   `Connection` from platform *k* to platform *k+1*:
   - **Times.** The departure is `start_s + dep_offset_s` and the arrival
     `start_s + arr_offset_s`, as absolute seconds from the service date's
     midnight.
   - **Rounding.** Times are rounded to the nearest minute, half up. Both
     ends of a connection use the same rule, so a stop with no dwell
     arrives and departs in the same minute and the trip stays
     non-decreasing. Departures are not floored and arrivals are not
     ceiled: that would make a no-dwell stop's arrival later than its
     departure, and a ride would look impossible.
   - `working_*` equals the public times (there is no working timetable).
   - `can_board` is `pickup != 1` and `can_alight` is `drop_off != 1`
     (GTFS: 1 means none; 2 and 3 are phone or arrange-with-driver, and are
     allowed).
3. **Append** to the rail connections before the one sort that
   `build_connections_with_passes` already does. A trip's minutes may
   exceed 1440, like a CIF day offset, because `start_s` may (the column
   allows up to 172,800), though the current feed has none.
4. **Day boundaries** follow rail. Date D's graph holds D's trips. With
   `acrossMidnight=true`, `trip_plan_overnight.rs` also plans D+1 from the
   same clock time and merges the two, which covers tram trips after
   midnight, since they are D+1 trips (§3.1).

### 6.2 Service identity (`uid`)

A tram connection's `uid` is `<network>:<n>`, where *n* is the trip's
index in the day's sorted trip list (`metrolink:1042`). It is unique
within a graph. It never matches a CIF UID (a letter and five digits), a
TRUST id or a Darwin RID, because it contains `:`. So the live overlay and
the in-search snapshot can never attach a train's report to it (§7.4). The
full GTFS `trip_id` (42 characters) is not used as the `uid`: it would add
a 64-byte allocation per connection (§10).

`PlanningGraph` gains a `transit: TransitTrips` side table indexed by *n*,
with the network, `trip_id`, line, headsign, pattern and the replacement-bus
flag. Leg dressing (§11) reads from it. It is a few hundred kB.

### 6.3 Replacement buses

Replacement-bus trips are in the timetable, and in the closure week of
§3.1 they were half of all trips. Under the transit spec's Q3 they show on
boards labelled as buses. The planner treats them the same way (proposed;
Q1):

- they are connections like any tram;
- their legs have `serviceMode: "replacementBus"`;
- they owe the bus side of a change (`ModalChangeBuffer`, 5 minutes).

### 6.4 The graph cache

`GraphCache` keys a date on `latest_schedule_publish`. The key becomes the
**later** of that marker and the newest `transit_networks.activated_at`
among the switch's networks. A new tram feed then invalidates the date at
once, as a CIF publish does, and the cache type does not change. A network
whose feed is missing or does not cover the date is left out and counted
(`api_trip_plan_transit_networks_skipped_total{network, reason}`). A rail
plan never fails because of tram data.

## 7. Scheduled, not live

The transit spec's honesty rules (its §2, §11 and C6) carry over to every
tram leg:

### 7.1 Leg fields

- `realtime: false` and `liveTracking: false` on every transit leg, with
  `serviceMode` `tram` or `replacementBus`.
- No `expected`, `delay`, `cancelled`, `status` or `live` field, ever. The
  live overlay (`trip_plan_live.rs`) skips transit legs. A transit leg is
  neither cancelled nor late in `liveFeasible`.
- The times are `scheduledDeparture` and `scheduledArrival` only. There are
  no `public*` or `working*` fields to be mistaken for a CIF timetable.

### 7.2 Frequency, not precision

The two feeds are scheduled-only (§3.1, §3.3). TfGM and TfL both describe
their trams to the public by frequency. So:

- **The planner searches on the scheduled trips.** Changes need real
  times, and the timetable is the best DS has.
- **The leg says how often trams run.** `headwayMinutes` is the median gap
  between departures of the same line and direction from the boarding
  platform in the 60 minutes around the leg's departure. It is computed
  from the graph at dressing time, and omitted when fewer than three
  departures fall in the window. The card reads "Tram (Blue Line)
  Piccadilly to Victoria, 08:14 to 08:22, about every 6 min".
- **The card carries the caption** "Scheduled times, not live" whenever an
  itinerary has a transit leg, the same words as the boards (transit spec
  §0).
- **A future feed with `frequencies.txt`.** With `exact_times=0` (headway
  only), the adapter expands the trips at the headway but flags them
  `frequency_based`. Their legs then show only "about every N min" and the
  ride time, with no minute times, and the search charges the full headway
  as the wait at boarding: the conservative choice. With `exact_times=1`
  they are ordinary scheduled trips. Neither feed needs this today, so it
  is specified here and not built.

### 7.3 Gaps and horizons are not facts about service

- **Beyond the horizon.** For a date past a network's horizon (60 days,
  transit spec Q8), or a date with no service day in its feed (§3.1), the
  plan is made without that network. The response says so:
  `transitNotices: [{network, reason: "noTimetable"}]`, shown as "Tram
  times aren't available for this date". It never says "no trams".
- **Disruption is shown, not inferred.** A current line-scoped situation on
  a leg's line, from `line_status` (Metrolink, `source = 'transit'`) or TfL
  (`tfl-tram`), adds a notice to the leg, such as "Disruption reported on
  the Blue Line", with the status text. The search does not drop or delay
  trams because of it. This follows the transit spec §10.2, "the rules
  never use the timetable", in the other direction. (Phase M3, §15.)

### 7.4 London Trams and TfL's live arrivals

TfL's arrivals for a tram stop are live (DS-136, `/tfl/stops/{id}`). A
London tram leg may link to that page as "Live arrivals" (Q8). The link
goes to TfL's data on its own page. The leg's own times stay scheduled and
labelled as such, and the planner never mixes the two.

## 8. Footpaths and interchange times

### 8.1 Rail and tram

Each `transit_rail_links` row of an enabled network becomes a `WALK` fixed
link both ways between the rail station's CRS and the tram station's node,
valid all day, every day. These are added in the graph build, not per
request. This is the shape `add_parent_walk_links` already uses for bus
stops.

- **Minutes.** The planner needs a number, though the transit spec's Q10
  decided not to show walking minutes on stop pages (Q2 here).
  - The figure is the straight-line distance between the tram station's
    NaPTAN coordinates (`transit_stops.lat`/`lon`) and the station's
    (`stations`), times `WALK_DETOUR` (1.25), at 75 m a minute. That is
    the `tfl_access` walk model, shared rather than copied.
  - It is clamped to at least 2 minutes for `interchange` and 4 for
    `nearby`, and at most 15.
  - The leg is a transfer with `access`-style wording: "Estimated walk
    between the tram stop and the station".
- **The station's change time still applies.** The planner adds the
  boarding member's minimum change time after any link. That is the MSN
  figure for a train and the platform's `change_minutes` for a tram, so the
  walk minutes are only the walk.
- **The ALF's MAN to MCV `TRANSFER` (22 min) stays.** It is the honest
  fallback when no tram runs (late at night, or a closure like §3.1's), and
  the search picks the tram when the tram is quicker.

### 8.2 Within a tram station

The network file gains `change_minutes` (proposed 3 for Metrolink and
London Trams), set for each platform in `change_time_by_tiploc`. The
existing sibling rule then charges 3 minutes for a change between
platforms and for a fresh boarding on the same platform. Staying aboard is
free. A trip's set-down-only and pick-up-only stops are handled by
`can_board` and `can_alight`.

### 8.3 Tram on one side of a change

`ModalChangeBuffer` today holds one set of bus and ferry UIDs and one
`minutes` figure. It becomes a map from service class to minutes:

- bus and ferry: `TRIP_PLAN_ROAD_WATER_CHANGE_MINUTES`, default 5, as now;
- tram: `TRIP_PLAN_TRANSIT_CHANGE_MINUTES`, proposed default 2 (Q6).

The tram figure is charged on the tram side of a tram-to-rail or
rail-to-tram change, never at the journey's origin or destination, and
mirrored in arrive-by, exactly as the bus buffer is. The reason is the
same: the tram's times are not observed. A change between two trams owes
none: the platform change time (§8.2) covers it. That is the one
departure from the bus rule, which charges each side whatever the other
side is. Replacement buses use the bus class.

### 8.4 Between tram stations

Walks between different tram stations (for example Market Street and
Piccadilly Gardens, a few hundred metres apart) are left out at first. The
tram network itself connects them. A later phase may add `WALK` links
between stations of one network within 400 m, with the same model, if
plans show a need.

### 8.5 TfL stops

A London tram station node and a TfL Tube or DLR stop at the same hub
(Wimbledon) are joined only through the rail station, by the rail-link
walk and the TfL access model at a Tube stop end. A Tube ride in the
middle of a journey stays what it is today: a curated or ALF fixed link
between rail stations.

## 9. Arrive-by and the other searches

The tram connections are in the one array every search reads, so:

- **Arrive-by** (`trip_planner::reverse`) needs no change: its backward
  sweep and `latest_departure` bisection treat a tram connection as any
  other. The rail-link walks are added both ways and the change buffers
  are mirrored (§8.3).
- **Frequent trams make arrive-by leave as late as the timetable allows.**
  That is correct on the timetable but optimistic without live data. The
  tram buffer (§8.3) covers the change. A tram as the last leg gets no
  buffer, and its "about every N min" text is the warning.
- **Waypoints, vias and the change cap.** A tram leg is a `TrainLeg` to
  the algorithms, so it counts towards `changeCount`. A rail-tram-rail
  journey is 2 changes, the default cap (Q5).
- **Ride merge** (`merge_transfer_rides`) is for transfer legs and leaves
  transit legs alone. Consecutive connections of one trip already merge
  into one leg by `uid`.
- **Same-operator alternatives** (`plan_for_journey`'s `allowed_uids`)
  ride trains only, as now.
- **The differential test** (CSA against RAPTOR) gains a tram-dense
  fixture: many trips, short hops, platform siblings and rail links.

## 10. Memory and latency budget

### 10.1 Memory

A `Connection` is three `String`s and four `u32`s, 96 bytes inline. Each
`String` is its own heap allocation, a 32-byte chunk under glibc for codes
of up to 24 bytes. So a connection costs about 192 bytes, and the rail
day's 457k is about 88 MB, which matches `GraphCache`'s "on the order of
100 MB".

| Added per cached date | Connections | Memory |
|---|---|---|
| Metrolink, full weekday | 27,614 | about 5.3 MB (6.2 MB if the 42-character `trip_id` were the `uid`) |
| London Trams, weekday | 17,585 | about 3.4 MB |
| `TransitTrips`, nodes and walks | about 2.5k trips, 400 nodes | under 1 MB |
| **Both networks** | 45,199 | **under 10 MB** |

`GraphCache` keeps 2 dates by default, so the api's RSS grows by **at most
20 MB**. **Budget: 25 MB for both networks.** The phase-M4 measurement
(§15) checks it on a copy of production.

### 10.2 Latency

Both searches sweep the whole array once per round, so their cost is
linear in connections. On the TfL stop ends spec's production copy, a CRS
plan took 8–47 ms (`fastest`) and 540–570 ms (`options`), release build,
graph cached. **Budget: at most +10% with both networks on**, that is
+5 ms `fastest` and +60 ms `options` at those medians. This is the 6% and
4% of §3 plus the dressing.

- **The graph build** gains three small reads and an in-memory expansion
  of about 45k connections. Budget: +100 ms on a cache miss, against the
  rail build's 490k-row read.
- **No per-request copy.** Tram stop ends are graph nodes, so unlike a TfL
  stop end they cost no `with_access` copy.
- **The `options` search-size guard** (`TRIP_PLAN_MAX_OPTIONS_SEARCH_SIZE`,
  default 252) bounds how many sweeps a request may make. Each sweep gets
  6–10% longer, so the worst case grows by the same share. Phase M4
  rechecks the default against the measured worst case.

### 10.3 What would break the budget

The Underground (226k connections a weekday, +50%) would. That is one
reason the Tube timetable is a separate decision (§13.3). Before it, the
`Connection` type should move to interned ids: `u32` stop and service
indices instead of `String`s, which cuts the rail graph too. That is not
part of this work.

## 11. API and frontend

### 11.1 The leg

A new leg kind. A tram is not a CIF train: it has no `trainUid` to track,
headcode, operator code or working times. Calling it `kind: "train"`, as
buses and ferries are, would make the frontend's saving code POST it as a
tracked train, which would fail. Calling it a `transfer` would lose its
departure time.

```json
{
  "kind": "transit",
  "network": "metrolink",
  "serviceMode": "tram",
  "liveTracking": false,
  "realtime": false,
  "serviceDate": "2026-11-04",
  "scheduledDeparture": "08:14:00",
  "scheduledArrival": "08:22:00",
  "departureDayOffset": 0,
  "arrivalDayOffset": 0,
  "line": {"id": "metrolink:blue", "name": "Blue Line", "colour": "#1f6fb5"},
  "headsign": "Bury",
  "originCrs": null,
  "destinationCrs": null,
  "originStop": {"id": "9400ZZMAPIC", "name": "Piccadilly", "platform": "To Bury"},
  "destinationStop": {"id": "9400ZZMAVIC", "name": "Victoria", "platform": "To Bury"},
  "headwayMinutes": 6,
  "notices": []
}
```

- **London Trams.**
  - `originStop` and `destinationStop` are the TfL stop summary
    (`{id, name, modes, lines}`, `tfl_stops::TflStopSummary`), so
    `tflStopPlaceName` reads "Wimbledon (Tram)".
  - `line` is TfL's tram line from `tfl_stops.lines`, so it links to the
    TfL line page and its `tfl-tram` status.
  - The platform is the NaPTAN indicator, where there is one.
- **A replacement bus** has `serviceMode: "replacementBus"` and `line`
  `null`, and keeps its headsign.
- **Response.** It gains `transitNotices` (§7.3) and, on each itinerary,
  `scheduledOnly: true` when any leg is transit. The frontend shows the
  caption from that flag.
- **Opt-in.** Transit legs appear only when the request says
  `transit=true`, or names a transit node as an end. That holds even with
  the switch on. Clients that switch on `kind`, which today assume
  `"train" | "transfer"` (DS-MCP and older frontends), see no change until
  they opt in.
- **`docs/api-changelog.md`** gets the leg, the opt-in, the new end forms
  and the `realtime: false` contract. The frontend's `TripPlanLeg` type
  (`lib/types.ts`) gains the variant.

### 11.2 Saving a plan with a tram leg

`itineraryLegsForSaving` (`frontend/lib/tripPlanSave.ts`) turns a transit
leg into an **untracked leg**, as it does a Tube transfer:

- `transitMode: "tram"` (already a `TransitMode`), or `"bus"` for a
  replacement bus;
- `lineName` from the line;
- `originStopId` and `destinationStopId` from the stop ids;
- `plannedDeparture` and `plannedArrival` from the leg's own scheduled
  times. A transfer has to borrow its times from the trains either side;
  a transit leg has its own.

Its state is then the clock-only untracked state the api already computes.

- **London tram stop ids already pass.** `journeys::attach_untracked_stops`
  accepts only `tfl_stops` ids (`common::tfl_stops::is_stop_id`, then a
  lookup).
- **Metrolink stop ids need a change.** The same function also accepts an
  enabled network's station from `transit_stops` (its public spelling), and
  an untracked leg's stop links to `/transit/{network}/stops/{id}`. Its
  error text ("must be a TfL stop id") widens to match.

A plan with no train at all, such as tram only from Piccadilly to
Altrincham, still cannot be selected today ("This route needs no train, so
there is nothing to track"). Whether it should become an all-untracked
journey is Q11.

### 11.3 Frontend display

- **The leg row** (`ItineraryOption.tsx`'s `legSummary`):
  - Text: "08:14 Tram (Blue Line) Piccadilly to Victoria 08:22", and below
    it, dimmed, "About every 6 min".
  - The line name carries its colour, as the transit pages show it
    (`TransitLineList`), shared rather than copied.
  - A `ServiceModeBadge` shows "Tram" or "Replacement bus".
- **Under the legs:** "Scheduled times, not live", on any itinerary with
  `scheduledOnly`.
- **Notices:**
  - a disruption notice in the tram line's status colour (phase M3);
  - for London, a "Live arrivals" link to `/tfl/stops/{id}` (Q8).
- **The rail-link walk** shows as today's transfer row, with its estimate
  note (§8.1).
- **From and To** (`lib/plannerEnds.ts`): Metrolink stations are added
  after TfL stops as "Piccadilly (Metrolink)". They come from
  `/public/transit/{network}/stops` for networks that
  `/public/transit/networks` marks `tripPlanning: true`. London tram stops
  are already offered as TfL stops (`Wimbledon (Tram)`) and need no
  change.
- **Copy** passes `scripts/check-copy.py`: sentence case; no em dashes,
  ellipses or arrows ("to", not an arrow); a tram is a "tram" and a stop a
  "stop".
- **Screens.** New visual baselines for an itinerary with a tram leg and
  with a replacement bus, at 390 and 1366 px, light and dark.

## 12. London Trams

### 12.1 The shared pipeline

**Yes: London Trams can use exactly the Metrolink path.** The BODS London
GTFS is keyless and OGL (§3.3), and its tram stops have the parent and
platform shape the GTFS adapter maps. It would be a network `london-trams`:

- **Network file** `networks/london-trams.toml`:
  - `nocs = ["TRAM"]`, `atco_prefix = "9400ZZCR"`, `naptan_area = "940"`;
  - `gtfsUrl` the London file;
  - one line, `id = "tram"`, from `gtfs_route_names = ["Tram"]`. Both
    routes share the name, and TfL reports the network as one line,
    `tfl-tram`.
- **Planner only.** A new network-file flag, `public = false`. It means:
  - no `/transit/london-trams` pages: the api refuses the network in
    `API_TRANSIT_NETWORKS`, and the chart fails the render;
  - no situations loop: `situationsEnabled` is refused, so there is no
    second London Trams line status beside TfL's `tfl-tram`;
  - no pins and no notifications, because there are no transit line
    status rows;
  - `public_stop_id = "stop-area"`, so ends and legs use the TfL `940G`
    ids and link to the TfL stop pages.
- **Rail links.** A checked-in `reference-data/transit/london-trams-rail-links.csv`
  with the 7 `tfl_stop_crs` tram rows (§3.3) as `interchange`, checked by
  the existing unit test against `crs-tiploc.csv` and the NaPTAN fixture.
  A DB test (`#[ignore]`) compares the CSV with `tfl_stop_crs`, so drift is
  caught in CI rather than in a plan.
- **The rest** follows the runbook's "Adding a network" (stream, budget,
  ACL user, values, grants, attribution). The OGL attribution already in
  the footer covers it. `/attribution` gains a London Trams line.
- **The producer's limits** need checking against the bigger file:
  - **Download.** 321 MB is under `gtfsMaxBytes` (400 MB) but close.
    Raise it to 600 MB for this network.
  - **Disk.** `workDirSizeLimit` 512Mi holds the zip, because entries are
    streamed and never unzipped to disk.
  - **Parse.** The 1.58 GB `stop_times.txt` streams through the trip
    filter in about 3 minutes (a stdlib Python pass took 2 min 51 s; the
    Rust adapter should be quicker). `progressStallSecs` (3600) covers it.

### 12.2 Alongside the TfL stop identity and status

- **One identity.** `tfl_stops` (`940GZZCRWMB`) stays the public identity
  of a London tram stop: its page, search entry, arrivals and lines. The
  transit tables hold the timetable under the `9400` spelling, which no
  user sees. `tfl_stop_crs` and `transit_rail_links` agree by the DB test
  above.
- **One status.** TfL's `tfl-tram` row in `line_status` is the only London
  Trams status. A tram leg's disruption notice (§7.3) reads it.
- **The access model stays as the fallback.** With `london-trams` not in
  the planner switch, or without `transit=true`, a `940GZZCR…` end is
  planned exactly as today, through `tfl_access` (§5.2's resolution
  order). With it, the end is a graph node and `tfl_access` is not called
  for that end. `tfl_access.rs` itself does not change: the branch is in
  `routes/trips.rs`, before the TfL stop lookup.

### 12.3 Dependency on the Old Street fix and the branch penalty

Two changes to `tfl_access.rs` are in progress (DS-151, the Tube
branch-change penalty, and the Old Street hub fix). The Old Street fix
makes line distances follow TfL route sequences and lets hub stops keep
line candidates.

- **Code conflict.** None planned: §12.2's branch is in `routes/trips.rs`.
  The London Trams phases (LT2) start after both changes are merged and
  rebase on them.
- **Behaviour.**
  - While `london-trams` is off, the Old Street fix improves tram access
    legs. Route sequences replace the spanning tree on the branching tram
    network, and Wimbledon and East Croydon, both hubs, now also get
    tram-ride candidates.
  - Once `london-trams` is on, tram stop ends bypass the model, so neither
    fix affects them. Both still matter for Tube and DLR ends.
- **Reuse.** The route sequences the fix fetches are the input to the
  stopgap of §12.4.

### 12.4 What carries over even without a timetable

These parts of the design help a tram network with no usable timetable,
whether London Trams before LT1 lands or any future network:

1. **Common stop identity** (§5). The node, sibling and change-time scheme
   needs only stops and coordinates. NaPTAN supplies both.
2. **Rail-link footpaths** (§8.1). They come from coordinates and a links
   table, with no timetable.
3. **Mid-journey tram legs from modelled links** (the stopgap). Fixed links
   between the rail hubs a tram line serves. For London Trams: WIM, MIJ,
   ECR, WCY, BKJ, BIK and ELE, pairwise along each route sequence. Their
   minutes use the `tfl_access` ride model: entry and wait, distance along
   the route sequence at 18 km/h, and exit. They are added in the graph
   build as `METRO` links, valid in `SERVICE_WINDOWS`.
   - Gain: today's curated Tube links between termini work this way, so a
     journey can ride rail to Wimbledon, tram, then rail from East Croydon
     with no tram timetable.
   - Cost: 1–2 days (LT0, optional).
   - What it cannot do: change tram lines, give times or reach stops that
     are not hubs.
   - Retirement: it is dropped when LT1 lands, and the switch never runs
     both.
4. **The honesty rules** (§7). An estimated link says "estimated", as
   access legs do now.

## 13. Other networks: NET, DLR and after

### 13.1 Nottingham NET

NET is being built now as a TransXChange adapter (DS-160) into the same
`transit_*` model (transit spec §6.2). The planner reads the model, not
the adapter, so NET needs:

- `change_minutes` and `public_stop_id` in its network file;
- `reference-data/transit/net-rail-links.csv`, which DS-160 writes anyway
  for its stop pages. Nottingham (NOT), Bulwell (BLW) and Hucknall (HKN)
  are the obvious rows; the rest are checked from NaPTAN when the file is
  written, as Metrolink's were;
- `api.tripPlanTransitNetworks: [metrolink, net]`.

About a day after DS-160 and phase M2 land, plus a soak. A TXC feed may
carry frequency-based journeys, which is the one place §7.2's
`frequency_based` rule could be needed. DS-160 should flag them in the
model even if the planner work comes later.

### 13.2 DLR

The DLR could follow London Trams exactly: NOC `LDLR`, 18,990 connections
a weekday (4.2%), 91 platforms and `940GZZDL…` ids in `tfl_stops`. It
would be planner-only and use TfL's `tfl-dlr` status. Two differences:

- **More hubs are shared with Tube stops** (Bank, Canning Town, Stratford).
  The TfL access model keeps handling Tube ends, so a DLR station node and
  a Tube stop at the same place meet only through a rail station, as in
  §8.5. Plans that would change between DLR and Tube without a rail
  station (for example at Bank) are not found. That is acceptable until
  the Tube is timetabled.
- **The London GTFS is one download.** Today's producer runs one process
  per network, so London Trams and the DLR would download the 321 MB file
  twice a day. A producer serving several networks from one feed (one
  download, one filtered pass, one stream per network) is worth building
  at the second London network. About 2 days.

Estimate: 4–6 days after LT, with the shared-download producer.

### 13.3 The Underground (not proposed)

The same feed has the Tube timetable (226,430 connections a weekday). It
would replace the `tfl_access` ride model and the curated Tube links with
real times and real changes between lines. But it adds half again to the
graph (§10.3). It needs the interned `Connection` first, and a decision on
how far DS goes beyond National Rail. It is listed for the user (Q7), not
designed here.

### 13.4 The generic rule

A network enters the planner when:

1. it has an active `transit_*` feed;
2. its network file has `change_minutes` and `public_stop_id`;
3. it has a rail-links file;
4. it is in `api.tripPlanTransitNetworks`.

Nothing in the planner names a network.

## 14. Rollout

Off by default, in the transit spec's style: one list, then a soak.

| Value | Default | What it does |
|---|---|---|
| `api.tripPlanTransitNetworks` (`API_TRIP_PLAN_TRANSIT_NETWORKS`) | `[]` | The networks whose trams enter the planner's graph. Empty: the graph, the cache key and every response are as today. The chart refuses an id not in `common::transit::NETWORKS`. |
| `api.tripPlanTransitChangeMinutes` (`TRIP_PLAN_TRANSIT_CHANGE_MINUTES`) | `2` | §8.3, clamped `0..=15`. |

The request opt-in (`transit=true`, §11.1) is not a deploy value: the
frontend sends it once phase M3 ships, and DS-MCP once it has handled the
new leg kind.

### 14.1 Metrolink

1. Merge M1 and M2 (dark). The list is empty, so nothing changes. A golden
   test pins that.
2. In Ranma-Config, set `api.tripPlanTransitNetworks: [metrolink]`. The
   api restarts. Still no visible change, because no client opts in yet.
3. Check, read-only:
   - `api_trip_plan_graph_cache_total{result="miss"}` once per date;
   - `api_trip_plan_transit_connections{network}` (a gauge set at build)
     at about 27k on a weekday;
   - the api's RSS within §10's budget.

   Then make one request by hand with `transit=true` (rail to Piccadilly,
   tram to Victoria).
4. Merge M3 (the frontend sends `transit=true`). After a day of use with
   no errors, Metrolink trams are in public plans.
5. **Rollback:** empty the list. The api restarts with the old graph. Saved
   untracked tram legs stay valid, because they are plain untracked legs.

### 14.2 London Trams

1. Merge LT1 (the network file, stream, ACL user and values, all off).
2. Roll out the producer as the runbook's "Adding a network" says, with
   `situationsEnabled` refused and the network **not** in
   `api.transitNetworks`. Check the feed's counts against §3.3: about 39
   stations, 66 platforms and 1,000 trips a weekday.
3. Merge LT2 (TfL identity resolution, the bypass of the access model).
4. Set `api.tripPlanTransitNetworks: [metrolink, london-trams]`. Check as
   in 14.1, plus a plan from `940GZZCRWMB` that now leaves on a timed tram.
5. **Rollback:** remove `london-trams` from the list. Tram stop ends fall
   back to the access model.

## 15. Tests, phases and effort

### 15.1 Tests

- **`trip-planner` (unit).**
  - Rail, walk, tram, walk, rail mid-journey.
  - A platform change charged at `change_minutes`.
  - Set-down-only and pick-up-only stops.
  - The tram change buffer on each side, mirrored in arrive-by.
  - The CSA and RAPTOR differential test with a tram-dense fixture
    (§9).
  - Arrive-by against the forward search on the same fixture.
- **`api` (unit).**
  - The expansion: rounding, the no-dwell rule, `pickup` and `drop_off`
    codes, `start_s` past midnight, service days, and replacement buses
    tagged.
  - `uid` uniqueness, and no match with the CIF UID, TRUST or RID
    grammars.
  - The rail-link walk minutes and clamps.
  - End parsing: both spellings, platform folding, the resolution order and
    the errors.
  - Leg dressing: `realtime: false`, no `live`, `headwayMinutes` with its
    three-departure floor, and notices.
  - The live overlay ignores transit legs.
  - **Golden:** with the list empty or without `transit=true`, responses
    are byte-identical to today's.
  - The cache key moves on a transit activation.
- **`api` (DB, `#[ignore]`, as the api role).**
  - The graph build over a migrated database seeded with a small Metrolink
    feed version: the api role already has SELECT on `transit_*`.
  - The London Trams rail-links CSV against `tfl_stop_crs`.
- **`poller-transit-schedule`.**
  - `london-trams.toml` parses.
  - `public = false` refuses situations.
  - The rail-links CSV checks.
  - A small London-shaped GTFS fixture: two routes named "Tram", parent
    stations in `940G` form.
- **Chart.**
  - Render checks for the list's ids.
  - A planner-only network refused in `api.transitNetworks`.
  - `chart-values-doc.py check`.
- **Frontend (vitest).**
  - `legSummary` and the caption.
  - `itineraryLegsForSaving` with a transit leg (its own times, `tram`
    mode and stop ids).
  - `tripPlanUrl` with both NaPTAN forms.
  - `plannerEnds` offering Metrolink stations.
- **E2E and visual.** `seed.sql` gains a tiny Metrolink feed version (CI
  already runs with `API_TRANSIT_NETWORKS`). `e2e/plan.spec.ts` plans rail
  to tram to rail and saves it. Baselines as §11.3.
- **Measurement (not committed data).** A release-build run on a copy of
  production for one weekday, as the TfL stop ends spec's §6 did, records
  the graph size, RSS and the latency medians against §10's budgets. Only
  the figures go in the PR.

### 15.2 Phases and effort (engineer-days)

| Phase | What ships | Effort |
|---|---|---|
| **M1** planner core | `transit_planning.rs` (expansion, `uid`s, `TransitTrips`), nodes and platform change times, rail-link walks, the per-class change buffer, cache key, switch, metrics, trip-planner tests | 4–5 |
| **M2** API | end resolution and errors, the `transit` leg and its dressing, headway, overlay and leg-detail exclusions, `transitNotices`, opt-in, transit stop ids on untracked legs, api-changelog, golden tests | 3–4 |
| **M3** frontend | From and To, URLs, leg row and caption, saving as untracked, line-status notices, e2e and visual baselines | 3–4 |
| **M4** measure and roll out | production-copy measurement, runbook section in `docs/transit-networks.md`, deploy notes | 1–2 |
| | **Metrolink total** | **11–15** |
| LT0 (optional stopgap) | modelled hub-to-hub tram links from route sequences (§12.4), after the Old Street fix | 1–2 |
| **LT1** network | `london-trams.toml`, `public = false`, rail-links CSV and DB test, stream, budget, ACL, values, attribution, producer limits | 3–4 |
| **LT2** TfL identity | `940G` resolution ahead of `tfl_access`, TfL stop summaries on legs, `tfl-tram` notices, "Live arrivals" link (Q8), tests | 2–3 |
| **LT3** roll out | §14.2 | 1 |
| | **London Trams total** | **6–8** (7–10 with LT0) |
| NET | §13.1, after DS-160 and M2 | about 1 |
| DLR (later) | §13.2, with the shared-download producer | 4–6 |

Metrolink and London Trams together: **17–23 days**, plus a soak for each
rollout. M1 and M2 can be reviewed as one PR. M3 is a second PR. LT1 can
run in parallel with M3, because it touches only the producer and the
chart.

### 15.3 Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A tram leg read as live | medium | misleading plans | `realtime: false`, no `live`, the caption, "about every N min" (§7) |
| A closure week (§3.1) or a feed gap hides trams | medium | plans fall back to rail and ALF links | `transitNotices`; the ALF MAN to MCV link stays; never "no trams" |
| Old clients meet `kind: "transit"` | low | a broken leg list | opt-in `transit=true` (§11.1) |
| The graph grows past the budget | low | slower plans | §10 budget, M4 measurement, the per-network switch |
| The London zip grows past the producer's caps | low–medium | London Trams timetable refused, access model keeps working | `gtfsMaxBytes` 600 MB for the network; `transit_feed_refused_total{reason="oversize"}` alert |
| `tfl_stop_crs` and the London rail-links CSV drift | low | a missing walk | the DB test (§12.1) |
| A tram buffer that is too small or too large | medium | missed or over-cautious changes | `TRIP_PLAN_TRANSIT_CHANGE_MINUTES` is a value, not a constant (Q6) |

## 16. Open questions for the user

**Decided 2026-10-24.** The user accepted every proposed default. The
decisions below are binding on the M and LT phases.

| # | Question | Decision |
|---|---|---|
| Q1 | Should replacement-bus trips from a tram timetable be planned (labelled "Replacement bus", with the bus change buffer)? In the closure week of §3.1 they were half of Metrolink's trips. | **Yes.** Planned from the tram timetable, labelled "Replacement bus" (`serviceMode: "replacementBus"`), with the bus change buffer. |
| Q2 | The planner needs walk minutes for rail-to-tram changes, worked out from coordinates and shown as estimates on the walk leg. Stop pages stay without them (transit spec Q10). Is that acceptable? | **Yes, as estimates.** The walk leg says the minutes are estimated. Stop pages stay without them. |
| Q3 | A new `kind: "transit"` leg, opt-in by `transit=true`, or tram legs as `kind: "train"` with `serviceMode: "tram"`, as buses and ferries are (fewer client changes, but saving and tracking would treat them as trains)? | **`kind: "transit"`**, opt-in by `transit=true`. |
| Q4 | Tram ends in URLs as bare NaPTAN codes (`9400ZZMAPIC`, `940GZZCRWMB`) or with a network prefix (`metrolink:9400ZZMAPIC`)? | **Bare NaPTAN codes.** |
| Q5 | A tram leg counts as a change, so rail-tram-rail is 2 changes, the default cap. Keep that, or raise the default cap to 3 when the plan has transit legs? | **Keep the cap.** A tram leg counts as a change; `maxChanges` raises it. |
| Q6 | The extra change time on the tram side of a tram-to-rail change. | **2 minutes** (`TRIP_PLAN_TRANSIT_CHANGE_MINUTES`). |
| Q7 | London Trams from the BODS London GTFS, as a planner-only network (321 MB download a day)? And later: the DLR the same way, and in the long run the Underground timetable from the same feed to replace the modelled Tube legs? | **London Trams next, then the DLR.** The Underground timetable is its own later decision. |
| Q8 | On a London tram leg, link to the TfL stop page's live arrivals ("Live arrivals"), clearly apart from the leg's scheduled times? | **Yes**, in phase LT2. |
| Q9 | Tram stations as waypoints, vias and avoids. | **Not now.** |
| Q10 | Past a network's 60-day horizon, plans have no trams and say "Tram times aren't available for this date". Keep the horizon, or store more for planning? | **Keep the 60-day horizon.** |
| Q11 | Should a tram-only plan (Piccadilly to Altrincham) be selectable and saved as a journey of untracked legs? Today a plan with no train cannot be picked. | **No, for now.** Tram-only plans are neither selectable nor saveable. |
| Q12 | Build the LT0 stopgap (modelled tram links between rail hubs, §12.4) before the London Trams timetable, or go straight to LT1? | **Skip LT0**; go straight to LT1. |

## 17. Implementation notes (DS-164)

Phases M1 to M3 for Metrolink, 2026-10-24. Where the build departs from
the text above:

- **Planner settings are compiled into the api**
  (`transit_planning::PLANNER_NETWORKS`: `change_minutes` 3,
  `public_stop_id`), not added to the producer's network file. The api
  never reads `crates/poller-transit-schedule/networks/*.toml`, and moving
  them through the stream would have needed a schema change. §13.4's rule
  "its network file has `change_minutes` and `public_stop_id`" reads
  "`PLANNER_NETWORKS` has an entry".
- **The tram-side change buffer rides on the rail-link walk** (§8.3).
  `ModalChangeBuffer` is unchanged: the walk's minutes are the estimated
  walk plus `TRIP_PLAN_TRANSIT_CHANGE_MINUTES`. Every rail-tram change
  crosses exactly one such walk and a tram-tram change none, so the rule is
  the spec's with no change to any search, and arrive-by mirrors it. Two
  consequences: like the bus buffer, a walk from a rail origin onto the
  tram (or off it to a rail destination) carries it too; and a replacement
  bus to rail change owes the bus buffer plus the tram one. The leg's
  `transitWalk.walkMinutes` is the walk alone.
- **Off-switch byte identity with the switch on.** The cached graph holds
  the tram connections whenever the switch lists a network; a request that
  does not opt in searches the graph's rail-only interchange data, where no
  walk reaches a tram platform, so no tram can be boarded. The opted-in
  request uses a second interchange table (a copy with the tram nodes and
  walks) built once per cached date.
- **The leg** is carried on `PlannedLeg::Train` (`transit`) and served as
  `kind: "transit"` by `PlannedLeg`'s `Serialize`, so every consumer that
  walks legs counts it as a ride (Q5) without changes to modules other
  work is changing (`tfl_access.rs`).
- **A tram stop end on a date without a timetable** is a 400 ("Tram times
  aren't available for this date, so the plan can't start or end at a tram
  stop."); a station-to-station plan on such a date carries
  `transitNotices`.
- **Measured on hand-made data** (`crates/api/tests/transit_planning_budget.rs`,
  release, 2026-10-26; a 460,041-connection rail day and a 28,579-connection
  Metrolink-shaped weekday):
  - **Latency, within budget.** `fastest` (CSA) +0.2% for a request that
    does not opt in and -3.8% with `transit=true`; `options` (RAPTOR, 4
    rounds) +2.0% and +4.0%.
  - **Memory, over budget on this data.** +14.3 MB per cached date, of
    which 5.6 MB is the transit interchange table (the copy of the rail
    one) and 8.7 MB the trams' connections, trips, nodes and walks: 28.6 MB
    for the two cached dates against 25 MB. The synthetic interchange table
    (12,000 TIPLOCs, 15,000 links) is likely larger than production's, and
    the phase M4 run on a production copy decides. If it is over there
    too, the options are: share the interchange copies between the cached
    dates, the interned `Connection` of §10.3, or one fewer cached date.
- **Rail-link walks with no coordinates** take 5 minutes (`interchange`)
  or 10 (`nearby`).
- **No OpenAPI document** exists in the repo; the contract is in
  `docs/api-changelog.md` (2026-10-24) and the route's doc comment.
- **Deferred:** sharing the walk model's haversine with `tfl_access.rs`
  (left untouched while DS-151 and the Old Street fix change it); the
  London Trams phases; the measurement on a production copy (M4, a
  runbook step).
