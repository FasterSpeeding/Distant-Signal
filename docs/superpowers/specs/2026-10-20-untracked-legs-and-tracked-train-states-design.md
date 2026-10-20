# Untracked journey legs and clearer tracked-train states (2026-10-20)

Status: design, implemented on the branch that adds this file. Work log
DS-66 (parts DS-67, DS-68, DS-69).

The request, verbatim:

> look into allowing the journey planner to keep track of journey legs done
> via transit we do not have live tracking or schedule data for like tube
> trains. as part of this handling we should improve how we display tracked
> trains with no matched schedule nor trcking data to distinguish them.
> another improvement is that travked trains with matched schedule entries
> should not display statuses like "Waiting to hear from Network Rail" and
> "Awaiting first report" until its we get to when we would first expect to
> get trust events or tracking data for that spevific train, allowing them
> to fitst show up as planned

Three parts:

- **A. Untracked legs.** A journey leg for a Tube ride, a bus, a walk, a
  ferry or anything else we have no data for. It has a mode, two place
  names, an optional line and planned times, and its status comes from the
  clock alone.
- **B. Trains we can't track.** A tracked train with no timetable match and
  no Network Rail report says so plainly, instead of looking like a live
  train that is about to report.
- **C. Planned until data is due.** A train matched to a timetable shows as
  planned, with its timetabled times, until the moment we would first
  expect Network Rail to report it. Only then does it fall back to the
  waiting copy.

Builds on `2026-09-22-journey-tracking-design.md` (journeys and legs),
`2026-09-12-journey-progress-visualization-design.md`,
`2026-09-22-reusable-repeating-journeys-design.md` (templates),
`2026-08-29-journey-ticket-tracking-design.md` and
`2026-10-14-ds-mcp-delegated-access-design.md` (ds-mcp saves itineraries).

## 1. What exists today

- `journey_legs` rows are all train legs. A leg is either bound to a
  `train_subscriptions` row (`match_mode` `manual` or `auto`) or an open
  time-window search (`unmatched`). `origin_crs`/`destination_crs` are
  nullable; `service_date` is not.
- `GET /Journeys/{id}` returns each leg with `trackedTrainState`, the same
  `TrackedTrainState` as `GET /Train/{trackingId}`, or `null` for an open
  leg. The frontend reads `trackedTrainState === null` as "needs a train
  picked" everywhere (`lib/journeyStatus.ts`, `JourneyLegCard`,
  `JourneyStatusBadge`, `journeyCanAddLeg`).
- A tracked train's displayed state comes from two columns:
  `train_subscriptions.resolution_status` (`pending`, `schedule_matched`,
  `resolved`, `unresolved`) and `train_current_state.status`
  (`awaiting_activation`, `en_route`, `cancelled`, `completed`, or no row
  at all).
- A train tracked by UID (`knownTrain`, the planner, ds-mcp) starts
  `pending` with its `trains` row known, so it has a stop list from the
  timetable at once. It stays `pending` until TRUST activates it. So a
  train booked for next week shows "Waiting to hear from Network Rail" with
  a spinner, and the journey badge says "Awaiting first report", for a
  week. That is part C.
- A train tracked by UID with no timetable entry for that date (a mistyped
  UID or date, or a train removed from the timetable), or a station and
  time pin that never matched, also shows "Waiting to hear from Network
  Rail". Nothing tells the traveller it will never report. That is part B.
- `STATUS_LABELS` in `TrackedTrainStatusBadge.tsx` shows "Pending match"
  for both of those, on `/track/mine`, the home page and group pages.

## 2. Part A: untracked legs

### 2.1 Data model

Additive columns on `journey_legs` (migration
`20261020150000_journey_legs_untracked.sql`), every one nullable or
defaulted, so a metadata-only `ALTER TABLE`:

| Column | Type | Train leg | Untracked leg |
|---|---|---|---|
| `kind` | `TEXT NOT NULL DEFAULT 'train'` | `'train'` | `'untracked'` |
| `transit_mode` | `TEXT` | `NULL` | one of the modes below |
| `origin_place` | `TEXT` | `NULL` | free text, up to 100 characters, optional |
| `destination_place` | `TEXT` | `NULL` | free text, up to 100 characters, optional |
| `line_name` | `TEXT` | `NULL` | free text, up to 60 characters, optional ("Victoria line") |
| `planned_departure` | `TIMESTAMPTZ` | `NULL` | required |
| `planned_arrival` | `TIMESTAMPTZ` | `NULL` | required, at or after `planned_departure`, at most 24 hours later |

An untracked leg also has `train_subscription_id = NULL`,
`match_mode = 'manual'` (the traveller entered it by hand and there is
nothing to search for or match, so the notifier's template sweep, which
reads only `match_mode = 'unmatched'`, never sees it), `window_searched =
FALSE`, `origin_crs`/`destination_crs` `NULL` and the four window bounds
`NULL`. `service_date` is the London date of `planned_departure`, so every
existing reader that keys on a leg's date (the template occurrence guard,
the `/Journeys/mine` row, the account export) keeps working.

Two `CHECK` constraints, added `NOT VALID` and validated by the next
migration (`20261020150100_journey_legs_untracked_validate.sql`, the
`line_status_transit_check` pattern):

- `journey_legs_kind_check`: `kind IN ('train', 'untracked')`.
- `journey_legs_untracked_shape_check`: a train leg has every new column
  `NULL`; an untracked leg has a mode, both planned times (arrival not
  before departure), no subscription, `match_mode = 'manual'`, no CRS
  codes, and text within the lengths above.

Modes (`transit_mode`, checked by the api, not the database, so adding one
needs no migration): `tube`, `bus`, `tram`, `rail`, `walk`, `ferry`,
`taxi`, `cycle`, `other`. Labels: Tube, Bus, Tram, Train (not tracked),
Walk, Ferry, Taxi, Cycle, Other. `rail` covers a train we have no data for
(a heritage line, Eurostar, a train abroad); a National Rail train belongs
in a train leg.

Places are free text. There is no TfL station lookup in Distant Signal
(the `/transit` networks are Metrolink only, and the TfL poller reads line
status, not stop points), so the form takes plain text. A later lookup can
fill the same columns. Places are optional because a walk "to the office"
needs neither; the frontend falls back to the mode and line ("Victoria
line", "Walk").

**Not added:** a per-leg free-text note, editing a leg in place (delete and
re-add, as for train legs), inserting a leg before an existing one (legs
stay append-only, ordered by `leg_order`). These are listed as open
questions (§9).

No new table, so `db-grants.yaml` is unchanged: `journey_legs` keeps its
table-level grants (api `SIUD`, notifier `SIU`). `gen-db-grants.py check`
still passes.

### 2.2 Status from the clock alone

Computed by the api at read time from `now` and the two planned times, and
sent as `untrackedStatus` (§2.4), so the server render, the client and
ds-mcp agree, and no client has to read the clock:

| State | When | Copy |
|---|---|---|
| `planned` | `now < planned_departure` | "Planned" |
| `inProgress` | `planned_departure <= now < planned_arrival` | "In progress (not live tracked)" |
| `done` | `now >= planned_arrival` | "Done" |

No other input: no delays, no cancellations, nothing live. A zero-length
leg (arrival equals departure) goes straight from `planned` to `done`.

### 2.3 Timeline, progress and connections

- An untracked leg is a card in the journey's leg list like any other,
  numbered "Leg N of M", with its mode, line, places and planned times, its
  state badge and the line "Times are planned. We have no live data for
  this leg." It never shows a stop list, a progress line or a "Change
  train" control.
- **Connections across mixed legs.** The connector between two legs names
  the change point and its time, read per kind: a train leg's arrival at
  its own destination, an untracked leg's `planned_arrival`. The gap to the
  next leg is measured from the arrival we now expect (a train's reported
  arrival, else its estimate, else the timetable: `legExpectedArrivalInstant`)
  to the next leg's timetabled departure (a train leg's public departure
  from its own origin, an untracked leg's `planned_departure`). When both
  are known the connector adds "12 min to change", or "The next leg leaves
  before this one arrives" when it is negative, so a late-running train
  shows a missed change. No minimum interchange time is assumed. The
  carry-over into "Add a leg" (below) still uses the timetabled arrival.
- **Carry over into "Add a leg".** The 2026-10-20 "start a new leg from the
  previous leg's arrival" change (`ea40c5ca`, DS-65, not yet on main when
  this was written, so this branch carries it) seeds the next leg's date
  and earliest departure from `journeyPriorArrival`. Its
  `legArrivalInstant` gains the untracked branch, so a rail leg added after
  a Tube leg starts from the Tube leg's planned arrival. A new untracked
  leg is seeded the same way: departure at the previous leg's arrival,
  arrival empty. Its "from" place is seeded from the previous leg's
  destination name.
- **Add a leg after an untracked leg.** `journeyCanAddLeg` used to require
  the last leg to have a matched train. An untracked leg counts as
  complete, so "Add a leg" shows after it.
- **Journey rollup.** `lib/journeyStatus.ts` gains `planned` (a train
  before its first report is due, part C), `noTimetable` (part B) and three
  untracked groups. Untracked legs rank below every train state, so a
  journey with any train leg is summarised by its trains; a journey of
  untracked legs only is summarised by the worst of those. New order:
  untracked (done, planned, in progress) < good < planned < awaiting <
  delayed < not in timetable < unmatched < skipped < cancelled.
- `/Journeys/mine`'s "current leg" (the first leg not finished) treats an
  untracked leg as finished once its planned arrival has passed, the same
  way a `completed` train is.
- **Start a journey with an untracked leg.** `POST /Journeys` accepts
  `mode: "untracked"`, and `/journeys/new` gains a third option, "Tube,
  bus or walk", for a journey that starts away from the railway.

### 2.4 API (additive)

**`POST /Journeys` and `POST /Journeys/{id}/legs`**: a new `mode`,
`"untracked"`:

```json
{"mode": "untracked", "transitMode": "tube", "lineName": "Victoria line",
 "originName": "King's Cross St Pancras", "destinationName": "Victoria",
 "plannedDeparture": "2026-10-21T08:05:00+01:00",
 "plannedArrival": "2026-10-21T08:20:00+01:00"}
```

`lineName`, `originName` and `destinationName` are optional; blank is
absent. 400s, as sentences: an unknown mode, a missing time, arrival before
departure, a leg longer than 24 hours, a departure more than 28 days ahead
(the tracking horizon, `PIN_MAX_DAYS_AHEAD`) or more than 7 days ago
(`TRACK_BY_UID_MAX_DAYS_BEHIND`), and text over its length. The journey's
20-leg cap and the per-user journey cap apply as for any leg. The tracked
train cap does not: an untracked leg tracks nothing. Response:
`trackingId: null`, as for a window leg.

**`GET /Journeys/{id}`**: every leg gains

- `kind`: `"train"` or `"untracked"`.
- `transitMode`, `lineName`, `plannedDeparture`, `plannedArrival`,
  `untrackedStatus`: set for an untracked leg, `null` for a train leg.
- `originName`/`destinationName` (existing fields) carry the untracked
  leg's place names, so any code that names a leg's ends reads one pair of
  fields whatever the kind. `originCrs`/`destinationCrs` stay `null`.

A train leg's existing fields are unchanged. An untracked leg's
`trackedTrainState` and `legSkip` are always `null`; clients must read
`kind` before treating `trackedTrainState: null` as "needs a train".

**`GET /Journeys/mine`**: each row gains `kind`, `transitMode`,
`lineName`, `plannedDeparture`, `plannedArrival` and `untrackedStatus` for
its current leg, and `trackingPhase` (§4.3).

**`POST /Journeys/itinerary`** (web planner and ds-mcp): a leg may carry
`"kind": "untracked"` with the untracked fields above instead of
`trainUid`/`serviceDate`. A leg with no `kind` is a train leg, exactly as
before; unknown fields are still refused. An itinerary still needs at least
one train leg, so the top-level `trackingId` (the first train leg's) stays
a number. Each `legs[]` entry of the response gains `kind`, and its
`trackingId` is `null` for an untracked leg. A request with no untracked
legs serialises, and so hashes for `Idempotency-Key`, byte for byte as
before, so a key claimed before the deploy replays correctly after it.

**Delegated saves may create untracked legs.** An untracked leg is inert:
no subscription, no push, no Network Rail lookup, no tracked-train cap, and
only the traveller's own text, stored as text and rendered as text. It
counts toward the same 20-leg cap and the 50 saves a day. That is the same
risk as a train leg, minus the live tracking, and it lets an assistant save
a door-to-door plan (a Tube hop to the terminus) as one journey. Delegated
delete rules are unchanged: only a journey whose `created_via` is the
client, not shared, within the daily limit. The deletion record's summary
gains the untracked fields so the Connected apps page can name the leg.

**Rejected on an untracked leg:** `GET .../candidates` and `POST
.../train` (400 "This leg isn't a train leg, so it has no train to
pick."). The database constraint backs this up: an untracked row cannot
hold a subscription.

**Templates.** "Save as a template" copies train legs only and skips
untracked ones; a journey with no train legs gets a 400 ("This journey has
no train legs to save as a template."). Templates are a repeating train
search, and a Tube leg has nothing to search for. "Track this journey
again" pre-fills from the first train leg, as before.

### 2.5 Notifications

Every push is keyed on a `train_subscriptions` row or on a template leg
with `match_mode = 'unmatched'`:

- Delay and cancellation (`candidates_for_trains_id`) fan out from
  `train_subscriptions`. An untracked leg has none.
- Station skips (`list_committed_legs_for_today`) join
  `train_subscriptions`. Excluded.
- "Today's occurrence needs a train" (`unmatched_auto_legs_for_commit_check`)
  reads template-materialised legs with `match_mode = 'unmatched'`.
  Templates never carry untracked legs and an untracked leg is `manual`;
  the query also gains `AND jl.kind = 'train'`, belt and braces.
- Journey copy ("Leg 2 of 3") counts every leg, untracked included, which
  is right: it is the leg's place in the trip.

So no push is ever sent for an untracked leg, and none is planned: we have
nothing to tell the traveller that they don't already know.

### 2.6 Account export and deletion

- Export: `journeys[].legs[]` is `to_jsonb(journey_legs)`, so the new
  columns appear with no code change. A DB test asserts it.
- Deletion: `journey_legs` rows go with their journey (`ON DELETE
  CASCADE`); nothing new to delete. Untracked legs create no other rows.

## 3. Part B: trains we can't track

A tracked train is **not in the timetable** when all of these hold:

- it is live-trackable (not a bus or ferry, which are timetable-only and
  keep their own display);
- Network Rail has not reported it: `resolution_status` is not `resolved`
  and there is no `en_route`, `cancelled` or `completed` status;
- we have no timetable for it: no `schedule_destination_departures` row for
  its UID and date, no calling points on its `trains` row, no UID at all
  for a station and time pin that never matched.

This is `trackingPhase: "noTimetable"` (§4). It is recomputed on every
read, so a train that gains a timetable later (a short-notice schedule, a
pin the sweep matches) moves on by itself.

Copy (style guide: terse, no em dashes):

- Badge: "Not in the timetable" (grape, `action`: the traveller should
  check something).
- Train page and journey leg heading: "Not matched to a timetable".
- Body, UID known: "We can't find train {UID} in the timetable for {date},
  so we can't track it. Check the UID and date. If they're right, it may
  have been cancelled or not published yet."
- Body, a station and time pin: "We can't find a timetabled train from
  {origin} at {time} on {date}, so we can't track it. Check the station,
  time and date."
- `unresolved` keeps its own existing copy ("Couldn't be matched to a live
  service"), which says the same thing for a pin the sweep gave up on.

Everywhere a tracked train appears: the journey leg card and journey badge,
`/track/mine` (journey rows and train rows), the home page's tracked rows,
group pages (train and journey rows, through `TrackedTrainStatusBadge`) and
both train pages (`/train/by-id/{id}`, `/train/{uid}/{date}`).

## 4. Part C: planned until the first report is due

### 4.1 When do we first expect data?

Network Rail's TRUST feed sends an Activation (0001) for a train before it
runs, then a Movement (0003) at its origin departure. We do not show a
train as live until one of those arrives (`resolution_status = 'resolved'`
on Activation; `status = 'en_route'` on the first Movement).

**Evidence.** Production, 2026-10-19 15:42 to 2026-10-20 15:43 UTC, every
Activation in `trust_event_backlog` (22,785 trains with a published
schedule), lead time = timetabled working departure from the train's own
origin (`schedule_destination_departures`, `origin_crs = true_origin_crs`)
minus when we received the Activation:

| Percentile | 1% | 5% | 10% | 25% | 50% | 75% | 90% | 99% |
|---|---|---|---|---|---|---|---|---|
| Minutes before origin departure | 44.6 | 58.6 | 59.0 | 59.1 | 59.8 | 59.9 | 59.9 | 60.0 |

None arrived after the timetabled departure; 2 of 22,785 arrived less than
30 minutes before it; 93 (0.4%) more than 2 hours before it. This matches
the `full-coverage-consumer`'s own `ACTIVATION_LEAD_MINUTES = 60` ("TRUST
activates about an hour ahead").

### 4.2 The threshold

Two instants, both from the **train's own origin** (the first timetabled
departure of the whole train, not the passenger's stop, because that is
when the train itself starts reporting):

- **Activation due** = origin departure minus **45 minutes** (99% of
  Activations have arrived by then). Before it, a train with a timetable
  and no Activation is *planned*. After it, still no Activation:
  "Waiting to hear from Network Rail", as today.
- **First movement due** = origin departure. Before it, an activated train
  with no movement yet is still *planned* (it is on time to leave). After
  it: "Awaiting first report", as today.

So `firstReportExpectedAt` is `origin departure - 45 min` until the
Activation arrives, and `origin departure` after. Origin departure is the
working timetable time (what TRUST reports against) on the service date,
in London time: `service_date + scheduled AT TIME ZONE 'Europe/London'`,
with the train's origin row preferred and the earliest departure row as a
fallback when the true origin has no station code (a depot start).

Departure from origin is the latest point at which "planned" can make
sense: once a train should have left, a missing report is worth saying.

### 4.3 States (`trackingPhase`)

A new, computed field on `TrackedTrainState` (`GET /Train/{id}`, journey
legs), `TrackedTrainListItem` (`GET /Train/mine`), `PublicTrainState`
(`GET /Train/by-uid/{uid}/{date}`) and `JourneyListItem` (`GET
/Journeys/mine`), with
`firstReportExpectedAt` (RFC3339 or `null`). Evaluated in this order:

| `trackingPhase` | When | Display |
|---|---|---|
| `timetableOnly` | a bus or ferry (`liveTracking: false`) | unchanged: "Timetabled only" |
| `live` | `status` is `en_route`, `cancelled` or `completed` | unchanged: live summary, delay, "Cancelled", "Arrived" |
| `noTimetable` | §3 | "Not matched to a timetable" |
| `planned` | a timetable, origin departure known, `now < firstReportExpectedAt` | "Planned", timetabled times, "We expect Network Rail's first report by {time}." |
| `awaitingReport` | everything else | unchanged: "Waiting to hear from Network Rail" (not activated) or "Awaiting first report" / "Waiting for its first movement report" (activated) |

`awaitingReport` is exactly today's behaviour, so the existing waiting
copy, the late-running display (a train whose `en_route` delay is known)
and the "may have arrived" inference all work as before once the threshold
passes. A `resolved` train with no timetable and no movement (activated
from live data alone) is `awaitingReport`, not `noTimetable`: Network Rail
has reported it.

`resolutionStatus` keeps its meaning; `trackingPhase` is derived, never
stored, and is what the frontend branches on for parts B and C. ds-mcp can
read it too.

Badge copy (`TrackedTrainStatusBadge`, list rows): `planned` "Planned"
(gray), `noTimetable` "Not in the timetable" (grape). Journey rollup:
`planned` "Planned" (gray), between `good` and `awaiting`.

### 4.4 Edge cases

- **After midnight.** Origin departure is computed on the service date, so
  a train that leaves its origin at 23:50 and reaches the passenger at
  00:20 next day is due its Activation at 23:05 on the service date, not
  the passenger's day. A train whose origin departure is 00:10 has service
  date = that day, and is due at 23:25 the evening before.
- **Cancelled.** A TRUST Cancellation sets `status = 'cancelled'`, which is
  `live`, whatever the clock. A train removed from the timetable before the
  day (an STP cancellation) has no timetable row and reads `noTimetable`,
  whose copy names cancellation as a possible cause.
- **Schedule matched late.** Every input is read per request; a pin the
  sweep matches, or a UID whose schedule is published later, moves from
  `noTimetable` to `planned` or `awaitingReport` on the next read.
- **Activation earlier than usual** (0.4% arrive more than 2 hours ahead).
  The train is then `resolved`; the threshold moves to its origin
  departure, so it stays `planned` until it should have left.
- **Clock skew or long page sessions.** The phase is computed on the
  server per request; the journey and train pages already refresh, so a
  page left open flips on its next refresh.
- **Delegated saves.** ds-mcp's saved train legs get the same fields; an
  untracked leg saved by ds-mcp follows §2.

## 5. Copy

All sentence case, no em dashes, ellipses or arrows
(`docs/style-guide.md`).

| Where | Copy |
|---|---|
| Untracked leg badge | "Planned", "In progress (not live tracked)", "Done" |
| Untracked leg card | "{Mode}: {Line}" heading ("Tube: Victoria line", "Walk"), "{from} to {to}", "08:05 to 08:20", "Times are planned. We have no live data for this leg." |
| Add a leg mode | "Tube, bus or walk" |
| Untracked form fields | "How", "Line (optional)", "From (optional)", "To (optional)", "Date", "Leaves", "Arrives" ("The next day." when it arrives after midnight), "Start journey" on `/journeys/new` |
| Connector | "Change at Euston, arrive 10:50", then ". 12 min to change." or ". The next leg leaves before this one arrives." |
| Train not in the timetable | heading "Not matched to a timetable", badge "Not in the timetable", body §3 |
| Planned train | heading "Planned", badge "Planned", body "We expect Network Rail's first report by 07:45." |

## 6. Coordination

- Another change (DS-61) is fixing "passed non-stopping stations shown as
  stops" in the stop list. This work does not touch stop-list building
  (`journey.rs`, `attach_journey_stops*`, `JourneyTimeline`); it adds
  fields to `TrackedTrainState` after the stop list is built, and reads
  only the timetable's origin departure.
- Another change owns journey names and previews (the shared "Origin to
  Final destination" helper). This work does not change journey labels;
  untracked legs expose `originName`, `destinationName`,
  `plannedDeparture` and `plannedArrival` so that helper can use them.

## 7. Tests

- Rust unit: the untracked request validation, `untracked_status`
  boundaries, `tracking_phase` for every row of §4.3 and the §4.4 edge
  cases, the itinerary body's backward-compatible hash.
- Rust DB (`--ignored`, as superuser and as the per-service role): create
  and read an untracked leg through `POST /Journeys`, `POST
  .../legs` and `POST /Journeys/itinerary` (web and delegated), the
  constraint refusing a bad row, candidates/train refused, export content,
  `/Journeys/mine` current-leg choice, the template promote skipping
  untracked legs, the batched origin-departure lookup.
- Frontend vitest: each untracked state, each `trackingPhase` display on
  `TrainJourney`, the badges, the rollup ranks, the connector gap, the
  carry-over from an untracked leg.
- Playwright (with `page.route` mocks): adding an untracked Tube leg; an
  unmatched train's display; a matched train showing "Planned" before its
  threshold. Visual baselines for changed pages.

## 8. Rollout

One release. Migrations first (two, additive, `NOT VALID` then
`VALIDATE`), then the api and frontend. An older frontend ignores the new
fields; it would show an untracked leg as an open leg, so the frontend and
api ship together. ds-mcp needs no change: every existing request and
response keeps its shape.

## 9. Open questions

1. Should the trip planner save its transfer legs (the cross-London Tube or
   walk between terminals it already plans) as untracked legs? It is a
   small change in `PlanTripFlow` now that the itinerary route accepts
   them; left out to keep this change apart from the in-flight planner
   work.
2. Editing an untracked leg in place, and inserting a leg before an
   existing one (so a journey can start with a Tube leg added later).
3. A TfL stop lookup for the place fields.
4. Should an untracked leg that runs past its planned arrival nudge the
   next train leg (for example "your Tube leg is planned to arrive after
   this train leaves")? Today the connector says so; nothing pushes.

## 10. As built (2026-10-20)

Differences from the design above, and things it left implicit:

- **Group pages.** `GET /groups/{id}/trains` and `.../journeys` rows do not
  carry `trackingPhase` yet; their badge keeps the old words ("Pending
  match"). A group's journey row now summarises the journey's first TRAIN
  leg (else its first leg), so a journey starting with a walk doesn't read
  as an unresolved train. Adding the phase there is a follow-up.
- **Journey titles.** The default title of a journey whose first leg is
  untracked reads "Unknown station, {date}" until the shared journey-name
  helper (another change) reads `originName`/`destinationName` and the
  planned times generically, which the API now provides for every leg.
- **Add a leg.** The three modes stack vertically in the dialog: side by
  side they cut "Search a time window" short.
- **Visual baselines.** `visual.spec.ts` shoots the seed's unresolved pin
  (not in the timetable), a train tracked by UID in 2099 (planned) and a
  named journey with a walk before that train, at both widths and schemes
  (12 new images). The ids come from insert order on a fresh database
  (`E2E_TRACKING_ID`, `E2E_PLANNED_TRACKING_ID`, `E2E_JOURNEY_ID`).
- **Tests.** Rust: `data::tracking_phase` (every row of §4.3, the
  after-midnight case, the batched lookup against a database),
  `data::journeys::untracked_tests`, the itinerary's validation and
  backward-compatible hash, and DB tests through the routes (create, add,
  detail, list, validation, export, delete, candidates and pick-a-train
  refused, the database constraint, templates, delegated save and delete,
  phases on every surface). Frontend: vitest for each display, Playwright
  for adding a Tube leg and starting with a walk (`page.route` mocks) and
  for the not-in-the-timetable and planned trains (seeded stack).
