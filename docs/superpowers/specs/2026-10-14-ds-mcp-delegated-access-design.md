# ds-mcp delegated access to a user's Distant Signal journeys (2026-10-14)

**Status: design approved in direction (2026-10-14), not implemented. A dev
Authentik spike passed on 2026-10-14 (§9.5).** The
user answered the main open questions on 2026-10-14. Section 0a records
those decisions, and the rest of the document has been updated to match.
The questions still open are in section 8. Nothing here is implemented, and
no code was changed to write it.

**Access (later on 2026-10-14):** delegated access is open to every
Authentik user. The `distant-signal-api` application has no policy
binding: Ranma-Config set its binding to `distant-signal-chatbot-users`
`state: absent`, as it did for `distant-signal-mcp` on 2026-09-30, and DS
itself dropped that group on the same day (`/chat` is open to every
logged-in user). The user's grant in DS is what gates use. O3 and §9.6
below are updated to match; the spike results in §9.5 (S8 included) are
kept as run.

Requested by the user. The aim: a user who plans a trip with the
Distant-Signal-MCP server's (ds-mcp's) `plan_journey` tool can save the
itinerary they pick into their own Distant Signal (DS) account, and list
their saved journeys. A later phase may also let them read journeys that
other people have shared with them through groups.

## Contents

0. Summary and recommendation
0a. Decisions (2026-10-14)
1. Goal and non-goals
2. What exists today (verified)
3. Why the 2026-09-29 bridge was removed, and the constraints that follow
4. Options compared
5. Recommended design
6. Threat model
7. Phased plan
8. Open questions for the user
9. Authentik spike runbook (before Phase 1)

## 0. Summary and recommendation

**Recommendation: DS becomes an OAuth resource server for a new, dedicated
Authentik provider (`distant-signal-api`). ds-mcp gets short-lived tokens for
that provider by exchanging (RFC 8693) the Authentik token it already holds
for each signed-in user. DS accepts such a token only if the user has
recorded a grant for ds-mcp on a DS "Connected apps" page.** This is option
B from the brief, with DS-side consent and revocation added, and with
option A (a second authorization-code login) kept as the fallback. It
appears in section 4 as option D2.

Why this combination:

- **No new long-lived secret per user.** ds-mcp already keeps each user's
  Authentik refresh token for its own login, encrypted. Token exchange turns
  that into a 5-minute DS token when one is needed. Option A would need a
  second refresh token per user, and option C a long-lived secret that the
  user pastes in.
- **No account-linking step, so no linking CSRF.** Authentik issues the
  exchanged token for the same `sub` it already authenticated. ds-mcp cannot
  link one user's Claude to another user's DS account, by mistake or
  through an attack.
- **DS keeps control of DS data.** Exchange alone would let ds-mcp act as
  any of its users without asking them. The DS-side grant record makes
  consent explicit, and the user sees and revokes it in DS. A revoke takes
  effect on the next request, without waiting for a token to expire.
- **It respects the reasons the 2026-09-29 bridge was removed.** No DS
  cookie leaves DS, no shared secret is added between the two charts, and
  ds-mcp's login does not depend on DS at all. DS's part is generic: a
  "connected apps" grant table and bearer verification, with nothing
  specific to Claude or MCP. See section 3.
- **DS does the same work under A or B.** Both deliver the same kind of
  token (from the `distant-signal-api` provider, audience
  `distant-signal-api`). If the Authentik spike (section 9) finds token
  exchange unsuitable, ds-mcp switches to the authorization-code flow and
  DS changes nothing.

**Phase 0 needs none of this:** a `get_shared_journey` tool on the public
share-link route, and an "open in Distant Signal" link from `plan_journey`
so the user can save the trip in their browser. See section 7.

The main risks are in section 6. In short:

- **A full ds-mcp compromise.** An attacker holding its Redis, its
  encryption key and its `distant-signal-api` client secret could act as
  every user who has granted access, within the granted scopes, until the
  grants are revoked or the secret is rotated.
- **Prompt injection.** Data that ends up in the model's context could steer
  it into saving, reading or deleting.
- **Privacy in Phase 3.** Group reads would send other people's journeys to
  an LLM vendor.

The design limits each of these:

- Scopes are narrow.
- Delete reaches only journeys this client created and that are not shared
  (§5.5a).
- Every save goes through an itinerary reference held by ds-mcp.
- There are per-user caps and a DS-side kill switch.
- Group reads need an opt-in on each group.

## 0a. Decisions (2026-10-14)

The user answered the main open questions from the first version of this
spec:

| # | Question | Decision | Where |
|---|---|---|---|
| D-1 | Approach | **D2**: token exchange into `distant-signal-api`, gated by a DS-side grant. Option A (authorization code) stays as the fallback | §4, §5 |
| D-2 | Save journeys | Yes. Only through an `itineraryRef`, with a cap of **50 per user per day** | §5.5, §5.10, §5.14 |
| D-3 | List and read own journeys | Yes (`journeys:read`) | §5.5 |
| D-4 | Read group journeys | Yes (`journeys:read_shared`), behind the **per-group opt-in, off by default**. Stays in Phase 3 | §5.13 |
| D-5 | Delete own journeys | Yes, as a **new scope, `journeys:delete`**. It is owner-only and audited, and it carries one safeguard against deletes caused by prompt injection, chosen in this revision: **delete only journeys this client created, and only when they are not shared** | §5.5a |
| D-6 | Grant expiry | The grant **expires after 90 days unused** | §5.3 |
| D-7 | Authentik logout | Revokes the **grant**, not just the live tokens | §5.9 |
| D-8 | Creating a grant | **Revised later on 2026-10-14:** a valid DS session is enough. Its lifetime follows the SSO configuration, with no forced re-login and no time window. The 10-minute window first proposed here was rejected. Compensating controls are a confirmation step, a push notification and a banner | §5.3 |

**Second round (later on 2026-10-14):**

| # | Question | Decision | Where |
|---|---|---|---|
| D-9 | Delete rule | Confirmed as built: delegated delete reaches only unshared journeys this client created | §5.5a |
| D-10 | Grant granularity | One grant per user, covering every MCP client connected to ds-mcp | §5.14 |
| D-11 | Limits and audit | The proposed limits are accepted: 50 saves and 20 deletes a day; per minute 10 writes, 60 reads and 5 deletes; a per-client ceiling at 20 times the per-user limit. Audit retention as proposed, with no extra audit table | §5.10, §5.11 |
| D-12 | DS-only logout | The grant is **kept** | §5.9 |
| D-8 (revised) | Creating a grant | A valid DS session is enough, and its lifetime follows the SSO configuration. The user's words: "10-min is too recent, we should consider that users may be logged in and keeping a long-running session dependent on sso config instead of constantly re-logging in." No `max_age` and no `authenticated_at`. Compensating controls are in §5.3, and the threat-model change is T6b | §5.3, §6 |

Decision D-7 replaces the first version's recommendation to keep the grant
across an Authentik logout. The cost is accepted: after any Authentik
logout, the user grants access again on the Connected-apps page before the
assistant can save. Signing back in to Claude alone is no longer enough.

## 1. Goal and non-goals

Goal:

- A ds-mcp tool saves an itinerary that `plan_journey` returned into the
  calling user's DS account, as a tracked journey.
- A ds-mcp tool lists that user's saved journeys and reads one.
- A ds-mcp tool deletes a journey that the assistant saved (D-5, §5.5a).
- Later: read the journeys that groups share with that user.

Non-goals:

- Any other DS write through ds-mcp:
  - deleting journeys created in the web app, or journeys that are shared;
  - share links;
  - group management;
  - preferences or the account;
  - train pins outside a journey;
  - journey templates.
- Making DS a general OAuth platform for third-party apps. The grant table
  is generic enough to admit a second client later, but this spec
  registers only ds-mcp.
- Changing how users log in to ds-mcp or to DS.
- Enterprise Managed Auth or ID-JAG. Ranma-Config's
  `docs/specs/claude-identity-linking.md` §1 covers why these are not
  usable here: Authentik can neither issue nor accept an ID-JAG, and only
  Okta can act as the identity provider.

## 2. What exists today (verified)

Verified against DS `origin/main` at `c7943a05` and the ds-mcp repository at
`fa14d88`. The ds-mcp facts come from reading that repository and from the
ds-mcp session.

### 2.1 Journey routes (`crates/api/src/routes/journeys.rs`)

Mounted at the router root, not under `/public` (`main.rs`), so the frontend
proxy reaches them as `/api/Journeys/...`. Every route takes
`AuthenticatedUser`, the session-cookie extractor, with one exception:

| Route | Auth | Notes |
|---|---|---|
| `POST /Journeys` | cookie | Creates a journey with **one** leg (`pin`, `knownTrain` or `window`). Capped by `MAX_JOURNEYS_PER_USER` (1000); a pin also hits the train-pin cap |
| `POST /Journeys/{id}/legs` | cookie, owner | Adds a `knownTrain` or `window` leg. `PlanTripFlow.tsx` saves a multi-leg plan as one `POST /Journeys` plus one call here per extra leg, which is not atomic |
| `GET /Journeys/mine` | cookie | `list_journeys_for_user` |
| `GET /Journeys/{id}` | cookie, owner **or group member** | The only route gated on `journey_readable_by` |
| `DELETE /Journeys/{id}`, `DELETE .../legs/{leg}` | cookie, owner | |
| `GET .../candidates`, `POST .../train` | cookie, owner | Picking a train for a window leg |
| `POST/DELETE /Journeys/{id}/share-link`, `POST .../extend` | cookie, owner | Mints an unlisted link with a 30-day lifetime (`JOURNEY_SHARE_LINK_TTL`); tokens are hashed at rest |
| `GET /Journeys/shared/{token}` | **none** | Public. Returns the same `JourneyDetailResponse` with `isOwner: false` |

`knownTrain` legs check only that the train UID and the CRS overrides are
well formed (`validate_train_uid`, `validate_known_train_overrides`). They do
not check that the train runs on that date. Anything that takes legs from a
model's output therefore needs its own integrity check (§5.14).

### 2.2 Groups (`crates/api/src/routes/groups.rs`)

Every group route is cookie-only. The read side relevant here:
`GET /groups/shared-journeys` (`list_shared_journeys_for_user`),
`GET /groups/{id}/journeys`, and `journey_readable_by`, which reads as
"owner, or a member of a group the journey is shared into". Group sharing is
read-only for members: every write route checks ownership in its own
`WHERE` clause.

### 2.3 DS authentication

- **Human login** (`auth/oidc.rs`, `routes/auth.rs`) uses Authentik OIDC
  with PKCE against the `distant-signal` provider (`SSO_CLIENT_ID`). It
  creates a `sessions` row, stored as a SHA-256 hash, and the
  `distant_signal_session` cookie: `SameSite=Lax`, with
  `SESSION_TTL_DAYS`, default 14. `users.id` is the OIDC `sub`. A user row
  exists only once someone has logged in to DS.
- **CSRF defences** (`auth.rs`):
  - `reject_cross_origin_cookie_mutation` is layered on the whole router.
    It rejects any mutation that carries a session cookie and whose
    `Origin`/`Referer` names another origin.
  - The stricter `is_same_origin` check guards `logout` and admin revoke.
  - The frontend proxy has its own Origin check and forwards **only** the
    session and login-state cookies. It **never forwards
    `Authorization`** (`frontend/app/api/[...path]/route.ts`; the
    `rate_limit.rs` module doc).
  - CORS allows GET only, with no credentials.
- **Service auth** (`auth/internal_oauth.rs`, `rate_limit.rs`):
  - ds-mcp gets a client-credentials JWT as `srv-ds-mcp`. A
    `ServiceTokenVerifier` checks it against the internal provider's JWKS
    (`iss`, `aud` = `INTERNAL_OAUTH_CLIENT_ID`, `exp`, `nbf`), and the
    rate limiter checks the `srv-ds-mcp` group.
  - The token's only effect is to move the request onto the `svc:mcp`
    budget. It carries no end-user identity. Handlers never see it.
  - **Any bearer that fails internal verification gets a 401 before any
    handler runs.** Every new kind of bearer has to be taught to this
    middleware first (§5.7).
  - `ensure_sso_and_internal_oauth_clients_differ` refuses to start if the
    SSO and internal client IDs are equal. This is the audience-separation
    rule, in code.
- **Revocation** (`docs/session-revocation.md`):
  - User "log out other sessions", admin `revoke-sessions`, and Authentik
    back-channel logout (`auth/backchannel_logout.rs`) all set
    `users.sessions_invalidated_at` and delete the user's sessions.
  - Back-channel logout works per user, not per session.
  - Authentik sends it only while the login's access token is still valid,
    so the `distant-signal` provider's `access_token_validity` is 14 days.
- **Audit**: revocations log `audit=true event=...` lines, which Loki keeps
  for 7 days (2026-09-30 observability spec).

### 2.4 ds-mcp authentication (from its repository)

- **Its own OAuth 2.1 authorization server** for MCP clients:
  - RFC 8414 and RFC 9728 metadata, with CIMD for Claude's published client
    IDs and sanitized DCR for other clients (`src/oauth/`).
  - Clients are public and must use PKCE S256.
- **Its own Authentik login**, as the confidential client
  `distant-signal-mcp`, requesting `openid profile groups offline_access`
  (`src/oauth/authentik.ts`, `authentikRoutes.ts`). An anti-CSRF cookie
  (`__Host-mcp-authz`) binds the Authentik callback to the browser that
  started it.
- **Tokens it issues to MCP clients** are opaque and stored as SHA-256
  hashes:
  - The access token lasts 1 hour.
  - The refresh token lasts 30 days. It rotates, and reuse revokes
    everything for that `sub`.
  - Every refresh also refreshes against Authentik and re-reads `groups`.
- **What it stores in Redis** (`src/oauth/store.ts`, keys `railmcp:oauth:`):
  - One `AuthentikGrant {sub, groups, refreshToken}` per `sub`.
  - The Authentik refresh token is encrypted with AES-256-GCM under
    `OAUTH_REFRESH_TOKEN_ENCRYPTION_KEY`. The rest of each record is plain
    JSON, protected by Redis auth and a NetworkPolicy.
- **Back-channel logout** at `/oauth/authentik/backchannel-logout` runs
  `revokeAllForSub`.
- **Users authenticate only through Authentik.** There is no other identity
  provider and no anonymous mode. Inside Authentik, users may come from
  federated sources such as Discord.
- **Calls to DS** carry only the service bearer, with no per-user identity
  (`src/ds/client.ts`, `src/ds/internalOAuth.ts`).
- **`plan_journey`** calls DS `GET /Trips/plan`. Each itinerary is
  `{legs[], departureMin, arrivalMin, changes, liveFeasible?}`. A train leg
  carries `uid`, `departureAt`/`arrivalAt`, `from`/`to` and `operator`; a
  transfer leg carries `mode` and `minutes`. Nothing in ds-mcp saves
  anything today.

### 2.5 Authentik facts this design relies on

From Ranma-Config's `docs/specs/claude-identity-linking.md` (2026-09-29) and
the 2026-09-02 internal-OAuth spec. These are verified only in the docs
unless stated otherwise.

- **Same `sub` in every provider.** Both providers leave `sub_mode` at the
  default `hashed_user_id`, which is `User.uid`, a value per install rather
  than per provider. **The `sub` ds-mcp sees is DS's `users.id`.** This
  holds only while every provider involved keeps the same `sub_mode`.
- **Token exchange.** Authentik 2026.8 supports RFC 8693, in impersonation
  mode and, since 2026.8.0, in delegation (on-behalf-of) mode. It accepts
  and issues `access_token` and `jwt` token types. See section 9 for what is
  not yet verified.
- **Token format and lifetime.** Access tokens are RS256 JWTs whose `aud` is
  the provider's `client_id`, and validity is set per provider.
- **Refresh tokens** rotate and are revoked on use. A refresh token is
  issued only for `offline_access`.

## 3. Why the 2026-09-29 bridge was removed, and the constraints that follow

Until 2026-09-29 the DS frontend's `/connect-claude/authorize` page finished
ds-mcp's OAuth grants:

1. A logged-in DS user approved the grant.
2. The DS frontend POSTed the user's **raw `distant_signal_session`
   cookie** to ds-mcp's `/internal/complete-authorization`, guarded by the
   shared secret `RAILMCP_INTERNAL_COMPLETE_TOKEN`.
3. ds-mcp replayed that cookie to DS `/public/auth/session` on every `/mcp`
   call.

ds-mcp commit `1c1e101` replaced this with ds-mcp's own Authentik client.
DS commits `5422e3b6`, `85fbb128`, `d1c1789a` and `fb918c99` removed the
DS side. The reasons recorded in that commit and in the Ranma-Config spec:

| # | Reason | Constraint for this design |
|---|---|---|
| R1 | The Claude connection borrowed a 14-day DS **browser session**. It died silently whenever the user logged out, logged in again or let the session expire, and could not recover | No DS browser session may be on the delegated path. The link must survive the user's ordinary DS logins and logouts |
| R2 | A **raw session cookie** crossed services and was replayed on every call | No DS credential that works anywhere else (cookie or SSO token) may leave DS. ds-mcp gets a token that is good only as a delegated bearer for a few named scopes |
| R3 | A dead session produced a **403** that Claude treats as terminal, so users had to reconnect by hand | Failures must map to responses ds-mcp can act on: re-exchange on 401, and a tool-level "grant access here" message on 403. They must never reach Claude as an HTTP 401 or 403 on `/mcp` |
| R4 | There was **no refresh**: one 90-day token | Short-lived DS tokens, renewed without user interaction |
| R5 | A **shared secret and cross-chart coupling** between the DS frontend and ds-mcp | No new DS↔ds-mcp shared secret and no lockstep releases. Anything shared is through Authentik |
| R6 | **"ds-mcp is the front for anything Claude/MCP-specific; DS should not hold that detail"** (the user's principle, `provider.ts:178-189`) | DS's side must be generic: verifying a bearer for a registered client, and a connected-apps grant. DS has no Claude- or MCP-specific code, and ds-mcp owns its tools and its UX |

A consequence noted at the time (ds-mcp `0f026ce`): someone can now use
ds-mcp without ever having a DS account. Saving to DS requires a DS account,
so the design has to handle "no DS user row yet" (§5.4).

## 4. Options compared

### A. Authorization code with PKCE against a DS-audience provider

**How it works.** ds-mcp is a confidential client of a new
`distant-signal-api` provider. When the user first saves, ds-mcp sends them
through a second Authentik authorization-code flow with PKCE. The consent
screen lists `journeys:read journeys:write`, and the user approves.

**What ds-mcp holds afterwards.** A refresh token per user for that
provider, encrypted, plus short access tokens.

**What DS does.** It verifies `iss`, `aud` = `distant-signal-api`, `exp`
and scope, and maps `sub` to `users.id`.

**Can it piggy-back on ds-mcp's existing login?** Not cleanly.

- Authentik puts one audience (the provider's `client_id`) in the token and
  does not support RFC 8707 resource indicators (to verify, §9).
- DS would therefore have to accept tokens whose audience is
  `distant-signal-mcp`. Those are ds-mcp's own login tokens, valid for 14
  days so that back-channel logout keeps working (§2.5).
- That breaks the audience separation DS enforces at startup, and it gives
  every ds-mcp user DS write access without opting in.
- So A always needs a second hop. Once the user has an Authentik browser
  session, the hop is a single consent click, and it happens once per link,
  not once per login.

**Assessment**

- **Security**
  - Good: standard flow, explicit consent at the identity provider, narrow
    scopes.
  - It adds an account-linking flow, which is open to linking CSRF. The
    `state` value must be bound to the ds-mcp user who started it, and the
    `sub` checked against theirs.
- **UX:** one extra browser round trip per user.
- **Work**
  - ds-mcp: a second OAuth client, callback, state store, and a second
    refresh lifecycle, which has the same refresh race that `store.ts`
    already documents.
  - DS: verifier, extractor, grant check.
  - Authentik: a provider with explicit consent and custom scope mappings.
- **Revocation**
  - Authentik consent can be revoked in Authentik's user settings, which DS
    cannot see.
  - Local JWT checks cannot see mid-lifetime revocation, so DS still needs
    its own grant record for instant revocation.
- **Blast radius of a leak**
  - An access token: minutes, within its scopes.
  - A refresh token: useless without the `distant-signal-api` client
    secret.
  - ds-mcp's Redis plus its keys plus its secret: every linked user, until
    revoked.

### B. Token exchange (RFC 8693)

**How it works.** ds-mcp presents the user's Authentik access token for
`distant-signal-mcp` as the `subject_token` at the `distant-signal-api`
provider's token endpoint. It authenticates with that provider's client
credentials and receives a short DS-audience token. In delegation mode the
token carries an `act` claim naming ds-mcp. (The dev spike found that Authentik 2026.8 sets `act` only for its own `Actor` objects, so DS does not rely on it; see §9.5.)

**Assessment**

- **Security**
  - The `sub` is carried over by Authentik, so ds-mcp cannot pick the
    identity. There is no linking flow and no linking CSRF.
  - **Exchange alone involves no consent from the user.** Any ds-mcp user
    becomes reachable at DS. That is a standing power ds-mcp holds over all
    its users, and it must be gated on the DS side (D2).
  - Authentik's exchange policy must accept subject tokens only from
    `distant-signal-mcp`.
- **UX:** nothing extra beyond a consent step somewhere. Without a DS
  grant, it is invisible to the user, which is a problem in itself.
- **Work**
  - ds-mcp: an exchange call, and a cache in memory.
  - Authentik: a provider with token exchange enabled and the
    federation/trust settings.
  - DS: the same as A.
- **Revocation:** the same gap as A, so DS needs a grant record.
  - Ending ds-mcp's Authentik grant (logout, back-channel logout, refresh
    failure) automatically stops new exchanges.
- **Blast radius of a leak**
  - An exchanged token: minutes.
  - The subject token alone: not usable for exchange without the
    `distant-signal-api` client secret.
  - A full ds-mcp compromise: every user with a DS grant, as in A.
- **Dependency:** Authentik 2026.8 behaviour that has been verified only in
  the docs (section 9).
- **Identity limit:** works only for users whose ds-mcp login is Authentik.
  That is true of every user today.

### C. Personal access tokens created by the user

**How it works.** The user creates a scoped token in DS Preferences, pastes
it into ds-mcp, and can revoke it in DS.

**Assessment**

- **Security**
  - **The obvious paste target is the chat itself.** The secret then lands
    in the LLM conversation, the vendor's logs, transcripts and possibly
    training or retention, all outside both services' control.
  - A ds-mcp web form avoids that, but it is another page with its own
    CSRF and phishing surface.
  - PATs are long-lived, typically 30 to 365 days.
  - They are a DS-built credential system, which is the direction the
    2026-09-02 internal-OAuth spec reverted away from ("building a bespoke
    identity/authorization system by hand when this app already has a real
    … authorization server").
- **UX:** the worst of the four. A copy-and-paste ceremony, and an expiry
  that breaks the connection silently, the same failure as R1.
- **Work**
  - DS: token table, hashing, UI, verifier.
  - ds-mcp: a paste flow and encrypted storage.
  - No Authentik work.
- **Revocation:** simple and instant at DS.
- **Blast radius of a leak:** the full PAT lifetime, within its scopes, and
  wherever the paste went.
- **Identity:** works whatever IdP ds-mcp uses, since the token is the
  identity, which is also its weakness.

### D1. No delegation: "open in Distant Signal" hand-off

**How it works.** `plan_journey` adds a DS `/plan?...` link to each result.
`/plan` already rebuilds a search from its query string
(`lib/tripPlanUrl.ts`). The user opens the link, picks the itinerary and
presses "Track this journey" in their own browser session.

**Assessment**

- **Security:** no new credential at all.
- **UX:** an extra browser step, and the user re-picks the itinerary unless
  `/plan` learns a "pick" parameter. It cannot list saved journeys inside
  the chat.
- **Recommendation:** ship it in Phase 0 anyway. It is free and remains
  useful after delegation exists.

### D2. Recommended: B's token exchange, A as fallback, consent and revocation in DS

This is a DS-audience resource server (A's verification), fed by token
exchange (B), and gated by a **DS-side grant** that the user creates and
revokes on a DS "Connected apps" page. Section 5 gives the details.

### D3. Rejected: DS accepts ds-mcp's existing Authentik tokens directly

Add a scope to the `distant-signal-mcp` provider and have DS accept its
tokens. This gives a single login with no second hop, but:

- the tokens are valid for 14 days;
- the audience belongs to another service;
- every ds-mcp user gets DS access without opting in;
- it breaks the rule that "a token for X is not a credential for Y".

Rejected.

### Comparison

| | A auth code | B exchange | C PAT | D1 hand-off | **D2 (recommended)** |
|---|---|---|---|---|---|
| Per-user secret ds-mcp stores | 1 extra refresh token | none extra | 1 PAT | none | none extra |
| DS-audience token lifetime | 5 min | 5 min | 30–365 days | — | 5 min |
| User consent | Authentik screen | none (!) | creating the PAT | the browser save itself | DS connected-apps page |
| Linking CSRF | yes, must mitigate | no | no (paste) | no | no |
| Instant DS-side revoke | needs grant table | needs grant table | yes | — | yes |
| Survives DS logout and login (R1) | yes | yes | yes, until expiry | — | yes |
| Follows ds-mcp's Authentik logout | no (separate grant) | yes | no | — | yes |
| Non-Authentik ds-mcp users | yes | no | yes | yes | via the A fallback |
| New DS↔ds-mcp shared secret (R5) | no | no | no | no | no |
| Lists journeys in the chat | yes | yes | yes | no | yes |
| Authentik features verified only in docs | consent and scope claims | exchange | none | none | exchange (A as fallback) |

## 5. Recommended design

### 5.1 Overview

```
User ──OAuth──> ds-mcp ──(own Authentik login, refresh token encrypted)──> Authentik
                  │                                                            │
                  │ 1. token exchange: subject = user's distant-signal-mcp     │
                  │    access token, client = distant-signal-api credentials   │
                  │<──────────── 2. DS-audience JWT, 5 min, sub = user ────────┘
                  │
                  │ 3. Authorization: Bearer <DS-audience JWT>  (in-cluster)
                  v
                 DS api ── verify iss/aud/exp/scope ── delegated_grants row
                           (user_id = sub, client = ds-mcp, active, scopes)
                           ── handler runs as that user
```

The user creates the grant once, in DS:

1. ds-mcp's first save returns a tool message with a link to
   `https://<ds>/account/connected-apps/ds-mcp`.
2. The user logs in to DS if they have no session. That creates their DS
   user row if this is their first visit.
3. The user sees what ds-mcp will be able to do and presses **Allow**.
4. The user goes back to the chat.

### 5.2 Authentik: the `distant-signal-api` provider

A new OAuth2 provider and application. Ranma-Config owns the live blueprint;
DS's dev blueprint gains a matching entry.

- `client_id` `distant-signal-api`. This is the audience DS checks. It is
  confidential, and **only ds-mcp holds its client secret**.
- `grant_types`: `token_exchange`, plus `authorization_code` and
  `refresh_token` only if the A fallback is turned on.
- **Federation / trusted subject issuers:** only the `distant-signal-mcp`
  provider. No other provider's tokens may be exchanged into this one.
- `sub_mode` the same as `distant-signal` and `distant-signal-mcp`
  (`hashed_user_id`). A comment in the blueprint must say why.
- `access_token_validity`: **minutes=5**. Unlike the other two providers,
  this one does not need long access tokens to keep back-channel logout
  working, because DS does not rely on this provider's back-channel logout
  (§5.9).
- Signing key: RS256, the same certificate as the others.
- Scope mappings: `ds-journeys-read`, `ds-journeys-write`,
  `ds-journeys-delete` and (Phase 3) `ds-journeys-read-shared`. Each emits its scope name into the token's
  `scope` claim, or into a `ds_scopes` claim if Authentik does not put
  `scope` on access tokens (section 9).
- No `groups` claim is needed. DS does not authorize delegated callers by
  IdP group.

DS configuration (chart `api.delegatedClients`, env `DELEGATED_CLIENTS`) is a
static list of registered clients:

```yaml
delegatedClients:
  - name: ds-mcp                 # the grant table's `client` value, shown in the UI as "Distant Signal assistant (MCP)"
    issuerUrl: https://sso.cursed.solutions/application/o/distant-signal-api/
    audience: distant-signal-api
    maxScopes: [journeys:read, journeys:write, journeys:delete]   # + journeys:read_shared in Phase 3
    enabled: true                # the kill switch
```

The startup guard grows from two client IDs to all of them. The SSO client
ID, the internal client ID and every delegated audience must be pairwise
distinct, and the delegated issuers must differ from `SSO_ISSUER_URL` and
`INTERNAL_OAUTH_ISSUER_URL`. A list that is empty or disabled makes the whole
feature inert, matching how `INTERNAL_OAUTH_GROUP_MCP` behaves.

### 5.3 Consent: the DS grant record

New table:

```sql
CREATE TABLE delegated_grants (
    id              BIGSERIAL PRIMARY KEY,
    user_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client          TEXT NOT NULL,            -- a DELEGATED_CLIENTS name
    scopes          TEXT[] NOT NULL,          -- subset of that client's maxScopes
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    not_before      TIMESTAMPTZ NOT NULL DEFAULT now(),  -- tokens issued earlier are refused
    last_used_at    TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    revoked_reason  TEXT                      -- user | admin | backchannel | client_logout | inactive | client_disabled
);
CREATE UNIQUE INDEX delegated_grants_one_active
    ON delegated_grants (user_id, client) WHERE revoked_at IS NULL;
```

The table is generic: no column mentions MCP or Claude (R6).

Routes. All are cookie-only (`AuthenticatedUser`). They are never reachable
with a delegated bearer, because a delegated caller must not be able to
widen its own grant.

- `GET /account/connected-apps`: the user's grants, with name, scopes,
  created date and last-used date.
- `POST /account/connected-apps/{client}`: create a grant, or replace the
  active one, with a body `{scopes}` that must be a subset of `maxScopes`.
  - It uses the **strict** `is_same_origin` check, as `logout` and admin
    revoke do, on top of the router-wide guard.
  - The scopes come from the form the user submitted, never from a query
    string that ds-mcp or the model wrote. A ds-mcp link may preselect
    boxes for display, but the server accepts only the submitted body.
- `DELETE /account/connected-apps/{client}`: revoke. Idempotent, 204, strict
  same-origin.

The frontend adds a page at `/account/connected-apps/[client]`. It shows a
plain explanation, the scopes as sentences, and Allow and Cancel buttons.
The same list also appears under Account/Preferences.

**Creating a grant needs a valid DS session, and nothing more (D-8, as
revised on 2026-10-14).**

- How long a DS session lasts is governed by the SSO configuration and
  `SESSION_TTL_DAYS`, not by this feature. There is no forced re-login, no
  `max_age`, and no `sessions.authenticated_at` column.
- The user's reason: people stay logged in on long-running sessions that
  follow the SSO configuration, and should not be made to keep logging in
  again.

**An optional step-up was considered and rejected for Phase 1.** It would
have been a configurable maximum login age, off by default, enforced
through `max_age` and `auth_time`.

- It needs the same `oidc.rs` and session-schema work as the dropped
  10-minute rule, all for a switch nobody plans to turn on.
- Its protection is weak where it matters. A thief holding a session cookie
  usually holds the browser too, and Authentik's own session in that
  browser would satisfy a silent `max_age` round trip, unless `prompt=login`
  is also forced.
- It stays available as a later option (Phase 4) if the risk assessment
  below changes.

**Why a cookie-only grant is acceptable.** A grant names one client
(ds-mcp), and DS honours it only for tokens that Authentik minted from
**the same user's own ds-mcp login** (§5.4, §5.7). So someone who steals a
DS session cookie and creates a grant has, by doing so, only enabled the
victim's own assistant. To use the grant, they would also need the victim's
Claude or ds-mcp connection.

- With the cookie alone they can already do more directly, in the web app:
  read and delete every journey, create share links and manage groups.
- What the grant adds is **persistence**. It outlives the stolen session
  and "log out other sessions" (which keeps grants). It is still limited to
  the assistant, ends at the next Authentik logout (D-7), and ends after
  90 days unused.

**Compensating controls**

1. **An explicit confirmation step.**
   - `GET /account/connected-apps/{client}` renders the consent page. It
     shows the signed-in username, the client's name, and each scope as a
     sentence.
   - Rendering it issues a single-use confirmation token that is bound to
     the session and the client, valid for 10 minutes, and stored hashed.
   - The POST must carry that token. Without it, the answer is 403
     `confirmation_required`.
   - The page sends `frame-ancestors 'none'` against clickjacking.
   - The token does not stop a cookie thief, who can render the page too.
     It does make every grant the result of a rendered page and a deliberate
     click, never a single blind request.
2. **Notices when a grant is created.** DS stores no email address, so it
   cannot send email. Instead:
   - Each of the user's existing web-push subscriptions gets a push
     notification: "Distant Signal assistant can now save and read your
     journeys. Not you? Disconnect it in Connected apps." This uses the
     notifier's existing VAPID push path, through a new
     `grant_created` push-queue item.
   - Every DS page shows a dismissible banner for 7 days after a grant is
     created, linking to Connected apps.
   - The Connected-apps list shows each grant's creation time and last-used
     time.
3. **Revocation that is easy to find.** Disconnect sits on the same page as
   the notice. It needs no re-login and takes effect on the next request
   (§5.9).
4. **Audit.** `event=delegated_grant_created` is logged with the user, the
   client, the scopes and a hash of the session ID, so a grant can be traced
   to the session that created it.

**Unused grants expire after 90 days (D-6).** An hourly maintenance pass,
in the api's existing maintenance loop, marks grants as revoked with reason
`inactive` when `last_used_at` is older than 90 days, or, for a grant never
used, when `created_at` is.

- The extractor also checks `last_used_at` directly, so an expired grant is
  refused even before the sweep runs.
- There is no absolute expiry for a grant in use. With D-7, any Authentik
  logout already ends the grant, so a grant in daily use is still renewed
  through a fresh consent every time the user logs out of Authentik.

**Account export and deletion.** `GET /account/export` includes the user's
grants. `DELETE /account` cascades.

### 5.4 Identity mapping and account linking

- **The mapping is `users.id = token.sub`, and nothing else.** No matching
  on email or username: DS no longer stores email at all, and usernames are
  not unique (`find_user_ids_by_username`).
- **Authentik alone guarantees that `sub` is the same person,** because all
  three providers share a `sub_mode`. A spike test in section 9 confirms the
  equality on the real instance. A drift in `sub_mode` makes tokens resolve
  to no grant, so DS answers 403 `delegation_not_granted`. It fails closed,
  never resolving to the wrong person, because a `sub` value from a
  different mode does not collide with a `hashed_user_id`.
- **No DS account yet** (ds-mcp-only users):
  - The token verifies, but there is no `users` row and so no grant.
  - DS answers 403 `delegation_not_granted`, the same response as for any
    user without a grant. DS deliberately does not reveal whether a DS
    account exists.
  - The consent page requires a DS login, which creates the row through the
    normal OIDC path, with the normal privacy and terms flow and the normal
    `inactiveAccountRetentionDays` lifecycle.
  - **DS never creates a user from a delegated token** (no just-in-time
    provisioning).
- **ds-mcp users who authenticate somewhere other than Authentik.** None
  exist today. If ds-mcp ever adds another login method, token exchange
  cannot serve those users because there is no Authentik subject token. For
  them, ds-mcp uses the **A fallback**:
  1. An authorization-code flow with PKCE against `distant-signal-api`.
  2. A `state` value stored server-side, single-use, valid for 10 minutes,
     bound to the ds-mcp principal that started the flow and to the
     `__Host-` CSRF cookie.
  3. After the callback, ds-mcp stores `{ds-mcp principal → DS sub, refresh
     token (encrypted)}` and shows the user "Linked to Distant Signal as
     <preferred_username>", so a mismatch is visible.
  4. DS's side is unchanged: the token's `sub` still needs a grant.
  - For an Authentik-login user going through A anyway (for example if
    exchange is turned off), ds-mcp **refuses the link unless the DS
    token's `sub` equals the user's ds-mcp `sub`**. That rules out linking
    to someone else's DS account altogether.

### 5.5 Scopes

| Scope | Allows | Phase |
|---|---|---|
| `journeys:read` | `GET /Journeys/mine`; `GET /Journeys/{id}` for journeys **the user owns** | 1 |
| `journeys:write` | Create a journey from an itinerary (`POST /Journeys/itinerary`, §5.8). Does **not** include delete, leg edits on existing journeys, train re-picks, or share links | 1 |
| `journeys:delete` | `DELETE /Journeys/{id}` for a journey the user owns **that this client created and that is not shared** (§5.5a) | 1 |
| `journeys:read_shared` | `GET /groups/shared-journeys`; `GET /Journeys/{id}` through group membership, only for groups that opted in (§5.13) | 3 |

DS computes the effective scopes per request as the intersection of three
sets, and each route asks for one scope:

- the scopes in the token's claim;
- the grant's `scopes`;
- the client's `maxScopes`.

There is still no share-link scope, because minting a public URL is an
exfiltration channel.

### 5.5a The delete scope (D-5)

**Rules.** A delegated `DELETE /Journeys/{id}` succeeds only when all of the
following hold:

1. The caller owns the journey: `user_id = token.sub`, folded into the
   `WHERE` clause, as every write route already does.
2. The journey was created by this client: `journeys.created_via = <client
   name>`.
3. The journey is not shared. There is no `group_journeys` row for it and
   no active unlisted share link.
4. The per-user delete limit has not been reached (§5.10).

When rules 1 or 2 fail, the response is 404 `not_found`, so the assistant
cannot learn about journeys it cannot delete. When rule 3 fails, it is
409 `journey_shared`, with a message telling the user to delete the
journey in Distant Signal. The handler then runs the existing
`data::journeys::delete_journey`, including its template-occurrence
tombstone, unchanged, inside a transaction that re-checks rules 2 and 3.

**Why "created by this client" rather than soft delete or an undo
window.** All three options soften prompt-injected deletes. This one is the
smallest, and the only one that cannot leak.

- **Worst case.** At worst, injected text makes the model delete journeys
  the assistant itself saved. Those are re-creatable from a new
  `plan_journey`, and none of them is shared with anyone. Journeys the user
  built by hand in the web app, journeys from templates, and anything
  shared with a group or through a link are out of reach.
- **Soft delete would cost far more.** A `deleted_at` column would have to
  be honoured by every reader of `journeys`, about 100 query sites today:
  - 36 in the notifier;
  - 25 in `data/journeys.rs`;
  - 13 in `data/groups.rs`;
  - 12 in `data/journey_templates.rs`;
  - more in retention, account and train.

  Every one that was missed would be a privacy or notification bug:
  - a "deleted" journey still shown to a group;
  - one still sending delay alerts;
  - one still resolving through its share link.

  That is a large, permanent cost to every future journey query, imposed
  for a feature of the assistant.
- **An undo window has the same problem, or a worse one.** A plain undo
  window is soft delete with a timer. A snapshot-and-recreate variant
  would bring a journey back with new ids, losing its tracking state,
  group shares and share link.
- **The cost of this choice.** The assistant cannot tidy journeys the user
  made in the web app. That is a deliberate limit, and the user still
  deletes those in DS.

**Audit.** Every delegated delete writes a `delegated_deletions` row, kept
for 30 days and deleted by the maintenance pass after that. The row holds:

- `user_id`, `client`, `grant_id`, `journey_id`, `deleted_at`;
- a small `summary` JSON: the custom name, the legs' service date, CRS
  pair and train UID.

The Connected-apps page lists these as "Recently deleted by the assistant",
so the user can see what happened and re-save a journey by hand. This
gives a durable trail beyond Loki's 7 days without soft-deleting the
journey itself. Each delete also logs `audit=true event=delegated_delete`
at WARN.

**On the ds-mcp side,** `delete_journey { id }` carries the MCP annotation
`destructiveHint: true`, so clients that support confirmations ask the user
first. Its description tells the model to delete only when the user asked
for it in this conversation. Neither is a security boundary; rules 1–4 are.

### 5.6 Token lifetimes and refresh

| Token | Held by | Lifetime | Renewal |
|---|---|---|---|
| ds-mcp's MCP access token | MCP client | 1 h (unchanged) | MCP refresh |
| ds-mcp's Authentik refresh token (`distant-signal-mcp`) | ds-mcp Redis, encrypted | 30 days sliding, rotating (unchanged) | on each MCP refresh |
| ds-mcp's Authentik access token (`distant-signal-mcp`), the **subject token** | ds-mcp Redis, **encrypted** (new field on `AuthentikGrant`) | 14 days (provider setting) | replaced on every Authentik refresh, which happens about hourly while the user is active |
| DS-audience token (`distant-signal-api`) | ds-mcp **process memory only**, keyed by `sub` | 5 min | exchanged again when under 30 s remain, or after a DS 401 (once) |

There is no refresh token for `distant-signal-api` in the exchange path.
Each DS token is minted fresh from the subject token. If the subject token
has expired or been revoked, the exchange fails, and ds-mcp treats it as
it already treats a dead Authentik grant: tool error, then sign in again.

In the A fallback, the `distant-signal-api` refresh token is stored
encrypted, rotates, and slides over 30 days. A grant that has not been used
for 90 days is expired by DS anyway.

### 5.7 DS verification, and how cookie and bearer auth coexist

**Verifier.** A third `ServiceTokenVerifier` instance is built for each
delegated client. It reuses the type: JWKS cache, negative `kid` cache,
global refresh cooldown, clock-skew leeway. Its claims struct adds `scope`
`iat` and `jti`. The checks:

1. Signature, `iss`, `aud`, `exp` and `nbf`, as now.
2. `iat` is present.
3. The route's scope is in the `scope` claim. The spike (S4) showed that
   Authentik issues tokens with `scope=""` when asked for no scope.
4. The `typ` header is not `logout+jwt`.

There is no `act` check. The spike (S3) showed that Authentik sets `act`
only for an internal Authentik `Actor` object. The client is identified by
the token's issuer and audience instead: only ds-mcp holds the
`distant-signal-api` secret, and that provider trusts only
`distant-signal-mcp` subject tokens (S6, S7). Exchanged tokens carry no
`sid`, and their `auth_time` is the time of the exchange, so DS never uses
either.

**Rate-limit middleware first.**

- `rate_limit::service_caller` currently answers 401 to any bearer that
  fails *internal* verification. It must route by issuer first:
  1. Peek at the unverified `iss`.
  2. If it is the internal issuer, use the internal verifier (as today).
  3. If it is a registered delegated issuer, use that client's verifier and
     charge the request to the delegated budgets (§5.10).
  4. Any other issuer gets 401.
- The peek only selects a verifier. Every path still verifies the
  signature.
- Without this change, every delegated request in a limited class would be
  rejected with 401.

**A new extractor, `DelegatedOrSessionUser<S: Scope>`, used only by the
opted-in routes.** It resolves to `Principal { user_id, via: Session |
Delegated { client, grant_id, jti } }`:

1. If `Authorization: Bearer` is present **and** the session cookie is
   present: **400 `ambiguous_credentials`**. DS never picks one of the two
   and never merges them.
2. Bearer only:
   1. Verify the token. If verification fails: 401
      `WWW-Authenticate: Bearer error="invalid_token"`. It never falls back
      to anything.
   2. Load the active grant for `(sub, client)`. None, or a user that does
      not exist: 403 `delegation_not_granted`.
   3. Refuse tokens issued too early: if `token.iat < max(grant.not_before,
      users.sessions_invalidated_at)`, the answer is 401 `invalid_token`.
      ds-mcp re-exchanges. If the cause is a revocation at Authentik, the
      exchange fails and ds-mcp tells the user.
   4. Check the effective scope (§5.5). Missing it: 403 `insufficient_scope`
      with `WWW-Authenticate: Bearer error="insufficient_scope",
      scope="journeys:write"`.
   5. Update `last_used_at` (throttled to once a minute per grant).
3. Cookie only: exactly `AuthenticatedUser`'s logic, scope ignored.

**The cookie path does not change.** Specifically:

- **`AuthenticatedUser` is not modified.** It still reads only the cookie.
  Every route that is not explicitly moved to the new extractor (groups,
  account, preferences, auth, admin, share links, leg deletes, templates,
  `/Train`) cannot be reached with a delegated bearer, by construction.
  - A test lists the routes that use the new extractor and fails when the
    list changes, as `router_builds_without_panicking` already guards
    routing.
- **The frontend proxy keeps refusing to forward `Authorization`.** The new
  test asserts it, so a browser on DS's origin can never present a
  delegated bearer through the proxy, and an XSS on DS cannot turn a stolen
  bearer into anything a cookie could not already do.
- **The CSRF guard keys on the cookie and stays as it is.** Bearer
  requests carry no ambient credential, so CSRF does not apply to them.
  Rule 1 removes the mixed case.
- **A delegated principal has no `groups`.** The admin check and anything
  else that reads `users.groups` keep taking `AuthenticatedUser`, so a
  delegated caller can never become an admin.
- **CORS is unchanged**: GET only, no credentials, `Authorization` not
  allowed in the preflight. A browser on another origin cannot send a
  bearer to the api even if it holds one.
- **Reachability.** ds-mcp reaches the api in-cluster; the api's NetworkPolicy
  already narrows extra-namespace ingress to the MCP pod (`4068cbff`). If
  `ingress.api.enabled` exposes the api publicly, delegated bearers would
  also be accepted there. Open question 2 asks whether to refuse delegated
  bearers on public ingress.

### 5.8 Which routes gain bearer auth

| Route | Today | Delegated scope | Phase | Notes |
|---|---|---|---|---|
| `POST /Journeys/itinerary` (**new**) | — | `journeys:write` | 1 | Creates a journey and 1–8 `knownTrain` legs in **one transaction**. Optional `customName`. Requires an `Idempotency-Key` header from delegated callers; the key is remembered per user for 24 h and a repeat returns the original response. Checks the train-pin cap and the journey cap. Also available with a cookie, so `PlanTripFlow` can drop its non-atomic loop (open question 3) |
| `GET /Journeys/mine` | cookie | `journeys:read` | 1 | Unchanged response |
| `GET /Journeys/{id}` | cookie (`journey_readable_by`) | `journeys:read` (own only) / `journeys:read_shared` | 1 / 3 | For a delegated caller without `read_shared`, ownership only. `shareLink` is always `null` for a delegated caller |
| `DELETE /Journeys/{id}` | cookie, owner | `journeys:delete` | 1 | Delegated callers: only journeys this client created that are not shared (§5.5a). The cookie path is unchanged |
| `GET /groups/shared-journeys` | cookie | `journeys:read_shared` | 3 | Filtered to groups that opted in |
| `POST /Journeys`, `POST /Journeys/{id}/legs` | cookie | **none** | — | Superseded for delegated callers by the atomic route |
| `DELETE /Journeys/{id}/legs/{leg}`, share-link routes, candidates and train picks, every `/groups` write, `/account`, `/preferences`, `/auth`, `/admin`, templates, `/Train/*` | cookie | **never** | — | |

`journeys` gains a `created_via TEXT NOT NULL DEFAULT 'web'` column holding
`'web'` or the client name. The journey page shows "Saved by Distant Signal
assistant", and the user's list can filter on it.

### 5.9 Revocation

From the DS side, every path takes effect on the next request, because the
extractor checks the grant row on every request:

| Trigger | DS action | ds-mcp effect |
|---|---|---|
| User presses **Disconnect** on Connected apps | `revoked_at = now()`, reason `user` | The next DS call gets 403 `delegation_not_granted`, and the tool tells the user how to grant again |
| Admin `POST /admin/users/revoke-sessions` | Also revokes every grant for the user, reason `admin` | as above |
| **Authentik logout, path 1** (D-7): Authentik back-channel logout on **`distant-signal`**. The user logged out of Authentik, was deactivated, or had a session deleted, in an Authentik session that had logged in to DS | Already sets `sessions_invalidated_at` and deletes the sessions. **Now also revokes every active grant for the user**, reason `backchannel`, in the same transaction | ds-mcp's own back-channel logout usually fires for the same session as well. In any case the next DS call gets 403, and the user must grant again |
| **Authentik logout, path 2** (D-7): Authentik back-channel logout on **`distant-signal-mcp` only**. The ended session had logged in to ds-mcp but never to DS, so DS gets no notification of its own | ds-mcp's back-channel handler, before `revokeAllForSub`, calls **`POST /delegated-grants/revoke {sub}`** with its existing `srv-ds-mcp` client-credentials token. DS revokes that user's grant **for this client only**, reason `client_logout` | ds-mcp retries the call with backoff (a Redis-backed outbox, up to 24 h) if DS is unavailable. In the meantime the grant cannot be used anyway, because ds-mcp has no subject token left |
| Deactivated user | Path 1 or path 2. Authentik also refuses exchanges for an inactive user (spike step S9) | |
| User "log out other sessions" in DS | Sets `sessions_invalidated_at`, so outstanding DS-audience tokens die. The grant is **kept**: this is not an Authentik logout | Re-exchanges and carries on |
| DS `POST /auth/logout` | Ends the DS session only. DS does not call Authentik's end-session endpoint, so Authentik sends no notification. The grant is **kept** (D-12) | |
| Account deletion | Cascades | |
| Client disabled (`enabled: false`) or removed from `DELEGATED_CLIENTS` | Every delegated request for it gets 401 | The kill switch for a ds-mcp compromise |
| 90 days unused (D-6) | reason `inactive` | |
| Grant re-created after a revoke | New row with `not_before = now()`, so tokens minted before the new consent are refused | |

**`POST /delegated-grants/revoke`** is the first DS route keyed on the
internal service identity. Until now, `srv-ds-mcp` only selected a
rate-limit budget.

- It sits in the generic DS layer. Each `DELEGATED_CLIENTS` entry gains a
  `serviceGroup` (`srv-ds-mcp`), and a verified internal token in that group
  may revoke grants **of that client only**.
- It can only revoke: no create, no read, and no response that shows
  whether a grant existed (always 204).
- A leaked `srv-ds-mcp` credential could therefore at worst revoke ds-mcp
  users' grants. That is a denial of service the holder could cause anyway
  by not serving requests.
- No new shared secret is involved (R5), and nothing in it is specific to
  MCP (R6).

**Whether Authentik session expiry counts.** An Authentik session that
simply expires, as opposed to an explicit logout, may or may not send a
back-channel notification. Spike step S10 finds out. If it does not, the
grant outlives that session, and only DS's own notifications, Disconnect
and the 90-day expiry end it. That is acceptable: no token can be minted
without ds-mcp's subject token, and that token dies with ds-mcp's own grant.

DS does **not** set a back-channel logout URI on the `distant-signal-api`
provider. Its 5-minute exchanged tokens are not tied to a browser session,
so Authentik would have nothing to notify about. Paths 1 and 2 cover the
same events.

**Revoking from the ds-mcp side by the user's choice.** ds-mcp gets no
"disconnect" tool. Its tools return the Connected-apps link instead, so that
a user's deliberate revocation always happens in DS's own UI. Path 2 is for
logout events only.

### 5.10 Rate limits: per user and per service

- **Today:** `svc:mcp` is one bucket for all of ds-mcp's anonymous public
  reads, five times the public defaults.
- **New:** delegated requests are charged to **two** buckets, and both must
  pass:
  - `user:<sub>` per class:
    - `delegated_write`: suggested 10/min, burst 5;
    - `delegated_read`: suggested 60/min, burst 20;
    - `delegated_delete`: suggested 5/min, burst 3.
  - `svc:<client>-delegated` per class: an aggregate ceiling for the whole
    client, suggested 20 times the per-user limit. This bounds a runaway
    loop across many users, or a compromised client.
- Delegated requests never use `svc:mcp` or the per-IP buckets.
- **Daily per-user creation cap (D-2):** at most **50** journeys per
  rolling 24 h with `created_via = <client>`. This is a database count,
  alongside the existing `MAX_JOURNEYS_PER_USER`. GCRA in memory per pod
  cannot express a daily cap. Over the cap, the response is 429
  `daily_limit`.
- **Daily per-user delete cap:** at most **20** delegated deletes per
  rolling 24 h, counted from `delegated_deletions`. Accepted (D-11).
- **Metrics:**
  - `distant_signal_api_delegated_requests_total{client,route,status}`;
  - `..._delegated_rejected_total{client,reason}`;
  - `..._delegated_grants_active{client}` (a gauge).
- **Alerts:** a burst of `delegation_not_granted` or `invalid_token` from
  one client, or a sharp rise in writes per user.

### 5.11 Audit logging

DS logs the following:

- **Every delegated request**, once, at INFO: `audit=true
  event=delegated_request client user_id grant_id jti method route status
  scopes`.
  - It never logs the token, and never the request body. Itineraries are
    personal travel data.
- **Grant lifecycle events**, at INFO for user actions and WARN for
  admin/system actions: `event=delegated_grant_created|revoked`, with
  `reason`, `client`, `user_id` and `scopes`.
- **Rejections**, at WARN: `event=delegated_rejected reason=...`, with a
  hash of the `sub` when the token verified.
- **Durable record.**
  - Loki keeps logs for only 7 days, so the lasting record is in the
    database: `created_via` on journeys, and `created_at`, `last_used_at`
    and `revoked_*` on grants.
  - No separate `delegated_audit` table (D-11).

ds-mcp logs one line per DS-backed tool call: tool, a hash of the `sub`, the
MCP client ID, the DS status and the `Idempotency-Key`. It never logs tokens
or itinerary contents, and never logs share tokens (Phase 0).

### 5.12 CSRF and confused-deputy risks

**CSRF**

- **Grant creation and revocation:**
  - The routes are cookie-authenticated POST and DELETE requests behind the
    strict Origin check.
  - The cookie is `SameSite=Lax`, the router-wide guard applies, and the
    proxy runs its own Origin check.
  - There is no state change on GET; the consent page is a GET that renders
    a form.
  - The scopes come from the submitted form, never from the URL.
- **Delegated API calls:** they use no ambient credential, so CSRF does not
  apply. A cookie and a bearer together are refused (§5.7).
- **Account-linking CSRF:**
  - Not possible in the exchange path, because there is no callback that
    binds an identity.
  - In the A fallback it is mitigated by binding `state` to the principal
    and the CSRF cookie, and by the `sub` equality rule (§5.4). ds-mcp
    already fixed a confused-deputy takeover in its own login callback this
    way (`e9421df`).

**Confused deputy.** ds-mcp holds power on behalf of many users, so these
are the real risks:

1. **The wrong user's token.**
   - ds-mcp must choose the DS token **only** from the verified MCP
     principal (`req.auth.extra.sub`), never from a tool argument.
   - The in-memory token cache is keyed by `sub` only. A ds-mcp test calls
     the tools as two users at the same time and checks each request's
     `Authorization` header.
   - DS still enforces `user_id = token.sub` for every row it touches.
2. **The wrong audience.**
   - DS refuses DS SSO ID tokens and `distant-signal-mcp` tokens, because
     their `aud` and `iss` differ.
   - It refuses internal `srv-*` tokens, because their issuer differs and
     the rate limiter sends them to a different verifier.
   - It refuses back-channel logout tokens, because of their `typ` and
     because they carry no scope.
   - The startup guard keeps all of these audiences distinct.
3. **Exchange as an escalation path.**
   - Authentik must accept only `distant-signal-mcp` subject tokens for
     `distant-signal-api`.
   - Even so, an exchanged token is useless at DS without the user's grant.
4. **The model as a deputy (prompt injection).**
   - Text from journey names, incident messages or, in Phase 3, other
     people's journey names can tell the model to save or read things.
   - Mitigations:
     - writes need a reference to an itinerary that `plan_journey` returned
       to *this* user (§5.14), so injected text cannot invent a journey;
     - delete reaches only unshared journeys this client created, with a
       30-day record of each deletion (§5.5a);
     - there is no share-link scope;
     - per-user caps;
     - ds-mcp marks `save_journey` with MCP tool annotations
       (`readOnlyHint: false`, `destructiveHint: false`,
       `idempotentHint: true`), and `delete_journey` with
       `destructiveHint: true`, so clients may ask the user to confirm.
   - Reads leak only to the model the user is already talking to. ds-mcp
     has no tool that sends data anywhere else.

### 5.13 Group reads (Phase 3)

A group member's journeys were shared with *people*, not with a third-party
model. Under `journeys:read_shared`, other members' journey names and
itineraries would be sent to an LLM vendor when someone asks their
assistant.

**Per-group opt-in.** `groups` gains `allow_assistant_reads BOOLEAN NOT
NULL DEFAULT false`, which group owners and admins toggle on the group page.
The group's member list shows a short notice when it is on.

- `GET /groups/shared-journeys` and `GET /Journeys/{id}` filter on it for
  delegated callers. Cookie callers are not affected.
- A new helper, `journey_readable_by_delegate`, is kept separate from
  `journey_readable_by` so the cookie path's query is not touched.
- Responses for shared journeys omit `customName` unless the owner opted
  in. See open question 1.
- ds-mcp passes shared data to the model inside a clearly marked "untrusted
  content from other users" block.
- A read through a group logs `group_id` in its audit line.

### 5.14 ds-mcp side

**Tools**

- **`save_journey { itineraryRef, index, name? }`**
  - `plan_journey` stores its result server-side for each `sub`, under an
    opaque `itineraryRef` (Redis, 1 h TTL, at most 20 per user), and
    returns that reference.
  - `save_journey` resolves the reference **for the calling `sub` only**
    and builds the `knownTrain` legs from the stored train legs. Transfer
    legs are not saved.
  - It sends `POST /Journeys/itinerary` with `Idempotency-Key = sha256(sub,
    itineraryRef, index)`.
  - The model never supplies train UIDs, so it cannot fabricate or alter
    an itinerary, and the lack of an existence check on `knownTrain` (§2.1)
    stops mattering.
- **`list_my_journeys`** and **`get_my_journey { id }`**: plain reads. The
  list marks which journeys the assistant saved, so the model knows which
  ones `delete_journey` can remove.
- **`delete_journey { id }`** (Phase 1, `journeys:delete`): sends
  `DELETE /Journeys/{id}`. A 409 `journey_shared` or a 404 becomes a tool
  message saying that journey can only be deleted in Distant Signal.
- **Phase 3: `list_shared_journeys`.**
- **Phase 0: `get_shared_journey { link }`** (section 7).

**Errors** go back to the model as tool results, never as HTTP errors on
`/mcp` (R3):

| Response from DS | ds-mcp action |
|---|---|
| 401 | Exchange again once, then report "sign in again" |
| 403 `delegation_not_granted` | Return the Connected-apps link |
| 403 `insufficient_scope` | Return the link and name the missing scope |
| 409 `journey_shared` | "Shared journeys can only be deleted in Distant Signal", with a link to the journey |
| 429 | Pass on `Retry-After`; for `daily_limit`, say when the 24 h window frees up |

**Token storage, and the blast radius of a Redis leak**

| Item | Today | Proposed |
|---|---|---|
| MCP access and refresh tokens | stored as SHA-256 hashes | unchanged |
| Authentik refresh token (`distant-signal-mcp`) | AES-256-GCM, `OAUTH_REFRESH_TOKEN_ENCRYPTION_KEY` | unchanged; add AAD = `sub`, so a ciphertext cannot be moved to another user's record |
| Authentik access token (`distant-signal-mcp`) | not kept | kept as the subject token, **AES-256-GCM with AAD = `sub`**. It is a 14-day bearer, so it must not be plaintext. A separate key (`DS_SUBJECT_TOKEN_ENCRYPTION_KEY`) allows rotation without touching the refresh tokens |
| DS-audience token | — | **memory only**, never written to Redis |
| `distant-signal-api` client secret | — | a Kubernetes Secret, mounted like the existing Authentik client secret, never in Redis |
| `itineraryRef` records | — | plain JSON (station codes, times, UIDs), 1 h TTL. Personal data, but low value without the user's identity |
| A-fallback refresh token (`distant-signal-api`) | — | encrypted like the others, AAD = ds-mcp principal |

What each kind of leak gives an attacker:

- **Redis dump alone:** nothing usable. Token hashes cannot be replayed, and
  the ciphertexts need the keys.
- **Redis dump plus the encryption keys:**
  - Subject tokens and refresh tokens, which still need a client secret to
    use: the exchange requires the `distant-signal-api` secret, and a
    refresh requires the `distant-signal-mcp` secret.
  - Exception: a subject token is itself a bearer for ds-mcp's own
    provider, so it reveals that user's Authentik profile through
    `/userinfo` for up to 14 days.
- **All of the above plus the client secrets (full ds-mcp compromise):**
  - The attacker can act as every user who has an active DS grant, within
    `journeys:read`, `journeys:write` and `journeys:delete`.
  - They can read the users' journeys, and create junk journeys up to 50
    per user per day.
  - They can delete journeys the assistant saved that are not shared, up
    to 20 per user per day. Each deletion leaves a 30-day
    `delegated_deletions` record from which the user, or an operator, can
    rebuild it.
  - They cannot delete web-created or shared journeys, share anything,
    touch groups or the account, or log in to DS.
  - Containment:
    1. set `enabled: false` (immediate);
    2. rotate the `distant-signal-api` secret in Authentik;
    3. optionally revoke all grants with a one-off SQL statement.
  - `created_via` identifies every journey the client created, for cleanup.

**One link per `sub`, not per MCP client.**

- The DS grant is per user and per client (ds-mcp). Every MCP client the
  user has connected (Claude.ai, Claude Code, the in-app chat) can use it.
- Revoking access for one MCP client alone happens in ds-mcp: revoke that
  client's MCP tokens.
- ds-mcp does not ask again when a new MCP client first uses the grant
  (D-10).

### 5.15 Error contract (DS → ds-mcp)

Bodies are JSON `{error, message}`, matching the 2026-10-06 `unavailable`
convention, and the codes are stable:

| Status | `error` | Meaning |
|---|---|---|
| 400 | `ambiguous_credentials` | Both cookie and bearer were sent |
| 400 | `idempotency_key_required` / `invalid_itinerary` | |
| 401 | `invalid_token` | Bad, expired, or issued before `not_before` or session invalidation |
| 403 | `delegation_not_granted` | No active grant (also covers "no DS account") |
| 403 | `insufficient_scope` | |
| 403 | `confirmation_required` | Cookie-only grant creation without a valid confirmation token from the consent page (§5.3) |
| 404 | `not_found` | Not owned or not readable, or, for a delegated delete, not created by this client. Never 403, matching the existing posture |
| 409 | `idempotency_conflict` | Same key, different body |
| 409 | `journey_shared` | Delegated delete of a journey that has a group share or an active link |
| 429 | `rate_limited` / `daily_limit` | With `Retry-After` |

## 6. Threat model

**L/I** is likelihood / impact, rated before mitigations.

| # | Threat | L/I | Mitigation | Residual |
|---|---|---|---|---|
| T1 | A DS-audience token leaks (ds-mcp log, memory dump) | M/L | 5-minute life; narrow scopes; held in memory only; never logged; grant revoke is instant | Up to 5 min of read and capped create for one user |
| T2 | ds-mcp Redis is dumped, without the keys | M/L | Hashed MCP tokens; encrypted refresh and subject tokens with AAD | Personal data in `itineraryRef` for up to 1 h |
| T3 | Redis plus encryption keys | L/M | The client secret is still needed for exchange or refresh | 14-day `/userinfo` exposure per subject token |
| T4 | Full ds-mcp compromise (keys and secrets) | L/H | No share scope; delete limited to unshared journeys the client created; per-user daily caps (50 saves, 20 deletes); per-client aggregate limit; kill switch; `created_via` and `delegated_deletions` for cleanup; grant list visible to users | Reads, capped saves and capped deletes of assistant-saved journeys for granted users until the kill switch |
| T5 | A user's Claude account or MCP token is stolen | M/M | This is the capability the feature intends, bounded by scopes and caps; the user can revoke in DS or ds-mcp | Same as T4 for that one user |
| T6 | Prompt injection makes the model save or read | M/L | `itineraryRef` only (no free-form legs); no share scope; idempotency; caps; tool annotations; untrusted-content marking | Unwanted extra journeys; the user's own data shown to their own model |
| T6a | Prompt injection makes the model delete | M/M | Delete only unshared journeys this client created (404 or 409 otherwise); `destructiveHint`; 20/day cap; 30-day `delegated_deletions` record shown on Connected apps | Loss of assistant-saved, unshared journeys; recoverable by re-saving from the record |
| T6b | **A stolen DS session cookie (or XSS, or an unattended signed-in browser) creates a grant.** With D-8 as revised, a valid session is enough | M/L | The grant is usable only by tokens minted from the victim's own ds-mcp login (§5.4), so the attacker gains nothing without the victim's Claude or ds-mcp connection as well; confirmation token from a rendered page; `frame-ancestors 'none'`; push notification and 7-day banner; one-click Disconnect; ends at the next Authentik logout (D-7) and after 90 days unused | Persistence of the victim's own assistant access beyond the stolen session, until the user notices, logs out of Authentik, or 90 days pass. The cookie alone already gives more direct access in the web app |
| T7 | ds-mcp mixes up users | L/H | Token chosen only from the verified principal; cache keyed by `sub`; concurrency test; DS enforces `token.sub` | — |
| T8 | Another token type is accepted as delegated (SSO ID token, `distant-signal-mcp` token, `srv-*`, logout token) | L/H | Distinct `iss`/`aud` per verifier; startup guard on pairwise distinctness; `typ` check; scope claim required | — |
| T9 | Another Authentik client exchanges its tokens into `distant-signal-api` | L/H | Federation trusts only `distant-signal-mcp`; only ds-mcp holds the client secret; a grant is still required. Dev spike S6 and S7: foreign subject tokens and clients were refused | Production configuration must match §9.6 |
| T10 | CSRF creates or revokes a grant | L/M | Strict same-origin, SameSite=Lax, POST/DELETE only, scopes from the body, no GET side effects | XSS on DS could create a grant (an XSS can already do worse) |
| T11 | Account-linking CSRF (A fallback only) | M/H | `state` bound to the principal and CSRF cookie; `sub` equality for Authentik users; "linked as" confirmation | — |
| T12 | The cookie path is weakened (a bearer through the proxy, mixed credentials, a delegated caller reaching admin or groups) | L/H | Proxy strips `Authorization` (tested); 400 on mixed; `AuthenticatedUser` unchanged; route list pinned by a test; no `groups` on a delegated principal | — |
| T13 | Mid-lifetime revocation missed by local JWT checks | M/M | Grant lookup per request; `iat` checked against `not_before` and `sessions_invalidated_at` | — |
| T13a | An Authentik logout fails to revoke the grant (D-7) | M/L | Path 1 (DS back-channel) plus path 2 (ds-mcp → `POST /delegated-grants/revoke`, with outbox retry); subject token gone either way | If a session expiring is not notified (S10), the grant lasts until Disconnect or the 90-day expiry, with no subject token to use it |
| T13b | A leaked `srv-ds-mcp` credential is used to revoke grants | L/L | The route only revokes, only for its own client, and reveals nothing | Users re-grant |
| T14 | Flood of writes or reads | M/L | Per-user and per-client GCRA; daily DB cap; the existing journey and pin caps | Per-pod limiter state (as today) |
| T15 | Enumerating other users' journeys | L/M | 404-not-403; ownership in `WHERE`; read-shared only on opted-in groups | — |
| T16 | Phase 0: a share token ends up in transcripts or logs | M/L | The user chose to paste it; ds-mcp redacts it in logs; 30-day TTL; the owner can revoke or regenerate | Anyone the transcript reaches can read that journey until it expires |
| T17 | Phase 0: SSRF through the share-link argument | M/M | ds-mcp parses only `https://<DS origin>/journeys/shared/<token>` or a bare token, checks the token format, and calls its fixed DS base URL; never fetches a user-supplied URL | — |
| T18 | Phase 3: group members' data sent to an LLM vendor without their consent | H/M | Per-group opt-in (default off), notice to members, optional name redaction | Members of opted-in groups |
| T19 | `sub_mode` drift maps tokens to the wrong user | L/H | Fails closed (no grant, so 403); blueprint comment; §9 check; a test that compares `sub` from both providers for one test user | — |
| T20 | Duplicate saves from retries | H/L | `Idempotency-Key` | — |
| T21 | Authentik signing key compromise | L/H | Out of scope; affects every Authentik-based auth equally | — |

## 7. Phased plan

**Phase 0: no linking, no DS auth changes.**

- ds-mcp `get_shared_journey { link }`:
  - Accepts a DS share URL or a bare token, validated as in T17.
  - Calls `GET /Journeys/shared/{token}` with its existing service bearer,
    so the request uses the `svc:mcp` budget.
  - Returns the journey detail. It never echoes or logs the token.
  - Maps 404 to "this link is invalid, expired or revoked".
- `plan_journey` adds an `openInDistantSignal` URL for each search, in the
  `lib/tripPlanUrl.ts` format (D1).
  - DS optional follow-up: a `pick` parameter so `/plan` preselects the
    itinerary, matched on the train UIDs and the date.
- DS work:
  - Record `JourneyDetailResponse` as a stable public shape in
    `docs/api-changelog.md`, since a second consumer now depends on it.
  - Consider putting `GET /Journeys/shared/{token}` in a rate-limit class:
    it is not brute-forceable (256-bit hashed tokens), but every call runs
    `build_journey_detail_response`.

**Phase 1a: the Authentik spike, about a day of work plus Ranma's
production steps.**

- Section 9 is the runbook.
- At the end, record the result of each exit criterion (§9.4) in this spec
  as a dated note, and confirm D2 or switch to the A fallback.
- **Dev part done on 2026-10-14:** D2 confirmed (§9.5). What remains is
  Ranma's production work (§9.6).

**Phase 1b: DS.**

1. **Configuration.** `DELEGATED_CLIENTS` (with `serviceGroup`), the
   verifiers, and the startup guard extended to every client ID and issuer.
2. **Rate limiter.** Issuer routing, and the delegated buckets for read,
   write and delete.
3. **Migrations.**
   - `delegated_grants`;
   - `delegated_deletions`;
   - `journeys.created_via`;
   - a table of hashed, single-use grant confirmation tokens.
4. **Compensating controls for D-8.**
   - The confirmation token issued by the consent page, and
     `frame-ancestors 'none'`.
   - A `grant_created` push-queue item, sent through the notifier's
     existing web-push path.
   - The 7-day banner.
5. **Connected-apps routes and frontend pages.**
   - List, create (session plus confirmation token) and revoke.
   - "Recently deleted by the assistant".
   - Account export.
6. **The extractor.** `DelegatedOrSessionUser`, the error contract, audit
   lines and metrics.
7. **Routes.**
   - `POST /Journeys/itinerary`: atomic and idempotent, with
     `created_via` and the 50/day cap.
   - `GET /Journeys/mine` and `GET /Journeys/{id}` (own journeys only),
     moved to the extractor.
   - **`DELETE /Journeys/{id}` for delegated callers**, under the §5.5a
     rules, with `delegated_deletions` and the 20/day cap.
8. **Revocation.**
   - Admin revoke and back-channel logout revoke grants (path 1).
   - `POST /delegated-grants/revoke` for the client's service identity
     (path 2).
   - `iat` checked against `sessions_invalidated_at`.
   - The 90-day sweep (D-6) and the 30-day purge of `delegated_deletions`.
9. **Tests.**
   - The route list is pinned.
   - Mixed credentials get 400.
   - The proxy strips `Authorization`.
   - Every wrong-token type is rejected.
   - Revocation takes effect on the next request.
   - Scope intersection.
   - Delete matrix: web-created gives 404, shared gives 409, created by
     this client and unshared gives 204.
   - A POST without a confirmation token, or with a reused or expired
     one, gives 403.
   - Creating a grant enqueues the push notification.
10. **Documentation.** Chart values, `docs/session-revocation.md`, and a new
    `docs/connected-apps.md`.

**Phase 1c: ds-mcp.**

1. The exchange client, and the subject token stored encrypted with
   AAD = `sub`.
2. An in-memory DS token cache.
3. `itineraryRef` in `plan_journey`.
4. `save_journey`, `list_my_journeys`, `get_my_journey` and
   **`delete_journey`**.
5. Error mapping.
6. The back-channel handler calls `POST /delegated-grants/revoke`, through
   a Redis outbox that retries for up to 24 h.
7. A two-user concurrency test.

**Ranma-Config.** The Authentik objects in §9.1 (O1–O4), the secret for
ds-mcp, and the DS chart values. This work can be done only by the Ranma
session, because the live Authentik configuration lives there.

**Phase 2: polish.**

- "Saved by assistant" in the DS UI, and a filter.
- Caps tuned from real usage.
- Dashboard panels and alerts.
- Optionally move `PlanTripFlow` to the atomic route.

**Phase 3: group reads (D-4).**

- `allow_assistant_reads` on groups, off by default, with UI.
- `journeys:read_shared` added to `maxScopes` and to the Authentik scope
  mappings.
- `journey_readable_by_delegate`.
- ds-mcp `list_shared_journeys`, with untrusted-content marking.
- A privacy-page update.

**Phase 4, only if needed.**

- The A-fallback flow for ds-mcp identities that do not come from
  Authentik.
- An optional step-up before creating a grant (a maximum login age through
  `max_age` and `auth_time`, off by default), if the T6b assessment changes
  (§5.3).

## 8. Open questions for the user

**Answered on 2026-10-14** (section 0a):

- First round:
  - the approach (D2, with A as fallback);
  - the scopes: save with a 50/day cap, read own, read group through a
    per-group opt-in that is off by default, and delete own;
  - 90-day expiry when unused;
  - Authentik logout revokes the grant.
- Second round:
  - delete limited to unshared journeys the assistant created, as built in
    §5.5a (D-9);
  - one grant per user covering every MCP client (D-10);
  - the proposed limits, and audit retention with no extra table (D-11);
  - the grant is kept on a DS-only logout (D-12);
  - D-8 revised: a valid DS session is enough to create a grant.

**Still open:**

1. **Group-read details** (Phase 3). Can only owners and admins toggle
   `allow_assistant_reads`? And should other members' journey names be
   redacted even in groups that opted in?
2. **Public ingress.** Should DS refuse delegated bearers when the api is
   reached through a public ingress (`ingress.api.enabled`)?
3. **The web planner.** Should `PlanTripFlow` move to
   `POST /Journeys/itinerary`?
4. **Privacy page.** Who updates the privacy page to cover connected apps
   and processing by a third-party LLM?
5. **Spec index.** `docs/superpowers/README.md` has no rows for specs after
   2026-10-06. Should it be brought up to date in a separate pass?

## 9. Authentik spike runbook (before Phase 1)

The facts this design takes from Authentik come from the Ranma-Config spec
and Authentik's docs. This runbook checks them on a real instance.

- **[DS]** steps run against the dev Authentik in this repository:
  `docker-compose.authentik.yml`, `ghcr.io/goauthentik/server:2026.8.0`,
  with blueprints in `authentik-blueprints/`.
- **[Ranma]** steps change or test the production Authentik
  (`https://sso.cursed.solutions`, 2026.8.1). Its configuration lives in
  Ranma-Config (`clusters/mine-bringer/apps/authentik.yaml`), not in this
  repository, so only the Ranma session may do them.
- **[ds-mcp]** steps need ds-mcp's code or credentials.

Field names below follow Authentik's documentation. The blueprint author
confirms each one against the 2026.8 model source before relying on it
(item S0).

### 9.1 Authentik objects

| Id | Object (blueprint model) | Settings | Dev [DS] | Prod [Ranma] |
|---|---|---|---|---|
| O1 | 4 scope mappings (`authentik_providers_oauth2.scopemapping`) | Names `ds-journeys-read`, `ds-journeys-write`, `ds-journeys-delete`, `ds-journeys-read-shared`, with `scope_name` `journeys:read`, `journeys:write`, `journeys:delete`, `journeys:read_shared`. Each has a consent-screen `description` written as a sentence ("Save journeys to your Distant Signal account"). The expression is `return {}` if S4 shows that the access token's `scope` claim lists the granted scopes. Otherwise it is a distinct claim per scope, for example `return {"ds_scope_journeys_write": True}`. Distinct keys are needed because mappings that return the same key overwrite one another | yes | yes (`read_shared` can wait until Phase 3) |
| O2 | Provider `distant-signal-api` (`authentik_providers_oauth2.oauth2provider`) | `client_type: confidential`; `client_id: distant-signal-api`; `client_secret` from the sealed Secret; `grant_types: [token_exchange]` (the spike also enables `authorization_code` and `refresh_token` to test fallback A, and production enables them only if A is chosen); `access_token_validity: minutes=5`; `refresh_token_validity: days=30` (A only); `sub_mode: hashed_user_id`, with a comment saying it must match `distant-signal` and `distant-signal-mcp`; `issuer_mode: per_provider`; `signing_key`: the RSA certificate the other providers use; `property_mappings`: O1 (plus `openid` for A); the token-exchange trust list (`jwt_federation_providers`, confirmed against the 2026.8.0 source and on dev) set to `[distant-signal-mcp]` only; `authorization_flow: default-provider-authorization-explicit-consent` (consent matters for A); `invalidation_flow: default-provider-invalidation-flow`; `redirect_uris` (A only), strict: `https://ds-mcp.cursed.solutions/oauth/distant-signal-api/callback`; **no** `logout_uri` | yes | yes |
| O3 | Application `distant-signal-api` (`authentik_core.application`) | Provider O2. No policy binding: open to every Authentik user, like the `distant-signal-mcp` application (the spike bound it to `distant-signal-chatbot-users`; that group is gone) | yes | yes |
| O4 | Secret | Ranma-Config seals `ds-mcp-secret` / `distant-signal-api-client-secret` (namespace `ds-mcp`), plus the Authentik copy, in the same way as `authentik-client-secret` | fixed dev value | yes |
| O5 | Spike only: provider and application `ds-spike-rogue` | A confidential client using authorization code, used to prove that foreign tokens cannot be exchanged. Delete it after the spike | yes | yes, then delete |
| O6 | Spike only: test users | `spike-local` (local, in `distant-signal-chatbot-users`); `spike-nogroup` (not in the group). In production only, `spike-discord` (from the Discord source), if a test Discord account is available | yes (local users) | yes, then delete |
| — | Existing `distant-signal` and `distant-signal-mcp` providers | **No change.** Only record their `sub_mode` (expected `hashed_user_id`) and access-token validity (14 days) | check | check |

### 9.2 Test steps

Wherever a step obtains a `distant-signal-mcp` access token, use an
authorization-code flow with PKCE as `spike-local`, with that provider's
dev credentials, or with ds-mcp in production. A small local script in the
session scratchpad, or `curl`, is enough. Decode JWTs locally and never
paste them anywhere.

| Step | Who | Action | Expected / record |
|---|---|---|---|
| S0 | DS (dev), Ranma (prod) | Apply O1–O6. Confirm each field name against the 2026.8 model, and check that the blueprint applies without serializer errors | Blueprint applied; the `distant-signal-api` discovery document and JWKS are served |
| S1 | DS | Log in as `spike-local` to `distant-signal` and to `distant-signal-mcp`, then compare `sub` | Equal. Record `sub_mode` on both |
| S2 | DS | Exchange: `POST /application/o/token/` with `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`, `subject_token=<distant-signal-mcp AT>`, `subject_token_type=urn:ietf:params:oauth:token-type:access_token`, `requested_token_type=urn:ietf:params:oauth:token-type:access_token`, `scope=journeys:read journeys:write journeys:delete`, and Basic auth as `distant-signal-api` | 200 and an RS256 JWT. Record `iss`, `aud` (= `distant-signal-api`), `sub` (= S1), `iat`, `exp`, `jti`, `azp`, the scope claims and `act` |
| S3 | DS | Delegation mode: repeat S2 with the actor parameters the Authentik docs give for on-behalf-of | The token carries `act` identifying ds-mcp. Record its exact shape for the `actor` setting in `DELEGATED_CLIENTS`. If there is no `act`, D2 still works and DS identifies the client by `aud` alone |
| S4 | DS | Scopes: request only `journeys:read`, then a scope that is not mapped | The first token shows only that scope. The second gets an error or has the scope dropped. Decide the O1 expression style from this |
| S5 | — | **Removed** with the revised D-8 (no `max_age`). If the optional Phase 4 step-up is ever built, restore this step: check that `max_age=600` forces a credential prompt and refreshes `auth_time` | — |
| S6 | DS | Foreign subject tokens: exchange (as in S2) a `ds-spike-rogue` access token, a `distant-signal` access token, and a `srv-ds-mcp` client-credentials token | **All refused.** If any succeeds, the trust list is not working: stop and treat D2 as blocked (§9.4) |
| S7 | DS | Wrong requesting client: do S2 authenticated as `ds-spike-rogue`, and as `distant-signal-mcp` | No token with `aud = distant-signal-api` is issued |
| S8 | DS | Group policy: obtain a subject token as `spike-local`, remove the user from `distant-signal-chatbot-users`, then exchange | Record whether the application's binding is checked at exchange. Either result is acceptable, because the DS grant gates use, but the result goes in the spec |
| S9 | DS | Inactive or revoked subject: (a) deactivate `spike-local` and exchange with an earlier subject token; (b) revoke a subject token at `distant-signal-mcp`'s revocation endpoint and exchange | Both refused |
| S10 | DS (dev), Ranma (prod) | Session end: (a) log out of Authentik in a browser session that has logged in to both DS and ds-mcp; (b) delete one session from the admin UI; (c) let a session expire (set a short session duration on a test flow) | (a) and (b): both back-channel calls fire (worker log), and a subject token from that session can no longer be exchanged. (c): record whether a notification is sent. This fixes the residual risk for T13a |
| S11 | DS | Lifetime | Exchanged token `exp - iat = 300`, whatever the subject token's 14 days |
| S12 | DS | Fallback A: authorization code with PKCE against `distant-signal-api`, with the explicit-consent flow | The consent screen lists the O1 descriptions. Refresh rotates the refresh token, and a reused refresh token is refused. A `resource` parameter (RFC 8707) is ignored or refused; record which |
| S13 | DS | Verification on the DS side: run a captured S2 token, and a captured `distant-signal` token, through a scratch harness that uses DS's `ServiceTokenVerifier` with the `distant-signal-api` issuer and audience | The S2 token verifies. The SSO token is refused (`iss`/`aud`) |
| S14 | Ranma | Repeat S1, S2, S6, S9 and S10 on production, plus S1 and S2 for `spike-discord`. §9.6 step 3 is the minimum | Same results as on dev |
| S15 | Ranma, DS | Clean-up: delete O5 and O6 everywhere, and keep O1–O4 | Only the intended objects remain |

### 9.3 What each side owns

- **DS session.**
  - The dev blueprint entries (O1–O3, O5, O6) in `authentik-blueprints/`.
    These are committed only once the spike passes; the dev values are not
    secrets.
  - S0–S13 on dev.
  - Recording the results in this spec.
- **Ranma session.**
  - Every production Authentik object (O1–O4, and the temporary O5 and
    O6).
  - The sealed secret.
  - The production steps in §9.6.
  - S15.
- **ds-mcp session.** Nothing is needed for the spike itself, beyond
  production S2 using ds-mcp's real `distant-signal-mcp` client if the Ranma
  session prefers that to a test client. The exchange client is Phase 1c
  work.

### 9.4 Exit criteria

| Result | Decision |
|---|---|
| S2, S6, S7, S9 and S11 pass | **D2 confirmed.** Build Phase 1 as specified |
| S6 or S7 fails (foreign subject tokens or clients accepted) | D2 is blocked: exchange cannot be limited to ds-mcp. **Switch to fallback A** (S12 must pass), and keep every DS-side design unchanged |
| S3 gives no `act` | Accepted. DS identifies the client by `aud`, and drops the `actor` check. **This happened on dev** (§9.5) |
| S4 shows no scope claim on the token | DS uses the per-scope claims (O1 fallback). If there are none at all, DS uses grant scopes ∩ `maxScopes` only, and records that in the spec |
| S10(c): no notification on expiry | Accept and record the T13a residual risk |
| S1 or S14: `sub` differs anywhere | Stop. Fix `sub_mode` before going further, because the identity mapping depends on it |

### 9.5 Dev spike results (2026-10-14)

**Verdict: D2 is viable. Every gating step passed (S2, S6, S7, S9, S11), so
there is no need to fall back to A.** One adjustment follows: DS does not
check `act`, because no `act` claim appears without an Authentik "Actor"
object (S3).

**Environment**

- The image `docker-compose.authentik.yml` pins:
  `ghcr.io/goauthentik/server:2026.8.0`, amd64 manifest
  `sha256:21000ceb…0da0`, with Postgres 16-alpine at the same digest.
- The compose file could not be used as it stands, for two reasons. This
  workspace has Podman with no compose provider, and Podman runs as a
  remote service that cannot bind-mount the session scratchpad. So the
  same images and environment were started by hand, on a private Podman
  network, with Authentik published on `127.0.0.1:19480`. Blueprints were
  copied into a named volume.
- The repository's `authentik-blueprints/oauth2-client.yaml` was loaded
  alongside a throwaway spike blueprint. It applied cleanly
  (`distant-signal-dev`, `hashed_user_id`, `authorization_code`).
- The spike blueprint modelled the three providers with a `spike-` prefix:
  - `spike-distant-signal` (14-day tokens, back-channel logout);
  - `spike-distant-signal-mcp` (14-day tokens, refresh, back-channel
    logout);
  - `spike-distant-signal-api` (O2: 5 minutes, token exchange, federation
    trusting `spike-distant-signal-mcp`, explicit consent, the four O1
    mappings).
- It also created the rogue provider (O5), the test users (O6), and
  application bindings to `distant-signal-chatbot-users`.
- Logins and consent were driven through Authentik's flow-executor API.
  A local HTTP listener recorded the back-channel logout POSTs.
- Everything was torn down afterwards (containers, volumes, network).
  Nothing touched production, and the spike scripts were left in the
  session scratchpad, not this repository.

The 2026.8.0 source in the image confirmed several details beyond the docs:

- The trust list field is `jwt_federation_providers`
  (`providers/oauth2/models.py`).
- Exchange is implemented in `providers/oauth2/token/token_exchange.py`.
- Subject tokens are looked up as live `AccessToken` rows and then
  signature-checked (`token/base_fed.py`).
- `audience` must name a provider that federates with the requesting one,
  and `resource` (RFC 8707) is refused with `invalid_target`.
- Requested scopes are clamped to the target provider's scope mappings.
- The application's policy bindings are evaluated at exchange time.
- `act` is set only for an `actor_token` that resolves to an Authentik
  `Actor`.
- Exchanged tokens are written with `auth_time = now` and no session.

| Step | Result | Evidence |
|---|---|---|
| S0 | **Pass** | Both blueprints applied with no serializer errors. The field names used in §9.1 are right, the federation field included (`jwt_federation_providers: [<mcp pk>]`). Discovery is served for all three providers. Discovery's `grant_types_supported` does **not** list token exchange, even though it works (cosmetic) |
| S1 | **Pass** | `sub` is `a4022ea6…65de` in the DS and ds-mcp access tokens, and in both ID tokens. All providers are `hashed_user_id` |
| S2 | **Pass** | `client=spike-distant-signal-api` with `subject_token=<mcp AT>` gives 200 and an RS256 JWT, with `typ: JWT` and a `kid`. Claims: `iss=…/application/o/spike-distant-signal-api/`, `aud=azp=spike-distant-signal-api`, `sub` unchanged, `scope="journeys:delete journeys:read journeys:write"`, `jti` present, **no `sid`**, and `auth_time` = **time of the exchange**. Response: `issued_token_type=…:access_token`, `expires_in=300`. `subject_token_type=…:jwt` works too. The variant with `client=spike-distant-signal-mcp` and `audience=spike-distant-signal-api` also works and gives the same claims (see the note below) |
| S3 | **No `act`** (accepted) | No `actor_token` means no `act` claim. Using the user's own ds-mcp token as `actor_token` gives 400 `invalid_grant`: actors must be Authentik `Actor` objects. **Decision:** DS identifies the client by `iss`/`aud` and the federation setting, and drops the `act` check (§5.7) |
| S4 | **Pass** | The `scope` claim is present, so O1's `return {}` is enough, and the per-scope claim fallback is not needed. Asking for `journeys:read` gives exactly that. An unknown scope is silently dropped. An **empty `scope` still issues a token with `scope=""`**, so DS must refuse tokens that lack the route's scope (as §5.5's intersection already does) |
| S5 | **Removed** | D-8 as revised has no `max_age` |
| S6 | **Pass** | Each of these is refused with 400 `invalid_grant`: a rogue access token, a DS SSO access token, a ds-mcp ID token, a DS ID token, an already-exchanged `distant-signal-api` token, and a tampered ds-mcp token. Not run: a `srv-ds-mcp` client-credentials token, because no service account was set up. It is issued by `distant-signal-internal`, which is not in the federation list, and so takes the same refusal path as the DS SSO token |
| S7 | **Pass** | `client=rogue`, `audience=api`: 400 `invalid_target`. `client=rogue` with no audience: `invalid_grant`. `client=DS` (no exchange grant): `invalid_grant`. `client=api` with a wrong secret: `invalid_client`. `client=mcp` targeting itself: `invalid_grant` |
| S8 | **Binding enforced** | Removing `spike-local` from `distant-signal-chatbot-users` makes the next exchange fail with 400 `invalid_grant`. Adding the user back makes it succeed again. Group membership is checked at every exchange |
| S9 | **Pass** | (a) Deactivating the user makes exchange fail, and **after reactivating, the same subject token still fails**, because deactivation deletes the sessions and with them the tokens. A fresh login works. (b) Revoking the ds-mcp access token at `/application/o/revoke/` makes exchange fail |
| S10 | **Pass** ((c) not run) | (a) A user logout (invalidation flow) sends back-channel POSTs to both `/ds` and `/mcp`, and the logged-out session's ds-mcp token can no longer be exchanged. The window also caught notifications that S9's deactivation had queued and delivered late, on this heavily loaded host. (b) An admin deleting the sessions gives exactly one `/ds` and one `/mcp` POST, with logout-token claims `iss`, `aud`, `iat`, `exp`, `jti`, `events`, `sub` and `sid`, after which exchange fails. **Exchanged tokens carry no `sid` and outlive the session for up to 5 minutes.** That is why §5.7 checks `iat` against `sessions_invalidated_at`, and path 1 and path 2 revoke the grant. (c) Session expiry was not run, and the T13a residual risk stands as written |
| S11 | **Pass** | The exchanged token has `exp − iat = 300` and `expires_in = 300`. The subject token has 1,209,600 (14 days) |
| S12 | **Pass** | Fallback A works: authorization code with PKCE and the explicit-consent flow. The consent challenge lists the O1 descriptions ("Read your saved journeys"). Refresh rotates the refresh token, and reusing the old one gives 400 `invalid_grant`. Auth-code tokens carry `sid` and the real `auth_time`. `resource` on `/authorize` is ignored, and on exchange it is refused (`invalid_target`) |
| S13 | **Pass (equivalent harness)** | The checks `ServiceTokenVerifier` makes (RS256 against the provider JWKS, exact `iss`, `aud` contains the client, `exp` with 60 s leeway) were re-implemented with PyJWT and run against live tokens. The exchanged token verifies, and the DS SSO and ds-mcp login tokens are rejected (`InvalidIssuerError`). The Rust type itself was not compiled into a scratch harness, because the host's load average was 80–100 and building `crates/api` was not practical. Its unit tests already cover the same claim checks |

**Which client authenticates the exchange.** Both shapes work:

- (i) ds-mcp authenticates as `distant-signal-api`, the API provider trusts
  `distant-signal-mcp`, and ds-mcp holds both client secrets.
- (ii) ds-mcp authenticates as `distant-signal-mcp`, with
  `audience=distant-signal-api`. This adds the exchange grant to ds-mcp's
  login provider, and needs no second secret.

**Recommendation: (i), as specified.** The separate `distant-signal-api`
secret is an Authentik-side kill switch for delegated access alone.
Rotating it stops every exchange while ds-mcp's logins keep working. It
also leaves the production `distant-signal-mcp` provider unchanged.

**What changes in §5 as a result**

- DS drops the `actor`/`act` check.
- DS requires the route's scope in the token's `scope` claim. A token with
  an empty scope is never enough.
- Since exchanged tokens have no `sid` and their `auth_time` is the
  exchange time, DS uses `iat` for the `not_before` and
  `sessions_invalidated_at` comparisons (as specified), never `auth_time`.

**Remaining production steps for Ranma** are in §9.6.

### 9.6 Production steps for Ranma (exact)

Order matters: each step depends on the previous one. None of these has
been done.

1. **Seal the client secret.**
   - Generate a 64-character random secret.
   - Seal it twice, as with `distant-signal-mcp`: as `ds-mcp-secret` /
     `distant-signal-api-client-secret` in namespace `ds-mcp`, and as the
     Authentik-side copy.
2. **Add the blueprint entries** to `clusters/mine-bringer/apps/authentik.yaml`,
   gated on the secret env var being non-empty, like `srv-ds-mcp`:
   - **O1:** `authentik_providers_oauth2.scopemapping` × 3 (Phase 1):
     - `ds-journeys-read` / `scope_name: journeys:read` / description
       "Read your saved journeys";
     - `ds-journeys-write` / `journeys:write` / "Save journeys to your
       Distant Signal account";
     - `ds-journeys-delete` / `journeys:delete` / "Delete journeys the
       assistant saved";
     - all with `expression: return {}`.
     - Add `ds-journeys-read-shared` / `journeys:read_shared` only for
       Phase 3.
   - **O2:** `authentik_providers_oauth2.oauth2provider`
     `distant-signal-api`:
     - `client_type: confidential`, `client_id: distant-signal-api`,
       `client_secret: !Env …`;
     - `grant_types: ["urn:ietf:params:oauth:grant-type:token-exchange"]`
       only (do not add `authorization_code` or `refresh_token` unless
       fallback A is ever chosen);
     - `access_token_validity: minutes=5`;
     - `sub_mode: hashed_user_id`, with a comment that it must match
       `distant-signal` and `distant-signal-mcp`;
     - `jwt_federation_providers: [!Find [authentik_providers_oauth2.oauth2provider, [client_id, distant-signal-mcp]]]`;
     - `signing_key`: the same RSA certificate as the other providers;
     - `authorization_flow`: `default-provider-authorization-explicit-consent`
       and `invalidation_flow`: `default-provider-invalidation-flow`, both
       required by the serializer;
     - `property_mappings`: the three O1 mappings;
     - `redirect_uris: []`;
     - no `logout_uri`.
   - **O3:** `authentik_core.application` `distant-signal-api`, with
     provider O2 and no policy binding.
     - The application is **required**: Authentik refuses an exchange whose
       target provider has no application (`target_without_application`).
     - Open to every Authentik user, like `distant-signal-mcp` (unbound
       since 2026-09-30). S8 showed a binding would be enforced at every
       exchange, so adding one back is the way to narrow it again.
   - **No change** to `distant-signal-mcp` or `distant-signal`.
3. **Smoke test on production** (S14, shortened), with a temporary test
   user (any Authentik account):
   1. Obtain a `distant-signal-mcp` access token, through ds-mcp's login or
      a one-off authorization-code run.
   2. `POST https://sso.cursed.solutions/application/o/token/`, with Basic
      auth `distant-signal-api:<secret>` and the form fields
      `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`,
      `subject_token=<AT>`,
      `subject_token_type=urn:ietf:params:oauth:token-type:access_token`
      and `scope=journeys:read journeys:write journeys:delete`.
   3. Expect 200 and a JWT with `iss=https://sso.cursed.solutions/application/o/distant-signal-api/`,
      `aud=distant-signal-api`, the same `sub` as the subject token, and
      `exp − iat = 300`.
   4. Repeat with a `distant-signal` (DS SSO) access token as the subject.
      Expect 400 `invalid_grant`.
   5. Deactivate the test user and repeat step 2. Expect 400.
   6. Delete the test user.
   7. Record the results as a dated note in §9.5. Paste no tokens anywhere.
4. **Hand over to the DS and ds-mcp sessions:**
   - the issuer URL `https://sso.cursed.solutions/application/o/distant-signal-api/`;
   - the audience `distant-signal-api`;
   - the secret's location for ds-mcp.

   DS's chart then sets `api.delegatedClients[0]` (§5.2) without `actor`.
