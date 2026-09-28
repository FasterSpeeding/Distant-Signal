# `/Trips/plan` live overlay (and waypoint chaining fix)

Status: implemented 2026-09-28. User decisions of 2026-09-28: fix the
waypoint-chaining bug, and add live data to `/Trips/plan`.

## 1. Problem

`GET /Trips/plan` searched the CIF timetable only. It could offer a train
that TRUST or Darwin already knew was cancelled today, plan a 4-minute
change onto a train whose feeder was running 20 minutes late, and showed
only booked platforms. Separately, in a multi-waypoint plan every segment
after the first searched from 00:00, so the onward train could leave before
the traveller reached the waypoint.

## 2. Waypoint chaining (fixed first, own commit)

Segment `n + 1` now searches from the earliest arrival among segment `n`'s
itineraries plus the minimum change time at the TIPLOC that itinerary
arrives at (`schedule_query::minimum_change_time`, the figure CSA and RAPTOR
charge for any change; the `NoInterchange` sentinel falls back to the
5-minute default because the traveller asked to stop there). The ready time
is carried in raw minutes and may pass 1440; the search then sees only this
service date's own post-midnight calls. A segment after one that found
nothing is CRS-validated but not searched. New additive fields: train legs'
`departureDayOffset`, and each segment's `departAfter: {time, dayOffset}`
(the time it was searched from; `null` when unchained).

## 3. What live data DS already holds

| Source | Table / module | Coverage (prod, 2026-09-28 07:25 BST) | Used for |
|---|---|---|---|
| TRUST train state | `trains` + `train_current_state` (status, `delay_minutes`, `updated_at`) | every activated train on a catalogued line: 7,984 `trains` rows today, 3,392 with state | whole-train cancellation, current delay |
| TRUST movements | `train_movement_events` via `journey::build_journey_stops_batch` | same trains | actual departure/arrival per stop; delay propagation (`apply_delay_estimates`) |
| TRUST cancellations / change of origin | `train_reasons` (`canx_type`, `loc_stanox`, reason code) + `stanox_crs` | 666 cancellations today | where a train is cut (EN ROUTE / OUT OF PLAN), new origin (0006), reason text |
| Darwin / LDBWS boards | `station_samples` (560 fresh boards) matched per stop by `stop_board` (RSID first) | sampled stations | `etd`, `isCancelled`, cancel/delay reason, live platform, observed time |
| Darwin skips | `trains.skipped_stations`, TRUST PASS (`StopStatus::Skipped`) | as above | a stop the train no longer calls at |

The overlay reuses the train-detail pipeline
(`build_journey_stops_batch` then the same stop rules
`/Train/by-uid/{uid}/{date}` uses) rather than a second interpretation of
the same feeds, so a leg's live status agrees with the train page.

## 4. Design

### 4.1 When it runs

* `?live=` (default `true`); `live=false` is byte-for-byte the pre-overlay
  response (no new keys).
* Chart kill-switch `api.tripPlanLive.enabled` (`TRIP_PLAN_LIVE_ENABLED`),
  default **true** (see 4.8).
* Only for service dates today or yesterday (London): a train running now
  belongs to one of those.
* Only for legs whose booked departure is between `lookbackMinutes` (120)
  before now and `horizonMinutes` (180) after now. Live data further out is
  mostly absent (TRUST activates about an hour before departure; boards list
  about two hours) and would only cost reads.

### 4.2 Per-train live profile

For each train leg's `(uid, serviceDate)`, one batched read gets its
`trains` / `train_current_state` row and its `train_reasons` rows (joined to
`stanox_crs` for the cut/new-origin TIPLOC); `build_journey_stops_batch` then
adds movements, board matches and skips. No row is ever created (the route
stays read-only). Per stop:

* **served** unless: `StopStatus::Skipped`; the stop's fresh board row says
  `isCancelled`; the train is wholly cancelled (`status = cancelled`, or a
  `train_reasons` 0002 with no state row) and the stop was not yet reached;
  it lies after an EN ROUTE / OUT OF PLAN cancellation location (ignored if
  TRUST reported the train at a later stop, i.e. it ran on); it lies before a
  change-of-origin location (ignored if the train was reported earlier).
* **departure delay**, first known of: TRUST actual departure; Darwin `etd`
  from the fresh, uniquely matched board row; TRUST-propagated estimate.
  **Arrival delay**: TRUST actual arrival, else TRUST-propagated estimate.
  A stop with nothing inherits the last known delay (the same forward-only
  propagation the train page uses); before any known delay, the timetable.
  Negative delays clamp to 0: planning a change on a train running early is
  a gamble, not information.
* **Staleness**: the TRUST delay is ignored when `train_current_state` was
  last updated more than `trustMaxAgeMinutes` (30) ago; boards older than
  `stop_board::BOARD_FRESHNESS` (10 minutes) are never matched. Actual
  times and cancellations are facts and do not go stale.

### 4.3 Adjusting the graph

A train whose profile differs from the timetable is withdrawn from the
cached day graph and replaced by its rebuilt chain of connections: only
between served stops, with the adjusted times (monotone). A wholly cancelled
train contributes none. This is `trip_planner::ConnectionOverlay`: the base
array is never copied; CSA/RAPTOR walk a merge of the base (minus replaced
UIDs) and the replacements.

### 4.4 Re-planning (bounded)

```
plan with the current overlay
  -> trains in the result (within horizon) not yet read?  no -> done
  -> read them (batched); overlay changed?                 no -> done
  -> replans == maxRounds (3)?                            yes -> done
  -> rebuild overlay, plan again
```

The first read also includes up to 20 trains booked to leave the origin in
the hour before `departAfter`: a late one may now be catchable. Trains read
after the last allowed replan are still annotated, and their legs flagged.
Worst case per request: `maxRounds + 1` searches per segment and
`maxRounds + 1` read batches, capped at `maxTrains` (60) trains read.

### 4.5 Response (additive, nullable)

Top level, only when `live` was requested:

```json
"live": {"applied": true, "reason": null, "replans": 1,
         "trainsRead": 7, "adjustedTrains": 2}
```

`applied: false` with a `reason` (`disabled`, `outsideLiveWindow`,
`unavailable`) when requested but not applied; legs then carry no `live`.

Each train leg, when applied: `"live": null` (outside the horizon, or no live
record) or

```json
"live": {"status": "Late", "cancelled": false, "delayMinutes": 12,
         "arrivalDelayMinutes": 9, "reason": "a signalling fault",
         "reasonSource": "darwin", "platform": "4",
         "observedAt": "2026-09-28T16:02:11Z", "interchangeFeasible": true}
```

* `status`: `Cancelled`, `Departed`, `Late`, `OnTime`, `Scheduled` (live
  record but no estimate), `Delayed` (Darwin says late, no time).
* `interchangeFeasible`: whether the change onto this leg from the previous
  train leg still works on live times (`null` for the first leg); false only
  when the replan budget ran out.
* `scheduledDeparture`/`scheduledArrival` stay the timetable times;
  `totalDurationMinutes` and chaining use live times.

Each itinerary, when applied: `"liveFeasible": bool` (no cancelled leg, no
infeasible change).

### 4.6 Cost

Timetable planning is unchanged (the graph cache and the four existing
bounds). The overlay adds per round: one `trains`+state query, one reasons
query, `build_journey_stops_batch` (4-5 batched queries plus one small
per-train calling-point read, since `trains.calling_points` is empty for
feed-created rows), and one extra search. Measured numbers are in the
implementation report.

### 4.7 Metrics

`api_trip_plan_live_requests_total{outcome}`
(`applied|not_requested|disabled|outside_window|unavailable`),
`api_trip_plan_live_replans_total`, `api_trip_plan_live_trains_read_total`,
`api_trip_plan_live_adjusted_connections_total` (base connections withdrawn
by the final overlay).

### 4.8 Default on

Default `enabled: true`: the overlay only removes or delays connections
that DS's own feeds confirm, the timetable answer is one query parameter
away, it degrades to timetable-only on any read error, and the reads are
the same small batched reads the train page does per view. The kill-switch
exists for an incident, not as a rollout gate.

## 5. Known limits

* Delays are applied only to trains the overlay read (those in a candidate
  itinerary, plus the origin look-back). A late train elsewhere that would
  newly make a connection possible is not discovered.
* No Darwin arrival forecast: LDBWS boards are departure boards, so the
  arrival delay at the alighting stop is TRUST-propagated or carried
  forward from the departure.
* Trains without a `trains` row (not on a catalogued line) have no live
  data; their legs read `"live": null`.

## 6. Provenance of ideas

* Designed from DS's own code: `stop_board` (RSID-first board matching),
  `stop_live_status` (status vocabulary), `journey::build_journey_stops_batch`
  and `apply_delay_estimates` (TRUST movement overlay and forward delay
  propagation), `train_reasons` (cancellation location and reasons).
* From Skye's train-mcp, as relayed by the coordinator (its source could
  not be opened in this session): the overall shape of turning board facts
  into adjusted connections and re-running the search for a bounded number
  of rounds, for legs within about two hours. No train-mcp or
  Distant-Signal-MCP code was read or copied.
