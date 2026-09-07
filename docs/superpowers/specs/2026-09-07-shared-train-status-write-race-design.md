# Design: The `train_current_state` Write Race Between `trust-consumer` and `trust-backlog-consumer`

**Status: design proposal, not approved. Spec stage only — no code, no
migration, no config change in this pass. This document exists specifically
to put an explicit decision in front of a human decision-maker, per the
prior whole-branch review that flagged this issue as "needing an explicit
design decision, not something safe to improvise." Nothing here should be
implemented until one of §4's options (or a variant of one) is picked.**

## Why this document exists

A prior whole-branch code review of the shared-train-identity work
(`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md`,
implemented by
`docs/superpowers/plans/2026-09-06-shared-train-identity-implementation-plan.md`)
flagged that two independent, continuously-running processes both write to
`train_current_state` for the same `trains_id`, with no ordering or
monotonicity guard between them:

1. `crates/trust-consumer` — posts live TRUST movement/cancellation events
   for subscriptions it is actively tracking.
2. `crates/trust-backlog-consumer` — independently derives and writes its
   own view of the same row for every train it sees that passes its own
   scope filter (below), not just tracked ones.

The original design spec said persistence "moves entirely to
`trust-backlog-consumer`," implying `trust-consumer` would stop writing
`train_current_state`/`train_movement_events` itself. **This document
verifies, directly against the code on `main` today, that this migration
never happened: both paths are live in production, both write the exact
same table, and there is no coordination between them.** It confirms the
review's mechanism, adds a finding the review didn't have (§1.4 —
`trust-backlog-consumer`'s write path is genuinely narrower in coverage
than `trust-consumer`'s, not equivalent, which changes the risk profile of
naively retiring the latter), and lays out two concrete resolution options
plus a recommendation, for a human to approve before any implementation
plan is written.

Required reading consumed in full before this document was written:
`docs/superpowers/specs/2026-09-06-shared-train-identity-design.md` (the
design that introduced this dual-writer situation, in particular §3);
`docs/superpowers/plans/2026-09-06-shared-train-identity-implementation-plan.md`
(the 24-task plan that implemented it, in particular Tasks 16-17 and the
Architecture line); `crates/trust-consumer/src/{main,process}.rs`;
`crates/trust-backlog-consumer/src/{main,process,crs_index}.rs`;
`crates/api/src/routes/ingest.rs`;
`crates/api/src/data/{train_tracking,trust_event_backlog,trust_event_backlog_match,trains}.rs`;
`crates/trust-schema/src/journey.rs`;
`crates/common/src/lib.rs` (`TrainMovementEventMessage`,
`TrustBacklogEventMessage`);
`docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md` (Decision
2, the catalogued-line CRS scoping this document leans on in §1.4).

## 1. Current state, verified against code on `main`

### 1.1 Both write paths are live today

**`trust-consumer`'s path.** `crates/trust-consumer/src/main.rs:151-176`:
every cycle, the batch of derived `TrainMovementEventMessage`s is posted
via `queries::post_train_events` to `config.api_ingest_url`
(`main.rs:157-158`) **unconditionally**, before a *separate*, additional
call builds and posts forwarding signals
(`process::build_forward_signals` /
`queries::post_train_forward_signals`, `main.rs:159-172`) to the
newer `notifier_forward_queue` mechanism. The forwarding-signal write was
added *alongside* the original event-posting call, not in place of it —
confirmed directly: there is no code path in `main.rs` that skips
`post_train_events`.

That POST lands on `POST /train-events`
(`crates/api/src/routes/ingest.rs:59`), handled by `post_train_events`
(`ingest.rs:237-249`), which calls
`queries_train_tracking::upsert_train_event` for every event in the batch
— **this is a direct, independent write to `train_movement_events` and
`train_current_state`.**

**`trust-backlog-consumer`'s path.** Its batch of
`TrustBacklogEventMessage`s lands on `POST /trust-event-backlog`
(`ingest.rs:68-71,270-294`), handled by `post_trust_event_backlog`. That
handler does two things, confirmed by reading it directly
(`ingest.rs:270-294`):

1. `upsert_trust_event_backlog_batch` — the original, narrow
   `trust_event_backlog` archival write (unrelated to this race).
2. **A second, separate loop that calls
   `crate::data::trust_event_backlog::ingest_shared_movement` for every
   event** (`ingest.rs:285-291`) — its own doc comment names this
   explicitly: "Additional, parallel write onto the shared
   trains/train_movement_events/train_current_state tables." A per-event
   failure here is logged and swallowed, never propagated, so it can't
   even fail the request that would surface the problem.

`ingest_shared_movement` (`crates/api/src/data/trust_event_backlog.rs:63-131`)
does its own independent read-modify-write of `train_current_state`: reads
the current row via `fetch_previous_derived_state`
(`trust_event_backlog.rs:75,134-168`, a plain `SELECT ... WHERE trains_id =
$1`, no lock, no transaction spanning the read and the eventual write),
derives new state via `trust_schema::journey::apply_movement`/
`apply_cancellation` against that read, then calls the exact same
`upsert_train_movement` function `trust-consumer`'s path also calls
(`trust_event_backlog.rs:131`).

**Conclusion of 1.1: both processes are live, both write the same table,
confirmed by reading the current wiring end-to-end, not inferred.**

### 1.2 Both paths converge on one function with no ordering guarantee at all

`upsert_train_movement` (`crates/api/src/data/train_tracking.rs:506-558`)
is the single point both writers fall through to. Its `train_current_state`
upsert:

```sql
INSERT INTO train_current_state
    (trains_id, status, last_reported_location, last_event_type,
     delay_minutes, next_calling_point, eta_next, eta_source, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET
    status                  = EXCLUDED.status,
    last_reported_location  = EXCLUDED.last_reported_location,
    last_event_type         = EXCLUDED.last_event_type,
    delay_minutes            = EXCLUDED.delay_minutes,
    next_calling_point       = EXCLUDED.next_calling_point,
    eta_next                 = EXCLUDED.eta_next,
    eta_source               = EXCLUDED.eta_source,
    updated_at               = NOW()
```

This is a **blind last-write-wins overwrite**. There is no `WHERE` clause
comparing the incoming event's own timestamp against what's already
stored, no sequence number, nothing. Whichever caller's `UPDATE` commits
last always wins, full stop, regardless of which one carries more current
real-world information. `updated_at = NOW()` records *when Postgres
applied the write*, not when the underlying TRUST event happened — it
provides no protection at all, since it always advances forward
irrespective of which writer produced it.

**Contrast with the rest of this same schema, deliberately**: every other
dual-write path this design introduced onto the shared `trains` row uses
`COALESCE` guards specifically to make concurrent/repeated writes order-
independent — e.g. `find_or_create_train_with_schedule_match`
(`crates/api/src/data/trains.rs:40-75`): `origin_crs = COALESCE(trains.origin_crs,
EXCLUDED.origin_crs)`, and identically for every other schedule column.
The codebase already knows how to make this kind of dual-writer situation
safe; `upsert_train_movement`'s `train_current_state` write is the one
place in this whole design that doesn't apply that pattern, because
`train_current_state` is a "latest wins" table by nature (each new event
should legitimately overwrite the last) — the missing piece isn't a
COALESCE, it's a guard on *which write is actually the latest one*.

### 1.3 The two writers don't just race — they derive state independently, from independent "previous" views

This makes the effective bug worse than a plain double-write of identical
data. `trust_schema::journey::apply_movement` (`crates/trust-schema/src/journey.rs:29-33`)
is a pure fold: `new_state = apply_movement(previous_state, event)`. Each
consumer supplies its own `previous_state`, from an independent source:

- `trust-consumer` folds against `ProcessorState.last_derived`
  (`crates/trust-consumer/src/process.rs:203-208,702-712`), an in-process
  `HashMap<train_id, DerivedState>` built up from every message *this
  process itself* has seen since its last restart.
- `trust-backlog-consumer`'s `ingest_shared_movement` folds against
  whatever is *currently sitting in the database row*
  (`fetch_previous_derived_state`, read fresh on every single event, no
  transaction).

If the two processes see the same national TRUST feed in a different
relative order (a plain reality of independent Kafka/Redis-Stream consumer
groups, both `movement-relay`-fed but not synchronized with each other),
their two folds can diverge for the exact same train even before either
one writes — one might already reflect a later event the other hasn't
folded in yet, or vice versa. The last one to commit its `UPDATE`
overwrites the other's fold in full, not merged, not compared — so the
row can regress to a state derived from an older real-world event even
though the previous writer had already applied a newer one.

### 1.4 Verified: `trust-backlog-consumer`'s coverage is narrower than `trust-consumer`'s, not a superset

The task brief asked whether `trust-consumer`'s write path provides any
capability `trust-backlog-consumer`'s ingestion doesn't already cover.
**It does — three concrete gaps, confirmed by reading the filter logic
directly, plus one non-overlapping side effect:**

`trust-backlog-consumer`'s `process_message`
(`crates/trust-backlog-consumer/src/process.rs:93-148`) drops a Movement
event unless **all** of the following hold:

- `event_type` is `ARRIVAL` or `DEPARTURE` — a `PASS` event is dropped
  unconditionally (`process.rs:96-98`).
- Its STANOX translates to a real CRS at all — an unmapped/untranslatable
  STANOX drops the event via the `?` short-circuit
  (`process.rs:100-103`).
- That CRS is in `crs_index`, a **static, bounded set built once from the
  catalogued `lines/*.toml` reference data** — "every catalogued-line CRS
  with at least one TIPLOC-bearing station"
  (`crates/trust-backlog-consumer/src/crs_index.rs:1-2,30-40`), **not**
  every CRS in the country. A location genuinely off that catalogue is
  silently out of scope (`process.rs:104-106`, exercised directly by its
  own test `a_departure_at_an_uncatalogued_crs_is_dropped`,
  `process.rs:290-`).

`trust-consumer`'s own `process_message`
(`crates/trust-consumer/src/process.rs:594-766`) applies none of these
filters once a train_id is resolved: every event type (including `PASS`),
every location (including one whose STANOX doesn't translate at all — it
falls back to the raw STANOX string rather than being dropped, per
`journey::apply_movement`'s own documented fallback), and every CRS
(catalogued or not) all still produce a written event.

**Net effect: for a train whose live journey passes through PASS events,
or through a station off the static line catalogue, or through a STANOX
this codebase's reference table doesn't translate, `trust-consumer` is
today the *only* writer that keeps `train_current_state` current at all**
— `trust-backlog-consumer` simply never sees those specific movements as
shared-store-worthy, regardless of whether the train is tracked.

**The non-overlapping side effect**: `trust-consumer`'s path also flips
`train_subscriptions.resolution_status` to `'resolved'` via
`flip_legacy_resolution` (`train_tracking.rs:599-634`, called from
`upsert_train_event`, `train_tracking.rs:649-660`) — a write
`trust-backlog-consumer`'s `ingest_shared_movement` never performs (it
only ever touches `trains`/`train_movement_events`/`train_current_state`,
confirmed directly against `trust_event_backlog.rs:63-131` — it has no
knowledge of `train_subscriptions` at all). This flip is still genuinely
load-bearing: the CHECK constraint on `train_subscriptions.resolution_status`
was never narrowed to the design spec's originally-stated end state — it
remains `('pending', 'schedule_matched', 'resolved', 'unresolved')`
(`crates/api/migrations/20260905150000_schedule_matched_resolution.sql:22-24`,
unchanged by the implementation plan's migrations,
`crates/api/migrations/2026090[6-7]*.sql`), and other code
(`crates/api/src/data/trust_event_backlog_match.rs:20-23,40-58`) explicitly
depends on this value reaching `'resolved'`.

**A third, bounded writer exists and is out of scope here**:
`trust_event_backlog_match.rs`'s one-shot backlog-replay path
(`replay_backlog_history`) also calls `upsert_train_event` — once, at the
moment a late-tracking subscription is created, to backfill its history.
It is not a second continuously-competing process the way the two
consumers above are, and this document does not attempt to resolve it —
noted only so a future reader doesn't mistake its omission for an
oversight.

## 2. Is this a real risk or a theoretical one?

Real, not theoretical, for the specific product surface this design exists
to serve: the public train-status page reads `train_current_state`
directly. Two ordinary operational conditions make the race routine rather
than rare:

- Both consumers restart independently (deploys, crashes, node
  rescheduling) and resume from their own committed offsets — there is no
  reason to expect them to be "caught up to the same point in the feed" at
  the same wall-clock moment, ever.
- Kafka/Redis-Stream partitioning and batching mean two consumer groups
  reading the same topic do not process messages in lockstep even under
  normal operation — one can legitimately be several batches behind the
  other at any instant.

Given §1.2's blind overwrite and §1.3's independent derivation, the
concrete failure mode is: a tracked train's public status page regresses
to a stale `status`/`last_reported_location`/`delay_minutes` because the
lagging writer's `UPDATE` for an older message committed after the leading
writer's `UPDATE` for a newer one. This is silent — no error, no log
signal distinguishing a legitimate new event from a regression, since both
look identical to Postgres (an `UPDATE` that succeeded).

## 3. What does *not* need solving here

- Whether `trust_event_backlog`'s own archival table
  (`upsert_trust_event_backlog_batch`) has an ordering issue — it doesn't;
  it's `INSERT ... ON CONFLICT (dedup_key) DO NOTHING`
  (`trust_event_backlog.rs:19-50`), append-only and idempotent, no
  "current state" concept to race over.
- `trust_event_backlog_match.rs`'s one-shot replay path (§1.4) — bounded,
  triggered once per newly-created late-tracking subscription, not a
  standing competing writer.
- Redesigning `journey::apply_movement`'s derivation logic itself — it is
  correct as a pure fold; the problem is entirely about *whose* fold wins
  when two independent folds exist for the same row.

## 4. Options

### Option A — Retire `trust-consumer`'s independent write to the shared tables

Stop `trust-consumer` from writing `train_movement_events`/
`train_current_state` at all; keep it purely as the notifier-forwarding
role its `notifier_forward_queue` mechanism already gives it
(§1.1). This is what the original design spec's §3 said would happen.

**What it would actually require, now that §1.4 is verified:**

1. Split `upsert_train_event`'s two effects apart for real (the design
   spec itself anticipated exactly this kind of split for a different
   reason, calling it out as "Split the combined upsert function" in §3):
   a small function/route that performs only `flip_legacy_resolution`'s
   `train_subscriptions.resolution_status` flip, with no call into
   `upsert_train_movement` at all. `trust-consumer`'s cycle would call
   that instead of today's `post_train_events`.
2. **Close, or knowingly accept, the three coverage gaps in §1.4** before
   this is safe to ship: widen `trust-backlog-consumer`'s scope to also
   forward `PASS` events, drop the catalogued-line `crs_index` filter (or
   widen it) for at least tracked trains, and stop dropping events whose
   STANOX doesn't translate. Each of these is itself a real, non-trivial
   change to a different crate's filtering rules — this is not a
   same-sized companion change to (1), it's the larger and riskier part of
   this option.
3. Leave `upsert_train_event`/`upsert_train_movement` in place, unused by
   `trust-consumer` going forward but still called by
   `trust_event_backlog_match.rs`'s one-shot replay (§1.4) — re-document
   it as that path's function, not a live per-cycle one.

**Tradeoffs.** Matches the original design intent and removes the root
architectural duplication (two processes independently deriving and
racing to write the same row) rather than papering over its symptom. But
it is **not** the capability-neutral, purely-subtractive change the task
brief hoped it might be — §1.4 shows real coverage would be lost for
tracked trains whose live journey includes PASS events, off-catalogue
stations, or untranslatable STANOX values, unless `trust-backlog-consumer`
is separately widened to cover them first. That widening is scoped by a
different design decision (how far to widen a deliberately-scoped
catalogued-line filter that Decision 2 of
`2026-09-05-trust-event-backlog-design.md` chose on purpose) and isn't
free to make here.

### Option B — Add an explicit monotonicity guard to the shared write, once, at the point both paths already converge

Both writers already call the same `upsert_train_movement`
(`train_tracking.rs:506-558`) — the guard only needs to exist there, not
duplicated across two crates. Add a column that carries each event's own
real-world time (`COALESCE(actual_timestamp, planned_timestamp)` — every
message type that reaches this function, Movement and Cancellation alike,
carries one of these two; confirmed directly:
`crates/trust-backlog-consumer/src/process.rs:142-143,156-159` and
`crates/trust-consumer/src/process.rs`'s own event construction both
populate `actual_timestamp` for both message types). Change the
`ON CONFLICT DO UPDATE` to apply only when the incoming event's time is
`>=` what's already stored (or the existing row has none yet):

```sql
... ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET
    status = EXCLUDED.status, ...
    WHERE EXCLUDED.event_time >= train_current_state.event_time
       OR train_current_state.event_time IS NULL
```

Whichever process's write reaches Postgres first or last no longer
matters — the row always reflects the most recent real-world event it has
been shown, from either source.

**Tradeoffs.** Small, contained, single-function change — no crate's scope
or role changes, no coverage regression, nothing else in §1.4 needs
touching first. It is a real safety net against the exact symptom named by
the review (stale data winning). But it does not remove the underlying
duplication §1.3 describes: two processes keep independently folding
derived state from two different "previous" views, and a monotonicity
guard by timestamp doesn't guarantee the *content* the later-timestamped
writer computed is itself correct if its own fold history diverged from
what a single authoritative writer would have produced — it only
guarantees recency-ordering between the two, not that either one's
computation was right in isolation. It also leaves both consumers doing
real, overlapping work indefinitely (every tracked train's movements are
computed twice, by two different processes, forever) — a standing
inefficiency and a second bug surface, not just this one.

### Option C — Do B now, keep A as the named target state

Ship Option B first, as the correctness fix for the specific race the
review flagged, without gating it on `trust-backlog-consumer`'s scope
question. Separately and later, once a product/infra decision is made
about whether to widen `trust-backlog-consumer`'s catalogued-line scope
(a decision this document deliberately doesn't make, since it revisits
Decision 2 of a different, already-approved design), revisit Option A as
the intended end state — at which point the monotonicity guard from B
remains in place regardless (as defense-in-depth against the backlog
consumer's own internal out-of-order delivery, and against the one-shot
replay path in §1.4 racing live ingestion for the same `trains_id`), so
nothing done for B is thrown away if A is adopted later.

## 5. Recommendation — for a human decision-maker to approve, not a decision made here

**Recommend Option C: ship Option B's monotonicity guard now, and treat
Option A as the named, deliberately-deferred target state**, gated on a
separate decision about widening `trust-backlog-consumer`'s scope that
this document does not make.

Reasoning: Option B is small, safe, and directly closes the exact gap the
review named — a stale write winning over a fresher one — without
depending on any other crate's scope decision. Option A is the
architecturally "correct" end state and matches what the original spec
said would happen, but §1.4's verification shows it is not the
drop-in, capability-neutral change the task brief hoped for: shipping it
today, as stated, would silently regress live-status freshness for any
tracked train whose journey touches a PASS event, an off-catalogue
station, or an untranslatable STANOX — a real product regression, not a
paper one, and one a human should decide is acceptable (or first close)
rather than have it fall out of an "obviously safe" retirement.

This mirrors the same posture this repo has already taken on structurally
similar calls elsewhere. `2026-09-06-shared-train-identity-design.md` §2's
Step B backfill edge case recommends a specific handling of an accepted
gap but explicitly declines to treat that recommendation as final until
someone runs a named diagnostic query against real production data (its
own §7 Open Question 1); that same document's Open Questions section
follows the identical convention throughout. This document does the same:
propose the options, name the concrete tradeoff each one carries, and
require an explicit human sign-off before turning either into an
implementation plan — not treat "which option" as safe to pick
unilaterally.

**This recommendation is not an approval to implement either option.** It
is the input to the decision the prior review asked for.

## 6. Rough scope estimate (Option C, if approved)

At the granularity of a plan's task breakdown, not a full plan:

1. **Migration**: add `event_time TIMESTAMPTZ` (nullable, for the small
   window before backfill) to `train_current_state`; one-off backfill from
   existing `updated_at` (a reasonable approximation for pre-existing rows,
   since no other historical event-time signal exists for them).
2. **`upsert_train_movement`** (`train_tracking.rs:506-558`): compute
   `COALESCE(event.actual_timestamp, event.planned_timestamp)` for the
   incoming event, pass it as a new bound parameter, and add the
   `WHERE EXCLUDED.event_time >= train_current_state.event_time OR
   train_current_state.event_time IS NULL` guard to the existing `ON
   CONFLICT DO UPDATE`.
3. **Test coverage**: a real-Postgres test (mirroring this codebase's own
   `db_tests` convention already used throughout
   `train_tracking.rs`/`trust_event_backlog.rs`) asserting that an
   out-of-order call to `upsert_train_movement` (older event arriving
   after a newer one already wrote the row) does **not** regress
   `status`/`last_reported_location`/`delay_minutes`, alongside a
   same-shape test confirming the existing in-order case is unaffected.
4. **`trust_event_backlog.rs`'s `ingest_shared_movement`**: no code change
   expected — it already funnels through `upsert_train_movement`, so the
   guard applies to it automatically; only its own tests need a
   corroborating out-of-order case added.
5. **Documentation**: update `upsert_train_movement`'s own doc comment to
   describe the new guard and why it exists (this document, by reference),
   so a future reader doesn't mistake the `WHERE` clause for dead code.
6. **Follow-up, not part of this pass**: file the Option A / `crs_index`
   scope-widening question as its own named open decision, referencing
   this document, rather than letting it live only in this file's §4.

Estimated size: steps 1-5 are a small, single-PR change (one migration,
one function, a handful of tests) — comparable in scope to the
`schedule_matched_resolution` migration already in this codebase
(`crates/api/migrations/20260905150000_schedule_matched_resolution.sql`),
not a multi-task plan on the scale of the original 24-task shared-identity
implementation.

## 7. Non-goals

- **Widening `trust-backlog-consumer`'s catalogued-line scope.** Named in
  §1.4/§4/§5 as the real gate on Option A, not designed or decided here —
  it revisits Decision 2 of
  `2026-09-05-trust-event-backlog-design.md` and deserves its own
  sign-off.
- **`trust_event_backlog_match.rs`'s one-shot replay path.** Named in §1.4
  as a third, bounded writer; not solved here.
- **Redesigning `journey::apply_movement`'s derivation logic.** Confirmed
  correct in isolation (§3); the problem this document addresses is
  entirely about which writer's output wins, not how either one computes
  its output.
- **Any change to `notifier`'s escalation/cooldown logic, or the
  `notifier_forward_queue` mechanism itself.** Both are working as
  designed and unaffected by this race.

## 8. Open questions / risks

1. **Is `updated_at`-as-`event_time` backfill for existing rows good
   enough?** For rows written before this guard exists, `updated_at`
   reflects "when Postgres last wrote this," which is close to but not
   identical to "the real-world time of the last event applied." This is
   very likely fine in practice (rows are written promptly relative to the
   event they represent) but hasn't been measured against production data.
2. **How large is the §1.4 coverage gap in practice?** No measurement
   exists today of how often a tracked train's journey includes a PASS
   event, an off-catalogue station, or an untranslatable STANOX. This
   number would materially inform whether Option A's gate (widening
   `trust-backlog-consumer`) is worth prioritizing soon or can stay
   deferred indefinitely.
3. **Should the monotonicity guard apply per-field or whole-row?** This
   document proposes a single whole-row `event_time` gate (simplest,
   matches how `journey::apply_movement` already treats `DerivedState` as
   one atomic fold result) rather than per-column freshness. Worth a
   second look during implementation if any field is found to need
   independent staleness handling — none is known to today.

## References

- `docs/superpowers/specs/2026-09-06-shared-train-identity-design.md`
- `docs/superpowers/plans/2026-09-06-shared-train-identity-implementation-plan.md`
- `docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md`
- `crates/trust-consumer/src/{main,process}.rs`
- `crates/trust-backlog-consumer/src/{main,process,crs_index}.rs`
- `crates/api/src/routes/ingest.rs`
- `crates/api/src/data/{train_tracking,trust_event_backlog,trust_event_backlog_match,trains}.rs`
- `crates/trust-schema/src/journey.rs`
- `crates/common/src/lib.rs`
