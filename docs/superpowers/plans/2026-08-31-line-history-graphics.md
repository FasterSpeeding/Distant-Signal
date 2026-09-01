# Per-Line Historical Graphics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each line's `/lines/[id]/history` page a "Trends" tab showing four historical metrics — cancellation rate, skipped-stop rate, average delay minutes, and delay rate — as charts over the selected date range. Today `common::SampleStats` is computed fresh every aggregation cycle but never durably persisted as a real time series (`line_status_history` strips it before deciding whether to write a row at all, per the design spec's Correction 1), so this plan's real job is: add a new daily rollup table and its incremental writer, a new read route over it, and a new charting tab — nothing here reuses or repurposes `line_status_history`.

**Architecture:**
```
frontend/app/lines/[id]/history/page.tsx      EXTENDED -- Tabs split (Timeline | Trends)
frontend/app/lines/[id]/history/TrendsResults.tsx   NEW -- fetches stats, renders charts
frontend/lib/api.ts        + getLineDailyStats
frontend/lib/types.ts      + LineDailyStats
frontend/package.json      BUMPED: @mantine/* ^9.4.1 -> 9.5.2, react/react-dom ^19.0.0 -> ^19.2.0
                            ADDED: @mantine/charts, recharts
                    |
                    | server-side fetch, no-store, no proxy (public route)
                    v
crates/api/src/routes/line_status.rs       + GET /Line/{id}/Stats/{from}/to/{to}
crates/api/src/data/queries.rs             + daily_stats_for_range, DailyStatsRow
crates/api/migrations/*_line_status_daily_stats.sql   NEW table
                    ^
                    | writes, once per line per cycle
                    |
crates/aggregator/src/queries.rs           + record_daily_stats, prune_daily_stats
crates/aggregator/src/main.rs              new call sites in run_cycle
crates/aggregator/src/config.rs            + daily_stats_retention_days
```

**Tech Stack:** Rust (`sqlx`, `chrono`/`chrono-tz`, `axum`) for the backend; Next.js App Router + TypeScript + Mantine v9 + `@mantine/charts` (Recharts under the hood) for the frontend; Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) for frontend tests, `#[ignore]`d live-database integration tests for the new SQL, matching this repo's existing convention in both `crates/aggregator/src/queries.rs` and `crates/api/src/data/queries.rs`.

**Spec:** `docs/superpowers/specs/2026-08-31-line-history-graphics-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Correction N" refer to that document.

**Status note:** unlike the journey-ticket-tracking-frontend plan's starting state, **no backend prerequisite for this feature exists yet** — confirmed while writing this plan: `line_status_daily_stats` does not exist in `crates/api/migrations/`, no `record_daily_stats`/`prune_daily_stats` exist in `crates/aggregator/src/queries.rs` (that file's current top-level functions are `load_incidents`, `load_station_samples`, `load_custom_lines`, `prune_removed_lines`, `write_line_status`, `prune_history`), and `crates/api/src/routes/line_status.rs` currently mounts exactly four routes (`/Line/Mode/{mode}/Status`, `/Line/{ids}/Status`, `/StopPoint/{crs}/Disruption`, `/Line/{id}/Status/{from}/to/{to}`) — no fifth `/Stats` route. This plan builds the whole vertical slice, backend through frontend, in that order.

**A note on parallel work:** a separate subagent is working on true per-service delay-repay dedup as unrelated, parallel work in this same repo. No task in this plan touches per-service/`service_id` deduplication of `SampleStats` counting — the per-poll-cycle counting semantics from Decision 2 of the design spec are what v1 (this plan) ships with, on purpose, not as a placeholder awaiting that other work.

## Global Constraints

- **No reuse of `line_status_history` or its writer.** `queries::write_line_status`'s dedup semantics (skip a write when `normalize_for_diff` says nothing changed) are load-bearing for the Timeline tab and must not be touched by this plan. The new rollup is entirely new table, new writer function, new call site.
- **`record_daily_stats` is called at most once per line per cycle**, never once per `LineStatus` entry a line currently carries. `aggregate()` (`crates/aggregator/src/aggregation.rs:104` and the `infer_from_samples` branch) clones the *same* `SampleStats` value onto every simultaneous status a line has; the new call site in `run_cycle` must read exactly one `SampleStats` per line's `LineStatusReport` (e.g. from `report.statuses.first().and_then(|s| s.sample_stats.clone())`, since every status on a report carries an identical clone when `Some`) and skip lines where it's `None`, not iterate `report.statuses`. Getting this wrong silently inflates every sum by however many concurrent incidents a line has — this is the exact failure mode the design spec's own sketch (Decision 1) calls out, and Task 3 below includes a test specifically for it.
- **`day` is a plain Europe/London calendar day, computed the same way `frontend/lib/dateFormat.ts`'s `londonDayKey` computes it for the Timeline tab** — not the aggregator's rail-day 02:00 cutoff (`aggregation.rs`'s `next_rail_day_boundary`, used elsewhere for incident staleness). Do not reuse or adapt `next_rail_day_boundary` for this feature.
- **Rates are computed at read time from stored sums, never stored pre-divided.** `daily_stats_for_range`/the new route return raw sums (`total`, `delayed`, `cancelled`, `skipped`, `runningCount`, `delayMinutesSum`) *and* the derived `avgDelayMinutes`/`delayRate`/`cancellationRate`/`skipRate` fields, computed server-side with an explicit `total == 0` guard (never a divide-by-zero/`NaN` reaching JSON).
- **All rate/metric language is "per sampled poll cycle," never "per train."** Per Decision 2, this is an accepted, honestly-labelled v1 limitation, not a bug to work around. No task may add per-service dedup, a `service_id` ledger, or any other mechanism intended to convert this into a "per train" metric — that is out of scope, being handled separately (see the parallel-work note above). Backend field/variable names avoid "service"/"train" phrasing (`sample_cycles`, not e.g. `service_count`); frontend copy says "share of sampled poll cycles," never "% of trains."
- **Sparse/no-data handling is driven by `sample_cycles`, never by `data_quality`.** A day below the sparse-data floor renders as a genuine chart gap (no point, no interpolation, no zero); a range with zero qualifying rows renders an explicit "Not enough sampled data yet for this line" state instead of an empty or flat-zero chart. See the Open judgment call below for the floor's actual value.
- **Average delay minutes is never plotted on the same axis/chart as the three rate metrics** (different unit — minutes vs. a 0–1 proportion). Two separate charts, sharing the same x-axis (day) and the same gap-rendering behavior.
- **No backfill.** The rollup starts accumulating from whatever day this feature ships; no task may attempt to reconstruct earlier history from `line_status_history` or anywhere else (per Correction 1, no raw per-cycle archive exists to backfill from).
- **DLR/TfL's separate `sample_stats` pipeline (`crates/poller-tfl`, `upsert_tfl_line_status`) is out of scope.** No task may add a second `record_daily_stats` call site inside `crates/api/src/data/queries.rs`'s `upsert_tfl_line_status` or anywhere else in `poller-tfl`. This plan covers `national-rail` lines only, exactly as the design spec scopes it.
- **The Timeline tab's own pre-existing 7-day retention truncation is not silently fixed as a side effect.** Decision 5 recommends surfacing it (a disabled/greyed preset button with a tooltip) once the new Trends tab sits next to it in one `Tabs` control — Task 8 below does exactly that, and only that (a UI-level surfacing), not a change to `history_retention_days` itself or to what the Timeline tab can return.
- **Testing convention:** Rust — `#[cfg(test)]` modules colocated in the same file, `#[ignore]`d integration tests against a live database for anything touching `sqlx::query`, matching `crates/aggregator/src/queries.rs`'s and `crates/api/src/data/queries.rs`'s existing patterns; run via `cargo test` (fast, non-DB tests) and `cargo test -- --ignored` (DB tests, requires `DATABASE_URL`). Frontend — colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine`, Vitest (`npm test` from `frontend/`). Every task's verification step runs the full relevant suite (`cargo test -p aggregator` / `cargo test -p api` / `npm test && npm run build` from `frontend/`) and requires it to pass with no new failures.
- **Chart pixel output is not asserted on.** Consistent with this repo having no existing precedent for testing chart pixel output (per the design spec's own Testing section), frontend chart tests assert render-without-throwing plus the surrounding empty-state/gap-state text and data-driven props, not Recharts' SVG internals.

## Open judgment calls made when sequencing this plan (not decided by this plan — flagged, not resolved)

The design spec left several product-level questions open rather than resolving them from code alone. This plan sequences around them without deciding them:

1. **`daily_stats_retention_days`'s default value** (spec Open question 1) — the config knob is added in Task 2 with **no pruning by default** (`Option<i64>`, `None` unless set), matching the spec's own "unset/no pruning for v1" fallback option rather than picking a specific day-count. `prune_daily_stats` (Task 5) is still implemented and wired into `run_cycle` behind that `Option`, so flipping on retention later is a config change, not a code change. Whether it should default to unset-forever or some generous number (the spec's own alternative, e.g. `400`) is left for the repo owner to decide before/at ship time — not decided by this plan.
2. **The sparse-data floor (`sample_cycles` threshold)** (spec Open question 3) — Task 7 implements the gap-rendering *mechanism* generically, parameterized by a floor constant, and the plan sets that constant to the spec's own example value, `20`, as a literal placeholder clearly marked as such (a code comment pointing back at this open question), not a validated number. Task 7's acceptance criteria do not require validating this number against real traffic — that was explicitly flagged in the spec as needing real `sample_cycles` distributions once the rollup has been running, which this plan cannot produce before it ships.
3. **`sample_stations` catalogue coverage audit** (spec Open question 2) — not performed by any task in this plan. The sparse/empty-state UI (Task 7) is built to degrade honestly regardless of how common the empty case turns out to be, so this plan does not block on auditing the 105 line TOML files first; that audit is called out as follow-up work worth doing once this ships, not a prerequisite.
4. **`DATE`/route path segment type** (spec Open question 6) — this plan's Task 4 makes a concrete choice (`NaiveDate` path segments on the new route, matching the table's own `DATE` column type directly, rather than reusing the existing history route's `DateTime<Utc>` convention) since the spec left it as "implementation-time call" and a plan needs one concrete answer to write route code against — flagged here as this plan's own call, not re-litigating the spec.
5. **`DATE`/Task 4's status-code posture on an unknown `line_id`** follows the spec's explicit instruction directly (empty array, not `404`, matching `line_status_history_for_range`'s existing behavior) — not a judgment call, restated here only to confirm it was carried forward.

---

### Task 1: `line_status_daily_stats` migration

**Files:**
- Create: `crates/api/migrations/20260831090000_line_status_daily_stats.sql`

**Interfaces:**
- Produces: the `line_status_daily_stats` table (schema below) and its `(line_id, day)` index.
- Consumed by: Task 3 (`record_daily_stats`/`prune_daily_stats` writes), Task 4 (`daily_stats_for_range` reads).

Owned by the `api` crate's migrations directory (run at `api` startup), matching how `line_status`/`line_status_history` themselves are defined in `20260510023522_initial.sql` despite being aggregator-written — the design spec's Decision 1 is explicit this is the convention to follow, not a new one to invent.

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260831090000_line_status_daily_stats.sql`:

```sql
-- -------------------------------------------------------------------------
-- Per-line daily rollup of SampleStats, written incrementally once per line
-- per aggregation cycle by crates/aggregator/src/queries.rs's
-- record_daily_stats. Exists because line_status_history cannot serve as a
-- SampleStats time series -- its own write path (write_line_status)
-- deliberately strips sample_stats before deciding whether a row changed,
-- so most cycles' numbers are never recorded anywhere else. See
-- docs/superpowers/specs/2026-08-31-line-history-graphics-design.md,
-- Decision 1.
--
-- `day` is a plain Europe/London CALENDAR day (midnight-to-midnight,
-- matching frontend/lib/dateFormat.ts's londonDayKey / the Timeline tab's
-- own grouping) -- NOT the aggregator's separate rail-day 02:00 cutoff used
-- elsewhere for incident staleness (next_rail_day_boundary). These are two
-- deliberately different conventions coexisting in this codebase -- see the
-- design spec's Open question 5.
--
-- Every numeric column is a running SUM across however many poll cycles
-- contributed to this line on this day -- rates are derived at READ time
-- (crates/api/src/data/queries.rs's daily_stats_for_range), never stored
-- pre-divided.
--
-- The rate this produces is a share of SAMPLED POLL CYCLES, not a share of
-- distinct trains -- SampleStats.total counts departures currently visible
-- in a poll's LDBWS response window, and the same physical service is
-- likely counted across many consecutive polls (Decision 2). This is an
-- accepted, explicitly-labelled v1 limitation -- true per-service
-- deduplication is separate, later work, not designed or built here.
-- -------------------------------------------------------------------------

CREATE TABLE line_status_daily_stats (
    line_id           TEXT             NOT NULL,
    day               DATE             NOT NULL,  -- Europe/London calendar day

    -- How many poll cycles contributed data to this row -- the coverage
    -- signal the frontend's sparse-data / gap rendering is driven by
    -- (never data_quality). See Decision 3.
    sample_cycles     BIGINT           NOT NULL DEFAULT 0,

    total             BIGINT           NOT NULL DEFAULT 0,  -- sum of SampleStats.total
    delayed           BIGINT           NOT NULL DEFAULT 0,
    cancelled         BIGINT           NOT NULL DEFAULT 0,
    skipped           BIGINT           NOT NULL DEFAULT 0,

    -- Sum of "running" (non-cancelled) departures per cycle -- the
    -- denominator for avg_delay_minutes, since SampleStats.avg_delay_minutes
    -- is itself averaged over non-cancelled departures only.
    running_count     BIGINT           NOT NULL DEFAULT 0,

    -- Sum of (SampleStats.avg_delay_minutes * that cycle's running count),
    -- so avgDelayMinutes can be recovered at read time as
    -- delay_minutes_sum / running_count without losing precision to
    -- averaging-of-averages.
    delay_minutes_sum DOUBLE PRECISION NOT NULL DEFAULT 0,

    PRIMARY KEY (line_id, day)
);

CREATE INDEX line_status_daily_stats_line_day ON line_status_daily_stats (line_id, day);
```

- [ ] **Step 2: Run the migration against a local database**

Run (from repo root, with `DATABASE_URL` pointed at a local dev Postgres — see this repo's existing compose setup): `sqlx migrate run --source crates/api/migrations`
Expected: migration applies cleanly; `\d line_status_daily_stats` in `psql` shows the expected columns and the composite primary key.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260831090000_line_status_daily_stats.sql
git commit -m "Add line_status_daily_stats table for per-line historical rollups"
```

---

### Task 2: `aggregator` config — `daily_stats_retention_days`

**Files:**
- Modify: `crates/aggregator/src/config.rs`

**Interfaces:**
- Produces: `Config.daily_stats_retention_days: Option<i64>`.
- Consumed by: Task 5 (`run_cycle`'s new `prune_daily_stats` call site).

Per this plan's own Open judgment call #1 above: defaults to `None` (no pruning), not a guessed day-count, following the spec's own "unset/no pruning for v1" option rather than the alternative generous-default option — the repo owner can supply a real value via CLI/env once one is decided, with no code change needed.

- [ ] **Step 1: Add the config field**

In `crates/aggregator/src/config.rs`, add after `history_retention_days`:

```rust
    /// How long to keep `line_status_daily_stats` rows before pruning them.
    /// Deliberately `Option`, defaulting to `None` (no pruning at all) --
    /// unlike `history_retention_days`, this rollup exists specifically to
    /// answer "how has this line trended over weeks/months," and the real
    /// retention ceiling is an unresolved product decision, not a technical
    /// one (storage is trivial at daily granularity either way -- see
    /// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md,
    /// Open question 1). Set this via CLI/env once that's decided; until
    /// then rows accumulate indefinitely.
    #[arg(long, env)]
    pub daily_stats_retention_days: Option<i64>,
```

- [ ] **Step 2: Build and run existing config tests**

Run (from repo root): `cargo build -p aggregator && cargo test -p aggregator config`
Expected: builds cleanly; existing config parsing tests (if any) still pass. `Option<i64>` with no `default_value_t` leaves the flag genuinely optional — confirm `cargo run -p aggregator -- --help` (or equivalent `Config::parse()` smoke test) shows it as such, not requiring a value.

- [ ] **Step 3: Commit**

```bash
git add crates/aggregator/src/config.rs
git commit -m "Add daily_stats_retention_days config knob (unset by default)"
```

---

### Task 3: `record_daily_stats` — the aggregator's incremental writer

**Files:**
- Modify: `crates/aggregator/src/queries.rs`

**Interfaces:**
- Produces: `pub async fn record_daily_stats(pool: &PgPool, line_id: &str, day: chrono::NaiveDate, stats: &common::SampleStats) -> Result<()>`, plus a small pure helper `pub fn london_calendar_day(instant: chrono::DateTime<chrono::Utc>) -> chrono::NaiveDate` (new, colocated in this file or `aggregation.rs` — either is fine, but must be a standalone pure function so it's unit-testable without a database, mirroring `next_rail_day_boundary`'s own pure-function shape).
- Consumed by: Task 5 (`run_cycle`'s new call site).

`london_calendar_day` is genuinely new: nothing in `crates/aggregator` currently computes a plain calendar day in Europe/London (only the rail-day-02:00 variant exists, `next_rail_day_boundary`). It must use `chrono-tz`'s `Europe::London` (already a dependency of this crate per the design spec's own research) the same way `next_rail_day_boundary` does, but return the calendar date instead of a rail-day boundary instant — do not derive it from `next_rail_day_boundary`, which answers a different question (staleness cutoff, not "what day is it").

- [ ] **Step 1: Write `london_calendar_day`**

Add to `crates/aggregator/src/queries.rs` (or `aggregation.rs`, if that reads more naturally alongside `next_rail_day_boundary` — pick one, keep the import graph simple):

```rust
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Europe::London;

/// The plain Europe/London CALENDAR day (midnight-to-midnight) `instant`
/// falls on -- matching `frontend/lib/dateFormat.ts`'s `londonDayKey`, the
/// convention the Timeline tab already groups by. Deliberately NOT
/// `next_rail_day_boundary`'s rail-day 02:00 cutoff, a different boundary
/// used elsewhere in this crate for incident staleness -- see
/// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md, Open
/// question 5, for why these two conventions coexist.
pub fn london_calendar_day(instant: DateTime<Utc>) -> NaiveDate {
    instant.with_timezone(&London).date_naive()
}
```

- [ ] **Step 2: Write `record_daily_stats`**

Add to `crates/aggregator/src/queries.rs`, following the design spec's own sketch (Decision 1) but as the authoritative implementation:

```rust
/// Upserts one line's contribution to its `(line_id, day)` daily rollup row
/// for this cycle. Called from `run_cycle` (`main.rs`) AT MOST ONCE per
/// line per cycle -- see this plan's Global Constraints for why calling it
/// once per `LineStatus` entry instead would double-count.
///
/// The rate this feeds is a share of SAMPLED POLL CYCLES, not distinct
/// trains -- `stats.total` counts departures currently visible in one
/// poll's LDBWS window, and the same physical service is very likely
/// counted across many consecutive polls. Accepted for v1; see Decision 2.
pub async fn record_daily_stats(
    pool: &PgPool,
    line_id: &str,
    day: chrono::NaiveDate,
    stats: &common::SampleStats,
) -> Result<()> {
    let running = stats.total.saturating_sub(stats.cancelled) as i64;
    sqlx::query(
        "INSERT INTO line_status_daily_stats
            (line_id, day, sample_cycles, total, delayed, cancelled, skipped,
             running_count, delay_minutes_sum)
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (line_id, day) DO UPDATE SET
            sample_cycles     = line_status_daily_stats.sample_cycles + 1,
            total             = line_status_daily_stats.total + EXCLUDED.total,
            delayed           = line_status_daily_stats.delayed + EXCLUDED.delayed,
            cancelled         = line_status_daily_stats.cancelled + EXCLUDED.cancelled,
            skipped           = line_status_daily_stats.skipped + EXCLUDED.skipped,
            running_count     = line_status_daily_stats.running_count + EXCLUDED.running_count,
            delay_minutes_sum = line_status_daily_stats.delay_minutes_sum + EXCLUDED.delay_minutes_sum",
    )
    .bind(line_id)
    .bind(day)
    .bind(stats.total as i64)
    .bind(stats.delayed as i64)
    .bind(stats.cancelled as i64)
    .bind(stats.skipped as i64)
    .bind(running)
    .bind(stats.avg_delay_minutes * running as f64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mirrors `prune_history`'s shape. Only called from `run_cycle` when
/// `daily_stats_retention_days` is `Some` -- see `config.rs`.
pub async fn prune_daily_stats(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM line_status_daily_stats WHERE day < (CURRENT_DATE - $1::int)",
    )
    .bind(retention_days as i32)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 3: Write tests**

Add to `crates/aggregator/src/queries.rs`'s existing `#[cfg(test)] mod tests`:

- `london_calendar_day` (pure, no DB): a UTC instant just before/after both London midnight and a DST transition maps to the expected `NaiveDate` — mirroring the existing `next_rail_day_boundary_*` test style in `aggregation.rs` (same DST-transition rigor, different boundary).
- `record_daily_stats` (`#[ignore]`d, live DB, matching this file's existing pattern): two calls for the same `(line_id, day)` in the same cycle-equivalent sum correctly (`sample_cycles` becomes `2`, sums add); a call for a new `day` starts a fresh row rather than accumulating into the previous day's; `delay_minutes_sum`/`running_count` recover the original `avg_delay_minutes` via division within floating-point tolerance.
- `prune_daily_stats` (`#[ignore]`d, live DB): a row older than the retention window is deleted; a row within it is kept — mirroring `prune_history`'s own existing test shape in this file.

- [ ] **Step 4: Run tests**

Run (from repo root): `cargo test -p aggregator` (fast tests) and, if a local database is available, `cargo test -p aggregator -- --ignored` (DB tests).
Expected: all pass, including the new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/aggregator/src/queries.rs
git commit -m "Add record_daily_stats/prune_daily_stats and a London-calendar-day helper"
```

---

### Task 4: `daily_stats_for_range` + `GET /Line/{id}/Stats/{from}/to/{to}`

**Files:**
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/line_status.rs`

**Interfaces:**
- Produces: `pub struct DailyStatsRow { day: chrono::NaiveDate, sample_cycles: i64, total: i64, delayed: i64, cancelled: i64, skipped: i64, running_count: i64, delay_minutes_sum: f64 }`, `pub async fn daily_stats_for_range(pool: &PgPool, line_id: &str, from: chrono::NaiveDate, to: chrono::NaiveDate) -> Result<Vec<DailyStatsRow>>`; a new route `GET /Line/{id}/Stats/{from}/to/{to}` returning the camelCase JSON shape from the spec's API contract section (`day`, `sampleCycles`, `total`, `delayed`, `cancelled`, `skipped`, `avgDelayMinutes`, `delayRate`, `cancellationRate`, `skipRate`).
- Consumed by: Task 6 (`frontend/lib/api.ts`'s `getLineDailyStats`).

Per this plan's own Open judgment call #4: `from`/`to` are `NaiveDate` path segments (not `DateTime<Utc>`, unlike the sibling history route), matching this new table's own `DATE` column directly. Response is **not** run through `to_tfl_shape` — this route has no TfL analog to mimic, unlike the other four routes in this file.

- [ ] **Step 1: Write `DailyStatsRow` and `daily_stats_for_range`**

Add to `crates/api/src/data/queries.rs`, after `line_status_history_for_range`:

```rust
pub struct DailyStatsRow {
    pub day: chrono::NaiveDate,
    pub sample_cycles: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

/// Reads the `line_status_daily_stats` rollup for one line over
/// `[from, to]` (inclusive both ends, matching the DATE column's own
/// semantics -- unlike the sibling `line_status_history_for_range`'s
/// timestamp `BETWEEN`, there is no time-of-day component to reason
/// about). Returns an empty vec for an unknown `line_id` -- no error, no
/// special-casing -- matching `line_status_history_for_range`'s existing
/// behavior for the same case (see
/// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md,
/// Error handling).
pub async fn daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_daily_stats
         WHERE line_id = $1 AND day BETWEEN $2 AND $3
         ORDER BY day",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DailyStatsRow {
                day: row.try_get("day")?,
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

- [ ] **Step 2: Add the route**

In `crates/api/src/routes/line_status.rs`, add to `router()`:

```rust
        .route("/Line/{id}/Stats/{from}/to/{to}", axum::routing::get(get_line_daily_stats))
```

Add the handler and its pure rate-derivation helper (kept pure and separately testable, matching this file's existing convention of pure helpers like `tfl_ids_to_overlay`/`overlay_for`):

```rust
/// Derives the four rate/average fields from one stored rollup row,
/// guarding every division against a zero denominator -- a day CAN have
/// total: 0 if every contributing cycle itself had total: 0 (rare given
/// min_sample_size, not impossible). Pure so it's unit-testable without a
/// database.
fn daily_stats_to_json(row: queries::DailyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| if row.total > 0 { numerator as f64 / row.total as f64 } else { 0.0 };

    serde_json::json!({
        "day": row.day,
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

async fn get_line_daily_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::daily_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}
```

- [ ] **Step 3: Write tests**

- `crates/api/src/data/queries.rs`: `daily_stats_for_range` (`#[ignore]`d, live DB) — range filtering (a row outside `[from, to]` excluded), ordering (ascending by `day`), unknown `line_id` returns empty vec.
- `crates/api/src/routes/line_status.rs`: unit tests for `daily_stats_to_json`'s rate math (a normal row; a `total: 0` row produces `0.0` for every rate field and `avgDelayMinutes`, never `NaN`/`Infinity` — assert this by parsing the resulting `Value` back and checking `is_finite()` or exact equality to `0.0`, not just "doesn't panic"). A route-level test (mirroring this file's existing pattern for constructing fixture rows) confirming the URL parses `NaiveDate` path segments correctly and the route mounts.

- [ ] **Step 4: Run tests and build**

Run (from repo root): `cargo test -p api` (fast tests), `cargo test -p api -- --ignored` (DB tests, if available), `cargo build --workspace`.
Expected: all pass; workspace builds cleanly.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/line_status.rs
git commit -m "Add GET /Line/{id}/Stats/{from}/to/{to} over the new daily rollup"
```

---

### Task 5: Wire the aggregator's cycle loop to `record_daily_stats`/`prune_daily_stats`

**Files:**
- Modify: `crates/aggregator/src/main.rs`

**Interfaces:**
- Produces: `run_cycle` calling `queries::record_daily_stats` once per line per cycle (skipping lines with no `SampleStats` this cycle) and, when `daily_stats_retention_days` is `Some`, `queries::prune_daily_stats`.
- Consumed by: nothing further in-process; this is the terminal write path the whole feature depends on to have any data at all.

This is the task most at risk of the double-counting failure mode called out in Global Constraints — read `aggregate()`'s Layer 2 (`aggregation.rs:82-107`) and `infer_from_samples` (`aggregation.rs:715-741`) again before writing this task's code: every status on a `LineStatusReport` carries an identical `Option<SampleStats>` clone when `Some`, so reading `report.statuses.first()` is sufficient and correct — do not loop `report.statuses` and call `record_daily_stats` per entry.

- [ ] **Step 1: Extend `run_cycle`'s signature and body**

In `crates/aggregator/src/main.rs`, change `run_cycle`'s signature to accept the new retention option, and thread it through from `main`'s call site (`config.daily_stats_retention_days`):

```rust
async fn run_cycle(
    pool: &sqlx::PgPool,
    static_lines: &HashMap<String, LineDefinition>,
    defaults: &Defaults,
    retention_days: i64,
    daily_stats_retention_days: Option<i64>,
) -> anyhow::Result<()> {
```

After the existing `for report in reports.values() { queries::write_line_status(pool, report).await?; }` loop, add a second loop over the same `reports` (kept separate from the write-status loop rather than merged into it, so a future change to one doesn't accidentally couple to the other):

```rust
    let today = queries::london_calendar_day(Utc::now());
    let mut daily_stats_recorded = 0u64;
    for report in reports.values() {
        // Every status on a report carries an identical SampleStats clone
        // when Some (aggregation.rs's Layer 2 and infer_from_samples both
        // set it this way) -- .first() is correct and sufficient; looping
        // report.statuses here would double-count for any line with
        // multiple concurrent incidents. See this plan's Global
        // Constraints and Task 3's own doc comment on record_daily_stats.
        let Some(stats) = report.statuses.first().and_then(|s| s.sample_stats.as_ref()) else {
            continue;
        };
        queries::record_daily_stats(pool, &report.id, today, stats).await?;
        daily_stats_recorded += 1;
    }

    let daily_stats_pruned = if let Some(retention) = daily_stats_retention_days {
        queries::prune_daily_stats(pool, retention).await?
    } else {
        0
    };
```

Add `chrono::Utc` to this file's imports if not already present. Extend the existing `tracing::info!`/metrics block at the end of `run_cycle` to include `daily_stats_recorded`/`daily_stats_pruned`, matching the existing style for `pruned_history_rows` (a `metrics::counter!` for pruned rows, a field on the `tracing::info!` call — do not invent a new logging shape).

- [ ] **Step 2: Update the call site in `main`**

Change:

```rust
        let result = run_cycle(&pool, &static_lines, &defaults, config.history_retention_days).await;
```

to:

```rust
        let result = run_cycle(
            &pool,
            &static_lines,
            &defaults,
            config.history_retention_days,
            config.daily_stats_retention_days,
        )
        .await;
```

- [ ] **Step 3: Write a test for the once-per-line-not-once-per-status invariant**

Add a test (in `main.rs` if it already has a `#[cfg(test)]` module, otherwise add one, or place this alongside `record_daily_stats`'s own tests in `queries.rs` if that reads more naturally — either location is fine as long as it exists) that: builds a `LineStatusReport` with two `LineStatus` entries that both carry the identical `Some(SampleStats { total: 10, .. })` (simulating a line with two concurrent incidents), runs the extraction logic this task adds, and asserts `record_daily_stats` (or whatever it's actually wired through) is invoked/would be invoked with `total: 10` — not `20`. If `run_cycle` itself is awkward to unit test directly (it takes a live pool), extract the `reports.values() -> Vec<(line_id, SampleStats)>` selection logic in Step 1 into its own small pure function first and test that in isolation — cleaner than trying to mock the database for this specific invariant.

- [ ] **Step 4: Run tests and build**

Run (from repo root): `cargo test -p aggregator && cargo build --workspace`.
Expected: all pass; workspace builds.

- [ ] **Step 5: Commit**

```bash
git add crates/aggregator/src/main.rs
git commit -m "Call record_daily_stats/prune_daily_stats once per line per aggregation cycle"
```

---

### Task 6: Frontend types + API client — `LineDailyStats`, `getLineDailyStats`

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `LineDailyStats` (types.ts), `getLineDailyStats(id: string, from: string, to: string): Promise<LineDailyStats[]>` (api.ts).
- Consumed by: Task 9 (`TrendsResults`).

Same shape as the existing `getLineStatusHistory`: public, server-side-only fetch, no cookie forwarding, no proxy — copied directly from the design spec's own hand-written API/type contract section, not re-derived.

- [ ] **Step 1: Add the type**

Add to `frontend/lib/types.ts`, after `LineStatusHistoryEntry`:

```ts
/** `GET /Line/{id}/Stats/{from}/to/{to}`'s per-day response shape.
 * `delayRate`/`cancellationRate`/`skipRate` are fractions (0-1), computed
 * server-side from stored sums -- never "% of trains", see
 * docs/superpowers/specs/2026-08-31-line-history-graphics-design.md
 * Decision 2. `sampleCycles` is the coverage signal the sparse-data
 * gap-rendering in `TrendsResults.tsx` depends on -- render it, don't
 * discard it. */
export interface LineDailyStats {
  day: string; // "YYYY-MM-DD", Europe/London calendar day
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

- [ ] **Step 2: Add `getLineDailyStats`**

Add `LineDailyStats` to `frontend/lib/api.ts`'s existing `import type { ... } from './types';` list, then add after `getLineStatusHistory`:

```ts
/** `GET /Line/{id}/Stats/{from}/to/{to}` -- the new daily rollup route.
 * `from`/`to` are `YYYY-MM-DD` calendar days (the route's own path segments
 * are `NaiveDate`, not RFC3339 instants -- see the backend plan's Task 4).
 * Same public, no-store, no-cookie-forwarding shape as
 * `getLineStatusHistory`. */
export async function getLineDailyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineDailyStats[]> {
  return fetchJson<LineDailyStats[]>(
    `${baseUrl()}/Line/${id}/Stats/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}
```

- [ ] **Step 3: Add tests**

Add to `frontend/lib/api.test.ts`, mirroring the existing `getLineStatusHistory` test(s): confirms the correct URL is built from `id`/`from`/`to`, `cache: 'no-store'` is set, and a `200` with an array resolves it as-is.

- [ ] **Step 4: Run tests and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both pass — additive, unused by anything else yet.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add LineDailyStats type and getLineDailyStats client"
```

---

### Task 7: Dependency bump — `@mantine/core|dates|hooks` to `9.5.2`, `react`/`react-dom` to `^19.2.0`, add `@mantine/charts` + `recharts`

**Files:**
- Modify: `frontend/package.json`
- Modify: `frontend/package-lock.json` (regenerated by the install, not hand-edited)

**Interfaces:**
- Produces: an installable, buildable, test-passing frontend at the new dependency versions, with `@mantine/charts`/`recharts` available for Task 9 to import.
- Consumed by: Task 9 (`TrendsResults`, the only consumer of the new charting packages).

This is explicit, sequenced, real work — not a drop-in — per the design spec's own Decision 6: `@mantine/charts@9.5.2` pins its `@mantine/core`/`@mantine/hooks` peers to the exact same release (`9.5.2`, not a range) and requires `react`/`react-dom >=19.2.0`. This repo is currently on `^9.4.1`/`^19.0.0`. Do this bump as its own isolated task, before any chart code is written, so a break surfaces here and not muddled together with new feature code.

- [ ] **Step 1: Bump existing Mantine packages and React**

In `frontend/package.json`'s `dependencies`:

```json
    "@mantine/core": "9.5.2",
    "@mantine/dates": "9.5.2",
    "@mantine/hooks": "9.5.2",
    "react": "^19.2.0",
    "react-dom": "^19.2.0",
```

(Pinned exact `9.5.2` for the three Mantine packages, not `^9.5.2` — matching `@mantine/charts`' own peer-dependency pin exactly, since Mantine ships its packages in lockstep and a caret here could let `npm install` later drift `@mantine/core` away from whatever `@mantine/charts` needs.)

- [ ] **Step 2: Add `@mantine/charts` and `recharts`**

In `frontend/package.json`'s `dependencies`, add:

```json
    "@mantine/charts": "9.5.2",
    "recharts": "^3.2.1",
```

- [ ] **Step 3: Install and resolve**

Run (from `frontend/`): `npm install`
Expected: resolves cleanly with no `--force`/`--legacy-peer-deps` needed. If a peer-dependency conflict does surface (e.g. from `@testing-library/react@^16.0.0` or another dependency pinned against React 19.0), resolve it by identifying and bumping the specific conflicting package to a version compatible with React `^19.2.0` — do not reach for `--legacy-peer-deps` as a way to paper over an unresolved conflict; a persisted flag would hide any future genuine incompatibility.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both pass with zero code changes elsewhere — this task's whole job is confirming the bump alone doesn't break anything before Task 9 adds new code on top of it. `frontend/vitest.setup.ts` already polyfills `window.ResizeObserver` (needed by Mantine's own components today); per the design spec's own verification, this is also what Recharts' `ResponsiveContainer` needs, so no new polyfill is expected to be necessary — if `npm test` after Task 9 (not this task) reveals otherwise, that fix belongs in Task 9, not here.

- [ ] **Step 5: Commit**

```bash
git add frontend/package.json frontend/package-lock.json
git commit -m "Bump Mantine to 9.5.2, React to ^19.2.0, add @mantine/charts + recharts"
```

---

### Task 8: `Tabs` split on the history page — Timeline / Trends, plus the retention-ceiling surfacing fix

**Files:**
- Modify: `frontend/app/lines/[id]/history/page.tsx`
- Modify: `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`
- Modify: `frontend/app/lines/[id]/history/HistoryRangePicker.test.tsx`

**Interfaces:**
- Produces: `page.tsx` rendering a Mantine `Tabs` ("Timeline" / "Trends") beneath the existing, unmodified-in-behavior `HistoryRangePicker`, with the existing `HistoryResults` moved under the Timeline panel and a new `TrendsResults` (Task 9) under the Trends panel. `HistoryRangePicker` gains an optional `retentionCeilingDays` prop (used to grey/disable the "Last 30 days" preset with an explanatory tooltip when the active tab's data source can't actually return that far back).
- Consumed by: nothing further; this is the page-level integration point.

Per Decision 5 and this plan's Global Constraints: the *only* change to the Timeline tab's own behavior is surfacing its existing 7-day truncation via a disabled/tooltipped preset button when Timeline is the active tab — `history_retention_days` itself, and what the Timeline tab can return, are unchanged. This task does not implement `TrendsResults` itself (Task 9) — it wires the tab shell and passes it a `Suspense` boundary, matching how `HistoryResults` is already wrapped.

- [ ] **Step 1: Restructure `page.tsx` around a `Tabs`**

In `frontend/app/lines/[id]/history/page.tsx`, add a `tab` query param to `searchParams`'s type (default `'timeline'`), import `Tabs` from `@mantine/core` and the new `TrendsResults` (Task 9's file), and wrap the existing `<Suspense>...<HistoryResults ... /></Suspense>` block in `<Tabs defaultValue="timeline">` with two panels:

```tsx
<Tabs defaultValue="timeline">
  <Tabs.List>
    <Tabs.Tab value="timeline">Timeline</Tabs.Tab>
    <Tabs.Tab value="trends">Trends</Tabs.Tab>
  </Tabs.List>
  <Tabs.Panel value="timeline">
    <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={240} />}>
      <HistoryResults id={id} from={range.from} to={range.to} />
    </Suspense>
  </Tabs.Panel>
  <Tabs.Panel value="trends">
    <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
      <TrendsResults id={id} from={range.from} to={range.to} />
    </Suspense>
  </Tabs.Panel>
</Tabs>
```

Keep the existing `HistoryRangePicker` render above the `Tabs`, unchanged in position, per Decision 5 (one page, one URL, one range picker — this plan does not fork range state per tab). Whether the active tab itself is tracked in the URL (a `?tab=` param, mirroring `from`/`to`/`range`'s own URL-is-source-of-truth convention) or left as uncontrolled `Tabs` state (`defaultValue` only) is this task's own implementation choice — prefer the URL-tracked version for consistency with this page's existing convention if it doesn't meaningfully complicate the diff, but an uncontrolled `Tabs` is an acceptable, simpler fallback if it does; either way, `HistoryRangePicker`'s existing from/to/preset URL-state behavior must not regress.

- [ ] **Step 2: Extend `HistoryRangePicker` with a retention-ceiling prop**

Add an optional prop and disable/tooltip the "Last 30 days" button when it would exceed the ceiling:

```tsx
export function HistoryRangePicker({
  lineId,
  preset,
  from,
  to,
  retentionCeilingDays,
}: {
  lineId: string;
  preset: RangePreset | null;
  from: string;
  to: string;
  /** If set, and less than 30, the "Last 30 days" preset is disabled with a
   * tooltip naming the real limit -- per Decision 5's recommendation to
   * surface (not fix) the Timeline tab's pre-existing 7-day
   * `history_retention_days` truncation now that Trends sits next to it in
   * one Tabs control with a genuinely longer retention window. */
  retentionCeilingDays?: number;
}) {
```

Wrap the existing 30-day `<Button {...presetProps('30d')}>Last 30 days</Button>` in a Mantine `Tooltip` and pass `disabled` when `retentionCeilingDays !== undefined && retentionCeilingDays < 30`, with label text like `` `Only the last ${retentionCeilingDays} days of this tab's data is kept` ``. `page.tsx` passes `retentionCeilingDays={7}` only when rendering the Timeline tab's context (or omit it entirely for the Trends tab / when the active tab is Trends) — since `HistoryRangePicker` is rendered once above the `Tabs`, not per-panel, this task's concrete approach is either (a) pass the ceiling for whichever tab is currently active (requires knowing the active tab in `page.tsx`, which Step 1's URL-tracked option gives for free), or (b) keep it simple and always pass `7` unconditionally with copy that reads correctly regardless of which tab is showing (e.g. "Timeline data" specifically, not "this tab's data") — pick whichever fits more cleanly with Step 1's actual tab-tracking choice; both satisfy Decision 5's intent.

- [ ] **Step 3: Update `HistoryRangePicker` tests**

Add to `frontend/app/lines/[id]/history/HistoryRangePicker.test.tsx`: a test that `retentionCeilingDays={7}` disables the "Last 30 days" button and a test that omitting the prop (or passing `30`) leaves it enabled, matching this file's existing test style.

- [ ] **Step 4: Run tests and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both pass. `TrendsResults` doesn't exist yet at this point if Task 9 hasn't run — if executing strictly in order, add a minimal placeholder (`export async function TrendsResults() { return null; }`) in a throwaway or Task 9's file now, to be replaced when Task 9 runs; same ordering pattern the journey-ticket-tracking-frontend plan used for `TicketEntryForm`/`TicketPanel`.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/\[id\]/history/page.tsx frontend/app/lines/\[id\]/history/HistoryRangePicker.tsx frontend/app/lines/\[id\]/history/HistoryRangePicker.test.tsx
git commit -m "Split history page into Timeline/Trends tabs; surface the Timeline tab's 7-day retention ceiling"
```

---

### Task 9: `TrendsResults` — fetch + two `@mantine/charts` `LineChart`s, empty/gap states

**Files:**
- Create: `frontend/app/lines/[id]/history/TrendsResults.tsx`
- Create: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`

**Interfaces:**
- Consumes: `getLineDailyStats` (Task 6), `@mantine/charts`' `LineChart` (Task 7).
- Produces: `async function TrendsResults({ id, from, to }: { id: string; from: string; to: string })`.
- Consumed by: Task 8 (`page.tsx`'s Trends `Tabs.Panel`).

This is the task that actually implements Decision 3's gap-rendering and Decision 7's copy rules — the most product-sensitive part of this plan. Read Decisions 3, 6, and 7 of the design spec again immediately before writing this component.

- [ ] **Step 1: Write the sparse-data floor constant and gap-transform helper**

At the top of `TrendsResults.tsx`:

```tsx
// Placeholder, not a validated number -- see this plan's own "Open
// judgment calls" section and
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md Open
// question 3. Revisit against real sample_cycles distributions once this
// has been running in production for a while.
const SPARSE_DATA_FLOOR_CYCLES = 20;
```

Write a pure helper (exported for testing) that maps `LineDailyStats[]` to chart-ready points, turning any day with `sampleCycles < SPARSE_DATA_FLOOR_CYCLES` into a gap — for Recharts/`@mantine/charts`, a gap is a data point with the metric fields set to `null`/`undefined` rather than `0` (Recharts' own documented way to break a line rather than draw a misleading dip to zero — verify this against `@mantine/charts`' real docs/an actual rendered test before relying on it, don't assume from memory, consistent with the design spec's own instruction not to guess library APIs):

```tsx
interface ChartPoint {
  day: string;
  delayRate: number | null;
  cancellationRate: number | null;
  skipRate: number | null;
  avgDelayMinutes: number | null;
  sampleCycles: number;
}

export function toChartPoints(stats: LineDailyStats[]): ChartPoint[] {
  return stats.map((row) => {
    const sparse = row.sampleCycles < SPARSE_DATA_FLOOR_CYCLES;
    return {
      day: row.day,
      delayRate: sparse ? null : row.delayRate,
      cancellationRate: sparse ? null : row.cancellationRate,
      skipRate: sparse ? null : row.skipRate,
      avgDelayMinutes: sparse ? null : row.avgDelayMinutes,
      sampleCycles: row.sampleCycles,
    };
  });
}
```

- [ ] **Step 2: Write the component**

```tsx
import { LineChart } from '@mantine/charts';
import { Stack, Text, Title } from '@mantine/core';
import { getLineDailyStats } from '@/lib/api';

// ... SPARSE_DATA_FLOOR_CYCLES, ChartPoint, toChartPoints from Step 1 ...

export async function TrendsResults({ id, from, to }: { id: string; from: string; to: string }) {
  const stats = await getLineDailyStats(id, from.slice(0, 10), to.slice(0, 10));

  if (stats.length === 0) {
    return <Text c="dimmed">Not enough sampled data yet for this line.</Text>;
  }

  const points = toChartPoints(stats);

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        Rates shown are the share of sampled poll cycles that looked delayed, cancelled, or skipping a stop --
        not a share of individual trains. Each point is based on that day&apos;s sample_cycles poll samples;
        days with too little coverage show as a gap rather than a misleading flat line.
      </Text>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Delay / cancellation / skip rate
        </Title>
        <LineChart
          h={280}
          data={points}
          dataKey="day"
          series={[
            { name: 'delayRate', label: 'Delay rate', color: 'blue.6' },
            { name: 'cancellationRate', label: 'Cancellation rate', color: 'red.6' },
            { name: 'skipRate', label: 'Skip rate', color: 'yellow.6' },
          ]}
          valueFormatter={(value) => `${(value * 100).toFixed(1)}%`}
          connectNulls={false}
        />
      </Stack>
      <Stack gap={4}>
        <Title order={4} size="h6">
          Average delay (minutes)
        </Title>
        <LineChart
          h={220}
          data={points}
          dataKey="day"
          series={[{ name: 'avgDelayMinutes', label: 'Avg delay (minutes)', color: 'grape.6' }]}
          connectNulls={false}
        />
      </Stack>
    </Stack>
  );
}
```

Before finalizing this step, confirm against `@mantine/charts@9.5.2`'s real, current docs (fetch them, don't recall from memory) that: (a) the `series` item shape is `{ name, label?, color? }` (not `dataKey`, which was this component's design-spec sketch's naming, or something else), (b) `connectNulls={false}` (or an equivalent prop) is the real, documented way to render a gap instead of interpolating across a `null` value, and (c) `valueFormatter` is the real prop name for a per-chart y-axis/tooltip value formatter. Adjust the code above to match whatever the real API turns out to be — this step's code block is this plan's best-effort sketch, not verified against the live library, exactly like the design spec's own sketches are marked throughout.

- [ ] **Step 3: Write tests**

Create `frontend/app/lines/[id]/history/TrendsResults.test.tsx`, mocking `@/lib/api`'s `getLineDailyStats` and calling the async component directly (`renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }))`), matching the new async-Server-Component test technique this repo's `TicketPanel.test.tsx` already established:

- Empty array: renders "Not enough sampled data yet for this line."
- A day below `SPARSE_DATA_FLOOR_CYCLES`: `toChartPoints` (tested directly, no render needed) turns its rate/avg fields to `null` while preserving `sampleCycles`; a render test confirms the page doesn't throw and doesn't render a flat-zero-looking chart for that day (assert on the underlying data passed to `LineChart`, e.g. via a light mock of `@mantine/charts` if full Recharts rendering in jsdom proves awkward — consistent with this repo's stated "no chart pixel output assertions" convention).
- A normal multi-day range: renders without throwing, and the "share of sampled poll cycles" copy (Decision 7) is present verbatim.
- Confirm avg-delay-minutes chart is a visually/structurally separate `LineChart` from the three-rate chart (e.g. two `LineChart` mocks/instances rendered, not one four-series chart) — the concrete assertion technique for this depends on Step 2's final resolved API shape; adjust once that's known.

- [ ] **Step 4: Run tests and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both pass. If Task 8 used a placeholder `TrendsResults`, delete it now.

- [ ] **Step 5: Commit**

```bash
git add "frontend/app/lines/[id]/history/TrendsResults.tsx" "frontend/app/lines/[id]/history/TrendsResults.test.tsx"
git commit -m "Add TrendsResults: rate/avg-delay charts with sparse-data gap and empty-state handling"
```

---

### Task 10: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Full backend build and test**

Run (from repo root): `cargo build --workspace && cargo test --workspace`
Expected: both pass. Run `cargo test --workspace -- --ignored` too if a local database is available, to cover every `#[ignore]`d integration test added in Tasks 3 and 4.

- [ ] **Step 2: Full frontend build and test**

Run (from `frontend/`): `npm test && npm run build`
Expected: both pass, no regressions to any existing page/component.

- [ ] **Step 3: Re-confirm the once-per-line invariant by reading the final `run_cycle` code, not just trusting Task 5's test**

Re-read the final state of `crates/aggregator/src/main.rs`'s `run_cycle` and confirm the `record_daily_stats` call site iterates `reports.values()` (one entry per line) and reads `report.statuses.first()`, never `report.statuses.iter()` calling `record_daily_stats` per status.

- [ ] **Step 4: Re-confirm no DLR/per-service-dedup scope creep, by grep**

```bash
grep -rn "record_daily_stats" crates/
```

Expected: call sites only in `crates/aggregator/src/main.rs` (Task 5) and the definition in `crates/aggregator/src/queries.rs` (Task 3) — nothing in `crates/api/src/data/queries.rs`'s `upsert_tfl_line_status` or anywhere in `crates/poller-tfl`.

```bash
grep -rn "service_id" crates/aggregator/ frontend/
```

Expected: no new matches introduced by this plan — per-service dedup is genuinely separate, parallel work, not something this plan's tasks should reference in new code.

- [ ] **Step 5: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree.

---

## Explicitly out of scope for this plan (carried forward from the spec, not resolved here)

- **True per-service deduplication of the cycle-weighted rate** (Decision 2 / spec's own "Explicitly out of scope"). A different subagent is handling this as separate, parallel work right now — no task above references `StationDeparture.service_id` or any "seen today" ledger.
- **DLR/TfL's separate `sample_stats` pipeline** (Correction 3). Extending `record_daily_stats` to `upsert_tfl_line_status`/`poller-tfl` is real, symmetrical, future work, not built here.
- **Actually lengthening `history_retention_days`** or otherwise changing what the Timeline tab itself can show beyond 7 days — Task 8 only *surfaces* the existing ceiling (a disabled preset + tooltip), matching Decision 5's own scope boundary.
- **Hourly (or finer) granularity**, cross-line comparison charts, CSV/image export, or alerting on trend changes — none designed in the spec, none built here.
- **Backfilling historical data from before this feature ships** — per Correction 1, no raw archive exists to backfill from; every line's rollup starts from zero on deployment day.
- **A `sample_stations` catalogue coverage audit** across the ~105 line TOML files (spec Open question 2) — not performed by this plan; the empty/sparse-state UI is built to degrade honestly regardless.
- **Validating the `SPARSE_DATA_FLOOR_CYCLES = 20` placeholder** against real production `sample_cycles` distributions (spec Open question 3) — flagged in Task 9 as a placeholder needing a later, data-informed revisit, not resolved by this plan.
- **Deciding `daily_stats_retention_days`'s real default** (spec Open question 1) — Task 2 ships it `None` (no pruning); a real default is a product decision for the repo owner, not this plan.

## Self-review notes

- **Spec coverage:** the new table and its calendar-day convention (Task 1, Decision 1/Open question 5), the aggregator write path and its once-per-line invariant (Tasks 3/5, Decision 1's own double-counting warning), the retention knob's honestly-unresolved default (Task 2, Open question 1), the read route and its read-time rate derivation with divide-by-zero guards (Task 4, Decision 4, Error handling), the dependency bump as its own isolated, verifiable step (Task 7, Decision 6), the Tabs UI plus the adjacent retention-ceiling surfacing fix (Task 8, Decision 5), and the sparse-data gap/empty-state rendering plus honest "per sampled poll cycle" copy (Task 9, Decisions 3/6/7) are each covered by exactly one task above.
- **Sequencing choice:** backend (Tasks 1–5) fully precedes frontend (Tasks 6–9) so that by the time chart code is written, real data can actually flow end-to-end through a local stack for manual sanity-checking — the journey-ticket-tracking-frontend precedent didn't need this ordering (its backend was already merged), but this feature's backend genuinely does not exist yet (see this plan's own Status note), so the ordering is a deliberate difference, not an oversight.
- **Judgment calls this plan made that the spec left open** are collected in one place near the top ("Open judgment calls made when sequencing this plan") rather than scattered per-task, so a reviewer can find and re-litigate them without hunting through ten tasks.
- **Type/interface consistency check:** `LineDailyStats` (Task 6) field names match the backend JSON shape produced by `daily_stats_to_json` (Task 4) exactly (`sampleCycles`, `avgDelayMinutes`, `delayRate`, `cancellationRate`, `skipRate`). `getLineDailyStats`'s signature (Task 6) is called with matching argument order/types in `TrendsResults` (Task 9). `DailyStatsRow` (Task 4) field names match what `record_daily_stats` (Task 3) writes into the table (Task 1) exactly (`sample_cycles`, `running_count`, `delay_minutes_sum`, etc.) — no silent renaming across the Rust/SQL/TS boundary anywhere in this plan.
