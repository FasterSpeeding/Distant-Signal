# Embedded Chatbot: Shared OAuth Foundation + Connect-Your-Own-Claude (Option C) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `distant-signal-mcp` (the derived MCP service `docs/superpowers/plans/2026-09-01-train-mcp-integration.md` forks from train-mcp) into a real, spec-compliant OAuth 2.1 authorization server *and* resource server for its own MCP clients — the "adapter auth layer" the sibling `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s Decision 4d names but deliberately does not design to protocol depth — then ship the one thin consumer both the chatbot design and its own sequencing recommendation say to build first: **Option C**, a user connecting `distant-signal-mcp` directly to their own Claude.ai/Claude Desktop account, with Distant Signal's (DS's) own OIDC login as the human-authentication step. Every piece here is genuinely shared between Option C (this plan) and Option B (an embedded DS-hosted chat orchestrator, explicitly **not** built by this plan — see "Not in this plan"): both end up presenting `distant-signal-mcp` with a real, per-user-scoped OAuth bearer token issued by the same adapter, following `docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`'s Decision 1.

**Architecture:**

```
Claude.ai / Claude Desktop (user's own account)
        │ OAuth 2.1: PRM discovery -> DCR -> PKCE authorization request -> browser redirect
        ▼
distant-signal-mcp (own repo, forked from train-mcp -- Task 1 of the sibling
train-mcp-integration plan) -- THIS PLAN adds an "adapter" module inside it:

  Task 1: src/oauth/store.ts        Redis-backed: DCR clients, pending
          (new Redis dependency)     authorizations, issued tokens<->session
        │
        ▼
  Task 2: src/oauth/discovery.ts    GET /.well-known/oauth-protected-resource
                                     GET /.well-known/oauth-authorization-server
        │
        ▼
  Task 3: src/oauth/register.ts     POST /register (open RFC 7591 DCR)
        │
        ▼
  Task 4: src/oauth/authorize.ts    GET /authorize (PKCE request validation,
                                     redirects browser to frontend's bridge)
        │                                    │
        │                                    ▼
        │                          Task 6: frontend/app/connect-claude/authorize/
        │                          (NEW Next.js route -- login-gate via the
        │                           EXISTING /api/auth/login?return_to=,
        │                           consent screen, hands the raw DS session
        │                           cookie to the adapter server-to-server)
        │                                    │
        ▼                                    ▼
  Task 5: src/oauth/internal.ts <────────────┘  (shared internal secret,
          + src/oauth/token.ts                   mirrors X-Internal-Token)
          POST /internal/complete-authorization
          POST /token  (PKCE code exchange -> MCP bearer token)
        │
        ▼
  Task 7: src/oauth/middleware.ts   Applied to all 6 existing tool routes:
                                     401 + WWW-Authenticate handshake;
                                     attaches held DS session to request
                                     context for annotateLeg.ts's TRUST tier
                                     (owned by the OTHER, sibling plan)
        │
        ▼
  Task 8: charts/distant-signal/    Retire railMcp.discord.*; add adapter's
          railmcp-deployment.yaml    Redis wiring + internal secret;
          + values.yaml + ingress.yaml  ingress.railMcp.{enabled,host} (public)
        │
        ▼
  Task 9: frontend/app/connect-claude/   Option C's own thin instructional
          page.tsx                        route -- connector URL + how-to,
                                           distinct from Task 6's OAuth bridge

  Task 10 (non-code): manual/external end-to-end verification against a
  real Authentik instance and a real Claude.ai custom connector, plus the
  NRE-attribution legal-sign-off gate cross-reference.
```

**Tech Stack:** TypeScript (Node, `@modelcontextprotocol/sdk`, `express`, `zod` — already in the fork per `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s own citations) inside `distant-signal-mcp`'s own repository, **plus one new dependency this plan adds**: `ioredis` (Task 1 — justified below, since DCR client registrations and issued access tokens must survive a pod restart, unlike the pure in-memory/no-new-dependency posture the sibling plan's own Global Constraints commit to for *its* tasks). Next.js App Router + TypeScript (`frontend/`, no new dependency). Helm (`charts/distant-signal/`, Go templates) for deployment. No Rust code changes anywhere in this plan — every claim below re-confirms Decision 4d's own "nothing about DS's login UX, Authentik configuration, or `crates/api`'s auth code changes," and this plan's own Task 6 design (below) is built specifically so that claim holds exactly, not just approximately.

**Specs:** `docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md` (hereafter "the dual-mode design") — read in full before starting; Decisions 1, 2, 6 are this plan's direct source. `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md` (hereafter "the train-mcp design") — its Decision 4 (specifically 4a-4d) and Decision 6 are the other direct source; this plan is the "follow-up plan work" Decision 4d's own closing paragraph explicitly defers protocol-level design to. `docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`'s `## Corrections (2026-09-02)` section — background only, not re-restated here.

## Status note — overlap with `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`, investigated directly this session, not assumed

**This is the single most important thing to read before executing either plan.** `docs/superpowers/plans/2026-09-01-train-mcp-integration.md` (commit `faa9dde`) was written and merged **before** the train-mcp design's own Decision 4/6 reversal (commit `7643b5f`, "Corrections: reverse train-mcp Decisions 4/6 to public + DS-OIDC-gated"). It has **never been updated** to reflect that reversal — confirmed directly this session via `git log --oneline -- docs/superpowers/plans/2026-09-01-train-mcp-integration.md` (one commit only) and by reading the plan's own text:

- Its Global Constraints state, verbatim: *"Every DS call this plan adds is anonymous — no `Authorization` header, no session cookie, ever (Decision 4, Auth Option 1)."* Its Task 2 is titled "DS API client + anonymous auth wiring (Decision 4)" and its `DsApiClient` (Task 2, Step 2) sends no `Authorization`/`Cookie` header on any call, with a dedicated smoke test asserting exactly that.
- Its Task 7 (chart) provisions `railMcp.discord.{clientId,allowedUserIds}` as the MCP server's own incoming gate — the Discord-OAuth shape the train-mcp design's own revised Decision 4d says is **"superseded, not stacked alongside DS's OIDC."**
- **The shipped chart matches that stale plan, not the revised design**, confirmed by direct read this session: `charts/distant-signal/values.yaml:767-833`'s `railMcp:` block still only has `discord.{clientId,allowedUserIds}` and `ldbws.*` — no adapter-facing OAuth config exists. `charts/distant-signal/templates/railmcp-service.yaml` is still `ClusterIP` only, its own top comment still reading *"no external Ingress/TLS is sketched here."* `charts/distant-signal/values.yaml:882-910`'s `ingress:` block has `frontend`/`api` sub-blocks only — no `railMcp` sub-block exists anywhere in `ingress.yaml` or `values.yaml` today.

**Conclusion: the shared OAuth foundation this plan builds is genuinely not covered anywhere else — not implemented, not even planned at task/file level.** The train-mcp design's own Decision 4d text says as much directly: *"This is deliberately not a full protocol-level design of the adapter's own endpoints... that belongs in an implementation plan."* This plan is that implementation plan. It is **not** a duplicate of anything in the other plan's Tasks 1-10.

**What this plan does NOT do, and why that's a real, load-bearing coordination risk, not an oversight:** this plan does not touch or fix `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s own Task 2 (`DsApiClient`, still anonymous-only after this plan lands), Task 6 (`annotateLeg.ts`'s TRUST-corroboration tier, still framed as "opportunistic" per that plan's own pre-reversal text, not "general" per the train-mcp design's own revised 3b.6), or Task 7 (chart `railMcp.discord.*` values, which this plan's own Task 8 below retires — a direct file-level collision if both plans' Task 7/8 are executed against the same chart files without coordination). **Whoever executes both plans must treat `docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s Task 2/6/7 as themselves requiring a revision pass** (not covered by any existing plan) to (a) make `DsApiClient`'s `/Train/by-uid/...` call carry the session this plan's Task 7 now makes available via request context, and (b) reconcile chart edits so the two plans' Task 7/8 don't stomp each other's diffs to `charts/distant-signal/templates/railmcp-deployment.yaml`/`values.yaml`. This plan flags the risk precisely rather than silently fixing the other plan's now-stale framing, since that plan's own Tasks 1-5 (fork baseline, `resolve_station` migration, leg-matching, line catalogue caching) are entirely orthogonal to auth and remain correct as written.

**Precondition, not re-executed here:** every task below assumes `distant-signal-mcp`'s forked repository already exists (`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s Task 1 — extract, checkpoint, rename to `distant-signal-mcp`). If that repository does not exist yet when this plan's tasks are dispatched, run that plan's Task 1 first (its Tasks 2-9 are not a precondition — only Task 1 is). This plan uses `distant-signal-mcp/` as the working directory name throughout, matching that plan's own convention.

## Global Constraints

- **The adapter is co-located inside `distant-signal-mcp`'s own Express process, not a separate deployable component.** The train-mcp design's Decision 4d explicitly allows either shape ("the derived service (or a thin companion component deployed alongside it; not designed to a specific process boundary here)"). This plan chooses co-location, for a reason distinct from — and not contradicted by — the dual-mode design's own Decision 2 (which splits Option B's *orchestrator*, holding DS's paid Anthropic API key, into its own `ClusterIP`-only service specifically to keep that credential out of the publicly-reachable process): the adapter built here holds no comparably sensitive secret. Its own credential (Task 8's `railMcp.internalCompleteToken`) is operationally equivalent in sensitivity to `SSO_CLIENT_SECRET`, which this chart already runs inside the publicly-reachable `api` process without a second service boundary. Splitting the adapter out would also contradict the spec text it's implementing almost verbatim ("may be hosted with the resource server").
- **The adapter never re-mints a `crates/api` session, and no new `crates/api` route is added by this plan.** Per the train-mcp design's own Decision 4c: *"the underlying mechanism is exactly right for a different part of this design... the adapter itself, once it has completed a real login, holds and reuses `AuthenticatedUser`'s exact cookie-based session for its own outbound calls to DS."* This plan's Task 6 (the frontend consent bridge) reads the **raw** `distant_signal_session` cookie value (`crates/api/src/auth.rs:63`, `SESSION_COOKIE_NAME`) directly from the incoming request after the user completes DS's real, existing, unmodified `/auth/login`→Authentik→`/auth/callback` flow, and hands that raw value to the adapter server-to-server. The adapter's own subsequent DS calls that need it (Decision 3b.6's TRUST-corroboration tier) present it as an ordinary `Cookie` header — `AuthenticatedUser`'s extractor (`auth.rs:182-198`) does not distinguish a browser-presented cookie from a server-to-server one. This is why the train-mcp design's own "nothing about `crates/api`'s auth code changes" claim holds exactly, not approximately: this plan adds zero Rust code.
- **No re-minted crates/api session means no new Authentik client registration either.** The adapter never talks to Authentik directly. The human-authentication step is DS's frontend's existing, unmodified login (`/api/auth/login` → `crates/api`'s existing `api.sso.clientId`/`clientSecret`) — see Task 6. This resolves what could otherwise read as ambiguous in the train-mcp design's own Decision 4d text ("via one statically pre-registered Authentik client") in the cheaper, zero-new-infrastructure direction: that client is `api.sso.clientId` itself, reused as-is, not a second Authentik application this plan would have to provision and keep in sync.
- **`railMcp.discord.*` is retired, not kept as a secondary gate — this plan makes the call the train-mcp design's own Open Question 8 left unresolved ("whether they're retired, repurposed, or kept as a secondary gate... is explicitly not decided by this document").** Decision 4d's own text says Discord OAuth "is superseded, not stacked alongside DS's OIDC," which only reads coherently as "retire it" — a *kept* secondary gate stacked alongside a mandatory OAuth bearer-token check adds an operational burden (a second allowlist to maintain) for a security property the bearer-token check (Task 7) already fully provides, and the train-mcp design's own reasoning for the reversal was that DS's own user identity is a *strictly stronger* gate than an operator-curated Discord allowlist, not a complementary one. Task 8 removes `railMcp.discord.*` from the chart entirely, rather than leaving unused values behind.
- **PKCE-only public clients — the adapter's DCR endpoint never issues a `client_secret`.** `token_endpoint_auth_methods_supported` is `["none"]`. This is deliberate, not a corner cut: MCP clients like Claude.ai run in contexts (a user's own browser-driven flow, no build-time secret injection point analogous to a server-side app) where a distributed `client_secret` provides no real confidentiality — PKCE (mandatory per the train-mcp design's own 4a citation of OAuth 2.1 §7.5.2) is the actual security boundary for this class of client, and every public MCP-client-facing DCR implementation this session's research found (the dual-mode design's own citations of `claude.com/docs/connectors/building/authentication`) is built around exactly this shape.
- **Redis is shared with `api`/`enricher`'s existing instance, not a new component.** `charts/distant-signal/templates/_helpers.tpl:111-114`'s `distant-signal.redisUrl` helper and the existing `redis-deployment.yaml`/`redis-service.yaml` are reused verbatim (Task 8) — the adapter's own keys are namespaced under a `railmcp:oauth:` prefix (Task 1) to avoid collision with `crates/enricher`'s trigger-queue keys, which is the only coordination this reuse needs.
- **Testing convention, matching the fork's own (unchanged by this plan):** flat `test/*.test.ts` at the repo root, `vitest run`. Every new test file this plan adds goes in `test/`, not colocated with `src/`. Redis-backed store tests use `ioredis-mock` (a new dev dependency, Task 1) rather than a live Redis in CI, mirroring how the sibling plan's own Task 2 mocks `fetch` rather than hitting a live DS.
- **Every new `Authorization`/bearer-related HTTP response uses RFC 6749-shaped error bodies** (`{ error: "invalid_request" | "invalid_client" | "invalid_grant" | ..., error_description }`), per the train-mcp design's own 4a citation ("Token refresh" — "RFC 6749-compliant error codes so Claude's own refresh logic works"), even though this plan does not implement a refresh grant (see Open questions/risks) — the error *shape* still needs to be spec-correct so a client's own retry/error-handling logic doesn't choke on a non-standard body.
- **No refresh-token grant is implemented by this plan.** Access tokens are long-lived (Task 5's `expires_in`, default 90 days) instead. Flagged explicitly in Open questions/risks as a real, deliberate simplification for a v1, not an oversight.
- **Parallelizable tasks:** Task 2 (discovery endpoints) and Task 3 (DCR) each depend only on Task 1 and touch disjoint files — dispatch in parallel. Task 9 (the Option C instructional page) depends only on Task 8's `railMcp.publicUrl`/`ingress.railMcp.host` values existing as a *concept* (the page can be written and tested with a placeholder URL, then have the real one filled in at deploy time) and can be dispatched at any point after this plan's Task 1 lands, in parallel with Tasks 2-8. Task 10 depends on everything.

---

### Task 1: Redis-backed OAuth stores + config

**Files:**
- Create (in `distant-signal-mcp/`): `src/oauth/store.ts`
- Modify: `src/config.ts`, `package.json` (new dependencies: `ioredis`; dev: `ioredis-mock`)
- Test: `test/oauth-store.test.ts` (new)

**Interfaces:**
- Produces: `OauthStore` class with three sub-stores: `registerClient`/`getClient` (DCR registrations, no TTL — a registered client persists until explicitly deleted, matching typical DCR semantics), `createPendingAuthorization`/`getPendingAuthorization`/`deletePendingAuthorization` (10-minute TTL), `createAuthorizationCode`/`consumeAuthorizationCode` (2-minute TTL, single-use — `consumeAuthorizationCode` atomically gets-and-deletes via a Redis `GETDEL`, so a code can never be replayed even under a race), `createAccessToken`/`getAccessToken` (90-day TTL). `Config.oauth: { redisUrl: string; issuer: string; internalCompleteToken: string }`.
- Consumed by: Tasks 2-7.
- **Depends on:** the fork's own checkpointed baseline (`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s Task 1).

- [ ] **Step 1: Add `ioredis` and `ioredis-mock`**

```bash
cd distant-signal-mcp
npm install ioredis
npm install --save-dev ioredis-mock
```

- [ ] **Step 2: Extend `src/config.ts`**

```ts
    oauth: {
        /** Shared with api/enricher's own Redis (crates/enricher's trigger
         * queue) -- see charts/distant-signal/templates/_helpers.tpl's
         * distant-signal.redisUrl helper. This store namespaces every key
         * under `railmcp:oauth:` (store.ts) to avoid collision. */
        redisUrl: string;
        /** This adapter's own issuer/resource identifier -- must equal
         * PUBLIC_URL exactly (RFC 8707 audience binding, train-mcp design
         * 4a). Reusing PUBLIC_URL directly rather than a second env var,
         * since they must always be identical and a second var would just
         * be a way to misconfigure this. */
        issuer: string;
        /** Shared secret between this process and frontend/'s new
         * /connect-claude/authorize route (Task 6) -- mirrors
         * X-Internal-Token's exact shape (crates/api/src/auth.rs), a new,
         * chart-provisioned credential, not reused from that one (they
         * protect different processes). */
        internalCompleteToken: string;
    };
```

Add to `loadConfig`'s return:

```ts
        oauth: {
            redisUrl: required(env, 'OAUTH_REDIS_URL'),
            issuer: required(env, 'PUBLIC_URL'),
            internalCompleteToken: required(env, 'OAUTH_INTERNAL_COMPLETE_TOKEN')
        },
```

- [ ] **Step 3: `src/oauth/store.ts`**

```ts
import Redis from 'ioredis';

export interface DcrClient {
    clientId: string;
    redirectUris: string[];
    clientName?: string;
}

export interface PendingAuthorization {
    clientId: string;
    redirectUri: string;
    codeChallenge: string;
    state: string;
    resource: string;
}

export interface IssuedAuthorizationCode {
    clientId: string;
    redirectUri: string;
    codeChallenge: string;
    resource: string;
    dsSessionCookieValue: string;
}

export interface IssuedAccessToken {
    resource: string;
    dsSessionCookieValue: string;
    /** Set only when the consenting user's DS identity was resolvable at
     * issuance time (Task 6 fetches it via the existing GET
     * /public/auth/session before completing the exchange) -- present for
     * diagnostics/future per-user rate limiting, not required by any tool
     * call this plan wires up. */
    dsUserId?: string;
}

const PREFIX = 'railmcp:oauth:';

export class OauthStore {
    private readonly redis: Redis;

    constructor(redisUrl: string, RedisImpl: typeof Redis = Redis) {
        this.redis = new RedisImpl(redisUrl);
    }

    async registerClient(clientId: string, client: DcrClient): Promise<void> {
        await this.redis.set(`${PREFIX}client:${clientId}`, JSON.stringify(client));
    }

    async getClient(clientId: string): Promise<DcrClient | null> {
        const raw = await this.redis.get(`${PREFIX}client:${clientId}`);
        return raw ? (JSON.parse(raw) as DcrClient) : null;
    }

    async createPendingAuthorization(id: string, pending: PendingAuthorization): Promise<void> {
        await this.redis.set(`${PREFIX}pending:${id}`, JSON.stringify(pending), 'EX', 600);
    }

    async getPendingAuthorization(id: string): Promise<PendingAuthorization | null> {
        const raw = await this.redis.get(`${PREFIX}pending:${id}`);
        return raw ? (JSON.parse(raw) as PendingAuthorization) : null;
    }

    async deletePendingAuthorization(id: string): Promise<void> {
        await this.redis.del(`${PREFIX}pending:${id}`);
    }

    async createAuthorizationCode(code: string, issued: IssuedAuthorizationCode): Promise<void> {
        await this.redis.set(`${PREFIX}code:${code}`, JSON.stringify(issued), 'EX', 120);
    }

    /** Atomic get-and-delete -- a code that is read here can never be read
     * again, closing the replay window a plain GET-then-DEL would leave
     * open under concurrent requests. */
    async consumeAuthorizationCode(code: string): Promise<IssuedAuthorizationCode | null> {
        const raw = await this.redis.getdel(`${PREFIX}code:${code}`);
        return raw ? (JSON.parse(raw) as IssuedAuthorizationCode) : null;
    }

    async createAccessToken(tokenHash: string, issued: IssuedAccessToken, ttlSeconds: number): Promise<void> {
        await this.redis.set(`${PREFIX}token:${tokenHash}`, JSON.stringify(issued), 'EX', ttlSeconds);
    }

    async getAccessToken(tokenHash: string): Promise<IssuedAccessToken | null> {
        const raw = await this.redis.get(`${PREFIX}token:${tokenHash}`);
        return raw ? (JSON.parse(raw) as IssuedAccessToken) : null;
    }
}
```

- [ ] **Step 4: `test/oauth-store.test.ts`**

Using `ioredis-mock` in place of the real `Redis` constructor (constructor takes an injectable `RedisImpl`, mirroring `DsApiClient`'s injectable `fetchImpl` from the sibling plan's Task 2):

```ts
import { describe, expect, it } from 'vitest';
import RedisMock from 'ioredis-mock';
import { OauthStore } from '../src/oauth/store.js';

function store(): OauthStore {
    return new OauthStore('redis://unused', RedisMock as unknown as typeof import('ioredis').default);
}

describe('OauthStore', () => {
    it('round-trips a registered DCR client', async () => {
        const s = store();
        await s.registerClient('c1', { clientId: 'c1', redirectUris: ['https://claude.ai/cb'] });
        expect(await s.getClient('c1')).toEqual({ clientId: 'c1', redirectUris: ['https://claude.ai/cb'] });
    });

    it('returns null for an unknown client', async () => {
        expect(await store().getClient('nope')).toBeNull();
    });

    it('consumeAuthorizationCode returns the code once, then null -- single use', async () => {
        const s = store();
        const issued = { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: 'x', resource: 'https://mcp.example.com', dsSessionCookieValue: 'raw-session-token' };
        await s.createAuthorizationCode('code1', issued);
        expect(await s.consumeAuthorizationCode('code1')).toEqual(issued);
        expect(await s.consumeAuthorizationCode('code1')).toBeNull();
    });

    it('pending authorizations round-trip and can be explicitly deleted', async () => {
        const s = store();
        const pending = { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: 'x', state: 'abc', resource: 'https://mcp.example.com' };
        await s.createPendingAuthorization('req1', pending);
        expect(await s.getPendingAuthorization('req1')).toEqual(pending);
        await s.deletePendingAuthorization('req1');
        expect(await s.getPendingAuthorization('req1')).toBeNull();
    });

    it('access tokens round-trip by hash', async () => {
        const s = store();
        const issued = { resource: 'https://mcp.example.com', dsSessionCookieValue: 'raw-session-token', dsUserId: 'user-123' };
        await s.createAccessToken('hash1', issued, 90 * 24 * 60 * 60);
        expect(await s.getAccessToken('hash1')).toEqual(issued);
    });
});
```

- [ ] **Step 5: Run the tests, commit**

```bash
npm test && npm run typecheck
git add src/oauth/store.ts src/config.ts package.json package-lock.json test/oauth-store.test.ts
git commit -m "Add Redis-backed OAuth stores for the adapter auth layer (train-mcp design Decision 4d)"
```

---

### Task 2: Discovery endpoints — RFC 9728 Protected Resource Metadata + RFC 8414 AS Metadata

**Files:**
- Create: `src/oauth/discovery.ts`
- Modify: `src/app.ts` (mount two new unauthenticated routes)
- Test: `test/oauth-discovery.test.ts`

**Interfaces:**
- Produces: `GET /.well-known/oauth-protected-resource`, `GET /.well-known/oauth-authorization-server`, both static JSON derived from `Config.oauth.issuer`.
- Consumed by: any MCP client's own discovery step (Claude.ai, per the train-mcp design's 4a citation of the MUST requirement); Task 7's `401` responses point here via `WWW-Authenticate`.
- **Depends on:** Task 1 (`Config.oauth.issuer`).

Per the train-mcp design's 4a: *"the MCP server **MUST** implement RFC 9728 Protected Resource Metadata... and **MUST** return it via a `WWW-Authenticate` header on a `401`; the authorization server **MUST** provide RFC 8414 metadata."* Both documents are static per-process — no store lookup, no request-scoped data — so both are plain, cacheable JSON.

- [ ] **Step 1: `src/oauth/discovery.ts`**

```ts
import type { Router } from 'express';
import { Router as createRouter } from 'express';
import type { Config } from '../config.js';

export function registerDiscovery(config: Config): Router {
    const router = createRouter();
    const issuer = config.oauth.issuer;

    router.get('/.well-known/oauth-protected-resource', (_req, res) => {
        res.json({
            resource: issuer,
            authorization_servers: [issuer]
        });
    });

    router.get('/.well-known/oauth-authorization-server', (_req, res) => {
        res.json({
            issuer,
            authorization_endpoint: `${issuer}/authorize`,
            token_endpoint: `${issuer}/token`,
            registration_endpoint: `${issuer}/register`,
            response_types_supported: ['code'],
            grant_types_supported: ['authorization_code'],
            code_challenge_methods_supported: ['S256'],
            token_endpoint_auth_methods_supported: ['none']
        });
    });

    return router;
}
```

- [ ] **Step 2: Mount in `src/app.ts`**

```ts
import { registerDiscovery } from './oauth/discovery.js';
// ... inside buildApp, alongside the existing route mounts ...
app.use(registerDiscovery(config));
```

- [ ] **Step 3: `test/oauth-discovery.test.ts`**

```ts
import { describe, expect, it } from 'vitest';
import request from 'supertest';
import { buildApp } from '../src/app.js';
// ... construct a test Config with oauth.issuer = 'https://mcp.example.com' ...

describe('OAuth discovery endpoints', () => {
    it('serves Protected Resource Metadata naming this server as its own authorization server', async () => {
        const app = buildApp(testOptions());
        const res = await request(app).get('/.well-known/oauth-protected-resource');
        expect(res.status).toBe(200);
        expect(res.body).toEqual({ resource: 'https://mcp.example.com', authorization_servers: ['https://mcp.example.com'] });
    });

    it('serves AS Metadata requiring PKCE S256 and no client secret', async () => {
        const app = buildApp(testOptions());
        const res = await request(app).get('/.well-known/oauth-authorization-server');
        expect(res.body.code_challenge_methods_supported).toEqual(['S256']);
        expect(res.body.token_endpoint_auth_methods_supported).toEqual(['none']);
        expect(res.body.grant_types_supported).toEqual(['authorization_code']);
    });
});
```

(If the fork's own `test/app.test.ts` doesn't already use `supertest` against `buildApp`, check its existing HTTP-testing convention first and match that instead — do not introduce a second HTTP-test-driving library if one is already established.)

- [ ] **Step 4: Run, commit**

```bash
npm test && npm run typecheck
git add src/oauth/discovery.ts src/app.ts test/oauth-discovery.test.ts
git commit -m "Add RFC 9728/8414 OAuth discovery endpoints"
```

---

### Task 3: Dynamic Client Registration — `POST /register` (RFC 7591, open)

**Files:**
- Create: `src/oauth/register.ts`
- Modify: `src/app.ts`
- Test: `test/oauth-register.test.ts`

**Interfaces:**
- Produces: `POST /register` accepting `{ redirect_uris: string[]; client_name?: string; token_endpoint_auth_method?: string }`, returning `201 { client_id, redirect_uris, client_name, client_id_issued_at, token_endpoint_auth_method: 'none' }` or `400` for a malformed/empty `redirect_uris`.
- Consumed by: Claude.ai's own first-contact DCR call (train-mcp design 4a — *"MCP clients and authorization servers SHOULD support... RFC 7591"*); Task 4 validates `client_id`/`redirect_uri` against what this task registers.
- **Depends on:** Task 1 (`OauthStore.registerClient`).

**Genuinely open, unlike Authentik's own DCR** (train-mcp design 4b: *"not an anonymous or open registration unless the configured policies explicitly allow that behavior"*) — no bearer token, no policy check. This is the entire reason the train-mcp design chose the adapter shape over delegating to Authentik's own DCR at all (4c, option (a), rejected).

- [ ] **Step 1: `src/oauth/register.ts`**

```ts
import { randomUUID } from 'node:crypto';
import type { Router } from 'express';
import { Router as createRouter } from 'express';
import { z } from 'zod';
import type { OauthStore } from './store.js';

const registrationRequest = z.object({
    redirect_uris: z.array(z.string().url()).min(1),
    client_name: z.string().optional(),
    token_endpoint_auth_method: z.string().optional()
});

export function registerDcr(store: OauthStore): Router {
    const router = createRouter();

    router.post('/register', async (req, res) => {
        const parsed = registrationRequest.safeParse(req.body);
        if (!parsed.success) {
            return res.status(400).json({ error: 'invalid_client_metadata', error_description: parsed.error.message });
        }
        const clientId = randomUUID();
        await store.registerClient(clientId, {
            clientId,
            redirectUris: parsed.data.redirect_uris,
            clientName: parsed.data.client_name
        });
        res.status(201).json({
            client_id: clientId,
            redirect_uris: parsed.data.redirect_uris,
            client_name: parsed.data.client_name,
            client_id_issued_at: Math.floor(Date.now() / 1000),
            token_endpoint_auth_method: 'none'
        });
    });

    return router;
}
```

- [ ] **Step 2: Mount in `src/app.ts`, alongside Task 2's `registerDiscovery`**

```ts
import { registerDcr } from './oauth/register.js';
// ...
app.use(registerDcr(oauthStore));
```

- [ ] **Step 3: `test/oauth-register.test.ts`**

```ts
describe('POST /register', () => {
    it('registers a client and issues a client_id, no client_secret, PKCE-only', async () => {
        const res = await request(app).post('/register').send({ redirect_uris: ['https://claude.ai/mcp/callback'], client_name: 'Claude' });
        expect(res.status).toBe(201);
        expect(res.body.client_id).toBeTruthy();
        expect(res.body.client_secret).toBeUndefined();
        expect(res.body.token_endpoint_auth_method).toBe('none');
    });

    it('rejects a registration with no redirect_uris', async () => {
        const res = await request(app).post('/register').send({});
        expect(res.status).toBe(400);
        expect(res.body.error).toBe('invalid_client_metadata');
    });

    it('rejects a non-URL redirect_uri', async () => {
        const res = await request(app).post('/register').send({ redirect_uris: ['not-a-url'] });
        expect(res.status).toBe(400);
    });
});
```

- [ ] **Step 4: Run, commit**

```bash
npm test && npm run typecheck
git add src/oauth/register.ts src/app.ts test/oauth-register.test.ts
git commit -m "Add open RFC 7591 Dynamic Client Registration endpoint"
```

---

### Task 4: `/authorize` — PKCE request validation, redirect into the frontend consent bridge

**Files:**
- Create: `src/oauth/authorize.ts`
- Modify: `src/app.ts`, `src/config.ts` (add `frontendOrigin`)
- Test: `test/oauth-authorize.test.ts`

**Interfaces:**
- Produces: `GET /authorize?response_type=code&client_id=&redirect_uri=&code_challenge=&code_challenge_method=S256&resource=&state=`, which validates the request and **302**s the browser to `${frontendOrigin}/connect-claude/authorize?mcp_request_id=<id>`; `400` for a malformed/unregistered request (never a redirect on failure — an attacker-controlled `redirect_uri` must not be trusted before it's validated against the registered client).
- Consumed by: the MCP client's own browser-driven authorization request; Task 6 (frontend bridge) reads `mcp_request_id` back out via `GET /internal/pending-authorization/:id`.
- **Depends on:** Task 1 (pending-authorization store), Task 3 (client validation).

- [ ] **Step 1: Extend `src/config.ts`**

```ts
    /** The frontend's own public origin -- where Task 6's consent-bridge
     * route lives. A genuinely new config value: nothing in the fork today
     * needs to know where DS's frontend is (it only ever talks to `api`
     * directly). */
    frontendOrigin: string;
```

`loadConfig`: `frontendOrigin: required(env, 'DS_FRONTEND_ORIGIN')`.

- [ ] **Step 2: `src/oauth/authorize.ts`**

```ts
import { randomUUID } from 'node:crypto';
import type { Router } from 'express';
import { Router as createRouter } from 'express';
import { z } from 'zod';
import type { Config } from '../config.js';
import type { OauthStore } from './store.js';

const authorizeQuery = z.object({
    response_type: z.literal('code'),
    client_id: z.string(),
    redirect_uri: z.string().url(),
    code_challenge: z.string(),
    code_challenge_method: z.literal('S256'),
    resource: z.string().url().optional(),
    state: z.string()
});

export function registerAuthorize(config: Config, store: OauthStore): Router {
    const router = createRouter();

    router.get('/authorize', async (req, res) => {
        const parsed = authorizeQuery.safeParse(req.query);
        if (!parsed.success) {
            return res.status(400).json({ error: 'invalid_request', error_description: parsed.error.message });
        }
        const { client_id, redirect_uri, code_challenge, resource, state } = parsed.data;

        const client = await store.getClient(client_id);
        if (!client) {
            return res.status(400).json({ error: 'invalid_client' });
        }
        if (!client.redirectUris.includes(redirect_uri)) {
            // Do NOT redirect here -- an unregistered redirect_uri is
            // exactly the case a 302 must never trust (RFC 8707/OAuth 2.1's
            // whole point: the redirect target is only safe to use once
            // it's been checked against what DCR actually registered).
            return res.status(400).json({ error: 'invalid_request', error_description: 'redirect_uri not registered for this client' });
        }
        // RFC 8707 resource indicator -- must name this server exactly, or
        // be omitted (defaulting to this server, the only resource this
        // adapter has ever protected).
        if (resource && resource !== config.oauth.issuer) {
            return res.status(400).json({ error: 'invalid_target' });
        }

        const mcpRequestId = randomUUID();
        await store.createPendingAuthorization(mcpRequestId, {
            clientId: client_id,
            redirectUri: redirect_uri,
            codeChallenge: code_challenge,
            state,
            resource: resource ?? config.oauth.issuer
        });

        const bridge = new URL('/connect-claude/authorize', config.frontendOrigin);
        bridge.searchParams.set('mcp_request_id', mcpRequestId);
        res.redirect(302, bridge.toString());
    });

    return router;
}
```

- [ ] **Step 3: Mount in `src/app.ts`**

```ts
import { registerAuthorize } from './oauth/authorize.js';
// ...
app.use(registerAuthorize(config, oauthStore));
```

- [ ] **Step 4: `test/oauth-authorize.test.ts`**

```ts
describe('GET /authorize', () => {
    it('redirects to the frontend consent bridge for a valid, registered request', async () => {
        await store.registerClient('c1', { clientId: 'c1', redirectUris: ['https://claude.ai/cb'] });
        const res = await request(app).get('/authorize').query({
            response_type: 'code', client_id: 'c1', redirect_uri: 'https://claude.ai/cb',
            code_challenge: 'abc', code_challenge_method: 'S256', state: 'xyz'
        });
        expect(res.status).toBe(302);
        expect(res.headers.location).toMatch(/^https:\/\/ds\.example\.com\/connect-claude\/authorize\?mcp_request_id=/);
    });

    it('400s, never redirects, for an unregistered redirect_uri', async () => {
        await store.registerClient('c1', { clientId: 'c1', redirectUris: ['https://claude.ai/cb'] });
        const res = await request(app).get('/authorize').query({
            response_type: 'code', client_id: 'c1', redirect_uri: 'https://evil.example.com/cb',
            code_challenge: 'abc', code_challenge_method: 'S256', state: 'xyz'
        });
        expect(res.status).toBe(400);
    });

    it('400s for an unknown client_id', async () => {
        const res = await request(app).get('/authorize').query({
            response_type: 'code', client_id: 'nope', redirect_uri: 'https://claude.ai/cb',
            code_challenge: 'abc', code_challenge_method: 'S256', state: 'xyz'
        });
        expect(res.status).toBe(400);
        expect(res.body.error).toBe('invalid_client');
    });

    it('400s for a resource parameter naming a different audience', async () => {
        await store.registerClient('c1', { clientId: 'c1', redirectUris: ['https://claude.ai/cb'] });
        const res = await request(app).get('/authorize').query({
            response_type: 'code', client_id: 'c1', redirect_uri: 'https://claude.ai/cb',
            code_challenge: 'abc', code_challenge_method: 'S256', state: 'xyz', resource: 'https://someone-elses-mcp.example.com'
        });
        expect(res.status).toBe(400);
        expect(res.body.error).toBe('invalid_target');
    });
});
```

- [ ] **Step 5: Run, commit**

```bash
npm test && npm run typecheck
git add src/oauth/authorize.ts src/app.ts src/config.ts test/oauth-authorize.test.ts
git commit -m "Add GET /authorize: PKCE request validation, redirect into the DS consent bridge"
```

---

### Task 5: Internal completion endpoints + `/token`

**Files:**
- Create: `src/oauth/internal.ts`, `src/oauth/token.ts`
- Modify: `src/app.ts`
- Test: `test/oauth-internal.test.ts`, `test/oauth-token.test.ts`

**Interfaces:**
- Produces: `GET /internal/pending-authorization/:id` (returns `{ clientName?: string }` for the consent screen to display — no secret data), `POST /internal/complete-authorization`, `POST /internal/deny-authorization` (all three gated by `X-Internal-Complete-Token`, matching `crates/api/src/auth.rs`'s `X-Internal-Token` shape), `POST /token`.
- Consumed by: Task 6 (frontend bridge, server-to-server); the MCP client's own token-exchange call.
- **Depends on:** Task 1 (all three stores), Task 4 (pending authorizations to complete/deny).

- [ ] **Step 1: Internal-token middleware — `src/oauth/internal.ts`**

```ts
import { randomBytes } from 'node:crypto';
import type { Router } from 'express';
import { Router as createRouter } from 'express';
import type { Config } from '../config.js';
import type { OauthStore } from './store.js';

function requireInternalToken(config: Config) {
    return (req: import('express').Request, res: import('express').Response, next: import('express').NextFunction) => {
        const provided = req.header('X-Internal-Complete-Token');
        if (provided !== config.oauth.internalCompleteToken) {
            return res.status(401).json({ error: 'unauthorized' });
        }
        next();
    };
}

export function registerInternal(config: Config, store: OauthStore): Router {
    const router = createRouter();
    router.use(requireInternalToken(config));

    router.get('/internal/pending-authorization/:id', async (req, res) => {
        const pending = await store.getPendingAuthorization(req.params.id);
        if (!pending) return res.status(404).json({ error: 'not_found' });
        const client = await store.getClient(pending.clientId);
        res.json({ clientName: client?.clientName });
    });

    router.post('/internal/complete-authorization', async (req, res) => {
        const { mcp_request_id, ds_session_cookie_value } = req.body as { mcp_request_id?: string; ds_session_cookie_value?: string };
        if (!mcp_request_id || !ds_session_cookie_value) {
            return res.status(400).json({ error: 'invalid_request' });
        }
        const pending = await store.getPendingAuthorization(mcp_request_id);
        if (!pending) return res.status(404).json({ error: 'not_found' });

        const code = randomBytes(32).toString('base64url');
        await store.createAuthorizationCode(code, {
            clientId: pending.clientId,
            redirectUri: pending.redirectUri,
            codeChallenge: pending.codeChallenge,
            resource: pending.resource,
            dsSessionCookieValue: ds_session_cookie_value
        });
        await store.deletePendingAuthorization(mcp_request_id);

        const redirectUrl = new URL(pending.redirectUri);
        redirectUrl.searchParams.set('code', code);
        redirectUrl.searchParams.set('state', pending.state);
        res.json({ redirectUrl: redirectUrl.toString() });
    });

    router.post('/internal/deny-authorization', async (req, res) => {
        const { mcp_request_id } = req.body as { mcp_request_id?: string };
        if (!mcp_request_id) return res.status(400).json({ error: 'invalid_request' });
        const pending = await store.getPendingAuthorization(mcp_request_id);
        if (!pending) return res.status(404).json({ error: 'not_found' });
        await store.deletePendingAuthorization(mcp_request_id);
        const redirectUrl = new URL(pending.redirectUri);
        redirectUrl.searchParams.set('error', 'access_denied');
        redirectUrl.searchParams.set('state', pending.state);
        res.json({ redirectUrl: redirectUrl.toString() });
    });

    return router;
}
```

Note `randomBytes(32).toString('base64url')` for the authorization code — a genuinely random, high-entropy value, matching `crates/api/src/auth.rs:154-158`'s own `generate_session_token` shape (256 bits, base64url) rather than reinventing a weaker scheme.

- [ ] **Step 2: `src/oauth/token.ts`**

```ts
import { createHash, randomBytes } from 'node:crypto';
import type { Router } from 'express';
import { Router as createRouter } from 'express';
import type { OauthStore } from './store.js';

const ACCESS_TOKEN_TTL_SECONDS = 90 * 24 * 60 * 60;

function sha256Base64Url(input: string): string {
    return createHash('sha256').update(input).digest('base64url');
}

export function registerToken(store: OauthStore): Router {
    const router = createRouter();

    router.post('/token', async (req, res) => {
        const body = req.body as Record<string, string | undefined>;
        if (body.grant_type !== 'authorization_code') {
            return res.status(400).json({ error: 'unsupported_grant_type' });
        }
        const { code, redirect_uri, code_verifier } = body;
        if (!code || !redirect_uri || !code_verifier) {
            return res.status(400).json({ error: 'invalid_request' });
        }

        const issued = await store.consumeAuthorizationCode(code);
        if (!issued) {
            return res.status(400).json({ error: 'invalid_grant', error_description: 'code unknown, expired, or already used' });
        }
        if (issued.redirectUri !== redirect_uri) {
            return res.status(400).json({ error: 'invalid_grant', error_description: 'redirect_uri mismatch' });
        }
        // PKCE S256 verification (OAuth 2.1 mandatory, train-mcp design 4a).
        if (sha256Base64Url(code_verifier) !== issued.codeChallenge) {
            return res.status(400).json({ error: 'invalid_grant', error_description: 'code_verifier does not match code_challenge' });
        }

        const accessToken = randomBytes(32).toString('base64url');
        const accessTokenHash = sha256Base64Url(accessToken);
        await store.createAccessToken(accessTokenHash, { resource: issued.resource, dsSessionCookieValue: issued.dsSessionCookieValue }, ACCESS_TOKEN_TTL_SECONDS);

        res.json({ access_token: accessToken, token_type: 'Bearer', expires_in: ACCESS_TOKEN_TTL_SECONDS });
    });

    return router;
}
```

Access tokens are stored **hashed**, matching `crates/api/src/auth.rs:160-168`'s own `hash_session_token` rationale exactly: "a DB dump/leak alone can't be replayed... only the original random token can." `getAccessToken` (Task 7) hashes an incoming bearer token the same way before looking it up.

- [ ] **Step 3: Mount both in `src/app.ts`**

```ts
import { registerInternal } from './oauth/internal.js';
import { registerToken } from './oauth/token.js';
// ...
app.use(registerInternal(config, oauthStore));
app.use(registerToken(oauthStore));
```

- [ ] **Step 4: `test/oauth-internal.test.ts`**

```ts
describe('internal completion endpoints', () => {
    it('rejects every internal route without the correct X-Internal-Complete-Token', async () => {
        const res = await request(app).post('/internal/complete-authorization').send({});
        expect(res.status).toBe(401);
    });

    it('completes a pending authorization, issues a single-use code, and returns the client redirect_uri with state', async () => {
        await store.createPendingAuthorization('req1', { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: 'x', state: 'xyz', resource: 'https://mcp.example.com' });
        const res = await request(app)
            .post('/internal/complete-authorization')
            .set('X-Internal-Complete-Token', TEST_TOKEN)
            .send({ mcp_request_id: 'req1', ds_session_cookie_value: 'raw-session-token' });
        expect(res.status).toBe(200);
        const url = new URL(res.body.redirectUrl);
        expect(url.origin + url.pathname).toBe('https://claude.ai/cb');
        expect(url.searchParams.get('state')).toBe('xyz');
        expect(url.searchParams.has('code')).toBe(true);
        expect(await store.getPendingAuthorization('req1')).toBeNull();
    });

    it('deny-authorization redirects with error=access_denied and never issues a code', async () => {
        await store.createPendingAuthorization('req2', { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: 'x', state: 'xyz', resource: 'https://mcp.example.com' });
        const res = await request(app)
            .post('/internal/deny-authorization')
            .set('X-Internal-Complete-Token', TEST_TOKEN)
            .send({ mcp_request_id: 'req2' });
        const url = new URL(res.body.redirectUrl);
        expect(url.searchParams.get('error')).toBe('access_denied');
    });
});
```

- [ ] **Step 5: `test/oauth-token.test.ts`**

```ts
describe('POST /token', () => {
    it('exchanges a valid code + matching PKCE verifier for a bearer access token', async () => {
        // code_verifier 'verifier-1' -> sha256/base64url == codeChallenge below (compute once, paste literal)
        await store.createAuthorizationCode('code1', { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: EXPECTED_CHALLENGE_FOR('verifier-1'), resource: 'https://mcp.example.com', dsSessionCookieValue: 'raw-session-token' });
        const res = await request(app).post('/token').type('form').send({ grant_type: 'authorization_code', code: 'code1', redirect_uri: 'https://claude.ai/cb', code_verifier: 'verifier-1' });
        expect(res.status).toBe(200);
        expect(res.body.token_type).toBe('Bearer');
        expect(typeof res.body.access_token).toBe('string');
    });

    it('rejects a mismatched code_verifier', async () => {
        await store.createAuthorizationCode('code2', { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: EXPECTED_CHALLENGE_FOR('verifier-1'), resource: 'https://mcp.example.com', dsSessionCookieValue: 'raw-session-token' });
        const res = await request(app).post('/token').type('form').send({ grant_type: 'authorization_code', code: 'code2', redirect_uri: 'https://claude.ai/cb', code_verifier: 'wrong-verifier' });
        expect(res.status).toBe(400);
        expect(res.body.error).toBe('invalid_grant');
    });

    it('rejects reusing an already-consumed code -- single use', async () => {
        await store.createAuthorizationCode('code3', { clientId: 'c1', redirectUri: 'https://claude.ai/cb', codeChallenge: EXPECTED_CHALLENGE_FOR('verifier-1'), resource: 'https://mcp.example.com', dsSessionCookieValue: 'raw-session-token' });
        const body = { grant_type: 'authorization_code', code: 'code3', redirect_uri: 'https://claude.ai/cb', code_verifier: 'verifier-1' };
        await request(app).post('/token').type('form').send(body);
        const second = await request(app).post('/token').type('form').send(body);
        expect(second.status).toBe(400);
        expect(second.body.error).toBe('invalid_grant');
    });

    it('rejects an unsupported grant_type with the RFC 6749 error shape', async () => {
        const res = await request(app).post('/token').type('form').send({ grant_type: 'client_credentials' });
        expect(res.status).toBe(400);
        expect(res.body.error).toBe('unsupported_grant_type');
    });
});
```

(`EXPECTED_CHALLENGE_FOR` is a small test helper computing `sha256(verifier)` base64url-encoded — write it once at the top of the file using Node's own `crypto`, not a hardcoded magic string, so the test is self-explaining.)

- [ ] **Step 6: Run, commit**

```bash
npm test && npm run typecheck
git add src/oauth/internal.ts src/oauth/token.ts src/app.ts test/oauth-internal.test.ts test/oauth-token.test.ts
git commit -m "Add internal completion endpoints and POST /token (PKCE code exchange)"
```

---

### Task 6: Frontend consent bridge — `frontend/app/connect-claude/authorize/`

**Files:**
- Create: `frontend/app/connect-claude/authorize/route.ts`
- Test: `frontend/app/connect-claude/authorize/route.test.ts`

**Interfaces:**
- Produces: `GET /connect-claude/authorize?mcp_request_id=` — redirects to DS login if not authenticated, otherwise renders a consent confirmation and, on `POST` approval, completes the exchange server-to-server and redirects the browser to the MCP client's own `redirect_uri`.
- Consumed by: Task 4's `/authorize` redirect target.
- **Depends on:** Task 5 (the adapter's `/internal/*` endpoints must exist to call).

This is the one piece of this plan that runs inside `frontend/`, not `distant-signal-mcp`. It is a Route Handler, not a page component, specifically so it can read the incoming request's raw cookies directly (`req.cookies`) and make its own server-to-server `fetch` to the adapter — a Server Component cannot easily do a POST-triggered side effect this way.

- [ ] **Step 1: `frontend/app/connect-claude/authorize/route.ts`**

```ts
import { NextRequest, NextResponse } from 'next/server';

// SESSION_COOKIE_NAME, crates/api/src/auth.rs:63 -- must match exactly.
const SESSION_COOKIE_NAME = 'distant_signal_session';

function railMcpBaseUrl(): string {
  const url = process.env.RAILMCP_BASE_URL;
  if (!url) throw new Error('RAILMCP_BASE_URL environment variable is not set');
  return url;
}

function internalCompleteToken(): string {
  const token = process.env.RAILMCP_INTERNAL_COMPLETE_TOKEN;
  if (!token) throw new Error('RAILMCP_INTERNAL_COMPLETE_TOKEN environment variable is not set');
  return token;
}

export async function GET(req: NextRequest) {
  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  if (!mcpRequestId) {
    return new NextResponse('missing mcp_request_id', { status: 400 });
  }

  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!sessionCookie) {
    // Same login entry point every other authenticated page uses
    // (LoginLink.tsx) -- return_to is a plain relative path with a query
    // string, exactly the shape crates/api/src/auth.rs's validate_return_to
    // already accepts (confirmed directly this session).
    const returnTo = `/connect-claude/authorize?mcp_request_id=${mcpRequestId}`;
    return NextResponse.redirect(new URL(`/api/auth/login?return_to=${encodeURIComponent(returnTo)}`, req.url));
  }

  // Fetch the requesting client's display name (if DCR captured one) for
  // the consent screen -- best-effort, absent on any failure rather than
  // blocking consent on this call succeeding.
  let clientName: string | undefined;
  try {
    const pendingRes = await fetch(`${railMcpBaseUrl()}/internal/pending-authorization/${mcpRequestId}`, {
      headers: { 'X-Internal-Complete-Token': internalCompleteToken() }
    });
    if (pendingRes.ok) {
      clientName = ((await pendingRes.json()) as { clientName?: string }).clientName;
    } else if (pendingRes.status === 404) {
      return new NextResponse('This authorization request has expired. Please try connecting again from Claude.', { status: 410 });
    }
  } catch {
    // Best-effort only -- render the consent screen without a client name
    // rather than fail the whole request on a transient adapter blip.
  }

  return renderConsentScreen({ mcpRequestId, clientName }); // small server-rendered HTML form, POSTing to this same route
}

export async function POST(req: NextRequest) {
  const mcpRequestId = req.nextUrl.searchParams.get('mcp_request_id');
  const sessionCookie = req.cookies.get(SESSION_COOKIE_NAME)?.value;
  if (!mcpRequestId || !sessionCookie) {
    return new NextResponse('invalid request', { status: 400 });
  }
  const form = await req.formData();
  const approved = form.get('decision') === 'approve';

  const path = approved ? 'complete-authorization' : 'deny-authorization';
  const body = approved
    ? { mcp_request_id: mcpRequestId, ds_session_cookie_value: sessionCookie }
    : { mcp_request_id: mcpRequestId };

  const completeRes = await fetch(`${railMcpBaseUrl()}/internal/${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Internal-Complete-Token': internalCompleteToken() },
    body: JSON.stringify(body)
  });
  if (!completeRes.ok) {
    return new NextResponse('Could not complete the connection. Please try again.', { status: 502 });
  }
  const { redirectUrl } = (await completeRes.json()) as { redirectUrl: string };
  return NextResponse.redirect(redirectUrl);
}
```

`renderConsentScreen` is a small helper returning a plain HTML `NextResponse` with two `<button>`s inside a `<form method="POST">` — this route intentionally does not use a React Server Component tree (a Route Handler can't render one directly); this matches this app's own precedent of a minimal, non-Mantine-styled surface for auth-adjacent plumbing (`crates/api`'s own auth routes return bare text/redirects, not styled HTML, for the equivalent reason: this is protocol machinery, not a product page — Task 9's `/connect-claude` page is where the actual designed UI lives).

- [ ] **Step 2: `frontend/app/connect-claude/authorize/route.test.ts`**

```ts
import { describe, expect, it, vi } from 'vitest';
import { GET, POST } from './route';

// Mock `fetch` for the adapter calls; construct NextRequest fixtures with
// and without the session cookie, following this app's existing Route
// Handler test conventions (check frontend/app/api/[...path]/route.test.ts
// if it exists for the exact fixture-construction helper to reuse, rather
// than inventing a second one).

describe('GET /connect-claude/authorize', () => {
  it('redirects to /api/auth/login with a correctly-encoded return_to when not logged in', async () => {
    // ... construct req with no distant_signal_session cookie ...
    const res = await GET(req);
    expect(res.status).toBe(307); // or 302, matching NextResponse.redirect's default
    const location = res.headers.get('location')!;
    expect(location).toContain('/api/auth/login?return_to=');
    expect(decodeURIComponent(location.split('return_to=')[1])).toBe('/connect-claude/authorize?mcp_request_id=req1');
  });

  it('renders a consent screen when a session cookie is present', async () => {
    // ... construct req WITH a distant_signal_session cookie, mock the
    // internal pending-authorization fetch to 200 with a clientName ...
    const res = await GET(req);
    expect(res.status).toBe(200);
    const body = await res.text();
    expect(body).toContain('Claude'); // the mocked clientName
  });

  it('returns 410 when the pending authorization has expired', async () => {
    // ... mock the internal fetch to 404 ...
    const res = await GET(req);
    expect(res.status).toBe(410);
  });
});

describe('POST /connect-claude/authorize', () => {
  it('on approval, forwards the RAW session cookie value to /internal/complete-authorization and redirects to the returned URL', async () => {
    const fetchSpy = vi.spyOn(global, 'fetch').mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?code=abc&state=xyz' }), { status: 200 }));
    // ... req WITH session cookie 'raw-token-value', form field decision=approve ...
    const res = await POST(req);
    expect(res.status).toBe(302);
    expect(res.headers.get('location')).toBe('https://claude.ai/cb?code=abc&state=xyz');
    const [, init] = fetchSpy.mock.calls[0];
    expect(JSON.parse(init!.body as string).ds_session_cookie_value).toBe('raw-token-value');
  });

  it('on denial, calls deny-authorization instead of complete-authorization', async () => {
    const fetchSpy = vi.spyOn(global, 'fetch').mockResolvedValue(new Response(JSON.stringify({ redirectUrl: 'https://claude.ai/cb?error=access_denied&state=xyz' }), { status: 200 }));
    // ... form field decision=deny ...
    await POST(req);
    expect(fetchSpy.mock.calls[0][0]).toContain('/internal/deny-authorization');
  });
});
```

- [ ] **Step 3: Run, commit**

```bash
cd frontend && npm test -- connect-claude && npm run build
git add frontend/app/connect-claude/authorize/route.ts frontend/app/connect-claude/authorize/route.test.ts
git commit -m "Add the DS-side consent bridge for MCP client authorization requests"
```

---

### Task 7: Bearer-token validation middleware — the "gate on the MCP server as a whole"

**Files:**
- Create: `src/oauth/middleware.ts`
- Modify: `src/app.ts`, `src/server.ts`

**Interfaces:**
- Produces: Express middleware `requireBearerToken(store, config)` applied ahead of every existing tool route (the `/mcp` endpoint `buildApp` already mounts, per the sibling plan's own Task 3, Step 3). On success, attaches `res.locals.dsSession = { cookieValue: string } | undefined` to the request for the duration of that call.
- Consumed by: every one of the six MCP tools, transitively (any one of them being reachable at all now requires a valid bearer token — train-mcp design 4d: *"the incoming gate is now 'has this caller completed DS's own login,' not a per-tool check... applies to every tool"*). Concretely consumed by `annotateLeg.ts`'s TRUST-corroboration tier (**owned by the OTHER, sibling plan's Task 6** — this task only makes `res.locals.dsSession` available; wiring `DsApiClient`'s `/Train/by-uid/...` call to read it is that plan's own follow-up revision, flagged in this plan's Status note above, not executed here).
- **Depends on:** Task 5 (`OauthStore.getAccessToken`), Task 2 (discovery URL for the `WWW-Authenticate` header).

- [ ] **Step 1: `src/oauth/middleware.ts`**

```ts
import { createHash } from 'node:crypto';
import type { NextFunction, Request, Response } from 'express';
import type { Config } from '../config.js';
import type { OauthStore } from './store.js';

function sha256Base64Url(input: string): string {
    return createHash('sha256').update(input).digest('base64url');
}

export function requireBearerToken(store: OauthStore, config: Config) {
    return async (req: Request, res: Response, next: NextFunction) => {
        const header = req.header('Authorization');
        const resourceMetadataUrl = `${config.oauth.issuer}/.well-known/oauth-protected-resource`;
        const challenge = () => res.set('WWW-Authenticate', `Bearer resource_metadata="${resourceMetadataUrl}"`);

        if (!header?.startsWith('Bearer ')) {
            challenge();
            return res.status(401).json({ error: 'invalid_token' });
        }
        const token = header.slice('Bearer '.length);
        const issued = await store.getAccessToken(sha256Base64Url(token));
        if (!issued) {
            challenge();
            return res.status(401).json({ error: 'invalid_token', error_description: 'token unknown, expired, or revoked' });
        }
        if (issued.resource !== config.oauth.issuer) {
            // Defense in depth -- Task 5's /token already only ever mints
            // tokens for this issuer's own resource, so this branch should
            // be unreachable in practice; kept as an explicit audience
            // check rather than an assumption, per the train-mcp design's
            // own "MUST NOT accept or pass through a token issued for some
            // other resource" (4a).
            challenge();
            return res.status(401).json({ error: 'invalid_token' });
        }

        res.locals.dsSession = { cookieValue: issued.dsSessionCookieValue };
        next();
    };
}
```

- [ ] **Step 2: Mount ahead of the `/mcp` route in `src/app.ts`**

```ts
import { requireBearerToken } from './oauth/middleware.js';
// ...
app.use('/mcp', requireBearerToken(oauthStore, config));
// existing app.post('/mcp', ...) handler, unchanged in shape by this task
```

- [ ] **Step 3: Thread `res.locals.dsSession` into `buildServer`'s per-request deps**

The sibling plan's own Task 3, Step 3 already threads a `DsApiClient` instance into `buildServer` inside the `/mcp` handler (`src/app.ts`, currently constructing `ds: { client: ds }` where `ds` is a single, process-lifetime, anonymous `DsApiClient`). This task adds the session alongside it, without changing that call's own shape for the four DS-anonymous tools:

```ts
            const server = buildServer({
                ldbws,
                ds: { client: ds, dsSessionCookieValue: res.locals.dsSession?.cookieValue },
                // ...timetable/plan deps, unchanged...
            });
```

**Coordination note, not a step this task executes:** `src/ds/client.ts`'s `DsApiClient` (sibling plan Task 2) has no method today that sends a `Cookie` header at all — every method is deliberately anonymous. Making `annotateLeg.ts`'s `/Train/by-uid/...` call (sibling plan Task 6) actually use `dsSessionCookieValue` requires a new `DsApiClient` method (or a second, session-aware client) that this task does not add — see this plan's Status note and "Not in this plan" below. This step only guarantees the value is *available* by the time that follow-up work happens.

- [ ] **Step 4: Extend `test/app.test.ts` (or add `test/oauth-middleware.test.ts` if that file doesn't already drive `/mcp` end-to-end)**

```ts
describe('bearer token gate on /mcp', () => {
    it('401s with a WWW-Authenticate challenge pointing at Protected Resource Metadata when no Authorization header is sent', async () => {
        const res = await request(app).post('/mcp').send({});
        expect(res.status).toBe(401);
        expect(res.headers['www-authenticate']).toContain('/.well-known/oauth-protected-resource');
    });

    it('401s for a well-formed but unknown bearer token', async () => {
        const res = await request(app).post('/mcp').set('Authorization', 'Bearer not-a-real-token').send({});
        expect(res.status).toBe(401);
    });

    it('accepts a valid, issued bearer token and reaches the MCP handler', async () => {
        await store.createAccessToken(HASH_OF('valid-token'), { resource: ISSUER, dsSessionCookieValue: 'raw-session' }, 3600);
        const res = await request(app).post('/mcp').set('Authorization', 'Bearer valid-token').send(MCP_INITIALIZE_BODY);
        expect(res.status).not.toBe(401);
    });
});
```

- [ ] **Step 5: Run, commit**

```bash
npm test && npm run typecheck
git add src/oauth/middleware.ts src/app.ts src/server.ts test/app.test.ts
git commit -m "Gate every MCP tool call behind a valid adapter-issued bearer token (train-mcp design Decision 4d)"
```

---

### Task 8: Chart — retire `railMcp.discord.*`, provision the adapter's new config, add public `Ingress`

**Files:**
- Modify: `charts/distant-signal/values.yaml`, `charts/distant-signal/templates/railmcp-deployment.yaml`, `charts/distant-signal/templates/ingress.yaml`, `charts/distant-signal/templates/_helpers.tpl`, `charts/distant-signal/templates/secret.yaml`

**Interfaces:**
- Produces: `railMcp.internalCompleteToken` (auto-generated if empty, matching `secrets.internalToken`'s exact 3-way `existingSecret`/explicit-value/`randAlphaNum 32` pattern), `railMcp.frontendOrigin`, `REDIS_URL`/`DS_FRONTEND_ORIGIN`/`OAUTH_INTERNAL_COMPLETE_TOKEN`/`PUBLIC_URL`-derived `OAUTH_REDIS_URL`/`OAUTH_ISSUER` env vars on `railmcp-deployment.yaml`; `ingress.railMcp.{enabled, host}`.
- **Depends on:** Tasks 1-7 (the env vars this task wires in must match what those tasks' `src/config.ts` actually reads).

- [ ] **Step 1: Remove `railMcp.discord.*` from `values.yaml`**

Delete the `discord:` block (currently `values.yaml:790-796`) and its two `existingSecretDiscord*Key` fields (currently `values.yaml:815-816`) entirely — not commented out, removed, per this plan's Global Constraints ("superseded, not stacked").

- [ ] **Step 2: Add the adapter's own new values, alongside the existing `ldbws:`/`existingSecret*` block**

```yaml
  # -- Frontend's own public origin, e.g. https://status.example.com --
  # where the OAuth consent bridge (frontend/app/connect-claude/authorize/)
  # lives. Required when railMcp.enabled is true.
  frontendOrigin: ""
  # -- Shared secret between this service and frontend/'s consent bridge --
  # mirrors secrets.internalToken's exact auto-generate-if-empty pattern
  # (see secret.yaml Step 3 below). Leave empty to auto-generate.
  internalCompleteToken: ""
  existingSecretInternalCompleteTokenKey: internal-complete-token
```

- [ ] **Step 3: `secret.yaml` — auto-generate `railMcp.internalCompleteToken`, mirroring `internal-token`'s block exactly**

Add alongside the existing `internal-token` block (`secret.yaml`, currently around line 36-40):

```yaml
{{- if .Values.railMcp.enabled }}
{{- $railMcpToken := .Values.railMcp.internalCompleteToken | default (get $existingData "internal-complete-token" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "internal-complete-token" ($railMcpToken | b64enc) -}}
{{- end }}
```

- [ ] **Step 4: `_helpers.tpl` — resolver helpers, mirroring the existing `railMcpDiscordClientIdSecretKey` shape exactly**

Replace the two removed Discord helpers with:

```gotemplate
{{- define "distant-signal.railMcpInternalCompleteTokenSecretKey" -}}
{{- if .Values.railMcp.existingSecret }}
{{- .Values.railMcp.existingSecretInternalCompleteTokenKey }}
{{- else }}
{{- print "internal-complete-token" }}
{{- end }}
{{- end }}
```

- [ ] **Step 5: `railmcp-deployment.yaml` — remove the two `DISCORD_*` env entries, add the adapter's own**

Remove the `DISCORD_CLIENT_ID`/`DISCORD_ALLOWED_USER_IDS` blocks (currently lines 66-75). Add, alongside the existing `DS_API_BASE_URL`/`DS_LINE_CATALOGUE_TTL_MS` entries:

```yaml
            - name: OAUTH_REDIS_URL
              value: {{ include "distant-signal.redisUrl" . | quote }}
            - name: OAUTH_ISSUER
              value: {{ .Values.railMcp.publicUrl | quote }}
            - name: DS_FRONTEND_ORIGIN
              value: {{ .Values.railMcp.frontendOrigin | quote }}
            - name: OAUTH_INTERNAL_COMPLETE_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: {{ include "distant-signal.railMcpInternalCompleteTokenSecretKey" . }}
```

(`OAUTH_ISSUER` reuses the already-existing `railMcp.publicUrl` value verbatim — per Task 1's config comment, `Config.oauth.issuer` and `PUBLIC_URL` must always be identical, so no new values.yaml field is introduced for it.)

Also update this file's own top comment (currently *"In-cluster DNS name for this chart's own `api` Service -- no new DS-side route or auth needed (Decision 4: every DS call this service makes is anonymous)"*, at the `DS_API_BASE_URL` entry) — that comment is now **stale** for the `/Train/by-uid/...` call this plan's Task 7 makes newly reachable via a held session; update it to note that `resolve_station`/`/public/lines`/`/Line/.../Status` calls remain anonymous but `/Train/by-uid/...` (once the sibling plan's own follow-up wires it, per this plan's Status note) will not be.

- [ ] **Step 6: Also add `frontend/`'s own two new env vars — `frontend-deployment.yaml`**

```yaml
            - name: RAILMCP_BASE_URL
              value: {{ include "distant-signal.railMcpInClusterUrl" . | quote }}
            - name: RAILMCP_INTERNAL_COMPLETE_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.railMcpSecretName" . }}
                  key: {{ include "distant-signal.railMcpInternalCompleteTokenSecretKey" . }}
```

Add the small `distant-signal.railMcpInClusterUrl` helper to `_helpers.tpl` (`http://{{ include "distant-signal.railMcpFullname" . }}:{{ .Values.railMcp.service.port }}`, mirroring `distant-signal.apiBaseUrl`'s own existing shape) — these two env vars should render **only when `.Values.railMcp.enabled` is true** (wrap in `{{- if .Values.railMcp.enabled }}`), since the frontend must keep working when the whole feature is off, matching this chart's existing posture for every other opt-in component's frontend-facing wiring (none exists yet for `railMcp`, so this is the first instance of that pattern for this component — model it on how `scheduleFeed`'s own opt-in env vars are guarded elsewhere in this chart, if such a guard exists there, rather than inventing a new convention).

- [ ] **Step 7: `ingress.yaml` — add the `railMcp` rule**

```yaml
{{- if and .Values.ingress.railMcp.enabled (not .Values.ingress.railMcp.host) }}
{{- fail "ingress.railMcp.enabled is true but ingress.railMcp.host is empty. Set it to the hostname the derived MCP service should be served on, or set ingress.railMcp.enabled=false." }}
{{- end }}
{{- if and .Values.ingress.railMcp.enabled (not .Values.railMcp.enabled) }}
{{- fail "ingress.railMcp.enabled is true but railMcp.enabled is false. A public Ingress rule for a service that isn't deployed makes no sense -- set railMcp.enabled=true too." }}
{{- end }}
```

(placed alongside the existing two guards at the top of the file, before the `apiVersion:` line) and a third rule inside `spec.rules`:

```yaml
    {{- if and .Values.ingress.railMcp.enabled .Values.ingress.railMcp.host }}
    - host: {{ .Values.ingress.railMcp.host | quote }}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {{ include "distant-signal.railMcpFullname" . }}
                port:
                  number: {{ .Values.railMcp.service.port }}
    {{- end }}
```

Add to `values.yaml`'s `ingress:` block, alongside `frontend`/`api`:

```yaml
  railMcp:
    # -- Public MCP endpoint. Unlike ingress.frontend (default true) and
    # matching ingress.api's own posture, defaults false: exposing this
    # publicly is only meaningful once the adapter auth layer (this
    # plan's Tasks 1-7) is actually running, and per the train-mcp
    # design's own Licensing note, the NRE-attribution legal question
    # should be resolved before real users can reach this (Task 10).
    enabled: false
    host: ""
```

**No `railMcp` `ClusterIP` Service change is needed** — `railmcp-service.yaml`'s own comment ("no external Ingress/TLS is sketched here... An operator who wants to expose this externally fronts it with their own Ingress/LoadBalancer") describes exactly the relationship this Ingress rule now formalizes in-chart, mirroring `ingress.api`'s existing relationship to `api-service.yaml`. Update that file's own top comment to say so, replacing the now-stale "no external Ingress/TLS is sketched here" line.

- [ ] **Step 8: `helm template` smoke check**

```bash
helm template charts/distant-signal --set railMcp.enabled=true --set railMcp.publicUrl=https://mcp.example.com --set railMcp.frontendOrigin=https://status.example.com --set ingress.enabled=true --set ingress.railMcp.enabled=true --set ingress.railMcp.host=mcp.example.com > /dev/null
```

Expected: renders with no `fail` triggered, `OAUTH_REDIS_URL`/`OAUTH_ISSUER`/`DS_FRONTEND_ORIGIN`/`OAUTH_INTERNAL_COMPLETE_TOKEN` all present on the `railmcp` Deployment, no `DISCORD_*` env var anywhere in the output (`helm template ... | grep DISCORD` prints nothing), and the new `railMcp` Ingress rule present.

Also confirm the guard fires correctly:

```bash
helm template charts/distant-signal --set ingress.enabled=true --set ingress.railMcp.enabled=true --set ingress.railMcp.host=mcp.example.com 2>&1 | grep -q "railMcp.enabled is false"
```

- [ ] **Step 9: Commit**

```bash
git add charts/distant-signal/values.yaml charts/distant-signal/templates/railmcp-deployment.yaml charts/distant-signal/templates/frontend-deployment.yaml charts/distant-signal/templates/ingress.yaml charts/distant-signal/templates/_helpers.tpl charts/distant-signal/templates/secret.yaml charts/distant-signal/templates/railmcp-service.yaml
git commit -m "Retire railMcp.discord.*, provision the adapter's own config, add a public railMcp Ingress rule"
```

---

### Task 9: Option C's DS-side instructional page — `frontend/app/connect-claude/`

**Files:**
- Create: `frontend/app/connect-claude/page.tsx`
- Test: `frontend/app/connect-claude/page.test.tsx`

**Interfaces:**
- Produces: `/connect-claude`, a static, login-gated instructional page (distinct from Task 6's `/connect-claude/authorize` protocol route — this is the human-readable "how to" page a user visits *before* ever starting the OAuth dance).
- **Depends on:** conceptually depends on Task 8's `ingress.railMcp.host` existing as a real value at deploy time; can be written and tested against a placeholder now.

Per the dual-mode design's own Decision 6: *"the connector URL for `distant-signal-mcp`... and static instructions mirroring the documented Claude.ai flow... plausibly gated behind DS's own login the same way any other authenticated route is, since a logged-out visitor has no DS identity to connect to in the first place."*

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import ConnectClaudePage from './page';
import * as api from '@/lib/api';

vi.mock('@/lib/api');

describe('/connect-claude', () => {
  it('shows a login prompt when not authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false });
    render(await ConnectClaudePage());
    expect(screen.getByText(/log in/i)).toBeInTheDocument();
  });

  it('shows the connector URL and step-by-step instructions when authenticated', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, email: 'rider@example.com', name: 'Ada Rider' });
    render(await ConnectClaudePage());
    expect(screen.getByText(/Customize/)).toBeInTheDocument();
    expect(screen.getByText(/Add custom connector/i)).toBeInTheDocument();
    expect(screen.getByText(process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL ?? '')).toBeInTheDocument();
  });
});
```

(Match whatever this repo's actual `SessionInfo`/`getSession()` shape is at execution time — `frontend/lib/types.ts:193`'s `SessionInfo` interface and `frontend/lib/api.ts:188`'s `getSession()` — confirmed both exist this session; do not re-invent a session check when this one already gates `frontend/app/track/mine/page.tsx` and every other authenticated route the same way.)

- [ ] **Step 2: Write `frontend/app/connect-claude/page.tsx`**

```tsx
import { Alert, Anchor, Code, List, Stack, Text, Title } from '@mantine/core';
import { getSession } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';

export const revalidate = 0;

/** The MCP server's own public URL -- baked in at build/deploy time via
 * NEXT_PUBLIC_RAILMCP_PUBLIC_URL (must match railMcp.publicUrl /
 * ingress.railMcp.host from the chart -- Task 8). Blank in any deployment
 * where railMcp isn't enabled; this page still renders in that case, just
 * with a placeholder, since hiding the whole route behind a feature flag
 * is more chart-wiring than this thin a page needs. */
const RAILMCP_PUBLIC_URL = process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL ?? '(not configured on this deployment)';

export default async function ConnectClaudePage() {
  const session = await getSession();

  if (!session.authenticated) {
    return (
      <Stack>
        <Title order={1}>Connect Claude to Distant Signal</Title>
        <Text>Log in to Distant Signal first, then come back here to connect your own Claude.ai or Claude Desktop account.</Text>
        <LoginLink>Log in</LoginLink>
      </Stack>
    );
  }

  return (
    <Stack>
      <Title order={1}>Connect Claude to Distant Signal</Title>
      <Text>
        Distant Signal exposes an MCP server so you can ask Claude directly about UK train departures,
        arrivals, and delay-aware journey planning -- inside Claude's own app, using your own Claude
        account. This does not use any of Distant Signal's own conversation features; Claude handles
        the whole conversation itself.
      </Text>
      <Alert color="blue">
        Connecting requires a Pro, Max, Team, or Enterprise Claude plan for full support (a free Claude.ai
        account gets one custom connector).
      </Alert>
      <List type="ordered">
        <List.Item>In Claude.ai or Claude Desktop, open <strong>Customize &gt; Connectors</strong>.</List.Item>
        <List.Item>Click <strong>+</strong>, then <strong>Add custom connector</strong>.</List.Item>
        <List.Item>
          Enter this URL: <Code>{RAILMCP_PUBLIC_URL}</Code>
        </List.Item>
        <List.Item>
          Approve access when prompted -- you'll be sent to Distant Signal's own login if you aren't already
          signed in here, then asked to confirm the connection.
        </List.Item>
      </List>
      <Text size="sm" c="dimmed">
        Conversations happen entirely inside Claude's own interface, billed to your own Claude plan --
        Distant Signal never sees the conversation itself, only the specific train/line/journey lookups
        Claude asks it to run on your behalf.
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 3: Run the tests, run the build**

```bash
cd frontend && npm test -- connect-claude && npm run build
```

- [ ] **Step 4: Commit**

```bash
git add frontend/app/connect-claude/page.tsx frontend/app/connect-claude/page.test.tsx
git commit -m "Add /connect-claude: Option C's instructional route for connecting a user's own Claude account"
```

---

### Task 10: Manual/external end-to-end verification (not unit-testable) + legal-gate cross-reference

**Files:** none — this task runs the deployed feature and inspects it directly.

**Interfaces:**
- Consumes: Tasks 1-9, fully deployed (`railMcp.enabled: true`, `ingress.railMcp.enabled: true`, a real Authentik instance, a real DNS name reachable from Anthropic's published egress range).
- **Depends on:** Tasks 1-9 all landed and deployed to a real environment.

Per this plan's own honest accounting: the great majority of this plan's *logic* (PKCE validation, DCR, code/token issuance, the consent bridge's cookie-forwarding) is unit-testable and covered by Tasks 1-7's own automated tests. What is **not** unit-testable in this repo's existing test setup is whether the real, live protocol handshake actually satisfies a genuine external MCP client (Claude.ai) end to end — that requires a real deployment, a real Authentik login, and a real Anthropic-side connector. No task in this plan invents automated coverage for this; it is named here plainly, matching this repo's own established convention for exactly this kind of gap (`docs/superpowers/plans/2026-09-01-pwa-manifest.md`'s own Task 4).

- [ ] **Step 1: Confirm public reachability from Anthropic's own documented egress range**

The dual-mode design's own citation (from the chatbot research doc): Anthropic's connector infrastructure reaches a connector server from `160.79.104.0/21`. Confirm the deployed `ingress.railMcp.host` is reachable from outside the cluster's own network (not merely from inside), and that no firewall/security-group rule scoped to internal traffic accidentally blocks that range.

- [ ] **Step 2: Add `distant-signal-mcp` as a custom connector in a real Claude.ai account**

Follow the exact steps `/connect-claude` (Task 9) documents. Confirm: Claude's own DCR call against `POST /register` succeeds, the browser is redirected to `/authorize`, then to `/connect-claude/authorize`, then (if not already logged in) through DS's real login, then to the consent screen, and approving it lands back in Claude.ai with the connection showing as active.

- [ ] **Step 3: Exercise at least one tool call from inside a real Claude.ai conversation**

Ask Claude (with the connector active) to resolve a station or check a line's status. Confirm the call succeeds, and confirm in `distant-signal-mcp`'s own logs that `Authorization: Bearer ...` was present and validated (Task 7) — this is the single clearest signal the whole chain (DCR → authorize → consent → token → bearer validation) is wired correctly end to end, not just correct in isolation per-task.

- [ ] **Step 4: Confirm token persistence across a pod restart**

Restart the `railmcp` Deployment's pod (`kubectl rollout restart` or equivalent) mid-session and confirm the previously-issued bearer token still works afterward (Task 1's Redis-backed persistence, not the in-memory alternative this plan explicitly avoided).

- [ ] **Step 5: Cross-reference the NRE-attribution legal gate — do not silently skip it**

The train-mcp design's own Licensing note states plainly that this reversal (a publicly reachable, authenticated-but-not-operator-curated MCP endpoint) "raises the stakes" on the still-unresolved NRE/Network-Rail-branding "presentation" question, and that `railMcp.enabled`/`ingress.railMcp.enabled` "should not be flipped to `true` in any real deployment ahead of that sign-off." This task does not resolve that question (no task in this plan does — see "Not in this plan"). Before Steps 1-4 above are run against a real, publicly reachable, real-user-facing deployment (as opposed to a private test environment with no real NRE-sourced data exposed to a genuinely external party), confirm that sign-off has actually happened. If it hasn't, this task's own verification should still run — but scoped to a non-production environment, not the real public deployment.

- [ ] **Step 6: Record the outcome**

No commit is produced by this task. If any check surfaces an unexpected result, treat it as a signal the *protocol wiring* between Tasks 1-9 has a gap Tasks 1-7's own unit tests didn't catch (each task tested its own piece in isolation; this task is the first point anything exercises the full chain together) — not evidence any individual task's own logic is wrong.

---

## Testing

Summarized across tasks, stated plainly per the task brief's own instruction to be honest about what's actually verifiable in this repo:

- **Genuinely unit-testable, covered by Tasks 1-7's own test files:** the Redis-backed stores (Task 1), both discovery documents' exact shape (Task 2), DCR's request validation (Task 3), `/authorize`'s PKCE-request validation and redirect-target safety — specifically that an unregistered `redirect_uri` never becomes a redirect target (Task 4), the internal completion endpoints' auth gate and single-use code issuance (Task 5), PKCE code-verifier validation and single-use code exchange at `/token` (Task 5), the consent bridge's login-gate/cookie-forwarding/approve-deny branching (Task 6), and the bearer-token gate's 401/`WWW-Authenticate`/success paths (Task 7).
- **Not unit-testable in this repo, honestly named rather than skipped:** whether Authentik's own hosted login UX renders correctly mid-flow (outside this repo's control entirely); whether a real Claude.ai client's own DCR/PKCE/redirect implementation actually round-trips correctly against this adapter (Task 10, Steps 1-3); whether Redis-backed persistence survives a real pod restart in a real cluster, not just an `ioredis-mock` instance in a test process (Task 10, Step 4); `helm template` rendering correctly is checked (Task 8, Step 8) but a real `helm install`/`kubectl apply` end-to-end deploy is not exercised by any task here.
- **Deliberately not tested because deliberately not built by this plan:** anything downstream of the bearer token actually being *used* for a session-aware DS call (`annotateLeg.ts`'s TRUST tier) — that code lives in the sibling plan, not this one (Status note, Task 7's coordination note).

## Not in this plan

Per the dual-mode design's own explicit scoping and this task's own brief — carried forward here, not silently dropped:

- **Option B in its entirety**: the DS-hosted chat orchestrator service (a new, separate TypeScript service holding DS's own Anthropic API key), `frontend/app/chat/` and its "track this leg" deep-link into `TrackTrainForm`, the SSE streaming transport and its own new proxy route, and the `chatbot_allowed_users` cost/access-gating allowlist. Per the dual-mode design's own Sequencing recommendation, this is explicitly scoped as *"a separate, larger follow-on — not a parallel equal track"* to this plan, and per that document's own breakdown table, none of these rows are shared with Option C — they depend on this plan's foundation but add substantial B-only surface this plan does not touch.
- **Option B's own non-interactive grant path** (Decision 1 of the dual-mode design: the orchestrator's own confidential OAuth client registration and its own server-side, non-interactive token-exchange-or-equivalent grant against this same adapter) — genuinely unresolved by this plan, since this plan only implements the *interactive* authorization-code+PKCE grant Option C (and a real external MCP client generally) needs. A future Option B plan will need to design and add a second grant type to Task 5's `/token` endpoint.
- **`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s own Task 2/6/7 revision** — making `DsApiClient` session-aware for the TRUST-corroboration tier, and reconciling that plan's own chart edits with this plan's Task 8. Flagged explicitly in this plan's Status note as a real, separate follow-up this plan does not execute.
- **A refresh-token grant.** Access tokens are long-lived instead (Global Constraints); flagged in Open questions/risks below, not designed.
- **Fixing or extending Authentik itself** (DCR support, consent-history self-service revocation) — this plan's entire design deliberately routes around needing anything from Authentik beyond what `api.sso.clientId`'s existing, unmodified relying-party registration already provides (Global Constraints). The dual-mode design's own Open Question 2 (whether Authentik's consent view supports self-service revocation) is unaffected by, and unresolved by, this plan.
- **The NRE/Network-Rail-branding attribution question for MCP tool-rendered output.** Genuinely unresolved by any document in this session's own chain; Task 10, Step 5 treats it as a hard pre-production gate, consistent with the train-mcp design's own Licensing note, but does not resolve it.
- **`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`'s Tasks 3, 4, 5, 9** (the `resolve_station` shim, the leg→line matching algorithm, the line-catalogue cache, the board-tools DS-migration proposal) — entirely orthogonal to auth, unaffected by and not touched by this plan.
- **A rich, DS-authored connector-management UI** (viewing/revoking a connected Claude app from inside DS's own frontend) — per the dual-mode design's own Decision 6, deliberately not built; Authentik's own consent view (unaffected by this plan) is the only place that state is visible.
- **Rate-limiting `distant-signal-mcp`'s now-publicly-reachable endpoint** beyond whatever ingress-controller-level annotation an operator applies — the train-mcp design's own Decision 6d already flags this as a real, unresolved gap; this plan does not add rate-limiting logic anywhere.

## Open questions / risks

1. **The 90-day access-token TTL and the absence of a refresh grant (Global Constraints) are both unresearched starting figures**, matching this repo's own established posture for unmeasured constants elsewhere (e.g. the sibling plan's own 15-minute line-catalogue cache TTL). A production deployment may want a shorter TTL plus a real refresh flow; not designed here.
2. **The consent screen (Task 6) shows only the DCR-registered `client_name`, which is entirely self-reported by the connecting MCP client at registration time — nothing in this plan verifies it.** A malicious or careless MCP client could register with a misleading name. This is a real, known limitation of open DCR generally (not specific to this implementation), inherent to the "no prior relationship" model the train-mcp design's own 4b explicitly chose the adapter shape to support; not mitigated further here.
3. **`frontend/app/connect-claude/authorize/route.ts` (Task 6) is this plan's one genuinely new trust boundary inside `frontend/`**: it's the first place this app's frontend server-side code reads a raw session cookie value and forwards it, over the network, to a *different* backend process than `api` (namely `distant-signal-mcp`). The internal-token gate (mirroring `X-Internal-Token`) protects this from an external caller, but a compromise of the `railmcp` pod itself would be able to harvest every DS session it's handed this way, for as long as that session's underlying Access Token (Task 5) remains valid — a materially larger blast radius than today's `X-Internal-Token`-protected routes, which never handle a *user's own* session credential at all. Flagged plainly, not mitigated by a new mechanism in this plan.
4. **Whether `distant-signal-mcp`'s underlying container image needs additional resource/scaling headroom once it's a public-facing OAuth AS (not just a tool server behind DS's own network)** is not assessed here — `railMcp.resources: {}` (unset) is carried forward unchanged from the sibling plan's own Task 7.
5. **Task 8's `frontend-deployment.yaml`'s exact existing conditional-env-var convention for an opt-in component was not independently re-verified against a concrete existing example this session** (the step notes to model it on an existing pattern "if such a guard exists" — flagged honestly as unconfirmed, not assumed to exist, since `railMcp` is the first component whose config needs to reach the frontend Deployment at all).
