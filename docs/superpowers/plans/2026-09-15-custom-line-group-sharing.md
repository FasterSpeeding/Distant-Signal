# Custom Line Group Sharing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a private custom line's **owner** grant one or more groups they
belong to read access to that line, so fellow group members can see its
definition, live status and disruption history — on the group's page, on
`/lines/{id}`, and on the home page — without the line becoming public,
without weakening the single-owner privacy boundary for anyone outside
those groups, and without giving away any of the owner's edit/delete
control.

**Architecture:** One new join table, `custom_line_group_grants`
(`group_id`, `line_id`, `granted_by`, `granted_at`), mirroring
`group_trains` but with a real `REFERENCES custom_lines(id) ON DELETE
CASCADE` that `group_trains`'s catalogue-shaped sibling could never have.
One new data-layer helper, `custom_lines::readable_custom_line_ids`,
replaces `custom_lines::owners_for_ids` at **every** custom-line read gate
(`get_line`, `get_line_definition`, `filter_private_custom_rows`,
`get_line_status_history`), turning each existing ownership check into a
two-part `owned by caller OR granted into a group the caller is currently
in`. Grant/revoke/list functions live in `crates/api/src/data/groups.rs`
next to their `group_trains` counterparts; three new routes hang off
`crates/api/src/routes/groups.rs`'s existing router, plus one
caller-scoped `GET /groups/shared-custom-lines` mirroring the existing
`GET /groups/shared-trains`. The frontend gains a "Shared custom lines"
section on `/groups/{id}`, a "Lines shared with you" section on the home
page, a read-only "Shared with" indicator on the owner's own edit page,
and — critically — an explicit `isOwner` flag on `GET /public/lines/{id}`
so the pre-existing "a `200` here proves ownership" assumption that gates
the Edit/Delete controls does not silently become false.

**Tech Stack:** Rust (axum, sqlx, Postgres) for the API; Next.js App
Router (Server Component pages, Mantine `'use client'` modals),
TypeScript, Vitest for the frontend.

**Spec:** `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`.
Supporting reading (all three were read in full before this plan was
written): `docs/superpowers/specs/2026-09-12-group-lines-design.md` (the
sibling this completes — note its own "not yet" recommendation applies to
*catalogue/TfL* line sharing, **not** to this document),
`docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md`
(the privacy boundary being extended), and
`docs/superpowers/specs/2026-09-11-shared-groups-design.md` (the parent
feature being mirrored).

---

## Resolved open questions

The design document answered most of its own questions; this section
records the answer for each, **plus** the four points the design left
under-specified or self-contradictory, which this plan resolves before any
code is written. Each of those four is marked **[NEW]**.

### R1. A separate `custom_line_group_grants` table, not a widened `group_lines`
Per design §2.1, adopted as written. Reinforced by a fact the design could
only anticipate: **`group_lines` does not exist in this repository at
all** (the sibling spec is unbuilt, and its own recommendation is "not
yet"). There is therefore nothing to widen, nothing to add a `kind`
discriminator to, and no `is_custom_line_id` to relax. This plan creates
one table whose every row, by construction of the schema, requires a live
membership check on read — a structural fact rather than a discipline
someone must remember.

### R2. Schema
Exactly design §2.2: composite PK `(group_id, line_id)`; `group_id` and
`line_id` both `ON DELETE CASCADE` (the latter is the entire answer to
"what happens when the owner deletes the line" — no application code);
`granted_by REFERENCES users(id)` with **no** cascade, matching
`group_trains.added_by`/`groups.created_by`'s attribution-FK precedent; no
`revoked_at` (removal is a real `DELETE`, like `group_trains`, unlike
`group_invite_links`); one index on `line_id`.

### R3. Who may GRANT: the line's own owner, who must also be a current member
Design §2.3, adopted. A group `admin`/`owner` **cannot** force a member's
private custom line into the group. `404 "custom line not found"` for
"doesn't exist" and "exists but isn't yours" alike — never `400`, never
`403` — reusing `get_line`'s exact message so an outsider cannot
distinguish the two cases.

### R4. Who may REVOKE: the granter, or any group `admin`/`owner`
Design §2.4, adopted — a group-permission question, answered identically
to `remove_train_from_group`. `false` from the data layer → `404`.

### R5. **[NEW]** A departed granter can still revoke — so the DELETE route must not `require_member`
Design §2.7 explicitly relies on this: *"the owner … can revoke the grant
themselves … note this still works correctly even after they leave the
group, since `remove_custom_line_grant`'s sharer-branch checks `granted_by
= $3` against the caller's id, not against current group membership"*. But
design §3.3 also says `require_member` gates all three new routes — and
`require_member` returns `404` for a non-member, making the sentence above
unreachable. **Resolved: `DELETE /groups/{id}/lines/custom/{lineId}` does
NOT call `require_member`.** It resolves the caller's role with
`groups::get_member_role` (an `Option`), passes
`role.is_some_and(GroupRole::can_manage)` as `caller_can_manage`, and lets
the data layer's own `granted_by = $3` branch be the gate for everyone
else. This leaks nothing: a non-member who never granted anything deletes
zero rows and gets the same `404 "no shared custom line with that id"` a
member targeting an unknown grant gets, and the same a total stranger
guessing a group id gets. The other two new routes keep `require_member`
exactly as the design specifies.

### R6. Removing a grant revokes access immediately and completely
Design §2.5, adopted. Every read path re-queries `custom_line_group_grants`
on every request; there is no token and nothing else to revoke. Task 2 and
Task 4 each prove this with a test that reads, revokes, and reads again.

One qualification on §2.5's absolute "no cached copy anywhere" phrasing,
found during review and recorded rather than changed:
`frontend/lib/liveDataCache.ts`'s `withStaleFallback` can serve a
**previously-fetched** `/Line/Mode/.../Status` payload — which may still
contain the line — for up to `STALE_DATA_TTL_MS` (10 minutes) **after**
revocation, but only to the *same* session (the cache key is
`sha256(session cookie) + logical key`) and only while the backend is
unreachable; any successful fetch replaces it, and 401/403/404 are never
stale-served. That is a pre-existing, general outage mechanism, not
something this feature introduces, and it cannot cross users. Left as is.

### R7. The group sees the LIVE line, never a snapshot
Design §2.6, adopted. The grant row carries nothing about the line's
content, so there is nothing that *could* go stale. An owner's rename or
station change is visible to every granted group on their next request.

### R8. The grant SURVIVES its granter leaving the group
Design §2.7, adopted — the deliberate divergence from `group_trains`'s
departed-member cleanup. `remove_member` is **not** extended to touch
`custom_line_group_grants`, and Task 3 adds a test named for exactly that,
whose doc comment states the reasoning rather than only asserting the
outcome. The owner retains three independent revocation routes at all
times (revoke the grant themselves per R5, have an `admin` revoke it, or
delete the line and let the FK cascade).

### R9. A granted member sees FULL detail, and can do nothing else
Design §3.2/§3.5, adopted. Full definition, status and history — a custom
line has no per-viewer private overlay analogous to a tracked train's
tickets/`notificationsEnabled`, so there is no narrower slice to serve.
What a granted member explicitly may **not** do:
- edit or delete the line (`update_custom_line`/`delete_custom_line` stay
  `WHERE id = $1 AND user_id = $2`, untouched, grant-blind);
- re-share it into one of *their* groups (R3's ownership check refuses);
- see it in `list_lines` / `GET /public/lines` (design §3.2's deliberate
  "not changed" — that list is "mine to edit", not "visible to me"), which
  is also what keeps it out of the add-picker in Task 8.

### R10. **[NEW]** `GET /public/lines/{id}` gains an explicit `isOwner` flag
The 2026-08-31 hardening deliberately **removed** `CustomLineDetail`'s
`isOwner` field, on the stated grounds that "a `200` from this endpoint is
by construction the real owner's own line." This feature makes that
sentence false, and three places depend on it:
`frontend/app/lines/[id]/page.tsx` (whose comment says in so many words
that "`isCustom` is now the whole gate" for rendering Edit/Delete),
`frontend/app/lines/[id]/edit/page.tsx`, and `CustomLineForm`. Left alone,
a granted group member would be shown Edit and Delete buttons for someone
else's line — controls whose only possible outcome is a `404`, on a
privacy-sensitive resource, which reads to the user as "this is mine."
**Resolved: re-introduce `isOwner: bool` on `CustomLineDetail`**, computed
as `owner == caller`, and gate every mutation affordance on it. The
backend remains the authority (`update_line`/`delete_line` are unchanged);
this is the defence-in-depth half, matching `/groups/{id}`'s own posture of
never rendering a control whose only outcome is a 403/404.

### R11. **[NEW]** `sharedWithGroups` is owner-only, and lists *every* group the line is granted into
Design §3.5 proposes `groups_shared_with_line(pool, line_id, owner_id)`
"scoped to the owner's own groups only — a fellow group member should
never learn *which other* groups the owner has also shared this line
into." The privacy goal is right; the mechanism the design reaches for is
not. **Resolved:** the field is populated **only when the caller is the
line's owner** (`isOwner == true`), and is `[]` for everyone else — that
alone fully satisfies the stated goal, since a fellow member never
receives the field's contents at all. For the owner it lists **every**
group the line is currently granted into, *including one they have since
left*. Scoping it to the owner's current memberships would hide a live
grant from the one person whose data it is and who is entitled to revoke
it (R5), which is the actual privacy failure, not a protection.

### R12. **[NEW]** Home-page surfacing (added to scope after the design was written)
A custom line shared into a group must also reach a logged-in member on
the home page, not only on `/groups/{id}`. **Resolved:** a new
caller-scoped route `GET /groups/shared-custom-lines`, mirroring the
existing `GET /groups/shared-trains` in every respect — one row per
`(group, line)` pair, the caller's own `group_members` rows are the entire
scope (so no `require_member` and no group id in the path), lines the
caller **owns** are excluded (`cl.user_id <> $1`, exactly parallel to
`ts.user_id <> $1`), `401` for an anonymous caller. The home page renders
a new **"Lines shared with you"** section immediately after "Your Lines",
using the status reports it *already* fetches (`allReports`, which now
includes granted custom lines thanks to the read-path change), tagged with
the same `from {groupName}` grape `Badge` `/track/mine` already uses, with
no owner controls of any kind. The section is omitted entirely when empty,
matching the "Your Tracked Trains" precedent.

### R13. Non-goals, restated so they are not re-litigated mid-implementation
No change to `update_custom_line`/`delete_custom_line`. No partial grants.
No ownership transfer. No bulk "share all my lines" toggle. No
notifications on grant/revoke. No caps. No change to `crates/aggregator`.
No `group_lines` / catalogue-line sharing of any kind. Additionally, two
pre-existing behaviours this plan deliberately leaves exactly as it found
them, flagged here so a reviewer does not mistake them for regressions:
1. **`GET /Line/{id}/Stats/...`** (daily/half-hourly/coverage rollups) has
   never been ownership-gated for custom lines. That is a pre-existing gap
   outside the design's named six-path surface; this plan neither widens
   nor fixes it.
2. **`pinned_lines` accepts any free-form id**, so a granted member could
   pin a shared custom line. If the grant is later revoked, the pin
   dangles and the line simply stops appearing for them (the read paths
   filter it) — the same benign behaviour a pin of a deleted line already
   has today.

---

## Global Constraints

- **Additive only, never subtractive.** Every read-path change in Task 4
  turns `X` into `X OR Y`. No existing check is removed or loosened. If a
  diff hunk deletes an ownership condition without adding a strictly wider
  one in the same expression, it is wrong.
- **`readable_custom_line_ids` returns a `HashSet<String>`, not a map of
  owners.** Callers at these gates only ever need "may this caller read
  this id", never "who owns it" — the function's shape must not leak owner
  ids (design §3.2).
- **404 never 403 for custom-line ownership**, everywhere, on every new
  path (`"custom line not found"` for `get_line`-shaped checks,
  `"line not found"` for `get_line_definition`). The `403` convention
  documented in `routes/groups.rs`'s module doc still applies to
  *group-role* failures within a group the caller is already known to be a
  member of, and only there.
- **Anonymous callers are unaffected.** `filter_private_custom_rows` and
  `get_line_status_history` keep `OptionalAuthenticatedUser`; an anonymous
  caller has no `user_id` to bind and so matches no grant, and every
  custom-line row is dropped for them exactly as today. `get_line` keeps
  `AuthenticatedUser` (`401`, unconditionally) — there is no anonymous
  access to a shared custom line, ever.
- **No new N+1.** `readable_custom_line_ids` takes a slice of ids and
  issues exactly one query, because `get_mode_status` calls it with every
  custom row on the instance.
- **Backend test convention:** live-database tests are `#[tokio::test]` +
  `#[ignore]`, with the standard incantation in the ignore message, run as
  `DATABASE_URL=… cargo test -p api -- --ignored --test-threads=1`.
  Fixture user ids use a reserved `TEST-…` prefix and are cleaned up in
  reverse FK order (group first, then users — `groups.created_by` has no
  cascade).
- **Frontend test convention:** Vitest, colocated `*.test.ts(x)`,
  `renderWithMantine` from `frontend/test/render.tsx` for components.
- **Keep the home-page diff surgical.** A concurrent agent is adding
  group-shared *tracked trains* to `frontend/app/page.tsx`. Confine this
  feature's change there to: one added import, one added `Promise.all`
  entry, one derived value, and one new `<Stack>` section placed directly
  after "Your Lines". Do not restructure, re-order, or reformat anything
  else in that file.
- **Wire-shape pinning.** Every new serialized struct gets a key-set test
  in the style of `group_train_wire_shape_tests`, because these shapes
  carry data across a privacy boundary and a future field added by
  accident is exactly the failure mode that matters here.

---

## File Structure

```
crates/api/migrations/20260915090000_custom_line_group_grants.sql   NEW   (Task 1)

crates/api/src/data/custom_lines.rs
  + readable_custom_line_ids()
  + db_tests: 4 new #[ignore]'d tests                                     (Task 2)

crates/api/src/data/groups.rs
  + grant_custom_line(), remove_custom_line_grant(),
    list_group_custom_lines(), list_shared_custom_lines_for_user(),
    groups_shared_with_line()
  + GroupCustomLine / SharedCustomLine / LineGroupRef structs
  + db_tests: 7 new #[ignore]'d tests
  + wire-shape tests                                                      (Task 3)

crates/api/src/routes/lines.rs
  + get_line / get_line_definition read-path change
  + CustomLineDetail gains isOwner + sharedWithGroups                     (Task 4, 5)

crates/api/src/routes/line_status.rs
  + filter_private_custom_rows / get_line_status_history read-path change (Task 4)

crates/api/src/routes/groups.rs
  + 3 grant routes + GET /groups/shared-custom-lines
  + route-level db_tests + literal-route precedence test                  (Task 6)

frontend/lib/types.ts        + GroupCustomLine, SharedGroupCustomLine,
                               CustomLineDetail.isOwner/.sharedWithGroups (Task 7)
frontend/lib/api.ts          + getGroupCustomLines, getSharedGroupCustomLines (Task 7)
frontend/lib/api.test.ts     + coverage for both                          (Task 7)
frontend/lib/sharedCustomLines.ts       NEW  (merge helper)               (Task 7)
frontend/lib/sharedCustomLines.test.ts  NEW                               (Task 7)

frontend/components/AddCustomLineToGroupButton.tsx        NEW             (Task 8)
frontend/components/RemoveCustomLineGrantButton.tsx       NEW             (Task 8)
frontend/components/AddCustomLineToGroupButton.test.tsx   NEW             (Task 8)
frontend/app/groups/[id]/page.tsx        MODIFIED (4th section)           (Task 8)

frontend/app/lines/[id]/page.tsx         MODIFIED (isOwner gate)          (Task 9)
frontend/app/lines/[id]/page.test.tsx    MODIFIED                         (Task 9)
frontend/app/lines/[id]/edit/page.tsx    MODIFIED (isOwner 404 + Shared with) (Task 9)

frontend/app/page.tsx                    MODIFIED (Lines shared with you) (Task 10)
```

---

## Task 1: Migration — `custom_line_group_grants`

**Files:** `crates/api/migrations/20260915090000_custom_line_group_grants.sql` (new)

- [ ] **Step 1: Write the table exactly as design §2.2 specifies.**
  ```sql
  CREATE TABLE custom_line_group_grants (
      group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
      line_id     TEXT NOT NULL REFERENCES custom_lines(id) ON DELETE CASCADE,
      granted_by  TEXT NOT NULL REFERENCES users(id),
      granted_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
      PRIMARY KEY (group_id, line_id)
  );
  CREATE INDEX custom_line_group_grants_line_id ON custom_line_group_grants (line_id);
  ```
- [ ] **Step 2: Header comment** in the house style of
  `20260911090000_shared_groups.sql`: what the table is for, why `line_id`
  carries a REAL FK where `pinned_lines.line_id` deliberately has none
  (a grant can only ever be created for an id that really was the caller's
  own custom line at grant time, so there is no "stale pin of a never-real
  id" tolerance requirement), and that the `line_id` cascade is the entire
  implementation of "the owner deleted the line" cleanup.
- [ ] **Step 3: Verify** `sqlx migrate` picks it up — the filename sorts
  after `20260912090000_incidents_first_seen_at_id.sql`.

## Task 2: `custom_lines::readable_custom_line_ids`

**Files:** `crates/api/src/data/custom_lines.rs`

- [ ] **Step 1:** Add the function with design §3.2's query verbatim,
  returning `HashSet<String>`. Doc comment must state: it replaces
  `owners_for_ids` at every read gate; it is one query, not one per id; it
  deliberately returns a set rather than an owner map.
- [ ] **Step 2:** Leave `owners_for_ids` in place (it is still exercised by
  its own test and is a smaller, honest primitive) but add a doc-comment
  line directing new read gates to `readable_custom_line_ids` instead.
- [ ] **Step 3:** Tests (`#[ignore]`d), named per design §6:
  `readable_custom_line_ids_includes_the_owners_own_line_with_no_grant`,
  `readable_custom_line_ids_includes_a_granted_line_for_a_fellow_member`,
  `readable_custom_line_ids_excludes_a_granted_line_for_a_non_member`
  (the non-member must be a member of a *different* group that has grants
  of its own, so the test would fail on a query that forgot to correlate
  `group_members.group_id` with the grant's), and
  `readable_custom_line_ids_excludes_a_line_after_its_grant_is_revoked`
  (R6: read, delete the grant, read again, assert gone).
- [ ] **Step 4:** Extend the existing `delete_custom_line` coverage with
  `delete_custom_line_cascades_its_group_grants` — grant into two groups,
  delete the line, assert both grant rows are gone via the FK (no
  application code involved).

## Task 3: Grant data layer in `data/groups.rs`

**Files:** `crates/api/src/data/groups.rs`

- [ ] **Step 1: `grant_custom_line(pool, group_id, line_id, user_id) -> Result<bool>`**
  — mirrors `add_train_to_group` exactly: first
  `SELECT id FROM custom_lines WHERE id = $1 AND user_id = $2`, return
  `false` if no row (the route maps that to `404`), then
  `INSERT … ON CONFLICT (group_id, line_id) DO NOTHING` with
  `granted_by = user_id`. Doc comment must say the ownership check is
  application-layer and is the same `WHERE id = $1 AND user_id = $2` shape
  used for trains and tickets, and that a group `admin` gets no special
  standing here.
- [ ] **Step 2: `remove_custom_line_grant(pool, group_id, line_id, user_id, caller_can_manage) -> Result<bool>`**
  — design §2.4's two-branch shape verbatim.
- [ ] **Step 3: `list_group_custom_lines(pool, group_id) -> Vec<GroupCustomLine>`**
  — `{ line_id, line_name, granted_by, granted_by_name }`, joining
  `custom_lines` and `users`, `ORDER BY granted_at`, collapsing the sharer
  to `users::display_label` (`users.name`, else `users.username`, else
  nothing) the same way `GroupTrain::from` does. **An email address is
  never a display-name fallback here or anywhere else in this feature** —
  not for the group page, not for the home page's shared rows, not in the
  wire shapes; `display_label` also rejects a name/username claim that is
  itself an email address. **No permission
  check here** — the route's `require_member` is the gate, stated in the
  doc comment as it is for `list_group_trains`. Live status is deliberately
  *not* joined in: the group page reads it through the existing
  `GET /Line/{ids}/Status` route (Task 8), which exercises this feature's
  own read-path change end to end rather than duplicating status rendering
  into this module.
- [ ] **Step 4: `list_shared_custom_lines_for_user(pool, user_id) -> Vec<SharedCustomLine>`**
  — `GroupCustomLine` plus `group_id`/`group_name`; scope is
  `FROM group_members me JOIN groups g … JOIN custom_line_group_grants …`
  with `WHERE me.user_id = $1 AND cl.user_id <> $1`. One row per
  `(group, line)` pair, `ORDER BY granted_at DESC, line_id DESC` for a
  total, stable order. Doc comment mirrors
  `list_shared_trains_for_user`'s: the membership join **is** the
  permission check.
- [ ] **Step 5: `groups_shared_with_line(pool, line_id) -> Vec<LineGroupRef>`**
  — `{ id, name }` for every group the line is granted into, `ORDER BY
  g.name`. Per R11 this takes **no** `user_id`: the route (Task 5) calls it
  only when the caller is the owner, and the doc comment must say so
  explicitly, along with why "every group, including ones the owner has
  left" is the right answer for the owner specifically.
- [ ] **Step 6: `remove_member` is NOT modified.** Add a comment beside its
  existing `DELETE FROM group_trains … added_by = $2` cleanup recording
  that `custom_line_group_grants` is deliberately excluded, pointing at
  design §2.7.
- [ ] **Step 7: Tests** (`#[ignore]`d): `grant_custom_line_rejects_a_line_the_caller_does_not_own`
  (assert `false` **and** zero rows in the table afterwards),
  `grant_custom_line_is_idempotent`,
  `remove_custom_line_grant_a_non_manager_can_only_remove_their_own`,
  `remove_custom_line_grant_allows_a_manager_to_remove_anyones`,
  `remove_member_does_not_touch_custom_line_group_grants_even_when_the_departing_member_is_the_grantor`
  (doc comment states §2.7's reasoning, not just the assertion),
  `list_shared_custom_lines_for_user_excludes_the_callers_own_lines_and_other_peoples_groups`,
  and extend `delete_group_cascades_members_and_trains_and_invite_links`
  with a `custom_line_group_grants` assertion (rename it accordingly).
- [ ] **Step 8: Wire-shape tests** pinning `GroupCustomLine`'s and
  `SharedCustomLine`'s exact JSON key sets.

## Task 4: The four read paths

**Files:** `crates/api/src/routes/lines.rs`, `crates/api/src/routes/line_status.rs`

- [ ] **Step 1: `filter_private_custom_rows`** — replace the
  `owners_for_ids` call and its `(user, owner)` match with
  `readable_custom_line_ids(pool, &custom_ids, &caller.id)` and a
  `readable.contains(&row.id)` check. When `user` is `None`, skip the query
  entirely and drop every custom row (identical to today, and avoids
  binding a meaningless `user_id`). Keep the "never drop on a lookup miss"
  posture for non-custom ids by only filtering rows whose id starts with
  `custom-`.
- [ ] **Step 2: `get_line_status_history`** — the `custom-` branch becomes
  a `readable_custom_line_ids` membership check; the `Ok(Json(vec![]))`
  behaviour for a non-readable id is unchanged.
- [ ] **Step 3: `get_line`** — replace
  `owner.as_deref() != Some(user.id.as_str())` with a readability check.
  Keep the existing `get_custom_line` call (its `CustomLine` payload is
  still needed, and its `owner` is still needed for `isOwner` in Task 5),
  and add the grant check only on the `!owns` branch:
  ```rust
  let is_owner = owner.as_deref() == Some(user.id.as_str());
  if !is_owner {
      let readable = custom_lines::readable_custom_line_ids(
          &app.database, std::slice::from_ref(&id), &user.id,
      ).await.map_err(internal_error)?;
      if !readable.contains(&id) {
          return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
      }
  }
  ```
  Same 404 and same message for everyone outside both sets.
- [ ] **Step 4: `get_line_definition`** — same treatment on its
  `caller_owns_it` branch, keeping `OptionalAuthenticatedUser` (an
  anonymous caller short-circuits to the existing 404 without a query) and
  the `"line not found"` message.
- [ ] **Step 5: `list_lines` is NOT changed** — design §3.2. Add a comment
  saying so and why ("mine to edit", not "visible to me"), because this is
  the one read path a future reader will expect to have changed.
- [ ] **Step 6: Route-level tests** in `routes::line_status`'s existing
  `db_tests`: a granted-but-not-owned custom line's row is **included** in
  `GET /Line/{ids}/Status` for a group member and **excluded** for a
  confirmed non-member; and `GET /Line/{id}/Status/{from}/to/{to}` returns
  history for the member and `[]` for the non-member.

## Task 5: `CustomLineDetail` gains `isOwner` and `sharedWithGroups`

**Files:** `crates/api/src/routes/lines.rs`

- [ ] **Step 1:** Add `is_owner: bool` and
  `shared_with_groups: Vec<groups::LineGroupRef>` to `CustomLineDetail`,
  and **replace** the struct's existing "No `isOwner` field" doc comment
  with one explaining R10 — that the 2026-08-31 assumption this endpoint
  used to rest on ("a 200 proves ownership") stopped being true when
  grants landed, and this field is what the frontend's Edit/Delete gate
  now reads.
- [ ] **Step 2:** In `get_line`, call `groups_shared_with_line` **only**
  when `is_owner`; otherwise pass `vec![]`. Comment must cite R11.
- [ ] **Step 3:** Tests: `get_line`'s existing `db_tests` gain
  `get_line_returns_is_owner_false_and_no_shared_groups_for_a_granted_member`
  and `get_line_returns_is_owner_true_and_the_groups_for_the_owner`, plus a
  wire-shape test pinning `CustomLineDetail`'s key set.

## Task 6: Routes

**Files:** `crates/api/src/routes/groups.rs`

- [ ] **Step 1: Register four routes.** `/groups/{id}/lines/custom`
  (`get` + `post`), `/groups/{id}/lines/custom/{line_id}` (`delete`), and
  the literal `/groups/shared-custom-lines` (`get`). Add a comment on the
  literal one pointing at the same matchit-precedence reasoning
  `/groups/shared-trains` already carries, and a sibling unit test
  (`shared_custom_lines_literal_route_wins_over_same_position_dynamic_id_route`).
- [ ] **Step 2: `list_group_custom_lines_route`** — `require_member`, then
  the data call.
- [ ] **Step 3: `add_custom_line_grant`** — `require_member`, then
  `grant_custom_line`; `false` → `404 "custom line not found"` (R3's exact
  message, deliberately identical to `get_line`'s). `204` on success.
  Request body `{ lineId }`, `#[serde(rename_all = "camelCase")]`.
- [ ] **Step 4: `remove_custom_line_grant_route`** — per **R5**, no
  `require_member`; resolve `get_member_role` into an `Option<GroupRole>`,
  pass `role.is_some_and(GroupRole::can_manage)`, `false` →
  `404 "no shared custom line with that id"`. The handler's doc comment
  must spell out why this one route deliberately diverges from the file's
  `require_member` habit, and why it leaks nothing.
- [ ] **Step 5: `list_shared_custom_lines_route`** — no group id, no
  `require_member`, doc comment mirroring `list_shared_trains_route`'s.
- [ ] **Step 6: Route-level `db_tests`** appended to the existing module:
  - a plain member **cannot** grant a line they don't own (`404`, and the
    table is empty afterwards);
  - a non-member of the group cannot grant their own line into it (`404`
    from `require_member`);
  - a plain member cannot revoke another member's grant (`404`, row
    survives) but the granter can (`204`);
  - **a granter who has since left the group can still revoke** (R5) — the
    direct HTTP-level proof of design §2.7's claim;
  - `GET /groups/shared-custom-lines` returns a fellow member's shared line
    tagged with the group, excludes the caller's own lines, returns nothing
    for a stranger, and `401`s with no session.

## Task 7: Frontend types, API client, merge helper

**Files:** `frontend/lib/types.ts`, `frontend/lib/api.ts`,
`frontend/lib/api.test.ts`, `frontend/lib/sharedCustomLines.ts` (+ test)

- [ ] **Step 1:** `GroupCustomLine { lineId, lineName, grantedBy,
  grantedByName }`, `SharedGroupCustomLine extends GroupCustomLine {
  groupId, groupName }`, `LineGroupRef { id, name }`, and
  `CustomLineDetail` gains `isOwner: boolean` and
  `sharedWithGroups: LineGroupRef[]`.
- [ ] **Step 2:** `getGroupCustomLines(id)` (throws on 401, like
  `getGroupTrains`) and `getSharedGroupCustomLines()` (`null` on 401, like
  `getSharedGroupTrains`).
- [ ] **Step 3:** `mergeSharedCustomLines(rows, ownLineIds?)` in
  `lib/sharedCustomLines.ts`, a direct analogue of `mergeSharedTrains`:
  one entry per line id carrying every distinct group name it arrived
  through, de-duplicated, order preserved. Unit-tested for the
  two-groups-one-line case, the de-dup case, and the own-line exclusion.

## Task 8: Group page — "Shared custom lines"

**Files:** `frontend/components/AddCustomLineToGroupButton.tsx` (new, +
test), `frontend/components/RemoveCustomLineGrantButton.tsx` (new),
`frontend/app/groups/[id]/page.tsx`

- [ ] **Step 1: `AddCustomLineToGroupButton`** — structural mirror of
  `AddTrainToGroupButton`: modal, lazy `fetch('/api/lines')` on open,
  **filtered client-side to `source === 'custom'`** (which, because
  `list_lines` is caller-scoped and deliberately unchanged, is already
  exactly "the caller's own custom lines" — belt and braces on top of the
  server-side ownership check, never the boundary itself),
  `excludeLineIds` for lines already granted here, same
  `needsLogin`/`Alert` error handling, `POST /api/groups/{id}/lines/custom`
  with `{ lineId }`.
- [ ] **Step 2: `RemoveCustomLineGrantButton`** — verbatim mirror of
  `RemoveGroupTrainButton` against
  `DELETE /api/groups/{id}/lines/custom/{lineId}`, with "Stop sharing"
  wording (removing a grant does not touch the line itself, and "Remove"
  would read as if it might).
- [ ] **Step 3: Page section.** A fourth section, "Shared custom lines",
  after "Shared trains", with its own `<Divider />`. Fetch
  `getGroupCustomLines(id)` in the existing `Promise.all`; then, only when
  that list is non-empty, fetch statuses via the existing
  `getLineStatus(ids, false)` and tolerate `ApiNotFoundError` → `[]` (that
  route 404s when it matches nothing). Each row: line name, a
  `StatusBadge` from `worstStatus` when a report exists, "Shared by
  {grantedByName ?? 'a member'}", a `<Link href={'/lines/'+lineId}>`, and
  the remove button gated on `canManage || grantedBy === currentUserId` —
  mirroring `SharedTrainRow`'s own `canRemove` exactly. Empty state: "No
  custom lines have been shared into this group yet."
- [ ] **Step 4: Test** the picker filters to `source === 'custom'` and
  excludes already-granted ids.

## Task 9: `/lines/[id]` and its edit page

**Files:** `frontend/app/lines/[id]/page.tsx` (+ test),
`frontend/app/lines/[id]/edit/page.tsx`

- [ ] **Step 1:** `/lines/[id]/page.tsx` — keep `isCustom` for the
  catalogue-vs-custom distinction, but gate the Edit/Delete pair on
  `isCustom && customLine.isOwner`. **Replace** the existing comment block
  that asserts "`isCustom` is now the whole gate" — leaving it would be
  actively misleading. Add a small dimmed line for a non-owner viewer
  ("Shared with you by a group") so the page explains why they can see a
  line they don't own.
- [ ] **Step 2:** `/lines/[id]/edit/page.tsx` — `notFound()` when
  `!line.isOwner`, before rendering the form. Nothing else changes there.
- [ ] **Step 2b:** Render the read-only "Shared with" list from
  `line.sharedWithGroups` (names linking to `/groups/{id}`) on
  **`/lines/[id]` itself**, not on the edit page. Design §3.5 put it on the
  edit page; this is a deliberate, narrow divergence: `/lines/[id]` is the
  surface an owner actually lands on (it is where Edit and Delete already
  live and what every bookmark and share link points at), it already has
  `CustomLineDetail` in hand so the indicator costs no extra fetch, and the
  edit page is a focused form whose job is the line's own fields. The
  substance of §3.5 is unchanged and is what matters: a plain, read-only
  indicator, owner-only, with **no** add/remove control anywhere on the
  line's own pages — granting and revoking both still happen from the
  group's page, where the group-permission context lives.
- [ ] **Step 3:** Extend `page.test.tsx` with a granted-non-owner case
  asserting no Edit/Delete control renders.

## Task 10: Home page — "Lines shared with you"

**Files:** `frontend/app/page.tsx`

- [ ] **Step 1:** Add `getSharedGroupCustomLines().catch(() => null)` as a
  fourth entry in the existing `Promise.all`, matching
  `getMyTrackedTrains()`'s fail-closed posture.
- [ ] **Step 2:** In the authenticated branch only, derive
  `sharedLines = mergeSharedCustomLines(sharedCustomLines ?? [])` and pair
  each with its report from the already-fetched `allReports` (no new status
  fetch — the read-path change is what puts granted custom lines in there).
- [ ] **Step 3:** Render, only when non-empty, a `<Stack>` titled "Lines
  shared with you" directly after the "Your Lines" section: per line, a
  `Card` linking to `/lines/{lineId}`, the line name, a `StatusBadge` when
  a report exists, one grape `from {groupName}` `Badge` per group, and
  "Shared by {grantedByName ?? 'a member'}". No pin toggle, no edit, no
  remove — view-only.
- [ ] **Step 4:** Keep the diff to the four touch points listed in Global
  Constraints so the expected textual conflict with the concurrent
  tracked-trains change stays trivial.

## Task 11: Verification

- [ ] `cargo fmt --all && cargo clippy -p api --all-targets -- -D warnings`
- [ ] `cargo test -p api` (non-ignored: unit + wire-shape tests)
- [ ] `cd frontend && npx tsc --noEmit && npm test && npx next lint`
      (whichever lint entry points exist)
- [ ] Re-read the diff against R1–R13 and confirm no read path lost a
      check, no mutation path gained a grant-awareness, and no new struct
      serializes a field it shouldn't.
