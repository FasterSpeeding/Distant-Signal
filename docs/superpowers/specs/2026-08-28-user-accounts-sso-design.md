# User Accounts via SSO (OIDC) — Design Sketch

**Status: sketch/proposal only, not an approved design.** Written to the
same rigor as the existing specs in this directory (e.g.
`docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md` and
`docs/superpowers/specs/2026-08-28-train-tracking-design.md`) so it can be
reviewed and iterated on the same way, but it has not gone through
implementation planning and nothing here is committed. It does **not**
contain a task-by-task implementation plan — that is a separate, later
step in this repo's process, done only after a design like this has been
reviewed.

## Problem

This app has no user-facing identity of any kind today.
`crates/api/src/auth.rs` implements exactly one thing: a shared-secret
`X-Internal-Token` header, compared in constant time, gating only
`private_router()` — the internal ingestion routes pollers use. Its own
module doc says so explicitly: "This is intentionally not a general auth
framework — just enough to keep the ingestion endpoints from being
reachable by anyone who can hit the API's port." There is no login flow,
no session, no per-visitor identity anywhere in the codebase.

That absence was a deliberate, explicitly-deferred choice, not an
oversight — and it was already load-bearing on three schema decisions:

- `crates/api/migrations/20260709100000_custom_lines.sql`'s header
  comment: "No owner/user column: unauthenticated for now, by design (see
  that spec's Non-goals) — add ownership in the migration that actually
  adds auth, not speculatively here."
- `crates/api/migrations/20260710090000_preferences.sql`'s header
  comment: "No owner column — unauthenticated for now, same rationale as
  `custom_lines`."
- `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
  Non-goals: "No auth/ownership on custom lines yet. Unauthenticated CRUD,
  consistent with this app's current 'single trusted personal instance'
  model. SSO/multi-user, and gating custom lines per-user, is a known
  future direction, not built here — no placeholder `owner_id` column
  either (add it in the migration that actually adds auth)."

Concretely, today: `crates/api/src/routes/lines.rs`'s custom-line CRUD
(`POST /lines`, `PUT /lines/{id}`, `DELETE /lines/{id}`) and
`crates/api/src/routes/preferences.rs`'s pinned-lines/pinned-stations
endpoints (`GET /preferences`, `PUT /preferences/pinned-lines`,
`PUT /preferences/pinned-stations`) all operate on a single global,
unauthenticated, process-wide set of rows. Anyone who can reach the
Next.js origin can create, edit, or delete any custom line, and any two
browsers share exactly one pinned-lines/pinned-stations set — there is no
per-visitor state at all, only per-deployment state. That is fine for a
single-user personal instance; it stops being fine the moment more than
one person uses the same deployment, which is the premise of this design.

A fourth table is about to make the same choice, freshly: the
train-tracking design
(`docs/superpowers/specs/2026-08-28-train-tracking-design.md`) proposes a
new `tracked_trains` table (not yet implemented — no migration or
implementation plan exists for it as of this writing; the design doc is
the only artifact) whose own "Tracking semantics" section describes
"user-initiated pin only" as the v1 trigger, without a real user model to
attach the pin to, since none existed when that doc was written. This
design's data-model section (below) explicitly folds `tracked_trains` in
so it's born with ownership rather than needing the same retrofit as the
other three tables.

This document designs the "migration that actually adds auth" all four
tables' comments point to: user accounts, authenticated via an external
SSO server, with per-user ownership of custom lines, pinned
lines/stations, and (once it exists) tracked trains.

## Goals

1. Let a visitor authenticate against an SSO server the operator already
   runs or subscribes to, without this app storing or handling passwords,
   or issuing its own credentials.
2. Give every custom line, pinned-lines set, pinned-stations set, and (once
   built) tracked train a real owning user, so multiple people can use one
   deployment without stepping on each other's data.
3. Keep the core product — viewing line status — fully usable with no
   account, unchanged from today.
4. Fit the app's existing conventions: env-only config via `clap`'s `env`
   feature (matching `crates/poller-ldbws/src/config.rs`), Postgres as the
   single source of truth (DESIGN.md §4), and the existing
   `public_router()`/`private_router()` split
   (`crates/api/src/routes/mod.rs`).
5. Define the migration(s) that add `user_id` to the three existing
   unauthenticated tables, including an explicit, safe answer for what
   happens to the rows that exist before this ships.

## Non-goals (this pass)

- **Building or hosting an identity provider.** This app is a relying
  party only. No password storage, no password hashing, no "forgot
  password" flow, no MFA implementation — all of that is the configured
  SSO server's job.
- **Multi-provider / "login with Google or GitHub or..." chooser UI.**
  This design targets exactly one configured OIDC issuer, matching how
  this app already points at exactly one of each external feed (one LDBWS
  base URL, one Knowledgebase endpoint, etc.). See Open Questions for
  whether that should change later.
- **Fine-grained authorization / roles / admin permissions.** "Owns this
  row or doesn't" is the entire authorization model proposed here. No
  admin role, no sharing/collaboration on custom lines between users, no
  organization/team concept.
- **Frontend UI/interaction design.** Sketched only, at the same level
  the train-tracking design sketched its own frontend — a full UI design
  is deferred to its own follow-up doc, per this repo's established
  pattern (`docs/superpowers/specs/2026-07-07-frontend-design.md` did this
  for the original line-status UI; the train-tracking design deferred the
  same way).
- **An implementation plan.** See the status note at the top.
- **Rate limiting / bot/abuse protection on auth routes.** Real concern
  for any public login endpoint, but orthogonal to the account/session
  design itself; flagged in Open Questions, not designed here.

## Research: protocol choice — OIDC over SAML

The ask is "point this app at an existing SSO server" — i.e., this app is
a relying party (RP) to infrastructure the operator already runs, not the
identity provider itself. Two mainstream protocols fit that shape: SAML
2.0 and OpenID Connect (OIDC, built on OAuth 2.0). OIDC is the right fit
here, for reasons specific to what this app actually is:

- **Transport and format match the rest of this app.** Every existing
  integration in this codebase — Knowledgebase, LDBWS, Stations, TOCs,
  TfL — is JSON/REST over HTTP with a bearer/API-key header (see
  `crates/poller-ldbws/src/config.rs`, `crates/api/src/auth.rs`). OIDC is
  the same shape: JSON over HTTPS, bearer tokens, a REST-ish authorization
  and token endpoint. SAML is XML-based, uses signed/encrypted XML
  assertions posted via browser form redirects, and has no natural mapping
  onto this app's existing "small Rust services talking JSON to each
  other" style. Adopting SAML would mean pulling in a materially
  different tooling and mental model (XML canonicalization, XML-DSig)
  for a single feature, when this app's entire existing surface is JSON.
- **Standard discovery removes hand-configuration.** OIDC's
  `.well-known/openid-configuration` document (defined by OpenID Connect
  Discovery 1.0) lets a relying party learn the authorization endpoint,
  token endpoint, JWKS URI, and supported flows from a single issuer URL.
  That maps directly onto this app's existing pattern of "point one env
  var at an external service's base URL and let a typed client do the
  rest" (`ldbws_base_url` in `crates/poller-ldbws/src/config.rs`) — the
  admin supplies an issuer URL, not a hand-assembled list of endpoint
  URLs. SAML has no equivalent single-URL bootstrap; it needs an IdP
  metadata XML document exchanged and configured out of band, which is
  more manual setup for both sides.
- **PKCE gives a public-client-safe code flow with no extra
  infrastructure.** The Authorization Code flow with PKCE (RFC 7636) lets
  a browser-based/public client (this app's Next.js frontend, proxying
  through to `crates/api`) complete the OAuth2/OIDC dance safely without
  needing a client-side secret, by binding the authorization request and
  the token exchange with a locally-generated code verifier/challenge
  pair. This is the OAuth Security BCP / OAuth 2.1 default posture for
  any client that isn't a fully confidential backend able to keep a
  secret. Because this design puts the entire OIDC dance server-side in
  `crates/api` (see Session design below), the app *is* in fact a
  confidential client (it can hold `client_secret`) — but using PKCE
  regardless is cheap defense-in-depth against authorization-code
  interception and is the modern default recommendation regardless of
  client type.
- **ID tokens are self-contained, signed JSON.** An OIDC ID token is a
  JWT: `sub`, `email`, `name`/`preferred_username`, `iss`, `aud`, `exp`,
  signed with a key published at the issuer's JWKS URI. Verifying it is a
  signature check plus a handful of claim comparisons — no XML-DSig
  canonicalization edge cases to get wrong. That verification is exactly
  the kind of "well-vetted library, not hand-rolled" job this app already
  reserves for cases where correctness is genuinely hard to hand-verify
  (contrast `crates/api/src/auth.rs`'s comment on why a single
  constant-time byte comparison is hand-rolled rather than pulling in the
  `subtle` crate — that reasoning cuts the other way here: JWT/JOSE
  signature verification against rotating JWKS keys is not a one-call-site
  job, and is exactly where reusing a mature library earns its
  dependency weight).
- **Refresh tokens and RP-initiated logout are both standardized.** OIDC's
  refresh token grant covers keeping a session alive past the ID token's
  short expiry without re-prompting the user. **OpenID Connect
  RP-Initiated Logout 1.0** (a finalized OpenID Foundation spec) defines
  exactly the flow this app needs for "log out everywhere," not just
  locally: redirect the browser to the issuer's discovered
  `end_session_endpoint` with an `id_token_hint` and
  `post_logout_redirect_uri`, so the SSO server also ends its own
  browser session, not only this app's local cookie. SAML has a
  conceptually similar Single Logout profile, but it's XML/SOAP-shaped and
  substantially more implementation-heavy for the same outcome.

**Conclusion: OIDC (Authorization Code + PKCE), not SAML.** Everything
about this app's existing shape — JSON APIs, single-issuer external
integrations configured by URL + credentials via env vars, a Rust/axum
backend — lines up with OIDC's tooling and mental model. SAML would be a
correct-but-foreign choice with no offsetting benefit for "point this app
at one SSO server."

## Research: Rust crate landscape (checked 2026-08-28, via crates.io API
## and GitHub, not from training-data recall)

| Crate | Role | Latest stable | Released | Notes |
|---|---|---|---|---|
| [`openidconnect`](https://crates.io/crates/openidconnect) | OIDC relying-party client: discovery, auth-code+PKCE flow helpers, ID token verification, UserInfo, refresh | 4.0.1 | 2025-07-06 (crates.io); most recent commit on `ramosbugs/openidconnect-rs` is 2025-11-08 | No new crates.io release in ~13 months as of this writing, but the GitHub repo has commits within the last ~4 months — actively maintained, just not releasing every point-fix immediately. Passes the OpenID Foundation's RP conformance test suite for the `response_type=code` profile per its own docs. Built directly on `oauth2`. |
| [`oauth2`](https://crates.io/crates/oauth2) | Underlying OAuth2 client (authorization code grant, PKCE, token exchange, refresh) that `openidconnect` wraps | 5.0.0 | 2025-01-21 | Same maintainer lineage as `openidconnect`; extensible, strongly-typed. 47M+ all-time downloads — the de facto standard OAuth2 client crate in the Rust ecosystem. |
| [`tower-sessions`](https://crates.io/crates/tower-sessions) | Generic session middleware for `tower`/`axum`: `Session` extractor, pluggable `SessionStore` | 0.15.0 | 2026-02-01 | Actively released (6 months old as of this doc). Successor to the deprecated `axum-sessions`. Provides *storage*, not authentication — an app still owns "what goes in the session." |
| [`axum-login`](https://crates.io/crates/axum-login) | Authn/authz framework built **on top of** `tower-sessions` (not a competitor to it): `AuthSession` extractor, `AuthnBackend`/`AuthzBackend` traits, route-protection middleware, permission/role support | 0.18.0 | 2025-07-20 | Adds a general-purpose user/permission abstraction (arbitrary backends: DB, LDAP, external IdP) this app doesn't need — see reasoning below. |
| `tower-sessions-sqlx-store` / `tower-sessions-redis-store` | Persistent `SessionStore` backends for `tower-sessions` | 0.15.0 / 0.16.0 | both 2025-01-01 | Both roughly 13 months stale relative to `tower-sessions` core's most recent release — a real, if minor, maintenance-lag risk if `tower-sessions` ships a breaking `SessionStore` trait change before these catch up. |
| `axum-oidc` / `axum-oidc-client` / `axum-oidc-layer` | Various third-party axum-specific OIDC middleware wrapping `openidconnect` | varies | varies (`axum-oidc-client` updated as recently as March 2026 per its own docs) | Smaller download counts / newer, less-established projects than `openidconnect`+`oauth2` themselves; `axum-oidc` is LGPLv3-licensed, which is a real consideration if this repo's licensing posture matters (not checked here — see Open Questions). |

**Recommendation: `openidconnect` (+ its `oauth2` dependency) for the
OIDC protocol layer, hand-rolled axum route handlers around it (not a
third-party `axum-oidc*` wrapper), and a self-owned Postgres-backed
session — not `tower-sessions`/`axum-login`.** Reasoning:

- The protocol layer (discovery, PKCE, ID-token JWT/JWKS verification,
  refresh, RP-initiated logout) is exactly where a mature, conformance-
  tested library earns its dependency weight, per the reasoning above.
  `openidconnect` is that library: highest download count of the RP-
  focused options found, backed by the same maintainer as `oauth2`, and
  its GitHub activity (Nov 2025 commit) shows it isn't abandoned even
  though its crates.io cadence is slow.
- The third-party axum-specific wrappers (`axum-oidc`, `axum-oidc-client`,
  `axum-oidc-layer`) trade a small amount of boilerplate for a real
  dependency-freshness/trust discount (smaller communities, one is
  LGPLv3, one wasn't found with a crates.io listing showing sustained
  activity). Given this app already writes its own thin axum handlers for
  everything else (there is no precedent anywhere in this codebase for
  reaching for a framework-specific wrapper crate over composing a
  narrower, well-vetted library directly), a handful of handler functions
  in a new `crates/api/src/auth/oidc.rs` calling `openidconnect` directly
  is more consistent with the codebase's existing style than adopting
  another crate's opinions about route shape.
- For *session storage specifically*, this app doesn't need
  `tower-sessions`'/`axum-login`'s generality. `axum-login`'s value
  proposition — pluggable `AuthnBackend`s (DB, LDAP, arbitrary), role/
  permission authorization — is aimed at apps with multiple authentication
  backends or fine-grained permission models. This design has exactly one
  authentication backend (the configured OIDC issuer) and exactly one
  authorization rule ("do you own this row"), so most of what
  `axum-login` provides goes unused. `tower-sessions` alone (without
  `axum-login`) is a closer fit, but its persistent-store backends
  (`tower-sessions-sqlx-store`, `tower-sessions-redis-store`) are the
  most dependency-stale pieces found in this whole survey — over a year
  behind the core crate's own release cadence — for a feature (opaque
  session-id → row lookup) that is genuinely simple to own directly.
  This mirrors the reasoning `crates/api/src/auth.rs` already gives for
  hand-rolling `constant_time_eq` instead of pulling in `subtle` for a
  single call site: pull a mature library where the logic is genuinely
  hard to get right (JWT/JWKS verification — hence `openidconnect`), and
  keep a thin, boring, self-owned implementation where it's genuinely
  simple (session-id lookup against a Postgres table this app already
  operates) rather than adding a dependency whose own transitive store
  crates are trailing its core release.
- Concretely: a `sessions` table (schema below), an opaque
  high-entropy random session id set as an `HttpOnly`/`Secure`/
  `SameSite=Lax` cookie, and one `axum::middleware::from_fn_with_state`
  extractor that looks the id up and attaches the resolved `User` to the
  request — the same shape as `auth::require_internal_token` already
  uses for the internal-token gate, just keyed by a DB lookup instead of
  a constant-time string compare.

## Session architecture: owned entirely by `crates/api`

The frontend proxy at `frontend/app/api/[...path]/route.ts` is the
deciding fact here. It's a same-origin catch-all (`GET`/`POST`/`PUT`/
`DELETE`) that forwards `/api/*` browser requests to
`${API_BASE_URL}/public/*` server-side, specifically because browser
JavaScript can't see `API_BASE_URL` (a server-only env var) and CORS on
the `api` service is otherwise avoided entirely. Two things about its
current implementation matter directly for session design:

- **The browser never talks to `crates/api` directly.** Every write
  (pin a line, create a custom line, and — after this design —
  login/logout) already goes browser → Next.js → `api`. A session cookie
  set by `crates/api` and sconed to the Next.js origin flows through this
  proxy transparently to the browser, the same way any other cookie
  would, with zero CORS complications, *provided the proxy forwards
  `Cookie` on the way in and `Set-Cookie` on the way out* — which it
  **does not currently do**. Reading `proxy()` in full: `init` only sets
  `method` and a hardcoded `Content-Type` header, never copying
  `req.headers` (so no `Cookie` reaches `api`), and the response is
  reconstructed with only `Content-Type` copied back (so no `Set-Cookie`
  reaches the browser). **This is a required, small change to the proxy**
  as part of this feature — not optional plumbing — covered in Auth
  routes below.
- **Next.js has no server-side identity of its own to protect.** It's a
  thin proxy with no database access and no business logic beyond path-
  scope validation. There's nothing for a Next.js-owned auth layer to
  *do* except duplicate what the proxy already does one layer up.

Given that, **the entire OIDC relying-party dance (auth-code+PKCE
exchange, ID token validation, refresh) and the resulting session cookie
are owned by `crates/api`.** The frontend adds no auth logic at all — no
NextAuth.js, no separate frontend session store, no token handling in
Next.js server components. It only needs the proxy fix above (forward
`Cookie` in both directions) plus UI that calls `/api/auth/login`,
reads "am I logged in" from a `/api/auth/session` (or equivalent) JSON
response, and calls `/api/auth/logout`.

Why not split it (e.g. NextAuth.js in the frontend, API trusts a token
Next.js hands it)? Two reasons specific to this app, not a generic
"backend should own auth" platitude:

1. **It would duplicate state across two services for no benefit.** A
   split design needs either (a) Next.js owns the session and forwards a
   bearer token to `crates/api` on every request — which reintroduces
   exactly the CORS/credential-plumbing problem the existing proxy was
   built to avoid, since now two services independently need to trust and
   validate tokens — or (b) both services independently understand OIDC,
   which is strictly more surface area than one. Given the proxy already
   makes the browser same-origin-only to Next.js, routing the *entire*
   session lifecycle through the one component (`api`) that actually owns
   the data being protected (`custom_lines`, `pinned_lines`,
   `pinned_stations`, `tracked_trains`) is simpler, not just "backend-
   owns-it by convention."
2. **`crates/api` is already the component with a database.** Session
   validity, revocation-on-logout, and refresh-token storage all need
   persistent state. `crates/api` already has the Postgres pool
   (`AppState.database` in `crates/api/src/app.rs`); Next.js has none.
   Putting the session store where the database connection already lives
   avoids inventing a second persistence path.

### Cookie shape

- Name: `nr_session` (placeholder, bikeshed later).
- Value: an opaque, high-entropy random token (e.g. 256 bits, base64url-
  encoded) — a lookup key into the `sessions` table, not a JWT and not
  itself carrying claims. Keeping it opaque means revocation (logout,
  admin-forced sign-out) is a single `DELETE` and never requires client
  cooperation, unlike a self-contained signed token that stays "valid"
  until its own `exp` regardless of server-side state.
- Attributes: `HttpOnly` (no JS access — the frontend doesn't need to read
  it, only forward it), `Secure` (HTTPS-only), `SameSite=Lax` (top-level
  navigation from the OIDC provider's redirect must still carry it; `Lax`
  permits that while blocking cross-site POST forgery — `Strict` would
  break the callback redirect), `Path=/`.
- Scoped to whatever the Next.js origin is (the domain the browser
  actually visits) — issued by `crates/api` but the `Set-Cookie` travels
  back through the proxy to the browser, so it lands as same-origin from
  the browser's perspective, matching every other cookie this app might
  ever set.

### Where it's issued and validated

- **Issued** by `crates/api`'s `/auth/callback` handler, after
  successfully exchanging the authorization code (see Auth routes below)
  and creating a `sessions` row.
- **Validated** by a new axum middleware (`crates/api/src/auth.rs`,
  alongside the existing `require_internal_token`) that reads the
  `nr_session` cookie, looks up the `sessions` row, checks
  `expires_at > now()`, and attaches the resolved user (or `None`) to
  request extensions. Applied to whichever routes need "acting as a
  known user" — most of `public_router()` stays reachable with no session
  at all (see Anonymous posture below); only the ownership-scoped
  endpoints require a resolved user, returning `401` otherwise.

### Flow through the Next.js proxy

1. Browser → Next.js `/api/auth/login` → proxied to `api`'s
   `/public/auth/login` (or, if login must not be spoofable via the
   `/public/*`-only proxy rule, a dedicated non-proxied Next.js redirect —
   see Auth routes below for why `/auth/*` likely needs special-casing in
   the proxy rather than going through the generic `/public/*` rule).
2. `api` redirects the browser to the SSO server's authorization endpoint
   (discovered via `.well-known/openid-configuration`), with a PKCE
   challenge and `state`/`nonce` stored server-side (short-lived row or
   signed cookie — implementation-level choice, not decided here).
3. SSO server authenticates the user, redirects the browser back to
   `api`'s `/auth/callback` — **this must be a direct browser redirect to
   the `api` origin (or a proxied path Next.js forwards unmodified),
   not something the SSO server can be pointed at Next.js for and have
   Next.js silently rewrite** — the exact registered redirect URI is an
   implementation detail to pin down, but it must be stable and
   pre-registered with the SSO server either way.
4. `api`'s callback handler exchanges the code (+ PKCE verifier) for
   tokens, validates the ID token, creates the `users` row (if new) or
   updates it (if returning), creates a `sessions` row, sets the
   `nr_session` cookie, and redirects the browser back to the frontend's
   original page.
5. Every subsequent browser request to Next.js already includes the
   cookie (browser-native behavior); Next.js's proxy must forward it
   (`Cookie` header in, `Set-Cookie` header out) to `api` — the fix noted
   above.
6. **Logout**: browser → Next.js `/api/auth/logout` → `api` deletes the
   `sessions` row, clears the cookie (`Set-Cookie` with `Max-Age=0`), and
   — for a true RP-initiated logout that also ends the SSO server's own
   browser session — redirects to the discovered `end_session_endpoint`
   with `id_token_hint` and `post_logout_redirect_uri` back to this app,
   per OpenID Connect RP-Initiated Logout 1.0. A "local-only" logout
   (just clear this app's cookie, leave the SSO server's session alone)
   is a simpler fallback if the configured issuer doesn't advertise
   `end_session_endpoint` — `openidconnect`'s discovery document makes
   that presence/absence checkable at runtime.

### Expiry and refresh

- `sessions.expires_at` bounds the local session (proposal: a sliding
  window, e.g. 14 days, refreshed on activity — exact figure is a product
  decision, not researched here).
- The OIDC refresh token (if the provider issues one — not guaranteed for
  every issuer/scope combination) is stored server-side only (never sent
  to the browser) and used to silently renew the ID token/access token
  before the local session's own expiry, so a long browser session
  doesn't force a re-login purely because the *ID token's* short `exp`
  (typically minutes) passed. If no refresh token is available or the
  refresh fails (revoked at the IdP, expired), the local session is
  deleted and the next request gets a `401`, forcing a fresh login.

## Data model

### New table: `users`

Only the claims this app actually has a use for — display and per-row
ownership — not a general profile store:

| column | type | notes |
|---|---|---|
| `id` | `TEXT PRIMARY KEY` | the OIDC `sub` claim, stored verbatim. Not a surrogate UUID — matches this repo's existing convention of natural-key `TEXT` primary keys (`incidents.incident_id`, `custom_lines.id`, `stations.crs`) rather than introducing a new `uuid` dependency (not currently in `crates/api/Cargo.toml`'s `sqlx` feature list) for a value that's already a stable, unique, string identifier. Safe under the single-issuer assumption this design makes (see Open Questions for what changes if that assumption is ever dropped). |
| `email` | `TEXT` | from the ID token's `email` claim, nullable — not every issuer/scope guarantees it. **Trust caveat**: only trust this if the ID token also asserts `email_verified: true`; store the raw value but treat unverified email as display-only, not as a unique/lookup key (see Open Questions). |
| `name` | `TEXT` | from `name` (or `preferred_username` if `name` is absent), nullable, display-only. |
| `created_at` | `TIMESTAMPTZ NOT NULL DEFAULT NOW()` | first-login timestamp. |
| `last_login_at` | `TIMESTAMPTZ NOT NULL DEFAULT NOW()` | updated on every successful callback; upserted, not just inserted once. |

Deliberately **not** stored: no password/credential material (this app
never sees one), no full IdP profile blob, no phone number/address/other
claims beyond the three above unless a real feature need for one shows
up. This follows the same restraint DESIGN.md already applies elsewhere
(e.g. §5.5's `dataQuality` field exists because it's used, not because
"more data is generally nice to have").

### New table: `sessions`

| column | type | notes |
|---|---|---|
| `id` | `TEXT PRIMARY KEY` | the opaque cookie value (or a hash of it — hashing the stored value, comparable to how a password hash works, means a DB read alone can't be replayed as a live session cookie; a real implementation-level call, flagged here not decided). |
| `user_id` | `TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE` | |
| `refresh_token` | `TEXT` | the OIDC refresh token, if issued. Never sent to the browser — server-side only, used to silently renew. Should be encrypted at rest if this app ever adds an at-rest encryption story; flagged, not designed here (no existing precedent for encrypting a column anywhere in this schema today). |
| `created_at` | `TIMESTAMPTZ NOT NULL DEFAULT NOW()` | |
| `expires_at` | `TIMESTAMPTZ NOT NULL` | |

### Ownership: adding `user_id` to the three existing tables

Two migrations, sorting after the latest existing one
(`crates/api/migrations/20260822120000_line_status_source.sql` is the most
recent as of this writing — the new files must timestamp-sort after it,
e.g. `20260828130000_user_accounts.sql` for `users`/`sessions`, then
`20260828140000_add_ownership.sql` for the three `ALTER TABLE`s below).

**`custom_lines`**: add `user_id TEXT REFERENCES users(id) ON DELETE
CASCADE`, nullable initially (see existing-rows handling below), with an
index for per-user listing.

**`pinned_lines`** / **`pinned_stations`**: these need more than an added
column. Today's schema
(`crates/api/migrations/20260710090000_preferences.sql`) has `line_id
TEXT PRIMARY KEY` / `crs CHAR(3) PRIMARY KEY` — i.e. **one global row per
pinned line/station**, full stop. Once ownership exists, the same line
must be pinnable independently by many users, so the primary key itself
has to change: add `user_id`, drop the old single-column primary key, add
a new composite `PRIMARY KEY (user_id, line_id)` /
`PRIMARY KEY (user_id, crs)`. This is a real schema-shape change, not
just an added nullable column — call it out explicitly in the migration
so a reviewer isn't surprised by a dropped constraint.

**What happens to existing unowned rows** (the real migration-safety
question, not hand-waved):

- **`custom_lines`**: existing rows keep `user_id = NULL` — no owner to
  attribute them to, since they predate any user model. `NULL` is
  distinct from "owned by nobody, therefore public" as an access rule: a
  `NULL`-owned row is either (a) treated as a **read-only legacy/orphaned
  line** — visible to everyone (matching today's fully-public behavior,
  so nothing currently working suddenly 404s), but not editable/deletable
  by anyone until an operator manually assigns an owner (a one-off
  `UPDATE custom_lines SET user_id = '<admin sub>' WHERE user_id IS
  NULL`, run once post-deploy by whoever operates the instance), or (b)
  migrated eagerly to a designated "system"/first-admin user at deploy
  time. **(a) is the safer default**: it doesn't silently hand
  ownership/delete rights over pre-existing data to whichever user
  happens to be first through the new login flow, and it doesn't destroy
  data (nothing is deleted or hidden). This needs an explicit operator
  runbook note (not just a migration) for whoever runs an existing
  deployment through this upgrade, since it's a manual follow-up step,
  not something the migration itself can decide (it doesn't know which
  user *should* own pre-existing custom lines — only a human operator
  does).
- **`pinned_lines` / `pinned_stations`**: same "distinct rows, no owner
  to attribute them to" problem, but the PK change makes the "leave
  `user_id` NULL" option structurally awkward (a composite PK with a NULL
  column member is legal in Postgres but every unowned row would
  effectively be its own group, since `NULL <> NULL` — they wouldn't
  collide with each other, but they also wouldn't collide with any real
  user's pins, meaning they'd become permanently invisible to every
  actual account without ever being cleaned up). Given these two tables
  are pure UI convenience state (unlike `custom_lines`, which is
  user-authored content with real value), the safer, simpler answer is:
  **do not carry pre-existing pinned-lines/pinned-stations rows forward
  at all.** Snapshot them (e.g. `SELECT` into a one-off backup table or
  just log them) for the operator's own reference if wanted, then
  `TRUNCATE` both tables as part of the same migration that adds the
  composite PK. Every user starts with an empty pinned set post-migration
  and re-pins, which is a one-time, low-cost inconvenience for a
  "single trusted personal instance"-sized deployment (DESIGN.md's own
  framing), and avoids the NULL-orphan-forever problem entirely. This
  should be called out prominently in the migration file's comment,
  matching this repo's convention of explaining non-obvious *why* in
  migration headers (see the existing `custom_lines.sql`/
  `preferences.sql` comments this whole feature descends from).

**`tracked_trains`**: per the train-tracking design's own framing
("a user marks it as tracked," written before any user model existed),
and since — per the check performed for this document — **neither a
migration nor an implementation plan for `tracked_trains` exists yet as
of this writing** (only the design doc), this table should be defined
**with `user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE`
from birth**, whenever it's actually created. There is no retrofit
question for this one — it simply shouldn't ship without ownership,
avoiding the exact situation this whole document exists to clean up for
the other three tables. If `tracked_trains`' migration lands before this
design is implemented, its author should either sequence it after this
document's `users` migration, or add the `NOT NULL` `user_id` column
directly at creation time referencing a `users` table stubbed in by
whichever of the two lands first — a sequencing detail for whoever
implements first, not a design ambiguity.

## Auth routes

New routes on `crates/api`, added as a new `crates/api/src/routes/auth.rs`
module, listed in `crates/api/src/routes/mod.rs`:

- `GET /auth/login` — redirects to the SSO server's authorization
  endpoint (builds the PKCE challenge, stores `state`/`nonce`/verifier
  server-side keyed to a short-lived cookie or DB row).
- `GET /auth/callback` — the OIDC redirect target; exchanges the code,
  validates the ID token, upserts the `users` row, creates a `sessions`
  row, sets the cookie, redirects to the originating page.
- `POST /auth/logout` — deletes the `sessions` row, clears the cookie,
  optionally redirects through the SSO server's `end_session_endpoint`
  (RP-initiated logout).
- `GET /auth/session` — returns the current user's `{id, email, name}` (or
  `204`/`{authenticated: false}`) for the frontend to render "logged in as
  X" vs. a login button. Read-only, safe to be unauthenticated-callable
  (it just reports "no session" rather than erroring).

**Where these mount, relative to `public_router()`/`private_router()`**:
neither existing router is quite right. `private_router()`
(`crates/api/src/routes/mod.rs`) is gated by the internal-token middleware
meant for *pollers*, not browsers — wrong fit entirely. `public_router()`
is unauthenticated by design and is what the Next.js proxy forwards
`/api/*` → `/public/*` onto (`frontend/app/api/[...path]/route.ts`) — the
closest fit, since these routes genuinely are meant to be reachable by
any visitor's browser, but two things need care:

1. **The proxy's path-scope check.** The proxy already restricts itself
   to `target.pathname.startsWith('/public/')`, so mounting auth routes
   under `/public/auth/*` and having Next.js's `/api/auth/*` forward to
   them works with zero proxy-route changes beyond the `Cookie`/
   `Set-Cookie` forwarding fix already called out above. This is the
   recommended mounting point — no new proxy special-casing needed if
   `/public/auth/*` is used.
2. **The proxy's current header handling drops cookies entirely** (see
   Session design above) — this is a required fix regardless of exactly
   where the routes mount: `proxy()`'s `init` must forward the incoming
   `Cookie` header, and the returned `NextResponse` must forward the
   upstream `Set-Cookie` header(s) (note: a response can carry *multiple*
   `Set-Cookie` headers, which `Headers.get()` collapses to one — the fix
   needs to iterate `response.headers` for all `Set-Cookie` entries, e.g.
   via `getSetCookie()`, not a single `.get('Set-Cookie')` call).
3. A new `require_session` (or `optional_session`) middleware in
   `crates/api/src/auth.rs`, applied per-route (not globally on
   `public_router()`, since most of it stays anonymous-accessible — see
   below) to the specific handlers that need a resolved user: custom-line
   mutations, pinned-lines/pinned-stations reads and writes, and (once it
   exists) tracked-train endpoints.

## Authz for existing endpoints

Every current handler in `crates/api/src/routes/preferences.rs` and
`crates/api/src/routes/lines.rs` operates on the single global row set —
concretely:

- `preferences::list_pinned_line_ids`/`list_pinned_station_crs` (no
  `WHERE` clause at all — `crates/api/src/data/preferences.rs`) and
  `replace_pinned_lines`/`replace_pinned_stations` (`DELETE FROM
  pinned_lines` with no predicate, then re-insert) — a full-table
  replace, today.
- `custom_lines::list_custom_lines`/`get_custom_line`/
  `insert_custom_line`/`update_custom_line`/`delete_custom_line`
  (`crates/api/src/data/custom_lines.rs`) — no ownership predicate
  anywhere; any caller can edit or delete any custom line by id.

Once `user_id` exists, every one of these needs a `WHERE user_id = $n`
(reads) or `... AND user_id = $n` (mutations) added:

- `GET /preferences`, `PUT /preferences/pinned-lines`,
  `PUT /preferences/pinned-stations` all require a resolved session
  (`require_session`), and every query scopes to `session.user.id` —
  `replace_pinned_lines`/`replace_pinned_stations` become
  `DELETE FROM pinned_lines WHERE user_id = $1` (not the whole table)
  before re-inserting.
- `POST /lines` (create) stamps the new row's `user_id` from the session.
- `PUT /lines/{id}` / `DELETE /lines/{id}` add `AND user_id = $2` to the
  `UPDATE`/`DELETE`, and must distinguish "not found" from "found, but
  not yours" in the response — recommend both return `404` (not `403`)
  to avoid confirming a given custom-line id exists to a non-owner probing
  ids, consistent with `get_line`'s existing comment about why it doesn't
  special-case catalogue-vs-custom-line 404s differently.
- `GET /lines` (list) and `GET /lines/{id}` (read) **stay unauthenticated
  and unscoped** — custom lines are status-computation input the
  aggregator treats the same as catalogue lines (per the original
  `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`),
  and their *status* is public product content, not private data. Only
  *authoring* (create/edit/delete) and *the pinned-lines/pinned-stations
  preference lists* are user-private. This mirrors a real distinction
  already implicit in the current design: a custom line's computed status
  feeds `/Line/.../Status` the same as any catalogue line once created —
  making it visible only to its creator would be a product regression,
  not a security improvement. (Whether *listing whose* custom line it is
  should be exposed is a separate, smaller question — not addressed here,
  since `LineSummary` today has no owner field to leak in the first
  place.)
- `GET /lines/{id}/definition` — unchanged, read-only, same reasoning as
  `GET /lines/{id}`.

## Anonymous / no-account posture

The core product — viewing line status — must keep working exactly as it
does today for a visitor with no account, full stop. Concretely, these
stay fully open, no session required:

- All of `line_status::router()` (`/Line/...`, `/StopPoint/.../Disruption`)
  — DESIGN.md §1's entire reason for existing.
- `GET /lines`, `GET /lines/{id}`, `GET /lines/{id}/definition` — reading
  any line's status/definition, catalogue or custom.
- `freshness::router()`, `reference::router()`, `health::router()` — all
  already-public informational endpoints.

Gated behind a resolved session (returns `401` with no session, per the
Authz section above):

- Creating, editing, deleting a custom line.
- Reading or writing your own pinned-lines/pinned-stations preferences.
- (Once built) creating/reading tracked trains.

This split — "read the product, no account needed; personalize/author
something, account needed" — is the same shape TfL's own site and most
comparable products use, and requires no new concept beyond "does this
handler need `require_session` or not."

## Frontend changes (sketch only — full design deferred)

Following this repo's own convention of deferring UI design to its own
doc (`docs/superpowers/specs/2026-07-07-frontend-design.md` for the
original line-status UI; the train-tracking design deferred its frontend
the same way), only sketched here:

- A user-menu / login button in `frontend/app/layout.tsx`'s existing nav
  `Group` (`frontend/app/layout.tsx`, alongside `ThemeToggle`,
  `PrideToggle`, `DataFreshnessInfo`) — calls `GET /api/auth/session` to
  decide whether to render "Log in" (linking to `/api/auth/login`) or a
  user menu with name/email and "Log out" (`POST /api/auth/logout`).
- Custom-line create/edit forms and the pinned-lines/pinned-stations UI
  need to handle a `401` (prompt login) where they previously always
  succeeded against the global unauthenticated endpoints.
- No token handling, no client-side OIDC library, no NextAuth.js — the
  frontend only ever talks to its own same-origin `/api/*` proxy, per the
  Session design section above.
- A full design pass (empty states, session-expiry UX, what a logged-out
  visitor sees on a custom-lines page they can view-but-not-edit, etc.) is
  its own follow-up document, not designed here.

## Open questions / risks

1. **Single issuer only, for v1.** This design assumes one configured
   OIDC issuer (one `SSO_ISSUER_URL`/`SSO_CLIENT_ID`/`SSO_CLIENT_SECRET`
   env triple, matching `crates/poller-ldbws/src/config.rs`'s pattern).
   `users.id` being the bare `sub` claim (no issuer qualifier) is safe
   only under that assumption — supporting multiple simultaneous issuers
   later would need a composite `(issuer, sub)` identity instead, which is
   a breaking schema change if deferred too long. Worth deciding now
   whether multi-provider is a real near-term need or genuinely out of
   scope; this design assumes the latter based on the "point this app at
   an existing SSO server" framing in the ask, but that's an assumption,
   not a confirmed requirement.
2. **Email trust/verification.** Storing `email` from the ID token without
   checking `email_verified` risks an IdP that allows unverified emails
   to let one user claim an address they don't control. Recommend
   surfacing `email_verified` as its own stored boolean (not folded into
   trusting `email` blindly) and never using email as a lookup/dedup key
   — `sub` already is the identity key, so this is a display-only
   concern, but worth being explicit about rather than silently assuming
   every configured issuer verifies email before asserting the claim.
3. **Multi-tenancy is not addressed.** This design is "many users, one
   deployment, one shared line-status product, per-user private
   preferences/authored content." It does not address multiple isolated
   *tenants* (e.g. separate organizations each wanting their own private
   custom-line catalogue) — out of scope unless a real requirement
   surfaces.
4. **Session store hashing.** Whether `sessions.id` (the DB column) stores
   the raw cookie value or a hash of it is flagged but not decided here —
   hashing is the more defensible default (a DB dump/leak doesn't hand out
   live session cookies) but adds a small amount of implementation
   complexity (hash-on-lookup) not resolved in this pass.
5. **Refresh-token-at-rest encryption.** `sessions.refresh_token` is
   sensitive (a live credential against the SSO server) and this schema
   stores it in plaintext, matching this repo's current lack of any
   column-level-encryption precedent anywhere else. Worth a follow-up
   decision on whether that's acceptable for this app's threat model or
   needs its own small design pass.
6. **Rate limiting / abuse protection on `/auth/*`.** Not addressed — a
   public login/callback endpoint is a real target for abuse
   (credential-stuffing-adjacent traffic, even though this app never sees
   a password itself, the callback/token-exchange path still costs a
   round trip to the SSO server per attempt). Flagged, not designed.
7. **Operator runbook for the `custom_lines.user_id IS NULL` backfill.**
   The Data model section proposes leaving pre-existing custom lines
   ownerless (readable, not editable) rather than auto-assigning an
   owner. This needs an actual runbook step for existing deployments
   upgrading through this migration, not just a schema note — who
   actually runs the one-off `UPDATE`, and when, is an operational
   decision for whoever deploys this, not something the migration can
   resolve unilaterally.
8. **RP-initiated logout support isn't universal.** Not every OIDC
   provider implements `end_session_endpoint` / RP-Initiated Logout 1.0.
   The design above handles this by checking discovery for its presence
   and falling back to local-only logout, but that fallback means "log
   out" won't always end the *SSO server's* browser session too — worth
   surfacing to the operator when configuring against a given issuer,
   not just silently degrading.
9. **`tracked_trains` sequencing.** As noted in the Data model section,
   if the train-tracking feature's migration is written before this
   design is implemented, its author needs to either wait on the `users`
   table or coordinate creation order — a real cross-feature sequencing
   risk given both are live design threads as of this writing, not a
   flaw in either individual design.
