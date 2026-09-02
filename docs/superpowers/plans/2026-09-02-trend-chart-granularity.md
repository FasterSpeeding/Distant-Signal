# Trend Chart Granularity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the line-info-page's embedded "Recent trends" preview
(`frontend/app/lines/[id]/page.tsx:161-184`) show **hourly** data over a
**rolling 24-hour window** instead of its current fixed 7-day daily rollup,
by building the new backend infrastructure this genuinely requires (a
`line_status_hourly_stats` table fed from the aggregator's *existing*
per-cycle dedup result -- no second dedup ledger -- plus a new read route),
generalizing the shared `TrendsCharts`/`gapSpans`/`ChartPoint` chart layer
to a bucket-key abstraction so both granularities render through the one,
already-polished chart component, and adding a new, structurally-parallel
`HourlyTrendsResults` Server Component for the hourly fetch/sparse-floor/
copy. The dedicated `/lines/[id]/history` page's Trends tab is **not**
touched -- it is already daily-over-the-selected-range, confirmed correct
by the design spec, not this plan.

**Spec:** `docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md`
-- read in full before starting; this plan does not restate its research,
only carries its eleven Decisions into concrete tasks and commits to the
exact names/shapes the spec deliberately left as "implementation choice"
(Decision 9's rename shape, in particular -- see Task 9). Cross-references
below to "Decision N" refer to that document.

**Status note -- every citation below independently re-read against this
worktree's actual `main` branch (`git show main:<path>`), not trusted
blind from the design spec, which itself was written before some of what
it cites had landed:**

- The design spec's Correction 2 frames `TrendsCharts.tsx`/`TrendsResults.tsx`
  as "substantially reworked... not yet merged to `main`" and repeatedly
  says it read them "from `worktree-agent-a8ea6b81a3cd9cb13`, not `main`."
  **That branch has since merged** (`main`'s own log:
  `095da3e Merge worktree-agent-a8ea6b81a3cd9cb13: line-history Trends chart
  fixes`) -- every file this plan touches was re-read directly from `main`
  and matches what the spec describes; this is stale provenance language
  in the spec, not a functional discrepancy.
- `TrendsCharts.tsx` (152 lines on `main`, read in full): `gapSpans` at
  lines 16-29, `referenceAreaBounds` at lines 49-58, the rate `LineChart`
  at lines 91-119 (`dataKey="day"` at 94, `withLegend` at 95, per-series
  `strokeDasharray` at 98-99, `valueFormatter` at 101, `xAxisProps` at
  103), the average-delay `LineChart` at lines 125-148 (`dataKey="day"` at
  128). **Not previously called out anywhere in the spec's own citations**:
  the file's doc comment at lines 60-79 explains `TrendsCharts` is a
  `'use client'` Client Component split out of `TrendsResults` specifically
  because a plain function prop (`valueFormatter`) cannot cross the
  Server-to-Client RSC boundary from an `async` Server Component -- this
  was a real production 500, not a style choice. This matters directly for
  Task 9/12 below: the new `granularity` prop this plan adds to
  `TrendsCharts` must stay a plain, serializable string (never a function),
  and `HourlyTrendsResults` (a new Server Component) must follow the exact
  same boundary discipline `TrendsResults` already does.
- `TrendsResults.tsx` (89 lines): `SPARSE_DATA_FLOOR_CYCLES = 20` at line
  13, `toChartPoints` at lines 23-35, the empty-state `<Paper>` block at
  lines 54-58, the honesty copy at lines 76-81.
- `chartPoint.ts` (13 lines): `ChartPoint { day: string; delayRate:
  number | null; cancellationRate: number | null; skipRate: number | null;
  avgDelayMinutes: number | null; sampleCycles: number }`.
- `TrendsCharts.test.tsx` (71 lines) and `TrendsResults.test.tsx` (191
  lines): both read in full; the former's "merges a multi-day gap" test
  (lines 19-33) carries its own comment noting it deliberately does NOT
  match the `2026-09-02-line-history-chart-fixes.md` plan's own literal
  (buggy) fixture -- `endDay` is the last **null** day, not the following
  valid day. This plan's Task 9 test-fixture generalization uses this
  file's actual current behavior as ground truth, not the older plan's
  typo.
- `frontend/app/lines/[id]/page.tsx` (187 lines): confirmed byte-identical
  to the spec's own citation -- `resolveRange({}, now)` at line 104,
  `<Skeleton height={280}>` at line 181, the `<TrendsResults>` call at line
  182, the heading "Recent trends (last 7 days)" at line 163.
- `crates/aggregator/src/main.rs` (302 lines): `run_cycle` at lines 81-154;
  `today` computed at line 117, the per-line loop at lines 120-127,
  `deduped` at line 121, `record_daily_stats` call at line 125. Matches
  the spec's own citation closely (it said "117-130" for the whole
  block, this plan uses the precise sub-line numbers above).
- `crates/aggregator/src/queries.rs` (1218 lines): `london_calendar_day` at
  lines 331-333 (spec cited "331-338", which folds in the doc comment
  above it at 324-330 -- same function, imprecise span only).
  `record_daily_stats` at lines 358-396 (spec cited "358-400" -- same
  drift). `prune_daily_stats` at lines 402-408. DST-transition tests at
  lines 983-1034 (spec cited "984-1039" -- trivial drift). None of this
  changes this plan's task content, only the exact line numbers cited.
- `crates/api/src/data/queries.rs` (1219 lines): `DailyStatsRow` at lines
  742-751, `daily_stats_for_range` at lines 762-795. **The spec cited
  "683-720" -- a ~60-line drift**, the largest found in this pass; other
  content was added to this file since the spec was written. The function
  itself is unchanged in shape from the spec's description, only its
  position in the file moved.
- `crates/api/src/routes/line_status.rs` (1099 lines): `router()` at lines
  36-43, `daily_stats_to_json` at lines 313-333, `get_line_daily_stats` at
  lines 335-344 (`Path<(String, chrono::NaiveDate, chrono::NaiveDate)>`),
  `get_line_status_history` at lines 269-306 (`Path<(String,
  DateTime<Utc>, DateTime<Utc>)>` -- the precedent Decision 6 leans on for
  the new hourly route's own `DateTime<Utc>` path segments). Matches spec.
- `crates/aggregator/src/config.rs` (91 lines): `daily_stats_retention_days`
  at lines 56-69 (default `300`), `history_retention_days` at lines 52-54
  (default `7`). Matches spec.
- `charts/distant-signal/values.yaml:487,496` /
  `charts/distant-signal/templates/aggregator-deployment.yaml:79-82`:
  confirmed exact match to the spec's citation --
  `historyRetentionDays`/`dailyStatsRetentionDays` values wired to
  `HISTORY_RETENTION_DAYS`/`DAILY_STATS_RETENTION_DAYS` env vars.
- `crates/api/migrations/`: newest migration on `main` is
  `20260901150000_stanox_crs.sql` -- this plan's new migration
  (Task 1) uses a later timestamp, `20260902090000`.
- **A genuinely new finding the spec does not surface at all**: within a
  rolling 24-hour window (24 complete hours + 1 in-progress, per the
  spec's own Volume paragraph), the wall-clock **hour label** (e.g.
  "14:00") legitimately repeats once whenever the window straddles a day
  boundary -- the same clock hour appears once yesterday and once today.
  Using a plain `formatTime`-style label as the chart's x-axis *category
  identity* (the way `day` already doubles as both identity and displayed
  tick text today) would silently collide two distinct hourly buckets
  into one category on Recharts' point-scale axis. Task 9/12 below resolve
  this by keeping the category **key** as the raw, always-unique
  `hourStart` RFC3339 string and adding a small, non-function
  `granularity` prop so `TrendsCharts` itself (already client-side, no
  RSC boundary issue) applies a `formatTime`-based tick formatter only for
  the hourly case -- daily's tick rendering is unchanged.

**Architecture (new/changed files):**

```
crates/api/
  migrations/20260902090000_line_status_hourly_stats.sql   NEW -- Task 1
  src/data/queries.rs        + HourlyStatsRow, hourly_stats_for_range      Task 7
  src/routes/line_status.rs  + GET /Line/{id}/Stats/Hourly/{from}/to/{to}, Task 8
                                hourly_stats_to_json, get_line_hourly_stats

crates/aggregator/
  src/queries.rs   + utc_hour_start                                        Task 2
                   + record_hourly_stats, prune_hourly_stats               Task 3
  src/config.rs    + hourly_stats_retention_hours (default 48)             Task 4
  src/main.rs      run_cycle EXTENDED -- record_hourly_stats/              Task 5
                   prune_hourly_stats calls, reusing the SAME `deduped`
                   value already computed for record_daily_stats
  src/dedup.rs     UNCHANGED -- Decision 2's whole point

charts/distant-signal/
  values.yaml                       + aggregator.hourlyStatsRetentionHours Task 6
  templates/aggregator-deployment.yaml  + HOURLY_STATS_RETENTION_HOURS env

frontend/
  app/lines/[id]/history/chartPoint.ts       GENERALIZED -- day -> bucketKey  Task 9
  app/lines/[id]/history/TrendsCharts.tsx    GENERALIZED -- dataKey="bucketKey",
                                              + granularity: 'day' | 'hour' prop
  app/lines/[id]/history/TrendsCharts.test.tsx   generalized + hourly fixtures
  app/lines/[id]/history/TrendsResults.tsx   bucketKey rename, passes granularity="day"
  app/lines/[id]/history/TrendsResults.test.tsx updated for the rename
  lib/api.ts     + getLineHourlyStats                                      Task 10
  lib/types.ts   + LineHourlyStats
  lib/api.test.ts  + test
  lib/history.ts   + resolveHourlyRange                                    Task 11
  lib/history.test.ts + test
  app/lines/[id]/history/HourlyTrendsResults.tsx      NEW                  Task 12
  app/lines/[id]/history/HourlyTrendsResults.test.tsx NEW
  app/lines/[id]/page.tsx        CHANGED -- embed now uses                 Task 13
                                  HourlyTrendsResults/resolveHourlyRange
  app/lines/[id]/page.test.tsx   updated for the swap
  app/lines/[id]/history/page.tsx        UNCHANGED (Decision 10/Correction 6)
```

**Tech Stack:** Rust/Axum/sqlx (Postgres) for `crates/api`/`crates/aggregator`;
Next.js App Router + TypeScript + `@mantine/charts`/`recharts` for the
frontend (all already pinned, no new dependency in either ecosystem, same
posture as the chart-fixes plan). `chrono` (already a dependency in both
Rust crates) for the new UTC-hour-truncation helper.

## Global Constraints

- **`crates/aggregator/src/dedup.rs` is not touched by any task in this
  plan.** Decision 2's whole point is that the hourly write reuses the
  exact same `deduped: Option<SampleStats>` value `main.rs`'s per-line loop
  already computes once for `record_daily_stats` -- no second ledger, no
  second dedup pass, no new period type. If any task below finds itself
  needing to modify `dedup.rs`, stop and re-read Decision 2 rather than
  proceeding -- that would mean a wrong turn was taken.
- **The new hourly bucket boundary is a plain UTC hour truncation, not an
  Europe/London local hour** (Decision 4) -- deliberately different from
  `london_calendar_day`'s Europe/London calendar-day convention used by
  the *daily* table. Do not "fix" `utc_hour_start` (Task 2) to route
  through `chrono_tz::Europe::London` by analogy with `london_calendar_day`
  -- that would reintroduce the very 23/25-hour-day DST edge case Decision
  4 exists to avoid, for a rolling-window use case that has no calendar-day
  identity to preserve in the first place.
- **`utc_hour_start` is implemented via `NaiveDate`/`Timelike::hour`
  arithmetic, not `chrono::DurationRound::duration_trunc`.** `duration_trunc`
  would be the more obviously "correct-looking" one-liner, but this repo's
  `chrono` dependency was not confirmed (during this planning pass) to have
  the `round` feature enabled, and pulling in an unverified Cargo feature
  is a worse risk than a few extra lines of explicit arithmetic that only
  relies on already-used `chrono` surface (`DateTime::date_naive`,
  `NaiveDate::and_hms_opt`, `Timelike::hour`, `NaiveDateTime::and_utc`).
  Task 2 spells out the exact implementation -- do not substitute
  `duration_trunc` without first confirming the feature is actually
  enabled in `Cargo.toml`.
- **Tasks 2-5 all edit `crates/aggregator/src/queries.rs` and/or
  `main.rs`, in that dependency order** (Task 3 adds functions Task 5's
  `main.rs` edit calls; Task 5 also needs Task 4's new config field). Do
  not dispatch Tasks 2, 3, 4, 5 to parallel subagents -- run them serially,
  each with its own commit, same posture as the chart-fixes plan's Global
  Constraints on same-file tasks.
- **Tasks 7 and 8 both live in `crates/api`, in different files
  (`src/data/queries.rs` vs `src/routes/line_status.rs`) but Task 8 calls
  a function Task 7 defines** -- Task 8 depends on Task 7, not the reverse.
  Both depend on Task 1 (the migration) existing, since their `#[ignore]`
  live-DB tests need the real table to run against (even though they don't
  block a normal `cargo test` the way every other live-DB test in this
  codebase doesn't).
- **Task 9 (`TrendsCharts.tsx`/`chartPoint.ts` generalization) must not
  change the *daily* Trends page's or embed's rendered output.** Every
  existing `TrendsResults.test.tsx`/`TrendsCharts.test.tsx` assertion that
  isn't itself about the renamed field must still pass unmodified after
  Task 9 -- this is the concrete, testable form of Decision 9's "reuse the
  shared leaf, no visible daily-side regression" requirement. Do not treat
  a broken existing daily test as a "test needs updating for the rename"
  case unless the assertion is literally checking the `day`/`bucketKey`
  field name itself.
- **No new npm or Cargo dependency anywhere in this plan.** Every Rust
  function uses `chrono`/`sqlx`/`axum` surface already present in these
  crates; every frontend change uses `@mantine/charts`/`recharts` surface
  already in use by the daily chart today.
- **Numeric placeholders are carried over from the spec, not re-derived
  here**: `hourly_stats_retention_hours` defaults to `48` (Decision 5),
  the hourly sparse-data floor defaults to `20` cycles (Decision 8's own
  "more defensible starting placeholder" -- out of a ~60-cycle/hour
  ceiling at the default 60s poll interval, i.e. roughly a third of an
  hour, explicitly flagged there as unvalidated). Do not tune either
  number as part of implementing this plan; that's future recalibration
  work against real traffic, per the spec's own Open questions 1-2.

---

### Task 1: Migration -- `line_status_hourly_stats` table

**Files:**
- Create: `crates/api/migrations/20260902090000_line_status_hourly_stats.sql`

**Interfaces:** New table `line_status_hourly_stats(line_id, hour_start,
sample_cycles, total, delayed, cancelled, skipped, running_count,
delay_minutes_sum)`, `PRIMARY KEY (line_id, hour_start)`.

**Depends on:** nothing -- first task, foundational for everything else.

- [ ] **Step 1: Write the migration**

Mirror `20260831090001_line_status_daily_stats.sql` column-for-column,
substituting `hour_start TIMESTAMPTZ` for `day DATE` (Decision 3):

```sql
-- -------------------------------------------------------------------------
-- Per-line hourly rollup of SampleStats -- the hourly-granularity sibling
-- of line_status_daily_stats (20260831090001), written by the SAME
-- per-cycle deduped SampleStats crates/aggregator/src/main.rs's run_cycle
-- already computes for the daily write (crates/aggregator/src/dedup.rs's
-- dedup_new_sample_stats) -- no second dedup pass. See
-- docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md,
-- Decisions 1-3.
--
-- `hour_start` is a plain UTC hour boundary (top of the hour), NOT an
-- Europe/London local hour the way line_status_daily_stats.day is a
-- London calendar day -- deliberately different conventions, see
-- Decision 4. A viewer never sees this column directly; it is always
-- rendered through frontend/lib/dateFormat.ts's formatTime (London
-- wall-clock) before display.
--
-- Every numeric column is a running SUM across however many poll cycles
-- contributed to this line in this hour -- rates are derived at READ time
-- (crates/api/src/data/queries.rs's hourly_stats_for_range), never stored
-- pre-divided, identical convention to the daily table.
-- -------------------------------------------------------------------------

CREATE TABLE line_status_hourly_stats (
    line_id           TEXT             NOT NULL,
    hour_start        TIMESTAMPTZ      NOT NULL,  -- plain UTC hour boundary

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

- [ ] **Step 2: Run the migration locally**

Bring up the compose Postgres (`docker compose up -d postgres` or
equivalent, per this repo's README) and confirm `sqlx migrate run` (or
however this repo's `api`/`aggregator` startup runs migrations --
check `crates/api`'s existing migration-runner invocation) applies
cleanly with no error, and that the table appears via `\d
line_status_hourly_stats` in `psql`.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260902090000_line_status_hourly_stats.sql
git commit -m "Add line_status_hourly_stats table for the new hourly trend rollup"
```

---

### Task 2: `utc_hour_start` helper + tests (aggregator)

**Files:**
- Modify: `crates/aggregator/src/queries.rs`

**Interfaces:** `pub fn utc_hour_start(instant: DateTime<Utc>) -> DateTime<Utc>`

**Depends on:** nothing (pure function, no DB).

- [ ] **Step 1: Add the function**

Add directly below `london_calendar_day` (after line 333), so the two
sibling bucket-boundary functions sit next to each other:

```rust
/// The plain UTC hour `instant` falls in, truncated to the top of the
/// hour (e.g. 14:37:12Z -> 14:00:00Z). Deliberately NOT routed through
/// `chrono_tz::Europe::London` the way `london_calendar_day` is -- Decision
/// 4 of docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
/// explains why: `line_status_hourly_stats` only ever backs a rolling
/// 24-hour window, which has no calendar-day identity worth preserving
/// through a DST transition the way the daily table's London-local `day`
/// does. A plain UTC truncation has no 23/25-hour-day edge case to get
/// wrong at all. Never displayed directly to a viewer -- always rendered
/// through `frontend/lib/dateFormat.ts`'s `formatTime` (London wall-clock)
/// first.
///
/// Implemented via explicit `NaiveDate`/`Timelike` arithmetic rather than
/// `chrono::DurationRound::duration_trunc`, since that trait's `round`
/// Cargo feature was not confirmed enabled for this crate's `chrono`
/// dependency -- this avoids depending on an unverified feature flag for
/// what is otherwise a three-line truncation.
pub fn utc_hour_start(instant: DateTime<Utc>) -> DateTime<Utc> {
    use chrono::Timelike;
    instant
        .date_naive()
        .and_hms_opt(instant.hour(), 0, 0)
        .expect("hour() is always 0-23, so this can never fail")
        .and_utc()
}
```

- [ ] **Step 2: Add tests**

Below the existing `london_calendar_day_*` tests (after line 1034), add a
parallel, deliberately *shorter* test suite -- per the doc comment above,
the whole point of the UTC choice is that there is no DST case to test:

```rust
// --- utc_hour_start ---
//
// Deliberately no DST-transition cases here, unlike london_calendar_day's
// suite above -- see Decision 4 / this function's own doc comment for why
// a plain UTC truncation has nothing DST-related to get wrong.

#[test]
fn utc_hour_start_truncates_to_the_top_of_the_hour() {
    let instant: DateTime<Utc> = "2026-08-15T14:37:12Z".parse().unwrap();
    assert_eq!(utc_hour_start(instant), "2026-08-15T14:00:00Z".parse::<DateTime<Utc>>().unwrap());
}

#[test]
fn utc_hour_start_on_the_exact_hour_boundary_is_a_no_op() {
    let instant: DateTime<Utc> = "2026-08-15T14:00:00Z".parse().unwrap();
    assert_eq!(utc_hour_start(instant), instant);
}

#[test]
fn utc_hour_start_just_before_midnight_stays_on_the_same_utc_day() {
    let instant: DateTime<Utc> = "2026-08-15T23:59:59Z".parse().unwrap();
    assert_eq!(utc_hour_start(instant), "2026-08-15T23:00:00Z".parse::<DateTime<Utc>>().unwrap());
}

#[test]
fn utc_hour_start_across_the_uk_spring_forward_transition_is_unaffected() {
    // Unlike london_calendar_day_across_the_spring_forward_transition, this
    // is purely a sanity check that UTC arithmetic doesn't care that the
    // UK clock changed at all -- there is no "skipped" or "repeated" UTC
    // hour on this date, only on the London-local wall clock.
    let instant: DateTime<Utc> = "2026-03-29T01:30:00Z".parse().unwrap();
    assert_eq!(utc_hour_start(instant), "2026-03-29T01:00:00Z".parse::<DateTime<Utc>>().unwrap());
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p aggregator utc_hour_start`
Expected: PASS (these are pure, not `#[ignore]`d -- no live database
needed).

- [ ] **Step 4: Commit**

```bash
git add crates/aggregator/src/queries.rs
git commit -m "Add utc_hour_start, the hourly-rollup sibling of london_calendar_day"
```

---

### Task 3: `record_hourly_stats` + `prune_hourly_stats` + tests (aggregator)

**Files:**
- Modify: `crates/aggregator/src/queries.rs`

**Interfaces:**
- `pub async fn record_hourly_stats(pool: &PgPool, line_id: &str, hour_start: DateTime<Utc>, stats: Option<&common::SampleStats>) -> Result<()>`
- `pub async fn prune_hourly_stats(pool: &PgPool, retention_hours: i64) -> Result<u64>`

**Depends on:** Task 1 (migration must exist for the `#[ignore]` live-DB
tests to run against a real table -- they don't block `cargo test`
without `--ignored`, same as every other live-DB test in this file, but
logically depend on the table existing).

- [ ] **Step 1: Add `record_hourly_stats`**

Add directly below `record_daily_stats` (after line 396), mirroring its
shape exactly except for the key column and conflict target:

```rust
/// Hourly-granularity sibling of `record_daily_stats` -- same
/// accumulate-upsert shape, same "fed the DEDUPED per-cycle contribution,
/// not raw SampleStats" contract (see that function's own doc comment,
/// which applies here unchanged), keyed on `hour_start` (a plain UTC hour
/// boundary from `utc_hour_start`, Decision 4) instead of a London
/// calendar `day`.
///
/// Called at the exact same call site as `record_daily_stats`, fed the
/// SAME `deduped: Option<&SampleStats>` value for a given line/cycle --
/// see `main.rs`'s `run_cycle` and
/// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
/// Decision 2. This invariant (both calls see the identical value) is
/// what makes a day's 24 hourly rows sum back to that day's
/// `line_status_daily_stats` row -- see this file's
/// `hourly_and_daily_stats_reconcile_for_a_single_line_and_period` test.
pub async fn record_hourly_stats(
    pool: &PgPool,
    line_id: &str,
    hour_start: DateTime<Utc>,
    stats: Option<&common::SampleStats>,
) -> Result<()> {
    let (total, delayed, cancelled, skipped, running, delay_minutes_sum) = match stats {
        Some(s) => {
            let running = s.total.saturating_sub(s.cancelled) as i64;
            (s.total as i64, s.delayed as i64, s.cancelled as i64, s.skipped as i64, running, s.avg_delay_minutes * running as f64)
        }
        None => (0, 0, 0, 0, 0, 0.0),
    };
    sqlx::query(
        "INSERT INTO line_status_hourly_stats
            (line_id, hour_start, sample_cycles, total, delayed, cancelled, skipped,
             running_count, delay_minutes_sum)
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (line_id, hour_start) DO UPDATE SET
            sample_cycles     = line_status_hourly_stats.sample_cycles + 1,
            total             = line_status_hourly_stats.total + EXCLUDED.total,
            delayed           = line_status_hourly_stats.delayed + EXCLUDED.delayed,
            cancelled         = line_status_hourly_stats.cancelled + EXCLUDED.cancelled,
            skipped           = line_status_hourly_stats.skipped + EXCLUDED.skipped,
            running_count     = line_status_hourly_stats.running_count + EXCLUDED.running_count,
            delay_minutes_sum = line_status_hourly_stats.delay_minutes_sum + EXCLUDED.delay_minutes_sum",
    )
    .bind(line_id)
    .bind(hour_start)
    .bind(total)
    .bind(delayed)
    .bind(cancelled)
    .bind(skipped)
    .bind(running)
    .bind(delay_minutes_sum)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mirrors `prune_daily_stats`'s shape exactly, called unconditionally
/// every cycle from `run_cycle`, keyed on the new
/// `hourly_stats_retention_hours` config knob (default 48, Decision 5 --
/// NOT a reuse of either `history_retention_days` or
/// `daily_stats_retention_days`, both of which govern unrelated tables).
pub async fn prune_hourly_stats(pool: &PgPool, retention_hours: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM line_status_hourly_stats WHERE hour_start < NOW() - ($1 || ' hours')::interval",
    )
    .bind(retention_hours.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 2: Add tests**

Below the existing `record_daily_stats`/`prune_daily_stats` tests (after
line 1217), add the hourly mirror set, plus the reconciliation invariant
test:

```rust
async fn cleanup_hourly_stats(pool: &PgPool, line_id: &str) {
    sqlx::query("DELETE FROM line_status_hourly_stats WHERE line_id = $1")
        .bind(line_id)
        .execute(pool)
        .await
        .expect("cleanup line_status_hourly_stats");
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            record_hourly_stats_accumulates_deduped_contributions_within_an_hour -- --ignored` \
            against docker compose's postgres"]
async fn record_hourly_stats_accumulates_deduped_contributions_within_an_hour() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const LINE_ID: &str = "TEST-HOURLY-STATS-ACCUMULATE";
    let hour: DateTime<Utc> = "2026-08-31T14:00:00Z".parse().unwrap();

    cleanup_hourly_stats(&pool, LINE_ID).await;

    let cycle1 = common::SampleStats { total: 4, delayed: 1, cancelled: 1, skipped: 0, avg_delay_minutes: 6.0 };
    let cycle2 = common::SampleStats { total: 2, delayed: 2, cancelled: 0, skipped: 1, avg_delay_minutes: 12.0 };

    record_hourly_stats(&pool, LINE_ID, hour, Some(&cycle1)).await.expect("record cycle 1");
    record_hourly_stats(&pool, LINE_ID, hour, Some(&cycle2)).await.expect("record cycle 2");

    let row = sqlx::query(
        "SELECT sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum \
         FROM line_status_hourly_stats WHERE line_id = $1 AND hour_start = $2",
    )
    .bind(LINE_ID)
    .bind(hour)
    .fetch_one(&pool)
    .await
    .expect("read accumulated row");

    cleanup_hourly_stats(&pool, LINE_ID).await;

    let sample_cycles: i64 = row.try_get("sample_cycles").unwrap();
    let total: i64 = row.try_get("total").unwrap();
    let running_count: i64 = row.try_get("running_count").unwrap();
    let delay_minutes_sum: f64 = row.try_get("delay_minutes_sum").unwrap();

    assert_eq!(sample_cycles, 2);
    assert_eq!(total, 6);
    assert_eq!(running_count, 5);
    assert!((delay_minutes_sum - 42.0).abs() < 1e-9);
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            record_hourly_stats_a_new_hour_starts_a_fresh_row -- --ignored` against docker \
            compose's postgres"]
async fn record_hourly_stats_a_new_hour_starts_a_fresh_row() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const LINE_ID: &str = "TEST-HOURLY-STATS-NEW-HOUR";
    let hour1: DateTime<Utc> = "2026-08-31T14:00:00Z".parse().unwrap();
    let hour2: DateTime<Utc> = "2026-08-31T15:00:00Z".parse().unwrap();

    cleanup_hourly_stats(&pool, LINE_ID).await;

    let stats = common::SampleStats { total: 5, delayed: 1, cancelled: 0, skipped: 0, avg_delay_minutes: 3.0 };
    record_hourly_stats(&pool, LINE_ID, hour1, Some(&stats)).await.expect("record hour 1");
    record_hourly_stats(&pool, LINE_ID, hour2, Some(&stats)).await.expect("record hour 2");

    let hour2_cycles: i64 = sqlx::query_scalar(
        "SELECT sample_cycles FROM line_status_hourly_stats WHERE line_id = $1 AND hour_start = $2",
    )
    .bind(LINE_ID)
    .bind(hour2)
    .fetch_one(&pool)
    .await
    .expect("read hour 2 row");

    cleanup_hourly_stats(&pool, LINE_ID).await;

    assert_eq!(hour2_cycles, 1, "a new hour must start its own fresh row, not accumulate into hour 1's");
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            record_hourly_stats_an_hour_boundary_crossing_a_day_boundary_is_unaffected -- --ignored` \
            against docker compose's postgres"]
async fn record_hourly_stats_an_hour_boundary_crossing_a_day_boundary_is_unaffected() {
    // 23:00Z and the next day's 00:00Z are adjacent hours that also cross a
    // UTC calendar day -- confirms record_hourly_stats treats this exactly
    // like any other hour boundary, with no special-casing or corruption.
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const LINE_ID: &str = "TEST-HOURLY-STATS-DAY-BOUNDARY";
    let hour1: DateTime<Utc> = "2026-08-31T23:00:00Z".parse().unwrap();
    let hour2: DateTime<Utc> = "2026-09-01T00:00:00Z".parse().unwrap();

    cleanup_hourly_stats(&pool, LINE_ID).await;

    let stats = common::SampleStats { total: 3, delayed: 0, cancelled: 0, skipped: 0, avg_delay_minutes: 1.0 };
    record_hourly_stats(&pool, LINE_ID, hour1, Some(&stats)).await.expect("record hour 1");
    record_hourly_stats(&pool, LINE_ID, hour2, Some(&stats)).await.expect("record hour 2");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM line_status_hourly_stats WHERE line_id = $1")
        .bind(LINE_ID)
        .fetch_one(&pool)
        .await
        .expect("count rows");

    cleanup_hourly_stats(&pool, LINE_ID).await;

    assert_eq!(count, 2, "two adjacent hours either side of a day boundary must stay two separate rows");
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            prune_hourly_stats_deletes_only_rows_older_than_the_retention_window -- --ignored` \
            against docker compose's postgres"]
async fn prune_hourly_stats_deletes_only_rows_older_than_the_retention_window() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const OLD_LINE_ID: &str = "TEST-HOURLY-STATS-PRUNE-OLD";
    const RECENT_LINE_ID: &str = "TEST-HOURLY-STATS-PRUNE-RECENT";
    const RETENTION_HOURS: i64 = 48;

    cleanup_hourly_stats(&pool, OLD_LINE_ID).await;
    cleanup_hourly_stats(&pool, RECENT_LINE_ID).await;

    sqlx::query(
        "INSERT INTO line_status_hourly_stats (line_id, hour_start, sample_cycles, total) VALUES \
            ($1, NOW() - (($3 + 1) || ' hours')::interval, 1, 1), \
            ($2, NOW() - (($3 - 1) || ' hours')::interval, 1, 1)",
    )
    .bind(OLD_LINE_ID)
    .bind(RECENT_LINE_ID)
    .bind(RETENTION_HOURS)
    .execute(&pool)
    .await
    .expect("seed old and recent rows");

    prune_hourly_stats(&pool, RETENTION_HOURS).await.expect("prune_hourly_stats");

    let old_survives: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM line_status_hourly_stats WHERE line_id = $1")
        .bind(OLD_LINE_ID)
        .fetch_one(&pool)
        .await
        .expect("count old survivors");
    let recent_survives: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM line_status_hourly_stats WHERE line_id = $1")
        .bind(RECENT_LINE_ID)
        .fetch_one(&pool)
        .await
        .expect("count recent survivors");

    cleanup_hourly_stats(&pool, OLD_LINE_ID).await;
    cleanup_hourly_stats(&pool, RECENT_LINE_ID).await;

    assert_eq!(old_survives, 0);
    assert_eq!(recent_survives, 1);
}

/// The single most important new test in this plan (Decision 2's
/// reconciliation invariant, made concrete): feeding the SAME
/// `Some(&SampleStats)` value to both `record_daily_stats` and
/// `record_hourly_stats` -- exactly as `main.rs`'s `run_cycle` now does at
/// its one call site -- must produce a daily row and an hourly row whose
/// sums agree. This doesn't call `run_cycle` itself (that would need a
/// full aggregate() pipeline); it directly exercises the two write
/// functions with an identical input, which is the actual invariant that
/// matters and is what would regress if a future edit ever computed two
/// separate `deduped` values instead of sharing one.
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
            hourly_and_daily_stats_reconcile_for_a_single_line_and_period -- --ignored` \
            against docker compose's postgres"]
async fn hourly_and_daily_stats_reconcile_for_a_single_line_and_period() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const LINE_ID: &str = "TEST-RECONCILE-DAILY-HOURLY";
    let day = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
    let hour: DateTime<Utc> = "2026-08-31T14:00:00Z".parse().unwrap();

    cleanup_daily_stats(&pool, LINE_ID).await;
    cleanup_hourly_stats(&pool, LINE_ID).await;

    let stats = common::SampleStats { total: 7, delayed: 2, cancelled: 1, skipped: 0, avg_delay_minutes: 5.0 };

    // Same `deduped` value, same call pattern as run_cycle's one call site.
    record_daily_stats(&pool, LINE_ID, day, Some(&stats)).await.expect("record daily");
    record_hourly_stats(&pool, LINE_ID, hour, Some(&stats)).await.expect("record hourly");

    let daily = sqlx::query("SELECT total, delayed, cancelled, running_count, delay_minutes_sum \
                              FROM line_status_daily_stats WHERE line_id = $1 AND day = $2")
        .bind(LINE_ID).bind(day).fetch_one(&pool).await.expect("read daily row");
    let hourly = sqlx::query("SELECT total, delayed, cancelled, running_count, delay_minutes_sum \
                               FROM line_status_hourly_stats WHERE line_id = $1 AND hour_start = $2")
        .bind(LINE_ID).bind(hour).fetch_one(&pool).await.expect("read hourly row");

    cleanup_daily_stats(&pool, LINE_ID).await;
    cleanup_hourly_stats(&pool, LINE_ID).await;

    let total_d: i64 = daily.try_get("total").unwrap();
    let total_h: i64 = hourly.try_get("total").unwrap();
    let delayed_d: i64 = daily.try_get("delayed").unwrap();
    let delayed_h: i64 = hourly.try_get("delayed").unwrap();
    let dms_d: f64 = daily.try_get("delay_minutes_sum").unwrap();
    let dms_h: f64 = hourly.try_get("delay_minutes_sum").unwrap();

    assert_eq!(total_d, total_h, "a single hour's stats must reconcile with that hour's own contribution to the day");
    assert_eq!(delayed_d, delayed_h);
    assert!((dms_d - dms_h).abs() < 1e-9);
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p aggregator` (the new tests are `#[ignore]`d like every
other live-DB test in this file, so this confirms compilation without a
database). Then, against a live compose Postgres:
`DATABASE_URL=... cargo test -p aggregator -- --ignored record_hourly record_daily hourly_and_daily`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/aggregator/src/queries.rs
git commit -m "Add record_hourly_stats/prune_hourly_stats, the hourly siblings of the daily rollup writers"
```

---

### Task 4: `hourly_stats_retention_hours` config knob (aggregator)

**Files:**
- Modify: `crates/aggregator/src/config.rs`

**Interfaces:** `Config.hourly_stats_retention_hours: i64`, CLI
`--hourly-stats-retention-hours`, env `HOURLY_STATS_RETENTION_HOURS`,
default `48`.

**Depends on:** nothing (independent of Tasks 2/3, but Task 5 needs this
field to exist before it can be threaded through `run_cycle`).

- [ ] **Step 1: Add the field**

In `crates/aggregator/src/config.rs`, directly below
`daily_stats_retention_days` (after line 69):

```rust
    /// How long to keep `line_status_hourly_stats` rows before pruning
    /// them. Deliberately NOT a reuse of `history_retention_days` (governs
    /// a different table, `line_status_history`) or
    /// `daily_stats_retention_days` (sized for a weeks/months trend use
    /// case this hourly rolling-24h view does not have -- reusing its
    /// default of 300 would mean accumulating ~300 days x 24 rows/line of
    /// data only the most recent ~25 rows of which are ever read). 48
    /// hours is a 2x safety margin over the 24-25 rows the line-info-page
    /// embed actually needs, per
    /// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
    /// Decision 5 -- a reasoned starting default, not empirically
    /// validated against real restart/deploy timing (see that spec's Open
    /// question 2).
    #[arg(long, env, default_value_t = 48)]
    pub hourly_stats_retention_hours: i64,
```

- [ ] **Step 2: Build check**

Run: `cargo build -p aggregator`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/aggregator/src/config.rs
git commit -m "Add hourly_stats_retention_hours config knob, default 48h"
```

---

### Task 5: Wire `record_hourly_stats`/`prune_hourly_stats` into `run_cycle` (aggregator)

**Files:**
- Modify: `crates/aggregator/src/main.rs`

**Interfaces:** `run_cycle`'s signature gains one new parameter
(`hourly_stats_retention_hours: i64`); no other function signature
changes.

**Depends on:** Task 2 (`utc_hour_start`), Task 3 (`record_hourly_stats`/
`prune_hourly_stats`), Task 4 (the new config field).

- [ ] **Step 1: Thread the new config value from `main()` into `run_cycle`**

In `main()` (around line 63-71), add the new argument to the `run_cycle`
call:

```rust
        let result = run_cycle(
            &pool,
            &static_lines,
            &defaults,
            config.history_retention_days,
            config.daily_stats_retention_days,
            config.hourly_stats_retention_hours,
            &mut dedup_ledger,
        )
        .await;
```

And add the matching parameter to `run_cycle`'s signature (around lines
81-88):

```rust
async fn run_cycle(
    pool: &sqlx::PgPool,
    static_lines: &HashMap<String, LineDefinition>,
    defaults: &Defaults,
    retention_days: i64,
    daily_stats_retention_days: i64,
    hourly_stats_retention_hours: i64,
    dedup_ledger: &mut SeenServiceLedger,
) -> anyhow::Result<()> {
```

- [ ] **Step 2: Compute `hour_start` once per cycle, from the same instant as `today`**

Replace the existing single-purpose `today` line (currently line 117)
with a shared `now` binding both bucket boundaries derive from -- avoids
any (however negligible) skew between two separate `Utc::now()` calls a
few nanoseconds apart, and keeps the two boundary computations visibly
paired:

```rust
    let cycle_now = chrono::Utc::now();
    let today = queries::london_calendar_day(cycle_now);
    let hour_start = queries::utc_hour_start(cycle_now);
```

- [ ] **Step 3: Add the `record_hourly_stats` call alongside `record_daily_stats`**

In the per-line loop (currently lines 120-127), add the new call and a
matching counter:

```rust
    let mut new_services_this_cycle: u64 = 0;
    let mut daily_stats_recorded = 0u64;
    let mut hourly_stats_recorded = 0u64;
    for (line_id, line) in lines_with_sample_coverage(&reports, &lines) {
        let deduped = dedup::dedup_new_sample_stats(dedup_ledger, line_id, today, line, &samples, defaults);
        if let Some(ref stats) = deduped {
            new_services_this_cycle += stats.total as u64;
        }
        // Both calls below are fed the SAME `deduped` value -- this is
        // Decision 2's whole point (see that function's own doc comment
        // and the hourly_and_daily_stats_reconcile_for_a_single_line_and_period
        // test in queries.rs): a day's 24 hourly rows must sum back to
        // that day's daily row, which only holds if both writes see an
        // identical per-cycle contribution, not two independently
        // computed ones.
        queries::record_daily_stats(pool, line_id, today, deduped.as_ref()).await?;
        queries::record_hourly_stats(pool, line_id, hour_start, deduped.as_ref()).await?;
        daily_stats_recorded += 1;
        hourly_stats_recorded += 1;
    }
    dedup_ledger.prune_before(today);

    let daily_stats_pruned = queries::prune_daily_stats(pool, daily_stats_retention_days).await?;
    let hourly_stats_pruned = queries::prune_hourly_stats(pool, hourly_stats_retention_hours).await?;
```

- [ ] **Step 4: Add metrics + tracing fields**

Alongside the existing `aggregator_daily_stats_recorded_total`/
`aggregator_daily_stats_pruned_total` metrics (currently lines 137-140),
add:

```rust
    metrics::counter!(common::metrics::metric_name("aggregator_hourly_stats_recorded_total"))
        .increment(hourly_stats_recorded);
    metrics::counter!(common::metrics::metric_name("aggregator_hourly_stats_pruned_total"))
        .increment(hourly_stats_pruned);
```

And in the `tracing::info!` call (currently lines 142-151), add
`hourly_stats_recorded = hourly_stats_recorded, hourly_stats_pruned =
hourly_stats_pruned,` alongside the existing daily fields.

- [ ] **Step 5: Build check + existing test suite**

Run: `cargo build -p aggregator && cargo test -p aggregator`
Expected: PASS -- confirms the signature change compiles and the existing
`lines_with_sample_coverage` unit tests (unaffected by this task) still
pass.

- [ ] **Step 6: Commit**

```bash
git add crates/aggregator/src/main.rs
git commit -m "Wire record_hourly_stats/prune_hourly_stats into run_cycle, reusing the daily write's deduped value"
```

---

### Task 6: Helm chart -- `hourlyStatsRetentionHours` value + env wiring

**Files:**
- Modify: `charts/distant-signal/values.yaml`
- Modify: `charts/distant-signal/templates/aggregator-deployment.yaml`

**Interfaces:** `aggregator.hourlyStatsRetentionHours` (default `48`),
`HOURLY_STATS_RETENTION_HOURS` env var.

**Depends on:** Task 4 (needs the exact CLI/env flag name
`hourly_stats_retention_hours`/`HOURLY_STATS_RETENTION_HOURS` clap
derives).

- [ ] **Step 1: Add the value**

In `charts/distant-signal/values.yaml`, directly below
`dailyStatsRetentionDays` (after line 496):

```yaml
  # Retention for the new hourly rollup table (line_status_hourly_stats) --
  # a much shorter window than either historyRetentionDays or
  # dailyStatsRetentionDays, since only the line-info-page's rolling
  # 24-hour embed ever reads it. See
  # docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
  # Decision 5.
  hourlyStatsRetentionHours: 48
```

- [ ] **Step 2: Wire the env var**

In `charts/distant-signal/templates/aggregator-deployment.yaml`, directly
below the `DAILY_STATS_RETENTION_DAYS` entry (after line 82):

```yaml
            - name: HOURLY_STATS_RETENTION_HOURS
              value: {{ .Values.aggregator.hourlyStatsRetentionHours | quote }}
```

- [ ] **Step 3: Lint check**

Run: `helm lint charts/distant-signal` (or `helm template
charts/distant-signal` and visually confirm the new env var renders).
Expected: PASS, new env var present in the rendered manifest.

- [ ] **Step 4: Commit**

```bash
git add charts/distant-signal/values.yaml charts/distant-signal/templates/aggregator-deployment.yaml
git commit -m "Wire hourlyStatsRetentionHours through the Helm chart"
```

---

### Task 7: `hourly_stats_for_range` query (api crate)

**Files:**
- Modify: `crates/api/src/data/queries.rs`

**Interfaces:**
- `pub struct HourlyStatsRow { pub hour_start: DateTime<Utc>, pub sample_cycles: i64, pub total: i64, pub delayed: i64, pub cancelled: i64, pub skipped: i64, pub running_count: i64, pub delay_minutes_sum: f64 }`
- `pub async fn hourly_stats_for_range(pool: &PgPool, line_id: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<HourlyStatsRow>>`

**Depends on:** Task 1 (the table must exist for the `#[ignore]` live-DB
test).

- [ ] **Step 1: Add the struct + query**

Directly below `daily_stats_for_range` (after line 795):

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

/// Hourly-granularity sibling of `daily_stats_for_range` -- same shape,
/// same "empty vec for an unknown line_id, no error" behavior, same
/// read-time rate derivation posture (never stored pre-divided). `from`/
/// `to` are real instants (`DateTime<Utc>`), not calendar dates -- an hour
/// bucket has no calendar-day analog to round-trip through, unlike the
/// daily route (Decision 6 of
/// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md).
pub async fn hourly_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_hourly_stats
         WHERE line_id = $1 AND hour_start BETWEEN $2 AND $3
         ORDER BY hour_start",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HourlyStatsRow {
                hour_start: row.try_get("hour_start")?,
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

- [ ] **Step 2: Add a test**

Mirroring `daily_stats_for_range`'s own existing (`#[ignore]`d) live-DB
test (`daily_stats_for_range_filters_orders_and_handles_unknown_lines`,
around line 1182 in this file):

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            hourly_stats_for_range_filters_orders_and_handles_unknown_lines -- --ignored` \
            against docker compose's postgres"]
async fn hourly_stats_for_range_filters_orders_and_handles_unknown_lines() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = sqlx::postgres::PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
    const LINE_ID: &str = "TEST-HOURLY-RANGE";

    sqlx::query("DELETE FROM line_status_hourly_stats WHERE line_id = $1").bind(LINE_ID).execute(&pool).await.unwrap();

    let h1: chrono::DateTime<chrono::Utc> = "2026-08-31T12:00:00Z".parse().unwrap();
    let h2: chrono::DateTime<chrono::Utc> = "2026-08-31T14:00:00Z".parse().unwrap();
    let out_of_range: chrono::DateTime<chrono::Utc> = "2026-08-28T00:00:00Z".parse().unwrap();

    sqlx::query(
        "INSERT INTO line_status_hourly_stats (line_id, hour_start, sample_cycles, total) VALUES \
            ($1, $2, 1, 5), ($1, $3, 1, 3), ($1, $4, 1, 99)",
    )
    .bind(LINE_ID).bind(h2).bind(h1).bind(out_of_range) // inserted out of order on purpose
    .execute(&pool).await.expect("seed rows");

    let rows = hourly_stats_for_range(
        &pool, LINE_ID,
        "2026-08-31T00:00:00Z".parse().unwrap(),
        "2026-09-01T00:00:00Z".parse().unwrap(),
    ).await.expect("hourly_stats_for_range");

    sqlx::query("DELETE FROM line_status_hourly_stats WHERE line_id = $1").bind(LINE_ID).execute(&pool).await.unwrap();

    assert_eq!(rows.len(), 2, "the out-of-range row must be excluded");
    assert_eq!(rows[0].hour_start, h1, "results must be ordered ascending by hour_start");
    assert_eq!(rows[1].hour_start, h2);

    let unknown = hourly_stats_for_range(
        &pool, "TEST-HOURLY-RANGE-UNKNOWN",
        "2026-08-31T00:00:00Z".parse().unwrap(),
        "2026-09-01T00:00:00Z".parse().unwrap(),
    ).await.expect("hourly_stats_for_range for an unknown line_id");
    assert!(unknown.is_empty());
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p api` (compiles; the new test is `#[ignore]`d).
Against a live compose Postgres:
`DATABASE_URL=... cargo test -p api -- --ignored hourly_stats_for_range`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add hourly_stats_for_range, the hourly sibling of daily_stats_for_range"
```

---

### Task 8: `GET /Line/{id}/Stats/Hourly/{from}/to/{to}` route (api crate)

**Files:**
- Modify: `crates/api/src/routes/line_status.rs`

**Interfaces:** New route `GET /Line/{id}/Stats/Hourly/{from}/to/{to}`,
handler `get_line_hourly_stats`, JSON mapper `hourly_stats_to_json`.

**Depends on:** Task 7 (`hourly_stats_for_range`/`HourlyStatsRow`).

- [ ] **Step 1: Register the route**

In `router()` (currently lines 36-43), add the new route below the
existing daily one:

```rust
pub fn router() -> Router {
    AxumRouter::new()
        .route("/Line/Mode/{mode}/Status", axum::routing::get(get_mode_status))
        .route("/Line/{ids}/Status", axum::routing::get(get_line_status))
        .route("/StopPoint/{crs}/Disruption", axum::routing::get(get_stop_point_disruption))
        .route("/Line/{id}/Status/{from}/to/{to}", axum::routing::get(get_line_status_history))
        .route("/Line/{id}/Stats/{from}/to/{to}", axum::routing::get(get_line_daily_stats))
        .route("/Line/{id}/Stats/Hourly/{from}/to/{to}", axum::routing::get(get_line_hourly_stats))
}
```

Note in this file's module doc comment (which already flags the
multi-segment `Path<(String, DateTime<Utc>, DateTime<Utc>)>` risk for the
history route) that this new route introduces a genuinely new routing
question the existing comment doesn't cover: whether axum's router
(matchit) correctly disambiguates the literal segment `Hourly` from the
sibling route's dynamic `{from}` segment at the same position (`/Line/
{id}/Stats/Hourly/...` vs. `/Line/{id}/Stats/{from}/...`). Step 3 below
adds a live-router test for exactly this, rather than assuming it from
matchit's documented static-beats-dynamic priority.

- [ ] **Step 2: Add `hourly_stats_to_json` + the handler**

Directly below `get_line_daily_stats` (after line 344):

```rust
/// Hourly sibling of `daily_stats_to_json` -- identical rate-derivation
/// logic, `hourStart` (an ISO instant) in place of `day`.
fn hourly_stats_to_json(row: queries::HourlyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| if row.total > 0 { numerator as f64 / row.total as f64 } else { 0.0 };

    serde_json::json!({
        "hourStart": row.hour_start,
        "sampleCycles": row.sample_cycles,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::hourly_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows.into_iter().map(hourly_stats_to_json).collect()))
}
```

- [ ] **Step 3: Add unit tests for `hourly_stats_to_json` + a route-mounting/disambiguation probe**

In the existing `#[cfg(test)] mod tests` block (after the
`daily_stats_to_json_*` tests, around line 509), add:

```rust
fn hourly_stats_row(
    total: i64, delayed: i64, cancelled: i64, skipped: i64, running_count: i64, delay_minutes_sum: f64,
) -> queries::HourlyStatsRow {
    queries::HourlyStatsRow {
        hour_start: "2026-08-15T14:00:00Z".parse().unwrap(),
        sample_cycles: 12,
        total, delayed, cancelled, skipped, running_count, delay_minutes_sum,
    }
}

#[test]
fn hourly_stats_to_json_computes_rates_for_a_normal_row() {
    let row = hourly_stats_row(100, 10, 5, 2, 95, 190.0);
    let json = hourly_stats_to_json(row);

    assert_eq!(json["hourStart"], serde_json::json!("2026-08-15T14:00:00Z"));
    assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0));
    assert_eq!(json["delayRate"], serde_json::json!(0.1));
}

#[test]
fn hourly_stats_to_json_zero_total_never_produces_nan_or_infinity() {
    let row = hourly_stats_row(0, 0, 0, 0, 0, 0.0);
    let json = hourly_stats_to_json(row);
    for field in ["avgDelayMinutes", "delayRate", "cancellationRate", "skipRate"] {
        let value = json[field].as_f64().unwrap();
        assert!(value.is_finite());
        assert_eq!(value, 0.0);
    }
}

#[tokio::test]
async fn both_stats_routes_coexist_and_route_to_the_correct_handler() {
    // The real risk this test exists for: `/Line/{id}/Stats/{from}/to/{to}`
    // (NaiveDate) and `/Line/{id}/Stats/Hourly/{from}/to/{to}` (DateTime<Utc>)
    // share a path prefix with a dynamic segment at the exact position the
    // new route's literal "Hourly" segment occupies. Two throwaway probe
    // handlers, mounted the same way the daily route's own precedent probe
    // (`get_line_daily_stats_route_mounts_and_parses_naive_date_path_segments`,
    // above) does, confirm axum's router sends each URL to the right one
    // rather than assuming it from matchit's documented priority rules.
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn daily_probe(Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>) -> String {
        format!("daily:{id}|{from}|{to}")
    }
    async fn hourly_probe(Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>) -> String {
        format!("hourly:{id}|{from}|{to}")
    }

    let app: axum::Router = axum::Router::new()
        .route("/Line/{id}/Stats/{from}/to/{to}", axum::routing::get(daily_probe))
        .route("/Line/{id}/Stats/Hourly/{from}/to/{to}", axum::routing::get(hourly_probe));

    let daily_response = app.clone()
        .oneshot(Request::builder().uri("/Line/northern/Stats/2026-08-01/to/2026-08-31").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(daily_response.status(), StatusCode::OK);
    let daily_body = axum::body::to_bytes(daily_response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(String::from_utf8(daily_body.to_vec()).unwrap(), "daily:northern|2026-08-01|2026-08-31");

    let hourly_response = app
        .oneshot(Request::builder().uri("/Line/northern/Stats/Hourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(hourly_response.status(), StatusCode::OK, "the Hourly route must not be shadowed by the sibling NaiveDate route");
    let hourly_body = axum::body::to_bytes(hourly_response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(hourly_body.to_vec()).unwrap(),
        "hourly:northern|2026-08-31 00:00:00 UTC|2026-09-01 00:00:00 UTC",
    );
}
```

The exact `Display` formatting of the parsed `DateTime<Utc>` in that last
assertion should be confirmed against a real run rather than assumed --
adjust the literal if `cargo test` shows a different (but still correct)
`Display` output; the important assertion is the `StatusCode::OK` and
that it hit `hourly_probe`, not the precise string.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p api hourly_stats`
Expected: PASS, including `both_stats_routes_coexist_and_route_to_the_correct_handler`
(not `#[ignore]`d -- no live database needed, it's a pure in-process
router test like the daily route's own precedent).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/line_status.rs
git commit -m "Add GET /Line/{id}/Stats/Hourly/{from}/to/{to}"
```

---

### Task 9: Generalize `chartPoint.ts`/`TrendsCharts.tsx` to a bucket-key abstraction

**Files:**
- Modify: `frontend/app/lines/[id]/history/chartPoint.ts`
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.test.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.tsx`
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:**
- `ChartPoint.day: string` renamed to `ChartPoint.bucketKey: string`.
- `gapSpans`'s parameter/return shape: `{ day, delayRate }[]` / `{
  startDay, endDay }[]` renamed to `{ bucketKey, delayRate }[]` / `{
  startKey, endKey }[]`.
- `TrendsCharts` gains one new required prop: `granularity: 'day' |
  'hour'`.

**Depends on:** nothing in this plan's backend tasks -- purely a frontend
refactor of already-`main` code, can be worked in parallel with Tasks
1-8. Must land before Task 12 (`HourlyTrendsResults` needs the
generalized `ChartPoint`/`TrendsCharts`).

This is the most delicate task in this plan -- see Global Constraints:
every existing daily-side assertion not itself about the `day`/`bucketKey`
field name must still pass afterwards.

- [ ] **Step 1: Rename `ChartPoint.day` to `ChartPoint.bucketKey`**

In `chartPoint.ts`:

```tsx
/** Shared between `TrendsResults.tsx`/`HourlyTrendsResults.tsx` (the
 * Server Components that fetch the data and derive these) and
 * `TrendsCharts.tsx` (the Client Component that actually renders them) --
 * pulled into its own module, rather than one file importing the type
 * from the other, to avoid a Server/Client pair importing each other at
 * all.
 *
 * `bucketKey` is deliberately generic, not `day` -- this type now backs
 * both the daily rollup ("YYYY-MM-DD" London calendar-day strings) and
 * the hourly rollup (RFC3339 UTC hour-start instants, kept as the raw
 * instant string rather than a pre-formatted display label -- see
 * `TrendsCharts.tsx`'s `granularity` prop for why: two different hourly
 * buckets can share the same wall-clock hour label across a day
 * boundary, so the category-axis IDENTITY must stay the always-unique raw
 * instant, with display formatting applied separately, only for the tick
 * labels). See
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
 * Decision 9. */
export interface ChartPoint {
  bucketKey: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  sampleCycles: number;
}
```

- [ ] **Step 2: Generalize `gapSpans`/`referenceAreaBounds` and the `dataKey`/`granularity` prop in `TrendsCharts.tsx`**

```tsx
'use client';

import { LineChart } from '@mantine/charts';
import { Stack, Title } from '@mantine/core';
import { ReferenceArea } from 'recharts';
import { formatTime } from '@/lib/dateFormat';
import type { ChartPoint } from './chartPoint';

/** A contiguous run of one or more buckets where every rate/delay field is
 * `null` -- i.e. below the caller's own sparse-data floor (see
 * `TrendsResults.tsx`'s/`HourlyTrendsResults.tsx`'s own `toChartPoints`-
 * shaped helpers). Checking `delayRate === null` alone is sufficient
 * today because both of those helpers guarantee all four fields are
 * nulled together for a sparse bucket -- an implicit coupling to that
 * invariant, not something `ChartPoint`'s own type enforces. Generalized
 * from day-specific `{ day, startDay, endDay }` naming to
 * `{ bucketKey, startKey, endKey }` -- Decision 9 of
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md;
 * the underlying algorithm is unchanged from the daily-only version. */
export function gapSpans(points: { bucketKey: string; delayRate: number | null }[]): { startKey: string; endKey: string }[] {
  const spans: { startKey: string; endKey: string }[] = [];
  let current: { startKey: string; endKey: string } | null = null;
  for (const point of points) {
    if (point.delayRate === null) {
      current = current ? { startKey: current.startKey, endKey: point.bucketKey } : { startKey: point.bucketKey, endKey: point.bucketKey };
    } else {
      if (current) spans.push(current);
      current = null;
    }
  }
  if (current) spans.push(current);
  return spans;
}

/** Widens a `gapSpans` span into the actual `x1`/`x2` values handed to
 * `<ReferenceArea>` -- unchanged in substance from the daily-only
 * version (see its prior doc comment, preserved in git history), only
 * the field names are generalized. Still needed for both granularities:
 * `@mantine/charts`' `LineChart` is a Recharts point-scale category axis
 * regardless of whether the category values are day strings or hour-start
 * instants, so an isolated single-bucket gap still needs widening to its
 * neighbors to render at all. */
function referenceAreaBounds(
  span: { startKey: string; endKey: string },
  points: { bucketKey: string }[],
): { x1: string; x2: string } {
  if (span.startKey !== span.endKey) return { x1: span.startKey, x2: span.endKey };
  const idx = points.findIndex((point) => point.bucketKey === span.startKey);
  const prev = idx > 0 ? points[idx - 1].bucketKey : span.startKey;
  const next = idx >= 0 && idx < points.length - 1 ? points[idx + 1].bucketKey : span.startKey;
  return { x1: prev, x2: next };
}

/** Split out of `TrendsResults`/`HourlyTrendsResults` (both `async` Server
 * Components) purely because of `valueFormatter` below: a plain function,
 * and Next's RSC serialization refuses to pass a function prop from a
 * Server Component across the boundary into a Client Component. See git
 * history for the full incident this originally fixed -- unchanged by
 * this generalization.
 *
 * `granularity` is new: a plain, serializable `'day' | 'hour'` string
 * (never a function, so it crosses the Server/Client boundary safely from
 * either caller) that controls ONLY the x-axis tick label formatting.
 * `points[].bucketKey` stays the raw, always-unique category identity for
 * BOTH granularities (a "YYYY-MM-DD" day string, or an RFC3339 hour-start
 * instant) -- `granularity === 'hour'` additionally renders each tick
 * through `formatTime` (e.g. "14:00") for a legible axis, without
 * changing what Recharts uses as the category key. This split matters
 * because a rolling 24-hour window's wall-clock hour label can legitimately
 * repeat once (yesterday's and today's same clock hour) whenever the
 * window straddles a day boundary -- using a formatted label as the
 * category KEY itself would silently collide two distinct buckets. */
export function TrendsCharts({ points, granularity }: { points: ChartPoint[]; granularity: 'day' | 'hour' }) {
  const xAxisProps = {
    padding: { right: 12 },
    ...(granularity === 'hour' ? { tickFormatter: (value: string) => formatTime(value) } : {}),
  };

  return (
    <>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Delay / cancellation / skip rate
        </Title>
        <LineChart
          h={310}
          data={points}
          dataKey="bucketKey"
          withLegend
          series={[
            { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
            { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6', strokeDasharray: '6 4' },
            { name: 'skipRate', label: 'Skip rate', color: 'yellow.6', strokeDasharray: '2 3' },
          ]}
          valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
          connectNulls={false}
          xAxisProps={xAxisProps}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startKey}-${span.endKey}`}
                x1={x1}
                x2={x2}
                fill="var(--mantine-color-gray-5)"
                fillOpacity={0.15}
                stroke="none"
                ifOverflow="visible"
              />
            );
          })}
        </LineChart>
      </Stack>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Average delay (minutes)
        </Title>
        <LineChart
          h={220}
          data={points}
          dataKey="bucketKey"
          series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
          valueFormatter={(value) => `${value.toFixed(1)} min`}
          connectNulls={false}
          xAxisProps={xAxisProps}
        >
          {gapSpans(points).map((span) => {
            const { x1, x2 } = referenceAreaBounds(span, points);
            return (
              <ReferenceArea
                key={`${span.startKey}-${span.endKey}`}
                x1={x1}
                x2={x2}
                fill="var(--mantine-color-gray-5)"
                fillOpacity={0.15}
                stroke="none"
                ifOverflow="visible"
              />
            );
          })}
        </LineChart>
      </Stack>
    </>
  );
}
```

- [ ] **Step 3: Update `TrendsResults.tsx`'s `toChartPoints` and its `<TrendsCharts>` call**

In `toChartPoints` (currently lines 23-35), rename the field it produces:

```tsx
export function toChartPoints(stats: LineDailyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES;
    return {
      bucketKey: row.day,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.sampleCycles,
    };
  });
}
```

And its `<TrendsCharts>` call (currently line 86):

```tsx
      <TrendsCharts points={points} granularity="day" />
```

- [ ] **Step 4: Update `TrendsCharts.test.tsx`'s existing fixtures for the rename, then add a parallel hourly fixture set**

Per Global Constraints, every existing case's *behavior* must be
preserved -- only the field names change. Rename the `point()` helper and
every `day`/`startDay`/`endDay` reference to `bucketKey`/`startKey`/
`endKey` throughout the file (all 8 existing `it()` blocks), keeping the
day-string literal values (`'2026-08-01'` etc.) exactly as they are today
-- they remain perfectly valid `bucketKey` values, just under the new
field name. Confirm the "merges a multi-day gap" case keeps its actual
current (not the older plan's buggy) expectation: `{ startKey:
'2026-08-02', endKey: '2026-08-03' }` for input days 01(0.1)/02(null)/
03(null)/04(0.2) -- per this file's own existing comment (Status note
above), do not regress to the `endDay: '2026-08-04'` value the older
chart-fixes plan's own fixture literal had, which its own test file
already flags as inconsistent with the algorithm.

Then add a second `describe('gapSpans (hourly buckets)', ...)` block with
RFC3339-shaped fixtures, confirming the generalization didn't silently
narrow behavior for the shape it now also has to support -- not just a
rename of the day cases:

```tsx
describe('gapSpans (hourly buckets)', () => {
  function hourPoint(hourStart: string, delayRate: number | null) {
    return { bucketKey: hourStart, delayRate };
  }

  it('returns a single-bucket span for one isolated sparse hour', () => {
    expect(
      gapSpans([
        hourPoint('2026-08-31T12:00:00Z', 0.1),
        hourPoint('2026-08-31T13:00:00Z', null),
        hourPoint('2026-08-31T14:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-08-31T13:00:00Z', endKey: '2026-08-31T13:00:00Z' }]);
  });

  it('merges a multi-hour gap into one span, including one that crosses a day boundary', () => {
    expect(
      gapSpans([
        hourPoint('2026-08-31T23:00:00Z', 0.1),
        hourPoint('2026-09-01T00:00:00Z', null),
        hourPoint('2026-09-01T01:00:00Z', null),
        hourPoint('2026-09-01T02:00:00Z', 0.2),
      ]),
    ).toEqual([{ startKey: '2026-09-01T00:00:00Z', endKey: '2026-09-01T01:00:00Z' }]);
  });

  it('does not collide two buckets that share the same wall-clock hour label on different days', () => {
    // Regression guard for the finding in this plan's Status note: the raw
    // RFC3339 instant, not a formatted "HH:mm" label, must be what
    // gapSpans/referenceAreaBounds treat as the bucket identity.
    const points = [
      hourPoint('2026-08-30T14:00:00Z', 0.1),
      hourPoint('2026-08-31T14:00:00Z', null),
    ];
    const spans = gapSpans(points);
    expect(spans).toEqual([{ startKey: '2026-08-31T14:00:00Z', endKey: '2026-08-31T14:00:00Z' }]);
    expect(spans[0].startKey).not.toBe(spans[0].endKey === points[0].bucketKey ? points[0].bucketKey : undefined);
  });
});
```

- [ ] **Step 5: Update `TrendsResults.test.tsx` for the field rename**

Every fixture/assertion in this file that reads `.day`/produces `{ day:
... }` needs the field renamed to `bucketKey` -- in particular
`toChartPoints`'s two tests (currently lines 65-87, expecting `{ day:
'2026-08-01', ... }` -- change to `bucketKey: '2026-08-01'`) and the
sparse-day test's `points.find((point) => point.day === '2026-08-01')`
(currently line 112 -- change to `point.bucketKey === '2026-08-01'`).
`row()`'s own `LineDailyStats` fixture shape is unaffected (that type
still has `day`, unchanged -- only `ChartPoint`, the *output* of
`toChartPoints`, is renamed). No behavioral assertion changes.

- [ ] **Step 6: Run the full existing test suite**

Run (from `frontend/`): `npm test -- TrendsCharts TrendsResults`
Expected: PASS -- every pre-existing assertion (legend, dash patterns,
`valueFormatter`, `xAxisProps.padding`, `connectNulls`, empty state, gap
spans) still passes unmodified in substance, confirming Global
Constraints' "no daily-side regression" requirement.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/lines/[id]/history/chartPoint.ts frontend/app/lines/[id]/history/TrendsCharts.tsx frontend/app/lines/[id]/history/TrendsCharts.test.tsx frontend/app/lines/[id]/history/TrendsResults.tsx frontend/app/lines/[id]/history/TrendsResults.test.tsx
git commit -m "Generalize ChartPoint/gapSpans/TrendsCharts to a bucket-key abstraction shared by daily and hourly views"
```

---

### Task 10: `getLineHourlyStats` + `LineHourlyStats` type

**Files:**
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- `export async function getLineHourlyStats(id: string, from: string, to: string): Promise<LineHourlyStats[]>`
- `export interface LineHourlyStats { hourStart: string; sampleCycles: number; total: number; delayed: number; cancelled: number; skipped: number; avgDelayMinutes: number; delayRate: number; cancellationRate: number; skipRate: number; }`

**Depends on:** Task 8 (the route's exact path/response shape).

- [ ] **Step 1: Add `LineHourlyStats` to `lib/types.ts`**

Directly below `LineDailyStats` (after line 134):

```tsx
/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}`'s per-hour response shape
 * -- hourly sibling of `LineDailyStats`. `hourStart` is an RFC3339 UTC
 * instant (the top of the hour), not a calendar day -- always render it
 * through `frontend/lib/dateFormat.ts`'s `formatTime` before display, same
 * convention `LineDailyStats.day` follows for its own rendering. Same
 * dedup/attribution caveat as `LineDailyStats` applies, reworded for "that
 * hour" instead of "that day". */
export interface LineHourlyStats {
  hourStart: string; // RFC3339 UTC instant, top of the hour
  sampleCycles: number;
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
}
```

- [ ] **Step 2: Add `getLineHourlyStats` to `lib/api.ts`**

Directly below `getLineDailyStats` (after line 138), also adding
`LineHourlyStats` to the top-of-file type import list:

```tsx
/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}` -- the hourly rollup
 * route. Unlike `getLineDailyStats`, `from`/`to` are passed straight
 * through as RFC3339 instants (no `londonDayKey` conversion) -- the
 * route's own path segments are `DateTime<Utc>`, not `NaiveDate`, since an
 * hour bucket has no calendar-day analog to round-trip through (Decision
 * 6 of docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md).
 * Same public, no-store, no-cookie-forwarding shape as
 * `getLineDailyStats`/`getLineStatusHistory`. */
export async function getLineHourlyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineHourlyStats[]> {
  return fetchJson<LineHourlyStats[]>(
    `${baseUrl()}/Line/${id}/Stats/Hourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}
```

- [ ] **Step 3: Add a test**

In `frontend/lib/api.test.ts`, mirroring `getLineDailyStats`'s existing
URL-building test:

```tsx
it('getLineHourlyStats builds the correct URL', async () => {
  mockFetchOnce([]);
  await getLineHourlyStats('wcml', '2026-08-31T00:00:00.000Z', '2026-09-01T00:00:00.000Z');
  expect(fetchSpy).toHaveBeenCalledWith(
    `${API_BASE_URL}/Line/wcml/Stats/Hourly/2026-08-31T00:00:00.000Z/to/2026-09-01T00:00:00.000Z`,
    expect.objectContaining({ cache: 'no-store' }),
  );
});
```

Adapt the exact mock helper names (`mockFetchOnce`/`fetchSpy`/
`API_BASE_URL`) to whatever this file's existing `getLineDailyStats` test
actually uses -- read that test first and match its pattern exactly
rather than inventing a new one.

- [ ] **Step 4: Run the tests**

Run (from `frontend/`): `npm test -- api.test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/types.ts frontend/lib/api.test.ts
git commit -m "Add getLineHourlyStats/LineHourlyStats"
```

---

### Task 11: `resolveHourlyRange` helper

**Files:**
- Modify: `frontend/lib/history.ts`
- Modify: `frontend/lib/history.test.ts`

**Interfaces:** `export function resolveHourlyRange(now: number): { from: string; to: string }`

**Depends on:** nothing -- small, independent, pure function.

- [ ] **Step 1: Add the helper**

Directly below `resolveRange` (after line 188 in `history.ts`):

```tsx
const HOUR_MS = 3_600_000;

/** The line-info-page embed's fixed rolling 24-hour window -- deliberately
 * NOT a `RangePreset`/`resolveRange` variant: Decision 11 of
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
 * is explicit that this view has no user-selectable range at all, unlike
 * the history page's day/30-day presets (which live in the URL). No
 * `preset`/`from`/`to`-from-query-params handling is needed here for the
 * same reason -- this always resolves the same window relative to `now`. */
export function resolveHourlyRange(now: number): { from: string; to: string } {
  return {
    from: new Date(now - 24 * HOUR_MS).toISOString(),
    to: new Date(now).toISOString(),
  };
}
```

- [ ] **Step 2: Add a test**

In `frontend/lib/history.test.ts`, alongside the existing
`describe('resolveRange', ...)` block:

```tsx
describe('resolveHourlyRange', () => {
  it('resolves exactly a 24-hour window ending at now', () => {
    const range = resolveHourlyRange(NOW);
    expect(range.to).toBe(new Date(NOW).toISOString());
    expect(Date.parse(range.to) - Date.parse(range.from)).toBe(24 * 60 * 60 * 1000);
  });
});
```

(`NOW` should reuse this file's existing top-of-file constant, the same
one `resolveRange`'s tests already use.)

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- history.test`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/history.ts frontend/lib/history.test.ts
git commit -m "Add resolveHourlyRange, the fixed rolling-24h window helper for the hourly embed"
```

---

### Task 12: New `HourlyTrendsResults.tsx` Server Component

**Files:**
- Create: `frontend/app/lines/[id]/history/HourlyTrendsResults.tsx`
- Create: `frontend/app/lines/[id]/history/HourlyTrendsResults.test.tsx`

**Interfaces:** `export async function HourlyTrendsResults({ id, from, to }: { id: string; from: string; to: string }): Promise<JSX.Element>`

**Depends on:** Task 9 (generalized `ChartPoint`/`TrendsCharts`), Task 10
(`getLineHourlyStats`/`LineHourlyStats`).

Per Decision 10, this is a genuinely new, structurally-parallel component
-- not a `granularity`-branching rewrite of `TrendsResults` itself. Its
own sparse-floor constant, honesty copy, and fetch are all
hourly-specific content, not shared plumbing (the shared part is
`TrendsCharts`, already generalized in Task 9).

- [ ] **Step 1: Write the component**

```tsx
import { Paper, Stack, Text, Title } from '@mantine/core';
import { getLineHourlyStats } from '@/lib/api';
import type { LineHourlyStats } from '@/lib/types';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholder, not a validated number -- see
// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
// Decision 8. Deliberately NOT a reuse of TrendsResults.tsx's
// SPARSE_DATA_FLOOR_CYCLES (20 out of a ~1,440-cycle/day ceiling at the
// default 60s poll interval -- reusing it as-is against an hour's
// ~60-cycle ceiling would demand ~33% coverage, a much stricter bar).
// This value (also 20, but re-derived against the hourly ceiling as
// roughly a third of an hour's maximum possible coverage) is Decision 8's
// own "more defensible starting placeholder" -- revisit against real
// sample_cycles-per-hour distributions once this has run in production.
const SPARSE_DATA_FLOOR_CYCLES_HOURLY = 20;

// Hourly sibling of TrendsResults.tsx's toChartPoints -- same
// null-all-four-fields-together gap logic, same connectNulls={false}
// rendering it feeds, different floor and a different source field
// (`hourStart`, an RFC3339 instant) becoming `bucketKey`.
export function toHourlyChartPoints(stats: LineHourlyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES_HOURLY;
    return {
      bucketKey: row.hourStart,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.sampleCycles,
    };
  });
}

/** Structurally parallel to `TrendsResults` (Decision 10) -- deliberately
 * a separate component, not a `granularity`-branching version of it,
 * since the fetch, sparse floor, and honesty copy are all genuinely
 * hourly-specific. The shared, reusable part is `TrendsCharts`
 * (generalized in the same plan's Task 9), which this renders exactly
 * the way `TrendsResults` does, passing `granularity="hour"`. */
export async function HourlyTrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  const stats = await getLineHourlyStats(id, from, to);

  if (stats.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet for this line.</Text>
      </Paper>
    );
  }

  const points = toHourlyChartPoints(stats);

  return (
    <Stack gap="lg">
      {/* Same honesty-copy posture as TrendsResults.tsx's own comment
          (marked "Must not be softened or dropped") -- reworded from "that
          day" to "that hour" per Decision 2's hourly attribution, not a
          new tradeoff. */}
      <Text size="sm" c="dimmed">
        Rates shown count each distinct train once per hour, based on its status the first time it was seen that
        hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while
        still in view will still show here as on time. Hours with too little coverage show as a gap rather than a
        misleading flat line.
      </Text>
      <TrendsCharts points={points} granularity="hour" />
    </Stack>
  );
}
```

- [ ] **Step 2: Write the test file**

Mirror `TrendsResults.test.tsx`'s structure (mock `@mantine/charts`,
mock `@/lib/api`'s `getLineHourlyStats`), covering: empty state (bounded
`Paper`), a sparse hour becoming a gap (not a zero), a normal multi-hour
range rendering both charts with the "that hour" honesty copy, and that
`granularity="hour"` actually reaches `TrendsCharts` (assert
`xAxisProps.tickFormatter` is a function on the mocked `LineChart`
call, or -- simpler and less implementation-coupled -- assert on a
representative formatted tick value if the mock captures
`xAxisProps.tickFormatter('2026-08-31T14:00:00Z')` directly and checks
it returns a `formatTime`-shaped string). Follow
`TrendsResults.test.tsx`'s exact mocking conventions (the `MockLineChartProps`
shape, `lineChartMock`, `row()`-style fixture builder renamed to
`hourlyRow()`) rather than inventing a new pattern.

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- HourlyTrendsResults`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/[id]/history/HourlyTrendsResults.tsx frontend/app/lines/[id]/history/HourlyTrendsResults.test.tsx
git commit -m "Add HourlyTrendsResults, the hourly sibling of TrendsResults"
```

---

### Task 13: Wire the line-info-page embed to the hourly view

**Files:**
- Modify: `frontend/app/lines/[id]/page.tsx`
- Modify: `frontend/app/lines/[id]/page.test.tsx`

**Interfaces:** No new exports -- swaps which component/range the embed
uses.

**Depends on:** Task 11 (`resolveHourlyRange`), Task 12
(`HourlyTrendsResults`).

- [ ] **Step 1: Swap the import and the range computation**

In `page.tsx`, replace:

```tsx
import { resolveRange } from '@/lib/history';
import { TrendsResults } from './history/TrendsResults';
```

with:

```tsx
import { resolveHourlyRange } from '@/lib/history';
import { HourlyTrendsResults } from './history/HourlyTrendsResults';
```

And replace the `trendsRange` computation (currently line 104):

```tsx
  // A fixed rolling 24-hour window, not a URL-driven preset -- this embed
  // has no range picker of its own (Decision 11 of
  // docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md).
  // "View history" below remains the way to reach the full range picker
  // and the daily Trends tab.
  const trendsRange = resolveHourlyRange(now);
```

- [ ] **Step 2: Update the heading and the `<Suspense>` block**

Replace the section at lines 161-184:

```tsx
      <Stack gap="xs">
        <Title order={2} size="h4">
          Recent trends (last 24 hours)
        </Title>
        {/* Hourly, not the dedicated history page's daily rollup -- Decision
            1/2 of docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md:
            a rolling 24-hour window needs intra-day resolution the daily
            table can't provide, so this renders through a new, separate
            hourly fetch/component (HourlyTrendsResults) rather than
            TrendsResults. It still shares TrendsCharts -- the actual chart
            rendering, legend, dash patterns, gap bands, edge padding --
            with the dedicated history page's daily Trends tab; only the
            fetch, sparse-data floor, and copy are hourly-specific.
            `View history` above remains the way to reach the full range
            picker, the Timeline tab, and the daily Trends tab.

            Wrapped in its own Suspense boundary, same rationale as before:
            `getLineHourlyStats` is comparatively slow, and without this
            boundary it would block the whole page behind a chart a visitor
            may not even scroll down to see. A brand-new line with no
            hourly-stats rows yet still resolves fast: `HourlyTrendsResults`
            renders its own "Not enough sampled data yet" text rather than
            leaving this section hanging. */}
        <Suspense fallback={<Skeleton height={280} />}>
          <HourlyTrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
      </Stack>
```

- [ ] **Step 3: Update `page.test.tsx`**

- In the top-of-file `vi.mock('@/lib/api', ...)` factory (currently lines
  9-19), replace `getLineDailyStats: vi.fn()` with `getLineHourlyStats:
  vi.fn()`.
- Replace the `dailyStatsRow()` fixture helper with an `hourlyStatsRow()`
  one (`hourStart: '2026-08-30T14:00:00Z'` in place of `day:
  '2026-08-30'`, everything else unchanged).
- In the `LineDetailPage Edit/Delete visibility` describe block's
  `beforeEach` (currently line 91), change `vi.mocked(api.getLineDailyStats
  ).mockResolvedValue([])` to `vi.mocked(api.getLineHourlyStats
  ).mockResolvedValue([])`.
- In the `LineDetailPage embedded trends` describe block (currently lines
  119-159): update both `it()`s to mock `api.getLineHourlyStats` instead
  of `api.getLineDailyStats`, using `hourlyStatsRow()` fixtures; update
  the heading assertion from `'Recent trends (last 7 days)'` to `'Recent
  trends (last 24 hours)'` in both places it appears.

- [ ] **Step 4: Run the tests**

Run (from `frontend/`): `npm test -- lines/\[id\]/page`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/[id]/page.tsx frontend/app/lines/[id]/page.test.tsx
git commit -m "Show hourly trends over a rolling 24h window on the line-info-page embed"
```

---

### Task 14: End-to-end verification -- screenshots + full test/build suite

**Files:** none modified by this task (verification only).

**Depends on:** Tasks 1-13, all landed and committed.

Mirrors the chart-fixes plan's own Task 7 posture: this repo's Vitest
suite mocks `@mantine/charts` wholesale and never exercises real Recharts
rendering, so passing unit tests prove props reached the mock, not that
the hourly chart actually renders correctly (legend, gap bands, and --
new to this plan -- the `formatTime` tick labels and the no-collision
behavior across a day-boundary-straddling window). Do not consider this
plan complete until this task's checks pass.

- [ ] **Step 1: Migrate + bring up a real, data-backed dev server**

Per this repo's README "Running it" section: bring up the compose stack
(ensuring the new migration from Task 1 applies), run the aggregator
against it for at least a couple of poll cycles so
`line_status_hourly_stats` actually accumulates real rows for at least
one line, then `npm run dev` in `frontend/`.

- [ ] **Step 2: Screenshot the line-info-page embed**

Navigate to `/lines/{id}` for a line with real accumulated hourly data.
Confirm:
- The heading reads "Recent trends (last 24 hours)".
- Both charts render with the legend, dash patterns, gap bands, and
  right-edge padding intact (inherited from `TrendsCharts`, unchanged by
  this plan).
- X-axis tick labels read as short times (e.g. "14:00"), not raw RFC3339
  strings -- confirms the `granularity="hour"` `tickFormatter` actually
  applies.
- If the dev session has been running across a day boundary (or seed data
  can be crafted to simulate it), confirm two ticks sharing the same
  wall-clock hour label render as two distinct points, not one merged
  category -- the specific risk this plan's Status note and Task 9's
  regression test both flag.

- [ ] **Step 3: Confirm the dedicated history page is untouched**

Navigate to `/lines/{id}/history`, Trends tab. Confirm it still shows
daily data over the selected range, with `dataKey`/x-axis behavior
unchanged from before this plan (raw day-string ticks, no
`tickFormatter`) -- the live confirmation of Decision 10/Correction 6 and
of Task 9's "no daily-side regression" constraint.

- [ ] **Step 4: Confirm a brand-new/quiet line's empty state**

For a line with no `line_status_hourly_stats` rows yet, confirm the
embed shows the same bounded "Not enough sampled data yet for this line."
`Paper` the daily view already uses -- not a broken/loading-forever
state.

- [ ] **Step 5: Full test + build check, both ecosystems**

Run:
```
cargo test -p aggregator
cargo test -p api
cd frontend && npm test && npm run build
```
Expected: PASS across all three. (Live-DB `#[ignore]`d Rust tests from
Tasks 3/7 should also be run at least once against the compose Postgres
brought up in Step 1, per each task's own `DATABASE_URL=... -- --ignored`
invocation.)

- [ ] **Step 6: Triage**

Any mismatch found in Steps 2-4 gets fixed by reopening the relevant
numbered task above and re-running its own steps, per
superpowers:systematic-debugging -- not patched ad hoc inside this
verification task.

No commit for this task by itself (verification-only) unless Step 6
required reopening an earlier task, in which case that task's own commit
step covers it.

---

## Not in this plan

Carried forward from the design spec's own "Explicitly out of scope,"
not silently dropped:

- Any change to the dedicated history page's Trends-tab granularity, its
  `HistoryRangePicker`, or its date-range presets -- confirmed already
  correct (daily-over-the-selected-range), no code change proposed there
  beyond Task 9's chart-leaf generalization, which changes no user-visible
  behavior on that page.
- A granularity toggle/selector UI anywhere (e.g. letting a user switch
  the history page between daily and hourly) -- not asked for, not
  designed by the spec, not built here.
- True sub-hourly (e.g. 15-minute) granularity -- would need its own
  floor/retention recalibration, out of scope.
- Any change to `crates/aggregator/src/dedup.rs` -- Decision 2's whole
  point is that none is needed; if implementation of this plan ever seems
  to require one, that's a signal something has gone wrong, not a
  legitimate extension of scope.
- Backfilling hourly history retroactively -- the new table starts
  accumulating from zero on deployment day, same posture as
  `line_status_daily_stats` did originally.
- DLR/TfL sample-stats pipeline parity -- national-rail-only scope,
  unchanged from the original daily-stats work.
- A caching layer for the new hourly endpoint -- judged unnecessary at
  this catalogue's scale (~105 lines, at most ~25 rows/request), same
  reasoning as the existing uncached daily route.
- Changing `AutoRefresh`'s cadence or building bespoke live-update
  animation for the filling current hour -- the existing 30s
  `router.refresh()` cadence already re-runs `HourlyTrendsResults`'s fetch
  for free, same mechanism the daily embed already relied on.
- Retroactively re-tuning `SPARSE_DATA_FLOOR_CYCLES`'s existing daily-case
  value, or empirically validating either the new hourly floor (`20`,
  Task 12) or the new `hourly_stats_retention_hours` default (`48`, Task
  4) against real production traffic -- both are carried-over placeholders
  per the spec's own Open questions 1-2, future recalibration work, not
  part of implementing this plan.
- A validation floor on the new `hourly_stats_retention_hours` knob
  guarding against an operator misconfiguring it below ~25h -- consistent
  with this repo's existing lack of one on `history_retention_days`/
  `daily_stats_retention_days`, flagged but not designed away by the spec,
  and not added here either.
