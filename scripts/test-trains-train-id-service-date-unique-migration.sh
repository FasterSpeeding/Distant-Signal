#!/usr/bin/env bash
# Local reproduction + verification harness for
# crates/api/migrations/20260925221500_close_out_trains_train_id_service_date_collisions.sql --
# the delete-both-sides-of-any-collision + subscription-detach migration
# that now runs immediately before
# 20260925222000_trains_train_id_service_date_unique.sql's
# `CREATE UNIQUE INDEX CONCURRENTLY` statement (split into two files because
# Postgres refuses to run CREATE INDEX CONCURRENTLY at all unless it is the
# only statement in its migration file -- see that file's own header
# comment).
#
# Builds a scratch Postgres database, applies every real migration up to but
# NOT including 20260925221500 (via `sqlx migrate run --target-version`),
# manually seeds:
#   * the known 2026-09-25 incident's exact shape (a 2-row collision), with
#     subscriptions in different resolution_status states to exercise the
#     recompute logic ('schedule_matched' -> 'pending', 'resolved' ->
#     'pending', 'unresolved' stays 'unresolved'),
#   * a SYNTHETIC 3-way collision (three trains rows sharing one
#     (train_id, service_date) pair) to prove the fix isn't pair-only,
#   * an unrelated control user/trains row/subscription/ticket/group-share
#     that must come out completely untouched,
# then applies the remaining pending migrations (including the modified
# 20260925222000) and asserts the outcome.
#
# Usage: bash scripts/test-trains-train-id-service-date-unique-migration.sh
# Requires: psql, sqlx-cli, a reachable scratch Postgres at
# postgres://postgres:postgres@localhost:5432 (never touches anything named
# as, or resembling, a real production database).

set -euo pipefail

PG_ADMIN_URL="postgres://postgres:postgres@localhost:5432/postgres"
TEST_DB="ds_trains_unique_migration_test_$$"
TEST_URL="postgres://postgres:postgres@localhost:5432/${TEST_DB}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIGRATIONS_DIR="${REPO_ROOT}/crates/api/migrations"
TARGET_MIGRATION="${MIGRATIONS_DIR}/20260925221500_close_out_trains_train_id_service_date_collisions.sql"
PRE_TARGET_VERSION="20260925221000"

pass=0
fail=0

check() {
    local desc="$1" expected="$2" actual="$3"
    if [[ "$expected" == "$actual" ]]; then
        echo "  [PASS] ${desc}"
        pass=$((pass + 1))
    else
        echo "  [FAIL] ${desc} (expected [${expected}], got [${actual}])"
        fail=$((fail + 1))
    fi
}

cleanup() {
    psql "$PG_ADMIN_URL" -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS ${TEST_DB};" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "== Building scratch database ${TEST_DB} =="
psql "$PG_ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE ${TEST_DB};" >/dev/null

echo "== Applying every real migration up to (not including) 20260925221500 =="
DATABASE_URL="$TEST_URL" sqlx migrate run --source "$MIGRATIONS_DIR" \
    --target-version "$PRE_TARGET_VERSION" >/dev/null

echo "== Seeding collisions + control data =="
psql "$TEST_URL" -v ON_ERROR_STOP=1 -q <<'SEED'
-- Users.
INSERT INTO users (id, email, name) VALUES
    ('test-user-y80926',   'y80926-owner@example.com',   'Y80926 Owner'),
    ('test-user-w34058',   'w34058-owner@example.com',   'W34058 Owner'),
    ('test-user-unresolved', 'unresolved-owner@example.com', 'Unresolved Owner'),
    ('test-user-3way-a',   '3way-a@example.com',         '3-Way Owner A'),
    ('test-user-3way-b',   '3way-b@example.com',         '3-Way Owner B'),
    ('test-user-3way-c',   '3way-c@example.com',         '3-Way Owner C'),
    ('test-user-control',  'control-owner@example.com',  'Control Owner');

-- ---------------------------------------------------------------------
-- Known-incident shape: TWO trains rows sharing (train_id, service_date).
-- ---------------------------------------------------------------------
INSERT INTO trains
    (id, train_uid, service_date, origin_crs, scheduled_departure,
     destination_crs, schedule_matched_at, train_id, resolved_at)
VALUES
    (6095140, 'Y80926', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:15:00+00', 'BHM',
     TIMESTAMPTZ '2026-09-25 06:00:00+00',
     'W345801FAKE', TIMESTAMPTZ '2026-09-25 07:20:00+00'),
    (6095141, 'W34058', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', 'LIV',
     TIMESTAMPTZ '2026-09-25 06:05:00+00',
     'W345801FAKE', TIMESTAMPTZ '2026-09-25 07:22:00+00');

-- Subscriptions pointing at the FIRST colliding row (6095140), one per
-- resolution_status this recompute logic must handle.
INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9100001, 'test-user-y80926', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:15:00+00', 'resolved', 6095140,
     'My commute to Birmingham'),
    (9100003, 'test-user-unresolved', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:15:00+00', 'unresolved', 6095140,
     'Ghost pin');

-- Subscription pointing at the SECOND colliding row (6095141), status
-- schedule_matched, to prove that path also resets to 'pending'.
INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9100002, 'test-user-w34058', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', 'schedule_matched', 6095141,
     'My commute to Liverpool');

-- A real ticket + custom_name on the first subscription -- must survive.
INSERT INTO tracked_train_tickets
    (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs, custom_name)
VALUES
    (9100001, 'test-user-y80926', 'LM', 'anytime_day_single', 'EUS', 'BHM', 'Ticket keepsake');

-- A group share on the first subscription (group_trains keys off
-- train_subscriptions.id, never trains.id) -- must survive.
INSERT INTO groups (id, name, created_by) VALUES ('grp-1', 'Commuters', 'test-user-y80926');
INSERT INTO group_members (group_id, user_id, role) VALUES ('grp-1', 'test-user-y80926', 'owner');
INSERT INTO group_trains (group_id, train_subscription_id, added_by)
    VALUES ('grp-1', 9100001, 'test-user-y80926');

-- Wrongly-glued movement/current-state/notifier-queue rows on both
-- colliding trains rows -- fine to lose.
INSERT INTO train_movement_events
    (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp,
     actual_timestamp, variation_status, raw_body)
VALUES
    (6095140, 'seed-tme-1', '0003', 'DEPARTURE', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', TIMESTAMPTZ '2026-09-25 07:20:00+00',
     'LATE', '{}'::jsonb),
    (6095141, 'seed-tme-2', '0003', 'DEPARTURE', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', TIMESTAMPTZ '2026-09-25 07:20:00+00',
     'LATE', '{}'::jsonb);
INSERT INTO train_current_state (trains_id, status, last_reported_location, last_event_type, delay_minutes)
VALUES
    (6095140, 'en_route', 'LIV', 'ARRIVAL', 5),
    (6095141, 'en_route', 'LIV', 'ARRIVAL', 5);
INSERT INTO notifier_forward_queue (trains_id, event_summary)
VALUES
    (6095140, 'seed notifier signal on 6095140'),
    (6095141, 'seed notifier signal on 6095141');

-- ---------------------------------------------------------------------
-- Synthetic 3-way collision: THREE trains rows sharing one
-- (train_id, service_date) pair, proving the fix isn't pair-only.
-- ---------------------------------------------------------------------
INSERT INTO trains
    (id, train_uid, service_date, origin_crs, scheduled_departure,
     destination_crs, schedule_matched_at, train_id, resolved_at)
VALUES
    (6200001, 'A11111', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:00:00+00', 'EDB', NULL, 'SHARED3WAY', TIMESTAMPTZ '2026-09-26 08:05:00+00'),
    (6200002, 'A22222', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:10:00+00', 'YRK', TIMESTAMPTZ '2026-09-26 07:00:00+00', 'SHARED3WAY', TIMESTAMPTZ '2026-09-26 08:15:00+00'),
    (6200003, 'A33333', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:20:00+00', 'NCL', NULL, 'SHARED3WAY', TIMESTAMPTZ '2026-09-26 08:25:00+00');

INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9200001, 'test-user-3way-a', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:00:00+00', 'resolved', 6200001, '3-way A'),
    (9200002, 'test-user-3way-b', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:10:00+00', 'resolved', 6200002, '3-way B'),
    (9200003, 'test-user-3way-c', DATE '2026-09-26', 'KGX', TIMESTAMPTZ '2026-09-26 08:20:00+00', 'unresolved', 6200003, '3-way C');

-- ---------------------------------------------------------------------
-- Unrelated control data -- must be completely untouched.
-- ---------------------------------------------------------------------
INSERT INTO trains
    (id, train_uid, service_date, origin_crs, scheduled_departure,
     destination_crs, schedule_matched_at, train_id, resolved_at)
VALUES
    (7000001, 'Y80999', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', 'MAN',
     TIMESTAMPTZ '2026-09-25 06:00:00+00',
     'Y80999REALID', TIMESTAMPTZ '2026-09-25 09:05:00+00');

INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9100099, 'test-user-control', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', 'resolved', 7000001,
     'My commute to Manchester');

INSERT INTO tracked_train_tickets
    (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs)
VALUES
    (9100099, 'test-user-control', 'AV', 'anytime_day_single', 'EUS', 'MAN');

INSERT INTO train_movement_events
    (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp,
     actual_timestamp, variation_status, raw_body)
VALUES
    (7000001, 'seed-tme-control-1', '0003', 'DEPARTURE', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', TIMESTAMPTZ '2026-09-25 09:00:00+00',
     'ON TIME', '{}'::jsonb);
INSERT INTO train_current_state (trains_id, status, last_reported_location, last_event_type, delay_minutes)
VALUES (7000001, 'en_route', 'EUS', 'DEPARTURE', 0);
SEED

echo "== Pre-check: collisions exist before the migration runs =="
v=$(psql "$TEST_URL" -t -A -c "
    SELECT count(*) FROM trains t
    WHERE t.train_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM trains t2 WHERE t2.train_id = t.train_id AND t2.service_date = t.service_date AND t2.id <> t.id);
")
check "5 trains rows currently in a collision (2 known-incident + 3 synthetic)" "5" "$v"

echo "== Applying the remaining pending migrations (including the new 20260925221500 cleanup + the 20260925222000 index build) =="
DATABASE_URL="$TEST_URL" sqlx migrate run --source "$MIGRATIONS_DIR" >/dev/null

echo "== Assertions =="

# Known-incident collision: both rows gone.
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM trains WHERE id IN (6095140, 6095141);")
check "both known-incident colliding trains rows deleted" "0" "$v"

# 3-way collision: all three rows gone.
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM trains WHERE id IN (6200001, 6200002, 6200003);")
check "all 3 synthetic-collision trains rows deleted" "0" "$v"

# Subscription 9100001 was 'resolved' -> 'pending', trains_id NULL.
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9100001;")
check "resolved subscription on deleted row -> pending, trains_id NULL" "pending|t" "$v"

# Subscription 9100002 was 'schedule_matched' -> 'pending', trains_id NULL.
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9100002;")
check "schedule_matched subscription on deleted row -> pending, trains_id NULL" "pending|t" "$v"

# Subscription 9100003 was 'unresolved' -> stays 'unresolved', trains_id NULL.
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9100003;")
check "unresolved subscription on deleted row stays unresolved, trains_id NULL" "unresolved|t" "$v"

# 3-way subscriptions: 'resolved' -> 'pending' (x2), 'unresolved' stays.
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9200001;")
check "3-way A resolved -> pending, trains_id NULL" "pending|t" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9200002;")
check "3-way B resolved -> pending, trains_id NULL" "pending|t" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id IS NULL FROM train_subscriptions WHERE id = 9200003;")
check "3-way C unresolved stays unresolved, trains_id NULL" "unresolved|t" "$v"

# Every subscription row itself still exists (never deleted).
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_subscriptions WHERE id IN (9100001, 9100002, 9100003, 9200001, 9200002, 9200003);")
check "all 6 affected subscription rows still exist" "6" "$v"

# Ticket + custom_name + group share on subscription 9100001 survive.
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM tracked_train_tickets WHERE tracked_train_id = 9100001;")
check "user's ticket survives" "1" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT custom_name FROM train_subscriptions WHERE id = 9100001;")
check "user's custom_name survives" "My commute to Birmingham" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM group_trains WHERE train_subscription_id = 9100001;")
check "user's group share survives" "1" "$v"

# Cascaded child rows on deleted trains rows are gone (acceptable loss).
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_movement_events WHERE trains_id IN (6095140, 6095141, 6200001, 6200002, 6200003);")
check "movement events on deleted trains rows are gone" "0" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_current_state WHERE trains_id IN (6095140, 6095141, 6200001, 6200002, 6200003);")
check "current_state rows on deleted trains rows are gone" "0" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM notifier_forward_queue WHERE trains_id IN (6095140, 6095141);")
check "notifier_forward_queue rows on deleted trains rows are gone" "0" "$v"

# Control data completely untouched.
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM trains WHERE id = 7000001;")
check "control trains row still exists" "1" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = 9100099;")
check "control subscription completely untouched" "resolved|7000001" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM tracked_train_tickets WHERE tracked_train_id = 9100099;")
check "control ticket untouched" "1" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_movement_events WHERE trains_id = 7000001;")
check "control movement events untouched" "1" "$v"
v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_current_state WHERE trains_id = 7000001;")
check "control current_state untouched" "1" "$v"

# The unique index exists and is valid.
v=$(psql "$TEST_URL" -t -A -c "SELECT indisvalid FROM pg_index WHERE indexrelid = 'trains_train_id_service_date'::regclass;")
check "trains_train_id_service_date index is valid" "t" "$v"

# No more collisions remain.
v=$(psql "$TEST_URL" -t -A -c "
    SELECT count(*) FROM trains t
    WHERE t.train_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM trains t2 WHERE t2.train_id = t.train_id AND t2.service_date = t.service_date AND t2.id <> t.id);
")
check "no collisions remain" "0" "$v"

echo "== Idempotency: re-applying the migration's own SQL statements directly =="
# sqlx itself never re-runs an already-applied migration -- but confirm the
# added UPDATE/DELETE statements (everything before CREATE UNIQUE INDEX
# CONCURRENTLY, which cannot be re-run against an existing index anyway)
# are themselves no-ops if executed again against a database already past
# this state -- i.e. re-running this migration's cleanup logic after it has
# already converged errors on nothing and changes nothing further.
psql "$TEST_URL" -v ON_ERROR_STOP=1 -q <<'REPLAY'
UPDATE train_subscriptions ts
SET trains_id = NULL,
    resolution_status = CASE
        WHEN ts.resolution_status = 'unresolved' THEN ts.resolution_status
        ELSE 'pending'
    END
WHERE ts.trains_id IN (
    SELECT t.id
    FROM trains t
    WHERE t.train_id IS NOT NULL
      AND EXISTS (
          SELECT 1
          FROM trains t2
          WHERE t2.train_id = t.train_id
            AND t2.service_date = t.service_date
            AND t2.id <> t.id
      )
);

DELETE FROM trains t
WHERE t.train_id IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM trains t2
      WHERE t2.train_id = t.train_id
        AND t2.service_date = t.service_date
        AND t2.id <> t.id
  );
REPLAY
echo "  [PASS] replaying the UPDATE/DELETE statements against a converged database is a clean no-op"
pass=$((pass + 1))

v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = 9100001;")
check "replay didn't touch the already-recomputed subscription" "pending|" "$v"

echo "== Fresh-empty-database sanity: full migrate-from-scratch is a clean no-op for the new statements =="
FRESH_DB="ds_trains_unique_migration_fresh_$$"
FRESH_URL="postgres://postgres:postgres@localhost:5432/${FRESH_DB}"
psql "$PG_ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE ${FRESH_DB};" >/dev/null
DATABASE_URL="$FRESH_URL" sqlx migrate run --source "$MIGRATIONS_DIR" >/dev/null
v=$(psql "$FRESH_URL" -t -A -c "SELECT indisvalid FROM pg_index WHERE indexrelid = 'trains_train_id_service_date'::regclass;")
check "fresh database: index builds and is valid with nothing to collide" "t" "$v"
psql "$PG_ADMIN_URL" -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS ${FRESH_DB};" >/dev/null 2>&1 || true

echo
echo "================================================================"
echo "RESULTS: ${pass} passed, ${fail} failed"
echo "================================================================"

if [[ "$fail" -ne 0 ]]; then
    exit 1
fi
