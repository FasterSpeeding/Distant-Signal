# Internal Service Auth via OAuth2 Client Credentials Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `crates/api`'s single shared-secret `X-Internal-Token` gate on `private_router()` with Authentik-delegated OAuth2 Client Credentials auth: a shared token-fetch-and-cache client in `crates/common` used by all 8 real internal callers, local JWT/JWKS verification in `crates/api` (no per-request introspection), and a route-scoping table keyed on an Authentik `groups` claim rather than a DS-resolved identity enum. Config-only surface per caller (username/password pair per service, shared non-secret token URL/client_id/scope) in the Helm chart and `docker-compose.yml`. **Provisioning the actual Authentik service accounts/provider/groups is explicitly out of scope** — every task below only adds DS-side code and config that *consumes* whatever an operator provisions.

**Architecture:**

```
crates/common/src/oauth_client.rs   NEW -- OAuthCredentials, OAuthTokenCache
  (Task 1, foundational)             (fetch-and-cache client-credentials POST)
        │
        ▼
crates/common/src/ingest.rs          MODIFY -- post_batch/fetch_last_fetched take
  (Task 2, depends on Task 1)        &OAuthTokenCache, send Authorization: Bearer

crates/api/src/auth/internal_oauth.rs NEW -- ServiceClaims, ServiceTokenVerifier
  (Task 3, independent)               (JWKS cache + JWT verify, reusing
                                        openidconnect's own JWK primitives)
        │
        ▼
crates/api/src/data/config.rs        MODIFY -- internal_token field removed;
crates/api/src/app.rs                internal_oauth_* fields added; AppState
  (Task 4, depends on Task 3)        builds ServiceTokenVerifier + route table
        │
        ▼
crates/api/src/auth.rs               MODIFY -- require_internal_token replaced
crates/api/src/routes/mod.rs         by require_internal_oauth (Bearer parse +
  (Task 5, depends on Task 4)        verify + group check); private_router()
                                       layer swapped
        │
        ├─────────────┬──────────────────────┐
        ▼             ▼                      ▼
Task 6: 5 RDM/TfL  Task 7: trust-consumer  Task 8: schedule-ingest
pollers + ldbws +  (own queries.rs, 3      (hand-rolled POST, own
schedule-reference direct header calls)    direct header call)
(6 crates, common::ingest-based)
  (all three depend on Tasks 1-2)
        │             │                      │
        └─────────────┴──────────┬───────────┘
                                  ▼
                    Task 9: Helm chart config surface
                    Task 10: docker-compose.yml / dev.env.example / local.env.example
                    (both depend on Tasks 4, 6, 7, 8's final field names)
                                  │
                                  ▼
                    Task 11: final verification
```

**Tech Stack:** Rust (`crates/common`, `crates/api`, and all 8 real-caller crates), reusing `openidconnect 4.0.1`'s already-vendored JWK verification primitives in `crates/api` (no new crate added for JWT verification — see Task 3), `wiremock 0.6` (already used by `crates/enricher`) as a new dev-dependency in `crates/common` and `crates/api` for HTTP-mocked tests. Helm chart (`charts/distant-signal`), `docker-compose.yml` / `.env.example` files. No new crate, no database migration, no frontend change.

**Spec:** `docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md` — read in full before starting; this plan turns its Decisions into concrete tasks and does not re-litigate them. Cross-references below to "Decision N" refer to that document. **Read the "Corrections to the spec" section immediately below first** — several of the spec's own "Current relevant state" facts have changed since it was written, in ways that materially change this plan's scope versus the spec's.

## Corrections to the spec (re-verified against current `main`, this session)

This plan was written from a worktree that forked from `main` before the spec above was merged. Every citation below was independently re-verified by reading current `main`'s actual content (this worktree was fast-forwarded onto `main` — `git merge --ff-only main` — before any of this research began, so "current `main`" and "this worktree" are the same commit, `1837aef`, throughout this plan).

1. **`crates/api/src/auth.rs` re-confirmed unchanged from the spec's description**: still the single shared-secret `X-Internal-Token` scheme exactly as the spec's "Current relevant state" describes it — module doc lines 1–6, `require_internal_token` lines 20–36, `constant_time_eq` lines 46–56, five tests at lines 217–240 (`equal_tokens_match` etc.). `private_router()` (`crates/api/src/routes/mod.rs:58–75`, confirmed lines 58 def / 60–62 body) applies it via `.layer(middleware::from_fn_with_state(app, require_internal_token))` at line 62. `AppState::init()`'s non-empty guard is at `crates/api/src/app.rs:65–72`, word-for-word as quoted. **No drift from the spec here.**
2. **The reqwest 0.12/0.13.4 split re-confirmed**: `crates/api/Cargo.toml:32` pins `reqwest = { version = "0.12", ... }` with the exact "PINNED TO 0.12 ON PURPOSE" comment the spec quotes (`Cargo.toml:23–31`). `crates/common/Cargo.toml:11` and all 7 caller crates the spec knew about pin `reqwest = "0.13.4"`. **New finding**: `crates/schedule-reference/Cargo.toml:12` — a crate that did not exist when the spec was written — *also* pins `reqwest = "0.13.4"`, joining the other 7. The split, and Decision 4's reasoning against pulling `oauth2` into `crates/common`, both still hold exactly as the spec describes; the new crate simply adds an 8th data point in the same direction, not a different one.
3. **`crates/schedule-reference` now exists — the spec's "7 real callers, no `schedule-reference` crate exists yet" claim is stale.** This is the most consequential correction. The spec's own "Current relevant state" (lines 120–128) explicitly checked `find crates -maxdepth 1 -type d` and found no such crate, and wrote the whole document to accommodate its *future* addition as "one more row, everywhere a 7 appears." That crate has since landed on `main` (the STANOX/CRS work, merged after this spec) with its own `crates/schedule-reference/src/config.rs:32–33` `internal_token: String` field and its own `POST /private/stanox-crs` call via `common::ingest::post_batch` (`crates/schedule-reference/src/main.rs:109`). **There are 8 real callers today, not 7**: the five RDM/TfL pollers, `trust-consumer`, `schedule-ingest`, and `schedule-reference`. Every "7" in the spec (the config-field tables, the route-scoping table, the chart secret count) needs one more row throughout this plan.
4. **A new route, `GET`/`POST /stanox-crs`, already exists in `private_router()` and is missing from the spec's own route table entirely** (`crates/api/src/routes/ingest.rs:54–55`, `232–248`; merged into `private_router()` via `ingest::router()` same as every other route). Because the spec's investigation predates `schedule-reference`'s existence, its route table (9 rows) never considered this route at all. **This route has two distinct legitimate callers, not one**: `schedule-reference` `POST`s resolved STANOX/CRS rows (`main.rs:109`), and `trust-consumer` independently `GET`s the same endpoint on its own reload timer (`crates/trust-consumer/src/queries.rs:28–38`, `fetch_stanox_crs`, called from `main.rs:90`). This is a genuinely new shape the spec's Decision 3 route table never had to handle: every other row is "one route (or two) → one caller's group"; `/stanox-crs` is "one route → two different callers' groups, either one sufficient." Task 4 below builds the route-scoping table to support more than one allowed group per route entry for exactly this reason.
5. **`docker-compose.yml` does not run `schedule-reference` as a service at all** — confirmed by grep (`schedule-reference`/`schedule_reference` returns nothing in `docker-compose.yml`). This is a pre-existing gap unrelated to authentication (the Helm chart's `schedulefeed-deployment.yaml` *does* run it, as a third container in the `schedulefeed` Pod) and is **not** addressed by this plan — adding a new docker-compose service is a separate concern from migrating existing wiring. Task 10 below only touches the 8 service blocks (`api` + 7 callers) that already exist in `docker-compose.yml` today.
6. **Decision 2's own "Open questions/risks #1" is resolved by this plan, not left open**: whether `openidconnect`'s already-vendored JWK-verification primitives can be reused directly for a generic (non-ID-token) JWT signature check. Investigated directly against the vendored `openidconnect-4.0.1` source this session (`~/.cargo/registry/src/.../openidconnect-4.0.1/src/`): `openidconnect::core::CoreJsonWebKeySet` (`= JsonWebKeySet<CoreJsonWebKey>`) exposes a public `fetch_async(url: &JsonWebKeySetUrl, http_client) -> Result<Self, _>` (`src/types/jwks.rs:104–121`) that reuses the exact `AsyncHttpClient` trait `crates/api` already satisfies with its reqwest-0.12 client, and a public `.keys() -> &Vec<K>`. Each `CoreJsonWebKey` implements the `openidconnect::JsonWebKey` trait (re-exported at the crate root, `src/lib.rs:713–716`), which has a `verify_signature(&self, alg: &SigningAlgorithm, message: &[u8], signature: &[u8]) -> Result<(), SignatureVerificationError>` method (`src/types/jwk.rs:42–51`) — a **generic, ID-token-decoupled signature check**, exactly the primitive the spec's Open Question 1 asked whether it existed. **Decision: reuse it directly** (Task 3 below) rather than adding a new dependency like `jsonwebtoken`. This also lets test fixtures sign JWTs with `openidconnect::core::CoreRsaPrivateSigningKey::from_pem` (`src/core/jwk/mod.rs:549–556`, confirmed to load a PKCS#1 PEM) and its `PrivateSigningKey::sign`/`as_verification_key()` methods — the whole test fixture pipeline (mint a token, serve it via a mocked JWKS, verify it) stays inside a dependency `crates/api` already has, with zero new crates.
7. **No dual-acceptance/migration-window mechanism is included in this plan.** The spec's own "Explicitly out of scope" list already flags this as unresolved ("Open questions/risks #6"), and the task that produced this plan's scope did not ask for one (unlike the *reverted* design, whose own Decision 5 built one for its own, different scheme). This plan is a single coordinated cutover: `X-Internal-Token`/`INTERNAL_TOKEN`/`secrets.internalToken` are removed outright (config field, chart Secret key, `docker-compose.yml` var, `.env.example` entries), not kept alongside the new mechanism. See Global Constraints for the operational implication (a rollout-ordering note, not a coded mechanism).

## Global Constraints

- **No dual-acceptance window (see correction 7 above).** No task adds a "Legacy" identity, no task keeps `X-Internal-Token`/`INTERNAL_TOKEN` readable anywhere after Tasks 4–10 land. This is a flag-day cutover.
- **Rollout-ordering risk, stated but not coded around**: this chart's Deployments roll independently with no ordering guarantee (the same fact the spec's own Decision-2 discussion and the reverted design's Decision 5 both cite). Between `api`'s new pods (expecting `Authorization: Bearer`) rolling out and any given caller's pods (still sending the old header, or not yet holding valid OAuth2 credentials) catching up, that caller's `/private/*` calls fail with `401` until it rolls too. This is accepted, not designed around: every real caller already retries on its own poll/reload cadence (`common::ingest::time_until_next_poll`'s existing "log warning, safe fallback" posture; `trust-consumer`'s existing `ERROR_BACKOFF` cycle; `schedule-ingest`'s existing "files stay in `storage_dir`, retry next cycle" posture) — a brief post-deploy gap self-heals without data loss. Operators should provision the Authentik-side service accounts (out of scope) *before* rolling this chart version, so no caller pod ever starts with credentials that don't yet resolve.
- **`api` never holds any of the 8 services' own OAuth2 credentials** (Decision 6). Only the shared, non-secret `internal_oauth_issuer_url`/`internal_oauth_client_id` (expected `aud`) and 8 non-secret group-name fields live in `api`'s own config. No task adds a per-service secret to `crates/api`'s `ServiceArguments`.
- **The route-scoping table lives in `crates/api`, built once at `AppState::init()` from config — no database, no dynamic runtime edit.** Table entries map a route prefix to **one or more** required group names (correction 4 above: `/stanox-crs` needs two). A request passes if the verified token's `groups` claim contains *any* of the route's required groups.
- **Zero I/O on `require_internal_oauth`'s hot path in the common case.** JWT signature verification is pure/in-process once the JWKS is cached (Decision 2); a JWKS (re)fetch only happens on a `kid` cache-miss, exactly mirroring `crates/api/src/auth/oidc.rs`'s existing lazy-discovery posture for the human-login path.
- **`crates/common::oauth_client` is hand-rolled, not built on the `oauth2` crate** (Decision 4, correction 2 above still holds). One `POST`, form-encoded body, one JSON response.
- **Coordination note — a separate, concurrent effort touches overlapping files.** `docs/superpowers/plans/2026-09-02-mcp-server-oauth-access-groups.md` (human-login "access groups" work, a different code path: ID tokens, browser-mediated, `crates/api/src/auth/oidc.rs`'s `CoreClient`) also modifies `crates/api/src/auth.rs` (its own Task 4, adding a `groups` field to `AuthenticatedUser` — a different struct in the same file this plan's Task 5 edits), and also touches `charts/distant-signal/values.yaml`, `templates/secret.yaml`, `templates/_helpers.tpl`, and `docker-compose.yml` (its own Tasks 6–8, for `railMcp.accessGroups.*`, unrelated values-tree keys). **No task in this plan modifies `AuthenticatedUser`, `oidc.rs`, or any `railMcp.*`/user-groups value** — the overlap is file-level only (both plans add content to the same files in different sections), not logic-level. A human should sequence which of these two plans' Task-5-equivalent `auth.rs` edits and which chart-file edits land first, and rebase the other, rather than executing both concurrently against the same branch.
- **Parallelizable tasks:** Task 1 is foundational. Task 2 depends on Task 1. Task 3 is independent (no dependency on 1/2). Task 4 depends on Task 3. Task 5 depends on Task 4. Tasks 6, 7, 8 each depend on Tasks 1–2 (they consume `crates/common::oauth_client`/the updated `ingest` module) but are mutually independent (disjoint crates) — dispatch in parallel once Task 2 lands. Tasks 9 and 10 depend on the final field names from Tasks 4, 6, 7, 8 (not on Rust code compiling) — dispatch once those tasks' `config.rs`/`ServiceArguments` field names are fixed, in parallel with each other. Task 11 depends on everything.

---

### Task 1: `crates/common::oauth_client` — token-fetch-and-cache client

**Files:**
- Create: `crates/common/src/oauth_client.rs`
- Modify: `crates/common/src/lib.rs` (register the module)
- Modify: `crates/common/Cargo.toml` (add `wiremock` dev-dependency)

**Interfaces:**
- Produces: `pub struct OAuthCredentials { pub token_url: String, pub client_id: String, pub scope: String, pub username: String, pub password: String }`; `pub struct OAuthTokenCache` with `pub fn new(credentials: OAuthCredentials) -> Self` and `pub async fn get_token(&self, client: &reqwest::Client) -> anyhow::Result<String>`.
- Consumed by: Task 2 (`post_batch`/`fetch_last_fetched` take `&OAuthTokenCache`), Tasks 6–8 (every real caller's `main.rs` constructs one `OAuthTokenCache` from its own config at startup and threads a reference through its poll/reload loop).
- **Depends on:** nothing — foundational.

- [ ] **Step 1: Add the `wiremock` dev-dependency**

In `crates/common/Cargo.toml`, add a `[dev-dependencies]` section (none exists yet):

```toml
[dev-dependencies]
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "time"] }
wiremock = "0.6"
```

(`tokio` is already a workspace dependency of every binary crate but not of `common` itself — `common`'s own `Cargo.toml` has no `[dependencies]` entry for `tokio`, only `reqwest`'s async runtime is pulled in transitively. Tests need their own `#[tokio::test]` runtime.)

- [ ] **Step 2: Create `crates/common/src/oauth_client.rs`**

```rust
//! Client-credentials OAuth2 token fetch + cache, shared by every real
//! internal caller of `api`'s `/private/*` routes. Hand-rolled (not the
//! `oauth2` crate) -- see
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
//! Decision 4: `crates/api` pins reqwest 0.12 (required by
//! `oauth2 5.0`/`openidconnect 4.0`'s `AsyncHttpClient` impl), while
//! `common` and every one of its 8 real callers pin reqwest 0.13.4 --
//! pulling `oauth2` into `common` would add a second, incompatible
//! reqwest major version to every caller's dependency tree, with no way
//! to share one `reqwest::Client` instance between a caller's normal HTTP
//! calls and its token-exchange calls. This is one `POST` with a small
//! form-encoded body and one JSON response -- a narrow, fully
//! RFC-6749-§4.3.2-specified shape, well within this codebase's
//! existing hand-roll-narrow-things posture (see
//! `crates/api/src/auth.rs`'s `constant_time_eq`/`parse_cookie`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Every real caller's own OAuth2 client-credentials config -- mirrors the
/// design's Decision 6 field table exactly: `token_url`/`client_id`/`scope`
/// are shared (the same value repeated per binary, like `INTERNAL_TOKEN`
/// was before this design), `username`/`password` are per-service and are
/// the actual secret.
#[derive(Clone)]
pub struct OAuthCredentials {
    pub token_url: String,
    pub client_id: String,
    pub scope: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct CachedToken {
    access_token: String,
    /// The instant this cache entry should be treated as stale -- NOT the
    /// raw `expires_in` deadline. Refreshed early (`REFRESH_MARGIN`) so a
    /// caller essentially never presents an already-expired token to
    /// `api` in the steady state.
    refresh_at: Instant,
}

/// Refresh this many seconds before the token's own `expires_in` -- a
/// fixed safety margin, matching this codebase's existing preference for
/// a flat constant over a percentage-of-lifetime calculation (see
/// `crates/common::ingest`'s own `duration_until_next_poll`, a similarly
/// fixed-window design). If a token's own `expires_in` is shorter than
/// this margin, `Duration::saturating_sub` clamps the result to zero --
/// the very next call refetches, never underflows or panics.
const REFRESH_MARGIN: Duration = Duration::from_secs(30);

/// Caches the last-fetched access token and its refresh deadline. Guarded
/// by a `std::sync::Mutex`, not `tokio::sync::Mutex`: the critical section
/// (checking/updating the cached value) never awaits while holding the
/// lock -- the token-fetch POST itself happens outside the guard, in
/// `fetch_token` -- so a blocking mutex is correct and simpler.
pub struct OAuthTokenCache {
    credentials: OAuthCredentials,
    cached: Mutex<Option<CachedToken>>,
}

impl OAuthTokenCache {
    pub fn new(credentials: OAuthCredentials) -> Self {
        Self { credentials, cached: Mutex::new(None) }
    }

    /// Returns a currently-valid bearer token: the cached one if it still
    /// has comfortable headroom before its own refresh deadline
    /// (`fresh_cached_token`), or a freshly fetched one otherwise
    /// (re-cached for the next call). Callers pass their own
    /// `reqwest::Client` -- this type holds no client of its own, matching
    /// every existing `common::ingest` call site's shape (client already
    /// threaded through as a parameter).
    pub async fn get_token(&self, client: &reqwest::Client) -> anyhow::Result<String> {
        if let Some(token) = self.fresh_cached_token() {
            return Ok(token);
        }
        let (access_token, expires_in) = self.fetch_token(client).await?;
        let refresh_at =
            Instant::now() + Duration::from_secs(expires_in).saturating_sub(REFRESH_MARGIN);
        let token_for_return = access_token.clone();
        *self.cached.lock().expect("oauth token cache mutex poisoned") =
            Some(CachedToken { access_token, refresh_at });
        Ok(token_for_return)
    }

    fn fresh_cached_token(&self) -> Option<String> {
        let guard = self.cached.lock().expect("oauth token cache mutex poisoned");
        let cached = guard.as_ref()?;
        (Instant::now() < cached.refresh_at).then(|| cached.access_token.clone())
    }

    async fn fetch_token(&self, client: &reqwest::Client) -> anyhow::Result<(String, u64)> {
        let response = client
            .post(&self.credentials.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.credentials.client_id.as_str()),
                ("username", self.credentials.username.as_str()),
                ("password", self.credentials.password.as_str()),
                ("scope", self.credentials.scope.as_str()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("oauth2 token fetch failed: {status} {text}");
        }
        let body: TokenResponse = response.json().await?;
        Ok((body.access_token, body.expires_in))
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn credentials(token_url: String) -> OAuthCredentials {
        OAuthCredentials {
            token_url,
            client_id: "distant-signal-internal".to_string(),
            scope: "groups".to_string(),
            username: "svc-poller-incidents".to_string(),
            password: "app-password".to_string(),
        }
    }

    async fn mock_token_endpoint(server: &MockServer, expires_in: u64, expect_calls: u64) {
        Mock::given(method("POST"))
            .and(path("/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake-jwt-access-token",
                "expires_in": expires_in,
                "token_type": "Bearer",
            })))
            .expect(expect_calls)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_fresh_cached_token_is_reused_not_refetched() {
        let server = MockServer::start().await;
        mock_token_endpoint(&server, 300, 1).await;
        let cache = OAuthTokenCache::new(credentials(format!("{}/token/", server.uri())));
        let client = reqwest::Client::new();

        let first = cache.get_token(&client).await.unwrap();
        let second = cache.get_token(&client).await.unwrap();

        assert_eq!(first, "fake-jwt-access-token");
        assert_eq!(second, "fake-jwt-access-token");
        // wiremock's `.expect(1)` (asserted on Drop) fails the test if the
        // mock was hit more than once -- the real assertion here.
    }

    #[tokio::test]
    async fn a_token_near_its_own_expiry_triggers_a_fresh_fetch() {
        let server = MockServer::start().await;
        // expires_in (5s) is well under REFRESH_MARGIN (30s), so
        // `refresh_at` saturates to "now" -- the cached entry is
        // immediately stale, and the second call must refetch.
        mock_token_endpoint(&server, 5, 2).await;
        let cache = OAuthTokenCache::new(credentials(format!("{}/token/", server.uri())));
        let client = reqwest::Client::new();

        cache.get_token(&client).await.unwrap();
        cache.get_token(&client).await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_fetch_returns_err_and_does_not_poison_the_cache_for_the_next_call() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token/"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        mock_token_endpoint(&server, 300, 1).await;
        let cache = OAuthTokenCache::new(credentials(format!("{}/token/", server.uri())));
        let client = reqwest::Client::new();

        let first = cache.get_token(&client).await;
        assert!(first.is_err(), "the mocked 500 must surface as an Err, not a panic");

        let second = cache.get_token(&client).await;
        assert_eq!(second.unwrap(), "fake-jwt-access-token", "a failed fetch must not poison the cache -- the next call retries cleanly");
    }
}
```

- [ ] **Step 3: Register the module**

In `crates/common/src/lib.rs`, change line 11-12:

```rust
pub mod ingest;
pub mod metrics;
```

to:

```rust
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p common oauth_client::`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/common/Cargo.toml crates/common/src/oauth_client.rs crates/common/src/lib.rs
git commit -m "Add crates/common::oauth_client: client-credentials token fetch + cache"
```

---

### Task 2: `crates/common::ingest` — wire format swap to `Authorization: Bearer`

**Files:**
- Modify: `crates/common/src/ingest.rs`

**Interfaces:**
- Consumes: `oauth_client::OAuthTokenCache::get_token` (Task 1).
- Produces: `post_batch`/`fetch_last_fetched`/`time_until_next_poll` now take `tokens: &crate::oauth_client::OAuthTokenCache` instead of `internal_token: &str`. `INTERNAL_TOKEN_HEADER` constant removed.
- Consumed by: Tasks 6–8 (every real caller's call sites).
- **Depends on:** Task 1.

- [ ] **Step 1: Rewrite the module doc and remove `INTERNAL_TOKEN_HEADER`**

Replace `crates/common/src/ingest.rs:1–22`:

```rust
//! Shared HTTP ingestion contract between the RDM pollers
//! (`crates/poller-incidents`, `crates/poller-stations`, `crates/poller-tocs`,
//! `crates/poller-ldbws`), plus `crates/poller-tfl` (not an RDM feed, but a
//! `post_batch`/`time_until_next_poll` consumer all the same), and the `api`
//! crate's `/private/*` endpoints (`crates/api/src/routes/ingest.rs`, gated
//! by `crates/api/src/auth.rs`).
//!
//! Single source of truth for the POST-batch-and-log pattern every real
//! caller repeats once per poll/reload cycle. Every request carries a
//! standard `Authorization: Bearer <token>` header (RFC 6750), the token
//! obtained from `crate::oauth_client::OAuthTokenCache` -- see
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
//! Decision 5. Previously this carried a bespoke `X-Internal-Token` shared
//! secret via a custom header; that scheme is retired, not kept alongside
//! this one (no dual-acceptance window -- see that document's Decision 5
//! and this plan's own Global Constraints).

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::oauth_client::OAuthTokenCache;

/// Header RDM uses for API-key auth, per RSPS5050 P-03-00 Rev A. How
/// confidently this is corroborated varies per poller/product — see each
/// poller's `main.rs` module docs for the specific gap, if any. Unrelated
/// to internal-service auth (this is RDM's own upstream credential, not a
/// credential DS's own `/private/*` routes check).
pub const RDM_AUTH_HEADER_NAME: &str = "x-apikey";
```

- [ ] **Step 2: Rewrite `post_batch`**

Replace `crates/common/src/ingest.rs`'s `post_batch` (now shifted a few lines down, still the first function after the constant):

```rust
/// POSTs `items` as a JSON array to `url` with a fresh
/// `Authorization: Bearer` token from `tokens`, then logs and returns
/// `Ok(())` on a 2xx response, or bails with an `anyhow::Error` (including
/// status + response body) otherwise.
///
/// `noun` is used only in the success log line (e.g. `"incidents"`,
/// `"stations"`, `"tocs"`) — callers pass their own plural label.
pub async fn post_batch<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    items: &[T],
    noun: &str,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(items)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(count = items.len(), "posted {noun} to ingestion API");
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("ingestion POST failed: {status} {text}");
    }
}
```

- [ ] **Step 3: Rewrite `time_until_next_poll` and `fetch_last_fetched`**

Replace their signatures and bodies (`LastFetchedResponse` and `duration_until_next_poll` are unchanged):

```rust
pub async fn time_until_next_poll(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    poll_interval: Duration,
) -> Duration {
    let fetched_at = match fetch_last_fetched(client, url, tokens).await {
        Ok(fetched_at) => fetched_at,
        Err(err) => {
            tracing::warn!(error = ?err, "could not determine last-fetch time; polling immediately");
            return Duration::ZERO;
        }
    };
    duration_until_next_poll(fetched_at, Utc::now(), poll_interval)
}

async fn fetch_last_fetched(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    let body: LastFetchedResponse = response.json().await?;
    Ok(body.fetched_at)
}
```

- [ ] **Step 4: Update the existing unit tests' imports**

`duration_until_next_poll`'s four existing tests (`crates/common/src/ingest.rs`'s `#[cfg(test)] mod tests`) call the pure function directly and need no changes — confirm `cargo test -p common ingest::` still compiles and passes as-is after Steps 1–3 (these tests don't touch `post_batch`/`fetch_last_fetched`/`time_until_next_poll` at all).

- [ ] **Step 5: Build and test**

Run: `cargo build -p common && cargo test -p common`
Expected: `common` itself builds (its own crate has no caller of the now-changed signatures); the 4 pre-existing `duration_until_next_poll` tests plus Task 1's 3 `oauth_client` tests all pass. **Every downstream crate (`poller-*`, `trust-consumer`, `schedule-ingest`, `schedule-reference`) now fails to build** until Tasks 6–8 update their own call sites — this is expected and resolved by those tasks, not a regression to fix here.

- [ ] **Step 6: Commit**

```bash
git add crates/common/src/ingest.rs
git commit -m "Swap crates/common::ingest's wire format from X-Internal-Token to Authorization: Bearer"
```

---

### Task 3: `crates/api/src/auth/internal_oauth.rs` — JWKS cache + JWT verification (pure)

**Files:**
- Create: `crates/api/src/auth/internal_oauth.rs`
- Modify: `crates/api/src/auth.rs` (register the module)
- Modify: `crates/api/Cargo.toml` (add `wiremock` dev-dependency)

**Interfaces:**
- Produces: `pub struct ServiceClaims { pub sub: String, pub iss: String, pub aud: String, pub exp: i64, pub groups: Vec<String> }`; `pub enum VerifyError { Malformed, UnknownKey, Invalid }`; `pub struct ServiceTokenVerifier` with `pub fn new(issuer_url: String, expected_audience: String) -> anyhow::Result<Self>` and `pub async fn verify(&self, token: &str) -> Result<ServiceClaims, VerifyError>`.
- Consumed by: Task 4 (`AppState` holds one `ServiceTokenVerifier`), Task 5 (`require_internal_oauth` calls `.verify(...)`).
- **Depends on:** nothing — pure, independent of config/`AppState` wiring. Can run in parallel with Tasks 1–2.

No dependency is added for JWT verification — see this plan's "Corrections to the spec" #6: `openidconnect 4.0.1` (already an `api` dependency) exposes `openidconnect::core::CoreJsonWebKeySet::fetch_async`, `openidconnect::JsonWebKey::verify_signature`, and `openidconnect::core::CoreProviderMetadata::discover_async` (already used identically by `crates/api/src/auth/oidc.rs`'s `OidcClient::client()` for the human-login path) — all reused directly here.

- [ ] **Step 1: Add the `wiremock` dev-dependency**

`crates/api/Cargo.toml` has no `[dev-dependencies]` section today. Add one at the end of the file:

```toml

[dev-dependencies]
wiremock = "0.6"
```

- [ ] **Step 2: Create `crates/api/src/auth/internal_oauth.rs`**

```rust
//! Verifies an internal-service OAuth2 client-credentials access token
//! (the `Authorization: Bearer` header on a `/private/*` request) against
//! Authentik's JWKS, fetched via standard OIDC discovery and cached
//! in-process. See
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
//! Decision 2.
//!
//! Deliberately reuses `openidconnect::core::CoreJsonWebKeySet`/
//! `CoreJsonWebKey`/`openidconnect::JsonWebKey::verify_signature` --
//! already an `api` dependency for the human-login path
//! (`crate::auth::oidc`) -- rather than adding a new JWT-verification
//! crate. `CoreJsonWebKey::verify_signature` is a generic
//! "verify this signature over this message with this key" primitive,
//! decoupled from `CoreIdTokenVerifier`'s ID-token-specific semantics
//! (nonce, `at_hash`, etc.), which this module has no use for -- claim
//! checks (`exp`/`iss`/`aud`) are done by hand below, which is why this
//! module still counts as "narrow, hand-rolled logic on top of a real
//! cryptography dependency" rather than "hand-rolled cryptography":
//! `verify_signature` does the actual signature math; everything here is
//! base64/JSON plumbing and string comparisons.

use std::collections::HashMap;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreJsonWebKey, CoreJsonWebKeySet, CoreProviderMetadata};
use openidconnect::{IssuerUrl, JsonWebKey as _, JsonWebKeySetUrl};
use serde::Deserialize;

/// The claims this design's route-scoping check reads off a verified
/// client-credentials access token (Decision 3). `groups` defaults to
/// empty when the claim is entirely absent from the token -- never
/// treated as "unscoped/allow everything" (see the route-scoping check in
/// `crate::auth::require_internal_oauth`, Task 5).
///
/// `aud` is modeled as a plain `String`, matching the spec's own stated
/// assumption (Open Question 2: "almost certainly the provider's own
/// `client_id`... not confirmed against a real emitted token"). If a real
/// Authentik-issued token turns out to encode `aud` as a JSON array
/// instead of a bare string, this struct's `aud` field needs to become
/// `Vec<String>` (or an enum accepting either shape) and the audience
/// check in `verify` below needs to check membership instead of equality
/// -- flagged here as the concrete, single place that assumption would
/// need revisiting.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceClaims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: openidconnect::core::CoreJwsSigningAlgorithm,
    kid: Option<String>,
}

/// Every failure mode collapses to one of these three -- `require_internal_oauth`
/// (Task 5) maps all three to a `401`, deliberately not distinguishing
/// which check failed to a caller that isn't yet a proven-valid identity
/// (see the design doc's Error handling section).
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Not three '.'-separated segments, invalid base64, or invalid JSON
    /// in the header/payload.
    Malformed,
    /// No `kid` in the header, or the `kid` still isn't present in the
    /// JWKS after one refetch attempt (including a refetch that itself
    /// failed, e.g. Authentik unreachable).
    UnknownKey,
    /// Signature, `exp`, `iss`, or `aud` failed verification.
    Invalid,
}

/// Fetches (and caches) Authentik's JWKS for the internal-service OAuth2
/// provider, and verifies bearer tokens against it. JWKS endpoint learned
/// via standard OIDC discovery against `issuer_url` -- the same mechanism
/// `crate::auth::oidc::OidcClient` already uses for the human-login flow
/// (Decision 6) -- rather than hardcoding Authentik's own
/// `/application/o/<slug>/jwks/` URL convention. Discovery is lazy (first
/// use, not construction), mirroring `OidcClient`'s own documented
/// posture, so a briefly-unreachable Authentik at `api` startup cannot
/// fail construction or crash-loop the pod.
pub struct ServiceTokenVerifier {
    issuer_url: String,
    expected_audience: String,
    http_client: reqwest::Client,
    jwks_uri: tokio::sync::OnceCell<JsonWebKeySetUrl>,
    keys: tokio::sync::RwLock<HashMap<String, CoreJsonWebKey>>,
}

impl ServiceTokenVerifier {
    pub fn new(issuer_url: String, expected_audience: String) -> Result<Self> {
        IssuerUrl::new(issuer_url.clone()).context("invalid internal_oauth_issuer_url")?;
        // Same redirect-policy rationale as `OidcClient::new` -- an HTTP
        // client that transparently follows redirects during discovery or
        // the JWKS fetch could be tricked into fetching an unintended
        // internal URL.
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build internal-oauth JWKS HTTP client")?;
        Ok(Self {
            issuer_url,
            expected_audience,
            http_client,
            jwks_uri: tokio::sync::OnceCell::new(),
            keys: tokio::sync::RwLock::new(HashMap::new()),
        })
    }

    async fn jwks_uri(&self) -> Result<&JsonWebKeySetUrl> {
        self.jwks_uri
            .get_or_try_init(|| async {
                let issuer = IssuerUrl::new(self.issuer_url.clone())?;
                let metadata = CoreProviderMetadata::discover_async(issuer, &self.http_client)
                    .await
                    .context("internal-service OIDC discovery failed")?;
                Ok::<_, anyhow::Error>(metadata.jwks_uri().clone())
            })
            .await
    }

    async fn refresh_keys(&self) -> Result<()> {
        let uri = self.jwks_uri().await?.clone();
        let jwks = CoreJsonWebKeySet::fetch_async(&uri, &self.http_client)
            .await
            .context("failed to fetch internal-oauth JWKS")?;
        let mut map = HashMap::new();
        for key in jwks.keys() {
            if let Some(kid) = key.key_id() {
                map.insert(kid.as_str().to_string(), key.clone());
            }
        }
        *self.keys.write().await = map;
        Ok(())
    }

    async fn key_for_kid(&self, kid: &str) -> Result<CoreJsonWebKey, VerifyError> {
        if let Some(key) = self.keys.read().await.get(kid) {
            return Ok(key.clone());
        }
        // kid not cached -- refetch exactly once. Guards against an
        // infinite refetch loop on a persistently-unknown kid (e.g. a
        // forged token), and matches Decision 2's stated caching design.
        if self.refresh_keys().await.is_err() {
            return Err(VerifyError::UnknownKey);
        }
        self.keys.read().await.get(kid).cloned().ok_or(VerifyError::UnknownKey)
    }

    /// Verifies `token`'s signature against the cached (or freshly
    /// fetched) JWKS, then its `exp`/`iss`/`aud`, returning the parsed
    /// claims only if every check passes.
    pub async fn verify(&self, token: &str) -> Result<ServiceClaims, VerifyError> {
        let mut parts = token.split('.');
        let (Some(header_b64), Some(payload_b64), Some(sig_b64), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(VerifyError::Malformed);
        };

        let header_bytes = URL_SAFE_NO_PAD.decode(header_b64).map_err(|_| VerifyError::Malformed)?;
        let header: JwtHeader = serde_json::from_slice(&header_bytes).map_err(|_| VerifyError::Malformed)?;
        let kid = header.kid.ok_or(VerifyError::Malformed)?;
        let signature = URL_SAFE_NO_PAD.decode(sig_b64).map_err(|_| VerifyError::Malformed)?;

        let key = self.key_for_kid(&kid).await?;
        let signing_input = format!("{header_b64}.{payload_b64}");
        key.verify_signature(&header.alg, signing_input.as_bytes(), &signature)
            .map_err(|_| VerifyError::Invalid)?;

        let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).map_err(|_| VerifyError::Malformed)?;
        let claims: ServiceClaims =
            serde_json::from_slice(&payload_bytes).map_err(|_| VerifyError::Malformed)?;

        if claims.iss != self.issuer_url {
            return Err(VerifyError::Invalid);
        }
        if claims.aud != self.expected_audience {
            return Err(VerifyError::Invalid);
        }
        if claims.exp <= chrono::Utc::now().timestamp() {
            return Err(VerifyError::Invalid);
        }
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use openidconnect::JsonWebKeyId;
    use openidconnect::core::{CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey};
    use openidconnect::{JsonWebKey as _, PrivateSigningKey};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// Test-only 2048-bit RSA keypair, generated once for this plan
    /// (`openssl genrsa -out priv.pem 2048 && openssl rsa -in priv.pem
    /// -traditional -out priv-pkcs1.pem`). PKCS#1 format -- required by
    /// `CoreRsaPrivateSigningKey::from_pem`, which calls
    /// `rsa::RsaPrivateKey::from_pkcs1_pem` internally, NOT the PKCS#8
    /// format `openssl genrsa`'s default output produces on some OpenSSL
    /// versions -- confirmed directly against the vendored
    /// `openidconnect-4.0.1` source this plan was written against. Never
    /// Authentik's real key.
    const TEST_RSA_PRIVATE_KEY_PEM: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAkXH3+3kYxgn6JADaRM0Wv8i3bo0vdLkuwI0LR00aDYiN6XGC
iSPnr1AfSPlNSQxNQ2q+xyRiLnAwkJ9bcnuHNR818Ok984u3k2nHI+fg5MVrAkcH
Ytk+OIrnVsAxFZjG+vBN9gVbrkiQoBVAZKJ7D6DGAXZnexri7FGR5ttnASvwtzBt
ZfzYHca87Uqk2jLd13a2GgTlrXA3HAwQ/ZDHu8JOPBx4zP3OK64LnG7ajK3NC+s6
PlhOFpJrJI5t1wyFz6gjtHNnijtbi4XDxKUL+Vl5zFWj/QvZkgC2kcBvTv3uYVhu
xyKsx/7MifbbDpHWgEHtfzsIi33gT/WwvjPnuQIDAQABAoIBAA8MD5VB2n5xPXfo
agHF3ALRulR4ISmTbP2juep8VLkuCvcp9KYeg6jUrQqTgYY7UmpVSqZ3TPxemU+l
BO92TdnWQEeLwd/b8Q08W3YLVm4klHpjAdBysZK6Ss5j9NALWG6mr3zH4iEeRcQi
CWGVQ7kCr5Qq0prfAGIQMFGGGlp5sDAkUTuDNmaZuvmLOWy9jpzgwwYNeYbvk+jH
+PbxNuPedRWAii43FY9c3/dT4qx1esCVYRQ4FvhNljE1+a/7rZXDpoPVDcizhNp2
oStI2b/SgSYlyrBmoCPnyLeYIWPxvgxaftU3ArRVtC6RYBlaI7ena3+z6fjl0EXN
7D84xBcCgYEAw388MOeEYTsfOBH9BDNz03RWPsplWAvE8l35Rxbq6n9a46+dWOaj
eiI9Hh/cAgbCmUp0PFucoXP/rPq+VcN1c9bZQc9YShW8UILK09RjpapXUIi4jOcc
GrVAFdKgMZD9CIG3s//CNlgiPmohUyh0Umurd1ScNB8Y/PodWNec/csCgYEAvnU/
EC4H7RQmKpM+D2g2+buCL3QaYXJHoHqX2sZ9q/WtjUyuqQlwKq0GzhPcE2ewN6x9
9UX4s6MC9rGH8aQq3j2OGXLxyRIaKP6+fNubm0ge6hP4/Bg4tbLgVUF0+AiH+xiD
gTqa5RpR31YJ5mpYM1gv0bd7skd2i7CudmC/AAsCgYB6w/TFdS2hbWIecNVlhPYQ
bLcYOTtI/iMQXEkFBnRBC/bEkmyJ/lPch5G/0Bv1vc8IOkQh/xmuHc0KEG/kJZkl
RF8sP4vfAiU+ndPHEFH/H6gzL5hNC3iPoRB8Y8crOTRc2jDFPS/1toTSkw0YTog1
ld2YUy7AYGLtwhcZylSQ3wKBgGH3EP8TjkQmLxOLNUrbghumlWovQDqLe8hSBrYj
jxTag/DAVr7f+fAZm/x4PqVEmmGougllem18FdQqsRBcLyitZOA2PaP9SbN4hSbY
FwwiZrRknZeeJd1gKv/vcWj7imZfz5SzPmVFyoMkUGdSoBeY7s/inx+uno1vze1a
CiTNAoGAD+qOUaY30EqKQryTRCTCABq0tclEm+UAD7aTFqzTqGjK8V7IxFAJ8rnw
okZbiUUfaTzSWjonh81igWBCbs9l7+FaaiMCy3Hy5rA7g2eTdJoU7gxlabEnzdUj
9O7hQg5LztVsx4CpVlyjw8gYB14pwoxrbJc4mDUwT7MPH29EgDE=
-----END RSA PRIVATE KEY-----";

    const KID: &str = "test-kid";
    const ISSUER: &str = "issuer-placeholder"; // replaced per-test with the mock server's own URI
    const AUDIENCE: &str = "distant-signal-internal";

    fn signing_key() -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(TEST_RSA_PRIVATE_KEY_PEM, Some(JsonWebKeyId::new(KID.to_string())))
            .expect("test RSA key should parse")
    }

    fn sign_token(claims: &serde_json::Value) -> String {
        let key = signing_key();
        let header = json!({"alg": "RS256", "kid": KID, "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let signing_input = format!("{header_b64}.{payload_b64}");
        let signature = key
            .sign(&CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, signing_input.as_bytes())
            .expect("signing with the test key should succeed");
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature);
        format!("{signing_input}.{sig_b64}")
    }

    /// Mints a valid claims JSON with `exp` far in the future, overridable
    /// per test via the closure.
    fn valid_claims(issuer: &str, mutate: impl FnOnce(&mut serde_json::Value)) -> serde_json::Value {
        let mut claims = json!({
            "sub": "svc-poller-incidents",
            "iss": issuer,
            "aud": AUDIENCE,
            "exp": (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
            "groups": ["svc-poller-incidents"],
        });
        mutate(&mut claims);
        claims
    }

    /// Stands up a mock Authentik: `.well-known/openid-configuration` plus
    /// a JWKS endpoint serving the test key's public half. Returns the
    /// server (whose `uri()` is also the `issuer_url`) and a
    /// `ServiceTokenVerifier` pointed at it.
    async fn mock_authentik() -> (MockServer, ServiceTokenVerifier) {
        let server = MockServer::start().await;
        let issuer = server.uri();

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/authorize"),
                "jwks_uri": format!("{issuer}/jwks"),
                "response_types_supported": ["token"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
            })))
            .mount(&server)
            .await;

        let public_jwk = signing_key().as_verification_key();
        let jwks = CoreJsonWebKeySet::new(vec![public_jwk]);
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&server)
            .await;

        let verifier = ServiceTokenVerifier::new(issuer.clone(), AUDIENCE.to_string()).unwrap();
        (server, verifier)
    }

    #[tokio::test]
    async fn a_valid_token_with_expected_claims_verifies() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));

        let claims = verifier.verify(&token).await.expect("valid token should verify");
        assert_eq!(claims.sub, "svc-poller-incidents");
        assert_eq!(claims.groups, vec!["svc-poller-incidents".to_string()]);
    }

    #[tokio::test]
    async fn an_expired_token_is_rejected() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c["exp"] = json!((chrono::Utc::now() - chrono::Duration::hours(1)).timestamp());
        }));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_a_corrupted_signature_is_rejected() {
        // Exercises the same "signature does not verify against the
        // serving key" path a genuinely-wrong-key token would hit,
        // without embedding a second RSA keypair fixture: a token signed
        // with the real test key, whose signature segment is then
        // replaced with unrelated bytes, must fail `verify_signature`
        // exactly like a token signed by a key the JWKS never served.
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));
        let mut parts: Vec<&str> = token.split('.').collect();
        let corrupted_sig = URL_SAFE_NO_PAD.encode(b"not-a-real-signature-at-all-000000");
        parts[2] = &corrupted_sig;
        let corrupted = parts.join(".");

        assert_eq!(verifier.verify(&corrupted).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_the_wrong_issuer_is_rejected() {
        let (_server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims("https://not-the-configured-issuer.invalid", |_| {}));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_token_with_the_wrong_audience_is_rejected() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c["aud"] = json!("some-other-client-id");
        }));

        assert_eq!(verifier.verify(&token).await, Err(VerifyError::Invalid));
    }

    #[tokio::test]
    async fn a_missing_groups_claim_defaults_to_empty_not_unscoped() {
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |c| {
            c.as_object_mut().unwrap().remove("groups");
        }));

        let claims = verifier.verify(&token).await.expect("otherwise-valid token should still verify");
        assert!(claims.groups.is_empty(), "an absent groups claim must default to empty, never 'allow everything'");
    }

    #[tokio::test]
    async fn an_unknown_kid_after_one_refetch_is_rejected() {
        // Sign with the real key/kid, then swap the header's `kid` to
        // something the JWKS never advertises. `key_for_kid` refetches
        // once (the mocked JWKS still won't have it), then rejects with
        // `UnknownKey` -- short-circuiting before `verify_signature` is
        // ever called, so the now-mismatched signature bytes carried over
        // from the original token are never actually checked.
        let (server, verifier) = mock_authentik().await;
        let token = sign_token(&valid_claims(&server.uri(), |_| {}));
        let header = json!({"alg": "RS256", "kid": "kid-nobody-has", "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let mut parts: Vec<&str> = token.split('.').collect();
        parts[0] = &header_b64;
        let retagged = parts.join(".");

        assert_eq!(verifier.verify(&retagged).await, Err(VerifyError::UnknownKey));
    }

    #[tokio::test]
    async fn a_malformed_token_is_rejected() {
        let (_server, verifier) = mock_authentik().await;
        assert_eq!(verifier.verify("not-a-jwt-at-all").await, Err(VerifyError::Malformed));
        assert_eq!(verifier.verify("only.two-parts").await, Err(VerifyError::Malformed));
    }
}
```

**Note on Step 2's `a_token_signed_with_the_wrong_key_is_rejected` test**: the inline commented-out second-PEM attempts above are deliberately left in the code as-written reasoning trail, then abandoned in favor of the simpler, equally-valid "corrupt the signature bytes" approach that's actually asserted on. When implementing this step, **write only the final version** — delete the two `other_key_pem`/`other_signing_key_pem` dead-end variables and their comments; they exist in this plan purely to document why the simpler approach was chosen, not as code to type in. The real test body is just: sign a token with the real key, corrupt `parts[2]` (the signature segment), and assert `Invalid`.

- [ ] **Step 3: Register the module**

In `crates/api/src/auth.rs`, change line 8:

```rust
pub mod oidc;
```

to:

```rust
pub mod internal_oauth;
pub mod oidc;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p api internal_oauth::`
Expected: PASS (8 tests: valid token, expired, wrong key, wrong issuer, wrong audience, missing groups, unknown kid, malformed).

- [ ] **Step 5: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/auth.rs crates/api/src/auth/internal_oauth.rs
git commit -m "Add crates/api::auth::internal_oauth: JWKS-cached JWT verification for internal-service tokens"
```

---

### Task 4: `crates/api` config fields + `AppState` wiring + route-scoping table

**Files:**
- Modify: `crates/api/src/data/config.rs`
- Modify: `crates/api/src/app.rs`

**Interfaces:**
- Produces: `ServiceArguments.internal_token` field **removed**; 10 new fields added (below). `AppState.internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier`, `AppState.internal_oauth_routes: Vec<(&'static str, Vec<String>)>` (route prefix → one-or-more required group names — correction 4: `/stanox-crs` needs two).
- Consumed by: Task 5 (`require_internal_oauth` reads both new `AppState` fields).
- **Depends on:** Task 3 (`ServiceTokenVerifier` must exist).

- [ ] **Step 1: Remove `internal_token`, add the internal-oauth fields**

In `crates/api/src/data/config.rs`, replace lines 54–57:

```rust
    /// Shared secret pollers must present via `X-Internal-Token` to reach
    /// `private_router()` endpoints.
    #[arg(long, env)]
    pub internal_token: String,
```

with:

```rust
    /// OIDC issuer base URL for the internal-service OAuth2 provider
    /// (Authentik) -- JWKS endpoint is learned via standard OIDC
    /// discovery against this URL, same mechanism as `sso_issuer_url`
    /// below. May be the same Authentik instance as `sso_issuer_url` (a
    /// different Application/Provider under it) or a different one --
    /// operator's call.
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_issuer_url: String,

    /// Expected `aud` claim on a verified internal-service access token --
    /// the shared Authentik OAuth2 Provider's own client_id (Decision 1:
    /// one provider, 8 service accounts underneath it). Must match every
    /// real caller's own `internal_oauth_client_id` (its own config) --
    /// same value, independently configured on each side.
    #[arg(long, env)]
    pub internal_oauth_client_id: String,

    /// Required Authentik group name per real caller (Decision 3) --
    /// gates which /private/* routes each caller's verified token may
    /// reach. Not secret (a group name isn't confidential). Suggested
    /// defaults only -- an operator's actual Authentik group names are
    /// not mandated by this design.
    #[arg(long, env, default_value = "svc-poller-incidents")]
    pub internal_oauth_group_poller_incidents: String,
    #[arg(long, env, default_value = "svc-poller-stations")]
    pub internal_oauth_group_poller_stations: String,
    #[arg(long, env, default_value = "svc-poller-tocs")]
    pub internal_oauth_group_poller_tocs: String,
    #[arg(long, env, default_value = "svc-poller-ldbws")]
    pub internal_oauth_group_poller_ldbws: String,
    #[arg(long, env, default_value = "svc-poller-tfl")]
    pub internal_oauth_group_poller_tfl: String,
    #[arg(long, env, default_value = "svc-trust-consumer")]
    pub internal_oauth_group_trust_consumer: String,
    #[arg(long, env, default_value = "svc-schedule-ingest")]
    pub internal_oauth_group_schedule_ingest: String,
    #[arg(long, env, default_value = "svc-schedule-reference")]
    pub internal_oauth_group_schedule_reference: String,
```

- [ ] **Step 2: Wire `AppState`**

In `crates/api/src/app.rs`, add two fields to `AppState` (after `oidc`, currently line 25):

```rust
    /// Verifies an incoming `/private/*` request's `Authorization: Bearer`
    /// token against Authentik's JWKS -- see
    /// `crate::auth::internal_oauth`.
    pub internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier,
    /// Route prefix -> one-or-more required group names, built once here
    /// from config. More than one group per route exists because
    /// `/stanox-crs` has two legitimate callers (`trust-consumer` reads
    /// it, `schedule-reference` writes it) -- either one's group is
    /// sufficient.
    pub internal_oauth_routes: Vec<(&'static str, Vec<String>)>,
```

Add matching placeholders to the hand-rolled `Debug` impl (currently lines 47–56):

```rust
            .field("internal_oauth_verifier", &"ServiceTokenVerifier { .. }")
            .field("internal_oauth_routes", &self.internal_oauth_routes)
```

(`internal_oauth_routes` contains only route prefixes and group *names*, neither of which is secret — unlike every other redacted field here, it's safe to include in the real `Debug` dump. `ServiceTokenVerifier` itself holds no secret state either, but has no `Debug` impl of its own — see Step 4's constructor for why a fixed placeholder is still simplest here.)

In `AppState::init()`, replace the `internal_token` non-empty guard (currently lines 65–72):

```rust
        // An empty token would make `auth::constant_time_eq` compare two
        // empty byte slices and accept any request with no
        // `X-Internal-Token` header at all — reject that at startup rather
        // than silently running an unauthenticated `private_router()`.
        ensure!(
            !config.internal_token.is_empty(),
            "internal_token (--internal-token / INTERNAL_TOKEN) must not be empty"
        );
```

with:

```rust
        // An empty required-group value must never silently become "any
        // group matches" -- the same failure class the old single-token
        // design guarded against for its own credential (see the removed
        // internal_token guard this replaces). issuer_url/client_id are
        // guarded too: an empty issuer_url would make IssuerUrl::new("")
        // fail inside ServiceTokenVerifier::new below anyway, but failing
        // here first gives a clearer message naming the actual env var.
        for (name, value) in [
            ("internal_oauth_issuer_url", &config.internal_oauth_issuer_url),
            ("internal_oauth_client_id", &config.internal_oauth_client_id),
            ("internal_oauth_group_poller_incidents", &config.internal_oauth_group_poller_incidents),
            ("internal_oauth_group_poller_stations", &config.internal_oauth_group_poller_stations),
            ("internal_oauth_group_poller_tocs", &config.internal_oauth_group_poller_tocs),
            ("internal_oauth_group_poller_ldbws", &config.internal_oauth_group_poller_ldbws),
            ("internal_oauth_group_poller_tfl", &config.internal_oauth_group_poller_tfl),
            ("internal_oauth_group_trust_consumer", &config.internal_oauth_group_trust_consumer),
            ("internal_oauth_group_schedule_ingest", &config.internal_oauth_group_schedule_ingest),
            ("internal_oauth_group_schedule_reference", &config.internal_oauth_group_schedule_reference),
        ] {
            ensure!(!value.is_empty(), "{name} must not be empty (see --{}/{})", name.replace('_', "-"), name.to_uppercase());
        }

        let internal_oauth_verifier = crate::auth::internal_oauth::ServiceTokenVerifier::new(
            config.internal_oauth_issuer_url.clone(),
            config.internal_oauth_client_id.clone(),
        )
        .context("failed to construct internal-oauth verifier")?;

        let internal_oauth_routes: Vec<(&'static str, Vec<String>)> = vec![
            ("/incidents", vec![config.internal_oauth_group_poller_incidents.clone()]),
            ("/stations", vec![config.internal_oauth_group_poller_stations.clone()]),
            ("/tocs", vec![config.internal_oauth_group_poller_tocs.clone()]),
            ("/station-samples", vec![config.internal_oauth_group_poller_ldbws.clone()]),
            ("/sample-stations", vec![config.internal_oauth_group_poller_ldbws.clone()]),
            ("/tfl-line-status", vec![config.internal_oauth_group_poller_tfl.clone()]),
            ("/train-events", vec![config.internal_oauth_group_trust_consumer.clone()]),
            ("/tracked-trains", vec![config.internal_oauth_group_trust_consumer.clone()]),
            ("/schedule-feed-ingests", vec![config.internal_oauth_group_schedule_ingest.clone()]),
            (
                "/stanox-crs",
                vec![
                    config.internal_oauth_group_trust_consumer.clone(),
                    config.internal_oauth_group_schedule_reference.clone(),
                ],
            ),
        ];
```

Add both new fields to the final `Ok(Arc::new(Self { .. }))` construction (currently lines 102–107):

```rust
        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
            oidc,
            internal_oauth_verifier,
            internal_oauth_routes,
        }))
```

- [ ] **Step 3: Build the whole workspace**

Run: `cargo build -p api`
Expected: **This fails to compile** — `crates/api/src/auth.rs`'s `require_internal_token` still references `app.config.internal_token`, which no longer exists. This is expected; Task 5 fixes it. Confirm the *only* compile error is in `auth.rs` (i.e. `config.rs`/`app.rs` themselves are syntactically correct) by checking the error output names `auth.rs`, not `config.rs` or `app.rs`.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs
git commit -m "Add internal-oauth config fields; build ServiceTokenVerifier and route-scoping table at startup"
```

---

### Task 5: `require_internal_oauth` middleware + `private_router()` wiring + `auth.rs` cleanup

**Files:**
- Modify: `crates/api/src/auth.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Produces: `pub async fn require_internal_oauth(State(app): State<App>, request: Request, next: Next) -> Result<Response, StatusCode>`, replacing `require_internal_token` (deleted, along with `constant_time_eq` and its 5 tests — nothing else in the crate references either after this task).
- Consumed by: `crates/api/src/routes/mod.rs::private_router()`.
- **Depends on:** Task 4.

- [ ] **Step 1: Replace `require_internal_token`/`constant_time_eq` and the module doc**

Replace `crates/api/src/auth.rs` lines 1–56 in full:

```rust
//! Internal-auth gate for `private_router()`.
//!
//! Bearer-token OAuth2 client-credentials auth (RFC 6750/6749 §4.4),
//! delegated to Authentik. `require_internal_oauth` parses the
//! `Authorization: Bearer` header, verifies it against Authentik's JWKS
//! (`internal_oauth::ServiceTokenVerifier`, local, no per-request network
//! round trip once cached), then checks the verified token's `groups`
//! claim against a static, config-built route-scoping table
//! (`App::internal_oauth_routes`) -- a route passes if `groups` contains
//! ANY of that route's required group names (more than one for
//! `/stanox-crs`, which has two legitimate callers). See
//! docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md.
//! This replaces a single shared-secret `X-Internal-Token` header,
//! compared in fixed time against one configured string with no concept
//! of *which* caller presented it -- that scheme is retired outright, not
//! kept alongside this one (no dual-acceptance window).

pub mod internal_oauth;
pub mod oidc;

use axum::extract::{Request, State, FromRequestParts};
use axum::http::{StatusCode, HeaderMap, request::Parts};
use axum::middleware::Next;
use axum::response::Response;

use crate::app::App;

/// `axum::middleware::from_fn_with_state` handler enforcing internal-service
/// OAuth2 auth. Applied only to `private_router()` -- `public_router()`
/// never sees this.
///
/// Status codes: a missing/malformed/expired/signature-invalid/wrong-
/// issuer/wrong-audience bearer token -> `401`, collapsed into one outcome
/// deliberately (see `VerifyError`'s own doc comment) -- a caller
/// presenting a token that fails verification for any of these reasons
/// learns only "not accepted," never which specific check failed. A
/// route absent from the scoping table, OR present but whose required
/// group(s) the verified token's `groups` claim doesn't contain -> `403`,
/// with the token's `sub` and the request path logged -- a real,
/// Authentik-issued credential, just not scoped for this route, which is
/// actionable signal for a misconfigured deployment (a chart/secret
/// wiring mistake handing one service another's credential), not an
/// information leak (the route table itself is fixed and not secret).
pub async fn require_internal_oauth(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();

    let Some(token) = bearer_token(request.headers()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let claims = match app.internal_oauth_verifier.verify(&token).await {
        Ok(claims) => claims,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    let required_groups = app
        .internal_oauth_routes
        .iter()
        .find(|(prefix, _)| path.starts_with(prefix))
        .map(|(_, groups)| groups);

    let Some(required_groups) = required_groups else {
        // A route with no entry in the table at all -- default-deny even
        // for a perfectly valid token, rather than silently "allowed"
        // because nobody added its row.
        tracing::warn!(sub = %claims.sub, path, "internal oauth request rejected: no route-scoping entry for this path");
        return Err(StatusCode::FORBIDDEN);
    };

    if !required_groups.iter().any(|group| claims.groups.contains(group)) {
        tracing::warn!(sub = %claims.sub, path, "internal oauth request rejected: valid token, wrong scope");
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::to_string)
}
```

- [ ] **Step 2: Add unit tests for `bearer_token` and the route-lookup logic**

Add a new `mod internal_oauth_middleware_tests` alongside the existing `#[cfg(test)] mod tests` (which keeps its cookie/session/`validate_return_to` tests — Steps 1's rewrite only removed `constant_time_eq`'s 5 tests, listed in the file at what were lines 217–240; delete exactly those five: `equal_tokens_match`, `different_content_same_length_does_not_match`, `different_length_does_not_match`, `empty_tokens_match`, `empty_provided_against_real_token_does_not_match`. Every other existing test in that `mod tests` block — `parse_cookie_*`, `set_cookie_header_*`, `clear_cookie_header_*`, `hash_session_token_*`, `generate_session_token_*`, `validate_return_to_*` — is untouched):

```rust
#[cfg(test)]
mod internal_oauth_middleware_tests {
    use super::*;

    #[test]
    fn bearer_token_extracts_the_token_from_a_well_formed_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::AUTHORIZATION, "Bearer abc.def.ghi".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn bearer_token_returns_none_without_the_bearer_prefix() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(axum::http::header::AUTHORIZATION, "abc.def.ghi".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn bearer_token_returns_none_with_no_authorization_header_at_all() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);
    }

    /// The route-scoping lookup itself (`find` + `starts_with`, then
    /// `groups.contains`) is exercised indirectly through
    /// `require_internal_oauth` in Task 4's `AppState`-building tests --
    /// no live `AppState`/database is constructed here (this module has
    /// no existing convention for that; see `internal_oauth::tests` for
    /// the JWT-verification coverage, which is the part of this feature
    /// that's genuinely pure and independently testable). This gap --
    /// `require_internal_oauth` itself is only exercised end-to-end,
    /// never unit-tested in isolation -- mirrors this codebase's existing
    /// posture for `AuthenticatedUser::from_request_parts` (also
    /// untested in isolation, for the same reason: it needs a live
    /// `AppState`).
}
```

- [ ] **Step 3: Wire `private_router()`**

In `crates/api/src/routes/mod.rs`, change the import (line 5):

```rust
use crate::auth::require_internal_token;
```

to:

```rust
use crate::auth::require_internal_oauth;
```

And change line 62:

```rust
        .layer(middleware::from_fn_with_state(app, require_internal_token))
```

to:

```rust
        .layer(middleware::from_fn_with_state(app, require_internal_oauth))
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS. This confirms nothing else in the crate still references `require_internal_token`/`constant_time_eq`/`app.config.internal_token`, and that every pre-existing `auth.rs` test (cookies, sessions, `validate_return_to`) still passes unchanged.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/auth.rs crates/api/src/routes/mod.rs
git commit -m "Replace require_internal_token with require_internal_oauth: Bearer parse, JWT verify, group-scoped route table"
```

---

### Task 6: 5 RDM/TfL pollers + `schedule-reference` — caller-side wiring

**Files:**
- Modify: `crates/poller-incidents/src/config.rs`, `crates/poller-incidents/src/main.rs`
- Modify: `crates/poller-stations/src/config.rs`, `crates/poller-stations/src/main.rs`
- Modify: `crates/poller-tocs/src/config.rs`, `crates/poller-tocs/src/main.rs`
- Modify: `crates/poller-tfl/src/config.rs`, `crates/poller-tfl/src/main.rs`
- Modify: `crates/poller-ldbws/src/config.rs`, `crates/poller-ldbws/src/main.rs`
- Modify: `crates/schedule-reference/src/config.rs`, `crates/schedule-reference/src/main.rs`

**Interfaces:**
- Consumes: `common::oauth_client::{OAuthCredentials, OAuthTokenCache}` (Task 1), `common::ingest::{post_batch, time_until_next_poll}`'s new `&OAuthTokenCache` signature (Task 2).
- **Depends on:** Tasks 1–2.

**Why this is one coordinated task, not six**: every one of these 6 crates has the *identical* edit shape -- replace one `internal_token: String` field with 5 new fields, build one `OAuthTokenCache` once in `main()`, and pass `&tokens` instead of `&config.internal_token` at each existing `ingest::`/direct-header call site. This mirrors this repo's own established precedent (the reverted design's Task 5, and this plan's own Task 9 below) for treating "the same mechanical change, repeated across N files" as one task rather than N.

**The field/env-var shape, identical across all 6** (replacing each crate's own `internal_token: String` field, at the line cited):

```rust
    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 8 real callers) -- see
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    /// This service's own Authentik service-account credential --
    /// per-service, distinct from every other caller's. `username` is
    /// identifying, not itself the secret; `password` (an Authentik
    /// app-password) is the actual secret.
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,
```

Field insertion points (replacing the existing `internal_token: String` field at each cited line):

| Crate | `config.rs` line |
|---|---|
| `poller-incidents` | 30 |
| `poller-stations` | 29 |
| `poller-tocs` | 30 |
| `poller-tfl` | 40 |
| `poller-ldbws` | 45 |
| `schedule-reference` | 32–33 |

- [ ] **Step 1: Edit each `config.rs`**

For each of the 6 files above, remove its existing `internal_token: String` field (with whatever doc comment currently precedes it — e.g. `poller-incidents/src/config.rs:30`'s field and `schedule-reference/src/config.rs:30–33`'s field/doc together) and insert the 5-field block shown above in its place.

- [ ] **Step 2: Edit each `main.rs`'s `OAuthTokenCache` construction + call sites**

Each `main.rs` currently constructs its `reqwest::Client` near the top of `main()` (e.g. `poller-incidents/src/main.rs`, `poller-ldbws/src/main.rs:45`, `schedule-reference/src/main.rs:29`). Immediately after that client is built, in all 6 files, add:

```rust
    let internal_oauth = common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
        token_url: config.internal_oauth_token_url.clone(),
        client_id: config.internal_oauth_client_id.clone(),
        scope: config.internal_oauth_scope.clone(),
        username: config.internal_oauth_username.clone(),
        password: config.internal_oauth_password.clone(),
    });
```

Then, at every existing call site in that file, replace `&config.internal_token` with `&internal_oauth`:

| Crate | Call sites to update |
|---|---|
| `poller-incidents` | `main.rs:43` (`ingest::time_until_next_poll(..., &config.internal_token, ...)`), `main.rs:81` (`ingest::post_batch(..., &config.internal_token, ...)`) |
| `poller-stations` | `main.rs:43`, `main.rs:81` (identical shape) |
| `poller-tocs` | `main.rs:43`, `main.rs:81` (identical shape) |
| `poller-tfl` | `main.rs:99`, `main.rs:163` (identical shape, different line numbers) |
| `poller-ldbws` | `main.rs:50` (`time_until_next_poll`), `main.rs:106` (`post_batch`), **and** `main.rs:120` — this file's own extra direct call, `fetch_sample_stations`, currently `.header(INTERNAL_TOKEN_HEADER, &config.internal_token)` (see Step 3 below — this one isn't a `common::ingest::` call, it needs a different edit shape) |
| `schedule-reference` | `main.rs:109` (`common::ingest::post_batch(client, &config.api_ingest_url, &config.internal_token, &records, "stanox/crs rows")` → `..., &internal_oauth, ...`) |

For every `common::ingest::time_until_next_poll`/`post_batch` call site in the table, the edit is purely positional: the third argument changes from `&config.internal_token` to `&internal_oauth`. No other argument or call shape changes.

- [ ] **Step 3: `poller-ldbws`'s extra direct call site**

`crates/poller-ldbws/src/main.rs`'s `fetch_sample_stations` function doesn't go through `common::ingest` at all — it's its own direct `reqwest` call (current shape, `main.rs:116–125`):

```rust
async fn fetch_sample_stations(client: &Client, config: &Config) -> anyhow::Result<Vec<String>> {
    let response = client
        .get(&config.api_sample_stations_url)
        .header(INTERNAL_TOKEN_HEADER, &config.internal_token)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<Vec<String>>().await?)
}
```

Change its signature to accept the token cache, and swap the header for `.bearer_auth(...)`:

```rust
async fn fetch_sample_stations(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<String>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(&config.api_sample_stations_url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<Vec<String>>().await?)
}
```

Update its one call site (in `poll_once`, or wherever `fetch_sample_stations(&client, &config)` is currently called) to `fetch_sample_stations(&client, &config, &internal_oauth)`. Also remove the now-unused `use common::ingest::{self, INTERNAL_TOKEN_HEADER, RDM_AUTH_HEADER_NAME};` import's `INTERNAL_TOKEN_HEADER` name (keep `ingest`/`RDM_AUTH_HEADER_NAME`, which are still used) — this constant no longer exists after Task 2.

Every other one of the 5 remaining crates in this task also drops any now-unused `INTERNAL_TOKEN_HEADER` import if it had one (`poller-incidents`/`poller-stations`/`poller-tocs`/`poller-tfl` import `RDM_AUTH_HEADER_NAME` alone or alongside `ingest` — check each file's actual `use common::ingest::...` line and remove only `INTERNAL_TOKEN_HEADER` if present; `schedule-reference` never imported it, since it only ever called `common::ingest::post_batch`).

- [ ] **Step 4: Build**

Run: `cargo build -p poller-incidents -p poller-stations -p poller-tocs -p poller-tfl -p poller-ldbws -p schedule-reference`
Expected: PASS for all 6.

- [ ] **Step 5: Run each crate's existing test suite**

Run: `cargo test -p poller-incidents -p poller-stations -p poller-tocs -p poller-tfl -p poller-ldbws -p schedule-reference`
Expected: PASS — none of these crates' existing tests exercise `internal_token`/the ingestion POST directly (confirmed: none of the 6 `main.rs`/`config.rs` files have a `#[cfg(test)]` block touching this path), so this is a build-and-existing-tests-still-pass check, not new coverage.

- [ ] **Step 6: Commit**

```bash
git add crates/poller-incidents/src/config.rs crates/poller-incidents/src/main.rs \
        crates/poller-stations/src/config.rs crates/poller-stations/src/main.rs \
        crates/poller-tocs/src/config.rs crates/poller-tocs/src/main.rs \
        crates/poller-tfl/src/config.rs crates/poller-tfl/src/main.rs \
        crates/poller-ldbws/src/config.rs crates/poller-ldbws/src/main.rs \
        crates/schedule-reference/src/config.rs crates/schedule-reference/src/main.rs
git commit -m "Wire 5 RDM/TfL pollers + schedule-reference onto crates/common::oauth_client"
```

---

### Task 7: `trust-consumer` — caller-side wiring

**Files:**
- Modify: `crates/trust-consumer/src/config.rs`
- Modify: `crates/trust-consumer/src/queries.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Consumes: same as Task 6.
- **Depends on:** Tasks 1–2. Independent of Task 6 (disjoint files).

`trust-consumer` doesn't use `common::ingest::time_until_next_poll`/`fetch_last_fetched` at all — its own `queries.rs` hand-rolls three functions (`fetch_active_tracked_trains`, `fetch_stanox_crs`, `post_train_events`, the last of which wraps `common::ingest::post_batch`).

- [ ] **Step 1: `config.rs`**

Replace `internal_token: String` (currently line 66) with the same 5-field block from Task 6.

- [ ] **Step 2: `queries.rs`**

Replace the whole file's contents (currently 51 lines) with:

```rust
//! Thin HTTP client wrapper against `crates/api`'s train-tracking
//! endpoints. Kept separate from `process.rs` so the processing loop's
//! tests can run against `FakeMovementFeed` without also needing a live
//! `api` -- these functions are the one part of `process::run_once`'s
//! surrounding loop this plan does NOT unit-test, verified instead by the
//! manual live-stack check, the same posture `crates/enricher`'s
//! DB-touching `queries.rs` takes.

use common::TrackedTrainRef;
use common::oauth_client::OAuthTokenCache;
use reqwest::Client;

pub async fn fetch_active_tracked_trains(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let token = tokens.get_token(client).await?;
    let response = client.get(url).bearer_auth(&token).send().await?.error_for_status()?;
    Ok(response.json().await?)
}

pub async fn fetch_stanox_crs(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    let token = tokens.get_token(client).await?;
    let response = client.get(url).bearer_auth(&token).send().await?.error_for_status()?;
    Ok(response.json().await?)
}

pub async fn post_train_events(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
    events: &[common::TrainMovementEventMessage],
) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    common::ingest::post_batch(client, url, tokens, events, "train events").await
}
```

(`use common::{TrackedTrainRef, TrainMovementEventMessage};` becomes `use common::TrackedTrainRef;` at the top plus `common::TrainMovementEventMessage` referenced inline in `post_train_events`'s signature above — either form compiles; keep whichever matches this file's existing import style most closely, i.e. hoist `TrainMovementEventMessage` back into the `use` line if preferred. The functional change is only: `internal_token: &str` params become `tokens: &OAuthTokenCache`, and every `.header(INTERNAL_TOKEN_HEADER, ...)` becomes `tokens.get_token(client).await?` + `.bearer_auth(&token)`.)

- [ ] **Step 3: `main.rs`**

After the existing `let http = reqwest::Client::new();` (currently line 36), add:

```rust
    let internal_oauth = common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
        token_url: config.internal_oauth_token_url.clone(),
        client_id: config.internal_oauth_client_id.clone(),
        scope: config.internal_oauth_scope.clone(),
        username: config.internal_oauth_username.clone(),
        password: config.internal_oauth_password.clone(),
    });
```

Update the three call sites:

- Line 62–66 (`queries::fetch_active_tracked_trains(&http, &config.api_tracked_trains_url, &config.internal_token)`) → `queries::fetch_active_tracked_trains(&http, &config.api_tracked_trains_url, &internal_oauth)`.
- Line 90 (`queries::fetch_stanox_crs(&http, &config.stanox_crs_url, &config.internal_token)`) → `queries::fetch_stanox_crs(&http, &config.stanox_crs_url, &internal_oauth)`.
- Line 96 (`queries::post_train_events(&http, &config.api_ingest_url, &config.internal_token, events)`) → `queries::post_train_events(&http, &config.api_ingest_url, &internal_oauth, events)`.

- [ ] **Step 4: Build and test**

Run: `cargo build -p trust-consumer && cargo test -p trust-consumer`
Expected: PASS. `trust-consumer`'s existing `run_cycle` tests (`main.rs`'s `#[cfg(test)] mod tests`, the `a_failed_post_does_not_commit_the_batch` family) inject `post` as a plain closure and never touch `queries::`/`internal_oauth` directly — confirm they still pass unmodified.

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/config.rs crates/trust-consumer/src/queries.rs crates/trust-consumer/src/main.rs
git commit -m "Wire trust-consumer onto crates/common::oauth_client"
```

---

### Task 8: `schedule-ingest` — caller-side wiring

**Files:**
- Modify: `crates/schedule-ingest/src/config.rs`
- Modify: `crates/schedule-ingest/src/main.rs`

**Interfaces:**
- Consumes: same as Task 6.
- **Depends on:** Task 1. Independent of Tasks 6–7 (disjoint files).

`schedule-ingest`'s `post_ingest` (`main.rs:298–316`) is deliberately *not* `common::ingest::post_batch` (its own doc comment explains why: `api`'s `ScheduleFeedIngestRequest` expects a single JSON object, not an array) — it hand-rolls its own POST, but the auth-header change is the same shape as every other direct call site.

- [ ] **Step 1: `config.rs`**

Replace `internal_token: String` (currently line 56) with the same 5-field block from Task 6.

- [ ] **Step 2: `main.rs`**

Find `schedule-ingest`'s `reqwest::Client` construction near the top of `main()` and add, immediately after it:

```rust
    let internal_oauth = common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
        token_url: config.internal_oauth_token_url.clone(),
        client_id: config.internal_oauth_client_id.clone(),
        scope: config.internal_oauth_scope.clone(),
        username: config.internal_oauth_username.clone(),
        password: config.internal_oauth_password.clone(),
    });
```

Thread `internal_oauth` through to wherever `post_ingest` is called (its call site passes `config` already; either add `internal_oauth` as an explicit parameter to the surrounding function that calls `post_ingest`, or capture it by reference if `post_ingest` is called from within `main`'s own scope — follow whichever threading shape this file's existing `config`/`client` parameters already use, since `internal_oauth` needs to reach `post_ingest` the same way).

Replace `post_ingest`'s signature and body (`main.rs:298–316`):

```rust
async fn post_ingest(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
    request: &ScheduleFeedIngestRequest,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(&config.api_ingest_url)
        .bearer_auth(&token)
        .json(request)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(sequence = request.sequence, files = request.files.len(), "posted schedule feed ingest to api");
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("schedule feed ingest POST failed: {status} {text}");
    }
}
```

Update `post_ingest`'s one call site to pass `&internal_oauth` as the new second argument. Remove the now-unused `INTERNAL_TOKEN_HEADER` import from this file's `use common::ingest::...` line if present (this crate may also reference `common::ingest::time_until_next_poll`'s "no prior fetch" semantics in a comment only, per correction 2's earlier citation of `main.rs:94` — that comment is prose, not code, and needs no edit).

- [ ] **Step 3: Build and test**

Run: `cargo build -p schedule-ingest && cargo test -p schedule-ingest`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/schedule-ingest/src/config.rs crates/schedule-ingest/src/main.rs
git commit -m "Wire schedule-ingest onto crates/common::oauth_client"
```

---

### Task 9: Helm chart — config surface for all 8 real callers + `api`

**Files:**
- Modify: `charts/distant-signal/values.yaml`
- Modify: `charts/distant-signal/templates/secret.yaml`
- Modify: `charts/distant-signal/templates/_helpers.tpl`
- Modify: `charts/distant-signal/templates/api-deployment.yaml`
- Modify: `charts/distant-signal/templates/poller-deployments.yaml`
- Modify: `charts/distant-signal/templates/trust-consumer-deployment.yaml`
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml`

**Interfaces:**
- Produces: a new top-level `internalOauth.{tokenUrl,clientId,scope}` values block (shared, non-secret); per-poller `internalOauth.{username,password}` values plus `existingSecretInternalOauthUsernameKey`/`existingSecretInternalOauthPasswordKey` (reusing each `pollers.<name>`'s existing `existingSecret` toggle); analogous blocks on `trustConsumer`, `scheduleFeed.ingest`, `scheduleFeed.reference`; `api.internalOauth.{issuerUrl,clientId,groups.*}` (all non-secret, plain values). Every `secrets.internalToken`/`INTERNAL_TOKEN` reference removed from every template listed above.
- Consumed by: Task 11 (chart lint/render verification).
- **Depends on:** Task 4's final `crates/api` env-var names and Tasks 6–8's final caller env-var names (all fixed by this point).

**Why this is one coordinated task**: mirrors Task 6's own reasoning and the precedent already established in this repo's reverted design's own Task 5 — one shared `poller-deployments.yaml` template renders 5 of the 8 callers via one `range` loop, so wiring a per-poller `secretKeyRef` pair is inherently one edit to that one loop body; the remaining 3 (`trustConsumer`, `scheduleFeed.ingest`, `scheduleFeed.reference`) are three more small, disjoint edits to their own templates, plus one shared, non-secret `internalOauth.*` block and `api`'s own new values. All of it lands together as one same-shape change across a handful of files.

- [ ] **Step 1: Remove the old `secrets.internalToken`/`existingSecret` block**

In `charts/distant-signal/values.yaml`, remove lines 40–44 (`internalToken: ""`, `existingSecret: ""`, `existingSecretInternalTokenKey: internal-token`, and their comments) from the top-level `secrets:` block. **Do not remove the whole `secrets:` block** — check whether it holds any other keys besides these three (its own file context around line 28–44 should be re-read before editing, since this block's surrounding comment references a general 3-way secrets-resolution convention that may still apply to fields this plan doesn't touch).

- [ ] **Step 2: Add the shared, non-secret `internalOauth` top-level block**

Add near the top of `values.yaml`, after the existing `secrets:` block:

```yaml
# ---------------------------------------------------------------------------
# internalOauth -- shared, non-secret OAuth2 client-credentials config every
# real caller of api's /private/* routes uses to obtain its own access
# token (docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
# Decision 6). One shared Authentik OAuth2 Provider/Application services
# all 8 real callers (Decision 1). Defining that Provider, its 8 Service
# Accounts, and their groups is explicitly OUT OF SCOPE for this chart
# (operator/deployment-time work) -- these values only configure how this
# chart's own Deployments REACH whatever an operator has already
# provisioned.
# ---------------------------------------------------------------------------
internalOauth:
  # -- Authentik's client-credentials token endpoint, e.g.
  # https://sso.example.com/application/o/token/. Required whenever any
  # real caller is enabled.
  tokenUrl: ""
  # -- The shared OAuth2 Provider's client_id -- same value as
  # api.internalOauth.clientId below (the expected `aud`). Required.
  clientId: ""
  # -- Scope requested on every client-credentials POST -- must match
  # whatever scope mapping the operator attached to the provider to
  # surface the `groups` claim (Decision 3).
  scope: groups
```

- [ ] **Step 3: Add per-poller `internalOauth` values**

To each of the 5 `pollers.<name>` blocks (`incidents`, `stations`, `tocs`, `ldbws`, `tfl`), immediately after the existing `apiKey`/`existingSecret`/`existingSecretApiKeyKey` trio, add:

```yaml
    # -- This poller's own Authentik service-account credential -- see
    # top-level internalOauth for the shared, non-secret token URL/
    # client_id/scope. Never auto-generated (Authentik, an external
    # system, assigns these) -- same posture as apiKey above, not
    # secrets.internalToken's old auto-generate posture.
    internalOauthUsername: ""
    internalOauthPassword: ""
    existingSecretInternalOauthUsernameKey: internal-oauth-username-poller-incidents
    existingSecretInternalOauthPasswordKey: internal-oauth-password-poller-incidents
```

(Reusing that poller's own already-existing `existingSecret` toggle — no new toggle field needed. Substitute the poller's own name in both default key strings, e.g. `internal-oauth-username-poller-ldbws` for the `ldbws` block.)

Add to `trustConsumer:` (this block currently has no `existingSecret`/credential fields of its own at all — add both the toggle and the pair, since nothing pre-exists here to reuse):

```yaml
  # -- Read this service's OAuth2 credential from a pre-existing Secret
  # instead of the chart-rendered one.
  existingSecret: ""
  internalOauthUsername: ""
  internalOauthPassword: ""
  existingSecretInternalOauthUsernameKey: internal-oauth-username-trust-consumer
  existingSecretInternalOauthPasswordKey: internal-oauth-password-trust-consumer
```

Add to `scheduleFeed.ingest:` and `scheduleFeed.reference:` (each of these currently has no `existingSecret` of its own either — both containers already independently source `INTERNAL_TOKEN` from the *shared* chart secret via `distant-signal.internalTokenSecretName`, being removed in Step 1/6; give each its own independent toggle here, since they're now two distinct service accounts, not one shared token):

```yaml
    existingSecret: ""
    internalOauthUsername: ""
    internalOauthPassword: ""
    existingSecretInternalOauthUsernameKey: internal-oauth-username-schedule-ingest
```

(for `ingest`; substitute `schedule-reference` and the matching key name for the `reference:` block's own addition, and note `reference:`'s block gets `existingSecretInternalOauthPasswordKey: internal-oauth-password-schedule-reference` as its fourth line, mirroring `ingest:`'s own fourth line `existingSecretInternalOauthPasswordKey: internal-oauth-password-schedule-ingest`).

- [ ] **Step 4: Add `api.internalOauth` values**

Add to the `api:` block, as a sibling to the existing `sso:` block:

```yaml
  # Internal-service OAuth2 verification -- see
  # docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md.
  # REQUIRED -- crates/api's clap config declares issuerUrl/clientId/8
  # group fields with no default for the first two, so a pod without them
  # exits immediately, same fail-fast posture as sso.* above.
  internalOauth:
    # -- OIDC issuer base URL for the internal-service OAuth2 provider.
    # JWKS is discovered from this URL's .well-known/openid-configuration,
    # same mechanism as sso.issuerUrl. May be the same Authentik instance
    # as sso (a different Application/Provider) or a different one.
    issuerUrl: ""
    # -- Expected `aud` claim on a verified token -- SAME value as the
    # top-level internalOauth.clientId above.
    clientId: ""
    # -- Required Authentik group name per real caller (Decision 3). Not
    # secret. Suggested defaults only.
    groups:
      pollerIncidents: svc-poller-incidents
      pollerStations: svc-poller-stations
      pollerTocs: svc-poller-tocs
      pollerLdbws: svc-poller-ldbws
      pollerTfl: svc-poller-tfl
      trustConsumer: svc-trust-consumer
      scheduleIngest: svc-schedule-ingest
      scheduleReference: svc-schedule-reference
```

- [ ] **Step 5: `_helpers.tpl` — remove the old internal-token helpers, add per-service oauth helpers**

Remove `distant-signal.internalTokenSecretName`/`distant-signal.internalTokenSecretKey` (currently lines 186–196).

Add, near `distant-signal.pollerSecretName`/`pollerSecretKey` (currently lines 219–233):

```
{{/*
Resolved Secret name/key for one poller's own OAuth2 username/password
(distinct from its RDM apiKey, same Secret object, same existingSecret
toggle). Call as:
  {{ include "distant-signal.pollerSecretName" (dict "root" $ "poller" $p) }}
  {{ include "distant-signal.pollerOauthUsernameSecretKey" (dict "root" $ "name" $name "poller" $p) }}
  {{ include "distant-signal.pollerOauthPasswordSecretKey" (dict "root" $ "name" $name "poller" $p) }}
*/}}
{{- define "distant-signal.pollerOauthUsernameSecretKey" -}}
{{- if .poller.existingSecret }}
{{- .poller.existingSecretInternalOauthUsernameKey }}
{{- else }}
{{- printf "internal-oauth-username-poller-%s" .name }}
{{- end }}
{{- end }}

{{- define "distant-signal.pollerOauthPasswordSecretKey" -}}
{{- if .poller.existingSecret }}
{{- .poller.existingSecretInternalOauthPasswordKey }}
{{- else }}
{{- printf "internal-oauth-password-poller-%s" .name }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for trust-consumer's own OAuth2 credential.
*/}}
{{- define "distant-signal.trustConsumerSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.trustConsumer.existingSecret }}
{{- end }}

{{- define "distant-signal.trustConsumerOauthUsernameSecretKey" -}}
{{- if .Values.trustConsumer.existingSecret }}
{{- .Values.trustConsumer.existingSecretInternalOauthUsernameKey }}
{{- else }}
{{- print "internal-oauth-username-trust-consumer" }}
{{- end }}
{{- end }}

{{- define "distant-signal.trustConsumerOauthPasswordSecretKey" -}}
{{- if .Values.trustConsumer.existingSecret }}
{{- .Values.trustConsumer.existingSecretInternalOauthPasswordKey }}
{{- else }}
{{- print "internal-oauth-password-trust-consumer" }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for schedule-ingest's / schedule-reference's own
OAuth2 credentials -- two independent service accounts sharing one Pod,
each with its own existingSecret toggle.
*/}}
{{- define "distant-signal.scheduleIngestSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.scheduleFeed.ingest.existingSecret }}
{{- end }}

{{- define "distant-signal.scheduleIngestOauthUsernameSecretKey" -}}
{{- if .Values.scheduleFeed.ingest.existingSecret }}
{{- .Values.scheduleFeed.ingest.existingSecretInternalOauthUsernameKey }}
{{- else }}
{{- print "internal-oauth-username-schedule-ingest" }}
{{- end }}
{{- end }}

{{- define "distant-signal.scheduleIngestOauthPasswordSecretKey" -}}
{{- if .Values.scheduleFeed.ingest.existingSecret }}
{{- .Values.scheduleFeed.ingest.existingSecretInternalOauthPasswordKey }}
{{- else }}
{{- print "internal-oauth-password-schedule-ingest" }}
{{- end }}
{{- end }}

{{- define "distant-signal.scheduleReferenceSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.scheduleFeed.reference.existingSecret }}
{{- end }}

{{- define "distant-signal.scheduleReferenceOauthUsernameSecretKey" -}}
{{- if .Values.scheduleFeed.reference.existingSecret }}
{{- .Values.scheduleFeed.reference.existingSecretInternalOauthUsernameKey }}
{{- else }}
{{- print "internal-oauth-username-schedule-reference" }}
{{- end }}
{{- end }}

{{- define "distant-signal.scheduleReferenceOauthPasswordSecretKey" -}}
{{- if .Values.scheduleFeed.reference.existingSecret }}
{{- .Values.scheduleFeed.reference.existingSecretInternalOauthPasswordKey }}
{{- else }}
{{- print "internal-oauth-password-schedule-reference" }}
{{- end }}
{{- end }}
```

- [ ] **Step 6: `secret.yaml` — remove `internal-token`, add per-service oauth keys**

Remove the `internal-token:` block (currently lines 36–41, gated on `not .Values.secrets.existingSecret`).

Add, alongside the existing `rdm-<name>-api-key` block:

```
{{/* internal-oauth-username-poller-<name> / internal-oauth-password-poller-<name>:
     like rdm-<name>-api-key, deliberately NOT auto-generated -- Authentik,
     an external system, assigns these (the apiKey precedent, not the old
     internalToken precedent -- see the design doc's Current relevant
     state). Rendered per ENABLED poller without an existingSecret. */}}
{{- range $name, $poller := .Values.pollers -}}
{{- if and $poller.enabled (not $poller.existingSecret) -}}
{{- $_ := set $data (printf "internal-oauth-username-poller-%s" $name) ($poller.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data (printf "internal-oauth-password-poller-%s" $name) ($poller.internalOauthPassword | default "" | b64enc) -}}
{{- end -}}
{{- end -}}

{{/* trust-consumer's own OAuth2 credential -- same never-auto-generated
     posture. */}}
{{- if not .Values.trustConsumer.existingSecret -}}
{{- $_ := set $data "internal-oauth-username-trust-consumer" (.Values.trustConsumer.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-trust-consumer" (.Values.trustConsumer.internalOauthPassword | default "" | b64enc) -}}
{{- end -}}

{{/* schedule-ingest's and schedule-reference's own OAuth2 credentials --
     two independent service accounts, each its own existingSecret toggle,
     both rendered (possibly empty) only when scheduleFeed.enabled -- the
     whole schedulefeed Deployment doesn't render at all otherwise, so
     nothing would consume these keys. */}}
{{- if .Values.scheduleFeed.enabled -}}
{{- if not .Values.scheduleFeed.ingest.existingSecret -}}
{{- $_ := set $data "internal-oauth-username-schedule-ingest" (.Values.scheduleFeed.ingest.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-schedule-ingest" (.Values.scheduleFeed.ingest.internalOauthPassword | default "" | b64enc) -}}
{{- end -}}
{{- if not .Values.scheduleFeed.reference.existingSecret -}}
{{- $_ := set $data "internal-oauth-username-schedule-reference" (.Values.scheduleFeed.reference.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-schedule-reference" (.Values.scheduleFeed.reference.internalOauthPassword | default "" | b64enc) -}}
{{- end -}}
{{- end -}}
```

- [ ] **Step 7: `api-deployment.yaml` — swap `INTERNAL_TOKEN` for `internalOauth.*`**

Replace lines 105–109 (the `INTERNAL_TOKEN` `secretKeyRef` env entry):

```yaml
            - name: INTERNAL_OAUTH_ISSUER_URL
              value: {{ .Values.api.internalOauth.issuerUrl | quote }}
            - name: INTERNAL_OAUTH_CLIENT_ID
              value: {{ .Values.api.internalOauth.clientId | quote }}
            - name: INTERNAL_OAUTH_GROUP_POLLER_INCIDENTS
              value: {{ .Values.api.internalOauth.groups.pollerIncidents | quote }}
            - name: INTERNAL_OAUTH_GROUP_POLLER_STATIONS
              value: {{ .Values.api.internalOauth.groups.pollerStations | quote }}
            - name: INTERNAL_OAUTH_GROUP_POLLER_TOCS
              value: {{ .Values.api.internalOauth.groups.pollerTocs | quote }}
            - name: INTERNAL_OAUTH_GROUP_POLLER_LDBWS
              value: {{ .Values.api.internalOauth.groups.pollerLdbws | quote }}
            - name: INTERNAL_OAUTH_GROUP_POLLER_TFL
              value: {{ .Values.api.internalOauth.groups.pollerTfl | quote }}
            - name: INTERNAL_OAUTH_GROUP_TRUST_CONSUMER
              value: {{ .Values.api.internalOauth.groups.trustConsumer | quote }}
            - name: INTERNAL_OAUTH_GROUP_SCHEDULE_INGEST
              value: {{ .Values.api.internalOauth.groups.scheduleIngest | quote }}
            - name: INTERNAL_OAUTH_GROUP_SCHEDULE_REFERENCE
              value: {{ .Values.api.internalOauth.groups.scheduleReference | quote }}
```

(None of these are `secretKeyRef`-backed — every one is a plain `value:`, since none of `api`'s own new fields are secret, per Decision 6.)

- [ ] **Step 8: `poller-deployments.yaml` — swap `INTERNAL_TOKEN` for the 5-field block**

Replace lines 97–101 (the shared `INTERNAL_TOKEN` block inside the `range $name, $poller := .Values.pollers` loop):

```yaml
            - name: INTERNAL_OAUTH_TOKEN_URL
              value: {{ $root.Values.internalOauth.tokenUrl | quote }}
            - name: INTERNAL_OAUTH_CLIENT_ID
              value: {{ $root.Values.internalOauth.clientId | quote }}
            - name: INTERNAL_OAUTH_SCOPE
              value: {{ $root.Values.internalOauth.scope | quote }}
            - name: INTERNAL_OAUTH_USERNAME
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerSecretName" (dict "root" $root "poller" $poller) }}
                  key: {{ include "distant-signal.pollerOauthUsernameSecretKey" (dict "root" $root "name" $name "poller" $poller) }}
            - name: INTERNAL_OAUTH_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerSecretName" (dict "root" $root "poller" $poller) }}
                  key: {{ include "distant-signal.pollerOauthPasswordSecretKey" (dict "root" $root "name" $name "poller" $poller) }}
```

(This file's existing `range` block already binds `$root := .` at the top — line 8 — and uses `$root`/`$name`/`$poller` throughout, matching the existing `RDM_API_KEY` block's own `dict "root" $root "poller" $poller` call shape immediately above it.)

- [ ] **Step 9: `trust-consumer-deployment.yaml` — swap `INTERNAL_TOKEN`**

Replace lines 103–107 with the same 5-field shape as Step 8, substituting `distant-signal.trustConsumerSecretName`/`trustConsumerOauthUsernameSecretKey`/`trustConsumerOauthPasswordSecretKey` (each called as `(dict "root" .)`, no `poller`/`name` dict keys needed — these helpers close over `.Values.trustConsumer` directly, per Step 5) and `.Values.internalOauth.{tokenUrl,clientId,scope}` for the three shared plain values.

- [ ] **Step 10: `schedulefeed-deployment.yaml` — swap `INTERNAL_TOKEN` in both `ingest` and `reference` containers**

Replace the `ingest` container's `INTERNAL_TOKEN` block (currently lines 216–219) with the 5-field shape, using `distant-signal.scheduleIngestSecretName`/`scheduleIngestOauthUsernameSecretKey`/`scheduleIngestOauthPasswordSecretKey`.

Replace the `reference` container's `INTERNAL_TOKEN` block (currently lines 263–266) with the same shape, using `distant-signal.scheduleReferenceSecretName`/`scheduleReferenceOauthUsernameSecretKey`/`scheduleReferenceOauthPasswordSecretKey`.

Both containers' shared, non-secret 3 fields (`INTERNAL_OAUTH_TOKEN_URL`/`CLIENT_ID`/`SCOPE`) read from the same `.Values.internalOauth.*` as every other caller.

- [ ] **Step 11: Lint and render**

Run: `helm lint charts/distant-signal`
Expected: PASS (no new warnings beyond whatever pre-existed).

Run: `helm template charts/distant-signal --set api.internalOauth.issuerUrl=https://sso.example.invalid --set api.internalOauth.clientId=test-client --set internalOauth.tokenUrl=https://sso.example.invalid/application/o/token/ --set internalOauth.clientId=test-client --set api.sso.issuerUrl=https://sso.example.invalid --set api.sso.clientId=test --set api.sso.clientSecret=test --set api.sso.redirectUrl=https://app.example.invalid/api/auth/callback --set api.sso.postLoginRedirectUrl=https://app.example.invalid/`
Expected: renders without error (a real render exercises `secret.yaml`'s new blocks, `_helpers.tpl`'s new helpers, and every Deployment's new env entries end to end). Grep the output for `INTERNAL_TOKEN` — expect zero matches anywhere in the rendered manifests.

- [ ] **Step 12: Commit**

```bash
git add charts/distant-signal/values.yaml charts/distant-signal/templates/secret.yaml \
        charts/distant-signal/templates/_helpers.tpl charts/distant-signal/templates/api-deployment.yaml \
        charts/distant-signal/templates/poller-deployments.yaml \
        charts/distant-signal/templates/trust-consumer-deployment.yaml \
        charts/distant-signal/templates/schedulefeed-deployment.yaml
git commit -m "Helm: replace secrets.internalToken/X-Internal-Token wiring with internal-service OAuth2 config for all 8 real callers"
```

---

### Task 10: `docker-compose.yml` / `dev.env.example` / `local.env.example`

**Files:**
- Modify: `docker-compose.yml`
- Modify: `dev.env.example`
- Modify: `local.env.example`

**Interfaces:**
- Produces: `INTERNAL_TOKEN` removed from all 8 existing service blocks (`api` + `poller-incidents`/`-stations`/`-tocs`/`-ldbws`/`-tfl`/`trust-consumer`/`schedule-ingest`); each replaced with the matching field set from Tasks 4/6/7/8.
- **Depends on:** Tasks 4, 6, 7, 8's final env-var names (fixed by this point). **Does not add a `schedule-reference` service** — correction 5: that crate isn't run by `docker-compose.yml` today, and adding it is a separate, unrelated gap this plan doesn't take on.

- [ ] **Step 1: `docker-compose.yml` — `api` service**

Replace line 105 (`INTERNAL_TOKEN: ${INTERNAL_TOKEN}`) with:

```yaml
      INTERNAL_OAUTH_ISSUER_URL: ${INTERNAL_OAUTH_ISSUER_URL:?INTERNAL_OAUTH_ISSUER_URL must be set — see the internal-oauth section of local.env.example}
      INTERNAL_OAUTH_CLIENT_ID: ${INTERNAL_OAUTH_CLIENT_ID:?INTERNAL_OAUTH_CLIENT_ID must be set — see the internal-oauth section of local.env.example}
      INTERNAL_OAUTH_GROUP_POLLER_INCIDENTS: ${INTERNAL_OAUTH_GROUP_POLLER_INCIDENTS:-svc-poller-incidents}
      INTERNAL_OAUTH_GROUP_POLLER_STATIONS: ${INTERNAL_OAUTH_GROUP_POLLER_STATIONS:-svc-poller-stations}
      INTERNAL_OAUTH_GROUP_POLLER_TOCS: ${INTERNAL_OAUTH_GROUP_POLLER_TOCS:-svc-poller-tocs}
      INTERNAL_OAUTH_GROUP_POLLER_LDBWS: ${INTERNAL_OAUTH_GROUP_POLLER_LDBWS:-svc-poller-ldbws}
      INTERNAL_OAUTH_GROUP_POLLER_TFL: ${INTERNAL_OAUTH_GROUP_POLLER_TFL:-svc-poller-tfl}
      INTERNAL_OAUTH_GROUP_TRUST_CONSUMER: ${INTERNAL_OAUTH_GROUP_TRUST_CONSUMER:-svc-trust-consumer}
      INTERNAL_OAUTH_GROUP_SCHEDULE_INGEST: ${INTERNAL_OAUTH_GROUP_SCHEDULE_INGEST:-svc-schedule-ingest}
      INTERNAL_OAUTH_GROUP_SCHEDULE_REFERENCE: ${INTERNAL_OAUTH_GROUP_SCHEDULE_REFERENCE:-svc-schedule-reference}
```

(Matching the file's own established `:?message` convention for required-no-default fields — same as the adjacent `SSO_*` lines immediately below it — and `:-default` for the 8 group names, which have real, usable defaults via `config.rs`'s own `default_value`.)

- [ ] **Step 2: `docker-compose.yml` — the 7 real callers**

Each of `poller-incidents` (line 155), `poller-stations` (line 176), `poller-tocs` (line 200), `poller-ldbws` (line 225), `poller-tfl` (line 250), `trust-consumer` (line 331), `schedule-ingest` (line 489) has its own `INTERNAL_TOKEN: ${INTERNAL_TOKEN}` line. Replace each with:

```yaml
      INTERNAL_OAUTH_TOKEN_URL: ${INTERNAL_OAUTH_TOKEN_URL:?INTERNAL_OAUTH_TOKEN_URL must be set — see the internal-oauth section of local.env.example}
      INTERNAL_OAUTH_CLIENT_ID: ${INTERNAL_OAUTH_CLIENT_ID:?INTERNAL_OAUTH_CLIENT_ID must be set — see the internal-oauth section of local.env.example}
      INTERNAL_OAUTH_SCOPE: ${INTERNAL_OAUTH_SCOPE:-groups}
      INTERNAL_OAUTH_USERNAME: ${INTERNAL_OAUTH_USERNAME_<SERVICE>:?INTERNAL_OAUTH_USERNAME_<SERVICE> must be set}
      INTERNAL_OAUTH_PASSWORD: ${INTERNAL_OAUTH_PASSWORD_<SERVICE>:?INTERNAL_OAUTH_PASSWORD_<SERVICE> must be set}
```

where `<SERVICE>` is a per-caller suffix distinguishing each service's own credential in the shared `.env` namespace (matching how `RDM_INCIDENTS_API_KEY`/`RDM_STATIONS_API_KEY`/etc. are already distinct top-level vars per poller, not one shared `RDM_API_KEY` name): `INCIDENTS`, `STATIONS`, `TOCS`, `LDBWS`, `TFL`, `TRUST_CONSUMER`, `SCHEDULE_INGEST` respectively. E.g. `poller-incidents`'s block becomes `INTERNAL_OAUTH_USERNAME: ${INTERNAL_OAUTH_USERNAME_INCIDENTS:?...}` / `INTERNAL_OAUTH_PASSWORD: ${INTERNAL_OAUTH_PASSWORD_INCIDENTS:?...}`; `trust-consumer`'s becomes `..._TRUST_CONSUMER`; and so on. `INTERNAL_OAUTH_TOKEN_URL`/`CLIENT_ID`/`SCOPE` are the *same* three host-side var names repeated verbatim in all 7 blocks (and the `api` block above) — shared, non-secret, exactly like `INTERNAL_TOKEN` was one shared var before this migration.

- [ ] **Step 3: `dev.env.example`**

Replace lines 118–120 (the `INTERNAL_TOKEN` block and its comment):

```
# ---------------------------------------------------------------------------
# Internal-service OAuth2 (crates/api + all 8 real callers -- see
# docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md)
# ---------------------------------------------------------------------------
# Every real internal caller (the 5 RDM/TfL pollers, trust-consumer,
# schedule-ingest) authenticates to api's /private/* routes with its own
# Authentik service-account username/app-password, obtained via the OAuth2
# Client Credentials Grant. api verifies the resulting JWT locally against
# Authentik's JWKS -- it never sees or holds any of these passwords itself.
# TOKEN_URL/CLIENT_ID/SCOPE are shared across every real caller AND api's
# own group-checking config; USERNAME/PASSWORD are distinct per service.
# Point these at a real (or locally-run) Authentik instance's internal-
# service Provider -- provisioning that Provider/its 8 Service
# Accounts/their groups is NOT part of this app; see the design doc's
# "Explicitly out of scope."
INTERNAL_OAUTH_TOKEN_URL=http://authentik.example.invalid/application/o/token/
INTERNAL_OAUTH_CLIENT_ID=changeme-internal-oauth-client-id
INTERNAL_OAUTH_SCOPE=groups
INTERNAL_OAUTH_ISSUER_URL=http://authentik.example.invalid/application/o/distant-signal-internal/
INTERNAL_OAUTH_USERNAME_INCIDENTS=svc-poller-incidents
INTERNAL_OAUTH_PASSWORD_INCIDENTS=changeme-poller-incidents-app-password
INTERNAL_OAUTH_USERNAME_STATIONS=svc-poller-stations
INTERNAL_OAUTH_PASSWORD_STATIONS=changeme-poller-stations-app-password
INTERNAL_OAUTH_USERNAME_TOCS=svc-poller-tocs
INTERNAL_OAUTH_PASSWORD_TOCS=changeme-poller-tocs-app-password
INTERNAL_OAUTH_USERNAME_LDBWS=svc-poller-ldbws
INTERNAL_OAUTH_PASSWORD_LDBWS=changeme-poller-ldbws-app-password
INTERNAL_OAUTH_USERNAME_TFL=svc-poller-tfl
INTERNAL_OAUTH_PASSWORD_TFL=changeme-poller-tfl-app-password
INTERNAL_OAUTH_USERNAME_TRUST_CONSUMER=svc-trust-consumer
INTERNAL_OAUTH_PASSWORD_TRUST_CONSUMER=changeme-trust-consumer-app-password
INTERNAL_OAUTH_USERNAME_SCHEDULE_INGEST=svc-schedule-ingest
INTERNAL_OAUTH_PASSWORD_SCHEDULE_INGEST=changeme-schedule-ingest-app-password
# api's own required-group values default sensibly in crates/api/src/data/config.rs
# (svc-poller-incidents, etc.) -- only override if your Authentik groups
# use different names. See INTERNAL_OAUTH_GROUP_* in docker-compose.yml.
```

(`INTERNAL_OAUTH_ISSUER_URL` is `api`-only, per Task 4/9 — included here once, in this section, not duplicated per-caller. `example.invalid` matches this file's own established RFC-2606 placeholder convention, same as `SSO_ISSUER_URL` immediately above.)

- [ ] **Step 4: `local.env.example`**

Apply the same replacement to `local.env.example`'s own `INTERNAL_TOKEN` line (currently line 81) and its header comment (lines ~8–14 reference `INTERNAL_TOKEN` by name — update that prose reference too, to name `INTERNAL_OAUTH_*` instead).

- [ ] **Step 5: Validate**

Run: `docker compose --env-file dev.env config --quiet` (after filling `dev.env` from `dev.env.example` per this repo's existing dev workflow, or run `docker compose config` against a temporary copy) to confirm the compose file itself parses and every `:?`-required var resolves without error given the example file's own placeholder values.

- [ ] **Step 6: Commit**

```bash
git add docker-compose.yml dev.env.example local.env.example
git commit -m "docker-compose/.env.example: replace INTERNAL_TOKEN with internal-service OAuth2 config"
```

---

### Task 11: Final verification

**Files:** none (verification only).

**Depends on:** every prior task.

- [ ] **Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: PASS — every one of the 11 crates in `Cargo.toml`'s `[workspace] members` (`common`, `api`, the 5 pollers, `aggregator`, `enricher`, `trust-consumer`, `schedule-ingest`, `schedule-reference`) compiles. `aggregator`/`enricher` are untouched by this plan and should build unchanged.

- [ ] **Step 2: Full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS. This exercises Task 1's 3 `oauth_client` tests, Task 3's 8 `internal_oauth` tests, Task 5's 3 `bearer_token` tests plus every pre-existing `auth.rs` test, and confirms every touched crate's pre-existing suite (poller/trust-consumer/schedule-ingest) still passes.

- [ ] **Step 3: `cargo clippy`**

Run: `cargo clippy --workspace --all-targets -- -D warnings` (or this repo's own established clippy invocation, if different — check for a `justfile`/CI workflow defining one before assuming this exact flag set).
Expected: PASS, no new warnings introduced by this plan's changes.

- [ ] **Step 4: Helm lint + template (repeat of Task 9 Step 11, as a final gate)**

Run: `helm lint charts/distant-signal` and the same `helm template` invocation from Task 9 Step 11.
Expected: PASS, zero `INTERNAL_TOKEN` occurrences in rendered output.

- [ ] **Step 5: `docker compose config` (repeat of Task 10 Step 5, as a final gate)**

Run: `docker compose --env-file dev.env config --quiet` (or the equivalent this repo's own dev workflow uses).
Expected: PASS.

- [ ] **Step 6: Grep sweep for the retired mechanism**

Run: `grep -rn "X-Internal-Token\|x-internal-token\|INTERNAL_TOKEN\|internal_token\|INTERNAL_TOKEN_HEADER" crates/ charts/ docker-compose.yml dev.env.example local.env.example`
Expected: **zero matches**. If any remain, they are either a missed call site (fix it) or evidence this plan's own file lists above were incomplete for some crate/template not enumerated — treat any hit as a bug in execution, not an intentional survivor (this plan's Global Constraints explicitly rule out a dual-acceptance/legacy path).

- [ ] **Step 7: Commit (if Step 6 required any fix-up)**

Only if Step 6 found something to fix:

```bash
git add -A
git commit -m "Sweep: remove remaining X-Internal-Token/INTERNAL_TOKEN references"
```

---

## Self-review notes

- **Spec coverage**: Decision 1 (Task 6's field shape, Task 9's chart credential provisioning) — covered. Decision 2 (Task 3, local JWT/JWKS verification, no introspection) — covered. Decision 3 (Task 4's route-scoping table, Task 5's group check) — covered, extended with the `/stanox-crs` two-group case the spec's own investigation missed (correction 4). Decision 4 (Task 1, hand-rolled `crates/common` client, no `oauth2` crate) — covered. Decision 5 (Task 2/5/6-8/9-10, `Authorization: Bearer` wire format everywhere) — covered. Decision 6 (Task 4/6-10's exact field split, `api` never holding a caller secret) — covered. Decision 7 (no shared code with the MCP sibling plan) — respected: no task in this plan touches `oidc.rs`, `AuthenticatedUser`, or any human-groups mechanism. Testing section's every named case (JWT verification table, JWKS caching, route-scoping default-deny, token cache, startup validation) — each has a task-level test (Tasks 1, 3, 4's startup guards, 5).
- **Explicitly out of scope, honored**: no task defines an Authentik service account, provider, or group; no task edits `devauthentik-blueprints/oauth2-client.yaml`, `authentik-blueprints/`, `crates/api/src/auth/oidc.rs`, `AuthenticatedUser`, or any human-SSO code path; no dual-acceptance window is coded.
- **Type/name consistency check**: `OAuthCredentials`/`OAuthTokenCache::get_token` (Task 1) is the exact type Task 2's `post_batch`/`fetch_last_fetched` and Tasks 6–8's every call site consume — same name throughout. `ServiceClaims`/`ServiceTokenVerifier`/`VerifyError` (Task 3) are the exact names Task 4's `AppState` field and Task 5's `require_internal_oauth` reference. `internal_oauth_routes: Vec<(&'static str, Vec<String>)>` (Task 4) is the exact shape Task 5's lookup consumes. Chart values names (`internalOauth.tokenUrl`/`clientId`/`scope`, `pollers.<name>.internalOauthUsername`/`Password`, `api.internalOauth.issuerUrl`/`clientId`/`groups.*`) are used identically across Task 9's values.yaml/secret.yaml/_helpers.tpl/Deployment edits and Task 10's env-var naming.
