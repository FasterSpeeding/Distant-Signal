# Slow-Query Warnings — Research

**Status: research only, not a plan.** No code or configuration was changed
to produce this document.

## Goal

A pasted, real log excerpt from the live deployment shows `sqlx::query` WARN
lines — "slow statement: execution time exceeded alert threshold",
`slow_threshold=1s` — on simple single-row `INSERT ... ON CONFLICT DO
UPDATE`/`UPDATE` statements against `notifier_cursor`, `line_status`,
`line_status_history`, and `line_status_half_hourly_stats`, elapsed
1.00s–1.70s, spanning two unrelated services (`aggregator`, `notifier`) in
the same rough time window, correlated with a Postgres `LOG: checkpoint
complete` line whose write phase took 132.938s. Determine what is most
likely actually happening — missing index, checkpoint-induced I/O stall,
connection-pool exhaustion, or something else — and give a ranked,
evidence-based diagnosis and recommendation, not a shotgun list of possible
fixes.

## Current relevant state

### 1. Every affected `ON CONFLICT`/`UPDATE` target has a real backing index

- `notifier_cursor`: `PRIMARY KEY (name)` —
  `crates/api/migrations/20260902100000_notifications.sql:43`. The two
  slow statements against it are
  `crates/notifier/src/queries.rs:26-34` (`read_cursor`, `INSERT ...
  ON CONFLICT (name) DO UPDATE ... RETURNING`) and
  `crates/notifier/src/queries.rs:37-44` (`advance_cursor`, plain
  `UPDATE ... WHERE name = $2`, which also hits the same PK index).
- `line_status`: `PRIMARY KEY (line_id)` —
  `crates/api/migrations/20260510023522_initial.sql:70`. Written by
  `crates/aggregator/src/queries.rs:374-393` (`write_line_status`,
  `ON CONFLICT (line_id) DO UPDATE`).
- `line_status_history`: `id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY
  KEY` — `crates/api/migrations/20260510023522_initial.sql:89-94`. This is
  a plain append-only `INSERT` with no `ON CONFLICT` at all
  (`crates/aggregator/src/queries.rs:396-402`), so there is no conflict
  target to be missing an index on in the first place.
- `line_status_half_hourly_stats`: `PRIMARY KEY (line_id, half_hour_start)`
  — declared as `line_status_hourly_stats` in
  `crates/api/migrations/20260902090000_line_status_hourly_stats.sql:23-36`
  and renamed (table, column, constraint, index — a pure rename, not a data
  migration) in
  `crates/api/migrations/20260902170000_line_status_hourly_stats_to_half_hourly.sql`.
  Written by `crates/aggregator/src/queries.rs:579-583`
  (`ON CONFLICT (line_id, half_hour_start) DO UPDATE`).

**Conclusion: the missing-index theory is ruled out.** Every conflict
target checked is backed by a real unique/primary-key index. This also
matches the log pattern's own shape: a missing index would make one
specific query's plan bad, and that query would stay bad regardless of
what else the system is doing — it would not explain four *different*
tables across two *different* services going slow in the same narrow
window and being fast the rest of the time.

### 2. `sqlx`'s slow-statement timer measures pure server round-trip time, not pool-acquire wait

This app does not implement its own slow-query logging — `WARN
sqlx::query: slow statement...` is `sqlx`'s own built-in instrumentation
(`sqlx` 0.8.6, per `Cargo.lock:4181-4182`), and nothing in this repo
overrides it (no `log_slow_statements` call anywhere in `crates/`, no
`RUST_LOG` tuning of the threshold in `docker-compose.yml`). The 1s
threshold and the `slow_threshold=1s` field are `sqlx`'s own out-of-the-box
default:

```
// sqlx-core-0.8.6/src/connection.rs:199
slow_statements_duration: Duration::from_secs(1),
```

Critically, the elapsed time is measured inside `PgConnection::run()`
(`sqlx-postgres-0.8.6/src/connection/executor.rs:198-205`), which only
runs *after* a connection has already been checked out of the pool:

```rust
pub(crate) async fn run<'e, 'c: 'e, 'q: 'e>(
    &'c mut self,
    query: &'q str,
    ...
) -> Result<...> {
    let mut logger = QueryLogger::new(query, self.inner.log_settings.clone());
    // before we continue, wait until we are "ready" to accept more queries
    self.wait_until_ready().await?;
    ...
```

`QueryLogger::new` starts the `Instant::now()` clock (`sqlx-core-0.8.6/src/logger.rs:71-80`)
*before* `wait_until_ready()`, which is a no-op on an already-idle,
already-acquired connection — not before `Pool::acquire()`, which happens
entirely outside this function, one call earlier. So a slow `pool.acquire()`
(queueing behind other in-flight work because the pool's max size is
saturated) would **not** show up as part of the logged `elapsed` value here.
This decisively narrows the diagnosis: these numbers really are
"Postgres took over a second to execute/round-trip this one statement,"
not "the app waited over a second to get a connection."

### 3. Total connection-pool pressure on this Postgres instance

Only four services connect to Postgres directly (others go through the API
or don't touch the DB):

| service | `max_connections` | citation |
|---|---|---|
| `api` | 50 | `crates/api/src/app.rs:200-201` |
| `aggregator` | 10 | `crates/aggregator/src/main.rs:38-39` |
| `notifier` | 5 | `crates/notifier/src/main.rs:33` |
| `enricher` | 5 | `crates/enricher/src/main.rs:52-53` |
| **sum** | **70** | |

Postgres's own default `max_connections` (unmodified anywhere in this repo
— see §5) is 100, minus 3 reserved for superuser by default, so effectively
~97 usable. 70/97 (~72%) is a real, latent risk (a connection leak, an
`api` traffic spike, or an ad-hoc `psql` session could tip it into actual
exhaustion) but it is not close to saturated under steady-state load, and
per §2 it would not explain the specific *execution-time* numbers in these
log lines even if it were. This is a secondary risk to flag, not the cause
of the pasted warnings.

### 4. The aggregator's per-cycle write pattern: many small autocommit round trips, not one batched write

`run_cycle` (`crates/aggregator/src/main.rs:87-196`) does, once per 60s
cycle (`POLL_INTERVAL_SECS_AGGREGATOR` default 60,
`docker-compose.yml:311`):

- One `write_line_status` call per line (line 105-107) — up to `lines=110`
  in the pasted log — each doing a `SELECT` (`existing_statuses`) then an
  `INSERT ... ON CONFLICT DO UPDATE` on `line_status`, plus a conditional
  second `INSERT` into `line_status_history` when the status actually
  changed (`crates/aggregator/src/queries.rs:355-406`).
- One `record_daily_stats` + one `record_half_hourly_stats` call per line
  with sample coverage (lines 130-146) — `half_hourly_stats_recorded=90` in
  the pasted log, so ~90+90 more.

None of this is wrapped in an explicit transaction or batched into a
multi-row `INSERT`; each `.execute(pool)` / `.fetch_one(pool)` call is its
own implicit autocommit transaction against `pool: &sqlx::PgPool`. With
`synchronous_commit` at its Postgres default of `on` (nothing in this repo
sets it — see §5), **every one of these ~300–400 statements per cycle is
its own WAL-fsync-gated commit**, not batched writes sharing one fsync. The
notifier's own cursor read/advance (`crates/notifier/src/queries.rs:25-44`)
and per-poll writes follow the same one-statement-one-commit shape, and
`notifier`'s default poll interval is also 60s
(`POLL_INTERVAL_SECS_NOTIFIER` default 60, `docker-compose.yml:336`) — the
same as the aggregator's and the LDBWS poller's
(`POLL_INTERVAL_SECS_LDBWS` default 60, `docker-compose.yml:264`). All
three processes are started together by `docker compose up`; 60s
`tokio::time::interval`s started within seconds of each other on process
start stay roughly phase-aligned over the timescale of a single incident,
so their write bursts land in the same few seconds of every minute rather
than being spread evenly across it.

### 5. Postgres runs on completely untuned defaults, with no resource limits, in both deployment paths

**docker-compose** (`docker-compose.yml:63-77`, and this is the deployment
the pasted log actually came from — the container names `aggregator-1`,
`notifier-1`, `postgres-1` etc. are docker-compose's `<service>-<replica>`
naming convention, not Kubernetes pod names):
- `image: postgres:16`, no `command:` overriding any `postgresql.conf`
  setting, no `deploy.resources`/`mem_limit`/`cpus:` limiting the
  container, no separate WAL volume — everything lives on the single
  `postgres_data` named volume.
- No init script sets `shared_buffers`, `checkpoint_completion_target`,
  `checkpoint_timeout`, `max_wal_size`, or `synchronous_commit` — a repo
  grep across `docker-compose*.yml` and `charts/` for all of those turns up
  nothing except the *documentation* of an unused knob (see below).

**Helm chart** (`charts/distant-signal/values.yaml:76-132`,
`charts/distant-signal/templates/postgres-statefulset.yaml`): same
picture. `postgresql.resources` defaults to `{}` (values.yaml:123-128, the
CPU/memory request/limit example is commented out, not applied) and
`postgresql.extraEnv` defaults to `[]` (values.yaml:118-119, docstring
literally says "e.g. shared_buffers tuning" — i.e. the chart *anticipates*
this knob being needed but nothing sets it out of the box). `storageClass`
is empty, i.e. whatever the cluster's default is (values.yaml:113).

So both the deployment the log came from and the chart run Postgres 16 at
its stock upstream defaults: `shared_buffers` 128MB, `checkpoint_timeout`
5min, `checkpoint_completion_target` 0.9, `max_wal_size` 1GB,
`synchronous_commit` on — and no CPU/memory guarantee at all, competing for
whatever the host/node gives it.

### 6. Re-reading the checkpoint log line against those defaults

```
checkpoint complete: wrote 896 buffers (5.5%); 0 WAL file(s) added, 0 removed, 0 recycled;
write=132.938 s, sync=0.014 s, total=133.321 s; sync files=50, longest=0.014 s, average=0.001 s;
distance=6552 kB, estimate=6730 kB
```

The task brief's framing treats `write=132.938s` for ~7MB (896 × 8KB) as
itself evidence of a pathologically slow disk. That is not quite right, and
it's worth being precise about why, because it changes where the
investigation should point:

- Postgres's checkpointer does not write buffers as fast as it can and then
  stop — with `checkpoint_completion_target=0.9` (the default) and
  `checkpoint_timeout=300s`, a time-driven checkpoint deliberately paces
  its buffer writes with sleeps in between, targeting completion by
  `0.9 × 300s = 270s` after the checkpoint started. A write phase of 133s
  is comfortably *inside* that pacing budget — on its own, this number is
  consistent with normal default-configuration behavior, not proof of a
  slow disk.
- The number that actually reflects real storage latency is the **sync**
  phase: `sync=0.014s` total across `sync files=50`, `average=0.001s`
  (1ms) per file. That is fast — not what you'd expect if the underlying
  block device itself had high fsync latency across the board.

Taken together, this specific checkpoint log line is weaker evidence for
"the disk is slow" than the task brief assumes, and is closer to "the
checkpointer is behaving exactly as configured by default." It should not
be read as the smoking gun by itself. What it *does* still support is
temporal correlation: this checkpoint's write phase (133s, i.e. spread
across more than two full 60s poll cycles) overlaps with the aggregator's
and notifier's write-heavy windows described in §4, and Postgres's
checkpointer and ordinary backend WAL-fsync traffic contend for the same
underlying I/O device and, right after a checkpoint starts, the same WAL
stream carries larger full-page-write records for the first change to each
page — a documented, real mechanism for backend commit latency to
transiently rise during/just after a checkpoint, independent of whether
the checkpoint's own buffer-write pacing was "slow" in absolute terms.

## Findings — ranked diagnosis

Ranked by how well each theory explains **all** of the observed shape: (a)
several *different*, *simple*, *properly-indexed* single-row writes, (b)
across *different* tables, (c) across *different* services, (d) elapsed
measured as true execution time (not pool-wait), (e) correlated with, but
not fully explained by, a nearby checkpoint.

1. **Most likely: WAL-fsync latency on shared, untuned, resource-unguaranteed storage, hit by many small autocommit transactions in the same narrow window — a systemic I/O/commit-latency issue, not a per-query one.**
   Every one of these statements is its own autocommit transaction
   (§4), so its latency is dominated by however long the WAL fsync backing
   that commit takes on this specific storage device, not by index lookup
   or lock wait on its own small table. Because Postgres has exactly one
   WAL per instance, a slow/contended fsync at the storage layer raises
   the floor for *every* concurrently-committing backend at once,
   regardless of which table or which service it belongs to — this is the
   one mechanism in this investigation that naturally explains why
   `notifier_cursor`, `line_status`, `line_status_history`, and
   `line_status_half_hourly_stats` all went slow together, from two
   unrelated services, rather than one specific query being consistently
   slow. It also fits perfectly with §2 (measured as real server-side
   execution time) and is amplified by §4's phase-aligned 60s poll cycles
   stacking ~300–400 individually-committing statements into the same
   few-second window every minute, and by §5/§6 (no resource guarantee, no
   `synchronous_commit` tuning, and a checkpoint — itself a documented
   source of transient commit-latency spikes via I/O contention and
   post-checkpoint full-page writes — landing in the same window).
2. **Contributing/triggering, not sufficient alone: checkpoint-adjacent write pressure.**
   As reasoned in §6, the checkpoint's own write-phase duration is mostly
   explained by normal `checkpoint_completion_target` pacing, not proof of
   a uniformly slow disk (the fast `sync` phase argues against that). But
   its timing overlapping the slow-statement warnings is real and is a
   plausible amplifier of theory 1, not a separate competing cause: it's
   the same underlying I/O/WAL system, stressed a bit further by
   checkpoint I/O and larger post-checkpoint WAL records, at the same
   moment the aggregator/notifier are doing their own commit-heavy burst.
3. **Ruled out: missing index on an `ON CONFLICT`/`UPDATE` target.**
   §1 confirms real PK/unique indexes back every conflict target checked.
   A missing index would also produce a *consistently* slow query
   regardless of what else is happening, not a correlated multi-table,
   multi-service burst — the observed pattern doesn't fit this theory even
   before checking the schema.
4. **Ruled out as the cause of these specific log lines: connection-pool exhaustion.**
   §2/§3: the combined pool ceiling (70) hasn't reached Postgres's default
   `max_connections` (100), and — more decisively — `sqlx`'s slow-statement
   timer starts after a connection is already acquired, so pool-acquire
   queueing wouldn't appear inside these `elapsed` values even if the pool
   were saturated. Worth fixing as a latent risk (see Recommendations) but
   it isn't what produced this log excerpt.
5. **Ruled out as its own explanation: `notifier_cursor`'s table design.**
   It's a two-column, one-row-per-cursor-name table with a real PK
   (§1) and the simplest possible access pattern (`read_cursor`/
   `advance_cursor`, `crates/notifier/src/queries.rs:25-44`) — nothing
   about its shape would cause lock contention with unrelated tables like
   `line_status`. It shows up in the warnings because it's written on the
   same 60s cadence from the same Postgres instance as everything else
   (§4), i.e. it's a genuine instance of theory 1, not a distinct problem
   of its own.

**Bottom line:** this reads as an infrastructure/resource-provisioning
issue, not an application logic bug. The proximate mechanism (many
individually-committing autocommit writes hitting WAL-fsync latency on
storage with no CPU/memory guarantee, amplified by phase-aligned 60s poll
cycles and made worse by an overlapping checkpoint) is real and is visible
in this repo's own config, but the actual fix for "why is fsync slow right
now" — the host/node's disk, its storage class, and what else is
competing with the Postgres container for CPU/I/O — is outside anything
this repo controls or can verify from source alone. That's stated plainly
in Recommendations below rather than papered over with an app-side change
that wouldn't move the real bottleneck.

## Recommendations

Prioritized; not mutually exclusive.

1. **(Infra, highest expected impact, needs an operator, not a code change) Investigate the actual storage/CPU the Postgres container is running on in the live deployment.** Concretely: what backs the `postgres_data` docker-compose volume (local block device vs. overlay-on-network storage vs. a constrained dev/CI host), and whether the host was under concurrent CPU/disk load from something else at the time of the pasted logs (a rebuild, another container, a shared/virtualized dev machine). This repo cannot answer this from source — `docker-compose.yml` and the Helm chart both leave Postgres fully unconstrained and untuned (§5), so there is nothing in-repo currently pinning it to particular hardware/storage characteristics one way or the other. This is the single highest-leverage thing to check given the ranked diagnosis in Findings.
2. **(Infra/config, cheap, safe to do regardless of #1's answer) Give the Postgres container explicit resource requests/limits and, if the live host has memory/CPU to spare, some baseline tuning.** In docker-compose, add `deploy.resources` (or `mem_limit`/`cpus`) to the `postgres` service (`docker-compose.yml:63-77`); in the chart, set `postgresql.resources` (`charts/distant-signal/values.yaml:123-128`, already scaffolded/commented) and consider using the existing `postgresql.extraEnv` hook (values.yaml:118-119, already documented for exactly this) to raise `shared_buffers` and/or set `synchronous_commit`/checkpoint settings appropriately for the actual hardware. This won't fix an underlying slow disk, but it removes "Postgres got starved of CPU by a neighbor" as a variable and gives predictable behavior to reason about next time.
3. **(App-level, moderate effort, reduces exposure but doesn't fix root cause) Batch the aggregator's per-cycle writes.** `run_cycle` (`crates/aggregator/src/main.rs:105-146`) issues up to ~300-400 separate autocommit statements per 60s cycle (§4). Wrapping the per-line `write_line_status`/`record_daily_stats`/`record_half_hourly_stats` calls in a single transaction (or batching them into multi-row `INSERT ... ON CONFLICT` statements) would collapse that down to a handful of WAL fsyncs per cycle instead of hundreds, meaningfully reducing how much surface area a slow-fsync window has to hit, and reducing how long the aggregator holds a connection checked out. This is worth doing on its own engineering merits (it's also strictly fewer round trips), but per the ranked diagnosis it treats a symptom (how much commit traffic gets exposed to bad fsync latency) rather than the root cause (why fsync is occasionally slow).
4. **(App-level, cheap, addresses the secondary risk from §3) Revisit combined pool sizing versus Postgres's `max_connections`.** 70 combined (`api` 50 + `aggregator` 10 + `notifier` 5 + `enricher` 5) against a default `max_connections` of 100 isn't the cause of the pasted warnings (§2/§4 of Findings), but it's closer to the ceiling than is comfortable, especially since nothing in this repo raises Postgres's own `max_connections` to compensate. Either lower `api`'s ceiling if 50 was a round-number guess rather than a measured need, or explicitly raise Postgres's `max_connections` alongside `shared_buffers`/`work_mem` in the same tuning pass as recommendation #2, so headroom is a deliberate choice rather than an accident of defaults.
5. **(Do not do, absent further evidence) Do not add or change any index.** §1 confirms this is not an index problem; adding an index here would add write overhead without addressing anything in the ranked diagnosis.

## Explicitly out of scope

- Reproducing the exact 1.0–1.7s latencies in a local environment — this
  document is a source/config-level investigation, not a live
  reproduction or load test.
- Any live inspection of the actual production/dev host's disk, storage
  class, or concurrent CPU load — recommendation #1 explicitly hands this
  to an operator with real access to that infrastructure, because it
  cannot be determined from this repository's source alone.
- Any change to `notifier_cursor`'s schema or access pattern — §1/§5 of
  Findings conclude its design is not implicated.
- Tuning `autovacuum` — not investigated here; flagged only as an open
  question below, since the write pattern in §4 (frequent UPDATEs to a
  small number of hot rows in `line_status`) is exactly the shape that
  benefits from checking `autovacuum` isn't falling behind, but nothing in
  the pasted logs directly implicates it (no autovacuum log lines were
  provided), so this document does not claim it as part of the ranked
  diagnosis.
- Any code change. Per the task, this document is research only.

## Open questions / risks

- **What is actually backing the `postgres_data` docker volume on the host
  the pasted logs came from?** This is the single biggest open question —
  the entire top-ranked diagnosis (Findings #1) depends on it, and it
  cannot be answered from this repository.
- **Was there other load on the same host/node at the time of the pasted
  logs** (a concurrent build, another container, a shared/virtualized dev
  box)? The container naming in the log excerpt (`aggregator-1`,
  `notifier-1`, `postgres-1`) confirms this came from `docker compose up`,
  which typically means a single shared host running every service in
  this stack plus, in a dev/CI setting, whatever else that machine is
  doing.
- **Does this same pattern show up in the Helm/Kubernetes deployment**, or
  only in the docker-compose stack? Both leave Postgres equally untuned
  (§5), but a Kubernetes node's own scheduler/cgroup behavior around an
  unconstrained (`resources: {}`) pod is a different failure mode
  (eviction risk, noisy-neighbor CPU throttling via cgroups) than a single
  docker-compose host, and is worth checking separately if this recurs
  there.
- **Is `autovacuum` keeping up on `line_status`?** It's a small,
  high-churn table (one row per line, updated every ~60s per line) —
  worth an operator running `SELECT * FROM pg_stat_user_tables WHERE
  relname = 'line_status'` to check `n_dead_tup` and last autovacuum time
  the next time this recurs, even though this document does not currently
  have evidence implicating it.
- **If recommendation #3 (batching the aggregator's writes) is pursued
  later**, it should go through the normal design process (a
  `-design.md` doc, not this research doc) — it touches
  `write_line_status`'s existing changed-detection logic
  (`crates/aggregator/src/queries.rs:355-406`) and the daily/half-hourly
  stats reconciliation invariant documented at
  `crates/aggregator/src/main.rs:136-142`, both of which have their own
  correctness constraints beyond just "fewer round trips."
