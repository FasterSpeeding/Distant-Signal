# Design: Option B's Live Consumer, in Shadow Mode

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass.** Gated by an explicit repo-owner override of
`docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md`'s "stays
gated on validation reaching 'go'" verdict: this document designs a real,
live, production-connected TRUST/schedule correlation consumer *now*, ahead
of that "go," under one binding condition — it lands in **shadow mode**,
computing and persisting real per-line stats but never affecting a real
line's severity or `DataQuality`, because `LineDefinition.full_coverage_enabled`
stays `false` for every catalogued line (`crates/common/src/lib.rs:498`,
`lines/*.toml`, confirmed unset everywhere). That condition is this
document's scope boundary, not a suggestion weighed against others.

Written to the same rigor as
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
("the base spec") and
`docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the scaffolding spec") — both required reading, consumed in full before
this document was written, alongside
`docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md` ("the
scoping doc"),
`docs/superpowers/plans/2026-09-03-option-b-consumer-first-slice-plan.md`,
`crates/schedule-query/src/{lib,resolve}.rs`, and every file under
`crates/trust-consumer/src/`.

## What this document assumes as settled, not re-litigated

- **The base spec's own architecture recommendation** — "Option B, if this
  proceeds at all... a new dedicated consumer, wholly separate from
  `trust-consumer`" (base spec, "Recommendation among the three") — is
  affirmed below with a concrete, code-grounded justification (Decision 1),
  not re-derived from first principles.
- **`crates/schedule-query`** (merged, real, tested against the full
  463,947-record production extract per the first-slice plan's Open
  Question 2 resolution) is the schedule-resolution library this consumer
  builds on. This document does not re-verify its correctness and does not
  modify it.
- **The full-coverage presentation scaffolding** (`FullCoverageAvailability`,
  `LineStatus.full_coverage_stats`/`full_coverage_availability`,
  `LineDefinition.full_coverage_enabled`, `aggregation::merge_full_coverage`/
  `merge_full_coverage_stats`/`escalate_from_coverage_stats`) is real, merged,
  and its shape is **not renegotiated here** — this document designs a real
  producer for its one remaining placeholder, not a replacement for it.

## Relationship to the concurrent per-station full-coverage design

**Revision (2026-09-04, post-initial-publish)**: this document originally
listed "per-station full-coverage stats" as out of scope, deferring to a
concurrent workstream and flagging the resulting coupling as an open risk
(the original Open Question 7). That workstream has since landed
`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`
("the per-station doc," in `worktree-per-station-full-coverage`), which
needed a concrete producer contract from this document and, in its absence,
built its own best-grounded guess (its Decision 2): a table
`station_full_coverage_samples(crs, operator, resolved_at, stats)`, written
by "Option B's future consumer" to a new `POST
/private/station-full-coverage-samples`, read via a `latest_station_full_coverage_samples(pool, crs)`
query. **This revision resolves that gap by extending this consumer's own
design to be exactly that producer**, converging on the per-station doc's
own table/endpoint naming rather than inventing a second, competing shape
— see the new Decision 2h and the station-level half of Decision 3 below.
This is a real, non-trivial addition to this consumer's surface area, not
a free extension — see the cost note at the end of Decision 2h.

## Current relevant state (verified against code, not re-derived)

**The placeholder this document exists to replace**, confirmed directly:
`crates/aggregator/src/main.rs:192`

```rust
aggregation::merge_full_coverage(&mut reports, &lines, &HashMap::new(), defaults);
```

`merge_full_coverage` (`crates/aggregator/src/aggregation.rs:1222`) already
does everything the scaffolding spec designed: for every line with
`full_coverage_enabled`, it looks up `full_coverage.get(&line.id)`; `Some`
merges via `merge_full_coverage_stats` (severity/`DataQuality::TrustInferred`
when no incident is present, escalate-only otherwise, per
`aggregation.rs:1177`); `None` sets `FullCoverageAvailability::Pending`.
**This is the one, single integration point this document's consumer must
feed** — nothing else in `aggregator`, `api`, or the frontend needs to
change.

**`trust-consumer` is real, currently-deployed code** (`charts/distant-signal/templates/trust-consumer-deployment.yaml`),
a persistent single-replica Kafka consumer group
(`kafka_consumer_group`, default `distant-signal-trust-consumer`,
`crates/trust-consumer/src/config.rs:36`) against the full, unfiltered
national Train Movements topic (`consumer.subscribe(&[&config.kafka_topic])`,
`crates/trust-consumer/src/feed/kafka.rs:62` — Kafka has no server-side
content filter, confirmed by the base spec and re-confirmed here). Its
matching is built entirely around a small, explicitly one-shot, opt-in
pinned-train set:

- `matching::resolve_origin_departure` (`crates/trust-consumer/src/matching.rs:1-7`,
  module doc, quoted in full in the scoping doc) is "best-effort resolution
  of a user's pin... a heuristic, not a guaranteed join," and its own doc
  states plainly this app "has no CIF schedule lookup to bridge Activation's
  `train_uid` to a departure time" — the exact gap `schedule-query` closes,
  but not wired into this module by this document (see Explicitly out of
  scope).
- `process::ProcessorState.resolved` (`crates/trust-consumer/src/process.rs:151`)
  is a `train_id -> tracked_train_id` map with **no unwind path** — "a claim
  is one-way" (`process.rs:376-379`'s own comment) — structurally a
  small-set, irreversible-claim design, not a population-membership test.
- `config.rs:34-37`'s own doc comment: "Fixed per deployment, not per-process
  -- multiple trust-consumer replicas sharing one group would each get a
  subset of partitions, which is fine for horizontal scaling but NOT this
  plan's v1 (single replica)." This crate's correctness today depends on
  running as exactly one replica against its one consumer group.
- `health.rs`'s connected/disconnected liveness semantics, `dedup.rs`'s
  `dedup_key` at-least-once idempotency, and the whole
  consume-post-commit-never-on-failure discipline in `main.rs::run_cycle`
  (`crates/trust-consumer/src/main.rs:159-191`) are real correctness
  properties of a feature that ships and is used today.

**`common::compute_sample_stats`** (`crates/common/src/lib.rs:1131-1168`)
is explicitly source-agnostic: its own doc comment states it's "the shared
delayed/cancelled/skipped/avg-delay arithmetic underlying every `SampleStats`
computation in this app," parameterized by an `is_skip` predicate precisely
because different callers mean different things by "skip" (line-level:
skips a stop anywhere on the line's route; per-station: skips this specific
station). It operates on `&[&StationDeparture]`
(`crates/common/src/lib.rs:404-427`) — an LDBWS-shaped struct
(`service_id`, `operator`, `destination_crs`, `scheduled`/`estimated`
strings, `is_cancelled`, `delay_minutes`, `skipped_stations`). Nothing
about its arithmetic depends on LDBWS as the *source* of that struct — it
is reused below by synthesizing one `StationDeparture` per matched
scheduled service (Decision 2).

**`common::StanoxCrsRecord`** (`crates/common/src/lib.rs:721-730`), the wire
type `/private/stanox-crs` already serves, already carries `tiploc`
alongside `stanox`/`crs`. `trust-consumer`'s own local `StanoxCrsTable`
(`crates/trust-consumer/src/stanox_crs.rs:56-65`), however, only retains
`stanox`/`crs` in its private `StanoxCrsRecord` struct — it drops `tiploc`
on load, because `trust-consumer` has never needed it. This is a real,
concrete implication for Decision 2 below: the new consumer needs its own
STANOX→TIPLOC table (trivially derived from the same already-live
`/private/stanox-crs` feed, keeping the field `trust-consumer`'s table
throws away), not a reuse of `trust-consumer`'s existing table type.

**CIF SCHEDULE's own file-push ingestion is no longer a hypothetical, new
operational commitment** — the base spec (2026-08-29) flagged standing up
SFTP/cloud-storage-push ingestion as a real, unbuilt cost; it has since
been built, deployed (opt-in, `scheduleFeed.enabled: false` by default,
`charts/distant-signal/values.yaml:830`), and validated against real
production data (per the first-slice plan's Open Question 2 resolution).
`schedule-ingest` and `schedule-reference` run as two of three sibling
containers in one `schedulefeed` Pod, sharing one **`ReadWriteOnce`**
PersistentVolumeClaim (`charts/distant-signal/templates/schedulefeed-pvc.yaml`,
`values.yaml:822-824`: "Renders ONE Deployment with TWO containers... sharing
one ReadWriteOnce PVC -- see the design doc's 'The reader/writer problem
push introduces' for why this is one Pod, not two"). **This is load-bearing
for Decision 2**: a `ReadWriteOnce` PVC cannot be safely mounted read-only
by a *separate* Deployment's pod on demand — the existing precedent this
repo already committed to for exactly this reason is co-locating every
reader of the local extract as a sibling container in the same Pod, not a
second Deployment reaching for the same volume.

## Decisions

### 1. Consumer architecture: a new crate, `crates/full-coverage-consumer`, with its own Kafka consumer group

**A new dedicated crate, not an extension of `trust-consumer`.** This
affirms the base spec's own recommendation (Option B over Option A/C) with
a concrete justification grounded in the code read above, not just
citing the prior doc's conclusion:

- **Structurally different consumption pattern.** `trust-consumer`'s
  matching (`matching::resolve_origin_departure`, `ProcessorState.resolved`)
  is built around "does this `train_id` belong to a small, explicitly
  pinned, one-shot-claimable set" — a lookup against at most a few dozen
  live entries. Full-coverage correlation needs "does this event's location
  fall on any shadow-computed line's TIPLOC set, and does this `train_id`'s
  `train_uid` belong to that line's *entire scheduled population* for
  today" — a reverse TIPLOC→line index and a per-line population map, an
  order of magnitude larger and continuously changing at a different cadence
  (once per rail day, not once per pin). Retrofitting this onto
  `ProcessorState`'s one-way-claim design would mean either forking that
  struct's semantics in place (risking the pinned-train feature's own
  correctness) or adding a second, parallel state shape to the same
  process — which is Option C's shape, already rejected by the base spec
  for carrying "the largest blast radius" while gaining none of Option B's
  isolation.
- **Blast radius on a shipping feature.** `trust-consumer` is real,
  deployed, single-replica-constrained code with its own at-least-once
  delivery and dedup correctness requirements (`config.rs:34-37`,
  `dedup.rs`, `main.rs::run_cycle`'s never-commit-on-failure discipline). A
  bug or a resource-hungry code path in full-coverage correlation logic —
  new, unvalidated, exactly the kind of code the scoping doc says needs
  runtime proof — must not be able to degrade or crash individual train
  tracking by sharing its process, its consumer-group offset sequence, or
  its restart/backoff behavior.
- **Independent operational lifecycle.** Shadow mode's entire value
  proposition (per the binding condition) is running this correlation logic
  continuously, in production, long enough to accumulate a real comparison
  sample. That needs its own restart/scaling/health story, decoupled from
  whatever `trust-consumer`'s own release or incident cadence looks like.

**This crate gets its own, new, additional Kafka consumer group — a real
infra/ops cost, named plainly, not minimized.** Per the base spec's own
volume research (base spec §"Throughput at national-feed volume," ~630k
msgs/day, ~611k Movement), Kafka consumer groups are independent: a second
group on the same topic reads the **full stream again**, doubling this
app's ingest-side read volume from RDM's broker. This is the base spec's
own Option B cost, paid for real here, not deferred: a second persistent
broker connection, its own reconnect/offset-management operational surface,
and (per `config.rs:39-53`'s own GAP-flagged uncertainty about RDM's exact
Kafka product terms) a possible, **unconfirmed** question of whether RDM's
subscription terms meter or restrict multiple consumer groups against one
credential — flagged honestly in Open Questions, not asserted either way.
What is *not* doubled, and worth stating precisely so the cost isn't
overstated: SASL credentials authenticate a *connection*, not a group
membership in Kafka's protocol — the same broker/topic/SASL values
`trustConsumer.kafka.*` already configure can plausibly open a second,
differently-named consumer group without a second RDM subscription grant,
though this is inferred from Kafka's protocol, not independently confirmed
against RDM's specific product terms (see Open Questions).

**Shared TRUST envelope-parsing logic is extracted, not duplicated.**
`trust-consumer/src/schema.rs` (envelope/message parsing),
`dedup.rs` (`dedup_key` derivation), and `journey.rs`
(`apply_movement`/`apply_cancellation` state derivation, including the
delay-minutes-from-planned/actual arithmetic this consumer also needs) are
pure, already-tested, source-format concerns with no dependency on
`trust-consumer`'s own pin-matching semantics — the scoping doc's own
"partially reusable" finding (base spec §"Whether this can reuse
`trust-consumer`'s existing plumbing"). Duplicating ~300 lines of envelope
parsing across two Kafka consumers would violate this repo's own "one crate
per concern" convention (DESIGN.md §12) applied to *shared logic*, not just
services. **Decision, flagged as a prerequisite refactor for whichever
implementation plan follows this design, not performed here**: extract
`schema.rs`/`dedup.rs`/`journey.rs` into a new lib crate,
`crates/trust-schema`, consumed by both `trust-consumer` and
`full-coverage-consumer` as a workspace dependency. Pure code motion, no
behavior change — the same low-risk shape this repo already used for the
`schedule-ingest`/`schedule-reference` split.

```
crates/trust-schema/          # NEW — pure lib, extracted from trust-consumer
  src/
    schema.rs                 # moved verbatim (envelope/message parsing)
    dedup.rs                  # moved verbatim
    journey.rs                # moved verbatim (state derivation, delay calc)

crates/trust-consumer/        # unchanged behavior; depends on trust-schema
crates/full-coverage-consumer/  # NEW — this document's subject
  src/
    main.rs                   # Kafka consume loop, own consumer group
    config.rs                 # clap Config, mirrors trust-consumer's shape
    correlate.rs               # per-line population matching (Decision 2)
    station_correlate.rs       # per-(crs, operator) grouping (Decision 2h)
    population.rs              # in-memory per-line UID population, reload
    stanox_tiploc.rs           # STANOX->TIPLOC *and* STANOX->CRS table
                                # (keeps both fields, unlike
                                # trust-consumer's stanox_crs.rs -- Decision 2h)
    stats.rs                   # SampleStats synthesis via compute_sample_stats
    queries.rs                 # HTTP calls to api (population reload, both writes)
    health.rs                  # mirrors trust-consumer's health.rs verbatim
```

### 2. Correlation logic

**2a. Where the day's per-line scheduled population comes from.** Per the
ReadWriteOnce PVC finding above, `full-coverage-consumer` does **not**
mount the CIF extract directly, and does **not** run as a fourth container
in the `schedulefeed` Pod (that would tie a long-lived, always-on Kafka
consumer's lifecycle/restart semantics to a Pod built around two
periodic-poll, short-cycle containers — a worse fit than the alternative
below). Instead, `schedule-reference` gains a **second derived product**,
alongside its existing STANOX/CRS publication — the same kind of job
(read the local complete delivery, publish a derived reference product to
`api`), not a new concern:

```rust
// crates/schedule-reference/src/main.rs -- sketch, addition to poll_once.
// After the existing STANOX/CRS read+post:
let mca_text = read_prefixed_lines(&delivery.mca_path, "BS")?; // + BX/LO/LI/CR/LT,
                                                                 // schedule_query::parse handles grouping
let index = schedule_query::ScheduleIndex::from_text(&mca_text);
for line in &catalogue.lines_configured_for_shadow(&config) {  // Decision 4
    let today = rail_day_today();
    let population = schedule_query::schedules_touching(&index, &line.tiplocs(), today);
    queries::post_line_population(client, &config.line_population_url, &internal_oauth,
        line.id.clone(), today, population).await?;
}
```

This keeps `schedule-query` itself untouched (still pure, no I/O — Decision
1's crate boundary from the first-slice plan stays intact) and reuses the
already-proven local-file-read path `schedule-reference` already runs.
Trade-off named plainly: this is technically a second responsibility on a
crate whose name and existing doc comment describe one job (STANOX/CRS).
The alternative — a fifth crate/container/Deployment purely so
`schedule-reference` stays single-purpose — was judged disproportionate for
"one more read of an already-open local file, one more POST" against a
crate that already exists specifically to publish CIF-derived reference
products from this Pod. If this second responsibility grows (e.g. the
concurrent per-station full-coverage workstream also needs CIF-derived
publication), splitting it out into its own crate at that point is the
natural next step, not designed here.

**2b. Population read path.** `full-coverage-consumer` reloads each
shadow-computed line's today's-and-tomorrow's population periodically
(mirrors `trust-consumer::main.rs`'s `reference_reload_secs` pattern
exactly — both days kept, to avoid a gap at the rail-day rollover boundary,
see below), via a new `api` endpoint:

```
GET /private/schedule-line-population?line_id={id}&service_date={date}
  -> [{ uid: "C11052", calling_points: [{tiploc, kind, booked_arrival, booked_departure}, ...] }, ...]
```

held in memory as `HashMap<line_id, HashMap<service_date, HashMap<uid, Vec<CallingPoint>>>>`
— the direct analog of `trust-consumer::process::Reference.pending`, but
keyed by scheduled population membership rather than a pinned-train list.

**2c. STANOX→TIPLOC and event placement.** A new, small `stanox_tiploc.rs`
table (built from the same live `/private/stanox-crs` feed
`trust-consumer` already reloads, keeping the `tiploc` field its own table
drops) translates each Movement's `loc_stanox` to a TIPLOC. A reverse index
built once per population reload, `HashMap<tiploc, Vec<line_id>>` (from
every shadow-computed line's `lines/*.toml` `Station.tiploc`,
`crates/common/src/lib.rs:442-450`), places an event on every line it's
relevant to in O(1) — this is also why "compute every catalogued line"
(Decision 4) doesn't materially raise per-message correlation cost: the
reverse index makes matching-cost independent of line count for the common
case (an event's TIPLOC belongs to zero or one line for most locations,
occasionally more for shared segments).

**2d. Matching algorithm**, per event:

- **Activation (`0001`)**: park `train_id -> train_uid` exactly as
  `trust-consumer::process::ProcessorState.pending_activations` already
  does (reused via `trust-schema::journey`, Decision 1) — no line
  attribution yet, since Activation carries no location.
- **Movement (`0003`)**: translate `loc_stanox` -> TIPLOC -> candidate
  line(s) via the reverse index. For each candidate line, look up whether
  this event's `train_id`'s already-known (or freshly-claimed-via-Activation)
  `train_uid` is a member of that line's population for the event's
  service date. On a match, accumulate the event's derived delay/status
  (reusing `trust-schema::journey::apply_movement`'s existing
  planned-vs-actual arithmetic, Decision 1) against a per-`(line_id, uid)`
  running record — **not** a per-`(line_id, tiploc)` counter, since one UID
  can touch a line's TIPLOCs more than once (an intermediate stop and a
  later one) and must be counted once per line per day, mirroring
  `dedup.rs`'s per-service-identity dedup precedent
  (`crates/aggregator/src/dedup.rs`) rather than per-event.
- **Cancellation (`0002`)**: if the `train_id` resolves to a `train_uid` in
  any shadow-computed line's today's population, mark that `(line_id, uid)`
  cancelled for the day — the direct TRUST-side complement to a CIF
  STP=C cancellation (which never enters the population at all, since
  `schedule_query::schedules_touching` already filters `!resolved.cancelled`
  — see `crates/schedule-query/src/resolve.rs:88`).
- **A UID with zero observed events by the time its window closes** (below)
  is the genuinely new case full coverage adds over both LDBWS sampling and
  `trust-consumer`'s own opportunistic pin-matching: neither confirmed
  running nor explicitly cancelled. **Decision, flagged as a real accuracy
  risk, not a settled fact**: treat an unconfirmed-by-window-close UID as
  `cancelled` for stats purposes — the honest "TRUST never confirmed this
  ran" reading. This could over-count cancellations if the real cause is a
  gap in *this consumer's own* visibility (an untranslatable STANOX, a
  missed Activation) rather than a genuine non-running service — exactly
  the kind of correctness question shadow-mode's continuous production
  comparison against sampling exists to surface before any real line's
  `full_coverage_enabled` ever flips (see Open Questions).

**2e. Resolved vs. Pending per line per cycle.** `FullCoverageAvailability::Available`'s
own doc comment (`crates/common/src/lib.rs:838-843`) sets
the bar explicitly: "every scheduled service on this line for the current
window has been matched." This document defines **"current window" as one
rail day**, reusing this repo's own existing rail-day boundary convention
(`aggregation.rs:214-227`'s `next_rail_day_boundary`, a 02:00 Europe/London
cutoff already used for incident staleness) rather than inventing a new
one or reusing the *other* existing convention (`line_status_daily_stats`'s
plain midnight-to-midnight Europe/London calendar day,
`crates/api/migrations/20260831090001_*`'s own doc comment) — the rail-day
convention is the domain-correct one here because a late-running service
past midnight is still that calendar day's scheduled service, which is
exactly what incident staleness's rail-day boundary already encodes for a
different purpose. Per line, per day: **`Pending`** until the rail-day
boundary for that population's service date has passed; **`Available`**
once it has, with every UID's outcome (matched-and-derived, or
treated-cancelled per 2d) folded into that day's `SampleStats`. This is a
deliberately literal reading of the type's own contract, not an
approximation — it means a shadow-computed line stays `Pending` all day and
only flips `Available` once, near end-of-day, which is an honest
reflection of what "every scheduled service has been matched" can actually
mean, not a stretch to make the number feel more "live." Since this is
shadow mode with zero live-severity effect, there is no cost to staying
literal here; a partial-day, lower-confidence "escalate but never own the
determination" mode is a plausible future refinement, not designed here
(Open Questions).

**2f. `SampleStats` synthesis — reusing `compute_sample_stats` as invited.**
Per `compute_sample_stats`'s own doc comment (`crates/common/src/lib.rs:1131-1141`),
this consumer normalizes its per-`(line_id, uid)` outcome records into a
`Vec<StationDeparture>`, one synthetic entry per population UID for that
line/day:

```rust
// crates/full-coverage-consumer/src/stats.rs -- sketch, not final.
fn synthesize_departure(uid: &str, outcome: &UidOutcome) -> common::StationDeparture {
    common::StationDeparture {
        service_id: uid.to_string(),
        operator: String::new(),          // not tracked by this consumer
        destination_crs: String::new(),   // not needed for compute_sample_stats
        scheduled: String::new(),
        estimated: String::new(),
        is_cancelled: outcome.cancelled,   // real Cancellation, or unconfirmed-by-close (2d)
        delay_minutes: outcome.last_delay_minutes.unwrap_or(0),
        cancel_reason: None,
        delay_reason: None,
        headcode: None,
        skipped_stations: outcome.passed_without_stopping.clone(), // 2g
    }
}

let departures: Vec<&common::StationDeparture> = synthesized.iter().collect();
let stats = common::compute_sample_stats(&departures, defaults.delay_threshold_minutes, |d| {
    !d.skipped_stations.is_empty()
});
```

**2g. `skipped` mapping — an already-flagged open question, not resolved
here either.** The scaffolding spec's own Open Question 1 already names
this precisely: "whether `skipped`'s TRUST-side analog (a `PASS` movement
event) is actually the right mapping onto `SampleStats.skipped`'s existing
meaning is not confirmed... no code reads TRUST's event types for this
purpose yet." This document inherits that exact caveat rather than
resolving it: a `PASS` event at a TIPLOC the population's calling points
booked as an `Intermediate` stop is the plausible mapping, populated into
`outcome.passed_without_stopping`, but unverified against a real observed
case. Flagged again in Open Questions below, not newly discovered here.

**2h. Station-level aggregation — the producer contract the per-station
full-coverage workstream needs, resolved here rather than left as a
coupling risk.** Every matched Movement/Cancellation this consumer already
processes (2d) carries exactly what a per-`(crs, operator)` grouping needs,
already in hand, with no new correlation problem:

- **Station identity**: the event's own `loc_stanox`, already translated
  (2c) — but to **CRS**, not TIPLOC, for this purpose. The same live
  `/private/stanox-crs` feed this consumer already reloads for its
  STANOX→TIPLOC table (2c, `stanox_tiploc.rs`) carries `crs` on the
  identical `common::StanoxCrsRecord` row (`crates/common/src/lib.rs:721-730`)
  — `stanox_tiploc.rs` is extended to also keep a `stanox -> crs` map
  alongside `stanox -> tiploc`, one more field retained from a record this
  consumer already parses in full, not a second reload or a second feed.
- **Operator identity**: TRUST's own self-declared `toc_id` —
  `Activation.toc_id: String` (`crates/trust-consumer/src/schema.rs:41`)
  and `Movement.toc_id: Option<String>` (`schema.rs:66`) — exactly the
  field the per-station doc's own Decision 4 already names as the right
  source ("TRUST's own `Activation.toc_id`/`Movement.toc_id`... a future
  consumer does not need a CIF schedule join just to learn which operator
  ran a given train"). `schedule-query`'s `BasicSchedule`
  (`crates/schedule-query/src/records.rs`) does not decode a TOC field at
  all — confirmed by reading its full field list, `uid`/`stp_indicator`/
  `date_from`/`date_to`/`days_of_week` only, per the first-slice plan's own
  "no invented API details" discipline — so operator identity for station
  grouping is **necessarily** TRUST-sourced, not CIF-sourced, unlike line
  membership (which is purely CIF-driven, per 2a/2b). This asymmetry has a
  real, honest consequence, below.

Per matched Movement/Cancellation, this consumer accumulates a second,
parallel running record, keyed `(crs, toc_id)` (not `(line_id, uid)`),
alongside the existing per-line one — the two groupings share the same
event stream and the same `apply_movement`/`apply_cancellation`-derived
delay/cancellation facts (`trust-schema::journey`, Decision 1), so this is
one pass over the feed producing two outputs, not two passes.

**The population/"total" count is asymmetric between the two groupings —
stated plainly, not smoothed over.** A line's population (2a/2b) is known
purely from CIF ahead of time: every UID `schedule_query::schedules_touching`
returns for a line counts toward that line's `total`, whether or not TRUST
ever reports on it (an unconfirmed UID becomes a `cancelled` entry per 2d,
but it's still counted). A station-operator bucket has no such guarantee:
a population UID's scheduled calling points (from the same
`ResolvedSchedule.calling_points`, resolved TIPLOC→CRS the same way
Decision 2c resolves STANOX→TIPLOC) tell this consumer *which* `(crs,
uid)` pairs to expect, but **not** which operator to file that expectation
under — that mapping only exists once a real Activation for that UID's
`train_id` has actually been observed this rail day. **Decision**: a
population UID only contributes to a `(crs, operator)` bucket once its
`toc_id` has been learned from a real Activation; a UID whose Activation is
never observed by window close still correctly inflates its *line's*
`cancelled` count (2d, unchanged) but is **excluded** from every
station-level bucket entirely, rather than guessed into one. This is a
real, asymmetric limitation of the station-level output relative to the
line-level output — a "confirmed by TRUST" precondition the line-level
grouping doesn't need — flagged in Open Questions, not silently absorbed
into the numbers.

`SampleStats` synthesis for a `(crs, operator)` bucket reuses
`compute_sample_stats` exactly as 2f does for lines — one synthetic
`StationDeparture` per attributed UID, `is_skip` unresolved for the
identical reason 2g already names (TRUST `PASS`-to-`skipped` mapping,
unconfirmed).

**Resolved vs. Pending, at station grain, mirrors 2e exactly**: a `(crs,
operator)` bucket is `Pending` until the rail day closes for every line
whose population contributed a calling point at that CRS, `Available`
once it has — the same literal, once-per-day semantic, for the same
reasoning (honesty over liveness, zero cost while this stays shadow-only).

**Cost, stated plainly per the task brief's own instruction not to
downplay it**: this roughly doubles this consumer's own internal
bookkeeping (two running-record maps instead of one, keyed differently,
both fed off the same event stream), adds a second POST call per cycle
(2h's write path, Decision 3), and adds a second, genuinely new failure
mode this consumer didn't have before (a UID correctly counted at line
grain but silently dropped at station grain for lack of an observed
Activation — 2h's asymmetric-population finding above). None of this
changes Decision 1's Kafka-consumer-group cost or Decision 4's shadow-scope
answer — it is additional in-process work on data this consumer is already
consuming, not a second consumer or a second subscription — but it is real,
additional design and implementation surface, not a free byproduct of the
line-level design.

### 3. Persistence and the aggregator integration point

**A new table, owned by `api`, one row per line, upserted every cycle —
the direct replacement for `merge_full_coverage`'s empty
`&HashMap::new()`.**

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_full_coverage_line_stats.sql
-- sketch, not final. One row per line -- a live snapshot (mirrors
-- LineStatus's own "current state, not history" shape), not an append
-- log; line_status_daily_coverage_stats (already merged, scaffolding
-- spec Decision 4) remains the historical rollup this table is NOT a
-- substitute for.
CREATE TABLE full_coverage_line_stats (
    line_id             TEXT PRIMARY KEY,
    service_date        DATE             NOT NULL,
    availability        TEXT             NOT NULL, -- 'pending' | 'available'
    total                INT             NOT NULL DEFAULT 0,
    delayed              INT             NOT NULL DEFAULT 0,
    cancelled            INT             NOT NULL DEFAULT 0,
    skipped              INT             NOT NULL DEFAULT 0,
    avg_delay_minutes   DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ      NOT NULL DEFAULT now()
);
```

`service_date` lets a stale row (a consumer outage spanning a rail-day
rollover) be detected and treated as `Pending` on read rather than served
as a silently-aging `Available` snapshot — a real freshness guard, cheap to
add now (see Open Questions for what enforces it).

**Write path**: `full-coverage-consumer` upserts one row per line at the
end of every correlation cycle (`Pending` rows every cycle while the rail
day is open, one final `Available` row once it closes per Decision 2e),
via a new private endpoint mirroring `schedule-reference`'s existing
`/private/stanox-crs` POST shape exactly:

```
POST /private/full-coverage-stats
  [{ lineId, serviceDate, availability, total, delayed, cancelled, skipped, avgDelayMinutes }, ...]
```

**Read path**: `aggregator` gains a new query, mirroring
`queries::fetch_stanox_crs`'s exact shape
(`crates/trust-consumer/src/queries.rs`'s sibling in `aggregator`):

```rust
// crates/aggregator/src/queries.rs -- sketch, not final.
pub async fn fetch_full_coverage_stats(
    http: &reqwest::Client,
    url: &str,
    oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<HashMap<String, common::SampleStats>> {
    // GET /private/full-coverage-stats -> only rows with availability =
    // 'available' AND service_date == today are included; a stale or
    // still-pending row is simply absent from the map, which
    // merge_full_coverage already treats identically to "no signal yet"
    // (sets Pending) -- no new branch needed in aggregator for this.
}
```

Wired into `crates/aggregator/src/main.rs`'s existing call site, replacing
the literal placeholder cited above:

```rust
// crates/aggregator/src/main.rs -- sketch, replaces line 192's literal.
let full_coverage = queries::fetch_full_coverage_stats(&http, &config.full_coverage_stats_url, &internal_oauth)
    .await
    .unwrap_or_default(); // fail open to empty -- identical posture to
                           // stanox_crs's own fail-open reload (process::apply_stanox_crs_reload)
aggregation::merge_full_coverage(&mut reports, &lines, &full_coverage, defaults);
```

**Why this is safe even before Decision 4's shadow-scope answer**:
`merge_full_coverage`'s existing, already-merged per-line gate
(`crates/aggregator/src/aggregation.rs:1222`, "for every line with
`full_coverage_enabled`") means this real, populated map has **zero
effect** on any line whose `full_coverage_enabled` is `false` — which is
every real catalogued line, per the binding condition. This safety
property requires no new code in `aggregator`; it already shipped with the
scaffolding. Feeding it a real map instead of an empty one changes nothing
about which lines it's allowed to touch.

**A second, sibling table for Decision 2h's station-level output — adopting
the per-station doc's own schema/endpoint naming verbatim, not a
competing shape.** The per-station doc's Decision 2 already sketched this
table by analogy, absent this document; that sketch converges cleanly with
what Decision 2h actually produces, so this document adopts it directly
rather than inventing a second name for the same thing:

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_station_full_coverage_samples.sql
-- sketch, not final. Matches
-- docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
-- Decision 2's own sketch verbatim -- one row per (crs, operator),
-- wholesale-replaced per resolution cycle, same "live snapshot, not
-- history" posture as full_coverage_line_stats above and as
-- station_samples itself (the sample-stats sibling this mirrors).
CREATE TABLE station_full_coverage_samples (
    crs         CHAR(3)     NOT NULL,
    operator    TEXT        NOT NULL,
    resolved_at TIMESTAMPTZ NOT NULL,
    stats       JSONB       NOT NULL,
    PRIMARY KEY (crs, operator)
);
```

**Write path**: `full-coverage-consumer` POSTs this table's rows at the end
of every correlation cycle, alongside (not instead of) the line-level POST
above — a second HTTP call per cycle, to a second endpoint, matching the
per-station doc's own assumed shape exactly:

```
POST /private/station-full-coverage-samples
  [{ crs, operator, resolvedAt, stats: { total, delayed, cancelled, skipped, avgDelayMinutes } }, ...]
```

Only `(crs, operator)` pairs that actually resolved this cycle (2h: at
least one population UID whose `toc_id` was learned) are included — a
`Pending` per-station state is expressed the same way the line-level
table expresses it (2e), by the row's absence for that day, not by a
row carrying a sentinel.

**No shared database transaction across the two POSTs.** `full-coverage-consumer`
holds no direct database connection at all — like every other producer in
this repo (`trust-consumer`, `poller-ldbws`, `schedule-reference`), it only
ever reaches `api` over HTTP, and this repo has no existing precedent for
a cross-endpoint transaction spanning two producer-owned tables written by
one caller (`station_samples` and `line_status`/its rollups are already
written by different producers on different cycles with no such
coordination). The two POSTs are independent, best-effort calls; a partial
failure (line POST succeeds, station POST fails, or vice versa) leaves the
two tables briefly inconsistent for one cycle — a harmless, self-healing
gap, the same kind of transient inconsistency this app already tolerates
elsewhere (e.g. `stanox_crs`'s own reload cadence racing pin resolution,
`crates/trust-consumer/src/process.rs`'s documented "failed post recovered
by a restart, not by an in-process retry" posture). If a stronger
consistency guarantee is ever wanted, it belongs in a single `api`-side
handler accepting both payloads in one request/transaction — not designed
here, since nothing in either document's current scope needs it.

**Read path, gating, and rendering are the per-station doc's job, not
this one's.** This document only designs the producer (the write side);
`latest_station_full_coverage_samples`, the `(station, operator)`
`full_coverage_enabled_for` gate, `compute_station_operator_stats`'s
station-level merge, the route handler's 404-widening, and every frontend
change are the per-station doc's Decisions 1/3/4/5/6 — unchanged by this
revision, now resting on a real (if still-shadow-only) producer contract
instead of a guess. The per-station doc's own Open Question 1 ("does the
live consumer's actual persistence shape produce per-(station, operator)
rows at all... what table/schema does it actually write them into... does
it POST to `crates/api`") is answered by this section: yes, this exact
table, this exact endpoint, POST, matching its own guessed shape almost
exactly (this document's `stats` column is `JSONB`, matching its sketch;
field names match). Its Open Question 4 (which internal-OAuth group gates
the new endpoint) is answered by Decision 5 below: the same group/credential
that gates `/private/full-coverage-stats`, since both endpoints are written
by the same service.

### 4. Shadow-mode scope: what gets computed, and why the single flag doesn't need splitting

**`full_coverage_enabled`'s existing, already-merged meaning is not changed
and not split into two flags.** Its own doc comment
(`crates/common/src/lib.rs:490-497`) already states precisely: "Gates
whether a future full-coverage... consumer even attempts this line, not
merely whether its result is shown once resolved." Read literally, this
flag was already designed as a *compute* gate, not a *show* gate — which
initially looks like it forces a hard choice (per the task brief's own
framing): either repurpose it as "compute AND show" (impossible while it's
`false` everywhere, per the binding condition — reading it literally would
mean zero lines get shadow-computed at all, defeating the point) or split
it into two per-line TOML fields.

**Neither is the right call, once the actual write path is traced
through.** `full_coverage_enabled` only has one real consumer today:
`merge_full_coverage`'s per-line loop, which decides whether a line's
report is *shown/escalated* — despite its doc comment's "even attempts"
language, no code today actually gates *computation* on it, because no
computation exists yet. **This document resolves the ambiguity by keeping
`full_coverage_enabled` scoped to exactly its one real, already-merged
effect (gating `merge_full_coverage`'s show/escalate path) and introducing
a separate, deliberately non-catalogue scoping mechanism for what the
consumer computes** — not a second field on the shared `LineDefinition`/
`lines/*.toml`. Reasoning: "which lines does this one service bother
correlating against, this deployment" is a workload/cost-control knob
specific to `full-coverage-consumer`'s own runtime, not a fact worth
publishing to every other reader of the catalogue TOML (frontend,
`api`, every poller) the way `full_coverage_enabled`/`severity_overrides`
genuinely are. Overloading the shared catalogue with a service-local
scoping concern would also make the catalogue lie about what "enabled"
means to a casual reader, exactly the kind of ambiguity a second field
plus the existing one would create.

```rust
// crates/full-coverage-consumer/src/config.rs -- sketch, not final.
/// Comma-separated line ids to shadow-compute, or "*" for every catalogued
/// national-rail line (the default -- see this doc's Decision 4). Does NOT
/// gate whether a line's stats are ever shown/escalated -- that remains
/// LineDefinition.full_coverage_enabled's job, unchanged, in aggregator.
/// This flag exists solely so this one service's own workload can be
/// narrowed later (a specific line's correlation proving too costly or
/// too noisy to be worth computing) without a code change or a shared
/// catalogue edit.
#[arg(long, env, default_value = "*")]
pub shadow_lines: String,
```

**Decision: default to `"*"` — shadow-compute every catalogued national-rail
line, not a narrow pilot subset.** The brief's own justification for shadow
mode at all is decisive here: the whole point is "the consumer's
correctness gets proven by running it for real, continuously, in
production, compared against sampling — a better validation method than
the stalled manual spot-check." The stalled spot-check's own stated
weakness was sample size — 1 of 1 real disruption instance, explicitly
flagged by the findings doc as "too small to carry that weight." A curated
pilot subset of lines would reproduce exactly that small-N problem at a
slower pace, for no compensating cost saving that matters in practice:

- **Correlation cost does not scale materially with line count.** The
  reverse `tiploc -> Vec<line_id>` index (Decision 2c) makes per-event
  matching cost independent of how many lines are shadow-computed for the
  common case (most TIPLOCs belong to zero or one line). What scales with
  line count is the size of the in-memory population map and the daily
  `schedules_touching` query cost at publish time — and the first-slice
  plan's own real-extract validation already measured this at a workable
  scale (a 5-TIPLOC WCML query over the full 463,947-record extract
  returning 1227 schedules; the first-slice plan's Open Question 2
  resolution). Scaling that per-line query across this app's ~20-50
  catalogued lines is not a new order of magnitude.
- **Excluded, per the base spec's own already-settled scope, regardless of
  this decision**: TfL-adjacent/non-national-rail lines (`CustomLine`'s
  `full_coverage_enabled: false` construction,
  `crates/common/src/lib.rs:1055`, already fixed by the scaffolding — a
  user-defined line is never a candidate either way) and any line whose
  `lines/*.toml` carries no `tiploc` on any station (nothing to correlate
  against; `schedules_touching` would trivially return nothing for it,
  which is a harmless, honest `Available` with `total: 0`, not an error).

### 5. Operational shape

**Deployment**: a new Helm Deployment,
`charts/distant-signal/templates/full-coverage-consumer-deployment.yaml`,
structurally copied from `trust-consumer-deployment.yaml`'s own precedent
line for line: `replicas: 1` (same single-consumer-group-per-deployment
constraint as `trust-consumer`, same reasoning); the same
readiness/liveness probe shape against its own `/healthz`
(`health.rs`, verbatim-mirrored from `trust-consumer/src/health.rs`,
Decision 1's crate layout); the same `automountServiceAccountToken: false`
and container `securityContext` posture; the same
`internal-service-oauth2-design.md` Decision 6 pattern for OAuth config
(shared, non-secret `token_url`/`client_id`/`scope`, a **distinct**
per-service `username`/`password` Authentik app-password credential — "this
service's own... credential -- per-service, distinct from every other
caller's," `crates/trust-consumer/src/config.rs:73-80`'s own comment,
reused verbatim as the convention here). Kafka broker/topic/SASL values
**reuse** `trustConsumer.kafka.{brokers,topic,saslMechanism}` (same RDM
product, same credential per the connection-vs-group-membership reasoning
in Decision 1) rather than duplicating them a second time in `values.yaml`
— only `fullCoverageConsumer.kafka.consumerGroup` is new
(`distant-signal-full-coverage-consumer`, mirroring
`trustConsumer.kafka.consumerGroup`'s own default-value convention). The
same fail-fast Helm guard block `trust-consumer-deployment.yaml:1-22`
already uses for an empty broker/topic/mechanism is reused for this
Deployment's own render, unconditionally (this crate also has no
`enabled` toggle, matching `trust-consumer`'s own unconditional-render
posture — a persistent Kafka consumer isn't optional infrastructure once
deployed, same as its sibling).

**Dockerfile**: a new `docker/full-coverage-consumer.Dockerfile`, structurally
identical to `docker/trust-consumer.Dockerfile` (same `cmake`/`libssl-dev`/
`libsasl2-dev`/`libcurl4-openssl-dev` builder-stage requirements — this
crate pulls in `rdkafka` for exactly the same reason).

**Config**: `crates/full-coverage-consumer/src/config.rs`, a `clap::Parser`
struct mirroring `trust-consumer/src/config.rs`'s shape: Kafka
brokers/topic/consumer-group/SASL fields (GAP-flagged identically, no
default for brokers/topic/mechanism, matching that file's own honest
"unconfirmed" posture, `config.rs:11-20`), the shared+distinct OAuth2
fields, `population_reload_secs` (mirrors `reference_reload_secs`),
`shadow_lines` (Decision 4), `full_coverage_stats_url`/
`station_full_coverage_stats_url`/`line_population_url` (`api` endpoint
URLs — the second new per Decision 3's station-level write path),
`health_bind_url`. Both `/private/full-coverage-stats` and
`/private/station-full-coverage-samples` are gated by the **same** new
internal-OAuth group (e.g. `internal_oauth_group_full_coverage` on `api`'s
own `app.rs`, mirroring `internal_oauth_group_ldbws`'s existing
one-group-per-producer convention,
`crates/api/src/app.rs:91-100`) — one service, one credential, two
endpoints it's allowed to write to, the same shape `schedule-reference`
already uses for its own single credential against `/private/stanox-crs`,
generalized to two endpoints since this producer writes two tables. This
resolves the per-station doc's own Open Question 4 directly.

**Metrics**: unlike `trust-consumer` (which notably has **no**
`common::metrics::install` call at all — confirmed absent from its
`main.rs`, an omission this document does not propose copying), this new
service follows the **majority** precedent every other real service in
this repo sets (`aggregator/src/main.rs:35-36`, `schedule-reference/src/main.rs:31-32`,
every `poller-*`): `common::metrics::install(config.metrics_port)` behind a
`metrics_enabled` flag, with new counters —
`full_coverage_consumer_events_matched_total{line_id}`,
`full_coverage_consumer_lines_available_total`/`_pending_total` (a per-cycle
gauge pair), `full_coverage_consumer_stations_available_total`/`_pending_total`
(Decision 2h's station-grain analog), `full_coverage_consumer_station_buckets_dropped_total`
(Decision 2h's asymmetric-population case: a population UID whose
Activation was never observed, so it inflated a line's `cancelled` count
but was excluded from every station bucket — worth watching on its own,
since a persistently high rate here would suggest a real Activation-visibility
gap, not just an inherent per-day tail), `full_coverage_consumer_cycle_duration_seconds` — genuinely new
operational surface this repo has never run before (a shadow producer with
no live-severity blast radius, but real cost/correctness worth watching
closely during the exact continuous-comparison period the binding condition
exists to enable). This is a deliberate deviation from `trust-consumer`'s
own precedent, flagged as such, not a silent inconsistency.

**Health checks**: `/healthz`, connected/disconnected semantics identical
to `trust-consumer/src/health.rs` (Decision 1's verbatim reuse) — a
persistent Kafka consumer needs real liveness signal, the same reasoning
`health.rs`'s own module doc already states for its sibling.

## Explicitly out of scope

- **Flipping `LineDefinition.full_coverage_enabled` for any real line, or
  changing its default.** The entire point of this document is that this
  stays `false` everywhere; a future, separate task judges the shadow
  comparison and makes that call per line.
- **Wiring `schedule-query` into `trust-consumer`'s own live pin-matching
  (`matching.rs`).** Named as a natural follow-up by the scoping doc, not
  committed to here or by this document.
- **STP-overlay edge cases, CIF `AA`/Association records, or any record
  type beyond what `schedule-query` already handles.** This document's
  consumer is a caller of that library's existing public API
  (`schedule_for_uid`, `schedules_touching`), not a modification of it.
- **Broadening beyond national-rail lines.** TfL modes stay entirely outside
  this document's scope, matching the base spec's own scoping and the
  scaffolding spec's Decision 5 "scoping stays identical" note.
- **The `trust-schema` extraction refactor's own implementation.** Named as
  a real prerequisite in Decision 1, not performed by this design document.
- **Per-station full-coverage *reading, gating, or rendering*.** Per the
  2026-09-04 revision (Decision 2h, Decision 3's station-level half), this
  consumer now **does** produce and write per-`(crs, operator)` data — but
  the read query, the `LineDefinition.full_coverage_enabled`-derived
  per-station gate, `compute_station_operator_stats`'s merge, the route
  handler, and every frontend change remain entirely the per-station doc's
  own scope (`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`,
  Decisions 1/3/4/5/6), unchanged by this document.
- **A UI or comparison dashboard for reading shadow-mode data against live
  sampling.** This document designs the persistence layer
  (`full_coverage_line_stats`) that such a comparison would read from, not
  the comparison tooling itself — a human can query the table directly, or
  a future, separate task can build a proper view.
- **Any implementation plan or code.** Spec stage only, per this task's own
  brief.

## Open questions / risks

1. **Whether RDM's Kafka product terms meter or restrict multiple consumer
   groups against one credential is unconfirmed** — Decision 1 reasons from
   Kafka's own protocol (SASL authenticates a connection, not group
   membership) that a second group likely doesn't require a second RDM
   subscription grant, but this is inferred, not independently verified
   against RDM's specific Train Movements product terms. Confirm before
   deploying, per this repo's own "no invented API details" convention.
2. **The unconfirmed-by-window-close = cancelled default (Decision 2d) is a
   real accuracy risk, not a settled fact.** It could inflate cancellation
   rates if the true cause is a gap in this consumer's own feed coverage
   (an untranslatable STANOX, a missed Activation, a genuinely late-arriving
   Kafka message) rather than a real non-running service. This is exactly
   the kind of question the shadow-comparison period this document exists
   to enable should surface — if shadow-computed cancellation rates run
   suspiciously high against sampling/spot-checks, this default is the
   first place to revisit, before ever proposing `full_coverage_enabled`
   for a real line.
3. **`skipped`'s TRUST-`PASS`-to-`SampleStats.skipped` mapping (Decision 2g)
   is inherited from the scaffolding spec's own already-flagged Open
   Question 1, still unconfirmed against a real observed case.** Not
   independently resolved by this document.
4. **The literal, once-per-day `Pending`-until-close `Available` semantic
   (Decision 2e) trades liveness for honesty.** A lower-confidence,
   partial-day "informative but not yet the line's determination" mode is
   a plausible future refinement if shadow-comparison data would be more
   useful sampled more often than once a day — not designed here, since
   shadow mode's zero-live-effect posture means there's no product pressure
   to solve this now.
5. **Whether `full_coverage_line_stats`'s staleness guard (a `service_date`
   column, Decision 3) needs active enforcement (e.g. `aggregator`
   rejecting a row whose `service_date` isn't today) or can stay a passive,
   inspectable field is not decided here** — the read-path sketch already
   filters on `service_date == today` server-side (`api`'s
   `/private/full-coverage-stats` GET), which may already be sufficient;
   flagged as an implementation-time judgment call, not a design gap.
6. **The `trust-schema` extraction (Decision 1) hasn't been scoped as its
   own task** — file boundaries, whether `journey.rs`'s
   `DerivedState`/`apply_movement` need any generalization to serve a
   second caller with a different `previous`-state shape (this consumer's
   per-`(line_id, uid)` records vs. `trust-consumer`'s per-`train_id`
   records) is real design work for whichever implementation plan follows
   this document, not resolved here.
7. ~~Whether `schedule-reference` gaining a second responsibility (Decision
   2a) is the right long-term call... depends on how the concurrent
   per-station full-coverage workstream's own needs shake out~~ **Resolved
   2026-09-04**: the per-station doc's own producer-contract need (its
   Open Question 1) is now answered by this document's Decision 2h/3
   directly — `schedule-reference`'s second responsibility (publishing
   per-line CIF populations) is unaffected, since station-level output is
   derived entirely from the *live TRUST stream* this consumer already
   processes (2h), not from a second CIF-derived product `schedule-reference`
   would need to publish. No coupling risk remains between the two
   documents' producer-side designs; they converge on one write path.
8. **The asymmetric-population finding (Decision 2h): a population UID
   whose Activation is never observed inflates its line's `cancelled`
   count but is silently excluded from every station bucket.** This is a
   real, structural difference between the line-level and station-level
   outputs, not a bug to fix in this pass — but it means a line's
   full-coverage `cancelled` rate and the sum of its constituent stations'
   `cancelled` rates will not reconcile exactly, which could be surprising
   to a future reader comparing the two tables without having read this
   section. Worth a code comment cross-referencing this doc at both write
   sites, not just documented here.
9. **Whether `(crs, operator)` is the right station-level key, versus
   `(tiploc, operator)` or a segment-aware key, is inherited from the
   per-station doc's own choice, not independently re-derived here** — this
   document adopts `crs` because that's what the per-station doc's Decision
   2 already committed to (matching `station_samples`'s own CRS-keyed
   convention), and because CRS is what this consumer's STANOX translation
   already produces without a second lookup (2c/2h). If the per-station
   doc's own key choice changes before implementation, this document's
   write path needs to change with it.
10. **No shared transaction across the two POSTs (Decision 3) means a
    line and its stations can briefly disagree on availability state for
    one cycle** (e.g. the line POST succeeds and reads `Available` while a
    station POST for the same line fails and still reads the prior cycle's
    `Pending`/stale row). Judged an acceptable, self-healing gap given this
    repo's existing tolerance for equivalent transient inconsistencies
    elsewhere (Decision 3's own citations) — flagged, not eliminated.
