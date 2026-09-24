# Unlisted links — implementation plan

Spec: `docs/superpowers/specs/2026-09-23-unlisted-links-design.md` — read
it before any task below; it is the binding authority for every design
choice referenced here (generic table shape, no `group_invite_links`
retrofit, no TTL for journeys, the `/journeys/shared/[token]` routing
mechanism, `journey_readable_by` untouched). Precedent to read directly:
`crates/api/src/data/groups.rs` lines 534-690 (`group_invite_links`'
`InviteLink`/`rotate_invite_link`/`revoke_invite_link`/
`get_active_invite_link`/`resolve_invite_link`), its migration
`crates/api/migrations/20260911090000_shared_groups.sql`,
`crates/api/src/data/journeys.rs::journey_readable_by` (~762),
`crates/api/src/routes/journeys.rs::get_journey` (~744),
`frontend/app/groups/join/[token]/page.tsx`,
`frontend/components/GroupInviteLinkCard.tsx`,
`frontend/components/ShareJourneyButton.tsx`.

## Global constraints

- Do NOT modify `group_invite_links`, its table, or any of its functions
  in `crates/api/src/data/groups.rs` — it is prior art, not a migration
  target (spec §3).
- Do NOT touch `journey_readable_by` — it stays exactly as documented
  ("READ-ONLY AUTHORIZATION ONLY... must NEVER be used to gate a write").
  The unlisted-link path is a THIRD, entirely separate way to read a
  journey; it never calls or is called by `journey_readable_by`.
- Every new write route (`POST`/`DELETE .../share-link`) is owner-only,
  folding `AND journeys.user_id = $caller` into its own query — the same
  convention every write route in `routes/journeys.rs` other than
  `get_journey` already uses. Never gate a write on `journey_readable_by`.
- The generic module (`crates/api/src/data/unlisted_links.rs`) must not
  import or reference anything journey-specific — it takes
  `resource_type`/`resource_id` as plain `&str` parameters and knows
  nothing about journeys, groups, or any other resource.
- JSON field names are `camelCase` on the wire (`#[serde(rename_all =
  "camelCase")]`), matching every existing route in this file.
- A background agent in a separate worktree is concurrently changing
  `crates/api/src/routes/journeys.rs`, `crates/api/src/data/journeys.rs`,
  and `frontend/app/journeys/[id]/page.tsx` for an unrelated
  whole-journey-deletion feature. Do not attempt to coordinate with it;
  conflicts are the primary session's problem to reconcile at merge time,
  not this branch's.
- Run `cargo fmt --all` and `cargo clippy --workspace --all-features
  --all-targets -- -D warnings` before every commit that touches Rust.
  Run `npx tsc --noEmit` and `npm run lint` (from `frontend/`) before
  every commit that touches TypeScript/TSX.
- DB-gated tests follow this codebase's existing convention exactly:
  `#[tokio::test]` + `#[ignore = "requires a live database; run with
  \`cargo test -p api <test_name> -- --ignored --test-threads=1\`"]`,
  reading `DATABASE_URL` via `std::env::var`. Non-DB unit tests (pure
  functions, JSON shape assertions, router-builds-without-panicking) run
  unconditionally.

## Task 1 — generic `unlisted_links` migration + data module

Create `crates/api/migrations/20260923110000_unlisted_links.sql`:

```sql
-- -------------------------------------------------------------------------
-- Unlisted links: a generic, reusable share-link primitive. One opaque,
-- high-entropy token resolves to one (resource_type, resource_id) pair --
-- polymorphic by a bare string discriminator rather than a typed foreign
-- key, since genericity across future resource types is the whole point
-- (a future feature adds its own resource_type string and calls the
-- functions in crates/api/src/data/unlisted_links.rs; no schema change).
-- See docs/superpowers/specs/2026-09-23-unlisted-links-design.md.
--
-- Deliberately NOT a retrofit of group_invite_links
-- (20260911090000_shared_groups.sql) -- that table stays exactly as it
-- is; this is a new, independent table. See the design doc's own
-- reasoning (§3) for why: group_invite_links carries a group-specific
-- side effect (consume_invite_link's membership insert) with no generic
-- equivalent, and its expires_at is NOT NULL with a hard 7-day TTL by
-- design, which this table's per-resource-type-optional expires_at would
-- either lose or need a conditional CHECK for.
--
-- expires_at is NULLABLE, unlike group_invite_links.expires_at -- whether
-- a resource type forces a TTL is that resource type's own call, made by
-- passing Some(ttl)/None to create_link/rotate_link. Journeys pass None
-- (design doc §5): a journey's link grants read-only access to one
-- already-bounded resource, not an ever-growing membership boundary, so
-- explicit revoke/regenerate are its only two owner-facing levers.
--
-- No ON DELETE CASCADE on resource_id -- it isn't a real foreign key (it
-- can't be: it points at a different table per resource_type). A
-- dangling row after its resource is deleted is inert, not unsafe: the
-- resource-specific resolution step (e.g. looking up a journey by the id
-- this row names) simply fails to find anything and 404s, the same
-- outcome an invalid/expired/revoked token already produces. Matches
-- custom_line_group_grants.granted_by's own accepted un-cascaded
-- attribution column, generalized.
-- -------------------------------------------------------------------------

CREATE TABLE unlisted_links (
    token         TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    created_by    TEXT NOT NULL REFERENCES users(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at    TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);

-- "The active link for this resource" (rotate/revoke/get_active) is
-- always looked up by (resource_type, resource_id) first -- the PK's
-- leading column (token) doesn't cover this, the same reason
-- group_invite_links_group_id exists.
CREATE INDEX unlisted_links_resource ON unlisted_links (resource_type, resource_id);
```

Create `crates/api/src/data/unlisted_links.rs` with this exact public
surface (module doc comment should summarize the spec's §3 reasoning in
your own words, referencing the spec file by path):

```rust
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlistedLink {
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct UnlistedLinkRow {
    token: String,
    expires_at: Option<DateTime<Utc>>,
}

/// Revokes any currently-active link for (resource_type, resource_id) and
/// inserts a fresh one, in one transaction -- exactly
/// `groups::rotate_invite_link`'s own shape, generalized. `ttl` is the
/// caller's own choice per resource type: `Some(d)` sets `expires_at =
/// NOW() + d`, `None` leaves it NULL (no forced expiry). Used for both
/// "create the first link" and "regenerate" -- there is no separate
/// create-only function, matching `rotate_invite_link`'s own dual role.
pub async fn rotate_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    created_by: &str,
    ttl: Option<Duration>,
) -> anyhow::Result<UnlistedLink> {
    // revoke active rows for (resource_type, resource_id), insert a new
    // row with a fresh crate::auth::generate_session_token() token, same
    // transaction, same "revoke WHERE revoked_at IS NULL then INSERT"
    // shape as rotate_invite_link.
}

/// Revokes the active link for (resource_type, resource_id), no
/// replacement. Idempotent: returns `false` if there was nothing active
/// to revoke (not an error) -- same contract as `revoke_invite_link`.
pub async fn revoke_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
) -> anyhow::Result<bool> { /* ... */ }

/// The active link for (resource_type, resource_id), if any -- `None`
/// once revoked or past `expires_at` (when set). Same validity predicate
/// as `get_active_invite_link`: `revoked_at IS NULL AND (expires_at IS
/// NULL OR expires_at > NOW())`.
pub async fn get_active_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
) -> anyhow::Result<Option<UnlistedLink>> { /* ... */ }

/// One resolved (resource_type, resource_id) pair for a token -- `None`
/// if the token doesn't exist or fails the same validity predicate above.
/// Never mutates (this module has no `consume`-shaped function at all --
/// see the design doc's Non-goals: a future resource type that needs
/// consume-once semantics builds it as its own wrapper around this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    pub resource_type: String,
    pub resource_id: String,
}

pub async fn resolve_link(pool: &PgPool, token: &str) -> anyhow::Result<Option<ResolvedLink>> {
    /* ... */
}
```

Implement each function's body against the real `unlisted_links` table,
reusing `crate::auth::generate_session_token()` for token generation
(same call `rotate_invite_link` already makes) and the same `sqlx::query`/
`query_as` style already used throughout `groups.rs`.

Add a `#[cfg(test)] mod db_tests` at the bottom of this new file (mirror
`groups.rs`'s own `db_tests` module structure: a local `connect()` reading
`DATABASE_URL`, a `seed_user` helper). Write these tests, generic — pick
an arbitrary `resource_type` string like `"widget"` and `resource_id`
like `"42"` for all of them, since this module must never assume
journeys exist:

1. `rotate_link_creates_a_link_and_revokes_any_previous_active_one` —
   rotate twice for the same resource; assert the second call's token
   differs from the first, and `get_active_link` returns only the second.
2. `revoke_link_is_idempotent_and_clears_the_active_link` — rotate, then
   revoke twice: first call returns `true`, second returns `false` both
   times `get_active_link` returns `None` after.
3. `resolve_link_returns_none_for_an_expired_link` — insert a row directly
   via `sqlx::query` with `expires_at` in the past (mirror
   `resolve_invite_link_returns_none_for_an_expired_link`'s own direct-
   insert approach in `groups.rs`); assert `resolve_link` returns `None`.
4. `resolve_link_returns_none_for_a_revoked_link` — rotate then revoke;
   assert `resolve_link` returns `None`.
5. `resolve_link_returns_the_resource_for_a_link_with_no_expiry` — rotate
   with `ttl: None`; assert `resolve_link` returns
   `Some(ResolvedLink { resource_type: "widget".into(), resource_id: "42".into() })`.
   This is the one test proving the nullable-`expires_at`, no-forced-TTL
   path (the journeys' own choice, spec §5) actually works end to end in
   the generic layer.
6. `resolve_link_never_confuses_two_different_resources_sharing_an_id` —
   rotate a link for `("widget", "1")` and a separate one for
   `("gadget", "1")` (same `resource_id`, different `resource_type`);
   resolve each token and assert each resolves to its OWN
   `resource_type`, not the other's. This is the concrete proof the
   polymorphic design doesn't collide across resource types that happen
   to share numeric ids.

Clean up every test's own rows with a direct `DELETE FROM unlisted_links
WHERE resource_type = $1` at the end (no cascade to rely on — this table
has no FK to a resource).

**Report file contract**: DONE with commit range, `cargo fmt --all --check`
and `cargo clippy -p api --all-features --all-targets -- -D warnings`
output, and confirmation these tests compile (`cargo test -p api
--no-run`) even though they can't run without a live database in this
task's own verification (note whether DATABASE_URL was reachable and, if
so, the live pass/fail for all 6).

## Task 2 — journey route wiring: share-link routes + public token route

Builds on Task 1's `unlisted_links` module. Read
`crates/api/src/routes/journeys.rs` in full before starting (it's long;
you need the existing `get_journey` handler, `JourneyDetailResponse`,
`internal_error`, and the router's existing route list).

**Resource-type constant**: add `const JOURNEY_RESOURCE_TYPE: &str =
"journey";` near the top of `crates/api/src/routes/journeys.rs` (not in
`data/journeys.rs` — this is routing-layer wiring, `data/journeys.rs`
stays untouched by this task per the Global Constraints above; every call
into `unlisted_links` happens from the route handlers, passing
`journey_id.to_string()` as `resource_id`).

**Refactor `get_journey` (behaviour-preserving)**: extract everything
after the `is_owner` computation through the final `Ok(Json(...))` into a
new private function:

```rust
async fn build_journey_detail_response(
    app: &App,
    journey_id: i64,
    is_owner: bool,
) -> Result<JourneyDetailResponse, (StatusCode, String)> { /* existing body */ }
```

`get_journey` becomes: check `journey_readable_by` (unchanged), fetch
`get_journey_summary` (unchanged) to compute `is_owner`, then `return
build_journey_detail_response(&app, journey_id, is_owner).await`. This
must not change `get_journey`'s existing behaviour or break any existing
test in this file — run the full existing test module for this file
after the refactor, before adding anything new, and confirm every
existing test still passes.

**`JourneyDetailResponse` gains one field**:

```rust
share_link: Option<ShareLinkResponse>,
```

with

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShareLinkResponse {
    token: String,
    expires_at: Option<DateTime<Utc>>,
}
```

Populated inside `build_journey_detail_response`: when `is_owner` is
`true`, call `unlisted_links::get_active_link(&app.database,
JOURNEY_RESOURCE_TYPE, &journey_id.to_string())` and map `Some(link) ->
Some(ShareLinkResponse { token: link.token, expires_at: link.expires_at })`.
When `is_owner` is `false`, always `None` — a non-owner (including a
group member, and including the public token route below) never sees a
live, usable token this way.

**New routes**, added to `router()`:

```rust
.route("/Journeys/shared/{token}", axum::routing::get(get_journey_by_share_token))
.route(
    "/Journeys/{journey_id}/share-link",
    axum::routing::post(create_journey_share_link).delete(revoke_journey_share_link),
)
```

- `create_journey_share_link(State(app), user: AuthenticatedUser,
  Path(journey_id): Path<i64>) -> Result<Json<ShareLinkResponse>,
  (StatusCode, String)>` — first confirm ownership with a direct,
  ownership-scoped query (`SELECT EXISTS(SELECT 1 FROM journeys WHERE id
  = $1 AND user_id = $2)`, the same folded-in-`WHERE` convention every
  other write route in this file uses — do NOT call
  `journey_readable_by` here); `404` (not `403`) if the caller doesn't
  own it, same convention as every other write route in this file. Then
  `unlisted_links::rotate_link(&app.database, JOURNEY_RESOURCE_TYPE,
  &journey_id.to_string(), &user.id, None)` (the `None` TTL is the
  journeys-specific choice from spec §5 — do not pass `Some(...)`), map
  to `ShareLinkResponse`.
- `revoke_journey_share_link(...) -> Result<StatusCode, (StatusCode,
  String)>` — same ownership check, then
  `unlisted_links::revoke_link(...)`, return `StatusCode::NO_CONTENT`
  regardless of whether a row was actually revoked (idempotent, matching
  `revoke_invite_link`'s route-level `204`-either-way posture).
- `get_journey_by_share_token(State(app), Path(token): Path<String>) ->
  Result<Json<JourneyDetailResponse>, (StatusCode, String)>` — NO
  `AuthenticatedUser` parameter at all (genuinely public, matching
  `get_join_preview`'s own shape in `routes/groups.rs`). Call
  `unlisted_links::resolve_link`; `404` ("no journey with that link" —
  match this codebase's existing terse error-string style) if `None`, or
  if `resolved.resource_type != JOURNEY_RESOURCE_TYPE`, or if
  `resolved.resource_id.parse::<i64>()` fails. On success, call
  `build_journey_detail_response(&app, journey_id, false)` — `is_owner`
  is always `false` here, never derived from the token or any caller
  identity.

**Router registration order**: place the new `/Journeys/shared/{token}`
route ahead of `/Journeys/{journey_id}` in the `Router::new()...route(...)`
chain (pure documentation — see the spec's own note that `matchit`
resolves literal segments ahead of same-position dynamic ones regardless
of order, per this file's existing `literal_route_wins_over_same_position_dynamic_route`
precedent in `routes::train`).

**Tests** (append to this file's existing `#[cfg(test)] mod db_tests`,
following the exact `test_app`/`test_router`/`seed_session`/`request`/
`post_json`/`delete_request`/`cleanup_user`/`cleanup_group` helpers
already defined there — do not duplicate them):

1. `create_journey_share_link_then_resolve_it_via_the_public_token_route`
   — owner creates a journey (known-train leg, mirror the existing
   `get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized`
   test's own journey-creation call), `POST
   /Journeys/{id}/share-link` as the owner (`200`, body has a non-empty
   `token` and `expiresAt: null`), then `GET /Journeys/shared/{token}`
   with **no cookie at all** — assert `200`, `isOwner: false`,
   `shareLink: null`, and the same leg detail the owner would see.
2. `create_journey_share_link_is_404_for_a_non_owner` — a second,
   unrelated authenticated user `POST`s the same journey's share-link
   route; assert `404` (mirror
   `post_leg_train_a_leg_owned_by_someone_else_is_404_not_403`'s own
   404-not-403 assertion style).
3. `get_journey_by_share_token_is_404_for_an_unknown_token` — no journey
   created at all; `GET /Journeys/shared/not-a-real-token` with no
   cookie; assert `404`.
4. `get_journey_by_share_token_is_404_after_revoke` — create + share-link,
   confirm the token resolves once (`200`), `DELETE
   /Journeys/{id}/share-link` as the owner (`204`), then the SAME token
   against `/Journeys/shared/{token}` now `404`s.
5. `get_journey_by_share_token_a_token_viewer_sees_no_owner_actions` —
   resolve a valid token with no cookie; assert the body's `isOwner` is
   `false` and `shareLink` is `null` (a token-only viewer never gets to
   see or manage the very token that let them in — spec §4).
6. `regenerating_a_share_link_invalidates_the_old_token` — `POST` the
   share-link route twice as the owner; assert the two returned tokens
   differ, and the FIRST token now `404`s against `/Journeys/shared/{token}`
   while the second still resolves `200`.
7. `get_journey_embeds_the_active_share_link_for_the_owner_only` — owner
   creates a journey, `POST`s a share-link, then `GET
   /Journeys/{id}` (the normal, existing, owner-authenticated route) as
   the OWNER and assert `shareLink.token` matches; then as a fellow group
   member (reuse the group-sharing seeding from the existing
   `get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized`
   test) and assert `shareLink` is `null` there even though the group
   member CAN read the journey.

Run this file's FULL existing test module (not just the new tests) after
the refactor and after adding the new tests, to confirm nothing broke.

**Report file contract**: DONE with commit range, `cargo fmt --all
--check`, `cargo clippy -p api --all-features --all-targets -- -D
warnings`, `cargo test -p api --no-run` output, and — if `DATABASE_URL`
is reachable in this task's environment — the live `--ignored` pass/fail
for every test touched or added in this file (old and new).

## Task 3 — frontend types + `api.ts`

Depends on Task 2's wire shapes. Edit `frontend/lib/types.ts` and
`frontend/lib/api.ts` only.

In `frontend/lib/types.ts`, add (near `GroupInviteLink`, ~line 1264, for
proximity to its closest sibling shape):

```typescript
/** `POST /Journeys/{id}/share-link`'s response, and the `shareLink` field
 * embedded on `JourneyDetail` for the owner only. `expiresAt` is always
 * `null` today -- journeys choose no forced TTL (design doc
 * docs/superpowers/specs/2026-09-23-unlisted-links-design.md §5) -- kept
 * as `string | null` rather than always-`null` so the type doesn't lie if
 * that choice is ever revisited. */
export interface JourneyShareLink {
  token: string;
  expiresAt: string | null;
}
```

Add one field to the existing `JourneyDetail` interface (~line 831, right
after `isOwner: boolean;`):

```typescript
/** The journey's currently active unlisted share link, owner-view only
 * -- always `null` for a non-owner (a group member, or a viewer who
 * reached this journey via the share link itself). See `JourneyShareLink`
 * and `ShareJourneyLinkButton.tsx`. */
shareLink: JourneyShareLink | null;
```

In `frontend/lib/api.ts`, add one function immediately after
`getJourney` (~line 667), matching its exact doc-comment style and
`errorForResponse` contract:

```typescript
/** `GET /Journeys/shared/{token}` -- genuinely unauthenticated (no cookie
 * needed, though harmless if sent): resolves an unlisted share-link
 * token to the same `JourneyDetail` shape `getJourney` returns, with
 * `isOwner: false` and `shareLink: null` always. Throws `ApiNotFoundError`
 * for an unknown, expired, or revoked token -- same contract
 * `getGroupJoinPreview` already uses for its own token-not-found case. */
export async function getJourneyByShareToken(token: string): Promise<JourneyDetail> {
  const url = `${baseUrl()}/Journeys/shared/${token}`;
  const response = await fetch(url, { cache: 'no-store' });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyDetail>;
}
```

No mutation functions belong in `api.ts` — creating/revoking a share link
is a browser-initiated fetch against the `/api/Journeys/{id}/share-link`
proxy path (`Journeys` is already in `ROOT_MOUNTED_PREFIXES` in
`frontend/app/api/[...path]/route.ts` — verify this by reading that file,
do not modify it), exactly the way `ShareJourneyButton.tsx` calls
`fetch('/api/groups/${id}/journeys', ...)` directly rather than through
`api.ts`. Task 4's component makes those calls itself.

**Tests**: extend `frontend/lib/api.test.ts` if it exists (check first;
follow its existing per-function test shape) with a test for
`getJourneyByShareToken`'s 404-becomes-`ApiNotFoundError` mapping, mocking
`fetch` the same way this file's existing `getJourney`/`getGroupJoinPreview`
tests do. If no such test file exists for `api.ts` functions today, skip
this — do not invent a new test-file convention; note in your report
which case applied.

**Report file contract**: DONE with commit range, `npx tsc --noEmit` and
`npm run lint` output (run from `frontend/`), and the test command/output
if a test was added.

## Task 4 — `ShareJourneyLinkButton` component + wire into the journey page

Depends on Task 3's types/`api.ts` addition and Task 2's routes. Read
`frontend/components/GroupInviteLinkCard.tsx` and
`frontend/components/ShareJourneyButton.tsx` in full before starting —
this component is a deliberate hybrid of both: `ShareJourneyButton`'s
Button-that-opens-a-Modal shape (the journey page's action row is
buttons, not cards), containing `GroupInviteLinkCard`'s content (URL
field, copy/share icon, Regenerate/Revoke buttons, expiry line — except
there is no expiry to show, see below).

Create `frontend/components/ShareJourneyLinkButton.tsx`:

- Props: `{ journeyId: number; shareLink: JourneyShareLink | null; origin: string }`
  — `shareLink` and `origin` are both server-resolved, passed down from
  `app/journeys/[id]/page.tsx` exactly the way `GroupInviteLinkCard`
  receives `inviteLink`/`origin` from `app/groups/[id]/page.tsx`
  (`getSiteOrigin()`, already imported there — add the same import to
  `app/journeys/[id]/page.tsx`).
- `'use client'`. `useState` for `opened`/`busy`/`error`/`copied`,
  `useDisclosure` for the modal (match `ShareJourneyButton`'s exact
  imports), `useNeedsLogin()` for the 401 case (match
  `GroupInviteLinkCard`'s exact usage), `useRouter().refresh()` after a
  successful create/regenerate/revoke (match `GroupInviteLinkCard`
  exactly — a `router.refresh()` re-fetches the Server Component's
  `getJourney` call, which re-populates `shareLink` from the backend, the
  same way it re-populates `inviteLink` on that page today).
- Button label: `"Get shareable link"` when `shareLink` is `null`,
  `"Manage shared link"` when it isn't — the one piece of copy this
  component invents (no precedent covers "no link yet" vs "link exists"
  as two different trigger labels since `GroupInviteLinkCard` is always
  visible, never behind a trigger button). Justify this exact choice in
  a doc comment if you pick different wording — the point is a caller
  should immediately understand whether clicking will show an existing
  link or offer to create the first one.
- Modal title: `"Share this journey"`. Body: same URL-field +
  copy/share-icon layout as `GroupInviteLinkCard` (`${origin}/journeys/shared/${shareLink.token}`),
  a `Regenerate`/`Create link` button (`POST /api/Journeys/${journeyId}/share-link`,
  label `"Create link"` when `shareLink` is `null`, `"Regenerate"`
  otherwise) and, only when `shareLink` isn't `null`, a `Revoke` button
  (`DELETE /api/Journeys/${journeyId}/share-link`). NO expiry line — spec
  §5's own choice is no TTL for journeys, so there is nothing to report;
  do not invent an expiry display for a value that is always `null`.
  Instead, add one short static line under the buttons: `"Anyone with this
  link can view this journey (read-only) without logging in, until you
  revoke it."` — the plain-language equivalent of spec §5's "no automatic
  TTL, explicit revoke is the lever" for the one audience that actually
  needs to know it (the owner deciding whether to share).
- 401 handling: identical shape to `GroupInviteLinkCard.regenerate`/
  `.revoke` (`needsLoginState.markNeedsLogin()` on a `401` response,
  `<LoginLink underline="always">Log in to manage this journey's share
  link</LoginLink>` rendered when `needsLoginState.needsLogin`).

Wire into `app/journeys/[id]/page.tsx`: add
`{journey.isOwner && <ShareJourneyLinkButton journeyId={journey.id} shareLink={journey.shareLink} origin={origin} />}`
next to the existing `{journey.isOwner && <ShareJourneyButton
journeyId={journey.id} />}` (same owner-only gate, placed immediately
after it — group-share and unlisted-link share read as two flavours of
the same "Share" concern and should sit adjacent). Add `const origin =
await getSiteOrigin();` near this page's existing `const fetchedAt = ...`
line, and the corresponding import.

**Tests**: `frontend/components/ShareJourneyLinkButton.test.tsx`, mirror
`GroupInviteLinkCard.test.tsx`'s structure exactly (`vi.stubGlobal('fetch', ...)`,
`renderWithMantine`, `next/navigation` mock for `useRouter().refresh`).
Cover at minimum:
1. Renders "Get shareable link" when `shareLink` is `null`; opening the
   modal shows "Create link", no "Revoke" button, no URL field.
2. Renders "Manage shared link" when `shareLink` is non-null; opening the
   modal shows the built URL (`${origin}/journeys/shared/${token}`), a
   "Regenerate" button, and a "Revoke" button.
3. Clicking "Create link"/"Regenerate" POSTs to
   `/api/Journeys/{journeyId}/share-link` and calls `router.refresh()` on
   success.
4. Clicking "Revoke" DELETEs `/api/Journeys/{journeyId}/share-link` and
   calls `router.refresh()` on success.
5. A `401` response from either call shows the login prompt rather than a
   generic error.

**Report file contract**: DONE with commit range, `npx tsc --noEmit`,
`npm run lint`, and the vitest output for this new test file (and confirm
`ShareJourneyButton.test.tsx`/`app/journeys/[id]/page.test.tsx` if it
exists still pass after the page edit).

## Task 5 — extract `JourneyDetailView` (behaviour-preserving refactor)

No dependency on Tasks 3/4's new files beyond their existence in
`app/journeys/[id]/page.tsx` (do this task on top of Task 4's branch
state, since Task 4 already edited that page file — do not attempt to
reorder ahead of Task 4).

Read `frontend/app/journeys/[id]/page.tsx` in full (current state, after
Task 4's edit). Extract into a new `frontend/components/JourneyDetailView.tsx`:
`defaultJourneyTitle`, `legSummaryPhrase`, `LegConnector`, and the
rendering currently inside the page's `return (<Stack p="lg" gap="md">...)`
block — EXCLUDING the top `<TextLink href="/track/mine" ...>Back to my
trains & journeys</TextLink>` line, which stays page-specific (see Task
6's own reasoning for why: an anonymous share-link viewer has no "my
trains" to go back to).

`JourneyDetailView` signature: `export function JourneyDetailView({
journey, fetchedAt }: { journey: JourneyDetail; fetchedAt: string })`.
Everything else (the `<Group justify="space-between">` header, status
badge, owner-only action buttons, leg cards, connectors) moves verbatim —
this is a pure extraction, not a rewrite; do not change any rendered
markup, class, prop, or conditional. `app/journeys/[id]/page.tsx` keeps
its own `getJourney` fetch, error-handling branches (`ApiNotFoundError`/
`ApiUnauthorizedError`), the "Back to my trains" link, `getSiteOrigin()`
call, and now renders `<JourneyDetailView journey={journey}
fetchedAt={fetchedAt} />` in place of the extracted markup.

This task must not change `app/journeys/[id]/page.tsx`'s observable
behaviour at all. Run the existing test file for that page (find it —
likely `frontend/app/journeys/[id]/page.test.tsx`; if none exists, note
that in your report rather than inventing new coverage this task doesn't
own) and confirm it still passes unmodified, or with only the minimal
import-path changes a pure extraction requires (e.g. if the test imports
`defaultJourneyTitle` directly, it now imports it from
`JourneyDetailView.tsx` instead — update the import path only, not the
assertion).

**Report file contract**: DONE with commit range, `npx tsc --noEmit`,
`npm run lint`, and the full output of whatever vitest suite covers
`app/journeys/[id]/page.tsx` and/or the new `JourneyDetailView.tsx`,
confirming no behavioural diff.

## Task 6 — `/journeys/shared/[token]` page

Depends on Task 3 (`getJourneyByShareToken`) and Task 5
(`JourneyDetailView`). Read `frontend/app/groups/join/[token]/page.tsx`
in full before starting — this page follows its exact shape (resolve
token first, then probe whether the current viewer is already
authorized the normal way, branch on the outcome).

Create `frontend/app/journeys/shared/[token]/page.tsx`:

```typescript
export const revalidate = 0;

export default async function SharedJourneyPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;

  let journey;
  try {
    journey = await getJourneyByShareToken(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Link not found</Title>
          <Alert color="red">This share link is invalid or has been revoked. Ask whoever shared it for a new one.</Alert>
        </Stack>
      );
    }
    throw err;
  }

  // Already-authorized probe -- exact same shape as
  // app/groups/join/[token]/page.tsx's own "alreadyMember" check: try the
  // normal, cookie-forwarding fetch this viewer would use on any other
  // route into the journey. Success means they're the owner or a member
  // of a group it's shared to (journey_readable_by, unchanged) -- send
  // them to the real page, never a second, degraded rendering of the
  // same data.
  try {
    await getJourney(journey.id);
    redirect(`/journeys/${journey.id}`);
  } catch (err) {
    if (!(err instanceof ApiNotFoundError) && !(err instanceof ApiUnauthorizedError)) {
      throw err;
    }
    // Falls through: this viewer is relying on the token itself, either
    // because they aren't logged in at all (ApiUnauthorizedError) or
    // because they're logged in but neither own this journey nor belong
    // to a group it's shared to (ApiNotFoundError) -- journey_readable_by's
    // own two negative outcomes, indistinguishable by design (see that
    // function's doc comment), and indistinguishable here for the same
    // reason: the token is what's carrying their access regardless of
    // which case they're in.
  }

  const fetchedAt = new Date().toISOString();

  return (
    <Stack p="lg" gap="md">
      <Text size="xs" c="dimmed">
        You&apos;re viewing this journey via a shared link.
      </Text>
      <JourneyDetailView journey={journey} fetchedAt={fetchedAt} />
    </Stack>
  );
}
```

Adjust imports precisely (`redirect` from `next/navigation`, `getJourney`/
`getJourneyByShareToken`/`ApiNotFoundError`/`ApiUnauthorizedError` from
`@/lib/api`, `JourneyDetailView` from `@/components/JourneyDetailView`,
Mantine `Stack`/`Title`/`Alert`/`Text`). No `notFound()` call anywhere in
this page — an invalid token is a real, expected outcome with its own
explanatory copy (matching `/groups/join/[token]`'s own "Invite link not
found" branch, not a bare 404).

`journey.isOwner` is always `false` on the object `getJourneyByShareToken`
returns (Task 2's own server-side guarantee), so `JourneyDetailView`
automatically renders every owner-only affordance
(`AddJourneyLegButton`/`ShareJourneyButton`/`ShareJourneyLinkButton`/
`SaveAsTemplateButton`, `JourneyLegCard`'s "Change train"/candidate
picker) as hidden — this is the whole mechanism that makes the token view
read-only; do not add any second, hand-written gate for this on top of
what `JourneyDetailView` already does.

**Tests**: `frontend/app/journeys/shared/[token]/page.test.tsx`, mirror
`frontend/app/groups/join/[token]/page.test.tsx`'s structure (mock
`@/lib/api`, mock `next/navigation`'s `redirect` as a function that
throws — Next's own `redirect()` works by throwing internally, so a
`vi.fn(() => { throw new Error('REDIRECT'); })`-shaped mock plus asserting
the mock was called with the right path is the usual pattern; check that
test file's actual mock shape and copy it exactly rather than inventing a
different one). Cover at minimum:
1. `getJourneyByShareToken` throwing `ApiNotFoundError` renders "Link not
   found", never calls `getJourney`.
2. Token resolves, and `getJourney` (the authorized-probe call) SUCCEEDS
   → `redirect('/journeys/{id}')` is called, and the token-scoped view is
   NOT rendered (no leg content in the output).
3. Token resolves, and `getJourney` throws `ApiUnauthorizedError` (not
   logged in) → renders the token-scoped view via `JourneyDetailView`,
   `redirect` is never called.
4. Token resolves, and `getJourney` throws `ApiNotFoundError` (logged in,
   not authorized) → same as case 3: renders the token-scoped view,
   `redirect` never called.
5. The rendered token-scoped view shows no owner-only actions (assert
   absence of "Share with a group"/"Get shareable link"/"Add a leg"/"Save
   as template" — reuse `journey.isOwner: false` in the mocked
   `getJourneyByShareToken` response, matching Task 2's real guarantee).

**Report file contract**: DONE with commit range, `npx tsc --noEmit`,
`npm run lint`, and this new test file's vitest output.

## Non-goals (do not implement)

- No second resource type wired onto `unlisted_links` — spec §6.
- No rate-limiting/abuse-prevention on the public token route beyond the
  token's own entropy — spec §6.
- No "who has this link"/view-count UI — spec §6.
- No `consume`-shaped (state-mutating) resolution for journeys or the
  generic module — spec §6.
- No changes to `group_invite_links`, `journey_readable_by`, or any file
  the concurrent whole-journey-deletion background agent is likely
  touching beyond the specific additions named in Tasks 2/4/5/6 above.
