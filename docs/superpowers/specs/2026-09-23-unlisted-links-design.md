# Unlisted links — a generic, reusable share-link subsystem, applied first to journeys

## 1. Intent

A journey's owner wants a "Share" button that mints an unlisted link: a
URL anyone can open, no login required, that shows a read-only view of the
journey. If the person opening it is logged in and already has normal
access (owner, or a member of a group the journey's been shared to via the
existing group-sharing feature), the link must resolve to the exact same
`/journeys/[id]` experience they already get any other way — never a
separate, degraded "unlisted" view for someone who was already allowed in.

The mechanism itself must be a generic, reusable subsystem — not
journey-specific plumbing — so a future feature (sharing a custom line, a
group, anything else) can mint its own unlisted links without
reimplementing token generation, expiry, revocation and resolution from
scratch.

## 2. Precedent read before designing

- `crates/api/src/data/groups.rs` (~536-690) — `group_invite_links`:
  `token` PK, `group_id`, `created_by`, `created_at`, `expires_at`,
  `revoked_at`. `rotate_invite_link` (revoke active + insert fresh, one
  tx), `revoke_invite_link`, `get_active_invite_link`,
  `resolve_invite_link` (unauthenticated preview, never mutates),
  `consume_invite_link` (the one group-specific step: joins the caller as
  a member). Every link expires 7 days after creation — an explicit,
  documented mitigation against "an old, forgotten, still-valid link found
  and reused much later" (line 541-543).
- `crates/api/migrations/20260911090000_shared_groups.sql` — the
  `group_invite_links` table itself: `token TEXT PRIMARY KEY`, not hashed
  (doc comment: the token IS the bearer capability, meant to be shared
  verbatim in a URL; the group's own membership list is the real access
  boundary once joined, not secrecy of this table).
- `crates/api/migrations/20260915100000_custom_line_group_grants.sql`'s
  own comment block establishes that `pinned_lines.line_id` already
  tolerates a free-form, client-supplied, non-FK id — the direct precedent
  for a polymorphic `resource_id` column with no real foreign key, used
  below.
- `crates/api/src/data/journeys.rs::journey_readable_by` (~762) — owner OR
  member of a group the journey's shared into, via
  `group_journeys`/`group_members`. Read-only authorization only, never
  used to gate a write.
- `crates/api/src/routes/journeys.rs::get_journey` — the one route in
  `/Journeys/*` gated on `journey_readable_by` rather than plain
  ownership; embeds `is_owner` in the response so the frontend can gate
  every owner-only action (`frontend/lib/types.ts::JourneyDetail`).
- `frontend/app/groups/join/[token]/page.tsx` — the confirm-before-join
  page: resolves a token via an unauthenticated preview call, then does
  its OWN frontend-side "is this viewer already a member" probe
  (`getGroup(preview.groupId)`, catching 404/401) rather than baking that
  check into the backend preview route. Directly reused below for the
  "already authorized → send them to the normal page" behaviour.
- `frontend/components/GroupInviteLinkCard.tsx` /
  `frontend/components/ShareJourneyButton.tsx` — the two UI shapes this
  app already uses for a share affordance: a persistent Card (groups' own
  detail page, room for one) and a Button-that-opens-a-Modal (the journey
  detail page's compact action row). The new journey share-link button
  follows the second shape — same page, same row.
- `crates/api/src/auth.rs::OptionalAuthenticatedUser` (~281-291) — already
  exists, currently only used by `GET /auth/session`. Not needed by this
  design (see §4) but confirms the codebase already has the primitive if
  a future resource type's resolution needs "resolve differently for a
  logged-in vs anonymous token holder" — journeys don't, because the
  redirect-if-already-authorized decision is made entirely by the
  *frontend* page (see §4), matching the `/groups/join/[token]` precedent
  exactly.
- `frontend/app/api/[...path]/route.ts` — the browser-facing proxy.
  `Journeys` is already in `ROOT_MOUNTED_PREFIXES`, so any new
  `/Journeys/...` route needs no proxy change: a client `fetch('/api/Journeys/...')`
  already forwards to the backend's `/Journeys/...` unprefixed.

## 3. Judgment call 1 — the generic subsystem's shape

**New table `unlisted_links`, new module `crates/api/src/data/unlisted_links.rs`,
left completely separate from `group_invite_links`.**

```sql
CREATE TABLE unlisted_links (
    token         TEXT PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    created_by    TEXT NOT NULL REFERENCES users(id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at    TIMESTAMPTZ,
    revoked_at    TIMESTAMPTZ
);

CREATE INDEX unlisted_links_resource ON unlisted_links (resource_type, resource_id);
```

- `resource_id` is `TEXT`, not a typed FK, and `resource_type` is a bare
  string discriminator (`"journey"` is the only value any caller uses
  today). This is the genuinely generic, polymorphic shape the user asked
  for — no future resource type needs a schema change, only a new
  `resource_type` string constant and its own thin wrapper functions. The
  `pinned_lines.line_id` precedent (§2) is exactly this same tolerance
  already accepted elsewhere in this schema, generalized. The cost: no
  `ON DELETE CASCADE` when a journey (or any future resource) is deleted —
  a dangling `unlisted_links` row for a since-deleted resource is inert
  (see §4: `resolve_link` on a dangling row 404s downstream the moment the
  resource-specific lookup fails to find the resource) rather than
  automatically cleaned up. Accepted for the same reason
  `custom_line_group_grants.granted_by` accepts an un-cascaded
  attribution column: this app has no resource-deletion cleanup sweep for
  polymorphic rows anywhere yet, and a dangling row is harmless, not
  unsafe (worst case: a handful of rows nobody will ever resolve again).
  A future task can add a cleanup sweep per resource type if this ever
  matters at real scale.
- `expires_at` is nullable, unlike `group_invite_links.expires_at` (`NOT
  NULL`). This is deliberate and generic: whether a resource type wants a
  forced TTL is that resource type's own call, made when it calls
  `create_link`/`rotate_link` with `Some(ttl)` or `None`. Journeys choose
  `None` — see Judgment call 3.
- `expires_at`/`revoked_at` reuse `group_invite_links`'s exact validity
  predicate (`revoked_at IS NULL AND (expires_at IS NULL OR expires_at >
  NOW())`), so the two features read as obviously-the-same-idea at a
  glance despite being independent code.

**Explicit decision: `group_invite_links` is NOT retrofitted onto this new
table.** It stays exactly as it is. Three reasons, in order of weight:
1. It works today, is covered by its own tests, and carries
   group-specific behaviour (`consume_invite_link`'s membership-insert
   side effect) that has no generic equivalent — a "generic" version of
   it would need an escape hatch for that side effect anyway, defeating
   half the point of generalizing.
2. The user's ask is about future reusability, not an audit of existing
   code; migrating working code is a real-risk, zero-user-value change
   with no new capability at the end of it.
3. `group_invite_links.expires_at` is `NOT NULL` with a hard 7-day TTL by
   design (§2) — collapsing it onto a nullable-`expires_at` polymorphic
   table would either lose that guarantee at the type level or force a
   `CHECK` that only applies to one `resource_type` value, which is worse
   than just leaving two tables.

`group_invite_links` remains prior art this design leans on, not a
migration target.

## 4. Judgment call 2 — end-to-end resolution flow

**A distinct route: `/journeys/shared/[token]`, resolved almost entirely
by the FRONTEND page component, mirroring `/groups/join/[token]` exactly.**

Backend additions (`crates/api/src/routes/journeys.rs`):
- `POST /Journeys/{journeyId}/share-link` — owner-only (folds `AND
  journeys.user_id = $caller` into its own lookup, the same
  ownership-only convention every OTHER write route in this file already
  uses — `get_journey` is the one deliberate exception, not the rule).
  Calls `unlisted_links::rotate_link(pool, "journey", &journey_id.to_string(), &user.id, None)`.
  Returns `{ token, expiresAt: null }`. Used for BOTH the first "Share"
  click and a later "Regenerate".
- `DELETE /Journeys/{journeyId}/share-link` — owner-only, calls
  `unlisted_links::revoke_link`. `204` either way (idempotent, matching
  `revoke_invite_link`'s own idempotent `204` posture).
- `JourneyDetailResponse` (the existing `GET /Journeys/{journeyId}`
  response) gains one new field, `share_link: Option<ShareLinkResponse>`
  — populated (via `unlisted_links::get_active_link`) only when
  `is_owner` is true, `None` otherwise. No dedicated `GET .../share-link`
  route, for the identical reason `get_active_invite_link`'s own doc
  comment gives: embedding into the resource's existing detail response
  the caller already fetches is simpler than a route with exactly one
  caller. A non-owner (including a group member) never sees a live
  token this way, matching `is_owner`'s existing "gate every owner-only
  affordance" contract (`frontend/lib/types.ts::JourneyDetail`'s own doc
  comment).
- `GET /Journeys/shared/{token}` — genuinely public: no
  `AuthenticatedUser` extractor at all, same posture as `get_join_preview`.
  Resolves the token via `unlisted_links::resolve_link`, confirms
  `resource_type == "journey"`, parses `resource_id` as `i64`, then
  returns the exact same `JourneyDetailResponse` shape `get_journey`
  returns — with `is_owner` hardcoded `false` and `share_link` always
  `None` (a token-only viewer never gets to see or manage the very token
  that let them in). Both handlers share one new private helper,
  `build_journey_detail_response(&app, journey_id, is_owner) ->
  Result<JourneyDetailResponse, ...>`, extracted from `get_journey`'s
  existing body — this is the one refactor to existing code this plan
  makes, and it changes no existing behaviour (`get_journey` still does
  its own `journey_readable_by` check first, then calls the same helper
  it always effectively ran inline). An invalid, expired, revoked, or
  wrong-resource-type token gets `404` — same "don't distinguish
  not-found from not-allowed" posture this codebase uses everywhere else,
  and there is no ownership distinction to leak here in the first place
  since this route has no caller identity at all.
- Router: `.route("/Journeys/shared/{token}", axum::routing::get(get_journey_by_share_token))`,
  added ahead of `/Journeys/{journey_id}` in registration order — pure
  documentation, since `matchit` (the underlying router) already resolves
  a literal segment ahead of a same-position dynamic one regardless of
  registration order, per this file's own existing
  `literal_route_wins_over_same_position_dynamic_route` precedent
  (`routes::train`) and `/groups/join/{token}`'s identical shape. A real
  journey id is always digits (`Path<i64>` extraction), so the literal
  `shared` segment can never collide with one.

Frontend additions:
- `frontend/lib/api.ts::getJourneyByShareToken(token)` — unauthenticated
  fetch (no cookie forwarding needed, though harmless if sent), same
  `ApiNotFoundError`-on-404 contract as `getGroupJoinPreview`.
- `frontend/app/journeys/shared/[token]/page.tsx`:
  1. Call `getJourney`-equivalent access check first: since the caller has
     no journey id yet (only a token), this can't reuse `getJourney`
     directly. Instead it calls `getJourneyByShareToken(token)` FIRST to
     learn the `journeyId` (or render "link not found" on
     `ApiNotFoundError`), then, exactly like `/groups/join/[token]`'s own
     "already a member" probe, tries `getJourney(journeyId)` (which
     forwards the visitor's own cookies) as the "is this viewer already
     properly authorized" check:
     - Succeeds → `redirect(`/journeys/${journeyId}`)`. This is the WHOLE
       mechanism behind "resolves to the normal, already-existing
       group-shared journey view" — the canonical page, the canonical
       fetch, zero new rendering code for this case.
     - Throws `ApiUnauthorizedError` (not logged in at all) or
       `ApiNotFoundError` (logged in, but neither owner nor in a group
       the journey's shared to) → render the token-scoped view using the
       response `getJourneyByShareToken` already fetched. Both errors
       fall through to the same branch: from the token page's point of
       view, "not logged in" and "logged in but not otherwise authorized"
       are the same case — the token is what's carrying their access
       either way, exactly the way `journey_readable_by` itself doesn't
       distinguish "no session" from "wrong session" for its own 404.
  2. Renders via a new shared component, `JourneyDetailView` (extracted
     from `app/journeys/[id]/page.tsx` — see below), with `journey`
     sourced from the token fetch, whose `isOwner` is always `false`. This
     is what makes the read-only guarantee automatic rather than a second
     hand-maintained gate: every owner-only affordance
     (`ShareJourneyButton`/`ShareJourneyLinkButton`/`AddJourneyLegButton`/
     `SaveAsTemplateButton`, `JourneyLegCard`'s own "Change train"/pick
     controls) is already gated on `journey.isOwner` today, for the
     existing group-member read path — a token viewer gets the identical
     read-only rendering a non-owning group member already gets, with no
     new gating logic to write or get wrong.
- `app/journeys/[id]/page.tsx` is refactored (behaviour-preserving) to
  extract its rendering (everything from the `<Stack>` return downward,
  minus the "Back to my trains" link) into
  `frontend/components/JourneyDetailView.tsx`, taking `journey:
  JourneyDetail` as its one prop. `app/journeys/[id]/page.tsx` keeps its
  own fetch/error-branch logic and its own "Back to my trains" link
  (meaningless for an anonymous token visitor, so NOT part of the shared
  component — see Judgment call 4).

This design was chosen over the alternative the brief floated (extending
`journey_readable_by` to accept an optional token as a third access path
checked inline on `/journeys/[id]` itself) for three reasons:
1. `GET /Journeys/{journeyId}` requires `AuthenticatedUser` today — an
   anonymous token holder can never reach that route at all; making the
   existing route accept a token would mean EITHER weakening its
   extractor to `OptionalAuthenticatedUser` (a change every other
   *existing* caller of that route now depends on, for a feature only one
   of them needs) OR adding a second unauthenticated route anyway — at
   which point the "one route" version has bought nothing over two
   separate ones.
2. A distinct URL is what actually IS shared — pasting
   `/journeys/shared/{token}` into a message is self-describing (it's
   obviously a share link), where `/journeys/{id}?token=...` looks like
   an ordinary, guessable, sequential-id URL with an incidental query
   string bolted on, and invites accidentally sharing the bare
   `/journeys/{id}` form instead (which 401s for the recipient).
3. It's the exact shape `/groups/join/{token}` already established for
   the closest existing precedent in this codebase — reusing a pattern
   this app's own reviewers have already exercised is lower-risk than
   inventing a second shape for the same underlying idea.

## 5. Judgment call 3 — revocation/expiry semantics

**No automatic TTL for a journey's unlisted link (`expires_at = NULL` at
creation) — explicit revoke, and explicit regenerate (rotate), are the
owner's only two levers.**

`group_invite_links`' 7-day TTL exists to bound a fundamentally different
risk: a link that GRANTS MEMBERSHIP in a group, an ever-growing,
open-ended boundary — a stale, forgotten, still-valid invite link found
years later can add a stranger to something that has kept accumulating
meaning (and other people's data) the whole time it sat unused. A
journey's unlisted link grants read-only visibility into ONE already-
bounded resource — a specific trip, on a specific `service_date` (Phase 1)
recorded at share-time, whose Phase-2 multi-leg-chaining growth is still
scoped to legs the SAME owner deliberately added to the SAME journey they
already chose to share. Forcing an arbitrary re-share cadence protects
against a threat model (an ever-growing shared boundary) that doesn't
describe this resource, at a real cost to the feature's actual use case —
"share a link so people I picked can watch how this trip is going," which
plausibly wants to stay valid for the trip's full duration and afterward
(reminiscing about a delay-plagued journey after the fact is a real, not
hypothetical, use of this app, given `JourneyTemplateDetail`'s own
existence).

The mitigation for "a leaked link stays a leaked link forever" is the
explicit control surface, not a clock: `ShareJourneyLinkButton` offers
Revoke (kill the link, no replacement) and Regenerate (issue a fresh token,
silently invalidating the old one — `rotate_link`'s existing semantics),
identical to `GroupInviteLinkCard`'s own two-button shape. An owner who
wants time-bounded sharing already has that lever today, just exercised
manually rather than automatically.

This is a real, accepted tradeoff, not an oversight: a share link handed
out and genuinely forgotten stays valid until the owner notices and
revokes it, unlike a group invite link, which self-heals after 7 days
even if nobody remembers it exists. Documented here so it reads as a
judgment call, not a gap. The schema supports either choice per resource
type (§3's nullable `expires_at`) — a future resource type with a
group-invite-link-shaped risk profile can pass `Some(ttl)` to
`create_link`/`rotate_link` and get the same forced-rotation behaviour
`group_invite_links` already has, with zero changes to this table.

## 6. Non-goals

- No unlisted links for anything other than journeys in this plan — the
  generic module is built and tested generically, but no second resource
  type is wired up. That's exactly the point: a future feature can adopt
  it without touching `unlisted_links.rs` at all.
- No rate-limiting or abuse-prevention on `GET /Journeys/shared/{token}`
  beyond the token's own entropy (`auth::generate_session_token`'s
  existing 32-random-byte shape, reused verbatim) — matches
  `resolve_invite_link`'s own unthrottled posture.
- No UI for "who has this link" or view-count tracking — out of scope,
  not asked for.
- `consume`-shaped semantics (a token that changes state when used) are
  deliberately NOT part of the generic module — journeys' `resolve_link`
  never mutates, matching `resolve_invite_link`'s (not
  `consume_invite_link`'s) semantics; the brief itself names this
  explicitly. A future resource type that needs consume-once semantics
  builds that as its own wrapper around `resolve_link`, the same way
  `consume_invite_link` is group-specific code sitting beside (not
  inside) the generic-shaped `group_invite_links` primitives.
