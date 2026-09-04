# Design: Per-Station Full-Coverage Stats — Concrete Wire Shape and Config Gating

**Status: design proposal, not approved.** Written directly on top of
`docs/superpowers/specs/2026-09-03-full-coverage-per-station-stats-design.md`
("the deferred doc"), which investigated the same question this document
answers and landed on a **deliberate "defer"** (its Decision 4), citing four
structural reasons: no per-station config surface to hang a rollout flag on,
no settled producer contract to scaffold against, cheap to add later, and no
way to dry-run the plumbing without inventing an unrelated config concept.
**The repo owner has since explicitly overridden that deferral** — the same
kind of override already applied to the line-level full-coverage scaffolding
this document builds on (`docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`,
"the line-level doc" below — real, merged, verified directly in this
worktree, see Current relevant state). This document does not re-litigate
whether to build; it resolves the deferred doc's four open reasons into a
concrete, buildable design, per that override. No implementation plan is
included; that is a separate, later step in this repo's process. Every
sketch below (types, schema, routes) is marked as a sketch, not final code.

## Relationship to the parallel live-consumer design

Per this task's brief, a second, concurrent document —
`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`,
covering Option B's actual live consumer and the settled producer contract
the deferred doc's Reason 2 said didn't exist yet — may or may not have
landed before this one. **Checked directly at the start of this pass**:
`ls docs/superpowers/specs/ | grep option-b-live` returns nothing; the file
does not exist in this worktree as of this writing. This document therefore
proceeds on its own best-grounded judgment (Decision 2, below) rather than
against that document's actual persistence/data shape, and names precisely
what to re-check once it lands (Open questions #1).

## Goal

Resolve the deferred doc's four concrete reasons for waiting into a real
design:

1. **What gates "does this station show full-coverage stats"** — the
   deferred doc's Decision 4 point 1 flagged this as genuinely unresolved:
   no `stations/*.toml`, no per-station catalogue row, nothing to hang a
   rollout flag on.
2. **Where the actual per-station full-coverage numbers come from** — the
   deferred doc's Decision 4 point 2: no settled producer contract existed
   to scaffold against (unlike the line-level case, which had
   `merge_dlr_sample_stats`'s already-proven hand-off shape to copy).
3. **The concrete wire/type shape** — extending
   `StationOperatorSampleStats`/`OperatorSampleStats` with a full-coverage
   analog, following the `LineStatus.full_coverage_stats`/
   `full_coverage_availability` precedent exactly, per this task's brief.
4. **A real dry-run path** — the deferred doc's Decision 4 point 4 flagged
   that no per-station flag could be manually exercised without first
   inventing a config surface. Decision 1 below resolves this as a
   byproduct of resolving point 1.

## Current relevant state (verified directly in this worktree, 2026-09-04)

**The line-level full-coverage scaffolding is real, merged, and running in
production today — in permanent shadow mode.** Not a sketch, not an
in-flight worktree: `common::FullCoverageAvailability`
(`crates/common/src/lib.rs:824-846`), `LineStatus.full_coverage_stats`/
`full_coverage_availability` (`lib.rs:374,376`),
`LineDefinition.full_coverage_enabled` (`lib.rs:498`), and
`aggregator::merge_full_coverage`/`merge_full_coverage_stats`
(`crates/aggregator/src/aggregation.rs:1177,1222`) all exist verbatim as the
line-level doc sketched them, `#[cfg(test)]`-covered
(`aggregation.rs:3447-3700`). Its only call site,
`crates/aggregator/src/main.rs:192`, is hard-wired to an empty map:

```rust
aggregation::merge_full_coverage(&mut reports, &lines, &HashMap::new(), defaults);
```

`LineDefinition.full_coverage_enabled` is `false` for every line in the
current catalogue (confirmed by its own doc comment, `lib.rs:496`: *"`false`
for every line in this repo's catalogue today -- nothing consumes this yet"*)
and by the frontend type's matching comment
(`frontend/lib/types.ts:88-89`: *"the ONLY value this can take -- nothing
sets `full_coverage_enabled` on any line yet, and no producer exists to
resolve `'pending'`/`'available'`"*). **This is the concrete, current shape
of "shadow mode": real plumbing, real types, real tests, zero live effect,**
and this document's own per-station scaffolding is designed to land in the
identical state — see Decision 4 below.

**A related, narrower scoping pass has already drawn a hard line around what
"build the consumer" is safe to do before Option B validates**:
`docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md` (read in
full) verdicts that only a pure, offline CIF schedule-parsing library
(`schedule_for_uid`/`schedules_touching`, no Kafka, no HTTP, no database
write, no wiring into any live path) is safe to build now; populating
`merge_full_coverage`'s map in production, for any line, "stays gated on
validation reaching 'go'" (that document's Verdict #1). **This document does
not change that boundary.** Everything below is presentation/read-path
scaffolding of the same permanently-inert kind the line-level doc already
established as safe to build ahead of a producer — not a live consumer, not
a new Kafka group, not a write into any real line's severity.

**The per-station sample-stats surface this document extends is real,
merged, and exactly as the per-station-stats-design doc shipped it**:
`crates/api/src/data/station_stats.rs`'s `compute_station_operator_stats`
and `OperatorSampleStats { operator, availability }`, and
`crates/api/src/routes/station_stats.rs`'s `GET
/public/stations/{crs}/sample-stats` handler. `frontend/lib/types.ts:138-142`'s
`StationOperatorSampleStats { operator, sampleAvailability, sampleStats? }`
matches. This document's Decision 3 extends these types additively, not
replacing them.

**`FullCoverageAvailability` already carries a reusable accessor** —
`impl FullCoverageAvailability { pub fn full_coverage_stats(&self) ->
Option<SampleStats> }` (`lib.rs:858-863`), extracting `Some(stats)` only from
the `Available` variant. This document's Decision 3 reuses it directly
rather than re-deriving the same match.

**`LineDefinition`'s fields relevant to Decision 1's gating mechanism**
(`lib.rs:456-498`): `operators: Vec<String>`, `stations: Vec<Station>`
(where `Station { crs: String, .. }`, `lib.rs:437-441`), and
`full_coverage_enabled: bool`. **`crates/api` already assembles the full,
live line list (catalogue + custom lines) as a plain `Vec<LineDefinition>`
at request time** — `crates/api/src/routes/samples.rs:23-25`:
```rust
let mut lines: Vec<LineDefinition> = app.config.lines.to_vec();
lines.extend(custom.into_iter().map(LineDefinition::from));
```
This is the exact list Decision 1's gating function reads; no new data
source is needed to reach it.

**The producer-to-`crates/api` write path for a live, per-station raw
signal already has a working precedent, twice over**:
`poller-ldbws` POSTs to `crates/api`'s `/private/station-samples`
(`crates/poller-ldbws/src/main.rs:3,389`), which
`upsert_station_samples` (`crates/api/src/data/queries.rs:258-`) writes
into `station_samples`, gated by `internal_oauth_group_ldbws`
(`crates/api/src/app.rs:91-100`). `latest_station_sample`
(`queries.rs:664-679`) is the matching read-time, single-CRS, no-history
query this document's Decision 2 mirrors exactly.

## Decisions

### 1. Config gating: derive from the station's covering lines' `full_coverage_enabled`, invent nothing new — resolves the deferred doc's Reason 1 and Reason 4

**No per-station config surface is added.** The deferred doc's Decision 4
point 1 correctly identified that inventing a `stations/*.toml`-equivalent
purely to hold a rollout flag would be backwards — the per-station-stats
design's own Decision 3 already declined to invent a per-station config
concept once, for threshold tuning, and this document does not become the
second, weaker reason to do it. Instead: **a (station, operator) pair's
full-coverage gate is computed dynamically, at read time, from whether any
line serving that operator at that station already has
`LineDefinition.full_coverage_enabled` set** — the exact flag the line-level
scaffolding already established as real, catalogue-authored, per-line
config.

This is not a new concept bolted onto stations; it is the observation that
"is full coverage enabled here" is already, structurally, a fact about
*lines*, and a station's answer to that question is just "the union over
every line that calls here" — the same relationship `poller-ldbws`'s own
`dedup_sample_stations` (`crates/api/src/data/samples.rs:11-23`) already
uses to derive *which stations to poll* from *which lines exist*, just
answering a different per-line-derived question.

```rust
// crates/api/src/data/station_stats.rs -- sketch, not final.

/// Whether a future full-coverage consumer would ever be expected to
/// resolve a signal for this (station, operator) pair -- derived
/// dynamically from `LineDefinition.full_coverage_enabled`
/// (`crates/common/src/lib.rs:498`), the SAME per-line rollout flag the
/// line-level scaffolding already established
/// (docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
/// Decision 3). No new per-station config surface: a station's gate is
/// the union, over every line that (a) runs this operator and (b) calls
/// at this CRS, of that line's own flag.
///
/// Route membership (`line.stations`), not `line.sample_stations`, is the
/// right membership check here -- deliberately wider than the LDBWS-only
/// sample-stats path uses. A full-coverage consumer's whole premise (per
/// the deferred doc's Decision 3 "granularity" argument) is "every
/// scheduled service on the line," not the curated 2-5-station LDBWS
/// subset, so gating on the curated list would under-scope exactly the
/// stations full coverage is meant to newly reach.
fn full_coverage_enabled_for(crs: &str, operator: &str, lines: &[LineDefinition]) -> bool {
    lines.iter().any(|line| {
        line.full_coverage_enabled
            && line.operators.iter().any(|op| op == operator)
            && line.stations.iter().any(|s| s.crs == crs)
    })
}
```

**This resolves the deferred doc's Reason 4 (no dry-run path) as a direct
byproduct, not a separate fix.** The line-level scaffolding's own dry-run
story was "an operator can flip `full_coverage_enabled` on one line in
`lines/*.toml` and manually construct a value to confirm the wire/frontend
path holds together" (deferred doc, Decision 4 point 4). Because this
document's gate reads that exact same flag, the identical manual exercise
now also lights up the per-station surface: flip `full_coverage_enabled` on
one line in `lines/*.toml`, and every (CRS, operator) pair covered by that
line's `stations` list flips from `NotEnabled` to `Pending` (or `Available`,
once a row exists per Decision 2) on the next request to
`GET /public/stations/{crs}/sample-stats` — no separate per-station toggle
needed, and nothing new to invent to exercise it.

**One real, flagged consequence of deriving the gate from route membership
rather than sample-station membership**: a station could show
`fullCoverageAvailability: { state: 'pending' | 'available' }` for an
operator even though that same station never appears in
`GET /public/stations/{crs}/sample-stats`'s LDBWS-derived rows at all today
(no `station_samples` row exists for it, per the sample-stats route's own
404 gate) — full coverage's route-membership reach is structurally wider
than LDBWS's `sample_stations`-only reach (deferred doc's own "Polling
scope" citation, inherited from the per-station-stats research doc). See
Decision 5 for how the route handler reconciles this.

### 2. Producer contract: a new `station_full_coverage_samples` table, written directly by Option B's future consumer, read at request time — best-grounded assumption, NOT a confirmed contract (see Open questions #1)

**Architectural constraint that shapes this decision**: per-station *sample*
stats are deliberately computed read-time in `crates/api` from raw,
directly-polled data (`station_samples`, Option C, per-station-stats-design
Decision 2) — `aggregator` never touches per-station data at all; only
`poller-ldbws` writes it, straight into `crates/api`. The line-level
full-coverage signal, by contrast, flows through `aggregator`
(`merge_full_coverage` merges an already-resolved per-line map onto
`LineStatus` before it's persisted to `line_status`). These are two
different existing architectures for two different existing grains — this
document keeps per-station full-coverage on the **read-time, Option-C**
side, mirroring the grain it's extending (per-station *sample* stats), not
switching it onto the aggregator-mediated line-level path.

**Concretely: a new table, `station_full_coverage_samples`, mirroring
`station_samples`'s shape and posture exactly, one level finer** (keyed by
`(crs, operator)`, since a full-coverage producer resolves per-operator
populations directly — TRUST's own `toc_id`, per the deferred doc's
Decision 1 — rather than requiring `crates/api` to re-filter a mixed board
by `operator` the way it does for `station_samples` today):

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_station_full_coverage_samples.sql
-- sketch, not final.
CREATE TABLE station_full_coverage_samples (
    crs         CHAR(3)     NOT NULL,
    operator    TEXT        NOT NULL,
    resolved_at TIMESTAMPTZ NOT NULL,
    stats       JSONB       NOT NULL,
    PRIMARY KEY (crs, operator)
);
```

**Written directly by Option B's future consumer to a new private ingestion
endpoint, mirroring `/private/station-samples`'s exact precedent** (not
routed through `aggregator`, which per Current relevant state never touches
per-station data):

```rust
// crates/api/src/routes/mod.rs -- sketch, not final, in private_router().
// mirrors station-samples' POST/GET pair (routes/samples.rs) exactly.
pub mod station_full_coverage_samples;
// .merge(station_full_coverage_samples::router())

// crates/api/src/app.rs -- sketch, not final, in build_internal_oauth_routes.
(
    "/station-full-coverage-samples",
    Method::POST,
    vec![config.internal_oauth_group_full_coverage.clone()], // new group, or reuse an existing one -- Open questions #4
),
```

**Read at request time, mirroring `latest_station_sample` exactly**:

```rust
// crates/api/src/data/queries.rs -- sketch, not final.

/// Full-coverage analog of `latest_station_sample` (queries.rs:664-679) --
/// same single-CRS, on-demand, no-history read, one level finer
/// (crs, operator). `station_full_coverage_samples` is wholesale-replaced
/// per producer resolution cycle for a given (crs, operator) row (same
/// posture as `station_samples`), NOT written by `aggregator` or
/// `poller-ldbws` -- written directly by Option B's future consumer once
/// it exists (not built here; see Explicitly out of scope), the
/// per-station analog of how `merge_full_coverage`'s per-line map
/// (`crates/aggregator/src/main.rs:192`) would be populated.
pub async fn latest_station_full_coverage_samples(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<StationFullCoverageSample>> {
    // SELECT crs, operator, resolved_at, stats
    // FROM station_full_coverage_samples WHERE crs = $1
    // -- sketch, not final.
}
```

```rust
// crates/common/src/lib.rs -- sketch, not final. One row per (crs,
// operator); a station's full result set is Vec<StationFullCoverageSample>
// filtered by crs, mirroring StationSample's own per-station grouping one
// level up.
pub struct StationFullCoverageSample {
    pub crs: String,
    pub operator: String,
    pub resolved_at: DateTime<Utc>,
    pub stats: SampleStats,
}
```

**Why this shape over the deferred doc's other sketched alternative** (a
`(station, operator, day)` accumulate-upsert rollup table, mirroring
`line_status_daily_coverage_stats`): the deferred doc's own Decision 4
reasoning against building a per-station *history* table applies here
unchanged — no confirmed product need for per-station full-coverage
*history* exists yet, and a live current-window row is cheaper, matches
Option C's already-established posture for the sample-stats sibling this
document extends, and costs nothing to widen into a rollup table later if a
need is confirmed (the same "cheap to extend later" argument the deferred
doc's own Reason 3 already made once for the read-time-vs-storage
question generally).

### 3. Wire/type shape: additive fields on the existing types, reusing `FullCoverageAvailability` verbatim — resolves this task's explicit ask

**No third sibling type.** The line-level doc's own Decision 1 reasoning for
introducing `FullCoverageAvailability` as a sibling to `SampleAvailability`
(rather than reusing it) was that `SampleAvailability::BelowThreshold`
encodes an LDBWS-specific station-count-threshold concept with no honest
full-coverage analog — a fact about the *producer's shape* (full coverage
has no "too few stations" state), not about *what grain* the number
describes. That reasoning is grain-independent: a per-(station, operator)
full-coverage signal has the identical three real states a per-line one
does (not enabled / enabled-but-not-yet-resolved / resolved) for the
identical reason. **`FullCoverageAvailability` is reused verbatim at
station grain, unchanged, not re-invented as a fourth type.**

```rust
// crates/api/src/data/station_stats.rs -- extended, sketch, not final.
use common::FullCoverageAvailability;

pub struct OperatorSampleStats {
    pub operator: String,
    pub availability: SampleAvailability,                       // UNCHANGED
    pub full_coverage_stats: Option<SampleStats>,                // NEW
    pub full_coverage_availability: FullCoverageAvailability,    // NEW
}
```

This is deliberately the exact same field pair, in the exact same shape,
`LineStatus` already carries (`full_coverage_stats: Option<SampleStats>` +
`full_coverage_availability: FullCoverageAvailability`, always both
present, one conditional) — the precedent this task's brief points at,
applied without modification.

**Wire (`crates/api/src/render.rs`), reusing the two existing `pub(crate)`
helpers `LineStatus`'s own rendering already uses, unchanged**:

```rust
// crates/api/src/routes/station_stats.rs -- extended, sketch, not final.
use crate::render::{full_coverage_availability_json, sample_availability_json, sample_stats_json};

// ...inside the .map(|s| { ... }) closure that builds each operator's JSON:
let mut out = json!({
    "operator": s.operator,
    "sampleAvailability": sample_availability_json(&s.availability),
});
if let Some(stats) = s.availability.sample_stats() {
    out["sampleStats"] = sample_stats_json(&stats);
}
if let Some(stats) = &s.full_coverage_stats {
    out["fullCoverageStats"] = sample_stats_json(stats);
}
out["fullCoverageAvailability"] = full_coverage_availability_json(&s.full_coverage_availability);
out
```

Zero new render helpers — `full_coverage_availability_json`
(`render.rs:120-128`) and `sample_stats_json`/`sample_availability_json`
(`render.rs:97-115`) already exist as `pub(crate)`, already used by
`status_to_json` for `LineStatus`, and slot into this route unchanged. This
is the same "hand-built JSON, no `#[serde(rename)]` nesting pitfall" posture
the per-station-stats design's own Decision 7 already established for this
route, extended rather than re-justified.

**Frontend (`frontend/lib/types.ts`), additive next to the existing
fields**:

```ts
// frontend/lib/types.ts -- extended, sketch, not final.
export interface StationOperatorSampleStats {
  operator: string;
  sampleAvailability: SampleAvailability;             // UNCHANGED
  sampleStats?: SampleStats;                           // UNCHANGED
  fullCoverageStats?: SampleStats;                     // NEW
  fullCoverageAvailability: FullCoverageAvailability;  // NEW, always present
}
```

`FullCoverageAvailability` (the TS type, `types.ts:91-94`) is reused as-is —
already exported, already exactly the three-state shape needed, no changes.

Example response, `GET /public/stations/EDB/sample-stats`, once a real
producer exists and one covering line has `full_coverage_enabled: true`
(entirely hypothetical today — see Decision 4):

```json
[
  { "operator": "GR", "sampleAvailability": { "state": "available" },
    "sampleStats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.4 },
    "fullCoverageStats": { "total": 52, "delayed": 6, "cancelled": 1, "skipped": 0, "avgDelayMinutes": 2.1 },
    "fullCoverageAvailability": { "state": "available" } },
  { "operator": "SR", "sampleAvailability": { "state": "below-threshold", "observed": 1, "required": 3 },
    "fullCoverageAvailability": { "state": "not-enabled" } }
]
```

### 4. Computation: group Option B's resolved matches by (station CRS, `toc_id`), gate per Decision 1, merge per Decision 3 — stays permanently inert until both a real flag and a real producer exist

Per the deferred doc's Decision 1 (unchanged, re-confirmed here, not
re-derived): TRUST's own `Activation.toc_id`/`Movement.toc_id`
(`crates/trust-consumer/src/schema.rs:41,66`) already self-declares
operator identity per message, so a future consumer does not need a CIF
schedule join just to learn which operator ran a given train — the same
structural fact LDBWS gives this app today via `StationDeparture.operator`.
Grouping a resolved-match population by `(station CRS via STANOX→CRS,
toc_id)` is therefore a direct, one-step derivation from data the consumer
already has once it exists, not a new correlation problem this document
introduces.

```rust
// crates/api/src/data/station_stats.rs -- extended, sketch, not final.

/// Extended per Decision 3/4: now also folds in Option B's future
/// full-coverage rows for this station (`full_coverage_rows`, already
/// filtered to `sample.crs` by the caller via
/// `latest_station_full_coverage_samples`) and the live line catalogue
/// (`lines`, for Decision 1's gate). `full_coverage_rows` is empty for
/// every station today -- no producer writes this table yet (Decision 2)
/// -- so every `full_coverage_availability` below resolves to `NotEnabled`
/// via `full_coverage_enabled_for` returning false for every line
/// (`full_coverage_enabled` is unset everywhere, per Current relevant
/// state), identically to how `merge_full_coverage`'s own empty-map call
/// site leaves every `LineStatus.full_coverage_availability` at
/// `NotEnabled` today.
pub fn compute_station_operator_stats(
    sample: &StationSample,
    defaults: &Defaults,
    full_coverage_rows: &[StationFullCoverageSample],
    lines: &[LineDefinition],
) -> Vec<OperatorSampleStats> {
    let operators: BTreeSet<&str> = sample.departures.iter().map(|d| d.operator.as_str()).collect();
    // Decision 1's gate additionally surfaces operators that have NO
    // current LDBWS departures at all but DO have a full-coverage row --
    // a real, if initially rare, case once full coverage reaches stations
    // LDBWS sampling never reaches (deferred doc's "Polling scope"
    // citation). Union, don't intersect:
    let operators: BTreeSet<&str> = operators
        .into_iter()
        .chain(full_coverage_rows.iter().map(|r| r.operator.as_str()))
        .collect();

    operators
        .into_iter()
        .map(|operator| {
            let relevant: Vec<&StationDeparture> =
                sample.departures.iter().filter(|d| d.operator == operator).collect();
            let availability = /* UNCHANGED from today's function */;

            let full_coverage_availability = if !full_coverage_enabled_for(&sample.crs, operator, lines) {
                FullCoverageAvailability::NotEnabled
            } else {
                match full_coverage_rows.iter().find(|r| r.operator == operator) {
                    Some(row) => FullCoverageAvailability::Available(row.stats.clone()),
                    None => FullCoverageAvailability::Pending,
                }
            };
            // Reuses the existing accessor (lib.rs:858-863) rather than
            // re-deriving the same match a second time.
            let full_coverage_stats = full_coverage_availability.full_coverage_stats();

            OperatorSampleStats {
                operator: operator.to_string(),
                availability,
                full_coverage_stats,
                full_coverage_availability,
            }
        })
        .collect()
}
```

**Skip-rate mapping**: unresolved, inherited unchanged from the deferred
doc's Open Question 2 — whatever a real Option B consumer's
`StationFullCoverageSample.stats.skipped` ends up meaning (TRUST `PASS`
events, per the line-level doc's own flagged-but-unconfirmed mapping) is
this document's assumed input, not independently re-derived here; see Open
questions.

### 5. Route handler: extend `GET /public/stations/{crs}/sample-stats` in place, not a new route

**No new route.** The 404-vs-`[]` gate (per-station-stats-design's Decision
7) is unchanged: a station with no `station_samples` row still 404s,
identically to today, *unless* it has full-coverage rows — see the next
paragraph. This is a small, additive extension to one existing handler, not
a second route to maintain — matching the deferred doc's own Reason 3
finding that this class of change is cheap once the base surface already
exists.

**One real interaction Decision 1 introduces, handled explicitly**: because
full coverage's route-membership gate is wider than LDBWS's
`sample_stations`-only reach, a station can have **zero** `station_samples`
row (no LDBWS coverage at all) while still having a real, resolved
`station_full_coverage_samples` row for some operator, once a producer
exists. The handler's 404 gate is widened to reflect this — 404 only when
**both** are absent:

```rust
// crates/api/src/routes/station_stats.rs -- extended, sketch, not final.
async fn get_station_sample_stats(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let sample = queries::latest_station_sample(&app.database, &crs).await.map_err(internal_error)?;
    let full_coverage_rows =
        queries::latest_station_full_coverage_samples(&app.database, &crs).await.map_err(internal_error)?;

    if sample.is_none() && full_coverage_rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no sample data collected for station: {crs}")));
    }

    let custom = custom_lines::list_custom_lines(&app.database).await.map_err(internal_error)?;
    let mut lines: Vec<LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(LineDefinition::from));

    let defaults = common::Defaults::default();
    let empty_sample = || common::StationSample { crs: crs.clone(), polled_at: Utc::now(), departures: vec![] };
    let stats = compute_station_operator_stats(
        &sample.unwrap_or_else(empty_sample),
        &defaults,
        &full_coverage_rows,
        &lines,
    );
    // ...unchanged JSON-building tail, per Decision 3.
}
```

This costs one extra query (`latest_station_full_coverage_samples`, mirrored
off an already-proven query shape) and one extra `lines` assembly
(mirrored verbatim off `routes/samples.rs:23-25`'s existing pattern) — no
new abstraction, no new failure mode beyond the ones the existing handler
and `get_sample_stations` already have individually.

### 6. Frontend rendering: extend the existing precedence helpers, not new components

`frontend/lib/sampleStats.ts`'s `SampleStatsCarrier` type (introduced by the
per-station-stats design's own Decision 9 specifically so both `LineStatus`
and `StationOperatorSampleStats` could share `sampleUnavailableReason`/
`formatSampleSummary`) widens the same way the line-level doc's Decision 1
already widened it for lines:

```ts
// frontend/lib/sampleStats.ts -- extended, sketch, not final.
type SampleStatsCarrier = {
  sampleStats?: SampleStats;
  sampleAvailability: SampleAvailability;
  dataQuality?: LineStatus['dataQuality'];
  fullCoverageStats?: SampleStats;             // NEW
  fullCoverageAvailability?: FullCoverageAvailability; // NEW, optional here
  // since StationOperatorSampleStats always sets it but this shared type
  // must still satisfy any future caller that doesn't
};
```

`statsUnavailableReason`'s existing precedence chain (line-level doc's
Decision 2: full-coverage → sample → tfl → sample-absence) already applies
unchanged once `StationOperatorSampleStats` structurally satisfies this
widened carrier — no new function, no new precedence rule to design, since
the chain was already written to be source-agnostic at the top.
`coverageProvenanceNote`'s existing confident-third-branch copy
(*"Based on real train-movement data..."*) likewise applies verbatim; no
station-specific copy variant is needed since the sentence never mentions
"line" or "station."

**One real, new state this document's Decision 1 introduces that the
line-level copy doesn't need to handle**: an operator row with
`fullCoverageAvailability.state !== 'not-enabled'` but *no*
`sampleAvailability`-derived departures at all (the zero-`station_samples`
case from Decision 5). The station page's existing three-state split
(per-station-stats-design Decision 9: not-sampled / sampled-but-quiet /
rows) needs a fourth, narrow case: "not LDBWS-sampled, but full-coverage
data exists for this operator" — flagged as a real, small, screenshot-level
UI decision, not designed to final copy here (see Open questions #5).

## Shadow-mode awareness — this stays as inert as the line-level scaffolding, by construction

Every layer of this design is built to be **doubly** inert until two
independent things both become true, mirroring the line-level scaffolding's
own single-condition inertness one level further:

1. **A real line's `full_coverage_enabled` flips to `true`** (currently
   `false` everywhere, per Current relevant state) — without this,
   `full_coverage_enabled_for` (Decision 1) returns `false` for every
   (station, operator) pair, and every `full_coverage_availability` in the
   response resolves to `NotEnabled`, identically to how every
   `LineStatus.full_coverage_availability` resolves to `NotEnabled` today.
2. **Option B's future consumer exists and writes real rows into
   `station_full_coverage_samples`** (Decision 2) — without this,
   `latest_station_full_coverage_samples` returns an empty `Vec` for every
   CRS, so even a station whose covering line *did* flip
   `full_coverage_enabled` on would show `Pending`, not `Available` — the
   exact "enabled, still resolving" state the line-level doc's own
   `FullCoverageAvailability::Pending` variant already exists to express,
   reused here unchanged.

**Both conditions are independently false today, for every station, and
this document does not propose changing either.** No new Kafka consumer,
no new production write path, no line's `full_coverage_enabled` flipped —
identical scope boundary to what `docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md`
already drew around the line-level case's own build-ahead-of-producer
posture (its Verdict #1), applied here without modification.

## Architecture

```
station page request
        │
        ▼
GET /public/stations/{crs}/sample-stats  (crates/api/src/routes/station_stats.rs)
        │
        ├─── queries::latest_station_sample(pool, crs)              ← UNCHANGED
        │       (Option<StationSample>; LDBWS-derived, per-station)
        │
        ├─── queries::latest_station_full_coverage_samples(pool, crs)  ← NEW
        │       (Vec<StationFullCoverageSample>; empty until Option B's
        │        future consumer exists and writes rows -- Decision 2)
        │
        └─── app.config.lines.to_vec() + custom_lines                ← existing
                assembly pattern (routes/samples.rs:23-25), reused for
                Decision 1's gate
        │
        ▼
station_stats::compute_station_operator_stats(sample, defaults,
                                               full_coverage_rows, lines)
        │  per operator: SampleAvailability (UNCHANGED) +
        │  FullCoverageAvailability, gated per Decision 1, merged per
        │  Decision 4's Option-B-analog grouping
        ▼
Vec<OperatorSampleStats>  →  hand-built JSON, reusing render.rs's existing
                              sample_stats_json/sample_availability_json/
                              full_coverage_availability_json unchanged
        │
        ▼
frontend: StationOperatorSampleStats (extended) → statsUnavailableReason /
          coverageProvenanceNote (both reused, unchanged) → page section
```

## Explicitly out of scope

- **Building Option B itself, its consumer, or any live TRUST-vs-schedule
  correlation.** Stays gated on validation reaching "go," per the base spec,
  validation-findings doc, and `2026-09-03-option-b-consumer-scoping.md`'s
  own explicit Verdict #1. This document only designs what this app does
  with that consumer's output once it exists.
- **The CIF `train_uid` → full booked-schedule bridge.** Named in the
  deferred doc as the real prerequisite under both line- and station-level
  full coverage; a pure, offline slice of exactly this
  (`schedule_for_uid`/`schedules_touching`) is being built separately per
  `2026-09-03-option-b-consumer-scoping.md`'s own scoped plan — unrelated to
  and not depended on by this document's own scaffolding, which only needs
  the *shape* of a future consumer's output, not the schedule bridge itself.
- **Any new or widened Kafka consumer group, or any wiring into
  `trust-consumer`'s live matching.** Zero network I/O is added by this
  design; `station_full_coverage_samples` is written by a producer that
  does not exist yet, exactly like `merge_full_coverage`'s map argument.
- **Flipping `LineDefinition.full_coverage_enabled` for any real line, or
  writing any real row into `station_full_coverage_samples`.** This
  document's scaffolding stays exactly as inert as the line-level
  precedent it extends.
- **A per-station history/rollup table** (the deferred doc's Decision 3
  "rollup table" alternative, `(station, operator, day)`-keyed). Decision 2
  picks the live-read shape instead, for the same reasons the deferred doc
  already gave against building per-station *history* ahead of a confirmed
  need; revisit only if that need is later confirmed, mirroring the
  per-station-stats research doc's own Option B posture for LDBWS data.
- **Whether Option B's eventual consumer actually externalizes
  per-(station, operator) granularity at all**, versus collapsing straight
  to a per-line rollup and discarding the finer intermediate result. This
  document assumes "yes, and in the shape Decision 2 sketches" as its
  working hypothesis — see Open questions #1, the single largest
  unconfirmed assumption in this document.
- **Final UI copy/screenshots for the new fourth station-page state**
  (Decision 6's "full-coverage-but-not-LDBWS-sampled" case). Flagged, not
  designed to pixel level.
- **Any change to the line-level full-coverage scaffolding itself**, or to
  `merge_full_coverage`'s existing behavior/call site.

## Open questions / risks

1. **This document's entire producer contract (Decision 2: a
   `station_full_coverage_samples` table, keyed `(crs, operator)`, written
   directly by Option B's consumer to a new private endpoint, read live) is
   an assumption, not a confirmed design.** It was built because
   `docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md` did
   not exist in this worktree as of this pass (confirmed by direct `ls`).
   **Once that document lands, re-check specifically**: (a) does the live
   consumer's actual persistence shape produce per-(station, operator)
   rows at all, or only a per-line materialized signal (in which case this
   document's entire Decision 2 needs replacing, not just tuning); (b) if
   it does produce per-station rows, what table/schema does it actually
   write them into, and does `crates/api` read them directly (as this
   document assumes) or through some other intermediary; (c) does it POST
   to `crates/api` the way `poller-ldbws`/`trust-consumer` do, or use a
   different ingestion shape (direct DB write from a shared schema,
   `aggregator`-mediated, something else) — this document's private-endpoint
   sketch in Decision 2 is a guess by analogy, not a confirmed shape.
2. **Whether Decision 1's route-membership gate (`line.stations`, not
   `line.sample_stations`) is the right membership check** — reasoned from
   first principles ("full coverage sees every calling point, not the
   curated LDBWS subset") but not validated against any real Option B
   output, since none exists.
3. **`Movement.toc_id`'s real-world reliability** (`Option<String>`,
   `#[allow(dead_code)]` today) — inherited unchanged from the deferred
   doc's Open Question 1; still unconfirmed against real TRUST traffic at
   volume.
4. **The exact internal-oauth-group shape for a new
   `/private/station-full-coverage-samples` ingestion endpoint** (Decision
   2's sketch invents `internal_oauth_group_full_coverage`) is a guess,
   not sized against how Option B's consumer will actually authenticate —
   plausibly it should reuse whatever group the line-level consumer's own
   future ingestion path uses instead of a new one, but that path doesn't
   exist yet either to check against.
5. **Decision 6's fourth station-page state (full-coverage-only, no LDBWS
   row) has no designed copy** — flagged, not a blocker, since it cannot
   actually render until both shadow-mode conditions (this document's
   "Shadow-mode awareness" section) are simultaneously true, which is not
   expected soon.
6. **Whether TRUST's `PASS` event type is the right analog for
   `SampleStats.skipped`** — inherited unchanged from both the deferred doc
   and the line-level doc's own identical, still-unresolved Open Question.
7. **The `min_sample_size`/threshold question the deferred doc's own
   Decision 3 sketch left open (Decision 3's "no per-station override
   mechanism") is unaffected by this document** — `full_coverage_stats` is
   attached whenever `FullCoverageAvailability::Available` resolves, with
   no threshold gate of its own (mirroring the line-level `merge_full_coverage`'s
   own "no threshold check, the producer already did that work" posture) —
   worth re-confirming once a real producer's actual resolution cadence is
   known, since "resolved this cycle" and "resolved with enough population
   to be meaningful" could, in principle, diverge for a station with very
   few scheduled services.
