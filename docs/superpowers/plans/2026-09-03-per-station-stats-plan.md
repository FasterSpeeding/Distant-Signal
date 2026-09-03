# Per-Station Delay/Cancellation Stats — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1–4 are backend, land in order (each depends on the previous
> one's code existing).** **Task 5 is frontend, depends on Task 4's route
> being live.** Do not start Task 5 against a route that hasn't shipped —
> there is no mock/stub layer in this app's frontend tests for a route
> that doesn't exist server-side yet (see `frontend/app/stations/[crs]/page.test.tsx`'s
> existing `vi.mock('@/lib/api', …)` pattern, which mocks real exported
> functions).

**Goal:** implement
`docs/superpowers/specs/2026-09-03-per-station-stats-design.md` end to
end — a new `GET /public/stations/{crs}/sample-stats` endpoint computing
per-(station, operator) `SampleStats` on demand from `station_samples`
(Option C, no new table, no new aggregator write path), plus a new
"Sample stats by operator" section on the station detail page. Scoped to
the ~286 CRS codes that already have live sampling; every other CRS gets
an honest "not sampled" message, not a blank or misleading result.

**Architecture:** one new shared function in `crates/common` (generalized
counting arithmetic, Decision 5), one small refactor of
`crates/aggregator`'s existing `stats_from_departures` to delegate to it
(behavior-preserving), one new pure-logic file in `crates/api/src/data/`,
one new route file in `crates/api/src/routes/`, two extracted helper
functions in `crates/api/src/render.rs`, and one new section on the
existing station detail page. No migration, no new table, no new
aggregator write path.

**Tech Stack:** Rust (axum, sqlx with runtime-checked queries — no
`cargo sqlx prepare` needed, `latest_station_sample` already exists and is
unmodified), Next.js 16 App Router + TypeScript, Vitest 2 +
`@testing-library/react` (`frontend/test/render.tsx`'s `renderWithMantine`
helper).

**Design doc:**
`docs/superpowers/specs/2026-09-03-per-station-stats-design.md` — its
Decisions section is authoritative for every type/route/wire shape below;
this plan does not repeat the reasoning, only the concrete steps.

---

## Non-goals

- **No new database table, migration, or aggregator write path.** Every
  task below reads `station_samples` at request time; nothing is ever
  written by this plan.
- **No per-station/per-operator threshold override mechanism.** Uses
  `common::Defaults::default()` only (design doc Decision 3).
- **No change to `ServiceArguments.defaults_file`'s current (unused)
  status.** Not read, not fixed, not touched by this plan.
- **No broadening of `poller-ldbws`'s polling scope.** Stays scoped to
  whatever CRS codes are already in `station_samples` today.
- **No severity/`StatusBadge`/incident interaction.** Purely
  informational, same as `LineStatus.sample_stats` today.
- **No picker, tab, or filter UI for many-operator stations.** A plain
  list of rows, capped at however many distinct operators are present
  (design doc Decision 1 — measured max around 8 for the busiest shared
  stations).
- **No rename of `sampleUnavailableReason`/`formatSampleSummary`.**
  Widened signature only (design doc Decision 9) — the separate,
  currently-unscheduled source-agnostic rename flagged by
  `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
  is not this plan's job.
- **No unfiltered "whole station" number, shown anywhere, ever, even as a
  secondary figure.** Design doc Decision 1 is final for this plan's
  scope.

## Global Constraints

- **Testing:** Rust — `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests), plus
  `DATABASE_URL=<url> cargo test -p api -p aggregator -- --ignored
  --test-threads=1` for the DB-backed tests this plan adds (mirrors
  `.github/workflows/ci.yml:215-216`'s exact CI invocation). Frontend —
  `npm test` (`frontend/package.json`'s `"test": "vitest run"`) and
  `npm run build` (`next build`) from `frontend/`.
- **File scope.** Modified: `crates/common/src/lib.rs`,
  `crates/aggregator/src/aggregation.rs`, `crates/api/src/render.rs`,
  `crates/api/src/routes/mod.rs`, `frontend/lib/types.ts`,
  `frontend/lib/api.ts`, `frontend/lib/sampleStats.ts`,
  `frontend/app/stations/[crs]/page.tsx`, plus each file's colocated
  test file. Created: `crates/api/src/data/station_stats.rs`,
  `crates/api/src/routes/station_stats.rs`, and their test modules.
- **No `internal_oauth_routes` entry needed** — the new route is
  unauthenticated, mounted via `public_router()` only (design doc
  Decision 8). Do not add anything to
  `crates/api/src/app.rs::build_internal_oauth_routes`.
- **CRS case handling:** no new normalization. Matches this codebase's
  existing convention (`has_station`, `belongs_to_line`,
  `stats_from_departures`'s `line_stations` check) of exact `==`
  comparison against already-canonical-uppercase CRS codes — do not add
  a `.to_uppercase()` call anywhere in this plan's new code that the
  existing `get_stop_point_disruption`/`latest_station_sample` call
  sites don't already have.

---

### Task 1: Promote shared counting arithmetic to `common` (Decision 5) — **backend**

**Files:**
- Modify: `crates/common/src/lib.rs`

Independent of every other task. Land first: Task 2 depends on this
function existing, and Task 3 depends on it too.

- [ ] **Step 1: Add `compute_sample_stats` to `crates/common/src/lib.rs`**

Place it directly after `thresholds_for` (`:916-928`), before the
existing `#[cfg(test)] mod defaults_tests`. Use the exact signature and
doc comment from the design doc's Decision 5 code sketch (reproduced
below for convenience — copy verbatim, do not re-derive):

```rust
/// Shared delayed/cancelled/skipped/avg-delay arithmetic underlying every
/// `SampleStats` computation in this app. `is_skip` is a caller-supplied
/// predicate rather than a fixed membership check, because "skip" means
/// two different, both legitimate things depending on the caller: the
/// line-level caller means "skips a stop somewhere on the line's route"
/// (`line.stations`); the per-(station, operator) caller
/// (docs/superpowers/specs/2026-09-03-per-station-stats-design.md
/// Decision 4) means "skips calling at this specific station"
/// (`skipped_stations.contains(this_crs)`). Only ever evaluated for a
/// non-cancelled departure, matching every existing caller.
pub fn compute_sample_stats(
    departures: &[&StationDeparture],
    delay_threshold_minutes: i64,
    is_skip: impl Fn(&StationDeparture) -> bool,
) -> SampleStats {
    let total = departures.len();
    let cancelled = departures.iter().filter(|d| d.is_cancelled).count();
    let delayed = departures
        .iter()
        .filter(|d| !d.is_cancelled && d.delay_minutes as i64 >= delay_threshold_minutes)
        .count();
    let skipped = departures
        .iter()
        .filter(|d| !d.is_cancelled && is_skip(d))
        .count();
    let running: Vec<&&StationDeparture> = departures.iter().filter(|d| !d.is_cancelled).collect();
    let avg_delay_minutes = if running.is_empty() {
        0.0
    } else {
        running.iter().map(|d| d.delay_minutes as f64).sum::<f64>() / running.len() as f64
    };

    SampleStats { total, delayed, cancelled, skipped, avg_delay_minutes }
}
```

- [ ] **Step 2: Add unit tests in `common`**

New `#[cfg(test)] mod compute_sample_stats_tests` (or extend the existing
`defaults_tests` module — pick whichever keeps `lib.rs` more readable,
your call). Cover, at minimum:

- Empty input → `total == 0`, `avg_delay_minutes == 0.0` (not NaN/panic).
- A mix of cancelled/delayed/on-time departures → correct counts, and
  `avg_delay_minutes` computed only over non-cancelled ("running") ones —
  construct a case where including a cancelled departure's
  `delay_minutes` in the average would give a different (wrong) answer,
  to prove the exclusion is real, not accidental.
- `is_skip` returning `true` for a cancelled departure's `skipped_stations`
  match does **not** count it as skipped (the `!d.is_cancelled &&` guard)
  — construct a departure that is both cancelled and has a matching
  `skipped_stations` entry, assert `skipped == 0`.
- Delay exactly at `delay_threshold_minutes` counts as delayed (`>=`, not
  `>`) — a boundary test.

- [ ] **Step 3: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p common
```

Expected: all PASS, zero clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "Add common::compute_sample_stats, the shared counting core for per-line and per-station stats"
```

---

### Task 2: Refactor `aggregator`'s `stats_from_departures` to delegate (Decision 5) — **backend**

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs`

Depends on Task 1. Behavior-preserving — no new aggregator behavior.

- [ ] **Step 1: Replace `stats_from_departures`'s body**

`crates/aggregator/src/aggregation.rs:816-844`. Keep the function
signature and its existing doc comment exactly as-is (it's still
accurate — this is an implementation swap, not a semantic change).
Replace the body:

```rust
pub(crate) fn stats_from_departures(
    departures: &[&StationDeparture],
    line: &LineDefinition,
    thresholds: &Defaults,
) -> SampleStats {
    let line_stations: HashSet<&str> = line.stations.iter().map(|s| s.crs.as_str()).collect();
    common::compute_sample_stats(departures, thresholds.delay_threshold_minutes, |d| {
        d.skipped_stations.iter().any(|crs| line_stations.contains(crs.as_str()))
    })
}
```

- [ ] **Step 2: Run the existing test suite unmodified — this is the regression check**

```bash
cargo test -p aggregator
```

Expected: every existing test in `aggregation.rs`'s `mod tests`
(`:1165` onward), including whatever directly exercises
`stats_from_departures`/`compute_sample_availability`/`infer_from_samples`,
passes **without any test-file edits**. If any test needs to change to
pass, stop — that means the refactor changed behavior, which it must
not. Diagnose and fix the refactor, not the test.

- [ ] **Step 3: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add crates/aggregator/src/aggregation.rs
git commit -m "Delegate aggregator's stats_from_departures to the new shared common::compute_sample_stats"
```

---

### Task 3: New `crates/api/src/data/station_stats.rs` (Decision 6) — **backend**

**Files:**
- Create: `crates/api/src/data/station_stats.rs`
- Modify: `crates/api/src/data/mod.rs` (add `pub mod station_stats;` — check the
  existing `pub mod` list's ordering convention, likely alphabetical, and
  match it)

Depends on Task 1 (`common::compute_sample_stats`). Independent of Task 2
(different crate, no shared code path other than the Task 1 function
both now use).

- [ ] **Step 1: Create the file with the design doc's Decision 6 code sketch verbatim**

Copy the `OperatorSampleStats`/`compute_station_operator_stats` sketch
from the design doc exactly (use the design doc as the source of truth
for the doc comments).

- [ ] **Step 2: Add unit tests (pure logic, no database — mirrors `eta_blend.rs`'s test shape)**

New `#[cfg(test)] mod tests` in the same file. Cover:

- A `StationSample` with departures from two different operators, one
  clearing `min_sample_size` and one not — assert the returned `Vec` has
  exactly two entries, in alphabetical-by-operator order, with the right
  `SampleAvailability` variant on each.
- An empty `departures` list → empty `Vec` (not a panic, not a synthetic
  entry).
- A departure whose `skipped_stations` contains `sample.crs` — assert it
  counts toward `skipped` in the `Available` case; a departure whose
  `skipped_stations` contains some *other* CRS (e.g. one on the same
  line's route but not this station) — assert it does **not** count,
  proving Decision 4's per-station (not per-route) skip definition is
  actually what's implemented, not accidentally the old per-route one.
- A cancelled departure with a matching `skipped_stations` entry — assert
  it is not double-counted as both cancelled and skipped.

- [ ] **Step 3: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
```

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/station_stats.rs crates/api/src/data/mod.rs
git commit -m "Add per-(station, operator) SampleStats computation (crates/api/src/data/station_stats.rs)"
```

---

### Task 4: Extract `render.rs` helpers, add the route (Decisions 7, 8) — **backend**

**Files:**
- Modify: `crates/api/src/render.rs`
- Create: `crates/api/src/routes/station_stats.rs`
- Modify: `crates/api/src/routes/mod.rs`

Depends on Task 3.

- [ ] **Step 1: Extract `sample_stats_json`/`sample_availability_json` from `status_to_json`**

`crates/api/src/render.rs:63-78`. Pull the two inline blocks out into the
two `pub(crate) fn`s from the design doc's Decision 7 sketch, and have
`status_to_json` call them in place of the inline `json!`/`match`. Keep
`status_to_json`'s own behavior byte-for-byte identical — this is a pure
extraction.

- [ ] **Step 2: Run `render.rs`'s existing tests unmodified — the regression check**

```bash
cargo test -p api render
```

Expected: `sample_stats_included_when_present`,
`sample_stats_omitted_when_absent`,
`sample_availability_is_always_present_unlike_sample_stats`,
`sample_availability_below_threshold_shape`,
`sample_availability_available_case_does_not_duplicate_sample_stats_fields`
(`render.rs:228-289`, exact names may differ slightly — check the file)
all pass unmodified. If any needs a change, the extraction changed
output; fix the extraction.

- [ ] **Step 3: Create `crates/api/src/routes/station_stats.rs`**

Copy the design doc's Decision 7 route sketch verbatim (router registers
`GET /stations/{crs}/sample-stats`, handler 404s on no row, builds `Vec<Value>`
via the two extracted helpers).

- [ ] **Step 4: Register the route in `public_router()`**

`crates/api/src/routes/mod.rs`: add `pub mod station_stats;` to the
existing `pub mod` list (match its ordering — likely alphabetical, so
between `reference` and… check exact neighbors), and
`.merge(station_stats::router())` inside `public_router()`
(`:22-53`)'s existing chain. Do **not** add anything to
`private_router()` or `build_internal_oauth_routes` — this route is
public (design doc Decision 8).

- [ ] **Step 5: Add route-level tests**

Two kinds, following this crate's established split:

1. **Unit-level, no DB**: a `tower::ServiceExt::oneshot` probe against a
   throwaway router (mirrors `crates/api/src/routes/line_status.rs:646-684`'s
   own probe pattern) asserting the route path
   `/stations/{crs}/sample-stats` actually parses and dispatches — cheap
   insurance the exact path-segment syntax is right, same reasoning as
   that file's own module-doc-documented precedent for a similarly
   shaped concern.
2. **DB-backed, `#[ignore]`-gated**: new `#[tokio::test] #[ignore = "requires
   a live database; run with `cargo test -p api
   station_sample_stats -- --ignored`"]` tests in
   `station_stats.rs`'s route module (or a `db_tests` submodule,
   matching `crates/api/src/data/custom_lines.rs:321-380`'s pattern —
   seed, assert, delete). Cover:
   - No row in `station_samples` for a fixture CRS → `404`, with the CRS
     named in the message.
   - A row present with a `Z…`-namespaced fixture CRS (same reserved
     namespace convention as `docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md`
     Task 1's fixtures, so cleanup can't touch real data) and empty
     `departures` → `200 []`.
   - A row with departures from two operators → `200`, two entries, in
     alphabetical order, correct `sampleAvailability`/`sampleStats`
     shape on the wire (assert exact JSON, not just status code — this
     is the one place the camelCase-nesting fix from Decision 7 actually
     gets proven end to end).
   Delete the fixture row(s) unconditionally at the end of each test.

- [ ] **Step 6: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api station_sample_stats -- --ignored --test-threads=1
```

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/render.rs crates/api/src/routes/station_stats.rs crates/api/src/routes/mod.rs
git commit -m "Add GET /public/stations/{crs}/sample-stats endpoint"
```

---

### Task 5: Frontend — station page "Sample stats by operator" section (Decision 9) — **frontend only**

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/sampleStats.ts`
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Modify (colocated tests): `frontend/lib/sampleStats.test.ts`,
  `frontend/app/stations/[crs]/page.test.tsx`

Depends on Task 4's route existing and its exact wire shape being final
(this task's mocks and expected JSON shapes come directly from Task 4's
DB-backed test assertions — do not start this task until Task 4 is
merged/landed, since a frontend test asserting against a wire shape that
later changes is wasted work).

- [ ] **Step 1: Add `StationOperatorSampleStats` to `frontend/lib/types.ts`**

Per design doc Decision 9's sketch, next to the existing
`SampleStats`/`SampleAvailability`/`LineStatus` types (`:60-92`).

- [ ] **Step 2: Add `getStationSampleStats` to `frontend/lib/api.ts`**

Per design doc Decision 9's sketch — `no-store`, same pattern as
`getStopPointDisruption` (`api.ts:99-102`).

- [ ] **Step 3: Add a unit test for `getStationSampleStats`**

`frontend/lib/api.test.ts` — follow whatever pattern the existing
`getStopPointDisruption` test uses in the same file (mock `fetch`, assert
URL and options).

- [ ] **Step 4: Widen `sampleUnavailableReason`/`formatSampleSummary`'s parameter type**

`frontend/lib/sampleStats.ts:34-52`. Introduce the `SampleStatsCarrier`
type from the design doc's Decision 9 sketch, change both functions'
parameter types to it (structurally compatible with the existing
`LineStatus`, no call-site changes needed at either existing call site).

- [ ] **Step 5: Add tests proving the widened signature works for a
      `dataQuality`-less carrier**

`frontend/lib/sampleStats.test.ts` — new test(s) passing a bare
`{ sampleStats, sampleAvailability }` object (no `dataQuality` field) and
asserting `sampleUnavailableReason`/`formatSampleSummary` behave
correctly (in particular: the `'tfl'`-quality branch is never reached,
and `'no-coverage'` availability still renders *some* sensible string
even though it's documented-unreachable from the real route — test the
type accepts it structurally regardless, since TypeScript can't enforce
the "never actually happens" invariant Decision 7 documents).

- [ ] **Step 6: Add the fetch wrapper and render the section in `page.tsx`**

`frontend/app/stations/[crs]/page.tsx`:
- Add `fetchStationSampleStats` (design doc Decision 9's sketch),
  structurally mirroring `fetchStationDisruptions` (`:60-70`).
- Add `getAllTocs().catch(() => [])` to the page's data-fetching
  `Promise.all`, alongside the existing `fetchStationDisruptions`/
  `getPreferences` calls.
- Render the three-state section (not-sampled / sampled-but-empty /
  operator rows) below the existing per-line disruption list, per
  design doc Decision 9's exact copy and structure. Reuse
  `formatSampleSummary` for each row's trailing text, and the same
  `tocs`-lookup-with-fallback pattern `AllLinesTable.tsx:81` already
  establishes for resolving an operator code to a display name.

- [ ] **Step 7: Update `page.test.tsx`'s mocks and add coverage**

`frontend/app/stations/[crs]/page.test.tsx`: extend the existing
`vi.mock('@/lib/api', …)` block (`:9-17`) to also mock
`getStationSampleStats` and `getAllTocs`. Add test cases for all three
states:
- `getStationSampleStats` rejecting with `ApiNotFoundError` → "not part
  of our live departure sampling" text renders.
- `getStationSampleStats` resolving `[]` → "no live departures currently
  recorded" text renders.
- `getStationSampleStats` resolving a two-operator array → both rows
  render, in the order returned, with `tocs`-resolved names where
  `getAllTocs` provides a match and bare codes where it doesn't (cover
  both in one test, using a `tocs` mock that only names one of the two
  operators).

- [ ] **Step 8: Test and build**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 9: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts \
        frontend/lib/sampleStats.ts frontend/lib/sampleStats.test.ts \
        "frontend/app/stations/[crs]/page.tsx" "frontend/app/stations/[crs]/page.test.tsx"
git commit -m "Show per-operator sample stats on the station detail page"
```

---

### Task 6: Final verification

- [ ] **Step 1: Full workspace verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -p aggregator -- --ignored --test-threads=1
```

- [ ] **Step 2: Full frontend verification**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 3: Manual smoke check against a real deployment (if available)**

`GET /public/stations/EDB/sample-stats` (or any known multi-operator
station from the research doc's measured list — `EDB`, `LIV`, `NCL`) —
confirm multiple operator entries, correct JSON casing throughout
(including nested `sampleStats.avgDelayMinutes`, not
`avg_delay_minutes`). `GET /public/stations/ZZZ/sample-stats` (an unsampled
code) — confirm `404`.

- [ ] **Step 4: Confirm no stray edits outside this plan's file scope**

```bash
git diff --stat main...HEAD
```

Compare against this plan's Global Constraints "File scope" list — flag
anything unexpected before considering the branch done.

## Testing

Summarized (see each task's own steps for the authoritative detail):

- **`crates/common`**: new unit tests for `compute_sample_stats` — pure,
  no I/O, covers cancelled/delayed/skipped/average arithmetic and the
  boundary/edge cases named in Task 1 Step 2.
- **`crates/aggregator`**: zero new tests — the existing `aggregation.rs`
  suite is the regression check for Task 2's refactor, and must pass
  unmodified.
- **`crates/api`**: new unit tests for `station_stats::compute_station_operator_stats`
  (pure, no DB, Task 3); extracted-but-unmodified `render.rs` tests as a
  regression check (Task 4); a `oneshot`-probe path test plus
  `#[ignore]`-gated DB-backed tests for the new route (Task 4), following
  this crate's two-tier testing convention throughout
  (`crates/api/src/routes/line_status.rs`/`crates/api/src/data/custom_lines.rs`
  as the exact precedents cited above).
- **`frontend`**: unit tests for the new `api.ts` fetcher, the widened
  `sampleStats.ts` functions, and all three new page states, via the
  existing `vi.mock('@/lib/api', …)`/`renderWithMantine` pattern already
  established in `page.test.tsx`.
- **CI**: this plan's DB-backed tests run under the existing
  `.github/workflows/ci.yml:215-216` job (`cargo test -p api -p
  aggregator -- --ignored --test-threads=1`) — no new CI job needed.
