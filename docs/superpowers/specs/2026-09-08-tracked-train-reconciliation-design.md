# Design: A Periodic Reconciliation Sweep for Stuck Tracked-Train State

**Status: approved design (confirmed production bug, fix authorized). Spec
stage — no code in this pass.**

## Why this document exists

Two independent, confirmed production bugs leave a tracked train's public
state permanently wrong, both verified against the live database and pod
logs this session, not speculated:

- **A subscription can stay `resolution_status = 'pending'` forever even
  after its train has genuinely been tracked correctly** — real
  `train_movement_events` rows exist under the right `trains_id`, but the
  one subscription's own status column never advanced. The frontend renders
  this as "Waiting to hear from Network Rail," indefinitely, for a train
  that has already run.
- **An NR-primary tracked train (`POST /Train/by-uid/{uid}/{date}/track`)
  can go forever without schedule data** (origin, destination, calling
  points) even though the CIF-published timetable for it has been sitting
  in `schedule_destination_departures` the whole time — nothing ever reads
  that table for this purpose.

Both bugs share the same shape: a one-shot, at-creation-time (or
in-process-only) attempt exists, nothing retries it, and no reconciliation
job exists anywhere in this codebase to catch what the one-shot attempt
missed. This document designs that reconciliation job.

Required reading consumed in full before this document was written:
`crates/api/src/data/{train_tracking,schedule_matching,trains}.rs`;
`crates/api/src/routes/train.rs` (`post_track_by_uid`/`enrich_shared_train`);
`crates/api/src/main.rs` (`schedule_match_sweep_loop`); `crates/api/src/data/config.rs`;
`crates/trust-consumer/src/{process,main}.rs`; `crates/trust-backlog-consumer/src/process.rs`;
`crates/aggregator/src/{main,queries,config}.rs`; every migration touching
`trains`/`train_subscriptions`/`schedule_destination_departures`
(`crates/api/migrations/2026090{6100000_trains,7100000_rename_tracked_trains,7130000_schedule_destination_departures,8120000_schedule_destination_departures_calling_point_search}.sql`);
`docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md`;
`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md` (the
design this fix sits inside — its resolution-status/schedule-matching
model, below, is taken as given, not re-litigated).

## 1. Current state, confirmed directly against code (not the design doc's
   aspirational end-state)

**Correction applied during review of this document**: an earlier pass of
this section claimed `trust-backlog-consumer`'s shared-table write (the
2026-09-06 shared-train-identity design's "primary movement-event writer"
step) "has not shipped." That was checked again against the actual route
handler and found to be wrong. `trust-backlog-consumer`'s only outbound
HTTP call is indeed `post_trust_event_backlog`
(`crates/trust-backlog-consumer/src/{process,queries,main}.rs`), but the
**server-side handler for that one route** (`crates/api/src/routes/ingest.rs:274-301`)
does two writes, not one: `upsert_trust_event_backlog_batch` (the backlog
archive), and then — its own comment reads "Additional, parallel write onto
the shared trains/train_movement_events/train_current_state tables" —
`trust_event_backlog::ingest_shared_movements_batch`
(`crates/api/src/data/trust_event_backlog.rs:130-`), which DOES write
`train_movement_events`/`train_current_state` and DOES call
`trains::mark_trains_resolved_batch` (setting `trains.train_id`/
`resolved_at`), gated on the identical `event.train_uid.is_some()`
condition as the movement-event write, in the same batch. So this path has,
in fact, shipped, and is a second, fully independent way `train_movement_events`
rows and `trains.train_id`/`resolved_at` can come to exist for a `trains_id`
that `trust-consumer` itself has no live-resolved knowledge of at all —
entirely apart from the I6 fan-out mechanism traced below, which remains
accurate and is a second, additional, independent cause of the identical
stuck state. Both are real; neither is required to explain any single
occurrence, and either alone is sufficient justification for Decision 1
below.

`train_subscriptions.resolution_status`'s CHECK constraint (confirmed
against `crates/api/migrations/20260905150000_schedule_matched_resolution.sql`,
never narrowed by a later migration) still permits all four values:
`pending` (default, no identity or schedule known), `schedule_matched`
(identity + booked schedule known via CIF, no live TRUST data yet),
`resolved` (live TRUST data confirmed, via `flip_legacy_resolution` alone),
`unresolved` (a hard failure state, out of scope here — no writer of it was
found in this fix's research).

### 1.1 Stall 1 — `flip_legacy_resolution` is the only writer of `resolution_status = 'resolved'`, and it is live-in-process-only

`crates/api/src/data/train_tracking.rs:700-735`. As of Task 22's cutover it
writes exactly one column:

```sql
UPDATE train_subscriptions SET resolution_status = 'resolved' WHERE id = $1
```

Called only from `upsert_train_event` (`train_tracking.rs:746-786`), only
when the incoming `TrainMovementEventMessage` carries `resolved_train_id`.
That field is populated by `trust-consumer::process::process_message`
(`crates/trust-consumer/src/process.rs:538-766`) exactly once per
`train_id`, on the first Movement after this process itself parked that
`train_id`'s Activation (`ProcessorState::pending_activations`) — a
requirement the module's own doc comment (`process.rs:32-99`) names
directly: *"A tracked train can stay `resolution_status = 'pending'` in the
database forever even while this process tracks it correctly... If the
Activation arrived before this process started, was pruned as expired, or
was simply never emitted on the slice of the feed this consumer sees, the
resolving Movement goes out with `resolved_train_uid: None`... it stays that
way indefinitely: nothing re-attempts the binding."*

Production redeploys `trust-consumer` roughly every 1.5-2h (confirmed via
`kubectl get events` this session, showing uniform pod ages across
Flux/Helm reconciliation cycles) — so a real, non-rare fraction of trains
have their Activation observed by a pod that is no longer running by the
time their origin-departure Movement would otherwise resolve them.

**A second, code-confirmed mechanism reaches the identical stuck state
without any redeploy at all**, worth naming precisely since it explains why
"movements exist, `resolution_status` stuck at `pending`" is a real,
reproducible shape and not only a redeploy-timing coincidence. Review
finding I6 (`process.rs:521-537`) fans one TRUST message out to one event
per subscriber sharing a `train_uid`, all posted in the same
`post_train_events` HTTP call (`crates/trust-backlog-consumer`... no —
`crates/trust-consumer/src/main.rs:151-176`, `queries::post_train_events`
iterating events). `process_message`'s own state mutation
(`state.resolved.insert(...)`) happens *before* that post is attempted, per
the module's own documented "Known simplification" (`process.rs:57-93`). If
that HTTP call fails partway — one subscriber's event already applied,
a later one in the same batch errors — the whole batch is left uncommitted
and is redelivered whole on the next poll. On redelivery,
`state.resolved.get(&movement.train_id)` now returns `Some` (mutated on the
first, partially-failed attempt), so the retry takes the "already resolved"
branch and computes `freshly_resolved` from
`activation_matched_awaiting_movement` — `false` for the plain CRS+time
match path, since that flag only exists for the Activation-direct-match fast
path. The redelivered event for the subscriber whose write failed the first
time therefore carries `resolved_train_id: None` on retry: `upsert_train_movement`
still writes `train_movement_events` (keyed on `trains_id`, shared across
subscribers, so it is now present for this `trains_id`), but
`flip_legacy_resolution` is never called for that specific subscription,
because `upsert_train_event`'s own `match &event.resolved_train_id { Some(_)
=> ..., None => None }` skips it entirely. Note `trains.train_id`
(live-TRUST id) is *not* lost in this scenario either — it was already set
by whichever subscriber's write succeeded, since `mark_train_resolved`
operates per-`trains_id`, not per-subscription. The result is a real,
reachable state: `trains.train_id` known, `train_movement_events` rows
present, one specific subscriber's own `train_subscriptions.resolution_status`
stuck at `'pending'` — exactly the "display bug" variant this fix's brief
asks to be covered by a real test, distinct from the zero-movement-events
live example below.

**Live-confirmed example of the *other* variant (Activation genuinely never
observed, zero movement events at all)**: `train_uid=L78659`,
`service_date=2026-09-08`, `trains_id=35879`, `train_subscriptions.id=23` —
scheduled origin departure 17:44:00, still `pending` at 21:29:55 (3h46m
later), zero rows in `train_movement_events` for `trains_id=35879`, zero log
lines mentioning it in any pod's current lifetime. This variant has no
`train_movement_events` evidence to reconcile from — see §3 (Decision 1) for
why this document does not attempt to fix it, and what it gets instead.

**No reconciliation job exists anywhere.** Grepped every writer of
`resolution_status` across the whole workspace (table above, §1): the four
values are written only by the `train_subscriptions` table's own `DEFAULT`
(`pending`), `apply_schedule_match` (`pending -> schedule_matched`,
`train_tracking.rs:802-811`), and `flip_legacy_resolution` (`-> resolved`,
above). `trust-consumer`'s `reference_reload`
(`crates/trust-consumer/src/main.rs:87-118`) only reads `api`'s current
state to reseed its own in-memory maps — `process::apply_reference_reload`
(`process.rs:257-328`) never writes anything back to the database.

### 1.2 Stall 2 — NR-primary tracking has exactly one, at-creation-time-only, backlog-only enrichment attempt

`create_subscription_for_train` (`train_tracking.rs:193-225`, called from
`post_track_by_uid`, `crates/api/src/routes/train.rs:737-753`) sets
`trains_id` immediately, at row-creation time. `apply_schedule_match`'s own
gate (`WHERE trains_id IS NULL AND resolution_status = 'pending'`,
`train_tracking.rs:802-811`) is therefore false by construction the instant
this row exists, and `list_pending_pins_for_schedule_match`'s periodic sweep
(`train_tracking.rs:904-915`, driven by `schedule_match_sweep_loop`,
`crates/api/src/main.rs:129-150`) explicitly excludes `trains_id`-bearing
rows too — this exclusion is correct for what that sweep actually does
(match a bare CRS+time pin against `schedule_line_population`, which an
NR-primary row has no `pin_origin_crs`/`pin_scheduled_departure` to feed it
in the first place), not a bug in that sweep. This gap is already named
in-code as "review finding I1" at both gate sites.

The one enrichment path that exists, `enrich_shared_train`
(`routes/train.rs:789-867`), runs exactly once, synchronously, inside
`post_track_by_uid`'s own request — never again. It tries
`trust_event_backlog_match::attempt_backlog_match_by_uid` first (replays
retained TRUST history, if any, to discover a real observed origin
departure), and only if that succeeds does it fall through to
`schedule_matching::attempt_schedule_match_for_shared_train`
(`schedule_matching.rs:253-296`) using the *replayed* `(origin_crs,
scheduled_departure)` as the match key. If the backlog has nothing for this
`train_uid` — the train hasn't run yet, or its origin CRS falls outside
`trust_event_backlog`'s catalogued-line scoping, or the backlog's own short
retention window has already rolled past it — `enrich_shared_train` returns
having written nothing, and nothing ever tries again.

**The gap named precisely**: `schedule_destination_departures` — the
CIF-derived, calling-point-keyed table backing `/trains/search`
(`crates/api/migrations/20260907130000_schedule_destination_departures.sql`,
generalized to calling-point-first search by
`docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md`,
already merged onto this branch — confirmed via `git log`) is a second,
completely independent source for the same `(origin_crs, scheduled_departure)`
key `attempt_schedule_match_for_shared_train` already knows how to consume.
It requires no TRUST data at all — CIF SCHEDULE is published well before a
train ever runs — and nothing in this codebase reads it for tracked-train
enrichment purposes today. **Live-confirmed**: `L78659`'s full 9-stop
AON→WAT timetable exists in `schedule_destination_departures` for
`service_date = 2026-09-08` right now, while `trains.origin_crs`,
`destination_crs`, `calling_points`, `matched_line_id`, `schedule_matched_at`
are all `NULL` for `trains_id = 35879`.

## 2. Reconstructing calling points from `schedule_destination_departures`: a real shape mismatch, and why the fix reuses the existing matcher instead of hand-rolling a second writer

`schedule_destination_departures` stores **one row per departure-bearing
calling point**, keyed `(service_date, destination_crs, scheduled,
train_uid, origin_crs)` with `origin_crs` meaning "the calling point *this
row represents*" (its own doc comment, and the calling-point-search
migration's), plus `true_origin_crs` (the whole schedule's real origin,
added by the calling-point-search migration). For one `(train_uid,
service_date)` it therefore has one row per departure-bearing stop of that
schedule — but **no TIPLOC, no arrival time, no "kind" (Origin/
Intermediate/Terminate), no half-minute flags, and no row at all for the
schedule's own final destination** (a terminating stop never departs, so it
earns no row in this table).

`trains.calling_points` is not a free-form blob: it is written exclusively
via `ScheduleCallingPointDto` (`schedule_matching.rs:54-76`) —
`{tiploc, kind, bookedArrival, bookedDeparture, isHalfMinuteArrival,
isHalfMinuteDeparture}` — and read back verbatim, opaque, by
`TrackedTrainState`/`PublicTrainState` (`train_tracking.rs:963`,
`trains.rs`'s `PublicTrainState::calling_points`) straight into the
frontend's `ScheduleCallingPoint` TypeScript type
(`frontend/lib/types.ts:367-373`), which types `tiploc: string` and
`kind: ScheduleCallingPointKind` as **required, non-null** fields. A
hand-rolled JSON array built from `schedule_destination_departures`'
CRS-only, no-TIPLOC, no-kind rows would violate that contract the moment it
reached the frontend — exactly the "half-populate a row into an
inconsistent state some other reader assumes can't happen" failure mode
this fix's brief warned against.

**Decision: do not build a second, parallel calling-points writer.**
`schedule_destination_departures` is used *only* to recover the two scalars
`attempt_schedule_match_for_shared_train` already needs as input —
`origin_crs` and `scheduled_departure` — sourced from the one row per
`(train_uid, service_date)` where `origin_crs = true_origin_crs` (the row
representing the schedule's own true origin departure). That function's
existing internal path (`find_schedule_match` against
`schedule_line_population`, which *does* carry full TIPLOC/kind/arrival/
half-minute data straight from CIF) does the actual matching and writing,
via the same, already-tested, `COALESCE`-safe
`find_or_create_train_with_schedule_match` (`trains.rs:87-124`) every other
schedule-match path uses. This is the same role
`trust_event_backlog_match::attempt_backlog_match_by_uid`'s replayed
`origin_departure` already plays inside `enrich_shared_train` today (§1.2)
— this fix adds a second, independent *source* for that same
`(origin_crs, scheduled_departure)` key, not a second *consumer* of it.
Every invariant `attempt_schedule_match_for_shared_train` already enforces
(UID-match validation against a possible different service at a busy
terminus, `COALESCE`-safe writes that never clobber an earlier match) is
therefore inherited for free, unchanged, and unit-tested already.

## 3. Decisions

### Decision 1: Stall 1's fix is a single `UPDATE ... WHERE EXISTS`, and does not attempt the zero-movement-events variant

```sql
UPDATE train_subscriptions
SET resolution_status = 'resolved'
WHERE resolution_status = 'pending'
  AND trains_id IS NOT NULL
  AND EXISTS (
      SELECT 1 FROM train_movement_events tme WHERE tme.trains_id = train_subscriptions.trains_id
  )
```

This exactly replicates `flip_legacy_resolution`'s own post-Task-22
invariant — it writes `resolution_status` alone, nothing else — because
that is the *only* column left for it to write: `trains_id` is already set
(the `WHERE` clause requires it), and every other column
`flip_legacy_resolution` used to also write (`train_uid`/`train_id`/
`resolved_at`) was dropped from `train_subscriptions` entirely by Task 22's
migration and now lives exclusively on the shared `trains` row, written by
`mark_train_resolved` — which, per §1.1's traced mechanism, is already
correctly set by whichever subscriber's write succeeded, independent of
which specific subscriber's own status column is stuck. One statement, not
a per-row loop: there is nothing per-row about this fix (no external call,
no matching heuristic to run) — a bulk `UPDATE` is both simpler and cheaper
than `run_schedule_match_sweep`'s per-row loop shape, which exists there
only because each row needs its own `attempt_schedule_match` call.

**Deliberately does not attempt the `L78659`-style zero-`train_movement_events`
variant.** There is no honest way to distinguish "this train's Activation
was genuinely missed and it is quietly running right now" from "this train
was cancelled/never ran/never will" from presence alone — the whole reason
this variant exists is that this codebase has no persisted record of the
train's real live behaviour at all. Flipping such a row to `'resolved'`
without any observed data to justify it would be actively dishonest, the
same reasoning `attempt_schedule_match_for_shared_train`'s own UID-mismatch
guard (§2) already applies elsewhere in this exact codebase. What this
variant gets instead: (a) Stall 2's fix, below — a real schedule display,
even while `resolution_status` stays `pending`; (b) live TRUST resolution
continues to work normally going forward if the train's Activation simply
hasn't happened yet (a plausible reading of the L78659 example — 17:44
scheduled, but rows outside this fix's own regression tests are not proof
of *why* a specific live example is stuck, only that it is); (c) if a
future need arises to also treat "past scheduled departure by a wide margin,
still zero movement events, presumed cancelled or missed" as its own signal,
that is a genuinely different, riskier decision (asserting something never
observed) requiring its own review — named here as a residual gap, not
solved.

### Decision 2: Stall 2's fix scopes to actively-subscribed trains, not every `trains` row in existence

```sql
SELECT DISTINCT tr.id, tr.train_uid, tr.service_date
FROM trains tr
JOIN train_subscriptions ts ON ts.trains_id = tr.id
WHERE tr.schedule_matched_at IS NULL
```

`trains` rows can exist with no subscriber at all — `find_or_create_trains_batch`
(`trains.rs:44-77`) is a batch primitive intended for exactly that use
(broad ingestion with no per-user pin), even though its only current caller
in this codebase is test-only pending Stall 1's noted §1 finding that
`trust-backlog-consumer` doesn't yet write through the shared tables.
Scoping this sweep to `trains` rows an actual `train_subscriptions` row
still points at keeps its cost bounded by *tracked* trains — the population
this fix exists to serve — rather than by every train this system has ever
incidentally observed, and keeps its purpose narrow and legible: this is a
tracked-train reconciliation job, not a general-purpose schedule-backfill
job for the whole network. If broad, subscriber-less enrichment is ever
wanted, that is a distinct, separately-scoped feature, not a free side
effect of this fix.

For each candidate row, a lookup against `schedule_destination_departures`
recovers the true origin departure:

```sql
SELECT origin_crs, scheduled, destination_crs
FROM schedule_destination_departures
WHERE train_uid = $1 AND service_date = $2 AND origin_crs = true_origin_crs
LIMIT 1
```

— `origin_crs = true_origin_crs` is what selects the one row (of the
several this `(train_uid, service_date)` may have, one per calling point)
that represents the schedule's *own* origin departure, not an intermediate
stop's. `scheduled` (a bare `TIME`) is combined with `service_date` and
converted London-local -> UTC via the same `eta_blend::london_to_utc`
helper `find_schedule_match` already uses internally
(`schedule_matching.rs:193`), producing exactly the `(origin_crs: &str,
scheduled_departure: DateTime<Utc>)` pair `attempt_schedule_match_for_shared_train`
takes as input. A miss (no row at all — CIF hasn't published this service_date
yet, or the `train_uid` never had a NR-primary tracking request that reached
this far) is a normal, silent skip, logged at `debug`, exactly
`enrich_shared_train`'s own posture for the equivalent "no backlog history"
case.

### Decision 3: A 30-minute grace period past scheduled departure, gating Stall 2's retry — not a data-availability requirement, a first-mover courtesy to the live/backlog paths

CIF SCHEDULE data is published well in advance of a train running — there
is no data-availability reason to wait before querying
`schedule_destination_departures`. The grace period exists instead so this
sweep is not the *first* thing to attempt enrichment for a freshly-created
NR-primary subscription, redundantly repeating work `enrich_shared_train`
already just attempted at creation time (§1.2), and so live TRUST
resolution / a genuine backlog match — both of which carry strictly richer
information (a real observed departure, not just a booked one; TRUST's own
`train_id` for live status) — get first crack at a train that is about to
run or has just started.

**30 minutes**, chosen as comfortably past `common::MATCH_TOLERANCE`
(±20 minutes — the same tolerance window `find_schedule_match`'s own CRS+time
matching already uses, `schedule_matching.rs:192`, and the same value the
2026-09-05 trust-event-backlog design's own Decision 3 reuses for its
CRS+time backlog lookup) — by the time this sweep considers a row, the
normal live-Activation-to-Movement matching window that `trust-consumer`
uses has already fully elapsed, so falling back to the CIF-only match is not
competing with, only backstopping, the live path. Documented here as a
choice within the brief's own suggested 15-30 minute range, at the upper
(more conservative, more courteous-to-the-live-path) end, since — unlike
Stall 1's fix — a too-early Stall 2 attempt costs a real `schedule_line_population`
lookup and `crs_line_index` scan for no benefit, not just a wasted no-op
`UPDATE`.

Computed in Rust, not SQL: `now - scheduled_departure_utc >=
Duration::minutes(30)`, evaluated per-row after the
`schedule_destination_departures` lookup and its London-local -> UTC
conversion above — the same "loop per row, call the existing single-row
function" shape `run_schedule_match_sweep` already uses
(`schedule_matching.rs:313-352`), not a new bespoke SQL predicate.

### Decision 4: Placement — a second background loop in `crates/api`, mirroring `schedule_match_sweep_loop`, not a job in `crates/aggregator`

The task brief's own starting suggestion was to follow `crates/aggregator`'s
prune-job pattern (`prune_schedule_destination_departures`,
`prune_trains`, et al. — `crates/aggregator/src/queries.rs`, run every
`poll_interval_secs` from `crates/aggregator/src/main.rs`'s single loop).
That pattern is real, but it is retention/deletion work, running in a
separate binary with no dependency on `crates/api`'s own business-logic
modules (`schedule_matching`, `trains`, `train_tracking`) at all — using it
here would mean either duplicating `attempt_schedule_match_for_shared_train`'s
matching/writing logic into `aggregator`, or having `aggregator` call back
into `api` over HTTP for what is fundamentally the same class of operation
`schedule_match_sweep_loop` already performs in-process.

**A stronger, more directly-applicable precedent exists in `crates/api`
itself**: `schedule_match_sweep_loop` (`crates/api/src/main.rs:129-150`,
spawned via `tokio::spawn` alongside the HTTP server, `main.rs:14`) is
already exactly this shape — "a service that is mostly a request/response
server also runs one background interval loop that periodically retries a
business-logic operation this same crate already knows how to perform
synchronously." This fix adds a second, sibling loop of the identical
shape, in the same crate, calling the same kind of already-existing
functions (`attempt_schedule_match_for_shared_train`, a new
`reconcile_stuck_resolution_status`) directly, in-process, with no new
service, no new HTTP route, no new deployment unit.

New module `crates/api/src/data/reconciliation.rs`:

- `reconcile_stuck_resolution_status(pool: &PgPool) -> anyhow::Result<u64>`
  — Decision 1's bulk `UPDATE`, returning `rows_affected()`.
- `retry_schedule_enrichment_for_nr_primary_trains(pool: &PgPool,
  crs_line_index: &HashMap<String, Vec<String>>, grace_period:
  chrono::Duration) -> anyhow::Result<u64>` — Decisions 2-3's per-row loop,
  returning a count of trains actually matched, same "log and skip, one bad
  row doesn't stop the sweep" posture as `run_schedule_match_sweep`.
- `run_reconciliation_sweep(pool: &PgPool, crs_line_index: &HashMap<String,
  Vec<String>>, grace_period: chrono::Duration) -> anyhow::Result<ReconciliationSweepResult>`
  — calls both, returns a small struct of both counts, logged by the new
  loop exactly as `schedule_match_sweep_loop` logs `matched`.

New `crates/api/src/main.rs` function `reconciliation_sweep_loop(app: App)`,
spawned alongside `schedule_match_sweep_loop` at `main.rs:14`, identical
`tokio::time::interval` shape.

### Decision 5: Interval — reuse `schedule_match_interval_secs`'s own 300s default and reasoning, as a new, independent config field

New `ServiceArguments` fields (`crates/api/src/data/config.rs`, alongside
`schedule_match_interval_secs`):

```rust
#[arg(long, env, default_value_t = 300)]
pub reconciliation_sweep_interval_secs: u64,

#[arg(long, env, default_value_t = 30)]
pub schedule_enrichment_grace_minutes: i64,
```

**300 seconds (5 minutes), the same default `schedule_match_interval_secs`
already established** (`config.rs`, cited in full in §1.2) for the
structurally identical concern ("periodic retry of a thing that should have
happened automatically, cheap enough at this cadence not to matter, frequent
enough that a fix lands within a rail day's working hours"). Stall 1's own
per-tick cost is a single indexed `UPDATE ... WHERE EXISTS` — cheaper than
`schedule_match_interval_secs`'s own per-row sweep — so there is no reason
to run it less often; Stall 2's cost is bounded by Decision 2's
subscriber-scoping and gated by Decision 3's grace period, keeping its
per-tick candidate set small in practice. A single shared interval for both
halves keeps one config knob, one loop, one log line to look for — split
intervals were considered and rejected as unwarranted complexity for two
operations already this cheap and already this closely related.

Two separate config fields, not one: the interval governs *how often* the
sweep runs; the grace period governs *which rows* Stall 2's half is willing
to touch on a given run, per Decision 3. Conflating them would make it
impossible to tune one without the other.

## 4. Non-goals

- **Fixing the zero-`train_movement_events` variant of Stall 1** (the
  `L78659` live example itself). Explicitly not attempted — Decision 1
  explains why asserting `'resolved'` with no observed data to back it
  would be dishonest, not merely incomplete.
- **Any change to how `schedule_destination_departures` itself is
  populated or retained.** This fix is a new *reader* of that table only.

  **Correction applied during final whole-branch review**: this bullet
  originally also claimed no new *index* was needed, reasoning that the
  existing `(service_date, origin_crs, scheduled, train_uid)` calling-point
  index would serve `true_origin_departure`'s lookup. Checked again and
  found wrong: that index (and the table's primary key,
  `(service_date, destination_crs, scheduled, train_uid, origin_crs)`)
  both lead with columns other than `train_uid`, so neither can seek on it
  — `true_origin_departure`'s `WHERE train_uid = $1 AND service_date = $2`
  degenerates to a scan of the whole day's rows (up to ~377k at national
  scale) per candidate per sweep tick. A new index,
  `schedule_destination_departures_train_uid_idx (service_date, train_uid)`
  (`crates/api/migrations/20260908140000_schedule_destination_departures_train_uid_idx.sql`),
  was added to fix this — this IS the UID-keyed reverse schedule index
  `2026-09-06-shared-train-identity-design.md` §1 named as an accepted,
  deferred gap, added now that this fix gives it a concrete, justified
  caller (it also benefits `trains::is_known_scheduled_train`'s identical
  lookup shape). `list_trains_needing_schedule_enrichment`'s candidate
  query was additionally bounded to `schedule_destination_departures`' own
  2-day retention window, so an older, permanently-unenrichable subscribed
  row stops being re-selected as a candidate on every sweep tick.
- **Broad, subscriber-less schedule enrichment** for every `trains` row
  this system has ever observed. Decision 2 explicitly scopes to
  subscriber-referenced rows only; a general-purpose backfill is a
  separate, unscoped feature.
- **Changing `crates/trust-consumer`'s in-process design or
  `flip_legacy_resolution`'s call site inside `upsert_train_event`.** This
  fix adds a reconciliation path alongside the live one; it does not touch
  or replace it.
- **Any further change to `trust-backlog-consumer`'s or
  `ingest_shared_movements_batch`'s write path.** §1's corrected finding is
  that this path has already shipped and is one of the two independent
  mechanisms motivating this fix, not something this fix modifies further.
- **The destination-arrival-time filter on `/trains/search`, and the
  schedule-first train detail page redesign.** Two features being worked
  on concurrently in separate worktrees, per this task's own brief.
  Confirmed no file overlap with this fix's own touched files
  (`crates/api/src/data/{reconciliation,train_tracking,schedule_matching}.rs`,
  `crates/api/src/main.rs`, `crates/api/src/data/config.rs`) — the
  calling-point-train-search work is already merged onto this branch (§1.2),
  so this fix is written against its landed shape, not a stale one.
- **Per-station stats / metrics work.** Unrelated, per this task's own
  brief.

## 5. Open questions / risks

1. **The zero-movement-events Stall 1 variant (`L78659`) has no fix in this
   design**, named as a residual gap in Decision 1 — worth a follow-up
   discussion if it turns out to affect a meaningful fraction of tracked
   trains, once this fix's own telemetry (the reconciliation sweep's log
   line, Decision 4) gives real numbers on how often Stall 1's *fixable*
   variant fires versus how often a row is still stuck after the sweep runs.
2. **Whether `reconciliation_sweep_interval_secs` should eventually diverge
   from `schedule_match_interval_secs`** if real production data shows one
   half of the combined sweep is meaningfully more/less expensive than
   assumed in Decision 5 — not resolved here, deliberately deferred until
   there is real cadence data to act on, same posture this codebase already
   takes for several of its other unmeasured constants.
3. **Grace period tuning (Decision 3).** 30 minutes is a documented choice
   within the brief's suggested range, not a measured optimum — if the
   live/backlog paths turn out to reliably resolve well inside 20 minutes in
   practice, a shorter grace period could close Stall 2's window faster with
   no real cost; this needs real timing data this fix does not yet have.
