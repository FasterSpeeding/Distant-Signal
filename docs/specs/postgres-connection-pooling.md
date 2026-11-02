# Postgres connection pooling (PgBouncer): investigation and design

Status: proposal, not implemented. Task DS-255. Written 2026-11-02.

This spec covers two questions:

1. Why the schedule-reference CronJob pods are slow to get their first
   database connection. The slowness started when schedule-reference
   moved to a CronJob on 2 Nov.
2. Whether Distant Signal should put a connection pooler such as
   PgBouncer between every database client and Postgres.

**Short answer.** The slow first acquire is not caused by the database. Every
new pod's first connections are **refused for about 2 to 5 seconds** while
the cluster's NetworkPolicy rules catch up with the pod's new IP. sqlx
retries a refused connect on its backoff schedule until the 5 s
`acquire_timeout`. A pooler would not fix this, because a new pod's first
packet to the pooler hits the same lag. At today's scale a pooler also has
nothing to fix: production has 35 client backends against 197 usable
connection slots. The recommendation is to **not deploy a pooler now**.
Instead, absorb the start-up lag cheaply (§5) and keep the design in §4
ready for when the triggers in §5.3 are met.

## 1. Diagnosis: the slow first acquire

### 1.1 What was observed (read-only, 2026-11-02 ~02:10 UTC)

These are the schedule-reference CronJob runs. The schema gate's own
queries took 5 to 9 ms in both runs.

| Run (UTC) | Start → first connection | Outcome |
|---|---|---|
| 01:30:03 | 5.0 s, then `PoolTimedOut` | `common::startup` retried after 625 ms, and the pool connected at once (1 attempt more, 0.6 s) |
| 02:00:03 | 3.376 s | sqlx `acquired connection, but time to acquire exceeded slow threshold` |

The same warning appears **once, at start-up, in every pod that connects to
Postgres**, whether it is a Deployment or a CronJob:

| Pod (start) | `aquired_after_secs` |
|---|---|
| aggregator (22:59) | 2.359 |
| notifier (22:59) | 3.396 |
| enricher (22:59) | 4.441 |
| api (00:36) | 3.337 |
| api-maintenance CronJob (01:17) | 3.355 |
| schedule-reference CronJob (02:00) | 3.376 |
| trust-backlog-consumer, poller-incidents | one each, at start-up |

After start-up the long-running pods log no further slow acquires and no
timeouts, even though their pools reconnect all the time.
`pg_stat_activity.backend_start` shows the aggregator, notifier, writer
and trust-backlog pools opening new backends throughout the night (sqlx's
`idle_timeout` is 10 min and `max_lifetime` is 30 min).

**Redis shows the same lag.** At 01:16:48 the new poller-tfl and
poller-transit-schedule pods got `Connection refused (os error 111)` from
**Redis** for about 3 s before it "recovered". At 01:16:30 the
roles-setup Job's `pg_isready` got "no response" for about 4 s before
Postgres was "accepting connections".

### 1.2 Why it is the network, not Postgres

* **The delays line up with sqlx's retry schedule.** sqlx 0.8.6
  (`sqlx-core/src/pool/inner.rs`) retries a connect that fails with
  `ConnectionRefused`. It sleeps 10 ms and doubles each time, capped at
  `acquire_timeout / 5` = 1 s. It stops at the `acquire_timeout` deadline
  with `PoolTimedOut`. So the attempts fall at about 0, 0.01, 0.03, 0.07,
  0.15, 0.31, 0.63, 1.27, **2.27, 3.27, 4.27**, 5.27 s. The observed
  acquires (2.36, 3.34 to 3.40, 4.44 s) are those attempts plus a few tens
  of ms for the connection itself. This means:
  * the server **refused** the TCP connection;
  * every attempt from ~2.3 s onward succeeded straight away;
  * at 01:30 the refusals outlasted the 4.27 s attempt, and the next
    attempt would have been after the 5 s deadline, hence `PoolTimedOut`.
* **A refused connection never reaches a backend.** Postgres was up the
  whole time (the pod had been Running for 3 h 28 m), and it never refuses
  a TCP connect. ECONNREFUSED to both Postgres and Redis, from new pods
  only, fits the k3s network-policy controller (kube-router). The
  namespace runs `default-deny-ingress` and `default-deny-egress`, and
  kube-router **REJECTs** traffic until a new pod's IP has been added to
  the policies' ipsets.
* **Postgres logged nothing at those times.** `log_connections` is off.
  With the default `log_min_messages`, a FATAL such as "too many
  connections for role" would still be logged, and the 4 h log has
  exactly one FATAL, unrelated (`role "postgres" does not exist` at
  01:01).
* The other candidates are ruled out:

| Candidate | Finding |
|---|---|
| SCRAM / TLS setup cost | `ssl = off` and `password_encryption = scram-sha-256` (4096 iterations, a few ms). Later connections from the same pods are fast, and so is schedule-reference's second, dedicated run-lock connection. |
| Schema gate / privilege queries (`ds_store::schema`) | Run **after** the pool connects. Logged as `waited_secs: 0`, 5 to 9 ms. |
| `max_connections` headroom | 35 client backends of 197 usable (200 − 3 superuser-reserved). The role `CONNECTION LIMIT`s add up to 128. `53300 too_many_connections` is a "transient" error that sqlx retries silently, but Postgres would have logged a FATAL. It logged none. |
| Role `CONNECTION LIMIT` (schedule_reference = 7) | 4 needed (pool of 3 plus the run lock), and no other holder. No FATAL. |
| Backend fork under memory pressure | The node is at 91 % memory and 78 % CPU. But a slow fork would slow every new backend, including the frequent pool reconnects in long-running pods, and those are fast. Worth keeping in mind (DS-251, earlyoom), but not this symptom. |
| `authentication_timeout`, etc. | Default 60 s. Not involved: authentication never started. |
| DNS | A DNS failure gives a resolve error or a 5 s resolver timeout, not ECONNREFUSED. |

### 1.3 Why it showed up with the CronJob switch

The lag has existed for every new pod. A long-running Deployment pays it
once per rollout, which is a single WARN nobody looked at. As a CronJob,
schedule-reference makes a new pod every 30 minutes: 48 times a day. The
5 s `acquire_timeout` (`common::pg`) leaves only about 0.7 s of margin
over a typical 4.3 s lag, so some runs will hit `PoolTimedOut`.
`common::startup::retry_until_ready` already recovers, so no run has
failed. The hourly api-maintenance CronJob has the same exposure and shows
the same 3.36 s warning.

### 1.4 What would confirm it further (Ranma-Config / node access)

These checks are not possible with this session's read-only kubectl:

* the k3s server log around a new pod's start, for the network-policy
  controller's sync timing;
* `iptables -L KUBE-NWPLCY-*` / `ipset list` on the node, watched while a
  CronJob pod starts;
* whether the controller's full `iptables-restore` slows down with the
  number of policies. The namespace has 48 NetworkPolicies. The node's
  CPU load could add to it.

## 2. Inventory of database clients

Pool sizes are the production values (env `DATABASE_MAX_CONNECTIONS`, or
the crate default from `PoolSettings::from_env`). The role limits are
`pg_roles.rolconnlimit`. "Now" is the client backends in
`pg_stat_activity` at 02:10 UTC.

| Client | Kind | Role | Pool (max) | Extra connections | Role limit | Now |
|---|---|---|---|---|---|---|
| api | Deployment (1 replica, +1 surge) | `distant_signal_api` | 16 | planner-snapshot build lock (pooled connection) | 36 | 9 |
| aggregator | Deployment | `distant_signal_aggregator` | 10 (crate default) | +2 archive pool when `archive.enabled` | 13 | 3 |
| notifier | Deployment | `distant_signal_notifier` | 5 (crate default) | | 6 | 5 |
| enricher | Deployment | `distant_signal_enricher` | 5 (crate default) | | 6 | 1 |
| ingest-writer | Deployment | `distant_signal_writer` | 6 | +1 dedicated loop-lock session (`-locks`) | 12 | 7 |
| trust-backlog-consumer | Deployment | `distant_signal_trust_backlog` | 3 | | 7 | 3 |
| trust-consumer | Deployment | `distant_signal_trust_consumer` | 2 | | 5 | 1 |
| full-coverage-consumer | Deployment | `distant_signal_full_coverage_ro` | 2 | | 5 | 1 |
| poller-incidents | Deployment | `distant_signal_incidents` | 2 | | 5 | 2 |
| poller-ldbws | Deployment | `distant_signal_ldbws_ro` | 1 | | 3 | 1 |
| schedulefeed (ingest) | Deployment | `distant_signal_schedule_ingest` | 1 | | 3 | 0 |
| schedule-reference | CronJob `*/30` | `distant_signal_schedule_reference` | 3 | +1 dedicated run-lock session | 7 | 0 (between runs) |
| api-maintenance | CronJob `17 * * * *` | `distant_signal_api` | 2 | | (shares api's 36) | 0 |
| poller-stations | CronJob daily | `distant_signal_stations` | 1 | | 3 | 0 |
| migrate | Helm hook Job | `distant_signal_owner` | 1 session (session `lock_timeout`/`statement_timeout`, session advisory lock) | | 3 | 0 |
| postgres-roles-setup | Helm hook Job | superuser `distant_signal` (psql) | 1 | | none | 0 |
| postgres-exporter | Deployment (Ranma-Config) | `distant_signal_exporter` | 1 | | 3 | 1 |
| pg_dump backup | CronJob daily (Ranma-Config) | `distant_signal_dump` | 1, long transaction + COPY | | 2 | 0 |
| pgBackRest | in the Postgres pod, local socket | `distant_signal_backup` | 1 to 2 during backup/check | | 4 | 0 |
| admin `psql` | `kubectl exec` | superuser | | | none | 1 |

The configured maximum is about **63 service connections** (79 during an
api rolling update, with 2 × 16), plus about 8 for Jobs, the exporter and
backups. That is **~71 to 87 of 197** usable. The chart enforces this
budget at render time (INF-7 in `api-deployment.yaml`, and the per-role
check in `scripts/gen-db-grants.py`).

## 3. Compatibility audit: DS code behind a pooler

The pool modes referred to below are PgBouncer's. In **session** mode a
client connection pins one server connection for its whole life. In
**transaction** mode a server connection is lent out for one transaction
(or one autocommit statement) at a time.

| Feature | Where | Session mode | Transaction mode |
|---|---|---|---|
| Named prepared statements (sqlx caches `sqlx_s_N` per connection, 100 by default) | every sqlx pool | fine | Needs PgBouncer ≥ 1.21 with `max_prepared_statements` > 0 (for example 200). Older versions fail with "prepared statement does not exist". The alternative, `statement_cache_capacity(0)`, costs a parse on every query. |
| Startup-packet `options` (`-c statement_timeout=… idle_in_transaction_session_timeout=… client_connection_check_interval=… tcp_keepalives_*`) | `common::pg::PoolSettings::connect_options`, every service | PgBouncer only forwards startup parameters it knows. Older releases reject `options` ("unsupported startup parameter"). Listing it in `ignore_startup_parameters` drops it **silently**, which removes the 60 s / 30 s timeouts. Recent releases accept `-c` settings in `options` and track them like `track_extra_parameters`; check the exact release when implementing. | same, and worse: a tracked parameter is re-sent with `SET` whenever the server connection changes hands |
| **Recommended for the timeouts:** `ALTER ROLE <service role> SET statement_timeout / idle_in_transaction_session_timeout` in `postgres-roles.sql` | | Server-side, so it applies on any path. The startup `options` then become a belt-and-braces duplicate for direct connections. | same |
| `SET LOCAL statement_timeout` / `idle_in_transaction_session_timeout` | `common::pg::set_local_*` (schedule publish, retention prunes, archive) | fine | fine: scoped to the transaction |
| Session advisory lock, **dedicated connection** | schedule-reference run lock (`ds_store::schedule::reference_signals`, `pg_try_advisory_lock`); ingest-writer loop locks (`ds_store::loops::runner`, `LockSource::Dedicated`) | fine | **Broken.** The lock lands on whichever server connection ran it, stays there after the client lets go, and the unlock may run on another one. Must bypass the pooler or use a session-mode pool. |
| Session advisory lock on a **pooled** sqlx connection | api `data/planner_snapshot.rs` (`BUILD_LOCK_KEY`), ingest-writer `LockSource::Pooled` | fine | **Broken** for the same reason. The fix is `pg_try_advisory_xact_lock` inside one transaction around the build, if a transaction that long is acceptable, or a session-mode pool for the api. |
| Transaction advisory locks (`pg_advisory_xact_lock` / `pg_try_advisory_xact_lock`) | `common::pg::lock_user_train_subscription`, `ds_store::corpus`, `schedule::publish`, `transit::write`, api `delegated.rs` | fine | fine |
| LISTEN / NOTIFY | none (no `PgListener`, `LISTEN` or `pg_notify` in `crates/`) | fine | n/a |
| Session `SET` | only `ds_store::migrate` (`SET lock_timeout`, `SET statement_timeout`) and test-only `after_connect` hooks (`SET lock_timeout`, `SET ROLE`, `SET search_path`) | fine | migrations must bypass the pooler, as they should anyway (DDL, session lock, `CREATE INDEX CONCURRENTLY`) |
| COPY | none in services. `pg_dump` only. | fine | n/a: pg_dump bypasses |
| Long transactions | schedule publish chunks (up to minutes, `SET LOCAL statement_timeout`); aggregator archive (waits on object storage between statements); pg_dump | fine | Works, but holds a server connection for the whole transaction, so nothing is gained there. Size the per-role `pool_size` for it. |
| Dead-client detection (`client_connection_check_interval`, keepalives) | every pool | Postgres now sees PgBouncer as the client. When a pod dies mid-query, PgBouncer closes that server connection, so detection still works, one hop removed. Test it. | same |
| `application_name` in `pg_stat_activity` | every pool | PgBouncer forwards it (`application_name_add_host` optional) | Shows the last client to use the server connection. Per-service identification is still possible through the per-role pools. |

## 4. Design (if and when a pooler is adopted)

### 4.1 Pool mode per client

| Client | Path | Mode |
|---|---|---|
| api, aggregator, notifier, enricher, ingest-writer (main pool), trust/full-coverage consumers, pollers, schedulefeed, api-maintenance, schedule-reference (main pool) | PgBouncer | **transaction**, after the §3 fixes (timeouts moved to `ALTER ROLE`, api build lock changed). Until then, **session**. |
| ingest-writer loop-lock session, schedule-reference run lock | direct to Postgres (or a session-mode PgBouncer database alias) | session |
| migrate, postgres-roles-setup | direct | n/a |
| pgBackRest (local socket), postgres-exporter, pg_dump, admin psql | direct | n/a |

Session mode buys almost nothing for sqlx clients. Each sqlx connection
already lives for minutes and would pin one server connection anyway.
The only gain would be a single place to cap connections per role, and
the role `CONNECTION LIMIT`s already do that. **Transaction mode is the
only mode worth deploying for.**

### 4.2 Per-role pools

PgBouncer keys server pools by (database, user), so each narrowed role
gets its own pool with no extra configuration. Set:

* `default_pool_size` low (for example 3);
* `[users] <role> = pool_size=N max_user_connections=N`, with N taken from
  `db-grants.yaml`'s `connection_limit` (generated by
  `scripts/gen-db-grants.py`, so one source of truth);
* `max_db_connections` at or below `max_connections − reserve`, leaving
  room for the direct clients in §4.1;
* `max_client_conn` around 500. Clients can then keep today's pool sizes
  while server connections shrink to what is actually busy, which is
  about 10 to 20 at today's load.

### 4.3 Authentication

**Recommended: `auth_query` with a dedicated lookup role.**

* New role `distant_signal_pgbouncer` (LOGIN, `CONNECTION LIMIT 2`).
* A `SECURITY DEFINER` function `pgbouncer.get_auth(username)`, owned by
  a superuser, that returns `rolname, rolpassword` from `pg_authid`. It
  is restricted to the service roles: no superuser and no `distant_signal`.
  It is granted only to the lookup role.
* With SCRAM secrets from `auth_query`, PgBouncer authenticates the
  client and reuses the secret to log in to the server. No plaintext
  passwords are needed in PgBouncer.
* Password rotation keeps working through `postgres-roles.sql`.

The alternative is a `userlist.txt` built from the per-role Secrets. That
needs a reloader or a pod restart on every rotation, and it puts every
role's secret into one more Secret. Not recommended.

### 4.4 Placement

**Recommended: its own Deployment**, `distant-signal-pgbouncer`:

* 2 replicas, with a PodDisruptionBudget and anti-affinity once there is a
  second node. With one node, use 1 replica and a fast restart.
* A Service on 6432.
* A ConfigMap for `pgbouncer.ini`, with `auth_query` / `auth_user`.

A sidecar per pod gives no cross-pod multiplexing, which is the point of
a pooler. It also doubles the sidecars to maintain and does not help
CronJob pods.

For the image, build our own (Forgejo `lucy/…`, like
`postgres-pgbackrest`), pinned by digest. The Bitnami images are no
longer maintained for free. Use PgBouncer ≥ 1.21, and preferably the
current 1.2x release for `options` support.

### 4.5 NetworkPolicy

DS chart:

* new `distant-signal-pgbouncer` policy:
  * ingress on 6432 from the opted-in components (the same selector list
    as `distant-signal-postgres`'s ingress);
  * egress to the postgres pod on 5432, plus DNS;
* add the pgbouncer component to `distant-signal-postgres`'s ingress;
* keep the direct entries for migrate, roles-setup, the lock sessions,
  exporter, dump and backup.

Ranma-Config: allow Prometheus to scrape the exporter port (see §4.6).
`allow-egress-same-namespace` already lets clients reach the pooler.

### 4.6 Metrics, dashboards and alerts

Add `prometheus-community/pgbouncer_exporter` as a sidecar in the
PgBouncer pod. It reads `SHOW POOLS/STATS` through the admin console,
logged in as a `stats_users` entry. Add a PodMonitor.

Alerts (chart `prometheusrule.yaml`):

* `pgbouncer_pools_client_waiting_connections > 0` for 2 m (a pool is too
  small);
* `pgbouncer_pools_client_maxwait_seconds > 1`;
* `pgbouncer_up == 0`;
* server connections per pool ≥ 90 % of `pool_size` for 10 m.

The existing `db_pool_acquire_seconds` / `db_pool_acquire_timeouts_total`
stay as they are. They now measure acquiring from the pooler.

Dashboard (Ranma-Config `monitoring-dashboards.yaml`): per-pool client
active/waiting, server active/idle, average query/transaction time.

### 4.7 Rollout

New chart values:

* `pgbouncer.enabled: false`, which renders nothing when false;
* `pgbouncer.services.<component>: false`, the per-service opt-in.

When a service opts in, its `DATABASE_URL` host becomes
`<fullname>-pgbouncer:6432`. Services with a lock session also get a
separate `DATABASE_LOCK_URL`, still direct. The budget helpers
(`distant-signal.servicePool`, INF-7) then count the opted-in services
against PgBouncer's `max_db_connections` instead of their own pools.

Order:

1. Ship the code prerequisites with the pooler still off:
   * timeouts via `ALTER ROLE … SET`;
   * api build lock moved to an xact lock;
   * a direct `DATABASE_LOCK_URL` for the two lock sessions;
   * a CI job that runs the DB tests through PgBouncer in transaction mode.
2. Enable PgBouncer with no services opted in, and check the
   exporter/alerts.
3. Opt in services one at a time, smallest first:
   * poller-ldbws;
   * full-coverage-consumer;
   * the notifier;
   * the aggregator;
   * the api, last.

   Watch `client_waiting` and `db_pool_acquire_seconds` after each.
4. Lower the per-service pools only once everything is behind PgBouncer.

### 4.8 What changes where

| DS chart / code (this repo) | Ranma-Config |
|---|---|
| `templates/pgbouncer-{deployment,service,configmap,networkpolicy,podmonitor}.yaml`, values, budget helpers, `DATABASE_URL` switching, per-role `pool_size` generated from `db-grants.yaml` | image pin and enabling the values for prod (`clusters/*/apps/distant-signal.yaml`) |
| `postgres-roles.sql`: lookup role, `get_auth` function, `ALTER ROLE … SET` timeouts | a Secret for the lookup role's password (sealed byte-exact) |
| `common::pg`, `planner_snapshot.rs`, `loops::runner` / `reference_signals` lock URL | Prometheus scrape allowance and dashboard panels |
| CI job: DB tests through PgBouncer | nothing for postgres-exporter or pg_dump (they stay direct) |

### 4.9 Interaction with the Postgres 18 upgrade

The pooler is independent of the upgrade, and the upgrade should go first.
`docs/postgres-18-upgrade.md` stops Postgres for `pg_upgrade --copy`. A
pooler in front would then only queue clients until `query_wait_timeout`
and report errors, adding a moving part to the maintenance window.

After the upgrade, PgBouncer needs nothing version-specific. Re-check
`auth_query` against PG 18's `pg_authid` (unchanged) and SCRAM (unchanged;
PG 18 deprecates MD5, which DS does not use).

`pg_upgrade` keeps role passwords, so `auth_query` keeps working.

## 5. Recommendation

### 5.1 Is a pooler worth it? Not now.

* **It does not fix the symptom.** A new pod's packets to the pooler go
  through the same default-deny policy and the same lag. A sidecar
  pooler would still need the pod's IP to be allowed to reach Postgres.
* **Nothing is short of connections.** Prod has 35 backends in use of 197
  usable (18 %). The configured maximum is ~71 to 87 (36 to 44 %), and the
  render-time budget checks stop overruns. Memory: Postgres uses
  1.3 GiB, almost all shared buffers. Idle backends cost a few MB each.
* **It has real costs:**
  * one more hop and a single point of failure on a single node;
  * an auth path with a `SECURITY DEFINER` function;
  * code changes at the 3 or 4 session-lock and session-option sites;
  * a CI job;
  * an image to maintain.

### 5.2 What fixes the symptom more cheaply

1. **Absorb the lag before the app starts, in CronJob/Job pods (chart
   only).** Add a `wait-for-postgres` init container (`pg_isready` loop),
   exactly as the migrate Job already has, to the schedule-reference,
   api-maintenance and poller-stations CronJobs. Init containers share
   the pod's IP, so by the time the app starts its policy rules exist.
   The cost is about 1 s and 16 Mi per run. The app's first acquire then
   takes about 10 ms, and the WARN and the `PoolTimedOut` retry go away.
2. **Give the start-up connect more than 5 s (small code change).** In
   `ds_store::pool::PoolSettings::connect_with_until_ready`, give the
   first connect its own budget. Add a `DATABASE_STARTUP_CONNECT_TIMEOUT_SECS`
   variable, 15 s by default, so sqlx's internal refused-retry rides out
   the lag. The run-time `acquire_timeout` stays at 5 s. Log the start-up
   case at INFO. This covers Deployments too: they currently log one WARN
   per rollout.
3. **Root cause (Ranma-Config / node).** Find out why the k3s
   network-policy controller takes 2 to 5 s or more to admit a new pod
   IP. Look at the controller's sync timing in the k3s log. Check
   whether CPU at 78 % and memory at 91 % stretch it (DS-251 is already
   about memory). Check whether k3s's controller can be tuned, or
   whether this is accepted as normal.
4. **Postgres settings:** none needed. If more evidence is ever wanted,
   turn on `log_connections = on` for a day (Ranma-Config / chart
   `postgresql.config`). It would show each connection's arrival time
   against the client's start. It takes effect on a reload; no restart
   is needed.

Pool sizes and `min_connections` do not matter here. `min_connections` is
0 everywhere and the first connect is eager. Making it lazy would only
move the 3 s wait to the first query.

### 5.3 When to revisit the pooler

Revisit when any of these happens:

* the api runs 3 or more replicas, or the configured maximum goes past
  ~150 of 197;
* steady backends exceed ~100;
* memory per backend becomes the limiting factor for `work_mem`;
* many short-lived clients appear (for example a per-request Job pattern).

## 6. Open questions for the user

1. **How to absorb the start-up lag?**
   (a) init container on the DB CronJobs;
   (b) a longer start-up connect budget in `ds_store::pool`;
   (c) both.
   *Recommended: (c).* (a) makes the CronJobs clean right away. (b) covers
   Deployments and any Job that is missed.
2. **Ask the Ranma-Config session to look into the k3s network-policy
   admission lag?** *Recommended: yes, at low priority.* It also hits
   Redis and every rollout, and a lag over ~5 s would fail first attempts
   anywhere.
3. **Build PgBouncer now (off by default) or defer?** *Recommended:
   defer.* Keep this spec, and revisit on the §5.3 triggers.
4. **If built: transaction mode with the code prerequisites, or session
   mode with no code changes?** *Recommended: transaction mode.* Session
   mode gives sqlx clients almost no benefit.
5. **If built: `auth_query` with a lookup role, or a userlist from
   Secrets?** *Recommended: `auth_query`.*
6. **If built: before or after the PG 18 upgrade?** *Recommended: after.*
7. **Move the per-service timeouts to `ALTER ROLE … SET` regardless of
   the pooler?** *Recommended: yes, but only together with the pooler
   work.* The startup `options` already cover today's direct
   connections.
