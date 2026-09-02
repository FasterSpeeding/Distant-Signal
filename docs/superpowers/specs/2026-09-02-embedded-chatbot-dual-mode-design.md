# Embedded Chatbot, Dual Mode: DS-Hosted Orchestrator (B) and Connect-Your-Own-Claude (C) — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` (decisions
with real alternatives weighed and rejected, an explicit "out of scope"
ledger) and `docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(section conventions: dated "Corrections" for revisiting prior decisions,
"Current relevant state" cited to real code, "Open questions/risks" as a
first-class section). No implementation plan is included — that is a
separate, later step in this repo's process.

## Corrections / relationship to prior specs

This document is not a from-scratch design — it's the point where two
independent research/design threads in this same session are told to
converge, per explicit direction (this document was scoped, not
self-initiated, to design **both** shapes together rather than picking
one).

- **`docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`**
  (hereafter "the chatbot research doc"), including its own
  **`## Corrections (2026-09-02)`** section (read in full this session from
  `main`, at commit `3becd14` — this document's own worktree branch predates
  that merge and does not carry the correction on disk; its content was
  read via `git show main:docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`,
  not from a stale on-disk copy). That correction is the direct origin of
  this document's Option B/Option C framing: it re-opened the original
  research's Option A/B analysis after `distant-signal-mcp` was redirected
  toward becoming public and OAuth-gated, reaffirmed Option B (a DS-hosted
  orchestrator) over the API-level `mcp_servers` connector (renamed "Option
  A1" in the correction) for reasons independent of public reachability,
  and identified — for the first time — **Option C**, a user connecting
  `distant-signal-mcp` directly to their own Claude.ai/Claude Desktop
  account, as a distinct, viable, unrecommended-neither-way product
  decision. This document is the "dedicated design-spec pass" that
  correction's own Recommendation item 5 called for, addressing item 6's
  explicit instruction not to default to Option B just because it was the
  original document's conclusion — designing both, as directed.
- **`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`**,
  read in full this session (on this branch, i.e. pre-revision). Its
  Decision 4 (Auth: anonymous HTTP client, `distant-signal-mcp` calls DS
  anonymously) and Decision 6 (Deployment: `ClusterIP`-only, `railMcp.enabled:
  false` by default, Discord-OAuth-gated) describe `distant-signal-mcp`'s
  **current, still-on-`main`** shape. **A sibling task in this same session
  is concurrently revising that document's own Decision 4 and Decision 6**
  to redesign `distant-signal-mcp` as a publicly-reachable OAuth 2.1
  resource server, gated behind Distant Signal's own Authentik-backed OIDC.
  That revision has **not landed on `main`** as of this session (confirmed:
  `git merge-base --is-ancestor 7643b5f main` reports `main lacks 7643b5f`,
  where `7643b5f` is that sibling task's "Corrections: reverse train-mcp
  Decisions 4/6 to public + DS-OIDC-gated" commit, present only on a
  separate, unmerged worktree branch). Per this task's own instruction,
  this document does **not** read that unmerged branch's content and does
  **not** wait for it — it works from the description given: *"the MCP
  server is being redesigned as an OAuth 2.1 resource server, publicly
  reachable, with Distant Signal's own Authentik-backed OIDC provider as
  (or federated with) the authorization server issuing per-user tokens."*
  Every claim below that depends on that revision's *concrete* mechanics
  (exact Ingress shape, exact token-validation code, exact chart values) is
  cross-referenced to that document by path, not duplicated or guessed.

## Goal

Design one shared authentication/authorization foundation that lets a user,
in **either** of two flows, end up with `distant-signal-mcp` acting on
their behalf with correct Distant Signal (DS) identity — then design each
flow's own additional pieces on top of that shared foundation:

- **Option B**: a DS-hosted chat orchestrator, embedded as a new route in
  `frontend/`, acting as its own MCP client calling `distant-signal-mcp`
  directly. DS's own Anthropic API key pays for every conversation.
- **Option C**: an end user connects `distant-signal-mcp` directly to their
  own Claude.ai/Claude Desktop account via Anthropic's native
  custom-connector feature. The conversation happens entirely inside
  Claude's own UI. Billing lands on the user's own Claude plan.

The document also produces a shared-vs-separate breakdown, a sequencing
recommendation, and an explicit out-of-scope ledger.

## Current relevant state (verified 2026-09-02)

### `distant-signal-mcp`'s current (pre-sibling-revision) deployment shape

Read directly this session, on this branch:

- `charts/distant-signal/templates/railmcp-service.yaml:1-8` — `ClusterIP`
  only, own top comment: *"no external Ingress/TLS is sketched here... An
  operator who wants to expose this externally fronts it with their own
  Ingress/LoadBalancer."*
- `charts/distant-signal/values.yaml:767-772` — `railMcp.enabled: false` by
  default ("Opt-in... matching `scheduleFeed.enabled`'s own pattern").
- `charts/distant-signal/values.yaml:783-798` — `railMcp.publicUrl` exists
  as a values field already (blank by default, "an operator enabling
  railMcp must set this"), and the block's own comment already frames the
  Discord client id/allowlist as *"the OAuth resource-server verification
  checks tokens against"* — i.e. even today's shipped chart already treats
  `distant-signal-mcp`'s existing Discord gate as *a* resource-server-style
  check, just not a Claude-connector-shaped one. This is the baseline the
  sibling revision replaces, not a green field.

### DS's own OIDC identity system — a relying party, not an authorization server

`crates/api/src/auth/oidc.rs`, read in full this session:

- DS's `crates/api` is an OIDC **relying party** against an external
  provider (Authentik, per `charts/distant-signal/values.yaml:409-415`'s
  `devAuthentik.image.repository: ghcr.io/goauthentik/server`, tag
  `2026.8.0` — cited via the train-mcp design doc's own re-verification,
  not independently re-read this session since it's not load-bearing
  beyond confirming which product is DS's IdP). `OidcClient::new`
  (`oidc.rs:108-129`) performs lazy OIDC discovery against `issuer_url`
  and drives an authorization-code+PKCE flow (`authorize_url`,
  `oidc.rs:156-166`; `exchange_code`, `oidc.rs:178-211`) — DS's backend is
  a **client** of Authentik, not itself an authorization server.
- `oidc.rs:172-177`'s own doc comment states DS's callback handler
  *"deliberately drops"* the refresh token returned by Authentik —
  *"nothing implements silent renewal yet"* — confirming DS holds no
  long-lived, reusable-on-a-user's-behalf token from Authentik today, for
  any purpose.
- `crates/api/src/auth.rs:63-64` — the session cookie DS mints after this
  exchange is `distant_signal_session` (`SESSION_COOKIE_NAME`), read by
  `AuthenticatedUser`'s extractor (`auth.rs:182-196`, confirmed read this
  session), whose resolved struct (`auth.rs:176-180`) is `{ id, email,
  name }` — **no role, tier, or permission field of any kind**, confirmed
  directly by re-reading the struct definition this session (matching the
  chatbot research doc's own Section 3c finding, re-verified rather than
  merely cited).

### The internal-service-accounts design is service-scoped, not user-scoped — by explicit, stated choice

`docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md`,
read in full this session. Its own Decision 2 chooses "a static,
code-defined table (service → allowed route prefixes)... not a DB-backed
ACL table," specifically because the caller set is *"small... and changes
only when a new service is written and deployed"* — a fixed, small,
deploy-time-known enumeration (`InternalService::{PollerIncidents,
PollerStations, PollerTocs, PollerLdbws, PollerTfl, TrustConsumer,
ScheduleIngest}`, per that doc's own Decision 2), with `require_internal_token`
running with **zero database dependency** ahead of any route handler. Its
own Decision 4 addresses the adjacent question directly and answers it
"no": *"The prompt for this spec explicitly asks whether the same 'named
credential → allowed route set' mechanism could serve both this feature
and a future human-user roles/permissions feature... keep them separate...
Service identities are static and deploy-time... Human roles... are
inherently dynamic — assignable to an existing `users` row at runtime...
The two features share a *concept*... but not an *implementation*."* This
is the exact fork this document's own Decision 1 has to resolve for Option
B's orchestrator — see below.

### No streaming precedent anywhere in this app today

`grep -rl "EventSource\|WebSocket\|text/event-stream\|Sse\b" crates
frontend` (run this session, both `.rs` and `.ts`/`.tsx`) returns **zero
matches**. Every existing dynamic surface in this app is either
periodically polled (`frontend/components/AutoRefresh.tsx`, `router.refresh()`
every 30s, no per-route opt-out) or a plain request/response fetch. A
chat feature's streaming transport (Option B only — Option C's streaming,
if any, happens entirely inside Claude's own client, outside this app) is
genuinely new infrastructure for this codebase, not an extension of an
existing pattern.

### No account/settings route exists today

`find frontend/app -maxdepth 2 -type d` (run this session): `api/`,
`incidents/[id]/`, `lines/[id]/`, `stations/[crs]/`, `track/mine/`,
`train/[uid]/`, `train/by-id/`. No `account/`, `settings/`, `connect/`, or
similarly-named route exists (confirmed by a direct `find` for those
names, zero results). Whatever Option C needs to expose lands as an
entirely new top-level route, not an addition to an existing settings
surface — there isn't one.

### The same-origin proxy pattern DS already uses for every authenticated browser call

`frontend/app/api/[...path]/route.ts:1-33`, read this session: browser
Client Components cannot read server-only env vars (`API_BASE_URL`), so
every authenticated mutation/read goes through a same-origin Next.js proxy
that forwards the `Cookie` header both directions (`route.ts:11-14`'s own
comment: *"the incoming `Cookie` header must reach `api`... every
`Set-Cookie` `api` sends back must reach the browser unmodified"*). This
is the existing mechanism by which any new DS-side surface — a chat
endpoint included — would carry a logged-in user's session forward,
without inventing a new cookie-forwarding mechanism.

### Authentik's own consent self-service — partially confirmed, not fully

Fetched this session, `docs.goauthentik.io`'s consent-stage documentation:
confirms *"Users can check their past consent approvals via the User
Settings menu"* and that Authentik *"keeps track of the user, the
application, and the granted permissions"* when a consent is stored. **Not
confirmed by what was fetched**: whether that same User Settings view lets
a user actively **revoke** a previously-granted consent themselves, as
opposed to merely viewing it — a second fetch targeting Authentik's
general user-settings overview page found no mention of "consent,"
"revoke," "authorized applications," or "connected apps" at all. Treated
here as a real, citable partial fact (viewing exists) plus a genuine,
unresolved gap (revocation is unconfirmed), not assumed either way — see
Decision 6 and Open questions/risks.

## Decisions

### 1. Shared foundation: both B and C authenticate to `distant-signal-mcp` via the same per-user OAuth mechanism — the internal-service-accounts pattern does not apply here

**This is the single most important decision in this document.** The
question, stated precisely: when DS's own backend orchestrator (Option B)
calls `distant-signal-mcp` mid-conversation on behalf of a specific,
already-logged-in DS user, how does it end up with a credential that
`distant-signal-mcp`'s new resource-server validation will accept **and**
that correctly identifies *that specific user* — not merely "a trusted DS
component"?

Two real shapes were weighed:

- **Reuse the internal-service-accounts mechanism** (a per-service bearer
  token, checked against a static, code-defined `InternalService` table,
  bypassing OAuth entirely for this internal call). **Rejected, for a
  reason that is structural, not a style preference: that mechanism has no
  user dimension at all, by its own design's explicit choice.**
  `InternalService`'s token→identity lookup resolves to one of a handful
  of fixed, deploy-time-known *service* identities (`PollerLdbws`,
  `TrustConsumer`, etc.) — it was never built to, and per that design's own
  Decision 4, was deliberately **not** extended to carry a *per-request
  end-user identity* on top of a service identity, because doing so would
  require exactly the dynamic, DB-backed, request-time lookup that design
  rejected for a completely different, valid reason (keeping
  `require_internal_token` free of a database round trip on `poller-ldbws`'s
  60-second-interval hot path). Reusing it for Option B would force one of
  two bad outcomes: (a) `distant-signal-mcp` treats every DS-orchestrator
  call as "the same anonymous trusted caller," which throws away the exact
  per-user identity the whole point of redesigning `distant-signal-mcp`
  around per-user OAuth tokens exists to provide (no TRUST-corroboration
  tier, no per-user rate limiting, no way to answer "which user is this
  conversation for" from `distant-signal-mcp`'s own side at all); or (b)
  someone bolts a dynamic, per-user extension onto a mechanism whose own
  design doc explicitly declined to build exactly that generalization
  ("the two features share a *concept*... but not an *implementation*").
  Neither outcome is acceptable, and both are avoidable — this is a
  genuinely different, orthogonal axis (which *service* is calling vs.
  which *end user* a call is on behalf of), not a case the existing
  mechanism was ever meant to cover.
- **Both B and C obtain a legitimately-scoped, per-user OAuth access token
  from the same authorization server `distant-signal-mcp` trusts, and
  present it as a normal bearer token — the same validation path on
  `distant-signal-mcp`'s side either way.** **Chosen.** `distant-signal-mcp`
  becomes, per the sibling revision, a uniform OAuth 2.1 resource server:
  it doesn't (and per the corrected research doc's own finding, Claude's
  connector infrastructure explicitly won't accept a design that does)
  distinguish "public untrusted caller" from "DS's own internal caller" by
  network origin or by a separate credential type — every caller presents
  a bearer token, and every bearer token is validated the same way
  (signature, issuer, audience, expiry, scope). This is exactly the shape
  the corrected research doc's own "Corrections" section already
  identified as newly available once `distant-signal-mcp` requires
  per-caller OAuth tokens anyway: *"a DS-hosted Option B orchestrator —
  which already knows which DS user it's acting for — is a natural,
  already-privileged position to mint or forward a correctly-scoped access
  token for that same user, rather than calling anonymously."*

**How B's orchestrator actually obtains that token, concretely, without
re-running Claude's own interactive flow:** the orchestrator does not need
the full human-facing browser-redirect-and-consent round trip Option C's
Claude client performs, because it is not a genuinely external client —
it already has a live, first-party DS session for the exact user it's
serving, carried forward the same way every other authenticated call in
this app already reaches DS's backend (the `frontend/app/api/[...path]/route.ts`
cookie-forwarding proxy pattern, Current relevant state above). Concretely:
the orchestrator is registered as its **own** confidential OAuth client
against Authentik (a second registration, alongside DS's existing
relying-party client `oidc.rs` already drives — the same operational
pattern `SSO_CLIENT_SECRET`'s `secretKeyRef` already establishes,
`charts/distant-signal/templates/api-deployment.yaml:132-136`), and
exchanges proof of the user's already-established DS session for a token
scoped to `distant-signal-mcp`'s audience via a server-side, non-interactive
grant — the standard shape for this is OAuth token exchange (RFC 8693,
"I already hold valid proof of this user's identity; issue me a new token
for a different audience"), though a first-party-trusted
authorization-code variant that skips the consent screen for an
Authentik-recognized first-party application is also plausible depending
on what Authentik itself supports.

**This exact mechanic — which grant Authentik actually supports for this
purpose — is explicitly not resolved here.** It depends on Authentik's own
capabilities (the same open question the corrected research doc's own Open
Question 5 already flagged for the DCR/CIMD/PKCE-S256/OIDC-Discovery
question, extended here to token exchange specifically), and it belongs to
the sibling design doc's own Decision 4, not this document — cross-referenced
by path, not guessed at. What **is** decided here, and load-bearing for
everything else in this document, is the *shape*: both options end up
presenting `distant-signal-mcp` with a real, per-user-scoped OAuth bearer
token from the same authorization server, obtained via different grant
paths appropriate to how privileged/first-party each caller is (interactive
consent for a genuinely external client in C; a trusted, non-interactive
server-side exchange for DS's own already-authenticated first-party
orchestrator in B) — never the internal-service-accounts mechanism, and
never an unscoped "DS backend, no specific user" credential.

### 2. Option B's orchestrator: a separate, still-internal-only TypeScript service — not folded into `distant-signal-mcp` itself, not a new Rust crate

Re-affirms the chatbot research doc's own finding (unchanged by the
public-exposure correction, per that correction's own "What this does and
doesn't change" subsection): TypeScript is the natural implementation
language, because Anthropic's own TypeScript SDK ships first-class MCP
client helpers (`mcpTools()` + `client.beta.messages.toolRunner()`, per
the research doc's citation of the fetched MCP connector page) with no
Rust equivalent documented anywhere Anthropic's own docs enumerate
per-language support.

**Where exactly, though, is a question the public-exposure correction
newly changes the answer to.** The original research sketched the
orchestrator as *"either a new module inside `distant-signal-mcp`'s own
fork... or a new, small sibling TypeScript service"* — a genuine
either/or, at the time, because `distant-signal-mcp` was internal-only
either way, so co-locating the orchestrator inside it cost nothing extra
in exposure. **That's no longer true.** `distant-signal-mcp` is being
redesigned specifically to accept traffic from the public internet (gated
by OAuth, but still publicly reachable — the sibling doc's own Decision 6
territory). DS's own Anthropic API key — the credential that directly
spends real money per conversation — is a fundamentally different class of
secret from `distant-signal-mcp`'s existing Discord/LDBWS credentials: it
is spent by the mere act of the orchestrator making a call, with no
per-caller allowlist check happening on Anthropic's side the way Discord's
`DISCORD_ALLOWED_USER_IDS` gate exists today. **Chosen: keep the
orchestrator as its own service, deployed `ClusterIP`-only exactly the way
`distant-signal-mcp` itself was until this revision** — its only inbound
traffic is same-origin, cookie-forwarded requests relayed through
`frontend/app/api/[...path]/route.ts` (Option B never needs the
orchestrator to be reachable from the public internet at all; only
`distant-signal-mcp` does, and only for Option C's benefit). This keeps
the Anthropic API key out of the one process in this deployment that
now has to weather arbitrary public internet traffic, a real security-boundary
reason, not merely a style preference: a vulnerability in the public-facing
`distant-signal-mcp` process (SSRF, an auth-bypass bug, anything) would not,
under this split, also hand an attacker DS's own paid Anthropic credential.
The orchestrator calls `distant-signal-mcp` over the same internal network
`distant-signal-mcp` already runs its own tool-calling loop against today
— no protocol translation, no new exposure for the orchestrator itself.

### 3. Option B's frontend route: `frontend/app/chat/`, a new top-level route, with a "track this leg" deep-link into `TrackTrainForm`

Reaffirms the chatbot research doc's own Section 4 finding, unaffected by
the public-exposure correction (that correction's own "does Option C
change this section's assumption" subsection explicitly confirms the
placement/`TrackTrainForm`-relation/`structuredContent`-reuse analysis is
Option-B-specific and stands unchanged). `frontend/app/` follows a
one-top-level-route-per-concern pattern (Current relevant state, above) —
a new `frontend/app/chat/` route fits it; a persistent floating widget
mounted in the root layout would be this app's first such element and is
a bigger UX commitment this document doesn't take on.

`plan_journey`'s typed `structuredContent` (`RenderedTrainLeg`'s `uid`/
`departureAt`, per the research doc's citation of the fork's own
`plan-journey.ts:1155-1188`) is the reusable surface for a "track this
leg" affordance that deep-links into `TrackTrainForm`, pre-filled the same
way `initialOrigin` already supports the existing "Track a train from
here" station-page shortcut (`TrackTrainForm.tsx:18-21`, per the research
doc). Not designed further here — sketched at the same depth the research
doc left it, per that document's own "Explicitly out of scope."

### 4. Option B's streaming transport: SSE over a new same-origin proxy route — genuinely new infrastructure, no existing pattern extended

Confirmed above: nothing in this app streams today (zero matches for
`EventSource`/`WebSocket`/`text/event-stream`/`Sse` across `crates` and
`frontend`). SSE is the natural fit over WebSocket: it's one-directional
(orchestrator → browser; the user's own turn is a plain request), matches
the shape Anthropic's own Messages API streaming already uses, and layers
onto the existing same-origin-proxy pattern (`frontend/app/api/[...path]/route.ts`)
as a chunked/streamed response rather than requiring a second, stateful
connection type this app has never operated. **Exact wire framing
(reconnect semantics, exact proxy route shape, how far the existing
catch-all proxy vs. a dedicated `frontend/app/api/chat/route.ts` handles
this) is not designed here** — carried forward as explicitly out of scope,
matching the research doc's own exclusion for the same reason: it's
implementation detail, not an architectural fork.

### 5. Option B's cost/access gating: the allowlist shape, not a role system — reaffirmed, now grounded in a direct re-read

The chatbot research doc's Section 3c and ranked Recommendation item 4
already concluded this app has no role/permission concept at all and that
a full RBAC system is disproportionate scope for gating one costed
feature. This document's own Current relevant state re-confirms the
underlying fact directly (`AuthenticatedUser { id, email, name }`, no role
field, re-read this session, not merely cited) rather than trusting the
citation secondhand. **Chosen, reaffirmed: a small, hand-curated allowlist**
(a `chatbot_allowed_users(user_id)` table or equivalent, checked by a new
extractor wrapping `AuthenticatedUser` with one more lookup — the same
shape the research doc sketched) gates who may open `frontend/app/chat/`
and drive orchestrator traffic, proportionate to a feature an operator
wants to soft-launch and budget for, not a general product-tier system.
This is DS-side, application-level gating, layered **on top of** the
Decision 1 per-user OAuth token — the token proves *who* the user is to
`distant-signal-mcp`; the allowlist decides *whether that specific user is
allowed to trigger DS-funded Anthropic spend at all*, a question
`distant-signal-mcp`'s own OAuth gate (built for Option C's arbitrary
Claude users too) has no reason to answer on Option B's behalf.

### 6. Option C's DS-side additions: a short connect/instructions route, no bespoke connector-management UI

What Option C needs from `frontend/` is deliberately smaller than Option
B's chat surface, per the corrected research doc's own finding: *"If
Option C ships... there is no chat UI inside Distant Signal's own frontend
for that path at all... conversations occur entirely within Claude.ai's
standard interface."* **Chosen: a new, small top-level route** (e.g.
`frontend/app/connect-claude/`, following the same one-route-per-concern
pattern Decision 3 uses for Option B) containing: the connector URL for
`distant-signal-mcp` (populated once the sibling design's Decision 6
assigns it a real public origin — cross-referenced, not invented here),
and static instructions mirroring the documented Claude.ai flow (Customize
> Connectors > "+" > "Add custom connector," per the corrected research
doc's citation of Anthropic's Help Center article) — plausibly gated
behind DS's own login the same way any other authenticated route is,
since a logged-out visitor has no DS identity to connect to in the first
place.

**Chosen: do not build a bespoke DS-side "manage your connected app /
revoke access" view for this pass.** Authentik's own consent stage already
tracks *"the user, the application, and the granted permissions"* and
exposes past consent approvals via its own User Settings menu (Current
relevant state, above — confirmed, not assumed). Building a second,
DS-authored view over the same underlying consent record Authentik already
owns would duplicate state DS doesn't control the source of truth for.
**This is a partial, not a full, answer**: this session's own fetches
confirmed *viewing* past consents is real but did **not** confirm
Authentik's own UI lets a user *revoke* one themselves — flagged plainly in
Open questions/risks, not assumed favorably just because it would be
convenient for this decision. If Authentik turns out not to support
self-service revocation, the fallback is operator-mediated (an admin
revokes via Authentik's own admin UI on request) — not designed further
here, and not blocking this decision, since it doesn't change what DS
itself needs to build (nothing, either way, for this pass).

## Shared vs. separate — breakdown

| Piece of work | Shared (B + C) | B-only | C-only |
|---|---|---|---|
| `distant-signal-mcp` redesigned as a public OAuth 2.1 resource server (401 + `WWW-Authenticate` handshake, Protected Resource Metadata, bearer-token validation) | ✔ | | |
| Authentik configured as (or federated with) the authorization server: OIDC Discovery, PKCE `S256`, DCR/CIMD support (sibling Decision 4 territory) | ✔ | | |
| `distant-signal-mcp` public network exposure (Ingress/TLS, egress-IP-allowlisting notes) (sibling Decision 6 territory) | ✔ | | |
| NRE/Network-Rail attribution question for MCP tool-rendered output (still unresolved, applies to any tool consumer) | ✔ | | |
| Per-user OAuth token acquisition mechanism *design* (Decision 1's shape: same authorization server, different grant paths) | ✔ (shape) | (B's own non-interactive grant/client registration) | (C's own interactive Claude-driven grant — no DS code) |
| Backend orchestrator service (Anthropic SDK tool-calling loop, `mcpTools()`/`toolRunner()`) | | ✔ | |
| DS's own Anthropic API key, secret provisioning | | ✔ | |
| `frontend/app/chat/` route + chat UI | | ✔ | |
| SSE streaming transport + new proxy route | | ✔ | |
| `chatbot_allowed_users` cost/access-gating allowlist | | ✔ | |
| "Track this leg" deep-link into `TrackTrainForm` | | ✔ | |
| `frontend/app/connect-claude/` onboarding route + instructions copy | | | ✔ |
| Reliance on Authentik's own consent-history UI (not built by DS) | | | ✔ |

## Sequencing recommendation

**Build the shared foundation once, ship Option C first, treat Option B as
a genuinely separate, larger follow-on — not two equally-weighted parallel
tracks.** Three concrete reasons, not a default preference:

1. **The riskiest, least-verified part of this whole document is whether
   Authentik actually supports what Claude's connector infrastructure and
   Option B's own non-interactive grant both need** (OIDC Discovery, PKCE
   `S256`, DCR/CIMD, and — per Decision 1 — some form of token exchange or
   trusted-first-party grant). Option C is the cheapest, most direct way to
   validate that the shared foundation actually works end-to-end against a
   real, independent external client (Claude itself) — if Authentik can't
   satisfy Claude's connector requirements, that's a foundation-level
   problem no amount of Option B UI work fixes, and it's far cheaper to
   discover that before building an orchestrator, a chat route, and a
   streaming transport on top of an unverified assumption.
2. **Option C's own DS-side surface is genuinely thin relative to Option
   B's**, per the breakdown above: one small instructional route, no
   orchestrator, no Anthropic API key to provision or protect, no
   streaming transport, no cost/access-gating design. Shipping it doesn't
   require Decision 2 through 5 above to be resolved at all.
3. **The cost stories are not comparable, and Option C is strictly
   cheaper to operate**: per the corrected research doc's own restatement,
   Option C's per-conversation LLM spend lands entirely on the connecting
   user's own Claude plan — Distant Signal pays $0 in Anthropic API costs
   for it. Option B's ongoing, usage-scaling Anthropic bill is a real
   operator budget decision this document doesn't make (and per Decision 5,
   needs its own access-gating before it ships at all). Shipping the
   cheaper, foundation-validating option first lets that budget decision
   happen on its own timeline, not as a blocker to shipping anything.

This does not mean Option B shouldn't be built — the corrected research
doc's own Recommendation item 6 is explicit that both are real, and this
document does not pick one to the exclusion of the other. It means the
natural order, given they share a foundation whose riskiest part is best
proven by the thinner consumer, is foundation + C, then B as its own,
separately-scoped follow-on design/implementation pass once the foundation
is confirmed working in production against a real external client.

## Architecture

```
                                   Authentik (DS's own OIDC provider,
                                   ghcr.io/goauthentik/server:2026.8.0)
                                   ── authorization server for BOTH flows ──
                                            │
                    issues per-user, distant-signal-mcp-scoped
                    OAuth access tokens, via two different grant paths:
                            │                              │
      ┌─────────────────────┘                              └─────────────────────┐
      │ interactive consent                                 │ non-interactive,
      │ (genuinely external client)                          │ server-side exchange
      │                                                       │ (first-party, already
      ▼                                                       │  holds a live DS session)
┌───────────────────────┐                          ┌──────────────────────────────┐
│ OPTION C               │                          │ OPTION B                     │
│ Claude.ai / Claude      │                          │                                │
│ Desktop (user's own     │                          │  frontend/app/chat/  (NEW)    │
│ account)                │                          │       │                        │
│                         │                          │       │ same-origin, cookie-   │
│ Customize > Connectors  │                          │       │ forwarded, per         │
│ > "Add custom connector"│                          │       │ frontend/app/api/      │
│ (user pastes DS's       │                          │       │ [...path]/route.ts     │
│  connector URL --       │                          │       ▼                        │
│  frontend/app/          │                          │  orchestrator (NEW, separate  │
│  connect-claude/ tells  │                          │  TS service, ClusterIP-only,  │
│  them how)              │                          │  holds DS's Anthropic API key)│
│                         │                          │   - chatbot_allowed_users     │
│ conversation happens    │                          │     gate (Decision 5)         │
│ entirely inside Claude's│                          │   - Anthropic Messages API,   │
│ own UI                  │                          │     tool_use loop             │
│                         │                          │   - SSE stream back to        │
│ billed to the user's    │                          │     frontend/app/chat/        │
│ own Claude plan         │                          │   - billed to DS's own        │
│                         │                          │     Anthropic API key         │
└───────────┬─────────────┘                          └───────────────┬───────────────┘
            │ bearer token                                            │ bearer token
            │ (Claude's own OAuth client)                             │ (orchestrator's own
            │                                                          │  OAuth client)
            ▼                                                          ▼
┌───────────────────────────────────────────────────────────────────────────────┐
│ distant-signal-mcp -- SHARED FOUNDATION                                         │
│ publicly reachable (sibling Decision 6), OAuth 2.1 resource server (sibling     │
│ Decision 4): validates every bearer token the same way regardless of caller     │
│  - 401 + WWW-Authenticate: Bearer resource_metadata=... handshake               │
│  - Protected Resource Metadata (RFC 9728) points at Authentik                   │
│  - resolve_station / get_departures / get_arrivals / get_service_detail /       │
│    find_services / plan_journey  -- same six tools, same code, for both flows   │
└──────────────────────────────────┬──────────────────────────────────────────────┘
                                    │ anonymous HTTP (Decision 4 of the sibling doc,
                                    │ unchanged by this document)
                                    ▼
                         Distant Signal `api` (crates/api)
                         /public/*, /Line/*/Status
```

## Error handling

- **Token acquisition failure (either flow) is a hard failure at the
  authorization-server boundary, not something either `distant-signal-mcp`
  or the orchestrator can paper over** — if Authentik rejects the
  authorization-code exchange (Option C) or the orchestrator's own
  non-interactive exchange (Option B), the caller never reaches
  `distant-signal-mcp`'s tools at all. For Option C this surfaces inside
  Claude's own UI (out of DS's control, per that flow's own shape). For
  Option B, the orchestrator returns an error the chat UI renders inline —
  not designed at wire-protocol depth here (Decision 4).
- **`distant-signal-mcp`'s own tool-level error handling** (a DS API
  timeout/5xx during `plan_journey`'s annotation loop, an ambiguous line
  match, etc.) is entirely unchanged by this document — it's the sibling
  train-mcp design's own Error handling section's territory
  (`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`'s
  Error handling), reused as-is by both B and C since both call the same
  tools.
- **Orchestrator-side failure (Option B only)**: an Anthropic API error
  (rate limit, content policy, timeout) during the tool-calling loop
  surfaces to the chat UI as a failed/partial conversation turn, not a
  crash of the whole `frontend/app/chat/` page — matching this app's
  existing posture of scoping a failure to the smallest unit that
  actually failed (e.g. the chatbot research doc's own citation of
  `eta_blend.rs`'s "deliberately NOT a guaranteed join" posture, reused
  here by analogy, not by direct code reuse).
- **`chatbot_allowed_users` rejection (Option B only)**: a logged-in-but-not-allowlisted
  user hitting `frontend/app/chat/` gets a plain "not available for your
  account" state, not a `404` — this is not an ownership check over a row
  whose existence should be hidden (the feature's existence is not a
  secret), so the `404`-for-both convention this app uses for ownership
  checks (`crates/api/src/routes/train.rs`'s own stated convention, per
  the internal-service-accounts design's Decision 3 discussion of the same
  distinction) does not apply here, matching that same design's own
  reasoning for why a different, more informative status is appropriate
  when the caller isn't an untrusted party probing for a secret resource.

## Testing

- **Decision 1 (shared foundation)**: not independently testable by this
  document alone — the actual token-validation/exchange logic lives in
  `distant-signal-mcp` and Authentik, neither owned by this repo's own test
  suite. What belongs to this repo: a smoke test (once the orchestrator
  exists) confirming it never falls back to an unscoped/anonymous call to
  `distant-signal-mcp` if its own token exchange fails — mirroring the
  train-mcp design's own "Auth" test (*"a smoke test confirming every DS
  call this service makes carries no `Authorization`/session cookie
  header"*) in spirit, inverted: here the assertion is that a call is
  *never* made without a correctly-scoped token, not that one is never made
  with one.
- **Decision 5 (allowlist)**: a table-driven test analogous to the
  internal-service-accounts design's own scope-enforcement test — an
  allowlisted user's session passes the `frontend/app/chat/` gate, a
  non-allowlisted logged-in user's does not, an anonymous session does
  not (no separate `getSession()` call needed if the gate is layered
  directly on `AuthenticatedUser`, matching the pattern the tracked-trains
  home-page design already established for "no separate session check
  needed" reasoning).
- **Decision 3's `TrackTrainForm` deep-link**: a fixture test confirming a
  `plan_journey`-shaped `structuredContent` leg correctly populates
  `TrackTrainForm`'s `initialOrigin`-equivalent prefill, mirroring the
  existing "Track a train from here" station-page shortcut's own test
  coverage (not independently verified this session — implementation-time
  concern).
- Nothing in this document proposes changes to `distant-signal-mcp`'s own
  tool logic or DS's Rust backend routes, so neither gets new tests from
  this pass beyond what's listed above; the sibling train-mcp design's own
  Testing section already covers the tool-level behavior both flows share.

## Explicitly out of scope

- **The exact OAuth grant/token-exchange mechanics for Decision 1's
  non-interactive path** (RFC 8693 vs. a trusted-first-party
  authorization-code variant vs. something Authentik-specific), and
  whether Authentik supports DCR/CIMD/PKCE `S256`/OIDC Discovery at all —
  belongs to the sibling design's own Decision 4, cross-referenced, not
  designed here.
- **`distant-signal-mcp`'s own Ingress/TLS/egress-IP-allowlisting
  deployment shape** — sibling Decision 6 territory.
- **Exact streaming wire protocol** (SSE framing, reconnect semantics,
  exact proxy route shape) for Option B — flagged as needed (Decision 4),
  not designed, matching the original research's own exclusion.
- **A concrete `frontend/app/chat/` component tree, exact orchestrator API
  surface, or system-prompt design.** Decision 3 sketches placement and
  reuse opportunities only.
- **A concrete `frontend/app/connect-claude/` page's exact copy/layout.**
  Decision 6 sketches content requirements only.
- **Full RBAC/role-tier system.** Decision 5 explicitly chooses the
  allowlist shape instead; a general permission-tier system remains a
  separate, independent product decision.
- **A rich, DS-authored connector-management UI** (viewing/revoking a
  connected Claude app from inside DS's own frontend) beyond what
  Authentik's own consent view already provides. Decision 6 explicitly
  declines to build this for this pass.
- **Mobile-specific chat UX** for Option B. Not addressed anywhere in this
  document; a later concern if Option B ships.
- **BYO-API-key / per-user Anthropic-credential storage in DS.** Already
  characterized by the chatbot research doc's Section 2 as real,
  non-trivial, unprecedented scope; unaffected by this document and not
  designed here.
- **Realizing the TRUST-corroboration tier** (`annotateLeg.ts`'s narrow
  per-leg case, per the sibling train-mcp design's Decision 3b.6) for
  either flow's session — Decision 1 makes it newly *reachable* in
  principle (a correctly-scoped per-user token exists for both B and C
  now), but neither `distant-signal-mcp`'s own tool code nor the
  orchestrator's use of that reachability is designed here.
- **The NRE/Network-Rail-branding attribution question for MCP
  tool-rendered output.** Still unresolved, carried forward unchanged from
  both the chatbot research doc and the sibling train-mcp design's own
  Licensing note — applies identically to both B and C since both render
  the same tool output, just through different UIs (DS's own chat surface
  vs. Claude's).
- **Per-conversation token/cost economics for Option B**, and the
  enricher-local-LLM-instead-of-Anthropic question (chatbot research doc
  Section 3a/3b) — both carried forward as unresolved, unaffected by this
  document's own scope.

## Open questions / risks

1. **Whether Authentik (the pinned `ghcr.io/goauthentik/server:2026.8.0`)
   actually supports what both flows' token acquisition needs** — OIDC
   Discovery, PKCE `S256`, DCR/CIMD (for Option C, per the corrected
   research doc's own Open Question 5), and some non-interactive
   token-exchange-or-equivalent grant (for Option B, per this document's
   own Decision 1). Not verified in this session for either purpose;
   belongs to the sibling design's Decision 4 for the Option-C-facing half,
   and is a wholly new open question this document raises for the
   Option-B-facing half.
2. **Whether Authentik's own consent-history view (confirmed to exist,
   Current relevant state) actually lets a user self-service-revoke a
   granted connector**, not merely view it. Unconfirmed by this session's
   fetches. If it turns out revocation isn't self-service, Decision 6's
   "no bespoke DS UI" call should be revisited — not urgently (an
   operator-mediated fallback exists), but it's a real gap in what was
   found, not something resolved favorably by assumption.
3. **The precise shape of "orchestrator's own OAuth client registration
   against Authentik"** (a new client id/secret, provisioned the same way
   `SSO_CLIENT_SECRET` is today, per Decision 1) is sketched but not
   specified — exact scopes requested, exact audience claim
   `distant-signal-mcp` expects to validate, and where in the chart this
   new secret is provisioned are all implementation-time decisions.
4. **Sequencing's own risk**: shipping Option C first (this document's
   recommendation) means the foundation gets production-validated against
   Claude's own connector infrastructure before Option B's orchestrator
   exists to exercise the non-interactive grant path at all — if that path
   turns out to need something meaningfully different from the interactive
   path Option C validates, some of what "the foundation is proven" means
   by the time Option B starts may not transfer cleanly. Flagged as a real
   risk of the recommended sequencing, not a reason to reject it (the
   alternative — building both at once — carries the larger, compounding
   risk of discovering an Authentik-support gap only after both consumers
   are already built against it).
5. **Cost/rate-limiting enforcement mechanics for Option B beyond the
   allowlist gate** (e.g. per-user or per-conversation spend caps, not
   just an on/off allowlist) are not designed here — Decision 5 only
   answers "who may use it at all," not "how much may a given allowlisted
   user spend." Flagged as a real gap if Option B ships, not resolved.

## References

- `docs/superpowers/specs/2026-09-01-embedded-chatbot-mcp-integration-research.md`,
  including its `## Corrections (2026-09-02)` section — read via `git show
  main:...` this session, per the Corrections section above. Treated as
  primary source for every Anthropic-connector-mechanics claim in this
  document (the `oauth_dcr`/`oauth_cimd`/`oauth_anthropic_creds` table,
  "every connection requires user consent," cross-host authorization
  servers, PKCE `S256`, the `160.79.104.0/21` egress range, the
  Claude.ai/Desktop custom-connector UI flow, and the "billed to the
  user's own Claude plan" finding) — not re-fetched independently this
  session, per this document's own constraint not to re-verify what an
  already-cited research doc already grounded.
- `docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`, read
  in full this session (pre-sibling-revision state) — source of truth for
  `distant-signal-mcp`'s current shape and every citation in this
  document's own Current relevant state section not independently
  re-verified.
- `docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md`,
  read in full this session — source for Decision 1's core reasoning
  (Decisions 2 and 4 specifically).
- `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`,
  `docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md` —
  this document's structural/house-style templates.
- This repository, read directly this session: `crates/api/src/auth/oidc.rs`,
  `crates/api/src/auth.rs`, `charts/distant-signal/templates/railmcp-service.yaml`,
  `charts/distant-signal/values.yaml`, `charts/distant-signal/templates/api-deployment.yaml`,
  `frontend/app/api/[...path]/route.ts`, plus `find frontend/app -maxdepth 2
  -type d` and a repo-wide grep for streaming primitives (both run this
  session).
- `docs.goauthentik.io`'s consent-stage documentation, fetched this session
  — source for the "Authentik consent viewing exists, revocation
  unconfirmed" finding in Current relevant state and Decision 6.
