# Embedded Chatbot / MCP Integration — Landscape Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
and `docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`
(the direct structural template for this document — evaluates plausibility,
cites what it could and couldn't confirm, reaches a ranked recommendation
without being an implementation plan). Nothing here is wired up; no code in
this repo or in `/workspaces/distant-signal-mcp` was modified as part of
this pass.

## Problem being researched

This session's earlier work (the two documents above, plus
`docs/superpowers/plans/2026-09-01-train-mcp-integration.md`, all executed)
produced a real, working, already-deployed artifact: `distant-signal-mcp`,
a forked-from-`train-mcp` TypeScript MCP server exposing six UK rail tools
(`resolve_station`, `get_departures`, `get_arrivals`, `get_service_detail`,
`find_services`, and a delay-aware `plan_journey`), sourcing station lookup
and live line-status/incident annotation from Distant Signal's (DS's) own
public REST API. That work assumed the MCP server's *client* is a
third-party AI assistant (Claude Desktop, Claude.ai's MCP connector
feature) running entirely outside DS — DS just hosts the tool server.

This document asks a different question: could DS itself host a
conversational chatbot, embedded in `frontend/`, that lets a user plan a
journey by talking to it rather than filling in `TrackTrainForm`'s fields —
with that chatbot's own LLM given access to `distant-signal-mcp`'s tools?
That requires DS to become the host of an agentic *client*, not just the
provider of a tool *server* — a materially different systems question, not
a small extension of the prior work. Four sub-questions, per the task
brief:

1. What does "embed a chatbot" actually require architecturally, given MCP
   is a protocol between an LLM-hosting client and a tool server?
2. Is "let users link their own Anthropic account" a real, existing
   mechanism, or does it collapse into "user pastes an API key" once
   checked against current Anthropic documentation?
3. Could the enricher's existing local LLM endpoint
   (`crates/enricher/src/config.rs`'s `llm_base_url`) be reused/shared for
   conversational traffic, and is gating it to certain users even a small
   change given this app's current auth model?
4. Where would a chat UI live, and does it need its own rendering, or can
   it reuse what `distant-signal-mcp` already built?

## Method

Single-agent research pass in this session. The `claude-api` skill
(bundled reference for the Claude API, MCP, and Anthropic's OAuth/account
model — see its own "cached: 2026-06-24" model table and "API Drift"
warnings) was loaded first and treated as the primary source of truth for
every factual claim about current Claude API/MCP mechanics, cross-checked
against two live fetches of Anthropic's own documentation
(`platform.claude.com/docs/en/agents-and-tools/mcp-connector`, fetched in
full this session) and two web searches for the account-linking question,
where the skill's own bundled content doesn't cover consumer-OAuth billing
policy. Every claim about this repository's own code is cited to a
`file:line` read directly this session — `crates/api/src/auth.rs`,
`crates/api/src/routes/auth.rs`, `crates/api/src/data/users.rs`,
`crates/enricher/src/config.rs`, `crates/enricher/src/llm.rs`,
`crates/enricher/src/stream.rs`, `frontend/app/api/[...path]/route.ts`,
`frontend/components/AutoRefresh.tsx`, `frontend/components/TrackTrainForm.tsx`,
and the `railMcp` Helm chart templates/values this session's prior work
added. Every claim about `distant-signal-mcp`'s current real shape is
cited to a file read directly in `/workspaces/distant-signal-mcp` this
session (`README.md`, `src/server.ts`, `src/ds/annotateLeg.ts`,
`src/tools/plan-journey.ts`, `src/tools/rendering.ts`, `package.json`) —
its own working code, not the design/plan documents' description of what
was intended, though those were also read for context and are cited where
they help characterize a decision already made.

## Corrections (2026-09-02): the public-exposure blocker has been removed

**What changed, precisely.** This document's original Architecture recommendation
(Section 1, and Recommendation item 1) chose Option B over Option A for one
decisive, non-preference reason: Anthropic's MCP connector documentation
states a connector-attached remote MCP server "must be publicly exposed
through HTTP," which directly conflicted with `distant-signal-mcp`'s
then-shipped `ClusterIP`-only, `railMcp.enabled: false`-by-default
deployment. **That blocker no longer applies.** The user has directed
making `distant-signal-mcp` publicly accessible, gated behind Distant
Signal's own first-party OIDC SSO session system and other access rules.
A sibling task in this same session is revising
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s own
Decision 4 (Auth) and Decision 6 (Deployment) to design this concretely;
this correction was written without waiting for that document to land,
working from the decision as described in this task's own brief, and
cross-references it by path below rather than its finished content.

This correction re-opens Section 1's Option A/B analysis, re-verifies the
public-exposure and data-retention findings against Anthropic's current
documentation (re-fetched today, not assumed unchanged), researches for
the first time a materially different third shape this document did not
originally consider, and explicitly confirms which of the original
findings (Sections 2 and 3) are unaffected. Nothing below deletes the
original analysis; each affected section gets a dated correction note
pointing here for the full reasoning, and the original text is left
standing as the "before" half of the record.

### The public-exposure requirement itself: unchanged, re-verified today

Re-fetched in full this session: `platform.claude.com/docs/en/agents-and-tools/mcp-connector`.
Its "Limitations" section reads, word-for-word identical to the original
research pass's citation: *"The server must be publicly exposed through
HTTP (supports both Streamable HTTP and SSE transports). Local STDIO
servers cannot be connected directly."* Its "Data retention" section is
also unchanged: *"The MCP connector is not covered by ZDR arrangements.
Data exchanged with MCP servers, including tool definitions and execution
results, is retained according to Anthropic's standard data retention
policy."* **Neither of these is a documentation change** — the correction
is entirely on Distant Signal's side (a deliberate decision to make
`distant-signal-mcp` public), not a loosening of Anthropic's own
requirements. Worth stating plainly since it would be easy to assume the
blocker was somehow resolved by Anthropic; it wasn't — DS simply chose to
satisfy a requirement that was always there.

### Three shapes, not two — the original "Option A" undersold what "Anthropic's remote MCP connector" actually covers

The original Section 1 defined Option A narrowly: *"DS's own backend...
holds an Anthropic API key and calls `POST /v1/messages` with
`distant-signal-mcp`'s URL wired in via the `mcp_servers` request
parameter."* That is a real, distinct mechanism — call it **Option A1**
below — but it is not the only way "Anthropic's remote MCP connector"
reaches an end user, and it is not the shape most worth re-evaluating now
that the public-exposure blocker is gone. Fetched today:
`claude.com/docs/connectors/building/authentication` (Anthropic's current
first-party developer docs for connectors — the Help Center article this
session also fetched,
`support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp`,
explicitly states "The guide to building custom connectors has moved" to
this page) describes a second, materially different mechanism:

- **Option A1 (as originally scoped): DS's own backend calls the Messages
  API with `mcp_servers`.** DS still holds an Anthropic API key, still
  pays per-token, still hosts the request/response relay, and (per the
  fetched authentication page's "Authentication" and "Custom connectors"
  sections) is now also responsible for obtaining a valid OAuth
  `authorization_token` for `distant-signal-mcp` on the calling user's
  behalf before every request — *"API consumers are expected to handle
  the OAuth flow and obtain the access token prior to making the API
  call, and to refresh the token as needed"* (MCP connector page,
  "Authentication" section, re-fetched this session). This is genuinely
  new work Option A1 didn't need to do when the blocker analysis stopped
  at "public reachability" — DS's orchestrator would need real per-user
  token acquisition/refresh logic against its own MCP server, on top of
  everything Option B already needs.
- **Option C (not considered in the original pass at all): an individual
  end user adds `distant-signal-mcp` as a custom connector directly in
  their own Claude.ai / Claude Desktop / Claude mobile / Cowork account,
  and chats entirely inside Claude's own interface — never touching
  Distant Signal's frontend.** Confirmed directly from the Help Center
  article fetched this session: users go to **Customize > Connectors**,
  click the "+" button, choose "Add custom connector," and enter
  `distant-signal-mcp`'s URL (optionally an OAuth client ID/secret under
  "Advanced settings"); free accounts get one custom connector, Pro/Max
  get full support, Team/Enterprise route through an org Owner.
  **"Conversations occur entirely within Claude.ai's standard interface"**
  (same source) — this is not an embedded-in-DS feature at all, it's a
  bring-your-own-Claude-client feature DS enables by publishing a
  connector.

Option A1 and Option C share the same public-reachability and
OAuth-authentication prerequisites, but are otherwise different products
with different cost and UX consequences — treating "Option A" as one
thing, as the original document did, obscures a real decision the removed
blocker now actually opens up. Both are addressed below and in Section
1's own correction subsection.

### Does DS's own OIDC session gate map onto how Claude expects a remote MCP server to authenticate callers?

Yes, in shape — but it's new engineering, not a free byproduct of the OIDC
relying-party system DS already has. This is the piece most worth
re-verifying, since the original rejection of Option A was purely about
public reachability and never examined Claude's authentication model at
all. Fetched in full this session, `claude.com/docs/connectors/building/authentication`
— Anthropic's current, first-party reference for exactly this question:

- **Per-user OAuth is the well-supported, expected default, not an edge
  case.** The page's authentication-types table lists `oauth_dcr` (RFC
  7591 Dynamic Client Registration, "supported out of the box"),
  `oauth_cimd` (Client ID Metadata Documents, also out of the box),
  `oauth_anthropic_creds` (Anthropic holds the OAuth client credentials,
  by arrangement), `static_headers` (one fixed org-level credential,
  beta), and `none` (authless). **"A pure machine-to-machine
  `client_credentials` grant... is not supported. Every connection
  requires user consent."** — real per-user identity, not a shared
  secret, is the norm Claude's connector infrastructure is built around,
  which is directly the shape "gate `distant-signal-mcp` behind DS's own
  user OIDC session" needs.
- **The MCP server does not have to be its own OAuth authorization
  server.** The page's "Cross-host authorization servers" section states
  the authorization server named in a server's Protected Resource
  Metadata (RFC 9728) "doesn't need anything special on its own... Claude
  resolves it regardless of which host it points at." Concretely:
  `distant-signal-mcp` could stay a thin **resource server** (validate
  bearer tokens, serve `/.well-known/oauth-protected-resource`) while an
  existing OAuth/OIDC **authorization server** issues the tokens — which
  is architecturally exactly the split "gated behind DS's existing OIDC
  SSO" implies, *if* that existing IdP (the one
  `crates/api/src/auth/oidc.rs` is already a relying party of, per the
  earlier train-mcp research doc's Auth section) can itself serve RFC
  8414/OIDC Discovery metadata, support PKCE with
  `code_challenge_methods_supported: ["S256"]` (mandatory — "Claude
  includes a PKCE `code_challenge` with `code_challenge_method=S256` on
  every authorization request"), and either expose a DCR
  `registration_endpoint` or advertise CIMD support. **This is not
  verified here** — whether DS's specific third-party OIDC provider
  supports DCR/CIMD/PKCE the way Claude's connector infrastructure
  expects is a concrete, checkable question that belongs to the sibling
  design doc's own Decision 4, not this document; flagged as a new open
  question below, not answered.
- **This is real, non-trivial new scope for `distant-signal-mcp` itself,
  not a checkbox.** Today `distant-signal-mcp` is gated by a flat
  Discord-user-ID allowlist (`DISCORD_ALLOWED_USER_IDS`, per the original
  research's own Section 1 citation) — a shared-secret check, not an
  OAuth resource-server implementation. Becoming
  Claude-connector-authenticatable means implementing (at minimum): the
  `401` + `WWW-Authenticate: Bearer resource_metadata=...` handshake on
  every unauthenticated request ("Claude does not honor a
  `WWW-Authenticate` header on a `200` response"), a correctly-shaped
  Protected Resource Metadata document whose `resource` field matches the
  connector URL exactly, bearer-token validation (signature, issuer,
  audience, expiry, scope) on every tool call, and (per "Token refresh")
  RFC 6749-compliant error codes so Claude's own refresh logic works.
  None of this exists in `distant-signal-mcp` today. It maps cleanly onto
  DS's stated intent *in concept* — but "cleanly" should not be read as
  "cheaply": this is genuinely comparable in scope to the
  OAuth-authorization-server-adjacent work the sibling design doc's
  Decision 4 is already taking on for its own reasons, not a separate
  cost Option C adds on top for free.
- **A concrete, favorable detail for network shape**: the Help Center
  article states the requirement more precisely than "fully public" —
  *"Your MCP server must be reachable over the public internet from
  Anthropic's IP ranges. Servers on private networks require firewall
  allowlisting of Anthropic's IPs."* The connector's own developer docs
  give the actual range: **`160.79.104.0/21`** ("Network reference"
  section, `claude.com/docs/connectors/building/authentication`). A
  human's own browser also needs to reach the authorization server's
  consent screen directly (the OAuth redirect is user-driven), so this
  doesn't reduce the MCP endpoint itself to "IP-allowlist only" — but
  it's a more precise, citable fact than the original document had,
  worth carrying into whichever document ends up specifying
  `distant-signal-mcp`'s actual ingress rules (the sibling Decision 6).

### What this does and doesn't change about Section 1's Option A vs. B conclusion

The original recommendation's *other* reasons for Option B — independent
of the public-reachability blocker — were real and are unaffected by any
of the above:

- Option B keeps `distant-signal-mcp`'s tool-call traffic entirely inside
  DS's own network, so it never becomes subject to Anthropic's non-ZDR
  MCP-connector data retention policy at all (a cost Option A1 still
  pays, unchanged from the original finding — the retention wording
  above is identical to before).
- Option B is the only shape where DS's own orchestrator sits in the
  tool-calling loop and can layer DS-specific context (rate limiting, its
  own error handling/retry policy, prompt/system-message control, and —
  newly, see below — a legitimately-scoped per-user session) onto every
  tool call, rather than handing that loop to Anthropic's infrastructure.
- Option A1 does not become simpler now that public exposure is allowed —
  it becomes *harder* than the original document's blocker-only framing
  suggested, because it now additionally needs DS's backend to do real
  per-user OAuth token acquisition/refresh against its own MCP server
  (see above), work Option B's direct MCP-client call never needed.

**So: for a DS-hosted, embedded-in-DS-frontend chatbot, nothing here
overturns Option B.** Option A1 specifically remains not recommended, for
reasons independent of the now-resolved public-exposure blocker.

**What is new: a legitimately-scoped per-user token becomes available to
Option B's own orchestrator, closing a gap the original document flagged
as unresolved.** The original Section 1 flagged that `distant-signal-mcp`'s
tools "never accept or forward a caller's session at all," so an embedded
orchestrator with the user's real DS session cookie available couldn't
actually use it to unlock `annotateLeg.ts`'s TRUST-corroboration tier
without new, undesigned code. Once `distant-signal-mcp` requires real
per-caller OAuth tokens anyway (a consequence of the sibling Decision 4,
not something this document is deciding), **a DS-hosted Option B
orchestrator — which already knows which DS user it's acting for — is a
natural, already-privileged position to mint or forward a correctly-scoped
access token for that same user**, rather than calling anonymously. This
doesn't design the session-forwarding variant (still explicitly out of
scope, per the original document's own "Explicitly out of scope" list and
Open question 4), but it means the prerequisite blocking it —
`distant-signal-mcp` having no per-caller identity concept at all — is
now expected to go away as a side effect of the sibling deployment
decision, independent of whatever else this document concludes.

**What is genuinely new and worth designing as its own decision: Option
C.** A user connecting `distant-signal-mcp` directly to their own
Claude.ai/Desktop account is not a variant of Option A or B — see Section
4's correction for the UX consequence and Cost/risk's correction for the
billing consequence.

## 1. Architecture: how would an embedded chatbot actually call `distant-signal-mcp`?

### The two options, precisely

**Option A — Anthropic's remote MCP connector, called from a DS-hosted
orchestrator.** DS's own backend (or a new small service) holds an
Anthropic API key and calls `POST /v1/messages` with `distant-signal-mcp`'s
URL wired in via the `mcp_servers` request parameter, letting **Anthropic's
own infrastructure** call the MCP server's tools directly — DS's
orchestrator never touches the MCP protocol itself, it just relays the
Messages API's streamed response to the frontend.

Verified directly against Anthropic's current MCP connector documentation
(fetched in full this session,
`platform.claude.com/docs/en/agents-and-tools/mcp-connector`, Beta,
required header `anthropic-beta: mcp-client-2025-11-20` — supersedes a
now-deprecated `mcp-client-2025-04-04`): this is real and does support a
*remote* server, not just local/stdio. The exact shape (not approximated —
copied from the fetched page):

```json
"mcp_servers": [
  { "type": "url", "url": "https://example-server.modelcontextprotocol.io/sse",
    "name": "example-mcp", "authorization_token": "YOUR_TOKEN" }
],
"tools": [
  { "type": "mcp_toolset", "mcp_server_name": "example-mcp" }
]
```

Both halves are required — the `claude-api` skill's own "Common Pitfalls"
list independently states this exact validation rule ("MCP connector needs
both halves... rejected as a validation error" without the matching
`mcp_toolset`), and the fetched page's own "Validation rules" section
confirms it from the other direction ("Server must be used: Every MCP
server defined in `mcp_servers` must be referenced by exactly one
MCPToolset").

**This is where Option A runs into a real, load-bearing conflict with a
decision this session's own prior work already made.** The fetched page's
own "Limitations" section states plainly: **"The server must be publicly
exposed through HTTP (supports both Streamable HTTP and SSE transports).
Local STDIO servers cannot be connected directly."** Anthropic's servers
have to be able to reach the MCP server over the public internet — there
is no private-network, VPC-peering, or on-request-only mode documented.
`distant-signal-mcp` is deployed today as exactly the opposite:
`railmcp-service.yaml` declares `type: ClusterIP` with the file's own
top comment stating "no external Ingress/TLS is sketched here, matching
Decision 6's own shallow deployment depth"
(`charts/distant-signal/templates/railmcp-service.yaml:1-8`), and
`railMcp.enabled` defaults to `false`
(`charts/distant-signal/values.yaml:768-772`, "Opt-in; false by default").
Nothing in this deployment is reachable from outside the cluster today,
by design. Option A would require either (a) fronting `distant-signal-mcp`
with a real public Ingress/TLS endpoint — genuinely new deployment surface
this session's prior work explicitly declined to design ("no external
Ingress/TLS is sketched here... out of scope",
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md:742-744`)
— or (b) standing up a *second*, publicly-exposed instance just for this
feature, duplicating the deployment. Either way, "opt-in, disabled by
default, internal-only" — the posture this session's prior work
deliberately chose — does not survive Option A intact; a chatbot feature
built this way forces a new public attack surface into existence as a
prerequisite, not a side effect.

A second real cost: the fetched page's "Data retention" section states
the MCP connector "is not covered by ZDR [zero data retention]
arrangements. Data exchanged with MCP servers, including tool definitions
and execution results, is retained according to Anthropic's standard data
retention policy." This means the NRE/Darwin/Network-Rail-derived text
DS's `distant-signal-mcp` tools return (line-status `reason` strings,
Knowledgebase incident descriptions — the same data the design spec's own
Licensing note already flagged as an open attribution question, see
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
Licensing note) would be retained by Anthropic under its standard policy
whenever this path runs — a new fact for whoever eventually resolves that
open attribution/licensing question, not resolved here.

**Option B — DS's own backend acts as the MCP client, in a conventional
tool-use loop.** An orchestrating service holds the Anthropic API key,
calls `POST /v1/messages` with ordinary custom `tools` (not `mcp_servers`),
and when Claude returns a `tool_use` block, the orchestrator itself calls
`distant-signal-mcp` — over the **internal, ClusterIP-only** network DS
already runs it on today — and feeds the result back as a `tool_result`
content block. No change to `distant-signal-mcp`'s deployment posture is
required; Anthropic's servers never need to reach it.

This does need real MCP-client code somewhere, but not necessarily new
Rust code. Two facts, both verified this session, point the same direction:

- **`distant-signal-mcp` is already TypeScript and already declares
  `@modelcontextprotocol/sdk` as a dependency**
  (`/workspaces/distant-signal-mcp/package.json:20`,
  `"@modelcontextprotocol/sdk": "1.29.0"`) — it does not yet declare
  `@anthropic-ai/sdk` (confirmed by the same file read), meaning today it
  is purely an MCP *server*, with no Anthropic-API-calling code at all.
- **Anthropic's own TypeScript SDK ships first-class MCP *client* helpers**
  for exactly this composition — confirmed directly from the fetched MCP
  connector page's "Client-side MCP helpers" section:
  `mcpTools(tools, mcpClient)` "Converts MCP tools to Claude API tools for
  use with `client.beta.messages.toolRunner()`", installed via
  `@anthropic-ai/sdk/helpers/beta/mcp`, alongside the official MCP
  TypeScript SDK's own `Client`/transport classes. The Rust SDK story is
  comparatively undocumented for this: neither the `claude-api` skill's
  per-language reading guide (which enumerates Python/TypeScript/Java/Go/
  Ruby/C#/PHP file sets throughout, e.g. its "Language-Specific Feature
  Support" section) nor this fetched page's own "Installation" tabs (which
  list Python, TypeScript, C#, Go, Java, PHP, Ruby — no Rust) mention a
  Rust MCP-client helper package at all, consistent with this session's
  earlier finding that "the Rust MCP ecosystem" is "considerably" less
  mature than the TypeScript one
  (`docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`'s
  own "Sketch: where would a derived service live" section, re-cited here
  as still-current reasoning, not re-verified independently this session).

The practical implication: **the natural home for an Option-B orchestrator
is TypeScript, not a new Rust crate** — either a new module inside
`distant-signal-mcp`'s own fork (it already has the MCP server *and*
already deploys as a chart-managed external component DS's CI doesn't
build, per Decision 1 of the prior design spec — adding an
Anthropic-SDK-calling orchestration layer to the same service is a much
smaller step than adding one to `crates/api`), or a new, small sibling
TypeScript service. Either shape reuses the official TS MCP client helpers
against `distant-signal-mcp`'s own already-running, already-internal
`ClusterIP` endpoint, using the ordinary Streamable HTTP MCP transport
DS's Helm chart already exposes (`railmcp-service.yaml`'s `http` port) —
no protocol translation, no new public exposure, no departure from the
"internal only" posture.

### Comparison against this app's own existing conventions

- **The `/api/[...path]` proxy pattern**
  (`frontend/app/api/[...path]/route.ts:1-154`) already establishes the
  precedent this session's chatbot design should follow: browser-facing
  Client Components can't read server-only env vars like `API_BASE_URL`
  (route.ts:3-9), so all backend calls go through a same-origin Next.js
  proxy that forwards cookies both directions. A chat endpoint would need
  the same shape — a same-origin route (not a direct browser→orchestrator
  connection) that can carry the user's `distant_signal_session` cookie
  (`crates/api/src/auth.rs:63-64`'s `SESSION_COOKIE_NAME`) to the
  orchestrator, and stream a response back (SSE or chunked, not sketched
  further here per the task's own "don't design streaming transport
  wire-protocol details" exclusion).
- **`AutoRefresh`'s 30-second polling posture**
  (`frontend/components/AutoRefresh.tsx:1-23`) is irrelevant to a chat
  feature's own request shape (chat is user-initiated, not polled) but is
  a useful contrast: everything this app has built so far is either a
  free, periodically-polled open-data feed or a user-initiated mutation
  against DS's own free-to-operate Postgres-backed API. A chat endpoint
  is the first user-initiated action in this app whose backend cost is
  metered per-call against a paid third-party API — see Cost/risk
  framing below.
- **The session/auth model** (`crates/api/src/auth.rs`,
  `crates/api/src/routes/auth.rs`) is a real, working OIDC-based
  first-party identity system, distinct from `distant-signal-mcp`'s own
  Discord-OAuth gate (its README's own "Authentication" section,
  `/workspaces/distant-signal-mcp/README.md:160-180`). An embedded chatbot
  sits inside DS's own frontend, so it's the *first* consumer of
  `distant-signal-mcp`'s tools that could plausibly carry a real DS
  session forward — a genuinely different situation from the external
  MCP-client design, which explicitly could not (Decision 4 of the prior
  design spec chose an anonymous-only `DsApiClient` specifically because
  train-mcp's Discord-authenticated caller has no DS session at all,
  `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md:494-522`).
  This matters concretely for `annotateLeg.ts`'s narrow TRUST-corroboration
  tier: `src/ds/client.ts`'s `DsApiClient` (per the plan's own Task 2
  code, `docs/superpowers/plans/2026-09-01-train-mcp-integration.md:271-332`)
  is anonymous by construction and carries no session — an embedded
  orchestrator that has the user's real session cookie available
  (because it's the same first-party surface) does **not** automatically
  get to use it for this tier: `distant-signal-mcp`'s tools as they exist
  today never accept or forward a caller's session at all. Realizing this
  advantage would require new code (a session-forwarding variant of the
  DS client, or a parallel authenticated tool path) that doesn't exist —
  a real, concrete follow-up this research surfaces but does not design.

### Recommendation for this section

**Option B (DS-hosted orchestrator as its own MCP client, calling
`distant-signal-mcp` over the internal network) over Option A (Anthropic's
remote MCP connector).** The deciding fact is Anthropic's own documented
requirement that a connector-attached MCP server "must be publicly
exposed through HTTP" — this is not a preference, it's a hard technical
requirement verified directly against Anthropic's current docs, and it
directly contradicts the internal-only, opt-in-disabled-by-default posture
this session's own prior work chose for exactly this service. Option B
needs real new code (an Anthropic-SDK-calling orchestrator, most naturally
added to `distant-signal-mcp`'s own TypeScript fork or a small sibling
service) but that code is a documented, first-class SDK pattern
(`mcpTools()` + `toolRunner()`), not a research risk — the risk in this
section was architectural, not implementation-difficulty, and it resolves
cleanly in Option B's favor.

### Correction (2026-09-02)

See the top-level "Corrections (2026-09-02)" section for the full
re-analysis this pointer summarizes. Short version: the public-exposure
blocker that drove this section's recommendation is gone (DS has decided
to make `distant-signal-mcp` public and OIDC-gated), but the recommendation
**for an embedded-in-DS-frontend chatbot is unchanged — Option B over
Option A1** — for reasons independent of that blocker (no Anthropic
non-ZDR retention exposure, DS keeps control of the tool-calling loop, and
Option A1 now additionally needs DS to run its own per-user OAuth token
acquisition against its own MCP server, work Option B never needed). What
*is* new: a third, previously-unconsidered shape — **Option C**, a user
connecting `distant-signal-mcp` directly to their own Claude.ai/Claude
Desktop account via Anthropic's custom-connector feature, chatting
entirely inside Claude's own UI, billed to the user's own Claude plan — is
now genuinely viable, well-supported by Anthropic's documented OAuth model
for remote connectors (`claude.com/docs/connectors/building/authentication`,
fetched in full this session), and worth a real product decision of its
own, distinct from the Option A/B choice this section made. Not a strict
alternative to Option B — a different product. See Sections 4 and
Cost/risk below for the UX and billing consequences.

## 2. "Let users link their own Anthropic account" — verified, not assumed

**Correction (2026-09-02):** this section's conclusion is unaffected by
the public-MCP-server decision and is not re-opened here — it was about
whether DS's *own embedded orchestrator* could let a user delegate their
personal Anthropic subscription to DS, a different question from either
Option A1/B (DS's own API key pays) or the newly-considered Option C
(Section 1's Correction, above). In Option C, the user's own Claude
subscription pays directly, through their own already-authenticated
Claude client — DS never touches the user's Anthropic identity at all in
that shape, so the "not a real, stable delegation mechanism" finding
below doesn't even apply to it; there's no delegation step for it to
apply to. Confirmed, not re-opened.

The task asked this to be checked precisely, distinguishing two very
different mechanisms: **OAuth account delegation** (a "Sign in with
Anthropic" button that hands a third-party app the ability to spend a
user's own Anthropic subscription/usage on their behalf, analogous to
"Sign in with Google") versus **bring-your-own-API-key** (a user manually
pastes a personal API key from `console.anthropic.com` into the app's
settings, and the app calls the API using that key, billed to the user's
own Console account).

**Finding, stated plainly: something OAuth-shaped exists, but it is not
the stable, documented, general-purpose developer feature the first
mechanism implies, and it should not be designed around as if it were.**

What was found (web search, not the `claude-api` skill — the skill's own
Authentication section documents `ant auth login`-based CLI/SDK credential
profiles for a *developer's own* machine, not an end-user account-linking
flow for a hosted multi-user web app, and never describes a "Sign in with
Anthropic" integration surface for third parties):

- Anthropic does let a *recognized set* of third-party developer tools
  (Cursor, Windsurf, Cline, and others, alongside Anthropic's own Claude
  Code) authenticate a user via the same OAuth flow `claude.ai`/Claude
  Code itself uses, drawing on the user's Claude subscription rather than
  a pasted API key.
- As of early 2026, usage through this OAuth path was moved to a separate,
  prepaid "extra usage credits" balance, distinct from the user's normal
  Pro/Max/Team plan limits — explicitly to stop third-party tools from
  eating into a user's regular `claude.ai` chat quota.
- **This mechanism has been through real, publicly visible policy
  instability throughout 2026**: Anthropic's Consumer Terms of Service
  were updated around February 17–18, 2026 to restrict this OAuth path to
  Claude Code and `claude.ai` specifically, with server-side enforcement
  reported from January 2026 onward; a further enforcement action on
  April 4, 2026 cut off several named third-party harnesses; messaging
  from Anthropic staff was reported as inconsistent in the following
  weeks; and a June 16, 2026 email reportedly walked back a *further*
  planned restriction specifically for Claude Agent SDK / `claude -p`
  usage. This is drawn from third-party reporting (see Sources below),
  not from Anthropic's own developer documentation — no page in
  `platform.claude.com`'s docs (the domain this session's other MCP
  connector fetch confirmed is Anthropic's current canonical docs host)
  was found describing this OAuth mechanism as a supported integration
  pattern for a third party to build against, and the `claude-api` skill
  — this session's designated primary source of truth for exactly this
  kind of claim — does not document it as one either.

**Conclusion: this is not a mechanism Distant Signal should design around.**
It exists in the sense that named coding tools use it today, but it is
(a) undocumented as a general developer-facing integration surface
Distant Signal could register for the way it registered its own OIDC
relying-party client against its SSO provider
(`crates/api/src/auth/oidc.rs`, per the earlier train-mcp research doc's
Auth section), (b) tied specifically to Anthropic's own consumer
subscription products (`claude.ai`, Claude Code) rather than general API
usage a third-party web app would call through, and (c) subject to
policy reversals within the same calendar year this research was written,
which alone makes it an unsound foundation for a public rail-status app's
production feature. Presenting it as equivalent to "Sign in with Google"
would be the exact overstatement the task asked this research to avoid.

**What is real and stable: bring-your-own-API-key.** This is the
mechanism the `claude-api` skill documents throughout as the normal way
any application calls the Claude API — a plain `ANTHROPIC_API_KEY`,
resolved by the SDK's normal credential chain (the skill's own
Authentication Quick Reference), obtained by a user from their own
Anthropic Console account and pasted into the calling application's
config. Distant Signal has a real, established pattern for *where* a
secret like this would live if adopted as a per-deployment (not
per-user) credential: Kubernetes `Secret` objects referenced via
`secretKeyRef`, following the exact shape `SSO_CLIENT_SECRET` already uses
(`charts/distant-signal/templates/api-deployment.yaml:132-136`,
`{{ include "distant-signal.ssoClientSecretName" . }}` /
`ssoClientSecretKey`) and the shape `railmcp-deployment.yaml` already uses
for the Discord/LDBWS credentials `distant-signal-mcp` needs
(`charts/distant-signal/templates/railmcp-deployment.yaml:68-107`, every
one of `DISCORD_CLIENT_ID`/`DISCORD_ALLOWED_USER_IDS`/the four LDBWS
values sourced via `secretKeyRef` against a chart-rendered `Secret`). If
one Anthropic API key funds every conversation for every DS user (the
operator's own key, not a per-user credential), this is a direct,
low-risk fit for that existing pattern — a new `chatbot.anthropicApiKey`-
shaped value alongside `railMcp`'s own block.

**What has no existing pattern in this app at all: a *per-user* pasted
credential.** If instead each user is meant to paste their *own* personal
Anthropic API key (so their own conversations bill to their own Console
account, not DS's), this is a different problem DS has never solved:
every secret this app currently stores server-side is either a
single, operator-provisioned, deployment-wide credential (the OIDC client
secret, the `railMcp` Discord/LDBWS values — one value, one Kubernetes
`Secret`, read once at pod startup) or a value DS itself mints and only
ever compares a hash of (`crates/api/src/data/users.rs`'s `sessions`
table — `hash_session_token`, `crates/api/src/auth.rs:169-175`, "mirrors
how a password hash works: a DB dump/leak alone can't be replayed... only
the original random token can"). Neither pattern fits a secret DS must
**store in a form it can use again** (an API key has to be sent to
Anthropic verbatim on every call — it can't be hashed the way a session
token is) **and that is scoped per-user, not per-deployment** (no existing
table holds anything like this — `users`/`sessions`/`oidc_login_state`,
per `crates/api/src/data/users.rs`'s own module doc, are the entire
current schema for user-scoped state). Building this would mean a new
`user_credentials`-shaped table, a real column-encryption decision (the
design spec's own Open Question 5 for the *refresh_token* column —
"this schema would hold it in plaintext with no column-encryption
precedent anywhere in the repo",
`crates/api/src/data/users.rs:82-93`'s own doc comment — applies with
even more force to an externally-billed, replayable-for-real-money API
key), and a UI for a user to enter/rotate/revoke it. None of this is
designed here; it's flagged as real, non-trivial scope distinct from
"add a config value," matching the task's own instruction not to present
BYO-API-key as a small thing either.

## 3. Gating certain users onto the local LLM server

**Correction (2026-09-02):** unaffected by the public-MCP-server decision.
This section's findings (serial contention in `crates/enricher`, the
deployed model's unconfirmed tool-calling support, and this app's lack of
any role/tier concept) are entirely about how DS's *own* backend/orchestrator
(Option B) would source its LLM calls and gate access to that orchestrator
— a question orthogonal to whether `distant-signal-mcp` itself is public
or OIDC-gated. It is also not applicable to Option C at all (Section 1's
Correction): a user connecting their own Claude.ai/Desktop account doesn't
route through DS's enricher LLM endpoint or DS's own auth model in any
way. Confirmed unchanged; relevant only if Option B ships.

Three genuinely separate questions, per the task brief — capacity,
suitability, and authorization. All three are real, and none is small.

### 3a. Capacity/contention

**Confirmed directly: the enricher pipeline calls its configured LLM
endpoint strictly serially today, by explicit design, with no concurrency
and no priority mechanism at all.** `LlmClient::new`'s own doc comment
states this plainly: *"both callers of this client — the stream consumer
loop and the hourly sweep — process incidents strictly serially, so a
single hung endpoint would stall ALL enrichment indefinitely rather than
just losing one incident"* (`crates/enricher/src/llm.rs:400-410`).
`stream.rs`'s own Redis Stream consumer reads and blocks on **one** entry
at a time (`crates/enricher/src/stream.rs:36-46`, `.count(1)`), and
`main.rs`'s reclaim/sweep loops (`sweep_interval_secs`, default 3600s;
`reclaim_interval_secs`, default 60s; `reclaim_min_idle_secs`, default
1000s — all `crates/enricher/src/config.rs:35-56`) exist to retry a
*stalled or crashed* extraction, not to add throughput or fairness.
There is no `Semaphore`, no request queue with priority, no rate limiter
anywhere in `crates/enricher` (confirmed by grep across the crate) — the
only thing bounding how many requests hit the endpoint concurrently is
that this app never issues more than one at a time, by construction.

**Consequence: routing conversational chatbot traffic through this same
endpoint would create real, direct contention with incident processing,
and there is no existing mechanism in this app to protect against it.**
A chatbot conversation issuing even one concurrent request to the same
`llm_base_url` would either queue behind, or (if the endpoint itself
allows concurrent connections despite this app's own serial usage) compete
for the same compute with, the incident-extraction pipeline — and given
the endpoint's own already-measured latency (see 3b), a single busy chat
turn is comparable in duration to one extraction call. Sharing this
endpoint for a materially different traffic shape (bursty, user-driven,
open-ended conversation length) would need new infrastructure — at
minimum a priority queue or a second, separate model/endpoint instance
carved out for chat traffic — none of which exists today. This is not a
config toggle; it is new capacity-management work.

### 3b. Model suitability, including tool-calling support — a real, possibly-blocking question

`Config.llm_base_url`'s own doc comment states plainly: *"Base URL of an
OpenAI-compatible Chat Completions endpoint... No vendor is assumed"*
(`crates/enricher/src/config.rs:11-13`) — the config layer is deliberately
generic. But this session found a concrete data point about what's
*actually* deployed behind it: `llm_request_timeout_secs`'s own doc
comment records a real benchmark against a real self-hosted endpoint,
**"Ollama, qwen3.5:4b"**, measuring "single-call latencies of 86-104s for
the *flat* single-period case alone" (`crates/enricher/src/config.rs:24-33`,
dated 2026-08-21). A 4-billion-parameter model is a small, extraction-sized
model — chosen (reasonably) for a narrow, structured-output task, not
general open-ended conversation. Whether it is a *good* conversational
agent for turn-taking, ambiguity-handling journey-planning dialogue is a
real quality question this research cannot resolve without a live eval,
but the size alone is a strong signal the choice was optimized for a
different job.

**More consequential: today's actual usage pattern gives no evidence this
endpoint supports tool use / function calling / MCP-shaped orchestration
at all — and confirms it isn't exercising that capability today, whatever
the model may or may not theoretically support.** `chat_completion`'s
request body sets `response_format: { kind: "json_schema", json_schema: {
strict: true, ... } }` (`crates/enricher/src/llm.rs:415-435`) and
`temperature: 0.0` (same block) — this is the OpenAI-compatible
**structured-output** mode (constrained JSON generation against a fixed
schema), not the `tools`/`tool_choice`/`function_call` request-parameter
family a tool-calling agent loop needs. "OpenAI-compatible chat
completions" as a category does not by itself imply tool-calling support:
the Chat Completions schema has a `tools` field that many
compatible servers implement, but "compatible" commonly means "accepts
these request fields and doesn't error," not "correctly performs
multi-step tool selection and argument generation the way the served
model was trained to." Whether the specific deployed model
(`qwen3.5:4b` via Ollama, per the citation above — and note this is a
timeout-tuning comment's incidental mention, not a canonical
"the deployed model is X" statement; the actual currently-running model
could differ) reliably drives an MCP-shaped multi-turn tool loop is
**unconfirmed and would need to be checked directly against that specific
deployment before this path could be considered technically viable at
all** — this is flagged, per the task brief, as a real, possibly-blocking
technical question, not merely a quality-of-experience one.

### 3c. Authorization — confirmed real, non-trivial new scope, not a config flag

**Confirmed by direct inspection this session (not merely restated from
the task brief): this app's authentication system has no concept of
roles, permission tiers, or access groups at all.**
`crates/api/src/auth.rs` defines exactly two authenticated-request
shapes: `AuthenticatedUser` (id/email/name, from a valid session — lines
158-179) and `OptionalAuthenticatedUser` (same, or `None` — lines
181-188). Neither carries anything role-shaped. `crates/api/src/data/users.rs`'s
`User`/`SessionUser` structs are `{ id, email, name }` (lines 47-51,
100-106) — no `role`, `tier`, `permissions`, or `groups` column anywhere.
`crates/api/src/routes/auth.rs`'s login/callback/session/logout handlers
(the entire auth surface) never branch on anything but "is there a valid
session" — every authenticated user is, today, identically "an
authenticated user," full stop.

**Gating a new feature to "certain users" is therefore not a small
addition — it requires designing a whole authorization concept this app
doesn't have, from nothing.** Sketched, at research depth only, two real
options bounded by what this app's existing user/session model could
cheaply support:

- **Smallest viable version: a hand-curated allowlist table.** A new
  `chatbot_allowed_users (user_id TEXT PRIMARY KEY REFERENCES users(id))`
  table (or a single `chatbot_allowed_users` env-var-seeded set, mirroring
  `distant-signal-mcp`'s own `DISCORD_ALLOWED_USER_IDS`,
  `/workspaces/distant-signal-mcp/README.md:148`, "Comma-separated Discord
  user IDs permitted to use this server. Must not be empty" — the exact
  shape of gate this app would be reaching for again, just against DS's
  own `users.id` instead of a Discord snowflake), checked by a new
  extractor analogous to `AuthenticatedUser` (`ChatbotAuthorizedUser`,
  wrapping `AuthenticatedUser` with one more table lookup). Cheapest by
  far, no schema concept beyond "is this specific user ID on a list" —
  proportionate to a feature an operator wants to soft-launch to a
  handful of people, not to a general product tier system.
- **Fuller version: an actual role/permission-tier system.** A `role`
  column on `users` (or a separate `user_roles` join table for
  multi-role), checked generically by new middleware, opens the door to
  future tiers beyond just this feature — but is real, independent
  product/schema design work with no existing precedent to build from in
  this codebase at all (not even a partial one — this session's grep
  found nothing role-shaped anywhere in `crates/api`). Disproportionate
  scope if the only current motivating feature is "gate the chatbot,"
  and risks becoming a parallel, half-finished authorization system the
  task brief specifically warned against over-designing here.

Neither option is designed further in this document — this section's job
was to characterize the size of the gap honestly, which the task brief
asked for explicitly, not to close it.

## 4. Frontend UX shape (research depth only)

**Where it would live.** A new, dedicated surface reads more consistent
with this app's existing navigation shape than a persistent floating
widget: `frontend/app/` today has one top-level route per concern
(`track/`, `stations/[crs]/`, `lines/[id]/`, `incidents/[id]/`,
`train/[uid]/` — confirmed via `find frontend/app -maxdepth 2 -type d`
this session) with no existing global chrome element beyond
`AutoRefresh` (a side-effect-only, render-nothing component,
`frontend/components/AutoRefresh.tsx:19-23`). A new `frontend/app/chat/`
(or similar) route fits that pattern; a persistent widget mounted in the
root layout would be the first such element this app has and is a
bigger UX commitment than this research is positioned to recommend
without a design pass of its own.

**Relation to `/track` and `TrackTrainForm`.** `TrackTrainForm`'s own doc
comment frames it as "the v1 entry point for individual train tracking —
a manual form, not a per-departure 'track this train' action... no public
API exposes individual departures today, so a departure-row action can't
be built" (`frontend/components/TrackTrainForm.tsx:8-13`). A conversational
journey-planning chat is not a replacement for this form — `plan_journey`
and `find_services` answer "what train should I catch," which is a
*different* question from `TrackTrainForm`'s "I already know which train,
start tracking it" — but a well-designed chat flow plausibly becomes the
natural on-ramp *into* `TrackTrainForm`: a user asks the chatbot to plan a
journey, gets a concrete `uid`/origin/scheduled-departure back from
`plan_journey`'s `structuredContent` (`RenderedTrainLeg`'s `uid`,
`departureAt`, per `/workspaces/distant-signal-mcp/src/tools/plan-journey.ts:160-179`),
and a "track this leg" affordance in the chat UI could deep-link into
`TrackTrainForm` pre-filled the same way the existing `initialOrigin` prop
already supports for the "Track a train from here" station-page shortcut
(`TrackTrainForm.tsx:18-21`). Not designed further here — a real, cheap-
looking follow-up worth flagging, not worth speccing at this pass's depth.

**Does a first-party chat UI need its own rendering, or can it reuse
`distant-signal-mcp`'s `rendering.ts`?** Read in full this session
(`/workspaces/distant-signal-mcp/src/tools/rendering.ts:1-198`). It is a
set of pure functions (`describeTiming`, `describeDisruption`,
`renderCallingPoint`, `quoteProse`) that turn typed board/service-detail
data into **plain-English prose lines** — designed for an MCP tool's
`content: [{ type: 'text', ... }]` field, i.e. text an LLM (or a
plain-text-rendering MCP client) reads directly, not for a React
component tree. **A first-party chat UI does not need this module's
string-formatting functions directly** — a conversational chat UI's
"rendering" is mostly "display the model's own prose response," which
already synthesizes tool results into conversational text without any
extra work; `rendering.ts`'s job (turning `ServiceDisruption`/`Timing`/
`CallingPoint` into consistent prose) is arguably *redundant* with what
the chat model itself would do when narrating a `plan_journey` result to
the user. **What genuinely is reusable, and worth reusing, are the typed
`structuredContent` shapes** every tool already returns alongside its
text — `plan_journey`'s `RenderedTrainLeg`/`liveStatus` schema
(`plan-journey.ts:1155-1188`, already Zod-validated,
`z.object(trainLegShape)`) and `resolve_station`'s `DsStationMatch`
shape (per the earlier design spec's Decision 2/Task 3 code) are exactly
the typed data a richer chat UI would want for an interactive "journey
card" widget rendered *inside* the chat transcript (departure/arrival
times, platform, live-status badge) rather than as a flat prose block —
porting those TypeScript interfaces into `frontend/lib/types.ts` (the
existing home for shared API-response types, per `TrackTrainForm.tsx:9`'s
own import) is a small, concrete piece of real reuse, distinct from
reusing `rendering.ts`'s text-generation functions, which is not
recommended.

### Correction (2026-09-02): does Option C change this section's "new route inside this app" assumption?

Yes, but only for Option C specifically — the analysis above (placement,
`TrackTrainForm` relation, `rendering.ts`-vs-`structuredContent` reuse) is
about Option B's embedded chatbot and is unaffected if that's what ships;
see the top-level Corrections section for the full Option A/B/C
re-analysis.

If Option C ships (a user connects `distant-signal-mcp` directly to their
own Claude.ai/Claude Desktop account), there is no chat UI inside Distant
Signal's own frontend for that path at all — confirmed directly from the
Help Center article fetched this session
(`support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp`):
"conversations occur entirely within Claude.ai's standard interface." The
`frontend/app/chat/` route sketched above, this section's `rendering.ts`-
vs-`structuredContent` reuse analysis, and the `TrackTrainForm` deep-link
idea are all specific to an embedded orchestrator (Option B) — none of
them are needed, or reachable, for Option C, because Claude's own client
renders the conversation, including however it chooses to present
`plan_journey`'s `structuredContent`, with no DS-authored component
involved anywhere in that path.

What Option C would need from `frontend/` instead is much smaller and
differently shaped: a page telling a user how to connect (the connector
URL, and — once the sibling design's Decision 4 lands — whatever DS-account
sign-in step that connection requires), not a chat surface. This still
fits the same "new, dedicated route per concern" navigation pattern this
section already found this app uses (`track/`, `stations/[crs]/`, etc.) —
just for a short onboarding/instructions page (e.g. a
`frontend/app/connect-claude/`-shaped route) rather than a live chat
transcript. A materially smaller design surface than a chat UI, not
designed further here.

This is not an either/or with Option B's UX shape: DS could ship neither,
either, or both — an embedded first-party chat (Option B, the design
above) and a "connect it to your own Claude" onramp (Option C) are
separate product decisions serving different users (someone who wants a
no-Claude-account-needed experience inside DS's own app, vs. an existing
Claude user who'd rather stay in their own client). Nothing here decides
which, if either, DS should build; it corrects the original document's
implicit assumption that "Option A" was strictly about DS's own frontend
either way — it wasn't, once Option C is distinguished from Option A1.

## Cost/risk framing

**This is a genuinely new class of operational cost for this app, and it
should be named plainly, not hedged.** Every upstream data source Distant
Signal integrates with today is a free, licensed open-data feed — TfL's
modified OGL v2.0, National Rail Enquiries' Terms & Conditions v3.0
covering Knowledgebase/LDBWS/Stations/TOCs, and Network Rail's own TRUST
feed terms (all three cited and read directly by this session's earlier
train-mcp research doc via `frontend/components/OpenDataAttribution.tsx`
and `docs/superpowers/specs/2026-08-28-train-tracking-design.md`). None of
these meter usage per-request the way an LLM API does. A hosted-orchestrator
chatbot (Option B, or Option A) means Distant Signal's own operator pays
Anthropic for every token of every conversation, for as long as the
feature is enabled — a real, ongoing, usage-scaling bill this app has
never had before, on top of (not instead of) its existing infrastructure
costs. Current published first-party pricing (per the `claude-api` skill's
own cached model table, "cached: 2026-06-24"): Claude Sonnet 5 at
$2.00/$10.00 per million input/output tokens, Claude Opus 5 at
$5.00/$25.00 — a single multi-turn `plan_journey`-driving conversation,
especially one that runs an agentic tool loop across several MCP tool
calls, plausibly spends thousands of tokens per turn once tool
definitions and results are counted in context. This is a cost DS's
operator has to actively decide to take on and budget for, not a
side-effect of "just adding a feature" — and it compounds directly with
the local-LLM-gating discussion above: whatever this feature costs to run
against Anthropic's hosted API is exactly the number a "route it through
the free local LLM instead" plan would need to actually save, weighed
against that path's real capacity-contention and tool-calling-support
risks (Section 3).

### Correction (2026-09-02): Option C's cost story is genuinely different — restated plainly, per this section's own instruction not to bury it

The framing above — "Distant Signal's own operator pays Anthropic for
every token of every conversation" — is stated as true of both Option A
and Option B, because at the time both routes required DS's own backend
to hold the Anthropic API key. **That framing is now only true for Option
B and Option A1** (Section 1's Correction). It is **not true for Option
C**: confirmed directly from the Help Center article fetched this session,
connector usage "occurs within the user's existing Claude plan" — the
conversation is billed against the connecting user's own Claude.ai account
(Free/Pro/Max/Team/Enterprise), not against any credential Distant Signal
holds. If Option C ships, **Distant Signal pays $0 in Anthropic API costs
for that conversation** — the "genuinely new class of operational cost"
this section names for Option A1/B does not apply to it at all.

This does not mean Option C is free for DS to operate. Making
`distant-signal-mcp` public and OAuth-gated (the sibling design's own
Decision 6/4) is itself new infrastructure and security-review cost
regardless of which option(s) DS ships — a public, authenticated endpoint
is a new attack surface DS now maintains, whether or not any Anthropic
token is ever billed to DS for using it. The genuinely new claim here is
narrower and more specific than "Option C has no cost" — it's that **the
per-conversation, usage-scaling LLM token cost this section's own pricing
table sizes (Sonnet 5 at $2/$10, Opus 5 at $5/$25 per million tokens) is
the end user's bill to pay, not Distant Signal's, if Option C is the shape
that ships.** Whoever eventually decides between Option B and Option C
should treat this as a real, materially different line item, not a
rounding difference — exactly the kind of framing this section's own
opening line ("should be named plainly, not hedged") already asks for.

## Explicitly out of scope

- **A full RBAC/role-tier system.** Section 3c sketches the smallest
  viable allowlist shape only; a general permission-tier system is a
  separate, independent product decision this document does not design.
- **Streaming transport wire-protocol details** (SSE vs. WebSocket framing,
  reconnect semantics, exact proxy route shape) for relaying the
  orchestrator's response to the browser — flagged as needed in Section 1,
  not designed.
- **A live eval of the enricher's actual deployed model's tool-calling
  quality/reliability.** Section 3b identifies this as a real, possibly-
  blocking open question; resolving it requires running one, not
  researching one.
- **The NRE/Network-Rail-branding attribution question for MCP tool
  output**, already flagged as unresolved by the prior design spec's own
  Licensing note — Section 1's finding that Option A's data additionally
  becomes subject to Anthropic's standard (non-ZDR) retention policy adds
  a new fact to that open question but does not resolve it here.
- **A concrete `frontend/app/chat/` implementation** (component tree,
  exact API surface, exact prompt/system-message design for the
  orchestrator). Section 4 sketches shape and reuse opportunities only.
- **Per-user BYO-API-key storage design** (schema, encryption-at-rest
  mechanism, key rotation/revocation UX) — Section 2 characterizes this
  as real, non-trivial, unprecedented scope in this app, but does not
  design the table or the crypto.
- **Realizing the "embedded chat can carry the user's real DS session
  into `distant-signal-mcp`'s TRUST-corroboration tier" opportunity**
  flagged in Section 1 — genuinely possible given the shared first-party
  origin, but `distant-signal-mcp`'s `DsApiClient` is anonymous-only today
  and nothing in this pass designs the authenticated variant it would
  need.

## Recommendation

Ranked:

1. **If this feature is pursued at all, architect it as Option B (a
   DS-hosted orchestrator acting as its own MCP client against
   `distant-signal-mcp`'s existing internal ClusterIP endpoint, using the
   Anthropic TypeScript SDK's own MCP client helpers), never Option A
   (Anthropic's remote MCP connector).** This is the one finding in this
   document with an unambiguous, verified-against-current-docs technical
   basis: Anthropic's MCP connector requires the tool server to be
   publicly reachable, which directly conflicts with the internal-only,
   opt-in-disabled-by-default posture this session's own prior work
   already chose and shipped for `distant-signal-mcp`. Choosing Option A
   would mean re-opening and reversing that decision as a prerequisite,
   not a side effect.

   **Correction (2026-09-02):** the public-exposure reason above no
   longer applies — DS has decided to make `distant-signal-mcp` public and
   OIDC-gated (see `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
   revised Decision 4/Auth and Decision 6/Deployment). Re-evaluated in
   Section 1's own correction and the top-level "Corrections (2026-09-02)"
   section: **the conclusion for an embedded-in-DS-frontend chatbot is
   unchanged — Option B still over Option A1** (the API-level `mcp_servers`
   connector, as originally scoped) — for reasons independent of the
   removed blocker (no Anthropic non-ZDR retention exposure, DS keeps
   control of the tool-calling loop, and Option A1 now additionally needs
   DS to run its own per-user OAuth token acquisition against its own MCP
   server, which Option B never needed). **What has changed is that a
   third, previously-unconsidered shape — Option C, an end user
   connecting `distant-signal-mcp` directly to their own Claude.ai/Claude
   Desktop account via Anthropic's custom-connector feature, with the
   conversation happening entirely in Claude's own UI and billed to the
   user's own Claude plan — is now genuinely viable and worth a real
   decision of its own**, confirmed against Anthropic's current
   documentation (`claude.com/docs/connectors/building/authentication`,
   fetched in full this session). It is not a strict alternative to
   Option B: it's a different product (bring-your-own-Claude-client vs.
   DS's own embedded chat) with a different cost profile (user-funded, not
   DS-funded — see item 2's own correction below) and no DS frontend
   chat-UI work, only a short connect/onboarding page. See the top-level
   Corrections section for the full re-analysis.
2. **Treat "let users link their own Anthropic account" as not currently
   real for this app's purposes, and do not design toward it.** What
   exists under that name today is narrow (a specific OAuth flow tied to
   Anthropic's own consumer products, used by a handful of named coding
   tools), undocumented as a general third-party integration surface, and
   demonstrably unstable across 2026's own policy history. If a
   per-conversation cost needs to be attributed to individual users at
   all, the only currently-real mechanism is bring-your-own-API-key — and
   that itself is new, non-trivial scope for this app (Section 2's
   per-user-secret-storage gap), not a quick win either. The more likely
   realistic shape, if this ships at all, is a single **operator-funded**
   API key (the pattern this app already knows how to store, per
   `SSO_CLIENT_SECRET`'s and `railMcp`'s own `secretKeyRef` precedent) —
   which puts the Cost/risk framing above squarely on the table as a real
   decision, not a footnote.

   **Correction (2026-09-02):** unaffected for Option B/A1 — this
   paragraph's finding stands unchanged for any DS-embedded orchestrator.
   It does not, however, apply to Option C at all: a user connecting
   `distant-signal-mcp` directly to their own Claude.ai/Desktop account
   pays through their own existing Claude plan, with no "link your
   Anthropic account to DS" delegation step of any kind — DS never handles
   the user's Anthropic identity or credentials in that shape. If Option C
   ships, this is a materially cheaper and simpler cost story than either
   the operator-funded-API-key path above or the per-user-BYO-API-key gap
   this section flags — see Cost/risk's own correction for the full
   restatement.
3. **Do not route chatbot traffic through the enricher's existing local
   LLM endpoint without first resolving two separate, real blockers**:
   confirmed contention (the enricher calls this endpoint strictly
   serially today, by its own explicit design, with zero existing
   concurrency/priority protection — Section 3a) and confirmed
   uncertainty about tool-calling support (today's only usage is
   structured-output JSON-schema extraction, not the `tools` parameter
   family a chat agent's tool loop needs — Section 3b). Both are
   checkable facts about the actual deployed endpoint, not open-ended
   design questions, and both should be resolved (a live capacity/
   contention test; a direct tool-calling smoke test against the real
   deployed model) before any further design work assumes this path is
   viable at all.
4. **Do not build a general-purpose authorization/role system as a
   prerequisite for this feature.** If access needs gating at all
   (whether for the LLM-endpoint-sharing reason above, or simply to
   soft-launch a costed feature to a subset of users first), the
   allowlist-table shape sketched in Section 3c is proportionate; a full
   role system is not justified by this feature alone and risks becoming
   its own unfinished, parallel-scope project.
5. **If and when this reaches implementation, follow this repository's own
   convention and write a dedicated design-spec pass** (at the depth
   `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md` or
   `2026-08-22-tfl-service-metrics-v2-design.md` gave their own features)
   before any code changes — this document is a landscape survey, not
   that spec, per its own Status line.
6. **(Added 2026-09-02) Treat "should DS build a connect-your-own-Claude
   onramp (Option C)" as a distinct product decision from "should DS build
   an embedded chatbot (Option B)," not a resolution of it.** Both are
   real and viable now that `distant-signal-mcp` is becoming public and
   OIDC-gated. They serve different users (no-Claude-account-needed vs.
   bring-your-own-Claude-client) and have genuinely different cost
   profiles (DS-funded vs. user-funded — Cost/risk's correction). Nothing
   in this document recommends one over the other; if this feature area
   is pursued, whoever writes the eventual design-spec pass (item 5,
   above) should decide explicitly which, if either, to build, rather
   than defaulting to Option B just because it was this document's
   original conclusion.

## Open questions (explicit, not resolved here)

1. **Whether the enricher's actually-currently-deployed model (not just
   the `qwen3.5:4b`/Ollama example cited in a timeout-tuning comment,
   which may not reflect every environment's real configuration) supports
   tool/function calling at all.** Section 3b's single most consequential
   unresolved question — would need a direct smoke test against the real
   endpoint.
2. **Whether Anthropic's consumer-subscription-OAuth mechanism (Section 2)
   stabilizes into a documented, general third-party integration surface
   at some future point.** Its 2026 policy history so far argues against
   relying on it, but this is a snapshot, not a permanent verdict — worth
   re-checking against `platform.claude.com`'s own docs (not third-party
   reporting) if this feature is revisited later.
3. **Actual per-conversation token/cost economics for a real
   `plan_journey`-driving chat session**, needed to size the Cost/risk
   framing section's operator-funded-API-key option concretely — not
   measured this pass (would require a working prototype and real traffic
   or at least a synthetic load estimate).
4. **Whether `distant-signal-mcp`'s tools could be given a
   session-forwarding authenticated variant** to unlock the TRUST-
   corroboration tier for the embedded (same-origin-session) case flagged
   in Section 1 — real opportunity, not designed here.
5. **(Added 2026-09-02) Whether DS's existing third-party OIDC provider
   (the one `crates/api/src/auth/oidc.rs` is already a relying party of)
   can itself serve as the OAuth authorization server Claude's connector
   infrastructure discovers** — i.e. whether it already supports RFC
   8414/OIDC Discovery metadata, PKCE with `S256`, and either Dynamic
   Client Registration or Client ID Metadata Document support — or
   whether `distant-signal-mcp`/DS needs a separate, dedicated
   authorization-server layer in front of or alongside it. This is the
   single most consequential unresolved question raised by the top-level
   Corrections section's OAuth research; it belongs to the sibling design
   doc's own Decision 4, not answered here.
6. **(Added 2026-09-02) Whether DS should build Option C (a connect-your-
   own-Claude onramp) at all, and if so, whether alongside, instead of, or
   before Option B (an embedded DS chatbot).** Recommendation item 6
   names this as a real, open product decision this correction surfaces
   but does not make.

## References

- `claude-api` skill (this session's designated primary source for
  current Claude API/MCP/agent mechanics — model table cached
  2026-06-24; Authentication, Server Tools, and "MCP connector needs both
  halves" Quick Reference sections cited directly above).
- MCP connector documentation, fetched in full this session:
  https://platform.claude.com/docs/en/agents-and-tools/mcp-connector
  (redirected from `docs.claude.com/en/docs/agents-and-tools/mcp-connector`,
  confirming `platform.claude.com` as the current canonical docs host).
- Web search results on Anthropic's 2026 consumer-OAuth/"extra usage
  credits" policy history (Section 2) — third-party reporting, not
  Anthropic's own docs, flagged as such in the text:
  [What Is the OpenClaw Ban?](https://www.mindstudio.ai/blog/anthropic-openclaw-ban-oauth-authentication),
  [Anthropic Bans Claude Subscription OAuth in Third-Party Apps](https://winbuzzer.com/2026/02/19/anthropic-bans-claude-subscription-oauth-in-third-party-apps-xcxwbn/),
  [Anthropic officially bans using subscription authentication for third-party Claude use](https://alternativeto.net/news/2026/2/anthropic-officially-bans-using-subscription-authentication-for-third-party-claude-use),
  [Anthropic Subscription Auth Warning: Third-Party Usage Draws From Extra Usage, Not Your Plan](https://fazm.ai/blog/anthropic-subscription-auth-warning-third-party-subscription-auth-warning-third-party-extra-usage),
  [OpenCode Third-Party Apps Extra Usage: $200 Credit Explained](https://fazm.ai/blog/opencode-third-party-apps-extra-usage-200-credit),
  [Third-Party Apps Now Draw From Your Extra Usage, Not Your Plan Limits](https://fazm.ai/blog/third-party-apps-draw-from-extra-usage-not-plan-limits).
- This repository, read directly this session: `crates/api/src/auth.rs`,
  `crates/api/src/routes/auth.rs`, `crates/api/src/data/users.rs`,
  `crates/enricher/src/config.rs`, `crates/enricher/src/llm.rs`,
  `crates/enricher/src/stream.rs`, `crates/enricher/src/main.rs`,
  `frontend/app/api/[...path]/route.ts`, `frontend/components/AutoRefresh.tsx`,
  `frontend/components/TrackTrainForm.tsx`,
  `charts/distant-signal/templates/railmcp-service.yaml`,
  `charts/distant-signal/templates/railmcp-deployment.yaml`,
  `charts/distant-signal/values.yaml`, `docker-compose.yml`.
- `/workspaces/distant-signal-mcp`, read directly this session:
  `README.md`, `package.json`, `src/server.ts`, `src/ds/annotateLeg.ts`,
  `src/tools/plan-journey.ts`, `src/tools/rendering.ts`.
- `docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`,
  `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`,
  `docs/superpowers/plans/2026-09-01-train-mcp-integration.md` (this
  session's prior work, treated as source of truth for
  `distant-signal-mcp`'s design decisions, re-verified against real code
  where load-bearing for this document's own claims).
- `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
  (this document's structural template).

### Added for the 2026-09-02 Corrections

- `platform.claude.com/docs/en/agents-and-tools/mcp-connector`, re-fetched
  in full this session (2026-09-02) to confirm the "must be publicly
  exposed through HTTP" and "not covered by ZDR" wording is unchanged
  from the original research pass, and to re-cite the "Authentication"
  section's `authorization_token`/OAuth-flow-is-the-caller's-responsibility
  language.
- `claude.com/docs/connectors/building/authentication`, fetched in full
  this session (2026-09-02) — Anthropic's current first-party reference
  for how a remote MCP server authenticates callers for Claude.ai/Claude
  Desktop custom connectors: the `oauth_dcr`/`oauth_cimd`/
  `oauth_anthropic_creds`/`static_headers`/`none` authentication-type
  table, the "every connection requires user consent" (no pure
  `client_credentials`) rule, cross-host authorization server support,
  mandatory PKCE `S256`, DCR-vs-CIMD guidance, the `401` +
  `WWW-Authenticate: Bearer resource_metadata=...` handshake, and the
  `160.79.104.0/21` Anthropic egress range.
- `support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp`,
  fetched in full this session (2026-09-02) — confirms the Claude.ai/Desktop
  custom-connector UI flow (Customize > Connectors > "+" > "Add custom
  connector"), Free/Pro/Max/Team/Enterprise tier support, that
  "conversations occur entirely within Claude.ai's standard interface,"
  and that usage is billed within the connecting user's own Claude plan.
- `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
  revised Decision 4 (Auth) and Decision 6 (Deployment) — being updated by
  a sibling task in this same session to design `distant-signal-mcp`'s
  public, OIDC-gated deployment concretely. Cross-referenced by path only;
  not read in its updated form as part of this correction, per the task's
  own instruction not to block on it.
