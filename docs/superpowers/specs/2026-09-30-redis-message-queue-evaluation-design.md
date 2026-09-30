# Design: Redis message queue evaluation (streams, encryption at rest, persistence)

**Status (2026-09-30): evaluation plus five small changes, implemented on
this branch (not merged). Every other recommendation below needs the
user's approval first.**

This document covers the Redis Streams that carry Distant Signal's work
between services. It does three things:

1. It looks for real weaknesses in how the queue is run: trimming,
   reclaim, dead letters, backpressure, connections, memory, single points
   of failure and metrics.
2. It judges whether Redis needs encryption at rest, and checks encryption
   and authentication in transit.
3. It rechecks persistence and the backup design's conclusion that Redis
   needs no backup, and adds AOF-health alerts.

## Sources

- The chart: `charts/distant-signal/templates/redis-*.yaml`,
  `prometheusrule.yaml`, and `values.yaml` (`redis.*`, `movementRelay.*`,
  `metrics.prometheusRule.*`).
- The code that uses streams:
  - `crates/movement-feed/src/redis_stream.rs`, shared by all three
    movement consumers;
  - `crates/movement-relay/src/{main,event_sink}.rs`;
  - `crates/enricher/src/stream.rs`;
  - `crates/api/src/data/queries.rs` (`upsert_incidents`);
  - `crates/common/src/redis_conn.rs`.
- Docs:
  - `docs/movement-events-deadletter.md`;
  - `docs/superpowers/specs/2026-09-04-movement-relay-design.md`;
  - the approved `2026-09-30-backup-and-observability-gaps-design.md`,
    section 5;
  - `/home/coder/ds-review/{full-review-infra,full-review-pipelines,uk-legal-compliance-2026-09-27}.md`.
- Ranma-Config, read only:
  - `clusters/mine-bringer/apps/distant-signal-exporters.yaml` (the
    redis_exporter);
  - `monitoring-rules.yaml` (its `redis` group);
  - `distant-signal.yaml` (`redis.persistence.size: 1Gi`).
- Production checks, read only, on 2026-09-30. These used `kubectl get`
  on the Redis Deployment and `redis-cli CONFIG GET`, `INFO
  persistence|memory|keyspace|server|clients`, `XLEN`, `XINFO GROUPS`, `ACL
  LIST` (with password hashes redacted) and `df /data`. No key contents
  and no Secrets were read.

## What production looks like (2026-09-30)

| Item | Value |
|---|---|
| Server | redis 7.4.11. The chart's `redis:` image is **not** valkey. |
| Args | `--appendonly yes --save "" --maxmemory 1536mb --maxmemory-policy noeviction`. There is no `--requirepass`. |
| AOF | `appendfsync everysec`, `aof-use-rdb-preamble yes`, `auto-aof-rewrite-percentage 100`, `auto-aof-rewrite-min-size 64mb`, `aof-load-truncated yes` |
| AOF on disk | 352 MB: a 230 MB base plus about 122 MB of increments. 4 rewrites since the last start, the last one took 3 s, and 0 failures. |
| Status | `aof_last_write_status:ok`, `aof_last_bgrewrite_status:ok`, `aof_delayed_fsync:0` |
| Memory | 820 MB used, 856 MB RSS, maxmemory 1.5 GB (about 55% used) |
| Keyspace | 2 keys: `movement-events`, with 1,048,578 entries, which is at its cap (about 24 h, about 820 B per entry); and `incident-text-changed`, with 5,352 entries. `movement-events-deadletter` does not exist, so nothing has ever been dead-lettered since the data was last lost. |
| Groups | `trust-consumer`, `full-coverage-consumer` and `trust-event-backlog` each had pending 0 and a lag of about 280 entries (seconds). `enricher` had pending 0 and lag 0. |
| Auth | `ACL LIST` gives `user default on nopass … +@all`. `protected-mode no`, `tls-port 0`. |
| Volume | The PVC is 1Gi (pinned by Ranma-Config), on `local-path`. Inside the pod, `/data` is **`/dev/vda4`, the node's 2.0 TB root filesystem, 63% used**. |

`local-path` does not enforce the PVC size: the AOF shares the node's root
disk with Postgres and everything else. The 1Gi pin is therefore harmless
today. The "a full volume fails the AOF write" warning in `values.yaml`
(`redis.persistence.size`) only applies to a StorageClass that enforces
size. On this cluster, "full" means the node's disk is full.

## Part 1: the queue

### What is in Redis

These facts come from the code, and the keyspace above confirms them.

| Key | Producer | Consumers (group) | Bound | Loss backstop |
|---|---|---|---|---|
| `movement-events` | movement-relay (Kafka → one `XADD` per TRUST envelope) | trust-consumer, full-coverage-consumer, trust-backlog-consumer (`trust-event-backlog`) | `MAXLEN ~ 1,048,576` (`event_sink.rs:35-47`, `config.rs:109`) | None inside DS. Kafka, only while the RDM topic still holds the data. |
| `movement-events-deadletter` | the three consumers (`redis_stream.rs:478-552`) | an operator (`docs/movement-events-deadletter.md`) | 10,000. It is never trimmed, and writes are refused when full (`redis_stream.rs:35`, `:498-515`). | None |
| `incident-text-changed` | api (`queries.rs`, `text_changed_xadd`) | enricher (`enricher`) | `MAXLEN ~ 10,000` | The enricher's hourly sweep |

**Nothing else uses this Redis.** Three things that might look like Redis
users are not:

- The notifier push queue is an in-process bounded queue
  (`crates/notifier/src/push_queue.rs`), not Redis.
- The api's rate limiter is in memory (`crates/api/src/rate_limit.rs`).
- Sessions, push subscriptions and OAuth state live in Postgres, or in the
  MCP's own `ds-mcp-redis`, which this document does not cover.

### What is sound already

- **Memory policy.** `noeviction` is set and verified in production
  (`values.yaml`, `redis.maxmemoryPolicy`). The chart comment explains why
  any other policy would evict whole streams.
- **Backpressure.** An `XADD` refused at maxmemory (OOM) makes the relay
  hold the Kafka record, pause, retry every 2 s and never commit
  (`event_sink.rs:94-140`, `main.rs` `run_cycle`). It also runs `XTRIM` so
  that lowering the cap can recover. Kafka holds the backlog.
- **Bounded connections (INF-5).** Every worker uses
  `common::redis_conn::RedisConn`: one bounded connect attempt, a 30 s
  response timeout, and the connection dropped on a timeout. **The api was
  the exception** (see W1; fixed).
- **Reclaim.** `XAUTOCLAIM` runs every `autoclaimMinIdle` (30 s) and the
  pending-entries list (PEL) is replayed from `0` in 1000-entry pages
  (`redis_stream.rs:978-1013`, M15). Entries that keep failing are only
  *reported* after 240 deliveries, never dead-lettered for their delivery
  count. That is correct: a long api outage must not empty the stream into
  the dead-letter stream.
- **Dead letters.** Only real poison is dead-lettered:
  - an explicit 4xx rejection, isolated one entry at a time;
  - a malformed entry;
  - a row the api rejected.

  The payload is kept for re-injection.
- **Recovering from NOGROUP.** A running consumer recreates its group at
  its last delivered id (`recreate_start_id`, `redis_stream.rs:683`), so an
  empty restart of Redis loses no entries written *after* the restart.
- **Rollouts.** `strategy: Recreate` (INF-1) and a 4 s AOF load at this
  size.

### Weaknesses found

**W1 (fixed): the api's Redis publish had no bound.**
`upsert_incidents` connected with redis-rs's default
`get_connection_manager()` (`queries.rs:380` before this branch). By
default that makes 6 more connect attempts on a 1 s-then-60 s backoff, with
no connect timeout and no response timeout. The new test showed the old
code still waiting when its 10 s limit ran out; by the default schedule it
would have waited about 5 minutes. On a half-open connection the wait had
no limit at all. The wait happened inside the poller's incident-ingest
HTTP request, after the rows were committed, so a Redis restart stalled
incident ingestion.

- Fix: use `common::redis_conn::connect`, the same as the workers.
- Commit: `fix(api): bound the text-changed publish's Redis connection
  (INF-5)`.

**W2 (fixed): the lag alerts ignored the pending list.**
After a transient POST failure, `post_failed` (`trust-consumer/src/main.rs:499-527`)
leaves the batch un-ACKed. The next `next_batch` then reads *new* entries
with `>` (`redis_stream.rs:872`). During an api outage each consumer
therefore keeps moving through the stream, and every failed batch piles
up in its PEL. `XINFO GROUPS lag` counts only undelivered entries, so it
stays near 0. `DistantSignalMovementLagHigh`/`Critical` (lag ÷ MAXLEN)
could not fire during the very outage they exist for, even as MAXLEN
began trimming entries that were pending. `DistantSignalStreamGap`
reported the loss only afterwards. (Ranma-Config's
`redis_stream_group_messages_pending > 1000` alert covers production. The
chart had nothing.)

- Fix: the relay exports `distant_signal_movement_relay_stream_pending{group}`,
  and the two lag alerts divide **lag + pending** by the cap.
- Commit: `feat(movement-relay,chart): count pending entries in the
  movement lag alerts`.

**W3 (fixed): the NearFull alert read a gauge that is set only on write.**
`distant_signal_movement_feed_deadletter_length` is set only inside
`dead_letter()` (`redis_stream.rs:560-565`). It is missing after every
consumer restart, and after an operator drains the stream it keeps its
last value until the next dead-letter. `DistantSignalDeadLetterNearFull`
could therefore never fire on a restarted pod, and it kept firing after a
drain.

- Fix: the relay reads `XLEN movement-events-deadletter` every 30 s and
  exports `distant_signal_movement_relay_deadletter_length{stream}`. The
  alert uses that value and falls back to the old gauge during rollout.
- Commit: `feat(movement-relay): export the dead-letter length and Redis
  AOF status`.

**W4 (fixed): a Redis that refuses writes stopped all TRUST ingestion
silently.**
The relay can be refused because of OOM, a failed AOF write (`MISCONF`),
NOAUTH after an auth change, or Redis being down. When that happens,
every relay cycle fails, and each failed cycle still counts as progress,
so `/livez` stays 200 (`main.rs`). The consumers stay caught up with a
stream that no longer grows, so no lag alert fires. The chart had no alert
for this state. Ranma-Config's `RedisDown` and OOM alerts cover two of
the four causes in production.

- Fix: `DistantSignalMovementRelayPublishFailing` (critical) fires when
  every `XADD` failed over 10 m and nothing was published.

**W5 (fixed): the chart could not see the AOF.**
The chart ships no redis_exporter. A failed AOF write makes Redis refuse
writes. A failed rewrite lets the AOF grow until the disk fills. Neither
was visible. Ranma-Config has the exporter's `redis_aof_*` series but no
alert on them. The backup design already listed that alert as a
Ranma-Config to-do.

- Fix: the relay reads `INFO persistence` every 30 s and exports
  `distant_signal_redis_aof_{enabled,last_write_ok,last_bgrewrite_ok}`.
- Alert: `DistantSignalRedisPersistenceFailing` (critical). The
  "AOF off" clause renders only for the bundled Redis with
  `redis.persistence.enabled`.

**W6 (fixed): losing Redis's data was invisible.**
After Redis comes back empty, the consumers recover through NOGROUP and
log a warning, and nothing else happens. `check_gap` cannot see the loss,
because the new stream has no trimmed range. The loss covers the unread
tail, every PEL and every dead letter.

- Fix: `distant_signal_movement_feed_group_recreated_total{group}`,
  registered at 0.
- Alert: `DistantSignalMovementGroupRecreated` (warning: "treat that rail
  day as partial").
- Commit: `feat(movement-feed): count consumer groups recreated after
  NOGROUP`.

**W7 (recommendation R2): a consumer that *restarts* after data loss skips
entries.**
`connect_to_stream` creates the group at `$` (`redis_stream.rs:284`). If
Redis comes back empty and the relay recreates `movement-events` before a
consumer (re)starts, every entry the relay wrote in between is skipped.
After an outage the relay is draining a Kafka backlog as fast as it can,
so that could be minutes of TRUST. The backup design's loss runbook says
"restart the three consumers". That instruction makes this worse: consumers
that are still running already recover at the right position through
NOGROUP.

**W8 (recommendation R3): the dead-letter stream has no age limit.**
It keeps raw TRUST payloads with no age bound (`redis_stream.rs:23-35`).
The self-imposed 1-day TRUST retention safeguard (`crates/aggregator/src/config.rs:104-123`;
LEG-17/LEG-27 in the legal review) covers `trust_event_backlog`, but
nothing in the dead-letter runbook asks for records to be resolved or
deleted within a day. The dead-letter stream is empty in production
today.

**W9 (recommendation R4): the consumer runs ahead during a downstream
outage.**
This is the behaviour behind W2. The alert now covers it. Changing the
behaviour itself is a separate decision: after a transient failure, read
the consumer's own PEL again instead of `>`. That would keep processing in
order and keep lag honest, but it changes how every consumer retries.

**Minor points, not worth a change on their own:**

- The dead-letter cap check is `XLEN` and then `XADD`, which is not
  atomic. Three groups racing could overshoot 10,000 by a batch.
- `DistantSignalMovementLagGrowing` still uses lag only.
- The `trustBacklogConsumer` resources comment said "drains a Redis list".
  Fixed in `docs(chart): trust-backlog-consumer reads a stream, not a
  Redis list`.

### Single points of failure

There is one Redis on one node:

- A Redis restart costs about 4 s. The relay applies backpressure and the
  consumers retry.
- A Redis outage stops all TRUST processing. Kafka holds the backlog for as
  long as the RDM topic retains it, and that retention is unknown (see the
  backup design).
- Losing the node takes Postgres, Redis and every consumer with it. The
  `local-path` PVC is bound to that node.

A replica or Sentinel on the same node would add failover for the process
only, at the cost of doubling Redis's memory (about 1.5 GB) plus another
AOF. **Recommendation: no HA.** The existing single points (the node and
Postgres) dominate, and the new alerts make an outage visible within
minutes.

## Part 2: encryption

### What is sensitive

**Nothing personal is in Redis.** It holds:

- `movement-events`, which is raw TRUST train-movement envelopes: train
  ids, STANOX codes, times and TOC codes;
- incident ids;
- dead-lettered copies of the same kind of data.

It holds no user ids, push endpoints or subscriptions, sessions, tokens or
IP addresses. The only sensitivity is **licensing**: RDM-licensed feed
data, which is open data under the Network Rail licence as currently
read, plus the self-imposed 1-day TRUST safeguard.

### Where it lands on disk

The AOF in `appendonlydir/` on the node's root filesystem (`/dev/vda4`,
through local-path) holds:

- an RDB-format base with the whole stream as of the last rewrite;
- an increment of every `XADD`/`XACK` since then.

In memory the stream covers about 24 h. On disk the oldest entry is at
most about 24 h plus the time since the last rewrite (a rewrite happens
when the AOF doubles, which is hours). The blocks of deleted AOF files are
not wiped either. There is no RDB (`--save ""`), and Redis is not backed up.

`/dev/vda4` is a plain partition: there is no dm-crypt in the guest.
Whether the hosting provider encrypts the virtual disk underneath was not
checked.

### Threat model

| Threat | Would Redis at-rest encryption help? |
|---|---|
| Someone with root on the node, or a container escape | No. They read Redis's memory, the Secrets and Postgres directly. |
| A stolen disk or a leaked provider snapshot of the VM | Yes, but the same snapshot holds Postgres, which **does** hold personal data (accounts, journeys, tickets, subscriptions). Encrypting Redis alone would protect the least sensitive store on the disk. |
| A leaked backup | Not applicable: Redis is not backed up. The Postgres dumps are age-encrypted already. |
| Another pod reading the volume | Not applicable: local-path volumes are per-PVC hostPath directories, and PodSecurity `restricted` blocks hostPath. |

### Options

1. **App-level payload encryption** (encrypt `payload` before `XADD`,
   decrypt in each consumer).
   - Costs:
     - key management across 4 or more services;
     - CPU on every message;
     - the `XRANGE`/re-inject runbook for dead letters stops working
       without the key;
     - the exporter's stream metrics are unaffected, but any debugging of
       the stream is harder.
   - Benefit: it protects about 24 h of non-personal open data.
   - **Rejected.**
2. **Volume or node disk encryption** (LUKS on the data partition, or the
   provider's disk encryption).
   - Benefit: it covers Postgres, Redis, schedulefeed and every other
     local-path volume at once.
   - Costs:
     - an unlock step at boot (a key file, TPM, or Clevis/Tang), which on
       a single rented VM usually means either a manual unlock after each
       reboot or a key stored next to the disk (which defeats the stolen
       disk case);
     - a migration with downtime.
   - It is worth doing only for Postgres's sake, and only if the
     stolen-disk or snapshot threat is credible for this provider.
   - That makes it an infrastructure decision (Ranma-Config and the host),
     not a DS one.
3. **Nothing Redis-specific.**

**Verdict: option 3 for Redis.** valkey and Redis have no native at-rest
encryption. Redis's contents are non-personal, short-lived and licensed
open data, and anyone who could read the disk could also read the
personal data in Postgres next to it. If at-rest encryption is wanted, do
it once at the node or provider level (option 2), justified by Postgres.
First find out whether the provider already encrypts the disk
(**user decision D3**). Separately, the licence posture already relies
on the 1-day safeguard, so R3 (dead letters) matters more than
encryption.

### In transit and authentication

- **TLS:** none (`tls-port 0`). All Redis clients run on the same node, so
  the traffic stays on the node's bridge and never crosses a wire.
  **TLS is not recommended**: it adds certificate management and every
  client needs `rediss://` support, for no gain on one node.
- **AUTH:** **off in production.** There is no `--requirepass` and `default
  nopass`. The chart supports it (`redis.auth.*`, INF-3), but it is off by
  default (`values.yaml:1649-1650`), and the prod overlay does not turn it
  on. Ranma-Config's exporter comment also says "The DS Redis has no
  password".
- **Mitigation today:** the chart's NetworkPolicies are on in production.
  Ingress to Redis on :6379 is allowed only from the api, the enricher,
  the three consumers, movement-relay and the redis_exporter. The residual
  risk is a compromised pod in that list: it could `DEL`/`XTRIM`/`XGROUP
  DESTROY` without any credential.
- **Recommendation R1:** turn on `redis.auth`, using the chart's two-step
  sequence. This is a Ranma-Config change, and the exporter must be
  updated in the same change (see deploy notes).

## Part 3: persistence

### Current settings

These are the defaults, and production runs them:

- AOF `everysec`;
- rewrite at 100% growth with a 64 MB minimum;
- RDB preamble on;
- RDB snapshots off;
- `aof-load-truncated yes`;
- Recreate strategy;
- the chart's PVC default is 4Gi (production pins 1Gi, which local-path
  does not enforce).

These are right for this workload. **No change is recommended to
`appendfsync`**; see the loss table.

### What each failure loses, and how the pipeline recovers

| Event | Lost | Recovery | Visible now? |
|---|---|---|---|
| The Redis process crashes or is OOMKilled | Nothing that was acknowledged. With `everysec`, each write reaches the page cache before the reply, and only the fsync is deferred. | Restart (about 4 s AOF load). The relay holds Kafka and the consumers retry. | Pod restarts |
| Kernel panic or power loss on the node | Up to about 1–2 s of `XADD`s that Redis had acknowledged. The relay had already committed those Kafka offsets, so the loss is permanent unless Kafka is replayed: about 25–60 TRUST messages. Lost `XACK`s and group positions just cause redelivery (deduplicated by `dedup_key`). | Automatic. `aof-load-truncated` loads a torn tail. | No. It is too small to see, and the gap check cannot see it. |
| AOF corrupted in the middle (not just a torn tail) | Redis refuses to start and crash-loops. | Manual: run `redis-check-aof --fix` on the incr file (R5 runbook). | `RedisDown` (Ranma) and **PublishFailing** (new) |
| AOF write fails (disk full or failing) | Nothing yet: Redis refuses writes and the relay holds Kafka. | Free disk space, and Redis resumes by itself. | **RedisPersistenceFailing** and **PublishFailing** (new) |
| PVC or data lost (node disk replaced, PVC deleted), with the consumers still running | The unread tail (lag, normally about 300 entries, a few seconds), every PEL (normally 0 to 100), all dead letters | Automatic. The relay's next `XADD` recreates the stream, and each consumer gets NOGROUP and recreates its group after its last delivered id. | **GroupRecreated** (new) |
| The same, but the consumers restart after the relay wrote again | The above, **plus** everything the relay wrote in between (W7) | The groups are created at `$` | GroupRecreated does not fire (no NOGROUP). Only the TRUST day's figures show it. |
| The node is lost | Everything on the node, Postgres included | Rebuild the node; Postgres comes back through PITR and the dump | Everything |

Full coverage marks a day partial on its own when the consumer restarts
(the "partial days" behaviour). After a NOGROUP recovery *without* a
restart, the day is not marked partial automatically. The new alert tells
the operator to do it.

### Does DS Redis need a backup? Confirmed, with one change

The backup design's conclusion holds for the queue:

- A periodic copy can never give back the entries that matter, which are
  the ones written after the copy was taken.
- The consumer positions in an old copy would be stale.
- A copy would put raw TRUST, including dead letters, somewhere with a
  7-day retention, which undercuts the 1-day safeguard.

**Change to the backup design's loss runbook:** do **not** restart the
consumers after Redis loses its data. Running consumers recover through
NOGROUP at the right position, and a restart creates the groups at `$`
and skips entries (W7). Restart one only if it has not recovered, which
shows as an alert that keeps repeating. With R2 in place, restarts become
safe too. The real replay source is Kafka (an offset reset on the RDM
group), and it is still unverified.

### AOF-health alerts

AOF-health alerts are now in the chart (W5). Ranma-Config should still add
`redis_aof_last_write_status != 1` and `redis_aof_last_bgrewrite_status
!= 1` on the exporter, as the backup design planned. Once the chart
alerts are live in production they are redundant, so either can be
dropped. **Keeping both double-pages**, which is why I recommend keeping
the chart's version and skipping the Ranma-Config one (**user decision
D4**).

## Recommendations, in priority order

| # | Change | Repo | Cost | Risk | Status |
|---|---|---|---|---|---|
| 1 | W1 to W6: bounded api connection; pending-aware lag alerts; fresh dead-letter length; PublishFailing, RedisPersistenceFailing and GroupRecreated alerts | DS app and chart | Done | Low. New alerts could be noisy: tune them with `metrics.prometheusRule.{relayPublishFailing,redisPersistence,groupRecreated}` | **Implemented on this branch** |
| R1 | Turn on Redis AUTH in production | Ranma-Config (and the exporter) | 30 min, two deploys | Clients get NOAUTH if the order is wrong (the chart's sequence avoids it). The exporter goes blind if it is not updated. | Needs approval (D1) |
| R2 | The relay creates the three consumer groups at `0` when `movement-events` does not exist yet, before its first `XADD` after a (re)start | DS app (movement-relay) | Small, with tests against local valkey | Behavioural. The group names are then listed in two crates (already true for the lag gauge). | Needs approval (D2) |
| R3 | Dead-letter retention: add to `docs/movement-events-deadletter.md` that records are resolved (re-injected or deleted) within 1 day, and optionally alert on the oldest entry's age | DS docs; the optional age gauge in the relay and chart | Small | None | Needs approval (D5) |
| R4 | After a transient downstream failure, the consumer rereads its own PEL instead of `>` (W9) | DS app (movement-feed) | Medium: it touches every consumer's retry path | Behavioural. It changes ordering and throughput during outages. | Needs approval (D6); low priority now that W2's alert exists |
| R5 | Runbook: a Redis crash-loop on a corrupt AOF (`redis-check-aof --fix appendonlydir/<incr>`, after copying the directory aside) | DS docs | Small | None | Can follow with R3 |
| R6 | At-rest encryption at the node or provider level, justified by Postgres, not Redis | Host and Ranma-Config | Large | Boot-unlock design and a migration | User decision (D3) |
| R7 | Ranma-Config: drop the duplicate `DistantSignalDeadLetterGrowing` and, if the chart alerts are adopted, skip the planned exporter AOF alerts | Ranma-Config | Tiny | None | D4 |

Not recommended:

- TLS inside the node;
- HA, Sentinel or a replica;
- `appendfsync always`: it costs an fsync on every write to close a
  window of about 1 s that only a power loss opens;
- app-level payload encryption;
- backing up Redis.

## Decisions needed from the user

- **D1**: turn on Redis AUTH in production (R1)?
- **D2**: implement R2 (the relay pre-creates the groups at `0` on a fresh
  stream)?
- **D3**: at-rest encryption: check whether the provider encrypts the VM
  disk. If it does not, is a stolen disk or leaked snapshot in scope,
  which would justify LUKS for the whole node?
- **D4**: keep the AOF alerts in the chart (as implemented) and skip the
  Ranma-Config exporter versions?
- **D5**: set a 1-day resolution rule for dead letters, plus an optional
  age alert?
- **D6**: change the consumers' retry behaviour (R4), or leave it with the
  alert?

## Deploy notes (Ranma-Config)

- Nothing in Ranma-Config has to change for this branch. The new alerts
  render wherever `metrics.prometheusRule.enabled` is already on, which it
  is on mine-bringer.
- The new gauges exist only once the new movement-relay image and the
  consumer images are running. The NearFull and lag alerts fall back to
  the old series until then.
- After rollout, check:
  - `distant_signal_redis_aof_last_write_ok` is 1;
  - `distant_signal_movement_relay_stream_pending{group}` reads about 0;
  - `distant_signal_movement_relay_deadletter_length` reads 0.
- If R1 is approved: create the Secret, deploy with `redis.auth.enabled:
  true, requirePass: false`, add `REDIS_PASSWORD` to the redis_exporter
  Deployment in `distant-signal-exporters.yaml` (as a `secretKeyRef` to the
  same Secret), and then deploy with `requirePass: true`. INFO, XINFO and
  XLEN need nothing beyond the default user's `+@all`. If ACLs are ever
  narrowed, the relay needs `INFO`, `XINFO` and `XLEN` on its user.
