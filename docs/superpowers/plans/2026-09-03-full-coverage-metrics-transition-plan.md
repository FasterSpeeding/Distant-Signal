# Full-Coverage Metrics Transition — Scoping Verdict & Minimal Plan

> **For agentic workers:** This plan's top-level outcome is DEFERRAL, not a
> full build. Do not treat the presence of a plan file as license to
> implement `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`'s
> full scope. Exactly one tiny task is in scope, and it has already been
> implemented and merged (see Status below). Everything else is explicitly
> listed as deferred, with the condition that must be true before it can be
> reconsidered.

**Spec:** `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the design doc") — read in full before touching anything below. Its own
Decision numbers are referenced throughout this plan.

## Status

Task 1 (below) is implemented as part of the same change that introduces
this plan document, in the `worktree-full-coverage-metrics` branch.
Everything under "Explicitly deferred" remains unimplemented.

## Verdict: defer

The design doc designs a real, coherent migration path for the day this
app's delay/cancellation metrics gain a second, full-coverage producer
(TRUST movement events correlated against the full CIF schedule — "Option
B", per `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`).
That day has not arrived and has no committed date:

- `grep -rn "TrustInferred" --include=*.rs crates/` returns exactly one
  line — the bare enum variant definition in `crates/common/src/lib.rs`.
  It is constructed nowhere in this codebase, confirmed directly, not
  asserted from memory.
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
  re-ran its own Task 8 go/no-go decision gate three separate times
  (2026-08-29, 2026-08-30, 2026-08-31/09-01) and landed on **"not yet"**
  every time. The blocking reasons moved across the three runs (SSO
  unreachable → a STANOX/CRS translation gap → finally, with both fixed,
  a genuine positive result but a sample of "1 of 1" real spot-checked
  disruption instances — too small to call, by the plan's own explicit
  criteria). As of the most recent run, the *mechanism* is proven
  end-to-end in production; the *decision to build Option B* is still
  ungated, with no committed monitoring-window extension or timeline
  attached anywhere in the repo.
- The design doc itself says so, in its own opening paragraph: this
  document does not propose building TRUST-vs-schedule delay inference
  itself — that stays gated on its own future validation/planning pass.
  No implementation plan is included there deliberately; every code/schema
  sketch in it is explicitly marked "sketch, not final code."

Given that, implementing the design doc's concrete scaffolding now —
`FullCoverageAvailability` as a new sibling type, `full_coverage_stats`/
`full_coverage_availability` fields on `LineStatus`, a `full_coverage_enabled`
per-line TOML flag, sibling daily/half-hourly coverage-stats tables, a new
`GET /Line/{id}/Stats/Coverage/{from}/to/{to}` route, and the frontend
precedence-chain/copy changes across `sampleStats.ts`, `AllLinesTable.tsx`,
`RepresentativeInfo.tsx`, `TrendsCharts.tsx` — would all be dead code:
schema and API surface with zero real producer, a TOML flag no line could
truthfully set, DB tables that would only ever hold rows if a human
manually inserted test data, and UI branches that can structurally never be
reached in production. That is a real, ongoing maintenance and review cost
for a trigger condition that is unapproved and undated, not a judgment call
that could reasonably go either way.

**One item is a genuine exception**, meeting the bar of "cheap, reversible,
useful-even-if-never-triggered documentation work": adding a doc comment to
`DataQuality::TrustInferred` (Task 1). It adds no new type, field, flag,
table, route, or code path — the variant already exists and already ships
(it's part of `DataQuality`, serialized on the wire today, with a
rendered-but-inert frontend label — `frontend/components/IssueList.tsx:41`'s
`'trust-inferred': 'Trust-inferred'`, and `frontend/lib/types.ts:85`'s type
union already includes it). All this task does is explain, for the benefit
of the next reader who greps for it, why an apparently-dead enum variant
exists at all and what would need to be true before it's ever constructed.

## Explicitly deferred (revisit only once Option B's own Task 8 gate reaches "go")

- **Decision 1** (`FullCoverageAvailability` type, `full_coverage_stats`/
  `full_coverage_availability` fields on `LineStatus`, wire shape in
  `render.rs`, `normalize_for_diff` changes, all six catalogued frontend
  call-site changes).
- **Decision 2's code changes** (the `statsUnavailableReason`/
  `coverageProvenanceNote` frontend functions, the confident third-branch
  copy, badge treatment). Only the doc-comment half of Decision 2 is taken
  now (Task 1); the frontend copy/precedence half stays deferred — it has
  nothing to render against until `full_coverage_stats` exists.
- **Decision 3** (`full_coverage_enabled` TOML field, `merge_full_coverage`/
  `escalate_from_coverage_stats` in `aggregator`, `representativeStatus`'s
  extended precedence).
- **Decision 4** (sibling daily/half-hourly coverage-stats tables, the new
  `Stats/Coverage` route, new Trends copy/gap-handling).
- **Decision 5**: no action needed — it's an analysis of what's already
  shipped, not a build item.
- Everything under the design doc's own "Explicitly out of scope" section
  remains out of scope a fortiori.

**Reconsideration trigger**: Option B's own Task 8 decision gate (per
`2026-08-29-trust-schedule-delay-validation-findings.md`) reaching an
actual "go" — i.e., a real committed decision and timeline to build the
TRUST-vs-schedule consumer itself, not just a further positive validation
data point. Until then, re-running this plan's scoping judgment is not
expected to change unless that gate moves.

## Global Constraints

- **File scope:** `crates/common/src/lib.rs` only.
- No other crate, no frontend file, no migration, no new route is touched
  by this plan.
- **Testing:** `cargo build -p common`; `cargo doc -p common --no-deps`
  confirmed to introduce no new warnings (3 pre-existing warnings in
  `crates/common/src/metrics.rs`, unrelated to this change, are
  unaffected).

---

### Task 1: Add a doc comment to `DataQuality::TrustInferred`

**Files:**
- Modify: `crates/common/src/lib.rs`

- [x] **Step 1: Add the doc comment**, mirroring `Tfl`'s existing style and
  explaining Option B's status plainly, including its still-not-"go"
  validation gate.
- [x] **Step 2: Verify the doc comment renders cleanly** —
  `cargo doc -p common --no-deps`: passes, no new warnings.
- [x] **Step 3: Verify no other file needed touching** —
  `grep -rn "TrustInferred" --include=*.rs crates/`: one line, the enum
  definition with the new doc comment above it. No construction site added,
  by design.
- [x] **Step 4: Build** — `cargo build -p common`: passes.
- [x] **Step 5: Commit** — included in the same commit as this plan
  document.
