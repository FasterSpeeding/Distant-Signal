# Design: A Sample-Volume ("how many trains counted") Chart on the History Page's Trends Tab

**Status: design proposal, not approved.** No implementation plan or code
is included here; every sketch is marked as a sketch, not final code.

## Goal

The Trends tab's delay/cancellation/skip-rate chart (`TrendsResults.tsx` +
`TrendsCharts.tsx`) shows a rate with no denominator context: "10%
delayed" reads identically whether that bucket saw 2 trains or 200. Add a
second chart, alongside the existing one, that answers "how many trains is
this rate actually counted over" for the same bucket the rate chart shows
— giving a viewer the sample-size context needed to judge how much to
trust a given rate.

## Corrections to the brief's framing

1. **The number this task wants already exists, is already computed
   correctly (deduped, not a raw poll-cycle recount), and is already
   returned by the read APIs the Trends tab calls today — it is just never
   threaded into the chart-rendering data shape.** This is not a new
   aggregation problem. It is a "the value is sitting unused in the same
   JSON response already being fetched" problem. See Decision 1 and
   Decision 4.
2. **`total` and `sample_cycles`/`running_count` answer three different
   questions, and the brief's own hint (don't conflate cycle-count with
   train-count) turns out to be exactly right — but the codebase already
   drew this line correctly, in a different module than the one that
   writes the accumulated columns.** `sample_cycles` is a poll-cycle
   coverage counter (Decision 1's "not this one"); `running_count` is an
   internal averaging denominator, not a train count at all (Decision 1's
   "definitely not this one"); `total` is the one column that is, by
   construction, already deduplicated per distinct Darwin `service_id` —
   see Decision 1 for the full trace.
3. **The LDBWS-sampled case and the (currently unpopulated)
   full-coverage case are architecturally already split into two separate
   chart sections on this same tab** (`TrendsResults`/`CoverageTrendsResults`,
   both already mounted on `history/page.tsx`), **and that split is the
   right shape to extend, not something this feature needs to invent.**
   But the two cases are *not* symmetric under the hood the way their
   parallel frontend components suggest: LDBWS's `total` is deduplicated
   at the aggregator's write path; the full-coverage historical rollup's
   `total` is explicitly **not** run through any equivalent dedup step
   today, by the aggregator's own comment. See Decision 3.
4. **The four-tier configurable-granularity feature this task was told to
   compose with is a written, approved design and plan
   (`docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md`,
   `docs/superpowers/plans/2026-09-05-configurable-trend-granularity-plan.md`)
   that has NOT been implemented on disk as of this pass.** Confirmed
   directly, not inferred from the plan's own checkboxes: `TrendsResults.tsx`
   still does one hard-wired daily fetch (no `granularity` parameter, no
   `switch`), `TrendsCharts.tsx`'s `granularity` prop is still the
   two-value `'day' | 'halfHour'` union the *prior* (already-shipped)
   half-hourly feature introduced, and no `GranularityControl.tsx` file
   exists anywhere in `frontend/app/lines/[id]/history/`. This document
   therefore designs against **the real, currently-shipped surface**
   (`TrendsResults`, `HalfHourlyTrendsResults`, `CoverageTrendsResults`,
   three separate Server Components, each producing `ChartPoint[]` for the
   shared `TrendsCharts`), and separately explains why and how the same
   design composes for free if/when the granularity plan lands (Decision 7)
   — without needing to design *for* not-yet-real code.

## Current relevant state (verified against the live worktree)

- **`SampleStats`** (`crates/common/src/lib.rs:770-776`) is the raw
  per-cycle struct: `{ total, delayed, cancelled, skipped, avg_delay_minutes }`
  (all `usize`/`f64`, no `running_count` field — that value is derived
  later, only at the point a `SampleStats` is folded into an accumulated
  row; see below). Built by `common::compute_sample_stats`
  (`crates/common/src/lib.rs:1181-1210`) from a list of
  `&StationDeparture`: `total = departures.len()`; `cancelled` = the
  `is_cancelled` count; `delayed` = `!is_cancelled && delay_minutes >=
  threshold`; `skipped` = `!is_cancelled && is_skip(d)`. **`delayed` and
  `skipped` are independently-computed, overlapping filters — both simply
  exclude cancelled — not a partition of `total`**: a train that is both
  delayed and skips a calling point is counted in both. `delayed +
  cancelled + skipped` does **not** sum to `total` in general. This matters
  directly for Decision 3 (why not a stacked composition chart).
- **Two functions build a `SampleStats`, for two different purposes, and
  only one of them deduplicates by train:**
  - `aggregation::stats_from_departures`/`compute_sample_stats`, called
    every cycle over **every currently-visible** departure at a line's
    `sample_stations` — correct for live severity classification (`infer_from_samples`),
    which wants "what does the window look like right now," and which
    re-counts the same physically-still-dwelling train every cycle it
    remains in Darwin's rolling departure-board window (`crates/aggregator/src/aggregation.rs:766-799`,
    `crates/aggregator/src/dedup.rs:1-38`'s own module doc explains the
    ~20-40x per-cycle overcount this produces at the default 60s cadence
    if summed raw across a period).
  - `dedup::dedup_new_sample_stats` (`crates/aggregator/src/dedup.rs:145-193`),
    called at the **same** call site, over the same `relevant_departures`,
    but filtered through `SeenServiceLedger::mark_seen` — a per-`(line_id,
    calendar day)` in-memory `HashSet<service_id>`
    (`crates/aggregator/src/dedup.rs:94-143`) — so a `StationDeparture`
    only contributes if its Darwin `service_id` has **not** already been
    counted for that line, that calendar day, on an earlier cycle (or an
    earlier station in the same cycle: `relevant_departures` flat-maps
    across all of a line's `sample_stations`, so a through-service visible
    at two sample stations in the same cycle is still deduplicated to one).
    This is the function whose *output* — `deduped: Option<&SampleStats>` —
    is what actually gets summed into `line_status_daily_stats`/
    `line_status_half_hourly_stats` (`crates/aggregator/src/main.rs:263-283`).
- **`record_daily_stats`/`record_half_hourly_stats`**
  (`crates/aggregator/src/queries.rs:572-620`, `:657-705`) accumulate the
  deduped `SampleStats.total` straight into the `total` column via a plain
  `+ EXCLUDED.total` upsert (`queries.rs:602`, `:687`). **The accumulated
  `total` column is therefore, exactly: "the count of distinct
  `service_id`s newly observed (first seen) during this bucket, among
  departures relevant to this line at its configured `sample_stations`."**
  It is a real, already-deduplicated train count — not a cycle count, not
  a re-derivable rate artifact.
  - **`running_count`** (same two functions) is computed inline as `s.total
    - s.cancelled` (`queries.rs:583`, `:668`) — i.e. "trains counted minus
    cancelled ones" — and exists *solely* as the accumulating denominator
    for `delay_minutes_sum` (so a cancelled train's zero/undefined delay
    doesn't drag the average down), consumed only by
    `daily_stats_to_json`/`half_hourly_stats_to_json`'s
    `delay_minutes_sum / running_count` division
    (`crates/api/src/routes/line_status.rs:387-389`, `:431-433`). It is
    **not** "how many trains are running" in any concurrency sense and
    **not** a better train-count candidate than `total` — it is `total`
    with cancellations subtracted out, for one specific downstream
    arithmetic need.
  - **`sample_cycles`** (`queries.rs:601`, `:686`, incremented `+ 1` every
    cycle regardless of whether `stats` was `Some` or `None`) is a
    poll-cycle coverage counter — it answers "how many times did the
    aggregator successfully run this bucket's line," not "how many trains
    did it see." It is already used, correctly, as the *reliability* gate
    for the existing rate charts (`SPARSE_DATA_FLOOR_CYCLES`,
    `TrendsResults.tsx:13`; `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`,
    `HalfHourlyTrendsResults.tsx:22`) — a genuinely different job from a
    train-count chart, and this document does not touch it.
  - **Caveat on what `total` per bucket actually represents**: because the
    dedup ledger keys on **calendar day**, not on the finer half-hourly
    bucket (`dedup_new_sample_stats`'s `period: NaiveDate` parameter,
    `crates/aggregator/src/main.rs:264-273` passing the same `today` to
    both the ledger call and to `record_daily_stats`), a train first
    observed at 08:03 and still dwelling at 08:35 contributes its `total`
    of 1 to the **08:00 half-hour bucket only** — the 08:30 bucket sees
    zero contribution from that same train, because the ledger already
    marked its `service_id` seen for that day. **A half-hourly (or, once
    it exists, hourly/6-hourly) bucket's `total` therefore means "trains
    newly seen for the first time this bucket," not "trains active/running
    during this bucket."** Summed across a full day, the buckets
    reconstruct the day's real distinct-train total exactly (this is the
    invariant `half_hourly_and_daily_stats_reconcile_for_a_single_line_and_period`
    already tests) — but read in isolation, a single sub-daily bucket's
    number is a "first observed" count, not a concurrency snapshot. This
    is a real, non-obvious semantic a chart title/tooltip needs to get
    right (Decision 2, Open question 2).
- **This value is already in the wire payload, at every read call site the
  Trends tab uses today**, and just isn't threaded past the point the
  response gets turned into a `ChartPoint`:
  - `crates/api/src/data/queries.rs:1039-1048`'s `DailyStatsRow` (and its
    `HalfHourlyStatsRow` sibling) already carry `total: i64`.
  - `daily_stats_to_json`/`half_hourly_stats_to_json`
    (`crates/api/src/routes/line_status.rs:386-411`, `:430-455`) already
    serialize `"total": row.total` onto every response object from `GET
    /Line/{id}/Stats/{from}/to/{to}` and `GET
    /Line/{id}/Stats/HalfHourly/{from}/to/{to}`.
  - `frontend/lib/types.ts`'s `LineDailyStats` (`:162-173`),
    `LineHalfHourlyStats` (`:185-196`), and — for the full-coverage side —
    `LineDailyCoverageStats` (`:210-221`) all already type a `total:
    number` field, already populated by `getLineDailyStats`/
    `getLineHalfHourlyStats`/`getLineDailyCoverageStats`
    (`frontend/lib/api.ts`) today.
  - **But `ChartPoint`** (`frontend/app/lines/[id]/history/chartPoint.ts:20-27`)
    **has no `total` field**, and all three existing mapper functions —
    `TrendsResults.tsx`'s `toChartPoints` (`:23-35`),
    `HalfHourlyTrendsResults.tsx`'s `toHalfHourlyChartPoints` (`:28-40`),
    `CoverageTrendsResults.tsx`'s `toCoverageChartPoints` (`:25-37`) — read
    `row.total` only to gate other fields' *rates* via `row.total > 0`
    checks upstream in the route handler, and never copy `row.total` onto
    the `ChartPoint` they return. It is dropped on the floor at exactly one
    place, three times over (once per existing mapper).
- **`TrendsCharts.tsx`** (`frontend/app/lines/[id]/history/TrendsCharts.tsx`)
  renders exactly two `<LineChart>` panels today — rate (0-1 proportions)
  and average delay minutes — both driven by the same `points: ChartPoint[]`
  and the same `gapSpans`/`referenceAreaBounds` gray-out-the-gap machinery
  (`:20-52`). Nothing about that machinery is tied to rendering a rate; it
  operates on "is this field null for this bucket," which generalizes to
  any field on `ChartPoint`.
- **The full-coverage side of this tab already exists, is already wired to
  the same `TrendsCharts` component, and is already, permanently empty
  today**: `CoverageTrendsResults.tsx` fetches
  `getLineDailyCoverageStats` and — because no full-coverage producer
  exists yet (`lines_with_full_coverage` always returns an empty set,
  `crates/aggregator/src/main.rs:299-330`'s own "Always a no-op today"
  comment) — always renders the "Not enough full-coverage data yet for
  this line" empty state. It is mounted directly under `TrendsResults` on
  the Trends tab (`history/page.tsx:137-156`), a second, independent
  `<Suspense>` boundary.
- **The full-coverage historical rollup's `total` is a materially
  different, and currently *not* deduplicated, number**, per the
  aggregator's own comment at the write site
  (`crates/aggregator/src/main.rs:299-304`): "this one is fed each
  status's raw `full_coverage_stats` directly, NOT run through a dedup
  step... no defined per-service dedup analog exists yet for a
  full-coverage producer." Separately, the full-coverage *design* intends
  its `total` to mean something conceptually better than the LDBWS
  case's — "every scheduled service on the line," not "the curated 2-5-station
  LDBWS subset" (`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md:179-192`)
  — but that intent has not yet been reconciled with the write path's
  current no-dedup posture. See Decision 3.
- **A separate, unrelated full-coverage table pair —
  `station_full_coverage_samples` (migration `20260904070000`) and
  `full_coverage_line_stats` (migration `20260904100000`)** — are both
  **live-snapshot** tables (current state, wholesale-replaced per
  resolution cycle), explicitly *not* history: `full_coverage_line_stats`'s
  own migration comment states "the existing
  `line_status_daily_coverage_stats`/`line_status_half_hourly_coverage_stats`
  tables remain the historical rollups this table is NOT a substitute
  for." Since the Trends tab is inherently a historical view over a date
  range, these two tables are out of scope for this feature entirely —
  the only full-coverage source this document should ever cite is the
  historical rollup pair, already covered above.

## Decisions

### 1. Chart `total` — an existing column, already meaning "distinct trains newly counted this bucket," not a cycle count and not `running_count`

**Chosen: the quantity to chart as "number of trains being counted" is the
existing `total` field**, already returned by every stats endpoint this
tab calls (`GET /Line/{id}/Stats/{from}/to/{to}`, `GET
/Line/{id}/Stats/HalfHourly/{from}/to/{to}`, and the full-coverage
`Coverage` siblings) — see the "Current relevant state" section for the
full trace of exactly what value flows into this column and why it is
already correctly deduplicated for the LDBWS case (Decision 1, above).

Rejected candidates, with the specific reason each is wrong for this job:
- **`sample_cycles`**: a poll-cycle coverage counter, already fully
  spoken for as the sparse-data reliability gate. Charting it as "number
  of trains" would be exactly the conflation the brief itself warned
  against — a line polled every 60s for 30 minutes has `sample_cycles ≈
  30` regardless of whether 2 or 40 distinct trains passed through.
- **`running_count`**: `total - cancelled`, an internal averaging
  denominator with no standalone meaning as a train count. Charting it
  instead of `total` would silently subtract cancelled trains from the
  volume figure for no user-facing reason, and would need re-deriving
  `total = running_count + cancelled` at read time anyway to recover the
  actual count — pointless, since `total` is already the number sitting
  in the same row.
- **A new, distinct-service-count column or migration**: unnecessary.
  `total` already *is* a per-bucket distinct-train count, by construction
  of `dedup_new_sample_stats` (Decision 1's "Current relevant state"
  trace) — there is nothing left to build on the backend.

**Precise wording this number should carry in the UI (not "how many
trains are running")**: "trains counted" or "distinct trains observed,"
never "trains running" or "trains active" — see the "first observed this
bucket, not concurrently active" caveat above (Open question 2 revisits
whether this needs an explicit tooltip, not just chart-title wording).

### 2. `total` is never nulled by the sparse-data floor — deliberately different treatment from every other `ChartPoint` field

**Chosen: the new field carries the row's raw `total` value into every
bucket unconditionally, bypassing the `sparse ? null : ...` gate every
existing mapper function (`toChartPoints`, `toHalfHourlyChartPoints`,
`toCoverageChartPoints`) already applies to `delayRate`/`cancellationRate`/
`skipRate`/`avgDelayMinutes`.**

This is the single most important rendering decision in this document,
and it is a deliberate asymmetry, not an oversight: the whole reason this
chart exists is to show a viewer *why* a given bucket's rate is
unreliable or gapped — "that gap in the rate chart is because only 2
trains were counted that day." If the volume chart nulled out exactly the
same sparse buckets the rate chart already grays out via
`gapSpans`/`referenceAreaBounds`, the new chart would show a gap in
precisely the place a viewer most wants to see a small, honest number —
defeating its entire purpose. A low `total` bar/line *is* the informative
signal here, not noise to be hidden.

One direct consequence: **`gapSpans`/`referenceAreaBounds` do not apply to
this new panel at all** — since `total` is never null for a bucket that
has a row in the underlying table, there are no gaps for this panel's own
`<ReferenceArea>` shading to compute. (A bucket with **no row at all** in
the underlying table — e.g. a calendar day the aggregator never ran a
cycle for — still produces no chart point on *any* panel, new or old,
since every mapper function only ever iterates the array the API actually
returned; this is pre-existing behavior for every field, not something
this feature changes or needs to fix — see Non-goals.)

### 3. LDBWS and full-coverage get the same split treatment the tab already has — but the full-coverage side inherits a real, pre-existing dedup gap this document does not fix

**Chosen: extend both of the tab's two existing chart sections
(`TrendsResults`'s LDBWS-sample series, `CoverageTrendsResults`'s
full-coverage series) with this new panel, using each section's own
already-fetched `total` field** — not a new, third, cross-cutting
component. This mirrors the split the tab already made for exactly this
reason: the two series have different population coverage, different
honesty copy, and (per Decision 4 below) will keep fetching from two
different existing functions.

**What is genuinely different between the two, and must not be papered
over by giving them the same chart title:**
- **LDBWS `total`** is inherently partial by construction — only
  departures visible at the line's configured 2-5 `sample_stations`,
  further filtered by `belongs_to_line` (operator/keyword/route
  matching). `TrendsResults`'s existing honesty copy already says this
  out loud ("Rates shown count each distinct train... not a share of poll
  cycles" — `TrendsResults.tsx:76-81`); the new volume panel's copy in
  this section must not imply "every train on the line," only "every
  train counted in this sample."
- **Full-coverage `total`** is *intended* to mean "every scheduled
  service on the line" — the complete population, not a curated subset
  (`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md:179-192`)
  — but **today, the historical rollup's write path does not deduplicate
  it at all** (`main.rs:299-304`'s own comment, above). If a real
  full-coverage producer is ever wired up without first (or
  simultaneously) building a dedup analog for it, `full_coverage_line_stats`... — no,
  this document means `line_status_daily_coverage_stats`/
  `line_status_half_hourly_coverage_stats`'s `total` — would very likely
  suffer the exact same "same train recounted every cycle it's visible"
  inflation the `dedup` module was built to fix for LDBWS, just for a
  different data source. **A volume chart built directly on this column,
  once real data exists, could show a number several times larger than
  the true distinct-train count for that line/period, and it would not be
  visually distinguishable from a correct number without a code fix
  upstream.** This document does **not** propose or attempt that dedup fix
  (out of scope — no full-coverage producer exists to test it against
  yet, and it is squarely an aggregator-write-path concern, not a
  frontend-chart concern) — flagging it here so whoever eventually builds
  a real full-coverage producer sees it before, not after, this chart
  ships against real full-coverage data. See Open question 1.
- **Today, in practice, this entire distinction is moot**: since
  `CoverageTrendsResults` always renders the "not enough full-coverage
  data yet" empty state (no producer exists), the full-coverage volume
  panel this document proposes will render nothing observable until a
  real producer exists — same "designed to compose, nothing to show yet"
  posture `CoverageTrendsResults.tsx`'s own doc comment already takes for
  its two existing rate charts.

### 4. Backend: zero new work. This is a purely frontend, additive change over data already on the wire

**Chosen**: no new column, no new migration, no new query, no new route.
Every value this chart needs (`total`) is already selected by
`daily_stats_for_range`/`half_hourly_stats_for_range`
(`crates/api/src/data/queries.rs`) and their coverage-table siblings,
already serialized by `daily_stats_to_json`/`half_hourly_stats_to_json`
(and siblings), and already typed and fetched on the frontend
(`LineDailyStats`/`LineHalfHourlyStats`/`LineDailyCoverageStats`,
`getLineDailyStats`/`getLineHalfHourlyStats`/`getLineDailyCoverageStats`).
The only code this feature needs to touch:

1. **`ChartPoint`** (`chartPoint.ts:20-27`) grows one new required field,
   `total: number` (never `null` — Decision 2).
2. **Every existing mapper function** — `toChartPoints`
   (`TrendsResults.tsx`), `toHalfHourlyChartPoints`
   (`HalfHourlyTrendsResults.tsx`), `toCoverageChartPoints`
   (`CoverageTrendsResults.tsx`), and (once/if it exists) a future
   half-hourly-coverage mapper — adds one line, `total: row.total,`,
   **outside** the `sparse ? ... : ...` ternaries the other four fields
   already use.
3. **`TrendsCharts.tsx`** grows one new chart panel (Decision 5) reading
   `points[].total` — everything else about the component (props,
   `granularity`, `order`, the two existing panels) is unchanged.

No `crates/aggregator`, `crates/api`, or migration file needs to change
for this feature at all.

### 5. Rendering: one new panel per existing chart section, a bar chart (not a third line on the rate axis), no stacked composition

**Chosen**: add one new `<BarChart>` (from `@mantine/charts`, confirmed
present in the pinned `9.5.2` install — `frontend/package.json:15`,
`node_modules/@mantine/charts/esm/BarChart/`) panel inside
`TrendsCharts.tsx`, immediately **above** the existing rate chart (so a
viewer sees "how many trains" before "what rate," matching this feature's
own stated purpose of giving context *before* judging the rate), sharing
the same `data={points}`/`dataKey="bucketKey"`/`xAxisProps` (hence the
same x-axis category identity and tick formatting `granularity` already
selects) as the two existing panels. A single series (`total`) — **not**
a stacked bar of `delayed`/`cancelled`/`skipped`.

**Why not stacked**: `delayed` and `skipped` are independently-computed,
overlapping filters (Decision 1's "Current relevant state" —
`common::compute_sample_stats`, `crates/common/src/lib.rs:1181-1210`), not
a partition of `total`. Stacking them would double-count a train that is
both delayed and skips a calling point, making the stack's visual height
exceed the very `total` this panel exists to show — a real, silently
misleading chart, not a stylistic nitpick. A single flat bar per bucket
sidesteps this failure mode entirely and loses nothing the existing rate
chart doesn't already convey as proportions.

**Why a bar chart, not a third line on the existing rate `<LineChart>` or
a third standalone `<LineChart>`**: a count is a volume/magnitude
quantity, not a trend proportion sharing the rate axis's 0-100% domain —
putting it on the same axis as the three rate series would need a second
y-axis or a wildly different scale, and Recharts/`@mantine/charts`
support for a clean dual-axis `LineChart` is unverified against this
project's pinned version (Open question 3). A separate bar panel, the
same pattern `CoverageTrendsResults` already uses for its own separate
`TrendsCharts` call ("a second, separate section... not conditionally
hidden," `history/page.tsx:142-152`'s own comment), avoids introducing
that risk and keeps every existing panel completely unchanged.

**No `ReferenceArea` gap-shading on this panel** (Decision 2's direct
consequence) — `gapSpans(points)` would return an empty array for this
field in the common case, since `total` is never null; the sketch below
reflects that this panel simply omits the `{gapSpans(points).map(...)}`
block the other two panels include.

```tsx
// Sketch, not final. Inside TrendsCharts.tsx, ordered first.
<Stack gap={4}>
  <Title order={order} size="h6">
    Trains counted
  </Title>
  <BarChart
    h={180}
    data={points}
    dataKey="bucketKey"
    series={[{ name: 'total', label: 'Trains counted', color: 'teal.6' }]}
    xAxisProps={xAxisProps}
  />
</Stack>
```

### 6. Copy: distinct honesty text per section, reusing each section's existing paragraph rather than inventing a third

**Chosen**: no new standalone paragraph. Fold one sentence into each
section's *existing* honesty-copy `<Text c="dimmed">` block
(`TrendsResults.tsx:76-81`, `HalfHourlyTrendsResults.tsx:91-96`,
`CoverageTrendsResults.tsx:76-79`) naming what the new panel shows and
that it is never gapped even when the rate chart above/below it is —
e.g., appending to `TrendsResults.tsx`'s existing paragraph: "The trains-counted
chart above always shows the real count, even for gapped periods — a low
number there is exactly why a period may show as a gap in the rate
charts." This keeps the "must not be softened or dropped" existing
sentences (`TrendsResults.tsx:70-75`'s comment) completely untouched,
rather than risking a rewrite of already-carefully-worded, comment-flagged
copy.

### 7. Composing with the (unlanded) configurable-granularity work: this feature needs no separate design for it — `total` is a sum, and sums-of-sums compose for free

**Not designed here as a separate integration, because none is needed.**
If/when the granularity plan's `sub_daily_stats_for_range`
(`crates/api/src/data/queries.rs`, per that plan's Task 1 sketch) ships,
it derives its 1-hour/6-hour rows by summing the same six raw columns
`half_hourly_stats_for_range` already selects, `total` included — the
same "summing sums is lossless" finding that plan's own Correction 4
already established for `delayed`/`cancelled`/`skipped`/`running_count`/
`delay_minutes_sum` applies identically to `total`. That plan's own
sketch of `sub_daily_stats_to_json`/`LineHourlyStats`/`LineSixHourlyStats`
(that plan's Judgment call 3) already carries `total` through unchanged
under the new `bucketStart` key. Once (if) that plan's `TrendsResults`
becomes a `granularity`-branching component, this document's `ChartPoint.total`
field and the new `<BarChart>` panel need **zero** additional changes —
every one of the four tiers' fetch functions already returns a row shaped
with a `total` field (Decision 4's finding), so whichever `toChartPoints`-shaped
mapper that plan's Task 4-ish work ends up writing per tier gets this
feature for free by including the same one non-ternary `total: row.total,`
line this document's Decision 4 already specifies for the three
mappers that exist today. **This is presented as a finding, not a
task this document needs to schedule** — there is no coupling to build,
only an observation that none is needed.

## Reuse assessment

- **`ChartPoint`**: widened by one required field (`total: number`),
  otherwise unchanged. Every existing consumer (`gapSpans`,
  `referenceAreaBounds`, the two existing `<LineChart>` panels) is
  unaffected, since neither reads or depends on the new field's presence.
- **`TrendsCharts`**: reused, with one new panel added (Decision 5). The
  `granularity` prop, `xAxisProps` computation, and both existing panels
  are untouched.
- **`toChartPoints`/`toHalfHourlyChartPoints`/`toCoverageChartPoints`**:
  each gets one new non-ternary line. No change to their sparse-floor
  logic, gap-null logic, or return type shape beyond the one new field.
- **Backend**: fully reused, verbatim, at every layer (`DailyStatsRow`/
  `HalfHourlyStatsRow`, `daily_stats_to_json`/`half_hourly_stats_to_json`,
  `LineDailyStats`/`LineHalfHourlyStats`/`LineDailyCoverageStats`,
  `getLineDailyStats`/`getLineHalfHourlyStats`/`getLineDailyCoverageStats`)
  — the `total` field already flows through every one of these
  unmodified; this feature only starts reading it one layer further,
  inside the two Server Components' mapper functions.

## Non-goals

- **Any new backend column, query, route, or migration.** `total` already
  exists and is already on the wire (Decision 4).
- **A stacked composition chart of `delayed`/`cancelled`/`skipped` against
  `total`.** Rejected — overlapping, non-partitioning categories would
  make the stack's height exceed `total` and silently mislead (Decision 5).
- **A "peak concurrent trains" or "trains simultaneously running" metric.**
  Nothing in the current schema captures concurrency — `total` is a
  period-cumulative *first-observed* distinct-train count (Decision 1's
  caveat), not a live snapshot of how many trains were in transit at any
  one instant. Building a genuine concurrency metric would require a
  wholly different aggregation (tracking open/close intervals per
  service, not a per-cycle dedup ledger) and is not attempted here.
- **Fixing the full-coverage historical rollup's missing dedup step.**
  Flagged as a real, pre-existing risk for whoever eventually builds a
  real full-coverage producer (Decision 3, Open question 1) — not solved
  or scoped here, since no producer exists yet to test a fix against.
- **Backfilling entirely-missing buckets (no DB row at all for a given
  period) with an explicit zero/placeholder point.** Pre-existing
  behavior, unrelated to this feature: every existing `ChartPoint`-based
  panel already only renders buckets the API actually returned a row for;
  a calendar day the aggregator never ran a cycle for produces no point
  on any panel today, and this document does not change that.
- **Recalibrating `SPARSE_DATA_FLOOR_CYCLES`/`SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`/
  `SPARSE_DATA_FLOOR_WINDOWS`, or changing which fields they gate.** Those
  three constants keep gating exactly the four fields they already gate;
  this feature's new field is deliberately excluded from that gate
  (Decision 2), not added to it.
- **Any change to the granularity-tier feature's own files** (per its
  plan's explicit non-goals list) — this document only observes that its
  design composes with a not-yet-built feature, it does not implement,
  extend, or re-scope that feature (Decision 7).
- **A tooltip/hover-detail redesign.** Whether the "first observed, not
  concurrently active" nuance (Decision 1's caveat) needs a dedicated
  tooltip versus just a chart subtitle is left to implementation-time
  judgment — see Open question 2.

## Open questions / risks

1. **The single biggest open risk: if a real full-coverage producer is
   ever built without also building a per-service dedup step for the
   historical coverage-stats write path, this chart's full-coverage panel
   will show an inflated, non-comparable number relative to its LDBWS
   sibling — the exact same failure mode the `dedup` module was built to
   fix for LDBWS, recurring for a different data source.** This document
   surfaces the gap (Decision 3, "Current relevant state") but does not
   fix it, since there is no real producer yet to validate a fix against.
   Whoever designs the eventual full-coverage producer should read this
   section before wiring `record_daily_coverage_stats`/
   `record_half_hourly_coverage_stats` to anything real.
2. **Whether "trains newly observed this bucket, not trains active during
   this bucket" (Decision 1's caveat) needs an explicit tooltip or footnote,
   beyond a chart title, especially at the finer sub-daily granularities**
   (existing half-hourly, and the two not-yet-built intermediate tiers) —
   a viewer drilling into 30-minute buckets is more likely to misread a
   low bar as "almost no trains were running then" than a viewer looking
   at a daily bucket, where "first observed this day" and "ran this day"
   are the same thing for all practical purposes. Left as an
   implementation-time copy decision.
3. **Whether `@mantine/charts`' `BarChart` (pinned `9.5.2`) accepts the
   same `xAxisProps`/category-axis shape `LineChart` does**, so the new
   panel's x-axis ticks line up identically with the two existing panels'
   under every `granularity` value — confirmed only that the component
   exists in the installed package (`node_modules/@mantine/charts/esm/BarChart/`),
   not verified against a real render. If it does not compose cleanly, a
   third `<LineChart>` (single series, no `ReferenceArea` shading per
   Decision 2) is the fallback, at some cost to the "count vs. rate"
   visual distinction Decision 5 argues for.
4. **Whether `sample_cycles` should also surface somewhere near this new
   panel** (e.g., a small "based on N poll cycles" caption) as a second,
   complementary reliability signal, or whether `total` alone answers what
   the brief actually asked for. The brief's own framing ("2 trains or 200
   trains") is answered fully by `total`; `sample_cycles` remains a
   purely internal gating signal in this document's proposal, not
   something newly surfaced to the viewer.
5. **This document was written against the pre-granularity-plan codebase
   (Correction 4) — if the granularity plan lands mid-implementation of
   this feature, the three-mapper-functions-become-N-mapper-functions
   shape described in Decision 4/7 could shift** (e.g. if that plan's
   eventual `TrendsResults` collapses all four tiers' fetch+map logic into
   one internal `switch`, this feature's "add one line to each mapper"
   instruction would need to become "add one line to the one switch,
   once, per tier-case" instead). Not a design risk, since the underlying
   `total` field and its non-ternary treatment are unchanged either way —
   only an implementation-sequencing note for whoever picks this up.
