# Design: Sample Coverage — Distinguishing Why Line Stats Are Absent, on the Lines List/Detail UI

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-sample-data-availability-design.md` (the
direct predecessor to this document — same underlying ambiguity, same
session's research, see Corrections below) and
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` (whose
"no new `DataQuality` variant" decision this document explicitly considers
and accepts, again see Corrections). No implementation plan is included;
that is a separate, later step in this repo's process.

## Goal

On `frontend/app/lines/AllLinesTable.tsx`'s Avg Delay / Cancelled columns
and `frontend/app/lines/[id]/page.tsx`'s line-detail view, a line with no
`SampleStats` renders as a bare `"—"` — identical whether the line is a
Tube line this app structurally never samples, a brand-new custom line
whose stations have never once been polled, or a normal National Rail line
that was polled fine this cycle and simply has too few live departures
right now. This document designs a concrete, shippable fix for two of the
three conflated situations — (1) TfL-quality lines, and (2) a real
"no live data received at all" gap — while explicitly scoping the third
(genuinely-zero vs structurally-thin, and the deeper "nothing is scheduled
right now" question) as follow-up work this document does not solve, with
reasoning for why not.

## Corrections / relationship to prior specs

Following this repo's established "Corrections" precedent
(`2026-08-29-journey-ticket-tracking-frontend-design.md`,
`2026-09-01-tracked-trains-home-page-design.md`): this document sits on top
of two prior decisions and is explicit about which it accepts unchanged and
which it knowingly revisits.

**1. Accepted unchanged: `2026-08-22-tfl-service-metrics-v2-design.md`'s
"no new `DataQuality` variant" decision (its Non-goals, "none of the four
areas need one").** This document does not add one either. `DataQuality`
answers "which system is authoritative for this status's severity"; the
question this document answers — "why is sample-derived delay/cancellation
data absent" — is a different axis, and conflating them would be exactly
the trap `DataQuality::Tfl`'s own doc comment (`crates/common/src/lib.rs:288-291`,
confirmed by reading) already warns against for a related reason ("not
folded into `Knowledgebase` — that name means the National Rail RDM
Knowledgebase feed specifically"). This document proposes a new,
independent type for the sample-provenance axis instead (Decision 2) —
that is a revisit of a *different* prior decision (below), not of this one.

**2. Knowingly revisited, narrowly:
`2026-08-31-sample-data-availability-design.md`'s Decision 2, the part that
rejected a structured `LineStatus`-level "why is `sample_stats` absent"
enum.** That document's own Correction 3 is the load-bearing finding:
*"a station whose row doesn't exist yet (never polled) is silently skipped,
identically to a station whose row exists but is hours stale"*
(`crates/aggregator/src/aggregation.rs:682-692`'s `relevant_departures`,
confirmed by re-reading it this session — see Current relevant state).
Decision 2 then reasoned about "2c" (a genuine gap) as a **single** problem
spanning both never-polled and stale-but-present rows, concluded that
telling it apart from "genuinely quiet" (2a) or "structurally under-sampled"
(2b) needs pattern-over-time data a single aggregation cycle can't produce,
and rejected a structured field for that whole combined question — settling
instead for one *global*, pipeline-wide freshness timestamp
(its Decision 2's proposed `stationSamples` field on `/public/freshness`,
**not implemented as of this reading** — confirmed no such field exists yet
in `crates/api/src/routes/freshness.rs`) plus a frontend-only value-driven
rule for case 1 (its Decision 1).

This document's finding: that reasoning correctly rejected a *staleness*-
aware enum (which does need a `polled_at` comparison and a "how stale is
too stale" threshold — real, unsolved questions), but it bundled a much
cheaper, single-cycle-computable question in with it: **not "is this
station's data fresh," but "does this station have any `StationSample` row
in this cycle's load at all."** That narrower question needs no new query,
no staleness threshold, and no pattern over time — it is answerable from
exactly the same `HashMap<String, StationSample>` `relevant_departures`
already holds, just checked before the operator-filtering/threshold step
discards the distinction. This document builds *only* that narrower piece
(Decision 2 below) as a real, per-line, per-cycle `LineStatus` field. It
explicitly does **not** revisit the staleness half of 08-31's Decision 2 —
`stationSamples` on `/public/freshness` remains exactly as proposed there,
unimplemented, and out of scope here (see Explicitly out of scope). The two
signals are complementary, not overlapping: this document's field says
*which lines* have no data this cycle; 08-31's still-open field would say
*when* the LDBWS pipeline as a whole last succeeded at all.

**3. Adopted, not re-derived: 08-31's Decision 1 (case 1) and its UI-copy
sketch.** This document reuses 08-31's frontend-only, value-driven rule for
TfL-quality lines (`dataQuality === 'tfl'` check ahead of any sample-related
field) and its exact case-1 copy ("Not measured by this app — status is
TfL's own.") rather than inventing a second version. Where this document's
new field interacts with that rule (case 1 must be checked *first*, ahead
of the new field — see Decision 4), that interaction is spelled out
explicitly, because getting the order wrong is a real, easy-to-hit bug (see
Error handling).

## Current relevant state (verified 2026-09-01)

**`common::SampleStats`** (`crates/common/src/lib.rs:675-681`, read in
full): `{ total: usize, delayed: usize, cancelled: usize, skipped: usize,
avg_delay_minutes: f64 }`. Unchanged since 08-31's own citation.

**`common::LineStatus`** (`crates/common/src/lib.rs:325-335`):
`sample_stats: Option<SampleStats>` and `data_quality: DataQuality` are
independent fields on the same struct — confirmed unchanged.

**`common::Defaults::min_sample_size`** (`crates/common/src/lib.rs:770-772`):
`#[serde_inline_default(3)]`, `pub min_sample_size: i64`, overridable per
line via `severity_overrides` (`thresholds_for`, `lib.rs:786-800`).

**The aggregator's National Rail path** (`crates/aggregator/src/aggregation.rs`,
read in full):

- `relevant_departures` (lines 682-692): `line.sample_stations.iter()
  .filter_map(|crs| samples.get(crs)).flat_map(|s| s.departures.iter())
  .filter(|dep| belongs_to_line(dep, line)).collect()`. A station absent
  from `samples` (`HashMap<String, StationSample>`, loaded whole-table, no
  `WHERE` clause, by `crates/aggregator/src/queries.rs`'s
  `load_station_samples` — re-confirmed this session, matches 08-31's own
  citation) is silently skipped by `filter_map`, identically to a station
  present but contributing zero departures that pass `belongs_to_line`.
  **`relevant_departures` alone cannot distinguish "no row for this
  station" from "row present, zero relevant departures in it"** — both
  collapse to the same filtered-out outcome.
- `compute_sample_stats` (lines 730-741): `let relevant =
  relevant_departures(line, samples); if (relevant.len() as i64) <
  thresholds.min_sample_size { return None; } Some(stats_from_departures(...))`.
  Exactly one gate, exactly as 08-31's Correction 1 established — `None`
  carries no information about *why*.
- `infer_from_samples` (lines 745-... ): calls `compute_sample_stats(...)?`
  and returns `None` on the `?` — its only caller,
  `aggregate`'s Layer 2 (line ~89), then does
  `report.statuses.push(inferred.unwrap_or_else(good_service))`.
  **This is the sharpest finding of this pass, not previously documented in
  08-31**: when `compute_sample_stats` returns `None`, the resulting
  `good_service()` (`aggregation.rs:906-915`) status carries `sample_stats:
  None` — the exact same observable shape as every other absent case, and
  the *reason* it's absent (no coverage at all vs. below threshold) is
  discarded at this exact point, before it ever reaches `LineStatus`. This
  is the majority-case code path: any line with no currently-active
  incident (i.e., most lines, most of the time) goes through here, not
  through the second Layer 2 branch below.
- Layer 2's second branch (`aggregate`, ~lines 92-105, active when
  `report.statuses` is non-empty from Layer 1's incident matching): `if let
  Some(stats) = compute_sample_stats(line, samples, defaults) { ... for
  status in &mut report.statuses { ...; status.sample_stats =
  Some(stats.clone()); } }` — when `compute_sample_stats` returns `None`,
  this whole block is skipped; every incident-derived status on the line
  keeps `sample_stats: None` from its construction in
  `status_from_incident`, again with no record of why.

**So today, `Option<SampleStats>` being `None` on a `LineStatus` has three
different real causes upstream, and every one of them is discarded before
`LineStatus` is built — not just "hard to tell apart from the frontend," as
08-31 characterized case 2 in general, but actively erased inside the
aggregator itself for the narrower "no row at all" sub-case this document
addresses.**

**The TfL path** (`crates/poller-tfl`, read in full):

- `crates/poller-tfl/src/schema.rs:145,148`: every parsed TfL status sets
  `data_quality: DataQuality::Tfl` and `sample_stats: None`, unconditionally,
  for every mode (tube, Overground, Elizabeth line, tram, DLR).
- `crates/poller-tfl/src/main.rs`'s DLR arrivals-diffing pilot
  (`dlr_pilot_enabled`, `crates/poller-tfl/src/config.rs:59-60`,
  `default_value_t = false` — confirmed still `false` by default): when
  enabled, `poll_once` (lines 129-152) calls `poll_dlr_sample_stats` (line
  193) each cycle and matches on its result:
  - `Ok(Some(stats))` → `merge_dlr_sample_stats` (lines 170-186) sets
    `status.sample_stats = Some(stats.clone())` on every status of the
    `tfl-dlr` line only.
  - `Ok(None)` → **"too soon to say"** (the function's own doc comment,
    line ~188-192) — no trip has fully resolved against the timetable yet
    this run. Nothing is mutated; the DLR line's statuses keep whatever
    `schema.rs` set (`sample_stats: None`).
  - `Err(err)` → logged (`tracing::warn!`) and swallowed; nothing mutated
    either — same observable outcome as `Ok(None)`.

  **All three of `Ok(None)`, `Err`, and pilot-disabled-entirely are
  observably identical to every other TfL line today** — one real signal
  (`Ok(Some(stats))`) and three different "not yet" reasons that all read
  as the same bare `None`.

**The wire boundary** (`crates/api/src/render.rs`, read in full):
`status_to_json` (lines 46-79) builds the public JSON by hand, independent
of `LineStatus`'s own serde derive (its own doc comment, lines 1-8, states
this explicitly — storage shape and public shape are deliberately
different concerns). `sampleStats` is only added to the output object
(`out["sampleStats"] = json!({...})`, lines 62-69) when
`status.sample_stats` is `Some`; otherwise the key is absent from the JSON
entirely (not `null`). Storage (`crates/api/migrations/20260510023522_initial.sql:69-77`,
confirmed): `line_status.statuses` is `JSONB`, one array of `LineStatus`
serialized via its own derive, no per-field columns — **adding a field to
`LineStatus` needs no migration**, it just starts appearing (or not) inside
the existing JSONB blob.

**Diff-suppression already exists and must be extended.** Both
`crates/aggregator/src/queries.rs:162-171` (`normalize_for_diff`, used by
`write_line_status`, line 258) and `crates/api/src/data/queries.rs:306-315`
(a same-named, independently-implemented twin, used by
`tfl_statuses_changed`) strip `sample_stats` from each status entry before
comparing old vs. new JSON, specifically because `sample_stats`'s live
counts "roll over almost every poll cycle even when nothing about the
underlying disruption has changed" (the aggregator's own comment, lines
164-166) — without the strip, `line_status_history` would grow a row every
single cycle. **Any new field with the same "changes every cycle,
independent of real disruption state" property must be added to both
strip lists, or it reintroduces exactly the bug the TfL spec's own "Hard
constraint" section (Area 1) already named and guarded against once.**

**`line_status_daily_stats` / Trends.** `lines_with_sample_coverage`
(`crates/aggregator/src/main.rs:175-186`, read in full) gates whether
`record_daily_stats` is called at all on `report.statuses.first()
.and_then(|s| s.sample_stats.as_ref()).is_some()` — a below-threshold cycle
does not increment `sample_cycles` at all (confirmed directly, not just
inferred: the filter runs before `record_daily_stats` is ever called for
that line that cycle). `frontend/app/lines/[id]/history/TrendsResults.tsx`'s
`SPARSE_DATA_FLOOR_CYCLES` (line 12, `= 20`) turns a day with too few
covered cycles into a rendered gap rather than a misleading rate — this
already exists and already partially addresses "don't show a confident
number over thin data" at the *daily-rollup* level, but it inherits the
identical presence-vs-threshold conflation this document fixes at the
*live-cycle* level, since `sample_cycles` only counts "had `Some(stats)`
this cycle," with the same lost-reason problem `compute_sample_stats`
has today.

**Frontend call sites** — every reference to
`firstSampleStats`/`cancelledPercent`/`formatSampleSummary`/`sampleStats`/
`dataQuality`, re-confirmed by grep this session (matches 08-31's own
table exactly, re-verified independently rather than trusted blind):

| Surface | File:line | Current behavior when `sampleStats` absent |
|---|---|---|
| All Lines table, mobile subtitle | `app/lines/AllLinesTable.tsx:224` | `formatSampleSummary(stats)` → `"No sample data"` |
| All Lines table, desktop Avg Delay / Cancelled | `AllLinesTable.tsx:228-245` | Bare `"—"`, no tooltip — **this document's primary target** |
| Pinned-line dashboard card | `components/LineStatusCard.tsx:47-51` | Whole `<Text>` omitted (`{stats && (...)}`) |
| Pinned-station dashboard row | `app/page.tsx:216-227` | Same conditional-omission pattern |
| Station detail page, per-line subtitle | `app/stations/[crs]/page.tsx:92-98` | `formatSampleSummary(stats)` → `"No sample data"`, unconditional (no truthiness guard) |
| Line detail page, `RepresentativeInfo` card | `components/RepresentativeInfo.tsx:8-10` | `return null` — never renders the absent case at all |

`frontend/lib/sampleStats.ts` (read in full, 24 lines): `firstSampleStats`
returns `SampleStats | undefined`; `formatSampleSummary` takes that same
`SampleStats | undefined` and returns `'No sample data'` when falsy — it
has no access to `dataQuality` or any other field of the owning
`LineStatus`, by construction. `frontend/lib/types.ts:71,74`: `dataQuality`
is a plain string union, already present on every `LineStatus` regardless
of `sampleStats`.

**Case 3 ("nothing scheduled right now" / expected-vs-observed) has no
signal anywhere.** `crates/schedule-ingest/src` (`config.rs`, `main.rs`,
`manifest.rs`, `scan.rs`, read in full) is entirely about DTD SFTP
delivery-sequence tracking (`SequenceRelation::Gap` etc.) — no per-line or
per-station expected-service-count concept anywhere. `crates/trust-consumer/src`
(read in full: `journey.rs`, `matching.rs`, `stanox_crs.rs`, `schema.rs`,
`process.rs`, `queries.rs`, `eta.rs`, `dedup.rs`, `health.rs`) uses
schedule/TIPLOC data only for matching live TRUST movement messages to a
pinned train's own journey and for STANOX↔CRS resolution — never to build
an aggregate "N services were timetabled for this line in this window"
figure. Building that would mean a new component reading CIF/schedule data
per-line, per-time-window, and comparing it against observed
`StationSample`/`SampleStats` counts — genuinely new ingestion-adjacent
work, not a field addition. This confirms the task's own framing: case 3 is
real, substantially bigger, and correctly out of scope for this pass (see
Explicitly out of scope).

## Decisions

### 1. Case 1 (TfL, structurally not sampled): reuse 08-31's rule and copy as-is, no changes

Per Corrections #3, this document does not re-derive Decision 1 of
`2026-08-31-sample-data-availability-design.md`. The rule
(`status.dataQuality === 'tfl'` checked ahead of any sample-presence
field, so it stays correct if `dlr_pilot_enabled` ever flips) and its copy
(`"Not measured by this app — status is TfL's own."`) are adopted verbatim.
The one addition this document makes here is procedural, not substantive:
this rule must be checked **before** the new `SampleAvailability` field
introduced below is read at all (Decision 4), because that field is not
meaningful for a TfL-quality status that never went through the aggregator
or DLR pilot (see Decision 3's "inert default" discussion).

### 2. A new `common::SampleAvailability` enum, computed alongside `SampleStats`, carrying exactly the presence/threshold distinction and nothing more

**New type**, `crates/common/src/lib.rs`, placed next to `SampleStats`:

```rust
// Sketch, not final.
/// Why a line's `sample_stats` is (or isn't) populated this cycle, on the
/// narrow question this type answers: did any configured sample station
/// have live departure data available to look at at all. Deliberately
/// does NOT attempt to distinguish "genuinely quiet" from "structurally
/// under-sampled" (both collapse into `BelowThreshold` here) -- that
/// needs a pattern over many cycles, not a single cycle's view, and is
/// left to `line_status_daily_stats`'s `sample_cycles`, per
/// 2026-08-31-sample-data-availability-design.md's Decision 3. Also does
/// NOT attempt to distinguish "no row" from "row present but stale" --
/// that is the still-open, deliberately separate `/public/freshness`
/// follow-up from that same spec's Decision 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SampleAvailability {
    /// None of this line's `sample_stations` had a `StationSample` row in
    /// this cycle's load at all (`samples.get(crs).is_none()` for every
    /// configured station). A real, currently-invisible signal -- a
    /// polling gap, a brand-new custom line never yet polled, or a
    /// widespread LDBWS outage.
    NoCoverage,
    /// At least one configured station had a row, but the operator-
    /// filtered relevant-departure count fell below `min_sample_size`.
    /// Collapses the "genuinely quiet" / "structurally under-sampled"
    /// distinction deliberately -- see this type's own doc above.
    BelowThreshold { observed: usize, required: i64 },
    /// At or above threshold; the real `SampleStats` this cycle produced
    /// is carried on `LineStatus.sample_stats` as it already is today --
    /// not duplicated inside this variant's own wire representation (see
    /// Decision 4's render.rs note). Internally (this Rust type, and
    /// storage) this variant does carry the `SampleStats` value, so
    /// `compute_sample_availability` has one return value, not two.
    Available(SampleStats),
}
```

**`LineStatus` gains one new field, additive:**

```rust
// Sketch, not final.
pub struct LineStatus {
    // ...existing fields, unchanged...
    pub sample_stats: Option<SampleStats>, // UNCHANGED: kept exactly as-is
    #[serde(default = "SampleAvailability::no_coverage_default")]
    pub sample_availability: SampleAvailability,
}
```

`sample_stats` is **not removed or retyped** — seeAt Decision 4/Migration
below for why. `sample_availability` is a new, always-present field (not
`Option`) with a serde default (needed only for deserializing rows written
before this field existed, e.g. any in-flight `line_status_history` reads —
`write_line_status` itself always writes a freshly-computed value, so this
default is a read-compat shim for old data, not a real "unknown" state).

**`compute_sample_stats` becomes `compute_sample_availability`**, in
`crates/aggregator/src/aggregation.rs`, replacing its body (lines 730-741):

```rust
// Sketch, not final.
fn compute_sample_availability(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> SampleAvailability {
    let thresholds = thresholds_for(defaults, &line.severity_overrides);
    let has_any_row = line.sample_stations.iter().any(|crs| samples.contains_key(crs));
    if !has_any_row {
        return SampleAvailability::NoCoverage;
    }
    let relevant = relevant_departures(line, samples);
    if (relevant.len() as i64) < thresholds.min_sample_size {
        return SampleAvailability::BelowThreshold {
            observed: relevant.len(),
            required: thresholds.min_sample_size,
        };
    }
    SampleAvailability::Available(stats_from_departures(&relevant, line, &thresholds))
}
```

The presence check (`has_any_row`) is deliberately **not** folded into
`relevant_departures` itself — that function's existing job (shared by
`compute_sample_availability`, `infer_from_samples`'s "most cited reason"
pass, and `dedup::dedup_new_sample_stats`, per its own doc comment,
`aggregation.rs:665-670`) is "give me the relevant departures," and adding
a second return channel to it would change its signature for three callers
that don't need the distinction. A `SampleStats`-shaped accessor
(`availability.sample_stats() -> Option<SampleStats>`, extracting
`Available`'s payload or `None`) is kept for the two call sites below that
still need the old `Option<SampleStats>` shape internally.

### 3. Both `aggregate()` call sites are updated so the reason is never discarded — including the majority-case `infer_from_samples` path

This is the substantive behavioral change, not just a new field sitting
unused: today, `infer_from_samples` (`aggregation.rs:745-...`) discards the
distinction entirely via `compute_sample_stats(...)?` and
`.unwrap_or_else(good_service)`. **Severity inference behavior is
unchanged** — a line with no or thin coverage still gets `GoodService`,
same as today; this document does not touch severity classification, only
what accompanies it.

```rust
// Sketch, not final. infer_from_samples's new shape.
fn infer_from_samples(line: &LineDefinition, samples: &..., defaults: &Defaults) -> LineStatus {
    let availability = compute_sample_availability(line, samples, defaults);
    let SampleAvailability::Available(stats) = &availability else {
        let mut status = good_service();
        status.sample_availability = availability;
        return status;
    };
    // ...existing classify()/reason-building body, unchanged, operating on `stats`...
    // every return path sets status.sample_stats = Some(stats.clone())
    // (as today) AND status.sample_availability = availability.clone() (new).
}
```

Note this also simplifies the call site: `infer_from_samples` no longer
returns `Option<LineStatus>` (it always has *something* to return —
`good_service()` was always the fallback anyway), so
`aggregate`'s Layer 2 `report.statuses.push(inferred.unwrap_or_else(good_service))`
becomes `report.statuses.push(infer_from_samples(...))` — a small,
incidental cleanup, not a goal of this document.

Layer 2's second branch (`aggregate`, ~lines 92-105) changes from
"only touch statuses when `Some`" to "always attach availability, only
escalate/attach real stats when `Available`":

```rust
// Sketch, not final.
let availability = compute_sample_availability(line, samples, defaults);
for status in &mut report.statuses {
    status.sample_availability = availability.clone();
}
if let SampleAvailability::Available(stats) = &availability {
    let thresholds = thresholds_for(defaults, &line.severity_overrides);
    for status in &mut report.statuses {
        let (escalated, annotation) = escalate_from_sample_stats(status.severity, stats, &thresholds);
        status.severity = escalated;
        if let Some(annotation) = annotation {
            status.reason.push_str(&format!(" ({annotation})"));
        }
        status.sample_stats = Some(stats.clone());
    }
}
```

Severity escalation (`escalate_from_sample_stats`) still only runs on
`Available`, exactly as today's `if let Some(stats) = ...` did — this
document does not change when or how severity gets escalated from live
samples, only what gets recorded when it can't be.

### 4. The DLR pilot's own states map onto the same enum — `Ok(None)` becomes `BelowThreshold`, not a fourth bespoke state

Per the task framing and Current relevant state above, the DLR pilot is a
second, independent producer of `SampleStats` that needs to present one
consistent signal, not two half-solutions. Mapping:

- **Pilot disabled, or a mode other than DLR** (`crates/poller-tfl/src/schema.rs`'s
  status construction, lines ~140-148): gains one more field literal,
  `sample_availability: SampleAvailability::NoCoverage`. This is the
  **inert default** referenced in Decision 1 — per that decision, the
  frontend never actually reads this value for a `dataQuality === 'tfl'`
  status with no `sample_stats`, because the `dataQuality` check is
  checked first and short-circuits. It is set here only because
  `LineStatus` is a plain Rust struct (not going through `#[serde(default)]`
  at construction time) and every field must have a value; `NoCoverage` is
  the most defensible literal choice among the enum's variants for "this
  code path never attempted any sampling," but its value is functionally
  inert as long as the frontend's precedence rule (Decision 1, reused)
  holds. See Error handling for why this is flagged as a real, named
  footgun rather than assumed safe forever.
- **Pilot enabled, `Ok(Some(stats))`** (`merge_dlr_sample_stats`,
  `crates/poller-tfl/src/main.rs:179-186`): extended to also set
  `status.sample_availability = SampleAvailability::Available(stats.clone())`,
  alongside its existing `status.sample_stats = Some(stats.clone())`.
- **Pilot enabled, `Ok(None)`** ("too soon to say" — no trip has resolved
  yet this run): a new sibling function, e.g. `mark_dlr_pending`, sets
  `sample_availability = SampleAvailability::BelowThreshold { observed: 0,
  required: 1 }` on the `tfl-dlr` line's statuses, leaving `sample_stats`
  at `None`. This is a deliberate, honest repurposing, not a perfect fit:
  the DLR pilot has no tunable `min_sample_size`-equivalent — it
  structurally needs at least one resolved trip before it can report
  anything — so `required: 1` is literally true (not a borrowed LDBWS
  constant), and `observed: 0` accurately reports "zero trips have
  resolved yet." Flagged explicitly as a judgment call in Open
  questions/risks: this reuses the shape of a distinction (LDBWS
  station-count threshold) built for a mechanically different pilot
  (per-trip resolution), and the two producers' internal semantics remain
  different even though the wire/UI treatment is now unified.
- **Pilot enabled, `Err`** (a transient failure this cycle, already logged
  and swallowed): **left unchanged, deliberately** — the DLR line's
  statuses keep whatever `schema.rs` already set that cycle
  (`NoCoverage`), collapsing a transient pilot failure into the same
  bucket as "never attempted." This is a known, accepted simplification —
  see Open questions/risks — not a gap this document claims to close;
  distinguishing "the pilot tried and failed" from "the pilot never runs"
  would need its own error-state plumbing this document judges not worth
  it for a feature that is itself still disabled by default.

### 5. Wire shape: additive, `sampleAvailability` sits alongside the unchanged `sampleStats`, no payload duplication

`crates/api/src/render.rs`'s `status_to_json` (lines 46-79) gains one more
unconditional block (unlike `sampleStats`, which is conditional on
`Some`— `sampleAvailability` is always present, since the new field on
`LineStatus` always has a value):

```rust
// Sketch, not final.
out["sampleAvailability"] = match &status.sample_availability {
    SampleAvailability::NoCoverage => json!({ "state": "no-coverage" }),
    SampleAvailability::BelowThreshold { observed, required } =>
        json!({ "state": "below-threshold", "observed": observed, "required": required }),
    SampleAvailability::Available(_) => json!({ "state": "available" }),
};
```

The `Available` case deliberately does **not** re-serialize the
`SampleStats` payload a second time — the existing `sampleStats` key
(unchanged, still conditional on `Some`) already carries it, exactly as
today. This mirrors this module's own established pattern (its doc
comment, lines 1-8: the stored/internal shape and the public wire shape are
deliberately different concerns) — the internal `SampleAvailability::Available(SampleStats)`
variant is convenient for `compute_sample_availability` to have one return
value, but the wire form only needs the tag.

**Frontend type** (`frontend/lib/types.ts`), additive next to the
unchanged `sampleStats?: SampleStats`:

```ts
// Sketch, not final.
export type SampleAvailability =
  | { state: 'no-coverage' }
  | { state: 'below-threshold'; observed: number; required: number }
  | { state: 'available' };

export interface LineStatus {
  // ...existing fields, unchanged...
  sampleStats?: SampleStats;       // UNCHANGED
  sampleAvailability: SampleAvailability; // NEW, always present
}
```

### 6. One shared frontend helper, reused by all six call sites, with precedence: `dataQuality` first, then `sampleAvailability`

`frontend/lib/sampleStats.ts` gains a new exported function, and
`formatSampleSummary`'s signature changes (see Migration below for why this
is judged acceptable):

```ts
// Sketch, not final.
/** The human-readable reason sample stats aren't shown, or `null` when
 * real stats are available and the caller should render numbers instead.
 * MUST check dataQuality before sampleAvailability -- see Decision 1/4:
 * sampleAvailability is not a meaningful signal for a TfL-quality status
 * that never went through the aggregator or DLR pilot. */
export function sampleUnavailableReason(status: LineStatus): string | null {
  if (status.sampleStats) return null;
  if (status.dataQuality === 'tfl') {
    return "Not measured by this app — status is TfL's own.";
  }
  if (status.sampleAvailability.state === 'no-coverage') {
    return 'No live departure data received for this line yet.';
  }
  return 'Too few live departures sampled to report a rate right now.';
}

export function formatSampleSummary(status: LineStatus | undefined): string {
  if (!status) return 'No sample data'; // defensive; should not occur in practice
  const reason = sampleUnavailableReason(status);
  if (reason) return reason;
  const stats = status.sampleStats!;
  const cancelled = cancelledPercent(stats);
  const delay = `Avg delay ${stats.avgDelayMinutes.toFixed(1)} min`;
  return cancelled === null ? delay : `${delay} · ${cancelled}% cancelled`;
}

/** Unchanged contract, still returns bare stats -- kept for callers that
 * only need the numbers (e.g. AllLinesTable's numeric sort comparators),
 * not the reason. */
export function firstSampleStats(statuses: LineStatus[]): SampleStats | undefined { /* unchanged body */ }

/** NEW: the first status carrying real stats if any does, else the first
 * status overall -- so a caller always has a dataQuality/sampleAvailability
 * to build a reason from, even when nothing has stats. */
export function representativeStatus(statuses: LineStatus[]): LineStatus | undefined {
  return statuses.find((s) => s.sampleStats) ?? statuses[0];
}
```

`cancelledPercent` is unchanged (still takes `SampleStats | undefined`, still
used only once real stats exist).

**Per-surface UI treatment**, converging all six call sites from the table
above onto `representativeStatus` + `sampleUnavailableReason`/
`formatSampleSummary`:

- **`AllLinesTable.tsx` desktop Avg Delay / Cancelled columns (this
  document's named driver).** Keep the bare `"—"` glyph (08-31 already
  judged, and this document agrees, that a dash in a numeric column
  correctly reads as "no number here" — no new iconography is introduced
  without a design pass to back it, see Open questions). **What changes**:
  wrap the dash in a Mantine `Tooltip` (already an established pattern in
  this codebase — `components/EtaBadge.tsx:26`, `components/DataFreshnessInfo.tsx:29`)
  with `label={sampleUnavailableReason(representativeStatus(report?.lineStatuses ?? []))}`.
  This was previously "optional... polish, not required" per 08-31; this
  document makes it a required part of the fix, since differentiating this
  exact dash is the task's stated goal.
- **Mobile subtitle (`AllLinesTable.tsx:224`), station detail page
  (`app/stations/[crs]/page.tsx:98`)**: both already call
  `formatSampleSummary`; only the argument changes from `stats` to
  `representativeStatus(statuses)`, and both now render one of three
  distinguishable strings instead of two.
- **`LineStatusCard.tsx:47-51`, `app/page.tsx`'s pinned-station row**:
  adopt 08-31's own recommended behavior change here too (its UI treatment
  section, "stop omitting the line entirely") — switch from
  `{stats && (<Text>...)}` to always rendering
  `formatSampleSummary(representativeStatus(...))`. This document adopts
  that call rather than leaving these two surfaces on the old two-way
  copy while the other four surfaces gain the new three-way distinction —
  converging wording without converging *presence* would leave these two
  surfaces silently behind.
- **`RepresentativeInfo.tsx`**: unchanged. It only ever renders the
  "stats present" case (`if (!withStats?.sampleStats) return null`); this
  document's new field has nothing to add there, matching 08-31's own
  scoping of this component as out of scope for absent-state messaging.

## Architecture

```
                         crates/aggregator (main-cycle path)
┌──────────────────────────────────────────────────────────────────────┐
│ queries::load_station_samples()  -- whole-table load, no staleness    │
│         │                                                              │
│         ▼                                                              │
│ aggregation::compute_sample_availability(line, samples, defaults)      │
│         │                                                              │
│   ┌─────┴─────────────┬───────────────────────┐                       │
│   ▼                    ▼                       ▼                       │
│ NoCoverage      BelowThreshold{n,req}      Available(SampleStats)      │
│ (no row for      (row present, under        (>= min_sample_size)      │
│  any sample_      threshold after                                     │
│  station)         operator filter)                                    │
│         │                    │                        │                │
│         └────────────────────┴────────────────────────┘                │
│                              ▼                                         │
│         LineStatus { sample_stats: Option<SampleStats> (unchanged),    │
│                       sample_availability: SampleAvailability (NEW) }  │
│         -- attached at BOTH Layer 2 call sites: infer_from_samples     │
│            (no incident) and the escalation branch (has incident)      │
└───────────────────────────────┬────────────────────────────────────────┘
                                 │  JSONB, no migration needed
                                 ▼
                    line_status.statuses (line_status_history mirrors,
                    normalize_for_diff strips sample_stats AND
                    sample_availability before diffing -- both change
                    every cycle independent of real disruption state)
                                 │
                                 ▼
                  crates/api render.rs::status_to_json
                  -- sampleStats: unchanged, conditional on Some
                  -- sampleAvailability: NEW, always present, tag-only
                                 │
                                 ▼
                        GET /public/lines, /Line/{id}/Status  (JSON)
                                 │
                                 ▼
              frontend/lib/sampleStats.ts (sampleUnavailableReason,
              formatSampleSummary, representativeStatus)
                                 │
        ┌────────────────────────┼──────────────────────────┐
        ▼                        ▼                           ▼
 AllLinesTable.tsx      LineStatusCard.tsx /          stations/[crs]/page.tsx
 (dash + Tooltip,        app/page.tsx pinned row       (subtitle text)
  primary target)         (now always renders)

                    crates/poller-tfl (independent producer)
┌──────────────────────────────────────────────────────────────────────┐
│ schema.rs: every status -> sample_availability: NoCoverage (inert     │
│            default, see Decision 4 -- frontend never reads it here    │
│            because dataQuality=='tfl' short-circuits first)           │
│ main.rs (dlr_pilot_enabled only): tfl-dlr line's statuses overridden: │
│   Ok(Some(stats)) -> Available(stats)     (+ sample_stats: Some)      │
│   Ok(None)        -> BelowThreshold{0,1}  (sample_stats stays None)   │
│   Err              -> left at NoCoverage (unchanged this cycle)       │
└──────────────────────────────────────────────────────────────────────┘
                                 │  same LineStatus shape, same endpoint,
                                 ▼  same frontend helper -- unified
                     (feeds into the same diagram above)
```

## Error handling

- **The `dataQuality`-before-`sampleAvailability` precedence rule is
  enforced by convention (one shared helper function), not by the type
  system.** A future call site that reads `status.sampleAvailability`
  directly, without routing through `sampleUnavailableReason`, would see
  `'no-coverage'` for every plain Tube/Overground/Elizabeth-line/tram
  status (Decision 4's inert default) and could misreport a structural,
  permanent non-signal as a live pipeline gap. Mitigation: a code comment
  on `SampleAvailability` itself (already in the sketch above) plus a unit
  test asserting the precedence order directly (see Testing) — not a
  compile-time guarantee, flagged honestly as a residual risk rather than
  claimed solved.
- **`normalize_for_diff` (both copies) must strip `sample_availability`
  alongside `sample_stats`, or `line_status_history` grows a row on every
  cycle a line's observed count merely fluctuates around the threshold**
  (e.g. `BelowThreshold{2,3}` → `BelowThreshold{3,3}` → `Available(...)` →
  back again, none of which reflect a real change in the underlying
  disruption). This is the same failure mode the TfL spec's "Hard
  constraint" (Area 1) already named for a different volatile field —
  this document treats it as a hard constraint of equal weight, not an
  afterthought, and requires a regression test (see Testing) confirming it.
- **DLR pilot `Err` collapsing into the same bucket as "never attempted"**
  (Decision 4) means a real, transient pilot failure is invisible as
  anything other than "no coverage" — no worse than today's behavior
  (which is *also* silent about the difference), but not an improvement
  for that specific case either. Named explicitly rather than implied
  solved by this document's broader fix.
- **A malformed/future `LineDefinition` with empty `sample_stations`**
  (`#[serde(default)]` on that field permits it, though 08-31's Correction
  5 confirmed no current catalogue or custom line has one) would make
  `has_any_row` vacuously `false` for every cycle, permanently reporting
  `NoCoverage` — this is the *correct*, honest outcome for that
  configuration (there is genuinely no coverage to have), not a new
  failure mode this document introduces.

## Testing

Following this repo's convention (colocated Rust `#[cfg(test)]` modules,
colocated Vitest for the frontend):

**`crates/aggregator/src/aggregation.rs`:**
- `compute_sample_availability` returns `NoCoverage` when none of a line's
  `sample_stations` appear in `samples`; `BelowThreshold { observed,
  required }` with the correct counts when at least one station is present
  but the operator-filtered total is under `min_sample_size` (including a
  per-line `severity_overrides` case, mirroring the existing
  `thresholds_for` override tests at `crates/common/src/lib.rs:839-849`);
  `Available(stats)` matching today's existing `compute_sample_stats`
  at-or-above-threshold behavior exactly, as a non-regression check.
- `infer_from_samples`: a no-coverage line still returns a `GoodService`
  status (severity behavior unchanged) but now carries
  `sample_availability: NoCoverage` where today it silently carries
  nothing — the core regression test for this document's headline finding.
- Layer 2's second branch (`aggregate()`, incident-derived-status path): a
  line with an active incident and zero `StationSample` coverage produces
  statuses carrying `sample_availability: NoCoverage` alongside their
  existing `sample_stats: None` — today this information is dropped
  entirely at this call site.

**`crates/aggregator/src/queries.rs` / `crates/api/src/data/queries.rs`:**
- Extend the existing `normalize_for_diff`-adjacent tests (e.g.
  `crates/api/src/data/queries.rs:967-999`'s
  `tfl_statuses_changed_ignores_sample_stats_only_differences`) with a
  sibling asserting a `sample_availability`-only difference (`NoCoverage`
  → `BelowThreshold{2,3}`, or `BelowThreshold{2,3}` →
  `BelowThreshold{3,3}`) also does **not** register as `changed` — the
  direct regression test for the Error handling hard constraint above.

**`crates/poller-tfl/src/schema.rs` / `src/main.rs`:**
- `schema.rs`: a parsed status carries `sample_availability: NoCoverage`
  by construction (extends whatever existing test already asserts
  `sample_stats: None`/`data_quality: Tfl` there).
- `main.rs`: extend `merge_dlr_sample_stats`'s existing coverage (if any;
  otherwise a new test) to assert `Ok(Some(stats))` sets
  `Available(stats)`; a new test for the `Ok(None)` branch asserting
  `BelowThreshold { observed: 0, required: 1 }` with `sample_stats`
  unchanged (`None`); a test confirming an `Err` branch leaves
  `sample_availability` at whatever `schema.rs` set (`NoCoverage`),
  documenting Decision 4's accepted simplification rather than leaving it
  implicit.

**`crates/api/src/render.rs`:**
- Extend the existing `sample_stats_included_when_present`/
  `sample_stats_omitted_when_absent` tests (lines 194-...) with assertions
  that `sampleAvailability` is always present in the output JSON (unlike
  `sampleStats`) and takes the shape `{"state": "no-coverage"}` /
  `{"state": "below-threshold", "observed": N, "required": N}` /
  `{"state": "available"}` for each respective case — including a direct
  assertion that the `Available` case does **not** duplicate the
  `SampleStats` fields inside `sampleAvailability` itself.

**Frontend (`frontend/lib/sampleStats.test.ts`, rewritten for the new
signatures):**
- `sampleUnavailableReason`: the four real outcomes — `sampleStats`
  present (`null`); `dataQuality === 'tfl'` with no stats (TfL copy,
  regardless of what `sampleAvailability` happens to hold — the direct
  test for the precedence rule named in Error handling); `sampleAvailability.state
  === 'no-coverage'` (new copy); `sampleAvailability.state ===
  'below-threshold'` (existing "too few... " copy, reused).
- `formatSampleSummary`: updated for the new `LineStatus | undefined`
  signature, covering the same four cases plus the existing numeric-
  formatting path unchanged.
- `representativeStatus`: returns the first status with real stats when
  any exists; falls back to `statuses[0]` when none does; `undefined` only
  for an empty array.

**`frontend/app/lines/AllLinesTable.test.tsx`:** a new test per
`sampleAvailability` state confirming the desktop dash's `Tooltip` label
text differs correctly across `no-coverage` / `below-threshold` / TfL
lines, and that the dash glyph itself (`"—"`) is unchanged in all three —
this is the direct test for the task's named driver.

**`frontend/components/LineStatusCard.test.tsx`:** extend for the "always
render" behavior change (Decision 6) — a report with no `sampleStats`
anywhere now renders a `formatSampleSummary` line instead of omitting the
block entirely.

## Explicitly out of scope

- **Case 3 in full — a schedule-derived "N services were expected" figure
  to compare against observed counts, for the "nothing scheduled right
  now" sub-case.** Confirmed this session (Current relevant state) that
  neither `crates/schedule-ingest` nor `crates/trust-consumer` has any
  per-line/per-window expected-service-count concept today — this would be
  new ingestion-adjacent work (reading CIF/schedule data per line, per
  time window, and reconciling it against `StationSample`/`SampleStats`),
  not a field addition on top of what exists. Real, and plausibly
  valuable (an early-morning line correctly showing "no trains scheduled"
  rather than any flavor of "no data"), but a materially larger and
  differently-shaped piece of work than this document's scope, needing its
  own design pass — including real questions this document does not
  attempt (which schedule source, what counts as "expected" through
  cancellations/late notices, how far in advance, per-station or
  per-line). Not designed here, named as a real follow-up.
- **Distinguishing 2a (genuinely quiet) from 2b (structurally
  under-sampled)** within `BelowThreshold`. Per 08-31's Decision 3
  (unchanged, not revisited here — see Corrections #2's precise scope),
  this needs a pattern over many cycles, which is `line_status_daily_stats`'s
  `sample_cycles`' job, not a single live cycle's. This document's
  `BelowThreshold` variant intentionally collapses both.
- **08-31's own still-open Decision 2 proposal** (a global `stationSamples`
  timestamp on `/public/freshness`, answering staleness rather than
  presence). Unimplemented, unaffected, and not built by this document —
  see Corrections #2 for exactly how the two relate.
- **Extending `line_status_daily_stats` or `TrendsResults.tsx` with this
  same NoCoverage/BelowThreshold split.** Considered and rejected for this
  pass: `sample_cycles` already answers a coarser but real "how much
  coverage did this line get today" question at the rollup level, and
  `SPARSE_DATA_FLOOR_CYCLES` already turns thin days into a rendered gap
  rather than a misleading rate. Splitting `sample_cycles` itself into
  "cycles with `NoCoverage`" vs. "cycles with `BelowThreshold`" sub-counts
  is a real, separate schema change to `line_status_daily_stats`
  (`crates/api/migrations/20260831090001_line_status_daily_stats.sql`)
  that this document does not need for its stated goal (fixing the *live*
  view's dash) and would be premature without a concrete use for the
  distinction at the rollup level — left for a future pass if the Trends
  view is ever judged to need it.
- **Any new visual language beyond a `Tooltip`** for the AllLinesTable
  dash (a distinct icon, color, or glyph per state). Considered — a
  warning-colored dot for `NoCoverage` specifically was discussed as a
  plausible richer treatment — but not proposed here without a design
  pass to back a new visual convention; a `Tooltip` reuses this codebase's
  own established pattern with zero new visual vocabulary.
- **Any change to `min_sample_size`'s default, or to severity
  classification/escalation logic.** This document is entirely about what
  accompanies an unchanged severity outcome, not about tuning when
  `GoodService` vs. a worse severity fires.
- **The DLR pilot's own rollout/correctness, or turning `dlr_pilot_enabled`
  on.** Treated here only as a fact this document's Decision 4 must design
  around, exactly as `2026-08-22-tfl-service-metrics-v2-design.md` and
  `2026-08-31-sample-data-availability-design.md` both already scoped it.

## Open questions / risks

1. **The `dataQuality`-before-`sampleAvailability` precedence rule is a
   convention, not a type-level guarantee** (Error handling). If this
   codebase later adds a second frontend surface that reads
   `sampleAvailability` without going through `sampleUnavailableReason`,
   it will silently misreport every plain TfL-quality line as a live
   pipeline gap. Worth a stronger guard (e.g. a lint rule, or restructuring
   `sampleAvailability` itself to be `Option<SampleAvailability>` with
   `None` for "not applicable" rather than an inert `NoCoverage` default)
   if this pattern is judged too easy to get wrong in practice — not
   resolved here, since making it `Option` reopens exactly the "why is
   this None" ambiguity this whole document exists to close, just one
   level up.
2. **The DLR pilot's `Ok(None)` → `BelowThreshold { observed: 0, required:
   1 }` mapping (Decision 4) is an honest but imperfect fit** — it reuses a
   shape built for a mechanically different producer (LDBWS station-count
   threshold vs. per-trip resolution warm-up). If the DLR pilot ever grows
   more graduated internal states (e.g. "N of M expected trips resolved"),
   this mapping should be revisited rather than assumed to still fit.
3. **Copy is this document's own best attempt, not user-tested** — same
   posture 08-31 and the anonymous-user-ux spec both already took with
   their own proposed strings. "No live departure data received for this
   line yet." in particular is new wording introduced by this document
   (not reused from 08-31, which never distinguished this case) and has
   had no product/copy review.
4. **Whether a `NoCoverage` line should be visually flagged anywhere more
   prominent than a tooltip** (e.g. an operational alerting signal, since
   it can indicate a real LDBWS outage affecting a specific line) is a
   real question this document does not answer — this is a passenger-
   facing UI fix, not an operability/alerting feature, and building the
   latter on top of this field is left open as a possible, not designed,
   follow-up.
5. **`formatSampleSummary`'s signature change (from `SampleStats |
   undefined` to `LineStatus | undefined`) is a real breaking change to an
   already-tested, already-used internal function**, not just an additive
   one. Every existing call site and `frontend/lib/sampleStats.test.ts`
   itself needs updating in the same change — flagged explicitly so this
   isn't discovered mid-implementation as unplanned scope; it is
   accounted for in Testing above, but is a bigger frontend diff than the
   backend's purely-additive `sampleAvailability` field.
