# Local Dev OIDC Server (Authentik) — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md` and
`docs/superpowers/specs/2026-08-18-helm-chart-design.md`) so it can be
reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. It does **not**
contain a task-by-task implementation plan or any `docker-compose`/YAML
changes, and it does not touch `charts/nr-status`'s actual
`values.yaml`/templates — those are separate, later steps in this repo's
process, done only after a design like this has been reviewed.

## Problem

`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md` and its
implementation plan added real OIDC-backed login to `crates/api`:
`crates/api/src/data/config.rs:58-106` requires `SSO_ISSUER_URL`,
`SSO_CLIENT_ID`, `SSO_CLIENT_SECRET`, `SSO_REDIRECT_URL` and
`SSO_POST_LOGIN_REDIRECT_URL` with no defaults, `docker-compose.yml:103-107`
fails config resolution (`${VAR:?...}`) if any of the five are unset, and
`crates/api/src/auth/oidc.rs` is a real `openidconnect`/`oauth2` relying
party that does lazy `.well-known/openid-configuration` discovery
(`crates/api/src/auth/oidc.rs:129-137`) against whatever issuer it's given.

None of that has a real issuer to talk to out of the box. Both
`dev.env.example:119-124` and `local.env.example:77-82` ship the OIDC block
pointed at `http://sso.example.invalid` — an RFC 2606 placeholder chosen
deliberately so the stack still boots (discovery is lazy) but every actual
sign-in attempt fails, exactly as `dev.env.example:115-118` documents:
"the stack comes up fine and only an actual sign-in attempt fails until you
point it at a real server." Getting a real server today means an operator
has already registered this app with an externally-hosted IdP (Keycloak,
Auth0, etc.) and handed over an issuer URL, client id and client secret —
manual, out-of-band, and undocumented anywhere in this repo.

The consequence: nobody can `git clone` this repo, run
`docker compose --env-file dev.env up --build`, and exercise the login flow
end to end. Every other piece of the dev stack — Postgres, Redis, the
pollers (against RDM placeholders, admittedly also non-functional, but that
gap is pre-existing and out of scope here), the aggregator, the frontend —
comes up self-contained. SSO is the one piece that structurally can't,
because there is no way today to spin up a *local* identity provider as
part of the stack.

This design closes that gap: an opt-in local OIDC IdP (Authentik) that a
developer can add to their compose stack to get a fully working login flow
with zero manual clicking, while leaving the door open — with zero added
friction — to keep pointing at a real external IdP instead.

## Goals

- A developer can bring up a local Authentik instance as part of
  `docker compose`, register/log into this app through it, and see a real
  session cookie and a real user row in `users` — no manual IdP-side setup.
- Open, self-service signup on the local IdP: anyone hitting it can create
  an account, no invitation/admin-provisioning step.
- No real default admin credential is baked into the committed compose
  config.
- Authentik's own state (its users, sessions, and its own config —
  including whatever pre-provisions this app's OIDC client) survives
  `docker compose down` / `up` cycles, matching this repo's existing
  `postgres_data` convention.
- A developer who already has a real IdP to test against is completely
  unaffected: no local Authentik container starts, no extra image pull,
  no extra `SSO_*` values required, unless they explicitly opt in.
- The same local Authentik IdP is available as a second, equally opt-in
  path for a developer running this app's Helm chart
  (`charts/nr-status/`, design in
  `docs/superpowers/specs/2026-08-18-helm-chart-design.md`) against a
  local/dev Kubernetes cluster (kind, minikube, k3d — see Design → Helm
  chart support below), via a `values.yaml` flag defaulting to `false`.
  An operator who installs the chart against a real external IdP — the
  chart's existing, documented behaviour today — is completely
  unaffected: `api.sso.*` stays required with no default, exactly as it
  is now, unless the flag is explicitly set.

## Non-goals

- **Production Authentik deployment, hardening, TLS/cert-manager
  automation, SMTP/email, HA, backup/restore, or autoscaling for
  Authentik itself, on either deployment path.** Both the
  `docker-compose` path and the Helm path added by this design are
  strictly a *dev convenience* — a disposable local IdP for exercising
  this app's login flow — not a production identity provider deployment.
  The Helm chart's own non-goals (no HPA, no backup/restore for its
  bundled Postgres,
  `docs/superpowers/specs/2026-08-18-helm-chart-design.md`) apply with
  equal force to the Authentik-specific infrastructure this design adds
  to that chart.
- **Using Authentik's own official Helm chart (`goauthentik/helm`) as a
  subchart dependency of `charts/nr-status`.** Considered and explicitly
  rejected — see Research → Helm chart deployment below. It depends on
  Bitnami's PostgreSQL chart (an OCI subchart) plus an
  `authentik-remote-cluster` chart dependency, which would require
  `helm dependency update`/`helm dependency build` and reachability to
  external chart registries at install time — directly contradicting
  `charts/nr-status`'s stated "one chart, one namespace, one command. No
  subchart dependencies... installable in an air-gapped cluster given
  the images" goal
  (`docs/superpowers/specs/2026-08-18-helm-chart-design.md:19-21`).
- LDAP/SAML sources, MFA, outposts (reverse-proxy/forward-auth), or any
  Authentik feature this app doesn't need as a plain OIDC relying party.
- Fixing the pre-existing RDM-feed placeholder gaps (`dev.env.example`'s
  `*.example.invalid` `RDM_*` values) — unrelated, already tracked in
  those files' own comments.
- A general "run any IdP" framework. Per the brief, this is Authentik,
  specifically, not an abstraction over multiple providers.
- Email verification / password recovery for local Authentik accounts (no
  SMTP is configured) — an accepted limitation of a throwaway dev IdP, not
  an oversight; see Non-goals-adjacent note under Bootstrap below.

## Research

### Authentik version and official deployment shape

Fetched directly from `https://goauthentik.io/docker-compose.yml`
(2026-08-29): the current default image tag is
`ghcr.io/goauthentik/server:2026.8.0`, published 2026-08-18 per
`https://api.github.com/repos/goauthentik/authentik/releases` (stable, not
an `-rc`). Authentik moved from a 2-month to a 3-month release cadence
starting in 2026 (`2026.2` → `2026.5` → `2026.8`); it supports the two most
recent version families. `:latest` has been frozen/deprecated since 2025.2
— pin to an explicit tag, `2026.8.0` on current evidence, matching this
repo's existing posture on pinning (e.g. the LLM model pinning rationale in
`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`).

The official compose is three services, not four:

```yaml
services:
  postgresql:   # postgres:16-alpine, its own named volume
  server:       # ghcr.io/goauthentik/server:2026.8.0, command: server
  worker:       # same image, command: worker, runs as root, mounts
                # /var/run/docker.sock (for outpost/container management)
```

**No Redis service is present.** This overturns an assumption in the
original brief (and a reasonable one — older Authentik releases did need
Redis for the worker's task queue). Research corroborates that somewhere
in the 2025.4–2026.6 release range Authentik moved worker task queuing off
Redis and onto Postgres, and the currently-published official compose
confirms it: fetched today, it defines exactly `postgresql`, `server`, and
`worker`, with no `redis` block and no `AUTHENTIK_REDIS__HOST` variable.
This simplifies the "shared vs. separate Redis" question the brief asked
for down to: there isn't one to make. See Design → Data services below.

Required env vars with no default: `AUTHENTIK_SECRET_KEY` (Django secret
key — mandatory, server and worker both fail to start without it) and the
Postgres credential trio (`PG_PASS` required, `PG_DB`/`PG_USER` default to
`authentik`). `shm_size: 512mb` is set on both server and worker.

Sources: [goauthentik.io/docker-compose.yml](https://goauthentik.io/docker-compose.yml)
(fetched 2026-08-29); [goauthentik/authentik releases](https://github.com/goauthentik/authentik/releases)
(`version/2026.8.0`, published 2026-08-18T22:04:25Z).

### Bootstrap / default admin

Authentik's automated-install docs
(`https://docs.goauthentik.io/install-config/automated-install/`) define
four bootstrap env vars, each documented as **"only read on the first
startup"**:

- `AUTHENTIK_BOOTSTRAP_PASSWORD` — plaintext password for the built-in
  `akadmin` user. Docs explicitly warn this stores the password in plain
  text in the environment and recommend the hash variant instead.
- `AUTHENTIK_BOOTSTRAP_PASSWORD_HASH` — same, but a pre-hashed Django value
  (generate via `docker compose run --rm server hash_password`).
  Setting both `_PASSWORD` and `_PASSWORD_HASH` is an error.
- `AUTHENTIK_BOOTSTRAP_TOKEN` — an API token for `akadmin`.
- `AUTHENTIK_BOOTSTRAP_EMAIL` — `akadmin`'s email address.
- None of the four accept the `file://`-secret indirection Authentik
  supports for most other settings.

If none of these are set, Authentik falls back to its interactive
first-visit **initial-setup flow** (`/if/flow/initial-setup/`), which
prompts whoever opens the UI first to set `akadmin`'s password themselves.
The `akadmin` user row itself already exists before this flow runs (created
by migrations) — it just has no usable password until either the flow
completes or a bootstrap var sets one.

**Recommendation: leave all four unset in the committed compose config —
genuinely no default admin, not a placeholder.** This is workable
specifically *because* provisioning this app's OIDC client and enabling
open signup (below) are both done via **blueprints**, which apply
independently of any admin account ever having logged in — a developer
exercising this app's login flow as an end user never needs to touch
Authentik's own admin UI at all. The admin account only matters if someone
wants to go poke around inside Authentik itself (inspect users, debug a
flow), which is optional, secondary, and can happen after the fact.

This is not risk-free — see Open Questions.

Source: [Automated install | authentik](https://docs.goauthentik.io/install-config/automated-install/)
(fetched 2026-08-29); [I can't log in to authentik | authentik](https://docs.goauthentik.io/troubleshooting/login/)
(fetched 2026-08-29, for the recovery-key fallback below).

### Blueprints: declarative first-boot configuration

Blueprints are YAML files describing Authentik objects (flows, stages,
policies, providers, applications, brands, users, ...) applied
transactionally against the database. Authentik's own defaults — e.g. the
built-in `default-authentication-flow` — are themselves shipped as
blueprints. Mechanically:

- Mounted into the container at `/blueprints` (subdirectories `default`,
  `example`, `system` in the shipped image); a deployment adds its own
  under a directory of its choosing, e.g. `/blueprints/local`.
- Re-applied on a schedule (every 60 minutes) and additionally
  file-watched, so editing a mounted blueprint file triggers a re-apply
  without a restart.
- Confirmed directly from the upstream repo (`goauthentik/authentik`,
  `blueprints/` tree, fetched 2026-08-29) that blueprints can create an
  `authentik_providers_oauth2.oauth2provider` and an
  `authentik_core.application` with **fixed, literal `client_id` and
  `client_secret` values** — i.e. this app's `SSO_CLIENT_ID`/
  `SSO_CLIENT_SECRET` can be deterministic, known-in-advance strings for
  local dev, with no UI step to go copy them out of Authentik after the
  fact. Current (2025+) provider schema for `redirect_uris` is a list of
  `{url, matching_mode}` objects, not a bare string — e.g.:

  ```yaml
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

  (Illustrative, not verified against a running instance — see Open
  Questions on first-boot ordering.) `signing_key` references the
  self-signed certificate keypair Authentik itself creates via its own
  default blueprints on first boot; `authorization_flow` picks the
  implicit-consent variant of Authentik's shipped provider-authorization
  flow so a fresh local-dev signup isn't also stopped by a consent screen.

- Blueprints can also set which flow a Brand/Identification stage uses —
  covered next, for open signup specifically.

Source: [Blueprints | authentik](https://docs.goauthentik.io/customize/blueprints/)
and [Working with blueprints | authentik](https://docs.goauthentik.io/customize/blueprints/working_with_blueprints/)
(fetched 2026-08-29); `blueprints/default/*.yaml` and
`blueprints/example/flows-enrollment-2-stage.yaml` in
[goauthentik/authentik](https://github.com/goauthentik/authentik) (fetched
2026-08-29, raw file contents below).

### Open signup: which flow, and how it's wired in

The brief's hunch — a default enrollment flow needs to be set somewhere —
is right, but the specific flow it points to matters and the obvious one
is a trap:

- `blueprints/default/flow-default-source-enrollment.yaml` (fetched
  verbatim from the upstream repo) defines `default-source-enrollment`,
  gated by a policy expression `return ak_is_sso_flow` — **this flow only
  fires for users arriving via an external source (Google/GitHub/SAML/
  etc.)**. It is not a general-purpose self-registration flow and using it
  for that purpose would silently no-op (the SSO-flow policy blocks it).
- The correct building block is `blueprints/example/flows-enrollment-2-stage.yaml`
  — a standalone flow (`designation: enrollment`,
  `authentication: require_unauthenticated`) that prompts for username,
  password, name and email with **no verification stage**, then
  auto-logs-in the new user via a `userwritestage` + `userloginstage`
  pair. It ships with `blueprints.goauthentik.io/instantiate: "false"` in
  its metadata labels — a marker meaning "library example, not
  auto-applied" — so a copy of it for this app's blueprint would drop
  that label (or flip it to `"true"`) so it actually applies at startup.
- Having the flow exist isn't enough to put a "Sign up" link in front of a
  developer, though. Fetched `blueprints/default/flow-default-authentication-flow.yaml`
  directly: the shipped `default-authentication-identification`
  `IdentificationStage` sets only `user_fields`, with no
  `enrollment_flow`. Per Authentik's own Identification-stage docs
  (fetched 2026-08-29), the stage's `Enrollment flow` field is what
  renders the "Need an account? Sign up." link under the login form — and
  it is unset by default. A `authentik_brands.brand` has default flows for
  authentication/invalidation/recovery/user-settings/etc., but (confirmed
  against `blueprints/default/default-brand.yaml`) **no enrollment-flow
  field at all** — enrollment is wired at the identification-stage level,
  not the brand level.

  So the local-IdP blueprint needs two things together: the enrollment
  flow itself (adapted from `flows-enrollment-2-stage.yaml`, given a fixed
  slug e.g. `nr-status-dev-enrollment`), and a second blueprint entry that
  finds and updates the existing identification stage:

  ```yaml
  model: authentik_stages_identification.identificationstage
  identifiers:
    name: default-authentication-identification
  attrs:
    enrollment_flow: !Find [authentik_flows.flow, [slug, nr-status-dev-enrollment]]
  ```

This satisfies the "open signup, no invite" requirement declaratively —
no clicking through Flows → Stages in the admin UI.

Sources: raw `blueprints/default/flow-default-source-enrollment.yaml`,
`blueprints/example/flows-enrollment-2-stage.yaml`,
`blueprints/default/flow-default-authentication-flow.yaml`,
`blueprints/default/default-brand.yaml`
([goauthentik/authentik](https://github.com/goauthentik/authentik) `main`,
fetched 2026-08-29); [Identification stage | authentik](https://docs.goauthentik.io/add-secure-apps/flows-stages/stages/identification/)
(fetched 2026-08-29).

### The browser-vs-container discovery reachability problem

This is a real, load-bearing gotcha specific to running the IdP *inside*
the same compose network as the app under test, not a generic Authentik
fact — worth spelling out because it would silently break login if missed.

`crates/api/src/auth/oidc.rs:129-137` does OIDC discovery exactly once,
from inside the `api` container, against `SSO_ISSUER_URL`. The resulting
`authorization_endpoint` — taken verbatim from whatever
`.well-known/openid-configuration` returns — is then handed straight to
the *browser* as a redirect target (`crates/api/src/auth/oidc.rs:151-163`,
`authorize_url`). For a real external IdP this is a non-issue: the issuer
has one public hostname, reachable identically from the container and the
browser. Self-hosted Authentik behind Docker, though, is well-documented
(see e.g. [goauthentik/authentik#3405](https://github.com/goauthentik/authentik/issues/3405),
fetched 2026-08-29 — a Kubernetes-flavored instance of the same class of
bug) to build its discovery document's URLs from the **Host header of the
request that asked for it**. If the `api` container reaches Authentik via
the Docker-internal service name (e.g. `http://authentik:9000/...`), the
issuer hands back `authorization_endpoint: http://authentik:9000/...` — a
hostname the developer's actual browser, running on the host, cannot
resolve.

The two vantage points (the `api` container's network, and the host
machine's browser) need to resolve **the exact same hostname:port string**
to the same Authentik instance for this to work at all. The proposed fix:
give the Authentik service a Compose network alias using a hostname in the
`*.localhost` space (e.g. `authentik.localhost`), and publish its HTTP
port to the host under that same port number:

```yaml
authentik-server:
  ports:
    - "9000:9000"
  networks:
    default:
      aliases:
        - authentik.localhost
```

- From inside the compose network, `api` resolves `authentik.localhost`
  via Docker's embedded DNS straight to the Authentik container (the
  alias), no loopback involved.
- From the host, the browser resolves `authentik.localhost` to `127.0.0.1`
  — modern browsers (Chrome, Firefox) special-case the `.localhost` TLD
  per RFC 6761 and loop it back without any OS/`/etc/hosts` configuration
  — landing on the published port, which forwards into the same
  container.

Both sides reach the identical container using the identical string
`authentik.localhost:9000`, so the Host-header-derived discovery document
is consistent for both. `SSO_ISSUER_URL` becomes
`http://authentik.localhost:9000/application/o/nr-status/`. This is
proposed, not verified end-to-end (no container was actually started for
this research pass) — flagged again under Open Questions.

### Helm chart deployment: official chart, blueprint delivery, and local-cluster reachability

Researched 2026-08-29, specifically to check none of the above is
assumed rather than confirmed for Kubernetes.

**Authentik does publish an official Helm chart** — `goauthentik/helm`
(chart `authentik`, at `charts.goauthentik.io` / Artifact Hub), org-owned,
actively released (chart/appVersion `2026.8.0`, matching the current
Docker tag; 470+ commits, CI lint-test workflow, 182 GitHub stars as of
this research). It is *not* unmaintained or abandoned. But fetched
directly from `github.com/goauthentik/helm/blob/main/charts/authentik/Chart.yaml`,
its `dependencies:` block is:

```yaml
dependencies:
  - name: postgresql
    version: 18.8.13
    repository: oci://registry-1.docker.io/bitnamicharts
    condition: postgresql.enabled
  - name: authentik-remote-cluster
    version: 2.1.0
    repository: https://charts.goauthentik.io
    condition: serviceAccount.create
    alias: serviceAccount
```

That is a real subchart dependency on an external OCI registry (Bitnami's
`postgresql` chart) plus a second chart dependency, both requiring `helm
dependency build`/`update` and network access to registries this repo does
not control. **Decision: do not use it as a chart dependency.** This isn't
a default-to-hand-rolling-because-it's-simpler call — the official chart
is good and well-maintained — it's that `charts/nr-status` states as a
Goal, not a preference, "One chart, one namespace, one command. No
subchart dependencies, no `helm dependency update`... installable in an
air-gapped cluster given the images"
(`docs/superpowers/specs/2026-08-18-helm-chart-design.md:19-21`), and the
official Authentik chart cannot be pulled in without breaking exactly that
property. `charts/nr-status` already makes this same call for its *own*
Postgres and Redis — hand-rolled `StatefulSet`/`Deployment` + `Service` (+
`PVC` for Postgres) manifests, not subcharts, not assumed to pre-exist in
the cluster — so hand-rolling Authentik's manifests too, mirroring that
established posture, is the consistent choice, not a new one. Confirmed
useful from the official chart even though it isn't depended on: it
configures Authentik's own blueprint loading via a `blueprints.configMaps`
/ `blueprints.secrets` list ("Only keys in the configMap ending with
`.yaml` will be discovered and applied") — i.e. even Authentik's own
maintainers mount blueprints from a `ConfigMap` under Kubernetes, which
corroborates (rather than assumes) that the compose bind-mount approach's
Kubernetes analogue really is "a `ConfigMap`, mounted as a volume, into
the same `/blueprints/<dir>` path Authentik already scans" — no different
mechanism needed, just a different delivery vehicle for the same files.

**The browser-vs-container discovery problem is worse under Kubernetes,
confirmed by a real report of the same bug class.**
`goauthentik/authentik#3405` (fetched 2026-08-29) is exactly this: an
operator running Authentik under Kubernetes found its
`/.well-known/openid-configuration` echoing the Kubernetes-internal Host
header (`authentik.default.svc.cluster.local`) into `authorization_endpoint`
et al., breaking every external client redirected there — the reporter's
proposed fix (a config-level external-vs-internal URL split) was not what
shipped; the practical mitigation is entirely on the deployer's side:
make sure the Host header Authentik receives is *always* the
externally-routable one, from every vantage point that talks to it. Under
`docker compose`, one Docker network with one alias made that trivial (see
above). Under Kubernetes there is no single network both the developer's
browser and the `api` Pod are on — the in-cluster Service DNS name
(`nr-status-devauthentik.nr-status.svc.cluster.local`) and whatever
hostname reaches the developer's browser are structurally different
strings, so naively pointing `SSO_ISSUER_URL` at the in-cluster DNS name
reproduces #3405 exactly; pointing it at the external hostname makes the
in-cluster `api` Pod's own discovery fetch fail to resolve or route.

The resolution mechanism that actually closes this gap without touching
cluster-wide config (CoreDNS, an `ExternalName` Service, or app code in
`crates/api`) is a Pod-level `hostAliases` entry — confirmed as
Kubernetes' documented, purpose-built mechanism for exactly "make this one
Pod resolve hostname `X` to a specific IP without changing DNS for anyone
else" (`kubernetes.io/docs/concepts/services-networking/dns-pod-service/`,
and a Kubernetes discussion-forum thread, fetched 2026-08-29, describing
this identical need — services behind a shared ingress hostname that also
need to reach each other using that same external hostname internally,
solved by adding a `hostAliases` entry per Pod). Design below applies this
directly: the `api` Deployment gets a `hostAliases` entry mapping the
shared dev hostname straight to the Authentik Service's `ClusterIP`, so
`api`'s own discovery request presents the identical Host header string
the browser uses, without ever going through an Ingress itself.

**The developer's browser reaching the cluster at all, on a fixed,
predictable hostname:port, is a local-cluster-provisioning concern, not
something the chart can fully own.** Research confirms the standard,
widely-documented pattern for kind is an `extraPortMappings` cluster
config binding container ports (conventionally 80/443, but the mechanism
is not port-specific) on the kind node to the same port on the host's
loopback interface — the well-known "kind + ingress-nginx on localhost"
recipe. minikube's `tunnel` command and k3d's built-in load-balancer
`--port` flag are the direct equivalents for those two tools. All three
converge on the same shape: *some* host port gets forwarded to loopback,
at a port number the cluster-creation step chooses. That step happens
before `helm install` and is entirely outside a Helm chart's reach — a
chart can render a `Service`/`Ingress` that *expects* to be reached this
way, and document the port it needs forwarded, but it cannot itself bind
a port on the developer's physical host. This is the concrete way the
Kubernetes path is harder than compose's `ports: ["9000:9000"]`, which
*is* fully inside one `docker-compose.yml`.

**The `.localhost` loopback trick the compose design uses for the browser
side still works unmodified for Kubernetes' browser side, confirmed
separately from Chrome's own `Secure`-cookie behaviour, which matters
because `crates/api/src/auth.rs:81` sets `Secure` unconditionally on the
session cookie.** Chromium's own design discussion (`blink-dev`, fetched
2026-08-29) confirms Chrome treats `localhost` and everything under the
`.localhost` TLD as a secure context / "potentially trustworthy origin" —
the same exception the compose design already leans on implicitly for
`http://localhost:3000` — **conditioned on the name actually resolving to
a loopback address**. That condition holds for the kind/minikube/k3d
recipes above (they forward straight to `127.0.0.1`), so a developer
reaching the frontend and Authentik both under `*.localhost` hostnames
gets a working `Secure` cookie over plain HTTP with no TLS termination
needed anywhere in the dev cluster. It does **not** hold for a remote or
shared dev cluster reached over a real network (a non-loopback ingress
IP, a genuinely separate hostname) — that case needs real TLS and is out
of scope here, consistent with the Non-goals above.

Sources: [goauthentik/helm](https://github.com/goauthentik/helm) and
[`charts/authentik/Chart.yaml`](https://github.com/goauthentik/helm/blob/main/charts/authentik/Chart.yaml),
[`charts/authentik/values.yaml`](https://raw.githubusercontent.com/goauthentik/helm/main/charts/authentik/values.yaml)
(fetched 2026-08-29); [goauthentik/authentik#3405](https://github.com/goauthentik/authentik/issues/3405)
(fetched 2026-08-29); [Kubernetes: DNS for Services and Pods](https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/)
and a Kubernetes discuss-forum thread on cross-service hostname resolution
behind a shared ingress hostname (fetched 2026-08-29); the kind project's
documented `extraPortMappings` + ingress-nginx-on-localhost pattern
(multiple corroborating tutorials, fetched 2026-08-29); Chromium
`blink-dev` "Intent to Implement and Ship: Treat `http://localhost` as a
secure context" design discussion (fetched 2026-08-29).

## Design

### Deployment shape: a third, purely opt-in compose file

This repo's compose wiring (`docker-compose.yml:1-49`,
`docker-compose.dev.yml:1-24`) is entirely driven by `COMPOSE_FILE` inside
whichever env file is passed to `--env-file`, colon-separated, with no
Compose profiles involved. That mechanism extends cleanly to a third file
with zero changes to the existing two:

- New file, `docker-compose.authentik.yml`, defining the local IdP
  services (below). Never referenced by default.
- `dev.env.example`'s `COMPOSE_FILE` line stays exactly
  `docker-compose.yml:docker-compose.dev.yml`. A comment next to it (and
  next to the existing `SSO_*` block at `dev.env.example:104-124`)
  documents the opt-in: append `:docker-compose.authentik.yml` to
  `COMPOSE_FILE`, and replace the four placeholder `SSO_*` values with the
  fixed local-dev ones the new file's header comment documents (the
  deterministic `client_id`/`client_secret`/issuer from the blueprint
  above).
- `local.env.example` is untouched altogether — it stays "production-style
  run," and its own comment already says "point at a real or locally-run
  OIDC-compliant SSO server"; nothing stops a developer manually wiring
  the same opt-in there, but it isn't the natural home for a dev
  convenience and this design doesn't propose changing it.

This is the concrete answer to requirement 5 (external-IdP opt-out): **the
opt-out is the default.** A developer who already has a real IdP changes
nothing — doesn't add the third compose file, doesn't touch the `SSO_*`
placeholders beyond pointing them at their real server, exactly as today.
The local-IdP machinery (extra image, extra containers, extra volume) is
never pulled, started, or referenced unless `COMPOSE_FILE` explicitly names
it.

### Services

```yaml
# docker-compose.authentik.yml — sketch, not final
services:
  authentik-postgres:
    image: postgres:16-alpine        # matches this app's own postgres:16
    environment:
      POSTGRES_USER: authentik
      POSTGRES_PASSWORD: ${AUTHENTIK_PG_PASSWORD:-changeme-authentik-local-dev-only}
      POSTGRES_DB: authentik
    volumes:
      - authentik_postgres_data:/var/lib/postgresql/data
    healthcheck: ...                 # pg_isready, mirrors postgres's own

  authentik-server:
    image: ghcr.io/goauthentik/server:2026.8.0
    command: server
    depends_on:
      authentik-postgres:
        condition: service_healthy
    environment:
      AUTHENTIK_SECRET_KEY: ${AUTHENTIK_SECRET_KEY:?AUTHENTIK_SECRET_KEY must be set}
      AUTHENTIK_POSTGRESQL__HOST: authentik-postgres
      AUTHENTIK_POSTGRESQL__USER: authentik
      AUTHENTIK_POSTGRESQL__PASSWORD: ${AUTHENTIK_PG_PASSWORD:-changeme-authentik-local-dev-only}
      AUTHENTIK_POSTGRESQL__NAME: authentik
      # Deliberately NOT set: AUTHENTIK_BOOTSTRAP_PASSWORD/_EMAIL/_TOKEN.
    volumes:
      - ./authentik-blueprints:/blueprints/local:ro
    ports:
      - "9000:9000"
    networks:
      default:
        aliases: [authentik.localhost]

  authentik-worker:
    image: ghcr.io/goauthentik/server:2026.8.0
    command: worker
    depends_on:
      authentik-postgres:
        condition: service_healthy
    environment: *authentik-worker-env   # same as server
    volumes:
      - ./authentik-blueprints:/blueprints/local:ro
      # No /var/run/docker.sock mount — the official reference compose
      # mounts it for outpost/container management, which this app never
      # uses (no proxy/forward-auth outposts, just plain OIDC). Omitting
      # it is a deliberate reduction of both footprint and the container's
      # access to the host Docker socket.

volumes:
  authentik_postgres_data:
```

`api` in `docker-compose.yml` needs no changes to its own definition — it
already reads `SSO_*` purely from the environment
(`docker-compose.yml:103-107`); which values land there is entirely a
function of which env file the developer filled in, per the opt-in above.

### Data services: dedicated Postgres, no Redis

**Recommendation: Authentik gets its own dedicated Postgres
(`authentik-postgres` above), not this app's existing `postgres` service —
even as a second database on the same server. No Redis at all, for either
option.**

Reasoning:

- Redis is moot: current Authentik (2026.8.0) doesn't use it (see
  Research above) — there is no shared-vs-separate Redis decision left to
  make. This app's `redis` service (`docker-compose.yml:68-76`) stays
  untouched and unreferenced by Authentik.
- Postgres, reusing this app's server with `POSTGRES_DB: authentik` as a
  second database, was seriously considered — the Helm chart design
  already reasons about `DATABASE_URL` as a swappable, single-tenant
  concern per service (`docs/superpowers/specs/2026-08-18-helm-chart-design.md:144-188`),
  so "one Postgres server, several databases" isn't unprecedented in how
  this app thinks about its infrastructure. But two concrete things push
  the other way for *this* case specifically:
  - **Independent, coupled migration lifecycles.** This app's own
    Postgres is migrated by `sqlx::migrate!` at `api`/`aggregator`
    startup, which "takes a Postgres advisory lock"
    (`docs/superpowers/specs/2026-08-18-helm-chart-design.md:271`).
    Authentik migrates itself via Django migrations, tied to its own
    versioned image and its own documented "upgrade sequentially, don't
    skip versions" policy. Neither project's migration tooling knows or
    cares about the other; sharing one Postgres *server* (even with
    separate databases, which don't share advisory-lock namespaces) adds
    a shared blast radius — one server outage or `docker compose down -v`
    now takes out both — for no benefit, since nothing in this app reads
    Authentik's schema or vice versa.
  - **Reset-friendliness matters more than the resource saving, for a
    dev-only stack.** A developer wiping this app's own Postgres to test
    a fresh migration chain (`docker compose down -v` then `up`) shouldn't
    also nuke every locally-registered dev IdP user, and vice versa. One
    more small `postgres:16-alpine` container (idle footprint is modest —
    tens of MB) buys that independence outright; a shared server would
    require either accepting the coupling or building selective-volume
    tooling to avoid it, which is more effort than the container it's
    avoiding.

  This mirrors the general shape of the Helm chart's own bundled-vs-external
  reasoning for this app's Postgres (`docs/superpowers/specs/2026-08-18-helm-chart-design.md:15-42`) —
  bundle by default for a self-contained dev/eval experience, keep the
  door open to pointing at something external — just applied one level
  further out, to infrastructure Authentik itself owns rather than to
  this app's own database.

### Bootstrap / admin account

Per Research above: `AUTHENTIK_BOOTSTRAP_PASSWORD`/`_EMAIL`/`_TOKEN` stay
**unset** in `docker-compose.authentik.yml` — no default value, no
placeholder, nothing. This is deliberately the "genuinely no default
admin" option from the brief's two choices, not the "clearly-fake
placeholder" one, justified specifically by the blueprint-driven
provisioning covering this app's actual need (a working login flow)
without any admin account being load-bearing.

If a developer does want into Authentik's own admin UI:

- First option, Authentik's own interactive first-run flow at
  `/if/flow/initial-setup/` (`http://authentik.localhost:9000/if/flow/initial-setup/`).
- Documented, more reliable fallback if that flow is ever unreachable
  (Authentik's own troubleshooting docs, fetched 2026-08-29):
  `docker compose --env-file dev.env run --rm authentik-server create_recovery_key 10 akadmin`,
  which prints a one-time link that logs straight in as `akadmin`
  regardless of flow state.

### Persistence

One new named volume, `authentik_postgres_data`, mounted at
`/var/lib/postgresql/data` on `authentik-postgres` — the direct
counterpart of this app's own `postgres_data` (`docker-compose.yml:59-60,
281-282`). Authentik's users, sessions, blueprint-applied objects (the
provisioned OAuth2 provider/application, the enrollment flow, the
identification-stage wiring) all live in that database, so this one volume
is sufficient to satisfy "survives `docker compose down`/`up`." The
official reference compose additionally bind-mounts `./data` and
`./custom-templates` for on-disk media/template overrides — not proposed
here; this deployment has no custom branding/templates and no user-uploaded
media that would need to survive a container recreate beyond what's
already in Postgres.

### Blueprint files

Two files under a new `authentik-blueprints/` directory at the repo root,
bind-mounted read-only into both `authentik-server` and `authentik-worker`
at `/blueprints/local` (both processes apply blueprints; mounting into
both matches the official reference compose's `./data`/`./custom-templates`
mounts appearing on both services):

- `authentik-blueprints/oauth2-client.yaml` — the OAuth2 Provider +
  Application sketch from Research above, with fixed `client_id`/
  `client_secret`/`redirect_uris`/`slug`.
- `authentik-blueprints/open-signup.yaml` — the adapted
  `flows-enrollment-2-stage.yaml` plus the `IdentificationStage` update
  wiring it in as `enrollment_flow`.

Both are inert on a real external IdP (they're never mounted unless
`docker-compose.authentik.yml` is in `COMPOSE_FILE`), require no manual
UI step, and (per Research) can be edited and picked up by the file-watch
without a container restart during iteration on this design's eventual
implementation.

### Helm chart support (opt-in, alongside docker compose)

A second, independent path into the same local Authentik IdP, for a
developer running `charts/nr-status` against a local/dev Kubernetes
cluster instead of `docker compose`. Same posture as the compose path:
strictly opt-in, off by default, zero effect on an install that points at
a real external IdP.

#### `values.yaml` shape

One new top-level block, `devAuthentik`, following the same
enabled-flag-plus-block convention `postgresql` and `redis` already use in
this chart:

```yaml
devAuthentik:
  enabled: false            # opt-in; default false, matches the brief
  hostname: authentik.localhost   # the ONE string both the browser and
                                   # the api Pod must resolve identically
  image:
    repository: ghcr.io/goauthentik/server
    tag: "2026.8.0"          # pinned, same rationale as the compose path
  secretKey: ""              # AUTHENTIK_SECRET_KEY; randAlphaNum-generated
                             # via the chart's existing lookup-preserve
                             # pattern (templates/secret.yaml) when empty
  service:
    port: 30900              # ClusterIP-facing port AND NodePort — see
    nodePort: 30900          # "Discovery reachability" below for why
                             # these two are deliberately the same number
  hostAliasIP: ""            # optional explicit override for the
                             # hostAliases IP (see below); empty = use
                             # `lookup` against the live Service
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

When `devAuthentik.enabled` is `true` **and** `api.sso.issuerUrl` (and the
other four `api.sso.*` values) are left at their empty default, the chart
computes them from `devAuthentik.hostname`/`service.port` and the fixed
blueprint-provisioned `client_id`/`client_secret` (identical values to the
compose blueprint in Design → Blueprint files above, so both paths are
interchangeable in a developer's head). This is exactly what closes the
gap flagged in the Helm chart's own README today: `api.sso.*` are
currently "required and the render aborts if any is missing" with **no
dev-friendly default at all** — unlike compose, which has the
`sso.example.invalid` placeholder to fall back on. `devAuthentik` becomes
that placeholder's Kubernetes equivalent, except it's a *working* default
rather than an inert one. An operator who explicitly sets any `api.sso.*`
value keeps it — `devAuthentik`, even if enabled, never overrides an
explicit value, so the two mechanisms compose sensibly rather than
conflicting (a developer could in principle run the bundled IdP against a
manually-set issuer URL, though that's an unlikely combination and not
one this design needs to guard against beyond "explicit wins").

#### Manifests (sketch — not final YAML, no template files written for this design)

All gated behind `devAuthentik.enabled`, in a new `devauthentik-*.yaml`
template group, following the existing chart's per-component file
naming:

- **`devauthentik-secret.yaml`** — one `Secret` holding
  `AUTHENTIK_SECRET_KEY` (chart-generated via the same
  `lookup`-then-`randAlphaNum` pattern `templates/secret.yaml` already
  uses for `postgres-password`/`internal-token`, so it survives `helm
  upgrade`) and the Authentik-Postgres password.
- **`devauthentik-blueprints-configmap.yaml`** — one `ConfigMap` whose
  data keys are the two blueprint files from Design → Blueprint files
  above (`oauth2-client.yaml`, `open-signup.yaml`), sourced via `.Files.Get`
  from a `files/devauthentik-blueprints/` directory shipped in the chart.
  Mounted as a volume at `/blueprints/local` on both the server and worker
  Deployments below — the direct Kubernetes analogue of the compose
  path's bind mount at the same container path, per Research above
  (Authentik's own official chart uses the identical
  ConfigMap-of-`*.yaml`-keys mechanism, corroborating this is the right
  shape rather than an invented one).
- **`devauthentik-postgres-statefulset.yaml`** + **`-service.yaml`** — a
  dedicated `StatefulSet` + headless `Service` + `volumeClaimTemplates`
  entry, mirroring this chart's *own* bundled Postgres exactly (same
  `PGDATA` subdirectory fix, same explicit `runAsUser`/`runAsGroup`/
  `fsGroup: 999`, same `pg_isready` probes —
  `docs/superpowers/specs/2026-08-18-helm-chart-design.md`'s PostgreSQL
  section applies verbatim, just instantiated a second time under a
  `devauthentik-` prefix). Kept dedicated rather than a second database
  on the app's own bundled Postgres, for the identical reasons Research →
  Data services already gives for the compose path (independent migration
  lifecycles, reset-friendliness) — nothing about Kubernetes changes that
  reasoning, so it isn't repeated with new justification, just re-applied.
- **`devauthentik-server-deployment.yaml`** + **`-worker-deployment.yaml`**
  — `Deployment`s (not `StatefulSet`s — Authentik's own state lives in its
  Postgres, not on the server/worker Pods themselves, so these are
  ordinary stateless workloads), `command: server` / `command: worker`
  respectively on the same image, `AUTHENTIK_BOOTSTRAP_*` left unset for
  the same "no default admin, blueprint-driven provisioning instead"
  reasoning as compose. No `/var/run/docker.sock` mount on the worker —
  same reasoning as compose (no outposts used), and doubly appropriate
  under Kubernetes, where mounting the host's container socket into a Pod
  is a materially worse privilege-escalation surface than under a
  developer's own local Docker daemon.
- **`devauthentik-service.yaml`** — a single `Service` in front of
  `devauthentik-server` (the worker exposes no HTTP surface, so it gets
  none), `type: NodePort`, with `port` and `nodePort` **both** set to
  `devAuthentik.service.port`/`nodePort` (default `30900`, i.e. the same
  literal number for both fields) and `targetPort: 9000` (Authentik's own
  listen port). See "Discovery reachability" below for why the port and
  nodePort must be identical.
- Reuses the existing `secret.yaml` machinery's pattern rather than a new
  one, and reuses the chart's existing security-context and
  `readOnlyRootFilesystem` conventions for the server/worker Deployments
  (Authentik's image runs as non-root already, matching this chart's
  Rust-workload posture — no `postgres`-style uid pinning needed there,
  only on `devauthentik-postgres`).

No PVC beyond the one on `devauthentik-postgres` — the server/worker Pods
themselves are stateless, matching compose's own "one named volume is
sufficient" conclusion (Design → Persistence above).

#### Blueprint delivery under Kubernetes

Confirmed in Research above rather than assumed: Authentik discovers
blueprints by scanning a mountable directory for `*.yaml` files, on a
periodic-plus-file-watch schedule, and its own official Helm chart already
delivers them the same way this design proposes — a `ConfigMap` whose keys
are the blueprint filenames, mounted as a volume onto the pod at (in this
design) `/blueprints/local`, exactly mirroring the compose path's
bind-mounted directory of the same name and content. The two blueprint
files themselves (`oauth2-client.yaml`, `open-signup.yaml`) are **byte-for-byte
the same content on both deployment paths** — the chart's `ConfigMap` is
built from the same source files the compose bind-mount uses, so there is
exactly one blueprint definition to maintain, not two. A `ConfigMap` (not
a `Secret`) is the right choice here specifically because nothing in
these two blueprints is sensitive: the fixed `client_id`/`client_secret`
are already-documented, known-in-advance dev-only values (the same ones
in the compose design's blueprint sketch above), not real secrets — the
`AUTHENTIK_SECRET_KEY` and the Authentik-Postgres password, which *are*
sensitive, live in `devauthentik-secret.yaml` instead, per the chart's
existing convention of routing anything sensitive through a `Secret`.

#### Discovery reachability under Kubernetes

This is the harder version of the same problem the compose design already
flags as an open risk (Research → the browser-vs-container discovery
reachability problem, and Research → Helm chart deployment above), and it
needs a genuinely different mechanism because Kubernetes gives the
browser and the `api` Pod no shared network the way one Docker Compose
network does. The design resolves it with three pieces that must all use
the *same* port number:

1. **A `NodePort` Service with `port == nodePort`.** As above,
   `devauthentik-service.yaml` sets both fields to
   `devAuthentik.service.port`/`nodePort` (default `30900` — inside
   Kubernetes' default `30000-32767` NodePort range, since `nodePort`
   cannot be an arbitrary value like the compose path's `9000` without a
   nonstandard `--service-node-port-range` on the apiserver). Because the
   Service's ClusterIP-facing `port` and its `nodePort` are the same
   number, both an in-cluster caller and a caller on the node's own
   published port reach Authentik on an identical port — there is no
   analogue of "the internal port differs from the externally-published
   one" to trip over.
2. **The developer's local cluster forwards that NodePort to their
   machine's loopback interface**, using whichever mechanism their
   cluster tool provides — kind's `extraPortMappings`, `minikube tunnel`,
   or k3d's `--port` — at the *same* port number
   (`devAuthentik.service.nodePort`, `30900` by default). This step lives
   in the developer's cluster-creation config, not in the chart (see
   Research above for why); the chart's `NOTES.txt` output, when
   `devAuthentik.enabled` is true, documents the exact port to forward and
   a worked kind config snippet, so it's a copy-pasteable one-time step
   rather than a mystery. The developer's browser then reaches
   `http://authentik.localhost:30900/...`, which resolves to `127.0.0.1`
   per the RFC 6761 `.localhost` handling confirmed in Research above, and
   lands on the forwarded port.
3. **The `api` Deployment gets a `hostAliases` entry** mapping
   `devAuthentik.hostname` (`authentik.localhost`) to the
   `devauthentik-service` Service's `ClusterIP` — *not* its NodePort, its
   ordinary in-cluster ClusterIP, reached at the same `port` number from
   inside the cluster. This makes `api`'s own outbound discovery request
   (`crates/api/src/auth/oidc.rs:129-137`) present the Host header
   `authentik.localhost:30900` — identical to what the browser sends —
   without the request ever leaving the cluster or passing through an
   Ingress. The ClusterIP is obtained the same way `templates/secret.yaml`
   already obtains a live cluster value: Helm's `lookup` function,
   reading `devauthentik-service`'s `spec.clusterIP` at render time (with
   `devAuthentik.hostAliasIP` as an explicit escape hatch for an operator
   who'd rather pin a literal IP than rely on `lookup` — useful on kind's
   commonly-default `10.96.0.0/12` service CIDR, where a fixed address is
   entirely predictable).

`SSO_ISSUER_URL` under this design is therefore
`http://authentik.localhost:30900/application/o/nr-status/` — same shape
as the compose path's `http://authentik.localhost:9000/...`, differing
only in port because of the NodePort range constraint above.

This is *proposed*, not verified end-to-end (mirroring the compose
design's own honesty about its unverified `.localhost`-alias trick) — see
Open questions / risks below for exactly what's still open here versus
what the compose path already flagged.

## Open questions / risks

### docker-compose path

- **First-boot ordering between this app's blueprint and Authentik's own
  defaults.** The OAuth2 Provider blueprint's `!Find` lookups (the
  self-signed certificate keypair, the `default-provider-authorization-
  implicit-consent` flow) depend on Authentik's *own* default blueprints
  having already applied. On a stone-cold first `docker compose up` this
  is plausibly a race, not a guarantee — blueprint application is
  eventual (periodic + file-watched), so a failed first pass should
  self-heal on the next 60-minute cycle or a restart, but "the deterministic
  client id/secret work immediately on the very first `up`" is not
  something this research confirmed hands-on. Worth validating against a
  real running instance before treating this as turnkey, and worth a
  documented "if it doesn't work the first time, restart
  `authentik-worker`" escape hatch either way.
- **The browser-vs-container discovery hostname fix is proposed, not
  verified.** The `*.localhost`-alias approach follows directly from how
  Authentik is documented to build its discovery URLs and how Compose
  network aliases and RFC 6761 `.localhost` resolution are documented to
  behave, but no container was actually started for this research pass —
  it should be smoke-tested (does `authentik.localhost` actually resolve
  to loopback in whatever browsers/OSes this team actually uses; does
  Authentik's `ALLOWED_HOSTS`-equivalent accept an arbitrary `*.localhost`
  Host header without extra config) before this is trusted as the final
  answer. A manual `/etc/hosts` entry is the documented fallback if
  automatic `.localhost` resolution doesn't hold on some platform.
- **Resource footprint.** Authentik is a substantial Django application:
  server + worker + its own Postgres is three more containers on top of
  an already 10-service dev stack (`postgres`, `redis`, `api`, four
  pollers, `poller-tfl`, `aggregator`, `enricher`, `frontend`). That's a
  real cost for a dev convenience whose entire job is "let me click
  through a login form." Proportionate for a team that expects to
  exercise the login flow repeatedly; less obviously so for an occasional
  check, where registering a free-tier external test IdP (an Auth0/
  Keycloak-cloud dev tenant) and just filling in the existing `SSO_*`
  placeholders might be less overhead than three extra containers. This
  design doesn't resolve that trade-off — it just makes the local option
  available and strictly opt-in, per requirement 5.
- **Version-pinning risk.** `2026.8.0` is current as of this research
  (2026-08-29) but Authentik's own upgrade docs require sequential
  version upgrades (don't skip a release family), and its 3-month cadence
  plus 2-version support window means this pin has a real, finite shelf
  life before it needs bumping — more churn-prone than e.g. `postgres:16`
  or `redis:7`, which this repo pins to a major version only.
- **No SMTP means no email verification or password recovery for local
  Authentik accounts.** Accepted for a throwaway dev IdP (matches the
  "frictionless local dev/testing" framing of requirement 3) but worth
  stating as a deliberate limitation rather than a gap discovered later —
  a developer who locks themselves out of a local dev account should just
  re-register (`always_create` in the enrollment flow's `userwritestage`
  will not stop them reusing a username tied to a lost password, since
  there is no recovery path being asked for here at all — they'd need a
  new username, or a manual DB/UI intervention).
- **`AUTHENTIK_BOOTSTRAP_*` known flakiness, moot here but worth recording.**
  A GitHub issue (`goauthentik/authentik#7546`, referenced during
  research) reports bootstrap env vars sometimes not applying when a
  blueprints directory is present at first boot. This design avoids the
  bootstrap vars entirely (see Bootstrap above), so the bug doesn't apply
  to it directly — noted here so a future implementer doesn't rediscover
  it by trying to "simplify" toward bootstrap vars later without knowing
  why this design steered away from them.

### Helm path

Deliberately not a copy of the list above — several compose-path risks
don't apply under Helm (there's no single-writer `COMPOSE_FILE` opt-in
mechanism to get wrong, and blueprint delivery via a chart-shipped
`ConfigMap` has no bind-mount-path-typo failure mode a developer could hit
locally), and a few are meaningfully worse. What's genuinely open for the
Helm path specifically:

- **`hostAliases` + `lookup` has a first-install ordering gap, same shape
  as the chart's existing Secret-generation caveat but with no fallback.**
  `templates/secret.yaml`'s `lookup`-preserve pattern degrades gracefully
  on a first install (an empty `lookup` just means "generate a fresh
  value," which is correct there). `devauthentik-server`'s `hostAliases`
  entry has no equally graceful degradation: on a from-scratch
  `helm install` with `devAuthentik.enabled: true`, `devauthentik-service`
  doesn't exist yet when the `api` Deployment is rendered in the *same*
  release, so `lookup` returns nothing and no `hostAliases` entry can be
  written at all — not a wrong one, an absent one, which breaks discovery
  until the very first `helm upgrade` re-renders `api` after the Service
  exists. This needs either `devAuthentik.hostAliasIP` set explicitly on
  first install (already provided as an escape hatch above, but that's an
  extra manual step the compose path doesn't have) or a documented
  "`helm upgrade` once, immediately after first install" instruction in
  `NOTES.txt`. Not resolved by this design — flagged as the single
  biggest gap between "designed" and "known to work."
- **The local-cluster port-forwarding step is a real external
  prerequisite this design cannot enforce or verify from inside the
  chart.** Unlike compose's `ports: ["9000:9000"]`, which is guaranteed
  to exist the moment `docker-compose.authentik.yml` is in `COMPOSE_FILE`,
  nothing stops a developer enabling `devAuthentik` on a kind/minikube/k3d
  cluster that was never configured to forward `30900` to loopback — the
  chart would install cleanly and every symptom (browser can't reach
  Authentik, or reaches a different service on a port collision) would
  show up as a runtime failure with no render-time signal. `NOTES.txt`
  can document the required forwarding step; it cannot check whether it
  was actually done. This is the harder-under-Kubernetes point flagged in
  Research above, made concrete.
- **None of this was smoke-tested against a real kind/minikube/k3d
  cluster.** Same honesty as the compose design's own unverified
  `.localhost`-alias claim: the `hostAliases`-plus-matching-NodePort
  mechanism follows directly from documented Kubernetes behaviour and a
  real precedent (`goauthentik/authentik#3405` and the Kubernetes
  discuss-forum thread cited in Research above), but no cluster was
  actually stood up for this research pass. Before this is trusted as
  final: confirm `hostAliases`' `/etc/hosts` entry is genuinely honoured
  by whatever HTTP client `crates/api`'s `openidconnect`/`oauth2` stack
  uses for its discovery fetch (Rust's usual HTTP stacks respect
  `/etc/hosts` via the OS resolver, but this hasn't been checked against
  this specific dependency chain), and confirm the chosen NodePort doesn't
  collide with anything else a kind/minikube/k3d default setup already
  publishes.
- **Resource footprint is worse, not just repeated.** The compose path
  already flags three extra containers as a real cost; the Helm path adds
  a `StatefulSet`+`Service` (Postgres), two `Deployment`s (server,
  worker), a `ConfigMap`, and a `Secret` on top of an already-larger set
  of Kubernetes objects than compose has services, on a developer's local
  cluster that may have tighter resource limits than their host machine
  running compose directly (kind and minikube both run inside a
  container/VM with a bounded CPU/memory allocation, unlike compose,
  which runs directly on the host's own resources). Not a blocker, but a
  sharper version of the same trade-off the compose design already
  declines to resolve.
- **Version-pinning risk is identical, not worse.** `devAuthentik.image.tag`
  pins to the same `2026.8.0` on the same evidence and carries the same
  finite shelf life as the compose path's pin — no Helm-specific wrinkle
  here, noted only to confirm it was considered rather than overlooked.
- **No SMTP / no email verification or recovery: identical to compose,
  not worse.** The blueprint-driven open-signup flow and its "just
  re-register" limitation apply unchanged under Kubernetes; nothing about
  running Authentik as Kubernetes Deployments instead of Compose services
  changes Authentik's own SMTP requirements.
