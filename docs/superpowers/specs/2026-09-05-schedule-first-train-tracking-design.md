# Design: Schedule-First Resolution for Tracked-Train Pins

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass.**

Triggered by a real, confirmed production bug: a user pinned an EUS-origin,
19:15-scheduled train more than an hour after that train's one-and-only
origin-departure TRUST Movement event had already flowed through Redis
Streams and been consumed. `crates/trust-consumer/src/matching.rs`'s
`resolve_origin_departure` only ever gets one shot at a live Movement inside
a ±20-minute window (`MATCH_TOLERANCE`, `matching.rs:23`); once missed, the
row is permanently stuck at `resolution_status = 'pending'`, exactly as
`crates/trust-consumer/src/process.rs:32-55`'s own module doc names as a
known, accepted limitation. The repo owner's own framing, verbatim: *"add a
schedule-based fallback match... shouldn't this be using the schedule by
default for getting the train info with trust events being layed ontop for
the actual tracking info."* This document investigates whether that
schedule-first re-architecture is feasible today, and designs it precisely
where it is.

Required reading consumed in full before this document was written:
`crates/schedule-query/src/{lib,records,parse,resolve}.rs`;
`crates/schedule-reference/src/main.rs`;
`crates/api/migrations/20260904090000_schedule_line_population.sql`,
`20260904110000_schedule_network_departures.sql`,
`20260828120000_train_tracking.sql`;
`crates/api/src/data/{queries,train_tracking}.rs`;
`crates/api/src/routes/departures.rs`;
`crates/trust-consumer/src/{matching,process}.rs`;
`crates/full-coverage-consumer/src/{correlate,population}.rs`;
`crates/common/src/lib.rs` (`TrackedTrainRef`, `TrainMovementEventMessage`);
`frontend/components/TrainJourney.tsx`, `frontend/lib/types.ts`;
`frontend/app/page.tsx`, `frontend/app/track/mine/page.tsx`,
`frontend/app/train/by-id/[trackingId]/page.tsx`.

## Current relevant state, grounded directly in code

**Three CIF-schedule-derived data shapes already exist. None of them was
built for this. Each covers a different, precisely-bounded slice of the
problem:**

1. **`schedule_line_population`** (`crates/api/migrations/20260904090000_schedule_line_population.sql`):
   `PRIMARY KEY (line_id, service_date)`, opaque JSONB —
   `Vec<schedule_query::LinePopulationEntry>` — published by
   `schedule-reference`'s `publish_schedule_line_population`
   (`crates/schedule-reference/src/main.rs:230-263`) for **every catalogued
   line with at least one `tiploc`-bearing station**
   (`lines_to_publish`, `main.rs:354-360` — 109 lines under `lines/*.toml`
   total, confirmed by `ls lines/*.toml | wc -l`; this predicate is **not**
   gated on `LineDefinition.full_coverage_enabled`, so it runs for every
   catalogued line, not just full-coverage ones). Built via
   `schedule_query::schedules_touching(&index, &tiplocs, today)`
   (`resolve.rs:77-96`), which keeps **every** non-cancelled schedule whose
   calling points touch any of that line's own TIPLOCs, **for the whole
   day, with no time-of-day filter**. Each `LinePopulationEntry` carries
   `uid` and the full `calling_points: Vec<CallingPoint>`
   (`records.rs:157-160`), each with `tiploc`, `kind`, `booked_arrival`,
   `booked_departure`, `is_half_minute_{arrival,departure}`
   (`records.rs:138-145`).

2. **`schedule_network_departures`** (`crates/api/migrations/20260904110000_schedule_network_departures.sql`):
   `PRIMARY KEY (crs, service_date)`, opaque JSONB —
   `Vec<schedule_query::ScheduleDeparture>` (`{uid, scheduled, destination_crs}`,
   `records.rs:183-188`, deliberately **no** `calling_points` — see its own
   doc comment, "row-size blowup for a feature this slice doesn't need").
   This is the only one of the three that is genuinely **national and
   CRS-keyed** — built from `departures_by_crs` over the **whole-network**
   `ScheduleIndex`, not filtered to catalogued lines (`main.rs:277-310`).
   But it is capped to `MAX_DEPARTURES_PER_STATION = 10`
   (`main.rs:273-276`) and filtered to `booked_departure >= now`
   **at publish time** (`resolve.rs:112-161`, `departures_by_crs`'s own
   doc). Critically, `poll_once` only recomputes and republishes when a
   *new* CIF delivery is detected (`main.rs:101-107`, the
   `last_processed_delivery` early return) — with `poll_interval_secs`
   defaulting to 1800s but CIF full/update deliveries landing far less
   often than that in practice, this table's "next 10 forward" window is
   effectively **frozen at whatever moment that day's delivery was last
   processed**, not continuously refreshed as the day goes on. For a busy
   terminus (EUS, the exact station in the reported bug), 10 departures
   from the start of the rail day is unlikely to reach as far as 19:15 at
   all.

3. **`schedule_query::ScheduleIndex`** itself (`crates/schedule-query/src/resolve.rs`):
   a pure, in-memory, national, UID-indexed structure —
   `ScheduleIndex::schedule_for_uid(uid, date)` is exactly the
   `train_uid -> booked schedule` bridge `matching.rs`'s own module doc
   names as missing. But per `crates/schedule-query/src/lib.rs:17-38`'s own
   module doc, this crate does **no I/O** and is **"not wired into any
   production data path"** — `schedule-reference` builds one fresh
   `ScheduleIndex` per delivery-processing cycle, uses it to compute both
   products above, and **discards it**. The full national index is never
   persisted anywhere.

**The `full-coverage-consumer` precedent** (`crates/full-coverage-consumer/src/correlate.rs`)
confirms the pattern this document proposes already works in production
shape: `apply_movement` (`correlate.rs:42-93`) resolves a TRUST Movement's
`train_uid` (from a parked Activation, `pending_activations`) and checks it
against `population.uids_for(line_id, service_date)` — schedule data
**first**, TRUST **layered on top** — for every catalogued line whose
TIPLOC index (`population.rs:68-81`, `build_tiploc_index`) the Movement's
location falls under. This is real, tested, and structurally exactly what
the repo owner is asking for. Its scope boundary is the same one that
applies here: it is **per-catalogued-line**, via the same
`schedule_line_population` data (`population.rs:1-3`), not a national
lookup.

**The pin's own key has no line concept.** `tracked_trains`
(`crates/api/migrations/20260828120000_train_tracking.sql:60-64`) stores
`pin_origin_crs`/`pin_scheduled_departure`/`pin_destination_crs` — a bare
CRS and a UTC timestamp, nothing that names a line. Resolving a pin against
`schedule_line_population` therefore requires a new **CRS -> candidate
line_ids** reverse index — the exact thing `population.rs`'s
`build_tiploc_index` already builds, keyed by TIPLOC rather than CRS, from
the static `lines/*.toml` catalogue (not from population data itself). This
is a small, direct generalization, not new architecture: build it the same
way, over **every** catalogued line's `stations`, not just full-coverage
ones.

**`api` owns the CRS->TIPLOC table already**, separately: `stanox_crs`
(`crates/api/src/data/queries.rs:649-724`, `list_stanox_crs`), populated
from the same `schedule-reference` cycle. No `WHERE crs = $1` variant
exists yet (`list_stanox_crs` returns everything), but the table itself has
every field needed (`stanox, crs, tiploc, station_name, source_sequence`).

## Decisions

### 1. Feasibility verdict: schedule-first resolution is real today, but only via `schedule_line_population`, and only for catalogued-line stations

Answering the brief's core question precisely: **schedule-first resolution
of a tracked pin's `train_uid` is feasible today, without any new
persistence, for a pin whose origin CRS sits on at least one of the 109
catalogued `lines/*.toml` lines** (confirmed: EUS is on
`lines/west-coast-main-line.toml:33-34`, so the reported bug's own station
qualifies) — via `schedule_line_population`, cross-referenced through a new
CRS->line_id reverse index (Decision 2).

It is **not** feasible via `schedule_network_departures` for anything but
the first ~10 departures of the rail day from a given station (Decision 1
of the "Current relevant state" section above) — which, for a major
terminus, excludes the overwhelming majority of realistic pin times,
including the exact 19:15 EUS case that motivated this document. This
table was built and shipped for a different job (a live "next few
departures" picker in `TrackTrainForm`,
`docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md`)
and its own doc comments say so explicitly; reusing it here would silently
fail for exactly the cases this fix exists to close, so this design does
**not** build on it.

For a pin whose origin CRS is on **no** catalogued line at all, there is
**no usable schedule data source today** — a real, honest gap. But it is a
*bounded* gap, not an open-ended one: the raw material
(`schedule_query::ScheduleIndex`, built fresh every cycle from the full CIF
`SCHEDULE` extract) already exists in `schedule-reference`'s memory every
cycle; what's missing is only *persisting more of it, uncapped and without
the now-filter*. See Decision 8 (Non-goals) for why closing that gap fully
is scoped out of this document.

### 2. New CRS->line_id reverse index, generalized beyond full-coverage lines

Add a function alongside `population.rs::build_tiploc_index`'s pattern (or,
since the consumer here is `api`, not `full-coverage-consumer`, a new
function in `crates/api`), built from `common::LineDefinition::stations`
over **every** catalogued line with at least one `tiploc`-bearing station
(mirroring `lines_to_publish`, `main.rs:354-360` exactly — not gated on
`full_coverage_enabled`, since `schedule_line_population` is already
published unconditionally for that same set):

```
crs -> Vec<line_id>   (built once at process startup / reference reload,
                        from the static lines/*.toml catalogue -- same
                        "static catalogue, not population data" posture
                        population.rs's own doc comment establishes)
```

A CRS shared by multiple catalogued lines (plausible for a major
interchange) yields multiple candidate `line_id`s; Decision 3 below
resolves the case where more than one produces a hit (in practice they will
agree, since the underlying `schedules_touching` result for the same UID at
the same station is identical regardless of which line's TIPLOC set
happened to include it — see Open Question 3).

### 3. Resolution-flow redesign: schedule match first, TRUST layered on top, without removing the existing TRUST path

**At pin-creation time** (`crates/api/src/data/train_tracking.rs::create_pin`,
or immediately after, inside the same request), attempt a schedule match:

1. Look up `pin_origin_crs`'s candidate `line_id`s via Decision 2's index.
   No candidate -> stop; the row stays exactly as it is today (`pending`,
   `train_uid` `NULL`) and is retried on the next periodic pass (below).
2. For each candidate `line_id`, fetch that line's
   `schedule_line_population` for `service_date`
   (`queries::get_schedule_line_population`, already exists,
   `queries.rs:760-776`) and deserialize its JSONB into
   `Vec<schedule_query::LinePopulationEntry>`. **This is a real, new
   dependency direction**: `api` currently treats this JSONB as opaque and
   says so in its own doc comments (`queries.rs:729-731`, "`api` never
   deserializes it... only `schedule-reference`... and
   `full-coverage-consumer`... need that shape") — this design breaks that
   boundary deliberately, adding `schedule_query` as a dependency of `api`.
   Flagged explicitly as a Decision, not a side effect: see Open Question 1
   for the resulting coupling.
3. For each entry's `calling_points`, find one whose TIPLOC (via
   `schedule_query::normalize_tiploc`) matches one of the pin's
   `pin_origin_crs`'s TIPLOCs (new `stanox_crs` query, `WHERE crs = $1` —
   trivial addition to `queries.rs`'s existing `list_stanox_crs` pattern)
   **and** whose `booked_departure`, converted from CIF's Europe/London
   local `NaiveTime` to a UTC instant on `service_date` (reuse
   `crates/api/src/data/eta_blend.rs`'s existing `london_to_utc`
   DST-aware conversion — do **not** re-derive this, it already handles the
   ambiguous/nonexistent-hour edge cases), falls within the same
   `MATCH_TOLERANCE` (±20 minutes, `matching.rs:23`) already used for live
   TRUST matching. Reuse that constant's *value*, not necessarily its
   crate — `trust-consumer` and `api` are separate crates; either
   duplicate the constant with a comment cross-referencing the other, or
   hoist it into `common` so both read from one definition (recommended,
   small, avoids silent drift).
4. On a match: set `tracked_trains.train_uid` to the matched UID
   immediately. Do **not** set `train_id` (that remains exclusively
   TRUST-sourced — a schedule can name *which* service this is, never *that
   specific day's live TRUST identifier* for it, which only exists once
   TRUST activates it). Move `resolution_status` to a new intermediate
   value (Decision 4). Store enough of the matched entry's
   `calling_points` for the frontend to show a real stopping pattern
   without a second cross-service query at read time (a new nullable
   column, sketched but not fully specified here — this is a design spec,
   not a migration; see Open Question 5).
5. On no match across every candidate line: leave the row untouched.

**Also run this same attempt periodically**, not only at creation, against
every row still lacking a `train_uid` (both `pending` and any pin created
before this shipped). This is what makes the fix retroactive-capable
(Decision 6) and also covers a pin created *before* that service's
`schedule_line_population` cycle had run for the day. The natural home for
this loop is **inside `api` itself**, not `trust-consumer` — `api` already
owns both `tracked_trains` and `schedule_line_population` in the same
Postgres instance; no cross-service HTTP round-trip is needed the way
`trust-consumer`'s existing reference-reload requires. A simple periodic
job (same shape as `schedule-reference`'s own poll loop) re-running
Decision 3's steps 1-5 against every `tracked_trains` row where
`train_uid IS NULL` is sufficient; no new message bus, no new consumer.

**TRUST's own live matching path is untouched, and must stay reachable for
schedule-matched rows.** `resolve_origin_departure` (`matching.rs:34-46`)
still needs to fire for a schedule-matched pin to ever learn its live
`train_id` — this is the one piece that genuinely cannot come from CIF
schedule data (TRUST assigns `train_id` fresh per real-world day, at
Activation time). Concretely: `process.rs::apply_reference_reload`'s match
arm (`process.rs:217-231`) currently only pushes `"pending"`-status rows
into `reference.pending` for live matching. **It must be widened to also
push the new intermediate status** — a schedule-matched row is not yet
"resolved" in the sense that matters to `trust-consumer` (it still has no
`train_id` to short-circuit on), so it must stay eligible for the exact
same CRS+time Movement-claiming heuristic that resolves a plain `pending`
row today. Nothing about `resolve_origin_departure` itself needs to change
— a schedule-matched pin is claimed by a live Movement exactly the way a
`pending` one is; the only difference is it already has a display-ready
`train_uid`/schedule metadata while it waits.

### 4. `resolution_status`: a new intermediate value, not an overload of `pending`

This is a real product decision, thought through rather than defaulted:

- **Not** silently keeping `pending` and just populating `train_uid` on a
  still-`pending` row. `TrainJourney.tsx:54` and every other frontend read
  site (`page.tsx:386`, `track/mine/page.tsx:132`,
  `train/by-id/[trackingId]/page.tsx:71`) treats `resolutionStatus ===
  'resolved'` as the *only* signal that `trainUid` is meaningful and
  displayable — overloading `pending` with a populated `train_uid` would
  either require auditing and changing every one of those four call sites
  to also check `trainUid !== null` (fragile, easy to miss a fifth site
  later) or would leave the schedule-matched information invisible to the
  user despite existing in the database — defeating the point.
- **Not** reusing `resolved` outright either — `resolved` today is a
  promise the frontend already leans on structurally
  (`TrainJourney.tsx:54-58`'s own comment: "`trainUid` is non-null per the
  backend's own resolution invariant... even though the TypeScript type
  can't express that correlation") that a live TRUST binding exists (or at
  minimum was attempted). Silently starting to set `resolved` from a
  schedule match alone would be a quiet redefinition of what that word has
  meant since `docs/superpowers/specs/2026-08-28-train-tracking-design.md`,
  and would make "resolved but zero live movement data ever arrives"
  indistinguishable from "resolved, TRUST simply hasn't posted a Movement
  yet" — a real loss of honest signal for a pin whose train never gets a
  matching live event at all (rarer, but real: a schedule can be matched
  against a service later cancelled by TRUST-side Cancellation before any
  Movement is ever posted).
- **Decision: add `'schedule_matched'`** as a new
  `tracked_trains.resolution_status` CHECK-constraint value, sitting
  between `pending` and `resolved`: "we know which physical service this
  pin refers to and its booked schedule, TRUST has not yet confirmed
  anything live about it." `resolved` keeps meaning exactly what it means
  today (both `train_uid` **and** `train_id` bound, i.e. the same
  two-field invariant `upsert_train_event` already enforces,
  `train_tracking.rs:400-412`) — this design does not touch that
  semantic, only adds an earlier, honest waypoint before it.
- Frontend: add a third explicit branch in `TrainJourney.tsx`, distinct
  from both the `pending` "waiting to hear from Network Rail" copy and the
  `resolved` "Train {uid}" copy — something like "Matched to a scheduled
  service, Train {uid} to {destination}, calling at {stops}... waiting for
  Network Rail's live tracking to begin" — because claiming a train is
  "matched"/"tracked" the same way `resolved` copy currently implies would
  overstate what's actually known (schedule, not live confirmation) and
  risks looking wrong if that specific scheduled service is altered or
  cancelled without TRUST ever posting a Movement for it at all.

### 5. Required companion fix: relax `upsert_train_event`'s two-field guard

This is the single most important technical coupling in this design, and
skipping it would make the schedule-first path *partially* land, then
silently stall in a **new**, more likely way. `upsert_train_event`
(`train_tracking.rs:394-462`) only writes `train_uid`/`train_id`/flips
`resolution_status` to `'resolved'` when the **same incoming event**
carries **both** `resolved_train_uid` **and** `resolved_train_id`
(`train_tracking.rs:400-412`). `process.rs`'s own module doc
(`process.rs:32-55`) already documents this as a known gap for the
*current* system: an Activation not seen in-process before the resolving
Movement means `resolved_train_uid` goes out `None`, and the row never
flips to `resolved` even though tracking keeps working via
`state.resolved`.

Once a schedule match can populate `train_uid` **before** any TRUST message
ever arrives, this gap gets **strictly worse**: the resolving Movement's
`resolved_train_uid` is *still* whatever this process's own
`pending_activations` map happened to capture (`process.rs:494-501`,
unrelated to the schedule match), so it will very often be `None` even
though `tracked_trains.train_uid` is already correctly set from the
schedule. Under the current guard, that means `train_id` never gets
written and `resolution_status` never advances past `schedule_matched` —
even though the live Movement genuinely did arrive and claim the pin, and
`train_current_state` (written unconditionally, `train_tracking.rs:434-458`)
genuinely does carry real, live, correct tracking data the whole time. The
user would see live-accurate location/delay data trapped behind a
`schedule_matched` status that never says "resolved."

**Required change**: split `upsert_train_event`'s guarded `UPDATE` so
`train_id` and `resolution_status = 'resolved'` are written whenever
`resolved_train_id.is_some()` **alone**, independent of whether
`resolved_train_uid` also arrived in that same message — and `train_uid`
is written only when a fresh value is actually supplied (`COALESCE`-style,
preserving whatever `train_uid` the row already carries from a schedule
match, or from an earlier message, rather than overwriting it with
`NULL`). This directly reopens the exact piece of already-reviewed work
`process.rs:51-55` names as deliberately deferred ("relaxing `crates/api`'s
two-field guard... reopens already-reviewed Task 4 work") — this design
cannot ship its stated benefit without also making that change, so it is
listed here as a required companion, not an optional nice-to-have.

### 6. Does this retroactively fix the already-stuck EUS row?

**Only if the fix is deployed and a periodic retry (Decision 3's periodic
pass) runs against still-unresolved rows — a fresh schedule match cannot
happen for a row created before this ships, on its own.** But *unlike*
`schedule_network_departures` (which drops a departure once it's in the
past, per its own `now`-forward filter, `departures_by_crs`,
`resolve.rs:112-161`), **`schedule_line_population` carries no such
filter** — `schedules_touching` (`resolve.rs:77-96`) keeps every
non-cancelled schedule touching the line's TIPLOCs for the whole
`service_date`, past or future. So as long as:

1. This design ships and includes the periodic retry pass (Decision 3), and
2. `west-coast-main-line`'s `schedule_line_population` row for **today**
   (`service_date` = the pin's own date) is still present — i.e., at least
   one `schedule-reference` cycle has successfully processed a delivery
   since that rail day started,

...then the periodic pass **will** find and schedule-match the stuck EUS
row the same as any other still-`pending` row, entirely mechanically, with
no special-cased backfill script. If either condition doesn't hold (the
fix hasn't shipped yet, or that day's population was never published for
some reason), the row needs a one-off manual fix regardless — this design
does not, by itself, retroactively repair a row from a rail day whose
population data no longer exists or predates deployment. Be precise here
rather than over-promising: **this is a real, mechanical retroactive fix
for rows still within the same/current rail day once shipped, not a
guarantee for every already-stuck row that will ever exist by the time
this lands.**

### 7. Backward compatibility

**Not a breaking wire-shape change**, verified against every current
`resolutionStatus` consumer:

- `frontend/lib/types.ts:357`'s `ResolutionStatus` union needs a new
  literal (`'schedule_matched'`) added — additive, not a removal or rename.
- `TrainJourney.tsx:24,39` are an `if`-chain, not an exhaustive `switch` —
  a wire value the current code doesn't recognize would fall through to
  the `resolved` render path at the bottom (since there's no final `else`
  catching an unrecognized value), which happens to render tolerably even
  *without* a frontend change (shows "Matched to train {uid}" +
  "Waiting for its first movement report", since `state.status` would
  still be `null`) — but this is accidental, not a designed fallback, and
  is exactly why Decision 4 calls for an explicit third branch rather than
  leaning on this coincidence.
- `page.tsx:386`, `track/mine/page.tsx:132` gate on
  `resolutionStatus === 'resolved' && trainUid` — a `schedule_matched` row
  evaluates `false` there, identical to today's `pending` behavior; no
  crash, no wrong-looking UI, just under-informative until Decision 4's UI
  work lands alongside it.
- `page.tsx:436`, `track/mine/page.tsx:273` badge on
  `resolutionStatus === 'unresolved' ? red : gray` — a new value renders
  gray, same bucket as `pending` today; harmless.
- Any new field added to `TrackedTrainState`/`TrackedTrainListItem` for
  schedule metadata (calling points, matched line, etc.) is a **new,
  additive, optional** field — non-breaking for existing frontend code by
  construction (existing code simply doesn't read it).

**Ship the frontend and backend changes together anyway** — the additive
guarantee means it *can't* break, but shipping the new status without its
UI branch (Decision 4) would show the accidental, half-right fallback
above to real users for however long that gap lasts.

### 8. Comparison to `full-coverage-consumer`: same pattern, same scope boundary, deliberately not reused wholesale

`correlate.rs::apply_movement` is real proof this "schedule data first,
TRUST layered on top" shape works — but it is a poor fit to reuse
*directly* (as a shared crate/function) for tracked-train pins, for three
concrete reasons, not merely "different enough to redo":

1. It resolves an **already-known** `train_uid` (from a live Activation)
   against a **known** line's population (`population.uids_for(line_id,
   ...)`) — the opposite direction from this design's problem, which is
   "no `train_uid` at all yet, only a CRS+time, resolve `train_uid` from
   schedule data." `schedules_touching`/`resolve_for_date`
   (`schedule_query::resolve`) are the right primitives either way, but
   the calling shape differs enough that force-sharing `correlate.rs`
   itself would mean bending it to a use it wasn't written for.
2. It runs inside a long-lived, whole-national-Kafka-stream consumer
   process with its own in-memory `Population` rebuilt each rail day
   (`population.rs`) — a very different operational shape from `api`'s
   request/response + periodic-job model this design uses (Decision 3).
3. Its scope boundary is identical to this design's own Decision 1
   finding: **per-catalogued-line only.** Nothing about
   `full-coverage-consumer`'s existence changes the fact that a pin on an
   uncatalogued-line station has no schedule data to correlate against
   either way.

## Explicitly out of scope (Non-goals)

- **A full national, uncapped, UID-and-CRS-indexed CIF schedule database.**
  The raw material for this already exists every cycle
  (`schedule_query::ScheduleIndex`, built and discarded by
  `schedule-reference`) and the missing piece is genuinely just
  "persist more of what's already computed, without the cap/now-filter" —
  smaller in kind than "build a new subsystem" — but it is still a
  distinct, separately-scoped, separately-reviewable piece of work (new
  table, new/changed ingest route, a real conversation about row-size and
  refresh cadence for ~2,500 stations' worth of full-day departures instead
  of 10). Not designed here. This is the one honest limitation of this
  document's proposal: it closes the gap for catalogued-line stations
  (which, in practice, covers most of what users are likely to track, since
  the 109 lines were curated around the passenger network this app already
  cares about) but not the whole national network.
- **Loosening `schedule_network_departures`'s cap/now-filter as a
  substitute.** Considered and rejected for this fix specifically (see
  Decision 1) — it would still only produce `{uid, scheduled,
  destination_crs}`, no calling points, and touching it at all risks
  destabilizing the already-shipped whole-network-trip-search feature's own
  row-size assumptions for a use case (pin resolution) it wasn't built for.
  If the "any station at all" gap above is ever pursued, it should be its
  own new persistence, not a repurposing of this one.
- **Direct `Activation.train_uid == tracked_trains.train_uid` matching** as
  a second, more precise binding path (an Activation, once a schedule match
  already knows the target `train_uid`, could in principle claim the pin by
  exact UID equality rather than the fuzzy ±20-minute CRS+time heuristic).
  Genuinely valuable, and enabled by this design's data (`train_uid` known
  before TRUST posts anything), but changes `trust-consumer`'s own matching
  logic in a way this document does not want to bundle into an
  already-multi-part change. Named here as a natural, separate follow-up.
- **Changing `MATCH_TOLERANCE`, `MAX_PIN_AGE`, or any other existing
  constant's value.** This design reuses them as-is (Decision 3).
- **A migration file or any Rust code.** Per this task's brief, this is a
  design spec only.

## Open questions / risks

1. **Biggest risk: `api` gaining a `schedule_query` dependency and
   deserializing what it currently treats as opaque JSONB** (Decision 3,
   step 2) is a real architectural boundary crossing this codebase's own
   comments have twice now stated as deliberate ("`api` never
   deserializes it, only stores/relays it" — `queries.rs:729-731` and the
   migration's own comment). This design argues the boundary crossing is
   justified (the whole point is for `api` to *use* the schedule data, not
   just shuttle it), but it's worth a second, independent look before
   implementation — e.g., whether `schedule_query` should gain a thin,
   `api`-friendly query function (`match_pin(population: &[LinePopulationEntry],
   crs_tiplocs: &[&str], scheduled: DateTime<Utc>) -> Option<&LinePopulationEntry>`)
   so `api` calls one well-tested function rather than hand-rolling the
   TIPLOC/time comparison inline.
2. **Two-field guard relaxation (Decision 5) touches already-reviewed,
   currently-shipped code** (`upsert_train_event`) that other things may
   depend on the current behavior of — needs its own test-suite audit
   before changing, not just a description in this doc.
3. **Multiple candidate lines for one CRS**: if `pin_origin_crs` maps to
   more than one catalogued line's TIPLOC set, do their independent
   `schedule_line_population` entries for the same real-world UID ever
   *disagree* (different STP-overlay resolution, since `schedules_touching`
   resolves per-line but per-UID resolution should be date-deterministic
   and line-independent)? Expected to always agree in practice (STP
   resolution depends only on `(uid, date)`, not on which line's TIPLOC set
   triggered the lookup), but not independently verified against a real
   multi-line-overlap station in this pass.
4. **Ambiguity within the tolerance window**: a busy station could have two
   different UIDs both booked within ±20 minutes of the pin's scheduled
   time (the same ambiguity `resolve_origin_departure`'s own
   "earliest-created pin wins" tie-break exists for, but inverted — here
   it's one pin, multiple candidate schedules). This document does not
   specify a tie-break rule (nearest-time wins is the obvious default, but
   not decided here).
5. **Exact shape of the new nullable schema-metadata storage** (Decision 3,
   step 4) — whether to denormalize a calling-points snapshot onto
   `tracked_trains` at match time (stable, but duplicates
   `schedule_line_population` data and can drift from a later-corrected
   schedule) versus storing only a pointer (`matched_line_id`) and
   re-deriving at read time (always current, but couples every `GET`
   read to a live JOIN + JSONB scan) is a real trade-off not resolved here
   — left for the implementation plan.
6. **A schedule match can be wrong in a way TRUST-only matching cannot**:
   a late-notice STP overlay/cancellation issued *after* `schedule-reference`'s
   last successful cycle for the day would not be reflected in
   `schedule_line_population` until the next cycle runs — meaning a
   `schedule_matched` pin could briefly show a calling pattern for a
   service that's actually been altered. Recommend caveated copy ("as
   scheduled — subject to late alterations") rather than treating matched
   schedule data with the same confidence as a live TRUST report.

## Summary answers to the brief's specific questions

- **Feasible today for any pin?** No. Feasible today, without new
  persistence, only for pins whose origin CRS sits on one of the 109
  catalogued lines, via `schedule_line_population` (Decision 1).
- **Minimum viable new persistence for the general case?** Not proposed
  here as in-scope — flagged as its own, smaller-than-expected-but-still-
  separate follow-up (Non-goals).
- **Resolution-flow redesign?** Schedule match at pin-creation time (+
  periodic retry), populating `train_uid` and schedule metadata
  independent of TRUST; TRUST's existing Activation/Movement path stays
  the only source of `train_id`/live status, unchanged, and must stay
  reachable for schedule-matched rows (Decisions 3, 5).
- **`resolution_status`/UX?** A new `'schedule_matched'` intermediate value
  with its own frontend copy, not an overload of `pending` or `resolved`
  (Decision 4).
- **Does this fix the stuck EUS row?** Retroactively, mechanically, once
  shipped with the periodic retry — but not before then; that row (or any
  row stuck before this ships) needs a one-off manual fix in the meantime
  (Decision 6).
- **Backward compatible?** Yes, additively — verified against every
  current `resolutionStatus` read site (Decision 7).
