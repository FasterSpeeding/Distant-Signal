# Design: Internal Service Accounts for `/private/*`

**Status: design proposal, not approved.** No implementation plan is
included; that is a separate, later step in this repo's process, per this
repo's own established convention (see, e.g., every design under
`docs/superpowers/specs/`, none of which contain a task list).

## Goal

Every internal microservice that pushes ingested data into this app —
`poller-incidents`, `poller-stations`, `poller-tocs`, `poller-ldbws`,
`poller-tfl`, `trust-consumer`, and `schedule-ingest` — authenticates to the
`api` crate's `/private/*` routes with the exact same shared-secret value
today: one `X-Internal-Token` header value, generated once by the Helm chart
(or set once in `dev.env`/`local.env`), copied unmodified into every one of
those services' containers. The check on the `api` side is binary: "is this
the one correct token," not "is this token allowed to call *this* route."
Any service holding that one value can call any `/private/*` route,
including routes that are not that service's job — `poller-tfl` could POST
to `/private/schedule-feed-ingests`, `poller-incidents` could POST directly
into `/private/tfl-line-status`, and so on, with nothing in this codebase
stopping either.

This spec redesigns that around **per-service accounts**: each internal
caller gets its own distinct credential, and the `api` crate is extended to
know not just "is this a valid internal caller" but "is *this* caller
allowed to reach *this* route." It covers the credential shape, the
route-scoping mechanism, the Helm chart and local-dev provisioning changes,
and rollout — but is design only, not code, not a migration, not a chart
edit.

## Current relevant state (verified this session)

### The shared-secret mechanism

`crates/api/src/auth.rs`'s own module doc (lines 1–6) states the mechanism
plainly: *"One shared-secret header (`X-Internal-Token`), compared in fixed
time against `ServiceArguments::internal_token`. This is intentionally not a
general auth framework — just enough to keep the ingestion endpoints from
being reachable by anyone who can hit the API's port."* The middleware
(`require_internal_token`, `crates/api/src/auth.rs:20–36`) pulls the header,
runs it through a hand-rolled `constant_time_eq` (`auth.rs:38–56`) against
**one single configured string**, `app.config.internal_token`, and returns
`401 UNAUTHORIZED` on any mismatch — length mismatch included. There is no
concept of *which* caller presented the token; a correct value is
indistinguishable from any other correct value, because there is only ever
one.

`crates/api/src/routes/mod.rs::private_router()` (lines 58–75) applies this
middleware via `.layer(middleware::from_fn_with_state(app,
require_internal_token))` to the **entire merged router** — every route
merged into it gets the identical gate, with no per-route variation. Its own
comment (line 53–57) explains why it takes `App` directly rather than
picking up state later: `from_fn` fixes the handler's state to `()`, so a
stateful check needs `from_fn_with_state` up front. This matters for the
design below, since any new per-route scoping has to happen inside that same
constructor, with the same state-availability constraint.

`crates/api/src/app.rs:65–72` — `AppState::init()` — refuses to start at all
if `internal_token` is empty, with the comment: *"An empty token would make
`auth::constant_time_eq` compare two empty byte slices and accept any
request with no `X-Internal-Token` header at all — reject that at startup
rather than silently running an unauthenticated `private_router()`."* Any
new per-service token scheme needs an equivalent startup guard, or it
inherits this same failure mode per service instead of once globally.

### The full `/private/*` route surface

Both route-module files merged into `private_router()`
(`crates/api/src/routes/mod.rs:60–61`) were read in full. Every internal
endpoint, and its one legitimate caller as confirmed by matching each
poller's/service's own `config.rs` default `*_url` value against the route:

| Route | Methods | Handler(s) | Legitimate caller |
|---|---|---|---|
| `/private/incidents` | GET, POST | `ingest.rs:60–67`, `:101–109` | `poller-incidents` |
| `/private/stations` | GET, POST | `ingest.rs:69–76`, `:111–119` | `poller-stations` |
| `/private/tocs` | GET, POST | `ingest.rs:78–81`, `:131–139` | `poller-tocs` |
| `/private/station-samples` | GET, POST | `ingest.rs:83–90`, `:121–129` | `poller-ldbws` |
| `/private/tfl-line-status` | GET, POST | `ingest.rs:92–99`, `:148–156` | `poller-tfl` |
| `/private/train-events` | POST | `ingest.rs:160–170` | `trust-consumer` |
| `/private/tracked-trains` | GET | `ingest.rs:176–183` | `trust-consumer` |
| `/private/schedule-feed-ingests` | GET, POST | `ingest.rs:206–213`, `:215–224` | `schedule-ingest` |
| `/private/sample-stations` | GET | `samples.rs:20–25` | `poller-ldbws` |

(`ingest.rs`'s `router()`, lines 28–53, and `samples.rs`'s `router()`, line
17, are the two `.merge(...)` calls in `private_router()`.) The caller for
each row was cross-checked against that service's own `config.rs` default
URL, not guessed: e.g. `crates/poller-ldbws/src/config.rs:33–40` defaults
`api_sample_stations_url` to `.../private/sample-stations` and
`api_ingest_url` to `.../private/station-samples`; `crates/trust-consumer/
src/config.rs:56–61` defaults `api_ingest_url` to `.../private/train-events`
and `api_tracked_trains_url` to `.../private/tracked-trains`;
`crates/schedule-ingest/src/config.rs:50–51` defaults `api_ingest_url` to
`.../private/schedule-feed-ingests`.

**No route today is legitimately called by more than one service.** Every
row above maps to exactly one caller. `poller-ldbws` is the only service
with more than one legitimate route (`/sample-stations` read plus
`/station-samples` read+write) — a detail that matters for Decision 2
below, since it means "one credential → one route" is too narrow a
primitive, but "one credential → one flat allowlist of routes" already
covers every real case in this codebase today.

### How each poller authenticates today

Confirmed by reading `crates/common/src/ingest.rs` in full (the shared
contract module — its own doc comment, lines 1–13, names every consumer:
the four RDM pollers, `poller-tfl`, and `api`'s ingest routes) plus two
pollers' `config.rs` directly:

- `INTERNAL_TOKEN_HEADER` is a single constant, `"x-internal-token"`
  (`crates/common/src/ingest.rs:22`).
- `post_batch` (`ingest.rs:35–57`) and `fetch_last_fetched` (`ingest.rs:99–
  112`) both attach it via `.header(INTERNAL_TOKEN_HEADER, internal_token)`
  — a single `&str` parameter, passed straight through from each poller's
  own `Config::internal_token` field.
- `crates/poller-ldbws/src/config.rs:42–45` and `crates/poller-stations/
  src/config.rs:26–29` both declare `internal_token: String` via
  `#[arg(long, env)]` (clap, so also settable as `INTERNAL_TOKEN`), each
  with an identical doc comment pointing at `crates/api/src/auth.rs`.
  `grep -rl "internal_token"` confirms the same field exists in
  `poller-incidents`, `poller-tocs`, `poller-tfl`, `trust-consumer`, and
  `schedule-ingest`'s `config.rs`/`main.rs` too — seven services in total,
  every one of them reading the identical env var name into an identical
  field shape.

No poller does anything more sophisticated than "read one string from env,
attach it to one header on every outbound request." There is no client
certificate, no JWT, no per-request signing — this app already treats
internal auth exactly the way it treats a plain API key.

### How the shared secret is provisioned today

`charts/distant-signal/templates/secret.yaml:36–41`:

```
{{/* internal-token: always needed (the api asserts it is non-empty), so it
     is generated whenever no value and no existingSecret is supplied. */}}
{{- if not .Values.secrets.existingSecret -}}
{{- $token := .Values.secrets.internalToken | default (get $existingData "internal-token" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "internal-token" ($token | b64enc) -}}
{{- end -}}
```

— one key, `internal-token`, in the chart's single rendered `Secret`
(`distant-signal.secretName`), auto-generated via `randAlphaNum 32` if
`.Values.secrets.internalToken` is empty, and **preserved** across
`helm upgrade` via the file's own documented lookup-preserve pattern
(`secret.yaml:1–22`) so a redeploy doesn't rotate it out from under running
pods. `charts/distant-signal/values.yaml:36–44` is the matching values
block (`secrets.internalToken`, `secrets.existingSecret`,
`secrets.existingSecretInternalTokenKey`).

Every consumer resolves the *same* secret name/key through the *same* two
helper templates, `distant-signal.internalTokenSecretName` /
`distant-signal.internalTokenSecretKey` (`_helpers.tpl:185–195`, whose own
comment reads *"Resolved Secret name/key for the shared internal token.
Takes root. Used by api-deployment.yaml and by all four poller
deployments"* — now six consumers, not four, per the `grep` below):

- `charts/distant-signal/templates/poller-deployments.yaml:97–101` — one
  template rendering all five poller Deployments (incidents, ldbws,
  stations, tfl, tocs), each getting an `INTERNAL_TOKEN` env var via this
  same `secretKeyRef`.
- `charts/distant-signal/templates/api-deployment.yaml:105–109` — the `api`
  container itself, reading the value it will compare every request
  against.
- `charts/distant-signal/templates/trust-consumer-deployment.yaml:103–107`.
- `charts/distant-signal/templates/schedulefeed-deployment.yaml:207–211`
  (the `schedule-ingest` container).

`docker-compose.yml` mirrors this exactly for local dev: a single
`${INTERNAL_TOKEN}` interpolated into `api`'s environment (line 96) and into
all five pollers' plus `trust-consumer`'s plus `schedule-ingest`'s
environments (lines 146, 167, 191, 216, 241, 322, 480) — nine occurrences of
the same variable. `dev.env.example:117–121` sets it once:
`INTERNAL_TOKEN=changeme-shared-secret-local-dev-only`, with the comment
*"Shared secret every poller must present via the `X-Internal-Token` header
to reach the api's `/private/*` ingestion endpoints."*

### Prior art already in this chart for per-service named credentials

Two real precedents exist for "multiple distinct named secrets in one
chart," both from this session's own earlier work, both directly relevant
to Decision 4 below:

1. **`pollers.<name>.apiKey`** (`values.yaml:143–274`, `secret.yaml:54–62`):
   each of the five enabled pollers already gets its **own** named secret
   key, `rdm-<name>-api-key`, rendered only when that poller is enabled and
   has no `existingSecret` of its own (`pollerSecretName`/`pollerSecretKey`
   helpers, `_helpers.tpl:223–234`). This is the closest structural
   precedent for "one named secret key per poller" — but it authenticates
   *outbound*, to RDM/TfL, not inbound to this app's own `/private/*`
   routes, and per `secret.yaml:54–56`'s own comment, an RDM key is
   "deliberately NOT auto-generated: a random RDM key is meaningless" —
   the opposite auto-generation posture from `internal-token`.
2. **`railMcp`'s eight credentials** (`secret.yaml:83–98`,
   `railmcp-deployment.yaml:68–107`, one dedicated `_helpers.tpl` template
   per key): Discord OAuth plus six LDBWS product URL/key pairs, each its
   own named `Secret` key with its own dedicated `secretKeyRef`. Same
   observation as (1): these are the derived MCP service's *outbound*
   credentials to third parties, not an inbound service-identity scheme,
   but they are direct, real, in-repo proof that this chart already
   comfortably carries N independent named secret keys in one `Secret`
   object with N dedicated Deployment `secretKeyRef`s, rather than
   collapsing everything into one shared value.
3. **The closer precedent — `scheduleFeed.sftp.password`**
   (`schedulefeed-secret.yaml:28–59`, corrected 2026-09-01 the same
   session): *this app mints the credential* for an inbound caller (DTD's
   SFTP push client), rather than an external system assigning one to
   this app. The file's own corrected comment draws the exact distinction
   this design needs: *"`pollers.*.apiKey` authenticates THIS app against
   an external system that already assigned the key, so a randomly
   generated value would be meaningless — but this account is THIS app's
   own server minting credentials for a caller to use, exactly the same
   shape as the SSH host key above (and secret.yaml's postgres password /
   internal token): this app decides the value, then tells the caller what
   it is, not the other way around."* Per-service `/private/*` tokens are
   in exactly this category — `api` mints them, pollers are simply told
   what to present — so they auto-generate via the same
   `randAlphaNum`-and-preserve chain `internal-token` already uses today,
   not the "never auto-generate, meaningless if random" posture `apiKey`
   and `llm-api-key` use.

### This app's existing authorization vocabulary

`grep -n "role\|permission\|scope\|group" crates/api/src/data/users.rs`
returns nothing. This app's human-user auth (`AuthenticatedUser`,
`crates/api/src/auth.rs:176–198`, backed by `crates/api/src/data/
users.rs`) has no concept of roles or permission groups today — every
logged-in user is authorized identically, and every ownership check in this
codebase (custom lines, tracked trains, tickets) is scoped to "does this
row's `user_id` match the caller," never to a role. `crates/api/src/
routes/train.rs:1–11`'s module doc states the ownership convention plainly:
*"same 404-for-both-'missing'-and-'not-yours' convention as every other
ownership check in this app, never `403`"* — this is the convention
Decision 3 below has to reason against, and (spoiler) explicitly does not
reuse, for reasons given there.

**"Access groups" for internal services is a related but distinct concept
from a future human-roles feature**, and this design does not build a
shared primitive for both — see Decision 4.

### `schedule-reference` — not yet in this codebase

`find crates -maxdepth 1 -type d` lists `aggregator`, `api`, `common`,
`enricher`, `poller-incidents`, `poller-ldbws`, `poller-stations`,
`poller-tfl`, `poller-tocs`, `schedule-ingest`, `trust-consumer` — **no
`schedule-reference` crate exists in this worktree as of this session**.
The prompt for this spec anticipated it might have landed by now; it has
not. This design is written to accommodate it as a future addition (a new
row in the service table below, once its own `/private/*` route(s) exist)
but does not invent a concrete scope entry for it, since guessing its route
surface ahead of that crate's own design would be exactly the kind of
invented detail this spec is required to avoid.

### Local dev (`docker-compose.yml` / `dev.env.example`)

Already covered above under "How the shared secret is provisioned" —
`docker-compose.yml` interpolates the single `${INTERNAL_TOKEN}` into nine
service definitions, and `dev.env.example` sets it once. Notably,
`dev.env.example`'s header (lines 7–19) already documents an accepted
"duplication cost" convention for this file and `local.env.example`: *"The
same secrets therefore live in TWO places (`dev.env` and `local.env`), and
they WILL drift if you only update one of them... This is the accepted cost
of the two-file split."* Per-service tokens extend this exact same,
already-accepted duplication pattern — more named variables in the same two
files, not a new kind of complexity. Decision 5 below builds on this
directly.

## Decisions

### 1. Credential shape: keep the bearer-token header, make the *value* per-service and identity-bearing via lookup — not mTLS, not a JWT, not a separate identity header

Three real alternatives were weighed, against this codebase's demonstrated
taste (stated explicitly in `auth.rs`'s own doc comment: *"not a general
auth framework — just enough"*) for the simplest mechanism that solves the
real problem:

- **mTLS (client certificates).** **Rejected.** Would require this app to
  stand up a CA, issue and rotate seven-plus per-service certificates, and
  teach every poller's `reqwest::Client` to present one — a wholly new
  category of infrastructure (nothing in this codebase issues or verifies
  certificates anywhere today) for a threat model this app doesn't
  actually have: every one of these calls already happens inside the same
  Kubernetes namespace / docker-compose network, with no cross-network hop
  and (per `charts/distant-signal/templates/networkpolicy.yaml`, which
  already exists) network-level isolation as a second layer. mTLS's real
  advantage — mutual, cryptographically-verified identity without a
  shared secret in transit — buys little here and costs a lot.
- **A structured JWT with per-service claims.** **Rejected.** Would add a
  JWT library dependency and claim-verification logic for a set of
  callers that is small, fixed at deploy time, and never needs
  self-issued or short-lived tokens (nothing about a poller's lifecycle
  calls for token expiry independent of the credential simply being
  rotated by the operator, the same way `internal_token` is rotated
  today). A JWT's main advantages — embedding claims, expiry, and
  cryptographic signing in the token itself — solve problems this app
  doesn't have yet (Decision 3 handles scoping server-side instead, where
  it's reviewable in one file rather than baked into a token nobody
  outside `api` can easily inspect).
- **A separate identity header alongside the token** (e.g.
  `X-Internal-Service: poller-ldbws` plus `X-Internal-Token: <value>`).
  **Rejected.** This reintroduces exactly the failure mode a shared secret
  already has today: a caller could claim to be any service by setting the
  identity header to whatever it likes, unless the token is *also*
  cross-checked against that specific claimed identity — at which point
  the identity header is redundant, since the token alone already
  determines identity once tokens are made distinct per service.
- **Chosen: one token per service, still sent as the same
  `X-Internal-Token` header, still just a `&str` on the wire** — the token
  value itself *is* the identity, resolved via a lookup rather than a
  single-value comparison. This is the minimal edit to the existing
  mechanism: `common::ingest::post_batch`/`fetch_last_fetched`'s
  signatures (`crates/common/src/ingest.rs:35–57`, `:99–112`) don't change
  at all — they already take an opaque `internal_token: &str` and attach
  it to one header; only the *value* each poller is configured with
  changes, from the one shared string to a distinct one per service. It
  also mirrors a pattern this codebase already has, just for a different
  actor: `AuthenticatedUser`'s session flow (`auth.rs:154–198`) is already
  "opaque token → hash → lookup → identity," via `hash_session_token`
  (`auth.rs:164–168`) and `get_session_with_user`
  (`crate::data::users::get_session_with_user`). Internal service auth
  becomes the same shape — opaque token → hash → lookup → `InternalService`
  identity — reusing a pattern this codebase has already chosen once
  rather than inventing a new one.

Concretely: `require_internal_token` stops comparing the header against one
fixed string, and instead hashes the provided token (the existing
`hash_session_token` function, or a sibling with the same shape) and looks
it up in a small, startup-built table mapping token-hash → service
identity. A lookup miss is `401` (unknown credential — same status as
today's only failure mode). A lookup hit resolves an identity, which
Decision 2 then checks against the requested route.

### 2. Route-scoping mechanism: a static, code-defined table (service → allowed route prefixes) — not a DB-backed ACL table

Two real alternatives:

- **A DB-backed table** (`internal_service_accounts` /
  `internal_service_routes` or similar, with a migration alongside the
  existing ones in `crates/api/migrations/`). **Rejected**, for reasons
  specific to this app's actual shape, not by default:
  - The set of internal callers is small (seven today, one anticipated —
    `schedule-reference`) and changes only when a new poller/service is
    *written and deployed* — a code change and a chart change happen
    either way; a DB row buys no independence from a deploy the way it
    would for, say, a dynamically-onboarded multi-tenant API consumer.
  - `require_internal_token` runs as `axum::middleware::from_fn_with_state`
    on **every** `/private/*` request, including the highest-frequency
    ones (`poller-ldbws` posts a full station-sample batch every 60s per
    `poll_interval_secs`'s default in `poller-ldbws/src/config.rs:50`). A
    DB round trip (or an in-memory cache with invalidation logic) on that
    hot path is real, ongoing complexity a static table has none of.
  - This mirrors a stance already visible elsewhere in this codebase:
    `2026-09-01-tracked-trains-home-page-design.md`'s Decision 3
    explicitly rejects adding backend flexibility "on a hypothesis rather
    than a measured cost," calling that "exactly the kind of speculative
    complexity this codebase's existing comments consistently avoid
    elsewhere." A dynamic, DB-backed permission system for seven
    statically-known callers is the same category of premature
    flexibility.
- **Chosen: a static table, defined in Rust, in `crates/api/src/auth.rs`
  itself** — an `InternalService` enum (one variant per legitimate caller:
  `PollerIncidents`, `PollerStations`, `PollerTocs`, `PollerLdbws`,
  `PollerTfl`, `TrustConsumer`, `ScheduleIngest`) with an associated
  `allowed_prefixes(&self) -> &'static [&'static str]` built directly from
  the route table above — e.g. `PollerLdbws` → `["/sample-stations",
  "/station-samples"]`, `TrustConsumer` → `["/train-events",
  "/tracked-trains"]`, `ScheduleIngest` → `["/schedule-feed-ingests"]`.
  `require_internal_token` checks the resolved identity's allowed prefixes
  against `request.uri().path()` with a plain prefix match — no new axum
  extractor, no `MatchedPath` machinery, consistent with this file's
  existing hand-rolled-over-dependency posture (`parse_cookie`'s own doc
  comment, `auth.rs:66–71`, gives the same justification for hand-rolling
  something this narrow rather than pulling in `axum-extra`).

This split deliberately separates two different kinds of change with two
different blast radii: **who is allowed to call what** (the scoping table)
is application code, reviewed like any other code change, and ships in the
same binary/image as the routes it protects — a new route and its allowed
caller land in the same PR, so it's structurally impossible to add a
`/private/*` route without also deciding who may call it. **What secret
value each service currently holds** stays where it already is:
operator-provisioned config/secrets, rotatable without a code change. This
is the same split `auth.rs`'s own doc comment already draws for the
existing single-token mechanism (fixed logic in code, one external value in
config) — just applied per service instead of globally.

### 3. Error handling / status codes for a wrong-scope request: `403`, not `404` — this is not the ownership convention

This app has an established, explicit convention for ownership checks:
*"same 404-for-both-'missing'-and-'not-yours' convention as every other
ownership check in this app, never `403`"* (`crates/api/src/routes/
train.rs:9–11`). That convention exists to avoid confirming to an
unauthorized **user** that some other user's specific resource (a tracked
train, a custom line) exists at all — the resource's existence is the
sensitive fact, and a `404` withholds it uniformly whether the row is
missing or just not yours.

That reasoning does not transfer here, and this design deliberately departs
from it:

- The thing being protected is not a **row a caller might not know
  exists** — it's a **fixed, small set of routes whose existence is
  already public information**, visible to anyone who reads this
  open-source repository's `crates/api/src/routes/ingest.rs`. There is no
  existence fact to withhold; `/private/schedule-feed-ingests` is not a
  secret path.
- The caller is not an anonymous or lightly-authenticated **human** probing
  for what exists — it's another **internal service holding a valid,
  provisioned credential**, already trusted enough to reach `api`'s
  private surface at all. A `403` here tells that caller "your credential
  is real, but not for this" — useful, actionable information for
  debugging a misconfigured deployment (the actual, likely failure mode:
  e.g. `poller-tfl`'s pod ending up with `poller-incidents`'s token by a
  chart/secret wiring mistake), not a leak to an adversary.
- `404`-for-everything would actively work against Decision 6
  (auditability): a `404` on a wrong-scope call is indistinguishable in
  logs/metrics from a genuine routing mistake or a stale URL, whereas a
  distinct `403` with the resolved (but disallowed) identity logged is
  immediately diagnosable.

So: unknown/invalid token → `401 Unauthorized` (unchanged from today —
no credential resolves at all). Known, valid, wrong-scope token →
`403 Forbidden` (a credential resolved to a real identity, and that
identity is not allowed here). This intentionally reintroduces the
distinction between "who are you" and "are you allowed" that the ownership
convention collapses on purpose for user-facing routes — because the
reason that convention collapses it (withholding existence from an
untrusted party) doesn't apply to a trusted internal caller hitting a
publicly-visible, fixed route table.

### 4. Not sharing a primitive with a future human-roles feature

The prompt for this spec explicitly asks whether the same "named credential
→ allowed route set" mechanism could serve both this feature and a future
human-user roles/permissions feature. Real call, not hand-waved: **no,
keep them separate**, for a structural reason confirmed by what's actually
in this codebase (Current relevant state, above) rather than by general
principle:

- **Service identities are static and deploy-time.** They're provisioned
  by the Helm chart/`docker-compose.yml` alongside the pods that use them,
  change only when a new service is written and deployed, and — per
  Decision 2 — deliberately need no database access at all: today,
  `require_internal_token` runs before any database query in the request
  path. Routing internal-service auth through a DB-backed table would add
  a database dependency to a check that currently has none, purely to
  share a primitive with a feature that doesn't exist yet.
- **Human roles, if this app ever gets them, are inherently dynamic** —
  assignable to an existing `users` row at runtime by some admin action,
  without a redeploy, the same way `AuthenticatedUser`'s session lookup
  already depends on the `users`/`sessions` tables (`crates/api/src/
  data/users.rs`) rather than static config. Forcing service accounts into
  that same DB-backed shape would add DB dependency and migration
  overhead to a concern that doesn't need it (Decision 2); forcing a
  future human-roles feature into this design's static, code-defined table
  would make role assignment a code change plus a deploy, which is very
  likely the wrong fit for real admin-facing role management.

The two features share a *concept* (a named identity mapped to a set of
allowed actions) but not an *implementation* — same as how this app already
has two structurally different "opaque token → identity" lookups
(`AuthenticatedUser`'s DB-backed session lookup vs. this design's
static-table service lookup) rather than one shared mechanism forced to
serve both. If a human-roles feature is designed later, it should read this
section and make its own call rather than inherit this one by default.

### 5. Rollout: a bounded dual-acceptance transition window, not a flag-day cutover

The prompt frames this as a real choice between "keep the old shared token
working during a transition" and "a coordinated cutover, since this is
entirely internal, same-deployment infrastructure the operator controls end
to end." The second framing is attractive but doesn't hold up against how
this chart actually rolls out changes: **`api`'s Deployment and each
poller's/service's Deployment are independent Helm-templated objects with
no ordering or coordination between them** — confirmed by reading
`poller-deployments.yaml` (one template rendering five separate `Deployment`
objects, `strategy: {type: Recreate}` each, `poller-deployments.yaml:24–
28`), `trust-consumer-deployment.yaml`, and `schedulefeed-deployment.yaml`
as four/five/six **separate** Kubernetes objects. A single `helm upgrade`
that changes both `api`'s token-checking logic and every poller's token
value does not guarantee `api`'s new pod is live before any poller's new
pod starts sending its new token, nor the reverse — Kubernetes rolls each
Deployment independently, and Helm does not sequence them for this chart
today (no documented hook/wait-for ordering exists anywhere in
`charts/distant-signal/templates/`).

Given that, a hard cutover has a real failure window: whichever pod (API or
poller) gets its new image/env first will, for some seconds-to-minutes,
either reject a still-old poller's old shared token (if `api` is upgraded
first and drops shared-token acceptance immediately) or send a new
per-service token an old `api` doesn't understand yet (if a poller is
upgraded first). Both directions crash-loop or 401-loop a poller until the
rollout catches up — self-healing, but noisy and avoidable.

**Chosen: `api` accepts both the legacy shared token and the new
per-service tokens for one release**, treating the legacy value as its own
special, unscoped `InternalService::Legacy` identity that's allowed every
route (today's actual behavior, preserved) but logged at `warn` on every
use (Decision 6) so the operator has a concrete, observable signal for "is
anything still using the old token." The legacy value stays wired exactly
as it is today (`secrets.internalToken` / `INTERNAL_TOKEN` env var,
unchanged chart key) through this window; once the operator confirms (via
that logging) that every poller/service has rolled onto its own per-service
token, a follow-up chart change removes legacy acceptance and the shared
`internal-token` secret key entirely. This is a real, bounded transition —
not a permanent second code path — sized to this chart's actual,
independently-rolling Deployment objects, not to a general "always support
both" habit.

## Architecture

```
poller-ldbws pod                                    api pod
┌─────────────────────┐                             ┌──────────────────────────────────────────┐
│ Config::internal_    │  POST /private/             │ private_router()                          │
│  token = "svc-ldbws- │  station-samples             │  .layer(from_fn_with_state(               │
│  9f2a...c04e"        ├──────────────────────────────►    require_internal_token))               │
│                       │  X-Internal-Token: svc-ldbws-│                                            │
│ common::ingest::      │   9f2a...c04e                │  1. hash(token)                           │
│  post_batch()         │                              │  2. lookup in static table:               │
└─────────────────────┘                             │       hash → InternalService                │
                                                       │     miss  → 401 Unauthorized              │
                                                       │     hit   → PollerLdbws                   │
                                                       │                                            │
                                                       │  3. check request.uri().path()             │
                                                       │     against PollerLdbws.allowed_prefixes: │
                                                       │       ["/sample-stations",                │
                                                       │        "/station-samples"]                │
                                                       │     "/station-samples" ∈ allowed  ✔        │
                                                       │       → next.run(request)                 │
                                                       │                                            │
                                                       │  ingest::router()                          │
                                                       │    post_station_samples() → 200 OK        │
                                                       └──────────────────────────────────────────┘

Wrong-scope example (misconfigured pod, or a compromised poller trying its
luck):

poller-tfl pod (holding poller-tfl's own token, correctly)
┌─────────────────────┐
│ token = "svc-tfl-    │  POST /private/schedule-feed-ingests
│  4b1e...77aa"        ├───────────────────────────────►  1. hash(token)
└─────────────────────┘                                   2. lookup → PollerTfl (valid identity)
                                                            3. "/schedule-feed-ingests" ∉
                                                               PollerTfl.allowed_prefixes
                                                               → 403 Forbidden
                                                               (Decision 3 + 6: logged with the
                                                                resolved identity, not silent)
```

`public_router()` and the `AuthenticatedUser`/session flow
(`crates/api/src/auth.rs:176–211`) are entirely untouched by this design —
this is scoped to `private_router()` and the internal-service side of
`auth.rs` only.

## Testing

Following this codebase's existing convention in `crates/api/src/auth.rs`'s
own `#[cfg(test)] mod tests` (lines 213–415: fixed-time-comparison unit
tests, cookie-parsing unit tests, `validate_return_to` table of accept/
reject cases):

- **Identity resolution**: a known service's token resolves to the correct
  `InternalService` variant; an unknown token resolves to nothing (`401`);
  an empty token behaves the same as today's `empty_provided_against_
  real_token_does_not_match` case (`auth.rs:238–240`) — still rejected,
  not accidentally matched against an empty legacy value.
- **Scope enforcement**: a table-driven test enumerating every real
  `(InternalService, route)` pair from the surface table above — each
  service's own allowed routes return `200`-eligible (not `401`/`403` at
  the auth layer; the handler's own logic is untested here, only that the
  middleware lets the request through), and at least one **other**
  service's token against that same route returns `403`. This test is
  explicitly a regression guard for "a newly added `/private/*` route
  forgets to declare who may call it" — the table should be exhaustive
  enough that adding a route without adding it to some service's allowlist
  is either a compile error (if the surface table is derived from the
  route list) or an obviously-failing test (if not).
- **Default-deny**: a fabricated route not present in *any* service's
  allowlist is rejected for every known identity — guards against an
  accidental "allow everything" fallback.
- **Legacy-token transition (Decision 5)**: the legacy shared token still
  resolves and is still granted every route during the transition window,
  and doing so is observably logged (Decision 6) so a test can assert the
  log event fires — this is the mechanism the "safe to retire" decision
  depends on, so it needs its own coverage, not just an assumption that
  logging happens.
- **Startup validation**: mirroring `app.rs:65–72`'s existing
  `internal_token` non-empty assertion, an equivalent check that no
  per-service token is empty (or that a service with an empty token is
  refused a scope) — an empty per-service token must not silently become
  "matches an empty header," the same failure class the current
  single-token design already guards against.

## Explicitly out of scope

- **A DB-backed, dynamically-editable service-account/permission table.**
  Decision 2 rejects this for the size and change-cadence of this app's
  actual caller set; revisit only if that changes materially (many more
  internal services, or a real need to revoke/re-scope a credential
  without a redeploy).
- **Any change to human-user auth, roles, or permissions.** Decision 4
  keeps this fully separate; this spec touches none of
  `crates/api/src/data/users.rs`, `AuthenticatedUser`, or the session
  cookie flow.
- **mTLS, JWTs, or any credential shape beyond a per-service bearer
  token.** Weighed and rejected in Decision 1.
- **Token expiry/rotation automation.** Per-service tokens are long-lived,
  operator-rotated, exactly like `internal_token` is today — no TTL or
  self-expiry concept is introduced.
- **A concrete scope entry for `schedule-reference`.** That crate does not
  exist in this codebase as of this session (Current relevant state,
  above); this design describes the shape a new entry would take (one more
  `InternalService` variant, one more Helm-provisioned token, one more row
  in the route table) but does not invent its actual route surface.
- **Any Prometheus/metrics surfacing of per-service call volume.** Decision
  6 recommends *logging* the resolved identity per request as a low-cost,
  immediate win; turning that into a metrics label (this app already has a
  metrics story per `docs/superpowers/specs/2026-08-29-metrics-design.md`)
  is a natural, separate follow-up, not designed here.
- **Helm chart / `values.yaml` / `docker-compose.yml` edits themselves.**
  This spec sketches the shape (service accounts get their own
  `pollers.<name>.internalToken`-style values entries and
  `internal-token-<name>`-style secret keys, following the
  `rdm-<name>-api-key` precedent at `secret.yaml:54–62` and the
  this-app-mints-the-credential auto-generation posture at
  `schedulefeed-secret.yaml:28–59`; local dev gets one
  `INTERNAL_TOKEN_<SERVICE>`-style env var per service, extending
  `dev.env.example`'s already-accepted per-file duplication convention,
  lines 7–19) but the constraint on this task is design only — no chart
  file is edited.

## Open questions / risks

1. **Exact Rust-side config shape for N named tokens** isn't fully
   specified here — most likely one explicit, named `clap` field per known
   service (mirroring `pollers.<name>.apiKey`'s one-field-per-known-poller
   shape in the chart, rather than a dynamically-sized generic map), but
   that's an implementation-time call, not resolved by this design.
2. **How long the Decision 5 transition window should be, and what
   "confirmed migrated" means operationally** (a fixed number of days, a
   manual operator checklist against the Decision 6 logs, or a future
   chart major version that simply drops the legacy key) — not specified
   precisely here.
3. **`schedule-reference`'s real route surface and legitimate caller are
   unknown** until that crate lands; the scope table above will need a new
   row (and possibly a new `InternalService` variant) added when it does,
   not invented ahead of time.
4. **`poller-ldbws` is the one service with more than one allowed route
   today** (`/sample-stations` + `/station-samples`); this design's "one
   identity → a flat list of allowed prefixes" primitive already covers
   that case, but if a future poller needs a more structured scope (e.g.
   different methods allowed on different routes, not just path
   prefixes), the static-table approach may need to grow from "list of
   prefixes" to "list of (method, prefix) pairs" — not needed by anything
   in today's route surface, flagged in case it is later.
5. **`403` on a wrong-scope request from a legitimate-but-misconfigured
   pod is the expected, likely real-world trigger for this code path** —
   Decision 3 argues this is fine to observe plainly (not an information
   leak), but it does mean a chart/secret wiring bug (the wrong
   `secretKeyRef` on the wrong poller) now surfaces as a clear `403` in
   `api`'s logs rather than silently working (as it would today, since
   every service currently holds the one token valid everywhere) — a
   behavior change worth calling out explicitly to whoever deploys this,
   not just a side effect.
