# API changelog

Changes to the Distant Signal (DS) HTTP API that a client such as DS-MCP
needs to know about. Newest first. Field names are as served (camelCase).

## 2026-10-08: `/public/trains/search` dates beyond 7 days, and the arrival at `stops_at`

Additive only: a request that worked before gets the same answer, plus
the new `stopsAt*` fields when it sets `stops_at`.

### `date`: any published date

- `date` is now accepted when it is within 7 days of today (as before) OR
  within the range of service dates DS holds timetable rows for. DS
  publishes today plus 7 days ahead and keeps 8 days back, so in practice
  the range is today−8 to today+7. It widens on its own if the publish
  window or retention grows. Querying a far date costs the same as
  querying today.
- A date outside both is a `400` (`text/plain`) naming both ranges:
  `date must be between 2026-09-30 and 2026-10-15: schedule data is
  published for 2026-09-30 to 2026-10-14`. With no rows at all, it is the
  text from before plus a suffix: `date must be within 7 days ago and 7
  days from today: no schedule data is published`.
- Unchanged: a date inside the range with no rows of its own (not yet
  published, or a gap) is still a `404`, `no CIF-derived schedule data has
  been published for YYYY-MM-DD`; a malformed date is still `400 date must
  be YYYY-MM-DD`. `GET /public/trains/resolve` keeps the plain ±7-day
  window.

### New: `GET /public/trains/search/dates`

`200 {"from": "2026-09-30", "to": "2026-10-15", "publishedFrom":
"2026-09-30", "publishedTo": "2026-10-14"}`. `from`/`to` (inclusive) bound
the `date`s the search accepts; `publishedFrom`/`publishedTo` bound the
dates with rows (`null` when there are none). Dates are London service
dates.

### New row fields with `stops_at`

Only when the request sets `stops_at`, each `results[]` row gains the
arrival at that station. Without `stops_at` the keys are absent.

| Field | Type | Meaning |
| --- | --- | --- |
| `stopsAtArrival` | `"HH:MM"` or `null` | Public (GBTT) arrival at `stops_at`. |
| `stopsAtArrivalDayOffset` | integer or `null` | Days after `date` (the service date) that `stopsAtArrival` falls on. |
| `stopsAtWorkingArrival` | `"HH:MM"` or `null` | Working-timetable arrival there: the time `arrival_from`/`arrival_to` compare. |
| `stopsAtWorkingArrivalDayOffset` | integer or `null` | Days after `date` for `stopsAtWorkingArrival`. |

- **Which call.** The call is the same one the `stops_at` filter matched:
  the EARLIEST call at `stops_at` that comes after `station` in the journey,
  allows alighting, and, when given, has its working arrival inside
  `arrival_from`/`arrival_to`. If `stops_at` is the train's terminus and no
  earlier call qualifies, it is the terminus (then the fields equal
  `publicDestinationArrival`/`destinationArrival` and their offset). So a
  train calling at `stops_at` twice reports the first call, unless an
  arrival bound excludes it, in which case it reports the call that
  satisfied the bound.
- **Day offsets** count from the service date, like `dayOffset`. A stop
  reached after midnight has `1`. An arrival before midnight at a stop left
  after midnight has the earlier day. A public time that rounds across
  midnight (23:59H working, 00:00 public) is on the next day.
- **Null.** A time and its offset are `null` together, when that call has
  no such time stored (a public time before the schedule's next publish).
- Example, `?station=PAD&stops_at=RDG&date=2026-10-20`:
  `{"uid": "C12345", "stationCrs": "PAD", "publicDeparture": "23:48",
  "dayOffset": 0, "destinationCrs": "BRI", "publicDestinationArrival":
  "01:32", "destinationArrivalDayOffset": 1, "stopsAtArrival": "00:15",
  "stopsAtArrivalDayOffset": 1, "stopsAtWorkingArrival": "00:14",
  "stopsAtWorkingArrivalDayOffset": 1, ...}`

## 2026-10-08: `GET /public/ready` and graceful shutdown

- New `GET /public/ready`, the readiness probe: `200 {"status":"ready"}`,
  or `503 {"status":"draining"}` while the pod is shutting down. Only with
  `API_READINESS_CHECKS_DB=true` (default false; meant for 2+ replicas) it
  also answers `503 {"status":"database_unreachable"}` when the database
  did not answer `SELECT 1` within 2 s (checked at most every 5 s).
  `Cache-Control: no-store`. `GET /public/health` is unchanged (liveness).
- On shutdown the api finishes in-flight requests (up to
  `API_SHUTDOWN_DRAIN_SECS`, default 20 s) instead of resetting them. A
  keep-alive connection is closed after its current response, so a client
  must reconnect for its next request, as on any closed connection.

## 2026-10-08: timetable data four weeks ahead

DS now publishes its timetable products for today through today+28 (was
today+7). No request or response shape changes; dates that used to `404`
for lack of data now answer.

- `GET /Trips/plan`: a `date` up to today+28 now plans. Past that it is
  still `404`, `no CIF-derived schedule data has been published for
  YYYY-MM-DD yet`.
- `GET /Train/by-uid/{uid}/{date}`: a train's scheduled stops and its
  bus/ferry mode are available up to today+28.
- `GET /public/trains/search`: the rows exist up to today+28. The route
  itself still accepts only dates within 7 days of today until the
  "any published date" change to `date` lands; from then on its range
  (and `GET /public/trains/search/dates`'s `publishedTo`) reaches
  today+28 with nothing else to change.
- Unchanged: tracking a train (`/Train/by-uid/{uid}/{date}/track`, pins)
  and `GET /public/trains/resolve` keep their 7-day forward limit.
- A far date shows the timetable as planned at the latest daily
  delivery. Late changes (engineering-works replacements, cancellations)
  often reach the timetable only a few weeks ahead, so results for a
  date 2-4 weeks away can change between deliveries. Nothing in a
  response marks a row as provisional.
- The window is a deployment setting (`SCHEDULE_FORWARD_PUBLISH_DAYS`,
  7-60, default 28). Dates past today+7 appear after the first delivery
  following the deploy (deliveries land around 20:00 UTC).

## 2026-10-07: live status and origin names on the schedule lists

Additive only: every existing field is unchanged. Applies to each row of
`GET /public/trains/search` (`results[]`) and of
`GET /public/stations/{crs}/schedule-departures`.

### New fields on both

- `live`: the same compact object as `live` on
  `GET /public/lines/{id}/trains?view=summary`:
  `{"status", "delayMinutes", "delayProvisional", "cancelled",
  "lastReportedLocation"}`, for the row's train on its service date.
  `null` when the train has no live state (not activated by TRUST yet, or
  never looked up), which is common for later trains. Example:
  `"live": {"status": "en_route", "delayMinutes": 4, "delayProvisional":
  false, "cancelled": false, "lastReportedLocation": "Reading"}`.
- `originName`: the station name of `originCrs` (the schedule's true first
  calling point), or `null` when the code is `null` or has no station.

### New fields on `GET /public/trains/search` only

- `dayOffset`: days after the searched `date` (the service date) that the
  departure from `station` falls on: `1` for a train that started before
  midnight and calls at `station` after it. The same convention as
  `/schedule-departures`' existing `dayOffset`: it moves the departure's
  calendar day, never the service date, so `GET /Train/by-uid/{uid}/{date}`
  still takes the searched `date`. `0` otherwise.

### New fields on `GET /public/stations/{crs}/schedule-departures` only

- `originCrs`: the schedule's true origin CRS (the same meaning as on
  `/public/trains/search`), or `null` when it does not resolve to a
  station. That route's `dayOffset` already existed.
- With `?date=` (the entry below), `live`, `originCrs` and `originName`
  are looked up for the requested service date, like the rows themselves.

## 2026-10-07: `?date=` on station schedule departures

### New query parameter (`GET /public/stations/{crs}/schedule-departures`)

- `date` (optional, `YYYY-MM-DD`): serve that London service date's
  CIF-derived bucket instead of today's. Only today or yesterday is
  accepted; any other date, or a malformed one, is a `400`. Omitting it is
  unchanged (today).
- Every row in a response has the requested date as its CIF service date;
  `dayOffset` counts from it. So a row's `/Train/by-uid/{uid}/{date}` link
  uses the requested date, and its departure falls on `date + dayOffset`.
- Why: a service that left its origin last evening can call at a station
  after midnight. Its rows are in yesterday's bucket with `dayOffset >= 1`,
  and today's bucket cannot hold them. DS's `/track` picker asks for
  yesterday between London 00:00 and 02:00 and keeps those rows still due.
- An older server ignores the parameter and returns today's bucket.

## 2026-10-07: `/Trips/plan` waypoint groups (`waypoints=group:LON`), `GET /Trips/station-groups`

Design: `docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md`,
"Waypoint groups". This closes `viaStop` with `LON` for DS-MCP: send it as
`waypoints=group:LON`. The entry below (OR vias) still applies; this one
replaces its "waypoints take single stations" 400.

### Changed parameter: `waypoints` entries may be OR choices

`waypoints=ALT[|ALT...][,ALT[|ALT...]...]`, the same syntax as `via`: commas
separate the waypoints (ordered, each one a stop), `|` the alternatives of
one waypoint, and an alternative is a CRS, a `tiploc:` code or `group:NAME`
(any case). `|` may be raw or `%7C`.

- **Semantics:** the journey must STOP at (call at, alight at, or walk
  into) ANY one station of each waypoint, in order. A train running
  through without calling does not count, as for a single waypoint.
- **Which member:** plain earliest arrival. The member a real train reaches
  first on the best journey wins (the latest-departing one for
  `arriveBy`), with no preference order among members. The member can
  differ per journey.
- **Dwell and changes:** no group-specific rule. Staying aboard the train
  that reached the member is no change (`continuesPreviousTrain: true` on
  the next segment). Otherwise the member's own minimum change time
  applies (5 minutes at a no-interchange sentinel), and `maxChanges`
  counts the whole journey.
- **Ends:** a choice that is the FIRST waypoint and includes the origin is
  satisfied at the origin, and a LAST one including the destination at
  the destination. Such a waypoint gets NO segment (see
  `waypointSatisfiedBy`). One holding both ends is satisfied at the
  origin. A member equal to an end anywhere else is a real stop (the
  journey must return to it). A single-station waypoint equal to an end
  is still a 400.
- **Avoid lists:** a member in `avoid`, `avoidStop` or `avoidChange` is
  dropped from the choice. It is a 400 only if no member is left.
- **Limits:** a choice is ONE waypoint against the cap (20) and in the
  `results=options` guard `(waypoints + 1) * (2 * vias + 1) * (maxChanges
  + 2)`. A choice has at most 24 stations, and the waypoints together at
  most 54, groups expanded and single stations counting 1. That allows 3
  `group:LON`, or 2 and 18 single stations. This budget is separate from
  the vias'.
- Single-station waypoints behave, and are answered, exactly as before.

### New and changed fields

- `segments[s].originCrs`/`destinationCrs`: a choice's label
  (`group:LON`, `KGX|EUS`) where a segment starts or ends at it. Each
  journey's legs show the actual station.
- `journeys[j].waypointSatisfiedBy` (ONLY when at least one waypoint is a
  choice): one entry per requested waypoint, in order:
  - `crs`: the waypoint as requested (its label);
  - `matchedCrs`: the station the journey stopped at (a CRS, or a
    `tiploc:` code);
  - `segment`: the index of the segment that ends there (the next one
    starts there). For `how: "origin"` it is 0; for `"destination"`, the
    last segment;
  - `how`: `call` (a train took the traveller there), `walk` (a transfer
    leg into it), `origin` or `destination` (an end of the trip satisfied
    it).
- `stationGroups` also lists the groups named in `waypoints`.

```json
"segments": [{"originCrs": "CBG", "destinationCrs": "group:LON", "...": "..."},
             {"originCrs": "group:LON", "destinationCrs": "BTN", "...": "..."}],
"journeys": [{"waypointSatisfiedBy": [
  {"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "how": "call"}
]}]
```

With `origin=KGX&waypoints=group:LON&destination=CBG` there is one segment
(KGX to CBG), and `waypointSatisfiedBy` is
`[{"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "how": "origin"}]`.

### New 400s

- `waypoints: 'X' stands for 25 stations, at most 24 allowed in one waypoint`
- `waypoints: the waypoints stand for 56 stations together, at most 54 allowed`
- `waypoints: 'group:XYZ' is not a known station group (known: group:LON)`
- `waypoints: 'KGX|' has an empty alternative`
- `waypoints: every station of 'KGX|EUS' is in an avoid list; a trip
  cannot stop at one and avoid them all`
- `waypoints: 'group:LON' and 'KGX' are next to each other and share 'KGX';
  one stop there would satisfy both, so name stations they do not share or
  put another waypoint between them`: two adjacent waypoints, at least one
  a choice, that share a station (after avoided members are dropped).
- An unknown station in a choice: `ORIGIN -> LABEL: 'ZZZ' is not a
  recognised station CRS code` (as for single waypoints).
- Removed: `waypoints: '...' is a choice of stations; waypoints take single
  stations ...`.

### New endpoint: `GET /Trips/station-groups`

Every group `group:NAME` accepts, for a picker. No parameters, no
authentication, `Cache-Control: public, max-age=3600` (the groups change
only with a deploy).

```json
{"groups": [{"group": "LON", "code": "group:LON", "name": "London Terminals",
  "members": [{"crs": "BFR", "name": "London Blackfriars"}, "..."]}]}
```

`code` is the entry to send. `members` are in a fixed order, and a
member's `name` may be `null`.

## 2026-10-07: `/Trips/plan` OR-choice vias and station groups (`group:LON`)

Design: `docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md`.
This closes the last gap keeping DS-MCP's `plan_journey` on its local engine
for a `LON` group code in `via` or `avoid`/`avoidStop` (`dsTripPlanEligible`).
`viaStop` is closed by the entry above (waypoint groups).

### Changed parameter: `via` entries may be OR choices

`via=ALT[|ALT...][,ALT[|ALT...]...]`. Commas still separate the vias, which
are ordered and at most 3, and must ALL be passed. `|` separates the
alternatives of one via; ANY one of them satisfies it. An alternative is a
CRS, a `tiploc:` code or a named group `group:NAME` (any case). `|` may be
sent raw or as `%7C`.

- `via=group:LON`: through any of the eighteen London Terminals (BFR CST
  CHX CTK EUS FST KGX LST LBG MYB MOG OLD PAD STP VXH VIC WAT WAE).
- `via=KGX|EUS|STP`: through any of the three.
- `via=group:LON|CBG,PBO`: through a London terminal or Cambridge, then
  Peterborough.
- What counts as passing a member is what counts for a single via: a
  call, a timing-point pass, a change or a walk.
- **Limits:** at most 24 stations in one via, and 54 across all vias, with
  groups expanded. A choice counts as ONE via against the cap of 3 and in
  the `results=options` search-size guard.
- **Ends:** a member equal to the origin or the destination satisfies the
  via there. Every journey passes it, so for example `origin=KGX` with
  `via=group:LON` always holds. A single-station via equal to an end is
  still a 400.
- **With `avoid`:** avoided members are never used. The via is a 400 only
  when EVERY station of it is in `avoid`.
- Single-station vias behave exactly as before.

### Changed parameter: avoid lists take groups

`avoid`, `avoidStop` and `avoidChange` accept `group:NAME` entries, which
avoid (or don't stop at, or don't change at) every member. A group counts as
one entry against the cap of 8 per list. An avoid list already means "none
of these", so it needs no `|`: `|` there is a 400. An avoided member that is
the origin, the destination or a waypoint is a 400 naming the group.

### New and changed fields

- `stationGroups` (top level, always present): each group the request
  named, mapped to its members, e.g. `{"LON": ["BFR", "CST", ...]}`.
  It is `{}` when no group was named.
- `via`: each via's label as applied, e.g. `["group:LON", "KGX|EUS", "PBO"]`.
- `avoid`/`avoidStop`/`avoidChange`: as applied, so with groups expanded
  into their member CRS codes.
- `journeys[j].viaSatisfiedBy[k].matchedCrs`: the station that satisfied
  via `k`, which for a choice is the member the journey passed. `crs` stays
  the via as requested (its label).

```json
"via": ["group:LON"],
"stationGroups": {"LON": ["BFR", "CST", "CHX", "...", "WAE"]},
"journeys": [{"viaSatisfiedBy": [
  {"crs": "group:LON", "matchedCrs": "KGX", "segment": 0, "leg": 1, "how": "call"}
]}]
```

- `noResultReason` (`constraint: "via"`) carries the label in `values`
  (`["group:LON"]`). Its message reads "... passes through any of
  group:LON ..." or "any of KGX, EUS".

### New 400s

- `via: 'group:XYZ' is not a known station group (known: group:LON)`
- `via: 'KGX|' has an empty alternative`
- `via: 'Z00|...' stands for 25 stations, at most 24 allowed in one via`
- `via: the vias stand for 60 stations together, at most 54 allowed`
- `via: 'KGX|EUS' is given twice in a row; name it once`: the same station
  set twice in a row, however it is written.
- `via: every station of 'KGX|EUS' is also in avoid; a trip cannot both
  pass through one and avoid them all`
- `via: 'ZZZ' is not a recognised station CRS code`: this now also names
  an unknown alternative.
- `avoid: 'KGX|EUS' uses '|', which only via takes; ...`
- `avoid: 'group:LON' includes 'PAD', which is the destination; a trip
  cannot avoid it`
- (Superseded by the waypoint-groups entry above: waypoints now take
  choices.) `waypoints: 'GROUP:LON' is a choice of stations; ...`

## 2026-10-07: walks between bus stops and their stations in the planner

Design: `docs/superpowers/specs/2026-10-06-tiploc-locations-design.md`
("Walking links to the parent station").

### Changed behaviour (`GET /Trips/plan`, no new fields)

- A journey can now change between a bus stop or ferry terminal and its
  parent station (`parentCrs`): bus, then walk, then train, and the reverse.
  The walk is an ordinary transfer leg: `{"kind": "transfer", "mode":
  "WALK", "originCrs": "tiploc:HTRBUS2", "destinationCrs": "HXX",
  "minutes": 5}`. So a transfer leg's `originCrs`/`destinationCrs` can now
  be a `tiploc:` code, as train legs' already could. Clients that branch
  on `leg.kind` need no change.
- Walking on from a bus or ferry is now a change off it. It owes the same
  5-minute buffer (configurable) as changing at the stop, before the walk
  starts. This applies to every walking or transfer link (e.g. a
  rail-replacement bus, then a walk to the Underground), and arrive-by
  already charged it. `minutes` is the walk alone. The buffer shows only
  in the times.

## 2026-10-07: Delay Repay borderline zone, outcome, ticket-aware bands and schemes

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`,
"Decisions (2026-10-07)". Applies to `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
and to every item of `GET /Train/tickets/mine` (the two now serve the same
fields from one function).

### New fields on the response (and each ticket list item)

- `outcome`: `arrived`, `departedOnly` or `notReached`; `null` while not
  known yet (en route, or a destination that reports nothing).
  - `departedOnly`: TRUST reported only a departure at the destination;
    the delay is final, measured on that departure.
  - `notReached`: cancelled before the destination (TRUST `0002` at origin,
    on call, out of plan, or en route from an earlier call), a TRUST pass
    through it, or Darwin's cancelled call once TRUST has reported the
    train beyond it. `delayMinutes`, `delayBasis` and `estimate` are then
    `null` and `provisional` is `false`; `measuredAtCrs` still names the
    destination. Show no percentage: eligibility depends on the
    replacement journey.
- `measuredAtName`: `measuredAtCrs`'s station name, when known.
- `ownScheme`: `true` for an operator running its own compensation scheme
  (Elizabeth line, London Overground, Merseyrail, Grand Central). `estimate`
  is then `null`; show the claim link only.
- `schemeOperator`: the operator the scheme was matched to (`"LNER"`,
  `"Caledonian Sleeper"`, ...), or `null` for the DR15 default.
- `rulesCheckedOn`: `YYYY-MM-DD` the scheme table was last checked
  (`2026-10-07`).

### Changed and new fields on `estimate`

- **`percentage` is now nullable**: `null` exactly when `borderline`.
- `borderline` (bool) and `thresholdMinutes`: while provisional, a
  projection less than 3 minutes above a band threshold (15–17, 30–32,
  60–62, 120–122; Heathrow Express 31–33, 61–63) is borderline. Show
  "could go either way" with the threshold, not a percentage. Final
  estimates are never borderline.
- `scheme` may also be `HX` (Heathrow Express's own scheme: 25% of the
  ticket for more than 30 minutes, 50% for more than 60; `bandMinutes` 30
  or 60 meaning "more than").
- `ticketKind`: `single`, `return` or `unknown`, from the ticket's
  free-text type (whole words `return`/`rtn`, `single`/`sgl`/`sngl`).
- `fareBasis` may also be `ticket` (Heathrow Express). At 120+ minutes it
  is `single` for a single ticket (already refunded in full from 60
  minutes) and `return` otherwise.
- `roomSupplementPercentage`: Caledonian Sleeper only, 50 at 30–59
  minutes and 100 at 60+; else `null`.

### Changed behaviour

- Scheme table: DR30 now also covers Caledonian Sleeper, Hull Trains and
  Lumo (with LNER, CrossCountry, ScotRail); Heathrow Express has its own
  scheme; the four own-scheme operators above get no estimate. Each has its
  own `claimUrl`.
- When the ticket's `operator` text names no known operator, the train's
  CIF ATOC code decides the scheme (before falling back to DR15). A ticket
  without an operator now gets an estimate when the train's code is known.

## 2026-10-07: a line's full-day timetable; `running` follows each train's whole run

Design: `docs/superpowers/specs/2026-10-06-line-page-trains-design.md` §5.

### New: `GET /public/lines/{id}/timetable`

One page of the line's trains for a service date, cursor-paged. Same read
gate, date default (London today) and `404` (no population for the date)
as `/trains`. Parameters (all optional):

| parameter | meaning |
|-----------|---------|
| `date` | `YYYY-MM-DD` |
| `scope` | as `/trains`; **default `line,shared`** |
| `dir` | `up`, `down`, `loop` (comma list); `direction` is accepted too |
| `from` | a station CRS: only trains with a public call there, listed (and paged) by their departure from it |
| `to` | a station CRS: only trains with a public call there -- after `from` when both are given (`to` must differ from `from`) |
| `at` | `HH:MM` (hours `00`–`47`): trains from this time on |
| `after` | the previous page's `nextCursor`, unchanged |
| `limit` | 1–200, default 50 |

```json
{
  "lineId": "swr-south-west-main",
  "date": "2026-10-07",
  "scopeApplied": true,
  "scopes": ["line", "shared"],
  "directions": ["down"],
  "from": "WOK",
  "to": "WEY",
  "at": "09:00",
  "stations": [{"crs": "WAT", "name": "London Waterloo", "role": "terminus"}],
  "counts": {"line": {"down": 41, "up": 40}, "shared": {"down": 12}},
  "trains": [{
    "uid": "L80147", "operator": "SW", "serviceMode": "train", "liveTracking": true,
    "scope": "line", "direction": "down",
    "lineDue": {"time": "08:35", "dayOffset": 0},
    "origin": {"crs": "WAT", "name": "London Waterloo"},
    "destination": {"crs": "WEY", "name": "Weymouth"},
    "onLineStops": [{"crs": "WAT", "time": "08:35", "dayOffset": 0}],
    "live": null,
    "time": {"time": "09:02", "dayOffset": 0},
    "arrival": {"time": "11:10", "dayOffset": 0}
  }],
  "nextCursor": "542.L80151"
}
```

- Each train is a `view=summary` train plus `time` (its departure from
  `from`, else its `lineDue`) and `arrival` (its arrival at `to`, `null`
  without `to`). Trains are ordered by `time`, then `uid`; a train with no
  public call on the line is not listed.
- `nextCursor`: pass it back as `after` with the same other parameters
  for the next page; `null` on the last page. Opaque -- don't build one.
- `counts`: the whole day's trains under `scope`, `from` and `to`, per
  scope and direction (`none` without one) -- before `dir`, `at` and the
  cursor, for direction tabs.
- `live` is looked up for the page's trains only.
- A malformed parameter is a `400`.

### Changed: `view=summary`'s `running`

`running` used to consider only trains that reached the line in the six
hours before `at`, so a long run (Edinburgh to Plymouth) dropped out of
it. It now uses each train's whole on-line run: due on the line at or
before `at`, last on-line call no more than three hours (the late-running
grace, as before) before it. Without a window (`from`/`to`), every listed
train now carries its `onLineStops`, `origin` and `destination` (they
were only filled for trains near `at`).

Both views now read a table derived from the population at publish time
when it has the line and date (`line_train_summaries`), and the
population itself otherwise; the responses are the same either way.

## 2026-10-07: named bus stops and timing points; bus stops in the planner

Design: `docs/superpowers/specs/2026-10-06-tiploc-locations-design.md`.

### New fields on `JourneyStop`

`locationType` (`station`, `bus_stop`, `ferry_terminal`, `junction`,
`siding`, `passing_point`, `other`, or `null` when nothing is known) and
`parentCrs` (a bus stop's or ferry terminal's station, else `null`).

### Changed behaviour

- `JourneyStop.name` is now set for stops with no station name: bus stops
  (`"Heathrow Terminal 3 (bus stop)"`), ferry terminals (`"Brodick (ferry
  terminal)"`), timing points (`"Marylebone 10 Signal"`). `null` only for a
  TIPLOC no source knows.
- A destination with no CRS (station boards, train search `destinationCrs`)
  is now keyed `tiploc:HTRBUS3` instead of `~HTRBUS3`, and its
  `destinationName` is set. Rows published before the change keep the `~`
  key, also named, until the next daily publish.
- `GET /Trips/plan`: `origin`, `destination`, `waypoints`, `via` and the
  avoid lists also take a bus stop's or ferry terminal's `tiploc:` code (any
  case). A `tiploc:` via is satisfied only by a call there (or a walk into
  it): nothing runs through a bus stop.
  `originCrs`/`destinationCrs` on segments and legs carry that code for such
  an end. A change to or from a bus or ferry costs 5 extra minutes per bus or
  ferry side (configurable), never at the start or end.

### New parameter

`GET /public/stations?q=...&stops=true` appends up to 10 matching bus stops
and ferry terminals: `{"code": "tiploc:KESWICK", "name": "Keswick (bus)",
"kind": "bus"}` (`parentCrs` when the stop has a station). Without
`stops=true` the response is
unchanged.

## 2026-10-06: `view=summary` on a line's trains

Design: `docs/superpowers/specs/2026-10-06-line-page-trains-design.md`.

`GET /public/lines/{id}/trains` without `view` (or with `view=full`) is
**unchanged**: a bare array of every train of the day with its calling
points (about 20 MB for a main line). The summary parameters below are
ignored there.

New, opt-in: `GET /public/lines/{id}/trains?view=summary`, the slim
windowed view the line page uses. Parameters (all optional):

| parameter | meaning |
|-----------|---------|
| `date` | `YYYY-MM-DD`, default London today (as before) |
| `scope` | `line`, `shared`, `touch` (comma list) or `all`; **default `line,shared`** in this view |
| `from`, `to` | `HH:MM`, hours `00`–`47` (`24:00`+ is the next morning of the service date). Keeps trains whose `lineDue` (first public call on the line) is in `[from, to)`. `to` at or before `from` (both before `24:00`) wraps past midnight: `23:00`–`01:00` is two hours. A missing bound is open. |
| `direction` | `up`, `down`, `loop` (comma list) |
| `at` | `HH:MM` (as `from`): also return `running`, the trains between their first and last on-line public call at that moment (last call pushed back by a known delay; not cancelled or completed). Looks back at most 6 hours. |
| `limit` | 1–2000, default 500: the most `trains` returned (`truncated` says when more matched) |

An unknown `view` and a malformed summary parameter are a `400`; no
population for the date is the usual `404`.

```json
{
  "lineId": "swr-south-west-main",
  "date": "2026-10-06",
  "scopeApplied": true,
  "scopes": ["line", "shared"],
  "window": {"from": "13:30", "to": "15:30"},
  "at": "14:00",
  "directions": null,
  "stations": [{"crs": "WAT", "name": "London Waterloo", "role": "terminus"}],
  "counts": {"line": {"down": 9, "up": 8}, "shared": {"down": 31, "up": 29}},
  "truncated": false,
  "trains": [{
    "uid": "L80147",
    "operator": "SW",
    "serviceMode": "train",
    "liveTracking": true,
    "scope": "line",
    "direction": "down",
    "lineDue": {"time": "13:35", "dayOffset": 0},
    "origin": {"crs": "WAT", "name": "London Waterloo"},
    "destination": {"crs": "WEY", "name": "Weymouth"},
    "onLineStops": [{"crs": "WAT", "time": "13:35", "dayOffset": 0}],
    "live": {"status": "en_route", "delayMinutes": 3, "delayProvisional": false,
             "cancelled": false, "lastReportedLocation": "WOK"}
  }],
  "running": []
}
```

- Trains are sorted by `lineDue`, then `uid`. Times are public (GBTT),
  `HH:MM`, with the day offset after the service date.
- `onLineStops`: the train's public calls at the line's catalogue
  stations (a line's `crs_aliases` count as their station).
- `origin`/`destination`: the first/last calling point whose TIPLOC
  resolves to a real station (depots, junctions and `X..` pseudo-CRS are
  walked past); when none does, the first/last on-line call. Never null
  for a train that calls on the line.
- `serviceMode` and `liveTracking`: as on the default view's entries (see
  "buses and ferries" below) -- the published `schedule_services` mode,
  else the population's CIF Train Status (`5` `replacementBus`, `B` `bus`,
  `S`/`4` `ferry`, otherwise `train`).
- `live` is looked up only for the trains in `trains` and `running`;
  `null` when TRUST has no record.
- `counts`: trains in the window per scope and direction (`none` without
  one), before the `direction` filter, for tab counts.
- `stations`: the line's catalogue stations, with `role`.
- `running` is present only with `at`.
- `scopeApplied: false` (and header `x-scope-applied: false`) for a
  population published before train membership: nothing is filtered,
  `scope`/`direction` are null, and `lineDue` is worked out from the
  calling points.

Also fixed: `GET /public/lines/{id}/trains` (default view, with or
without `scope`) could time out on a large population (a per-entry
re-read of the whole population in SQL).

## 2026-10-06: buses and ferries (`serviceMode`, `liveTracking`); no writes on train-page reads

The CIF timetable carries buses and ferries alongside trains (about 9% of
weekday schedules, 19% on Saturday, 25% on Sunday), and Network Rail's
live feed never reports any of them. Every surface used to present them as
trains forever waiting for a movement report.

### New fields: `serviceMode`, `liveTracking`

Added (always present from this release) wherever a scheduled service is
described:

- `GET /Train/by-uid/{uid}/{date}`;
- `GET /Train/{trackingId}`, `GET /Train/mine` items and journey legs'
  tracked-train states;
- `GET /public/trains/search` rows and
  `GET /Journeys/{journeyId}/legs/{legId}/candidates` rows;
- `GET /public/stations/{crs}/schedule-departures` rows;
- `GET /public/lines/{id}/trains` entries (top level, beside `liveStatus`
  and the membership fields; a bus or ferry is never a line's own train,
  so it comes back `scope: "shared"` or `"touch"`, never `"line"`);
- `GET /Trips/plan` train legs;
- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`.

```json
"serviceMode": "replacementBus",
"liveTracking": false
```

- `serviceMode` is `train`, `replacementBus` (CIF Train Status `5` or
  Train Category `BR`), `bus` (any other bus: status `B` or category
  `BS`) or `ferry` (status `S`/`4`). It is per `(uid, service date)`: an
  STP overlay can make a train a replacement bus on some dates only.
- `liveTracking` is `false` exactly when `serviceMode` is not `train`: no
  live position, delay, platform or arrival will ever arrive for it.
- **`/Trips/plan` legs keep `kind: "train"`** for a bus or ferry leg, so
  a client branching on `kind` keeps working; read `serviceMode` to label
  it.
- A schedule the backend has no mode for (not yet published for that
  date) reads as `train`, which is the old behaviour.

### Delay Repay

`GET /Train/{trackingId}/tickets/{ticketId}/delay-repay` for a bus or
ferry returns `delayMinutes: null`, `estimate: null` and a new
`unmeasurableReason` string saying the delay cannot be measured;
`claimUrl` and `disclaimer` are still populated. `unmeasurableReason` is
`null` for a train.

### Changed behaviour

- **Tracking a bus or ferry.** Allowed, timetable-only: the subscription
  is schedule-matched from CIF at once and stays `schedule_matched` (it
  can never become `resolved`). It gets no live alerts and no station-skip
  alerts.
- **Recurring-journey auto-commit** picks a train whenever one fits the
  leg, and a bus or ferry only when none does.
- **`GET /Train/by-uid/{uid}/{date}` no longer creates a `trains` row.**
  For a published schedule nobody has tracked and TRUST has not reported,
  it returns a read-only schedule view with **`trainsId: 0`** (no shared
  row exists; still a number, as DS-MCP's schema requires) and every live
  field `null`. Once the train is tracked or reported, `trainsId` is the
  real row's id. Clients must not treat `trainsId` as stable across that
  transition (none did: it was never a tracking id).

## 2026-10-06: train membership (`scope`) on a line's trains

Design: `docs/superpowers/specs/2026-10-06-line-membership-design.md`.

A line's population (`GET /public/lines/{id}/trains` and `/schedule`) is
every train touching one of its stations: at a hub, mostly other routes'
trains. Each entry now says how it belongs to the line.

### New optional query parameter: `scope`

`GET /public/lines/{id}/trains?scope=` and `GET /public/lines/{id}/schedule?scope=`
filter the entries (in SQL) by membership:

- `line`: the line's own trains;
- `shared`: runs a stretch of the line but is another route's or another
  operator's train (CrossCountry along the South West Main Line);
- `touch`: only touches the line (a hub call, a crossing);
- comma-separated combinations (`line,shared`), or `all`.

**Without `scope` the response is unchanged**: every entry, no filter. The
line page passes `scope=line,shared`; touch-only trains are reached from
station pages. An unknown value is a `400`.

When `scope` is given, the response carries the header
`x-scope-applied: true`, or `false` when the line's population was
published before membership existed: then it was NOT filtered and every
entry is returned. (A header, because both bodies are bare arrays.)

### New fields on `/trains` entries

Each omitted when absent (on an older population, all of them):

```json
{
  "scope": "line",
  "direction": "down",
  "runFirstCrs": "WAT",
  "runLastCrs": "WEY",
  "lineDue": { "time": "23:35:00", "dayOffset": 0 }
}
```

- `direction`: `down`/`up` by the line's catalogue station order (the
  order of `GET /public/lines/{id}/definition`'s `stations`), `loop` when
  the run starts and ends at the same station.
- `runFirstCrs`/`runLastCrs`: the first and last line station of the
  train's run along the line (absent for `touch`).
- `lineDue`: the train's first public call at one of the line's stations,
  Europe/London local time, `dayOffset` days after the service date.

`/schedule` entries carry the same data under the population's own
snake-case names: `scope`, `direction`, `run_first_crs`, `run_last_crs`,
`line_due` (`{"time", "day_offset"}`).

## 2026-10-06: 503 + `Retry-After` when DS is temporarily unavailable

Every route, public and `/private`, now answers **503 Service Unavailable**
instead of 500 when it could not reach its database (or Redis, or an
upstream HTTP dependency it calls): the pool timed out or is closed, the
connection was refused, reset or cut, or Postgres said it is shutting down,
starting up or out of connections (SQLSTATE class 08, 57P01, 57P02, 57P03,
53300). Every other failure is still a 500. On 2026-10-01 a six-hour
Postgres outage made every route answer 500.

```http
HTTP/1.1 503 Service Unavailable
Content-Type: application/json
Retry-After: 30

{"error":"service_unavailable","retryable":true,"message":"Distant Signal is temporarily unavailable. Please retry shortly."}
```

- Match on `error == "service_unavailable"` (or `retryable == true`), never
  on `message`.
- `Retry-After` is in seconds (30 by default; the operator's
  `api.timeouts.unavailableRetryAfterSecs`, 1-3600). Wait at least that
  long, with backoff and jitter, before retrying.
- Every other 503 now also carries `Retry-After`, with its own plain-text
  body as before: `/Trips/plan` shedding load ("too many trip plans are
  being computed right now"), a ticket parse with every parse slot busy,
  sign-in while the identity provider cannot be reached, and a schedule
  publish rolled back by its statement timeout. All 503s are retryable.
- A 500 still means a bug or a bad request the server could not classify;
  retrying it is unlikely to help.
- For DS-MCP: a 503 from `/Trips/plan` means "DS is temporarily
  unavailable", not "no route found" (still a 200 with no itineraries, or a
  404 when no schedule is published for the date).

## 2026-10-06: `/Trips/plan` pass-through `via`; `maxChanges` up to 6

Design:
`docs/superpowers/specs/2026-10-06-trips-plan-via-and-max-changes-design.md`.
This closes the two gaps for which DS-MCP's `plan_journey` still used its
local engine (`dsTripPlanEligible`).

### New parameter: `via`

`GET /Trips/plan?...&via=CRS[,CRS]`: at most 3 stations every journey
must pass through, in this order.

- **What counts.** Staying aboard a train that runs through without
  stopping or calls there, or changing or walking there. This is
  `plan_journey`'s `via` (pass through, stopping or not).
- **Vias and waypoints.** `waypoints` (must call, in order) and `via` are
  separate ordered lists that may interleave.
- **Bus stops.** A via may be a bus stop's or ferry terminal's `tiploc:`
  code (see the 2026-10-07 entry above); only a call there satisfies it.
- **With the bus and ferry buffer.** A change on a bus or ferry side still
  costs the extra minutes, whether or not it happens at a via.
- **With the avoid lists.** `avoidStop=X` with `via=X` means "pass X
  without stopping". `avoid=X` with `via=X` is a 400.
- **Timing points only.** Running through is seen only where CIF records a
  passing point, at timing points. Elsewhere only a call there counts.
- **400s:**
  - an unknown or group code (`via: 'ZZZ' is not a recognised station CRS
    code`);
  - more than 3 vias;
  - the same via twice in a row;
  - a via equal to the origin or the destination.

### New fields

- Top level: `via`. The vias as applied, uppercased and in order. Always
  present (`[]`).
- `journeys[j].viaSatisfiedBy`: present only when `via` is given.
  `[{crs, segment, leg, how}]`, one entry per via in order.
  `segments[segment].itineraries[j].legs[leg]` is the leg that first
  passed it. `how` is one of:
  - `call`: the train called there;
  - `pass`: it ran through without calling, or past a cancelled call;
  - `walk`: a transfer leg into it.

```json
"via": ["ZVP"],
"journeys": [{"changeCount": 0, "departure": {...}, "arrival": {...},
  "totalDurationMinutes": 60,
  "viaSatisfiedBy": [{"crs": "ZVP", "segment": 0, "leg": 0, "how": "pass"}]}]
```

### New `noResultReason.constraint`: `via`

An itinerary exists without the vias, but none with them. `values` is the
single via whose removal alone is enough, or every via. Every segment
carries it. A `maxChanges` cap, or a time or route that fails even without
the vias, is reported as before instead.

### Changed: `maxChanges` accepts 0 to 6 (was 0 to 4)

Out of range is a 400: `maxChanges must be a whole number from 0 to 6
(default 2), not '7'`.

With `results=options`, a new 400 applies when
`(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)` is over the
deployment's bound: 252 by default, configurable in the chart as
`api.tripPlanMaxOptionsSearchSize` (8-504). The message is `... is too
large a search (... = N, at most 252); use fewer waypoints or vias, a
lower maxChanges, or results=fastest`. The limits at the default:

- without vias: all 20 waypoints, at any `maxChanges`;
- with 1 via: 20, 13, 11 and 9 waypoints at `maxChanges` 2, 4, 5 and 6;
- with 3 vias: 8, 5, 4 and 3 waypoints at `maxChanges` 2, 4, 5 and 6.

`fastest` is not affected. DS-MCP: on this 400, fall back to the local
engine.

## 2026-10-06: incidents on the lines they name; `upcoming` on line status

Design:
`docs/superpowers/specs/2026-10-06-incident-line-evidence-design.md`.

### New field: `upcoming`

`GET /Line/Mode/{mode}/Status` and `GET /Line/{ids}/Status` reports gain
`upcoming`: disruptions announced for the line that have not started yet,
soonest first, at most 5. Always present; `[]` when there are none (and
always `[]` for a `TfL` line).

```json
"upcoming": [
  {
    "from": "2026-10-10T23:00:00+00:00",
    "to": "2026-10-11T23:00:00+00:00",
    "summary": "Industrial action to affect TransPennine Express services on Sunday 11, 18 and 25 October",
    "incidentId": "1D3D4694..."
  }
]
```

- `from`/`to` are RFC3339; `to` is exclusive and may be `null`. A whole-day
  event runs from one Europe/London midnight to the next.
- An entry is a note, not a status: it never changes `lineStatuses` or a
  severity. When the day comes, the incident appears in `lineStatuses` as
  usual (and drops out of `upcoming`).
- Listed: unplanned incidents with a high-confidence period that has not
  started yet, for industrial action at any distance, otherwise starting
  within 14 days. `incidentId` is the `GET /public/incidents/{incidentId}`
  id.

### Changed behaviour (no shape change)

- **Which lines an incident is on.** An unplanned incident is on the lines
  its text gives evidence for: the stations it names (resolved against the
  station reference data), a line keyword or brand, or a closed section.
  It is on every line of its operator only for a network-wide notice
  (industrial action, a reduced timetable, "across the ... network", ...)
  or when it names no place at all. Before, most incidents were on every
  line of their operator ("(operator-wide report)" in `reason`). This
  changes `lineStatuses`, `affectedLines` on `GET /public/incidents` rows
  (and its `line` filter), and the detail's `currentlyAffectsLines` alike.
  Archived rows keep their stored `affectedLines` until
  `backfill_incident_lines` is re-run. `LN`/`WM` incidents
  now reach the `LM` lines, and `ZN` ("National Rail") incidents reach the
  lines whose stations they name.
- **How long an unplanned incident shows.** Until the 02:00 rail-day
  boundary after it was listed, reopened or last re-worded (it used to be
  after it was first seen, ever). After that, only while a dated period of
  it is in progress (a stated end date or a weekly schedule window), or
  while it says it lasts "until further notice" (then at most Reduced
  Service, with "(long-running notice)" in `reason`).
- **Not in effect today.** A long-running notice whose schedule window
  excludes now (a Sunday under a Monday-Saturday window) still shows, with
  a `validityPeriods[0].fromDate` at its next window start and
  `isNow: false`; live data does not escalate it, and the line can also
  show its own `ldbws-inferred` status.
- **A notice about the future** (every period still to come, e.g. a strike
  next week) is no longer a status at all; see `upcoming`.

## 2026-10-06: incidents can be "Ended (no longer listed)"

Design:
`docs/superpowers/specs/2026-10-06-incident-source-removal-design.md`.

RDM's Knowledgebase feed drops incidents (nightly, and planned work is never
cleared) without ever setting `ClearedIncident`. DS used to show such an
incident as active forever. An incident is now in one of three states:

| State | `isCleared` | `sourceRemovedAt` |
| --- | --- | --- |
| Active | `false` | `null` |
| Cleared (RDM cleared it) | `true` | `null` |
| Ended (the feed stopped listing it without clearing it) | `false` | when the feed last listed it |

### New field

- `sourceRemovedAt` (RFC3339 or `null`) on `GET /public/incidents` rows and
  on `GET /public/incidents/{incidentId}`.

A client that treats `isCleared: false` as "live" should also check that
`sourceRemovedAt` is `null`.

### New filter

- `GET /public/incidents?state=active|cleared|ended`. Any other value is a
  `400`.

### Changed meaning

- `cleared=false` now returns Active incidents only, not Ended ones.
  `cleared=true` is unchanged. `cleared` is kept as the legacy spelling of
  `state=active`/`state=cleared`.
- Passing both `state` and `cleared` is a `400`.

### History

A change of `isCleared` alone now adds an entry to the detail's `history`,
so the history shows when RDM cleared an incident. Becoming Ended adds no
entry.

## 2026-10-01: public times, phase 2

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`
(§9 decisions, §11 what phase 2 shipped).

Every delay DS serves is now measured against the **public** timetable
(the times a passenger is sold, GBTT), at the passenger's own stop. The
trip planner plans on public times. Working-timetable (WTT) times stay
internal, for matching, except where noted.

### Deprecation: `scheduled*` (working timetable)

These fields still carry the **working** (WTT) time, truncated to the
minute, for one more release. After that they are switched to the public
time or removed. Read `public*` instead now, and `working*` where a WTT time
is wanted:

| Where | Deprecated field | Use instead |
| --- | --- | --- |
| `JourneyStop` (train page `journeyStops[]`, journey legs) | `scheduledArrival`, `scheduledDeparture` | `publicArrival`, `publicDeparture`; `workingArrival`, `workingDeparture` |
| Trip-plan train legs (`GET /Trips/plan`) | `scheduledDeparture`, `scheduledArrival` (and their `departureDayOffset`/`arrivalDayOffset`) | `publicDeparture`, `publicArrival` (and `publicDepartureDayOffset`/`publicArrivalDayOffset`) |
| Timetable board rows, `/public/trains/search` rows, leg candidates | `scheduled`, `destinationArrival` | `publicDeparture`, `publicDestinationArrival`; leg candidates also `legPublicDestinationArrival` (below) |

A `public*` field is `null` where the call has no public time in that
direction (the departure of a set-down-only stop, the arrival of a
pick-up-only one), and until the next schedule publish for a schedule
stored before public times were.

### Changed meaning

**Per-stop delay, `JourneyStop.delayMinutes`** (train page, journey detail):
- Now measured against the public time, on the side of the stop's latest
  TRUST report: an arrival against the public arrival, a departure against
  the public departure.
- New `delayBasis` names the baseline (see the table below).
- A train arriving at Euston at 06:21, booked 06:19 working and 06:22
  public, now shows `-1`, where it used to show `2`.

**Per-stop status, `JourneyStop.status` (`Late`/`OnTime`) and `lateMinutes`:**
- The estimate is now compared with the public time (else the working
  time on a side with no public time).
- `estimatedArrival`/`estimatedDeparture` are unchanged: the working time
  plus TRUST's running delay.

**Train-level `delayMinutes`** is the delay at the passenger's own stop,
against the public timetable. It is measured once the train has reported
at that stop, and forecast before then. The forecast is the stop's working
time plus TRUST's running delay, compared with its public time, so a
terminus's recovery margin counts. Where it is served, and what "own stop"
means there:

| Endpoint | Field | Stop |
| --- | --- | --- |
| `GET /Train/{trackingId}` | `delayMinutes` | the pin destination |
| `GET /Train/mine` | `[].delayMinutes` | the pin destination |
| `GET /Journeys/mine` | `[].delayMinutes` | the current leg's destination |
| `GET /Journeys/{id}` (and its share-link twin) | each leg's tracked-train `delayMinutes` | that subscription's pin destination |
| `GET /groups/{id}/trains`, `GET /groups/shared-trains` | `delayMinutes` | the sharer's pin destination |
| `GET /groups/{id}/journeys`, `GET /groups/shared-journeys` | `delayMinutes` | the first leg's pin destination |
| `GET /Train/by-uid/{uid}/{date}` | `delayMinutes` | no stop of the user's own: the latest call the train reported at (an arrival or departure, never a pass); before any, TRUST's running delay with `delayBasis: "working"` |
| `GET /public/lines/{id}/trains` | `liveStatus.delayMinutes` | as `by-uid` |

Each of these rows gains:
- `delayBasis`: see the table below.
- `delayProvisional` (bool): `true` while `delayMinutes` is a forecast.

**Push notifications.** The notifier judges the 15-minute threshold on
the delay at each subscriber's own stop (their journey leg's destination,
else the pin destination), against the public timetable. The push copy
("now running about N minutes late") uses the same figure. A train 16
minutes late on the working timetable into a terminus with 2 minutes of
recovery margin no longer notifies; one late at a junction but recovering
before the subscriber's stop is judged at that stop.

**Delay Repay: `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
and `GET /Train/tickets/mine`:**
- `delayMinutes` is the delay against the public arrival at the ticket's
  destination. It was the train's latest working-timetable delay anywhere,
  passes included.
- The destination is the ticket's `destinationCrs`, else the tracked
  train's pin destination, else the train's terminus.
- Final once the train has arrived there. Before that it is a projection,
  `provisional: true`: the current delay carried to the destination's
  working arrival, compared with its public arrival.
- New on the response and on each ticket-list item:
  - `provisional` (bool);
  - `delayBasis`;
  - `measuredAtCrs`: where the delay was measured, `null` when no delay is
    known.
- `estimate` gains:
  - `provisional` (bool), the same as the response's.
  - `fareBasis`: `"single"`, or `"return"` in the 120-minute band.
- `estimate.disclaimer` for a provisional estimate starts with
  "Provisional: the train has not reached your destination yet…", and is
  otherwise the same caveat as before.
- New 120-minute band: `bandMinutes: 120`, `percentage: 100`,
  `fareBasis: "return"`, for both DR15 and DR30.
- The bands are DR15 15/30/60/120, and DR30 30/60/120.

**Trip planning, `GET /Trips/plan`:**
- The search, arrive-by deadlines, waypoint chaining and minimum change
  times now run on public times. A call with no public time in that
  direction falls back to its working time.
- Itineraries can shift by ±1–3 minutes, and a change that only worked
  on WTT can disappear (or the reverse).
- `totalDurationMinutes` is public-to-public.
- Leg `scheduledDeparture`/`scheduledArrival` are still WTT (deprecated,
  above); `publicDeparture`/`publicArrival` are what the search used.
- Live overlay, `legs[].live.delayMinutes`/`arrivalDelayMinutes`: against
  the public time. A TRUST-reported call uses the delay measured from that
  report's own fields; the CIF time is never subtracted from a TRUST
  actual.

**Journey leg candidates, `GET /Journeys/{journeyId}/legs/{legId}/candidates`:**
- A leg can now end at a set-down-only stop. Before, it could not be
  found.
- A leg never ends at a pick-up-only stop, and never boards at a
  set-down-only one.
- New per row: `publicDeparture`, `publicDestinationArrival`, and
  `legPublicDestinationArrival` (the public arrival at the leg's own
  destination, `"HH:MM"`, `null` when unknown).

**`/public/trains/search` with `stops_at`:** a pick-up-only call at
`stops_at` no longer matches (you cannot get off there); a set-down-only
one now does. Boarding rows are unchanged.

### `delayBasis` values

| Value | Baseline |
| --- | --- |
| `public` | TRUST's own public time for the movement (`gbtt_timestamp`) |
| `publicSchedule` | The public time from the CIF schedule at that call. Used when TRUST sent none: movements matched from the TRUST backlog, movements stored before 2026-10-01, and every forecast. Applied as TRUST's working time plus the schedule's `public - working` gap, never as a TRUST instant minus a CIF one. |
| `working` | The working timetable: the call has no public time in that direction (a pass, the departure of a set-down-only stop), or no schedule row is available |

### Unchanged

- Station boards' `delayMinutes` (LDBWS `etd - std`, already public).
- Line status, operator and network statistics (LDBWS departures).
- Full-coverage statistics: TRUST's `timetable_variation`, an operational,
  working-timetable measure.
- TRUST-to-schedule matching and `GET /public/trains/resolve`, which stay on
  WTT.

## 2026-10-01: public times, phase 1

Design: `docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md`
(§10 what phase 1 shipped).

All additive: no existing field changed meaning in phase 1. DS now stores
the public (GBTT) times and the passenger direction of every call from the
CIF schedule, and serves them next to the working-timetable (WTT) times.

**Null until the next publish.** The new schedule columns refill on the
next schedule-reference publish (and `trains.calling_points` for a train
matched after it). Until then a `public*` field is `null` even where the
call has a public time. A client must treat `null` as "not known", not as
"no public call".

### `scheduled*` stays WTT

`scheduledArrival`/`scheduledDeparture` (`JourneyStop`, trip-plan legs) and
`scheduled`/`destinationArrival` (board and search rows) keep their
meaning: the **working** time, truncated to the minute. They are kept for
one release and then switched to the public time or removed; phase 2
(above) deprecates them.

### `JourneyStop`: public times, working times, direction

On every `JourneyStop` (train page `journeyStops[]`, journey legs),
flattened onto the stop, not nested:

| Field | Type | Meaning | `null` when |
| --- | --- | --- | --- |
| `publicArrival` | RFC 3339 UTC instant | The public (GBTT) arrival: what a passenger timetable and the station screens show. | No public arrival here (the origin, a pick-up-only stop, a passing point), or the schedule predates the next publish. |
| `publicDeparture` | RFC 3339 UTC instant | The public departure. | No public departure here (the terminus, a set-down-only stop, a passing point), or the schedule predates the next publish. |
| `workingArrival` | RFC 3339 UTC instant | The exact WTT arrival, with `:30` seconds for a half-minute (`H`). | No booked arrival (the origin, a passing point). |
| `workingDeparture` | RFC 3339 UTC instant | The exact WTT departure, `:30` for a half-minute. | No booked departure (the terminus, a passing point). |
| `workingPass` | RFC 3339 UTC instant | The WTT pass time. Set only on a passing point (the train runs through without stopping), which has no other time. | Any call that is not a pass, and a pass in a schedule stored before the next publish. |
| `canBoard` | bool | A passenger can board here. `false` at a set-down-only (`D`) stop. | Never. |
| `canAlight` | bool | A passenger can alight here. `false` at a pick-up-only (`U`) stop. | Never. |
| `requestStop` | bool | A request stop (`R`): the train calls only if asked. | Never. |

- Every instant is dated on its own day. A stop that dwells across
  midnight has its departure on the next day. A public time that rounds
  across midnight (a 23:59H WTT arrival is a 00:00 public one) is on the
  next day too.
- For a schedule stored before the next publish, `workingArrival`/
  `workingDeparture` are rebuilt from the minute WTT time plus the stored
  half-minute flag. Where neither is stored (the
  `schedule_calling_points_full` fallback), they are the minute WTT time.
- `canBoard`/`canAlight`/`requestStop` are always booleans. Where the
  schedule predates the direction columns, DS fills them in as before
  phase 1: the origin is boardable only, the terminus alightable only, an
  untimed passing point neither, every other call both; `requestStop` is
  `false`.
- A client that may also talk to a server older than phase 1 sees the
  fields absent: read a missing `canBoard`/`canAlight` as `true`, a missing
  `requestStop` as `false`, and a missing `public*`/`working*` as `null`.
- Set-down-only (`D`) stops are now in the stored schedule, with
  `canBoard: false`; before phase 1 they were left out of it. `OP`
  (operational) and `N` (not advertised) stops are still left out.

### Board, search and trip-plan rows

Times here are local (Europe/London) clock times, like the `scheduled*`
fields beside them.

| Where | Field | Format | Meaning |
| --- | --- | --- | --- |
| Schedule-departure board rows, `GET /public/stations/{crs}/schedule-departures` | `publicDeparture` | `"HH:MM"` | Public departure at the board's station. `dayOffset` is the WTT departure's; the public time is minutes from it. |
| `GET /public/trains/search` rows | `publicDeparture` | `"HH:MM"` | Public departure at the searched station (`stationCrs`). |
| `GET /public/trains/search` rows | `publicDestinationArrival` | `"HH:MM"` | Public arrival at the train's destination (`destinationCrs`). |
| Trip-plan train legs, `GET /Trips/plan` (`kind: "train"`) | `publicDeparture` | `"HH:MM:SS"` | Public departure at the leg's boarding call. |
| Trip-plan train legs | `publicArrival` | `"HH:MM:SS"` | Public arrival at the leg's alighting call. |
| Trip-plan train legs | `publicDepartureDayOffset` | integer | Days past `serviceDate` of `publicDeparture`. |
| Trip-plan train legs | `publicArrivalDayOffset` | integer | Days past `serviceDate` of `publicArrival`. |

- Each `public*` time is `null` where the CIF has no public time for that
  call in that direction, and until the next publish for a schedule stored
  before public times were.
- A day offset is `null` exactly when its time is. It is usually the WTT
  time's own offset (`departureDayOffset`/`arrivalDayOffset`), one more
  where rounding crosses midnight.
- In phase 1 the trip planner still searched on WTT and attached the public
  times afterwards; phase 2 (above) moved the search onto them.

### Trip planning and direction

- The planner (`GET /Trips/plan`, including waypoints, arrive-by and the
  live overlay) never boards a train where `canBoard` is false and never
  alights where `canAlight` is false. It still rides through those stops.
- So an itinerary can now end at a set-down-only stop (before, the stop
  was not in the schedule at all), and one that alighted at a pick-up-only
  stop is gone.
