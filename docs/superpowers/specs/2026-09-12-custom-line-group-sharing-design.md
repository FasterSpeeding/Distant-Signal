# Design: Sharing a Private Custom Line into a Group

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-12-group-lines-design.md` (the sibling
spec this one completes) and
`docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md`
(the privacy boundary this one extends). No implementation, no code or
migration changes — this is a design document only.

## 0. What this is, and why it exists

`2026-09-12-group-lines-design.md` designed `group_lines` — sharing a
**catalogue or TfL** line into a shared group — and explicitly excluded
custom lines, concluding that the catalogue/TfL version has only modest
value ("saves one paste," since `/lines/{id}` is already public with its
own `ShareButton`) while naming custom-line sharing as *"the one version
of this feature with genuine, non-duplicative value"* — a household's own
hand-built "my commute" line is genuinely private today, with no public
URL a member could just paste into chat instead. That spec then stopped,
naming a `custom_line_group_grants`-style table as *"the natural extension
point, not built"* and listing three unanswered questions almost verbatim
to the ones this document is asked to resolve.

This document designs exactly that extension: letting a custom line's
owner share it into one or more groups they belong to, so fellow group
members can see it without the line becoming public, without weakening
`2026-08-31-private-custom-lines-and-tracked-trains-design.md`'s ownership
boundary for anyone outside the group, and without giving the owner up
any of their existing edit/delete control.

## 1. Current relevant state (verified 2026-09-12)

- **Custom lines are strictly single-owner-private**, end to end, per the
  2026-08-31 design (now shipped, confirmed by direct inspection):
  - `custom_lines.user_id` is `NOT NULL` (migration
    `20260901120000_custom_lines_owner_not_null.sql`) — every custom line
    has exactly one real owner, permanently, with no transfer mechanism of
    any kind. Confirmed: `crates/api/src/data/custom_lines.rs` has no
    `transfer_ownership`/`reassign_owner` function, and no route in
    `crates/api/src/routes/lines.rs` accepts a new `user_id` for an
    existing line. A custom line's ownership is fixed at creation and can
    only ever be removed by deleting the line outright.
  - `get_line` (`GET /public/lines/{id}`, `crates/api/src/routes/lines.rs`)
    requires `AuthenticatedUser` and 404s for "doesn't exist," "exists but
    owned by someone else," treating both identically — reused message
    `"custom line not found"`. Never `403`.
  - `get_line_status`/`get_mode_status` (`GET /Line/{ids}/Status`,
    `GET /Line/Mode/{modes}/Status`, `crates/api/src/routes/line_status.rs`)
    call `filter_private_custom_rows`, which drops any `custom-`-prefixed
    row the caller doesn't own (via `custom_lines::owners_for_ids`), silent
    filtering rather than a hard error.
  - `get_line_status_history` (`GET /Line/{id}/Status/{from}/to/{to}`)
    returns an empty `history` array for a custom line the caller doesn't
    own, identical to "unknown id" — this route has never distinguished
    the two.
  - `list_lines`/`get_line_definition` are similarly ownership-scoped.
  - **All six of these paths are gated purely on `user_id == caller.id`.**
    There is no existing concept anywhere in this codebase of "a custom
    line readable by more than one specific user."
- **`custom_lines::delete_custom_line`** (verified,
  `crates/api/src/data/custom_lines.rs`) deletes the `custom_lines` row and,
  in the same transaction, any `pinned_lines` row referencing that id —
  handling exactly the "referenced by a free-form id, source deleted" case
  for `pinned_lines`, because `pinned_lines.line_id` carries **no FK** to
  `custom_lines` by design (ids are free-form, client-supplied strings,
  "never validated against any line catalogue," per that file's own doc
  comment). This is the direct precedent open question 2 asks about: a
  join-table row referencing a custom line by its free-form id, with no FK
  to enforce cleanup, needs an *explicit* delete alongside
  `delete_custom_line`'s own `DELETE FROM custom_lines`, exactly the way
  `pinned_lines` already gets one.
- **`group_trains`'s departed-member cleanup** (verified,
  `crates/api/src/data/groups.rs::remove_member`): when a member leaves (or
  is removed), `DELETE FROM group_trains WHERE group_id = $1 AND added_by
  = $2` runs in the *same transaction* as the `group_members` deletion —
  a leaving member's shared trains are deleted, never left "shared by
  someone no longer in the group." `remove_member` also runs a completely
  separate piece of logic for the specific case where the departing member
  is the group's **owner**: ownership transfers to the longest-standing
  remaining `admin`, or failing that, the longest-standing remaining
  `member` (or the whole group is deleted if no one else remains). That
  ownership-transfer logic exists *because a group itself always has
  exactly one owner and a defined, precedented successor rule*.
- **A custom line has no such successor rule.** This is the structural
  asymmetry open question 4 turns on: `remove_member`'s owner-departure
  branch works because `groups` was designed from day one with the
  question "what happens when the owner leaves" already answered
  (auto-promote, or delete if empty). `custom_lines` was designed — twice,
  now, once in the original ownership retrofit and again in the
  2026-08-31 privacy hardening — with the *opposite* answer: ownership is
  fixed, permanent, and has no transfer path at all, not even an
  operator-run one outside the one-time `legacy-unclaimed` migration
  runbook for pre-existing orphans. "The owner leaves the group" and "the
  train's sharer leaves the group" are therefore not analogous events:
  leaving a group has zero effect on who owns a `train_subscriptions` row
  (the owner keeps their own train regardless of group membership — only
  the *group's visibility* into it is affected), but leaving a group
  **cannot** revoke or reassign who owns a `custom_lines` row either — the
  owner's departure from the group changes nothing about the line's
  ownership, only (per this design, §3.4) whether the group can still see
  it.
- **`GroupRole`** (`crates/api/src/data/groups.rs`) has three tiers
  (`Owner`/`Admin`/`Member`) with `can_manage()` (`Owner | Admin`) and
  `is_owner()` predicates. This design reuses both verbatim, exactly as
  `group_lines` did.
- **This app's 404-never-403 convention** for ownership (`custom_lines`,
  `train_tracking::tracked_train_owner`) coexists with a **different,
  documented 403 convention specific to `routes/groups.rs`** (its own
  module doc): a resolved member who lacks permission for a group action
  gets `403`, because they already know the group and their own
  membership exist — hiding that via 404 would be "actively confusing,
  not protective." Both conventions are in play here simultaneously and
  this design has to keep them straight per surface (see §5).
- **`group_lines`** (from the sibling spec, not yet built) explicitly
  rejects any `custom-`-prefixed `line_id` with `400` at the API layer.
  This design does **not** touch that decision or that table — see §3.1.

## 2. Decisions

### 2.1 A separate table, `custom_line_group_grants` — not a widened `group_lines`

**Decided: yes, a genuinely separate table, engaging with (not just
inheriting) the sibling spec's own reasoning for why.**

The sibling spec's stated reason for keeping `group_lines` catalogue/TfL-only
was: *"so a catalogue line's 'anyone can share it, it's already public'
rule is never accidentally applied to a resource that isn't."* Taken at
face value, that's an argument about the **write path** (who may add an
entry), not directly about table shape — one *could* imagine a single
`group_lines` table with a `kind` discriminator and per-kind validation
in the route handler, achieving the same access-control outcome without a
second table.

That alternative is rejected here for three reasons specific to this
resource, not just deference to the prior spec's phrasing:

1. **The read paths are structurally different, not just gated
   differently.** A catalogue/TfL row in `group_lines` needs zero
   ownership filtering on read (§3.6 of the sibling spec: "no private data
   of any kind"). A custom-line grant needs the *opposite* default —
   invisible to everyone except the owner and (once granted) fellow group
   members — which means every read path touching it needs an explicit
   membership check it would otherwise never need. Folding both into one
   table means every future reader of `group_lines` has to remember "oh,
   except if `kind = 'custom'`, in which case check membership too" —
   exactly the kind of accidentally-widened-access bug a `custom-` prefix
   check already exists elsewhere in this codebase to guard against
   (`filter_private_custom_rows`, `is_custom_line_id` in the sibling
   spec). A separate table makes "this table's rows require a live
   membership check on every read" a structural fact of the schema, not a
   discipline someone has to remember to apply consistently across every
   query written against a shared table, forever.
2. **The write-side identity check is different in kind.** Adding to
   `group_lines` checks "is `line_id` a known catalogue/TfL id" (a lookup
   against static config or a summaries query, no ownership concept).
   Adding to `custom_line_group_grants` checks "does the caller *own*
   `line_id`" (an ownership check identical in shape to
   `add_train_to_group`'s `WHERE id = $1 AND user_id = $2`). These are
   different queries against different data with a different failure
   mode (`400` "not a known line" vs. `404` "no custom line with that id
   you own," never `403` — see §3.4). A shared table doesn't remove the
   need for two different validation functions; it just makes it easier to
   call the wrong one against the wrong row by mistake.
3. **A real FK is possible here, and wasn't for `group_lines`.** The
   sibling spec's `group_lines.line_id` has no FK because catalogue lines
   are TOML files and TfL lines have no identity table — there is
   *nothing* to reference. `custom_lines`, by contrast, is a real table
   with a real primary key. `custom_line_group_grants.line_id` **can** and
   should carry a genuine
   `REFERENCES custom_lines(id) ON DELETE CASCADE` (§2.2) — a strictly
   stronger integrity guarantee than `group_lines` can ever offer, and one
   that directly answers open question 2's second half (owner deletes the
   line entirely) for free, at the schema level, with no application code
   required. Merging the two tables would mean either dropping this FK (to
   accommodate `group_lines`'s FK-less catalogue/TfL rows in the same
   column) or maintaining two different validity regimes in one table —
   both worse than two tables, one clean FK each.

None of this contradicts the sibling spec's own reasoning — it's the same
reasoning, applied to a resource where it points even more strongly toward
separation, because unlike catalogue/TfL lines, custom lines have a real
row to key an FK against and a real privacy default to get wrong.

### 2.2 Schema

```sql
CREATE TABLE custom_line_group_grants (
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    line_id     TEXT NOT NULL REFERENCES custom_lines(id) ON DELETE CASCADE,
    granted_by  TEXT NOT NULL REFERENCES users(id),
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, line_id)
);

-- "Which groups is this custom line shared into" -- needed by
-- delete_custom_line's extension (§2.4) and by the owner's own
-- /lines/{id} edit page (§4) to show "shared with: Family, Commute Club".
-- Mirrors group_lines_line_id / group_trains_train_subscription_id's own
-- "index the join table's non-PK-leading FK column preemptively" precedent.
CREATE INDEX custom_line_group_grants_line_id ON custom_line_group_grants (line_id);
```

Column-by-column, against this app's established conventions:

- **`group_id ... ON DELETE CASCADE`**: identical to `group_lines.group_id`
  and `group_trains.group_id` — deleting a group cleans up every grant
  into it, no orphaned rows, no application code needed.
- **`line_id REFERENCES custom_lines(id) ON DELETE CASCADE`**: the one
  genuinely new FK shape in this design, made possible by §2.1's point 3.
  **This is also the answer to open question 2's second half**: when the
  owner deletes their custom line entirely (`delete_custom_line`), this
  FK cascades automatically — every grant into every group vanishes in
  the same statement, with zero new application code in
  `delete_custom_line` itself. Contrast this with `pinned_lines`, which
  needed (and got) an explicit, hand-written `DELETE FROM pinned_lines
  WHERE line_id = $1` inside `delete_custom_line`'s own transaction,
  specifically *because* `pinned_lines.line_id` has no FK by design (it
  has to tolerate ids that were never real lines at all — a stale pin of
  a typo'd id, for instance). `custom_line_group_grants.line_id` has no
  such tolerance requirement: a grant can only ever be created for an id
  that, at grant time, really was the caller's own real custom line
  (§2.3), so there is no "stale pin of a never-real id" case to
  accommodate, and a real FK is strictly better than mirroring
  `pinned_lines`'s hand-rolled cleanup here.
- **`granted_by REFERENCES users(id)`, no `ON DELETE CASCADE`**: identical
  shape to `group_trains.added_by`/`groups.created_by`/
  `group_invite_links.created_by` — an attribution field, not a cascade
  target, consistent with this app having no user-deletion feature at
  all yet (same latent hazard already documented for those three
  columns; not newly introduced here). Since §3.4 requires the ADDER to
  be the line's owner, `granted_by` is, at grant time, always equal to
  `custom_lines.user_id` for that row — but it's still stored
  separately, not derived via a join at read time, for the same reason
  `group_trains.added_by` is stored rather than re-derived: it's cheap,
  it's a normal FK-backed column, and a future relaxation of §3.4 (not
  currently planned, see Non-goals) would silently produce wrong
  attribution if the display path assumed `granted_by ==
  custom_lines.user_id` instead of reading the stored column.
- **Composite PK `(group_id, line_id)`**: identical shape to
  `group_trains`/`group_lines` — a custom line can be granted to more than
  one group at once (a household's "my commute" line shared with both a
  "family" group and a "commute buddies" group), so a single `group_id`
  column on `custom_lines` itself would foreclose that, exactly as
  `group_lines`'s own §3.2 reasoning already established for catalogue/TfL
  lines.
- **No `revoked_at`/soft-delete column.** A grant is either present or
  it isn't; removing a grant is a real `DELETE`, matching
  `group_trains`/`group_lines`'s own hard-delete removal semantics (as
  opposed to `group_invite_links`, which *does* soft-delete via
  `revoked_at` because a revoked link's *history* — when it was created,
  by whom — still matters for that feature's own reasoning; nothing in
  this design needs a grant's history preserved past its removal).

### 2.3 Who can ADD a grant: the custom line's own owner, only — Open question 5

**Decided: the caller must be the custom line's real owner. Not merely a
`can_manage`/`is_owner` group member acting on a line they don't own.**

Justification, stated explicitly rather than left implicit: every single
one of this app's six existing custom-line read/write paths (§1) gates on
`user_id == caller.id`, with a `404` — never `403` — for anyone else,
including a caller who is otherwise fully privileged elsewhere (an
`admin`/`owner` of some *other* resource has no special standing over a
custom line they don't own). Letting a group's `admin`/`owner` name and
share a custom line belonging to some third party — even a fellow group
member — would be the first ownership exception this resource has ever
had, and it would be a strange one: it lets someone who has never once
been checked against `custom_lines.user_id` decide that resource's
visibility, which is precisely the "ambient authority" shape
`add_train_to_group`'s own doc comment already rejects for trains
(*"Ownership is enforced at the APPLICATION layer... the exact `WHERE id
= $1 AND user_id = $2` shape"*). `group_lines`'s catalogue/TfL version can
afford "any member, any known line" specifically because those lines have
no owner to violate (its own §3.4: *"no ambient-authority concern... no
ownership concept to restrict by"*) — the exact condition that does not
hold here.

Concretely: `POST /groups/{id}/lines/custom` (§3.3) takes a `lineId` and
resolves it via the **same ownership check** `get_line` already uses —
`custom_lines::get_custom_line(pool, line_id)`, comparing the returned
`user_id` against the caller's own `AuthenticatedUser.id`. A `lineId` that
doesn't exist, or exists but belongs to someone else, is **`404`**, reusing
`get_line`'s own message convention (`"custom line not found"`) — not
`400` (this isn't "not a valid kind of thing to add," per the sibling
spec's `400` for a catalogue-rejecting `group_lines` add; it's "you don't
have a claim on this specific thing," the same shape as every other
custom-line ownership check in this app) and never `403` (an outside
caller must not be able to distinguish "doesn't exist" from "exists,
someone else's," any more than `get_line` lets them).

This is a real, deliberate restriction worth naming plainly: **a group's
`admin`/`owner` cannot force a member's private custom line into the
group's shared view.** Only the line's own owner can choose to share it.
This mirrors `add_train_to_group`'s own precedent exactly (a group
`admin` cannot share a train they don't own either) and keeps this
feature from becoming a way to leak a member's private data against
their will via a group-permission loophole.

### 2.4 Who can REMOVE a grant: sharer, or group `admin`/`owner` — a genuinely different question from §2.3

**Decided: removing a grant is a *group-permission* question, answered
identically to `group_trains`'s existing rule — the member who granted
it, or any `admin`/`owner` of the group.** This is deliberately not
"only the owner can revoke" — once a custom line's owner has chosen to
share it into a group, that group's own management structure gets the
same say over removing it from the group's shared view that it already
has over any other shared resource (an `admin` can also remove someone
else's shared train, per `remove_train_from_group`'s existing rule).
Removing a grant never touches `custom_lines` itself — the line, its
data, and its ownership are completely unaffected; only the group's
*visibility* into it changes, symmetrically with `group_trains`.

Concretely, `remove_custom_line_grant(pool, group_id, line_id, user_id,
caller_can_manage)` mirrors `remove_train_from_group`'s own two-branch
shape exactly:

```rust
pub async fn remove_custom_line_grant(
    pool: &PgPool,
    group_id: &str,
    line_id: &str,
    user_id: &str,
    caller_can_manage: bool,
) -> Result<bool> {
    let result = if caller_can_manage {
        sqlx::query(
            "DELETE FROM custom_line_group_grants WHERE group_id = $1 AND line_id = $2",
        )
    } else {
        sqlx::query(
            "DELETE FROM custom_line_group_grants \
             WHERE group_id = $1 AND line_id = $2 AND granted_by = $3",
        )
        .bind(user_id)
    }
    .bind(group_id)
    .bind(line_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

`false` (no matching row deleted — unknown grant, or a non-manager
targeting someone else's grant) maps to `404`, identical to
`remove_train_from_group`'s own convention.

### 2.5 Does removing from a group revoke access retroactively? — Open question 2 (first half)

**Decided: yes, immediately and completely, with no grace period or
lingering read access.** A grant's existence is the *entire* access
grant — there is no cached/snapshotted copy of the line's data anywhere a
former group member's session could still reach after the row is gone.
Every read path in §3.2 checks `custom_line_group_grants` fresh, on every
request; deleting the row makes the very next request from any
now-ungranted member 404, with no further action needed anywhere else.
This is a direct consequence of choosing a live, queried-every-time
access model (§2.6) rather than anything resembling a capability token or
a cached permission — there's nothing to separately revoke.

### 2.6 Does the group see the LIVE line, or a snapshot? — Open question 3

**Decided: live, always reflecting the owner's latest edit — no
snapshot, ever.** Justified explicitly, not merely asserted by analogy:

- **The precedent cited in the brief actually supports this, but for a
  narrower reason than "it's the existing pattern."** `pinned_lines`
  resolves live because it stores nothing about the line itself — only a
  free-form id — so "live" isn't a design choice there at all, it's the
  only thing the schema is capable of doing. `custom_line_group_grants`
  is the same shape (a grant row carries `group_id`/`line_id`/
  `granted_by`/`granted_at` and nothing about the line's name, stations,
  or operators), so it inherits the same structural fact: there is
  nothing to go stale, because nothing about the line's content is ever
  copied into the grant row in the first place.
- **A snapshot would actively fight this app's own established
  reasoning for custom lines elsewhere.** `update_custom_line`'s own doc
  comment already establishes that a custom line's `id` is stable across
  edits specifically because *other* references depend on it staying so
  (bookmarked URLs, `pinned_lines` rows) — the design intent for this
  resource has always been "the id is durable, the content is mutable,
  and every consumer reads current content through the durable id." A
  grant snapshotting name/stations/operators at grant time would be the
  first place in this app that deliberately freezes a custom line's
  content against its own owner's edits — a real, new behavior with no
  existing precedent to justify it, and a real cost: the owner edits
  their line expecting every consumer (themselves included) to see the
  update, and a group member silently wouldn't.
- **This also keeps the read path uniform.** §3.2's read-path change is
  "add one more clause to the existing ownership check" — `owned by
  caller, OR the id has a live grant into a group the caller belongs to."
  Both branches resolve against the *same* `custom_lines` row via the
  *same* query shape already in place. A snapshot would require an
  entirely separate, second data path (serving frozen data to group
  members while the live path keeps serving the owner) — real complexity
  this design has no reason to introduce.

**Consequence, stated plainly so it isn't missed**: if the owner renames
the line, changes its stations, or otherwise edits it, every group it's
shared into sees the update on the group members' very next request, with
no notification. This mirrors exactly how a group's own `name` change
(via `rename_group`) is immediately visible to every member with no
separate acknowledgment step — consistent with this app's existing
posture, not a new one.

### 2.7 What happens when the grant's own OWNER leaves the group? — Open question 4, the load-bearing decision

**Decided: the grant survives. It is NOT auto-revoked when the owner
leaves the group, or when anyone else leaves.** No departed-grantor
cleanup, deliberately diverging from `group_trains`'s own precedent. This
is stated as the single most consequential decision in this document,
because getting it wrong in either direction has a real, different cost:

**Why not mirror `group_trains`'s cleanup (delete the grant when its
`granted_by` leaves):** `group_trains`'s cleanup exists because a shared
train's underlying resource — the `train_subscriptions` row — is *also*
deleted the moment its owner stops tracking it, and "shared by someone no
longer in the group, showing a train they may not even still be tracking"
is actively confusing. Neither premise holds here in the analogous way:

1. **The custom line's ownership is completely unaffected by the owner
   leaving the group.** Unlike a group (which has a defined successor
   rule the instant its owner departs — `remove_member`'s own
   ownership-transfer branch), a custom line has *no* ownership event
   here at all. The owner leaving `Family Group` changes nothing about
   who owns `custom-my-commute` — they still own it, can still edit it,
   can still delete it, exactly as before. This is exactly the structural
   asymmetry named in §1: `remove_member`'s owner-departure branch exists
   to answer "who owns the GROUP now" — a question with no equivalent
   here, because leaving a group is not an event that touches
   `custom_lines.user_id` at all, ever.
2. **The remaining group members' relationship to the shared line is
   unaffected by the sharer's departure.** The line's data, its live
   status, its history — none of it depended on the (now-departed)
   sharer's continued presence in the group any more than a shared
   catalogue line does (per `group_lines`'s own §3.2 reasoning for why
   *its* departed-member cleanup was rejected: "every member... can
   already see the exact same... status," so removing it on departure
   "would delete a small, genuinely shared, ownerless convenience... for
   no privacy benefit"). The privacy-relevant boundary here was already
   crossed once, deliberately, at grant time by the owner's own choice —
   the owner leaving the group is not a privacy event; it doesn't put
   the line back behind a boundary the remaining members were never
   inside of.
3. **Auto-revoking on departure would make a household's actual use case
   fragile in exactly the wrong way.** The paradigm use case named in the
   sibling spec's §1 ("the one version of this feature with genuine value")
   is a household sharing its own commute line into its own family group.
   If the line's owner ever needs to leave and rejoin that same group
   (e.g. to accept a fresh invite link after their old membership lapsed,
   or after being accidentally removed and re-added by another admin),
   auto-revocation on leave would silently delete the grant, and nothing
   about "rejoining" would restore it — the owner would have to remember
   to re-share it, a papercut with no corresponding safety benefit, since
   (per point 1) nothing privacy-relevant changed by their leaving.

**What "orphaned from the group's perspective" concretely means, and why
it's acceptable:** once the owner leaves, the grant's `granted_by` still
names them, and `list_members` (unaffected by this design) will no longer
show them as a current member — so the group page's attribution line
("shared by Alex") will name someone no longer in the member list. This
is the exact same "the sharer no longer being a member" cosmetic
non-issue `group_lines`'s own §3.2 already accepted for catalogue/TfL
lines' `added_by` attribution, and this design accepts it here for the
same reason: `added_by`/`granted_by` were always documented as
attribution-only fields, never permission-bearing ones, in both features
alike.

**What remains true regardless:** the owner retains full control at all
times, whether still a group member or not — they can revoke the grant
themselves (§2.4, "the member who granted it" — note this still works
correctly even after they leave the group, since `remove_custom_line_grant`'s
sharer-branch checks `granted_by = $3` against the caller's id, not
against current group membership) by hitting `DELETE
/groups/{id}/lines/custom/{lineId}` as themselves, or any current
`admin`/`owner` of the group can remove it on the group's behalf at any
time regardless of who granted it, or the owner can delete the custom
line outright (§2.2's cascade removes every grant everywhere
instantly). There is no scenario in which the group can see the line
after the owner has genuinely decided — by any of these three routes —
that they should not.

**One asymmetry worth naming, not smoothing over:** because remove
authority (§2.4) is `granted_by`-scoped, not `owner`-scoped, a subtlety
falls out that's worth stating rather than leaving as an implicit
surprise: if a *different* group member added a grant for a line they
don't own — impossible under this design, since §2.3 requires the adder
to be the owner — this wouldn't arise. Because §2.3 and §2.4 compose
(only the owner can add, so `granted_by` is always the owner for every
row this design can ever create), "the sharer" and "the owner" are
always the same person here, unlike `group_trains` where they're also
always the same person for the identical reason (only the train's owner
can add it, per `add_train_to_group`). So this asymmetry is actually a
non-issue in practice — flagged here only to confirm it was checked, not
because it produces any real divergent behavior.

## 3. Read-path and API changes

### 3.1 `group_lines` is untouched

This design adds a **new**, separate authorization path
(`custom_line_group_grants`) alongside the sibling spec's `group_lines`.
It does not relax `group_lines`'s own `400`-for-`custom-`-prefix
rejection, does not add a `kind` column to `group_lines`, and does not
change `is_custom_line_id`'s behavior in any way. The two tables answer
genuinely different questions ("is this a known public line" vs. "has
this specific private line's owner granted this group access") and stay
structurally separate per §2.1.

### 3.2 `get_line`/`get_line_status`/`get_mode_status`/`get_line_status_history`: add one clause, in one place

**Decided: extend the *existing* ownership check in each handler to a
two-part OR — "owned by caller, OR granted to a group the caller
belongs to" — rather than adding a parallel code path.** Concretely, one
new data-layer helper does all the new work:

```rust
/// Every custom-line id in `ids` that `user_id` may read: either they own
/// it outright, or it's been granted (§2.3-2.7) into at least one group
/// they're currently a member of. Used everywhere `owners_for_ids` was the
/// sole gate before this feature -- see call sites in `routes::lines` and
/// `routes::line_status`. A single query (not one grant-lookup per id) via
/// a LEFT JOIN through custom_line_group_grants + group_members, so the
/// bulk routes (`get_mode_status`) don't gain an N+1.
///
/// Deliberately returns a Set, not a HashMap<String, Option<String>> like
/// `owners_for_ids` -- callers here only ever need "can this caller read
/// this id," never "who owns it," so there's no reason to leak owner ids
/// through this function's shape.
pub async fn readable_custom_line_ids(
    pool: &PgPool,
    ids: &[String],
    user_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT cl.id \
         FROM custom_lines cl \
         LEFT JOIN custom_line_group_grants g ON g.line_id = cl.id \
         LEFT JOIN group_members gm ON gm.group_id = g.group_id AND gm.user_id = $2 \
         WHERE cl.id = ANY($1) AND (cl.user_id = $2 OR gm.user_id IS NOT NULL)",
    )
    .bind(ids)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
```

Per call site:

- **`get_line` (`crates/api/src/routes/lines.rs`)**: replace the direct
  `owner.as_deref() != Some(user.id.as_str())` comparison with a call to
  `readable_custom_line_ids(pool, &[id.clone()], &user.id)` and check
  membership in the result set. Still `AuthenticatedUser` (unauthenticated
  stays `401`, unconditionally, exactly as today — a group member must
  still have their own account and session; there is no anonymous access
  to a shared custom line, ever). Still `404`, same message, for anyone
  outside both the ownership and the grant set — a non-group-member gets
  the identical response to a total stranger, by construction, since they
  simply aren't in the result set. **This is the direct answer to the
  read-path open question**: `get_line`'s response shape is completely
  unchanged for a granted reader — same `CustomLineDetail`, same fields,
  full detail, not a narrower slice. See §3.5 for why "full detail, not a
  partial view" is the deliberate answer rather than an oversight.
- **`get_line_status`/`get_mode_status` (`filter_private_custom_rows`,
  `crates/api/src/routes/line_status.rs`)**: replace the `owners_for_ids`
  call and its inline ownership comparison with
  `readable_custom_line_ids` and a membership check against the result
  set. Same silent-filtering behavior as today (drop the row, don't error
  the request) — a granted reader now simply doesn't get their row
  dropped. `OptionalAuthenticatedUser` stays `OptionalAuthenticatedUser`:
  an anonymous caller is never in any group, so `user_id` is never
  bindable and every custom-line row is dropped for them exactly as
  today, unchanged.
- **`get_line_status_history`**: identical shape — the `id.starts_with
  ("custom-")` branch's ownership check becomes a
  `readable_custom_line_ids` membership check; the "return the same empty
  array a genuinely unknown id already produces" behavior for a
  non-granted, non-owning caller is completely unchanged.
- **`get_line_definition`**: same treatment, same message
  (`"line not found"`), same `OptionalAuthenticatedUser` shape.
- **`list_lines`**: **deliberately NOT changed.** This route already
  calls `list_custom_lines_for_user`, which is explicitly "the caller's
  own lines" — this is the bulk "what can I create/edit" listing behind
  `/lines`' All Lines table and the edit-picker flows, not a "what am I
  allowed to view" listing. A line shared *into* a group the caller
  belongs to, but that they don't own, should not appear here as if it
  were the caller's own to edit — it appears instead on the group's own
  page (§4), which is the correct, and only, place a non-owning member
  encounters it. Extending `list_lines` to also include granted-not-owned
  lines would blur "mine to edit" with "visible to me via a group," a
  distinction this design is careful to preserve (see §5's non-goals: no
  edit/delete capability is EVER granted to anyone but the owner).

### 3.3 New routes, mounted in `crates/api/src/routes/groups.rs`

```rust
.route(
    "/groups/{id}/lines/custom",
    axum::routing::get(list_custom_line_grants_route).post(add_custom_line_grant),
)
.route(
    "/groups/{id}/lines/custom/{lineId}",
    axum::routing::delete(remove_custom_line_grant_route),
)
```

| Method + path | Purpose | Who |
|---|---|---|
| `GET /groups/{id}/lines/custom` | List custom lines granted into the group, each with a live status snapshot | Any current member (`require_member`) |
| `POST /groups/{id}/lines/custom` | Grant one of the caller's OWN custom lines (`{ lineId }`) | The custom line's owner, who must also be a current group member (§2.3) |
| `DELETE /groups/{id}/lines/custom/{lineId}` | Revoke a grant | The original granter, or any `admin`/`owner` (§2.4) |

A distinct `.../lines/custom` sub-path, not an overloaded
`.../lines/{lineId}` shared with `group_lines`'s own
`DELETE /groups/{id}/lines/{lineId}`: the two resources have genuinely
different ownership/permission semantics on add (§2.1's point 2), and
`custom-`-prefixed ids can never collide with catalogue/TfL ids in the
first place (guaranteed by `slugify`'s own `custom-` prefix), so there is
no ambiguity a shared path would even need to resolve — this is purely
about keeping each route's handler answering exactly one, unambiguous
question, mirroring the sibling spec's own reasoning for using `400` to
keep `group_lines` unambiguously catalogue/TfL-only. Following the
existing convention in `groups.rs`, `require_member` gates all three
(§this file's own module doc: `404` for "not a member," never `403`,
since a total non-member has no legitimate claim to know the group
exists at all), then `add_custom_line_grant` layers the ownership check
from §2.3 on top (via `custom_lines::get_custom_line`, `404` for
"doesn't exist or isn't yours" — see §2.3), and
`remove_custom_line_grant_route` layers the sharer-or-manager check from
§2.4 (reusing the exact `role.can_manage()` pattern
`remove_group_train`'s own handler already uses).

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddCustomLineGrantRequest {
    line_id: String,
}

async fn add_custom_line_grant(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
    Json(req): Json<AddCustomLineGrantRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_member(&app, &group_id, &user.id).await?;

    // Ownership check (§2.3) -- the SAME check get_line already performs,
    // never widened for this caller just because they're a group member.
    let line = custom_lines::get_custom_line(&app.database, &req.line_id)
        .await
        .map_err(internal_error("check custom line ownership"))?;
    let Some((_, owner)) = line else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };
    if owner.as_deref() != Some(user.id.as_str()) {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    groups::grant_custom_line(&app.database, &group_id, &req.line_id, &user.id)
        .await
        .map_err(internal_error("grant custom line to group"))?;
    Ok(StatusCode::NO_CONTENT)
}
```

`groups::grant_custom_line` mirrors `add_train_to_group`'s idempotent
`ON CONFLICT (group_id, line_id) DO NOTHING` shape exactly — re-granting
an already-granted line is a silent no-op, not an error.

### 3.4 Frontend: `/groups/{id}` gets a "Shared custom lines" section, separate from `group_lines`'s "Shared lines"

The sibling spec's frontend design adds a "Shared lines" section (for
catalogue/TfL lines) to `frontend/app/groups/[id]/page.tsx`, alongside the
existing "Members" and "Shared trains" sections. **This design adds a
fourth, separate section — "Shared custom lines" — rather than folding
both kinds into one list.** Reasoning: the add-affordance is genuinely
different (a `Select` sourced from `GET /api/lines` filtered to
non-custom for `group_lines`, vs. a `Select` sourced from the caller's
**own** custom lines only — `getAllLines()` filtered client-side to
`source === 'custom'` — for this feature, mirroring
`AddTrainToGroupButton`'s own "picker sourced from the caller's own
resources" precedent exactly), and a merged list would either need a
`kind` badge on every row to explain why some entries can be added by
anyone and others only by their owner, or hide that distinction and
create confusion about why "Add a line" sometimes doesn't offer a line
the viewer knows exists. Two clearly-labeled sections, each with its own
add button and its own permission story, is more honest than one merged
list papering over a real difference.

- **`AddCustomLineToGroupButton.tsx`** — mirrors `AddTrainToGroupButton.tsx`
  structurally: a modal, fetches the caller's own custom lines
  (`getAllLines()` filtered to `source === 'custom'` — no dedicated
  "my custom lines" endpoint exists or is needed, since `list_lines`
  already scopes custom entries to the caller per §3.2's "deliberately not
  changed" note), excludes lines already granted into this group
  (`excludeLineIds`), same `needsLogin`/`Alert` error handling.
- **`SharedCustomLineRow`** (colocated in `page.tsx`, alongside
  `SharedLineRow`/`SharedTrainRow`) — same embedded-summary shape as
  `group_lines`'s own `SharedLineRow` (§3.6 of the sibling spec: name,
  category, worst-severity badge, single worst active reason, "shared by
  {member}" attribution, link out to `/lines/{id}` for full detail) — no
  reason to invent a different display shape for what is, once granted,
  the same `LineStatusReport` shape as any other line. Remove gated
  identically to `SharedLineRow`'s own `canRemove` (`canManage ||
  grantedBy === currentUserId`).
- **`RemoveCustomLineGrantButton.tsx`** — mirrors
  `RemoveGroupTrainButton.tsx`/the sibling spec's `RemoveGroupLineButton.tsx`
  verbatim, swapping the endpoint path to
  `DELETE /groups/{id}/lines/custom/{lineId}`.

### 3.5 Frontend: the owner's own `/lines/{id}` edit page

**Decided: show a plain, read-only "Shared with" indicator — group
names only, no per-group management UI on this page.** A new data-layer
function, `groups_shared_with_line(pool, line_id, owner_id)` (scoped to
the owner's own groups only — a fellow group member should never learn
*which other* groups the owner has also shared this line into, since that
leaks the owner's group memberships beyond the one group the reader
shares with them), backing a new field on `CustomLineDetail`
(`sharedWithGroups: { id: string; name: string }[]`), rendered on
`/lines/[id]/edit/page.tsx` as a small, non-editable list ("Shared with:
Family Group, Commute Buddies") with each name linking to
`/groups/{id}`. **Deliberately no add/remove control here** — granting
and revoking both happen from the group's own page (§3.3/§3.4), where the
group-permission context (who else is a member, what role they hold)
actually lives; duplicating that control onto the line's edit page would
mean maintaining two UIs for the same mutation with two different
surrounding contexts, for a feature whose primary surface is unambiguously
the group page, not the line's own settings.

**Why full detail, not a narrower slice, for a non-owning group member
(the concrete answer to Open question 1):** the alternative — restricting
a granted reader to name/status only, withholding disruption history or
the full definition — was considered and rejected. A custom line's
history/status (`get_line_status_history`, `get_line_status`) is exactly
the same operational information `group_trains` already shares for a
tracked train (live status, not raw ticket data) — the equivalent "don't
leak this" list for custom lines, per this design's own read-path audit
(§3.2), has no analogue to tickets or `notifications_enabled`: a custom
line carries no private, per-viewer overlay data at all (unlike a tracked
train, whose `pin_*`/`custom_name`/`notifications_enabled` fields are
explicitly filtered out of `GroupTrain`, per the parent groups spec's
§4). Its entire content — name, stations, operators, headcode filters,
live status, disruption history — is exactly what the owner is choosing
to make visible by granting the group access in the first place; there is
no finer-grained "share the status but not the definition" concept this
app's existing UI, model, or use case calls for. **Non-goal, stated
explicitly**: a future partial-grant (e.g. "share the status but not the
edit-form fields") is not designed here and not implied by anything in
this document — if wanted later, it would need its own explicit design,
not an assumption smuggled in via this decision.

## 4. Permission model summary

| Action | Who | Precedent |
|---|---|---|
| Grant a custom line to a group | The line's own OWNER, who must also be a current member of that group | `add_train_to_group`'s ownership gate (§2.3) — NOT `group_lines`'s "any member, any known line" (no ambient authority over someone else's private resource) |
| Revoke a grant | The original granter, or any `admin`/`owner` of the group | `remove_train_from_group`'s sharer-or-manager rule, identical shape (§2.4) |
| View a group's granted custom lines | Any current member | Identical to `group_trains`/`group_lines` (§3.3) |
| Read the granted line's full detail/status/history directly (`/lines/{id}` etc.) | The owner, or any current member of a group it's granted into | New: extends the existing ownership-only gate to an ownership-OR-grant-membership gate (§3.2) |
| Edit/delete the custom line itself | The owner, ONLY — unchanged | `update_custom_line`/`delete_custom_line`'s existing `WHERE ... AND user_id = $2` — never touched by this design |

## 5. Non-goals

- **Any change to editing or deleting a custom line.** A grant conveys
  read access only. `update_custom_line`/`delete_custom_line` remain
  gated purely on `user_id = caller.id`, with zero awareness of any grant
  — a group `admin`/`owner`, or any granted member, has no more ability to
  edit or delete a line they don't own after this design than before it.
- **Partial/narrower grants** (status-only, definition-only, etc.). See
  §3.5 — full detail or nothing, matching this app's existing all-or-
  nothing custom-line access model.
- **Transferring custom-line ownership**, via group mechanisms or
  otherwise. Confirmed (§1): no such mechanism exists anywhere in this
  app today, and nothing in this design creates one. "The owner leaves
  the group" (§2.7) explicitly does NOT trigger any ownership change.
- **A group-wide "make all my custom lines visible to this group"
  bulk toggle.** Each grant is a discrete, per-line, per-group decision —
  matching `group_trains`'s own one-train-at-a-time precedent, not
  introducing a bulk concept neither existing feature has.
- **Any change to `group_lines`, `is_custom_line_id`, or the sibling
  spec's `400`-for-custom-id rejection.** The two features coexist as
  fully separate mechanisms per §2.1/§3.1.
- **Notifying the owner or the group when a grant is added/removed.**
  Matches `group_trains`/`group_lines`'s own silent-mutation precedent —
  no notification feature exists for any group mutation today.
- **Caps on grants-per-line or grants-per-group.** Consistent with this
  codebase's established "no cap" stance for every other group-adjacent
  join table.
- **Any change to `crates/aggregator`.** Line status computation remains
  completely grant-blind, exactly as it is already ownership-blind —
  this design is purely a read-time authorization concern in `crates/api`.
- **Building `custom_line_group_grants` before `group_lines` itself has
  shipped and been used.** See §6 — this document's timing conclusion is
  more cautious than the sibling spec's own.

## 6. Testing approach

Following `crates/api/src/data/groups.rs`'s and `custom_lines.rs`'s
existing `db_tests` convention exactly (colocated `#[cfg(test)] mod
db_tests`, `#[ignore]`d live-database tests with the standard
`DATABASE_URL` incantation, `seed_user`/`cleanup` fixture helpers reused
as-is):

- `readable_custom_line_ids_includes_the_owners_own_line_with_no_grant` —
  baseline: an owner with zero grants still reads their own line (this
  function must never be a strict widening that accidentally requires a
  grant even for the owner).
- `readable_custom_line_ids_includes_a_granted_line_for_a_fellow_member` —
  the core positive case: grant a line into a group, seed a second member,
  assert the second member's id appears in the result set for that line.
- `readable_custom_line_ids_excludes_a_granted_line_for_a_non_member` —
  the core negative case, and the one that most directly protects the
  privacy boundary: a line granted into Group A must not become readable
  by some unrelated user who is a member of Group B, even if Group B also
  happens to have grants of its own.
- `grant_custom_line_rejects_a_line_the_caller_does_not_own` — mirrors
  `add_line_to_group_rejects_a_custom_line_id`'s "prove the refusal really
  refused" pattern from the sibling spec: attempt a grant as a non-owner,
  assert `404`, then query `custom_line_group_grants` directly and assert
  zero rows.
- `grant_custom_line_is_idempotent` — mirrors `add_train_to_group`'s own
  `ON CONFLICT DO NOTHING` test (grant twice, assert one row).
- `remove_custom_line_grant_a_non_manager_can_only_remove_their_own` —
  mirrors `remove_train_from_group`'s sharer-vs-manager split exactly.
- `remove_member_does_not_touch_custom_line_group_grants_even_when_the_departing_member_is_the_grantor`
  — the direct, explicit proof of §2.7's central decision: seed a grant
  where `granted_by` is the departing member, remove that member from the
  group (via `groups::remove_member`), assert the grant row survives.
  Named deliberately close to the sibling spec's own
  `remove_member_does_not_touch_group_lines` test, since both exist to
  disprove the same `group_trains`-shaped intuition for a different,
  independently-justified reason (§2.7's points 1-3) — this test's own
  doc comment should say so, not just assert the outcome.
- `delete_custom_line_cascades_its_group_grants` — the direct proof of
  §2.2's FK: grant a line into two different groups, delete the line via
  `delete_custom_line`, assert both `custom_line_group_grants` rows are
  gone (via the real FK cascade, not application code) alongside the
  existing `pinned_lines` cleanup assertion this function's tests already
  make.
- `delete_group_cascades_custom_line_group_grants` — mirrors
  `delete_group_cascades_members_and_trains_and_invite_links`, extended to
  also assert `custom_line_group_grants` rows are gone.
- Route-level (`crates/api/src/routes/groups.rs`'s own `db_tests`
  convention): one test confirming `get_line`/`get_line_status` for a
  granted-but-not-owned line returns `200`/includes the row for a group
  member and still `404`s/excludes it for a confirmed non-member — the
  HTTP-level version of the two `readable_custom_line_ids` data-layer
  tests above, following `routes::lines::db_tests`'s established pattern
  of needing its own HTTP-level coverage beyond what the data layer
  proves for permission-dense paths.

## 7. Timing and recommendation

**Recommendation: wait — and more firmly than the sibling spec's own
"wait" for `group_lines`.**

The sibling spec's own "wait" rested on `group_trains` (the feature that
justified building `groups` at all) having had *zero days* of real usage
to learn whether a shared-line view was even wanted, on top of the
catalogue/TfL version's own modest, "saves one paste" value. Custom-line
sharing avoids that second problem — the value case here is real and
was already argued convincingly by the sibling spec itself (§0 above).
But it introduces a strictly larger risk on the *first* axis, for reasons
that surfaced specifically during this design, not assumed going in:

1. **This design reopens, however carefully, a privacy boundary that was
   deliberately and recently hardened, on the explicit instruction of the
   repo owner, across six separate read paths, in direct response to that
   boundary having been too loose before** (`2026-08-31-private-custom-lines-and-tracked-trains-design.md`'s
   own "Corrections" section describes finding "a materially larger
   surface than the brief named" the first time this exact area was
   touched). This design believes it has extended that boundary correctly
   — every read path change is additive (one more OR clause, §3.2), never
   a removal of an existing check — but "we believe we got the six-path
   surface right this time too" is exactly the kind of claim that
   benefits from `group_lines`'s own shipped, exercised read-path pattern
   (`filter_private_custom_rows`, `readable_custom_line_ids`'s eventual
   real-world call sites) existing and being battle-tested first, rather
   than landing both the catalogue/TfL and the custom-line extensions to
   the same six routes in the same release with no usage gap between
   them.
2. **The single most consequential decision in this document (§2.7 — the
   owner leaving a group does NOT revoke the grant) has no precedent to
   check itself against.** `group_lines`'s equivalent decision (also "no
   departed-member cleanup") had the benefit of a resource with genuinely
   zero privacy stakes either way (§2.1 of that spec: "no privacy benefit"
   from cleanup, because nothing was private to begin with). This design's
   §2.7 reaches the same "don't auto-revoke" conclusion by a real, argued
   chain of reasoning (§2.7 points 1-3) — but it's reasoning about a
   resource that *was* private before the grant, decided without any
   observed instance of "a household actually uses this feature this
   way" to confirm the chain holds up under real use, e.g. that no group
   owner is ever surprised to discover a former member's line still
   sitting in their group's shared view.
3. **Unlike `group_lines`, there is no way to "just try it and see" at
   low cost.** A wrong call on `group_lines`'s scope is fully reversible
   with zero data-loss risk — worst case, ship it, learn no one uses it,
   remove the feature, `group_lines` rows vanish with the table. A wrong
   call here that under-restricts read access, even briefly, discloses
   real private content (a household's commute patterns, a hand-built
   line's operators/stations) to people the owner never intended — not
   reversible after the fact in the way an unused convenience feature is.

None of this is a finding that the design itself is unsound — §2-§5 above
resolve every open question the sibling spec left, with a stated
justification for each, and the read-path/schema mechanics are
straightforward, low-risk extensions of patterns already proven
elsewhere in this codebase. The recommendation to wait is about
*sequencing*, not soundness: land and observe `group_lines` first (it's
the lower-stakes, already-designed sibling, and its own read-path
plumbing — `is_custom_line_id`, the `400` boundary — is a direct
dependency-free precursor this design deliberately did not touch, §3.1),
let real usage surface whether households actually want this kind of
sharing at all, and then build this design — which is now fully
specified and ready to execute against with no further design work
needed — once that signal exists, rather than shipping the harder,
higher-stakes half of "share a line into a group" in the same push as the
easier, lower-stakes half with no gap to learn from either.

## 8. Self-review notes

- **Every open question named in the brief has a stated decision, not a
  restatement**: §2.3 (Q5, adder must be owner), §2.5/§2.7 (Q2, both
  halves — retroactive revocation on group-removal is immediate per
  §2.5; owner-deletes-the-line cleanup is a real FK cascade per §2.2),
  §2.6 (Q3, live not snapshot), §2.7 (Q4, survives the owner leaving,
  with the asymmetry against `group_trains` explained), §3.2 (Q1, full
  detail via one extended OR-clause, not a narrower slice, justified in
  §3.5).
- **Placeholder scan**: no `TODO`/`FIXME`/bracketed-placeholder text
  remains anywhere in this document.
- **Internal consistency check**: §2.3 and §2.4 together establish that
  `granted_by` is always the line's owner for every row this design can
  create (no code path grants on behalf of a non-owner) — §2.7 relies on
  and explicitly confirms this composition rather than assuming it
  silently.
- **Scope check**: no changes proposed to `crates/aggregator`,
  `group_lines`, `custom_lines`' write paths, or the six-path privacy
  boundary's existing owner-only behavior for anyone outside a grant —
  confirmed against §5's Non-goals list.
