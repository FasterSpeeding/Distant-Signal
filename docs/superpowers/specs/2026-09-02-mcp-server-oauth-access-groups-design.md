# MCP Server OAuth Access Groups — Design

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
(area-by-area structure, decisions with real alternatives weighed) and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(section conventions — "Current relevant state" cited to real code, a
"Corrections/relationship to prior specs" section as a first-class
citizen, "Open questions/risks" likewise). Builds directly on
`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md`
(hereafter "the sibling doc"), including its own "Corrections" section
revising Decision 4 (Auth) and Decision 6 (Deployment) — read in full
this session, not summarized from memory. No implementation plan is
included; that is a separate, later step in this repo's process.

## Goal

Two things the user has asked for, concretely:

1. **Remove Discord-based auth from the derived MCP service
   ("distant-signal-mcp") entirely**, replacing it with the same
   OAuth/OIDC identity DS's own users already have — not a second,
   parallel identity system.
2. **Gate MCP features behind a real authorization-tier concept** — which
   authenticated DS user may use which MCP tool — using this app's own
   access-group primitive, not anything Discord-specific.

The sibling doc's Corrections section already reversed its own Decision 4
this session, replacing train-mcp's original Discord-OAuth incoming gate
with DS's own OIDC login, mediated by an adapter that acts as its own
minimal OAuth 2.1 authorization server to MCP clients. This document's
first job is to determine **precisely** what that reversal did and did not
settle, then design the remainder concretely — the Helm chart and
`docker-compose.yml` still carry the pre-reversal Discord config as live,
shipped state (confirmed by direct inspection below, and by the user's own
pasted `docker compose up` output showing `RAIL_MCP_DISCORD_CLIENT_ID`/
`RAIL_MCP_DISCORD_ALLOWED_USER_IDS` still being read), and no document has
yet designed an access-group concept for this app at all.

## Relationship to the sibling doc — precisely what it did and did not settle

Reading the sibling doc's Corrections section and its revised Decision 4/4d
closely (`docs/superpowers/specs/2026-09-01-train-mcp-integration-design.md:561-758`)
resolves the question the task opened with: **the reversal replaces *who
you are* (identity/authentication) but explicitly does not design *who is
allowed* (authorization/access-tiering).** These are different axes, and
the sibling doc treats them as different on its own terms:

- **What it settled.** Decision 4d's adapter is a real, spec-compliant
  OAuth 2.1 authorization server facing MCP clients, backed by DS's own
  existing, unmodified OIDC login for the human-authentication step
  (`train-mcp-integration-design.md:698-736`). Discord OAuth, "train-mcp's
  own original incoming gate," is explicitly stated to be **"superseded,
  not stacked alongside DS's OIDC"** (`:745-747`) as the mechanism that
  answers "who is this request on behalf of."
- **What it explicitly left open.** The same section immediately continues:
  *"The already-shipped `railMcp.discord.*` chart values
  (`values.yaml:790-796`, consumed by `railmcp-deployment.yaml`) are not
  resolved by this document — whether they're retired, repurposed, or kept
  as a secondary gate is a chart-and-implementation-plan-level decision,
  out of scope for a design-document edit per this task's own
  constraints"* (`:747-752`), restated verbatim as Open question/risk 8
  (`:1283-1288`). And Decision 4d's own gate description is unconditional
  on identity alone: *"the incoming gate is now 'has this caller completed
  DS's own login,' not a per-tool check"* (`:736-737`) — **any** DS user
  who can complete DS's own OIDC login, full stop. Discord's
  `RAIL_MCP_DISCORD_ALLOWED_USER_IDS` allowlist (a real, if crude,
  authorization control — only specific Discord user IDs could ever call
  the server) has no replacement anywhere in the sibling doc. Its removal,
  as designed there, is a straight subtraction, not a swap.

**So: the reversal is the complete answer to "auth mechanism" and not the
answer to "who's allowed to use it."** This document's job is exactly the
gap the sibling doc named and declined to fill: designing the
authorization-tier concept, using this app's own primitives rather than
inventing a parallel one, and reconciling it with (not duplicating)
Decision 4d's adapter shape. Nothing below revisits or contradicts Decision
4d's chosen shape for authentication; it is treated as settled and reused
as-is.

## Current relevant state (verified 2026-09-02)

### Discord config is real, shipped state today — every reference, by file

- **`charts/distant-signal/values.yaml:767-822`** — the `railMcp:` block.
  Lines **794-796**:
  ```yaml
    discord:
      clientId: ""
      allowedUserIds: ""
  ```
  Lines **815-816**:
  ```yaml
    existingSecretDiscordClientIdKey: discord-client-id
    existingSecretDiscordAllowedUserIdsKey: discord-allowed-user-ids
  ```
- **`charts/distant-signal/templates/secret.yaml:83-98`** — the comment at
  83-88 currently reads *"railMcp's own **eight** credentials (Discord
  OAuth + the six LDBWS product values...)"*; lines **90-91** render the
  two Discord values:
  ```
  {{- $_ := set $data "discord-client-id" (.Values.railMcp.discord.clientId | default "" | b64enc) -}}
  {{- $_ := set $data "discord-allowed-user-ids" (.Values.railMcp.discord.allowedUserIds | default "" | toString | b64enc) -}}
  ```
  (Lines 92-97, the six `ldbws-*` keys, are untouched by this design — see
  Explicitly out of scope.)
- **`charts/distant-signal/templates/_helpers.tpl:491-505`** — two whole
  `define` blocks, `distant-signal.railMcpDiscordClientIdSecretKey` and
  `distant-signal.railMcpDiscordAllowedUserIdsSecretKey`, each resolving
  either `existingSecret`'s configured key name or the hardcoded fallback
  (`"discord-client-id"`/`"discord-allowed-user-ids"`) — the same
  `existingSecret`/`existingSecretXKey` pattern every other credential in
  this chart follows (per the file's own header comment at 472-482).
- **`charts/distant-signal/templates/railmcp-deployment.yaml:68-77`** —
  two `env` entries, `DISCORD_CLIENT_ID` and `DISCORD_ALLOWED_USER_IDS`,
  each a `secretKeyRef` into the two helpers above. Immediately above them
  (lines 61-63) a comment already reads *"no new DS-side route or auth
  needed (Decision 4: every DS call this service makes is anonymous)"* —
  this comment is itself now stale relative to the sibling doc's own
  Corrections (revised Decision 4 has the derived service call
  `/Train/by-uid/*` with the caller's held DS session, 3b.6 revised); noted
  here as an aside since a follow-up plan touching this file should fix it
  while it's already being edited for Discord removal, but it is not this
  document's own scope to design.
- **`docker-compose.yml:486-522`** — the `rail-mcp:` service. Lines
  **512-513**:
  ```yaml
        DISCORD_CLIENT_ID: ${RAIL_MCP_DISCORD_CLIENT_ID}
        DISCORD_ALLOWED_USER_IDS: ${RAIL_MCP_DISCORD_ALLOWED_USER_IDS}
  ```
  This is the exact source of the user's pasted warning
  (`RAIL_MCP_DISCORD_ALLOWED_USER_IDS`/`RAIL_MCP_DISCORD_CLIENT_ID` "not
  set. Defaulting to a blank string.") — the compose var substitution
  reads an unset host-shell/`.env` variable. **No `.env.example` (or any
  `.env*` file) exists anywhere in this repo** (confirmed:
  `find . -maxdepth 1 -iname '.env*'` returns nothing this session) — the
  warning is not evidence of a stale committed example file, only of the
  operator's own shell/`.env` lacking these vars, which is expected until
  either they're supplied or (this design's own outcome) the variables
  stop being referenced at all.
- **No other file in the repo references `RAIL_MCP_DISCORD_*`/
  `DISCORD_CLIENT_ID`/`DISCORD_ALLOWED_USER_IDS`** (grepped this session,
  `charts/` and `docker-compose*.yml` together — the five citations above
  are the complete set).

### This app's real current group/role capability — re-confirmed, still none

Every piece cited below was read directly this session, not carried over
from an earlier one:

- **`crates/api/src/data/users.rs:50-55`** — the entire `User` struct:
  ```rust
  pub struct User {
      pub id: String,
      pub email: Option<String>,
      pub name: Option<String>,
  }
  ```
  **`crates/api/src/data/users.rs:102-107`** — `SessionUser`, the row
  actually joined at request time, is the same three fields. **`get_session_
  with_user`** (`users.rs:114-124`) selects exactly `u.id, u.email, u.name`.
- **`crates/api/src/auth.rs:176-180`** — `AuthenticatedUser`, the extractor
  every ownership-scoped route depends on, is `{id, email, name}`. No
  fourth field of any kind.
- **`crates/api/migrations/20260828090000_user_accounts.sql:31-37`** — the
  `users` table schema itself: `id, email, name, created_at,
  last_login_at`. No groups/roles table, no join table, anywhere in this
  migration or any other (grepped `crates/*/migrations` this session for
  `role|group|permission` — zero hits outside comments already quoted
  above).
- **`grep -rniE 'role|group|permission|scope' crates/api/src/data/users.rs
  crates/api/src/auth.rs crates/api/src/auth/`** (run this session): the
  only hits are `oauth2::Scope` (an OIDC library type, not an app concept)
  and a docstring using the English word "scoped" to describe an ownership
  check. **Confirmed: still true, no role/permission/access-group concept
  exists anywhere in this app's own code today.**

This matters directly: "gate behind normal access groups" cannot be a
small wiring task onto an existing primitive, because no such primitive
exists yet. It has to be designed here, for the first time — the sibling
doc did not design it and does not assume it exists (its own Decision 4d
gates on "completed DS's own login," which needs no group concept at all).

### DS's own OIDC client: what scope/claims it requests and reads today

- **`crates/api/src/auth/oidc.rs:161-162`** — `authorize_url` requests
  exactly two scopes beyond the implicit `openid`:
  `add_scope(Scope::new("email"))`, `add_scope(Scope::new("profile"))`. No
  `groups` scope, no custom scope, requested anywhere.
- **`crates/api/src/auth/oidc.rs:37-42`** — `RawClaims`: `sub`, `email`,
  `email_verified`, `name`. **`oidc.rs:21-26`** — `OidcIdentity`: the same
  four fields. **`identity_from_claims`** (`:47-54`) maps exactly these
  four and nothing else.
- **`crates/api/src/auth/oidc.rs:94-95`** — the `DiscoveredClient` type
  alias is built on `openidconnect::core::CoreClient`. Confirmed via
  `docs.rs/openidconnect/4.0.1` this session (matching the pinned version
  in `Cargo.lock:2277-2279`, `openidconnect = "4.0.1"`): **`core::CoreClient`
  fixes its `AdditionalClaims` type parameter to `EmptyAdditionalClaims`**
  — i.e., even if an ID token Authentik issues today already carries a
  `groups` claim, this app's current `CoreClient`-typed verification path
  cannot see it; `EmptyAdditionalClaims` deserializes and silently discards
  anything beyond the standard claim set `openidconnect::core` models.
  Reading a `groups` claim would require switching from the `core`
  module's `CoreClient` alias to the crate's generic
  `openidconnect::Client<MyAdditionalClaims, ...>` with a custom
  `AdditionalClaims`-implementing type — a real, if bounded, code change to
  `oidc.rs`'s type signature, not just a new field read off an
  already-parsed struct.
- **`charts/distant-signal/files/devauthentik-blueprints/oauth2-client.yaml:82-85`**
  — the dev IdP's own provisioned OAuth2 client, read directly this
  session, attaches exactly three scope mappings: `openid`, `email`,
  `profile` (`!Find [authentik_providers_oauth2.scopemapping, [scope_name,
  ...]]` for each). No `groups` scope mapping is attached. This mirrors
  `oidc.rs`'s own two `add_scope` calls exactly — the dev blueprint and the
  Rust client agree, consistently, on today's scope set.
- **`crates/api/src/routes/auth.rs:223-233`** — `GET /auth/session`
  (`routes/mod.rs:20-50`: merged into `public_router()`, unauthenticated,
  via `OptionalAuthenticatedUser`) returns exactly `{authenticated, id,
  email, name}` — the same three-field identity, nothing more, already the
  one existing read-back point any consumer (including a future adapter)
  could use to learn about the current session.

### Does Authentik expose group membership via a standard OIDC claim? Researched this session, not assumed

Fetched from `docs.goauthentik.io` and corroborated via a web search
against Authentik's own community/GitHub discussion threads and its
Terraform provider examples, this session:

- Authentik's own docs (`add-secure-apps/providers/oauth2/`) state the
  built-in `profile` **scope** "includes basic profile information, such
  as the user's username, name, and group membership" — group membership
  is a documented part of the standard `profile` scope's intent, not a
  bespoke Authentik extension requiring a separate product.
- The mechanism is Authentik's **scope mappings** (a form of "property
  mapping"): a built-in mapping named `authentik default OAuth Mapping:
  OpenID 'profile'` is a plain Python expression evaluated per-token, and
  multiple independently corroborating real-world sources (a GitHub
  discussion, a Terraform property-mapping example, a homelab SSO
  writeup) all show/describe the same underlying line: **`"groups":
  [group.name for group in request.user.ak_groups.all()]`** as part of
  that mapping's expression — i.e., group names, sourced from Authentik's
  own native group model (`ak_groups`), are designed to ride the *already-
  requested* `profile` scope, not a separate one.
- **This is corroborated, not certain for every deployment**: the same
  search surfaced real reports of the groups claim not populating
  reliably via the stock `profile` mapping in practice, and a commonly
  recommended, more reliable pattern is an operator-authored **custom**
  scope mapping (attached under a dedicated `groups` scope) rather than
  depending on the built-in `profile` mapping's exact expression, which an
  operator could also have edited or replaced.
- **Net finding, stated precisely**: Authentik groups are real, native,
  and *designed* to be exposable via a standard OIDC claim carried on a
  scope DS's OIDC client already requests (`profile`) — but whether a
  given deployment's ID token actually carries a populated `groups` claim
  today depends on that deployment's exact mapping configuration, which
  this design cannot assert as universal, and DS's own code (`RawClaims`/
  `CoreClient`, above) could not read it even if present. Both gaps —
  "is the claim actually there" and "can DS's code see it" — are real and
  independent; this design does not assume either is already true, and
  addresses both explicitly (Decision 2, below).

## Decisions

### 1. Authentication mechanism: unchanged from the sibling doc's Decision 4d — reused as-is, not re-derived

**Not re-decided here.** The sibling doc's adapter — its own minimal
OAuth 2.1 authorization server to MCP clients, backed by DS's existing,
unmodified OIDC login for the human step, holding the resulting DS session
server-side keyed to the MCP access token it issues
(`train-mcp-integration-design.md:698-736`) — is the complete answer to
"how does the MCP server authenticate a human user going forward." This
document adds exactly one thing to that picture: **the adapter, once it
holds a completed DS login, also has to learn that user's DS group
membership** before it can make an authorization decision (Decision 3,
below) — a new read, not a new authentication flow.

### 2. Access-group source of truth: Authentik-native groups, via a `groups` OIDC claim DS's own OIDC client is extended to request and read — not a DS-built group-management system

Two real shapes were weighed for "where do group memberships live":

- **(A) DS builds and owns its own group-management primitive** — a new
  `groups`/`user_groups` table, an admin UI or API to assign users to
  groups, DS becomes the source of truth. **Rejected.** This app has zero
  existing precedent for *any* admin-facing user-management surface
  (confirmed above: `users` has no admin-editable field at all beyond what
  OIDC login itself writes) — building one starts from nothing, for a
  single-operator-run app that already has a real IdP doing exactly this
  job. It would also create two disagreeing group models the moment an
  operator ever manages users in both Authentik (for login) and DS (for
  MCP access) — a durable source of confusion this app has no reason to
  invite.
- **(B) Authentik's own native groups, read off the ID token as a `groups`
  claim DS already has the pipeline to consume.** **Chosen.** Per Current
  relevant state above, this is a real, designed Authentik capability
  (native `ak_groups`, exposable via a scope mapping on the already-
  requested `profile` scope or a dedicated custom scope), not invented for
  this design. An operator already manages who exists and authenticates in
  Authentik; extending that to "and which of those people may use MCP
  tools" by adding them to an Authentik group is the same administrative
  motion they already perform for every other access decision this app's
  login gate makes, not a second system to learn.

**Concretely, two independent gaps have to close for (B) to actually work,
both grounded in Current relevant state, neither assumed already solved:**

1. **DS's OIDC client has to explicitly request the claim.** Add a
   `groups` scope to `oidc.rs:161-162`'s `authorize_url` (alongside
   `email`/`profile`) — this design does not rely on the `profile` scope's
   built-in mapping alone, given the corroborated real-world reports that
   it doesn't always populate reliably; a dedicated `groups` scope mapping,
   explicitly authored/attached in whatever Authentik instance an operator
   runs (dev blueprint: a new `oauth2-client.yaml` entry alongside the
   three existing `!Find [...scopemapping...]` lines,
   `oauth2-client.yaml:82-85`), is the more defensible, less-implicit path.
2. **DS's OIDC verification path has to be able to see the claim.** Per
   Current relevant state, `core::CoreClient`'s `EmptyAdditionalClaims`
   fixing means the claim is invisible even if present. `oidc.rs` needs a
   custom `AdditionalClaims`-implementing type (a small struct, `{groups:
   Option<Vec<String>>}`, deserialized permissively — a missing claim is
   not an error, just "no groups asserted") and the `DiscoveredClient`/
   `CoreClient` usage throughout `oidc.rs` widened to the generic
   `openidconnect::Client<...>` form carrying that type, in place of the
   `core` module's fixed alias. `RawClaims`/`OidcIdentity`/
   `identity_from_claims` each gain a `groups: Vec<String>` field
   (defaulting empty, never erroring on absence — the same "never trust
   silence as something stronger than it is" posture `email_verified`'s
   own handling already takes, `oidc.rs:44-46`).

**Persistence and propagation, chosen shape**: `users.rs::upsert_user`
already re-syncs `email`/`name` "on every return visit," not just first
login (`users.rs:57-59`'s own doc comment) — groups follow the identical
pattern: a new nullable/array `groups` column on `users` (one new,
additive migration; no change to `sessions`), overwritten on every login
with whatever the fresh ID token asserted, never independently managed by
DS. `AuthenticatedUser`/`SessionUser` gain the same field, and `GET
/auth/session`'s `SessionResponse` (`routes/auth.rs:223-233`) gains a
`groups: Vec<String>` field — the one existing read-back endpoint any
consumer, including the adapter, already knows how to call for identity
becomes the same place it learns about group membership, with no new route
needed for that purpose.

**How the adapter learns a caller's groups**: per Decision 4d, the adapter
already "holds the resulting DS session server-side" after driving DS's
login end-to-end. It reads the caller's groups by calling DS's own,
already-existing `GET /auth/session` with that held session (now widened
per this decision) — **not** a second, independent connection to
Authentik's own userinfo/introspection endpoint. This keeps DS as the
single source of identity truth the adapter consumes (matching Decision
4d's own framing: the adapter drives DS's *existing* login, it does not
grow a parallel relationship with Authentik of its own) and avoids two
independent claim-reading implementations (Rust in `crates/api`, and
whatever the TypeScript adapter would otherwise need) ever disagreeing
about what a `groups` claim means.

**Staleness trade-off, stated plainly, not hidden**: because groups are
captured at login time and refreshed only on the *next* login (same
posture `email`/`name` already have), a user removed from an Authentik
group keeps whatever access their last-issued DS session/adapter token
carries until they log in again — this mirrors `eta_blend.rs`'s own
documented "deliberately NOT a guaranteed join" posture for a different
join in this codebase, and is flagged again in Open questions/risks rather
than solved with a live-lookup-on-every-call design this app has no
existing infrastructure for.

### 3. Group split: two groups for this pass — a whole-server gate and a live-boards gate — not one, not six

The user's own framing ("gate MCP *features*," plural) and the app's real
current tool set (`resolve_station`, `get_departures`, `get_arrivals`,
`get_service_detail`, `find_services`, `plan_journey`) were weighed against
three shapes:

- **(a) A single group gating the whole server** (e.g. `mcp-users`), no
  finer split. **Rejected as insufficient on its own** — it's a faithful
  replacement for Discord's old *binary* allowlist role (which also had no
  per-tool concept), but the user explicitly asked for feature-level
  gating, and there's a real, concrete reason to want one: see (c) below.
- **(b) Six groups, one per tool.** **Rejected as premature for a first
  pass.** This app has no existing multi-group administrative workflow at
  all (Decision 2) — six independently-managed Authentik groups for a
  likely-small, single-operator-run user base is real, ongoing
  administrative overhead (six memberships to keep correct per user) for
  a distinction the tool set doesn't obviously need at that granularity:
  `resolve_station` and `find_services` share the same real-world risk
  profile (cheap, no metered external call — see below), so splitting them
  from each other buys nothing concrete today.
- **(c) Two groups: a coarse whole-server gate, plus one narrower gate
  covering the tools that hit a real, external, metered resource.**
  **Chosen.** Per the sibling doc's own Decision 5 (unchanged, reused
  here): `get_departures`, `get_arrivals`, and `get_service_detail` call
  Darwin/LDBWS **directly**, against the operator's own metered LDBWS/Rail
  Data Marketplace product keys — "an ongoing RDM-quota/architecture cost"
  in that doc's own words (`train-mcp-integration-design.md:775-779`).
  `plan_journey` also touches this same metered surface: its own Decision
  3a keeps "train-mcp's own existing Darwin board check... retained
  unmodified as the foundational mechanism"
  (`train-mcp-integration-design.md:402-408`) for any leg inside the live-
  board horizon — i.e. `plan_journey` is not purely a local CIF-store
  computation, it makes the same class of external metered call the three
  board tools make. `resolve_station` (DS's own already-public,
  unauthenticated `/public/stations`) and `find_services` (train-mcp's own
  local CIF/SQLite store, no live external call per the sibling doc's own
  characterization) share neither DS's own infrastructure risk nor any
  metered external dependency.

**Chosen groups, both Authentik-native, both operator-named via chart
values (not hardcoded strings this design mandates)**:

- **`mcp-users`** (suggested default name) — the whole-server gate,
  required to use *any* MCP tool at all. This is the direct, designed
  replacement for Discord's `RAIL_MCP_DISCORD_ALLOWED_USER_IDS` allowlist
  role — "may this DS user reach the MCP server at all" — now expressed as
  Authentik group membership instead of a Discord user-ID list.
- **`mcp-live-boards`** (suggested default name) — required, *in addition
  to* `mcp-users`, for `get_departures`, `get_arrivals`,
  `get_service_detail`, and `plan_journey` — the four tools that reach
  outside DS's own infrastructure onto the operator's metered LDBWS
  product keys. `resolve_station` and `find_services` require only
  `mcp-users`.

**Explicitly not a closed design** — the mechanism (arbitrary named
group → set of gated tools) generalizes to a finer split later (e.g.
separating `plan_journey` from the three simpler board tools, or gating it
alone given it's the sibling doc's own "standout, buildable-now, high-
value" capability) without redesigning anything here; it's a
configuration change to which group(s) a tool requires, not a new
mechanism. Not designed further than naming that this is a natural,
cheap-to-make future refinement — see Open questions/risks.

### 4. Where the group check is enforced: the adapter, at two different moments, matching the two different gates

The adapter (Decision 4d) already terminates every incoming MCP-client
request and already knows, per Decision 3, which of the two gates a given
tool requires. Two enforcement points, deliberately different in kind
because the two gates mean different things:

- **`mcp-users` (whole-server gate): enforced once, at OAuth authorization-
  grant time**, immediately after the adapter completes DS's login and
  reads the caller's groups (Decision 2's `GET /auth/session` call) —
  **before** it ever issues an MCP-scoped Bearer token. A caller who
  authenticated as a real DS user but isn't in `mcp-users` never receives
  a working token at all; there is no window where a not-entitled DS user
  holds a technically-valid MCP credential that then fails per-call.
- **`mcp-live-boards` (feature gate): enforced per-tool, at `tools/list`
  and again at `tools/call`** — see Error handling for exactly what each
  produces. This is a request-shape decision (which tools this specific,
  already-token-holding caller may see/invoke), not an identity decision,
  so it belongs after token issuance, not before.

This two-tier enforcement point split is itself part of the design, not
incidental: collapsing both into "check everything at grant time" would
mean re-issuing a token every time an operator changes someone's
`mcp-live-boards` membership (since nothing about the *token* encodes
per-tool state if it's checked once, up front, unless the token itself
embeds group membership — see Open questions/risks for the caching
trade-off this implies either way).

## Architecture

Extends the sibling doc's own revised diagram
(`train-mcp-integration-design.md:964-1033`) with the groups-claim flow —
everything left of the adapter box is new to this document; everything
right of it (the derived MCP service's own tool implementations, its calls
to DS's `/public/*`/`/Line/*`/`/Train/by-uid/*`) is unchanged, reused
exactly as the sibling doc designed it.

```
                            Authentik (DS's OIDC provider)
                            groups: ak_groups.all(), via a
                            `groups` scope mapping this design
                            adds (Decision 2)
                                    │
                                    │ ID token, groups claim included
                                    ▼
┌───────────────────────────────────────────────────────────────────────┐
│ crates/api (unchanged route surface, extended OIDC internals)           │
│  auth/oidc.rs: authorize_url requests groups scope (NEW, Decision 2)    │
│                custom AdditionalClaims type reads it (NEW)              │
│  data/users.rs: upsert_user persists groups, refreshed every login (NEW)│
│  auth.rs: AuthenticatedUser/SessionUser gain `groups: Vec<String>` (NEW)│
│  routes/auth.rs: GET /auth/session now returns groups too (NEW field,   │
│                  same existing route)                                   │
└───────────────────────────┬─────────────────────────────────────────┘
                             │ GET /auth/session, using the adapter's own
                             │ held DS session (Decision 4d, reused)
                             ▼
┌───────────────────────────────────────────────────────────────────────┐
│ adapter auth layer (sibling doc Decision 4d, UNCHANGED shape)          │
│  • drives DS's existing OIDC login, holds the resulting DS session      │
│  • NEW: reads that session's groups via GET /auth/session               │
│  • NEW gate 1 (Decision 4 here): mcp-users membership checked BEFORE    │
│    issuing an MCP Bearer token -- missing membership -> OAuth           │
│    error=access_denied, no token issued at all                         │
│  • NEW gate 2 (Decision 4 here): mcp-live-boards membership checked     │
│    per tools/list (filters the tool set shown) and per tools/call       │
│    (403 fallback) for get_departures/get_arrivals/get_service_detail/   │
│    plan_journey specifically                                            │
└───────────────────────────┬─────────────────────────────────────────┘
                             │ Authorization: Bearer <MCP-scoped token>
                             ▼
                  derived MCP service -- UNCHANGED from the sibling doc
                  (resolve_station, get_departures, get_arrivals,
                   get_service_detail, find_services, plan_journey;
                   its own calls to DS's /public/*, /Line/*/Status,
                   /Train/by-uid/* are exactly as the sibling doc designed)
```

## Error handling

Two distinct, designed outcomes for "authenticated but not entitled,"
matching the two gates (Decision 4):

- **Missing `mcp-users` (whole-server gate)**: the adapter's own
  authorization endpoint returns a standard OAuth 2.1 error redirect,
  `error=access_denied` (RFC 6749 §4.1.2.1), back to the connecting MCP
  client — the same real, spec-defined outcome any OAuth authorization
  server uses for "the resource owner denied the request," reused here for
  "the resource owner isn't entitled," rather than a bespoke error shape.
  **No MCP-scoped Bearer token is ever issued** in this case — there is no
  window where a not-entitled but real DS user holds a working-looking
  credential that then fails on first use; the denial happens at the
  earliest possible point, matching how a real OAuth AS is expected to
  behave.
- **Missing `mcp-live-boards` (feature gate), for an otherwise valid
  MCP-token holder**:
  - **`tools/list`**: the adapter filters the returned tool set to what
    the caller's groups actually permit — a caller without
    `mcp-live-boards` sees `resolve_station`/`find_services` only. This is
    the primary, designed UX: an AI assistant simply never learns a tool
    it can't use exists, avoiding a confusing "access denied" surfaced
    mid-conversation and avoiding leaking which gated tools/groups exist
    to a caller who can't reach them.
  - **`tools/call` on a filtered-out tool anyway** (a stale cached tool
    list from before a group change, or a client that doesn't re-fetch
    `tools/list`): a JSON-RPC error response carrying HTTP 403 — not 401,
    which stays reserved for "no/invalid Bearer token at all" — with a
    plain message naming the missing group requirement. This is
    defense-in-depth, not the primary path; it should be rare in practice
    given the `tools/list` filter above, but is a real, designed fallback
    rather than an unhandled case.
- **DS's `GET /auth/session` call itself failing** (network error, DS
  down) while the adapter is trying to learn a caller's groups: treated
  as "cannot currently verify entitlement" — the adapter fails closed
  (denies the grant / hides the gated tools), not open, matching the "fail
  the render / fail the request rather than silently degrade" posture
  `api.sso.*`'s own comment already commits to elsewhere in this chart
  (`values.yaml:299-310`) for a comparable "can't do the thing safely"
  situation.

## Testing

Following this repo's established convention of table-driven auth tests
(the sibling doc's own Decision 4 testing section,
`train-mcp-integration-design.md:1100-1112`, is the direct precedent this
extends, not replaces):

- **`crates/api/src/auth/oidc.rs`**: `identity_from_claims`/`RawClaims`
  unit tests for the new `groups` field — present-and-populated claim
  parses to the expected `Vec<String>`; absent claim defaults to an empty
  vec, not an error (mirroring the existing `missing_email_verified_
  claim_defaults_to_unverified` test's own posture, `oidc.rs:254-258`).
- **`crates/api/src/data/users.rs`**: `upsert_user` test confirming groups
  are overwritten (not merged/unioned) on every login, matching the
  existing `email`/`name` re-sync behavior exactly — a user removed from a
  group in Authentik and then logging in again sees that removal reflected
  in `users.groups`, not a stale accumulation.
- **`crates/api/src/routes/auth.rs`**: `GET /auth/session` response-shape
  test confirming `groups` is present (populated for a real session, empty
  array for none) alongside the three existing fields.
- **Adapter (implementation-plan-level, sketched here as required
  coverage)**: a table-driven test partitioning caller-group combinations
  against gate outcomes — `{}` (no groups) → OAuth `access_denied` at grant
  time, before any token issuance; `{mcp-users}` → token issued,
  `tools/list` returns exactly `resolve_station`/`find_services`;
  `{mcp-users, mcp-live-boards}` → token issued, all six tools listed. A
  second test: `tools/call` on a `mcp-live-boards`-gated tool by a caller
  lacking it returns the 403 fallback described in Error handling, not a
  silent success or an unrelated 401.
- **Regression test on Discord's removal**: grep-shaped or config-shaped
  test (implementation-plan-level) confirming no `DISCORD_*`/
  `RAIL_MCP_DISCORD_*` reference remains in `charts/distant-signal/` or
  `docker-compose.yml` post-implementation — guards directly against a
  partial removal leaving dead env-var plumbing behind, the same category
  of leftover this design's own "What gets removed" section exists to
  prevent.

## Explicitly out of scope

- **The LDBWS/Rail Data Marketplace credentials themselves**
  (`RAIL_MCP_LDBWS_*`/`ldbws.*` in the chart, `LDBWS_DEPARTURES_URL` etc.
  in compose) — per the sibling doc's own unchanged Decision 5, these are
  a wholly separate upstream Darwin/LDBWS API credential the forked
  service's own board tools still need directly; nothing about this
  design's auth/access-group work touches them. They remain a genuinely
  distinct config surface from the Discord credentials this design does
  remove — confirmed side-by-side in Current relevant state above
  (`secret.yaml`'s single comment block covers both only because they're
  rendered by the same conditional, not because they're related
  concerns).
- **The compose opt-in gating mechanism** for `rail-mcp` (the sibling,
  separate-session fix for `docker compose up` currently having no
  `profiles:`-style opt-out for a service with no credentials supplied).
  Not designed here. This design's own compose changes (removing the two
  `DISCORD_*` lines, Decision 3/4's env additions) are additive/
  subtractive edits to the existing `rail-mcp:` service block and don't
  assume anything about whether that service is always running — they
  compose cleanly regardless of what mechanism the sibling fix adds.
- **Re-designing Decision 4d's adapter authentication shape.** Reused
  exactly as the sibling doc designed it (Decision 1, above) — this
  document adds a read (the caller's groups, via the already-existing
  `GET /auth/session`) and two authorization checks on top, not a new
  authentication flow.
- **A DS-owned group-management UI or storage.** Considered and rejected
  in Decision 2 — Authentik remains the sole place groups are created and
  membership assigned; DS only ever reads and caches what Authentik
  asserts.
- **Any other DS feature (frontend, custom lines, ticket uploads, etc.)
  consuming the new `groups` claim.** The `oidc.rs`/`users`-table plumbing
  this design adds is general (any future DS feature could read
  `AuthenticatedUser.groups`), but this document scopes its actual use to
  gating the derived MCP service's tools — no other consumer is designed
  or implied here.
- **NRE/Network-Rail-branding attribution requirements for MCP
  tool-rendered output.** Already flagged, unresolved, and explicitly not
  re-litigated by the sibling doc's own Licensing note
  (`train-mcp-integration-design.md:1171-1223`) — this document doesn't
  touch or narrow that question either; it is orthogonal to who may call
  the server.
- **Retiring the Discord OAuth application on Discord's own side** (the
  external application registration, independent of this repo's config).
  An operational/implementation-plan-level action, not a design concern.
- **Per-tool granularity beyond the two groups chosen in Decision 3.**
  Named as a natural, cheap future refinement (same mechanism, more
  entries), not designed to that depth here.

## Open questions/risks

1. **Whether a given operator's real Authentik instance's `profile` scope
   mapping actually populates `groups` reliably, or needs a dedicated
   custom scope mapping instead** — Current relevant state found real,
   corroborated reports of this being inconsistent in practice. This
   design's own Decision 2 already hedges by requesting a dedicated
   `groups` scope rather than relying on the built-in `profile` mapping's
   default expression alone, but whether an operator's Authentik instance
   has a `groups` scope mapping correctly attached at all is an
   operator-environment fact this design cannot assert, the same category
   of caveat the sibling doc's own DCR research (Decision 4b there) made
   about Authentik version currency.
2. **Group staleness between logins** (Decision 2's own stated trade-off):
   a user removed from `mcp-users`/`mcp-live-boards` in Authentik keeps
   whatever access their last-issued session/adapter token carries until
   their next login. Not designed around here — no live-lookup-on-every-
   call mechanism is proposed, matching this codebase's existing tolerance
   for comparable staleness elsewhere (`eta_blend.rs`'s "deliberately NOT a
   guaranteed join").
3. **Whether the adapter's own MCP-scoped Bearer token should embed group
   membership at issuance time (avoiding a live `GET /auth/session` call
   on every `tools/list`/`tools/call`) or re-check DS on every gated
   request** — a real caching-vs-freshness trade-off this document
   surfaces (Decision 4's two-tier enforcement split implies the
   `mcp-live-boards` check plausibly happens often enough to want caching)
   but does not resolve to implementation depth, consistent with the
   sibling doc's own posture of not designing the adapter's protocol
   internals (`train-mcp-integration-design.md:754-758`,
   "Explicitly out of scope" there).
4. **The exact Authentik group names (`mcp-users`/`mcp-live-boards`) are
   this document's own suggested defaults, not researched against any
   existing naming convention** — this app has no other Authentik-group-
   consuming feature yet to establish one against. An operator is free to
   name them anything via the chart values this design implies (not
   specified to exact YAML key names here, since no code is written by
   this document); flagged as a naming choice, not a load-bearing
   technical one.
5. **Whether `mcp-live-boards` should eventually be split further (e.g.
   isolating `plan_journey` alone, given its heavier DS-annotation cost
   per the sibling doc's own Decision 3)** — Decision 3 above names this
   as a natural, cheap future refinement but doesn't resolve it now,
   deliberately, absent real usage data on which tools actually need
   separate gating in practice.
6. **The stale `railmcp-deployment.yaml:61-63` comment** ("Decision 4:
   every DS call this service makes is anonymous"), noted in passing in
   Current relevant state, is left for whoever implements this design (or
   the sibling doc's own follow-up) to correct while already editing that
   file for Discord removal — not itself a design question, just a
   known, small piece of drift worth not forgetting.
