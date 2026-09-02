# Embedded Chatbot: DS-Hosted Chat Orchestrator (Option B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the DS-hosted chat orchestrator itself — an in-app chat UI at `frontend/app/chat/`, a new backend service holding Distant Signal's (DS's) own Anthropic API key that runs the Messages API tool-calling loop against `distant-signal-mcp`'s six tools, a same-origin SSE streaming transport connecting the two, and the `chatbot_allowed_users` cost/access gate — per
`docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`'s
(hereafter "the dual-mode design") Decisions 2 through 5. This is
**Option B only.** Option C (a user connecting `distant-signal-mcp`
directly to their own Claude.ai/Claude Desktop account) and the shared
per-user OAuth foundation both flows sit on top of are **not** designed or
re-planned here — they are `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`'s
(hereafter "the foundation plan") own scope, already written, already
merged to `main`, and already flagged by that plan's own "Not in this
plan" section as the thing a future Option B plan — this one — would need
to pick up.

**Global constraints / prerequisites — read before dispatching any task:**

- **This plan depends on the foundation plan's Tasks 1-8 having landed on
  `main` first.** Confirmed, this session, that they have not: `main`'s
  `charts/distant-signal/values.yaml` `railMcp:` block still has only
  `discord.*`/`ldbws.*` fields, no adapter/OAuth fields anywhere (the
  foundation plan's own Task 8 is what adds `railMcp.frontendOrigin`/
  `internalCompleteToken` and retires `discord.*`); and `distant-signal-mcp`'s
  own checked-out repository (`/workspaces/distant-signal-mcp`, `git log
  --oneline -10`: `9d97b96`..`feff6c2`) has no `src/oauth/` directory at
  all — its most recent commits are the *sibling, orthogonal*
  `train-mcp-integration.md` Tasks 2-5 (anonymous `DsApiClient`,
  `resolve_station` migration, leg-matching, line-catalogue cache, DS
  `liveStatus` annotation), not any of the foundation plan's OAuth work.
  Only the foundation plan's own *planning document* is on `main`
  (`ab0a496`, "Add implementation plan: embedded-chatbot shared OAuth
  foundation + Option C") — none of its code. Every task below that reads
  "distant-signal-mcp already has X" (the `OauthStore`, `/register`,
  `/authorize`, `/token`, `requireBearerToken` gating `/mcp`,
  `res.locals.dsSession`) is citing that plan's own Tasks 1, 3, 5, 7 —
  code this plan assumes exists by the time its own tasks are dispatched,
  not code this plan writes.
- **Narrower than "the whole foundation plan," the one piece of it this
  plan's own Task 1 directly extends is Task 5's `POST /token` endpoint.**
  The foundation plan's own "Not in this plan" section says this in so
  many words: *"Option B's own non-interactive grant path... genuinely
  unresolved by this plan, since this plan only implements the
  interactive authorization-code+PKCE grant Option C... needs. A future
  Option B plan will need to design and add a second grant type to Task
  5's `/token` endpoint."* This plan's Task 1 is that second grant type.
  It is additive to `/token`, not a rewrite of it — the foundation plan's
  own `grant_type=authorization_code` branch, `OauthStore`, and
  `requireBearerToken` middleware are all reused completely unmodified;
  a bearer token minted by either grant type is validated by the exact
  same `requireBearerToken` check (Task 7 of the foundation plan) either
  way, since that check only ever looks up a token hash in the store — it
  has no notion of which grant produced the token it's validating.
- **This plan does not touch `distant-signal-mcp`'s Discord gate, DCR,
  `/authorize`, the consent bridge, or the bearer middleware itself** —
  all foundation-plan territory, already decided (retire Discord, PKCE-
  only DCR, etc.) and, per the constraint above, not yet built. Nothing
  here second-guesses those decisions.
- **This plan does not touch `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s
  own Task 2 (`DsApiClient`, anonymous-only) or Task 6 (`annotateLeg.ts`'s
  TRUST-corroboration tier).** The foundation plan already named making
  `DsApiClient` session-aware for that tier as its own separate, unexecuted
  follow-up (its "Not in this plan" list). Option B's orchestrator never
  calls `DsApiClient` at all — it only ever talks to `distant-signal-mcp`
  over MCP (Task 3 below) and to DS's own `crates/api` directly for one
  narrow purpose (the allowlist check, Task 3's Step 3) via a new, small,
  session-forwarding client of its own, distinct from and not reusing
  `distant-signal-mcp`'s `DsApiClient`. Realizing the TRUST tier for
  Option B conversations remains exactly as unreached as the dual-mode
  design's own "Explicitly out of scope" already says.
- **Where the orchestrator lives, concretely: a new top-level directory in
  *this* repository (`orchestrator/`), not a new fork, and not inside
  `distant-signal-mcp`.** The dual-mode design's Decision 2 settles the
  *shape* (own process, `ClusterIP`-only, holds the Anthropic key,
  `distant-signal-mcp` stays public-facing without it) but leaves *which
  repository* open. `distant-signal-mcp` is a fork of an external
  open-source project (`train-mcp`) with its own CI/release cadence —
  bolting DS's own paid-credential, DS-product-specific orchestrator onto
  that fork would tie an unrelated upstream's repo lifecycle to a feature
  that has nothing to do with it. This repo already builds and ships one
  other TypeScript service this way — `frontend/` (own `Dockerfile`, built
  by `.github/workflows/containers.yml`'s own matrix, `frontend-deployment.yaml`
  templated the same way every Rust crate's deployment is) — the
  orchestrator follows that precedent: `orchestrator/` (own `package.json`,
  own `Dockerfile`), a new matrix entry in `containers.yml`, an 11th
  image alongside the 9 Rust crates + `frontend`, `repository:
  distant-signal/chat-orchestrator` in the chart (this repo's own CI
  builds and publishes it — not `ghcr.io/CHANGE-ME/...`, the pattern
  `railMcp.image.repository` uses for the externally-built fork).

**Tech Stack:** TypeScript (Node) for `orchestrator/` — Anthropic's
TypeScript SDK ships the MCP client helpers this needs
(`mcpTools()`/`client.beta.messages.toolRunner()`, per the chatbot
research doc's own citation, reaffirmed unchanged by the dual-mode
design's Decision 2) — plus `express` (matching `distant-signal-mcp`'s own
web-framework choice, no reason to diverge for a sibling internal
service). TypeScript, small additive change to `src/oauth/token.ts`, in
`distant-signal-mcp`'s own repository (Task 1 only). Rust
(`crates/api`, `sqlx` migrations) for the allowlist (Task 2). Next.js App
Router + TypeScript (`frontend/`) for the proxy route and chat UI (Tasks
4-5). Helm (`charts/distant-signal/`) for deployment (Task 6).

**Specs:** `docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`
— read in full before starting; Decisions 2, 3, 4, 5 are this plan's
direct source, Decision 1 is the shape Task 1 below implements one grant
path of. `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`
— the prerequisite plan; its Tasks 1, 5, 7, 8 are cited by file/interface
throughout. `docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`
— background source for the `structuredContent`/`TrackTrainForm` reuse
sketch (Task 5) and the `chatbot_allowed_users` shape (Task 2), cited
directly where used.

## Architecture

```
frontend/app/chat/  (NEW, Task 5)
  - allowlist-gated page (calls GET /public/chatbot/access, Task 2)
  - message list + input, fetch()+ReadableStream (not EventSource --
    needs a POST body), "track this leg" -> /track?origin=...
        │ same-origin
        ▼
frontend/app/api/chat/route.ts  (NEW, Task 4)
  - forwards Cookie header + user message to the orchestrator
  - streams the orchestrator's SSE response back to the browser verbatim
        │ ClusterIP-internal, Cookie header forwarded
        ▼
orchestrator/  (NEW top-level service, Task 3 -- own repo directory,
own Dockerfile, ClusterIP-only, holds ANTHROPIC_API_KEY)
  1. GET {DS_API_BASE_URL}/public/chatbot/access, forwarding the Cookie
     header -- reject (no Anthropic spend) if not allowed (Task 2)
  2. POST {RAILMCP_BASE_URL}/token, grant_type=
     urn:distant-signal:orchestrator-session, ds_session_cookie_value=
     <the forwarded raw cookie>, authenticated by a shared secret (Task 1)
     -- cached per session, not re-exchanged every message
  3. Anthropic Messages API tool-calling loop (mcpTools()/toolRunner()),
     calling distant-signal-mcp's /mcp with Authorization: Bearer <token
     from step 2>
  4. SSE stream of the model's response back to step 2 (the frontend proxy)
        │ MCP over HTTP, Authorization: Bearer <orchestrator-minted token>
        ▼
distant-signal-mcp  (EXISTING FORK -- foundation plan's Tasks 1-8,
ALREADY LANDED per this plan's prerequisite)
  src/oauth/token.ts  Task 1 of THIS plan adds a second grant_type
                       branch here, alongside the foundation plan's own
                       authorization_code branch -- same OauthStore,
                       same requireBearerToken gate downstream either way
  requireBearerToken (foundation plan Task 7) -- unmodified, validates
                       both grant types' tokens identically
  resolve_station / get_departures / get_arrivals / get_service_detail /
  find_services / plan_journey -- same six tools, unmodified by this plan
        │ anonymous HTTP (foundation plan's own Decision 4 of the
        │ train-mcp design, unchanged)
        ▼
Distant Signal `api` (crates/api)
  GET /public/chatbot/access  (NEW, Task 2) -- ChatbotAuthorizedUser
                                extractor, chatbot_allowed_users table
  /public/*, /Line/*/Status -- unchanged
```

## Task 1: `distant-signal-mcp` — a non-interactive, first-party grant for the orchestrator

**Files** (in `distant-signal-mcp/`, the separate forked repository —
confirmed checked out at `/workspaces/distant-signal-mcp` this session):
- Modify: `src/oauth/token.ts`, `src/config.ts`
- Test: `test/oauth-token.test.ts` (extend)

**Interfaces:**
- Produces: a new branch in the existing `POST /token` handler,
  `grant_type=urn:distant-signal:orchestrator-session` (a private-use URI
  per RFC 6749 §4.5's own convention for extension grant types — deliberately
  namespaced under `urn:distant-signal:`, not a bare word, so it can never
  collide with a future IANA-registered grant type). Request body:
  `{ grant_type, ds_session_cookie_value, resource? }`, authenticated by a
  new header, `X-Orchestrator-Internal-Token` (not the foundation plan's
  `X-Internal-Complete-Token` — see rationale below), not by any DCR
  `client_id`/PKCE material at all. Response: the same `{ access_token,
  token_type, expires_in }` shape the `authorization_code` branch already
  returns.
- Consumed by: `orchestrator/`'s own token-acquisition step (Task 3, Step
  2).
- **Depends on:** the foundation plan's Task 1 (`OauthStore.createAccessToken`,
  reused verbatim) and Task 5 (the existing `/token` router this task adds
  a branch to).

**Why a new grant type on the existing endpoint, not a new endpoint:**
mirrors the dual-mode design's own Decision 1 sketch almost exactly —
*"a first-party-trusted authorization-code variant that skips the consent
screen for an Authentik-recognized first-party application"* — except,
per the foundation plan's own Global Constraints (*"the adapter never
talks to Authentik directly... no new Authentik client registration
either"*), there is no Authentik-recognized client to check here at all;
the trust boundary is a chart-provisioned shared secret between two
processes DS itself deploys, the same shape the foundation plan's own
Task 5/6 already uses for `X-Internal-Complete-Token` between
`distant-signal-mcp` and `frontend/`'s consent bridge. Reusing `/token`
rather than inventing `POST /internal/issue-token` keeps every access
token — regardless of which grant minted it — flowing through one place
(`OauthStore.createAccessToken`, one TTL, one hashing scheme), so
`requireBearerToken` (foundation plan Task 7) needs zero changes and zero
new code path to validate.

**A separate secret from `X-Internal-Complete-Token`, not the same one
reused:** the foundation plan's `internalCompleteToken` protects
`frontend/`'s consent-bridge calls, which only ever *complete or deny* an
*already browser-initiated* pending authorization (foundation plan Task
5) — a caller holding that secret can influence one specific pending
authorization at a time, nothing more. This task's secret, held by
`orchestrator/`, can mint a fresh, unattended access token for **any**
`ds_session_cookie_value` it's handed, on demand, with no pending-authorization
record or browser round trip involved at all — a materially broader
capability. Collapsing the two into one secret would mean a compromise of
either process hands an attacker the other process's full capability;
keeping them separate (`railMcp.orchestratorInternalToken`, provisioned
alongside `railMcp.internalCompleteToken` in Task 6 below, same
auto-generate-if-empty chart pattern) keeps the blast radius of leaking
one scoped to what that one process is actually supposed to be able to do.

**Deliberately not advertised in AS Metadata:** this grant type is never
added to `/.well-known/oauth-authorization-server`'s
`grant_types_supported` (foundation plan Task 2). That document is read
by arbitrary external MCP clients (Claude.ai, per the foundation plan's
own Task 10 verification) discovering what they're allowed to request —
this grant is not for them, it has no PKCE material and no consent step,
and advertising it would invite an external client to try requesting it
(harmlessly rejected by the internal-token check either way, but
needlessly confusing/attractive-looking). It exists only as an
undocumented-to-the-public branch inside `/token`'s own handler,
reachable only by a caller that already holds `orchestratorInternalToken`.

- [ ] **Step 1: Extend `src/config.ts`**

```ts
    /** Shared secret between this process and orchestrator/'s own
     * token-acquisition step (Task 1 of the Option B plan) -- a SEPARATE
     * credential from oauth.internalCompleteToken (frontend/'s consent
     * bridge): that one can only complete an already-pending, browser-
     * initiated authorization; this one mints a fresh token unattended
     * for any session handed to it, a materially broader capability that
     * must not share a blast radius with the narrower one. */
    orchestratorInternalToken: string;
```

`loadConfig`: `orchestratorInternalToken: required(env, 'OAUTH_ORCHESTRATOR_INTERNAL_TOKEN')`.

- [ ] **Step 2: Add the branch to `src/oauth/token.ts`'s existing `POST /token` handler**

```ts
const ORCHESTRATOR_GRANT_TYPE = 'urn:distant-signal:orchestrator-session';
const ORCHESTRATOR_ACCESS_TOKEN_TTL_SECONDS = 60 * 60; // 1 hour -- see
// Open questions/risks: shorter than the interactive grant's 90-day
// token deliberately, since this one is re-minted cheaply per
// orchestrator session rather than held long-term by an external client.

export function registerToken(store: OauthStore, config: Config): Router {
    const router = createRouter();

    router.post('/token', async (req, res) => {
        const body = req.body as Record<string, string | undefined>;

        if (body.grant_type === ORCHESTRATOR_GRANT_TYPE) {
            const provided = req.header('X-Orchestrator-Internal-Token');
            if (provided !== config.oauth.orchestratorInternalToken) {
                return res.status(401).json({ error: 'invalid_client' });
            }
            const { ds_session_cookie_value, resource } = body;
            if (!ds_session_cookie_value) {
                return res.status(400).json({ error: 'invalid_request' });
            }
            if (resource && resource !== config.oauth.issuer) {
                return res.status(400).json({ error: 'invalid_target' });
            }
            const accessToken = randomBytes(32).toString('base64url');
            const accessTokenHash = sha256Base64Url(accessToken);
            await store.createAccessToken(
                accessTokenHash,
                { resource: resource ?? config.oauth.issuer, dsSessionCookieValue: ds_session_cookie_value },
                ORCHESTRATOR_ACCESS_TOKEN_TTL_SECONDS
            );
            return res.json({ access_token: accessToken, token_type: 'Bearer', expires_in: ORCHESTRATOR_ACCESS_TOKEN_TTL_SECONDS });
        }

        if (body.grant_type !== 'authorization_code') {
            return res.status(400).json({ error: 'unsupported_grant_type' });
        }
        // ... foundation plan's existing authorization_code branch, unchanged ...
    });

    return router;
}
```

(`registerToken` gains a second parameter, `config` — update its one call
site in `src/app.ts`, `registerToken(oauthStore, config)`.)

- [ ] **Step 3: Extend `test/oauth-token.test.ts`**

```ts
describe('POST /token, orchestrator-session grant', () => {
    it('mints a bearer token for a forwarded DS session, given the correct internal token', async () => {
        const res = await request(app)
            .post('/token')
            .set('X-Orchestrator-Internal-Token', TEST_ORCHESTRATOR_TOKEN)
            .type('form')
            .send({ grant_type: 'urn:distant-signal:orchestrator-session', ds_session_cookie_value: 'raw-session-token' });
        expect(res.status).toBe(200);
        expect(res.body.token_type).toBe('Bearer');
        expect(res.body.expires_in).toBe(3600);
    });

    it('rejects a missing or wrong X-Orchestrator-Internal-Token', async () => {
        const res = await request(app)
            .post('/token')
            .type('form')
            .send({ grant_type: 'urn:distant-signal:orchestrator-session', ds_session_cookie_value: 'raw-session-token' });
        expect(res.status).toBe(401);
        expect(res.body.error).toBe('invalid_client');
    });

    it('a token minted by this grant is accepted by requireBearerToken exactly like an authorization_code-minted one', async () => {
        const tokenRes = await request(app)
            .post('/token')
            .set('X-Orchestrator-Internal-Token', TEST_ORCHESTRATOR_TOKEN)
            .type('form')
            .send({ grant_type: 'urn:distant-signal:orchestrator-session', ds_session_cookie_value: 'raw-session-token' });
        const mcpRes = await request(app).post('/mcp').set('Authorization', `Bearer ${tokenRes.body.access_token}`).send(MCP_INITIALIZE_BODY);
        expect(mcpRes.status).not.toBe(401);
    });

    it('this grant_type is absent from GET /.well-known/oauth-authorization-server', async () => {
        const res = await request(app).get('/.well-known/oauth-authorization-server');
        expect(res.body.grant_types_supported).not.toContain('urn:distant-signal:orchestrator-session');
    });
});
```

- [ ] **Step 4: Run, commit**

```bash
cd distant-signal-mcp
npm test && npm run typecheck
git add src/oauth/token.ts src/config.ts src/app.ts test/oauth-token.test.ts
git commit -m "Add a non-interactive orchestrator-session grant to /token, for Option B"
```

---

## Task 2: `crates/api` — `chatbot_allowed_users` allowlist + `GET /public/chatbot/access`

**Files:**
- Create: `crates/api/migrations/<timestamp>_chatbot_allowed_users.sql`
- Modify: `crates/api/src/auth.rs` (or a new `crates/api/src/auth/chatbot.rs`,
  matching whichever module shape is already established for `auth.rs` by
  the time this task runs — check first, don't assume), a new route
  module or an addition to an existing one exposing `/public/chatbot/*`
- Test: inline `#[cfg(test)]` module, matching `auth.rs`'s own convention

**Interfaces:**
- Produces: `chatbot_allowed_users(user_id TEXT PRIMARY KEY REFERENCES
  users(id))` table; a `ChatbotAuthorizedUser` extractor wrapping
  `AuthenticatedUser` with one lookup against it; `GET
  /public/chatbot/access` returning `200 { "allowed": true }` for an
  allowlisted logged-in user, `401` (matching `AuthenticatedUser`'s own
  rejection, unchanged) for no session, `403 { "error":
  "chatbot_not_available" }` for a logged-in, non-allowlisted user — never
  `404`, per the dual-mode design's own Error handling section (*"a
  logged-in-but-not-allowlisted user... gets a plain 'not available for
  your account' state, not a `404`... the feature's existence is not a
  secret"*).
- Consumed by: `frontend/app/chat/page.tsx` (Task 5, page-load gate) and
  `orchestrator/` (Task 3, Step 3 — the actual cost-protecting check,
  since a request can reach the orchestrator without ever rendering the
  page).
- **Depends on:** nothing in this plan or the foundation plan — this is
  ordinary `crates/api` schema/route work, independently mergeable and
  parallelizable with Task 1.

This is exactly the shape the chatbot research doc's own Section 3c
sketched (`chatbot_allowed_users (user_id TEXT PRIMARY KEY REFERENCES
users(id))`, *"checked by a new extractor analogous to `AuthenticatedUser`
... wrapping `AuthenticatedUser` with one more table lookup"*) and the
dual-mode design's Decision 5 reaffirmed after re-reading `AuthenticatedUser`'s
struct directly (`auth.rs:176-180`, confirmed no role/tier field exists).

- [ ] **Step 1: Migration**

```sql
CREATE TABLE chatbot_allowed_users (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE
);
```

`ON DELETE CASCADE`, matching this repo's own established posture for a
row that only ever means something in relation to a `users` row (same
reasoning `custom_lines`/`tracked_trains`' own ownership columns already
use elsewhere in this schema) — an allowlist entry for a deleted user is
meaningless, not an orphan worth preserving.

- [ ] **Step 2: `ChatbotAuthorizedUser` extractor**

```rust
/// Wraps `AuthenticatedUser` with one more lookup against
/// `chatbot_allowed_users` -- the dual-mode design's Decision 5 gate.
/// Deliberately a SEPARATE rejection shape from `AuthenticatedUser`'s own
/// 401 ("no session at all") -- a resolved, real user who simply isn't on
/// the list is a genuinely different case, and per that design's own
/// Error handling section must not collapse into a 404 (this isn't an
/// ownership check hiding a secret resource -- the feature's existence
/// isn't a secret).
pub struct ChatbotAuthorizedUser(pub AuthenticatedUser);

impl FromRequestParts<App> for ChatbotAuthorizedUser {
    type Rejection = (axum::http::StatusCode, axum::Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, app).await.map_err(|(status, msg)| {
            (status, axum::Json(serde_json::json!({ "error": msg })))
        })?;
        let allowed = sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM chatbot_allowed_users WHERE user_id = $1)",
            user.id
        )
        .fetch_one(&app.database)
        .await
        .unwrap_or(Some(false))
        .unwrap_or(false);
        if !allowed {
            return Err((
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": "chatbot_not_available" })),
            ));
        }
        Ok(ChatbotAuthorizedUser(user))
    }
}
```

(Match whatever this repo's own `sqlx::query_scalar!`/error-handling idiom
actually is by the time this runs — `auth.rs`'s existing
`get_session_with_user` call is the closest precedent to mirror for error
propagation, not the sketch above verbatim if it's since diverged.)

- [ ] **Step 3: `GET /public/chatbot/access` route**

```rust
async fn chatbot_access(ChatbotAuthorizedUser(_user): ChatbotAuthorizedUser) -> impl IntoResponse {
    axum::Json(serde_json::json!({ "allowed": true }))
}
```

Mount under the existing `/public` router, alongside `/public/auth/session`.

- [ ] **Step 4: Tests**

Table-driven, per the dual-mode design's own Testing section: an
allowlisted user's session passes, a non-allowlisted logged-in user's
session gets `403`, an anonymous request gets `401` (`AuthenticatedUser`'s
existing behavior, unchanged).

- [ ] **Step 5: Run, commit**

```bash
cargo test -p api
git add crates/api/migrations/<timestamp>_chatbot_allowed_users.sql crates/api/src/auth.rs crates/api/src/routes/*.rs
git commit -m "Add chatbot_allowed_users gate + GET /public/chatbot/access (dual-mode design Decision 5)"
```

---

## Task 3: `orchestrator/` — the chat orchestrator service

**Files:**
- Create (new top-level directory): `orchestrator/package.json`,
  `orchestrator/tsconfig.json`, `orchestrator/Dockerfile`,
  `orchestrator/src/config.ts`, `orchestrator/src/dsClient.ts`,
  `orchestrator/src/mcpToken.ts`, `orchestrator/src/chat.ts`,
  `orchestrator/src/app.ts`, `orchestrator/src/index.ts`
- Test: `orchestrator/test/dsClient.test.ts`, `orchestrator/test/mcpToken.test.ts`,
  `orchestrator/test/chat.test.ts`

**Interfaces:**
- Produces: `POST /chat` — `{ conversationId?: string, message: string
}`, requires a `Cookie` header carrying `distant_signal_session`; response
  is `text/event-stream`. `401` if the allowlist check (Step 3) itself
  gets a `401` from `crates/api` (no session at all — mirrors
  `ApiUnauthorizedError`'s own meaning). `403 { "error":
  "chatbot_not_available" }` passthrough if the allowlist check returns
  `403`.
- Consumed by: `frontend/app/api/chat/route.ts` (Task 4), the only caller
  this service ever has (`ClusterIP`-only, per the dual-mode design's
  Decision 2 — no Ingress, no public reachability, unlike
  `distant-signal-mcp`).
- **Depends on:** Task 1 (`/token`'s new grant), Task 2 (`/public/chatbot/access`).

- [ ] **Step 1: Scaffold `package.json`/`tsconfig.json`/`Dockerfile`**

`package.json` dependencies: `express`, `@anthropic-ai/sdk`, `zod`;
dev: `@types/express`, `@types/node`, `typescript`, `vitest`, `supertest`.
`Dockerfile`: same multi-stage `builder`→`runtime-prod` shape
`frontend/Dockerfile` already uses (confirm its exact stage names before
copying — this task should not invent a third convention when this repo
already has one working TS-service Dockerfile to mirror).

- [ ] **Step 2: `src/dsClient.ts` — the allowlist check**

A small, session-forwarding client, genuinely separate from
`distant-signal-mcp`'s own `DsApiClient` (that one is anonymous-only, in a
different repository, and does something different — see this plan's
Global Constraints). Its only job is this one call:

```ts
export async function checkChatbotAccess(dsApiBaseUrl: string, cookieHeader: string): Promise<'allowed' | 'unauthenticated' | 'forbidden'> {
    const res = await fetch(`${dsApiBaseUrl}/public/chatbot/access`, {
        headers: { Cookie: cookieHeader }
    });
    if (res.status === 200) return 'allowed';
    if (res.status === 401) return 'unauthenticated';
    return 'forbidden'; // 403, or any other non-200/401 -- fail closed,
    // never spend an Anthropic token on an ambiguous allow.
}
```

- [ ] **Step 3: `src/mcpToken.ts` — Task 1's grant, with a per-session cache**

```ts
interface CachedToken { accessToken: string; expiresAt: number; }
const cache = new Map<string, CachedToken>(); // keyed by a hash of the
// raw session cookie value, never the raw value itself -- avoids holding
// a second, in-memory-plaintext copy of a live DS session credential
// alongside the one already flowing through each request.

export async function getMcpToken(railMcpBaseUrl: string, orchestratorInternalToken: string, dsSessionCookieValue: string): Promise<string> {
    const key = sha256Hex(dsSessionCookieValue);
    const cached = cache.get(key);
    if (cached && cached.expiresAt > Date.now() + 30_000) return cached.accessToken;

    const res = await fetch(`${railMcpBaseUrl}/token`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded', 'X-Orchestrator-Internal-Token': orchestratorInternalToken },
        body: new URLSearchParams({ grant_type: 'urn:distant-signal:orchestrator-session', ds_session_cookie_value: dsSessionCookieValue })
    });
    if (!res.ok) throw new Error(`token exchange failed: ${res.status}`);
    const body = await res.json() as { access_token: string; expires_in: number };
    cache.set(key, { accessToken: body.access_token, expiresAt: Date.now() + body.expires_in * 1000 });
    return body.access_token;
}
```

An in-process `Map`, not Redis — unlike the foundation plan's own stores
(which must survive a pod restart because an *external, long-lived*
Claude.ai connection depends on them), this cache only ever saves one
redundant `/token` round trip within a single conversation's lifetime; if
this process restarts mid-conversation, the next message simply
re-exchanges, at the cost of one extra request, not a broken feature.

- [ ] **Step 4: `src/chat.ts` — the tool-calling loop**

```ts
import Anthropic from '@anthropic-ai/sdk';

export async function* runChatTurn(opts: {
    anthropic: Anthropic;
    mcpUrl: string;
    mcpBearerToken: string;
    conversationHistory: Anthropic.MessageParam[];
    userMessage: string;
}): AsyncGenerator<{ type: 'text-delta'; text: string } | { type: 'tool-result'; toolName: string; structuredContent?: unknown } | { type: 'done' }> {
    // Uses the SDK's own mcpTools()/toolRunner() (per the chatbot research
    // doc's citation) against opts.mcpUrl with an Authorization: Bearer
    // opts.mcpBearerToken header on every MCP call the SDK makes. Exact
    // system-prompt content, model choice, and max-turns bound are
    // implementation-time decisions -- the dual-mode design's own
    // "Explicitly out of scope" list names "a concrete... system-prompt
    // design" as un-designed by that document, and this plan does not
    // resolve it either; Task 3's own commit should record whatever
    // starting choice is made, flagged as unresearched (mirroring the
    // foundation plan's own posture toward its 90-day TTL/15-minute cache
    // TTL -- an honest starting figure, not a researched one).
}
```

- [ ] **Step 5: `src/app.ts` — `POST /chat`**

```ts
app.post('/chat', async (req, res) => {
    const cookieHeader = req.header('Cookie') ?? '';
    const access = await checkChatbotAccess(config.dsApiBaseUrl, cookieHeader);
    if (access === 'unauthenticated') return res.status(401).json({ error: 'unauthenticated' });
    if (access === 'forbidden') return res.status(403).json({ error: 'chatbot_not_available' });

    const sessionCookieValue = extractSessionCookieValue(cookieHeader); // parses out
    // `distant_signal_session=...` specifically, mirroring
    // frontend/app/connect-claude/authorize/route.ts's own
    // SESSION_COOKIE_NAME constant (foundation plan Task 6) -- kept as a
    // literal string here too, not imported, since this is a different
    // repository/deploy unit with no shared package to import it from.
    const mcpToken = await getMcpToken(config.railMcpBaseUrl, config.orchestratorInternalToken, sessionCookieValue);

    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.flushHeaders();
    for await (const event of runChatTurn({ /* ... */ mcpBearerToken: mcpToken, userMessage: req.body.message, /* ... */ })) {
        res.write(`data: ${JSON.stringify(event)}\n\n`);
    }
    res.end();
});
```

- [ ] **Step 6: Tests**

`dsClient.test.ts`: mocked `fetch`, one case per `checkChatbotAccess`
return value. `mcpToken.test.ts`: mocked `fetch`, confirms a second call
within the cache window does not re-hit `/token`, confirms a call after
`expiresAt` does. `chat.test.ts`: confirms `POST /chat` never reaches
`getMcpToken`/`runChatTurn` when the allowlist check fails (the
cost-protecting property this whole task exists for) — mirrors the
foundation plan's own smoke-test framing (*"a smoke test confirming it
never falls back to an unscoped/anonymous call... if its own token
exchange fails"*), inverted here to "never spends an Anthropic call if the
allowlist check fails."

- [ ] **Step 7: Run, commit**

```bash
cd orchestrator && npm test && npm run typecheck
git add orchestrator/
git commit -m "Add the chat orchestrator service: allowlist gate, MCP token exchange, tool-calling loop, SSE"
```

---

## Task 4: `frontend/app/api/chat/route.ts` — the same-origin SSE proxy

**Files:**
- Create: `frontend/app/api/chat/route.ts`
- Test: `frontend/app/api/chat/route.test.ts`

**Interfaces:**
- Produces: `POST /api/chat` — forwards the incoming request's `Cookie`
  header and JSON body to `orchestrator/`'s `POST /chat`, and streams the
  response body back to the browser unmodified (`Response`'s own
  streaming body support — a Route Handler can return a `ReadableStream`
  directly, no buffering needed).
- Consumed by: `frontend/app/chat/` (Task 5).
- **Depends on:** Task 3 (the orchestrator's `POST /chat` to forward to).

This is the dual-mode design's own Decision 4 (*"SSE over a new same-origin
proxy route... genuinely new infrastructure, no existing pattern
extended"*) made concrete: **a dedicated route, not an extension of the
existing catch-all `frontend/app/api/[...path]/route.ts`.** That proxy's
own design (Current relevant state, dual-mode design) is a generic
body/status passthrough with special-cased redirect/cookie handling for
OIDC — it was never built to hold a connection open and stream a chunked
response for an indeterminate duration, and folding chat's genuinely
different lifecycle (long-lived, streamed, no `Set-Cookie` handling
needed) into that route's existing branching would make an already
dense file harder to reason about for both concerns. A separate route
costs nothing extra (Next.js routes independently per path already) and
keeps each proxy's own concern legible.

- [ ] **Step 1: `route.ts`**

```ts
import { NextRequest } from 'next/server';

function orchestratorBaseUrl(): string {
  const url = process.env.ORCHESTRATOR_BASE_URL;
  if (!url) throw new Error('ORCHESTRATOR_BASE_URL environment variable is not set');
  return url;
}

export async function POST(req: NextRequest) {
  const cookie = req.headers.get('cookie') ?? '';
  const body = await req.text();

  const upstream = await fetch(`${orchestratorBaseUrl()}/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Cookie: cookie },
    body
  });

  // Stream the orchestrator's own SSE body straight through -- no
  // buffering, no re-encoding. Status/error bodies (401/403 JSON) pass
  // through unmodified too, same as every other status this proxy family
  // forwards.
  return new Response(upstream.body, {
    status: upstream.status,
    headers: { 'Content-Type': upstream.headers.get('Content-Type') ?? 'application/json' }
  });
}
```

- [ ] **Step 2: `route.test.ts`**

Mocks `fetch`; confirms the `Cookie` header is forwarded, confirms a 401/403
from the orchestrator passes through with its body intact, confirms a 200
SSE response's body stream is the same object handed to the outgoing
`Response` (not consumed/re-read).

- [ ] **Step 3: Run, commit**

```bash
cd frontend && npm test -- api/chat && npm run build
git add frontend/app/api/chat/route.ts frontend/app/api/chat/route.test.ts
git commit -m "Add the same-origin SSE proxy route for the chat orchestrator"
```

---

## Task 5: `frontend/app/chat/` — the chat UI

**Files:**
- Create: `frontend/app/chat/page.tsx`, `frontend/components/ChatPanel.tsx`,
  `frontend/components/ChatPanel.test.tsx`
- Modify: `frontend/lib/api.ts` (add `getChatbotAccess()`), `frontend/lib/types.ts`
  (port `RenderedTrainLeg`'s shape, per the research doc's own suggestion)

**Interfaces:**
- Produces: `/chat`, a Server Component page gating on `getChatbotAccess()`
  (mirroring `track/mine/page.tsx`'s own `null`-means-"not logged in"
  convention, extended with the allowlist's own third state), rendering
  `ChatPanel` (a Client Component, since it needs `fetch`+`ReadableStream`
  reading and local message state) when allowed.
- **Depends on:** Task 2 (`getChatbotAccess()`'s backing route), Task 4
  (`/api/chat` to `fetch()` from).

- [ ] **Step 1: `lib/api.ts` — `getChatbotAccess()`**

```ts
export async function getChatbotAccess(): Promise<'allowed' | 'unauthenticated' | 'forbidden'> {
  try {
    await fetchJson(`${baseUrl()}/public/chatbot/access`, { headers: { Cookie: (await cookies()).toString() } });
    return 'allowed';
  } catch (err) {
    if (err instanceof ApiUnauthorizedError) return 'unauthenticated';
    return 'forbidden'; // the 403 case -- fetchJson's errorForResponse
    // doesn't special-case 403 today (Step 1 note: extend it with an
    // ApiForbiddenError if that reads more consistently with the rest of
    // this file's own error-type conventions by the time this runs,
    // rather than this string-return sketch).
  }
}
```

- [ ] **Step 2: `app/chat/page.tsx`**

```tsx
export const revalidate = 0; // same reasoning as track/mine/page.tsx --
// no dynamic segment, would otherwise be eligible for static generation.

export default async function ChatPage() {
  const access = await getChatbotAccess();
  if (access === 'unauthenticated') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        <Text>Sign in to ask about live departures, disruptions and journeys.</Text>
        <LoginLink />
      </Stack>
    );
  }
  if (access === 'forbidden') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        <Text c="dimmed">Not available for your account yet.</Text>
      </Stack>
    );
  }
  return <ChatPanel />;
}
```

- [ ] **Step 3: `components/ChatPanel.tsx`**

`'use client'`. Local state: an array of `{ role: 'user' | 'assistant',
content: string }` plus any `RenderedTrainLeg`-shaped structured results
attached to an assistant turn. Submitting a message: `fetch('/api/chat', {
method: 'POST', body: JSON.stringify({ message }) })`, then reads
`response.body.getReader()` manually (native `EventSource` is GET-only
and can't carry a POST body or a custom `Cookie`-forwarding proxy path
the way this needs — a concrete, implementation-time choice this plan
makes explicitly, since the dual-mode design's own Decision 4 left exact
wire framing undesigned), parsing `data: ...\n\n` frames matching Task
3's own `event.type` shapes. A `plan_journey` `tool-result` event whose
`structuredContent` matches `RenderedTrainLeg`'s shape renders as a small
card with a "Track this train" button linking to `/track?origin=<crs>`
(mirroring `TrackTrainForm`'s existing `initialOrigin` prop and the
station-page shortcut that already populates it, `TrackTrainForm.tsx:18-21`)
— not a full pre-fill of every `TrackTrainForm` field, since `plan_journey`'s
`RenderedTrainLeg` (per the research doc's own citation,
`plan-journey.ts:1155-1188`) carries `uid`/`departureAt`/origin, which
maps onto `/track`'s existing `origin` query param exactly; wiring a
second query param for the scheduled departure/uid, if `TrackTrainForm`
doesn't already read one, is this task's own small addition, not a
`TrackTrainForm` rewrite.

- [ ] **Step 4: Tests**

`ChatPanel.test.tsx`: mocks `fetch` returning a scripted SSE stream,
confirms message rendering, confirms the "track this leg" link's `href`
for a fixture `RenderedTrainLeg` tool-result event. Page-level test (if
this repo's own convention tests Server Components this way — check
`track/mine/page.test.tsx`'s own shape first) for each of the three
`getChatbotAccess()` branches.

- [ ] **Step 5: Run, commit**

```bash
cd frontend && npm test -- chat && npm run build
git add frontend/app/chat/ frontend/components/ChatPanel.tsx frontend/components/ChatPanel.test.tsx frontend/lib/api.ts frontend/lib/types.ts
git commit -m "Add /chat: the embedded chat UI, SSE consumption, track-this-leg deep-link"
```

---

## Task 6: Chart + CI — deploy `orchestrator/`

**Files:**
- Create: `charts/distant-signal/templates/orchestrator-deployment.yaml`,
  `charts/distant-signal/templates/orchestrator-service.yaml`
- Modify: `charts/distant-signal/values.yaml`, `charts/distant-signal/templates/secret.yaml`,
  `charts/distant-signal/templates/_helpers.tpl`,
  `charts/distant-signal/templates/frontend-deployment.yaml` (new
  `ORCHESTRATOR_BASE_URL` env var), `.github/workflows/containers.yml`
  (11th matrix entry), `docker-compose.yml`/`docker-compose.dev.yml`
  (profile-gated, matching `rail-mcp`'s own `profiles: ["rail-mcp"]` shape
  — this service only makes sense when `rail-mcp` is also enabled, so it
  should carry that same profile, not a new one, unless the two need to be
  toggled independently for local dev — check whether that's ever true
  before assuming a shared profile).

**Interfaces:**
- Produces: `orchestrator.enabled` (default `false`, matching `railMcp.enabled`'s
  own opt-in-and-costed-feature posture), `orchestrator.image.repository:
  distant-signal/chat-orchestrator` (this repo's own CI builds it, unlike
  `railMcp.image.repository`), `orchestrator.anthropicApiKey` (never
  auto-generated — a genuinely external credential, same posture as
  `railMcp.ldbws.*`), `orchestratorInternalToken` on `railMcp`'s own block
  (Task 1's secret, auto-generated if empty, same 3-way pattern
  `internal-token`/`internalCompleteToken` already use), `ClusterIP`-only
  `orchestrator-service.yaml` (deliberately no `orchestrator` entry
  anywhere in `ingress.yaml` — Decision 2's whole point).
- **Depends on:** Tasks 1, 3 (the env vars this task wires in must match
  what those tasks' own configs actually read); the foundation plan's own
  Task 8 (this task's `railMcp.orchestratorInternalToken` addition sits
  alongside that task's existing `internalCompleteToken` block in the same
  `values.yaml`/`secret.yaml` sections — write this task's diff assuming
  that block already exists, not assuming a stale pre-foundation-plan
  chart).

- [ ] **Step 1: `values.yaml` — new `orchestrator:` block**

```yaml
orchestrator:
  enabled: false
  image:
    repository: distant-signal/chat-orchestrator
    tag: ""
    pullPolicy: IfNotPresent
  service:
    port: 3001
  anthropicApiKey: ""
  existingSecret: ""
  existingSecretAnthropicApiKeyKey: anthropic-api-key
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

- [ ] **Step 2: `railMcp:` block — add `orchestratorInternalToken`, alongside the foundation plan's own `internalCompleteToken`**

```yaml
  orchestratorInternalToken: ""
  existingSecretOrchestratorInternalTokenKey: orchestrator-internal-token
```

- [ ] **Step 3: `secret.yaml` — auto-generate, mirroring `internal-complete-token`'s own block**

```yaml
{{- if .Values.railMcp.enabled }}
{{- $orchestratorToken := .Values.railMcp.orchestratorInternalToken | default (get $existingData "orchestrator-internal-token" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "orchestrator-internal-token" ($orchestratorToken | b64enc) -}}
{{- end }}
{{- if .Values.orchestrator.enabled }}
{{- $apiKey := .Values.orchestrator.anthropicApiKey | default (get $existingData "anthropic-api-key" | b64dec) -}}
{{- if not $apiKey }}{{ fail "orchestrator.anthropicApiKey (or an existingSecret providing it) is required when orchestrator.enabled is true" }}{{ end }}
{{- $_ := set $data "anthropic-api-key" ($apiKey | b64enc) -}}
{{- end }}
```

`anthropicApiKey` fails loudly rather than auto-generating (`randAlphaNum`
would produce a syntactically-present but useless value for a credential
this chart cannot create — same posture `railMcp.ldbws.*` already takes
for its own externally-issued keys).

- [ ] **Step 4: `_helpers.tpl` — `orchestratorFullname`, secret-key resolvers, mirroring the existing `railMcp*SecretKey` helpers' shape**

- [ ] **Step 5: `orchestrator-deployment.yaml`**

Mirrors `railmcp-deployment.yaml`'s structure closely (single container,
no verifier counterpart), with its own env: `PORT`, `ANTHROPIC_API_KEY`
(secret ref), `DS_API_BASE_URL` (`distant-signal.apiBaseUrl`, reused
verbatim — same in-cluster DNS name `railmcp-deployment.yaml` already
uses), `RAILMCP_BASE_URL` (new helper resolving to `distant-signal.railMcpFullname`'s
in-cluster Service DNS name + port — **internal** cluster DNS, not
`railMcp.publicUrl`, since this call never leaves the cluster), `OAUTH_ORCHESTRATOR_INTERNAL_TOKEN`
(secret ref, Task 1's/Step 2's value).

- [ ] **Step 6: `orchestrator-service.yaml`**

`ClusterIP`, mirroring `railmcp-service.yaml`'s own top comment (adapted:
*"no external Ingress/TLS is sketched here, deliberately — Decision 2 of
the dual-mode design requires this service is never reachable from
outside the cluster"*).

- [ ] **Step 7: `frontend-deployment.yaml`**

Add `ORCHESTRATOR_BASE_URL`, conditional on `.Values.orchestrator.enabled`
(check the existing conditional-env-var convention on this file first —
the foundation plan's own Open questions/risks entry 5 flags this exact
gap as unconfirmed against a concrete precedent; if none exists yet by
the time this task runs, add the simplest correct `{{- if }}` guard rather
than inventing a heavier pattern).

- [ ] **Step 8: `containers.yml` — 11th matrix entry**

Add `chat-orchestrator` alongside the existing 9 Rust + 1 frontend
entries, `context: orchestrator`, `dockerfile: orchestrator/Dockerfile`,
no `target` needed unless `orchestrator/Dockerfile` ends up multi-stage
like `frontend/Dockerfile` (in which case name the equivalent
`runtime-prod` stage explicitly, matching that file's own comment
explaining why an implicit last-stage build would be wrong).

- [ ] **Step 9: `docker-compose.yml`/`docker-compose.dev.yml`**

New `orchestrator` service block, `profiles: ["rail-mcp"]` (reusing the
existing profile rather than inventing a second one, since this service
is meaningless without `distant-signal-mcp` already running — confirm
this reasoning holds before writing the diff, per this task's own Files
note above).

- [ ] **Step 10: `helm template`, commit**

```bash
helm template charts/distant-signal --set orchestrator.enabled=true --set railMcp.enabled=true --set orchestrator.anthropicApiKey=test --set railMcp.publicUrl=https://example.com --set railMcp.frontendOrigin=https://example.com > /dev/null
git add charts/distant-signal/ .github/workflows/containers.yml docker-compose.yml docker-compose.dev.yml
git commit -m "Chart + CI: deploy the chat orchestrator, ClusterIP-only, alongside railMcp"
```

---

## Task 7: Manual/external end-to-end verification + cost/NRE-gate cross-reference

**Files:** none.

**Interfaces:**
- Consumes: Tasks 1-6, fully deployed, `railMcp.enabled: true`,
  `orchestrator.enabled: true`, at least one real user's `id` inserted
  into `chatbot_allowed_users`.
- **Depends on:** Tasks 1-6 all landed; per this plan's own Global
  Constraints, also depends on the foundation plan's Tasks 1-8 having
  landed first (restated here since this is the task where that
  dependency actually gets exercised end to end, not just assumed).

- [ ] **Step 1: Confirm a non-allowlisted logged-in user is rejected before any Anthropic spend**

Hit `/chat` (or `/api/chat`) as a logged-in user *not* in
`chatbot_allowed_users`; confirm a `403` and confirm, in
`orchestrator/`'s own logs, that `getMcpToken`/`runChatTurn` were never
reached (Task 3, Step 6's own test covers this in isolation — this step
confirms it against the real deployed allowlist check, not a mock).

- [ ] **Step 2: Exercise a real multi-turn conversation including a tool call**

Ask a real question that triggers `plan_journey` or `find_services`.
Confirm the SSE stream renders incrementally in the browser (not
buffered/delayed until the whole response completes — the actual
user-visible point of Decision 4's streaming choice), confirm
`distant-signal-mcp`'s own logs show a `Bearer` token minted via the
`urn:distant-signal:orchestrator-session` grant (distinguishable from an
`authorization_code`-minted one only by which endpoint issued it — this
step's job is confirming the *orchestrator's own path* actually reaches
`distant-signal-mcp` successfully end to end, since Task 1's own unit
tests only confirm the grant in isolation).

- [ ] **Step 3: Exercise the "track this leg" deep-link**

Confirm a `plan_journey` result's card links to `/track?origin=...` and
that `TrackTrainForm` renders pre-filled correctly on arrival.

- [ ] **Step 4: Confirm token-cache behavior across multiple messages in one conversation**

Send several messages in the same browser session within
`orchestrator/`'s cache TTL (Task 3, Step 3); confirm `distant-signal-mcp`'s
own logs show only one `/token` call for the `urn:distant-signal:orchestrator-session`
grant across all of them, not one per message.

- [ ] **Step 5: Cross-reference the NRE-attribution legal gate — do not silently skip it**

Same gate the foundation plan's own Task 10, Step 5 names: Option B
renders the exact same NRE-sourced tool output Option C does, just
through DS's own chat UI instead of Claude's — the still-unresolved
attribution question applies identically here. Confirm sign-off has
happened (or scope this verification to a non-production environment) per
that task's own reasoning, not repeated in full here.

- [ ] **Step 6: Record the outcome**

No commit. Matches the foundation plan's own Task 10, Step 6 framing: an
unexpected result here signals a gap in how Tasks 1-6 fit together, not
necessarily a bug in any one task's own isolated logic.

---

## Testing

- **Genuinely unit-testable, covered by Tasks 1-6's own test files:** the
  new grant branch's auth/validation and its interoperability with
  `requireBearerToken` (Task 1); the allowlist extractor's three outcomes
  (Task 2); the orchestrator's allowlist-gate-before-spend property, the
  token cache's hit/miss behavior (Task 3); the SSE proxy's cookie
  forwarding and status/body passthrough (Task 4); the chat UI's message
  rendering and track-this-leg link construction (Task 5); `helm template`
  rendering without error across the new values (Task 6).
- **Not unit-testable in this repo, honestly named rather than skipped:**
  whether the orchestrator's own system prompt/tool-loop actually produces
  good conversational answers (a real-Anthropic-API, real-tool-results
  question, outside any test harness this repo has); whether SSE actually
  streams incrementally through a real browser and a real same-origin
  proxy hop, not just that the proxy's `Response` construction is
  syntactically correct (Task 7, Step 2); real container builds/CI matrix
  behavior for the new 11th image (Task 6, Step 8) — not exercised by
  `helm template` alone.
- **Deliberately not tested because deliberately not built by this plan:**
  anything downstream of a session-aware `DsApiClient`/TRUST-corroboration
  tier for Option B conversations — carried forward, unresolved, from both
  the dual-mode design's own "Explicitly out of scope" and the foundation
  plan's own "Not in this plan."

## Not in this plan

- **Anything in `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`'s
  own Tasks 1-10** — the OAuth adapter itself, DCR, `/authorize`, the
  consent bridge, the bearer middleware, Option C's own `/connect-claude`
  page, and that plan's own manual verification. This plan assumes all of
  it, per this plan's Global Constraints, rather than re-planning any of
  it.
- **`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s own
  Task 2/6 revision** (making `DsApiClient` session-aware for
  `annotateLeg.ts`'s TRUST-corroboration tier) — already flagged as a
  separate, unexecuted follow-up by the foundation plan; unaffected by and
  not touched by this plan either. Option B conversations get the same
  "opportunistic, not general" TRUST-tier behavior every other caller of
  `distant-signal-mcp` gets today.
- **A refresh grant for the orchestrator-session token type.** Task 1's
  token is short-lived (1 hour) specifically *because* there's no refresh
  grant — `orchestrator/`'s own cache (Task 3, Step 3) simply re-exchanges
  using the still-live forwarded DS session cookie, which is cheap and
  available on every request already; a refresh-token grant would add
  complexity for no real benefit given that.
- **Per-user or per-conversation Anthropic spend caps beyond the on/off
  allowlist gate.** Exactly the dual-mode design's own Open Question 5,
  carried forward unresolved — Task 2's allowlist answers "who may use it
  at all," not "how much may a given allowlisted user spend."
- **The NRE/Network-Rail-branding attribution question.** Unresolved by
  any document in this chain; Task 7, Step 5 treats it as a hard
  pre-production gate, same as the foundation plan's own Task 10, Step 5,
  without resolving it.
- **Mobile-specific chat UX.** Not addressed anywhere in the dual-mode
  design; not addressed here either.
- **Rate-limiting `orchestrator/`'s own `/chat` endpoint beyond the
  allowlist gate** (e.g. per-user requests/minute) — a real, separate gap
  from the NRE-adjacent public-endpoint rate-limiting the foundation plan
  already flagged for `distant-signal-mcp` itself; this plan's own
  `/chat` is not publicly reachable (Decision 2), which lowers the stakes
  but doesn't eliminate the concern (an allowlisted user's own client bug
  or a compromised browser session could still hammer it) — not designed
  further here.
- **Exact system-prompt content, model choice, or max-tool-turns bound**
  for the orchestrator's own loop (Task 3, Step 4's own note) — flagged as
  an implementation-time decision this plan does not resolve, matching the
  dual-mode design's own explicit exclusion.

## Open questions / risks

1. **Task 1's 1-hour orchestrator-grant TTL and the foundation plan's own
   90-day interactive-grant TTL are both unresearched starting figures**,
   chosen for opposite reasons (this one is cheap to re-mint per
   conversation and never held long-term; that one is held by an external
   client with no refresh grant available) but neither backed by
   measurement. A production deployment may want different numbers for
   either.
2. **This plan's Task 1 assumes the foundation plan's `/token` handler is
   structured in a way a second `grant_type` branch can be added to
   cleanly** (a single `router.post('/token', ...)` with an
   `if/else` on `body.grant_type`, per that plan's own Task 5 code). If
   that code has since been refactored into per-grant-type sub-routers or
   similar by the time this task is dispatched, Task 1's own diff should
   be adapted to whatever the actual current shape is, not force-fit
   against a stale sketch.
3. **Whether `orchestrator/`'s own resource/scaling needs (concurrent SSE
   connections held open, each backed by a live Anthropic streaming call)
   are materially different from `distant-signal-mcp`'s** is not assessed
   here — `orchestrator.resources: {}` (unset) is chosen only because
   that's this chart's own default posture for every other component at
   this stage (`railMcp.resources: {}` included), not because it's been
   sized.
4. **Whether the orchestrator-session grant's non-interactive shape
   actually needs anything from the interactive grant's own
   production-validation (foundation plan's Open Question 4) turns out to
   matter in practice** is exactly the risk that plan's own sequencing
   recommendation named — this plan's Task 1 is the first code that
   actually exercises that question, not just a hypothetical one anymore;
   if `/token`'s `authorization_code` branch turns out to have relied on
   something PKCE-specific in a way this plan's own Task 1 sketch didn't
   anticipate, Task 1 should be revisited against whatever the real,
   landed code looks like, not against this plan's own code sketch.
5. **Whether a compromised `orchestrator` pod is a materially larger or
   smaller blast radius than a compromised `frontend` pod** (both, per
   this plan and the foundation plan respectively, handle a raw DS session
   cookie value in-flight) is not compared here. Flagged plainly, not
   mitigated by a new mechanism — the foundation plan's own Open Question
   3 already names this same class of risk for the consent bridge; this
   plan's Task 3 inherits the identical shape for the orchestrator, one
   more process now.

## References

- `docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`
  — Decisions 1-5, the shared-vs-separate breakdown table, and the
  architecture diagram this plan's own diagram extends.
- `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`
  — the prerequisite plan; its Tasks 1, 2, 5, 7, 8 and its own "Not in
  this plan"/"Open questions" sections are cited directly throughout this
  plan's Global Constraints and Task 1.
- `docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`
  — Section 3c (the `chatbot_allowed_users` sketch, Task 2), Section 4
  (frontend placement, `TrackTrainForm`/`structuredContent` reuse, Task 5).
- This repository, read directly this session: `crates/api/src/auth.rs`
  (`AuthenticatedUser`, `SESSION_COOKIE_NAME`), `frontend/app/api/[...path]/route.ts`,
  `frontend/lib/api.ts`, `frontend/app/track/page.tsx`,
  `frontend/app/track/mine/page.tsx`, `frontend/components/TrackTrainForm.tsx`,
  `charts/distant-signal/templates/railmcp-deployment.yaml`,
  `charts/distant-signal/templates/secret.yaml`,
  `charts/distant-signal/values.yaml`, `.github/workflows/containers.yml`,
  `docker-compose.yml`.
- `/workspaces/distant-signal-mcp` (the forked repository, checked out
  this session): `src/oauth/` confirmed absent, `git log --oneline -10`
  confirmed as the direct evidence this plan's prerequisite has not yet
  landed.
