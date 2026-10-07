# `/Trips/plan`: OR-choice vias and station groups (2026-10-07)

Requested by the DS-MCP session. Its station resolver has the group code
`LON`, which stands for the eighteen London Terminals with OR semantics:
"through ANY of these stations", never "through all of them". DS's `via`,
`avoid`, `avoidStop` and `waypoints` each took individually named stations
with AND semantics, so DS could not express "pass through at least one of
X, Y, Z". Expanding the group into 18 vias is also over `MAX_VIAS` (3). This
was the last case keeping `plan_journey` on the MCP's local engine.

## API

A `via` entry (vias are still comma-separated, ordered, at most 3) is now
an **OR choice**: one or more alternatives separated by `|`. Each
alternative is:

- a station CRS (`KGX`);
- a bus stop's or ferry terminal's `tiploc:` code; or
- a named group, `group:NAME` (any case), from
  `reference-data/station-groups.csv`.

`via=KGX|EUS|STP` and `via=group:LON` are each ONE via, passed by any of
their stations. `via=group:LON|CBG` mixes the two. `|` may be sent
percent-encoded (`%7C`) or raw.

The avoid lists (`avoid`, `avoidStop`, `avoidChange`) take `group:NAME`
entries, which avoid every member. An avoid list already means "none of
these", so avoiding any of several stations is the same as avoiding them
all, and nothing else is needed for OR avoids. `|` in an avoid list is a
400 rather than silently meaning the same thing as a comma.

`waypoints` does not take groups or `|`; either is a 400 (see "Not done").

### Why both shapes

`|` lets a client name any ad hoc set without DS knowing it. A named group
lets DS own a curated list such as the London Terminals, so a client need
not copy 18 codes and both sides agree on the membership. The response
echoes what a group expanded to.

### Response

- `via`: each via's label as applied: `KGX`, `KGX|EUS`, `group:LON`.
  A single station is unchanged.
- `journeys[j].viaSatisfiedBy[k]` gains `matchedCrs`: the station that
  satisfied the via. That is the member actually passed for a choice, and
  the via itself otherwise. `crs` stays the via as requested (its label).
- `stationGroups`: each group named anywhere in the request (via or avoid
  lists), mapped to its member CRS codes. `{}` when none.
- `avoid`/`avoidStop`/`avoidChange`: as applied, so with groups expanded.
- `noResultReason` with `constraint: "via"` names the label in `values`,
  and its message reads "passes through any of KGX, EUS".

### Semantics and validation

- **Satisfied by any member.** A member satisfies the via on every path a
  single-station via already allowed: a call (staying aboard, boarding or
  alighting), a timing-point pass, a run past a cancelled call, or a
  change or walk there.
- **Origin or destination as a member.** The via is satisfied there, since
  every journey passes it. It is attributed to the first leg (`call`) or
  the last leg. Routeing-guide "via London Terminals" from King's Cross is
  trivially true. A single-station via equal to an end is still a 400, as
  before.
- **Avoided members.** A member in `avoid` is never used; the via is a 400
  only when every station of it is in `avoid`. `avoidStop` and
  `avoidChange` members combine as they do for a single via (e.g. "pass
  without stopping").
- **Duplicates.** The same station set twice in a row (`KGX|EUS,EUS|KGX`)
  is a 400, like a repeated single via. Alternatives repeated within one
  via are kept once.
- An unknown station in a choice is a 400 naming it, and so is an unknown
  group (the message lists the known groups) or an empty alternative
  (`KGX|`).

### Limits

| limit | value | why |
|---|---|---|
| vias | 3 (unchanged) | a choice counts as one via |
| stations in one via | 24 | room for `group:LON` (18) and a few more |
| stations across all vias | 54 | the measured worst case: three 18-station groups |
| avoid-list entries | 8 each (unchanged) | a group counts as one entry |
| options search-size guard | unchanged | a choice is one via in `(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)` |

## Engine

`trip_planner::Vias` already took a TIPLOC set per via, because one CRS
covers several TIPLOCs. An OR choice is therefore the union of its
stations' TIPLOCs, and it is still one step of via progress. The search
state, the dominance rule, RAPTOR rounds and the arrive-by mirror are all
unchanged. The only engine changes are:

- `ViaLeg` gains `tiploc`, the via TIPLOC actually called at, passed or
  walked into (`Vias::hits` reports it).
- A via satisfied at the origin itself is now attributed to the first leg.
  Before, that case could not arise.

`build_vias` (api) takes the codes per via and gives pass spans to every
train that calls at or runs through any member. That is the only cost that
grows with the group's size.

### Measured cost

`bench_group_vias_against_single` (`crates/trip-planner/tests/bench_arrive_by.rs`,
release, synthetic network of 27.9k trains and 401k connections) runs three
vias at the guard's limits. It compares single stations with 18-station
groups. With groups, 13.7k trains carry spans; with single stations,
7.8k do. Medians are over three OD pairs, on a shared machine.

| waypoints / rounds | members | CSA | CSA arrive-by | RAPTOR | arrive-by rounds |
|---|---|---|---|---|---|
| 0 / 8 | 1 | 32 ms | 98 ms | 4.0 s | 1.5 s |
| 0 / 8 | 18 | 28 ms | 102 ms | 3.5 s | 1.6 s |
| 5 / 6 | 1 | 200 ms | 407 ms | 3.4 s | 4.2 s |
| 5 / 6 | 18 | 166 ms | 413 ms | 3.8 s | 4.4 s |
| 3 / 8 | 1 | 103 ms | 289 ms | 4.4 s | 2.9 s |
| 3 / 8 | 18 | 134 ms | 281 ms | 5.7 s | 5.0 s |

A group costs about what a single via does: within noise for most cells,
and up to about 1.7x on the 3-waypoint, 8-round arrive-by. That is well
inside the guard's 2x headroom, so the guard counts a group as one via.

## Station groups (reference data)

`reference-data/station-groups.csv` (`group,crs,name`) is compiled into the
api (`crates/api/src/data/station_groups.rs`, `include_str!`); its unit test
checks every row. It holds one group:

- `LON`: the eighteen London Terminals of the National Rail Routeing Guide
  (fares group NLC 1072): BFR CST CHX CTK EUS FST KGX LST LBG MYB MOG OLD
  PAD STP VXH VIC WAT WAE. This is the same list as DS-MCP's
  `LONDON_TERMINALS`.

The membership is domain knowledge that CIF, CORPUS and MSN cannot provide.
Re-verify it against the Routeing Guide if in doubt. Adding a group is a
CSV row per member. A member that the deployment's station data does not
know makes the group's requests a 400 naming that member: loud rather than
silently narrower.

## Not done (follow-ups)

- **Waypoint groups** ("call at any London terminal", DS-MCP's `viaStop`
  with `LON`). The joint search would take it, since a waypoint is already
  a TIPLOC set. But segments are named by their waypoint CRS, the chained
  explainer resolves CRS codes per segment, and the waypoint conflict
  checks compare codes. The segment shape for "a group as a segment end"
  needs its own decision. For now `waypoints=group:LON` (or `KGX|EUS`) is
  a 400 that says waypoints take single stations, and DS-MCP keeps a `LON`
  in `viaStop` on its local engine.
- **Frontend.** The `/plan` page's "Pass through" picker selects stations.
  Exposing groups there needs a group option in the picker, so it is not
  trivial; it is left for later.
- **A discovery endpoint** for the groups. For now the CSV and this doc
  list them, and `stationGroups` echoes the members in use.
