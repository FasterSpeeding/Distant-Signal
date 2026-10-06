-- =============================================================================
-- One-off backfill: mark incidents the Knowledgebase feed already stopped
-- listing as "Ended (no longer listed)"
-- =============================================================================
--
-- REVIEWED, NOT YET RUN. Design:
-- docs/superpowers/specs/2026-10-06-incident-source-removal-design.md.
--
-- BACKGROUND
-- ----------
-- RDM's Knowledgebase feed purges incidents nightly (~22:57-23:00 UTC)
-- whatever their state, and never clears planned ones. Distant Signal read
-- "active" as `NOT incidents.is_cleared`, so every incident that left the
-- feed without `ClearedIncident=true` stayed active forever: 407 rows on
-- 2026-10-06 (9 unplanned, 398 planned).
--
-- Migration 20261006120000_incidents_source_removed.sql added
-- `incidents.source_removed_at` / `source_missing_polls`, and the api now
-- marks a row ended once it is missing from 2 consecutive COMPLETE poller
-- snapshots (`crates/api/src/data/incident_removal.rs`). That inference
-- also catches these old rows by itself, about three complete polls
-- (~15 minutes) after the new api and poller are both live: the first
-- complete snapshot only records the size baseline, the next two count the
-- misses. This script just does it at once, for the rows we already know
-- are gone, with the same result the inference would write.
--
-- WHAT IT DOES
-- ------------
-- Marks every incident that is not cleared, not already ended, and was last
-- listed (`fetched_at`) more than 30 minutes before the last complete
-- snapshot (`incident_feed_state.last_complete_at`):
--   source_removed_at    = fetched_at  (when the feed last listed it; the
--                                       same value the inference writes)
--   source_missing_polls = 2           (as if the inference had marked it)
-- A row the feed still lists has `fetched_at` within one poll (5 minutes)
-- of `last_complete_at`, so the 30-minute margin keeps every listed row out.
-- Planned and unplanned rows are treated the same.
--
-- EXPECTED SCOPE
-- --------------
-- About 407 rows as of 2026-10-06 (9 unplanned + 398 planned), plus
-- whatever the nightly purges have added since. If the dry-run count is far
-- from that (say, over a thousand, or anywhere near the size of the live
-- feed), stop and investigate before applying.
--
-- WHEN TO RUN
-- -----------
-- After the migration, the api and the poller-incidents image that sends
-- `complete` have all deployed AND at least one complete snapshot has
-- landed: `incident_feed_state` must have its row (the dry run prints it).
-- With no row the UPDATE matches nothing, by construction. Run it between
-- poller cycles (the poller logs "posted incidents to ingestion API" every
-- 5 minutes); the transaction is short either way.
--
-- REVERSIBLE
-- ----------
-- Nothing is deleted. The next poll resets `source_removed_at` to NULL and
-- `source_missing_polls` to 0 for every incident the feed still lists, so a
-- row marked wrongly un-ends itself within 5 minutes. To undo it by hand
-- (this also un-ends rows the api's own inference marked, which it then
-- marks again after two more complete polls):
--   UPDATE incidents SET source_removed_at = NULL, source_missing_polls = 0
--    WHERE source_removed_at IS NOT NULL;
--
-- HOW TO RUN
-- ----------
-- Dry run (read-only, the default):
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 \
--        -f scripts/backfill-2026-10-06-incident-source-removed.sql
-- Apply (dry run first, then the UPDATE in one short transaction):
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -v apply=1 \
--        -f scripts/backfill-2026-10-06-incident-source-removed.sql
-- In production, the same through the Postgres pod, the file on stdin:
--   kubectl -n distant-signal exec -i distant-signal-postgres-0 -- \
--     psql -U distant_signal -d distant_signal -v ON_ERROR_STOP=1 \
--     < scripts/backfill-2026-10-06-incident-source-removed.sql
-- adding `-v apply=1` only after reviewing the dry run's count and sample.
--
-- The UPDATE between the APPLY markers below is also run by
-- `incident_removal::db_tests::the_backfill_script_marks_only_long_unlisted_rows`
-- (crates/api/src/data/incident_removal.rs), so keep psql meta-commands
-- out of that block.
-- =============================================================================

-- ---------------------------------------------------------------------------
-- 1. Dry run: what would be marked. Read-only.
-- ---------------------------------------------------------------------------
SELECT last_complete_at, last_complete_size
  FROM incident_feed_state
 WHERE singleton;

SELECT count(*)                                AS would_mark,
       count(*) FILTER (WHERE is_planned)      AS planned,
       count(*) FILTER (WHERE NOT is_planned)  AS unplanned,
       min(fetched_at)                         AS oldest_last_listed,
       max(fetched_at)                         AS newest_last_listed
  FROM incidents
 WHERE NOT is_cleared
   AND source_removed_at IS NULL
   AND fetched_at < (SELECT last_complete_at FROM incident_feed_state WHERE singleton)
                    - interval '30 minutes';

SELECT incident_id, is_planned, priority, first_seen_at, fetched_at,
       left(summary, 80) AS summary
  FROM incidents
 WHERE NOT is_cleared
   AND source_removed_at IS NULL
   AND fetched_at < (SELECT last_complete_at FROM incident_feed_state WHERE singleton)
                    - interval '30 minutes'
 ORDER BY fetched_at DESC
 LIMIT 20;

\if :{?apply}
-- ---------------------------------------------------------------------------
-- 2. Apply: one short transaction.
-- ---------------------------------------------------------------------------
-- BEGIN APPLY
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

UPDATE incidents
   SET source_removed_at = fetched_at,
       source_missing_polls = 2
 WHERE NOT is_cleared
   AND source_removed_at IS NULL
   AND fetched_at < (SELECT last_complete_at FROM incident_feed_state WHERE singleton)
                    - interval '30 minutes';

COMMIT;
-- END APPLY

SELECT count(*) AS ended_now
  FROM incidents
 WHERE source_removed_at IS NOT NULL;
\else
\echo 'Dry run only; nothing changed. Re-run with -v apply=1 to apply.'
\endif
