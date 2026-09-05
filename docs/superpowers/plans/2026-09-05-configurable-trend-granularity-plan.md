# Configurable Trend Granularity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** let a viewer of `/lines/[id]/history`'s Trends tab pick one of
four bucket granularities (30-minute, 1-hour, 6-hour, daily) for the
delay/cancellation/skip-rate and average-delay charts, instead of always
rendering one point per calendar day — implementing the approved spec's
Decisions 1–8 exactly, no re-litigation.

**What changed since the spec was written**: the spec's Open question 1
flagged the proposed 840-hour (35-day) `half_hourly_stats_retention_hours`
default as the single biggest open risk, pending confirmation that it stays
under the RDM Live Departure Board licence's 365-day ceiling. **The repo
owner has now confirmed this directly: "half-hourly is still fine as long
as we aren't retaining for more than 300 days."** The proposed 840-hour
(35-day) default clears that bar with enormous margin. This plan treats
Decision 3 as approved and unblocked, and does not re-flag it as an open
legal question anywhere below.

**Architecture:** the 30-minute and daily tiers reuse the existing
`GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` and
`GET /Line/{id}/Stats/{from}/to/{to}` routes verbatim (Task 1–2 add nothing
for them). The two new intermediate tiers (1-hour, 6-hour) are served by one
new query function, `sub_daily_stats_for_range`, that groups the existing
`line_status_half_hourly_stats` rows at read time via Postgres's `date_bin`
— no new table, no new aggregator write path (Task 1), exposed via two new
thin routes (Task 2). `crates/api/src/routes/history_retention.rs` grows two
new fields so the frontend can compute, honestly, which tiers a given date
range can actually support (Task 3). `crates/aggregator/src/config.rs`'s
`half_hourly_stats_retention_hours` default bumps from 48 to 840 (Task 4).
On the frontend, `TrendsResults.tsx` is rewritten to branch internally on a
`granularity` parameter (reversing the granularity design's Decision 10,
per this spec's own Decision 4) rather than forking a third/fourth
sibling component; a new `GranularityControl.tsx` renders the picker
scoped to the Trends panel only; `frontend/lib/history.ts` grows the
`resolveGranularity`/`availableGranularities`/`granularityShortfallDays`
logic that decides which tiers are offered and whether the current
selection needs an honesty banner (Tasks 5–10).

**Tech Stack:** Rust (axum, sqlx, Postgres 16), Next.js/React Server
Components, Mantine, Vitest.

**Spec:**
`docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md`
— its Decisions 1–8 are authoritative for every architectural choice below;
this plan does not re-derive or second-guess them, only turns them into
concrete file-level tasks.

---

## Judgment calls this plan makes (read before Task 1)

The spec left several things as genuine implementation-time calls (its own
Decision 4 / Open questions 3–4). Resolved here, concretely, against the
real current worktree:

1. **`TrendGranularity` shape: four explicit tags
   (`'halfHour' | 'hour' | 'sixHour' | 'day'`), not a collapsed
   `'day' | 'subDaily'` union — and defined ONCE, reused verbatim by both
   `TrendsCharts`'s prop and `TrendsResults`'s internal switch.** The spec's
   own Reuse assessment section floated the two-value collapse as an
   equally-valid option, since `TrendsCharts.tsx`'s only use of the prop
   (`TrendsCharts.tsx:96`, current: `granularity === 'halfHour' ? {
   tickFormatter: ... } : {}`) is a single day-vs-not-day branch. But
   `HalfHourlyTrendsResults.tsx` — explicitly **untouched** by this feature
   (spec Decision 4, Non-goals) — passes the literal
   `granularity="halfHour"` to `TrendsCharts` today
   (`HalfHourlyTrendsResults.tsx:100`). Collapsing `TrendsCharts`'s prop
   type to `'day' | 'subDaily'` would make that literal fail to type-check,
   forcing an edit to a file this plan is explicitly forbidden from
   touching. Four explicit tags avoid that entirely, and avoid a second
   type plus a translation step at every call site for zero behavioral
   gain (the tick-formatter branch is one line either way:
   `granularity !== 'day'`).
2. **`sub_daily_stats_for_range`'s SQL uses `date_bin`, confirmed available.**
   `docker-compose.yml:64` pins `postgres:16` — `date_bin` shipped in
   PostgreSQL 14, so it's available, not merely assumed from the spec's own
   sketch.
3. **The new hourly/six-hourly JSON responses use a `bucketStart` field,
   not `halfHourStart`.** The spec's Decision 2 says the new query "reuses
   the existing row shape verbatim" — true for the Rust
   `HalfHourlyStatsRow` struct (Task 1), which this plan does reuse as-is.
   But that struct's existing JSON serializer,
   `half_hourly_stats_to_json` (`line_status.rs:430-456`), hardcodes the
   JSON key `"halfHourStart"` — reusing it unchanged for a 1-hour or
   6-hour bucket would mislabel the field. Task 2 adds one new sibling
   serializer, `sub_daily_stats_to_json`, identical in every way except the
   JSON key name (`"bucketStart"`). The frontend's new
   `LineHourlyStats`/`LineSixHourlyStats` types (Task 5) use `bucketStart`
   to match.
4. **Spec Open question 5 (should the daily tier ever become
   "unavailable"?) is resolved: no — `day` is never disabled by either the
   retention check or the point-count check.** Disabling the one tier that
   already existed, unguarded, before this feature would be a strict
   regression for a niche case (a custom range wider than 300 days), not a
   fix — and it would risk `availableGranularities` returning an empty
   list, which nothing downstream is built to handle. The existing
   Timeline-tab-style shortfall banner (extended to the Trends tab as
   `granularityShortfallDays`, Task 6) already covers the honesty case for
   an over-wide daily range, same as it always has.
5. **Where the option is "offered" vs. "disabled": an unavailable tier is
   omitted from `GranularityControl`'s Mantine `SegmentedControl` data
   entirely, not rendered disabled.** This follows the one existing
   precedent for a conditional `SegmentedControl` option in this codebase —
   `frontend/components/IssueList.tsx:293-305`'s "Ended" bucket, which is
   "only offered when something is actually in it," rather than a
   `disabled` per-item flag (which nothing in this repo demonstrates
   working with this Mantine version). A dimmed text line beneath the
   control names which tiers are hidden and why, in the same plain-text
   honesty-copy style the existing retention banners already use.

---

## Non-goals

Same as the spec's own Non-goals section, restated because they bound this
plan's file scope:

- No free-text/arbitrary bucket-size input — four named tiers only.
- No change to `HalfHourlyTrendsResults.tsx`/`resolveHalfHourlyRange` (the
  line-info page's fixed rolling-24h widget) or its test file.
- No change to `CoverageTrendsResults.tsx`, `HalfHourlyCoverageTrendsResults.tsx`,
  or anything under the `Coverage` stats routes.
- No change to the Timeline tab, `line_status_history`, or
  `history_retention_days`'s existing behavior/banner.
- No new schema migration, no new aggregator write path, no new
  accumulate-upsert table.
- No backfilling of historical half-hourly rows beyond what the bumped
  retention keeps from the day this ships forward.
- No recalibration of the two existing floor constants
  (`SPARSE_DATA_FLOOR_CYCLES`, `SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`) — only
  the two new tiers' floors are newly derived (Decision 5, values below).
- No change to `HistoryRangePicker.tsx` — the granularity control lives only
  inside the Trends `TabsPanel` (Decision 6).
- No legal/compliance re-litigation of the retention bump — approved per
  this document's header note.

## Global Constraints

- **Reuse the daily and half-hourly routes/queries verbatim.**
  `daily_stats_for_range`, `half_hourly_stats_for_range`,
  `record_daily_stats`, `record_half_hourly_stats`, and every aggregator
  write path stay untouched.
- **No new migration, no new table.** `sub_daily_stats_for_range` (Task 1)
  reads `line_status_half_hourly_stats` only.
- **`bucket_minutes` is never taken from raw request input.** It is always
  one of exactly two Rust-literal values (`60`, `360`), selected by which
  of two thin routes was hit (`get_line_hourly_stats`/
  `get_line_six_hourly_stats`, Task 2) — never string-interpolated into SQL
  text.
- **Sparse-data floors (Decision 5, exact values):**
  `halfHour: 10, hour: 20, sixHour: 120, day: 20` (the existing two floors
  unchanged; `hour`/`sixHour` newly derived by the same "~third of max
  possible poll-cycle coverage" rule).
- **Retention (Decision 3):** `half_hourly_stats_retention_hours` default
  bumps from `48` to `840` (35 days) — approved, not re-flagged as an open
  legal question (see header note above).
- **`TrendGranularity` is one type, four explicit tags, defined once** in
  `frontend/lib/history.ts`, imported (not redefined) by `TrendsCharts.tsx`
  and `TrendsResults.tsx`.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy -p api --all-features`,
  `cargo clippy -p aggregator --all-features`, `cargo test -p api`, `cargo
  test -p aggregator`. Every DB-touching test in `crates/api` in this plan
  follows the existing convention of `#[ignore = "requires a live
  database..."]`, run explicitly with
  `DATABASE_URL=<...> cargo test -p api <test_name> -- --ignored` against
  docker compose's postgres. Frontend: `npm test` (= `vitest run`, per
  `frontend/package.json`'s `scripts.test`) from the `frontend/` directory
  after every frontend task. **In addition to `npm test`, a plan-executing
  agent should run `npm run build` at least once near the end of the
  frontend tasks, and start the dev server (`npm run dev`) to manually
  click through the new granularity control against a real `/lines/[id]/history`
  page in a browser** — this repo's own standing practice for UI changes.
  This planning pass does not perform that manual check itself.
- **File scope.**
  - Modified (backend): `crates/api/src/data/queries.rs`,
    `crates/api/src/routes/line_status.rs`,
    `crates/api/src/routes/history_retention.rs`,
    `crates/api/src/data/config.rs`, `crates/api/src/auth.rs`,
    `crates/api/src/routes/chatbot.rs`, `crates/api/src/routes/departures.rs`,
    `crates/api/src/routes/ingest.rs`, `crates/api/src/routes/lines.rs`,
    `crates/api/src/routes/station_stats.rs`, `crates/api/src/routes/train.rs`
    (the last seven only for one new fixture-literal field pair each — see
    Task 3), `crates/aggregator/src/config.rs`, `docker-compose.yml`,
    `charts/distant-signal/templates/api-deployment.yaml`,
    `charts/distant-signal/values.yaml`.
  - Modified (frontend): `frontend/lib/types.ts`, `frontend/lib/api.ts`,
    `frontend/lib/api.test.ts`, `frontend/lib/history.ts`,
    `frontend/lib/history.test.ts`,
    `frontend/app/lines/[id]/history/TrendsCharts.tsx`,
    `frontend/app/lines/[id]/history/TrendsCharts.test.tsx`,
    `frontend/app/lines/[id]/history/TrendsResults.tsx`,
    `frontend/app/lines/[id]/history/TrendsResults.test.tsx`,
    `frontend/app/lines/[id]/history/page.tsx`,
    `frontend/app/lines/[id]/history/page.test.tsx`.
  - Created: `frontend/app/lines/[id]/history/GranularityControl.tsx`,
    `frontend/app/lines/[id]/history/GranularityControl.test.tsx`.
  - **Not touched, anywhere in this plan**: `crates/aggregator/src/queries.rs`,
    `crates/aggregator/src/main.rs`, any `crates/api/migrations/*.sql` file,
    `frontend/app/lines/[id]/history/HalfHourlyTrendsResults.tsx` (+ its
    test), `frontend/app/lines/[id]/history/CoverageTrendsResults.tsx` (+
    its test), `frontend/app/lines/[id]/history/HalfHourlyCoverageTrendsResults.tsx`
    (+ its test), `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`
    (+ its test), `frontend/app/lines/[id]/history/chartPoint.ts`.

---

# Part 1 — Backend (`api` crate)

## Task 1: `sub_daily_stats_for_range` — the one new query function

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add after `half_hourly_stats_for_range`, currently ending around line 1149, before the `--- Decision 4 scaffolding ---` comment at line 1151).
- Test: same file's `#[cfg(test)] mod db_tests` (ends at line 1819).

Independent of every other task except that Task 2 calls what this task
produces.

**Interfaces:**
- Produces: `pub async fn sub_daily_stats_for_range(pool: &PgPool, line_id: &str, from: DateTime<Utc>, to: DateTime<Utc>, bucket_minutes: i64) -> Result<Vec<HalfHourlyStatsRow>>` — the existing `HalfHourlyStatsRow` struct (`queries.rs:1094-1103`), reused verbatim as the return type (Decision 2).

- [ ] **Step 1: Add the query function.** Insert immediately after
  `half_hourly_stats_for_range` (ends line 1149):

```rust
/// Sub-daily sibling of `half_hourly_stats_for_range`, for the two
/// intermediate granularities (Decision 1 of
/// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md):
/// 1-hour and 6-hour buckets, derived at READ time by grouping
/// `line_status_half_hourly_stats` rows via `date_bin` -- no new table, no
/// new aggregator write path (Decision 2; every column here is a SUM, and
/// summing sums is lossless per that decision's Correction 4).
///
/// `bucket_minutes` is NEVER taken from raw request input: it is a plain
/// bound parameter, always one of exactly two caller-supplied literals (60
/// or 360) from this crate's two thin route handlers
/// (`routes::line_status::get_line_hourly_stats`/`get_line_six_hourly_stats`)
/// -- never string-interpolated into the query text, so there is no
/// SQL-injection surface despite selecting the bucket width dynamically.
///
/// The `date_bin` origin (`2000-01-01T00:00:00Z`, a UTC midnight) is
/// arbitrary but load-bearing: `utc_half_hour_start`
/// (`crates/aggregator/src/queries.rs`) only ever produces `:00`/`:30` UTC
/// timestamps, and any UTC midnight divides evenly into 30-minute, 1-hour,
/// AND 6-hour buckets alike, so this origin aligns every bucket boundary
/// to whole hours regardless of which `bucket_minutes` value is requested
/// -- no origin-dependent edge case to get wrong. `date_bin` requires
/// PostgreSQL 14+; this deployment runs Postgres 16
/// (`docker-compose.yml`'s `postgres:16` image), confirmed, not assumed.
///
/// Returns the same `HalfHourlyStatsRow` shape `half_hourly_stats_for_range`
/// does (Decision 2's own sketch: "reuses the existing row shape
/// verbatim") -- its `half_hour_start` field here is the START OF
/// WHATEVER BUCKET WIDTH WAS REQUESTED, not literally a half hour.
/// Callers must not re-expose this field name to JSON unchanged for this
/// function's results -- see `routes::line_status::sub_daily_stats_to_json`,
/// which renames it to `bucketStart` for exactly this reason.
pub async fn sub_daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_minutes: i64,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            date_bin($4 * INTERVAL '1 minute', half_hour_start, TIMESTAMPTZ '2000-01-01T00:00:00Z') AS half_hour_start,
            SUM(sample_cycles)::bigint AS sample_cycles,
            SUM(total)::bigint AS total,
            SUM(delayed)::bigint AS delayed,
            SUM(cancelled)::bigint AS cancelled,
            SUM(skipped)::bigint AS skipped,
            SUM(running_count)::bigint AS running_count,
            SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = $1 AND half_hour_start BETWEEN $2 AND $3
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .bind(bucket_minutes)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
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

- [ ] **Step 2: Add two DB-gated tests** to `mod db_tests` (after the
  existing `half_hourly_stats_for_range_filters_orders_and_handles_unknown_lines`,
  which ends the file at line 1819):

```rust
#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            sub_daily_stats_for_range_groups_half_hourly_rows_into_hourly_buckets -- --ignored` \
            against docker compose's postgres"]
async fn sub_daily_stats_for_range_groups_half_hourly_rows_into_hourly_buckets() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("connect to postgres");
    const LINE_ID: &str = "TEST-SUB-DAILY-HOURLY";

    sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .unwrap();

    // Two half-hourly rows in the same 1-hour bucket (12:00, 12:30), one in
    // the next hour (13:00) -- summed columns must add losslessly
    // (Correction 4 of the design spec).
    sqlx::query(
        "INSERT INTO line_status_half_hourly_stats
            (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum)
         VALUES
            ($1, '2026-08-31T12:00:00Z', 10, 100, 10, 2, 1, 98, 50.0),
            ($1, '2026-08-31T12:30:00Z', 12, 120, 12, 0, 2, 118, 60.0),
            ($1, '2026-08-31T13:00:00Z', 5, 50, 5, 1, 0, 49, 20.0)",
    )
    .bind(LINE_ID)
    .execute(&pool)
    .await
    .expect("seed fixture rows");

    let rows = sub_daily_stats_for_range(
        &pool,
        LINE_ID,
        "2026-08-31T00:00:00Z".parse().unwrap(),
        "2026-09-01T00:00:00Z".parse().unwrap(),
        60,
    )
    .await
    .expect("sub_daily_stats_for_range");

    sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2, "two hourly buckets: 12:00-13:00 and 13:00-14:00");
    assert_eq!(
        rows[0].half_hour_start,
        "2026-08-31T12:00:00Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap()
    );
    assert_eq!(rows[0].sample_cycles, 22, "10 + 12, the two half-hour rows binned into the same hour");
    assert_eq!(rows[0].total, 220);
    assert_eq!(rows[0].delayed, 22);
    assert_eq!(rows[0].delay_minutes_sum, 110.0);
    assert_eq!(
        rows[1].half_hour_start,
        "2026-08-31T13:00:00Z".parse::<chrono::DateTime<chrono::Utc>>().unwrap()
    );
    assert_eq!(rows[1].sample_cycles, 5);
}

#[tokio::test]
#[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
            sub_daily_stats_for_range_with_360_minute_buckets_groups_six_hours_together -- --ignored` \
            against docker compose's postgres"]
async fn sub_daily_stats_for_range_with_360_minute_buckets_groups_six_hours_together() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("connect to postgres");
    const LINE_ID: &str = "TEST-SUB-DAILY-SIX-HOURLY";

    sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO line_status_half_hourly_stats (line_id, half_hour_start, sample_cycles, total) VALUES
            ($1, '2026-08-31T00:00:00Z', 1, 10),
            ($1, '2026-08-31T05:30:00Z', 1, 10),
            ($1, '2026-08-31T06:00:00Z', 1, 10)",
    )
    .bind(LINE_ID)
    .execute(&pool)
    .await
    .expect("seed fixture rows");

    let rows = sub_daily_stats_for_range(
        &pool,
        LINE_ID,
        "2026-08-31T00:00:00Z".parse().unwrap(),
        "2026-09-01T00:00:00Z".parse().unwrap(),
        360,
    )
    .await
    .expect("sub_daily_stats_for_range");

    sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2, "00:00-06:00 and 06:00-12:00 six-hour buckets");
    assert_eq!(rows[0].sample_cycles, 2, "the 00:00 and 05:30 rows both fall in the first six-hour bucket");
    assert_eq!(rows[1].sample_cycles, 1);
}
```

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api sub_daily_stats_for_range   # compiles; DB tests skip (ignored) without DATABASE_URL
# Then, against docker compose's postgres:
DATABASE_URL=postgres://<user>:<pass>@localhost:5432/<db> cargo test -p api \
  sub_daily_stats_for_range -- --ignored
```

Expected: both new DB tests pass, confirming the `date_bin` grouping and
column sums are correct.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "api: add sub_daily_stats_for_range, deriving 1h/6h buckets from line_status_half_hourly_stats via date_bin"
```

---

## Task 2: Two new routes — `/Stats/Hourly/...`, `/Stats/SixHourly/...`

**Files:**
- Modify: `crates/api/src/routes/line_status.rs`.

Depends on Task 1 (`sub_daily_stats_for_range`).

**Interfaces:**
- Consumes: `queries::sub_daily_stats_for_range` (Task 1), `queries::HalfHourlyStatsRow`.
- Produces: `GET /Line/{id}/Stats/Hourly/{from}/to/{to}`,
  `GET /Line/{id}/Stats/SixHourly/{from}/to/{to}`, each returning
  `Vec<Value>` shaped `{ bucketStart, sampleCycles, total, delayed,
  cancelled, skipped, avgDelayMinutes, delayRate, cancellationRate,
  skipRate }`.

- [ ] **Step 1: Register the two routes.** In `router()` (line 36-73),
  insert immediately after the existing `/Stats/HalfHourly/...` route
  (currently lines 55-58), before the `// Decision 4 scaffolding` comment
  at line 59:

```rust
        .route(
            "/Line/{id}/Stats/Hourly/{from}/to/{to}",
            axum::routing::get(get_line_hourly_stats),
        )
        .route(
            "/Line/{id}/Stats/SixHourly/{from}/to/{to}",
            axum::routing::get(get_line_six_hourly_stats),
        )
```

- [ ] **Step 2: Add `sub_daily_stats_to_json`.** Insert after
  `half_hourly_stats_to_json` (currently ends line 456), before
  `get_line_half_hourly_stats`:

```rust
/// Sub-daily sibling of `half_hourly_stats_to_json` -- identical
/// rate-derivation logic, `bucketStart` in place of `halfHourStart`. A
/// distinct field name is deliberate: reusing "halfHourStart" here would
/// misname a 1-hour or 6-hour bucket's start instant. Backs BOTH new
/// sub-daily routes (`get_line_hourly_stats`/`get_line_six_hourly_stats`)
/// -- they share this one function the same way they share
/// `queries::sub_daily_stats_for_range` itself (Decision 2 of
/// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
fn sub_daily_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "bucketStart": row.half_hour_start,
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
```

- [ ] **Step 3: Add the two handlers**, right after
  `get_line_half_hourly_stats` (currently ends line 469):

```rust
async fn get_line_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::sub_daily_stats_for_range(&app.database, &id, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}

async fn get_line_six_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::sub_daily_stats_for_range(&app.database, &id, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows.into_iter().map(sub_daily_stats_to_json).collect()))
}
```

- [ ] **Step 4: Unit tests for `sub_daily_stats_to_json`.** Add to
  `#[cfg(test)] mod tests` (after `half_hourly_stats_to_json_zero_total_never_produces_nan_or_infinity`,
  which ends around line 844):

```rust
#[test]
fn sub_daily_stats_to_json_uses_bucket_start_not_half_hour_start() {
    let row = half_hourly_stats_row(100, 10, 5, 2, 95, 190.0);
    let json = sub_daily_stats_to_json(row);

    assert_eq!(json["bucketStart"], serde_json::json!("2026-08-15T14:00:00Z"));
    assert!(json.get("halfHourStart").is_none(), "must not also expose the half-hour-specific field name");
    assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0));
    assert_eq!(json["delayRate"], serde_json::json!(0.1));
}

#[test]
fn sub_daily_stats_to_json_zero_total_never_produces_nan_or_infinity() {
    let row = half_hourly_stats_row(0, 0, 0, 0, 0, 0.0);
    let json = sub_daily_stats_to_json(row);
    for field in ["avgDelayMinutes", "delayRate", "cancellationRate", "skipRate"] {
        let value = json[field].as_f64().unwrap();
        assert!(value.is_finite());
        assert_eq!(value, 0.0);
    }
}
```

- [ ] **Step 5: Route-coexistence test.** The daily route's `{from}` segment
  (a bare dynamic `NaiveDate` path parameter) sits at the exact position the
  new routes' literal `Hourly`/`SixHourly` segments occupy — the same shape
  of risk `both_stats_routes_coexist_and_route_to_the_correct_handler`
  (line 846) already proved for `HalfHourly`. Add a sibling test right
  after it:

```rust
#[tokio::test]
async fn hourly_and_six_hourly_routes_are_not_shadowed_by_the_daily_or_half_hourly_routes() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn hourly_probe(
        Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
    ) -> String {
        format!("hourly:{id}|{from}|{to}")
    }
    async fn six_hourly_probe(
        Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
    ) -> String {
        format!("six-hourly:{id}|{from}|{to}")
    }
    async fn daily_probe(
        Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
    ) -> String {
        format!("daily:{id}|{from}|{to}")
    }

    let app: axum::Router = axum::Router::new()
        .route("/Line/{id}/Stats/{from}/to/{to}", axum::routing::get(daily_probe))
        .route("/Line/{id}/Stats/Hourly/{from}/to/{to}", axum::routing::get(hourly_probe))
        .route("/Line/{id}/Stats/SixHourly/{from}/to/{to}", axum::routing::get(six_hourly_probe));

    let hourly_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/Line/northern/Stats/Hourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hourly_response.status(), StatusCode::OK);
    let hourly_body = axum::body::to_bytes(hourly_response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(hourly_body.to_vec()).unwrap(),
        "hourly:northern|2026-08-31 00:00:00 UTC|2026-09-01 00:00:00 UTC"
    );

    let six_hourly_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/Line/northern/Stats/SixHourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(six_hourly_response.status(), StatusCode::OK);

    let daily_response = app
        .oneshot(
            Request::builder()
                .uri("/Line/northern/Stats/2026-08-01/to/2026-08-31")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        daily_response.status(),
        StatusCode::OK,
        "the daily NaiveDate route must still work alongside the two new literal-segment routes"
    );
}
```

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api
```

Expected: all pass, including the new unit and coexistence tests.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/line_status.rs
git commit -m "api: add GET /Line/{id}/Stats/{Hourly,SixHourly}/{from}/to/{to} over sub_daily_stats_for_range"
```

---

## Task 3: Extend `/public/history-retention` with the two other ceilings

**Files:**
- Modify: `crates/api/src/routes/history_retention.rs`.
- Modify: `crates/api/src/data/config.rs` (`ServiceArguments`).
- Modify (fixture literals only — one field pair added each):
  `crates/api/src/auth.rs` (`test_config`, line 608),
  `crates/api/src/routes/chatbot.rs` (line 62),
  `crates/api/src/routes/departures.rs` (line 163),
  `crates/api/src/routes/ingest.rs` (line 434),
  `crates/api/src/routes/line_status.rs` (`test_app`, line 1139 — the
  `db_tests` fixture used by Tasks 1–2's own tests),
  `crates/api/src/routes/lines.rs` (line 501),
  `crates/api/src/routes/station_stats.rs` (line 193),
  `crates/api/src/routes/train.rs` (line 880).
- Modify: `docker-compose.yml` (api service's `environment:` block, around
  line 137).
- Modify: `charts/distant-signal/templates/api-deployment.yaml` (around
  line 176).

Independent of Tasks 1–2 (this is Decision 8, a separate, additive change
to a different route). `ServiceArguments` has no `Default` impl and no
`..Default::default()` usage anywhere (confirmed:
`grep -n "\.\.Default::default()\|\.\.ServiceArguments" crates/api/src/routes/*.rs crates/api/src/auth.rs`
returns nothing) — every one of the 8 literal-construction sites listed
above **must** get the two new fields added, or the crate will not compile.

**Interfaces:**
- Produces: `HistoryRetention { history_retention_days, daily_stats_retention_days, half_hourly_stats_retention_hours }` (all `i64`, camelCase on the wire).

- [ ] **Step 1: Add the two `ServiceArguments` fields.** In
  `crates/api/src/data/config.rs`, immediately after `history_retention_days`
  (ends line 187), add:

```rust
    /// How many days of `line_status_daily_stats` rows the aggregator
    /// actually keeps before `queries::prune_daily_stats` deletes them.
    /// This crate never reads or prunes that table itself -- the only
    /// reason this field exists here is so `/public/history-retention`
    /// (`routes/history_retention.rs`) can hand the frontend's Trends-tab
    /// granularity control the real ceiling. Deployments MUST set this to
    /// the same value they give the aggregator's own
    /// `DAILY_STATS_RETENTION_DAYS` -- same convention as
    /// `history_retention_days`, above.
    #[arg(long, env, default_value_t = 300)]
    pub daily_stats_retention_days: i64,

    /// How many hours of `line_status_half_hourly_stats` rows the
    /// aggregator actually keeps before `queries::prune_half_hourly_stats`
    /// deletes them. Same "static config echo, never enforced here"
    /// posture as `history_retention_days`/`daily_stats_retention_days`
    /// above. Deployments MUST set this to the same value they give the
    /// aggregator's own `HALF_HOURLY_STATS_RETENTION_HOURS`.
    #[arg(long, env, default_value_t = 840)]
    pub half_hourly_stats_retention_hours: i64,
```

- [ ] **Step 2: Extend `HistoryRetention` and the handler.** In
  `crates/api/src/routes/history_retention.rs`:

```rust
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetention {
    pub history_retention_days: i64,
    pub daily_stats_retention_days: i64,
    pub half_hourly_stats_retention_hours: i64,
}

async fn get_history_retention(State(app): State<App>) -> Json<HistoryRetention> {
    Json(HistoryRetention {
        history_retention_days: app.config.history_retention_days,
        daily_stats_retention_days: app.config.daily_stats_retention_days,
        half_hourly_stats_retention_hours: app.config.half_hourly_stats_retention_hours,
    })
}
```

  Update the existing test:

```rust
#[test]
fn serializes_as_camel_case() {
    let body = HistoryRetention {
        history_retention_days: 7,
        daily_stats_retention_days: 300,
        half_hourly_stats_retention_hours: 840,
    };
    let json = serde_json::to_value(&body).unwrap();
    assert_eq!(json["historyRetentionDays"], 7);
    assert_eq!(json["dailyStatsRetentionDays"], 300);
    assert_eq!(json["halfHourlyStatsRetentionHours"], 840);
    assert!(json.get("history_retention_days").is_none());
    assert!(json.get("daily_stats_retention_days").is_none());
    assert!(json.get("half_hourly_stats_retention_hours").is_none());
}
```

- [ ] **Step 3: Update every fixture literal.** In each of the 8 files
  listed above, find its `ServiceArguments { ... }` literal and add, right
  after `history_retention_days: <n>,`:

```rust
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
```

  (Field ordering inside the literal doesn't matter to the compiler; placing
  it next to `history_retention_days` keeps the three retention knobs
  visually grouped, matching this file's existing style.)

- [ ] **Step 4: Wire the two new env vars onto the api service/Deployment.**
  In `docker-compose.yml`, immediately after the existing
  `HISTORY_RETENTION_DAYS: ${HISTORY_RETENTION_DAYS:-7}` line in the `api`
  service's `environment:` block (line 137):

```yaml
      # Must be kept equal to the aggregator service's own
      # DAILY_STATS_RETENTION_DAYS/HALF_HOURLY_STATS_RETENTION_HOURS -- see
      # crates/api/src/data/config.rs's doc comments on these two fields.
      DAILY_STATS_RETENTION_DAYS: ${DAILY_STATS_RETENTION_DAYS:-300}
      HALF_HOURLY_STATS_RETENTION_HOURS: ${HALF_HOURLY_STATS_RETENTION_HOURS:-840}
```

  In `charts/distant-signal/templates/api-deployment.yaml`, immediately
  after the existing `HISTORY_RETENTION_DAYS` block (lines 171-176):

```yaml
            # Must equal the aggregator Deployment's own DAILY_STATS_RETENTION_DAYS
            # -- sourced from the SAME value (not a separate api.* setting)
            # so the two can't drift apart. See
            # crates/api/src/data/config.rs's daily_stats_retention_days doc.
            - name: DAILY_STATS_RETENTION_DAYS
              value: {{ .Values.aggregator.dailyStatsRetentionDays | quote }}
            # Must equal the aggregator Deployment's own HALF_HOURLY_STATS_RETENTION_HOURS
            # -- same reasoning. See
            # crates/api/src/data/config.rs's half_hourly_stats_retention_hours doc.
            - name: HALF_HOURLY_STATS_RETENTION_HOURS
              value: {{ .Values.aggregator.halfHourlyStatsRetentionHours | quote }}
```

  No new `values.yaml` keys are needed here — `.Values.aggregator.dailyStatsRetentionDays`
  and `.Values.aggregator.halfHourlyStatsRetentionHours` already exist
  (`values.yaml:602`, `:613`, the latter bumped to `840` by Task 4 below).

- [ ] **Step 5: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api
helm template distant-signal charts/distant-signal \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 \
  --set trustConsumer.kafka.topic=test-topic \
  --set trustConsumer.kafka.saslMechanism=PLAIN \
  --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=test-model \
  --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=test-client \
  --set api.sso.clientSecret=test-secret \
  --set api.sso.redirectUrl=https://app.example.com/callback \
  --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  -s templates/api-deployment.yaml | grep -A1 "DAILY_STATS_RETENTION_DAYS\|HALF_HOURLY_STATS_RETENTION_HOURS"
```

Expected: `cargo test -p api` passes (confirms every fixture literal
compiles); the `helm template` output shows both new env vars rendered with
values `300`/`840`.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/routes/history_retention.rs \
  crates/api/src/auth.rs crates/api/src/routes/chatbot.rs crates/api/src/routes/departures.rs \
  crates/api/src/routes/ingest.rs crates/api/src/routes/line_status.rs crates/api/src/routes/lines.rs \
  crates/api/src/routes/station_stats.rs crates/api/src/routes/train.rs \
  docker-compose.yml charts/distant-signal/templates/api-deployment.yaml
git commit -m "api: report dailyStatsRetentionDays/halfHourlyStatsRetentionHours from /public/history-retention"
```

---

# Part 2 — Aggregator

## Task 4: Bump `half_hourly_stats_retention_hours` default to 840

**Files:**
- Modify: `crates/aggregator/src/config.rs` (field at lines 94-95).
- Modify: `charts/distant-signal/values.yaml` (`halfHourlyStatsRetentionHours: 48` at line 613).

Independent of every other task. Purely a default-value + doc-comment
change (Decision 3) — no code path change, since retention enforcement
(`prune_half_hourly_stats`) already reads this field generically.

- [ ] **Step 1: Update the Rust default and doc comment.** Current
  (`crates/aggregator/src/config.rs:71-95`):

```rust
    /// How long to keep `line_status_half_hourly_stats` rows before
    /// pruning them. Deliberately NOT a reuse of `history_retention_days`
    /// (governs a different table, `line_status_history`) or
    /// `daily_stats_retention_days` (sized for a weeks/months trend use
    /// case this half-hourly rolling-24h view does not have -- reusing its
    /// default of 300 would mean accumulating ~300 days x 48 rows/line of
    /// data only the most recent ~49 rows of which are ever read). 48
    /// hours is a 2x safety margin over the 48-49 rows the line-info-page
    /// embed actually needs at 30-minute granularity, per
    /// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
    /// Decision 5 -- a reasoned starting default, not empirically
    /// validated against real restart/deploy timing (see that spec's Open
    /// question 2).
    ///
    /// This field's UNIT is deliberately unchanged from the table's
    /// original 1-hour-bucket era: retention is measured in wall-clock
    /// hours, not bucket count, so halving the bucket size (1h -> 30min,
    /// alongside this field's own rename from `hourly_stats_retention_hours`)
    /// does not change the default value either -- 48 hours of real time
    /// is still 48 hours of real time. The only consequence is that this
    /// same window now holds roughly twice as many rows per line (~96
    /// instead of ~48) to cover it, which is a trivial row count for
    /// Postgres and not something that needs its own knob.
    #[arg(long, env, default_value_t = 48)]
    pub half_hourly_stats_retention_hours: i64,
```

  Change to:

```rust
    /// How long to keep `line_status_half_hourly_stats` rows before
    /// pruning them.
    ///
    /// Bumped from 48 hours to 840 (35 days) by
    /// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
    /// Decision 3: this table is now also read directly (30-minute
    /// granularity) AND grouped into 1-hour/6-hour buckets
    /// (`crates/api/src/data/queries.rs`'s `sub_daily_stats_for_range`) by
    /// the History page's Trends tab, over the user's actual selected
    /// range -- up to the existing 30-day `RangePreset` ceiling, plus a
    /// 5-day buffer. 48 hours was sized only for the line-info page's
    /// fixed rolling-24h embed (`HalfHourlyTrendsResults`), which still
    /// only ever requests the most recent 24 hours regardless of this
    /// value -- this bump is purely additive for that view, unchanged
    /// behavior.
    ///
    /// This table is fed the SAME LDBWS-derived `SampleStats` value as
    /// `line_status_daily_stats` every cycle (`main.rs`'s `run_cycle`), so
    /// the same RDM Live Departure Board licence lineage applies: the
    /// repo owner confirmed directly that "half-hourly is still fine as
    /// long as we aren't retaining for more than 300 days" -- the same
    /// 300-day ceiling `daily_stats_retention_days` already uses. 840
    /// hours (35 days) clears that with enormous margin, mirroring
    /// `daily_stats_retention_days`'s own "real margin under a hard
    /// compliance ceiling, not a number picked to just barely clear it"
    /// reasoning.
    ///
    /// This field's UNIT is unchanged from the table's original
    /// 1-hour-bucket era: retention is measured in wall-clock hours, not
    /// bucket count. At 840 hours, storage is ~105 lines x 48 rows/day x
    /// 35 days ~= 176,400 rows -- trivial for Postgres, same order of
    /// magnitude this repo's specs have called "trivial" elsewhere.
    #[arg(long, env, default_value_t = 840)]
    pub half_hourly_stats_retention_hours: i64,
```

- [ ] **Step 2: Update the Helm chart default.** In
  `charts/distant-signal/values.yaml`, change line 613 from
  `halfHourlyStatsRetentionHours: 48` to `halfHourlyStatsRetentionHours: 840`,
  and update the comment immediately above it (lines 603-612) to match the
  new reasoning (reference
  `docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md`
  Decision 3 instead of the granularity design's Decision 5, and note the
  repo owner's confirmation, same wording as the Rust doc comment above).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all
cargo clippy -p aggregator --all-features
cargo test -p aggregator
helm lint charts/distant-signal
```

Expected: all pass; no aggregator test asserts the literal default value
today (confirmed: `grep -rn "48" crates/aggregator/src/config.rs` shows
only this field's own `default_value_t`, no test referencing it), so this
is a pure default-value change with nothing to update in
`crates/aggregator`'s own test suite.

- [ ] **Step 4: Commit**

```bash
git add crates/aggregator/src/config.rs charts/distant-signal/values.yaml
git commit -m "aggregator: bump half_hourly_stats_retention_hours default 48 -> 840, per repo-owner-confirmed 300-day licence ceiling"
```

---

# Part 3 — Frontend

## Task 5: New fetch functions + types (`getLineHourlyStats`, `getLineSixHourlyStats`)

**Files:**
- Modify: `frontend/lib/types.ts` (after `LineHalfHourlyStats`, currently ending line 196).
- Modify: `frontend/lib/api.ts` (after `getLineHalfHourlyStats`, currently ending line 189).
- Modify: `frontend/lib/api.test.ts`.

Depends on Task 2 (the routes these call). Independent of Tasks 3-4 and
every other frontend task.

**Interfaces:**
- Produces: `LineHourlyStats`, `LineSixHourlyStats` (both `{ bucketStart: string; sampleCycles: number; total: number; delayed: number; cancelled: number; skipped: number; avgDelayMinutes: number; delayRate: number; cancellationRate: number; skipRate: number }`), `getLineHourlyStats(id, from, to): Promise<LineHourlyStats[]>`, `getLineSixHourlyStats(id, from, to): Promise<LineSixHourlyStats[]>`.

- [ ] **Step 1: Add the two types.** In `frontend/lib/types.ts`, after
  `LineHalfHourlyStats` (ends line 196):

```ts
/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}`'s per-bucket response shape
 * -- same fields as `LineHalfHourlyStats`, but `bucketStart` in place of
 * `halfHourStart`: this is the start of a 1-hour bucket, derived at READ
 * time by grouping `line_status_half_hourly_stats` rows
 * (`crates/api/src/data/queries.rs`'s `sub_daily_stats_for_range`) --
 * reusing "halfHourStart" for a 1-hour bucket would be a misleading field
 * name. Always an RFC3339 UTC instant; render it through
 * `frontend/lib/dateFormat.ts`'s `formatTime` before display, same
 * convention `LineHalfHourlyStats.halfHourStart` follows. */
export interface LineHourlyStats {
  bucketStart: string; // RFC3339 UTC instant, start of the 1-hour bucket
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

/** `GET /Line/{id}/Stats/SixHourly/{from}/to/{to}`'s per-bucket response
 * shape -- identical to `LineHourlyStats` except the bucket is 6 hours
 * wide, not 1. */
export interface LineSixHourlyStats {
  bucketStart: string; // RFC3339 UTC instant, start of the 6-hour bucket
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

- [ ] **Step 2: Add the two fetch functions.** In `frontend/lib/api.ts`,
  import `LineHourlyStats, LineSixHourlyStats` in the existing `@/lib/types`
  import block (alongside `LineDailyStats, LineHalfHourlyStats`), then add
  after `getLineHalfHourlyStats` (ends line 189):

```ts
/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}` -- the 1-hour sub-daily
 * rollup route (Decision 2 of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
 * Same RFC3339-instant/public/no-store/no-cookie-forwarding shape as
 * `getLineHalfHourlyStats`. */
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

/** `GET /Line/{id}/Stats/SixHourly/{from}/to/{to}` -- the 6-hour sub-daily
 * rollup route, sibling of `getLineHourlyStats`. */
export async function getLineSixHourlyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineSixHourlyStats[]> {
  return fetchJson<LineSixHourlyStats[]>(
    `${baseUrl()}/Line/${id}/Stats/SixHourly/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}
```

- [ ] **Step 3: Add tests to `frontend/lib/api.test.ts`.** Import
  `getLineHourlyStats, getLineSixHourlyStats` alongside the existing
  `getLineDailyStats, getLineHalfHourlyStats` import (line 7-8), then add
  after the existing `getLineHalfHourlyStats builds the correct URL` test
  (ends line 225):

```ts
  it('getLineHourlyStats builds the correct URL', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })));
    await getLineHourlyStats('wcml', '2026-08-31T00:00:00.000Z', '2026-09-01T00:00:00.000Z');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Stats/Hourly/2026-08-31T00:00:00.000Z/to/2026-09-01T00:00:00.000Z',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });

  it('getLineSixHourlyStats builds the correct URL', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify([]), { status: 200 })));
    await getLineSixHourlyStats('wcml', '2026-08-31T00:00:00.000Z', '2026-09-01T00:00:00.000Z');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/wcml/Stats/SixHourly/2026-08-31T00:00:00.000Z/to/2026-09-01T00:00:00.000Z',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
```

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- api.test.ts
```

Expected: all pass, including the two new tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "frontend: add getLineHourlyStats/getLineSixHourlyStats over the new sub-daily routes"
```

---

## Task 6: `TrendGranularity` + `resolveGranularity`/`availableGranularities`/`granularityShortfallDays`

**Files:**
- Modify: `frontend/lib/history.ts` (append after `retentionShortfallDays`, currently ending line 269).
- Modify: `frontend/lib/history.test.ts`.

Independent of Task 5. Depends on nothing else — pure logic, no fetches.

**Interfaces:**
- Produces: `type TrendGranularity = 'halfHour' | 'hour' | 'sixHour' | 'day'`, `interface GranularityRetentionCeilings { dailyStatsRetentionDays: number; halfHourlyStatsRetentionHours: number }`, `availableGranularities(rangeWidthMs: number, ceilings: GranularityRetentionCeilings): TrendGranularity[]`, `resolveGranularity(params: { granularity?: string }, rangeWidthMs: number, ceilings: GranularityRetentionCeilings): TrendGranularity`, `granularityShortfallDays(range: Pick<ResolvedRange, 'from'>, granularity: TrendGranularity, ceilings: GranularityRetentionCeilings, now: number): number | null`.

- [ ] **Step 1: Add the type, constants, and three functions.** Append to
  `frontend/lib/history.ts`:

```ts
export type TrendGranularity = 'halfHour' | 'hour' | 'sixHour' | 'day';

/** Finest to coarsest -- the four tiers Decision 1 of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
 * settled on. Order matters: `resolveGranularity`'s fallback walks this
 * array rightward (toward `'day'`) to find "the next coarser available
 * tier" (Decision 7). */
const GRANULARITY_ORDER: TrendGranularity[] = ['halfHour', 'hour', 'sixHour', 'day'];

/** Bucket width in minutes per tier -- used only to estimate how many
 * chart points a range would render (`withinPointBudget`). Matches the
 * bucket sizes fixed by Decision 1/2 (30/60/360 minutes); `day`'s 1440 is
 * never actually compared against `MAX_CHART_POINTS` -- see
 * `withinPointBudget`'s own doc comment. */
const BUCKET_MINUTES: Record<TrendGranularity, number> = { halfHour: 30, hour: 60, sixHour: 360, day: 1440 };

/** Decision 7's placeholder point-count ceiling -- "a reasoned, not
 * measured" starting value, the same unvalidated-guess posture as every
 * other floor/ceiling constant in this feature area (design spec Open
 * question 2). Revisit against real chart-legibility feedback once this
 * has run in production. */
const MAX_CHART_POINTS = 200;

/** The two retention ceilings bounding the three sub-daily tiers (all
 * backed by one table, `line_status_half_hourly_stats` -- Decision 2) and
 * the daily tier, echoed from `/public/history-retention` (Decision 8). */
export interface GranularityRetentionCeilings {
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}

/** Whether `granularity`'s real backing retention window reaches back far
 * enough to cover a range this wide. `'day'` is always `true` here --
 * deliberately NOT checked against `dailyStatsRetentionDays`. This
 * resolves the design spec's Open question 5 (should the daily tier ever
 * become unavailable?) in favor of "no": the daily tier already existed,
 * unguarded, before this feature, and disabling the only tier a viewer has
 * ever had for an over-wide custom range would be a regression, not a fix
 * -- the existing shortfall-banner honesty mechanism
 * (`granularityShortfallDays`, below) already covers that case the same
 * way `retentionShortfallDays` always has for the Timeline tab. This also
 * guarantees `availableGranularities` never returns an empty array. */
function withinRetention(
  granularity: TrendGranularity,
  rangeWidthMs: number,
  ceilings: GranularityRetentionCeilings,
): boolean {
  if (granularity === 'day') return true;
  return rangeWidthMs <= ceilings.halfHourlyStatsRetentionHours * HOUR_MS;
}

/** Whether rendering `granularity` over a range this wide would stay at or
 * under `MAX_CHART_POINTS`. `'day'` is exempt for the same reason
 * `withinRetention` exempts it -- see that function's doc comment. */
function withinPointBudget(granularity: TrendGranularity, rangeWidthMs: number): boolean {
  if (granularity === 'day') return true;
  return rangeWidthMs / (BUCKET_MINUTES[granularity] * 60_000) <= MAX_CHART_POINTS;
}

/** The tiers `GranularityControl` should actually offer for a range this
 * wide, finest first. Always includes `'day'` (see `withinRetention`'s doc
 * comment) -- callers can rely on this never being empty. */
export function availableGranularities(
  rangeWidthMs: number,
  ceilings: GranularityRetentionCeilings,
): TrendGranularity[] {
  return GRANULARITY_ORDER.filter(
    (granularity) => withinRetention(granularity, rangeWidthMs, ceilings) && withinPointBudget(granularity, rangeWidthMs),
  );
}

function isTrendGranularity(value: string | undefined): value is TrendGranularity {
  return value === 'halfHour' || value === 'hour' || value === 'sixHour' || value === 'day';
}

/** The Trends tab's `?granularity=` URL param, resolved the same
 * "URL is the source of truth, invalid falls back to a default rather than
 * erroring" way `resolveRange` already resolves `?range=` (Decision 6).
 * Defaults to `'day'` when unset/unparseable -- the existing, always-safe
 * behavior, unchanged for a viewer who has never touched the new control.
 * If the requested tier is a real `TrendGranularity` but isn't currently
 * available for `rangeWidthMs` (e.g. a wide custom range pushes `'hour'`
 * past the point budget), falls back to the next coarser AVAILABLE tier
 * (Decision 7) -- never silently ignored, never thrown. */
export function resolveGranularity(
  params: { granularity?: string },
  rangeWidthMs: number,
  ceilings: GranularityRetentionCeilings,
): TrendGranularity {
  const available = availableGranularities(rangeWidthMs, ceilings);
  if (!isTrendGranularity(params.granularity)) return 'day';
  if (available.includes(params.granularity)) return params.granularity;

  const idx = GRANULARITY_ORDER.indexOf(params.granularity);
  return GRANULARITY_ORDER.slice(idx + 1).find((g) => available.includes(g)) ?? 'day';
}

/** Trends-tab sibling of `retentionShortfallDays`, for the sub-daily-aware
 * ceiling Decision 8 adds. Reuses `retentionShortfallDays`'s exact
 * day-based math rather than duplicating it: a sub-daily tier's ceiling is
 * expressed in hours (`halfHourlyStatsRetentionHours`), converted here to
 * an equivalent whole-day figure that function already knows how to
 * compare a range against. */
export function granularityShortfallDays(
  range: Pick<ResolvedRange, 'from'>,
  granularity: TrendGranularity,
  ceilings: GranularityRetentionCeilings,
  now: number,
): number | null {
  const retentionDays =
    granularity === 'day' ? ceilings.dailyStatsRetentionDays : Math.floor(ceilings.halfHourlyStatsRetentionHours / 24);
  return retentionShortfallDays(range, retentionDays, now);
}
```

- [ ] **Step 2: Add tests to `frontend/lib/history.test.ts`.** Append after
  the existing `retentionShortfallDays` describe block:

```ts
describe('availableGranularities', () => {
  const GENEROUS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };

  it('offers all four tiers for a narrow (12-hour) range', () => {
    expect(availableGranularities(12 * 3_600_000, GENEROUS)).toEqual(['halfHour', 'hour', 'sixHour', 'day']);
  });

  it('excludes half-hourly and hourly (point budget) but keeps six-hourly and daily for a 10-day range', () => {
    // 10 days: halfHour -> 480 points, hour -> 240 points (both over 200);
    // sixHour -> 40 points (under 200); all three are still within the
    // 840-hour (35-day) retention ceiling, so only the point budget excludes them.
    const tenDaysMs = 10 * 86_400_000;
    expect(availableGranularities(tenDaysMs, GENEROUS)).toEqual(['sixHour', 'day']);
  });

  it('excludes every sub-daily tier (retention) for a range wider than the shared 35-day ceiling, leaving only day', () => {
    const fortyDaysMs = 40 * 86_400_000;
    expect(availableGranularities(fortyDaysMs, GENEROUS)).toEqual(['day']);
  });

  it('never returns an empty array, even with zero ceilings', () => {
    expect(
      availableGranularities(365 * 86_400_000, { dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 }),
    ).toEqual(['day']);
  });
});

describe('resolveGranularity', () => {
  const GENEROUS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };
  const ONE_DAY_MS = 86_400_000;

  it('defaults to day when unset', () => {
    expect(resolveGranularity({}, ONE_DAY_MS, GENEROUS)).toBe('day');
  });

  it('falls back to day for junk input', () => {
    expect(resolveGranularity({ granularity: 'fortnightly' }, ONE_DAY_MS, GENEROUS)).toBe('day');
  });

  it('honours a requested tier that is available', () => {
    expect(resolveGranularity({ granularity: 'hour' }, ONE_DAY_MS, GENEROUS)).toBe('hour');
  });

  it('falls back to the next coarser available tier when the requested one is not available', () => {
    // 10 days: hour is unavailable (point budget), sixHour is the next coarser available tier.
    const tenDaysMs = 10 * 86_400_000;
    expect(resolveGranularity({ granularity: 'hour' }, tenDaysMs, GENEROUS)).toBe('sixHour');
  });

  it('falls all the way back to day when nothing finer is available', () => {
    const fortyDaysMs = 40 * 86_400_000;
    expect(resolveGranularity({ granularity: 'halfHour' }, fortyDaysMs, GENEROUS)).toBe('day');
  });
});

describe('granularityShortfallDays', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');
  const CEILINGS = { dailyStatsRetentionDays: 300, halfHourlyStatsRetentionHours: 840 };

  it('is null for the day tier when the range fits within dailyStatsRetentionDays', () => {
    const range = { from: new Date(NOW - 30 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'day', CEILINGS, NOW)).toBeNull();
  });

  it('reports the shortfall for the day tier when the range exceeds dailyStatsRetentionDays', () => {
    const range = { from: new Date(NOW - 310 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'day', CEILINGS, NOW)).toBe(10);
  });

  it('is null for a sub-daily tier when the range fits within the hours-derived ceiling', () => {
    const range = { from: new Date(NOW - 30 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'hour', CEILINGS, NOW)).toBeNull();
  });

  it('reports the shortfall for a sub-daily tier converted from hours to days', () => {
    // 840 hours = 35 days; a 40-day-old range is 5 days beyond that.
    const range = { from: new Date(NOW - 40 * 86_400_000).toISOString() };
    expect(granularityShortfallDays(range, 'halfHour', CEILINGS, NOW)).toBe(5);
  });
});
```

  Update the file's import line to include the new exports:

```ts
import {
  groupHistoryByDay,
  resolveRange,
  resolveHalfHourlyRange,
  retentionShortfallDays,
  availableGranularities,
  resolveGranularity,
  granularityShortfallDays,
} from './history';
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- history.test.ts
```

Expected: all pass, including every new test above.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/history.ts frontend/lib/history.test.ts
git commit -m "frontend: add TrendGranularity, resolveGranularity/availableGranularities/granularityShortfallDays"
```

---

## Task 7: Widen `TrendsCharts`'s `granularity` prop to `TrendGranularity`

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.tsx`.
- Modify: `frontend/app/lines/[id]/history/TrendsCharts.test.tsx`.

Depends on Task 6 (`TrendGranularity`). Independent of Task 5 and 8-10.
**Must not change `HalfHourlyTrendsResults.tsx`** — its existing
`granularity="halfHour"` literal (line 100) must still type-check unchanged
against the widened prop type (see "Judgment calls" #1, above).

**Interfaces:**
- Consumes: `TrendGranularity` (Task 6, from `@/lib/history`).
- Produces: `TrendsCharts({ points, granularity, order })` where `granularity: TrendGranularity`.

- [ ] **Step 1: Widen the prop type and the tick-formatter condition.**
  Add the import:

```ts
import type { TrendGranularity } from '@/lib/history';
```

  Change the prop type (current, lines 77-93):

```ts
export function TrendsCharts({
  points,
  granularity,
  order,
}: {
  points: ChartPoint[];
  granularity: 'day' | 'halfHour';
  order: TitleOrder;
}) {
```

  to:

```ts
export function TrendsCharts({
  points,
  granularity,
  order,
}: {
  points: ChartPoint[];
  granularity: TrendGranularity;
  order: TitleOrder;
}) {
```

  and change the `xAxisProps` condition (current, line 94-97):

```ts
  const xAxisProps = {
    padding: { right: 12 },
    ...(granularity === 'halfHour' ? { tickFormatter: (value: string) => formatTime(value) } : {}),
  };
```

  to:

```ts
  const xAxisProps = {
    padding: { right: 12 },
    ...(granularity !== 'day' ? { tickFormatter: (value: string) => formatTime(value) } : {}),
  };
```

- [ ] **Step 2: Update the doc comment above the component** (lines
  61-76) to reflect that `granularity` now has four possible values, all
  three non-`'day'` values sharing the identical `formatTime`-based
  tick-label treatment — replace the sentence "a plain, serializable
  `'day' | 'halfHour'` string" with "a plain, serializable
  `TrendGranularity` string (`'halfHour' | 'hour' | 'sixHour' | 'day'`,
  `frontend/lib/history.ts`)" and "`granularity === 'halfHour'`
  additionally renders..." with "any non-`'day'` value additionally
  renders...". Leave the rest of the comment (the collision-risk
  explanation) unchanged — it still applies identically to all three
  sub-daily tiers.

- [ ] **Step 3: Add direct prop-level tests.** `TrendsCharts.test.tsx`
  today only imports `gapSpans`, not the component itself. Add, after the
  existing imports:

```tsx
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

vi.mock('@mantine/charts', () => ({
  LineChart: (props: { xAxisProps?: { tickFormatter?: (value: string) => string } }) => (
    <div
      data-testid="line-chart"
      data-has-tick-formatter={String(typeof props.xAxisProps?.tickFormatter === 'function')}
    />
  ),
}));
```

  (`vi` must also be imported from `'vitest'` — add it to the existing
  `import { describe, it, expect } from 'vitest';` line.)

  Then add a new describe block at the end of the file:

```tsx
describe('TrendsCharts granularity prop', () => {
  const points: ChartPoint[] = [
    { bucketKey: '2026-08-01T12:00:00Z', delayRate: 0.1, cancellationRate: 0, skipRate: 0, avgDelayMinutes: 1, sampleCycles: 50 },
  ];

  it.each(['halfHour', 'hour', 'sixHour'] as const)(
    'gives the x-axis a tickFormatter for the %s granularity',
    (granularity) => {
      renderWithMantine(<TrendsCharts points={points} granularity={granularity} order={2} />);
      expect(screen.getAllByTestId('line-chart')[0]).toHaveAttribute('data-has-tick-formatter', 'true');
    },
  );

  it('gives the x-axis no tickFormatter for the day granularity', () => {
    renderWithMantine(<TrendsCharts points={points} granularity="day" order={2} />);
    expect(screen.getAllByTestId('line-chart')[0]).toHaveAttribute('data-has-tick-formatter', 'false');
  });
});
```

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- TrendsCharts.test.tsx
```

Expected: all pass, including the pre-existing `gapSpans` tests (unchanged)
and the four new prop-level tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/\[id\]/history/TrendsCharts.tsx frontend/app/lines/\[id\]/history/TrendsCharts.test.tsx
git commit -m "frontend: widen TrendsCharts' granularity prop to the full four-tier TrendGranularity"
```

---

## Task 8: Rewrite `TrendsResults.tsx` to branch on `granularity` (Decision 4)

**Files:**
- Modify: `frontend/app/lines/[id]/history/TrendsResults.tsx`.
- Modify: `frontend/app/lines/[id]/history/TrendsResults.test.tsx`.

Depends on Tasks 5-7 (fetch functions, `TrendGranularity`, widened
`TrendsCharts`). This is "the one place real, new frontend logic lands"
per the spec's own Reuse assessment.

**Interfaces:**
- Consumes: `getLineDailyStats`, `getLineHalfHourlyStats`, `getLineHourlyStats`, `getLineSixHourlyStats` (Task 5); `TrendGranularity` (Task 6); `TrendsCharts` (Task 7).
- Produces: `TrendsResults({ id, from, to, granularity? })` — `granularity` optional, defaulting to `'day'` so every existing call site (`page.tsx`, today calling `<TrendsResults id={id} from={range.from} to={range.to} />` with no `granularity` prop) keeps compiling and behaving identically until Task 10 wires the new param through.

- [ ] **Step 1: Replace the whole file.**

```tsx
import { Paper, Stack, Text, Title } from '@mantine/core';
import { getLineDailyStats, getLineHalfHourlyStats, getLineHourlyStats, getLineSixHourlyStats } from '@/lib/api';
import { londonDayKey } from '@/lib/dateFormat';
import type { TrendGranularity } from '@/lib/history';
import { TrendsCharts } from './TrendsCharts';
import type { ChartPoint } from './chartPoint';

// Placeholders, not validated numbers -- see
// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
// Decision 5. `day`/`halfHour` are unchanged from their prior standalone
// constants (`SPARSE_DATA_FLOOR_CYCLES`/`SPARSE_DATA_FLOOR_CYCLES_HALF_HOURLY`
// in this file's and HalfHourlyTrendsResults.tsx's git history);
// `hour`/`sixHour` are newly derived by the same "~third of the bucket's
// max possible poll-cycle coverage" rule.
const SPARSE_FLOOR: Record<TrendGranularity, number> = {
  halfHour: 10,
  hour: 20,
  sixHour: 120,
  day: 20,
};

// One honesty-copy sentence per granularity (Ruling A,
// .superpowers/sdd/2026-08-31-line-history-graphics/progress.md, extended
// to the two new tiers by the same template) -- must not be softened or
// dropped, same as this file's pre-existing `day` copy.
const HONESTY_COPY: Record<TrendGranularity, string> = {
  day: 'Rates shown count each distinct train once per day, based on its status the first time it was seen that day -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Days with too little coverage show as a gap rather than a misleading flat line.',
  halfHour: 'Rates shown count each distinct train once per half hour, based on its status the first time it was seen that half hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Half-hour periods with too little coverage show as a gap rather than a misleading flat line.',
  hour: 'Rates shown count each distinct train once per hour, based on its status the first time it was seen that hour -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Hours with too little coverage show as a gap rather than a misleading flat line.',
  sixHour: 'Rates shown count each distinct train once per six-hour period, based on its status the first time it was seen in that period -- not a share of poll cycles. A train that starts on time and only becomes delayed later while still in view will still show here as on time. Six-hour periods with too little coverage show as a gap rather than a misleading flat line.',
};

interface StatsRow {
  sampleCycles: number;
  delayRate: number;
  cancellationRate: number;
  skipRate: number;
  avgDelayMinutes: number;
}

// Generalized from the original day-only toChartPoints: same
// null-all-four-fields-together gap logic (Decision 3 of
// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md), now
// parameterized over which row field supplies `bucketKey` and which floor
// applies, so one function serves all four granularities' differently-shaped
// row types (LineDailyStats.day, LineHalfHourlyStats.halfHourStart,
// LineHourlyStats/LineSixHourlyStats.bucketStart).
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
      sampleCycles: row.sampleCycles,
    };
  });
}

// Dispatches to the right fetch + floor + bucket-key field for the
// selected tier (Decision 4's own sketch). `day` still converts its
// RFC3339 `from`/`to` to London calendar-day keys first, exactly as
// before -- the only granularity whose route takes NaiveDate path segments.
async function fetchPoints(id: string, granularity: TrendGranularity, from: string, to: string): Promise<ChartPoint[]> {
  switch (granularity) {
    case 'day': {
      const stats = await getLineDailyStats(id, londonDayKey(from), londonDayKey(to));
      return toChartPoints(stats, (row) => row.day, SPARSE_FLOOR.day);
    }
    case 'halfHour': {
      const stats = await getLineHalfHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.halfHourStart, SPARSE_FLOOR.halfHour);
    }
    case 'hour': {
      const stats = await getLineHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.hour);
    }
    case 'sixHour': {
      const stats = await getLineSixHourlyStats(id, from, to);
      return toChartPoints(stats, (row) => row.bucketStart, SPARSE_FLOOR.sixHour);
    }
  }
}

export async function TrendsResults({
  id,
  from,
  to,
  granularity = 'day',
}: {
  id: string;
  from: string;
  to: string;
  /** Defaults to `'day'` -- the existing, always-safe behavior, unchanged
   * for any call site that doesn't pass this yet (Decision 6). */
  granularity?: TrendGranularity;
}) {
  // Reversing Decision 4 of
  // docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
  // also means this component now serves the same backend table the
  // line-info page's HalfHourlyTrendsResults reads for three of its four
  // tiers -- adopting that component's own try/catch degrade-gracefully
  // posture here too (previously this file had none, and an unhandled
  // rejection would have propagated to app/error.tsx, blanking the whole
  // page over a secondary chart) makes error handling consistent across
  // all four tiers rather than arbitrarily different for `day` alone.
  let points: ChartPoint[];
  try {
    points = await fetchPoints(id, granularity, from, to);
  } catch {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Trend data isn&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (points.length === 0) {
    // Live-investigated (docs/superpowers/plans/2026-09-02-line-history-chart-fixes.md
    // Task 6 Step 1): the reported "dead whitespace below the footer" is a
    // transient Suspense loading-flash artifact, not a persistent layout
    // bug. Wrapped in a bounded `Paper` regardless -- it reads as a
    // deliberately-finished component rather than a chart that failed to
    // render.
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Not enough sampled data yet for this line.</Text>
      </Paper>
    );
  }

  return (
    <Stack gap="lg">
      <Text size="sm" c="dimmed">
        {HONESTY_COPY[granularity]}
      </Text>
      {/* Both charts (including their `valueFormatter`) live in TrendsCharts,
          a Client Component -- see its own doc comment for why: a plain
          function prop can't cross the Server-to-Client boundary straight
          out of this `async` Server Component. */}
      {/* order={2}: this sits directly under /lines/[id]/history's only
          h1 ("History: {name}"), with nothing between -- h2 keeps the
          chart titles one level below that h1, with no skip. */}
      <TrendsCharts points={points} granularity={granularity} order={2} />
    </Stack>
  );
}
```

- [ ] **Step 2: Rewrite `TrendsResults.test.tsx`.** The exported
  `toChartPoints` signature changed from `(stats: LineDailyStats[])` to a
  generic `(stats, bucketKeyOf, floor)` — the existing `describe('toChartPoints', ...)`
  block (lines 60-83) must be updated to call it with the new arity.
  Replace the whole file with:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrendsResults, toChartPoints } from './TrendsResults';
import * as api from '@/lib/api';
import type { LineDailyStats, LineHalfHourlyStats, LineHourlyStats, LineSixHourlyStats } from '@/lib/types';

vi.mock('@/lib/api');

type MockLineChartProps = {
  data: unknown[];
  series: { name: string; strokeDasharray?: string | number }[];
  connectNulls?: boolean;
  withLegend?: boolean;
  valueFormatter?: (value: number) => string;
  xAxisProps?: { padding?: unknown; tickFormatter?: (value: string) => string };
};

const lineChartMock = vi.fn((props: MockLineChartProps) => (
  <div
    data-testid="line-chart"
    data-series={props.series.map((series) => series.name).join(',')}
    data-connect-nulls={String(props.connectNulls)}
    data-points={JSON.stringify(props.data)}
    data-with-legend={String(props.withLegend)}
    data-dash-patterns={props.series.map((series) => series.strokeDasharray ?? '').join(',')}
  />
));

vi.mock('@mantine/charts', () => ({ LineChart: (props: MockLineChartProps) => lineChartMock(props) }));

function dailyRow(overrides: Partial<LineDailyStats> = {}): LineDailyStats {
  return {
    day: '2026-08-01',
    sampleCycles: 500,
    total: 100,
    delayed: 10,
    cancelled: 2,
    skipped: 1,
    avgDelayMinutes: 3.5,
    delayRate: 0.1,
    cancellationRate: 0.02,
    skipRate: 0.01,
    ...overrides,
  };
}

function halfHourlyRow(overrides: Partial<LineHalfHourlyStats> = {}): LineHalfHourlyStats {
  return { ...dailyRow(), halfHourStart: '2026-08-31T14:00:00Z', ...overrides } as LineHalfHourlyStats;
}

function hourlyRow(overrides: Partial<LineHourlyStats> = {}): LineHourlyStats {
  return { ...dailyRow(), bucketStart: '2026-08-31T14:00:00Z', ...overrides } as LineHourlyStats;
}

function sixHourlyRow(overrides: Partial<LineSixHourlyStats> = {}): LineSixHourlyStats {
  return { ...dailyRow(), bucketStart: '2026-08-31T12:00:00Z', ...overrides } as LineSixHourlyStats;
}

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
    expect(point.sampleCycles).toBe(19);
  });
});

describe('TrendsResults', () => {
  it('defaults to the day granularity when none is passed, unchanged from before this feature', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyRow({ day: '2026-08-01' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(api.getLineDailyStats).toHaveBeenCalledWith('wcml', '2026-08-01', '2026-08-08');
    expect(screen.getByText(/Rates shown count each distinct train once per day/)).toBeInTheDocument();
  });

  it('renders the empty state when there are no rows, inside a bounded container', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    const text = screen.getByText('Not enough sampled data yet for this line.');
    expect(text).toBeInTheDocument();
    expect(screen.queryByTestId('line-chart')).not.toBeInTheDocument();
    expect(text.closest('.mantine-Paper-root')).not.toBeNull();
  });

  it('degrades gracefully, not to app/error.tsx, when the backend fetch throws', async () => {
    vi.mocked(api.getLineDailyStats).mockRejectedValue(new Error('boom'));
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByText("Trend data isn't available right now.")).toBeInTheDocument();
  });

  it('a sparse day does not throw and does not render a flat-zero-looking point', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([
      dailyRow({ day: '2026-08-01', sampleCycles: 19 }),
      dailyRow({ day: '2026-08-02', sampleCycles: 500 }),
    ]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    const sparseDay = points.find((point: { bucketKey: string }) => point.bucketKey === '2026-08-01');
    expect(sparseDay.delayRate).toBeNull();
    expect(sparseDay.delayRate).not.toBe(0);
  });

  it('renders both chart titles at h2, one level below this page\'s only h1 ("History: {name}")', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyRow({ day: '2026-08-01' })]);
    renderWithMantine(await TrendsResults({ id: 'wcml', from: '2026-08-01T00:00:00Z', to: '2026-08-08T00:00:00Z' }));
    expect(screen.getByRole('heading', { name: 'Delay / cancellation / skip rate', level: 2 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Average delay (minutes)', level: 2 })).toBeInTheDocument();
  });

  it.each([
    ['halfHour', 'getLineHalfHourlyStats', halfHourlyRow, 10, 'per half hour'] as const,
    ['hour', 'getLineHourlyStats', hourlyRow, 20, 'per hour'] as const,
    ['sixHour', 'getLineSixHourlyStats', sixHourlyRow, 120, 'per six-hour period'] as const,
  ])('dispatches to the right fetch, floor, and honesty copy for the %s granularity', async (granularity, fnName, rowFactory, floor, copyFragment) => {
    const mockFn = vi.mocked(api[fnName as keyof typeof api]) as unknown as ReturnType<typeof vi.fn>;
    mockFn.mockResolvedValue([rowFactory({ sampleCycles: floor })]);
    renderWithMantine(
      await TrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z', granularity }),
    );
    expect(mockFn).toHaveBeenCalledWith('wcml', '2026-08-31T00:00:00Z', '2026-09-01T00:00:00Z');
    expect(screen.getByText(new RegExp(`Rates shown count each distinct train once ${copyFragment}`))).toBeInTheDocument();

    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    expect(points[0].delayRate).not.toBeNull(); // exactly at the floor -- not sparse
  });

  it.each([
    ['halfHour', 'getLineHalfHourlyStats', halfHourlyRow, 10] as const,
    ['hour', 'getLineHourlyStats', hourlyRow, 20] as const,
    ['sixHour', 'getLineSixHourlyStats', sixHourlyRow, 120] as const,
  ])('treats a %s bucket one below its floor as a gap', async (granularity, fnName, rowFactory, floor) => {
    const mockFn = vi.mocked(api[fnName as keyof typeof api]) as unknown as ReturnType<typeof vi.fn>;
    mockFn.mockResolvedValue([rowFactory({ sampleCycles: floor - 1 })]);
    renderWithMantine(
      await TrendsResults({ id: 'wcml', from: '2026-08-31T00:00:00Z', to: '2026-09-01T00:00:00Z', granularity }),
    );
    const charts = screen.getAllByTestId('line-chart');
    const rateChart = charts.find((chart) => chart.dataset.series === 'delayRate,cancellationRate,skipRate');
    const points = JSON.parse(rateChart!.dataset.points as string);
    expect(points[0].delayRate).toBeNull();
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- TrendsResults.test.tsx
```

Expected: all pass. Confirm no other file imports `TrendsResults.tsx`'s old
single-argument `toChartPoints` (only this test file did):
`grep -rn "toChartPoints" frontend --include=*.tsx --include=*.ts | grep -v node_modules`.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/\[id\]/history/TrendsResults.tsx frontend/app/lines/\[id\]/history/TrendsResults.test.tsx
git commit -m "frontend: TrendsResults branches on a granularity prop across all four tiers (Decision 4)"
```

---

## Task 9: `GranularityControl.tsx` — the Trends-tab picker (Decision 6)

**Files:**
- Create: `frontend/app/lines/[id]/history/GranularityControl.tsx`.
- Create: `frontend/app/lines/[id]/history/GranularityControl.test.tsx`.

Depends on Task 6 (`TrendGranularity`, `availableGranularities`).
Independent of Tasks 7-8. This component is a plain, router-driven client
component in the mold of `HistoryRangePicker.tsx` — new URL navigation, no
external state library.

**Interfaces:**
- Produces: `GranularityControl({ lineId, preset, from, to, granularity, available })` — a `'use client'` component rendering a `SegmentedControl` and navigating via `router.push` on change.

- [ ] **Step 1: Write the component.**

```tsx
'use client';

import { useRouter } from 'next/navigation';
import { SegmentedControl, Stack, Text } from '@mantine/core';
import type { RangePreset, TrendGranularity } from '@/lib/history';

const LABELS: Record<TrendGranularity, string> = {
  halfHour: '30 min',
  hour: 'Hourly',
  sixHour: '6-hourly',
  day: 'Daily',
};

// Finest to coarsest -- matches frontend/lib/history.ts's own
// GRANULARITY_ORDER. Duplicated here (not imported) because it's a plain
// display-order constant with no logic attached; `available` (computed by
// page.tsx via history.ts's own availableGranularities) is the actual
// source of truth for which tiers show up at all.
const DISPLAY_ORDER: TrendGranularity[] = ['halfHour', 'hour', 'sixHour', 'day'];

/** Renders a `SegmentedControl` scoped to the Trends `TabsPanel` (Decision
 * 6 of docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md
 * -- deliberately NOT added to `HistoryRangePicker.tsx`, since granularity
 * has no meaning on the Timeline tab). An unavailable tier is OMITTED from
 * `data` entirely rather than rendered disabled -- following the one
 * existing precedent for a conditional `SegmentedControl` option in this
 * codebase (`components/IssueList.tsx`'s "Ended" bucket, "only offered
 * when something is actually in it"). `available` always includes `'day'`
 * (see `availableGranularities`'s own doc comment), so `data` is never
 * empty. State lives in the URL, matching every other piece of range state
 * on this page (`HistoryRangePicker.tsx`'s own `handlePreset`/`handleSearch`
 * convention) -- switching tiers navigates with the SAME range params
 * (`preset`, or `from`/`to`) plus a `?granularity=` param, never losing the
 * currently-viewed date range. */
export function GranularityControl({
  lineId,
  preset,
  from,
  to,
  granularity,
  available,
}: {
  lineId: string;
  preset: RangePreset | null;
  from: string;
  to: string;
  granularity: TrendGranularity;
  available: TrendGranularity[];
}) {
  const router = useRouter();
  const unavailable = DISPLAY_ORDER.filter((g) => !available.includes(g));

  function handleChange(value: string) {
    const rangeParams = preset ? `range=${preset}` : `from=${encodeURIComponent(from)}&to=${encodeURIComponent(to)}`;
    router.push(`/lines/${lineId}/history?${rangeParams}&granularity=${value}`);
  }

  return (
    <Stack gap={4}>
      <SegmentedControl
        value={granularity}
        onChange={handleChange}
        data={DISPLAY_ORDER.filter((g) => available.includes(g)).map((g) => ({ label: LABELS[g], value: g }))}
      />
      {unavailable.length > 0 && (
        <Text size="xs" c="dimmed">
          {unavailable.map((g) => LABELS[g]).join(', ')} {unavailable.length === 1 ? 'is' : 'are'} not shown for this
          range -- it&apos;s wider than what&apos;s retained at that granularity, or would render too many points to
          read clearly.
        </Text>
      )}
    </Stack>
  );
}
```

- [ ] **Step 2: Write the test file**, mirroring
  `HistoryRangePicker.test.tsx`'s conventions:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GranularityControl } from './GranularityControl';

const push = vi.fn();
vi.mock('next/navigation', () => ({ useRouter: () => ({ push }) }));

describe('GranularityControl', () => {
  it('renders all four options when all four are available', () => {
    renderWithMantine(
      <GranularityControl
        lineId="northern"
        preset="7d"
        from="2026-08-14T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    for (const label of ['30 min', 'Hourly', '6-hourly', 'Daily']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.queryByText(/are not shown for this range/)).not.toBeInTheDocument();
  });

  it('omits unavailable tiers and names them in the dimmed note', () => {
    renderWithMantine(
      <GranularityControl
        lineId="northern"
        preset={null}
        from="2026-07-01T00:00:00Z"
        to="2026-08-10T00:00:00Z"
        granularity="day"
        available={['sixHour', 'day']}
      />,
    );
    expect(screen.queryByText('30 min')).not.toBeInTheDocument();
    expect(screen.queryByText('Hourly')).not.toBeInTheDocument();
    expect(screen.getByText('6-hourly')).toBeInTheDocument();
    expect(screen.getByText(/30 min, Hourly are not shown for this range/)).toBeInTheDocument();
  });

  it('navigates with the preset and the new granularity when a preset range is active', () => {
    renderWithMantine(
      <GranularityControl
        lineId="northern"
        preset="30d"
        from="2026-07-22T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    fireEvent.click(screen.getByText('Hourly'));
    expect(push).toHaveBeenCalledWith('/lines/northern/history?range=30d&granularity=hour');
  });

  it('navigates with the raw from/to when a custom range is active (no preset)', () => {
    renderWithMantine(
      <GranularityControl
        lineId="northern"
        preset={null}
        from="2026-07-22T00:00:00Z"
        to="2026-08-21T00:00:00Z"
        granularity="day"
        available={['halfHour', 'hour', 'sixHour', 'day']}
      />,
    );
    fireEvent.click(screen.getByText('30 min'));
    expect(push).toHaveBeenCalledWith(
      '/lines/northern/history?from=2026-07-22T00%3A00%3A00Z&to=2026-08-21T00%3A00%3A00Z&granularity=halfHour',
    );
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- GranularityControl.test.tsx
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/lines/\[id\]/history/GranularityControl.tsx frontend/app/lines/\[id\]/history/GranularityControl.test.tsx
git commit -m "frontend: add GranularityControl, the Trends-tab-scoped granularity picker"
```

---

## Task 10: Wire it all together in `page.tsx` (Decisions 6, 7, 8)

**Files:**
- Modify: `frontend/app/lines/[id]/history/page.tsx`.
- Modify: `frontend/app/lines/[id]/history/page.test.tsx`.

Depends on Tasks 6, 8, 9 (all three land here). This is the final
integration task.

**Interfaces:**
- Consumes: `resolveGranularity`, `availableGranularities`, `granularityShortfallDays` (Task 6), `TrendsResults`'s new `granularity` prop (Task 8), `GranularityControl` (Task 9), `getHistoryRetention`'s two new fields (Task 3).

- [ ] **Step 1: Update imports.** Change:

```tsx
import { getHistoryRetention, getLineStatus, getLineStatusHistory } from '@/lib/api';
...
import { groupHistoryByDay, resolveRange, retentionShortfallDays } from '@/lib/history';
...
import { HistoryRangePicker } from './HistoryRangePicker';
import { TrendsResults } from './TrendsResults';
import { CoverageTrendsResults } from './CoverageTrendsResults';
```

  to:

```tsx
import { getHistoryRetention, getLineStatus, getLineStatusHistory } from '@/lib/api';
...
import {
  availableGranularities,
  granularityShortfallDays,
  groupHistoryByDay,
  resolveGranularity,
  resolveRange,
  retentionShortfallDays,
} from '@/lib/history';
...
import { GranularityControl } from './GranularityControl';
import { HistoryRangePicker } from './HistoryRangePicker';
import { TrendsResults } from './TrendsResults';
import { CoverageTrendsResults } from './CoverageTrendsResults';
```

- [ ] **Step 2: Replace `resolveHistoryRetentionDays` with a
  three-ceiling `resolveRetention`.** Current (lines 37-50):

```tsx
async function resolveHistoryRetentionDays(): Promise<number | null> {
  try {
    const { historyRetentionDays } = await getHistoryRetention();
    return historyRetentionDays;
  } catch (err) {
    console.warn('Could not resolve the history retention window; hiding the retention notice.', err);
    return null;
  }
}
```

  Replace with:

```tsx
/** The three real retention ceilings the Timeline/Trends tabs need, or
 * safe fallbacks if the fetch fails. `historyRetentionDays` keeps its
 * existing `null`-means-unknown/hide-the-banner semantics for the Timeline
 * tab (unchanged -- Non-goal of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
 * The two new fields (Decision 8) default to `0` on failure rather than
 * `null`: `resolveGranularity`/`availableGranularities` need concrete
 * numbers, and `0` (combined with `'day'` always being exempt from both
 * checks -- see `frontend/lib/history.ts`) collapses safely to "only Daily
 * is offered" -- the same "don't guess, degrade to the least you can
 * promise" posture the Timeline banner already takes, extended one step
 * further here (hide the choice too, not just the notice). */
async function resolveRetention(): Promise<{
  historyRetentionDays: number | null;
  dailyStatsRetentionDays: number;
  halfHourlyStatsRetentionHours: number;
}> {
  try {
    const retention = await getHistoryRetention();
    return {
      historyRetentionDays: retention.historyRetentionDays,
      dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
      halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
    };
  } catch (err) {
    console.warn('Could not resolve retention ceilings; hiding the retention notice and offering only Daily.', err);
    return { historyRetentionDays: null, dailyStatsRetentionDays: 0, halfHourlyStatsRetentionHours: 0 };
  }
}
```

- [ ] **Step 3: Widen `searchParams` and compute the new values in
  `LineHistoryPage`.** Current (lines 52-69):

```tsx
export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string }>;
}) {
  const { id } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, retentionDays] = await Promise.all([
    resolveLineName(id),
    resolveHistoryRetentionDays(),
  ]);
  const range = resolveRange(query, now);
  const shortfallDays = retentionShortfallDays(range, retentionDays, now);
```

  Replace with:

```tsx
export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string; granularity?: string }>;
}) {
  const { id } = await params;
  const query = await searchParams;

  const now = Date.now();
  const [name, retention] = await Promise.all([
    resolveLineName(id),
    resolveRetention(),
  ]);
  const range = resolveRange(query, now);
  const shortfallDays = retentionShortfallDays(range, retention.historyRetentionDays, now);

  const rangeWidthMs = Date.parse(range.to) - Date.parse(range.from);
  const ceilings = {
    dailyStatsRetentionDays: retention.dailyStatsRetentionDays,
    halfHourlyStatsRetentionHours: retention.halfHourlyStatsRetentionHours,
  };
  const available = availableGranularities(rangeWidthMs, ceilings);
  const granularity = resolveGranularity(query, rangeWidthMs, ceilings);
  const granularityShortfall = granularityShortfallDays(range, granularity, ceilings, now);
  const retentionDaysForGranularity =
    granularity === 'day' ? retention.dailyStatsRetentionDays : Math.floor(retention.halfHourlyStatsRetentionHours / 24);
```

- [ ] **Step 4: Mount `GranularityControl`, add the Trends-tab banner, and
  fold `granularity` into the Trends Suspense key.** Current Trends
  `TabsPanel` (lines 137-157):

```tsx
        <TabsPanel value="trends">
          <Stack gap="md" pt="md">
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
              <TrendsResults id={id} from={range.from} to={range.to} />
            </Suspense>
            {/* ... */}
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
              <CoverageTrendsResults id={id} from={range.from} to={range.to} />
            </Suspense>
          </Stack>
        </TabsPanel>
```

  Replace with:

```tsx
        <TabsPanel value="trends">
          <Stack gap="md" pt="md">
            <GranularityControl
              lineId={id}
              preset={range.preset}
              from={range.from}
              to={range.to}
              granularity={granularity}
              available={available}
            />
            {/* Sub-daily-aware sibling of the Timeline tab's own shortfall
                banner (Decision 8) -- only non-null when the CURRENTLY
                SELECTED tier's own real retention ceiling doesn't reach
                back to range.from. A custom range can still outrun a
                35-day sub-daily retention or a 300-day daily one even
                though GranularityControl already hides tiers that can't
                cover the FULL range -- this covers the case where the
                selected tier partially, not fully, exceeds its ceiling. */}
            {granularityShortfall !== null && (
              <Alert color="yellow" variant="light" title="Some of this range isn't available at this granularity">
                This server only keeps {retentionDaysForGranularity}{' '}
                {retentionDaysForGranularity === 1 ? 'day' : 'days'} of data at this granularity. The oldest{' '}
                {granularityShortfall} {granularityShortfall === 1 ? 'day' : 'days'} of the range you picked has
                already been removed — if this range looks empty or short, that may be why, not because nothing
                happened.
              </Alert>
            )}
            <Suspense
              key={`${granularity}-${range.preset ?? `${range.from}-${range.to}`}`}
              fallback={<Skeleton height={320} />}
            >
              <TrendsResults id={id} from={range.from} to={range.to} granularity={granularity} />
            </Suspense>
            {/* Decision 4's daily full-coverage series -- unaffected by
                granularity (Non-goal of
                docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md),
                so its own Suspense key is deliberately left unchanged. */}
            <Suspense key={range.preset ?? `${range.from}-${range.to}`} fallback={<Skeleton height={320} />}>
              <CoverageTrendsResults id={id} from={range.from} to={range.to} />
            </Suspense>
          </Stack>
        </TabsPanel>
```

- [ ] **Step 5: Update `page.test.tsx`'s mock.** The `getHistoryRetention`
  mock (line 81) must now resolve the two new required fields:

```tsx
vi.mocked(api.getHistoryRetention).mockResolvedValue({
  historyRetentionDays: 7,
  dailyStatsRetentionDays: 300,
  halfHourlyStatsRetentionHours: 840,
});
```

  Add two new tests to the `describe('LineHistoryPage', ...)` block, after
  the existing "switching to the Trends tab renders the daily-stats charts"
  test:

```tsx
  it('the Trends tab shows a granularity control offering all four tiers by default', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyStatsRow({ day: '2026-08-30' })]);
    await renderPage();
    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    for (const label of ['30 min', 'Hourly', '6-hourly', 'Daily']) {
      expect(await screen.findByText(label)).toBeInTheDocument();
    }
  });

  it('a very wide custom range hides the sub-daily tiers and still renders the daily chart', async () => {
    vi.mocked(api.getLineDailyStats).mockResolvedValue([dailyStatsRow({ day: '2026-01-01' })]);
    await renderPage({ from: '2025-01-01T00:00:00Z', to: '2026-08-21T00:00:00Z' });
    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    expect(await screen.findByText('Daily')).toBeInTheDocument();
    expect(screen.queryByText('30 min')).not.toBeInTheDocument();
    expect(screen.getByText(/are not shown for this range/)).toBeInTheDocument();
  });
```

  (`dailyStatsRow` and `renderPage` are the file's existing helpers, lines
  39-61 — unchanged.)

- [ ] **Step 6: Verify**

```bash
cd frontend && npm test -- page.test.tsx
cd frontend && npm test
```

Expected: the whole frontend suite passes, including every test touched or
added by Tasks 5-10.

- [ ] **Step 7: Build and manually verify.**

```bash
cd frontend && npm run build
```

Expected: builds cleanly (this exercises the real Next.js/TypeScript
compiler across every file this plan touched, catching any type mismatch
`vitest` alone wouldn't). **A plan-executing agent should additionally
start the dev server (`npm run dev`), open a real `/lines/<some-line-id>/history`
page in a browser, switch to the Trends tab, and click through all four
granularity options against both the "Last 7 days" and "Last 30 days"
presets and a wide custom range** — confirming the chart re-fetches and
re-renders on each switch, the unavailable-tiers note appears for a range
wide enough to exceed the point budget, and the shortfall banner appears
for a range wider than the real backend's configured retention. This
planning pass does not perform that manual check itself.

- [ ] **Step 8: Commit**

```bash
git add frontend/app/lines/\[id\]/history/page.tsx frontend/app/lines/\[id\]/history/page.test.tsx
git commit -m "frontend: wire GranularityControl + resolveGranularity/availableGranularities into the history page"
```

---

## Self-review

- **Spec coverage**: Decision 1 (four tiers) — Task 6's `GRANULARITY_ORDER`/type. Decision 2 (one new query, two new routes, no new table) — Tasks 1-2. Decision 3 (retention bump) — Task 4. Decision 4 (one parameterized `TrendsResults`) — Task 8, with `TrendGranularity`'s shape resolved per Judgment call 1. Decision 5 (sparse floors) — Task 8's `SPARSE_FLOOR`. Decision 6 (control placement, URL state, Suspense keying) — Tasks 9-10. Decision 7 (auto-floor via disabling options) — Task 6's `availableGranularities`/`resolveGranularity`, rendered by Task 9. Decision 8 (retention echo + banner) — Tasks 3, 10.
- **Placeholder scan**: no task leaves a "TODO"/"add error handling"/"similar to Task N" instruction — every step above contains complete, literal code or an exact file-line edit.
- **Type consistency**: `TrendGranularity` is defined once (Task 6, `frontend/lib/history.ts`) and imported verbatim by `TrendsCharts.tsx` (Task 7), `TrendsResults.tsx` (Task 8), and `GranularityControl.tsx` (Task 9) — no second/collapsed type anywhere. `bucketStart` is used consistently as the JSON field name (backend Task 2) and the TypeScript field name (frontend Task 5) for both new sub-daily types. `GranularityRetentionCeilings` (Task 6) is the one shape threaded through `availableGranularities`, `resolveGranularity`, `granularityShortfallDays`, and `page.tsx`'s own `ceilings` local (Task 10) — no mismatched field names.
