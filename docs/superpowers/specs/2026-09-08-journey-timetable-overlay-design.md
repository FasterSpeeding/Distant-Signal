# Design: Timetable-First Journey Detail (scheduled stop list + per-stop live overlay)

**Status: design proposal, approved for implementation by the requesting
session (no separate human sign-off step in this pipeline).** Revises
Decision 3 of
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md` (the
doc `components/TrainJourney.tsx` currently cites as its own design basis)
and supersedes that doc's "No journey timeline" scoping call in the same
Decision 3 and its own Explicitly-out-of-scope section.

## Goal

Product request, verbatim intent: the train detail page
(`components/TrainJourney.tsx`, rendered by `/train/[uid]/[date]` and
`/train/by-id/[trackingId]`) currently makes **live tracking state** the
primary content — a `resolutionStatus`-keyed switch that shows nothing
structured until `resolutionStatus === 'resolved'`, and even then shows
only a single denormalized "current position" summary (last reported
location, one delay-minutes figure, next calling point), never the train's
full route. Restructure this so the **scheduled timetable — every calling
point, in order, with its scheduled time — is the primary, always-shown
structure**, with live data (actual times, delay, current status) overlaid
**per stop** once available, so a user can see exactly how the delay
built up along the route, not just one end-to-end number.

## Required reading consumed in full before this document was written

`frontend/components/TrainJourney.tsx`; `frontend/lib/types.ts` (all of
it); `crates/api/src/data/trains.rs` (whole file, including `db_tests`);
`crates/api/src/data/train_tracking.rs` (`TrackedTrainState`,
`TRACKED_TRAIN_STATE_SELECT`, `upsert_train_movement`,
`create_subscription_for_train`, and the `post_track_by_uid_backfills_*`
test in `crates/api/src/routes/train.rs`); `crates/api/src/data/schedule_matching.rs`
(whole file); `crates/api/src/data/eta_blend.rs`; `crates/api/src/data/queries.rs`
(`crs_for_tiploc`, `list_stanox_crs_for_crs`, the
`schedule_destination_departures` section); `crates/schedule-query/src/records.rs`
(`DestinationDeparture`, `CallingPoint`, `LinePopulationEntry`);
`crates/trust-consumer/src/process.rs` (event/derived-state construction);
migrations `20260828120000_train_tracking.sql`, `20260906100000_trains.sql`,
`20260906110000_train_movement_trains_id.sql`,
`20260906120000_train_movement_events_nullable_tracked_train_id.sql`,
`20260901150000_stanox_crs.sql`, `20260904090000_schedule_line_population.sql`,
`20260907130000_schedule_destination_departures.sql`,
`20260908120000_schedule_destination_departures_calling_point_search.sql`;
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md` (whole
doc, this is the design being revised); `docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`
(sibling data-model area, precedent for tone/structure); `frontend/app/train/[uid]/[date]/page.tsx`;
`frontend/app/train/by-id/[trackingId]/page.tsx`; `frontend/components/EtaBadge.tsx`;
`frontend/lib/dateFormat.ts`.

## 0. Settling the data-model question — which sources actually back "the scheduled timetable"

The brief named four candidate sources and asked for citations on which were
used and which were rejected. Reading the real code (not the brief's
secondhand summary of it) gives a definitive answer.

### 0.1 `trains.calling_points` (JSONB) — used, as the PRIMARY source, when present

Written by `schedule_matching::attempt_schedule_match` →
`trains::find_or_create_train_with_schedule_match` (`schedule_matching.rs:120-145`,
`trains.rs:84-119`), as a `Vec<ScheduleCallingPointDto>` (`tiploc`, `kind`,
`booked_arrival`, `booked_departure`, half-minute flags), already
camelCase-shaped at write time. This is the richest available source:
**every** calling point (including the terminus itself), **both** arrival
and departure booked times, and an explicit `kind` (Origin/Intermediate/
Terminate).

**Confirmed NOT reliably populated for the NR-primary "Track this train by
UID" flow**, per the brief's own suspicion — but the actual gate is narrower
than "never populated for off-catalogue trains." Three real paths populate
it:

1. The legacy pin-based flow (`POST /Train/track` → `attempt_schedule_match`),
   gated on the pin's origin CRS being on a **catalogued line**
   (`crs_to_line_ids`, built from `lines/*.toml`) with published
   `schedule_line_population` for a candidate line. Off-catalogue origin →
   never populated this way.
2. The periodic sweep (`run_schedule_match_sweep`), same gate, retried for
   still-`pending` rows.
3. **`post_track_by_uid`'s backlog-enrichment path**
   (`crates/api/src/routes/train.rs`, proven by
   `post_track_by_uid_backfills_schedule_and_movement_data_from_the_backlog`):
   when `trust_event_backlog` already holds an Activation + located origin
   DEPARTURE for the UID being tracked, the backfill reaches into the SAME
   `attempt_schedule_match`-shaped lookup keyed off the replayed departure's
   own `(CRS, planned time)`, and populates `calling_points` even though the
   subscription itself never went through the pin-based flow.

So `trains.calling_points` ends up populated whenever the train was ever
resolved through a catalogued line (via either of the first two paths) OR
had recent backlog history reaching back to its own origin departure at
track-time. It is **not** populated for: a bare NR-primary track of a train
whose origin isn't on any catalogued line, tracked before any backlog
history existed for it, and never independently schedule-matched. This is
common enough (any train tracked shortly after departure, on a line this
app doesn't catalogue) that a second source is required — confirming the
brief's core premise.

### 0.2 `schedule_destination_departures` — used, as the FALLBACK source

Confirmed via `crates/schedule-query/src/resolve.rs`'s
`departures_by_destination_crs` and the migration's own header ("ONE ROW PER
DEPARTURE, not one row per destination bucket"): for one
`(train_uid, service_date)`, every row shares the same `destination_crs`
(the schedule's true terminus, computed once via `.last()`) and
`true_origin_crs` (the schedule's true origin, computed once via `.first()`,
added by the sibling calling-point-search migration this session is
concurrent with), with `origin_crs`/`scheduled` varying per row — one row
per **departure-bearing** calling point. Querying
`WHERE train_uid = $1 AND service_date = $2 ORDER BY scheduled` therefore
reconstructs the ordered stop list directly, **for every CIF schedule
published that day, independent of line-catalogue membership** — exactly
the decoupling the brief asked for.

Two real gaps, both accepted rather than worked around:

- **Departure time only, never arrival**, for every row (`resolve.rs`'s
  loop keys on `cp.booked_departure`, `continue`s past anything with none).
  Acceptable for a schedule-first structural display; the live overlay
  (movement events) fills in a real arrival once one is reported.
- **The terminus itself has no row** — a `Terminate` calling point has no
  `booked_departure`, so `resolve.rs`'s loop drops it before a row is ever
  built (confirmed by
  `2026-09-08-calling-point-train-search-design.md` §9's own identical
  finding for the search feature). The fallback stop list is therefore
  built from the query's rows (origin + intermediates, chronological) with
  one **synthetic final stop appended** using the row-constant
  `destination_crs` column and no scheduled time — the first real movement
  event reported for that CRS (see §0.4) supplies its scheduled time
  retroactively, from that event's own `planned_timestamp`.

### 0.3 `schedule_line_population` — investigated and REJECTED as a third, direct source

This table (`(line_id, service_date) → JSONB Vec<LinePopulationEntry>`) does
carry the same rich per-UID `CallingPoint` shape `trains.calling_points`
does (arrival, departure, kind). But `crates/api` never deserializes it —
its own migration comment states it is relayed "opaque JSONB... `api` never
deserializes it, only stores/relays it" to `full-coverage-consumer`. Using
it here would mean: (a) writing new deserialization code `api` doesn't have
today, and (b) **first resolving which `line_id` a bare UID belongs to** —
which needs the exact same catalogued-line candidate lookup
`attempt_schedule_match` already performs. It supplies no capability
`trains.calling_points` doesn't already provide (it IS the raw input
`schedule_matching` already turns into `trains.calling_points`), and adds
a second parallel path to reach data already materialized once success
happens. Rejected: no incremental benefit, real added complexity.

### 0.4 `train_movement_events` — used, as the LIVE OVERLAY source

Confirmed via `crates/api/migrations/20260828120000_train_tracking.sql:92-115`
and `train_tracking::upsert_train_movement` (`train_tracking.rs:600-655`):
one row is appended per confirmed TRUST movement message, carrying
`loc_crs` (best-effort STANOX→CRS, `NULL` if untranslatable),
`event_type` (`ARRIVAL`/`DEPARTURE`/`PASS`), `planned_timestamp`,
`actual_timestamp`, `variation_status`. **No existing query reads this
table for a list** — grepped every caller in `crates/api/src/`; every
non-write hit is a bare `COUNT(*)`/dedup-key check in a test, or the
already-known single-row current-state upsert. This is genuinely new query
work, not a rewire of something that already exists, matching the brief's
suspicion. It is exactly the per-stop scheduled-vs-actual data the request
asks for: **latest event per `loc_crs`** (by `received_at`, so a later
DEPARTURE supersedes an earlier ARRIVAL at the same location) gives one
overlay record per distinct calling point actually reported so far.

### 0.5 `eta_next`/`eta_source` on `train_current_state` — reused, not duplicated

Per the brief's explicit instruction: `etaNext`/`etaSource` already exist
(`train_current_state.eta_next`/`.eta_source`, blended at read time by
`eta_blend::find_darwin_eta` for the `next_calling_point`/pin destination
only) and are already rendered by `EtaBadge`. **This design does not
invent a second ETA mechanism.** A stop with no actual time yet renders
its scheduled time only — never a fabricated ETA — with the single
existing `EtaBadge` staying exactly where it is today (the current/next
stop's estimate), unchanged. See §5 Decision 4 for why per-stop ETAs are
explicitly out of scope.

### 0.6 The TIPLOC/CRS key mismatch, and how it's closed

`trains.calling_points` is TIPLOC-keyed (CIF's native location key);
`train_movement_events.loc_crs` and `schedule_destination_departures`'s
`origin_crs`/`destination_crs` are CRS-keyed (station codes). These cannot
be joined directly. `crates/api/src/data/queries.rs::crs_for_tiploc`
already exists (`SELECT crs FROM stanox_crs WHERE UPPER(tiploc) = UPPER($1)
LIMIT 1`, used today only to resolve a matched schedule's own terminus CRS)
— reused here, in a new **batched** sibling (`crs_for_tiplocs_batch`, one
`WHERE UPPER(tiploc) = ANY($1)` query for every distinct TIPLOC in a
calling-point list, mirroring the existing single/batch pairing convention
`trains::find_or_create_train`/`find_or_create_trains_batch` already
establishes) so a stop list with N calling points costs one extra query,
not N. `schedule_destination_departures`-derived stops need no
translation — they're already CRS-keyed.

## 1. Decision: when is a stop list available at all?

**Only once `train_uid` is known** — i.e. `resolutionStatus` is
`'schedule_matched'` or `'resolved'`. Reasoning, stated explicitly per the
brief's request:

- `pending`/`unresolved` genuinely carry no `train_uid` (confirmed:
  `TRACKED_TRAIN_STATE_SELECT`'s `LEFT JOIN trains tr ON tr.id = tt.trains_id`
  yields `NULL` for every `tr.*` column while `trains_id` is `NULL`, and
  `trains_id` is only ever set together with a resolution past `pending`).
  There is no reliable way to know *which* CIF schedule (if any)
  corresponds to this specific real-world service without that identity.
  Guessing from origin CRS + scheduled time alone (the pin's own criteria)
  risks showing a **different train's** stops under a confident-looking
  timetable UI — worse than the honest "waiting" message this page already
  shows, and directly against this codebase's repeated "drop/degrade, never
  fabricate" convention (`eta_blend`'s own doc comment, the calling-point
  search design's §9, `resolve.rs`'s destination-drop behavior). **`pending`
  and `unresolved` keep today's bare pin-summary rendering, unchanged.**
- `schedule_matched` **always** has `trains.calling_points` populated (it's
  set in the same write that flips the status —
  `schedule_matching::attempt_schedule_match`'s dual-write). This state
  today renders literally nothing but a "matched, waiting for live
  tracking" sentence; under this design it renders the **full booked
  timetable** immediately. This is the single biggest concrete improvement
  the brief asked for ("feel less broken... by showing the timetable
  regardless") and it required no fallback-source work at all — the data
  was already being written, just never read back for this purpose.
- Every `resolved` sub-state (`awaiting_activation`, `en_route`,
  `cancelled`, `completed`) has a known `train_uid`, so the fallback
  (§0.2) always has a shot at producing a stop list even when
  `trains.calling_points` is `None` (the live-TRUST-only resolution path,
  confirmed by the by-uid backfill test's own "before this fix" comment
  describing exactly this gap). `awaiting_activation` in particular goes
  from "waiting for its first movement report" with zero structure to
  showing the full booked timetable with no overlay yet — a real
  improvement for a state every tracked train passes through.

If **neither** source yields anything (no `trains.calling_points` AND zero
`schedule_destination_departures` rows for that `train_uid`/`service_date`
— a real train that isn't itself a CIF-published schedule that day, e.g. a
genuine ad-hoc special), the response's stop-list field is `null` and the
page falls back to today's current-state-only rendering for that state.
This is an accepted, named gap, not silently papered over.

## 2. New wire shape: `JourneyStop[]`

One computed, merged record per calling point — booked schedule + best
current live data, already resolved to CRS and to real UTC instants
server-side (never a bare `"HH:MM"` local string on the wire for this new
field, closing the exact class of UTC/London skew bug fixed in
`baa4e75 Fix trains-search review findings: UTC/London date-time skew`
by construction, not by frontend care — every scheduled time is converted
via the existing `eta_blend::london_to_utc` at the point it's read, the
same helper the pin-created-time / Darwin-ETA paths already use).

```rust
// crates/api/src/data/journey.rs (new module)

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyStop {
    pub crs: Option<String>,             // None only if a TIPLOC-sourced stop's TIPLOC didn't resolve
    pub name: Option<String>,            // stations.name, best-effort, same LEFT-JOIN-style fallback as everywhere else
    pub tiploc: Option<String>,          // Some for a calling_points-sourced stop, None for a fallback-sourced one
    pub kind: Option<String>,            // "Origin" | "Intermediate" | "Terminate"; None only for a fallback-sourced
                                          // intermediate stop that isn't provably first or last (see §3.2)
    pub scheduled_arrival: Option<DateTime<Utc>>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    pub last_event_type: Option<String>, // which reported event this stop's actual data came from
    pub variation_status: Option<String>,// raw TRUST string, passed through unchanged (no app-side reinterpretation)
    pub delay_minutes: Option<i32>,      // (actual - scheduled) in minutes, whichever pair is available; None until an actual exists
}
```

```ts
// frontend/lib/types.ts addition

export type JourneyStopKind = 'Origin' | 'Intermediate' | 'Terminate';

export interface JourneyStop {
  crs: string | null;
  name: string | null;
  tiploc: string | null;
  kind: JourneyStopKind | null;
  scheduledArrival: string | null;   // RFC3339
  scheduledDeparture: string | null; // RFC3339
  actualArrival: string | null;      // RFC3339
  actualDeparture: string | null;    // RFC3339
  lastEventType: string | null;      // "ARRIVAL" | "DEPARTURE" | "PASS"
  variationStatus: string | null;
  delayMinutes: number | null;
}
```

Added to `TrainJourneyState` (so both `TrackedTrainState` and the
`PublicTrainState`→`TrainJourneyState` adapter in
`app/train/[uid]/[date]/page.tsx` carry it) as:

```ts
journeyStops: JourneyStop[] | null;
```

`scheduleCallingPoints`/`PublicTrainState.callingPoints` (the existing raw,
booked-only, TIPLOC-keyed field) are **kept, unchanged** — grepped every
frontend call site; nothing reads them today (confirmed: the only
non-`lib/types.ts` hits are the `page.tsx` passthrough and two test
fixtures setting them to `null`), so there's no dead-field cleanup forced
by this change, and removing a field nothing else in this plan touches
would be scope creep. `journeyStops` is additive, matching every other
change in this data-model area this session.

## 3. Backend: `crates/api/src/data/journey.rs` (new module)

### 3.1 Entry point

```rust
pub async fn build_journey_stops(
    pool: &PgPool,
    trains_id: i64,
    train_uid: &str,
    service_date: NaiveDate,
    calling_points_json: Option<&serde_json::Value>,
) -> anyhow::Result<Option<Vec<JourneyStop>>>
```

Called from both `routes::train::get_by_tracking_id`/`get_by_uid_and_date`
(via `train_tracking::get_tracked_train_by_id`/`get_tracked_train_by_uid_and_date`)
and `routes::train::get_by_uid_and_date`'s public counterpart
(`trains::get_public_train_state`) — **only** when `train_uid`/`trains_id`
are non-`None` (§1). Both existing read paths already have every input
this needs; no new query is needed to obtain them, only to act on them.

`TrackedTrainState` gains a `trains_id: Option<i64>` field
(`#[serde(skip_serializing)]` — an internal plumbing detail, never on the
wire, mirroring how `id` already means something different on
`PublicTrainState`) so the route handler knows which shared `trains` row's
movement events to read; `TRACKED_TRAIN_STATE_SELECT` gains
`tr.id AS trains_id` to the existing `LEFT JOIN trains tr` (additive column
on an existing join, no behavior change to any existing field).
`PublicTrainState` already carries `trains_id` (public, on-the-wire) — no
struct change needed there.

### 3.2 Building the base stop list

- **`calling_points_json` is `Some`:** deserialize as
  `Vec<RawCallingPoint>` (a new, private, `Deserialize`-only struct in this
  module mirroring `schedule_matching::ScheduleCallingPointDto`'s exact
  camelCase field names — decoupled by design, not importing
  `schedule_matching`'s private type, matching this codebase's existing
  "each layer owns its own wire shape" posture). Batch-resolve every
  distinct TIPLOC via `queries::crs_for_tiplocs_batch`. Compute
  `scheduled_arrival`/`scheduled_departure` via
  `eta_blend::london_to_utc(service_date.and_time(booked_time))` for
  whichever of `booked_arrival`/`booked_departure` is present. `kind` is
  copied straight from the stored `ScheduleCallingPointKind`. Order:
  preserved as stored (schedule order, per `schedule_query::CallingPoint`'s
  own contract).
- **`calling_points_json` is `None`:** call new
  `queries::list_calling_point_departures_for_train(pool, train_uid,
  service_date) -> Vec<CallingPointDepartureRow>`
  (`SELECT origin_crs, scheduled, true_origin_crs, destination_crs FROM
  schedule_destination_departures WHERE train_uid = $1 AND service_date = $2
  ORDER BY scheduled`). Empty result → this whole function returns
  `Ok(None)` (§1's named gap). Otherwise: one stop per row, `crs =
  origin_crs`, `tiploc = None`, `scheduled_departure =
  london_to_utc(service_date.and_time(scheduled))`, `scheduled_arrival =
  None`, `kind = Some("Origin")` for the first row (`origin_crs ==
  true_origin_crs`, both known to be the same value on that row per §0.2),
  `Some("Intermediate")` for every other row from this source. Then, if
  `destination_crs` (constant across every row, from the last row) is
  `Some` and differs from the last emitted stop's `crs`, append one
  synthetic final stop: `crs = destination_crs`, `kind =
  Some("Terminate")`, both scheduled fields `None` (§0.2's accepted gap —
  filled retroactively in §3.3 if a movement event arrives for it).

### 3.3 Overlaying live data

`queries::latest_movement_event_per_location(pool, trains_id) ->
Vec<MovementEventRow>` — new query:

```sql
SELECT DISTINCT ON (UPPER(loc_crs)) loc_crs, event_type, planned_timestamp,
       actual_timestamp, variation_status
FROM train_movement_events
WHERE trains_id = $1 AND loc_crs IS NOT NULL
ORDER BY UPPER(loc_crs), received_at DESC
```

Build a `HashMap<String /* UPPER(crs) */, MovementEventRow>`. For each base
stop with `crs: Some(c)`, look up `c.to_uppercase()`:

- `event_type == "ARRIVAL"`: `actual_arrival = actual_timestamp`;
  `scheduled_arrival = scheduled_arrival.or(planned_timestamp)` (fills the
  Terminate-row gap and any fallback-sourced intermediate stop that had no
  arrival time at all).
- `event_type == "DEPARTURE"`: `actual_departure = actual_timestamp`;
  `scheduled_departure = scheduled_departure.or(planned_timestamp)`.
- `event_type == "PASS"`: both `actual_arrival` and `actual_departure` set
  to `actual_timestamp` (a passing train's arrival and departure are the
  same instant, for display purposes) — with `scheduled_arrival`/
  `scheduled_departure` backfilled the same `.or(planned_timestamp)` way.
- `last_event_type`, `variation_status` copied through unchanged.
- `delay_minutes`: `(actual_timestamp -
  scheduled_departure.or(scheduled_arrival)).num_minutes()` when both
  sides resolve to a real instant, mirroring `trust-consumer::process.rs`'s
  own `(a - p).num_minutes()` delay computation exactly (same sign
  convention: positive = late) — not a new formula.

A movement event whose `loc_crs` matches no stop in the base list (an
unscheduled diversion location, or a STANOX→CRS translation that doesn't
line up with either source's own CRS) is **silently not merged into any
stop** — it remains visible in the existing single-line "Last reported: X"
summary, unaffected. Named as an accepted gap in Open Questions, not solved
here.

### 3.4 Station names

New `queries::station_names_for_crs_batch(pool, crs_codes: &[String]) ->
HashMap<String, String>` — one `SELECT crs, name FROM stations WHERE
UPPER(crs) = ANY($1)` for every distinct stop CRS, mirroring the existing
`LEFT JOIN stations`/`None`-on-no-match convention used everywhere else in
this data model (`pin_origin_name`, etc.) rather than inventing a new
fallback rule.

## 4. Frontend: restructuring `TrainJourney.tsx`

**The core restructuring the brief asks for.** Today, `JourneyDetails`
(the only place any movement data renders) is nested inside the
`resolved`+non-`awaiting_activation` branch only. This design moves the
stop list to render **once, at the top level, whenever
`state.journeyStops` is non-`null`** — before/alongside the existing
per-status messages, not nested inside one branch of them:

```tsx
export function TrainJourney({ state }: { state: TrainJourneyState }) {
  const pinSummary = ...; // unchanged

  return (
    <Stack gap="sm">
      <StatusMessage state={state} pinSummary={pinSummary} />
      {state.journeyStops && <JourneyTimeline stops={state.journeyStops} />}
    </Stack>
  );
}
```

`StatusMessage` is the existing `resolutionStatus`/`status`-keyed switch,
extracted verbatim (same branches, same copy, same cancelled/
may-have-finished alerts) — **minus** the `JourneyDetails` call, which
`JourneyTimeline` now supersedes for any state that has `journeyStops`.
`JourneyDetails`'s old denormalized summary (`lastReportedLocation`,
`delayMinutes`, `nextCallingPoint`, `EtaBadge`) is **kept as a fallback**,
rendered only when `journeyStops` is `null` but movement data still
exists (the "real train exists in `train_current_state` but isn't itself a
CIF schedule that day" gap from §1) — so nothing regresses for that named
edge case.

New `components/JourneyTimeline.tsx`:

- One row per stop, in array order (already schedule-ordered per §3.2).
- Station name (falling back to CRS, falling back to "Unknown location" —
  matching `lib/stationLabel.ts`'s existing fallback ladder), with a small
  `kind` indicator (Origin/Terminate visually distinguished from
  Intermediate — e.g. bold/larger for the two endpoints, matching this
  app's existing "don't invent new visual language, reuse Mantine
  primitives already in use" posture).
- Scheduled time: departure preferred if both known (matches how a real
  station board reads), falling back to arrival-only for the terminus.
  Rendered via `formatTime` (`lib/dateFormat.ts`), the same
  London-pinned, en-GB formatter every other network-time value in this
  app already uses — **not** a raw string, closing the exact bug class
  the June session's UTC/London fix addressed.
- Once an actual time exists for that stop: actual time next to scheduled,
  plus a delta badge reusing the **existing** delay-badge visual
  convention already in `JourneyDetails` (`orange`/`"Xm late"` vs.
  `green`/`"On time"` — extended here to also cover **early**, e.g.
  `teal`/`"Xm early"` for a negative `delayMinutes`, which the current
  single-figure badge never needed to distinguish because a negative
  overall delay is rare but a negative **per-stop** delay — an early
  arrival before an on-time departure — is a completely normal, expected
  case this per-stop view will show constantly).
- A stop with no actual time yet: scheduled time only, no fabricated ETA
  (§0.5) — visually "not yet reached," e.g. dimmed/muted text, distinct
  from a reached stop.
- The existing top-level `EtaBadge` (current ETA for the *next* stop)
  stays exactly where it is today, inside the fallback `JourneyDetails`
  path and also duplicated as a small "next stop" indicator above/below
  the timeline for the `journeyStops` path — **not** attached per-stop
  (§0.5 — this app has exactly one ETA mechanism, already scoped to "the
  next stop," and this design doesn't multiply it across every future
  stop, which `eta_blend`'s matching logic has no way to compute anyway).

### `pending`/`unresolved`: unchanged, deliberately

Per §1's reasoning: `state.journeyStops` is always `null` for these two
states (the backend never attempts §3 for them), so
`{state.journeyStops && <JourneyTimeline .../>}` simply doesn't render —
`StatusMessage`'s existing "Waiting to hear from Network Rail" /
"Couldn't be matched" copy is untouched. This is the honest answer to the
brief's "decide, with reasoning, whether/how much of the timetable can be
shown even before resolution" question: **none**, because there is no safe
way to know which timetable without a `train_uid`, and the improvement
this design delivers for the "feels broken" complaint comes from
`schedule_matched`/`awaiting_activation` (§1) — both of which the current
code also renders as bare "waiting" messages today, and both of which DO
have a `train_uid` and therefore DO get the full timeline under this
design.

## 5. Decisions carried forward, and new ones

1. **Server-side merge, not client-side.** `JourneyStop` is fully resolved
   (CRS, real UTC instants, computed delay) before it reaches the wire —
   matching this codebase's established pattern of shaping response DTOs
   server-side (`ScheduleCallingPointDto`, `calling_point_departure_json`)
   rather than shipping raw joins for the frontend to interpret, and
   avoiding a second implementation of the London/UTC conversion and
   TIPLOC/CRS resolution logic in TypeScript.
2. **One overlay record per location, not per event.** A location visited
   twice in one journey (a real but rare CIF anomaly, flagged as an open
   question by the sibling calling-point-search design too) collapses to
   its single latest-reported event. Not solved here — named, matching
   that sibling design's own precedent for leaving this exact class of
   edge case flagged rather than blocking on it.
3. **`journeyStops: null` is a real, renderable outcome**, not an error —
   the fallback `JourneyDetails` path (§4) exists specifically so a
   resolved-but-off-schedule train still shows *something* rather than a
   blank timeline.
4. **No per-stop ETA.** Only the current, already-existing `etaNext`
   (next-stop-only, Darwin/TRUST-blended) is shown. Computing a
   *predicted* time for every not-yet-reached stop would need real
   forward propagation across the whole remaining route — a genuinely new
   estimation mechanism, not an overlay of data that already exists, and
   explicitly out of scope per the brief's own "reuse if it exists, don't
   duplicate" instruction.
5. **`scheduleCallingPoints`/`callingPoints` (raw, TIPLOC-keyed, booked-only)
   are left in place, unused by any renderer**, exactly as they are today
   — additive-only change, no cleanup forced.

## 6. Testing

Following this repo's established convention for this data-model area:

- `crates/api/src/data/journey.rs`: unit tests for `build_journey_stops`
  with (a) `calling_points_json` present — TIPLOC resolution, kind
  passthrough, arrival+departure both present; (b) `calling_points_json`
  absent, `schedule_destination_departures` rows present — Origin/
  Intermediate/synthetic-Terminate construction, `true_origin_crs`-vs-
  `origin_crs` discrimination (mirroring the sibling search design's own
  "at least one schedule where these differ" fixture requirement); (c)
  neither source has anything — `Ok(None)`; (d) a movement event overlay
  for `ARRIVAL`/`DEPARTURE`/`PASS`, including the delay-minutes sign
  convention and the Terminate-row scheduled-time backfill; (e) an event
  whose `loc_crs` matches no stop — silently dropped, doesn't panic or
  attach to the wrong stop. All DB-backed, `#[ignore]`d per this
  workspace's convention, run via
  `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo
  test -p api -- --ignored --test-threads=1`.
- `queries::crs_for_tiplocs_batch`, `list_calling_point_departures_for_train`,
  `latest_movement_event_per_location`, `station_names_for_crs_batch`: each
  gets its own direct DB-backed test, same convention as every other
  function in `queries.rs`.
- `routes::train`: extend the existing `db_tests` for
  `get_by_tracking_id`/`get_by_uid_and_date` to assert `journeyStops`
  appears (or is `null`) in the right cases, reusing this file's existing
  fixture-seeding helpers.
- `components/JourneyTimeline.tsx`: new colocated `*.test.tsx` — renders
  the right row per stop, the right badge color for late/early/on-time,
  the not-yet-reached muted state, kind indicators.
- `components/TrainJourney.tsx`: existing state-table tests
  (`pending`/`schedule_matched`/`unresolved`/every `resolved` sub-state)
  updated to also assert `journeyStops` renders (or doesn't) per state,
  keeping the existing per-state message assertions intact.
- Full suite before considering this done: `cargo build --workspace`,
  `cargo test --workspace`, the DB-backed `-- --ignored --test-threads=1`
  run above, `npx vitest run`, `npm run build` (frontend).

## 7. Explicitly out of scope

- **Per-stop ETA / forward delay propagation** (§5 Decision 4).
- **A location visited twice in one journey** (§5 Decision 2) — flagged,
  not solved, matching the sibling search design's identical posture on
  its own analogous edge case.
- **The TRUST-resolution "stuck at pending forever" bug.** This design
  makes `pending` feel less broken as a *side effect* by making
  `schedule_matched`/`awaiting_activation` far more useful (§1), but does
  not touch `crates/trust-consumer/` or `resolution_status`-flipping logic
  — a different investigation's scope.
- **The destination-arrival-time search filter on `/trains/search`** —
  entirely different route/files (`crates/api/src/routes/trains.rs`,
  `TrainSearchForm.tsx`), a concurrent, unrelated worktree's work.
- **Metrics/per-station stats.**
- **Removing `scheduleCallingPoints`/`callingPoints`** (§5 Decision 5).
- **A departure-board-style "next departure in N min" live countdown.**
  Not asked for; `formatTime`'s static rendering (refreshed by the
  existing 30s `AutoRefresh`) is unchanged and sufficient, same posture
  every other live figure in this app already takes.

## 8. Open questions / risks

1. **Real-world frequency of the fallback path (§0.2) vs. the primary
   path (§0.1)** is unverifiable by static reading — how often a
   NR-primary track genuinely lacks both a catalogued-line match and
   backlog history at track-time is a production-observation question,
   not a code-reading one. Both paths are implemented and tested either
   way, so this doesn't block shipping, but it's worth instrumenting
   later if there's ever a reason to know which source served a given
   page view.
2. **A movement event with an untranslatable `loc_crs` (`NULL`) is
   dropped from the `DISTINCT ON` query entirely** (the `WHERE loc_crs IS
   NOT NULL` clause) — consistent with how `loc_crs: None` already means
   "can't be shown" everywhere else in this table, but means a stop can
   have real movement history that never reaches the overlay if the
   STANOX→CRS translation happens to fail for that message. Not solvable
   without improving the translation table itself, which is out of this
   design's scope.
3. **Long journeys with many calling points** (a handful of very long
   InterCity/sleeper services can have 20-30+ stops) — no pagination or
   collapsing is designed here; `JourneyTimeline` renders the full list.
   If this turns out to be a real UX problem in practice, a
   collapsed/expandable long-list treatment is a natural frontend-only
   fast-follow, not designed here since nothing in the codebase today
   establishes a precedent for that pattern to follow.
