# Full-Coverage Metrics Scaffolding — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to work this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This plan reverses, by explicit repo-owner direction, the "defer"
> verdict recorded in
> `docs/superpowers/plans/2026-09-03-full-coverage-metrics-transition-plan.md`.**
> That plan's Task 1 (the `DataQuality::TrustInferred` doc comment) is
> already implemented and merged — do not repeat it, only reuse it. This
> plan implements everything that plan deferred: the design doc's
> Decisions 1–4 in full, as scaffolding for a producer
> (Option B's TRUST-vs-schedule consumer) that does not exist yet and is
> explicitly out of scope here. See this repo's own commit history
> (`01d9657`, `ba7312e`) for the original deferral reasoning, and the task
> brief that authorized reversing it.

**Spec:** `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the design doc") — read in full before touching anything below. Its
Decisions 1–4 are authoritative for shape; this plan only sequences and
lands them. Decision 5 is pure analysis, nothing to build.

**Architecture recap** (see the design doc's own diagram for the full
picture):

- `crates/common`: new `FullCoverageAvailability` enum (sibling to
  `SampleAvailability`, NOT a variant of it), two new additive fields on
  `LineStatus`, one new additive field on `LineDefinition`.
- `crates/aggregator`: a new post-`aggregate()` merge pass
  (`merge_full_coverage`, mirroring `poller-tfl`'s `merge_dlr_sample_stats`
  shape, not `compute_sample_availability`'s), gated per-line on the new
  TOML flag; a new sibling pair of rollup tables + migration + write path,
  fed alongside (not instead of) the existing sample rollup writes.
- `crates/api`: wire shape in `render.rs`; two new `normalize_for_diff`
  strip targets (both copies); two new read queries + two new routes,
  siblings of the existing daily/half-hourly stats routes.
- `frontend`: additive types; extended precedence in
  `sampleStats.ts`(`representativeStatus`, `sampleUnavailableReason`/
  `formatSampleSummary`), a new `coverageProvenanceNote` helper; the six
  catalogued call sites; a new Trends-tab section for the coverage rollup.

**Tech stack:** Rust (axum, sqlx runtime-checked queries), Next.js 16 App
Router + TypeScript, Vitest 2. Same conventions as every other plan in this
repo — see `docs/superpowers/plans/2026-09-03-per-station-stats-plan.md`
for the closest structural precedent (backend-then-frontend task
ordering, `#[ignore]`-gated DB tests).

---

## Non-goals

- **Building Option B itself** — its TRUST-vs-schedule matching logic, its
  Kafka consumer, its own escalation thresholds. Nothing in this plan
  produces real `full_coverage_stats` data; every new field/table/route
  this plan builds has zero real writers when this plan is done. That is
  expected, not a bug — see the task brief.
- **A real "per-line materialized signal" transport.** `aggregator`'s new
  `merge_full_coverage` takes a `full_coverage: &HashMap<String,
  SampleStats>` parameter that is always empty in production
  (`&HashMap::new()` at its only call site in `main.rs`) — this plan does
  not build a consumer, queue, or HTTP endpoint that would ever populate
  it. That is Option B's own future task.
- **Segment-level `LineStatus` granularity.** Per Decision 3, per-status
  attachment already handles this for surfaces that iterate the full
  statuses array (`IssueList.tsx`) with zero new plumbing — not rebuilt
  here.
- **A true cross-source mixed-state summary UI** (e.g. "this stretch:
  TRUST-confirmed, that stretch: still sampled"). Flagged by the design
  doc as a real, larger follow-up, not designed or built here.
- **Per-station full-coverage stats.** Scoped, like the design doc, to the
  line-level `LineStatus` surfaces only.
- **Deleting or deprecating `sample_stats`/`sample_availability`, ever.**
  Permanent, per Decision 3.
- **The `sampleUnavailableReason`/`formatSampleSummary` source-agnostic
  rename.** Flagged by both this design doc and the per-station-stats
  design doc as a real, separate, not-yet-scheduled cost. Not this plan's
  job — only the parameter/precedence widening happens here (already-in-
  place `SampleStatsCarrier` structural typing does the rest).
- **`min_sample_size`/`severity_overrides`/LDBWS threshold changes.** Not
  touched.
- **`/public/freshness` extensions for full-coverage pipeline health.**
  Real, plausible, not designed or built here.
- **Half-hourly coverage data in the frontend Trends UI.** The backend
  route/table is built (for symmetry with the existing daily/half-hourly
  pair and so Option B's consumer has both available immediately), but
  only the **daily** coverage series gets a frontend chart in this plan —
  see Task 8's judgment-call note. The design doc itself leaves the exact
  mixed-range chart UI "deliberately not decided" (Decision 4); adding a
  second, half-hourly, definitely-still-empty chart surface for a
  producer that doesn't exist is exactly the kind of exercise this
  plan's own brief says not to gold-plate.

## Global Constraints

- **Testing:** Rust — `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests). DB-backed
  tests added by this plan follow the repo's `#[ignore]` convention
  (`cargo test -p api -p aggregator -- --ignored --test-threads=1`) but
  **cannot be run in this sandbox** — no live Postgres is available here;
  say so plainly rather than skip writing them. Frontend — `npm test`,
  `npx tsc --noEmit`, `npm run build` from `frontend/`.
- **Commit granularity:** one commit per coherent piece (new type, new
  migration+queries+route, aggregator wiring, each frontend surface) —
  not one giant commit. Matches this repo's own stated convention.
- **Every new Rust type/field needs real unit tests**, matching the
  density of `crates/common/src/lib.rs`'s `compute_sample_stats_tests`/
  `sample_availability_tests`. **Every new frontend function/component
  needs Vitest tests**, matching this repo's existing `sampleStats.test.ts`/
  `TrendsResults.test.tsx` conventions.
- **File scope** (created or modified): `crates/common/src/lib.rs`;
  `crates/aggregator/src/{aggregation,queries,main}.rs`;
  `crates/aggregator/src/main.rs`'s test fixtures;
  `crates/api/src/render.rs`; `crates/api/src/data/queries.rs`;
  `crates/api/src/routes/line_status.rs`; `crates/api/migrations/*.sql`
  (new); `crates/notifier/src/{main,queries}.rs` (test fixtures only);
  `crates/poller-tfl/src/{schema,main}.rs` (test/construction fixtures
  only); `frontend/lib/{types,sampleStats,api}.ts`;
  `frontend/components/{AllLinesTable→app/lines/AllLinesTable,LineStatusCard,
  RepresentativeInfo,IssueList}.tsx`; `frontend/app/page.tsx`;
  `frontend/app/stations/[crs]/page.tsx` (verify only — should need no
  change, per Decision 1's table); `frontend/app/lines/[id]/history/*`
  (new `CoverageTrendsResults.tsx` + `page.tsx` wiring). Plus every
  touched file's colocated test file.

---

### Task 1: `common` — `FullCoverageAvailability`, `LineStatus` fields, `LineDefinition` flag

**Files:** `crates/common/src/lib.rs`

Independent starting point; every other task depends on this one's types
existing.

- [ ] **Step 1:** Add `FullCoverageAvailability` (sibling enum, not a
  `SampleAvailability` variant — see design doc Decision 1's sketch,
  adapted: use `not_enabled_default()` as the `#[serde(default = ...)]`
  shim, mirroring `SampleAvailability::no_coverage_default`'s existing
  precedent).
- [ ] **Step 2:** Add `full_coverage_stats: Option<SampleStats>` (`#[serde(default,
  skip_serializing_if = "Option::is_none")]`) and `full_coverage_availability:
  FullCoverageAvailability` (`#[serde(default = "FullCoverageAvailability::not_enabled_default")]`)
  to `LineStatus`.
- [ ] **Step 3:** Add `full_coverage_enabled: bool` (`#[serde(default)]`) to
  `LineDefinition`, next to `severity_overrides`/`exclusive_segments`.
  Update `CustomLine`'s `From` impl to set it `false` (a user-defined line
  is never a Decision 3 rollout candidate).
- [ ] **Step 4:** Tests, mirroring `sample_availability_tests`'s density:
  - Wire-tag shape for all three `FullCoverageAvailability` states
    (`not-enabled`, `pending`, `available` — `Available`'s payload NOT
    re-embedded a second time, mirroring the existing
    `sample_availability_available_case_does_not_duplicate_sample_stats_fields`
    test's assertion shape).
  - `not_enabled_default()` returns `NotEnabled`.
  - A `LineStatus` JSON blob from before this field existed (no
    `full_coverage_stats`/`full_coverage_availability` keys) still
    deserializes, with `full_coverage_availability` defaulting to
    `NotEnabled` and `full_coverage_stats` to `None` — the read-compat
    case `SampleAvailability::no_coverage_default`'s own doc comment
    documents for its sibling field, exercised here for real.
  - `CustomLine::into::<LineDefinition>()` sets `full_coverage_enabled ==
    false` (extend the existing `custom_line_tests` module).
- [ ] **Step 5:** `cargo fmt --all && cargo clippy --workspace
  --all-features` — this will show every downstream compile error from
  the new required `LineStatus` fields; that's expected, fixed in Task 2.
- [ ] **Step 6:** Commit (after Task 2 also compiles — see that task's own
  Step; these two land as one commit since `common`'s new required fields
  don't compile alone without every construction site updated).

### Task 2: Update every `LineStatus`/`LineDefinition` construction site

**Files:** every non-test and test construction site of a `LineStatus` or
`LineDefinition` literal across the workspace (found via `grep -rn
"LineStatus {" crates/ --include=*.rs`): `crates/aggregator/src/main.rs`,
`crates/aggregator/src/queries.rs` (x2, DB test fixtures),
`crates/aggregator/src/aggregation.rs` (`status_from_incident`,
`infer_from_samples`, `good_service` — all real, non-test), `crates/api/src/render.rs`
(x2, test fixtures), `crates/api/src/routes/line_status.rs` (test
fixture), `crates/notifier/src/main.rs` + `crates/notifier/src/queries.rs`
(test fixtures), `crates/poller-tfl/src/schema.rs` (`map_status`, real),
`crates/poller-tfl/src/main.rs` (x3, test fixtures).

Depends on Task 1.

- [ ] **Step 1:** Add `full_coverage_stats: None` and
  `full_coverage_availability: common::FullCoverageAvailability::NotEnabled`
  (or the crate-local unqualified form, matching each site's existing
  import style) to every site above. Every one of these is currently
  `NotEnabled`-correct: none of them represent a full-coverage-enabled
  line, since Task 1's new TOML flag defaults `false` everywhere until a
  future `lines/*.toml` opts in (out of scope for this plan — no line is
  enabled by this plan).
- [ ] **Step 2:** `cargo build --workspace` clean, `cargo fmt --all`,
  `cargo clippy --workspace --all-features` — zero warnings.
- [ ] **Step 3:** `cargo test --workspace` — every existing test (none of
  which reference the two new fields yet) passes unmodified. This is the
  regression check: adding two fields to a struct that already exists
  everywhere as a value, not a diff, must not change any existing
  behavior.
- [ ] **Step 4:** Commit Tasks 1+2 together:
  ```
  git add crates/common/src/lib.rs crates/aggregator/src/*.rs \
          crates/api/src/render.rs crates/api/src/routes/line_status.rs \
          crates/notifier/src/*.rs crates/poller-tfl/src/*.rs
  git commit -m "Add FullCoverageAvailability and LineStatus.full_coverage_{stats,availability} (Decision 1 types)"
  ```

### Task 3: Wire shape in `render.rs` + `normalize_for_diff` (both copies)

**Files:** `crates/api/src/render.rs`, `crates/aggregator/src/queries.rs`,
`crates/api/src/data/queries.rs`

Depends on Task 1/2.

- [ ] **Step 1:** In `status_to_json` (`render.rs`), add the
  `fullCoverageStats`/`fullCoverageAvailability` block from Decision 1's
  sketch, following the exact `sampleStats`/`sampleAvailability` pattern
  immediately above it (conditional key for the stats, unconditional for
  availability). Extract `full_coverage_availability_json` as its own
  `pub(crate) fn`, mirroring `sample_availability_json`'s existing split
  (not inlined into `status_to_json`, for the same testability reason the
  existing helpers were split out).
- [ ] **Step 2:** Tests in `render.rs`'s `mod tests`, mirroring the
  existing `sample_stats_*`/`sample_availability_*` tests one-for-one:
  `full_coverage_stats_included_when_present`,
  `full_coverage_stats_omitted_when_absent`,
  `full_coverage_availability_is_always_present`,
  `full_coverage_availability_pending_shape`,
  `full_coverage_availability_available_case_does_not_duplicate_stats_fields`.
  Update `sample_report`'s (and `overlay_status`'s) literal per Task 2 —
  already done there, just confirm.
- [ ] **Step 3:** `normalize_entry_for_diff` (`crates/aggregator/src/queries.rs`):
  add `obj.remove("full_coverage_stats"); obj.remove("full_coverage_availability");`
  alongside the existing `sample_stats`/`sample_availability` removal.
  Add a test mirroring `normalize_for_diff_ignores_sample_stats_changes`
  for the two new fields.
- [ ] **Step 4:** `normalize_for_diff` (`crates/api/src/data/queries.rs`):
  same two `obj.remove(...)` additions, same reasoning (this copy strips
  fields before `tfl_statuses_changed`'s diff — TfL lines don't populate
  `full_coverage_stats` today, but the strip must exist symmetrically for
  the day a future producer changes that, matching this file's own doc
  comment's stated rationale for `sample_stats`). Add a test.
- [ ] **Step 5:** `cargo fmt --all && cargo clippy --workspace
  --all-features && cargo test -p api -p aggregator`.
- [ ] **Step 6:** Commit:
  ```
  git add crates/api/src/render.rs crates/aggregator/src/queries.rs crates/api/src/data/queries.rs
  git commit -m "Wire full_coverage_stats/full_coverage_availability onto the wire and into normalize_for_diff"
  ```

### Task 4: Aggregator — per-line TOML flag wiring, `merge_full_coverage`, `escalate_from_coverage_stats`

**Files:** `crates/aggregator/src/aggregation.rs`, `crates/aggregator/src/main.rs`

Depends on Task 1/2. Independent of Task 3.

- [ ] **Step 1:** Add `escalate_from_coverage_stats(severity, stats,
  thresholds) -> (Severity, Option<String>)`, byte-for-byte mirroring
  `escalate_from_sample_stats`'s shape (reuses the same `classify()`),
  differing only in its reason-annotation prefix (`"full-coverage data
  shows: {reason}"` vs. `"live samples show: {reason}"`) and doc comment.
- [ ] **Step 2:** Add `merge_full_coverage_stats(report: &mut
  LineStatusReport, stats: &SampleStats, thresholds: &Defaults)` — a
  post-hoc merge over an already-built report's statuses, mirroring
  `poller-tfl::merge_dlr_sample_stats`'s shape (Decision 1's stated
  precedent), NOT `compute_sample_availability`'s per-station shape.
  **Judgment call** (design doc's sketches don't resolve this): "no
  active incident present" — the condition that gates setting
  `DataQuality::TrustInferred` outright rather than merely escalating —
  is approximated as "this status's current `data_quality` is already
  `LdbwsInferred`" (i.e., Layer 1 found no incident for this line this
  cycle, so Layer 2's `infer_from_samples`/`good_service` literal is what
  produced it). This mirrors the exact precedent
  `escalate_from_sample_stats` already established for the identical
  question at Layer 2. Document this reasoning in the function's doc
  comment, not just this plan.
  - When `full_coverage`'s own `classify()` result is non-`GoodService`
    AND the status's `data_quality == LdbwsInferred`: set `severity`,
    `reason`, and `data_quality = TrustInferred` outright (the
    full-coverage classification IS the line's determination).
  - Otherwise (an incident is present, or coverage data itself reads
    quiet): call `escalate_from_coverage_stats` instead — escalate-only,
    never touches `data_quality`.
  - Unconditionally, for every status: `full_coverage_stats = Some(stats.clone())`,
    `full_coverage_availability = Available(stats.clone())`.
- [ ] **Step 3:** Add `merge_full_coverage(reports: &mut
  HashMap<String, LineStatusReport>, lines: &HashMap<String,
  LineDefinition>, full_coverage: &HashMap<String, SampleStats>, defaults:
  &Defaults)`: for every line with `full_coverage_enabled`, look up
  `full_coverage.get(&line.id)` — `Some` merges via Step 2;
  `None` sets `full_coverage_availability = Pending` on every status
  (enabled, not yet resolved this cycle — Decision 3's "upgrading, not
  yet upgraded" state). Lines without the flag are untouched (already
  `NotEnabled` from construction). Full doc comment explaining this is a
  **separate post-`aggregate()` pass**, deliberately not a new parameter
  on `aggregate()` itself, specifically so this addition doesn't touch
  `aggregate()`'s existing signature or its many existing call
  sites/tests — and that `full_coverage` is always empty in production
  today (no consumer exists yet; wiring one is explicitly out of scope).
- [ ] **Step 4:** Wire the call site in `main.rs::run_cycle`, immediately
  after `aggregation::aggregate(...)`:
  ```rust
  let mut reports = aggregation::aggregate(&lines, &incidents, &samples, &registry, defaults);
  aggregation::merge_full_coverage(&mut reports, &lines, &HashMap::new(), defaults);
  ```
  With a comment on the `&HashMap::new()` explaining it's the integration
  point Option B's future consumer would populate.
- [ ] **Step 5:** Tests in `aggregation.rs`'s `mod tests`, mirroring the
  existing `escalate_from_sample_stats`/DLR-merge test density:
  - `escalate_from_coverage_stats` — mirrors every existing
    `escalate_from_sample_stats` test case (escalates on higher rank,
    no-op on equal/lower rank, `total == 0` no-op).
  - `merge_full_coverage_stats_sets_stats_and_availability_on_every_status`.
  - `merge_full_coverage_stats_sets_trust_inferred_when_no_incident_and_coverage_implies_disruption`
    — the core new-provenance-tag test, mirroring
    `DataQuality::TrustInferred`'s doc comment's own stated rule.
  - `merge_full_coverage_stats_escalates_without_changing_data_quality_when_an_incident_is_present`
    — the negative case, mirroring `escalate_from_sample_stats`'s own
    "preserves original provenance" precedent from Current relevant state.
  - `merge_full_coverage_only_touches_full_coverage_enabled_lines`.
  - `merge_full_coverage_marks_pending_when_enabled_but_no_signal_present_this_cycle`.
- [ ] **Step 6:** `cargo fmt --all && cargo clippy --workspace
  --all-features && cargo test -p aggregator`.
- [ ] **Step 7:** Commit:
  ```
  git add crates/aggregator/src/aggregation.rs crates/aggregator/src/main.rs
  git commit -m "Add full_coverage_enabled TOML flag wiring, merge_full_coverage, escalate_from_coverage_stats (Decision 3)"
  ```

### Task 5: Sibling rollup tables + migration + aggregator write path

**Files:** new migration `crates/api/migrations/YYYYMMDDHHMMSS_line_status_daily_coverage_stats.sql`,
`crates/aggregator/src/queries.rs`, `crates/aggregator/src/main.rs`

Depends on Task 4 (needs `full_coverage_stats` populated on statuses to
have something to write). Independent of Task 3.

- [ ] **Step 1:** Migration. Two tables, column-for-column identical to
  `line_status_daily_stats`/`line_status_half_hourly_stats` except
  `sample_cycles` → `resolved_windows` (design doc's own naming), per
  Decision 4's sketch: `line_status_daily_coverage_stats` (`PRIMARY KEY
  (line_id, day)`) and `line_status_half_hourly_coverage_stats`
  (`PRIMARY KEY (line_id, half_hour_start)`), each with its own index
  mirroring the existing tables'.
- [ ] **Step 2:** `record_daily_coverage_stats`/`record_half_hourly_coverage_stats`
  in `crates/aggregator/src/queries.rs` — same accumulate-upsert shape as
  `record_daily_stats`/`record_half_hourly_stats`, `resolved_windows`
  incrementing by 1 identically to `sample_cycles`. **Judgment call**: fed
  directly from `status.full_coverage_stats` (whatever a line's Layer-3
  merge produced this cycle), NOT a deduped "new distinct trains" value —
  `dedup::dedup_new_sample_stats`'s per-service dedup ledger is LDBWS/
  Darwin-`service_id`-specific and has no defined analog for a
  materialized full-coverage signal (the design doc's own sketch doesn't
  specify one either — Option B's future consumer would need to define
  what "new this cycle" even means for its own population, which is
  Option B's own design question, not this plan's). Document this
  explicitly in the function's doc comment.
- [ ] **Step 3:** `prune_daily_coverage_stats`/`prune_half_hourly_coverage_stats`,
  mirroring `prune_daily_stats`/`prune_half_hourly_stats` exactly.
  **Judgment call**: reuse the existing `daily_stats_retention_days`/
  `half_hourly_stats_retention_hours` config knobs rather than adding two
  new ones — same retention posture is reasonable for a sibling table
  with the same shape and no evidence yet (no real data) that it needs a
  different window. Document this choice in the function doc comment.
- [ ] **Step 4:** Wire into `main.rs::run_cycle`: a new
  `lines_with_full_coverage(reports)` selector (mirrors
  `lines_with_sample_coverage`'s exact shape/doc-comment reasoning, keyed
  on `full_coverage_stats` instead of `sample_stats`), then a write loop
  parallel to the existing dedup/`record_daily_stats` loop (own
  `WRITE_CHUNK_SIZE`-batched transactions, own metrics counters —
  `aggregator_coverage_stats_recorded_total`/`_pruned_total`, mirroring
  the existing metric names), and unconditional pruning calls alongside
  the existing ones.
- [ ] **Step 5:** Tests: pure-logic test for `lines_with_full_coverage`
  (mirrors the two existing `lines_with_sample_coverage` tests: counts a
  line with two concurrent statuses once, excludes a line with no
  `full_coverage_stats`). DB-backed `#[ignore]`-gated tests for
  `record_daily_coverage_stats`/`record_half_hourly_coverage_stats`/their
  prune functions, mirroring the existing
  `record_daily_stats_accumulates_deduped_contributions_across_a_day`/
  `record_daily_stats_a_new_day_starts_a_fresh_row`/etc. test set
  one-for-one, renamed for the coverage table. **Cannot be run in this
  sandbox** (no live Postgres) — write them, note as unverified.
- [ ] **Step 6:** `cargo fmt --all && cargo clippy --workspace
  --all-features && cargo test -p aggregator` (non-DB tests only, here).
- [ ] **Step 7:** Commit:
  ```
  git add crates/api/migrations/*_line_status_*coverage_stats.sql crates/aggregator/src/queries.rs crates/aggregator/src/main.rs
  git commit -m "Add line_status_{daily,half_hourly}_coverage_stats tables and their aggregator write path (Decision 4)"
  ```

### Task 6: `crates/api` — read queries + two new routes

**Files:** `crates/api/src/data/queries.rs`, `crates/api/src/routes/line_status.rs`

Depends on Task 5 (needs the tables to exist). Independent of Tasks 3/4 in
code terms, but ordered after them for a sane review diff.

- [ ] **Step 1:** `DailyCoverageStatsRow`/`HalfHourlyCoverageStatsRow` +
  `daily_coverage_stats_for_range`/`half_hourly_coverage_stats_for_range`
  in `crates/api/src/data/queries.rs`, mirroring
  `DailyStatsRow`/`daily_stats_for_range` exactly (`resolved_windows` in
  place of `sample_cycles`).
- [ ] **Step 2:** `daily_coverage_stats_to_json`/`half_hourly_coverage_stats_to_json`
  + `get_line_daily_coverage_stats`/`get_line_half_hourly_coverage_stats`
  handlers in `crates/api/src/routes/line_status.rs`, mirroring
  `daily_stats_to_json`/`get_line_daily_stats` exactly (`resolvedWindows`
  in the JSON, in place of `sampleCycles`).
- [ ] **Step 3:** Register two new routes in `router()`:
  `/Line/{id}/Stats/Coverage/{from}/to/{to}` and
  `/Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}` — path naming
  chosen to sort visually next to the existing `Stats`/`Stats/HalfHourly`
  pair and read unambiguously as "the Coverage variant of Stats", the
  design doc's own sketch route name kept verbatim for the daily one.
- [ ] **Step 4:** Tests, mirroring `line_status.rs`'s existing two-tier
  convention:
  - Pure: `daily_coverage_stats_to_json`/`half_hourly_coverage_stats_to_json`
    rate-derivation tests, mirroring the existing
    `daily_stats_to_json`/`half_hourly_stats_to_json` tests (zero-total
    guard, non-zero rate math).
  - `oneshot`-probe path-parsing tests for both new routes, mirroring the
    existing probe tests for `/Line/{id}/Stats/{from}/to/{to}`.
  - DB-backed `#[ignore]`-gated end-to-end tests seeding a
    `line_status_daily_coverage_stats`/`..._half_hourly_coverage_stats`
    fixture row and asserting the route's JSON shape, mirroring the
    existing daily/half-hourly route DB tests. **Cannot be run in this
    sandbox** — write them, note as unverified.
- [ ] **Step 5:** `cargo fmt --all && cargo clippy --workspace
  --all-features && cargo test -p api` (non-DB).
- [ ] **Step 6:** Commit:
  ```
  git add crates/api/src/data/queries.rs crates/api/src/routes/line_status.rs
  git commit -m "Add GET /Line/{id}/Stats/Coverage{,/HalfHourly}/{from}/to/{to} (Decision 4)"
  ```

### Task 7: Frontend — types, `sampleStats.ts` precedence, `coverageProvenanceNote`

**Files:** `frontend/lib/types.ts`, `frontend/lib/sampleStats.ts`, their
colocated `.test.ts` files

Depends on nothing backend-blocking for the type/precedence work itself
(the wire shape is fully specified by Task 3/6's now-landed sketches);
blocks every remaining frontend task.

- [ ] **Step 1:** `types.ts`: add `FullCoverageAvailability` type, add
  `fullCoverageStats?: SampleStats`/`fullCoverageAvailability:
  FullCoverageAvailability` to `LineStatus`. Add `LineDailyCoverageStats`/
  `LineHalfHourlyCoverageStats` (mirroring `LineDailyStats`/
  `LineHalfHourlyStats`, `resolvedWindows` in place of `sampleCycles`,
  with the new Decision-4 honesty-copy doc comment, not a copy-paste of
  the sample one).
- [ ] **Step 2:** `sampleStats.ts`:
  - Widen `SampleStatsCarrier` to include `fullCoverageStats?:
    SampleStats` (structurally compatible with `LineStatus` and
    `StationOperatorSampleStats` — the latter never carries this field,
    which is fine, same as it never carries `dataQuality`).
  - `sampleUnavailableReason`: prepend the `fullCoverageStats` check
    (Decision 2's sketch: `if (status.fullCoverageStats) return null;`
    before the existing `sampleStats` check).
  - `formatSampleSummary`: change `status.sampleStats!` to `(status.fullCoverageStats
    ?? status.sampleStats)!` — Decision 1's "prefer full coverage over
    sample" numeric-rendering rule, applied at the one place that
    actually reads the numbers.
  - `representativeStatus`: extend the precedence chain per Decision 3's
    sketch (`fullCoverageStats` → `sampleStats` → first).
  - New `coverageProvenanceNote(status: SampleStatsCarrier): string |
    null` per Decision 2's sketch.
  - New `pendingCoverageNote(status: LineStatus): string | null` — the
    "still resolving" fourth copy state from Decision 2's rendered-copy
    table (`fullCoverageAvailability.state === 'pending'` →
    `'Full train-movement data is being resolved for this line — showing
    the live sample in the meantime.'`; needs the full `LineStatus` type,
    not the narrower carrier, since `StationOperatorSampleStats` has no
    `fullCoverageAvailability` field at all).
- [ ] **Step 3:** Tests in `sampleStats.test.ts`:
  - `sampleUnavailableReason`/`formatSampleSummary` return real numbers
    (not a hedge string) when only `fullCoverageStats` is present.
  - `formatSampleSummary` prefers `fullCoverageStats` over `sampleStats`
    when both are present on the same status.
  - `representativeStatus` prefers a status with `fullCoverageStats` over
    one with only `sampleStats`, over the first status overall — three
    cases.
  - `coverageProvenanceNote` returns the confident sentence when
    `fullCoverageStats` is present, `null` otherwise.
  - `pendingCoverageNote` returns the "still resolving" sentence only
    when `fullCoverageAvailability.state === 'pending'`, `null` for every
    other state.
- [ ] **Step 4:** `npx tsc --noEmit && npm test` (from `frontend/`).
- [ ] **Step 5:** Commit:
  ```
  git add frontend/lib/types.ts frontend/lib/sampleStats.ts frontend/lib/sampleStats.test.ts
  git commit -m "Extend sampleStats.ts precedence for fullCoverageStats, add coverageProvenanceNote/pendingCoverageNote (Decisions 1-2)"
  ```

### Task 8: Frontend — the six catalogued call sites + IssueList badge copy

**Files:** `frontend/app/lines/AllLinesTable.tsx`,
`frontend/components/LineStatusCard.tsx`,
`frontend/components/RepresentativeInfo.tsx`,
`frontend/components/IssueList.tsx`, `frontend/app/page.tsx`
(`representativeStatusAcrossReports`, the pinned-dashboard-row case), plus
each file's `.test.tsx`

Depends on Task 7.

- [ ] **Step 1: `AllLinesTable.tsx`.** Replace the `stats = firstSampleStats(...)`
  line with `stats = representative?.fullCoverageStats ??
  representative?.sampleStats` (computed from the already-updated
  `representative`, not a separate scan — this is what makes "prefer full
  coverage on the numeric columns" and "prefer full coverage for the
  representative status" the same underlying choice, per Decision 1's
  table). `formatSampleSummary`/`sampleUnavailableReason` call sites are
  unchanged (already routed through the extended `representative`).
- [ ] **Step 2: `LineStatusCard.tsx`.** No logic change needed —
  `representativeStatus`/`formatSampleSummary` already extended in Task
  7. Confirm via its existing test suite passing unmodified plus one new
  test asserting a `fullCoverageStats`-only status renders real numbers,
  not a hedge.
- [ ] **Step 3: `RepresentativeInfo.tsx`.** Extend the guard: `const
  withStats = statuses.find((status) => status.fullCoverageStats ??
  status.sampleStats)`-shaped logic — per Decision 1's table, prefer a
  `fullCoverageStats`-carrying status, fall back to `sampleStats`-carrying,
  and render whichever stats object was actually found (not always
  `sampleStats`). Add `coverageProvenanceNote` as a second, dimmed `Text`
  line under the numbers when non-null — this is the "additive
  trust-signaling" surface the design doc names for Decision 2's
  "Full coverage, resolved" case.
- [ ] **Step 4: `IssueList.tsx`.** Wrap the `DATA_QUALITY_LABELS` badge
  in a `Tooltip` sourced from `coverageProvenanceNote(status)` (widened to
  accept a bare `{ fullCoverageStats }`-shaped read, or pass the whole
  status through `SampleStatsCarrier`) when non-null — the concrete
  "where in `DataQuality` badge rendering it plugs in" the task brief
  calls out for Decision 2. No other change: per Decision 3, per-status
  mixed-state rendering is already correct here.
- [ ] **Step 5: `page.tsx`'s `representativeStatusAcrossReports`.**
  Extend the `.find` predicate to prefer `fullCoverageStats` first, same
  three-tier precedence as `representativeStatus` (the "pinned-station
  dashboard row" case Decision 3's table names).
- [ ] **Step 6:** Verify `frontend/app/stations/[crs]/page.tsx` needs no
  change — its subtitle already routes through `formatSampleSummary`
  (Task 7 already fixed this transitively). Confirm by reading it, not by
  assumption; do not edit it unless something is actually found wrong.
- [ ] **Step 7:** Tests: one new case per touched component asserting the
  full-coverage-preferred behavior (numbers render from
  `fullCoverageStats` when present, `coverageProvenanceNote`/tooltip
  appears where wired), plus confirm every existing test in each file's
  suite still passes unmodified (regression check — these are additive
  precedence extensions, not behavior changes for any status lacking
  `fullCoverageStats`).
- [ ] **Step 8:** `npm test && npx tsc --noEmit` (from `frontend/`).
- [ ] **Step 9:** Commit:
  ```
  git add frontend/app/lines/AllLinesTable.tsx frontend/components/LineStatusCard.tsx \
          frontend/components/RepresentativeInfo.tsx frontend/components/IssueList.tsx \
          frontend/app/page.tsx frontend/app/lines/AllLinesTable.test.tsx \
          frontend/components/LineStatusCard.test.tsx frontend/components/RepresentativeInfo.test.tsx \
          frontend/components/IssueList.test.tsx frontend/app/page.test.tsx
  git commit -m "Wire fullCoverageStats through the six catalogued call sites (Decision 1/3 presentation)"
  ```

### Task 9: Frontend — Trends layer, daily coverage series

**Files:** `frontend/lib/api.ts`, new
`frontend/app/lines/[id]/history/CoverageTrendsResults.tsx`,
`frontend/app/lines/[id]/history/page.tsx`, plus colocated tests

Depends on Task 6 (route) and Task 7 (types).

- [ ] **Step 1:** `api.ts`: add `getLineDailyCoverageStats`/
  `getLineHalfHourlyCoverageStats`, mirroring `getLineDailyStats`/
  `getLineHalfHourlyStats` exactly (both fetchers added for backend
  symmetry with Task 6's two routes, even though only the daily one gets
  a frontend chart this task — see Non-goals).
- [ ] **Step 2:** New `CoverageTrendsResults.tsx`, structurally mirroring
  `TrendsResults.tsx`: fetches `getLineDailyCoverageStats`, an own
  `toCoverageChartPoints` using `resolvedWindows` in place of
  `sampleCycles` for the sparse-gap floor (own placeholder constant, per
  Decision 4's "not designed to a specific number here" — reuse
  `SPARSE_DATA_FLOOR_CYCLES`'s numeric value as a starting point with its
  own name, not a shared constant, since the two are calibrated against
  different underlying cadences per Decision 4), renders the new Decision
  4 honesty copy (verbatim from the design doc, not "trains" → "services"
  find-replaced), reuses `TrendsCharts` unmodified. Empty-state case: "Not
  enough full-coverage data yet for this line." (distinct wording from
  the sample-series empty state, since the reason is different — no
  producer, not "not enough polling yet").
- [ ] **Step 3: `page.tsx` wiring.** **Judgment call**: render
  `CoverageTrendsResults` as a second `Stack` section under the existing
  sample-based `TrendsResults`, inside the same "trends" tab panel, always
  attempted (not conditionally hidden) — the simplest of the three
  UI shapes Decision 4 leaves open (separate series, not a merged/switching
  one), chosen because it needs no new "does this line have any coverage
  rows at all" pre-check and degrades to an honest, harmless empty-state
  message today. Document this choice inline as a comment citing Decision
  4's Open Question 4.
- [ ] **Step 4:** Tests: `CoverageTrendsResults.test.tsx` mirroring
  `TrendsResults.test.tsx`'s existing cases (empty state, populated chart
  points, sparse-gap nulling) against the new fetcher/route shape. Update
  `page.test.tsx` if the new section changes any existing assertion
  (e.g., an exact count of rendered headings).
- [ ] **Step 5:** `npm test && npx tsc --noEmit && npm run build` (from
  `frontend/`).
- [ ] **Step 6:** Commit:
  ```
  git add frontend/lib/api.ts frontend/lib/api.test.ts \
          frontend/app/lines/[id]/history/CoverageTrendsResults.tsx \
          frontend/app/lines/[id]/history/CoverageTrendsResults.test.tsx \
          frontend/app/lines/[id]/history/page.tsx frontend/app/lines/[id]/history/page.test.tsx
  git commit -m "Add the daily full-coverage Trends series (Decision 4 frontend)"
  ```

### Task 10: Final verification

- [ ] **Step 1:** `cd crates && cargo fmt --all -- --check && cargo
  clippy --workspace --all-features && cargo test --workspace` from the
  repo root.
- [ ] **Step 2:** Note explicitly (do not attempt) which tests are
  `#[ignore]`-gated and unverified for lack of a live database in this
  sandbox — list them by name in the final report.
- [ ] **Step 3:** `cd frontend && npm test && npx tsc --noEmit && npm run
  build`.
- [ ] **Step 4:** `git diff --stat main...HEAD` — compare against this
  plan's Global Constraints file-scope list; flag anything unexpected.

## Testing

- **`crates/common`**: new unit tests for `FullCoverageAvailability`'s
  wire shape/default, `LineStatus` read-compat deserialization,
  `CustomLine`'s `full_coverage_enabled: false` — pure, no I/O.
- **`crates/aggregator`**: new unit tests for
  `escalate_from_coverage_stats`, `merge_full_coverage_stats`,
  `merge_full_coverage`, `lines_with_full_coverage` — all pure/synchronous,
  no DB. New `#[ignore]`-gated DB tests for
  `record_daily_coverage_stats`/`record_half_hourly_coverage_stats`/their
  prune functions — **written, not run** (no live Postgres in this
  sandbox).
- **`crates/api`**: new unit tests for the render.rs wire-shape helpers
  and the two new `daily/half_hourly_coverage_stats_to_json` functions —
  pure. New `oneshot`-probe path tests for the two new routes — pure. New
  `#[ignore]`-gated DB tests for the two new routes and
  `normalize_for_diff`'s extension — **written, not run**.
- **`frontend`**: new Vitest coverage for every widened `sampleStats.ts`
  function, the two new helpers, all six touched call sites, and the new
  `CoverageTrendsResults` component — via this repo's existing
  `renderWithMantine`/`vi.mock('@/lib/api', …)` conventions.
- **CI**: the new DB-backed tests run under the existing
  `.github/workflows/ci.yml` `--ignored` job once merged — no new CI job
  needed.
