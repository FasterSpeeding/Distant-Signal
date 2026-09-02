# Embedded Chatbot Option B: Client-Side Anthropic Keys — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md`
(hereafter "the dual-mode design") and
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`:
decisions with real alternatives weighed and rejected, file:line citations
against real code (not the plan that described it), and an explicit "out of
scope" ledger. No implementation plan is included — a separate, later step,
per this repo's process.

## Corrections / relationship to prior specs

**This document SUPERSEDES specific decisions in two prior documents,
following this repo's established "Corrections" precedent** (most recently
`docs/superpowers/specs/2026-09-01-line-status-sample-coverage-design.md`'s
own "Corrections / relationship to prior specs" section). It does not
reopen everything those documents settled — only the pieces the repo
owner's own explicit, out-of-band architecture decision (stated in this
task's own brief, not written down anywhere before this document) directly
contradicts. Every other decision either document made stands, unless
named below.

**The reversal, stated once, precisely:** the dual-mode design and the
`docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md` plan (now
**fully implemented**, per this document's own Current relevant state
below — 7 tasks, unmerged, on `worktree-agent-a6fc94940b8aa651c` in this
repo and `feat/embedded-chatbot-option-b` in `distant-signal-mcp`) both
describe Option B as **DS-hosted and DS-funded**: a server-side
orchestrator holds Distant Signal's own `ANTHROPIC_API_KEY` and spends it
on every allowlisted user's behalf. **The repo owner has now decided,
after weighing and rejecting two other shapes in conversation (recorded
here for the first time, not previously written down), that Option B moves
to per-user, client-side-only Anthropic authentication**: each user
supplies their own Anthropic API key, held only in their own browser, used
only for calls that browser makes directly to Anthropic's API. DS never
receives, stores, or transits a user's key, not even per-request.

**What is explicitly overturned, decision by decision:**

- **Dual-mode design Decision 2** ("Option B's orchestrator: a separate,
  still-internal-only TypeScript service... holds DS's own Anthropic API
  key... `ClusterIP`-only... its only inbound traffic is same-origin,
  cookie-forwarded requests"). **Overturned.** There is no longer a DS-held
  Anthropic key for this orchestrator to protect, which was that decision's
  entire stated reason to exist as a separate, network-isolated service
  (*"the one process in this deployment that now has to weather arbitrary
  public internet traffic"* — that process, `distant-signal-mcp`, was never
  going to hold the Anthropic key either way; the orchestrator's isolation
  was about keeping DS's key away from `distant-signal-mcp`'s own public
  exposure, a threat model that no longer applies once no DS-owned key
  exists anywhere in this feature). See Decision 3 below for the concrete
  recommendation (remove, not shrink).
- **Dual-mode design Decision 5** ("Option B's cost/access gating: the
  allowlist shape... proportionate to a feature an operator wants to
  soft-launch and budget for"). **Partially overturned.** The *budget*
  half of that reasoning — protecting DS-funded Anthropic spend — no
  longer applies (Decision 4 below); the table itself is cheap to keep as
  a pure feature-flag gate, which this document recommends doing, but for
  a different reason than the one that motivated it originally.
- **The option-b plan's Task 1** (`distant-signal-mcp`'s
  `urn:distant-signal:orchestrator-session` grant,
  `src/oauth/orchestratorGrant.ts`) and **Task 3** (`orchestrator/`, the
  whole service). **Overturned — recommended for removal**, not
  reimplementation. See Decisions 1, 3, and 5 below for why a browser MCP
  client doing its own interactive OAuth (Option C's exact shape) replaces
  both.
- **Not reopened:** the shared per-user-OAuth foundation itself
  (`distant-signal-mcp` as its own OAuth 2.1 authorization server, DCR,
  PKCE, the `/connect-claude/authorize` consent bridge) — dual-mode design
  Decision 1 and the
  `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md`
  plan that built it. That foundation is exactly what this document's own
  Decision 1 leans on, unmodified — this document's central finding is that
  Option B can now **reuse** it as-is (the same shape Option C already
  uses), not that it needs revising.
- **Not reopened:** `distant-signal-mcp`'s tool logic, `crates/api`'s
  unrelated routes, or the NRE/Network-Rail attribution question — all
  carried forward unresolved, exactly as both prior documents left them.

## Goal

Redesign Option B so that (1) every Anthropic API call is made directly
from the user's own browser, authenticated with a key the user supplies
and that never reaches any Distant Signal server, and (2) the browser
authenticates to `distant-signal-mcp` the same way Option C's Claude
Desktop already does — reusing, not rebuilding, the shared OAuth
foundation — then work out, with evidence rather than assumption, what
that leaves of the real, already-implemented `orchestrator/` service, the
`urn:distant-signal:orchestrator-session` grant, and the
`chatbot_allowed_users` gate.

## Current relevant state (verified 2026-09-02, against the real unmerged implementation)

**Both branches are fully implemented, not just planned.** Confirmed this
session: `worktree-agent-a6fc94940b8aa651c` (this repo, 4 commits:
`098e6f6`, `4cae0b8`, `940cc8e`, `c8e0178`, `92c7576` — chatbot allowlist,
orchestrator service, SSE proxy, chat UI, chart+CI) and
`feat/embedded-chatbot-option-b` (`distant-signal-mcp`, on top of
`52b637d` — the shared-foundation OAuth work already merged there — adding
`a42ea8c`/`4ea9261`, `src/oauth/orchestratorGrant.ts`). Read in full this
session via `git show <branch>:<path>`, not the plan documents that
described them — every claim below is against the landed code.

### `distant-signal-mcp` is already a real OAuth 2.1 authorization server — the exact mechanism a browser client needs

- `src/oauth/provider.ts`'s `RailMcpOAuthProvider` implements the MCP
  SDK's `OAuthServerProvider`: `registerClient` (DCR, PKCE-only —
  *"the adapter never issues a `client_secret`, no matter what was
  requested... PKCE is the real [confidentiality boundary]"*),
  `authorize()` (validates the RFC 8707 resource indicator, records a
  pending authorization, redirects to `frontendOrigin` +
  `/connect-claude/authorize`), `exchangeAuthorizationCode`
  (single-use code → bearer token, hashed at rest).
- `src/oauth/router.ts` mounts `/authorize`, `/token`, `/register` using
  the MCP SDK's own vendored handlers
  (`@modelcontextprotocol/sdk/server/auth/handlers/{authorize,token,register}.js`),
  not hand-rolled code.
- `frontend/app/connect-claude/authorize/route.ts` (read via `git show
  main:...` — already on `main`, not just the unmerged branch) is the
  human-facing consent bridge: `GET` renders a same-origin, CSRF-checked
  approve/deny form after routing an unauthenticated visitor through DS's
  existing `/api/auth/login`; `POST` calls
  `distant-signal-mcp`'s `/internal/complete-authorization`
  (`X-Internal-Complete-Token`-gated, `src/oauth/internal.ts`) with the
  raw `distant_signal_session` cookie value, which mints the authorization
  code and redirects back to the MCP client's own `redirect_uri`.
- **This is precisely the flow a browser-based MCP client needs**, and it
  already exists, unmodified, for exactly this purpose — Claude Desktop
  (a native app, not a browser page) is its only consumer today.

### `distant-signal-mcp` has NO CORS on `/mcp` or `/authorize` — confirmed by direct inspection, a real gap

- `src/app.ts`'s `/mcp` handler (`app.all('/mcp', addressLimiter,
  express.json(), auth, userLimiter, ...)`) has zero `cors` import, zero
  `Access-Control-*` header anywhere in the file. Confirmed via `grep -rn
  "cors\|Access-Control" src/` (this session): no hits anywhere in
  `distant-signal-mcp`'s own application code, and `cors` is not a
  dependency in its own `package.json` (`dependencies`:
  `@modelcontextprotocol/sdk`, `express`, `express-rate-limit`, `ioredis`,
  `yauzl`, `zod` — no `cors`).
- `src/oauth/discovery.ts` (`registerDiscovery`) and `src/oauth/router.ts`
  (`registerOauthRouter`) **do** get CORS, but only because the MCP SDK's
  own vendored handlers apply it internally, not because
  `distant-signal-mcp` asked for it: `grep -n "cors" node_modules/@modelcontextprotocol/sdk/dist/esm/server/auth/handlers/*.js`
  shows `metadata.js`, `register.js`, `token.js`, and `revoke.js` each
  `import cors from 'cors'; router.use(cors());` — a permissive,
  any-origin default. `authorize.js` and `server/streamableHttp.js` (the
  code backing `/mcp` itself) have **no** `cors` import at all — confirmed
  by the same grep returning zero hits in either file.
- **Net effect**: DCR (`/register`) and the token exchange (`/token`) are
  already fetchable cross-origin from any browser page (the SDK's default
  `cors()`, unscoped). The actual tool-calling traffic (`/mcp`) is not — a
  browser page on DS's own frontend origin issuing `fetch()` calls to
  `/mcp` today would be blocked by the browser's own CORS enforcement,
  independent of whether the bearer token itself is valid. `/authorize`
  needs no CORS (it is reached by a top-level navigation/redirect, which
  is not subject to CORS at all — only `fetch`/`XHR` are).

### `@modelcontextprotocol/sdk`'s client pieces are genuinely browser-bundlable — verified by inspecting the installed package, not assumed

- `node_modules/@modelcontextprotocol/sdk` (installed in
  `distant-signal-mcp`, v1.29.0) exposes `./client` as a real subpath
  export (`dist/esm/client/index.js`). `grep -rln "from 'node:" dist/esm/client/
  dist/esm/shared/` returns **only** `client/stdio.js` (the child-process
  transport, irrelevant here) — `client/index.js`,
  `client/streamableHttp.js`, and `client/auth.js` import nothing from
  `node:*`. `client/index.js` itself does not import `stdio.js` (confirmed
  by grep), so importing just `@modelcontextprotocol/sdk/client` +
  `@modelcontextprotocol/sdk/client/streamableHttp` does not transitively
  pull in Node's `child_process` machinery.
- `client/streamableHttp.js` is built on plain `fetch` and
  `eventsource-parser/stream` (a WHATWG-streams library) — no Node-only
  HTTP client.
- **`client/auth.js` is a complete, already-written browser-shaped OAuth
  client**: `export async function auth(provider, options)`,
  `discoverOAuthProtectedResourceMetadata`, `discoverOAuthMetadata`,
  `startAuthorization`, `exchangeAuthorization`, `registerClient`
  (DCR), all driven by an `OAuthClientProvider` interface
  (`client/auth.d.ts`) whose own doc comment frames it explicitly for a
  redirect-based flow: `redirectUrl`, `clientInformation()`/
  `saveClientInformation()`, `tokens()`/`saveTokens()`, and (per the
  interface's own doc) a method to *"redirect the user agent to the given
  URL to begin the authorization flow."* This is not a Node-server-shaped
  API repurposed for the browser — it is written for exactly this shape of
  client already. `StreamableHTTPClientTransport`'s constructor takes an
  `authProvider` option and calls this `auth()` helper itself, including
  on a `401`/`403` mid-connection (`streamableHttp.js:96,314,336`-ish, per
  this session's grep of `this._authProvider`), which is the same
  automatic-reauth behavior any MCP client (Claude Desktop included) gets
  for free.
- Neither `frontend/package.json` nor any `next.config.*` exists in this
  repo naming or configuring `@anthropic-ai/sdk` or
  `@modelcontextprotocol/sdk` today (confirmed: `grep -i
  "anthropic\|mcp"` against `frontend/package.json` returns nothing but
  `next` itself) — bundling either into `frontend/`'s client build is new
  ground for this codebase, not an extension of an existing pattern.

### The real orchestrator implementation (`orchestrator/`, `worktree-agent-a6fc94940b8aa651c`) — what it does today, all of which becomes unnecessary

Read in full: `orchestrator/src/app.ts`, `chat.ts`, `mcpToken.ts`,
`dsClient.ts`. Its `POST /chat` handler does exactly four things, in
order: (1) `checkChatbotAccess` — forwards the browser's `Cookie` header to
`GET /public/chatbot/access`; (2) `getMcpToken` — exchanges the forwarded
DS session for a `distant-signal-mcp` bearer token via the
`urn:distant-signal:orchestrator-session` grant, cached in an in-process
`Map`; (3) `runChatTurn` (`chat.ts`) — a hand-rolled Anthropic
`toolRunner()` loop, opening its own `StreamableHTTPClientTransport`/
`McpClient` against `distant-signal-mcp`'s `/mcp` with
`Authorization: Bearer <step 2's token>`, using **DS's own**
`ANTHROPIC_API_KEY` (`orchestrator/src/config.ts`, not read in full this
session but referenced by every file above); (4) streams `text-delta`/
`tool-result`/`done` SSE events back through
`frontend/app/api/chat/route.ts` to `frontend/app/chat/`
(`ChatPanel`, referenced by `page.tsx`).

### The real `urn:distant-signal:orchestrator-session` grant (`distant-signal-mcp`, `src/oauth/orchestratorGrant.ts`)

Mounted at the literal `/token` path **before** the MCP SDK's own
`tokenHandler` (`src/app.ts`: `app.use('/token', registerOrchestratorGrant(...))`
ahead of `app.use(registerOauthRouter(oauthProvider))`), falling through
via `next()` for any other `grant_type` — the SDK's own token handler has
a closed `switch (grant_type)` that cannot be extended in place (confirmed
by this file's own doc comment, which reads the vendored SDK source
directly: *"Additional auth methods will not be added on the server side
of the SDK"*). Authenticated by a **separate** shared secret
(`X-Orchestrator-Internal-Token`, `config.oauth.orchestratorInternalToken`)
from the consent bridge's `X-Internal-Complete-Token`, deliberately, per
its own doc comment: *"can mint a fresh, unattended access token for
**any** `ds_session_cookie_value` it's handed, on demand... a materially
broader capability."* Its own doc comment already names the exact
provisioning gap this document's Decision 5 discusses below: DS-added
`chatbot_allowed_users` membership and Authentik's `mcp-users`/
`mcp-live-boards` groups are *"two entirely separate provisioning systems
with no automatic link between them."*

### `chatbot_allowed_users` / `GET /public/chatbot/access` (`crates/api`, `worktree-agent-a6fc94940b8aa651c`)

`crates/api/migrations/20260902110000_chatbot_allowed_users.sql`:
`CREATE TABLE chatbot_allowed_users (user_id TEXT PRIMARY KEY REFERENCES
users(id) ON DELETE CASCADE)` — bare membership, no metadata.
`crates/api/src/routes/chatbot.rs`: `ChatbotAuthorizedUser` extractor
wraps `AuthenticatedUser`; `GET /public/chatbot/access` returns `200
{"allowed": true}` / `403 {"error": "chatbot_not_available"}` / `401`
(three states, 6 tests total across `db_tests`, `#[ignore]`d pending a
live database). Two real callers today: `frontend/app/chat/page.tsx`
(page-load gate) and `orchestrator/src/dsClient.ts`'s
`checkChatbotAccess` (the actual cost-protecting check, since a request
can reach the orchestrator without rendering the page first).

### Test coverage that exists today for the pieces this document proposes removing

`orchestrator/test/{chat,dsClient,mcpToken}.test.ts`: 6 + 7 + 5 = **18
tests** (grep-counted this session, matching this task's own framing
exactly). `distant-signal-mcp`'s `test/oauth-orchestrator-grant.test.ts`:
12 tests. `frontend/app/api/chat/route.test.ts`: 6 tests.
`frontend/app/chat/page.test.tsx`: 3 tests. All of this is real,
already-written, already-passing test coverage for code this document
recommends deleting, not hypothetical work being avoided — a genuine cost
of the recommendation below, named plainly in Testing.

### Existing browser-storage precedent in `frontend/`

`git grep -l "localStorage\|sessionStorage" -- frontend` finds
`ThemeToggle.tsx` and `PrideToggle.tsx` — both per-viewer, non-sensitive
UI preferences, read/written directly from a Client Component, no
existing precedent in this app for storing anything credential-shaped
client-side. `ThemeToggle.tsx`'s own doc comment notes Mantine's
`colorScheme` reads `localStorage` **synchronously, even on the client's
first pre-hydration render** — the closest existing precedent for
"a value read before React fully mounts," relevant to Decision 6 below
only as prior art for the mechanism, not for the sensitivity class of what
gets stored.

## Decisions

### 1. The tool-calling loop and the MCP client both move into the browser — confirmed technically viable, not assumed

Two real shapes were weighed for where the Anthropic-calling loop and the
MCP client run, now that no DS-funded key exists to protect server-side:

- **Keep a server-side loop, just remove the key from it** (e.g. the
  browser sends its key per-request to a thin DS-hosted relay that
  forwards it to Anthropic). **Rejected outright** — this is exactly the
  shape the repo owner's own decision (Corrections, above) rules out: *"the
  key must NEVER be sent to or persisted by any DS server, not even
  transiently/per-request."* A relay that only forwards and never persists
  still receives the key in cleartext on every request, which is the exact
  property being avoided.
- **The browser itself becomes the orchestrator: it holds the Anthropic
  key, calls Anthropic's Messages API directly, and is its own MCP
  client against `distant-signal-mcp`'s `/mcp`.** **Chosen.** This
  session's own inspection of the installed `@modelcontextprotocol/sdk`
  package (Current relevant state, above) found the client-side pieces
  genuinely browser-bundlable: `client/index.js` and
  `client/streamableHttp.js` import no `node:` builtins and do not
  transitively pull in the Node-only `stdio.js` transport, and
  `client/auth.js` is an already-written, redirect-based OAuth client
  built for exactly this shape of caller (an `OAuthClientProvider`
  interface with `redirectUrl`/`tokens()`/`saveTokens()`, consumed
  automatically by `StreamableHTTPClientTransport`'s own `authProvider`
  option, including automatic re-auth on a `401`). Anthropic's own SDK
  supports the equivalent for the Messages API side via
  `dangerouslyAllowBrowser: true` (setting the
  `anthropic-dangerous-direct-browser-access` header) — a real,
  documented, still-current feature (Anthropic added it in August 2024
  specifically to support "bring your own key" client-side apps; see
  References). Both halves of "browser calls both Anthropic and
  `distant-signal-mcp` directly" are real, not aspirational.

**What this replaces, concretely:** `orchestrator/src/chat.ts`'s
`runChatTurn` (the `toolRunner()` loop + `McpClient`/
`StreamableHTTPClientTransport` pairing) moves, close to verbatim in
shape, into a Client Component in `frontend/`. The MCP tool discovery
(`listTools()`), `callTool()`, and `structuredContent` extraction for the
"track this leg" deep-link (dual-mode design Decision 3, unchanged) all
run the same way, just in the browser instead of in `orchestrator/`'s
Node process — the actual `chat.ts` logic is not being redesigned, only
relocated.

### 2. CORS: add explicit, origin-scoped CORS to `distant-signal-mcp`'s `/mcp`; Anthropic's own CORS support is already sufficient, unmodified

**`distant-signal-mcp` needs a real code change** — a gap, not a
misunderstanding. `/mcp` (`src/app.ts`) has no `cors` middleware today
(Current relevant state, confirmed by direct inspection of both the
application code and the vendored SDK handlers it composes). A browser
`fetch()` from DS's own frontend origin to `/mcp` would fail the
browser's CORS preflight/response check today, regardless of whether the
bearer token is valid. **Recommended: add `cors({ origin:
config.frontendOrigin.origin, ... })`, scoped to the single known
first-party origin `distant-signal-mcp` already threads through
(`config.frontendOrigin`, `src/config.ts`'s existing `DS_FRONTEND_ORIGIN`),
not a wildcard** — more conservative than the SDK's own default
`cors()` (any-origin) already applied to `/register`/`/token`/`/revoke`/
discovery, and consistent with those endpoints needing to stay open to
arbitrary external MCP clients (Claude.ai's own connector UI, potentially
browser-hosted, is exactly why the SDK defaults them open) while `/mcp`
itself only ever needs to answer DS's own frontend and non-browser
clients (Claude Desktop, which is not subject to CORS at all). `/authorize`
needs no change — it is reached by a top-level navigation, never a
`fetch()`, so CORS does not apply to it regardless of caller.

**Anthropic's own API needs no DS-side change at all.** Confirmed via web
search this session (not assumed): Anthropic added first-class CORS
support for direct browser calls in August 2024, specifically for the
"bring your own key" pattern this design uses — setting
`dangerouslyAllowBrowser: true` in the TypeScript SDK sends
`anthropic-dangerous-direct-browser-access: true`, and Anthropic's API
responds with the CORS headers needed for a third-party browser page to
read the response. This is a stable, intentional feature, not a loophole —
Anthropic's own SDK maintainers added the flag precisely to stop
developers routing around the restriction with a hand-rolled proxy (which
is what this document's Decision 1 already rejected as a shape that would
have re-introduced a DS-held-key-adjacent problem). One real caveat worth
naming plainly: this CORS opening is not scoped to DS's origin by
Anthropic — once the header is set, the browser can reach
`api.anthropic.com` from any page, meaning the only thing standing between
a user's key and misuse is standard browser-storage/XSS hygiene on DS's
own frontend, not anything Anthropic enforces per-caller. This is inherent
to the BYO-key browser pattern generally, not something this document's
own choices make worse or could mitigate away.

### 3. `orchestrator/` — recommend full removal, not a shrink

Three things `orchestrator/` did (Current relevant state): the allowlist
check, the MCP token exchange, and the Anthropic tool-calling loop with
SSE re-emission. Weighed two shapes for what's left of it:

- **Shrink it to just the MCP token exchange** (i.e. keep a thin
  server-side endpoint that trades a forwarded DS session for a
  `distant-signal-mcp` bearer token, matching Option C's own framing that
  a DS-hosted, already-session-authenticated caller is *"a natural,
  already-privileged position to mint... a correctly-scoped access token
  for that same user, rather than calling anonymously"* — the dual-mode
  design's own Decision 1 language). **Rejected.** That framing was
  written to justify **skipping the human consent screen** for a trusted
  first-party server — the actual reason the non-interactive
  `orchestrator-session` grant exists at all (`orchestratorGrant.ts`'s own
  doc comment: *"The only thing this grant is entitled to skip is the
  human interactive consent screen... orchestrator/'s own allowlist check...
  already gates who may reach this endpoint at all before it's ever
  called, which is what makes skipping consent reasonable."*). A browser
  page is not a trusted first-party server in that sense — it is the same
  category of caller Claude Desktop already is (an external, interactive
  MCP client acting on behalf of the human sitting in front of it), and
  Option C already proves the interactive consent flow works fine for
  exactly that category, in-band, with no server-side component at all.
  Keeping a shrunk token-exchange endpoint would mean maintaining two
  parallel ways to get a `distant-signal-mcp` token for what is now
  structurally the same kind of caller, for no remaining benefit.
- **Remove `orchestrator/` entirely; the browser does its own interactive
  OAuth against `distant-signal-mcp`, identical in shape to Option C.**
  **Chosen.** Once the browser is its own MCP client (Decision 1) with no
  DS-funded key to protect (this document's whole premise) and no
  consent-skipping justification left (above), there is nothing left for
  a DS-hosted process to do that `distant-signal-mcp`'s existing
  `/register` → `/authorize` → consent bridge → `/token` flow (already
  built, already serving Option C, Current relevant state) doesn't already
  do for free. The browser: (a) performs DCR against `/register` once
  (cached client_id, no secret — same PKCE-only public-client shape
  Claude Desktop gets); (b) redirects the top-level page to `/authorize`
  with a `redirect_uri` pointing at a **new, small** `frontend/app/chat/callback`-shaped
  route (genuinely new work, but a static callback page, not a service —
  see Architecture); (c) the existing `/connect-claude/authorize` consent
  bridge handles login/consent exactly as it does for Claude Desktop today
  — in fact simpler here, since the user is very likely already
  mid-session on DS's own frontend, not switching apps; (d) the callback
  page exchanges the code at `/token` (already CORS-open via the SDK's own
  default, Decision 2) and stores the resulting bearer token itself
  (Decision 6). **No new server-side code in `orchestrator/`'s place is
  needed at all** — this is the single most consequential finding in this
  document, and it is evidence-based (Current relevant state's direct
  reading of `client/auth.js` and the consent bridge), not a preference.

**Recommendation, stated plainly for the report:** remove `orchestrator/`
entirely — the deployment (Helm chart block, CI matrix entry,
`docker-compose` profile), the service directory, and its 18 tests, all of
which become dead code once the browser is the client. This is a genuine
loss of already-working, already-tested infrastructure (named honestly in
Testing), not a free change.

### 4. `chatbot_allowed_users` — repurpose as a pure feature-flag gate, not remove

Two shapes weighed:

- **Remove entirely, relying solely on `distant-signal-mcp`'s own
  `mcp-users`/`mcp-live-boards` Authentik access groups** (the
  `mcp-server-oauth-access-groups` design's gates, confirmed landed on
  `distant-signal-mcp`'s `master` this session — `src/oauth/accessGroups.ts`,
  merged `46f955e`, per `orchestratorGrant.ts`'s own updated doc comment).
  These already answer "who may use the tools at all," independent of
  billing (per this document's own premise, billing is no longer DS's
  concern either). **Rejected as the sole gate**, for a real reason: an
  operator soft-launching an embedded chat *UI* inside DS's own frontend
  is a separate, DS-product-level question from "who may call
  `distant-signal-mcp`'s tools" — the latter also gates Option C's
  arbitrary Claude.ai users, a materially different population than "who
  should see a beta feature link inside `distant-signal-mcp`'s
  sibling product." Removing `chatbot_allowed_users` would mean the only
  way to soft-launch `/chat` is either "everyone with a DS account" or
  "everyone in `mcp-users`" (a group provisioned for a different purpose,
  by a different team's process, in Authentik) — neither is a clean
  proxy for "should see the embedded chat page."
- **Keep the table, extractor, and route, but re-frame what it means: a
  feature-flag/beta-access gate for the `/chat` page itself, not a
  spend-protection mechanism.** **Chosen.** The code
  (`crates/api/src/routes/chatbot.rs`, the migration, the three-state
  response shape) is already built, already tested, and costs nothing
  extra to keep — the only real change is dropping the "protects DS-billed
  Anthropic spend" framing from its own doc comments (Current relevant
  state's citation of the migration's own header comment, which should be
  corrected when this lands) since that's no longer true. `orchestrator/`'s
  own consumption of it (`dsClient.ts`'s `checkChatbotAccess`, "the actual
  cost-protecting check") disappears along with `orchestrator/` itself
  (Decision 3) — `frontend/app/chat/page.tsx`'s own page-load gate is the
  **only** remaining caller, which is exactly right for a UI-visibility
  feature flag and exactly wrong for a spend-protection mechanism (a
  page-load-only gate was never sufficient to stop spend on its own, since
  nothing stopped a request that skipped the page — this document's own
  Decision 1 removes the very API surface, `orchestrator/`'s `/chat`
  endpoint, that gap could have been exploited against, so the gap closes
  as a side effect rather than needing to be separately fixed here).

### 5. The `urn:distant-signal:orchestrator-session` grant — recommend removal

Directly downstream of Decision 3: this grant exists for exactly one
caller (`orchestrator/`), for exactly one purpose (skip consent for a
trusted first-party server, `orchestratorGrant.ts`'s own doc comment,
quoted in Decision 3). Once that caller is removed and the browser uses
the standard `authorization_code` + PKCE grant instead (Decision 1),
nothing calls this grant type. **Recommended: remove
`src/oauth/orchestratorGrant.ts`, its mount point in `src/app.ts`
(`app.use('/token', registerOrchestratorGrant(...))`), its config
(`config.oauth.orchestratorInternalToken`), and its 12 tests
(`test/oauth-orchestrator-grant.test.ts`)** — real, working code and
tests, a genuine deletion, not a no-op. This also resolves, by removal
rather than by design, the exact operational gap that grant's own doc
comment already named unresolved: the `chatbot_allowed_users` /
`mcp-users`/`mcp-live-boards` provisioning-mismatch risk it flagged
(Current relevant state) was a property of a caller that skipped consent
based on one allowlist while a second, unrelated allowlist gated the
tools underneath it — with no non-interactive grant left, every caller
(browser included) goes through the same interactive consent +
access-group check Option C's users already go through, so there is only
one allowlist-shaped thing to keep in sync (`mcp-users`/
`mcp-live-boards`), not two.

### 6. Key storage and UX: `localStorage`, a settings affordance inside `/chat`, explicit disclosure, clear error surfacing

**Storage location — three shapes weighed:**

- **`sessionStorage`.** Rejected as the default: it would force a user to
  re-enter their Anthropic key every browser session (tab close, browser
  restart) purely for a UX-cost reason, with no realistic security benefit
  over `localStorage` against this app's actual threat model (both are
  equally readable by any JS running on the origin — the real boundary is
  XSS on `distantsignal.app`, not tab lifetime).
- **IndexedDB.** Rejected as unnecessary complexity: nothing about this
  key needs IndexedDB's structured-storage or larger-quota properties (a
  single string, well under any `localStorage` quota) — it would be a
  heavier API for identical actual security properties to `localStorage`.
- **`localStorage`, matching this app's own existing precedent
  (`ThemeToggle.tsx`/`PrideToggle.tsx`, Current relevant state) for a
  per-viewer, browser-local value.** **Chosen.** Persists across sessions
  (a user shouldn't have to re-paste their key every visit), consistent
  with the only existing browser-storage pattern this codebase already
  has, and the correct default for the stated privacy goal: the key
  should live in exactly one place the user controls (their own browser
  profile), for as long as they want it there.

**UX, concretely:**
- A settings affordance inside `/chat` itself (not a new top-level route
  — this is a small, single-field control, not a page-sized concern the
  way `/connect-claude` warranted its own route in the dual-mode design)
  lets the user enter, view-masked, replace, or clear their key at any
  time.
- **A one-time, explicit disclosure is a real trust requirement, not
  optional polish**, given the security posture being promised: before or
  at first key entry, the UI must state plainly that the key is stored
  only in the browser, is sent only to Anthropic directly, and is never
  seen by any Distant Signal server — the exact claim this whole
  redesign exists to make true, which only has value if the user is told
  it's true.
- **Invalid/expired/revoked key**: Anthropic's Messages API returns a
  `401` for an invalid/revoked key. The chat UI must surface this as a
  clear, distinct error state (*"Your Anthropic API key was rejected —
  check it in Settings"*), not a silent failure or a generic "chat
  failed" message indistinguishable from a tool error or an MCP-token
  problem — three genuinely different failure classes (bad Anthropic key,
  expired/revoked `distant-signal-mcp` token, a tool-call error) that a
  user needs to be able to tell apart to know what to fix.

### 7. Testing: honestly narrower than today's 18 orchestrator tests, with a concrete floor

**What's genuinely still unit-testable**, extracted as pure functions the
same way `orchestrator/`'s own tests already did for logic that happened
to run server-side: request/response shaping for the Anthropic call,
SSE-equivalent event parsing/formatting for rendering
`text-delta`/`tool-result` events in the UI, the `structuredContent` →
"track this leg" deep-link mapping (dual-mode design Decision 3, unchanged
regardless of where the loop runs), the `OAuthClientProvider`
implementation's own `tokens()`/`saveTokens()`/`clientInformation()`
methods (pure `localStorage` read/write, easily unit-tested with a mocked
`Storage`), and the callback page's code-exchange logic (a pure function
given a `URLSearchParams` and a mocked `fetch`).

**What's genuinely harder to test than the current server-side loop, named
honestly rather than glossed over:** the actual Anthropic
`toolRunner()`/streaming loop and the actual `StreamableHTTPClientTransport`
↔ `distant-signal-mcp` MCP handshake, now running in a browser context
against two real third-party network boundaries (Anthropic's API,
`distant-signal-mcp`'s `/mcp`) instead of one Node process calling out to
both — there is no server-side integration point left to write a
Vitest/Supertest test against the way `orchestrator/test/chat.test.ts`
did. This repo already has real Playwright e2e infrastructure
(`frontend/playwright.config.ts`, `frontend/e2e/service-worker.spec.ts`,
confirmed this session), which is the correct tool for this gap: a
Playwright spec driving `/chat` with the real page but **mocked** network
responses (Playwright's own request interception) for both
`api.anthropic.com` and `distant-signal-mcp`'s `/mcp`/`/token` endpoints —
verifying the UI correctly renders a scripted SSE-shaped sequence of
Anthropic streaming events and correctly surfaces each of Decision 6's
three distinct error classes. This is real, buildable coverage, but it is
still not the same as `orchestrator/test/mcpToken.test.ts`'s exact
assertion today (*"a second call within the cache window does not re-hit
`/token`"*) — there is no cache to test once every browser tab manages its
own token independently, which is a genuine behavior change, not just a
relocated test.

**Net honesty for the report**: this document's recommendation trades 18
real, already-passing `orchestrator/` tests plus 12 real
`orchestratorGrant` tests (30 total, Current relevant state) for a smaller
set of pure-function unit tests plus new Playwright coverage that did not
exist before — a real, acknowledged reduction in what's mechanically
verifiable, in exchange for removing an entire service and its
server-side attack surface. Whether that trade is acceptable is a product
call this document surfaces, not one it makes on the repo owner's behalf.

## Architecture

```
Browser (frontend/app/chat/ + a NEW small callback route)
  │
  ├─ 1. First use: DCR against distant-signal-mcp's /register (already
  │     CORS-open, SDK default), caches client_id (no secret) in
  │     localStorage -- same PKCE-only public-client shape Claude Desktop
  │     already gets from RailMcpOAuthProvider.clientsStore.
  │
  ├─ 2. Interactive OAuth: top-level redirect to distant-signal-mcp's
  │     /authorize -> (no CORS needed, top-level navigation) ->
  │     frontend/app/connect-claude/authorize (EXISTING, unmodified --
  │     same consent bridge Option C already uses) -> user logs in via DS's
  │     existing session if needed, approves -> redirected back to a NEW,
  │     small frontend/app/chat/callback route with ?code=...
  │
  ├─ 3. Callback route exchanges the code at distant-signal-mcp's /token
  │     (already CORS-open, SDK default) -> stores the resulting bearer
  │     token in localStorage (Decision 6) -> redirects back to /chat.
  │
  ├─ 4. User enters their OWN Anthropic API key once (Decision 6),
  │     stored in localStorage, NEVER sent to any DS server.
  │
  └─ 5. Chat turn: browser runs its own toolRunner()-shaped loop
        (relocated from orchestrator/src/chat.ts, Decision 1) --
              │                                    │
              │ Authorization: Bearer              │ x-api-key / OAuth
              │ <step 3's token>                    │ header, dangerouslyAllowBrowser:
              │ (NEW CORS on /mcp,                  │ true (Decision 2 --
              │  Decision 2, scoped to              │  Anthropic's own,
              │  config.frontendOrigin)              │  no DS change needed)
              ▼                                     ▼
    distant-signal-mcp /mcp                 api.anthropic.com/v1/messages
    (EXISTING, unmodified tool logic --     (direct browser call, the
     resolve_station/get_departures/...)     user's own key/spend)

REMOVED entirely (Decisions 3, 5):
  orchestrator/                    -- whole service, Helm block, CI matrix
                                       entry, docker-compose profile, 18 tests
  urn:distant-signal:orchestrator-session grant, src/oauth/orchestratorGrant.ts,
                                       12 tests

KEPT, RE-FRAMED (Decision 4):
  crates/api chatbot_allowed_users / GET /public/chatbot/access --
    feature-flag gate for /chat's own visibility, no longer "spend
    protection" in its own doc comments; only remaining caller is
    frontend/app/chat/page.tsx's page-load gate.

UNCHANGED:
  distant-signal-mcp's own OAuth server (provider.ts, router.ts,
    discovery.ts), the /connect-claude/authorize consent bridge, DCR,
    mcp-users/mcp-live-boards access groups, all six MCP tools.
```

## Error handling

- **Anthropic key rejected (`401` from `api.anthropic.com`)**: surfaced as
  its own distinct UI state pointing the user at the key-entry control
  (Decision 6) — never conflated with an MCP/tool error.
- **`distant-signal-mcp` bearer token expired/revoked (`401`/`403` from
  `/mcp`)**: the browser's own `OAuthClientProvider`, driven by
  `StreamableHTTPClientTransport`'s existing automatic-reauth-on-401
  behavior (Current relevant state's citation of `client/auth.js`'s
  `auth()` calls inside `streamableHttp.js`), attempts a silent
  re-authorization first (a fresh top-level redirect through `/authorize`,
  since no refresh-token grant exists on this authorization server today
  — the interactive grant's own `ACCESS_TOKEN_TTL_SECONDS`, 90 days, means
  this is rare, not a per-message occurrence). If that also fails (e.g.
  the user revoked consent), the UI surfaces a distinct "reconnect to
  Distant Signal's rail data" state, separate from the Anthropic-key error
  above.
- **`distant-signal-mcp` tool-level errors** (a DS API 5xx during
  `plan_journey`, an ambiguous line match, etc.): unchanged from the
  dual-mode design's own Error handling section — still
  `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
  territory, reused as-is regardless of which process is the MCP client.
- **`chatbot_allowed_users` rejection**: unchanged in shape from the
  dual-mode design's own posture — a plain "not available for your
  account" state on `/chat`, not a `404` (the feature's existence is not a
  secret) — only its underlying meaning changes (Decision 4), not its
  presentation.
- **CORS misconfiguration itself** (e.g. `config.frontendOrigin` drifting
  from the actual deployed frontend origin): fails as a browser-visible,
  unambiguous network error at the `fetch()` call site (a blocked CORS
  preflight is not silent) — an operator-configuration bug, not a
  user-facing error state this document designs UI copy for.

## Testing

Covered in full under Decision 7 above (kept alongside the other decisions
rather than duplicated here, since the honest gap-vs-today's-coverage
framing is itself the substance of that decision, not a separate
afterthought).

## Explicitly out of scope

- **The exact `frontend/app/chat/callback` route implementation**
  (component tree, exact `localStorage` key names, exact
  `OAuthClientProvider` class shape) — Decision 1/3 establish that this is
  a small, new, static page reusing SDK-exported functions; the concrete
  code is implementation-time work, not designed here.
- **A concrete settings-affordance UI** for entering/clearing the
  Anthropic key (Decision 6) — placement inside `/chat` and its required
  disclosure content are decided; exact component/copy is not.
- **Migrating or backfilling `chatbot_allowed_users` rows** if this ships —
  an operator/provisioning concern, not a schema or code change this
  document proposes.
- **Any change to `distant-signal-mcp`'s tool logic, `mcp-users`/
  `mcp-live-boards` access groups, or DCR/consent-bridge behavior** beyond
  the single, additive CORS change on `/mcp` (Decision 2). Everything else
  about that authorization server is reused verbatim.
- **A refresh-token grant** for the interactive `authorization_code` flow.
  Unaffected by this document; carried forward as the same open gap the
  shared-foundation plan already left (Error handling above references its
  practical consequence — an occasional re-auth redirect — without
  resolving it).
- **Rate-limiting or abuse protection for a user's own Anthropic spend.**
  Structurally out of DS's hands once the key and the calls are the
  user's own — not a gap this document leaves unaddressed so much as one
  that no longer belongs to Distant Signal at all.
- **The NRE/Network-Rail-branding attribution question** for MCP
  tool-rendered output. Still unresolved, unaffected by this document,
  carried forward unchanged from every prior document in this chain.
- **Multi-device/multi-browser key sync.** A user who enters their key on
  one browser must re-enter it on another (`localStorage` is
  per-origin-per-browser-profile) — an inherent property of Decision 6's
  chosen storage, not a gap this document proposes solving (syncing would
  require exactly the server-side custody the repo owner's decision
  explicitly rejected).
- **Removing/renaming the `chatbot_` prefix** on the retained
  `chatbot_allowed_users` table/route despite its re-framed meaning
  (Decision 4) — a cosmetic rename, not designed here; flagged so it isn't
  mistaken for an oversight.

## Open questions / risks

1. **Whether Anthropic's CORS support for `dangerouslyAllowBrowser` could
   change or tighten in the future** (e.g. requiring an allowlisted
   origin, the way this document's own `/mcp` CORS change scopes to a
   single origin) is a third-party product decision outside this repo's
   control. Not mitigated here beyond noting it; a change on Anthropic's
   side would be a hard external dependency risk for this whole design,
   not something DS's own code could work around short of falling back to
   a server-side relay (which Decision 1 already rejected on privacy
   grounds).
2. **Whether the browser's own `OAuthClientProvider` implementation should
   itself use `StreamableHTTPClientTransport`'s built-in `authProvider`
   machinery wholesale, or hand-roll the redirect/exchange/store sequence
   directly against `client/auth.js`'s exported functions** — both are
   viable per Current relevant state's own reading of the SDK; which is
   less code and easier to reason about for a Next.js Client Component's
   own lifecycle (page unmount during a redirect, etc.) is an
   implementation-time call, not resolved here.
3. **Whether removing `chatbot_allowed_users`' "spend protection" framing
   from its own doc comments (Decision 4) should happen as part of
   whatever implementation plan follows this document, or is cosmetic
   enough to defer indefinitely** — not resolved; flagged so the stale
   framing doesn't silently persist and mislead a future reader the way
   this document's own Corrections section found and fixed for the prior
   two documents.
4. **The genuine UX cost of Decision 3's removal of `orchestrator/`**: a
   first-time user must now complete a DCR + interactive-consent + code-
   exchange round trip (even if fast, since they're already on DS's own
   origin) before their **first** message, where the old orchestrator-grant
   design let a first-time allowlisted user chat immediately with zero
   extra clicks (the non-interactive grant's whole point). This is a real,
   named trade-off of this document's own recommendation, not something
   resolved in the user's favor by assumption — whether it's acceptable is
   a product call, same posture as Decision 7's testing trade-off.
5. **Whether Anthropic's Messages API rate limits, when hit by many
   individual users' own keys against the shared `distant-signal-mcp`
   `/mcp` endpoint concurrently, interact with `distant-signal-mcp`'s own
   `userLimiter`/`addressLimiter` (`src/app.ts`, unchanged by this
   document) in any surprising way** — not assessed here; both limiters
   already key on the authenticated DS user/address, which should compose
   fine with per-user Anthropic keys in principle, but this was not
   load-tested or reasoned through in depth this session.

## References

- `docs/superpowers/specs/2026-09-02-embedded-chatbot-dual-mode-design.md` —
  Decisions 1-5 and the architecture diagram this document's own Corrections
  section names as partially superseded.
- `docs/superpowers/plans/2026-09-02-embedded-chatbot-option-b.md` — the
  now-fully-implemented plan this document supersedes the shared-key
  premise of; its own Tasks 1-6 are cited throughout Current relevant
  state against the real landed code, not the plan text.
- `docs/superpowers/plans/2026-09-02-embedded-chatbot-shared-foundation-and-option-c.md` —
  unmodified by this document; its own Tasks 1-8 are the foundation
  Decision 1 reuses as-is.
- This repository, read directly this session (via `git show
  worktree-agent-a6fc94940b8aa651c:<path>`, since this branch's own
  worktree had already been cleaned up by the time this session ran):
  `orchestrator/src/{app,chat,mcpToken,dsClient}.ts`,
  `orchestrator/test/*.test.ts`, `frontend/app/chat/page.tsx`,
  `frontend/app/api/chat/route.ts`, `frontend/app/chat/page.test.tsx`,
  `frontend/app/api/chat/route.test.ts`, `crates/api/src/routes/chatbot.rs`,
  `crates/api/migrations/20260902110000_chatbot_allowed_users.sql`,
  `charts/distant-signal/values.yaml`'s `orchestrator:`/`railMcp:` blocks,
  `frontend/app/connect-claude/authorize/route.ts` (read from `main`),
  `frontend/components/ThemeToggle.tsx`, `frontend/playwright.config.ts`,
  `frontend/e2e/service-worker.spec.ts`.
- `/workspaces/distant-signal-mcp`, read directly this session (via `git
  show feat/embedded-chatbot-option-b:<path>`, without checking that
  branch out, since a separate review agent had it checked out in that
  repo's shared, non-worktree-isolated working directory):
  `src/oauth/{orchestratorGrant,internal,provider,router,discovery}.ts`,
  `src/config.ts`, `src/app.ts`, `test/oauth-orchestrator-grant.test.ts`,
  `package.json`, and the installed
  `node_modules/@modelcontextprotocol/sdk` package (v1.29.0) — its
  `package.json` exports map, and direct `grep`s of
  `dist/esm/client/{index,streamableHttp,auth}.js` and
  `dist/esm/server/auth/handlers/*.js` for `node:` imports and `cors`
  usage respectively.
- Web search, this session: Anthropic's August 2024
  `anthropic-dangerous-direct-browser-access` header /
  `dangerouslyAllowBrowser` SDK flag — confirmed real, current, and
  purpose-built for the "bring your own key" browser pattern this
  document uses (Simon Willison's contemporaneous writeup and the
  `anthropic-sdk-typescript` GitHub issues that motivated it).
