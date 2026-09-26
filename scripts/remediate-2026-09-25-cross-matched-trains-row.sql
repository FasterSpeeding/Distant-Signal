-- =============================================================================
-- Remediation: unglue the 2026-09-25 cross-matched `trains` row(s)
-- =============================================================================
--
-- BACKGROUND
-- ----------
-- On 2026-09-25 a CRS+time matching heuristic (both the live TRUST-consumer
-- path in `crates/trust-consumer/src/matching.rs` and the backlog-replay path
-- in `crates/api/src/data/trust_event_backlog_match.rs`) picked the wrong
-- candidate under ambiguity, and glued an unrelated Avanti Euston->Liverpool
-- service's (train_uid `W34058`) live TRUST movements onto the shared
-- `trains` row for a user's London Northwestern Euston->Birmingham service
-- (train_uid `Y80926`, service_date `2026-09-25`, `trains.id = 6095140` at
-- the time this was found). Concretely: that `trains` row's `train_uid`
-- stayed `Y80926` (correct -- it came from a real CIF schedule match) but its
-- `train_id` (TRUST's own daily identifier, and everything derived from it --
-- `train_movement_events`/`train_current_state`) actually belongs to
-- `W34058`.
--
-- Both write paths that could cause this have since been patched with an
-- application-level veto (`is_provable_identity_contradiction` in both
-- modules named above), and a schema-level backstop
-- (`crates/api/migrations/20260925222000_trains_train_id_service_date_unique.sql`,
-- `UNIQUE (train_id, service_date) WHERE train_id IS NOT NULL`) makes this
-- whole bug class structurally impossible going forward. Neither of those
-- fixes touches EXISTING corrupted data -- that migration's own header
-- comment says so explicitly, and names the risk this script exists to
-- retire: if the real W34058 service was ALSO independently tracked and
-- correctly resolved by someone else, its `trains` row would carry the SAME
-- `train_id` the corrupted row wrongly grabbed, and
-- `CREATE UNIQUE INDEX CONCURRENTLY` would fail at deploy time with a
-- duplicate-key violation -- leaving an INVALID index behind and the API
-- unable to start until a human intervenes. Running this script BEFORE that
-- migration deploys removes the wrongly-glued `train_id` from the corrupted
-- row, so no such collision can exist by the time the migration runs.
--
-- WHAT THIS SCRIPT DOES
-- ----------------------
-- For every `trains` row matching the corruption signature below, this:
--   1. Deletes the wrongly-glued `train_movement_events` rows (keyed by
--      `trains_id`, per `20260906110000_train_movement_trains_id.sql`).
--   2. Deletes the wrongly-glued `train_current_state` row (keyed by
--      `trains_id`, per `20260906130000_nullable_pin_columns.sql`).
--   3. Resets every subscriber's `resolution_status` on
--      `train_subscriptions` back to what it would have been WITHOUT the
--      bug: `'schedule_matched'` if this `trains` row already has a real
--      CIF schedule match (`schedule_matched_at IS NOT NULL`, which is true
--      for the known 2026-09-25 incident), else `'pending'`. A subscription
--      already `'unresolved'` is left alone (the bug's own write path,
--      `flip_legacy_resolution`, only ever moves a subscription TOWARD
--      `'resolved'`, never touches an already-`'unresolved'` one, so there is
--      nothing for this script to undo there).
--   4. Clears `trains.train_id` and `trains.resolved_at` back to `NULL` --
--      the same "schedule-matched but not yet live-resolved" shape
--      `find_or_create_train` leaves a brand-new row in (see
--      `20260906100000_trains.sql`).
--
-- The row is left exactly where the ALREADY-FIXED matching code (both the
-- live trust-consumer path and the backlog-replay path, both now carrying
-- the `is_provable_identity_contradiction` veto) can correctly re-resolve it
-- the next time a live TRUST Activation or a backlog sweep reaches it. This
-- is a plain, one-time DATA fix -- it does NOT belong in
-- `crates/api/migrations/`, changes no schema, and touches no code.
--
-- WHAT THIS SCRIPT DELIBERATELY DOES NOT TOUCH
-- ----------------------------------------------
--   * The `trains` row itself is never deleted -- only its wrongly-glued
--     `train_id`/`resolved_at` are cleared. Its `train_uid`/schedule-derived
--     columns (`origin_crs`, `scheduled_departure`, `destination_crs`,
--     `calling_points`, `matched_line_id`, `schedule_matched_at`) came from a
--     real, unaffected CIF schedule match and are left exactly as they are.
--   * `train_subscriptions` rows are never deleted, and their `trains_id`
--     link is never repointed -- only `resolution_status` is reset, and only
--     when it needs to be.
--   * `tracked_train_tickets`, `train_notification_state`, `group_trains`,
--     `journey_legs`, any `custom_name` -- every bit of USER data attached to
--     the subscription -- is untouched. This script only ever writes to
--     `trains`, `train_movement_events`, `train_current_state`, and
--     `train_subscriptions.resolution_status`.
--   * `notifier_forward_queue` rows are deliberately left alone. Per its own
--     migration's doc comment it is "a forwarding SIGNAL, not a data store"
--     -- notifier consumes it via a monotonic id-watermark cursor
--     (`notifier_cursor` row named `'notifier_forward_queue'`), so any
--     already-fired signal for the corrupted `trains_id` has near-certainly
--     already been consumed (or will be skipped harmlessly -- the movement
--     data it would have summarized no longer exists after step 1 above).
--     Nothing about leaving these rows in place is unsafe.
--   * Outbound notifications/pushes already SENT to the user based on the
--     wrong movements cannot be un-sent by any SQL script. Out of scope here.
--
-- CORRUPTION SIGNATURE
-- ---------------------
-- A `trains` row is considered corrupted here if BOTH:
--   (a) it has a non-NULL `train_id` (i.e. claims to be live-resolved), AND
--   (b) EITHER:
--       - TRUST's own Activation data (`trust_event_backlog`, msg_type
--         '0001', the only row type that ever carries a `train_uid`) for
--         that exact `train_id` names a DIFFERENT `train_uid` than this row
--         claims to be -- i.e. TRUST itself contradicts the identity this
--         row is glued to. This is the general, reusable signature: it
--         would catch any future instance of this same bug class, not just
--         the known 2026-09-25 row, and needs no hardcoded id.
--       - OR it is the specific, already-confirmed 2026-09-25 incident row,
--         identified by its stable natural key (`train_uid = 'Y80926'`,
--         `service_date = '2026-09-25'`) rather than its surrogate `id`
--         (which this script's author has no live production access to
--         re-confirm as still `6095140`). This fallback exists because
--         `trust_event_backlog` retains data for only 1 DAY by default
--         (`20260905160000_trust_event_backlog.sql`), so by the time this
--         runs the Activation that proves (a) may already have aged out of
--         the backlog even though the corruption itself is still sitting in
--         `trains`.
--
-- Both conditions require `train_id IS NOT NULL`, which is exactly what
-- clearing it to NULL in step 4 removes -- so re-running this script after
-- it has already succeeded finds nothing left to do. See "IDEMPOTENCY"
-- below.
--
-- DRY RUN -- RUN THIS FIRST, ALWAYS
-- -----------------------------------
-- Before running the remediation transaction below, a human operator MUST
-- run this SELECT-only preview against production and manually confirm its
-- output looks exactly as expected (one row, for the known Y80926 pin, with
-- a small, plausible `movement_events_to_delete` count and exactly one
-- `affected_subscription_ids` entry belonging to the known affected user):
--
--     WITH affected AS (
--         SELECT
--             t.id                 AS trains_id,
--             t.train_uid,
--             t.service_date,
--             t.train_id           AS wrong_train_id,
--             t.resolved_at,
--             t.schedule_matched_at
--         FROM trains t
--         WHERE t.train_id IS NOT NULL
--           AND (
--                 EXISTS (
--                     SELECT 1 FROM trust_event_backlog b
--                     WHERE b.train_id = t.train_id
--                       AND b.msg_type = '0001'
--                       AND b.train_uid IS NOT NULL
--                       AND UPPER(b.train_uid) <> UPPER(t.train_uid)
--                 )
--                 OR (t.train_uid = 'Y80926' AND t.service_date = DATE '2026-09-25')
--               )
--     )
--     SELECT
--         a.*,
--         (SELECT COUNT(*) FROM train_movement_events tme
--           WHERE tme.trains_id = a.trains_id)      AS movement_events_to_delete,
--         EXISTS (SELECT 1 FROM train_current_state tcs
--                  WHERE tcs.trains_id = a.trains_id) AS current_state_row_to_delete,
--         (SELECT array_agg(ts.id) FROM train_subscriptions ts
--           WHERE ts.trains_id = a.trains_id)         AS affected_subscription_ids,
--         (SELECT array_agg(ts.user_id) FROM train_subscriptions ts
--           WHERE ts.trains_id = a.trains_id)         AS affected_user_ids,
--         (SELECT array_agg(ts.resolution_status) FROM train_subscriptions ts
--           WHERE ts.trains_id = a.trains_id)         AS affected_subscription_current_status
--     FROM affected a;
--
-- If that query returns ZERO rows, either the corruption has already been
-- fixed (nothing to do -- safe to stop here) or the corruption signature
-- above genuinely does not match anything in production (in which case STOP
-- and investigate before running the remediation transaction -- do not
-- proceed blind). If it returns a row you do not recognise, or more than the
-- one expected row, STOP and investigate before proceeding -- do not run the
-- transaction below until every returned row is understood.
--
-- IDEMPOTENCY
-- ------------
-- Safe to run twice. The affected-row query requires `train_id IS NOT NULL`,
-- and the remediation's own last statement sets `train_id = NULL` on every
-- row it touches -- so a second run finds zero affected rows and every
-- statement below becomes a no-op. The `DELETE`s are likewise no-ops once
-- their target rows are already gone.
--
-- HOW TO RUN
-- -----------
-- This is a plain SQL script, not a `sqlx` migration -- do not add it to
-- `crates/api/migrations/`. Run it with `psql` directly against production,
-- as a human who has ALREADY run the dry-run query above and confirmed its
-- output:
--
--     psql "$PRODUCTION_DATABASE_URL" -f scripts/remediate-2026-09-25-cross-matched-trains-row.sql
--
-- Tested against a local scratch reproduction of the exact corruption shape
-- described above -- see
-- `scripts/test-remediate-2026-09-25-cross-matched-trains-row.sh` in this
-- same worktree for the reproduction + assertions. This script itself has
-- NOT been run against the real production database: no real production
-- database access was available (or used) to whoever wrote this script --
-- see the accompanying report for details.
-- =============================================================================

BEGIN;

-- Snapshot exactly which `trains` rows match the corruption signature, ONCE,
-- before any write below -- every subsequent statement in this transaction
-- reads from this snapshot rather than re-deriving it, so all four writes
-- below are guaranteed to act on the exact same row set (no risk of a
-- mid-transaction change to `trains.train_id` shifting which rows later
-- statements would otherwise select).
CREATE TEMP TABLE _remediation_2026_09_25_affected_trains
    ON COMMIT DROP
AS
SELECT
    t.id                 AS trains_id,
    t.train_uid,
    t.service_date,
    t.train_id            AS wrong_train_id,
    t.schedule_matched_at
FROM trains t
WHERE t.train_id IS NOT NULL
  AND (
        -- General, reusable signature: TRUST's own Activation for this
        -- train_id names a different train_uid than this row claims.
        EXISTS (
            SELECT 1
            FROM trust_event_backlog b
            WHERE b.train_id = t.train_id
              AND b.msg_type = '0001'
              AND b.train_uid IS NOT NULL
              AND UPPER(b.train_uid) <> UPPER(t.train_uid)
        )
        -- Known-incident fallback (see header comment above).
        OR (t.train_uid = 'Y80926' AND t.service_date = DATE '2026-09-25')
      );

-- Step 1: delete the wrongly-glued movement history.
DELETE FROM train_movement_events
WHERE trains_id IN (SELECT trains_id FROM _remediation_2026_09_25_affected_trains);

-- Step 2: delete the wrongly-glued "where is it right now" row.
DELETE FROM train_current_state
WHERE trains_id IN (SELECT trains_id FROM _remediation_2026_09_25_affected_trains);

-- Step 3: reset every subscriber's resolution_status back to what it would
-- have been without the bug. 'schedule_matched' if this trains row already
-- has a real CIF schedule match, else the earlier 'pending' waypoint (see
-- 20260905150000_schedule_matched_resolution.sql for that state machine).
-- Never touches a subscription already 'unresolved' -- the bug's own write
-- path never produces that value, so there is nothing to undo there, and
-- touching it would be a change this script has no basis for.
UPDATE train_subscriptions ts
SET resolution_status = CASE
        WHEN a.schedule_matched_at IS NOT NULL THEN 'schedule_matched'
        ELSE 'pending'
    END
FROM _remediation_2026_09_25_affected_trains a
WHERE ts.trains_id = a.trains_id
  AND ts.resolution_status <> 'unresolved';

-- Step 4: clear the wrongly-glued identity itself. Must run LAST -- steps
-- 1-3 above rely on `trains_id` still being present in the snapshot table,
-- which is unaffected by this step, but this is also the write that makes
-- the corruption signature stop matching on any future/repeat run.
UPDATE trains t
SET train_id = NULL,
    resolved_at = NULL
FROM _remediation_2026_09_25_affected_trains a
WHERE t.id = a.trains_id;

-- Sanity check, visible in the script's own output when run interactively:
-- how many `trains` rows this run actually remediated. Expect exactly 1 for
-- the known 2026-09-25 incident on a not-yet-remediated database, and 0 on
-- any re-run.
SELECT count(*) AS trains_rows_remediated_this_run
FROM _remediation_2026_09_25_affected_trains;

COMMIT;
