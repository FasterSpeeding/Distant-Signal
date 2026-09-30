# Backup and observability gaps on mine-bringer — design

Status: design only. Nothing here is applied, and no repository other than
this document's own was edited.
Date: 2026-09-30.

## Context

Distant Signal (DS) and the Distant-Signal-MCP run on one k3s node,
`mine-bringer`, deployed by Flux from Ranma-Config. The deploy session's
read-only survey of the live cluster (30 Sep 2026) and the DS architecture
page list these gaps:

1. Nothing backs up the DS Redis, the MCP's `ds-mcp-redis`, the MCP's SQLite
   timetable store, or the schedulefeed volume.
2. Postgres has only a nightly logical dump: no point-in-time recovery (PITR).
3. The cold archive is staged but off. It has no egress NetworkPolicy to
   Thoth, and Thoth has no lifecycle API.
4. Prometheus keeps its TSDB on an `emptyDir`.
5. There is no log aggregation.
6. The backup CronJobs have no `timeZone`. Their comments say 02:00 UTC, but
   the node runs at UTC+2, so the DS dump actually runs at 00:00 UTC.

This document gives a recommendation for each gap, says which repository
changes, estimates the cost, sets out restore and verification steps
(including a drill), and lists the risks. The items are in priority order.

### Sources

- The deploy session's live-cluster write-up of mine-bringer (30 Sep 2026)
  and the DS architecture page built from it.
- This repository: `charts/distant-signal` (`postgresql.*`, `redis.*`,
  `scheduleFeed.*` and `archive.*` values, and the Postgres, Redis and
  schedulefeed templates), `docs/cold-archive.md`,
  `docs/full-coverage-consumer.md` and `docs/personal-data-retention.md`.
- Ranma-Config (read only): `clusters/mine-bringer/apps/distant-signal.yaml`
  (the backup CronJob and the staged archive values),
  `netpol-distant-signal.yaml`, `netpol-monitoring.yaml`, `monitoring.yaml`,
  and `docs/specs/{distant-signal-postgres-pitr,distant-signal-cold-archive,backup-encryption,active-monitoring}.md`.
- Distant-Signal-MCP (read only): `src/oauth/store.ts`, `README.md` ("The
  timetable store") and `charts/*/values.yaml`.

### Facts this design depends on

| Fact | Value | Source |
|---|---|---|
| Node | 1 node, 16 cores, 62.8 GiB. About 98% of CPU is already requested. About 1 TB free on the root filesystem. | PITR spec, live |
| Default StorageClass | `local-path`. It is a directory on the root filesystem: sizes are **not enforced** and volumes cannot be expanded. | write-up; local-path README |
| Where the DS dump runs | The `distant-signal-postgres-backup` CronJob lives in **Ranma-Config**, not in the DS chart. The DS chart has no CronJobs. | `apps/distant-signal.yaml`; architecture page §3 |
| Backup destination | Thoth S3 under `<bucket>/mine-bringer/backups/<job>/`, reached through static GeeseFS PVs. Output is piped through `age -R`, and the age identity is kept offline. Retention is 7 days, and the newest file is never pruned. | write-up §7; `backup-encryption.md` |
| Thoth | It is S3 served by `tailscale serve` on the node host itself, and pods reach it through the node's tailnet address on one TCP port. It uses one shared key with no bucket or prefix scoping. There is no lifecycle API (`GET ?lifecycle` returns 501). ListObjects V1 is not implemented; V2 works. | write-up §1 and §9; cold-archive spec |
| DS Postgres | Postgres 16.15, the chart's own single-replica StatefulSet. About 6.5 GB (6.6 GB measured 26 Sep; the backup comment says about 6.4 GB on 27 Sep). | PITR spec; backup CronJob comment |
| WAL rate | About 35 GB/day raw, measured **before** the chart's current WAL settings (`wal_compression: lz4`, `max_wal_size: 4GB`, `checkpoint_timeout: 15min`). Up to about 90% of it was full-page images. Not re-measured since. | PITR spec; `values.yaml` `postgresql.config` |
| Chart hooks for Postgres | `postgresql.config` (rendered as `-c` args), `postgresql.extraEnv`, and image/digest overrides. There are **no** `extraVolumes` or sidecar hooks. | `templates/postgres-statefulset.yaml` |
| Prometheus | v3.14.0: 30 s scrape and evaluation interval, about 226k–250k head series and about 4.5k samples/s. Retention is 10d or 20 GB, and 10 days is about 5 GB today. Storage is an `emptyDir`. Grafana's DB is also an `emptyDir`. | `monitoring.yaml`; `active-monitoring.md` |
| Container logs | About 150 MB/day across the node, before compression. | `active-monitoring.md` (kubelet log metrics) |

### Cross-cutting findings

These affect several items, so they are listed once here.

- **X1. Is Thoth off-host?** Thoth's front end runs on the same machine as
  the cluster. Earlier specs describe a remote storage box behind it.
  **Confirm with the Thoth operator that objects are stored off this host.**
  If they are not, no backup in this document survives the loss of the
  host, and gap 2 needs a second, truly off-site target before anything
  else. Every drill below includes "check the object exists in Thoth's
  backing store", not just on the endpoint.
- **X2. Keys that exist only on the cluster.** Several secrets that a restore
  needs are either rendered by the chart inside the cluster or sealed with
  the sealed-secrets controller's own key:
  - the Postgres app password, which the chart generates when
    `postgresql.auth.existingSecret` is unset;
  - the schedulefeed SSH host key and push password;
  - any new pgBackRest cipher passphrase.

  If the host is lost, all of these go with it. Anything needed to
  **decrypt** a backup must also be kept offline next to the age identity.
  Anything needed to **reconnect** after a restore must be sealed in git.
  Each item below says which applies.
- **X3. Personal data in backups.** `DELETE /public/account` removes a
  user's rows at once (`docs/personal-data-retention.md`), but a copy stays
  in every backup until that backup expires. That is 7 days today. Every
  retention window proposed here stays at about 7–14 days, and the
  personal-data page should say so.
- **X4. One Thoth key for everything.** Every new writer (pgBackRest, the
  archive) uses the backup key, which can also delete the backups. Every
  design below that deletes objects has a hard-coded prefix guard. Ask the
  Thoth operator for scoped keys whenever that becomes possible.

---

## Priority order

| # | Item | Why this position | Effort |
|---|---|---|---|
| 1 | Postgres PITR (gap 2) | Postgres is the system of record, and the RPO today is about 24 h. It is the only store whose loss can't be rebuilt from upstream. | Medium: a new image, chart values, a CronJob and drills |
| 2 | Explicit `timeZone` on CronJobs (gap 6) | Almost free. Item 1 adds CronJobs, and so might item 3, so fix the convention before adding more. It also moves the dump away from the nightly schedule ingest. | Small |
| 3 | Enable the cold archive safely (gap 3) | Every day it stays off, trains history older than 14–30 days is deleted for good. | Small to medium |
| 4 | Persistent Prometheus (gap 4) | Every Prometheus pod replacement loses the 10 days of history you need to look into an incident. Cheap to fix. | Small |
| 5 | Redis, MCP SQLite, schedulefeed (gap 1) | Mostly **no backup needed**. What's left is sealing one key and writing runbooks. | Small |
| 6 | Log aggregation (gap 5) | Real value, but it's the most new infrastructure, and the alerts added since 27 Sep cover the worst blind spot. | Medium |

---

## 1. Postgres point-in-time recovery

### Recommendation

Use **pgBackRest on the existing StatefulSet**, archiving to a new Thoth
prefix with pgBackRest's own repository encryption. **Keep the nightly
age-encrypted `pg_dump`.** It is independent of pgBackRest, portable across
versions, and encrypted to a key that never touches the cluster.

This reverses the Stage 2 pick in Ranma-Config's earlier PITR spec
(CloudNativePG). The reasons are in the evaluation below. The main ones are
client-side encryption and operational cost on a single node that is
already short of CPU.

### Options evaluated

| | (a) Plain WAL archiving to the existing encrypted destination | (b1) pgBackRest | (b2) WAL-G | (c) CloudNativePG + Barman Cloud |
|---|---|---|---|---|
| Encryption at rest on Thoth | age, with the identity offline (strongest) | `repo1-cipher-type=aes-256-cbc`, with a symmetric passphrase held on the cluster | libsodium (symmetric, on the cluster), or OpenPGP. Whether PGP push works with only the public key needs checking against v3.0.x. | **No client-side encryption.** barman-cloud offers only S3 server-side options, and there's no sign Thoth supports those. WAL and base backups would sit on Thoth unencrypted, readable by anyone holding the shared key. |
| Guard against filling the disk while Thoth is down | None | **`archive-push-queue-max`**: drops WAL, keeps the DB up, and leaves a PITR gap that is reported | None for `archive_command` | None |
| What's in the Postgres pod | age plus a GeeseFS mount or S3 client. The chart has no `extraVolumes`, so it needs a chart change. | A `postgres:16.15` + `pgbackrest` image built by DS CI | A static `wal-g` binary in the image | Replaced by the operator's pods |
| Base backups and retention | Hand-written: `pg_basebackup`, WAL cleanup on S3, `restore_command` decryption | Built in: full, diff and incremental backups, `expire`, `verify`, delta restore | Built in | Built in, declarative |
| Durability of `archive_command` | Hard to get right through GeeseFS, which buffers writes and can report success before the upload lands | Waits for the S3 PUT, and can archive asynchronously in parallel | Waits for the S3 PUT | Waits for the S3 PUT |
| Thoth compatibility | Same as the backups (GeeseFS with `--list-type 2`) | Uses ListObjectsV2. Signs its own payloads, so it avoids the aws-chunked problem. Needs **DeleteObjects** for `expire` (not tested on Thoth). | aws-sdk-go v1 avoids aws-chunked. Delete support not tested. | botocore needs the checksum env vars. DeleteObjects not tested. |
| Migration | None | None: one Postgres restart to set `archive_mode` | None: one restart | Dump and restore into a new Cluster (a maintenance window of about 30–90 min), then repoint `externalDatabase` |
| Standing operational cost | High: bespoke scripts | Medium: one image to rebuild when Postgres gets patched | Medium | Operator, CRDs and upgrades. Worth it only as part of a cluster-wide CNPG decision. |
| Automated restore drills | No: decryption needs the offline identity | **Yes**: the passphrase is on the cluster | Yes | Yes |

**(a) is rejected.** It is the most bespoke option, and the least safe in the
one place that matters: acknowledging a WAL segment before it is durable
loses data without anyone noticing. Its one advantage, the offline key, is
kept anyway by keeping the nightly age-encrypted dump.

**(c) is deferred, not rejected.** Revisit it if Ranma-Config decides on
CNPG cluster-wide (cert-manager is already installed for that) **and**
either Thoth gains encryption at rest or barman-cloud gains client-side
encryption. The DS chart is already ready for it (`postgresql.enabled:
false` plus `externalDatabase.existingSecret`).

**Why (b1) over (b2):** the disk-full guard. On `local-path` with no quota, an
archive that stalls grows `pg_wal` on the node's root filesystem until
Postgres panics and every tenant on the host is squeezed.
`archive-push-queue-max` turns that into a PITR gap that is reported.
pgBackRest is also available as a PGDG package for the exact Debian base
of `postgres:16` (trixie-pgdg), which keeps image patching simple.

**Is PITR worth it at this size?** The database is small (about 6.5 GB), but
it is written constantly (TRUST, LDBWS, schedule publishes) and it holds
user data that can't be rebuilt: accounts, journeys, tracked trains,
groups, push subscriptions. A 24-hour RPO loses a day of user changes.
pgBackRest brings the RPO to about 1–2 minutes for roughly one Postgres
restart and one image.

### Design

**Image (DS repo).** Add `docker/postgres-pgbackrest.Dockerfile`:
`FROM postgres:16.15-trixie@sha256:…`, then `apt-get install pgbackrest=<pinned>`
from the already-configured PGDG repo. CI builds it and publishes it next to
the other images, digest-pinned. Renovate proposes bumps together with the
base image.

**Chart (DS repo, `charts/distant-signal`).** A new opt-in block. It is off by
default, so rendered output doesn't change for other installs.

```yaml
postgresql:
  pgbackrest:
    enabled: false
    image: {}                 # defaults to the postgres-pgbackrest image; digest-pinned
    stanza: ds
    repo:
      s3:
        endpoint: ""          # Thoth
        bucket: ""
        path: ""              # e.g. /mine-bringer/pgbackrest/distant-signal
        region: us-east-1
        uriStyle: path
        existingSecret: ""    # access-key-id, secret-access-key, cipher-pass
      retentionFullType: time
      retentionFull: 7        # days of PITR window (see X3)
    archive:
      async: true
      processMax: 2
      queueMax: 8GiB          # disk-full guard
      timeoutSecs: 60         # archive_timeout
    backup:
      timeZone: Etc/UTC
      fullSchedule: "30 3 * * 0"     # weekly full, Sunday 03:30 UTC
      diffSchedule: "30 3 * * 1-6"   # daily differential
      checkSchedule: "0 5 * * *"     # daily `check` + `verify`
```

When `enabled` is true, the chart:

- swaps the Postgres image for the pgBackRest image. This is the only
  change to the Postgres container, apart from the items below;
- adds these to the rendered `-c` args:
  - `archive_mode=on`
  - `archive_command='pgbackrest --stanza=ds archive-push %p'`
  - `archive_timeout=60`

  `postgresql.config` stays the place for everything else;
- adds `PGBACKREST_*` env for the repo, with `repo1-cipher-type=aes-256-cbc`,
  compression `zstd` at level 3, `repo1-s3-uri-style=path`, and
  `spool-path` on the data PVC beside `PGDATA` (not inside it). The key
  pair and cipher passphrase come in through `secretKeyRef`, and never
  appear in values;
- renders three CronJobs (full, diff and check), each with `timeZone` (see
  item 2). Each one runs `kubectl exec distant-signal-postgres-0 -c postgres
  -- pgbackrest --stanza=ds backup --type=…` (or `check` / `verify`). A
  Role grants `pods/exec` and `get` on **that one pod name**, and nothing
  else;
- adds PrometheusRules (next section).

The exec-based CronJob breaks Ranma-Config's "backup pods mount no
ServiceAccount token" rule (`backup-no-sa-token.yaml`). This is on purpose,
and the grant is narrow. The alternatives are worse:

- A `pgbackrest server` TLS sidecar needs certificates and a new container
  hook in the chart.
- An in-pod scheduler hides the schedule from kube-state-metrics'
  CronJob alerts.

Record the exception in Ranma-Config.

**Ranma-Config.**

- Set `pgbackrest.enabled: true` and the repo values.
- Add the SealedSecret `distant-signal-pgbackrest`. It holds the Thoth key
  pair (the same shared key, see X4) and a freshly generated cipher
  passphrase, generated with `scripts/mint-sealed-secret.sh`.
- **Store the passphrase offline with the age identity** (X2). Without it,
  the repository can't be read after a cluster loss.
- Use a new prefix `<bucket>/mine-bringer/pgbackrest/distant-signal/`, outside
  `backups/` so neither job's pruning can touch the other.
- Add a NetworkPolicy letting the Postgres pod reach Thoth: the node's
  tailnet `/32` on the Thoth port. Reuse the shape of the blackbox
  exporter's rule and of item 3's rule.
- Seal the Postgres app password: set `postgresql.auth.existingSecret`
  (X2, and "Risks" below).

**MCP chart.** No change.

**Postgres settings to confirm before enabling.** Keep the chart's
`wal_compression: lz4`, `max_wal_size: 4GB` and `checkpoint_timeout: 15min`.
They exist to cut the full-page-image share of WAL, and so of what gets
archived.

### Cost

| | Estimate | Basis |
|---|---|---|
| CPU | Archiving: tens of millicores on average, bursting during schedule publishes. Weekly full: 2 processes for about 10–20 min. The CronJob pods request 20m each. No new long-running pod. | pgBackRest zstd-3 on about 6.5 GB |
| Memory | About 50–100 MiB extra in the Postgres container while archiving or backing up. Fits in the existing 5 GiB limit. | – |
| WAL archived | **Re-measure first.** 35 GB/day raw was measured before the current settings. Expect several times less now. Plan on **2–8 GB/day stored** after zstd. | PITR spec measurement; the FPI share |
| Thoth storage, 7-day window | About 15–60 GB of WAL, plus 1–2 fulls of about 2–3 GB, plus 6 diffs → **about 25–75 GB**. | estimates |
| Node disk | A spool of a few MB normally. The worst case is capped at `queueMax` (8 GiB) during a Thoth outage. | – |
| Thoth capacity | **Unknown. Confirm before enabling.** | X1 |

### Rollout

1. **Measure.** Take a 24 h `pg_stat_wal.wal_bytes` delta with the current
   settings. From it, size Thoth storage and confirm capacity with the
   Thoth operator.
2. **Smoke-test Thoth** from a tailnet device with a throwaway prefix. Run
   `stanza-create`, `archive-push`, `backup`, `expire` (this checks
   DeleteObjects) and `restore`. If `expire` fails because DeleteObjects is
   missing, stop. Either get Thoth fixed, or fall back to WAL-G after
   checking its delete path.
3. Ship the image and the chart block (off). Bump Ranma-Config with it still
   off.
4. Turn it on. Postgres restarts once, for `archive_mode`. Then run
   `stanza-create` once via `kubectl exec`, `check`, and a manual full
   backup (`kubectl create job --from=cronjob/…-full`).
5. Watch for 7 days, then run the first drill (below).

### Monitoring

These are new PrometheusRules in the chart, rendered only when pgBackRest is
enabled:

- **Archiver failures:** `pg_stat_archiver` failed count increasing, or last
  archived WAL older than 10 min. Use postgres-exporter's archiver data, and
  enable that collector if it isn't on by default. Warning.
- **Backup not succeeding:** the full, diff or check CronJob hasn't succeeded
  within its window. This is covered by the existing `CronJobNotSucceeding`
  rule; confirm it matches the new job names.
- **Dropped WAL:** when `queueMax` drops WAL, `pg_stat_archiver` still shows
  success. The daily `check`/`verify` job fails if the WAL chain has a gap,
  so rely on that job's failure rather than the archiver metric. Critical.

### Restore and verification

**Point-in-time restore (real incident).**

1. Scale api, aggregator, enricher and notifier to 0, and stop Postgres
   (scale the StatefulSet to 0).
2. Keep the damaged PGDATA: rename the directory, or snapshot it on the
   host.
3. Run a one-off Job using the pgBackRest image, with the data PVC mounted
   and the same env:
   `pgbackrest --stanza=ds --delta --type=time --target="<UTC timestamp>" --target-action=promote restore`.
4. Scale Postgres back to 1. It replays WAL to the target and promotes.
5. Check that the app role's password matches the chart Secret (see Risks).
   Then scale the apps back up. Migrations are a no-op, because the schema
   is at the target point.
6. Run the verification queries below.

**Verification queries (every restore and every drill):**

- `SELECT max(version) FROM _sqlx_migrations WHERE success`: it should match
  the running api.
- The row counts of `users`, `journeys`, `tracked_trains` and `trains`
  should be within the expected change of the source.
- `SELECT max(created_at)` on a few append-mostly tables should be close to
  the target time.
- `SELECT indexrelid::regclass FROM pg_index WHERE NOT indisvalid` should
  return nothing.

**Drill (monthly for the first quarter, then quarterly).** Never touch
production for a drill.

1. A Job starts a scratch Postgres in a pod with an `emptyDir` of about
   2× the database size (about 15 GB).
2. Restore into it with `--type=time` to a timestamp about 1 h in the past.
3. Start it and run the verification queries.
4. Record the RTO: fetch time plus replay time. The earlier spec's estimate
   is 30–90 min. The first drill replaces that estimate with a measurement.

Because the passphrase is on the cluster, this drill can be a CronJob that
runs itself. It is optional, but recommended once the manual drill passes
twice.

**Keep drilling the logical dump too.** Once a quarter, restore the latest
`.sql.gz.age` using the procedure in Ranma-Config's `backup-encryption.md`.
That drill needs the offline identity, so it stays manual. This proves the
independent copy still works.

### Risks

- **The password doesn't match after a physical restore.** A physical restore
  brings back `pg_authid`. If the cluster is rebuilt and the chart generates
  a new Postgres password, the apps can't log in. Mitigation: seal the
  password (`postgresql.auth.existingSecret`) before go-live, or
  `ALTER ROLE … PASSWORD` in step 5 of the restore.
- **The shared key can delete everything** (X4). pgBackRest only deletes under
  `repo1-path` during `expire`, and that path is fixed in values.
- **Thoth is down for longer than `queueMax` allows:** PITR gets a gap until
  the next full backup, and the daily check job alerts. The alternative is
  a Postgres panic, so this is the right trade.
- **Maintaining the custom image:** it must follow `postgres:16.x` patch
  releases. Renovate handles the base digest, and CI rebuilds the image.
- **DeleteObjects or other S3 calls are missing on Thoth.** This is caught in
  rollout step 2, before anything goes live.
- **A longer window keeps personal data longer** (X3). Keep 7 days unless
  there's a reason for more.

---

## 2. Explicit `timeZone` on every CronJob

### Recommendation

- Every CronJob sets `spec.timeZone` explicitly. The field has been GA
  since Kubernetes 1.27, and k3s here is v1.36.
- **Backups and maintenance use `Etc/UTC`,** so there are no DST jumps.
- Jobs tied to the GB rail day use **`Europe/London`**. Keep their schedules
  out of 01:00–02:59 local, where DST transitions skip or repeat runs.
- Never rely on the node's local time. The node's timezone belongs to the
  host, which the cluster doesn't control.

### Which repo

- **Ranma-Config:**
  - the six backup CronJobs (`distant-signal-postgres-backup`,
    `forgejo-postgres-backup`, `forgejo-data-backup`,
    `coder-postgres-backup`, `authentik-postgres-backup`,
    `vaultwarden-sqlite-backup`);
  - the hourly and 6-hourly token and registry CronJobs, for consistency
    (the timezone makes no functional difference to them);
  - fix the "02:00 UTC" comments.
  - **RenovateJob:** its "03:00 node time" is operator-managed. Check
    whether the CRD takes a timezone; if not, document it as node-local.
- **DS chart:** there are no CronJobs today. Every CronJob this design adds
  (item 1, and optionally item 3) takes a `timeZone` value, defaulting to
  `Etc/UTC`, and the chart test asserts it is rendered.
- **MCP chart:** no CronJobs. Nothing to do.

### Proposed schedule, in UTC

| Job | Now (node-local, i.e. actual UTC) | Proposed (`Etc/UTC`) |
|---|---|---|
| distant-signal-postgres-backup | `0 2` (00:00 UTC) | `0 3 * * *` |
| forgejo-postgres-backup | `15 2` (00:15) | `15 3 * * *` |
| forgejo-data-backup | `30 2` (00:30) | `30 3 * * *`. This overlaps the DS pgBackRest full on Sundays; move one of them by 30 min. |
| coder-postgres-backup | `45 2` (00:45) | `45 3 * * *` |
| authentik-postgres-backup | `0 3` (01:00) | `0 4 * * *` |
| vaultwarden-sqlite-backup | `15 3` (01:15) | `15 4 * * *` |
| DS pgBackRest full/diff | – | `30 3 * * 0` / `30 3 * * 1-6`. Move to `0 5` if it collides with the DS dump in practice. |

**Why move the DS dump later:**

- The schedule ingest takes deliveries between 22:00 and 01:30 and runs
  whole-day publishes, which are the heaviest WAL and CPU bursts of the day.
- A dump running then competes with those bursts. It also holds a snapshot
  open, which delays vacuum on the tables being rewritten.
- 03:00 UTC is after that window in both GMT and BST. **Check the ingest
  window's timezone in `schedule-ingest` before finalising.**

File names don't change: the scripts stamp them with `date -u`.

### Cost

None. When a job's schedule changes, its first run happens at the new time,
so the gap between two consecutive dumps grows by 3 h once.

### Verification and drill

1. Run `kubectl get cronjob -A -o custom-columns=NS:.metadata.namespace,NAME:.metadata.name,TZ:.spec.timeZone,SCHED:.spec.schedule`.
   Every row should have a TZ.
2. After the first night, check that each Job's `.status.startTime` is at the
   intended UTC time.
3. **Guard:** add a Ranma-Config CI check (a `yq` query or a conftest policy)
   that fails if any CronJob lacks `timeZone`. Add a chart unit test in DS
   for the same thing.

### Risks

- A typo in the zone name makes the API server reject the CronJob. Flux
  reports this, so it isn't silent.
- Moving schedules can create new overlaps. Use the table above, and look at
  the node CPU panel on the first night.

---

## 3. Cold archive: enable safely

### Recommendation

Turn on the archive that is already built and staged (`archive.enabled`, with
`trains` only and `failurePolicy: retain`). Do it only after three things
are in place:

- **(a)** a narrow egress NetworkPolicy from the aggregator to Thoth;
- **(b)** client-side expiry, because Thoth can't do lifecycle rules;
- **(c)** a decision on how long to keep the archive. The earlier spec
  suggests 2 years.

Do expiry **inside the aggregator**, behind a new `archive.expiry` block.
That way it shares the code that knows the key layout, and it exports
Prometheus metrics like every other retention task.

### (a) Egress rule: Ranma-Config

Add a policy to `netpol-distant-signal.yaml`. It replaces the `TODO(archive)`
comment there.

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: allow-egress-aggregator-thoth
  namespace: distant-signal
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/instance: distant-signal
      app.kubernetes.io/component: aggregator
  policyTypes: [Egress]
  egress:
    - to:
        - ipBlock:
            cidr: <node tailnet address>/32   # same /32 as allow-egress-blackbox-exporter
      ports:
        - protocol: TCP
          port: <thoth port>
```

Notes on the rule:

- **Why it's needed:** `allow-egress-internet` deliberately excludes
  `100.64.0.0/10`, which covers the Thoth address. DNS egress is already
  allowed.
- **Keep the address in one place:** the tailnet address is already
  hard-coded in the blackbox rule. Keep both rules in step, and add a comment
  in each that points to the other.
- **If the tailnet address changes:** both rules break together, and the
  existing `blackbox-thoth` probe alerts. That alert is the canary.
- **Chart (optional, DS):** for other installs, add
  `networkPolicy.archiveEgress: {cidrs: [], ports: []}`. When it's set and
  `archive.enabled` is true, the chart renders the same rule. It isn't
  needed on mine-bringer, where the repo-authored policies are the ones in
  force.

### (b) Expiry done client-side: DS repo

The archiver never deletes, and the chart won't render the archive unless
`archive.s3.lifecycleConfirmed: true` is set. Add expiry:

```yaml
archive:
  expiry:
    enabled: false
    retentionDays: 730        # the policy decision (c)
    minRetentionDays: 90      # hard floor; the chart and binary refuse anything lower
    intervalSecs: 86400
    maxDeletesPerRun: 20000   # circuit breaker
    dryRun: true              # log and count only
```

**How it works:**

- A new task in the aggregator, separate from the prune task, runs once a
  day.
- For each archived table, it lists `<prefix>/<table>/` with ListObjectsV2.
- It deletes an object only if all of the following hold:
  - its key matches **exactly**
    `^<prefix>/(trains|train_movement_events|train_current_state)/service_date=\d{4}-\d{2}-\d{2}/part-\d{19}\.jsonl\.zst$`;
  - its `service_date` from the key, not `LastModified`, is older than
    `today − retentionDays`, in the Europe/London rail day. Taking the date
    from the key follows the backup pruning's rule of never trusting S3
    mtimes;
  - `retentionDays ≥ minRetentionDays`.
- It uses single-object `DELETE`. Thoth's DeleteObjects support hasn't been
  tested, and at about one object per table per batch the volume is small.
- It stops when it reaches `maxDeletesPerRun`, and increments a counter so
  an alert can fire.
- It deletes the oldest dates first.

**Guards against the shared key** (X4):

- **At startup:** the binary refuses to start if the prefix is empty, has
  fewer than two path segments, or equals or contains any prefix listed in
  a new `ARCHIVE_PROTECTED_PREFIXES`. The chart defaults that list to
  nothing, and Ranma-Config sets it to `mine-bringer/backups/` and
  `mine-bringer/pgbackrest/`.
- **Per key:** keys that don't match the pattern are counted and logged,
  never deleted.

**The chart gate changes to:**

> render only if `lifecycleConfirmed || (expiry.enabled && !expiry.dryRun)`

That gate still means someone has decided on expiry.

**Metrics:**

- `aggregator_archive_expiry_objects_deleted_total{table}`
- `aggregator_archive_expiry_candidates{table}` (a gauge; in dry-run it
  counts what *would* be deleted)
- `aggregator_archive_expiry_failures_total`
- `aggregator_archive_oldest_service_date_seconds{table}`
- `aggregator_archive_expiry_unexpected_keys_total`

**Alerts (chart PrometheusRule):**

- failures over 0 in 24 h;
- unexpected keys over 0;
- the oldest service date older than `retentionDays + 7`, which means
  expiry isn't running.

**A useful consequence:** with 730 days of retention, nothing is due for
deletion until 2028. Expiry can ship in `dryRun: true` now, prove that
listing and matching are right (with the candidate count at 0), and be
switched to live well before the first object comes due. The delete path
itself is proved in the drill below, against a scratch prefix.

### (c) Retention decision: user

The archived rows are TRUST-derived, with `raw_body` stripped. The chart
already refuses LDBWS-derived tables and `trust_event_backlog`, so RDM's
300-day ceiling doesn't apply. The project's working assumption is that its
current RDM use is permitted. **Still, choose `retentionDays` on purpose:**
2 years as the earlier spec suggested, or longer. Record the decision in
Ranma-Config's cold-archive spec.

### Cost

| | Estimate |
|---|---|
| Aggregator CPU and memory | Negligible: zstd JSONL batches of up to 1,000 trains during retention runs, plus a daily LIST. Within the current limits. |
| Thoth storage | Tens of MB/day, **a few GB/year**. This is an estimate from `train_movement_events` at about 2.2 GB for a 14–30-day window. Measure it after a week (LIST the total `Size`). |
| Node | None |

### Rollout

1. Do this after item 2, so that the expiry task's day boundary (rail day,
   Europe/London) is settled.
2. **DS:** implement `archive.expiry`, the protected-prefix guard and the
   metrics, with tests. Cut a chart build.
3. **Ranma-Config:**
   - Add the NetworkPolicy.
   - Verify connectivity: start a throwaway pod with the aggregator's
     labels and `curl -sS -o /dev/null -w '%{http_code}' https://<thoth-endpoint>/`.
     Any HTTP status, including 403, proves the path works. A timeout
     means it doesn't.
   - Delete the throwaway pod.
4. **Ranma-Config:** bump the chart, then set:
   - `archive.enabled: true`;
   - `expiry.enabled: true` with `dryRun: true`, `retentionDays: <decision>`;
   - `ARCHIVE_PROTECTED_PREFIXES`.
5. Verify, using the cold-archive spec's section 5:
   - `aggregator_archive_rows_total` and `…_objects_total` rise;
   - `…_upload_failures_total` stays at 0;
   - the prune rate is unchanged;
   - objects appear under `service_date=`.
6. **Before the first object comes due,** set `dryRun: false`.

### Restore, verification and drill

"Restore" here means reading the data back. The app never reads the archive.

- **Monthly spot check:** pick one archived `service_date`. Read it with
  DuckDB, following `docs/cold-archive.md` ("Reading an archive offline").
  Check that `count(DISTINCT id)` for `trains` matches
  `aggregator_archive_rows_total` for that period. Also check X1: that the
  object exists in Thoth's backing store.
- **Expiry drill (once before going live, then after any change to the
  expiry code):**
  1. Point a second aggregator binary, run locally or as a one-off Job, at a
     scratch prefix such as `mine-bringer/distant-signal/archive-drill/`.
  2. Seed it with fake keys dated around the cutoff, plus some keys that
     don't match the pattern, plus one key under a protected prefix.
  3. Run expiry in dry-run mode, then live.
  4. Assert three things: only the expired, matching keys were removed;
     the keys that don't match are counted; and the protected-prefix
     configuration refuses to start.
  5. Delete the scratch prefix.

### Risks

- **The shared key can delete backups** (X4). This is mitigated by the key
  pattern, the protected prefixes, the retention floor and the
  per-run cap.
- **Thoth is unreachable** while `failurePolicy: retain` is set: the trains
  tables grow past their window. The existing
  `DistantSignalArchiveUploadFailures` alert covers this. If it lasts more
  than a few days, turn the archive off rather than switching to `delete`.
- **Getting the rail-day boundary wrong** would delete one day early or late.
  With a 2-year window that doesn't matter, and the day-based floor makes it
  impossible to delete recent data.
- **Egress is too broad:** it isn't. It is one address, one port, and one
  pod.

---

## 4. Persistent Prometheus (and Grafana)

### Recommendation

Put Prometheus on a `local-path` PVC through `prometheusSpec.storageSpec`.
Raise retention from 10 days to **30 days**, with `retentionSize: 25GB` as
the binding cap. At the same time, give Grafana a 1 GiB PVC
(`grafana.persistence`) so that its settings, annotations and user
preferences survive. Dashboards are already provisioned from ConfigMaps.

**Why 30 days:** it covers month-over-month comparisons and a whole incident
review cycle. Longer-term trends already live in Postgres (windowed stats,
daily stats), so a longer TSDB would add cost for little benefit.

### Sizing

| Input | Value |
|---|---|
| Ingest | About 4.5k samples/s, which is about 390M samples/day |
| Observed | 10 days is about 5 GB, which is about 0.5 GB/day, or about 1.3 bytes per sample |
| 30 days | About 15 GB of blocks, plus 1–3 GB of WAL and head chunks, **so about 17–18 GB** |
| Growth room | New exporters were estimated at +5–10k series in total (under 5% of 226k), so they change nothing material |
| `retentionSize` | 25 GB (the cap that wins if series grow) |
| PVC request | 40Gi. local-path doesn't enforce it, so this records intent. It can't be expanded, so size it generously now. |
| Memory | Unchanged. The head block drives memory, not retention. Queries over 30 days use more, and the existing 1.5 GiB request / 4 GiB limit covers that. |
| CPU | Unchanged, apart from slightly longer compactions |

### Which repo

**Ranma-Config** only: `clusters/mine-bringer/apps/monitoring.yaml`.

```yaml
prometheus:
  prometheusSpec:
    retention: 30d
    retentionSize: 25GB
    storageSpec:
      volumeClaimTemplate:
        spec:
          storageClassName: local-path
          accessModes: [ReadWriteOnce]
          resources: {requests: {storage: 40Gi}}
grafana:
  persistence:
    enabled: true
    storageClassName: local-path
    size: 1Gi
```

The DS chart and the MCP chart need no change.

### Rollout

- Adding `storageSpec` makes prometheus-operator recreate the StatefulSet.
  **The emptyDir history is lost one last time.** Do it right after a quiet
  period, not during an incident.
- **Grafana:** its first start on the PVC creates a fresh `grafana.db`. The
  `alertmanagersChoice` sidecar puts back the external Alertmanager setting
  within about 60 s, as designed.
- **Permissions:** Prometheus runs non-root with an `fsGroup`, and
  local-path creates the directory with permissive modes. PSA `restricted`
  isn't affected.

### Verification and drill

- **After rollout:** `prometheus_tsdb_storage_blocks_bytes` grows, and
  `kubectl -n monitoring get pvc` shows the PVCs Bound.
- **Restart drill (right after rollout, and after every chart upgrade):**
  1. Note `prometheus_tsdb_lowest_timestamp_seconds`.
  2. Delete the `prometheus-…-0` pod.
  3. After it restarts, the lowest timestamp should be unchanged, and a
     Grafana panel over the last 7 days should have no gap except the
     restart itself.
  4. For Grafana: star a dashboard, delete the pod, and check the star is
     still there.
- **No TSDB backup.** Metrics aren't a system of record. Before a risky
  upgrade, the operator can take a snapshot (`/api/v1/admin/tsdb/snapshot`
  with the admin API enabled temporarily), but routinely it isn't needed.

### Risks

- **Disk on the shared root filesystem.** `retentionSize` bounds it. Check
  that the node filesystem alert has a threshold that leaves room for
  25 GB.
- **An unclean shutdown corrupts the WAL** (`corruptionCount: 1` was seen
  after the reboots). Prometheus repairs by truncating on start. This now
  loses minutes rather than everything.
- **The PVC can't be expanded:** changing size later means a new PVC and
  another loss of history. That's why the request is 40Gi now.

---

## 5. DS Redis, MCP Redis, MCP SQLite, schedulefeed volume

Each store is judged on what it holds, whether that can be rebuilt, and what
a periodic backup would give back that its persistence doesn't already.

| Store | Holds | Rebuildable? | Backup? |
|---|---|---|---|
| **DS Redis** (7.4, AOF `everysec`, 1Gi PVC) | `movement-events` (about 24 h of TRUST, capped at about 1M entries) and its consumer-group state; `movement-events-deadletter` (up to 10,000, never trimmed); `incident-text-changed` | Partly. Consumers turn `movement-events` into Postgres rows within seconds. The enricher's hourly sweep backstops `incident-text-changed`. Dead letters can't be rebuilt. | **No** |
| **ds-mcp-redis** (7.4, AOF, 96 MB noeviction, 1Gi PVC) | MCP OAuth state only: DCR client records (30 d TTL), grants (30 d), refresh tokens (30 d), access tokens (1 h), login state (10 min). Also Authentik refresh tokens, **stored in plaintext**. | Yes, by users signing in again | **No** |
| **MCP SQLite** (`/data/timetable.sqlite`, 2Gi PVC) | Schedules parsed from a full CIF extract. The ingest keeps a `.prev` copy for rollback. | Yes, by re-ingesting a CIF extract | **No** |
| **schedulefeed PVC** (5Gi) | Up to 3 CIF deliveries, 3 CORPUS extracts plus the rejected ones, and `incoming/`. SFTPGo's user DB is not on it: that is rebuilt at every start. | Yes. The provider pushes a full extract daily, and Postgres already holds everything derived from it. | **No**. But **seal the SSH host key and push password** (below). |

### DS Redis: no backup

**What a nightly snapshot could and couldn't give back:**

- **It can't give back the stream.** A snapshot that is up to 24 h old,
  restored after a loss, would put back entries the consumers already
  processed. Consumer-group positions would be stale, and the consumers
  would re-POST events. A snapshot also can't give back what matters after
  a loss: the entries written since the snapshot.
- **Postgres already holds what matters.** The durable product of the stream
  is Postgres rows, which item 1 protects.
- **The loss is bounded.** The AOF with `appendfsync everysec` limits a
  crash to about 1 s of writes. That persistence was added after the
  2026-09-04 incident, and it is the right protection here.
- **Dead letters shouldn't be copied.** They carry TRUST messages. The
  project keeps raw TRUST for only 1 day (`trust_event_backlog`), and the
  archive strips `raw_body`, so copying dead letters into 7-day backups
  would work against that licensing safeguard.

**What to do instead (each is a small change, noted by repo):**

- **Alerts, Ranma-Config:** alert on `redis_aof_last_write_status != 1` and
  `redis_aof_last_bgrewrite_status != 1` from the existing redis-exporter.
  These catch the case where persistence has failed silently.
- **Loss runbook, DS `docs/movement-events-deadletter.md`:** if Redis state
  is lost, do the following:
  1. Restart `trust-consumer`, `full-coverage-consumer` and
     `trust-backlog-consumer`. They recreate their groups at startup with
     `XGROUP CREATE … MKSTREAM`.
  2. Expect the current day to be marked partial in full coverage. That is
     already the documented behaviour.
  3. Accept that the dead letters are gone.
- **Replaying from Kafka (unverified):** `movement-relay` commits Kafka
  offsets only after `XADD`. If the RDM topic keeps data longer than the
  outage, resetting the consumer group's offsets could replay the gap.
  **Check the topic's retention with RDM before relying on this.** A replay
  also re-delivers entries the consumers already handled.
- **If the DS Redis moves to Valkey,** don't copy the AOF or RDB across until
  you've checked that the target Valkey version can load Redis 7.4's RDB
  format, because the AOF base file is RDB-format. The safer path needs no
  backup at all:
  1. Stop `movement-relay`, so Kafka holds its position.
  2. Let the consumers drain until pending is 0.
  3. Swap the server, then restart the consumers so they recreate their
     groups.
  4. Start the relay.

  This loses only the dead letters. Export them first with `XRANGE` to
  the operator's own machine if they're wanted, and delete that copy
  within a day.

**Drill (after every Redis image bump):**

1. Record the length of `movement-events` and each group's `last-delivered-id`
   (`XINFO STREAM` / `XINFO GROUPS`).
2. Run `kubectl rollout restart deploy/distant-signal-redis`.
3. Check the length and group positions are preserved, and that the
   consumers' lag metrics return to baseline within minutes.

### ds-mcp-redis: no backup

**What losing it costs:** every connected Claude or MCP client has to sign in
again. Their access and refresh tokens are gone, and clients registered
through DCR get `invalid_client` and have to register again. Clients using
CIMD re-resolve by themselves. It costs users one sign-in, with no data
loss.

**Why a backup would be a net harm:** the store holds **plaintext Authentik
refresh tokens** (audit SEC-3). Copying them into a 7-day backup set on a
shared-key Thoth prefix would widen who can see them for no real recovery
benefit.

**What to do instead:**

- **Loss runbook, MCP repo:** a short section in the MCP README ("If the
  OAuth Redis is lost") covering what users see and what to tell them.
- **Alerting, Ranma-Config:** the existing `DistantSignalMcp*Sustained`
  Redis-errors alert covers outages. Optionally add a redis_exporter sidecar
  to the hand-managed `apps/ds-mcp/redis.yaml` to get AOF status alerts like
  DS's.

**Drill:** restart the Redis pod, then check that `DBSIZE` is unchanged and
that an already-connected client still works.

### MCP SQLite: no backup

**How to rebuild it:** the store is built from a full CIF extract by a manual
ingest in the pod, which checks record-count floors and keeps a `.prev`
copy. The DS schedulefeed PVC always holds the latest `timetable_full.zip`
from the daily push.

**What losing it costs:** until someone re-ingests, `find_services` and the
local `plan_journey` fallback report that no timetable has been ingested.
Nothing crashes.

**What to do instead:**

- **Runbook, MCP repo, `README.md` "The timetable store":** copy the newest
  delivery from the schedulefeed pod to the operator's workstation, then
  `kubectl cp` it into the MCP pod and run the documented ingest. **First
  check that the DTD `timetable_full.zip` is the same full-CIF format the
  ingest expects (RJTTF).**
- **Optional, later, MCP chart:** a weekly ingest Job would remove the manual
  step. It is out of scope here.

**Drill (quarterly, 10 minutes):** run the ingest from the latest delivery
into a scratch path (`TIMETABLE_DB_PATH=/tmp/drill.sqlite`), and check that
the record-count floors pass. This proves the rebuild path works without
touching the live file.

### schedulefeed PVC: no backup, but seal the key

**Why the volume doesn't need a backup:** the data on it can be re-delivered
(a daily full push), and everything derived from it is in Postgres.

**What can't be re-delivered:** the SFTP server's **SSH host key** and the
**push account's password**. Both are in a Secret the chart renders and
preserves with `lookup`. If the cluster is rebuilt, the chart generates new
ones, and the provider's push client rejects the new host key until the
provider's operators re-confirm it. That dependency is outside our control
and slow.

**Action:**

1. Check whether the sealed prod values already set
   `scheduleFeed.sftp.existingSecretHostKey` and an existing password
   Secret.
2. If they don't, seal both into a Ranma-Config SealedSecret, point the
   values at it, and keep an offline copy of the host key's fingerprint
   (X2).

**Cost:** none at runtime.

**Drill:** after the change, the host key fingerprint the server shows
(`ssh-keyscan -p <nodeport>` from outside) must match the recorded one.

---

## 6. Log aggregation

### Recommendation

Use **Loki in single-binary mode**, with filesystem storage on a `local-path`
PVC, and **Grafana Alloy** as the collector. Keep logs for **7 days**. Add
Loki as a Grafana data source, and use the Loki ruler for log-based alerts
into the existing Alertmanager. This is the phase 3 that Ranma-Config's
`active-monitoring.md` proposes. It follows that spec's resource analysis,
with the changes below.

**Why Loki rather than VictoriaLogs:**

- **Grafana support:** Loki is a built-in Grafana data source. VictoriaLogs
  needs a plugin installed at Grafana start-up, which means another
  version pin and another outbound fetch from the monitoring namespace.
- **Alerting:** the Loki ruler sends to the Alertmanager that already
  exists. VictoriaLogs needs vmalert for that.

VictoriaLogs is lighter: roughly 50–100 MiB less memory, and retention is
a single flag. It is the fallback if memory becomes the constraint. Because
Alloy's `loki.write` can target VictoriaLogs' Loki-compatible push
endpoint, switching wouldn't change the collector.

**Collector mode: API tailing** (`loki.source.kubernetes`), not hostPath
file tailing.

- **For:** it runs as one small Deployment in a `restricted` namespace with
  no host mounts. The file-based mode needs `/var/log/pods` as a hostPath,
  which only a `privileged` namespace allows.
- **Against:** it adds a log stream per container through the API server
  (about 80 containers here, so fine), and it can miss or duplicate a few
  lines around a collector restart.
- **Fallback:** if either of those matters, switch to file tailing in a
  dedicated privileged namespace. That's the same pattern as
  `monitoring-node-exporter`, with a read-only hostPath mount.

### Which repo

**Ranma-Config:**

- HelmReleases for `loki` (single-binary; gateway, caches and canary off)
  and `alloy`, in a new `logging` namespace with PSA `restricted`.
- Alloy's RBAC: `get`/`list`/`watch` on pods and `get` on `pods/log`,
  cluster-wide.
- NetworkPolicies:
  - Alloy to the API server and to Loki `:3100`;
  - Grafana (in `monitoring`) to Loki `:3100`;
  - Prometheus to both components' metrics ports;
  - Loki's egress: DNS only.
- A Grafana data source through kube-prometheus-stack's
  `grafana.additionalDataSources`.
- Ruler rules in a ConfigMap.
- Digest pins for both images.

**DS repo (optional):** a "Logs" row on the DS Grafana dashboard. The
dashboard ConfigMap lives in Ranma-Config, but its JSON is maintained
alongside DS metrics. Plus LogQL examples in the runbooks
(`docs/movement-events-deadletter.md`, `docs/full-coverage-consumer.md`).

**MCP:** no change. Its logs are collected like everything else.

### Configuration outline

- **Labels:** namespace, pod, container, `app.kubernetes.io/component`, and
  node. Never per-request values.
- **Structured metadata:** parse the JSON log `level` and `target` fields
  that the Rust services emit, as structured metadata, not labels.
- **Filtering:** drop kube-probe and health-check access lines at Alloy.
- **Retention:** `limits_config.retention_period: 168h`, with
  `compactor.retention_enabled: true`. Use the TSDB index with
  `schema_config` v13.
- **Ingestion limits:** set to about 3× today's rate, so a log storm is
  throttled rather than filling the disk.
- **Alerts (ruler), for example:**
  - repeated `NOGROUP` from the stream consumers;
  - `panicked at` in any DS container;
  - Authentik token-endpoint failures in poller logs.

### Cost

| | Estimate |
|---|---|
| Memory | Loki 150–250 MiB (request 256Mi, limit 512Mi). Alloy 100–150 MiB (request 128Mi, limit 256Mi). **About 0.3–0.4 GiB in total.** |
| CPU | 50–150m in total. Request 50m for Loki and 30m for Alloy, because the node is almost fully requested. No CPU limits. |
| Disk | About 150 MB/day raw, and roughly 15–30 MB/day stored after compression. 7 days is **under 0.5 GB**. PVC 10Gi (local-path, not enforced, can't be expanded). |
| Thoth | None. Logs are not backed up (see below). |

### Privacy

Logs can contain personal data: client IPs from `X-Real-IP`, the admin
revoke audit line with an email address or user id, and OIDC subjects.

- **Retention:** 7 days matches the backup window (X3).
- **Access:** Grafana is tailnet-only and gated by `grafana-access`.
- **Documentation:** add a line to `docs/personal-data-retention.md` saying
  that logs are held for 7 days.
- **What not to collect:** don't collect the SFTPGo container's
  authentication debug output at a debug log level.

### Verification and drill

- **After rollout:** in Grafana Explore, `{namespace="distant-signal"}`
  returns lines from every DS component, and
  `sum by (namespace) (rate({namespace=~".+"}[5m]))` shows all namespaces.
  `loki_ingester_chunks_flushed_total` and Alloy's `loki_write_sent_entries_total`
  should both be rising.
- **Crash-log drill** (the reason this exists):
  1. Delete a DS worker pod, for example a poller.
  2. Five minutes later, its previous instance's final lines, including
     shutdown messages, should still be searchable in Loki, even though
     `kubectl logs --previous` no longer has them.
- **Retention drill:** after 8 days, a query for day 1 returns nothing, and
  `loki_compactor_*` metrics show that retention ran.
- **Restart drill:** restart Loki. Querying the time around the restart
  should show no gap in the collected lines, only a short delay. If there
  is a gap, Alloy's retry or WAL isn't set up correctly.
- **No backup of Loki.** Logs are diagnostic and short-lived. Losing the PVC
  loses at most 7 days of history.

### Risks

- **Memory on a node that is heavily requested.** The requests are small,
  but a query storm can push Loki to its limit. The limit and
  `max_query_series` or `max_query_length` settings contain that.
- **The API server is a dependency:** if it's down, no logs are collected.
  Alerts already cover API-server health, and falling back to file tailing
  is an option.
- **Label cardinality:** control it with the label list above. A mistake
  shows up as a rising `loki_ingester_memory_streams`, so alert on it.
- **Sensitive data in logs:** see Privacy. Review before widening Grafana
  access.

---

## Summary of changes by repo

| Repo | Changes |
|---|---|
| **Distant-Signal (this repo)** | `postgres-pgbackrest` image and CI; `postgresql.pgbackrest.*` chart block (env, `-c` args, 3 CronJobs with `timeZone`, pod-scoped exec Role, PrometheusRules); `archive.expiry.*` in the aggregator (task, key-pattern and protected-prefix guards, metrics) and chart (values, gate change, alerts); optional `networkPolicy.archiveEgress`; chart tests requiring `timeZone`; runbook additions (Redis loss, dead letters); a line in `docs/personal-data-retention.md` about backups and logs |
| **Ranma-Config** | pgBackRest values, SealedSecret, Thoth egress for Postgres; seal the Postgres password; `timeZone` plus new schedules on all CronJobs, a CI check, and the fixed comments; the aggregator Thoth egress policy; archive enable plus expiry values and protected prefixes; Prometheus `storageSpec`/30d/25GB and Grafana persistence; AOF alerts; the schedulefeed host-key/password SealedSecret; Loki, Alloy, the `logging` namespace, policies and the data source |
| **Distant-Signal-MCP** | README runbooks only (OAuth Redis loss, timetable rebuild from the DS delivery). No chart change is required. |

## Open questions for the user

1. **Thoth:** is it off-host (X1), and how much capacity does it have for
   about 25–75 GB of pgBackRest data? Can the Thoth operator mint keys
   scoped to a prefix?
2. **Cold-archive retention:** 730 days, or some other value?
3. **Accepting pgBackRest over CNPG,** as a reversal of the earlier Stage 2
   recommendation, on the grounds of encryption and cost.
4. **The exec exception:** a CronJob that mounts a ServiceAccount token
   (pods/exec on one pod) for the pgBackRest schedule.
5. **Log retention:** 7 days, and whether collection should cover all
   namespaces or only `distant-signal` and `ds-mcp`. Covering fewer
   namespaces roughly halves Loki's footprint.
