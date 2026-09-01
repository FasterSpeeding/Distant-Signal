# Disruption Impact Type Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the `enricher` pipeline's existing primary LLM pass a new, nullable, per-period `impact_type` fact (`rail_replacement_bus` / `no_scheduled_service` / `diversion` / `null`), and wire it all the way through `combine::combine_periods` → the aggregator's private `ExtractionPeriod` mirror → a new `governing_impact_type` collapsing function → `common::Disruption` → `render.rs`'s wire shape → a shared `frontend/lib/impactType.ts` label lookup → a new `Badge` in both `DisruptionDetail.tsx` (expanded) and `IssueList.tsx`'s collapsed accordion row — so a passenger can tell "buses replace trains" apart from "no service at all" apart from a plain delay.

**Architecture:**

```
crates/enricher/src/llm.rs              ExtractionPeriod.impact_type: Option<String> [NEW]
  PRIMARY_PROMPT (+paragraph)             primary_schema() period item gains "impact_type"
  primary_schema() (+field)               (Task 1)
        │
        ▼
crates/enricher/src/combine.rs          combine_periods copies impact_type through
  combine_periods (+1 field in the         UNCHANGED (no adversarial pass touches it,
  Ok(ExtractionPeriod{...}) literal)       same as scope_description/date_range)
        │                                  (Task 2)
        ▼
crates/enricher/src/main.rs             extraction_model_version bump:
  "@periods-v1" -> "@periods-v2"          forces sweep.rs to re-extract every incident
        │                                  (Task 3)
        ▼
incidents.extracted_periods JSONB       NO migration -- schemaless column
        │
        ▼
crates/aggregator/src/aggregation.rs
  private ExtractionPeriod mirror         #[serde(default)] impact_type (backward-compat
  governing_impact_type() [NEW]            for pre-change rows -- Task 4)
  status_from_incident()                  governing_impact_type(): pure, unit-tested,
                                            reuses period_phase/now_within_window
                                            (Task 4)
        │                        ┌─────────────────────────┐
        ▼                        ▼                          ▼
crates/common/src/lib.rs   aggregation.rs's OTHER      crates/poller-tfl/src/schema.rs
  Disruption.impact_type    Disruption literal            map_status's Disruption
  [NEW] #[serde(default,    (infer_from_samples,           literal: impact_type: None
  skip_serializing_if)]     LDBWS-derived): impact_type:   (permanent -- TfL never
  (Task 4)                  None (permanent)               extracts this) (Task 4)
        │
        ▼
crates/api/src/render.rs::status_to_json   "disruption": { ..., "impactType": ... }
  (only when detail=true, same gate every other disruption field already has) (Task 5)
        │
        ▼
frontend/lib/types.ts   Disruption.impactType: string | null   [NEW, always-present]
        │
        ├──▶ frontend/lib/impactType.ts   [NEW] shared IMPACT_TYPE_LABELS lookup (Task 6)
        │
        ├──▶ DisruptionDetail.tsx   badge above the description, variant="light"
        │      (Task 7)              color="orange"
        │
        └──▶ IssueList.tsx          same badge, first in .issueRow__meta, before the
               (Task 8)               optional "N lines" badge and the data-quality badge
```

**Tech Stack:** Rust (`enricher`/`aggregator`/`common`/`api`/`poller-tfl` crates — no new crate, no new dependency, no new database migration); Next.js App Router + TypeScript + Mantine v9 (`Badge`, existing `variant`/`color` props — no new frontend dependency).

**Spec:** `docs/superpowers/specs/2026-09-01-disruption-impact-type-design.md` — read in full before starting; this plan does not restate its research or Decisions, only carries them into concrete tasks. Cross-references below to "Decision N" refer to that document. Its own upstream research doc, `docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md`, is the source of the two real example incident texts (`Example 1`, the Barrhead–Dumfries/Kilmarnock–Troon–Ayr notice, and `Example 2`, the Norwood Junction overnight notice) that Task 1's worked example and Task 1's two new eval fixtures are built from — see that doc's "Proposed minimal taxonomy" and "Prompt design (sketch)" sections for the exact quoted clauses.

**Status note — every citation below independently re-confirmed against this worktree's current source, not trusted blind from either spec doc:** `crates/enricher/src/llm.rs`: `ExtractionPeriod` struct at lines 54-80 (exact match to the design doc's citation), `PrimaryExtraction` at 88-92, `primary_schema()` at 185-222 (period item `properties`/`required` at 192-217, `required` array literally at line 216), `PRIMARY_PROMPT` at 224-274 (exact match), `ADVERSARIAL_PROMPT`/`SEVERITY_ADVERSARIAL_PROMPT` at 302-312/345-354 (confirmed: neither ever asks about `impact_type`, matching Decision 2). `crates/enricher/src/combine.rs`: `combine_periods` at 112-152, its `Ok(ExtractionPeriod { ... })` literal at lines 141-149 (exact match), the `period()` test helper at 215-225. `crates/aggregator/src/aggregation.rs`: `status_from_incident` at 120-156 (its `Disruption` literal at 139-145 — note this file **already has** `SampleAvailability`/`sample_availability` wired through from a prior, already-landed plan on this branch, confirmed live at line 154; this plan's changes are additive alongside that, not a conflict), private `ExtractionPeriod` mirror at 337-348, `period_phase` at 374-403, `now_within_window` at 456-494, `parse_periods` at 412-418, `apply_extraction` at 557-659, `infer_from_samples`'s own, second `Disruption` literal (LDBWS-derived, unrelated to the incident-extraction pipeline) at 822-836, the `period_from_json` test helper at 2347-2349, `apply_extraction_two_active_periods_both_fire_most_severe_wins_both_annotations_kept` at 2445-2482 (the style precedent Task 4's new tests follow), `load_all_lines`/`incident`/`aggregate_with_defaults` test helpers at 965-1004. `crates/common/src/lib.rs`: `Disruption` struct at 309-321 (exact match; `source`'s `#[serde(default, skip_serializing_if = "Option::is_none")]` at line 319 is the precedent Task 4 follows for `impact_type`, not the design doc's Architecture diagram, which omits this attribute — see "New finding" below). `crates/api/src/render.rs`: `status_to_json` at 47-96, its `disruption` JSON block at 80-93, test module at 102-313 (`sample_report` at 108-124, `disruption_omitted_without_detail_flag`/`disruption_included_with_detail_flag` at 161-193, `overlay_status` at 257-267). `crates/api/src/routes/line_status.rs`: `a_status` at 403-413 — confirmed its `disruption` field is `None`, so **this file needs no change** (correcting the earlier sample-coverage plan's citation of this same helper for an unrelated field — that one did need a change, this one doesn't, since `impact_type` only exists inside a `Some(Disruption)`). `crates/poller-tfl/src/schema.rs`: `map_status` at 115-151, its `Disruption` literal at 136-144, `Disruption` already imported at line 29, its existing disruption-carries-through test's assertions at 280-287. `frontend/lib/types.ts`: `Disruption` interface at 12-18. `frontend/components/DisruptionDetail.tsx` (40 lines, read in full): `affectedStops` badge idiom at 14-20 (`variant="outline" color="gray"`), no `Badge` import change needed (already imported at line 3). `frontend/components/IssueList.tsx` (397 lines, read in full): `DATA_QUALITY_LABELS` at 37-43, imports at 17-23, the collapsed row's `.issueRow__meta` block at 351-370 (the "N lines" badge at 355-359, the data-quality badge at 367-369 — both confirmed at the exact lines the design doc cites), the expanded panel's `status.disruption` check at 383-389. `frontend/lib/severity.ts`: `GROUP_COLOR` at 34-40, confirmed palette is exactly `green`/`gray`/`blue`/`yellow`/`red` — `orange` unused, confirming the design's badge-color rationale.

**New finding this plan's own verification pass surfaced, not called out by either spec doc:** `common::Disruption` is genuinely deserialized back from storage, not just constructed fresh every cycle. `crates/aggregator/src/queries.rs:261` writes `serde_json::to_value(&report.statuses)` (a `Vec<LineStatus>`, which nests `Option<Disruption>`) into `line_status_history.statuses JSONB`; `crates/api/src/data/queries.rs:637-661`'s `line_status_history_for_range` later reads that same column back via `serde_json::from_value::<Vec<common::LineStatus>>`, for the `/lines/[id]/history` page. This means a required (non-`#[serde(default)]`) `impact_type` field on `Disruption` would hard-fail deserialization of every `line_status_history` row written before this feature ships, the instant a build carrying the new struct reads one back — silently breaking the history page for old data, not a hypothetical risk. `Disruption.source` already carries exactly the right attribute for this (`#[serde(default, skip_serializing_if = "Option::is_none")]`, `lib.rs:319`) for the identical reason. Task 4 gives `impact_type` the same attribute — a real, deliberate addition beyond what the design doc's Architecture diagram shows (which lists `impact_type: Option<String> [NEW field]` with no serde attribute called out).

**Construction sites that break once each new required field lands** (same category of mechanical fallout the 2026-09-01 line-status-sample-coverage plan flagged for its own field addition — enumerated explicitly here rather than left to surface as an unplanned build/typecheck break mid-plan):

- **`ExtractionPeriod { ... }` Rust struct literals** (5 total, confirmed by `grep -rn "ExtractionPeriod {" crates/`): `crates/enricher/src/combine.rs:141` (production — `combine_periods`'s `Ok(ExtractionPeriod { ... })`, fixed in Task 2) and `combine.rs:216` (test-only — the `period()` helper, fixed in Task 2); `crates/enricher/src/llm.rs:758, 767, 808` (all test-only, inside `extract_adversarial_parses_a_period_aligned_response` and `extract_severity_adversarial_parses_a_period_aligned_response`, fixed in Task 1). Note `crates/aggregator/src/aggregation.rs:2347`'s `period_from_json` is **not** a literal-construction site — it deserializes via `serde_json::from_value`, so it needs no per-site fix (the aggregator's private `ExtractionPeriod` mirror gains `#[serde(default)]` instead, Task 4).
- **`Disruption { ... }` Rust struct literals** (5 total, confirmed by `grep -rn "Disruption {" crates/`): `crates/aggregator/src/aggregation.rs:139` (production, `status_from_incident` — gains the real `governing_impact_type(...)` call, Task 4) and `:826` (production, `infer_from_samples`'s LDBWS-derived disruption — gains a permanent `impact_type: None`, Task 4); `crates/poller-tfl/src/schema.rs:136` (production, `map_status` — gains a permanent `impact_type: None`, Task 4); `crates/api/src/render.rs:163, 177` (test-only, `disruption_omitted_without_detail_flag`/`disruption_included_with_detail_flag`, fixed in Task 5).
- **`Disruption` TypeScript object literals** (3 total, confirmed by `grep -rln "disruption:\s*{" frontend --include="*.test.tsx"` plus a direct read of `DisruptionDetail.test.tsx`): `frontend/components/DisruptionDetail.test.tsx:7-13` (the shared `sample` fixture, fixed in Task 6 alongside the type addition it's forced by) and `frontend/components/IssueList.test.tsx:250-256, 267-273` (two inline literals, fixed in Task 8). No non-test `.tsx` file constructs a `Disruption` object literal — every component only ever receives one as a prop.

## Global Constraints

- **No new database migration, anywhere.** `extracted_periods` (`crates/api/migrations/20260822090000_incident_extraction_periods.sql`) is schemaless `JSONB`, and `common::Disruption`/`LineStatus` are stored the same way inside `line_status`/`line_status_history.statuses JSONB` — a new Rust struct field just starts appearing inside the existing blob. Do not create a migration file in any task.
- **No new dependency, in either crate ecosystem, and no new LLM call.** Per Decision 2, `impact_type` is primary-pass-only — no fourth adversarial call, no new Cargo crate, no new npm package.
- **`ExtractionPeriod::impact_type` gets NO `#[serde(default)]` on the enricher's own struct (`crates/enricher/src/llm.rs`); the aggregator's private mirror (`crates/aggregator/src/aggregation.rs`) gets it WITH `#[serde(default)]`.** This is a real, deliberate asymmetry (Decision 5 vs. Decision 6), not an inconsistency to "fix": the enricher's struct is only ever constructed fresh from a schema-validated LLM response (every sibling field — `scope_description`, `date_range`, `schedule_window` — already omits `#[serde(default)]` for the identical reason), while the aggregator's mirror must deserialize `extracted_periods` rows written by an `enricher` process that predates this change. Getting this backwards in either direction is a bug: `#[serde(default)]` on the enricher's struct would blur the "this field is emitted for every response" contract every sibling field already has; omitting it on the aggregator's mirror would hard-fail parsing every pre-change row, per `parse_periods`'s fail-safe (`aggregation.rs:412-418`) silently dropping **every** extracted fact for that incident, not just the new one.
- **`common::Disruption::impact_type` gets `#[serde(default, skip_serializing_if = "Option::is_none")]`, matching `source`'s existing attribute exactly** — see "New finding" above. This is load-bearing for `line_status_history` read-back compatibility, not decorative.
- **`governing_impact_type` never feeds `apply_extraction`'s severity floor/ceiling logic.** Per Decision 2/Verification posture, `impact_type` is purely display/annotation, exactly like `scope_description` today. No task in this plan touches `apply_extraction`'s escalation-candidate or demote-floor computation (`aggregation.rs:557-659`) — `governing_impact_type` is a separate, additive function called alongside it, not a change to it.
- **`extraction_model_version`'s literal suffix must bump** (`crates/enricher/src/main.rs:75`, currently `"@periods-v1"`) so `sweep.rs`'s `incidents_needing_extraction` (`crates/enricher/src/sweep.rs:28-33`) re-extracts every existing incident and backfills `impact_type` — otherwise only newly-created or newly-changed incidents ever get a value. This is Task 3, not optional cleanup.
- **`Severity::BusService`/`Severity::Diverted` (`crates/common/src/lib.rs`) and `severity_from_incident`'s keyword classifier (`aggregation.rs:266-288`) are explicitly untouched.** Both are pre-existing, structurally independent, whole-incident signals (Decision/research doc's own Finding 4) — this plan adds a new per-period signal alongside them, never modifies either. Do not add `severity_from_incident` to any task's file list.
- **No UI filter/facet chip for `impact_type`, anywhere.** Per the design's own "Explicitly out of scope," `IssueList.tsx`'s existing Severity/Source chip rows (`IssueList.tsx:248-289`) are not extended with a third row in this plan.
- **`frontend/lib/api.ts` needs no changes.** Every typed fetcher parses JSON straight into its report type with no field-by-field mapping — adding `impactType` to the `Disruption` interface (Task 6) is sufficient. Do not add `lib/api.ts` to any task's file list.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file, run via `cargo test -p <crate>`. Every test in this plan is pure-function/type-level or a mock-server HTTP fixture (no live database, no `#[ignore = "requires a live database..."]` needed) except the two new live-eval fixtures in Task 1, which follow the pre-existing `#[ignore = "requires network access..."]` convention already used by `live_eval_battery` (`crates/enricher/src/llm.rs:981-996`) and are never run in normal CI. Frontend: colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`, equivalently `npx vitest run`). Every frontend task's verification step runs both `npx vitest run` and `npx tsc --noEmit` (equivalently `npm run build`, which wraps `next build`'s own project-wide type-check) — the latter is required specifically because the "Construction sites" list above is exactly where an unplanned `tsc` break would otherwise surface mid-plan.
- **Parallelizable tasks:** Tasks 2 and 3 each depend only on Task 1 (both touch disjoint files from each other — `combine.rs` vs. `main.rs`) and can be dispatched to separate subagents in parallel once Task 1 lands. Task 5 depends only on Task 4 and touches disjoint files from Tasks 2/3. Tasks 7 and 8 each depend only on Task 6 and touch disjoint component files — same. Task 4 must not start before Task 1 lands (it needs the enricher's real JSON key name, `impact_type`, settled, even though the aggregator's mirror deserializes generic JSON and has no Rust-level compile dependency on `crates/enricher`).

---

### Task 1: Enricher schema/prompt — `ExtractionPeriod.impact_type`, `primary_schema()`, `PRIMARY_PROMPT`, eval fixtures

**Files:**
- Modify: `crates/enricher/src/llm.rs`

**Interfaces:**
- Produces: `ExtractionPeriod.impact_type: Option<String>` (new field, no `#[serde(default)]`). `primary_schema()`'s period item gains an `"impact_type"` property (`type: ["string", "null"]`, `enum: ["rail_replacement_bus", "no_scheduled_service", "diversion", null]`) and lists it in that item's `"required"` array.
- Consumed by: Task 2 (`combine::combine_periods` must copy the new field through), Task 4 (the aggregator's mirror struct gains a matching field).
- **Depends on:** nothing — foundational.

- [ ] **Step 1: Add `impact_type` to the `ExtractionPeriod` struct**

In `crates/enricher/src/llm.rs`, extend the struct (currently lines 54-80), inserting the new field after `apparent_severity` and before the two confidence fields (grouped with the other primary-pass-only, non-adversarially-checked fields, matching `scope_description`'s precedent):

```rust
pub struct ExtractionPeriod {
    pub scope_description: Option<String>,
    pub date_range: Option<DateRange>,
    pub schedule_window: Option<ScheduleWindow>,
    pub resolution_status: String,
    pub apparent_severity: String,
    /// `rail_replacement_bus` | `no_scheduled_service` | `diversion` | `null`.
    /// Primary-pass-only, like `scope_description` -- no adversarial check
    /// exists for this field (design doc Decision 2), so it is copied
    /// through `combine::combine_periods` unchanged. Deliberately has NO
    /// `#[serde(default)]` -- every sibling field in this struct is
    /// required in the real schema, and this one follows the same
    /// contract (see the aggregator's own mirror struct for the opposite,
    /// backward-compat-driven choice).
    pub impact_type: Option<String>,
    #[serde(default)]
    pub resolution_status_confidence: String,
    #[serde(default)]
    pub severity_confidence: String,
}
```

- [ ] **Step 2: Extend `primary_schema()`**

Add the property and list it as required (currently lines 185-222):

```rust
fn primary_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "category": { "type": "string" },
            "periods": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "scope_description": { "type": ["string", "null"] },
                        "date_range": {
                            "type": ["object", "null"],
                            "properties": {
                                "from_date": { "type": ["string", "null"] },
                                "to_date": { "type": ["string", "null"] }
                            },
                            "required": ["from_date", "to_date"]
                        },
                        "schedule_window": {
                            "type": ["object", "null"],
                            "properties": {
                                "days_of_week": { "type": "array", "items": { "type": "integer", "minimum": 1, "maximum": 7 } },
                                "start_time": { "type": "string" },
                                "end_time": { "type": "string" }
                            },
                            "required": ["days_of_week", "start_time", "end_time"]
                        },
                        "resolution_status": { "type": "string", "enum": ["ongoing", "residual", "resolved"] },
                        "apparent_severity": { "type": "string", "enum": ["normal", "moderate_disruption", "severe_disruption", "blocked_or_suspended"] },
                        "impact_type": {
                            "type": ["string", "null"],
                            "enum": ["rail_replacement_bus", "no_scheduled_service", "diversion", null]
                        }
                    },
                    "required": ["scope_description", "date_range", "schedule_window", "resolution_status", "apparent_severity", "impact_type"]
                }
            }
        },
        "required": ["category", "periods"]
    })
}
```

- [ ] **Step 3: Extend `PRIMARY_PROMPT`**

Append a new paragraph and worked example to the `PRIMARY_PROMPT` constant (currently lines 224-274). Replace the whole constant with the following — every sentence through `"...just because the text is matter-of-fact."` is byte-identical to what is already there today (do not reword or reflow any existing line); only the final paragraph, starting at `` `impact_type` is `rail_replacement_bus` ``, is new:

```rust
const PRIMARY_PROMPT: &str = "You extract structured facts from UK National Rail Knowledgebase incident \
    text. Read the summary and description exactly as given -- do not speculate beyond what the text \
    states. The text describes one incident that may cover one or MORE distinct periods; segment it into \
    the `periods` array. Only split into more than one period where the text itself demarcates a distinct \
    date range and/or a distinct scope/impact -- if the entire text describes one continuous fact with no \
    clearly distinct sub-periods, return a single-element `periods` array with `date_range: null`. Err \
    toward fewer periods when in doubt: do NOT split for stylistic variation, repeated wording, or several \
    stations/lines listed under one shared date range -- that is still one period. `periods` must always \
    contain at least one element. \
    For each period: `scope_description` is short, display-only text distinguishing what's different about \
    that period from the incident's other periods (e.g. \"platform 2 closed, calls at platform 1\"), or \
    null if there's only one period. `resolution_status` is `resolved` only if the text explicitly says the \
    disruption/root cause has ended; `residual` if it says the cause is fixed but knock-on effects continue; \
    `ongoing` otherwise, including whenever the text doesn't clearly say either way -- judge this \
    per-period, since different periods of the same incident can genuinely have different resolution \
    states. `apparent_severity` is your own read of how severe that period's disruption sounds, independent \
    of any specific keywords: `blocked_or_suspended` if any line, route, or station is described as blocked, \
    suspended, or closed to trains; `severe_disruption` if the text describes major/widespread delays, \
    cancellations, or long journey-time increases without an outright blockage; `moderate_disruption` for a \
    noticeable but contained impact; `normal` for routine minor delay language with no sign of broader \
    impact. \
    `date_range` MUST be populated whenever the text states an explicit date, even approximately -- never \
    leave it null and describe the dates only in `scope_description` instead; `scope_description` is for \
    what's DIFFERENT about the period (platform, direction, route), not a place to restate dates you should \
    have structured. `date_range` is null ONLY when the text truly states no date at all for that period. \
    When present, `from_date`/`to_date` must each be a fully-resolved ISO-8601 UTC timestamp string (or null \
    on either side, meaning no stated start/end respectively). An ETA (\"normal service expected to resume \
    from 18:00\") is expressed as a period whose `date_range.to_date` is that time, with `from_date: null` \
    -- do not add a separate field for it. Apply these conventions when resolving a stated date into that \
    timestamp: (1) Year inference -- the text is given together with a reference date this incident was \
    first reported around; if a date has no stated year, resolve it to whichever occurrence of that \
    month/day falls closest to the reference date -- do NOT invent an unrelated year. (2) Inclusivity -- a \
    stated end day (e.g. \"to Sunday 26 July\") means *through* that day, so its resolved `to_date` must be \
    the *following* day's 00:00 in Europe/London local time, converted to UTC -- not that day's own 00:00. \
    (3) Timezone -- a bare date with no stated time-of-day is a Europe/London calendar-day boundary; convert \
    it to UTC accounting for GMT/BST as appropriate for that date. `schedule_window` is null unless the text \
    states a weekly time-of-day restriction narrower than the period's own date range. \
    When in doubt about `resolution_status`, choose `ongoing` -- never guess `resolved` or `residual` from \
    tone, length, or the absence of further detail; only an explicit statement that the disruption or its \
    root cause has ended justifies anything other than `ongoing`. \
    Worked example, reference date 2026-03-01T00:00:00Z: input \"Monday 6 April to Friday 15 May: Platform 3 \
    at Clapham Junction is closed, trains call at platform 4. Saturday 16 May to Sunday 14 June: Platform 5 \
    is closed, trains call at platform 6.\" segments into exactly two periods -- period 1: \
    `scope_description` \"platform 3 closed, calls at platform 4\", `date_range` `{\"from_date\": \
    \"2026-04-06T00:00:00Z\", \"to_date\": \"2026-05-16T00:00:00Z\"}` (2026 because that's the closest \
    occurrence to the March 2026 reference date; `to_date` is the day AFTER the stated 15 May end), \
    `schedule_window: null`, `resolution_status: \"ongoing\"` (no statement that it has ended); period 2: \
    `scope_description` \"platform 5 closed, calls at platform 6\", `date_range` `{\"from_date\": \
    \"2026-05-16T00:00:00Z\", \"to_date\": \"2026-06-15T00:00:00Z\"}`, `resolution_status: \"ongoing\"`. Note \
    both periods got real `date_range` values -- never null when dates are stated -- and neither was marked \
    `resolved` just because the text is matter-of-fact. \
    `impact_type` is `rail_replacement_bus` if that period states that buses (or another road vehicle) \
    replace, substitute for, or operate in place of trains for some or all of the affected journey -- \
    regardless of the exact phrasing used (\"buses replace trains,\" \"a replacement bus service,\" \"buses \
    will operate between X and Y\"). It is `no_scheduled_service` if that period states plainly that no \
    trains (and no replacement service) run at all -- do not use `rail_replacement_bus` for this; a \
    withdrawn service and a substitute service are different facts even when both are severe. It is \
    `diversion` if that period states trains are running via a different route than usual, without a bus \
    substitute. Use `null` for any period that does not state one of these three specific facts -- an \
    ordinary delay or cancellation notice with no stated substitute-service arrangement is `null`, not a \
    forced guess. \
    Worked example, reference date 2026-08-01T00:00:00Z: input \"Buses operate between Kilmarnock and Troon, \
    where passengers can connect with trains to / from Ayr, Saturdays 29 August to 12 September. No \
    scheduled services operate between Kilmarnock and Ayr / Stranraer on Sundays 30 August to 13 September.\" \
    segments into two periods, each with its own `schedule_window` restricting it to the stated day -- period \
    1: `scope_description` \"Saturday bus, Kilmarnock-Troon\", `schedule_window` restricted to Saturday, \
    `impact_type: \"rail_replacement_bus\"`; period 2: `scope_description` \"Sunday no service, \
    Kilmarnock-Ayr/Stranraer\", `schedule_window` restricted to Sunday, `impact_type: \"no_scheduled_service\"`. \
    Note these are two periods with two different `impact_type` values, not one merged period and not the \
    same tag applied to both -- a substitute bus service and a full withdrawal are different facts even on \
    immediately adjacent days of the same date range.";
```

- [ ] **Step 4: Fix this file's own `ExtractionPeriod` struct literals (test-only)**

Three sites, per the Construction sites list above — `extract_adversarial_parses_a_period_aligned_response` (currently lines 757-776) and `extract_severity_adversarial_parses_a_period_aligned_response` (currently lines 808-816). Add `impact_type: None,` to each of the three literals:

```rust
        let periods = vec![
            ExtractionPeriod {
                scope_description: None,
                date_range: None,
                schedule_window: None,
                resolution_status: "resolved".to_string(),
                apparent_severity: "normal".to_string(),
                impact_type: None,
                resolution_status_confidence: String::new(),
                severity_confidence: String::new(),
            },
            ExtractionPeriod {
                scope_description: Some("phase 2".to_string()),
                date_range: None,
                schedule_window: None,
                resolution_status: "resolved".to_string(),
                apparent_severity: "normal".to_string(),
                impact_type: None,
                resolution_status_confidence: String::new(),
                severity_confidence: String::new(),
            },
        ];
```

(And the same one-line addition to `extract_severity_adversarial_parses_a_period_aligned_response`'s single-element `periods` vec.)

- [ ] **Step 5: Add a unit test that `extract_primary` round-trips a non-null `impact_type`**

Mirror `extract_primary_parses_a_single_flat_period`'s shape (currently lines 514-554):

```rust
    #[tokio::test]
    async fn extract_primary_parses_a_non_null_impact_type() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({
                            "category": "engineering_works",
                            "periods": [{
                                "scope_description": null,
                                "date_range": null,
                                "schedule_window": null,
                                "resolution_status": "ongoing",
                                "apparent_severity": "severe_disruption",
                                "impact_type": "rail_replacement_bus"
                            }]
                        }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string(), DEFAULT_REQUEST_TIMEOUT);
        let result = client
            .extract_primary("Buses replace trains", "Engineering works", reference_date())
            .await
            .unwrap();

        assert_eq!(result.periods[0].impact_type.as_deref(), Some("rail_replacement_bus"));
    }

    #[tokio::test]
    async fn extract_primary_parses_a_null_impact_type() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({
                            "category": "signal_failure",
                            "periods": [{
                                "scope_description": null,
                                "date_range": null,
                                "schedule_window": null,
                                "resolution_status": "ongoing",
                                "apparent_severity": "normal",
                                "impact_type": null
                            }]
                        }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string(), DEFAULT_REQUEST_TIMEOUT);
        let result = client.extract_primary("Signal failure", "Delays", reference_date()).await.unwrap();

        assert_eq!(result.periods[0].impact_type, None);
    }
```

Note: every *pre-existing* mock JSON fixture in this file's other tests (`extract_primary_parses_a_single_flat_period`, `..._multiple_periods_with_nested_schedule_windows`, `..._rejects_periods_beyond_the_soft_cap`, `..._accepts_periods_exactly_at_the_soft_cap`, `..._threads_the_reference_date_into_user_content`) needs **no change** — per Decision 5, these mock server bodies bypass `primary_schema()`'s `"required"` validation entirely (they hand-write the response JSON directly), and `serde_json::from_str::<PrimaryExtraction>` only cares about the Rust struct's own `#[serde(default)]` attributes. Since `impact_type` has none, a response body that omits the key entirely would in fact fail to deserialize for those tests — but since none of them assert on `impact_type`, and this task does not touch their JSON bodies, they are unaffected by this addition either way; only the two new tests above exercise the field.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p enricher --lib llm::`
Expected: PASS.

- [ ] **Step 7: Add the two new live-eval fixtures**

Add alongside `WANDSWORTH_TOWN_SUMMARY`/`FLAT_ETA_SUMMARY`/`TRAP_SUMMARY` (currently lines 921-943), built from the two real example texts `docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md`'s "Proposed minimal taxonomy" table quotes:

```rust
    // Example 1 from docs/superpowers/specs/2026-09-01-disruption-type-extraction-research.md:
    // a Saturday rail-replacement-bus leg immediately adjacent to a Sunday
    // no-scheduled-service leg within the same overarching date range --
    // the case design doc Decision 4's governing_impact_type collapsing
    // rule (schedule-window disambiguation) is built to handle.
    const IMPACT_BUS_NOSERVICE_SUMMARY: &str = "Buses replace trains between Kilmarnock and Ayr";
    const IMPACT_BUS_NOSERVICE_DESCRIPTION: &str = "From Saturday 29 August to Sunday 13 September, \
        engineering work is taking place between Kilmarnock and Ayr. Buses operate between Kilmarnock and \
        Troon, where passengers can connect with trains to / from Ayr, on Saturdays. No scheduled services \
        operate between Kilmarnock and Ayr / Stranraer on Sundays.";

    // Example 2: a rail-replacement-bus paragraph and a separately-worded
    // diversion clause with no date/scope boundary of its own -- the
    // segmentation ambiguity design doc's Open questions/risks item 1
    // (and the research doc's Open question 1) name as unresolved.
    const IMPACT_DIVERSION_SUMMARY: &str = "Rail replacement buses and diversions between London Bridge and Croydon";
    const IMPACT_DIVERSION_DESCRIPTION: &str = "Monday to Thursday nights, buses will replace trains between \
        London Bridge and East / West Croydon while overnight engineering work takes place. Some trains will \
        be diverted via an alternative route.";
```

Add both as new named cases in `live_eval_battery` (currently lines 981-996), following the existing `"multi"`/`"flat"`/`"trap"` pattern:

```rust
    #[tokio::test]
    #[ignore = "requires network access to a real LLM_BASE_URL; run explicitly, see comment above"]
    async fn live_eval_battery() {
        let client = live_client_from_env();
        let repeats: u32 = std::env::var("LIVE_EVAL_REPEATS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);

        for attempt in 1..=repeats {
            run_battery_attempt(&client, "multi", attempt, WANDSWORTH_TOWN_SUMMARY, WANDSWORTH_TOWN_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "flat", attempt, FLAT_ETA_SUMMARY, FLAT_ETA_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "trap", attempt, TRAP_SUMMARY, TRAP_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "impact_bus_noservice", attempt, IMPACT_BUS_NOSERVICE_SUMMARY, IMPACT_BUS_NOSERVICE_DESCRIPTION).await;
        }
        for attempt in 1..=repeats.min(2) {
            run_battery_attempt(&client, "impact_diversion", attempt, IMPACT_DIVERSION_SUMMARY, IMPACT_DIVERSION_DESCRIPTION).await;
        }
    }
```

Also extend `run_battery_attempt`'s per-period log line (currently lines 954-979) to print `impact_type`, so a battery run actually surfaces the new field's output:

```rust
                for (i, p) in primary.periods.iter().enumerate() {
                    eprintln!(
                        "  BATTERY fixture={label} attempt={attempt} period[{i}] scope={:?} date_range={:?} \
                         schedule_window={:?} resolution_status={:?} apparent_severity={:?} impact_type={:?}",
                        p.scope_description, p.date_range, p.schedule_window, p.resolution_status, p.apparent_severity, p.impact_type
                    );
                }
```

These two fixtures are `#[ignore]`d exactly like the existing three — they require `LLM_BASE_URL`/`LLM_MODEL` env vars and are never run in normal CI. Per Decision 2/the design's Explicitly out of scope, this task does not pin exact prompt wording against a live model; it only adds the fixtures so a later, separate eval pass can be run against them.

- [ ] **Step 8: Commit**

```bash
git add crates/enricher/src/llm.rs
git commit -m "Add ExtractionPeriod.impact_type: primary_schema, PRIMARY_PROMPT, and two live-eval fixtures"
```

---

### Task 2: `combine::combine_periods` passthrough

**Files:**
- Modify: `crates/enricher/src/combine.rs`

**Interfaces:**
- Consumes: `ExtractionPeriod.impact_type` (Task 1).
- Produces: no new public interface — `combine_periods` continues returning `Vec<ExtractionPeriod>`, now with `impact_type` populated.
- **Depends on:** Task 1.

- [ ] **Step 1: Copy `impact_type` through unchanged in `combine_periods`**

Currently lines 141-149 — `impact_type` joins `scope_description`/`date_range`/`schedule_window` as a field cloned straight through, untouched by either adversarial pass (per Decision 2, matching the research doc's own confirmation that these three fields already flow through with zero adversarial interaction):

```rust
            Ok(ExtractionPeriod {
                scope_description: period.scope_description.clone(),
                date_range: period.date_range.clone(),
                schedule_window: period.schedule_window.clone(),
                resolution_status,
                apparent_severity,
                impact_type: period.impact_type.clone(),
                resolution_status_confidence,
                severity_confidence,
            })
```

- [ ] **Step 2: Fix the `period()` test helper (test-only construction site)**

Currently lines 215-225 — add an `impact_type` parameter so tests can exercise a non-null value, defaulting existing call sites to `None` via a small signature change:

```rust
    fn period(scope: Option<&str>, resolution_status: &str, apparent_severity: &str) -> ExtractionPeriod {
        period_with_impact(scope, resolution_status, apparent_severity, None)
    }

    fn period_with_impact(
        scope: Option<&str>,
        resolution_status: &str,
        apparent_severity: &str,
        impact_type: Option<&str>,
    ) -> ExtractionPeriod {
        ExtractionPeriod {
            scope_description: scope.map(str::to_string),
            date_range: None,
            schedule_window: None,
            resolution_status: resolution_status.to_string(),
            apparent_severity: apparent_severity.to_string(),
            impact_type: impact_type.map(str::to_string),
            resolution_status_confidence: String::new(),
            severity_confidence: String::new(),
        }
    }
```

Every existing call site (`period(None, "resolved", "normal")`, etc., across `combine_periods_combines_each_index_independently` and the length/alignment-mismatch tests) needs no change — `period()` keeps its original 3-argument signature via the new thin wrapper.

- [ ] **Step 3: Add a test asserting `impact_type` survives combination unchanged**

Alongside `combine_periods_combines_each_index_independently` (currently lines 235-256):

```rust
    #[test]
    fn combine_periods_copies_impact_type_through_unchanged() {
        let primary = vec![period_with_impact(None, "ongoing", "normal", Some("rail_replacement_bus"))];
        let resolution = vec![resolution_verdict(0, None, "ongoing")];
        let severity = vec![severity_verdict(0, None, "normal")];

        let result = combine_periods(&primary, &resolution, &severity).unwrap();

        assert_eq!(result[0].impact_type.as_deref(), Some("rail_replacement_bus"));
    }

    #[test]
    fn combine_periods_copies_a_null_impact_type_through_unchanged() {
        let primary = vec![period(None, "ongoing", "normal")]; // impact_type: None
        let resolution = vec![resolution_verdict(0, None, "ongoing")];
        let severity = vec![severity_verdict(0, None, "normal")];

        let result = combine_periods(&primary, &resolution, &severity).unwrap();

        assert_eq!(result[0].impact_type, None);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p enricher --lib combine::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/enricher/src/combine.rs
git commit -m "Copy ExtractionPeriod.impact_type through combine_periods unchanged"
```

---

### Task 3: `extraction_model_version` bump

**Files:**
- Modify: `crates/enricher/src/main.rs`

**Interfaces:**
- Produces: no new interface — changes the literal value `main.rs::model_version` resolves to at runtime.
- Consumed by: `crates/enricher/src/sweep.rs`'s `incidents_needing_extraction`, unchanged code, comparing against the new literal.
- **Depends on:** Task 1 (the prompt/schema genuinely changed, which is what justifies the bump — sequenced after so the bump isn't landed against a stale prompt).

- [ ] **Step 1: Bump the version suffix**

Currently `crates/enricher/src/main.rs:75`:

```rust
    let model_version = format!("{}@periods-v2", config.llm_model);
```

(was `"@periods-v1"`). Per the design doc's Rollout section, this forces `sweep.rs`'s `incidents_needing_extraction` (`crates/enricher/src/sweep.rs:28-33`, unchanged) to treat every existing `incidents` row as needing re-extraction on its next sweep pass, since no stored row's `extraction_model_version` can match the new literal — the same, already-used mechanism this codebase relies on for every prior prompt/schema change.

- [ ] **Step 2: Confirm the existing sweep test coverage exercises a version-suffix mismatch**

No new test is needed in `sweep.rs` itself — `incidents_needing_extraction_reextracts_on_a_bumped_model_version` (or equivalent; confirmed present via `cargo test -p enricher --lib sweep::` at the existing `"gpt-oss-20b@prompt-v2"` fixture, `sweep.rs:106`) already proves the general mechanism works for *any* literal-suffix change, including this one. Run the existing suite to confirm nothing regresses:

Run: `cargo test -p enricher --lib sweep::`
Expected: PASS, unchanged.

- [ ] **Step 3: Run the full enricher test suite**

Run: `cargo test -p enricher`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/enricher/src/main.rs
git commit -m "Bump extraction_model_version to @periods-v2 for the impact_type prompt/schema change"
```

---

### Task 4: `common::Disruption.impact_type` + `governing_impact_type()` + aggregator wiring

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/aggregator/src/aggregation.rs`
- Modify: `crates/poller-tfl/src/schema.rs`

**Interfaces:**
- Produces: `common::Disruption.impact_type: Option<String>` (new, `#[serde(default, skip_serializing_if = "Option::is_none")]`). `crates/aggregator/src/aggregation.rs::governing_impact_type(loaded: &LoadedIncident, now: DateTime<Utc>) -> Option<String>` (new, pure, private). The aggregator's private `ExtractionPeriod` mirror gains `impact_type: Option<String>` with `#[serde(default)]`.
- Consumed by: Task 5 (`render.rs` reads `disruption.impact_type`).
- **Depends on:** Task 1 (needs the real `impact_type` JSON key name the enricher now emits — not a compile-time Rust dependency, since the mirror deserializes generic JSON, but sequenced after for correctness).

- [ ] **Step 1: Add `impact_type` to `common::Disruption`**

In `crates/common/src/lib.rs`, extend the struct (currently lines 309-321):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disruption {
    /// `"RealTime"` | `"PlannedWork"` | `"Information"`
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub affected_stops: Vec<String>,
    #[serde(default)]
    pub affected_routes: Vec<AffectedRoute>,
    /// e.g. `"knowledgebase-incident-12345"`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// `rail_replacement_bus` | `no_scheduled_service` | `diversion` | `null`
    /// -- the currently-governing period's `impact_type`, per
    /// `aggregator::governing_impact_type`. `None` for a disruption with no
    /// currently-Active period stating one of these facts (the overwhelming
    /// majority), and unconditionally `None` for a TfL- or LDBWS-derived
    /// disruption, which never runs the enricher's extraction pipeline at
    /// all. `#[serde(default, ...)]` matches `source`'s own attribute --
    /// load-bearing for deserializing `line_status_history` rows written
    /// before this field existed (see `crates/api/src/data/queries.rs`'s
    /// `line_status_history_for_range`, which reads this struct back from
    /// stored JSONB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_type: Option<String>,
}
```

- [ ] **Step 2: Add a serde round-trip test for the new field's backward compatibility**

Add near `common::Disruption`'s existing usage in `lib.rs` (or alongside any existing `defaults_tests`-style module):

```rust
#[cfg(test)]
mod disruption_impact_type_tests {
    use super::*;

    #[test]
    fn a_pre_change_disruption_json_with_no_impact_type_key_deserializes_to_none() {
        let json = serde_json::json!({
            "category": "RealTime",
            "description": "Signal failure",
            "affected_stops": [],
            "affected_routes": [],
            "source": null
        });
        let disruption: Disruption = serde_json::from_value(json).expect("pre-change row must still parse");
        assert_eq!(disruption.impact_type, None);
    }

    #[test]
    fn impact_type_is_omitted_from_serialized_json_when_none() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure".to_string(),
            affected_stops: vec![],
            affected_routes: vec![],
            source: None,
            impact_type: None,
        };
        let value = serde_json::to_value(&disruption).unwrap();
        assert!(value.get("impact_type").is_none());
    }
}
```

- [ ] **Step 3: Fix `crates/aggregator/src/aggregation.rs`'s two `Disruption` literals (production, non-incident-extraction)**

`infer_from_samples`'s LDBWS-derived disruption (currently lines 822-836) — this path never runs the enricher's pipeline, so `impact_type` is permanently `None`, not a placeholder:

```rust
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
            impact_type: None, // LDBWS sampling never runs the enricher's extraction pipeline
        }),
        data_quality: DataQuality::LdbwsInferred,
        sample_stats: Some(stats),
        sample_availability: availability,
    }
```

- [ ] **Step 4: Add `impact_type` to the aggregator's private `ExtractionPeriod` mirror**

Currently lines 337-348 — add the field with `#[serde(default)]`, the opposite serde choice from Task 1's enricher-side struct, for the backward-compatibility reason its own doc comment already establishes for the two confidence fields:

```rust
#[derive(Deserialize)]
struct ExtractionPeriod {
    scope_description: Option<String>,
    date_range: Option<DateRange>,
    schedule_window: Option<ScheduleWindow>,
    resolution_status: String,
    apparent_severity: String,
    #[serde(default)]
    resolution_status_confidence: String,
    #[serde(default)]
    severity_confidence: String,
    /// `#[serde(default)]` is load-bearing here for the identical reason
    /// as the two confidence fields above: a row written by an `enricher`
    /// process older than this field must still parse, degrading to
    /// `None` rather than failing the whole `extracted_periods` parse.
    #[serde(default)]
    impact_type: Option<String>,
}
```

- [ ] **Step 5: Add `governing_impact_type`**

Place immediately after `apply_extraction` (currently ending at line 659), per Decision 4's five-step collapsing rule — reuses `parse_periods`/`period_phase`/`now_within_window` unchanged, so it can never disagree with `apply_extraction` about which periods count as currently live:

```rust
/// Picks the `impact_type` to attach to `common::Disruption` from among an
/// incident's currently-relevant periods -- see
/// docs/superpowers/specs/2026-09-01-disruption-impact-type-design.md
/// Decision 4. Filters to `Active` periods (matching `apply_extraction`'s
/// own definition of "currently relevant"), then to periods that actually
/// state an `impact_type`, then prefers one whose `schedule_window` is
/// either absent or currently matching `now` -- the same real-time
/// refinement `apply_extraction`'s own schedule-window demotion check
/// already applies, reused here as a filter rather than a demotion
/// trigger. Resolves the Saturday-bus/Sunday-no-service case: on a
/// Saturday only the bus period's window matches; on a Sunday only the
/// no-service period's does; on a weekday matching neither, this returns
/// `None`, correctly reflecting that neither stated fact currently
/// applies. Takes the FIRST (array/text order) remaining candidate --
/// deliberately not a severity-like ranking across the three values (see
/// Decision 4's "Alternative considered and rejected"); a real, named,
/// unresolved tie-break gap for two simultaneously-eligible periods with
/// no `schedule_window` to disambiguate them (design doc Open
/// questions/risks item 1).
fn governing_impact_type(loaded: &LoadedIncident, now: DateTime<Utc>) -> Option<String> {
    parse_periods(loaded)
        .into_iter()
        .filter(|period| period_phase(period, now) == PeriodPhase::Active)
        .filter(|period| period.impact_type.is_some())
        .find(|period| match &period.schedule_window {
            None => true,
            Some(window) => now_within_window(window, now),
        })
        .and_then(|period| period.impact_type)
}
```

- [ ] **Step 6: Wire `governing_impact_type` into `status_from_incident`'s `Disruption` literal**

Currently lines 139-145 — replace the missing field with the real computation:

```rust
    let disruption = Disruption {
        category: if incident.is_planned { "PlannedWork" } else { "RealTime" }.to_string(),
        description: if incident.description.is_empty() { incident.summary.clone() } else { incident.description.clone() },
        affected_stops: affected_stations,
        affected_routes,
        source: Some(format!("knowledgebase-incident-{}", incident.incident_id)),
        impact_type: governing_impact_type(loaded, now),
    };
```

- [ ] **Step 7: Fix `crates/poller-tfl/src/schema.rs`'s `Disruption` literal (production, permanent `None`)**

Currently lines 136-144 — TfL's own feed never runs the enricher's extraction pipeline, so this is a permanent value, not a placeholder awaiting later wiring:

```rust
        disruption: status.disruption.as_ref().map(|disruption| Disruption {
            category: disruption.category.clone().unwrap_or_default(),
            description: disruption.description.clone().unwrap_or_default(),
            affected_stops: vec![],
            affected_routes: vec![],
            source: Some(format!("tfl-line-status-{line_id}")),
            impact_type: None, // TfL never runs the enricher's extraction pipeline
        }),
```

Extend the existing disruption-carries-through test (its assertions currently at `schema.rs:280-287`) with one more line:

```rust
        assert_eq!(disruption.impact_type, None);
```

- [ ] **Step 8: Add `governing_impact_type`'s own dedicated unit tests**

Add alongside the `period_phase_*`/`apply_extraction_*` tests (near line 2482, following `apply_extraction_two_active_periods_both_fire_most_severe_wins_both_annotations_kept`'s established style of building `LoadedIncident` directly from inline JSON):

```rust
    #[test]
    fn governing_impact_type_returns_none_with_no_periods() {
        let loaded = LoadedIncident {
            message: incident("GIT1", "Signal failure", "Delays", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: None,
        };
        assert_eq!(governing_impact_type(&loaded, Utc::now()), None);
    }

    #[test]
    fn governing_impact_type_returns_none_when_the_active_periods_impact_type_is_null() {
        let periods = serde_json::json!([{
            "scope_description": null,
            "date_range": null,
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "normal",
            "resolution_status_confidence": "high",
            "severity_confidence": "",
            "impact_type": null
        }]);
        let loaded = LoadedIncident {
            message: incident("GIT2", "Signal failure", "Delays", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };
        assert_eq!(governing_impact_type(&loaded, Utc::now()), None);
    }

    #[test]
    fn governing_impact_type_returns_the_single_active_periods_value() {
        let periods = serde_json::json!([{
            "scope_description": null,
            "date_range": null,
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "severe_disruption",
            "resolution_status_confidence": "high",
            "severity_confidence": "high",
            "impact_type": "rail_replacement_bus"
        }]);
        let loaded = LoadedIncident {
            message: incident("GIT3", "Buses replace trains", "Engineering works", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };
        assert_eq!(governing_impact_type(&loaded, Utc::now()), Some("rail_replacement_bus".to_string()));
    }

    #[test]
    fn governing_impact_type_ignores_a_pre_change_period_missing_the_key_entirely() {
        // Proves the aggregator mirror's #[serde(default)] backward-compat.
        let periods = serde_json::json!([{
            "scope_description": null,
            "date_range": null,
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "normal",
            "resolution_status_confidence": "high",
            "severity_confidence": ""
        }]);
        let loaded = LoadedIncident {
            message: incident("GIT4", "Signal failure", "Delays", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };
        assert_eq!(governing_impact_type(&loaded, Utc::now()), None);
    }

    #[test]
    fn governing_impact_type_never_uses_an_elapsed_or_not_started_periods_value() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let periods = serde_json::json!([
            {
                "scope_description": "already over",
                "date_range": { "from_date": null, "to_date": (now - Duration::hours(1)).to_rfc3339() },
                "schedule_window": null,
                "resolution_status": "ongoing",
                "apparent_severity": "normal",
                "resolution_status_confidence": "high",
                "severity_confidence": "",
                "impact_type": "rail_replacement_bus"
            },
            {
                "scope_description": "not yet started",
                "date_range": { "from_date": (now + Duration::hours(1)).to_rfc3339(), "to_date": null },
                "schedule_window": null,
                "resolution_status": "ongoing",
                "apparent_severity": "normal",
                "resolution_status_confidence": "high",
                "severity_confidence": "",
                "impact_type": "diversion"
            }
        ]);
        let loaded = LoadedIncident {
            message: incident("GIT5", "Engineering works", "Various", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };
        assert_eq!(governing_impact_type(&loaded, now), None);
    }

    #[test]
    fn governing_impact_type_saturday_sunday_case_picks_the_period_whose_window_matches_now() {
        // The exact Example 1 shape: one shared date_range, two periods
        // with different schedule_windows and different impact_types.
        let periods = serde_json::json!([
            {
                "scope_description": "Saturday bus",
                "date_range": null,
                "schedule_window": { "days_of_week": [6], "start_time": "00:00", "end_time": "23:59" },
                "resolution_status": "ongoing",
                "apparent_severity": "moderate_disruption",
                "resolution_status_confidence": "high",
                "severity_confidence": "",
                "impact_type": "rail_replacement_bus"
            },
            {
                "scope_description": "Sunday no service",
                "date_range": null,
                "schedule_window": { "days_of_week": [7], "start_time": "00:00", "end_time": "23:59" },
                "resolution_status": "ongoing",
                "apparent_severity": "blocked_or_suspended",
                "resolution_status_confidence": "high",
                "severity_confidence": "",
                "impact_type": "no_scheduled_service"
            }
        ]);
        let loaded = LoadedIncident {
            message: incident("GIT6", "Buses replace trains", "Weekend engineering works", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };

        let saturday_noon: DateTime<Utc> = "2026-06-13T12:00:00Z".parse().unwrap(); // a Saturday
        assert_eq!(governing_impact_type(&loaded, saturday_noon), Some("rail_replacement_bus".to_string()));

        let sunday_noon: DateTime<Utc> = "2026-06-14T12:00:00Z".parse().unwrap(); // a Sunday
        assert_eq!(governing_impact_type(&loaded, sunday_noon), Some("no_scheduled_service".to_string()));

        let monday_noon: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap(); // neither window matches
        assert_eq!(governing_impact_type(&loaded, monday_noon), None);
    }
```

- [ ] **Step 9: Add the `status_from_incident` wiring test**

Confirms the value actually reaches the constructed `Disruption`, not just `governing_impact_type` in isolation. `status_from_incident` is private but reachable from the test module via `use super::*`; `Match`/`Evidence` (`crate::matcher`) have all-`pub` fields, so a `Match` can be built directly without going through `lines_affected_by`:

```rust
    #[test]
    fn status_from_incident_threads_governing_impact_type_into_the_disruption() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let periods = serde_json::json!([{
            "scope_description": null,
            "date_range": null,
            "schedule_window": null,
            "resolution_status": "ongoing",
            "apparent_severity": "normal",
            "resolution_status_confidence": "high",
            "severity_confidence": "",
            "impact_type": "rail_replacement_bus"
        }]);
        let loaded = LoadedIncident {
            message: incident("GIT7", "Engineering works", "Buses replace trains", &["SW"], &["AHT"]),
            first_seen_at: Utc::now(),
            extracted_periods: Some(periods),
        };
        let m = crate::matcher::Match {
            line: alton,
            scope: MatchScope::ExclusiveSegment,
            evidence: crate::matcher::Evidence {
                stations: vec!["AHT".to_string()],
                segments: vec![],
                operators: vec![],
                keywords: vec![],
            },
        };

        let status = status_from_incident(&m, &loaded, now);

        assert_eq!(status.disruption.unwrap().impact_type.as_deref(), Some("rail_replacement_bus"));
    }
```

- [ ] **Step 10: Run the tests**

Run: `cargo build --workspace && cargo test -p common -p aggregator -p poller-tfl`
Expected: PASS.

- [ ] **Step 11: Commit**

```bash
git add crates/common/src/lib.rs crates/aggregator/src/aggregation.rs crates/poller-tfl/src/schema.rs
git commit -m "Add common::Disruption.impact_type and the aggregator's governing_impact_type collapsing function"
```

---

### Task 5: API wire shape — `impactType` on `status_to_json`

**Files:**
- Modify: `crates/api/src/render.rs`

**Interfaces:**
- Produces: `disruption.impactType` key on the wire, present (string or `null`) whenever the `disruption` block itself is present (i.e. `detail=true` and `status.disruption.is_some()` — the same existing gate every other disruption field already has).
- Consumed by: the frontend wire contract Task 6 types against.
- **Depends on:** Task 4.
- No change needed in `crates/api/src/routes/line_status.rs` — its `a_status` test helper (lines 403-413) constructs `disruption: None`, so it is unaffected by this field (re-confirmed by direct read; corrects an assumption an earlier plan made about this same helper for an unrelated field).

- [ ] **Step 1: Extend `status_to_json`'s `disruption` block**

Currently lines 80-93:

```rust
    if detail
        && let Some(disruption) = &status.disruption
    {
        out["disruption"] = json!({
            "category": disruption.category,
            "description": disruption.description,
            "affectedStops": disruption.affected_stops,
            "affectedRoutes": disruption.affected_routes.iter().map(|r| json!({
                "from": r.from_crs,
                "to": r.to_crs,
            })).collect::<Vec<_>>(),
            "source": disruption.source,
            "impactType": disruption.impact_type,
        });
    }
```

(`json!()`'s handling of `Option<String>` already renders `None` as JSON `null` and `Some(s)` as the string, matching `source`'s existing behavior exactly — no special-casing needed.)

- [ ] **Step 2: Fix this file's own two `Disruption` test literals**

`disruption_omitted_without_detail_flag` (currently lines 161-173) and `disruption_included_with_detail_flag` (currently lines 175-193) both construct a `Disruption` literal — add `impact_type: None,` to the first, and extend the second to exercise a real value:

```rust
    #[test]
    fn disruption_omitted_without_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![],
            source: Some("knowledgebase-incident-1".to_string()),
            impact_type: None,
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }

    #[test]
    fn disruption_included_with_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![common::AffectedRoute { from_crs: "WAT".to_string(), to_crs: "WOK".to_string() }],
            source: Some("knowledgebase-incident-1".to_string()),
            impact_type: Some("rail_replacement_bus".to_string()),
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), true);
        let d = &json["lineStatuses"][0]["disruption"];
        assert_eq!(d["category"], "RealTime");
        assert_eq!(d["description"], "Signal failure at Woking");
        assert_eq!(d["affectedStops"][0], "WOK");
        assert_eq!(d["affectedRoutes"][0]["from"], "WAT");
        assert_eq!(d["affectedRoutes"][0]["to"], "WOK");
        assert_eq!(d["source"], "knowledgebase-incident-1");
        assert_eq!(d["impactType"], "rail_replacement_bus");
    }
```

- [ ] **Step 3: Add a dedicated null-case test**

```rust
    #[test]
    fn impact_type_renders_as_json_null_when_absent() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure".to_string(),
            affected_stops: vec![],
            affected_routes: vec![],
            source: None,
            impact_type: None,
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), true);
        assert!(json["lineStatuses"][0]["disruption"]["impactType"].is_null());
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p api render::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "Add impactType to the public disruption wire shape"
```

---

### Task 6: Frontend wire type + shared label lookup

**Files:**
- Modify: `frontend/lib/types.ts`
- Create: `frontend/lib/impactType.ts`
- Create: `frontend/lib/impactType.test.ts`
- Modify: `frontend/components/DisruptionDetail.test.tsx`

**Interfaces:**
- Produces: `Disruption.impactType: string | null` (new, always-present field, matching `source`'s exact shape — not `impactType?: string`). `IMPACT_TYPE_LABELS: Record<string, string>` and `impactTypeLabel(impactType: string | null | undefined): string | null`.
- Consumed by: Task 7 (`DisruptionDetail.tsx`), Task 8 (`IssueList.tsx`).
- **Depends on:** Task 5 (wire contract — the field name `impactType` and its always-present/nullable shape).

- [ ] **Step 1: Extend `Disruption` in `frontend/lib/types.ts`**

Currently lines 12-18:

```typescript
export interface Disruption {
  category: string;
  description: string;
  affectedStops: string[];
  affectedRoutes: AffectedRoute[];
  source: string | null;
  impactType: string | null;
}
```

- [ ] **Step 2: Create `frontend/lib/impactType.ts`**

Following this codebase's established pattern of small, focused `frontend/lib/*.ts` modules (`dateFormat.ts`, `severity.ts`, `validity.ts`) imported by both consuming components, rather than duplicating the label map inline in each (unlike `DATA_QUALITY_LABELS`, which is local to `IssueList.tsx` alone because only one component uses it — two components render this fact and must not drift on wording):

```typescript
/** Labels for `Disruption.impactType`'s three known values -- see
 * `common::Disruption` (`crates/common/src/lib.rs`) and
 * docs/superpowers/specs/2026-09-01-disruption-impact-type-design.md
 * Decision 7. Rendered by both `DisruptionDetail.tsx` (expanded panel) and
 * `IssueList.tsx` (collapsed row) via `impactTypeLabel`, so both surfaces
 * stay in sync on wording by construction. */
export const IMPACT_TYPE_LABELS: Record<string, string> = {
  rail_replacement_bus: 'Rail Replacement Bus',
  no_scheduled_service: 'No Scheduled Service',
  diversion: 'Diversion',
};

/** `null` for a `null`/absent `impactType` (the common case -- no specific
 * fact stated) AND for any unrecognized value (schema drift, a future
 * taxonomy addition this frontend hasn't shipped yet) -- both fail safe to
 * "render nothing" rather than a raw snake_case string, unlike
 * `severityColor`'s fallback-to-'gray' (which must always render
 * *something*, since every status has a severity). `impact_type` is
 * already optional/supplementary everywhere in this design, so silently
 * omitting an unrecognized value is safe. */
export function impactTypeLabel(impactType: string | null | undefined): string | null {
  if (!impactType) return null;
  return IMPACT_TYPE_LABELS[impactType] ?? null;
}
```

- [ ] **Step 3: Add `frontend/lib/impactType.test.ts`**

```typescript
import { describe, it, expect } from 'vitest';
import { impactTypeLabel, IMPACT_TYPE_LABELS } from './impactType';

describe('impactTypeLabel', () => {
  it('returns the correct label for each known value', () => {
    expect(impactTypeLabel('rail_replacement_bus')).toBe('Rail Replacement Bus');
    expect(impactTypeLabel('no_scheduled_service')).toBe('No Scheduled Service');
    expect(impactTypeLabel('diversion')).toBe('Diversion');
  });

  it('returns null for null', () => {
    expect(impactTypeLabel(null)).toBeNull();
  });

  it('returns null for undefined', () => {
    expect(impactTypeLabel(undefined)).toBeNull();
  });

  it('returns null for an unrecognized value, not the raw string', () => {
    expect(impactTypeLabel('some_future_taxonomy_value')).toBeNull();
  });

  it('exposes exactly the three known keys', () => {
    expect(Object.keys(IMPACT_TYPE_LABELS).sort()).toEqual(
      ['diversion', 'no_scheduled_service', 'rail_replacement_bus'].sort(),
    );
  });
});
```

- [ ] **Step 4: Fix `DisruptionDetail.test.tsx`'s `sample` fixture (construction site)**

Currently lines 7-13 — add the new required field (matching how the fixture already carries every other field literally):

```typescript
const sample: Disruption = {
  category: 'RealTime',
  description: 'Signal failure at Woking',
  affectedStops: ['WOK', 'WAT'],
  affectedRoutes: [{ from: 'WAT', to: 'WOK' }],
  source: 'knowledgebase-incident-123',
  impactType: null,
};
```

(This is the only construction-site fix `impactType`'s type addition forces in this file — Task 7 adds new assertions against this same fixture, spread with an override, rather than new top-level fixtures.)

- [ ] **Step 5: Run the tests**

Run: `npx vitest run lib/impactType.test.ts components/DisruptionDetail.test.tsx`
Expected: PASS.

Run: `npx tsc --noEmit`
Expected: still fails at this point — `IssueList.test.tsx`'s two `Disruption` literals (Task 8) haven't been fixed yet. This is expected and resolved by Task 8; do not treat it as a regression in this task.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/impactType.ts frontend/lib/impactType.test.ts frontend/components/DisruptionDetail.test.tsx
git commit -m "Add Disruption.impactType to the frontend wire type, plus a shared label lookup"
```

---

### Task 7: `DisruptionDetail.tsx` badge

**Files:**
- Modify: `frontend/components/DisruptionDetail.tsx`
- Modify: `frontend/components/DisruptionDetail.test.tsx`

**Interfaces:**
- Consumes: `Disruption.impactType` (Task 6), `impactTypeLabel` (Task 6).
- **Depends on:** Task 6.

- [ ] **Step 1: Add the badge above the description**

Currently the component (whole file, 40 lines) opens with a sanitized-description `div`. Insert a conditional `Badge` before it, using `variant="light" color="orange"` — deliberately not the `variant="outline" color="gray"` styling used for `affectedStops` just below, since that styling already means "neutral provenance/list metadata" on this page, and `orange` is unused anywhere in `frontend/lib/severity.ts`'s existing palette (`green`/`gray`/`blue`/`yellow`/`red`), so this badge cannot be mistaken for a severity reading or for provenance metadata:

```tsx
'use client';

import { Stack, Text, Badge, Group } from '@mantine/core';
import type { Disruption } from '@/lib/types';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { incidentIdFromSource } from '@/lib/incidents';
import { impactTypeLabel } from '@/lib/impactType';
import { TextLink } from './TextLink';

export function DisruptionDetail({ disruption }: { disruption: Disruption }) {
  const incidentId = incidentIdFromSource(disruption.source);
  const impactLabel = impactTypeLabel(disruption.impactType);
  return (
    <Stack gap="xs">
      {impactLabel && (
        <Badge variant="light" color="orange" w="fit-content">
          {impactLabel}
        </Badge>
      )}
      <div dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }} />
      {disruption.affectedStops.length > 0 && (
        <Group gap="xs">
          {disruption.affectedStops.map((crs) => (
            <Badge key={crs} variant="outline" color="gray">
              {crs}
            </Badge>
          ))}
        </Group>
      )}
      {disruption.affectedRoutes.map((route, i) => (
        <Text key={i} size="sm" c="dimmed">
          {route.from} → {route.to}
        </Text>
      ))}
      {disruption.source && (
        <Text size="xs" c="dimmed">
          Source: {disruption.source}
        </Text>
      )}
      {incidentId && (
        <TextLink href={`/incidents/${encodeURIComponent(incidentId)}`} underline="always">
          View full incident details
        </TextLink>
      )}
    </Stack>
  );
}
```

- [ ] **Step 2: Add tests**

Alongside the existing tests in `DisruptionDetail.test.tsx` (which already has `sample.impactType: null` from Task 6):

```typescript
  it('renders the badge with the correct label for a rail-replacement-bus impact type', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'rail_replacement_bus' }} />,
    );
    expect(screen.getByText('Rail Replacement Bus')).toBeInTheDocument();
  });

  it('renders the badge with the correct label for a no-scheduled-service impact type', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'no_scheduled_service' }} />,
    );
    expect(screen.getByText('No Scheduled Service')).toBeInTheDocument();
  });

  it('renders the badge with the correct label for a diversion impact type', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, impactType: 'diversion' }} />);
    expect(screen.getByText('Diversion')).toBeInTheDocument();
  });

  it('renders no badge when impactType is null', () => {
    renderWithMantine(<DisruptionDetail disruption={{ ...sample, impactType: null }} />);
    expect(screen.queryByText('Rail Replacement Bus')).not.toBeInTheDocument();
    expect(screen.queryByText('No Scheduled Service')).not.toBeInTheDocument();
    expect(screen.queryByText('Diversion')).not.toBeInTheDocument();
  });

  it('renders no badge for an unrecognized impactType value, not the raw string', () => {
    renderWithMantine(
      <DisruptionDetail disruption={{ ...sample, impactType: 'some_future_taxonomy_value' }} />,
    );
    expect(screen.queryByText('some_future_taxonomy_value')).not.toBeInTheDocument();
  });
```

- [ ] **Step 3: Run the tests**

Run: `npx vitest run components/DisruptionDetail.test.tsx`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx
git commit -m "Render an impactType badge above the disruption description"
```

---

### Task 8: `IssueList.tsx`'s collapsed-row badge

**Files:**
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`

**Interfaces:**
- Consumes: `Disruption.impactType` (Task 6), `impactTypeLabel` (Task 6).
- **Depends on:** Task 6.

- [ ] **Step 1: Fix this file's two `Disruption` construction sites (test-only)**

Currently `IssueList.test.tsx` lines 250-256 and 267-273 — add `impactType: null` to both existing literals:

```typescript
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
        impactType: null,
      },
```

```typescript
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: 'knowledgebase-incident-123',
        impactType: null,
      },
```

- [ ] **Step 2: Add the badge to the collapsed row**

Currently the `.issueRow__meta` block (lines 351-370). Add the new badge as the FIRST element in that group — before the optional "N lines" badge, before the always-present data-quality badge — shown only when `status.disruption?.impactType` resolves to a recognized label:

```tsx
import { StatusBadge } from './StatusBadge';
import { DisruptionDetail } from './DisruptionDetail';
import type { LineStatus } from '@/lib/types';
import type { IssueItem } from '@/lib/stationIssues';
import { bucketFor, governingPeriod, periodIsActive, type IssueBucket } from '@/lib/validity';
import { formatDate, formatDateTime } from '@/lib/dateFormat';
import { isGoodSeverity } from '@/lib/severity';
import { impactTypeLabel } from '@/lib/impactType';
```

```tsx
                <div className="issueRow__meta">
                  <Text size="xs" c="dimmed">
                    {formatValiditySummary(status, now)}
                  </Text>
                  {impactTypeLabel(status.disruption?.impactType) && (
                    <Badge variant="light" size="sm" color="orange">
                      {impactTypeLabel(status.disruption?.impactType)}
                    </Badge>
                  )}
                  {(linesByStatus.get(status) ?? []).length > 1 && (
                    <Badge variant="outline" size="sm" color="gray">
                      {linesByStatus.get(status)!.length} lines
                    </Badge>
                  )}
                  <Badge variant="outline" size="sm" color="gray">
                    {DATA_QUALITY_LABELS[status.dataQuality]}
                  </Badge>
                </div>
```

Same `orange`/`light` treatment and shared label lookup as `DisruptionDetail.tsx` (Task 7), at `size="sm"` to match the row's other badges, per Decision 7.

- [ ] **Step 3: Add tests**

Alongside the existing collapsed-row tests (near `renders the data-quality badge as neutral gray, not the brand colour`, currently line 360):

```typescript
  it('renders the impact-type badge on the collapsed row when set', () => {
    const withImpactType: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
        impactType: 'rail_replacement_bus',
      },
    };
    renderWithMantine(<IssueList items={toItems([withImpactType])} now={NOW} />);
    expect(screen.getByText('Rail Replacement Bus')).toBeInTheDocument();
  });

  it('renders no impact-type badge on the collapsed row for the common null case', () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
    expect(screen.queryByText('Rail Replacement Bus')).not.toBeInTheDocument();
    expect(screen.queryByText('No Scheduled Service')).not.toBeInTheDocument();
    expect(screen.queryByText('Diversion')).not.toBeInTheDocument();
  });

  it('places the impact-type badge before the data-quality badge in the meta group', () => {
    const withImpactType: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
        impactType: 'no_scheduled_service',
      },
    };
    renderWithMantine(<IssueList items={toItems([withImpactType])} now={NOW} />);
    const description = screen.getByText('Signal failure');
    const control = description.closest('button') as HTMLElement;
    const meta = control.querySelector('.issueRow__meta') as HTMLElement;
    const badgeTexts = Array.from(meta.querySelectorAll('.mantine-Badge-root')).map((el) => el.textContent);
    expect(badgeTexts.indexOf('No Scheduled Service')).toBeLessThan(badgeTexts.indexOf('Knowledgebase'));
  });
```

- [ ] **Step 4: Run the tests**

Run: `npx vitest run components/IssueList.test.tsx`
Expected: PASS — including every pre-existing test in this 600+-line file, none of whose assertions this addition should perturb (per the design's Testing section, since none of the pre-existing fixtures set `impactType` to a non-null value).

- [ ] **Step 5: Commit**

```bash
git add frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx
git commit -m "Render an impactType badge on IssueList's collapsed accordion row"
```

---

### Task 9: End-to-end verification

**Files:** none (verification only).

**Interfaces:** none.
**Depends on:** Tasks 1-8, all landed.

- [ ] **Step 1: Full Rust workspace build and test**

Run: `cargo build --workspace`
Expected: PASS, no warnings about unused `impact_type` fields or unreachable code.

Run: `cargo test --workspace`
Expected: PASS — every crate's suite, including `enricher`, `aggregator`, `common`, `api`, `poller-tfl`.

- [ ] **Step 2: Full frontend test and type-check**

From `frontend/`:

Run: `npx vitest run`
Expected: PASS — full suite, no regressions in any file not touched by this plan.

Run: `npx tsc --noEmit`
Expected: PASS — confirms every `Disruption`/`LineStatus` object literal across the whole frontend (not just the files this plan touched) still type-checks now that `impactType` is a required field.

Run: `npm run build` (equivalently, `next build` — this repo's own convention for catching a project-wide type-check gap `tsc --noEmit` alone might miss, per the sample-coverage plan's own precedent)
Expected: PASS.

- [ ] **Step 3: Spot-check the live-eval fixtures were added but not run**

Run: `cargo test -p enricher --lib llm:: -- --list | grep -i impact`
Expected: lists `llm::tests::extract_primary_parses_a_non_null_impact_type`, `llm::tests::extract_primary_parses_a_null_impact_type`, and confirms `live_eval_impact_bus_noservice`/`live_eval_impact_diversion`-style names are absent from a normal (non-`--ignored`) listing if named as standalone tests, or that `live_eval_battery` itself is marked `#[ignore]` — i.e. confirm nothing in this plan accidentally made a live-network test run in normal CI.

- [ ] **Step 4: No commit** — this task is verification-only. If any step fails, fix the regression in the task that introduced it and re-run this task's steps from Step 1.
