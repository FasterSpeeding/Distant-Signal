# Design: Trend Chart Granularity — Hourly Line-Info Preview, Daily History Page

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md` (the
original Trends-chart design this spec extends) and
`docs/superpowers/specs/2026-09-02-line-history-chart-fixes-design.md` (the
most recent, not-yet-merged rework of the same chart components — read in
full from its own branch, not `main`, since `main` is stale relative to it;
see Current relevant state). No implementation plan or code is included
here — that is a separate, later step in this repo's process. Every sketch
below (schema, route shapes, component tree) is marked as a sketch, not
final code.

## Goal

Make the "Recent trends" charts more granular in two different ways, in two
different places:

1. **The line-info-page embed** (`frontend/app/lines/[id]/page.tsx`) should
   show **hourly** data over the **past 24 hours**, instead of its current
   daily-over-7-days view.
2. **The dedicated `/lines/[id]/history` page's Trends tab** should show
   **daily** data over whatever date range the user has selected — which,
   per the investigation below, is already exactly what it does today.
   This half of the task is confirmation, not redesign.

## Corrections to the brief's assumptions (recorded for posterity)

Following this repo's own `2026-08-30-inferred-time-ranges-design.md` /
`2026-08-31-line-history-graphics-design.md` precedent of recording where
direct code inspection overturned the brief's framing:

1. **The line-info page is not greenfield for this feature.** It already
   renders a "Recent trends (last 7 days)" section
   (`frontend/app/lines/[id]/page.tsx:161-184`) that reuses
   `TrendsResults`/`TrendsCharts` — the exact same daily-rollup components
   the dedicated history page's Trends tab uses — wholesale, with a fixed
   `resolveRange({}, now)` call that always resolves the `7d` preset
   (`frontend/lib/history.ts:182-187`). This task is about changing that
   embed's granularity and window, not adding a chart where none existed.
2. **`TrendsCharts.tsx`/`TrendsResults.tsx` were substantially reworked
   very recently, on `worktree-agent-a8ea6b81a3cd9cb13`, not yet merged to
   `main`** (`git log --oneline main..worktree-agent-a8ea6b81a3cd9cb13`
   shows 9 unmerged commits as of this writing, spec-documented in
   `2026-09-02-line-history-chart-fixes-design.md`). That branch added a
   legend, per-series `strokeDasharray`, a `valueFormatter`, an exported
   `gapSpans` helper (plus `referenceAreaBounds`) rendering `<ReferenceArea>`
   bands across contiguous sparse-data gap runs, x-axis right-edge padding,
   and a bounded empty-state treatment. This spec reads and builds on that
   branch's actual file contents (fetched via `git show
   worktree-agent-a8ea6b81a3cd9cb13:...`), not `main`'s stale versions.
3. **Neither existing table can produce hourly granularity — this is not a
   "write a new query over old data" problem.** `line_status_history` is
   written only on a genuine status change and explicitly strips
   `sample_stats` from its own change-diff comparison before deciding to
   write at all (`crates/aggregator/src/queries.rs:260-306`'s
   `write_line_status`, corroborated by the newer migration's own comment:
   "`line_status_history` cannot serve as a `SampleStats` time series — its
   own write path... deliberately strips `sample_stats` before deciding
   whether a row changed, so most cycles' numbers are never recorded
   anywhere else," `crates/api/migrations/20260831090001_line_status_daily_stats.sql:4-9`).
   `line_status_daily_stats` — the table the *existing* daily charts
   actually read — is written incrementally by the aggregator, one
   accumulating row per `(line_id, day)`
   (`crates/api/migrations/20260831090001_line_status_daily_stats.sql:33-59`,
   `crates/aggregator/src/queries.rs:358-400`'s `record_daily_stats`), and
   the accumulation collapses every cycle's contribution into that one
   row — no intra-day resolution survives to be queried back out later.
   Getting real hourly numbers requires new aggregator write-path
   infrastructure, not a new SQL query. See Decision 1.
4. **This means the "day-level aggregation" the task brief asked about
   (frontend `groupHistoryByDay`-style vs. backend SQL `GROUP BY`/
   `date_trunc` vs. client aggregation) is none of those.** It is an
   **aggregator-side incremental accumulate-upsert**, called once per line
   per poll cycle (`crates/aggregator/src/main.rs:117-127`'s `run_cycle`),
   fed by a **per-Darwin-`service_id` dedup pass**
   (`crates/aggregator/src/dedup.rs`'s `dedup_new_sample_stats` against a
   `SeenServiceLedger` keyed `(line_id, day)`) so that a train dwelling in
   the LDBWS departure-board window across many consecutive polls is
   counted once, not once per poll. This is a materially different, and
   materially larger, mechanism than "bucket some rows by day" — and it is
   the mechanism this spec's hourly design has to extend, not bypass.
5. **A genuinely useful simplification falls out of point 4**: the
   per-cycle dedup pass already answers "is this the first time today this
   physical train has been seen" — which is *also* the exact question an
   hourly rollup needs answered for "first time this hour." The existing
   per-cycle `deduped` value computed once for the daily write
   (`crates/aggregator/src/main.rs:121`) can be reused verbatim for a
   second, hourly accumulate-upsert, with **no second dedup ledger and no
   second dedup pass**. See Decision 2 — this is the single most
   consequential finding in this spec, since it turns "hourly needs a
   parallel dedup subsystem" into "hourly needs one more SQL write per
   cycle, fed from data already computed."
6. **The dedicated history page's date-range picker is already
   calendar-day granularity, confirmed by reading it directly, not
   assumed.** `HistoryRangePicker.tsx`'s `toCalendarDay` helper
   (`frontend/app/lines/[id]/history/HistoryRangePicker.tsx:9-11`) and its
   Mantine `DatePickerInput type="range"` only ever deal in whole calendar
   days; `resolveRange`'s presets (`7d`/`30d`,
   `frontend/lib/history.ts:152-188`) are day-counted; `getLineDailyStats`
   itself takes `londonDayKey(from)`/`londonDayKey(to)` day strings
   (`frontend/app/lines/[id]/history/TrendsResults.tsx:38`). Point 2 of
   this task ("should show daily data... this is presumably closer to, or
   identical to, its current behavior") is confirmed **identical**, not
   merely close — no change is proposed for this half. See Decision 10.

## Current relevant state (verified against the live worktree + `worktree-agent-a8ea6b81a3cd9cb13`)

**Line-info-page embed** (`frontend/app/lines/[id]/page.tsx:161-184`):
renders `<Title>Recent trends (last 7 days)</Title>` then
`<Suspense fallback={<Skeleton height={280} />}><TrendsResults id={id}
from={trendsRange.from} to={trendsRange.to} /></Suspense>`, where
`trendsRange = resolveRange({}, now)` always resolves the `7d` preset
(line 104). The comment at lines 165-183 explicitly documents this as
reusing the history page's own Trends tab component "wholesale... rather
than re-deriving any of that here."

**`TrendsCharts.tsx`** (read in full from `worktree-agent-a8ea6b81a3cd9cb13`):
a Client Component taking `points: ChartPoint[]` and rendering two
`@mantine/charts` `LineChart`s:
- The rate chart: three series (`delayRate`/`cancellationRate`/`skipRate`),
  `dataKey="day"`, `withLegend`, per-series `strokeDasharray`,
  `valueFormatter={(v) => \`${(v*100).toFixed(1)}%\`}`,
  `connectNulls={false}`, `xAxisProps={{ padding: { right: 12 } }}`, and a
  `children` slot rendering one `<ReferenceArea>` per contiguous gap span
  from `gapSpans(points)`.
- The average-delay chart: one series (`avgDelayMinutes`), same
  `dataKey="day"`, `valueFormatter`, `connectNulls`, edge padding, and gap
  bands, no legend.
- `gapSpans(points: { day: string; delayRate: number | null }[]): {
  startDay: string; endDay: string }[]` — a pure helper deriving contiguous
  runs of gap days. **`day`/`startDay`/`endDay` are hardcoded field names,
  not a generic "bucket key"** — its own doc comment explicitly notes this
  couples it to `toChartPoints`'s "all four fields null together" invariant.
  Its test file (`TrendsCharts.test.tsx`, read in full) literally names
  every case by day (`'2026-08-01'`, `'2026-08-02'`, ...).
- `referenceAreaBounds` — widens an isolated single-day gap span to its
  immediate `points` neighbors, because `@mantine/charts`' `LineChart` uses
  a Recharts **point-scale category axis** (not a banded one): a zero-width
  `x1 === x2` span renders no `<ReferenceArea>` at all on a point scale,
  confirmed via a live dev-server render per that function's own doc
  comment. This point-scale-axis behavior applies identically regardless of
  what the category values represent (day strings today; hour labels would
  behave the same way) — no hourly-specific risk here beyond what already
  had to be solved for daily.

**`TrendsResults.tsx`** (read in full from the same branch): an async
Server Component. `SPARSE_DATA_FLOOR_CYCLES = 20` (a placeholder, per its
own comment, calibrated against a **day's** maximum coverage). `toChartPoints`
nulls all four rate/delay fields together for any day where
`row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES`. `getLineDailyStats(id,
londonDayKey(from), londonDayKey(to))` is the only fetch. A zero-row result
renders a bounded `<Paper>` "Not enough sampled data yet for this line."
empty state. The rendered "honesty copy" text (unchanged since the original
spec, `TrendsResults.tsx`'s own comment: "Must not be softened or dropped")
reads: "Rates shown count each distinct train once per day, based on its
status the first time it was seen that day — not a share of poll cycles."

**`chartPoint.ts`**: `ChartPoint { day: string; delayRate: number | null;
cancellationRate: number | null; skipRate: number | null; avgDelayMinutes:
number | null; sampleCycles: number }` — `day` is a plain field name, not a
discriminated/generic bucket-key type.

**Backend write path — the actual "day-level aggregation" mechanism**:
- `crates/aggregator/src/main.rs:117-130` (`run_cycle`): once per
  aggregation cycle (`poll_interval_secs`, default `60`,
  `crates/aggregator/src/config.rs:47`), for every line with raw sample
  coverage this cycle (`lines_with_sample_coverage`, lines 171-181):
  ```rust
  let today = queries::london_calendar_day(chrono::Utc::now());       // line 117
  let deduped = dedup::dedup_new_sample_stats(dedup_ledger, line_id, today, line, &samples, defaults); // line 121
  queries::record_daily_stats(pool, line_id, today, deduped.as_ref()).await?;                          // line 125
  ```
- `crates/aggregator/src/dedup.rs`'s `SeenServiceLedger` (lines 106-107):
  `seen: HashMap<(String, NaiveDate), HashSet<String>>` — in-memory,
  process-lifetime, keyed by `(line_id, day)`. `dedup_new_sample_stats`
  returns `Some(SampleStats)` built only from Darwin `service_id`s not
  already marked seen for that `(line_id, day)` this period, `None` on a
  cycle where every currently-visible service was already counted earlier
  that day (the common case). `mark_seen` (line 120) is the only place a
  service gets recorded as seen — deliberately in-memory, not persisted;
  see `dedup.rs`'s module doc for why that's judged sufficient (a bounded,
  one-off over-count around rare restarts, not a systemic gap).
- `record_daily_stats` (`crates/aggregator/src/queries.rs:358-400`): an
  `INSERT ... ON CONFLICT (line_id, day) DO UPDATE SET ... = ... + EXCLUDED....`
  accumulate-upsert. `stats: Option<&common::SampleStats>` — `None` still
  counts the cycle (`sample_cycles += 1`) but contributes zero to every sum;
  this is the **common** case once a line's currently-dwelling services have
  already been counted for the day.
- Schema (`crates/api/migrations/20260831090001_line_status_daily_stats.sql:33-59`):
  `line_status_daily_stats(line_id TEXT, day DATE, sample_cycles BIGINT,
  total BIGINT, delayed BIGINT, cancelled BIGINT, skipped BIGINT,
  running_count BIGINT, delay_minutes_sum DOUBLE PRECISION, PRIMARY KEY
  (line_id, day))`, indexed `(line_id, day)`. `day` is deliberately a plain
  **Europe/London calendar day** (`london_calendar_day`,
  `crates/aggregator/src/queries.rs:331-338`), not UTC and not the
  aggregator's separate rail-day-02:00 convention used elsewhere for
  incident staleness — chosen to match the Timeline tab's own
  `londonDayKey` grouping (original spec, Decision 1).
- Read path: `GET /Line/{id}/Stats/{from}/to/{to}`
  (`crates/api/src/routes/line_status.rs:42`) backed by
  `daily_stats_for_range` (`crates/api/src/data/queries.rs:683-720`), a
  plain `SELECT ... WHERE line_id = $1 AND day BETWEEN $2 AND $3 ORDER BY
  day` — rates (`delayRate`/`cancellationRate`/`skipRate`/`avgDelayMinutes`)
  are derived at read time from the stored sums, never stored pre-divided.
  Fetched by `frontend/lib/api.ts:129-137`'s `getLineDailyStats`, typed by
  `frontend/lib/types.ts:123-134`'s `LineDailyStats`.
- Retention: `history_retention_days` (default `7`,
  `crates/aggregator/src/config.rs:54`) governs `line_status_history`,
  pruned by `prune_history` (`queries.rs:313-320`, `DELETE ... WHERE
  computed_at < NOW() - ($1 || ' days')::interval`) — **no minimum-value
  validation exists** (a plain `i64` CLI/env flag; a deployer could set it
  to `0`). `daily_stats_retention_days` (default `300`,
  `crates/aggregator/src/config.rs:56-69`) governs `line_status_daily_stats`,
  pruned by `prune_daily_stats` (`queries.rs:402-408`) — `300`, not `365`,
  specifically because RDM's Live Departure Board licence (Schedule 1 §9)
  requires deleting LDBWS-derived data within a year (see that field's own
  doc comment and `docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`).
  **Neither knob governs data this spec's hourly design depends on** — see
  Decision 4 for a new, dedicated retention knob.
- Helm chart naming precedent: `aggregator.historyRetentionDays` /
  `aggregator.dailyStatsRetentionDays` (`charts/distant-signal/values.yaml:487,496`,
  wired through `charts/distant-signal/templates/aggregator-deployment.yaml:80,82`).

**Timezone/formatting precedent**: `frontend/lib/dateFormat.ts` is this
app's single locale/timezone decision point (its own doc comment: "This is
a UK rail product, so dates are en-GB and times are London wall-clock").
It already exports a ready-made `formatTime` (`TIME` formatter,
`timeStyle: 'short'`, e.g. `"19:56"`, lines 30-33, 59-62) — exactly the
shape needed for hourly x-axis tick labels — and `londonDayKey` (the
`en-CA` `DAY_KEY` formatter, lines 35-43, 64-66) used throughout the
existing daily code path. No UTC-timestamp display exists anywhere in this
app; Europe/London wall-clock is the confirmed, established convention for
every existing time display, not an assumption.

**Live-refresh precedent**: `frontend/components/AutoRefresh.tsx` calls
`router.refresh()` every 30s (`REFRESH_INTERVAL_MS = 30_000`), re-running
every Server Component's data fetch on the current route, already
including `TrendsResults`'s fetch today. A new hourly embed inherits this
automatically — no new live-update plumbing is needed for a still-filling
current-hour bucket to visibly update.

**Volume**: at the current catalogue's scale (~105 lines,
`2026-08-31-line-history-graphics-design.md`'s own count), the existing
daily route already serves the 7-day embed on every line-info-page view
with no caching layer (`cache: 'no-store'` throughout `lib/api.ts`) and no
reported performance concern on record. A 24-hour hourly range is at most
25 rows per line (24 complete hours + 1 in-progress), read through the
exact same `(line_id, <bucket>)` composite-key index shape the daily table
already uses successfully at this volume — no reason to expect this to be
materially more expensive per request than the existing 7-row daily fetch
it's replacing on this specific page.

## Decisions

### 1. New backend storage is required — no existing table can be queried into hourly shape

Three options considered:

- **(a) Read `line_status_history` and bucket its rows by hour at query
  time (client- or server-side).** Rejected outright, for the reason
  Correction 3 establishes: that table's own write path strips
  `sample_stats` from its change-diff and only writes on a genuine status
  change. Bucketing it by hour would not produce "hourly rate data with
  occasional real transitions," as the task brief speculated — it would
  produce **almost entirely empty hours**, since most hours see zero
  history rows at all (the original spec's own measured volume: ~16-37
  rows/**day**, total, across an entire line — WCML/SWR-Alton,
  `2026-08-31-line-history-graphics-design.md` Correction 1). This isn't a
  "sparse but honest" hourly signal; it's a table that was never designed
  to hold this information at all.
- **(b) Add intra-day resolution to `line_status_daily_stats`.** Not
  possible after the fact — `record_daily_stats`'s `ON CONFLICT DO UPDATE
  SET sample_cycles = ... + EXCLUDED....` accumulation is a genuine
  collapse: once two cycles' contributions are summed into one `(line_id,
  day)` row, which specific hour each contribution happened in is gone.
  Reading this table can only ever answer "how did this day look," never
  "how did 14:00-15:00 look."
- **(c) (chosen) A new table, `line_status_hourly_stats`, written by a new,
  parallel accumulate-upsert call at the exact same call site that already
  writes the daily table.** Mirrors Decision 1 of the original design spec
  exactly, one level down in granularity. See Decision 3 for the schema
  sketch.

### 2. Reuse the existing per-cycle dedup result for the hourly write — no second ledger, no second dedup pass

**Chosen: feed the same `deduped: Option<&SampleStats>` value
`crates/aggregator/src/main.rs:121` already computes once per line per
cycle into a second call, `record_hourly_stats(pool, line_id, hour_start,
deduped.as_ref())`, immediately alongside the existing
`record_daily_stats` call.** This works because "is this the first time
today this Darwin `service_id` has been seen" (the day ledger's question)
and "is this the first time this hour this service has been seen" are, for
attribution purposes, the *same event* — a service is only ever "new" once,
at the moment of its first sighting, and that moment falls in exactly one
hour of exactly one day. Attributing its whole contribution to that
first-sighting hour is the natural hourly analog of the daily table's
existing "first time it was seen that day" attribution (the honesty copy
already shown on the chart, `TrendsResults.tsx`'s own comment marked "Must
not be softened or dropped" — this spec extends the same wording to "that
hour" for the hourly view, not a new tradeoff).

**Alternative considered and rejected: a second, hour-keyed
`SeenServiceLedger` running its own independent
`dedup_new_sample_stats` pass.** Rejected on two grounds:
- **Wasted work for no better answer** — doubling the per-cycle dedup CPU
  and memory cost (a second `HashMap<(String, NaiveDate-or-hour), HashSet<String>>`
  alongside the existing one) for a computation whose result would, for the
  overwhelming majority of services (anything that doesn't happen to
  straddle an hour boundary while still being "new" relative to the day),
  be identical to what the day ledger already produces.
- **Would actively break a useful invariant.** An hour-scoped ledger, run
  independently, would flag a service as "new this hour" the first time it
  appears within *that specific hour's* window — even if the exact same
  service was already counted (and summed into `total`/`delayed`/etc.) in
  an *earlier* hour that same day, because the day-ledger and hour-ledger
  would disagree about "already seen." That would make the 24 hourly rows
  for a day **not sum to** that day's `line_status_daily_stats` row — a
  real, silent data-integrity divergence between two views of what should
  be the same underlying trains, and a much harder bug to notice than
  "hourly charting doesn't exist yet." Reusing the single day-scoped
  dedup result for both writes makes this invariant hold by construction:
  the same `deduped` value is the source of truth for both.

This means **`crates/aggregator/src/dedup.rs` needs zero changes** — no new
period type, no ledger generalization, no second `mark_seen`/`prune_before`
call site. The only new aggregator code is a second SQL accumulate-upsert
function and one new call site line.

### 3. New table schema: `line_status_hourly_stats`, mirroring `line_status_daily_stats` column-for-column

Sketch (not final):
```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_line_status_hourly_stats.sql
-- Same shape as line_status_daily_stats (20260831090001), one level finer.
-- Fed from the SAME per-cycle deduped SampleStats already computed for the
-- daily rollup (Decision 2) -- not a second dedup pass.
CREATE TABLE line_status_hourly_stats (
    line_id           TEXT             NOT NULL,
    hour_start        TIMESTAMPTZ      NOT NULL,  -- UTC hour boundary, see Decision 4
    sample_cycles     BIGINT           NOT NULL DEFAULT 0,
    total             BIGINT           NOT NULL DEFAULT 0,
    delayed           BIGINT           NOT NULL DEFAULT 0,
    cancelled         BIGINT           NOT NULL DEFAULT 0,
    skipped           BIGINT           NOT NULL DEFAULT 0,
    running_count     BIGINT           NOT NULL DEFAULT 0,
    delay_minutes_sum DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (line_id, hour_start)
);
CREATE INDEX line_status_hourly_stats_line_hour ON line_status_hourly_stats (line_id, hour_start);
```
The only structural difference from `line_status_daily_stats` is the key
column (`hour_start TIMESTAMPTZ` vs. `day DATE`) — every numeric column,
the accumulate-upsert shape, and the read-time rate-derivation approach
(Decision 4 of the original spec: never store pre-divided rates) carry over
unchanged.

### 4. Bucket boundary: plain UTC hour truncation, not an Europe/London local hour — a deliberate divergence from the daily table's own convention

The daily table's `day` column is deliberately an Europe/London **calendar**
day (`london_calendar_day`, `crates/aggregator/src/queries.rs:331-338`),
chosen so a line's daily figures line up with what a user actually sees
grouped on the Timeline tab. That reasoning does not transfer to an hourly
bucket used only to build a **rolling 24-hour window**, and following it
anyway would import a real, unnecessary edge case: an Europe/London local
hour boundary has a **23-hour** day at the BST "spring forward" transition
and a **25-hour** day (with one repeated wall-clock hour) at "fall back."
`crates/aggregator/src/queries.rs`'s own `london_calendar_day` tests (lines
984-1039) already exercise exactly this DST-transition machinery for the
*daily* case — deliberately not needed here.

**Chosen: `hour_start` is a plain UTC instant, truncated to the top of the
hour** (conceptually `computed_at.duration_trunc(Duration::hours(1))`, or
equivalently `date_trunc('hour', computed_at AT TIME ZONE 'UTC')` if ever
computed in SQL). This cleanly separates two different concerns that the
daily table's design necessarily conflates for good reason (a *calendar
day identity* the UI displays as a heading) but that an hourly rolling
window does not need conflated at all: **which bucket an instant belongs
to** (unambiguous under a fixed-offset truncation, no DST-transition
double-counting or bucket-skipping) versus **how a bucket's start is
displayed to a viewer** (already solved — `formatTime`,
`frontend/lib/dateFormat.ts:59-62`, converts any instant to Europe/London
wall-clock for display, independent of how it's stored). A viewer never
sees `hour_start` directly; they see it formatted through the existing
London-wall-clock formatter, so this divergence from the daily table's
storage convention is invisible in the UI.

### 5. Retention: a new, much shorter knob — not a reuse of either existing one

**Chosen: a new `hourly_stats_retention_hours` CLI/env flag** (aggregator
crate, mirroring `history_retention_days`/`daily_stats_retention_days`'s
existing shape), with a proposed default of **48 hours** — a 2x safety
margin over the 24-25 rows the UI actually needs, small enough that no
licence-compliance reasoning (the LDBWS 1-year ceiling that drove
`daily_stats_retention_days` down to 300) is remotely in play. A new
`prune_hourly_stats` mirrors `prune_daily_stats`'s exact shape
(`crates/aggregator/src/queries.rs:402-408`), called unconditionally every
cycle from `run_cycle`.

**Neither existing knob is reusable**: `history_retention_days` (default
`7`) governs a different table entirely (`line_status_history`, not read by
this feature at all — Decision 1); `daily_stats_retention_days` (default
`300`) is sized for a "trend over weeks/months" use case this hourly view
does not have, and reusing it would mean `line_status_hourly_stats`
accumulating **300 days × 24 rows/line × ~105 lines ≈ 756,000 rows** for
data only the most recent 25 rows of which are ever read by anything — a
real, unforced storage cost for a table whose only consumer is a rolling
24-hour window.

**Storage**: at 48h retention and the current catalogue size, this table
holds at most `105 lines × 48 rows ≈ 5,040 rows` — trivial, several orders
of magnitude smaller than even the daily table's own "~38k rows/year"
figure from the original spec.

### 6. New read route and query, mirroring the daily route's shape but keyed on instants, not `NaiveDate`

```
GET /Line/{id}/Stats/Hourly/{from}/to/{to}
```
`from`/`to` as `DateTime<Utc>` path segments — matching the *existing*
history route's own precedent (`line_status_history_for_range`,
`crates/api/src/data/queries.rs:637-660`, already proven to parse
`DateTime<Utc>` correctly in a multi-segment `Path` extractor, per the
original spec's own Open question 6) rather than the daily route's
`NaiveDate` — an hour bucket has no calendar-day analog to round-trip
through. New `hourly_stats_for_range` query, structurally identical to
`daily_stats_for_range` (`crates/api/src/data/queries.rs:683-720`):
```rust
pub struct HourlyStatsRow {
    pub hour_start: chrono::DateTime<chrono::Utc>,
    pub sample_cycles: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

pub async fn hourly_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HourlyStatsRow>> {
    // SELECT ... FROM line_status_hourly_stats
    // WHERE line_id = $1 AND hour_start BETWEEN $2 AND $3 ORDER BY hour_start
}
```
Rate fields (`delayRate`/`cancellationRate`/`skipRate`/`avgDelayMinutes`)
derived at read time from the stored sums, identical guard-against-`total:
0` behavior as `daily_stats_for_range`. Response shape (camelCase,
matching this crate's existing convention): `hourStart` (ISO instant) in
place of `day`, everything else identical to `LineDailyStats`'s shape.

### 7. Include the current, still-filling partial hour in the response

**Chosen: no special-casing** — the query above naturally includes
whatever row exists for the in-progress current hour, exactly as the daily
route already includes "today," a partial day, without any dedicated
handling. Its `sample_cycles` will legitimately be low early in the hour
and climb as more cycles land; the sparse-data floor (Decision 8) already
handles "too little coverage yet" as a gap, the same honest mechanism
already in place for a brand-new day. Combined with `AutoRefresh`'s
existing 30s `router.refresh()` cadence, the current hour's point visibly
fills in without any new live-update code — this is inherited behavior,
not a new mechanism.

**Alternative considered and rejected: only return complete hours.**
Would need an explicit `WHERE hour_start < <current hour start>` filter,
adds a special case, and makes the chart's rightmost, freshest point always
one hour stale relative to what `line_status` (the live, current-cycle
view shown elsewhere on the same page) already displays — a worse, not
better, user experience for a page whose whole point is showing recent
trend alongside current live status.

### 8. Sparse-hour floor needs its own, separately-calibrated value — `SPARSE_DATA_FLOOR_CYCLES` does not transfer

`SPARSE_DATA_FLOOR_CYCLES = 20` was calibrated against a **day's** maximum
possible coverage: `86400 / poll_interval_secs` = 1,440 cycles at the
default 60s cadence, so `20` represents roughly **1.4%** minimum coverage.
An **hour's** maximum possible coverage is `3600 / poll_interval_secs` = 60
cycles at the same default — reusing `20` as-is would demand **33%**
minimum coverage for an hour to count as non-sparse, a vastly stricter bar
that would gap out a large fraction of hours with entirely normal, healthy
sampling. This is a genuinely new number, not a reused constant, and — like
`SPARSE_DATA_FLOOR_CYCLES` itself (an explicitly-flagged placeholder,
`TrendsResults.tsx`'s own comment citing "this plan's own 'Open judgment
calls' section") — this spec does not attempt to derive the correct value
from first principles. A **proportional** placeholder (roughly the same
~1.4% of an hour's ceiling, i.e. `~1` cycle) would be too permissive to
mean anything; an **absolute** floor in the same rough neighborhood as
today's `20` (i.e., "at least a third of the hour actually had live
coverage") is the more defensible starting placeholder, but this is
flagged as unresolved — see Open questions/risks.

Importantly, this is a **calibration** problem, not the data-availability
problem the task brief worried about: because this table is coverage-driven
(written every cycle regardless of whether the line's *status* changed —
unlike `line_status_history`), a normally-trafficked hour has plenty of
real `sample_cycles` to work with; the risk is only in getting the floor
number itself right, not in the underlying data not existing at all
(Correction 3/Decision 1 already rule that concern out).

### 9. Component reuse: generalize the shared chart leaf; do not fork a second component tree

**Chosen: generalize `ChartPoint`, `gapSpans`/`referenceAreaBounds`, and
`TrendsCharts`'s hardcoded `dataKey="day"` to be bucket-key-agnostic, and
reuse them for both the existing daily view and the new hourly view.**
Concretely (sketch, not final — exact naming is an implementation
choice): rename `ChartPoint.day: string` to a more general bucket-key field
usable for either a `"YYYY-MM-DD"` day string or an ISO hour-start label;
add a `bucketKey`/`dataKey` prop to `TrendsCharts` in place of the two
hardcoded `dataKey="day"` occurrences; generalize `gapSpans`'s `{ day,
startDay, endDay }` shape to a generic `{ key, startKey, endKey }` (or
equivalent) — a real, non-trivial rename given its **own tests are
literally day-named today** (`TrendsCharts.test.tsx`, confirmed on the
branch), not just a type-signature tweak.

**Alternative considered and rejected: fork a second, hourly-specific
chart component.** Rejected because it would either duplicate or silently
drop every piece of the just-shipped (branch `worktree-agent-a8ea6b81a3cd9cb13`)
accessibility/polish work — the legend, per-series dash patterns, the
`valueFormatter` tooltip formatting, the gap-span shading, and the
right-edge axis padding — none of which is granularity-specific in any way.
A fork would need to either re-implement all of it a second time (real
duplicated surface area and a second place for the next fix to be missed)
or ship the hourly view visibly worse than the daily one it sits right next
to on the same line-info page. Generalizing the shared leaf is the smaller,
safer diff, and keeps both call sites automatically benefiting from any
future chart-layer fix — the same "shared layer, no preview-specific work"
principle `2026-09-02-line-history-chart-fixes-design.md`'s Decision 6
already established for the *existing* daily preview/full-page split.

### 10. A new, structurally-parallel Server Component for the hourly fetch — not a reuse of `TrendsResults` itself

**Chosen: a new component (e.g. `HourlyTrendsResults.tsx`, or an existing
`TrendsResults` parameterized by a `granularity` prop — an implementation
choice not fixed here) that fetches the new hourly endpoint and produces
generalized `ChartPoint`-shaped data, feeding the same generalized
`TrendsCharts` from Decision 9.** `TrendsResults.tsx`'s current body is
genuinely daily-specific in ways worth keeping separate rather than
branching internally: its fetch (`getLineDailyStats(id, londonDayKey(from),
londonDayKey(to))`), its `SPARSE_DATA_FLOOR_CYCLES` threshold (Decision 8
establishes this must differ for hourly), and its honesty copy text ("...
the first time it was seen **that day**...", which needs to read "**that
hour**" for the new view, per Decision 2's attribution) are all
granularity-specific content, not shared plumbing. The **shared, reusable**
part is exactly `TrendsCharts` (Decision 9) — the rendering leaf, not the
data-fetching/labeling Server Component around it.

### 11. Line-info-page embed: keep the accessibility treatment, shrink for context, no range picker

**Chosen**: the hourly embed keeps the legend and per-series dash patterns
(dropping them for a "preview" would be a real accessibility regression on
the page most visitors land on first, for no stated benefit), reduces chart
height for its smaller card context (an implementation-time, screenshot-
verified number — this spec doesn't fix one, consistent with
`2026-09-02-line-history-chart-fixes-design.md`'s own posture of not
pinning exact heights), and has **no date-range picker at all** — the task
frames this view as a fixed rolling 24 hours, and `HistoryRangePicker`'s
whole reason to exist (letting the URL carry a user-chosen range,
`2026-08-31-line-history-graphics-design.md` Decision 5) doesn't apply to a
window that's never user-selectable. "View history" (already present,
`frontend/app/lines/[id]/page.tsx:141-143`) remains the way to reach the
full range picker and the daily Trends tab, same as it is today for the
existing 7-day preview.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                            │
│                                                                              │
│  app/lines/[id]/page.tsx              CHANGED -- embed now fetches the    │
│                                        new hourly Server Component/range   │
│                                        over the fixed last-24h window,     │
│                                        instead of resolveRange({},...)'s   │
│                                        7d daily preset                     │
│  app/lines/[id]/history/page.tsx      UNCHANGED -- already daily over the │
│                                        user-selected range (Decision 10 /  │
│                                        Correction 6)                       │
│  app/lines/[id]/history/HourlyTrendsResults.tsx   NEW -- fetches          │
│                                        getLineHourlyStats, produces        │
│                                        generalized ChartPoint-shaped data,  │
│                                        hourly-specific floor + copy        │
│  app/lines/[id]/history/TrendsResults.tsx         UNCHANGED logic, still  │
│                                        the daily Server Component          │
│  app/lines/[id]/history/TrendsCharts.tsx          GENERALIZED -- bucket-  │
│                                        key-agnostic dataKey/gapSpans,      │
│                                        shared by both granularities        │
│  app/lines/[id]/history/chartPoint.ts             GENERALIZED -- bucket   │
│                                        key field usable by both           │
│  lib/api.ts    + getLineHourlyStats                                       │
│  lib/types.ts  + LineHourlyStats                                          │
└──────────────────────────┬──────────────────────────────────────────────┘
     server-side fetch, no-store, no proxy needed (public route, same
     pattern as the existing getLineDailyStats/getLineStatusHistory)
                            ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ api crate                                                                  │
│  GET /Line/{id}/Stats/Hourly/{from}/to/{to}   NEW route, line_status.rs   │
│  queries::hourly_stats_for_range               NEW query, data/queries.rs │
└──────────────────────────┬──────────────────────────────────────────────┘
                            │ reads
                            ▼
                 line_status_hourly_stats   NEW table (migration owned by
                                             api crate, mirrors
                                             line_status_daily_stats)
                            ▲
                            │ writes, once per line per cycle, fed the SAME
                            │ deduped SampleStats already computed for the
                            │ daily write -- no second dedup pass (Decision 2)
┌──────────────────────────┴──────────────────────────────────────────────┐
│ aggregator crate                                                          │
│  main.rs::run_cycle    EXTENDED -- one new record_hourly_stats call,      │
│                         reusing the existing `deduped`/`today`-adjacent   │
│                         `hour_start` values already in scope              │
│  queries::record_hourly_stats   NEW, mirrors record_daily_stats           │
│  queries::prune_hourly_stats    NEW, mirrors prune_daily_stats            │
│  queries::utc_hour_start        NEW, mirrors london_calendar_day's        │
│                                  shape but truncates to a plain UTC hour  │
│                                  (Decision 4 -- deliberately NOT London-   │
│                                  local, unlike the day case)              │
│  config::Config        + hourly_stats_retention_hours (new knob)          │
│  dedup.rs               UNCHANGED -- Decision 2's whole point             │
└────────────────────────────────────────────────────────────────────────┘
```

## Error handling

- **Sparse/no-change hours** — resolved by Decision 8's separately-
  calibrated floor; not a data-existence problem (Decision 1/Correction 3
  already establish the new table is coverage-driven, written every cycle
  regardless of status changes, unlike `line_status_history`), only a
  threshold-calibration one.
- **A day/hour where `total: 0`** (every contributing cycle had zero
  relevant departures) — identical divide-by-zero guard as
  `daily_stats_for_range` already implements; rate fields computed as
  `0.0` server-side, never `NaN` reaching the frontend.
- **Zero rows across the whole 24h window** (brand-new custom line, or a
  line with no/misconfigured `sample_stations`) — reuses the existing
  "Not enough sampled data yet for this line." empty state verbatim
  (`TrendsResults.tsx`'s current `<Paper>` block), same component, same
  copy, since the underlying condition (no qualifying rows) is identical
  in shape to the daily case.
- **Retention-window edge case, investigated per the task's explicit ask**:
  `history_retention_days` (default `7`, no enforced minimum in
  `crates/aggregator/src/config.rs`, confirmed by reading the file in full)
  and `daily_stats_retention_days` (default `300`) are both **irrelevant to
  this design** — the hourly view is sourced from neither table (Decision
  1), so a misconfigured `history_retention_days` (e.g. an operator setting
  it to `0`, which the current code does not prevent) cannot affect the
  hourly chart at all. This is called out for completeness, as asked, not
  because it's a live risk under the chosen design.
- **The new hourly table's own retention (Decision 5) must stay comfortably
  above 24-25 hours or the rolling window's leading edge would silently
  truncate** — the proposed 48h default (2x margin) makes this unlikely in
  practice, but unlike `history_retention_days`/`daily_stats_retention_days`
  today, no validation floor is proposed here either (consistent with this
  repo's existing precedent of not validating these knobs) — flagged, not
  designed away, in Open questions/risks.
- **Point-scale-axis isolated-gap rendering** (the `referenceAreaBounds`
  widening `TrendsCharts.tsx` already implements for daily gaps) applies
  identically to hourly gaps — a category axis's point-scale behavior
  doesn't depend on what the category values mean, only on their being a
  Recharts category axis at all. No new edge case here beyond what daily
  already solved.

## Testing

Following this repo's existing convention (per both prior Trends specs'
Testing sections):

- **`crates/aggregator/src/queries.rs`**: `record_hourly_stats`
  upsert-accumulation tests mirroring `record_daily_stats`'s existing ones
  (lines 1050-1173 today) — two cycles in the same hour sum correctly, a
  cycle in a new hour starts a fresh row, an hour boundary crossing a day
  boundary doesn't corrupt anything. `prune_hourly_stats` test mirroring
  `prune_daily_stats`'s. A `utc_hour_start` unit test suite mirroring
  `london_calendar_day`'s own DST-transition test names (lines 984-1039) —
  though, per Decision 4, the whole point of the UTC choice is that these
  tests should be *simpler*, with no 23/25-hour-day cases to handle.
- **`crates/aggregator/src/main.rs`**: the single most important new test —
  asserting `record_hourly_stats` is called with the **exact same
  `deduped` value** already passed to `record_daily_stats` within one
  `run_cycle` invocation, not a second independently-computed one. This is
  the concrete, testable form of Decision 2's invariant (hourly sums must
  reconcile to the daily total); a regression here is a subtle,
  hard-to-notice data-integrity bug, not a crash, so it needs an explicit
  test rather than relying on it being "obviously" true from the code.
- **`crates/api/src/data/queries.rs`**: `hourly_stats_for_range` test
  (range filtering, ordering, the `total: 0` guard) mirroring
  `daily_stats_for_range`'s existing test.
- **`crates/api/src/routes/line_status.rs`**: a route test for the new
  `GET /Line/{id}/Stats/Hourly/{from}/to/{to}` endpoint, mirroring the
  existing `/Line/{id}/Stats/{from}/to/{to}` test pattern.
- **`frontend/lib/api.test.ts`**: `getLineHourlyStats` builds the correct
  URL, mirroring `getLineDailyStats`'s existing test.
- **`frontend/app/lines/[id]/history/TrendsCharts.test.tsx`**: re-run the
  existing `gapSpans` test cases (unchanged in substance) against the
  generalized field names, **plus** a parallel set of hourly-labeled
  fixtures — not just a rename of the existing day-named cases, to confirm
  the generalization didn't silently narrow behavior for the new shape it
  now also has to support.
- **A new `HourlyTrendsResults.test.tsx`** (or equivalent): empty-state,
  sparse-hour-gap, and normal-multi-hour-points cases, mirroring
  `TrendsResults.test.tsx`'s existing structure.
- **`frontend/app/lines/[id]/page.test.tsx`**: extend to confirm the embed
  now calls the hourly fetch/component, not the daily one — a guard
  against an accidental copy-paste reuse of the wrong Server Component,
  which would silently ship the old 7-day daily preview unchanged.
- **What stays out of scope for tests, per established precedent**: no
  assertion on Recharts' real SVG/axis-tick rendering for hourly labels —
  this repo's `@mantine/charts` mock strategy (replacing `LineChart` with a
  plain `<div>` capturing only props) is explicitly scoped to prop-level
  assertions, not pixel output, for both granularities equally.

## Explicitly out of scope

- **Any change to the dedicated history page's Trends-tab granularity** —
  confirmed already daily-over-the-selected-range (Correction 6/Decision
  10); no code change is proposed for `frontend/app/lines/[id]/history/page.tsx`
  or `HistoryRangePicker.tsx` beyond what Decision 9's chart-leaf
  generalization touches incidentally (which changes no user-visible
  behavior on that page).
- **A granularity toggle/selector UI** anywhere (e.g. letting a user switch
  the history page itself between daily and hourly views) — not asked for,
  not designed here.
- **True sub-hourly (e.g. 15-minute) granularity** — the task specifies
  hourly; a finer grain would need its own floor/retention recalibration
  and isn't evaluated.
- **Any change to `crates/aggregator/src/dedup.rs`** — Decision 2's whole
  point is that none is needed.
- **Backfilling hourly history retroactively** — same posture as the
  original daily-stats spec's equivalent item; the new table starts
  accumulating from zero on deployment day, same as `line_status_daily_stats`
  did.
- **DLR/TfL sample-stats pipeline parity** — the original spec's
  Correction 3/Explicitly-out-of-scope carve-out (national-rail-only scope)
  applies identically here; DLR's separate `sample_stats` source
  (`crates/poller-tfl`) is not touched by this design.
- **A caching layer for the new hourly endpoint** — investigated (Current
  relevant state's Volume paragraph) and judged unnecessary at this
  catalogue's scale; the existing daily route already serves this exact
  page's embed today with no caching and no reported issue, at a
  comparable per-request row count.
- **Changing `AutoRefresh`'s cadence** or building a bespoke live-updating
  chart animation for the filling current hour — Decision 7 explicitly
  relies on the existing 30s cadence doing this for free.
- **Retroactively fixing `SPARSE_DATA_FLOOR_CYCLES`'s own daily-case
  value** — unrelated to this spec; Decision 8 only concerns the new,
  separate hourly threshold.

## Open questions / risks

1. **The hourly sparse-data floor's exact numeric value (Decision 8) is an
   unvalidated placeholder**, same posture as `SPARSE_DATA_FLOOR_CYCLES`
   itself was in the original spec — should be checked against real
   `sample_cycles`-per-hour distributions once this has run in production,
   not shipped as a permanent guess.
2. **The 48-hour hourly retention default (Decision 5) is a reasoned
   safety margin, not an empirically validated number** — no traffic data
   exists yet to confirm 48h comfortably covers every real-world delay
   between an aggregator restart/deploy and the next successful write.
3. **No validation floor is proposed on the new `hourly_stats_retention_hours`
   knob** (consistent with this repo's existing lack of one on
   `history_retention_days`/`daily_stats_retention_days`) — an operator
   misconfiguring it below ~25h would silently truncate the rolling
   window's leading edge with no warning banner (unlike the Timeline tab's
   existing `retentionShortfallDays` mechanism, `frontend/lib/history.ts:190-220`,
   which has no hourly-view analog proposed here). Flagged, not designed
   away.
4. **The exact rename shape for `ChartPoint`/`gapSpans`/`TrendsCharts`'s
   generalization (Decision 9) is left to implementation** — this spec
   establishes *that* a generic bucket-key concept is the right shape, not
   the final field/prop names, consistent with "design, not code."
5. **Whether the line-info-page embed should keep both charts (rate +
   average-delay) at hourly grain, or drop to just the rate chart to save
   vertical space**, given it now shares the page with `IssueList`,
   `RepresentativeInfo`, and (for lines with a TfL counterpart) a second
   `IssueList` — Decision 11 leans toward keeping both for metric parity
   with the full history page, but this is a real density/UI-taste call
   this spec deliberately doesn't fix a final answer to, matching
   `2026-09-02-line-history-chart-fixes-design.md`'s own established
   posture of leaving exact sizing to an implementation-time screenshot
   pass.
6. **DST-transition correctness for the UTC-bucket approach (Decision 4)
   was reasoned about, not tested against a live database across a real
   BST/GMT transition** — the reasoning (a fixed-offset truncation has no
   23/25-hour-day concept to get wrong) is expected to hold, but hasn't
   been observed in production.
7. **Whether `record_hourly_stats` should be a second, independent SQL
   statement at the same `run_cycle` call site (this spec's recommendation)
   or merged into one combined multi-table write helper** — a small
   implementation-time style choice, not resolved here; either shape
   preserves Decision 2's core invariant (one `deduped` value, two writes)
   equally well.
