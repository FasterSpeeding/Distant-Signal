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
