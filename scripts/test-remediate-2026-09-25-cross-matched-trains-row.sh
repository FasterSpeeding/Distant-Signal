#!/usr/bin/env bash
# Local reproduction + verification harness for
# scripts/remediate-2026-09-25-cross-matched-trains-row.sql
#
# Builds a scratch Postgres database (via `sqlx migrate run` against this
# repo's real crates/api/migrations, so the schema is exactly what
# production runs), seeds it with a row shaped like the real 2026-09-25
# corruption (plus unrelated control data that must NOT be touched), runs
# the remediation script, and asserts:
#
#   1. The corrupted row's train_id/resolved_at are cleared, and its
#      wrongly-glued train_movement_events/train_current_state rows are gone.
#   2. The corrupted row's own train_uid, schedule-derived columns, and the
#      trains row itself all still exist, untouched.
#   3. The user's subscription (and its ticket) still exists, untouched
#      except for resolution_status moving back to 'schedule_matched'.
#   4. A second, unrelated user's correctly-resolved trains row/subscription/
#      movement events/current state are completely untouched.
#   5. Before remediation, inserting a second `trains` row for the REAL
#      W34058 service with the SAME (wrongly-glued) train_id fails with a
#      unique-violation -- reproducing the exact deploy-time risk the
#      20260925222000 migration's own header comment names. After
#      remediation, that same insert succeeds -- proving the risk is gone.
#   6. Running the remediation script a second time is a no-op (idempotency).
#
# Usage: bash scripts/test-remediate-2026-09-25-cross-matched-trains-row.sh
# Requires: psql, sqlx-cli, a reachable scratch Postgres at
# postgres://postgres:postgres@localhost:5432 (never touches anything named
# as, or resembling, a real production database).

set -euo pipefail

PG_ADMIN_URL="postgres://postgres:postgres@localhost:5432/postgres"
TEST_DB="ds_remediation_test_$$"
TEST_URL="postgres://postgres:postgres@localhost:5432/${TEST_DB}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMEDIATION_SQL="${REPO_ROOT}/scripts/remediate-2026-09-25-cross-matched-trains-row.sql"

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

echo "== Applying real crates/api migrations (including the risky 20260925222000 unique index) =="
DATABASE_URL="$TEST_URL" sqlx migrate run --source "${REPO_ROOT}/crates/api/migrations" >/dev/null

echo "== Seeding the corrupted shape + unrelated control data =="
psql "$TEST_URL" -v ON_ERROR_STOP=1 -q <<'SEED'
-- Users.
INSERT INTO users (id, email, name) VALUES
    ('test-user-y80926', 'y80926-owner@example.com', 'Y80926 Owner'),
    ('test-user-control', 'control-owner@example.com', 'Control Owner');

-- The corrupted trains row: train_uid is the user's real London
-- Northwestern service (Y80926), but train_id was wrongly resolved to the
-- unrelated Avanti service's (W34058) TRUST identifier.
INSERT INTO trains
    (id, train_uid, service_date, origin_crs, scheduled_departure,
     destination_crs, schedule_matched_at, train_id, resolved_at)
VALUES
    (6095140, 'Y80926', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:15:00+00', 'BHM',
     TIMESTAMPTZ '2026-09-25 06:00:00+00',
     'W345801FAKE', TIMESTAMPTZ '2026-09-25 07:20:00+00');

-- An unrelated, CORRECTLY resolved control row -- must remain untouched.
INSERT INTO trains
    (id, train_uid, service_date, origin_crs, scheduled_departure,
     destination_crs, schedule_matched_at, train_id, resolved_at)
VALUES
    (7000001, 'Y80999', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', 'MAN',
     TIMESTAMPTZ '2026-09-25 06:00:00+00',
     'Y80999REALID', TIMESTAMPTZ '2026-09-25 09:05:00+00');

-- TRUST's own Activation data: the REAL owner of 'W345801FAKE' is W34058,
-- not Y80926 -- this is the general corruption signature the remediation
-- script's EXISTS clause detects.
INSERT INTO trust_event_backlog
    (crs, train_uid, train_id, service_date, msg_type, event_type,
     planned_timestamp, actual_timestamp, variation_status, dedup_key)
VALUES
    (NULL, 'W34058', 'W345801FAKE', DATE '2026-09-25', '0001', NULL,
     NULL, NULL, NULL, 'seed-activation-w34058'),
    ('EUS', NULL, 'W345801FAKE', DATE '2026-09-25', '0003', 'DEPARTURE',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', TIMESTAMPTZ '2026-09-25 07:20:00+00',
     'LATE', 'seed-movement-w34058-eus');

-- The control row's OWN, correctly-matching Activation (train_uid == trains.train_uid).
INSERT INTO trust_event_backlog
    (crs, train_uid, train_id, service_date, msg_type, event_type,
     planned_timestamp, actual_timestamp, variation_status, dedup_key)
VALUES
    (NULL, 'Y80999', 'Y80999REALID', DATE '2026-09-25', '0001', NULL,
     NULL, NULL, NULL, 'seed-activation-y80999');

-- Wrongly-glued train_movement_events on the corrupted row (really W34058's
-- movements).
INSERT INTO train_movement_events
    (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp,
     actual_timestamp, variation_status, raw_body)
VALUES
    (6095140, 'seed-tme-1', '0003', 'DEPARTURE', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:18:00+00', TIMESTAMPTZ '2026-09-25 07:20:00+00',
     'LATE', '{}'::jsonb),
    (6095140, 'seed-tme-2', '0003', 'ARRIVAL', 'LIV',
     TIMESTAMPTZ '2026-09-25 09:40:00+00', TIMESTAMPTZ '2026-09-25 09:45:00+00',
     'LATE', '{}'::jsonb);

-- Wrongly-glued train_current_state on the corrupted row.
INSERT INTO train_current_state
    (trains_id, status, last_reported_location, last_event_type, delay_minutes)
VALUES
    (6095140, 'en_route', 'LIV', 'ARRIVAL', 5);

-- Control row's OWN, correct movement events + current state.
INSERT INTO train_movement_events
    (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp,
     actual_timestamp, variation_status, raw_body)
VALUES
    (7000001, 'seed-tme-control-1', '0003', 'DEPARTURE', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', TIMESTAMPTZ '2026-09-25 09:00:00+00',
     'ON TIME', '{}'::jsonb);
INSERT INTO train_current_state
    (trains_id, status, last_reported_location, last_event_type, delay_minutes)
VALUES
    (7000001, 'en_route', 'EUS', 'DEPARTURE', 0);

-- The user's subscription: wrongly flipped to 'resolved' by the bug.
INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9100001, 'test-user-y80926', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 07:15:00+00', 'resolved', 6095140,
     'My commute to Birmingham');

-- A real ticket attached to that subscription -- must remain untouched.
INSERT INTO tracked_train_tickets
    (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs)
VALUES
    (9100001, 'test-user-y80926', 'LM', 'anytime_day_single', 'EUS', 'BHM');

-- Control subscription -- must remain fully untouched.
INSERT INTO train_subscriptions
    (id, user_id, service_date, pin_origin_crs, pin_scheduled_departure,
     resolution_status, trains_id, custom_name)
VALUES
    (9100002, 'test-user-control', DATE '2026-09-25', 'EUS',
     TIMESTAMPTZ '2026-09-25 09:00:00+00', 'resolved', 7000001,
     'My commute to Manchester');
SEED

echo "== Pre-check: reproducing the exact deploy-time risk =="
# Attempting to insert a second trains row for the REAL W34058 service,
# independently resolved by someone else, with the SAME wrongly-glued
# train_id, must fail with a unique-violation before remediation -- this is
# exactly what would make 20260925222000's CREATE UNIQUE INDEX CONCURRENTLY
# fail at deploy time if this state existed in production.
if psql "$TEST_URL" -v ON_ERROR_STOP=1 -q -c \
    "INSERT INTO trains (id, train_uid, service_date, train_id, resolved_at)
     VALUES (6095141, 'W34058', DATE '2026-09-25', 'W345801FAKE', NOW());" \
    >/tmp/pretest_insert_output_$$.txt 2>&1; then
    echo "  [FAIL] pre-remediation collision insert unexpectedly SUCCEEDED"
    fail=$((fail + 1))
else
    if grep -qi "duplicate key value violates unique constraint" /tmp/pretest_insert_output_$$.txt; then
        echo "  [PASS] pre-remediation insert of the real W34058 row correctly hit the unique-violation this script exists to prevent"
        pass=$((pass + 1))
    else
        echo "  [FAIL] pre-remediation insert failed for an unexpected reason:"
        cat /tmp/pretest_insert_output_$$.txt
        fail=$((fail + 1))
    fi
fi
rm -f /tmp/pretest_insert_output_$$.txt

echo "== Running the dry-run preview query =="
psql "$TEST_URL" -v ON_ERROR_STOP=1 <<'DRYRUN'
WITH affected AS (
    SELECT
        t.id                 AS trains_id,
        t.train_uid,
        t.service_date,
        t.train_id           AS wrong_train_id,
        t.resolved_at,
        t.schedule_matched_at
    FROM trains t
    WHERE t.train_id IS NOT NULL
      AND (
            EXISTS (
                SELECT 1 FROM trust_event_backlog b
                WHERE b.train_id = t.train_id
                  AND b.msg_type = '0001'
                  AND b.train_uid IS NOT NULL
                  AND UPPER(b.train_uid) <> UPPER(t.train_uid)
            )
            OR (t.train_uid = 'Y80926' AND t.service_date = DATE '2026-09-25')
          )
)
SELECT
    a.*,
    (SELECT COUNT(*) FROM train_movement_events tme
      WHERE tme.trains_id = a.trains_id)      AS movement_events_to_delete,
    EXISTS (SELECT 1 FROM train_current_state tcs
             WHERE tcs.trains_id = a.trains_id) AS current_state_row_to_delete,
    (SELECT array_agg(ts.id) FROM train_subscriptions ts
      WHERE ts.trains_id = a.trains_id)         AS affected_subscription_ids
FROM affected a;
DRYRUN

dry_run_row_count=$(psql "$TEST_URL" -t -A -v ON_ERROR_STOP=1 -c "
    SELECT count(*) FROM trains t
    WHERE t.train_id IS NOT NULL
      AND (
            EXISTS (
                SELECT 1 FROM trust_event_backlog b
                WHERE b.train_id = t.train_id AND b.msg_type = '0001'
                  AND b.train_uid IS NOT NULL AND UPPER(b.train_uid) <> UPPER(t.train_uid)
            )
            OR (t.train_uid = 'Y80926' AND t.service_date = DATE '2026-09-25')
          );
")
check "dry-run finds exactly 1 affected trains row (control row not flagged)" "1" "$dry_run_row_count"

echo "== Running the remediation script (1st time) =="
psql "$TEST_URL" -v ON_ERROR_STOP=1 -f "$REMEDIATION_SQL"

echo "== Assertions after 1st run =="

v=$(psql "$TEST_URL" -t -A -c "SELECT train_id IS NULL FROM trains WHERE id = 6095140;")
check "corrupted row train_id cleared to NULL" "t" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT resolved_at IS NULL FROM trains WHERE id = 6095140;")
check "corrupted row resolved_at cleared to NULL" "t" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT train_uid FROM trains WHERE id = 6095140;")
check "corrupted row train_uid still Y80926 (untouched)" "Y80926" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT destination_crs FROM trains WHERE id = 6095140;")
check "corrupted row schedule-derived destination_crs untouched" "BHM" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_movement_events WHERE trains_id = 6095140;")
check "corrupted row's wrongly-glued movement events deleted" "0" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_current_state WHERE trains_id = 6095140;")
check "corrupted row's wrongly-glued current_state deleted" "0" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status FROM train_subscriptions WHERE id = 9100001;")
check "user's subscription resolution_status reset to schedule_matched" "schedule_matched" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT custom_name FROM train_subscriptions WHERE id = 9100001;")
check "user's subscription custom_name untouched" "My commute to Birmingham" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM tracked_train_tickets WHERE tracked_train_id = 9100001;")
check "user's ticket still present, untouched" "1" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_subscriptions WHERE id = 9100001;")
check "user's subscription row still exists (not deleted)" "1" "$v"

# Control row / control subscription must be completely untouched.
v=$(psql "$TEST_URL" -t -A -c "SELECT train_id FROM trains WHERE id = 7000001;")
check "control trains row train_id untouched" "Y80999REALID" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_movement_events WHERE trains_id = 7000001;")
check "control row's own movement events untouched" "1" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT count(*) FROM train_current_state WHERE trains_id = 7000001;")
check "control row's own current_state untouched" "1" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status FROM train_subscriptions WHERE id = 9100002;")
check "control subscription resolution_status untouched" "resolved" "$v"

echo "== Post-check: the deploy-time risk is now gone =="
if psql "$TEST_URL" -v ON_ERROR_STOP=1 -q -c \
    "INSERT INTO trains (id, train_uid, service_date, train_id, resolved_at)
     VALUES (6095141, 'W34058', DATE '2026-09-25', 'W345801FAKE', NOW());" \
    >/tmp/posttest_insert_output_$$.txt 2>&1; then
    echo "  [PASS] the real W34058 row can now be independently inserted with no collision"
    pass=$((pass + 1))
else
    echo "  [FAIL] insert of the real W34058 row still fails after remediation:"
    cat /tmp/posttest_insert_output_$$.txt
    fail=$((fail + 1))
fi
rm -f /tmp/posttest_insert_output_$$.txt

echo "== Running the remediation script a 2nd time (idempotency check) =="
psql "$TEST_URL" -v ON_ERROR_STOP=1 -f "$REMEDIATION_SQL"

v=$(psql "$TEST_URL" -t -A -c "SELECT train_id FROM trains WHERE id = 6095140;")
check "2nd run: corrupted row train_id still NULL" "" "$v"

v=$(psql "$TEST_URL" -t -A -c "SELECT resolution_status FROM train_subscriptions WHERE id = 9100001;")
check "2nd run: subscription resolution_status unchanged (still schedule_matched)" "schedule_matched" "$v"

# The newly-inserted real W34058 row from the post-check above must be
# untouched by a second remediation run (it correctly matches its own
# Activation, so it must never be flagged by the corruption signature).
v=$(psql "$TEST_URL" -t -A -c "SELECT train_id FROM trains WHERE id = 6095141;")
check "2nd run: the real, correctly-resolved W34058 row is left alone" "W345801FAKE" "$v"

echo
echo "================================================================"
echo "RESULTS: ${pass} passed, ${fail} failed"
echo "================================================================"

if [[ "$fail" -ne 0 ]]; then
    exit 1
fi
