#!/bin/sh
# pgbackrest-daily-check: the daily repository check, baked into
# docker/postgres-pgbackrest.Dockerfile as /usr/local/bin/pgbackrest-daily-check.
# The chart's `<release>-pgbackrest-check` CronJob runs it inside the
# Postgres container with `kubectl exec`, so it has that container's
# PGBACKREST_* env (repository, stanza, cipher) and POSTGRES_USER /
# POSTGRES_DB for psql over the local socket.
#
# Usage: pgbackrest-daily-check [--no-verify]
#
# Exits non-zero, failing the Job (DistantSignalPgBackRestCheckFailed), when
# any of these fails:
#   1. `pgbackrest check`: forces a WAL switch and waits for that segment to
#      reach the repository, so it fails while archiving is broken (Thoth
#      down, bad credentials, no stanza).
#   2. `pgbackrest verify` (skipped with --no-verify): reads every backup and
#      WAL file back from the repository, decrypts it and checks it. The
#      chart's daily check passes --no-verify and runs verify in its own
#      weekly CronJob instead.
#   3. The WAL gap check. When Thoth is down long enough for the spool to
#      reach archive-push-queue-max, pgBackRest drops WAL instead of letting
#      pg_wal fill the node's disk, and Postgres still counts the dropped
#      segments as archived. So list the archived segments on the newest
#      backup's timeline, and fail if any is missing between the oldest
#      backup's start segment on that timeline and the newest archived one.
#      Point-in-time recovery can't cross a gap: take a new full backup
#      (docs/postgres-pitr.md, "WAL gap").
set -eu

verify=1
case "${1-}" in
    --no-verify) verify=0 ;;
    "") ;;
    *)
        echo "usage: pgbackrest-daily-check [--no-verify]" >&2
        exit 2
        ;;
esac

stanza="${PGBACKREST_STANZA:?PGBACKREST_STANZA is not set}"
db_user="${POSTGRES_USER:?POSTGRES_USER is not set}"
db_name="${POSTGRES_DB:?POSTGRES_DB is not set}"

echo "== pgbackrest check"
pgbackrest --log-level-console=info check

if [ "${verify}" -eq 1 ]; then
    echo "== pgbackrest verify"
    pgbackrest --log-level-console=info verify
fi

echo "== WAL gap check"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
pgbackrest --output=json info | tr -d '\n' >"${tmp}/info.json"
pgbackrest repo-ls --recurse "archive/${stanza}" >"${tmp}/archive.txt"

# psql does the JSON parsing and the arithmetic; the image has no jq. Both
# inputs go in through COPY, not psql variables, so their size isn't bounded
# by the argument length limit. The info JSON is one CSV field: the quote and
# delimiter are control characters JSON never contains unescaped.
result="$(
    {
        echo 'CREATE TEMP TABLE info (doc text);'
        printf '%s\n' "COPY info FROM STDIN WITH (FORMAT csv, QUOTE E'\\x01', DELIMITER E'\\x02');"
        cat "${tmp}/info.json"
        echo
        echo '\.'
        echo 'CREATE TEMP TABLE listing (path text);'
        echo 'COPY listing FROM STDIN;'
        # Only well-formed segment names: no .history, .partial or
        # archive.info lines, and nothing COPY's text format would escape.
        grep -E '(^|/)[0-9A-F]{24}-' "${tmp}/archive.txt" || true
        echo '\.'
        cat <<'SQL'
WITH per_log AS (
    -- Segments per 4 GiB "log" file: 256 at the default 16 MB segment size.
    SELECT 4294967296 / setting::bigint AS n
    FROM pg_settings
    WHERE name = 'wal_segment_size'
),
backups AS (
    SELECT b -> 'archive' ->> 'start' AS start_wal
    FROM info,
        jsonb_array_elements(info.doc::jsonb) AS s,
        jsonb_array_elements(s -> 'backup') AS b
    WHERE s ->> 'name' = :'stanza'
),
newest_tli AS (
    SELECT left(max(start_wal), 8) AS tli FROM backups
),
start_wal AS (
    SELECT min(start_wal) AS name
    FROM backups, newest_tli
    WHERE left(start_wal, 8) = newest_tli.tli
),
segs AS (
    SELECT DISTINCT
        ('x' || substr(m[1], 9, 8))::bit(32)::bigint * per_log.n
        + ('x' || substr(m[1], 17, 8))::bit(32)::bigint AS n
    FROM listing,
        per_log,
        newest_tli,
        regexp_match(listing.path, '(?:^|/)([0-9A-F]{24})-') AS m
    WHERE left(m[1], 8) = newest_tli.tli
),
bounds AS (
    SELECT
        ('x' || substr(start_wal.name, 9, 8))::bit(32)::bigint * per_log.n
        + ('x' || substr(start_wal.name, 17, 8))::bit(32)::bigint AS lo,
        (SELECT max(segs.n) FROM segs) AS hi
    FROM start_wal, per_log
    WHERE start_wal.name IS NOT NULL
),
missing AS (
    SELECT g AS n FROM bounds, generate_series(bounds.lo, bounds.hi) AS g
    EXCEPT
    SELECT segs.n FROM segs
),
named AS (
    SELECT upper(
        newest_tli.tli
        || lpad(to_hex(missing.n / per_log.n), 8, '0')
        || lpad(to_hex(missing.n % per_log.n), 8, '0')
    ) AS name
    FROM missing, per_log, newest_tli
)
SELECT
    coalesce(
        (SELECT coalesce((hi - lo + 1)::text, 'empty') FROM bounds), 'none'
    )
    || ' ' || (SELECT count(*) FROM named)
    || ' ' || coalesce((SELECT min(name) FROM named), '-')
    || ' ' || coalesce((SELECT max(name) FROM named), '-');
SQL
    } | psql -X -q -At -v ON_ERROR_STOP=1 -v stanza="${stanza}" \
        -p "${PGBACKREST_PG1_PORT:-5432}" -U "${db_user}" -d "${db_name}"
)"

read -r span missing first last <<EOF
${result}
EOF

if [ "${span}" = "none" ]; then
    echo "no backups in the repository yet; nothing to check for gaps"
    exit 0
fi
if [ "${span}" = "empty" ]; then
    echo "the repository has backups but no archived WAL on their timeline" >&2
    exit 1
fi
if [ "${missing}" -ne 0 ]; then
    echo "WAL gap: ${missing} of ${span} segments since the oldest backup are missing from the repository (first ${first}, last ${last})." >&2
    echo "Point-in-time recovery can't cross the gap. Take a full backup now: docs/postgres-pitr.md, \"WAL gap\"." >&2
    exit 1
fi
echo "no WAL gap: ${span} consecutive segments since the oldest backup"
