# `/plan`: filtering by operator in "Plan a route for me"

Design, 2026-10-31. Status: **accepted for build** (DS-214). The open
questions of the first draft were answered on 2026-10-31 (§11); the rest
are proposed defaults. No feature code ships with this document. It
covers `GET /Trips/plan`, the `/plan` page's "Plan a route for me" mode,
saved journeys' replans, and DS-MCP's `plan_journey`. "§" alone means a
section of this document.

The user decided on 2026-10-30:

- **Both directions:** "only these operators" (include) and "avoid these
  operators".
- **Codes, not modes:** filtering an operator also covers its own buses,
  replacement buses and ferries, which carry the same ATOC code.
- **Brand groups** in the picker, e.g. GTR = Thameslink, Great Northern,
  Southern and Gatwick Express.
- **"Prefer an operator"** is a labelled option in Compare options
  ("Avanti West Coast only"), not a tie-break.
- Defaults left to this design: a train with no known operator passes an
  include filter; London Underground (`LT`) is selectable; the operator
  index lives on the planning graph.

And on 2026-10-31, answering the first draft:

- **The operator id rides on `Connection`** (§6.2), built from the
  graph's table. A UID lookup in the hot path was measured at 8% to 36%
  more instructions.
- **A filter on an operator covers every way of riding that operator.**
  "Avoid London Underground" avoids the Tube entirely: `LT` trains in the
  CIF, Tube fixed links, `TfL` access rides on the Tube, and Tube transit
  trips once TfL P4 lands. "Only these" without London Underground means
  no Tube. The same holds for the DLR, London Trams and the other
  networks DS can attribute (§5.1).
- **Prefer** is bounded: Compare options only, kept only within twice
  the alternatives' detour allowance. With Fastest it is disabled with a
  note.
- **Saved journeys remember the plan's operator filter**, and their
  replans and break suggestions honour it (§6.9).

## Contents

1. [Summary](#1-summary)
2. [What exists today](#2-what-exists-today)
3. [The data](#3-the-data)
4. [On `/plan`](#4-on-plan)
5. [Semantics and edge cases](#5-semantics-and-edge-cases)
6. [Planner and API design](#6-planner-and-api-design)
7. [Performance](#7-performance)
8. [Implications and risks](#8-implications-and-risks)
9. [Phases and effort](#9-phases-and-effort)
10. [Tests](#10-tests)
11. [Decisions and proposed defaults](#11-decisions-and-proposed-defaults)

## Errata from the build (2026-10-31)

Found while building P0 and P1:

- **NCZ–NOP is the Tyne and Wear Metro, not NET.** NOP is Northumberland
  Park (Tyne & Wear). `reference-data/fixed-link-operators.csv` gives it
  `TW`, the Metro's own ATOC code. No `METRO` link is NET's.
- **`LF` is identified** from the timetable (§11 item 9). It runs 10
  express passenger trains a weekday under open-access retail service IDs:
  London Euston to Stirling and back via Preston, Carlisle, Motherwell and
  Larbert, plus Preston to Euston and back. That is Grand Union's
  Euston to Stirling service. `reference-data/operator-names.csv` records it,
  with the evidence.
- **The "only five of ten" budget on a Connection Scan** is 1.5x, not
  +5%. One of the 20 bench pairs loses its route, and a scan with no answer
  runs to the end of the day (§7.3).
- **`preferOperator` ships with P3**, not P1: P1's API has `operator` and
  `avoidOperator` only.
- **`noResultReason.values`** for `operator` / `avoidOperator` lists the
  codes in code order, not request order. The echoed `operator` and
  `avoidOperator` keep request order.
- **Group members aren't checked against the timetable.** Only ATOC codes
  named directly are. A group may hold a code that runs nothing that week;
  `operatorsNotRunning` says so.

## 1. Summary

- **Control.** One "Operators" field in `/plan`'s Advanced options: a
  choice of *Only these*, *Avoid these* or *Prefer* (Compare options
  only), and a multi-select of operators and brand groups. The picker is
  the Trains search's (`operatorChoices`), plus groups and the operators
  that run no CIF trains (DLR, London Trams, Metrolink, NET).
- **API.** Three mutually exclusive list parameters on `GET /Trips/plan`:
  `operator` (only these), `avoidOperator` and `preferOperator`. Each
  takes comma-separated ATOC codes, DS operator codes (`ds:DLR`) or
  `group:NAME` entries. A new `GET /Trips/operator-groups` endpoint lists
  the groups and DS operators. The response echoes the codes as applied,
  plus `operatorGroups`.
- **One operator, every way of riding it.** The rule is checked inside
  every search, as `avoid` is, on four things:
  - CIF connections (trains, buses and ferries), by ATOC code;
  - fixed links, by an operator attributed to each link (`TUBE` links,
    and `TRANSFER` links between the same station pairs, are London
    Underground; London `METRO` links are the DLR or London Trams;
    Manchester's are Metrolink);
  - `TfL` access rides, by their line (Tube lines, the DLR, trams);
  - transit trips, by network and line mode.

  Walks (`WALK` links and same-station changes) always pass, as
  does anything DS can't attribute to an operator.
- **Index.** `PlanningGraph` gains an operator table built from
  `schedule_services.operator_atoc`, which covers every graph UID (the
  current source, `schedule_destination_departures`, misses about 850
  buses and ferries a day). Each `Connection` and each `FixedLink`
  carries a 2-byte operator id, so the check is an array read.
- **Empty results explain the filter.** There are two new
  `noResultReason` constraints, `operator` and `avoidOperator`. When the
  Tube (or another link-only network) is what's missing, the message
  names it: "No journey using only Avanti West Coast and Southern
  connects at London Euston: crossing London needs the London
  Underground, which this filter leaves out."
- **Prefer** adds one option labelled `{"kind": "operatorOnly"}` ("Avanti
  West Coast only") to Compare options, if it's within twice the
  alternatives' allowance. It costs one restricted Connection Scan per
  segment, about +5% of an options request.
- **Saved journeys** store the filter on `journeys` (one migration,
  three nullable columns). Replans and break suggestions read it.
- **Fixes on the way (P0):**
  - the journey replan's "same operator" search uses the graph's table,
    which keeps the operator's own buses and ferries and never blocks
    trams;
  - leg `operator` is read from the same table, so bus and ferry legs
    stop showing no operator;
  - `Restrictions::blocks` stops trimming TIPLOCs it doesn't need to.
    That trimming currently costs +27% instructions on every restricted
    Connection Scan.
- **Phases:** P0 index and fixes (1.5 days), P1 API (2.5 days), P2 `/plan`
  UI (2 days), P3 Prefer (1 day), P4 DS-MCP (0.5 day), P5 saved journeys
  (1.5 days). Total about 9 days.

## 2. What exists today

Verified on `main` at `6fc1cc29` (after #74 options, #80/#82 TfL
timings and #86 Trains stop filters).

### 2.1 Planner

- `Restrictions` (`crates/trip-planner/src/restrictions.rs`) is checked
  per connection by CSA (`csa.rs:700`), RAPTOR (`raptor.rs:330`), the
  staged waypoint search (`staged.rs:605`) and arrive-by (`reverse.rs:339`).
  Vias, `acrossMidnight`, the live overlay's replacement connections and
  `options::diversify` all go through those four. A blocked connection
  also ends the ride on its train (the searches drop the UID from their
  "aboard" set), so a passenger can't board before a blocked stretch and
  stay on after it.
- **Fixed links are never restricted by operator today.** They are
  expanded by `fixed_links_from` in CSA (`csa.rs:558`), RAPTOR
  (`raptor.rs:276`) and the staged search (`staged.rs:468`), and by
  arrive-by's own reverse index (`reverse.rs:132`). `FixedLink` carries
  only `mode`, `to_crs`, minutes and validity. `TfL` access rides
  (DS-135) are added as per-request fixed links of mode `TUBE` or `METRO`
  (`tfl_access::line_mode`), so they go through the same four places.
- `Restrictions::with_allowed_uids(HashSet<String>)` already limits the
  searches to a set of trains. Its only caller is the journey replan's
  "same operator" search (`crates/api/src/routes/journey_replan.rs`,
  about line 518). It builds the set with `SELECT DISTINCT train_uid FROM
  schedule_destination_departures WHERE service_date = $1 AND
  operator_atoc = ANY($2)`. Two problems:
  - **It misses buses and ferries.** On 2026-10-31, 854 of the graph's
    25,738 UIDs had no `operator_atoc` row there, every one a bus or a
    ferry (SR 299 buses, GW 188, EM 61 and so on; QC 35 ferries). So a
    "same operator" replan for a GWR journey can't use GWR's own
    rail-replacement or connecting buses. `schedule_services` covers all
    25,738.
  - **It would block every tram.** Transit trips are ordinary connections
    with `<network>:<n>` UIDs (`transit_planning::transit_uid`), and
    they're never in the set. That's latent today: `plan_for_journey`
    plans with `transit: false`, so no tram is reachable anyway. It
    becomes real as soon as replans plan with transit, and TfL P4 will
    put the Tube in the same array.
- `Connection` (`schedule_query::connections`) has no operator.
  `PlanningGraph` (`crates/api/src/data/trip_planning.rs:500`) has none
  either.
- Leg details (`trip_leg_details.rs:284`) read each leg's `operator`
  from `schedule_destination_departures`, so the same 854 buses and
  ferries are served with `operator: null`.
- Empty segments are explained by `explain_empty_segment`
  (`trip_planning_itinerary.rs:2490`). It re-runs the search with
  `restrictions = None` and blames the avoid lists when that finds
  something. Any operator rule placed in `Restrictions` would be dropped
  by that re-run, and the avoid lists would be blamed for it (§6.5).

### 2.2 Tickets and journeys

- `alternative_ranking::ticket_constraint` turns tracked tickets into
  `OperatorOnly` (a ticket type containing "only") or `TrainSpecific`
  (Advance).
- `delay_repay_rules::atoc_for_operator_text` maps a ticket's operator
  text to an ATOC code.
- Both serve the mid-journey replans (mid-journey changes spec §17), not
  `/Trips/plan`.
- A planned route is saved with `POST /Journeys/itinerary`
  (`routes/journey_itinerary.rs`, `ItineraryRequest`: `customName` and
  `legs`). The request's serialisation is the idempotency hash, so field
  order and `skip_serializing_if` are load-bearing. Nothing about how
  the route was planned is stored: `journeys` holds the owner and name;
  `journey_revisions` and `journey_revision_legs` hold the legs.

### 2.3 Frontend

- **Trains search** (`TrainSearchForm.tsx`): "Operator (optional)" is a
  Mantine `MultiSelect` on the main form, not inside Advanced options. It
  says "Only trains run by one of these operators." and fills from
  `operatorChoices(operatorNames, unnamedOperatorCodes)`: `tocs` names
  first ("GW · Great Western Railway"), then timetable codes with no name
  ("TW"). The URL holds it as `?operator=GW,SW`. #86 put the stop filters
  in `TrainsAdvancedOptions`, which shares `AdvancedOptionsDisclosure` and
  `STOP_LIST_FIELDS` with the planner.
- **Planner** (`PlanTripAdvancedOptions.tsx`): pass-through vias, the
  three avoid lists and "Most changes", in the same disclosure. The
  collapsed summary line reads, for example, "Pass through STA · Max 1
  change". `countAdvancedOptions` feeds the "(2 set)" count.
  `noResultHint` turns each `noResultReason.constraint` into a
  suggestion.
- `/plan` shows **no operator on a leg**. `ItineraryOption.tsx` never
  reads `leg.operator`, though the API serves it.
- `OperatorField` (`CodeLookupField` with `searchTocs`/`operatorLabels`)
  is the single-code picker used by the ticket and leg forms.
  `JourneyLegCandidates` (the add-leg dialog) has its own operator filter,
  whose redesign is planned separately (§8.6).
- Analytics: `trains_filters` records which stop filters a Trains search
  used (kinds only). The planner has no `*_filters` feature.

### 2.4 DS-MCP

`plan_journey` (`Distant-Signal-MCP/src/tools/plan-journey.ts`) takes
`via`, `avoid`, `avoidStop`, `avoidChange` and `maxChanges`, and now
prints each leg as "Great Western Railway (GW)" (DSMCP-26). It has no
operator input.

## 3. The data

Read-only SELECTs on production, 2026-10-30.

### 3.1 Codes in the timetable

`schedule_services` for Tuesday 2026-11-03, by `operator_atoc`:

| Code | `tocs` name | Schedules | Trains | Repl. bus | Bus | Ferry |
|---|---|---:|---:|---:|---:|---:|
| SR | ScotRail | 2,923 | 2,278 | 0 | 643 | 2 |
| NT | Northern | 2,849 | 2,743 | 105 | 1 | 0 |
| GW | Great Western Railway | 2,187 | 1,724 | 14 | 449 | 0 |
| SE | Southeastern | 1,823 | 1,809 | 14 | 0 | 0 |
| SW | South Western Railway | 1,762 | 1,612 | 7 | 79 | 64 |
| SN | Southern | 1,707 | 1,692 | 15 | 0 | 0 |
| LO | London Overground | 1,542 | 1,542 | 0 | 0 | 0 |
| LM | LNR & WMR | 1,334 | 1,319 | 15 | 0 | 0 |
| LE | Greater Anglia | 1,311 | 1,307 | 0 | 0 | 4 |
| AW | Transport for Wales | 1,261 | 1,063 | 19 | 164 | 15 |
| ME | Merseyrail | 817 | 618 | 199 | 0 | 0 |
| XR | Elizabeth line | 696 | 696 | 0 | 0 | 0 |
| TL | Thameslink | 653 | 653 | 0 | 0 | 0 |
| EM | East Midlands Railway | 636 | 508 | 4 | 124 | 0 |
| GN | Great Northern | 447 | 400 | 15 | 32 | 0 |
| LT | London Underground | 435 | 435 | 0 | 0 | 0 |
| VT | Avanti West Coast | 423 | 318 | 6 | 97 | 2 |
| CC | c2c | 398 | 267 | 79 | 52 | 0 |
| CH | Chiltern Railways | 355 | 350 | 5 | 0 | 0 |
| XC | CrossCountry | 346 | 280 | 0 | 66 | 0 |
| TP | TransPennine Express | 345 | 319 | 24 | 2 | 0 |
| GR | LNER | 318 | 200 | 0 | 118 | 0 |
| TW | *(none)* | 174 | 174 | 0 | 0 | 0 |
| HX | Heathrow Express | 152 | 152 | 0 | 0 | 0 |
| IL | Island Line | 138 | 0 | 138 | 0 | 0 |
| QC | *(none)* | 77 | 0 | 0 | 0 | 77 |
| GX | Gatwick Express | 72 | 72 | 0 | 0 | 0 |
| GC | Grand Central | 20 | 20 | 0 | 0 | 0 |
| HT | Hull Trains | 15 | 15 | 0 | 0 | 0 |
| LD | Lumo | 12 | 12 | 0 | 0 | 0 |
| LF | *(none)* | 10 | 10 | 0 | 0 | 0 |
| CS | Caledonian Sleeper | 10 | 10 | 0 | 0 | 0 |

What this means for the design:

- **No NULL or placeholder codes.** Every graph UID has a code. So
  "unknown operator" is a safety rule for a missing `schedule_services`
  row, not a live case.
- **Buses and ferries are a big share of some operators.** They are 22%
  of ScotRail's schedules, 21% of GWR's, 37% of LNER's and 25% of
  Avanti's. "Only LNER" keeps LNER's buses by the user's decision. Results
  therefore have to show the bus clearly (§4.4).
- **Three codes have no `tocs` name.** The Trains search already shows
  them as bare codes. By their routes, `TW` is the Tyne and Wear Metro
  (Sunderland line) and `QC` runs ferries (e.g. Ullapool to Stornoway).
  `LF` is **unidentified**: 10 trains a day via the West Coast Main Line
  (one runs Preston, Wigan, Warrington, Crewe, Stafford and on south).
  §11 has the plan to identify it.
- **`tocs` has codes the timetable never uses.** These are `LN` (London
  Northwestern), `WM` (West Midlands Railway), `SX` (Stansted Express),
  `ES`, `HS`, `NR`, `ZN`, `GM`, `HV`, `XP` and `XS`. LNR and WMR both run
  as `LM` ("LNR & WMR"), and Stansted Express runs as `LE`. A picker built
  from `tocs` alone would offer "West Midlands Railway", and that choice
  matches nothing. §4.2 handles this with aliases.
- **`IL` (Island Line)** is all `replacement_bus` in this week's timetable
  (engineering works). "Only Island Line" would return its buses, which
  is correct.

### 3.2 The London Underground and the other link-only networks

The London Underground appears to the planner in four ways. Only the
first carries an ATOC code:

| Way | What | Size |
|---|---|---|
| CIF `LT` trains | Underground trains on Network Rail lines, e.g. `C61095`: the Bakerloo from Harrow & Wealdstone to Elephant & Castle | about 435 a day |
| `TUBE` fixed links | The timetable's "allow N minutes by Tube" links between stations | 1,909 rows, 483 station pairs, from 52 London stations |
| `TfL` access rides | A Tube stop as From or To (DS-135), timed rides on a Tube line (#80/#82) | 11 Tube lines in `tfl_stops` (272 stops) |
| Transit trips (TfL P4) | Tube trips as connections, `tfl:<n>` | about 230,000 connections a weekday, when built |

The other networks DS can attribute have no ATOC code at all:

| Network | Fixed links | Access rides | Transit trips |
|---|---|---|---|
| DLR | `METRO` links CUS–SRA, CUS–WEH, CWX–GNW, CWX–LEW, CWX–SRA, SRA–GNW, SRA–LEW | `dlr` line (45 stops) | TfL P4 |
| London Trams | `METRO` link ELE–BKJ | `tram` line (39 stops) | TfL P4 |
| Metrolink | `METRO` links around ALT, DGT, MAN, MCV, TIM, RCD, NVR, XNA (about 1,700 rows) | none | `metrolink:<n>` (DS-164) |
| NET (Nottingham) | none | none | `net:<n>` |

The Elizabeth line (`XR`) and the Overground (`LO`) run only as CIF
trains. They have no fixed links, `tfl_stops` lists none of their lines,
and the TfL spec keeps them out of the transit network. So their ATOC
code already covers every way of riding them.

**`TRANSFER` links are not walks.** The 661 `TRANSFER` rows (230 pairs,
5 to 70 minutes, mean 30) are the timetable's mode-unspecified transfer
allowances. 140 of their pairs are exactly London Tube pairs, at Tube
speed: EUS–VIC is `TRANSFER` 19 minutes, `TUBE` 9 or 14, and about 62
minutes on foot by DS's walk model (straight line × 1.25 at 4.5 km/h).
PAD–LST is `TRANSFER` 35 against about 109 on foot. Others are regional
(Manchester MAN–MCV 22, Edinburgh EDA–HYM 40, bus-station hops such as
BRI–QBB 10) or London pairs with no Tube link (BFR–CHX 15, LBG–VIC 30).

The rest of the links table has no operator DS can name: `WALK` 249 rows
(1 to 60 minutes; in London only short ones such as KGX–STP 1 and FST–LST
12), `BUS` 57 (local buses such as FEL–HTR), `FERRY` 2 (KWR–XMT), `TRAM`
4 (BPN–QBN, BPS–QBS), a few `METRO` rows (LTN–LUA, the Luton airport
shuttle; DCL–DHT), and the 90 `TRANSFER` pairs that aren't Tube pairs.
They stay unattributed (§5.1).

**Walking instead of the Tube.** No `TUBE` pair has a `WALK` row. The
main termini have only `TUBE` and `TRANSFER` links between them, both
Tube-speed. Of the 258 Tube pairs whose stations have coordinates in
`stations`, DS's walk model puts 13 within 20 minutes on foot, 27 within
30 and 38 within 40 (LBG–LST 24, VIC–WAT 39, EUS–WAT 52, KGX–LST 53).
The other 225 pairs lack coordinates (mostly Underground-only `Z` codes).
§5.1 adds estimated walks so that "no Tube" still leaves the walkable
crossings.

Found on the way, outside this design: `tripPlanSave.ts` treats a
`TRANSFER` leg as a change within one station and drops it when saving
(`transferAsUntracked` returns `null`), but EUS–VIC `TRANSFER` is a
19-minute cross-London transfer. That is worth its own card.

### 3.3 Associations

VV/JJ associations (joins and divides) never cross operators in the data,
so a through journey on an associated train never changes operator
mid-ride. No special case is needed.

## 4. On `/plan`

### 4.1 Where the control lives

It goes in **Advanced options**, between "Don't change at" and "Most
changes". This is not the Trains search's place (its Operator is on the
main form), and that's deliberate:

- The planner's main form is already the busiest in the app (From, To,
  stops, date and time, results). Every other way to shape a route lives
  in Advanced options.
- The collapsed disclosure keeps a set filter visible through its "(1
  set)" count and its summary line, so a restored or shared search never
  hides the filter. That matters here because the filter can make results
  look worse (§8.1).

**Consistency with the Trains search:** the same option labels
(`operatorChoices`, "GW · Great Western Railway"), the same URL syntax
(comma-separated codes) and the same "No matching operators" message.
The difference, a mode selector, follows from the planner having three
modes.

### 4.2 The field

```
Operators
  ( Any )  ( Only these )  ( Avoid these )  ( Prefer )      <- SegmentedControl
  [ GTR (Thameslink, Great Northern, Southern, Gatwick Express) x ] [ London Underground x ]
  Only routes on these operators' trains, buses, ferries, links and
  connecting rides. Walks between stations are always allowed.
```

- **Mode.** A `SegmentedControl` (radios underneath, keyboard-operable).
  With *Any* the picker is hidden. Switching mode keeps the picks.
- **Description**, per mode:
  - *Only these:* "Only routes on these operators' trains, buses,
    ferries, links and connecting rides. Walks between stations are
    always allowed. Add London Underground to cross London by Tube."
  - *Avoid these:* "Never use these operators' trains, buses, ferries,
    links or connecting rides."
  - *Prefer:* "Compare options adds the best route using only these
    operators, labelled." With Results set to Fastest, the field shows
    "Prefer needs Compare options." and is disabled.
- **Quick add.** Under *Only these*, when the list doesn't include
  London Underground, a text button reads "Add London Underground". It
  makes the common "my trains plus the Tube" choice one tap.
- **Picker.** A `MultiSelect`, searchable and clearable, at most 8
  entries (the avoid lists' `MAX_AVOIDED`). Options:
  1. **Groups first**, from `GET /Trips/operator-groups`: "GTR
     (Thameslink, Great Northern, Southern, Gatwick Express)". Sent as
     `group:GTR`, like station groups.
  2. **Every code running in the published window** (the planner's 8
     days), by name: `operatorChoices` over `tocs` names plus unnamed
     codes. A code that is in `tocs` but has no trains in the window
     (`ES`, `NR` and so on) is left out. London Underground's label says
     what it covers: "LT · London Underground (the Tube, and its trains
     on National Rail lines)".
  3. **DS operators**, from the same endpoint: "DLR", "London Trams",
     "Metrolink", "NET". Sent as `ds:DLR`, `ds:LONDON_TRAMS`,
     `ds:METROLINK` and `ds:NET` (§6.4).
  4. **Aliases.** Typing "West Midlands Railway" or "London
     Northwestern" finds "LM · LNR & WMR (West Midlands Railway and London
     Northwestern Railway)". Typing "Stansted Express" finds "LE ·
     Greater Anglia (includes Stansted Express)". Typing "Tube" or
     "Underground" finds `LT`. An alias is search text only, never a
     separate value, so a user can't pick a code that matches nothing.
     Unnamed codes get DS display names: `TW` "Tyne and Wear Metro", `QC`
     "CalMac ferries". `LF` stays a bare code until identified (§11).
- **Initial groups** (`reference-data/operator-groups.csv`, compiled in
  like `station-groups.csv`):

  | Group | Members | Status |
  |---|---|---|
  | `GTR` | TL, GN, SN, GX | User decision |
  | `TFL` | LO, XR, LT, ds:DLR, ds:LONDON_TRAMS | Proposed default |
  | `SWR` | SW, IL (Island Line is run by SWR) | Proposed default |

  West Midlands Trains needs no group (one code, `LM`). Groups are
  reviewed whenever `tocs` or the timetable gains a code (§5.9).
- **Validation on the field**, mirroring the API's 400s the way
  `advancedOptionErrors` does: more than 8 entries, and Prefer with
  Fastest. There are no cross-field conflicts with the station lists. A
  code the API rejects (from a stale URL) shows the API's message on the
  field.
- **Summary line and count.** The filter counts once in "(n set)", and
  the summary reads "Only Avanti West Coast, LNER", "Avoid GTR" or
  "Prefer LNER".

### 4.3 URL

`/plan` keeps using the API's names (`planPageSearch`): `operator=VT,GR`,
`avoidOperator=group:GTR` or `preferOperator=GR`. Parsing
(`PlanFormInitial`) accepts 2-character alphanumeric codes, `ds:NAME` and
`group:NAME`, uppercases them, drops anything else, and keeps only the
first of the three parameters present (the API would 400 on two). A
saved or shared URL from before this change has none of them, so it is
unaffected.

### 4.4 Results

- **Each train, bus or ferry leg shows its operator.** This is new, and
  the filter is hard to trust without it. Add "Avanti West Coast" to the
  leg's secondary line, from `leg.operator` through `operatorNames`, the
  same lookup the Trains search rows use. A bus leg already says it's a
  bus. With a filter on, it reads "LNER rail replacement bus", so "only
  LNER" visibly kept LNER's own bus. Show nothing for a missing operator,
  as the Trains search does. Transfers already say "Tube", "Walk" and so
  on.
- **A filter banner** above the results whenever a filter applied, from
  the response echo, not the form. It has a "Change" link that opens
  Advanced options. Examples:
  - "Showing routes using only Avanti West Coast. Walks between stations
    are included; the Tube is not."
  - "Showing routes using only Avanti West Coast and London Underground."
  - "Showing routes that avoid London Underground, including the Tube.
    London crossings are by other trains or on foot."

  When a Prefer option was added, there is no banner; the option's
  label says it.
- **Empty or no-route states** (`noResultHint`):
  - `operator`: "No journey using only Avanti West Coast departs after
    18:00 on Tue 3 Nov; one exists with other operators." Hint: "Try
    adding operators, or remove the operator filter in Advanced
    options."
  - `operator` where the missing piece is a link-only network
    (`linkOperators` set, §6.5): "No journey using only Avanti West
    Coast and Southern; crossing London needs the London Underground,
    which this filter leaves out." Hint, with a button: "Add London
    Underground".
  - `avoidOperator` with `linkOperators`: "No journey avoiding London
    Underground; every route needs the Tube somewhere, and there's no
    walk link instead." Hint: "Remove London Underground from Avoid
    these."
  - `operator` with a message saying the operator runs nothing that day:
    "Avanti West Coast runs no trains on Sun 1 Nov." (§5.9)
  - `avoidOperator`: "No journey avoiding GTR …; one exists using them."
    Hint: "Try removing GTR from Avoid these."
  - `avoidCombined` grows to cover an operator filter that only fails
    together with avoid lists (§6.5): "Try removing some of the
    stations or operators in Advanced options: together they rule out
    every route."
- **Compare options labels:** `operatorOnly` renders "Avanti West Coast
  only" (or "GTR only" for a group, "Avanti West Coast or LNER only" for
  two entries). It uses the neutral tone, like `alternative`.
- **Fastest with a filter** returns the fastest compliant route, with no
  "faster with other operators" note (proposed default, §11).

### 4.5 Accessibility

- The mode is a labelled radio group ("Operators") with a visible
  description, and the picker's `aria-describedby` points at it.
- The banner is plain text in the results region, announced with the
  results (no extra live region). Its "Change" link is a real link that
  opens the disclosure and moves focus to the mode control.
- The "Add London Underground" buttons are real buttons whose text says
  what they do.
- The leg operator text sits in the leg's existing text, not in colour
  or an icon alone. The `operatorOnly` badge text is the full label.
- The `MultiSelect` keeps the Trains search's clear-button label ("Clear
  operator filter") and its 24px hit area (`iconHitArea24`).

## 5. Semantics and edge cases

### 5.1 One operator, every way of riding it

An **operator** here is either an ATOC code or a DS operator (`ds:DLR`,
`ds:LONDON_TRAMS`, `ds:METROLINK`, `ds:NET`). Everything the planner can
ride is attributed to at most one operator:

| What | Attributed by | Operator |
|---|---|---|
| CIF connection (train, bus, ferry) | `schedule_services.operator_atoc` | its ATOC code |
| `TUBE` fixed link | its mode | `LT` |
| `TRANSFER` fixed link on a Tube pair | its station pair also has a `TUBE` link | `LT` |
| `METRO` fixed link | `reference-data/fixed-link-operators.csv` (station pair) | `ds:DLR`, `ds:LONDON_TRAMS`, `ds:METROLINK`, `ds:NET` |
| `TfL` access ride | its line (`TflStopLine.id`) | Tube lines: `LT`; `dlr`: `ds:DLR`; tram: `ds:LONDON_TRAMS` |
| Transit trip | its network and, for `tfl`, its line's mode | `ds:METROLINK`, `ds:NET`; Tube: `LT`, DLR, London Trams |

The rule:

- **Only these:** anything attributed to an operator outside the list is
  blocked.
- **Avoid these:** anything attributed to a listed operator is blocked.
- **Always allowed:** walks (`WALK` links, same-station changes, walk
  access to a `TfL` stop, the walks of `alight_then_walk`).
- **Unattributed** (no operator DS can name): passes both modes, like a
  train with no known operator. Today that's no CIF train, and the
  `BUS`, `FERRY`, `TRAM`, leftover `METRO` and non-Tube `TRANSFER` links
  of §3.2. The CSV can attribute them later. Each one is counted at
  graph build (§6.7).
- **Estimated walks instead of the Tube** (approved, §11 item 14). When the
  rule leaves `LT` out, each `LT`-attributed link pair whose stations
  have coordinates gets a walk link instead, timed by `tfl_access`'s walk
  model (straight line × 1.25 at 4.5 km/h). It's only offered when that
  takes at most **30 minutes**: 27 of today's measurable pairs, e.g.
  LBG–LST 24. The link is labelled as an estimated walk. Without it, "no
  Tube" would also mean no walking between termini, because the
  timetable has no walk rows there (§3.2).

So, for London:

- "**Only Avanti West Coast**" from Manchester to Brighton: Avanti to
  Euston, then no Tube. Euston to Victoria is a Tube or Tube-speed
  `TRANSFER` link and about 62 minutes on foot, so no walk is offered.
  The route fails, and the reason names London Underground (§6.5). "Only
  Avanti West Coast and London Underground" gets the Tube back.
- "**Avoid London Underground**": no `LT` trains, no `TUBE` links, no
  Tube-pair `TRANSFER` links, no Tube access rides, no Tube transit trips.
  London crossings go by other operators' trains (Thameslink, the
  Elizabeth line, the Overground), on foot where the estimate is 30
  minutes or less, by an unattributed `TRANSFER` (BFR–CHX), or not at
  all.
- **A `TfL` stop end** (Uxbridge, DS-135) under a filter that blocks the
  Tube keeps only its walk candidates. With none left, the segment has
  no route and the reason names London Underground. It is not a 400:
  the stop is valid, the filter emptied it.

**Unknown operator** for a CIF connection (no `schedule_services` row:
today never, but a schedule-reference publish can lag): passes both
include and avoid, the user's default. It is counted in a metric (§6.7)
so a lag shows up.

### 5.2 Include ("Only these")

Changing between two allowed rides is allowed through any walk. A
blocked connection ends the ride (§2.1), so a train that changed operator
mid-schedule couldn't be ridden through, but schedules never change code
(§3.3).

### 5.3 Avoid

Anything attributed to a listed operator is blocked. Nothing else changes.

### 5.4 Prefer

Prefer is not a filter. The normal search runs unfiltered. Then, with
Compare options only:

1. If a kept option already rides only preferred operators (under §5.1's
   attribution, links and access rides included), it gets the
   `operatorOnly` label too. No extra search runs.
2. Otherwise one extra search runs with the include rule: a restricted
   Connection Scan in the request's direction, the same cost as one
   `diversify` attempt. For a waypoint plan, it is the staged scan.
3. The result is kept only if it arrives (arrive-by: leaves) within
   **twice** the alternatives' detour allowance (`allowance_minutes`, so
   40 to 180 minutes) and within `maxChanges`. If it is kept and the
   list is full (`MAX_OPTIONS` = 5), it replaces the last alternative. A
   Prefer the user asked for outranks an alternative the user didn't ask
   for.
4. When nothing qualifies, the response carries `preferOperatorResult:
   "none"` or `"tooSlow"`. The UI then says "No Avanti West Coast only
   route within 2 hours of these."

With Fastest, Prefer is disabled in the UI and a 400 in the API. Prefer
never hides or reorders the other options. That is the user's "not a
tie-break".

### 5.5 Combinations

| With | Behaviour |
|---|---|
| `via`, `avoid`, `avoidStop`, `avoidChange` | All apply together (AND). A via is only satisfied by something actually ridden or walked, so under "Only X" a via needs an X train, an X link or a walk through it. |
| `waypoints` | The filter applies to every segment. It is per request, not per segment (proposed default). |
| `maxChanges` | Unchanged. An include filter often needs more changes. `cappedByMaxChanges` and the `maxChanges` reason fire as today, and that check runs before the operator blame (§6.5). |
| `arriveBy` | The reverse scans check the same rule, links included. |
| `dayOffset`, `acrossMidnight` | Each date's graph has its own table. A code is matched on both dates' trains. |
| Compare options alternatives | `diversify` adds its bans on top of the request's restrictions, so every alternative obeys the filter too. With an include filter there are usually fewer alternatives (§7). Labels are unchanged. |
| Earlier/later routes paging | It re-sends the same query, so the filter carries over. |
| Live overlay | Replacement connections keep the base UID, and so the operator. A filtered train that is cancelled or late is handled exactly as unfiltered. The in-search live delays (DS-140) use the same restrictions. |
| `transit=true`, tram or `TfL` stop ends | Transit trips and access rides follow §5.1: "Only Northern" from a Metrolink stop needs `ds:METROLINK` in the list, and the Add button's equivalent appears in the hint. |

### 5.6 Saved journeys and replans

A journey saved from a filtered plan **remembers the filter** (§6.9).
Its replans and break suggestions apply it the same way `/Trips/plan`
does:

- **Only / Avoid** apply to every replan search: the main options
  search, the next departures, the rejoin search and the ticket's "same
  operator" search. For the last, the ticket's operators are intersected
  with an Only list (an empty intersection skips the search), or have
  an Avoid list removed.
- **Prefer** adds one labelled "<Operator> only" search to the replan,
  next to the ticket's "same operator" search, with the same keep bound
  as §5.4.
- **When the filter leaves no suggestion at all** (approved, §11 item 11):
  the replan runs once more without it and returns its best suggestion,
  flagged `outsideOperatorFilter: true` and shown as "Uses operators you
  chose to avoid" or "Uses other operators". A disrupted traveller should
  never get "nothing" because of a preference they set days ago. The
  flagged suggestion never outranks a compliant one.
- The ticket ranking (`alternative_ranking`) is unchanged and still
  applies on top. The filter narrows what is searched; the ticket
  decides how suggestions rank.

### 5.7 Tickets

The filter selects **services, not fares**. Three caveats show in the
field's help text, behind an "About tickets" disclosure:

- **"Only these" is not "valid on my ticket".** An "LNER only" ticket
  also limits route and time. An Advance is valid on one train
  (`TrainSpecific`). A route of only LNER trains may still be off-route
  for the ticket.
- **Cross-London.** A through ticket usually includes the Underground
  between London termini. "Only LNER" leaves the Tube out (§5.1), so the
  help text says "Your ticket may include the Tube across London: add
  London Underground to use it."
- **Avoid** isn't a "Not via" or "Not Avanti" ticket route. It doesn't
  touch stations.

Later (not in this design's phases): prefill the filter from a tracked
ticket (`ticket_constraint` → "Only LNER and London Underground") when
planning a return.

### 5.8 Brand groups

A group is expanded to its members before validation and search. `GTR`
in "Only" means any of TL, GN, SN or GX. Groups can mix with codes
(`group:GTR,VT`). Members are deduplicated.

### 5.9 Codes over time

- Matching is on the date's own timetable. A code that stops running
  (a franchise change at a timetable change) simply has no trains on
  later dates.
- **Validation (400)** accepts an ATOC code when it is in `tocs` or runs
  a schedule on any searched date. This is the Trains search's
  `known_operator_codes` rule, so a typo is rejected. `ds:` codes are a
  fixed list. A known code with nothing running on the date is **not** a
  400:
  - under *Only*, if nothing in the list runs, the segment's reason is
    `operator`, with the message "Avanti West Coast runs no trains on Sun
    1 Nov";
  - under *Avoid*, it does nothing.
  - The API also echoes it in `operatorsNotRunning` so the UI can say so
    once. A `ds:` operator counts as running when it has links, access
    rides or trips in the date's graph.
- `operator-groups.csv`, `fixed-link-operators.csv` and the aliases
  (§4.2) are DS data, reviewed when a new code or link appears. The
  Trains search operators endpoint already lists unnamed codes, so a new
  code is visible there first.

## 6. Planner and API design

### 6.1 The operator table on the graph

`PlanningGraph` gains:

```rust
pub struct OperatorTable {
    /// Index = `OperatorId`: the ATOC codes of the date, then the DS
    /// operators (`ds:DLR`, ...), in a fixed order.
    codes: Vec<OperatorCode>,
    /// The date's schedules: UID -> operator id.
    by_uid: HashMap<String, OperatorId>,
}
pub struct OperatorId(u16); // OperatorId::UNKNOWN = u16::MAX
```

- **Source.** `SELECT uid, operator_atoc FROM schedule_services WHERE
  service_date = $1`, read in `fetch_calling_points_for_date`'s
  transaction. That is about 26k small rows. A missing row or NULL code
  is `UNKNOWN`.
- **Users.**
  - the searches (via the per-connection and per-link ids, §6.2);
  - leg details: `operator` comes from the table instead of
    `schedule_destination_departures`, which fixes §2.1's missing buses
    and ferries;
  - the replan's searches (§6.9);
  - validation (which codes run on the date), and `operatorsNotRunning`.
- **Memory.** About 26k entries at roughly 70 bytes is about 2 MB per
  cached date, 4 MB for two. Against the TfL spec's +60 MB budget this is
  small. The extension of `transit_planning_budget` in P0 measures it.

### 6.2 The ids on `Connection` and `FixedLink`

- `Connection` gains `operator: OperatorId`, set by `build_connections`
  from the table. Transit connections get their network's or line
  mode's DS operator (§5.1). The field fits in the struct's existing
  padding after the two `bool`s, so memory doesn't change. There are 57
  `Connection { .. }` literals, mostly in tests; they get
  `operator: OperatorId::UNKNOWN`.
- `FixedLink` gains `operator: OperatorId`, set when the interchange
  data is built: `TUBE` → `LT`; `TRANSFER` → `LT` when its station pair
  also has a `TUBE` link, else `UNKNOWN`; `METRO` → the CSV's operator
  for the station pair (either direction), else `UNKNOWN`; every other
  mode `UNKNOWN`. `WALK` links carry a separate flag (`is_walk`), so the
  rule can never block them by accident.
- **Estimated walks** (§5.1): at graph build, each `LT`-attributed pair
  whose stations have coordinates and whose walk estimate is at most 30
  minutes gets a `WALK` link with `is_walk` and `only_without: LT`. The
  searches skip it unless the request's rule leaves `LT` out, so plans
  without a filter don't change. That's a few dozen links per date.
  Transfer legs built from them say "Walk (estimated)".
- `TfL` access rides (`tfl_access`) set the link's `operator` from the
  candidate's line when they become per-request links. Their walk
  candidates are walks.

This is the user's 2026-10-31 decision (§11). §7 measured the
alternative: a per-connection UID lookup costs **+36% (SipHash) or +20%
(Fx) instructions** over the id on an avoid-one-of-ten Connection Scan,
and +15% or +8% on RAPTOR. The id costs nothing beyond the
`Restrictions` path itself. If TfL P4 interns `Connection` codes into
trip ids first, the id can live in the trip table instead, with the same
cost and the same API.

### 6.3 The `Restrictions` rule

```rust
pub enum OperatorRule {
    /// Only rides whose operator is allowed (UNKNOWN passes).
    Only(OperatorMask),
    /// Not rides whose operator is in the mask (UNKNOWN passes).
    Avoid(OperatorMask),
}
pub struct OperatorMask(Box<[bool]>); // indexed by OperatorId; or a u64 bitset
impl Restrictions {
    pub fn with_operator_rule(self, rule: OperatorRule) -> Self;
    pub fn operator_rule(&self) -> Option<&OperatorRule>;
    pub fn without_operator_rule(&self) -> Self;  // explain_empty_segment
    pub fn with_links_unrestricted(&self) -> Self; // explain_empty_segment
    pub fn allows_link(&self, link: &FixedLink) -> bool;
}
```

- `blocks` checks the rule first: one array read, no hashing.
- **Links.** `allows_link` is checked where a link is expanded: the
  three `fixed_links_from` loops (`csa.rs:558`, `raptor.rs:276`,
  `staged.rs:468`) and arrive-by's reverse index (`reverse.rs:132`).
  It is one array read per link visited, and walks skip it. The cached
  interchange data is not copied.
- **`allowed_uids` is removed.** The replan's searches use
  `OperatorRule` built from the same graph. Two ways to say "only these
  trains" would drift apart.
- **Fast path (P0).** Today `blocks` normalizes both TIPLOCs (`trim`) and
  does three empty-set probes on every connection whenever any
  restriction is set. Callgrind shows the `trim` alone is +27% of a
  restricted Connection Scan (§7.2), and every avoid, via and
  alternatives search pays it. P0 caches `has_station_rules: bool` and
  skips that part when it is false. That's a small, separately testable
  win, and it makes the operator rule's own cost visible.
- `with_extra` (used by `diversify`) clones `Restrictions`. The mask is a
  few dozen bytes. The old `allowed_uids` set was about 13k strings per
  clone.

### 6.4 API parameters

On `GET /Trips/plan`, all optional:

| Param | Meaning |
|---|---|
| `operator=VT,GR` | Only these operators (include). Same name and syntax as `/public/trains/search`, plus `ds:` and `group:` entries. |
| `avoidOperator=LT` | Never these. |
| `preferOperator=GR` | With `results=options`: add the best route on only these, labelled (§5.4). |

Each takes up to 8 entries, comma-separated: a 2-character ATOC code, a
DS operator (`ds:DLR`, `ds:LONDON_TRAMS`, `ds:METROLINK`, `ds:NET`) or a
`group:NAME`. Entries are case-insensitive, trimmed, and deduplicated
after expansion. Validation happens before any graph read, so it is
cheap:

- **400** for:
  - more than one of the three parameters;
  - a malformed entry ("operator must be a 2-character ATOC code,
    ds:NAME or group:NAME");
  - an unknown group or `ds:` name;
  - an unknown ATOC code (§5.9: not in `tocs` and not running on any
    searched date), naming every unknown code as the Trains search does;
  - more than 8 entries;
  - `preferOperator` without `results=options` ("preferOperator needs
    results=options").

  `alternatives=false` with `preferOperator` is allowed: Prefer is
  independent of the alternative routes.
- `GET /Trips/operator-groups`: `{"groups": [{"code": "GTR", "name":
  "GTR", "members": ["TL", "GN", "SN", "GX"]}], "dsOperators": [{"code":
  "ds:DLR", "name": "DLR"}]}`, with `Cache-Control: public,
  max-age=3600`, like `/Trips/station-groups`.
- `GET /public/trains/search/operators` stays as it is. The planner's
  picker filters it to codes running in the window. A `running: true`
  flag on that endpoint is additive and also helps the Trains search
  (§11).

### 6.5 Response

Additive top-level fields, always present (empty when unused), like
`avoid`:

```json
"operator": ["VT"],
"avoidOperator": [],
"preferOperator": [],
"operatorGroups": {"GTR": ["TL", "GN", "SN", "GX"]},
"operatorsNotRunning": [],
"preferOperatorResult": "added"   // options + preferOperator only: added | labelled | tooSlow | none
```

- **Codes are echoed expanded**, in request order, with groups listed in
  `operatorGroups`. This is the pattern of `stationGroups`.
- **`noResultReason`** gains `operator` and `avoidOperator`. `values`
  holds the codes, plus an optional `linkOperators` list naming the
  link-only network that made the difference (e.g. `["LT"]`).
  `explain_empty_segment`'s order becomes:
  1. `maxChanges`, as today.
  2. With every restriction off, still nothing: fall through to the time
     and route reasons, as today.
  3. With only the operator rule off: blame `operator` or
     `avoidOperator`. Then one more scan with the rule still on for
     connections but off for links and access rides
     (`with_links_unrestricted`). If that finds a route, set
     `linkOperators` to the operators of the links it used, and the
     message names them ("crossing London needs the London
     Underground").
  4. With only the station lists off: blame them as today (`avoid`, the
     single list, or `avoidCombined`).
  5. Both needed: `avoidCombined`, whose `values` now include the
     operator codes and whose message names both.

  That's at most three extra searches, and only for an empty segment.
  Without step 3, the existing code would blame `avoid` for an operator
  filter (§2.1).
- `labels` gains `{"kind": "operatorOnly", "operators": ["GR"]}`.
- Leg `operator` is now set for bus and ferry legs too (§6.1). That's a
  value change from null to a code, so it gets a changelog line.
- A transfer leg (`kind: "transfer"`) gains `operator` (`"LT"`,
  `"ds:DLR"` or null), so the UI and DS-MCP can show which network a
  link used.

### 6.6 `docs/api-changelog.md`

Two entries.

"Operator filters on `/Trips/plan`":

- the three parameters and their 400s;
- `ds:` operators and `/Trips/operator-groups`;
- the attribution rule (§5.1), since "avoid `LT`" now removes Tube
  links;
- the echo fields;
- the two reasons, `linkOperators` and the `avoidCombined` widening;
- the new label kind;
- the fact that bus and ferry legs carry `operator`, and transfers gain
  it;
- that clients which switch on the label kind or reason should treat an
  unknown kind as "other" (DS-MCP and the frontend already do for
  labels).

"Journeys remember their operator filter" (§6.9).

### 6.7 Observability

- A counter `trip_plan_operator_filter_total{mode}` (`only`, `avoid`,
  `prefer`). Modes only; no codes.
- A counter for CIF connections with an `UNKNOWN` operator at graph
  build, `trip_plan_graph_unknown_operator`. It should be 0. A non-zero
  value means `schedule_services` lags the calling points.
- A gauge of unattributed non-walk links per mode at graph build, so a
  new `METRO` link is noticed and added to the CSV.

### 6.8 DS-MCP `plan_journey`

- New optional inputs `operators: string[]`, `avoidOperators: string[]`
  and `preferOperators: string[]`. Each is described as ATOC codes,
  `ds:` operators or a group (e.g. `group:GTR`). They are mutually
  exclusive, which the MCP checks too. They go straight to the API's
  parameters.
- The tool description says the rule plainly: "London Underground (LT)
  covers the whole Tube. With operators set, add LT to cross London by
  Tube."
- The rendering adds a line under the summary: "Only Avanti West Coast
  (VT); walks between stations included, not the Tube."
- It renders the `operatorOnly` label and the new no-result reasons
  (with `linkOperators`) through the existing generic paths.
- A model asking "how do I get there on LNER only?" maps to `operators:
  ["GR"]`. The tool description lists the common codes' names via the
  operator catalogue it already has (DSMCP-26), so the model needn't
  guess codes.

### 6.9 Saved journeys: storing and using the filter

**Where.** On `journeys`, not on a revision. The filter is the
traveller's intent for the whole journey, like `custom_name`. It must
survive the new revisions that replans create, and it has no history
worth keeping.

**Migration** (`crates/ds-store/migrations/2026110…_journey_operator_filter.sql`,
the repo's `SET LOCAL lock_timeout = '5s'` pattern; adding nullable
columns and columns with constant defaults is metadata-only, so it is
quick on a live table):

```sql
ALTER TABLE journeys
    ADD COLUMN operator_filter_mode    TEXT,                       -- NULL: no filter
    ADD COLUMN operator_filter_codes   TEXT[] NOT NULL DEFAULT '{}', -- expanded, as applied
    ADD COLUMN operator_filter_entries TEXT[] NOT NULL DEFAULT '{}'; -- as chosen: 'group:GTR', 'VT'
ALTER TABLE journeys
    ADD CONSTRAINT journeys_operator_filter_mode_check
        CHECK (operator_filter_mode IN ('only', 'avoid', 'prefer')) NOT VALID,
    ADD CONSTRAINT journeys_operator_filter_consistent
        CHECK ((operator_filter_mode IS NULL) = (cardinality(operator_filter_codes) = 0)) NOT VALID;
```

A second migration validates the constraints, as `journey_revisions_
checks_validate` does. No backfill: existing journeys have no filter.

- **Expanded codes** are what replans apply, so a later edit to a group
  in `operator-groups.csv` doesn't silently change a saved journey.
- **Entries** are what the journey page shows ("Planned using only GTR").

**Write path.**

- `POST /Journeys/itinerary` gains an optional `operatorFilter`:
  `{"mode": "only", "entries": ["group:GTR", "LT"]}`. It is parsed and
  validated by the same code as `/Trips/plan` (the same 400s), then
  stored expanded.
- It's declared with `#[serde(default, skip_serializing_if =
  "Option::is_none")]` and placed last in `ItineraryRequest`, so a body
  without it hashes byte for byte as before. That keeps idempotency keys
  across the deploy, the rule the struct's doc spells out.
- `/plan`'s save (`tripPlanSave.ts`) sends the filter from the
  **response echo** (what the plan was made with), not the form.
- `PATCH /Journeys/{id}` accepts `operatorFilter` (an object, or `null`
  to clear it). The journey page shows "Planned using only Avanti West
  Coast · Change", and changing it re-runs nothing until the next
  replan.
- `GET` of a journey echoes `operatorFilter` (null when none).
- Templates made from a journey don't copy the filter in this design
  (proposed default; a later column on `journey_templates`).

**Read path (replans).**

- `journey_replan`'s journey read (the "journey as the break logic sees
  it" query) also selects the three columns.
- `ReplanQuery` replaces `allowed_uids` with `operator_rule:
  Option<OperatorFilter>` (mode plus codes). `plan_for_journey` resolves
  the codes to that date's `OperatorId`s on the graph (a code not running
  that day matches nothing) and builds the `OperatorRule`.
- The searches then follow §5.6: Only and Avoid on every search, with
  the ticket's "same operator" search intersected; Prefer as one extra
  labelled search; and the unfiltered fallback when nothing is left.
- Break suggestions are stored from the same replan, so the open break,
  `replan/preview` and the sweep (`ReplanSlot::Sweep`) all honour the
  filter with no extra code. A stored suggestion carries
  `outsideOperatorFilter` when it is the fallback.

## 7. Performance

### 7.1 Method

The api's memory and latency bench (`transit_planning_budget`,
`DS_RUN_BUDGET_BENCH`) was not run for this spec. The filter adds no
connections, the ids fit in existing padding, and the table's memory is
computed in §6.1. P0 adds the table to that bench.

`crates/trip-planner/tests/bench_arrive_by.rs`'s synthetic day: 27,892
trains, 400,995 connections, 2,000 stations. Each line's trains were
given one of ten operators (`line % 10`), and 20 random pairs were
planned (`bench_options_cpu`'s seed). A throwaway prototype of each
variant went into `Restrictions::blocks` (not committed):

- **uidset (sip)**: today's `allowed_uids` `HashSet<String>`;
- **map (sip)** / **map (fx)**: a graph map UID → operator id with std
  SipHash or an Fx hasher;
- **field**: the id read from the connection.

The box was heavily loaded (load average 20 to 40 on 6 cores), so CPU
time was too noisy: medians moved ±30% between identical runs. The
figures below are **callgrind instruction counts** (one repetition, the
graph build subtracted). They are deterministic and comparable. Searches
as `bench_fastest_cpu` and `bench_options_cpu`:

- CSA depart-after 08:00;
- CSA arrive-by 18:00;
- RAPTOR 6 rounds with `maxChanges` 4;
- options = RAPTOR plus `diversify`.

The synthetic day has no fixed links, so link filtering (§6.3) is not
in these numbers. It is one array read per link visited, on a few
thousand links against hundreds of thousands of connections. The P0
bench measures it with `bench_fixed_links_both_ways`' synthetic links.

### 7.2 Results

Instructions, millions, for all 20 queries (found = queries with an
answer, or options in all):

| Filter | Lookup | CSA depart | Arrive-by | RAPTOR | Options |
|---|---|---:|---:|---:|---:|
| none | – | 555 (19) | 1,677 | 28,033 (39) | 30,476 (61) |
| no rule, empty `Restrictions` | – | 705 (+27%) | 1,920 (+14%) | 31,612 (+13%) | 34,050 (+12%) |
| avoid 1 of 10 | uidset (sip) | 931 (+68%) | 2,183 (+30%) | – | – |
| | map (sip) | 944 (+70%) | 2,200 (+31%) | 33,785 (+21%) | – |
| | map (fx) | 828 (+49%) | 2,032 (+21%) | 31,801 (+13%) | 35,218 (+16%) |
| | **field** | **692 (+25%)** | **1,847 (+10%)** | **29,503 (+5%)** | **32,582 (+7%)** |
| only 5 of 10 | uidset (sip) | 1,142 (18) | 2,155 | – | – |
| | map (sip) | 1,173 | 2,197 | – | – |
| | map (fx) | 995 | 1,957 | 26,220 (−6%) | – |
| | **field** | **788** | **1,680** | **23,336 (−17%)** | – |
| only 1 of 10 | uidset (sip) | 2,657 (5) | 2,644 | – | – |
| | map (sip) | 2,865 | 2,802 | – | – |
| | map (fx) | 2,189 | 2,306 | 12,585 (−55%) | – |
| | **field** | **1,407 (5)** | **2,031** | **9,835 (−65%)** | **10,270 (−66%)** (12) |

Reading it:

- **The rule itself is free with the field.** Avoid with the field is at
  or below the "empty `Restrictions`" row. Blocking 10% of trains saves
  about what the check costs. The remaining +25% / +10% / +5% over
  "none" is the existing station-rule path (`str::trim` in
  `normalize_tiploc`: 157M instructions in the CSA profile, the whole
  150M difference). P0's fast path removes it, and it already taxes
  every `avoid`, `via` and alternatives search.
- **Any UID lookup is not free.** Over the field, SipHash adds 19% to
  36% on a Connection Scan and Fx 10% to 20%; on RAPTOR, 15% and 8%.
- **"Only" is cheaper for RAPTOR and options** (fewer trips per round,
  fewer rounds). A narrow include on a **Connection Scan can cost more**:
  with one operator, 15 of 20 pairs had no route, and a scan with no
  answer can't stop early, so it runs to the end of the day. That's
  +153% on CSA with the field (2.5 times the instructions), about 70M
  instructions per query, and it is the worst case. Real operators are
  regional, not uniform, so production runs mostly between this and
  "only 5 of 10". Leaving the Tube out (§5.1) makes more London trips
  fail, so this case will be more common than the first draft assumed.
- **Prefer's extra search** is one restricted Connection Scan: 70M
  (field, depart) to 102M (arrive-by) instructions per query, against an
  options query's 1,524M. That's **+5% to +7%** per segment.
- **Empty-segment explanations** add at most three restricted scans,
  only when a segment is empty.

### 7.3 Budget

The budget for P1 to P3 is checked by a new `bench_operator_filter_cpu`
in `bench_arrive_by.rs` (gated like the others and run by
`scripts/check-bench-gates.py`), measured against the same request
without an operator parameter, after P0's fast path:

| Case | Budget |
|---|---|
| avoid, any search | ≤ +5% CPU |
| only (≥ half the operators) | ≤ +5% CPU on arrive-by and RAPTOR; ≤ 1.5x on CSA (a pair left with no route scans to the end of the day; measured 1.12x) |
| only (one operator), `fastest` | ≤ 3× CPU (bounded by one full-day scan) |
| link filtering, with the synthetic links | ≤ +2% CPU |
| `preferOperator`, options | ≤ +10% CPU |
| empty segment with a filter | ≤ 4 Connection Scans (the search and three explanations) |
| options search-size guard | unchanged: the filter adds no rounds |

Wall time on production is re-checked as in the max-changes spec, on a
read-only copy of one weekday. Use 10 real pairs per mode, including:

- "Only LNER" Edinburgh to King's Cross (should be as fast);
- "Only Northern" Leeds to Penzance (should explain the failure
  quickly);
- "Only Avanti West Coast and Southern" Manchester to Brighton (should
  name London Underground);
- "Avoid London Underground" Reading to Stratford (should cross on the
  Elizabeth line).

## 8. Implications and risks

### 8.1 Results that look worse

An include filter can return a route that is slower, has more changes,
walks across London or uses a bus. That's what was asked for, but it
reads like a bad planner. Mitigations:

- the banner (§4.4) on every filtered result;
- operator names on legs and networks on transfers, so the reason is
  visible;
- the `operator` no-result reason, with `linkOperators` and the "Add
  London Underground" button;
- Prefer as the gentler option, which shows the operator route next to
  the others.

### 8.2 Consequences of "every way of riding it"

- **"Only these" without London Underground loses the Tube**, which is
  most London crossings. No Tube pair has a timetable walk link, and the
  estimated walks (§5.1) cover only the pairs within 30 minutes on foot:
  27 of the 258 measurable today, not Euston to Victoria, King's Cross
  to Liverpool Street or Paddington to Liverpool Street (§3.2). Expect
  many more "no route" answers for trips through London, and 20 to
  30 minute walks where one is offered. The quick-add button, the banner
  wording and the reason that names London Underground are there for
  this.
- **"Avoid London Underground"** makes London crossings walking-only
  unless another operator's train does the job. A `TfL` stop end keeps
  only its walk access, so Uxbridge as an end usually fails. That's the
  stated intent.
- **Metrolink and NET** follow the same rule: "Only Northern" from a
  Metrolink stop fails unless `ds:METROLINK` is added.
- **Unattributed links still pass** (local `BUS`, two `FERRY`, `TRAM`
  BPN–QBN, the Luton airport shuttle, and the 90 non-Tube `TRANSFER`
  pairs such as BFR–CHX and MAN–MCV, whose mode the timetable doesn't
  say). A user with "Only Northern" might still be given a Manchester
  `TRANSFER` that people often make by tram. It is labelled
  "Transfer", it is rare, and the CSV can attribute such pairs later.
- **Replans honour a filter set days earlier.** The fallback in §5.6
  keeps a disrupted traveller from getting nothing; the filter shows on
  the journey page with a way to clear it.

### 8.3 Other confusion

- **Buses under "Only LNER"**: labelled as LNER buses (§4.4).
- **Codes vs brands**: picking "West Midlands Railway" is LNR as well
  (one code), and the label says so (§4.2).
- **`LT` trains vs the Tube**: no longer a distinction users need to
  know about. One choice covers both.

### 8.4 Performance and abuse

An unauthenticated client can ask for "only one operator" on far-apart
stations, and each segment scans the whole day. That's bounded: one full
scan (about 70M instructions, the same as today's worst unreachable
pair) plus at most three explanation scans. The existing `PLAN_SLOTS` and
search-size guard still apply, so there's no new limit.

### 8.5 Analytics (types only)

In `frontend/lib/usageEvents.ts`:

- a new feature `plan_filters: ['operator_only', 'operator_avoid',
  'operator_prefer', 'operator_add_underground']`, sent once per search
  with that mode (and when the quick-add button is used). Kinds only,
  never codes, like `trains_filters`;
- `compare_options` gains `'operator_option_selected'`;
- the matching server-side allow-list entries.

No codes, groups or stations are sent.

### 8.6 The add-leg dialog's operator filter

That redesign (`JourneyLegCandidates`) is planned separately. This
design gives it:

- `GET /Trips/operator-groups`, the `ds:` operators and the alias search
  text (§4.2), so both pickers offer the same groups and names;
- the rule that bus and ferry legs carry their operator (§6.1);
- one shared picker component: P2 builds `OperatorMultiSelect`
  (`operatorChoices` + groups + DS operators + aliases), which the
  dialog can adopt instead of the single-code `OperatorField`.

They must not share state. The dialog filters candidate trains for one
leg; the planner filters a whole route. A journey's stored filter
(§6.9) could seed the dialog's filter later; not in this design.

### 8.7 Caching and SEO

None. `/Trips/plan` isn't cached and `/plan` result pages aren't indexed.

## 9. Phases and effort

| Phase | What | Effort |
|---|---|---|
| **P0** Index and fixes | `OperatorTable` on `PlanningGraph` from `schedule_services`; `Connection.operator`; `FixedLink.operator` and `is_walk` with `fixed-link-operators.csv` and the Tube-pair `TRANSFER` rule; transit trips' DS operators; `OperatorRule` replacing `allowed_uids`; the replan's "same operator" search on the table (buses and ferries kept, transit attributed); leg `operator` from the table; the `blocks` fast path; extend `transit_planning_budget` with the table; the new bench (§7.3). No API or UI change apart from the leg operator fix. | 1.5 days |
| **P1** API | The three parameters, `ds:` operators, validation and 400s; `operator-groups.csv` and `/Trips/operator-groups`; `allows_link` in the four link loops; the estimated walks for `LT` pairs; access rides attributed by line; echo fields; `operatorsNotRunning`; transfer `operator`; `explain_empty_segment` order, the two reasons and `linkOperators`; metrics; changelog. | 2.5 days |
| **P2** `/plan` | The Operators field (mode, `OperatorMultiSelect`, DS operators, aliases, "Add London Underground"), URL round-trip, summary and count, client-side errors, leg operator names, the filter banner, `noResultHint` cases, analytics types, a11y checks, e2e for one filtered plan. | 2 days |
| **P3** Prefer | `preferOperator`, the extra scan and keep rule (§5.4), `operatorOnly` label, `preferOperatorResult`, the label's rendering. | 1 day |
| **P4** DS-MCP | `plan_journey` inputs, description, rendering and tests (separate repo and PR). | 0.5 day |
| **P5** Saved journeys | The migration pair; `operatorFilter` on `POST /Journeys/itinerary` (hash-stable), `PATCH` and `GET`; `tripPlanSave` sends the echo; the journey page line; `ReplanQuery.operator_rule`; the ticket intersection, the Prefer search and the unfiltered fallback with `outsideOperatorFilter`; changelog. | 1.5 days |

Total about 9 days. P0 is useful alone (it fixes two bugs and speeds up
every avoid and via search) and can merge first. P1 and P2 must both
land before the UI exposes anything. P3, P4 and P5 depend only on P1,
not on each other. P5 needs P2's save change to have anything to store.

## 10. Tests

- `restrictions.rs`: Only, Avoid and UNKNOWN pass; transit DS operators;
  a blocked connection ends the ride (the existing pattern);
  `allows_link` for `TUBE`, a Tube-pair and a non-Tube `TRANSFER`,
  attributed and unattributed `METRO`, walks, and an estimated walk
  (used only when `LT` is left out); `with_extra` keeps the rule;
  `without_operator_rule` and `with_links_unrestricted`.
- `trip-planner/tests/restricted_calls.rs`-style integration:
  - an include filter rides two allowed trains with a walk between them;
  - an include filter without `LT` refuses a `TUBE` link and its
    `TRANSFER` twin and takes the estimated walk, or finds nothing when
    the walk is over 30 minutes;
  - an include filter with `LT` takes the `TUBE` link;
  - "avoid `LT`" drops `LT` trains, `TUBE` links and Tube-pair
    `TRANSFER` links;
  - with no filter, the estimated walks are never used;
  - arrive-by, staged waypoints and vias honour the rule, links
    included;
  - `diversify` alternatives obey it.
- API DB tests:
  - each 400, group and `ds:` expansion, and the echo;
  - `operatorsNotRunning`;
  - each new reason, `linkOperators` (an Avanti-and-Southern trip that
    needs EUS–VIC by Tube) and the `avoidCombined` widening
    (station-only, operator-only, both);
  - a bus leg carries `operator`, and a Tube transfer carries `LT`;
  - a `TfL` stop end under "avoid `LT`" keeps only walk access;
  - the replan "same operator" search keeps a bus of that operator.
- Journeys DB tests: the migration's checks; `operatorFilter` round-trip
  through `POST /Journeys/itinerary`, `PATCH` and `GET`; a body without
  it hashes as before (`a_train_leg_hashes_as_it_did_before_untracked_legs`
  style); a replan honours Only and Avoid, intersects the ticket search,
  adds the Prefer search, and falls back with `outsideOperatorFilter`
  when nothing complies.
- Frontend unit: URL parse and serialise (`ds:` included); summary text;
  `noResultHint` with and without `linkOperators`; label text for one
  code, two codes and a group; leg operator line; the quick-add button.
- e2e: one "Only" plan, its banner and leg names; one empty result with
  the "Add London Underground" hint.

## 11. Decisions and proposed defaults

**Decided by the user, 2026-10-31:**

1. **Index placement:** the operator id rides on `Connection`, built
   from the graph's table (§6.2).
2. **A filter on an operator covers every way of riding it.** "Avoid
   London Underground" avoids the Tube entirely, and "Only these"
   without London Underground means no Tube. The same holds for the DLR,
   London Trams, Metrolink and NET (§5.1). The Elizabeth line and the
   Overground already ride only as CIF trains (§3.2).
3. **Prefer's keep bound:** within twice the alternatives' detour
   allowance (§5.4).
4. **Prefer with Fastest:** disabled, with a note (§4.2); a 400 in the
   API.
5. **Saved journeys** remember the plan's operator filter, and their
   replans and break suggestions honour it (§5.6, §6.9).

**Proposed defaults** (applied in this design; say if any should
change). Items 11, 13 and 14 were approved on 2026-10-31 and are marked
so:

6. **No "faster with other operators" note** on a filtered Fastest
   result for now. It costs one more search (about +5%). Revisit with
   the `plan_filters` analytics.
7. **One filter per request**, not per waypoint segment.
8. **Groups:** `GTR` (decided), `TFL` = LO, XR, LT, `ds:DLR`,
   `ds:LONDON_TRAMS`, and `SWR` = SW, IL.
9. **Display names for unnamed codes:** `TW` "Tyne and Wear Metro" and
   `QC` "CalMac ferries", from DS reference data. **`LF` is
   unidentified.** The plan to identify it, in P1 and from timetable data
   only:
   - list its schedules over the 8-day window from `schedule_services`
     and the calling points (routes, days, `train_category`,
     `train_status`, headcodes and retail service IDs);
   - compare its pattern with the open-access and charter services the
     timetable already names;
   - record the finding in the reference data with the evidence.

   Done in P1 (see "Errata from the build"): Grand Union. Before that,
   the picker would have shown the bare code "LF", as the Trains search
   does.
10. **The Trains search** can use the planner's `running` flag and
    aliases, so "West Midlands Railway" stops matching nothing on
    `/trains`. This is offered to whoever owns `TrainSearchForm`; it is
    not part of these phases.
11. **The replan fallback** (§5.6): when the filter leaves no suggestion
    in a disruption, show the best unfiltered one, flagged "Uses other
    operators", so a stored preference never strands a traveller.
    **Approved by the user, 2026-10-31.**
12. **Templates** made from a filtered journey don't copy the filter yet.
13. **`TRANSFER` links on Tube pairs count as London Underground.** The
    timetable doesn't give their mode, but they're Tube-speed (EUS–VIC
    19 minutes against about 62 on foot), and 140 of the 230 `TRANSFER`
    pairs are Tube pairs. The other 90 stay unattributed (§3.2).
    **Approved by the user, 2026-10-31.**
14. **Estimated walks replace the Tube when it's left out**, up to 30
    minutes on foot (§5.1). Without them, "no Tube" would mean no way
    between most London termini, because the timetable has no walk rows
    there. **Approved by the user, 2026-10-31.**
