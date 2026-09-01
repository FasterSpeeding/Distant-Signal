# Line Status Sample Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the frontend distinguish three cases that today all render as the same bare `"—"` on `AllLinesTable.tsx`'s Avg Delay / Cancelled columns (and the same silent omission elsewhere): a TfL-quality line this app structurally never samples, a line with a real live-data gap (no `StationSample` row for any of its configured stations this cycle), and a line that was polled fine but has too few live departures to clear `min_sample_size` right now. Ship this by adding a new `common::SampleAvailability` enum (`NoCoverage` / `BelowThreshold { observed, required }` / `Available(SampleStats)`) alongside the existing `LineStatus.sample_stats: Option<SampleStats>`, fixing the aggregator bug that currently discards this distinction before it ever reaches `LineStatus` (both `aggregate()` call sites, most importantly the majority-case `infer_from_samples` path), and threading the new field through the wire boundary into one shared frontend helper that every consuming surface calls through.

**Architecture:**

```
crates/common/src/lib.rs                    + SampleAvailability enum
                                              + LineStatus.sample_availability
        │
        ▼
crates/aggregator/src/aggregation.rs         compute_sample_stats -> compute_sample_availability
                                              infer_from_samples: Option<LineStatus> -> LineStatus
                                              aggregate()'s two Layer-2 call sites, both fixed
        │                                    (Task 1)
        ├────────────────┬─────────────────────────────┐
        ▼                ▼                             ▼
crates/aggregator/       crates/poller-tfl/src/main.rs  crates/api/src/render.rs
  src/queries.rs           merge_dlr_sample_stats ext'd  status_to_json gains
crates/api/src/            + new mark_dlr_pending          sampleAvailability
  data/queries.rs          (Task 3, DLR pilot)           (Task 4, wire shape)
  normalize_for_diff strips
  sample_availability too
  (Task 2, diff suppression)
        │                        │                              │
        └────────────────────────┴──────────────────────────────┘
                                  ▼
                    frontend/lib/types.ts + sampleStats.ts
                    (Task 5: SampleAvailability TS type,
                     sampleUnavailableReason, representativeStatus,
                     formatSampleSummary's signature change,
                     + mechanical fixture updates across every
                     existing test file with a LineStatus literal)
                                  │
        ┌─────────────────────────┼──────────────────────────────┐
        ▼                         ▼                               ▼
  AllLinesTable.tsx        LineStatusCard.tsx /            stations/[crs]/page.tsx
  (Task 6, primary          app/page.tsx pinned row         (Task 8, argument swap)
   target: dash Tooltip)    (Task 7, always-render)
```

**Tech Stack:** Rust (the existing `common`/`aggregator`/`poller-tfl`/`api` crates — no new crate, no new dependency); Next.js App Router + TypeScript + Mantine v9 (existing `Tooltip` pattern, no new frontend dependency); PostgreSQL `JSONB` (no migration — `LineStatus`'s stored shape gains a field the same way `sample_stats` itself did, per `crates/api/migrations/20260510023522_initial.sql:69-77`'s own comment that `statuses` is "always written and read as a unit").

**Spec:** `docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md` — read in full before starting; this plan does not restate its research, only carries its Decisions into concrete tasks. Cross-references below to "Decision N" refer to that document. Note for implementers: as of this plan's writing that spec file lives on a sibling worktree branch (`worktree-agent-a279d9fca58155758`, commit `ffa9cfd`) and has not yet landed on `main` or on this plan's own branch — merge or cherry-pick it in before starting Task 1, or fetch its content from that commit. This plan's own citations below were independently re-verified against this worktree's actual current source (not trusted blind from the spec) as of 2026-09-01.

**Status note — every citation below re-confirmed directly against this worktree's current source, not trusted from the spec:** `common::LineStatus` (`crates/common/src/lib.rs:325-335`) and `common::SampleStats` (`lib.rs:675-681`) match the spec's own citations exactly. `crates/aggregator/src/aggregation.rs`: `relevant_departures` at lines 682-692 (exact match), `compute_sample_stats` at 730-743, `infer_from_samples` at 745-809, `good_service` at 906-915, `aggregate()`'s two Layer-2 call sites at line 92 (`report.statuses.push(inferred.unwrap_or_else(good_service))`) and lines 96-106 — all confirmed unchanged from the spec's citations. `crates/poller-tfl/src/main.rs`'s `merge_dlr_sample_stats` is at lines 179-186 (exact match) and already has a test at `main.rs:351-389` (`dlr_sample_stats_are_merged_onto_the_matching_line_only`) — the spec's Testing section hedged this as "if any; otherwise a new test"; it already exists and this plan extends it rather than creating it. `crates/api/src/render.rs`'s `status_to_json` is at lines 47-88, its test module at line 95, with `sample_stats_included_when_present`/`sample_stats_omitted_when_absent` at lines 194/213 (spec cited "194-..."). `crates/aggregator/src/queries.rs`'s `normalize_for_diff`/`normalize_entry_for_diff` are at lines 162-188, stripping `sample_stats` at line 180; `crates/api/src/data/queries.rs`'s independently-implemented twin is at lines 299-318, stripping at line 313 — both confirmed. `frontend/lib/types.ts:71,74` and `frontend/lib/sampleStats.ts` (24 lines) match the spec exactly. All six frontend call sites in the spec's own table were independently re-confirmed at the exact cited lines: `AllLinesTable.tsx:224` (mobile subtitle) and `:228-245` (desktop dash), `LineStatusCard.tsx:47-51`, `app/page.tsx:216-227`, `app/stations/[crs]/page.tsx:92-98`, `components/RepresentativeInfo.tsx` (whole file, unchanged per this plan too).

**New finding this plan's own verification pass surfaced, not called out by the spec:** `LineStatus` is a plain Rust struct with **no** `#[derive(Default)]` and every production/test construction site is a full struct literal — grepping `LineStatus {` across the workspace finds **9 distinct construction sites** that must all gain the new `sample_availability` field for `cargo build --workspace`/`cargo test --workspace` to pass once the field is added (the `#[serde(default = ...)]` attribute only helps *deserialization* of old stored rows, it does nothing for a Rust struct literal or for TypeScript object literals). Production sites (must compile under plain `cargo build --workspace`): `crates/aggregator/src/aggregation.rs:144` (`status_from_incident`), `:795` (`infer_from_samples`'s classified-result path), `:907` (`good_service`), `crates/poller-tfl/src/schema.rs:132` (`map_status`). Test-only sites (compile under `cargo test`): `crates/aggregator/src/aggregation.rs`'s own test module, `crates/aggregator/src/main.rs:212` (`status_with_stats`), `crates/api/src/render.rs:106,220`, `crates/api/src/routes/line_status.rs:404` (`a_status`), `crates/poller-tfl/src/main.rs:359,373` (`dlr_sample_stats_are_merged_onto_the_matching_line_only`'s fixtures). The equivalent frontend problem is larger: `frontend/lib/types.ts`'s `LineStatus` interface is used as a **required-field** type by every test file constructing a `LineStatus` object literal (TypeScript's structural typing enforces this at `tsc`/`next build` time project-wide, not per-file) — a grep for `dataQuality:` across `frontend/**/*.test.{ts,tsx}` finds **9 files, ~24 distinct object literals**: `app/lines/AllLinesTable.test.tsx` (4, at lines 37/51/242/301), `components/IssueList.test.tsx` (12, 4 shared top-level fixtures at lines ~26/34/42/50 plus 8 more inline), `components/LineStatusCard.test.tsx` (3, none of which import `dataQuality:` as a grep hit because they spread `...report.lineStatuses[0]` — confirmed by direct read, at lines 15-21/53-59/60-66), `components/RepresentativeInfo.test.tsx` (1), `lib/history.test.ts` (1), `lib/severity.test.ts` (2), `lib/stationIssues.test.ts` (2), `lib/validity.test.ts` (1), `lib/sampleStats.test.ts` (1, via its own `status()` factory). Task 1 and Task 5 below both budget explicit, separate steps for this mechanical fallout rather than letting it surface as an unplanned build break mid-plan.

## Global Constraints

- **No new database migration, anywhere.** `LineStatus.sample_availability` is additive on a `JSONB` column (`line_status.statuses`, `crates/api/migrations/20260510023522_initial.sql:69-77`) — it just starts appearing inside the existing blob, exactly as `sample_stats` itself does today. Do not create a migration file in any task.
- **No new dependency, in either crate ecosystem.** Every change in this plan is either a new pure Rust type/function, a JSON-shape addition, or a TypeScript type/function addition — no new Cargo crate, no new npm package, no new Cargo feature flag.
- **Severity classification is unchanged, full stop.** This plan changes what *accompanies* a severity outcome (whether/why `sample_stats` is absent), never when `GoodService` vs. a worse severity fires. `classify()`, `escalate_from_sample_stats`, and every existing severity-threshold test must keep passing with identical assertions on `status.severity` — if a task's change moves a severity assertion, that is a bug in the task, not an intended side effect.
- **`compute_sample_stats` becomes `compute_sample_availability` and changes its return type from `Option<SampleStats>` to `SampleAvailability`; `infer_from_samples` changes from `Option<LineStatus>` to `LineStatus`.** These are real, deliberate signature changes (Decision 3), not accidental breakage — every call site and every test that pattern-matches on the old `Option` shape must be updated in Task 1, not deferred.
- **The `dataQuality`-before-`sampleAvailability` precedence rule is structural in the frontend, not optional polish.** `sampleUnavailableReason` (Task 5) MUST check `status.dataQuality === 'tfl'` before reading `status.sampleAvailability` at all — a TfL-quality status's `sampleAvailability` is `NoCoverage` by construction (Decision 4's "inert default," set in `crates/poller-tfl/src/schema.rs`) regardless of whether that's meaningful, and reading it without the precedence check would misreport every plain Tube/Overground/Elizabeth-line/tram status as a live pipeline gap. Every new frontend call site in Tasks 6-8 must go through `sampleUnavailableReason`/`formatSampleSummary`, never read `status.sampleAvailability` directly.
- **`normalize_for_diff` (both the aggregator's and the api crate's independently-implemented copies) must strip `sample_availability` alongside `sample_stats`, or `line_status_history` grows a row on every cycle a line's observed count merely fluctuates around the threshold.** This is a hard constraint carried forward from the design's own Error handling section, with equal weight to the original `sample_stats`-churn fix it extends — Task 2 is not optional polish.
- **`RepresentativeInfo.tsx` is explicitly unchanged by this plan.** It only ever renders the "stats present" case and `return null`s otherwise; the spec's Decision 6 scopes it out and this plan does not touch it. Do not add it to any task's file list.
- **`frontend/lib/api.ts` needs no changes.** Every typed fetcher (`getLineStatusForMode`, `getLineStatus`, `getStopPointDisruption`, confirmed at `lib/api.ts:70,78,88`) parses JSON straight into `LineStatusReport` via `fetchJson<T>` with no field-by-field mapping — adding `sampleAvailability` to the `LineStatus` interface (Task 5) is sufficient; no fetcher needs a matching edit. Do not add `lib/api.ts` to any task's file list.
- **`crates/aggregator`'s `lines_with_sample_coverage` (`main.rs:175-185`) is explicitly unchanged.** It keeps gating on `sample_stats.is_some()`, not `sample_availability` — per the spec's "Explicitly out of scope," splitting `sample_cycles`/daily-stats coverage by `NoCoverage` vs. `BelowThreshold` is a separate, larger change to `line_status_daily_stats` this plan does not make (see Not in this plan, below). The one edit this file needs is mechanical: its own test fixture (`status_with_stats`, `main.rs:211-220`) must gain the new required field — that is in Task 1's scope, nothing else in this file changes.
- **Dedup is untouched.** `crates/aggregator/src/dedup.rs` calls `relevant_departures`/`stats_from_departures` directly (`dedup.rs:99,176`), never `compute_sample_stats`/`compute_sample_availability` — no task in this plan touches `dedup.rs`.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file, run via `cargo test -p <crate>`. This plan's changes are all pure-function/type-level (no new route, no new query needing a live database), so no task needs the `db_tests`/`#[ignore = "requires a live database..."]` convention (`crates/api/src/routes/train.rs:491,697` for reference) — every test in this plan runs under a plain `cargo test`. Frontend: colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures — `npm run build` is required specifically because TypeScript's project-wide type-check is where the mechanical fixture fallout (Status note above) would otherwise surface unplanned.
- **Parallelizable tasks:** Tasks 2, 3, and 4 each depend only on Task 1 and touch disjoint files — they can be dispatched to separate subagents in parallel once Task 1 lands. Tasks 6, 7, and 8 each depend only on Task 5 and touch disjoint files — same. Task 5 must not start before Task 4 lands (it needs the wire shape's field names settled, even though nothing in Task 5 depends on Task 4's files at compile time).

---

### Task 1: `common::SampleAvailability` type + aggregator wiring (the core fix)

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/aggregator/src/aggregation.rs`
- Modify: `crates/aggregator/src/main.rs`
- Modify: `crates/poller-tfl/src/schema.rs`

**Interfaces:**
- Produces: `common::SampleAvailability` (enum, `NoCoverage` / `BelowThreshold { observed: usize, required: i64 }` / `Available(SampleStats)`), `SampleAvailability::no_coverage_default()`, `SampleAvailability::sample_stats()`. `LineStatus.sample_availability: SampleAvailability` (new, always-present field). `crates/aggregator/src/aggregation.rs::compute_sample_availability` (replaces `compute_sample_stats`). `infer_from_samples`'s new signature: `fn infer_from_samples(...) -> LineStatus` (was `-> Option<LineStatus>`).
- Consumed by: Task 2 (`normalize_for_diff` strips the new field), Task 3 (DLR pilot maps its own states onto this enum), Task 4 (`render.rs` reads `status.sample_availability`), Task 5 (frontend `SampleAvailability` TS type mirrors this).
- **Depends on:** nothing — this is the foundational task.

- [ ] **Step 1: Add the `SampleAvailability` enum to `crates/common/src/lib.rs`**

Add immediately after `SampleStats` (currently `lib.rs:675-681`):

```rust
/// Why a line's `sample_stats` is (or isn't) populated this cycle -- see
/// docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md
/// Decision 2. Answers only the single-cycle "did any configured sample
/// station have live departure data to look at at all" question --
/// deliberately does NOT distinguish "genuinely quiet" from "structurally
/// under-sampled" (both collapse into `BelowThreshold`; that distinction
/// needs a pattern over many cycles, which is `line_status_daily_stats`'s
/// `sample_cycles`'s job, not a single cycle's -- see this plan's Not in
/// this plan section), and does NOT distinguish "no row" from "row present
/// but stale" (that is 2026-08-31-sample-data-availability-design.md's
/// still-open, separate `stationSamples`-on-`/public/freshness` proposal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SampleAvailability {
    /// None of this line's `sample_stations` had a `StationSample` row in
    /// this cycle's load at all (`samples.get(crs).is_none()` for every
    /// configured station). A real, currently-invisible signal -- a
    /// polling gap, a brand-new custom line never yet polled, or a
    /// widespread LDBWS outage.
    NoCoverage,
    /// At least one configured station had a row, but the operator-filtered
    /// relevant-departure count fell below `min_sample_size`.
    BelowThreshold { observed: usize, required: i64 },
    /// At or above threshold; the real `SampleStats` this cycle produced is
    /// also carried, unchanged, on `LineStatus.sample_stats` -- not
    /// duplicated a second time on the wire (see `crates/api/src/render.rs`,
    /// Task 4).
    Available(SampleStats),
}

impl SampleAvailability {
    /// `#[serde(default = ...)]` shim for deserializing `line_status_history`
    /// rows written before this field existed. `write_line_status` itself
    /// always writes a freshly computed value -- this default is a
    /// read-compat fallback, not a real "unknown" state.
    pub fn no_coverage_default() -> Self {
        SampleAvailability::NoCoverage
    }

    /// Extracts the `Available` payload, if any -- kept for the two call
    /// sites below that still want the old `Option<SampleStats>` shape
    /// internally, so `relevant_departures`/`stats_from_departures` don't
    /// need a second return channel added just for this.
    pub fn sample_stats(&self) -> Option<SampleStats> {
        match self {
            SampleAvailability::Available(stats) => Some(stats.clone()),
            _ => None,
        }
    }
}
```

Then extend `LineStatus` (`lib.rs:325-335`):

```rust
pub struct LineStatus {
    pub severity: Severity,
    pub reason: String,
    pub validity: ValidityPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disruption: Option<Disruption>,
    #[serde(default)]
    pub data_quality: DataQuality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_stats: Option<SampleStats>, // UNCHANGED
    #[serde(default = "SampleAvailability::no_coverage_default")]
    pub sample_availability: SampleAvailability, // NEW, always present
}
```

- [ ] **Step 2: Add a unit test for the enum's own serde shape**

Add near the existing `defaults_tests` module in `lib.rs`:

```rust
#[cfg(test)]
mod sample_availability_tests {
    use super::*;

    #[test]
    fn wire_tags_match_the_design_spec() {
        assert_eq!(serde_json::to_value(SampleAvailability::NoCoverage).unwrap(), serde_json::json!({"state": "no-coverage"}));
        assert_eq!(
            serde_json::to_value(SampleAvailability::BelowThreshold { observed: 2, required: 3 }).unwrap(),
            serde_json::json!({"state": "below-threshold", "observed": 2, "required": 3})
        );
    }

    #[test]
    fn sample_stats_accessor_extracts_only_the_available_variant() {
        assert_eq!(SampleAvailability::NoCoverage.sample_stats(), None);
        assert_eq!(SampleAvailability::BelowThreshold { observed: 0, required: 3 }.sample_stats(), None);
        let stats = SampleStats { total: 5, delayed: 1, cancelled: 0, skipped: 0, avg_delay_minutes: 2.0 };
        assert_eq!(SampleAvailability::Available(stats.clone()).sample_stats(), Some(stats));
    }
}
```

- [ ] **Step 3: Rewrite `compute_sample_stats` as `compute_sample_availability` in `crates/aggregator/src/aggregation.rs`**

Replace the current body (`aggregation.rs:730-743`):

```rust
fn compute_sample_availability(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> common::SampleAvailability {
    let thresholds = thresholds_for(defaults, &line.severity_overrides);
    let has_any_row = line.sample_stations.iter().any(|crs| samples.contains_key(crs));
    if !has_any_row {
        return common::SampleAvailability::NoCoverage;
    }

    let relevant = relevant_departures(line, samples);
    if (relevant.len() as i64) < thresholds.min_sample_size {
        return common::SampleAvailability::BelowThreshold {
            observed: relevant.len(),
            required: thresholds.min_sample_size,
        };
    }

    common::SampleAvailability::Available(stats_from_departures(&relevant, line, &thresholds))
}
```

`has_any_row` is deliberately not folded into `relevant_departures` itself, per Decision 2 -- that function's shared job (`compute_sample_availability`, `infer_from_samples`'s "most cited reason" pass at line 777, and `dedup::dedup_new_sample_stats`) is "give me the relevant departures," and a second return channel would change its signature for two callers that don't need the distinction.

- [ ] **Step 4: Rewrite `infer_from_samples`**

Replace the current body (`aggregation.rs:745-809`) — note this now returns `LineStatus`, not `Option<LineStatus>`:

```rust
fn infer_from_samples(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> LineStatus {
    let availability = compute_sample_availability(line, samples, defaults);
    let Some(stats) = availability.sample_stats() else {
        // NoCoverage or BelowThreshold: severity is unchanged (still
        // GoodService, same as today's `.unwrap_or_else(good_service)`
        // fallback), but -- unlike today -- the reason it's absent is no
        // longer discarded here. This is the core fix this plan exists for.
        let mut status = good_service();
        status.sample_availability = availability;
        return status;
    };
    let thresholds = thresholds_for(defaults, &line.severity_overrides);

    let cancel_rate = stats.cancelled as f64 / stats.total as f64;
    let delay_rate = stats.delayed as f64 / stats.total as f64;
    let skip_rate = stats.skipped as f64 / stats.total as f64;

    let (severity, mut reason) = classify(
        cancel_rate,
        delay_rate,
        skip_rate,
        &thresholds,
        stats.total,
        stats.cancelled,
        stats.delayed,
        stats.skipped,
    );
    if severity == Severity::GoodService {
        let mut status = good_service();
        status.sample_stats = Some(stats);
        status.sample_availability = availability;
        return status;
    }

    let relevant = relevant_departures(line, samples);
    let reasons: Vec<&str> = relevant
        .iter()
        .filter_map(|d| d.delay_reason.as_deref().or(d.cancel_reason.as_deref()))
        .collect();
    if let Some(most_common) = most_common(&reasons) {
        reason.push_str(&format!(" (most cited: {most_common})"));
    }

    let mut affected_stops: Vec<String> = samples.keys().cloned().collect();
    affected_stops.sort();

    LineStatus {
        severity,
        reason: reason.clone(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: Some(Disruption {
            category: "RealTime".to_string(),
            description: reason,
            affected_stops,
            affected_routes: vec![],
            source: Some("ldbws-sampling".to_string()),
        }),
        data_quality: DataQuality::LdbwsInferred,
        sample_stats: Some(stats),
        sample_availability: availability,
    }
}
```

Note both `good_service()`-based early returns above independently set `sample_availability` after construction, matching Step 6's placeholder-then-overwrite pattern in `good_service()` itself.

- [ ] **Step 5: Update `aggregate()`'s two Layer-2 call sites (`aggregation.rs`, currently lines 89-107)**

```rust
    for line in lines.values() {
        let report = reports.get_mut(&line.id).unwrap();
        if report.statuses.is_empty() {
            report.statuses.push(infer_from_samples(line, samples, defaults));
            continue;
        }
        let availability = compute_sample_availability(line, samples, defaults);
        for status in &mut report.statuses {
            status.sample_availability = availability.clone();
        }
        if let Some(stats) = availability.sample_stats() {
            let thresholds = thresholds_for(defaults, &line.severity_overrides);
            for status in &mut report.statuses {
                let (escalated, annotation) = escalate_from_sample_stats(status.severity, &stats, &thresholds);
                status.severity = escalated;
                if let Some(annotation) = annotation {
                    status.reason.push_str(&format!(" ({annotation})"));
                }
                status.sample_stats = Some(stats.clone());
            }
        }
    }
```

This is the second, previously-undocumented fix this plan makes: today, when a line has an active incident and zero coverage, `compute_sample_stats` returning `None` skips the whole block and every incident-derived status keeps `sample_stats: None` with no record of why. After this change, `sample_availability` is unconditionally attached to every status on the line every cycle; only the severity-escalation/`sample_stats`-attachment sub-block stays conditional on `Available`, unchanged from today's behavior.

- [ ] **Step 6: Fix the two remaining production `LineStatus` literal construction sites**

`status_from_incident` (`aggregation.rs:144-151`) — add a placeholder, always immediately overwritten by Step 5's `for status in &mut report.statuses { status.sample_availability = availability.clone(); }` on every code path that reaches this status (Layer 2 always runs for every line, per `aggregate()`'s structure):

```rust
    LineStatus {
        severity,
        reason,
        validity: validity_for_output(&incident.validity),
        disruption: Some(disruption),
        data_quality: if incident.is_planned { DataQuality::Planned } else { DataQuality::Knowledgebase },
        sample_stats: None,
        sample_availability: SampleAvailability::NoCoverage, // always overwritten by aggregate()'s Layer 2, immediately after construction
    }
```

`good_service()` (`aggregation.rs:906-915`) — same placeholder-always-overwritten pattern (every caller of `good_service()` — both inside `infer_from_samples`, per Step 4 — sets `.sample_availability` on the returned value immediately):

```rust
fn good_service() -> LineStatus {
    LineStatus {
        severity: Severity::GoodService,
        reason: "Good Service".to_string(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: None,
        data_quality: DataQuality::LdbwsInferred,
        sample_stats: None,
        sample_availability: SampleAvailability::NoCoverage, // placeholder; always overwritten by every caller
    }
}
```

- [ ] **Step 7: Fix the one production `LineStatus` literal site outside this crate**

`crates/poller-tfl/src/schema.rs`'s `map_status` (currently lines 132-150) — this is Decision 4's "inert default," and is also its *final* correct value (Task 3 does not need to revisit this line): every TfL-quality status gets `sample_availability: SampleAvailability::NoCoverage` unconditionally, since this code path never attempts any sampling and the frontend's `dataQuality === 'tfl'` precedence check (Global Constraints, Task 5) means this value is never actually read for a plain TfL line:

```rust
    LineStatus {
        severity,
        reason: reason_text(status),
        validity: select_validity(&status.validity_periods, now, fallback),
        disruption: status.disruption.as_ref().map(|disruption| Disruption { /* ...unchanged... */ }),
        data_quality: DataQuality::Tfl,
        sample_stats: None,
        sample_availability: common::SampleAvailability::NoCoverage,
    }
```

- [ ] **Step 8: Fix the aggregator's own test-only `LineStatus` construction sites**

`crates/aggregator/src/main.rs`'s `status_with_stats` (currently lines 211-220) — add `sample_availability: SampleAvailability::NoCoverage,` (or `Available(stats.clone())` matching whichever the test's own intent is; this specific helper's tests only assert on `sample_stats`, so a flat `NoCoverage` placeholder is sufficient and keeps the diff minimal). Add `use common::SampleAvailability;` to that test module's imports (currently `use common::{DataQuality, LineStatus, SampleStats, Severity, ValidityPeriod};` at `main.rs:189`).

- [ ] **Step 9: Rewrite this crate's own existing `infer_from_samples`-dependent tests**

`infer_from_samples_returns_none_below_min_sample_size` (`aggregation.rs:1160-1177`) — the old `.is_none()` assertion no longer type-checks (the function no longer returns `Option`). Replace with an assertion on the new, more informative shape:

```rust
    #[test]
    fn infer_from_samples_returns_below_threshold_availability_with_the_correct_counts() {
        // swr-alton.toml: sample_stations = ["AHT", "FRM", "AON"]
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let mut samples = HashMap::new();
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                // Only 2 relevant departures, below the default min_sample_size of 3.
                departures: vec![departure("AON", 0, false), departure("AON", 0, false)],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults);
        assert_eq!(status.severity, Severity::GoodService, "severity behavior is unchanged by this plan");
        assert_eq!(status.sample_availability, SampleAvailability::BelowThreshold { observed: 2, required: 3 });
        assert_eq!(status.sample_stats, None);
    }
```

Every other `infer_from_samples(alton, &samples, &defaults).expect("...")` call site (`aggregation.rs:1200, 1226, 1249, 1275, 1307, 1396, 1415`) drops the now-unnecessary `.expect(...)` — `infer_from_samples` always returns a plain `LineStatus`. Do a direct find-and-replace of `.expect("should classify")` / `.expect("should still classify (Good Service)")` → nothing (just `infer_from_samples(alton, &samples, &defaults)`), at all 7 sites.

- [ ] **Step 10: Add the `NoCoverage` and `Available` regression tests the design's Testing section calls for**

Add alongside the rewritten test from Step 9:

```rust
    #[test]
    fn infer_from_samples_returns_no_coverage_when_no_sample_station_has_a_row() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let samples = HashMap::new(); // no rows for AHT/FRM/AON at all
        let status = infer_from_samples(alton, &samples, &defaults);
        assert_eq!(status.severity, Severity::GoodService);
        assert_eq!(status.sample_availability, SampleAvailability::NoCoverage);
        assert_eq!(status.sample_stats, None);
    }

    #[test]
    fn infer_from_samples_at_or_above_threshold_yields_available_matching_compute_sample_stats_today() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let mut samples = HashMap::new();
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![departure("AON", 0, false), departure("AON", 0, false), departure("AON", 0, false)],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults);
        let SampleAvailability::Available(stats) = &status.sample_availability else {
            panic!("expected Available, got {:?}", status.sample_availability);
        };
        assert_eq!(status.sample_stats.as_ref(), Some(stats));
    }
```

Also add a per-line `severity_overrides` case for `compute_sample_availability`'s `BelowThreshold.required`, mirroring the existing override tests at `crates/common/src/lib.rs:839-849` (a line with `min_sample_size` overridden to e.g. `5` should report `required: 5`, not the global default `3`) — construct a `LineDefinition` with `severity_overrides: HashMap::from([("min_sample_size".to_string(), 5.0)])` and 4 relevant departures, asserting `BelowThreshold { observed: 4, required: 5 }`.

Finally, add the incident-path regression test the design's Testing section names: a line with an active incident (so `report.statuses` is non-empty entering Layer 2) and zero `StationSample` coverage now carries `sample_availability: NoCoverage` on its incident-derived status(es) — today this information is dropped entirely at this call site. Build this using the existing `aggregate_with_defaults` test helper (`aggregation.rs:959`) with a matching `IncidentMessage` fixture and an empty `samples` map, asserting `report.statuses[0].sample_availability == SampleAvailability::NoCoverage`.

- [ ] **Step 11: Run the tests**

Run: `cargo build --workspace && cargo test -p common -p aggregator`
Expected: PASS. Note `cargo test --workspace` will **not** yet fully pass — `crates/api` and `crates/poller-tfl`'s own test-only `LineStatus` literals (Status note above) are fixed in Tasks 3 and 4, not here. `cargo build --workspace` (production code only) must pass after this task.

- [ ] **Step 12: Commit**

```bash
git add crates/common/src/lib.rs crates/aggregator/src/aggregation.rs crates/aggregator/src/main.rs crates/poller-tfl/src/schema.rs
git commit -m "Add common::SampleAvailability and wire it through the aggregator's sample-coverage inference"
```

---

### Task 2: Diff suppression — both `normalize_for_diff` copies

**Files:**
- Modify: `crates/aggregator/src/queries.rs`
- Modify: `crates/api/src/data/queries.rs`

**Interfaces:**
- Produces: no new public interface — extends the existing private `normalize_entry_for_diff`/`normalize_for_diff` functions in both crates.
- Consumed by: `write_line_status` (`crates/aggregator/src/queries.rs:258`) and `tfl_statuses_changed` (`crates/api/src/data/queries.rs:299`) — both unchanged call sites, only the stripped-field set grows.
- **Depends on:** Task 1 (the field must exist for this stripping to matter at runtime — not a compile-time dependency, since both functions operate on generic `serde_json::Value`, but land this after Task 1 to avoid a live history-spam regression window in between).

Per this plan's Global Constraints: without this, `line_status_history` grows a row on every cycle a line's observed count merely fluctuates around `min_sample_size` (e.g. `BelowThreshold{2,3}` → `BelowThreshold{3,3}` → `Available(...)` → back again), none of which reflect a real change in the underlying disruption.

- [ ] **Step 1: Extend the aggregator's `normalize_entry_for_diff`**

In `crates/aggregator/src/queries.rs`, `normalize_entry_for_diff` (currently lines 174-188):

```rust
fn normalize_entry_for_diff(entry: &serde_json::Value) -> serde_json::Value {
    let mut entry = entry.clone();
    if let Some(validity) = entry.get_mut("validity").and_then(|v| v.as_object_mut()) {
        validity.remove("from_date");
    }
    if let Some(obj) = entry.as_object_mut() {
        obj.remove("sample_stats");
        obj.remove("sample_availability");
    }
    if let Some(reason) = entry.get_mut("reason")
        && let Some(stripped) = reason.as_str().map(strip_live_sample_annotation)
    {
        *reason = serde_json::Value::String(stripped.to_string());
    }
    entry
}
```

Also extend this function's own doc comment (`queries.rs:140-161`) to name `sample_availability` alongside `sample_stats` as a field that "changes every cycle, independent of real disruption state."

- [ ] **Step 2: Add the regression test**

Alongside the existing `normalize_for_diff_ignores_sample_stats_changes` (`queries.rs:631`) and `normalize_for_diff_still_detects_real_changes` (`queries.rs:665`):

```rust
    #[test]
    fn normalize_for_diff_ignores_sample_availability_only_changes() {
        let a = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-09-01T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "ldbws-inferred",
            "sample_availability": { "state": "below-threshold", "observed": 2, "required": 3 }
        }]);
        let b = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-09-01T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "ldbws-inferred",
            "sample_availability": { "state": "below-threshold", "observed": 3, "required": 3 }
        }]);
        assert_eq!(normalize_for_diff(&a), normalize_for_diff(&b));

        let c = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-09-01T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "ldbws-inferred",
            "sample_availability": { "state": "no-coverage" }
        }]);
        assert_eq!(normalize_for_diff(&a), normalize_for_diff(&c), "no-coverage <-> below-threshold churn must not register as changed either");
    }
```

- [ ] **Step 3: Extend `crates/api/src/data/queries.rs`'s twin**

`normalize_for_diff` (currently `queries.rs:308-318`):

```rust
fn normalize_for_diff(statuses: &serde_json::Value) -> serde_json::Value {
    let mut statuses = statuses.clone();
    if let Some(entries) = statuses.as_array_mut() {
        for entry in entries {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("sample_stats");
                obj.remove("sample_availability");
            }
        }
    }
    statuses
}
```

Update this function's own doc comment (`queries.rs:306-307`) and `tfl_statuses_changed`'s (`queries.rs:287-298`) the same way as Step 1.

- [ ] **Step 4: Add the sibling regression test**

Alongside `tfl_statuses_changed_ignores_sample_stats_only_differences` (`queries.rs:967`):

```rust
    #[test]
    fn tfl_statuses_changed_ignores_sample_availability_only_differences() {
        let existing = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_availability": { "state": "no-coverage" }
        }]);
        let incoming = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_availability": { "state": "below-threshold", "observed": 0, "required": 1 }
        }]);
        assert!(!tfl_statuses_changed(Some(&existing), &incoming));
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p aggregator -p api`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/aggregator/src/queries.rs crates/api/src/data/queries.rs
git commit -m "Strip sample_availability from both normalize_for_diff copies, alongside sample_stats"
```

---

### Task 3: DLR pilot mapping onto `SampleAvailability`

**Files:**
- Modify: `crates/poller-tfl/src/main.rs`
- Modify: `crates/poller-tfl/src/schema.rs` (test module only)

**Interfaces:**
- Produces: `merge_dlr_sample_stats` extended to also set `Available`; new `mark_dlr_pending(reports: &mut [LineStatusReport])` function.
- Consumed by: `poll_once`'s existing `match poll_dlr_sample_stats(...).await` (`main.rs:141-153`) — one new match arm.
- **Depends on:** Task 1 (`SampleAvailability` type must exist).

Per Decision 4: the DLR pilot is a second, independent producer of `SampleStats` that must present one consistent signal through the same enum, not a fourth bespoke state. `dlr_pilot_enabled` defaults to `false` (`crates/poller-tfl/src/config.rs:60`), so none of this is live by default — this task only prepares the mapping.

- [ ] **Step 1: Extend `merge_dlr_sample_stats`**

Current body (`main.rs:179-186`):

```rust
fn merge_dlr_sample_stats(reports: &mut [common::LineStatusReport], stats: common::SampleStats) {
    let line_id = dlr_line_id();
    for report in reports.iter_mut().filter(|r| r.id == line_id) {
        for status in &mut report.statuses {
            status.sample_stats = Some(stats.clone());
            status.sample_availability = common::SampleAvailability::Available(stats.clone());
        }
    }
}
```

- [ ] **Step 2: Add `mark_dlr_pending`**

New function, placed next to `merge_dlr_sample_stats`:

```rust
/// The DLR pilot has no tunable `min_sample_size`-equivalent -- it
/// structurally needs at least one resolved trip before it can report
/// anything, so `required: 1` is literally true (not a borrowed LDBWS
/// constant) and `observed: 0` accurately reports "zero trips have
/// resolved yet." An honest, deliberately imperfect reuse of
/// `BelowThreshold`'s shape for a mechanically different producer
/// (per-trip resolution warm-up, not a station-count threshold) -- see
/// docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md
/// Decision 4 and its Open Question 2.
fn mark_dlr_pending(reports: &mut [common::LineStatusReport]) {
    let line_id = dlr_line_id();
    for report in reports.iter_mut().filter(|r| r.id == line_id) {
        for status in &mut report.statuses {
            status.sample_availability = common::SampleAvailability::BelowThreshold { observed: 0, required: 1 };
        }
    }
}
```

- [ ] **Step 3: Wire the new state into `poll_once`**

Current match (`main.rs:141-153`):

```rust
    if config.dlr_pilot_enabled {
        match poll_dlr_sample_stats(client, config, dlr_state).await {
            Ok(Some(stats)) => merge_dlr_sample_stats(&mut reports, stats),
            Ok(None) => mark_dlr_pending(&mut reports),
            Err(err) => {
                // The DLR pilot failing must never take down the rest of
                // the TfL line-status batch -- log and post everything
                // else as normal, same as any other line keeps reporting
                // if one call in a multi-call cycle has a bad day. Left at
                // whatever schema.rs already set (NoCoverage) for
                // sample_availability -- a known, accepted simplification,
                // not a gap this task claims to close (Decision 4).
                tracing::warn!(error = ?err, "DLR arrivals-diffing pilot failed this cycle; continuing without it");
            }
        }
    }
```

(Only the `Ok(None)` arm changes — was `Ok(None) => {}`.)

- [ ] **Step 4: Fix `schema.rs`'s existing test to assert the new field explicitly**

The existing test at `schema.rs:~276` already asserts `status.sample_stats.is_none()` — extend it (same test function) with:

```rust
        assert!(matches!(status.sample_availability, common::SampleAvailability::NoCoverage));
```

- [ ] **Step 5: Fix `main.rs`'s own test fixtures and extend its `merge_dlr_sample_stats` coverage**

`dlr_sample_stats_are_merged_onto_the_matching_line_only` (`main.rs:351-389`) constructs two `common::LineStatus` literals (lines 359, 373) that need `sample_availability: common::SampleAvailability::NoCoverage,` added (matching what `schema.rs`'s `map_status` would actually produce for a freshly-parsed status). Extend the test's own assertions:

```rust
        assert_eq!(reports[0].statuses[0].sample_availability, common::SampleAvailability::Available(stats));
        assert_eq!(reports[1].statuses[0].sample_availability, common::SampleAvailability::NoCoverage, "unaffected line's availability must be untouched");
```

Add two new tests alongside it:

```rust
    #[test]
    fn dlr_ok_none_marks_below_threshold_pending_on_the_matching_line_only() {
        let mut reports = vec![
            common::LineStatusReport {
                id: "tfl-dlr".to_string(),
                name: "DLR".to_string(),
                mode_name: "dlr".to_string(),
                operators: vec!["TfL".to_string()],
                statuses: vec![common::LineStatus {
                    severity: common::Severity::GoodService,
                    reason: "Good Service".to_string(),
                    validity: common::ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
                    disruption: None,
                    data_quality: common::DataQuality::Tfl,
                    sample_stats: None,
                    sample_availability: common::SampleAvailability::NoCoverage,
                }],
            },
        ];

        mark_dlr_pending(&mut reports);

        assert_eq!(
            reports[0].statuses[0].sample_availability,
            common::SampleAvailability::BelowThreshold { observed: 0, required: 1 }
        );
        assert_eq!(reports[0].statuses[0].sample_stats, None, "Ok(None) must not fabricate sample_stats");
    }
```

Add a third test (or a code comment if a runnable test would just be re-asserting `poll_once`'s untouched `Err` branch, which needs no new assertion): document that an `Err` from `poll_dlr_sample_stats` leaves `sample_availability` at whatever `schema.rs` already set (`NoCoverage`) — Decision 4's accepted simplification — by adding a one-line comment above the `Err(err) => { ... }` arm in `poll_once` (already included in Step 3's snippet above) rather than a redundant unit test, since `poll_once` itself has no existing unit-test harness to extend without mocking HTTP (out of scope for this task).

- [ ] **Step 6: Run the tests**

Run: `cargo test -p poller-tfl`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/poller-tfl/src/main.rs crates/poller-tfl/src/schema.rs
git commit -m "Map the DLR pilot's Ok(Some)/Ok(None)/Err states onto SampleAvailability"
```

---

### Task 4: API wire shape — `sampleAvailability` on `status_to_json`

**Files:**
- Modify: `crates/api/src/render.rs`
- Modify: `crates/api/src/routes/line_status.rs` (test fixture only)

**Interfaces:**
- Produces: `sampleAvailability` key, always present in `status_to_json`'s output, shape `{"state": "no-coverage"}` / `{"state": "below-threshold", "observed": N, "required": N}` / `{"state": "available"}`.
- Consumed by: the frontend wire contract Task 5 types against.
- **Depends on:** Task 1.

- [ ] **Step 1: Extend `status_to_json`**

Current body (`render.rs:47-88`), add the new block after the existing `sampleStats` block (lines 62-70):

```rust
    if let Some(stats) = &status.sample_stats {
        out["sampleStats"] = json!({
            "total": stats.total,
            "delayed": stats.delayed,
            "cancelled": stats.cancelled,
            "skipped": stats.skipped,
            "avgDelayMinutes": stats.avg_delay_minutes,
        });
    }

    out["sampleAvailability"] = match &status.sample_availability {
        common::SampleAvailability::NoCoverage => json!({ "state": "no-coverage" }),
        common::SampleAvailability::BelowThreshold { observed, required } =>
            json!({ "state": "below-threshold", "observed": observed, "required": required }),
        common::SampleAvailability::Available(_) => json!({ "state": "available" }),
    };
```

The `Available` case deliberately does not re-serialize the `SampleStats` payload — `sampleStats` (unchanged, still conditional on `Some`) already carries it, matching this module's own established stored-vs-public-shape split (its doc comment, `render.rs:1-8`).

- [ ] **Step 2: Fix this file's own two test fixtures**

`sample_report` (`render.rs:100-115`) and `overlay_status` (`render.rs:220-...`) both construct `LineStatus` literals — add `sample_availability: common::SampleAvailability::NoCoverage,` to both (import `common::SampleAvailability` alongside the existing `use common::{DataQuality, Disruption, SampleStats, ValidityPeriod};` at `render.rs:98`).

- [ ] **Step 3: Extend the existing `sampleStats`-adjacent tests**

Alongside `sample_stats_included_when_present`/`sample_stats_omitted_when_absent` (`render.rs:194,213`):

```rust
    #[test]
    fn sample_availability_is_always_present_unlike_sample_stats() {
        let report = sample_report(None); // sample_stats is None
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(json["lineStatuses"][0]["sampleAvailability"], serde_json::json!({"state": "no-coverage"}));
    }

    #[test]
    fn sample_availability_below_threshold_shape() {
        let mut report = sample_report(None);
        report.statuses[0].sample_availability = SampleAvailability::BelowThreshold { observed: 2, required: 3 };
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["sampleAvailability"],
            serde_json::json!({"state": "below-threshold", "observed": 2, "required": 3})
        );
    }

    #[test]
    fn sample_availability_available_case_does_not_duplicate_sample_stats_fields() {
        let mut report = sample_report(None);
        let stats = SampleStats { total: 10, delayed: 4, cancelled: 1, skipped: 2, avg_delay_minutes: 6.5 };
        report.statuses[0].sample_stats = Some(stats.clone());
        report.statuses[0].sample_availability = SampleAvailability::Available(stats);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(json["lineStatuses"][0]["sampleAvailability"], serde_json::json!({"state": "available"}));
        assert!(json["lineStatuses"][0]["sampleAvailability"].get("total").is_none(), "Available must not re-embed SampleStats fields");
    }
```

(Add `use common::SampleAvailability;` to this test module's imports.)

- [ ] **Step 4: Fix `routes/line_status.rs`'s own test fixture (required companion change)**

`a_status` (`routes/line_status.rs:403-411`) also constructs a `LineStatus` literal, in the same `api` crate — `cargo test -p api` compiles this file too, so it needs the same one-line addition: `sample_availability: common::SampleAvailability::NoCoverage,` (or `DataQuality`'s existing import path — check whether `SampleAvailability` needs a new `use` in this file's test module).

- [ ] **Step 5: Run the tests**

Run: `cargo test -p api`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/render.rs crates/api/src/routes/line_status.rs
git commit -m "Add sampleAvailability to the public wire shape"
```

---

### Task 5: Frontend wire type + shared helper + mechanical fixture compatibility

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/sampleStats.ts`
- Modify: `frontend/lib/sampleStats.test.ts`
- Modify: `frontend/app/lines/AllLinesTable.test.tsx`
- Modify: `frontend/components/IssueList.test.tsx`
- Modify: `frontend/components/LineStatusCard.test.tsx`
- Modify: `frontend/components/RepresentativeInfo.test.tsx`
- Modify: `frontend/lib/history.test.ts`
- Modify: `frontend/lib/severity.test.ts`
- Modify: `frontend/lib/stationIssues.test.ts`
- Modify: `frontend/lib/validity.test.ts`

**Interfaces:**
- Produces: `SampleAvailability` TS type (union on `state`). `LineStatus.sampleAvailability: SampleAvailability` (new, required field). `sampleUnavailableReason(status: LineStatus): string | null`. `representativeStatus(statuses: LineStatus[]): LineStatus | undefined`. `formatSampleSummary`'s signature changes from `(stats: SampleStats | undefined) => string` to `(status: LineStatus | undefined) => string`.
- Consumed by: Task 6 (`AllLinesTable.tsx`), Task 7 (`LineStatusCard.tsx`, `app/page.tsx`), Task 8 (`app/stations/[crs]/page.tsx`).
- **Depends on:** Task 4 (wire contract).

This is the largest single frontend task in this plan, for two reasons the Status note above already flagged: (1) `formatSampleSummary`'s signature change is a real breaking change to an already-tested, already-used function (Open Question 5 in the spec), not just an additive one; (2) making `sampleAvailability` a required field on `LineStatus` means every existing test file with a `LineStatus` object literal fails `tsc`/`next build`'s project-wide type-check, not just the files this feature conceptually touches — 8 files beyond `sampleStats.test.ts` itself, confirmed by this plan's own grep pass (Status note above).

- [ ] **Step 1: Add the `SampleAvailability` type and extend `LineStatus`**

In `frontend/lib/types.ts`, add before `LineStatus` (currently at line 67):

```typescript
export type SampleAvailability =
  | { state: 'no-coverage' }
  | { state: 'below-threshold'; observed: number; required: number }
  | { state: 'available' };
```

Extend `LineStatus` (currently `types.ts:67-75`):

```typescript
export interface LineStatus {
  statusSeverity: number;
  statusSeverityDescription: string;
  reason: string;
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;
  sampleStats?: SampleStats;       // UNCHANGED
  sampleAvailability: SampleAvailability; // NEW, always present
}
```

- [ ] **Step 2: Rewrite `frontend/lib/sampleStats.ts`**

Full new content:

```typescript
import type { LineStatus, SampleStats } from './types';

/** The aggregator attaches the same sample-derived stats to every status on
 * a line's report, so the first one found is representative of all of them
 * — the rationale `RepresentativeInfo` already documents, extracted here
 * because four call sites had independently reimplemented it. */
export function firstSampleStats(statuses: LineStatus[]): SampleStats | undefined {
  return statuses.find((status) => status.sampleStats)?.sampleStats;
}

/** The first status carrying real stats if any does, else the first status
 * overall — so a caller always has a `dataQuality`/`sampleAvailability` to
 * build a reason from, even when nothing on the line has stats. Returns
 * `undefined` only for an empty array. */
export function representativeStatus(statuses: LineStatus[]): LineStatus | undefined {
  return statuses.find((s) => s.sampleStats) ?? statuses[0];
}

/** `null` rather than 0 for an empty sample: "0% cancelled" out of nothing
 * is a claim the data doesn't support. */
export function cancelledPercent(stats: SampleStats | undefined): number | null {
  if (!stats || stats.total === 0) return null;
  return Math.round((stats.cancelled / stats.total) * 100);
}

/** The human-readable reason sample stats aren't shown, or `null` when real
 * stats are available and the caller should render numbers instead. MUST
 * check `dataQuality` before `sampleAvailability` — a TfL-quality status's
 * `sampleAvailability` is `'no-coverage'` by construction (it never went
 * through the aggregator or DLR pilot), not a meaningful live-pipeline-gap
 * signal. See this app's plan/spec docs for
 * docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md's
 * Decision 1/4. */
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
```

- [ ] **Step 3: Rewrite `frontend/lib/sampleStats.test.ts`**

Full new content:

```typescript
import { describe, it, expect } from 'vitest';
import { cancelledPercent, firstSampleStats, formatSampleSummary, representativeStatus, sampleUnavailableReason } from './sampleStats';
import type { LineStatus, SampleStats } from './types';

const stats: SampleStats = { total: 160, delayed: 142, cancelled: 8, skipped: 1, avgDelayMinutes: 12.44 };

function status(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    ...overrides,
  };
}

describe('firstSampleStats', () => {
  it('returns undefined when nothing carries stats', () => {
    expect(firstSampleStats([status()])).toBeUndefined();
  });

  it('returns the first status that carries stats', () => {
    expect(firstSampleStats([status(), status({ sampleStats: stats })])).toBe(stats);
  });
});

describe('representativeStatus', () => {
  it('returns the first status carrying real stats when any exists', () => {
    const withStats = status({ sampleStats: stats });
    expect(representativeStatus([status(), withStats])).toBe(withStats);
  });

  it('falls back to the first status overall when none carries stats', () => {
    const first = status({ reason: 'first' });
    expect(representativeStatus([first, status({ reason: 'second' })])).toBe(first);
  });

  it('returns undefined only for an empty array', () => {
    expect(representativeStatus([])).toBeUndefined();
  });
});

describe('cancelledPercent', () => {
  it('rounds to a whole percentage', () => {
    expect(cancelledPercent(stats)).toBe(5);
  });

  it('returns null rather than dividing by zero on an empty sample', () => {
    expect(cancelledPercent({ ...stats, total: 0 })).toBeNull();
  });

  it('returns null for missing stats', () => {
    expect(cancelledPercent(undefined)).toBeNull();
  });
});

describe('sampleUnavailableReason', () => {
  it('returns null when sampleStats is present', () => {
    expect(sampleUnavailableReason(status({ sampleStats: stats }))).toBeNull();
  });

  it('returns the TfL copy when dataQuality is tfl, regardless of sampleAvailability', () => {
    const tflStatus = status({ dataQuality: 'tfl', sampleAvailability: { state: 'below-threshold', observed: 0, required: 1 } });
    expect(sampleUnavailableReason(tflStatus)).toBe("Not measured by this app — status is TfL's own.");
  });

  it('returns the no-coverage copy', () => {
    expect(sampleUnavailableReason(status({ sampleAvailability: { state: 'no-coverage' } }))).toBe(
      'No live departure data received for this line yet.',
    );
  });

  it('returns the below-threshold copy', () => {
    expect(
      sampleUnavailableReason(status({ sampleAvailability: { state: 'below-threshold', observed: 2, required: 3 } })),
    ).toBe('Too few live departures sampled to report a rate right now.');
  });
});

describe('formatSampleSummary', () => {
  it('renders the one-line summary used across cards, rows and tables', () => {
    expect(formatSampleSummary(status({ sampleStats: stats }))).toBe('Avg delay 12.4 min · 5% cancelled');
  });

  it('says so when there is no status at all', () => {
    expect(formatSampleSummary(undefined)).toBe('No sample data');
  });

  it('renders the no-coverage reason when there is a status but no stats', () => {
    expect(formatSampleSummary(status())).toBe('No live departure data received for this line yet.');
  });
});
```

- [ ] **Step 4: Mechanical fixture fix — add `sampleAvailability` to every remaining existing `LineStatus` literal**

For each file below, add `sampleAvailability: { state: 'no-coverage' }` (the neutral, behavior-preserving default — none of these tests assert on the new field, so its exact value doesn't matter as long as it type-checks) to every existing `LineStatus`-typed object literal. This is a pure mechanical/compile-fix step with no behavioral intent — do not add new assertions here (Task 6/7/8 add feature-specific tests in their own files).

  - `frontend/app/lines/AllLinesTable.test.tsx`: 4 sites, at lines 37, 51, 242, 301 (grep `dataQuality:` to re-locate exact lines if they've shifted).
  - `frontend/components/IssueList.test.tsx`: 12 sites — 4 shared top-level `const` fixtures (`minorNow`, `severePlanned`, `inferredNow`, `plannedRange`, around lines 22-53) plus 8 more inline literals further down the file (grep `dataQuality:` for exact locations).
  - `frontend/components/LineStatusCard.test.tsx`: 3 sites — the top-level `report` fixture (lines 7-23) and two inline literals inside the `mixed` fixture in the "picks the more severe status" test (lines 50-68).
  - `frontend/components/RepresentativeInfo.test.tsx`: 1 site.
  - `frontend/lib/history.test.ts`: 1 site.
  - `frontend/lib/severity.test.ts`: 2 sites.
  - `frontend/lib/stationIssues.test.ts`: 2 sites.
  - `frontend/lib/validity.test.ts`: 1 site.

For each site, add the one line `sampleAvailability: { state: 'no-coverage' },` next to that literal's existing `dataQuality:`/`validityPeriods:` fields — no other change in any of these 8 files.

- [ ] **Step 5: Run the tests and the build**

Run: `npm test && npm run build` (both from `frontend/`)
Expected: PASS, with no new failures — this is the point at which the mechanical fallout from Step 4 either resolves cleanly or surfaces a missed site (re-run `grep -rn "dataQuality:" frontend --include=*.test.ts --include=*.test.tsx` and diff against the Step 4 list if `tsc` still fails).

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/sampleStats.ts frontend/lib/sampleStats.test.ts frontend/app/lines/AllLinesTable.test.tsx frontend/components/IssueList.test.tsx frontend/components/LineStatusCard.test.tsx frontend/components/RepresentativeInfo.test.tsx frontend/lib/history.test.ts frontend/lib/severity.test.ts frontend/lib/stationIssues.test.ts frontend/lib/validity.test.ts
git commit -m "Add SampleAvailability to the frontend wire type and sampleStats.ts's shared helpers"
```

---

### Task 6: `AllLinesTable.tsx` — the primary target (dash Tooltip + mobile subtitle)

**Files:**
- Modify: `frontend/app/lines/AllLinesTable.tsx`
- Modify: `frontend/app/lines/AllLinesTable.test.tsx`

**Interfaces:**
- Consumes: `representativeStatus`, `sampleUnavailableReason`, `formatSampleSummary` (Task 5).
- **Depends on:** Task 5.

- [ ] **Step 1: Add `representativeStatus` alongside the existing `firstSampleStats`/`cancelledPercent` computation**

`rows` (currently `AllLinesTable.tsx:98-108`):

```typescript
  const rows = useMemo(
    () =>
      lines.map((line) => {
        const report = reportsById.get(line.id);
        const worst = report ? worstStatus(report) : undefined;
        const stats = firstSampleStats(report?.lineStatuses ?? []);
        const cancelledPct = cancelledPercent(stats);
        const representative = representativeStatus(report?.lineStatuses ?? []);
        return { line, worst, stats, cancelledPct, representative };
      }),
    [lines, reportsById],
  );
```

Import `representativeStatus` alongside the existing `firstSampleStats, cancelledPercent, formatSampleSummary` import (`AllLinesTable.tsx:20`).

- [ ] **Step 2: Mobile subtitle — swap the argument (currently `AllLinesTable.tsx:224`)**

```tsx
                <Text size="xs" c="dimmed" hiddenFrom="sm">
                  {formatSampleSummary(representative)}
                </Text>
```

- [ ] **Step 3: Desktop Avg Delay / Cancelled columns — wrap the dash in a `Tooltip` (currently `AllLinesTable.tsx:228-245`)**

`sampleUnavailableReason` requires a real `LineStatus`, not `undefined` — when a line has no report at all (`representative` is `undefined`, e.g. `swr` in the existing test fixture), there is no `LineStatus` to build a reason from. Guard the `Tooltip` itself on `representative` being defined, falling back to a plain (label-less) dash when it isn't:

```tsx
              <TableTd visibleFrom="sm">
                {stats ? (
                  <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                ) : representative ? (
                  <Tooltip label={sampleUnavailableReason(representative)}>
                    <Text size="sm" c="dimmed">
                      —
                    </Text>
                  </Tooltip>
                ) : (
                  <Text size="sm" c="dimmed">
                    —
                  </Text>
                )}
              </TableTd>
```

(Apply the identical pattern to the Cancelled column.) Import `Tooltip` from `@mantine/core` (add to the existing import list at `AllLinesTable.tsx:4-15`) and `sampleUnavailableReason` from `@/lib/sampleStats` (`AllLinesTable.tsx:20`).

- [ ] **Step 4: Add tests**

Extend `frontend/app/lines/AllLinesTable.test.tsx` with a test per `sampleAvailability` state, confirming the desktop dash's `Tooltip` label text differs correctly across `no-coverage` / `below-threshold` / TfL lines, and that the dash glyph itself (`"—"`) is unchanged in all three. Use `screen.getByRole('tooltip')` (Mantine renders the label into the DOM on hover/focus — check `EtaBadge.test.tsx` or `DataFreshnessInfo.test.tsx`, if either exists, for this codebase's established Mantine-`Tooltip`-testing idiom before writing a new one from scratch) or, if hover-triggered assertions prove awkward in this codebase's existing test setup, assert on the `Tooltip`'s `label` prop reaching the DOM via `aria-describedby`/title-role query — pick whichever this codebase's own prior art actually uses, don't invent a new pattern.

- [ ] **Step 5: Run the tests and the build**

Run: `npm test && npm run build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "Show why the AllLinesTable dash is a dash: sampleAvailability-driven Tooltip"
```

---

### Task 7: `LineStatusCard.tsx` + pinned-station row — always render (Decision 6)

**Files:**
- Modify: `frontend/components/LineStatusCard.tsx`
- Modify: `frontend/components/LineStatusCard.test.tsx`
- Modify: `frontend/app/page.tsx`

**Interfaces:**
- Consumes: `representativeStatus`, `formatSampleSummary` (Task 5).
- **Depends on:** Task 5.

Adopts 08-31's own recommended "stop omitting the line entirely" behavior change on both of these surfaces — converging presence, not just wording, with the other four call sites (per Decision 6).

- [ ] **Step 1: `LineStatusCard.tsx`**

Current (`LineStatusCard.tsx:8,13,47-51`):

```tsx
import { representativeStatus, formatSampleSummary } from '@/lib/sampleStats';
// ...
  const representative = representativeStatus(report.lineStatuses);
// ...
        <Text size="xs" c="dimmed">
          {formatSampleSummary(representative)}
        </Text>
```

(Drop the `{stats && (...)}` conditional wrapper — this block now always renders.)

- [ ] **Step 2: `app/page.tsx`'s pinned-station row**

Replace `sampleStatsAcrossReports` (currently `app/page.tsx:40-45`) with a `LineStatus`-returning analog:

```typescript
/** The first status carrying real stats across every affected line's
 * report, if any does, else the first status overall — mirrors
 * `representativeStatus`'s own fallback, extended across a station's
 * several affected lines the same way `sampleStatsAcrossReports` extended
 * `firstSampleStats`. */
function representativeStatusAcrossReports(reports: LineStatusReport[]): LineStatus | undefined {
  const withStats = reports.map((r) => representativeStatus(r.lineStatuses)).find((s) => s?.sampleStats);
  return withStats ?? reports[0]?.lineStatuses[0];
}
```

Import `representativeStatus` and the `LineStatus` type alongside the existing `firstSampleStats, formatSampleSummary` import (`app/page.tsx:17`) and `LineStatusReport` type import (`app/page.tsx:19`).

Then the pinned-station row (`app/page.tsx:216-227`):

```tsx
              const representative = representativeStatusAcrossReports(reports);
              return (
                <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Card withBorder>
                    <Stack gap={4}>
                      <Group justify="space-between">
                        <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
                        <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                      </Group>
                      {representative && (
                        <Text size="xs" c="dimmed">
                          {formatSampleSummary(representative)}
                        </Text>
                      )}
                    </Stack>
                  </Card>
                </Link>
              );
```

Note: unlike `LineStatusCard.tsx` (which always has at least one status per report), a pinned station can in principle have zero affected reports/statuses at all (`reports[0]?.lineStatuses[0]` is `undefined`) — keep the `{representative && (...)}` guard here specifically for that empty-reports edge case, not as a "no stats" guard (the point of Decision 6 is that "no stats" alone must no longer suppress this block).

- [ ] **Step 3: Update `LineStatusCard.test.tsx`**

The existing "omits the sample stats line entirely when no status carries sample stats" test (`LineStatusCard.test.tsx:95-98`) now asserts the opposite — rename and rewrite:

```typescript
  it('renders the reason instead of omitting the block when no status carries sample stats', () => {
    renderWithMantine(<LineStatusCard report={report} />);
    expect(screen.getByText('No live departure data received for this line yet.')).toBeInTheDocument();
  });
```

- [ ] **Step 4: Run the tests and the build**

Run: `npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/LineStatusCard.tsx frontend/components/LineStatusCard.test.tsx frontend/app/page.tsx
git commit -m "Always render the sample-summary line on LineStatusCard and the pinned-station row"
```

---

### Task 8: `app/stations/[crs]/page.tsx` — argument swap

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`

**Interfaces:**
- Consumes: `representativeStatus` (Task 5).
- **Depends on:** Task 5.

The smallest of the UI tasks: this surface already calls `formatSampleSummary` unconditionally (no truthiness guard, unlike `LineStatusCard`/the pinned-station row) — only the argument changes.

- [ ] **Step 1: Swap the argument**

Current (`app/stations/[crs]/page.tsx:92,98`):

```tsx
            {orderedReports.map((report) => {
              const representative = representativeStatus(report.lineStatuses);
              return (
                <Group key={report.id} justify="space-between" wrap="nowrap" gap="sm">
                  <Stack gap={0} style={{ minWidth: 0 }}>
                    <TextLink href={`/lines/${report.id}`}>{report.name}</TextLink>
                    <Text size="xs" c="dimmed">
                      {formatSampleSummary(representative)}
                    </Text>
                  </Stack>
                  <StatusBadge severity={worstStatus(report).statusSeverity} />
                </Group>
              );
            })}
```

Swap the `firstSampleStats` import for `representativeStatus` (`app/stations/[crs]/page.tsx:10`).

- [ ] **Step 2: Check for an existing test file and extend or note its absence**

If `frontend/app/stations/[crs]/page.test.tsx` (or equivalent) exists, extend it with a case asserting the three-way copy split now reachable here (no-coverage / below-threshold / TfL), mirroring Task 5's `sampleStats.test.ts` cases. If no such test file exists for this route today, this task does not create one from scratch — matching this repo's existing convention of not backfilling coverage for a route with none, and keeping this task's diff to the argument swap it's actually making.

- [ ] **Step 3: Run the tests and the build**

Run: `npm test && npm run build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/stations/[crs]/page.tsx
git commit -m "Use representativeStatus for the station-detail per-line sample summary"
```

---

## Not in this plan

Per the design spec's own "Explicitly out of scope" section — carried forward here, not silently dropped:

- **Case 3 in full — a schedule-derived "N services were expected" figure**, for the "nothing scheduled right now" sub-case. Confirmed by the spec (and re-confirmed nothing has changed since) that neither `crates/schedule-ingest` nor `crates/trust-consumer` has any per-line/per-window expected-service-count concept today — this is new ingestion-adjacent work needing its own design pass, not a field addition on top of what this plan builds.
- **Distinguishing 2a (genuinely quiet) from 2b (structurally under-sampled)** within `BelowThreshold`. Per `2026-08-31-sample-data-availability-design.md`'s Decision 3 (unrevisited by the sample-coverage spec), this needs a pattern over many cycles — `line_status_daily_stats`'s `sample_cycles`'s job, not a single live cycle's. `BelowThreshold` intentionally collapses both here.
- **`2026-08-31-sample-data-availability-design.md`'s own still-open Decision 2 proposal** — a global `stationSamples` timestamp on `/public/freshness`, answering staleness rather than presence. Unimplemented, unaffected, and not built by this plan.
- **Extending `line_status_daily_stats` or `TrendsResults.tsx` with this same `NoCoverage`/`BelowThreshold` split.** `sample_cycles` already answers a coarser "how much coverage did this line get today" question at the rollup level, and `SPARSE_DATA_FLOOR_CYCLES` already turns thin days into a rendered gap. Splitting `sample_cycles` into per-state sub-counts is a real, separate schema change to `line_status_daily_stats` (`crates/api/migrations/20260831090001_line_status_daily_stats.sql`) this plan does not make — left for a future pass if the Trends view is ever judged to need it. `crates/aggregator/src/main.rs`'s `lines_with_sample_coverage` is explicitly unchanged (Global Constraints, above).
- **Any new visual language beyond a `Tooltip`** for the `AllLinesTable` dash (a distinct icon, color, or glyph per state) — considered in the spec and not proposed without a design pass to back a new visual convention.
- **Any change to `min_sample_size`'s default, or to severity classification/escalation logic** — this plan is entirely about what accompanies an unchanged severity outcome.
- **The DLR pilot's own rollout/correctness, or turning `dlr_pilot_enabled` on** — Task 3 only designs around the pilot's existing states, unchanged from how `2026-08-22-tfl-service-metrics-v2-design.md` and `2026-08-31-sample-data-availability-design.md` both already scoped it.
