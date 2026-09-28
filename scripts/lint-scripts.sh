#!/usr/bin/env bash
# Runs the same checks as CI's `scripts-lint` job over the repo's shell and
# Python scripts and the workflow run: blocks.
#
# Needs the tools pinned in pyproject.toml's `lint` dependency group on PATH,
# e.g. (pip >= 25.1, for --group):
#   python3 -m venv ~/.venvs/ds-lint
#   ~/.venvs/ds-lint/bin/pip install --group lint
#   PATH="${HOME}/.venvs/ds-lint/bin:${PATH}" scripts/lint-scripts.sh
#
# Usage: scripts/lint-scripts.sh [--fix]
#   --fix: apply shfmt and ruff format/autofixes first, then check.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

sh_list="$(git ls-files -- '*.sh')"
mapfile -t sh_files <<<"${sh_list}"

if [[ "${1-}" == "--fix" ]]; then
    shfmt -w "${sh_files[@]}"
    ruff format
    ruff check --fix
fi

status=0
run() {
    echo "== $*"
    "$@" || status=1
}

run shellcheck "${sh_files[@]}"
run shfmt -d "${sh_files[@]}"
run ruff check
run ruff format --check
run mypy
# actionlint passes --norc to shellcheck, so .shellcheckrc does not reach the
# workflow run: blocks; SHELLCHECK_OPTS does.
run env SHELLCHECK_OPTS='--enable=all --severity=style' actionlint

exit "${status}"
