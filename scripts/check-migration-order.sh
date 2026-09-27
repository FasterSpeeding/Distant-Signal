#!/usr/bin/env bash
# Fails if a migration added since BASE has a version that is not greater
# than every migration BASE already had (DB review 2026-09-27, A4/DB2-35).
#
# sqlx applies any local migration missing from _sqlx_migrations whatever
# its version, so a branch whose migration timestamp predates one already
# merged (and deployed) runs it late, out of order, with no error. Give it a
# later timestamp instead.
#
# Editing or renaming an existing migration is caught separately by
# crates/api/tests/migration_checksums.rs (checksum lock).
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

added="$(git diff --no-renames --name-only --diff-filter=A "$base" HEAD -- "$dir/" | grep '\.sql$' || true)"
status=0
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
exit "$status"
