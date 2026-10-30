# `/plan`: filtering by operator in "Plan a route for me"

Design, 2026-10-31. Status: **proposed, research and spec only** (DS-214).
No feature code ships with this document. It covers `GET /Trips/plan`,
the `/plan` page's "Plan a route for me" mode, and DS-MCP's
`plan_journey`. "§" alone means a section of this document.

The user decided on 2026-10-30:

- **Both directions:** "only these operators" (include) and "avoid these
  operators".
- **Codes, not modes:** filtering an operator also covers its own buses,
  replacement buses and ferries, which carry the same ATOC code.
- **Brand groups** in the picker, e.g. GTR = Thameslink, Great Northern,
  Southern and Gatwick Express.
- **"Prefer an operator"** is a labelled option in Compare options
  ("Avanti West Coast only"), not a tie-break.
- Defaults left to this design, kept here: a train with no known operator
  passes an include filter; London Underground (`LT`) is selectable; the
  operator index lives on the planning graph. §6.2 refines the last one
  with a measurement and asks the user to confirm (§11, Q1).

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
11. [Open questions for the user](#11-open-questions-for-the-user)

## 1. Summary

- **Control.** One "Operators" field in `/plan`'s Advanced options: a
  choice of *Only these*, *Avoid these* or *Prefer* (Compare options
  only), and a multi-select of operators and brand groups. The picker is
  the Trains search's (`operatorChoices`), with groups added.
- **API.** Three mutually exclusive list parameters on `GET /Trips/plan`:
  `operator` (only these), `avoidOperator` and `preferOperator`. Each
  takes comma-separated ATOC codes or `group:NAME` entries. A new
  `GET /Trips/operator-groups` endpoint lists the groups. The response
  echoes the codes as applied, plus `operatorGroups`.
- **Rule.** The filter is checked inside every search, through the
  existing `Restrictions::blocks` path, as `avoid` is. It applies to
  National Rail timetable connections only (CIF trains, buses and
  ferries). Walks, fixed links (Tube and bus links included), `TfL`
  access legs and transit trips (trams, and the Tube after TfL P4) always
  pass. So does a train with no known operator, in either direction.
- **Index.** `PlanningGraph` gains an operator table built from
  `schedule_services.operator_atoc`, which covers every graph UID. The
  current source, `schedule_destination_departures`, misses about 850
  buses and ferries a day. Each `Connection` carries a 2-byte operator
  id. A UID map in the search hot path costs 8% to 36% more
  instructions than the id, which costs nothing measurable (§7).
- **Empty results explain the filter.** There are two new
  `noResultReason` constraints, `operator` and `avoidOperator`, e.g. "No
  journey using only Avanti West Coast departs after 18:00; one exists
  with other operators."
- **Prefer** adds one option labelled `{"kind": "operatorOnly"}` ("Avanti
  West Coast only") to Compare options. It costs at most one extra
  restricted Connection Scan per segment, about +5% of an options request.
- **Fixes on the way (P0):** the journey replan's "same operator" search
  takes its UIDs from the graph's table, which keeps the operator's own
  buses and ferries and, if replans ever plan with transit, never blocks
  trams. Leg `operator` is read from the same table, so bus and ferry
  legs stop showing no operator. `Restrictions::blocks` stops trimming
  TIPLOCs it doesn't need to, which currently costs +27% instructions on every
  restricted Connection Scan.
- **Phases:** P0 index and fixes (1 day), P1 API (2 days), P2 `/plan` UI
  (2 days), P3 Prefer (1 day), P4 DS-MCP (0.5 day). Total about 6.5 days.

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

### 2.2 Tickets

- `alternative_ranking::ticket_constraint` turns tracked tickets into
  `OperatorOnly` (a ticket type containing "only") or `TrainSpecific`
  (Advance).
- `delay_repay_rules::atoc_for_operator_text` maps a ticket's operator
  text to an ATOC code.
- Both serve the mid-journey replans (mid-journey changes spec §17), not
  `/Trips/plan`.

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
  (Sunderland line), `QC` runs ferries (e.g. Ullapool to Stornoway), and
  `LF` runs a few long-distance trains via the West Coast Main Line. §4.2
  proposes display names for them, kept in DS's own reference data.
- **`tocs` has codes the timetable never uses.** These are `LN` (London
  Northwestern), `WM` (West Midlands Railway), `SX` (Stansted Express),
  `ES`, `HS`, `NR`, `ZN`, `GM`, `HV`, `XP` and `XS`. LNR and WMR both run
  as `LM` ("LNR & WMR"), and Stansted Express runs as `LE`. A picker built
  from `tocs` alone would offer "West Midlands Railway", and that choice
  matches nothing. §4.2 handles this with aliases.
- **`LT` is CIF Underground running on Network Rail metals.** For
  example, `C61095` runs the Bakerloo line from Harrow & Wealdstone to
  Elephant & Castle. That's about 435 trains a day. It isn't the Tube
  network: Tube links are fixed links (`TUBE` mode) and `TfL` access
  legs, and after TfL P4 they become transit trips.
- **`IL` (Island Line)** is all `replacement_bus` in this week's timetable
  (engineering works). "Only Island Line" would return its buses, which
  is correct.

### 3.2 Associations

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
  [ GTR (Thameslink, Great Northern, Southern, Gatwick Express) x ] [ Avanti West Coast x ]
  Only routes on these operators' trains, buses and ferries. Walks and the
  Underground between stations are always allowed.
```

- **Mode.** A `SegmentedControl` (radios underneath, keyboard-operable).
  With *Any* the picker is hidden. Switching mode keeps the picks.
- **Description**, per mode:
  - *Only these:* "Only routes on these operators' trains, buses and
    ferries. Walks and the Underground between stations are always
    allowed."
  - *Avoid these:* "Never use these operators' trains, buses or ferries."
  - *Prefer:* "Compare options adds the best route using only these
    operators, labelled." With Results set to Fastest, the field shows
    "Prefer needs Compare options." and is disabled (Q4).
- **Picker.** A `MultiSelect`, searchable and clearable, at most 8
  entries (the avoid lists' `MAX_AVOIDED`). Options:
  1. **Groups first**, from `GET /Trips/operator-groups`: "GTR
     (Thameslink, Great Northern, Southern, Gatwick Express)". Sent as
     `group:GTR`, like station groups.
  2. **Every code running in the published window** (the planner's 8
     days), by name: `operatorChoices` over `tocs` names plus unnamed
     codes. A code that is in `tocs` but has no trains in the window
     (`ES`, `NR` and so on) is left out.
  3. **Aliases.** Typing "West Midlands Railway" or "London
     Northwestern" finds "LM · LNR & WMR (West Midlands Railway and London
     Northwestern Railway)". Typing "Stansted Express" finds "LE ·
     Greater Anglia (includes Stansted Express)". An alias is search text
     only, never a separate value, so a user can't pick a code that
     matches nothing. The same applies to the three unnamed codes: `TW`
     as "Tyne and Wear Metro", `QC` as "CalMac ferries" and `LF` (to
     confirm, Q8).
- **Initial groups** (`reference-data/operator-groups.csv`, compiled in
  like `station-groups.csv`):

  | Group | Codes | Why |
  |---|---|---|
  | `GTR` | TL, GN, SN, GX | One company, four brands, shared tickets on many flows (user decision) |
  | `TFL_RAIL` | LO, XR, LT | "TfL" services in the National Rail timetable |
  | `SWR` | SW, IL | Island Line is run by SWR (Q7) |

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
(`PlanFormInitial`) accepts 2-character alphanumeric codes and
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
  as the Trains search does.
- **A filter banner** above the results whenever a filter applied, from
  the response echo, not the form: "Showing routes using only Avanti
  West Coast. Walks and the Underground between stations are included."
  or "Showing routes that avoid GTR." It has a "Change" link that opens
  Advanced options. When a Prefer option was added, there is no banner;
  the option's label says it.
- **Empty or no-route states** (`noResultHint`):
  - `operator`: "No journey using only Avanti West Coast departs after
    18:00 on Tue 3 Nov; one exists with other operators." Hint: "Try
    adding operators, or remove the operator filter in Advanced
    options."
  - `operator` with `values` empty and a message saying no trains of
    that operator run that day: "Avanti West Coast runs no trains on Sun
    1 Nov." (§5.9)
  - `avoidOperator`: "No journey avoiding GTR …; one exists using them."
    Hint: "Try removing GTR from Avoid operators."
  - `avoidCombined` grows to cover an operator filter that only fails
    together with avoid lists (§6.5): "Try removing some of the
    stations or operators in Advanced options: together they rule out
    every route."
- **Compare options labels:** `operatorOnly` renders "Avanti West Coast
  only" (or "GTR only" for a group, "Avanti West Coast or LNER only" for
  two entries). It uses the neutral tone, like `alternative`.
- **Fastest with a filter** returns the fastest compliant route. It
  doesn't show a "faster with other operators" note in P2, which would
  cost a second search (Q5).

### 4.5 Accessibility

- The mode is a labelled radio group ("Operators") with a visible
  description, and the picker's `aria-describedby` points at it.
- The banner is plain text in the results region, announced with the
  results (no extra live region). Its "Change" link is a real link that
  opens the disclosure and moves focus to the mode control.
- The leg operator text sits in the leg's existing text, not in colour
  or an icon alone. The `operatorOnly` badge text is the full label.
- The `MultiSelect` keeps the Trains search's clear-button label ("Clear
  operator filter") and its 24px hit area (`iconHitArea24`).

## 5. Semantics and edge cases

### 5.1 What a filter looks at

Only **connections built from the CIF timetable**. Each has the
schedule's `operator_atoc` from `schedule_services`. That means trains,
replacement buses, buses and ferries, all by code.

**Always allowed**, in every mode:

- walks, including same-CRS platform changes and the walk legs of
  `alight_then_walk`;
- fixed links of every mode (the links table's `TUBE` 1,909 rows,
  `METRO` 1,776, `TRANSFER` 661, `WALK` 249, `BUS` 57, `TRAM` 4 and
  `FERRY` 2). These are the timetable's own "allow this long to get
  across" links, not services with an operator;
- `TfL` access legs (DS-135; the timed Tube rides from #80/#82);
- transit trips (`is_transit_uid`): Metrolink and NET trams now, and the
  TfL network's Tube, DLR and trams after TfL P4.

So "Only Avanti West Coast" from Manchester Piccadilly to Uxbridge may
ride Avanti to Euston and then use the `TfL` access leg. With only
Avanti trains to ride, this is a train-plus-Tube route by design. The
banner says so ("Walks and the Underground between stations are
included").

**Unknown operator** (no `schedule_services` row: today never, but a
schedule-reference publish can lag): passes both include and avoid. The
answer is "probably fine" rather than a silent hole. This is the user's
default. It is counted in a metric (§6.7) so a lag shows up.

### 5.2 Include ("Only these")

A connection whose code is not in the list is blocked. Changing between
two allowed trains is allowed whatever lies between: a walk, a Tube link
or an access leg. A blocked connection ends the ride (§2.1), so a train
that changes operator mid-schedule couldn't be ridden through, but
schedules never change code (§3.2).

### 5.3 Avoid

A connection whose code is in the list is blocked. Nothing else changes.

### 5.4 Prefer

Prefer is not a filter. The normal search runs unfiltered. Then, with
Compare options:

1. If a kept option already rides only preferred operators, it gets the
   `operatorOnly` label too. No extra search runs.
2. Otherwise one extra search runs with the include rule: a restricted
   Connection Scan in the request's direction, the same cost as one
   `diversify` attempt. For a waypoint plan, it is the staged scan.
3. The result is kept if it arrives (arrive-by: leaves) within **twice**
   the alternatives' detour allowance (`allowance_minutes`, so 40 to 180
   minutes) and within `maxChanges`. If it is kept and the list is full
   (`MAX_OPTIONS` = 5), it replaces the last alternative. A Prefer the
   user asked for outranks an alternative the user didn't ask for.
4. When nothing qualifies, the response carries `preferOperatorResult:
   "none"` or `"tooSlow"`. The UI then says "No Avanti West Coast only
   route within 2 hours of these." (Q3 asks about the bound.)

Prefer never hides or reorders the other options. That is the user's "not
a tie-break".

### 5.5 Combinations

| With | Behaviour |
|---|---|
| `via`, `avoid`, `avoidStop`, `avoidChange` | All apply together (AND). A via is only satisfied by a train actually ridden, so under "Only X" a via needs an X train (or a walk) through it. |
| `waypoints` | The filter applies to every segment. It is per request, not per segment (Q6). |
| `maxChanges` | Unchanged. An include filter often needs more changes. `cappedByMaxChanges` and the `maxChanges` reason fire as today, and that check runs before the operator blame (§6.5). |
| `arriveBy` | The reverse scans check the same `blocks` rule. |
| `dayOffset`, `acrossMidnight` | Each date's graph has its own table. A code is matched on both dates' trains. |
| Compare options alternatives | `diversify` adds its bans on top of the request's restrictions, so every alternative obeys the filter too. With an include filter there are usually fewer alternatives (§7). Labels are unchanged. |
| Earlier/later routes paging | It re-sends the same query, so the filter carries over. |
| Live overlay | Replacement connections keep the base UID, and so the operator. A filtered train that is cancelled or late is handled exactly as unfiltered. The in-search live delays (DS-140) use the same restrictions. |
| `transit=true`, tram or `TfL` stop ends | Transit trips pass (§5.1). |

### 5.6 Saved journeys and replans

- A journey saved from a filtered plan **doesn't record the filter** in
  P1 to P4. Its mid-journey alternatives are ranked by the tickets, as
  today. If the user has said "only LNER" because that's their ticket,
  the ticket logic already covers it (`OperatorOnly`). Storing the filter
  on the revision is Q9.
- The replan's own "same operator" search switches to the graph table
  (P0) and stops blocking transit trips.

### 5.7 Tickets

The filter selects **services, not fares**. Three caveats show in the
field's help text, behind an "About tickets" disclosure:

- **"Only these" is not "valid on my ticket".** An "LNER only" ticket
  also limits route and time. An Advance is valid on one train
  (`TrainSpecific`). A route of only LNER trains may still be off-route
  for the ticket.
- **Cross-London.** A through ticket usually includes the Underground
  between London termini. That matches "walks and the Underground are
  always allowed". A route that rides an `LT` *train* (the Bakerloo
  north of Queen's Park) under "Only LNER" is blocked, which is right:
  that's a train, not the transfer.
- **Avoid** isn't a "Not via" or "Not Avanti" ticket route. It doesn't
  touch stations.

Later (not in this design's phases): prefill the filter from a tracked
ticket (`ticket_constraint` → "Only LNER") when planning a return.

### 5.8 Brand groups

A group is expanded to its codes before validation and search. `GTR` in
"Only" means any of TL, GN, SN or GX. Groups can mix with codes
(`group:GTR,VT`). Codes are deduplicated.

### 5.9 Codes over time

- Matching is on the date's own timetable. A code that stops running
  (a franchise change at a timetable change) simply has no trains on
  later dates.
- **Validation (400)** accepts a code when it is in `tocs` or runs a
  schedule on any searched date. This is the Trains search's
  `known_operator_codes` rule, so a typo is rejected. A known code with no
  trains on the date is **not** a 400:
  - under *Only*, the segment's reason is `operator`, with the message
    "Avanti West Coast runs no trains on Sun 1 Nov";
  - under *Avoid*, it does nothing.
  - The API also echoes it in `operatorsNotRunning` so the UI can say so
    once.
- `operator-groups.csv` and the aliases (§4.2) are DS data, reviewed when
  a new code appears. The Trains search operators endpoint already lists
  unnamed codes, so a new code is visible there first.

## 6. Planner and API design

### 6.1 The operator table on the graph

`PlanningGraph` gains:

```rust
pub struct OperatorTable {
    /// Index = `OperatorId`; the ATOC code (`"VT"`).
    codes: Vec<[u8; 2]>,
    /// The date's schedules: UID -> operator id.
    by_uid: HashMap<String, OperatorId>,
}
pub struct OperatorId(u16); // OperatorId::UNKNOWN = u16::MAX
```

- **Source.** `SELECT uid, operator_atoc FROM schedule_services WHERE
  service_date = $1`, read in `fetch_calling_points_for_date`'s
  transaction. That is about 26k small rows. A missing row or NULL code
  is `UNKNOWN`.
- **Users.** The searches (via the per-connection id, §6.2), leg
  details (`operator` from the table instead of
  `schedule_destination_departures`: fixes §2.1's missing buses and
  ferries), the replan's "same operator" search, validation (which codes
  run on the date), and `operatorsNotRunning`.
- **Memory.** About 26k entries at roughly 70 bytes is about 2 MB per
  cached date, 4 MB for two. Against the TfL spec's +60 MB budget this is
  small. The extension of `transit_planning_budget` in P0 measures it.

### 6.2 The per-connection id (deviation from "map on the graph")

`Connection` gains `operator: OperatorId`, set by `build_connections`
from the table. Transit connections get `UNKNOWN`. The field fits in the
struct's existing padding after the two `bool`s, so memory doesn't
change. There are 57 `Connection { .. }` literals, mostly in tests; they
get `operator: OperatorId::UNKNOWN`.

**Why not look up the UID in the graph map, as decided on 2026-10-30?**
§7 measured it. On the synthetic day, a per-connection UID lookup costs
**+36% (SipHash) or +20% (Fx) instructions** over the id on an
avoid-one-of-ten Connection Scan, and +15% or +8% on RAPTOR. The id costs nothing beyond the
`Restrictions` path itself. The table still lives on the graph; only the
hot-path read moves onto the connection. If TfL P4 interns `Connection`
codes into trip ids first, the id can live in the trip table instead.
Same cost, same API. The user confirms in Q1.

### 6.3 The `Restrictions` rule

```rust
pub enum OperatorRule {
    /// Only connections whose operator is allowed (UNKNOWN passes).
    Only(OperatorMask),
    /// Not connections whose operator is in the mask (UNKNOWN passes).
    Avoid(OperatorMask),
}
pub struct OperatorMask(Box<[bool]>); // indexed by OperatorId; or a u64 bitset
impl Restrictions {
    pub fn with_operator_rule(self, rule: OperatorRule) -> Self;
    pub fn operator_rule(&self) -> Option<&OperatorRule>;
    pub fn without_operator_rule(&self) -> Self; // for explain_empty_segment
}
```

- `blocks` checks the rule first: one array read, no hashing.
- **`allowed_uids` is removed.** The replan's "same operator" search
  becomes `OperatorRule::Only` over its replaced operators, built from
  the same graph. Two ways to say "only these trains" would drift apart.
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
| `operator=VT,GR` | Only these operators (include). Same name and syntax as `/public/trains/search`. |
| `avoidOperator=group:GTR` | Never these. |
| `preferOperator=GR` | With `results=options`: add the best route on only these, labelled (§5.4). |

Each takes up to 8 entries, comma-separated, each a 2-character ATOC
code or a `group:NAME` (case-insensitive, trimmed, deduplicated after
expansion). Validation happens before any graph read, so it is cheap:

- **400** for: more than one of the three parameters; a malformed code
  ("operator must be a 2-character ATOC code or group:NAME"); an unknown
  group; an unknown code (§5.9: not in `tocs` and not running on any
  searched date), naming every unknown code as the Trains search does;
  more than 8 entries; `preferOperator` without `results=options`
  ("preferOperator needs results=options"); `alternatives=false` with
  `preferOperator` (Q4).
- `GET /Trips/operator-groups`: `{"groups": [{"code": "GTR", "name":
  "GTR", "members": ["TL", "GN", "SN", "GX"]}]}`, with `Cache-Control:
  public, max-age=3600`, like `/Trips/station-groups`.
- `GET /public/trains/search/operators` stays as it is. The planner's
  picker filters it to codes running in the window. A `running: true`
  flag on that endpoint is additive and also helps the Trains search
  (Q10).

### 6.5 Response

Additive top-level fields, always present (empty when unused), like
`avoid`:

```json
"operator": ["VT"],
"avoidOperator": [],
"preferOperator": [],
"operatorGroups": {"GTR": ["TL", "GN", "SN", "GX"]},
"operatorsNotRunning": ["VT"],
"preferOperatorResult": "added"   // options + preferOperator only: added | labelled | tooSlow | none
```

- **Codes are echoed expanded**, in request order, with groups listed in
  `operatorGroups`. This is the pattern of `stationGroups`.
- **`noResultReason`** gains `operator` and `avoidOperator` (values =
  the codes). `explain_empty_segment`'s order becomes:
  1. `maxChanges`, as today.
  2. With every restriction off, still nothing: fall through to the time
     and route reasons, as today.
  3. With only the operator rule off: blame `operator` or
     `avoidOperator`.
  4. With only the station lists off: blame them as today (`avoid`, the
     single list, or `avoidCombined`).
  5. Both needed: `avoidCombined`, whose `values` now include the
     operator codes and whose message names both.

  That's at most two extra searches, and only for an empty segment, as
  today. Without step 3, the existing code would blame `avoid` for an
  operator filter (§2.1).
- `labels` gains `{"kind": "operatorOnly", "operators": ["GR"]}`.
- Leg `operator` is now set for bus and ferry legs too (§6.1). That's a
  value change from null to a code, so it gets a changelog line.

### 6.6 `docs/api-changelog.md`

One entry, "operator filters on `/Trips/plan`", covering:

- the three parameters and their 400s;
- `/Trips/operator-groups`;
- the echo fields;
- the two reasons and the `avoidCombined` widening;
- the new label kind;
- the fact that bus and ferry legs now carry `operator`;
- that clients which switch on the label kind or reason should treat an
  unknown kind as "other" (DS-MCP and the frontend already do for
  labels).

### 6.7 Observability

- A counter `trip_plan_operator_filter_total{mode}` (`only`, `avoid`,
  `prefer`). Modes only; no codes.
- A counter for connections with an `UNKNOWN` operator at graph build,
  `trip_plan_graph_unknown_operator`. It should be 0. A non-zero value
  means `schedule_services` lags the calling points.

### 6.8 DS-MCP `plan_journey`

- New optional inputs `operators: string[]`, `avoidOperators: string[]`
  and `preferOperators: string[]`. Each is described as ATOC codes or a
  group (e.g. `group:GTR`). They are mutually exclusive, which the MCP
  checks too. They go straight to the API's parameters.
- The rendering adds a line under the summary: "Only Avanti West Coast
  (VT); walks and the Underground between stations included."
- It renders the `operatorOnly` label and the new no-result reasons
  through the existing generic paths.
- A model asking "how do I get there on LNER only?" maps to `operators:
  ["GR"]`. The tool description lists the common codes' names via the
  operator catalogue it already has (DSMCP-26), so the model needn't
  guess codes.

## 7. Performance

### 7.1 Method

The api's memory and latency bench (`transit_planning_budget`,
`DS_RUN_BUDGET_BENCH`) was not run for this spec. The filter adds no
connections, the id fits in existing padding, and the table's memory is
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
  150M difference).
  P0's fast path removes it, and it already taxes every `avoid`, `via`
  and alternatives search.
- **Any UID lookup is not free.** Over the field, SipHash adds 19% to
  36% on a Connection Scan and Fx 10% to 20%; on RAPTOR, 15% and 8%.
- **"Only" is cheaper for RAPTOR and options** (fewer trips per round,
  fewer rounds). A narrow include on a **Connection Scan can cost more**:
  with one operator, 15 of 20 pairs had no route, and a scan with no
  answer can't stop early, so it runs to the end of the day. That's
  +153% on CSA with the field (2.5 times the instructions), about 70M
  instructions per query, and it is the worst case. Real operators are
  regional, not uniform, so production runs mostly between this and
  "only 5 of 10".
- **Prefer's extra search** is one restricted Connection Scan: 70M
  (field, depart) to 102M (arrive-by) instructions per query, against an
  options query's 1,524M. That's **+5% to +7%** per segment.
- **Empty-segment explanations** add at most two restricted scans, only
  when a segment is empty, as today.

### 7.3 Budget

The budget for P1 to P3 is checked by a new `bench_operator_filter_cpu`
in `bench_arrive_by.rs` (gated like the others and run by
`scripts/check-bench-gates.py`), measured against the same request
without an operator parameter, after P0's fast path:

| Case | Budget |
|---|---|
| avoid, any search | ≤ +5% CPU |
| only (≥ half the operators) | ≤ +5% CPU |
| only (one operator), `fastest` | ≤ 3× CPU (bounded by one full-day scan) |
| `preferOperator`, options | ≤ +10% CPU |
| options search-size guard | unchanged: the filter adds no rounds |

Wall time on production is re-checked as in the max-changes spec, on a
read-only copy of one weekday. Use 10 real pairs per mode, including
"Only LNER" Edinburgh to King's Cross (should be as fast) and "Only
Northern" Leeds to Penzance (should explain the failure quickly).

## 8. Implications and risks

### 8.1 Results that look worse

An include filter can return a route that is slower, has more changes,
or uses a bus. That's what was asked for, but it reads like a bad planner.
Mitigations:

- the banner (§4.4) on every filtered result;
- operator names on legs, so the reason is visible;
- the `operator` no-result reason;
- Prefer as the gentler option, which shows the operator route next to
  the others.

### 8.2 Confusion

- **"Only Avanti but I rode the Tube"**: the banner and help text say so
  (§5.1). That's the right default for cross-London trips; the
  alternative is "no route" for almost every London trip.
- **"Avoid London Underground" still uses Tube links and access legs.**
  `LT` covers only `LT` trains in the CIF. Help text under the option
  says "Avoids London Underground trains in the National Rail timetable
  (for example the Bakerloo line north of Queen's Park). Use 'Avoid
  completely' on a station to keep away from it." Whether avoiding `LT`
  should also drop `TUBE` links and access legs is Q2.
- **Buses under "Only LNER"**: labelled as LNER buses (§4.4).
- **Codes vs brands**: picking "West Midlands Railway" is LNR as well
  (one code), and the label says so (§4.2).

### 8.3 Performance and abuse

An unauthenticated client can ask for "only one operator" on far-apart
stations, and each segment scans the whole day. That's bounded: one full
scan (about 70M instructions, the same as today's worst unreachable
pair) plus at most two explanation scans. The existing `PLAN_SLOTS` and
search-size guard still apply, so there's no new limit.

### 8.4 Analytics (types only)

In `frontend/lib/usageEvents.ts`:

- a new feature `plan_filters: ['operator_only', 'operator_avoid',
  'operator_prefer']`, sent once per search with that mode. Kinds only,
  never codes, like `trains_filters`;
- `compare_options` gains `'operator_option_selected'`;
- the matching server-side allow-list entries.

No codes, groups or stations are sent.

### 8.5 Caching and SEO

None. `/Trips/plan` isn't cached and `/plan` result pages aren't indexed.

### 8.6 The add-leg dialog's operator filter

That redesign (`JourneyLegCandidates`) is planned separately. This
design gives it:

- `GET /Trips/operator-groups` and the alias search text (§4.2), so both
  pickers offer the same groups and names;
- the rule that bus and ferry legs carry their operator (§6.1);
- one shared picker component: P2 builds `OperatorMultiSelect`
  (`operatorChoices` + groups + aliases), which the dialog can adopt
  instead of the single-code `OperatorField`.

They must not share state. The dialog filters candidate trains for one
leg; the planner filters a whole route.

## 9. Phases and effort

| Phase | What | Effort |
|---|---|---|
| **P0** Index and fixes | `OperatorTable` on `PlanningGraph` from `schedule_services`; `Connection.operator`; `OperatorRule` replacing `allowed_uids`; the replan's "same operator" search on the table (buses and ferries kept, transit passes); leg `operator` from the table; the `blocks` fast path; extend `transit_planning_budget` with the table; the new bench (§7.3). No API or UI change apart from the leg operator fix. | 1 day |
| **P1** API | The three parameters, validation and 400s; `operator-groups.csv` and `/Trips/operator-groups`; echo fields; `operatorsNotRunning`; `explain_empty_segment` order and the two reasons; metrics; changelog. | 2 days |
| **P2** `/plan` | The Operators field (mode, `OperatorMultiSelect`, aliases), URL round-trip, summary and count, client-side errors, leg operator names, the filter banner, `noResultHint` cases, analytics types, a11y checks, e2e for one filtered plan. | 2 days |
| **P3** Prefer | `preferOperator`, the extra scan and keep rule (§5.4), `operatorOnly` label, `preferOperatorResult`, the label's rendering. | 1 day |
| **P4** DS-MCP | `plan_journey` inputs, rendering and tests (separate repo and PR). | 0.5 day |

Total about 6.5 days. P0 is useful alone (it fixes two bugs and speeds
up every avoid and via search) and can merge first. P1 and P2 must both
land before the UI exposes anything. P3 and P4 are independent of each
other.

## 10. Tests

- `restrictions.rs`: Only, Avoid and UNKNOWN pass; transit UIDs pass; a
  blocked connection ends the ride (the existing pattern); `with_extra`
  keeps the rule; `without_operator_rule`.
- `trip-planner/tests/restricted_calls.rs`-style integration: an include
  filter rides two allowed trains with a walk and with a fixed `TUBE`
  link between them; an avoid filter drops a faster route; arrive-by,
  staged waypoints and vias honour the rule; `diversify` alternatives
  obey it.
- API DB tests: each 400; group expansion and echo; `operatorsNotRunning`;
  each new reason and the `avoidCombined` widening (station-only,
  operator-only, both); a bus leg carries `operator`; the replan
  "same operator" search keeps a bus of that operator.
- Frontend unit: URL parse and serialise; summary text; `noResultHint`;
  label text for one code, two codes and a group; leg operator line.
- e2e: one "Only" plan, its banner and leg names; one empty result with
  the hint.

## 11. Open questions for the user

1. **Index placement.** OK to carry a 2-byte operator id on `Connection`
   (built from the graph's table), rather than looking each UID up in a
   map? The map costs 8% to 36% more instructions per
   restricted search (§7.2).
2. **"Avoid London Underground"**: should it also drop Tube links (fixed
   `TUBE` rows), `TfL` access legs and, after TfL P4, Tube transit trips?
   Recommended: no for now, with help text. Dropping them makes most
   cross-London trips fail.
3. **Prefer's keep bound**: twice the alternatives' allowance (40 to 180
   minutes later), or always show the operator route however slow it
   is, as long as one exists?
4. **Prefer with Fastest**: disable it with a note (recommended), or have
   Prefer switch Results to Compare options?
5. **"Faster with other operators" note** on a filtered Fastest result
   (one more search, about +5%): worth it in P2, later, or never?
6. **Per-segment filters** for waypoint plans (e.g. LNER to York, then
   anything): out of scope for now?
7. **Groups**: is `SWR` = SW + IL wanted, and `TFL_RAIL` = LO + XR + LT?
   Any others (e.g. Northern + TransPennine for a North pass)?
8. **Display names for unnamed codes** `TW`, `QC`, `LF`: OK to keep
   DS-side names ("Tyne and Wear Metro", "CalMac ferries"), and what is
   `LF`?
9. **Saved journeys**: should a journey remember its plan's operator
   filter so its mid-journey alternatives honour it?
10. **Trains search**: adopt the `running` flag and aliases there too, so
    "West Midlands Railway" stops matching nothing on `/trains`? (Another
    agent owns `TrainSearchForm`, so that is their change.)
