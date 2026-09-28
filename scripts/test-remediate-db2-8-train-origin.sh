#!/usr/bin/env bash
# Local verification harness for scripts/remediate-db2-8-train-origin.sql.
#
# Builds a scratch database from crates/api/migrations, seeds one trains row
# with a mid-route origin (the DB2-8 bug), one correct row and one row whose
# first TIPLOC has no CRS, then checks that:
#   1. the preview runs inside a read-only transaction and lists only the bad row;
#   2. `-v apply=1` fixes the bad row and leaves the other two untouched;
#   3. a second run finds nothing (idempotent).
#
# Usage: bash scripts/test-remediate-db2-8-train-origin.sh
# Requires psql, sqlx-cli and a scratch Postgres at postgres:postgres@localhost:5432.

set -euo pipefail

PG_ADMIN_URL="postgres://postgres:postgres@localhost:5432/postgres"
TEST_DB="ds_db2_8_test_$$"
TEST_URL="postgres://postgres:postgres@localhost:5432/${TEST_DB}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="${REPO_ROOT}/scripts/remediate-db2-8-train-origin.sql"

cleanup() { psql -X -q "${PG_ADMIN_URL}" -c "DROP DATABASE IF EXISTS ${TEST_DB}" >/dev/null; }
trap cleanup EXIT

psql -X -q "${PG_ADMIN_URL}" -c "CREATE DATABASE ${TEST_DB}" >/dev/null
sqlx migrate run --source "${REPO_ROOT}/crates/api/migrations" --database-url "${TEST_URL}" >/dev/null

q() { psql -X -q -t -A "${TEST_URL}" -c "$1"; }

q "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) VALUES
     ('T-WAT', 'ZWA', 'TWATRLMN', 'ORIGIN', 1), ('T-RAY', 'ZRA', 'TRAYNSPK', 'MIDDLE', 1)"
cp='[{"tiploc":"TWATRLMN","kind":"Origin","dayOffset":0,"bookedArrival":null,"bookedDeparture":"07:47:00"},
     {"tiploc":"TRAYNSPK","kind":"Intermediate","dayOffset":0,"bookedArrival":"08:06:00","bookedDeparture":"08:07:00"}]'
nocrs='[{"tiploc":"TNOCRS","kind":"Origin","dayOffset":0,"bookedArrival":null,"bookedDeparture":"09:00:00"}]'
q "INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure, calling_points, schedule_matched_at) VALUES
     ('TBAD01', '2026-09-24', 'ZRA', '2026-09-24 08:07:27+01', '${cp}', NOW()),
     ('TGOOD1', '2026-09-24', 'ZWA', '2026-09-24 07:47:00+01', '${cp}', NOW()),
     ('TNOCRS', '2026-09-24', 'ZZZ', '2026-09-24 09:00:00+01', '${nocrs}', NOW())"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

preview=$(PGOPTIONS="-c default_transaction_read_only=on" psql -X -q -t -A "${TEST_URL}" -f "${SCRIPT}")
preview_lines=$(wc -l <<<"${preview}")
[[ "${preview_lines}" == 1 && "${preview}" == *"TBAD01|2026-09-24|ZRA|ZWA|"* ]] \
    || fail "preview should list only TBAD01, got: ${preview}"

psql -X -q -t -A "${TEST_URL}" -v apply=1 -f "${SCRIPT}" >/dev/null
bad=$(q "SELECT origin_crs || ' ' || scheduled_departure FROM trains WHERE train_uid = 'TBAD01'")
[[ "${bad}" == "ZWA 2026-09-24 06:47:00+00" ]] || fail "TBAD01 not fixed"
nocrs_origin=$(q "SELECT origin_crs FROM trains WHERE train_uid = 'TNOCRS'")
[[ "${nocrs_origin}" == "ZZZ" ]] || fail "a row whose first TIPLOC has no CRS must keep its origin_crs"
good=$(q "SELECT origin_crs FROM trains WHERE train_uid = 'TGOOD1'")
[[ "${good}" == "ZWA" ]] || fail "TGOOD1 changed"

again=$(PGOPTIONS="-c default_transaction_read_only=on" psql -X -q -t -A "${TEST_URL}" -f "${SCRIPT}")
[[ -z "${again}" ]] || fail "second preview should be empty, got: ${again}"

echo "PASS: remediate-db2-8-train-origin.sql"
