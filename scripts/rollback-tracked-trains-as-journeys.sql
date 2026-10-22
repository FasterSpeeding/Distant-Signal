-- Undo migration 20261022090000_tracked_trains_as_journeys (DS-124,
-- docs/superpowers/specs/2026-10-21-tracked-trains-as-journeys-design.md
-- §5.3). Only before phase 3, which drops the bookkeeping this reads.
--
-- Run as the schema owner, in one transaction, after rolling the api back
-- to a release before the migration:
--
--   psql "$MIGRATION_DATABASE_URL" -v ON_ERROR_STOP=1 --single-transaction \
--       -f scripts/rollback-tracked-trains-as-journeys.sql
--
-- 1. Each moved group share goes back to group_trains, and the journey
--    share the move made (not one the user had made themselves) goes.
-- 2. Each converted journey goes while it is still exactly the one leg
--    the migration made, on the same train (one the user has since added
--    a leg to, or changed the train of, is theirs now and stays), with its
--    share links. The subscription stays: the FK is on the leg, and its
--    tracked_standalone is still as it was.
-- Rows whose journey, train or group has gone since were cascaded away
-- with it and are not restored. Rerunning finds nothing left to undo.

INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at)
SELECT m.group_id, m.train_subscription_id, m.added_by, m.added_at
FROM group_train_share_moves m
WHERE EXISTS (SELECT 1 FROM users u WHERE u.id = m.added_by)
ON CONFLICT (group_id, train_subscription_id) DO NOTHING;

DELETE FROM group_journeys gj
USING group_train_share_moves m
WHERE m.created_share AND gj.group_id = m.group_id AND gj.journey_id = m.journey_id;

DELETE FROM group_train_share_moves;

-- One statement, no temporary table: the owner role may not create one.
WITH doomed AS (
    SELECT c.journey_id
    FROM standalone_track_conversions c
    WHERE (SELECT COUNT(*) FROM journey_legs jl WHERE jl.journey_id = c.journey_id) = 1
      AND EXISTS (
          SELECT 1 FROM journey_legs jl
          WHERE jl.journey_id = c.journey_id
            AND jl.train_subscription_id = c.train_subscription_id
      )
), links AS (
    DELETE FROM unlisted_links
    WHERE resource_type = 'journey'
      AND resource_id IN (SELECT journey_id::text FROM doomed)
)
DELETE FROM journeys WHERE id IN (SELECT journey_id FROM doomed);

DELETE FROM standalone_track_conversions;
