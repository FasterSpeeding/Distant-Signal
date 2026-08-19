# Helm Chart for Kubernetes Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package the whole nr-status stack (Postgres, api, aggregator, frontend, four optional pollers) as a single self-contained Helm chart at `charts/nr-status`, installable with one command into one namespace with no external chart repositories.

**Architecture:** One chart, no subchart dependencies. A `_helpers.tpl` owns every name, label set, image ref and — critically — the *single* resolved `secretKeyRef` for the Postgres password and the internal token, so the Postgres StatefulSet and its api/aggregator consumers can never disagree about which Secret holds what. `DATABASE_URL` is assembled with Kubernetes' own `$(VAR)` env interpolation so the password never appears in a Deployment spec. The four pollers share one `range`-driven template fed by a values map that carries their differing base-URL env-var names and ingest paths.

**Tech Stack:** Helm (installed here: **v4.1.4**), Kubernetes manifests (apps/v1, networking.k8s.io/v1), `kubeconform` v0.7.0 and `kubectl` v1.35.4 for rendered-output validation. No application code, Dockerfile or `docker-compose.yml` changes anywhere in this plan.

---

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-18-helm-chart-design.md`. It is the authority on scope. Do not add scope.
- **Explicit non-goals — plan no work for any of these:** no image build/publish pipeline (the README documents a *manual* build/push loop and nothing more), no HorizontalPodAutoscaler, no Postgres backup/restore/replication, no changes to application code / `docker/*.Dockerfile` / `frontend/Dockerfile` / `docker-compose.yml`, no ServiceMonitor or metrics wiring.
- **Everything this plan creates lives under `charts/nr-status/`.** No file outside that directory is created or modified, except this plan file itself.
- Chart name: `nr-status`. Helper prefix: `nr-status.` (e.g. `nr-status.fullname`). Release-scoped names are `{{ .Release.Name }}-nr-status` truncated at 63 chars, per the standard `fullname` helper.
- Component suffixes are fixed and used verbatim by every template and by the NetworkPolicy selectors: `postgres`, `api`, `aggregator`, `frontend`, `poller-incidents`, `poller-stations`, `poller-tocs`, `poller-ldbws`.
- Secret keys are fixed and used verbatim: `postgres-password`, `internal-token`, `rdm-incidents-api-key`, `rdm-stations-api-key`, `rdm-tocs-api-key`, `rdm-ldbws-api-key`.
- Default image repositories (no registry exists yet; these are the names the README's manual build loop tells operators to tag): `nr-status/api`, `nr-status/aggregator`, `nr-status/frontend`, `nr-status/poller-incidents`, `nr-status/poller-stations`, `nr-status/poller-tocs`, `nr-status/poller-ldbws`. Each `image.tag` defaults to `""`, meaning "fall back to `.Chart.AppVersion`".
- Chart `version: 0.1.0`, `appVersion: "0.1.0"` (matches `crates/api/Cargo.toml`'s `version = "0.1.0"`), `kubeVersion: ">=1.23.0-0"` (the NetworkPolicy templates rely on the automatic `kubernetes.io/metadata.name` namespace label, GA in 1.22).
- Every commit in this plan is scoped to `charts/nr-status/`. Commit after every task.

### Tooling available on this machine (verified, not assumed)

Checked at plan-writing time:

| Tool | Present | Version | Notes |
|---|---|---|---|
| `helm` | yes | **v4.1.4** (via mise) | Helm **4**, not 3 — see the Helm 4 note below |
| `kubectl` | yes | v1.35.4 | client only; no cluster is assumed |
| `kubeconform` | yes | v0.7.0 | verified working against stdin, schemas resolve |
| `minikube` | yes | v1.38.1 | **not used by this plan** — no live install is in scope |

All three verification tools are present, so no fallback is required. **If a later executor finds them missing**, the fallbacks in priority order are: (1) `mise use helm@4 kubeconform@0.7` to reinstall, (2) drop the `kubeconform` step and use `kubectl apply --dry-run=client --validate=false -f -` (parse-only, much weaker), (3) if `helm` itself is unavailable, the task's verification cannot be performed and the task must not be marked complete — say so rather than claiming success.

### Helm 4 note — this bites in Task 2

The spec's Secret snippet is written for Helm 3:

```
{{- $existing := lookup "v1" "Secret" .Release.Namespace $name -}}
{{- $pw := (get ($existing).data "postgres-password" | b64dec) | default (randAlphaNum 32) -}}
```

**That exact expression fails under the Helm 4.1.4 installed here.** Verified by rendering it:

```
Error: execution error at (tc/templates/s.yaml:2:32) ...
  wrong type for value; expected map[string]interface {}; got interface {}
```

`lookup` returns an *empty map* when nothing is found, so `$existing.data` is a nil `interface{}`, and sprig's `get` demands a concrete `map[string]interface{}`. The corrected three-line form below was rendered successfully on this machine and is what Task 2 uses:

```
{{- $existing := lookup "v1" "Secret" .Release.Namespace $secretName -}}
{{- $existingData := default (dict) $existing.data -}}
{{- $pw := (get $existingData "postgres-password" | b64dec) | default (randAlphaNum 32) -}}
```

The semantics the spec asks for are unchanged: reuse the live value on `helm upgrade`, generate a fresh one only on first install.

### Discrepancies found between the spec and the actual code — resolutions used here

These were found by reading the code and are called out so the executor does not silently "fix" them in a different direction.

1. **`postgres:16` does not run as a non-root USER.** The spec says "Every runtime image already declares a non-root `USER`, so `runAsNonRoot` is satisfied without the chart pinning a `runAsUser` UID." That is true of all seven *application* images (`docker/api.Dockerfile` → `USER api`, `docker/aggregator.Dockerfile` → `USER aggregator`, the four poller Dockerfiles → `USER poller`, `frontend/Dockerfile` → `USER frontend`). It is **not** true of `postgres:16`, whose default USER is root and which drops to `postgres` via `gosu` inside its entrypoint. Applying a bare `runAsNonRoot: true` to it would fail the pod at admission. **Resolution used in Task 3:** the Postgres pod gets `runAsNonRoot: true` **plus** an explicit `runAsUser: 999`, `runAsGroup: 999`, `fsGroup: 999`. The image runs correctly when it is already the `postgres` uid (999) and skips its own `gosu` step. Postgres also keeps a writable root filesystem (it writes `/var/run/postgresql`); the spec only requires `readOnlyRootFilesystem: true` for api, aggregator and the pollers, so this is a gap-fill, not a contradiction.
2. **`LINES_DIR` is already `/app/lines` by default in both consuming crates.** The spec says the api leaves `LINES_DIR` at the image default but has the aggregator set `LINES_DIR=/app/lines` explicitly. `crates/api/src/data/config.rs` and `crates/aggregator/src/config.rs` both declare `#[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", ...)]`, so the aggregator's explicit value is redundant but harmless. **Resolution:** follow the spec literally — the aggregator sets it, the api does not — and carry a comment in `aggregator-deployment.yaml` saying it matches the binary's own default.
3. **The api's `--defaults-file` flag has no `env` attribute.** In `crates/api/src/data/config.rs`, `defaults_file` is `#[arg(long, value_parser = ..., value_hint = ..., value_name = "FILE")]` with no `env`, so it is CLI-only and *cannot* be set from a Deployment env var. The spec does not mention it, and this plan does not expose it. Anyone wanting it must use a container `args:` override, which is out of scope here.
4. **`INTERNAL_TOKEN` non-emptiness is enforced, and the enforcement is real.** The spec's claim is correct — `crates/api/src/app.rs:28-29` asserts `!config.internal_token.is_empty()` with the message `internal_token (--internal-token / INTERNAL_TOKEN) must not be empty`. This is why Task 2's generation path must never render an empty `internal-token` key.
5. **`API_SAMPLE_STATIONS_URL`'s path is `/private/sample-stations`, and it is separate from the ingest path.** The spec's poller table lists only `ingestPath` per poller and mentions `API_SAMPLE_STATIONS_URL` in prose. Confirmed against `docker-compose.yml` and `crates/poller-ldbws/src/config.rs`: ldbws needs **both** `API_INGEST_URL=…/private/station-samples` **and** `API_SAMPLE_STATIONS_URL=…/private/sample-stations`. The values map in Task 7 carries `sampleStationsPath` as a separate key, present only for ldbws.
6. **Every env var name in the spec's tables matches the code.** Verified one by one against `crates/poller-incidents/src/config.rs` (`RDM_INCIDENTS_BASE_URL`, `RDM_API_KEY`, `API_INGEST_URL`, `INTERNAL_TOKEN`, `POLL_INTERVAL_SECS`), `crates/poller-stations/src/config.rs` (`RDM_STATIONS_BASE_URL`, …), `crates/poller-tocs/src/config.rs` (`RDM_TOCS_BASE_URL`, …), `crates/poller-ldbws/src/config.rs` (`LDBWS_BASE_URL`, `RDM_API_KEY`, `NUM_ROWS`, `API_SAMPLE_STATIONS_URL`, `API_INGEST_URL`, …), `crates/aggregator/src/config.rs` (`DATABASE_URL`, `LINES_DIR`, `POLL_INTERVAL_SECS`, `HISTORY_RETENTION_DAYS`) and `crates/api/src/data/config.rs` (`BIND_URL`, `DATABASE_URL`, `INTERNAL_TOKEN`, `LINES_DIR`). Default cadences also match (300 / 86400 / 86400 / 60 / 60). No corrections needed.
7. **The api's route layout is as the spec describes.** `crates/api/src/main.rs` merges `routes::line_status::router()` at the top level and nests `public_router()` under `/public` and `private_router()` under `/private`. `sqlx::migrate!().run(...)` runs *before* `TcpListener::bind`, which is exactly why the api needs a generous `startupProbe` (Task 4).

---

## File structure

Every file below is created by this plan. Nothing else changes.

```
charts/nr-status/
  Chart.yaml                          Task 1
  values.yaml                         Tasks 1-9 (grown incrementally)
  values-example.yaml                 Task 11
  README.md                           Task 11
  templates/
    _helpers.tpl                      Tasks 1, 2, 4 (grown incrementally)
    NOTES.txt                         Task 10
    serviceaccount.yaml               Task 1
    secret.yaml                       Task 2
    postgres-statefulset.yaml         Task 3
    postgres-service.yaml             Task 3
    api-deployment.yaml               Task 4
    api-service.yaml                  Task 4
    aggregator-deployment.yaml        Task 5
    frontend-deployment.yaml          Task 6
    frontend-service.yaml             Task 6
    poller-deployments.yaml           Task 7
    ingress.yaml                      Task 8
    networkpolicy.yaml                Task 9
    tests/test-api-health.yaml        Task 10
```

---

### Task 1: Chart scaffold, naming/label helpers, ServiceAccount

**Files:**
- Create: `charts/nr-status/Chart.yaml`
- Create: `charts/nr-status/values.yaml`
- Create: `charts/nr-status/templates/_helpers.tpl`
- Create: `charts/nr-status/templates/serviceaccount.yaml`

**Interfaces:**
- Produces, consumed by every later task:
  - `nr-status.name` → chart name, overridable by `.Values.nameOverride`.
  - `nr-status.fullname` → release-scoped base name, overridable by `.Values.fullnameOverride`.
  - `nr-status.chart` → `name-version` for the `helm.sh/chart` label.
  - `nr-status.labels` — takes `(dict "root" $ "component" "api")`, emits the full common label set including `app.kubernetes.io/component`.
  - `nr-status.selectorLabels` — takes the same dict, emits only the two immutable selector labels plus `app.kubernetes.io/component`.
  - `nr-status.serviceAccountName` — takes root (`.`).
  - `nr-status.image` — takes `(dict "root" $ "image" .Values.api.image)`, emits `repository:tag` with tag defaulting to `.Chart.AppVersion`.
  - `nr-status.postgresFullname` / `nr-status.apiFullname` / `nr-status.frontendFullname` — take root, emit `<fullname>-postgres` / `-api` / `-frontend`.
  - `nr-status.apiBaseUrl` — takes root, emits `http://<apiFullname>:<api.service.port>`.
  - `nr-status.podSecurityContext` — takes `(dict "override" .Values.<workload>.podSecurityContext)`, merging that workload's override over the chart-wide defaults.
  - `nr-status.containerSecurityContext` — takes `(dict "readOnlyRootFilesystem" true)`.

- [ ] **Step 1: Create the chart directory and `Chart.yaml`**

```bash
mkdir -p charts/nr-status/templates/tests
```

Create `charts/nr-status/Chart.yaml`:

```yaml
apiVersion: v2
name: nr-status
description: >-
  National Rail status stack: PostgreSQL, the api, the aggregator, the
  frontend and four optional Rail Data Marketplace pollers, mirroring the
  topology of the repository's docker-compose.yml.
type: application

# Chart version: bumped for chart-only changes.
version: 0.1.0

# Tracks the workspace crate version (crates/api/Cargo.toml). Used as the
# default image tag for every workload whose `image.tag` is left empty.
appVersion: "0.1.0"

# templates/networkpolicy.yaml selects the ingress-controller namespace via
# the automatic `kubernetes.io/metadata.name` label, which is GA from 1.22.
kubeVersion: ">=1.23.0-0"

home: https://github.com/FasterSpeeding/nr-status-v2
sources:
  - https://github.com/FasterSpeeding/nr-status-v2

# Deliberately no `dependencies:` — the chart bundles its own PostgreSQL
# StatefulSet rather than pulling a subchart, so `helm dependency update` is
# never needed and the chart installs in an air-gapped cluster.
```

- [ ] **Step 2: Create the initial `values.yaml`**

This file grows in later tasks. Create it now with only the global blocks Task 1 needs:

```yaml
# Values for the nr-status chart.
#
# The layout mirrors .env.example so the docker-compose path and the Helm
# path do not drift. Every service section names the crate whose
# `config.rs` defines its environment contract.

# -- Override the chart name used in resource names and labels.
nameOverride: ""
# -- Override the fully-qualified release name entirely.
fullnameOverride: ""

# -- Image pull secrets applied to every pod in the chart.
imagePullSecrets: []
# - name: my-registry-creds

serviceAccount:
  # -- Create a ServiceAccount for the chart's workloads.
  create: true
  # -- Name to use. When empty and `create` is true, the fullname is used.
  name: ""
  # -- Annotations for the ServiceAccount (e.g. cloud workload identity).
  annotations: {}
```

- [ ] **Step 3: Create `templates/_helpers.tpl`**

```
{{/*
Chart name, overridable.
*/}}
{{- define "nr-status.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Fully-qualified release name. Every object in this chart derives its name
from this, so overriding it renames the whole install consistently.
*/}}
{{- define "nr-status.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Chart name-and-version for the helm.sh/chart label.
*/}}
{{- define "nr-status.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Selector labels. Call as:
  {{- include "nr-status.selectorLabels" (dict "root" . "component" "api") }}
These land in an immutable `selector.matchLabels`, so nothing that changes
between releases (version, chart version) may appear here.
*/}}
{{- define "nr-status.selectorLabels" -}}
app.kubernetes.io/name: {{ include "nr-status.name" .root }}
app.kubernetes.io/instance: {{ .root.Release.Name }}
app.kubernetes.io/component: {{ .component }}
{{- end }}

{{/*
Full common label set. Call as:
  {{- include "nr-status.labels" (dict "root" . "component" "api") }}
*/}}
{{- define "nr-status.labels" -}}
helm.sh/chart: {{ include "nr-status.chart" .root }}
{{ include "nr-status.selectorLabels" . }}
{{- if .root.Chart.AppVersion }}
app.kubernetes.io/version: {{ .root.Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .root.Release.Service }}
app.kubernetes.io/part-of: nr-status
{{- end }}

{{/*
ServiceAccount name. Takes root.
*/}}
{{- define "nr-status.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "nr-status.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Image reference. Call as:
  {{ include "nr-status.image" (dict "root" . "image" .Values.api.image) }}
An empty `tag` falls back to the chart's appVersion.
*/}}
{{- define "nr-status.image" -}}
{{- printf "%s:%s" .image.repository (default .root.Chart.AppVersion .image.tag) }}
{{- end }}

{{/*
Per-component object names. Each takes root.
*/}}
{{- define "nr-status.postgresFullname" -}}
{{- printf "%s-postgres" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "nr-status.apiFullname" -}}
{{- printf "%s-api" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "nr-status.frontendFullname" -}}
{{- printf "%s-frontend" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
In-cluster base URL of the api Service. Consumed by the frontend
(API_BASE_URL), by every poller (API_INGEST_URL / API_SAMPLE_STATIONS_URL)
and by the helm test pod. Takes root.
*/}}
{{- define "nr-status.apiBaseUrl" -}}
{{- printf "http://%s:%d" (include "nr-status.apiFullname" .) (int .Values.api.service.port) }}
{{- end }}

{{/*
Pod-level security context. Call as:
  {{- include "nr-status.podSecurityContext" (dict "override" .Values.api.podSecurityContext) | nindent 8 }}
The chart-wide defaults below are merged with the workload's own
`podSecurityContext` value, which wins on conflict. Postgres deliberately
does NOT use this helper -- it must pin uid/gid 999, see
postgres-statefulset.yaml.
*/}}
{{- define "nr-status.podSecurityContext" -}}
{{- $defaults := dict "runAsNonRoot" true "seccompProfile" (dict "type" "RuntimeDefault") -}}
{{- toYaml (mergeOverwrite $defaults (default (dict) .override | deepCopy)) }}
{{- end }}

{{/*
Container-level security context. Call as:
  {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
The frontend passes false: `next start` writes its incremental cache under
.next/cache.
*/}}
{{- define "nr-status.containerSecurityContext" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: {{ .readOnlyRootFilesystem }}
capabilities:
  drop:
    - ALL
{{- end }}
```

- [ ] **Step 4: Create `templates/serviceaccount.yaml`**

```yaml
{{- if .Values.serviceAccount.create }}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "nr-status.serviceAccountName" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "shared") | nindent 4 }}
  {{- with .Values.serviceAccount.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
# Nothing in this stack talks to the Kubernetes API server, so no pod
# mounts this token either (every pod spec sets
# automountServiceAccountToken: false as well).
automountServiceAccountToken: false
{{- end }}
```

- [ ] **Step 5: Run `helm lint`**

Run: `helm lint charts/nr-status`

Expected: `1 chart(s) linted, 0 chart(s) failed`. An `[INFO] Chart.yaml: icon is recommended` line is expected and is not a failure — the chart ships no icon.

- [ ] **Step 6: Render and confirm exactly one object comes out**

Run:

```bash
helm template nr-status charts/nr-status | grep -E '^(kind|  name):'
```

Expected:

```
kind: ServiceAccount
  name: nr-status
```

(The release is called `nr-status` and the chart is called `nr-status`, so `nr-status.fullname`'s `contains` branch collapses the two into a single `nr-status` — that is correct, not a bug.)

- [ ] **Step 7: Confirm the ServiceAccount can be disabled**

Run:

```bash
helm template nr-status charts/nr-status --set serviceAccount.create=false | grep -c 'kind:' || true
```

Expected: `0`.

- [ ] **Step 8: Commit**

```bash
git add charts/nr-status
git commit -m "Add nr-status Helm chart scaffold with naming helpers and ServiceAccount"
```

---

### Task 2: Secret, the lookup-preserve pattern, and shared secret-reference helpers

This is the correctness-critical task. Read the "Helm 4 note" in Global Constraints before starting.

**Files:**
- Create: `charts/nr-status/templates/secret.yaml`
- Modify: `charts/nr-status/templates/_helpers.tpl` (append the secret-reference helpers)
- Modify: `charts/nr-status/values.yaml` (append the `secrets`, `postgresql.enabled` + `postgresql.auth`, `externalDatabase` and `pollers` blocks)

**Interfaces:**
- Consumes: `nr-status.fullname` (Task 1).
- Produces, consumed by Tasks 3, 4, 5 and 7:
  - `nr-status.secretName` (root) → the chart-rendered Secret's name.
  - `nr-status.postgresSecretName` (root) → `postgresql.auth.existingSecret` when set, else the chart Secret.
  - `nr-status.postgresSecretPasswordKey` (root) → `postgresql.auth.existingSecretPasswordKey` when an existingSecret is set, else `postgres-password`.
  - `nr-status.internalTokenSecretName` (root) / `nr-status.internalTokenSecretKey` (root) → same resolution for `secrets.existingSecret` / `secrets.existingSecretInternalTokenKey` / `internal-token`.
  - `nr-status.pollerSecretName` (`dict "root" $ "poller" $p`) / `nr-status.pollerSecretKey` (`dict "root" $ "name" $name "poller" $p`) → same resolution for each poller's `existingSecret` / `existingSecretApiKeyKey` / `rdm-<name>-api-key`.
- Produces the values map `.Values.pollers` with the four fixed entries `incidents`, `stations`, `tocs`, `ldbws`, consumed by Task 7.

- [ ] **Step 1: Append the secret/auth/poller value blocks to `values.yaml`**

Append to `charts/nr-status/values.yaml`:

```yaml

# ---------------------------------------------------------------------------
# Shared secrets
# ---------------------------------------------------------------------------
# Every secret value in this chart follows the same three-way rule:
#   1. `existingSecret` set  -> the chart renders NO key for it; the workloads
#                               read the named Secret directly. Use this with
#                               External Secrets Operator, Vault or SOPS.
#   2. an explicit value set -> the chart renders that value.
#   3. neither               -> for `internalToken` and the postgres password
#                               ONLY, a 32-character alphanumeric value is
#                               generated, and PRESERVED across `helm upgrade`
#                               by reading the live Secret back (see README).
secrets:
  # -- Shared secret the pollers present as `X-Internal-Token` to reach the
  # api's /private/* endpoints (crates/api/src/auth.rs). Generated when empty.
  # The api refuses to start if this ends up empty (crates/api/src/app.rs).
  internalToken: ""
  # -- Read `internal-token` from this pre-existing Secret instead.
  existingSecret: ""
  # -- Key within `existingSecret` holding the internal token.
  existingSecretInternalTokenKey: internal-token

# ---------------------------------------------------------------------------
# postgres
# ---------------------------------------------------------------------------
postgresql:
  # -- Deploy the bundled single-replica PostgreSQL StatefulSet. Set to false
  # to use a managed database, and fill in `externalDatabase` below.
  enabled: true
  auth:
    # -- Database role the api and aggregator connect as.
    username: nr_status
    # -- Database name.
    database: nr_status
    # -- Password. Generated (32 alphanumeric chars) when empty.
    #
    # IMPORTANT: DATABASE_URL is assembled as a URL, and the chart never sees
    # this value at render time when `existingSecret` is used, so it cannot
    # percent-encode it for you. A password containing any of
    #   @ : / ? # [ ] %
    # must be percent-encoded by YOU before being put here. Generated
    # passwords use randAlphaNum (letters and digits only), so the default
    # path is never affected.
    password: ""
    # -- Read `postgres-password` from this pre-existing Secret instead.
    existingSecret: ""
    # -- Key within `existingSecret` holding the password.
    existingSecretPasswordKey: postgres-password

# ---------------------------------------------------------------------------
# External database (used only when postgresql.enabled is false)
# ---------------------------------------------------------------------------
# Exactly one of `url` or `existingSecret` must be set when
# postgresql.enabled is false; otherwise rendering fails with an explicit
# message. `existingSecret` is the preferred form -- it keeps the password
# out of `helm get values` as well as out of the Deployment spec.
externalDatabase:
  # -- Full libpq/sqlx connection URL, e.g.
  # postgres://user:pass@host.example.com:5432/nr_status
  url: ""
  # -- Pre-existing Secret holding the whole connection URL.
  existingSecret: ""
  # -- Key within `existingSecret` holding the URL.
  existingSecretUrlKey: database-url

# ---------------------------------------------------------------------------
# Rail Data Marketplace pollers
# ---------------------------------------------------------------------------
# ALL FOUR ARE DISABLED BY DEFAULT. As documented in .env.example, no
# confirmed RDM endpoint exists for any of these feeds -- every base URL in
# the repository today is a deliberately non-functional `*.example.invalid`
# placeholder. A default install therefore brings up postgres + api +
# aggregator + frontend and works immediately, instead of four pods logging
# connection failures.
#
# Enabling a poller without setting its `baseUrl` aborts rendering with an
# explicit message rather than deploying a pod that cannot work.
#
# `baseUrlEnvVar` and `ingestPath` are part of each poller's contract with
# its binary (see crates/poller-*/src/config.rs) -- change them only if that
# code changes.
pollers:
  incidents:
    enabled: false
    image:
      repository: nr-status/poller-incidents
      tag: ""
      pullPolicy: IfNotPresent
    # -- RDM Knowledgebase Incidents feed base URL. Required when enabled.
    baseUrl: ""
    # -- Env var this binary reads the base URL from. Do not change.
    baseUrlEnvVar: RDM_INCIDENTS_BASE_URL
    # -- Path on the api Service this poller POSTs its results to.
    ingestPath: /private/incidents
    # -- RSPS5050 P-03-00 Rev A section 10: "Recommend every 5 minutes."
    pollIntervalSecs: 300
    # -- RDM API key, sent as the `x-apikey` header. Rendered into the chart
    # Secret as `rdm-incidents-api-key` when `existingSecret` is empty.
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: rdm-incidents-api-key
    logLevel: info
    extraEnv: []
    resources: {}
    nodeSelector: {}
    tolerations: []
    affinity: {}
    podAnnotations: {}
    podSecurityContext: {}
  stations:
    enabled: false
    image:
      repository: nr-status/poller-stations
      tag: ""
      pullPolicy: IfNotPresent
    baseUrl: ""
    baseUrlEnvVar: RDM_STATIONS_BASE_URL
    ingestPath: /private/stations
    # -- RSPS5050 P-03-00 Rev A section 6: once every 24 hours, overnight.
    pollIntervalSecs: 86400
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: rdm-stations-api-key
    logLevel: info
    extraEnv: []
    resources: {}
    nodeSelector: {}
    tolerations: []
    affinity: {}
    podAnnotations: {}
    podSecurityContext: {}
  tocs:
    enabled: false
    image:
      repository: nr-status/poller-tocs
      tag: ""
      pullPolicy: IfNotPresent
    baseUrl: ""
    baseUrlEnvVar: RDM_TOCS_BASE_URL
    ingestPath: /private/tocs
    # -- RSPS5050 P-03-00 Rev A section 3: at least once every 24 hours.
    pollIntervalSecs: 86400
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: rdm-tocs-api-key
    logLevel: info
    extraEnv: []
    resources: {}
    nodeSelector: {}
    tolerations: []
    affinity: {}
    podAnnotations: {}
    podSecurityContext: {}
  ldbws:
    enabled: false
    image:
      repository: nr-status/poller-ldbws
      tag: ""
      pullPolicy: IfNotPresent
    # -- LDBWS base URL up to and including the /LDBWS/api/20220120 segment;
    # the binary appends /GetDepBoardWithDetails/{crs} itself.
    baseUrl: ""
    baseUrlEnvVar: LDBWS_BASE_URL
    ingestPath: /private/station-samples
    # -- ldbws only: the api endpoint listing which stations to sample. This
    # is a SECOND api URL, separate from ingestPath -- see
    # crates/poller-ldbws/src/config.rs.
    sampleStationsPath: /private/sample-stations
    # -- ldbws only: LDBWS `numRows` query parameter (upstream default 10).
    numRows: 10
    # -- DESIGN.md section 4 targets 30-60s; 60 is the conservative end
    # because this feed's real rate limit is unconfirmed.
    pollIntervalSecs: 60
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: rdm-ldbws-api-key
    logLevel: info
    extraEnv: []
    resources: {}
    nodeSelector: {}
    tolerations: []
    affinity: {}
    podAnnotations: {}
    podSecurityContext: {}
```

- [ ] **Step 2: Append the secret-reference helpers to `_helpers.tpl`**

These helpers are the *only* place any template decides which Secret and key
a password lives in. The Postgres StatefulSet and its api/aggregator
consumers all call the same two, so they cannot disagree.

Append to `charts/nr-status/templates/_helpers.tpl`:

```
{{/*
Name of the Secret this chart renders. Takes root.
*/}}
{{- define "nr-status.secretName" -}}
{{- include "nr-status.fullname" . }}
{{- end }}

{{/*
Resolved Secret name/key for the postgres password. Takes root.
Used by postgres-statefulset.yaml (POSTGRES_PASSWORD) AND by
api-deployment.yaml / aggregator-deployment.yaml (PGPASSWORD). Because both
sides call these, an `existingSecret` override can never desynchronise them.
*/}}
{{- define "nr-status.postgresSecretName" -}}
{{- default (include "nr-status.secretName" .) .Values.postgresql.auth.existingSecret }}
{{- end }}

{{- define "nr-status.postgresSecretPasswordKey" -}}
{{- if .Values.postgresql.auth.existingSecret }}
{{- .Values.postgresql.auth.existingSecretPasswordKey }}
{{- else }}
{{- print "postgres-password" }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for the shared internal token. Takes root.
Used by api-deployment.yaml and by all four poller deployments.
*/}}
{{- define "nr-status.internalTokenSecretName" -}}
{{- default (include "nr-status.secretName" .) .Values.secrets.existingSecret }}
{{- end }}

{{- define "nr-status.internalTokenSecretKey" -}}
{{- if .Values.secrets.existingSecret }}
{{- .Values.secrets.existingSecretInternalTokenKey }}
{{- else }}
{{- print "internal-token" }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for one poller's RDM API key. Call as:
  {{ include "nr-status.pollerSecretName" (dict "root" $ "poller" $p) }}
  {{ include "nr-status.pollerSecretKey" (dict "root" $ "name" $name "poller" $p) }}
*/}}
{{- define "nr-status.pollerSecretName" -}}
{{- default (include "nr-status.secretName" .root) .poller.existingSecret }}
{{- end }}

{{- define "nr-status.pollerSecretKey" -}}
{{- if .poller.existingSecret }}
{{- .poller.existingSecretApiKeyKey }}
{{- else }}
{{- printf "rdm-%s-api-key" .name }}
{{- end }}
{{- end }}
```

- [ ] **Step 3: Create `templates/secret.yaml`**

```
{{/*
The chart-rendered Secret.

THE LOOKUP-PRESERVE PATTERN BELOW IS THE SINGLE MOST IMPORTANT CORRECTNESS
DETAIL IN THIS CHART. Without it, every `helm upgrade` would call
randAlphaNum again and rotate the postgres password out from under the
running database's PVC, breaking every connection. `lookup` reads the live
Secret from the cluster and reuses whatever is already there.

Note the shape: `lookup` returns an EMPTY MAP when the Secret does not
exist, so `$existing.data` is a nil interface{} and cannot be handed
straight to sprig's `get` (which demands map[string]interface{}). The
`default (dict)` on the line below is what makes this work on Helm 4 --
without it rendering fails with
  "wrong type for value; expected map[string]interface {}; got interface {}".

Known limitation, also stated in README.md: `lookup` always returns empty
during `helm template` and `--dry-run`, so an offline render shows a
DIFFERENT generated value every time. That is cosmetic for dry runs, but it
does mean `helm template | kubectl apply` is not a supported install path
when relying on generated secrets -- set explicit values or use
`existingSecret` for that workflow.
*/}}
{{- $secretName := include "nr-status.secretName" . -}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace $secretName -}}
{{- $existingData := default (dict) $existing.data -}}
{{- $data := dict -}}

{{/* postgres-password: omitted entirely when an existingSecret is in use,
     and never rendered at all when the bundled postgres is disabled. */}}
{{- if and .Values.postgresql.enabled (not .Values.postgresql.auth.existingSecret) -}}
{{- $pw := .Values.postgresql.auth.password | default (get $existingData "postgres-password" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "postgres-password" ($pw | b64enc) -}}
{{- end -}}

{{/* internal-token: always needed (the api asserts it is non-empty), so it
     is generated whenever no value and no existingSecret is supplied. */}}
{{- if not .Values.secrets.existingSecret -}}
{{- $token := .Values.secrets.internalToken | default (get $existingData "internal-token" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "internal-token" ($token | b64enc) -}}
{{- end -}}

{{/* One rdm-<name>-api-key per ENABLED poller without an existingSecret.
     Deliberately NOT auto-generated: a random RDM key is meaningless. The
     key is still rendered (possibly empty) so the pod's secretKeyRef always
     resolves instead of wedging in CreateContainerConfigError. */}}
{{- range $name, $poller := .Values.pollers -}}
{{- if and $poller.enabled (not $poller.existingSecret) -}}
{{- $_ := set $data (printf "rdm-%s-api-key" $name) ($poller.apiKey | default "" | b64enc) -}}
{{- end -}}
{{- end -}}

{{- if $data }}
apiVersion: v1
kind: Secret
metadata:
  name: {{ $secretName }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "shared") | nindent 4 }}
type: Opaque
data:
  {{- range $key, $value := $data }}
  {{ $key }}: {{ $value | quote }}
  {{- end }}
{{- end }}
```

- [ ] **Step 4: Verify the default render produces both generated keys**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/secret.yaml
```

Expected: a `kind: Secret` named `nr-status` with exactly two keys,
`internal-token` and `postgres-password`, each a base64 blob. Decode one to
confirm the length:

```bash
helm template nr-status charts/nr-status --show-only templates/secret.yaml \
  | grep 'postgres-password:' | awk '{print $2}' | tr -d '"' | base64 -d | wc -c
```

Expected: `32`.

- [ ] **Step 5: Verify `existingSecret` removes the key entirely**

Run:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.auth.existingSecret=my-db-secret \
  --show-only templates/secret.yaml
```

Expected: a Secret containing **only** `internal-token`. No `postgres-password` key.

Then check that with *both* overrides the Secret disappears completely:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.auth.existingSecret=my-db-secret \
  --set secrets.existingSecret=my-token-secret \
  --show-only templates/secret.yaml
```

Expected: Helm errors with `could not find template templates/secret.yaml in chart` — which is how `--show-only` reports a template that rendered to nothing. Confirm the same via the full render instead:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.auth.existingSecret=my-db-secret \
  --set secrets.existingSecret=my-token-secret | grep -c 'kind: Secret' || true
```

Expected: `0`.

- [ ] **Step 6: Verify an enabled poller adds exactly one key**

Run:

```bash
helm template nr-status charts/nr-status \
  --set pollers.ldbws.enabled=true \
  --set pollers.ldbws.baseUrl=http://example.invalid \
  --set pollers.ldbws.apiKey=my-key \
  --show-only templates/secret.yaml
```

Expected: three keys — `internal-token`, `postgres-password`, `rdm-ldbws-api-key` — and `echo <blob> | base64 -d` on the last yields `my-key`.

- [ ] **Step 7: Verify explicit values are passed through untouched**

Run:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.auth.password=SENTINELPGPASS \
  --set secrets.internalToken=SENTINELTOKEN \
  --show-only templates/secret.yaml \
  | grep -E 'postgres-password|internal-token'
```

Expected: the two base64 values decode to `SENTINELPGPASS` and `SENTINELTOKEN` respectively. The **plaintext** strings must NOT appear anywhere in the output — this is the same property Task 12 checks across the whole render.

- [ ] **Step 8: Lint**

Run: `helm lint charts/nr-status`
Expected: `0 chart(s) failed`.

- [ ] **Step 9: Commit**

```bash
git add charts/nr-status
git commit -m "Add nr-status chart Secret with upgrade-safe generated passwords"
```

---

### Task 3: PostgreSQL StatefulSet and headless Service

**Files:**
- Create: `charts/nr-status/templates/postgres-statefulset.yaml`
- Create: `charts/nr-status/templates/postgres-service.yaml`
- Modify: `charts/nr-status/values.yaml` (extend the `postgresql` block)

**Interfaces:**
- Consumes: `nr-status.fullname`, `nr-status.labels`, `nr-status.selectorLabels`, `nr-status.image`, `nr-status.postgresFullname`, `nr-status.serviceAccountName` (Task 1); `nr-status.postgresSecretName`, `nr-status.postgresSecretPasswordKey` (Task 2).
- Produces: a headless Service named `{{ include "nr-status.postgresFullname" . }}` on port `.Values.postgresql.service.port` (5432), which Task 4's `nr-status.databaseEnv` helper hardcodes as the DB host.

- [ ] **Step 1: Extend the `postgresql` block in `values.yaml`**

Insert these keys into the existing `postgresql:` mapping, after `enabled:` and around the existing `auth:` sub-block (order within the mapping does not matter to Helm; keep `auth` where it is and add the rest):

```yaml
  image:
    repository: postgres
    # -- Pinned to the same major the docker-compose stack uses.
    tag: "16"
    pullPolicy: IfNotPresent
  service:
    # -- Port the headless Service and the container listen on.
    port: 5432
  persistence:
    # -- Attach a PersistentVolumeClaim. When false, an emptyDir is used and
    # ALL DATA IS LOST when the pod is rescheduled -- ephemeral testing only.
    enabled: true
    size: 8Gi
    # -- StorageClass name. Empty means the cluster default.
    storageClass: ""
    accessModes:
      - ReadWriteOnce
    # -- Use a pre-existing PVC instead of a volumeClaimTemplates entry.
    existingClaim: ""
  # -- Extra PostgreSQL container env vars (e.g. shared_buffers tuning).
  extraEnv: []
  # -- Merged over the pod securityContext. The uid/gid/fsGroup 999 pins are
  # required by the postgres image -- override them only if you know why.
  podSecurityContext: {}
  resources: {}
    # requests:
    #   cpu: 100m
    #   memory: 256Mi
    # limits:
    #   memory: 1Gi
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
```

Also append this comment immediately above `postgresql:` so operators understand the deliberate absence of a `replicaCount`:

```yaml
# There is intentionally no `replicaCount` here: this is a single-replica
# StatefulSet with no replication, backup or restore story (an explicit
# non-goal of the design). If you need HA, set `postgresql.enabled: false`
# and point `externalDatabase` at a managed database.
```

- [ ] **Step 2: Create `templates/postgres-service.yaml`**

```yaml
{{- if .Values.postgresql.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.postgresFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "postgres") | nindent 4 }}
spec:
  # Headless: this Service exists to give the StatefulSet stable pod DNS,
  # not to load-balance. With one replica the name resolves to the single
  # pod's IP, which is exactly what DATABASE_URL needs.
  clusterIP: None
  ports:
    - name: postgres
      port: {{ .Values.postgresql.service.port }}
      targetPort: postgres
      protocol: TCP
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "postgres") | nindent 4 }}
{{- end }}
```

- [ ] **Step 3: Create `templates/postgres-statefulset.yaml`**

```yaml
{{- if .Values.postgresql.enabled }}
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: {{ include "nr-status.postgresFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "postgres") | nindent 4 }}
spec:
  # Fixed at 1 -- see the comment above `postgresql:` in values.yaml.
  replicas: 1
  serviceName: {{ include "nr-status.postgresFullname" . }}
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "postgres") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "postgres") | nindent 8 }}
      {{- with .Values.postgresql.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      # NOTE: postgres:16 is the one image in this chart whose default USER
      # is root -- its entrypoint drops to `postgres` via gosu. A bare
      # `runAsNonRoot: true` would therefore fail admission, so the uid/gid
      # are pinned explicitly below (the image runs correctly when it is
      # already uid 999 and skips its own gosu step). fsGroup: 999 is what
      # lets the postgres user write a freshly-provisioned PVC.
      #
      # This is the one workload that does not use the chart-wide
      # nr-status.podSecurityContext helper, because those two extra pins are
      # mandatory here. `postgresql.podSecurityContext` still overrides.
      securityContext:
        {{- $pgSecurityDefaults := dict "runAsNonRoot" true "runAsUser" 999 "runAsGroup" 999 "fsGroup" 999 "seccompProfile" (dict "type" "RuntimeDefault") }}
        {{- toYaml (mergeOverwrite $pgSecurityDefaults (default (dict) .Values.postgresql.podSecurityContext | deepCopy)) | nindent 8 }}
      containers:
        - name: postgres
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.postgresql.image) | quote }}
          imagePullPolicy: {{ .Values.postgresql.image.pullPolicy }}
          securityContext:
            allowPrivilegeEscalation: false
            # NOT read-only: postgres writes its socket to /var/run/postgresql.
            readOnlyRootFilesystem: false
            capabilities:
              drop:
                - ALL
          ports:
            - name: postgres
              containerPort: {{ .Values.postgresql.service.port }}
              protocol: TCP
          env:
            - name: POSTGRES_USER
              value: {{ .Values.postgresql.auth.username | quote }}
            - name: POSTGRES_DB
              value: {{ .Values.postgresql.auth.database | quote }}
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.postgresSecretName" . }}
                  key: {{ include "nr-status.postgresSecretPasswordKey" . }}
            # PGDATA must be a SUBDIRECTORY of the mount, not the mount
            # itself: many CSI drivers put a `lost+found` entry in a fresh
            # volume, and initdb refuses to initialise a non-empty directory.
            - name: PGDATA
              value: /var/lib/postgresql/data/pgdata
            {{- with .Values.postgresql.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          # Same probe command docker-compose.yml uses for its healthcheck.
          readinessProbe:
            exec:
              command:
                - sh
                - -c
                - exec pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h 127.0.0.1 -p {{ .Values.postgresql.service.port }}
            initialDelaySeconds: 5
            periodSeconds: 5
            timeoutSeconds: 5
            failureThreshold: 10
          livenessProbe:
            exec:
              command:
                - sh
                - -c
                - exec pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" -h 127.0.0.1 -p {{ .Values.postgresql.service.port }}
            initialDelaySeconds: 30
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 6
          volumeMounts:
            - name: data
              mountPath: /var/lib/postgresql/data
          {{- with .Values.postgresql.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.postgresql.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.postgresql.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.postgresql.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- if or (not .Values.postgresql.persistence.enabled) .Values.postgresql.persistence.existingClaim }}
      volumes:
        - name: data
          {{- if .Values.postgresql.persistence.existingClaim }}
          persistentVolumeClaim:
            claimName: {{ .Values.postgresql.persistence.existingClaim }}
          {{- else }}
          emptyDir: {}
          {{- end }}
      {{- end }}
  {{- if and .Values.postgresql.persistence.enabled (not .Values.postgresql.persistence.existingClaim) }}
  volumeClaimTemplates:
    - metadata:
        name: data
        labels:
          {{- include "nr-status.labels" (dict "root" . "component" "postgres") | nindent 10 }}
      spec:
        accessModes:
          {{- toYaml .Values.postgresql.persistence.accessModes | nindent 10 }}
        resources:
          requests:
            storage: {{ .Values.postgresql.persistence.size | quote }}
        {{- if .Values.postgresql.persistence.storageClass }}
        storageClassName: {{ .Values.postgresql.persistence.storageClass | quote }}
        {{- end }}
  {{- end }}
{{- end }}
```

- [ ] **Step 4: Render and check the two Kubernetes-specific requirements**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/postgres-statefulset.yaml \
  | grep -E 'PGDATA|/var/lib/postgresql/data/pgdata|fsGroup|runAsUser'
```

Expected: `PGDATA` set to `/var/lib/postgresql/data/pgdata`, `runAsUser: 999`, `fsGroup: 999`.

- [ ] **Step 5: Confirm the volumeClaimTemplates / emptyDir / existingClaim branches**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/postgres-statefulset.yaml | grep -c 'volumeClaimTemplates'
helm template nr-status charts/nr-status --set postgresql.persistence.enabled=false --show-only templates/postgres-statefulset.yaml | grep -c 'emptyDir'
helm template nr-status charts/nr-status --set postgresql.persistence.existingClaim=my-pvc --show-only templates/postgres-statefulset.yaml | grep -c 'claimName: my-pvc'
```

Expected: `1`, `1`, `1` respectively.

- [ ] **Step 6: Confirm nothing Postgres renders when disabled**

Because Task 4 has not landed yet, nothing else consumes the database, so this check is meaningful now:

```bash
helm template nr-status charts/nr-status --set postgresql.enabled=false | grep -c 'postgres' || true
```

Expected: `0`.

- [ ] **Step 7: Validate the rendered manifests**

Run:

```bash
helm template nr-status charts/nr-status | kubeconform -strict -summary -kubernetes-version 1.31.0 -
```

Expected: `Valid: 4, Invalid: 0, Errors: 0, Skipped: 0` — the ServiceAccount, the Secret, the headless Service and the StatefulSet.

- [ ] **Step 8: Lint and commit**

```bash
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add bundled PostgreSQL StatefulSet and headless Service to nr-status chart"
```

---

### Task 4: `DATABASE_URL` assembly helper, api Deployment and Service

The `$(VAR)` interpolation in this task is the second correctness-critical
detail after Task 2's `lookup`. Read the inline comments carefully.

**Files:**
- Modify: `charts/nr-status/templates/_helpers.tpl` (append `nr-status.databaseEnv`)
- Create: `charts/nr-status/templates/api-deployment.yaml`
- Create: `charts/nr-status/templates/api-service.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `api` block)

**Interfaces:**
- Consumes: everything from Tasks 1-3.
- Produces:
  - `nr-status.databaseEnv` (takes root) — emits the complete `env` entries a
    database consumer needs, as a YAML list fragment. Consumed verbatim by
    this task's api Deployment and by Task 5's aggregator Deployment. **Both
    call this helper; neither builds its own DB env.**
  - A ClusterIP Service named `{{ include "nr-status.apiFullname" . }}` on
    `.Values.api.service.port` (8080), consumed by Tasks 6, 7 and 10 via
    `nr-status.apiBaseUrl`.

- [ ] **Step 1: Append the `api` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# api (crates/api/src/data/config.rs: ServiceArguments)
# ---------------------------------------------------------------------------
api:
  image:
    repository: nr-status/api
    # -- Empty means "use the chart's appVersion".
    tag: ""
    pullPolicy: IfNotPresent
  # -- Replicas. >1 is safe: crates/api/src/main.rs runs sqlx::migrate!() in
  # process at startup, and sqlx's Migrator takes a Postgres advisory lock,
  # so concurrent startups serialise their migrations rather than racing.
  replicaCount: 1
  service:
    type: ClusterIP
    port: 8080
  # -- tracing-subscriber EnvFilter syntax: "info", "api=debug", "warn", ...
  logLevel: info
  probes:
    # -- Path served by crates/api/src/routes/health.rs, nested under /public
    # by main.rs. Changing this must be matched in tests/test-api-health.yaml.
    path: /public/health
    startup:
      # 30 x 2s = 60s of headroom. The api runs sqlx::migrate!() BEFORE it
      # binds its listener (crates/api/src/main.rs), so a cold start against
      # an empty database can take a while and must not be killed by the
      # liveness probe.
      periodSeconds: 2
      failureThreshold: 30
      timeoutSeconds: 3
    readiness:
      periodSeconds: 10
      failureThreshold: 3
      timeoutSeconds: 3
    liveness:
      periodSeconds: 10
      failureThreshold: 3
      timeoutSeconds: 3
  # -- Extra env vars appended to the container.
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  # -- Merged over the chart-wide pod securityContext defaults
  # (runAsNonRoot: true, seccompProfile: RuntimeDefault).
  podSecurityContext: {}
```

- [ ] **Step 2: Append `nr-status.databaseEnv` to `_helpers.tpl`**

```
{{/*
Environment entries giving a workload a working DATABASE_URL. Takes root.
Used identically by api-deployment.yaml and aggregator-deployment.yaml.

WHY THE $(PGPASSWORD) INDIRECTION: crates/api/src/data/config.rs and
crates/aggregator/src/config.rs both want ONE `DATABASE_URL` string that
contains the password -- there is no host/user/password-parts form. Writing
that string into env[].value would expose the password to anyone with
`get deployments`, a strictly wider audience than `get secrets`. Instead the
password is injected as its own secretKeyRef entry and referenced with
Kubernetes' `$(VAR)` syntax, which the kubelet expands at container start
from EARLIER entries in the SAME container's env list. Order therefore
matters: PGPASSWORD must come before DATABASE_URL. The password never
appears in the rendered Deployment.

CAVEAT (also in values.yaml and README.md): the chart cannot percent-encode
a password it never sees, so a password containing @ : / ? # [ ] % must be
percent-encoded by the operator. Generated passwords use randAlphaNum, so
the default path is never affected.
*/}}
{{- define "nr-status.databaseEnv" -}}
{{- if .Values.postgresql.enabled -}}
- name: PGPASSWORD
  valueFrom:
    secretKeyRef:
      name: {{ include "nr-status.postgresSecretName" . }}
      key: {{ include "nr-status.postgresSecretPasswordKey" . }}
- name: DATABASE_URL
  value: {{ printf "postgres://%s:$(PGPASSWORD)@%s:%d/%s" .Values.postgresql.auth.username (include "nr-status.postgresFullname" .) (int .Values.postgresql.service.port) .Values.postgresql.auth.database | quote }}
{{- else if .Values.externalDatabase.existingSecret -}}
- name: DATABASE_URL
  valueFrom:
    secretKeyRef:
      name: {{ .Values.externalDatabase.existingSecret }}
      key: {{ .Values.externalDatabase.existingSecretUrlKey }}
{{- else if .Values.externalDatabase.url -}}
- name: DATABASE_URL
  value: {{ .Values.externalDatabase.url | quote }}
{{- else -}}
{{- fail "postgresql.enabled is false but no external database is configured. Set externalDatabase.existingSecret (preferred) together with externalDatabase.existingSecretUrlKey, or set externalDatabase.url, or re-enable the bundled database with postgresql.enabled=true." -}}
{{- end -}}
{{- end }}
```

- [ ] **Step 3: Create `templates/api-service.yaml`**

```yaml
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.apiFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "api") | nindent 4 }}
spec:
  type: {{ .Values.api.service.type }}
  ports:
    - name: http
      port: {{ .Values.api.service.port }}
      targetPort: http
      protocol: TCP
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "api") | nindent 4 }}
```

- [ ] **Step 4: Create `templates/api-deployment.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "nr-status.apiFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "api") | nindent 4 }}
spec:
  replicas: {{ .Values.api.replicaCount }}
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "api") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "api") | nindent 8 }}
      {{- with .Values.api.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.api.podSecurityContext) | nindent 8 }}
      containers:
        - name: api
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.api.image) | quote }}
          imagePullPolicy: {{ .Values.api.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          ports:
            - name: http
              containerPort: {{ .Values.api.service.port }}
              protocol: TCP
          env:
            - name: BIND_URL
              value: {{ printf "0.0.0.0:%d" (int .Values.api.service.port) | quote }}
            # PGPASSWORD must stay immediately before DATABASE_URL -- the
            # kubelet only expands $(VAR) from EARLIER entries in this list.
            {{- include "nr-status.databaseEnv" . | nindent 12 }}
            - name: INTERNAL_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.internalTokenSecretName" . }}
                  key: {{ include "nr-status.internalTokenSecretKey" . }}
            - name: RUST_LOG
              value: {{ .Values.api.logLevel | quote }}
            # LINES_DIR is deliberately NOT set: crates/api/src/data/config.rs
            # already defaults it to /app/lines, which is exactly where
            # docker/api.Dockerfile bakes the line catalogue.
            {{- with .Values.api.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          startupProbe:
            httpGet:
              path: {{ .Values.api.probes.path }}
              port: http
            periodSeconds: {{ .Values.api.probes.startup.periodSeconds }}
            failureThreshold: {{ .Values.api.probes.startup.failureThreshold }}
            timeoutSeconds: {{ .Values.api.probes.startup.timeoutSeconds }}
          readinessProbe:
            httpGet:
              path: {{ .Values.api.probes.path }}
              port: http
            periodSeconds: {{ .Values.api.probes.readiness.periodSeconds }}
            failureThreshold: {{ .Values.api.probes.readiness.failureThreshold }}
            timeoutSeconds: {{ .Values.api.probes.readiness.timeoutSeconds }}
          livenessProbe:
            httpGet:
              path: {{ .Values.api.probes.path }}
              port: http
            periodSeconds: {{ .Values.api.probes.liveness.periodSeconds }}
            failureThreshold: {{ .Values.api.probes.liveness.failureThreshold }}
            timeoutSeconds: {{ .Values.api.probes.liveness.timeoutSeconds }}
          {{- with .Values.api.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.api.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.api.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.api.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 5: Verify the `$(PGPASSWORD)` indirection and env ordering**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/api-deployment.yaml \
  | sed -n '/env:/,/startupProbe/p'
```

Expected, in this exact order: `BIND_URL` (`0.0.0.0:8080`), then `PGPASSWORD` with a `secretKeyRef` to `nr-status` / `postgres-password`, then `DATABASE_URL` with the literal value `postgres://nr_status:$(PGPASSWORD)@nr-status-postgres:5432/nr_status`, then `INTERNAL_TOKEN` via `secretKeyRef`, then `RUST_LOG`.

- [ ] **Step 6: Verify no password leaks into the Deployment**

Run:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.auth.password=SENTINELPGPASS \
  --show-only templates/api-deployment.yaml | grep -c SENTINELPGPASS || true
```

Expected: `0`.

- [ ] **Step 7: Verify the `fail` guard and both external-database branches**

Run each and check the *exit code*, not just the message (piping to `head` masks it):

```bash
helm template nr-status charts/nr-status --set postgresql.enabled=false > /dev/null 2>&1; echo "exit=$?"
```

Expected: `exit=1`, and running it without `> /dev/null 2>&1` prints:

```
Error: execution error at (nr-status/templates/api-deployment.yaml:...): postgresql.enabled is false but no external database is configured. Set externalDatabase.existingSecret (preferred) together with externalDatabase.existingSecretUrlKey, or set externalDatabase.url, or re-enable the bundled database with postgresql.enabled=true.
```

Then the two working forms:

```bash
helm template nr-status charts/nr-status \
  --set postgresql.enabled=false \
  --set externalDatabase.url='postgres://u:p@db.example.com:5432/nr_status' \
  --show-only templates/api-deployment.yaml | grep -A1 'name: DATABASE_URL'

helm template nr-status charts/nr-status \
  --set postgresql.enabled=false \
  --set externalDatabase.existingSecret=my-db \
  --show-only templates/api-deployment.yaml | grep -A4 'name: DATABASE_URL'
```

Expected: the first prints a literal `value:` with the URL and **no** `PGPASSWORD` entry anywhere; the second prints a `secretKeyRef` to `my-db` / `database-url` and also no `PGPASSWORD`.

- [ ] **Step 8: Validate and lint**

```bash
helm template nr-status charts/nr-status | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
```

Expected: `Invalid: 0, Errors: 0`; `0 chart(s) failed`.

- [ ] **Step 9: Commit**

```bash
git add charts/nr-status
git commit -m "Add api Deployment and Service with password-free DATABASE_URL assembly"
```

---

### Task 5: aggregator Deployment

**Files:**
- Create: `charts/nr-status/templates/aggregator-deployment.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `aggregator` block)

**Interfaces:**
- Consumes: `nr-status.databaseEnv` (Task 4) — the same helper the api uses, which is what guarantees the two agree on the Secret reference.
- Produces: no Service and no probes; nothing else consumes this workload.

- [ ] **Step 1: Append the `aggregator` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# aggregator (crates/aggregator/src/config.rs: Config)
# ---------------------------------------------------------------------------
# There is intentionally no `replicaCount` here. The aggregator is a
# singleton write loop against Postgres: two copies would double-write
# line_status and race the history prune. The Deployment is pinned to
# replicas: 1 with strategy Recreate for the same reason -- a RollingUpdate
# would briefly run two.
aggregator:
  image:
    repository: nr-status/aggregator
    tag: ""
    pullPolicy: IfNotPresent
  # -- DESIGN.md section 4 targets every 30-60s; 60 is the conservative end.
  pollIntervalSecs: 60
  # -- How long line_status_history rows are kept before being pruned.
  historyRetentionDays: 7
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  # -- Merged over the chart-wide pod securityContext defaults
  # (runAsNonRoot: true, seccompProfile: RuntimeDefault).
  podSecurityContext: {}
```

- [ ] **Step 2: Create `templates/aggregator-deployment.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-aggregator" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "aggregator") | nindent 4 }}
spec:
  # Fixed at 1 -- see the comment above `aggregator:` in values.yaml.
  replicas: 1
  strategy:
    # Recreate, not RollingUpdate: a rolling update would briefly run two
    # aggregators, double-writing line_status and racing the history prune.
    type: Recreate
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "aggregator") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "aggregator") | nindent 8 }}
      {{- with .Values.aggregator.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.aggregator.podSecurityContext) | nindent 8 }}
      containers:
        - name: aggregator
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.aggregator.image) | quote }}
          imagePullPolicy: {{ .Values.aggregator.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          # No probes: this binary exposes no HTTP surface at all. Failure
          # handling is restart-on-exit via the default restartPolicy.
          env:
            {{- include "nr-status.databaseEnv" . | nindent 12 }}
            # Matches the binary's own default (crates/aggregator/src/config.rs)
            # and where docker/aggregator.Dockerfile bakes the catalogue in.
            - name: LINES_DIR
              value: /app/lines
            - name: POLL_INTERVAL_SECS
              value: {{ .Values.aggregator.pollIntervalSecs | quote }}
            - name: HISTORY_RETENTION_DAYS
              value: {{ .Values.aggregator.historyRetentionDays | quote }}
            - name: RUST_LOG
              value: {{ .Values.aggregator.logLevel | quote }}
            {{- with .Values.aggregator.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.aggregator.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.aggregator.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.aggregator.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.aggregator.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 3: Verify the singleton guarantees and the shared secret reference**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/aggregator-deployment.yaml \
  | grep -E 'replicas:|type: Recreate|Probe|name: PGPASSWORD|DATABASE_URL'
```

Expected: `replicas: 1`, `type: Recreate`, **no** line containing `Probe`, plus the `PGPASSWORD` / `DATABASE_URL` pair.

- [ ] **Step 4: Verify api and aggregator resolve to the *same* Secret reference**

This is the property Task 2's helpers exist to guarantee. Run:

```bash
for t in api aggregator; do
  helm template nr-status charts/nr-status \
    --set postgresql.auth.existingSecret=shared-db-secret \
    --set postgresql.auth.existingSecretPasswordKey=pw \
    --show-only templates/$t-deployment.yaml | grep -A3 'name: PGPASSWORD'
done
```

Expected: both print `name: shared-db-secret` and `key: pw`. Any difference is a bug in the helpers, not in the templates.

- [ ] **Step 5: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add aggregator Deployment as a Recreate-strategy singleton"
```

---

### Task 6: frontend Deployment and Service

**Files:**
- Create: `charts/nr-status/templates/frontend-deployment.yaml`
- Create: `charts/nr-status/templates/frontend-service.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `frontend` block)

**Interfaces:**
- Consumes: `nr-status.apiBaseUrl` (Task 1) resolved against the api Service created in Task 4.
- Produces: a ClusterIP Service named `{{ include "nr-status.frontendFullname" . }}` on `.Values.frontend.service.port` (3000), consumed by Task 8's Ingress and Task 9's NetworkPolicy.

- [ ] **Step 1: Append the `frontend` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# frontend (frontend/lib/api.ts)
# ---------------------------------------------------------------------------
frontend:
  image:
    repository: nr-status/frontend
    tag: ""
    pullPolicy: IfNotPresent
  replicaCount: 1
  service:
    type: ClusterIP
    port: 3000
  probes:
    # -- Next.js ships no dedicated health route, so both probes hit "/".
    # Change this if one is ever added.
    path: /
    readiness:
      periodSeconds: 10
      failureThreshold: 3
      timeoutSeconds: 3
    liveness:
      periodSeconds: 10
      failureThreshold: 3
      timeoutSeconds: 3
  # -- Override the api base URL the server-side render calls. Leave empty to
  # use the in-cluster api Service. API_BASE_URL is read at REQUEST time in
  # Server Components, not baked at build time, so this is a runtime value.
  apiBaseUrl: ""
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  # -- Merged over the chart-wide pod securityContext defaults
  # (runAsNonRoot: true, seccompProfile: RuntimeDefault).
  podSecurityContext: {}
```

- [ ] **Step 2: Create `templates/frontend-service.yaml`**

```yaml
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.frontendFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "frontend") | nindent 4 }}
spec:
  type: {{ .Values.frontend.service.type }}
  ports:
    - name: http
      port: {{ .Values.frontend.service.port }}
      targetPort: http
      protocol: TCP
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "frontend") | nindent 4 }}
```

- [ ] **Step 3: Create `templates/frontend-deployment.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "nr-status.frontendFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "frontend") | nindent 4 }}
spec:
  replicas: {{ .Values.frontend.replicaCount }}
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "frontend") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "frontend") | nindent 8 }}
      {{- with .Values.frontend.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.frontend.podSecurityContext) | nindent 8 }}
      containers:
        - name: frontend
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.frontend.image) | quote }}
          imagePullPolicy: {{ .Values.frontend.image.pullPolicy }}
          securityContext:
            # readOnlyRootFilesystem is FALSE here, and only here. `next start`
            # writes its incremental cache under .next/cache at runtime, so a
            # read-only root filesystem makes this container fail on first
            # render. Every other workload in this chart is read-only.
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" false) | nindent 12 }}
          ports:
            - name: http
              containerPort: {{ .Values.frontend.service.port }}
              protocol: TCP
          env:
            - name: API_BASE_URL
              value: {{ default (include "nr-status.apiBaseUrl" .) .Values.frontend.apiBaseUrl | quote }}
            {{- with .Values.frontend.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          readinessProbe:
            httpGet:
              path: {{ .Values.frontend.probes.path }}
              port: http
            periodSeconds: {{ .Values.frontend.probes.readiness.periodSeconds }}
            failureThreshold: {{ .Values.frontend.probes.readiness.failureThreshold }}
            timeoutSeconds: {{ .Values.frontend.probes.readiness.timeoutSeconds }}
          livenessProbe:
            httpGet:
              path: {{ .Values.frontend.probes.path }}
              port: http
            periodSeconds: {{ .Values.frontend.probes.liveness.periodSeconds }}
            failureThreshold: {{ .Values.frontend.probes.liveness.failureThreshold }}
            timeoutSeconds: {{ .Values.frontend.probes.liveness.timeoutSeconds }}
          {{- with .Values.frontend.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.frontend.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.frontend.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.frontend.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 4: Verify `API_BASE_URL` and the read-only exception**

Run:

```bash
helm template nr-status charts/nr-status --show-only templates/frontend-deployment.yaml \
  | grep -A1 'name: API_BASE_URL'
helm template nr-status charts/nr-status --show-only templates/frontend-deployment.yaml \
  | grep 'readOnlyRootFilesystem'
```

Expected: `value: "http://nr-status-api:8080"`, and `readOnlyRootFilesystem: false`.

Then confirm the override works:

```bash
helm template nr-status charts/nr-status \
  --set frontend.apiBaseUrl=https://api.example.com \
  --show-only templates/frontend-deployment.yaml | grep -A1 'name: API_BASE_URL'
```

Expected: `value: "https://api.example.com"`.

- [ ] **Step 5: Confirm every other workload is still read-only**

Run:

```bash
helm template nr-status charts/nr-status | grep -c 'readOnlyRootFilesystem: true'
```

Expected: `2` at this point (api, aggregator). Postgres and the frontend are the two deliberate `false`s; the pollers arrive in Task 7.

- [ ] **Step 6: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add frontend Deployment and Service"
```

---

### Task 7: The four pollers from a single ranged template

**Files:**
- Create: `charts/nr-status/templates/poller-deployments.yaml`

(The `pollers` values map was created in Task 2 so the Secret could render
each poller's API key; this task adds only the template.)

**Interfaces:**
- Consumes: `.Values.pollers` (Task 2), `nr-status.pollerSecretName` / `nr-status.pollerSecretKey` (Task 2), `nr-status.internalTokenSecretName` / `nr-status.internalTokenSecretKey` (Task 2), `nr-status.apiBaseUrl` (Task 1).
- Produces: zero Deployments by default; one per enabled poller, named `<fullname>-poller-<name>` with `app.kubernetes.io/component: poller-<name>`. Task 9's NetworkPolicy matches on those component values.

The four entries and their differing contracts, restated here so the
implementer does not have to cross-reference (all verified against
`crates/poller-*/src/config.rs` and `docker-compose.yml`):

| Entry | `baseUrlEnvVar` | `ingestPath` | Cadence | Extra |
|---|---|---|---|---|
| `incidents` | `RDM_INCIDENTS_BASE_URL` | `/private/incidents` | 300 | — |
| `stations` | `RDM_STATIONS_BASE_URL` | `/private/stations` | 86400 | — |
| `tocs` | `RDM_TOCS_BASE_URL` | `/private/tocs` | 86400 | — |
| `ldbws` | `LDBWS_BASE_URL` | `/private/station-samples` | 60 | `NUM_ROWS`, `API_SAMPLE_STATIONS_URL=/private/sample-stations` |

- [ ] **Step 1: Create `templates/poller-deployments.yaml`**

```yaml
{{/*
One template, four Deployments. Everything that differs between the pollers
lives in the `.Values.pollers` map (baseUrlEnvVar, ingestPath, cadence, and
ldbws's two extra vars), so this file never branches on a poller's name.

Go templates iterate maps in sorted key order, so the rendered document
order is stable: incidents, ldbws, stations, tocs.
*/}}
{{- $root := . -}}
{{- range $name, $poller := .Values.pollers }}
{{- if $poller.enabled }}
{{- if not $poller.baseUrl }}
{{- fail (printf "pollers.%s.enabled is true but pollers.%s.baseUrl is empty. Set it to the confirmed Rail Data Marketplace base URL for this feed (see .env.example -- no confirmed endpoint exists for any of the four feeds yet), or set pollers.%s.enabled=false." $name $name $name) }}
{{- end }}
{{- $component := printf "poller-%s" $name }}
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-%s" (include "nr-status.fullname" $root) $component | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" $root "component" $component) | nindent 4 }}
spec:
  # Every poller is a singleton loop; a second copy would just duplicate
  # every upstream request and every ingest POST.
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" $root "component" $component) | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" $root "component" $component) | nindent 8 }}
      {{- with $poller.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" $root }}
      automountServiceAccountToken: false
      {{- with $root.Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" $poller.podSecurityContext) | nindent 8 }}
      containers:
        - name: {{ $component }}
          image: {{ include "nr-status.image" (dict "root" $root "image" $poller.image) | quote }}
          imagePullPolicy: {{ $poller.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          # No probes: none of the pollers expose an HTTP surface.
          env:
            - name: {{ $poller.baseUrlEnvVar }}
              value: {{ $poller.baseUrl | quote }}
            - name: RDM_API_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.pollerSecretName" (dict "root" $root "poller" $poller) }}
                  key: {{ include "nr-status.pollerSecretKey" (dict "root" $root "name" $name "poller" $poller) }}
            - name: INTERNAL_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.internalTokenSecretName" $root }}
                  key: {{ include "nr-status.internalTokenSecretKey" $root }}
            - name: API_INGEST_URL
              value: {{ printf "%s%s" (include "nr-status.apiBaseUrl" $root) $poller.ingestPath | quote }}
            {{- if $poller.sampleStationsPath }}
            # ldbws only: a SECOND api endpoint, listing which stations to
            # sample (crates/poller-ldbws/src/config.rs).
            - name: API_SAMPLE_STATIONS_URL
              value: {{ printf "%s%s" (include "nr-status.apiBaseUrl" $root) $poller.sampleStationsPath | quote }}
            {{- end }}
            {{- if $poller.numRows }}
            # ldbws only: LDBWS's own `numRows` query parameter.
            - name: NUM_ROWS
              value: {{ $poller.numRows | quote }}
            {{- end }}
            - name: POLL_INTERVAL_SECS
              value: {{ $poller.pollIntervalSecs | quote }}
            - name: RUST_LOG
              value: {{ $poller.logLevel | quote }}
            {{- with $poller.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with $poller.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with $poller.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with $poller.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with $poller.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
{{- end }}
```

- [ ] **Step 2: Confirm the default install deploys no pollers**

Run:

```bash
helm template nr-status charts/nr-status | grep -c 'poller' || true
```

Expected: `0`. A default install brings up postgres + api + aggregator + frontend and works immediately, rather than four pods failing against `*.example.invalid`.

- [ ] **Step 3: Confirm the `fail` guard fires for every poller**

Run each and check the exit code:

```bash
for p in incidents stations tocs ldbws; do
  helm template nr-status charts/nr-status --set pollers.$p.enabled=true > /dev/null 2>&1
  echo "$p exit=$?"
done
```

Expected: all four print `exit=1`. Run one without redirection to confirm the message names the right key:

```bash
helm template nr-status charts/nr-status --set pollers.tocs.enabled=true
```

Expected: an error containing `pollers.tocs.enabled is true but pollers.tocs.baseUrl is empty`.

- [ ] **Step 4: Enable all four and check every per-poller difference**

Write a reusable values file — Task 12 uses it again:

```bash
cat > /tmp/nr-status-all-pollers.yaml <<'EOF'
pollers:
  incidents:
    enabled: true
    baseUrl: https://rdm.example.com/incidents
  stations:
    enabled: true
    baseUrl: https://rdm.example.com/json/1.0
  tocs:
    enabled: true
    baseUrl: https://rdm.example.com/tocs
  ldbws:
    enabled: true
    baseUrl: https://rdm.example.com/LDBWS/api/20220120
EOF

helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --show-only templates/poller-deployments.yaml \
  | grep -E 'kind: Deployment|name: (RDM_INCIDENTS_BASE_URL|RDM_STATIONS_BASE_URL|RDM_TOCS_BASE_URL|LDBWS_BASE_URL|API_INGEST_URL|API_SAMPLE_STATIONS_URL|NUM_ROWS|POLL_INTERVAL_SECS)|value:'
```

Expected: four `kind: Deployment` blocks. Confirm by eye that

- `incidents` has `RDM_INCIDENTS_BASE_URL` and `API_INGEST_URL: "http://nr-status-api:8080/private/incidents"`, `POLL_INTERVAL_SECS: "300"`
- `stations` has `RDM_STATIONS_BASE_URL`, `.../private/stations`, `"86400"`
- `tocs` has `RDM_TOCS_BASE_URL`, `.../private/tocs`, `"86400"`
- `ldbws` has `LDBWS_BASE_URL`, `API_INGEST_URL: ".../private/station-samples"`, `API_SAMPLE_STATIONS_URL: ".../private/sample-stations"`, `NUM_ROWS: "10"`, `"60"`
- `API_SAMPLE_STATIONS_URL` and `NUM_ROWS` appear **exactly once each** across the whole output:

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  | grep -c 'API_SAMPLE_STATIONS_URL'
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  | grep -c 'NUM_ROWS'
```

Expected: `1` and `1`.

- [ ] **Step 5: Confirm the API keys land in the Secret, one per enabled poller**

Run:

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --show-only templates/secret.yaml | grep -c 'rdm-.*-api-key'
```

Expected: `4`.

Then confirm a per-poller `existingSecret` both removes its key and redirects its `secretKeyRef`:

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --set pollers.tocs.existingSecret=vault-tocs \
  --set pollers.tocs.existingSecretApiKeyKey=key \
  --show-only templates/secret.yaml | grep -c 'rdm-tocs-api-key' || true

helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --set pollers.tocs.existingSecret=vault-tocs \
  --set pollers.tocs.existingSecretApiKeyKey=key \
  --show-only templates/poller-deployments.yaml | grep -A4 'name: RDM_API_KEY' | grep -E 'vault-tocs|key: key'
```

Expected: `0` for the first; the second prints both `name: vault-tocs` and `key: key`.

- [ ] **Step 6: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add the four RDM poller Deployments from one ranged template"
```

---

### Task 8: Ingress

**Files:**
- Create: `charts/nr-status/templates/ingress.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `ingress` block)

**Interfaces:**
- Consumes: the frontend and api Services (Tasks 4, 6).
- Produces: `.Values.ingress.api.enabled`, read by Task 9's NetworkPolicy to decide whether the api accepts ingress-controller traffic.

- [ ] **Step 1: Append the `ingress` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# Ingress
# ---------------------------------------------------------------------------
# SEPARATE HOSTNAMES, NOT PATH-SPLITTING ONE HOST. The api serves its
# TfL-compatible routes at UNPREFIXED top-level paths -- GET /Line/... and
# GET /StopPoint/{crs}/Disruption -- alongside /public/... and /private/...
# (crates/api/src/routes/mod.rs explains why: clients built against TfL's own
# API must work unchanged). Splitting a single host by path prefix would
# either collide with Next.js's own routes or break that compatibility.
ingress:
  enabled: false
  # -- IngressClass name, e.g. "nginx" or "traefik". Empty uses the cluster
  # default IngressClass.
  className: ""
  # -- Annotations applied to the single Ingress object, e.g.
  # cert-manager.io/cluster-issuer: letsencrypt-prod
  annotations: {}
  frontend:
    enabled: true
    # -- Hostname for the web UI, e.g. status.example.com. Required when
    # ingress.enabled and ingress.frontend.enabled are both true.
    host: ""
  api:
    # SECURITY: enabling this exposes /private/* to the internet as well.
    # Those endpoints are protected ONLY by the X-Internal-Token shared
    # secret (crates/api/src/auth.rs) -- there is no other authentication in
    # front of them. If you do not need external API access, leave this off;
    # the frontend reaches the api over the in-cluster Service either way.
    enabled: false
    # -- Hostname for the api, e.g. api.example.com.
    host: ""
  # -- TLS blocks passed through verbatim. Shaped for cert-manager (pair it
  # with a cluster-issuer annotation above) but issuer-agnostic.
  tls: []
  # - secretName: nr-status-tls
  #   hosts:
  #     - status.example.com
  #     - api.example.com
```

- [ ] **Step 2: Create `templates/ingress.yaml`**

```yaml
{{- if .Values.ingress.enabled }}
{{- if and .Values.ingress.frontend.enabled (not .Values.ingress.frontend.host) }}
{{- fail "ingress.frontend.enabled is true but ingress.frontend.host is empty. Set it to the hostname the web UI should be served on, or set ingress.frontend.enabled=false." }}
{{- end }}
{{- if and .Values.ingress.api.enabled (not .Values.ingress.api.host) }}
{{- fail "ingress.api.enabled is true but ingress.api.host is empty. Set it to the hostname the api should be served on, or set ingress.api.enabled=false." }}
{{- end }}
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {{ include "nr-status.fullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "shared") | nindent 4 }}
  {{- with .Values.ingress.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
spec:
  {{- with .Values.ingress.className }}
  ingressClassName: {{ . | quote }}
  {{- end }}
  {{- with .Values.ingress.tls }}
  tls:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  rules:
    {{- if and .Values.ingress.frontend.enabled .Values.ingress.frontend.host }}
    - host: {{ .Values.ingress.frontend.host | quote }}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {{ include "nr-status.frontendFullname" . }}
                port:
                  number: {{ .Values.frontend.service.port }}
    {{- end }}
    {{- if and .Values.ingress.api.enabled .Values.ingress.api.host }}
    # Whole-host, not a path prefix -- see the values.yaml comment.
    - host: {{ .Values.ingress.api.host | quote }}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {{ include "nr-status.apiFullname" . }}
                port:
                  number: {{ .Values.api.service.port }}
    {{- end }}
{{- end }}
```

- [ ] **Step 3: Confirm nothing renders by default**

```bash
helm template nr-status charts/nr-status | grep -c 'kind: Ingress' || true
```

Expected: `0`.

- [ ] **Step 4: Render the frontend-only, both-hosts and TLS cases**

```bash
helm template nr-status charts/nr-status \
  --set ingress.enabled=true \
  --set ingress.frontend.host=status.example.com \
  --show-only templates/ingress.yaml
```

Expected: one Ingress with a single `host: "status.example.com"` rule backed by `nr-status-frontend:3000`, and no `tls:` block.

```bash
helm template nr-status charts/nr-status \
  --set ingress.enabled=true \
  --set ingress.className=nginx \
  --set ingress.frontend.host=status.example.com \
  --set ingress.api.enabled=true \
  --set ingress.api.host=api.example.com \
  --set 'ingress.annotations.cert-manager\.io/cluster-issuer=letsencrypt-prod' \
  --set 'ingress.tls[0].secretName=nr-status-tls' \
  --set 'ingress.tls[0].hosts[0]=status.example.com' \
  --set 'ingress.tls[0].hosts[1]=api.example.com' \
  --show-only templates/ingress.yaml
```

Expected: `ingressClassName: "nginx"`, the cert-manager annotation, a `tls:` block naming both hosts, and **two** `- host:` rules — `status.example.com` → `nr-status-frontend:3000`, `api.example.com` → `nr-status-api:8080`. Both use `path: /` with `pathType: Prefix`.

- [ ] **Step 5: Confirm the host `fail` guards**

```bash
helm template nr-status charts/nr-status --set ingress.enabled=true > /dev/null 2>&1; echo "exit=$?"
helm template nr-status charts/nr-status --set ingress.enabled=true \
  --set ingress.frontend.host=status.example.com --set ingress.api.enabled=true > /dev/null 2>&1; echo "exit=$?"
```

Expected: `exit=1` both times.

- [ ] **Step 6: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status \
  --set ingress.enabled=true --set ingress.frontend.host=status.example.com \
  | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add Ingress with separate frontend and api hostnames"
```

---

### Task 9: Optional NetworkPolicy

**Files:**
- Create: `charts/nr-status/templates/networkpolicy.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `networkPolicy` block)

**Interfaces:**
- Consumes: the `app.kubernetes.io/component` values fixed in Global Constraints, and `.Values.ingress.api.enabled` (Task 8).
- Produces: nothing consumed elsewhere.

- [ ] **Step 1: Append the `networkPolicy` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# NetworkPolicy (optional)
# ---------------------------------------------------------------------------
# Off by default: many clusters run a CNI that does not enforce
# NetworkPolicy at all, and a silently-unenforced policy is worse than an
# absent one because it looks like protection.
#
# EGRESS IS DELIBERATELY UNRESTRICTED. The pollers must reach arbitrary
# external Rail Data Marketplace hosts, and constraining that would mean
# making every operator enumerate them.
networkPolicy:
  enabled: false
  # -- Namespace your ingress controller runs in. Matched via the automatic
  # `kubernetes.io/metadata.name` namespace label. Only used when
  # ingress.enabled is true.
  ingressControllerNamespace: ingress-nginx
```

- [ ] **Step 2: Create `templates/networkpolicy.yaml`**

```yaml
{{- if .Values.networkPolicy.enabled }}
{{- $root := . -}}
{{/*
Reusable pieces. Each policy below is default-deny for ingress (an empty or
absent `ingress:` list denies everything) plus the specific allows the
service actually needs.
*/}}
{{- if .Values.postgresql.enabled }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "nr-status.postgresFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "postgres") | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "postgres") | nindent 6 }}
  policyTypes:
    - Ingress
  ingress:
    # Only the api and the aggregator talk to Postgres. The pollers never do.
    - from:
        - podSelector:
            matchExpressions:
              - key: app.kubernetes.io/instance
                operator: In
                values:
                  - {{ .Release.Name }}
              - key: app.kubernetes.io/component
                operator: In
                values:
                  - api
                  - aggregator
      ports:
        - protocol: TCP
          port: {{ .Values.postgresql.service.port }}
---
{{- end }}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "nr-status.apiFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "api") | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "api") | nindent 6 }}
  policyTypes:
    - Ingress
  ingress:
    - from:
        - podSelector:
            matchExpressions:
              - key: app.kubernetes.io/instance
                operator: In
                values:
                  - {{ .Release.Name }}
              - key: app.kubernetes.io/component
                operator: In
                values:
                  - frontend
                  {{- range $name, $poller := .Values.pollers }}
                  {{- if $poller.enabled }}
                  - poller-{{ $name }}
                  {{- end }}
                  {{- end }}
      {{- if and .Values.ingress.enabled .Values.ingress.api.enabled }}
        # The api is published externally, so the ingress controller must
        # reach it too. SECURITY: this also exposes /private/*, guarded only
        # by the X-Internal-Token shared secret.
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: {{ .Values.networkPolicy.ingressControllerNamespace | quote }}
      {{- end }}
      ports:
        - protocol: TCP
          port: {{ .Values.api.service.port }}
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "nr-status.frontendFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "frontend") | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "frontend") | nindent 6 }}
  policyTypes:
    - Ingress
  {{- if and .Values.ingress.enabled .Values.ingress.frontend.enabled }}
  ingress:
    - from:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: {{ .Values.networkPolicy.ingressControllerNamespace | quote }}
      ports:
        - protocol: TCP
          port: {{ .Values.frontend.service.port }}
  {{- else }}
  # No Ingress in front of the frontend, so nothing outside the pod needs to
  # reach it. Omitting `ingress` entirely is a default-deny.
  {{- end }}
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ printf "%s-aggregator" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "aggregator") | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "aggregator") | nindent 6 }}
  policyTypes:
    - Ingress
  # Default-deny: the aggregator exposes no listener at all.
{{- range $name, $poller := .Values.pollers }}
{{- if $poller.enabled }}
---
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ printf "%s-poller-%s" (include "nr-status.fullname" $root) $name | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" $root "component" (printf "poller-%s" $name)) | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" $root "component" (printf "poller-%s" $name)) | nindent 6 }}
  policyTypes:
    - Ingress
  # Default-deny: pollers expose no listener.
{{- end }}
{{- end }}
{{- end }}
```

- [ ] **Step 3: Confirm nothing renders by default**

```bash
helm template nr-status charts/nr-status | grep -c 'kind: NetworkPolicy' || true
```

Expected: `0`.

- [ ] **Step 4: Render the enabled case and count the policies**

```bash
helm template nr-status charts/nr-status --set networkPolicy.enabled=true \
  | grep -c 'kind: NetworkPolicy'
```

Expected: `4` (postgres, api, frontend, aggregator; no pollers are enabled).

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --set networkPolicy.enabled=true | grep -c 'kind: NetworkPolicy'
```

Expected: `8` (the four above plus one per poller).

- [ ] **Step 5: Confirm the ingress-controller allows appear only when the Ingress does**

```bash
helm template nr-status charts/nr-status --set networkPolicy.enabled=true \
  | grep -c 'kubernetes.io/metadata.name' || true

helm template nr-status charts/nr-status --set networkPolicy.enabled=true \
  --set ingress.enabled=true --set ingress.frontend.host=status.example.com \
  --set ingress.api.enabled=true --set ingress.api.host=api.example.com \
  | grep -c 'kubernetes.io/metadata.name'
```

Expected: `0` for the first (no Ingress, so no external source is allowed), `2` for the second (one in the api policy, one in the frontend policy).

- [ ] **Step 6: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml \
  --set networkPolicy.enabled=true \
  | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add optional default-deny NetworkPolicies, off by default"
```

---

### Task 10: NOTES.txt and the `helm test` health-probe pod

**Files:**
- Create: `charts/nr-status/templates/NOTES.txt`
- Create: `charts/nr-status/templates/tests/test-api-health.yaml`
- Modify: `charts/nr-status/values.yaml` (append the `tests` block)

**Interfaces:**
- Consumes: `nr-status.apiBaseUrl`, `.Values.api.probes.path` (Task 4), the frontend Service (Task 6), the ingress values (Task 8).
- Produces: nothing consumed elsewhere.

- [ ] **Step 1: Append the `tests` block to `values.yaml`**

```yaml

# ---------------------------------------------------------------------------
# helm test
# ---------------------------------------------------------------------------
tests:
  # -- Render the `helm test` pod. Running it requires a live cluster with
  # the images available.
  enabled: true
  image:
    # -- Empty means "reuse the api image", which already ships curl
    # (docker/api.Dockerfile installs it for the compose HEALTHCHECK). That
    # keeps `helm test` working in an air-gapped cluster with no extra pull.
    # Set a repository here only if you want a different probe image.
    repository: ""
    tag: ""
    pullPolicy: IfNotPresent
```

- [ ] **Step 2: Create `templates/tests/test-api-health.yaml`**

```yaml
{{- if .Values.tests.enabled }}
apiVersion: v1
kind: Pod
metadata:
  name: {{ printf "%s-test-api-health" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "test") | nindent 4 }}
  annotations:
    helm.sh/hook: test
    helm.sh/hook-delete-policy: before-hook-creation,hook-succeeded
spec:
  restartPolicy: Never
  automountServiceAccountToken: false
  {{- with .Values.imagePullSecrets }}
  imagePullSecrets:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  securityContext:
    {{- include "nr-status.podSecurityContext" (dict "override" (dict)) | nindent 4 }}
  containers:
    - name: probe
      {{- if .Values.tests.image.repository }}
      image: {{ include "nr-status.image" (dict "root" . "image" .Values.tests.image) | quote }}
      {{- else }}
      # Default: the api image, which already contains curl.
      image: {{ include "nr-status.image" (dict "root" . "image" .Values.api.image) | quote }}
      {{- end }}
      imagePullPolicy: {{ .Values.tests.image.pullPolicy }}
      securityContext:
        {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 8 }}
      # Overrides the image's ENTRYPOINT (/usr/local/bin/api) with a one-shot
      # probe. -f makes curl exit non-zero on any HTTP error status, which is
      # what `helm test` reads as failure.
      command:
        - curl
      args:
        - -fsS
        - --max-time
        - "10"
        - {{ printf "%s%s" (include "nr-status.apiBaseUrl" .) .Values.api.probes.path | quote }}
{{- end }}
```

- [ ] **Step 3: Create `templates/NOTES.txt`**

```
nr-status {{ .Chart.AppVersion }} has been deployed as release "{{ .Release.Name }}" in namespace "{{ .Release.Namespace }}".

Watch it come up:

  kubectl get pods -n {{ .Release.Namespace }} -l app.kubernetes.io/instance={{ .Release.Name }} -w

The api runs its database migrations in-process before it binds, so the api
pod can legitimately take up to {{ mul .Values.api.probes.startup.periodSeconds .Values.api.probes.startup.failureThreshold }}s to become ready on a first install.

Reach the web UI:
{{- if and .Values.ingress.enabled .Values.ingress.frontend.enabled .Values.ingress.frontend.host }}

  http{{ if .Values.ingress.tls }}s{{ end }}://{{ .Values.ingress.frontend.host }}/
{{- else }}

  kubectl port-forward -n {{ .Release.Namespace }} svc/{{ include "nr-status.frontendFullname" . }} {{ .Values.frontend.service.port }}:{{ .Values.frontend.service.port }}
  # then open http://localhost:{{ .Values.frontend.service.port }}/
{{- end }}

Reach the api:
{{- if and .Values.ingress.enabled .Values.ingress.api.enabled .Values.ingress.api.host }}

  http{{ if .Values.ingress.tls }}s{{ end }}://{{ .Values.ingress.api.host }}{{ .Values.api.probes.path }}

  ############################################################################
  # SECURITY WARNING
  #
  # ingress.api.host is enabled, so /private/* is reachable from outside the
  # cluster as well. Those ingestion endpoints are protected ONLY by the
  # X-Internal-Token shared secret (crates/api/src/auth.rs) -- there is no
  # other authentication in front of them. If you do not need external API
  # access, set ingress.api.enabled=false; the frontend reaches the api over
  # the in-cluster Service either way.
  ############################################################################
{{- else }}

  kubectl port-forward -n {{ .Release.Namespace }} svc/{{ include "nr-status.apiFullname" . }} {{ .Values.api.service.port }}:{{ .Values.api.service.port }}
  # then: curl http://localhost:{{ .Values.api.service.port }}{{ .Values.api.probes.path }}
{{- end }}

{{- if not .Values.secrets.existingSecret }}

Read the internal token (generated on first install, preserved on upgrade):

  kubectl get secret -n {{ .Release.Namespace }} {{ include "nr-status.secretName" . }} -o jsonpath='{.data.internal-token}' | base64 -d; echo
{{- end }}

{{- $anyPoller := false }}
{{- range $name, $poller := .Values.pollers }}{{- if $poller.enabled }}{{- $anyPoller = true }}{{- end }}{{- end }}
{{- if not $anyPoller }}

No RDM pollers are enabled, so no external feed data is being ingested. This
is the default: as documented in .env.example, no confirmed Rail Data
Marketplace endpoint exists for any of the four feeds yet. Once you have real
account data, enable a poller with, for example:

  --set pollers.incidents.enabled=true \
  --set pollers.incidents.baseUrl=https://<your-rdm-host>/... \
  --set pollers.incidents.apiKey=<your-key>
{{- end }}

{{- if .Values.tests.enabled }}

Verify the api is serving:

  helm test {{ .Release.Name }} -n {{ .Release.Namespace }}
{{- end }}
```

- [ ] **Step 4: Verify the test pod renders and targets the right URL**

```bash
helm template nr-status charts/nr-status --show-only templates/tests/test-api-health.yaml
```

Expected: a `kind: Pod` with `helm.sh/hook: test`, `restartPolicy: Never`, image `nr-status/api:0.1.0`, and the final arg `"http://nr-status-api:8080/public/health"`.

- [ ] **Step 5: Verify NOTES.txt renders in both the port-forward and the ingress case**

`helm template` executes NOTES.txt but does not print it, so render it with `--dry-run` instead (no cluster needed):

```bash
helm install nr-status charts/nr-status --dry-run=client -n nr-status | sed -n '/NOTES/,$p'
```

Expected: the port-forward instructions, the internal-token command, and the "No RDM pollers are enabled" paragraph. Then:

```bash
helm install nr-status charts/nr-status --dry-run=client -n nr-status \
  --set ingress.enabled=true \
  --set ingress.frontend.host=status.example.com \
  --set ingress.api.enabled=true --set ingress.api.host=api.example.com \
  | sed -n '/NOTES/,$p'
```

Expected: both hostnames, and the `SECURITY WARNING` block about `/private/*`.

- [ ] **Step 6: Validate, lint and commit**

```bash
helm template nr-status charts/nr-status | kubeconform -strict -summary -kubernetes-version 1.31.0 -
helm lint charts/nr-status
git add charts/nr-status
git commit -m "Add NOTES.txt and a helm test pod probing /public/health"
```

---

### Task 11: values-example.yaml and README.md

**Files:**
- Create: `charts/nr-status/values-example.yaml`
- Create: `charts/nr-status/README.md`

**Interfaces:**
- Consumes: every value key defined in Tasks 1-10.
- Produces: nothing consumed by templates. `values-example.yaml` is used as an input by Task 12's acceptance sweep.

- [ ] **Step 1: Create `charts/nr-status/values-example.yaml`**

```yaml
# A filled-in "real deployment" example. Render it with:
#
#   helm template nr-status ./charts/nr-status -f charts/nr-status/values-example.yaml
#
# Every RDM base URL below is still a placeholder host -- as of this chart's
# writing no confirmed Rail Data Marketplace endpoint exists for any of the
# four feeds (see .env.example in the repository root). Replace them, and the
# API keys, with real values before installing this for real.

imagePullSecrets:
  - name: registry-creds

api:
  image:
    repository: registry.example.com/nr-status/api
    tag: "0.1.0"
  replicaCount: 2
  logLevel: info
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
    limits:
      memory: 512Mi

aggregator:
  image:
    repository: registry.example.com/nr-status/aggregator
    tag: "0.1.0"
  pollIntervalSecs: 60
  historyRetentionDays: 30
  resources:
    requests:
      cpu: 50m
      memory: 128Mi
    limits:
      memory: 512Mi

frontend:
  image:
    repository: registry.example.com/nr-status/frontend
    tag: "0.1.0"
  replicaCount: 2
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      memory: 768Mi

postgresql:
  enabled: true
  auth:
    username: nr_status
    database: nr_status
    # Left empty on purpose: generated on first install and preserved across
    # upgrades. Set `existingSecret` instead if an external secret manager
    # owns this credential.
    password: ""
  persistence:
    enabled: true
    size: 20Gi
    storageClass: fast-ssd
  resources:
    requests:
      cpu: 250m
      memory: 512Mi
    limits:
      memory: 2Gi

secrets:
  # Left empty on purpose: generated on first install, preserved on upgrade.
  internalToken: ""

pollers:
  incidents:
    enabled: true
    image:
      repository: registry.example.com/nr-status/poller-incidents
      tag: "0.1.0"
    baseUrl: https://rdm.example.com/incidents
    apiKey: replace-me-rdm-incidents-key
    pollIntervalSecs: 300
  stations:
    enabled: true
    image:
      repository: registry.example.com/nr-status/poller-stations
      tag: "0.1.0"
    baseUrl: https://rdm.example.com/json/1.0
    apiKey: replace-me-rdm-stations-key
    pollIntervalSecs: 86400
  tocs:
    enabled: true
    image:
      repository: registry.example.com/nr-status/poller-tocs
      tag: "0.1.0"
    baseUrl: https://rdm.example.com/tocs
    apiKey: replace-me-rdm-tocs-key
    pollIntervalSecs: 86400
  ldbws:
    enabled: true
    image:
      repository: registry.example.com/nr-status/poller-ldbws
      tag: "0.1.0"
    baseUrl: https://rdm.example.com/LDBWS/api/20220120
    apiKey: replace-me-rdm-ldbws-key
    numRows: 10
    pollIntervalSecs: 60

ingress:
  enabled: true
  className: nginx
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  frontend:
    enabled: true
    host: status.example.com
  api:
    # Left off deliberately: enabling it publishes /private/*, which is
    # guarded only by the X-Internal-Token shared secret.
    enabled: false
    host: ""
  tls:
    - secretName: nr-status-tls
      hosts:
        - status.example.com

networkPolicy:
  enabled: true
  ingressControllerNamespace: ingress-nginx
```

- [ ] **Step 2: Create `charts/nr-status/README.md`**

Write the file with these sections, in this order. Content requirements per
section are exact; prose wording is the implementer's.

1. **Title and one-paragraph summary** — what the chart deploys (Postgres, api, aggregator, frontend, four optional pollers), and that it has no subchart dependencies and needs no `helm dependency update`.
2. **Prerequisites** — Kubernetes >= 1.23, Helm 3.8+ or 4.x, a default StorageClass (or `postgresql.persistence.storageClass`), and **images already present in a registry the cluster can pull from** (this chart builds nothing).
3. **Building and pushing the images (manual)** — a table mapping each Dockerfile to its default image repository, and the exact loop:

   ```bash
   REG=registry.example.com/nr-status
   TAG=0.1.0
   docker build -f docker/api.Dockerfile              -t $REG/api:$TAG .
   docker build -f docker/aggregator.Dockerfile       -t $REG/aggregator:$TAG .
   docker build -f docker/poller-incidents.Dockerfile -t $REG/poller-incidents:$TAG .
   docker build -f docker/poller-stations.Dockerfile  -t $REG/poller-stations:$TAG .
   docker build -f docker/poller-tocs.Dockerfile      -t $REG/poller-tocs:$TAG .
   docker build -f docker/poller-ldbws.Dockerfile     -t $REG/poller-ldbws:$TAG .
   docker build -f frontend/Dockerfile --target runtime-prod -t $REG/frontend:$TAG .
   for i in api aggregator poller-incidents poller-stations poller-tocs poller-ldbws frontend; do
     docker push $REG/$i:$TAG
   done
   ```

   State explicitly that there is **no** build/publish pipeline in this repository and the chart does not add one.
4. **Install** —

   ```bash
   helm install nr-status ./charts/nr-status -n nr-status --create-namespace
   ```

   plus the `-f values-example.yaml` variant, and a note that a default install brings up postgres + api + aggregator + frontend with all four pollers off.
5. **Upgrade** — `helm upgrade nr-status ./charts/nr-status -n nr-status`, and the warning below.
6. **Generated secrets and the `lookup` limitation** — must state all of:
   - `postgres-password` and `internal-token` are generated with `randAlphaNum 32` when left empty and no `existingSecret` is given.
   - They are **preserved** on `helm upgrade` because the Secret template reads the live Secret back with `lookup`.
   - `lookup` returns nothing during `helm template` and `--dry-run`, so an offline render shows a **different** generated value each time. This is cosmetic for dry runs, but it means **`helm template | kubectl apply` is not a supported install path when relying on generated secrets** — set explicit values or use `existingSecret` for that workflow.
   - How to read the generated values back out with `kubectl get secret ... -o jsonpath=... | base64 -d`.
7. **Using externally-managed secrets** — a worked example pointing `postgresql.auth.existingSecret`, `secrets.existingSecret` and each `pollers.<name>.existingSecret` at Secrets produced by External Secrets Operator / Vault / SOPS, noting that any key supplied this way is omitted from the chart-rendered Secret entirely.
8. **Using an external database** — `postgresql.enabled: false` plus **either** `externalDatabase.existingSecret` + `existingSecretUrlKey` (preferred) **or** `externalDatabase.url`; setting neither aborts rendering with an explicit message.
9. **Password encoding caveat** — `DATABASE_URL` is a URL, so a password containing `@ : / ? # [ ] %` must be percent-encoded by the operator. The chart cannot do it, because with `existingSecret` it never sees the value. Generated passwords are alphanumeric, so the default path is unaffected.
10. **Ingress** — the two-hostname design and *why* (the api serves TfL-compatible routes at unprefixed top-level paths, so path-splitting one host would break them or collide with Next.js routes), plus the security warning that `ingress.api.enabled` also publishes `/private/*`, guarded only by `X-Internal-Token`.
11. **NetworkPolicy** — off by default and why; what it allows when on; and that egress is deliberately unrestricted because the pollers must reach arbitrary external RDM hosts.
12. **Enabling the pollers** — the per-poller table (base-URL env var, ingest path, default cadence) reproduced from this plan's Task 7, plus the note that all four are off by default because no confirmed RDM endpoint exists yet, and that enabling one without a `baseUrl` aborts the render.
13. **Values reference** — a table of every top-level and second-level key with its default and a one-line description. Generate the skeleton from `values.yaml` and fill it in; do not leave any row blank.
14. **Testing** — `helm test nr-status -n nr-status`, what the pod does (`curl -fsS <api>/public/health`), and that it needs a live cluster with the images available.
15. **Uninstall** — `helm uninstall nr-status -n nr-status`, plus the warning that the PVC created by `volumeClaimTemplates` **survives** uninstall and must be deleted manually if the data is not wanted:

    ```bash
    kubectl delete pvc -n nr-status -l app.kubernetes.io/instance=nr-status
    ```
16. **Not in scope** — a short list restating the design's non-goals: no image build/publish pipeline, no HPA, no Postgres backup/restore/replication, no metrics or ServiceMonitor.

- [ ] **Step 3: Verify `values-example.yaml` renders and validates**

```bash
helm template nr-status charts/nr-status -f charts/nr-status/values-example.yaml \
  | kubeconform -strict -summary -kubernetes-version 1.31.0 -
```

Expected: `Invalid: 0, Errors: 0`.

- [ ] **Step 4: Verify the example's object inventory**

```bash
helm template nr-status charts/nr-status -f charts/nr-status/values-example.yaml \
  | grep -E '^kind:' | sort | uniq -c
```

Expected: 7 `Deployment` (api, aggregator, frontend, four pollers), 1 `Ingress`, 8 `NetworkPolicy`, 1 `Pod` (the helm test hook), 1 `Secret`, 1 `ServiceAccount`, 3 `Service` (postgres, api, frontend), 1 `StatefulSet`.

- [ ] **Step 5: Confirm the README covers every required section**

```bash
grep -c '^#' charts/nr-status/README.md
grep -nE 'lookup|percent-encode|X-Internal-Token|existingSecret|externalDatabase|helm test|pvc' charts/nr-status/README.md
```

Expected: every one of `lookup`, `percent-encode`, `X-Internal-Token`, `existingSecret`, `externalDatabase`, `helm test`, `pvc` appears at least once. If any is missing, the corresponding section from Step 2 was skipped — go back and write it.

- [ ] **Step 6: Lint and commit**

```bash
helm lint charts/nr-status -f charts/nr-status/values-example.yaml
git add charts/nr-status
git commit -m "Add values-example.yaml and chart README"
```

---

### Task 12: Full acceptance sweep against the spec's Verification section

No new files. This task runs every acceptance criterion the design spec
lists, in one pass, and is the gate for calling the chart done. Do not mark
any step complete without having seen the expected output.

**Files:**
- Modify: none (fixes go back into whichever template failed, in a separate commit)

- [ ] **Step 1: `helm lint` clean**

```bash
helm lint charts/nr-status
helm lint charts/nr-status -f charts/nr-status/values-example.yaml
```

Expected: `0 chart(s) failed` from both. The only acceptable non-error output is `[INFO] Chart.yaml: icon is recommended`.

- [ ] **Step 2: All five required render scenarios succeed**

```bash
set -e
echo "1. defaults"
helm template nr-status charts/nr-status > /dev/null

echo "2. all pollers enabled with base URLs"
helm template nr-status charts/nr-status -f /tmp/nr-status-all-pollers.yaml > /dev/null

echo "3. external database"
helm template nr-status charts/nr-status \
  --set postgresql.enabled=false \
  --set externalDatabase.url='postgres://u:p@db.example.com:5432/nr_status' > /dev/null

echo "4. ingress with TLS"
helm template nr-status charts/nr-status \
  --set ingress.enabled=true \
  --set ingress.className=nginx \
  --set ingress.frontend.host=status.example.com \
  --set ingress.api.enabled=true \
  --set ingress.api.host=api.example.com \
  --set 'ingress.tls[0].secretName=nr-status-tls' \
  --set 'ingress.tls[0].hosts[0]=status.example.com' \
  --set 'ingress.tls[0].hosts[1]=api.example.com' > /dev/null

echo "5. networkPolicy enabled"
helm template nr-status charts/nr-status --set networkPolicy.enabled=true > /dev/null

echo "all five rendered"
set +e
```

Expected: the script prints all five labels and `all five rendered`, with no `Error:` lines.

- [ ] **Step 3: The two `fail` guards abort with the intended messages**

```bash
echo "--- poller enabled without baseUrl ---"
helm template nr-status charts/nr-status --set pollers.incidents.enabled=true; echo "exit=$?"
echo "--- postgresql disabled with no externalDatabase ---"
helm template nr-status charts/nr-status --set postgresql.enabled=false; echo "exit=$?"
```

Expected: both print `exit=1`. The first message contains `pollers.incidents.enabled is true but pollers.incidents.baseUrl is empty`; the second contains `postgresql.enabled is false but no external database is configured`.

- [ ] **Step 4: Rendered output validates against a current Kubernetes schema**

```bash
for args in \
  "" \
  "-f /tmp/nr-status-all-pollers.yaml" \
  "-f charts/nr-status/values-example.yaml" \
  "--set networkPolicy.enabled=true" ; do
  echo "--- helm template $args ---"
  helm template nr-status charts/nr-status $args \
    | kubeconform -strict -summary -kubernetes-version 1.31.0 -
done
```

Expected: every run reports `Invalid: 0, Errors: 0, Skipped: 0`.

If `kubeconform` is unavailable on the machine, substitute the weaker
fallback and say in the completion report that the stronger check was not
run:

```bash
helm template nr-status charts/nr-status | kubectl apply --dry-run=client --validate=false -f -
```

- [ ] **Step 5: No secret value appears outside the Secret object**

Render with sentinel values that could not occur by chance, then confirm they
appear nowhere at all in plaintext (the Secret stores them base64-encoded):

```bash
helm template nr-status charts/nr-status \
  -f /tmp/nr-status-all-pollers.yaml \
  --set postgresql.auth.password=SENTINELPGPASS \
  --set secrets.internalToken=SENTINELTOKEN \
  --set pollers.incidents.apiKey=SENTINELRDMKEY \
  > /tmp/nr-status-rendered.yaml

grep -c 'SENTINELPGPASS\|SENTINELTOKEN\|SENTINELRDMKEY' /tmp/nr-status-rendered.yaml || true
```

Expected: `0`.

Now confirm the base64 forms appear **only** inside the Secret document:

```bash
for s in SENTINELPGPASS SENTINELTOKEN SENTINELRDMKEY; do
  b=$(printf '%s' "$s" | base64)
  echo "$s -> $(grep -c "$b" /tmp/nr-status-rendered.yaml) occurrence(s)"
done

awk '/^---/{doc++} {print doc": "$0}' /tmp/nr-status-rendered.yaml \
  | grep -E "$(printf '%s' SENTINELPGPASS | base64)" \
  | head -1
```

Expected: each sentinel's base64 form occurs exactly `1` time, and the
document it occurs in is the one whose `kind:` is `Secret` (check by
searching that document number in the numbered output).

Finally, confirm the api and aggregator carry `$(PGPASSWORD)` rather than a
literal:

```bash
grep -c 'postgres://nr_status:\$(PGPASSWORD)@nr-status-postgres:5432/nr_status' /tmp/nr-status-rendered.yaml
```

Expected: `2` — one for the api Deployment, one for the aggregator Deployment.

- [ ] **Step 6: Generated secrets are stable across a re-render only via the cluster (documented limitation check)**

```bash
helm template nr-status charts/nr-status --show-only templates/secret.yaml | grep 'internal-token' > /tmp/a
helm template nr-status charts/nr-status --show-only templates/secret.yaml | grep 'internal-token' > /tmp/b
diff /tmp/a /tmp/b || echo "DIFFERENT (expected offline: lookup returns nothing during helm template)"
```

Expected: the two differ, printing the `DIFFERENT` line. This is the exact
behaviour README.md's "lookup limitation" section documents — it is not a
bug, but confirm the README says so:

```bash
grep -n 'helm template | kubectl apply' charts/nr-status/README.md
```

Expected: at least one hit.

- [ ] **Step 7: Confirm the `helm test` pod is defined**

```bash
helm template nr-status charts/nr-status \
  --show-only templates/tests/test-api-health.yaml | grep -E 'kind: Pod|helm.sh/hook|public/health'
```

Expected: `kind: Pod`, `helm.sh/hook: test`, and the `/public/health` URL.

Actually *running* `helm test` needs a live cluster with the images
available, which is an operator step and explicitly **not** part of this
work's acceptance criteria — a live end-to-end install is out of scope
because no published images exist yet. Do not attempt it.

- [ ] **Step 8: Confirm the chart's file inventory matches the design**

```bash
find charts/nr-status -type f | sort
```

Expected exactly:

```
charts/nr-status/Chart.yaml
charts/nr-status/README.md
charts/nr-status/templates/NOTES.txt
charts/nr-status/templates/_helpers.tpl
charts/nr-status/templates/aggregator-deployment.yaml
charts/nr-status/templates/api-deployment.yaml
charts/nr-status/templates/api-service.yaml
charts/nr-status/templates/frontend-deployment.yaml
charts/nr-status/templates/frontend-service.yaml
charts/nr-status/templates/ingress.yaml
charts/nr-status/templates/networkpolicy.yaml
charts/nr-status/templates/poller-deployments.yaml
charts/nr-status/templates/postgres-service.yaml
charts/nr-status/templates/postgres-statefulset.yaml
charts/nr-status/templates/secret.yaml
charts/nr-status/templates/serviceaccount.yaml
charts/nr-status/templates/tests/test-api-health.yaml
charts/nr-status/values-example.yaml
charts/nr-status/values.yaml
```

- [ ] **Step 9: Confirm nothing outside the chart changed**

```bash
git status --porcelain
# Diff everything since the commit that first added the chart.
BASE=$(git log --format=%H -1 --diff-filter=A -- charts/nr-status/Chart.yaml)^
git diff --stat $BASE..HEAD -- . ':(exclude)charts/nr-status' ':(exclude)docs/superpowers/plans'
```

Expected: `git status --porcelain` is empty (everything was committed task by
task), and the diff outside `charts/nr-status` is empty — no application
code, no Dockerfile, no `docker-compose.yml` was touched.

- [ ] **Step 10: Clean up scratch files**

```bash
rm -f /tmp/nr-status-all-pollers.yaml /tmp/nr-status-rendered.yaml /tmp/a /tmp/b
```

- [ ] **Step 11: Final commit if any fixes were needed**

If Steps 1-9 required template changes:

```bash
git add charts/nr-status
git commit -m "Fix chart issues found in the acceptance sweep"
```

Otherwise there is nothing to commit and the chart is complete.

---

## Acceptance-criteria traceability

Every bullet in the design spec's "Verification" section, and where it is checked:

| Spec criterion | Checked in |
|---|---|
| `helm lint charts/nr-status` clean | Task 12 Step 1 (and at the end of every earlier task) |
| `helm template` renders for defaults | Task 12 Step 2 case 1 |
| ... all pollers enabled with base URLs | Task 7 Step 4, Task 12 Step 2 case 2 |
| ... `postgresql.enabled: false` + `externalDatabase.url` | Task 4 Step 7, Task 12 Step 2 case 3 |
| ... ingress enabled with TLS | Task 8 Step 4, Task 12 Step 2 case 4 |
| ... `networkPolicy.enabled: true` | Task 9 Step 4, Task 12 Step 2 case 5 |
| poller enabled with no `baseUrl` **fails** with the intended message | Task 7 Step 3, Task 12 Step 3 |
| `postgresql.enabled: false` with no `externalDatabase` **fails** | Task 4 Step 7, Task 12 Step 3 |
| Rendered output passes `kubeconform` | Task 12 Step 4 (and at the end of Tasks 3-10) |
| No secret value appears outside the Secret object | Task 4 Step 6, Task 12 Step 5 |
| `helm test` pod is defined (running it is an operator step) | Task 10 Step 4, Task 12 Step 7 |
| Live end-to-end install is **not** an acceptance criterion | Task 12 Step 7, stated explicitly |
