# Design: Internal Service Auth via OAuth2 Client Credentials (Authentik)

**Status: design proposal, not approved.** No implementation plan is
included; that is a separate, later step in this repo's process, per this
repo's own established convention (see, e.g., every design under
`docs/superpowers/specs/`, none of which contain a task list).

## Goal

Every internal microservice that pushes ingested data into this app —
`poller-incidents`, `poller-stations`, `poller-tocs`, `poller-ldbws`,
`poller-tfl`, `trust-consumer`, and `schedule-ingest` — authenticates to the
`api` crate's `/private/*` routes today with one shared-secret value,
`X-Internal-Token`, compared in fixed time against a single configured
string. Any service holding that one value can call any `/private/*` route,
including routes that are not its job.

This app already runs a real OAuth2/OIDC authorization server — Authentik —
integrated for human users (`crates/api/src/auth/oidc.rs`). This design
redesigns internal-service auth around **that same authorization server**,
using its standard machine-to-machine mechanism (the OAuth2 Client
Credentials Grant), rather than a second, DS-built identity/authorization
system. Concretely, this spec covers: confirming exactly what Authentik
supports for M2M auth (not assumed); how `crates/api` validates an incoming
request's Authentik-issued access token; how an access group/claim on that
token drives route-level authorization, replacing a hand-rolled Rust
route-scoping enum; and the DS-side config surface — one field set per real
internal caller — needed for each of the 7 services to obtain and present
such a token. Defining the actual Authentik-side service accounts, OAuth2
provider, and groups is explicitly out of scope (below) — this is entirely
a DS-side design.

## Relationship to prior specs

`docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md` and
`docs/superpowers/plans/2026-09-01-internal-service-accounts.md` describe a
**reverted, superseded approach**: a custom, DS-built per-service token
scheme (`InternalService` enum, `InternalServiceRegistry`, a static
Rust route-scoping table, per-service secrets minted and stored by this
app's own Helm chart/`docker-compose.yml`). That work was implemented,
merged (`0bcd0e285963e7aee07e5bb3b45fadaa1ee07d03`), and has just been
**reverted** (`927efc5`, a clean revert — confirmed via `cargo build
--workspace` and `helm lint` on this worktree) because it was flagged as
the wrong architectural direction: building a bespoke identity/authorization
system by hand when this app already has a real, working OAuth2/OIDC
authorization server integrated is duplicate, unnecessary infrastructure.
Both documents are left in place (this repo's non-silent-revision
convention), not deleted, but describe a dead end. **This spec replaces
that approach entirely.**

One structural piece carries forward deliberately, not by accident: the
reverted design's Decision 2 (a static, code-defined table mapping "who may
call what route") is the right shape for *authorization* regardless of
where *identity* comes from, and this spec reuses that shape — a static
route-scoping table in `crates/api`. What changes completely is what the
table's *key* is and where *identity/authentication* comes from: the
reverted design's key was a `InternalService` enum resolved by DS's own
secret-lookup; this design's key is an Authentik-asserted group name on a
verified OAuth2 access token, resolved by Authentik, not minted or looked
up by DS at all. See Decision 3.

This spec also depends on, and does not duplicate, the concurrently-written
`docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md`
(hereafter "the MCP sibling spec"), which investigated Authentik's `groups`
claim mechanism for the **human ID-token login** case and found DS's OIDC
client currently requests only `email`/`profile` scopes and that
`openidconnect`'s `core::CoreClient` type is hardcoded to
`EmptyAdditionalClaims`, silently discarding any claim beyond the standard
set. This spec cites and builds on that investigation's findings about
Authentik's group-claim mechanism (Current relevant state, below) but
independently re-investigates whether the same code-side obstacle applies
to *this* spec's flow — it largely does not, because this is a different
token type verified by different, new code (Decision 3).

## Current relevant state (verified this session)

### The shared-secret mechanism, as it exists right now (post-revert)

`crates/api/src/auth.rs`'s module doc (lines 1–6) states the mechanism
plainly: one shared-secret header (`X-Internal-Token`), compared in fixed
time (`constant_time_eq`, lines 38–56) against
`ServiceArguments::internal_token` (`crates/api/src/data/config.rs:57`).
`require_internal_token` (`auth.rs:20–36`) is the entire check — no concept
of *which* caller presented the token, only "is it the one correct value."

`crates/api/src/routes/mod.rs::private_router()` (lines 58–75) applies this
middleware via `.layer(middleware::from_fn_with_state(app,
require_internal_token))` (line 62) to the entire merged router — every
route gets the identical gate. Its own doc comment (lines 53–57) explains
why it takes `App` directly: `from_fn` fixes the handler's state to `()`,
so a stateful check needs `from_fn_with_state` up front — this constraint
carries forward unchanged into this design's own middleware.

`crates/api/src/app.rs`'s `AppState::init()` (lines 61–72) refuses to start
if `internal_token` is empty, with the comment: *"An empty token would make
`auth::constant_time_eq` compare two empty byte slices and accept any
request with no `X-Internal-Token` header at all — reject that at startup
rather than silently running an unauthenticated `private_router()`."* Any
new scheme needs an equivalent startup guard against its own empty-config
failure modes.

### The full `/private/*` route surface — unchanged from before the revert

`crates/api/src/routes/ingest.rs::router()` (lines 28–53) and
`crates/api/src/routes/samples.rs::router()` (line 17), merged into
`private_router()`:

| Route | Methods | Handler(s) | Legitimate caller |
|---|---|---|---|
| `/incidents` | GET, POST | `ingest.rs:60–67`, `:101–109` | `poller-incidents` |
| `/stations` | GET, POST | `ingest.rs:69–76`, `:111–119` | `poller-stations` |
| `/tocs` | GET, POST | `ingest.rs:78–81`, `:131–139` | `poller-tocs` |
| `/station-samples` | GET, POST | `ingest.rs:83–90`, `:121–129` | `poller-ldbws` |
| `/tfl-line-status` | GET, POST | `ingest.rs:92–99`, `:148–156` | `poller-tfl` |
| `/train-events` | POST | `ingest.rs:160–170` | `trust-consumer` |
| `/tracked-trains` | GET | `ingest.rs:176–183` | `trust-consumer` |
| `/schedule-feed-ingests` | GET, POST | `ingest.rs:206–213`, `:215–224` | `schedule-ingest` |
| `/sample-stations` | GET | `samples.rs:20–25` | `poller-ldbws` |

`find crates -maxdepth 1 -type d` (this session) lists `aggregator`, `api`,
`common`, `enricher`, `poller-incidents`, `poller-ldbws`,
`poller-stations`, `poller-tfl`, `poller-tocs`, `schedule-ingest`,
`trust-consumer` — **11 crates, no `schedule-reference` crate exists yet**.
So the confirmed list of real internal callers today is exactly **7**:
the five RDM/TfL pollers, `trust-consumer`, and `schedule-ingest`. This
spec is written to accommodate a future `schedule-reference` addition (one
more row, everywhere a "7" appears below) but does not invent its route
surface, matching the reverted design's own stated posture on this point.

Every one of the 7 declares an identically-shaped `internal_token: String`
field via `#[arg(long, env)]` — confirmed directly this session:
`crates/poller-incidents/src/config.rs:30`,
`crates/poller-stations/src/config.rs:29`,
`crates/poller-tocs/src/config.rs:30`,
`crates/poller-ldbws/src/config.rs:45`,
`crates/poller-tfl/src/config.rs:40`,
`crates/trust-consumer/src/config.rs:66`,
`crates/schedule-ingest/src/config.rs:56`. All 7 also depend on `common =
{ path = "../common" }` and, notably, all 7 pin `reqwest = "0.13.4"` in
their own `Cargo.toml` — this matters directly for Decision 4.

### How each poller/service authenticates today

`crates/common/src/ingest.rs` is the shared contract module — its own doc
comment (lines 1–13) names every consumer: the four RDM pollers,
`poller-tfl`, and `api`'s ingest routes (`trust-consumer`/
`schedule-ingest` use the same module too, confirmed by the `common`
dependency above and their own `main.rs` files). `INTERNAL_TOKEN_HEADER`
is one constant, `"x-internal-token"` (`ingest.rs:22`). `post_batch`
(`:35–57`) and `fetch_last_fetched`/`time_until_next_poll` (`:83–112`) each
take a plain `internal_token: &str` and attach it via
`.header(INTERNAL_TOKEN_HEADER, internal_token)` — a static string, read
once from config, unchanged for the life of the process. This is the exact
call-site shape Decision 4/5 below has to replace: a live, periodically
refreshed OAuth2 access token in place of a static config value.

### How the shared secret is provisioned today

`charts/distant-signal/templates/secret.yaml:36–41` auto-generates one
`internal-token` Secret key via `randAlphaNum 32` if
`.Values.secrets.internalToken` is empty, preserved across `helm upgrade`.
`charts/distant-signal/values.yaml:36–44` (`secrets.internalToken`,
`secrets.existingSecret`, `secrets.existingSecretInternalTokenKey`) and
`_helpers.tpl:186–190`
(`distant-signal.internalTokenSecretName`/`internalTokenSecretKey`) are the
resolving helpers, consumed by `api-deployment.yaml`, all five poller
Deployments, `trust-consumer-deployment.yaml`, and
`schedulefeed-deployment.yaml`. `docker-compose.yml` interpolates the same
`${INTERNAL_TOKEN}` into eight service definitions (lines 105, 155, 176,
200, 225, 250, 331, 489); `dev.env.example:120` sets it once.

**The precedent that matters most for this design** is the distinction the
reverted design's own Decision 1/Task 5 already drew, correctly, between
two categories of chart secret: `pollers.<name>.apiKey`
(`values.yaml:159–162`) is **never** auto-generated — its own comment says
a random RDM key is "meaningless" because an *external system* (RDM)
already assigned that value, and this app is only ever handed it. Versus
`secrets.internalToken`/`scheduleFeed.sftp.password`
(`schedulefeed-secret.yaml:70–73`), which **are** auto-generated, because
*this app itself* mints and hands out those credentials. Under this
design, every one of the 7 services' own OAuth2 credentials falls into the
**`apiKey` category, not the `internalToken` category** — Authentik, an
external system, assigns each service account's username/password, and
`api` never mints or stores a copy of it. This flips which auto-generation
posture applies compared to the reverted design, where the tokens were
`internalToken`-shaped (Decision 6, below).

### `crates/api`'s existing OIDC client (human SSO) — re-confirmed this session

`crates/api/src/auth/oidc.rs:161-162` requests exactly `email`/`profile`
scopes; `crates/api/src/auth/oidc.rs:94-95`'s `DiscoveredClient` type alias
is built on `openidconnect::core::CoreClient`, which per `docs.rs/
openidconnect/4.0.1` (matching the pinned `Cargo.lock` version) fixes its
`AdditionalClaims` type parameter to `EmptyAdditionalClaims` — any claim
beyond the standard set (including a `groups` claim, if present) is
silently discarded on this path. `crates/api/Cargo.toml` pins `reqwest =
"0.12"` specifically because `oauth2 = "5.0"`/`openidconnect = "4.0"`
implement their `AsyncHttpClient` trait for reqwest 0.12's `Client` (the
crate's own comment on this, `Cargo.toml`, explains the pin in detail).
This is the exact gap the MCP sibling spec found and designed around for
the **human ID-token login** path. It is cited here, not re-investigated
from scratch, per this spec's task scope — but see the next two
subsections for why it does not block *this* spec's flow the same way.

### Authentik's real client-credentials/machine-to-machine support — investigated this session, not assumed

Fetched directly from `docs.goauthentik.io` (the M2M page, the OAuth2
provider page, and the service-accounts page) this session, corroborated
by real-world reports (GitHub discussions/issues):

- **Authentik's OAuth2/OpenID provider supports four grant types:
  authorization code, client credentials, device code, and token
  exchange** — "by default, all types are selected" on a given provider.
  The client credentials flow is documented as "typically used for
  machine-to-machine or server-to-server authentication, where no user is
  involved." **This confirms RFC 6749 §4.4 support as a real, first-class
  Authentik capability**, not something to bolt on.
- **Authentik does *not* use a plain, per-caller `client_id`+`client_secret`
  pair for this**, and says why explicitly: *"OAuth providers can only have
  a single secret at any given time"* — one Authentik Application/Provider
  has exactly one `client_secret`, so distinct credentials for N distinct
  M2M callers cannot be N different `client_secret` values under one
  provider. Instead, Authentik layers its own, more specific primitive
  underneath the `client_credentials` grant: a **Service Account** — *"a
  specialized user designed for machine-to-machine authentication and
  automation"* (per `docs.goauthentik.io/sys-mgmt/service-accounts/`),
  which "cannot access the authentik user or Admin interfaces," "lacks a
  usable account password" (only app passwords/API tokens), and "should
  never represent actual people." Each service account gets its own
  username + app-password (a token), created via **Directory > Users >
  New User > Service Account** (default token expiry: 360 days,
  operator-configurable) and, per that same doc, **can be assigned to
  groups** — the same group-membership mechanism a human user has.
  **This directly answers the task's question**: Authentik has both — the
  OAuth2 *grant type* is literally `client_credentials`, but the *specific
  caller credential* it authenticates against is a service account's
  username + app-password (or a JWT-bearer assertion), not a raw shared
  provider secret. This matches, and confirms, the user's own phrasing —
  "username and password ... if authentik supports this for service
  accounts" — precisely: it does, and it is the *only* real way to get N
  independent M2M credentials out of one Authentik Application, since a
  provider's own `client_secret` is singular.
- **Wire shape, confirmed via the actual documented request format**:
  ```
  POST /application/o/token/ HTTP/1.1
  Content-Type: application/x-www-form-urlencoded

  grant_type=client_credentials&client_id=<provider_client_id>
    &username=<service_account_username>&password=<app_password>
    &scope=profile groups
  ```
  ("Authentik treats a grant type of `password` the same as
  `client_credentials`" — either works.) A **base64-encoded variant** also
  exists: `username:app_password` base64-encoded into the standard
  `client_secret` field, letting a caller use the conventional
  two-field (`client_id`+`client_secret`) request shape without an
  explicit `username`/`password` pair — useful if a future implementation
  wants to hand an off-the-shelf OAuth2 client-credentials library a
  plain `client_id`/`client_secret`, though this design (Decision 4)
  recommends hand-rolling the request instead, for an unrelated reason.
- **Token format: a signed JWT, meant to be verified locally.** *"This
  will return a JSON response with an `access_token`, which is a signed
  JWT token. This token can be sent along with requests to other hosts,
  which can then validate the JWT based on the signing key configured in
  authentik."* Signing is asymmetric (RS256) when a Signing Key is
  configured on the provider (verified via the provider's JWKS,
  `/application/o/<application_slug>/jwks/`), or symmetric (HS256, signed
  with the provider's own `client_secret`) when no Signing Key is
  configured. **This design assumes/recommends the operator configures an
  asymmetric Signing Key on the internal-service provider** — with HS256,
  `crates/api` would need to hold a copy of the provider's own
  `client_secret` just to verify signatures, re-introducing a shared
  secret `api` has to store; with RS256, `api` verifies with a JWKS public
  key it fetches over HTTP, and never needs to hold any secret credential
  for verification at all. This exact operator-side setting is a real
  dependency this design has on the Authentik-side provisioning that is
  out of scope to define here — flagged again in Open questions/risks.
- **Token introspection also exists** — a global endpoint,
  `/application/o/introspect/`, "though access to tokens is still scoped
  by provider" — a real, usable RFC 7662-shaped facility, just not the one
  this design recommends for the per-request hot path (Decision 2).
- **Access token validity is operator-configurable per provider** (a
  distinct field from refresh/ID-token validity); this session's search
  did not turn up an authoritative single default value, but real-world
  configuration examples commonly show single-digit-minutes settings.
  This design does not assert a specific number — it recommends the
  operator configure a **short** validity for the internal-service
  provider specifically (Decision 2's revocation-tradeoff mitigation).

### Authentik groups on a client-credentials-issued token — investigated this session, not assumed to be the same as the ID-token case

The MCP sibling spec found, for the **ID-token/human-login** case, that
Authentik's built-in `profile` scope mapping's default expression includes
`"groups": [group.name for group in request.user.ak_groups.all()]`, and
that this rides the **already-requested** `profile` scope, with a
dedicated custom scope mapping being the more reliable pattern in
practice. The task for this spec explicitly asked not to assume the same
mechanism applies verbatim to a client-credentials access token, since it
carries no ID token and no browser-mediated user at all.

**Investigated directly**: the *mechanism* is the same, because a service
account **is** a `User` object in Authentik's data model (confirmed by the
service-accounts doc's own framing above — a service account is "a
specialized user," not a separate object type). The same scope-mapping
expression (`request.user.ak_groups.all()`) evaluates against the service
account's own group memberships when the token request's `request.user` is
a service account, exactly as it does for a human's `request.user`. The
M2M docs confirm the *request-side* mechanism is identical, too: *"Scopes,
if required, must be defined in the request... To include custom claims
from scope mappings in the issued JWT, select the scope mappings on the
OAuth2 provider and request their scope names in the token request with
the scope parameter, for example `scope=profile custom-scope`."* — i.e.
requesting `scope=groups` (or whatever custom scope name is configured) in
the client-credentials POST is the M2M-side equivalent of the ID-token
case's `add_scope` call.

**Where this spec's flow genuinely differs from the sibling spec's, and
why the `EmptyAdditionalClaims` gap does not block it**: the sibling
spec's obstacle is specific to `crates/api`'s *existing* ID-token
verification code path — `openidconnect::core::CoreClient`, used because
`oidc.rs` already exists and already does full OIDC discovery + ID-token
verification for the browser login flow. This spec's flow verifies a
**different token** (an OAuth2 access token, not an ID token) on a
**different, new code path** (an inbound bearer-token check on
`/private/*` requests, not an outbound login flow) that this design
proposes writing from scratch (Decision 2/3) — it was never going to run
through `CoreClient`/`CoreIdTokenVerifier` at all, since those types are
specifically for verifying *ID tokens* returned from an authorization-code
flow, not arbitrary bearer access tokens presented on an API request. A
freshly written verifier defines its own claims struct with whatever
fields it needs, including `groups: Vec<String>`, with no
`EmptyAdditionalClaims`-shaped constraint to work around. **Net finding**:
the *claim-surfacing gap* (does an Authentik deployment's scope mapping
actually populate `groups` reliably) is a real, shared operator-environment
risk both specs carry (Open questions/risks, both documents); the
*code-side* gap (DS's Rust code can't see the claim even if present) is
specific to the ID-token/`CoreClient` path and does not apply here.

### `crates/common`'s reqwest-version split — a real constraint on Decision 4

`crates/api/Cargo.toml` pins `reqwest = "0.12"`, with its own comment
explaining this is load-bearing: `oauth2 5.0`/`openidconnect 4.0` only
implement `AsyncHttpClient` for reqwest 0.12's `Client` type, so bumping
`api`'s reqwest without a matching `oauth2`/`openidconnect` release breaks
the `request_async` call site. **`crates/common/Cargo.toml` and every one
of the 7 pollers'/services' own `Cargo.toml` pin `reqwest = "0.13.4"`** —
a different major version, confirmed directly this session (grep above).
Pulling the `oauth2` crate into `crates/common` (to reuse it for the
client-credentials request on the poller side) would therefore introduce
a *second*, incompatible major version of `reqwest` into every one of the
7 binaries' dependency trees, and the two `reqwest::Client` types could not
be shared as one instance. This is a concrete, verified reason (not a
taste preference alone) informing Decision 4.

## Decisions

### 1. Grant type & credential shape: OAuth2 Client Credentials Grant, presented as an Authentik service account's username + app-password

Confirmed above: Authentik's `client_credentials` grant is real, and its
actual per-caller credential is a service account's username +
app-password (a token), not a directly-provisioned `client_secret` per
caller (Authentik allows only one `client_secret` per provider). This
design adopts exactly that shape: **one shared Authentik OAuth2
Provider/Application for all 7 internal callers** (one `client_id`,
common to all), with **7 distinct Authentik Service Accounts** underneath
it — one per real caller — each with its own username + app-password.
Defining those 7 service accounts, the provider, and their group
memberships in Authentik is explicitly out of scope (below); this decision
only fixes the *shape* of credential DS's own config needs to hold per
service: a username and a password/token, plus the (shared) client_id and
token endpoint.

Alternatives considered and rejected: mTLS and a DS-issued structured JWT
were already rejected in the reverted design's own Decision 1, for reasons
that still hold (no existing certificate infrastructure in this app; no
lifecycle need for self-issued/short-lived tokens *DS* controls) — moot
here regardless, since the whole point of this redesign is delegating
identity to Authentik rather than DS minting anything. A single shared
`client_secret` reused by all 7 callers (closest analogue to today's
single `X-Internal-Token`) was considered and rejected: it reintroduces
exactly the "any caller can impersonate any other caller" property this
design exists to remove, and Authentik's own one-secret-per-provider limit
means it wouldn't even let 7 distinct secrets exist under one provider —
the operator would need 7 separate Applications/Providers to get 7
distinct `client_secret`s, which is *more* Authentik-side objects to
provision than 7 service accounts under one shared provider, for no
compensating benefit.

### 2. Token validation: local JWT signature verification against Authentik's JWKS — not per-request introspection

Two real alternatives, both genuinely supported by Authentik (Current
relevant state, above):

- **Token introspection (RFC 7662), `/application/o/introspect/`, per
  request or with caching.** **Rejected as the primary mechanism.**
  `require_internal_token`'s successor runs on **every** `/private/*`
  request, including `poller-ldbws`'s every-60s station-sample POST — the
  reverted design's own Decision 2 already made this exact argument
  against a DB round trip on this hot path, and a network call to
  Authentik's introspection endpoint is the same category of added
  latency and added runtime dependency (if Authentik is briefly
  unreachable, `api`'s entire ingestion surface fails, not just new
  logins), just swapped for a different remote system. A cached-
  introspection variant (cache the introspection result until the token's
  own `exp`) closes most of the latency gap but converges on the same
  *effective* trust model as local JWT verification anyway (trust the
  token's own claims for its lifetime) while adding an extra network round
  trip on every cache miss that local verification never needs at all.
- **Local JWT verification against Authentik's JWKS (fetched once, cached,
  refreshed on a `kid` cache-miss or a timer).** **Chosen.** This is what
  Authentik's own M2M documentation describes as the intended mode for
  exactly this token type: *"can then validate the JWT based on the
  signing key configured in authentik."* Signature + `exp`/`iss`/`aud`
  checks are pure, in-process, sub-millisecond — zero I/O on the request
  path once the JWKS is cached, matching this codebase's demonstrated
  preference (constant_time_eq, hand-rolled cookie parsing, the reverted
  design's own static route table) for a hot-path check with no ongoing
  network dependency.

**The real cost of this choice, stated plainly**: local JWT verification
cannot see a *mid-lifetime* revocation. If an operator deletes/disables a
service account or rotates its password, an already-issued, not-yet-
expired access token remains signature-valid and is accepted until its own
`exp`. This is mitigated, not eliminated, by recommending a **short**
Authentik-side "Access token validity" setting for the internal-service
provider (an operator-time config choice, out of scope to mandate a exact
number here) — bounding the blast radius of a leaked or should-be-revoked
token to that same short window, the same "accept bounded staleness rather
than build a live-lookup-on-every-call mechanism" posture the MCP sibling
spec explicitly adopts for its own groups-staleness tradeoff (citing
`eta_blend.rs`'s "deliberately NOT a guaranteed join"), and the same
posture this codebase already has for the human session cookie itself
(cookie validity, not instant revocation, is the existing security
boundary there too). Introspection remains available as an operator-level
debugging/defense-in-depth tool; it is not designed as part of the
request-path mechanism here (Open questions/risks revisits this if a real
need for immediate revocation ever emerges).

**Implementation-time note, not resolved here**: `crates/api` already
depends on `openidconnect 4.0` for the human-login flow, which already
knows how to fetch/parse a JWKS document (used internally by
`CoreClient`'s ID-token verifier). Whether that dependency's own
lower-level JWK-verification primitives can be reused directly for a
*generic* JWT signature check (decoupled from `CoreClient`/
`CoreIdTokenVerifier`'s ID-token-specific semantics), versus adding a
small, dedicated JWT-verification crate (e.g. `jsonwebtoken`) purely for
this, is a real implementation-time investigation this spec does not
resolve — see Open questions/risks. What *is* decided here: this is real
cryptographic signature verification, not a narrow, single-call-site
mechanism like `constant_time_eq`/`parse_cookie` — hand-rolling RS256
verification from scratch is explicitly rejected as inappropriate for this
codebase's own stated hand-roll-narrow-things-only posture (`auth.rs`'s
own `parse_cookie` doc comment draws exactly this line: hand-roll only
what's "narrow," not general cryptographic verification).

### 3. Access groups: a `groups` claim on the access token, requested via scope, mapped by a static route-scoping table keyed on group *name* (a string), not a DS-resolved identity enum

Per the user's own explicit direction, the authorization decision — "may
this caller reach this route" — must be driven by what Authentik asserts
about the token, not a hand-rolled Rust enum matching route prefixes
(the reverted design's `InternalService`/`InternalServiceRegistry`).
Concretely:

- **Claim shape**: the internal-service OAuth2 provider has a `groups`
  scope mapping attached (built-in `profile` mapping's default expression,
  or — the more reliable pattern per Current relevant state — a dedicated
  custom scope mapping), and every one of the 7 services' token requests
  includes that scope (`scope=groups`, or whatever name the operator's
  mapping uses) alongside whatever else is needed. The resulting JWT
  access token carries `groups: [<Authentik group names the calling
  service account belongs to>]`, plus the standard `sub`/`iss`/`aud`/`exp`
  claims every OAuth2 access token carries.
- **Route-scoping table**: retained from the reverted design's Decision 2
  in *shape* only — a static, code-reviewed table in `crates/api`, no
  database, no dynamic runtime edit — but inverted and re-keyed: instead
  of `InternalService variant -> allowed route prefixes` (resolved by a
  DS-held secret lookup), this design uses `route prefix -> required group
  name` (a plain string), checked against the verified token's `groups`
  claim. E.g. `/incidents` requires membership in whatever group is
  configured as this deployment's "poller-incidents" group; `/station-
  samples` and `/sample-stations` both require the "poller-ldbws" group
  (mirroring the one service in today's surface with two legitimate
  routes); `/schedule-feed-ingests` requires the "schedule-ingest" group;
  and so on, one row per real caller, exactly mirroring the reverted
  design's own route table (Current relevant state, above) with the
  identity source swapped out.
- **Required group names are configurable, not hardcoded string
  literals** — a new `ServiceArguments` field per real caller (e.g.
  `internal_oauth_group_poller_incidents: String`, one per row of the
  table above, each with a suggested default like `svc-poller-incidents`)
  rather than a fixed Rust `&'static str`. This mirrors the MCP sibling
  spec's own explicit choice for the *same underlying reason*: this app
  has no established Authentik-group-naming convention of its own to
  hardcode against, and the exact names are the operator's call when they
  provision the groups (out of scope here). Unlike the credential fields
  (Decision 6), these are **not secrets** — a group name is not
  confidential — so they need not live in the chart's `Secret` object at
  all; a plain `ConfigMap`/values entry is sufficient, a real reduction in
  what `api`'s own secret material has to carry compared to the reverted
  design (further discussed in Decision 6).
- **One group per real caller, not one coarse group for all seven.** The
  MCP sibling spec deliberately keeps its own human-facing group count
  small (two groups, not six), reasoning that per-tool granularity adds
  real, ongoing *administrative* overhead (N users × M features) for a
  distinction its tool set doesn't yet need. That reasoning does not
  transfer here: each of these 7 callers is a single, fixed service
  account, provisioned once at deploy time and essentially never touched
  again by a human — assigning it to one already-named group, once, at
  provisioning time, costs the same whether there is one shared group or
  seven distinct ones; there is no human-facing combinatorial growth to
  economize on. Keeping one group per caller preserves the real
  least-privilege property the reverted design earned (a compromised or
  misconfigured `poller-tfl` pod cannot reach `/schedule-feed-ingests`)
  at no extra administrative cost in this usage pattern — the opposite
  conclusion from the sibling spec, reached for a genuinely different
  reason, not a contradiction of it.
- **Identity for logging, decoupled from authorization**: the token's
  `sub` (the service account's Authentik user identifier) is logged on a
  rejected or allowed request purely for diagnosability — the same
  motivation the reverted design's own Decision 3 gave for a distinct
  `403` on a wrong-scope request (a misconfigured pod holding the wrong
  service account's credential is the realistic trigger, and a resolvable
  identity in the log is what makes that diagnosable) — but `sub` plays no
  role in the authorization decision itself, which is entirely
  `groups`-driven.

### 4. Where the token-exchange logic lives: hand-rolled in `crates/common`, not the `oauth2` crate

**Rejected**: adding `oauth2 = "5.0"` (already a dependency of
`crates/api`) to `crates/common` and reusing its `ClientCredentialsTokenRequest`
support. Per Current relevant state, this would pull reqwest 0.12 into
every one of the 7 pollers'/services' dependency trees alongside their own
already-pinned reqwest 0.13.4 — two incompatible major versions of the
same crate, with no way to share one `reqwest::Client` instance between a
poller's normal HTTP calls (0.13) and its token-exchange calls (0.12).
Cargo permits this (multiple major versions can coexist in one binary's
tree), but it is real, avoidable complexity for what the client-credentials
request actually is: one `POST` with a small form-encoded body, one JSON
response with `access_token`/`expires_in`/`token_type` — a narrow, fully
RFC-specified shape.

**Chosen**: a new module in `crates/common` (the crate already depended on
by all 7 real callers, and already the home of the shared ingest wire
contract `INTERNAL_TOKEN_HEADER`/`post_batch`/`fetch_last_fetched` this
design replaces), hand-rolling the client-credentials POST using
`common`'s own already-present `reqwest::Client` (0.13) and `serde_json`
— consistent with this codebase's demonstrated taste (`constant_time_eq`,
`parse_cookie`) for hand-rolling something this bounded over adding a
dependency, unlike Decision 2's JWT-verification code, which is real
cryptography and does justify a real dependency. This module owns:

- A small in-memory cache: the last-fetched access token string plus its
  expiry instant, guarded by a mutex (or an async-aware equivalent) —
  because Authentik's access tokens are short-lived (Decision 2's own
  recommended short validity), a poller cannot simply read one static
  config value at startup the way `internal_token: &str` was read before;
  it must fetch, cache, and proactively refresh before expiry.
- A "get a currently-valid token" entry point that returns the cached
  token if it still has comfortable headroom before `exp`, or performs a
  fresh client-credentials POST and re-caches otherwise — called once per
  outbound request in `post_batch`/`fetch_last_fetched`'s successors,
  replacing their current `internal_token: &str` parameter.
- A failed token fetch propagates as a plain `anyhow::Error`, exactly
  matching `post_batch`'s existing `anyhow::bail!` pattern for a failed
  POST — no new error-handling concept invented; see Error handling.

### 5. Wire format: standard `Authorization: Bearer <token>`, replacing the custom `X-Internal-Token` header

The old `X-Internal-Token` header existed only because the value it
carried was a bespoke shared secret, not a real OAuth2 access token. Once
the value *is* a real OAuth2 access token, the standard, RFC 6750-shaped
`Authorization: Bearer <token>` header is the correct wire format — no
reason to keep a custom header name for a now-standard credential type.
`common::ingest::INTERNAL_TOKEN_HEADER` and every `.header(...)` call site
in `post_batch`/`fetch_last_fetched` (Decision 4) and
`require_internal_token`'s successor (`crates/api/src/auth.rs`, not
touched by this design document but the target of a future implementation
plan) switch to reading/writing the standard `Authorization` header.

### 6. Config field shape per service, and where each piece lives

Per real caller (7 today), following Authentik's confirmed credential
shape (Decision 1):

**Each poller/service binary's own config** (`crates/poller-incidents/src/config.rs`,
etc. — replacing the single `internal_token: String` field):

| Field | Shared or per-service | Sensitivity |
|---|---|---|
| `internal_oauth_token_url` | Shared (same value, repeated per binary, like `INTERNAL_TOKEN` was) | Not secret — a public endpoint URL |
| `internal_oauth_client_id` | Shared | Not secret |
| `internal_oauth_scope` | Shared (default e.g. `"groups"`) | Not secret |
| `internal_oauth_username` | **Per-service, distinct** | Identifying, not the actual secret |
| `internal_oauth_password` | **Per-service, distinct** | **The actual secret** |

**`crates/api`'s own new config** (`crates/api/src/data/config.rs`):

| Field | Sensitivity |
|---|---|
| `internal_oauth_issuer_url` | Not secret |
| `internal_oauth_client_id` (expected `aud`) | Not secret |
| `internal_oauth_group_poller_incidents` (…and 6 more, one per real caller) | Not secret |

`api` learns the JWKS endpoint via standard OIDC discovery against
`internal_oauth_issuer_url` — the same discovery mechanism
`crates/api/src/auth/oidc.rs`'s existing `OidcClient` already performs for
the human-login flow (`CoreProviderMetadata::discover_async`, called
before `CoreClient::from_provider_metadata`) — rather than hardcoding
Authentik's own `/application/o/<slug>/jwks/` URL convention directly.
This is a direct, cheap reuse of a pattern already proven to work against
this exact deployment's Authentik instance, not a new mechanism.

**Critically, `api` never holds any of the 7 services' own secret
credentials** — a structural improvement over the reverted design, where
`api` held a copy of every one of the 7 per-service secrets in its own
`InternalServiceRegistry` for lookup (a symmetric shared-secret model:
both `api` and each poller held the same value). Here, only Authentik and
the one poller/service holding its own credential ever see that
credential; `api` only needs Authentik's *public* JWKS and a small,
non-secret table of required group names. The number of places a leaked
credential could originate from is cut roughly in half.

**Chart/compose provisioning, following the `apiKey`-not-`internalToken`
precedent established in Current relevant state**: each of the 7
credentials is *externally assigned* (by Authentik/the operator, not
minted by this app), so — unlike the reverted design's per-service tokens,
which were meant to auto-generate — these follow `pollers.<name>.apiKey`'s
existing posture exactly: **never auto-generated**, and the chart must
require the operator to supply a value or an `existingSecret` reference.
Concretely: `pollers.<name>.oauthUsername`/`oauthPassword`/
`existingSecretOauthUsernameKey`/`existingSecretOauthPasswordKey`,
directly parallel to that same block's existing `apiKey`/`existingSecret`/
`existingSecretApiKeyKey` trio (`values.yaml:159–162`); an analogous pair
of fields on `trustConsumer` and `scheduleFeed.ingest` for the two
non-`pollers.*` services, mirroring the shape the reverted design's own
Task 5 already sketched for those same two services (never implemented,
but the shape transfers cleanly). The shared, non-secret values
(`internal_oauth_token_url`/`client_id`/`scope`, and `api`'s own
`internal_oauth_issuer_url`/expected-`aud`/7 group-name fields) are plain
`values.yaml` scalars, not Secret-backed — following no existing chart
precedent exactly, since this app has no prior non-secret-but-per-service
config surface, but consistent with every other chart convention here
(operator-settable value, sensible suggested default, `existingSecret`
escape hatch only where an actual secret is involved).

`docker-compose.yml`/`dev.env.example` mirror the same field set as env
vars per service (`OAUTH_TOKEN_URL`, `OAUTH_CLIENT_ID`, `OAUTH_SCOPE`
repeated across all 7 service blocks; `OAUTH_USERNAME`/`OAUTH_PASSWORD`
distinct per service), extending `dev.env.example`'s own already-accepted
"duplication cost" convention (its header, lines 7–19 per the reverted
design's own citation, already documents that the same secrets
deliberately live in two files and will drift if only one is updated —
this design adds more named variables to that same accepted pattern, not
a new kind of complexity).

### 7. Relationship to the MCP sibling spec: same architectural idea, independent code paths — not a shared Rust mechanism

Both this spec and the MCP sibling spec land on "Authentik-native groups
are the source of truth for authorization, read off a token DS's own OIDC
client already has a pipeline to obtain." That is a real, shared
architectural conviction, worth stating plainly rather than treating as
coincidence — but they do not, and should not, share Rust code:

- The sibling spec's mechanism is a **human ID token**, verified via
  `openidconnect::core::CoreClient`'s existing browser-login flow, with
  groups persisted to the `users` table and re-read via the already-
  existing `GET /auth/session` endpoint.
- This spec's mechanism is a **machine access token**, verified by a
  wholly new, `CoreClient`-independent JWT verifier (Decision 2), with
  groups checked per-request against a static route table (Decision 3),
  never touching the `users` table or any session concept at all.

The two are related in intent and independent in implementation, matching
how this app already keeps two structurally different "identity ->
allowed actions" mechanisms rather than forcing one shared primitive to
serve unrelated call patterns (the reverted design's own Decision 4 drew
exactly this same kind of line, for a different pair of concerns). If a
future need arises to unify them, that is a decision for whoever proposes
it then, informed by both documents, not inherited by default here.

## Architecture

```
poller-ldbws pod                                        Authentik
┌──────────────────────┐  POST /application/o/token/     ┌──────────────────────────┐
│ common::oauth_client   │  grant_type=client_credentials  │ Service account:          │
│  (new module)          ├─────────────────────────────────►  poller-ldbws-svc         │
│  cached? no/expiring   │  client_id=<shared>              │  member of group:         │
│  -> POST token_url     │  username=poller-ldbws-svc       │   svc-poller-ldbws        │
│     with username/     │  password=<app password>         │                            │
│     password/scope     │  scope=groups                    │ issues signed JWT          │
│                        │◄─────────────────────────────────┤  access_token, containing: │
│  cache {token, exp}    │  { access_token: "<JWT>",         │   sub, iss, aud, exp,      │
└──────────┬─────────────┘    expires_in: 300,               groups: ["svc-poller-ldbws"]│
           │                   token_type: "Bearer" }        └──────────────────────────┘
           │
           │ POST /private/station-samples
           │ Authorization: Bearer <JWT>
           ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ api pod                                                                            │
│  private_router()'s successor to require_internal_token:                          │
│   1. parse Authorization: Bearer header                                           │
│   2. verify JWT signature against cached Authentik JWKS                           │
│      (fetched via OIDC discovery against internal_oauth_issuer_url; refreshed     │
│       on a kid cache-miss or timer -- zero network I/O on the common case)        │
│      -> invalid signature / expired / wrong iss / wrong aud -> 401                │
│   3. read verified `groups` claim                                                 │
│   4. look up required group for request.uri().path() in the static route table    │
│      "/station-samples" -> internal_oauth_group_poller_ldbws ("svc-poller-ldbws") │
│   5. claim's groups contains "svc-poller-ldbws"?                                  │
│        no  -> 403 Forbidden (log sub + path -- diagnosable, not a leak)           │
│        yes -> next.run(request)  -> ingest::router()::post_station_samples()      │
└──────────────────────────────────────────────────────────────────────────────────┘

Wrong-scope example (misconfigured pod, or a compromised poller trying its
luck): poller-tfl's own (validly issued, correctly signed) token, whose
`groups` claim is `["svc-poller-tfl"]`, presented against
`/schedule-feed-ingests` — step 5 above fails ("svc-poller-tfl" is not
"svc-schedule-ingest"), yielding a `403`, not a `401`: the credential is
real, just not for this route.
```

`public_router()` and the `AuthenticatedUser`/session flow
(`crates/api/src/auth.rs`) are entirely untouched by this design, exactly
as they were untouched by the reverted design — this is scoped to
`private_router()` and a new, internal-service-specific verification path
only.

## Error handling

- **Missing/malformed/expired/signature-invalid/wrong-issuer/wrong-audience
  bearer token**: `401 Unauthorized`, indistinguishable from today's only
  failure mode (no credential resolves at all). These are collapsed into
  one outcome deliberately — a caller presenting a token that fails
  verification for any of these reasons learns only "not accepted," never
  which specific check failed, to avoid leaking verification internals to
  a caller that isn't yet a proven-valid identity.
- **Valid signature and claims, but the resolved `groups` claim does not
  contain the route's required group**: `403 Forbidden`, with the token's
  `sub` and the request path logged — the same reasoning the reverted
  design's own Decision 3 gave for departing from this app's usual
  404-for-ownership convention (`crates/api/src/routes/train.rs:9–11`):
  the thing being protected is a fixed, publicly-visible route table, not
  a row whose existence should be hidden, and the caller is a trusted
  internal service holding a real, Authentik-issued credential, not an
  untrusted human prober — a `403` here is actionable signal for a
  misconfigured deployment (the realistic trigger: a chart/secret wiring
  mistake handing one service another's credential), not an information
  leak.
- **Expired token specifically, from the presenting service's own
  perspective**: not an error case at all in the steady state — Decision
  4's token cache proactively refreshes before expiry, so a live poller
  should essentially never present an already-expired token. If one slips
  through anyway (clock skew, an unusually slow request), it is
  indistinguishable from any other verification failure above (`401`),
  and the poller's own existing retry/poll-loop behavior (already the
  pattern for any failed outbound call, per `common::ingest`'s existing
  "log warning, safe fallback" posture in `time_until_next_poll`) recovers
  on the next cycle without any new handling.
- **Revoked service account / rotated password, with an already-issued,
  not-yet-expired token still in a poller's cache**: per Decision 2, local
  JWT verification cannot detect this until the token's own `exp`. Not
  treated as an error case this design resolves — it is the accepted,
  bounded-by-short-token-lifetime tradeoff of choosing local verification
  over introspection, stated plainly rather than silently accepted.
- **Authentik unreachable when a poller/service needs a fresh token**
  (Decision 4's cache is empty or near-expiry, and the token-endpoint POST
  fails): the token-fetch function returns an `anyhow::Error`, propagated
  through `post_batch`/`fetch_last_fetched`'s existing error path exactly
  as a failed POST/GET already is today — no new failure mode invented;
  the poller's existing poll-loop retries on its normal cadence.
- **Authentik unreachable when `crates/api` needs to (re)fetch its JWKS**
  (a `kid` cache-miss triggers a refetch, and that refetch itself fails):
  fails closed — the request being verified is treated as unverifiable and
  rejected (`401`), matching the "fail the request rather than silently
  degrade" posture the MCP sibling spec explicitly adopts for the
  comparable "can't currently verify entitlement" situation (its own
  citation: `values.yaml:299-310`'s existing `api.sso.*` comment
  committing to the same stance elsewhere in this chart).
- **Authentik unreachable at `api`'s own startup**, before any JWKS has
  ever been successfully cached: this design recommends the same lazy,
  first-use posture `crates/api/src/auth/oidc.rs`'s existing `OidcClient`
  already documents for the human-login flow ("discovery is lazy, not
  performed here in `init`") — `api` should not hard-crash-loop forever
  just because Authentik isn't yet reachable at the moment its own pod
  starts (a real, plausible bootstrap-ordering scenario this app already
  tolerates for the same dependency, for the same reason, on the SSO
  path). Every `/private/*` request naturally fails closed (`401`, via the
  point above) until the first successful JWKS fetch succeeds — this app
  simply isn't ingesting data during that window, the same practical
  consequence as today's crash-loop, achieved without one.

## Testing

- **JWT verification, table-driven**: a locally generated RSA keypair
  (test-only, never Authentik's real key) signs fixture tokens covering:
  a valid token with the expected `iss`/`aud`/unexpired `exp` and a
  populated `groups` claim (accept); an expired token (reject, `401`-
  shaped outcome); a token signed with the wrong key (reject); a token
  with the wrong `iss`/`aud` (reject); a valid token whose `groups` claim
  doesn't contain the route's required group (a distinct, `403`-shaped
  outcome, not `401`); a valid token with an *empty* `groups` claim
  (rejected the same as any other non-matching case, never treated as
  "unscoped/allow everything"). Mirrors the reverted design's own
  table-driven scope-enforcement test in spirit, adapted to a claim-driven
  rather than enum-driven check.
- **JWKS caching/refresh**: a `kid` present in the cache verifies without
  any network call (pure, no I/O — test via a fake/mocked JWKS source, not
  a real network hop); a `kid` absent from the cache triggers exactly one
  refetch, and a *still*-absent `kid` after that refetch is rejected
  (guards against an infinite refetch loop on a persistently-unknown
  `kid`, e.g. a token forged with a bogus key ID).
- **Route-scoping table, default-deny**: every real (route, required-group)
  pair from Decision 3's table is exercised — a token whose `groups`
  contains only the matching group passes; a token whose `groups` contains
  every *other* service's group but not this route's required one is
  rejected. A fabricated route present in no row of the table is rejected
  for every possible `groups` claim, guarding against a newly added
  `/private/*` route silently defaulting to "allowed" because nobody added
  its row.
- **`crates/common`'s token cache (Decision 4)**: a cached token with
  comfortable headroom before `exp` is reused, not re-fetched (asserted
  via a call-count on a fake token endpoint); a cached token near its
  `exp` triggers a fresh fetch; a failed fetch (fake endpoint returns an
  error) surfaces as a plain `Err`, not a panic, and does not poison the
  cache with a bad value for the next call.
- **Startup validation**: mirroring `app.rs`'s existing non-empty guards
  for `internal_token`/`sso_client_secret`, an equivalent check that
  `crates/api`'s own new config (issuer URL, expected client_id, the 7
  required-group fields) is non-empty — an empty required-group value must
  not silently become "any group matches," the same failure class the
  current single-token design already guards against for its own
  credential.
- **Real-Authentik-instance verification, noted as implementation-plan-
  level, not designed to depth here**: `charts/distant-signal/files/
  devauthentik-blueprints/oauth2-client.yaml` already provisions this
  chart's dev Authentik instance's human-login OAuth2 client (cited by the
  MCP sibling spec too) — extending that same blueprint file with the
  internal-service Application/Provider, its 7 service accounts, and their
  groups is the natural place a future implementation plan would add
  genuine end-to-end coverage (a poller actually completing a
  client-credentials exchange against a real, locally running Authentik
  and being accepted/rejected by `api` accordingly), not designed further
  here.

## Explicitly out of scope

- **Defining the actual Authentik-side service accounts, the shared
  OAuth2 Provider/Application, the 7 groups, and their memberships.**
  Per the user's own explicit direction, this is operator/deployment-time
  work — this document designs only the DS-side config surface that
  consumes whatever an operator provisions, and gives suggested default
  names/values (Decision 3/6) that are not binding.
- **The exact Authentik group names.** Suggested defaults only
  (`svc-poller-incidents`, etc.), matching the non-binding posture the
  MCP sibling spec already takes for its own two group names.
- **Editing `crates/api/src/auth.rs` or any other source file.** This is a
  design document; no code was written or modified to produce it, per this
  task's own constraint.
- **Retiring or otherwise touching the human SSO/OIDC login flow.**
  `crates/api/src/auth/oidc.rs`, `AuthenticatedUser`, and the session
  cookie flow are untouched — this design is scoped entirely to
  `private_router()`'s internal-service gate.
- **A bounded dual-acceptance migration/rollout window** (the reverted
  design's own Decision 5 designed one, for its own scheme). Whether a
  future implementation plan needs an equivalent transition period
  (accepting both the legacy `X-Internal-Token` and new bearer tokens for
  one release) is a real question but not resolved here — flagged in Open
  questions/risks rather than designed to the reverted document's own
  depth, since the task scoping this document did not ask for a rollout
  section.
- **A DS-owned group-management UI, database table, or any DS-side
  storage of group membership.** Authentik remains the sole place groups
  are created and assigned; `crates/api` only ever reads and checks
  against what a verified token asserts, never stores or edits it.
- **Token introspection as the primary verification mechanism.** Weighed
  and rejected in Decision 2 in favor of local JWT verification; remains
  available as an operator-level tool, not designed into the request path.
- **Changing anything about `schedule-reference`'s route surface**, since
  that crate does not exist in this codebase yet (Current relevant state)
  — this design's table gains one more row when it does, not invented
  ahead of time.
- **The MCP sibling spec's own adapter, tools, or human-groups
  mechanism.** Related in architectural intent (Decision 7), fully
  independent in implementation; nothing in that document is designed,
  extended, or assumed complete by this one.

## Open questions/risks

1. **Whether `openidconnect`'s already-vendored JWK-verification
   primitives can be reused directly for this design's generic JWT
   signature check, versus adding a small, dedicated JWT-verification
   crate.** Not resolved here (Decision 2) — a real implementation-time
   investigation, not a hand-rolling candidate either way.
2. **The exact `aud`/audience claim shape Authentik puts on a
   client-credentials-issued access token** (almost certainly the
   provider's own `client_id`, per how OAuth2 access tokens conventionally
   work, but not confirmed against a real emitted token this session,
   only against documentation prose) — needs confirming against a real
   dev-Authentik-issued token before `crates/api`'s `aud` check is
   implemented.
3. **Whether a given operator's real Authentik instance's `groups` scope
   mapping actually populates reliably** — the same caveat the MCP
   sibling spec already carries for the ID-token case, restated here for
   the access-token case: this design's own Current relevant state
   confirms the *mechanism* exists and applies equally to service
   accounts, but whether a specific deployment has it correctly attached
   is an operator-environment fact neither this document nor the sibling
   one can assert as universal.
4. **The operator must configure an asymmetric Signing Key on the
   internal-service provider** (Decision 2) for local JWKS-based
   verification to work without `api` holding a shared secret — a real,
   load-bearing dependency on Authentik-side provisioning that this
   document flags but, per its own scope, does not design or mandate the
   mechanics of.
5. **No default "Access token validity" number is asserted** (Current
   relevant state) — a future implementation plan (or the operator
   directly) needs to pick a concrete short duration for the
   internal-service provider, trading off Decision 2's revocation-latency
   tradeoff against how often 7 services' worth of token-refresh POSTs
   hit Authentik's token endpoint under a very short setting.
6. **Whether a bounded dual-acceptance rollout window (old
   `X-Internal-Token` and new bearer tokens both accepted for one
   release) is needed**, mirroring the reverted design's own Decision 5
   reasoning about this chart's independently-rolling Deployment objects
   (no ordering guarantee between `api`'s and any poller's own rollout) —
   plausible that the same argument applies here, but not designed to
   that depth in this document (Explicitly out of scope).
7. **`poller-ldbws` remains the one service with two legitimate routes**
   (`/sample-stations` + `/station-samples`) — this design's route table
   already accommodates that (both routes requiring the same single
   `svc-poller-ldbws` group), the same non-novel case the reverted
   design's own Open Question 4 already flagged for its own, structurally
   similar table.
8. **Whether the token-fetch cache (Decision 4) needs cross-process
   coordination** — each poller/service is a single process today (no
   horizontal replica count above 1 confirmed for any of the 7 in this
   chart), so a per-process in-memory cache is sufficient; if any of these
   services is ever scaled to multiple replicas, each replica fetching its
   own token independently is still correct (just N times more token-
   endpoint traffic than one shared cache would produce) — not a
   correctness risk, only a minor efficiency one, not designed around
   further here.
