-- =============================================================================
-- Optional one-off data fix for DB2-8: shared `trains` rows whose
-- origin_crs / scheduled_departure disagree with their own schedule
-- =============================================================================
--
-- BACKGROUND
-- ----------
-- Before the DB2-8 fix, `schedule_matching::attempt_schedule_match` wrote the
-- PIN's boarding station and time onto the shared `trains` row as the train's
-- `origin_crs` / `scheduled_departure`. A pin can match at any calling point,
-- so a pin boarded mid-route recorded that stop as the train's origin, and
-- `find_or_create_train_with_schedule_match` COALESCEs those columns (first
-- writer wins), so no later writer ever corrected it. The fix now derives both
-- from the schedule's first calling point, exactly as
-- `schedule_destination_departures.true_origin_crs` is derived.
--
-- This script finds rows whose stored origin disagrees with their own
-- `calling_points[0]` and, only when asked, rewrites them to match.
--
-- WHAT IT COMPARES
-- ----------------
-- * `origin_crs` against the CRS of `calling_points[0].tiploc` (tiploc_crs
--   first, then stanox_crs, as `queries::crs_for_tiploc` does). Rows whose
--   first TIPLOC has no CRS, or only an X-prefixed pseudo-CRS, are left alone.
-- * `scheduled_departure` against `calling_points[0].bookedDeparture` on
--   `service_date + dayOffset`, Europe/London local time.
--
-- PRODUCTION CHECK (2026-09-27, read-only)
-- ----------------------------------------
-- 174 schedule-matched `trains` rows; 3 disagree (2 wrong origin_crs, 3 wrong
-- scheduled_departure), all service_date 2026-09-24, none with a subscriber.
-- They will age out with `trains` retention. Running the fix is optional.
--
-- USAGE
-- -----
-- Preview (read-only, the default):
--   psql "$DATABASE_URL" -f scripts/remediate-db2-8-train-origin.sql
-- Apply (one transaction, prints the rows it changed):
--   psql "$DATABASE_URL" -v apply=1 -f scripts/remediate-db2-8-train-origin.sql
-- Idempotent: a second run finds nothing.
-- Test harness: scripts/test-remediate-db2-8-train-origin.sh
-- =============================================================================

\set ON_ERROR_STOP on

\if :{?apply}
BEGIN;
SET LOCAL lock_timeout = '5s';
WITH candidates AS (
    WITH firsts AS (
        SELECT tr.id,
               tr.train_uid,
               tr.service_date,
               tr.origin_crs,
               tr.scheduled_departure,
               UPPER(TRIM(tr.calling_points -> 0 ->> 'tiploc')) AS first_tiploc,
               ((tr.service_date + COALESCE((tr.calling_points -> 0 ->> 'dayOffset')::int, 0))
                   + (tr.calling_points -> 0 ->> 'bookedDeparture')::time)
                   AT TIME ZONE 'Europe/London' AS first_departure
        FROM trains tr
        WHERE jsonb_typeof(tr.calling_points) = 'array'
          AND jsonb_array_length(tr.calling_points) > 0
    ), resolved AS (
        SELECT f.*,
               (SELECT UPPER(m.crs)
                  FROM (SELECT crs, 1 AS priority FROM tiploc_crs WHERE tiploc = f.first_tiploc
                        UNION ALL
                        SELECT crs, 2 AS priority FROM stanox_crs WHERE tiploc = f.first_tiploc) m
                 ORDER BY m.priority
                 LIMIT 1) AS first_crs
        FROM firsts f
    )
    SELECT r.id,
           r.train_uid,
           r.service_date,
           r.origin_crs,
           CASE WHEN r.first_crs NOT LIKE 'X%' THEN r.first_crs END AS new_origin_crs,
           r.scheduled_departure,
           r.first_departure AS new_scheduled_departure,
           (SELECT count(*) FROM train_subscriptions s WHERE s.trains_id = r.id) AS subscribers
    FROM resolved r
    WHERE (r.first_crs NOT LIKE 'X%' AND r.origin_crs IS DISTINCT FROM r.first_crs)
       OR (r.first_departure IS NOT NULL AND r.scheduled_departure IS DISTINCT FROM r.first_departure)
)
UPDATE trains t
   SET origin_crs = COALESCE(c.new_origin_crs, t.origin_crs),
       scheduled_departure = COALESCE(c.new_scheduled_departure, t.scheduled_departure)
  FROM candidates c
 WHERE t.id = c.id
RETURNING t.id, t.train_uid, t.service_date, c.origin_crs AS old_origin_crs, t.origin_crs,
          c.scheduled_departure AS old_scheduled_departure, t.scheduled_departure;
COMMIT;
\else
WITH candidates AS (
    WITH firsts AS (
        SELECT tr.id,
               tr.train_uid,
               tr.service_date,
               tr.origin_crs,
               tr.scheduled_departure,
               UPPER(TRIM(tr.calling_points -> 0 ->> 'tiploc')) AS first_tiploc,
               ((tr.service_date + COALESCE((tr.calling_points -> 0 ->> 'dayOffset')::int, 0))
                   + (tr.calling_points -> 0 ->> 'bookedDeparture')::time)
                   AT TIME ZONE 'Europe/London' AS first_departure
        FROM trains tr
        WHERE jsonb_typeof(tr.calling_points) = 'array'
          AND jsonb_array_length(tr.calling_points) > 0
    ), resolved AS (
        SELECT f.*,
               (SELECT UPPER(m.crs)
                  FROM (SELECT crs, 1 AS priority FROM tiploc_crs WHERE tiploc = f.first_tiploc
                        UNION ALL
                        SELECT crs, 2 AS priority FROM stanox_crs WHERE tiploc = f.first_tiploc) m
                 ORDER BY m.priority
                 LIMIT 1) AS first_crs
        FROM firsts f
    )
    SELECT r.id,
           r.train_uid,
           r.service_date,
           r.origin_crs,
           CASE WHEN r.first_crs NOT LIKE 'X%' THEN r.first_crs END AS new_origin_crs,
           r.scheduled_departure,
           r.first_departure AS new_scheduled_departure,
           (SELECT count(*) FROM train_subscriptions s WHERE s.trains_id = r.id) AS subscribers
    FROM resolved r
    WHERE (r.first_crs NOT LIKE 'X%' AND r.origin_crs IS DISTINCT FROM r.first_crs)
       OR (r.first_departure IS NOT NULL AND r.scheduled_departure IS DISTINCT FROM r.first_departure)
)
SELECT * FROM candidates ORDER BY service_date, id;
\endif
