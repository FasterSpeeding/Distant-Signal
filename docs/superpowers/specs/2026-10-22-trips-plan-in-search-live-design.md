# `/Trips/plan`: later legs from live arrivals (in-search live data)

Status: implemented 2026-10-22 (DS-140), default on
(`api.tripPlanLive.inSearch`). Builds on
`2026-09-28-trips-plan-live-overlay-design.md`.

## 1. Requirement

A later leg must never depart before the real (actual, or estimated)
arrival of the earlier leg plus the minimum change time, and the search
itself, not only a check afterwards, should account for live delays when
it looks for the quickest route.

## 2. Audit (2026-10-22, before this change)

Where a later leg's earliest departure comes from an earlier leg's arrival:

| Place | Live-aware? | Finding |
|---|---|---|
| CSA / RAPTOR (`trip-planner` `csa`, `raptor`) | yes, for overlaid trains | Changes are charged the MSN / fixed-link time on the overlay's (live) times. Only the trains the overlay holds are live, though. |
| `/Trips/plan` re-plan loop (`routes::trips::plan_live`) | partly | The first search is timetable-only; live data is read only for trains of a plan already found (plus the origin look-back), then at most 3 (fastest) / 1 (options) re-plans. Three concrete failures, each now a DB test in `routes/trips/db_tests/live_in_search.rs`: **(a)** options: the one re-plan lands on a train that is itself cancelled, read only afterwards, so a cancelled leg is served (`liveFeasible: false`); **(b)** fastest: four cancellations in a row exhaust the three re-plans and the fourth is served; **(c)** a late onward train that now makes the connection (10:20 running 20 late, feeder in at 10:30) is never read, so the plan waits for the 11:00. |
| Waypoints (`staged`, joint plan) | yes, in the search | The joint search chains through waypoints on the overlay's times. But a later segment's `departAfter` echo was the timetabled ready time whenever the delayed change still worked (no re-plan, so the train was never overlaid). |
| Arrive-by (`reverse`) | yes, for overlaid trains | Honours the overlay; the first (timetable) answer could arrive after the deadline on live times and needed a re-plan from the budget. |
| Options mode | partly | One re-plan: finding (a). |
| Saved journeys, phase 0 (`journey_feasibility`) | yes | Judges each change on live arrival vs departure with the same `interchange_feasible` rule. No change. |
| Alternatives / break sweep (`journey_replan`) | yes | Searches from the previous leg's live arrival plus the change time (`Analysis::ready_for`), rejoin deadlines from the next leg's live departure, and plans through `plan_on_date` with live on, so it inherits this change. No defect found. |
| Add leg (frontend `journeyPriorArrival`) | no | Pre-filled the next leg's earliest departure from the timetabled arrival, deliberately. A late train pre-filled a window starting before the traveller gets in. |
| "Plan the rest" | n/a | There is no separate flow; it is the alternatives sheet above. |

## 3. Options compared

**A. Improve the post-hoc overlay**: more re-plans (options too), drop
infeasible itineraries. Cheap to write, but each extra round costs one
search plus one batch of per-train reads (options: +0.2-0.5 s a round),
every round still starts from a plan made without knowing the trains it
has not read, and finding (c) is out of reach by construction: no plan
ever contains the late train, so it is never read.

**B. In-search** (chosen). Before the first search, apply what TRUST knows
of every train of the date to the connection array, so CSA and RAPTOR
(and the staged and arrive-by searches built on them) find the quickest
route on live times directly. The per-train reads and the re-plan loop
stay, now only to correct what they know better than TRUST's running
state (Darwin board estimates and cancellations, skips, actual times).

## 4. Design

### 4.1 The snapshot (`trip_plan_live::LiveSnapshot`)

Per service date, two queries (`fetch_snapshot_states`, on the
`trains (service_date)` index): every train of the date that is
`cancelled`, has a `train_reasons` row (cancellation, cut, change of
origin), or is `en_route` with `delay_minutes > 0` and a state updated
within `trustMaxAgeMinutes`. Each becomes a `TrainLive` profile
(`snapshot_train_live`) by the overlay's own rules:

* **Time basis.** TRUST's delay is actual minus the working time. Each call
  gets `working + delay`, expressed as a delay against its public time
  (what the connections are planned on) and never below it: a public time
  padded later than the working one absorbs that much of the delay. The
  same pairing the per-train path uses for its estimates.
* **Forward only.** The delay applies to calls after the one last reported
  (`event_time - delay` is that call's booked working minute); calls
  already reached keep their timetable times.
* **Early running ignored** (delay <= 0 changes nothing).
* **No decay.** The delay carries to the end of the train undiminished.
  DS has no recovery model and the train page shows the same flat
  estimate; for the earlier leg of a change this is the conservative
  choice. The risk is the other side: an onward train assumed to stay late
  may recover and leave before the traveller arrives; the per-train read
  of the plan's own trains (Darwin `etd` where boards have one) and the
  journey page's phase-0 check remain the backstop. Open question in §6.
* **Staleness.** A delay older than `trustMaxAgeMinutes` is ignored;
  cancellations, cuts and changes of origin never go stale
  (`apply_reasons`, now shared with the per-train `train_live`).

`LiveSnapshot::build` does one pass over the day array for the trains'
chains (`chains_for`), evaluates each, and keeps only the trains whose
connections change, as a `ConnectionOverlay`.

### 4.2 The cached graph is never mutated

The overlay is per request on top of the shared, cached day array, as the
re-plans already were. The snapshot is cached per (date, graph) for
`snapshotTtlSeconds` (30 s) behind an async mutex (one build at a time; a
new graph or publish invalidates it). Each later overlay in the re-plan
loop is the snapshot with every train read one by one replacing the
snapshot's view of it (`combined_overlay`; a read train that turns out on
time is the timetable again).

A snapshot-sized overlay replaces thousands of trains, and the merge used
to hash every base connection's UID: RAPTOR depart-after +17%, arrive-by
+60% on the synthetic 400k-connection network. `ConnectionOverlay::indexed`
marks the withdrawn base positions in a bitset once (the snapshot, and each
re-plan's combined overlay); the searches then test one bit per connection.

### 4.3 Shown times

Unchanged contract: a train leg's `scheduled*`/`working*` are the
timetable; `live.delayMinutes` / `arrivalDelayMinutes` the live offsets;
itinerary totals and `journeys[]` times are live. `annotate` now finds a
leg's calls through the profile the search actually ran on (the
snapshot's, or a per-train read's) and reports the best profile known (a
per-train read beats the snapshot), so a leg planned on the snapshot's
12 late and read as 14 late shows 14 with its timetable times intact. A
later waypoint segment's `departAfter` is recomputed from the live
arrival after annotation.

### 4.4 Add leg

`journeyPriorArrival` pre-fills from the reported arrival, else the later
of the estimate and the timetable (early estimates ignored), else the
timetable. No change time is added (as before: the dialog does not know
the station's minimum change time, and a margin could hide a train).

## 5. Cost

Release builds on a loaded shared dev box (absolute numbers noisy; compare
within a row).

Search only (`trip-planner` `bench_live_overlay`: 27.9k trains, 401k
connections; 3,805 trains replaced, 41k replacement connections; median
over 10 pairs):

| search | no overlay | overlay by UID | overlay indexed |
|---|---|---|---|
| CSA depart 08:00 | 13.1 ms | 13.6 ms | 11.4 ms |
| RAPTOR depart 08:00 (4 rounds) | 543 ms | 636 ms | 535 ms |
| RAPTOR arrive-by 12:00 | 129 ms | 224 ms | 193 ms |

End to end (`api` `bench_in_search`: the 180k-connection synthetic day of
`bench_across_midnight`, TRUST state for 2,100 trains: 1,500 late, 600
cancelled; median over 8 pairs; snapshot warm):

| mode | timetable | live, re-plan loop only | live, in-search |
|---|---|---|---|
| fastest | 11.9 ms | 56.5 ms | 57.1 ms |
| options | 202 ms | 429 ms | 290 ms |

Snapshot: 8.7 ms to read 2,100 states, 61-111 ms to build over 180k
connections (once per 30 s per date); `combined_overlay` 2.5-5 ms per
re-plan round. In-search needed 0 re-plans where the loop alone needed
5 (fastest) and 6 (options) over the same pairs. Default on: no slower
than the loop for fastest, faster for options, and correct where the loop
was not.

## 6. Open questions

* **Delay decay / recovery.** Flat carry-forward is conservative for the
  earlier leg and optimistic for a late onward train. A cap (for example,
  plan onto a late onward train only within the per-train read's horizon,
  or only when Darwin confirms the `etd`) would trade found routes for
  safety; not done without a decision. *Decided 2026-10-22 (DS-141):* keep
  the delay (no recovery modelled) and flag the change instead; see §7.
* **Darwin-only facts** (board cancellations and estimates for trains with
  no fresh TRUST delay, skips) are still per-train, so the re-plan budget
  can still run out on them, leaving an itinerary flagged
  `liveFeasible: false` rather than removed.
* The snapshot reads `trains` rows only; trains not on a catalogued line
  stay timetable-only, as before.
* The numbers above are synthetic; `bench_trip_plan_live` against a
  production snapshot would confirm them.

## 7. Changes that rely on a late train (DS-141)

The live overlay keeps a late onward train late, so it can offer a change
that only works because that train is late. If the train makes up time,
it leaves before the traveller can reach it. Such a change is offered but
flagged.

**Definition** (`trip_plan_live::late_departure_slack`, one rule for the
planner and saved journeys). With `ready` the traveller's live arrival
(plus any walk) and `change` the minimum change time (or link plus MCT
for a saved journey's change between stations), the change onto a train
relies on its late departure when it works on the train's expected
departure and fails on its timetabled (public, else working) one, both by
`interchange_feasible`:

    expected >= ready + change   and   booked < ready + change

The inbound train's delay is part of `ready`: an inbound and an onward
train both late counts. Staying aboard one train never counts, and
neither does a first leg (no change; the traveller can just be early).

**The weaker case is not flagged.** A change that also works on the
booked time is not flagged, however little slack the timetable leaves:
if the train recovers, the traveller still makes it, so "it could leave
earlier" is no risk to them. A tight change is already covered by
`atRisk` (saved journeys) and the ranking's tight-slack penalty. A change
that fails on live times is infeasible, never "relied on".

**Live times used.** The planner judges it on the same times as
`interchangeFeasible`: the leg's best profile (a per-train read, else the
snapshot the search ran on), so the two never disagree. When a per-train
read finds the delay has shrunk, the flag follows the read; when it has
shrunk too far, the change is infeasible and the plan is re-planned.

**Where it is served** (all additive, absent when not set):

* `/Trips/plan`: `live.reliesOnLateDeparture` on the train leg changed
  onto, `{scheduledDeparture, expectedDeparture, delayMinutes,
  slackIfOnTimeMinutes}` (clock times like `scheduledDeparture`).
  Only inside the live window, like the rest of `live`.
* `GET /Journeys/{id}`: `connection.reliesOnLateDeparture`, same fields as
  instants. The `state` is judged as before (usually `ok`).
* Alternatives (`.../alternatives`, the sheet's saved sets): the same on
  each train leg (`legs[].reliesOnLateDeparture`), as instants.

**Ranking** (`alternative_ranking`, mid-journey changes spec §17.4): a
light `late_departure` penalty of 5 minutes. An option that relies on a
late train is never Recommended over an otherwise equal one that does
not, but one arriving more than 5 minutes sooner still wins.

**UI**: a calm note on the leg (planner, alternatives sheet) or the
connection (journey page, the home page's current journey for its next
change): "Relies on this train running late. It's due to leave at 10:35,
booked 10:20. It could leave earlier."
