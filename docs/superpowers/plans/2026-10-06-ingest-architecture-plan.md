# Ingest architecture: implementation plan

Spec: [2026-10-06-ingest-architecture-design](../specs/2026-10-06-ingest-architecture-design.md).
Read the spec first. Section numbers (§) below refer to it.

**The goal.** The public `api` serves customers and the MCP only.

- The massive-dump producers write Postgres directly, with narrow roles:
  schedule-reference, poller-stations, poller-incidents and schedule-ingest.
- The small producers go through Redis Streams to an `ingest-writer`.
- The TRUST backlog consumer writes directly (R1).
- Internal reads are direct and read-only (R2).
- Migrations and loops have one owner each (R3).

Decisions D1–D15 (spec §16, 2026-10-06 and 2026-10-07) apply. Phase 0 is in progress
(chart and tooling, off by default). Phase 1A waits for the in-flight api
branches to merge (D4).

## Ground rules for whoever executes this

- **Follow `/home/coder/ds-review/fix-brief-common.md`:**
  - rustc 1.88, with no dependency above it;
  - the Cargo.lock pins;
  - the disk-budget cargo environment and the shared `CARGO_TARGET_DIR`;
  - the assigned migration timestamp range only;
  - index DDL in its own `-- no-transaction` file;
  - `SET LOCAL lock_timeout = '5s';` in transactional migrations;
  - never edit an existing migration.
- **New tooling is typed stdlib Python**, run with `uv run`, ruff and
  mypy `--strict` clean, with dependencies in `pyproject.toml` groups and
  `uv.lock`. Config starts from the strict-config baseline; every deviation
  carries a reason comment.
- **Every new switch ships with today's behaviour as the default.** The
  defaults are `sink: http`, `source: http`, writer streams `off`,
  `ingestWriter.enabled: false`, `migrate.job.enabled: false`,
  `api.migrateOnStartup: true`, `API_BACKGROUND_LOOPS: true`,
  `api.privateRoutes.enabled: true` and `api.strategy.type: Recreate`. List
  every off-by-default setting in the report.
- **One task, one commit, tests in the same commit.** Commit messages end
  with the `Co-Authored-By` line the session gives.
- **Never put a credential in the repo**, including test fixtures. DB and
  Redis test passwords are generated at test time.
- **Every phase ends with the full verification list** from the brief:
  - fmt;
  - clippy `-D warnings`;
  - the 1.88 check;
  - `cargo test --workspace`;
  - the DB-gated suites on a fresh database, run both as the app role and,
    from phase 0b on, per service role (`scripts/test-postgres-roles.py`);
  - the Redis-gated suites;
  - `uv run scripts/lint-scripts.py`;
  - `helm lint` and the CI render flags;
  - `uv run scripts/chart-values-doc.py check`.
- **Production is read-only for agents.** Flips are Ranma-Config values
  changes made by the user or Ranma.

## Phase 0: prerequisites

**Status (2026-10-06): the DS side is built, every switch off.** 0b.1–0b.7
and 0c.1–0c.4 are done (`db-grants.yaml`, `gen-db-grants.py`,
`postgresql.roles.perService`, `observe-role-usage.py`,
`test-postgres-roles.py --mode per-service`, `redis.acl`,
`redis_url_with_credentials`, `crates/common/tests/redis_acl.rs`,
`docs/redis-acl.md`), with CI. Ranma's order: `docs/ingest-phase0-runbook.md`.
Differences from the table below:

- 0b.5: the existing INF-7 pool check stays in `api-deployment.yaml` (it
  now uses the api's effective pool); the new role-limit sum is a separate
  helper, `distant-signal.postgresRoleBudgetCheck`, counting only the roles
  a client actually connects as.
- 0c.1: the api reads `REDIS_USERNAME` from the environment instead of a
  new `Config` field, so the ~20 api test modules that build `Config` are
  untouched while api branches are in flight (D4); it moves into `Config`
  in phase 1A.
- 0c.3: the conformance test creates the users with `ACL SETUSER` (under a
  random prefix) on CI's Redis service instead of starting a server with the
  file; the file itself is checked by `check-ingest-phase0-chart.py` against
  `render-redis-acl.py`, and was booted once by hand on valkey.
- Not in phase 0 (unchanged plan): the migrate hook Job, the schema gate and
  `api.migrateOnStartup` (1B.1–1B.4, they need `ds-migrate`); the pool
  metrics (1A.11). 0a and 0d are Ranma's.

### 0a. The Postgres role split (Ranma; DS supports)

| # | Task | Files | Tests / verification |
|---|---|---|---|
| 0a.1 | Support Ranma's Stage A–C rollout of `postgresql.roles` (`docs/postgres-app-role.md`). Read-only checks after each stage: `pg_roles`, ownership, `\ddp`, `pg_stat_activity.usename` | none (Ranma-Config) | the runbook's verification queries; no `permission denied` (42501) in any service log for a day |

Exit: every chart service connects as `distant_signal_app`, migrations run
as `distant_signal_owner`, and the exporter, dump and backup use their own
roles.

Rollback: the runbook's.

### 0b. Grants as data, and observed per-service roles

| # | Task | Files | Tests |
|---|---|---|---|
| 0b.1 | `db-grants.yaml`: classify all 69 tables, the sequences, the function and (later) the views, as spec §6.4 | `charts/distant-signal/files/db-grants.yaml` | – |
| 0b.2 | `gen-db-grants.py`: `render` writes `postgres-grants.sql` (group roles; per-service `LOGIN` roles that are, for now, **members of `distant_signal_app`** with no grants of their own; `CONNECTION LIMIT`s; the `schema_gate` and `read_shared` groups). `check --database-url` fails on any `public` object missing from the YAML, on stale YAML, or on a stale SQL file | `scripts/gen-db-grants.py`, `charts/distant-signal/files/postgres-grants.sql`, `pyproject.toml` (PyYAML in the scripts group), `uv.lock` | `scripts/tests/test_gen_db_grants.py`: classification gaps, stale output, limits summing over budget |
| 0b.3 | CI: after the `rust-db-test` migration step, run `gen-db-grants.py check` against the fresh DB | `.github/workflows/*.yml` | CI green; a deliberately unclassified table in a scratch branch fails |
| 0b.4 | Chart: the setup Job also runs `postgres-grants.sql`. Per-service passwords: one `existingSecret` per role, each its own SealedSecret in Ranma (Q12, decided D15), each defaulting to generated values as today. Each Deployment gets its own `DATABASE_URL` user when `postgresql.roles.perService` is on (default off) | `templates/postgres-roles.yaml`, `_helpers.tpl` (`distant-signal.databaseEnv` takes a role), `values.yaml`, chart README | `helm template` with `perService` on and off: the default render is unchanged; each Deployment names its own role |
| 0b.5 | The render-time connection budget sums every role's limit (spec §6.6) | `templates/api-deployment.yaml` (the INF-7 check moves to `_helpers.tpl`) | render fails at 98; passes at 92 |
| 0b.6 | `observe-role-usage.py` (read-only): reads `pg_stat_statements` joined to `pg_roles` and extracts table and verb per role from the normalised text. It writes a Markdown report with a diff against `db-grants.yaml` | `scripts/observe-role-usage.py` | unit tests on fixture query texts (CTEs, `UPDATE … FROM`, `INSERT … SELECT`, `ON CONFLICT`) |
| 0b.7 | `test-postgres-roles.py --mode per-service`: migrate as owner, apply both SQL files, then each crate's DB suite as its service's role (still members of `app`, so it passes; the mode is ready for narrowing later) | `scripts/test-postgres-roles.py` | CI job |

Rollout (Ranma):

1. `perService: true`, one service at a time.
2. After 7 days, run `observe-role-usage.py` and commit the report as an
   appendix to the plan's tracking issue.

Exit:

- every service is on its own role (`pg_stat_activity.usename`);
- the report exists, and its differences from the YAML are explained or
  fixed.

Rollback: `perService: false`. The services are back on `app`.

### 0c. Redis per-client ACL users

| # | Task | Files | Tests |
|---|---|---|---|
| 0c.1 | `common::redis_auth::redis_url_with_credentials(url, user, password)`; every Redis client reads `REDIS_USERNAME` (optional; absent means `default`, as today) | `crates/common/src/redis_auth.rs`; the Redis-using crates' config (api, enricher, movement-relay, the three consumers) | unit: the redis crate parses back the same user and password; an absent user is unchanged |
| 0c.2 | ACL template and render: `files/redis-users.acl.tpl` with the users of spec §8.2, including the future stream users with final rights. A Redis initContainer renders `users.acl` into a `medium: Memory` emptyDir from per-user env passwords. `redis.acl.enabled` (default off) switches `--requirepass` to `--aclfile`. `redis.acl.stage: open\|narrow` picks the step-1 (`+@all`) or step-3 rights; `redis.acl.defaultUser: on\|off` | `charts/distant-signal/files/redis-users.acl.tpl`, `templates/redis-deployment.yaml`, `templates/*-deployment.yaml` (`REDIS_USERNAME` plus a per-user `secretKeyRef`), `values.yaml`, README | `helm template` with each stage; the default render is unchanged |
| 0c.3 | ACL conformance test: start Redis or valkey with the rendered `narrow` file, then run each client's real command sequence (relay publish and trim and group create and INFO; consumer read, ack, claim and dead-letter; enricher; a producer XADD and XREVRANGE; writer; exporter) as its user. Also assert the forbidden ones fail (`FLUSHALL`, a producer XADD to another stream, a consumer `XADD movement-events`) | `crates/common/tests/redis_acl.rs` (ignored, Redis-gated), `scripts/render-redis-acl.py` (renders the template for the test) | the test itself |
| 0c.4 | Runbook: the four steps of spec §8.4, with checks (`CLIENT LIST`, `ACL LOG`, the exporter's `redis_up`) and rollback | `docs/redis-acl.md` | – |

Rollout (Ranma), each step its own release:

1. Create the SealedSecret `distant-signal-redis-users`, then set
   `acl.enabled`, `stage: open`.
2. Set `REDIS_USERNAME` on each client, one at a time; the exporter in
   Ranma.
3. `stage: narrow`.
4. `defaultUser: off`.

Exit: `ACL LIST` shows `default off`; `CLIENT LIST` shows no `user=default`;
`ACL LOG` is empty for 7 days.

Rollback: the previous step's values.

### 0d. NetworkPolicy narrowing (Ranma; DS checks)

| # | Task | Files | Tests |
|---|---|---|---|
| 0d.1 | Ranma narrows `allow-ingress-same-namespace`. DS checks, from Loki or a short packet-flow log, that every flow into api, postgres and redis matches the chart's lists, and fixes the chart first where they do not match | possibly `templates/networkpolicy.yaml` | `helm template` netpol tests |

Exit: Ranma's same-namespace allow no longer admits all pods to all pods.

## Phase 1: `ds-store`, migrations and loops

Entry: 0a done, CI green.

### 1A. The behaviour-neutral extraction

Every task is a **move**. Each one:

- moves the functions, types and DB tests unchanged;
- leaves `pub use ds_store::…` shims in `crates/api/src/data/*`, so no call
  site changes;
- keeps the api's SQL text, metric names and routes identical.

Verification for each task, besides the standard list:

- `scripts/diff-api-surface.py` (new in 1A.1): an api `/metrics` series-name
  list and a normalised dump of every SQL string literal in `api` plus
  `ds-store`, compared with the base commit. A move changes neither;
- `scripts/check-crate-deps.py` (new in 1A.1): `ds-store`'s normal
  dependency closure still holds no forbidden crate;
- the moved DB tests pass in their new home.

**How a mover runs the checks.** From the worktree root, with the
uncommitted or committed move in place:

```
export UV_PROJECT_ENVIRONMENT=<scratchpad>/venv
uv run scripts/diff-api-surface.py diff "$(git merge-base HEAD main)"
uv run scripts/check-crate-deps.py
```

- `diff BASE` compares BASE (read through `git show`, no checkout) with
  the work tree, untracked `ds-store` files included; `diff BASE HEAD`
  compares two commits. Exit 0 means no change, 1 a change (the unified
  diff is printed), 2 a git error.
- The metric list is read statically: macro and `metric_name(...)`
  arguments, with constants resolved. A scrape of a running api would
  show only series already observed or registered, and needs Postgres,
  Redis and auth config.
- SQL literals in test code (`tests/`, `#[cfg(test)]` items and
  `#[cfg(test)] mod x;` files) form their own `test-sql` section. If a move
  must duplicate a test fixture's SQL, rerun with `--ignore-test-sql` and
  say why in the commit message. Production `sql` and `metrics` must never
  differ.
- In a two-commit split (1A.9: copy, then delete), only the second commit
  must be clean; the copy commit shows each copied literal twice.
- `uv run scripts/diff-api-surface.py dump [REV]` prints one tree's surface.

**What 1A.1 set up so the moves need not touch shared files.**

- Every `ds-store` module in the table below already exists as an empty,
  documented file declared in `lib.rs`. A move fills its own file (or
  turns `trains.rs` into `trains/mod.rs`) and does not edit `lib.rs`.
- `crates/ds-store/Cargo.toml` already declares the spec §5.1 dependency
  set: `common` (no default features, `postgres`), `sqlx` without `macros`
  or `migrate` (with `derive`), `chrono`, `chrono-tz`, `serde`,
  `serde_json`, `anyhow`, `tracing`, `metrics`, `rand`, `tokio`,
  `trust-schema` and `schedule-query`.
- `common`'s reqwest-based modules and `metrics::install` are behind a new
  default `http` feature, so `ds-store` can depend on `common` without
  hyper or reqwest. A moved function that needs one of those modules does
  not belong in `ds-store`.
- CI already runs `cargo test -p ds-store -- --ignored` wherever it runs
  the api's DB tests: as the superuser, as the app role (both cluster
  modes) and per service (as the api role).
- The api already depends on `ds-store`.

**Keeping parallel moves mergeable.** Put each `pub use ds_store::…` shim
where the moved item was, not in a block at the top of the file. Then two
moves out of the same file (`queries.rs`, `routes/ingest.rs`) change
separate places, with unchanged lines between them, and merge cleanly.

| # | Task | Files | Tests |
|---|---|---|---|
| 1A.1 | **Done (2026-10-07).** Create the `crates/ds-store` crate (workspace member, lints, `publish = false`, sqlx without `migrate`/`macros`), with every 1A module declared empty. Put `common`'s HTTP client and metrics listener behind a default `http` feature. Add `check-crate-deps.py`: `ds-store`'s normal dependency closure must not contain `axum`, `tower`, `hyper`, `redis`, `reqwest`, `oauth2`, `openidconnect` or `api`. Add `diff-api-surface.py` | `Cargo.toml`, `crates/common/{Cargo.toml,src/lib.rs,src/metrics.rs}`, `crates/ds-store/`, `scripts/check-crate-deps.py`, `scripts/diff-api-surface.py`, CI | the scripts' unit tests under `scripts/tests/` |
| 1A.2 | `validate`: `validate_short_text`, `validate_code_list`, `is_crs_code` from `routes/mod.rs`; `routes` re-exports them | `ds-store/src/validate.rs`, `api/src/routes/mod.rs` | moved unit tests |
| 1A.3 | `freshness`: `record_ingest` (now `pub`), every `last_*_fetch`, `data_freshness`, `last_per_key`, `normalize_code` | `ds-store/src/freshness.rs`, `api/src/data/queries.rs` | moved |
| 1A.4 | `trains`: `trains.rs`'s shared functions, `stop_delay.rs`, `stop_live_status.rs`, `eta_blend::london_to_utc`, and the `JourneyStop`/`StopStatus`/`StopTimetable` types (re-exported from `journey.rs`) | `ds-store/src/trains/{mod,types,stop_delay,stop_live_status}.rs`; `api/src/data/{trains,stop_delay,stop_live_status,eta_blend,journey}.rs` | moved DB tests, also as the api role |
| 1A.5 | `samples`: the station-sample, full-coverage, TfL and IoI upserts and readers (spec §5.2 table) | `ds-store/src/samples.rs`; `api/src/data/{queries,full_coverage_window,island_of_ireland}.rs` | moved |
| 1A.6 | `reference` and `corpus` (spec §5.2), plus `corpus_load_problem`, `is_sha256_hex` and `schedule_feed_ingest_problem` from `routes/ingest.rs` | `ds-store/src/{reference,corpus,corpus_crosswalk}.rs`; `api/src/data/{queries,corpus,corpus_crosswalk,corpus_comparison}.rs`; `api/src/routes/ingest.rs` | moved, plus the validation unit tests |
| 1A.7 | `schedule`: the publish protocol and products (spec §5.2), `SchedulePublishPart::new` (the chunk-parameter validation from `ScheduleChunkParams`) | `ds-store/src/schedule/{mod,publish,population,markers}.rs`; `api/src/data/queries.rs`; `api/src/routes/ingest.rs` | moved: the 2026-09-27 regression tests, PL-14, and the `analyze_publish_keys` `reltuples` test |
| 1A.8 | `incidents`: `upsert_incident_snapshot` and helpers, `incident_removal.rs`, `parse_snapshot`. **Not yet split from Redis**: the function takes a `FnOnce(Vec<String>)` publish callback, and the api passes `publish_text_changed`. This keeps Redis out of `ds-store` and the order unchanged | `ds-store/src/incidents/{mod,removal}.rs`; `api/src/data/{queries,incident_removal}.rs`; `api/src/routes/ingest.rs` | moved; the publish-order test (publish after commit, before inference) |
| 1A.9 | Split `train_tracking.rs`: the ingest half (spec §5.2 `tracking`) moves, the user half stays; `notifier_forward_queue` moves | `ds-store/src/tracking.rs`; `api/src/data/{train_tracking,notifier_forward_queue}.rs` | moved; the `git log --follow`-friendly split is 2 commits (copy, then delete) |
| 1A.10 | `backlog` (`trust_event_backlog.rs`, `train_reasons` writers, `trust_event_backlog_match.rs`) and `sweeps` (`schedule_matching`, `reconciliation`) | `ds-store/src/{backlog,sweeps}/…`; the api files; `api/src/bin/replay_uidless_movements.rs` | moved |
| 1A.11 | `pool`: a `PoolSettings` wrapper that records `db_pool_*` (spec §14.1) and the DB health probe for any service; the api switches to it (new metrics only, nothing removed) | `ds-store/src/pool.rs`, `api/src/app.rs`, `api/src/data/db_health.rs` | unit: gauges registered at 0; DB: `in_use` rises under a held connection |
| 1A.12 | `schema::REQUIRED_MIGRATION` (no caller yet) | `ds-store/src/schema.rs` | unit: equals the newest file in `crates/api/migrations` |

Exit: the api's surface diff is empty against the pre-1A base. `ds-store`
holds every function in spec §5.2 except `migrate` and `reads`. The api
exports `db_pool_*` in production.

Rollback: revert the commit (no runtime switch).

### 1B. Migrator, schema gate, writer skeleton, loops, maintenance CronJob

**Status (2026-10-07): the parts that do not need `ds-store` are built,
every switch off.** The default render is unchanged.

- **Done:** 1B.1 (`ds-migrate run` and `wait`); 1B.5; the chart parts of 1B.4, 1B.8, 1B.9 and 1B.10; 1B.6
  (skeleton, image and the real loops); 1B.7; 1B.8's `maintenance` bin.
- **Done in code:** 1B.2 (`wait_for_schema`, which the writer calls before
  readiness and before registering its loops) and 1B.3 (the api reading
  `API_MIGRATE_ON_STARTUP`; the other DB services gated).
- **Waiting on code:** nothing; what is left is turning the switches on
  (the migrate Job with `api.migrateOnStartup: false`, the writer's loops).

Details and differences from the table below:

- **1B.1, done.** `ds_store::migrate` is `api::migrate` moved whole (the
  api keeps `pub use ds_store::migrate`), plus
  `migrate::contract::ensure_ready_for_contract_migration` and the catalog
  helpers the api's `legacy_backfill` still uses (it re-exports the
  guard). `crates/ds-store/migrations` is a pure rename, so no checksum
  changed; `migration_checksums` and `migration_index_locking` stay in the
  api, pointed at the new directory, and `check-migration-order.py` reads
  a BASE from before the move from `crates/api/migrations`, file by file.
  `crates/ds-migrate` (in the api image as `/usr/local/bin/ds-migrate`):
  `run` does the api's startup sequence (the contract check, then
  `migrate::run`) with `MIGRATION_DATABASE_URL`, else `DATABASE_URL`;
  `wait` runs the schema gate from outside a service
  (`schema::wait_for_schema_with` on one lazy connection, so a database
  not up yet is retried until the deadline): the migration only, or with
  `--role <api|aggregator|enricher|notifier|writer>` (`DS_MIGRATE_ROLE`)
  also that role's `db-grants.yaml` grants, checked as the `DATABASE_URL`
  user. The chart does not run it (the Job runs `run`; each service gates
  itself); it is for scripts, init containers and operators. `check-crate-deps.py` holds ds-migrate to ds-store's
  rules. The moved DB tests are `ds_store::migrate::tests` (the CI filter
  `migrate::tests` still selects them).
- **1B.2.** `ds_store::schema::wait_for_schema(pool, DbRole, progress)`;
  `wait_for_schema_with` takes a `SchemaGate` (required version,
  privileges, poll interval, deadline) so the DB tests run in under a
  second. The privileges are per role key: the role's own grants in
  `db-grants.yaml` (tables and views; `has_column_privilege` for a column
  grant) plus SELECT on every `read_shared` table for a member of that
  group. `build.rs` parses the file's YAML subset itself (no YAML crate);
  a shape it does not know fails the build. A missing table or column
  counts as a missing privilege, and a check that errors (no
  `_sqlx_migrations` yet, a dropped connection) is retried, both until the
  deadline. `progress` is beaten on every poll, so liveness stays up while
  the gate waits. The gauge is `distant_signal_db_schema_ready`, no
  labels. The DB tests shadow `_sqlx_migrations` with a TEMP table, so they
  never write the real one (the app role cannot).
- **1B.3.** `API_MIGRATE_ON_STARTUP` is read in `api/src/main.rs` (unset
  or empty = true; anything but true/false fails startup), not in
  `Config`, so the ~25 test `Config` literals are untouched. With false,
  `run_startup` runs the gate on the api's pool, and skips the
  legacy-backfill check, which belongs to the migrator. The aggregator,
  enricher, notifier and ingest-writer gate before readiness and their
  loops, each with its own `db-grants.yaml` role; each crate has a DB test
  that its role passes, which CI's per-service step runs as that role.
- **Pool histogram buckets.** `distant_signal_db_pool_acquire_seconds`
  gets 1 ms-5 s buckets from `common::metrics::SHARED_BUCKETS`, which
  `common::metrics::install` and the api's recorder
  (`api::route_metrics::install_recorder`, replacing axum-prometheus's
  `with_default_metrics` with the same request-duration buckets plus
  these) both apply. Before, it rendered as a summary.

- **1B.4 (chart).** The `pg_isready` wait is an init container from the
  Postgres image (the api image has no libpq tools), not
  "initContainer-free". The Job's Postgres admission and egress are hook
  NetworkPolicies (weight -10): a `pre-upgrade` hook runs before the
  release's own policies change. The role setup Job gets an egress policy
  (Ranma's constraint). Without the role split the Job migrates as
  `postgresql.auth.username`, or with the external database's URL.
  `api.migrateOnStartup: false` without the Job fails the render. The
  chart renders `API_MIGRATE_ON_STARTUP` only when false, so the default
  render is unchanged.
- **1B.5.** The check covers migrations added since the base plus every
  one newer than `CONTRACT_CHECK_CUTOFF` (20261007210000); the 28
  destructive statements in 9 older, applied migrations are grandfathered
  (their checksums are locked). It also counts `DROP
  PROCEDURE/TYPE/SEQUENCE/SCHEMA/MATERIALIZED VIEW`, scans `DO` blocks, and
  exempts changes to objects created in the same file.
- **1B.6, done (with the 1B.2 schema gate before its loops).**
  - `crates/ingest-writer` has no dependency on `api`. It has config,
    `health-http`, metrics, the writer pool (`common::pg`, 6) and the line
    catalogue.
  - The generic loop runner (`ds_store::loops::runner`, moved from the
    writer for 1B.7) runs each named loop on its interval while its
    `LockSession` holds `pg_try_advisory_lock(key)`. The writer's
    `LockSession` is one dedicated connection beyond the pool: role
    limit = pool + 1, and the chart counts both in its budgets. A lock is
    held across ticks until the process exits; a runner whose lock is held
    elsewhere skips the tick (debug log,
    `distant_signal_loop_ticks_total{outcome="skipped"}`).
  - `INGEST_WRITER_LOOPS` (default false) registers the no-op `canary`
    (`INGEST_WRITER_CANARY_INTERVAL_SECS`, 60) and the four train-domain
    loops, `ds_store::loops::{schedule_match, reconciliation,
    backlog_match, corpus_crosswalk}`: the api's intervals, variable names
    and defaults (`SCHEDULE_MATCH_INTERVAL_SECS`,
    `RECONCILIATION_SWEEP_INTERVAL_SECS`,
    `SCHEDULE_ENRICHMENT_GRACE_MINUTES`,
    `BACKLOG_MATCH_SWEEP_INTERVAL_SECS`), and
    `INGEST_WRITER_CORPUS_CROSSWALK_INTERVAL_SECS` (600) for the CORPUS
    `rebuild_if_stale` plus freshness gauge. The CRS-to-line index is built
    from `LINES_DIR` as the api builds it.
  - Each body logs the api loops' own messages; the runner only counts. The
    runner's metrics are one family whichever process runs a loop:
    `distant_signal_loop_{cycles_total, last_success_timestamp_seconds,
    ticks_total, lock_held, seconds}` (they were
    `distant_signal_ingest_writer_*` in the never-deployed skeleton).
  - The lock keys are `common::advisory_locks` (`SCHEDULE_MATCH_SWEEP`,
    `RECONCILIATION_SWEEP`, `BACKLOG_MATCH_SWEEP`, `CORPUS_CROSSWALK`,
    `WRITER_CANARY`).
  - `wait_for_schema` runs before readiness and before the loops are
    registered (1B.2, done).
  - The writer role is still a member of app, so it has every privilege
    the loops use. Against its planned grants in `db-grants.yaml` (read
    from the sweeps' SQL, not yet run as a narrowed role) nothing is
    missing: `train_subscriptions` SU, `trains` SIU,
    `train_movement_events`/`train_current_state` SIU,
    `trust_event_backlog` SU, the three `corpus_*` crosswalk tables SIUD,
    and the schedule and reference tables through `read_shared`.
  - `docker/ingest-writer.Dockerfile` has the generated cargo-chef builder
    (`scripts/gen-rust-dockerfiles.py`); `containers.yml` has its matrix
    leg and the digest step's `SERVICE_TO_PATH` entry `.ingestWriter`.
  - The chart's env contract (1B.9): `DATABASE_URL`,
    `DATABASE_MAX_CONNECTIONS` (default 6, the crate's), the other
    `DATABASE_*` pool settings, `INGEST_WRITER_LOOPS`, `METRICS_ENABLED`,
    `METRICS_PORT` (9091), `HEALTH_BIND_URL` (8090),
    `PROGRESS_STALL_SECS`, `RUST_LOG` (plus `ingestWriter.extraEnv`).
    `LINES_DIR` is the image's `/app/lines` default.
    `RECONCILIATION_SWEEP_INTERVAL_SECS`,
    `SCHEDULE_ENRICHMENT_GRACE_MINUTES` and
    `BACKLOG_MATCH_SWEEP_INTERVAL_SECS` are rendered from the api's values,
    so the two cannot drift.
  - DB tests (`crates/ds-store/tests/loop_locks.rs`): two runners, one
    body run per tick; skip while held elsewhere; interval; reconnect
    after a killed lock session; and the 1B.7 tests below.
- **1B.7, done.** The api's three periodic sweeps are the same
  `ds_store::loops` specs the writer runs, on a `LoopRunner` whose
  `LockSession::from_pool` holds one connection checked out of the api's
  own pool (closed, never returned, when let go), so the connection
  budgets are unchanged. Its CORPUS startup task is
  `ds_store::loops::runner::run_once`: it takes `CORPUS_CROSSWALK` for the
  one run and releases it, and is skipped while the writer holds it.
  `API_BACKGROUND_LOOPS` (default true; chart `api.backgroundLoops`,
  rendered only when false) gates all of them and
  `session_cleanup_sweep_loop`, which takes no lock (the CronJob replaces
  it). Differences from before, on by default: the api's sweeps now take
  advisory locks (with more than one api replica, one replica runs each),
  export the `distant_signal_loop_*` metrics, and the CORPUS errors read
  "CORPUS crosswalk rebuild failed" and "CORPUS freshness gauge read
  failed" (no "startup"); a zero interval logs and leaves that sweep off
  instead of panicking its task. DB tests: an api-shaped and a
  writer-shaped runner tick the real sweeps (one runs, the other skips,
  per loop per tick; the writer takes over once the api's session
  closes); spawned side by side at 100 ms they never run one sweep's body
  twice at once; the api's CORPUS one-shot never keeps the lock.
- **1B.8, done.** `crates/api/src/bin/maintenance.rs` runs one pass of
  `prune_expired_sessions`, `prune_dead_links` and `prune_personal_data`.
  It reuses the existing functions and the api's retention variables
  (which the CronJob renders from the api's values), and exits non-zero
  if any step failed. The api image builds it as
  `/usr/local/bin/maintenance`, the CronJob's default command. Its pool
  (2) counts in the connection budgets. It has a DB test.
- **1B.9 (chart).** The writer role is `observed` in `db-grants.yaml`, so
  the setup Job creates it whenever `perService.enabled` (unused until
  `perService.writer.connect`, which needs `ingestWriter.enabled`). The
  PodMonitor entry is in the shared PodMonitor.
- **1B.10 (chart).** The surge pod's pool counts in the INF-7 budget and
  the app role's computed limit, so `RollingUpdate` needs a smaller api
  pool (or the api on its own role).

| # | Task | Files | Tests |
|---|---|---|---|
| 1B.1 | Move `api::migrate` and `legacy_backfill::ensure_ready_for_contract_migration` to `ds_store::migrate`, and `crates/api/migrations` to `crates/ds-store/migrations` (D9; a rename, every file byte-identical); add the `crates/ds-migrate` binary (`run`, `wait`); build it into the api image | `ds-store/src/migrate.rs`, `crates/ds-store/migrations/`, `crates/ds-migrate/`, `docker/api.Dockerfile`, `api/src/main.rs` (uses `ds_store::migrate`), and every path naming the directory (`migration_checksums`, `migration_index_locking`, `check-migration-order.py`, `gen-db-grants.py`, `test-postgres-roles.py`, CI, Dockerfiles, docs) | moved migrate tests (the INVALID-index heal, the role split test `the_app_role_has_dml_only_and_the_owner_owns_the_schema`); `migration_checksums` and `migration_index_locking` unchanged |
| 1B.2 | `schema::wait_for_schema` (5 s poll, 15 m deadline, `db_schema_ready` gauge, a `has_table_privilege` check per required table from `db-grants.yaml`, baked in at build time by `build.rs`) | `ds-store/src/schema.rs`, `ds-store/build.rs` | DB: waits until a later migration is applied, then returns; times out and errors |
| 1B.3 | `api.migrateOnStartup` (`API_MIGRATE_ON_STARTUP`, default true). When false, `run_startup` runs `wait_for_schema` instead of `migrate::run`. The aggregator, enricher and notifier call `wait_for_schema` before their loops | `api/src/main.rs`, `aggregator/src/main.rs`, `enricher/src/main.rs`, `notifier/src/main.rs`, `values.yaml` | adapted `run_startup` tests: gate, then loops, then bind; a failed gate starts nothing |
| 1B.4 | Chart `migrate-job.yaml`: hook `pre-upgrade,post-install`, weight -5, `before-hook-creation`, `backoffLimit: 1`, `activeDeadlineSeconds: 900`, a `pg_isready` wait, owner credentials only here; `migrate.job.enabled` (default false). With the Job on and `migrateOnStartup` false, the api gets no `MIGRATION_DATABASE_URL` | `templates/migrate-job.yaml`, `templates/api-deployment.yaml`, `values.yaml`, README | `helm template`: the owner secret appears only in the Job; the default render is unchanged |
| 1B.5 | `check-migration-order.py`: destructive DDL (`DROP TABLE/COLUMN/VIEW/FUNCTION`, `RENAME`, `ALTER … TYPE`, `SET NOT NULL` on an existing column) needs a `-- contract: …` header | `scripts/check-migration-order.py`, `scripts/tests/` | positive and negative fixture migrations |
| 1B.6 | `crates/ingest-writer` skeleton: config, `health-http`, metrics, pool (writer role), `wait_for_schema`, the line catalogue; loops behind `INGEST_WRITER_LOOPS` (default false): schedule-match, reconciliation, backlog-match, CORPUS crosswalk `rebuild_if_stale` plus freshness gauge every 600 s. Each loop under `pg_try_advisory_lock(<key>)` held for the session; while it is held elsewhere the loop skips its tick and logs at debug | `crates/ingest-writer/`, `docker/ingest-writer.Dockerfile`, `Cargo.toml` | DB: two runners, only one sweeps per tick; loops tick on `sweep_interval` |
| 1B.7 | api: `API_BACKGROUND_LOOPS` (default true). The api's four loops and the CORPUS startup task take the same advisory locks, so writer and api can overlap safely during cutover | `api/src/main.rs` | DB: api and writer loops never both run a sweep body at once |
| 1B.8 | `api-maintenance` bin: one pass of `prune_expired_sessions`, `prune_dead_links`, `prune_personal_data`; chart CronJob hourly, `concurrencyPolicy: Forbid`, `timeZone` set (the CI CronJob check), api role; `apiMaintenance.enabled` (default false). The api's `session_cleanup_sweep_loop` stays until the CronJob is on, behind `API_BACKGROUND_LOOPS` | `api/src/bin/maintenance.rs`, `templates/api-maintenance-cronjob.yaml`, `values.yaml` | DB: one pass prunes what the loop pruned; `helm template` |
| 1B.9 | Chart for the writer: Deployment (Recreate, 1 replica), its NetworkPolicy (egress postgres; redis added in phase 3), PodMonitor, `ingestWriter.*` values, the `writer` role (members of `app` until phase 5 narrows it), and `IngestWriterDown` | `templates/ingest-writer-deployment.yaml`, `templates/networkpolicy.yaml`, `templates/prometheusrule.yaml`, `values.yaml` | `helm template`; `helm lint`; alert unit tests (`scripts/alert-rules-tests`) |
| 1B.10 | `api.strategy.type` value (default `Recreate`); `RollingUpdate` (`maxSurge 1`, `maxUnavailable 0`) is allowed only when `migrate.job.enabled` and not `migrateOnStartup` (render check) | `templates/api-deployment.yaml`, `values.yaml` | render fails for RollingUpdate with startup migration |

Rollout:

1. Release N: Job on, `migrateOnStartup=false`. The hook runs, then the
   api starts with the gate.
2. Release N+1: `ingestWriter.enabled`, `loops.enabled=true`.
3. Release N+2: `API_BACKGROUND_LOOPS=false`, `apiMaintenance.enabled=true`.
4. Release N+3: `api.strategy.type=RollingUpdate`.

Verification in production (read-only):

- the Job's logs;
- `_sqlx_migrations`;
- the writer's sweep logs and metrics;
- the api's absence of sweep logs;
- the CronJob's runs;
- during an api deploy, the `5xx` count and the frontend's synthetic
  check: no gap.

Exit:

- the api pod has no `MIGRATION_DATABASE_URL` and runs no loops;
- the writer runs the train-domain loops, and the CronJob the user-data
  ones;
- one api deploy with zero failed requests;
- a deliberately failing migration on a test cluster leaves the old pods
  serving.

Rollback, per release, in reverse:

- `Recreate`;
- `API_BACKGROUND_LOOPS=true` with the CronJob off;
- writer loops off;
- `migrateOnStartup=true` with the Job off.

## Phase 2: direct writes

Entry: phase 1 done.

For each producer:

1. its role exists through `postgres-grants.sql`, still a member of `app`
   until the 2.x "narrow" task;
2. its NetworkPolicy egress to postgres has rendered.

### 2a. schedule-reference

| # | Task | Files | Tests |
|---|---|---|---|
| 2a.1 | `PublishSink` trait plus `SinkError` in schedule-reference; `HttpSink` wraps today's `post_batch*`/`post_json` calls, mapping 409 to `Busy`, 503 to `Timeout`, other 4xx to `Rejected` and the rest to `Transient` | `crates/schedule-reference/src/{sink.rs,main.rs}` | the existing wiremock tests, now through `HttpSink` (unchanged assertions) |
| 2a.2 | `DbSink`: the `ds_store::schedule` publish parts, products, crosswalks, fixed links, population and marker; maps `SchedulePublishBusy` to `Busy`, 57014 to `Timeout`, class 22/23 to `Rejected` | `crates/schedule-reference/src/sink/db.rs`, `Cargo.toml` (`ds-store`, sqlx) | DB-gated: the protocol tests parameterised over both sinks; a small CIF day through each sink gives identical tables |
| 2a.3 | Config `INGEST_SINK` (`http`\|`db`), `DATABASE_URL`, pool 3, the schema gate before the first poll; the dedup seed reads `last_completed_schedule_reference_publish` directly under `db` | `config.rs`, `main.rs` | config tests; `chart_env_wiring_tests` updated |
| 2a.4 | Chart: the `reference` container gets the `schedule_reference` role env when `scheduleFeed.reference.ingest.sink=db`; the schedulefeed netpol egress to postgres; the postgres netpol ingress from `schedulefeed` | `templates/schedulefeed-deployment.yaml`, `templates/networkpolicy.yaml`, `values.yaml` | `helm template` both ways; `check-schedulefeed-chart.py` extended |
| 2a.5 | Metrics: `db_writes_total{operation="publish_part"…}`; `store_schedule_publish_staged_mismatch_total`; alert expressions take `or` of the `api_` and `store_` names | `ds-store/src/schedule/publish.rs`, `templates/prometheusrule.yaml` | alert unit tests |
| 2a.6 | Narrow: the `schedule_reference` role leaves `app` and gets exactly the spec §6.4 grants; run the DB suites as that role | `files/db-grants.yaml`, regenerate SQL | `test-postgres-roles.py --mode per-service` |

Rollout:

1. Narrow the role (2a.6) **before** the flip, so the flip exercises the
   final grants.
2. `sink=db`.
3. Watch the next day's publishes (about 58 chunks):
   - `db_writes_total`;
   - `DistantSignalSchedulePublishStagedMismatch`;
   - `DistantSignalScheduleReferencePublishStale`;
   - the api's `/private/schedule-*` at 0.

Verification (read-only): `SELECT service_date, count(*) FROM
schedule_calling_points_full GROUP BY 1` against the previous day's
publish; `pg_stat_activity` shows `distant_signal_schedule_reference`
during a publish; `*_publish_keys` analyzed (`last_analyze`).

Exit: 7 days of publishes on `db` with no stale or mismatch alert.

Rollback: `sink=http`. The api routes still exist.

### 2b. poller-stations

| # | Task | Files | Tests |
|---|---|---|---|
| 2b.1 | Sink trait (`HttpSink`, `DbSink` calling `ds_store::reference::upsert_stations`); the cursor from `freshness::last_stations_fetch` under `db` | `crates/poller-stations/src/{sink.rs,main.rs,config.rs}` | both sinks: the same rows for a fixture feed; a test that the 38 MB fixture path holds memory near the parsed vector size (the existing allocator meter) |
| 2b.2 | The CORPUS crosswalk refresh no longer depends on the POST: confirm the writer's 10-minute `rebuild_if_stale` loop is on (1B.6); api's `post_stations` keeps its call until phase 5 | – | DB: a new station gets crosswalk fills on the next loop tick |
| 2b.3 | Chart: role env, netpol egress and ingress; narrow the role (SIU `stations`, SIU `ingest_freshness`) | `templates/poller-deployments.yaml`, `templates/networkpolicy.yaml`, `files/db-grants.yaml` | `helm template`; per-role suite |

Exit: 7 daily refreshes on `db`. `md5(string_agg(crs || name ||
coalesce(accessibility::text,'') ORDER BY crs))` matches before and after
the first `db` refresh on a day with no upstream change.

Rollback: `sink=http`.

### 2c. poller-incidents, then the write-amplification fix

| # | Task | Files | Tests |
|---|---|---|---|
| 2c.1 | Split `ds_store::incidents` into `apply_snapshot` (returns `text_changed_ids`) and `infer_removals`; the api's handler calls them in today's order and publishes in between | `ds-store/src/incidents/*.rs`, `api/src/routes/ingest.rs` | moved order test; inference tests unchanged |
| 2c.2 | poller-incidents: sink trait; `DbSink` runs apply, then the XADD (`common::redis_conn`, the `poller-incidents` Redis user, best effort, `INCIDENT_TEXT_CHANGED_MAXLEN`), then inference; `LINES_DIR` plus the line catalogue in the image; the cursor from `last_incidents_fetch` | `crates/poller-incidents/src/{sink.rs,main.rs,config.rs}`, `docker/poller-incidents.Dockerfile` | both sinks give the same `incidents`, `incident_history` and `affected_lines` and the same inference outcome for a fixture sequence; the XADD happens after commit and before inference; a Redis-down run still ingests |
| 2c.3 | Chart: role env, Redis user env, netpol egress (postgres, redis), postgres and redis ingress from `poller-incidents`; narrow the role | templates, `db-grants.yaml` | `helm template`; per-role suite |
| 2c.4 | Readers of `incidents.fetched_at` switch to `incidents::FETCHED_AT_SQL` (`GREATEST(…)`, spec §9.4). First `grep -rn 'fetched_at' crates/*/src` over incident queries and list them in the commit message | `ds-store/src/incidents/mod.rs`, `api/src/routes/incidents.rs`, `api/src/data/queries.rs` (`search_incidents`, `incident_by_id`), aggregator queries if any | DB: identical output while the old per-row bump still runs |
| 2c.5 | Expand migration: `incident_feed_state.last_snapshot_at`, `previous_snapshot_at` (nullable) | `crates/api/migrations/<assigned>_incident_feed_snapshot_times.sql` | `migration_checksums`, `check-migration-order.py` |
| 2c.6 | The fix, behind `INCIDENTS_ROW_HEARTBEAT=true\|false` (default true, today's behaviour): when false, drop the per-row bump, reset only non-zero counters, update the feed-state times per snapshot, and stamp `fetched_at = previous_snapshot_at` on the 0→1 miss | `ds-store/src/incidents/{mod,removal}.rs`, poller and api config | DB: an identical repeated snapshot makes **zero** `incidents` updates (`pg_stat_xact_user_tables.n_tup_upd`); a display-time equivalence test over a 6-snapshot sequence with a disappearance and a reappearance; existing removal tests pass in both modes |

Rollout:

1. 2c.1–2c.3, then `sink=db`, then 7 days.
2. 2c.4 (readers) released.
3. 2c.5 migration.
4. `INCIDENTS_ROW_HEARTBEAT=false`.

Verification: `n_tup_upd` on `incidents` per day drops from about 600k to
under 1k (read-only `pg_stat_user_tables` deltas); `fetchedAt` on the
incidents page tracks the poll time.

Exit: 7 days on each step; `DistantSignalIncidentRemovalStalled` silent;
removal counts in line with the 2026-10-06 baseline.

Rollback: heartbeat `true` (instant), then `sink=http`.

### 2d. schedule-ingest: CORPUS and feed markers

| # | Task | Files | Tests |
|---|---|---|---|
| 2d.1 | Sink trait for `/private/corpus-locations` and `/private/schedule-feed-ingests`; `DbSink` calls `replace_corpus_locations_with_provenance` (with `corpus_load_problem`) and `insert_schedule_feed_ingest` (with `schedule_feed_ingest_problem`); `refresh_last_delivery_metric` and `log_after_load` move to the writer's crosswalk tick | `crates/schedule-ingest/src/{sink.rs,…}` | both sinks; validation errors identical |
| 2d.2 | Chart: the `ingest` container gets the `schedule_ingest` role; narrow the role | `templates/schedulefeed-deployment.yaml`, `db-grants.yaml` | `helm template`; per-role suite |

Exit: one monthly CORPUS delivery and 7 days of feed markers on `db`
(`DistantSignalCorpusStale` silent, `corpus_deliveries` row present).

Rollback: `sink=http`.

## Phase 3: streams

Entry:

- the stream users exist (0c, step 1);
- the writer is running (1B);
- Redis memory has at least 300 MB headroom
  (`redis_memory_used_bytes`).

### 3a. The stream runtime, then station samples and full coverage

**Status (2026-10-07): the runtime library is built, with no callers**
(`crates/ingest-stream`, spec §7.7, [`docs/ingest-stream-runtime.md`](../../ingest-stream-runtime.md)).
3a.1 and 3a.2 are done; 3a.3's Redis half (groups, PEL first, XAUTOCLAIM,
DELCONSUMER, dead letters, retries, graceful shutdown, metrics) is done in
`ingest_stream::consumer`, and the writer keeps its DB half. Decisions D5–D8
and implementation choices I1–I3 (spec §16) apply. Differences from the
table below:

- The runtime is the crate `crates/ingest-stream` (I1), not
  `common::ingest_stream`; producers depend on it with no sqlx.
- Metric names are `ingest_stream_*` (I2; spec §14.1), not
  `ingest_writer_*`/`ingest_producer_*`. 3a.4's alerts use them.
- Dead-letter streams are capped at their source's `MAXLEN` (I3) and the
  budget is 512 MB (D5), both checked by `ingest_stream::budget`.
- Tests: unit and golden (`cargo test -p ingest-stream`), and Redis-gated
  (`--test redis_stream -- --ignored`, a step in CI's rust-db-test job).

How the rest of 3a uses it:

- **The writer (3a.3)**: one `StreamConsumer` per stream not `off`, with
  `ConsumerConfig::new(stream, pod_name, budget::decl(stream).dead_letter_maxlen())`,
  `.with_progress(..)` for `/livez`, and `run(&handler, shutdown)`. Its
  `Handler` is the schema registry: decode `entry.envelope.payload_as()`,
  run the `ingest_dedup` insert and the `ds-store` upsert in one
  transaction, and map errors: SQLSTATE class 08/40/53/57, pool timeouts
  → `Transient`; class 22/23 for the whole entry, an unknown schema
  *name* → `Poison`; per-row rejects → `Handled::PartiallyRejected`; an
  unknown schema *version* → `UnsupportedSchema`; a dedup hit →
  `Duplicate`; shadow mode → `Skipped`. Still the writer's: `ingest_dedup`
  and its pruning, `MINID` trims of the dead-letter streams, the modes,
  the observed-at guards.
- **Producers (3a.7, 3a.8, 3c)**: `Producer::spawn(client, ProducerConfig::new(stream,
  budget::decl(stream).maxlen(), ProducePolicy::LatestSnapshot))` with the
  client built by `redis_url_with_credentials` for its own ACL user (D6);
  `split_snapshot(…, 100, |rows| to_raw_value(&body(rows)))` then
  `submit(parts)`; readiness `stream_unavailable` from `is_available()`;
  `metrics::record_oversize` on `TooLarge`; `last_produced_at` for the
  `CursorSource::Stream` cursor; `shutdown(task, grace)` on exit.

| # | Task | Files | Tests |
|---|---|---|---|
| 3a.1 | **Done 2026-10-07** (as `crates/ingest-stream`, I1). The envelope (spec §7.2), schema ids, gzip above 8 KiB, the 512 KiB `split_snapshot`, `xadd_entry` (`XADD … MAXLEN ~ N *`), and `last_produced_at` (`XREVRANGE COUNT 1`) | `crates/ingest-stream/src/{envelope,producer}.rs`, `tests/golden.rs` + `tests/fixtures/` | unit: round trip; threshold; split; unknown `v` refused (pending) and bad fields refused (poison); oversize and gzip-bomb caps; keys stable across retries; golden JSON and wire fixtures |
| 3a.2 | **Done 2026-10-07.** `Producer`: latest-only pending snapshot or a bounded event buffer with backpressure, `common::backoff` (1 s → 60 s), `ingest_stream_produce_*` metrics, `is_available()` for readiness `stream_unavailable` | `crates/ingest-stream/src/producer.rs` | Redis-gated: latest-only vs event while Redis is down (dead port, then a forwarder), superseded counter, shutdown; under the `poller-ldbws` ACL user |
| 3a.3 | Writer stream runtime. **Redis half done 2026-10-07** in `ingest_stream::consumer`: groups at `0 MKSTREAM`; PEL-first retry; `XAUTOCLAIM` every 60 s for entries idle over 5 min; `DELCONSUMER` hygiene; the dead-letter rules (spec §7.3); graceful shutdown; the stream metrics of spec §14.1. **Writer half, to do:** `ingest_dedup` (expand migration: the table plus a `applied_at` index in its own no-transaction file); hourly pruning; `MINID` trims of the dead-letter streams; the `observed_at` guard helpers (D13, spec §7.8): the observed time is the row's own time where it has one, else the envelope's `produced_at`; it is clamped to the writer's `now() + 2 min` with `ingest_stream_observed_at_clamped_total{stream,schema}` counting each clamp; and the guard SQL fragment is `EXCLUDED.t >= t.t OR t.t > now() + interval '2 min'`; modes `off`/`shadow`/`apply`; the handler registry; the `DistantSignalIngestClockSkew` alert goes with 3a.4 | `crates/ingest-stream/src/consumer.rs` (done); `crates/ingest-writer/src/{stream.rs,dedup.rs,handlers.rs}`, migrations (assigned range) | Done (Redis-gated, `crates/ingest-stream/tests/redis_stream.rs`): in-order apply; a crash's PEL reclaimed and applied first; poison, undecodable and oversize to the dead-letter stream; transient retried in order, never acked; an unknown schema or envelope version stays pending; `MAXLEN` trimming and a trimmed pending entry; under the `ingest-writer` ACL user. To do (DB-gated): applied once with dedup; a data error with the rest committed; shadow writes nothing; unit: a time 5 min ahead is clamped and counted, one 1 min ahead is not; DB: a row stamped 1 h in the future is overwritten by the next snapshot, and an older snapshot still changes nothing |
| 3a.4 | Alerts of spec §14.2 (stream backlog, stalled, dead letters, dead-letter expiring, unsupported schema, XADD failing, memory high, clock skew) and the dead-letter runbook (re-injection keeps the original `produced_at`, spec §7.8) | `templates/prometheusrule.yaml`, `docs/ingest-streams-deadletter.md`, `values.yaml` (`metrics.prometheusRule.ingest*`) | `scripts/alert-rules-tests` cases for each |
| 3a.5 | Expand migrations for the ordering guard columns where missing (`full_coverage_line_stats.source_updated_at`, `line_status.source_updated_at`; nullable). The handlers fill them with `source_updated_at := produced_at` (D13): the `/1` bodies carry no time. A row written before the column has `NULL`, so the guard treats `NULL` as older (`t.source_updated_at IS NULL OR …`) | migrations | `migration_checksums`, order check |
| 3a.6 | Handlers `station-samples/1`, `full-coverage-stats/1`, `full-coverage-window-stats/1` (with `full_coverage_window::validate`), `station-full-coverage-samples/1`, each with the `observed_at` guard (3a.3's helpers: `polled_at`, `resolved_at`, `computed_at`, or `source_updated_at` for `full_coverage_line_stats`). Freshness becomes "data as of" (D13): `record_ingest(conn, source, observed_at)` stores `GREATEST(ingest_freshness.fetched_at, EXCLUDED.fetched_at)`; the writer passes `produced_at`, and the api and the phase 2 direct writers pass their fetch time (`now()`), so their behaviour is unchanged | `crates/ingest-writer/src/handlers/*.rs`, `ds-store/src/{samples,freshness}.rs` and its callers | DB: an older snapshot after a newer one changes nothing; same rows as the HTTP route for a fixture; a snapshot applied late leaves `ingest_freshness` at its `produced_at`, and an older one after it does not move it back |
| 3a.7 | poller-ldbws: `INGEST_SINK=http\|http+shadow\|stream`; chunks of 100 stations; the cursor from `last_produced_at` under `stream`. Chart: Redis user env, netpol egress redis, redis ingress | `crates/poller-ldbws/src/…`, templates | sink tests; `helm template` |
| 3a.8 | full-coverage-consumer: the same for its three outputs on `ds:ingest:full-coverage` | `crates/full-coverage-consumer/src/…`, templates | sink tests |
| 3a.9 | (Q6, own switch `INGEST_WRITER_CHANGED_ROWS_ONLY`) the snapshot handlers skip timestamp-only bumps for `full_coverage_line_window_stats` and `station_full_coverage_samples` and record per-feed observed times in `ingest_freshness` (D13, spec §7.8). **Readers first**, as in 2c.4: readers of `full_coverage_line_window_stats.computed_at` (the `StaleRow` check's loader) and `station_full_coverage_samples.resolved_at` (freshness reads, station stats) switch to a shared `GREATEST(row time, feed observed_at)` fragment; list them by grep in the commit message. The guard compares against the same derivation. First check that every snapshot carries every live key; a table where it does not keeps its per-row bump. **`station_samples` is excluded** and keeps its per-row `polled_at` update (a cycle visits about 255 of 560 stations, so a feed time would mark unvisited ones fresh) | `ds-store/src/samples.rs`, `ds-store/src/samples/full_coverage_window.rs`, the readers in `api` and `aggregator` | DB: an identical snapshot makes zero updates to the two tables; readers return identical output while the per-row bump still runs; with the switch on, an unchanged window is not `StaleRow` after 180 s; an older snapshot after a skipped newer one changes nothing; `station_samples` rows still get their `polled_at` |

Rollout per producer:

1. writer `streams.station-samples: shadow`;
2. producer `http+shadow`;
3. 3 days;
4. flip both (`apply` plus `stream`);
5. 7 days.

Then the same for `full-coverage`.

Verification: writer `ingest_stream_consumed_total{outcome="skipped"}` (shadow) equals
the api route's request count; `ingest_stream_bytes` within budget;
`DistantSignalLdbwsStationStale` and
`DistantSignalFullCoverageWindowStatsStalled` silent.

Exit: both streams on `apply` for 7 days; the api routes at 0.

Rollback: producer `http`.

### 3b. The TRUST backlog (direct) and train events (stream)

**Status (2026-10-08): 3b.1–3b.4 built, off by default**
(`trustBacklogConsumer.ingest.sink: http`, `trustConsumer.ingest.sink:
http`). Differences from the table:

- 3b.1: the api handler's whole write (the backlog insert, the shared
  movements for the accepted rows, the PL-7 classification, the sorted
  `rejected`) moved into `ds_store::backlog::ingest_trust_event_backlog`;
  `post_trust_event_backlog` and `DbSink` both call it, with its
  `api_trust_event_backlog_*` metrics (names unchanged; under `db` the
  consumer pod emits them). Under `db` the rejected rows dead-letter as
  `rejected_by_db`, the failure counters are `db_write` and
  `db_write_reasons`, and a transient failure backs off 1 s → 60 s (the
  HTTP sink keeps 2 s → 60 s). The STANOX/CRS reload honours the api's
  `CORPUS_FALLBACK_ENABLED` (the chart passes `api.corpusFallback.enabled`),
  so `trust_backlog` also gets SELECT on `tiploc_crs` and
  `corpus_stanox_crs`.
- 3b.2/3b.4: both roles are `narrow` from creation (they were `planned`;
  neither ever was a member of `app`), connected with
  `postgresql.roles.perService.{trust_backlog,trust_consumer}.connect`.
  `trust_consumer` keeps SELECT only on `train_subscriptions` (decided
  2026-10-08; see 3b.3 for the outbox). trust-consumer's
  `API_CALL_OPERATIONS` gains `db_write`. Under `db` trust-backlog-consumer
  has no api egress (it calls nothing there); trust-consumer keeps it for
  its reads (tracked trains, STANOX/CRS), which stay on the api until
  phase 4. CI runs both crates' DB tests as the superuser, and as their
  narrow roles in the per-service step (fixtures through
  `DATABASE_URL_API`; nothing the narrow roles lack is needed).
- 3b.3: the dedup of a redelivered forward signal needed an expand
  migration: `notifier_forward_queue.dedup_key` (nullable,
  `20261009120000`) with a partial unique index (`20261009120100`, no
  transaction). The key is `<trains_id>:<dedup_key of the movement that
  raised it>` (the movement entry carries no stream id into the event);
  both sinks send it, and `insert_forward_signals` skips a known key. The
  DB sink runs `upsert_train_events_batch_in` and `insert_forward_signals_on`
  in one transaction.
- 3b.3, the subscription writes (decided 2026-10-08: no UPDATE on
  `train_subscriptions` for `trust_consumer`): `upsert_train_event_on`
  updates a subscription for a resolution (`resolved_train_id`: the
  status flip and the `trains_id` link), a cancellation (`status =
  'cancelled'`) and a reinstatement (`0005`). The DB sink
  (`ds_store::tracking::outbox::write_train_events_deferring`) writes
  those, and every later event of the same subscription while one of its
  rows is pending, to `train_event_outbox` (expand migration
  `20261009130000`; `trust_consumer` SI, writer SUD) in the batch's
  transaction, with the forward signal each raised; the rest go direct.
  The ingest-writer's `train_event_outbox` loop
  (`TRAIN_EVENT_OUTBOX` lock, `INGEST_WRITER_TRAIN_EVENT_OUTBOX_INTERVAL_SECS`,
  5 s) applies them in id order with `upsert_train_event_on`, queues
  their signals and deletes them; a data error leaves the row with
  `rejected_at` (logged, `store_train_event_outbox_total{outcome="rejected"}`)
  instead of the movement dead-letter stream. A watermark loop over
  `train_current_state` (option a) was rejected: the resolution decides
  which `trains` row the subscription's movements land on, so it cannot
  be derived after them. The chart refuses `trustConsumer.ingest.sink: db`
  without `ingestWriter.enabled` and `ingestWriter.loops.enabled`. Under
  `http` nothing changes: the api applies everything inline.

| # | Task | Files | Tests |
|---|---|---|---|
| 3b.1 | trust-backlog-consumer: a `BacklogSink` trait (`HttpSink` as today; `DbSink` calling `upsert_trust_event_backlog_batch`, then `ingest_shared_movements_batch` for the accepted rows, then `upsert_reasons`, keeping the exact `rejected` handling of `post_trust_event_backlog`); a transient failure means **pause** (backoff 1 s → 60 s) before the next read; STANOX/CRS from `list_stanox_crs` under `db`; pool 3 | `crates/trust-backlog-consumer/src/{sink.rs,main.rs,queries.rs,config.rs}` | DB: same rows as the HTTP route for a fixture batch; a data-error row is rejected and dead-lettered while the rest commit; a transient error leaves the batch un-ACKed and backs off; `deliver_batch` tests over both sinks |
| 3b.2 | Chart: role env, netpol egress postgres, postgres ingress; narrow the role (spec §6.4). `API_CALL_OPERATIONS` and the chart alert list gain `db_write` and `db_write_reasons` | templates, `db-grants.yaml`, `templates/prometheusrule.yaml` | the existing "chart alerts on every operation" test |
| 3b.3 | (D1) trust-consumer: a `TrainEventSink` (`HttpSink` as today; `DbSink` calling `upsert_train_events_batch` and `insert_forward_signals` in one transaction, rejected rows dead-lettered as today); **ACK `movement-events` only after the commit**; a transient failure backs off (1 s → 60 s) before the next read; pool 2 | `crates/trust-consumer/src/{sink.rs,main.rs,queries.rs,config.rs}` | DB: same rows as the HTTP routes for a fixture batch; a redelivered entry inserts one forward signal (dedup by the movement entry id); a DB failure means no ACK |
| 3b.4 | Chart: the `trust_consumer` role env, netpol egress postgres, postgres ingress; narrow the role | templates, `db-grants.yaml` | `helm template`; per-role suite |

Rollout:

1. backlog `sink=db`, 7 days (watch the movement lag, `db_writes_total`,
   and backlog rows per hour against the about 30k/h baseline);
2. then trust-consumer `sink=db` (D1), 7 days.

Exit: the api's `/private/trust-event-backlog`, `/train-reasons`,
`/train-events` and `/train-forward-signals` at 0 for 7 days.

Rollback: `sink=http` per producer.

### 3c. TfL, tocs, island of Ireland

| # | Task | Files | Tests |
|---|---|---|---|
| 3c.1 | Handlers `tfl-line-status/1` (with the `source_updated_at` guard), `tocs/1` (dedup), `ioi-*/1`. TfL (D13): `line_status.computed_at`, `source_updated_at` and `line_status_history.computed_at` come from `produced_at`, not `NOW()`; a line the guard refuses as older is skipped, not mistaken for the other-source refusal that `upsert_tfl_line_status` aborts on today, and writes no history row. Each handler calls `record_ingest(…, produced_at)` | `crates/ingest-writer/src/handlers/*.rs`, `ds-store/src/samples.rs` | DB per handler; TfL: a snapshot applied an hour late stamps both tables with its `produced_at`; an older snapshot after a newer one changes neither table; freshness reports the `produced_at` |
| 3c.2 | poller-tfl, poller-tocs and the three IoI pollers: `INGEST_SINK`; chart Redis users, netpol. The IoI pollers move to the `stream` sink like the others (D8) and stay disabled by default (their `enabled` values stay false); they keep no `http` path for phase 5 to delete | crates, templates | sink tests; `helm template` |
| 3c.3 | RLS on `line_status` (D10, spec §6.4): `ENABLE ROW LEVEL SECURITY`; the writer role's policy allows only TfL rows (`USING` and `WITH CHECK`); permissive `USING (true)` policies keep every other role's access unchanged | `crates/ds-store/migrations/<assigned>_line_status_rls.sql`, `db-grants.yaml` | DB, as the writer role: a non-TfL insert or update fails and a TfL one succeeds; as the aggregator and api roles: unchanged; `migration_checksums`, `check-migration-order.py` |
| 3c.4 | **Before TfL goes to `apply`** (D13, spec §7.8): the notifier skips line-status history rows whose `computed_at` is older than `LINE_HISTORY_MAX_AGE_SECS` (default 900; `0` disables; chart `notifier.lineHistoryMaxAgeSeconds`). It advances its cursor past them, keeps them as the "previous" status for the next row, and counts them in `notifier_line_history_skipped_total{reason="stale"}` | `crates/notifier/src/{queries.rs,main.rs,config.rs}`, the chart's notifier env and values | DB: a burst of history rows, all but the last older than 15 min, pushes only the last change and counts the rest; a fresh row after a stale one is compared with the stale one's statuses; `0` pushes all; the chart env wiring test |

Order: 3c.4 is deployed before the writer's `tfl` stream goes to `apply`.

Exit: TfL and tocs on `stream` for 7 days. The IoI pollers are on
`stream` but disabled (D8, Q9), so they have no soak; they are prod-tested
when they are enabled.

## Phase 4: internal reads

Entry: phase 1 done.

| # | Task | Files | Tests |
|---|---|---|---|
| 4.1 | Expand migration: the views `ingest_active_tracked_trains` (today's `list_active_tracked_trains` SELECT, no user columns), `ingest_sample_station_pins` (pin counts per line), `ingest_custom_line_stations` (custom line id plus station list, no owner); classify them in `db-grants.yaml` | migrations, `db-grants.yaml` | DB: the views' column lists contain no `user_id`/`owner`; results equal the api's functions |
| 4.2 | `ds_store::reads`: `list_population_versions`, the view readers, `select_sample_stations` (pure) | `ds-store/src/reads.rs` | DB as each reader role |
| 4.3 | full-coverage-consumer `POPULATION_SOURCE`, `STANOX_CRS_SOURCE`: `DbSource` (spec §11.1), keeping the first-load wait and backoff | `crates/full-coverage-consumer/src/{population_reload.rs,queries.rs,config.rs}` | DB: a changed `updated_at` refetches only that pair; an unchanged one fetches nothing; abort-after-3 still applies |
| 4.4 | trust-consumer `TRACKED_TRAINS_SOURCE`, `STANOX_CRS_SOURCE` | `crates/trust-consumer/src/…` | DB |
| 4.5 | poller-ldbws `SAMPLE_STATIONS_SOURCE` | `crates/poller-ldbws/src/…`, `docker/poller-ldbws.Dockerfile` (`lines/`) | the selection equals the api's for fixtures with pins and custom lines |
| 4.6 | `common::ingest` `CursorSource` (`Http`, `Stream`, `Db`) for every poller's startup cursor | `crates/common/src/ingest.rs`, the pollers | the existing `time_until_next_poll` tests across sources |
| 4.7 | Chart: the three `_ro` roles, env, netpol; narrow them | templates, `db-grants.yaml` | per-role suites |

Exit: the read routes and every last-fetched GET at 0 requests for 7 days;
full-coverage population checksums equal in both modes for a day.

Rollback: `source=http`.

## Phase 5: remove `/private` and lock down

Entry:

- every switch in spec §13.1 has been on its new value for 14 days;
- every `/private` route has had 0 requests for 7 days (Prometheus,
  read-only).

| # | Task | Files | Tests |
|---|---|---|---|
| 5.1 | `API_PRIVATE_ROUTES` (default true); false skips `.nest("/private", …)`. Release with false; 7 days | `api/src/main.rs`, `values.yaml` | route test: `/private/*` is 404 when off |
| 5.2 | Delete `routes/ingest.rs`, `routes/samples.rs`, `private_router`, the ingest entries of `internal_oauth_route_table` and their `ServiceArguments` groups, the `INTERNAL_OAUTH_GROUP_*` chart env (MCP kept), and `api.privateRoutes` | `api/src/{app.rs,routes/*,data/config.rs}`, `templates/api-deployment.yaml`, `values.yaml`, README | the adapted MCP group test; `helm template` |
| 5.3 | Delete the producers' `HttpSink`/`HttpSource`, OAuth token caches, `API_*_URL` config and token-URL egress; `INGEST_SINK` values collapse to the new path | the producer crates, templates | the remaining sink tests |
| 5.4 | Delete the api's Redis client (`AppState.redis`, `REDIS_*` env, the `api` Redis user), the shims in `api/src/data/*` (direct `ds_store` imports), and the `or` clauses for `api_*` metric names in alerts | api, templates | build; alert tests |
| 5.5 | Final grants: the api role narrowed (spec §6.4), `aggregator`/`enricher`/`notifier` narrowed from the 0b report (enricher: column-level `UPDATE` on the incident extraction columns), `distant_signal_app` dropped from every membership, then dropped | `db-grants.yaml`, regenerated SQL, `docs/postgres-app-role.md` (rewritten for per-service roles) | per-role suites; `observe-role-usage.py` after 7 days shows no denied verbs |
| 5.6 | NetworkPolicies of spec §15.1: the api ingress list without producers, postgres and redis lists as specified, producers' egress without api | `templates/networkpolicy.yaml` | netpol render tests for each component |
| 5.7 | Docs: README architecture section, `DESIGN.md`, the chart README, the runbooks; mark this spec `implemented` in the index | docs | `chart-values-doc.py check` |

Verification (read-only): `kubectl exec` into a producer is **not**
allowed for agents. Ranma runs the negative checks:

- psql as each role: `CREATE TABLE`, `TRUNCATE`, `DELETE FROM users`;
- from a poller, `redis-cli` with another stream's XADD;
- all must fail.

Exit: the spec §16 phase-5 exit list.

Rollback:

- before 5.2: `API_PRIVATE_ROUTES=true`;
- after 5.2: revert the commit and redeploy;
- for grants: the setup Job applies the previous `db-grants.yaml` (it
  re-grants idempotently).

## Effort summary

| Phase | Engineer-days |
|---|---|
| 0 | 5–6 |
| 1A | 7–9 |
| 1B | 5–6 |
| 2a / 2b / 2c (+fix) / 2d | 5–6 / 2 / 5–6 / 2 |
| 3a / 3b / 3c | 9–10 / 3–4 / 3–4 |
| 4 | 4–5 |
| 5 | 3–4 |
| **Total** | **53–64**, plus 3–7 days of soak per flip (about 10–14 weeks of calendar with one implementer) |

3a gained a day on 2026-10-07 for D13: 3a.9's reader derivations are a new
task the size of 2c.4. The other D13 additions (the clamp in 3a.3's guard
helpers, `produced_at` stamping in 3a.5, 3a.6 and 3c.1, and the notifier
skip, 3c.4) are small and fit their existing ranges.

## Off-by-default settings this plan adds

`postgresql.roles.perService`, `redis.acl.enabled` (`stage`, `defaultUser`),
`migrate.job.enabled`, `api.migrateOnStartup` (default true),
`API_BACKGROUND_LOOPS` (default true), `ingestWriter.enabled`,
`ingestWriter.loops.enabled`, `ingestWriter.streams.*` (default `off`),
`apiMaintenance.enabled`, `api.strategy.type` (default `Recreate`), each
producer's `ingest.sink` (default `http`), each reader's
`internalReads.source` (default `http`), `INCIDENTS_ROW_HEARTBEAT`
(default true), `INGEST_WRITER_CHANGED_ROWS_ONLY` (default false),
`api.privateRoutes.enabled` (default true), and the notifier's
`LINE_HISTORY_MAX_AGE_SECS` / `notifier.lineHistoryMaxAgeSeconds` (3c.4;
default 900, **on**, because it must be in place before TfL `apply`; `0`
restores today's behaviour. Until then it only skips changes a notifier
outage over 15 minutes left behind).
