# Local Dev OIDC Server (Authentik) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a developer bring up a fully working local OIDC identity
provider (Authentik) as an opt-in part of either deployment path this repo
already supports — `docker compose` or the `charts/nr-status` Helm chart —
so `SSO_ISSUER_URL`/`SSO_CLIENT_ID`/`SSO_CLIENT_SECRET`/`SSO_REDIRECT_URL`/
`SSO_POST_LOGIN_REDIRECT_URL` (`crates/api/src/data/config.rs:58-106`) point
at something real and the login flow added by
`docs/superpowers/plans/2026-08-28-user-accounts-sso.md` can be exercised
end to end with zero manual IdP-side clicking — while a developer who
already has a real external IdP is completely unaffected either way, per
`docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md`.

**Architecture:** Two independent, strictly opt-in paths into the same
local Authentik IdP, neither touching `crates/api` or any other application
code:

1. **`docker compose`** — a third, purely additive compose file
   (`docker-compose.authentik.yml`) a developer appends to `COMPOSE_FILE`,
   bringing up `authentik-postgres` (dedicated, `postgres:16-alpine`),
   `authentik-server` and `authentik-worker`
   (`ghcr.io/goauthentik/server:2026.8.0`), bootstrapped declaratively by
   two blueprint files (`authentik-blueprints/`) bind-mounted read-only —
   no bootstrap admin, open self-service signup, a fixed deterministic
   OAuth2 client matching this app's own `SSO_*` config.
2. **Helm (`charts/nr-status/`)** — a new `devAuthentik.enabled` values.yaml
   flag (default `false`) rendering hand-rolled plain Kubernetes manifests
   (explicitly *not* a subchart dependency on `goauthentik/helm` — see
   Global Constraints), a `ConfigMap` carrying byte-for-byte copies of the
   same two blueprint files, a dedicated Postgres `StatefulSet`, `server`/
   `worker` `Deployment`s, and a `NodePort` `Service` whose `port` and
   `nodePort` are deliberately identical. When `devAuthentik.enabled` and
   `api.sso.*` is left empty, the chart computes `api.sso.*` from
   `devAuthentik.*` and a `hostAliases` entry on the `api` `Deployment`
   makes the in-cluster OIDC discovery fetch present the same Host header
   the developer's browser sends — the harder, Kubernetes-flavored version
   of the same discovery-reachability problem the compose path solves with
   one Docker network alias.

Both paths share one blueprint definition (kept in sync as two physical
copies, since Helm's `.Files.Get` cannot read outside the chart directory)
and are built in that order — compose complete and independently verified
first, Helm building on the now-proven blueprint content second — per this
plan's own task ordering below.

**Tech Stack:** `ghcr.io/goauthentik/server:2026.8.0` (a Django application;
no Redis — current Authentik doesn't use one), `postgres:16-alpine`
(dedicated to Authentik, not shared with this app's own Postgres), Authentik
blueprint YAML (a declarative first-boot configuration format, not
application code), Docker Compose (a third overlay file, no Compose
profiles), Helm 4 plain Kubernetes manifests (no subchart, no `helm
dependency update`). No new Rust/TypeScript code, no changes to
`crates/api` or any other crate, no changes to `frontend/`.

**Spec:** `docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` —
read in full before starting; this plan does not restate its research, only
its resulting decisions. Also load-bearing:
`docs/superpowers/specs/2026-08-18-helm-chart-design.md` (the existing Helm
chart's own design and conventions this plan's Helm tasks must match
exactly) and `docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`
(the config surface this plan populates).

## What this plan is not

This is infra/config work, not application code — there is no `cargo test`
framing that fits it. Every task's verification step is one or more of:
`docker compose ... config` (validates YAML/interpolation without starting
anything), `helm lint`/`helm template` (renders without a cluster), a
rendered-output grep or `diff`, or — where genuinely meaningful and stated
explicitly as such — actually bringing up a real stack (`docker compose up`
against real container images, or a real kind/minikube/k3d cluster) and
confirming Authentik boots, a blueprint applies, and a login round-trip
succeeds. Every task below says plainly which of its verification steps are
static checks runnable anywhere versus which need real infrastructure this
environment may or may not have. **If a live-infrastructure step cannot be
run, that must be reported as "not verified here," never assumed to
pass.**

This plan also does not pretend the design doc's two open, unresolved risks
are solved. Both are carried forward explicitly into the tasks that
implement them:

- **The `*.localhost` Host-header/discovery-reachability trick (both
  paths) is the design's own best proposal, not a proven fix.** Task 4
  (compose) and Task 13 (Helm, where infrastructure permits) include the
  actual browser-reachability smoke test the design doc itself calls for,
  and this plan's own text flags the outcome as "verified here" or "not
  verified here" rather than assuming success.
- **The Helm path's NodePort-forwarding step is a real external
  prerequisite this plan cannot enforce, matching the design doc's own
  statement that this is outside the chart's control.** Task 10 and Task 12
  document it as loudly as the design doc does; nothing in this plan claims
  it is solved.

## Global Constraints

- **No application code changes.** Nothing in `crates/api` (or any other
  crate, or `frontend/`) is modified by this plan — every task touches only
  compose files, Helm chart files, Authentik blueprint YAML, or repo
  documentation. This mirrors the design doc's own Non-goals.
- **Both deployment paths are strictly opt-in, off by default, with zero
  effect on a config pointed at a real external IdP.** Compose:
  `docker-compose.authentik.yml` is never loaded unless a developer
  explicitly appends it to `COMPOSE_FILE`; `local.env.example` is untouched
  entirely, and `dev.env.example`'s `COMPOSE_FILE` value itself does not
  change, only its neighboring comment. Helm: `devAuthentik.enabled`
  defaults to `false`; `api.sso.*` stays required with no default exactly
  as today unless the flag is explicitly set, and an operator's explicit
  `api.sso.*` value always wins over anything `devAuthentik` would compute.
- **No subchart dependency on `goauthentik/helm`, on either path.**
  Explicitly rejected by the design doc (its `dependencies:` block pulls
  Bitnami's OCI `postgresql` chart plus `authentik-remote-cluster`,
  contradicting `charts/nr-status`'s own "one chart, one namespace, one
  command. No subchart dependencies... installable in an air-gapped
  cluster" goal, `docs/superpowers/specs/2026-08-18-helm-chart-design.md:19-21`).
  Every Kubernetes object this plan adds is a hand-rolled plain manifest,
  matching how this chart already hand-rolls its own Postgres and Redis.
- **`ghcr.io/goauthentik/server:2026.8.0`, pinned, on both paths.** Matches
  this repo's existing posture on pinning. The design doc's own Open
  Questions flag this as having a genuinely finite shelf life (3-month
  release cadence, 2-version support window) — this plan does not add any
  machinery to track or bump it automatically; that is a documented,
  accepted follow-up cost, not something either path resolves.
- **No default Authentik admin account, on either path.**
  `AUTHENTIK_BOOTSTRAP_PASSWORD`/`_PASSWORD_HASH`/`_EMAIL`/`_TOKEN` stay
  unset everywhere this plan touches. This app's own login flow never needs
  one — the two blueprint files provision everything required declaratively.
  A developer who wants into Authentik's *own* admin UI uses its
  interactive first-run flow or the documented `create_recovery_key`
  fallback (both paths' docs say so).
- **One blueprint definition, two physical copies, kept byte-for-byte
  identical.** `authentik-blueprints/oauth2-client.yaml` and
  `authentik-blueprints/open-signup.yaml` (Task 1) are the source; Helm's
  `charts/nr-status/files/devauthentik-blueprints/` copies (Task 6) must
  match them exactly — Helm's `.Files.Get` cannot read paths outside the
  chart directory, so there is no way to avoid physical duplication. Task 6
  verifies this with `diff`; keeping the two in sync on any future edit is
  a documented, not automated, maintenance duty.
- **Fixed, deterministic, dev-only OIDC client values, identical on both
  paths:** `client_id: nr-status-dev`, `client_secret:
  nr-status-dev-local-only-not-a-real-secret`, application slug `nr-status`,
  enrollment flow slug `nr-status-dev-enrollment`, redirect URI
  `http://localhost:3000/api/auth/callback`. These are not real secrets —
  known-in-advance, committed-to-git, dev-only values — documented as such
  everywhere they appear (never auto-generated, unlike `internal-token`/
  `postgres-password`).
- **Dedicated Postgres for Authentik, no Redis, on both paths.** Not a
  second database on this app's own bundled Postgres — independent
  migration lifecycles and reset-friendliness, per the design doc's Data
  services research. Current Authentik (2026.8.0) uses no Redis at all;
  this app's own `redis`/`redis.enabled` stays completely unreferenced by
  either Authentik path.
- **Compose Host-header fix:** `authentik-server` gets a Compose network
  alias `authentik.localhost` and publishes port `9000` to the host under
  the identical port number (`ports: ["9000:9000"]`). `SSO_ISSUER_URL`
  becomes `http://authentik.localhost:9000/application/o/nr-status/`. This
  is the design's own proposed-not-verified answer — Task 4 is where it
  gets an actual browser smoke test.
- **Helm `devAuthentik` values shape is fixed by the design doc's own
  sketch** (`hostname`, `image.{repository,tag,pullPolicy}`, `secretKey`,
  `service.{port,nodePort}` — deliberately identical numbers — `hostAliasIP`
  escape hatch, `postgresql.{image,persistence,resources}`, `resources`,
  `nodeSelector`, `tolerations`, `affinity`). Task 5 reproduces it with only
  the additions needed to make it renderable (e.g. `pullPolicy`, which the
  design's sketch omitted as a minor gap).
- **Helm NodePort `port == nodePort` is enforced, not just documented.**
  `devauthentik-service.yaml` (Task 10) calls Helm's `fail` if the two
  values ever differ — a direct, cheap application of this chart's existing
  fail-fast posture (poller `baseUrl`, `api.sso.*`, `redis.externalUrl`) to
  an invariant the design doc states is load-bearing.
- **Helm `hostAliases` resolution has a documented, unresolved first-install
  ordering gap.** `devAuthentik.hostAliasIP`, when unset, is resolved via
  Helm's `lookup` against the live `devauthentik-service` ClusterIP — which
  does not exist yet on a from-scratch `helm install` in the same release
  as `api`. Task 11 omits the `hostAliases` entry entirely (never a wrong
  IP) when `lookup` returns nothing, and Task 12's `NOTES.txt`/README
  updates state the `helm upgrade` workaround as loudly as the design doc
  itself does — this plan does not pretend to have closed this gap.
- **`ConfigMap`, not `Secret`, for blueprint delivery under Kubernetes.**
  Nothing in the two blueprint files is sensitive (the fixed client
  id/secret are already-documented dev-only values); `AUTHENTIK_SECRET_KEY`
  and the dedicated Postgres password — which *are* sensitive — go through
  a chart-rendered `Secret` (`devauthentik-secret.yaml`, Task 7) using the
  same `lookup`-preserve pattern `secret.yaml` already uses for
  `postgres-password`/`internal-token`.
- **No `existingSecret` escape hatch for `devAuthentik`'s own generated
  values.** Unlike `postgresql.auth.existingSecret`/`api.sso.existingSecret`,
  `devAuthentik.secretKey` and its Postgres password have no
  externally-managed-secret path — this is a throwaway dev-only subsystem,
  not a production credential an operator would reasonably already manage
  elsewhere.
- **No `/var/run/docker.sock` mount on the worker, on either path.** The
  official reference compose mounts it for outpost/container management;
  this app uses no outposts (plain OIDC only), so it is deliberately
  omitted — doubly appropriate under Kubernetes, where mounting the host
  container socket into a Pod is a materially worse privilege-escalation
  surface than under a developer's own local Docker daemon.
- **New top-level files/directories this plan introduces:**
  `authentik-blueprints/oauth2-client.yaml`,
  `authentik-blueprints/open-signup.yaml`, `docker-compose.authentik.yml`,
  `charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml`,
  `charts/nr-status/files/devauthentik-blueprints/open-signup.yaml`,
  `charts/nr-status/templates/devauthentik-blueprints-configmap.yaml`,
  `charts/nr-status/templates/devauthentik-secret.yaml`,
  `charts/nr-status/templates/devauthentik-postgres-statefulset.yaml`,
  `charts/nr-status/templates/devauthentik-postgres-service.yaml`,
  `charts/nr-status/templates/devauthentik-server-deployment.yaml`,
  `charts/nr-status/templates/devauthentik-worker-deployment.yaml`,
  `charts/nr-status/templates/devauthentik-service.yaml`.
- **Task ordering: compose path complete and independently verifiable
  before any Helm task starts.** Tasks 1-4 are the whole compose path,
  ending in a live end-to-end check. Tasks 5-13 are the whole Helm path,
  reusing Task 1's blueprint content byte-for-byte once it is proven to
  work.

## Prerequisites this plan cannot verify without real infrastructure

Every task's static verification (`docker compose config`, `helm lint`,
`helm template`, `diff`) can be run with nothing more than the tools
themselves installed. The following, called out explicitly wherever they
apply, need more:

1. **Docker + network access to pull `ghcr.io/goauthentik/server:2026.8.0`
   and `postgres:16-alpine`** — needed for Task 4's live compose
   verification. If unavailable in the environment executing this plan,
   Task 4's live steps must be reported as not run, not assumed to pass.
2. **A local Kubernetes cluster (kind, minikube, or k3d) with the
   documented NodePort forwarded to loopback** — needed for Task 13's
   optional live Helm verification. This is the design doc's own stated
   "outside the chart's control" prerequisite; this plan does not create
   one and does not assume one exists.
3. **`kubeconform` or a `kubectl` with API-schema access** — needed for the
   rendered-manifest schema-validation steps in Tasks 8-10 and 13. If
   neither is available, those steps are static YAML-shape checks only
   (`helm template` succeeding is still meaningful on its own), and this
   plan says so at each such step.

---

## Docker Compose path

### Task 1: Blueprint files — OAuth2 client and open signup

**Files:**
- Create: `authentik-blueprints/oauth2-client.yaml`
- Create: `authentik-blueprints/open-signup.yaml`

**Interfaces:**
- Produces: the two blueprint files. Consumed by Task 2 (compose bind
  mount at `/blueprints/local`) and, once proven, Task 6 (Helm `ConfigMap`,
  byte-for-byte copies).

- [ ] **Step 1: Write `authentik-blueprints/oauth2-client.yaml`**

The design doc's own Research section (lines 222-246) gives this content at
sketch-but-concrete level, confirmed against the upstream blueprint schema
directly (not fabricated for this plan) — use it verbatim, with a header
comment:

```yaml
# Fixed, deterministic OIDC client provisioning for this app's local dev
# IdP -- see docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md's
# "Blueprints: declarative first-boot configuration" research section.
#
# client_id/client_secret below are dev-only, known-in-advance, committed
# values -- NOT real secrets. They must match crates/api's own SSO_CLIENT_ID
# / SSO_CLIENT_SECRET env vars exactly (see dev.env.example's OIDC section
# and docker-compose.authentik.yml's header comment) and, once the Helm
# path exists, the identical fixed values api-deployment.yaml computes when
# devAuthentik.enabled and api.sso.clientId/clientSecret are left empty
# (charts/nr-status/templates/_helpers.tpl's nr-status.devAuthentikClientId/
# nr-status.devAuthentikClientSecret).
#
# `signing_key`/`authorization_flow` !Find lookups depend on Authentik's OWN
# default blueprints (its self-signed cert, its shipped
# default-provider-authorization-implicit-consent flow) having already
# applied -- on a stone-cold first `docker compose up` this is plausibly a
# race, not a guarantee (design doc Open Questions). If this blueprint
# doesn't apply on the very first boot, restart authentik-worker (or wait
# for the next 60-minute re-apply cycle) -- this is a documented escape
# hatch, not evidence the blueprint is wrong.
model: authentik_providers_oauth2.oauth2provider
identifiers:
  name: nr-status-dev
attrs:
  client_type: confidential
  client_id: nr-status-dev
  client_secret: nr-status-dev-local-only-not-a-real-secret
  redirect_uris:
    - url: "http://localhost:3000/api/auth/callback"
      matching_mode: strict
  signing_key: !Find [authentik_crypto.certificatekeypair, [name, "authentik Self-signed Certificate"]]
  authorization_flow: !Find [authentik_flows.flow, [slug, default-provider-authorization-implicit-consent]]
  property_mappings:
    - !Find [authentik_providers_oauth2.scopemapping, [scope_name, openid]]
    - !Find [authentik_providers_oauth2.scopemapping, [scope_name, email]]
    - !Find [authentik_providers_oauth2.scopemapping, [scope_name, profile]]
---
model: authentik_core.application
identifiers:
  slug: nr-status
attrs:
  name: NR Status (dev)
  provider: !Find [authentik_providers_oauth2.oauth2provider, [name, nr-status-dev]]
```

- [ ] **Step 2: Write `authentik-blueprints/open-signup.yaml`**

The design doc describes this file's shape (adapted from upstream
`blueprints/example/flows-enrollment-2-stage.yaml`, given a fixed slug, its
`instantiate: "false"` library-example marker dropped so it actually
applies) but does not reproduce the full upstream flow content — only the
second, IdentificationStage-wiring document (design doc lines 304-310). Fetch
the actual upstream file to use as the concrete base rather than
guessing its shape:

```bash
curl -s https://raw.githubusercontent.com/goauthentik/authentik/main/blueprints/example/flows-enrollment-2-stage.yaml
```

Adapt it: give the flow a fixed slug (`nr-status-dev-enrollment`), drop the
`blueprints.goauthentik.io/instantiate: "false"` metadata label entirely (so
it applies automatically, per the design doc's Open signup research), and
rename every stage/prompt `identifiers.name` and `id` from the upstream
`default-enrollment-*` prefix to `nr-status-dev-enrollment-*` so this app's
blueprint objects are clearly namespaced and cannot collide with anything
else that might someday also copy the same upstream example under a
different local blueprint directory (the upstream copy itself, still shipped
un-instantiated at `/blueprints/example/`, is never applied and so never
collides either way — this rename is a clarity/defensiveness choice, not a
correctness requirement). Append the IdentificationStage-wiring document
from the design doc as a second YAML document in the same file:

```yaml
# Open, self-service signup for this app's local dev IdP -- see the design
# doc's "Open signup: which flow, and how it's wired in" research section
# for why the OBVIOUS candidate (default-source-enrollment) is a trap: it's
# gated by `return ak_is_sso_flow` and only fires for users arriving via an
# external source, never for direct self-registration.
#
# Adapted from upstream goauthentik/authentik's
# blueprints/example/flows-enrollment-2-stage.yaml (fetched 2026-08-29,
# `main` branch) -- a standalone enrollment flow (username/password/name/
# email, no verification stage, auto-login on success). The upstream copy
# ships with blueprints.goauthentik.io/instantiate: "false" (a "library
# example, not auto-applied" marker); this copy drops that label so it
# actually applies at startup, and renames every stage/prompt identifier
# from its upstream default-enrollment-* prefix to
# nr-status-dev-enrollment-* so this app's own blueprint objects are
# clearly namespaced.
version: 1
metadata:
  name: NR Status (dev) - Open signup enrollment flow
entries:
  - identifiers:
      slug: nr-status-dev-enrollment
    model: authentik_flows.flow
    id: flow
    attrs:
      name: NR Status (dev) enrollment
      title: Create your NR Status dev account
      designation: enrollment
      authentication: require_unauthenticated
  - id: prompt-field-username
    model: authentik_stages_prompt.prompt
    identifiers:
      name: nr-status-dev-enrollment-field-username
    attrs:
      field_key: username
      label: Username
      type: username
      required: true
      placeholder: Username
      placeholder_expression: false
      order: 0
  - identifiers:
      name: nr-status-dev-enrollment-field-password
    id: prompt-field-password
    model: authentik_stages_prompt.prompt
    attrs:
      field_key: password
      label: Password
      type: password
      required: true
      placeholder: Password
      placeholder_expression: false
      order: 0
  - identifiers:
      name: nr-status-dev-enrollment-field-password-repeat
    id: prompt-field-password-repeat
    model: authentik_stages_prompt.prompt
    attrs:
      field_key: password_repeat
      label: Password (repeat)
      type: password
      required: true
      placeholder: Password (repeat)
      placeholder_expression: false
      order: 1
  - identifiers:
      name: nr-status-dev-enrollment-field-name
    id: prompt-field-name
    model: authentik_stages_prompt.prompt
    attrs:
      field_key: name
      label: Name
      type: text
      required: true
      placeholder: Name
      placeholder_expression: false
      order: 0
  - identifiers:
      name: nr-status-dev-enrollment-field-email
    id: prompt-field-email
    model: authentik_stages_prompt.prompt
    attrs:
      field_key: email
      label: Email
      type: email
      required: true
      placeholder: Email
      placeholder_expression: false
      order: 1
  - identifiers:
      name: nr-status-dev-enrollment-prompt-second
    id: nr-status-dev-enrollment-prompt-second
    model: authentik_stages_prompt.promptstage
    attrs:
      fields:
        - !KeyOf prompt-field-name
        - !KeyOf prompt-field-email
  - identifiers:
      name: nr-status-dev-enrollment-prompt-first
    id: nr-status-dev-enrollment-prompt-first
    model: authentik_stages_prompt.promptstage
    attrs:
      fields:
        - !KeyOf prompt-field-username
        - !KeyOf prompt-field-password
        - !KeyOf prompt-field-password-repeat
  - identifiers:
      name: nr-status-dev-enrollment-user-login
    id: nr-status-dev-enrollment-user-login
    model: authentik_stages_user_login.userloginstage
  - identifiers:
      name: nr-status-dev-enrollment-user-write
    id: nr-status-dev-enrollment-user-write
    model: authentik_stages_user_write.userwritestage
    attrs:
      user_creation_mode: always_create
  - identifiers:
      target: !KeyOf flow
      stage: !KeyOf nr-status-dev-enrollment-prompt-first
      order: 10
    model: authentik_flows.flowstagebinding
  - identifiers:
      target: !KeyOf flow
      stage: !KeyOf nr-status-dev-enrollment-prompt-second
      order: 11
    model: authentik_flows.flowstagebinding
  - identifiers:
      target: !KeyOf flow
      stage: !KeyOf nr-status-dev-enrollment-user-write
      order: 20
    model: authentik_flows.flowstagebinding
  - identifiers:
      target: !KeyOf flow
      stage: !KeyOf nr-status-dev-enrollment-user-login
      order: 100
    model: authentik_flows.flowstagebinding
---
# Wire the enrollment flow above into the shipped default login form's
# "Need an account? Sign up." link. Per the design doc's research: the
# IdentificationStage's enrollment_flow field is what renders that link,
# it is unset by default on Authentik's own shipped
# default-authentication-identification stage, and it is NOT settable via
# the Brand model -- only here, at the stage level.
model: authentik_stages_identification.identificationstage
identifiers:
  name: default-authentication-identification
attrs:
  enrollment_flow: !Find [authentik_flows.flow, [slug, nr-status-dev-enrollment]]
```

- [ ] **Step 3: Static YAML-validity check**

Run:
```bash
python3 -c "import yaml,sys; list(yaml.safe_load_all(open('authentik-blueprints/oauth2-client.yaml')))" && echo OK
python3 -c "import yaml,sys; list(yaml.safe_load_all(open('authentik-blueprints/open-signup.yaml')))" && echo OK
```
Expected: both print `OK` — confirms the files are syntactically valid
multi-document YAML. **This only validates YAML syntax, not that Authentik
accepts the blueprint schema itself** — that requires a running Authentik
instance and is covered by Task 4's live verification, where the
blueprint's actual apply/failure is visible in `authentik-worker`'s logs.

- [ ] **Step 4: Commit**

```bash
git add authentik-blueprints/oauth2-client.yaml authentik-blueprints/open-signup.yaml
git commit -m "Add Authentik blueprints for the local dev OIDC client and open signup"
```

---

### Task 2: `docker-compose.authentik.yml` — Authentik services

**Files:**
- Create: `docker-compose.authentik.yml`

**Interfaces:**
- Consumes: `authentik-blueprints/` (Task 1), bind-mounted read-only.
- Produces: `authentik-postgres`, `authentik-server`, `authentik-worker`
  services and the `authentik_postgres_data` named volume. Never referenced
  unless a developer's `COMPOSE_FILE` explicitly names this file (Task 3).

- [ ] **Step 1: Write the file**

```yaml
# Local dev-only Authentik IdP overlay -- OPT-IN, never loaded unless a
# developer appends ":docker-compose.authentik.yml" to COMPOSE_FILE in
# their own dev.env (see the comment beside COMPOSE_FILE in dev.env.example,
# and docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md).
#
# Provides a local OIDC identity provider so SSO_ISSUER_URL/SSO_CLIENT_ID/
# SSO_CLIENT_SECRET/SSO_REDIRECT_URL/SSO_POST_LOGIN_REDIRECT_URL (the api's
# own SSO_* config, docker-compose.yml:103-107) can point at a real,
# locally-running server instead of the sso.example.invalid placeholder.
#
# Three services -- matches the official goauthentik.io/docker-compose.yml
# shape as of 2026-08-29. No Redis: current Authentik (2026.8.0) doesn't
# use one, confirmed directly against that reference compose.
#   authentik-postgres  postgres:16-alpine, DEDICATED to Authentik -- NOT
#                       this app's own `postgres` service. See the design
#                       doc's Data services section: independent migration
#                       lifecycles, reset-friendliness.
#   authentik-server    the web UI / OIDC endpoints (discovery, authorize,
#                       token, JWKS).
#   authentik-worker    background task processing, including blueprint
#                       apply.
#
# Deliberately NOT set anywhere below: AUTHENTIK_BOOTSTRAP_PASSWORD/_HASH/
# _EMAIL/_TOKEN. There is genuinely no default admin account -- see the
# design doc's Bootstrap section. This app's own login flow never needs
# one: authentik-blueprints/oauth2-client.yaml and open-signup.yaml
# provision everything a developer needs declaratively. To get into
# Authentik's OWN admin UI:
#   - interactive first-run flow: http://authentik.localhost:9000/if/flow/initial-setup/
#   - or, if that's ever unreachable, the documented recovery-key fallback:
#       docker compose --env-file dev.env run --rm authentik-server \
#         create_recovery_key 10 akadmin
#
# AUTHENTIK_SECRET_KEY has NO default and MUST be set in your env file --
# both authentik-server and authentik-worker refuse to start without it.
# Generate one with, e.g., `openssl rand -base64 60`.
#
# HOST-HEADER / DISCOVERY REACHABILITY -- PROPOSED, NOT INDEPENDENTLY
# VERIFIED beyond this plan's own Task 4 smoke test: authentik-server
# publishes port 9000 under the Compose network alias `authentik.localhost`
# (RFC 6761 -- modern browsers resolve any *.localhost name to loopback with
# no /etc/hosts entry needed). This makes the api container (reaching
# Authentik over the Docker-internal network) and the developer's actual
# browser resolve the EXACT SAME hostname:port to the exact same container,
# which Authentik's Host-header-derived .well-known/openid-configuration
# document needs to work at all -- see the design doc's "browser-vs-
# container discovery reachability problem" research. If
# authentik.localhost doesn't resolve to loopback in whatever browser/OS
# you're using, add a manual /etc/hosts entry (127.0.0.1 authentik.localhost)
# as the documented fallback.
#
# SSO_ISSUER_URL for this local IdP:
#   http://authentik.localhost:9000/application/o/nr-status/

services:
  authentik-postgres:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_USER: authentik
      POSTGRES_PASSWORD: ${AUTHENTIK_PG_PASSWORD:-changeme-authentik-local-dev-only}
      POSTGRES_DB: authentik
    volumes:
      - authentik_postgres_data:/var/lib/postgresql/data
    healthcheck:
      # Same probe shape as docker-compose.yml's own `postgres` service.
      test: ["CMD-SHELL", "pg_isready -U authentik -d authentik"]
      interval: 5s
      timeout: 5s
      retries: 10
      start_period: 5s

  authentik-server:
    image: ghcr.io/goauthentik/server:2026.8.0
    command: server
    restart: unless-stopped
    depends_on:
      authentik-postgres:
        condition: service_healthy
    environment: &authentik-env
      AUTHENTIK_SECRET_KEY: ${AUTHENTIK_SECRET_KEY:?AUTHENTIK_SECRET_KEY must be set -- generate one with e.g. `openssl rand -base64 60`}
      AUTHENTIK_POSTGRESQL__HOST: authentik-postgres
      AUTHENTIK_POSTGRESQL__USER: authentik
      AUTHENTIK_POSTGRESQL__PASSWORD: ${AUTHENTIK_PG_PASSWORD:-changeme-authentik-local-dev-only}
      AUTHENTIK_POSTGRESQL__NAME: authentik
      # Deliberately NOT set: AUTHENTIK_BOOTSTRAP_PASSWORD/_HASH/_EMAIL/
      # _TOKEN -- see the file header comment above.
    # Matches the official reference compose's shm_size for both server and
    # worker (Django/Postgres shared-memory headroom).
    shm_size: 512mb
    volumes:
      - ./authentik-blueprints:/blueprints/local:ro
    ports:
      - "9000:9000"
    networks:
      default:
        aliases:
          - authentik.localhost
    healthcheck:
      # /-/health/live/ is Authentik's documented liveness path; NOT
      # independently re-confirmed by this plan's own research pass beyond
      # citing it -- verify against real container behaviour in Task 4's
      # live check and adjust if it 404s (a bare TCP probe on 9000 is the
      # safe fallback).
      test: ["CMD", "wget", "--spider", "-q", "http://localhost:9000/-/health/live/"]
      interval: 10s
      timeout: 5s
      retries: 12
      start_period: 30s

  authentik-worker:
    image: ghcr.io/goauthentik/server:2026.8.0
    command: worker
    restart: unless-stopped
    depends_on:
      authentik-postgres:
        condition: service_healthy
    environment: *authentik-env
    shm_size: 512mb
    volumes:
      - ./authentik-blueprints:/blueprints/local:ro
      # No /var/run/docker.sock mount -- the official reference compose
      # mounts it for outpost/container management, which this app never
      # uses (plain OIDC only, no proxy/forward-auth outposts). Omitting it
      # is a deliberate reduction of both footprint and this container's
      # access to the host Docker socket.

volumes:
  authentik_postgres_data:
```

- [ ] **Step 2: Static config-resolution check**

Run:
```bash
AUTHENTIK_SECRET_KEY=static-check-placeholder \
  docker compose -f docker-compose.yml -f docker-compose.authentik.yml config --quiet
```
Expected: exit code `0`, no error output. This validates the new file's
YAML and interpolation (including the `${AUTHENTIK_SECRET_KEY:?...}` guard
resolving now that it's set) **without starting any container** — it does
not confirm the services actually boot; that's Task 4.

- [ ] **Step 3: Commit**

```bash
git add docker-compose.authentik.yml
git commit -m "Add opt-in docker-compose.authentik.yml local dev IdP overlay"
```

---

### Task 3: `dev.env.example` — opt-in instructions and local-dev SSO values

**Files:**
- Modify: `dev.env.example`

**Interfaces:** none new — this task only documents the opt-in path Task 2
already made mechanically possible. `local.env.example` is deliberately
**not** touched, per the design doc (it "stays production-style run").

- [ ] **Step 1: Add a comment next to `COMPOSE_FILE`**

In `dev.env.example`, next to the existing
`COMPOSE_FILE=docker-compose.yml:docker-compose.dev.yml` line (around line
57), add a comment documenting the opt-in without changing the value
itself:

```
# To also bring up a local Authentik IdP for testing the SSO login flow
# with zero manual IdP-side setup, append ":docker-compose.authentik.yml"
# to the line above, i.e.:
#   COMPOSE_FILE=docker-compose.yml:docker-compose.dev.yml:docker-compose.authentik.yml
# and set AUTHENTIK_SECRET_KEY (see docker-compose.authentik.yml's own
# header comment), then replace the SSO_* placeholders below with the local
# dev IdP's values -- see the OIDC section below for the exact values.
# See docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md.
```

- [ ] **Step 2: Document the local-dev SSO values in the existing OIDC block**

In the existing OIDC SSO comment block (around lines 103-124), after the
existing "point at a real or locally-run OIDC-compliant SSO server..."
sentence, add:

```
# If you opted into the bundled local Authentik IdP above
# (docker-compose.authentik.yml), use these exact values instead of the
# changeme-* placeholders below -- they match
# authentik-blueprints/oauth2-client.yaml's fixed, deterministic client
# provisioning:
#   SSO_ISSUER_URL=http://authentik.localhost:9000/application/o/nr-status/
#   SSO_CLIENT_ID=nr-status-dev
#   SSO_CLIENT_SECRET=nr-status-dev-local-only-not-a-real-secret
# SSO_REDIRECT_URL and SSO_POST_LOGIN_REDIRECT_URL below already match the
# blueprint's fixed redirect_uris and need no change.
```

Leave the existing `SSO_ISSUER_URL=http://sso.example.invalid` etc. default
placeholders exactly as they are — this task only adds documentation, it
does not change what a fresh `dev.env` boots against by default (per Global
Constraints: the opt-out stays the default).

- [ ] **Step 3: Confirm the unmodified default path still resolves**

Run:
```bash
docker compose --env-file dev.env config --quiet
```
Expected: exit `0` — confirms this task's comment-only edits didn't
introduce a stray syntax error, and that `COMPOSE_FILE`'s actual value is
unchanged (the default two-file dev stack still resolves with no Authentik
overlay in play).

- [ ] **Step 4: Commit**

```bash
git add dev.env.example
git commit -m "Document the opt-in local Authentik IdP overlay in dev.env.example"
```

---

### Task 4: Compose path end-to-end verification

**Files:** none (verification only) — this is the checkpoint the process
requires before any Helm task starts: the compose path fully built and
independently proven, not merely rendered.

- [ ] **Step 1: Static config-resolution check with the overlay enabled**

Run:
```bash
AUTHENTIK_SECRET_KEY=static-check-placeholder \
  docker compose --env-file dev.env -f docker-compose.yml -f docker-compose.dev.yml -f docker-compose.authentik.yml config --quiet
```
Expected: exit `0`. Static only — does not start anything.

- [ ] **Step 2: Live check — bring up the Authentik overlay**

**Requires real infrastructure: Docker plus network access to pull
`ghcr.io/goauthentik/server:2026.8.0` and `postgres:16-alpine`.** If this
environment cannot do that, report this step and Steps 3-6 below as **not
run here**, not as passing.

```bash
cp dev.env dev.env.authentik-test
# Append the overlay to COMPOSE_FILE:
sed -i 's/^COMPOSE_FILE=.*/COMPOSE_FILE=docker-compose.yml:docker-compose.dev.yml:docker-compose.authentik.yml/' dev.env.authentik-test
# Set a real secret key and the local-dev SSO values from Task 3 Step 2:
cat >> dev.env.authentik-test <<'EOF'
AUTHENTIK_SECRET_KEY=REPLACE-WITH-openssl-rand--base64-60-OUTPUT
SSO_ISSUER_URL=http://authentik.localhost:9000/application/o/nr-status/
SSO_CLIENT_ID=nr-status-dev
SSO_CLIENT_SECRET=nr-status-dev-local-only-not-a-real-secret
EOF

docker compose --env-file dev.env.authentik-test up -d authentik-postgres authentik-server authentik-worker
docker compose --env-file dev.env.authentik-test ps
```
Expected: `authentik-postgres`, `authentik-server`, `authentik-worker` all
report healthy/running. Authentik's own startup (including applying its own
default blueprints) can take a minute or two on a cold first boot — allow
for that before treating a not-yet-healthy state as a failure.

- [ ] **Step 3: Confirm this app's blueprint applied**

```bash
docker compose --env-file dev.env.authentik-test logs authentik-worker | grep -i blueprint
```
Then open `http://authentik.localhost:9000/if/flow/initial-setup/` (or use
the Authentik API) to confirm an OAuth2 Provider named `nr-status-dev` and
an Application slugged `nr-status` exist. **Per the design doc's own Open
Questions**, the `!Find` lookups in `oauth2-client.yaml` depend on
Authentik's own default blueprints having already applied — on a stone-cold
first boot this is plausibly a race. If the objects are not present after
~2 minutes:
```bash
docker compose --env-file dev.env.authentik-test restart authentik-worker
```
and re-check. Record in this task's own notes whether a restart was
needed — that is exactly the design doc's stated risk being exercised for
real, not a bug in this plan.

- [ ] **Step 4: Confirm open signup and the Host-header/discovery fix, in a real browser**

Open `http://authentik.localhost:9000/` in an actual browser (not `curl` —
the point of this check is whether the *browser* resolves the hostname).
Expected: the login page loads with no Host-header/`ALLOWED_HOSTS`
rejection and no certificate/mixed-content error, and a "Need an account?
Sign up." link is present and leads to the `nr-status-dev-enrollment` flow.
**This is the single most important smoke test of the design's own admitted
"proposed, not verified" browser-vs-container discovery fix.** If
`authentik.localhost` does not resolve to loopback in whatever browser/OS
is actually used here, fall back to a manual `/etc/hosts` entry (`127.0.0.1
authentik.localhost`) per the design doc's documented fallback, and record
plainly in this task's notes whether the automatic `.localhost` resolution
worked unaided or the manual fallback was needed — do not silently paper
over which one actually happened.

- [ ] **Step 5: Full login round-trip against this app itself**

Requires Task 3's real dev values and the full stack:
```bash
docker compose --env-file dev.env.authentik-test up --build -d
```
Open `http://localhost:3000/api/auth/login`, sign up a throwaway account
through the Authentik enrollment flow, confirm redirect back to the
frontend, then:
```bash
curl -s http://localhost:3000/api/auth/session   # expect authenticated: true
psql "$DATABASE_URL" -c "SELECT id FROM users"   # expect the new sub-derived id
```
Expected: all of the above succeed. Any failure here is a real integration
bug between this overlay and `crates/api`'s existing OIDC client, not a
test-fixture artifact.

- [ ] **Step 6: Tear down**

```bash
docker compose --env-file dev.env.authentik-test down -v
rm dev.env.authentik-test
```
`dev.env.authentik-test` is a local-only scratch file — never commit it.

- [ ] **Step 7: No commit for this task**

This is a verification-only task; nothing new is created to commit. If any
of Steps 2-5 could not be run because this environment lacks Docker/network
access, state that plainly rather than proceeding to the Helm path as if
the compose path were proven end to end when it wasn't.

---

## Helm path

### Task 5: `devAuthentik` values.yaml block

**Files:**
- Modify: `charts/nr-status/values.yaml`

**Interfaces:**
- Produces: `devAuthentik.{enabled,hostname,image,secretKey,service,hostAliasIP,postgresql,resources,nodeSelector,tolerations,affinity}`.
  Consumed by every subsequent Helm task.

- [ ] **Step 1: Add the block**

Insert between the existing `api:` block (ends around line 375) and
`aggregator:` (starts around line 379):

```yaml
# ---------------------------------------------------------------------------
# devAuthentik -- OPT-IN local dev-only OIDC identity provider
# (docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md)
# ---------------------------------------------------------------------------
# A hand-rolled (NOT the goauthentik/helm subchart -- see the design doc's
# Non-goals) Authentik deployment for exercising this app's own login flow
# against a local/dev Kubernetes cluster with zero manual IdP-side setup,
# mirroring docker-compose.authentik.yml's job for the compose path.
# Strictly a dev convenience: no HA, no TLS automation, no backup/restore --
# see the design doc's Non-goals, which apply with equal force here.
#
# When enabled AND api.sso.* is left at its empty default, the chart
# computes api.sso.issuerUrl/clientId/clientSecret/redirectUrl/
# postLoginRedirectUrl from the values below and the fixed blueprint-
# provisioned client_id/client_secret in
# charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml (the
# SAME fixed values docker-compose.authentik.yml's blueprint uses -- see
# authentik-blueprints/oauth2-client.yaml). An operator who sets any
# api.sso.* value explicitly keeps it; devAuthentik never overrides an
# explicit value.
devAuthentik:
  # -- Opt-in; false by default. An install that already points api.sso.*
  # at a real external IdP is completely unaffected either way.
  enabled: false
  # -- The ONE hostname both the developer's browser and the api Pod must
  # resolve identically -- see "Discovery reachability under Kubernetes" in
  # the design doc. Resolves to loopback with no /etc/hosts entry needed in
  # modern browsers (RFC 6761 .localhost handling), the same trick
  # docker-compose.authentik.yml uses.
  hostname: authentik.localhost
  image:
    repository: ghcr.io/goauthentik/server
    # -- Pinned, same rationale/finite-shelf-life caveat as the compose
    # path's tag (design doc Research + Open questions: Authentik's
    # 3-month release cadence and 2-version support window mean this needs
    # periodic bumping -- not automated by this chart).
    tag: "2026.8.0"
    pullPolicy: IfNotPresent
  # -- AUTHENTIK_SECRET_KEY. Chart-generated via the same lookup-then-
  # randAlphaNum pattern secret.yaml already uses for postgres-password/
  # internal-token when left empty, so it survives `helm upgrade`. No
  # existingSecret override -- this is a throwaway dev IdP, not a value an
  # operator would reasonably already have externally managed.
  secretKey: ""
  service:
    # -- ClusterIP-facing port AND NodePort. MUST be the same number as
    # nodePort below -- see "Discovery reachability under Kubernetes" in
    # the design doc: an in-cluster caller and a caller on the node's
    # published port must reach Authentik on an identical port number.
    # devauthentik-service.yaml aborts the render if these two ever differ.
    port: 30900
    # -- Must equal `port` above. Default 30900 is inside Kubernetes'
    # default 30000-32767 NodePort range (a nodePort can't be an arbitrary
    # value like the compose path's 9000 without a nonstandard
    # --service-node-port-range on the apiserver).
    nodePort: 30900
  # -- Explicit override for the IP the api Deployment's hostAliases entry
  # points devAuthentik.hostname at. Leave empty to use `lookup` against
  # devauthentik-service's live ClusterIP at render time (the chart's usual
  # pattern) -- BUT on a from-scratch `helm install`, devauthentik-service
  # doesn't exist yet when api is first rendered in the SAME release, so
  # `lookup` returns nothing and NO hostAliases entry is written at all.
  # Either set this explicitly on first install (predictable on e.g. kind's
  # commonly-default 10.96.0.0/12 service CIDR), or run `helm upgrade` once
  # immediately after the first `helm install` -- see NOTES.txt and the
  # chart README. THIS IS THE SINGLE BIGGEST GAP BETWEEN "DESIGNED" AND
  # "KNOWN TO WORK" ON THIS PATH, per the design doc's Open questions /
  # risks, Helm path section -- not resolved by this chart.
  hostAliasIP: ""
  # -- Dedicated Postgres for Authentik's own state -- NOT a second
  # database on this chart's own bundled `postgresql`. Same reasoning as
  # the compose path (design doc's Data services section): independent
  # migration lifecycles, reset-friendliness. Always rendered when
  # devAuthentik.enabled -- unlike the app's own postgresql/externalDatabase
  # pair there is no external-database escape hatch here, because this is a
  # throwaway dev-only database with no production posture to accommodate.
  postgresql:
    image: postgres:16-alpine
    persistence:
      enabled: true
      size: 1Gi
      storageClass: ""
    resources: {}
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
```

- [ ] **Step 2: Confirm the chart still renders unaffected**

Run:
```bash
helm lint charts/nr-status
helm template charts/nr-status --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x >/dev/null
```
Expected: both succeed with no error — the new block is inert until a later
task's templates reference it (`devAuthentik.enabled` defaults to `false`,
and nothing reads it yet).

- [ ] **Step 3: Commit**

```bash
git add charts/nr-status/values.yaml
git commit -m "Add the devAuthentik values.yaml block"
```

---

### Task 6: Blueprint `ConfigMap` delivery

**Files:**
- Create: `charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml`
- Create: `charts/nr-status/files/devauthentik-blueprints/open-signup.yaml`
- Create: `charts/nr-status/templates/devauthentik-blueprints-configmap.yaml`

**Interfaces:**
- Produces: a `ConfigMap` named `<fullname>-devauthentik-blueprints` whose
  data keys are the two blueprint filenames. Consumed by Task 9's server
  and worker `Deployment`s (mounted at `/blueprints/local`).

- [ ] **Step 1: Copy the blueprint files byte-for-byte**

```bash
mkdir -p charts/nr-status/files/devauthentik-blueprints
cp authentik-blueprints/oauth2-client.yaml charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml
cp authentik-blueprints/open-signup.yaml charts/nr-status/files/devauthentik-blueprints/open-signup.yaml
```
Helm's `.Files.Get` can only read paths inside the chart directory, so this
physical duplication is unavoidable, not an oversight — see Global
Constraints. Keeping the two copies in sync on any future edit is a
documented, not automated, maintenance duty; Step 3 below verifies they
start in sync.

- [ ] **Step 2: Write `devauthentik-blueprints-configmap.yaml`**

```yaml
{{- if .Values.devAuthentik.enabled }}
apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ include "nr-status.devAuthentikFullname" . }}-blueprints
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik") | nindent 4 }}
data:
  {{- range $path, $_ := .Files.Glob "files/devauthentik-blueprints/*.yaml" }}
  {{ base $path }}: |
    {{- $.Files.Get $path | nindent 4 }}
  {{- end }}
{{- end }}
```

This references `nr-status.devAuthentikFullname`, added in Task 7 — write
this file now, it will not render correctly until Task 7 lands, which is
fine since it's still gated behind `devAuthentik.enabled: false` by
default.

- [ ] **Step 3: Verify byte-for-byte parity and render shape**

```bash
diff authentik-blueprints/oauth2-client.yaml charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml
diff authentik-blueprints/open-signup.yaml charts/nr-status/files/devauthentik-blueprints/open-signup.yaml
```
Expected: both produce no output (files identical). This step cannot fully
confirm the `ConfigMap` template itself renders correctly until Task 7's
helpers exist — re-run after Task 7:
```bash
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/devauthentik-blueprints-configmap.yaml
```
Expected: a `ConfigMap` with exactly two data keys
(`oauth2-client.yaml`, `open-signup.yaml`) whose content matches the two
source files.

- [ ] **Step 4: Commit**

```bash
git add charts/nr-status/files/devauthentik-blueprints charts/nr-status/templates/devauthentik-blueprints-configmap.yaml
git commit -m "Deliver the dev IdP blueprints to Kubernetes via a ConfigMap"
```

---

### Task 7: `_helpers.tpl` additions and `devauthentik-secret.yaml`

**Files:**
- Modify: `charts/nr-status/templates/_helpers.tpl`
- Create: `charts/nr-status/templates/devauthentik-secret.yaml`

**Interfaces:**
- Produces: `nr-status.devAuthentikFullname`,
  `nr-status.devAuthentikWorkerFullname`,
  `nr-status.devAuthentikPostgresFullname`,
  `nr-status.devAuthentikSecretName`,
  `nr-status.devAuthentikSecretKeySecretKey`,
  `nr-status.devAuthentikPostgresSecretKey`,
  `nr-status.devAuthentikClientId`, `nr-status.devAuthentikClientSecret`,
  `nr-status.devAuthentikIssuerUrl`, `nr-status.devAuthentikRedirectUrl`,
  `nr-status.devAuthentikPostLoginRedirectUrl`,
  `nr-status.devAuthentikHostAliasIP`; and the `Secret` object
  `devauthentik-secret.yaml`. Consumed by Tasks 6, 8, 9, 10, 11.

- [ ] **Step 1: Add naming and secret-reference helpers**

Append to `charts/nr-status/templates/_helpers.tpl`:

```
{{/*
Per-component devAuthentik object names. Each takes root. The server
Deployment and the NodePort Service in front of it share ONE name (matching
how api's Deployment and Service already share nr-status.apiFullname); the
worker Deployment and dedicated Postgres each get their own.
*/}}
{{- define "nr-status.devAuthentikFullname" -}}
{{- printf "%s-devauthentik" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "nr-status.devAuthentikWorkerFullname" -}}
{{- printf "%s-devauthentik-worker" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "nr-status.devAuthentikPostgresFullname" -}}
{{- printf "%s-devauthentik-postgres" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Resolved Secret name/keys for devAuthentik's own AUTHENTIK_SECRET_KEY and
its dedicated Postgres password. Takes root. Unlike the chart's other
secret pairs there is no existingSecret override -- see values.yaml's
devAuthentik.secretKey comment for why. Always the chart-rendered Secret,
devauthentik-secret.yaml.
*/}}
{{- define "nr-status.devAuthentikSecretName" -}}
{{- printf "%s-devauthentik" (include "nr-status.secretName" .) }}
{{- end }}

{{- define "nr-status.devAuthentikSecretKeySecretKey" -}}
{{- print "authentik-secret-key" }}
{{- end }}

{{- define "nr-status.devAuthentikPostgresSecretKey" -}}
{{- print "authentik-postgres-password" }}
{{- end }}

{{/*
Fixed, blueprint-provisioned OIDC client id/secret for the local dev IdP --
IDENTICAL literal values to authentik-blueprints/oauth2-client.yaml (the
compose path's blueprint, Task 1) and its byte-for-byte chart copy at
charts/nr-status/files/devauthentik-blueprints/oauth2-client.yaml (Task 6).
Not secret in any meaningful sense -- known-in-advance, dev-only, committed
to git in both places -- but still routed through secret.yaml's
sso-client-secret entry for clientSecret rather than inlined directly in a
Deployment spec, matching this chart's usual posture for anything named
"secret" even when its value isn't sensitive.
*/}}
{{- define "nr-status.devAuthentikClientId" -}}
{{- print "nr-status-dev" }}
{{- end }}

{{- define "nr-status.devAuthentikClientSecret" -}}
{{- print "nr-status-dev-local-only-not-a-real-secret" }}
{{- end }}

{{/*
Computed api.sso.* defaults for when devAuthentik.enabled is true and the
corresponding api.sso.* value is left empty. Takes root. Consumed by
api-deployment.yaml's top-of-file local-variable block (Task 11), the only
caller.

redirectUrl/postLoginRedirectUrl assume the developer reaches the frontend
at exactly http://localhost:3000 -- e.g. via
`kubectl port-forward svc/<release>-frontend 3000:3000` -- the SAME fixed
strings docker-compose.authentik.yml's blueprint (Task 1) already commits
the redirect_uris to, since the blueprint's redirect_uris list is a fixed
literal either way. The design doc describes the computed-defaults
MECHANISM for the Helm path but does not spell out these two literal
strings; this plan resolves that gap by reusing the compose path's own
fixed values verbatim, so both deployment paths are interchangeable in a
developer's head -- matching the design's stated intent for the client
id/secret pair.
*/}}
{{- define "nr-status.devAuthentikIssuerUrl" -}}
{{- printf "http://%s:%d/application/o/nr-status/" .Values.devAuthentik.hostname (int .Values.devAuthentik.service.port) }}
{{- end }}

{{- define "nr-status.devAuthentikRedirectUrl" -}}
{{- print "http://localhost:3000/api/auth/callback" }}
{{- end }}

{{- define "nr-status.devAuthentikPostLoginRedirectUrl" -}}
{{- print "http://localhost:3000/" }}
{{- end }}

{{/*
IP for the api Deployment's hostAliases entry mapping devAuthentik.hostname
straight to devauthentik-service's ClusterIP. Takes root.
devAuthentik.hostAliasIP wins when set; otherwise `lookup`s the live
Service. Returns an empty string (NEVER a wrong value) when neither is
available -- see values.yaml's devAuthentik.hostAliasIP comment for the
from-scratch-install ordering gap this reflects. api-deployment.yaml (Task
11) omits the whole hostAliases entry when this returns empty, rather than
writing an IP of "".
*/}}
{{- define "nr-status.devAuthentikHostAliasIP" -}}
{{- if .Values.devAuthentik.hostAliasIP -}}
{{- .Values.devAuthentik.hostAliasIP -}}
{{- else -}}
{{- $svc := lookup "v1" "Service" .Release.Namespace (include "nr-status.devAuthentikFullname" .) -}}
{{- if $svc -}}
{{- $svc.spec.clusterIP -}}
{{- end -}}
{{- end -}}
{{- end }}
```

- [ ] **Step 2: Write `devauthentik-secret.yaml`**

```yaml
{{- if .Values.devAuthentik.enabled }}
{{/*
Chart-rendered Secret for devAuthentik's own AUTHENTIK_SECRET_KEY and its
dedicated Postgres password. A separate object from the chart's main
Secret (secret.yaml) -- devAuthentik is an independent, fully opt-in
subsystem with no existingSecret escape hatch (see values.yaml), so
keeping it in its own object keeps the two lifecycles visually separate.

Uses the SAME lookup-preserve pattern secret.yaml already uses -- see that
file's header comment for why it is load-bearing, not stylistic: without
it, every `helm upgrade` would regenerate both values and break the
running Authentik deployment (a rotated AUTHENTIK_SECRET_KEY invalidates
every existing Authentik session/signed value; a rotated Postgres password
breaks the connection outright).
*/}}
{{- $secretName := include "nr-status.devAuthentikSecretName" . -}}
{{- $existing := lookup "v1" "Secret" .Release.Namespace $secretName -}}
{{- $existingData := default (dict) $existing.data -}}
{{- $secretKey := .Values.devAuthentik.secretKey | default (get $existingData (include "nr-status.devAuthentikSecretKeySecretKey" .) | b64dec) | default (randAlphaNum 60) -}}
{{- $pgPassword := (get $existingData (include "nr-status.devAuthentikPostgresSecretKey" .) | b64dec) | default (randAlphaNum 32) -}}
apiVersion: v1
kind: Secret
metadata:
  name: {{ $secretName }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik") | nindent 4 }}
type: Opaque
data:
  {{ include "nr-status.devAuthentikSecretKeySecretKey" . }}: {{ $secretKey | b64enc | quote }}
  {{ include "nr-status.devAuthentikPostgresSecretKey" . }}: {{ $pgPassword | b64enc | quote }}
{{- end }}
```

- [ ] **Step 3: Verify**

```bash
helm lint charts/nr-status
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/devauthentik-secret.yaml
```
Run the `helm template` command twice. Expected: both runs render a
`Secret` with both keys present, but with **different generated values
between the two runs** — this is the same documented `lookup`-during-
`helm template` limitation `secret.yaml` already states in its own header
comment and in the chart README (`lookup` always returns empty during
offline rendering), not a bug in this task. Also re-run Task 6 Step 3's
`ConfigMap` render now that `nr-status.devAuthentikFullname` exists.

- [ ] **Step 4: Commit**

```bash
git add charts/nr-status/templates/_helpers.tpl charts/nr-status/templates/devauthentik-secret.yaml
git commit -m "Add devAuthentik naming helpers and its generated Secret"
```

---

### Task 8: Dedicated Authentik Postgres — `StatefulSet` and `Service`

**Files:**
- Create: `charts/nr-status/templates/devauthentik-postgres-statefulset.yaml`
- Create: `charts/nr-status/templates/devauthentik-postgres-service.yaml`

**Interfaces:**
- Consumes: `nr-status.devAuthentikPostgresFullname`,
  `nr-status.devAuthentikSecretName`/`devAuthentikPostgresSecretKey` (Task 7).
- Produces: `devauthentik-postgres` `StatefulSet` + headless `Service` +
  `volumeClaimTemplates` entry. Consumed by Task 9's server/worker env
  (`AUTHENTIK_POSTGRESQL__HOST`).

- [ ] **Step 1: Write `devauthentik-postgres-statefulset.yaml`**

Mirrors `postgres-statefulset.yaml` exactly in shape — same `PGDATA`
subdirectory fix, same explicit `runAsUser`/`runAsGroup`/`fsGroup: 999`,
same `pg_isready` probes, per the design doc's own instruction that this
section of the base chart's design "applies verbatim, just instantiated a
second time":

```yaml
{{- if .Values.devAuthentik.enabled }}
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: {{ include "nr-status.devAuthentikPostgresFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-postgres") | nindent 4 }}
spec:
  replicas: 1
  serviceName: {{ include "nr-status.devAuthentikPostgresFullname" . }}
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "devauthentik-postgres") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-postgres") | nindent 8 }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      # Same rationale as postgres-statefulset.yaml's identical block:
      # postgres:16-alpine's default USER is root; it drops to `postgres`
      # via gosu in its own entrypoint, so a bare runAsNonRoot: true would
      # fail admission. Pinned uid/gid/fsGroup 999, same as this chart's
      # own bundled Postgres.
      securityContext:
        {{- $pgSecurityDefaults := dict "runAsNonRoot" true "runAsUser" 999 "runAsGroup" 999 "fsGroup" 999 "seccompProfile" (dict "type" "RuntimeDefault") }}
        {{- toYaml $pgSecurityDefaults | nindent 8 }}
      containers:
        - name: postgres
          image: {{ .Values.devAuthentik.postgresql.image | quote }}
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: false
            capabilities:
              drop:
                - ALL
          ports:
            - name: postgres
              containerPort: 5432
              protocol: TCP
          env:
            - name: POSTGRES_USER
              value: authentik
            - name: POSTGRES_DB
              value: authentik
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.devAuthentikSecretName" . }}
                  key: {{ include "nr-status.devAuthentikPostgresSecretKey" . }}
            # Same CSI lost+found gotcha as postgres-statefulset.yaml.
            - name: PGDATA
              value: /var/lib/postgresql/data/pgdata
          readinessProbe:
            exec:
              command: ["sh", "-c", "exec pg_isready -U authentik -d authentik -h 127.0.0.1 -p 5432"]
            initialDelaySeconds: 5
            periodSeconds: 5
            timeoutSeconds: 5
            failureThreshold: 10
          livenessProbe:
            exec:
              command: ["sh", "-c", "exec pg_isready -U authentik -d authentik -h 127.0.0.1 -p 5432"]
            initialDelaySeconds: 30
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 6
          volumeMounts:
            - name: data
              mountPath: /var/lib/postgresql/data
          {{- with .Values.devAuthentik.postgresql.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.devAuthentik.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- if not .Values.devAuthentik.postgresql.persistence.enabled }}
      volumes:
        - name: data
          emptyDir: {}
      {{- end }}
  {{- if .Values.devAuthentik.postgresql.persistence.enabled }}
  volumeClaimTemplates:
    - metadata:
        name: data
        labels:
          {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-postgres") | nindent 10 }}
      spec:
        accessModes:
          - ReadWriteOnce
        resources:
          requests:
            storage: {{ .Values.devAuthentik.postgresql.persistence.size | quote }}
        {{- if .Values.devAuthentik.postgresql.persistence.storageClass }}
        storageClassName: {{ .Values.devAuthentik.postgresql.persistence.storageClass | quote }}
        {{- end }}
  {{- end }}
{{- end }}
```

- [ ] **Step 2: Write `devauthentik-postgres-service.yaml`**

Mirrors `postgres-service.yaml` exactly — headless, for stable pod DNS:

```yaml
{{- if .Values.devAuthentik.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.devAuthentikPostgresFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-postgres") | nindent 4 }}
spec:
  clusterIP: None
  ports:
    - name: postgres
      port: 5432
      targetPort: postgres
      protocol: TCP
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "devauthentik-postgres") | nindent 4 }}
{{- end }}
```

- [ ] **Step 3: Verify**

```bash
helm lint charts/nr-status
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/devauthentik-postgres-statefulset.yaml,templates/devauthentik-postgres-service.yaml
```
Expected: both objects render. **Static YAML-shape check only** — if
`kubeconform` or a `kubectl` with API-schema access is available, also run:
```bash
helm template charts/nr-status --set devAuthentik.enabled=true ... | kubeconform -summary
```
(or `kubectl apply --dry-run=client -f -`, per the base chart's own stated
verification convention). If neither tool is available in this environment,
report this schema-validation half as not run, not as passing.

- [ ] **Step 4: Commit**

```bash
git add charts/nr-status/templates/devauthentik-postgres-statefulset.yaml charts/nr-status/templates/devauthentik-postgres-service.yaml
git commit -m "Add a dedicated Postgres StatefulSet for the Kubernetes dev IdP"
```

---

### Task 9: Authentik server and worker `Deployment`s

**Files:**
- Create: `charts/nr-status/templates/devauthentik-server-deployment.yaml`
- Create: `charts/nr-status/templates/devauthentik-worker-deployment.yaml`

**Interfaces:**
- Consumes: `nr-status.devAuthentikFullname`/`devAuthentikWorkerFullname`/
  `devAuthentikPostgresFullname`, the `devauthentik-secret` (Task 7), the
  blueprints `ConfigMap` (Task 6).
- Produces: two stateless `Deployment`s (`command: server` / `command:
  worker` on the same image). Consumed by Task 10 (the server's `Service`
  fronts this `Deployment`).

- [ ] **Step 1: Write `devauthentik-server-deployment.yaml`**

`Deployment`, not `StatefulSet` — Authentik's own state lives entirely in
its Postgres, matching the design doc's own reasoning. Reuses this chart's
`nr-status.podSecurityContext`/`nr-status.containerSecurityContext` helpers
(Authentik's image runs non-root already, matching this chart's Rust-
workload posture — no `postgres`-style uid pinning needed here). `command:
server` in the reference compose becomes `args: ["server"]` in Kubernetes
(the image's own entrypoint is unchanged; only the CMD-equivalent argument
differs):

```yaml
{{- if .Values.devAuthentik.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "nr-status.devAuthentikFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-server") | nindent 4 }}
spec:
  replicas: 1
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "devauthentik-server") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-server") | nindent 8 }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" dict) | nindent 8 }}
      containers:
        - name: authentik-server
          image: {{ printf "%s:%s" .Values.devAuthentik.image.repository .Values.devAuthentik.image.tag | quote }}
          imagePullPolicy: {{ .Values.devAuthentik.image.pullPolicy }}
          args: ["server"]
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          ports:
            - name: http
              containerPort: 9000
              protocol: TCP
          env:
            - name: AUTHENTIK_SECRET_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.devAuthentikSecretName" . }}
                  key: {{ include "nr-status.devAuthentikSecretKeySecretKey" . }}
            - name: AUTHENTIK_POSTGRESQL__HOST
              value: {{ include "nr-status.devAuthentikPostgresFullname" . }}
            - name: AUTHENTIK_POSTGRESQL__USER
              value: authentik
            - name: AUTHENTIK_POSTGRESQL__NAME
              value: authentik
            - name: AUTHENTIK_POSTGRESQL__PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.devAuthentikSecretName" . }}
                  key: {{ include "nr-status.devAuthentikPostgresSecretKey" . }}
            # Deliberately NOT set: AUTHENTIK_BOOTSTRAP_PASSWORD/_HASH/
            # _EMAIL/_TOKEN -- see values.yaml's devAuthentik header comment
            # and the design doc's Bootstrap section. No default admin; the
            # blueprints ConfigMap mounted below provisions everything a
            # developer actually needs.
          volumeMounts:
            - name: blueprints
              mountPath: /blueprints/local
              readOnly: true
            # The reference compose sets shm_size: 512mb (Django/Postgres
            # shared-memory headroom); Kubernetes has no direct equivalent
            # field, so the translation is an explicitly-sized emptyDir
            # mounted at /dev/shm. The design doc's own Research mentions
            # shm_size only in its docker-compose research pass, not
            # separately for Helm -- this is a straightforward compose ->
            # Kubernetes translation of the same documented requirement,
            # not an independently researched decision; verify it's
            # sufficient during this task's live-cluster check (Task 13).
            - name: dshm
              mountPath: /dev/shm
          # Authentik's /-/health/live/ and /-/health/ready/ paths are NOT
          # independently confirmed by the design doc's research pass (its
          # Non-goals explicitly exclude hardening Authentik itself).
          # Verify against real container behaviour before trusting these;
          # a plain TCP probe on the container port is the safe fallback if
          # either path turns out wrong.
          readinessProbe:
            httpGet:
              path: /-/health/ready/
              port: http
            initialDelaySeconds: 20
            periodSeconds: 10
            timeoutSeconds: 5
            failureThreshold: 12
          livenessProbe:
            httpGet:
              path: /-/health/live/
              port: http
            initialDelaySeconds: 30
            periodSeconds: 15
            timeoutSeconds: 5
            failureThreshold: 6
          {{- with .Values.devAuthentik.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      volumes:
        - name: blueprints
          configMap:
            name: {{ include "nr-status.devAuthentikFullname" . }}-blueprints
        - name: dshm
          emptyDir:
            medium: Memory
            sizeLimit: 512Mi
      {{- with .Values.devAuthentik.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

- [ ] **Step 2: Write `devauthentik-worker-deployment.yaml`**

Same image, `args: ["worker"]`, no `ports`/probes — like the aggregator,
the worker exposes no HTTP surface; failure handling is restart-on-exit via
the default `restartPolicy`. No `/var/run/docker.sock` mount (no outposts
used, and mounting the host container socket into a Pod is a materially
worse privilege-escalation surface than under a developer's own local
Docker daemon):

```yaml
{{- if .Values.devAuthentik.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "nr-status.devAuthentikWorkerFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-worker") | nindent 4 }}
spec:
  replicas: 1
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "devauthentik-worker") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-worker") | nindent 8 }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" dict) | nindent 8 }}
      containers:
        - name: authentik-worker
          image: {{ printf "%s:%s" .Values.devAuthentik.image.repository .Values.devAuthentik.image.tag | quote }}
          imagePullPolicy: {{ .Values.devAuthentik.image.pullPolicy }}
          args: ["worker"]
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          env:
            - name: AUTHENTIK_SECRET_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.devAuthentikSecretName" . }}
                  key: {{ include "nr-status.devAuthentikSecretKeySecretKey" . }}
            - name: AUTHENTIK_POSTGRESQL__HOST
              value: {{ include "nr-status.devAuthentikPostgresFullname" . }}
            - name: AUTHENTIK_POSTGRESQL__USER
              value: authentik
            - name: AUTHENTIK_POSTGRESQL__NAME
              value: authentik
            - name: AUTHENTIK_POSTGRESQL__PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.devAuthentikSecretName" . }}
                  key: {{ include "nr-status.devAuthentikPostgresSecretKey" . }}
          volumeMounts:
            - name: blueprints
              mountPath: /blueprints/local
              readOnly: true
            - name: dshm
              mountPath: /dev/shm
          # No probes -- like the aggregator, the worker exposes no HTTP
          # surface. Failure handling is restart-on-exit via the default
          # restartPolicy.
          {{- with .Values.devAuthentik.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      volumes:
        - name: blueprints
          configMap:
            name: {{ include "nr-status.devAuthentikFullname" . }}-blueprints
        - name: dshm
          emptyDir:
            medium: Memory
            sizeLimit: 512Mi
      {{- with .Values.devAuthentik.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.devAuthentik.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

- [ ] **Step 3: Verify**

```bash
helm lint charts/nr-status
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/devauthentik-server-deployment.yaml,templates/devauthentik-worker-deployment.yaml
```
Expected: both `Deployment`s render. **Static shape check only** — whether
Authentik actually boots successfully under `readOnlyRootFilesystem: true`
plus the probes/health paths above cannot be confirmed without a live
cluster; that is Task 13's optional live check, not this task's.

- [ ] **Step 4: Commit**

```bash
git add charts/nr-status/templates/devauthentik-server-deployment.yaml charts/nr-status/templates/devauthentik-worker-deployment.yaml
git commit -m "Add server and worker Deployments for the Kubernetes dev IdP"
```

---

### Task 10: `NodePort` `Service` for the Authentik server

**Files:**
- Create: `charts/nr-status/templates/devauthentik-service.yaml`

**Interfaces:**
- Produces: a `NodePort` `Service` fronting Task 9's server `Deployment`
  only (the worker exposes no HTTP surface, so it gets none). Consumed by
  Task 7's `nr-status.devAuthentikHostAliasIP` helper (via `lookup`) and by
  the developer's browser, once the design's own documented, unenforceable
  external prerequisite (the local cluster forwarding this `NodePort` to
  loopback) is satisfied.

- [ ] **Step 1: Write the file**

```yaml
{{- if .Values.devAuthentik.enabled }}
{{/*
port and nodePort MUST be identical -- see "Discovery reachability under
Kubernetes" in the design doc: an in-cluster caller (the api Pod, via
hostAliases) and a caller on the node's published NodePort must reach
Authentik on the exact same port number, or the Host header the two send
diverges and Authentik's Host-header-derived discovery document breaks for
one side or the other. This is a direct, cheap application of this chart's
existing fail-fast posture (poller baseUrl, api.sso.*, redis.externalUrl)
to an invariant the design doc states is load-bearing.
*/}}
{{- if ne (.Values.devAuthentik.service.port | int) (.Values.devAuthentik.service.nodePort | int) }}
{{- fail "devAuthentik.service.port and devAuthentik.service.nodePort must be equal. See the design doc's Discovery reachability under Kubernetes section." }}
{{- end }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.devAuthentikFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "devauthentik-server") | nindent 4 }}
spec:
  type: NodePort
  ports:
    - name: http
      port: {{ .Values.devAuthentik.service.port }}
      nodePort: {{ .Values.devAuthentik.service.nodePort }}
      targetPort: http
      protocol: TCP
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "devauthentik-server") | nindent 4 }}
{{- end }}
```

- [ ] **Step 2: Verify**

```bash
helm lint charts/nr-status
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/devauthentik-service.yaml
```
Expected: a `NodePort` `Service` with `port: 30900` and `nodePort: 30900`
(both present). Then confirm the guard fires:
```bash
helm template charts/nr-status --set devAuthentik.enabled=true --set devAuthentik.service.nodePort=31234 \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x
```
Expected: render **fails** with the `fail` message above (`port` still
`30900`, `nodePort` now `31234`).

**Remember: rendering this `Service` correctly does not mean a developer's
browser can reach it.** Getting `NodePort 30900` forwarded to the
developer's loopback interface is the design doc's own stated
external-to-the-chart prerequisite (kind's `extraPortMappings`, `minikube
tunnel`, k3d's `--port`) — this task cannot enforce or verify that step;
Task 12 documents it, Task 13 checks it where a real cluster is available.

- [ ] **Step 3: Commit**

```bash
git add charts/nr-status/templates/devauthentik-service.yaml
git commit -m "Add the NodePort Service fronting the Kubernetes dev IdP"
```

---

### Task 11: Wire `devAuthentik` into `api-deployment.yaml` and `secret.yaml`

**Files:**
- Modify: `charts/nr-status/templates/api-deployment.yaml`
- Modify: `charts/nr-status/templates/secret.yaml`

**Interfaces:**
- Consumes: `nr-status.devAuthentikIssuerUrl`/`ClientId`/`ClientSecret`/
  `RedirectUrl`/`PostLoginRedirectUrl`/`HostAliasIP` (Task 7).
- Produces: `api-deployment.yaml`'s five `fail` guards become conditional
  on `devAuthentik.enabled`; its `SSO_*` env values fall back to computed
  `devAuthentik.*` defaults when `api.sso.*` is empty; its pod spec gains a
  conditional `hostAliases` entry. `secret.yaml`'s `sso-client-secret`
  entry gains the same fallback.

- [ ] **Step 1: Make the `api.sso.*` fail guards conditional**

In `charts/nr-status/templates/api-deployment.yaml`, replace the existing
top-of-file block (the five `{{- if not .Values.api.sso.X }}{{- fail ... }}{{- end }}`
guards, currently lines 1-28) with:

```yaml
{{/*
Fail-fast on an unusable SSO config -- same pattern as before, EXCEPT an
empty api.sso.* value is no longer automatically fatal when
devAuthentik.enabled: true. In that case the value is computed instead,
from devAuthentik.* and the fixed blueprint-provisioned client id/secret
(charts/nr-status/templates/_helpers.tpl's nr-status.devAuthentik* helpers)
-- see docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md's Helm
chart support section. An operator's explicit api.sso.* value ALWAYS wins
over the computed default; devAuthentik never overrides one.
*/}}
{{- $issuerUrl := .Values.api.sso.issuerUrl | default (and .Values.devAuthentik.enabled (include "nr-status.devAuthentikIssuerUrl" .)) -}}
{{- $clientId := .Values.api.sso.clientId | default (and .Values.devAuthentik.enabled (include "nr-status.devAuthentikClientId" .)) -}}
{{- $redirectUrl := .Values.api.sso.redirectUrl | default (and .Values.devAuthentik.enabled (include "nr-status.devAuthentikRedirectUrl" .)) -}}
{{- $postLoginRedirectUrl := .Values.api.sso.postLoginRedirectUrl | default (and .Values.devAuthentik.enabled (include "nr-status.devAuthentikPostLoginRedirectUrl" .)) -}}
{{- $clientSecretAvailable := or .Values.api.sso.clientSecret .Values.api.sso.existingSecret .Values.devAuthentik.enabled -}}
{{- if not $issuerUrl }}
{{- fail "api.sso.issuerUrl is empty and devAuthentik.enabled is false. Set api.sso.issuerUrl to your OIDC provider's issuer base URL (e.g. --set api.sso.issuerUrl=https://sso.example.com/realms/rail), or set devAuthentik.enabled=true to use the bundled local dev IdP instead." }}
{{- end }}
{{- if not $clientId }}
{{- fail "api.sso.clientId is empty and devAuthentik.enabled is false. Set api.sso.clientId, or set devAuthentik.enabled=true." }}
{{- end }}
{{- if not $clientSecretAvailable }}
{{- fail "api.sso.clientSecret is empty, no api.sso.existingSecret was given, and devAuthentik.enabled is false. Set one of them, or set devAuthentik.enabled=true." }}
{{- end }}
{{- if not $redirectUrl }}
{{- fail "api.sso.redirectUrl is empty and devAuthentik.enabled is false. Set api.sso.redirectUrl (not the api's own origin -- see this chart's README), or set devAuthentik.enabled=true." }}
{{- end }}
{{- if not $postLoginRedirectUrl }}
{{- fail "api.sso.postLoginRedirectUrl is empty and devAuthentik.enabled is false. Set api.sso.postLoginRedirectUrl, or set devAuthentik.enabled=true." }}
{{- end }}
```

- [ ] **Step 2: Use the computed values in the env section**

Further down in the same file, in the container `env` list, replace the
existing four plain-value `SSO_*` entries (leave `SSO_CLIENT_SECRET`'s
`secretKeyRef` form unchanged — its fallback is handled in `secret.yaml`,
Step 4 below):

```yaml
            - name: SSO_ISSUER_URL
              value: {{ $issuerUrl | quote }}
            - name: SSO_CLIENT_ID
              value: {{ $clientId | quote }}
            - name: SSO_CLIENT_SECRET
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.ssoClientSecretName" . }}
                  key: {{ include "nr-status.ssoClientSecretKey" . }}
            - name: SSO_REDIRECT_URL
              value: {{ $redirectUrl | quote }}
            - name: SSO_POST_LOGIN_REDIRECT_URL
              value: {{ $postLoginRedirectUrl | quote }}
```

- [ ] **Step 3: Add the conditional `hostAliases` entry**

In the same file, in the pod spec (immediately after
`automountServiceAccountToken: false`, before `imagePullSecrets`):

```yaml
      {{- $hostAliasIP := include "nr-status.devAuthentikHostAliasIP" . }}
      {{- if and .Values.devAuthentik.enabled $hostAliasIP }}
      # Makes this Pod's own outbound OIDC discovery request present the
      # SAME Host header the developer's browser sends via the NodePort --
      # see "Discovery reachability under Kubernetes" in the design doc. On
      # a from-scratch helm install this may be ABSENT (see
      # nr-status.devAuthentikHostAliasIP's own comment) -- that is a known,
      # documented gap requiring one `helm upgrade`, not a bug.
      hostAliases:
        - ip: {{ $hostAliasIP }}
          hostnames:
            - {{ .Values.devAuthentik.hostname }}
      {{- end }}
```

- [ ] **Step 4: Add the fallback to `secret.yaml`**

In `charts/nr-status/templates/secret.yaml`, replace the existing
`sso-client-secret` block:

```yaml
{{/* sso-client-secret: like before, never auto-generated with a random
     value -- but when devAuthentik.enabled and no explicit clientSecret is
     set, falls back to the FIXED blueprint-provisioned dev secret
     (nr-status.devAuthentikClientSecret) instead of an empty string, so
     the computed defaults in api-deployment.yaml actually work end to
     end. */}}
{{- if not .Values.api.sso.existingSecret -}}
{{- $ssoSecret := .Values.api.sso.clientSecret | default (and .Values.devAuthentik.enabled (include "nr-status.devAuthentikClientSecret" .)) | default "" -}}
{{- $_ := set $data "sso-client-secret" ($ssoSecret | b64enc) -}}
{{- end -}}
```

- [ ] **Step 5: Verify — new computed-default path renders**

```bash
helm lint charts/nr-status
helm template charts/nr-status --set devAuthentik.enabled=true --set devAuthentik.hostAliasIP=10.96.0.42 \
  --show-only templates/api-deployment.yaml
```
Expected: renders without any `fail` (previously this combination —
`devAuthentik.enabled: true`, `api.sso.*` all empty — would have failed via
the original unconditional guards). Grep the output for
`SSO_ISSUER_URL`/`SSO_CLIENT_ID`/`SSO_REDIRECT_URL`/
`SSO_POST_LOGIN_REDIRECT_URL` and confirm they show the computed
`devAuthentik`-derived values, and for `hostAliases` to confirm the entry
is present (since `hostAliasIP` was set explicitly here).

- [ ] **Step 6: Verify — explicit `api.sso.*` still wins**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true --set devAuthentik.hostAliasIP=10.96.0.42 \
  --set api.sso.issuerUrl=https://real-idp.example.com/realms/rail \
  --set api.sso.clientId=real-client --set api.sso.clientSecret=real-secret \
  --set api.sso.redirectUrl=https://real.example.com/api/auth/callback \
  --set api.sso.postLoginRedirectUrl=https://real.example.com/ \
  --show-only templates/api-deployment.yaml | grep -A1 SSO_ISSUER_URL
```
Expected: shows `https://real-idp.example.com/realms/rail`, not the
computed `devAuthentik` value — confirms an explicit operator value is
never overridden even when `devAuthentik.enabled` is also `true`.

- [ ] **Step 7: Verify — the original real-IdP path still fails as before**

```bash
helm template charts/nr-status
```
(no overrides at all — `devAuthentik.enabled` defaults `false`, `api.sso.*`
all empty). Expected: render still **fails**, with the same class of
message as before this task (confirms this task did not accidentally make
the SSO config optional for a real-IdP install).

- [ ] **Step 8: Verify — `hostAliases` absence when `lookup` can't resolve**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x \
  --show-only templates/api-deployment.yaml | grep -c hostAliases
```
Expected: `0` — with no `devAuthentik.hostAliasIP` override and no live
cluster to `lookup` against (`helm template` always sees `lookup` return
empty, same documented limitation as `secret.yaml`), no `hostAliases` entry
is written. **This is the correct behavior per Task 7's own helper
contract, but it also means this static check cannot confirm the
`lookup`-against-a-live-Service path itself works — only Task 13's
optional live-cluster check can.**

- [ ] **Step 9: Commit**

```bash
git add charts/nr-status/templates/api-deployment.yaml charts/nr-status/templates/secret.yaml
git commit -m "Compute api.sso.* defaults and a hostAliases entry from devAuthentik"
```

---

### Task 12: Documentation — `NOTES.txt` and README

**Files:**
- Modify: `charts/nr-status/templates/NOTES.txt`
- Modify: `charts/nr-status/README.md`

**Interfaces:** none — this task only surfaces what Tasks 5-11 already
built, plus the design's own open risks, to whoever runs `helm install`.

- [ ] **Step 1: Add a `devAuthentik`-enabled block to `NOTES.txt`**

Insert after the existing "Read the internal token" block, before the
poller-status block:

```
{{- if .Values.devAuthentik.enabled }}

############################################################################
# Local dev identity provider (devAuthentik) is ENABLED.
############################################################################

This is a throwaway, unhardened local Authentik instance for exercising
this app's own login flow -- see charts/nr-status/README.md's "Local dev
identity provider" section and
docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md for the full
design and its OPEN RISKS, not fully verified against a real cluster as of
this writing:

1. Your local cluster tool must forward NodePort
   {{ .Values.devAuthentik.service.nodePort }} to your machine's loopback
   interface, AT THE SAME PORT NUMBER, or your browser cannot reach
   Authentik at all. This is outside this chart's control -- see the
   worked examples in charts/nr-status/README.md (kind's
   extraPortMappings, `minikube tunnel`, k3d's `--port`).

2. If this is your FIRST `helm install` with devAuthentik.enabled=true, and
   you did not set devAuthentik.hostAliasIP explicitly, the api
   Deployment's hostAliases entry could not be written yet --
   devauthentik-service did not exist when api was rendered in this same
   release. Run:

     helm upgrade {{ .Release.Name }} <chart> -n {{ .Release.Namespace }} --reuse-values

   once, immediately, so api picks up the now-existing Service's
   ClusterIP. This is a known, documented gap, not a bug you're hitting by
   accident.

Once both of the above are done, reach Authentik itself at:

  http://{{ .Values.devAuthentik.hostname }}:{{ .Values.devAuthentik.service.nodePort }}/

There is NO default admin account (deliberately -- see the design doc's
Bootstrap section). Sign in through THIS APP's own login flow --
devAuthentik provisions open, self-service signup. To get into Authentik's
OWN admin UI instead, use its interactive first-run flow:

  http://{{ .Values.devAuthentik.hostname }}:{{ .Values.devAuthentik.service.nodePort }}/if/flow/initial-setup/

api.sso.redirectUrl/postLoginRedirectUrl were computed assuming the
frontend is reachable at exactly http://localhost:3000, e.g.:

  kubectl port-forward -n {{ .Release.Namespace }} svc/{{ include "nr-status.frontendFullname" . }} 3000:{{ .Values.frontend.service.port }}
{{- end }}
```

- [ ] **Step 2: Add a "Local dev identity provider" README section**

Insert into `charts/nr-status/README.md`, after the existing "Single
sign-on (OIDC)" section (currently ending around line 239) and before
"Using an external database" (currently line 240):

```markdown
## Local dev identity provider (devAuthentik)

For a local/dev Kubernetes cluster (kind, minikube, k3d) only — set
`devAuthentik.enabled: true` to bring up a throwaway local Authentik
instance and skip registering this app with a real external IdP entirely.
Mirrors `docker-compose.authentik.yml`'s job for the `docker compose`
deployment path. **Off by default; an install pointed at a real external
IdP is completely unaffected.**

When enabled and `api.sso.*` is left at its empty default, the chart
computes it from `devAuthentik.*` and the fixed, blueprint-provisioned
dev-only OIDC client (`client_id: nr-status-dev`) — no manual IdP-side
setup, matching `docker-compose.authentik.yml`'s own zero-click bootstrap.
An explicit `api.sso.*` value always wins.

```yaml
devAuthentik:
  enabled: true
```

**Two manual steps this chart cannot do for you:**

1. **Forward `devAuthentik.service.nodePort` (default `30900`) to your
   machine's loopback interface**, using whatever mechanism your cluster
   tool provides:
   - kind: an `extraPortMappings` entry in your cluster config, e.g.
     ```yaml
     nodes:
       - role: control-plane
         extraPortMappings:
           - containerPort: 30900
             hostPort: 30900
     ```
   - minikube: `minikube tunnel`
   - k3d: `k3d cluster create --port 30900:30900@loadbalancer`

   This step lives in your cluster-creation config, not in this chart —
   see the design doc's Research for why a Helm chart cannot bind a port
   on the host machine itself.

2. **On your very first `helm install` with `devAuthentik.enabled: true`**
   (unless you also set `devAuthentik.hostAliasIP` explicitly), run `helm
   upgrade` once immediately afterward. `NOTES.txt` reminds you of this
   after every install/upgrade where `devAuthentik.enabled` is `true`.

**Known limitations / unverified, stated plainly rather than assumed
solved:**

- The `hostAliases`-vs-`lookup` first-install ordering gap above has no
  fully graceful degradation — it requires the one-time `helm upgrade`
  workaround, not a design this chart claims to have fully closed.
- The NodePort-forwarding step is entirely outside this chart's control and
  cannot be verified from inside a `helm template`/`helm install` run — if
  you skip it, the chart installs cleanly and the failure only shows up as
  "the browser can't reach Authentik," with no render-time signal.
- None of the above has been smoke-tested by this chart's own authors
  against a real kind/minikube/k3d cluster as of this writing — see
  `docs/superpowers/plans/2026-08-29-dev-oidc-server.md`'s Task 13 for
  what was and wasn't actually verified.

See `docs/superpowers/specs/2026-08-29-dev-oidc-server-design.md` for the
full design, including why `AUTHENTIK_BOOTSTRAP_*` is deliberately never
set (no default admin — see its Bootstrap section) and why this is a
hand-rolled deployment rather than the official `goauthentik/helm` chart
(see its Non-goals).
```

- [ ] **Step 3: Add a `### devAuthentik` values-reference subsection**

In the "Values reference" section, add a `### devAuthentik` subsection
(same table format as the existing `### postgresql`/`### api` subsections)
immediately after `### api`, listing every field from Task 5's values.yaml
block with its default and a one-line description.

- [ ] **Step 4: Verify**

```bash
helm lint charts/nr-status
```
Expected: clean — `NOTES.txt` is templated, so a syntax error there would
fail lint. There is no automated check for documentation *accuracy* beyond
this and human review; this task's own earlier tasks already wrote the
comments this documentation cross-references, so the main risk is drift,
not fabrication.

- [ ] **Step 5: Commit**

```bash
git add charts/nr-status/templates/NOTES.txt charts/nr-status/README.md
git commit -m "Document the devAuthentik local dev IdP path in NOTES.txt and the README"
```

---

### Task 13: Final verification

**Files:** none (verification only)

- [ ] **Step 1: `helm lint`**

Run: `helm lint charts/nr-status`
Expected: clean, no warnings introduced by this plan's changes.

- [ ] **Step 2: Confirm the real-IdP path is unaffected**

```bash
helm template charts/nr-status \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x
```
Expected: renders the full stack exactly as it did before this plan
(`devAuthentik.enabled` defaults `false`; no `devauthentik-*` objects
appear). Then:
```bash
helm template charts/nr-status
```
Expected: **fails**, same as before this plan (`api.sso.*` required, no
`devAuthentik` opt-out of that requirement by default).

- [ ] **Step 3: Confirm the full `devAuthentik` stack renders**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true
```
Expected: renders cleanly with computed `api.sso.*` defaults and every
`devauthentik-*` object, no `fail` triggered.

- [ ] **Step 4: Confirm `devAuthentik` composes with an external main database**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true \
  --set postgresql.enabled=false \
  --set externalDatabase.url=postgres://nr_status:s3cret@db.example.com:5432/nr_status
```
Expected: renders cleanly — confirms `devAuthentik`'s own dedicated
Postgres is fully independent of the app's own `DATABASE_URL` choice, per
the design's stated independence rationale.

- [ ] **Step 5: Schema check on the rendered output**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true | kubeconform -summary
```
(or `helm template ... | kubectl apply --dry-run=client -f -` if `kubectl`
with API-schema access is available instead). Expected: no schema errors.
**If neither `kubeconform` nor a schema-aware `kubectl` is available in
this environment, report this step as not run, not as passing.**

- [ ] **Step 6: Confirm no secret value leaks outside a `Secret` object**

```bash
helm template charts/nr-status --set devAuthentik.enabled=true \
  | awk '/^kind: Secret/{s=1} /^kind: [A-Za-z]+/{if($2!="Secret")s=0} !s' \
  | grep -iE "authentik-secret-key|devauthentik.*password"
```
Expected: no output — the only place those key *names* legitimately appear
outside a `Secret` object is as a `secretKeyRef.key` reference (a name, not
a value), which this grep does not match against. The fixed
`nr-status-dev`/`nr-status-dev-local-only-not-a-real-secret` client
id/secret literals are **expected** to appear in plain rendered `env`
entries or inside the `devauthentik-blueprints` `ConfigMap` — they are
deliberately not sensitive, per Global Constraints.

- [ ] **Step 7 (optional — requires a real local Kubernetes cluster, NOT
  assumed available in this environment): live Helm verification**

```bash
kind create cluster --config <config with the 30900 extraPortMappings entry from Task 12>
helm install nr-status-test charts/nr-status -n nr-status-test --create-namespace \
  --set devAuthentik.enabled=true --set devAuthentik.hostAliasIP=<predictable kind service CIDR IP>
kubectl get pods -n nr-status-test -w
```
Then, per the documented gap: `helm upgrade nr-status-test charts/nr-status
-n nr-status-test --reuse-values` once. Confirm every `devauthentik-*` pod
reaches `Ready`, and — in a real browser — that
`http://authentik.localhost:30900/` loads with no Host-header rejection,
closing the design doc's own admitted "none of this was smoke-tested
against a real cluster" risk for real, or documenting exactly where it
still fails if it doesn't. **If this step cannot be run, state so plainly
in this task's record rather than letting Steps 1-6's static passes stand
in for it.**

- [ ] **Step 8: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree (everything committed task-by-task above).
