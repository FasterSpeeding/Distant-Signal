# Trend Sample-Volume Chart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** add a "Trains counted" bar-chart panel to the LDBWS-sample-backed
Trends charts (the four-tier `TrendsResults` on `/lines/[id]/history`'s
Trends tab, and the half-hourly `HalfHourlyTrendsResults` on `/lines/[id]`'s
"Recent trends" section) so a viewer can see how many distinct trains a
given bucket's delay/cancellation/skip rate is actually counted over —
without changing any backend code, since the value (`total`) is already
computed, deduplicated, and on the wire today.

**Architecture:** `total` already flows unmodified from
`dedup_new_sample_stats` (aggregator) through every `Line*Stats` row and
JSON route to `frontend/lib/types.ts`'s four `Line*Stats` interfaces
(`LineDailyStats`/`LineHalfHourlyStats`/`LineHourlyStats`/
`LineSixHourlyStats`) — it is simply dropped on the floor, unread, by
every `ChartPoint`-producing mapper function. This plan (1) widens
`ChartPoint` with one new, never-null `total: number` field and adds the
one non-ternary line each of the four existing mapper functions needs to
populate it (Task 1); (2) adds a new, opt-in `<BarChart>` panel to the
shared `TrendsCharts.tsx`, gated behind a new `showVolume` prop that
defaults to `false` (Task 2); (3) turns that prop on only for the two
LDBWS-sample-backed callers — `TrendsResults.tsx` (Task 3) and
`HalfHourlyTrendsResults.tsx` (Task 4) — leaving `CoverageTrendsResults.tsx`
and `HalfHourlyCoverageTrendsResults.tsx` completely untouched, by
construction of the default. No migration, query, or route changes; no
`crates/aggregator` or `crates/api` changes at all.

**Tech Stack:** Next.js/React Server Components, Mantine (`@mantine/charts`
`BarChart`, pinned `9.5.2`), Vitest, `@testing-library/react`.

**Spec:**
`docs/superpowers/specs/2026-09-05-trend-sample-volume-chart-design.md` —
its Decisions 1, 2, 4, 5, and 6 are authoritative for the underlying data
semantics (what `total` means, why it's never nulled, why a bar chart not
a line, why no new backend work) and this plan does not re-derive or
re-litigate them. **Decision 3 and Decision 7 of that spec are stale**: they
were written against a pre-granularity-merge codebase (three separate
sibling mapper functions, a two-value `granularity` union, no
`GranularityControl.tsx`) that has since been replaced on `main` by
`docs/superpowers/plans/2026-09-05-configurable-trend-granularity-plan.md`
(merged, commit `ab43057`). This plan re-derives the file-level facts
against the real, current worktree (see "What changed since the spec was
written" below) rather than trusting that spec's own file/line citations.

---

## What changed since the spec was written (read before Task 1)

The design spec's Correction 4 already flagged that it was written against
a codebase where the granularity plan "has NOT been implemented on disk."
**It has been, since.** Concretely, as verified fresh against this worktree
right now:

1. **`TrendsResults.tsx` is no longer a single hard-wired daily fetch.** It
   is now one component that branches internally on a `TrendGranularity`
   (`frontend/lib/history.ts:270`, `'halfHour' | 'hour' | 'sixHour' |
   'day'`) via a `fetchPoints` switch
   (`frontend/app/lines/[id]/history/TrendsResults.tsx:70-89`), calling one
   of `getLineDailyStats`/`getLineHalfHourlyStats`/`getLineHourlyStats`/
   `getLineSixHourlyStats` per tier. **There are no longer three separate
   mapper functions** (`toChartPoints`/`toHalfHourlyChartPoints`/
   `toCoverageChartPoints`) doing the spec's imagined job for the four-tier
   surface — there is now **one generic `toChartPoints<T extends
   StatsRow>`** (`TrendsResults.tsx:48-64`) parameterized over a
   `bucketKeyOf` accessor and a sparse floor, reused across all four tiers
   by `fetchPoints`'s switch (`TrendsResults.tsx:70-89`).
2. **`HalfHourlyTrendsResults.tsx` still exists as its own file, unmerged
   into `TrendsResults.tsx`, and is NOT mounted on the history page.** It
   is mounted on `/lines/[id]/page.tsx` (line 15 import, line 201 render)
   under a "Recent trends (last 24 hours)" heading — a second, independent
   surface, still with its own `toHalfHourlyChartPoints` mapper
   (`HalfHourlyTrendsResults.tsx:28-40`) and its own sparse floor constant.
   It shares the same underlying LDBWS-sample/dedup semantics as
   `TrendsResults`'s `halfHour` tier — same `getLineHalfHourlyStats` route,
   same `LineHalfHourlyStats` type, same dedup guarantee — just consumed by
   a second page. **This plan treats it as in-scope for the new panel**
   (Task 4) precisely because it is equally LDBWS-backed and deduplicated,
   not a coverage variant.
3. **`CoverageTrendsResults.tsx` (daily) and `HalfHourlyCoverageTrendsResults.tsx`
   (half-hourly) both exist as separate, already-shipped files**, each with
   their own mapper (`toCoverageChartPoints`, `CoverageTrendsResults.tsx:25-37`;
   `toHalfHourlyCoverageChartPoints`, `HalfHourlyCoverageTrendsResults.tsx:25-37`)
   and their own sparse floor. `CoverageTrendsResults` is mounted on the
   history page's Trends tab (`history/page.tsx:217`), directly under
   `TrendsResults`; `HalfHourlyCoverageTrendsResults` is mounted on
   `/lines/[id]/page.tsx` (line 16 import, line 211 render), directly under
   `HalfHourlyTrendsResults`. Both always render their "not enough
   full-coverage data yet" empty state today (`lines_with_full_coverage`
   returns nothing — no real producer exists).
4. **The aggregator's own dedup-gap comment is unchanged and still
   accurate, verified fresh**: `crates/aggregator/src/main.rs:299-304`
   still reads "this one is fed each status's raw `full_coverage_stats`
   directly, NOT run through a dedup step... no defined per-service dedup
   analog exists yet for a full-coverage producer." The spec's Decision 3
   risk (a real full-coverage producer, once built, could report an
   inflated, non-deduplicated `total` relative to the LDBWS case) is still
   live and still unaddressed. See "Scope decision: full-coverage" below.
5. **All four `Line*Stats` interfaces already carry `total: number` on the
   wire, confirmed directly, not assumed from the spec**:
   `frontend/lib/types.ts` — `LineDailyStats:165`, `LineHalfHourlyStats:188`,
   `LineHourlyStats:210`, `LineSixHourlyStats:226` — and both coverage
   siblings (`LineDailyCoverageStats:251`, `LineHalfHourlyCoverageStats:267`).
   On the Rust side, `crates/api/src/data/queries.rs`'s `DailyStatsRow`
   (`:1042`) and `HalfHourlyStatsRow` (`:1097`) both carry `pub total: i64`;
   `sub_daily_stats_for_range` (`queries.rs:1183`) groups the *same*
   `HalfHourlyStatsRow` shape into 1-hour/6-hour buckets (confirmed by
   reading its signature and the tests around `queries.rs:1900-2020`), and
   `crates/api/src/routes/line_status.rs`'s `sub_daily_stats_to_json`
   (`:487`, reused for both the Hourly and SixHourly routes at `:523`/`:535`)
   serializes `"total": row.total` (`:504`) from that same row shape — so
   the Hourly/SixHourly tiers genuinely already carry `total`, not just the
   Daily/HalfHourly ones the spec inspected directly.
6. **`@mantine/charts`' `BarChart` (pinned `9.5.2`) is confirmed, by reading
   its real `.d.ts`, to share the same `GridChartBaseProps` as `LineChart`**
   (`frontend/node_modules/@mantine/charts/lib/types.d.ts:16-60`: `data`,
   `dataKey`, `xAxisProps`, `valueFormatter` are all on the shared
   `GridChartBaseProps` both components extend) — **this resolves the
   design spec's Open question 3** (whether `BarChart` accepts the same
   `xAxisProps` shape `LineChart` does). It does. No third-`<LineChart>`
   fallback is needed.

## Scope decision: full-coverage (`CoverageTrendsResults`/`HalfHourlyCoverageTrendsResults`)

**This plan's `total` data-population (Task 1) reaches all four existing
mapper functions, including the two full-coverage ones** — this costs
nothing extra (the field is already on the wire for those two routes too,
per `LineDailyCoverageStats`/`LineHalfHourlyCoverageStats` above) and keeps
`ChartPoint` a single, uniformly-populated type rather than a type two of
its four producers can't actually satisfy.

**But the new `<BarChart>` panel itself is explicitly out of scope for
`CoverageTrendsResults.tsx`/`HalfHourlyCoverageTrendsResults.tsx`, and
neither file is touched by this plan at all**, because:
- `main.rs:299-304`'s own comment (re-verified above, unchanged) states the
  full-coverage historical rollup's `total` is not run through any
  per-service dedup step. If a real full-coverage producer is ever wired up
  without also fixing that, its `total` could be inflated several-fold
  relative to the LDBWS case — building a volume chart on it now would bake
  in a currently-misleading number before anyone has a chance to fix the
  write path.
- No real full-coverage producer exists yet (`lines_with_full_coverage`
  always returns empty, `main.rs:309` — unchanged since the design spec was
  written), so both `CoverageTrendsResults`/`HalfHourlyCoverageTrendsResults`
  render nothing but their "not enough data yet" empty state today. There is
  nothing observable to gain from shipping the panel there now.
- The mechanism this plan builds (a `showVolume` prop on the shared
  `TrendsCharts`, defaulting to `false`, Task 2) makes turning the panel on
  for the coverage variants later a one-line change per call site, once a
  real producer and its dedup fix both exist — not a re-architecture.

This is a plan-level, explicit scope call, not a silent omission: whoever
eventually builds a real full-coverage producer should read the design
spec's Decision 3/Open question 1 before flipping `showVolume` on for
`CoverageTrendsResults`/`HalfHourlyCoverageTrendsResults`.

## Judgment call: sparse buckets show their real (possibly low) `total`, never hidden — resolving an apparent tension in this plan's own brief

The brief that produced this plan asked for manual verification that a
sparse/gapped bucket "correctly shows no bar (not a zero bar)." Read
literally against Decision 2's "never null `total`, even for sparse
buckets" rule, these sound contradictory. They are not, once "sparse"
and "gapped" are pulled apart into the two different things they mean in
this codebase:

1. **A bucket present in the API response, but below the sparse-data
   floor** (a real row exists; `sampleCycles`/`resolvedWindows` is too low
   to trust a rate off of). Per Decision 2, **this plan does NOT null or
   hide `total` for this case** — it shows the bucket's real, possibly
   small, `total` value as a normal bar, precisely because that low number
   is *why* the rate chart above shows a gap there. Hiding it here would
   defeat the whole feature's purpose.
2. **A bucket with no row in the API response at all** (the aggregator
   never ran a cycle for that period). Every existing `ChartPoint`-based
   panel already only renders buckets the API actually returned a row for
   — this is pre-existing, unmodified behavior (see spec Non-goals) — so
   there is no chart point, on any panel including the new one, for a
   period like this. This is the case that genuinely "shows no bar, not a
   zero bar," and it requires no new code: it already falls out of every
   mapper function only ever `.map()`-ing over the array the API returned.

**This plan's manual-verification step (Task 5) tests case 2 as
"unchanged, still no point rendered," and tests case 1 as "the new bar
chart shows the real low number, the rate chart above it shows a gap for
the same bucket."** No `gapSpans`/`<ReferenceArea>` treatment is added to
the new panel (see Task 2) — that machinery exists to gray out spans where a
field is null, and `total` is never null for a row that exists.

## Global Constraints

- **No backend changes of any kind.** No new column, migration, query, or
  route in `crates/aggregator` or `crates/api`. `total` is already
  selected, serialized, typed, and fetched at every layer (see "What
  changed" item 5 above) — this plan only starts reading it one layer
  further, inside frontend mapper functions.
- **`ChartPoint.total` is `number`, never `number | null`.** It is
  populated unconditionally, outside the `sparse ? null : ...` ternaries
  the other four fields already go through, in every mapper function that
  produces a `ChartPoint` (Task 1) — see the Judgment call above.
- **No stacked bar of `delayed`/`cancelled`/`skipped` against `total`.**
  Rejected by the design spec's Decision 5: those three fields are
  independently-computed, overlapping filters over `total`, not a
  partition of it (`common::compute_sample_stats`,
  `crates/common/src/lib.rs:1181-1210`) — stacking them would visually
  exceed `total` and mislead. The new panel is a single flat bar series
  (`total` only).
- **No `gapSpans`/`<ReferenceArea>` gap-shading on the new panel.**
  Deliberate, per Decision 2 and the Judgment call above — `total` is never
  null for a bucket with a real row, so there is nothing for that
  machinery to shade.
- **`showVolume` defaults to `false` on `TrendsCharts`.** Only
  `TrendsResults.tsx` and `HalfHourlyTrendsResults.tsx` pass `showVolume`
  (Tasks 3-4). `CoverageTrendsResults.tsx`/`HalfHourlyCoverageTrendsResults.tsx`
  are not touched by this plan at all (see Scope decision above) — their
  behavior is unchanged purely by omission, not by an explicit new
  conditional in those files.
- **Panel placement: above the existing rate chart, sharing the same
  `xAxisProps`/category-axis identity.** Matches the design spec's Decision
  5 ("a viewer sees 'how many trains' before 'what rate'") and keeps every
  panel's x-axis ticks aligned under every `granularity` value.
- **Testing.** Frontend only, `cd frontend`: `npm test -- <file>` after
  each task touching a specific test file, `npm test` (= `vitest run`, per
  `frontend/package.json`'s `scripts.test`) at the end. **In addition to
  `npm test`, run `npm run build` once near the end (Task 5) and start the
  dev server (`npm run dev`) to manually verify the new chart in a real
  browser** — this repo's own standing practice for UI changes (see the
  granularity plan's own Global Constraints for precedent). This planning
  pass does not perform that manual check itself.
- **File scope.** Modified: `frontend/app/lines/[id]/history/chartPoint.ts`,
  `frontend/app/lines/[id]/history/TrendsResults.tsx`,
  `frontend/app/lines/[id]/history/TrendsCharts.tsx`,
  `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx`,
  `frontend/app/lines/[id]/history/CoverageTrendsResults.tsx` (Task 1 only —
  its mapper function, not its render path or call site),
  `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx`
  (Task 1 only, same reason). Test files: `TrendsResults.test.tsx`,
  `TrendsCharts.test.tsx`, `HalfHourlyTrendsResults.test.tsx`,
  `CoverageTrendsResults.test.tsx`, `HalfHourlyCoverageTrendsResults.test.tsx`.
  **Not modified, anywhere in this plan**: any `crates/` file,
  `GranularityControl.tsx`, `history/page.tsx`, `/lines/[id]/page.tsx`,
  `frontend/lib/history.ts`, `frontend/lib/types.ts`, `frontend/lib/api.ts`
  (all already carry/fetch `total`, nothing to add).

## Non-goals

- **Any backend column, query, route, or migration.** Covered above.
- **A stacked composition chart.** Covered above (Global Constraints).
- **A "peak concurrent trains" / true concurrency metric.** `total` is a
  period-cumulative, first-observed-this-bucket distinct-train count (see
  Tooltip/copy decision below) — nothing in the current schema tracks
  concurrency, and building that would need a wholly different
  aggregation. Not attempted here.
- **Fixing the full-coverage historical rollup's missing dedup step.**
  Flagged (Scope decision above), not fixed — no real producer exists yet
  to test a fix against, and it is an aggregator-write-path concern, not a
  frontend-chart concern.
- **Turning the new panel on for `CoverageTrendsResults`/
  `HalfHourlyCoverageTrendsResults`.** Explicitly excluded (Scope decision
  above) — the mechanism (`showVolume`) makes this a one-line follow-up
  once it's warranted, not something this plan schedules.
- **Any change to `GranularityControl.tsx`, `history/page.tsx`, or
  `/lines/[id]/page.tsx`.** The granularity control already re-renders
  `TrendsResults` (and therefore the new panel) on every tier switch via
  its existing `Suspense` key (`history/page.tsx:199-204`,
  keyed on `${granularity}-...`) — no new fetch, no new wiring needed. See
  Task 5's manual-verification step, which confirms this rather than
  assumes it.
- **Recalibrating any `SPARSE_DATA_FLOOR_*` constant, or changing which
  fields they gate.** Those constants keep gating exactly the four rate/
  delay fields they already gate; `total` is deliberately excluded from
  that gate (Judgment call above), not added to it.
- **A tooltip/hover-detail redesign or a new UI affordance beyond the
  existing per-section honesty-copy paragraph.** See the Tooltip/copy
  decision immediately below — resolved as a copy addition, not a new
  interactive element.

## Tooltip/copy decision

**Chosen: extend each of the two now-in-scope components' existing
honesty-copy text with one additional sentence, not a new tooltip,
footnote, or interactive affordance.** The design spec's Decision 1 caveat
— a bucket's `total` means "trains first observed this bucket," not
"trains concurrently running during this bucket" — is real and, per the
spec's own Open question 2, more likely to be misread at sub-daily
granularities (a 30-minute bucket's low `total` reads more easily as
"almost nothing was running then" than a daily bucket's does). Given this
repo's established pattern of carrying exactly this kind of caveat in the
existing per-tier `<Text c="dimmed">` paragraph (already done, per-tier, in
`TrendsResults.tsx`'s `HONESTY_COPY` record and `HalfHourlyTrendsResults.tsx`'s
hardcoded paragraph) rather than a tooltip, this plan follows the same
pattern rather than introducing a new UI mechanism. Task 3 and Task 4 each
append one sentence to their component's existing copy — see their Step 1s
for the exact wording.

---

## Task 1: Widen `ChartPoint` and populate `total` in every existing mapper function

**Files:**
- Modify: `frontend/app/lines/[id]/history/chartPoint.ts`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.tsx:33-39` (the
  local `StatsRow` interface `toChartPoints` is generic over) and
  `:48-64` (`toChartPoints` itself)
- Modify: `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx:28-40`
  (`toHalfHourlyChartPoints`)
- Modify: `frontend/app/lines/[id]/history/CoverageTrendsResults.tsx:25-37`
  (`toCoverageChartPoints`) — **only this function; its render path and
  `CoverageTrendsResults` call site at `:83` are untouched, per the Scope
  decision above**
- Modify: `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx:25-37`
  (`toHalfHourlyCoverageChartPoints`) — same "mapper only" scope as above
- Test: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`,
  `HalfHourlyTrendsResults.test.tsx`, `CoverageTrendsResults.test.tsx`,
  `HalfHourlyCoverageTrendsResults.test.tsx`

**Interfaces:**
- Produces: `ChartPoint.total: number` — consumed by Task 2's new
  `<BarChart>` panel via `points[].total`.
- Every mapper function's return type stays `ChartPoint[]`, unchanged in
  shape besides the new field.

This task must land as one unit: `ChartPoint.total` is a required
(non-optional) field, so TypeScript's object-literal checking means widening
the type without simultaneously fixing all four functions that construct a
`ChartPoint` literal leaves the tree failing to compile — there is no
meaningful "approve the type change but not its producers" split here.

- [ ] **Step 1: Widen `ChartPoint`**

In `frontend/app/lines/[id]/history/chartPoint.ts`, add `total` to the
interface (after `avgDelayMinutes`, before `sampleCycles`, matching the
field order every mapper already builds its object literals in):

```ts
export interface ChartPoint {
  bucketKey: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  /** How many distinct trains (LDBWS `service_id`s, deduplicated) were
   * first observed during this bucket -- NOT how many were concurrently
   * running (see the design doc's Decision 1 caveat, and each caller's own
   * honesty-copy paragraph). Never `null`, unlike the four fields above:
   * deliberately NOT gated by the sparse-data floor a mapper applies to
   * those -- a low `total` for a sparse bucket is exactly why that
   * bucket's rate fields are null, and hiding it here would defeat the
   * point of showing it at all. */
  total: number;
  sampleCycles: number;
}
```

- [ ] **Step 2: Populate `total` in `TrendsResults.tsx`'s generic `toChartPoints`**

Widen the local `StatsRow` interface (`TrendsResults.tsx:33-39`) so the
generic function can read `row.total`:

```ts
interface StatsRow {
  sampleCycles: number;
  total: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
  avgDelayMinutes: number;
}
```

Then add the one non-ternary line to `toChartPoints` (`:48-64`):

```ts
export function toChartPoints<T extends StatsRow>(
  stats: T[],
  bucketKeyOf: (row: T) => string,
  floor: number,
): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < floor;
    return {
      bucketKey: bucketKeyOf(row),
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.sampleCycles,
    };
  });
}
```

- [ ] **Step 3: Update `TrendsResults.test.tsx`'s `toChartPoints` assertions**

The two `toEqual`/direct-value assertions in the `describe('toChartPoints
(generic)', ...)` block (`:60-83`) need `total` added — `dailyRow()`'s
default `total` is `100` (`:36`):

```ts
describe('toChartPoints (generic)', () => {
  it('preserves a bucket at or above the given floor', () => {
    const stats = [dailyRow({ sampleCycles: 20 })];
    const [point] = toChartPoints(stats, (row) => row.day, 20);
    expect(point).toEqual({
      bucketKey: '2026-08-01',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      total: 100,
      sampleCycles: 20,
    });
  });

  it('turns a bucket below the given floor into a gap, preserving sampleCycles', () => {
    const stats = [dailyRow({ sampleCycles: 19 })];
    const [point] = toChartPoints(stats, (row) => row.day, 20);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.total).toBe(100); // never nulled, even when sparse
    expect(point.sampleCycles).toBe(19);
  });
});
```

- [ ] **Step 4: Run the `TrendsResults` unit tests to verify Steps 2-3**

```bash
cd frontend && npm test -- TrendsResults.test.tsx
```

Expected: PASS. `TrendsCharts` itself isn't touched until Task 2, and the
new `total` field is additive (nothing existing reads or asserts its
absence), so the `toChartPoints` describe block and every other existing
`TrendsResults` describe block should already be green.

- [ ] **Step 5: Populate `total` in `HalfHourlyTrendsResults.tsx`'s `toHalfHourlyChartPoints`**

```ts
export function toHalfHourlyChartPoints(stats: LineHalfHourlyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY;
    return {
      bucketKey: row.halfHourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.sampleCycles,
    };
  });
}
```

- [ ] **Step 6: Update `HalfHourlyTrendsResults.test.tsx`'s `toHalfHourlyChartPoints` assertions**

`halfHourlyRow()`'s default `total` is `100` (it spreads `dailyRow()`,
`:48-50` of the test file). Update the one `toEqual` assertion:

```ts
describe('toHalfHourlyChartPoints', () => {
  it('preserves a half hour at or above the sparse-data floor', () => {
    const stats = [halfHourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY })];
    const [point] = toHalfHourlyChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-31T14:00:00Z',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      total: 100,
      sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY,
    });
  });

  it('turns a half hour below the sparse-data floor into a gap, preserving sampleCycles', () => {
    const stats = [halfHourlyRow({ sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1 })];
    const [point] = toHalfHourlyChartPoints(stats);
    expect(point.delayRate).toBeNull();
    expect(point.cancellationRate).toBeNull();
    expect(point.skipRate).toBeNull();
    expect(point.avgDelayMinutes).toBeNull();
    expect(point.total).toBe(100); // never nulled, even when sparse
    expect(point.sampleCycles).toBe(SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1);
    expect(point.bucketKey).toBe('2026-08-31T14:00:00Z');
  });
});
```

- [ ] **Step 7: Populate `total` in the two full-coverage mappers**

`CoverageTrendsResults.tsx`'s `toCoverageChartPoints`:

```ts
export function toCoverageChartPoints(stats: LineDailyCoverageStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.resolvedWindows < SPARSE_DATA_FLOOR_WINDOWS;
    return {
      bucketKey: row.day,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.resolvedWindows,
    };
  });
}
```

`HalfHourlyCoverageTrendsResults.tsx`'s `toHalfHourlyCoverageChartPoints`:

```ts
export function toHalfHourlyCoverageChartPoints(stats: LineHalfHourlyCoverageStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.resolvedWindows < SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY;
    return {
      bucketKey: row.halfHourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      total: row.total,
      sampleCycles: row.resolvedWindows,
    };
  });
}
```

Note: **do not touch either file's `CoverageTrendsResults`/
`HalfHourlyCoverageTrendsResults` component function or its `<TrendsCharts
.../>` call site** in this step — this step is the mapper function only,
per the Scope decision above.

- [ ] **Step 8: Update the two coverage test files' mapper assertions**

Both test files' `row()`/`halfHourlyCoverageRow()` helpers already default
`total: 100` (`CoverageTrendsResults.test.tsx:42`,
`HalfHourlyCoverageTrendsResults.test.tsx:45`). Add `total: 100,` to each
file's one `toEqual` assertion and a `expect(point.total).toBe(100)` to
each file's "below the floor" test, mirroring Steps 3/6 exactly:

`CoverageTrendsResults.test.tsx`:

```ts
  it('preserves a day at or above the sparse-data floor', () => {
    const stats = [row({ resolvedWindows: SPARSE_DATA_FLOOR_WINDOWS })];
    const [point] = toCoverageChartPoints(stats);
    expect(point).toEqual({
      bucketKey: '2026-08-01',
      delayRate: 0.1,
      cancellationRate: 0.02,
      skipRate: 0.01,
      avgDelayMinutes: 3.5,
      total: 100,
      sampleCycles: SPARSE_DATA_FLOOR_WINDOWS,
    });
  });
```

And add `expect(point.total).toBe(100);` inside the "below the floor" test
right after the existing `expect(point.avgDelayMinutes).toBeNull();` line.

`HalfHourlyCoverageTrendsResults.test.tsx`: the identical edit, to its own
`toEqual` (bucketKey `'2026-08-31T14:00:00Z'`) and its own "below the
floor" test.

- [ ] **Step 9: Run all four touched test files**

```bash
cd frontend && npm test -- TrendsResults.test.tsx HalfHourlyTrendsResults.test.tsx CoverageTrendsResults.test.tsx HalfHourlyCoverageTrendsResults.test.tsx
```

(`chartPoint.ts` has no standalone test file — it's a pure type with no
runtime behavior of its own to unit-test; its correctness is exercised
through these four suites.) Expected: all four suites PASS, including the
newly-added `total` assertions.

- [ ] **Step 10: Commit**

```bash
cd frontend
git add app/lines/\[id\]/history/chartPoint.ts app/lines/\[id\]/history/TrendsResults.tsx app/lines/\[id\]/history/HalfHourlyTrendsResults.tsx app/lines/\[id\]/history/CoverageTrendsResults.tsx app/lines/\[id\]/history/HalfHourlyCoverageTrendsResults.tsx app/lines/\[id\]/history/TrendsResults.test.tsx app/lines/\[id\]/history/HalfHourlyTrendsResults.test.tsx app/lines/\[id\]/history/CoverageTrendsResults.test.tsx app/lines/\[id\]/history/HalfHourlyCoverageTrendsResults.test.tsx
git commit -m "Widen ChartPoint with a total field, populated by every existing mapper"
```

---

## Task 2: Add the opt-in `<BarChart>` "Trains counted" panel to `TrendsCharts.tsx`

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Test: `frontend/app/lines/[id]/history/TrendsCharts.test.tsx`

**Interfaces:**
- Consumes: `ChartPoint.total` (Task 1).
- Produces: `TrendsCharts`'s new `showVolume?: boolean` prop (default
  `false`), consumed by Task 3 (`TrendsResults.tsx`) and Task 4
  (`HalfHourlyTrendsResults.tsx`), each of which will pass `showVolume`.
  `CoverageTrendsResults.tsx`/`HalfHourlyCoverageTrendsResults.tsx` are not
  changed and therefore keep the default `false` (Scope decision above).

- [ ] **Step 1: Add the `BarChart` import and `showVolume` prop**

In `frontend/app/lines/[id]/history/TrendsCharts.tsx`:

```tsx
import { BarChart, LineChart } from '@mantine/charts';
```

Widen the component's props (`:79-95`) with `showVolume`:

```tsx
export function TrendsCharts({
  points,
  granularity,
  order,
  showVolume = false,
}: {
  points: ChartPoint[];
  granularity: TrendGranularity;
  order: TitleOrder;
  /** Renders the "Trains counted" bar-chart panel above the rate chart
   * when `true`. Defaults to `false` -- only the two LDBWS-sample-backed
   * callers (`TrendsResults`, `HalfHourlyTrendsResults`) pass `true`. The
   * full-coverage callers (`CoverageTrendsResults`,
   * `HalfHourlyCoverageTrendsResults`) deliberately do not, since that
   * rollup's `total` isn't deduplicated yet -- see
   * docs/superpowers/plans/2026-09-05-trend-sample-volume-chart-plan.md's
   * "Scope decision: full-coverage". */
  showVolume?: boolean;
}) {
```

- [ ] **Step 2: Render the new panel, above the rate chart, no gap-shading**

Insert this `<Stack>` as the *first* child of the returned `<>...</>`
fragment (`:101-172`), immediately before the existing "Delay /
cancellation / skip rate" `<Stack>`:

```tsx
return (
  <>
    {showVolume && (
      <Stack gap={4}>
        <Title order={order} size="h6">
          Trains counted
        </Title>
        {/* Single flat series, never a stack of delayed/cancelled/skipped
            against total -- those three are independently-computed,
            overlapping filters over total (crates/common/src/lib.rs's
            compute_sample_stats), not a partition of it; stacking them
            would visually exceed total and mislead. No gapSpans/
            ReferenceArea here (unlike the two panels below): `total` is
            never null for a bucket with a real row -- see this plan's
            "Judgment call" section for why a low bar for a sparse bucket
            is the informative signal, not something to hide. */}
        <BarChart
          h={180}
          data={points}
          dataKey="bucketKey"
          series={[{ name: 'total', label: 'Trains counted', color: 'teal.6' }]}
          xAxisProps={xAxisProps}
        />
      </Stack>
    )}
    <Stack gap={4}>
      <Title order={order} size="h6">
        Delay / cancellation / skip rate
      </Title>
```

(The rest of the two existing `<Stack>` panels, and the closing `</>`, are
unchanged.)

- [ ] **Step 3: Update the component's own doc comment**

The doc comment above `TrendsCharts` (`:55-78`) currently doesn't mention a
third panel. Add one sentence noting the new opt-in panel, e.g. appended
after the existing paragraph about `granularity`: "`showVolume` is a third,
independent prop: when `true`, an additional 'Trains counted' `<BarChart>`
panel renders first, above the rate chart, reading `points[].total`
directly -- see this file's own render logic and
`docs/superpowers/plans/2026-09-05-trend-sample-volume-chart-plan.md`."

- [ ] **Step 4: Update `TrendsCharts.test.tsx`'s `@mantine/charts` mock**

The existing mock (`:7-14`) only stubs `LineChart`. Since `TrendsCharts.tsx`
now also imports `BarChart` from the same module, add a `BarChart` stub —
note this is safe even for tests that don't pass `showVolume` at all,
since `{showVolume && <BarChart .../>}` short-circuits and never
evaluates/constructs the `<BarChart>` element when `showVolume` is falsy:

```tsx
vi.mock('@mantine/charts', () => ({
  LineChart: (props: { xAxisProps?: { tickFormatter?: (value: string) => string } }) => (
    <div
      data-testid="line-chart"
      data-has-tick-formatter={String(typeof props.xAxisProps?.tickFormatter === 'function')}
    />
  ),
  BarChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="bar-chart" data-series={props.series.map((s) => s.name).join(',')} data-points={JSON.stringify(props.data)} />
  ),
}));
```

- [ ] **Step 5: Add new tests for the `showVolume` prop**

Append to `TrendsCharts.test.tsx`:

```tsx
describe('TrendsCharts showVolume prop', () => {
  const points: ChartPoint[] = [
    { bucketKey: '2026-08-01T12:00:00Z', delayRate: 0.1, cancellationRate: 0, skipRate: 0, avgDelayMinutes: 1, total: 42, sampleCycles: 50 },
  ];

  it('does not render the bar chart by default', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    expect(screen.queryByTestId('bar-chart')).not.toBeInTheDocument();
  });

  it('renders the bar chart, reading total, when showVolume is true', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = screen.getByTestId('bar-chart');
    expect(barChart).toHaveAttribute('data-series', 'total');
    const barPoints = JSON.parse(barChart.dataset.points as string);
    expect(barPoints[0].total).toBe(42);
  });

  it('renders the bar chart above (before) the rate line chart in document order', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    const barChart = screen.getByTestId('bar-chart');
    const lineCharts = screen.getAllByTestId('line-chart');
    expect(barChart.compareDocumentPosition(lineCharts[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('still renders the two rate/delay line charts unchanged when showVolume is true', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} showVolume />);
    expect(screen.getAllByTestId('line-chart')).toHaveLength(2);
  });
});
```

- [ ] **Step 6: Run it**

```bash
cd frontend && npm test -- TrendsCharts.test.tsx
```

Expected: PASS, including the four new `showVolume` tests.

- [ ] **Step 7: Commit**

```bash
cd frontend
git add app/lines/\[id\]/history/TrendsCharts.tsx app/lines/\[id\]/history/TrendsCharts.test.tsx
git commit -m "Add an opt-in Trains counted bar-chart panel to TrendsCharts"
```

---

## Task 3: Turn `showVolume` on for `TrendsResults.tsx` (the four-tier Trends-tab surface) and extend its honesty copy

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsResults.tsx:15-31` (the
  `HONESTY_COPY` record), `:150` (the `<TrendsCharts .../>` call site)
- Test: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:**
- Consumes: `TrendsCharts`'s `showVolume` prop (Task 2).

- [ ] **Step 1: Extend all four `HONESTY_COPY` entries with the volume-chart clarification**

Append one sentence to each of the four strings in
`TrendsResults.tsx:26-31` (per the Tooltip/copy decision above) — shown
here for `day` and `halfHour`; `hour`/`sixHour` get the same sentence
verbatim (it doesn't vary per tier, unlike the rest of the paragraph):

```ts
const HONESTY_COPY: Record<TrendGranularity, string> = {
  day: 'Rates shown count each distinct train once per day, based on its status the first time it was seen that day -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Days with too little coverage show as a gap rather than a misleading flat line. The trains-counted chart below always shows the real count, even for days too sparse to trust for a rate -- a low number there is exactly why a day may show as a gap above; it counts each train once, in the day it was first seen, not how many were simultaneously running.',
  halfHour: 'Rates shown count each distinct train once per half hour, based on its status the first time it was seen that half hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Half-hour periods with too little coverage show as a gap rather than a misleading flat line. The trains-counted chart below always shows the real count, even for half hours too sparse to trust for a rate -- a low number there is exactly why a half hour may show as a gap above; it counts each train once, in the half hour it was first seen, not how many were simultaneously running.',
  hour: 'Rates shown count each distinct train once per hour, based on its status the first time it was seen that hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Hours with too little coverage show as a gap rather than a misleading flat line. The trains-counted chart below always shows the real count, even for hours too sparse to trust for a rate -- a low number there is exactly why an hour may show as a gap above; it counts each train once, in the hour it was first seen, not how many were simultaneously running.',
  sixHour: 'Rates shown count each distinct train once per six-hour period, based on its status the first time it was seen in that period -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Six-hour periods with too little coverage show as a gap rather than a misleading flat line. The trains-counted chart below always shows the real count, even for six-hour periods too sparse to trust for a rate -- a low number there is exactly why a period may show as a gap above; it counts each train once, in the period it was first seen, not how many were simultaneously running.',
};
```

- [ ] **Step 2: Pass `showVolume` at the `<TrendsCharts .../>` call site**

```tsx
<TrendsCharts points={points} granularity={granularity} order={2} showVolume />
```

- [ ] **Step 3: Add tests asserting the panel renders per-tier and a sparse bucket keeps its real `total`**

Append to `TrendsResults.test.tsx`, inside (or alongside) the existing
`describe('TrendsResults', ...)` block:

```ts
  it('renders the trains-counted bar chart for the default day granularity', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyRow({ day: '2026-08-01', total: 42 })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByRole('heading', { name: 'Trains counted' })).toBeInTheDocument();
  });

  it('a sparse day still shows its real total in the bar chart while its rate is a gap', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([
      dailyRow({ day: '2026-08-01', sampleCycles: 19, total: 3 }),
      dailyRow({ day: '2026-08-02', sampleCycles: 500, total: 150 }),
    ]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const barChart = screen.getByTestId('bar-chart');
    const points = JSON.parse(barChart.dataset.points as string);
    const sparseDay = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-01');
    expect(sparseDay.total).toBe(3); // real value, never hidden or zeroed

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const ratePoints = JSON.parse(rateChart!.dataset.points as string);
    const sparseRatePoint = ratePoints.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-01');
    expect(sparseRatePoint.delayRate).toBeNull(); // rate IS gapped for the same bucket
  });

  it.each([
    ['halfHour', 'getLineHalfHourlyStats', halfHourlyRow] as const,
    ['hour', 'getLineHourlyStats', hourlyRow] as const,
    ['sixHour', 'getLineSixHourlyStats', sixHourlyRow] as const,
  ])('renders the trains-counted bar chart for the %s granularity too', async (granularity, fnName, rowFactory) => {
    const mockFn = vi.mocked(api[fnName as keyof typeof api]) as unknown as ReturnType<typeof vi.fn>;
    mockFn.mockResolvedValue([rowFactory({ total: 7 })]);
    renderWithMantine(
      await TrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z', granularity }),
    );
    const barChart = screen.getByTestId('bar-chart');
    const points = JSON.parse(barChart.dataset.points as string);
    expect(points[0].total).toBe(7);
  });
```

This requires the same `@mantine/charts` mock update Task 2 made to
`TrendsCharts.test.tsx` — apply the identical `BarChart` stub (Task 2 Step
4's snippet) to `TrendsResults.test.tsx`'s own `vi.mock('@mantine/charts',
...)` call (`:30`), since `TrendsResults.tsx` renders the real
`TrendsCharts`, which now imports `BarChart` too:

```tsx
vi.mock('@mantine/charts', () => ({
  LineChart: (props: MockLineChartProps) => lineChartMock(props),
  BarChart: (props: { data: unknown[]; series: { name: string }[] }) => (
    <div data-testid="bar-chart" data-series={props.series.map((s) => s.name).join(',')} data-points={JSON.stringify(props.data)} />
  ),
}));
```

- [ ] **Step 4: Run it**

```bash
cd frontend && npm test -- TrendsResults.test.tsx
```

Expected: PASS, including the new bar-chart and sparse-bucket tests, and
every pre-existing test in this file (the honesty-copy assertions at
`:90`/`:141` match on a fixed substring via regex, e.g.
`/Rates shown count each distinct train once per day/`, which still
matches the now-longer string).

- [ ] **Step 5: Commit**

```bash
cd frontend
git add app/lines/\[id\]/history/TrendsResults.tsx app/lines/\[id\]/history/TrendsResults.test.tsx
git commit -m "Show the trains-counted chart on the four-tier Trends tab"
```

---

## Task 4: Turn `showVolume` on for `HalfHourlyTrendsResults.tsx` (the line-detail-page surface) and extend its honesty copy

**Files:**
- Modify: `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx:91-96`
  (honesty-copy paragraph), `:100` (the `<TrendsCharts .../>` call site)
- Test: `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.test.tsx`

**Interfaces:**
- Consumes: `TrendsCharts`'s `showVolume` prop (Task 2) — identical
  mechanism to Task 3, applied to this component's separate call site.

- [ ] **Step 1: Extend the honesty-copy paragraph**

```tsx
<Text size="sm" c="dimmed">
  Rates shown count each distinct train once per half hour, based on its status the first time it was seen
  that half hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later
  while still in view will still show here as on time. Half-hour periods with too little coverage show as a
  gap rather than a misleading flat line. The trains-counted chart below always shows the real count, even for
  half hours too sparse to trust for a rate -- a low number there is exactly why a half hour may show as a gap
  above; it counts each train once, in the half hour it was first seen, not how many were simultaneously
  running.
</Text>
```

- [ ] **Step 2: Pass `showVolume` at the call site**

```tsx
<TrendsCharts points={points} granularity="halfHour" order={3} showVolume />
```

- [ ] **Step 3: Update the test file's `@mantine/charts` mock and add coverage**

Apply the same `BarChart` stub addition Task 3 Step 3 made, to
`HalfHourlyTrendsResults.test.tsx`'s own mock (`:39`). Then append:

```ts
  it('renders the trains-counted bar chart', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([halfHourlyRow({ halfHourStart: '2026-08-31T14:00:00Z', total: 9 })]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    expect(screen.getByRole('heading', { name: 'Trains counted' })).toBeInTheDocument();
    const barChart = screen.getByTestId('bar-chart');
    const points = JSON.parse(barChart.dataset.points as string);
    expect(points[0].total).toBe(9);
  });

  it('a sparse half hour keeps its real total in the bar chart even though its rate is gapped', async () => {
    vi.mocked(api.getLineHalfHourlyStats).mockResolvedValue([
      halfHourlyRow({ halfHourStart: '2026-08-31T13:30:00Z', sampleCycles: SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY - 1, total: 2 }),
      halfHourlyRow({ halfHourStart: '2026-08-31T14:00:00Z', sampleCycles: 25, total: 30 }),
    ]);
    renderWithMantine(
      await HalfHourlyTrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z' }),
    );
    const barChart = screen.getByTestId('bar-chart');
    const points = JSON.parse(barChart.dataset.points as string);
    const sparse = points.find((p: { bucketKey: string }) => p.bucketKey === '2026-08-31T13:30:00Z');
    expect(sparse.total).toBe(2);
  });
```

Note: the pre-existing test `'a normal multi-bucket range renders without
throwing...'` (`:96-111` in the current file) asserts
`expect(screen.getAllByTestId('line-chart')).toHaveLength(2)` — this still
holds unchanged, since the new bar chart is stubbed with a distinct
`data-testid="bar-chart"`, not `"line-chart"`.

- [ ] **Step 4: Run it**

```bash
cd frontend && npm test -- HalfHourlyTrendsResults.test.tsx
```

Expected: PASS, including the two new tests and every pre-existing one.

- [ ] **Step 5: Commit**

```bash
cd frontend
git add app/lines/\[id\]/history/HalfHourlyTrendsResults.tsx app/lines/\[id\]/history/HalfHourlyTrendsResults.test.tsx
git commit -m "Show the trains-counted chart on the line-detail page's half-hourly trends too"
```

---

## Task 5: Full suite, build, and manual verification

**Files:** none modified — this task only runs and observes.

- [ ] **Step 1: Run the full frontend test suite**

```bash
cd frontend && npm test
```

Expected: every suite passes, including the four touched in Tasks 1, 3, 4
and `TrendsCharts.test.tsx` from Task 2. Also confirm
`CoverageTrendsResults.test.tsx`/`HalfHourlyCoverageTrendsResults.test.tsx`
pass **unmodified beyond Task 1's mapper-only edit** — no `bar-chart`
testid should appear anywhere in either file's assertions, since neither
component passes `showVolume`.

- [ ] **Step 2: Build**

```bash
cd frontend && npm run build
```

Expected: builds cleanly. This is the step that actually exercises
TypeScript's structural checking of every `ChartPoint`-returning object
literal across all four mapper functions — `vitest` alone does not run a
full type-check, so a real compile is the only step in this plan that
would catch a mismatched literal.

- [ ] **Step 3: Manual dev-server verification**

Start the dev server (`npm run dev`) and, against a real line with sampled
data:

1. Open `/lines/<some-line-id>/history`, switch to the Trends tab, and
   confirm a "Trains counted" bar chart renders **above** the existing
   "Delay / cancellation / skip rate" chart, for the default (Daily) view.
2. Use `GranularityControl` to switch through all four tiers (30 min,
   Hourly, 6-hourly, Daily) and confirm the bar chart re-renders each
   time, with bars aligned to the same x-axis ticks as the rate chart
   below it, and that switching tiers triggers exactly one network
   request per tier (open the browser's network tab) — confirming no
   second fetch is needed for the volume data, since it rides along in the
   same response the rate chart already consumes.
3. Find or manufacture a sparse bucket (a very short custom date range
   right at a line's live edge often has one) and confirm: the rate chart
   above shows a shaded gap for that bucket, while the trains-counted bar
   chart shows a real, small (not zero, not missing) bar for the exact
   same bucket.
4. Confirm a calendar period with literally no underlying row (e.g. a date
   range predating this line's sampling) renders no point on either chart
   — no bar, not a zero-height bar — consistent with the pre-existing
   "only render buckets the API returned a row for" behavior.
5. Open `/lines/<some-line-id>` (not `/history`) and confirm the "Recent
   trends (last 24 hours)" section also shows the new "Trains counted"
   panel above its own rate chart.
6. Confirm `/lines/<some-line-id>/history`'s "Full coverage" section
   (`CoverageTrendsResults`) does **not** show a "Trains counted" panel —
   it should render exactly as it did before this plan (either its "not
   enough full-coverage data yet" empty state, or its existing two
   panels only).

This planning pass does not perform this manual check itself.

- [ ] **Step 4: Final commit (if Step 3 surfaced any fixups)**

```bash
cd frontend
git add -A
git commit -m "Fix up trains-counted chart per manual verification"
```

(Skip this step entirely if Step 3 found nothing to fix.)
