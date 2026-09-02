# Line History Trends Chart Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all seven screenshot-confirmed problems in the `/lines/{id}/history` "Trends" tab's two `@mantine/charts` `LineChart`s (and, automatically, the embedded 7-day preview on `/lines/{id}`, which renders the same components unmodified) by making the six concrete, file-scoped edits the design spec settled on: import the missing `@mantine/charts/styles.css` stylesheet (the tooltip's real root cause), add a legend plus per-series dash patterns to the three-series rate chart, add a `valueFormatter` to the average-delay chart, render shaded `ReferenceArea` bands across contiguous sparse-data gap-day runs on both charts, inset the x-axis's right edge so the last plotted point stops clipping, and give the empty-state text a bounded visual treatment — after first confirming, live, whether Finding 7's whitespace is a real persistent bug or the transient SSR-streaming artifact the design's own Open Questions flags as unresolved.

**Spec:** `docs/superpowers/specs/2026-09-02-line-history-chart-fixes-design.md` — read in full before starting; this plan does not restate its research, only carries its seven Decisions into concrete tasks. Cross-references below to "Decision N" / "Open question N" refer to that document.

**Architecture (before/after prop list — no file changes shape beyond what's listed):**

```
frontend/app/globals.css
  @import '@mantine/core/styles.css';     (existing, line 1)
  @import '@mantine/dates/styles.css';    (existing, line 2)
+ @import '@mantine/charts/styles.css';   NEW -- Task 1 / Decision 1

frontend/app/lines/[id]/history/TrendsCharts.tsx

  Rate chart (delayRate/cancellationRate/skipRate LineChart, lines 38-49):
    data, dataKey, series, valueFormatter, connectNulls   (unchanged)
    series[].color, series[].label                        (unchanged)
+   series[].strokeDasharray   NEW -- Task 2 / Decision 2 (one distinct pattern/series)
+   withLegend={true}          NEW -- Task 2 / Decision 2 (Mantine default top position kept)
+   h={280 -> ~310-320}        NEW -- Task 2, budget for the added legend row, confirm at 390px
+   xAxisProps={{ padding: { right: 12 } }}   NEW -- Task 5 / Decision 5
+   children: <ReferenceArea> per contiguous gap-day run   NEW -- Task 4 / Decision 4

  Average-delay chart (avgDelayMinutes LineChart, lines 55-61), h={220, unchanged}:
    data, dataKey, series, connectNulls                    (unchanged)
+   valueFormatter={(v) => `${v.toFixed(1)} min`}   NEW -- Task 3 / Decision 3
+   xAxisProps={{ padding: { right: 12 } }}          NEW -- Task 5 / Decision 5
+   children: <ReferenceArea> per contiguous gap-day run   NEW -- Task 4 / Decision 4
    (no withLegend -- single series, Decision 2 explicitly excludes it)

+ export function gapSpans(points: ChartPoint[]): { startDay: string; endDay: string }[]
    NEW pure helper, colocated in TrendsCharts.tsx -- Task 4

frontend/app/lines/[id]/history/TrendsResults.tsx
+   empty-state branch (line 41): wrap in a bounded block, not bare Text
                                                     NEW -- Task 6 / Decision 7
    (toChartPoints, honesty copy, gap-nulling logic: unchanged)

frontend/app/lines/[id]/page.tsx
    (no changes -- inherits everything via TrendsResults/TrendsCharts,
     confirmed unchanged by re-reading it in full: Decision 6)

frontend/app/lines/[id]/history/TrendsCharts.test.tsx
+   NEW test file -- doesn't exist today; Task 4 adds it for gapSpans

frontend/app/lines/[id]/history/TrendsResults.test.tsx
    extended in Tasks 2, 3, 4, 5, 6 -- mock grows to capture
    withLegend/strokeDasharray/valueFormatter/xAxisProps/children,
    plus the empty-state wrapper assertion
```

**Tech Stack:** Next.js App Router + TypeScript + `@mantine/charts@9.5.2` (already pinned, `frontend/package.json:14`) wrapping `recharts@^3.2.1` (`frontend/package.json:23`) — no new dependency in either ecosystem. Vitest + `@testing-library/react` for prop-level unit tests (existing convention). Playwright (already a devDependency, `@playwright/test`) or the `run`/Playwright-MCP tooling for the real-render screenshot verification Task 7 requires.

**Status note — every citation below independently re-confirmed against this worktree's actual current source, not trusted blind from the design spec:**

- `frontend/app/lines/[id]/history/TrendsCharts.tsx` (39 lines, read in full): rate chart `LineChart` at lines 38-49 (`valueFormatter` at line 47, `connectNulls={false}` at line 48, series array at lines 42-46); average-delay chart's `Stack` at lines 51-62, its `LineChart` at lines 55-61 with **no `valueFormatter`** (confirmed by direct read — matches the design's Finding 3 root cause exactly). Neither call passes `withLegend`, `legendProps`, `xAxisProps`, `dotProps`, or `children` today.
- `frontend/app/lines/[id]/history/TrendsResults.tsx` (72 lines, read in full): `SPARSE_DATA_FLOOR_CYCLES = 20` at line 13; `toChartPoints` at lines 23-35, nulling all four rate/delay fields together whenever `sampleCycles < 20` (line 25); the empty-state short-circuit `<Text c="dimmed">Not enough sampled data yet for this line.</Text>` at line 41; the honesty copy at lines 59-64.
- `frontend/app/lines/[id]/history/chartPoint.ts` (13 lines, read in full): `ChartPoint` — `day: string`, three `number | null` rate fields, `avgDelayMinutes: number | null`, `sampleCycles: number`. Unchanged by this plan.
- `frontend/app/lines/[id]/page.tsx` (187 lines, read in full): lines 161-184 render `<TrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />` inside `<Suspense fallback={<Skeleton height={280} />}>`, unmodified, per the comment at lines 165-180 ("Reuses `/lines/[id]/history`'s own Trends-tab component wholesale"). No preview-specific styling or props exist anywhere in this file — confirmed, this plan adds no task for it (Decision 6).
- `frontend/app/lines/[id]/history/page.tsx`: line 138 wraps the same `<TrendsResults>` call (line 139) in `<Suspense fallback={<Skeleton height={320} />}>` inside a `Tabs` structure. Unchanged by this plan.
- `frontend/app/globals.css`: lines 1-2 are exactly `@import '@mantine/core/styles.css';` / `@import '@mantine/dates/styles.css';`. No `@mantine/charts/styles.css` or `styles.layer.css` import anywhere in this 989-line file — confirmed by direct read of the import block and by `grep -rn "mantine/charts/styles" frontend/` returning zero matches repo-wide.
- **Testing precedent, confirmed exactly:** `TrendsResults.test.tsx` (140 lines, read in full) mocks `@mantine/charts` wholesale at lines 22-31 (comment at lines 15-21 states the convention explicitly: not asserting on Recharts' own SVG output), and covers `toChartPoints`'s gap logic (lines 49-73), the empty state (lines 76-81), a sparse day not rendering as zero (lines 83-100), the two-chart split (lines 102-129), and `connectNulls={false}` on both charts (lines 131-139). No `TrendsCharts.test.tsx` file exists today (confirmed by directory listing). `frontend/app/lines/[id]/page.test.tsx` (mock at lines 31-38, capturing only `data`/`series`) and `frontend/app/lines/[id]/history/page.test.tsx` (mock at lines 21-23, capturing nothing) are page-composition tests, not chart-behavior tests — neither needs a change from this plan.
- **`recharts` version, re-confirmed against a real local install (the design's own worktree had none; this one does, at the actual repo root):** `/workspaces/github-com-fasterspeeding-network-rail-status/frontend/node_modules/recharts/package.json` resolves to **`3.10.1`**, not the exact `3.2.1` the design fetched from npm/unpkg — both satisfy the pinned `^3.2.1` range in `package.json:23`, so this is expected semver drift, not a discrepancy to fix. `@mantine/charts` resolves to exactly `9.5.2` as pinned, matching the design. Every API surface this plan relies on was independently re-checked against this real `3.10.1`/`9.5.2` install, not re-trusted from the design's own npm-fetched `3.2.1` sources:
  - `ReferenceArea` exported from `recharts` — confirmed, `node_modules/recharts/types/index.d.ts:62-63`.
  - `XAxisPadding` type — confirmed, `node_modules/recharts/types/state/cartesianAxisSlice.d.ts:10-13`: `{ left?: number; right?: number } | 'gap' | 'no-gap'`.
  - `CartesianChart.js`'s `defaultMargin` — confirmed present, `node_modules/recharts/es6/chart/CartesianChart.js:17`.
  - `GridChartBaseProps.withLegend` (default `false`), `.withTooltip` (default `true`), `.valueFormatter` ("formats values on Y axis and inside the tooltip"), `.xAxisProps`, `.legendProps` — all confirmed, `node_modules/@mantine/charts/lib/types.d.ts:16-70` (exact line numbers: `xAxisProps` 28, `legendProps` 46, `withLegend` 50, `valueFormatter` 60).
  - `LineChartProps.children` ("Additional components that are rendered inside recharts `LineChart` component"), `.dotProps`, `.lineChartProps`, and `LineChartSeries.strokeDasharray` — all confirmed, `node_modules/@mantine/charts/lib/LineChart/LineChart.d.ts` (full file read; `strokeDasharray` at line 13, `dotProps` at line 36, `lineChartProps` at line 42, `children` at line 46).
  - Note: `GridChartBaseProps` also has its own, *different* `strokeDasharray?: string | number` (`lib/types.d.ts:38`, "Dash array for the grid lines and cursor") — this is the whole chart's grid/cursor dash, not the per-series one `LineChartSeries.strokeDasharray` provides. Task 2 must set the per-series field on each `series[]` entry, not the top-level `GridChartBaseProps` one, or every series (and the grid) gets the same dash instead of three distinct ones.
- **This worktree vs. `main` — confirmed identical for every file this plan touches.** This worktree's branch and local `main` diverged 24 commits back (`git merge-base` = `789b993`; `main` has 24 commits this worktree's branch doesn't, this branch has none `main` lacks), but `git diff HEAD main -- frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsResults.tsx frontend/app/globals.css frontend/app/lines/[id]/page.tsx` returns **zero output** — all four files this plan edits are byte-identical between this worktree and `main`. All line-number citations above were read directly from this worktree, not assumed from the design spec. If `main` gains further commits touching these files before this plan is executed, re-run that diff and re-verify line numbers before starting Task 1.
- **No screenshot evidence from the design's own stage-1/stage-2 pipeline survives on disk in this worktree, in `.superpowers/sdd/`, or anywhere in this session's filesystem** (checked all three; also checked git history for any committed `.png`/`.jpg`/screenshot-named file — none relate to this feature). Task 7 below plans around capturing fresh screenshots rather than diffing against originals that no longer exist, and flags this explicitly rather than assuming a comparison set is available.

## Global Constraints

- **All six code-bearing fixes land in exactly three files**: `frontend/app/globals.css` (one new `@import` line), `frontend/app/lines/[id]/history/TrendsCharts.tsx` (both `LineChart` calls plus the new `gapSpans` helper), and `frontend/app/lines/[id]/history/TrendsResults.tsx` (the empty-state branch only). `frontend/app/lines/[id]/page.tsx` and `frontend/app/lines/[id]/history/page.tsx` are **not touched by any task** — both already inherit every fix by rendering `TrendsResults`/`TrendsCharts` unmodified (Decision 6, re-confirmed above). Do not add either to any task's file list.
- **Tasks 2, 3, 4, and 5 all edit the same two `LineChart` call sites inside `TrendsCharts.tsx`.** Unlike a plan whose tasks touch disjoint files, these cannot be dispatched to separate subagents in parallel — running them out of order or concurrently will produce merge conflicts on the same ~25 lines. Execute Tasks 1 through 6 **serially, in the numbered order below**, each with its own commit before the next task starts.
- **Task 2 has a real dependency on Task 1, not just a sequencing convention.** The design's Decision 1 discussion notes the missing `@mantine/charts/styles.css` import leaves `ChartLegend` unstyled for the identical reason it leaves `ChartTooltip` unstyled — Task 2's legend is not meaningfully verifiable (and may render as bare, unstyled markup) until Task 1's stylesheet import has landed and been screenshot-confirmed.
- **This repo's existing Vitest suite mocks `@mantine/charts` wholesale and never exercises real Recharts rendering** (`TrendsResults.test.tsx:15-21`'s own comment, `page.test.tsx`, `history/page.test.tsx`). Every unit test added in Tasks 2 through 6 proves only that the right props reached the mocked `LineChart` — it proves nothing about how the tooltip, legend, gap bands, or edge padding actually render. Do not treat `npm test` passing as evidence that Findings 1, 2, 4, or 5 are fixed. Task 7's screenshot verification is mandatory before this plan is considered complete, not an optional nice-to-have.
- **No new npm dependency, anywhere.** `@mantine/charts`, `recharts`, and `@playwright/test` are all already present (`frontend/package.json:14,23` and devDependencies). Every prop/export this plan uses was independently re-confirmed against the real installed `9.5.2`/`3.10.1` packages (Status note above) — do not add a package to `package.json` in any task.
- **No backend, database, or migration change.** This is a purely frontend, presentation-layer plan — `getLineDailyStats`'s response shape, `LineDailyStats`, and `SPARSE_DATA_FLOOR_CYCLES`'s value are all explicitly out of scope (see Not in this plan).
- **Numeric/visual precision is deliberately left open by the design and must not be invented here.** Exact dash-pattern strings (Task 2), the rate chart's bumped `h` value (Task 2), the gap-band fill color/opacity (Task 4), and confirmation that `padding.right: 12` is the right number (Task 5) are all called out in the design's own Open Questions as needing a real screenshot to settle, not a guess baked into code. Each relevant task below has its own "confirm visually" step for exactly this reason — do not skip it and hand-wave a number as final.
- **Task 6 (Decision 7) must not skip its own investigation step.** The design explicitly could not confirm a persistent whitespace bug exists (Open question 3) — Task 6 starts with a live-repro step, not straight to a code change, and its fix step branches on what that repro finds.

---

### Task 1: Import the missing `@mantine/charts/styles.css` stylesheet (Decision 1)

**Files:**
- Modify: `frontend/app/globals.css`

**Interfaces:** none — a global CSS side-effect import, no exported symbol changes.

**Depends on:** nothing — this is the foundational task, and per the design's own framing, this alone is expected to resolve the entire Finding 1 (tooltip) symptom.

This is deliberately the smallest, highest-confidence task in this plan, and its own verification checkpoint — confirm it in isolation before layering Tasks 2-6 on top, per the design's Open question 1 (whether mixing the non-layered `styles.css` variant with this repo's other two already-non-layered Mantine imports produces any cascade surprise was never verified against a live render).

- [ ] **Step 1: Add the import**

In `frontend/app/globals.css`, add a third import line immediately after the existing two (currently lines 1-2):

```css
@import '@mantine/core/styles.css';
@import '@mantine/dates/styles.css';
@import '@mantine/charts/styles.css';
```

No other change to this file in this task — the WCAG anchor-color block currently starting at line 4 shifts down by one line but is otherwise untouched.

- [ ] **Step 2: Build check**

Run (from `frontend/`): `npm run build`
Expected: PASS. This is a real npm package export (`@mantine/charts`'s `package.json` declares `"./styles.css": "./styles.css"` alongside `"sideEffects": ["*.css"]`, confirmed against the real installed copy) — either the build resolves it (expected, since the identical pattern already works for the two existing imports on the lines above it) or it fails loudly at compile time, which is itself useful signal, not a silent no-op to worry about.

- [ ] **Step 3: Screenshot checkpoint — verify the tooltip fix in isolation**

Bring up a real dev server (see Task 7 for the general approach: `docker-compose.yml`/`docker-compose.dev.yml` per this repo's README "Running it" section, plus `npm run dev` in `frontend/`). Navigate to any line's `/lines/{id}/history` Trends tab, hover a data point on either chart, and screenshot the tooltip at a standard desktop viewport.

Expected: the tooltip now renders with a visible background, box-shadow, border, and constrained `min-width` (Mantine's `ChartTooltip` module classes — `.m_e4d36c9b` for the box model, `.m_3de8964e` for inter-row spacing, per the design's own citation of the shipped `styles.css`) — no more bare, unstyled `<div>`s spilling into content below.

**If the tooltip is still broken after this step**, stop before starting Task 2 and re-diagnose (per superpowers:systematic-debugging) rather than assuming a later task will incidentally fix it — the design's own root-cause diagnosis says this single import should be sufficient, so a miss here means the diagnosis needs revisiting, not that more props are needed.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/globals.css
git commit -m "Import @mantine/charts/styles.css, fixing the unstyled Trends chart tooltip"
```

---

### Task 2: Legend + per-series dash patterns on the rate chart (Decision 2)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:** no new exports — prop changes only to the rate chart's `LineChart` call (lines 38-49).

**Depends on:** Task 1 (real dependency, not just sequencing — see Global Constraints).

- [ ] **Step 1: Add `withLegend` and per-series `strokeDasharray`**

In `TrendsCharts.tsx`'s rate chart call (currently lines 38-49):

```tsx
<LineChart
  h={280}
  data={points}
  dataKey="day"
  withLegend
  series={[
    { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
    { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6', strokeDasharray: '6 4' },
    { name: 'skipRate', label: 'Skip rate', color: 'yellow.6', strokeDasharray: '2 3' },
  ]}
  valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
  connectNulls={false}
/>
```

`delayRate` deliberately keeps `strokeDasharray` unset (solid, Recharts' own default) rather than setting it explicitly — one of the three series should be the visual "default" line. No `legendProps` override — Mantine's own default (`verticalAlign="top"`) is the chosen placement per Decision 2, so omit the prop entirely rather than passing `legendProps={{ verticalAlign: 'top' }}` redundantly. Do not add `withLegend` or any `strokeDasharray` to the average-delay chart (lines 55-61) — Decision 2 explicitly scopes the legend to the three-series chart only.

The `'6 4'`/`'2 3'` values above are a reasonable starting point, not fixed by the design (Open question 6) — confirm they read as visually distinct dashed/dotted patterns (not near-identical) during Task 7's screenshot pass, and adjust if not.

- [ ] **Step 2: Budget height for the legend row**

Bump the rate chart's `h` from `280` to `310` (the design's own estimated range is "roughly 310-320", not measured). Leave the average-delay chart's `h={220}` unchanged — it gains no legend.

- [ ] **Step 3: Extend the test mock**

In `TrendsResults.test.tsx`, extend the `vi.mock('@mantine/charts', ...)` block (currently lines 22-31) to also surface `withLegend` and each series' `strokeDasharray`:

```tsx
vi.mock('@mantine/charts', () => ({
  LineChart: (props: {
    data: unknown[];
    series: { name: string; strokeDasharray?: string | number }[];
    connectNulls?: boolean;
    withLegend?: boolean;
  }) => (
    <div
      data-testid="line-chart"
      data-series={props.series.map((series) => series.name).join(',')}
      data-connect-nulls={String(props.connectNulls)}
      data-points={JSON.stringify(props.data)}
      data-with-legend={String(props.withLegend)}
      data-dash-patterns={props.series.map((series) => series.strokeDasharray ?? '').join(',')}
    />
  ),
}));
```

- [ ] **Step 4: Add assertions**

Alongside the existing "renders the average-delay chart as a separate LineChart instance" test (currently lines 118-129), add:

```tsx
it('gives the rate chart a legend and three distinct dash patterns, but not the average-delay chart', async () => {
  vi.mocked(api.getLineDailyStats).mockResolvedValue([row({ day: '2026-08-01' })]);
  renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

  const charts = screen.getAllByTestId('line-chart');
  const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate')!;
  const avgDelayChart = charts.find((chart) => chart.dataset.series === 'avgDelayMinutes')!;

  expect(rateChart.dataset.withLegend).toBe('true');
  const dashPatterns = rateChart.dataset.dashPatterns!.split(',');
  expect(new Set(dashPatterns).size).toBe(3); // three distinct values, including the empty string for the solid default

  expect(avgDelayChart.dataset.withLegend).not.toBe('true');
});
```

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test`
Expected: PASS. Note (Global Constraints): this only proves the props reached the mock — it proves nothing about the real legend's layout or wrap behavior at 390px. That is Task 7's job.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsResults.test.tsx
git commit -m "Add a legend and per-series dash patterns to the Trends rate chart"
```

---

### Task 3: `valueFormatter` on the average-delay chart (Decision 3)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:** no new exports — one new prop on the average-delay chart's `LineChart` call (lines 55-61).

**Depends on:** Task 2 (same file, same-region edits — see Global Constraints on serial execution). No conceptual dependency; this could in principle be written first, but must not be applied concurrently with Task 2's edit to the file.

- [ ] **Step 1: Add the formatter**

In `TrendsCharts.tsx`'s average-delay chart call (currently lines 55-61), matching the rate chart's existing pattern at line 47:

```tsx
<LineChart
  h={220}
  data={points}
  dataKey="day"
  series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
  valueFormatter={(value) => `${value.toFixed(1)} min`}
  connectNulls={false}
/>
```

- [ ] **Step 2: Extend the test mock to capture `valueFormatter`**

Add `valueFormatter` to the mocked `LineChart`'s captured props (alongside Task 2's additions) and surface it as an inspectable value, e.g. by attaching it to the rendered element via a ref-free approach — simplest is exposing it through a `data-*`-unfriendly channel like a module-level capture array, or (preferred, matching this repo's existing "assert on props handed to LineChart" convention) capture it on the DOM node via a non-serializable prop the test reads back directly from the mock's last call using `vi.fn()`:

```tsx
const lineChartMock = vi.fn((props: { valueFormatter?: (value: number) => string; series: { name: string }[] }) => (
  <div data-testid="line-chart" data-series={props.series.map((s) => s.name).join(',')} />
));
vi.mock('@mantine/charts', () => ({ LineChart: (props: unknown) => lineChartMock(props) }));
```

- [ ] **Step 3: Add the assertion, invoking the formatter directly against the design's own example float**

```tsx
it('formats the average-delay tooltip to one decimal place with a unit suffix', async () => {
  vi.mocked(api.getLineDailyStats).mockResolvedValue([row({ day: '2026-08-01', avgDelayMinutes: 0.41267123328767123 })]);
  renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

  const avgDelayCall = lineChartMock.mock.calls.find(([props]) => props.series[0]?.name === 'avgDelayMinutes')!;
  const [avgDelayProps] = avgDelayCall;
  expect(avgDelayProps.valueFormatter?.(0.41267123328767123)).toBe('0.4 min');
});
```

If Task 2's mock rewrite (Step 3 there) already threads props through a `vi.fn()` capture, reuse that single mock rather than introducing a second, parallel mocking approach — reconcile the two mock rewrites into one coherent mock module by the end of this task.

- [ ] **Step 4: Run the tests**

Run (from `frontend/`): `npm test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsResults.test.tsx
git commit -m "Format the average-delay chart's tooltip to one decimal place, matching the rate chart"
```

---

### Task 4: Gap-day shaded bands via `ReferenceArea` (Decision 4)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Create: `frontend/app/lines/[id]/history/TrendsCharts.test.tsx`

**Interfaces:**
- Produces: `export function gapSpans(points: ChartPoint[]): { startDay: string; endDay: string }[]` — new pure helper in `TrendsCharts.tsx`, exported for direct unit testing (matching this repo's existing convention of exporting `toChartPoints` for the same reason, `TrendsResults.tsx:23`).
- Consumed by: both `LineChart` calls' new `children` prop, inside this same file.

**Depends on:** Task 3 (same file, serial execution — see Global Constraints). This is the most involved task in this plan, since it adds real derivation logic rather than only a prop tweak.

- [ ] **Step 1: Add the `gapSpans` helper**

Add above the `TrendsCharts` function in `TrendsCharts.tsx`:

```tsx
import { ReferenceArea } from 'recharts';

/** A contiguous run of one or more days where every rate/delay field is
 * `null` -- i.e. below `SPARSE_DATA_FLOOR_CYCLES` (`TrendsResults.tsx`'s
 * `toChartPoints`, lines 23-35). Checking `delayRate === null` alone is
 * sufficient today because `toChartPoints` guarantees all four fields are
 * nulled together for a sparse day -- an implicit coupling to that
 * invariant, not something `ChartPoint`'s own type enforces (design spec
 * Open question 5). If a future change ever nulls fields independently,
 * this derivation would need to change too. */
export function gapSpans(points: { day: string; delayRate: number | null }[]): { startDay: string; endDay: string }[] {
  const spans: { startDay: string; endDay: string }[] = [];
  let current: { startDay: string; endDay: string } | null = null;
  for (const point of points) {
    if (point.delayRate === null) {
      current = current ? { ...current, endDay: point.day } : { startDay: point.day, endDay: point.day };
    } else {
      if (current) spans.push(current);
      current = null;
    }
  }
  if (current) spans.push(current);
  return spans;
}
```

Note the parameter type is a structural subset of `ChartPoint` (just `day`/`delayRate`), not the full `ChartPoint` interface — keeps the helper's own test fixtures minimal (Step 4 below) without needing to fabricate `cancellationRate`/`skipRate`/`avgDelayMinutes`/`sampleCycles` values that this function never reads. `TrendsCharts`'s own call sites still pass full `ChartPoint[]`, which satisfies this narrower parameter type structurally.

- [ ] **Step 2: Render one `<ReferenceArea>` per span, inside each chart's `children`**

Add to both `LineChart` calls (rate chart, and average-delay chart from Task 3):

```tsx
{gapSpans(points).map((span) => (
  <ReferenceArea
    key={`${span.startDay}-${span.endDay}`}
    x1={span.startDay}
    x2={span.endDay}
    fill="var(--mantine-color-gray-5)"
    fillOpacity={0.15}
    stroke="none"
    ifOverflow="visible"
  />
))}
```

Both charts render the *same* spans (computed once per chart call from the same `points` array — a single isolated gap day gets a thin band exactly like a multi-day gap, per Decision 4's "one mechanism, no special-cased isolated-point branch"). The fill color/opacity above is a starting point, not fixed by the design (Open question 6) — confirm during Task 7 that it reads as a visible-but-not-overpowering background band, and adjust if not.

- [ ] **Step 3: Flag the single-day-span alignment risk for Task 7**

Add a short comment above the `<ReferenceArea>` block noting the design's own Open question 4: whether `x1`/`x2` resolve cleanly against this chart's string `day` category values for a single isolated gap day (covering exactly that day's category width, not spilling into neighbors) was never verified against a live render by the design. If Task 7's screenshot pass shows a single-day band overshooting or undershooting its category, switch to adjacent-day midpoints for `x1`/`x2` instead of the exact category values — do not treat that as a required change now, only as the documented fallback if verification shows it's needed.

- [ ] **Step 4: Create `TrendsCharts.test.tsx` and test `gapSpans` directly**

New file (this file doesn't exist today — confirmed by directory listing):

```tsx
import { describe, it, expect } from 'vitest';
import { gapSpans } from './TrendsCharts';

function point(day: string, delayRate: number | null) {
  return { day, delayRate };
}

describe('gapSpans', () => {
  it('returns no spans when there are no gap days', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', 0.2)])).toEqual([]);
  });

  it('returns a single-day span for one isolated gap day', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null), point('2026-08-03', 0.2)])).toEqual([
      { startDay: '2026-08-02', endDay: '2026-08-02' },
    ]);
  });

  it('merges a multi-day gap into one span', () => {
    expect(
      gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null), point('2026-08-03', null), point('2026-08-04', 0.2)]),
    ).toEqual([{ startDay: '2026-08-02', endDay: '2026-08-04' }]);
  });

  it('returns multiple separate spans for multiple separate gap runs', () => {
    expect(
      gapSpans([
        point('2026-08-01', null),
        point('2026-08-02', 0.1),
        point('2026-08-03', null),
        point('2026-08-04', null),
        point('2026-08-05', 0.2),
      ]),
    ).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-01' },
      { startDay: '2026-08-03', endDay: '2026-08-04' },
    ]);
  });

  it('returns one full-width span when every day is a gap', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', null)])).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-02' },
    ]);
  });

  it('handles a leading gap flush against the start of the range', () => {
    expect(gapSpans([point('2026-08-01', null), point('2026-08-02', 0.1)])).toEqual([
      { startDay: '2026-08-01', endDay: '2026-08-01' },
    ]);
  });

  it('handles a trailing gap flush against the end of the range', () => {
    expect(gapSpans([point('2026-08-01', 0.1), point('2026-08-02', null)])).toEqual([
      { startDay: '2026-08-02', endDay: '2026-08-02' },
    ]);
  });

  it('returns no spans for an empty points array', () => {
    expect(gapSpans([])).toEqual([]);
  });
});
```

This directly covers every case the design's own Testing section and Error handling section call for: no gaps, one isolated gap day, multiple separate runs, an all-gap dataset, leading gaps, trailing gaps.

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsCharts.test.tsx
git commit -m "Render shaded ReferenceArea bands across contiguous sparse-data gap-day runs"
```

---

### Task 5: Right-edge x-axis padding (Decision 5)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:** no new exports — one new prop on both `LineChart` calls.

**Depends on:** Task 4 (same file, serial execution).

- [ ] **Step 1: Add `xAxisProps` to both charts**

```tsx
xAxisProps={{ padding: { right: 12 } }}
```

Add to both the rate chart and the average-delay chart's `LineChart` calls. No `padding.left` — the design found the left edge isn't clipped (the y-axis's own rendered tick-label width already provides clearance there), so adding unneeded left padding would be speculative symmetry, not a fix for anything observed. `12` is chosen as comfortably larger than the default dot radius + stroke width (`r: 3` + `strokeWidth: 1`, both confirmed against the real `recharts`/`@mantine/charts` install — see Status note above) while small relative to the chart's plotted width.

- [ ] **Step 2: Extend the test mock and add the assertion**

Extend the mock (from Tasks 2/3) to also capture `xAxisProps`, then add:

```tsx
it('insets the right edge of the x-axis on both charts so the last dot stops clipping', async () => {
  vi.mocked(api.getLineDailyStats).mockResolvedValue([row({ day: '2026-08-01' })]);
  renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

  for (const [props] of lineChartMock.mock.calls) {
    expect(props.xAxisProps).toEqual({ padding: { right: 12 } });
  }
});
```

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test`
Expected: PASS. Note (Global Constraints): this proves the prop reached the mock, not that `12` is visually the right number — confirm during Task 7 and adjust if it's still too tight or noticeably over-padded.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsResults.test.tsx
git commit -m "Add right-edge x-axis padding to stop the last plotted point clipping"
```

---

### Task 6: Empty-state whitespace — investigate first, then fix (Decision 7)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsResults.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:** no new exports — the empty-state branch's JSX changes (line 41).

**Depends on:** Task 5 is a same-file-region dependency for the test file only; the `TrendsResults.tsx` edit itself has no code dependency on Tasks 1-5.

**This task starts with investigation, not a code change** — the design's Open question 3 explicitly states it could not confirm from the code it read whether Finding 7's "dead whitespace below the footer" is a real persistent bug or a transient SSR-streaming artifact (the `<Skeleton>` fallbacks at `app/lines/[id]/page.tsx:181` / `history/page.tsx:138` are Suspense **loading** fallbacks that React replaces entirely once `TrendsResults` resolves — no `min-height`/footer-pinning rule was found in `globals.css`, and the zero-rows branch itself, `TrendsResults.tsx:41`, is one bare `<Text>` with no height styling of its own). Do not skip straight to the cosmetic wrap below without first checking whether a real mechanism explains the screenshot.

- [ ] **Step 1: Reproduce live and determine which case this is**

Using a real dev server (see Task 7's setup), load a line whose `getLineDailyStats` call resolves to zero rows for the active date range (or force this via a network/API mock for a controlled repro). Compare:
  - A screenshot taken **after** the page has fully hydrated and `TrendsResults` has resolved (well past any Suspense flash).
  - A screenshot taken **during** the loading flash itself (throttle the network in devtools, or add a temporary artificial delay to the fetch, to catch the `<Skeleton height={280|320}>` fallback mid-render).

If the whitespace is present only in the second (transient) case and disappears once resolved: this confirms the design's own leading hypothesis (a transient SSR-streaming artifact, not a persistent bug) — proceed to Step 3's cosmetic fix, which is worth doing either way per the design.

If the whitespace is **still visible after full resolution**: this contradicts the design's hypothesis and means a real mechanism exists that wasn't found in the files read during the design session. Before writing any fix, use browser devtools (or Playwright's `browser_evaluate`) to inspect computed styles on every ancestor of the empty-state `<Text>` for a non-zero `min-height`/`height` that doesn't come from content — check `globals.css` more broadly than the design's session did (it only checked for a `body`/root rule), and check any CSS Module scoped to this route. Document what's found.

- [ ] **Step 2: Fix the real cause, if Step 1 found one**

If Step 1 found a genuine persistent mechanism (a CSS rule, a layout container with a forced min-height, etc.), fix that directly — the specific edit depends entirely on what Step 1 finds, so it isn't prescribed here. Skip this step if Step 1 confirmed the transient-artifact hypothesis instead.

- [ ] **Step 3: Apply the bounded empty-state treatment (worth doing regardless of Step 1's outcome)**

In `TrendsResults.tsx`, replace the bare empty-state line (currently line 41):

```tsx
if (stats.length === 0) {
  return (
    <Paper withBorder p="md">
      <Text c="dimmed">Not enough sampled data yet for this line.</Text>
    </Paper>
  );
}
```

Add `Paper` to the existing `@mantine/core` import (currently `import { Stack, Text, Title } from '@mantine/core';`, line 1). No forced `min-height` matching the chart skeletons — per Decision 7, this doesn't literally shrink whitespace below it, it makes the empty state read as a deliberately-finished component rather than a chart that failed to render.

- [ ] **Step 4: Extend the existing empty-state test**

Extend the existing test (currently lines 76-81 of `TrendsResults.test.tsx`):

```tsx
it('renders the empty state when there are no rows, inside a bounded container', async () => {
  vi.mocked(api.getLineDailyStats).mockResolvedValue([]);
  renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
  const text = screen.getByText('Not enough sampled data yet for this line.');
  expect(text).toBeInTheDocument();
  expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
  // Confirms the bounded wrapper, not just the text -- Mantine's Paper renders a div with its own class.
  expect(text.closest('.mantine-Paper-root')).not.toBeNull();
});
```

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test`
Expected: PASS.

- [ ] **Step 6: Commit**

Commit message depends on what Step 1/2 found — if only the cosmetic wrap was needed:

```bash
git add frontend/app/lines/[id]/history/TrendsResults.tsx frontend/app/lines/[id]/history/TrendsResults.test.tsx
git commit -m "Give the Trends empty state a bounded, self-contained visual treatment"
```

If Step 2 also fixed a real found mechanism, split into two commits (the root-cause fix, then the cosmetic wrap) so each is independently reviewable.

---

### Task 7: Screenshot verification of every visual-only fix (Decisions 1, 2, 4, 5, and Task 6's outcome)

**Files:** none modified by this task (verification only), unless screenshots are checked in as fixtures — not called for by the design, so treat that as out of scope unless separately requested.

**Depends on:** Tasks 1-6, all landed and committed.

Per Global Constraints: this repo's Vitest suite mocks `@mantine/charts` wholesale and never renders real Recharts output. Tasks 1-6's unit tests only prove the right props were passed — they are not evidence that the tooltip, legend, gap bands, or edge padding actually render correctly. This task is where that's actually checked, against a real dev server, the way the design's own stage-1/stage-2 review was itself screenshot-based rather than a Vitest run. **Do not consider this plan complete until this task's checks pass** — `npm test` passing across Tasks 1-6 is necessary but not sufficient.

No original screenshot evidence from the design's stage-1/stage-2 pipeline survives on disk in this session (confirmed — see Status note above) — this task captures a fresh baseline rather than diffing against originals that no longer exist.

- [ ] **Step 1: Bring up a real, data-backed dev server**

Per this repo's `README.md` "Running it" section: bring up the local stack via `docker-compose.yml`/`docker-compose.dev.yml`, then run `npm run dev` in `frontend/`. Confirm (via the API or existing seed data) that at least:
  - one line has a real multi-day `LineDailyStats` history including at least one day with `sampleCycles < 20` (a genuine sparse-data gap, to exercise Task 4), ideally including both an isolated single-day gap and a multi-day gap run;
  - one line/date-range resolves to zero rows (to exercise Task 6's empty state).

If no such data exists yet in the dev environment, that's a blocker to resolve before this task can actually verify Task 4/6 — flag it rather than skipping the check.

- [ ] **Step 2: Capture the fix set at desktop width**

Using the `run` skill (or directly via the Playwright MCP tools — `browser_navigate`, `browser_hover`, `browser_take_screenshot`) against the dev server from Step 1:
  - `/lines/{id}/history` Trends tab: hover a data point on each chart and screenshot the tooltip (Task 1/Decision 1).
  - Screenshot the rate chart showing its legend and the three distinguishable dash patterns (Task 2/Decision 2).
  - Screenshot the right edge of each chart, zoomed if useful, confirming the last point/dot is no longer clipped against the plot boundary (Task 5/Decision 5).
  - Screenshot a gap span — both the isolated single-day case and a multi-day run if the seed data has both — confirming the shaded band covers the intended day(s) without spilling into neighboring valid days (Task 4/Decision 4, the specific risk flagged in Task 4 Step 3).
  - Screenshot the empty-state line/date-range, confirming Task 6's bounded treatment renders as intended.

- [ ] **Step 3: Repeat at this repo's own 390px mobile reference width**

Using `browser_resize` (or equivalent) to 390px — this repo's own established mobile baseline (`frontend/app/layout.tsx:138`, `frontend/app/globals.test.ts:86`, `frontend/app/lines/AllLinesTable.tsx:218`, all cited by the design as precedent for this exact width) — repeat the same screenshot set, paying particular attention to:
  - whether the rate chart's legend wraps onto a second row (design's Open question 2), and whether the `h` bump from Task 2 Step 2 (`280 → 310`) actually accommodates it without compressing the plot area — adjust `h` and re-screenshot if not.
  - whether the gap bands and right-edge padding still read correctly at this narrower width.

- [ ] **Step 4: Confirm the embedded preview inherits every fix with no separate work**

Screenshot `/lines/{id}` (both desktop and 390px) for a line with the same seeded gap/empty-state data, confirming its embedded 7-day Trends preview shows the same tooltip/legend/gap-band/edge-padding fixes with no code changes of its own — this is the live confirmation of Decision 6 (already established by reading `page.tsx` in full — Status note above — but worth one visual spot-check since this is the surface the original review screenshotted).

- [ ] **Step 5: Triage any fix that doesn't hold up**

For any of the above that doesn't look right in the real render: do not silently patch it inside this verification task. Re-open the relevant task above (re-run its steps with a corrected value/approach — e.g. adjusted dash patterns, a corrected `h`, `ReferenceArea` `x1`/`x2` switched to adjacent-day midpoints per Task 4 Step 3's documented fallback, a different `padding.right` value, or a revisit of Task 6 Step 1's investigation if the empty-state fix didn't actually address what the screenshot shows), following superpowers:systematic-debugging rather than guessing a second time.

- [ ] **Step 6: Final full-suite check**

Run (from `frontend/`): `npm test && npm run build`
Expected: PASS — confirms no regression across all six tasks' combined prop/JSX changes, on top of Steps 1-5's visual confirmation.

No commit for this task by itself (verification-only, per the Files note above) — if Step 5 required reopening an earlier task, that task's own commit step covers it.

---

## Not in this plan

Carried forward from the design's own "Explicitly out of scope," not silently dropped:

- Combining the rate chart and average-delay chart into one chart/axis — `TrendsCharts.tsx`'s own comment (lines 34-37) already documents why they're deliberately kept separate (different units); unrelated to any of the seven findings.
- Changing `SPARSE_DATA_FLOOR_CYCLES`'s value (still `20`, still an explicitly-flagged placeholder, `TrendsResults.tsx:13`) — orthogonal to how gap days are *rendered*, which is all Task 4 addresses.
- A custom per-point dot marker for isolated gap-adjacent points — considered and rejected by the design on real API grounds (`dotProps` is series-wide, not per-point, in Mantine's typed surface — re-confirmed against the real `9.5.2` install, Status note above).
- A bespoke hover tooltip/annotation specifically for `null` gap points — Task 4's shaded band is a background/context affordance, not a second tooltip mechanism.
- Changing the `<Skeleton>` fallback heights (`280`/`320`, `page.tsx:181`/`history/page.tsx:138`) themselves — Task 6 only touches the *resolved* empty-state render, not the loading-state skeletons.
- Any change to `TrendsCharts.tsx`'s existing Server/Client boundary doc-comment rationale (lines 7-26) — unrelated to these seven findings.
- Checking screenshots into the repo as a permanent visual-regression fixture set — not called for by the design; Task 7 captures screenshots for this plan's own verification, not as a new ongoing test asset, unless separately requested later.
