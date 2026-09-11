# Shared Groups for Train Tracking

**Status: approved design, ready for implementation planning.**

## 1. Motivation and decided direction

Today `train_subscriptions` is strictly single-owner: one row per user per
tracked train, with no sharing or multi-user concept anywhere in the schema.
The underlying `trains` table (schedule/live-movement data) is already
shared/public — `GET /Train/by-uid/{uid}/{date}` is public and unauthenticated
once a train resolves to a real UID. What's actually private is the
per-subscription overlay: custom name, notification prefs, and tickets.

A standalone "share this tracked train via a link" feature was considered and
rejected: it would mostly duplicate the already-public `{uid}/{date}` page
once a train resolves, with the only genuine gaps being pre-resolution
visibility and carrying the tracker's custom name along.

**Decided direction: shared groups are the sharing mechanism**, collapsing
"share a tracked train" and "groups to tag/organize journeys" into one
feature. Users create named groups with multiple members; a tracked train
added to a group becomes visible to other group members (custom name +
live status), covering the pre-resolution gap for free. Tickets are **never**
shared under any circumstances — a hard constraint, not configurable.

Membership is granted via a shareable join link, not email invites:
`users.email` is only populated post-login and only when the IdP asserts
`email_verified: true` (see `docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md`),
so there's no reliable way to resolve "invite jane@example.com" to a real
`user_id` before she's ever logged in. A join link sidesteps needing to
identify anyone in advance — this app has no user-directory/lookup feature
at all today, and building one is out of scope for this feature.

## 2. Schema

### 2.1 `groups` and `group_members`

```sql
CREATE TABLE groups (
    id          TEXT PRIMARY KEY,   -- short random id, generated server-side
                                     -- (same shape as auth::generate_session_token()),
                                     -- doubles as a non-guessable URL identifier
    name        TEXT NOT NULL,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE group_members (
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member')),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, user_id)
);
```

`role` has three tiers, not two — **the group creator is a permanent
`owner`**, distinct from promotable `admin`s (decided: an owner can never be
removed or demoted by a co-admin, protecting against being locked out of a
group you created). The creator is inserted as `owner` in the same
transaction that creates the group. Promoted `admin`s have the same
day-to-day management powers as the owner (invite-link management, member
removal, renaming) except they can never remove, demote, or otherwise act on
the `owner` row itself.

**Last-admin-leaves**: if the sole `owner` leaves a group that still has
other members, ownership must transfer rather than leaving the group
ownerless — auto-promote the longest-standing remaining `admin` (by
`joined_at`), or if none exists, the longest-standing remaining `member`, to
`owner`. An owner attempting to leave a group with no other members present
simply deletes the group instead (nothing to hand off to).

### 2.2 `group_trains` (join table, not a direct FK)

```sql
CREATE TABLE group_trains (
    group_id               TEXT   NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    train_subscription_id  BIGINT NOT NULL REFERENCES train_subscriptions(id) ON DELETE CASCADE,
    added_by               TEXT   NOT NULL REFERENCES users(id),
    added_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, train_subscription_id)
);
```

A join table, not a `group_id` FK directly on `train_subscriptions`, so a
train can be shared into more than one group at once (e.g. relevant to both
a "family" group and a "coworkers" group). This repo already paid the cost
of under-modeling an analogous relationship once — `tracked_train_tickets`
originally assumed "one ticket, one train" and needed a nullable-FK retrofit
(`20260901140000_standalone_tickets.sql`) — a join table avoids repeating
that mistake here for a comparably cheap extra table.

`ON DELETE CASCADE` on `train_subscription_id` means untracking a train
removes it from every group it was shared into immediately, automatically,
with no application code needed — the same cascade pattern already used for
`train_movement_events`/`train_current_state`.

Enforced at the application layer, not the DB: adding a row requires
`train_subscriptions.user_id = current_user AND train_subscriptions.id = $1`
(same ownership-check shape already used for ticket ownership,
`WHERE id = $1 AND user_id = $2` in `crates/api/src/data/train_tracking.rs`).

**Departed-member cleanup (decided): pulled automatically.** When a
`group_members` row is deleted (member leaves or is removed), delete their
`group_trains` rows where `added_by = <that user>` in the same transaction —
a departed member's train should not linger, attributed to someone no longer
in the group.

### 2.3 `group_invite_links`

```sql
CREATE TABLE group_invite_links (
    token       TEXT PRIMARY KEY,  -- opaque, high-entropy; reuse
                                     -- auth::generate_session_token() directly
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,   -- see default-expiry decision below
    revoked_at  TIMESTAMPTZ             -- nullable: set on manual revoke/rotate
);
```

A row is valid to redeem iff `revoked_at IS NULL AND expires_at > NOW()`.

**Reusable, with a default expiry (decided): 7 days.** Unlike the original
proposal's "no expiry" default, every link expires 7 days after creation
unless rotated — a low-effort mitigation against an old, forgotten, still-valid
link being found and reused much later, while still supporting the casual
"share via text/chat" use case within that window. Regenerating a link
(`POST /groups/{id}/invite-link`) sets `revoked_at = NOW()` on any existing
active link and inserts a fresh one with a new 7-day `expires_at` — rotation
and "extend the window" are the same action, matching how a leaked API key
is typically handled.

**Confirm-before-join, never silent auto-join.** Visiting
`/groups/join/{token}` renders a confirmation page ("Join {group name}? ...")
with an explicit Join button — never joins on GET. If the visitor isn't
logged in, the token is preserved through the existing OIDC login redirect
(mirroring `validate_return_to` in `crates/api/src/auth.rs`) so the same
confirm page renders again, now authenticated, before the explicit join
action.

## 3. Permissions model

| Action | Who |
|---|---|
| Create a group | Any authenticated user (creator becomes `owner`) |
| View group (members + shared trains) | Any current member |
| Add own train to a group | Any current member, for a train **they own** |
| Remove a train from a group | The member who added it, or any `admin`/`owner` |
| Invite (generate/rotate/revoke join link) | `admin` or `owner` |
| Remove another member | `admin` or `owner` — but an `admin` can never remove the `owner` |
| Promote a member to `admin` | `owner` only |
| Leave the group (remove self) | Any member; if the sole `owner` leaves a non-empty group, ownership transfers first (§2.1) |
| Rename the group | `admin` or `owner` |
| Delete the group entirely | `owner` only |

No cap is placed on members-per-group or groups-per-user for this initial
version, consistent with this app not capping `custom_lines`/`pinned_lines`
counts either.

## 4. What a group member sees for a shared train

- **Shown**: the tracker's `custom_name` (same computed default the tracker
  themselves would see if unset, per `custom_tracking_names`'s Decision 3 —
  never stored, always computed), the train's live status, and — **decided:
  attribution is shown** — which member added it ("shared by Alex"). Groups
  are explicit and opt-in; members already know each other well enough to
  have joined together, so this doesn't conflict with the separate,
  unrelated cross-user-anonymity goal in
  `docs/superpowers/specs/2026-09-06-shared-train-identity-design.md`, which
  is scoped to unrelated strangers on the public site, not fellow group
  members.
- **Never shown**: tickets (hard constraint), the tracker's
  `notifications_enabled` or any other private per-subscription preference,
  and the exact `tracked_at` timestamp (low value to other members, mildly
  diary-like).

## 5. API surface

All under the existing session-authenticated `public_router()`, not the
internal-token gate.

| Method + path | Purpose |
|---|---|
| `POST /groups` | Create a group; creator becomes `owner` |
| `GET /groups` | List groups the current user is a member of |
| `GET /groups/{id}` | Group detail: name, owner, member count |
| `PUT /groups/{id}` | Rename (`admin`/`owner`) |
| `DELETE /groups/{id}` | Delete group (`owner` only) |
| `GET /groups/{id}/members` | List members with roles |
| `DELETE /groups/{id}/members/{userId}` | Remove a member (`admin`/`owner`, never targeting the owner), or self-remove ("leave") |
| `POST /groups/{id}/members/{userId}/promote` | Promote a member to `admin` (`owner` only) |
| `POST /groups/{id}/invite-link` | Generate/rotate the group's active join link (`admin`/`owner`) |
| `DELETE /groups/{id}/invite-link` | Revoke the active join link with no replacement (`admin`/`owner`) |
| `GET /groups/join/{token}` | Resolve a join token → group name/preview, for the confirm page (no membership change) |
| `POST /groups/join/{token}` | Consume the token: add the current authenticated user to `group_members` |
| `GET /groups/{id}/trains` | List trains shared into the group |
| `POST /groups/{id}/trains` | Add one of the current user's own `train_subscriptions` rows to the group |
| `DELETE /groups/{id}/trains/{trainSubscriptionId}` | Remove a train from the group (sharer, or `admin`/`owner`) |

## 6. Frontend surface

**Decided: a new top-level `Groups` nav item**, alongside `Distant Signal` /
`All Lines` / `Station Lookup` / `Find a Train` / `My Trains & Tickets` in
`frontend/app/layout.tsx` — a group has its own members/invite-link/settings
surface, not just a filtered view over tracked trains, matching how tickets
and tracked trains already got their own combined page rather than being
folded together. Only rendered for authenticated users, same conditional
pattern as `AuthNavItem`/`TrackedTrainsNavItem`.

- `/groups` — list of the user's groups (name, member count, role badge,
  "Create group" CTA).
- `/groups/new` — create form (name only); on success, immediately generate
  the group's first invite link.
- `/groups/{id}` — group detail, sectioned into:
  - **Members**: list with role badges; "Remove" per row for `admin`/`owner`
    (disabled against the `owner` row); "Promote to admin" for `owner`;
    "Leave group" for the current user; the current invite link with
    copy/share affordances (reuse `ShareButton.tsx`'s copy-to-clipboard /
    Web Share pattern verbatim) plus "Regenerate" and "Revoke", visible only
    to `admin`/`owner`.
  - **Shared trains**: same visual language as the existing tracked-trains
    list — custom name, live status, "shared by {member}" attribution, a
    remove action for the sharer/`admin`/`owner`. An "Add one of my trains"
    picker sourced from the user's own `/track/mine` list.
- `/groups/join/{token}` — the confirm-before-join page (§2.3): group name,
  a plain-language explanation of what joining means, explicit Join button;
  routes through the existing OIDC login with the token preserved if not
  already authenticated.

## 7. Alternatives considered and rejected

1. **Direct `group_id` FK on `train_subscriptions`.** Simpler, but forecloses
   multi-group sharing and repeats the exact "assumed one relationship,
   needed several" mistake already paid down once for tickets. Rejected in
   favor of the `group_trains` join table (§2.2).
2. **Email-based invite tokens.** Rejected: `users.email` is only reliably
   known post-login with `email_verified: true`, so there's no way to
   resolve an invite to a real `user_id` in advance, and no user-directory
   feature exists to look anyone up by. A join link sidesteps needing to
   identify anyone ahead of time.
3. **Plain array column for membership** (`groups.member_ids TEXT[]`,
   mirroring `custom_lines.stations`). Rejected: group membership needs a
   role and a join timestamp per member and needs efficient bidirectional
   querying ("which groups is user X in") — a real join table fits better
   than the array-column precedent, which works for `custom_lines` only
   because station membership carries no per-member metadata.
4. **Single-use, per-invitee invite tokens.** Rejected as the default in
   favor of one reusable, rotatable link per group (§2.3) — closer to this
   app's existing `ShareButton` precedent (a durable link shared casually via
   text/chat) than a security-sensitive onboarding flow. The schema doesn't
   preclude an admin wanting stricter single-use behavior later, but nothing
   in this feature's requirements calls for tracking individual invitees by
   name before they join.

## 8. Non-goals

- No user directory/lookup feature (inviting a specific known person by
  searching for them) — out of scope; the join-link mechanism exists
  specifically to avoid needing this.
- No configurability of ticket sharing — tickets are never shared, full
  stop, not a per-group setting.
- No member-count or group-count limits in this initial version.
- No changes to the existing public `{uid}/{date}` page or its own
  `ShareButton` — groups are an additive, separate visibility mechanism.
