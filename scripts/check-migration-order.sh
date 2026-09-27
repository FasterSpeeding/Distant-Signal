#!/usr/bin/env bash
# Fails if, since BASE (DB review 2026-09-27, A4/A5, DB2-35, INF-16):
#   - a migration was added whose version is not greater than every
#     migration BASE already had; or
#   - a migration BASE already had was modified, deleted or renamed.
#
# sqlx applies any local migration missing from _sqlx_migrations whatever
# its version, so a branch whose migration timestamp predates one already
# merged (and deployed) runs it late, out of order, with no error. Give it a
# later timestamp instead.
#
# A merged migration is immutable: sqlx compares each applied file's SHA-384
# with _sqlx_migrations.checksum at startup, so an edited file crash-loops
# the api on every database that already ran it, and a renamed or deleted
# one leaves an applied row with no file. This diff-against-BASE check needs
# no upkeep; crates/api/tests/migration_checksums.rs (the checksum lock)
# covers the same ground for migrations recorded in its .lock file, including
# changes made outside a PR.
#
# Usage: scripts/check-migration-order.sh BASE
#   BASE: a commit, e.g. `$(git merge-base origin/main HEAD)` for a branch,
#   or the pre-push commit for a push. Run it before merging a worktree
#   branch locally: scripts/check-migration-order.sh "$(git merge-base main HEAD)"
set -euo pipefail

base="${1:?usage: $0 BASE}"
dir=crates/api/migrations

version_of() {
    sed -n 's#^.*/\([0-9]\{1,\}\)_[^/]*\.sql$#\1#p'
}

max_base="$(git ls-tree -r --name-only "$base" -- "$dir/" | version_of | sort -n | tail -n 1)"
if [ -z "$max_base" ]; then
    echo "no migrations at $base; nothing to compare"
    exit 0
fi

status=0

# --no-renames reports a rename as a deletion plus an addition, so the old
# name fails here and the new name is checked as an added file below.
changed="$(git diff --no-renames --name-status --diff-filter=MDT "$base" HEAD -- "$dir/" \
    | awk -F '\t' '$2 ~ /\.sql$/ { print $1 "\t" $2 }' || true)"
while IFS=$'\t' read -r kind file; do
    [ -n "$file" ] || continue
    case "$kind" in
        D) what="deleted (or renamed)" ;;
        *) what="modified" ;;
    esac
    echo "::error file=$file::$file already exists on the base and was $what. Merged migrations are immutable: sqlx checks every applied file's checksum at startup. Add a new migration instead."
    status=1
done <<<"$changed"

added="$(git diff --no-renames --name-only --diff-filter=A "$base" HEAD -- "$dir/" | grep '\.sql$' || true)"
for file in $added; do
    version="$(printf '%s\n' "$file" | version_of)"
    if [ -z "$version" ]; then
        echo "::error file=$file::cannot read a numeric version from $file"
        status=1
    elif [ "$version" -le "$max_base" ]; then
        echo "::error file=$file::$file (version $version) is not newer than the newest migration on the base ($max_base); sqlx would apply it out of order. Give it a later timestamp."
        status=1
    else
        echo "ok: $file ($version > $max_base)"
    fi
done
[ -n "$added" ] || echo "no migrations added since $base (newest there: $max_base)"
[ -n "$changed" ] || echo "no existing migrations modified, deleted or renamed since $base"
exit "$status"
