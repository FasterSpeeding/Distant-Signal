# MCP Server OAuth Access Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give DS's own OIDC client the ability to see Authentik-native
`ak_groups` membership as a `groups` claim (Tasks 1-6, all in this repo,
`crates/api` + the dev Authentik blueprint), retire the shipped
`railMcp.discord.*` config in favour of two plain, operator-named
access-group values (Tasks 7-8, this repo's Helm chart + `docker-compose.yml`),
and enforce the resulting two-tier gate (`mcp-users` whole-server,
`mcp-live-boards` for the four metered-Darwin/LDBWS tools) inside
`distant-signal-mcp`'s own OAuth adapter, wherever that adapter's
authorization-grant and tool-listing code actually lives by the time this
task runs (Task 9, a **separate repository**).

**Architecture:**

```
Authentik (DS's OIDC provider)
  groups: ak_groups.all(), via a NEW custom `groups` ScopeMapping
  this plan adds to the dev blueprint (Task 6); a real deployment's
  operator provisions the equivalent on their own instance.
        │ ID token, groups claim included
        ▼
crates/api (this repo -- Tasks 1-5)
  Task 1: migration -- users.groups TEXT[] NOT NULL DEFAULT '{}'
  Task 2: auth/oidc.rs -- authorize_url requests `groups` scope;
          AccessGroupClaims (a real AdditionalClaims impl) replaces
          core::CoreClient's fixed EmptyAdditionalClaims; RawClaims/
          OidcIdentity/identity_from_claims carry groups: Vec<String>
  Task 3: data/users.rs -- upsert_user persists groups (overwritten,
          not merged, every login); SessionUser/get_session_with_user
          select it back out
  Task 4: auth.rs -- AuthenticatedUser/OptionalAuthenticatedUser gain
          groups: Vec<String>
  Task 5: routes/auth.rs -- GET /auth/session's SessionResponse gains
          a `groups` field (camelCase on the wire)
        │
        │ GET {DS_API_BASE_URL}/public/auth/session
        │ (existing route, existing DsApiClient-style call --
        │  Task 9 is the only consumer of the new field)
        ▼
charts/distant-signal + docker-compose.yml (this repo -- Tasks 7-8)
  railMcp.discord.{clientId,allowedUserIds} retired entirely;
  railMcp.accessGroups.{mcpUsersGroup,mcpLiveBoardsGroup} added as
  plain (non-secret) config, threaded to the derived MCP service's
  own process as MCP_USERS_GROUP / MCP_LIVE_BOARDS_GROUP
        │
        ▼
distant-signal-mcp's own adapter -- SEPARATE REPOSITORY (Task 9)
  reads MCP_USERS_GROUP/MCP_LIVE_BOARDS_GROUP at boot; calls
  GET /public/auth/session (now groups-bearing) once it holds a
  completed DS login; gate 1 (mcp-users) enforced before issuing an
  MCP bearer token; gate 2 (mcp-live-boards) enforced at tools/list
  (filters) and tools/call (403 fallback) for the four board/journey
  tools
        │
        ▼
              derived MCP service tool implementations -- UNCHANGED
```

**Tech Stack:** Rust (`crates/api`, `openidconnect` 4.0.1 pinned,
`sqlx` 0.8.6 postgres) for Tasks 1-5. Helm/Go templates
(`charts/distant-signal`) for Task 6-7. `docker-compose.yml` (Compose
`profiles:`, already gated behind `rail-mcp`) for Task 8. TypeScript
(Node, Express) inside `distant-signal-mcp`'s **own, separate git
repository** for Task 9 — see that task's own branching instructions.

**Spec:** `docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md`
— read in full before starting; this plan does not restate its research,
only carries its Decisions into concrete tasks. Cross-references below to
"Decision N" refer to that document. It in turn builds on (and revises)
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`.

## Coordination note — TWO concurrent efforts this plan does not control, read before starting

**This plan was written while two other pieces of work were independently
in flight, neither merged to `main` at the time of writing (re-verified
this session, not assumed):**

1. **`docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`**
   builds the actual OAuth 2.1 adapter authorization-server layer inside
   `distant-signal-mcp` (DCR, `/authorize`, `/token`, the six-tool-route
   middleware) that Task 9 below plugs into. **This plan does not build or
   re-design that adapter** — Decision 1 of the spec this plan implements
   treats it as settled, reused as-is. Task 9 is written against that
   sibling plan's own Task 5 (`src/oauth/internal.ts`'s
   `POST /internal/complete-authorization`) and Task 7
   (`src/oauth/middleware.ts`, applied to all six tool routes) as the two
   concrete integration points, since that is the most specific real
   information available at plan-writing time — **but that sibling plan
   had not landed on `main` (in `distant-signal-mcp`) as of this session**,
   and it may land with a different internal shape, land after this plan,
   or not land at all in its current form. Task 9's own preamble repeats
   this and gives fallback instructions for locating the equivalent
   integration points if the file/function names have moved.
   **A human must confirm, before Task 9 is executed, which of the two
   efforts actually landed first in `distant-signal-mcp` and reconcile
   accordingly** — this plan cannot arbitrate that from inside this
   worktree, since `distant-signal-mcp` is a different repository this
   worktree has no visibility into.
2. **Tasks 7-8 of this plan (the Helm chart and `docker-compose.yml`
   Discord-removal edits) touch exactly the same files
   (`charts/distant-signal/values.yaml`,
   `charts/distant-signal/templates/railmcp-deployment.yaml`,
   `docker-compose.yml`) that the sibling embedded-chatbot plan's own
   Task 8 rewrites substantially** (it adds Redis wiring, an internal
   completion-token secret, and `ingress.railMcp.*` — its own "Global
   Constraints" already states it retires `railMcp.discord.*` too,
   independently reaching the same conclusion this plan's spec reaches).
   **This is a direct, same-file, same-lines merge-conflict risk between
   two plans neither of which depends on the other landing first.** A
   human must sequence Tasks 7-8 here against that sibling plan's Task 8
   — whichever lands second should rebase onto the first's diff rather
   than both being merged independently and silently reintroducing
   `railMcp.discord.*` or double-defining `MCP_USERS_GROUP`.
3. **A third, unrelated, separately-dispatched spec effort** is
   redesigning DS's *internal service* (poller/trust-consumer/
   schedule-ingest) auth to delegate to an OAuth2 server via
   client-credentials-style tokens, replacing the current
   `X-Internal-Token` shared secret. **This is orthogonal** — it is
   service-to-service auth, not human-user access-groups, and shares no
   task-level file overlap with this plan. The only real connection is
   that both efforts eventually touch DS's OIDC/Authentik integration
   surface (`crates/api/src/auth/oidc.rs`, the dev Authentik blueprint
   directory) in the loose sense of "the same subsystem," which is worth
   a human keeping in mind when sequencing merges, but this plan does not
   assume or require any shared implementation surface with it beyond
   that.

**None of the above blocks starting Tasks 1-6 of this plan** (`crates/api`
+ the dev Authentik blueprint) — nothing there touches a file either
concurrent effort touches. Tasks 7-9 are the ones needing human
sequencing before landing on `main`.

## Global Constraints

- **Access groups are Authentik-native (`ak_groups`), read via a `groups`
  OIDC claim — never a DS-built group-management table/UI.** Decision 2.
  DS only ever reads and caches what Authentik asserts; an operator
  manages membership entirely in Authentik.
- **Two groups only, this pass**: `mcp-users` (whole-server gate,
  suggested default name, operator-configurable via chart value) and
  `mcp-live-boards` (additionally required for `get_departures`,
  `get_arrivals`, `get_service_detail`, `plan_journey` — the four tools
  touching metered Darwin/LDBWS keys directly). `resolve_station` and
  `find_services` require only `mcp-users`. Decision 3. Not designed
  finer than this by this plan.
- **Groups are captured at login time and overwritten (never merged) on
  every subsequent login** — the same re-sync posture `email`/`name`
  already have in `upsert_user`. A user removed from a group in Authentik
  keeps their last-issued access until they next log in; this is a known,
  accepted staleness trade-off (spec's Open question/risk 2), not solved
  by this plan.
- **`core::CoreClient`'s `AdditionalClaims` type parameter is hardcoded to
  `EmptyAdditionalClaims`, and so is `core::CoreTokenResponse` (via
  `CoreIdTokenFields`) independently of whatever `AC` a `Client` is
  otherwise instantiated with** — confirmed by reading the pinned
  `openidconnect` 4.0.1 crate source directly this session
  (`core/mod.rs`'s `CoreClient`/`CoreIdTokenFields`/`CoreTokenResponse`
  type aliases). Task 2 therefore widens **both** the `Client`'s `AC`
  parameter and its `TR` (token-response) parameter together — swapping
  only one does not compile, since `core::CoreTokenResponse` would still
  silently discard the claim even with a widened `Client<AC, ...>`.
- **`railMcp.discord.*` is retired outright, not kept as a secondary
  gate** — the mandatory MCP bearer-token check (Task 9, enforcing
  `mcp-users`/`mcp-live-boards`) is a strictly stronger gate than the old
  Discord allowlist ever was; keeping both is pure operational overhead
  for no added security property. Matches the sibling embedded-chatbot
  plan's own independent conclusion (see Coordination note above).
- **No frontend changes anywhere in this plan.** Per the spec's own
  "Explicitly out of scope": the `groups` plumbing this plan adds is
  general (any future DS feature could read `AuthenticatedUser.groups`),
  but this plan's own use of it stops at gating the derived MCP service's
  tools. No other consumer is designed or implied here.
- **Migration convention:** additive only, `crates/api/migrations/`,
  `YYYYMMDDHHMMSS_description.sql` naming, matching every existing
  migration in that directory (most recent as of this session:
  `20260901150000_stanox_crs.sql`). This plan's migration is
  `20260902160000_user_access_groups.sql`.
- **Testing convention (`crates/api`):** colocated `#[cfg(test)] mod
  tests` per file, `cargo test -p api <test_name>`. DB-backed tests
  follow the existing `#[ignore = "requires a live database..."]`
  pattern in `crates/api/src/data/users.rs`'s `db_tests` module — run
  explicitly with `cargo test -p api <name> -- --ignored` against a real
  `DATABASE_URL`. Final verification (Task 10) runs `cargo test
  --workspace` and `cargo clippy --workspace --all-targets`.
- **Helm convention:** `helm template charts/distant-signal --set
  railMcp.enabled=true ... --show-only templates/<file>.yaml | grep ...`,
  matching `docs/superpowers/plans/2026-08-29-dev-oidc-server.md`'s own
  established invocation shape. `helm lint charts/distant-signal` as a
  final check.
- **`distant-signal-mcp` is a separate git repository, not part of this
  worktree's isolation.** Task 9 says so explicitly and gives its own
  branching instructions — do not attempt to run its steps from inside
  this worktree's checkout of the main `distant-signal` repo.
- **Parallelizable tasks:** Tasks 1-5 are sequential within `crates/api`
  (each depends on the previous: migration → claim plumbing → persistence
  → extractor → route). Task 6 (dev Authentik blueprint) depends on
  nothing in this plan — it's a standalone YAML file — and can be
  dispatched at any point, even first. Tasks 7 and 8 each depend only on
  Decision 3's group names (no code dependency on Tasks 1-6) and touch
  disjoint files from each other — dispatch in parallel, but see the
  Coordination note above before either lands on `main`. Task 9 depends
  on Task 5 (the `groups` field existing on `GET /auth/session`) and
  Tasks 7-8 (the two env var names) being merged first, plus the separate
  adapter work per the Coordination note. Task 10 depends on Tasks 1-8
  (Task 9's own verification lives in `distant-signal-mcp`'s repo, not
  here).

---

### Task 1: Migration — `users.groups`

**Files:**
- Create: `crates/api/migrations/20260902160000_user_access_groups.sql`

**Interfaces:**
- Produces: `users.groups TEXT[] NOT NULL DEFAULT '{}'` — consumed by
  Task 3's `upsert_user`/`get_session_with_user`.
- **Depends on:** nothing — foundational.

- [ ] **Step 1: Write the migration**

```sql
-- Adds the Authentik-native access-group names asserted by the OIDC
-- provider's `groups` claim (see crates/api/src/auth/oidc.rs's
-- AccessGroupClaims, added alongside this migration) -- see
-- docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
-- Decision 2.
--
-- NOT NULL DEFAULT '{}' rather than the design doc's own "nullable"
-- phrasing: this app never needs to distinguish "no groups claim was
-- ever asserted" from "the claim was asserted empty" -- both mean
-- "no access groups," so a non-nullable empty-array default avoids an
-- Option<Vec<String>> at the SQL layer for zero behavioural difference,
-- matching the same "never trust absence as something colder than empty"
-- posture identity_from_claims already takes for email_verified
-- (crates/api/src/auth/oidc.rs).
--
-- Overwritten wholesale on every login by upsert_user (Task 3), never
-- merged/unioned -- a group removed in Authentik is reflected on the
-- user's very next login, matching how email/name already re-sync every
-- return visit.
ALTER TABLE users ADD COLUMN groups TEXT[] NOT NULL DEFAULT '{}';
```

- [ ] **Step 2: Apply the migration and confirm the column exists**

Run (against a local/test `DATABASE_URL` this repo's existing migration
tooling already targets): `sqlx migrate run --source crates/api/migrations`
(or however this environment normally applies `crates/api`'s migrations —
follow the same command Task 1 of
`docs/superpowers/plans/2026-08-28-user-accounts-sso.md` used, if this
environment differs from a bare `sqlx-cli` install).

Expected: migration applies cleanly; `psql $DATABASE_URL -c '\d users'`
shows a new `groups` column, type `text[]`, `NOT NULL`, default `'{}'::text[]`.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260902160000_user_access_groups.sql
git commit -m "Add users.groups column for Authentik-native MCP access groups"
```

---

### Task 2: `auth/oidc.rs` — request and read the `groups` claim

**Files:**
- Modify: `crates/api/src/auth/oidc.rs`

**Interfaces:**
- Produces: `AccessGroupClaims { groups: Option<Vec<String>> }` (a real
  `openidconnect::AdditionalClaims` impl); `OidcIdentity.groups:
  Vec<String>`; `RawClaims.groups: Option<Vec<String>>`;
  `identity_from_claims` now defaults a missing claim to an empty vec.
- Consumed by: Task 3 (`data::users::upsert_user` reads
  `identity.groups`).
- **Depends on:** nothing else in this plan — self-contained within this
  one file. (Re-verified this session against current `main`: the two
  `add_scope` calls this task extends are at `oidc.rs:161-162`, the
  `DiscoveredClient` type alias this task replaces is at `oidc.rs:94-95`
  — both confirmed unchanged from the design spec's own citations.)

Current `oidc.rs` (`main`, confirmed this session) has this
`DiscoveredClient` alias:

```rust
type DiscoveredClient =
    CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;
```

`core::CoreClient`'s own definition (read directly from the pinned
`openidconnect` 4.0.1 source this session,
`~/.cargo/registry/.../openidconnect-4.0.1/src/core/mod.rs`) fixes its
`AC` (AdditionalClaims) parameter to `EmptyAdditionalClaims`, **and**
separately fixes its `TR` (token response) parameter to
`CoreTokenResponse`, which is itself `StandardTokenResponse<CoreIdTokenFields,
CoreTokenType>` where `CoreIdTokenFields` **also** hardcodes
`EmptyAdditionalClaims` as its own first parameter — independently of
whatever `AC` a generic `Client<AC, ...>` is instantiated with. Both must
move together; this task widens both.

- [ ] **Step 1: Extend the imports**

Replace (`oidc.rs:1-15`):

```rust
use anyhow::{Context, Result};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::url::Url;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope,
};
```

with:

```rust
use anyhow::{Context, Result};
use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreErrorResponseType, CoreGenderClaim,
    CoreJsonWebKey, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreProviderMetadata,
    CoreRevocableToken, CoreRevocationErrorResponse, CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::url::Url;
use openidconnect::{
    AdditionalClaims, AuthorizationCode, Client, ClientId, ClientSecret, CsrfToken,
    EmptyExtraTokenFields, EndpointMaybeSet, EndpointNotSet, EndpointSet, IdTokenFields, IssuerUrl,
    Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    StandardErrorResponse, StandardTokenResponse,
};
```

(`CoreClient` itself is no longer imported — every call site below moves
onto the fully generic `Client` plus this file's own `DiscoveredClient`
alias.)

- [ ] **Step 2: Add `AccessGroupClaims` and the two supporting type aliases**

Insert immediately after the existing `OidcIdentity`/`RawClaims`/
`identity_from_claims` block (before the current `OidcConfig` struct,
`oidc.rs:55-56` in today's `main`):

```rust
/// The `groups` claim this app additionally requests and reads off the ID
/// token, beyond what `openidconnect::core`'s fixed `CoreClient` alias can
/// see. Confirmed directly against the pinned `openidconnect` 4.0.1
/// source this session: `core::CoreClient` hardcodes its
/// `AdditionalClaims` type parameter to `EmptyAdditionalClaims`, which
/// silently discards any claim beyond the standard set `openidconnect::
/// core` models -- reading `groups` requires this real `AdditionalClaims`
/// impl and a `Client` built on it, not just a new field read off an
/// already-parsed struct. See
/// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
/// Decision 2.
///
/// `#[serde(default)]` on `groups`: a missing claim deserializes to
/// `None`, never a deserialization error -- the same "never trust silence
/// as something stronger than it is" posture `email_verified`'s own
/// handling already takes below.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AccessGroupClaims {
    #[serde(default)]
    pub groups: Option<Vec<String>>,
}

impl AdditionalClaims for AccessGroupClaims {}

/// Mirrors `openidconnect::core`'s own `CoreIdTokenFields`/
/// `CoreTokenResponse` type aliases (`core/mod.rs`), but with
/// `AccessGroupClaims` in place of `EmptyAdditionalClaims` as the
/// `AdditionalClaims` type parameter. Both must be redefined together --
/// see this file's own module-level note on why swapping only
/// `DiscoveredClient`'s `AC` parameter and leaving `TR` at
/// `CoreTokenResponse` would silently keep discarding the claim.
type GroupsIdTokenFields = IdTokenFields<
    AccessGroupClaims,
    EmptyExtraTokenFields,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;
type GroupsTokenResponse = StandardTokenResponse<GroupsIdTokenFields, CoreTokenType>;
```

- [ ] **Step 3: Extend `OidcIdentity` and `RawClaims`, update `identity_from_claims`**

Replace the existing `OidcIdentity` struct:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OidcIdentity {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
}
```

with:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct OidcIdentity {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub groups: Vec<String>,
}
```

Replace the existing `RawClaims` struct:

```rust
#[derive(Debug, Clone)]
pub struct RawClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}
```

with:

```rust
#[derive(Debug, Clone)]
pub struct RawClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub groups: Option<Vec<String>>,
}
```

Replace `identity_from_claims`:

```rust
pub fn identity_from_claims(claims: RawClaims) -> OidcIdentity {
    OidcIdentity {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
        name: claims.name,
    }
}
```

with:

```rust
pub fn identity_from_claims(claims: RawClaims) -> OidcIdentity {
    OidcIdentity {
        sub: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
        name: claims.name,
        groups: claims.groups.unwrap_or_default(),
    }
}
```

- [ ] **Step 4: Replace the `DiscoveredClient` type alias**

Replace:

```rust
type DiscoveredClient =
    CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;
```

with:

```rust
/// `CoreClient` after `from_provider_metadata` + `set_redirect_uri`, at
/// its concrete typestate -- see the original comment on this type (now
/// below) for why the six endpoint-typestate parameters are what they
/// are; unchanged by this task. Built on the fully generic
/// `openidconnect::Client<...>` rather than the `core` module's
/// `CoreClient` alias, since `CoreClient` fixes its `AdditionalClaims`
/// parameter to `EmptyAdditionalClaims` (see `AccessGroupClaims`'s own
/// doc comment, above). Every parameter here besides `AC`/`TR` is copied
/// verbatim from `core::CoreClient`'s own definition
/// (`openidconnect::core::mod`, confirmed against the pinned 4.0.1
/// source this session).
type DiscoveredClient = Client<
    AccessGroupClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    GroupsTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
```

- [ ] **Step 5: Swap `CoreClient::from_provider_metadata` for `Client::from_provider_metadata`**

Inside `client()`, replace:

```rust
                let client = CoreClient::from_provider_metadata(
                    metadata,
                    ClientId::new(self.config.client_id.clone()),
                    Some(ClientSecret::new(self.config.client_secret.clone())),
                )
                .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);
```

with:

```rust
                let client = Client::from_provider_metadata(
                    metadata,
                    ClientId::new(self.config.client_id.clone()),
                    Some(ClientSecret::new(self.config.client_secret.clone())),
                )
                .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);
```

(`metadata` here is still `CoreProviderMetadata` -- unchanged. `from_provider_metadata`
is generic over the `Client`'s own type parameters and does not require
`ProviderMetadata`'s parameters to match them; confirmed by reading its
signature in the pinned crate source this session.)

- [ ] **Step 6: Request the `groups` scope in `authorize_url`**

Replace:

```rust
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
```

with:

```rust
            .add_scope(Scope::new("email".to_string()))
            .add_scope(Scope::new("profile".to_string()))
            // A dedicated scope, not relying on the built-in `profile`
            // mapping's own group-membership behaviour alone -- see
            // Decision 2 of
            // docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
            // on why (real-world reports of the built-in mapping not
            // reliably populating groups). Task 6 adds the matching
            // custom ScopeMapping to this repo's own dev Authentik
            // blueprint; a real deployment's operator provisions the
            // equivalent on their own instance.
            .add_scope(Scope::new("groups".to_string()))
```

- [ ] **Step 7: Read `groups` out of the verified ID token in `exchange_code`**

Replace:

```rust
        let raw = RawClaims {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().to_string()),
            email_verified: claims.email_verified(),
            name: claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string()),
        };
```

with:

```rust
        let raw = RawClaims {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().to_string()),
            email_verified: claims.email_verified(),
            name: claims.name().and_then(|n| n.get(None)).map(|n| n.as_str().to_string()),
            groups: claims.additional_claims().groups.clone(),
        };
```

(`claims` here is `&IdTokenClaims<AccessGroupClaims, CoreGenderClaim>` --
`additional_claims()` is a real accessor on `IdTokenClaims`, confirmed in
the pinned crate source this session, returning `&AccessGroupClaims`.)

- [ ] **Step 8: Update the existing unit tests' fixture and add two new tests**

Replace the test module's `claims()` helper:

```rust
    fn claims(email_verified: Option<bool>) -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
        }
    }
```

with:

```rust
    fn claims(email_verified: Option<bool>) -> RawClaims {
        claims_with_groups(email_verified, None)
    }

    fn claims_with_groups(email_verified: Option<bool>, groups: Option<Vec<String>>) -> RawClaims {
        RawClaims {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
            groups,
        }
    }
```

(Every existing test in this module calls `claims(...)` unchanged and
keeps passing -- `groups: None` there is equivalent to today's implicit
absence.)

Add two new tests, alongside the existing
`missing_email_verified_claim_defaults_to_unverified`:

```rust
    #[test]
    fn groups_claim_is_kept_when_present() {
        let identity = identity_from_claims(claims_with_groups(
            Some(true),
            Some(vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]),
        ));
        assert_eq!(identity.groups, vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]);
    }

    #[test]
    fn missing_groups_claim_defaults_to_empty_vec_not_an_error() {
        let identity = identity_from_claims(claims(Some(true)));
        assert_eq!(identity.groups, Vec::<String>::new());
    }
```

- [ ] **Step 9: Build and run the tests**

Run: `cargo build -p api && cargo test -p api`

Expected: builds cleanly (this is the step that actually exercises
whether the `Client<AccessGroupClaims, ...>` type-parameter substitution
compiles against the pinned `openidconnect` 4.0.1 -- if it doesn't, the
compiler's own error will name which of the eleven fixed type parameters
in `DiscoveredClient` needs adjusting; re-check against
`core::CoreClient`'s definition in
`~/.cargo/registry/src/*/openidconnect-4.0.1/src/core/mod.rs` rather than
guessing). All `oidc` module tests pass, including the two new ones.

- [ ] **Step 10: Commit**

```bash
git add crates/api/src/auth/oidc.rs
git commit -m "Request and read a groups OIDC claim via a real AdditionalClaims impl"
```

---

### Task 3: `data/users.rs` — persist and re-read `groups`

**Files:**
- Modify: `crates/api/src/data/users.rs`

**Interfaces:**
- Consumes: `OidcIdentity.groups: Vec<String>` (Task 2).
- Produces: `SessionUser.groups: Vec<String>` — consumed by Task 4
  (`AuthenticatedUser`'s extractor).
- **Depends on:** Task 1 (the `users.groups` column), Task 2
  (`OidcIdentity.groups`).

- [ ] **Step 1: `upsert_user` persists `groups`, overwritten every login**

Replace:

```rust
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
```

with:

```rust
pub async fn upsert_user(pool: &PgPool, identity: &OidcIdentity) -> Result<User> {
    let email = verified_email(identity);
    let row = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, name, groups, created_at, last_login_at) \
         VALUES ($1, $2, $3, $4, NOW(), NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
            email = EXCLUDED.email, name = EXCLUDED.name, groups = EXCLUDED.groups, \
            last_login_at = NOW() \
         RETURNING id, email, name",
    )
    .bind(&identity.sub)
    .bind(email)
    .bind(&identity.name)
    .bind(&identity.groups)
    .fetch_one(pool)
    .await?;
    Ok(row)
}
```

(`groups = EXCLUDED.groups`, unconditionally -- overwritten, never
merged/unioned with whatever was already stored, per Global Constraints.
`User`'s own `RETURNING`/struct shape is untouched: nothing in this repo
consumes `User.groups` today -- only `SessionUser`, below, needs it,
since that's what `AuthenticatedUser`'s extractor is built from.)

- [ ] **Step 2: `SessionUser` and `get_session_with_user` select `groups` too**

Replace:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}
```

with:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub groups: Vec<String>,
}
```

Replace:

```rust
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
```

with:

```rust
pub async fn get_session_with_user(pool: &PgPool, hashed_token: &str) -> Result<Option<SessionUser>> {
    let row = sqlx::query_as::<_, SessionUser>(
        "SELECT u.id, u.email, u.name, u.groups \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > NOW()",
    )
    .bind(hashed_token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 3: Add a DB-backed regression test for overwrite-not-merge**

Add to the existing `#[cfg(test)] mod db_tests` block, alongside
`session_round_trip_creates_looks_up_and_deletes`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                groups_are_overwritten_not_merged_on_repeat_login -- --ignored`"]
    async fn groups_are_overwritten_not_merged_on_repeat_login() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        let mut identity = OidcIdentity {
            sub: "TEST-USER-GROUPS-OVERWRITE".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            groups: vec!["mcp-users".to_string(), "mcp-live-boards".to_string()],
        };
        let user = upsert_user(&pool, &identity).await.expect("first login upsert");

        insert_session(&pool, "test-hashed-token-groups", &user.id, 14).await.expect("insert session");
        let found = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(found.groups, vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]);

        // Second login, with mcp-live-boards removed in Authentik -- must
        // be reflected exactly, not unioned with the first login's set.
        identity.groups = vec!["mcp-users".to_string()];
        upsert_user(&pool, &identity).await.expect("second login upsert");
        let found_again = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should still exist");
        assert_eq!(found_again.groups, vec!["mcp-users".to_string()]);

        delete_session(&pool, "test-hashed-token-groups").await.expect("delete session");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-GROUPS-OVERWRITE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo build -p api && cargo test -p api`
Expected: builds cleanly; non-DB tests pass. If a real `DATABASE_URL` is
available in this environment, additionally run: `cargo test -p api
groups_are_overwritten_not_merged_on_repeat_login -- --ignored` and
confirm PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/users.rs
git commit -m "Persist and re-read groups on upsert_user/get_session_with_user"
```

---

### Task 4: `auth.rs` — `AuthenticatedUser` gains `groups`

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Consumes: `SessionUser.groups` (Task 3).
- Produces: `AuthenticatedUser.groups: Vec<String>` — consumed by Task 5
  (`GET /auth/session`'s handler).
- **Depends on:** Task 3.

- [ ] **Step 1: Extend `AuthenticatedUser`**

Replace:

```rust
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}
```

with:

```rust
pub struct AuthenticatedUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub groups: Vec<String>,
}
```

- [ ] **Step 2: Update the extractor's construction site**

Replace:

```rust
        Ok(AuthenticatedUser { id: session.id, email: session.email, name: session.name })
```

with:

```rust
        Ok(AuthenticatedUser { id: session.id, email: session.email, name: session.name, groups: session.groups })
```

(`OptionalAuthenticatedUser` wraps `AuthenticatedUser` unchanged — no
edit needed there. No other call site in this crate destructures
`AuthenticatedUser` exhaustively; every consumer (`routes/lines.rs`,
`routes/preferences.rs`, `routes/train.rs`) takes it as a typed extractor
parameter and only ever reads `.id`, confirmed by grep this session — the
new field is additive and does not break any existing call site.)

- [ ] **Step 3: Build**

Run: `cargo build -p api`
Expected: builds cleanly.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "AuthenticatedUser/OptionalAuthenticatedUser gain a groups field"
```

---

### Task 5: `routes/auth.rs` — `GET /auth/session` returns `groups`

**Files:**
- Modify: `crates/api/src/routes/auth.rs`

**Interfaces:**
- Consumes: `AuthenticatedUser.groups` (Task 4).
- Produces: `SessionResponse.groups: Vec<String>` (serialized as
  `"groups"`, camelCase, matching the struct's existing
  `#[serde(rename_all = "camelCase")]`) — this is the field Task 9 (a
  separate repository) reads over HTTP once this lands.
- **Depends on:** Task 4.

- [ ] **Step 1: Extend `SessionResponse` and the `session` handler**

Replace:

```rust
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

with:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    authenticated: bool,
    id: Option<String>,
    email: Option<String>,
    name: Option<String>,
    /// Always present, empty when logged out or when the logged-in user
    /// asserted no groups -- never omitted, so a consumer (Task 9's
    /// adapter, in distant-signal-mcp's own separate repository) can
    /// always treat this as a plain string array rather than an optional
    /// field. See
    /// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
    /// Decision 2's "one existing read-back point" framing.
    groups: Vec<String>,
}

async fn session(OptionalAuthenticatedUser(user): OptionalAuthenticatedUser) -> Json<SessionResponse> {
    match user {
        Some(u) => {
            Json(SessionResponse { authenticated: true, id: Some(u.id), email: u.email, name: u.name, groups: u.groups })
        }
        None => Json(SessionResponse { authenticated: false, id: None, email: None, name: None, groups: vec![] }),
    }
}
```

- [ ] **Step 2: Add a response-shape unit test**

This crate has no existing HTTP-level integration test harness for
`crates/api/src/routes/` (confirmed this session — no `crates/api/tests/`
directory exists); every existing test in this file is a pure-function
unit test (`captured_return_to`, `post_login_target`). Consistent with
that, test the response shape via direct JSON serialization rather than
introducing a new HTTP test harness solely for this change. Add to the
existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn session_response_serializes_groups_as_a_plain_camel_case_array() {
        let response = SessionResponse {
            authenticated: true,
            id: Some("user-123".to_string()),
            email: Some("rider@example.com".to_string()),
            name: Some("Ada Rider".to_string()),
            groups: vec!["mcp-users".to_string(), "mcp-live-boards".to_string()],
        };
        let json = serde_json::to_value(&response).expect("serializes");
        assert_eq!(json["groups"], serde_json::json!(["mcp-users", "mcp-live-boards"]));
    }

    #[test]
    fn session_response_groups_is_an_empty_array_not_null_when_logged_out() {
        let response = SessionResponse { authenticated: false, id: None, email: None, name: None, groups: vec![] };
        let json = serde_json::to_value(&response).expect("serializes");
        assert_eq!(json["groups"], serde_json::json!([]));
    }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p api session_response`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/auth.rs
git commit -m "GET /auth/session now returns the caller's groups"
```

---

### Task 6: Dev Authentik blueprint — a custom `groups` scope mapping

**Files:**
- Modify: `charts/distant-signal/files/devauthentik-blueprints/oauth2-client.yaml`

**Interfaces:**
- Produces: a `groups`-scoped Authentik `ScopeMapping` in the dev IdP,
  attached to the `distant-signal-dev` OAuth2 provider.
- **Depends on:** nothing else in this plan — standalone YAML,
  parallelizable with Tasks 1-5.

Authentik does **not** ship a built-in `groups` scope mapping the way it
ships `openid`/`email`/`profile` (researched and cited directly in the
design spec this plan implements) — the existing three `!Find` lines in
this file look up Authentik's own pre-existing, default-blueprint-applied
mappings by `scope_name`. A `groups` mapping has to be **defined**, not
just found.

- [ ] **Step 1: Add a new `ScopeMapping` entry**

Insert a new list entry under `entries:`, before the existing
`oauth2provider` entry (order doesn't matter to Authentik's blueprint
applier, but keeping the thing being referenced defined before the
provider that references it reads more naturally):

```yaml
  - model: authentik_providers_oauth2.scopemapping
    identifiers:
      name: distant-signal-dev-groups
    attrs:
      scope_name: groups
      description: >-
        DS's own MCP access-group claim -- Authentik-native ak_groups
        membership, not a Discord- or DS-owned group system. See
        docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
        Decision 2. This dev instance has no built-in `groups` scope
        mapping (unlike openid/email/profile, which ship as Authentik
        defaults) -- this entry defines one.
      expression: |
        return {"groups": [group.name for group in request.user.ak_groups.all()]}
```

- [ ] **Step 2: Attach it to the existing provider's `property_mappings`**

Replace:

```yaml
      property_mappings:
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, openid]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, email]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, profile]]
```

with:

```yaml
      property_mappings:
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, openid]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, email]]
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, profile]]
        # Defined earlier in THIS file (not an Authentik default, unlike
        # the three above) -- see the new authentik_providers_oauth2.
        # scopemapping entry's own comment.
        - !Find [authentik_providers_oauth2.scopemapping, [scope_name, groups]]
```

- [ ] **Step 3: Render and sanity-check the blueprint file**

This file is mounted verbatim into a ConfigMap
(`charts/distant-signal/templates/`) rather than templated itself, so
`helm template` won't catch a YAML error inside it on its own merits, but
will catch the ConfigMap wrapper failing to render. Run:

```bash
helm template charts/distant-signal --set devAuthentik.enabled=true \
  --show-only templates/devauthentik-blueprints-configmap.yaml \
  | grep -A2 "scope_name: groups"
```

Expected: the ConfigMap renders (confirms valid YAML/no templating
error), and the `grep` shows the new `scope_name: groups` block present
in the rendered blueprint content. Also run a plain YAML syntax check on
the file directly: `python3 -c "import yaml, sys; yaml.safe_load_all(open('charts/distant-signal/files/devauthentik-blueprints/oauth2-client.yaml'))"`
(or any locally available YAML linter) — expect no error.

(This blueprint only affects the **dev** Authentik instance this chart
can optionally stand up — a real operator's own Authentik instance needs
the equivalent custom scope mapping provisioned by them, which is an
operational/documentation concern, not something this repo's dev
blueprint controls. Not designed further here, matching the design spec's
own Open question/risk 1.)

- [ ] **Step 4: Commit**

```bash
git add charts/distant-signal/files/devauthentik-blueprints/oauth2-client.yaml
git commit -m "Add a custom groups ScopeMapping to the dev Authentik blueprint"
```

---

### Task 7: Helm chart — retire `railMcp.discord.*`, add `railMcp.accessGroups.*`

**Files:**
- Modify: `charts/distant-signal/values.yaml`
- Modify: `charts/distant-signal/templates/secret.yaml`
- Modify: `charts/distant-signal/templates/_helpers.tpl`
- Modify: `charts/distant-signal/templates/railmcp-deployment.yaml`

**Interfaces:**
- Produces: `railMcp.accessGroups.mcpUsersGroup` /
  `railMcp.accessGroups.mcpLiveBoardsGroup` chart values, rendered as
  plain (non-secret) `MCP_USERS_GROUP`/`MCP_LIVE_BOARDS_GROUP` env vars on
  the `railmcp` Deployment — consumed by Task 9 (a separate repository).
- **Depends on:** nothing else in this plan (no code dependency on Tasks
  1-6) — but see the Coordination note at the top of this plan before
  landing this task: the embedded-chatbot sibling plan's own Task 8
  rewrites `railmcp-deployment.yaml`/`values.yaml` substantially and
  independently also retires `railMcp.discord.*`. **A human must sequence
  this task against that one.**

All four citations below were re-verified against current `main` this
session (not trusted from the design spec's own citations, which had
already drifted slightly by a few lines by the time of this research —
e.g. the spec cites `docker-compose.yml:512-513` for the Discord env
lines; current `main` has them at `docker-compose.yml:554-555`, after an
unrelated compose-gating fix landed in between).

- [ ] **Step 1: `values.yaml` — replace `discord:` with `accessGroups:`**

Replace (`values.yaml`, inside the `railMcp:` block):

```yaml
  # -- The Discord application this MCP server's OAuth resource-server
  # verification checks tokens against -- the fork's own DISCORD_CLIENT_ID/
  # DISCORD_ALLOWED_USER_IDS. Neither has a sensible chart-wide default;
  # both are required by the fork's own src/config.ts at boot.
  discord:
    clientId: ""
    allowedUserIds: ""
```

with:

```yaml
  # -- Authentik-native access-group names this deployment's operator has
  # configured (Decision 3 of
  # docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md):
  # mcpUsersGroup gates the whole MCP server -- the direct replacement for
  # Discord's old RAIL_MCP_DISCORD_ALLOWED_USER_IDS allowlist --
  # mcpLiveBoardsGroup additionally gates the four tools that hit metered
  # Darwin/LDBWS product keys directly. Enforced entirely by the derived
  # MCP service's own adapter (a separate repository); this chart only
  # ever threads these two names through as plain, non-secret config, the
  # same way it passes any other operator-chosen string -- an Authentik
  # group name is not a credential.
  accessGroups:
    mcpUsersGroup: mcp-users
    mcpLiveBoardsGroup: mcp-live-boards
```

Also remove the two now-dead `existingSecret*Key` lines further down in
the same block:

```yaml
  existingSecretDiscordClientIdKey: discord-client-id
  existingSecretDiscordAllowedUserIdsKey: discord-allowed-user-ids
```

(delete both — no secret-backed lookup is needed for a plain string
value; `railMcp.existingSecret*` continues to exist for the six
`ldbws.*` credentials, untouched by this task.)

- [ ] **Step 2: `secret.yaml` — stop rendering the two Discord secret keys**

Replace:

```yaml
{{/* railMcp's own eight credentials (Discord OAuth + the six LDBWS
     product values, unchanged by this integration -- Decision 5): like
     llm-api-key/kafka-sasl-*, deliberately NOT auto-generated, since a
     random Discord client ID or LDBWS key is meaningless. Rendered
     (possibly empty) whenever no railMcp.existingSecret is configured, so
     railmcp-deployment.yaml's secretKeyRefs always resolve. */}}
{{- if not .Values.railMcp.existingSecret -}}
{{- $_ := set $data "discord-client-id" (.Values.railMcp.discord.clientId | default "" | b64enc) -}}
{{- $_ := set $data "discord-allowed-user-ids" (.Values.railMcp.discord.allowedUserIds | default "" | toString | b64enc) -}}
{{- $_ := set $data "ldbws-departures-url" (.Values.railMcp.ldbws.departuresUrl | default "" | b64enc) -}}
```

with:

```yaml
{{/* railMcp's own six LDBWS product-key credentials (Decision 5 of
     docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md,
     unchanged by this integration). Discord OAuth's own two credentials,
     previously rendered here, are retired -- see
     docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md.
     Like llm-api-key/kafka-sasl-*, deliberately NOT auto-generated, since
     a random LDBWS key is meaningless. Rendered (possibly empty) whenever
     no railMcp.existingSecret is configured, so
     railmcp-deployment.yaml's secretKeyRefs always resolve. */}}
{{- if not .Values.railMcp.existingSecret -}}
{{- $_ := set $data "ldbws-departures-url" (.Values.railMcp.ldbws.departuresUrl | default "" | b64enc) -}}
```

(Only the comment and the two `discord-*` `set` lines are removed; the
`ldbws-departures-url` line and everything after it in this `if` block is
unchanged — reproduced above only to anchor the diff.)

- [ ] **Step 3: `_helpers.tpl` — delete the two Discord secret-key helpers**

Delete these two `define` blocks entirely:

```
{{- define "distant-signal.railMcpDiscordClientIdSecretKey" -}}
{{- if .Values.railMcp.existingSecret }}
{{- .Values.railMcp.existingSecretDiscordClientIdKey }}
{{- else }}
{{- print "discord-client-id" }}
{{- end }}
{{- end }}

{{- define "distant-signal.railMcpDiscordAllowedUserIdsSecretKey" -}}
{{- if .Values.railMcp.existingSecret }}
{{- .Values.railMcp.existingSecretDiscordAllowedUserIdsKey }}
{{- else }}
{{- print "discord-allowed-user-ids" }}
{{- end }}
{{- end }}
```

(The six `railMcpLdbws*SecretKey` helpers immediately after these two are
untouched.)

- [ ] **Step 4: `railmcp-deployment.yaml` — swap the two `secretKeyRef` env entries for plain values**

Replace:

```yaml
            - name: DISCORD_CLIENT_ID
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: {{ include "distant-signal.railMcpDiscordClientIdSecretKey" . }}
            - name: DISCORD_ALLOWED_USER_IDS
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: {{ include "distant-signal.railMcpDiscordAllowedUserIdsSecretKey" . }}
```

with:

```yaml
            # Plain config values, not secretKeyRefs -- an Authentik
            # group name is not a credential (Decision 3 of
            # docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md).
            # Consumed by the derived MCP service's own adapter (a
            # separate repository) to enforce the two access-group gates;
            # this chart only threads the names through.
            - name: MCP_USERS_GROUP
              value: {{ .Values.railMcp.accessGroups.mcpUsersGroup | quote }}
            - name: MCP_LIVE_BOARDS_GROUP
              value: {{ .Values.railMcp.accessGroups.mcpLiveBoardsGroup | quote }}
```

While already editing this file for this task, also fix the stale
comment the design spec flagged in passing (its own Open question/risk
6) — immediately above the `env:` block, this comment currently reads:

```yaml
            # In-cluster DNS name for this chart's own `api` Service -- no
            # new DS-side route or auth needed (Decision 4: every DS call
            # this service makes is anonymous).
```

Replace with:

```yaml
            # In-cluster DNS name for this chart's own `api` Service. NOTE
            # (2026-09-02): the "every DS call is anonymous" framing this
            # comment previously carried is stale -- the sibling adapter
            # work (see docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md's
            # own Coordination note) has the adapter call DS with a caller's
            # held session for TRUST-corroboration and groups lookups. Left
            # as a plain factual correction, not a design change to this
            # file -- see that spec for the actual current picture.
```

- [ ] **Step 5: Render and check**

Run:

```bash
helm template charts/distant-signal --set railMcp.enabled=true \
  --show-only templates/railmcp-deployment.yaml | grep -c DISCORD
```

Expected: `0`.

```bash
helm template charts/distant-signal --set railMcp.enabled=true \
  --show-only templates/railmcp-deployment.yaml \
  | grep -A1 "name: MCP_USERS_GROUP\|name: MCP_LIVE_BOARDS_GROUP"
```

Expected: both env vars present, `value: "mcp-users"` /
`value: "mcp-live-boards"` (the chart defaults).

```bash
helm lint charts/distant-signal
```

Expected: no new lint errors.

- [ ] **Step 6: Commit**

```bash
git add charts/distant-signal/values.yaml charts/distant-signal/templates/secret.yaml \
  charts/distant-signal/templates/_helpers.tpl charts/distant-signal/templates/railmcp-deployment.yaml
git commit -m "Retire railMcp.discord.*, add railMcp.accessGroups.* chart values"
```

---

### Task 8: `docker-compose.yml` — same retirement, compose side

**Files:**
- Modify: `docker-compose.yml`

**Interfaces:**
- Produces: `MCP_USERS_GROUP`/`MCP_LIVE_BOARDS_GROUP` env vars on the
  `rail-mcp` compose service, replacing the two `DISCORD_*` lines.
- **Depends on:** nothing else in this plan — parallelizable with Task 7.
  See the Coordination note at the top of this plan: the embedded-chatbot
  sibling plan's own Task 8 also edits this file's `rail-mcp:` service
  block (Redis/internal-secret wiring). **A human must sequence this
  against that plan.**

`docker-compose.yml`'s `rail-mcp:` service is already gated behind
`profiles: ["rail-mcp"]` (confirmed on `main` this session — this fix
landed independently of both this plan and its spec, dated in the file's
own comment as "fixed 2026-09-02"), so this task does not need to design
or touch that gating mechanism at all — only the two env var lines.

- [ ] **Step 1: Replace the two `DISCORD_*` lines**

Replace (`docker-compose.yml`, inside `rail-mcp:`'s `environment:` block):

```yaml
      DISCORD_CLIENT_ID: ${RAIL_MCP_DISCORD_CLIENT_ID}
      DISCORD_ALLOWED_USER_IDS: ${RAIL_MCP_DISCORD_ALLOWED_USER_IDS}
```

with:

```yaml
      # Plain, non-secret config -- an Authentik group name, not a
      # credential (Decision 3 of
      # docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md).
      # Defaults match charts/distant-signal/values.yaml's own
      # railMcp.accessGroups.* defaults.
      MCP_USERS_GROUP: ${RAIL_MCP_USERS_GROUP:-mcp-users}
      MCP_LIVE_BOARDS_GROUP: ${RAIL_MCP_LIVE_BOARDS_GROUP:-mcp-live-boards}
```

- [ ] **Step 2: Validate the compose file**

Run:

```bash
docker compose --profile rail-mcp config | grep -c DISCORD
```

Expected: `0`.

```bash
docker compose --profile rail-mcp config | grep -A1 "MCP_USERS_GROUP\|MCP_LIVE_BOARDS_GROUP"
```

Expected: both present, resolved to their default values
(`mcp-users`/`mcp-live-boards`) since `RAIL_MCP_USERS_GROUP`/
`RAIL_MCP_LIVE_BOARDS_GROUP` are unset in this environment.

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "Retire DISCORD_* env vars on rail-mcp, add MCP_USERS_GROUP/MCP_LIVE_BOARDS_GROUP"
```

---

### Task 9: `distant-signal-mcp` adapter — enforce the two gates (SEPARATE REPOSITORY)

**This task's files live in `distant-signal-mcp`, a completely separate
git repository from this one.** Do not attempt any step below from
inside this worktree's checkout — clone/enter `distant-signal-mcp`'s own
working copy and create a feature branch there, e.g.:

```bash
cd /path/to/distant-signal-mcp   # wherever that repo is checked out locally
git fetch origin
git checkout -b access-groups-gating origin/main   # or whatever base branch
                                                     # currently holds the
                                                     # landed adapter work --
                                                     # see below
```

**Before starting this task, re-read the Coordination note at the top of
this plan.** This task is written against the embedded-chatbot sibling
plan's own Task 5 (`src/oauth/internal.ts`'s
`POST /internal/complete-authorization` handler) and Task 7
(`src/oauth/middleware.ts`, applied to all six tool routes) as the two
concrete integration points — the most specific real information
available when this plan was written. **If that plan has not landed, has
landed with different file/function names, or a human has decided to
reconcile the two efforts differently, locate the functional equivalent
of each integration point instead of following the file paths below
literally:**

- **Gate 1's equivalent**: wherever the adapter turns a just-completed DS
  login into an issued MCP-scoped Bearer token (the last point before a
  token is handed back to the connecting MCP client).
- **Gate 2's equivalent**: wherever the adapter both lists the available
  MCP tools (`tools/list`) and dispatches a tool invocation
  (`tools/call`) for an already-token-holding caller.

**Contract this task depends on (stable regardless of the adapter's
internal shape, already landed by Tasks 5/7-8 above in the `distant-signal`
repo by the time this task runs):**

- `GET {DS_API_BASE_URL}/public/auth/session`, called with the header
  `Cookie: distant_signal_session=<raw session value the adapter already
  holds per the sibling plan's own Decision/Task 6>`, now returns JSON
  shaped `{ authenticated: boolean; id: string | null; email: string |
  null; name: string | null; groups: string[] }` — `groups` is always
  present, never omitted, empty array when none.
- Two env vars are available to the adapter's own process at boot:
  `MCP_USERS_GROUP` (default `"mcp-users"`) and `MCP_LIVE_BOARDS_GROUP`
  (default `"mcp-live-boards"`).

**Files (best-effort, contingent on the sibling plan's shape — see above):**
- Create: `src/oauth/accessGroups.ts` (pure gate logic — independent of
  the adapter's exact request/response plumbing, testable in isolation
  regardless of how the rest of the adapter is shaped)
- Modify: `src/config.ts` (read the two new env vars)
- Modify: `src/oauth/internal.ts` (or its functional equivalent — Gate 1)
- Modify: `src/oauth/middleware.ts` (or its functional equivalent — Gate 2)
- Test: `test/oauth-access-groups.test.ts`

**Interfaces:**
- Produces: `hasGroup(groups: string[], required: string): boolean`;
  `TOOL_GROUP_REQUIREMENTS: Record<string, string | null>` (tool name →
  the one additional group required beyond `mcp-users`, or `null` for the
  two tools requiring only `mcp-users`); `filterToolsForGroups(tools:
  string[], groups: string[], config: { mcpUsersGroup: string;
  mcpLiveBoardsGroup: string }): string[]`.
- **Depends on:** Task 5 (`groups` on `GET /auth/session`), Tasks 7-8
  (`MCP_USERS_GROUP`/`MCP_LIVE_BOARDS_GROUP` env vars) — and, outside
  this plan's control, the sibling adapter work per the Coordination
  note.

- [ ] **Step 1: Extend `src/config.ts`**

```ts
    accessGroups: {
        /** Whole-server gate -- the direct replacement for the old
         * DISCORD_ALLOWED_USER_IDS allowlist. See
         * docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
         * Decision 3 (in the distant-signal repo). */
        mcpUsersGroup: string;
        /** Feature gate, required IN ADDITION to mcpUsersGroup, for the
         * four tools hitting metered Darwin/LDBWS keys directly. */
        mcpLiveBoardsGroup: string;
    };
```

Add to `loadConfig`'s return (matching this file's existing
`required`/`env` helper pattern, with a default since both env vars are
optional):

```ts
        accessGroups: {
            mcpUsersGroup: env.MCP_USERS_GROUP ?? 'mcp-users',
            mcpLiveBoardsGroup: env.MCP_LIVE_BOARDS_GROUP ?? 'mcp-live-boards'
        },
```

- [ ] **Step 2: `src/oauth/accessGroups.ts` — pure gate logic**

```ts
/** The four tools that call Darwin/LDBWS directly against the operator's
 * own metered product keys -- see
 * docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md
 * Decision 3. `resolve_station` and `find_services` are intentionally
 * absent -- they require only the whole-server mcp-users gate. */
const LIVE_BOARD_TOOLS = new Set(['get_departures', 'get_arrivals', 'get_service_detail', 'plan_journey']);

export function requiresLiveBoardsGroup(toolName: string): boolean {
    return LIVE_BOARD_TOOLS.has(toolName);
}

export function hasGroup(groups: string[], required: string): boolean {
    return groups.includes(required);
}

/** Gate 1 (whole-server): true iff the caller may use the MCP server at
 * all. Enforced BEFORE issuing an MCP bearer token -- see this plan's
 * Task 9 Step 3/4 (or their functional equivalent). */
export function isEntitledToServer(groups: string[], mcpUsersGroup: string): boolean {
    return hasGroup(groups, mcpUsersGroup);
}

/** Gate 2 (per-tool): true iff an already-token-holding caller may use
 * this specific tool. Assumes isEntitledToServer already passed at grant
 * time (mcp-users membership) -- this only adds the live-boards check on
 * top, for the four tools that need it. */
export function isEntitledToTool(toolName: string, groups: string[], mcpLiveBoardsGroup: string): boolean {
    if (!requiresLiveBoardsGroup(toolName)) {
        return true;
    }
    return hasGroup(groups, mcpLiveBoardsGroup);
}

/** tools/list filtering -- a caller without mcp-live-boards simply never
 * sees get_departures/get_arrivals/get_service_detail/plan_journey in
 * the list (Error handling section of the design spec: "an AI assistant
 * simply never learns a tool it can't use exists"). */
export function filterToolsForGroups(allToolNames: string[], groups: string[], mcpLiveBoardsGroup: string): string[] {
    return allToolNames.filter((name) => isEntitledToTool(name, groups, mcpLiveBoardsGroup));
}
```

- [ ] **Step 3: `test/oauth-access-groups.test.ts`**

```ts
import { describe, expect, it } from 'vitest';
import {
    filterToolsForGroups,
    isEntitledToServer,
    isEntitledToTool,
    requiresLiveBoardsGroup
} from '../src/oauth/accessGroups.js';

const ALL_TOOLS = ['resolve_station', 'get_departures', 'get_arrivals', 'get_service_detail', 'find_services', 'plan_journey'];

describe('requiresLiveBoardsGroup', () => {
    it('is true for exactly the four metered-board tools', () => {
        expect(requiresLiveBoardsGroup('get_departures')).toBe(true);
        expect(requiresLiveBoardsGroup('get_arrivals')).toBe(true);
        expect(requiresLiveBoardsGroup('get_service_detail')).toBe(true);
        expect(requiresLiveBoardsGroup('plan_journey')).toBe(true);
    });

    it('is false for resolve_station and find_services', () => {
        expect(requiresLiveBoardsGroup('resolve_station')).toBe(false);
        expect(requiresLiveBoardsGroup('find_services')).toBe(false);
    });
});

describe('isEntitledToServer', () => {
    it('false for a caller with no groups at all', () => {
        expect(isEntitledToServer([], 'mcp-users')).toBe(false);
    });

    it('true once mcp-users is present, regardless of other groups', () => {
        expect(isEntitledToServer(['mcp-users'], 'mcp-users')).toBe(true);
        expect(isEntitledToServer(['some-other-group', 'mcp-users'], 'mcp-users')).toBe(true);
    });
});

describe('filterToolsForGroups / isEntitledToTool -- the three caller-group partitions the design spec's own Testing section names', () => {
    it('{} (no groups): would already have been denied a token at grant time -- this only covers the filtering behaviour in isolation', () => {
        expect(filterToolsForGroups(ALL_TOOLS, [], 'mcp-live-boards')).toEqual(['resolve_station', 'find_services']);
    });

    it('{mcp-users} only: tools/list returns exactly resolve_station/find_services', () => {
        expect(filterToolsForGroups(ALL_TOOLS, ['mcp-users'], 'mcp-live-boards')).toEqual([
            'resolve_station',
            'find_services'
        ]);
    });

    it('{mcp-users, mcp-live-boards}: tools/list returns all six', () => {
        expect(filterToolsForGroups(ALL_TOOLS, ['mcp-users', 'mcp-live-boards'], 'mcp-live-boards')).toEqual(ALL_TOOLS);
    });

    it('tools/call on a live-boards-gated tool by a caller lacking it is not entitled -- the 403 fallback case', () => {
        expect(isEntitledToTool('get_departures', ['mcp-users'], 'mcp-live-boards')).toBe(false);
    });

    it('tools/call on a live-boards-gated tool by an entitled caller succeeds', () => {
        expect(isEntitledToTool('get_departures', ['mcp-users', 'mcp-live-boards'], 'mcp-live-boards')).toBe(true);
    });

    it('tools/call on an ungated tool never depends on mcp-live-boards membership', () => {
        expect(isEntitledToTool('find_services', [], 'mcp-live-boards')).toBe(true);
    });
});
```

- [ ] **Step 4: Run the pure-logic tests**

Run: `npm test -- oauth-access-groups`
Expected: PASS (all cases above).

- [ ] **Step 5: Commit the pure logic + config on its own**

```bash
git add src/oauth/accessGroups.ts src/config.ts test/oauth-access-groups.test.ts
git commit -m "Add pure access-group gate logic (mcp-users, mcp-live-boards)"
```

- [ ] **Step 6: Wire Gate 1 in — before token issuance**

At the point identified above (sibling plan's `src/oauth/internal.ts`,
`POST /internal/complete-authorization` handler, or its functional
equivalent — the handler that turns a completed DS login into an issued
authorization code/token), add a groups lookup and the gate check
**before** the code/token is created. Sketch, adapt field/variable names
to whatever actually landed:

```ts
// After resolving the caller's raw DS session cookie value, before
// creating the authorization code:
const sessionResponse = await fetch(`${config.dsApiBaseUrl}/public/auth/session`, {
    headers: { Cookie: `distant_signal_session=${dsSessionCookieValue}` }
});
const session = (await sessionResponse.json()) as { authenticated: boolean; groups: string[] };

if (!isEntitledToServer(session.groups, config.accessGroups.mcpUsersGroup)) {
    // OAuth 2.1 / RFC 6749 §4.1.2.1 error redirect -- see Error handling
    // in docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md.
    // No code/token is created in this branch.
    const redirectUrl = new URL(pending.redirectUri);
    redirectUrl.searchParams.set('error', 'access_denied');
    redirectUrl.searchParams.set('state', pending.state);
    return res.json({ redirectUrl: redirectUrl.toString() });
}
```

If `GET /public/auth/session` itself fails (network error, DS down),
treat that identically to "not entitled" — fail closed, per the design
spec's Error handling section — do not fall through to issuing a token.

- [ ] **Step 7: Wire Gate 2 in — `tools/list` filtering and `tools/call` 403 fallback**

At the point identified above (sibling plan's `src/oauth/middleware.ts`,
applied to all six tool routes, or its functional equivalent), add:

- On `tools/list`: after resolving the caller's groups for the current
  bearer token (cached or freshly looked up per whatever the landed
  adapter's own caching strategy is — this plan does not resolve that
  trade-off, see the design spec's Open question/risk 3), call
  `filterToolsForGroups(allToolNames, groups, config.accessGroups.mcpLiveBoardsGroup)`
  and return only the filtered list.
- On `tools/call`: before dispatching to the tool implementation, call
  `isEntitledToTool(toolName, groups, config.accessGroups.mcpLiveBoardsGroup)`;
  if `false`, return a JSON-RPC error response carrying **HTTP 403** (not
  401 — reserved for "no/invalid bearer token at all") with a message
  naming the missing group requirement, e.g. `` `this tool requires the
  ${config.accessGroups.mcpLiveBoardsGroup} group` ``.

- [ ] **Step 8: Run the full adapter test suite**

Run: `npm test && npm run typecheck` (or whatever this repo's own
`package.json` scripts are named by the time this task runs — match
Task 1 of the sibling plan's own established convention).
Expected: PASS, including any table-driven gate tests the sibling plan's
own Task 7 (middleware) already established, now covering the two new
gates as well.

- [ ] **Step 9: Commit**

```bash
git add src/oauth/internal.ts src/oauth/middleware.ts   # or their actual paths
git commit -m "Enforce mcp-users/mcp-live-boards gates at grant time and per-tool"
```

- [ ] **Step 10: Open a PR in `distant-signal-mcp`'s own repository**, not
  this one. Note in the PR description that it depends on
  `distant-signal`'s Tasks 5, 7, and 8 (the `groups` field on
  `GET /auth/session` and the two `MCP_*_GROUP` env vars) being deployed
  to whatever environment this adapter is tested against.

---

### Task 10: Final verification (this repository only)

**Files:** none modified — verification only.

**Depends on:** Tasks 1-8 (this repository). Task 9's own verification
lives entirely in `distant-signal-mcp`'s repository and is not re-run
here.

- [ ] **Step 1: Full workspace build and test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: builds cleanly; all tests pass, including every new test added
in Tasks 2, 3, and 5.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings introduced by this plan's changes.

- [ ] **Step 3: Regression check — no Discord references remain**

Run:

```bash
grep -rn 'DISCORD\|discord' charts/distant-signal docker-compose.yml
```

Expected: no matches. (If this returns anything, it means Task 7 or 8
missed a reference — re-check against the file:line citations in those
tasks, re-verified against `main` this session.)

- [ ] **Step 4: Helm chart renders cleanly end-to-end**

Run:

```bash
helm lint charts/distant-signal
helm template charts/distant-signal --set railMcp.enabled=true --set devAuthentik.enabled=true \
  --set api.sso.issuerUrl=https://x --set api.sso.clientId=x --set api.sso.clientSecret=x \
  --set api.sso.redirectUrl=https://x --set api.sso.postLoginRedirectUrl=https://x >/dev/null
```

Expected: both succeed with no error (mirrors the minimal-required-values
invocation `docs/superpowers/plans/2026-08-29-dev-oidc-server.md`'s own
final verification already established for this chart).

- [ ] **Step 5: docker compose config renders cleanly**

Run: `docker compose --profile rail-mcp config >/dev/null`
Expected: succeeds with no error.

- [ ] **Step 6: Manually confirm the coordination items are still open, not silently resolved by this plan**

Re-read the Coordination note at the top of this plan. Confirm (this is a
manual check, not an automated one): whether the embedded-chatbot sibling
plan has landed in `distant-signal-mcp` yet, and if so, whether Task 9's
own integration points (Steps 6-7) still match reality or need
adjustment before that task is dispatched. This step exists so whoever
runs this plan doesn't mistake "Tasks 1-8 verified clean" for "the whole
feature is done" — Task 9, in a different repository, is the piece that
actually makes any of this enforceable end-to-end.
