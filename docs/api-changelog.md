# API changelog

Changes to the Distant Signal (DS) HTTP API that a client such as DS-MCP
needs to know about. Newest first. Field names are as served (camelCase).

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
