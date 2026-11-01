# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Distant Signal (DS) is a personal UK rail companion: TfL-shaped line status,
train and journey tracking, groups, trip planning and Delay Repay support.
Rust workspace (`crates/`), Next.js frontend (`frontend/`), Helm chart
(`charts/distant-signal/`), TOML line catalogue (`lines/`, `lines/SCHEMA.md`).
README.md explains segments (junctions belong to the shared trunk), match
scopes and the severity scale; DESIGN.md's "pollers POST to `/private/*`"
picture is out of date (see Ingest below).

## Commands

Copied from `.github/workflows/ci.yml`. Rust toolchain: MSRV 1.88 (edition
2024); the rdkafka crates need cmake, libssl-dev, pkg-config, libsasl2-dev
and libcurl4-openssl-dev.

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets --all-features -- -D warnings
cargo clippy --workspace --locked --all-targets --no-default-features -- -D warnings
cargo +1.88.0 check --workspace --locked --all-features --all-targets   # MSRV
cargo deny --locked check bans licenses sources
cargo test --workspace                       # unit tests; #[ignore] DB/Redis tests skipped
cargo test -p api some_test_name             # one crate, tests matching a filter
cargo run -p line-catalogue-validator        # lines/*.toml identifiers
```

DB/Redis-backed tests are `#[ignore]`d and need `DATABASE_URL` (a superuser,
CI uses `postgres://postgres:postgres@localhost:5432/postgres`) and
`REDIS_URL` (`redis://localhost:6379`), with the migrations applied:

```sh
(cd crates/ds-store && sqlx migrate run)     # sqlx-cli 0.8.6, postgres,rustls features
cargo test -p api -p ds-store -p ds-migrate -p aggregator -p notifier -p schedule-reference -p poller-stations -p poller-incidents -- --ignored --test-threads=1
cargo test -p enricher -- --ignored --skip live_eval --skip replay_ --test-threads=1
cargo test -p api some_db_test -- --ignored --test-threads=1   # a single DB test
uv sync --locked --group lint
uv run --no-sync scripts/gen-db-grants.py check --database-url "${DATABASE_URL}"
```

CI builds those test binaries once (`rust-db-build`, a cargo-nextest
archive) and runs its passes from it in three parallel jobs: as the
superuser, as the app role (existing and new cluster), and as each
service's own Postgres role. `scripts/db-tests.py` holds every pass's steps
(each a former `cargo test` line; `list-passes` prints them); with
cargo-nextest installed it runs a pass locally, building from source:

```sh
uv run scripts/db-tests.py list-passes
uv run scripts/db-tests.py run superuser                 # DATABASE_URL, REDIS_URL, FAKE_GCS_URL
uv run --no-sync scripts/test-postgres-roles.py --mode per-service --check-setup-job \
  --observe api,aggregator,enricher,notifier -- \
  uv run --no-sync scripts/db-tests.py run per-service   # DATABASE_URL_<ROLE> per step
```

An `#[ignore]`d benchmark must return early unless its env var is set
(`scripts/check-bench-gates.py`, in scripts-lint): the DB jobs run every
ignored test, in debug.

Frontend (run in `frontend/`, Node 22; `check-copy.py` runs from the repo root):

```sh
npm ci
npm run lint                     # eslint --max-warnings 0
uv run --locked --no-default-groups scripts/check-copy.py   # from the repo root
npm run format:check             # prettier
npx tsc --noEmit
npm run typecheck:scripts        # sw.js and the Node scripts (checkJs)
npm test                         # vitest
npx vitest run components/AppNavBar.test.tsx -t "part of a test name"
npm run build
```

`npm run build` runs `scripts/stamp-sw-version.mjs`, which rewrites the
tracked `frontend/public/sw.js` with the build id. Discard it with
`git checkout -- frontend/public/sw.js` before committing.

E2E and visual suites run against a seeded stack, as CI's `frontend-e2e`
job does (its `env:` block has every variable, including the `E2E_*` fixture
ids, `TFL_BASE_URL: http://127.0.0.1:8099` and `API_TRANSIT_NETWORKS`):
migrate, `psql -v ON_ERROR_STOP=1 "${DATABASE_URL}" -f frontend/e2e/fixtures/seed.sql`,
start `python3 scripts/tfl-arrivals-stub.py --port 8099` (fixed TfL arrivals,
so no test reaches TfL), `cargo build -p api && ./target/debug/api`, then in
`frontend/` `npm run build && npm run start -- -p 3000`, and:

```sh
npx playwright install --with-deps chromium firefox
npx playwright test                          # e2e + axe; skips the visual project
npx playwright test e2e/plan.spec.ts --project=chromium
uv run --locked --no-default-groups scripts/visual-tests.py           # from the repo root
uv run scripts/visual-tests.py --update -- --update-snapshots=all     # regenerate baselines
```

`visual-tests.py` runs the `visual` project in the `mcr.microsoft.com/playwright`
image matching the locked `@playwright/test` (docker or podman). Baselines
(`frontend/e2e/visual-baselines/`) are only valid from that container: never
regenerate them outside it, and review every changed image (docs/style-guide.md,
"Visual regression").

Scripts, chart and migration checks (repo root):

```sh
uv sync --locked --group lint
uv run --no-sync scripts/lint-scripts.py     # shellcheck, shfmt, ruff, mypy, actionlint, hadolint... (--fix: shfmt, ruff fixes)
uv run --no-sync python -m unittest discover -s scripts/tests -v
uv run --no-sync scripts/chart-values-doc.py check   # chart README values table vs values.yaml
uv run scripts/gen-db-grants.py check        # db-grants.yaml vs files/postgres-grants.sql (render to fix)
uv run scripts/gen-rust-dockerfiles.py --check   # rewrite without --check
uv run --no-sync scripts/check-crate-deps.py # ds-store/ds-migrate stay free of axum, redis, reqwest...
uv run --locked --no-default-groups scripts/check-migration-order.py "$(git merge-base origin/main HEAD)"
helm lint --strict charts/distant-signal
helm lint --strict charts/distant-signal -f charts/distant-signal/values-example.yaml
helm lint --strict charts/ds-ingest-bucket
```

CI's `helm-lint` job also asserts on many `helm template --set` renders, and
`scripts-lint` runs the other `scripts/check-*-chart.py`. Local stack:
`docker compose --env-file local.env up --build` (`dev.env` for debug builds
and hot reload; see `docker-compose.yml`'s header).

## Architecture

**Ingest (release B of ingest phase 5; the api has no `/private` routes).**
Producers write Postgres directly or XADD to Redis streams; nothing posts to
the api. poller-incidents, poller-stations, trust-consumer,
trust-backlog-consumer, schedule-ingest and schedule-reference write
Postgres. poller-tocs, poller-tfl, poller-ldbws, full-coverage-consumer and
the island-of-Ireland pollers send snapshots on `ds:ingest:*` streams
(`ingest-stream`: versioned envelope, MAXLEN budgets, dead letters), which
`ingest-writer` applies (dedup, observed-time clamp); the writer also runs
the train-domain background loops. `movement-relay` is the only Kafka
client: it copies RDM's TRUST movements into the `movement-events` stream,
read by three consumer groups (tracked trains, full-coverage delay stats,
a backlog for late-tracked trains). The CIF timetable is pushed over SFTP;
schedule-ingest detects and extracts each delivery, and schedule-reference
(same pod, shared PVC; or a CronJob with `scheduleFeed.reference.mode:
cronjob`, docs/scheduled-jobs.md) publishes the derived tables (STANOX/TIPLOC to CRS,
line populations, departures and calling points) through ds-store's publish
protocol. `aggregator` matches incidents to lines (`common::matcher`, the
same matcher the ingest uses) and writes `line_status`; `enricher` sends
changed incident text to an LLM. `ds-store` holds the shared SQL and the
schema gate; `ds-migrate` runs migrations (the chart's migrate hook Job).
Docs: docs/ingest-phase5-runbook.md, docs/ingest-stream-runtime.md,
docs/superpowers/specs/2026-10-06-ingest-architecture-design.md.

**api (axum).** TfL-shaped unprefixed routes (`/Line/...`, `/StopPoint/...`,
`/Train/...`, `/Journeys/...`, `/Trips/plan`) plus `/public/*`. Browser
users authenticate by OIDC session cookie (`AuthenticatedUser`); cookie
mutations get an Origin check. Delegated access (ds-mcp acting for a user)
is accepted only on a separate in-cluster listener, only on
`DELEGATED_ROUTES`, scoped by the grant (docs/connected-apps.md). The rate
limiter keys on `X-Real-IP` (set by the frontend proxy) with tiers for
login, `/Trips/plan`, `/Train/by-uid` plus TfL arrivals, and other public
writes; the MCP service token gets its own budget and delegated calls
per-user and per-client buckets (`crates/api/src/rate_limit.rs`). The trip
planner (`trip-planner`: CSA and RAPTOR) searches a day's connections array
kept in a `GraphCache` invalidated by schedule publishes; the live overlay
(`data/trip_plan_live.rs`) swaps in cancelled or late trains without copying
the array, including an in-search snapshot of every such train. Journeys own
ordered `journey_legs`, each bound to a `train_subscriptions` row or an open
time window; tracking a train is a one-leg journey. Journey breaks (at most
one open per journey) and append-only revisions record mid-journey changes
(docs/journey-breaks.md, docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md).
"Today" is London's service date: use `london_now()`/`london_today()`.

**notifier.** Polls `line_status_history` and train movements by watermark
and sends Web Push. Rules in `crates/notifier/src/decision.rs`: ranks via
`common::severity_rank` (never `Severity`'s `Ord`); lines notify on
escalation, de-escalation after a cooldown, never twice for the same rank;
trains and skipped stops notify on escalation only. One push per user per
train: a journey-break notice replaces that train's plain push
(`push_queue.rs`).

**Postgres roles.** `charts/distant-signal/files/db-grants.yaml` classifies
every public table, view, sequence and function and gives each service role
(`planned`, `observed` or `narrow`) its grants; `gen-db-grants.py render`
produces `files/postgres-grants.sql`. A new table must be added there (CI
checks a migrated DB against it). docs/postgres-app-role.md and the chart
README's "Per-service Postgres roles".

**Chart and deploys.** One chart for the whole stack, documented in
`charts/distant-signal/README.md` (keep its values reference in step,
`chart-values-doc.py`). `containers.yml` builds the images and pushes the
chart to `oci://ghcr.io/fasterspeeding/charts` only from `main`. Production
values live in Ranma-Config and Flux applies them. Renovate opens a PR per
chart build, but DS releases are filed deliberately as deploy-bringer
requests (with deploy notes: migrations, grants, flags, post-deploy checks)
and approved by the user's passkey; a green `main` can reach prod soon. Put
risky changes behind off-by-default values. Agents only read production.

**Frontend.** Next.js app router (`frontend/app/`), Mantine 9 with a grape
primary, theme in `lib/theme.ts`. Browsers only talk to the frontend; its
`app/api/[...path]` route proxies to the api. Rules for look, copy and
components are in docs/style-guide.md (no inline `style` props, enforced by
ESLint; hand-inlined SVG icons). PR template: screenshots at 390 and 1366 px,
light and dark; page title and meta description; `aria-label` on icon buttons.

docs/superpowers/specs and plans are dated historical records (status in their
README index); code, the chart README and top-level docs/ are the truth.

## Conventions

- **Migrations** (`crates/ds-store/migrations`, sqlx): forward-only; a merged
  file is immutable (sqlx checksums it, an edit crash-loops the api). A new
  migration's timestamp must be newer than every migration on main
  (`check-migration-order.py`). Append its line to
  `crates/api/tests/migration_checksums.lock` once it merges. Lock-taking
  statements need `SET LOCAL lock_timeout` first; indexes on existing tables
  are `CREATE INDEX CONCURRENTLY` in a file whose first line is
  `-- no-transaction`; constraints go on `NOT VALID` and are validated in a
  later migration (`SET NOT NULL` via a validated `CHECK`). Destructive DDL
  (drops, renames, type changes, `SET NOT NULL`) needs a
  `-- contract: <what> (code stopped using it in <commit>)` header and ships
  a release after the code stopped using it. Enforced by
  `crates/api/tests/migration_index_locking.rs` and `migration_checksums.rs`.
- **API changes** get an entry in docs/api-changelog.md (newest first, what a
  client like DS-MCP needs to know).
- **Copy**: sentence case, "Couldn't <verb> <noun>." failures via
  `describeFailure`, no em dashes, ellipses or arrows, no internal
  vocabulary (ingest, TOC, CRS, "sign in"). `scripts/check-copy.py` holds the
  banned list; docs/style-guide.md, "Writing", has the rest.
- **Commits** carry a `Task: <card>` trailer (e.g. `Task: DS-123`) naming the worklog card.
- **Scripts** are typed stdlib Python (ruff, mypy) run with uv (`uv.lock`,
  `pyproject.toml`'s `lint` group, `.python-version`); bash only for trivial
  one-liners. No pip or requirements files.
- Rust: line data only from `lines/*.toml`; tests in inline `mod tests`, DB
  tests `#[ignore]`d; runtime `sqlx::query`, no `query!` macros.
- Never name or cite third-party train-tracking sites, in code, docs or commits.
