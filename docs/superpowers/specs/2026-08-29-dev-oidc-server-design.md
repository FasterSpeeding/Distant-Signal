# Local Dev OIDC Server (Authentik) — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md` and
`docs/superpowers/specs/2026-08-18-helm-chart-design.md`) so it can be
reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. It does **not**
contain a task-by-task implementation plan or any `docker-compose`/YAML
changes — that is a separate, later step in this repo's process, done only
after a design like this has been reviewed.

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

## Non-goals

- Production Authentik deployment, hardening, TLS, SMTP/email, or the Helm
  chart (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`). That
  chart's non-goals already exclude anything beyond this app's own
  workloads plus (optionally) bundled Postgres — an IdP is a further step
  out from that boundary and stays there. This design is dev-`docker
  compose`-only.
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

## Open questions / risks

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
