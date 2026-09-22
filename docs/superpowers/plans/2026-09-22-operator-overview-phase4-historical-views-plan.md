# Plan: Operator Overview Phase 4 — Historical Views for Operators and the Network

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 4 of
`docs/superpowers/specs/2026-09-22-operator-overview-design.md` (section D,
the operator/network half only): extend the existing per-line Trends
rate-rollup mechanism (`line_status_daily_stats` / `line_status_half_hourly_stats`,
rendered by `TrendsCharts.tsx`) so an operator (e.g. "SW", "GW") and the
whole network can each get their own delay/cancellation/skip-rate history
view, computed on read by summing the same per-line rollup rows across a
set of `line_id`s — no new tables, no new migration, no aggregator change.

**Explicitly NOT this plan's job** (per the spec, and restated in this
plan's own Non-goals below): the per-line Timeline and Trends tabs already
work and need no changes; an operator/network status-change **Timeline**
equivalent is deliberately not built (spec §D.3 — "disproportionate
complexity"); Phases 1–3 (all-lines dashboard, per-line drill-down
completion, the `/operators` list page + operator pinning) are being
planned/executed separately and are not touched here except for one named
external dependency (below).

**External dependency (read before Task 1):** this plan needs a way to
resolve "which line_ids belong to operator code X, excluding private
custom lines." Phase 3 (the `/operators` list page, planned separately) is
expected to introduce exactly this as a byproduct of building
`GET /public/operators`. This plan assumes the following primitive exists,
by this exact contract:

```rust
/// Resolves every line_id whose `operators` array contains `operator_code`
/// -- catalogue lines (`app.config.lines`) plus, when `operator_code ==
/// common::TFL_OPERATOR` ("TfL"), every TfL line id from
/// `queries::tfl_line_summaries` that isn't merged into an NR line
/// (`common::nr_line_id_for_tfl(&id).is_none()`). Deliberately excludes
/// EVERY custom line, unconditionally (not just a non-owner's) -- this is
/// a public aggregate; a private, user-authored line must never leak into
/// it, regardless of who's asking. Returns `Vec::new()` for a code that
/// matches no line, never an error -- same "unknown key -> empty, not a
/// failure" convention `queries::daily_stats_for_range` already uses for
/// an unknown `line_id`.
pub async fn line_ids_for_operator(
    pool: &PgPool,
    catalogue_lines: &[common::LineDefinition],
    operator_code: &str,
) -> anyhow::Result<Vec<String>>
```

Task 1 below checks whether this (or something contract-equivalent) exists
yet and implements it as a small prerequisite if not — Phase 3's landing
order is not a blocker for this plan. If Phase 3 already landed with a
different name/module path by the time this plan is executed, adapt this
plan's imports to that real name; the contract above (inputs, exclusion of
custom lines, empty-not-error for an unknown code) is what matters, not
the exact path.

**Architecture:** backend before frontend. Task 1 resolves the external
dependency. Task 2 adds three new cross-line query functions to
`crates/api/src/data/queries.rs` — `daily_stats_for_range_multi`,
`half_hourly_stats_for_range_multi`, `sub_daily_stats_for_range_multi` —
each a `GROUP BY`/`SUM` sibling of the existing single-line function of
the same name (minus `_multi`), taking `&[String]` line ids instead of one
`&str`. Task 3 promotes the four existing per-line JSON-shaping functions
in `crates/api/src/routes/line_status.rs` (`daily_stats_to_json` etc.)
from private to `pub(crate)` so Task 4's new routes reuse them verbatim —
the operator/network response bodies must be byte-for-byte the same shape
as the per-line ones, since Task 7/8's frontend components are going to
feed them through the *exact same* `toChartPoints`/`ChartPoint`/
`TrendsCharts` pipeline the per-line page already uses. Task 4 adds one new
route module, `crates/api/src/routes/operator_history.rs`, with eight GET
routes (four operator-scoped, four network-scoped), mounted into
`routes::public_router()` — fully unauthenticated, since the line-id sets
these routes ever touch exclude private custom lines entirely (Non-goals).
Tasks 5–8 are frontend: new `lib/api.ts` fetch functions and `lib/types.ts`
aliases (Task 5); a small, additive generalization of
`GranularityControl.tsx`/`HistoryRangePicker.tsx` so their hardcoded
`/lines/{id}/history` navigation target becomes a caller-supplied
`basePath` (Task 6 — this is a real, verified correction to the spec's
"reuse unmodified" framing, see Judgment Call 2); and two new page routes,
`/operators/[code]/history` and `/network/history`, each with its own
small Server Component sibling to `TrendsResults.tsx` (Tasks 7–8). Task 9
is manual end-to-end verification plus best-effort entry-point wiring.

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres 16.

**Spec:** `docs/superpowers/specs/2026-09-22-operator-overview-design.md`
— authoritative for the architectural decisions this plan implements
(compute-on-read over precompute; no operator/network Timeline; reuse
`TrendsCharts.tsx` et al.; retention echo unchanged). This plan does not
re-litigate anything that spec already settled; it resolves the specific
things the spec left open for "the implementation plan" plus two things
the spec asserted that direct code-reading disproved (Judgment Calls 2
and 3 below) — sections A, B, C, and the per-line half of D are out of
scope entirely; other agents are covering those.

---

## Judgment calls this plan makes (read before Task 1)

1. **The spec's flagged-as-unverified index question (Open Question 5 /
   D.2) is resolved: yes, the needed indexes exist, and table sizes make
   compute-on-read cheap even at full network scope — precompute is not
   needed now or foreseeably.** Verified directly against the migrations:
   `crates/api/migrations/20260831090001_line_status_daily_stats.sql`
   creates `line_status_daily_stats_line_day ON line_status_daily_stats
   (line_id, day)`; `20260902090000_line_status_hourly_stats.sql` (renamed
   by `20260902170000_...to_half_hourly.sql`) creates
   `line_status_half_hourly_stats_line_half_hour ON
   line_status_half_hourly_stats (line_id, half_hour_start)`. Both are
   plain composite btree indexes with `line_id` leading — exactly what a
   `WHERE line_id = ANY($1) AND day/half_hour_start BETWEEN $2 AND $3`
   query needs. Beyond the index, the tables are also just small: this
   deployment has on the order of 125 lines total (per the spec's own
   estimate), `daily_stats_retention_days` defaults to 300 in this repo's
   test fixtures (`crates/api/src/routes/lines.rs:896`) giving an upper
   bound of roughly 125 × 300 ≈ 37,500 rows ever live in
   `line_status_daily_stats`, and `half_hourly_stats_retention_hours: 840`
   (`lines.rs:897`) gives roughly 125 × 840 ≈ 105,000 rows in the
   half-hourly table. A network-wide query with no operator filter at all
   (`WHERE line_id = ANY($1)` with every catalogue id) is, worst case, a
   full scan of a five-figure-row table — trivially fast on Postgres 16.
   **Conclusion: compute-on-read is not a stopgap here, it's simply
   correct at this data volume** — this plan does not add a "revisit if
   read cost becomes a problem" TODO because the numbers already rule that
   concern out at current scale; a future large increase in line count or
   retention would be the actual trigger to revisit, not time elapsed.

2. **The spec's claim that `GranularityControl.tsx`/`HistoryRangePicker.tsx`
   are reusable "UNMODIFIED" is false for both, verified by reading both
   files in full — the fix is a one-word prop rename, not a rewrite, and
   is included in this plan (Task 6).** `TrendsCharts.tsx` genuinely is
   generic (`points: ChartPoint[]`, `granularity: TrendGranularity`, no
   line-specific prop) — that half of the spec's claim holds. But
   `GranularityControl.tsx`'s `handleChange` and `HistoryRangePicker.tsx`'s
   `handleSearch`/`handlePreset` all call `router.push(`/lines/${lineId}/history?...`)`
   — the `/lines/{id}/history` path is hardcoded into both components, not
   parameterized. An operator or network history page reusing either
   component as-is would have its range/granularity controls silently
   navigate back to `/lines/{lineId}/history` instead of staying on
   `/operators/{code}/history` or `/network/history`. The smallest fix
   that doesn't touch behavior for the existing caller: rename each
   component's `lineId: string` prop to `basePath: string` and replace
   `` `/lines/${lineId}/history` `` with `` `${basePath}` `` at each of the
   three call sites inside those two files, then update
   `/lines/[id]/history/page.tsx`'s two existing call sites to pass
   `basePath={`/lines/${id}/history`}` instead of `lineId={id}`. This is a
   pure prop-rename plus a plugged-in literal that was already being built
   the same way inline — the per-line page's rendered output and behavior
   are unchanged (Task 6's Verify step confirms this explicitly, since the
   spec's own framing ("no work needed") makes an accidental regression
   here the single easiest way to violate this plan's scope).

3. **New sibling Server Components (`OperatorTrendsResults.tsx`,
   `NetworkTrendsResults.tsx`), not a `scope`-parameterized rewrite of
   `TrendsResults.tsx`.** This codebase already has a live precedent for
   exactly this choice: `HalfHourlyTrendsResults.tsx`'s own doc comment
   says it is "deliberately a separate component, not a
   `granularity`-branching version of [`TrendsResults`], since the fetch,
   sparse floor, and honesty copy are all genuinely half-hourly-specific."
   The operator/network case is the same shape of decision one level up:
   the sparse floors and honesty copy should stay byte-identical across
   line/operator/network scope (a "day" is a day regardless of how many
   lines feed it), so duplicating those verbatim would be worse than
   reusing them — but the *fetch* target genuinely differs (a `code`
   versus an `id` versus nothing), and `TrendsResults.tsx` backs the
   spec's own "already fully built, needs no work" per-line page. Widening
   its prop signature to a `scope` discriminated union is exactly the kind
   of change that could regress a working, unrelated page for the sake of
   this plan's new surface. Resolution: Task 6 additionally exports
   `TrendsResults.tsx`'s existing, currently module-private
   `SPARSE_FLOOR`/`HONESTY_COPY` consts and its already-exported, fully
   generic `toChartPoints<T extends StatsRow>` helper; Tasks 7/8 import all
   three into their own new sibling components rather than duplicating the
   floor numbers or the (long, careful) honesty-copy sentences by hand.
   `TrendsResults.tsx`'s own `fetchPoints` dispatch function and JSX are
   left completely untouched.

4. **No operator/network full-coverage rollup** (no
   `OperatorCoverageTrendsResults`/`NetworkCoverageTrendsResults`
   equivalent of `CoverageTrendsResults.tsx`). Verified:
   `line_status_daily_coverage_stats`/`line_status_half_hourly_coverage_stats`
   have no real writer yet — `CoverageTrendsResults.tsx`'s own doc comment
   states "Always resolves an empty array today, since no full-coverage
   producer exists yet," and grepping the aggregator/pollers confirms
   nothing populates those tables in this codebase as it stands. Extending
   an always-empty rollup to a second and third scope (operator, network)
   before it holds real data anywhere adds route/query/frontend surface
   for something with literally nothing to show. Non-goal — revisit
   alongside (not before) whatever future work gives the per-line coverage
   charts real data.

5. **TfL-tagged lines never accrue rows in `line_status_daily_stats`/
   `line_status_half_hourly_stats` at all — confirmed, not assumed — so
   "network" scope in this plan means catalogue (National Rail) lines
   only, and an operator page for a TfL-only operator will show "not
   enough data" indefinitely.** Verified via
   `crates/aggregator/src/main.rs:192`'s
   `aggregation::merge_custom_lines(static_lines, custom_lines)` — the
   list the aggregator's `record_daily_stats`/`record_half_hourly_stats`
   pass (`main.rs:345-347`) iterates is catalogue + custom lines only, no
   TfL lines merged in anywhere; and `crates/poller-tfl/` has zero
   references to `record_daily_stats`/`record_half_hourly_stats` (grepped
   directly). TfL's own `line_status` rows exist (written by
   `poller-tfl`), but the two rate-rollup tables this whole feature reads
   from are never written for a `tfl-`-prefixed line id. This plan's
   `network_line_ids` helper (Task 4) therefore draws only from
   `app.config.lines` (the NR catalogue) — the same source `list_lines`
   and `record_daily_stats`'s own line set already use — never from
   `queries::tfl_line_summaries`. This is stated here plainly as a known,
   inherent limitation (not a bug to silently paper over): an
   `/operators/TfL/history` page (or `LO`/`XR`, if Phase 3 ever widens
   "TfL" the same way `AllLinesTable.tsx`'s `TFL_ADJACENT_OPERATORS` does
   for filtering) will render the same honest "not enough sampled data
   yet" empty state `TrendsResults.tsx` already shows for a genuinely
   quiet line, forever, unless a future TfL-side rate-rollup producer is
   built — out of scope for this plan.

6. **New route module, not an edit to Phase 3's (not-yet-written)
   `routes/operators.rs`.** Since Phase 3 may not exist in this worktree
   yet at execution time (confirmed: no `routes/operators.rs`, no
   `app/operators/` directory exist as of this plan's writing — grepped
   directly), and since this plan's routes are read-only history queries
   with no relationship to Phase 3's own route handlers beyond the one
   named dependency, putting them in their own file
   (`crates/api/src/routes/operator_history.rs`) avoids a merge conflict
   with whichever agent lands Phase 3, and avoids this plan silently
   depending on Phase 3's internal route-file structure (only its one
   named function).

7. **Every new route is fully public/unauthenticated — no
   `OptionalAuthenticatedUser`, no per-line ownership check.** The
   per-line `/Line/{id}/Stats/...` routes need `empty_if_unreadable`
   because a `custom-` line id can appear in their `{id}` path segment and
   is privacy-sensitive. This plan's line-id sets (Judgment Call 5/the
   external dependency's own contract) never include a custom line at
   all, so there is nothing to gate — resolves spec Open Question 3
   (operator/network history should be public, matching
   `/public/tocs/all`/`/public/history-retention`). Confirmed against
   precedent: `getHistoryRetention()`/`getDataFreshness()` in
   `frontend/lib/api.ts` — genuinely public, session-irrelevant endpoints
   — deliberately omit `cookieForwardInit()`, unlike every
   `getLine*Stats` fetch function (which forwards cookies because a
   `custom-` id might be in play). This plan's new frontend fetch
   functions (Task 5) follow the `getHistoryRetention`/`getDataFreshness`
   precedent, not the `getLine*Stats` one.

---

## Non-goals

- **No operator/network status-change Timeline.** Spec §D.3, restated: the
  complexity of reconstructing a merged "worst status across N lines at
  time T" view from N independent `line_status_history` event streams is
  disproportionate to the value over "current rollup (Phase 3) + per-line
  drill-down (already built) for detail." No task here touches
  `line_status_history` or `queries::line_status_history_for_range`.
- **No operator/network full-coverage rollup charts.** See Judgment Call
  4. No task adds a coverage-stats route/query/component at operator or
  network scope.
- **No precomputed `operator_status_daily_stats`/`network_status_daily_stats`
  tables, no new migration.** See Judgment Call 1 — compute-on-read is not
  just "good enough for now," the numbers say it's simply correct here.
- **No change to `TrendsCharts.tsx`.** Verified genuinely generic (Judgment
  Call 2) — zero edits.
- **No behavior change to `/lines/[id]/history` or `/lines/[id]`.** Task
  6's prop rename in `GranularityControl.tsx`/`HistoryRangePicker.tsx` is
  the only edit reaching the per-line page, and it's verified
  output-identical (Task 6's own Verify step).
- **No Phase 1/2/3 work.** The all-lines dashboard, per-line drill-down
  completion, the `/operators` list page, and operator pinning are each
  being planned/executed separately. This plan's only contact with them is
  the one named external dependency (top of this document) and one
  best-effort, non-blocking link-wiring step in Task 9.
- **No auth/session gating on any new route.** See Judgment Call 7.
- **No new frontend types for the operator/network response shapes.**
  `LineDailyStats`/`LineHalfHourlyStats`/`LineHourlyStats`/
  `LineSixHourlyStats` (`frontend/lib/types.ts`) carry no `lineId`/line-
  specific field today — verified by reading all four definitions
  (`types.ts:220-292`). Task 5 adds plain type *aliases*
  (`OperatorDailyStats = LineDailyStats`, etc.) for readability at the new
  call sites, not new structural types.

## Global Constraints

- **External dependency contract** (top of this document) must not be
  reimplemented with a different exclusion rule than "every custom line,
  unconditionally, excluded" — this is a public-aggregate correctness
  requirement, not a style preference (spec Open Question 1).
- **Response JSON shape parity.** Every new route's response body must be
  produced by the *same* `daily_stats_to_json`/`half_hourly_stats_to_json`/
  `sub_daily_stats_to_json` functions the per-line routes already use
  (Task 3 promotes them to `pub(crate)` for exactly this reuse) — never a
  second, hand-written copy of the same rate-derivation arithmetic. Two
  independently-maintained copies of `avg_delay_minutes = delay_minutes_sum
  / running_count` (with its zero-guard) are exactly the kind of drift
  this constraint exists to prevent.
- **No new migration.** Every task in this plan reads existing tables only.
- **Testing** (this repo's actual CI invocations —
  `.github/workflows/ci.yml`): Rust: `cargo fmt --all`, `cargo clippy
  --workspace --all-features --all-targets -- -D warnings` (CI's own job
  runs `check-args: --all-features` via `auguwu/clippy-action`; this plan
  additionally asks for `--all-targets` locally, the stricter default),
  `cargo test --workspace` (ignored tests skipped, `ci.yml:219-220`), and
  `cargo test -p api -- --ignored --test-threads=1` (`ci.yml:229-230`) for
  every DB-gated test this plan adds, against
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres` with
  a real local Postgres 16 (`ci.yml`'s own `services:` block). Frontend:
  `npx tsc --noEmit`, `npm test` (vitest, `ci.yml:268-269`), `npm run
  build` (`ci.yml`'s own three frontend steps) — run all three before
  considering any frontend task done, not just the changed test file.
  **UI verification**: no automated end-to-end coverage exists for either
  new page — start the dev stack and manually verify in a real browser
  (folded into Task 9, not a separate task).
- **File scope.** Modified/created:
  `crates/api/src/data/operators.rs` (new, only if Task 1 finds the
  external dependency doesn't already exist),
  `crates/api/src/data/mod.rs` (only alongside the above),
  `crates/api/src/data/queries.rs`,
  `crates/api/src/routes/line_status.rs`,
  `crates/api/src/routes/operator_history.rs` (new),
  `crates/api/src/routes/mod.rs`,
  `frontend/lib/api.ts`,
  `frontend/lib/types.ts`,
  `frontend/app/lines/[id]/history/GranularityControl.tsx`,
  `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`,
  `frontend/app/lines/[id]/history/TrendsResults.tsx`,
  `frontend/app/lines/[id]/history/page.tsx`,
  `frontend/app/operators/[code]/history/page.tsx` (new),
  `frontend/app/operators/[code]/history/OperatorTrendsResults.tsx` (new),
  `frontend/app/network/history/page.tsx` (new),
  `frontend/app/network/history/NetworkTrendsResults.tsx` (new).
  Task 9 may additionally touch a Phase-3 `/operators/[code]/page.tsx` or
  a Phase-1 entry point **only if those files already exist** at
  execution time — see that task's own guardrail. No other file changes.

---

## Task 1: External dependency — verify or implement `line_ids_for_operator`

**Files:** possibly create `crates/api/src/data/operators.rs`, modify
`crates/api/src/data/mod.rs`. Independent, first task.

- [ ] **Step 1: Check whether Phase 3 already landed this.**

```bash
grep -rn "fn line_ids_for_operator\|fn.*operator.*line_ids\|fn lines_for_operator" crates/api/src/data/ crates/api/src/routes/
```

  If this finds a function matching the contract at the top of this
  document (takes a pool + catalogue lines + an operator code, excludes
  custom lines, returns empty-not-error for an unknown code) — **skip to
  Step 5**, importing that real function in place of
  `operators::line_ids_for_operator` throughout Tasks 2–4 below (adjust
  the `use` path to match).

- [ ] **Step 2 (only if Step 1 found nothing): create
  `crates/api/src/data/operators.rs`** implementing exactly the contract
  above:

```rust
//! Prerequisite for
//! docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md
//! -- resolves "which line_ids belong to operator code X." Phase 3 (the
//! planned `/operators` list page) is expected to need and supersede this
//! same primitive; if it lands first, delete this file and re-point Phase
//! 4's imports at Phase 3's own version instead (same contract).
//!
//! Mirrors `routes::lines::list_lines`'s own three-source enumeration
//! (`crates/api/src/routes/lines.rs:379-448`) minus the custom-line
//! branch: catalogue lines (`app.config.lines`) whose `operators` array
//! contains `operator_code`, plus -- only for `common::TFL_OPERATOR`
//! ("TfL") -- every TfL line id from `queries::tfl_line_summaries` that
//! isn't merged into an NR line (`common::nr_line_id_for_tfl`, the same
//! check `list_lines`'s own `is_merged_into_nr_line` makes). Custom lines
//! are excluded unconditionally, not just a non-owner's -- this backs a
//! PUBLIC aggregate (operator/network history), and a private,
//! user-authored line must never leak into one regardless of who is
//! asking. See this plan's top-level "External dependency" section and
//! spec Open Question 1.

use anyhow::Result;
use sqlx::PgPool;

use crate::data::queries;

/// See this module's doc comment. Returns `Vec::new()` for a code
/// matching no line -- not an error -- matching
/// `queries::daily_stats_for_range`'s own "unknown line_id -> empty vec"
/// convention.
pub async fn line_ids_for_operator(
    pool: &PgPool,
    catalogue_lines: &[common::LineDefinition],
    operator_code: &str,
) -> Result<Vec<String>> {
    let mut ids: Vec<String> = catalogue_lines
        .iter()
        .filter(|line| line.operators.iter().any(|op| op == operator_code))
        .map(|line| line.id.clone())
        .collect();

    if operator_code == common::TFL_OPERATOR {
        let tfl = queries::tfl_line_summaries(pool).await?;
        ids.extend(
            tfl.into_iter()
                .filter(|line| common::nr_line_id_for_tfl(&line.id).is_none())
                .map(|line| line.id),
        );
    }

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogue_line(id: &str, operators: &[&str]) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: format!("Test {id}"),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            stations: vec![],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                line_ids_for_operator -- --ignored`"]
    async fn a_known_catalogue_operator_returns_its_lines_only() {
        let pool = crate::test_support::connect_for_test().await; // adapt to this crate's real DB-test connect helper, see Task 2's own tests for the inline pattern if no shared helper exists
        let catalogue = vec![
            catalogue_line("TEST-OP-A", &["SW"]),
            catalogue_line("TEST-OP-B", &["GW"]),
            catalogue_line("TEST-OP-C", &["SW", "GW"]),
        ];
        let ids = line_ids_for_operator(&pool, &catalogue, "SW")
            .await
            .expect("line_ids_for_operator");
        let mut ids = ids;
        ids.sort();
        assert_eq!(ids, vec!["TEST-OP-A".to_string(), "TEST-OP-C".to_string()]);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                line_ids_for_operator -- --ignored`"]
    async fn an_unknown_operator_code_returns_empty_not_an_error() {
        let pool = crate::test_support::connect_for_test().await;
        let catalogue = vec![catalogue_line("TEST-OP-A", &["SW"])];
        let ids = line_ids_for_operator(&pool, &catalogue, "NOSUCHCODE")
            .await
            .expect("line_ids_for_operator for an unknown code");
        assert!(ids.is_empty());
    }
}
```

  **Note on the test helper placeholder**: this file has no live-database
  connection helper of its own yet; the two tests above use a placeholder
  `crate::test_support::connect_for_test()` — replace both call sites with
  a plain inline `sqlx::postgres::PgPoolOptions::new().connect(&std::env::var("DATABASE_URL").expect(...)).await.expect(...)`,
  matching the exact inline pattern every other DB-gated test in this
  crate uses (e.g. `crates/api/src/data/queries.rs`'s
  `daily_stats_for_range_filters_orders_and_handles_unknown_lines` test,
  read directly while researching this plan) — there is no shared
  `connect()` helper across files in this crate; each test file/module
  defines (or inlines) its own. Do not introduce a new shared helper here;
  match the existing convention.

- [ ] **Step 3: Register the module.** In `crates/api/src/data/mod.rs`, add
  `pub mod operators;` alphabetically (between `pub mod notifier_forward_queue;`
  and `pub mod preferences;`).

- [ ] **Step 4: Verify**

```bash
cargo build -p api
cargo test -p api operators:: 
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api line_ids_for_operator -- --ignored --test-threads=1
```

  Expected: builds clean; both DB-gated tests pass.

- [ ] **Step 5: Commit** (skip if Step 1 found an existing implementation —
  in that case there is nothing to commit for this task; proceed straight
  to Task 2 using the real import path)

```bash
git add crates/api/src/data/operators.rs crates/api/src/data/mod.rs
git commit -m "api: add line_ids_for_operator, the operator->line-id resolution Phase 4's history queries depend on"
```

---

## Task 2: Cross-line query functions in `queries.rs`

**Files:** modify `crates/api/src/data/queries.rs`.

Depends on nothing from Task 1 (these functions take a plain `&[String]`
of line ids, not an operator code) — can be done in parallel with Task 1
if desired, though this plan lists them in sequence for clarity.

- [ ] **Step 1: Add `daily_stats_for_range_multi`**, directly after
  `sub_daily_stats_for_range`'s closing brace (`queries.rs:1937`, right
  before the `// --- Decision 4 scaffolding ...` comment at line 1939):

```rust
/// Cross-line sibling of `daily_stats_for_range` -- sums the same
/// `line_status_daily_stats` rows across every id in `line_ids` instead of
/// reading one line. Used for an operator's or the whole network's Trends
/// rollup (docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md).
///
/// Every column here is ALREADY a running sum-per-line-per-day (see that
/// table's own migration comment) -- summing a sum across several lines
/// for the same day is exactly as lossless as summing a sum across
/// several poll cycles for one line, which `sub_daily_stats_for_range`
/// already relies on (that function's own doc comment: "summing sums is
/// lossless per [Decision 2's] Correction 4"). Rates are still derived at
/// READ time from the summed numerator/denominator columns, never
/// pre-averaged across lines.
///
/// An empty `line_ids` slice is valid and returns an empty vec (Postgres'
/// `= ANY('{}')` is always false, never an error) -- the caller (an
/// unknown operator code, or a network with zero catalogue lines) needs
/// no special-case branch for this.
pub async fn daily_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT day,
                SUM(sample_cycles)::bigint AS sample_cycles,
                SUM(total)::bigint AS total,
                SUM(delayed)::bigint AS delayed,
                SUM(cancelled)::bigint AS cancelled,
                SUM(skipped)::bigint AS skipped,
                SUM(running_count)::bigint AS running_count,
                SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_daily_stats
         WHERE line_id = ANY($1) AND day BETWEEN $2 AND $3
         GROUP BY day
         ORDER BY day",
    )
    .bind(line_ids)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DailyStatsRow {
                day: row.try_get("day")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}
```

- [ ] **Step 2: Add `half_hourly_stats_for_range_multi`**, directly below:

```rust
/// Cross-line sibling of `half_hourly_stats_for_range` -- same relationship
/// `daily_stats_for_range_multi` has to `daily_stats_for_range`.
pub async fn half_hourly_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT half_hour_start,
                SUM(sample_cycles)::bigint AS sample_cycles,
                SUM(total)::bigint AS total,
                SUM(delayed)::bigint AS delayed,
                SUM(cancelled)::bigint AS cancelled,
                SUM(skipped)::bigint AS skipped,
                SUM(running_count)::bigint AS running_count,
                SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = ANY($1) AND half_hour_start BETWEEN $2 AND $3
         GROUP BY half_hour_start
         ORDER BY half_hour_start",
    )
    .bind(line_ids)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}
```

- [ ] **Step 3: Add `sub_daily_stats_for_range_multi`**, directly below —
  the cross-line sibling of `sub_daily_stats_for_range`, combining both
  the `date_bin` re-bucketing AND the cross-line `SUM` in one query:

```rust
/// Cross-line sibling of `sub_daily_stats_for_range` -- same `date_bin`
/// re-bucketing (1-hour or 6-hour, selected by `bucket_minutes`, always a
/// literal `60`/`360` from this crate's own route handlers, never raw
/// request input -- see that function's own doc comment for the full
/// injection-safety/origin-alignment reasoning, unchanged here), but
/// summed across every id in `line_ids` in the SAME `GROUP BY` pass rather
/// than as a separate step -- there is no correctness difference between
/// "sum across lines, then re-bucket" and "re-bucket, then sum across
/// lines" for a plain SUM aggregate, so the single combined query is
/// preferred for one round trip instead of two.
pub async fn sub_daily_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_minutes: i64,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            date_bin($4 * INTERVAL '1 minute', half_hour_start, TIMESTAMPTZ '2000-01-01T00:00:00Z') AS half_hour_start,
            SUM(sample_cycles)::bigint AS sample_cycles,
            SUM(total)::bigint AS total,
            SUM(delayed)::bigint AS delayed,
            SUM(cancelled)::bigint AS cancelled,
            SUM(skipped)::bigint AS skipped,
            SUM(running_count)::bigint AS running_count,
            SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = ANY($1) AND half_hour_start BETWEEN $2 AND $3
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(line_ids)
    .bind(from)
    .bind(to)
    .bind(bucket_minutes)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Add DB-gated tests**, alongside the existing
  `daily_stats_for_range_filters_orders_and_handles_unknown_lines`/
  `half_hourly_stats_for_range_filters_orders_and_handles_unknown_lines`/
  `sub_daily_stats_for_range_*` tests in this file's `#[cfg(test)] mod
  tests` block (same inline `PgPoolOptions::connect` pattern those use —
  read directly at `queries.rs:3451-3691` while researching this plan):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                daily_stats_for_range_multi -- --ignored`"]
    async fn daily_stats_for_range_multi_sums_across_lines_and_excludes_others() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-MULTI-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-MULTI-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0), \
                ('TEST-MULTI-OTHER', '2026-08-01', 20, 200, 20, 20, 20, 160, 500.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let from = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let line_ids = vec!["TEST-MULTI-A".to_string(), "TEST-MULTI-B".to_string()];
        let rows = daily_stats_for_range_multi(&pool, &line_ids, from, to)
            .await
            .expect("daily_stats_for_range_multi");

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-MULTI-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        assert_eq!(rows.len(), 1, "one row per day, not one per contributing line");
        let row = &rows[0];
        assert_eq!(row.total, 180, "100 + 80, TEST-MULTI-OTHER excluded");
        assert_eq!(row.delayed, 8);
        assert_eq!(row.sample_cycles, 18);
        assert_eq!(row.delay_minutes_sum, 180.0);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                daily_stats_for_range_multi_an_empty_line_id_set -- --ignored`"]
    async fn daily_stats_for_range_multi_an_empty_line_id_set_returns_empty_not_an_error() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let from = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        let rows = daily_stats_for_range_multi(&pool, &[], from, to)
            .await
            .expect("daily_stats_for_range_multi with no line ids");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                half_hourly_stats_for_range_multi -- --ignored`"]
    async fn half_hourly_stats_for_range_multi_sums_across_lines() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let bucket: chrono::DateTime<chrono::Utc> = "2026-08-01T12:00:00Z".parse().unwrap();
        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats \
                (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-HH-MULTI-A', $1, 5, 50, 2, 0, 1, 49, 30.0), \
                ('TEST-HH-MULTI-B', $1, 4, 40, 1, 0, 0, 40, 10.0) \
             ON CONFLICT (line_id, half_hour_start) DO UPDATE SET total = EXCLUDED.total",
        )
        .bind(bucket)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let line_ids = vec!["TEST-HH-MULTI-A".to_string(), "TEST-HH-MULTI-B".to_string()];
        let rows = half_hourly_stats_for_range_multi(
            &pool,
            &line_ids,
            bucket - chrono::Duration::minutes(30),
            bucket + chrono::Duration::minutes(30),
        )
        .await
        .expect("half_hourly_stats_for_range_multi");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id LIKE 'TEST-HH-MULTI-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total, 90);
        assert_eq!(rows[0].delayed, 3);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                sub_daily_stats_for_range_multi -- --ignored`"]
    async fn sub_daily_stats_for_range_multi_groups_by_bucket_and_sums_across_lines() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let first: chrono::DateTime<chrono::Utc> = "2026-08-01T12:00:00Z".parse().unwrap();
        let second: chrono::DateTime<chrono::Utc> = "2026-08-01T12:30:00Z".parse().unwrap();
        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats \
                (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-SUBDAY-MULTI-A', $1, 5, 50, 2, 0, 1, 49, 30.0), \
                ('TEST-SUBDAY-MULTI-B', $2, 4, 40, 1, 0, 0, 40, 10.0) \
             ON CONFLICT (line_id, half_hour_start) DO UPDATE SET total = EXCLUDED.total",
        )
        .bind(first)
        .bind(second)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let line_ids = vec!["TEST-SUBDAY-MULTI-A".to_string(), "TEST-SUBDAY-MULTI-B".to_string()];
        let rows = sub_daily_stats_for_range_multi(
            &pool,
            &line_ids,
            first - chrono::Duration::minutes(30),
            second + chrono::Duration::minutes(30),
            60,
        )
        .await
        .expect("sub_daily_stats_for_range_multi");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id LIKE 'TEST-SUBDAY-MULTI-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        // Both half-hourly rows fall in the same 1-hour bucket (12:00-13:00) --
        // one combined row, both lines' contributions summed together.
        assert_eq!(rows.len(), 1, "both rows fall in the same 1-hour bucket");
        assert_eq!(rows[0].total, 90);
        assert_eq!(rows[0].delayed, 3);
    }
```

- [ ] **Step 5: Verify**

```bash
cargo build -p api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api \
  -- --ignored --test-threads=1 daily_stats_for_range_multi half_hourly_stats_for_range_multi sub_daily_stats_for_range_multi
```

  Expected: builds clean, all six new tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "api: add cross-line daily/half-hourly/sub-daily stats query functions for operator/network scope"
```

---

## Task 3: Promote the four JSON mapper functions to `pub(crate)`

**Files:** modify `crates/api/src/routes/line_status.rs`.

Small, mechanical, zero-behavior-change task — resolves the "Response JSON
shape parity" Global Constraint. Independent of Tasks 1–2; can be done any
time before Task 4.

- [ ] **Step 1: Widen visibility** of the four existing mapper functions —
  change `fn daily_stats_to_json` (`line_status.rs:420`) to `pub(crate) fn
  daily_stats_to_json`; likewise `fn half_hourly_stats_to_json`
  (`:468`), `fn sub_daily_stats_to_json` (`:521`), and — not strictly
  needed by this plan (coverage is out of scope, Non-goals) but included
  for consistency since it's the same visibility change on a sibling
  function in the same file — leave `daily_coverage_stats_to_json`/
  `half_hourly_coverage_stats_to_json` as private `fn`, unchanged: this
  plan has no caller for them outside this file, so widening their
  visibility would be speculative, not reuse.

  Concretely, only these two lines change:

```rust
fn daily_stats_to_json(row: queries::DailyStatsRow) -> Value {
```
  becomes
```rust
pub(crate) fn daily_stats_to_json(row: queries::DailyStatsRow) -> Value {
```
  and
```rust
fn half_hourly_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
```
  becomes
```rust
pub(crate) fn half_hourly_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
```
  and
```rust
fn sub_daily_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
```
  becomes
```rust
pub(crate) fn sub_daily_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
```

- [ ] **Step 2: Verify** — a pure visibility widening cannot change this
  file's own behavior or break any existing caller (all of which are in
  this same file and already call these by their bare, now-still-valid
  names):

```bash
cargo build -p api
cargo test -p api --lib line_status::
```

  Expected: builds clean, this file's existing unit tests
  (`a_single_mode_still_works` etc.) pass unchanged.

- [ ] **Step 3: Commit**

```bash
git add crates/api/src/routes/line_status.rs
git commit -m "api: widen daily/half-hourly/sub-daily stats JSON mappers to pub(crate) for reuse by operator_history routes"
```

---

## Task 4: New route module `operator_history.rs`

**Files:** create `crates/api/src/routes/operator_history.rs`, modify
`crates/api/src/routes/mod.rs`.

Depends on Task 1 (the external dependency), Task 2 (the `_multi` query
functions), and Task 3 (the `pub(crate)` mappers).

- [ ] **Step 1: Write the route module**

```rust
//! `GET /public/operators/{code}/stats/...` and `GET /public/network/stats/...`
//! -- the operator- and network-scoped Trends rollup, per
//! docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md.
//! Four granularities per scope (daily, half-hourly, hourly, six-hourly),
//! mirroring the four per-line routes in `line_status.rs` exactly --
//! same response shape (reusing that file's own JSON mappers, now
//! `pub(crate)`), same `{from}/to/{to}` path-segment idiom. Deliberately
//! unauthenticated (no `OptionalAuthenticatedUser`, no
//! `empty_if_unreadable`-style gate): every line id these routes ever
//! touch comes from `line_ids_for_operator`/`network_line_ids`, both of
//! which exclude custom lines unconditionally -- see this plan's Judgment
//! Call 7. Nested under `/public` (`routes::public_router()`), unlike
//! `line_status.rs`'s TfL-shape-compatible routes, since there is no TfL
//! API compatibility concern for a wholly new surface.
//!
//! No Timeline-equivalent, no coverage-stats sibling -- both are
//! deliberate Non-goals of the plan above (spec §D.3; coverage tables
//! have no real producer yet at any scope).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router as AxumRouter};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::{operators, queries};
use crate::routes::line_status::{daily_stats_to_json, half_hourly_stats_to_json, sub_daily_stats_to_json};

pub fn router() -> Router {
    AxumRouter::new()
        .route(
            "/operators/{code}/stats/{from}/to/{to}",
            axum::routing::get(get_operator_daily_stats),
        )
        .route(
            "/operators/{code}/stats/half-hourly/{from}/to/{to}",
            axum::routing::get(get_operator_half_hourly_stats),
        )
        .route(
            "/operators/{code}/stats/hourly/{from}/to/{to}",
            axum::routing::get(get_operator_hourly_stats),
        )
        .route(
            "/operators/{code}/stats/six-hourly/{from}/to/{to}",
            axum::routing::get(get_operator_six_hourly_stats),
        )
        .route(
            "/network/stats/{from}/to/{to}",
            axum::routing::get(get_network_daily_stats),
        )
        .route(
            "/network/stats/half-hourly/{from}/to/{to}",
            axum::routing::get(get_network_half_hourly_stats),
        )
        .route(
            "/network/stats/hourly/{from}/to/{to}",
            axum::routing::get(get_network_hourly_stats),
        )
        .route(
            "/network/stats/six-hourly/{from}/to/{to}",
            axum::routing::get(get_network_six_hourly_stats),
        )
}

/// Every catalogue (National Rail) line id -- the "network" scope's own
/// line-id set. Deliberately NOT `queries::tfl_line_summaries` -- TfL
/// lines never accrue rows in `line_status_daily_stats`/
/// `line_status_half_hourly_stats` at all (see this plan's Judgment Call
/// 5: the aggregator's own `record_daily_stats`/`record_half_hourly_stats`
/// pass only ever iterates catalogue + custom lines, never TfL), so
/// including `tfl-`-prefixed ids here would add ids to the `= ANY($1)`
/// list that can never match a row -- harmless, but pointless. Pure and
/// synchronous so it's unit-testable without a database.
fn network_line_ids(catalogue_lines: &[common::LineDefinition]) -> Vec<String> {
    catalogue_lines.iter().map(|line| line.id.clone()).collect()
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "operator/network history query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

async fn get_operator_daily_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::line_ids_for_operator(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let rows = queries::daily_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}

async fn get_operator_half_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::line_ids_for_operator(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let rows = queries::half_hourly_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(half_hourly_stats_to_json).collect(),
    ))
}

async fn get_operator_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::line_ids_for_operator(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}

async fn get_operator_six_hourly_stats(
    State(app): State<App>,
    Path((code, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = operators::line_ids_for_operator(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}

async fn get_network_daily_stats(
    State(app): State<App>,
    Path((from, to)): Path<(chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::daily_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}

async fn get_network_half_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::half_hourly_stats_for_range_multi(&app.database, &line_ids, from, to)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(half_hourly_stats_to_json).collect(),
    ))
}

async fn get_network_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}

async fn get_network_six_hourly_stats(
    State(app): State<App>,
    Path((from, to)): Path<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let line_ids = network_line_ids(&app.config.lines);
    let rows = queries::sub_daily_stats_for_range_multi(&app.database, &line_ids, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_line_ids_lists_every_catalogue_line_and_nothing_else() {
        let lines = vec![
            common::LineDefinition {
                id: "a".to_string(),
                name: "A".to_string(),
                mode: "national-rail".to_string(),
                category: "main-line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec![],
                sample_stations: vec![],
                match_keywords: vec![],
                excluded_keywords: vec![],
                severity_overrides: Default::default(),
                exclusive_segments: vec![],
                destination_crs_filter: vec![],
            },
            common::LineDefinition {
                id: "b".to_string(),
                name: "B".to_string(),
                mode: "national-rail".to_string(),
                category: "main-line".to_string(),
                operators: vec!["GW".to_string()],
                stations: vec![],
                sample_stations: vec![],
                match_keywords: vec![],
                excluded_keywords: vec![],
                severity_overrides: Default::default(),
                exclusive_segments: vec![],
                destination_crs_filter: vec![],
            },
        ];
        assert_eq!(
            network_line_ids(&lines),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    // Fields copied verbatim from line_status.rs's own db_tests::test_app
    // (read directly while researching this plan, crates/api/src/routes/line_status.rs:1372-1439)
    // -- every placeholder inert except `lines`, which each test supplies.
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new().merge(super::router()).with_state(app)
    }

    fn catalogue_line(id: &str, operators: &[&str]) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: format!("Test {id}"),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            stations: vec![],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
        }
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_operator_daily_stats -- --ignored --test-threads=1`"]
    async fn get_operator_daily_stats_sums_only_that_operators_lines() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-ROUTE-OP-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-ROUTE-OP-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let lines = vec![
            catalogue_line("TEST-ROUTE-OP-A", &["SW"]),
            catalogue_line("TEST-ROUTE-OP-B", &["SW"]),
        ];
        let router = test_router(test_app(pool.clone(), lines));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/SW/stats/2026-08-01/to/2026-08-01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let rows = json.as_array().expect("array response");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["total"], 180);
        assert_eq!(rows[0]["delayed"], 8);

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-ROUTE-OP-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_operator_daily_stats_an_unknown_operator -- --ignored --test-threads=1`"]
    async fn get_operator_daily_stats_an_unknown_operator_code_is_200_empty_not_404() {
        let pool = connect().await;
        let router = test_router(test_app(pool, vec![catalogue_line("TEST-ROUTE-OP-C", &["SW"])]));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/operators/NOSUCHCODE/stats/2026-08-01/to/2026-08-01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "an unknown operator code is an empty result, not a 404 -- matching \
             daily_stats_for_range's own unknown-line_id convention"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_network_daily_stats -- --ignored --test-threads=1`"]
    async fn get_network_daily_stats_sums_every_catalogue_line() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-ROUTE-NET-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-ROUTE-NET-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let lines = vec![
            catalogue_line("TEST-ROUTE-NET-A", &["SW"]),
            catalogue_line("TEST-ROUTE-NET-B", &["GW"]),
        ];
        let router = test_router(test_app(pool.clone(), lines));

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/network/stats/2026-08-01/to/2026-08-01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let rows = json.as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["total"], 180, "both catalogue lines summed, regardless of operator");

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-ROUTE-NET-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");
    }
}
```

- [ ] **Step 2: Register the module and mount its router.** In
  `crates/api/src/routes/mod.rs`: add `pub mod operator_history;`
  alphabetically (between `pub mod notifications;` and `pub mod
  preferences;`), and add `.merge(operator_history::router())` to
  `public_router()`'s builder chain (alongside the other `.merge(...)`
  calls, e.g. directly after `.merge(lines::router())`).

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api --lib operator_history::tests::
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api \
  -- --ignored --test-threads=1 get_operator_daily_stats get_network_daily_stats
```

  Expected: builds and lints clean; `network_line_ids`'s pure unit test
  passes with no database; all three DB-gated route tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/operator_history.rs crates/api/src/routes/mod.rs
git commit -m "api: add GET /public/operators/{code}/stats/... and /public/network/stats/... routes"
```

---

## Task 5: Frontend fetch functions and type aliases

**Files:** modify `frontend/lib/api.ts`, `frontend/lib/types.ts`.

Depends on Task 4 (the real routes must exist for these to have anything
to call, though the code itself compiles independently).

- [ ] **Step 1: Add four type aliases** in `frontend/lib/types.ts`,
  directly after `LineSixHourlyStats`'s closing brace (`types.ts:292`) —
  no new fields, since these response shapes carry nothing line-specific
  (verified: none of the four existing interfaces has a `lineId` member):

```typescript
/** `GET /public/operators/{code}/stats/...` and `GET /public/network/stats/...`
 * share the exact same per-bucket response shape the per-line routes
 * already use -- `LineDailyStats` etc. carry no line-specific field, so
 * these are plain aliases for readability at the new call sites, not new
 * structural types. See
 * docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md. */
export type OperatorDailyStats = LineDailyStats;
export type OperatorHalfHourlyStats = LineHalfHourlyStats;
export type OperatorHourlyStats = LineHourlyStats;
export type OperatorSixHourlyStats = LineSixHourlyStats;
export type NetworkDailyStats = LineDailyStats;
export type NetworkHalfHourlyStats = LineHalfHourlyStats;
export type NetworkHourlyStats = LineHourlyStats;
export type NetworkSixHourlyStats = LineSixHourlyStats;
```

- [ ] **Step 2: Add eight fetch functions** in `frontend/lib/api.ts`,
  directly after `getLineHalfHourlyCoverageStats` (`api.ts:282`) — same
  `fetchJson`/`cache: 'no-store'` shape as the per-line functions, but
  deliberately WITHOUT `cookieForwardInit()` (Judgment Call 7: these
  routes are genuinely public, matching `getHistoryRetention`/
  `getDataFreshness`'s own precedent, not `getLineDailyStats`'s):

```typescript
import type {
  // ...(existing imports)...
  OperatorDailyStats,
  OperatorHalfHourlyStats,
  OperatorHourlyStats,
  OperatorSixHourlyStats,
  NetworkDailyStats,
  NetworkHalfHourlyStats,
  NetworkHourlyStats,
  NetworkSixHourlyStats,
} from './types';

/** `GET /public/operators/{code}/stats/{from}/to/{to}` -- the operator-scoped
 * daily Trends rollup (Phase 4). Public, unauthenticated -- no
 * `cookieForwardInit()`, matching `getHistoryRetention`/`getDataFreshness`'s
 * own precedent for a genuinely public endpoint, unlike the per-line
 * `getLineDailyStats` family (which forwards cookies because a `custom-`
 * id might be in play; this route's line-id set never includes one). */
export async function getOperatorDailyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorDailyStats[]> {
  return fetchJson<OperatorDailyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}

/** Half-hourly sibling of `getOperatorDailyStats` -- `from`/`to` are RFC3339
 * instants, same reasoning as `getLineHalfHourlyStats`. */
export async function getOperatorHalfHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorHalfHourlyStats[]> {
  return fetchJson<OperatorHalfHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/half-hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}

/** 1-hour sub-daily sibling, mirrors `getLineHourlyStats`. */
export async function getOperatorHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorHourlyStats[]> {
  return fetchJson<OperatorHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}

/** 6-hour sub-daily sibling, mirrors `getLineSixHourlyStats`. */
export async function getOperatorSixHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorSixHourlyStats[]> {
  return fetchJson<OperatorSixHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/six-hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}

/** `GET /public/network/stats/{from}/to/{to}` -- the whole-network
 * (catalogue National Rail lines only -- see this plan's Judgment Call 5
 * for why TfL lines never contribute) daily Trends rollup. */
export async function getNetworkDailyStats(from: string, to: string): Promise<NetworkDailyStats[]> {
  return fetchJson<NetworkDailyStats[]>(`${baseUrl()}/public/network/stats/${from}/to/${to}`, {
    cache: 'no-store',
  });
}

export async function getNetworkHalfHourlyStats(
  from: string,
  to: string,
): Promise<NetworkHalfHourlyStats[]> {
  return fetchJson<NetworkHalfHourlyStats[]>(
    `${baseUrl()}/public/network/stats/half-hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}

export async function getNetworkHourlyStats(from: string, to: string): Promise<NetworkHourlyStats[]> {
  return fetchJson<NetworkHourlyStats[]>(`${baseUrl()}/public/network/stats/hourly/${from}/to/${to}`, {
    cache: 'no-store',
  });
}

export async function getNetworkSixHourlyStats(
  from: string,
  to: string,
): Promise<NetworkSixHourlyStats[]> {
  return fetchJson<NetworkSixHourlyStats[]>(
    `${baseUrl()}/public/network/stats/six-hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: no new type errors (these are additive exports; nothing
  existing references them yet until Tasks 7–8).

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/types.ts
git commit -m "frontend: add operator/network Trends fetch functions and type aliases"
```

---

## Task 6: Generalize `GranularityControl.tsx`/`HistoryRangePicker.tsx` to a `basePath` prop

**Files:** modify
`frontend/app/lines/[id]/history/GranularityControl.tsx`,
`frontend/app/lines/[id]/history/HistoryRangePicker.tsx`,
`frontend/app/lines/[id]/history/TrendsResults.tsx`,
`frontend/app/lines/[id]/history/page.tsx`.

Resolves Judgment Calls 2 and 3. Independent of Tasks 1–5; can be done any
time, but must land before Tasks 7–8 (which consume the widened props/
exports).

- [ ] **Step 1: `GranularityControl.tsx`** — rename the `lineId: string`
  prop to `basePath: string` and update `handleChange`'s `router.push`
  call:

```typescript
export function GranularityControl({
  basePath,
  preset,
  from,
  to,
  granularity,
  available,
}: {
  basePath: string;
  preset: RangePreset | null;
  from: string;
  to: string;
  granularity: TrendGranularity;
  available: TrendGranularity[];
}) {
  const router = useRouter();
  const unavailable = DISPLAY_ORDER.filter((g) => !available.includes(g));

  function handleChange(value: string) {
    const rangeParams = preset ? `range=${preset}` : `from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`;
    router.push(`${basePath}?${rangeParams}&granularity=${value}`);
  }
  // ...rest unchanged...
```

- [ ] **Step 2: `HistoryRangePicker.tsx`** — same rename, applied to both
  navigation call sites (`handleSearch`, `handlePreset`):

```typescript
export function HistoryRangePicker({
  basePath,
  preset,
  from,
  to,
}: {
  basePath: string;
  preset: RangePreset | null;
  from: string;
  to: string;
}) {
  // ...unchanged state/effects...

  function handleSearch() {
    const [start, end] = value;
    if (!start || !end) return;
    router.push(`${basePath}?from=${new Date(start).toISOString()}&to=${new Date(end).toISOString()}`);
  }

  function handlePreset(next: RangePreset) {
    router.push(`${basePath}?range=${next}`);
  }
  // ...rest unchanged...
```

- [ ] **Step 3: Update `/lines/[id]/history/page.tsx`'s two call sites**
  (`page.tsx:112`, `page.tsx:191-198`) to pass `basePath` instead of
  `lineId` — the only behavior-preserving choice, since this page's own
  base path (`` `/lines/${id}/history` ``) is exactly what `lineId={id}`
  used to reconstruct implicitly inside each component:

```typescript
      <HistoryRangePicker basePath={`/lines/${id}/history`} preset={range.preset} from={range.from} to={range.to} />
```

  and

```typescript
            <GranularityControl
              basePath={`/lines/${id}/history`}
              preset={range.preset}
              from={range.from}
              to={range.to}
              granularity={granularity}
              available={available}
            />
```

- [ ] **Step 4: Export `TrendsResults.tsx`'s existing `SPARSE_FLOOR`/
  `HONESTY_COPY` consts** (currently module-private, `TrendsResults.tsx`
  lines defining `const SPARSE_FLOOR: Record<TrendGranularity, number> = {...}`
  and `const HONESTY_COPY: Record<TrendGranularity, string> = {...}`) —
  add the `export` keyword to both declarations, no other change:

```typescript
export const SPARSE_FLOOR: Record<TrendGranularity, number> = {
  halfHour: 10,
  hour: 20,
  sixHour: 120,
  day: 20,
};

export const HONESTY_COPY: Record<TrendGranularity, string> = {
  // ...unchanged text...
};
```

  (`toChartPoints` is already `export`ed — no change needed there.)

- [ ] **Step 5: Verify — automated**

```bash
cd frontend && npx tsc --noEmit
npm test
npm run build
```

  Expected: no new type errors; existing test suites for
  `GranularityControl`/`HistoryRangePicker`/`TrendsResults` (if any exist
  — check with `grep -rln "GranularityControl\|HistoryRangePicker" frontend/**/*.test.tsx`)
  still pass, since every call site was updated in the same task; clean
  build.

- [ ] **Step 6: Verify — manual, in a real browser (this is the task that
  could regress an already-shipped page, so verify it directly rather than
  trusting the type-checker alone).** Start the dev stack, open
  `/lines/{any-line-id}/history`, and confirm: the Timeline/Trends tabs
  render exactly as before; clicking a "7 days"/"30 days" preset still
  navigates within `/lines/{id}/history`; picking a custom date range via
  "Custom…" then "Show history" still works; switching the Trends tab's
  granularity control still navigates within `/lines/{id}/history` and
  preserves the current range. None of this should look or behave any
  differently than before this task.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/lines/\[id\]/history/GranularityControl.tsx \
        frontend/app/lines/\[id\]/history/HistoryRangePicker.tsx \
        frontend/app/lines/\[id\]/history/TrendsResults.tsx \
        frontend/app/lines/\[id\]/history/page.tsx
git commit -m "frontend: generalize GranularityControl/HistoryRangePicker to a basePath prop for reuse beyond /lines/[id]/history"
```

---

## Task 7: `/operators/[code]/history` page

**Files:** create `frontend/app/operators/[code]/history/page.tsx`,
`frontend/app/operators/[code]/history/OperatorTrendsResults.tsx`.

Depends on Tasks 5 (fetch functions) and 6 (`basePath`, exported
`SPARSE_FLOOR`/`HONESTY_COPY`/`toChartPoints`).

- [ ] **Step 1: `OperatorTrendsResults.tsx`** — structurally parallel to
  `TrendsResults.tsx` (Judgment Call 3), reusing its exports rather than
  duplicating the floors/copy:

```typescript
import { Paper, Stack, Text } from '@mantine/core';
import {
  getOperatorDailyStats,
  getOperatorHalfHourlyStats,
  getOperatorHourlyStats,
  getOperatorSixHourlyStats,
} from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { HONESTY_COPY, SPARSE_FLOOR, toChartPoints } from '../../../lines/[id]/history/TrendsResults';
import { TrendsCharts } from '../../../lines/[id]/history/TrendsCharts';
import type { ChartPoint } from '../../../lines/[id]/history/chartPoint';

// Dispatches to the right fetch + floor + bucket-key field for the
// selected tier -- operator-scoped sibling of TrendsResults.tsx's own
// fetchPoints, reusing its SPARSE_FLOOR/toChartPoints (Judgment Call 3 of
// docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md)
// rather than re-deriving them.
async function fetchPoints(
  code: string,
  granularity: TrendGranularity,
  from: string,
  to: string,
): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getOperatorDailyStats(code, londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getOperatorHalfHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getOperatorHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getOperatorSixHourlyStats(code, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

/** Operator-scoped sibling of `TrendsResults` -- same fetch/error/empty-state
 * shape, `code` in place of `id`. See this plan's Judgment Call 3 for why
 * this is a new sibling component, not a `scope`-branching rewrite of
 * `TrendsResults` itself. */
export async function OperatorTrendsResults({
  code,
  from,
  to,
  granularity = 'day',
}: {
  code: string;
  from: string;
  to: string;
  granularity?: TrendGranularity;
}) {
  let points: ChartPoint[];
  try {
    points = await fetchPoints(code, granularity, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (points.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">
          Not enough sampled data yet for this operator. If this operator&apos;s lines are TfL-operated, this
          may never populate -- TfL lines don&apos;t currently feed this rollup (see Judgment Call 5 of
          docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md).
        </Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        {HONESTY_COPY[granularity]} Rates shown are summed across every line this operator runs (excluding
        any private custom lines, which never appear in a public rollup).
      </Text>
      <TrendsCharts points={points} granularity={granularity} order={2} showVolume />
    </Stack>
  );
}
```

  **Note on relative import depth**: `frontend/app/operators/[code]/history/`
  is four segments below `frontend/app/` — `../../../lines/[id]/history/TrendsResults`
  resolves to `frontend/app/lines/[id]/history/TrendsResults`, confirm this
  path is correct for the actual file once created (adjust the `../` count
  if the directory nesting above doesn't match exactly what Next.js
  materializes for a bracket-segment route — verify with
  `npx tsc --noEmit` in Step 4 below, which will fail loudly on a wrong
  path rather than silently misresolving).

- [ ] **Step 2: `page.tsx`** — structurally parallel to
  `/lines/[id]/history/page.tsx`, but with no Timeline tab (Non-goals) and
  therefore no `Tabs` wrapper at all — just the range picker, granularity
  control, and one Trends section:

```typescript
import { Suspense } from 'react';
import { Alert, Skeleton, Stack, Text, Title } from '@mantine/core';
import { getAllTocs, getHistoryRetention } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import {
  availableGranularities,
  granularityShortfallDays,
  resolveGranularity,
  resolveRange,
} from '@/lib/history';
import { GranularityControl } from '@/app/lines/[id]/history/GranularityControl';
import { HistoryRangePicker } from '@/app/lines/[id]/history/HistoryRangePicker';
import { OperatorTrendsResults } from './OperatorTrendsResults';

export const revalidate = 0;

/** "TfL" has no `tocs` row (it's a synthetic operator tag, not a real
 * ATOC code -- see spec Open Question 2) -- special-cased the same literal
 * way `AllLinesTable.tsx` already does, since there is no Rust->TypeScript
 * constant bridge to import `common::TFL_OPERATOR` from here. */
async function resolveOperatorName(code: string): Promise<string> {
  if (code === 'TfL') return 'TfL';
  try {
    const tocs = await getAllTocs();
    return tocs.find((toc) => toc.code === code)?.name ?? code;
  } catch (err) {
    console.warn(`Could not resolve a name for operator "${code}"; falling back to the code.`, err);
    return code;
  }
}

async function resolveRetention(): Promise<{
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}> {
  try {
    const retention = await getHistoryRetention();
    return {
      dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
      halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
    };
  } catch (err) {
    console.warn('Could not resolve retention ceilings; offering only Daily.', err);
    return { dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 };
  }
}

export default async function OperatorHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ code: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const { code } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, ceilings] = await Promise.all([resolveOperatorName(code), resolveRetention()]);
  const range = resolveRange(query, now);
  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? ceilings.dailyStatsRetentionDays : Math.floor(ceilings.halfHourlyStatsRetentionHours / 24);
  const basePath = `/operators/${code}/history`;

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/operators/${code}`} underline="always">
        Back to operator
      </TextLink>
      <Title order={1}>History: {name}</Title>
      <HistoryRangePicker basePath={basePath} preset={range.preset} from={range.from} to={range.to} />
      <GranularityControl
        basePath={basePath}
        preset={range.preset}
        from={range.from}
        to={range.to}
        granularity={granularity}
        available={available}
      />
      {granularityShortfall !== null && (
        <Alert color="yellow" variant="light" title="Some of this range isn't available at this granularity">
          This server only keeps {retentionDaysForGranularity}{' '}
          {retentionDaysForGranularity === 1 ? 'day' : 'days'} of data at this granularity. The oldest{' '}
          {granularityShortfall} {granularityShortfall === 1 ? 'day' : 'days'} of the range you picked has
          already been removed -- if this range looks empty or short, that may be why, not because nothing
          happened.
        </Alert>
      )}
      <Suspense
        key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
        fallback={<Skeleton height={320} />}
      >
        <OperatorTrendsResults code={code} from={range.from} to={range.to} granularity={granularity} />
      </Suspense>
    </Stack>
  );
}
```

  **Note**: `<TextLink href={`/operators/${code}`}>` points at Phase 3's
  operator detail page, which may not exist yet — this is a forward link
  to a route this plan does not create; it 404s harmlessly until Phase 3
  lands, matching this plan's Non-goals ("does not depend on ... Phase 3's
  `/operators` list page existing").

- [ ] **Step 3: Verify — automated**

```bash
cd frontend && npx tsc --noEmit
npm run build
```

  Expected: no type errors (in particular, confirm the relative import
  paths in `OperatorTrendsResults.tsx` resolve); clean build. `npm run
  build` will attempt to prerender/analyze this new route — if it fails
  because `/operators/{code}` doesn't exist as a dynamic segment sibling
  yet, that's fine (Next.js doesn't require a sibling route to exist), but
  confirm the build actually succeeds before moving on.

- [ ] **Step 4: Verify — manual, in a real browser.** Start the dev stack,
  navigate directly to `/operators/SW/history` (or any real ATOC code
  this deployment's catalogue lines carry — check `lines/*.toml`'s
  `operators` arrays, or query `/public/operators/SW/stats/...` directly
  with `curl` first to confirm data exists for a chosen code). Confirm:
  the range picker and granularity control render and navigate correctly
  within `/operators/{code}/history`; the Trends chart renders with real
  data for a code with real catalogue lines; a made-up code (e.g.
  `/operators/NOSUCHCODE/history`) renders the "not enough sampled data"
  empty state rather than an error page.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/operators
git commit -m "frontend: add /operators/[code]/history Trends page"
```

---

## Task 8: `/network/history` page

**Files:** create `frontend/app/network/history/page.tsx`,
`frontend/app/network/history/NetworkTrendsResults.tsx`.

Depends on Tasks 5 and 6, same as Task 7. Independent of Task 7 itself
(no shared new files) — can be done in parallel if desired.

- [ ] **Step 1: `NetworkTrendsResults.tsx`** — same shape as
  `OperatorTrendsResults.tsx`, no `code`/`id` parameter at all:

```typescript
import { Paper, Stack, Text } from '@mantine/core';
import {
  getNetworkDailyStats,
  getNetworkHalfHourlyStats,
  getNetworkHourlyStats,
  getNetworkSixHourlyStats,
} from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { HONESTY_COPY, SPARSE_FLOOR, toChartPoints } from '../../lines/[id]/history/TrendsResults';
import { TrendsCharts } from '../../lines/[id]/history/TrendsCharts';
import type { ChartPoint } from '../../lines/[id]/history/chartPoint';

async function fetchPoints(granularity: TrendGranularity, from: string, to: string): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getNetworkDailyStats(londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getNetworkHalfHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getNetworkHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getNetworkSixHourlyStats(from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

/** Network-scoped sibling of `TrendsResults`/`OperatorTrendsResults` --
 * every catalogue (National Rail) line, summed. See this plan's Judgment
 * Call 5: TfL lines never contribute (they carry no rows in the
 * underlying rollup tables at all), so this is honestly a National Rail
 * network view, not literally "every mode." */
export async function NetworkTrendsResults({
  from,
  to,
  granularity = 'day',
}: {
  from: string;
  to: string;
  granularity?: TrendGranularity;
}) {
  let points: ChartPoint[];
  try {
    points = await fetchPoints(granularity, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (points.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet across the network.</Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        {HONESTY_COPY[granularity]} Rates shown are summed across every National Rail catalogue line (TfL
        lines aren&apos;t currently part of this rollup).
      </Text>
      <TrendsCharts points={points} granularity={granularity} order={2} showVolume />
    </Stack>
  );
}
```

- [ ] **Step 2: `page.tsx`** — same shape as Task 7's, minus the `code`
  param and operator-name resolution:

```typescript
import { Suspense } from 'react';
import { Alert, Skeleton, Stack, Title } from '@mantine/core';
import { getHistoryRetention } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import {
  availableGranularities,
  granularityShortfallDays,
  resolveGranularity,
  resolveRange,
} from '@/lib/history';
import { GranularityControl } from '@/app/lines/[id]/history/GranularityControl';
import { HistoryRangePicker } from '@/app/lines/[id]/history/HistoryRangePicker';
import { NetworkTrendsResults } from './NetworkTrendsResults';

export const revalidate = 0;

async function resolveRetention(): Promise<{
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}> {
  try {
    const retention = await getHistoryRetention();
    return {
      dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
      halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
    };
  } catch (err) {
    console.warn('Could not resolve retention ceilings; offering only Daily.', err);
    return { dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 };
  }
}

export default async function NetworkHistoryPage({
  searchParams,
}: {
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const query = await searchParams;
  const now = Date.now();
  const ceilings = await resolveRetention();
  const range = resolveRange(query, now);
  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? ceilings.dailyStatsRetentionDays : Math.floor(ceilings.halfHourlyStatsRetentionHours / 24);
  const basePath = '/network/history';

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/lines" underline="always">
        Back to all lines
      </TextLink>
      <Title order={1}>Network history</Title>
      <HistoryRangePicker basePath={basePath} preset={range.preset} from={range.from} to={range.to} />
      <GranularityControl
        basePath={basePath}
        preset={range.preset}
        from={range.from}
        to={range.to}
        granularity={granularity}
        available={available}
      />
      {granularityShortfall !== null && (
        <Alert color="yellow" variant="light" title="Some of this range isn't available at this granularity">
          This server only keeps {retentionDaysForGranularity}{' '}
          {retentionDaysForGranularity === 1 ? 'day' : 'days'} of data at this granularity. The oldest{' '}
          {granularityShortfall} {granularityShortfall === 1 ? 'day' : 'days'} of the range you picked has
          already been removed -- if this range looks empty or short, that may be why, not because nothing
          happened.
        </Alert>
      )}
      <Suspense
        key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
        fallback={<Skeleton height={320} />}
      >
        <NetworkTrendsResults from={range.from} to={range.to} granularity={granularity} />
      </Suspense>
    </Stack>
  );
}
```

- [ ] **Step 3: Verify — automated**

```bash
cd frontend && npx tsc --noEmit
npm run build
```

- [ ] **Step 4: Verify — manual, in a real browser.** Navigate to
  `/network/history`; confirm the picker/granularity control work and the
  chart renders real, non-empty data (this deployment's catalogue almost
  certainly has recent daily-stats rows across many lines, so unlike a
  single made-up operator code, this should show real data immediately).

- [ ] **Step 5: Commit**

```bash
git add frontend/app/network
git commit -m "frontend: add /network/history Trends page"
```

---

## Task 9: End-to-end verification and best-effort entry-point wiring

**Files:** none required; may touch a Phase-1/Phase-3 file **only if it
already exists** (see Step 3's guardrail).

- [ ] **Step 1: Full verification sweep**, all of it, in order:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api -- --ignored --test-threads=1
cd frontend && npx tsc --noEmit && npm test && npm run build
```

  Expected: all green. (`cargo fmt --all --check` is `continue-on-error`
  in CI per `.github/workflows/ci.yml`'s own comment about pre-existing
  drift on `main` — run it anyway and fix anything this plan's own new
  files introduce, even though it won't block CI.)

- [ ] **Step 2: Direct API smoke test.** With a local stack running
  (`docker compose up` or equivalent), confirm all eight new routes
  respond:

```bash
curl -s "http://localhost:8080/public/operators/SW/stats/2026-08-01/to/2026-08-31" | head -c 300
curl -s "http://localhost:8080/public/network/stats/2026-08-01/to/2026-08-31" | head -c 300
```

  (substitute the real base URL/port this deployment's `docker-compose.yml`
  exposes, and a real ATOC code from this repo's `lines/*.toml` catalogue).
  Expected: a JSON array, `200`, non-error, with the same field names
  (`day`, `sampleCycles`, `total`, `delayed`, `cancelled`, `skipped`,
  `avgDelayMinutes`, `delayRate`, `cancellationRate`, `skipRate`) the
  existing per-line `/Line/{id}/Stats/...` routes already return.

- [ ] **Step 3: Best-effort entry-point wiring (non-blocking).** Check
  whether either of these already exist in the worktree at execution time:

```bash
test -f frontend/app/operators/\[code\]/page.tsx && echo "Phase 3 operator detail page exists"
test -f frontend/app/status/page.tsx && echo "Phase 1 all-lines dashboard exists"
```

  - If `frontend/app/operators/[code]/page.tsx` exists, add one
    `<TextLink href={`/operators/${code}/history`}>View history</TextLink>`
    (or equivalent, matching that page's own component conventions) to it.
  - If `frontend/app/status/page.tsx` exists, add one link to
    `/network/history` from it (e.g. under its five-bucket summary strip).
  - **If neither exists yet, do nothing here** — this plan's two new pages
    are already directly URL-reachable and fully functional without a
    link from either; this step is a convenience, not a completion
    requirement (Non-goals: "does not depend on ... existing"). Do not
    create either Phase 1/3 file just to have somewhere to put a link.

- [ ] **Step 4: If Step 3 touched a file, commit it separately**

```bash
git add <whatever Step 3 touched, if anything>
git commit -m "frontend: link to operator/network history from <wherever Step 3 wired it in>"
```

---

## Self-review (spec coverage)

- Spec §D.1/D.2 ("extend, don't replace, `TrendsCharts.tsx`'s pattern...
  the gap is the aggregation granularity, not the charting") → Tasks 2–4
  (backend cross-line aggregation), Tasks 7–8 (frontend reuse of
  `TrendsCharts`/`toChartPoints` unmodified in substance).
- Spec §D.2's "compute-on-read, no new tables" recommendation → Task 2;
  Judgment Call 1 additionally resolves the spec's own flagged-unverified
  index/performance question in favor of compute-on-read being correct
  indefinitely at current scale, not just as a stopgap.
- Spec §D.3 ("do not build an operator/network Timeline-equivalent") →
  Non-goals, restated; no task touches `line_status_history`.
- Spec §D.4 ("reuse the existing retention echo, no new retention
  concept") → Tasks 7–8 call the existing, unmodified `getHistoryRetention()`;
  no change to `crates/api/src/routes/history_retention.rs`.
- Spec's visualization recommendation ("reuse `TrendsCharts.tsx` verbatim
  ... do not build a new chart component") → `TrendsCharts.tsx` is never
  edited by any task; Judgment Call 2 corrects the "unmodified" framing
  for its two sibling nav components with the smallest fix that preserves
  this intent.
- Spec Open Question 1 (custom lines excluded from public operator
  rollups) → the external dependency's own contract (top of document) and
  Judgment Call 7.
- Spec Open Question 3 (operator/network history should be public,
  unauthenticated) → Judgment Call 7, Task 4.
- Spec Open Question 4 (same range-picker presets as the per-line page) →
  Tasks 7–8 reuse `HistoryRangePicker`/`resolveRange` unchanged in
  substance.
- Spec Open Question 5 (precompute vs. compute-on-read) → Judgment Call 1,
  resolved in favor of compute-on-read.
- The plan's own external-dependency requirement → Task 1, with a
  self-contained fallback implementation so Phase 3's landing order never
  blocks this plan.
