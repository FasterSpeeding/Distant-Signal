# Half-Hourly Full-Coverage Trends — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to work this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This plan is frontend-only.** The backend routes/tables/aggregator
> write path this plan's chart reads from already exist and are already
> tested — see the design doc's Current relevant state. Nothing under
> `crates/` is touched by either task below.

**Goal:** implement
`docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md`
("the design doc") end to end — a new `HalfHourlyCoverageTrendsResults.tsx`
Server Component, structurally mirroring the existing
`HalfHourlyTrendsResults.tsx` (the sample-derived half-hourly chart),
rendered on `frontend/app/lines/[id]/page.tsx` underneath the existing
half-hourly sample chart, over the same fixed rolling 24-hour window.
This closes the one real gap the full-coverage-metrics scaffolding left
open: `CoverageTrendsResults.tsx` renders only a daily full-coverage
series today; this plan adds the half-hourly one, in the same place this
repo already puts half-hourly trend charts for the sample-derived series.

**Design doc:**
`docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md`
— its Decisions section is authoritative for shape/placement/copy; this
plan only sequences and lands them.

**Tech stack:** Next.js 16 App Router + TypeScript, Vitest 2 +
`@testing-library/react` (`frontend/test/render.tsx`'s `renderWithMantine`
helper). No Rust, no migration, no new route — every backend piece this
plan depends on already shipped in the full-coverage-metrics scaffolding.

---

## Non-goals

- **Any backend change.** No new migration, route, query, or aggregator
  write path. `GET /Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}`
  and its underlying `line_status_half_hourly_coverage_stats` table
  already exist and are already tested.
- **Any new frontend type or API fetcher.** `LineHalfHourlyCoverageStats`
  (`frontend/lib/types.ts`) and `getLineHalfHourlyCoverageStats`
  (`frontend/lib/api.ts`) already exist, added by the scaffolding plan's
  Task 9 specifically for this future use, unused until this plan.
- **A granularity toggle/selector UI.** Design doc Decision 1 — this
  repo's settled answer is two fixed views in two fixed places, not a
  switch. Not built here, not reconsidered here.
- **Any change to `CoverageTrendsResults.tsx`** (the existing daily
  full-coverage chart) or `TrendsResults.tsx`/`HalfHourlyTrendsResults.tsx`
  (the sample-derived charts). All three are read for their structure,
  none are modified.
- **Any change to `TrendsCharts.tsx`.** Already bucket-key- and
  granularity-agnostic (`granularity: 'day' | 'halfHour'` already
  supported); reused unmodified.
- **Calibrating any sparse-data-floor constant against real data.** No
  full-coverage producer exists yet; the new floor constant is a
  placeholder, same posture as every sibling constant already in this
  file tree (design doc Decision 2, Open question 1).
- **Resolving the line-info-page chart-density question** the design doc's
  own Open question 2 raises (four chart-bearing sections once this
  ships). Flagged, not solved, matching the granularity design's own
  precedent of leaving exact sizing/density to a later screenshot pass.

## Global Constraints

- **Testing:** `npm test`, `npx tsc --noEmit`, and `npm run build`, all
  from `frontend/`. No DB-backed tests are added by this plan (nothing
  here talks to a database — it's a pure Server Component reading an
  already-live route through an already-existing fetcher).
- **Commit granularity:** one commit per task, matching this repo's
  stated convention (see the scaffolding plan's own commit-per-task
  precedent).
- **The new component's tests must mirror `HalfHourlyTrendsResults.test.tsx`'s
  existing case set** (empty state, sparse-bucket gap handling, honesty
  copy rendered verbatim, `granularity="halfHour"` tick formatting,
  heading levels) — not a thinner test file than its direct structural
  sibling.
- **File scope** (created or modified): new
  `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx`
  and its colocated
  `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.test.tsx`;
  modified `frontend/app/lines/[id]/page.tsx` and
  `frontend/app/lines/[id]/page.test.tsx`. Nothing else.

---

### Task 1: New `HalfHourlyCoverageTrendsResults.tsx` component + its own tests

**Files:**
`frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx`
(new), `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.test.tsx`
(new)

Independent starting point — this component compiles and is fully
testable on its own, with no dependency on Task 2's page wiring.

- [ ] **Step 1:** Create the component, structurally mirroring
  `HalfHourlyTrendsResults.tsx` (read it in full first) crossed with
  `CoverageTrendsResults.tsx`'s existing full-coverage-specific pieces
  (read it in full too):
  - Fetch via the already-existing `getLineHalfHourlyCoverageStats(id, from, to)`
    (`frontend/lib/api.ts`) — no `try`/`catch`-to-error-Paper wrapping
    needed unless mirroring `HalfHourlyTrendsResults`'s own
    unreachable-backend handling turns out to apply identically here
    (it does — same route family, same failure mode); mirror that
    component's `try { ... } catch { return <Paper>...isn't available
    right now.</Paper> }` guard verbatim, reworded for "coverage" instead
    of generic "trend."
  - `SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY = 10` (design doc Decision 2)
    — its own module-level constant, own doc comment explaining the
    halving derivation from `CoverageTrendsResults.tsx`'s
    `SPARSE_DATA_FLOOR_WINDOWS = 20`, mirroring
    `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`'s existing comment shape.
  - `toHalfHourlyCoverageChartPoints(stats: LineHalfHourlyCoverageStats[]): ChartPoint[]`
    — same shape as `CoverageTrendsResults.tsx`'s `toCoverageChartPoints`,
    `bucketKey: row.halfHourStart`, sparse test against
    `row.resolvedWindows < SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY`.
    Exported (not just used internally), matching every sibling
    `toXChartPoints` function's exported-for-testing convention.
  - Empty state: `<Paper withBorder p="md"><Text c="dimmed">Not enough
    full-coverage data yet for this line.</Text></Paper>` — verbatim
    match of `CoverageTrendsResults.tsx`'s own empty-state text.
  - Honesty copy: `CoverageTrendsResults.tsx`'s existing sentence, reused
    **verbatim, no rewording** (design doc Decision 3 — this sentence has
    no per-bucket attribution clause to reword, unlike the sample-series
    pair's copy).
  - Section heading: `<Title order={3} size="h6">Full coverage</Title>`
    (design doc Decision 4).
  - `<TrendsCharts points={points} granularity="halfHour" order={4} />`.
- [ ] **Step 2:** Tests in the new `.test.tsx`, mirroring
  `HalfHourlyTrendsResults.test.tsx`'s case set one-for-one against the
  new fetcher/floor/copy:
  - `toHalfHourlyCoverageChartPoints` preserves a bucket at/above the
    floor, nulls one below it while preserving `sampleCycles`
    (`resolvedWindows`'s value, threaded through the same `ChartPoint.sampleCycles`
    field `CoverageTrendsResults.tsx`'s own `toCoverageChartPoints` already
    uses for this purpose).
  - Empty-state case: bounded `Paper`, no chart rendered.
  - Unreachable-backend case: the "isn't available right now"-shaped
    fallback, mirroring `HalfHourlyTrendsResults.test.tsx`'s equivalent
    case if present, else add it fresh.
  - A sparse-bucket-does-not-render-a-flat-zero-line case, mirroring
    `HalfHourlyTrendsResults.test.tsx`'s own.
  - Normal multi-bucket case: renders both charts, honesty copy text
    matches the design doc's Decision 3 sentence **exactly** (assert the
    literal string, same way `HalfHourlyTrendsResults.test.tsx` asserts
    its own copy verbatim — this sentence must not silently drift from
    what `CoverageTrendsResults.tsx` already renders on the other page).
  - `granularity="halfHour"` tick-formatter case, mirroring
    `HalfHourlyTrendsResults.test.tsx`'s own.
  - Heading-level case: `Full coverage` renders at `level: 3`, its two
    `TrendsCharts` titles render at `level: 4` (one below), no skipped
    level.
- [ ] **Step 3:** `npx tsc --noEmit && npm test -- HalfHourlyCoverageTrendsResults`
  (from `frontend/`) — new file only, clean.
- [ ] **Step 4:** Commit:
  ```
  git add frontend/app/lines/\[id\]/history/HalfHourlyCoverageTrendsResults.tsx \
          frontend/app/lines/\[id\]/history/HalfHourlyCoverageTrendsResults.test.tsx
  git commit -m "Add HalfHourlyCoverageTrendsResults, the half-hourly full-coverage Trends chart"
  ```

### Task 2: Wire it onto the line-info page

**Files:** `frontend/app/lines/[id]/page.tsx`, `frontend/app/lines/[id]/page.test.tsx`

Depends on Task 1's component existing.

- [ ] **Step 1:** Import `HalfHourlyCoverageTrendsResults` in
  `page.tsx`. Add a second `<Suspense fallback={<Skeleton height={280}
  />}>...</Suspense>` block immediately after the existing
  `HalfHourlyTrendsResults` one, inside the same "Recent trends (last 24
  hours)" `Stack`, passing the same `trendsRange.from`/`trendsRange.to`
  already computed once for `HalfHourlyTrendsResults` (design doc
  Decision 1 — same window for both sections, not two independently
  computed "now"s). Own, separate `Suspense` boundary from the existing
  one (design doc Decision 4), so a slow coverage fetch never blocks the
  sample chart above it.
- [ ] **Step 2:** Update `page.test.tsx`:
  - Add `getLineHalfHourlyCoverageStats: vi.fn()` to the existing
    `vi.mock('@/lib/api', ...)` block (it currently mocks
    `getLineHalfHourlyStats` but not this new fetcher — an unmocked call
    would attempt a real fetch and fail every existing test that renders
    this page).
  - Default every existing test's mock to `mockResolvedValue([])` for the
    new fetcher (empty coverage data, matching current production
    reality — no producer exists yet), so existing assertions about the
    page's other content are undisturbed.
  - Add at least one new test asserting the "Full coverage" section
    renders on this page (heading present, or the empty-state text when
    the mock returns `[]`) — a real regression guard that Task 2 Step 1's
    wiring actually landed, not just that Task 1's component works in
    isolation.
- [ ] **Step 3:** `npm test && npx tsc --noEmit && npm run build` (from
  `frontend/`) — full suite, not just the new/touched files, to catch any
  snapshot/heading-count assertion elsewhere in this page's test file
  that assumed exactly one Trends section.
- [ ] **Step 4:** Commit:
  ```
  git add frontend/app/lines/\[id\]/page.tsx frontend/app/lines/\[id\]/page.test.tsx
  git commit -m "Render the half-hourly full-coverage Trends chart on the line-info page"
  ```

## Testing

- **`frontend`**: new Vitest coverage for
  `toHalfHourlyCoverageChartPoints` and `HalfHourlyCoverageTrendsResults`
  itself (Task 1), plus an integration-level assertion that
  `/lines/[id]/page.tsx` actually renders the new section (Task 2) — via
  this repo's existing `renderWithMantine`/`vi.mock('@/lib/api', …)`/
  `vi.mock('@mantine/charts', …)` conventions, identical to how
  `HalfHourlyTrendsResults.test.tsx` and `page.test.tsx` already test the
  sample-derived half-hourly chart and its page wiring.
- **No backend testing** — nothing under `crates/` changes.
- **CI**: no new CI job needed; this runs under the existing frontend
  `npm test`/`npm run build` steps.
