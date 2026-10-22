# Pre-departure platform and delay prediction (2026-10-21)

Status: design. The user answered the open questions on 2026-10-22 (see
"Decisions" below); phases 0 and 1 are being built under DS-127. Work log
DS-123.

## Decisions (2026-10-22)

The user's answers to section 11. Where they differ from the text further
down, these win.

- **Q1, inferred previous working: yes.** The platform-sequence inference
  (same operator, terminating at the same platform 2-60 min before our
  departure) may be used and shown, always with a confidence level. Stated
  timetable association links replace it in phase 2.
- **Q2, notifications: yes, at high confidence only.** A predicted late
  start sends a push only when its confidence is `high`; otherwise it is
  display only. The push respects the user's notification settings (the
  train's and journey's own switches, quiet settings and push
  subscriptions) and is deduplicated with the existing delay pushes, so a
  user never gets a "late start" push and then a delay push for the same
  lateness. Because phase 1 has no association links, `high` is redefined
  for inferred links from the 2026-10-20 check (section 3), conservatively:
  - the incoming train has eaten all of the booked turnaround's slack
    (excess ≥ 0, so the predicted late start is at least `TURN_MIN`);
  - the excess is at most 15 min, below the point where the rule starts
    over-predicting;
  - the incoming train's delay comes from a live report (arrived at, or
    running towards, our origin) that is at most 10 min old, one link
    deep.
  On 2026-10-20 the excess 0-15 buckets held 325 pairs, of which about 85%
  started at least 3 min late.
- **Q3, new feeds: approved, subscriptions pending.** Darwin Push Port and
  the LDBWS arrival boards are approved, but the RDM subscriptions wait on
  the user (DS-126). Their ingests are not built now; they stay in phases
  2-3.
- **Q4, the incoming unit's arrival platform as "likely": the spec's
  recommendation is taken.** It is shown only as a `changed` platform with
  a "likely" qualifier. It needs the TRUST `platform` field kept by the
  backlog consumer, so it is left out of phase 1 and follows with the
  phase 2 association work.
- **Q5, naming the incoming train: the spec's recommendation is taken.**
  The wording says "the incoming train"; the provenance line links that
  phrase to the incoming train's own page (the API returns its uid and
  date). When the incoming train is cancelled, there is no prediction and
  nothing is said about it.
- **Q6, thresholds: the defaults.** Confidence cut-offs at 15 and 30 min of
  excess, a 3-hour horizon, and `TURN_MIN` of 4 min.
- **Q7, the compact booked style: a dotted underline.** A booked, not yet
  confirmed platform shows with a dotted underline and the accessible label
  "booked, not confirmed". A changed platform is highlighted (the existing
  blue style, with the booked one beside it).

## The goal

A tracked train shows next to nothing before it starts. Until Network
Rail's movement feed (TRUST) reports the run, all we show is the timetable.
This design covers what DS can say earlier, for every stop the user cares
about:

- **(a) Platform.** The booked platform from the timetable (CIF). A
  confirmed platform once one is known. A clear flag when the platform
  differs from the booked one.
- **(b) Late start.** An estimate of how late the train will start, and
  so how late it will be at the user's stops.

For (b) the working hypothesis is to use the train's **previous working**:
the earlier service the same unit runs, which ends at our train's origin.
If that train is running late and the turnaround cannot absorb the delay,
ours will start late.

Section 3 has prod measurements (aggregates only, taken read-only on
2026-10-21) that size each idea. The recommendation is in section 9.

## 1. What DS has today

### 1.1 Timetable (CIF)

- `schedule-ingest` takes the full CIF extract (`timetable_full.zip`, the
  MCA file) over SFTP. A bucket source is built but switched off.
  `schedule-query/src/parse.rs` decodes only `BS`, `BX`, `LO`, `LI` and
  `LT`. **`AA` association records are present in the file but dropped**
  (`parse.rs:282`, the catch-all, documented at `parse.rs:147-151`).
  `cif_check.rs` only checks that they are legal record types.
- `schedule_calling_points_full` holds the booked `platform` for every
  calling point, plus working and public times and the board and alight
  flags. It has no line or path column.
- `schedule_services` holds the headcode (train identity), `rsid`,
  operator, category and STP indicator. It does not hold the train service
  code, portion id, power type or timing load.
- The API already returns the CIF platform as `JourneyStop.bookedPlatform`
  (`ds-store/src/trains/types.rs`). It is deliberately kept separate from
  `platform`, and **no frontend component renders it**.

### 1.2 Movements (TRUST)

- `movement-relay` reads the Train Movements topic. `trust-consumer`,
  `trust-backlog-consumer` and `full-coverage-consumer` read its stream.
- They model activations (0001), cancellations (0002), movements (0003),
  reinstatements (0005), change of origin (0006) and change of identity
  (0007). Change of location (0008) is dropped.
- **TRUST movement messages do carry a `platform` field** (and `line_ind`).
  DS discards both at deserialise time, and `raw_body` is stored as `{}`.
  The field is filled only for some reporting points, and it describes a
  movement that has already happened. So it does not help with our
  train's own stops, but it could tell us which platform the incoming unit
  arrived in (section 6.4).
- **No feed DS takes carries a unit number or diagram.**
- Activations arrive about 60 minutes before origin departure (p50 59.7
  min, from the 2026-09-27 windowed-stats spec). Before then DS has
  nothing live for the run.
- `full-coverage-consumer` keeps a `trains` row and a `train_current_state`
  row (`delay_minutes`, `status`) for every train in the country, not just
  tracked ones: 31,085 `trains` rows for 2026-10-21. **So the previous
  working's live delay is already in Postgres. No new feed is needed for
  it.**

### 1.3 Live departure boards (`poller-ldbws`)

- It calls only `GetDepBoardWithDetails/{crs}?numRows=10`, with no
  `timeOffset`, `timeWindow` or `filterCrs`.
- It covers about 560 stations: the union of every line's sample stations.
  Each 60 s cycle polls them one after another within a 45 s budget, so a
  board is about 1-2 minutes old (measured: p50 56 s, p90 105 s).
- Per row it parses the service id, operator, destination, `std`/`etd`,
  cancellation, delay and cancel reasons, `platform`, `rsid`, and the
  subsequent calling points.
  - It does **not** parse `sta`/`eta`, previous calling points, length or
    formation.
  - The public board API has no confirmed flag and no hidden-platform
    flag. A hidden platform is simply absent, so DS cannot tell "hidden"
    from "not allocated".
- `planned_platform` is reconstructed in `platform_history.rs`: it is the
  first platform this poller process saw for `(crs, serviceID)`. It is held
  in memory only and lost on restart. A platform change made before the
  first poll is never flagged as a change.
- `station_samples` keeps only the latest board per station, with no
  history.

### 1.4 How a tracked train's stops get a platform today

- `journey.rs::apply_origin_platform` puts the platform from the board row
  the user picked, captured when they tracked the train, on the **origin
  stop only**. That platform is never refreshed by that path.
- `stop_board.rs::apply_station_sample_board` overrides any stop that has
  a uniquely matched, fresh (≤10 min) row on that station's latest board.
  It matches on RSID, then RSID prefix, then a ±2 min timetable match.
- So a live platform appears for one of the user's stops only when all of
  these hold:
  1. The station is one of the ~560 sampled.
  2. The train is within that board's 10 rows.
  3. The stop has a booked departure. **Boards list departures only, so a
     terminating or set-down-only stop never gets one. That is usually the
     user's destination.**
  4. The match is unique.

  This is why DS shows the platform later than it could.
- Delay before the start:
  - `eta_blend.rs` takes a numeric `etd` from the origin's board, labelled
    "Live departure board".
  - Otherwise it uses `apply_delay_estimates`, which adds the train's
    current TRUST delay uniformly. Before the first TRUST report that
    delay is unknown, so no estimate is shown.
  - There is no previous-working logic.
- The frontend's `PlatformBadge` styles:
  - unchanged: gray
  - changed: blue "9 (was 6)"
  - cancelled: red, struck through
  - nothing: when `platform` is null
  
  There is no booked, unconfirmed or confirmed state.

### 1.5 On-demand queries

Nothing calls `GetServiceDetails`, `GetArrBoardWithDetails` or a staff
board API. LDBWS is polled only by the rotating poller.

## 2. What each source could give

| Source | Have it? | Platform | Confirmed / hidden | Previous working | Forecast |
|---|---|---|---|---|---|
| CIF `LO/LI/LT` | yes | booked, every stop | no | no | no |
| CIF `AA` | in the file, dropped | no | no | next-working (`NP`), join (`JJ`), divide (`VV`) links, per date range and location | no |
| TRUST 0003 | yes | after the fact, some points only; discarded today | no | no | no |
| TRUST all trains (`train_current_state`) | yes | no | no | supplies the incoming train's live delay once a link exists | uniform delay |
| LDBWS departure board (public) | yes, ~560 stations | current platform for that station's departures | none; a hidden platform is just absent | no | `etd` at that station |
| LDBWS with `timeOffset`/`timeWindow` | same product, unused | same, for a chosen window | none | no | `etd` |
| LDBWS arrival board / service details | separate Rail Data Marketplace (RDM) products, not subscribed | arrivals: the station's platform for arriving trains; service details: that station only | none | no | `eta`/`etd` |
| Darwin Push Port | no | every location, live | yes: confirmed flag, public-display suppression, platform source (planned, automatic, manual) | yes: live association messages including next-working, kept up to date | per location, with Darwin's own knock-on |
| Unit or diagram data | not in any feed DS takes | | | the only exact source | |

**What the Push Port would add, and what it costs.**

- **Data:** per-location platform with a confirmed flag and suppression
  flags; platform alterations at every stop, not just the one whose board
  we poll; associations that follow real-time changes; and forecasts for
  every location.
- **Price:** Darwin is offered on RDM as a Kafka topic, and is believed to
  be free like the other Darwin and NR products DS uses. **Confirm in the
  RDM catalogue**; this is open question Q3.
- **Engineering, which is the real cost:**
  - a new relay like `movement-relay`, for a much higher message rate
    (millions of messages a day);
  - an XML schema (the Push Port schema, version 16 or later) with
    snapshot and resync handling;
  - identity mapping from the Darwin RID to our `(uid, service_date)`, which
    Darwin schedule messages give directly;
  - a table of current per-location state.
  
  Roughly the size of the TRUST ingest work: several PRs plus a chart
  change.

## 3. Prod measurements (2026-10-21, read-only, aggregates)

**Board freshness and reach.** Snapshot taken about 22:00 local, so in the
evening; peak-hour reach will be shorter.

- 565 boards, averaging 6.1 rows each.
- 172 boards were full (10 rows). Those reach only p10 18 min, p50 49 min
  and p90 88 min ahead. The other 362 reach p50 89 min.

**Platform present on board rows, by minutes to departure:**

| Minutes ahead | 0-10 | 10-20 | 20-30 | 30-40 | 40-50 | 50-60 | 60-70 | 70-80 | 80-100 | 100-120 |
|---|---|---|---|---|---|---|---|---|---|---|
| Rows with a platform | 98% | 97% | 98% | 98% | 89% | 91% | 89% | 77% | 72% | 70% |

**Stations whose boards hide platforms until near departure.**

- 85 stations show a platform for near trains but none for later ones.
- On those, the first hidden row is p10 42 min, p50 74 min and p90 100
  min ahead.
- This is one snapshot. It needs the history measurement in phase 0
  before we rely on it.

**Board platform against the CIF booked platform.**

- Matched on RSID, which only a minority of rows carry: 592 rows.
- 510 agree with the booked platform and 19 differ (3.5%). The rest lack
  one side.
- So a platform on the board is almost always the booked one. **Showing
  it does not mean it is confirmed.**
- Separately, the in-memory first-seen comparison flagged only 17 of 3,181
  rows as changed.

**Tracked trains.**

- Prod has 23 subscriptions from 3 users.
- Of the 22 train legs, the boarding station is on a sampled board for
  all 22, and the alighting station for 12.
- Even sampled destination stops get no platform when the train
  terminates there, because boards list departures only.

**Previous working inferred from the platform sequence.**

- **Rule:** among 2026-10-21 passenger origins with a booked platform
  (20,587 of 22,809), take the event just before the departure on the same
  station and platform in the timetable. Count it if it is a terminating
  arrival by the same operator 2-60 min before departure.
- **Coverage:** 16,061 (78%) qualify, which is 70% of all passenger
  origins.
- **Booked turnaround:** p10 6 min, p50 13 min, p90 33 min.
- **Ambiguity:** a looser rule (any same-operator arrival within 90 min,
  any platform) is ambiguous for 85% of origins. The platform sequence is
  what makes the inference usable.

**Validation of the late-start rule against what happened** (2026-10-20,
TRUST actuals, 15,780 inferred pairs, 14,178 with both actuals):

- **Excess** is the incoming train's arrival delay at the terminus minus
  the booked turnaround.
- **Predicted** is `max(0, excess + 3)`, the rule with a 3-minute minimum
  turnaround.

| Excess (min) | Pairs | Mean actual late start | Mean predicted | Rule's mean error | Error if we assume on time | Late ≥ 3 min |
|---|---|---|---|---|---|---|
| ≤ -3 (absorbed) | 13,339 | 0.1 | 0.0 | 0.8 | 0.8 | 5% |
| -3 to 0 | 478 | 2.9 | 1.8 | 1.8 | 2.9 | 49% |
| 0 to 5 | 222 | 6.5 | 5.6 | 2.5 | 6.6 | 89% |
| 5 to 15 | 103 | 10.0 | 12.0 | 5.0 | 10.2 | 76% |
| 15 to 30 | 29 | 15.1 | 23.5 | 10.9 | 15.1 | 72% |
| > 30 | 7 | 7.6 | 50.7 | 43.1 | 7.7 | 43% |

What the table shows:

- **Precision.** When the incoming train eats the whole turnaround
  (excess > 0, 361 pairs), 83% of our trains started at least 3 min late.
  The rule's mean error was 4.7 min, against 8.3 min for "assume on
  time".
- **Recall.** About 44% of all late starts (≥ 3 min) among inferred pairs
  fell in buckets the rule flags.
- **Calibration.** The rule under-predicts by about 1 min when the excess
  is small, so a 4-minute minimum turnaround fits better. It
  over-predicts beyond about 15 min of excess: operators swap stock or
  shorten turnarounds. Beyond 30 min it is mostly wrong, because the unit
  was very likely swapped.

This is one weekday and one heuristic. Phase 0 repeats it over a week,
and per operator.

## 4. Getting the previous working

| Option | Accuracy | Coverage | Effort |
|---|---|---|---|
| **A. Platform-sequence inference** from CIF: the last same-operator terminating arrival on the same booked platform, with no other departure in between | Good where it fires (validated above), but unverified per pair. Fails on unit swaps, stock sent to sidings, through platforms shared by two units, and splits or joins | 78% of passenger origins that have a booked platform | Low: one pass over `schedule_calling_points_full` at publish time |
| **B. CIF `AA` `NP` links** | Planned links, exact when they exist. Can be overridden on the day | **Unknown**: we drop `AA` today. Phase 0 counts `NP` links in one MCA file, offline | Medium: parse `AA` with STP overlay (associations have their own P/O/N/C layering and date ranges), plus a table |
| **C. Darwin Push Port associations** | Best: they follow on-the-day changes | National | High: needs the Push Port (section 2) |
| **D. Headcode family** (the same first two characters, arriving shortly before) | Weak; renumbering is common | Inconsistent | Low, but adds little over A |
| **E. Unit or diagram data** | Exact | Not available in any feed DS takes | n/a |

**Recommendation.**

1. Build **A** now. It needs no new feed and it validates acceptably.
2. Treat **B** as a strict improvement: where an `NP` link exists for the
   date it overrides A, and its confidence is higher. Do it once phase 0
   shows meaningful `NP` coverage.
3. **C** comes with the Push Port if the user takes it on (Q3).
4. Use D only as a tie-breaker inside A (prefer the same headcode family
   when two arrivals tie). Drop E.

**Storage.** A new `schedule_turnarounds` table:

- key `(service_date, uid)`;
- `prev_uid`, `prev_service_date`;
- `booked_turnaround_minutes`;
- `link` (`association` or `inferred`).

`schedule-ingest` writes it in the same publish as
`schedule_calling_points_full`. Reads become a key lookup, and the
association override is one more writer of the same table.

## 5. Delay rule

Inputs for our train T:

- P, its previous working (section 4);
- S, the booked turnaround: T's working origin departure minus P's working
  terminus arrival;
- `TURN_MIN`, the minimum turnaround: 4 min by default, one constant.
  Calibrate it per operator later.

P's terminus delay `A`, depending on P's state:

1. **Arrived** (a TRUST arrival at its terminus): its actual delay.
2. **Running:** its current `train_current_state.delay_minutes`. This is
   DS's existing uniform propagation, with no recovery assumed.
3. **Not started:** P's own predicted late start, recursively, at most two
   links deep. Each link lowers the confidence by one step.
4. **Cancelled, terminated short, or changed origin away from T's origin:**
   no prediction, because the unit is not coming. Show "incoming train
   cancelled" only if Q5 says so.

Predicted late start: `L = max(0, A - (S - TURN_MIN))`.

**Confidence:**

- **high:** the link is an association (B or C), the excess is ≤ 15 min,
  and P's data is fresh.
- **medium:** the link is inferred (A) and the excess is ≤ 15 min.
- **low:** the excess is over 15 min, P's data is over 10 min old, or the
  recursion is two links deep.
  - Show it qualitatively ("may start late"), never as a number.
  - Above 30 min of excess, show nothing. Validation says the unit was
    likely swapped.

**When to show it.**

- From when P has a live state, or from T's departure minus 3 hours,
  whichever is later.
- Until T's own data supersedes it. In order of precedence:
  1. TRUST movements for T;
  2. a numeric `etd` for T on its origin's board (already blended by
     `eta_blend`, and Darwin's forecast may include the same knock-on);
  3. our prediction.
- An origin board that says "On time" does not cancel a prediction of
  medium or higher. Darwin often shows "On time" until its own knock-on
  runs. Phase 0 measures how often the board catches up and how early.

**Propagation to the user's stops.** Feed `L` into the existing
`apply_delay_estimates` as the train's delay, with a new basis.

- Uniform, no recovery: the same contract as the TRUST estimate today, so
  every estimate in the timeline follows one rule.
- These estimates are labelled as predictions, and they never set actual
  times.

**Notifications.** None from predictions in phase 1. See Q2.

## 6. Platform rules

### 6.1 States per stop

| State | Source | Meaning |
|---|---|---|
| `booked` | CIF | The timetable's platform. Nothing live has confirmed it |
| `board` | LDBWS row matched to this stop | A station board is showing it. At stations that hide platforms until near departure, this is effectively the reveal |
| `confirmed` | Push Port confirmed flag (phase 3) | The platform has been confirmed |
| `changed` | Any live source whose platform ≠ CIF booked | Shown as the live platform, with the booked one beside it |
| `cancelled` | Board says cancelled at this stop | Existing behaviour, kept |

### 6.2 Changes from today

1. **Compare against the CIF booked platform, not the first-seen one.**
   `platformChanged` becomes `platform != bookedPlatform` when both are
   known. Fall back to the first-seen comparison only when there is no
   booked platform, for example a short-term plan without platforms. This
   catches changes made before the poller first saw the row, and it
   survives poller restarts.
2. **Show the booked platform before anything live is known**, in a
   visibly provisional style (section 8).
3. **Targeted board polls for tracked stops.** This is the main
   earlier-than-today gain without new feeds.
   - **Scope.** For each tracked train whose first user stop is within
     the next 2 h, poll the board for each user stop (boarding,
     alighting, and any change point) with `timeOffset` and `timeWindow`
     set narrowly around that stop's booked time. Add `filterCrs` set to
     the next stop when that helps uniqueness.
   - **What it fixes.** It reaches beyond the 10-row cap at busy stations
     and works at stations outside the ~560 sample set.
   - **Arrival boards.** For a stop where the train terminates or only
     sets down, use the arrival board. That is a separate RDM product
     (Q3); without it these stops stay `booked`.
   - **Cadence.** Every 2 min from T-120 min, every 1 min from T-20 min.
   - **Placement.** A second lane in `poller-ldbws` with its own budget
     share, writing to a new keyed table (`tracked_stop_boards`: train
     key, stop TIPLOC, row JSON, `polled_at`). `stop_board.rs` reads it
     before falling back to `station_samples`.
   - **Volume.** Today it is tens of requests an hour. It scales with
     tracked trains, not stations.
4. **Phase 3, with the Push Port:**
   - `confirmed` for every stop;
   - suppression known explicitly, so "hidden" and "not allocated" are
     different states;
   - platform alterations at every stop with no polling.
5. **Optional: the incoming unit's platform.** Keep the TRUST `platform`
   field from P's terminus arrival. If it differs from T's booked
   origin platform, that is an early hint that T will leave from there.
   Show it only as `changed` with a "likely" qualifier, and only if Q4
   wants it. It is cheap: one field kept in the backlog consumer.

## 7. API (additive)

Nothing is removed or renamed. Every addition goes in
`docs/api-changelog.md`.

**`JourneyStop`, new fields:**

- `platformBasis: 'booked' | 'board' | 'confirmed' | null`: what the shown
  platform rests on. Clients that ignore it keep working: `platform` stays
  the live value only, and `bookedPlatform` stays the CIF value.
- `displayPlatform: string | null`: the platform to show. This is the live
  `platform` if known, else `bookedPlatform`. A server-side convenience,
  so clients apply the same precedence.
- `platformChanged` keeps its name. Its meaning changes as described in
  section 6.2: the comparison is against booked first.
- `platformStatus` gains no new value. "Provisional" is expressed by
  `platformBasis = 'booked'`.

**Train level** (`GET /Train/{id}`, the by-uid endpoint and journey legs),
a new `predictedStart`, nullable:

```json
{
  "lateMinutes": 7,
  "confidence": "medium",
  "basis": "previous-working",
  "computedAt": "...",
  "previousWorking": {
    "trainUid": "...", "serviceDate": "...",
    "link": "inferred",
    "status": "en_route",
    "delayMinutes": 12,
    "observedAt": "...",
    "bookedTurnaroundMinutes": 9
  }
}
```

`lateMinutes` is null at low confidence.

**Other changes:**

- `EtaSource` gains `'previous-working'`. Clients must treat an unknown
  value as a generic estimate. Check `EtaBadge` and the DS-MCP clients
  first.
- Stop estimates produced from a prediction carry the same source.

## 8. UI states

Platforms (`PlatformBadge`). Every state is told apart by text, never by
colour alone, and stays at AA contrast.

| State | Look | Words (full / compact) |
|---|---|---|
| booked | gray outline (not filled), dimmed text | "Platform 4 (booked)" / "4?" or "4" with a dotted underline and the tooltip "Booked platform, not yet shown on station boards" |
| board | gray light (as today) | "Platform 4" |
| confirmed | gray light plus a check glyph | "Platform 4, confirmed" |
| changed | blue (as today) | "Platform 9 (booked 6)" / "9 (was 6)" |
| cancelled | red struck through (as today) | unchanged |

The Platform column in `JourneyTimeline` appears when any stop has a
`displayPlatform`, so booked platforms make it appear from the start.

Late start (journey leg card and train page), shown while the train is
"Planned" (the state from the 2026-10-20 states spec):

- **medium or high:** orange "Expected to start about 7 min late". The
  provenance line reads "Based on the incoming train, running 12 min
  late". Estimated times at the user's stops are shown with the existing
  estimate styling and the source "Incoming train".
- **low:** yellow "May start late". The provenance line explains: "The
  incoming train is running 25 min late".
- **None, or superseded:** today's behaviour.

## 9. Phased delivery

**Phase 0. Measure. No user-facing change.**

- Count `NP`/`JJ`/`VV` associations in one MCA file, offline, by
  operator.
- Rerun the section 3 validation over 7 days and per operator. Fit
  `TURN_MIN`.
- Make the poller emit a histogram of first-seen platform lead time per
  station (a metric, not a table), to learn reveal times.
- Measure how often and how early the origin board's `etd` already
  reflects the knock-on that our rule predicts.

**Phase 1. Existing feeds only. Recommended first.**

1. Platforms:
   - render the booked platform as provisional;
   - add `platformBasis` and `displayPlatform`;
   - compare against the booked platform for `platformChanged`.
2. Targeted board polls for tracked stops, departure boards only.
3. `schedule_turnarounds` from the platform-sequence inference.
4. `predictedStart` with the section 5 rule, shown as in section 8. No
   notifications.

**Phase 2. CIF associations.** Parse `AA` with the STP overlay. `NP`
overrides the inferred links. `JJ`/`VV` mark portions, so the UI can say
"joins/divides at X" and we skip predictions where a split makes the unit
ambiguous.

**Phase 3. Darwin Push Port**, if Q3 is yes:

- the `confirmed` state and explicit suppression;
- live associations;
- per-stop platform alterations everywhere;
- the arrival-side platform for terminating stops;
- the targeted polls retire.

## 10. Risks

- **A wrong inference.** The platform sequence can pick the wrong unit:
  stock to sidings, two units sharing a long platform, splits and joins,
  or a swap on the day. Mitigations:
  - confidence bands and "expected" wording;
  - nothing above 30 min of excess;
  - no notifications;
  - associations override the inference.
- **A booked platform read as a promise.** It must look provisional. The
  data shows a board platform is the booked one 94% of the time, so
  `board` mostly means "still the booked platform". Wording must not
  imply confirmation before phase 3.
- **The LDBWS request budget and terms.**
  - Targeted polls add a lane, so give it its own cap.
  - Arrival boards and service details are separate RDM products.
  - Terms are assumed fine (standing user decision), but the product
    subscriptions are new.
- **Cost of a read-time lookup.** Precomputing `schedule_turnarounds` keeps
  the API to key lookups. P's state is one more row from
  `train_current_state`.
- **Darwin already applying the knock-on.** When the origin board shows a
  numeric `etd`, it wins. The risk is two different estimates in two
  places; the precedence in section 5 settles it.
- **Data for one day only.** The validation is a Tuesday evening snapshot
  plus one day of pairs. Phase 0 widens it before any number is shown.

## 11. Open questions for the user

- **Q1.** Is an *inferred* previous working acceptable to show, labelled
  "based on the incoming train", or only planned links (phase 2)?
- **Q2.** Should a predicted late start ever notify? Suggested answer:
  not in phase 1, and later only at high confidence.
- **Q3.** Should we subscribe on RDM to the Darwin Push Port (needed for a
  real "confirmed" state) and to the arrival board product (needed for
  platforms at terminating stops before phase 3)? Please confirm whether
  either has a charge.
- **Q4.** Should we show the incoming unit's arrival platform as a
  "likely" platform for our start (section 6.2, item 5)?
- **Q5.** Should the UI name the incoming train (a link to its page) and
  say when it has been cancelled, or just say "the incoming train"?
- **Q6.** Thresholds: confidence cut-offs of 15 and 30 min of excess, a
  3-hour horizon, and a `TURN_MIN` of 4 min. Accept these defaults, to be
  tuned by phase 0?
- **Q7.** The compact booked style: "4?" or a dotted underline?
