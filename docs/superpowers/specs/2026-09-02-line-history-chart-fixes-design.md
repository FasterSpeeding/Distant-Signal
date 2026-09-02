# Design: Line History "Trends" Chart Fixes

**Status: design proposal, not approved.** Stage 3 of a 6-stage pipeline
(screenshot capture → critical review → this design spec → plan → implement
→ coordinator review-and-merge). This spec designs concrete fixes for seven
real, screenshot-confirmed problems a stage-2 critical UX review found in
the two Mantine `LineChart` components that render `/lines/{id}/history`'s
"Trends" tab and its embedded 7-day preview on `/lines/{id}`. No
implementation plan or code is included here — that is stage 4.

## Goal

Fix all seven findings from the stage-2 review, in the shared chart layer
(`TrendsCharts.tsx`/`TrendsResults.tsx`) so both the full history page and
the embedded preview inherit every fix automatically, without inventing new
chart infrastructure beyond what `@mantine/charts@9.5.2` (the version this
repo actually has pinned, `frontend/package.json:14`) already exposes.

## Verification method

This worktree has no installed `node_modules` for the frontend (confirmed:
`ls node_modules` under `frontend/` is empty), so `@mantine/charts`' and
`recharts`' real current APIs could not be read from a local install.
Instead, every claim below about their actual behavior was verified by
downloading the **exact pinned versions** —
`@mantine/charts@9.5.2`/`@mantine/core@9.5.2` and `recharts@^3.2.1` (pinned
to `3.2.1` in `frontend/package.json:14-15,23`) — from the npm registry via
unpkg, and reading their real shipped type definitions (`lib/*.d.ts`), the
actual built ESM source (`esm/**/*.mjs`) for the components in question, and
`@mantine/charts`' own shipped `styles.css`. None of the `@mantine/charts`
or `recharts` claims in this document rely on general/possibly-stale
familiarity with either library — each is cited against the specific file
fetched this session (see inline citations, e.g. "`@mantine/charts@9.5.2`
`esm/ChartTooltip/ChartTooltip.mjs`").

## Current relevant state (verified this session)

- **`frontend/app/lines/[id]/history/TrendsCharts.tsx`** (read in full):
  a Client Component rendering two `@mantine/charts` `LineChart`s from one
  `points: ChartPoint[]` array. The rate chart (lines 38-49) renders three
  series (`delayRate`/`cancellationRate`/`skipRate`, each just a `color`
  and `label`, no other differentiation) at `h={280}`, with
  `valueFormatter={(value) => \`${(value * 100).toFixed(1)}%\`}` (line 47)
  and `connectNulls={false}` (line 48). The average-delay chart (lines
  51-62) renders one series at `h={220}`, with `connectNulls={false}` but
  **no `valueFormatter` prop at all** (compare lines 55-61 to the rate
  chart's line 47) — the tooltip therefore falls back to whatever raw
  number is in the data, which is exactly the "raw unrounded float" Finding
  3 describes. Neither `LineChart` call passes `withLegend`, `legendProps`,
  `lineChartProps`, `xAxisProps`, `dotProps`, or `children` — none of these
  props are used anywhere in this file today.
- **`frontend/app/lines/[id]/history/TrendsResults.tsx`** (read in full):
  an async Server Component. `toChartPoints` (lines 23-35) turns any day
  with `sampleCycles < SPARSE_DATA_FLOOR_CYCLES` (`20`, line 13, an
  explicitly-flagged placeholder) into a **gap**: `delayRate`,
  `cancellationRate`, `skipRate`, and `avgDelayMinutes` are all set to
  `null` together for that day (never independently). A completely empty
  `stats` array short-circuits to a plain `<Text c="dimmed">Not enough
  sampled data yet for this line.</Text>` (line 41) before any `LineChart`
  is rendered at all. When `stats.length > 0`, the honesty copy at lines
  59-64 ("Days with too little coverage show as a gap rather than a
  misleading flat line") is the **only** place this app currently explains
  gap semantics — nothing inside the chart itself does.
- **`frontend/app/lines/[id]/history/chartPoint.ts`** (read in full): the
  shared `ChartPoint` type — `day: string`, three `number | null` rate
  fields, `avgDelayMinutes: number | null`, `sampleCycles: number`.
- **`frontend/app/lines/[id]/page.tsx`** (read in full): the embedded 7-day
  preview. Lines 161-184 render `<TrendsResults id={id} from=... to=...
  />` under a `<Suspense fallback={<Skeleton height={280} />}>` — the exact
  same `TrendsResults`/`TrendsCharts` components as the full history page,
  unmodified, per the comment at lines 165-170 ("Reuses
  `/lines/[id]/history`'s own Trends-tab component wholesale"). No
  preview-specific styling or props exist anywhere in this file.
- **`frontend/app/lines/[id]/history/page.tsx`** (read in full): the full
  Trends tab. Line 138 wraps the same `<TrendsResults>` call in
  `<Suspense fallback={<Skeleton height={320} />}>`, inside a `Tabs`
  structure (lines 95-143).
- **The tooltip's missing background/box, root cause.** `frontend/app/
  globals.css:1-2` imports exactly two Mantine stylesheets —
  `@import '@mantine/core/styles.css'` and `@import '@mantine/dates/
  styles.css'` — and `frontend/app/layout.tsx:1` imports only `'@/app/
  globals.css'`. **No file in this repo imports `@mantine/charts/
  styles.css`** (confirmed by `grep -rn "mantine/charts/styles"` across
  the whole `frontend/` tree, zero matches) or `@mantine/charts/
  styles.layer.css`. `@mantine/charts@9.5.2`'s own `package.json` declares
  `"sideEffects": ["*.css"]` and exports both `./styles.css` and
  `./styles.layer.css` as separate entry points — exactly the pattern
  where a consumer must explicitly import one of them, the same pattern
  this repo's own `globals.css` already correctly follows for
  `@mantine/core` and `@mantine/dates`, just not for `@mantine/charts`.
  Reading `@mantine/charts@9.5.2 esm/ChartTooltip/ChartTooltip.mjs`
  (lines ~50-108) confirms `ChartTooltip` is a plain, otherwise-unstyled
  `<div>` tree — every visual property (background, box-shadow, border,
  `min-width`, padding, the flex layout that keeps each series' row on one
  line, the spacing between rows) comes entirely from Mantine's `useStyles`
  CSS-module class lookup, resolving to static, content-hashed class names
  (`ChartTooltip.module.mjs`: `tooltip → "m_e4d36c9b"`, `tooltipItem →
  "m_3de8964e"`, etc.) that are always applied to the DOM regardless of
  whether the backing stylesheet is loaded. The shipped `styles.css`
  confirms exactly what those classes provide: `.m_e4d36c9b` sets
  `min-width: calc(12.5rem * var(--mantine-scale))`, `background-color:
  var(--mantine-color-body)`, `box-shadow: var(--mantine-shadow-md)`,
  `border-radius`, and a themed `border`; `.m_3de8964e:where(.m_3de8964e +
  .m_3de8964e)` sets the `margin-top` between stacked tooltip rows. Without
  the stylesheet import, every one of these rules is absent — the tooltip
  renders as bare, unstyled `<div>`s with none of that box model, which
  matches the reviewed symptom exactly: no visible background/border, and
  content that isn't constrained to a defined box so it visually spills
  into whatever renders below it (the second chart, on mobile the footer).
  **This is diagnosis (a) from the task brief — a missing stylesheet
  import, not a component misconfiguration** — confirmed by reading the
  actual component source, not assumed.
- **`GridChartBaseProps`** (`@mantine/charts@9.5.2 lib/types.d.ts`, read in
  full) — the interface `LineChartProps` extends — is where every prop
  needed for Findings 1-5 actually lives: `withLegend?: boolean` (default
  `false`, per its doc comment), `legendProps?: Omit<LegendProps, 'ref'>`,
  `withTooltip?: boolean` (default `true`), `tooltipProps?:
  Omit<TooltipProps<any,any>, 'ref'>`, `valueFormatter?: (value: number) =>
  string` (doc comment: "A function to format values on Y axis **and
  inside the tooltip**"), and `xAxisProps?: Omit<XAxisProps, 'ref'>`.
  `LineChartProps` itself (`lib/LineChart/LineChart.d.ts`) additionally
  exposes `dotProps?: MantineChartDotProps`, `lineChartProps?:
  React.ComponentProps<typeof ReChartsLineChart>` (passed straight to
  Recharts' own `LineChart`), and `children?: React.ReactNode`
  ("Additional components that are rendered inside recharts `LineChart`
  component"), and each `LineChartSeries` entry may carry its own
  `strokeDasharray?: string | number`.
- **Legend default position.** `@mantine/charts@9.5.2 esm/LineChart/
  LineChart.mjs` (~line 163) renders `<Legend verticalAlign="top" ...
  {...legendProps}>` — Mantine's own default is `top`, overriding
  Recharts' own component default of `bottom` (confirmed in `recharts@3.2.1
  es6/component/Legend.js`'s `defaultProps`), and `legendProps` is spread
  last so it can still be overridden. The Mantine docs page for
  `LineChart` (fetched this session) confirms this directly: "setting
  `legendProps={{ verticalAlign: 'bottom', height: 50 }}` will render the
  legend at the bottom of the chart." The legend's own CSS
  (`styles.css` `.m_847eaf`) is `display: flex; flex-wrap: wrap;
  justify-content: flex-end` — it wraps onto additional rows rather than
  overflowing or truncating when horizontal space is tight.
- **Tooltip clipping/right-edge dot clipping, root cause.** Neither
  `TrendsCharts.tsx` call passes `lineChartProps` or `xAxisProps`, so both
  charts use Recharts' own defaults. `recharts@3.2.1 es6/chart/
  CartesianChart.js` (`defaultMargin`) confirms the chart's default margin
  is `{ top: 5, right: 5, bottom: 5, left: 5 }`. `recharts@3.2.1
  es6/cartesian/Line.js` confirms the default dot radius is `r: 3` when no
  `dotProps.r` override is given (matching this repo's code — neither
  chart passes `dotProps`), rendered with `strokeWidth: 1` per Mantine's
  own dot default (`@mantine/charts esm/LineChart/LineChart.mjs`, the
  `dot: withDots ? {..., strokeWidth: 1, ...dotProps}` block). Both charts
  use a category x-axis (`dataKey="day"`, string values), and neither
  passes `xAxisProps.padding`, so Recharts positions the first and last
  category ticks exactly at the plot area's left/right edges with no
  inset. The **left** edge isn't visibly clipped because the y-axis's own
  rendered width (tick label text) already sits between the chart's
  nominal 5px left margin and the actual leftmost plotted point, giving it
  real clearance; the **right** edge has no equivalent — neither chart
  passes `withRightYAxis`, so nothing occupies that space, and the
  rightmost point's ~3.5-4px dot radius sits right against the 5px margin
  boundary with effectively no room, producing exactly the "clipped on the
  right, fully visible on the left" asymmetry the review found.
  `recharts@3.2.1 types/state/cartesianAxisSlice.d.ts` confirms `XAxis`'s
  `padding` prop accepts `{ left?: number; right?: number }` — a real,
  targeted way to inset category-axis endpoints from the plot boundary.
- **Gap-day affordance was designed once already, and never shipped.** The
  original chart spec this rollup builds on
  (`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md`,
  Decision 3, lines 369-384) proposed exactly this: a sparse day "rendered
  as a gap in the chart (no point plotted, not a zero, not an interpolated
  line across it) **with a small dimmed marker a viewer can hover for
  'limited data this day.'**" `toChartPoints` (`TrendsResults.tsx:23-35`)
  implemented the gap half of that (nulling the fields) but never
  implemented the marker/annotation half — there is no code anywhere in
  `TrendsCharts.tsx` or `TrendsResults.tsx` that renders anything for a
  gap day beyond the absence of a point. Finding 4 is this known,
  documented gap in the original design finally being confirmed visually.
- **Testing precedent.** `frontend/app/lines/[id]/history/
  TrendsResults.test.tsx` (read in full) mocks `@mantine/charts` wholesale
  (lines 15-31): `LineChart` is replaced with a plain `<div
  data-testid="line-chart">` that surfaces only `series`, `connectNulls`,
  and `data` as `data-*` attributes, with an explicit comment (lines 15-21)
  stating this repo's convention is **not** to assert on Recharts' own SVG
  output. The existing suite covers `toChartPoints`'s gap logic (lines
  49-73), the empty state (lines 76-81), gap days not rendering as zero
  (lines 83-100), the two-chart split (lines 102-129), and
  `connectNulls={false}` on both charts (lines 131-139) — nothing about
  tooltip styling, legend, formatting, or edge clipping, since none of
  that was in scope before now. `frontend/app/lines/[id]/page.test.tsx`
  (lines 29-33) and `frontend/app/lines/[id]/history/page.test.tsx` (lines
  20-22) mock `@mantine/charts` similarly but capture even less (`series`
  only, or nothing) — both are page-composition tests, not chart-behavior
  tests.
- **The 390px mobile viewport is an established reference width in this
  codebase**, not invented for this spec: `frontend/app/layout.tsx:138`
  ("40px of gutter on a 390px screen"), `frontend/app/globals.test.ts:86`
  (a WCAG check "at 390px"), and `frontend/app/lines/AllLinesTable.tsx:218`
  ("At 390px five columns cannot all fit") all cite it as this repo's own
  mobile baseline.

## Decisions

### 1. Tooltip: import `@mantine/charts/styles.css` globally, alongside the existing Mantine stylesheet imports

**Chosen: add `@import '@mantine/charts/styles.css';` to `frontend/app/
globals.css`**, directly next to the existing `@import '@mantine/core/
styles.css'` and `@import '@mantine/dates/styles.css'` at lines 1-2. This
is not a dependency change — `@mantine/charts` is already an installed,
pinned dependency (`frontend/package.json:14`) that is already imported and
rendered (`TrendsCharts.tsx:3`); it is missing exactly one wiring step this
repo already performs correctly for its other two Mantine packages that
ship their own stylesheet. Given the root-cause diagnosis above (a
genuinely unstyled, class-name-correct-but-CSS-absent tooltip, not a
misconfigured component), this one-line addition is expected to resolve
the entire Finding 1 symptom on its own: background, box-shadow, border,
`min-width`, and the row-spacing rules all come from this single
stylesheet, with no `ChartTooltip`-specific props needed anywhere in
`TrendsCharts.tsx`.

**Alternative considered and rejected: pass explicit inline styling via
`tooltipProps`/a custom `Tooltip content` render function.** Rejected —
this would hand-roll, in this app's own code, a worse copy of exactly what
Mantine's shipped `ChartTooltip` + `styles.css` already provide correctly
(min-width, theming-aware colors via `--mantine-color-*` variables that
already respect light/dark mode, consistent spacing). It would also leave
every other `@mantine/charts` component this repo might add later (legend
included — see Decision 2) still unstyled, since the same missing-import
root cause applies to `ChartLegend` too. The stylesheet import fixes both
at once with strictly less code.

Only one real open question remains: this repo's existing Mantine imports
use the plain (non-layered) `styles.css` variant, not `styles.layer.css`
(CSS `@layer`-wrapped). For consistency, this spec recommends the same
plain `styles.css` variant for `@mantine/charts`. Whether mixing that with
the two already-layered-or-not Mantine imports produces any cascade
surprise could not be verified in this session (no way to run the app) —
flagged in Open questions/risks; implementation should screenshot-verify
this specific change in isolation before layering the other fixes on top,
since it's expected to be the single highest-impact change in this spec.

### 2. Legend: `withLegend` on the rate chart only, plus per-series dash patterns for non-color differentiation

**Chosen: `withLegend={true}` on the three-series rate chart, left at its
Mantine default position (top, per the verified default above — no
`legendProps.verticalAlign` override needed), combined with a distinct
`strokeDasharray` per series** (e.g. delay rate solid/default, cancellation
rate dashed, skip rate dotted — exact dash values are an implementation
detail, not fixed here). **Not added to the single-series average-delay
chart** — a legend mapping one color to one already-titled line ("Average
delay (minutes)", `TrendsCharts.tsx:53`) adds no information a legend
could resolve; Finding 2 is specifically about the *three-series* chart.

**Placement — top vs. bottom — real alternatives weighed:**
- **Bottom** (Mantine's non-default, via `legendProps={{ verticalAlign:
  'bottom' }}`). Considered because it keeps the chart's plotted lines the
  first thing a viewer's eye reaches. **Rejected**: this chart is reached
  after a `Title order={4}` heading ("Delay / cancellation / skip rate")
  that already announces what the three lines are about; a legend
  immediately below that heading and above the plot establishes the
  color→series mapping *before* the eye reaches the lines, which is the
  point of fixing Finding 2 (a legend a viewer only reaches after already
  having looked at three unlabeled colored lines is a weaker fix than one
  they see first).
- **Top** (Mantine's default, zero extra config). **Chosen** for the
  reasoning above, and because it requires no `legendProps` override at
  all — smaller diff, and it directly matches the one placement example
  the Mantine docs demonstrate as the non-default choice (bottom), implying
  top is the well-trodden path.

**Color-only vs. color+dash for accessibility — real alternatives
weighed:**
- **Legend alone, hue unchanged on the lines themselves.** Considered,
  since Finding 2's core complaint (no legend at all, "differentiated only
  by hue... in the chart's default, non-hover state") is arguably resolved
  by the legend alone: a viewer can now match "blue = delay rate" once and
  remember it. **Rejected as insufficient given the review's own explicit
  accessibility framing** ("differentiated only by hue" is named as the
  problem, not just "no legend") — a legend fixes *lookup* but does
  nothing for a colorblind viewer or a grayscale/print rendering trying to
  tell the three *plotted lines* apart while looking at the chart body,
  away from the legend.
- **Per-series `strokeDasharray` (solid/dashed/dotted) in addition to the
  legend.** **Chosen.** `LineChartSeries.strokeDasharray` is a real,
  already-typed, zero-extra-dependency prop (`@mantine/charts@9.5.2
  lib/LineChart/LineChart.d.ts`) — this is a small, concrete addition, not
  new infrastructure. Three visually distinct dash patterns plus color
  plus a color-keyed legend gives full redundant coding (shape *and*
  color *and* a lookup), directly answering the review's accessibility
  concern rather than only its lookup complaint.

**Height budget, flagged not resolved precisely.** `LineChart`'s `h` prop
sets the whole component's box height (data, legend, axes, and plot area
all share it via Recharts' `ResponsiveContainer`) — adding a legend inside
the existing `h={280}` shrinks the actual plotted area by however tall the
legend renders, which at 390px mobile width may wrap the three legend
items (`.m_847eaf`'s `flex-wrap: wrap`, confirmed above) onto two rows
rather than one. This spec recommends implementation bump the rate chart's
`h` modestly (a starting guess, not measured: `280` → roughly `310-320`) to
absorb one extra legend row without compressing the plot itself, and
confirm the actual wrapped height against a real 390px screenshot before
settling on a final number — this could not be measured in this session
(no way to render the app). The average-delay chart's `h={220}` is
unaffected (no legend added there).

### 3. Average-delay tooltip: add the missing `valueFormatter`

**Chosen: add `valueFormatter={(value) => \`${value.toFixed(1)} min\`}`**
to the average-delay `LineChart` (`TrendsCharts.tsx:55-61`), matching the
exact pattern the rate chart already uses one call above it (line 47) and
relying on the same verified `GridChartBaseProps.valueFormatter` behavior
("formats values on Y axis and inside the tooltip" —
`@mantine/charts@9.5.2 lib/types.d.ts`) that already makes the rate
chart's tooltip show `"12.3%"` instead of a raw `0.123`.

**Precision — one decimal place, justified concretely.** One decimal
place on minutes is roughly 6-second granularity, which is more than
enough resolution for a value that is itself an average across many
delayed services in a day (not a precise single-train measurement) — the
`0.41267123328767123`-style raw float the review found is float noise
inherited from a database average, not meaningful precision. `toFixed(1)`
also directly mirrors the rate chart's own existing `toFixed(1)` call
(line 47), so the two charts stay visually consistent in how precisely
they report numbers, which the review explicitly flagged as currently
inconsistent (Finding 3's own wording: "inconsistent with the rate
chart's own already-formatted tooltip"). The `" min"` suffix is kept even
though the chart's `Title` already says "(minutes)" (line 53), for the
same reason the rate chart's tooltip still appends `%` despite its own
title saying "rate" — each chart's convention is to make every tooltip
value self-labeled, not dependent on the viewer having read the heading.

### 4. Gap-day rendering: a shaded background band across each contiguous gap span, via `ReferenceArea` in the existing `children` slot

**Chosen: derive contiguous runs of gap days from `points` (any day where
`delayRate === null`, which `toChartPoints` guarantees is set exactly when
every other field on that `ChartPoint` is also nulled — `TrendsResults.tsx
23-35`) and render one `<ReferenceArea>` per run inside each `LineChart`'s
`children` slot** (`children?: React.ReactNode`, confirmed real on
`LineChartProps` above; `ReferenceArea` confirmed exported from
`recharts@3.2.1`), spanning `x1`/`x2` from the run's first to last day, a
low-opacity neutral fill (no stroke), `ifOverflow="visible"`. A single
isolated gap day (the exact case the review screenshot caught — one
orphaned valid dot with no visible reason why) gets the same treatment as
a multi-day gap: a thin band around it — **one mechanism, no special-cased
"isolated point" branch**. This also means a day range where *every* day
falls below the sparse-data floor (an edge case not previously flagged)
naturally renders as one full-width band rather than needing its own
handling — validated by the same derivation logic, not a separate code
path (see Error handling below).

This is also, in effect, finally implementing the "small dimmed marker...
hover for 'limited data this day'" affordance the *original* chart spec
already called for (`2026-08-31-line-history-graphics-design.md`, Decision
3) and that never shipped — not a new idea invented for this review, a
follow-through on one already on record.

**Real alternatives weighed:**
- **In-chart text annotation/label directly on or near the gap.**
  Considered — closest to the original spec's literal "hover for 'limited
  data this day'" wording. **Rejected as the primary mechanism** (though
  a short static label on the band is worth keeping if it fits — see
  below): a 7-30 day range can have multiple separate gap spans, and
  cramming a readable text label into each one, especially at 390px
  mobile width where the whole chart is already tight (Finding 6/the
  review's own mobile screenshots), risks becoming its own clutter
  problem — the exact category of bug this whole spec is fixing.
  Critically, the page **already carries a clear, always-visible
  explanation** directly above the chart (`TrendsResults.tsx:59-64`,
  "Days with too little coverage show as a gap rather than a misleading
  flat line") — a shaded band's job is to visually connect what a viewer
  sees in the chart back to prose they've already read once, not to
  repeat that prose a second time inside the plot area itself. If a
  band happens to be wide enough at implementation time to fit a small
  static caption ("limited data") without crowding, that's a reasonable
  bonus, not a requirement of this decision.
- **A distinct dot/marker style for the isolated valid point itself**
  (rather than shading the gap around it). **Rejected on real, verified
  API grounds**: Mantine's typed `dotProps: MantineChartDotProps` is
  applied uniformly to every dot on a `<Line>` (`@mantine/charts@9.5.2
  esm/LineChart/LineChart.mjs`: `dot: withDots ? {..., ...dotProps}`) —
  there is no supported way, within Mantine's typed props, to style one
  specific data point differently from its neighbors. Doing so would
  require dropping out of `dotProps` entirely into a fully custom
  Recharts `dot={(props) => <CustomDot .../>}` render function passed via
  `lineProps`/raw Recharts composition — a materially larger, riskier
  change than this fix warrants, for a problem the shaded-band approach
  already solves without it.
- **Explanatory text only in the page copy above the chart (status
  quo).** Rejected outright — this is exactly the state Finding 4
  criticizes; the whole point is that the chart itself gives no visual
  affordance today.

### 5. Right-edge clipping: `xAxisProps={{ padding: { right: 12 } }}` on both charts

**Chosen: pass `xAxisProps={{ padding: { right: 12 } }}` to both `LineChart`
calls in `TrendsCharts.tsx`.** `12` is chosen as comfortably larger than
the verified worst case (default dot radius `r: 3` + `strokeWidth: 1`,
`recharts@3.2.1 es6/cartesian/Line.js`/Mantine's dot default), while small
relative to the chart's overall plotted width so it doesn't meaningfully
compress the visible date range — an implementation-time visual check
against a real render (not possible in this session) should confirm `12`
is neither too tight (still clips) nor unnecessarily generous.

**Alternative considered and rejected: widen the whole chart's right
margin via `lineChartProps={{ margin: { right: N } }}`.** This would also
work (it increases the same default-`5`px margin identified as the root
cause), but it's a less targeted fix than `xAxisProps.padding`: `margin`
shifts the *entire* plot area's right boundary inward, including the
y-axis-side asymmetry that isn't actually broken (the left side already
has natural clearance from the y-axis's own rendered width, per Current
relevant state) — `xAxisProps.padding.right` insets only the endpoints of
the category axis itself, which is the precise mechanism actually causing
the clipping (Recharts places the last category tick exactly at the plot
boundary with zero inset by default). No `padding.left` is added, since
the review didn't find the left edge broken and the current natural
y-axis clearance already handles it — adding unneeded left padding would
only be speculative symmetry, not a fix for anything observed.

### 6. Shared-layer confirmation: all five fixes land in `TrendsCharts.tsx` (plus one global CSS import); no preview-specific work

All of Decisions 1-5 land in the shared component tree: the `styles.css`
import (Decision 1) is a single global addition to `frontend/app/
globals.css`, already loaded once for the whole app via `frontend/app/
layout.tsx:1`; Decisions 2-5 are all prop changes to the two `<LineChart>`
calls inside `frontend/app/lines/[id]/history/TrendsCharts.tsx`. Verified
this session: the embedded preview (`frontend/app/lines/[id]/page.tsx:
161-184`) renders `<TrendsResults>` unmodified, which itself renders
`<TrendsCharts points={points} />` unmodified (`TrendsResults.tsx:69`) —
there is no separate preview-specific chart code anywhere in
`frontend/app/lines/[id]/page.tsx`, confirmed by reading it in full. No
reason was found to treat the preview any differently; all five fixes
reach it automatically once they land in the shared component.

### 7. Empty-state whitespace: a bounded, self-contained empty-state block, not a literal "stop reserving space" fix

**What was actually verified, stated plainly.** This spec could not
confirm a persistent, CSS-driven "reserved chart-height space" bug in the
code read this session. The two `<Skeleton>` fallbacks that reserve
280px/220px-scale space (`app/lines/[id]/page.tsx:181`, `history/
page.tsx:138`) are Suspense **loading** fallbacks — once `TrendsResults`
resolves (whether to the empty-state text or the real charts), React
replaces the fallback entirely; nothing in either file, nor in
`globals.css` (no `body`/root `min-height` or footer-pinning rule was
found), keeps that space reserved afterward. The zero-rows branch itself
(`TrendsResults.tsx:40-42`) is a single bare `<Text c="dimmed">` line with
no height styling of its own. It's plausible the review's screenshot
caught a transient SSR-streaming flash of the fallback rather than a
persistent state — this could not be distinguished from a live render in
this session (no installed dependencies, no backend service to fetch
against). This is called out explicitly rather than inventing a
mechanism to "fix."

**Chosen fix regardless: give the empty state a small, intentionally
self-contained visual treatment** (e.g. wrap `TrendsResults.tsx:41`'s text
in a modestly-padded bordered/muted block — a `Paper`-style container, no
forced `min-height` matching the chart skeletons) rather than one bare
line of dimmed text. This doesn't literally shrink anything below it —
whatever whitespace legitimately remains on a short page under a tall
desktop viewport is normal end-of-page whitespace, not something to
eliminate — but it makes that remaining space read as *following a
clearly-finished, deliberate component*, rather than as a chart that got
cut off or failed to render, which is what a single short line floating
above a large gap currently suggests. This is a real, useful fix whether
or not the "reserved space" framing turns out to describe a genuine
persistent bug or a transient one — see Open questions/risks.

## Architecture

Before/after prop list for `TrendsCharts.tsx`'s two `LineChart` calls (no
other files change shape beyond the one-line `globals.css` addition):

```
frontend/app/globals.css
  @import '@mantine/core/styles.css';     (existing)
  @import '@mantine/dates/styles.css';    (existing)
+ @import '@mantine/charts/styles.css';   NEW -- Decision 1

frontend/app/lines/[id]/history/TrendsCharts.tsx

  Rate chart (delayRate/cancellationRate/skipRate), h={280 → ~310-320}:
    data, dataKey, series, valueFormatter, connectNulls   (unchanged)
    series[].color, series[].label                        (unchanged)
+   series[].strokeDasharray   NEW -- Decision 2 (one distinct pattern/series)
+   withLegend={true}          NEW -- Decision 2 (default top position kept)
+   xAxisProps={{ padding: { right: 12 } }}   NEW -- Decision 5
+   children: <ReferenceArea> per contiguous gap-day run   NEW -- Decision 4

  Average-delay chart (avgDelayMinutes), h={220, unchanged}:
    data, dataKey, series, connectNulls                    (unchanged)
+   valueFormatter={(v) => `${v.toFixed(1)} min`}   NEW -- Decision 3
+   xAxisProps={{ padding: { right: 12 } }}          NEW -- Decision 5
+   children: <ReferenceArea> per contiguous gap-day run   NEW -- Decision 4
    (no withLegend -- single series, Decision 2 explicitly excludes it)

frontend/app/lines/[id]/history/TrendsResults.tsx
+   empty-state branch (line 41): wrap in a bounded block, not bare Text
                                                     NEW -- Decision 7
    (toChartPoints, honesty copy, gap-nulling logic: unchanged)

frontend/app/lines/[id]/page.tsx
    (no changes -- inherits everything via TrendsResults/TrendsCharts,
     confirmed by Decision 6)
```

A new small pure helper (name/location not fixed here — implementation
choice) derives contiguous gap-day runs from `points` for the
`<ReferenceArea>` children, likely colocated in `TrendsCharts.tsx` since it
only needs `ChartPoint[]`, not the raw `sampleCycles`/floor threshold that
lives server-side in `TrendsResults.tsx`.

## Error handling

- **All-gap dataset** (every day in range falls below the sparse-data
  floor, `stats.length > 0` but every field nulled): the gap-span
  derivation in Decision 4 naturally produces one run spanning the whole
  chart width — no special-case branch needed, and no line/dots render at
  all (already true today via `connectNulls={false}`), which is an honest
  signal ("the whole visible range had insufficient coverage"), not a
  crash or a blank-looking chart with no explanation.
- **Leading/trailing gap runs** (the very first or last day(s) in range
  are gaps): the shaded band sits flush against the chart's left/right
  edge in that case — no different handling needed from a mid-range gap.
- **Truly empty `stats` (zero rows)**: unaffected by any of Decisions
  1-6 — `TrendsResults.tsx:40-42`'s existing short-circuit still returns
  before any `LineChart` (or its children) is ever constructed; only
  Decision 7 touches that branch's own rendering.
- **`@mantine/charts/styles.css` import failure**: this is a build-time
  resolution (a real npm package export, not a runtime URL fetch), so
  there is no runtime failure mode to design for — either the build
  resolves it (expected, since the identical pattern already works for
  `@mantine/core/styles.css` on the same line) or the build fails loudly
  at compile time, which is preferable to a silent no-op.

## Testing

Following this repo's existing `@mantine/charts` mock convention
(`TrendsResults.test.tsx:15-31`, `page.test.tsx:29-33`, `history/
page.test.tsx:20-22`) — unit tests assert on **props handed to
`LineChart`**, not on Recharts' real SVG/DOM output:

- **Decision 2 (legend + dash patterns)**: extend `TrendsResults.test.tsx`'s
  mock to also capture `withLegend` and each `series[].strokeDasharray`;
  assert the rate chart receives `withLegend={true}` and three distinct
  `strokeDasharray` values, and the average-delay chart does **not**
  receive `withLegend` (or receives it `false`/`undefined`) — directly
  testing the "legend on the 3-series chart only" half of Decision 2.
- **Decision 3 (formatter)**: extend the mock to capture `valueFormatter`
  and invoke it directly in the test against a known float (e.g. the
  review's own `0.41267123328767123` example) to assert the exact string
  output (`"0.4 min"`), on the average-delay chart specifically — this is
  a pure function assertion, fully testable without touching Recharts'
  real rendering.
- **Decision 4 (gap-day shading)**: a new set of `toChartPoints`-adjacent
  or `TrendsCharts`-level tests asserting the derived gap-span helper
  produces the correct contiguous day-runs for: no gaps (empty
  span list), one isolated gap day (a single-day span), multiple
  separate gap runs, an all-gap dataset (one full-width span), and
  leading/trailing gaps. If the mock is extended to also capture
  `children`, a light assertion on the number/boundaries of rendered
  `<ReferenceArea>`s is reasonable; asserting Recharts actually paints the
  band correctly is not (see below).
- **Decision 5 (edge padding)**: extend the mock to capture `xAxisProps`
  and assert both charts receive `padding: { right: 12 }` (or whatever
  final value implementation settles on).
- **Decision 6**: no new tests needed specifically — the existing
  `page.test.tsx`/`history/page.test.tsx` composition tests already prove
  both pages render `TrendsResults` unmodified; nothing about this spec
  changes that composition.
- **Decision 7 (empty-state block)**: extend `TrendsResults.test.tsx`'s
  existing "renders the empty state when there are no rows" test (lines
  76-81) to assert the new wrapper element/class is present, alongside the
  existing assertions that the text is shown and no `line-chart` testid
  renders.

**What unit tests cannot cover, stated plainly, per the task's own
instruction.** Decision 1 (tooltip background/box), Decision 2's actual
wrapped legend layout at 390px, and Decision 5's actual pixel-level
clipping outcome are all real-rendering, real-CSS concerns that this
repo's own established mock strategy explicitly does not exercise
(`TrendsResults.test.tsx:15-21`'s own comment: not asserting on Recharts'
SVG output; the mock replaces `LineChart` with a plain `<div>`, so it never
renders Recharts' real tooltip, legend, or axis DOM at all). These three
need screenshot/visual verification — both a desktop viewport and this
repo's own established 390px mobile reference — during implementation,
the same way the original stage-1/stage-2 review of this feature was
itself screenshot-based, not a Vitest run.

## Explicitly out of scope

- Combining the rate chart and average-delay chart into one chart/axis —
  unrelated to any of the seven findings, and `TrendsCharts.tsx`'s own
  comment (lines 34-37) already documents why they're deliberately kept
  separate (different units).
- Changing `SPARSE_DATA_FLOOR_CYCLES`'s value (still `20`, still an
  explicitly-flagged placeholder per `TrendsResults.tsx:8-13`) — orthogonal
  to how gap days are *rendered*, which is all Decision 4 addresses.
- A custom per-point dot marker for isolated gap-adjacent points —
  considered and explicitly rejected in Decision 4 on real API grounds
  (`dotProps` is series-wide, not per-point, in Mantine's typed surface).
- A bespoke hover tooltip/annotation specifically for `null` gap points —
  Decision 4's shaded band is a background/context affordance, not a
  second tooltip mechanism; a `null`-valued series point still produces no
  tooltip row today (unchanged), which is expected Recharts behavior, not
  a bug this spec is fixing.
- Changing the `Skeleton` fallback heights (`280`/`320`) themselves —
  Decision 7 only touches the *resolved* empty-state render, not the
  loading-state skeletons.
- Any change to `TrendsCharts.tsx`'s existing Server/Client boundary
  doc-comment rationale (lines 7-26) — unrelated to these seven findings,
  stays exactly as-is.
- Choosing final numeric values (dash-pattern strings, the rate chart's
  bumped `h`, the exact gap-band fill opacity) precisely — these are
  called out as implementation-time, screenshot-verified choices, not
  fixed by this design spec, consistent with this repo's own established
  posture of not inventing unvalidated precision (e.g.
  `SPARSE_DATA_FLOOR_CYCLES`'s own placeholder framing).

## Open questions / risks

1. **`styles.css` vs. `styles.layer.css` cascade behavior** (Decision 1)
   was not verified against a live render in this session — this repo's
   existing Mantine imports use the plain, non-layered variant, and this
   spec recommends matching that, but implementation should screenshot
   this change in isolation first, since it's expected to be the highest-
   impact single fix in this document and any surprise here would be worth
   catching before layering Decisions 2-5 on top.
2. **The legend's actual wrapped height at 390px** (Decision 2) is an
   estimate (`h={280}` → "roughly 310-320"), not a measurement — this
   session had no way to run the app (`node_modules` not installed, no
   backend to fetch `getLineDailyStats` against). Implementation must
   confirm the real wrapped row count and adjust `h` accordingly.
3. **Decision 7's root cause is genuinely unresolved** — this spec could
   not confirm from the code read this session whether the "dead
   whitespace below the footer" the review found is a persistent bug or a
   transient SSR-streaming artifact. The recommended bounded-empty-state
   treatment is worth doing either way, but if implementation reproduces a
   literal persistent reserved-space mechanism this spec didn't find (e.g.
   in a CSS module not read this session), that root cause should be
   fixed directly rather than only cosmetically overridden.
4. **`ReferenceArea` on a category axis** (Decision 4) — `ReferenceArea`
   being exported by `recharts@3.2.1` and `xAxisProps.padding` being a
   real prop were both verified from source; whether `ReferenceArea`'s
   `x1`/`x2` resolve cleanly against this chart's string `day` category
   values for a single isolated gap day (covering exactly that day's
   category width, not spilling into neighbors) was not verified against
   a live render. Implementation should confirm the rendered band visually
   covers the intended day(s) and adjust the `x1`/`x2` boundary values
   (e.g. using adjacent-day midpoints instead of exact category values) if
   it doesn't line up as expected.
5. **The gap-span helper's day-null derivation** (Decision 4) relies on
   `toChartPoints`'s current guarantee that all four rate/delay fields on
   a `ChartPoint` are nulled together for a sparse day
   (`TrendsResults.tsx:23-35`) — checking `delayRate === null` alone is
   sufficient today, but this is an implicit coupling to that invariant,
   not something the `ChartPoint` type itself enforces. If a future change
   ever nulls fields independently (e.g. per-metric gaps), this derivation
   would silently miscount gap-day runs. Worth a comment at the call site
   when implemented, not a reason to change the approach now.
6. **Dash-pattern choices** (Decision 2) are a reasonable starting point
   for visual distinctness, not validated against real colorblindness
   simulation or user testing — same "placeholder, not researched" posture
   this codebase already takes elsewhere for unvalidated numbers.
