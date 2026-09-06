# A Windowed TRUST-Event Backlog for Late-Tracking Pins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a tracked-train pin created *after* its train's live TRUST
window has already closed a way to learn what TRUST actually reported
happening to that train — real arrival/departure times, real delay, a real
cancellation — by retaining a short, windowed, catalogued-line-scoped copy
of TRUST's own observed events for every train, not just ones a user had
already pinned when those events arrived.

**Architecture:** A new Postgres table, `trust_event_backlog`, fed by a
new, independent Rust binary crate (`trust-backlog-consumer`) reading a
**third** named consumer group on the existing `movement-events` Redis
Stream (`movement-relay`'s own stream; `trust-consumer` and
`full-coverage-consumer` already read it as two independent groups — a
third is additive, not competing, confirmed against
`crates/movement-feed/src/redis_stream.rs`'s own group-creation/delivery
mechanics in this plan's own research). The new consumer filters to (a)
only Activation/Cancellation/Movement message types, (b) only
Movement rows whose `event_type` is `ARRIVAL`/`DEPARTURE` (never `PASS` —
this plan's own resolution of "what counts as a key journey point," see
below), and (c) only locations that translate to a CRS on a catalogued
line, using an **independently built** CRS reverse index — this plan does
**not** wait on or reuse the schedule-first design's own
`crs_to_line_ids` (that function lives privately inside `api`'s own
process memory, per that plan's still-unmerged Task 4; a separate binary
crate cannot reach it, and does not need to — see "Dependency on the
schedule-first plan," below). `api` gains a new private ingest route for
the consumer to POST batches to, a real, wired retention/prune job (added
to `aggregator`, mirroring its own existing `prune_history`/
`prune_daily_stats` pattern — not a repeat of `trust-consumer`'s dead
`retention_days` config field bug), and new backlog-consumption queries
that replay backfilled rows through the **already-existing**
`upsert_train_event` function, so `train_movement_events`/
`train_current_state`/`resolution_status` end up exactly where a
live-watching consumer would have left them.

**Tech Stack:** Rust (`sqlx` runtime-checked queries, `axum`, `tokio`,
`redis`), Postgres (a new migration), Helm/Docker for a new service
deployment.

**Spec:** `docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md`
(read in full; every Decision/Open Question below refers to that
document unless stated otherwise). Also required reading, referenced
throughout: `docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md`
(exists only on the still-unmerged `worktree-schedule-first-plan` branch
as of this writing — not yet on `main`; see "Dependency on the
schedule-first plan" below for exactly what that means for this plan).

## What counts as a "key journey point" — resolved, not guessed at

The repo owner's own instruction for this plan (not the original spec,
which left retention *tiers* open but did not narrow message scope this
far): *"this should only be storing data of times when trains got to key
journey points"* — i.e. NOT every raw TRUST message type, and NOT
fine-grained running/signal data.

Investigated directly against this codebase's own vocabulary, not
guessed:

1. **`crates/schedule-query/src/records.rs`'s `CallingPointKind`**
   (re-verified directly, real variants): `Origin` (`LO` — departure
   only), `Intermediate` (`LI` — both arrival and departure),
   `Terminate` (`LT` — arrival only). This is CIF's own, complete
   vocabulary for "a real booked calling point." There is no `Pass`
   variant anywhere in this enum, and no CIF schedule-body record type
   for a non-stopping passage — the schedule format only ever encodes
   stops.
2. **`crates/trust-schema/src/schema.rs`'s `Movement` struct** carries
   `event_type: String` with the confirmed real values `ARRIVAL |
   DEPARTURE | PASS` (the module's own doc comment). `PASS` is real and
   reachable: `crates/trust-consumer/src/process.rs`'s own tests
   (`a_pass_at_the_pinned_origin_does_not_claim_the_pin`) exercise it
   directly. TRUST reports a Movement at every timing point in a
   service's schedule, including ones the CIF schedule never books an
   arrival/departure at — those show up as `PASS`. A `PASS` event is,
   by construction, **not** a real calling point: CIF's own `LO`/`LI`/`LT`
   records only ever carry an arrival and/or a departure, never a bare
   pass, so a location TRUST reports a `PASS` at has no booked calling
   point in the schedule at all.
3. **Resolution**: "key journey points" = every Movement whose
   `event_type` is `ARRIVAL` or `DEPARTURE` — **every real scheduled
   stop, arrival and departure, including intermediate ones, not just
   origin/destination** — excluding every `PASS`. This is interpretation
   (a) from this task's own framing ("every real calling-point movement…
   including intermediate ones"), not the narrower (b): the repo owner's
   own phrasing ("times when trains got to key journey points," plural)
   and Decision 3's own late-tracking replay flow (which needs the
   **whole** observed journey, not just two points, to reconstruct
   `train_movement_events` the way a live consumer would have) both argue
   for the full-calling-point reading, not an origin/destination-only
   one.
   - **Filtering by `event_type` alone is used, not a live per-event JOIN
     against `schedule_line_population`.** This is the cheap, well-grounded
     implementation of the same idea: CIF's own vocabulary for "a real
     calling point" (`Origin`/`Intermediate`/`Terminate`) maps exactly onto
     "has an arrival and/or departure," which is exactly what `PASS`
     structurally lacks. A per-event JOIN against a whole line's whole-day
     `schedule_line_population` JSONB (to confirm each individual TIPLOC is
     *this specific service's* booked stop) would be strictly more precise
     but is the same "live JOIN into a large JSONB blob per event" cost the
     spec's own Decision 1 already rejected Redis Streams for at a different
     layer — at this consumer's expected event volume, adding it here would
     undermine the whole point of a cheap, high-volume-tolerant table.
     **Honest, named limitation, not glossed over**: a non-public "pathing
     point" TIPLOC that TRUST reports as `ARRIVAL`/`DEPARTURE` rather than
     `PASS` (if RDM's feed ever does this — not confirmed either way in this
     codebase's own research) would still be stored. This is the same
     "unmeasured, honest ceiling, not a guess dressed up as certainty"
     posture the design spec itself uses for Open Question 1.
   - **`Activation` (`0001`) and `Cancellation` (`0002`) are kept**, even
     though neither carries calling-point timing data itself: Activation is
     load-bearing plumbing (it's the only source of `train_uid`, without
     which the CRS+time backlog lookup in Decision 3 step 2 could never
     resolve a `train_uid` at all), and Cancellation is itself a real,
     timestamped thing that happened to the journey (the point it stopped
     happening) — arguably the most "key" journey point of all for a user
     who tracks a train that never shows up. Both are genuinely narrower
     than the design spec's own Decision 2 sketch, which kept all five
     confirmed types including `ChangeOfOrigin`/`ChangeOfIdentity`.
   - **`ChangeOfOrigin` (`0006`) and `ChangeOfIdentity` (`0007`) are
     dropped**, a deliberate divergence from the spec's Decision 2 (written
     before this session's own, more specific "key journey points" scope
     instruction). `crates/trust-schema/src/schema.rs`'s own
     `ChangeOfOrigin`/`ChangeOfIdentity` structs carry nothing but a bare
     `train_id` — no location, no timestamp, no journey-point data of any
     kind. They are not "key journey point" timing data by the repo owner's
     own stated scope for this table, so they are filtered out before
     insert and never stored here.

**Effect on the spec's own storage math**: the spec's Decision 4 ceiling
(week tier: ~880MB / ~4.4M rows) was computed for **all five** confirmed
message types at full national-feed volume, no `PASS` filtering. This
plan's narrower scope (three message types, `PASS` excluded) reduces the
real row count below that ceiling by an amount this plan does **not**
invent a number for — same honest-ceiling posture as the spec's own Open
Question 1 (catalogued-line scoping fraction), now compounded by a second,
also-unmeasured reduction. **This plan keeps the spec's own ~880MB/~4.4M-row
week-tier figures as its provisioning ceiling** (a safe, conservative
number to size Postgres against), while stating plainly that the real
cost is very likely smaller, by an amount neither this plan nor the spec
measures.

## Scope decision: retention tier and the licensing safeguard

The repo owner explicitly resolved the spec's own retention-tier open
question in favor of the **week** tier (~880MB / ~4.4M rows at the spec's
national-feed ceiling) for this feature's real target. The spec's own
Decision 5 flags that tier as blocked on a **secondhand, imprecisely
sourced** citation (`docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`'s
"TRUST's own licence is unrestricted per the audit" — no schedule/clause
quoted, source PDFs already deleted) — genuinely favorable evidence, but
weaker than the LDBWS citation it sits beside. **This plan does not
silently resolve that open question on the repo owner's behalf.** Per
this task's own explicit instruction, it is carried forward as a real,
loud, pre-launch safeguard:

- **Default retention is 1 day, everywhere this plan touches config.**
  The 1-day tier is uncontroversial under either reading of the licensing
  evidence (even LDBWS's own strict 1-year cap wouldn't touch it).
- **Reaching 7 days requires an explicit config change** — a distinct,
  clearly-labeled `--trust-event-backlog-retention-days` value above the
  shipped default, not a code change and not a silent default bump.
- **A loud, unmissable runtime warning fires on every process start**
  where the configured value exceeds 1 day (Task 6), so the safeguard
  lives somewhere a deployer actually sees it (structured logs, at every
  restart), not only in a comment nobody re-reads.
- **This plan's own Global Constraints (below) state in plain language**
  that a human must confirm TRUST's real licence terms before that
  config change is ever applied in a real production deployment. No task
  in this plan performs that confirmation — it is out of scope for a
  Rust/SQL implementation plan, same as the spec's own Non-goals section
  already states.

## Dependency on the schedule-first plan — resolved: **no hard dependency**

The task brief for this plan asks whether the new consumer's CRS
scoping should reuse the schedule-first design's own `crs -> Vec<line_id>`
reverse index, and whether this plan needs that plan to land first. Read
directly (`docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md`
Task 4, on the still-unmerged `worktree-schedule-first-plan` branch — this
plan is not on `main` as of this writing):

- `schedule_query::crs_to_line_ids` (that plan's own name for the
  function) is a **pure, in-memory re-keying of `AppState.config.lines`**,
  computed once inside `api`'s own `AppState::init` and stored as a
  private field (`AppState.schedule_crs_line_index`) on `api`'s own
  running process. It is never exposed over HTTP, never persisted, and
  is architecturally **unreachable** from a separate binary crate like
  the one this plan adds — there is no route, no shared crate export, no
  way for `trust-backlog-consumer` to "reuse" it even if
  `worktree-schedule-first-plan` were already merged.
- The **inputs** that function operates on (`common::LineDefinition`,
  loaded from the same static `lines/*.toml` catalogue every service
  already independently parses via `--lines-dir`/`LINES_DIR`) are
  already available, today, to any new crate that wants them —
  `crates/full-coverage-consumer/src/population.rs`'s own
  `build_tiploc_index` is the **existing, real, already-shipped**
  precedent for exactly this: a small, pure, crate-local function
  re-keying the same static catalogue into a lookup index, built
  independently by each consumer that needs one (nothing in this
  codebase shares that logic across `full-coverage-consumer`/`aggregator`/
  `api` today, even though the underlying predicate — "does this line
  have at least one TIPLOC-bearing station" — is structurally identical
  in `schedule-reference::lines_to_publish`,
  `full-coverage-consumer::config::shadow_line_ids`, and (once merged)
  `api::data::schedule_matching::crs_to_line_ids`).
- **Conclusion**: this plan builds its **own**, independent CRS reverse
  index (Task 8) directly from the same static `lines/*.toml` catalogue,
  using the same "line has ≥1 TIPLOC-bearing station" predicate every one
  of those three existing/pending implementations already uses. This is
  not a workaround or a stopgap — it is this codebase's own established,
  already-shipped pattern for this exact kind of thing, and it means
  **this plan has zero ordering dependency on
  `worktree-schedule-first-plan` landing first.** It can be implemented,
  reviewed, and merged independently, in either order.
- **The one place a soft (non-blocking) coupling exists**: Decision 3
  step 6 of the design spec notes that a backfilled Movement with no
  captured Activation can only reach `resolution_status = 'schedule_matched'`
  today (not `'resolved'`), because `upsert_train_event`'s existing
  two-field guard (`train_tracking.rs:400-412`) requires both
  `resolved_train_uid` **and** `resolved_train_id` together. Relaxing
  that guard is the schedule-first design's own Decision 5, planned as
  that plan's own Task 9. **This plan does not touch that guard** — it
  is out of scope here (see Non-goals) — so until/unless that companion
  fix lands, a backlog-only Movement history with no Activation in the
  same retention window will leave a pin at whatever status it already
  had (`'pending'` today, `'schedule_matched'` once the other plan
  lands), never advancing to `'resolved'` purely from Movement rows.
  This is a **named, accepted, non-blocking gap**, identical in kind to
  the live-matching gap this codebase already has and ships with today
  (`process.rs`'s own module doc: "A tracked train can stay
  `resolution_status = 'pending'` in the database forever even while this
  process tracks it correctly") — not a new failure mode this plan
  introduces.

## Global Constraints

- **Retention defaults to 1 day everywhere.** No task in this plan ships
  a default retention value above 1 day for `trust_event_backlog`.
  Reaching 7 (or any value above 1) requires an explicit, separately
  documented config change at deploy time.
- **A human must confirm TRUST's real Train Movements licence terms
  directly with RDM before that config change is ever applied to a real
  production deployment.** This plan does not perform that confirmation
  and does not claim to. This sentence is not decorative — repeat it
  verbatim in `trust-backlog-consumer`'s own `--retention-days` CLI help
  text (Task 7) and in the migration's own header comment (Task 1).
- **Only real calling-point timing data is stored**: `msg_type IN
  ('0001', '0002', '0003')`, and for `0003` (Movement) rows,
  `event_type IN ('ARRIVAL', 'DEPARTURE')` only — `PASS` is excluded at
  the database level via a `CHECK` constraint, not just filtered in
  application code, so a future write path cannot silently reintroduce
  it. `ChangeOfOrigin`/`ChangeOfIdentity` (`0006`/`0007`) are never
  written to this table.
- **No raw TRUST payload is ever stored.** No `raw_body` column, no
  `serde_json::Value` blob of any kind, on `trust_event_backlog`. This
  mirrors the design spec's own Decision 2 and is load-bearing for the
  storage-cost math this plan inherits.
- **Never data for lines outside this app's own catalogue.** The new
  consumer drops any Movement whose translated CRS does not appear in
  its own independently-built CRS reverse index (Task 8) before it ever
  reaches the batch POSTed to `api` — filtering happens at the producer,
  not the ingest route (mirrors every other ingest route in this
  codebase, which trusts its one known caller's own scoping rather than
  re-validating it server-side).
- **The new consumer group must never compete with or duplicate-process
  entries `trust-consumer`/`full-coverage-consumer` already read.** Redis
  Streams consumer groups are independent by construction
  (`XREADGROUP`/`XACK` are scoped per group) — Task 9's own verification
  step proves this directly against a real Redis instance, not by
  assertion.
- **Every SQL write in this plan uses runtime-checked `sqlx::query`/
  `query_as`** (never the `query!` macro family), matching
  `crates/api/src/data/queries.rs`'s own module-doc convention.
- **`api` gaining a `trust_schema` dependency (Task 5) is a deliberate,
  named boundary crossing**, argued explicitly in that task, mirroring
  the schedule-first design's own precedent for `api` gaining a
  `schedule_query` dependency (that plan's Decision 3) — not something
  done quietly as an incidental side effect.
- **`upsert_train_event`'s existing two-field guard is not modified by
  this plan.** Any interaction with that guard's known limitation is
  named (see "Dependency on the schedule-first plan," above) but left
  for that other, already-scoped fix.
- **`trust-consumer`'s existing dead `retention_days` config field is not
  touched by this plan.** Named because this plan's own research
  reconfirmed it (same finding the design spec already surfaced), not
  because fixing it is this plan's job — an independent, separately
  pick-up-able cleanup.

## Explicitly out of scope (Non-goals)

- **Resolving the TRUST/RDM Train Movements licensing question with
  certainty.** Carried forward as a real pre-launch checklist item (see
  above), not resolved here.
- **Relaxing `upsert_train_event`'s two-field guard.** That is the
  schedule-first design's own Decision 5 / that plan's own Task 9. This
  plan names the interaction (above) but does not implement it.
- **Reusing or waiting on `worktree-schedule-first-plan`'s own
  `crs_to_line_ids`.** Resolved above: this plan builds its own
  equivalent, independently, with zero ordering dependency.
- **Designing or implementing the MCP integration design's own Phase 3b**
  (`full_coverage_train_state`) — positioned relative to it in the design
  spec's Decision 6, not built here.
- **A `MAXLEN` bump on `movement-events`** as an interim step — legitimate
  on its own merits per the spec's Decision 1, not part of this plan.
- **Any downsampled/coarser long-term summary shape** for month/year-tier
  retention (the spec's own Open Question 5) — out of scope; this plan
  only implements the day/week tiers with a single full-fidelity table.
- **A new public-facing API route for a human/frontend to query the
  backlog directly.** This is backend plumbing only: Task 4's ingest
  route is private/internal-oauth-gated (`trust-backlog-consumer`'s own
  writes), and Task 5's consumption logic is called internally from
  `routes::train::post_track`, not exposed as its own route. No task in
  this plan adds a public route over `trust_event_backlog` itself.
  (An earlier draft of this section pointed at a since-removed "API
  surface" section below that no longer exists in this document — fixed
  during this plan's second review pass; this bullet is now
  self-contained.)
- **Deduplicating the now-three near-identical
  CRS/TIPLOC-reverse-index-building functions** (`full-coverage-consumer`'s
  `build_tiploc_index`, `api`'s pending `crs_to_line_ids`, and this plan's
  own new one) into a shared `common` helper. Named as a real, visible
  repeat this plan's own research surfaced, worth a future cleanup, but
  not this plan's job — matches this codebase's own existing, already-
  accepted posture on this exact kind of small duplication.

---

## Task 1: Migration — `trust_event_backlog` table

**Files:**
- Create: `crates/api/migrations/20260905160000_trust_event_backlog.sql`

**Interfaces:**
- Produces: the `trust_event_backlog` table and its three indexes,
  consumed by Task 4 (writes) and Task 5 (reads).

- [ ] **Step 1: Write the migration**

```sql
-- ---------------------------------------------------------------------
-- A windowed backlog of real TRUST calling-point events, for
-- late-tracking pins (docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md,
-- docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md).
--
-- RETENTION SAFETY, STATED HERE BECAUSE THIS IS WHERE A FUTURE READER
-- WILL LOOK: this table's default retention is 1 DAY
-- (crates/aggregator's own `trust_event_backlog_retention_days` config
-- field, Task 6). A human must confirm TRUST's real Train Movements
-- licence terms directly with RDM before that value is ever configured
-- above 1 in a real production deployment -- see the design spec's
-- Decision 5 and this plan's own "Scope decision: retention tier and
-- the licensing safeguard" section. Do not bump the default here, or in
-- any Helm values.yaml default, without that confirmation happening
-- first.
--
-- Deliberately narrower message-type/event-type coverage than
-- train_movement_events (see the plan's own "What counts as a key
-- journey point" section): only Activation/Cancellation/Movement,
-- and Movement is further restricted to ARRIVAL/DEPARTURE (never PASS)
-- -- real calling points only, not every TRUST reporting point.
--
-- No raw_body column, unlike train_movement_events -- a deliberate,
-- named scope-narrowing tradeoff (see the design spec's Decision 2 and
-- this plan's Global Constraints), not an oversight. Do not "fix" this
-- by adding one back without re-running this plan's own storage-cost
-- math against the much bigger row size that would produce.
-- ---------------------------------------------------------------------

CREATE TABLE trust_event_backlog (
    id                 BIGSERIAL PRIMARY KEY,

    -- Best-effort STANOX->CRS translation, same posture as
    -- train_movement_events.loc_crs. NULL for Activation/Cancellation
    -- rows (neither carries a location at all) and for any Movement row
    -- whose STANOX didn't translate (dropped before insert by the
    -- consumer in that case -- see Task 9 -- since an untranslated
    -- Movement is useless for the CRS+time lookup this column exists
    -- to serve).
    crs                TEXT,

    -- NULL until an Activation for this train_id has been observed by
    -- the consumer (may never arrive within the retention window -- see
    -- Task 9's own note on this being an accepted, pre-existing category
    -- of gap, not a new one).
    train_uid          TEXT,

    -- TRUST's own daily identifier -- present on all three kept message
    -- types.
    train_id           TEXT NOT NULL,

    -- Best-effort service date. Sourced from an Activation's own
    -- schedule_start_date when this consumer observed one in-process for
    -- this train_id; falls back to the current Europe/London rail day
    -- otherwise (see Task 9's own note -- an accepted approximation
    -- identical in kind to trust-consumer's own existing Activation-
    -- binding gap, not a new one).
    service_date       DATE NOT NULL,

    -- Activation / Cancellation / Movement only. ChangeOfOrigin/
    -- ChangeOfIdentity carry no location or timing data at all
    -- (trust_schema::schema's own ChangeOfOrigin/ChangeOfIdentity
    -- structs are bare {train_id}), so they are not "key journey point"
    -- data by this plan's own resolution of that question and are never
    -- written here.
    msg_type           TEXT NOT NULL
        CHECK (msg_type IN ('0001', '0002', '0003')),

    -- Movement only. PASS is excluded at the database level, not just
    -- application level: a PASS event is TRUST reporting a train
    -- running through a location with no booked calling point at all
    -- (CIF's own LO/LI/LT records only ever carry an arrival and/or a
    -- departure, never a bare pass) -- see the plan's own "What counts
    -- as a key journey point" section for the full reasoning.
    event_type         TEXT
        CHECK (event_type IS NULL OR event_type IN ('ARRIVAL', 'DEPARTURE')),

    planned_timestamp  TIMESTAMPTZ,
    actual_timestamp   TIMESTAMPTZ,

    -- Raw TRUST field, needed to recompute delay_minutes identically to
    -- trust_schema::journey's own "LATE" gate when a backfilled row is
    -- replayed through upsert_train_event (Task 5).
    variation_status   TEXT,

    -- Denormalized convenience for a direct query of this table itself
    -- (e.g. future debugging/analytics) -- NOT load-bearing for the
    -- replay path in Task 5, which recomputes this itself from
    -- planned_timestamp/actual_timestamp/variation_status via the same
    -- trust_schema::journey logic a live event already uses, so this
    -- column and the replayed value are expected to agree but are never
    -- cross-checked against each other.
    delay_minutes      INTEGER,

    -- trust_schema::dedup::dedup_key(train_id, msg_type, event_type,
    -- loc_stanox, planned_timestamp) -- identical shape to
    -- train_movement_events.dedup_key, making a blind, at-least-once-safe
    -- INSERT ... ON CONFLICT DO NOTHING correct here too.
    dedup_key          TEXT NOT NULL,

    received_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX trust_event_backlog_dedup_key
    ON trust_event_backlog (dedup_key);

-- Decision 3 step 2's own query: "does any backlog row at this CRS,
-- around this scheduled time, exist" -- the CRS+time lookup a
-- late-tracking pin uses to discover a train_uid.
CREATE INDEX trust_event_backlog_crs_time
    ON trust_event_backlog (crs, planned_timestamp)
    WHERE crs IS NOT NULL;

-- Decision 3 step 3's own query: "the entire observed history for this
-- train" -- the full backfill. Keyed on train_id, NOT train_uid: Task 9's
-- own consumer never writes a train_uid onto a Movement/Cancellation row
-- (only an Activation row ever carries one -- see that task's own "this
-- consumer doesn't correlate Activation->Movement in-process" comment).
-- train_id, by contrast, is NOT NULL on every one of the three kept
-- message types, and it's the only column that actually ties a train's
-- Activation/Movement/Cancellation rows together in this table -- so it,
-- not train_uid, is the real backfill key. (An earlier draft of this
-- migration indexed (train_uid, service_date) here; that would have made
-- the Task 5 backfill query only ever retrieve the Activation row itself,
-- never the Movement/Cancellation history the whole feature exists to
-- replay -- caught and fixed during this plan's second review pass.)
CREATE INDEX trust_event_backlog_train
    ON trust_event_backlog (train_id, service_date);
```

- [ ] **Step 2: Run the migration against a local database and verify**

```bash
cd crates/api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres sqlx migrate run
psql "$DATABASE_URL" -c "\d trust_event_backlog"
```

Expected: the table exists with all 13 columns, the two `CHECK`
constraints on `msg_type`/`event_type` show in the printed output, and
all three indexes (`trust_event_backlog_dedup_key`,
`trust_event_backlog_crs_time`, `trust_event_backlog_train`) are listed.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260905160000_trust_event_backlog.sql
git commit -m "Add the trust_event_backlog table for late-tracking pin resolution"
```

---

## Task 2: `common` — the consumer-to-`api` wire message type

**Files:**
- Modify: `crates/common/src/lib.rs`

**Interfaces:**
- Produces: `pub struct TrustBacklogEventMessage`, the POST body shape
  `trust-backlog-consumer` (Task 9) sends and `api`'s new ingest route
  (Task 4) deserializes.

- [ ] **Step 1: Add the struct**

Add near `TrainMovementEventMessage` (same file), following its own
`#[serde(default, skip_serializing_if = "Option::is_none")]` convention
for every optional field:

```rust
/// One row's worth of data for `trust_event_backlog`
/// (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md) --
/// the wire shape `trust-backlog-consumer` POSTs in batches to `api`'s
/// `/private/trust-event-backlog` route. Deliberately NOT
/// `TrainMovementEventMessage`: that type carries `tracked_train_id`/
/// `resolved_train_uid`/`resolved_train_id`/derived-current-state
/// fields this table has no equivalent of (it isn't scoped to any one
/// pin), and this type carries `crs`/`service_date` fields
/// `TrainMovementEventMessage` has no use for. Sharing one struct
/// between two genuinely different wire shapes would mean every field
/// on it is `Option`-everything and meaningless for whichever message
/// kind doesn't use it -- two distinct types are clearer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustBacklogEventMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub train_uid: Option<String>,
    pub train_id: String,
    pub service_date: NaiveDate,
    pub msg_type: String, // "0001" | "0002" | "0003" only, see the plan's Global Constraints
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>, // "ARRIVAL" | "DEPARTURE" only, Movement rows
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_minutes: Option<i32>,
    pub dedup_key: String,
}
```

**Confirmed directly, not guessed: `NaiveDate` is NOT yet in this file's
top-level `chrono` import.** `crates/common/src/lib.rs`'s own `use
chrono::{DateTime, Utc};` line (confirm with `grep -n "^use chrono"
crates/common/src/lib.rs`) does not include `NaiveDate`, even though the
type itself is already used elsewhere in this file via its fully
qualified path (`TrackPinRequest.service_date: chrono::NaiveDate`). Add
`NaiveDate` to that `use` line (`use chrono::{DateTime, NaiveDate,
Utc};`) rather than writing `chrono::NaiveDate` inline on the new struct,
for consistency with `TrainMovementEventMessage`'s neighboring fields
(`DateTime<Utc>`, not `chrono::DateTime<Utc>`).

- [ ] **Step 2: Build**

```bash
cargo build -p common
```

Expected: compiles clean.

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "Add TrustBacklogEventMessage, the trust-backlog-consumer to api wire type"
```

---

## Task 3: `api` — internal OAuth group + private route registration

**Files:**
- Modify: `crates/api/src/data/config.rs` (or wherever
  `ServiceArguments`'s `internal_oauth_group_*` fields live — confirm
  with `grep -n "internal_oauth_group_full_coverage" crates/api/src/data/config.rs`)
- Modify: `crates/api/src/app.rs`

**Interfaces:**
- Produces: `config.internal_oauth_group_trust_backlog: String`, and a
  new `("/trust-event-backlog", Method::POST, vec![...])` entry in
  `AppState`'s internal-oauth route table, consumed by Task 4's route.

- [ ] **Step 1: Add the config field**

In `ServiceArguments` (wherever `internal_oauth_group_full_coverage` is
declared), add, following that field's own doc-comment shape:

```rust
    /// Authentik group required to call `/private/trust-event-backlog`.
    /// `trust-backlog-consumer`'s own service-account group.
    #[arg(long, env, default_value = "svc-trust-backlog-consumer")]
    pub internal_oauth_group_trust_backlog: String,
```

- [ ] **Step 2: Register the route**

In `crates/api/src/app.rs`, add to the same `Vec<(&str, Method,
Vec<String>)>` literal `internal_oauth_routes` (or whatever the
function/const is named — confirmed above at the `/train-events` entry),
directly below the `/train-events` entry, following its own "POST-only:
no GET handler for this path" comment convention:

```rust
        // POST-only: trust-backlog-consumer's per-cycle event batch --
        // ingest::router() never wires a GET handler for this path,
        // mirroring /train-events exactly (see that entry's own comment).
        (
            "/trust-event-backlog",
            Method::POST,
            vec![config.internal_oauth_group_trust_backlog.clone()],
        ),
```

- [ ] **Step 3: Add the ServiceTokenVerifier startup guard**

In `AppState::init`, alongside the existing
`"internal_oauth_group_trust_consumer"`/`"internal_oauth_group_full_coverage"`
empty-value guards (`app.rs:351-364` region), add the same guard for the
new field:

```rust
                "internal_oauth_group_trust_backlog",
                &config.internal_oauth_group_trust_backlog,
```

(Confirm the exact surrounding call shape with
`grep -n "internal_oauth_group_full_coverage" -B3 -A3 crates/api/src/app.rs`
before editing — it's a call to some `ensure_group_configured`-style
helper or an inline `ensure!`; match its existing signature exactly.)

- [ ] **Step 4: Update every hand-built `AppState`/`ServiceArguments`
      test fixture**

```bash
grep -rln "internal_oauth_group_full_coverage" crates/api/src/
```

(NOT `crates/api/src/**/*.rs` — without `shopt -s globstar`, bash's
default `**` behaves like a single `*` and does not match `/`, so that
glob silently misses any file directly inside `src/` itself, one
directory level up from where the glob can reach. Confirmed directly by
running both forms against this repo during this plan's second review
pass: the `**/*.rs` form misses `crates/api/src/auth.rs:622` — itself a
hand-built `ServiceArguments` literal this step needs to update — while
the plain-directory form above finds it along with every other match.
`crates/api/src/app.rs` and `crates/api/src/data/config.rs` also match
this grep but are not fixtures to edit here — `app.rs` only reads
`config.internal_oauth_group_full_coverage`, and `config.rs` is the
field's own declaration, already handled by Step 1; skip both. A missed
fixture is still caught by Step 5's `cargo build` as a missing-struct-
field compile error, not a silent bug, but there is no reason to leave an
engineer to debug that when the fix is a simpler, correct command.)

For each match that constructs a literal `ServiceArguments { ... }` (test
fixtures, not `ServiceArguments::parse()` call sites), add:

```rust
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
```

- [ ] **Step 5: Build and test**

```bash
cargo build -p api
cargo test -p api --lib
```

Expected: compiles clean, existing non-DB-gated test suite still passes
(this task adds no new tests of its own — it's pure wiring, exercised
indirectly by Task 4's own route test).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs
git commit -m "Wire the trust-backlog-consumer OAuth group and /private/trust-event-backlog route entry"
```

---

## Task 4: `api` — ingest route + storage query

**Files:**
- Create: `crates/api/src/data/trust_event_backlog.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/routes/ingest.rs`

**Interfaces:**
- Consumes: `common::TrustBacklogEventMessage` (Task 2).
- Produces: `pub async fn upsert_trust_event_backlog_batch(pool,
  events: &[TrustBacklogEventMessage]) -> anyhow::Result<u64>` (rows
  actually inserted, dedup-aware), mounted at `POST
  /private/trust-event-backlog`.

- [ ] **Step 1: Write the storage module**

```rust
// crates/api/src/data/trust_event_backlog.rs
//! Storage for `trust_event_backlog`
//! (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md). Write
//! side only -- Task 5 (`schedule_matching.rs` or a new sibling module)
//! owns the read/consumption side.

use common::TrustBacklogEventMessage;
use sqlx::PgPool;

/// Blind, at-least-once-safe batch insert -- `ON CONFLICT DO NOTHING` on
/// `dedup_key` (the same posture `train_movement_events` already uses for
/// the same reason: Redis Streams' own at-least-once delivery means a
/// redelivered batch after a crash-before-XACK is expected, not
/// exceptional). Returns how many rows this call actually inserted (for
/// the caller's own logging), not the batch length -- a redelivered batch
/// legitimately inserts 0.
pub async fn upsert_trust_event_backlog_batch(
    pool: &PgPool,
    events: &[TrustBacklogEventMessage],
) -> anyhow::Result<u64> {
    let mut inserted = 0u64;
    let mut tx = pool.begin().await?;
    for event in events {
        let result = sqlx::query(
            "INSERT INTO trust_event_backlog \
                (crs, train_uid, train_id, service_date, msg_type, event_type, \
                 planned_timestamp, actual_timestamp, variation_status, delay_minutes, dedup_key) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (dedup_key) DO NOTHING",
        )
        .bind(&event.crs)
        .bind(&event.train_uid)
        .bind(&event.train_id)
        .bind(event.service_date)
        .bind(&event.msg_type)
        .bind(&event.event_type)
        .bind(event.planned_timestamp)
        .bind(event.actual_timestamp)
        .bind(&event.variation_status)
        .bind(event.delay_minutes)
        .bind(&event.dedup_key)
        .execute(&mut *tx)
        .await?;
        inserted += result.rows_affected();
    }
    tx.commit().await?;
    Ok(inserted)
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    fn fixture_event(train_id: &str, dedup_key: &str) -> TrustBacklogEventMessage {
        TrustBacklogEventMessage {
            crs: Some("EUS".to_string()),
            train_uid: Some("C11052".to_string()),
            train_id: train_id.to_string(),
            service_date: "2026-09-05".parse().unwrap(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            planned_timestamp: Some("2026-09-05T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-05T19:16:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            delay_minutes: Some(1),
            dedup_key: dedup_key.to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                upsert_trust_event_backlog_batch -- --ignored`"]
    async fn a_fresh_batch_inserts_every_row() {
        let pool = connect().await;
        let events = vec![
            fixture_event("TEST-TRUST-BACKLOG-1", "test-dedup-key-1"),
            fixture_event("TEST-TRUST-BACKLOG-2", "test-dedup-key-2"),
        ];

        let inserted = upsert_trust_event_backlog_batch(&pool, &events)
            .await
            .expect("insert");
        assert_eq!(inserted, 2);

        sqlx::query("DELETE FROM trust_event_backlog WHERE dedup_key LIKE 'test-dedup-key-%'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_redelivered_batch_inserts_nothing_twice -- --ignored`"]
    async fn a_redelivered_batch_inserts_nothing_twice() {
        let pool = connect().await;
        let event = fixture_event("TEST-TRUST-BACKLOG-3", "test-dedup-key-3");

        let first = upsert_trust_event_backlog_batch(&pool, &[event.clone()])
            .await
            .expect("first insert");
        assert_eq!(first, 1);

        let redelivered = upsert_trust_event_backlog_batch(&pool, &[event])
            .await
            .expect("redelivered insert");
        assert_eq!(redelivered, 0, "same dedup_key must not insert twice");

        sqlx::query("DELETE FROM trust_event_backlog WHERE dedup_key = 'test-dedup-key-3'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/api/src/data/mod.rs`, add (alphabetically):

```rust
pub mod trust_event_backlog;
```

- [ ] **Step 3: Add the route**

In `crates/api/src/routes/ingest.rs`, add to `router()`:

```rust
        .route(
            "/trust-event-backlog",
            axum::routing::post(post_trust_event_backlog),
        )
```

And the handler, alongside `post_train_events`:

```rust
async fn post_trust_event_backlog(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrustBacklogEventMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let inserted = crate::data::trust_event_backlog::upsert_trust_event_backlog_batch(
        &app.database,
        &events,
    )
    .await
    .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted: inserted }))
}
```

(`UpsertResponse`/`internal_error` are already defined in this file for
`post_train_events` — reuse them, don't redefine.)

- [ ] **Step 4: Run the DB-gated tests**

```bash
cd crates/api && sqlx migrate run
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api trust_event_backlog -- --ignored --test-threads=1
```

Expected: both new tests pass.

- [ ] **Step 5: Build the whole crate**

```bash
cargo build -p api
cargo test -p api --lib
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/trust_event_backlog.rs crates/api/src/data/mod.rs crates/api/src/routes/ingest.rs
git commit -m "Add POST /private/trust-event-backlog ingest route and dedup-safe batch upsert"
```

---

## Task 5: `api` — backlog consumption (Decision 3 steps 2-4), independent of schedule-first

**Files:**
- Create: `crates/api/src/data/trust_event_backlog_match.rs`
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/routes/train.rs` (call site at pin creation)

**Interfaces:**
- Consumes: `trust_schema::journey::{apply_movement, apply_cancellation,
  DerivedState}`, `trust_schema::schema::Movement`,
  `crate::data::train_tracking::upsert_train_event`,
  `common::{MATCH_TOLERANCE, TrainMovementEventMessage}` (note:
  `common::MATCH_TOLERANCE` only exists once
  `worktree-schedule-first-plan`'s own Task 3 lands; until then, define a
  local `const MATCH_TOLERANCE: chrono::Duration =
  chrono::Duration::minutes(20);` in this new module, with a comment
  pointing at the eventual hoist — see this task's own Step 1 note).
- Produces: `pub async fn attempt_backlog_match(pool, tracked_train_id,
  pin_origin_crs, pin_scheduled_departure, service_date) ->
  anyhow::Result<bool>`, called from `routes::train::post_track` (pin
  creation) and available for a future periodic sweep (not wired to one
  in this plan — see this task's own note on why).

**Important boundary-crossing note, stated plainly per this plan's
Global Constraints**: this task adds `trust-schema` as a new dependency
of `api`. `api` has never depended on `trust-schema` before this plan —
confirmed directly (`grep -n "trust-schema" crates/api/Cargo.toml`
returns nothing on `main` as of this writing). This mirrors the
schedule-first design's own precedent for `api` gaining a
`schedule_query` dependency (that design's Decision 3): a deliberate,
reviewed crossing, done because reusing `trust_schema::journey`'s
already-tested derivation logic is strictly better than re-deriving
`DerivedState` a second, parallel way inside `api` that could drift from
what a live TRUST event actually produces.

- [ ] **Step 1: Add `trust-schema` as an `api` dependency**

In `crates/api/Cargo.toml`, add (alphabetically):

```toml
trust-schema = { path = "../trust-schema" }
```

- [ ] **Step 2: Write the backlog-match module**

```rust
// crates/api/src/data/trust_event_backlog_match.rs
//! Backlog-consumption side of `trust_event_backlog`
//! (docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md
//! Decision 3, docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md
//! Task 5). Walks Decision 3 steps 2-4 exactly:
//!
//! 1. CRS+time lookup against `trust_event_backlog` to discover a
//!    `train_id` (TRUST's own daily identifier) for a pin whose live
//!    TRUST window has already closed, plus a `train_uid` (CIF's own
//!    identifier) if an Activation for that `train_id` is also in the
//!    backlog.
//! 2. Full backfill: every backlog row for that `train_id`+`service_date`,
//!    in `received_at` order. Keyed on `train_id`, NOT `train_uid` --
//!    see `fetch_backlog_history`'s own doc comment for why (a real bug
//!    caught in this plan's second review pass: `train_uid` is only ever
//!    non-NULL on an Activation row in this table, never on a Movement/
//!    Cancellation row, so a `train_uid`-keyed backfill query would only
//!    ever retrieve the Activation row itself and silently miss every
//!    Movement/Cancellation event this feature exists to replay).
//! 3. Replay each row through the SAME `train_tracking::upsert_train_event`
//!    path a live event would have taken, so `train_movement_events`/
//!    `train_current_state`/`resolution_status` end up exactly where a
//!    live-watching trust-consumer would have left them.
//!
//! `MATCH_TOLERANCE` is a local constant, not `common::MATCH_TOLERANCE`:
//! that hoist is `docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md`'s
//! own Task 3, on a still-unmerged branch as of this writing. This
//! module defines its own copy with the SAME VALUE (20 minutes,
//! matching `trust-consumer::matching::resolve_origin_departure`'s
//! existing constant) rather than blocking on that other plan landing
//! first -- see this plan's own "Dependency on the schedule-first plan"
//! section. Whoever lands second should collapse these into one
//! `common::MATCH_TOLERANCE` definition; this module's own doc comment
//! flags it so that isn't missed.

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use trust_schema::journey::{self, DerivedState};
use trust_schema::schema::Movement;

use crate::data::train_tracking;

/// See this module's own doc comment on why this isn't `common::MATCH_TOLERANCE` yet.
const MATCH_TOLERANCE: chrono::Duration = chrono::Duration::minutes(20);

#[derive(Debug, Clone, sqlx::FromRow)]
struct BacklogRow {
    train_uid: Option<String>,
    train_id: String,
    msg_type: String,
    event_type: Option<String>,
    // The already-translated CRS a Movement row was observed at (`None`
    // for Activation/Cancellation, which carry no location at all -- see
    // Task 1's migration). MUST be threaded through to `apply_movement`'s
    // `loc_crs` param and the replayed event's own `loc_crs` field below --
    // an earlier draft of this function didn't select this column at all
    // and passed `None` unconditionally, silently discarding a value the
    // table actually stores. That would have left every backfilled pin's
    // `train_current_state.last_reported_location` permanently `NULL`
    // even though the real CRS was sitting right there in
    // `trust_event_backlog.crs` -- caught during this plan's second
    // review pass.
    crs: Option<String>,
    planned_timestamp: Option<DateTime<Utc>>,
    actual_timestamp: Option<DateTime<Utc>>,
    variation_status: Option<String>,
}

/// Decision 3 step 2: does any backlog row at `pin_origin_crs`, within
/// `MATCH_TOLERANCE` of `pin_scheduled_departure`, exist? Returns that
/// row's `train_id` (TRUST's own daily identifier -- present on every row
/// this table ever stores, per Task 9) plus, opportunistically, a
/// `train_uid` (CIF's own identifier) if an Activation row for that same
/// `train_id` is also present somewhere in the backlog (it may not be --
/// see this module's own doc comment and this plan's "Dependency on the
/// schedule-first plan" section on why that's an accepted, named gap, not
/// a bug). Arbitrary among ties -- this table has no equivalent of
/// `resolve_origin_departure`'s own "only a DEPARTURE may claim"
/// refinement, since by construction this table already excludes PASS and
/// only Activation/Cancellation/Movement rows exist here at all.
///
/// Deliberately does NOT look at this matching row's own `train_uid`
/// column: a Movement/Cancellation row's `train_uid` is always NULL as
/// written by Task 9's own consumer (only an Activation row ever carries
/// one), so the matching row found here is realistically always a
/// Movement (the only kept type that carries a `crs`) and its `train_uid`
/// column is realistically always NULL. The real train_uid lookup is the
/// second, explicit query below, by `train_id`.
async fn find_backlog_match(
    pool: &PgPool,
    pin_origin_crs: &str,
    pin_scheduled_departure: DateTime<Utc>,
) -> anyhow::Result<Option<(String, Option<String>)>> {
    let window_start = pin_scheduled_departure - MATCH_TOLERANCE;
    let window_end = pin_scheduled_departure + MATCH_TOLERANCE;

    let row: Option<(String,)> = sqlx::query_as(
        "SELECT train_id FROM trust_event_backlog \
         WHERE UPPER(crs) = UPPER($1) AND planned_timestamp BETWEEN $2 AND $3 \
         ORDER BY planned_timestamp LIMIT 1",
    )
    .bind(pin_origin_crs)
    .bind(window_start)
    .bind(window_end)
    .fetch_optional(pool)
    .await?;

    let Some((train_id,)) = row else {
        return Ok(None);
    };

    // Look for an Activation row for the SAME train_id anywhere in the
    // backlog, unscoped by CRS (an Activation carries no location at
    // all) -- the only row type in this table that ever carries a
    // train_uid.
    let activation_uid: Option<(String,)> = sqlx::query_as(
        "SELECT train_uid FROM trust_event_backlog \
         WHERE train_id = $1 AND msg_type = '0001' AND train_uid IS NOT NULL \
         LIMIT 1",
    )
    .bind(&train_id)
    .fetch_optional(pool)
    .await?;
    Ok(Some((train_id, activation_uid.map(|(uid,)| uid))))
}

/// Decision 3 step 3: every backlog row for `train_id`/`service_date`, in
/// `received_at` order -- the entire observed history for this train.
///
/// Keyed on `train_id`, NOT `train_uid`. This is deliberate, not a typo:
/// Task 9's own consumer writes `train_uid: None` on every Movement and
/// Cancellation row (only an Activation row ever carries a real
/// `train_uid` -- see that task's own "this consumer doesn't correlate
/// Activation->Movement in-process" comment), so a query filtering on
/// `train_uid = $1` would only ever match the Activation row itself and
/// would silently return zero Movement/Cancellation rows -- exactly the
/// data this whole function exists to retrieve. `train_id`, by contrast,
/// is `NOT NULL` on all three kept message types (the migration's own
/// schema, Task 1) and is the column that actually ties one train's
/// Activation/Movement/Cancellation rows together in this table.
async fn fetch_backlog_history(
    pool: &PgPool,
    train_id: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Vec<BacklogRow>> {
    let rows = sqlx::query_as::<_, BacklogRow>(
        "SELECT train_uid, train_id, msg_type, event_type, crs, planned_timestamp, \
                actual_timestamp, variation_status \
         FROM trust_event_backlog \
         WHERE train_id = $1 AND service_date = $2 \
         ORDER BY received_at",
    )
    .bind(train_id)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Decision 3 step 4: replays `history` through the SAME
/// `train_tracking::upsert_train_event` path a live event would have
/// taken. `resolved_train_id` is set on the FIRST replayed row only,
/// mirroring `trust-consumer::process.rs`'s own "only the resolving
/// message carries these" convention -- every subsequent row passes
/// `None`, since `upsert_train_event`'s guard only needs to fire once per
/// pin. `resolved_train_uid` is set alongside it on that same first row
/// **only if `train_uid` is `Some`** -- i.e. only if `find_backlog_match`
/// found an Activation for this `train_id` somewhere in the backlog. If
/// it didn't (Decision 3 step 6's named, accepted gap: the Activation
/// fell outside the retention window, predates this consumer's own
/// deployment, or was simply never emitted on the slice of the feed this
/// consumer saw), `resolved_train_uid` stays `None` on every row, and
/// `upsert_train_event`'s existing two-field guard
/// (`train_tracking.rs:400-412`) will not advance `resolution_status`
/// past whatever it already was -- real Movement/Cancellation data still
/// lands in `train_movement_events`/`train_current_state`, just without
/// the status bump. This is not a new failure mode this replay
/// introduces; it is the exact interaction this plan's own "Dependency on
/// the schedule-first plan" section already names and accepts.
async fn replay_backlog_history(
    pool: &PgPool,
    tracked_train_id: i64,
    train_uid: Option<&str>,
    history: Vec<BacklogRow>,
) -> anyhow::Result<()> {
    let mut previous = DerivedState::awaiting_activation();
    let mut resolution_claimed = false;

    for row in history {
        let (derived, event_type, planned, actual, variation_status) = match row.msg_type.as_str()
        {
            "0003" => {
                let movement = Movement {
                    train_id: row.train_id.clone(),
                    event_type: row.event_type.clone().unwrap_or_default(),
                    gbtt_timestamp: None,
                    planned_timestamp: row.planned_timestamp.map(|t| t.timestamp_millis().to_string()),
                    actual_timestamp: row.actual_timestamp.map(|t| t.timestamp_millis().to_string()),
                    reporting_stanox: None,
                    loc_stanox: None,
                    toc_id: None,
                    variation_status: row.variation_status.clone(),
                };
                let mut derived =
                    journey::apply_movement(&previous, &movement, row.crs.as_deref());
                // Mirrors trust-consumer::process.rs's own post-apply_movement
                // override exactly: apply_movement's own delay_minutes is a
                // coarse variation_status-only estimate; a real timestamp
                // delta is used when both timestamps and a "LATE" variation
                // are present, same as a live event.
                if let (Some(p), Some(a), Some("LATE")) = (
                    row.planned_timestamp,
                    row.actual_timestamp,
                    row.variation_status.as_deref(),
                ) {
                    derived.delay_minutes = Some((a - p).num_minutes() as i32);
                }
                (
                    derived,
                    row.event_type.clone(),
                    row.planned_timestamp,
                    row.actual_timestamp,
                    row.variation_status.clone(),
                )
            }
            "0002" => (
                journey::apply_cancellation(&previous),
                None,
                None,
                row.actual_timestamp, // canx_timestamp lands in actual_timestamp, mirrors process.rs
                None,
            ),
            // "0001" (Activation) carries no derivable state change of its
            // own in trust_schema::journey -- it only supplies train_uid,
            // already known by the time this function is called. Skipped
            // as a no-op replay step, same as trust-consumer's own
            // process_message treating Activation as producing no posted
            // event.
            _ => continue,
        };

        // `loc_stanox` is always `None` here -- `trust_event_backlog`
        // never persists it (only the already-translated `crs`, see
        // Task 1's migration), so this dedup_key can differ from what a
        // live trust-consumer would have computed for the exact same
        // real-world event (which passes the real `loc_stanox`). Named,
        // accepted limitation, same posture as the plan's own raw_body
        // gap: `ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING`
        // still makes this replay idempotent against ITSELF (a retried
        // `attempt_backlog_match` call, or a redelivered ingest batch
        // upstream of it), which is all this table's own writes ever
        // need -- a live trust-consumer event for the same tracked_train_id
        // arriving *after* a full backfill of an already-departed train is
        // not a realistic scenario this design needs to guard against (by
        // the time a backlog match runs, that train's live TRUST window
        // has already closed, which is the entire reason this feature
        // exists).
        let dedup = trust_schema::dedup::dedup_key(
            &row.train_id,
            &row.msg_type,
            event_type.as_deref(),
            None,
            planned.map(|t| t.timestamp_millis().to_string()).as_deref(),
        );

        let (resolved_train_uid, resolved_train_id) = if !resolution_claimed {
            resolution_claimed = true;
            (train_uid.map(str::to_string), Some(row.train_id.clone()))
        } else {
            (None, None)
        };

        let event = common::TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid,
            resolved_train_id,
            dedup_key: dedup,
            msg_type: row.msg_type.clone(),
            event_type,
            loc_stanox: None, // never persisted by trust_event_backlog -- see the dedup_key note above
            loc_crs: row.crs.clone(),
            planned_timestamp: planned,
            actual_timestamp: actual,
            variation_status,
            raw_body: serde_json::json!({}),
            status: derived.status.clone(),
            last_reported_location: derived.last_reported_location.clone(),
            last_event_type: derived.last_event_type.clone(),
            delay_minutes: derived.delay_minutes,
            next_calling_point: derived.next_calling_point.clone(),
            eta_next: None,
            eta_source: None,
        };
        train_tracking::upsert_train_event(pool, &event).await?;
        previous = derived;
    }
    Ok(())
}

/// Entry point: attempts a full backlog match+replay for one pin.
/// Returns `Ok(true)` only if a matching `train_id` was found AND at
/// least one history row was replayed. `Ok(false)` covers every honest
/// "nothing in the backlog for this pin" outcome (no CRS+time match, or
/// the backlog's retention window has already rolled past this
/// service_date) -- exactly Decision 3 step 8's "no regression, no new
/// failure mode" posture: a pin left `Ok(false)` here is exactly as it
/// would have been without this feature at all.
///
/// `Ok(true)` does NOT by itself mean `resolution_status` reached
/// `'resolved'` -- see `replay_backlog_history`'s own doc comment on the
/// no-Activation-found case, where real Movement/Cancellation data is
/// still replayed but the status bump doesn't fire.
pub async fn attempt_backlog_match(
    pool: &PgPool,
    tracked_train_id: i64,
    pin_origin_crs: &str,
    pin_scheduled_departure: DateTime<Utc>,
    service_date: NaiveDate,
) -> anyhow::Result<bool> {
    let Some((train_id, train_uid)) =
        find_backlog_match(pool, pin_origin_crs, pin_scheduled_departure).await?
    else {
        return Ok(false);
    };

    let history = fetch_backlog_history(pool, &train_id, service_date).await?;
    if history.is_empty() {
        return Ok(false);
    }

    replay_backlog_history(pool, tracked_train_id, train_uid.as_deref(), history).await?;
    Ok(true)
}
```

- [ ] **Step 3: Wire the call site into pin creation**

**Confirmed directly, not guessed**: `post_track`'s own `INSERT INTO
tracked_trains ... RETURNING id` does NOT live inline in
`crates/api/src/routes/train.rs` — it's inside
`train_tracking::create_pin` (`crates/api/src/data/train_tracking.rs`).
`post_track` itself just calls that function and gets back an `i64`
tracking id. As of this writing (confirmed with
`grep -n "fn post_track" -A15 crates/api/src/routes/train.rs`), the
handler reads:

```rust
async fn post_track(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(pin): Json<TrackPinRequest>,
) -> Result<Json<TrackPinResponse>, (StatusCode, String)> {
    train_tracking::validate_pin(&pin, Utc::now()).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let tracking_id = train_tracking::create_pin(&app.database, &pin, &user.id)
        .await
        .map_err(internal_error("create tracking pin"))?;

    Ok(Json(TrackPinResponse {
        tracking_id,
        resolution_status: "pending",
    }))
}
```

`pin: TrackPinRequest` (`crates/common/src/lib.rs`) carries
`service_date: NaiveDate`, `origin_crs: String`, and
`scheduled_departure: DateTime<Utc>` — exactly the three inputs
`attempt_backlog_match` needs, already in scope under those names (NOT
`request.pin_origin_crs`/`request.pin_scheduled_departure` — those are
`tracked_trains`' own column names, not this request struct's field
names). Add, directly after the `let tracking_id = ...` line and before
the handler builds its `Ok(Json(...))` response:

```rust
    if let Err(err) = crate::data::trust_event_backlog_match::attempt_backlog_match(
        &app.database,
        tracking_id,
        &pin.origin_crs,
        pin.scheduled_departure,
        pin.service_date,
    )
    .await
    {
        tracing::warn!(error = ?err, tracking_id, "backlog match attempt failed; pin remains pending");
    }
```

**Ordering relative to the schedule-first design's own call site is
safe either way, confirmed directly rather than assumed**: that design's
own Task 7 adds an `attempt_schedule_match` call at this exact same spot
in `post_track` (on the still-unmerged `worktree-schedule-first-plan`
branch). Whichever of the two calls a merge order ends up placing first,
neither can clobber the other's result: `attempt_schedule_match`'s own
`UPDATE` is guarded by `WHERE train_uid IS NULL AND resolution_status =
'pending'` (that plan's own Task 6), so it silently no-ops against a row
this plan's `attempt_backlog_match` already resolved; conversely, this
plan's `upsert_train_event` write has no dependency on
`resolution_status`'s prior value at all. This plan does not require a
particular splice order relative to that other call site — place this
call before or after it, whichever lands second during merge.

(If `post_track`'s real shape has drifted from the snippet above by the
time this task is executed, re-confirm with the same grep command before
editing — the field/variable names above are what matters, not the exact
surrounding line numbers.)

**No periodic sweep is wired in this plan.** Unlike the schedule-first
design's own `run_schedule_match_sweep`, this plan does not add a
background retry loop for backlog matching. Reasoning: a backlog match
can only ever succeed once (the whole retention window is already
present at pin-creation time — there is no future data a sweep could
discover that isn't already there right now, unlike schedule matching,
which retries because `schedule_line_population` itself is populated on
its own schedule). If `attempt_backlog_match` returns `Ok(false)` at
creation time, running it again later against the same
already-fully-ingested window would return `Ok(false)` again. This is a
deliberate simplification, not an oversight — flagged here so a future
reader doesn't add a periodic sweep call site under a mistaken belief
that it would ever behave differently from the creation-time call.

- [ ] **Step 4: Write unit-shaped tests for `replay_backlog_history`'s pure logic**

`replay_backlog_history` and `find_backlog_match`/
`fetch_backlog_history` are DB-bound; add `#[ignore]`d db tests to
`trust_event_backlog_match.rs`'s own `#[cfg(test)] mod db_tests`
(following `trust_event_backlog.rs`'s own `connect()` helper shape),
covering:

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                attempt_backlog_match -- --ignored --test-threads=1`"]
    async fn a_full_activation_plus_movement_backlog_resolves_the_pin_to_resolved() {
        let pool = connect().await;
        let user_id = "TEST-BACKLOG-MATCH-USER";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("backlog-match@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        let scheduled: DateTime<Utc> = "2026-09-05T18:15:00Z".parse().unwrap();

        // Faithful to Task 9's real producer behavior, NOT a shortcut:
        // the Activation row (msg_type '0001') is the ONLY row that ever
        // carries a real `train_uid` and the ONLY row with `crs = NULL`;
        // the Movement row (msg_type '0003') carries the real `crs` +
        // timing data but `train_uid = NULL` -- Task 9's own consumer
        // never correlates the two in-process, `attempt_backlog_match`
        // does that at read time instead (see `find_backlog_match`'s own
        // doc comment). An earlier draft of this test set `train_uid` on
        // the Movement row directly, which papered over a real bug in
        // this plan's own backfill query -- caught and fixed during this
        // plan's second review pass (see Task 1's migration and this
        // module's `fetch_backlog_history`).
        sqlx::query(
            "INSERT INTO trust_event_backlog \
                (crs, train_uid, train_id, service_date, msg_type, event_type, \
                 planned_timestamp, actual_timestamp, variation_status, dedup_key) \
             VALUES (NULL, $1, $2, $3, '0001', NULL, NULL, NULL, NULL, $4), \
                    ($5, NULL, $2, $3, '0003', 'DEPARTURE', $6, $6, 'ON TIME', $7)",
        )
        .bind("C99999")
        .bind("TEST-BACKLOG-TRAIN-ID")
        .bind(service_date)
        .bind("test-backlog-dedup-activation")
        .bind("EUS")
        .bind(scheduled)
        .bind("test-backlog-dedup-movement")
        .execute(&pool)
        .await
        .expect("seed backlog rows");

        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("EUS")
        .bind(scheduled)
        .fetch_one(&pool)
        .await
        .expect("seed tracked_trains row");

        let matched = attempt_backlog_match(&pool, tracked_train_id, "EUS", scheduled, service_date)
            .await
            .expect("attempt_backlog_match");
        assert!(matched);

        let (resolution_status, train_uid): (String, Option<String>) = sqlx::query_as(
            "SELECT resolution_status, train_uid FROM tracked_trains WHERE id = $1",
        )
        .bind(tracked_train_id)
        .fetch_one(&pool)
        .await
        .expect("read back tracked_trains");
        assert_eq!(resolution_status, "resolved");
        assert_eq!(train_uid, Some("C99999".to_string()));

        sqlx::query("DELETE FROM trust_event_backlog WHERE train_id = 'TEST-BACKLOG-TRAIN-ID'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                attempt_backlog_match_with_no_matching_rows -- --ignored --test-threads=1`"]
    async fn no_matching_backlog_rows_leaves_the_pin_untouched() {
        let pool = connect().await;
        let user_id = "TEST-BACKLOG-MATCH-EMPTY-USER";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("backlog-match-empty@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        let scheduled: DateTime<Utc> = "2026-09-05T09:00:00Z".parse().unwrap();
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("ZZZ-NOWHERE")
        .bind(scheduled)
        .fetch_one(&pool)
        .await
        .expect("seed tracked_trains row");

        let matched =
            attempt_backlog_match(&pool, tracked_train_id, "ZZZ-NOWHERE", scheduled, service_date)
                .await
                .expect("attempt_backlog_match");
        assert!(!matched);

        let (resolution_status,): (String,) =
            sqlx::query_as("SELECT resolution_status FROM tracked_trains WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back tracked_trains");
        assert_eq!(resolution_status, "pending");
    }
}
```

- [ ] **Step 5: Register the module and run**

Add `pub mod trust_event_backlog_match;` to `crates/api/src/data/mod.rs`.

```bash
cd crates/api && sqlx migrate run
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api attempt_backlog_match -- --ignored --test-threads=1
cargo build -p api
cargo test -p api --lib
```

Expected: both new db tests pass; the crate builds and its non-DB test
suite is unaffected.

- [ ] **Step 6: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/data/trust_event_backlog_match.rs crates/api/src/data/mod.rs crates/api/src/routes/train.rs
git commit -m "Add backlog-consumption replay for late-tracking pins, reusing upsert_train_event"
```

---

## Task 6: `aggregator` — real, wired retention/prune job

**Files:**
- Modify: `crates/aggregator/src/config.rs`
- Modify: `crates/aggregator/src/queries.rs`
- Modify: `crates/aggregator/src/main.rs`

**Interfaces:**
- Produces: `config.trust_event_backlog_retention_days: i64` (default
  `1`), `pub async fn prune_trust_event_backlog(pool, retention_days) ->
  Result<u64>`, wired into the existing prune cycle.

**Why `aggregator`, not the new consumer or `api` itself**: this
codebase's own established pattern for pruning a table populated via an
`api` ingest route is a periodic call from `aggregator`'s own main loop
(`queries::prune_history`/`prune_daily_stats`/`prune_half_hourly_stats`,
all real and wired, confirmed directly in this plan's own research) —
not a job inside the writer (`trust-consumer`'s own dead
`retention_days` field is the cautionary example this plan explicitly
avoids repeating) and not inside `api`'s own request-handling code
(which has no background-task loop of its own today). This plan follows
the existing precedent rather than inventing a new one.

- [ ] **Step 1: Add the config field**

In `crates/aggregator/src/config.rs`, add alongside
`half_hourly_stats_retention_hours`:

```rust
    /// How long to keep `trust_event_backlog` rows before pruning them.
    ///
    /// DEFAULT IS 1 DAY, DELIBERATELY. The design spec this table
    /// implements
    /// (docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md,
    /// Decision 5) found only a secondhand, imprecisely sourced citation
    /// that TRUST/Train Movements retention is unrestricted by licence --
    /// genuinely favorable evidence, but weaker than the quoted-clause
    /// standard this repo holds itself to for LDBWS
    /// (docs/superpowers/plans/2026-09-01-ldbws-data-retention.md). A
    /// human must confirm TRUST's real licence terms directly with RDM
    /// before this value is ever configured above 1 in a real production
    /// deployment -- do not bump this default, or any Helm values.yaml
    /// default derived from it, without that confirmation happening
    /// first. See
    /// docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md's
    /// own "Scope decision: retention tier and the licensing safeguard"
    /// section.
    #[arg(long, env, default_value_t = 1)]
    pub trust_event_backlog_retention_days: i64,
```

- [ ] **Step 2: Add the prune query**

In `crates/aggregator/src/queries.rs`, directly below `prune_history`
(same file, same shape — `received_at` is a `TIMESTAMPTZ`, same as
`line_status_history.computed_at`, so the identical interval-string
pattern applies):

```rust
/// Prunes `trust_event_backlog` rows older than `retention_days`. See
/// `Config::trust_event_backlog_retention_days`'s own doc comment for
/// the licensing safeguard this default (1) exists to enforce -- this
/// function itself has no opinion on the value passed in; it prunes
/// whatever it's told to.
pub async fn prune_trust_event_backlog(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM trust_event_backlog WHERE received_at < NOW() - ($1 || ' days')::interval",
    )
    .bind(retention_days.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 3: Wire it into the main loop**

In `crates/aggregator/src/main.rs`, find the existing call site for
`queries::prune_history`/`prune_daily_stats` (the `run_cycle` function,
or wherever those two calls currently live — confirmed at `main.rs:238`/
`main.rs:295` in this plan's own research). Add, directly alongside
them, both:
1. A parameter thread-through (`run_cycle`'s own signature already
   passes `history_retention_days`/`daily_stats_retention_days`/
   `half_hourly_stats_retention_hours` — add
   `trust_event_backlog_retention_days: i64` to that same parameter
   list, and to the call site in `main()` that supplies
   `config.trust_event_backlog_retention_days`).
2. **The loud startup/per-cycle safety warning** (this plan's own Global
   Constraints requirement — a log line a deployer will actually see,
   not just a doc comment):

```rust
    if trust_event_backlog_retention_days > 1 {
        tracing::warn!(
            configured_days = trust_event_backlog_retention_days,
            "trust_event_backlog retention is configured above the safe 1-day default -- \
             a human must have already confirmed TRUST's real Train Movements licence terms \
             directly with RDM before this value was set; see \
             docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md's own \
             \"Scope decision: retention tier and the licensing safeguard\" section"
        );
    }
    let trust_event_backlog_pruned =
        queries::prune_trust_event_backlog(pool, trust_event_backlog_retention_days).await?;
    metrics::counter!(common::metrics::metric_name(
        "aggregator_trust_event_backlog_rows_pruned_total"
    ))
    .increment(trust_event_backlog_pruned);
```

Place the warning check once per cycle (not gated behind a "first cycle
only" flag) — cheap, and guarantees it reappears in logs on every
restart too, not just the very first one, so it survives log rotation
and isn't a one-time-only notice a deployer could plausibly miss.

- [ ] **Step 4: Write a DB-gated test for the prune query**

Add to `crates/aggregator/src/queries.rs`'s existing `#[cfg(test)]`
module, mirroring `prune_daily_stats_deletes_only_rows_older_than_the_retention_window`'s
own shape exactly:

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            prune_trust_event_backlog -- --ignored --test-threads=1`"]
async fn prune_trust_event_backlog_deletes_only_rows_older_than_the_retention_window() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.unwrap();

    sqlx::query(
        "INSERT INTO trust_event_backlog (train_id, service_date, msg_type, received_at, dedup_key) \
         VALUES ('TEST-PRUNE-OLD', '2026-09-01', '0001', NOW() - interval '2 days', 'test-prune-old'), \
                ('TEST-PRUNE-NEW', '2026-09-05', '0001', NOW(), 'test-prune-new')",
    )
    .execute(&pool)
    .await
    .expect("seed fixture rows");

    let pruned = prune_trust_event_backlog(&pool, 1).await.expect("prune");
    assert_eq!(pruned, 1, "only the 2-day-old row should be pruned at a 1-day retention");

    let remaining: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM trust_event_backlog WHERE train_id = 'TEST-PRUNE-NEW'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining.0, 1);

    sqlx::query("DELETE FROM trust_event_backlog WHERE train_id IN ('TEST-PRUNE-OLD', 'TEST-PRUNE-NEW')")
        .execute(&pool)
        .await
        .ok();
}
```

- [ ] **Step 5: Run**

```bash
cd crates/api && sqlx migrate run
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p aggregator prune_trust_event_backlog -- --ignored --test-threads=1
cargo build -p aggregator
cargo test -p aggregator --lib
```

- [ ] **Step 6: Verify the warning actually fires (by inspection, not just compiling)**

```bash
RUST_LOG=warn cargo run -p aggregator -- --database-url "$DATABASE_URL" --lines-dir lines --trust-event-backlog-retention-days 7 2>&1 | grep "retention is configured above"
```

Expected: the warning line appears at least once before the process is
interrupted (Ctrl-C once it's clearly looping). This is the concrete
proof this plan's own retention job is real and wired — the exact thing
`trust-consumer`'s dead `retention_days` field never had.

- [ ] **Step 7: Commit**

```bash
git add crates/aggregator/src/config.rs crates/aggregator/src/queries.rs crates/aggregator/src/main.rs
git commit -m "Add a real, wired trust_event_backlog prune job with a loud >1-day retention warning"
```

---

## Task 7: `trust-backlog-consumer` — crate scaffold

**Files:**
- Create: `crates/trust-backlog-consumer/Cargo.toml`
- Create: `crates/trust-backlog-consumer/src/config.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: the `trust-backlog-consumer` binary crate skeleton and its
  `Config` struct, consumed by every later task in this crate.

- [ ] **Step 1: Register the workspace member**

In the root `Cargo.toml`'s `members` array (confirmed NOT alphabetized
overall — it's ordered roughly by when each crate was added — so
"alphabetically" isn't a real constraint here; just add it near its
closest relative), add directly above `"crates/trust-consumer"`:

```toml
    "crates/trust-backlog-consumer",
```

- [ ] **Step 2: `Cargo.toml`**

**Confirmed directly, not guessed: this workspace has no
`[workspace.dependencies]` table at all.** Every crate pins its own
explicit version string inline — grepped directly
(`grep -rln "workspace = true" crates/*/Cargo.toml` returns zero matches
anywhere in this repo) and confirmed against the root `Cargo.toml` (a
bare `[workspace]` + `members = [...]`, no `[workspace.dependencies]`
section). An earlier draft of this Cargo.toml used `{ workspace = true }`
throughout, which would fail to resolve at all (`cargo` errors on
`workspace = true` with no matching `[workspace.dependencies]` entry) —
caught during this plan's second review pass. Every dependency below is
copied verbatim from `crates/full-coverage-consumer/Cargo.toml` (the
sibling crate needing the same `common`/`movement-feed`/`health-http`/
`metrics`/`reqwest`/`serde`/`tokio`/`tracing` stack), plus `chrono-tz`
(copied from `crates/aggregator/Cargo.toml`, needed for Task 9/10's own
Europe/London rail-day calculation) and `dotenv`/`clap`/`anyhow` at the
same versions `full-coverage-consumer` already pins. Also note
`edition = "2024"`, not `"2021"` — every one of this workspace's 21
member crates uses `"2024"` (`grep -h "^edition" crates/*/Cargo.toml`),
confirmed directly rather than assumed:

```toml
[package]
name = "trust-backlog-consumer"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.104"
chrono = { version = "0.4.45", features = ["serde"] }
chrono-tz = "0.10"
clap = { version = "4.6.6", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
health-http = { path = "../health-http" }
metrics = "0.24"
movement-feed = { path = "../movement-feed" }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip", "query"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "time"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
trust-schema = { path = "../trust-schema" }
```

This plan does NOT need `rdkafka`/`redis` direct deps or
`common::service_args::KafkaConnectionArgs` — see Task 8's own note on
why this consumer is Redis-Streams-only, no Kafka backend. (Re-confirm
every version number above against `crates/full-coverage-consumer/Cargo.toml`
and `crates/aggregator/Cargo.toml` at implementation time in case either
has moved on since this plan was written — the versions above are a
snapshot, not a promise they'll still be current.)

- [ ] **Step 3: `config.rs`**

```rust
use std::path::Path;

use clap::Parser;
use common::config::{LineCatalogue, parse_lines};

use crate::stanox_crs::StanoxCrsTable;

fn parse_stanox_crs(path: &str) -> anyhow::Result<StanoxCrsTable> {
    StanoxCrsTable::from_file(Path::new(path))
}

/// CLI/env configuration for the `trust-backlog-consumer` service -- a
/// third, independent consumer group on the same `movement-events` Redis
/// Stream `trust-consumer`/`full-coverage-consumer` already read. See
/// docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md and
/// docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md.
///
/// Deliberately Redis-Streams-only, unlike `trust-consumer`/
/// `full-coverage-consumer` (which both still support a legacy direct-
/// Kafka backend from before Deploy A). This crate is new, built after
/// `movement-relay`'s own Redis Streams design was already the
/// established path -- there is no legacy Kafka deployment of this
/// consumer to keep compatible with, so it only ever speaks to the
/// `movement-events` Redis Stream directly via `movement_feed::redis_stream::RedisStreamMovementFeed`.
#[derive(Debug, Parser)]
pub struct Config {
    #[arg(long, env, default_value = "redis://redis:6379")]
    pub redis_url: String,

    /// How long an entry may sit unacked in this consumer's own
    /// pending-entries list before its periodic sweep reclaims it. Same
    /// default/reasoning as `trust-consumer`'s identical field.
    #[arg(long, env, default_value_t = 30)]
    pub redis_autoclaim_min_idle_secs: u64,

    /// How often (seconds) this crate compares its own consumer group's
    /// `last-delivered-id` against the stream's oldest retained entry.
    /// Same cadence/reasoning as `trust-consumer`'s identical field.
    #[arg(long, env, default_value_t = 60)]
    pub redis_gap_check_secs: u64,

    /// The `api` crate's ingestion endpoint for this crate's own event
    /// batches.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/trust-event-backlog"
    )]
    pub api_ingest_url: String,

    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

    /// STANOX->CRS translation table, loaded once at startup. Same file
    /// format/provenance as `trust-consumer`'s identical field --
    /// deliberately a separate, crate-local copy of that logic (see
    /// `stanox_crs`'s own module doc), matching this codebase's own
    /// existing precedent of NOT sharing this kind of small,
    /// crate-specific reference-table logic across consumer crates
    /// (`full-coverage-consumer`'s own `stanox_tiploc.rs` is a third,
    /// independent, differently-shaped implementation of the same idea).
    #[arg(
        long = "stanox-crs-file",
        env = "STANOX_CRS_FILE",
        default_value = "/app/reference-data/stanox-crs.csv",
        value_parser = parse_stanox_crs,
        value_name = "FILE"
    )]
    pub stanox_crs: StanoxCrsTable,

    #[arg(long, env, default_value_t = 3600)]
    pub stanox_crs_reload_secs: u64,

    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub stanox_crs_url: String,

    /// Static line catalogue, needed to build the CRS reverse index this
    /// consumer scopes its writes by (Task 8) -- built independently of,
    /// and with zero dependency on,
    /// docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md's
    /// own equivalent index (see this plan's "Dependency on the
    /// schedule-first plan" section for the full reasoning).
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,

    #[arg(long, env, default_value = "0.0.0.0:8083")]
    pub health_bind_url: String,
    #[arg(long, env, default_value_t = 9096)]
    pub metrics_port: u16,
    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,
}
```

- [ ] **Step 4: Build check (will fail until `stanox_crs`/`main.rs` exist — expected)**

```bash
cargo check -p trust-backlog-consumer 2>&1 | tail -20
```

Expected: errors about missing `mod stanox_crs;`/`mod main` — confirms
the scaffold parses as valid Rust syntax before the rest of the crate
exists. Do not attempt to fix these errors in this task; Task 8 adds
`stanox_crs.rs` and Task 10 adds `main.rs`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/trust-backlog-consumer/Cargo.toml crates/trust-backlog-consumer/src/config.rs
git commit -m "Scaffold the trust-backlog-consumer crate and its CLI config"
```

---

## Task 8: `trust-backlog-consumer` — CRS reverse index + STANOX/CRS table

**Files:**
- Create: `crates/trust-backlog-consumer/src/crs_index.rs`
- Create: `crates/trust-backlog-consumer/src/stanox_crs.rs`

**Interfaces:**
- Produces: `pub fn build_crs_index(lines: &[common::LineDefinition]) ->
  std::collections::HashSet<String>` and `pub struct StanoxCrsTable`
  (with `stanox_to_crs`, `from_file`, `from_records`), consumed by
  Task 9.

- [ ] **Step 1: `crs_index.rs`**

```rust
//! An independently-built `HashSet<CRS>` of every catalogued-line CRS
//! with at least one TIPLOC-bearing station -- this consumer's own scope
//! boundary (Decision 2 of
//! docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md).
//!
//! Deliberately NOT reused from, or dependent on,
//! docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md's
//! own `crs_to_line_ids` (a private field inside `api`'s own process
//! memory, unreachable from a separate binary crate regardless of merge
//! order) -- see
//! docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md's own
//! "Dependency on the schedule-first plan" section for the full
//! reasoning. This module is this codebase's THIRD independent
//! implementation of the same "line has >=1 TIPLOC-bearing station"
//! predicate, alongside `full-coverage-consumer::population::build_tiploc_index`
//! and (once merged) `api::data::schedule_matching::crs_to_line_ids` --
//! a named, accepted repeat (see that plan's own Non-goals), not
//! deduplicated here.
//!
//! A plain `HashSet<String>` (not a `HashMap<String, Vec<line_id>>` like
//! the other two): this consumer only ever needs a yes/no "is this CRS
//! in scope" answer (Decision 2's own scoping rule), never "which line,"
//! so the smaller, simpler type is used rather than carrying unused
//! line-id data through every Movement this consumer processes.

use std::collections::HashSet;

use common::LineDefinition;

pub fn build_crs_index(lines: &[LineDefinition]) -> HashSet<String> {
    let mut index = HashSet::new();
    for line in lines {
        for station in &line.stations {
            if station.tiploc.is_some() {
                index.insert(station.crs.to_uppercase());
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, stations: Vec<(&str, Option<&str>)>) -> LineDefinition {
        LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "rail".to_string(),
            category: "national-rail".to_string(),
            operators: vec![],
            stations: stations
                .into_iter()
                .map(|(crs, tiploc)| common::Station {
                    crs: crs.to_string(),
                    tiploc: tiploc.map(str::to_string),
                    role: "minor".to_string(),
                    segment: None,
                })
                .collect(),
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    #[test]
    fn a_tiploc_bearing_station_is_indexed_uppercased() {
        let lines = vec![line("wcml", vec![("eus", Some("EUSTON"))])];
        let index = build_crs_index(&lines);
        assert!(index.contains("EUS"));
    }

    #[test]
    fn a_station_with_no_tiploc_is_not_indexed() {
        let lines = vec![line("wcml", vec![("ZZZ", None)])];
        let index = build_crs_index(&lines);
        assert!(!index.contains("ZZZ"));
    }

    #[test]
    fn two_lines_sharing_a_crs_index_it_once() {
        let lines = vec![
            line("line-a", vec![("EUS", Some("EUSTON"))]),
            line("line-b", vec![("EUS", Some("EUSTON"))]),
        ];
        let index = build_crs_index(&lines);
        assert_eq!(index.len(), 1);
    }
}
```

- [ ] **Step 2: `stanox_crs.rs`**

Port `crates/trust-consumer/src/stanox_crs.rs` verbatim into
`crates/trust-backlog-consumer/src/stanox_crs.rs` (same
`StanoxCrsTable` struct, same `from_file`/`from_records`/
`stanox_to_crs` methods, same clap `value_parser` shape used by
`config.rs`'s `parse_stanox_crs`). Read the source file first
(`crates/trust-consumer/src/stanox_crs.rs`) and copy it exactly — this
is a deliberate, named duplication (see this task's own module doc
above and this plan's Non-goals), not a fresh implementation, so it must
match `trust-consumer`'s own tested behavior exactly, including its CSV
parsing edge cases and excluded-ambiguous-STANOX handling.

- [ ] **Step 3: Test**

```bash
cargo test -p trust-backlog-consumer crs_index::
cargo test -p trust-backlog-consumer stanox_crs::
```

Expected: `crs_index`'s 3 new tests pass; `stanox_crs`'s ported tests
pass unchanged (same assertions as `trust-consumer`'s own module, since
the code is a verbatim copy).

- [ ] **Step 4: Commit**

```bash
git add crates/trust-backlog-consumer/src/crs_index.rs crates/trust-backlog-consumer/src/stanox_crs.rs
git commit -m "Add trust-backlog-consumer's independent CRS reverse index and STANOX/CRS table"
```

---

## Task 9: `trust-backlog-consumer` — message filtering/mapping (`process.rs`)

**Files:**
- Create: `crates/trust-backlog-consumer/src/process.rs`

**Interfaces:**
- Consumes: `trust_schema::schema::TrustMessage`,
  `crate::crs_index`/`crate::stanox_crs`.
- Produces: `pub struct ProcessorState` (carries the same kind of
  cross-batch `pending_activations: HashMap<train_id,
  PendingActivation>` map `trust-consumer` already uses, for the same
  reason — a bare Movement/Cancellation carries no `service_date`, only
  an Activation does), `pub fn process_message(message: &TrustMessage,
  state: &mut ProcessorState, stanox_crs: &StanoxCrsTable, crs_index:
  &HashSet<String>, today: NaiveDate) -> Option<common::TrustBacklogEventMessage>`,
  consumed by Task 10's main loop.

- [ ] **Step 1: Write the module**

```rust
//! Filters and maps raw `TrustMessage`s into `trust_event_backlog` rows,
//! per docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md's
//! own "What counts as a key journey point" section:
//!
//! - Only Activation (`0001`) / Cancellation (`0002`) / Movement (`0003`)
//!   survive at all -- `ChangeOfOrigin`/`ChangeOfIdentity`/`Unknown` are
//!   dropped unconditionally, they carry no journey-point data.
//! - A Movement survives only if its `event_type` is `ARRIVAL` or
//!   `DEPARTURE` (never `PASS`) AND its translated CRS is in this
//!   consumer's own `crs_index` (catalogued-line scoping, Decision 2).
//! - An Activation/Cancellation survives regardless of location (neither
//!   carries one) -- scoping by CRS is meaningless for them; they are
//!   kept because they're load-bearing plumbing (Activation) or
//!   themselves a real journey event (Cancellation), per the plan's own
//!   reasoning.
//!
//! `service_date` for a bare Movement/Cancellation (neither carries a
//! date field) is sourced from a parked Activation's own
//! `schedule_start_date` when one has been observed for this `train_id`
//! in-process, falling back to the current Europe/London rail day
//! otherwise -- an accepted approximation identical in kind to
//! `trust-consumer::process.rs`'s own pre-existing "an Activation this
//! process never saw" gap, not a new one this module invents.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use trust_schema::schema::TrustMessage;

use crate::stanox_crs::StanoxCrsTable;

/// Cross-batch memory, mirroring `trust-consumer::process::ProcessorState`'s
/// own `pending_activations` map exactly (same purpose: a later
/// Movement/Cancellation needs the `service_date` an earlier Activation
/// carried). Deliberately does NOT carry a `resolved`/`last_derived`
/// equivalent -- this consumer has no notion of "resolving a pin" and no
/// per-train derived-state fold to maintain; every message is mapped
/// independently, not folded against a running journey state.
#[derive(Debug, Default)]
pub struct ProcessorState {
    pub pending_service_dates: HashMap<String, NaiveDate>,
}

pub fn process_message(
    message: &TrustMessage,
    state: &mut ProcessorState,
    stanox_crs: &StanoxCrsTable,
    crs_index: &HashSet<String>,
    today: NaiveDate,
) -> Option<common::TrustBacklogEventMessage> {
    match message {
        TrustMessage::Activation(activation) => {
            let service_date = activation
                .schedule_start_date
                .parse::<NaiveDate>()
                .unwrap_or(today);
            state
                .pending_service_dates
                .insert(activation.train_id.clone(), service_date);

            let dedup = trust_schema::dedup::dedup_key(
                &activation.train_id,
                "0001",
                None,
                None,
                None,
            );
            Some(common::TrustBacklogEventMessage {
                crs: None,
                train_uid: Some(activation.train_uid.clone()),
                train_id: activation.train_id.clone(),
                service_date,
                msg_type: "0001".to_string(),
                event_type: None,
                planned_timestamp: None,
                actual_timestamp: None,
                variation_status: None,
                delay_minutes: None,
                dedup_key: dedup,
            })
        }

        TrustMessage::Movement(movement) => {
            // Only a real calling point -- never PASS. See this module's
            // own doc comment.
            if movement.event_type != "ARRIVAL" && movement.event_type != "DEPARTURE" {
                return None;
            }

            let loc_crs = movement
                .loc_stanox
                .as_deref()
                .and_then(|stanox| stanox_crs.stanox_to_crs(stanox))?;
            if !crs_index.contains(&loc_crs.to_uppercase()) {
                return None;
            }

            let planned = movement.planned_timestamp.as_deref().and_then(parse_epoch_millis);
            let actual = movement.actual_timestamp.as_deref().and_then(parse_epoch_millis);
            let delay_minutes = match (planned, actual, movement.variation_status.as_deref()) {
                (Some(p), Some(a), Some("LATE")) => Some((a - p).num_minutes() as i32),
                _ => None,
            };

            let service_date = state
                .pending_service_dates
                .get(&movement.train_id)
                .copied()
                .unwrap_or(today);

            let dedup = trust_schema::dedup::dedup_key(
                &movement.train_id,
                "0003",
                Some(&movement.event_type),
                movement.loc_stanox.as_deref(),
                movement.planned_timestamp.as_deref(),
            );

            Some(common::TrustBacklogEventMessage {
                crs: Some(loc_crs),
                train_uid: None, // this consumer doesn't correlate Activation->Movement in-process;
                                  // api's own backlog-match (Task 5) joins them at read time instead.
                train_id: movement.train_id.clone(),
                service_date,
                msg_type: "0003".to_string(),
                event_type: Some(movement.event_type.clone()),
                planned_timestamp: planned,
                actual_timestamp: actual,
                variation_status: movement.variation_status.clone(),
                delay_minutes,
                dedup_key: dedup,
            })
        }

        TrustMessage::Cancellation(cancellation) => {
            let service_date = state
                .pending_service_dates
                .get(&cancellation.train_id)
                .copied()
                .unwrap_or(today);
            let actual = cancellation
                .canx_timestamp
                .as_deref()
                .and_then(parse_epoch_millis);

            let dedup =
                trust_schema::dedup::dedup_key(&cancellation.train_id, "0002", None, None, None);

            Some(common::TrustBacklogEventMessage {
                crs: None,
                train_uid: None,
                train_id: cancellation.train_id.clone(),
                service_date,
                msg_type: "0002".to_string(),
                event_type: None,
                planned_timestamp: None,
                actual_timestamp: actual,
                variation_status: None,
                delay_minutes: None,
                dedup_key: dedup,
            })
        }

        TrustMessage::ChangeOfOrigin(_)
        | TrustMessage::ChangeOfIdentity(_)
        | TrustMessage::Unknown(_) => None,
    }
}

fn parse_epoch_millis(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let millis: i64 = raw.parse().ok()?;
    chrono::DateTime::from_timestamp_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stanox_table() -> StanoxCrsTable {
        StanoxCrsTable::from_records(vec![common::StanoxCrsRecord {
            stanox: "87212".to_string(),
            crs: "WAT".to_string(),
            tiploc: "WATRLMN".to_string(),
            station_name: "LONDON WATERLOO".to_string(),
            source_sequence: 1,
        }])
    }

    fn crs_index_with(crs: &[&str]) -> HashSet<String> {
        crs.iter().map(|c| c.to_uppercase()).collect()
    }

    fn today() -> NaiveDate {
        "2026-09-05".parse().unwrap()
    }

    #[test]
    fn a_departure_at_a_catalogued_crs_is_kept() {
        let message = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "221832406",
            "DEPARTURE",
            Some("87212"),
            Some("ON TIME"),
        ));
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().crs, Some("WAT".to_string()));
    }

    #[test]
    fn a_pass_event_is_dropped() {
        let message = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "221832406",
            "PASS",
            Some("87212"),
            Some("ON TIME"),
        ));
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn a_departure_at_an_uncatalogued_crs_is_dropped() {
        let message = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "221832406",
            "DEPARTURE",
            Some("87212"),
            Some("ON TIME"),
        ));
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["EUS"]), // WAT not in scope
            today(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn a_departure_at_an_untranslatable_stanox_is_dropped() {
        let message = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "221832406",
            "DEPARTURE",
            Some("99999"), // not in stanox_table()
            Some("ON TIME"),
        ));
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn a_change_of_origin_is_always_dropped() {
        let message = TrustMessage::ChangeOfOrigin(trust_schema::schema::ChangeOfOrigin {
            train_id: "221832406".to_string(),
        });
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        );
        assert!(result.is_none());
    }

    #[test]
    fn a_movement_reuses_the_activations_own_service_date() {
        let activation = TrustMessage::Activation(trust_schema::schema_test_support::activation(
            "221832406",
            "C21373",
            "2026-09-04",
        ));
        let mut state = ProcessorState::default();
        process_message(&activation, &mut state, &stanox_table(), &crs_index_with(&["WAT"]), today());

        let movement = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "221832406",
            "DEPARTURE",
            Some("87212"),
            Some("ON TIME"),
        ));
        let result = process_message(
            &movement,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        )
        .unwrap();
        assert_eq!(result.service_date, "2026-09-04".parse::<NaiveDate>().unwrap());
    }

    #[test]
    fn a_movement_with_no_parked_activation_falls_back_to_today() {
        let message = TrustMessage::Movement(trust_schema::schema_test_support::movement(
            "999999999",
            "DEPARTURE",
            Some("87212"),
            Some("ON TIME"),
        ));
        let mut state = ProcessorState::default();
        let result = process_message(
            &message,
            &mut state,
            &stanox_table(),
            &crs_index_with(&["WAT"]),
            today(),
        )
        .unwrap();
        assert_eq!(result.service_date, today());
    }
}
```

- [ ] **Step 2: Add a small test-support helper to `trust-schema`**

The tests above construct `trust_schema::schema::Movement`/`Activation`
values directly, but those structs' fields are all `pub`, so no new
helper is strictly required — **use direct struct literals instead of
inventing a `schema_test_support` module** (that module does not exist
in this codebase and this task does not add one, to avoid growing
`trust-schema`'s public surface for a need this crate's own tests can
satisfy directly). Replace every
`trust_schema::schema_test_support::movement(...)`/`::activation(...)`
call above with a direct struct literal, e.g.:

```rust
    fn movement(train_id: &str, event_type: &str, loc_stanox: Option<&str>, variation_status: Option<&str>) -> trust_schema::schema::Movement {
        trust_schema::schema::Movement {
            train_id: train_id.to_string(),
            event_type: event_type.to_string(),
            gbtt_timestamp: None,
            planned_timestamp: Some("1787941920000".to_string()),
            actual_timestamp: Some("1787941920000".to_string()),
            reporting_stanox: None,
            loc_stanox: loc_stanox.map(str::to_string),
            toc_id: None,
            variation_status: variation_status.map(str::to_string),
        }
    }

    fn activation(train_id: &str, train_uid: &str, schedule_start_date: &str) -> trust_schema::schema::Activation {
        trust_schema::schema::Activation {
            train_id: train_id.to_string(),
            train_uid: train_uid.to_string(),
            toc_id: "SW".to_string(),
            train_service_code: "22345000".to_string(),
            schedule_wtt_id: "WTT1".to_string(),
            schedule_start_date: schedule_start_date.to_string(),
            schedule_end_date: schedule_start_date.to_string(),
        }
    }
```

added directly inside `process.rs`'s own `#[cfg(test)] mod tests`, and
update every call site above to use these local helpers instead of the
non-existent `schema_test_support` path.

- [ ] **Step 3: Run the tests**

```bash
cargo test -p trust-backlog-consumer process::
```

Expected: all 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/trust-backlog-consumer/src/process.rs
git commit -m "Add trust-backlog-consumer's key-journey-point filtering and mapping logic"
```

---

## Task 10: `trust-backlog-consumer` — `main.rs` loop wiring (third consumer group)

**Files:**
- Create: `crates/trust-backlog-consumer/src/main.rs`
- Create: `crates/trust-backlog-consumer/src/queries.rs`

**Interfaces:**
- Consumes: `movement_feed::redis_stream::RedisStreamMovementFeed`,
  `crate::{config::Config, crs_index, process, stanox_crs}`.
- Produces: the running binary; no further public interface (this is
  the top of the dependency graph).

- [ ] **Step 1: `queries.rs` — the two HTTP calls this crate makes**

```rust
//! HTTP client calls to `api`, mirroring `full-coverage-consumer::queries`'s
//! own shape exactly (same `fetch_stanox_crs`/`post_*` pattern, same
//! `internal_oauth`-bearer-token convention).

pub async fn fetch_stanox_crs(
    client: &reqwest::Client,
    url: &str,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    let token = internal_oauth.token().await?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn post_trust_event_backlog(
    client: &reqwest::Client,
    url: &str,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    events: &[common::TrustBacklogEventMessage],
) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let token = internal_oauth.token().await?;
    client
        .post(url)
        .bearer_auth(token)
        .json(events)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
```

(Confirm `common::oauth_client::OAuthTokenCache`'s exact `.token()`
method name/signature against `full-coverage-consumer::queries::fetch_stanox_crs`'s
own real body — `grep -n "fn fetch_stanox_crs" -A15 crates/full-coverage-consumer/src/queries.rs`
— and match it exactly; the sketch above may differ from the real
signature in small ways.)

- [ ] **Step 2: `main.rs`**

```rust
//! `trust-backlog-consumer`: a third, independent Redis Streams consumer
//! group on the `movement-events` stream, retaining a short,
//! catalogued-line-scoped, key-journey-point-only backlog of TRUST
//! events for late-tracking pins. See
//! docs/superpowers/specs/2026-09-05-trust-event-backlog-design.md and
//! docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md.
//!
//! Loop shape mirrors `full-coverage-consumer/src/main.rs`'s own
//! multi-cadence-in-one-loop shape (stanox_crs reload / consume-and-filter
//! / batch POST, each on its own timer or per-iteration, all checked once
//! per loop) -- this crate needs no population/stats-write cadence of its
//! own, so it is simpler than that crate's loop, not a copy of it.

mod config;
mod crs_index;
mod process;
mod queries;
mod stanox_crs;

use std::sync::RwLock;
use std::time::Duration;

use clap::Parser;
use config::Config;
use movement_feed::ActiveFeed;
use movement_feed::MovementFeed;
use movement_feed::redis_stream::RedisStreamMovementFeed;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::parse();
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let connection_state =
        health_http::spawn(config.health_bind_url.clone(), "connected", "disconnected");
    let http = reqwest::Client::new();
    let internal_oauth = config.internal_oauth.token_cache();

    // Built once: purely static-catalogue-derived, needs no reload at
    // runtime (config.lines doesn't change without a restart).
    let crs_index = crs_index::build_crs_index(&config.lines);

    // Wrapped in `ActiveFeed::RedisStream`, not used bare -- this is what
    // actually threads `connection_state` through to flip the /healthz
    // readiness flag and the `trust_backlog_consumer_ready` gauge on every
    // `next_batch` call, exactly `full-coverage-consumer/src/main.rs`'s own
    // established pattern for a Redis-Streams backend
    // (`crates/movement-feed/src/active_feed.rs`'s own `ActiveFeed::RedisStream`
    // variant already does this generically -- see that module's doc
    // comment). `ActiveFeed<K>` is generic over a Kafka backend type `K`
    // this crate never uses (Task 7's own "Redis-Streams-only" decision);
    // `RedisStreamMovementFeed` itself trivially satisfies `K: MovementFeed`,
    // so `ActiveFeed<RedisStreamMovementFeed>` type-checks even though the
    // `Kafka` variant is never constructed.
    let mut feed: ActiveFeed<RedisStreamMovementFeed> = ActiveFeed::RedisStream(
        Box::new(
            RedisStreamMovementFeed::connect(
                &config.redis_url,
                "trust-event-backlog",
                "trust-event-backlog-1",
                Duration::from_secs(config.redis_autoclaim_min_idle_secs),
            )
            .await?,
        ),
        connection_state,
        "trust_backlog_consumer_ready",
    );

    let stanox = RwLock::new(config.stanox_crs.clone());
    let mut process_state = process::ProcessorState::default();

    let stanox_crs_reload_interval = Duration::from_secs(config.stanox_crs_reload_secs);
    let mut last_stanox_crs_reload = tokio::time::Instant::now() - stanox_crs_reload_interval;
    let redis_gap_check_interval = Duration::from_secs(config.redis_gap_check_secs);
    let mut last_redis_gap_check = tokio::time::Instant::now() - redis_gap_check_interval;

    loop {
        // 1. stanox_crs reload.
        if last_stanox_crs_reload.elapsed() >= stanox_crs_reload_interval {
            match queries::fetch_stanox_crs(&http, &config.stanox_crs_url, &internal_oauth).await {
                Ok(records) if !records.is_empty() => {
                    *stanox.write().expect("stanox lock poisoned") =
                        stanox_crs::StanoxCrsTable::from_records(records);
                }
                Ok(_) => {
                    tracing::warn!("live stanox_crs table is empty; keeping the currently loaded table");
                }
                Err(err) => {
                    tracing::error!(error = ?err, "failed to reload stanox_crs table; keeping previous snapshot");
                    metrics::counter!(
                        common::metrics::metric_name("trust_backlog_consumer_errors_total"),
                        "operation" => "reload_stanox_crs"
                    )
                    .increment(1);
                }
            }
            last_stanox_crs_reload = tokio::time::Instant::now();
        }

        // 2. redis-stream gap check.
        if last_redis_gap_check.elapsed() >= redis_gap_check_interval {
            match feed.check_gap().await {
                Ok(Some(gap)) => {
                    tracing::error!(
                        last_delivered = %gap.group_last_delivered_id,
                        new_first_entry = %gap.stream_first_entry_id,
                        "movement-events stream gap detected: some events between these IDs were \
                         trimmed before trust-backlog-consumer ever read them"
                    );
                    metrics::counter!(common::metrics::metric_name(
                        "trust_backlog_consumer_stream_gap_detected_total"
                    ))
                    .increment(1);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = ?err, "failed to check movement-events stream for a gap");
                }
            }
            last_redis_gap_check = tokio::time::Instant::now();
        }

        // 3. consume + filter + POST.
        let cycle_start = std::time::Instant::now();
        match feed.next_batch().await {
            Ok(batch) => {
                let today = current_rail_day(chrono::Utc::now());
                let snapshot = stanox.read().expect("stanox lock poisoned").clone();
                let mut events = Vec::new();
                for raw in &batch {
                    match trust_schema::schema::parse_batch(raw) {
                        Ok(messages) => {
                            for message in messages {
                                if let Some(event) = process::process_message(
                                    &message,
                                    &mut process_state,
                                    &snapshot,
                                    &crs_index,
                                    today,
                                ) {
                                    events.push(event);
                                }
                            }
                        }
                        Err(err) => {
                            tracing::error!(error = ?err, raw = %raw, "failed to parse TRUST batch; dropping this payload");
                            metrics::counter!(
                                common::metrics::metric_name("trust_backlog_consumer_errors_total"),
                                "operation" => "parse_batch"
                            )
                            .increment(1);
                        }
                    }
                }

                if let Err(err) =
                    queries::post_trust_event_backlog(&http, &config.api_ingest_url, &internal_oauth, &events)
                        .await
                {
                    tracing::error!(error = ?err, "failed to post trust-event-backlog batch; will retry next cycle");
                    metrics::counter!(
                        common::metrics::metric_name("trust_backlog_consumer_errors_total"),
                        "operation" => "post_batch"
                    )
                    .increment(1);
                    // Deliberately does NOT commit on a failed post -- same
                    // "only ack after a successful downstream write"
                    // posture as trust-consumer's own main loop, since this
                    // consumer's whole reason to exist is not losing events
                    // a late-tracking pin might need.
                    tokio::time::sleep(ERROR_BACKOFF).await;
                    continue;
                }
                metrics::counter!(common::metrics::metric_name(
                    "trust_backlog_consumer_events_stored_total"
                ))
                .increment(events.len() as u64);

                if let Err(err) = feed.commit().await {
                    tracing::error!(error = ?err, "failed to commit Redis Streams offsets");
                    metrics::counter!(
                        common::metrics::metric_name("trust_backlog_consumer_errors_total"),
                        "operation" => "commit_offsets"
                    )
                    .increment(1);
                }
            }
            Err(err) => {
                tracing::error!(error = ?err, "error receiving from movement feed");
                metrics::counter!(
                    common::metrics::metric_name("trust_backlog_consumer_errors_total"),
                    "operation" => "movement_feed_receive"
                )
                .increment(1);
                tokio::time::sleep(ERROR_BACKOFF).await;
            }
        }
        metrics::histogram!(common::metrics::metric_name(
            "trust_backlog_consumer_cycle_duration_seconds"
        ))
        .record(cycle_start.elapsed().as_secs_f64());
    }
}

const ERROR_BACKOFF: Duration = Duration::from_secs(2);

/// The Europe/London rail day `at` falls on -- the calendar date the
/// process.rs/migration doc comments already promise ("falls back to the
/// current Europe/London rail day"), NOT a bare UTC calendar date. An
/// earlier draft of this main loop used `chrono::Utc::now().date_naive()`
/// directly, which is plain UTC and ignores both the Europe/London
/// timezone offset AND this codebase's own established 02:00 rail-day
/// cutoff convention (`common::rail_day::next_rail_day_boundary`,
/// extracted specifically so more than one crate could share this exact
/// DST-transition-safe logic) -- caught during this plan's second review
/// pass. This function is the inverse of `next_rail_day_boundary`: a
/// small, crate-local, pure duplication of the same "before/after local
/// 02:00" check, not a call into `common::rail_day` itself, because that
/// module only exposes the *next boundary*, not *which rail day `at`
/// currently falls in* -- adding the latter to `common::rail_day` instead
/// is a reasonable follow-up, but out of scope for this plan to also
/// change a shared crate's public surface.
fn current_rail_day(at: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDate {
    let local = at.with_timezone(&chrono_tz::Europe::London);
    let cutoff = chrono::NaiveTime::from_hms_opt(2, 0, 0).expect("2:00:00 is a valid time");
    if local.time() < cutoff {
        local.date_naive() - chrono::Duration::days(1)
    } else {
        local.date_naive()
    }
}

#[cfg(test)]
mod rail_day_tests {
    use super::*;

    #[test]
    fn well_after_the_0200_cutoff_is_that_calendar_days_rail_day() {
        let at: chrono::DateTime<chrono::Utc> = "2026-09-05T13:00:00Z".parse().unwrap();
        assert_eq!(
            current_rail_day(at),
            "2026-09-05".parse::<chrono::NaiveDate>().unwrap()
        );
    }

    #[test]
    fn just_before_the_0200_cutoff_is_still_the_previous_calendar_days_rail_day() {
        // 2026-09-05T01:30:00Z is 02:30 BST (September is daylight saving) --
        // wait, that's AFTER 02:00 local, so pick a UTC time that's clearly
        // before 02:00 Europe/London instead: 00:30 UTC = 01:30 BST.
        let at: chrono::DateTime<chrono::Utc> = "2026-09-05T00:30:00Z".parse().unwrap();
        assert_eq!(
            current_rail_day(at),
            "2026-09-04".parse::<chrono::NaiveDate>().unwrap()
        );
    }
}
```

**Confirmed directly, not guessed**: `full-coverage-consumer/src/main.rs`'s
own `RedisStreamMovementFeed::connect(...)` call takes exactly the same 4
arguments as the sketch above (`redis_url`, `group`, `consumer`,
`autoclaim_min_idle`) — `connection_state` is NOT an extra argument to
`connect()` itself. An earlier draft of this task claimed otherwise (a
misreading of `full-coverage-consumer/src/main.rs`'s own
`ActiveFeed::RedisStream(Box::new(RedisStreamMovementFeed::connect(...).await?),
connection_state, "full_coverage_consumer_ready")` construction, where
those two extra values are fields of the `ActiveFeed::RedisStream` enum
variant *wrapping* the already-connected feed, not arguments passed into
`connect()`) — caught during this plan's second review pass; see
`crates/movement-feed/src/active_feed.rs` for the real shape, already
used correctly above.

- [ ] **Step 3: Build**

```bash
cargo build -p trust-backlog-consumer
```

Expected: compiles clean (after the connection-state wiring fix from the
note above is applied).

- [ ] **Step 4: Integration-shaped test against `FakeMovementFeed`**

Add to `main.rs`'s own `#[cfg(test)] mod tests`, mirroring
`full-coverage-consumer/src/main.rs`'s own
`an_activation_and_movement_batch_produces_the_expected_line_and_station_stats`
test shape — this crate's loop body isn't directly unit-testable (it's
`main`'s own top-level `loop`), so this test exercises `process::process_message`
end-to-end over a small batch instead, which is the meaningful unit of
behavior `main`'s loop just glues together:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_with_a_pass_and_a_departure_keeps_only_the_departure() {
        let activation_and_movements = r#"[
            {"header":{"msg_type":"0003"},"body":{
                "train_id":"221832406","event_type":"PASS",
                "planned_timestamp":"1787941920000","actual_timestamp":"1787941920000",
                "loc_stanox":"87212","variation_status":"ON TIME"
            }},
            {"header":{"msg_type":"0003"},"body":{
                "train_id":"221832406","event_type":"DEPARTURE",
                "planned_timestamp":"1787942000000","actual_timestamp":"1787942000000",
                "loc_stanox":"87212","variation_status":"ON TIME"
            }}
        ]"#;
        let messages = trust_schema::schema::parse_batch(activation_and_movements).unwrap();

        let stanox = stanox_crs::StanoxCrsTable::from_records(vec![common::StanoxCrsRecord {
            stanox: "87212".to_string(),
            crs: "WAT".to_string(),
            tiploc: "WATRLMN".to_string(),
            station_name: "LONDON WATERLOO".to_string(),
            source_sequence: 1,
        }]);
        let crs_index: std::collections::HashSet<String> = ["WAT".to_string()].into_iter().collect();
        let mut state = process::ProcessorState::default();
        let today: chrono::NaiveDate = "2026-09-05".parse().unwrap();

        let events: Vec<_> = messages
            .iter()
            .filter_map(|m| process::process_message(m, &mut state, &stanox, &crs_index, today))
            .collect();

        assert_eq!(events.len(), 1, "the PASS event must be dropped, only the DEPARTURE kept");
        assert_eq!(events[0].event_type, Some("DEPARTURE".to_string()));
    }
}
```

```bash
cargo test -p trust-backlog-consumer
```

Expected: this test and every test from Tasks 8-9 pass.

- [ ] **Step 5: Commit**

```bash
git add crates/trust-backlog-consumer/src/main.rs crates/trust-backlog-consumer/src/queries.rs
git commit -m "Wire trust-backlog-consumer's main loop: third movement-events consumer group"
```

---

## Task 11: Verify the third consumer group is genuinely safe — not assumed

**Files:**
- Modify: `crates/movement-feed/src/redis_stream.rs`

**Why this task exists, stated plainly**: this plan's own Global
Constraints require proving, not assuming, that a third consumer group
never competes with or duplicate-drains entries `trust-consumer`/
`full-coverage-consumer` already read. `redis_stream.rs`'s own
`ensure_group`/`XREADGROUP` mechanics (read directly in this plan's own
research) already strongly imply this — Redis Streams consumer groups
each maintain their own independent `last-delivered-id` and
pending-entries list, so `XREADGROUP` for one group never consumes what
another group would also read — but this task adds a **direct,
real-Redis test** proving it, rather than leaving it as an inference from
reading the client library's own semantics.

**Interfaces:**
- Consumes: `RedisStreamMovementFeed::connect_for_test` (already exists,
  test-only).
- Produces: one new `#[ignore]`d integration test in
  `redis_stream.rs`'s own `redis_tests` module.

- [ ] **Step 1: Update `connect`'s own stale doc comment**

`RedisStreamMovementFeed::connect`'s doc comment currently reads `group`
is one of "the two fixed literals (`"trust-consumer"` /
`"full-coverage-consumer"`)". This plan adds a real, legitimate third —
update that comment (in this same file) to say "one of a small number of
fixed literals (`"trust-consumer"` / `"full-coverage-consumer"` /
`"trust-event-backlog"`, one per consumer crate)" so it doesn't read as
a stale, now-false constraint to the next person who adds a fourth.

- [ ] **Step 2: Write the test**

Add to `crates/movement-feed/src/redis_stream.rs`'s existing
`#[cfg(test)] mod redis_tests`:

```rust
#[tokio::test]
#[ignore = "needs REDIS_URL"]
async fn three_independent_consumer_groups_each_receive_every_entry_independently() {
    let stream = unique_stream("three-groups");

    // Mirrors this plan's real deployment shape: trust-consumer,
    // full-coverage-consumer, and trust-backlog-consumer are three
    // independent named groups on the SAME stream.
    let mut trust_consumer = RedisStreamMovementFeed::connect_for_test(
        &redis_url(),
        &stream,
        "trust-consumer",
        "trust-consumer-1",
        Duration::from_secs(3600),
    )
    .await
    .unwrap();
    let mut full_coverage_consumer = RedisStreamMovementFeed::connect_for_test(
        &redis_url(),
        &stream,
        "full-coverage-consumer",
        "full-coverage-consumer-1",
        Duration::from_secs(3600),
    )
    .await
    .unwrap();
    let mut trust_backlog_consumer = RedisStreamMovementFeed::connect_for_test(
        &redis_url(),
        &stream,
        "trust-event-backlog",
        "trust-event-backlog-1",
        Duration::from_secs(3600),
    )
    .await
    .unwrap();

    xadd(&stream, "payload-1").await;

    // Each group's own startup PEL-replay pass is legitimately empty
    // first (see this module's own doc comment on every other test here),
    // then the SAME entry is delivered to all three independently.
    trust_consumer.next_batch().await.unwrap();
    full_coverage_consumer.next_batch().await.unwrap();
    trust_backlog_consumer.next_batch().await.unwrap();

    let a = trust_consumer.next_batch().await.unwrap();
    let b = full_coverage_consumer.next_batch().await.unwrap();
    let c = trust_backlog_consumer.next_batch().await.unwrap();

    assert_eq!(a, vec!["payload-1".to_string()], "trust-consumer must see the entry");
    assert_eq!(b, vec!["payload-1".to_string()], "full-coverage-consumer must ALSO see the same entry");
    assert_eq!(c, vec!["payload-1".to_string()], "trust-backlog-consumer must ALSO see the same entry -- proving the third group does not steal it from, or split it with, the other two");

    // Each group acks independently -- one group's XACK must not affect
    // another's own pending-entries list.
    trust_consumer.commit().await.unwrap();
    let pending_full_coverage: redis::streams::StreamPendingCountReply = full_coverage_consumer
        .conn
        .xpending_count(&stream, "full-coverage-consumer", "-", "+", 10)
        .await
        .unwrap();
    assert_eq!(
        pending_full_coverage.ids.len(),
        1,
        "full-coverage-consumer's own pending entry must be unaffected by trust-consumer's ack"
    );

    cleanup(&stream).await;
}
```

- [ ] **Step 3: Run against a real Redis**

```bash
REDIS_URL=redis://localhost:6379 cargo test -p movement-feed three_independent_consumer_groups -- --ignored
```

Expected: pass — direct, real-Redis proof (not an inference) that the
third consumer group added by this plan is safe.

- [ ] **Step 4: Commit**

```bash
git add crates/movement-feed/src/redis_stream.rs
git commit -m "Add a real-Redis test proving three independent consumer groups never compete for entries"
```

---

## Task 12: Deployment — Dockerfile, Helm chart, CI image matrix, compose

**Files:**
- Create: `docker/trust-backlog-consumer.Dockerfile`
- Create: `charts/distant-signal/templates/trust-backlog-consumer-deployment.yaml`
- Modify: `charts/distant-signal/values.yaml`
- Modify: `.github/workflows/containers.yml`
- Modify: `docker-compose.yml`
- Modify: `.github/workflows/ci.yml`

Depends on Task 10 (the crate must build).

- [ ] **Step 1: Dockerfile**

`docker/trust-backlog-consumer.Dockerfile`, structurally identical to
`docker/full-coverage-consumer.Dockerfile` (this crate has no `rdkafka`
dependency at all — no Kafka backend, per Task 7's own decision — so
this Dockerfile does NOT need `cmake`/`libsasl2-dev`'s `sasl2-sys` build
step; confirm by checking whether `full-coverage-consumer`'s own
Dockerfile needs those for its OWN (still-present) Kafka-backend
support, and if so, DROP them here — same `rust:1.88-bookworm` base,
same non-root runtime user, `cargo build --bin trust-backlog-consumer`).
No reference-data COPY step needed for `lines/*.toml` (mounted as a
ConfigMap volume at deploy time, same as `aggregator`/
`full-coverage-consumer`) but DOES need the `reference-data/stanox-crs.csv`
COPY step `trust-consumer`'s own Dockerfile has (this crate's
`--stanox-crs-file` default is `/app/reference-data/stanox-crs.csv`,
same as `trust-consumer`'s).

- [ ] **Step 2: Helm Deployment**

`charts/distant-signal/templates/trust-backlog-consumer-deployment.yaml`,
copied structurally from `full-coverage-consumer-deployment.yaml`:

- `replicas: 1` (single-consumer-group-per-deployment, same reasoning as
  every other consumer in this chart).
- `readinessProbe`/`livenessProbe` against `/healthz`, same shape.
- No Kafka connection block at all (unlike `full-coverage-consumer`'s
  Deployment, which reuses `trustConsumer.kafka.*`) — this crate only
  ever speaks Redis. Env vars: `REDIS_URL` from
  `.Values.redis.url`-equivalent (match whatever existing Deployment
  already sources Redis connection info from — `movement-relay`'s own
  Deployment is the closest precedent, check
  `charts/distant-signal/templates/movement-relay-deployment.yaml` if it
  exists, or `redis.enabled`'s own values.yaml convention).
- `API_INGEST_URL` built from `distant-signal.apiBaseUrl` +
  `/private/trust-event-backlog`; `STANOX_CRS_URL` similarly.
- `INTERNAL_OAUTH_*` from a **new**, distinct
  `trustBacklogConsumerSecretName`/OAuth-username/password secret-key-ref
  pair (own Authentik service-account credential, mirroring
  `fullCoverageConsumerSecretName`'s own `_helpers.tpl` pattern — see
  `charts/distant-signal/files/devauthentik-blueprints/` for whether a
  new OAuth2 client blueprint entry is also needed there, following
  whatever entry exists for `full-coverage-consumer`).
- `LINES_DIR` + volume/volumeMount for the `lines/*.toml` ConfigMap,
  copied from `aggregator-deployment.yaml`'s own block (same as
  `full-coverage-consumer-deployment.yaml`'s own Step 2 note already
  established for that Deployment).
- `STANOX_CRS_FILE` baked-in default path — confirm the Dockerfile's own
  COPY destination matches.
- `HEALTH_BIND_URL`, `METRICS_PORT`, `RUST_LOG`.

- [ ] **Step 3: `values.yaml`**

New `trustBacklogConsumer:` top-level block: `image`, `healthPort`,
`metricsPort`, `logLevel`, `retentionDaysWarningAcknowledged: false` (a
values-level flag purely for documentation/discoverability — **this
plan does NOT wire this flag to any actual gating logic**; the real
safety mechanism is Task 6's `trust_event_backlog_retention_days`
config field living on `aggregator`, not on this chart block at all —
this flag exists only so a human editing `values.yaml` sees a clearly
named place documenting that a real decision is required, pointing in
a comment at `aggregator.trustEventBacklogRetentionDays`, wherever that
value actually lives), plus the usual `resources`/`nodeSelector`/
`tolerations`/`affinity`/`podAnnotations`/`podSecurityContext`/`extraEnv`
tail every other service block already has. Add
`aggregator.trustEventBacklogRetentionDays: 1` to `aggregator:`'s own
existing block, with a comment pointing at Task 6's own doc comment on
`Config::trust_event_backlog_retention_days` verbatim.

- [ ] **Step 4: CI + compose**

`.github/workflows/containers.yml`'s matrix:

```yaml
- service: trust-backlog-consumer
  dockerfile: docker/trust-backlog-consumer.Dockerfile
  target: ""
```

`docker-compose.yml`: a `trust-backlog-consumer` service block, copied
structurally from the existing `full-coverage-consumer` block (no Kafka
env vars needed, per Step 1's own note).

- [ ] **Step 5: Update `ci.yml`'s ignored-test invocation**

Task 11 added a new `#[ignore]`d Redis-gated test to `movement-feed` —
already covered by the existing `cargo test -p movement-feed --
--ignored --test-threads=1` line, no change needed there. If Task 9/10
end up adding any `#[ignore]`d tests directly inside
`trust-backlog-consumer` itself (this plan's own tasks deliberately kept
all of that crate's tests non-`#[ignore]`d — confirm with
`grep -rn "#\[ignore" crates/trust-backlog-consumer/src/` before this
step), add `-p trust-backlog-consumer` to whichever ignored-test
invocation line in `.github/workflows/ci.yml`'s `rust-test` job
corresponds (the `-p api -p aggregator` line for Postgres-gated tests,
or a new Redis-gated line alongside `-p movement-feed`, depending on
which resource that test needs).

- [ ] **Step 6: Verify and commit**

```bash
helm template charts/distant-signal > /dev/null
docker compose config > /dev/null
```

```bash
git add docker/trust-backlog-consumer.Dockerfile \
        charts/distant-signal/templates/trust-backlog-consumer-deployment.yaml \
        charts/distant-signal/values.yaml .github/workflows/containers.yml \
        docker-compose.yml .github/workflows/ci.yml
git commit -m "Deploy trust-backlog-consumer: Helm Deployment, Dockerfile, CI image matrix, compose service"
```

---

## Task 13: Final verification

- [ ] **Step 1: Full workspace verification, matching CI's own real invocations**

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-features --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres REDIS_URL=redis://localhost:6379 \
  cargo test -p api -p aggregator -- --ignored --test-threads=1
REDIS_URL=redis://localhost:6379 cargo test -p movement-feed -- --ignored --test-threads=1
```

Expected: everything passes, including the two new DB-gated backlog
tests (Tasks 4/5/6) and the new Redis-gated three-consumer-groups test
(Task 11).

- [ ] **Step 2: Confirm the retention safeguard, by inspection, not by trust**

```bash
grep -n "default_value_t = 1" crates/aggregator/src/config.rs | grep -i trust_event_backlog
grep -n "retention is configured above" crates/aggregator/src/main.rs
grep -n "trustEventBacklogRetentionDays" charts/distant-signal/values.yaml
```

Expected: all three match — the default is really 1, the warning is
really wired into `main.rs`, and the Helm value defaults to 1 too (a
Helm `values.yaml` default of 7, silently overriding the Rust-level
default of 1, would defeat the entire safeguard — this check exists to
catch exactly that class of mistake).

- [ ] **Step 3: Confirm no raw payload is ever stored**

```bash
grep -n "raw_body\|JSONB" crates/api/migrations/20260905160000_trust_event_backlog.sql
```

Expected: **zero matches** — confirms this plan's own Global Constraint
("no raw TRUST payload is ever stored") held through implementation, not
just in this plan's prose.

- [ ] **Step 4: Confirm PASS exclusion is enforced at the database level, not just in application code**

```bash
psql "$DATABASE_URL" -c "\d trust_event_backlog" | grep -i "event_type"
```

Expected: the printed `CHECK` constraint text includes exactly
`'ARRIVAL'::text, 'DEPARTURE'::text` and nothing else — proving `PASS`
cannot land in this table even if a future code change forgets Task 9's
own application-level filter.

- [ ] **Step 5: Confirm the new consumer group is independent (Task 11's own proof), one more time, in context**

```bash
REDIS_URL=redis://localhost:6379 cargo test -p movement-feed three_independent_consumer_groups_each_receive_every_entry_independently -- --ignored
```

Expected: pass.

- [ ] **Step 6: Confirm the schedule-first independence claim holds — no accidental new dependency crept in**

```bash
grep -n "worktree-schedule-first-plan\|schedule_matched\|schedule_query" crates/trust-backlog-consumer/src/*.rs crates/api/src/data/trust_event_backlog*.rs
```

Expected: no real code dependency — only the doc-comment citations this
plan itself wrote pointing at that other plan for context. If a real
`use schedule_query::...` or a query against a `schedule_matched`-only
column appears here, this plan's own "zero ordering dependency" claim
has been violated during implementation — stop and reconcile before
merging.
