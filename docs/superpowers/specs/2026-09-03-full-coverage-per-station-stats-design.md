# Design: Per-Station Stats Under Full-Coverage (TRUST) Data

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the line-level doc") and `docs/superpowers/specs/2026-09-03-per-station-stats-design.md`
("the station-level doc") — the two documents this one sits directly
between. The line-level doc's own Explicitly out of scope section names
exactly this gap and declines to fill it: *"Per-station full-coverage
stats. The brief notes a second, concurrent research effort into
per-station stats specifically — this document deliberately stays scoped
to the line-level `LineStatus` surfaces... and does not attempt to also
design the station-level transition, to avoid duplicating or preempting
that parallel work."* This document is that deferred piece, written now
that both prerequisite documents are settled: the station-level doc is
merged to `main` in full (verified directly, see Current relevant state),
and the line-level doc exists and is implemented in a separate,
not-yet-merged worktree (per this task's brief) — this document treats the
line-level doc's own code sketches, not any in-progress implementation, as
the authoritative source for that work's shape.

This document does not propose building Option B itself (TRUST-vs-schedule
delay inference, `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`),
its line-level consumer, or any per-station full-coverage consumer sketched
in Decision 3 below. All three stay gated on Option B's own future
validation/planning pass — `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`'s
verdict is still "not yet" as of its most recent 2026-09-03 re-run, now
blocked on production SSO access rather than a broken mechanism (skimmed
for context only; not this document's concern to resolve). No
implementation plan is included; that is a separate, later step in this
repo's process. Every sketch below is marked as a sketch, not final code.

## Goal

Resolve four concrete, currently-unanswered questions about what happens
to per-station stats once (or as) full-coverage TRUST/schedule data
exists, given that today's per-station work is entirely LDBWS-sample-based
and today's full-coverage scaffolding is entirely line-level:

1. Does the station-level doc's operator-scoping decision (one entry per
   (station, operator), never blended) still hold once the underlying feed
   is TRUST instead of LDBWS, or does TRUST's per-train (not
   pre-aggregated-per-station) shape change the calculus?
2. Does the shared-arithmetic pattern (`common::compute_sample_stats`,
   promoted once already so the line-level and station-level *sample*
   computations share one implementation) generalize a third time for the
   line-level full-coverage scaffolding's own counting logic?
3. Where would full-coverage per-station data actually come from,
   architecturally — what would a not-yet-built Option B consumer need to
   produce, at what granularity, for a per-station read to work?
4. Should the equivalent per-station scaffolding be built now, alongside
   the line-level scaffolding currently in flight — mirroring that
   document's own precedent of building ahead of the producer — or does
   the station-level case have a real, different reason to wait?

## Current relevant state (verified 2026-09-03, in this worktree)

**The station-level doc is fully implemented and merged, not a sketch.**
Confirmed directly against the working tree (not assumed from the design
doc's own claims): `crates/api/src/data/station_stats.rs` (69 lines,
`compute_station_operator_stats`) and `crates/api/src/routes/station_stats.rs`
(the `GET /public/stations/{crs}/sample-stats` handler, including both a
`#[cfg(test)]` unit module and an `#[ignore]`d `db_tests` module) exist
verbatim as that document's Decision 6/7 sketched them, down to the exact
function/struct names and doc comments. `common::compute_sample_stats`
(`crates/common/src/lib.rs:961-984`) is a real, promoted, `#[cfg(test)]`-covered
function, and `crates/aggregator/src/aggregation.rs:816-825`'s
`stats_from_departures` is the thin wrapper Decision 5 of that doc
specified, still crate-private to `aggregator`. The frontend side
(`frontend/lib/types.ts:117-121`'s `StationOperatorSampleStats`,
`frontend/lib/api.ts`'s `getStationSampleStats`,
`frontend/app/stations/[crs]/page.tsx`'s `fetchStationSampleStats`) is
likewise present and matches the doc's Decision 9. This document takes all
of it as real, load-bearing existing code, not a hypothetical to design
against.

**The line-level doc is a real design, not yet implemented in this
worktree.** Confirmed by direct grep: `FullCoverageAvailability`,
`full_coverage_stats`, and `full_coverage_enabled` occur nowhere under
`crates/` or `frontend/` in this worktree. Per this task's brief, a
separate, parallel, not-yet-merged worktree is implementing it — this
document reads that doc's own code sketches (types, `LineDefinition`
field, wire shape, `merge_full_coverage`/`escalate_from_coverage_stats`
naming) as the shape to design against, and does not re-verify them
against code that does not yet exist here.

**`common::compute_sample_stats`** (`crates/common/src/lib.rs:961-984`) —
the shared arithmetic both existing sample computations already delegate
to:

```rust
pub fn compute_sample_stats(
    departures: &[&StationDeparture],
    delay_threshold_minutes: i64,
    is_skip: impl Fn(&StationDeparture) -> bool,
) -> SampleStats
```

Its input is concretely `&StationDeparture` (`lib.rs:394-401`) — an
LDBWS-schema-shaped struct (`operator`, `is_cancelled`, `delay_minutes`,
`skipped_stations`, `destination_crs`, `headcode`) — not a generic trait.
Both existing callers supply real `StationDeparture` values, differing
only in *population* (`aggregation.rs:794-810`'s `relevant_departures`
pools across a line's several `sample_stations`; `station_stats.rs:36-49`
filters one station's own `StationSample.departures` by `operator`) and in
the `is_skip` predicate (line-membership vs. this-CRS-membership, per the
station-level doc's Decision 4).

**`LineDefinition`** (`crates/common/src/lib.rs:450-`) already has a
natural per-line config surface for opt-in flags: `severity_overrides`
(`:464`), `exclusive_segments` (`:469`), both `#[serde(default)]`,
catalogue-authored in `lines/*.toml`. The line-level doc's `full_coverage_enabled`
sketch hangs off this same struct. **No station-level analog of this
struct exists.** The station-level doc's own Decision 3, re-confirmed by
reading the merged code: *"No per-station equivalent of `LineDefinition.severity_overrides`
exists anywhere in this codebase, and this document does not invent
one... uses `common::Defaults::default()` directly."* There is no
`stations/*.toml`, no per-station catalogue row, nothing a per-station
opt-in flag could be added to without inventing a wholly new config
concept for this purpose alone.

**TRUST's own message schema already carries an operator field, per-message, confirmed by direct reading, not inferred from the design docs' domain-knowledge claims.**
`crates/trust-consumer/src/schema.rs`:

```rust
pub struct Activation {
    pub train_id: String,
    pub train_uid: String,
    // ...
    pub toc_id: String,                    // schema.rs:41, mandatory
}

pub struct Movement {
    pub train_id: String,
    pub event_type: String,                // schema.rs:57 -- "ARRIVAL | DEPARTURE | PASS"
    // ...
    pub loc_stanox: Option<String>,        // schema.rs:64
    pub toc_id: Option<String>,            // schema.rs:66, currently #[allow(dead_code)]
}
```

`Movement.toc_id` is marked dead code today specifically because
`trust-consumer`'s only live consumer, `matching.rs`, doesn't need it for
its current job (best-effort pin-vs-live-feed correlation, not stats) —
not because the field is unavailable or unreliable. `event_type`'s own
comment already lists `PASS` as a real, confirmed variant, distinct from
`ARRIVAL`/`DEPARTURE` — direct, code-level corroboration of the line-level
doc's own flagged-but-unconfirmed Open Question 1 (whether TRUST's `PASS`
is the right analog for `SampleStats.skipped`), though still not proof
that mapping is exactly right (see Open questions).

**STANOX-to-CRS resolution already exists, twice over — a real prerequisite this document does not need to invent.**
`crates/schedule-reference/src/main.rs:1-9`'s own module doc: reads a
schedule-feed delivery's reference records, resolves a STANOX→CRS table,
and `POST`s it to `/private/stanox-crs`. `crates/trust-consumer/src/stanox_crs.rs:1-20`
(module doc, read in full) already loads and applies that exact table to
bridge TRUST's `loc_stanox` (a raw STANOX) to the CRS codes the rest of
this app uses everywhere else. Any future full-coverage consumer, at line
or station grain, would reuse this unchanged.

**What does *not* yet exist, confirmed directly, and is the real bottleneck under both line- and station-level full coverage:**
a train_uid → full booked-schedule (every calling point, with booked
times) bridge. `crates/trust-consumer/src/matching.rs:1-6`'s own module
doc, quoted in full because it is precise and load-bearing: *"Best-effort
resolution of a user's pin (origin CRS + scheduled departure time, date —
no train_uid) against the live TRUST feed... this app has no CIF schedule
lookup to bridge Activation's train_uid to a departure time."* Today's
`trust-consumer` heuristically matches one pinned train's *origin*
Movement event against a user-supplied expected time — it does not, and
by its own doc comment cannot yet, resolve any train's full booked
calling-point-by-calling-point schedule, which is exactly what "every
scheduled service, cross-referenced against real TRUST movement events"
(the line-level doc's own Goal-section framing of what Option B does)
requires. This gap is not new information this document is introducing —
it is the concrete shape of why Option B "stays gated on its own future
validation/planning pass" — but it is worth citing precisely here because
Decision 3 below depends on it directly.

## Decisions

### 1. Operator-scoping still holds for full-coverage data — same decision, a more direct mechanism, resolves Q1

**The station-level doc's Decision 1 (one entry per (station, operator),
never a blended number) carries forward unchanged.** The 53/286
multi-operator-station finding that decision rests on is a fact about
which operators' timetabled services call at a given physical station — a
property of the national timetable, not of which feed (LDBWS or TRUST)
happens to be reporting on it. Full-coverage data does not make Edinburgh,
Liverpool Lime Street, or Newcastle single-operator stations; it only
changes how confidently the app can report a number once it's scoped
correctly. If anything, the case for scoping strengthens under full
coverage: a *hedged* blended number is already understood by a reader as
approximate, but a *confident, full-coverage-badged* blended number
(Decision 2 of the line-level doc gives full coverage exactly this kind of
confident, unhedged presentation) would misrepresent the same 19% of
stations with less excuse, not more.

**What genuinely changes is not whether to scope by operator, but how the
operator is determined — and TRUST turns out to make this easier, not
harder, contrary to what "TRUST movement events are per-train, not
pre-aggregated by operator" might suggest at first glance.** An LDBWS
departure-board row's `operator` field is Darwin's own enrichment,
computed upstream of this app by a schedule join Darwin performs before
this app ever sees the row — this app has never had to derive operator
identity itself for the LDBWS path. TRUST's raw feed looks, at first
glance, like it lacks that pre-aggregation: a `Movement` message is one
event for one train at one location, with no "board" grouping multiple
trains by anything. But confirmed directly (Current relevant state,
above): TRUST's own `Activation` and `Movement` messages **already carry
`toc_id` as a field on the message itself** — `Activation.toc_id` is
mandatory (`schema.rs:41`), `Movement.toc_id` is present but currently
unused (`schema.rs:66`). This means a full-coverage station-stats
computation does not need Option B to perform a CIF schedule join *just to
learn which operator ran a given train* — the signalling feed self-declares
it, the same structural fact LDBWS gives this app today, just via a field
this app has not yet had a reason to read.

**The one real caveat**: `Movement.toc_id` is `Option<String>`, not a
guaranteed-present `String` the way `Activation.toc_id` is — whether it is
reliably populated across real production TRUST traffic at volume (versus
frequently `None`, requiring a fallback to the corresponding `Activation`'s
`toc_id` by `train_id`) is not verified here; flagged in Open questions.
This does not change the scoping *decision*, only how confidently a future
implementation could rely on `Movement.toc_id` alone versus needing to
also track each `train_id`'s `toc_id` from its `Activation` as a fallback.

**Conclusion**: this is the same decision, resting on the same underlying
fact (a shared station genuinely serves multiple operators), reached for
the same product reason (a blended number is misleading at exactly the
stations busy enough to have real data) — TRUST's per-train message shape
does not force a re-derivation of operator identity, because operator
identity already travels with each TRUST message, currently dormant
rather than absent.

### 2. The shared-arithmetic pattern generalizes a third time — but the line-level scaffolding currently in flight has no arithmetic of its own to generalize yet, a correction to the brief's framing

**Correction, in this repo's established style**: the brief's premise —
that the line-level full-coverage scaffolding has "its own new counting
logic" this document should check for reusability — does not hold as
stated, checked directly against that document's own Decision 1 and
Architecture section, not assumed. Its Architecture diagram is explicit:

> `Layer 3 (NEW): full coverage / merge_full_coverage (analog of
> merge_dlr_sample_stats, NOT compute_sample_availability -- the producer
> already resolved the population)`

`merge_full_coverage` is designed as the analog of `merge_dlr_sample_stats`
(`crates/poller-tfl/src/main.rs:206-214`, cited by the line-level doc's own
Current relevant state) — a function that takes an **already-computed**
`SampleStats` from an external producer and assigns it directly onto a
`LineStatus`, with no threshold check, no filtering, no arithmetic of its
own. This is a deliberate design choice the line-level doc makes
explicitly (its own words: *"the producer already did that work"*) — the
scaffolding's job is to plumb a pre-resolved per-line signal through
`aggregator`, `crates/api`, and the frontend; it is not designed to compute
anything from raw TRUST events itself. **There is, as designed, no new
counting logic inside the line-level scaffolding to generalize — the
question this document was asked to check for turns out not to exist at
that layer, which is itself the useful finding.**

**The real counting logic this question is actually about lives one level
further out: inside Option B's own future consumer** — wherever it ends up
living (a new crate, or an expansion of `trust-consumer` once the
schedule-bridge gap named in Current relevant state is closed) — the thing
that turns raw TRUST `Movement`/`Activation` events plus a CIF schedule
join into the `SampleStats`-shaped numbers `merge_full_coverage` will
receive. **That** is where the question "should this be shaped the same
reusable way from the start" actually applies, and the answer is yes, with
one concrete design choice flagged rather than settled:

`common::compute_sample_stats`'s signature is coupled to `&[&StationDeparture]`,
not a generic trait (Current relevant state, above). A future full-coverage
consumer's raw substrate is not `StationDeparture` — it is TRUST events
cross-referenced against a CIF schedule it does not yet have a way to read
(Current relevant state's schedule-bridge gap). Whatever eventually closes
that gap will naturally produce, per resolved train-at-calling-point,
values carrying the same *essential* facts `StationDeparture` already
carries (cancelled-or-not, an actual-vs-booked delay, which calling points
were skipped, an operator code) even though it will not be literally a
`StationDeparture` (no `destination_crs`/`headcode`/string-typed
`scheduled`/`estimated` concept from a resolved-movement record). Two
non-exclusive ways this could reuse `compute_sample_stats`, neither
committed to here since neither can be validated without a real second
concrete input shape to check it against:

```rust
// Option (a) -- sketch, not proposed now. Normalize onto the EXISTING
// struct as a pure computation vehicle, mirroring the line-level doc's own
// Decision 1 reasoning for SampleStats itself ("the struct's own doc
// comment already only says 'sample-derived,' not anything about HOW the
// sample was taken" -- treat StationDeparture the same way, as a value
// shape, not an LDBWS-schema-coupled one, when a future consumer needs to
// hand compute_sample_stats something).
let synthetic: Vec<StationDeparture> = resolved_matches
    .iter()
    .map(|m| StationDeparture {
        operator: m.toc_id.clone(),
        is_cancelled: m.cancelled,
        delay_minutes: m.actual_minus_booked_minutes,
        skipped_stations: m.pass_only_stanox_crs.clone(),
        // ...destination_crs/headcode/scheduled/estimated: whatever
        // placeholder or real value is cheapest; compute_sample_stats
        // never reads them.
        ..Default::default()
    })
    .collect();
common::compute_sample_stats(&synthetic.iter().collect::<Vec<_>>(), threshold, is_skip)

// Option (b) -- sketch, not proposed now. Generalize the function itself
// if (a) turns out to be an awkward fit once a real second input shape
// exists to check it against -- a strictly bigger, riskier signature
// change than (a), not attempted here since it can't be validated with
// only one concrete caller shape (StationDeparture) to generalize from.
pub trait DelayOutcome {
    fn is_cancelled(&self) -> bool;
    fn delay_minutes(&self) -> i64;
}
pub fn compute_sample_stats<T: DelayOutcome>(
    outcomes: &[&T],
    delay_threshold_minutes: i64,
    is_skip: impl Fn(&T) -> bool,
) -> SampleStats
```

**This document's recommendation, non-binding since Option B doesn't exist
to validate it against**: prefer (a) when the time comes — it costs zero
new arithmetic and directly extends a reuse pattern this repo has already
chosen once (`SampleStats` itself, per the line-level doc's Decision 1);
name (b) as the fallback if (a) proves awkward in practice, not as a
default.

**The clean symmetry this sets up, worth stating explicitly since it is
the actual payoff of asking this question**: once Option B exists, this
app would have four `SampleStats`-shaped computations — line-level LDBWS
sample, per-(station, operator) LDBWS sample (both live today), line-level
full coverage, per-(station, operator) full coverage (both hypothetical) —
and all four would reduce to "supply `common::compute_sample_stats` a
population and a skip predicate," differing only in how the population is
assembled (pooled sample stations / one station's board / a line's whole
matched schedule / one station's matched schedule) and what "skip" means
for that caller (route-membership / this-CRS-membership / TRUST `PASS`
somewhere on the route / TRUST `PASS` at this CRS). That symmetry is real
and worth designing toward, contingent on option (a) above holding up when
someone actually tries it.

### 3. Architecture sketch: what a full-coverage per-station read would need — design-level only, resolves Q3

Necessarily speculative on top of an already-speculative Option B — marked
throughout as a sketch of shape, not a commitment, per this document's own
Status line.

**Building blocks that already exist and would be reused unchanged, not invented by this sketch**:
STANOX→CRS resolution (`schedule-reference` + `trust-consumer/src/stanox_crs.rs`,
Current relevant state) and TRUST's own per-message `toc_id` (Decision 1).
Neither is a new prerequisite this document introduces.

**The real missing piece, not station-specific**: the train_uid → full
booked-schedule bridge (Current relevant state). Every sketch below
assumes that bridge exists as a side effect of Option B's own future line-
level work — this document does not design it, size it, or assume it is
close.

**Granularity, worked from first principles of what "was this train
delayed / did it skip a stop" actually requires to compute at all**: to
produce a per-line `SampleStats` the way the line-level doc's Goal section
describes ("every scheduled service on a line, cross-referenced against
real TRUST movement events"), Option B's own matching logic is
*structurally forced* to resolve, for each scheduled service, its
actual-vs-booked outcome at individual calling points — you cannot know a
train was "delayed" without knowing its actual time at some specific
point, and you cannot know it "skipped" a stop without knowing which
specific calling point it passed rather than called at. **A per-line
number is therefore already a rollup of per-(train, calling-point) facts,
not a different kind of computation from a per-station one** — the
granularity a per-station read needs already has to exist, at least
transiently, inside whatever produces the per-line number. What is
genuinely undecided is whether Option B's consumer ever *externalizes*
that finer-grained intermediate result, or collapses straight to a
per-line row and discards it (see Open questions).

**Sketch of the read path, conditional on that intermediate result being
externalized in some form**:

```
Option B's future consumer (not this document's job to build)
        │
        │ resolves, per scheduled service on an enabled line:
        │   { train_uid, toc_id, stanox (→ CRS via schedule-reference's
        │     existing table), booked_time, actual_time | PASS | cancelled }
        │   per calling point -- the same intermediate granularity
        │   already required to compute a per-line SampleStats at all
        ▼
  ┌─────────────────────┬───────────────────────────────────────┐
  │ per-line rollup      │ per-station rollup (NOT DESIGNED HERE  │
  │ (line-level doc's    │ WHETHER THIS IS EVER PRODUCED --       │
  │ own scope, Decision  │ open question below)                  │
  │ 1's merge_full_       │                                       │
  │ coverage consumes    │  grouped by (station CRS, toc_id),     │
  │ this)                │  fed through common::compute_sample_   │
  │                      │  stats with an is_skip predicate of    │
  │                      │  "PASS at this station's STANOX(es)"   │
  │                      │  -- the full-coverage analog of the    │
  │                      │  station-level doc's Decision 4        │
  └─────────────────────┴───────────────┬───────────────────────┘
                                          │
                            two non-exclusive shapes this
                            document does not choose between:
                                          │
              ┌───────────────────────────┴───────────────────────────┐
              ▼                                                       ▼
  read-time, Option-C analog:                          rollup table, per-station-stats
  a per-CRS current-window row,                        doc's own "Option B" analog:
  read via a latest_station_full_                      a per-(station, operator, day)
  coverage_sample-shaped query                          accumulate-upsert table, mirroring
  mirroring latest_station_sample                       line_status_daily_coverage_stats
  (queries.rs:664-679) exactly                          (the line-level doc's Decision 4 sketch)
                                          │
                                          ▼
              GET /public/stations/{crs}/sample-stats's future
              full-coverage-aware sibling (or an extension of the
              existing route) -- NOT DESIGNED HERE, see Decision 4/5
```

**Why the rollup-table option stops being premature, specifically once
Option B exists, even though the station-level research doc explicitly
deferred it for LDBWS-only data**: that doc's own reasoning for deferring
a `station_status_daily_stats` table was that building durable per-station
history against the smaller of two possible future datasets (286-station
LDBWS-only) risked being obsoleted by TRUST's broader coverage "for free."
Once Option B is real, that risk has resolved in one specific direction —
and the calling-point-level population a per-station rollup would need is,
per the granularity argument above, *already being computed* as a
byproduct of the per-line number, not a separate collection effort. This
is a materially different cost calculus than building the same table today
against LDBWS data alone.

**What this document deliberately does not decide**: whether Option B's
consumer should ever expose per-station granularity as a first-class,
independently-queryable output at all, versus keeping it purely an
internal detail of computing the per-line rollup, never persisted or read
at station grain until a concrete product need is confirmed — the same
"don't build storage ahead of a confirmed need" posture the station-level
research doc already took once for LDBWS-only history. Flagged in Open
questions, not settled here, because settling it requires knowing things
about Option B's actual consumer architecture (which crate, what its
natural internal data shape is, what its own performance/storage
constraints are) that do not exist yet.

### 4. Sequencing recommendation: do not build the equivalent scaffolding now — a real, different reason to wait, not a default to caution — resolves Q4

**Recommendation, stated plainly: defer.** Do not add a
`full_coverage_availability`-shaped field to the per-station-stats response
now, alongside the line-level scaffolding currently in flight. This is a
genuine "wait," reasoned against the specific precedent the line-level
scaffolding sets, not a reflexive more-conservative default — four
concrete, structural differences justify it:

**1. There is no station-level home for a rollout flag, unlike the
line-level case, and inventing one purely to support unexercised
scaffolding would be backwards.** The line-level scaffolding's
`full_coverage_enabled` flag has a natural, already-established home:
`LineDefinition` (`crates/common/src/lib.rs:450-`), a per-line,
catalogue-authored struct that already carries two structurally identical
precedents (`severity_overrides`, `exclusive_segments`). The station-level
computation has no equivalent catalogue entity at all — it is deliberately
architected as a stateless, read-time function over whatever
`station_samples` happens to hold (`compute_station_operator_stats`,
`station_stats.rs:32`), with the station-level doc's own Decision 3
explicitly declining to invent a per-station override/config concept for
threshold tuning, the closest analogous need that already existed and was
still rejected. Adding a `full_coverage_enabled`-style flag now would mean
inventing a wholly new per-station config surface — the first one this
codebase would have — purely to gate a feature with no real producer yet,
rather than reusing an established pattern the way the line-level case
did.

**2. The line-level scaffolding had a settled producer *contract* to
scaffold against before it was built; the station-level case does not.**
The line-level doc's own Correction 1 makes clear its scaffolding is not
built against a vague hope — it is built against a specific, already-designed
target shape: `2026-08-29-trust-schedule-delay-inference-design.md`'s
Option B was *already recommended* as "a new dedicated consumer... hand
`aggregator` a per-line materialized signal to consume as a third input,"
and `merge_dlr_sample_stats` already demonstrates that exact hand-off shape
working in production for a different producer. The line-level scaffolding
is therefore built against a real, settled contract — only the producer
implementing that contract is missing. Per Decision 3 above, no equivalent
settled contract exists yet at station grain: whether Option B's consumer
would ever expose per-station output at all, and in what shape (live
read-time row vs. persisted rollup), is explicitly undecided, not merely
unbuilt. Building a wire shape now would be scaffolding against a guess of
a guess, not against an already-specified target the line-level case had
the benefit of.

**3. The future migration cost of adding this later is small and
well-precedented, unlike the line-level case's own justification for going
early.** The line-level doc's own case for building ahead of the producer
rests on real complexity worth settling early — a gradual per-line rollout
state machine, mixed-state presentation across `IssueList`'s per-status
iteration and four different "representative status" summary surfaces, and
a new sibling pair of rollup tables (its own Decision 4) — six-plus call
sites reworked in one pass, per its own table. The per-station-stats
surface has exactly one call site (`GET /public/stations/{crs}/sample-stats`,
one route, one component section) and no rollup/history table at all. The
station-level doc's own Decision 5/9 already demonstrate, in the code that
shipped, how cheap this kind of later extension is in practice: promoting
`compute_sample_stats` and widening `sampleUnavailableReason`'s input type
were both small, additive, non-breaking changes made *after* the line-level
`SampleAvailability` work had already shipped. Adding a
`fullCoverageAvailability`-shaped field to one route and one frontend
section later is the same class of change, not the six-site
migration the line-level case was justified in getting ahead of.

**4. Concretely, there would be nothing real to exercise even the
plumbing against.** The line-level scaffolding, once built, has a genuine
(if manual) way to be dry-run end-to-end even before Option B ships: an
operator can flip `full_coverage_enabled` on one line in `lines/*.toml` and
manually construct a `FullCoverageAvailability::Available` value to confirm
the wire/frontend path holds together, because the gating mechanism (a
TOML flag) already exists as real, editable config. Per point 1, no
equivalent dry-run capability exists for a per-station flag without first
inventing the config surface to hold it — meaning building it now would be
scaffolding that cannot even be manually exercised, a strictly weaker
position than the line-level precedent this recommendation is explicitly
weighed against.

**This is not "the station case is inherently less important" or "always
prefer the safe option" — it is that the specific enabling conditions that
made building the line-level scaffolding early a sound bet (a settled
target contract, a natural config home, a real dry-run path, and enough
downstream complexity to be worth de-risking early) are each concretely
absent at the station grain today.** If any of them change, the calculus
changes — see the trigger conditions below.

### 5. Trigger conditions for revisiting Decision 4

Per this document's own recommendation to defer rather than build, precise
conditions under which the station-level scaffolding becomes worth
building, mirroring the "reconsider only when a specific, named condition
changes" convention this repo's adjacent documents already use (e.g. the
station-level research doc's own "either a concrete product need... is
confirmed, or TRUST's own Task 8 reaches a verdict" framing for its Option
B):

1. **Option B's future consumer's own design work decides whether it
   exposes per-station/calling-point granularity as a first-class,
   independently-queryable output** (Decision 3's open question) — this
   alone, even before Option B is fully built or validated, would give
   per-station scaffolding the same kind of settled contract the line-level
   case already had before its own producer existed, resolving point 2 of
   Decision 4 independently of Option B's validation timeline.
2. **A concrete, unrelated need emerges for a per-station config/catalogue
   concept** (e.g. a per-station `severity_overrides`-equivalent, for
   threshold tuning reasons that have nothing to do with full coverage) —
   this would resolve point 1 of Decision 4 by giving a rollout flag a
   natural home, independent of Option B's timeline.
3. **Option B's own Task 8 validation (per the 2026-08-29 validation
   findings doc) reaches an actual "go" verdict and a real consumer begins
   being built** — at that point, whatever concrete shape that consumer's
   design work settles on (point 1 above) should be read directly, and this
   document's Decision 3 sketch revisited against real code rather than the
   speculative shape sketched here.

None of these three needs to happen before per-station stats keeps working
exactly as it does today (LDBWS-sample-only, per the merged station-level
doc) — this document does not propose any change to what ships today, only
to when a full-coverage-aware extension of it becomes worth designing in
concrete wire-shape terms.

## Explicitly out of scope

- **Building Option B itself, its line-level consumer, or any per-station
  full-coverage consumer.** All three stay gated on Option B's own future
  validation/planning pass, per the base spec and validation-findings
  documents.
- **The CIF train_uid → full booked-schedule bridge** identified in
  Current relevant state as the real missing prerequisite under both
  line- and station-level full coverage. Named, not sized or designed —
  a materially large, separate piece of work.
- **Deciding whether Option B's future consumer ever exposes per-station
  granularity as a first-class output versus an internal-only byproduct
  of the per-line rollup.** Flagged as the central open architectural
  question in Decision 3, deliberately not settled here.
- **Any change to `common::compute_sample_stats`'s signature.** Decision 2
  sketches two non-exclusive future shapes (normalize onto
  `StationDeparture`, or generalize to a trait) and recommends the former
  as a default, but commits to neither — this is real future work, not
  proposed for this pass.
- **A per-station config/catalogue concept analogous to `LineDefinition`/`lines/*.toml`.**
  Named in Decision 4 as the real missing home for a future rollout flag,
  not designed — inventing one is explicitly rejected as a reason to build
  scaffolding now (Decision 4, point 1), and would be a separate design
  exercise if a real, unrelated need for it ever emerges (trigger 2).
- **Any actual wire/type/field changes to the per-station-stats endpoint or
  its response shape.** Per Decision 4/5, this document recommends
  deferring that work entirely; no sketch of it is proposed as buildable
  now.
- **Reconciling `ServiceArguments.defaults_file`'s dead-field status.**
  Already flagged out of scope by the station-level doc; unchanged, still
  true, not revisited here.
- **Any change to the line-level full-coverage scaffolding itself.** That
  work is a separate, parallel, in-flight effort; this document reads its
  design but proposes no changes to it.

## Open questions / risks

1. **Whether `Movement.toc_id`'s current `Option<String>`/`#[allow(dead_code)]`
   status reflects real, reliable production presence, or whether it is
   frequently `None`** (requiring a fallback join to the corresponding
   `Activation.toc_id` by `train_id`) is not verified here — Decision 1's
   "same mechanism, more direct" claim assumes it is reliably present;
   unconfirmed against real TRUST traffic at volume.
2. **Whether TRUST's `PASS` event type is exactly the right analog for
   `SampleStats.skipped`** remains the line-level doc's own unresolved
   Open Question 1, inherited unchanged here for the per-station case —
   this document's Decision 3 sketch assumes it, but does not
   independently confirm it against a real TRUST message.
3. **Whether option (a) in Decision 2 (normalizing a future full-coverage
   consumer's resolved matches onto synthetic `StationDeparture` values to
   reuse `compute_sample_stats` unchanged) actually holds up once someone
   builds it, versus needing the trait-based generalization (option b)**
   — cannot be validated without a real second concrete input shape, which
   does not exist until Option B's consumer is designed in detail.
4. **The scope and cost of the CIF train_uid → full booked-schedule
   bridge** (Current relevant state) is not sized here — it is the actual
   hard prerequisite under both line- and station-level full coverage, and
   this document deliberately does not attempt to estimate it, consistent
   with the line-level doc's and validation-findings doc's own posture of
   leaving Option B's build cost to its own future planning pass.
5. **Whether a per-station full-coverage read, if ever built, should
   inherit the station-level doc's exact Decision 7 shape** (404-vs-`[]`
   honesty, hand-built JSON to avoid the nested-rename pitfall, an
   unreachable-`NoCoverage`-through-this-route invariant) or needs its own
   treatment given a structurally different producer — not resolved here,
   deferred along with the rest of the concrete design per Decision 4/5.
