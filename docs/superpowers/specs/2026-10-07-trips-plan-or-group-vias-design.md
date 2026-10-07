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

`waypoints` takes OR choices too, as a follow-up the same day: see
"Waypoint groups" below. (The first version of this branch made them a
400.)

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

## Waypoint groups (follow-up, 2026-10-07)

DS-MCP's `viaStop` with `LON` ("call at any London terminal") was the case
left on its local engine. A `waypoints` entry now takes the same OR-choice
syntax as `via`: `waypoints=group:LON`, `waypoints=KGX|EUS|STP`,
`waypoints=YRK,group:LON|CBG`. Each comma-separated entry is still ONE
waypoint, in order.

The DS-MCP session decided four points (2026-10-07), recorded here as the
contract:

1. **An end as a member counts.** As for an OR via, a group holding the
   origin or the destination may be satisfied there (details below).
2. **It is needed.** DS-MCP switches `viaStop` with `LON` to
   `waypoints=group:LON` once this ships.
3. **Member selection is plain earliest arrival.** No preference order
   among members: the member a real train reaches first wins.
4. **No group-specific dwell or minimum-stop rule.** Only the ordinary
   same-train continuation exemption that any single-station waypoint
   already gets: no change is charged when the train that reached the
   member continues on the same working.

### Semantics

- **Satisfied by stopping at ANY member.** A call there (the traveller
  alights, or stays aboard a train that calls there), or a walk into it,
  exactly as for a single-station waypoint. A train running through a
  member without calling does not satisfy it.
- **Which member.** The engine needs no new state: a waypoint was already
  a TIPLOC set, so a group is the union of its members' TIPLOCs, and
  arriving at any of them advances the stage. The journey's own optimum
  decides the member: the earliest arrival at the destination (or, for
  arrive-by, the latest departure), and along that journey the FIRST
  member reached. When one train calls at two members (WAE then WAT, say),
  the stage advances at the first, and the traveller rides on from there
  in the next segment. A later member cannot "re-take" it, because the
  ride at the greater stage dominates the ride at the lesser one
  (`a_group_waypoint_is_satisfied_at_the_first_member_reached`).
- **Segments.** The journey is split at whichever member it used. Segment
  ends are named by the waypoint's label (`group:LON`, `KGX|EUS`), the
  same for every journey, since the member differs per journey; the legs
  and `journeys[j].waypointSatisfiedBy` say which member.
- **Dwell, changes and minimum stop.** All as for a single-station
  waypoint, at the member used. Staying aboard the train that reached it
  is no change (`continuesPreviousTrain`). A fresh boarding there costs
  that member's own minimum change time, or the waypoint fallback of 5
  minutes at a `NoInterchange` sentinel. The change cap counts the whole
  journey.
- **Arrive-by.** The backward scan mirrors the same state rule, and the
  journey is built by the forward search from the latest departure, so
  "first reached" means the same in both directions. The random oracle
  test (`a_group_waypoint_is_the_best_of_its_members`: the group's
  earliest arrival and latest departure equal the best single member's)
  found one gap, which is now fixed in `reverse.rs`. When the onward train
  reaches ANOTHER member later, the scan kept only "aboard at the earlier
  stage" (the dominating state), and at that stage a `NoInterchange`
  member forbids boarding. The forward search boards there at the next
  stage, with the fallback. A boarding the scan would drop for that reason
  is now retried at the next stage
  (`arrive_by_boards_at_a_no_interchange_member_after_stopping_there`).
  With single-station waypoints this could only arise on a train calling
  twice at the waypoint.
- **Ends.** Per point 1, an OR choice that is the FIRST waypoint and
  includes the origin is satisfied at the origin. One that is the LAST
  waypoint and includes the destination is satisfied at the destination.
  Either one is left out of the search and gets no segment: an empty
  segment carries no information and would be an itinerary with no legs.
  `waypointSatisfiedBy` reports it with `how: "origin"` (segment 0) or
  `"destination"` (the last segment). If one choice holds both ends, it is
  satisfied at the origin, which is reached first. A member equal to an
  end in any OTHER position is an ordinary stop: the journey must come
  back to it, which keeps the waypoints' order meaningful. A single
  station equal to an end is still the old 400.
- **Avoid lists.** A member in `avoid`, `avoidStop` or `avoidChange` is
  dropped from the choice, since a stop there is not allowed. That
  matches "avoided members are never used" for vias, and the
  single-station waypoint rule, where any of the three is a 400. A choice
  left with no member is a 400. A single-station waypoint in an avoid list
  is still the old 400, and so is an avoid-list group that contains a
  single-station waypoint.
- **Adjacent waypoints** that share a station, at least one of them a
  choice (`group:LON,KGX`, `group:LON,group:LON`), are a 400: one stop
  would satisfy both. The check runs after avoided members are dropped. The
  same station twice as single waypoints keeps its old message ("is the
  waypoint before it"). `group:LON,CBG,group:LON` is fine.
- **Vias** are independent of waypoints, as before. A via (or an OR via)
  and a waypoint group may share stations: `via=KGX&waypoints=group:LON`
  passes King's Cross somewhere and stops at some terminal.
- **The explainer.** When no journey stops at every waypoint, the
  per-segment chained planner names the failing segment. Its segments
  start and end at every member of a choice. So a chained segment may
  leave from a different member than the previous one reached, which is
  looser than the joint search. That is acceptable for finding which
  segment cannot be planned. Messages name the label
  (`KGX -> group:LON`).

### Limits and the guard

| limit | value | why |
|---|---|---|
| waypoints | 20 (unchanged, `TRIP_PLAN_MAX_WAYPOINTS`) | a choice counts as one |
| stations in one waypoint | 24 | as for a via: room for `group:LON` and a few more |
| stations across all waypoints | 54 | as for vias, and separately from them; singles count 1 each, so 2 `LON` groups and 18 singles, or 3 `LON` groups |
| options search-size guard | unchanged formula | a choice is one waypoint in `(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)` |

`bench_group_waypoints_against_single` (release, the same synthetic
27.9k-train network, 8 rounds, medians over three OD pairs on a shared,
busy machine) puts 18-member groups in place of single hubs:

| waypoints (groups) / vias | members | CSA | CSA arrive-by | RAPTOR | arrive-by rounds |
|---|---|---|---|---|---|
| 3 (3) / 0 | 1 | 93 ms | 135 ms | 5.6 s | 2.9 s |
| 3 (3) / 0 | 18 | 91 ms | 110 ms | 4.3 s | 0.9 s |
| 20 (2) / 0 | 1 | 937 ms | 547 ms | 4.4 s | 4.4 s |
| 20 (2) / 0 | 18 | 738 ms | 398 ms | 4.7 s | 2.1 s |
| 3 (3) / 3 | 1 | 91 ms | 253 ms | 5.1 s | 3.0 s |
| 3 (3) / 3 | 18 | 88 ms | 194 ms | 4.7 s | 2.6 s |

The last two rows have three 18-member group vias with the 18-member
waypoints, at the guard's limit. A group waypoint costs no more than a
single one: within noise, and often less, because a stage fills in sooner
when any of 18 stations advances it. The per-connection cost is one
`HashSet` lookup either way. So the guard counts a group as one waypoint.
The 20-waypoint rows find no arrive-by journey by 23:00 on this network;
they measure the search's cost, not a result.

### Response

- `segments[s].originCrs`/`destinationCrs`: a choice's label at its ends.
- `journeys[j].waypointSatisfiedBy[k]` (only when some waypoint is a
  choice, so single-waypoint responses are byte-identical):
  `{crs, matchedCrs, segment, how}`. `crs` is the waypoint as requested.
  `matchedCrs` is the station stopped at. `segment` is the segment that
  ends there. `how` is `call`, `walk`, `origin` or `destination`.
- `stationGroups` also lists groups named in `waypoints`.

## Discovery: `GET /Trips/station-groups`

This is for pickers (the `/plan` page; any client). It takes no
parameters and reads no database, since the groups are compiled in.
`Cache-Control: public, max-age=3600`.

```json
{"groups": [{"group": "LON", "code": "group:LON", "name": "London Terminals",
  "members": [{"crs": "BFR", "name": "London Blackfriars"}, "..."]}]}
```

`name` comes from `GROUP_NAMES` in `station_groups.rs`, falling back to
the code. Members' names are the CSV's third column.

## Frontend (`/plan`)

The Advanced options' "Pass through" picker, and the "Call at" stops
picker, offer each group from `GET /Trips/station-groups` above the
station suggestions. The label is "Any London terminal (18 stations)",
and the request sends `group:LON`. An itinerary names the member it used:
"via King's Cross (any London terminal)", from `matchedCrs`.

## Not done (follow-ups)

- More groups: only `LON` exists. Each new group needs a name in
  `GROUP_NAMES` for the picker.
