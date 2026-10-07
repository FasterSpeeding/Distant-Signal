# TIPLOC locations: naming bus stops and other non-station TIPLOCs, and bus-stop planner end points

Date: 2026-10-06. Status: implemented.

## Problem

Over a week of `schedule_calling_points_full`, about 990 distinct TIPLOCs are
called at by bus or ship services. Most resolve to a station, but 132 TIPLOCs
with a public call (22,000+ calls) had no `tiploc_crs` row: bus stops such as
`HTRBUS3` "HEATHROW TERMINAL 3 BUS", `SANWBUS` "ST ANDREWS BUS STATION" and
`KESWICK`, ferry terminals such as `BDICK` (Brodick), and a few timing points.
`resolve_tiploc_crs` drops every `TI` record with STANOX `00000`, so these had
no name: the train page showed "Stop N" or "Unknown station", the working
timetable the raw TIPLOC, station boards a `~HTRBUS3` destination key, and the
planner could not start or end a journey there.

## Product: `tiploc_locations`

`schedule-reference` publishes one row per CIF `TI` record (about 12,100), to
`POST /private/tiploc-locations` (schedule-reference's writer credential),
which replaces the table in one transaction (migration
`20261007100000_tiploc_locations.sql`). It is deliberately separate from
`tiploc_crs`, which feeds the planner's interchange data and the CRS
crosswalks: a bus stop must never become a station there.

Columns: the `TI` name, CRS and STANOX; the MSN `A` record's name, 3-letter
code, grid reference (metres, OSGB36, from MSN's 100 m `1EEEE`/`6NNNN`
fields; placeholders such as `19500E69999` are dropped) and interchange
status; the derived `location_type`, `name` and `display_name`;
`parent_crs`/`parent_source`/`parent_distance_m`; and per-TIPLOC counts of
rail calls, rail passes, bus calls and ship calls over every schedule in the
delivery.

### Location types

`station`, `bus_stop`, `ferry_terminal`, `junction`, `siding`,
`passing_point`, `other`. First rule that applies
(`crates/schedule-reference/src/locations.rs`, `classify`):

1. A STANOX and a bookable (not `X`-prefixed) CRS of its own -- the TIPLOCs
   `tiploc_crs` holds: **station**, whatever calls there (a station whose
   trains are replaced by buses for a while stays a station).
2. No rail service calls or passes, but bus or ship services do (CIF train
   status `B`/`5` bus, `S`/`4` ship, tallied by a streamed pass over the
   `MCA`): **ferry_terminal** when ships call at least as often as buses or
   the name says ferry, else **bus_stop**.
3. No rail service at all: the name (`TI` description, then the MSN name):
   bus words (`BUS`, `COACH`...) and ferry words (`FERRY`, `PIER`, `QUAY`,
   `HARBOUR`, `PORT`, `SLIP`, `LANDING STAGE`...), then the rail words below.
4. Rail services call or pass: junction words (`JN`, `JCN`, `JUNCTION`),
   siding words (`SIDINGS`, `SDGS`, `CS`, `DEPOT`, `TMD`, `YARD`, `GOODS`,
   `FREIGHT`...), passing-point words (`SIGNAL`, `LOOP`, `LP`, `SB`, `GF`,
   `XOVER`, `TUNNEL`...). Whole words only (`BUSHEY` is not a bus stop).
5. Rail services only ever pass it: **passing_point**.
6. Otherwise **other**.

The name rules live in `common::location_naming` so `api`'s CORPUS fallback
(a TIPLOC with no row) classifies the same way.

### Display names

Title case with fixed rules (`common::location_naming::title_case`): numbers
and dotted abbreviations kept (`A62`, `I.O.W.`, `P.H.`), `ST`/`ST.` -> `St`,
`RD` -> `Rd`, `JN` -> `Jn`, small words lower case inside a name
(`Stow-on-the-Wold`, `Isle of Man`), short vowel-less words kept as acronyms
(`TMD`, `CS`, `D R`), `CO-OP` -> `Co-op`, `O'Brien`, `McDonald`. A bracket the
26-character `TI` field cut off is closed (`EAST MIDLANDS AIRPORT (BUS`).

A bus stop's trailing marker becomes a suffix: `HEATHROW TERMINAL 3 BUS` ->
`Heathrow Terminal 3 (bus stop)`, `KESWICK (BUS STATION)` -> `Keswick (bus
station)`; a ferry terminal gets `(ferry terminal)`; everything else is the
title-cased name (`Marylebone 10 Signal`). `name` is the place name without
the suffix.

### Parent stations

Only bus stops and ferry terminals get one; first rule that finds one:

1. **Same TIPLOC**: the stop's own `TI`/MSN code is a station's CRS (MSN files
   a station's subsidiary bus stops under the station's code, e.g. `BANSBUS`
   under Banstead `BAD`).
2. **Nearest**: the station whose MSN grid reference is closest, within
   400 m (four 100 m grid squares).
3. **Curated**: `reference-data/tiploc-parent-stations.csv`, seeded with four
   checked cases: `HTRBUS3` -> `HXX` (412 m), `CRDFAIR` -> `RIA`, `IVRNABS` ->
   `IVA`, `PNZQUAY` -> `PNZ`.

Readers join `stations` and serve a parent only when that CRS has a row.

## API

- `JourneyStop` gains `locationType` and `parentCrs`; `name` is filled from
  `tiploc_locations` (CORPUS fallback) for every stop the station-name pass
  left unnamed, including a station CRS with no `stations` row.
- A destination with no CRS is keyed `tiploc:TIPLOC` (was `~TIPLOC`, still
  read until the next publish replaces the rows), and named from
  `tiploc_locations` wherever `station_names_for_crs_batch` names
  destinations (boards, train search).
- `GET /public/stations?q=...&stops=true` appends up to 10 bus stops and ferry
  terminals that some bus or ship serves: `code` `tiploc:KESWICK`, `name`
  `Keswick (bus)` / `Brodick (ferry)`, `kind`, `parentCrs`. Off by default.
- `GET /Trips/plan` accepts `tiploc:` codes (any case) as origin, destination,
  waypoint and in the avoid lists.

### Identifier scheme: `tiploc:TIPLOC`

A bus stop's MSN code (`SAO`, `KWK`) looks like a CRS and lives in the same
namespace: some are another place's CRS, several stops share one (`HOLSCHR`
and `HOLSLIB` are both `XEE`; `HTRBUS2` shares `HWA` with `HTRWTM2`), and 19
of the stops have none. The TIPLOC is unique, already the schedule's own key
(no mapping table to drift), and stable across deliveries. A `tiploc:`
prefix (lower case, with a colon) can never be a CRS, and it does not encode
the mode, so a stop reclassified bus <-> ferry keeps its code. The same key
replaces the old `~TIPLOC` destination key, so one string means one place
everywhere.

### Station pages and boards

A bus stop gets no page of its own. The train page links its name to the
parent station when there is one, and shows plain text otherwise; the
planner shows the name only. Every station picker other than the planner's
keeps calling the search without `stops=true`, so a `tiploc:` code never
reaches a station page, board or tracked leg (the planner never sends one as
a leg's CRS override).

## Planner

`fetch_interchange_data` adds every bus stop and ferry terminal whose TIPLOC
has no station CRS as its own end point (`crs_to_tiplocs["tiploc:SANWBUS"] =
["SANWBUS"]`). Changes there use the stop's own change time (in practice
the default 5: see "Walking links to the parent station" below), and since
2026-10-07 a stop with a parent station is linked to it by a walk.

**Change buffer** (`schedule_query::ModalChangeBuffer`): a change costs the
station's minimum change time plus `TRIP_PLAN_ROAD_WATER_CHANGE_MINUTES`
(chart `api.tripPlanRoadWaterChangeMinutes`, default 5, clamped 0-30) for
each bus or ferry side: alighting from one to change, and boarding one at a
change. Bus-to-train is +5, bus-to-bus +10, never at the origin or
destination. CSA, RAPTOR and the staged search apply it; the arrive-by
backward scan mirrors it. The bus/ferry services are the UIDs whose
`schedule_services` mode (per service date) is `replacement_bus`, `bus` or
`ferry`, so a rail-replacement bus calling only at station TIPLOCs is caught
too. A UID with no `schedule_services` row (that date not yet published, or
the table unreadable) falls back to the original heuristic: it is a bus or
ferry when it calls at a bus stop or ferry terminal
(`trip_planning::modal_change_buffer`, merged 2026-10-06).

### Walking links to the parent station (decided 2026-10-07)

Until now a bus stop was an end point only: you could start or finish a
journey there, but not change between it and its parent station. (In
practice no bus stop has a `tiploc_crs`/`stanox_crs` change time, because
its STANOX is `00000`. So it got the default 5 minutes, not a 98/99
sentinel. What actually stopped a change was the missing link to the
station.)

**Decision:** every bus stop and ferry terminal that is its own `tiploc:`
end point, and whose `parent_crs` has a `stations` row the planner knows,
gets a `WALK` link each way to that station.

- **Where the links come from.** `trip_planning::add_parent_walk_links`
  builds them in memory in `fetch_interchange_data`, from `tiploc_locations`
  joined to `stations` (`tiploc_locations::parent_links`). `fixed_links` and
  its ALF ingest are untouched, and there is no migration. The links follow
  each publish through the existing graph cache. A stop whose TIPLOC
  already belongs to a station (it is one of the station's sibling TIPLOCs)
  is skipped.
- **Walking time** (`common::tiploc_parents::walk_minutes`). If the curated
  CSV gives `walk_minutes` for this stop and this parent, that wins.
  Otherwise it comes from the MSN grid distance: `ceil(m / 80) + 2`,
  clamped to 3-15 minutes, at 80 m a minute plus 2 minutes to get out of
  the stand or terminal. MSN grid squares are 100 m, so this also absorbs
  the rounding. With no distance it is a cautious 15.
  `reference-data/tiploc-parent-stations.csv` gains a `walk_minutes`
  column (empty means "use the distance"), seeded as estimates, not
  surveyed times: `HTRBUS3`->HXX 8, `CRDFAIR`->RIA 20, `IVRNABS`->IVA 15,
  `PNZQUAY`->PNZ 10. Both `schedule-reference` and `api` read the file
  through `common::tiploc_parents`.
- **Change time at the stop.** A linked stop keeps its own change time. A
  98/99 sentinel there, if one ever appears, is dropped so that a walk-in
  boarding works. Unlinked stops are left as they are.
- **Bus/ferry buffer.** The buffer is not baked into the walk. Boarding a
  bus after the walk owes it as before. Alighting from a bus or ferry and
  then walking on now owes it too, before the walk starts. That applies to
  every fixed link, ALF ones included. The arrive-by scan already charged
  it, but CSA, RAPTOR and the staged search did not, so the forward and
  backward searches disagreed. They now agree. A journey that starts at the
  stop owes nothing. Example: a bus arrives at 10:00, walk 8, change 2 at
  HXX, so the train must leave at 10:15 or later.
- **Response.** The walk is a normal transfer leg, `kind: "transfer"`,
  `mode: "WALK"`, with the stop's `tiploc:` code as its
  `originCrs`/`destinationCrs`. `minutes` is the walk alone.

Rejected options:
- Writing `tiploc:` rows into `fixed_links`: that table is ALF's, and it is
  replaced on every ALF ingest.
- A new table: it would need a migration and a publisher change, for data
  the API can derive.
- Baking the buffer into the stop-to-station walk: there would be no
  planner change, but the arrive-by scan, which already charges the buffer
  when you get off a bus, would charge it twice.

Coverage on the 2026-10-05 delivery: 67 stops have a parent in `stations`
(62 bus stops, 5 ferry terminals; 38 nearest, 25 same TIPLOC, 4 curated).
Prod's `tiploc_crs`/`stanox_crs` have none of them as a station TIPLOC, and
all 60 parents are known to the planner. That makes 67 stops linked, 134
directed links. 46 of the stops have bus or ship calls. Their walk times
are 3 min (16), 4 (10), 5 (13), 6 (2), 7, 8, 10, 15 and 20 (one each).
Example, from a local DB seeded read-only from prod's 2026-10-08 timetable
slice: Watford Junction -> Hayes & Harlington (`results=options`). The
09:56 RailAir coach reaches Heathrow Terminal 2's bus stop (`HTRBUS2`) at
10:47. A 5-minute `WALK` leads to HXX, and the 11:06 Elizabeth line train
arrives at 11:12. That is 76 minutes with 1 change, against 87 minutes
with 2 changes via Euston and Paddington.

## Coverage (2026-10-05 delivery)

Of the 132 TIPLOCs with a public call and no `tiploc_crs` row: all 132 named;
93 bus stops, 33 ferry terminals, 6 rail locations (`MARY10`, `VRGN217`
passing points, `ABHLJN` junction, `LRDDEAC` siding, `PADTLUL`, `PRNCSTG`
other). 32 have a parent station in `stations` (11 same TIPLOC, 17 nearest,
4 curated). Across the whole delivery: 224 bus stops and 143 ferry
terminals, 152 of them served.
