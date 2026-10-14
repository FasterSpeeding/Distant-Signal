# ds-mcp delegated access to a user's Distant Signal journeys (2026-10-14)

**Status: design proposal, not approved.** This is a design and security
review written so the user can decide whether to build the feature. Nothing
here is implemented, and no code was changed to write it.

Requested by the user. The aim: a user who plans a trip with the
Distant-Signal-MCP server's (ds-mcp's) `plan_journey` tool can save the
itinerary they pick into their own Distant Signal (DS) account, and list
their saved journeys. A later phase may also let them read journeys that
other people have shared with them through groups.

## Contents

0. Summary and recommendation
1. Goal and non-goals
2. What exists today (verified)
3. Why the 2026-09-29 bridge was removed, and the constraints that follow
4. Options compared
5. Recommended design
6. Threat model
7. Phased plan
8. Open questions for the user
9. Things to verify on the real Authentik before Phase 1

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
  it into saving or reading.
- **Privacy in Phase 3.** Group reads would send other people's journeys to
  an LLM vendor.

The design limits each of these. Scopes are narrow and there is no delete.
Every save goes through an itinerary reference held by ds-mcp. There are
per-user caps and a DS-side kill switch. Group reads need an opt-in on each
group.

## 1. Goal and non-goals

Goal:

- A ds-mcp tool saves an itinerary that `plan_journey` returned into the
  calling user's DS account, as a tracked journey.
- A ds-mcp tool lists that user's saved journeys and reads one.
- Later: read the journeys that groups share with that user.

Non-goals:

- Any other DS write through ds-mcp: deleting journeys, share links,
  group management, preferences, the account, train pins outside a journey,
  or journey templates.
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
token carries an `act` claim naming ds-mcp.

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
                 DS api ── verify iss/aud/exp/act/scope ── delegated_grants row
                           (user_id = sub, client = ds-mcp, active, scopes)
                           ── handler runs as that user
```

The user creates the grant once, in DS:

1. ds-mcp's first save returns a tool message with a link to
   `https://<ds>/account/connected-apps/ds-mcp`.
2. The user logs in to DS if needed. That creates their DS user row if this
   is their first visit.
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
- Scope mappings: `ds-journeys-read`, `ds-journeys-write` and (Phase 3)
  `ds-journeys-read-shared`. Each emits its scope name into the token's
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
    actor: distant-signal-mcp    # required `act.sub`/`act.client_id` when delegation mode is used
    maxScopes: [journeys:read, journeys:write]
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
    revoked_reason  TEXT                      -- user | admin | backchannel | inactive | client_disabled
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

**Unused grants expire after 90 days** (proposed). A maintenance pass marks
grants whose `last_used_at` (or, if never used, `created_at`) is older than
90 days as revoked with reason `inactive`. See open question 7.

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
| `journeys:read_shared` | `GET /groups/shared-journeys`; `GET /Journeys/{id}` through group membership, only for groups that opted in (§5.13) | 3 |

DS computes the effective scopes per request as the intersection of three
sets, and each route asks for one scope:

- the scopes in the token's claim;
- the grant's `scopes`;
- the client's `maxScopes`.

There is deliberately no delete scope in v1 (open question 5). A model that
is acting on injected text should not be able to destroy data. There is
also no share-link scope: minting a public URL is an exfiltration channel.

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
or `ds_scopes`, `act`, `iat` and `jti`. The checks:

1. Signature, `iss`, `aud`, `exp` and `nbf`, as now.
2. `iat` is present.
3. `act` names the client's `actor`, if delegation mode is in use.
4. The `typ` header is not `logout+jwt`.

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
  account, preferences, auth, admin, share links, delete, templates,
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
- **A delegated principal has no `groups`.** `ChatbotAuthorizedUser`, the
  admin check and anything else that reads `users.groups` keep taking
  `AuthenticatedUser`, so a delegated caller can never become an admin or a
  chatbot user.
- **CORS is unchanged**: GET only, no credentials, `Authorization` not
  allowed in the preflight. A browser on another origin cannot send a
  bearer to the api even if it holds one.
- **Reachability.** ds-mcp reaches the api in-cluster; the api's NetworkPolicy
  already narrows extra-namespace ingress to the MCP pod (`4068cbff`). If
  `ingress.api.enabled` exposes the api publicly, delegated bearers would
  also be accepted there. Open question 10 asks whether to refuse delegated
  bearers on public ingress.

### 5.8 Which routes gain bearer auth

| Route | Today | Delegated scope | Phase | Notes |
|---|---|---|---|---|
| `POST /Journeys/itinerary` (**new**) | — | `journeys:write` | 1 | Creates a journey and 1–8 `knownTrain` legs in **one transaction**. Optional `customName`. Requires an `Idempotency-Key` header from delegated callers; the key is remembered per user for 24 h and a repeat returns the original response. Checks the train-pin cap and the journey cap. Also available with a cookie, so `PlanTripFlow` can drop its non-atomic loop (open question 11) |
| `GET /Journeys/mine` | cookie | `journeys:read` | 1 | Unchanged response |
| `GET /Journeys/{id}` | cookie (`journey_readable_by`) | `journeys:read` (own only) / `journeys:read_shared` | 1 / 3 | For a delegated caller without `read_shared`, ownership only. `shareLink` is always `null` for a delegated caller |
| `GET /groups/shared-journeys` | cookie | `journeys:read_shared` | 3 | Filtered to groups that opted in |
| `POST /Journeys`, `POST /Journeys/{id}/legs` | cookie | **none** | — | Superseded for delegated callers by the atomic route |
| `DELETE /Journeys/...`, share-link routes, candidates and train picks, every `/groups` write, `/account`, `/preferences`, `/auth`, `/admin`, templates, `/Train/*` | cookie | **never** | — | |

`journeys` gains a `created_via TEXT NOT NULL DEFAULT 'web'` column holding
`'web'` or the client name. The journey page shows "Saved by Distant Signal
assistant", and the user's list can filter on it.

### 5.9 Revocation

From the DS side, every path is instant on the next request, because the
grant row is checked on every request:

| Trigger | DS action | ds-mcp effect |
|---|---|---|
| User presses **Disconnect** on Connected apps | `revoked_at = now()`, reason `user` | Next DS call: 403 `delegation_not_granted`, and the tool tells the user how to grant again |
| Admin `POST /admin/users/revoke-sessions` | Also revokes every grant for the user, reason `admin`. An admin revoke is a security action and should cut the assistant too | as above |
| Authentik back-channel logout on `distant-signal` (user logged out of Authentik, deactivated, or session deleted) | Already sets `sessions_invalidated_at`. Through §5.7 step 2.3, every outstanding DS-audience token stops working at once. The grant is **kept** | ds-mcp's own back-channel logout (`distant-signal-mcp` provider) fires for the same session and runs `revokeAllForSub`, so ds-mcp can no longer exchange. After the user signs in to Claude again, saving works without another DS consent. This follows R1: an ordinary logout should not undo a deliberate grant |
| Deactivated user | as above. Authentik also refuses exchanges and refreshes for an inactive user (to verify, §9) | |
| User "log out other sessions" | Sets `sessions_invalidated_at`, so outstanding DS-audience tokens die. The grant is kept | Re-exchanges and carries on. Disconnect is the control for cutting the assistant |
| Account deletion | Cascades | |
| Client disabled (`enabled: false`) or removed from `DELEGATED_CLIENTS` | Every delegated request for it gets 401 | The kill switch for a ds-mcp compromise |
| 90 days unused | reason `inactive` | |
| Grant re-created after a revoke | New row with `not_before = now()`, so tokens minted before the new consent are refused | |

DS does **not** set a back-channel logout URI on the `distant-signal-api`
provider. That provider's 5-minute tokens would make Authentik's
notification unreliable (§2.3: it sends only while an access token is
unexpired), and DS already learns about the same sessions through its own
provider's notification.

**From the ds-mcp side.** A `disconnect_distant_signal` tool (Phase 2) calls
`DELETE /account/connected-apps/ds-mcp`. That route is cookie-only, so this
would need a dedicated delegated route with its own scope. **Recommendation:
don't build it.** The tool should return the Connected-apps link instead, so
that revocation always happens in DS's own UI.

### 5.10 Rate limits: per user and per service

- **Today:** `svc:mcp` is one bucket for all of ds-mcp's anonymous public
  reads, five times the public defaults.
- **New:** delegated requests are charged to **two** buckets, and both must
  pass:
  - `user:<sub>` per class:
    - `delegated_write`: suggested 10/min, burst 5;
    - `delegated_read`: suggested 60/min, burst 20.
  - `svc:<client>-delegated` per class: an aggregate ceiling for the whole
    client, suggested 20 times the per-user limit. This bounds a runaway
    loop across many users, or a compromised client.
- Delegated requests never use `svc:mcp` or the per-IP buckets.
- **Daily per-user creation cap** for delegated callers: at most 50
  journeys per rolling 24 h with `created_via = <client>`. This is a
  database count, alongside the existing `MAX_JOURNEYS_PER_USER`. GCRA in
  memory per pod cannot express a daily cap.
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
  - Whether to add a `delegated_audit` table is open question 9.

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
     - there is no delete and no share-link scope;
     - per-user caps;
     - ds-mcp marks `save_journey` with MCP tool annotations
       (`readOnlyHint: false`, `destructiveHint: false`,
       `idempotentHint: true`), and clients may ask the user to confirm.
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
  in. Treat this as open question 4.
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
- **`list_my_journeys`** and **`get_my_journey { id }`**: plain reads.
- **Phase 3: `list_shared_journeys`.**
- **Phase 0: `get_shared_journey { link }`** (section 7).

**Errors** go back to the model as tool results, never as HTTP errors on
`/mcp` (R3):

| Response from DS | ds-mcp action |
|---|---|
| 401 | Exchange again once, then report "sign in again" |
| 403 `delegation_not_granted` | Return the Connected-apps link |
| 403 `insufficient_scope` | Return the link and name the missing scope |
| 429 | Pass on `Retry-After` |

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
    `journeys:read`/`journeys:write`.
  - They can read the users' journeys and create junk journeys, up to
    50 per user per day.
  - They cannot delete, share, touch groups or the account, or log in to
    DS.
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
- See open question 2 for whether ds-mcp should ask again for each MCP
  client.

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
| 404 | `not_found` | Not owned or not readable. Never 403, matching the existing posture |
| 409 | `idempotency_conflict` | Same key, different body |
| 429 | `rate_limited` | With `Retry-After` |

## 6. Threat model

**L/I** is likelihood / impact, rated before mitigations.

| # | Threat | L/I | Mitigation | Residual |
|---|---|---|---|---|
| T1 | A DS-audience token leaks (ds-mcp log, memory dump) | M/L | 5-minute life; narrow scopes; held in memory only; never logged; grant revoke is instant | Up to 5 min of read and capped create for one user |
| T2 | ds-mcp Redis is dumped, without the keys | M/L | Hashed MCP tokens; encrypted refresh and subject tokens with AAD | Personal data in `itineraryRef` for up to 1 h |
| T3 | Redis plus encryption keys | L/M | The client secret is still needed for exchange or refresh | 14-day `/userinfo` exposure per subject token |
| T4 | Full ds-mcp compromise (keys and secrets) | L/H | Scopes without delete or share; per-user daily cap; per-client aggregate limit; kill switch; `created_via` for cleanup; grant list visible to users | Reads and capped writes for granted users until the kill switch |
| T5 | A user's Claude account or MCP token is stolen | M/M | This is the capability the feature intends, bounded by scopes and caps; the user can revoke in DS or ds-mcp | Same as T4 for that one user |
| T6 | Prompt injection makes the model save or read | M/L | `itineraryRef` only (no free-form legs); no delete or share; idempotency; caps; tool annotations; untrusted-content marking | Unwanted extra journeys; the user's own data shown to their own model |
| T7 | ds-mcp mixes up users | L/H | Token chosen only from the verified principal; cache keyed by `sub`; concurrency test; DS enforces `token.sub` | — |
| T8 | Another token type is accepted as delegated (SSO ID token, `distant-signal-mcp` token, `srv-*`, logout token) | L/H | Distinct `iss`/`aud` per verifier; startup guard on pairwise distinctness; `typ` check; scope claim required | — |
| T9 | Another Authentik client exchanges its tokens into `distant-signal-api` | L/H | Federation trusts only `distant-signal-mcp`; only ds-mcp holds the client secret; `act` check; a grant is still required | Depends on Authentik configuration (§9) |
| T10 | CSRF creates or revokes a grant | L/M | Strict same-origin, SameSite=Lax, POST/DELETE only, scopes from the body, no GET side effects | XSS on DS could create a grant (an XSS can already do worse) |
| T11 | Account-linking CSRF (A fallback only) | M/H | `state` bound to the principal and CSRF cookie; `sub` equality for Authentik users; "linked as" confirmation | — |
| T12 | The cookie path is weakened (a bearer through the proxy, mixed credentials, a delegated caller reaching admin or groups) | L/H | Proxy strips `Authorization` (tested); 400 on mixed; `AuthenticatedUser` unchanged; route list pinned by a test; no `groups` on a delegated principal | — |
| T13 | Mid-lifetime revocation missed by local JWT checks | M/M | Grant lookup per request; `iat` checked against `not_before` and `sessions_invalidated_at` | — |
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

**Phase 1a: Authentik spike, about a day.**

- Run every item in section 9 against a dev Authentik 2026.8 with the
  blueprint.
- Decide between exchange (D2) and the A fallback for launch.

**Phase 1b: DS.**

1. `DELEGATED_CLIENTS` config, the verifiers, the startup guard extension.
2. Rate-limiter issuer routing and the delegated buckets.
3. The `delegated_grants` migration and data module.
4. Connected-apps routes and frontend pages, plus account export.
5. The `DelegatedOrSessionUser` extractor, the error contract, audit lines
   and metrics.
6. `POST /Journeys/itinerary`, which is atomic and idempotent, with
   `created_via`.
7. Opt `GET /Journeys/mine` and `GET /Journeys/{id}` (own only) into the
   extractor.
8. Revocation hooks: admin revoke and `iat` against
   `sessions_invalidated_at`.
9. Tests:
   - the route list is pinned;
   - mixed credentials get 400;
   - the proxy strips `Authorization`;
   - every wrong-token type is rejected;
   - revocation takes effect on the next request;
   - scope intersection.
10. Chart values, `docs/session-revocation.md` and a new
    `docs/connected-apps.md`.

**Phase 1c: ds-mcp.**

1. The exchange client and the subject token kept encrypted.
2. An in-memory DS token cache.
3. `itineraryRef` in `plan_journey`.
4. `save_journey`, `list_my_journeys`, `get_my_journey`.
5. Error mapping.
6. A two-user concurrency test.

**Ranma-Config.**

- The `distant-signal-api` provider.
- The secret for ds-mcp.
- DS chart values.

**Phase 2: polish.**

- "Saved by assistant" in the DS UI and a filter.
- A per-user daily cap set from real usage.
- Dashboard panels and alerts.
- The 90-day inactive-grant sweep.
- Optionally move `PlanTripFlow` to the atomic route.

**Phase 3: group reads.**

- `allow_assistant_reads` on groups, with UI.
- `journeys:read_shared`.
- `journey_readable_by_delegate`.
- ds-mcp `list_shared_journeys`, with untrusted-content marking.
- A privacy-page update.

**Phase 4, only if needed.**

- The A-fallback flow for non-Authentik ds-mcp identities.
- A delete scope, if the user asks for one (open question 5).

## 8. Open questions for the user

1. **Option.** Approve D2 (token exchange, a DS-side grant, A as fallback)?
   Or choose plain A, which is a well-established flow with one more
   per-user secret in ds-mcp and an extra login hop, over relying on
   Authentik 2026.8's newer exchange support?
2. **Granularity.**
   - The proposal: one DS grant per user for ds-mcp, shared by all of that
     user's MCP clients.
   - Should ds-mcp also ask before a *new* MCP client (for example a newly
     added Claude Code) first uses it?
3. **Logout semantics.**
   - The proposal: an Authentik logout kills outstanding DS tokens and
     ds-mcp's session, but keeps the DS grant, so signing back in to Claude
     restores saving without asking for consent again.
   - Or should any Authentik logout also revoke the grant?
4. **Group reads.**
   - A per-group opt-in, off by default, and toggled by owners and admins
     only?
   - Should journey names from other members be redacted even then?
   - Or is Phase 3 not wanted at all?
5. **Delete.** Keep journeys undeletable through the assistant for good, or
   add a `journeys:delete` scope later, perhaps limited to journeys with
   `created_via = ds-mcp`?
6. **Limits.** Are 10 writes/min and 50 journeys/day per user, and 60
   reads/min, acceptable starting values?
7. **Grant expiry.** Should grants that go unused for 90 days expire? Or
   should every grant have an absolute expiry, such as one year, even when
   in use?
8. **Step-up.** Should creating a grant require a recent DS login
   (`prompt=login`, or a session younger than 10 minutes)?
9. **Audit retention.** Is 7 days of Loki plus the database columns enough,
   or should there be a `delegated_audit` table with longer retention?
10. **Exposure.** Should DS refuse delegated bearers when the api is
    reached through a public ingress (`ingress.api.enabled`), so they work
    in-cluster only?
11. **Atomic save for the web.** Should the frontend's `PlanTripFlow` move to
    `POST /Journeys/itinerary` as well, which fixes its partial-commit case?
12. **Privacy and terms.** Who updates the privacy page to cover connected
    apps and the processing of journey data by a third-party LLM?
13. **Spec index.** `docs/superpowers/README.md` has no rows for specs
    after 2026-10-06. Should this spec be added to the index, along with
    the other recent ones, in a separate pass?

## 9. Things to verify on the real Authentik before Phase 1

The ds-mcp-side facts come from the Ranma-Config spec and Authentik's docs.
None of these items has been tested on a live instance.

1. **Token exchange setup.**
   - How a provider is configured to accept `subject_token`s from another
     provider (federation, trusted issuers).
   - That it **refuses** subject tokens from any other provider.
   - Which client authenticates the request.
2. **Delegation mode.** Whether the issued token carries `act`, and what
   identifies ds-mcp in it.
3. **Scopes.** Whether exchanged and auth-code access tokens carry a `scope`
   claim, or whether a custom mapping must emit `ds_scopes`. Whether the
   exchange request can narrow the scopes.
4. **`sub`.** That the exchanged token's `sub` equals the subject token's
   `sub` and DS's `users.id`, for a local user and for a Discord-sourced
   user.
5. **Lifetime.** That exchanged tokens use the target provider's
   `access_token_validity` (5 min), not the subject token's.
6. **Inactive users.** That an exchange for a deactivated user, or with a
   revoked subject token, fails.
7. **Session effects.** Whether deleting an Authentik session revokes the
   `distant-signal-mcp` access token used as the subject. This bears on
   whether exchange stops at once after logout, or only once ds-mcp's
   back-channel logout drops the stored token.
8. **Fallback A.** Explicit consent flow, custom scopes on the consent
   screen, and whether RFC 8707 `resource` is supported (expected: no).
9. **`iat` and `jti`.** That `iat` and `jti` are present on exchanged
   tokens.
