# API changelog

Changes to the Distant Signal (DS) HTTP API that a client such as DS-MCP
needs to know about. Newest first. Field names are as served (camelCase).

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
