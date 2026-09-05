# Design: Configurable Trend Granularity on the History Page's Trends Tab

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md` ("the
granularity design") and `docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md`
("the coverage-trends design") — the two direct precedents this document
extends. No implementation plan or code is included here; every sketch
(schema, route shapes, component tree) is marked as a sketch, not final
code.

## Goal

The `/lines/[id]/history` page's Trends tab (`TrendsResults`/`TrendsCharts`)
always renders one point per **calendar day**, regardless of how wide a
date range `HistoryRangePicker` resolves. Make this user-configurable: let
a viewer pick a finer granularity (a few points per day) for a closer look,
or fall back to a coarser, averaged daily view — without silently rendering
hundreds of points or a chart backed by data that was already pruned.

## Corrections to the brief's framing

1. **This is not greenfield.** The granularity design already built and
   shipped a second, finer granularity — no longer hourly as originally
   proposed, but **half-hourly** (`line_status_half_hourly_stats`,
   `HalfHourlyTrendsResults.tsx`, confirmed by reading the current worktree,
   not the original spec's hourly sketch — see the rename migration
   `crates/api/migrations/20260902170000_line_status_hourly_stats_to_half_hourly.sql`).
   `TrendsCharts.tsx`/`chartPoint.ts` are already generalized to a
   granularity-agnostic `{ bucketKey, startKey, endKey }` shape specifically
   so a second granularity could reuse them (granularity design Decision 9,
   already merged) — this is confirmed, not assumed, by reading
   `frontend/app/lines/[id]/history/TrendsCharts.tsx:77-96` and
   `frontend/app/lines/[id]/history/chartPoint.ts:20-27` directly: `granularity:
   'day' | 'halfHour'` already exists as a prop, and it controls **only**
   the x-axis tick formatter, nothing else.
2. **The half-hourly granularity is not on this tab today, and is not
   user-range-selectable anywhere.** It is rendered only on the line-info
   page (`frontend/app/lines/[id]/page.tsx`, "Recent trends (last 24
   hours)"), over a **fixed rolling 24-hour window**
   (`resolveHalfHourlyRange`, `frontend/lib/history.ts:231-236`) that is
   deliberately not a `RangePreset` and has no picker — Decision 11 of the
   granularity design states this in so many words: "the task frames this
   view as a fixed rolling 24 hours... `HistoryRangePicker`'s whole reason
   to exist... doesn't apply to a window that's never user-selectable."
   That reasoning was correct **for that view**, but it does not settle
   *this* task's question, which is materially different: this task asks
   for a granularity choice on a tab that already has a user-driven range
   picker, not a fixed-window widget. Re-litigating "should there be a
   granularity toggle" is unavoidable here — the prior "no" was scoped to a
   single-purpose fixed view, not to a range-driven tab.
3. **The half-hourly read route already accepts an arbitrary range.**
   `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}`
   (`crates/api/src/routes/line_status.rs:55-58`) and
   `half_hourly_stats_for_range` (`crates/api/src/data/queries.rs:1116-1149`)
   take plain `DateTime<Utc>` bounds and run an unfiltered
   `WHERE half_hour_start BETWEEN $2 AND $3` — nothing in the route or query
   restricts it to "the last 24 hours." The only thing tying it to a fixed
   24h window today is the frontend: `resolveHalfHourlyRange` is
   `HalfHourlyTrendsResults`'s only caller
   (`frontend/lib/history.ts:229` doc comment: "this function's only
   caller"). **Reusing this route/query against the user's actual selected
   range is not new backend work — it is calling an existing endpoint with
   different arguments.** This is the single most consequential finding in
   this document, since it turns "half-hourly on the History tab" from "new
   backend feature" into "new frontend call site."
4. **Rates are derived at read time from raw additive sums, never stored
   pre-divided, in both existing tables.** `daily_stats_for_range`
   (`crates/api/src/data/queries.rs:1059-1092`) and
   `half_hourly_stats_for_range` (same file, :1116-1149) both return raw
   `total`/`delayed`/`cancelled`/`skipped`/`running_count`/
   `delay_minutes_sum` columns; `avgDelayMinutes` etc. are computed from
   these sums by the route handler, not stored. Because every one of these
   columns is a **sum**, summing several consecutive rows and re-deriving
   the rate is mathematically identical to what a purpose-built coarser
   accumulate-upsert table would produce — a genuinely new intermediate
   granularity (e.g. hourly, 6-hourly) does **not** need a new table or a
   new aggregator write path; it can be produced by grouping the *existing*
   `line_status_half_hourly_stats` rows at read time. See Decision 2.
5. **The half-hourly table is fed by the same LDBWS-sourced, licence-bound
   data as the daily table — a real compliance constraint this document
   surfaces that the current code's own comments do not mention.**
   `crates/aggregator/src/main.rs:285-289` feeds `record_daily_stats` and
   `record_half_hourly_stats` the **identical** `deduped: Option<&common::SampleStats>`
   value in the same cycle, with the comment there explicitly noting "a
   day's 48 half-hourly rows must sum back to that day's daily row." Since
   `SampleStats` is LDBWS-derived (the daily migration's own comment,
   `crates/api/migrations/20260831090001_line_status_daily_stats.sql:4-9`),
   and `daily_stats_retention_days`'s 300-day default exists specifically
   because "RDM's Live Departure Board licence (Schedule 1 §9) requires
   deleting all data received within 1 year"
   (`crates/aggregator/src/config.rs:56-67`), the same licence obligation
   logically extends to `line_status_half_hourly_stats` — it holds a second
   copy of the same licensed data, not independently-sourced data.
   **`half_hourly_stats_retention_hours`'s own doc comment
   (`crates/aggregator/src/config.rs:71-93`) never mentions this** — it
   reasons only about "a rolling 24h view" not needing more than 48 hours,
   because nobody had yet proposed keeping this table's data for weeks.
   Any retention bump this document proposes must stay strictly under 365
   days for the same reason `daily_stats_retention_days` was capped at 300,
   not 365. Flagged as a finding to confirm with whoever owns the LDBWS
   licence compliance tracking (`docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`),
   not a settled legal reading — see Open questions.
6. **The widest selectable range today is 30 days**, calendar-day
   granularity only. `RangePreset = '7d' | '30d'`
   (`frontend/lib/history.ts:178`, `PRESET_DAYS`, line 189);
   `HistoryRangePicker.tsx`'s `DatePickerInput type="range"` only deals in
   whole calendar days (`toCalendarDay`, lines 9-11); a custom range can, in
   principle, be typed wider than 30 days via the two-date picker, since
   nothing in `resolveRange` (`frontend/lib/history.ts:198-214`) caps a
   custom `from`/`to` pair's width — worth noting since it means "how wide
   can the user actually ask for" is not hard-capped at 30 days even today.
7. **No retention ceiling for either stats table is surfaced to the
   frontend today.** `/public/history-retention`
   (`crates/api/src/routes/history_retention.rs:30-40`) reports only
   `history_retention_days` — the knob governing the **Timeline tab's**
   `line_status_history` table (default 7 days,
   `crates/aggregator/src/config.rs:52-54`). It says nothing about
   `daily_stats_retention_days` (default 300,
   `crates/aggregator/src/config.rs:56-69`) or
   `half_hourly_stats_retention_hours` (default 48,
   `crates/aggregator/src/config.rs:71-95`) — the two knobs that actually
   bound what the Trends tab can ever show. `retentionShortfallDays`
   (`frontend/lib/history.ts:257-268`) is wired only to the Timeline tab's
   banner (`history/page.tsx:110-117`); the Trends tab currently has **no**
   analogous "some of what you asked for was already pruned" honesty
   mechanism at all, for either existing granularity. This is a real,
   pre-existing gap this feature makes materially worse if it goes
   unaddressed (a user could select a wide range plus a fine granularity
   and get a chart that's mostly gaps for a purely retention-driven reason,
   indistinguishable from "the line was quiet").

## Current relevant state (verified against the live worktree)

- **Two backend tables exist and are fully built**, both written every
  cycle by the aggregator (`crates/aggregator/src/main.rs:285-289`), keyed
  differently: `line_status_daily_stats` (`day DATE`, Europe/London
  calendar day, `crates/aggregator/src/queries.rs:505-509`'s
  `london_calendar_day`) and `line_status_half_hourly_stats`
  (`half_hour_start TIMESTAMPTZ`, a plain UTC 30-minute truncation,
  `crates/aggregator/src/queries.rs:511-540`'s `utc_half_hour_start`, chosen
  specifically because that table "only ever backs a rolling 24-hour
  window" and has no calendar-day identity worth preserving through a DST
  transition — a rationale this document's proposed reuse must re-examine,
  see Decision 3).
- **Both tables' schemas are otherwise identical**: `sample_cycles`,
  `total`, `delayed`, `cancelled`, `skipped`, `running_count`,
  `delay_minutes_sum`, accumulated via `ON CONFLICT ... DO UPDATE SET ... =
  ... + EXCLUDED....` (`record_daily_stats`,
  `crates/aggregator/src/queries.rs:572-620`; `record_half_hourly_stats`,
  same file :657-703-ish, same shape).
- **Poll cadence**: `poll_interval_secs` defaults to 60
  (`crates/aggregator/src/config.rs:47-50`), so a day's ceiling is ~1,440
  cycles and a half-hour's is ~30.
- **Sparse-data floors are placeholders, calibrated by a documented ratio,
  not first-principles data**: `SPARSE_DATA_FLOOR_CYCLES = 20` (daily, ~1.4%
  of 1,440, `TrendsResults.tsx:13`) and
  `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY = 10` (half-hourly, ~33% of ~30,
  `HalfHourlyTrendsResults.tsx:7-22`, whose own comment documents the
  halving-preserves-the-ratio derivation rule this document reuses in
  Decision 5).
- **`TrendsCharts`/`ChartPoint`/`gapSpans`/`referenceAreaBounds` are already
  bucket-key-agnostic** (Correction 1) — the shared rendering leaf needs no
  structural change for a new bucket size, only (at most) a wider prop
  union.
- **Two Server Components exist, each hard-wired to one fetch, one floor,
  one copy string, on two different pages, over two different fixed-shape
  ranges**: `TrendsResults.tsx` (daily, history tab, user range) and
  `HalfHourlyTrendsResults.tsx` (half-hourly, line-info page, fixed 24h).
  Decision 10 of the granularity design chose "separate components, not one
  branching on a prop" because, at the time, the two views' fetch/floor/copy
  were "granularity-specific content, not shared plumbing" **and** the two
  views lived on different pages with different, non-interchangeable
  ranges. Decision 4 below revisits this specifically because this task
  removes the second half of that justification — see Decision 4.

## Decisions

### 1. Granularity tiers: 30-minute, 1-hour, 6-hour, daily — four, not an arbitrary or larger fixed set

**Chosen: expose exactly four tiers** — the already-shipped extremes
(30-minute, daily) plus two new intermediate tiers (1-hour, 6-hour) —
rather than a free-text bucket-size input, or a larger ladder (e.g. adding
3-hourly/12-hourly too).

Why these four, grounded in what's cheap (Decision 2) and what's useful:
- **30-minute and daily are free** — both tables and both read routes
  already exist, tested, and shipped. Dropping either would waste working,
  tested infrastructure for no reason.
- **1-hour** is the smallest new tier that meaningfully answers the
  brief's own example ("a few data points per day instead of just one") for
  a *wide* range (24 points/day) without being identical to what already
  exists.
- **6-hour** (4 points/day) is the coarser-than-half-hourly,
  finer-than-daily "averaged overview" tier the brief explicitly asked for
  — useful specifically for a wide range where even 1-hour would be too
  dense to render legibly or too far past the retention ceiling (Decision
  3) to be honest.
- **Not 3-hourly/12-hourly/etc.**: each additional tier is a full unit of
  new ceremony under Decision 2's design (a new route, a new sparse-floor
  constant derived by the same ratio rule, a new bit of UI copy, a new
  fixture set in `TrendsCharts.test.tsx`) for a marginal perceptual gain
  between adjacent tiers already only 2-6x apart. Four tiers is a "small
  fixed set," matching the brief's own suggested example almost exactly
  (hourly/6-hourly/daily) while not orphaning the half-hourly tier that
  already shipped with real test coverage.
- **Not a free-text/arbitrary bucket-minutes input**: would require
  validating arbitrary user input against SQL interval construction
  (Decision 2's `date_bin` approach) for no real UX benefit over four named
  choices, and produces a combinatorial sparse-floor-calibration problem
  (every possible bucket size needs its own floor) instead of four.

### 2. Backend: reuse both existing routes verbatim; add ONE new parameterized query for the two new intermediate tiers — no new table, no new aggregator write path

**Chosen**: the 30-minute and daily tiers are served by the **existing**
`GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` and
`GET /Line/{id}/Stats/{from}/to/{to}` routes, completely unchanged. The two
new intermediate tiers are served by **one new query function**,
```rust
// Sketch, not final.
pub async fn sub_daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_minutes: i64, // caller-validated: 60 or 360 only, never raw user input
) -> Result<Vec<HalfHourlyStatsRow>> // reuses the existing row shape verbatim
```
implemented via a `GROUP BY date_bin($4::interval, half_hour_start, <epoch anchor>)`
(or equivalent) over `line_status_half_hourly_stats`, summing the same six
raw columns `half_hourly_stats_for_range` already selects — reusing
Correction 4's finding that these sums compose losslessly. `bucket_minutes`
is never taken directly from a request path/query parameter as a raw SQL
fragment or interval string; it is selected server-side by which of two new
thin routes was hit:
```
GET /Line/{id}/Stats/Hourly/{from}/to/{to}      -> sub_daily_stats_for_range(..., 60)
GET /Line/{id}/Stats/SixHourly/{from}/to/{to}   -> sub_daily_stats_for_range(..., 360)
```
mirroring the existing `/Stats/HalfHourly/...` naming precedent
(`crates/api/src/routes/line_status.rs:55-58`) rather than a numeric path
segment — consistent with this crate's existing preference for named, not
numeric, granularity segments.

**Rejected: a third and fourth accumulate-upsert table
(`line_status_hourly_stats`, `line_status_six_hourly_stats`), written by
two more `record_*` calls at the same `run_cycle` site.** This was the
granularity design's own Decision 1/3 approach for the half-hourly tier
when it was first built — reasonable *then*, because at the time no finer
table existed at all to derive a coarser one from. That's no longer true:
the half-hourly table now holds real information; deriving 1-hour/6-hour
figures from it at *read* time is strictly less work (one new query,
grouping already-summed columns) than duplicating the accumulate-upsert
write path, the dedup-reuse invariant test, the pruning job, and the
retention knob two more times for tables whose numbers are, by
Correction 4, mathematically recoverable from data already stored.

**Concretely, this means**: **no schema migration** for these two new
tiers, **no aggregator change**, **no new dedup reasoning** (Decision 2 of
the granularity design already settled that once, for the half-hourly
table itself — nothing here reopens it), and exactly one new function plus
two new thin routes in `crates/api`.

### 3. Retention: bump `half_hourly_stats_retention_hours` from 48 to 840 (35 days) — covering the existing widest preset with margin, staying comfortably under the LDBWS 365-day ceiling

**Chosen**: raise the default from 48 hours to **840 hours (35 days)** —
enough to cover the existing 30-day `RangePreset` in full, with a 5-day
buffer, while staying well under the 365-day licence ceiling Correction 5
identifies as applying to this table too (mirroring exactly why
`daily_stats_retention_days` landed at 300, not 365 — real margin under a
hard compliance ceiling, not a number picked to just barely clear it).
Storage at this retention: ~105 lines × 48 rows/day × 35 days ≈ **176,400
rows** — the same order of magnitude this repo's own specs have repeatedly
called "trivial" for Postgres (the daily table's own cross-catalogue
estimate is "~38k rows/year," per
`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md`'s Open
question 1).

**This bump is purely additive for the existing fixed-24h line-info-page
widget** (`HalfHourlyTrendsResults`/`resolveHalfHourlyRange`) — it always
requests only the most recent 24 hours regardless of how much history the
table now holds, so its behavior is unchanged.

**Alternative considered and rejected: leave retention at 48h and let the
UI simply refuse sub-daily granularity beyond 48h.** Rejected because it
would make three of this document's four tiers (30-min, hourly, 6-hourly)
usable only over the narrowest possible ranges, defeating the point of
adding them — the whole reason a "few points per day" view is useful is
precisely for ranges wider than 2 days.

**This retention number is a judgment call, not a value validated against
real Postgres cost or `date_bin`-over-176k-rows query latency at this
catalogue's scale — see Open questions.**

### 4. Component shape: one parameterized `TrendsResults`, not four sibling components — a deliberate departure from the granularity design's Decision 10

**Chosen: `TrendsResults` grows a `granularity` parameter and internally
selects which fetch to call, which sparse-floor constant to apply, and
which honesty-copy wording to render**, rather than forking a third and
fourth near-duplicate Server Component file (`HourlyTrendsResults.tsx`,
`SixHourlyTrendsResults.tsx`) alongside the existing
`TrendsResults.tsx`/`HalfHourlyTrendsResults.tsx` pair.

This explicitly revisits, and reverses, the granularity design's own
Decision 10 ("a new component... deliberately a separate component, not a
`granularity`-branching version of it"). That reversal is deliberate, not
an oversight of the precedent — the situation Decision 10 was reasoning
about no longer holds:
- Decision 10's daily and half-hourly views lived on **two different
  pages**, each with its **own fixed-shape range** (user-selected vs.
  always-last-24h) that could never be interchanged. Forking made sense
  because there was, structurally, nothing to parameterize over — each
  component's entire reason to exist was "the other view's range doesn't
  apply here."
- This task's four tiers live on **one tab, sharing one user-selected
  range** (`HistoryRangePicker`'s output, already threaded through
  `history/page.tsx:140` to `TrendsResults`). Four sibling components would
  each need to independently receive that same `from`/`to`, independently
  wrap the same `<Suspense>`/empty-state/`<Paper>` shape, and diverge only
  in which of three fetch functions they call and which of four
  floor/copy constants they use — real, avoidable duplication of the
  shared "fetch, gap on sparse, empty-state, render" skeleton that Decision
  10's two-file world didn't have to face, because it never had more than
  two call sites sharing one range to begin with.
- The one part of "fetch, floor, copy are granularity-specific content, not
  shared plumbing" that's still true is preserved exactly the same way —
  as an internal `switch` (or lookup table) inside the one component, not
  by collapsing the *distinctness* of each tier's floor/copy, only the
  *file* they live in.

Sketch:
```ts
// frontend/app/lines/[id]/history/TrendsResults.tsx (sketch)
type TrendGranularity = 'halfHour' | 'hour' | 'sixHour' | 'day';

const SPARSE_FLOOR: Record<TrendGranularity, number> = {
  halfHour: 10, hour: 20, sixHour: 120, day: 20, // see Decision 5
};

async function fetchStats(id: string, granularity: TrendGranularity, from: string, to: string) {
  switch (granularity) {
    case 'day': return getLineDailyStats(id, londonDayKey(from), londonDayKey(to));
    case 'halfHour': return getLineHalfHourlyStats(id, from, to);
    case 'hour': return getLineHourlyStats(id, from, to);       // new
    case 'sixHour': return getLineSixHourlyStats(id, from, to); // new
  }
}
```
`HalfHourlyTrendsResults.tsx` itself is **untouched** — the line-info
page's fixed-24h widget keeps its own simpler, single-purpose component,
since Decision 10's original reasoning (a genuinely fixed, non-selectable
range with no sibling tiers to share plumbing with) still applies there
unchanged. This document only reverses Decision 10 for the *History tab's*
Trends component, where the reasoning has changed; it does not blanket-undo
Decision 10 everywhere.

`TrendsCharts`'s `granularity` prop widens from `'day' | 'halfHour'` to
include the two new values, purely for its x-axis `tickFormatter` branch
(`TrendsCharts.tsx:94-97`) — every sub-daily tier wants the identical
`formatTime`-based tick label, so this could equally be collapsed to a
two-value `'day' | 'subDaily'` semantic instead of four explicit tags; left
as an implementation-time naming call (see Open questions), since either
shape changes no rendering behavior.

### 5. Sparse-data floor per new tier: same halving/scaling-ratio rule already established, applied to the two new tiers

**Chosen**: derive each new tier's floor the same way
`SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`'s own comment already documents —
roughly a third of that bucket's maximum possible poll-cycle coverage at
the default 60s cadence:
- **Hourly**: ceiling ≈ 60 cycles/hour → floor ≈ **20**. (Notably, this is
  exactly the original hourly-era placeholder from the granularity design's
  Decision 8, before that spec's bucket was halved to 30 minutes and its
  floor halved to 10 alongside it — a nice continuity check that this
  derivation rule is self-consistent, not a coincidence worth reading into
  further.)
- **Six-hourly**: ceiling ≈ 360 cycles/6h → floor ≈ **120**.

Both are placeholders in exactly the same unvalidated-guess posture every
existing floor constant in this file tree already carries
(`SPARSE_DATA_FLOOR_CYCLES`'s own comment: "a placeholder, not a validated
number... revisit against real `sample_cycles` distributions"). This
document does not attempt to do better than that established precedent —
see Open questions.

### 6. UI placement: a control inside the Trends tab panel, not on the shared `HistoryRangePicker` — because granularity is meaningless on the Timeline tab

**Chosen**: a small `SegmentedControl` (or `Select`, implementation's
call) rendered at the top of the Trends `TabsPanel`
(`history/page.tsx:137-157`), directly above `TrendsResults`, **not** added
to `HistoryRangePicker.tsx` itself.

**Why not on `HistoryRangePicker`**: that component is rendered once, above
the `Tabs` (`history/page.tsx:76`), and is shared by **both** the Timeline
and Trends panels — it controls the one date range both tabs read. A
granularity concept doesn't apply to the Timeline tab at all (it renders
individual grouped status-change events via `groupHistoryByDay`, not
bucketed stats rows) — putting the control there would show a control with
no effect while looking at Timeline, which is exactly the kind of
"control and result can disagree/confuse" failure `HistoryRangePicker.tsx`'s
own doc comment (lines 13-17) describes fixing for the range picker itself.
Scoping the new control to the Trends panel keeps it visible only where it
does something.

**State lives in the URL, matching every other piece of range state on
this page**: a new `?granularity=` query param, resolved by a new
`resolveGranularity(params, rangeWidthDays)` helper in `frontend/lib/history.ts`,
mirroring `resolveRange`'s existing "URL is the source of truth, invalid
value falls back to a default rather than erroring" posture
(`frontend/lib/history.ts:191-197`'s own comment). Defaults to `'day'`
(the existing, always-safe behavior) when unset or invalid — a
conservative default that changes nothing for a user who has never touched
the new control, and matches the existing behavior's baseline.

**Suspense keying**: both `TabsPanel value="trends"`'s `<Suspense>`
boundaries (`history/page.tsx:139`, `:153`) are currently keyed on
`range.preset ?? \`${range.from}-${range.to}\``; this needs `granularity`
folded into that key too (e.g. `` `${granularity}-${range.preset ?? ...}` ``)
so switching granularity remounts the chart with a fresh fetch rather than
reusing a stale-keyed subtree — the same reasoning the existing comment
there already gives for why the key is preset-name-based rather than
raw timestamps.

### 7. Auto-floor for wide ranges: disable options a range can't honestly support, rather than silently rendering a mostly-empty chart

**Chosen**: the granularity control's available options are computed from
the currently-resolved range's width and the real, server-reported
retention ceilings (Decision 8 extends `/public/history-retention` for
exactly this), **not** offered unconditionally:
- A tier whose retention window (only relevant for the three sub-daily
  tiers, all backed by `line_status_half_hourly_stats`, sharing one
  ceiling per Decision 2) doesn't reach back to the range's `from` is
  disabled, not silently degraded — same "tell the user honestly, don't
  guess" posture `retentionShortfallDays`'s existing banner already
  established for the Timeline tab.
- Independently, a tier that would render more than a placeholder
  point-count ceiling (proposed starting value: **~200 points**, in the
  same "reasoned, not measured" posture as every sparse-floor constant in
  this file tree) for the current range width is also disabled — this is
  the mechanism that actually answers the brief's "does picking a very wide
  range force a coarser floor" question: yes, mechanically, via disabling
  options rather than a separate "silently downgrade" behavior, so the
  user always sees why a tier isn't offered rather than discovering their
  selection was overridden.
- If the currently-selected `?granularity=` value becomes invalid for a
  newly-resolved range (e.g. the user had `hourly` selected, then picked a
  wider custom range), `resolveGranularity` falls back to the next
  coarser *available* tier — the same fallback shape `resolveRange` already
  uses for an unparseable `range=` value, applied one level over.

### 8. Extend `/public/history-retention` with the two other tables' ceilings, so the Trends tab gets its own honest shortfall banner

**Chosen**: add `dailyStatsRetentionDays: i64` and
`halfHourlyStatsRetentionHours: i64` fields to the existing
`HistoryRetention` struct (`crates/api/src/routes/history_retention.rs:30-34`),
sourced from `app.config.daily_stats_retention_days`/
`half_hourly_stats_retention_hours` — the same static-config-echo pattern
the existing `historyRetentionDays` field already uses, not a new route.
The three sub-daily tiers (30-min/1h/6h) all read the same underlying
table, so they share the one `halfHourlyStatsRetentionHours` ceiling; the
daily tier uses `dailyStatsRetentionDays`. `resolveGranularity`
(Decision 7) and a new `sub-daily`-aware sibling of
`retentionShortfallDays` consume these to compute both the disabled-options
set and, when the *currently selected* tier's range still exceeds what's
retained (a custom range can still outrun a 35-day sub-daily retention or a
300-day daily one), a banner inside the Trends panel — mirroring the
Timeline tab's existing one (`history/page.tsx:110-117`) in wording and
placement, scoped to the Trends panel instead.

This is a small, additive backend change (two struct fields, two config
reads) — not a new endpoint, not a new query.

## Reuse assessment

- **`ChartPoint` / `gapSpans` / `referenceAreaBounds`**: reusable
  completely as-is. Nothing about their `{ bucketKey, startKey, endKey }`
  shape (chartPoint.ts:20-27, TrendsCharts.tsx:20-52) is tied to a specific
  bucket size — they already treat the bucket key as an opaque, always-
  unique string, which every new tier's `half_hour_start`/`hour_start`/
  `six_hour_start`-shaped RFC3339 instant satisfies identically to the
  existing half-hourly case.
- **`TrendsCharts`**: reusable with one small, optional widening — the
  `granularity` prop's type union grows from two values to four (or
  collapses to `'day' | 'subDaily'`, an implementation-time naming choice,
  Decision 4) purely to keep selecting the same `formatTime`-based tick
  formatter for every sub-daily tier. No new rendering logic.
- **`TrendsResults.tsx`**: structurally reused, but its body changes
  materially — from one fixed fetch/floor/copy to a `granularity`-keyed
  switch over four (Decision 4). This is the one place real, new frontend
  logic lands.
- **`HalfHourlyTrendsResults.tsx`**: untouched. Its fixed-24h, single-tier
  reasoning still holds on the line-info page.
- **Backend**: `daily_stats_for_range`/`half_hourly_stats_for_range`
  reused verbatim; one new query function (Decision 2) covers both new
  tiers via a `bucket_minutes` parameter, itself never exposed to raw user
  input.

## Non-goals

- **A free-text or arbitrary bucket-size input.** Four named tiers only
  (Decision 1).
- **Any change to the line-info page's fixed "last 24 hours" widget**
  (`HalfHourlyTrendsResults`/`resolveHalfHourlyRange`) — unaffected by the
  retention bump (Decision 3) or the new tiers; it keeps its own, simpler,
  single-purpose shape.
- **Any change to the Timeline tab, `line_status_history`, or
  `history_retention_days`.** This document only touches the Trends tab and
  the two stats tables backing it.
- **A frontend/coverage-series analog of this feature**
  (`CoverageTrendsResults.tsx`/a future `HalfHourlyCoverageTrendsResults.tsx`)
  — no full-coverage producer exists yet to populate either coverage table
  (per the coverage-trends design's own scope), so there is nothing to
  make configurable there today. A natural, structurally-parallel follow-up
  once one exists, not designed here.
- **Backfilling historical half-hourly rows** beyond whatever the bumped
  retention keeps from the day this ships forward — same posture the
  original daily-stats spec and the granularity design both already took
  for their own tables.
- **Recalibrating the two existing floor constants**
  (`SPARSE_DATA_FLOOR_CYCLES`, `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`) —
  only the two new tiers' floors are newly derived here, by the same
  existing rule.
- **Legal/compliance sign-off on the retention bump.** Correction 5/
  Decision 3 identify a real licence-lineage argument for keeping the new
  default under 365 days, but this document is not a substitute for
  confirming that reading with whoever owns LDBWS licence compliance
  tracking.

## Open questions / risks

1. **The single biggest open risk: whether `half_hourly_stats_retention_hours`
   is actually governed by the RDM Live Departure Board licence the same
   way `daily_stats_retention_days` is (Correction 5).** This document
   infers it from shared data lineage (both tables are fed the identical
   `deduped: Option<&SampleStats>` value each cycle,
   `crates/aggregator/src/main.rs:285-289`) — a reasonable, code-grounded
   inference, but not a legal reading, and the existing code's own comments
   never made this connection because nobody had proposed keeping this
   table's data for more than 48 hours before. If this inference is wrong
   (or right but needs a different number), the proposed 840-hour default
   (Decision 3) needs to change before implementation — this should be
   confirmed with whoever tracks LDBWS licence compliance
   (`docs/superpowers/plans/2026-09-01-ldbws-data-retention.md`) before
   building Decision 3, not discovered after.
2. **The 35-day retention default (Decision 3) and the ~200-point
   auto-floor ceiling (Decision 7) are both reasoned placeholders, not
   measured against real query latency, real chart legibility, or real
   `sample_cycles`-per-bucket distributions** — same unvalidated-guess
   posture this codebase has repeatedly and explicitly accepted for every
   prior floor/retention constant in this feature area, flagged rather than
   resolved here.
3. **Whether `date_bin`-based SQL grouping (Decision 2) or a plain fetch-
   then-sum-in-Rust approach is the right implementation for the two new
   intermediate tiers** is left open — SQL grouping avoids pulling
   full-resolution rows across the wire for a wide range, but wasn't
   benchmarked against this catalogue's real row counts at the proposed 35-
   day retention.
4. **The exact `TrendsCharts` granularity-prop shape** (four explicit tags
   vs. a collapsed `'day' | 'subDaily'` semantic, Decision 4/Reuse
   assessment) is left to implementation, consistent with this repo's
   "design, not code" posture on exact naming.
5. **Whether `resolveGranularity`'s "fall back to the next coarser
   available tier" behavior (Decision 7) should also fire when a *daily*
   ceiling is exceeded** (i.e., a custom range wider than 300 days) is not
   fully worked through here — the existing Timeline-tab shortfall banner
   already handles the *display* of a stale/truncated result, but this
   document doesn't specify whether the daily tier itself should ever
   become "unavailable" the way a sub-daily tier can.
6. **No count of how many lines in the current ~105-line catalogue actually
   have `sample_stations` coverage dense enough for any sub-daily tier to
   be meaningful** was taken in this pass — the granularity design's own
   original spec flagged the analogous open item for the daily case
   (`docs/superpowers/specs/2026-08-31-line-history-graphics-design.md`
   Open question 2) and it was never closed; this document inherits the
   same gap for the new tiers rather than re-investigating it.
