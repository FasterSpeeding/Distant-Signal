# User Accounts via SSO (OIDC) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Execution-order dependency with another in-flight plan:** This plan's
> Task 1 migration (`crates/api/migrations/20260828090000_user_accounts.sql`,
> creating `users`) **must apply before**
> `docs/superpowers/plans/2026-08-28-train-tracking.md`'s Task 1 migration
> (`crates/api/migrations/20260828120000_train_tracking.sql`, creating
> `tracked_trains`) — that plan's `tracked_trains.user_id` column now
> references `users(id)`, per the coordination fix applied to that file
> alongside this plan's authoring. The timestamp prefixes already sort
> correctly (`20260828090000` / `20260828100000` < `20260828120000`); this
> note exists so neither plan is executed out of order by two workers
> unaware of the other. See that file's own matching note at its top, and
> this plan's Global Constraints below.

**Goal:** Let a visitor authenticate against an operator-configured OIDC SSO
server (Authorization Code + PKCE), give every custom line, pinned-lines
set, pinned-stations set a real owning user, and keep the core line-status
product fully usable with no account — per
`docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`.

**Architecture:** `crates/api` becomes the sole OIDC relying party and the
sole owner of session state — the Next.js frontend adds no auth logic of
its own, only a proxy fix so cookies and redirects survive the existing
`/api/*` → `/public/*` forwarding path. Three new Postgres tables
(`users`, `sessions`, `oidc_login_state`) back an opaque, `HttpOnly`/
`Secure`/`SameSite=Lax` session cookie (`nr_session`); a fourth migration
retrofits `user_id` onto the three tables that predate any user model
(`custom_lines` nullable, `pinned_lines`/`pinned_stations` truncated and
rebuilt with a composite primary key). The OIDC protocol layer
(discovery, PKCE, ID-token verification) is handled entirely by the
`openidconnect`/`oauth2` crates — nothing here hand-rolls JWT/JWKS
verification; a hand-rolled Postgres session store is used instead of
`tower-sessions`/`axum-login`, per the design doc's crate-landscape
research. OIDC discovery is performed lazily (on first `/auth/login` or
`/auth/callback` request, cached after success), not at `AppState::init`
time, so a misconfigured or briefly-unreachable SSO server cannot crash
the whole `api` service and take line-status display down with it —
mirroring `AppState.redis`'s existing laziness (`crates/api/src/app.rs`)
for the same reason.

**Tech Stack:** Rust/axum/sqlx (`crates/api`), PostgreSQL, `openidconnect`
+ `oauth2` (new dependencies), `reqwest` (new dependency, the OIDC HTTP
client), `sha2`/`rand`/`base64` (new direct dependencies of `crates/api`;
already resolved transitively in `Cargo.lock` via `crates/enricher`, so no
new major-version surprises expected), Next.js/TypeScript (one file:
`frontend/app/api/[...path]/route.ts`).

**Spec:** `docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`
— read in full before starting; this plan does not restate its research,
only its resulting decisions, and resolves several things the design doc
left as open implementation-level choices (flagged inline below wherever
this plan makes such a call).

## Prerequisites this plan assumes exist before Task 11 (final verification) can fully run

Every task's code can be written and unit-tested without any of the
following — the OIDC protocol layer is exercised against a hand-rolled
`RawClaims` fixture (Task 4), never a real or fake-signed token (see Task
4's Step 1 for why). But real end-to-end login only works once a human has,
out of band:

1. **Registered this app as an OIDC client with a real SSO server** (any
   standards-compliant IdP the operator runs or subscribes to — Keycloak,
   Authentik, Auth0, Okta, etc.), obtaining an issuer URL, client id, and
   client secret.
2. **Registered the exact redirect URI** this plan's config points at
   (`SSO_REDIRECT_URL`, Task 3) with that same SSO server — it must be the
   frontend's own public origin plus `/api/auth/callback` (see Task 3's
   doc comment for why, not the `api` service's own origin).
3. Confirmed whether that SSO server's discovery document is reachable
   from wherever `crates/api` actually runs (network egress, TLS trust,
   etc.) — the same class of "does this service have a network path to an
   external dependency" concern this codebase already has for
   `LLM_BASE_URL`/RDM endpoints.

No CI test in this plan depends on any of the above (see Task 4's testing
decision). Task 11's manual verification step is the one place a real IdP
is needed.

## Global Constraints

- **Config is env-only via `clap`'s `env` feature**, matching every
  existing crate (`crates/poller-ldbws/src/config.rs`). `SSO_CLIENT_SECRET`
  is a genuinely new *kind* of secret for `crates/api` — every existing
  credential here (`internal_token`, RDM API keys) is a single shared/
  bearer token, not a paired OAuth2 confidential-client secret — but is
  handled with the same posture: required, no default, never logged (see
  Task 3).
- **Migration ordering.** Timestamp-prefixed SQL under
  `crates/api/migrations/`; the latest existing file as of this writing is
  `20260822120000_line_status_source.sql`. This plan's two migrations are
  `20260828090000_user_accounts.sql` (Task 1) and
  `20260828100000_add_ownership.sql` (Task 2) — both timestamped to sort
  strictly before `docs/superpowers/plans/2026-08-28-train-tracking.md`'s
  `20260828120000_train_tracking.sql`, per the cross-plan dependency noted
  at the top of this document.
- **`users.id` is the bare OIDC `sub` claim**, stored verbatim as a
  natural-key `TEXT PRIMARY KEY` — matches this schema's existing
  convention (`incidents.incident_id`, `custom_lines.id`, `stations.crs`)
  rather than adding a `uuid` dependency. Safe only under this design's
  single-issuer assumption (design doc Open Question 1); out of scope to
  change here.
- **OIDC protocol layer: `openidconnect` + `oauth2` directly, no
  third-party axum wrapper (`axum-oidc`/etc.), no `tower-sessions`/
  `axum-login`.** Session storage is a hand-rolled `sessions` Postgres
  table. See the design doc's full crate-landscape research for why; this
  plan does not re-litigate it.
- **OIDC discovery is lazy, not eager.** `AppState::init` validates the
  *syntactic* shape of `SSO_ISSUER_URL`/`SSO_REDIRECT_URL` (fails fast on
  a typo) but never makes a network call to the issuer at startup. The
  actual `.well-known/openid-configuration` fetch + JWKS discovery happens
  once, on first use, memoized in a `tokio::sync::OnceCell` (Task 4). This
  is a deliberate deviation from a naive reading of the design doc's
  "issued by `crates/api`'s `/auth/callback` handler" framing — the design
  doc doesn't actually specify *when* discovery happens, and this plan
  resolves that gap in favor of the same robustness `AppState.redis`
  already has: an external dependency being briefly down must not be able
  to take the entire `api` service (including unrelated line-status
  routes) down with it, which a blocking startup call would risk.
- **Session id is stored hashed, not raw** (`sessions.id` = SHA-256 hex of
  the actual cookie value). Resolves design doc Open Question 4 in favor
  of its own stated "more defensible default": a DB dump/leak alone can't
  be replayed as a live session cookie, mirroring how a password hash
  works.
- **Cookie names are fixed constants** (`nr_session` for the real session,
  `nr_login` for the short-lived OIDC login-state cookie), not
  configurable — same posture as `crates/enricher`'s hardcoded Redis
  stream/consumer-group names.
- **`email` is only ever persisted when the ID token also asserts
  `email_verified: true`.** An unverified claim is silently dropped, never
  stored-but-flagged — resolves design doc Open Question 2. No separate
  `email_verified` column: since a false/absent claim already means
  `email` itself is never written, there's no present need to distinguish
  "verified" from "no claim at all" after the fact; add the column later
  if a real feature need for that distinction shows up (same restraint
  DESIGN.md already applies elsewhere, per the design doc's own framing).
- **No "return to originating page" round-trip.** `/auth/callback` and
  `/auth/logout` always redirect to one fixed, configured URL
  (`SSO_POST_LOGIN_REDIRECT_URL`), not back to whatever page the user was
  on. A v1 scope simplification, consistent with the design doc's own
  Frontend section already being sketch-only.
- **No RP-Initiated Logout in this pass.** The design doc describes
  redirecting through the IdP's own `end_session_endpoint` on logout
  (OpenID Connect RP-Initiated Logout 1.0) so the SSO server's own browser
  session also ends, with a documented fallback to "local-only" logout
  when a provider doesn't advertise that endpoint (design doc Open
  Question 8 — support isn't universal). **This plan narrows that to
  local-only logout for v1**: `/auth/logout` deletes the `sessions` row
  and clears the cookie, full stop. Full RP-initiated logout is real,
  useful, and left as a documented follow-up — not implemented here,
  matching the train-tracking plan's own precedent of narrowing a design
  doc's stated goal to a shippable v1 slice with the gap called out
  explicitly (see that plan's Global Constraints on CIF SCHEDULE).
- **Authz model: "owns this row or doesn't," nothing more** — no roles, no
  admin, no sharing. `GET /lines`, `GET /lines/{id}`,
  `GET /lines/{id}/definition`, and everything under `line_status::router()`
  stay fully public and unauthenticated, unchanged — per the design doc's
  Anonymous/no-account posture and Authz-for-existing-endpoints sections.
  Only custom-line *authoring* (create/edit/delete) and the pinned-lines/
  pinned-stations preference endpoints require a resolved session.
- **A rejected/expired custom-line or pinned-preference write returns
  `404`, never `403`, for "exists but not yours."** Matches `update_line`'s
  existing comment on why catalogue-vs-custom-line 404s aren't
  distinguished, and the design doc's explicit recommendation against
  confirming a row's existence to a non-owner.
- **No new crate.** Everything in this plan lives in `crates/api` (plus one
  frontend file) — there is no `poller`-shaped or long-running-consumer
  component to this feature, unlike `crates/enricher`/`trust-consumer`.
- **Testing convention for the OIDC exchange.** This codebase's one
  existing DB-backed test pattern (`#[ignore]`d, requires `DATABASE_URL`,
  see `crates/api/src/data/queries.rs::tfl_line_summaries_lists_only_tfl_owned_rows`)
  is reused for session storage. The OIDC protocol exchange itself
  (discovery, PKCE, ID-token signature/claims verification) is **not**
  tested against a fake or real IdP in this plan — Task 4 factors the one
  piece of this app's *own* logic (mapping verified claims onto the
  `users` row shape) into a pure, unit-tested function fed by a
  hand-constructed fixture, and treats protocol conformance as already
  covered by `openidconnect`'s own upstream test suite (the design doc
  cites it as passing the OpenID Foundation's RP conformance suite) —
  hand-rolling a fake signed-JWT IdP here would duplicate that coverage
  for dubious extra confidence, at real risk of the plan's own test
  fixture code being subtly wrong. Real end-to-end login against a real
  configured IdP is Task 11's manual verification step, matching this
  repo's existing precedent for `LLM_BASE_URL`-dependent tests in
  `crates/enricher/src/llm.rs` (`#[ignore = "requires network access to a
  real LLM_BASE_URL..."]`).
- New dependencies introduced by this plan (all in `crates/api`):
  `openidconnect`, `oauth2`, `reqwest`, `sha2`, `rand`, `base64`. No new
  dev-dependencies (see the testing-convention bullet above).

---

### Task 1: `users`, `sessions`, `oidc_login_state` schema migration

**Files:**
- Create: `crates/api/migrations/20260828090000_user_accounts.sql`

**Interfaces:**
- Produces: `users`, `sessions`, `oidc_login_state` tables. Consumed by
  Task 5 (`data/users.rs` queries), Task 2 (`custom_lines`/`pinned_lines`/
  `pinned_stations` FK to `users`), and, once it lands,
  `docs/superpowers/plans/2026-08-28-train-tracking.md`'s Task 1
  (`tracked_trains.user_id REFERENCES users(id)`).

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260828090000_user_accounts.sql`:

```sql
-- -------------------------------------------------------------------------
-- User accounts via OIDC SSO: `users` and `sessions`, plus a short-lived
-- `oidc_login_state` table bridging GET /auth/login -> GET /auth/callback.
-- See docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Data
-- model / Session architecture sections and
-- docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 1/4/5.
--
-- IMPORTANT: this migration must apply before
-- crates/api/migrations/20260828120000_train_tracking.sql
-- (docs/superpowers/plans/2026-08-28-train-tracking.md's Task 1) --
-- tracked_trains.user_id references users(id). This file's timestamp
-- prefix (20260828090000) already sorts earlier; preserve that ordering if
-- either file is ever renamed. See the note at the top of both plan
-- documents.
--
-- users.id is the OIDC `sub` claim, stored verbatim -- a natural-key TEXT
-- primary key, matching this schema's existing convention
-- (incidents.incident_id, custom_lines.id, stations.crs) rather than
-- adding a uuid dependency for a value that's already a stable unique
-- string. Safe only under this design's single-issuer assumption (design
-- doc Open Question 1).
--
-- email is only ever written by crates/api/src/data/users.rs's
-- upsert_user when the ID token also asserted email_verified: true (design
-- doc Open Question 2) -- not enforced by this schema, enforced at the
-- application layer. No separate email_verified column: a dropped
-- (never-written) email already carries that signal for this app's
-- current needs.
-- -------------------------------------------------------------------------

CREATE TABLE users (
    id             TEXT        PRIMARY KEY,
    email          TEXT,
    name           TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- sessions.id stores a SHA-256 hex digest of the opaque cookie value, not
-- the raw token -- see crates/api/src/auth.rs's `hash_session_token` doc
-- comment (Task 6): a DB dump/leak alone then can't be replayed as a live
-- session cookie, the same property a password hash gives you.
CREATE TABLE sessions (
    id             TEXT        PRIMARY KEY,
    user_id        TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token  TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at     TIMESTAMPTZ NOT NULL
);

CREATE INDEX sessions_user_id ON sessions (user_id);

-- Short-lived, single-use rows bridging /auth/login -> /auth/callback:
-- the PKCE verifier, nonce, and CSRF state token generated at login time
-- must survive the round trip to the SSO server and back, without relying
-- on the not-yet-issued real session cookie. `id` is set as a separate
-- short-lived `nr_login` cookie (Task 7). Rows older than 15 minutes are
-- swept opportunistically by crates/api/src/data/users.rs's
-- `insert_login_state` -- no cron needed for a table this small and
-- self-limiting.
CREATE TABLE oidc_login_state (
    id             TEXT        PRIMARY KEY,
    pkce_verifier  TEXT        NOT NULL,
    nonce          TEXT        NOT NULL,
    csrf_state     TEXT        NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 2: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS — three new, unreferenced tables; no existing query touches
them.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260828090000_user_accounts.sql
git commit -m "Add users, sessions, oidc_login_state tables"
```

---

### Task 2: Ownership retrofit — `custom_lines`, `pinned_lines`, `pinned_stations`

**Files:**
- Create: `crates/api/migrations/20260828100000_add_ownership.sql`

**Interfaces:**
- Produces: `custom_lines.user_id` (nullable), `pinned_lines`/
  `pinned_stations` rebuilt with `user_id NOT NULL` and a composite primary
  key. Consumed by Task 9 (`custom_lines` queries/routes), Task 10
  (`preferences` queries/routes).

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260828100000_add_ownership.sql`:

```sql
-- -------------------------------------------------------------------------
-- Ownership retrofit for the three tables that predate any user model. See
-- docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Data
-- model section for the full reasoning -- this comment restates the
-- conclusions only. Depends on the `users` table from
-- 20260828090000_user_accounts.sql, which must run first.
--
-- custom_lines: user_id is added NULLABLE. Existing rows keep
-- user_id = NULL -- there is no owner to attribute them to. NULL is
-- deliberately NOT "public, owned by nobody" for write access: a
-- NULL-owned row stays readable (GET /lines, GET /lines/{id} are
-- unauthenticated and unscoped either way -- nothing currently working
-- 404s) but is not editable or deletable by anyone
-- (crates/api/src/data/custom_lines.rs's update_custom_line/
-- delete_custom_line now filter `AND user_id = $n`, which a NULL user_id
-- can never match) until an operator manually assigns a real owner.
--
-- OPERATOR RUNBOOK (existing deployments with pre-existing custom_lines
-- rows only -- not needed on a fresh install): after this migration runs,
-- decide who should own any pre-existing custom lines and run, once, by
-- hand:
--   UPDATE custom_lines SET user_id = '<admin sub>' WHERE user_id IS NULL;
-- This migration deliberately does not do this automatically -- it has no
-- way to know which user *should* own pre-existing data, only a human
-- operator does. Leaving rows ownerless is safe (read-only until that
-- manual step happens), never destructive.
--
-- pinned_lines / pinned_stations: unlike custom_lines, these need more
-- than an added column -- today's schema has ONE GLOBAL ROW per pinned
-- line/station (line_id / crs as the sole PRIMARY KEY). Once ownership
-- exists, the same line must be independently pinnable by many users, so
-- the primary key itself changes to a composite (user_id, line_id) /
-- (user_id, crs). A NULL user_id can't carry existing rows forward
-- through that change (NULL <> NULL under a composite PK means every
-- unowned row would be a permanently-invisible group of its own, visible
-- to no real account, ever) -- so instead of a NULL-owner retrofit,
-- existing rows are intentionally NOT carried forward. They're pure UI
-- convenience state (unlike custom_lines' authored content, which IS
-- carried forward), so this TRUNCATEs both tables as part of adding the
-- composite PK. Every user starts with an empty pinned set post-migration
-- and re-pins -- a one-time, low-cost inconvenience for this app's
-- "single trusted personal instance"-sized deployments (DESIGN.md), not a
-- data-loss concern.
-- -------------------------------------------------------------------------

ALTER TABLE custom_lines
    ADD COLUMN user_id TEXT REFERENCES users(id) ON DELETE CASCADE;

CREATE INDEX custom_lines_user_id ON custom_lines (user_id) WHERE user_id IS NOT NULL;

-- See header comment: pre-existing rows are deliberately not preserved.
TRUNCATE TABLE pinned_lines;
ALTER TABLE pinned_lines
    ADD COLUMN user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    DROP CONSTRAINT pinned_lines_pkey,
    ADD PRIMARY KEY (user_id, line_id);

TRUNCATE TABLE pinned_stations;
ALTER TABLE pinned_stations
    ADD COLUMN user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    DROP CONSTRAINT pinned_stations_pkey,
    ADD PRIMARY KEY (user_id, crs);
```

- [ ] **Step 2: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS. (Tasks 9/10 are what actually update the Rust queries to
match this new shape — until then, existing queries against
`pinned_lines`/`pinned_stations` that don't bind `user_id` will fail at
*runtime* against a real database, not at compile time, since this repo
uses runtime-checked `sqlx::query`, not `query!`. That's expected and is
exactly why Tasks 9/10 exist; don't skip straight to Task 3 in a real
deployment sequence — implementation order within this plan doesn't have
to match task order strictly, but Task 2's migration and Tasks 9/10's
query updates must land together in any real rollout.)

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260828100000_add_ownership.sql
git commit -m "Retrofit user_id ownership onto custom_lines, pinned_lines, pinned_stations"
```

---

### Task 3: OIDC dependencies and config

**Files:**
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/data/config.rs`

**Interfaces:**
- Produces: `ServiceArguments` gains `sso_issuer_url`, `sso_client_id`,
  `sso_client_secret`, `sso_redirect_url`, `sso_post_login_redirect_url`,
  `session_ttl_days`. Consumed by Task 4 (`OidcClient::new`), Task 7
  (`AppState::init` validation + wiring).

- [ ] **Step 1: Add the new dependencies**

```bash
cd crates/api
cargo add openidconnect@4.0
cargo add oauth2@5.0
cargo add reqwest@0.13 --no-default-features --features json,native-tls,gzip
cargo add sha2@0.10
cargo add rand@0.8
cargo add base64@0.22
cd ../..
```

Expected: `crates/api/Cargo.toml` gains six new dependency lines;
`Cargo.lock` updates (network access required for this step — `sha2`/
`rand`/`base64` are already transitively resolved in the lockfile via
`crates/enricher`, so those three should resolve without pulling new major
versions; `openidconnect`/`oauth2`/`reqwest` are genuinely new to the
workspace at the version pinned by the design doc's crate-landscape
research, `openidconnect` 4.0.1 / `oauth2` 5.0.0 as of that research pass —
confirm nothing newer/breaking has shipped since). If `cargo add
openidconnect@4.0` doesn't pull in `reqwest`-based HTTP client support
automatically, check whether a `reqwest` feature flag needs enabling
explicitly (`cargo add openidconnect@4.0 --features reqwest` — confirm the
exact feature name against `openidconnect` 4.0's docs.rs at implementation
time; this plan's Task 4 assumes `openidconnect`'s types work directly
against a caller-supplied `reqwest::Client`, per `oauth2` 5.0's
trait-based async HTTP client design).

- [ ] **Step 2: Add the SSO config fields**

In `crates/api/src/data/config.rs`, add to `ServiceArguments` (alongside
the existing `internal_token` field):

```rust
    /// OIDC issuer base URL (e.g. `https://sso.example.com/realms/rail`).
    /// `crates/api` discovers every other endpoint (authorization, token,
    /// JWKS) from this single URL's `.well-known/openid-configuration`
    /// document -- see the design doc's OIDC-over-SAML research for why.
    /// No default: every deployment must point this at its own
    /// operator-run/subscribed SSO server. Discovery itself is lazy (see
    /// this plan's Global Constraints) -- this field is only syntactically
    /// validated at startup, not dereferenced over the network.
    #[arg(long, env)]
    pub sso_issuer_url: String,

    /// OIDC client id this app is registered as with the issuer above.
    #[arg(long, env)]
    pub sso_client_id: String,

    /// OIDC client secret paired with `sso_client_id`. A genuinely new
    /// *kind* of secret for this crate -- every other credential here
    /// (`internal_token`, the RDM API keys in sibling pollers) is a single
    /// shared/bearer token, not a paired OAuth2 confidential-client secret
    /// -- but handled with the same posture: env-only, required, never
    /// logged. `ServiceArguments` derives `Debug`; avoid ever logging
    /// `app.config` wholesale (nothing in this codebase does today) --
    /// log individual non-secret fields instead if a future debug log
    /// needs to reference config.
    #[arg(long, env)]
    pub sso_client_secret: String,

    /// The exact redirect URI registered with the SSO server for the
    /// authorization-code callback. Deliberately NOT this service's own
    /// origin -- it must be the *frontend's* public origin plus
    /// `/api/auth/callback` (e.g.
    /// `https://rail.example.com/api/auth/callback`), proxied through to
    /// this crate's `/public/auth/callback` by
    /// `frontend/app/api/[...path]/route.ts` (Task 8). If this pointed at
    /// `crates/api`'s own origin instead, the `Set-Cookie` the callback
    /// handler issues would be scoped to `api`'s origin, not the origin
    /// the browser subsequently talks to for every other request -- the
    /// session cookie would never come back. See the design doc's Session
    /// architecture section.
    #[arg(long, env)]
    pub sso_redirect_url: String,

    /// Where `/auth/callback` and `/auth/logout` send the browser once
    /// they're done -- the frontend's own root URL (e.g.
    /// `https://rail.example.com/`). One fixed target, not a round-tripped
    /// "return to this page" value -- a v1 scope simplification (see this
    /// plan's Global Constraints).
    #[arg(long, env)]
    pub sso_post_login_redirect_url: String,

    /// Sliding-window session lifetime in days. Design doc proposes 14 as
    /// a starting figure, not researched further there; kept configurable
    /// since it's a product/ops tuning knob, not a protocol constant.
    #[arg(long, env, default_value_t = 14)]
    pub session_ttl_days: i64,
```

- [ ] **Step 3: Confirm the crate builds**

Run: `cargo build -p api`
Expected: PASS — these fields are unused until Task 7, which will produce
an "unused field" situation only if `ServiceArguments` itself isn't
referenced elsewhere fully; in practice `clap::Parser`'s derive uses every
field for argument parsing regardless, so no dead-code warning is expected
here.

- [ ] **Step 4: Commit**

```bash
git add crates/api/Cargo.toml crates/api/Cargo.lock crates/api/src/data/config.rs
git commit -m "Add OIDC dependencies and SSO config fields"
```

---

### Task 4: OIDC relying-party client wrapper

**Files:**
- Create: `crates/api/src/auth/oidc.rs`
- Modify: `crates/api/src/auth.rs` (add `pub mod oidc;`)

**Interfaces:**
- Produces: `struct OidcConfig { issuer_url, client_id, client_secret, redirect_url }`;
  `struct OidcClient` with `OidcClient::new(config: OidcConfig) -> Result<Self>`,
  `async fn authorize_url(&self) -> Result<(Url, PkceCodeVerifier, CsrfToken, Nonce)>`,
  `async fn exchange_code(&self, code: String, pkce_verifier: PkceCodeVerifier, expected_nonce: &Nonce) -> Result<(OidcIdentity, Option<String>)>`;
  `struct RawClaims { sub, email, email_verified, name }`;
  `fn identity_from_claims(claims: RawClaims) -> OidcIdentity` (pure,
  unit-tested); `struct OidcIdentity { sub, email, email_verified, name }`.
  Consumed by Task 5 (`data/users.rs::upsert_user` takes `&OidcIdentity`),
  Task 7 (`routes/auth.rs` calls `authorize_url`/`exchange_code`).

- [ ] **Step 1: Write the failing tests for `identity_from_claims`**

Create `crates/api/src/auth/oidc.rs` with just the types, the pure
function, and its tests first:

```rust
//! OIDC relying-party client: lazy discovery, PKCE authorization-code
//! flow, and ID-token claim mapping. Wraps `openidconnect`/`oauth2`
//! directly -- see
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's
//! crate-landscape research for why no third-party axum-oidc wrapper is
//! used instead.

/// The claims this app actually reads out of a verified ID token and
/// persists -- see the design doc's `users` table section for why nothing
/// beyond these four is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct OidcIdentity {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
}

/// The four raw claim values pulled off a verified `openidconnect`
/// `CoreIdTokenClaims`, immediately after signature/issuer/audience/nonce
/// verification (which is `openidconnect`'s job -- see this plan's Global
/// Constraints on why that surface isn't re-tested here). This
/// indirection exists so `identity_from_claims` -- the one piece of this
/// app's *own* logic in the whole OIDC exchange -- is testable against a
/// plain, hand-constructed fixture, without needing a real or fake-signed
/// ID token to build one.
#[derive(Debug, Clone)]
pub struct RawClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}

/// Maps raw claims onto the subset this app persists. A missing/absent
/// `email_verified` claim defaults to `false` (never trust silence as
/// verification) -- see design doc Open Question 2.
pub fn identity_from_claims(claims: RawClaims) -> OidcIdentity {
    OidcIdentity {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
        name: claims.name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(email_verified: Option<bool>) -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
        }
    }

    #[test]
    fn sub_and_name_pass_through_unconditionally() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.sub, "user-123");
        assert_eq!(identity.name, Some("Ada Rider".to_string()));
    }

    #[test]
    fn verified_email_is_kept() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.email, Some("rider@example.com".to_string()));
        assert!(identity.email_verified);
    }

    #[test]
    fn unverified_email_claim_still_flows_through_here_unfiltered() {
        // identity_from_claims itself doesn't drop the email on
        // email_verified: false -- that gating happens one layer up, in
        // data::users::upsert_user (Task 5), which is the actual
        // enforcement point per design doc Open Question 2. This function
        // only maps and defaults; asserting that split explicitly here
        // documents where the real decision lives.
        let identity = identity_from_claims(claims(Some(false)));
        assert_eq!(identity.email, Some("rider@example.com".to_string()));
        assert!(!identity.email_verified);
    }

    #[test]
    fn missing_email_verified_claim_defaults_to_unverified() {
        let identity = identity_from_claims(claims(None));
        assert!(!identity.email_verified);
    }
}
```

- [ ] **Step 2: Declare the new module**

In `crates/api/src/auth.rs`, add `pub mod oidc;` near the top of the file
(alongside the existing module-level doc comment, above
`require_internal_token`).

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p api identity_from_claims`
Expected: PASS — pure function, implementation and tests written together
(same posture the incident-extraction plan's `text_changed`/`combine`
steps used).

- [ ] **Step 4: Add the lazy-discovery client**

Add to `crates/api/src/auth/oidc.rs`, above the `#[cfg(test)]` block:

```rust
use anyhow::{Context, Result};
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreProviderMetadata, CoreResponseType,
};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use openidconnect::url::Url;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

/// The OIDC relying-party client. Discovery is deliberately lazy -- see
/// this plan's Global Constraints -- so constructing this value can never
/// fail on a briefly-unreachable issuer; only `IssuerUrl`/`RedirectUrl`
/// syntax is validated eagerly, in `new`.
pub struct OidcClient {
    config: OidcConfig,
    http_client: reqwest::Client,
    inner: tokio::sync::OnceCell<CoreClient>,
}

impl OidcClient {
    pub fn new(config: OidcConfig) -> Result<Self> {
        // Validate URL syntax now (fail fast on a typo'd env var) without
        // making a network call -- the real discovery fetch is deferred
        // to `client()`, below.
        IssuerUrl::new(config.issuer_url.clone()).context("invalid SSO_ISSUER_URL")?;
        RedirectUrl::new(config.redirect_url.clone()).context("invalid SSO_REDIRECT_URL")?;

        // `redirect(Policy::none())`: `openidconnect`/`oauth2`'s HTTP
        // client contract requires the caller supply a client that does
        // NOT auto-follow redirects, per the crates' own SSRF-hardening
        // guidance -- an HTTP client that transparently follows redirects
        // could be tricked by a malicious/compromised endpoint into
        // fetching an unintended internal URL during discovery or token
        // exchange. Confirm this requirement's exact wording against
        // `openidconnect`/`oauth2` 4.0/5.0's own docs at implementation
        // time; this reflects the crates' documented posture as of the
        // design doc's research pass.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build OIDC HTTP client")?;

        Ok(Self { config, http_client, inner: tokio::sync::OnceCell::new() })
    }

    /// Performs OIDC discovery on first use only, then caches the result
    /// for the process lifetime. Deliberately NOT done in `new`/at
    /// `AppState::init` time -- see this plan's Global Constraints.
    async fn client(&self) -> Result<&CoreClient> {
        self.inner
            .get_or_try_init(|| async {
                let issuer = IssuerUrl::new(self.config.issuer_url.clone())?;
                let metadata = CoreProviderMetadata::discover_async(issuer, &self.http_client)
                    .await
                    .context("OIDC discovery failed")?;
                let client = CoreClient::from_provider_metadata(
                    metadata,
                    ClientId::new(self.config.client_id.clone()),
                    Some(ClientSecret::new(self.config.client_secret.clone())),
                )
                .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);
                Ok::<_, anyhow::Error>(client)
            })
            .await
    }

    /// Builds the browser-redirect URL for `GET /auth/login`, plus the
    /// three values that must be round-tripped to the callback (stored
    /// server-side -- see `data::users::insert_login_state`, Task 5): the
    /// PKCE verifier, the CSRF state token, and the nonce.
    pub async fn authorize_url(&self) -> Result<(Url, PkceCodeVerifier, CsrfToken, Nonce)> {
        let client = self.client().await?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, csrf_state, nonce) = client
            .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        Ok((url, pkce_verifier, csrf_state, nonce))
    }

    /// Exchanges the authorization code for tokens, verifies the ID
    /// token's signature/issuer/audience/nonce/expiry (`openidconnect`'s
    /// job, not re-implemented here), extracts the four claims this app
    /// cares about into `RawClaims`, and maps them through
    /// `identity_from_claims`. Also returns the refresh token, if the
    /// provider issued one (not guaranteed).
    pub async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: PkceCodeVerifier,
        expected_nonce: &Nonce,
    ) -> Result<(OidcIdentity, Option<String>)> {
        let client = self.client().await?;

        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .context("failed to build code exchange request")?
            .set_pkce_verifier(pkce_verifier)
            .request_async(&self.http_client)
            .await
            .context("token exchange failed")?;

        let id_token = token_response
            .extra_fields()
            .id_token()
            .context("token response had no id_token")?;
        let claims = id_token
            .claims(&client.id_token_verifier(), expected_nonce)
            .context("id token verification failed")?;

        let raw = RawClaims {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().to_string()),
            email_verified: claims.email_verified(),
            name: claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string()),
        };
        let refresh_token = token_response.refresh_token().map(|t| t.secret().clone());

        Ok((identity_from_claims(raw), refresh_token))
    }
}
```

Note: the exact accessor/builder method names above
(`CoreAuthenticationFlow::AuthorizationCode`, `.authorize_url(...)`,
`token_response.extra_fields().id_token()`, `claims.subject().as_str()`,
`claims.name().and_then(|n| n.get(None))`, etc.) match `openidconnect`
4.0's documented Authorization-Code-flow example as of the design doc's
research pass, but this plan was written without the ability to
compile-check against the crate directly — confirm each against `cargo doc
-p openidconnect --open` while implementing this step, and adjust names
that have drifted. The *shape* (lazy `OnceCell`-memoized discovery, PKCE
challenge/verifier pair, nonce-checked claims, extraction into `RawClaims`
before touching this app's own types) is the part this plan is actually
prescribing; exact method names are a compile-time detail to true up.

- [ ] **Step 5: Confirm the crate builds**

Run: `cargo build -p api`
Expected: PASS once any method-name drift from Step 4's note is corrected.
`OidcClient` is not yet constructed anywhere (that's Task 7) — an "unused"
warning for the whole module is expected and harmless until then.

- [ ] **Step 6: Run the full API crate test suite**

Run: `cargo test -p api`
Expected: PASS, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/auth/oidc.rs crates/api/src/auth.rs
git commit -m "Add lazy-discovery OIDC relying-party client"
```

---

### Task 5: `users`/`sessions`/`oidc_login_state` data layer

**Files:**
- Create: `crates/api/src/data/users.rs`
- Modify: `crates/api/src/data/mod.rs` (add `pub mod users;`)

**Interfaces:**
- Consumes: `OidcIdentity` (Task 4).
- Produces: `struct User { id, email, name }`;
  `async fn upsert_user(pool, identity: &OidcIdentity) -> Result<User>`;
  `struct SessionUser { id, email, name }`;
  `async fn insert_session(pool, hashed_token, user_id, refresh_token, ttl_days) -> Result<()>`;
  `async fn get_session_with_user(pool, hashed_token) -> Result<Option<SessionUser>>`;
  `async fn delete_session(pool, hashed_token) -> Result<()>`;
  `struct LoginState { pkce_verifier, nonce, csrf_state }`;
  `async fn insert_login_state(pool, id, pkce_verifier, nonce, csrf_state) -> Result<()>`;
  `async fn consume_login_state(pool, id) -> Result<Option<LoginState>>`;
  `fn verified_email(identity: &OidcIdentity) -> Option<&str>` (pure,
  unit-tested). Consumed by Task 6 (`AuthenticatedUser` extractor calls
  `get_session_with_user`), Task 7 (routes call the rest).

- [ ] **Step 1: Write the failing test for `verified_email`**

Create `crates/api/src/data/users.rs`:

```rust
//! Queries for `users`/`sessions`/`oidc_login_state` -- the tables Task
//! 1's migration creates. See
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Data
//! model section.

use anyhow::Result;
use sqlx::PgPool;

use crate::auth::oidc::OidcIdentity;

/// Only ever `Some` when the ID token asserted `email_verified: true` --
/// the actual enforcement point for design doc Open Question 2 (see
/// `crates/api/src/auth/oidc.rs`'s `identity_from_claims` doc comment,
/// which maps the claim through unfiltered; this is where it's filtered).
fn verified_email(identity: &OidcIdentity) -> Option<&str> {
    identity.email_verified.then_some(identity.email.as_deref()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(email_verified: bool) -> OidcIdentity {
        OidcIdentity {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
        }
    }

    #[test]
    fn verified_email_is_kept() {
        assert_eq!(verified_email(&identity(true)), Some("rider@example.com"));
    }

    #[test]
    fn unverified_email_is_dropped() {
        assert_eq!(verified_email(&identity(false)), None);
    }

    #[test]
    fn no_email_claim_at_all_is_none_regardless_of_verified_flag() {
        let mut i = identity(true);
        i.email = None;
        assert_eq!(verified_email(&i), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p api verified_email`
Expected: PASS.

- [ ] **Step 3: Wire `users` into `data/mod.rs`**

In `crates/api/src/data/mod.rs`, add `pub mod users;` alongside the
existing `pub mod custom_lines;`/`pub mod preferences;`.

- [ ] **Step 4: Add the query functions**

Add to `crates/api/src/data/users.rs`:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Creates the user on first login, or updates `email`/`name`/
/// `last_login_at` on every return visit -- design doc: "upserted, not
/// just inserted once."
pub async fn upsert_user(pool: &PgPool, identity: &OidcIdentity) -> Result<User> {
    let email = verified_email(identity);
    let row = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, name, created_at, last_login_at) \
         VALUES ($1, $2, $3, NOW(), NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
            email = EXCLUDED.email, name = EXCLUDED.name, last_login_at = NOW() \
         RETURNING id, email, name",
    )
    .bind(&identity.sub)
    .bind(email)
    .bind(&identity.name)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn insert_session(
    pool: &PgPool,
    hashed_token: &str,
    user_id: &str,
    refresh_token: Option<&str>,
    ttl_days: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
         VALUES ($1, $2, $3, NOW(), NOW() + make_interval(days => $4))",
    )
    .bind(hashed_token)
    .bind(user_id)
    .bind(refresh_token)
    .bind(ttl_days as i32)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Looks up a session by its *hashed* token and joins the owning user, but
/// only if it hasn't expired -- an expired row reads back identically to
/// no row at all. Expired rows are never explicitly pruned by a
/// background job in this plan (a small table; left as a documented
/// follow-up, same posture as not implementing RP-initiated logout).
pub async fn get_session_with_user(pool: &PgPool, hashed_token: &str) -> Result<Option<SessionUser>> {
    let row = sqlx::query_as::<_, SessionUser>(
        "SELECT u.id, u.email, u.name \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > NOW()",
    )
    .bind(hashed_token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_session(pool: &PgPool, hashed_token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1").bind(hashed_token).execute(pool).await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LoginState {
    pub pkce_verifier: String,
    pub nonce: String,
    pub csrf_state: String,
}

pub async fn insert_login_state(
    pool: &PgPool,
    id: &str,
    pkce_verifier: &str,
    nonce: &str,
    csrf_state: &str,
) -> Result<()> {
    // Opportunistic cleanup -- no cron needed for a table this small and
    // self-limiting; every login attempt takes out its own trash.
    sqlx::query("DELETE FROM oidc_login_state WHERE created_at < NOW() - INTERVAL '15 minutes'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO oidc_login_state (id, pkce_verifier, nonce, csrf_state, created_at) \
         VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(id)
    .bind(pkce_verifier)
    .bind(nonce)
    .bind(csrf_state)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetches and deletes in one step -- login state is single-use by
/// construction (a replayed callback with the same state must not
/// succeed twice). `None` if the id is unknown, already consumed, or
/// older than the 15-minute window `insert_login_state` also sweeps on.
pub async fn consume_login_state(pool: &PgPool, id: &str) -> Result<Option<LoginState>> {
    let row = sqlx::query_as::<_, LoginState>(
        "DELETE FROM oidc_login_state \
         WHERE id = $1 AND created_at > NOW() - INTERVAL '15 minutes' \
         RETURNING pkce_verifier, nonce, csrf_state",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod db_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                session_round_trip_creates_looks_up_and_deletes -- --ignored`"]
    async fn session_round_trip_creates_looks_up_and_deletes() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        let identity = OidcIdentity {
            sub: "TEST-USER-ROUND-TRIP".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
        };
        let user = upsert_user(&pool, &identity).await.expect("upsert user");
        assert_eq!(user.id, "TEST-USER-ROUND-TRIP");

        insert_session(&pool, "test-hashed-token", &user.id, None, 14)
            .await
            .expect("insert session");

        let found = get_session_with_user(&pool, "test-hashed-token")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(found.id, "TEST-USER-ROUND-TRIP");

        delete_session(&pool, "test-hashed-token").await.expect("delete session");
        let gone = get_session_with_user(&pool, "test-hashed-token").await.expect("lookup after delete");
        assert!(gone.is_none());

        // Cleanup -- cascades to sessions via ON DELETE CASCADE, though
        // the session row above was already explicitly deleted.
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-ROUND-TRIP'")
            .execute(&pool)
            .await
            .expect("cleanup test user");
    }
}
```

- [ ] **Step 5: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS. The `#[ignore]`d DB test is skipped by default, as
expected.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/users.rs crates/api/src/data/mod.rs
git commit -m "Add users/sessions/oidc_login_state data layer"
```

---

### Task 6: Session cookie helpers and axum extractors

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Produces: `pub const SESSION_COOKIE_NAME: &str`;
  `fn parse_cookie(headers: &HeaderMap, name: &str) -> Option<String>` (pure,
  unit-tested); `fn set_cookie_header(name, value, max_age_secs) -> String`
  and `fn clear_cookie_header(name) -> String` (pure, unit-tested);
  `fn generate_session_token() -> String`;
  `fn hash_session_token(token: &str) -> String` (pure, unit-tested);
  `struct AuthenticatedUser { id, email, name }` implementing
  `FromRequestParts<App>`; `struct OptionalAuthenticatedUser(Option<AuthenticatedUser>)`.
  Consumed by Task 7 (routes), Task 9/10 (ownership-scoped handlers).

- [ ] **Step 1: Write the failing tests for the pure helpers**

Add to `crates/api/src/auth.rs`, below the existing
`require_internal_token`/`constant_time_eq` but above the existing
`#[cfg(test)] mod tests` block (merge into that same block rather than
adding a second one):

```rust
    #[test]
    fn parse_cookie_finds_a_single_named_cookie() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "nr_session=abc123".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), Some("abc123".to_string()));
    }

    #[test]
    fn parse_cookie_finds_one_among_several() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark; nr_session=abc123; other=x".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), Some("abc123".to_string()));
    }

    #[test]
    fn parse_cookie_returns_none_when_absent() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::COOKIE, "theme=dark".parse().unwrap());
        assert_eq!(parse_cookie(&headers, "nr_session"), None);
    }

    #[test]
    fn parse_cookie_returns_none_with_no_cookie_header_at_all() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(parse_cookie(&headers, "nr_session"), None);
    }

    #[test]
    fn set_cookie_header_includes_all_required_attributes() {
        let header = set_cookie_header("nr_session", "abc123", 1_209_600);
        assert!(header.starts_with("nr_session=abc123;"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("Secure"));
        assert!(header.contains("SameSite=Lax"));
        assert!(header.contains("Max-Age=1209600"));
        assert!(header.contains("Path=/"));
    }

    #[test]
    fn clear_cookie_header_zeroes_max_age() {
        let header = clear_cookie_header("nr_session");
        assert!(header.starts_with("nr_session=;"));
        assert!(header.contains("Max-Age=0"));
    }

    #[test]
    fn hash_session_token_is_deterministic() {
        assert_eq!(hash_session_token("same-token"), hash_session_token("same-token"));
    }

    #[test]
    fn hash_session_token_differs_for_different_tokens() {
        assert_ne!(hash_session_token("token-a"), hash_session_token("token-b"));
    }

    #[test]
    fn generated_session_tokens_are_not_repeated() {
        // Not a proof of randomness, just a smoke test that two calls
        // don't collide -- a collision here would indicate
        // generate_session_token is broken (e.g. always returning a fixed
        // value), not bad luck.
        assert_ne!(generate_session_token(), generate_session_token());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api parse_cookie set_cookie_header hash_session_token generated_session_tokens`
Expected: FAIL — compile errors, none of these functions exist yet.

- [ ] **Step 3: Implement the pure helpers**

Add to `crates/api/src/auth.rs`, above the existing `#[cfg(test)]` block:

```rust
use axum::http::HeaderMap;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const SESSION_COOKIE_NAME: &str = "nr_session";
pub const LOGIN_STATE_COOKIE_NAME: &str = "nr_login";

/// Parses a `Cookie` request header for one named value. Hand-rolled
/// rather than pulling in `axum-extra`'s `CookieJar` -- this app needs
/// exactly "read one cookie by name" and "build one Set-Cookie value",
/// both single-call-site jobs, matching this file's existing
/// `constant_time_eq` precedent for hand-rolling something this narrow
/// rather than adding a dependency for it.
pub fn parse_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

pub fn set_cookie_header(name: &str, value: &str, max_age_secs: i64) -> String {
    format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age_secs}")
}

pub fn clear_cookie_header(name: &str) -> String {
    format!("{name}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0")
}

/// A fresh, high-entropy opaque session/login-state token: 256 bits of OS
/// randomness, base64url-encoded (no padding) for a clean cookie value.
/// This is the value actually sent to the browser -- never stored
/// verbatim (see `hash_session_token`).
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `sessions.id` stores this, not the raw token -- mirrors how a password
/// hash works: a DB dump/leak alone can't be replayed as a live session
/// cookie, only the original random token can. Resolves design doc Open
/// Question 4 in favor of its own stated "more defensible default."
pub fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api parse_cookie set_cookie_header hash_session_token generated_session_tokens`
Expected: PASS.

- [ ] **Step 5: Add the `AuthenticatedUser`/`OptionalAuthenticatedUser` extractors**

Add to `crates/api/src/auth.rs`:

```rust
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::app::App;

/// A resolved, authenticated user -- the `axum` extractor every
/// ownership-scoped handler (custom-line mutations, pinned-lines/
/// pinned-stations reads and writes -- Tasks 9/10) depends on instead of
/// `State<App>` alone. Rejects with `401` if there's no session cookie, no
/// matching (unexpired) `sessions` row, or the row's user was deleted out
/// from under it.
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

impl FromRequestParts<App> for AuthenticatedUser {
    type Rejection = (axum::http::StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let token = parse_cookie(&parts.headers, SESSION_COOKIE_NAME)
            .ok_or((axum::http::StatusCode::UNAUTHORIZED, "no session".to_string()))?;
        let hashed = hash_session_token(&token);
        let session = crate::data::users::get_session_with_user(&app.database, &hashed)
            .await
            .map_err(|err| {
                tracing::error!(error = ?err, "session lookup failed");
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "session lookup failed".to_string())
            })?
            .ok_or((axum::http::StatusCode::UNAUTHORIZED, "session expired or unknown".to_string()))?;
        Ok(AuthenticatedUser { id: session.id, email: session.email, name: session.name })
    }
}

/// Same lookup as `AuthenticatedUser`, but never rejects -- `None` for "no
/// session" instead of `401`. Used only by `GET /auth/session` (Task 7),
/// which must report "not logged in" as a normal `200`, not an error.
pub struct OptionalAuthenticatedUser(pub Option<AuthenticatedUser>);

impl FromRequestParts<App> for OptionalAuthenticatedUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthenticatedUser(AuthenticatedUser::from_request_parts(parts, app).await.ok()))
    }
}
```

- [ ] **Step 6: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS. The two extractors aren't used by any route yet (Task 7)
— no warning is expected for them specifically since they're `pub`, but
confirm no other regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "Add session cookie helpers and AuthenticatedUser extractor"
```

---

### Task 7: Auth routes and `AppState` wiring

**Files:**
- Create: `crates/api/src/routes/auth.rs`
- Modify: `crates/api/src/routes/mod.rs`
- Modify: `crates/api/src/app.rs`

**Interfaces:**
- Consumes: `OidcClient` (Task 4), `data::users::*` (Task 5), `auth::*`
  helpers/extractors (Task 6).
- Produces: `GET /public/auth/login`, `GET /public/auth/callback`,
  `POST /public/auth/logout`, `GET /public/auth/session`. `AppState` gains
  `pub oidc: auth::oidc::OidcClient`. Consumed by the frontend (Task 8's
  proxy makes these reachable at `/api/auth/*`) and Tasks 9/10 (which
  don't call these directly, but depend on `AuthenticatedUser` now being
  resolvable end-to-end).

- [ ] **Step 1: Wire `OidcClient` into `AppState`**

In `crates/api/src/app.rs`, add the import and field:

```rust
use crate::auth::oidc::{OidcClient, OidcConfig};
```

```rust
#[derive(Debug)]
pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    pub redis: redis::Client,
    pub oidc: OidcClient,
}
```

`OidcClient` will need `#[derive(Debug)]` support or a manual `impl Debug`
for this to compile (it holds a `reqwest::Client` and a
`tokio::sync::OnceCell<CoreClient>`, and `CoreClient` may not implement
`Debug`) — if `#[derive(Debug)]` on `AppState` fails to compile once this
field is added, either derive/implement `Debug` manually for `OidcClient`
(printing just `"OidcClient { .. }"`, no secrets) or remove
`#[derive(Debug)]` from `AppState` and hand-roll an equivalent `impl Debug`
that skips the `oidc`/`database`/`redis` fields — check which is less
invasive once the actual compiler error is in hand.

And in `AppState::init`, after the existing `redis` setup:

```rust
        ensure!(
            !config.sso_client_secret.is_empty(),
            "sso_client_secret (--sso-client-secret / SSO_CLIENT_SECRET) must not be empty"
        );

        let oidc = OidcClient::new(OidcConfig {
            issuer_url: config.sso_issuer_url.clone(),
            client_id: config.sso_client_id.clone(),
            client_secret: config.sso_client_secret.clone(),
            redirect_url: config.sso_redirect_url.clone(),
        })
        .context("failed to construct OIDC client")?;

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
        }))
```

Note this only validates URL syntax (per `OidcClient::new`'s own doc
comment, Task 4) — it does not require network access to the SSO server at
startup, so `docker compose up` and any environment without a real,
reachable IdP configured still boots the `api` service cleanly and serves
line-status normally; only `/auth/login`/`/auth/callback` are affected if
the issuer turns out to be unreachable when actually hit.

- [ ] **Step 2: Write the auth routes**

Create `crates/api/src/routes/auth.rs`:

```rust
//! `/public/auth/...`: OIDC login/callback/logout and session-status
//! check. Mounted under `/public` so the existing Next.js proxy forwards
//! `/api/auth/*` unmodified -- see
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Auth
//! routes section and Task 8's proxy fix (required for the redirects and
//! cookies this module issues to actually reach the browser).

use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::{self, AuthenticatedUser, OptionalAuthenticatedUser};
use crate::data::users;

pub fn router() -> Router {
    Router::new()
        .route("/auth/login", axum::routing::get(login))
        .route("/auth/callback", axum::routing::get(callback))
        .route("/auth/logout", axum::routing::post(logout))
        .route("/auth/session", axum::routing::get(session))
}

async fn login(State(app): State<App>) -> Response {
    let (url, pkce_verifier, csrf_state, nonce) = match app.oidc.authorize_url().await {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(error = ?err, "OIDC discovery/authorize_url failed");
            return (StatusCode::SERVICE_UNAVAILABLE, "sign-in temporarily unavailable").into_response();
        }
    };

    let login_state_id = auth::generate_session_token();
    if let Err(err) = users::insert_login_state(
        &app.database,
        &login_state_id,
        pkce_verifier.secret(),
        nonce.secret(),
        csrf_state.secret(),
    )
    .await
    {
        tracing::error!(error = ?err, "failed to store login state");
        return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
    }

    let mut response = Redirect::temporary(url.as_str()).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(auth::LOGIN_STATE_COOKIE_NAME, &login_state_id, 900))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

#[derive(Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(State(app): State<App>, headers: axum::http::HeaderMap, Query(params): Query<CallbackParams>) -> Response {
    if let Some(error) = params.error {
        tracing::warn!(oidc_error = %error, "SSO server returned an error to the callback");
        return (StatusCode::BAD_GATEWAY, "sign-in was not completed").into_response();
    }
    let (Some(code), Some(state)) = (params.code, params.state) else {
        return (StatusCode::BAD_REQUEST, "missing code or state").into_response();
    };

    let Some(login_state_id) = auth::parse_cookie(&headers, auth::LOGIN_STATE_COOKIE_NAME) else {
        return (StatusCode::BAD_REQUEST, "missing login state cookie").into_response();
    };
    let stored = match users::consume_login_state(&app.database, &login_state_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return (StatusCode::BAD_REQUEST, "login state expired or already used").into_response(),
        Err(err) => {
            tracing::error!(error = ?err, "login state lookup failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
        }
    };
    if stored.csrf_state != state {
        tracing::warn!("OIDC callback state mismatch -- possible CSRF attempt or stale link");
        return (StatusCode::BAD_REQUEST, "state mismatch").into_response();
    }

    let exchange_result = app
        .oidc
        .exchange_code(
            code,
            openidconnect::PkceCodeVerifier::new(stored.pkce_verifier),
            &openidconnect::Nonce::new(stored.nonce),
        )
        .await;
    let (identity, refresh_token) = match exchange_result {
        Ok(result) => result,
        Err(err) => {
            tracing::warn!(error = ?err, "OIDC code exchange failed");
            return (StatusCode::BAD_GATEWAY, "sign-in failed").into_response();
        }
    };

    let user = match users::upsert_user(&app.database, &identity).await {
        Ok(u) => u,
        Err(err) => {
            tracing::error!(error = ?err, "failed to upsert user");
            return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
        }
    };

    let session_token = auth::generate_session_token();
    let insert_result = users::insert_session(
        &app.database,
        &auth::hash_session_token(&session_token),
        &user.id,
        refresh_token.as_deref(),
        app.config.session_ttl_days,
    )
    .await;
    if let Err(err) = insert_result {
        tracing::error!(error = ?err, "failed to create session");
        return (StatusCode::INTERNAL_SERVER_ERROR, "sign-in failed").into_response();
    }

    let max_age = app.config.session_ttl_days * 24 * 60 * 60;
    let mut response = Redirect::temporary(&app.config.sso_post_login_redirect_url).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::set_cookie_header(auth::SESSION_COOKIE_NAME, &session_token, max_age))
            .expect("cookie header value is always valid ASCII"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(auth::LOGIN_STATE_COOKIE_NAME))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

async fn logout(State(app): State<App>, headers: axum::http::HeaderMap) -> Response {
    // Local-only logout -- this plan does not implement RP-Initiated
    // Logout (see Global Constraints). If the session cookie is missing
    // or already invalid, logout is still a no-op success (idempotent),
    // not an error.
    if let Some(token) = auth::parse_cookie(&headers, auth::SESSION_COOKIE_NAME) {
        if let Err(err) = users::delete_session(&app.database, &auth::hash_session_token(&token)).await {
            tracing::error!(error = ?err, "failed to delete session on logout");
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth::clear_cookie_header(auth::SESSION_COOKIE_NAME))
            .expect("cookie header value is always valid ASCII"),
    );
    response
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    authenticated: bool,
    id: Option<String>,
    email: Option<String>,
    name: Option<String>,
}

async fn session(OptionalAuthenticatedUser(user): OptionalAuthenticatedUser) -> Json<SessionResponse> {
    match user {
        Some(u) => Json(SessionResponse { authenticated: true, id: Some(u.id), email: u.email, name: u.name }),
        None => Json(SessionResponse { authenticated: false, id: None, email: None, name: None }),
    }
}
```

- [ ] **Step 3: Mount the router**

In `crates/api/src/routes/mod.rs`, add `pub mod auth;` alongside the
existing module declarations, and add `.merge(auth::router())` to
`public_router()`'s builder chain, alongside the existing
`.merge(preferences::router())`.

- [ ] **Step 4: Confirm the workspace builds and the full API test suite passes**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 5: Manually verify against a live dev stack**

This step needs real `SSO_*` env values pointing at an actual OIDC
provider — skip the full round trip if none is configured yet, but at
minimum confirm the service starts and `/auth/session` reports "not
logged in":

```bash
docker compose --env-file dev.env up --build -d api postgres
curl -s http://localhost:8080/public/auth/session
```

Expected: `{"authenticated":false,"id":null,"email":null,"name":null}` —
confirms the service started cleanly even without network access to a
real SSO server (per Task 4/7's lazy-discovery design) and that
`OptionalAuthenticatedUser` correctly reports "no session" rather than
erroring. If a real IdP is configured, additionally open
`http://localhost:8080/public/auth/login` in a browser and confirm it
redirects to the IdP's login page, and that completing login redirects
back to `sso_post_login_redirect_url` with a `nr_session` cookie set
(check via browser devtools — full proxy-mediated verification is Task
11's job, once Task 8 lands).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/auth.rs crates/api/src/routes/mod.rs crates/api/src/app.rs
git commit -m "Add OIDC login/callback/logout/session routes"
```

---

### Task 8: Frontend proxy — forward cookies and redirects

**Files:**
- Modify: `frontend/app/api/[...path]/route.ts`

**Interfaces:**
- Produces: the existing `/api/*` → `/public/*` proxy now forwards the
  `Cookie` request header, every `Set-Cookie` response header, and passes
  `3xx` redirect responses through to the browser unfollowed (status +
  `Location` header) instead of transparently following them server-side.
  Consumed by: every route this plan adds (`/auth/login`'s redirect,
  `/auth/callback`'s redirect + `Set-Cookie`, `/auth/logout`'s
  `Set-Cookie`, `/auth/session`'s `Cookie`-dependent read), and by Tasks
  9/10's now-session-gated preference/custom-line routes.

- [ ] **Step 1: Read the current implementation and confirm the two gaps**

Read `frontend/app/api/[...path]/route.ts` in full. Confirm two separate,
both load-bearing gaps:

1. **Cookies are dropped entirely.** `init` only ever sets `method` and a
   hardcoded `Content-Type` — `req.headers` is never read, so no `Cookie`
   reaches `api`; the response is reconstructed with only `Content-Type`
   copied back, so no `Set-Cookie` reaches the browser. This is the gap
   the design doc calls out explicitly.
2. **Redirects are silently followed, not forwarded.** `fetch`'s default
   `redirect` mode is `'follow'` — a `3xx` response from `api` (which
   `/auth/login` and `/auth/callback`, Task 7, both return) would be
   followed by the *Next.js server's own* `fetch` call, transparently,
   before this handler ever sees it; the browser would receive whatever
   the *final* destination responded with, not the redirect itself. For
   `/auth/login`, that's fatal: the browser must be the one redirected to
   the SSO server's authorization endpoint (so it can complete
   authentication using its own cookies/session with that server), not
   have the Next.js server silently fetch that page on its own. This gap
   isn't explicitly named in the design doc (which only calls out the
   Cookie/Set-Cookie gap) but is equally required for `/auth/login`/
   `/auth/callback` to function at all — found by tracing through exactly
   how a browser-initiated OIDC redirect flow has to move through this
   proxy.

- [ ] **Step 2: Fix `proxy()`**

Replace `proxy()`'s body in `frontend/app/api/[...path]/route.ts`:

```typescript
async function proxy(req: NextRequest, path: string[]): Promise<NextResponse> {
  const target = new URL(`${process.env.API_BASE_URL}/public/${path.join('/')}${req.nextUrl.search}`);
  if (!target.pathname.startsWith('/public/')) {
    return new NextResponse('invalid path', { status: 400 });
  }

  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const cookie = req.headers.get('cookie');
  if (cookie) {
    headers.Cookie = cookie;
  }

  const init: RequestInit = {
    method: req.method,
    headers,
    // 'manual': a 3xx response from `api` (the OIDC login/callback
    // redirects -- see this file's module doc comment) must reach the
    // *browser* as a redirect, not be followed transparently by this
    // server-side fetch call. Node's fetch (unlike a browser's) still
    // gives back a normal, readable Response for a manual redirect --
    // status in [300, 400) and a real `location` header -- rather than an
    // opaque one, so this is safe to branch on below.
    redirect: 'manual',
  };
  if (req.method !== 'GET' && req.method !== 'DELETE') {
    init.body = await req.text();
  }

  const response = await fetch(target, init);

  // A response can carry *multiple* Set-Cookie headers, which
  // `Headers.get()` collapses into one comma-joined string -- unusable
  // for cookies, since a cookie's own `Expires` attribute contains a
  // comma. `getSetCookie()` returns them as a proper string array.
  const setCookies = response.headers.getSetCookie();

  if (response.status >= 300 && response.status < 400) {
    const responseHeaders = new Headers();
    const location = response.headers.get('location');
    if (location) {
      responseHeaders.set('location', location);
    }
    for (const setCookie of setCookies) {
      responseHeaders.append('set-cookie', setCookie);
    }
    return new NextResponse(null, { status: response.status, headers: responseHeaders });
  }

  const body = await response.text();
  const responseHeaders = new Headers({
    'Content-Type': response.headers.get('Content-Type') ?? 'application/json',
  });
  for (const setCookie of setCookies) {
    responseHeaders.append('set-cookie', setCookie);
  }
  // Null-body statuses (204/205/304) may not carry a body on the outgoing
  // Response, not even an empty string -- see the existing PUT/DELETE
  // endpoints this handled before this change; unaffected by this edit.
  return new NextResponse(body === '' ? null : body, { status: response.status, headers: responseHeaders });
}
```

Every other function in this file (`GET`/`POST`/`PUT`/`DELETE` exports)
is unchanged — this edit is entirely inside `proxy()`.

- [ ] **Step 3: Add/update the module doc comment**

Update the file's top-of-file comment (currently explaining only the
`/public/*` scoping rationale) to also explain the redirect-forwarding
addition, referencing this plan's Task 7 auth routes as the reason it's
now needed — a reviewer reading this file cold should understand why
`redirect: 'manual'` exists without needing to reconstruct the reasoning
from git blame.

- [ ] **Step 4: Run the frontend test suite**

Run: `cd frontend && npm test` (or this repo's actual configured test
command — confirm via `frontend/package.json`'s `scripts` if `npm test`
isn't it)
Expected: PASS, no regressions. If `route.ts` has no existing test file
covering `proxy()`, that's consistent with this repo's stated Server/API
route testing convention (`app/` files are verified by hand against the
running stack, not unit tested — see `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`'s
testing-conventions survey) — no new test file is required by this task.

- [ ] **Step 5: Manually verify against a live dev stack**

```bash
docker compose --env-file dev.env up --build -d
curl -si http://localhost:3000/api/auth/session
```

Expected: `200` with `{"authenticated":false,...}` body, proving the
proxy still round-trips a normal JSON GET correctly post-edit. If a real
SSO provider is configured (Task 7 Step 5's prerequisite), additionally:

```bash
curl -si http://localhost:3000/api/auth/login
```

Expected: `307` (or `303`/`302` depending on how `Redirect::temporary`
renders) with a `location` header pointing at the SSO server's
authorization endpoint, and a `set-cookie: nr_login=...` header — proving
both fixes (redirect forwarding, Set-Cookie forwarding) landed correctly.
Confirm an existing, already-working proxied call still works too (regression
check on the non-redirect path):

```bash
curl -s http://localhost:3000/api/lines
```

Expected: unchanged from pre-this-task behavior — a JSON array of lines.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/api/[...path]/route.ts
git commit -m "Forward cookies and redirects through the /api/* proxy"
```

---

### Task 9: Scope custom-line authoring to the authenticated user

**Files:**
- Modify: `crates/api/src/data/custom_lines.rs`
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Consumes: `AuthenticatedUser` (Task 6).
- Produces: `insert_custom_line`/`update_custom_line`/`delete_custom_line`
  gain a `user_id: &str` parameter and enforce it; `POST /lines`,
  `PUT /lines/{id}`, `DELETE /lines/{id}` all now require a resolved
  session (`401` without one). `GET /lines`, `GET /lines/{id}`,
  `GET /lines/{id}/definition` are unchanged — still public, still
  unscoped, per the design doc's Authz-for-existing-endpoints section
  (custom-line *status* is public product content; only *authoring* is
  private).

- [ ] **Step 1: Update `data/custom_lines.rs`'s write functions**

In `crates/api/src/data/custom_lines.rs`:

Change `insert_custom_line`'s signature to `pub async fn insert_custom_line(pool: &PgPool, new: NewCustomLine, user_id: &str) -> Result<CustomLine>`.
Bind `user_id` into the `custom_lines` INSERT (add the column to the
column list and a `$7` placeholder bound to `user_id`). The companion
auto-pin insert changes to match the new composite `pinned_lines` primary
key from Task 2:

```rust
            sqlx::query(
                "INSERT INTO pinned_lines (user_id, line_id, pinned_at) VALUES ($1, $2, NOW()) \
                 ON CONFLICT (user_id, line_id) DO NOTHING",
            )
            .bind(user_id)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
```

Change `update_custom_line`'s signature to
`pub async fn update_custom_line(pool: &PgPool, id: &str, new: NewCustomLine, user_id: &str) -> Result<Option<CustomLine>>`,
adding `AND user_id = $7` to the `UPDATE ... WHERE id = $1` clause (bind
`user_id` as the extra parameter). A row that exists but belongs to
someone else now has `rows_affected() == 0`, identically to a row that
doesn't exist at all — `update_line` (below) already returns `404` for
that case, so no new branching is needed to satisfy the design doc's
"don't distinguish not-found from not-yours" recommendation; it falls out
of this change for free.

Change `delete_custom_line`'s signature to
`pub async fn delete_custom_line(pool: &PgPool, id: &str, user_id: &str) -> Result<bool>`,
adding `AND user_id = $2` to the `custom_lines` DELETE. **Do not** add a
`user_id` predicate to the companion `pinned_lines` cleanup DELETE — that
one intentionally stays `DELETE FROM pinned_lines WHERE line_id = $1`
(no user filter): deleting a custom line removes it for everyone, so
*every* user who had it pinned (not just its owner) has a now-dangling
pin that needs cleaning up too. This is an existing behavior, unchanged in
shape by this task — just confirm it still compiles against the new
composite-PK `pinned_lines` schema (it does; `DELETE ... WHERE line_id = $1`
doesn't reference the primary key at all).

- [ ] **Step 2: Update `routes/lines.rs`'s write handlers**

In `crates/api/src/routes/lines.rs`, add
`use crate::auth::AuthenticatedUser;` and thread a `user: AuthenticatedUser`
extractor parameter through `create_line`, `update_line`, `delete_line`
(placed after `State(app): State<App>` and before any `Json(...)`
extractor, per axum's requirement that body-consuming extractors come
last):

```rust
async fn create_line(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    // ...unchanged validation...
    let created = custom_lines::insert_custom_line(
        &app.database,
        NewCustomLine { /* ...unchanged... */ },
        &user.id,
    )
    .await
    .map_err(internal_error)?;
    // ...unchanged response construction...
}
```

Apply the same `user: AuthenticatedUser` threading to `update_line`
(passing `&user.id` to `update_custom_line`) and `delete_line` (passing
`&user.id` to `delete_custom_line`). `list_lines`, `get_line`,
`get_line_definition` are **not** modified — they keep their current
`State(app): State<App>`-only signatures, matching the design doc's
explicit "GET /lines and GET /lines/{id} stay unauthenticated and
unscoped" call-out.

- [ ] **Step 3: Confirm the workspace builds and the full API test suite passes**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS. No new unit tests are added by this task — the existing
`slugify`/`tfl_display_name`/`is_merged_into_nr_line` tests at the bottom
of `lines.rs` are unaffected (none of them touch ownership), and this
file's CRUD functions have never had DB-backed unit tests (only manual
`curl` verification, matching this repo's documented testing-conventions
survey) — this task doesn't introduce a new testing pattern, it follows
the existing one.

- [ ] **Step 4: Manually verify against a live dev stack**

Requires a completed login (Tasks 7/8) to obtain a real `nr_session`
cookie — either through a real configured IdP, or by hand-inserting a test
`users`/`sessions` row and cookie value matching Task 5's DB test pattern:

```bash
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"

curl -si -X POST http://localhost:8080/public/lines \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"name":"Test Line","operators":["NT"],"stations":["WAT","WOK"]}'
```

Expected: `200` with the created line's JSON, and
`SELECT user_id FROM custom_lines WHERE id = 'custom-test-line'` shows
`TEST-USER`. Repeat without the `Cookie` header:

```bash
curl -si -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"Test Line 2","operators":["NT"],"stations":["WAT","WOK"]}'
```

Expected: `401`. Clean up:
`psql "$DATABASE_URL" -c "DELETE FROM custom_lines WHERE id = 'custom-test-line'; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/custom_lines.rs crates/api/src/routes/lines.rs
git commit -m "Scope custom-line authoring to the authenticated user"
```

---

### Task 10: Scope pinned-lines/pinned-stations to the authenticated user

**Files:**
- Modify: `crates/api/src/data/preferences.rs`
- Modify: `crates/api/src/routes/preferences.rs`

**Interfaces:**
- Consumes: `AuthenticatedUser` (Task 6).
- Produces: every `data::preferences` query gains a `user_id: &str`
  parameter and scopes accordingly; `GET /preferences`,
  `PUT /preferences/pinned-lines`, `PUT /preferences/pinned-stations` all
  now require a resolved session (`401` without one).

- [ ] **Step 1: Update `data/preferences.rs`**

Replace `crates/api/src/data/preferences.rs`'s four ownership-relevant
functions:

```rust
pub async fn list_pinned_line_ids(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT line_id FROM pinned_lines WHERE user_id = $1 ORDER BY pinned_at")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("line_id")?)).collect()
}

pub async fn list_pinned_station_crs(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT crs FROM pinned_stations WHERE user_id = $1 ORDER BY pinned_at")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("crs")?)).collect()
}
```

`filter_existing_station_crs` is unchanged — it has no ownership concept,
it's a pure lookup against the global `stations` table.

```rust
/// Replaces `user_id`'s entire pinned-lines set with `ids`, in one
/// transaction. Scoped to `user_id` now, not the whole table -- the
/// pre-ownership version's `DELETE FROM pinned_lines` (no predicate) would
/// wipe every other user's pins too.
pub async fn replace_pinned_lines(pool: &PgPool, user_id: &str, ids: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    for id in ids {
        sqlx::query("INSERT INTO pinned_lines (user_id, line_id, pinned_at) VALUES ($1, $2, NOW())")
            .bind(user_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn replace_pinned_stations(pool: &PgPool, user_id: &str, crs_codes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_stations WHERE user_id = $1").bind(user_id).execute(&mut *tx).await?;
    for crs in crs_codes {
        sqlx::query("INSERT INTO pinned_stations (user_id, crs, pinned_at) VALUES ($1, $2, NOW())")
            .bind(user_id)
            .bind(crs)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
```

- [ ] **Step 2: Update `routes/preferences.rs`**

In `crates/api/src/routes/preferences.rs`, add
`use crate::auth::AuthenticatedUser;` and thread `user: AuthenticatedUser`
through all three handlers:

```rust
async fn get_preferences(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<PreferencesResponse>, (StatusCode, String)> {
    let pinned_line_ids = preferences::list_pinned_line_ids(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    // ...unchanged custom-lines/known_line_ids logic...
    let pinned_station_candidates = preferences::list_pinned_station_crs(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    // ...unchanged filter_existing_station_crs call and response...
}

async fn put_pinned_lines(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(ids): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_lines(&app.database, &user.id, &ids)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_pinned_stations(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(crs_codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    // ...unchanged length-validation...
    preferences::replace_pinned_stations(&app.database, &user.id, &crs_codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}
```

Also update this file's module doc comment (currently: "Unauthenticated,
same rationale as `/public/lines`") — that rationale no longer holds for
this file specifically once this task lands; `/public/lines`'s *reads*
stay unauthenticated but preferences are now fully session-gated, both
read and write. Reword to reflect that split accurately.

- [ ] **Step 3: Confirm the workspace builds and the full API test suite passes**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Manually verify against a live dev stack**

Using the same manual test session as Task 9 Step 4 (re-create it if
already cleaned up):

```bash
curl -si -X PUT http://localhost:8080/public/preferences/pinned-lines \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '["swr-alton"]'
curl -s http://localhost:8080/public/preferences -H "Cookie: nr_session=manual-test-token"
```

Expected: `204` then `{"pinnedLines":["swr-alton"],"pinnedStations":[]}`.
Without the cookie:

```bash
curl -si http://localhost:8080/public/preferences
```

Expected: `401`. Clean up:
`psql "$DATABASE_URL" -c "DELETE FROM pinned_lines WHERE user_id = 'TEST-USER'; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/preferences.rs crates/api/src/routes/preferences.rs
git commit -m "Scope pinned-lines and pinned-stations to the authenticated user"
```

---

### Task 11: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions anywhere in the workspace.

- [ ] **Step 2: Run `cargo clippy` across the workspace**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings introduced by this plan's changes.

- [ ] **Step 3: Run the frontend test suite**

Run whatever `frontend/package.json` designates as its test script.
Expected: PASS, no regressions.

- [ ] **Step 4: Bring up the full dev stack**

```bash
docker compose --env-file dev.env up --build -d
docker compose ps
```

Expected: `api` shows healthy (confirms Task 7's lazy-discovery design
works — the service starts even before any real SSO provider is
reachable). Confirm `docker compose logs api` shows no crash/restart loop.

- [ ] **Step 5: Manual end-to-end login against a real configured IdP**

This is the one verification step this plan cannot automate or fake — see
this plan's Global Constraints on why the OIDC exchange itself isn't
tested against a mock IdP. Requires the Prerequisites section's real SSO
registration to be complete:

1. Set `SSO_ISSUER_URL`/`SSO_CLIENT_ID`/`SSO_CLIENT_SECRET`/
   `SSO_REDIRECT_URL`/`SSO_POST_LOGIN_REDIRECT_URL` in `dev.env` (or
   equivalent) to real values.
2. Restart the stack, open `http://localhost:3000/api/auth/login` in a
   real browser.
3. Complete login at the SSO server.
4. Confirm the browser lands back on the frontend's root page, and
   `document.cookie` (or devtools' Application/Storage panel) shows an
   `nr_session` cookie scoped to the frontend's own origin, `HttpOnly` (so
   `document.cookie` itself won't show it — confirm via devtools' Network
   tab request headers on the next request instead).
5. Confirm `GET /api/auth/session` now returns
   `{"authenticated":true,"id":"<sub>","email":...,"name":...}`.
6. Pin a line via the UI (or `curl` with the real cookie value from
   devtools), confirm `SELECT user_id FROM pinned_lines` shows the real
   `sub`-derived user id, not `TEST-USER`.
7. Log out via `POST /api/auth/logout`, confirm the cookie is cleared and
   `GET /api/auth/session` reports `authenticated: false` again.

Expected: all seven steps succeed. Any failure here is a real bug in this
plan's implementation, not a test-fixture artifact — unlike Task 4's
`identity_from_claims` unit tests, this is the actual protocol exchange
against a real IdP.

- [ ] **Step 6: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree (everything committed task-by-task above).
