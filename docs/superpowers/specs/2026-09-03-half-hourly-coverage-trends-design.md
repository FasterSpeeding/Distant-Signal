# Design: Half-Hourly Full-Coverage Trends

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the transition design" — the design this whole feature is scaffolded
from) and `docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md`
("the granularity design" — the direct, load-bearing precedent this
document follows for the exact question it answers: how this app already
offers a sample-derived Trends series at two granularities). No
implementation plan is embedded here — that is the separate
`docs/superpowers/plans/2026-09-03-half-hourly-coverage-trends-plan.md`.

## Goal

`docs/superpowers/plans/2026-09-03-full-coverage-metrics-scaffolding-plan.md`
("the scaffolding plan") built the full-coverage rollup end to end —
`crates/api/migrations/*_line_status_{daily,half_hourly}_coverage_stats.sql`,
`crates/aggregator`'s write path for both tables, and both read routes
(`GET /Line/{id}/Stats/Coverage/{from}/to/{to}` and
`GET /Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}`,
`crates/api/src/routes/line_status.rs:66-73`) — but its own Non-goals
section deliberately stopped short of a frontend chart for the
half-hourly table, calling a second, definitely-still-empty chart surface
"exactly the kind of exercise this plan's own brief says not to
gold-plate" while Option B (the future TRUST-vs-schedule producer) didn't
exist yet to populate either table. That reasoning was sound for *that*
plan's scope, but it leaves a real, permanent gap once a producer does
land: `frontend/app/lines/[id]/history/CoverageTrendsResults.tsx`
(confirmed by reading its own doc comment) renders only the **daily**
coverage series. This document designs the missing half-hourly frontend
surface — wire-compatible with what already shipped, zero backend
changes.

## Current relevant state (verified 2026-09-03)

**The backend is done and already tested.** `crates/api/src/routes/line_status.rs`
registers both coverage routes (lines 66-73) and both have pure
`daily_coverage_stats_to_json`/`half_hourly_coverage_stats_to_json` tests
(lines 473, 517) plus `oneshot`-probe path tests for both routes (lines
1054-1084) and DB-backed `#[ignore]`-gated end-to-end tests (line
1786+). Nothing in this document touches `crates/`.

**The frontend already has every piece except the component and its call
site.** Confirmed by direct inspection, not assumed:
- `frontend/lib/types.ts:220-231`'s `LineHalfHourlyCoverageStats` already
  exists — `halfHourStart`, `resolvedWindows`, and the same six
  count/rate fields `LineDailyCoverageStats` has, doc-commented as the
  "half-hourly sibling of `LineDailyCoverageStats`."
- `frontend/lib/api.ts:208-221`'s `getLineHalfHourlyCoverageStats` already
  exists, hitting `/Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}`
  exactly, added "for backend symmetry with Task 6's two routes, even
  though only the daily one gets a frontend chart" per its own comment
  (Task 9, scaffolding plan).
- `frontend/app/lines/[id]/history/TrendsCharts.tsx` is already
  bucket-key-agnostic (granularity design Decision 9, already merged): it
  takes a `granularity: 'day' | 'halfHour'` prop and only changes its
  x-axis `tickFormatter` between the two — the same component
  `CoverageTrendsResults.tsx` already reuses unmodified for the daily
  coverage series.
- `frontend/lib/history.ts:231-236`'s `resolveHalfHourlyRange(now)`
  already exists — a fixed rolling 24-hour window (`now - 24h` to `now`),
  used today only by the sample-stats half-hourly component.

**This repo has already solved "offer both granularities" once, for the
sample-derived series — and the answer is not a toggle.** Read in full,
both the granularity design and the two components it produced:

- **Daily**: `frontend/app/lines/[id]/history/TrendsResults.tsx`, rendered
  inside the dedicated `/lines/[id]/history` page's "Trends" tab
  (`history/page.tsx:137-157`), over whatever date range the user picked
  via `HistoryRangePicker`.
- **Half-hourly**: `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx`,
  rendered on the **line-info page itself**
  (`frontend/app/lines/[id]/page.tsx:173-202`, under the heading "Recent
  trends (last 24 hours)"), over the fixed rolling 24-hour window
  `resolveHalfHourlyRange` computes — **no date-range picker**, per the
  granularity design's own Decision 11 ("the task frames this view as a
  fixed rolling 24 hours... `HistoryRangePicker`'s whole reason to exist...
  doesn't apply to a window that's never user-selectable").
- These are **two separate, structurally-parallel Server Components**, not
  one component branching on a `granularity` prop — the granularity
  design's own Decision 10 states this explicitly: each component's
  fetch, sparse-data floor, and honesty copy are "granularity-specific
  content, not shared plumbing," while `TrendsCharts` is "the shared,
  reusable part... the rendering leaf, not the data-fetching/labeling
  Server Component around it."
- **A granularity toggle/selector UI was explicitly considered and
  rejected.** The granularity design's own "Explicitly out of scope"
  section states this in so many words: "A granularity toggle/selector
  UI anywhere (e.g. letting a user switch the history page itself between
  daily and hourly views) — not asked for, not designed here." This repo
  has a settled, working answer to "how do we offer two granularities of
  the same trend data," and it is **two fixed views in two fixed places**,
  not a switch.

`CoverageTrendsResults.tsx` already mirrors the daily half of this split
exactly (same page, same tab, same user-selected range,
`getLineDailyCoverageStats`). The half-hourly half has no analog yet.

## Decisions

### 1. Mirror the existing split exactly: a new `HalfHourlyCoverageTrendsResults.tsx`, rendered on the line-info page, fixed rolling 24h window — not a toggle, not a new location

**Chosen: apply the granularity design's already-settled pattern
unchanged.** Per Current relevant state, this repo does not have an open
design question here — it already decided, built, and shipped "daily on
the history page's Trends tab, half-hourly on the line-info page's fixed
24h window, two separate components, no toggle" for the sample-derived
series. The coverage series is a second data source of the *identical*
shape (`SampleStats`-derived count/rate fields, a coverage/gap signal
field, a bucket key) read through the *identical* chart leaf. There is no
reason specific to full-coverage data to answer "how do we offer both
granularities" any differently than the sample-derived series already
does — doing so would leave this app with two different UX answers to the
same UX question, for no stated benefit.

Concretely:
- New file `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx`,
  structurally mirroring `HalfHourlyTrendsResults.tsx` (Server Component,
  same shape as `CoverageTrendsResults.tsx`'s existing daily-to-half-hourly
  relationship mirrors `TrendsResults.tsx`'s relationship to
  `HalfHourlyTrendsResults.tsx`).
- Rendered on `frontend/app/lines/[id]/page.tsx`, inside the same "Recent
  trends (last 24 hours)" `Stack` that already renders
  `HalfHourlyTrendsResults`, as a second section directly underneath it —
  the same "second, separate section, always attempted" placement
  `CoverageTrendsResults.tsx` already uses relative to `TrendsResults` on
  the history page (scaffolding plan Task 9 Step 3's judgment call,
  applied here a second time for the same reason: simplest of the shapes
  Decision 4 of the transition design leaves open, degrades to an honest
  empty state today).
- Uses the same `resolveHalfHourlyRange(now)` window
  `HalfHourlyTrendsResults` already uses — one `trendsRange` computed once
  on the page, fed to both components, so both sections show the same
  24-hour window rather than two independently-computed "now"s a few
  milliseconds apart.
- **Not** added to the `/lines/[id]/history` page's Trends tab. That tab
  is daily-only by the granularity design's own Correction 6/Decision 10
  ("already daily over the selected range... no change is proposed"), and
  nothing about full coverage changes that scoping — the coverage series'
  daily half already lives there (`CoverageTrendsResults`), correctly,
  and stays there unchanged.

### 2. Data plumbing: reuse the already-built types/fetcher verbatim; only the component and its floor/copy are new

**Chosen: zero new types, zero new API functions.** `LineHalfHourlyCoverageStats`
and `getLineHalfHourlyCoverageStats` (Current relevant state) are already
exactly what this component needs — they were added in the scaffolding
plan's Task 9 Step 1 specifically for this future use, unused since. This
document does not propose touching `frontend/lib/types.ts` or
`frontend/lib/api.ts` at all.

**`toHalfHourlyCoverageChartPoints`** — a new, small pure function in the
new file, structurally identical to `CoverageTrendsResults.tsx`'s existing
`toCoverageChartPoints` (which maps `LineDailyCoverageStats[]` →
`ChartPoint[]` using `resolvedWindows` as the sparse-gap signal), applied
to `LineHalfHourlyCoverageStats[]` instead — same field mapping, same
`bucketKey: row.halfHourStart` shape `toHalfHourlyChartPoints` (the
sample-series half-hourly equivalent) already uses for its own `bucketKey`.

**Sparse-data floor: a fourth, independently-derived placeholder
constant, following the exact scaling precedent already established
twice.** This repo already has two floor constants and one documented
derivation rule connecting them:
- `TrendsResults.tsx`'s `SPARSE_DATA_FLOOR_CYCLES = 20` (daily,
  sample-derived).
- `HalfHourlyTrendsResults.tsx`'s `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY = 10`
  — derived, per that file's own comment, by halving the daily value
  because halving the bucket duration halves the cycle ceiling, holding
  the same ~33% coverage-bar ratio.
- `CoverageTrendsResults.tsx`'s `SPARSE_DATA_FLOOR_WINDOWS = 20` (daily,
  full-coverage) — explicitly *not* shared with `SPARSE_DATA_FLOOR_CYCLES`
  (own constant, own name), but set to the same starting number since
  neither has any real production distribution to calibrate against yet.

**Chosen: `SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY = 10`**, applying the
identical halving rule the sample-series pair already established, to the
full-coverage daily floor `CoverageTrendsResults.tsx` already set. Its own
comment must say so explicitly (mirroring
`SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`'s comment almost verbatim) and
flag it, like every other floor constant in this file tree, as an
unvalidated placeholder — there is still no real full-coverage producer
to observe a `resolved_windows`-per-half-hour distribution from.

### 3. Honesty copy: half-hourly wording of the existing daily coverage copy, not the sample-series half-hourly copy

**Chosen: reuse `CoverageTrendsResults.tsx`'s existing full-coverage
sentence, re-scoped from "day" to "half hour" the same way the
sample-series pair already did.** The sample-series precedent
(`TrendsResults.tsx` → `HalfHourlyTrendsResults.tsx`) changed exactly one
phrase, "once per day... that day" → "once per half hour... that half
hour," keeping every other word of the honesty sentence identical
(`HalfHourlyTrendsResults.tsx`'s own comment: "reworded from 'that day' to
'that half hour' per Decision 2's per-bucket attribution, not a new
tradeoff"). The full-coverage sentence has no per-bucket attribution
clause to reword (it states a *population* fact — "every scheduled
service... cross-referenced against real train-movement data" — not a
first-sighting-per-period counting rule), so it is reused **completely
verbatim**, with no wording change at all:

```
Rates shown cover every scheduled service on this line, cross-referenced
against real train-movement data — not a sample of live departures at a
handful of stations.
```

This is a real, deliberate difference from how the sample-series pair's
copy differs between granularities — worth stating outright rather than
mechanically "porting" a rewording pattern that doesn't apply here, since
the underlying honesty claim this sentence makes is granularity-independent
by construction (it's a statement about *population coverage*, which is
the same fact at any bucket size).

**Section heading**: `"Full coverage"`, same literal text
`CoverageTrendsResults.tsx` already uses for its `<Title order={4}>` —
this is a second, sibling occurrence of the same concept on a different
page, not a new name.

**Empty-state copy**: `"Not enough full-coverage data yet for this
line."`, identical to `CoverageTrendsResults.tsx`'s existing empty state
— same underlying condition (no producer exists yet), same honest
wording, in the same `<Paper withBorder p="md">` bounded container
`HalfHourlyTrendsResults.tsx` already uses for its own empty state.

### 4. Heading level and Suspense wrapping: match the section's existing surrounding structure exactly

**Chosen**: `<Title order={3} size="h6">Full coverage</Title>`, matching
`CoverageTrendsResults.tsx`'s own heading level exactly (that component
already sits under an h2 in its own page — the history page's implicit
tab heading structure — and lands its own heading at h3/`size="h6"`; the
line-info page's structure is the same depth: h1 line name → h2 "Recent
trends (last 24 hours)" → this section's h3, immediately following
`HalfHourlyTrendsResults`' own two h3 chart titles with no level skipped).
`TrendsCharts`' own internal chart titles render one level below whatever
`order` is passed to it (`order={4}` in `CoverageTrendsResults.tsx`,
matching its own h3 section heading) — this component passes `order={4}`
to its own `TrendsCharts` call for the identical reason, one level below
this section's own h3.

Wrapped in its own `<Suspense fallback={<Skeleton height={280} />}>` on
`page.tsx`, matching `HalfHourlyTrendsResults`' existing wrapping exactly
(comment already present there explains why: the fetch is comparatively
slow and a brand-new/quiet line still resolves fast to its own empty
state rather than leaving the section hanging) — as a **separate**
Suspense boundary from `HalfHourlyTrendsResults`' own, not a shared one,
so a slow coverage fetch never blocks the (real, populated-today) sample
chart above it from appearing, and vice versa.

## Architecture

```
frontend/app/lines/[id]/page.tsx                        CHANGED -- renders
  "Recent trends (last 24 hours)"                        a second Suspense-
    <HalfHourlyTrendsResults .../>          UNCHANGED     wrapped section
    <HalfHourlyCoverageTrendsResults .../>  NEW            under the existing
                                                            sample-series one,
                                                            same trendsRange

frontend/app/lines/[id]/history/
  HalfHourlyCoverageTrendsResults.tsx       NEW -- mirrors HalfHourlyTrendsResults.tsx
    fetches getLineHalfHourlyCoverageStats  ALREADY EXISTS (frontend/lib/api.ts)
    toHalfHourlyCoverageChartPoints()       NEW, mirrors toCoverageChartPoints
    SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY   NEW placeholder constant (=10)
    reuses TrendsCharts, granularity="halfHour"  ALREADY GENERALIZED

  CoverageTrendsResults.tsx                 UNCHANGED -- stays the daily-only,
                                             history-page-Trends-tab component
  TrendsCharts.tsx                          UNCHANGED -- already bucket-key-
                                             and granularity-agnostic
```

No backend files are touched by this design. The two coverage routes and
both rollup tables already exist and are already tested, per Current
relevant state.

## Explicitly out of scope

- **Any backend change.** The routes, tables, and aggregator write path
  are done; this document is a pure frontend consumer of what already
  ships.
- **A granularity toggle/selector UI.** Rejected for the identical reason
  the granularity design already rejected it for the sample-derived
  series (Decision 1) — this repo has one settled answer to this
  question, applied here rather than re-litigated.
- **Changing where the daily coverage chart lives, or its behavior.**
  `CoverageTrendsResults.tsx` is untouched.
- **Recalibrating any existing sparse-data floor** (`SPARSE_DATA_FLOOR_CYCLES`,
  `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`, `SPARSE_DATA_FLOOR_WINDOWS`).
  Only a new, fourth placeholder is added, following the same
  unvalidated-placeholder posture every existing one already has.
- **Deciding the mixed sample+coverage chart-range UI question** the
  transition design's Decision 4 left open (separate series vs. a marked
  transition vs. hiding the sample series). Both granularities already
  render coverage as a wholly separate section from the sample series, on
  both pages, once this document ships — the same "separate series" shape
  the daily case already chose, extended one more time, not a new answer
  to that open question.
- **Retention/floor validation against real production data.** No
  full-coverage producer exists yet (per the transition design's own
  scope); this remains true after this document ships. Every number here
  is a placeholder, same as every sibling constant already in this file
  tree.

## Open questions / risks

1. **`SPARSE_DATA_FLOOR_WINDOWS_HALF_HOURLY`'s value (10) is doubly
   unvalidated** — it is a halving of an already-unvalidated daily
   placeholder (`SPARSE_DATA_FLOOR_WINDOWS = 20`, itself flagged
   "not designed to a specific number here" by the transition design's
   Decision 4). Revisit both together once a real producer's
   `resolved_windows` distribution exists to look at — same posture every
   prior floor constant in this codebase has shipped with.
2. **Whether the line-info page is getting crowded** (it will now render
   four chart-bearing sections in the "Recent trends" block once this
   ships: sample rate chart, sample avg-delay chart, coverage rate chart,
   coverage avg-delay chart, on top of `IssueList`/`RepresentativeInfo`
   above it) is a real density question this document doesn't resolve —
   the granularity design's own Open question 5 already flagged an
   analogous "keep both charts or drop to one" tension for the sample
   series alone and left it to a screenshot-driven implementation pass;
   this document takes the same posture and doesn't pre-decide it.
