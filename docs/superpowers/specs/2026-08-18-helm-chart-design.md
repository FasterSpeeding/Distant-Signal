# Helm Chart for Kubernetes Deployment — Design

Package the whole nr-status stack as a single self-contained Helm chart at
`charts/nr-status`, so that

```sh
helm install nr-status ./charts/nr-status -n nr-status --create-namespace
```

brings up Postgres, the api, the aggregator and the frontend into one
namespace with no prior cluster setup and no external chart repositories.

The stack is currently only deployable via `docker-compose.yml` (prod/dev
profiles). This spec translates that topology to Kubernetes; it does not
change any application code.

## Goals

- One chart, one namespace, one command. No subchart dependencies, no
  `helm dependency update`, no operator prerequisites — installable in an
  air-gapped cluster given the images.
- Deploy the chart's dependencies (PostgreSQL) alongside the app, while
  leaving a clean path to an external/managed database.
- Preserve the service topology, environment contract and cadences that
  `docker-compose.yml` and `.env.example` already establish, so the two
  deployment paths do not drift.
- Keep secret material out of rendered Deployment specs, and support
  externally-managed secrets (External Secrets Operator, Vault, SOPS).
- Default to a configuration that actually works on a fresh install, given
  that the four Rail Data Marketplace feeds still have no confirmed
  endpoints.

## Non-goals

- **No image build or publish pipeline.** The six Dockerfiles under
  `docker/` and `frontend/Dockerfile` build locally only; there is no
  registry push today. The chart assumes images already exist at the
  configured refs. The chart README documents a manual build/push loop.
- **No HorizontalPodAutoscaler.** The aggregator and all four pollers are
  singleton loops that must not be scaled, and the api is database-bound —
  an HPA would imply a scaling story the system does not have.
- **No backup, restore or replication for the bundled Postgres.** It is a
  single-replica StatefulSet on a PVC. Anyone needing HA should set
  `postgresql.enabled: false` and point at a managed database.
- No changes to application code, Dockerfiles, or `docker-compose.yml`.
- No ServiceMonitor / metrics wiring — the services expose no metrics
  endpoint today.

## Current state

`docker-compose.yml` defines seven services (each in a `prod` and a `dev`
variant except postgres):

| Service | Port | Needs | Notes |
|---|---|---|---|
| `postgres` | 5432 | — | `postgres:16`, named volume, `pg_isready` healthcheck |
| `api` | 8080 | `DATABASE_URL`, `BIND_URL`, `INTERNAL_TOKEN` | runs `sqlx::migrate!()` in-process at startup |
| `poller-incidents` | — | `RDM_INCIDENTS_BASE_URL`, `RDM_API_KEY`, `INTERNAL_TOKEN`, `POLL_INTERVAL_SECS`, `API_INGEST_URL` | 300s cadence |
| `poller-stations` | — | `RDM_STATIONS_BASE_URL`, + same | 86400s cadence |
| `poller-tocs` | — | `RDM_TOCS_BASE_URL`, + same | 86400s cadence |
| `poller-ldbws` | — | `LDBWS_BASE_URL`, + same, plus `NUM_ROWS`, `API_SAMPLE_STATIONS_URL` | 60s cadence |
| `aggregator` | — | `DATABASE_URL`, `LINES_DIR`, `POLL_INTERVAL_SECS`, `HISTORY_RETENTION_DAYS` | talks to Postgres directly, not to the api |
| `frontend` | 3000 | `API_BASE_URL` | Next.js; `API_BASE_URL` is read at request time, not baked at build |

Three facts from the code that shape this design:

1. **`crates/api/src/routes/mod.rs`** deliberately mounts the four
   line-status endpoints at *unprefixed* top-level paths
   (`GET /Line/Mode/national-rail/Status`, `GET /StopPoint/{crs}/Disruption`)
   rather than under `/public`, so clients built against TfL's own API work
   unchanged. Only `health`, `freshness`, `lines`, `preferences` and
   `reference` sit under `/public`. `/private/*` is guarded by the
   `X-Internal-Token` header (`crates/api/src/auth.rs`).
2. **`crates/api/src/data/config.rs`** requires a single `DATABASE_URL`
   string; there is no host/user/password-parts form. `INTERNAL_TOKEN` must
   be non-empty or the api refuses to start.
3. **`.env.example`** documents that every `RDM_*_BASE_URL` and
   `LDBWS_BASE_URL` is a deliberately non-functional `*.example.invalid`
   placeholder — no confirmed endpoint exists for any of the four feeds.

There is no Kubernetes or Helm configuration in the repository today.

## Chart layout

```
charts/nr-status/
  Chart.yaml                    # no dependencies; appVersion tracks the repo
  values.yaml                   # commented, mirroring .env.example's structure
  values-example.yaml           # a filled-in "real deployment" example
  README.md                     # install/upgrade/values reference
  templates/
    _helpers.tpl                # names, labels, DATABASE_URL assembly, secret lookup
    NOTES.txt
    serviceaccount.yaml
    secret.yaml
    postgres-statefulset.yaml
    postgres-service.yaml
    api-deployment.yaml
    api-service.yaml
    aggregator-deployment.yaml
    frontend-deployment.yaml
    frontend-service.yaml
    poller-deployments.yaml     # a single `range` over all four pollers
    ingress.yaml
    networkpolicy.yaml          # optional, default off
    tests/test-api-health.yaml  # `helm test` probes /public/health
```

## PostgreSQL

A `StatefulSet` with `replicas: 1` (not configurable — there is no
replication story), a headless `Service`, and a `volumeClaimTemplates`
entry. Gated on `postgresql.enabled`, default `true`.

Carried over from compose: the `postgres:16` image, and
`pg_isready -U $POSTGRES_USER -d $POSTGRES_DB` as both the readiness and
liveness probe.

Two Kubernetes-specific requirements that compose does not have:

- **`PGDATA` must be a subdirectory of the mount.** Mounting a PVC directly
  at `/var/lib/postgresql/data` fails `initdb` on CSI drivers that create a
  `lost+found` entry in a fresh volume. The chart sets
  `PGDATA=/var/lib/postgresql/data/pgdata` and mounts the claim at
  `/var/lib/postgresql/data`.
- **`fsGroup: 999`** on the pod security context, so the `postgres` user in
  the image can write the mounted volume.

Values: `postgresql.image.*`, `postgresql.auth.{username,database,password,
existingSecret,existingSecretPasswordKey}`, `postgresql.persistence.
{enabled,size,storageClass,accessModes,existingClaim}`, plus `resources`,
`nodeSelector`, `tolerations`, `affinity`.

When `postgresql.enabled: false`, the `externalDatabase` block takes over
(see below) and no Postgres objects are rendered.

## Assembling DATABASE_URL without leaking the password

Both `api` and `aggregator` need one `DATABASE_URL` string that contains the
password. Rendering that string into a Deployment's `env[].value` would
expose it to anyone with `get deployments` — a strictly wider audience than
`get secrets`.

Instead the chart relies on Kubernetes' own `$(VAR)` env-var interpolation,
which the kubelet expands at container start from earlier entries in the
same container's `env` list:

```yaml
env:
  - name: PGPASSWORD
    valueFrom:
      secretKeyRef:
        name: <secret>
        key: postgres-password
  - name: DATABASE_URL
    value: "postgres://nr_status:$(PGPASSWORD)@nr-status-postgres:5432/nr_status"
```

The password lives only in the Secret. This also makes
`postgresql.auth.existingSecret` work without the chart ever needing to read
the password value itself.

**Documented caveat:** a password containing URL-reserved characters
(`@ : / ? # [ ] %`) must be percent-encoded by the operator, because the
chart cannot encode a value it never sees. `values.yaml` states this, and
generated passwords use `randAlphaNum` (alphanumeric only) so the default
path is never affected.

The `secretKeyRef` above resolves to `postgresql.auth.existingSecret` and
`postgresql.auth.existingSecretPasswordKey` when those are set, and to the
chart-rendered Secret otherwise — the api and aggregator templates read the
same resolved reference the Postgres StatefulSet does, so the two can never
disagree about which Secret holds the password.

When `postgresql.enabled: false`, no `PGPASSWORD` env var is injected at
all; `DATABASE_URL` instead comes whole from `externalDatabase.url`
(literal) or from `externalDatabase.existingSecret` +
`externalDatabase.existingSecretUrlKey` as a direct `secretKeyRef`, which is
the preferred form. Setting neither while `postgresql.enabled` is `false`
calls `fail`.

## Secrets

One chart-rendered `Secret` holds:

| Key | Consumer |
|---|---|
| `postgres-password` | postgres, api, aggregator |
| `internal-token` | api, all four pollers |
| `rdm-incidents-api-key` | poller-incidents |
| `rdm-stations-api-key` | poller-stations |
| `rdm-tocs-api-key` | poller-tocs |
| `rdm-ldbws-api-key` | poller-ldbws |

Every value additionally accepts an `existingSecret` + key override, so a
production install can leave all values empty and point at a Secret managed
by External Secrets Operator, Vault or SOPS. Keys whose value is supplied by
an `existingSecret` are omitted from the chart-rendered Secret entirely.

`postgres-password` and `internal-token` auto-generate via `randAlphaNum 32`
when left empty and no `existingSecret` is given. Generation uses the
**lookup-preserve** pattern:

```
{{- $existing := lookup "v1" "Secret" .Release.Namespace $name -}}
{{- $pw := (get ($existing).data "postgres-password" | b64dec) | default (randAlphaNum 32) -}}
```

so `helm upgrade` reuses the live value rather than rotating a password out
from under a running Postgres volume. This is the single most important
correctness detail in the Secret template: without it, every upgrade breaks
the database connection.

Known limitation, stated in the README: `lookup` returns empty during
`helm template` and `--dry-run`, so rendering offline shows a *different*
generated password each time. This is cosmetic for dry runs but means
`helm template | kubectl apply` is not a supported install path when relying
on generated secrets. Set explicit values or use `existingSecret` for that
workflow.

## Workloads

All workloads share: `runAsNonRoot: true`, `allowPrivilegeEscalation: false`,
`capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`,
`automountServiceAccountToken: false` (nothing in this stack talks to the
Kubernetes API server), and per-workload `replicaCount`, `resources`,
`nodeSelector`, `tolerations`, `affinity`, `podAnnotations` and
`podSecurityContext` values.

Every runtime image already declares a non-root `USER`, so `runAsNonRoot`
is satisfied without the chart pinning a `runAsUser` UID.

### api

`Deployment` + ClusterIP `Service` on 8080.

- `startupProbe` on `GET /public/health` with a generous `failureThreshold`
  (default 30 × 2s = 60s), because `sqlx::migrate!()` runs in-process before
  the listener binds. `readinessProbe` and `livenessProbe` then use the same
  path with normal timings.
- `replicaCount` defaults to `1`. Values.yaml documents that >1 is safe:
  sqlx's `Migrator` takes a Postgres advisory lock, so concurrent startups
  serialise their migrations rather than racing.
- Env: `BIND_URL=0.0.0.0:8080`, `DATABASE_URL` (as above), `INTERNAL_TOKEN`
  from the Secret, `RUST_LOG`. `LINES_DIR` is left at the image default
  (`/app/lines`, baked in by `docker/api.Dockerfile`).
- `readOnlyRootFilesystem: true`.

### aggregator

`Deployment`, `replicas: 1` (fixed, not a value), `strategy: Recreate`.

It is a singleton write loop against Postgres; a rolling update would
briefly run two copies, double-writing `line_status` and racing the history
prune. `Recreate` prevents that.

- No probes — the binary exposes no HTTP surface. Failure handling is
  restart-on-exit via the default `restartPolicy`.
- Env: `DATABASE_URL`, `LINES_DIR=/app/lines`, `POLL_INTERVAL_SECS`
  (default 60), `HISTORY_RETENTION_DAYS` (default 7), `RUST_LOG`.
- `readOnlyRootFilesystem: true`.

### pollers (×4)

A single `poller-deployments.yaml` template `range`s over a values map, so
all four share one implementation. Each entry carries its own `enabled`,
`image`, `baseUrl`, `baseUrlEnvVar`, `ingestPath`, `pollIntervalSecs`,
`apiKey`/`existingSecret`, and standard scheduling knobs.

Per-poller env-var names differ and are encoded in the values map:

| Poller | Base URL var | Ingest path | Default cadence |
|---|---|---|---|
| incidents | `RDM_INCIDENTS_BASE_URL` | `/private/incidents` | 300 |
| stations | `RDM_STATIONS_BASE_URL` | `/private/stations` | 86400 |
| tocs | `RDM_TOCS_BASE_URL` | `/private/tocs` | 86400 |
| ldbws | `LDBWS_BASE_URL` | `/private/station-samples` | 60 |

All four carry `RDM_API_KEY` from the Secret, `INTERNAL_TOKEN`,
`POLL_INTERVAL_SECS`, `RUST_LOG`, and `API_INGEST_URL` pointing at the
in-cluster api Service. `ldbws` additionally sets `NUM_ROWS` (default 10)
and `API_SAMPLE_STATIONS_URL`.

**All four are disabled by default**, because no confirmed RDM endpoint
exists for any of them (`.env.example`). A default install therefore brings
up postgres + api + aggregator + frontend and works immediately, rather
than four pods logging connection failures against `*.example.invalid`.

Enabling a poller without setting its `baseUrl` calls Helm's `fail`, so the
render aborts with a clear message instead of deploying a pod that cannot
work.

`readOnlyRootFilesystem: true` on all four.

### frontend

`Deployment` + ClusterIP `Service` on 3000.

- Env: `API_BASE_URL` → the in-cluster api Service
  (`http://<release>-api:8080`). It is read at request time in Server
  Components, so no build-time value is needed.
- Readiness and liveness probe `GET /` on 3000. Next.js ships no dedicated
  health route; the path is a value so it can be changed if one is added.
- `readOnlyRootFilesystem: false`, because `next start` writes its
  incremental cache under `.next/cache`. This is the one workload that
  cannot take a read-only root, and the template carries a comment saying
  why.

## Ingress

A single `Ingress` resource with up to two host entries, both optional and
independently toggleable.

| Value | Backend | Path |
|---|---|---|
| `ingress.frontend.host` | frontend Service :3000 | `/` |
| `ingress.api.host` | api Service :8080 | `/` |

**Separate hostnames, not path-splitting one host.** The api serves
TfL-compatible routes at unprefixed top-level paths (`/Line/…`,
`/StopPoint/…`) as well as `/public/…`, so any attempt to split a single
host by path prefix would either collide with Next.js's own routes or break
the TfL-shape compatibility that `routes/mod.rs` calls out as load-bearing.

`ingressClassName`, arbitrary `annotations` and a `tls` block (shaped for
cert-manager's `cluster-issuer` annotation, but issuer-agnostic) are all
pass-through values.

**Security note, stated in both `values.yaml` and `NOTES.txt`:** exposing
`ingress.api.host` publicly also exposes `/private/*`, which is protected
only by the `X-Internal-Token` shared secret. Operators who do not need
external API access should leave `ingress.api.enabled: false`.

## NetworkPolicy

Optional, `networkPolicy.enabled: false` by default (many clusters run no
CNI that enforces it, and a silently-unenforced policy is worse than an
absent one).

When enabled it renders default-deny ingress per workload plus explicit
allows:

- postgres ← api, aggregator only
- api ← frontend, the four pollers, and — when `ingress.api.enabled` — the
  namespace selector configured in `networkPolicy.ingressControllerNamespace`
- frontend ← the same ingress-controller selector

Egress is left unrestricted: the pollers must reach arbitrary external RDM
hosts, and constraining that would require operators to enumerate them.

## Verification

- `helm lint charts/nr-status` clean.
- `helm template` renders without error for: defaults; all pollers enabled
  with base URLs; `postgresql.enabled: false` + `externalDatabase.url`;
  ingress enabled with TLS; `networkPolicy.enabled: true`.
- `helm template` with a poller enabled and no `baseUrl` **fails** with the
  intended message.
- Rendered output passes `kubeconform` (or `kubectl apply --dry-run=client`)
  against a current Kubernetes schema.
- Grep the rendered default output to confirm no secret value appears
  outside the Secret object.
- `helm test` (the `/public/health` probe pod) is defined; running it
  requires a live cluster with the images available, which is an operator
  step rather than a chart-authoring one.

A live end-to-end install is explicitly **not** part of the acceptance
criteria for this work, since no published images exist yet.
