# Design: Per-Line Historical Graphics (Cancellation/Skip/Delay Rate, Avg Delay)

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(the closest structural precedent — a scoped, code-verified frontend+backend
design doc, not a research-only survey) and, for how it handles
newly-discovered gaps against the brief's assumptions,
`docs/superpowers/specs/2026-08-30-inferred-time-ranges-design.md`. No
implementation plan is included; that is a separate, later step in this
repo's process. Every sketch below (schema, route shapes, component tree,
chart config) is marked as a sketch — not final code.

## Goal

The repo owner wants per-line historical graphics: cancellation rate,
skipped-stop rate, average delay time, and delay rate, over time. Today
`/lines/[id]/history` shows only a chronological, badge-based incident
timeline (`frontend/lib/history.ts`'s `groupHistoryByDay`) — no charts, no
rates, no aggregates. `common::SampleStats` already carries the raw
ingredients for all four metrics, computed fresh by the aggregator roughly
every poll cycle, but — as this research found — it is **not currently
persisted anywhere as a real time series**: the one history table that
exists (`line_status_history`) explicitly strips `sample_stats` before
deciding whether to write a row at all, so it is sparse, irregular, and
frequently absent exactly when a metric wouldn't be changing anyway. This
spec's core job is deciding what new backend storage/aggregation makes
these four metrics chartable at all, and only then what the chart
page/library/UI looks like.

## Corrections to the brief's assumptions (recorded for posterity)

Following `2026-08-30-inferred-time-ranges-design.md`'s own "Corrections"
precedent: direct inspection of the code turned up several things the
brief didn't establish, materially affecting the design below.

1. **`line_status_history` is not a `sample_stats` time series and cannot
   be read as one.** `crates/aggregator/src/queries.rs`'s `write_line_status`
   only inserts a history row when `normalize_for_diff(existing) !=
   normalize_for_diff(fresh)` — and `normalize_entry_for_diff` (line
   179-181) explicitly does `obj.remove("sample_stats")` before that
   comparison, on top of stripping `validity.from_date` and the
   live-sample-count reason suffix. **`sample_stats` churning is
   deliberately excluded from "did this line's status change."** So a
   history row's `sample_stats` is only ever a snapshot of whatever the
   numbers happened to be at the moment some *unrelated* field (severity,
   reason text, disruption) changed — never a regular sample. Real,
   measured volume from `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`:
   WCML had "295 status recomputes across 110 incidents" over 8 days
   (~37 rows/day); SWR-Alton had 127/8 days (~16 rows/day) — sparse and
   irregular, not a substitute for a real cadence. Building rate-over-time
   charts by reading `line_status_history` on every page load is not
   viable as-is; something new has to persist the numbers.
2. **The brief's framing ("Knowledgebase-derived lines never accumulate
   much `SampleStats` history") conflates two different things.**
   `compute_sample_stats` (`crates/aggregator/src/aggregation.rs:675-712`)
   is called **once per line per cycle regardless of `data_quality`** and
   is attached to every status entry the line currently has — including
   `Knowledgebase`/`Planned` ones (`aggregation.rs:89-106`: "attached as
   supplementary `sample_stats` on top of the incident-derived
   status(es)"). The gate is `relevant.len() >= thresholds.min_sample_size`
   (default `3`, `crates/common/src/lib.rs`) live LDBWS departures at the
   line's configured `sample_stations` — nothing to do with whether the
   line's *incident text* happens to read as Knowledgebase-derived. A line
   that is 100% Knowledgebase-worded (SWR-Alton, per the validation
   findings) can still have real, current `sample_stats` on every one of
   its statuses. The real sparse-data risk is a line with few/no
   `sample_stations` configured or too little live LDBWS traffic to clear
   `min_sample_size` — not `data_quality`. See Decision 3.
3. **`sample_stats` is not exclusive to the aggregator/national-rail
   pipeline — DLR (a TfL mode) has its own, separate source of it.**
   `crates/poller-tfl/src/main.rs`'s `poll_dlr_sample_stats`/
   `merge_dlr_sample_stats` attach a real `common::SampleStats` to DLR's
   line status; every other TfL mode (tube, overground, elizabeth-line,
   tram) hardcodes `sample_stats: None`
   (`crates/poller-tfl/src/schema.rs:148`, and
   `poller-tfl/src/config.rs:58`'s own comment: "DLR reports `sample_stats:
   None`, same as every other TfL line" — describing the *other* lines,
   not DLR itself). This write goes through a completely different path —
   `crates/api/src/data/queries.rs`'s `upsert_tfl_line_status`, called from
   `/private/tfl-line-status`, not the aggregator's `write_line_status` —
   though it does write its own `line_status_history` rows with the same
   `source='tfl'` scoping. This spec deliberately scopes to national-rail
   lines only (Decision 1); DLR is a real, symmetrical but separate
   follow-up, not silently covered here. See Explicitly out of scope.
4. **The existing history page's 30-day preset already silently truncates
   against the 7-day retention ceiling, independent of this feature.**
   `crates/aggregator/src/config.rs`'s `history_retention_days` defaults to
   `7`, enforced by `queries::prune_history`. `HistoryRangePicker.tsx`
   already offers a "Last 30 days" button today with no indication that
   only the most recent 7 days of it can possibly have data. This is a
   pre-existing gap this spec did not introduce, but the new Trends tab
   proposed below sits right next to it and makes the inconsistency more
   visible (its own rollup data can genuinely cover 30+ days once deployed
   for that long) — see Decision 5.
5. **`SampleStats.total` is not "N distinct trains" — it's "N departures
   currently visible in this poll's LDBWS response window."** Darwin's
   departure-board response returns a rolling window of upcoming services
   at a station; the same physical service (`StationDeparture.service_id`)
   is very likely present across many consecutive ~60s polls until it
   leaves the window. Summing raw per-cycle counts across a day therefore
   weights each service by how long it dwelt in the window, not by "one
   train, one count" — a delayed service sitting in the window for 20
   minutes is sampled roughly 20 times at the default 60s poll interval.
   This is not something the brief anticipated, and it materially affects
   what the rolled-up "rate" actually means. See Decision 2.

## Current relevant state (verified 2026-08-31)

**`common::SampleStats`** (`crates/common/src/lib.rs:664-670`):
```rust
pub struct SampleStats {
    pub total: usize,
    pub delayed: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub avg_delay_minutes: f64,
}
```
`avg_delay_minutes` is averaged over non-cancelled ("running") departures
only. Computed by `compute_sample_stats` (`aggregation.rs:675-712`) from
LDBWS `StationDeparture`s at the line's `sample_stations`, independent of
`data_quality`, gated on `min_sample_size` (default `3`). Attached to
**every** `LineStatus` a line currently has, every aggregation cycle
(default `poll_interval_secs = 60`). It is present on `line_status`
(current-state, overwritten every cycle) but, per Correction 1, only
incidentally present in `line_status_history`.

**Schema today** (`crates/api/migrations/20260510023522_initial.sql:89`
and later migrations):
- `line_status` — one row per `line_id`, `statuses` JSONB fully replaced
  every cycle, `source` column (`'aggregator'` or `'tfl'`).
- `line_status_history` — append-only, `(line_id, computed_at DESC)`
  indexed, written only on a real status change (Correction 1), pruned by
  `history_retention_days` (default `7`).
- No table anywhere stores a regular-cadence, long-running record of
  `sample_stats` on its own.

**Aggregator config** (`crates/aggregator/src/config.rs`): `poll_interval_secs`
(default `60`), `history_retention_days` (default `7`) — both CLI/env
configurable, no separate knob exists yet for anything this spec proposes.
`chrono-tz = "0.10"` is already a dependency of the `aggregator` crate, and
`aggregation.rs` already has a working Europe/London local-time pattern
(`next_rail_day_boundary`, using `chrono_tz::Europe::London` +
`from_local_datetime`) — though that function computes a **rail-day 02:00
cutoff**, a different boundary from the plain calendar-day one this spec
needs (see Decision 1).

**Existing history route** (`crates/api/src/routes/line_status.rs:39,229-252`):
`GET /Line/{id}/Status/{from}/to/{to}` — unauthenticated, matches TfL's own
URL scheme (unprefixed, merged directly onto the root router, per that
file's own module doc). Backed by
`crates/api/src/data/queries.rs:599-623`'s `line_status_history_for_range`,
a plain `SELECT statuses, computed_at FROM line_status_history WHERE
line_id = $1 AND computed_at BETWEEN $2 AND $3 ORDER BY computed_at` — no
aggregation, returns every row in range as-is, each wrapped through
`to_tfl_shape` into the same `LineStatusReport`-shaped JSON the live
status endpoints use. Fetched directly by
`frontend/lib/api.ts`'s `getLineStatusHistory(id, from, to)` — a plain
server-side `fetchJson` against `${baseUrl()}/Line/{id}/Status/{from}/to/{to}`,
**no cookie-forwarding, no proxy** — this route needs neither, since it's
public. Any new stats route can follow the exact same pattern: a
Server-Component-only fetch straight to the backend, no
`app/api/[...path]/route.ts` allowlist change needed (that proxy exists
only for browser-initiated, same-origin requests, none of which this
feature needs).

**Existing history page** (`frontend/app/lines/[id]/history/page.tsx`):
resolves the line name, resolves a range via `resolveRange` (URL params
`from`/`to`/`range`, defaulting to the `7d` preset), renders
`HistoryRangePicker` (client component, presets + a Mantine
`DatePickerInput` range, navigates by pushing a new URL — the range lives
in the URL, not local-only state), then a `Suspense`-wrapped
`HistoryResults` that fetches `getLineStatusHistory` and renders
`groupHistoryByDay`'s output as one section per London calendar day, each
containing badge rows per collapsed incident span. `groupHistoryByDay`
(`frontend/lib/history.ts`) buckets by `londonDayKey` — a plain
`Europe/London` **calendar-day** boundary (midnight, via
`Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/London', ... })`,
`frontend/lib/dateFormat.ts:38-43`) — not the aggregator's rail-day 02:00
cutoff. This is the convention Decision 1 follows for the new rollup, for
consistency with what users already see grouped on this exact page.

**Frontend charting**: confirmed no charting dependency exists today
(`frontend/package.json` has only `@mantine/core`/`@mantine/dates`/
`@mantine/hooks`, all pinned `^9.4.1`; `react`/`react-dom` pinned `^19.0.0`).
See Decision 6 for the verified real-world compatibility check.

## Decisions

### 1. New table + new aggregator write path — a daily rollup, not a reuse of `line_status_history`

Per Correction 1, `line_status_history` cannot serve as the raw material
for rate-over-time charts: its dedup logic exists specifically to make
`sample_stats` churn *not* trigger a write, so most of a day's actual
`sample_stats` values were simply never recorded anywhere. Two options
were considered:

- **(a)** Stop stripping `sample_stats` from the diff, so every cycle's
  numbers get their own history row. Rejected: this repurposes a table
  whose whole existing contract (an incident-change audit log, "useful for
  debugging regressions and building a status-over-time view" per its own
  migration comment) is built around *not* churning every cycle — every
  existing consumer (the Timeline tab, `groupHistoryByDay`'s per-day
  incident collapsing) depends on that sparsity to stay readable and cheap
  to query. Turning every line into ~1,440 rows/day (at the default 60s
  cadence) would 40x the existing table's write volume and directly
  contradict the "changed" semantics `write_line_status`'s own tests
  assert.
- **(b) (chosen)** A new table, `line_status_daily_stats`, one row per
  `(line_id, day)`, written incrementally by a **new** aggregator query
  function called once per line per cycle (independent of whether
  `line_status_history` gets a row that cycle), accumulating running sums.
  `day` is the plain Europe/London calendar day of `computed_at` (matching
  `frontend/lib/history.ts`'s `londonDayKey`, not the aggregator's own
  rail-day-02:00 convention — chosen for consistency with what the
  adjacent Timeline tab already groups by, since Decision 5 puts both
  views on the same page).

Sketch (not final):
```sql
-- crates/api/migrations/20260831090000_line_status_daily_stats.sql
-- Owned by this migration file (run at `api` startup, same as every other
-- table) even though only the aggregator crate writes to it day-to-day --
-- matching how line_status/line_status_history themselves were defined in
-- the initial migration despite being aggregator-written.
CREATE TABLE line_status_daily_stats (
    line_id            TEXT        NOT NULL,
    day                DATE        NOT NULL,  -- Europe/London calendar day
    sample_cycles      BIGINT      NOT NULL DEFAULT 0,  -- how many poll
                                                          -- cycles contributed
                                                          -- data this day --
                                                          -- the "how much do
                                                          -- we actually have"
                                                          -- signal (Decision 3)
    total               BIGINT     NOT NULL DEFAULT 0,  -- sum of SampleStats.total
    delayed              BIGINT    NOT NULL DEFAULT 0,
    cancelled            BIGINT    NOT NULL DEFAULT 0,
    skipped              BIGINT    NOT NULL DEFAULT 0,
    running_count        BIGINT    NOT NULL DEFAULT 0,  -- sum of "running"
                                                          -- (non-cancelled)
                                                          -- departures --
                                                          -- denominator for
                                                          -- avg_delay_minutes
    delay_minutes_sum    DOUBLE PRECISION NOT NULL DEFAULT 0,  -- sum of
                                                          -- (avg_delay_minutes
                                                          -- * running count)
                                                          -- per cycle
    PRIMARY KEY (line_id, day)
);
CREATE INDEX line_status_daily_stats_line_day ON line_status_daily_stats (line_id, day);
```

```rust
// crates/aggregator/src/queries.rs -- sketch, not final.
//
// Called once per line per cycle from the aggregation loop (main.rs),
// only when `compute_sample_stats` returned `Some` for that line -- NOT
// once per `LineStatus` entry the line has, since `aggregate()` clones
// the same `SampleStats` onto every simultaneous status a line carries
// (aggregation.rs:104) and double-counting per-status would inflate every
// sum by however many concurrent incidents a line has.
pub async fn record_daily_stats(
    pool: &PgPool,
    line_id: &str,
    day: chrono::NaiveDate, // Europe/London calendar day of `now`
    stats: &common::SampleStats,
) -> Result<()> {
    let running = stats.total.saturating_sub(stats.cancelled) as i64;
    sqlx::query(
        "INSERT INTO line_status_daily_stats
            (line_id, day, sample_cycles, total, delayed, cancelled, skipped,
             running_count, delay_minutes_sum)
         VALUES ($1, $2, 1, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (line_id, day) DO UPDATE SET
            sample_cycles   = line_status_daily_stats.sample_cycles + 1,
            total           = line_status_daily_stats.total + EXCLUDED.total,
            delayed         = line_status_daily_stats.delayed + EXCLUDED.delayed,
            cancelled       = line_status_daily_stats.cancelled + EXCLUDED.cancelled,
            skipped         = line_status_daily_stats.skipped + EXCLUDED.skipped,
            running_count   = line_status_daily_stats.running_count + EXCLUDED.running_count,
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
```

**Retention is a separate, new config knob, not `history_retention_days`.**
The two tables serve different purposes — `line_status_history` is a
debugging/audit log deliberately kept short; this rollup exists
specifically to answer "how has this line trended over weeks/months," so a
7-day ceiling would defeat the point. Proposed:
`daily_stats_retention_days` (aggregator CLI/env flag, default
**unset/no pruning for v1** — see Open questions — or a generous default
like `400` if a number is required at ship time), enforced by a new
`prune_daily_stats` mirroring `prune_history`'s shape. Storage cost is
trivial either way: one row per `(line_id, day)` — at the catalogue's
current ~105 lines, a full year is ~38,325 rows total, not per line.

### 2. The rate this rollup produces is "share of sampled poll cycles," not "share of trains" — an explicit, named limitation

Per Correction 5, `SampleStats.total` counts departures currently visible
in the LDBWS window, and the same service is very likely recounted across
many consecutive polls. Summing raw counts across a day (as
`record_daily_stats` above does) inherits this: a service that sits
cancelled/delayed in the window for 20 minutes contributes roughly 20x the
weight of one that clears quickly. **This is accepted for v1, on the
grounds that:**

- It requires no new state beyond what `compute_sample_stats` already
  produces per cycle — no service-identity tracking, no persisted
  "already counted today" set.
- True per-service dedup is a real, larger, separate piece of work:
  `StationDeparture.service_id` exists and could in principle be used, but
  doing this correctly needs a per-day "seen service ids" ledger that
  survives aggregator restarts (an in-memory `HashSet` reset at
  process start would silently undercount after every restart/deploy) —
  effectively a new table and a new invalidation policy, not a small
  addition.
- The resulting number is still a real, meaningful, and honestly-labelable
  signal — "what fraction of this line's sampled poll cycles this day
  looked delayed/cancelled/skipping a stop" is a legitimate time-weighted
  proxy for disruption, just not identical to "what fraction of trains."

**Decision: label it as such, everywhere it's rendered.** Backend field
names avoid implying "trains" (`sample_cycles`, not `service_count`);
frontend copy says "share of sampled poll cycles were delayed," not "X% of
trains were late" (see Decision 7). `sample_cycles` is stored precisely so
the frontend can show "based on N poll samples across M cycles today" as a
transparency signal, and per-service dedup is flagged in Open
questions/risks as a real follow-up, not silently deferred.

### 3. Sparse/no-data handling: an honest gap in the chart, not an interpolated line — driven by `sample_cycles`, not `data_quality`

Per Correction 2, the brief's original framing (KB-quality lines have no
stats) doesn't match the code — the real gates are (a) whether a line has
`sample_stations` configured at all, and (b) whether it clears
`min_sample_size` (default `3`) live departures per cycle. Real sparse
cases that do exist:

- A line with no/few `sample_stations` in its catalogue TOML — **not
  individually audited across all 105 lines in this research pass**; flagged
  as needing a real check at implementation time (see Open questions).
- A low-frequency or overnight line dipping below `min_sample_size` for
  stretches of the day, producing a day with a nonzero-but-low
  `sample_cycles`.
- A brand-new custom line — the rollup only starts accumulating from
  whatever day it was created; there is no way to backfill days before
  that (see Open questions).
- Any line for the first N days after this feature itself ships — the
  rollup table starts empty; there is no historical archive to construct
  it from retroactively (per Correction 1, the only place raw per-cycle
  numbers ever existed was in-memory during each aggregation cycle).

**Design: every daily row carries `sample_cycles`, and the frontend uses
it as an explicit coverage signal, not just a display footnote.** Given
`poll_interval_secs` (default `60`), the maximum possible cycles in a day
is `86400 / poll_interval_secs` (1,440 at the default). Rule of thumb
proposed: a day with `sample_cycles` below some absolute floor (e.g. `20`
— roughly 20 minutes' worth of coverage at the default cadence) is
rendered as a **gap** in the chart (no point plotted, not a zero, not an
interpolated line across it) with a small dimmed marker a viewer can hover
for "limited data this day." A range with **zero** rows anywhere (new
line, feature just shipped, line genuinely has no `sample_stations`) does
not render an empty/misleading chart at all — it renders an explicit
"Not enough sampled data yet for this line" state, matching the brief's
explicit requirement for an honest not-enough-data state rather than a
graph built from a handful of samples. The exact floor value is a product
call, not something this research pass can settle from the code alone —
flagged in Open questions.

### 4. Backend read path: a new dedicated route over the rollup table, not folded into the existing history route

New route, mirroring the existing history endpoint's URL shape and
public/unauthenticated posture (`crates/api/src/routes/line_status.rs`):

```
GET /Line/{id}/Stats/{from}/to/{to}
```

`from`/`to` are `NaiveDate` (or `DateTime<Utc>` truncated server-side to
London calendar days — TBD at implementation, matching whichever is
simpler given the existing route already parses `DateTime<Utc>` path
segments successfully, per that file's own module doc). Backed by a new,
equally simple query:

```rust
// crates/api/src/data/queries.rs -- sketch, not final.
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

pub async fn daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyStatsRow>> {
    // SELECT ... FROM line_status_daily_stats
    // WHERE line_id = $1 AND day BETWEEN $2 AND $3 ORDER BY day
}
```

**Rate fields are computed at read time from the stored sums**, not
stored pre-divided — cheap arithmetic over at most a few hundred rows
(365 rows for a full year at daily granularity), no different in cost from
the existing history route's own per-row work. This directly answers the
brief's "does this need precomputed rollups" question: **yes — the daily
rollup table itself is the precomputation**, and no further on-read
aggregation over raw `line_status_history`/per-cycle data is needed or
advisable, since (per Correction 1) that raw data doesn't durably exist in
queryable form in the first place.

Response shape (camelCase, matching this crate's public JSON convention —
not run through `to_tfl_shape`, since this has no TfL analog to mimic,
unlike the four existing routes in this file):
```json
[
  {
    "day": "2026-08-24",
    "sampleCycles": 1387,
    "total": 4210,
    "delayed": 512,
    "cancelled": 38,
    "skipped": 91,
    "avgDelayMinutes": 6.4,
    "delayRate": 0.1216,
    "cancellationRate": 0.0090,
    "skipRate": 0.0216
  }
]
```
`avgDelayMinutes`/`delayRate`/`cancellationRate`/`skipRate` are derived:
`delayMinutesSum / runningCount`, `delayed / total`, `cancelled / total`,
`skipped / total` (guarding divide-by-zero — a day can have `total: 0` if
every contributing cycle itself had `total: 0`, though `min_sample_size`
makes this rare in practice, not impossible).

### 5. Frontend: a new "Trends" tab on the existing `/lines/[id]/history` page, sharing its range state

The existing Timeline content (badge list) and the new charts answer
different questions from the same underlying idea ("how has this line
behaved recently") and both need the exact same range-selection UX
`HistoryRangePicker` already provides — presets in the URL, a custom
`DatePickerInput` range, "the URL is the source of truth" (already fixed
once, per that component's own comments, from an earlier bug where the
picker and results could disagree). **Decision: keep one page, one URL,
one range picker; add a Mantine `Tabs` split ("Timeline" / "Trends")
beneath it**, rather than a new top-level route. Considered a fully
separate `/lines/[id]/trends` route; rejected because it would either
duplicate `HistoryRangePicker`'s range-in-URL logic under a second param
namespace, or force a decision about which page "owns" the canonical
range — neither is necessary when both views can read the same
`resolveRange` output and just fetch different data under it.

```tsx
// frontend/app/lines/[id]/history/page.tsx -- sketch, not final.
<Stack p="lg" gap="md">
  <TextLink href={`/lines/${id}`}>Back to line</TextLink>
  <Title order={1}>History: {name}</Title>
  <HistoryRangePicker lineId={id} preset={range.preset} from={range.from} to={range.to} />
  <Tabs defaultValue="timeline">
    <Tabs.List>
      <Tabs.Tab value="timeline">Timeline</Tabs.Tab>
      <Tabs.Tab value="trends">Trends</Tabs.Tab>
    </Tabs.List>
    <Tabs.Panel value="timeline">
      <Suspense fallback={<Skeleton height={240} />}>
        <HistoryResults id={id} from={range.from} to={range.to} />  {/* unchanged */}
      </Suspense>
    </Tabs.Panel>
    <Tabs.Panel value="trends">
      <Suspense fallback={<Skeleton height={320} />}>
        <TrendsResults id={id} from={range.from} to={range.to} />  {/* new */}
      </Suspense>
    </Tabs.Panel>
  </Tabs>
</Stack>
```

**Retention ceiling surfaced to the user.** Per Correction 4, the existing
30-day preset already silently truncates against the 7-day
`line_status_history` retention with no indication to the user — a
pre-existing gap, not introduced here. Because the new rollup has its own,
longer retention (Decision 1), the Trends tab's 30-day (and any longer)
preset can be genuinely meaningful once the feature has been live that
long, while the Timeline tab right next to it in the same `Tabs` cannot.
**Decision: recommend fixing the Timeline tab's silent truncation at the
same time**, even though it's not strictly caused by this spec — sitting
the two side by side in one `Tabs` control makes the inconsistency far
more visible than it is today (two different components, two different
pages a user would have to compare manually). Concretely: extend
`HistoryRangePicker` (or pass it as a prop) to know each tab's actual
retention ceiling and disable/gray a preset button beyond what that tab's
data source can possibly return, with a tooltip naming the real limit
("Only the last 7 days of timeline data is kept"). Flagged as a
recommended adjacent fix, not mandatory scope creep — see Explicitly out
of scope for the boundary drawn.

### 6. Charting library: `@mantine/charts` — verified current version, verified real (not assumed) API, real version-bump cost identified

Per the brief's instruction not to guess library APIs from memory, this
was checked against the live docs (`mantine.dev/charts/getting-started`)
and the published package (`unpkg.com/@mantine/charts@9.5.2/package.json`),
not recalled:

- **Current published version: `9.5.2`**, wraps Recharts directly ("Most
  of the components in the `@mantine/charts` package are based on the
  recharts library"). Exports `LineChart`, `BarChart`, `AreaChart`,
  `CompositeChart`, `DonutChart`, `PieChart`, and several others not
  relevant here.
- **Verified peer dependencies** (from the published `package.json`, not
  the docs prose): `@mantine/core: 9.5.2`, `@mantine/hooks: 9.5.2` (pinned
  to the exact same monorepo release — Mantine ships its packages in
  lockstep), `react: ^19.2.0`, `react-dom: ^19.2.0`, `recharts: >=3.2.1`.
- **This repo's current versions do not clear those peers as-is**:
  `@mantine/core`/`@mantine/dates`/`@mantine/hooks` are pinned `^9.4.1`
  (need bumping to `9.5.2` to match, since `@mantine/charts` pins its
  Mantine peers to an exact release rather than a broad range), and
  `react`/`react-dom` are pinned `^19.0.0` (need a floor bump to
  `^19.2.0`). Both are minor bumps within already-adopted majors — low
  expected risk — but this is real, concrete implementation work, not a
  drop-in install; flagged explicitly rather than assumed away. `recharts`
  itself needs adding as a new direct dependency (`@mantine/charts`
  declares it as a peer, not a transitive dependency it bundles).
- **No new test-infra gap.** `frontend/vitest.setup.ts` already polyfills
  `window.ResizeObserver` (for Mantine's own `SegmentedControl`/
  `FloatingIndicator`) — Recharts' `ResponsiveContainer` (which every
  `@mantine/charts` chart wraps its content in) needs exactly this same
  polyfill in jsdom, and it's already present. Confirmed no second
  polyfill is needed for chart component tests to render at all.
- **Chart type mapping for these four metrics**: delay rate, cancellation
  rate, and skip rate are all `0-1` proportions on the same axis — a
  single `LineChart` with three `series` (`dataKey`s `delayRate`,
  `cancellationRate`, `skipRate`), y-axis formatted as a percentage, is
  the natural fit (Recharts-style `series` prop, per `@mantine/charts`'
  `LineChart` API). Average delay minutes is a **different unit**
  (minutes, not a proportion) and would mislead if put on the same y-axis
  as the three rates — proposed as its own, second `LineChart` (or a
  `BarChart`, since "average minutes late per day" reads reasonably as
  discrete daily bars too; final choice is a UI-taste call, not something
  this research settles) directly beneath the rate chart, sharing the same
  x-axis domain (day) and the same gap-rendering behavior from Decision 3.
- No other charting option was seriously evaluated beyond confirming this
  one fits: the app is already fully committed to Mantine as its only
  component library (no competing UI kit anywhere in `frontend/`), and
  `@mantine/charts` is Mantine's own official package for this — bringing
  in an unrelated charting library (e.g. raw Recharts without the Mantine
  wrapper, or a completely different one like Chart.js/Nivo) would mean
  hand-rolling the Mantine theme integration `@mantine/charts` already
  provides for free (light/dark mode, Mantine's own color tokens, spacing
  scale) for no discernible benefit here.

### 7. Rendering rules and copy — stated explicitly, since Decision 2's limitation is easy to accidentally overstate in the UI

- Every rate is labelled as a rate **across sampled poll cycles**, e.g.
  "12.2% of sampled poll cycles this day showed a delayed service" — never
  phrased as "12.2% of trains were late," per Decision 2.
- Each chart point/tooltip surfaces `sampleCycles` for that day (e.g. "based
  on 1,387 poll samples") so a viewer can judge confidence themselves, not
  just trust a smoothed line.
- A day below the sparse-data floor (Decision 3) renders as a genuine gap
  — no interpolation across it, no zero plotted in its place — with a
  distinguishable marker/tooltip ("limited data this day").
- A line/range with zero qualifying rows renders the explicit
  "Not enough sampled data yet for this line" empty state instead of an
  empty or flat-zero chart.
- Average delay minutes is never plotted on the same axis as the three
  rate metrics (Decision 6).

## API/type contract

Hand-written, matching this repo's existing convention of not generating
types from the Rust source (per the journey-ticket-tracking-frontend
spec's own contract section):

```ts
// frontend/lib/types.ts additions -- sketch, not final.

/** `GET /Line/{id}/Stats/{from}/to/{to}`'s per-day response shape.
 * `delayRate`/`cancellationRate`/`skipRate` are fractions (0-1), computed
 * server-side from the stored sums -- see the backend design's Decision 4.
 * `sampleCycles` is the coverage signal Decision 3's sparse-data handling
 * depends on; render it, don't discard it. */
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

```ts
// frontend/lib/api.ts additions -- sketch, not final. Same shape as the
// existing getLineStatusHistory: public, no cookie-forwarding, no-store.
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

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                            │
│                                                                              │
│  app/lines/[id]/history/page.tsx     EXTENDED -- adds a Tabs split        │
│                                        (Timeline | Trends) beneath the     │
│                                        existing, unmodified                │
│                                        HistoryRangePicker                  │
│  app/lines/[id]/history/TrendsResults.tsx   NEW -- fetches                │
│                                        getLineDailyStats, renders the      │
│                                        two @mantine/charts LineCharts      │
│                                        (Decision 6/7)                      │
│  lib/api.ts    + getLineDailyStats                                        │
│  lib/types.ts  + LineDailyStats                                           │
│  package.json  BUMPED: @mantine/core|dates|hooks ^9.4.1 -> 9.5.2,          │
│                 react|react-dom ^19.0.0 -> ^19.2.0; ADDED: @mantine/charts,│
│                 recharts                                                   │
└──────────────────────────┬──────────────────────────────────────────────┘
     server-side fetch, no-store, no proxy needed (public route, same as
     the existing getLineStatusHistory)
                            ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ api crate                                                                  │
│  GET /Line/{id}/Stats/{from}/to/{to}   NEW route, line_status.rs           │
│  queries::daily_stats_for_range         NEW query, data/queries.rs         │
└──────────────────────────┬──────────────────────────────────────────────┘
                            │ reads
                            ▼
                 line_status_daily_stats   NEW table (migration owned by
                                            api crate, same as every other
                                            table -- Decision 1)
                            ▲
                            │ writes, once per line per cycle, whenever
                            │ compute_sample_stats returned Some
┌──────────────────────────┴──────────────────────────────────────────────┐
│ aggregator crate                                                          │
│  aggregate() (aggregation.rs)  -- unchanged computation, new call site    │
│  queries::record_daily_stats   NEW, queries.rs (Decision 1)               │
│  config::Config                + daily_stats_retention_days (new knob)    │
│  queries::prune_daily_stats    NEW, mirrors prune_history                 │
└────────────────────────────────────────────────────────────────────────┘
```

## Error handling

- `GET /Line/{id}/Stats/{from}/to/{to}` on an unknown `line_id` returns an
  empty array (matching `line_status_history_for_range`'s existing
  behavior for the same case — the existing history route does not `404`
  on an unknown line either, it just returns nothing), not a `404` — kept
  consistent with its closest sibling route rather than inventing new
  behavior.
- `TrendsResults`' own fetch failure (5xx, network) falls through to the
  existing root `app/error.tsx`, same as `HistoryResults` does today — no
  new error boundary.
- A day with `total: 0` (all contributing cycles had zero relevant
  departures — rare given `min_sample_size`, not impossible) has its rate
  fields computed as `0.0` server-side with an explicit guard, never a
  divide-by-zero/`NaN` reaching the frontend.

## Testing

Following this repo's existing convention:

- `crates/aggregator/src/queries.rs`: unit/integration tests for
  `record_daily_stats`'s upsert-accumulation (two cycles in the same day
  sum correctly; a cycle in a new day starts a fresh row; `sample_cycles`
  increments once per call) and `prune_daily_stats`, mirroring the
  existing `#[ignore]`d live-database test pattern already in this file.
- `crates/aggregator/src/aggregation.rs` or `main.rs` (wherever the new
  call site lands): a test confirming `record_daily_stats` is called
  **once per line per cycle**, not once per `LineStatus` entry, for a line
  with multiple simultaneous statuses — the exact double-counting failure
  mode Decision 1's sketch calls out.
- `crates/api/src/data/queries.rs`: `daily_stats_for_range` test (range
  filtering, ordering) plus a rate-computation test covering the
  `total: 0` guard.
- `crates/api/src/routes/line_status.rs`: a route test for the new
  endpoint, mirroring this file's existing `#[cfg(test)]` module
  structure (pure-function tests plus the existing pattern for
  constructing fixture rows).
- `frontend/lib/api.test.ts`: `getLineDailyStats` builds the correct range
  URL, mirroring the existing `getLineStatusHistory` test.
- `frontend/app/lines/[id]/history/TrendsResults.test.tsx`: render tests
  for the empty-state (zero rows), the sparse-data gap rendering (a day
  below the floor), and the normal multi-day chart case — using this
  repo's `renderWithMantine`/Vitest convention. Chart internals themselves
  (Recharts SVG output) are not asserted on beyond "did it render without
  throwing" — consistent with this repo not having any existing precedent
  for testing chart pixel output.

## Explicitly out of scope

- **DLR's separate `sample_stats` pipeline** (Correction 3). Extending
  this design to DLR/TfL lines needs a second call site for
  `record_daily_stats` inside `crates/api/src/data/queries.rs`'s
  `upsert_tfl_line_status` (or a shared helper both crates call) — real,
  symmetrical, but separate work, not covered here.
- **True per-service deduplication** of the cycle-weighted rate (Decision
  2) — would need a persisted, restart-surviving per-day "seen service
  ids" ledger; flagged as a real follow-up, not designed here.
- **Fixing the existing Timeline tab's silent 7-day truncation** beyond
  *surfacing* it via a disabled/greyed preset (Decision 5) — actually
  lengthening `history_retention_days` itself, or changing what the
  Timeline tab shows beyond 7 days, is a separate decision with its own
  storage-growth tradeoffs (recall `line_status_history` is NOT
  daily-rolled-up — it's one row per real status change, unbounded by
  design) not evaluated here.
- **Hourly (or finer) granularity.** Daily was chosen to match the
  existing history page's own day-based grouping and because it keeps
  storage trivial; an hourly table would be 24x the rows (still small in
  absolute terms) and is a straightforward but unevaluated future
  extension if a shorter (e.g. "last 24 hours") view is ever wanted.
- **Cross-line comparison charts, CSV/image export, or any
  alerting/notification on trend changes** — none of these were asked for
  and none are designed here.
- **Backfilling historical data from before this feature ships.** Per
  Correction 1, no raw archive of past `sample_stats` exists to backfill
  from — every line starts its rollup history from zero on deployment day.

## Open questions / risks

1. **`daily_stats_retention_days`'s actual default value is a real,
   unresolved product decision**, not something the code or existing
   config precedent settles. Storage is cheap at daily granularity either
   way (~38k rows/year for the whole current catalogue), so the ceiling is
   really about "how far back should the Trends tab ever be able to show,"
   not a technical constraint — needs a real answer before implementation,
   not a default picked arbitrarily by this spec.
2. **`sample_stations` coverage across the catalogue's 105 line TOML files
   was not individually audited in this research pass.** Decision 3's
   sparse-data handling is designed to degrade honestly regardless, but
   whether "most lines have good coverage" or "a meaningful chunk have
   none" changes how prominently the not-enough-data state needs to be
   designed for versus treated as an edge case — worth a real count at
   implementation time.
3. **The sparse-data floor (Decision 3's `sample_cycles` threshold, e.g.
   `20`) is a placeholder, not a validated number.** It should be checked
   against real `sample_cycles` distributions once the rollup has been
   running for a while, not shipped as a guess and never revisited.
4. **Decision 2's cycle-weighted rate, while labelled honestly, may still
   read as surprising to a user who assumes "rate" means "per train."**
   The copy in Decision 7 is written to head this off, but this is a
   genuine, not-fully-resolved product/UX tension between "technically
   accurate" and "immediately intuitive" — worth watching once shipped,
   same posture as the journey-ticket-tracking-frontend spec's own
   analogous open item about its Delay Repay ambiguity.
5. **Two different "day" conventions already coexist in this codebase**
   (this spec's plain Europe/London calendar day vs. `aggregation.rs`'s
   rail-day 02:00 cutoff, used for incident staleness). Deliberately using
   the calendar-day one here (Decision 1) for consistency with the
   adjacent Timeline tab's own grouping was a considered choice, not an
   oversight, but a late-night service near midnight will still bucket
   differently under this feature than it would under the rail-day
   convention used elsewhere in the aggregator — flagged in case this
   divergence ever needs reconciling.
6. **Whether `from`/`to` on the new route should be `NaiveDate` or
   `DateTime<Utc>` (matching the existing history route's path-segment
   type) is left as an implementation-time call** — the existing route
   already proved `DateTime<Utc>` parses correctly in a multi-segment
   `Path` extractor (per that file's own module doc), so reusing the exact
   same type would be the path of least resistance even though the new
   table's `day` column is a bare `DATE`; not resolved here.
