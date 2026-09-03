# Design: Transitioning Sample-Hedged Metrics to Full-Coverage Metrics, Once Option B Ships

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md`
(the direct predecessor this document builds on top of — see Corrections
below for exactly how) and
`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md` (the
closest precedent for designing a metrics rollup layer with an explicit,
named honesty hedge). This document does not propose building
TRUST-vs-schedule delay inference itself (`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`'s
"Option B") — that stays gated on its own future validation/planning pass,
per `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`'s
repeated "not a confident verdict either way" conclusion, most recently
restated at that document's very end ("the remaining gap is genuinely just
sample size, not a broken mechanism"). This document assumes Option B
*eventually* ships and designs the migration path for what the app does
with its output once it exists. No implementation plan is included; that is
a separate, later step in this repo's process. Every sketch below (types,
schema, routes, copy) is marked as a sketch, not final code.

## Goal

Today, every rendering surface that shows delay/cancellation numbers
honestly hedges them as sample-derived — `"Too few live departures sampled
to report a rate right now."`, `"No live departure data received for this
line yet."`, and the Trends chart's own copy, `"Rates shown count each
distinct train once per day, based on its status the first time it was
seen that day — not a share of poll cycles."` These hedges are correct and
necessary today, because the underlying data really is a curated 2-5
station sample per line (`lines/*.toml`'s `sample_stations`, gated by
`min_sample_size`). If Option B is eventually validated and built, this app
will gain a second, structurally different producer of the same *kind* of
number — total/delayed/cancelled/skipped counts and an average delay — but
computed from every scheduled service on a line, cross-referenced against
real TRUST movement events, not a handful of polled stations. This document
designs: what type this new data takes, what changes on the wire and at
every existing frontend call site, what a confident (not hedged) badge/copy
looks like, how a line or segment can be in a genuinely mixed state during
a gradual per-line rollout, how the historical/trend rollup layer picks
this up, and how all of it sequences against the `SampleAvailability` work
this repo has *already shipped* since the brief for this document was
written.

## Corrections to the brief's assumptions (recorded for posterity)

Following this repo's established "Corrections" precedent: direct
inspection of the code turned up one thing the brief's framing did not
establish precisely, and it changes the shape of this whole document's item
5 substantially.

1. **`SampleAvailability` is not "not-yet-implemented" — it has already
   shipped, in full, exactly as
   `2026-09-01-line-status-sample-coverage-design.md` designed it.**
   Confirmed directly: `common::SampleAvailability`
   (`crates/common/src/lib.rs:718-747`) is a real, tested enum
   (`#[cfg(test)] mod sample_availability_tests`, `lib.rs:770-812`) with
   exactly the three variants that design proposed (`NoCoverage`,
   `BelowThreshold { observed, required }`, `Available(SampleStats)`).
   `common::LineStatus` (`lib.rs:339-350`) carries
   `sample_availability: SampleAvailability` as an always-present field,
   alongside the unchanged `sample_stats: Option<SampleStats>`. The
   aggregator computes it every cycle
   (`crates/aggregator/src/aggregation.rs:859-882`'s
   `compute_sample_availability`, called from both `infer_from_samples`,
   line 884, and `aggregate()`'s Layer 2 escalation branch, lines 89-107).
   It's on the wire (`crates/api/src/render.rs:73-78`, unconditionally
   present, unlike the still-conditional `sampleStats` key) and fully
   consumed on the frontend: `frontend/lib/types.ts:68-89` types it,
   `frontend/lib/sampleStats.ts`'s `sampleUnavailableReason` implements
   the exact `dataQuality`-before-`sampleAvailability` precedence rule that
   design specified, and every one of the six call sites that design
   catalogued (`AllLinesTable.tsx:21,106,237,245,260`,
   `LineStatusCard.tsx:8,13,48`, `stations/[crs]/page.tsx:11,141,147`)
   already routes through it. **This document does not design against a
   hypothetical future enum — it builds directly on top of a real, live
   one, and treats "does full-coverage data make parts of this moot"
   (this document's own item 5) as a question about already-shipped code,
   not speculative design.** See Decision 5.

   A second, smaller thing worth recording in the same vein: the DLR
   pilot (`crates/poller-tfl/src/main.rs`'s `merge_dlr_sample_stats`/
   `mark_dlr_pending`, lines 206-226) has *also* since been built on top of
   `SampleAvailability`, reusing `BelowThreshold`'s shape for a
   mechanically different producer (per-trip resolution warm-up, not a
   station-count threshold) — its own code comment cites this as "an
   honest, deliberately imperfect reuse," directly quoting the
   sample-coverage design's own Decision 4 and Open Question 2. This is
   *live evidence*, not a hypothetical, of exactly the failure mode
   Decision 1 below argues against repeating for full-coverage data: don't
   force a second, structurally different producer into the same enum
   shape a first one was designed for, however tempting the shape reuse
   looks at first glance.

2. **The historical rollup's own honesty hedge has also moved since the
   two Trends specs named in the brief were written, in a way that changes
   what "the same hedge, at the historical level" actually means today.**
   `2026-08-31-line-history-graphics-design.md`'s original hedge was "share
   of *sampled poll cycles*," explicitly named as an accepted limitation
   (its Decision 2) because the raw per-cycle counts double-count a train
   dwelling in Darwin's rolling window across many polls. That has since
   been fixed: `crates/aggregator/src/dedup.rs` (read in full) now
   deduplicates by Darwin `service_id` before the daily/half-hourly rollup
   ever sees a count, and `frontend/lib/types.ts:105-117`'s current
   `LineDailyStats` doc comment states the *current*, superseding framing
   plainly: "computed server-side from stored sums over DISTINCT trains,
   deduped by Darwin `service_id`... not a share of poll cycles. Each train
   is counted once per day, using its status the FIRST time it was
   observed that day." The rollup schema itself has also grown a second
   granularity since `2026-09-02-trend-chart-granularity-design.md` was
   written: `crates/api/migrations/20260902170000_line_status_hourly_stats_to_half_hourly.sql`
   renamed the hourly table to `line_status_half_hourly_stats` (30-minute
   buckets, not 60), and `crates/aggregator/src/main.rs:222-247` calls
   both `record_daily_stats` and `record_half_hourly_stats` per cycle, fed
   from the *same* deduped value. **This matters for Decision 4**: the
   remaining honesty gap at the historical level is no longer "per-cycle
   vs. per-train" (already closed) — it is "population" (curated
   `sample_stations` only, vs. every scheduled service on the line). This
   document's Decision 4 is written against that corrected, current gap,
   not the one the brief's citation of the two Trends specs implied.

## Current relevant state (verified 2026-09-03)

**`common::SampleStats`** (`crates/common/src/lib.rs:710-716`), unchanged
since both prior specs' own citations: `{ total: usize, delayed: usize,
cancelled: usize, skipped: usize, avg_delay_minutes: f64 }`. No provenance
field of any kind — a plain population count.

**`common::DataQuality`** (`lib.rs:280-294`): `Knowledgebase` (`#[default]`),
`LdbwsInferred`, `TrustInferred`, `Planned`, `Tfl`. `TrustInferred` carries
**no doc comment at all** — every other non-default variant either has one
(`Tfl`'s four-line explanation of why it isn't folded into `Knowledgebase`)
or is self-explanatory from its name in a way `TrustInferred` currently
is not, given nothing produces it. Confirmed by direct grep, repo-wide,
excluding its own definition and test fixtures: **`DataQuality::TrustInferred`
is constructed nowhere in this codebase** (`grep -rn "TrustInferred"
--include=*.rs crates/` returns only the enum definition, and the fixture
row in `crates/api/src/render.rs`'s own test module that exercises every
variant generically). This is the concrete evidence the brief asked for:
nothing in production reads or writes this variant today.

**`DataQuality::LdbwsInferred` is set narrowly, not whenever `sample_stats`
is populated.** Confirmed by reading `infer_from_samples`
(`crates/aggregator/src/aggregation.rs:884-975`): it is set only on the
path where `classify()` produces a non-`GoodService` severity from sample
data with no incident present (line 975); the early-return `GoodService`
branch (lines 916-920, hit whenever coverage is `NoCoverage`,
`BelowThreshold`, or genuinely quiet) leaves `data_quality` at whatever
`good_service()`'s own literal sets, which is itself `DataQuality::LdbwsInferred`
(`aggregation.rs:98` — `good_service()`'s own comment there notes this is
overwritten by every real caller in practice, `Knowledgebase`-precedence
statuses use their own literal). Separately, `escalate_from_sample_stats`
(`aggregation.rs:1098-1128`, Layer 2's escalation branch for a line that
already has an incident-derived status) **never touches `data_quality`** —
it only mutates `severity`/`reason`; the status's original
`Knowledgebase`/`Planned` provenance is preserved even when its severity
was escalated by live sample data. **This is the precise, load-bearing
precedent Decision 1/2 below extend to a TRUST-derived signal**: a
provenance tag marks *which system decided this status's severity*, not
merely *which system contributed a number to it*.

**Precedent for merging an externally-computed per-line signal onto a
`LineStatus`, without re-deriving it from raw per-station data inside
`aggregator`**: `crates/poller-tfl/src/main.rs`'s DLR arrivals-diffing
pilot. `merge_dlr_sample_stats` (lines 206-214) takes an already-computed
`common::SampleStats` (produced by a separate Arrivals-vs-Timetable
diffing pass, one fixed station) and sets it directly onto every status of
one line: `status.sample_stats = Some(stats.clone()); status.sample_availability
= SampleAvailability::Available(stats.clone());` — no threshold check, no
station-list filter, because the producer already did that work.
**This is a closer architectural precedent for how Option B's output
should reach `LineStatus` than `compute_sample_availability` is**, because
Option B's own recommended architecture
(`2026-08-29-trust-schedule-delay-inference-design.md`'s "Option B,"
recommended over Options A/C specifically for blast-radius reasons) is "a
new dedicated consumer... hand `aggregator` a per-line materialized signal
to consume as a third input" — i.e. `aggregator` reads an
*already-resolved, per-line* row, the same shape DLR's pilot already
demonstrates working in production, not a raw per-station map it must
itself filter and threshold the way `compute_sample_availability` does for
LDBWS.

**`common::LineDefinition`** (`lib.rs:434-460`) already has the exact
precedent shape for a new per-line opt-in TOML field: `severity_overrides:
HashMap<String, f64>`, `exclusive_segments: Vec<String>`, both
`#[serde(default)]`, both catalogue-authored per line. This is the natural,
already-established mechanism for a per-line `full_coverage_enabled`-style
flag (Decision 3) — a materially better fit than
`poller-tfl/src/config.rs`'s `dlr_pilot_enabled`, which is a single
*global* boolean gating one TfL mode, not a per-line catalogue setting;
Option B's own rollout is expected to cover a curated subset of lines
first (per the base spec's "proceed with caveats... not as a
straightforward replace/augment sampling project" recommendation), which
is structurally a per-line question, not a single on/off switch.

**`crates/aggregator/src/segments.rs`**'s `SegmentRegistry` (read in full)
already exists and is already used by `aggregate()`
(`aggregation.rs:47-51`'s signature) to resolve which lines/segments an
incident affects. This is the piece of existing infrastructure a future
segment-level Option B output would need to place a TRUST-vs-schedule
determination onto — confirmed present, not something this document
proposes building, and not itself extended by this document (see
Explicitly out of scope).

**The rollup layer's current write path** (`crates/aggregator/src/main.rs:202-247`,
read in full): once per line per cycle, `dedup::dedup_new_sample_stats`
produces a per-service-deduped `Option<SampleStats>`, fed identically into
both `queries::record_daily_stats` (`crates/aggregator/src/queries.rs:509-562`)
and `queries::record_half_hourly_stats` (`queries.rs:594-648`) — two
sibling accumulate-upsert calls against two sibling tables
(`line_status_daily_stats`, `line_status_half_hourly_stats`), both keyed
`PRIMARY KEY (line_id, <period>)`, one row per line per period. Read back
via `GET /Line/{id}/Stats/{from}/to/{to}` and (per the granularity spec) a
half-hourly sibling route, rendered by `frontend/app/lines/[id]/history/TrendsResults.tsx`
/ `HalfHourlyTrendsResults.tsx` through a shared, already-generalized chart
leaf, `TrendsCharts.tsx` (bucket-key-agnostic per that spec's Decision 9).

**Wire/frontend shape today** (re-confirmed, matching both cited prior
specs' own tables exactly, not re-derived): `frontend/lib/types.ts:76-90`'s
`LineStatus` carries `dataQuality`, `sampleStats?`, `sampleAvailability`
(always present). `frontend/components/IssueList.tsx:38-44`'s
`DATA_QUALITY_LABELS` already has an entry for every `DataQuality` variant
including the unused one: `'trust-inferred': 'Trust-inferred'` — a bare,
undecorated label with no accompanying explanatory copy, unlike the richer
prose `sampleUnavailableReason` gives the sample-absence cases.

## Decisions

### 1. Type/shape migration: reuse `SampleStats`'s shape verbatim; do not reuse `SampleAvailability`'s shape; add new, additive, non-`sample`-prefixed fields

**`SampleStats`'s five fields are source-agnostic — reuse the struct
unchanged.** "How many services, how many delayed, how many cancelled, how
many skipped a booked call, what's the average delay" is exactly as
meaningful computed from a TRUST-vs-schedule diff as from an LDBWS sample —
the struct's own doc comment already only says "sample-derived," not
anything about *how* the sample was taken. Reusing it (rather than
inventing a parallel `FullCoverageStats` struct with the same five fields)
means every existing pure formatter — `cancelledPercent`, the numeric
Avg-Delay/Cancelled columns in `AllLinesTable.tsx` — works unmodified
against the new field with zero new code, a genuine, mechanical win from
not renaming what doesn't need renaming.

One caveat, flagged rather than assumed: `skipped`'s current definition
(`stats_from_departures`, `aggregation.rs`) is specifically
"`d.skipped_stations` reported by Darwin" — an LDBWS-schema concept. TRUST
carries a `PASS` movement-event type distinct from `ARRIVAL`/`DEPARTURE`,
which is the structurally analogous "this service didn't stop where it was
booked to" signal — but this document does not independently confirm that
field-level shape (no code reads it yet; per the base spec's own "no
invented API details" convention, this is a plausible mapping, not a
confirmed one). Flagged in Open questions.

**`SampleAvailability` is *not* reused for the new signal — a sibling
type is introduced instead.** `SampleAvailability::BelowThreshold {
observed, required }` encodes vocabulary specific to LDBWS's station-count
threshold (`min_sample_size`). Per Correction 1, this repo has *already*
forced one structurally different producer (the DLR pilot's per-trip
resolution warm-up) into this exact shape, and its own code says so
explicitly, calling it "an honest, deliberately imperfect reuse." A
TRUST-vs-schedule consumer has an even less natural fit: there is no
"too few stations sampled" concept at all once every scheduled service is
in view by construction — the only real states are "not attempted for
this line" and "attempted, not yet resolved this cycle" and "resolved."
Forcing that into `BelowThreshold{observed, required}` a second time would
repeat Decision 4's already-flagged compromise at a second, wider blast
radius, for no shape-reuse benefit `SampleStats`'s reuse doesn't already
give for free.

```rust
// crates/common/src/lib.rs -- sketch, not final.

/// Why `full_coverage_stats` is (or isn't) populated -- the full-coverage
/// analog of `SampleAvailability`, deliberately a SIBLING type, not a
/// reuse of it. `SampleAvailability::BelowThreshold` encodes an
/// LDBWS-specific station-count threshold with no honest full-coverage
/// analog: a dedicated TRUST/schedule consumer either has resolved this
/// line's service population for the current window, or it hasn't --
/// there is no "too few observed, raise the threshold" state once every
/// scheduled service is structurally in view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum FullCoverageAvailability {
    /// No full-coverage producer exists for this line at all -- either
    /// Option B has never shipped, or it has shipped but this specific
    /// line has not been enabled for it yet (see Decision 3). The
    /// default, and the ONLY value this type can take until both are true.
    NotEnabled,
    /// Enabled for this line, but the consumer has not yet resolved a
    /// population for the current window this cycle -- a fresh
    /// deployment, a consumer outage, or (structurally different from
    /// LDBWS's per-cycle immediacy) TRUST resolution lag: a scheduled
    /// service with no Activation/Movement event seen yet, which this
    /// cycle cannot distinguish from "hasn't run yet."
    Pending,
    /// Resolved: every scheduled service on this line for the current
    /// window has been matched against real TRUST movement data. The
    /// payload is NOT duplicated a second time on the wire -- see
    /// Decision 1's render.rs note, mirroring `SampleAvailability::Available`'s
    /// own established precedent.
    Available(SampleStats),
}

impl FullCoverageAvailability {
    pub fn not_enabled_default() -> Self {
        FullCoverageAvailability::NotEnabled
    }
}
```

**`LineStatus` gains two new, additive fields — `sample_stats`/
`sample_availability` are UNCHANGED, not removed, not renamed:**

```rust
// crates/common/src/lib.rs -- sketch, not final.
pub struct LineStatus {
    // ...existing fields, unchanged...
    pub data_quality: DataQuality,                     // UNCHANGED
    pub sample_stats: Option<SampleStats>,              // UNCHANGED
    pub sample_availability: SampleAvailability,        // UNCHANGED
    pub full_coverage_stats: Option<SampleStats>,       // NEW
    #[serde(default = "FullCoverageAvailability::not_enabled_default")]
    pub full_coverage_availability: FullCoverageAvailability, // NEW, always present
}
```

`sample_stats`/`sample_availability` staying exactly as they are today is
not a temporary migration shim — it is a permanent decision (see Decision
3's "sample data as a permanent cross-check," and Decision 5): even a line
fully migrated to `FullCoverageAvailability::Available` keeps being LDBWS
sampled and keeps carrying real `sample_stats`, as a safety net.

**Wire shape** (`crates/api/src/render.rs`'s `status_to_json`, extending
its existing lines 63-78 pattern):

```rust
// sketch, not final.
if let Some(stats) = &status.full_coverage_stats {
    out["fullCoverageStats"] = json!({
        "total": stats.total, "delayed": stats.delayed,
        "cancelled": stats.cancelled, "skipped": stats.skipped,
        "avgDelayMinutes": stats.avg_delay_minutes,
    });
}
out["fullCoverageAvailability"] = match &status.full_coverage_availability {
    FullCoverageAvailability::NotEnabled => json!({ "state": "not-enabled" }),
    FullCoverageAvailability::Pending => json!({ "state": "pending" }),
    FullCoverageAvailability::Available(_) => json!({ "state": "available" }),
};
```

`normalize_for_diff` (both copies, `crates/aggregator/src/queries.rs` and
`crates/api/src/data/queries.rs`) must strip both new fields alongside the
existing `sample_stats`/`sample_availability` strip, for the identical
reason the sample-coverage design's own Error Handling section already
established for its pair: these fields change every cycle independent of
real disruption state, and would otherwise grow `line_status_history` a
row every cycle a line's live coverage count merely fluctuates.

**Frontend, additive next to the unchanged existing fields**
(`frontend/lib/types.ts`):

```ts
// sketch, not final.
export type FullCoverageAvailability =
  | { state: 'not-enabled' }
  | { state: 'pending' }
  | { state: 'available' };

export interface LineStatus {
  // ...existing fields, unchanged...
  sampleStats?: SampleStats;                          // UNCHANGED
  sampleAvailability: SampleAvailability;              // UNCHANGED
  fullCoverageStats?: SampleStats;                     // NEW
  fullCoverageAvailability: FullCoverageAvailability;  // NEW, always present
}
```

**Every existing call site, worked through concretely:**

| Surface | File:line | Change |
|---|---|---|
| `sampleStats.ts`'s core helper | `frontend/lib/sampleStats.ts` | `sampleUnavailableReason`'s precedence chain gains a new first check (see Decision 2); its `null`-return contract (real numbers exist, caller should render them) now also covers `fullCoverageStats` |
| `AllLinesTable.tsx` desktop Avg Delay / Cancelled columns | `AllLinesTable.tsx:228-245` | Prefer `status.fullCoverageStats` over `status.sampleStats` when both exist on the representative status — same numeric rendering, no new column |
| `AllLinesTable.tsx` mobile subtitle, station page subtitle | `AllLinesTable.tsx:224`, `stations/[crs]/page.tsx:98` | `formatSampleSummary` (renamed candidate: `formatStatsSummary` — a real rename, since "sample" is no longer accurate for every input it now formats; see Open questions on rename cost) routes through the same extended precedence |
| `LineStatusCard.tsx:47-51` | unchanged behavior (always renders per the sample-coverage design's own Decision 6), new copy sourced from the same helper | |
| `RepresentativeInfo.tsx:9-10` | `if (!withStats?.sampleStats) return null` extends to `if (!withStats?.sampleStats && !withStats?.fullCoverageStats) return null` — this card should show full-coverage numbers when they exist, not silently keep showing only the sample ones once a better number is available | |
| `IssueList.tsx`'s `DATA_QUALITY_LABELS` | line 41 | See Decision 2 — the label string stays, its surrounding treatment gains a confident branch |

### 2. `DataQuality` badge copy: `'trust-inferred'` gets a real doc comment and a genuinely confident third branch, not a fourth hedge

**Backend**: `DataQuality::TrustInferred` (`lib.rs:287`) gains the doc
comment its siblings already have, mirroring `Tfl`'s style:

```rust
// crates/common/src/lib.rs -- sketch, not final.
pub enum DataQuality {
    #[default]
    Knowledgebase,
    LdbwsInferred,
    /// Set only when a full-coverage TRUST-vs-schedule consumer (see
    /// docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md,
    /// "Option B") determined this status's severity with no active
    /// Knowledgebase incident present -- the direct analog of
    /// `LdbwsInferred` for a structurally more complete data source.
    /// Mirrors `LdbwsInferred`'s own narrow scope: escalating an
    /// incident-derived status's severity using full-coverage data
    /// (Decision 3's `escalate_from_coverage_stats`) does NOT set this --
    /// the escalated status keeps its original `Knowledgebase`/`Planned`
    /// provenance, exactly as `escalate_from_sample_stats` already
    /// preserves it today (`aggregation.rs:1098-1128`).
    TrustInferred,
    Planned,
    Tfl,
}
```

This is set by exactly the same narrow rule `LdbwsInferred` already
follows (Current relevant state, above) — not "whenever
`full_coverage_stats` is populated," but only when a full-coverage-derived
status *is* the line's determination (no incident present, and the
full-coverage classification itself produced a non-`GoodService`
severity). A line whose full-coverage stats are `Available` but read as
`GoodService` never sets it, same as `LdbwsInferred` never fires for a
quiet, sample-covered line today.

**Frontend badge**: `DATA_QUALITY_LABELS['trust-inferred']` (`IssueList.tsx:41`)
keeps its existing terse text, `'Trust-inferred'`, for consistency with
every other entry's brevity (`'LDBWS-inferred'`, `'Knowledgebase'`). What
changes is a new, third, genuinely *confident* branch in the reason/summary
helper — the task's explicit ask, and something today's two-hedge copy
(`"Not measured..."` / `"Too few live departures sampled..."`) has no room
for, because both existing strings are written specifically to avoid
overclaiming thin data:

```ts
// frontend/lib/sampleStats.ts -- sketch, not final.
// Precedence, in order: full-coverage available (new, most confident) ->
// TfL (structural, per Decision 1 of the sample-coverage design, reused
// unchanged) -> sample available/absent (existing, unchanged).
export function statsUnavailableReason(status: LineStatus): string | null {
  if (status.fullCoverageStats) return null;
  if (status.sampleStats) return null;
  if (status.dataQuality === 'tfl') {
    return "Not measured by this app — status is TfL's own.";
  }
  if (status.sampleAvailability.state === 'no-coverage') {
    return 'No live departure data received for this line yet.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}

/** NEW: a short, confident provenance line shown alongside real numbers
 * once full coverage exists -- the genuinely new, third branch this
 * document's brief asked for. Deliberately does not replace the numeric
 * rendering itself (Decision 1's table already routes fullCoverageStats
 * through the SAME numeric formatter sampleStats used); this is purely
 * the trust-building sentence next to it. */
export function coverageProvenanceNote(status: LineStatus): string | null {
  if (status.fullCoverageStats) {
    return 'Based on real train-movement data for every scheduled service on this line — not a live-departure sample.';
  }
  return null;
}
```

Rendered copy, added to the existing table (per-case, matching the
sample-coverage design's own established tone of naming *whose* data this
is rather than judging it):

- **Full coverage, resolved**: numbers render exactly as they do today
  (same `formatSampleSummary`-style "Avg delay N min · X% cancelled"
  string, now sourced from `fullCoverageStats`), with
  `coverageProvenanceNote`'s sentence surfaced as a tooltip/subtitle rather
  than replacing the numeric line — this is additive trust-signaling, not
  a second competing number.
- **Full coverage enabled, still `Pending`**: a genuinely new fourth state,
  distinct from both existing hedges — *"Full train-movement data is being
  resolved for this line — showing the live sample in the meantime."* —
  falls through to the existing sample numbers/copy underneath it, since
  `sample_stats` keeps being populated regardless (Decision 1). This is
  the honest "upgrading, not yet upgraded" state a gradual per-line
  rollout needs and neither existing hedge string can express.
- **`NotEnabled`**: identical to today, zero new copy — this is the
  overwhelming majority case until Option B ships and a line is enabled
  for it, and must render exactly as it does now, not as a fourth
  permanently-visible "not yet" message on every line.

### 3. Mixed-state / gradual rollout: attach the new fields per-`LineStatus`, not per-line-report; gate rollout with a new per-line TOML flag; keep sample data running forever as a cross-check

**Per-status attachment, not per-report, because a line's `statuses` array
already carries multiple simultaneous entries.** `LineStatusReport.statuses:
Vec<LineStatus>` already supports a line having, say, one incident-derived
status covering a segment and a separate `GoodService` status for the rest
— `IssueList.tsx` already iterates this array and renders one
`DataQuality` badge per entry (`IssueList.tsx:374`). Attaching
`full_coverage_stats`/`full_coverage_availability` at the same granularity
`sample_stats`/`sample_availability` already use means a **structurally
real mixed state is representable with zero new plumbing** at the
per-incident detail level: one status entry can read `TrustInferred` while
a sibling entry on the same line's report still reads `LdbwsInferred`, and
`IssueList` already renders both correctly once Decision 2's label/copy
land — no new component, no new prop. This is a genuine, concrete finding:
**segment-level mixed-state UI is mostly already built**, for the
surfaces that already iterate the full statuses array; it is *not* solved
for the line-summary surfaces (below), which is a real, separate gap.

**Per-line rollout gate, not a single global flag.** Per Current relevant
state, `LineDefinition` (`lib.rs:434-460`) already has the exact precedent
shape (`severity_overrides`, `exclusive_segments` — both per-line,
catalogue-authored, `#[serde(default)]`). This document proposes the same
pattern, deliberately **not** `poller-tfl`'s `dlr_pilot_enabled`
single-global-boolean shape, because Option B's own rollout is expected to
cover a curated subset of lines first, not flip on for the whole national-rail
catalogue at once:

```rust
// crates/common/src/lib.rs -- sketch, not final, added to LineDefinition.
/// Opt-in per line, catalogue-authored -- mirrors `severity_overrides`'s
/// existing per-line-TOML-field precedent. Gates whether Option B's
/// future consumer even attempts this line, not merely whether its
/// result is shown once resolved -- so a line's
/// `full_coverage_availability` genuinely stays `NotEnabled` (not
/// `Pending` forever) until this flag is set, distinguishing "not
/// rolled out to yet" from "rolled out, still resolving."
#[serde(default)]
pub full_coverage_enabled: bool,
```

**Presentation, worked through per surface — the task's explicit ask.**
There is a real split between surfaces that already show per-status detail
and surfaces that reduce a whole line to one summary:

- **`IssueList.tsx` (station page and line detail page)**: already
  correct, per above, once Decision 2's label lands — no design work
  needed beyond that.
- **`AllLinesTable.tsx`, `LineStatusCard.tsx`, the pinned-station
  dashboard row, `stations/[crs]/page.tsx`'s subtitle**: all four reduce a
  line's several simultaneous statuses to one "representative" status via
  `representativeStatus` (`sampleStats.ts`), whose current rule
  (`statuses.find((s) => s.sampleStats) ?? statuses[0]`) has no concept of
  full coverage at all. **Extend the same precedence pattern one more
  step**, preferring full coverage over sample over plain-first:

  ```ts
  // frontend/lib/sampleStats.ts -- sketch, not final.
  export function representativeStatus(statuses: LineStatus[]): LineStatus | undefined {
    return (
      statuses.find((s) => s.fullCoverageStats) ??
      statuses.find((s) => s.sampleStats) ??
      statuses[0]
    );
  }
  ```

  This is a genuine, honest simplification for a summary row, not a loss
  of information: a one-line dashboard card was never going to show a full
  per-segment breakdown regardless of data source, and preferring the more
  complete number when one status on the line has it (mirroring
  `escalate_from_sample_stats`'s existing "prefer worse-but-more-informed"
  posture) is strictly more correct than today's arbitrary first-with-stats
  rule. **What this does *not* solve**: a line where segment A is
  full-coverage-confirmed-quiet and segment B is sample-derived-and-delayed
  would show segment B's (worse, still real) status as representative —
  correct today too (severity is already the row's primary sort signal,
  not data quality), and this document does not change that ordering.
- **True segment-level mixed-state summarization** (e.g., a line detail
  page showing "this stretch: TRUST-confirmed; that stretch: still sampled")
  would need a genuinely new UI affordance this document does not design —
  flagged in Open questions, since it depends on segment-level status
  entries becoming more granular than the current line-wide
  `infer_from_samples`/its future full-coverage analog produce by default
  (per the base spec's own scoping: segment precision is "the strongest,
  clearest case" for Option B, but *producing* it is Option B's own future
  design problem, not this document's).

**Sample data stays computed and populated forever, even for a fully
migrated line — a deliberate, permanent decision, not a transitional
shim.** Two reasons, both grounded in this repo's own already-established
posture elsewhere: (1) it is the exact same "keep the old source as a
fallback/cross-check indefinitely, not time-limited" pattern the
schedule-ingest STANOX/CRS design already committed to for its own CSV
fallback (`2026-09-01-schedule-ingest-stanox-crs-table-design.md`
Decision 3: "kept, not deleted... indefinitely, not as a time-limited
migration step"); (2) `escalate_from_sample_stats` remains a real,
independent safety net — if a future full-coverage pipeline silently
degrades (a TRUST/schedule feed outage that resolves to stale `Available`
data rather than flipping to `Pending`), a disagreeing live sample can
still catch it. This document proposes a symmetrical
`escalate_from_coverage_stats`, using full-coverage data with strictly
higher trust than sample data when both are present and disagree
(`severity_rank`-max of the two escalation outcomes), but never
demoting below whatever Knowledgebase/Planned already established —
identical escalate-only posture to today's rule, one level stronger.

### 4. Trends/rollup layer: the honesty gap has shifted from measurement frequency (already fixed) to population coverage — needs a sibling rollup table, not a shared/discriminated one

Per Correction 2, the "share of poll cycles" hedge this document's brief
expected to still need fixing was **already fixed** by the per-service
dedup ledger — the current honesty copy is accurate about *how* a train is
counted (once, at first sighting). What full coverage changes is *which*
trains get counted at all: today's rollup only ever sees trains passing
through a line's curated `sample_stations`; a full-coverage rollup would
see every scheduled service on the line. **This is a population-coverage
gap, not a re-run of the measurement-frequency problem the two Trends
specs already solved** — worth stating plainly since the brief's own
framing (citing both specs for "the identical hedge, at the historical
level") slightly overstates the overlap.

**A new, sibling pair of tables — not a `source` column added to the
existing ones.** `line_status_daily_stats`/`line_status_half_hourly_stats`
each have `PRIMARY KEY (line_id, day)` / `PRIMARY KEY (line_id,
half_hour_start)` — one row per line per period. During a gradual per-line
rollout (Decision 3), a line can genuinely have *both* a real sample-derived
number and a real full-coverage number for the same overlapping period —
these are two different populations, not two measurements of the same
population, and summing or overwriting one with the other in a single row
would silently misrepresent both. A `source`-discriminated composite key
(`(line_id, day, source)`) would work but changes an existing table's
primary key underneath consumers that currently assume one row per
`(line_id, day)` (`daily_stats_for_range`,
`crates/api/src/data/queries.rs:683-720`, and `TrendsResults.tsx`'s own
`toChartPoints`). A wholly new, additive sibling table avoids that:

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_line_status_daily_coverage_stats.sql
-- sketch, not final. Column-for-column identical to
-- line_status_daily_stats (20260831090001) -- same accumulate-upsert
-- shape, same rate-derived-at-read-time posture -- fed from a
-- structurally different population (every scheduled service, not the
-- curated sample_stations), so kept as a genuinely separate table rather
-- than a source-discriminated row in the existing one (see Decision 4).
CREATE TABLE line_status_daily_coverage_stats (
    line_id            TEXT             NOT NULL,
    day                DATE             NOT NULL,
    resolved_windows    BIGINT          NOT NULL DEFAULT 0, -- the full-
                                                              -- coverage
                                                              -- analog of
                                                              -- sample_cycles
                                                              -- -- how many
                                                              -- cycles this
                                                              -- day saw
                                                              -- Available
                                                              -- (not Pending)
    total               BIGINT          NOT NULL DEFAULT 0,
    delayed              BIGINT         NOT NULL DEFAULT 0,
    cancelled            BIGINT         NOT NULL DEFAULT 0,
    skipped              BIGINT         NOT NULL DEFAULT 0,
    running_count        BIGINT         NOT NULL DEFAULT 0,
    delay_minutes_sum    DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (line_id, day)
);
```
(and a sibling `line_status_half_hourly_coverage_stats`, identical shape,
mirroring the existing daily/half-hourly pair exactly).

**Read path**: a new route, `GET /Line/{id}/Stats/Coverage/{from}/to/{to}`
(and a half-hourly sibling), mirroring `daily_stats_for_range`'s exact
shape and rate-derived-at-read-time posture. **Frontend rendering is a
genuinely open UI-taste question this document does not settle**: whether
a line with both real sample and real full-coverage rows for the same
range should render as two separate chart series (labelled distinctly),
as one series that switches source mid-range (visually marking the
transition point), or with the sample series simply hidden once
full-coverage rows exist for that range — flagged in Open questions,
deliberately not decided here, matching this repo's own established
posture of leaving exact chart-UI calls to an implementation-time pass
(`2026-09-02-line-history-chart-fixes-design.md`'s own precedent for
leaving pixel-level decisions open). What *is* decided: `TrendsCharts.tsx`'s
already-generalized, bucket-key-agnostic chart leaf
(`2026-09-02-trend-chart-granularity-design.md` Decision 9) is directly
reusable for whichever rendering choice is made — no third fork of the
chart component is needed regardless of which UI answer is picked.

**Honesty copy for the new series, genuinely different from the existing
one, not a copy-paste with "trains" swapped for "services":**

```
Rates shown cover every scheduled service on this line, cross-referenced
against real train-movement data — not a sample of live departures at a
handful of stations.
```

This drops the population-limitation hedge entirely (there is none left to
state) while keeping the same register/precision the existing copy already
established. The existing sample-rollup copy is **unchanged** for periods
still sample-only.

**Sparse/gap handling needs its own, separately-calibrated signal, not a
reuse of `SPARSE_DATA_FLOOR_CYCLES`.** That constant is calibrated against
LDBWS poll-cycle coverage; the full-coverage analog is `resolved_windows`
(how many cycles this day/half-hour actually had `Available`, not
`Pending`, data) — a day dominated by `Pending` (TRUST resolution lag)
should render as a gap in the coverage series, using the exact same
gap-rendering machinery (`gapSpans`/`referenceAreaBounds`,
`TrendsCharts.tsx`) already generalized for the daily/hourly split, with a
new, separately-derived floor value — not designed to a specific number
here, same posture the granularity spec itself took for its own new floor.

### 5. Sequencing against the (already-shipped) `SampleAvailability` work

Per Correction 1, this is not a "if X ships first" hypothetical — `X` has
already shipped. This document's design builds directly on top of it:

- **What is reused, unchanged**: `SampleAvailability` itself, its wire
  shape, `sampleUnavailableReason`'s existing `dataQuality`-before-
  `sampleAvailability` precedence rule (Decision 2 above only *prepends* a
  new check, it does not restructure the existing one), every existing
  frontend call site's routing through the shared helper, and the DLR
  pilot's own (already-shipped) reuse of `BelowThreshold`. None of this is
  touched, replaced, or deprecated by this document.
- **What becomes structurally moot, specifically for a line/segment once
  it graduates to `FullCoverageAvailability::Available`, and only then**:
  `SampleAvailability`'s own central unsolved ambiguity — the
  `2026-08-31-sample-data-availability-design.md` taxonomy's case 2a
  ("genuinely quiet") vs. case 2b ("structurally under-sampled"), which
  that spec's Decision 3 deliberately left unresolved within a single live
  cycle and deferred to `sample_cycles`' pattern-over-time — **cannot
  recur** for a fully-covered line, because a full-coverage signal has no
  "too few curated stations" concept at all; every scheduled service is
  inherently in view. This is a genuine, concrete benefit of Option B
  beyond raw confidence: it doesn't just answer the 2a/2b question better,
  it retires the question entirely for the lines it covers. Framed
  precisely: `SampleAvailability::BelowThreshold`'s occurrences don't stop
  happening (the app still runs LDBWS sampling forever, per Decision 3),
  they simply stop being the *only* signal available for that line, so a
  viewer sees the confident full-coverage number instead of ever having to
  interpret the ambiguous one.
- **What is untouched and remains equally relevant regardless of Option
  B's fate**: `2026-08-31-sample-data-availability-design.md`'s own
  still-open Decision 2 proposal (a global `stationSamples` timestamp on
  `/public/freshness`, answering LDBWS pipeline *staleness*) — this is
  about whether the LDBWS pipeline itself is healthy, a question that
  matters exactly as much on a line with full coverage (per Decision 3's
  "sample data as a permanent cross-check") as on one without it. This
  document does not build it, and does not consider it superseded.
- **Scoping stays identical**: Case 1 (TfL-quality lines) is completely
  outside this document's scope, exactly as the base spec scoped Option B
  to national-rail lines only. Nothing here proposes extending
  `FullCoverageAvailability` to TfL modes.

## Architecture

```
                    Option B's future dedicated consumer
                 (NOT this document's job to build -- gated
                  on its own future validation/planning pass)
                                    │
                                    │ per-line materialized signal, once
                                    │ validated + `full_coverage_enabled`
                                    ▼
┌──────────────────────────────────────────────────────────────────────┐
│ crates/aggregator                                                       │
│                                                                          │
│  Layer 1: Knowledgebase incidents        UNCHANGED -- always wins       │
│  Layer 2: LDBWS sampling                 UNCHANGED (Decision 3:         │
│           compute_sample_availability     permanent cross-check,        │
│           infer_from_samples              not superseded)               │
│           escalate_from_sample_stats                                    │
│  Layer 3 (NEW): full coverage             merge_full_coverage (analog   │
│           read: per-line materialized     of merge_dlr_sample_stats,    │
│           row from Option B's consumer,   NOT compute_sample_availability│
│           gated on full_coverage_enabled  -- the producer already       │
│           escalate_from_coverage_stats    resolved the population)      │
│                                            never demotes below           │
│                                            Knowledgebase/Planned         │
│                                                                          │
│  LineStatus { ..., sample_stats, sample_availability   UNCHANGED,       │
│               full_coverage_stats, full_coverage_availability  NEW }    │
└────────────────────────────────┬────────────────────────────────────────┘
                                  │ record_daily_stats / record_half_hourly_stats
                                  │ UNCHANGED (sample-derived, per-service-deduped)
                                  │        +
                                  │ record_daily_coverage_stats /
                                  │ record_half_hourly_coverage_stats  NEW,
                                  │ sibling tables, NOT a source column on the
                                  │ existing ones (Decision 4)
                                  ▼
┌──────────────────────────────────────────────────────────────────────┐
│ crates/api                                                              │
│  render.rs: sampleStats/sampleAvailability UNCHANGED (always present   │
│             for sampleAvailability, conditional for sampleStats)       │
│             fullCoverageStats/fullCoverageAvailability NEW, same shape │
│  GET /Line/{id}/Stats/{from}/to/{to}            UNCHANGED               │
│  GET /Line/{id}/Stats/Coverage/{from}/to/{to}   NEW, sibling route      │
└────────────────────────────────┬────────────────────────────────────────┘
                                  ▼
┌──────────────────────────────────────────────────────────────────────┐
│ frontend/lib/sampleStats.ts                                             │
│  sampleUnavailableReason -> statsUnavailableReason (extended            │
│    precedence: fullCoverageStats > sampleStats > tfl > sample-absence)  │
│  representativeStatus (extended: prefer fullCoverageStats status)       │
│  coverageProvenanceNote  NEW -- the confident third copy branch         │
│                                                                          │
│  Already-correct, no change needed: IssueList.tsx (per-status detail,   │
│  DATA_QUALITY_LABELS['trust-inferred'] already wired, gains real copy)  │
│                                                                          │
│  TrendsCharts.tsx's generalized, bucket-key-agnostic leaf REUSED for a  │
│  new coverage series/table (UI treatment of mixed sample+coverage       │
│  ranges left open -- Decision 4)                                       │
└──────────────────────────────────────────────────────────────────────┘
```

## Explicitly out of scope

- **Building Option B itself** (the TRUST-vs-schedule consumer, its
  matching logic, its Kafka consumer group, its own escalation
  thresholds). Stays gated on its own future validation/planning pass, per
  the base spec and validation-findings documents' repeated "not yet"
  verdicts. This document only designs what happens to the app's existing
  metrics surfaces *given* that output, once it exists.
- **Segment-level `LineStatus` granularity itself.** This document's
  Decision 3 shows that *once* segment-level statuses exist, the
  per-status attachment already handles mixed-state presentation for
  detail surfaces with zero new plumbing — but producing genuinely
  segment-scoped statuses (rather than one line-wide determination) is
  Option B's own future architecture question, not designed here.
- **A true cross-line/cross-source mixed-state summary UI** (e.g., a map
  or line-detail view showing exactly which stretch of a line is
  TRUST-confirmed vs. still sampled). Flagged as a real, larger follow-up
  in Decision 3, not designed to a final answer.
- **Per-station full-coverage stats.** The brief notes a second,
  concurrent research effort into per-station stats specifically — this
  document deliberately stays scoped to the line-level `LineStatus`
  surfaces (matching `SampleStats`/`SampleAvailability`'s own existing
  line-level scope) and does not attempt to also design the station-level
  transition, to avoid duplicating or preempting that parallel work.
- **Deleting or deprecating `sample_stats`/`sample_availability`, for any
  line, ever.** Decision 3 makes this explicit and permanent, not a
  time-limited migration step — mirroring this repo's established posture
  for the STANOX/CRS CSV fallback.
- **The exact rename of `formatSampleSummary`/`sampleUnavailableReason` to
  a source-agnostic name** (e.g. `formatStatsSummary`). Flagged as a real,
  non-trivial breaking rename affecting several already-tested call sites
  (the sample-coverage design's own Open Question 5 already named this
  exact cost once for a smaller signature change) — named as a probable
  need, not committed to a final name here.
- **Any change to `min_sample_size`, `severity_overrides`, or LDBWS
  classification thresholds.** This document is entirely about what a
  second, more complete data source does alongside the existing one, not
  about tuning the existing one.
- **`/public/freshness` extensions for full-coverage pipeline health**
  (an analog of the still-open `stationSamples` proposal, for Option B's
  own consumer). Real, plausible, not designed here.

## Open questions / risks

1. **Whether `skipped`'s TRUST-side analog (a `PASS` movement event) is
   actually the right mapping onto `SampleStats.skipped`'s existing
   meaning is not confirmed** (Decision 1) — no code reads TRUST's event
   types for this purpose yet; this is a plausible mapping reasoned from
   domain knowledge, not verified against a real message.
2. **The exact rename cost of `sampleStats.ts`'s public functions**
   (`sampleUnavailableReason` → `statsUnavailableReason`,
   `formatSampleSummary` → a source-agnostic name) is real and not fully
   scoped here — every existing call site and test file would need
   updating in the same change, the same category of cost the sample-
   coverage design's own Open Question 5 flagged for a smaller rename.
3. **Whether a line in the `Pending` full-coverage state should surface
   any indication on the line-summary surfaces (`AllLinesTable`, etc.), or
   only in the detail-level copy Decision 2 proposes**, is a real product
   call this document doesn't settle — showing "upgrading" status on a
   dashboard row for every line mid-rollout could be noisy; showing
   nothing could look like the rollout silently stalled.
4. **The UI treatment for a chart range spanning both sample-only and
   full-coverage periods (Decision 4) is deliberately left open** — three
   plausible shapes were named (separate series, one series with a marked
   transition, sample series hidden once coverage exists) with no
   recommendation between them; this needs a real screenshot-driven pass
   once Option B is closer to real.
5. **`escalate_from_coverage_stats`'s exact precedence rule when sample
   and full-coverage data disagree** (Decision 3 proposes "higher
   `severity_rank` wins, escalate-only, never below Knowledgebase") is a
   reasoned default, not validated against any real disagreement case,
   since no such case can exist until Option B produces real data.
6. **Whether `full_coverage_enabled` should eventually support
   segment-level (not just whole-line) opt-in**, mirroring
   `exclusive_segments`'s existing per-segment shape on the same struct,
   is a natural extension this document names but does not design,
   pending Option B's own segment-level architecture actually existing.
7. **This document's naming (`FullCoverage*`, `full_coverage_enabled`)
   is this document's own best attempt, not reviewed against any
   product/copy process** — same posture every cited prior spec already
   takes with its own proposed names and strings.
