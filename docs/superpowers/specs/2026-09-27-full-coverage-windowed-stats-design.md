# Design: windowed full-coverage stats (recent window + day-to-date)

> **Status (2026-09-28):** branch `wt-fc-windowed-impl` has been merged into
> `main` (merge commit `9872c278`); every switch is still off by default.

**Status: implemented (branch `wt-fc-windowed-impl`), every switch off by
default. The "Decisions (2026-09-27)" section below overrides the rest of
this document where they differ; "Implementation notes" at the end records
what the implementation changed and why.**
Base: local `main` at `c3d5a0fc` (includes the 2026-09-27 full-coverage
restart fix: startup replay, partial days, population gating, date-keyed
`full_coverage_line_stats`, exact gap detection).

Required background, read before this document:

- `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
  (the presentation scaffolding: `FullCoverageAvailability`,
  `merge_full_coverage`, `escalate_from_coverage_stats`).
- `docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`
  (the consumer: Decisions 2d "unconfirmed = cancelled" and 2e
  "Pending until the rail day closes").
- `/home/coder/ds-review/full-coverage-lag-2026-09-27.md` (restart damage,
  memory history).

All production numbers below were measured read-only on 2026-09-27 between
03:30Z and 05:00Z (SELECTs through `kubectl exec ... psql`, Prometheus
through the apiserver proxy). The analysis script is committed as
`scripts/fc-windowed-analysis.py`; its header lists the exact export
queries, so every number here can be re-derived.

## Decisions (2026-09-27)

The user's answers to section 10's open questions, applied in the
implementation:

1. **Escalation tiers (open question 1).** `enforce` escalates only to
   **Severe Delays and Part Suspended**:
   `FULL_COVERAGE_WINDOW_MIN_ESCALATION_RANK` defaults to 4, the Severe
   tier (`common::full_coverage_window::FULL_COVERAGE_WINDOW_DEFAULT_MIN_ESCALATION_RANK`).
   The lower tiers are **computed and stored anyway**: in both `shadow` and
   `enforce` the aggregator writes one row per line per 15-minute bucket to
   `full_coverage_window_verdicts` (migration `20260927120200`), with
   `would_escalate_to` (the window's tier when strictly worse than what the
   line was showing) and the flag `below_min_rank` (a Minor Delays /
   Reduced Service would-escalation held back by the gate), plus
   `enforced`. `compare_full_coverage --windows` reports enforced vs
   would-escalate tiers, so widening the enforced tiers later is an
   evidence-based config change.
2. **Pilot lines (open question 2).** `FULL_COVERAGE_WINDOW_ENFORCE_LINES`
   is kept, but its default is **empty**, not `*` (this replaces the `*`
   in section 8.1): switching to `enforce` enforces nothing until lines are
   named. The operator picks about 5 lines of different volumes from the
   shadow report; its section 6 lists per-line volume and suggests five
   clean candidates near daytime medians of 6, 10, 20, 30 and 45 trains per
   window. `*` still means every full-coverage-enabled line.
3. **"Delayed" (open question 4).** A train is delayed when it is **at
   least 3 minutes late at its first calling point on the line**, from
   TRUST's `timetable_variation`: the delay of the earliest report that
   shows the train on the line (a report at one of the line's TIPLOCs, or
   planned at or after its due time -- the line's first station is not
   always a TRUST reporting point). This replaces section 4.3.1's "maximum
   reported delay among reports at L's stations" and section 5's "line's
   merged `delay_threshold_minutes` (5)". The threshold is
   `Defaults.full_coverage_delay_threshold_minutes`, default
   `FULL_COVERAGE_DELAY_THRESHOLD_MINUTES = 3`, overridable per line through
   `severity_overrides`. **Every share in section 3 was measured at 5
   minutes, so expect more delayed trains** (and more Minor/Severe Delays
   verdicts) than this document reports; the shadow report is the new
   baseline. LDBWS keeps 5 minutes, so full coverage's late rates run
   higher by construction.
4. **Minimum sample (open question 5).** 6 evaluable trains per 60-minute
   window (`full_coverage_min_sample_size`), and 3 affected trains per tier
   (`full_coverage_min_affected`), as section 5. **Superseded for the
   Severe tiers by "Decisions (2026-10-02)" item 1: 5 affected trains.**
5. **No frontend or UI change (open question 3).** Day-to-date numbers are
   computed and stored (`full_coverage_line_window_stats`,
   `window_kind = 'day_to_date'`, and the v2 `full_coverage_line_stats`
   row) but not shown.

Also fixed first, as separate commits, because they damage the existing
closed-day rows too (each with a regression test): `delayed` was always 0
(the LATE delay now comes from `timetable_variation`); `apply_cancellation`
ignored trains with no matched movement; the next day's Activations were
wiped at the rollover (and never replayed after a restart); and
rail-replacement buses and ships were counted as cancellations (they are left
out once the population carries `train_status`).

## Decisions (2026-10-02)

A shadow evaluation in production (4.3 weekdays) found Severe firing on
3.53% of judgeable windows (§8.4's limit is 3%) and 8 lines Severe in more
than 20% of their daytime windows. The cause was the 3-minute "late"
threshold combined with a 3-affected-train minimum: 3 of 6 trains at 3+
minutes late is 50%, which read as Severe Delays. The user's decisions,
which override the rest of this document where they differ:

1. **Calibration.** The Severe-rank tiers (Part Suspended, Severe Delays
   by lateness or by skipped stops) need **at least 5 affected trains**:
   `Defaults.full_coverage_severe_min_affected`, default
   `common::full_coverage_window::FULL_COVERAGE_SEVERE_MIN_AFFECTED = 5`,
   overridable per line through `severity_overrides` like the other keys.
   It is never below `full_coverage_min_affected`. The Minor Delays /
   Reduced Service tiers keep 3. The 3-minute late threshold stays, and
   rank 3 stays off for enforcement (`minEscalationRank: 4` is the pilot
   setting). So "3 of 6 late" is now Minor Delays (recorded, not
   enforced), and "5 of 8 late" is Severe Delays.
2. **Memory.** The shadow run measured a consumer working set of
   430–570 MiB with windowed stats on, bounded and reset at each rail-day
   rollover. This is accepted. The resource target in §8.4 item 6 is now
   **~650 MiB** (was 400 MiB). The chart's
   `fullCoverageConsumer.resources.requests.memory` is raised from 512Mi
   to 640Mi to cover it. The limit stays 1 GiB.
3. **Rail-day boundary (fixed before the pilot).** Populations are
   published per CIF service date, and the consumer only counted the
   current service date's population. Rail day D runs from 02:00 London
   on D to 02:00 on D + 1, so D + 1's trains due between local midnight
   and 02:00 were in no window when due (D's population did not hold them)
   and were outside D + 1's day-to-date and closed-day ranges (which start
   at `rail_day_start(D + 1)`): about 0.6% of trains. Evidence: the
   Elizabeth line on 09-30 had 678 relevant trains against a closed-day
   row of 675. **Rule: a train belongs to the rail day its due time falls
   in.** Every window of rail day D now counts D's population (judged on
   `TrainState::current`) plus D + 1's trains due before
   `rail_day_start(D + 1)` (judged on `TrainState::next`, where their
   Activations already go). This was chosen over re-keying the population
   by rail day: `schedule-reference` publishes per service date, the
   reloader already holds D and D + 1, and the TRUST state is already
   split the same way, so nothing upstream or in the cache changes. The
   closed-day row of D is written at D's close, when those trains are all
   due. If D + 1's population is not held, any window reaching local
   midnight is `partial` (so a closed day cannot read `available` with
   those trains missing). Remaining gaps, both outside any evaluable
   daytime window: D's own trains due after `rail_day_start(D + 1)` (a
   few sleepers' first calls) are still counted nowhere, and for about 70
   minutes after the rollover the `recent` window does not see D's
   after-midnight trains (their TRUST state is dropped at the rollover).
4. **Only statuses in effect now are raised (fix C).** Shadow showed
   `enforce` would have escalated planned-works notices that were not in
   effect (for example "Buses replace late night trains ... from Monday to
   Thursday", whose validity spans whole days). `enforce` and the existing
   LDBWS escalation (Layer 2) now raise only statuses in effect now, and
   the line's "current severity" that a window verdict must beat is the
   worst of those statuses alone (Good Service when there are none).
   "In effect" is `aggregation::in_effect_now`: `validity.is_now`, or, for
   anything but a planned notice, a validity period covering now.
   `poller-incidents` publishes `is_now = false` for every notice with an
   end date, so a bounded planned notice is never in effect. In production
   on 2026-10-02 every planned status had `is_now = false`, and every
   Knowledgebase, LDBWS and TfL status had `is_now = true`. Planned
   notices stay exactly as published. When nothing on a line is in effect,
   live data gets its own status instead of being attached to the notice:
   Layer 2 adds its LDBWS-inferred status if the samples show worse than
   Good Service, and `enforce` adds a `TrustInferred` status (disruption
   source `full-coverage-window`) for an enforced verdict. The legacy
   whole-day merge (`merge_full_coverage`) is unchanged; it never matches
   (§1) and `enforce` bypasses it for the lines it enforces.
5. **Custom lines are not full-coverage lines.** Users' custom lines
   (`custom-milton-keynes-drain`, `custom-west-barnes-drain` in production)
   counted as `missing` every cycle: `FULL_COVERAGE_ENABLED_DEFAULT=true`
   enabled them, but `full-coverage-consumer` only covers the
   `lines/*.toml` catalogue. The aggregator now leaves every custom line
   out of full coverage (`full_coverage_window::full_coverage_lines`): no
   window verdict, no `missing` count, no `Pending` under `enforceLines:
   "*"`, and the legacy merge leaves them `NotEnabled`, which is what
   `CustomLine`'s `From` impl already intended.
6. **Production values are not changed by this work.** The pilot values
   (after a clean 7-day re-shadow including a weekend) are in the
   2026-10-02 fix report: `enforce` on `gwr-windsor-branch`,
   `scotrail-cathcart-circle`, `swr-chertsey-loop`,
   `lnwr-birmingham-crewe`, `greater-anglia-west-anglia` and
   `elizabeth-shenfield`, with `minEscalationRank: 4`.

## 1. Problem

Full coverage today produces one number per line per rail day:
`stats::build_line_row` (`crates/full-coverage-consumer/src/stats.rs:51`)
takes the line's whole-day population as its denominator and counts every
population UID with no correlated TRUST state as cancelled.

1. **Not-yet-due trains read as cancelled.** At 02:01Z (one minute into the
   rail day) every row reads `cancelled = total`, and the row only becomes
   meaningful at the very end of the day.
2. **So the aggregator only trusts closed days**, and its read can never
   match. `load_full_coverage_line_stats`
   (`crates/aggregator/src/queries.rs:254`) requires
   `availability = 'available' AND service_date = <UTC today>`. A day's
   `available` row is only written once the day has closed, at
   01:00Z/02:00Z on the *next* UTC date, so `service_date` is always
   yesterday by the time it is `available`. `merge_full_coverage`
   (`crates/aggregator/src/aggregation.rs:1439`) therefore always takes the
   `None` branch and marks the line `Pending`. The one opted-in line
   (`lines/tfw-conwy-valley.toml`, `full_coverage_enabled = true`) has never
   been escalated by full coverage.
3. **Even if it matched, a whole-day number is the wrong signal for live
   severity.** A morning meltdown would still be escalating the line at 23:00,
   and an evening meltdown would be invisible until the next day.

LDBWS sample stats, by contrast, describe "the board right now"
(`aggregation.rs:990-1010`, `compute_sample_availability`), and escalate an
incident-derived severity only upwards (`escalate_from_sample_stats`,
`aggregation.rs:1260`).

## 2. Goal

Replace the whole-day stat with three views per line:

| View | Question it answers | Used for |
|---|---|---|
| **recent** | Of the trains due on this line in the last *W* minutes (ending *grace* ago), how many ran on time, late, or were cancelled? | live severity, escalate-only, behind a flag |
| **day-to-date** | Same, over every train due since the rail-day start, up to now − grace | context (API/frontend), audit trend |
| **closed day** | Same, over the whole rail day, written once it closes | audit record (existing `full_coverage_line_stats` row) |

The key change is that **only trains that are already due are counted**, and
"no event seen" only means "presumed cancelled" once a train is past its due
time plus a grace period and has not even been *activated*.

## 3. What production shows (measured 2026-09-27, rail day 2026-09-26)

Sources: `trust_event_backlog` (every TRUST 0001/0002/0003/0005 the relay
saw, national, ~24 h retention; exported at 03:37Z, so it covers 09-26
from 03:36Z to 09-27 02:03Z), `schedule_calling_points_full`,
`schedule_line_population`, `schedule_destination_departures`
(`operator_atoc`), `stanox_crs`, `line_status_half_hourly_stats`,
`line_status_history`, the CIF `BS` records of `RJTTF971MCA.txt` on the
schedulefeed volume, and the last 200k entries of the `movement-events`
stream (18:19Z–04:20Z). Trains due before 06:00Z are excluded because
their activations may predate the export. Prometheus could not supply
history: its TSDB head starts at 04:02Z on 09-27 (it lost its data in the
reboots), so memory figures come from the review and the live pod.

### 3.1 Current output is not usable

- `full_coverage_line_stats` at 04:23Z: 243 rows for 2026-09-27, all
  `pending`, `total = cancelled = 170,157`, `delayed = 0`. Conwy Valley:
  102/102 cancelled.
- **`delayed` is structurally always 0.** `trust_schema::journey::variation_to_minutes`
  returns `None` for `LATE` ("caller overwrites with a real value",
  `crates/trust-schema/src/journey.rs:243`), and the full-coverage consumer
  never overwrites it, so `synthesize_departure` (`stats.rs:17`) reads
  every late train as `delay_minutes = 0`. The windowed design must take
  delay from TRUST's `timetable_variation` field, which `trust_schema::schema::Movement`
  does not deserialize today.
- **Explicit cancellations of trains that have not moved yet are dropped.**
  `correlate::apply_cancellation` (`correlate.rs:115`) only resolves a
  `train_id` that a *matched movement* already resolved, and only flips
  `derived` entries that already exist. A train cancelled at origin never
  moved, so its 0002 is ignored (it then reads "cancelled" only through
  the no-event rule).
- **Activations received before the rail day starts are lost.** TRUST
  activates a train ~60 min before its origin departure (measured below),
  so trains departing 01:00Z–03:00Z are activated before the 01:00Z
  rollover. 267 activations for service date 09-27 arrived before 01:00Z
  (231 in the 23:00Z hour). The rollover (`main.rs:190-213`) replaces
  `DayState` and wipes them, and the startup replay starts at the day
  start, so the day's first trains can never be matched.
- Observation, not investigated further: the production consumer (image
  `sha256:cb4fc6b1…`, started 03:40Z, pre-dates `c3d5a0fc`) had matched no
  movement at all by 04:25Z although 59 population trains of 09-27 had
  reported movements. `events_matched_total` has no series at all.

### 3.2 What a line's population is

`schedules_touching` puts every non-cancelled schedule that calls at *or
passes* any TIPLOC of the line into its population. Per-line day
population on 09-26 (243 lines), under candidate relevance filters:

| Filter | Line-train entries | Median per line | p10 | p90 | Conwy Valley | cross-country |
|---|---|---|---|---|---|---|
| all (today) | 287,852 | 892 | 228 | 2,437 | 198 | 4,898 |
| buses/ships excluded (below) | 270,511 | 858 | 223 | 2,313 | 145 | 4,377 |
| + calls (booked time) at ≥ 1 line station | 233,762 | 804 | 215 | 1,868 | 145 | 4,376 |
| + calls at ≥ 2 distinct line stations | 88,594 | 295 | 52 | 849 | 68 | 1,459 |
| + operator ∈ line `operators` | 116,671 | 352 | 131 | 1,012 | 133 | 231 |
| **+ operator AND ≥ 2 line stations** | **57,371** | **160** | **40** | **538** | **68** | **132** |

Conwy Valley's "all" population is mostly North Wales Coast and even
Birmingham trains that call at Llandudno Junction. Only the last filter
approximates "a train running on this line".

**Buses.** 891 of 19,599 distinct population UIDs (4.5%) never produced
any TRUST message. Checked against the CIF: **882 of them are rail
replacement buses** (Train Status `5`/`B`, category `BR`/`BS`; e.g.
Huddersfield–Brighouse, Seven Sisters–Enfield Town, Guildford–Heathrow bus
station), concentrated in TP (43% of its UIDs that Saturday), XC (33%), GR
(29%) and LO (10%). They were spread evenly across the day. TRUST never
reports buses, so today every one of them is a "cancellation". With
buses and ships excluded (3,176 UIDs, 17,341 line-population entries),
only **9 of 18,717** relevant UIDs (0.05%) were silent all day.

### 3.3 Volume: trains due per line per window

Relevant trains (operator AND ≥ 2 line stations, buses excluded), counted
by their first booked call on the line, over sliding windows every 15 min:

| Window | Band (London time) | p10 | p25 | p50 | p75 | p90 | windows with ≥ 6 |
|---|---|---|---|---|---|---|---|
| 60 min | 07–10 | 2 | 4 | 10 | 19 | 30 | 69% |
| 60 min | 10–16 | 2 | 4 | 10 | 20 | 31 | 71% |
| 60 min | 16–19 | 2 | 4 | 10 | 20 | 30 | 71% |
| 60 min | 19–23 | 2 | 4 | 8 | 18 | 28 | 66% |
| 60 min | 23–06 | 0 | 0 | 0 | 2 | 8 | 13% |
| 30 min | 10–16 | 1 | 2 | 5 | 10 | 16 | 47% |
| 90 min | 10–16 | 4 | 6 | 15 | 30 | 46 | 85% |

- Per line, the median trains due per 60 min between 07:00 and 19:00
  London: p10 = 2, p25 = 4, p50 = 9–10, p75 = 20, p90 = 30, max = 50.
  171 of 243 lines have a daytime median ≥ 6; 151 have ≥ 8.
- Relevant trains per line per day: p10 = 40, p50 = 160, p90 = 538,
  max = 926; 2 lines have none.
- For comparison, LDBWS deduped distinct services per line-half-hour:
  p50 = 4, p90 = 17.

### 3.4 Outcomes and how they arrive

End-of-day outcome of 52,597 relevant line-trains due 06:00Z–00:30Z
(classification of §4.3, with everything received by 02:03Z):

| Outcome | Share |
|---|---|
| ran, < 5 min late at the line | 90.4% |
| ran, ≥ 5 min late at the line | 7.9% |
| explicitly cancelled (0002, not reinstated), never reached the line | 1.2% |
| activated, never reported on or beyond the line, not cancelled | 0.5% |
| never activated, never moved (presumed cancelled) | 0.04% |

Per distinct UID (18,717): 97.0% activated and moved, 1.4% activated then
cancelled without moving, 0.9% moved then cancelled (partial), 0.12%
activated and then silent, 0.37% moved without a recorded activation, and
0.05% silent.

**Cancellations are overwhelmingly explicit** once buses are excluded:
explicit 0002 for ~455 train ids (270 before any movement, 185 en
route), against 9 wholly silent UIDs and 22 activated-then-silent ones.
Cancellation types for relevant trains in the stream sample: `AT ORIGIN`
75, `EN ROUTE` 76, `OUT OF PLAN` 2, `ON CALL` 3 (`ON CALL` is almost
entirely freight/runs-as-required: 455 of 582 were outside every
population). 0006 (change of origin) appeared 55 times and 0005
(reinstatement) 16 times in the same 10 h.

**Timing:**

- Activation arrives **~60 min before the train's origin departure**:
  p1 = 58.9 min, p50 = 59.7 min, p99 = 60.0 min, max 480 min. Relative to
  the first call on the line: p1 = 59 min, p50 = 60 min, p95 = 158 min,
  p99 = 247 min before due; **never after due**.
- Movement feed lag (`received_at − actual`): p50 = 26 s, p90 = 81 s,
  p99 = 6.4 min; 1.2% over 5 min (the p99.9 of 6 h is the restart
  backlog).
- 0002 arrives relative to the line due time: p25 = 78 min after,
  p50 = 22 min after, p75 = 58 min before; 61% arrive after the due
  time. (A cancelled train is first "activated, not yet seen"; the 0002
  often follows.)

### 3.5 Simulation of a recent window (every 15 min, 06:00Z–00:30Z)

Classifying each due train with **only the messages received by the
evaluation instant**, and comparing with its end-of-day outcome:

| At evaluation (W = 60, grace = 10) | Share | End-of-day truth |
|---|---|---|
| on time | 89.9% | 100% on time |
| late (reported ≥ 5 min on/after the line) | 7.5% | 100% late |
| explicit cancelled | 1.0% | 98.0% cancelled, 1.1% late, 0.7% ran |
| presumed cancelled | 0.04% | 82% never ran, 10% ran, 8% late |
| activated, not yet reported on the line, last known delay < 5 | 1.3% | 34% ran, 31% never reported, 19% late, 16% cancelled |

- **Inferring lateness from silence does not work.** Counting an activated
  train that has not reported on the line by `now` as "late by
  `now − due`" marks 27–32% trains that actually ran on time as late
  (TRUST reporting points are sparse on some lines), and put
  `wmr-stourbridge-town` (Class 139 shuttle) at Minor/Severe Delays in 93%
  of its daytime windows. Treating those trains as *pending* (excluded
  from the denominator unless an upstream report already shows ≥ 5 min)
  still catches 96.6% of the trains that turn out late, inside the window.
- Grace: moving from 5 → 10 → 15 min changes little (late-at-t 7.18% →
  7.52% → 7.68%; presumed stays ~0.04%). A 10 min grace absorbs the p99
  feed lag (6.4 min) plus the one-minute stats cadence.
- Window length: at 30 min, 60% of line-windows fall below 6 trains; at
  60 min, 37%; at 90 min, 23% (all hours, 06:00Z–00:30Z).
- Per-line observability (share of activated, non-cancelled trains that
  ever reported on/beyond the line by end of day): p5 = 0.984, min
  0.971 — no line is unobservable once lateness is not inferred from
  silence.

With the recommended rule (§5) — evaluable n ≥ 6 and ≥ 3 affected trains
for the triggering tier — the recent window classified 92.2% of the
10,641 evaluable line-windows Good, 6.0% Minor Delays, 1.5% Severe Delays,
0.19% Reduced Service, 0.03% Part Suspended. Against the severity each
line was actually showing at that moment (`line_status_history`), it
would have **escalated 4.7% of evaluable line-windows: 252 line-hours on
83 lines** (Minor over Good/rank-2: 334, Severe: 158, Reduced: 6,
Part Suspended: 3). Only rank-4 escalations (Severe / Part Suspended):
161 line-windows (1.5%).

### 3.6 Full coverage vs LDBWS for the same day

Across the 139 lines with ≥ 50 LDBWS-sampled services on 09-26:

- late rate (≥ 5 min): full coverage median 6.2%, LDBWS median 0.7%,
  correlation 0.53. LDBWS dedups a service the first time it is seen on a
  board, usually before departure and before its delay is known, so it
  structurally undercounts lateness.
- cancellation rate: full coverage median 0.3%, LDBWS 0.0%,
  correlation 0.54.
- Where both see a real problem they agree: `thameslink-core` 24.9% vs
  26.5% late, 3.5% vs 2.9% cancelled; `thameslink-cambridge` 29.2% vs
  27.0% late.
- Where they disagree it is late running LDBWS does not see:
  `tfw-north-wales-coast` 21% vs 4%, `northern-blackpool-south` 22% vs 2%,
  `tfw-shrewsbury-chester` 22% vs 2%.

## 4. Decisions: what is counted, and how

### 4.1 Which trains belong to a line (the denominator)

A population UID counts for line *L* on service date *D* only if **all** of:

1. **It is a train.** Its resolved CIF Train Status is not `B`/`5` (bus)
   or `S`/`4` (ship). Measured: this alone removes 882 of the 891 UIDs
   that TRUST never reports (§3.2).
2. **Its operator runs the line**: `operator_atoc ∈ L.operators`. Same
   rule LDBWS's `belongs_to_line` applies (`aggregation.rs:1145`), minus
   the destination/headcode narrowing, which is a board-sampling device.
3. **It serves the line**: it has a booked (arrival or departure) time at
   **≥ 2 distinct stations of L** (CRS resolved through the same
   `stanox_crs` crosswalk `population::build_tiploc_index` uses). A train
   that merely passes, or touches one shared hub, is not running on the
   line.

Measured effect (§3.2): 57,371 line-train entries a day instead of
287,852; Conwy Valley 68 instead of 198.

**Where the facts come from.** The population wire entry
(`schedule_query::LinePopulationEntry`) already carries each calling
point's TIPLOC, booked times and `day_offset`. It does not carry the
operator or the Train Status. Both are added as optional fields
(`#[serde(default, skip_serializing_if = "Option::is_none")]`):

- `operator_atoc: Option<String>`, copied from `ResolvedSchedule.operator_atoc`;
- `train_status: Option<char>` (serialized as a one-character string),
  decoded from the `BS` record byte 29 (0-based; CIF column 30) in
  `schedule_query::parse`, carried through `BasicSchedule` →
  `ResolvedSchedule` → `LinePopulationEntry`.

`api` stores the population as opaque JSON (`SchedulePopulationBody.population`
is a `RawValue`, `crates/api/src/routes/ingest.rs:575`), so it needs no change.
The api's other population reader (`schedule_matching`, pin matching) ignores
unknown fields.

**Version skew.** A population published by an old `schedule-reference`
lacks both fields. The consumer then applies rule 3 only and records
`relevance = 'stops_only'` for that line and date. It also **disables
presumed cancellation** for that line and date, because buses cannot be
told apart (§4.3.4). A new consumer against a new population records
`relevance = 'full'`.

Rejected: filtering buses out in `schedules_touching` at publish time.
That would silently change the api's pin matching too, and it hides the
information instead of recording it.

### 4.2 Due time

**A train is *due* on line L at its first booked call at a station of L**:
the booked departure there, or the booked arrival if it has no departure,
converted from Europe/London local time on `D + day_offset` to UTC with the
DST-aware `chrono_tz` conversion `common::rail_day` already uses. Also
kept: `last_due`, the last booked call on L (same conversion). The
consumer also keeps `origin_dep`, the schedule's first calling point's
departure, per UID.

Why not the alternatives:

- **Origin departure.** 42% of relevant line-trains originate off the
  line. Among them, the first line call is p50 = 34 min and p90 = 68 min
  after origin, and long-distance trains are hours away. Counting at
  origin would put trains in a line's window long before they can affect
  it.
- **Each calling point on the line.** A train calls at a median of 4
  (mean 5.7, p90 12) line stations. Per-call counting multiplies one late
  train into many correlated samples, which makes `min_sample` meaningless.
  It also multiplies memory by ~6.

**Memory.** The consumer parses the population body per line and reduces
each entry on the spot to `{uid, due_min: u32, last_due_min: u32,
relevant: bool}` (minutes since the Unix epoch). It keeps `origin_dep_min`
per UID. The calling points are dropped immediately, as `UidOnly` does
today. The transient peak is one line's body, as today (largest ~21 MB of
text), times the reload concurrency.

Steady state adds about 12 bytes per population entry, so ~7 MB for
today plus tomorrow (576k entries), plus a per-UID TRUST record (§4.3,
~30k trains/day × ≤ 64 B ≈ 2 MB). The 1 Gi limit keeps well over 5×
headroom on the ~120–190 MB working set.

The reduction needs the line's TIPLOC → CRS set when the population is
parsed. The reloader therefore gets the line TIPLOC sets through the same
`ArcSwap` it already reads `line_ids` from. When a stanox/crs reload
changes a line's TIPLOC set, that line's stored `ETag` is dropped, so the
next reload re-derives it instead of accepting a `304`.

### 4.3 Per-train state and outcome classification

#### 4.3.1 State kept per UID for the service date

`TrainDay` is keyed by UID and holds everything TRUST said about any
`train_id` of that UID:

- `activated: bool` (0001 seen)
- `cancel: Option<Canx>`, the latest 0002 not undone by a later 0005:
  `{canx_type, dep_min: Option<u32>}`. `dep_min` is the 0002
  `dep_timestamp`, the planned departure at the location it was cancelled
  from.
- `origin_change_dep_min: Option<u32>`, from 0006 `dep_timestamp`, the
  planned departure at the new origin.
- `last_report: Option<{planned_min, delay}>` from the latest 0003.

Per `(line, UID)`:

- `reached: bool`
- `delay_on_line: i16`

A 0003 **reaches** line L when its location resolves to a TIPLOC of L, or
when its planned time is ≥ the train's `due` on L (the line's own station
is not always a TRUST reporting point).

`delay_on_line` is the maximum reported delay among reports at L's
stations. If there are none, it is the delay of the first report whose
planned time is ≥ `due`.

**Delay** is TRUST's `timetable_variation` (integer minutes) when
`variation_status = "LATE"`, and 0 for `ON TIME` and `EARLY`. `OFF ROUTE`
reports update nothing. `trust_schema::schema::Movement` gains
`timetable_variation: Option<String>` (serde default). This is the fix for
§3.1's "`delayed` is always 0"; it also avoids the timestamp-skew guard in
`common::trust_timestamp` entirely.

Activation, cancellation and reinstatement are attributed through a
`train_id → uid` map filled by 0001. A 0002/0005/0006 whose `train_id` is
unknown, because its activation predates the replay, is kept in a small
pending map keyed by `train_id` and applied when the UID is learnt. When
the day closes, whatever is still pending is counted in
`full_coverage_consumer_unattributed_total{msg_type}`.

The fix for §3.1's lost cancellations and lost pre-day activations is:

- **0002/0005/0006 act on `TrainDay` directly.** They no longer need a
  prior matched movement.
- **Activations and TRUST state are keyed by service date**, not by the
  day being correlated. The service date comes from the Activation's
  `tp_origin_timestamp` (the date of origin departure), or else from the
  last two digits of `train_id` (the day-of-month of origin departure),
  matched against D and D + 1. **Not** from `schedule_start_date`, which
  is the CIF validity start (`trust_schema::schema::Activation`'s own
  doc). A message for date *D + 1* that
  arrives before *D*'s close goes into a pre-built `next` `TrainDay` map,
  which becomes current at rollover instead of being discarded. The
  startup replay starts **6 h before the rail-day start**, and applies
  only 0001/0002/0005/0006 from that lookback segment. Measured: activation
  precedes the line due time by ≤ 247 min at p99 and ≤ 513 min max.

#### 4.3.2 Classification of one due train at evaluation instant `now`

A line-train is **due** when `due ≤ now − grace`. Classify in this order,
using only what has been received:

| # | Condition | Class | Counts in |
|---|---|---|---|
| 1 | `reached`, and a live cancellation or origin change says it stops serving L early (`dep_min < last_due`, or `origin_change_dep_min > due`) | **skipped** (partial on the line) | `total`, `skipped`; delayed too if `delay_on_line ≥ threshold` |
| 2 | `reached` | **late** if `delay_on_line ≥ delay_threshold_minutes` (line's merged `Defaults`, 5), else **on time** | `total`, `delayed` |
| 3 | not reached, live 0002 with `dep_min` unknown or `≤ last_due` | **cancelled (explicit)** | `total`, `cancelled`, `cancelled_explicit` |
| 4 | not reached, `origin_change_dep_min > last_due` | **cancelled (explicit)**: the train no longer runs on L | as 3 |
| 5 | not reached, not activated, no report, and presumed cancellation allowed (§4.3.4) | **cancelled (presumed)** | `total`, `cancelled`, `cancelled_presumed` |
| 6 | not reached, `last_report.delay ≥ threshold` | **late** (already late upstream) | `total`, `delayed` |
| 7 | anything else | **pending**: activated or reporting upstream, not yet seen on L | `pending` only, **not** `total` |

Notes:

- A 0002 with `dep_min > last_due` (cancelled *after* the line) is not a
  cancellation for L. The train stays pending until it reports, and is
  then classified by rule 2.
- `ON CALL` 0002s are applied like the rest. Measured: 3 in 10 h for
  relevant trains, and the Train Status/operator filter keeps
  runs-as-required freight out of the population anyway.
- **Reinstatement (0005)** clears `cancel`. Rules 1, 3 and 4 then no
  longer apply, and the train falls through to rules 2, 6 or 7. A
  movement after a cancellation is already honoured by rule 2's
  precedence.
- **Change of origin (0006).** A train that now starts after the whole
  line is cancelled for L (rule 4). One that now starts part-way along L
  is skipped once it reports (rule 1). Before it reports, rule 7 applies.
  0006 bodies carry `dep_timestamp`, so `trust_schema::schema::ChangeOfOrigin`
  gains `dep_timestamp`/`loc_stanox` (serde default), and `Cancellation`
  gains `dep_timestamp`/`loc_stanox`.
- **No lateness from silence.** Rule 7 is the §3.5 finding. Inferring
  "late by `now − due`" from missing reports turns 27–32% of on-time
  trains late, and put a whole shuttle line at Minor/Severe Delays. A
  future refinement can use the 0003 `next_report_run_time`/`next_report_stanox`
  fields to know when a report is overdue; it is out of scope here.
- `avg_delay_minutes` is the mean of `delay_on_line` (or the rule 6
  upstream delay) over non-cancelled trains in `total`, as in
  `common::compute_sample_stats`.

#### 4.3.3 Partial days

`DayState` gains `observed_from: DateTime<Utc>`: the earliest instant
from which this process holds *every* movement-events entry. It is
`rail_day_start − 6 h` after a complete replay (or the day start for a
rollover-entered day with no lookback gap). It is the first replayed
entry's time when the day start was trimmed, and the process start under
Kafka.

- A line-train with `origin_dep < observed_from + 60 min` may have been
  activated before the process could see it. So rule 5 is replaced by
  rule 7 for it (never presumed). A train whose cancellation might
  predate `observed_from` keeps whatever it has: rules 3 and 4 still work
  from any 0002 that is seen.
- For any train with `due < observed_from`, rule 2's "reached" may have
  been missed. Such trains are excluded from `total` and counted in
  `unobserved`.
- A window whose `[window_start, window_end]` is entirely after
  `observed_from + 60 min` is **complete**. Otherwise it is `partial =
  true`, and a partial recent window never influences severity. The
  day-to-date and closed-day rows carry `partial` as today.
- `partial_lines` (population missing at start) makes every window of
  that line `partial` for the day, as today.
- A stream gap detected by `check_gap` (`main.rs:235`) sets
  `observed_from = now` for the rest of the day. Everything before is
  suspect.

#### 4.3.4 When "presumed cancelled" is allowed

All of the following must hold, per line and window:

1. the population for (L, D) carried `train_status` (`relevance = 'full'`);
2. `origin_dep ≥ observed_from + 60 min` (the activation would have been
   seen);
3. **feed health**: the newest movement-events entry this process has
   consumed is ≤ 5 min older than `now` (wall clock vs stream id
   milliseconds), and at least `activations_min` (default 20) 0001s were
   consumed in the last 60 min. Overnight the national activation rate is
   still > 36/h (§3.1: 36–47/h at 00:00–03:00Z).

If 3 fails, the whole write is marked `feed_stale = true`. No window
from it may influence severity, and presumed cancellation is off.
Without this gate a TRUST or relay outage would read as every due train
presumed cancelled.

Feed-health numbers, from the stream sample: national 0001s per hour were
36, 47, 42 and 101 at 00Z, 01Z, 02Z and 03Z; all messages were 445/h at
the quietest hour (02Z) and 30–50k/h in the evening. So `activations_min
= 20` per 60 min and a 5 min staleness limit do not trip on a normal
night.

**Received time.** 0002/0005/0006 `dep_timestamp` values carry the same
local-as-UTC skew as movement timestamps. They must be parsed with
`common::trust_timestamp::parse_trust_epoch_millis` against a
`received_at`:

- live consumption: `Utc::now()` at dispatch (feed lag p50 26 s);
- startup replay: the stream entry id's milliseconds (`RangePage.entries`
  already carries ids).

`DayState::dispatch_payload` gains a `received_at` parameter.

## 5. Windows and parameters

The consumer computes three views on every stats write (60 s cadence,
`STATS_WRITE_INTERVAL_SECS`). All three are over line-trains whose `due`
falls in the range shown.

| View | `due` range | Written | Influences severity |
|---|---|---|---|
| `recent` | `(now − grace − W, now − grace]` | every write | yes, when enabled (§6) |
| `day_to_date` | `[rail_day_start(D), now − grace]` | every write | never |
| closed day | the whole rail day | once, at close (existing `full_coverage_line_stats`) | never |

(Since "Decisions (2026-10-02)" item 3, each range covers D + 1's
trains due in it as well, i.e. those due before `rail_day_start(D + 1)`.)

Parameters, with their defaults as consumer CLI/env options, and why:

| Parameter | Default | Justification (§3) |
|---|---|---|
| `FULL_COVERAGE_RECENT_WINDOW_MINUTES` (W) | **60** | Median line has 9–10 trains due per daytime hour; at 60 min, 69–71% of daytime line-windows reach 6 trains vs 47% at 30 min. 90 min only buys 85% at the price of reacting ~30 min later and keeping a cleared problem visible for 1.5 h. |
| `FULL_COVERAGE_GRACE_MINUTES` | **10** | Covers the p99 feed lag (6.4 min) plus the 60 s write cadence. Activation is ≥ 59 min before due, so presumed cancellation is decided ≥ 69 min after the activation was due. 5 → 15 min moves late-at-evaluation only 7.2% → 7.7%. |
| minimum evaluable sample (`total`) | **6** | Below 6, one train is ≥ 17% of the rate, and the 25% tiers trip on 2 trains. Measured: with 6, 37% of all line-windows (06:00Z–00:30Z) and ~30% of daytime ones are `below_threshold`. That is honest for 2-trains-an-hour branches (p10 line), which LDBWS already covers. |
| minimum affected trains for a tier | **3** | Cuts Minor-Delays windows from 10.0% to 6.0% of evaluable windows at n ≥ 6 (§3.5, no-silence rule), removing "2 of 6 late" flicker, while leaving Severe (1.5%) untouched. |
| delay threshold | line's merged `delay_threshold_minutes` (5) | Same as LDBWS, so the two sources share one meaning of "delayed". |
| `activations_min` / staleness | 20 per 60 min / 5 min | §4.3.4 |

Minimum sample and minimum affected are aggregator settings (§6), not
consumer settings. The consumer writes raw counts, and the aggregator
decides. `min_sample_size` in `Defaults` (3) is the LDBWS knob and is not
reused: full coverage gets its own `full_coverage_min_sample_size` (6)
and `full_coverage_min_affected` (3) in `Defaults`. Both are overridable
per line through `severity_overrides`, like every other key in
`thresholds_for`.

## 6. Severity mapping

A pure function in `common` (so the aggregator and the comparison tool
share it):

```rust
pub fn classify_full_coverage_window(
    w: &FullCoverageWindowStats,   // recent window counts (§7)
    t: &Defaults,                  // thresholds_for(defaults, &line.severity_overrides)
) -> WindowVerdict                 // Ineligible(reason) | Good | Escalate { severity, reason }
```

1. **Ineligible** (never escalates, reported with its reason) when
   `feed_stale`, `partial`, the row is older than 3 × the write interval
   (`computed_at < now − 3 min`), or `total < full_coverage_min_sample_size`.
2. Rates over `total` (evaluable trains, pending excluded):
   - `cancel = cancelled / total`, with `cancelled = cancelled_explicit +
     cancelled_presumed`;
   - `late = delayed / total`;
   - `skip = skipped / total`.
3. Tiers, in this order (thresholds are the existing `Defaults`, so
   LDBWS and full coverage mean the same thing by each severity):
   - `cancel ≥ part_suspended_pct (0.60)` and `cancelled ≥ min_affected`
     → **Part Suspended**
   - `late ≥ severe_delays_pct (0.50)` and `delayed ≥ min_affected` →
     **Severe Delays**
   - `skip ≥ severe_delays_skip_pct (0.50)` and `skipped ≥ min_affected`
     → **Severe Delays**
   - `cancel ≥ reduced_service_pct (0.25)` and `cancelled ≥ min_affected`
     → **Reduced Service**
   - `late ≥ minor_delays_pct (0.25)` and `delayed ≥ min_affected` →
     **Minor Delays**
   - `skip ≥ minor_delays_skip_pct (0.25)` and `skipped ≥ min_affected`
     → **Minor Delays**
   - else **Good**
4. **Presumed cancellations never decide a tier on their own.** If a
   cancellation tier is met only because of `cancelled_presumed` (it
   would not be met with `cancelled_explicit` alone), the verdict drops to
   the next tier that is met without them. Measured: presumed
   cancellations are 0.04% of evaluations and changed no escalation on
   09-26 (§3.5, "explicit-only" row), so this costs nothing today and
   protects against a feed gap the health gate misses.

This is `classify()`'s logic (`aggregation.rs:1185`) with count gates and
one ordering change. Severe Delays is checked before Reduced Service
because both rank 4 vs 3 under `severity_rank` (`crates/common/src/lib.rs:121`).
`classify()` would label "30% cancelled and 60% late" Reduced Service
(rank 3), which undersells it. The reason text uses "trains due in the
last hour", e.g. "5 of 12 trains due in the last hour were cancelled",
not "sampled services".

**Combination (escalate-only).** The window verdict is merged in
`merge_full_coverage` after Layer 1 (incidents) and Layer 2 (LDBWS
samples) have run, for each status of an enabled line:

- `Escalate{severity}` with `severity_rank(severity) > severity_rank(status.severity)`:
  - if the status is LDBWS-inferred (no incident), replace severity and
    reason, and set `data_quality = TrustInferred`. This is the existing
    branch of `merge_full_coverage_stats` (`aggregation.rs:1387`), and it
    stays escalate-only.
  - otherwise (incident-derived), raise the severity and append
    `(train-running data shows: <reason>)`, as `escalate_from_coverage_stats`
    does.
- equal or lower rank, `Good`, or `Ineligible`: nothing changes. In
  particular, full coverage never *lowers* an LDBWS or incident severity,
  and a Good window never clears one. LDBWS and full coverage are
  independent escalators, so the higher of the two wins.
- `status.full_coverage_stats` is set to the recent window's counts
  (mapped into `SampleStats`: `total`, `delayed`, `cancelled`, `skipped`,
  `avg_delay_minutes`), and `full_coverage_availability` becomes
  `Available(..)` when eligible, `Pending` when ineligible. This happens
  **only when the aggregator mode is `enforce`** (§8). In `off`/`shadow`
  nothing on `LineStatus` changes, because the frontend already prefers
  `fullCoverageStats` over `sampleStats` (`frontend/app/lines/AllLinesTable.tsx:253`).
- **Phase gate.** `FULL_COVERAGE_WINDOW_MIN_ESCALATION_RANK`: only verdicts
  whose `severity_rank` is ≥ this value escalate. The default is 4, so
  initially only Severe Delays and Part Suspended escalate (measured:
  1.5% of evaluable line-windows vs 4.7% with Minor/Reduced included,
  §3.5). Lower it to 3 once the shadow comparison supports it; see open
  question 1.
- The skip tiers were not measured on 09-26: the analysis did not have
  0002 locations for the whole day. The shadow comparison must report
  them before `enforce`.

The legacy whole-day read (`load_full_coverage_line_stats` requiring
`available` for UTC today) is **removed from the severity path in
`enforce` mode**. It never matched (§1), and the closed-day row is audit
only. In `off`/`shadow` it stays exactly as today, a no-op in practice, so
`off` is byte-for-byte today's behaviour.

The coverage rollups fed from `LineStatus.full_coverage_stats`
(`record_daily_coverage_stats` / `record_half_hourly_coverage_stats`,
`crates/aggregator/src/main.rs:378-400`) re-add the same snapshot every
cycle (see `full_coverage_comparison.rs`'s module doc). With a 60-min
window in that field they would be meaningless, so in `enforce` mode
they are skipped. `full_coverage_line_window_stats` (§7) is the history.
The two tables are left in place (no drop in this change).

## 7. Data model and API

### 7.1 Storage: one bucketed history table for the windows

Decision: **one table, keyed `(line_id, window_kind, bucket_start)`**, where
`bucket_start` is `computed_at` truncated to 15 minutes. Each 60 s write
upserts the current bucket's row, so the latest write in a bucket wins.
The newest bucket per line is the live value, and the older buckets are
a 15-minute history.

This rejects the two alternatives in the brief:

- **Latest-only `(line_id, window_kind)`** has no history, so shadow mode
  could not be evaluated after the fact. That is the exact auditability
  failure the 2026-09-27 review found in the old line-keyed table.
- **Keyed by `computed_at`** would be 243 lines × 2 kinds × 1,440 rows a
  day (700k/day) for no analytic gain over 15-min buckets.

Size: 243 × 2 × 96 = 46.7k rows a day; **retention 14 days** (653k rows,
a few tens of MB), pruned by the aggregator like the other stats tables
(`FULL_COVERAGE_WINDOW_STATS_RETENTION_DAYS`, default 14). 14 days covers
the 7-day shadow evaluation (§8) with a week to spare. The closed-day
rows keep their 90-day retention.

Migration `crates/api/migrations/20260927120000_full_coverage_line_window_stats.sql`
(transactional; a new table, so its index is built in the same
transaction on an empty table, which `migration_index_locking` does not
flag):

```sql
SET LOCAL lock_timeout = '5s';

CREATE TABLE IF NOT EXISTS full_coverage_line_window_stats (
    line_id            TEXT             NOT NULL,
    window_kind        TEXT             NOT NULL CHECK (window_kind IN ('recent', 'day_to_date')),
    bucket_start       TIMESTAMPTZ      NOT NULL,
    service_date       DATE             NOT NULL,
    window_start       TIMESTAMPTZ      NOT NULL, -- due-time range covered
    window_end         TIMESTAMPTZ      NOT NULL,
    computed_at        TIMESTAMPTZ      NOT NULL,
    total              INT              NOT NULL, -- evaluable: excludes pending/unobserved
    on_time            INT              NOT NULL,
    delayed            INT              NOT NULL,
    cancelled_explicit INT              NOT NULL,
    cancelled_presumed INT              NOT NULL,
    skipped            INT              NOT NULL,
    pending            INT              NOT NULL,
    unobserved         INT              NOT NULL,
    avg_delay_minutes  DOUBLE PRECISION NOT NULL,
    relevance          TEXT             NOT NULL CHECK (relevance IN ('full', 'stops_only')),
    presumed_enabled   BOOLEAN          NOT NULL,
    partial            BOOLEAN          NOT NULL,
    feed_stale         BOOLEAN          NOT NULL,
    stats_version      SMALLINT         NOT NULL,
    updated_at         TIMESTAMPTZ      NOT NULL DEFAULT now(),
    PRIMARY KEY (line_id, window_kind, bucket_start)
);

CREATE INDEX IF NOT EXISTS full_coverage_line_window_stats_bucket
    ON full_coverage_line_window_stats (bucket_start);
```

If the implementer finds that `migration_index_locking` does flag the
index, move it to `20260927120050_…` as a `-- no-transaction`
`CREATE INDEX CONCURRENTLY` file (one statement).

### 7.2 Closed-day row: the audit record

`full_coverage_line_stats` stays keyed `(line_id, service_date)` with its
90-day retention. Under `stats_version = 2`, the open day's row carries
the day-to-date counts (`pending` availability, as today), and the row
written at close carries the final counts over all trains of the day.
`available` still requires closed ∧ ¬partial.

It gains the breakdown columns. Migration
`20260927120100_full_coverage_line_stats_breakdown.sql` (transactional;
constant defaults, so no table rewrite):

```sql
SET LOCAL lock_timeout = '5s';

ALTER TABLE full_coverage_line_stats
    ADD COLUMN IF NOT EXISTS cancelled_explicit INT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cancelled_presumed INT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS pending            INT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS unobserved         INT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS stats_version      SMALLINT NOT NULL DEFAULT 1;
```

`stats_version = 1` marks every existing row as the legacy
whole-population method (unseen = cancelled), so the comparison tool never
compares v1 and v2 rows as like with like.

### 7.3 Wire types (`crates/common/src/lib.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FullCoverageWindowKind { Recent, DayToDate }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FullCoverageWindowCounts {
    pub total: u32, pub on_time: u32, pub delayed: u32,
    pub cancelled_explicit: u32, pub cancelled_presumed: u32,
    pub skipped: u32, pub pending: u32, pub unobserved: u32,
    pub avg_delay_minutes: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCoverageWindowStatsRow {
    pub line_id: String,
    pub window_kind: FullCoverageWindowKind,
    pub service_date: chrono::NaiveDate,
    pub window_start: chrono::DateTime<chrono::Utc>,
    pub window_end: chrono::DateTime<chrono::Utc>,
    pub computed_at: chrono::DateTime<chrono::Utc>,
    pub counts: FullCoverageWindowCounts,
    pub relevance: String,          // "full" | "stops_only"
    pub presumed_enabled: bool,
    pub partial: bool,
    pub feed_stale: bool,
    pub stats_version: u16,         // 2
}
```

`bucket_start` is computed by the api from `computed_at`, never trusted
from the wire.

`FullCoverageLineStatsRow` (`lib.rs:1531`) gains
`#[serde(default)] breakdown: Option<FullCoverageWindowCounts>` and
`#[serde(default)] stats_version: Option<u16>`. The api writes
`breakdown`'s extra counts into the new columns when present, and
defaults otherwise (`stats_version` 1). `SampleStats` stays as the
`stats` payload (`cancelled = explicit + presumed`) so an old api keeps
working.

### 7.4 Endpoint

`POST /private/full-coverage-window-stats` (body `Vec<FullCoverageWindowStatsRow>`,
one batch per write: 243 lines × 2 kinds) and `GET` for last-fetched,
both under the existing `internal_oauth_group_full_coverage` group like
`/full-coverage-stats` (`crates/api/src/routes/ingest.rs:94`). Upsert:
`ON CONFLICT (line_id, window_kind, bucket_start) DO UPDATE` when
`EXCLUDED.computed_at >= existing.computed_at`, so a replayed or late POST
cannot move a bucket backwards.

**Version skew**, deploy order api → consumer → aggregator:

| Pairing | Effect |
|---|---|
| new consumer, old api | window POST gets 404: logged at warn at most once per 10 min, `full_coverage_consumer_errors_total{operation="post_window_stats"}`; closed-day POST still works (the old api ignores the new optional fields) |
| old consumer, new api | no window rows; the aggregator sees no fresh row and the verdict is `Ineligible(stale)`, so no effect |
| new aggregator, old api or schema | the window query fails, is logged, and fails open to an empty map (the same posture as `load_full_coverage_line_stats`, `main.rs:275`) |
| new consumer, old `schedule-reference` population | `relevance = 'stops_only'`, presumed disabled (§4.1) |

### 7.5 How the aggregator reads it

`queries::load_full_coverage_windows(pool, now)` runs:

```sql
SELECT DISTINCT ON (line_id, window_kind) *
  FROM full_coverage_line_window_stats
 WHERE bucket_start >= $now - interval '30 minutes'
 ORDER BY line_id, window_kind, bucket_start DESC
```

It returns `HashMap<line_id, LineWindows { recent, day_to_date }>`, with
per-row skipping like `full_coverage_stats_from_row`
(`crates/aggregator/src/queries.rs:284`). The staleness check against
`computed_at` happens in `classify_full_coverage_window` (§6), so a
consumer that stopped writing ages out within 3 minutes.

### 7.6 What users see in the first version

**Nothing new**, until `enforce`. In `enforce`, an escalated line shows the
escalated severity and the reason text (§6), and `fullCoverageStats`
carries the recent window's counts. The frontend already renders
`fullCoverageStats` in preference to `sampleStats`. The day-to-date view
is stored and available to the comparison tool, but not exposed on the
public API in this change (see open question 3).

## 8. Rollout

### 8.1 Flags (all OFF by default)

| Setting | Where | Default | Effect |
|---|---|---|---|
| `FULL_COVERAGE_WINDOWED_STATS` (`fullCoverageConsumer.windowedStats.enabled`) | consumer | `false` | `false`: today's `build_line_row` output, no window POSTs. `true`: v2 closed-day/day-to-date rows and window POSTs. |
| `FULL_COVERAGE_RECENT_WINDOW_MINUTES` / `FULL_COVERAGE_GRACE_MINUTES` | consumer | 60 / 10 | §5 |
| `FULL_COVERAGE_ACTIVATIONS_MIN` / `FULL_COVERAGE_FEED_STALE_SECS` | consumer | 20 / 300 | §4.3.4 |
| `FULL_COVERAGE_WINDOW_MODE` (`aggregator.fullCoverageWindow.mode`) | aggregator | `off` | `off`: today's code path exactly. `shadow`: read windows, compute verdicts for every line, emit metrics and logs, change nothing. `enforce`: §6 merge for enabled lines. |
| `FULL_COVERAGE_WINDOW_ENFORCE_LINES` | aggregator | `*` | Comma list or `*`; intersected with `full_coverage_enabled` ∨ `FULL_COVERAGE_ENABLED_DEFAULT` |
| `FULL_COVERAGE_WINDOW_MIN_ESCALATION_RANK` | aggregator | 4 | §6 phase gate |
| `FULL_COVERAGE_WINDOW_STATS_RETENTION_DAYS` | aggregator | 14 | prune; runs in every mode |
| `full_coverage_min_sample_size` / `full_coverage_min_affected` | `Defaults` / `severity_overrides` | 6 / 3 | §5 |

**Production already has `FULL_COVERAGE_ENABLED_DEFAULT=true` on the
aggregator** (`charts/distant-signal/values.yaml:1062`, flipped
2026-09-12). The per-line gate therefore does not protect anything:
every line is "enabled". That is why the window mode is a separate
switch, and why `enforce` also takes a line allowlist for the pilot.

Metrics:

- consumer: `full_coverage_consumer_window_rows_posted_total`,
  `…_window_feed_stale` (gauge), `…_presumed_cancelled_total`,
  `…_unattributed_total{msg_type}`, `…_pending_trains` (gauge);
- aggregator: `aggregator_full_coverage_window_verdicts_total{verdict}`,
  `aggregator_full_coverage_window_escalations_total{severity, mode}`
  (`mode = shadow` counts would-escalate).

All are initialised to 0 at startup (as `init_metrics` does), so an absent
series means "never scraped".

### 8.2 Stages

0. **Ship with everything off.** Migrations run; nothing changes.
1. **Consumer on** (`FULL_COVERAGE_WINDOWED_STATS=true`), after
   `schedule-reference` has republished populations with
   `operator_atoc`/`train_status`. Check the working set (expect < 650
   MiB, limit 1 Gi; this said < 250 MB before "Decisions (2026-10-02)"
   item 2), the startup replay time with the 6 h lookback
   (expect ~5 min), and that window rows arrive for ~243 lines every
   minute.
2. **Aggregator shadow** (`FULL_COVERAGE_WINDOW_MODE=shadow`) for **≥ 7
   consecutive days including a weekend**. Run the comparison report
   daily.
3. **Enforce for a pilot set** of 5 lines of different volumes that
   meet §8.4 individually (see open question 2 on Conwy Valley), with
   `MIN_ESCALATION_RANK=4`, for 7 days.
4. **Enforce everywhere** (`ENFORCE_LINES=*`), rank 4.
5. **Lower to rank 3** (Minor Delays / Reduced Service) only after
   another 7 days of shadow-quality evidence for those tiers (open
   question 1).

Rollback at any stage: `FULL_COVERAGE_WINDOW_MODE=off` (aggregator
restart). The next cycle rewrites every line from Layers 1–2 alone.

### 8.3 Comparison report: `compare_full_coverage --windows`

Extend `crates/api/src/bin/compare_full_coverage.rs` and
`crates/api/src/data/full_coverage_comparison.rs`. The existing daily mode
is unchanged. The new `--windows` mode takes `--line-id <id>|--all-lines`
and `--from/--to`, reads read-only, and produces:

1. **Health**: per line, the share of 15-min buckets present, and
   ineligible windows by reason (`below_threshold`, `partial`,
   `feed_stale`, `stale_row`).
2. **Would-escalate log**: for every `recent` bucket,
   `classify_full_coverage_window` (the same `common` function the
   aggregator uses) against the line's actual severity at `computed_at`
   from `line_status_history` (the latest row ≤ that instant). Lists every
   escalation (line, time, from, to, reason text, counts) and totals by
   severity and hour of day. Flags flapping (≥ 3 escalate/clear
   transitions within 2 h on a line) and lines escalated in > 20% of their
   daytime windows.
3. **Against LDBWS**: each `recent` bucket paired with the
   `line_status_half_hourly_stats` rows (LDBWS deduped) covering the same
   due range. Reports late and cancel rates side by side, their
   correlation, and a severity confusion matrix (LDBWS classified with
   the same thresholds when its half-hour `total ≥ 6`).
4. **Against the closed day**: for each line-day, the v2 closed-day row
   vs the last `day_to_date` bucket before close (totals within 2%, as
   the last hour's trains resolve). Also the closed-day rates vs
   `line_status_daily_stats` (LDBWS), reusing `classify_cancellation`
   (`full_coverage_comparison.rs:228`). v1 rows are shown separately,
   never mixed.
5. `--csv <dir>` writes items 2 and 3 as CSV for manual review.

### 8.4 Criteria to leave shadow (stage 2 → 3, and 3 → 4)

Over the 7-day window, from the report:

1. **Completeness.** Window rows are present for ≥ 95% of 15-min buckets
   per line. `feed_stale` appears only during a verified feed or relay
   outage. `partial` buckets are < 2% outside restarts.
2. **Classification precision**, from the report's end-of-day re-check
   of each evaluation (same method as §3.5): explicit-cancelled ≥ 95%
   (measured 98.0%), late ≥ 99% (measured 100%). Presumed cancellations
   ≤ 0.2% of evaluations (measured 0.04%).
3. **Volume.** Would-escalate at the enabled rank on ≤ 3% of evaluable
   line-windows (measured 1.5% for rank 4). No line is escalated in
   > 20% of its daytime windows unless LDBWS or an incident shows ≥
   Minor Delays for at least half of that time (catches systematically
   biased lines).
4. **Corroboration.** Of the 20 largest full-coverage-only escalations
   (no incident, LDBWS Good), ≥ 16 are corroborated on manual review
   (Realtime Trains, NRE incident text, or LDBWS within ± 1 h).
5. **Agreement with LDBWS where both see trouble.** When LDBWS shows ≥
   Minor on a half-hour with ≥ 6 services, the eligible recent window
   shows ≥ Minor at least 60% of the time.
6. **Resources.** Consumer working set p99 ≤ ~650 MiB (was 400 MiB; see
   "Decisions (2026-10-02)" item 2), no OOM kills,
   startup replay ≤ 10 min.

## 9. Implementation tasks, file-level changes and tests

Line numbers are at `c3d5a0fc`. Migrations use only
`20260927120000`–`20260927129999`. Each task ends green on the brief's
verification list (fmt, clippy `-D warnings`, `cargo +1.88.0 check`,
workspace tests, DB-gated tests of touched crates on a fresh DB, and
`migration_index_locking`/`migration_checksums`). No new dependencies:
`chrono-tz` is already reachable through `common`.

### Task 1: schedule facts on the population wire (`schedule-query`, `schedule-reference`)

- `crates/schedule-query/src/records.rs:131` `BasicSchedule`: add
  `#[serde(default)] pub train_status: Option<char>`.
- `crates/schedule-query/src/parse.rs:323` `parse_basic_schedule`: decode
  byte 29 (0-based; CIF col 30, Train Status). A blank byte gives `None`.
  Update the byte-layout doc at `records.rs:100-128`.
- `crates/schedule-query/src/resolve.rs:25,129` `ResolvedSchedule`: carry
  `train_status` from the winning schedule.
- `crates/schedule-query/src/records.rs:398,403` `LinePopulationEntry`:
  add `operator_atoc: Option<String>` and `train_status: Option<char>`,
  both `#[serde(default, skip_serializing_if = "Option::is_none")]`. Fill
  them in `From<ResolvedSchedule>`.
- `crates/schedule-reference/src/main.rs:1191`: no logic change. It
  already publishes `LinePopulationEntry`s. Check that the population
  `ETag` changes once, since the body changes.
- Tests:
  - `parse_basic_schedule` decodes `P` from the real `BSNC005732605172612060000001 PXX1S003101…`
    line quoted in `records.rs`, and `5` from a bus line such as
    `BSN…  5BR…`;
  - `LinePopulationEntry` round-trips with and without the new fields,
    and an old body without them still deserializes;
  - `crates/schedule-query/tests/real_cif_fixtures.rs` keeps passing.

### Task 2: TRUST fields (`trust-schema`)

- `crates/trust-schema/src/schema.rs:103` `Movement`: add
  `timetable_variation: Option<String>`. `:122` `Cancellation`: add
  `dep_timestamp`, `loc_stanox`. `:132` `ChangeOfOrigin`: add
  `dep_timestamp`, `loc_stanox`. `:163` `Reinstatement`: add
  `dep_timestamp`. All `Option<String>`, serde default. Remove
  `#[allow(dead_code)]` from `canx_type`.
- Add `pub fn movement_delay_minutes(m: &Movement) -> Option<i32>`:
  `LATE` → `timetable_variation` parsed; `ON TIME`/`EARLY` → 0;
  otherwise `None`.
- Do **not** change `journey::apply_movement`'s behaviour for
  trust-consumer.
- Tests:
  - parse the real bodies in §3.4 (keys listed there);
  - `LATE` + `"12"` → 12; `OFF ROUTE` → `None`;
  - a body missing the new fields still parses.

### Task 3: population reduction with due times (consumer)

- `crates/full-coverage-consumer/src/population.rs:19` `UidOnly`: replace
  it with a streaming-reducible `PopulationEntryLite { uid, calling_points:
  Vec<CallingPointLite{tiploc, booked_arrival, booked_departure,
  day_offset}>, operator_atoc, train_status }`, deserialized per line.
  Add `fn reduce(entry, line_crs_by_tiploc, line_operators, date) ->
  Option<LineTrain>`. It returns `LineTrain { due_min, last_due_min,
  relevant }`; `relevant` follows §4.1; times are Europe/London →
  UTC minutes.
- `Population` (`population.rs:25`): per `(line, date)` store
  `Arc<LinePop>`. `LinePop` holds:
  - `uids: HashSet<String>`, for movement matching, unchanged semantics;
  - `due: Vec<(u32 due_min, u32 last_due_min, u32 uid_ix)>`, relevant
    only, sorted by `due_min`;
  - `relevance: Relevance {Full, StopsOnly}`.

  Also per date: `origin_dep_min: HashMap<String, u32>`. Keep `contains`
  (`:221`) and `uids_for` (`:241`) working.
- `crates/full-coverage-consumer/src/population_reload.rs:67,111`: pass
  the line's TIPLOC → CRS set and operators, through a new shared
  `ArcSwap<LineGeometry>` built in `main::reload_stanox_crs`
  (`main.rs:442`). Drop the stored `ETag` of a line whose geometry
  changed.
- Tests:
  - the due time is the first line call (arrival when there is no
    departure);
  - BST and GMT conversion, and `day_offset` = 1 for an after-midnight
    call;
  - bus `train_status = '5'` is excluded;
  - operator mismatch is excluded;
  - one line station → not relevant;
  - a missing `operator_atoc`/`train_status` gives `StopsOnly`;
  - a geometry change forces a refetch (wiremock, extending
    `reload_cycle_revalidates_with_etags_and_keeps_the_snapshot_on_304`).
  - Memory regression: a unit test builds a 3,357-entry, 22-call line body
    and asserts `LinePop` holds no calling points (size bound, not
    allocator).

### Task 4: per-train TRUST state and rollover (consumer)

- New `crates/full-coverage-consumer/src/trains.rs`:
  - `TrainDay` and `LineProgress` (§4.3.1);
  - `apply_{activation,movement,cancellation,reinstatement,change_of_origin}`,
    using `common::trust_timestamp::parse_trust_epoch_millis(dep_timestamp,
    received_at)` for the `dep_timestamp` fields;
  - the `train_id` → uid pending map;
  - the service-date routing (`tp_origin_timestamp`, then the `train_id`
    day digits) to the `current` or `next` map.
- `crates/full-coverage-consumer/src/day.rs:44` `DayState`: add
  `trains: TrainState` (current + next), `observed_from`,
  `last_event_at`, and `activation_times: VecDeque<DateTime>` (60-min
  ring for the health gate). `dispatch_payload` (`:79`) and
  `dispatch_message` (`:111`) take `received_at`, and route
  0002/0005/0006 to `trains` (today 0006/0005 are ignored at `:111-122`).
  The existing `correlation`/`stations` behaviour is unchanged (station
  samples are out of scope).
- `crates/full-coverage-consumer/src/main.rs:190-213` rollover:
  `DayState::roll(next)` keeps `trains.next` as the new `current`.
  `:262` live dispatch passes `Utc::now()`.
- `crates/full-coverage-consumer/src/replay.rs:116,205`: start the replay
  at `rail_day_start − 6 h`. Entries before `rail_day_start` apply
  0001/0002/0005/0006 only. Set `observed_from` (§4.3.3). The existing
  trimmed/partial logic keys on the *lookback* start for `observed_from`
  and on the day start for `partial_reason`, as today.
- `main.rs:235-249`: when a gap is detected, set `observed_from = now`.
- Tests:
  - `trains.rs`:
    - a 0002 before any movement cancels;
    - a 0005 after a 0002 clears it;
    - an EN ROUTE 0002 with `dep_min` after `last_due` is not a
      cancellation for the line;
    - 0006 past the line cancels, and 0006 mid-line gives skipped once
      reached;
    - a 0002 for an unknown `train_id` is applied when its 0001 arrives;
    - `LATE` delay is taken from `timetable_variation`.
  - `main.rs`: an activation for D + 1 received at 00:30Z survives the
    01:00Z rollover (a regression test for §3.1's lost activations).
  - `replay.rs`: the lookback segment applies 0001 but not 0003;
    `observed_from` in the trimmed and Kafka cases.

### Task 5: window computation and posting (consumer)

- `crates/full-coverage-consumer/src/stats.rs`: add
  `classify_line_train(...) -> TrainClass` (§4.3.2 table) and
  `build_window(line, kind, range, now, day, population, params) ->
  FullCoverageWindowStatsRow`. Keep `build_line_row` (`:51`) for the flag
  off. Add `build_line_row_v2` for the day-to-date/closed-day row
  (`stats_version = 2`, `breakdown`).
- `crates/full-coverage-consumer/src/config.rs`: the §8.1 consumer flags.
- `crates/full-coverage-consumer/src/main.rs:551` `write_stats`: when
  enabled, build both windows for every shadow line and POST them
  (`queries.rs`: new `post_full_coverage_window_stats`). Use v2 rows for
  `full_coverage_line_stats`. Add metrics and `init_metrics` entries
  (`:295`).
- `charts/distant-signal/values.yaml:1603` (`fullCoverageConsumer`) and
  `templates/full-coverage-consumer-deployment.yaml`: `windowedStats.enabled:
  false`, window/grace/health values, and
  `FULL_COVERAGE_WINDOW_STATS_URL`.
- Tests, table-driven over the §4.3.2 rules, with fixed `now`:
  - pending is excluded from `total`;
  - presumed is only allowed when `relevance = Full`,
    `origin_dep ≥ observed_from + 60`, and the feed is healthy;
  - `feed_stale` is set when `last_event_at` is 6 min old, or when
    activations in the last 60 min are < 20;
  - a window starting before `observed_from + 60 min` is `partial`;
  - the day-to-date range starts at `rail_day_start`;
  - the closed-day v2 row at close equals day-to-date over the whole day;
  - flag off → byte-identical legacy rows (a golden test against today's
    `build_line_row` output).

### Task 6: storage and endpoint (`api`, `common`)

- `crates/common/src/lib.rs:1531`: add the §7.3 types and extend
  `FullCoverageLineStatsRow`. Add `full_coverage_min_sample_size` (6) and
  `full_coverage_min_affected` (3) to `Defaults` (`:1746`) and
  `thresholds_for` (`:1792`).
- Add `classify_full_coverage_window` (§6) in a new
  `crates/common/src/full_coverage_window.rs`.
- Migrations `20260927120000_full_coverage_line_window_stats.sql` and
  `20260927120100_full_coverage_line_stats_breakdown.sql` (§7.1, §7.2).
- `crates/api/src/routes/ingest.rs:94`: the `/full-coverage-window-stats`
  route (same OAuth group). `crates/api/src/data/queries.rs:3576`: the v2
  columns in `upsert_full_coverage_line_stats`. New
  `upsert_full_coverage_window_stats` (bucket computed server-side,
  monotonic `computed_at` guard) and
  `last_full_coverage_window_stats_fetch`.
- Tests:
  - `common`: `classify_full_coverage_window` truth table, covering:
    - each tier at its threshold and at the `min_affected` edge;
    - presumed-only cancellation falls to the next tier;
    - Severe checked before Reduced;
    - Ineligible for each reason;
    - line `severity_overrides` for the two new keys.
  - `api` DB-gated:
    - upsert and bucket;
    - an older `computed_at` does not overwrite;
    - v1 POST (no breakdown) still accepted, `stats_version = 1`;
    - a route auth test like the existing `/full-coverage-stats` ones.

### Task 7: aggregator read, shadow and enforce

- `crates/aggregator/src/config.rs`: `full_coverage_window_mode`
  (`off|shadow|enforce`, default `off`),
  `full_coverage_window_enforce_lines` (`*`),
  `full_coverage_window_min_escalation_rank` (4) and
  `full_coverage_window_stats_retention_days` (14).
- `crates/aggregator/src/queries.rs`: `load_full_coverage_windows` (§7.5)
  and `prune_full_coverage_window_stats` (next to `:1272`).
- `crates/aggregator/src/aggregation.rs:1439`: new
  `merge_full_coverage_windows(reports, lines, windows, defaults, mode,
  allowlist, min_rank, enabled_default)` implementing §6. It reuses
  `merge_full_coverage_stats`'s escalate-only shape (`:1387`) with the
  new reason text. `merge_full_coverage` stays for `off`/`shadow`.
- `crates/aggregator/src/main.rs:274-287`: dispatch on mode. `:378-400`:
  skip the coverage rollups in `enforce`. `:620`: prune the window table.
  Add metrics.
- `charts/distant-signal/values.yaml` (aggregator block near `:1062`)
  and `templates/aggregator-deployment.yaml:116`: the new env, default
  `off`.
- Tests:
  - `off` is identical to today (the existing `merge_full_coverage`
    tests untouched);
  - `shadow` changes no `LineStatus` field but counts would-escalate;
  - `enforce`:
    - escalates only on a strictly higher rank;
    - never demotes an incident or LDBWS status;
    - LDBWS-inferred → `TrustInferred`;
    - an incident status gets the annotation;
    - a line outside the allowlist is untouched;
    - rank-3 verdicts are ignored at `min_rank = 4`;
    - a stale, partial or `feed_stale` row → `Pending`, no escalation;
    - coverage rollups are skipped.
  - DB-gated: `load_full_coverage_windows` picks the newest bucket and
    ignores rows older than 30 min; prune.

### Task 8: comparison report

- `crates/api/src/data/full_coverage_comparison.rs`: the window-mode
  functions (§8.3 items 1–4), each a pure function over loaded rows plus a
  loader.
- `crates/api/src/bin/compare_full_coverage.rs`: `--windows`,
  `--all-lines`, `--csv`.
- Tests: pure-function tests for would-escalate against a synthetic
  `line_status_history` timeline, flapping detection, the LDBWS
  half-hour pairing, and the v1/v2 separation.

### Task 9: docs

- `docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`
  and the consumer runbook: a "superseded by" note on Decisions 2d/2e.
- `lines/tfw-conwy-valley.toml` pilot comment: point at this design.
- `crates/full-coverage-consumer/src/main.rs` module doc: windows.

**Order:** 1 and 2 (independent) → 3 → 4 → 5; 6 can run in parallel with
3–5; then 7, then 8. Deploy order: `schedule-reference` + api → consumer
→ aggregator.

## 10. Open questions (product decisions)

1. **Should full coverage raise lines to Minor Delays / Reduced Service
   (rank 3), or only to Severe Delays / Part Suspended (rank 4)?**
   Measured on a Saturday: rank-4-only escalates 1.5% of evaluable
   line-windows; including rank 3 escalates 4.7% (252 line-hours on 83
   lines). Much of that is real late running that LDBWS structurally
   misses (§3.6), but it is a visible increase in yellow on the map.
   *Recommendation:* ship rank-4-only (`MIN_ESCALATION_RANK=4`). Lower to
   3 after a week of shadow data meets §8.4 for those tiers.
2. **Pilot line(s).** `tfw-conwy-valley` has 68 relevant trains a day
   and is almost always below the 6-train minimum in a 60-min window, so
   it would never escalate and proves nothing. *Recommendation:* pilot on
   five lines with daytime medians of 6, 10, 20, 30 and 45 trains/hour,
   chosen from the shadow report as ones with clean §8.4 results. Keep
   Conwy Valley's flag as is.
3. **Show the day-to-date numbers to users?** It is computed and stored
   either way. *Recommendation:* not in this change. After `enforce`,
   add a one-line "Today so far: N trains due, X% on time, Y cancelled"
   to the line detail page, from the latest `day_to_date` bucket, as a
   separate small change.
4. **Is "≥ 5 minutes late at the first station on the line" the right
   meaning of "delayed"?** It matches LDBWS and `Defaults`, but industry
   punctuality measures use lateness at destination. *Recommendation:*
   keep 5 min at the line. It is the question a user of *that line*
   cares about, and per-line `severity_overrides.delay_threshold_minutes`
   already exists for exceptions.
5. **Minimum sample of 6 trains per hour.** This excludes ~30% of
   daytime line-windows (the quietest branch lines) from full-coverage
   severity entirely; LDBWS and incidents still cover them.
   *Recommendation:* accept it. Lowering to 4 gains ~15 percentage points
   of coverage, but two late trains then read as "Minor Delays".

## Implementation notes (2026-09-27)

What the implementation did differently from sections 4-9, and why:

- **Where the code lives.** To keep the change local for later merges:
  window classification and counting are in a new
  `crates/full-coverage-consumer/src/windows.rs` (not `stats.rs`, whose
  legacy `build_line_row` is untouched); the aggregator's read, verdicts,
  merge and prune are in a new `crates/aggregator/src/full_coverage_window.rs`
  (not `queries.rs`/`aggregation.rs`); the api's storage is
  `crates/api/src/data/full_coverage_window.rs` and the report
  `crates/api/src/data/full_coverage_window_report.rs`. The common types
  and `classify_full_coverage_window` are in
  `crates/common/src/full_coverage_window.rs`, re-exported from `lib.rs`.
- **A third migration**, `20260927120200_full_coverage_window_verdicts`,
  for decision 1's stored verdicts. Like `20260927120000`, it creates a new
  table and its index in one transaction; `migration_index_locking` does not
  flag an index on a table the same file creates, so neither index needed a
  separate `CONCURRENTLY` file. `20260927120100` only adds columns with
  constant defaults (no rewrite). All three set `lock_timeout = '5s'`.
- **`origin_dep` is kept per line-train** (`LineTrain.origin_dep_min`),
  not in a per-date UID map: the reloader carries a line's population over
  on a `304`, and a per-line value travels with it.
- **Relevance.** A line/date is `Full` when any entry carries
  `train_status` or `operator_atoc`. A line with no `operators` configured
  skips the operator rule. Under `Full`, an entry without an operator is
  not relevant to a line that has operators.
- **Buses and ships are left out of the matching uid set too** (not only
  the windows), which is the legacy-row bus fix. TRUST never reports them,
  so no movement matching changes. Ships (`S`/`4`) are excluded with buses,
  as section 4.1 says.
- **Feed health's `last_event_at`** is the newest consumed Movement's
  (corrected) `actual_timestamp`, not the stream entry id's time: the live
  `MovementFeed` returns payloads without ids, and changing that trait would
  touch every consumer. The replay does use the entry id's time as
  `received_at`.
- **`dep_timestamp` correction.** A 0002/0006 `dep_timestamp` is a planned
  time, often hours ahead of receipt, so
  `common::trust_timestamp::parse_trust_epoch_millis` would reject the
  corrected value as implausible and fall back to the raw (skewed) one. The
  per-train state instead applies the skew the latest Movement showed
  (`corrected - raw` of its `actual_timestamp`).
- **Parked 0002/0005/0006** are only kept for `train_id`s whose
  day-of-month digits are today's or tomorrow's; anything else is not this
  day's train.
- **Window bounds.** `recent` covers due times in
  `(now - grace - W, now - grace]` and stores that exclusive start as
  `window_start`, so `window_end - window_start = W` (the reason text says
  "in the last hour"). The closed-day row covers the whole rail day.
- **The closed-day `on_time`** is not stored in `full_coverage_line_stats`
  (the spec's migration has no such column); read back, it is derived.
- **`enforce` scope.** Only the lines `enforce` may change (enabled and
  allow-listed) leave the legacy whole-day merge and the day/half-hour
  coverage rollups; every other line keeps today's code path even in
  `enforce`. `shadow` evaluates every line with a fresh window, enabled or
  not. A line with no fresh window is `Pending` under `enforce`, counted as
  `missing`, and gets no verdict row.
- **The report** judges each stored bucket at its own `computed_at`, so a
  stored row is never "stale"; a stopped consumer shows as missing buckets
  in the health section.
- **No alert rules were added** (the design asks for none). Proposed, for
  after `shadow` starts: `full_coverage_consumer_window_feed_stale == 1`
  for 15 minutes outside a known TRUST outage;
  `increase(full_coverage_consumer_errors_total{operation="post_window_stats"}[30m]) > 0`
  once the api is deployed; `rate(full_coverage_consumer_window_rows_posted_total[10m]) == 0`
  while the flag is on; and in `enforce`,
  `increase(aggregator_full_coverage_window_verdicts_total{verdict="missing"}[15m])`
  above the line count.
  **Update (integration):** the first three are now in the chart's
  PrometheusRule (`metrics.prometheusRule.fullCoverageWindow`) as
  `DistantSignalFullCoverageWindowFeedStale`,
  `DistantSignalFullCoverageWindowPostErrors` and
  `DistantSignalFullCoverageWindowStatsStalled`, rendered only when
  `fullCoverageConsumer.windowedStats.enabled` is true. The `enforce`
  missing-verdicts alert is not added yet.

