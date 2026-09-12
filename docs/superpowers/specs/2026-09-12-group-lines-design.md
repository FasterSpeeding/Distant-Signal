# Design: Shared Group Lines (Extending Shared Groups to Watch a Line)

**Status: design proposal, not approved. Recommendation: not yet — see
Corrections below for the full reasoning.** Written to the same rigor as
`docs/superpowers/specs/2026-09-11-shared-groups-design.md`. No
implementation, no code or migration changes — this is a design document
only.

## 0. The idea, as briefed

The shared-groups feature (`groups`/`group_members`/`group_trains`/
`group_invite_links`, shipped 2026-09-11) currently lets a group share
individual tracked trains: a `train_subscription` a member owns can be
added to a group so every member sees its custom name and live status.
This spec explores extending that same mechanism to a whole **line** — a
household or commute group that all rides the same route would see one
shared line-status view inside their group page, via a `group_lines` join
table mirroring `group_trains`.

## 1. Corrections — is extending this now premature? (read first)

This was flagged, before any design work started, as the riskiest of a
recent batch of proposed group-adjacent features — the one "most exposed
to 'too much, too soon'" — because the shared-groups feature had shipped
**one day** before this document was written, and its own spec drew its
scope narrowly on purpose. Two of that spec's own passages are worth
quoting exactly, because they're the test this extension has to pass:

> "A standalone 'share this tracked train via a link' feature was
> considered and rejected: it would mostly duplicate the already-public
> `{uid}/{date}` page once a train resolves, with the only genuine gaps
> being pre-resolution visibility and carrying the tracker's custom name
> along."

> Non-goals: "No changes to the existing public `{uid}/{date}` page or its
> own `ShareButton` — groups are an additive, separate visibility
> mechanism."

The parent spec's entire justification for building groups at all was that
tracked trains had a **real** gap versus their already-public detail page:
before a train resolves to a real UID, there is nothing to look at, and a
tracker's custom name lives nowhere public. Groups closed exactly those two
gaps, plus the ticket-privacy question, and nothing else.

**Neither gap exists for lines**, and that is the central finding of this
investigation, not an incidental note:

- A catalogue (or TfL) line's `/lines/{id}` page is **already fully public,
  unauthenticated, and bookmarkable** — confirmed by reading
  `frontend/app/lines/[id]/page.tsx`: it already renders a `<ShareButton />`
  unconditionally, next to the status badge, with no login gate anywhere on
  the read path (`GET /Line/{ids}/Status` takes no auth extractor at all —
  `crates/api/src/routes/line_status.rs`). There is no "pre-resolution"
  state analogous to an unresolved train — a line's status is either
  computed or it isn't, and if it isn't, the page 404s the same way for
  everyone.
- There is no private, per-viewer overlay analogous to a tracked train's
  custom name. A line's name, category, and operators are the same for
  every viewer; there is nothing here shaped like `custom_name` or
  `notifications_enabled` that a group could usefully un-hide.
- So a household that "all rides the same line" can **already** get this
  exact outcome today, with zero engineering: one member pastes the
  `/lines/{id}` URL into the group's group chat. `group_lines` would save
  that one paste and embed the status inline on the group page instead of
  requiring a click-through — a real, but modest, convenience gain, not a
  new capability the way tracked-train sharing was.

The one place a *real* gap could exist — sharing a **custom** line, which
is genuinely private and does carry a real per-owner boundary — collides
directly with a separate, very recently and deliberately hardened
decision. `docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md`
("Corrections: the investigation found a materially larger surface than
the brief named") locked custom lines down across **six** separate read
paths specifically so a custom line's name, status, and disruption history
are invisible to anyone but its owner, with `403` explicitly rejected
everywhere in favor of an indistinguishable 404. That work is confirmed
**already shipped** (verified directly against `crates/api/src/routes/lines.rs`
and `line_status.rs` — `get_line` requires `AuthenticatedUser` and 404s any
non-owner; `list_lines`/`list_custom_lines_for_user` are caller-scoped).
Building `group_lines` for custom lines now would mean carving a brand-new
exception into a privacy boundary that was finished days ago, before any
real usage signal exists for either feature. That is the textbook shape of
"too much, too soon."

**Recommendation: wait.** Not because the design is hard — §2–§7 below show
it isn't — but because:

1. The shared-groups feature has had zero days of real usage to learn
   whether members actually want a shared line view inside the group page
   versus the free bookmark-and-paste they already have.
2. The one version of this feature with genuine, non-duplicative value —
   sharing a *custom* line, e.g. a household's own hand-built "my commute"
   line — is exactly the version that requires re-opening a privacy design
   that was deliberately finished only days before this document, with no
   new information justifying reopening it yet.
3. Building the catalogue-only version now for its own sake risks shipping
   a second "additive, separate visibility mechanism" onto a schema whose
   own most recent addition (`group_trains`) was justified in writing by a
   real, singular gap — diluting that discipline by shipping a lookalike
   feature whose primary gap turns out to be "saves one paste."

The rest of this document designs the feature properly anyway, per the
brief — a design being valuable doesn't require recommending building it
immediately, and having the design ready removes "we'd have to figure this
out" as a reason to delay if/when real demand shows up (e.g. a group owner
asking for it, or telemetry showing members frequently re-sharing the same
line link inside a group's `group_trains` train card comments — this app
has no such comments feature today, but it would be the first concrete
demand signal worth watching for).

## 2. Current relevant state (verified 2026-09-12)

- `groups`/`group_members`/`group_trains`/`group_invite_links` — shipped,
  read directly from `crates/api/src/data/groups.rs`,
  `crates/api/src/routes/groups.rs`, and
  `crates/api/migrations/20260911090000_shared_groups.sql`. `GroupRole` has
  three tiers (`Owner`/`Admin`/`Member`) with `can_manage()`
  (`Owner | Admin`) and `is_owner()` predicates — this design reuses both
  verbatim, no new role or predicate.
- `group_trains` is a join table (`group_id`, `train_subscription_id`,
  `added_by`, `added_at`), composite PK, `ON DELETE CASCADE` on both FKs to
  `groups`/`train_subscriptions`. Adding a row is application-layer-gated
  on `train_subscriptions.user_id = current_user`; removing is gated on
  "the sharer, or any `admin`/`owner`." Departed-member cleanup deletes a
  leaving member's `group_trains` rows in the same transaction as their
  `group_members` removal.
- Custom lines are **private**, confirmed shipped (not merely proposed):
  `GET /public/lines/{id}` (`get_line`) requires `AuthenticatedUser` and
  404s for "doesn't exist," "exists but owned by someone else," and
  "legacy NULL-owner row" alike (that legacy state no longer exists either
  — `custom_lines.user_id` is `NOT NULL` per migration
  `20260901120000_custom_lines_owner_not_null.sql`). `list_lines`
  (`GET /public/lines`) calls `custom_lines::list_custom_lines_for_user`
  for an authenticated caller and omits custom lines entirely for an
  anonymous one. `get_line_status`/`get_mode_status`/
  `get_line_status_history` (`crates/api/src/routes/line_status.rs`) all
  filter custom-line rows by ownership via
  `custom_lines::owners_for_ids`. Catalogue lines (`app.config.lines`, the
  static TOML catalogue) and TfL lines (`queries::tfl_line_summaries`) have
  no owner concept and are fully public on every read path, unfiltered.
- Every custom line's id is guaranteed to start with `custom-`
  (`custom_lines::slugify` always produces `format!("custom-{slug}")`) —
  this is the one cheap, reliable signal this design uses to keep custom
  lines out of `group_lines` without a database round-trip on every
  candidate id.
- `frontend/app/groups/[id]/page.tsx` renders two sections today: Members
  and Shared trains, each with its own add/remove affordance
  (`AddTrainToGroupButton.tsx`, `RemoveGroupTrainButton.tsx`) gated on the
  same `canManage`/`viewerIsOwner`/`currentUserId` values `get_group`
  computes server-side.

## 3. Decisions (the design, built regardless of the timing recommendation)

### 3.1 Scope: catalogue and TfL lines only. Custom lines are out for v1.

**Decided, explicitly, not left implicit:** `group_lines` may only
reference a **catalogue** line (an id present in `app.config.lines`) or a
**TfL** line (an id present in `queries::tfl_line_summaries`). A custom
line id (`custom-*`) is rejected at the API layer with the same
`400`-shaped validation error this app already uses for other rejected
inputs, not a `404` — this is a real, named restriction on a value the
caller explicitly asked to add, not an ownership check hiding a secret
resource, so `403`'s "genuinely different case" reasoning from
`crates/api/src/routes/groups.rs`'s own module doc applies here too:
telling a group member "custom lines can't be shared into a group yet" is
honest, not a leak.

**Why, beyond the timing argument in §1:** a catalogue/TfL line has no
owner and no private data at any layer — sharing it into a group adds
*attribution* ("added by Alex") and a *convenience embed*, nothing a group
member couldn't already see by visiting `/lines/{id}` directly. A custom
line, by contrast, is now strictly single-owner-private end to end (§2).
Letting it into `group_lines` would require answering, at minimum: does
adding a custom line to a group grant every member read access to a
resource one specific user still fully owns and can still edit/delete
out from under the group? Does removing it from the group later need to
also revoke that access retroactively? Does a non-owning group member's
view of "the group's shared custom line" survive the owner renaming its
stations? None of these questions has a forced answer, and getting any of
them wrong reopens exactly the privacy surface
`2026-08-31-private-custom-lines-and-tracked-trains-design.md` closed
office-hours ago. Deferring custom lines entirely, rather than guessing at
answers no one has asked for yet, is the disciplined call — matching that
same document's own posture of naming a real question and explicitly not
answering it when nothing forces an answer (see its own "Open questions").

If custom-line sharing is wanted later, the shape most consistent with
this codebase's precedents would be: the *custom line's owner* uses their
own existing `admin`/`owner`-equivalent standing over the line (there is
none today — a custom line has exactly one owner, full stop) to decide
per-group visibility, most likely via a new, explicit
`custom_line_group_grants`-style table separate from `group_lines`, so a
catalogue line's "anyone can share it, it's already public" rule is never
accidentally applied to a resource that isn't. **Not designed further
here** — flagged as the natural extension point, not built.

### 3.2 Schema: `group_lines`, mirroring `group_trains`'s exact shape

```sql
CREATE TABLE group_lines (
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    line_id     TEXT NOT NULL,
    added_by    TEXT NOT NULL REFERENCES users(id),
    added_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, line_id)
);

-- "Which groups is this line shared into" has no caller today, but the
-- pattern this codebase already follows (group_trains_train_subscription_id,
-- group_members_user_id) is to index the join table's non-PK-leading FK
-- column preemptively rather than add it reactively once a caller exists.
CREATE INDEX group_lines_line_id ON group_lines (line_id);
```

Same join-table reasoning as `group_trains` §2.2 of the parent spec: a
line can be relevant to more than one group at once (a commute line shared
with both a "family" group and a "coworkers" group), so a `group_id` FK
directly on some `lines` table (which doesn't even exist — catalogue lines
are TOML files, not rows) would both foreclose that and have nowhere to
live anyway.

**Deliberately no FK on `line_id`.** There is no `lines` table to
reference: catalogue lines are static TOML (`app.config.lines`), TfL lines
are rows in `line_status`/derived tables with no dedicated "TfL lines"
table of their own. This is the same "free-form, application-validated
id" shape `pinned_lines.line_id` already uses today (see
`custom_lines.rs`'s own doc comment: "`pinned_lines` has no FK to
`custom_lines` by design... ids are free-form, client-supplied strings...
never validated against any line catalogue"). `group_lines` goes one step
further than `pinned_lines` by **validating at write time** (§3.1) rather
than accepting any string — closer to `group_trains`'s
application-layer ownership check than to `pinned_lines`'s total laxity —
but the schema itself still can't express that constraint via a real FK,
for the same structural reason `pinned_lines` can't.

**No departed-member cleanup, deliberately diverging from `group_trains`.**
The parent spec's departed-member cleanup exists because a `group_trains`
row's *entire reason to be visible* is the sharer's own private
subscription — once they leave, "shared by someone no longer in the
group" is actively confusing, and the underlying resource was never the
group's to keep. Neither is true here: a catalogue/TfL line has no
sharer-owned private data behind it, and every member (before or after
this member leaves) can already see the exact same public status by
visiting `/lines/{id}` directly. Auto-removing a shared line the moment
its adder leaves would delete a small, genuinely shared, ownerless
convenience — closer to how `groups.name` itself isn't deleted when its
renamer leaves — for no privacy benefit. **`added_by` is retained purely
as attribution** ("added by Alex") and is never used to gate anything once
a line is in the group; if `added_by`'s `users` row is ever deleted (no
such feature exists today, matching the parent spec's own noted hazard for
`group_members`' owner-join), the row simply keeps existing with a
dangling-in-spirit (but FK-valid, since `users(id) ON DELETE CASCADE` was
not applied here on purpose — see below) `added_by`.

Actually: `added_by REFERENCES users(id)` with **no** `ON DELETE CASCADE`
mirrors `group_invite_links.created_by`/`groups.created_by`'s own existing
"attribution FK, not a cascade target" shape from the parent migration —
consistent with an app that has no user-deletion feature at all yet
(same hazard the parent spec's `get_group_detail` doc comment already
names as latent, not new here).

### 3.3 Validating `line_id` at write time

New pure function, `is_custom_line_id`, living alongside
`add_line_to_group` in `data/groups.rs` (not `data/custom_lines.rs` — this
is group-feature-specific policy, not a custom-lines concern):

```rust
/// A line is shareable into a group iff it's a catalogue or TfL line —
/// never a custom line (see this feature's design doc §3.1). Custom ids
/// are recognized cheaply by their guaranteed `custom-` prefix
/// (`custom_lines::slugify` always produces it) with no database
/// round-trip; catalogue/TfL membership still needs one lookup each,
/// since there's no in-memory TfL line list to check against directly.
fn is_custom_line_id(line_id: &str) -> bool {
    line_id.starts_with("custom-")
}
```

The route handler (mirroring `add_group_train`'s shape) rejects a
`custom-`-prefixed id with `400` before touching the database at all, then
checks the id against `app.config.lines` (in-memory, free) and, if not
found there, against `queries::tfl_line_summaries` (one query, same one
`list_lines` already runs) — an id matching neither is also `400`, "not a
known line." No ambient-authority concern here the way `group_trains`'s
ownership check has: any current member may add *any* known catalogue/TfL
line, not just "one they own," because catalogue/TfL lines have no owner
to restrict to (see §3.4).

### 3.4 Permission model: `GroupRole` reused exactly, adding-a-line is deliberately broader than adding-a-train

| Action | Who | Compare to `group_trains` |
|---|---|---|
| Add a line to the group | Any current member, for **any** catalogue/TfL line id (§3.3) | `group_trains` restricts to trains **the member owns** — `group_lines` has no ownership concept to restrict by, so membership alone is the gate |
| Remove a line from the group | The member who added it, or any `admin`/`owner` | Identical to `group_trains` §3 |
| View shared lines | Any current member | Identical to `group_trains` §3 |

No new role, no new predicate — `GroupRole::can_manage()` gates the
manager-can-remove-anyone's-line branch exactly the way it already gates
`remove_train_from_group`'s equivalent branch; nothing here needed
`is_owner()`.

### 3.5 API surface

Added to the existing `crates/api/src/routes/groups.rs` router, same file,
same conventions (`404` for "not a member," never `403`; the shared
`require_member` helper gates all three):

| Method + path | Purpose |
|---|---|
| `GET /groups/{id}/lines` | List lines shared into the group, each with a live status snapshot |
| `POST /groups/{id}/lines` | Add a catalogue/TfL line (`{ lineId }`) — any current member |
| `DELETE /groups/{id}/lines/{lineId}` | Remove a shared line — sharer, or `admin`/`owner` |

`POST` validation errors (`400`, custom-line id or unknown id) reuse the
plain-text error-body convention `validate_group_name` already
establishes in this same file, rendered verbatim by the frontend the same
way `AddTrainToGroupButton`'s error `Alert` already does.

### 3.6 What a group member sees for a shared line

**Decided: an embedded live-status summary on the group page itself, plus
a link out to the full `/lines/{id}` page for depth** — the same shape
`group_trains` already established for shared trains (custom name + live
status inline, not just a bare link), applied to lines:

- **Embedded, inline**: the line's name, category, worst-severity badge
  (reusing `StatusBadge`/`severityLabel`/`worstStatus` exactly as
  `/lines/[id]/page.tsx` already computes them), the single worst
  currently-active reason if any, and "shared by {member}" attribution —
  a compact card, not the full `RepresentativeInfo`/`IssueList` treatment
  the dedicated page renders. This is the "at a glance, without leaving
  the group page" value the household use case actually wants.
- **Link out**: "View full status" to `/lines/{id}` for the complete issue
  list, TfL counterpart section, and trend charts — no reason to duplicate
  that page's full rendering inline when a `<Link>` already exists and
  costs nothing.

**Why not just a bare link with no embed:** a bare link would make
`group_lines` functionally identical to "paste the URL in chat" (§1's own
point about why this may not be worth building yet) — if it's built at
all, it should at least deliver the one real, non-duplicative value a
group page can offer over a raw bookmark: several lines' current status
visible in one glance, the way `group_trains`'s shared-trains section
already does for trains.

**No per-viewer "Never shown" list, unlike `group_trains`.** The parent
spec's §4 has a hard "never shown" list (tickets, `notifications_enabled`,
exact `tracked_at`) because a shared train carries private, per-subscription
data alongside its public live status. A catalogue/TfL line's status has
**no private data of any kind** — it's the exact same `LineStatusReport` any
anonymous visitor to `/lines/{id}` already sees. This is a genuine, worth-
stating asymmetry between the two features: `group_lines`'s read path
needs no ownership filtering at all (unlike `line_status.rs`'s handling of
custom lines, §2) precisely because §3.1 already excluded the one line
type that would have needed it.

Data layer: `list_group_lines(pool, group_id)` joins `group_lines` to the
existing `queries::line_status_for_ids`-style bulk lookup (same query
`GET /Line/{ids}/Status` already runs, called once with every `line_id` in
the group) rather than a new aggregation path — no new backend
computation, purely a new join over data the aggregator already produces
for every catalogue/TfL line regardless of any group.

### 3.7 Frontend surface

Extends `frontend/app/groups/[id]/page.tsx` with a third section,
"Shared lines," sitting alongside Members and Shared trains, same visual
language (`Card withBorder`, same attribution/remove-button placement as
`SharedTrainRow`):

- `AddLineToGroupButton.tsx` — mirrors `AddTrainToGroupButton.tsx`
  exactly: a modal, a `Select` populated from `GET /api/lines` (the
  existing `getAllLines()` call) **filtered to `source !== 'custom'`**
  client-side (belt-and-suspenders on top of the backend's own `400`
  rejection — never rely on the frontend filter alone for the actual
  security boundary, which is enforced server-side in §3.3), excluding
  lines already in `excludeLineIds`.
- `SharedLineRow` (colocated in `page.tsx`, same as `SharedTrainRow`) —
  renders the embedded summary from §3.6, a `<Link href={`/lines/${line
  .lineId}`}>` for "View full status," and a remove action gated
  identically to `SharedTrainRow`'s own `canRemove` computation
  (`canManage || addedBy === currentUserId`).
- `RemoveGroupLineButton.tsx` — mirrors `RemoveGroupTrainButton.tsx`
  verbatim, swapping the endpoint path.

## 4. Alternatives considered and rejected

1. **A bare link, no embedded status.** Rejected in §3.6 — would make the
   feature close to indistinguishable from pasting a URL in chat, the
   exact redundancy §1 already flags as the core reason to hesitate on
   building this at all; if built, it should deliver the one thing a raw
   link can't (an at-a-glance multi-line view).
2. **Allowing custom lines in v1, gated by the custom line's own owner
   opting in per-group.** Rejected for v1 — real design questions (revoke
   semantics, edit-while-shared, retroactive access) with no forcing
   demand yet, and it would reopen a privacy boundary finished days before
   this document (§3.1). Named as the natural extension point, not
   designed.
3. **Restricting "add a line" to `admin`/`owner` only, matching invite-link
   management's permission tier.** Rejected — a catalogue/TfL line carries
   no risk an `admin`/`owner`-only gate would meaningfully reduce (unlike
   invite-link generation, which controls who can join the group at all,
   or renaming, which is group-identity-affecting); restricting it would
   only make the feature more annoying to use for no safety gain, unlike
   `group_trains`'s ownership restriction, which exists for a real reason
   (a member can't share a train they don't own).
4. **Carrying over `group_trains`'s departed-member cleanup verbatim.**
   Rejected, reasoned in full in §3.2 — the cleanup exists to prevent a
   privacy-shaped confusion (someone no longer in the group still
   "owning" visible content) that doesn't apply to an ownerless, fully
   public line.
5. **A `group_id` FK directly on some new `lines` table.** Not viable —
   no such table exists; catalogue lines are TOML, TfL lines are derived
   rows with no independent identity table. A join table with a
   free-form, application-validated `line_id` is the only shape available,
   consistent with `pinned_lines`'s own precedent.

## 5. Non-goals

- **Custom lines.** Out of scope for v1, in full, per §3.1 — not a partial
  implementation, not a "read-only" carve-out, nothing. A `custom-`-
  prefixed `line_id` is rejected outright.
- **Per-group line notifications** ("notify the group when this line's
  status changes"). `docs/superpowers/specs/2026-09-02-line-status-notifications-design.md`
  already designs per-user line notifications separately; extending that
  to a group-broadcast concept is a distinct, unbuilt feature this spec
  does not touch or assume.
- **A "group's default/home line" concept**, or any ranking/ordering of
  shared lines beyond "oldest-shared-first" (matching `group_trains`'s own
  `ORDER BY added_at` convention).
- **Any change to `/lines/{id}`, `ShareButton`, or any existing public
  line-status read path.** This is purely additive, same posture the
  parent spec's own Non-goals took for the `{uid}/{date}` page.
- **Any change to `crates/aggregator`.** Line status is computed exactly
  as today, owner-blind and group-blind alike; `group_lines` only adds a
  new *read* join over existing output.
- **Caps on lines-per-group or groups-per-line.** Consistent with the
  parent spec's own "no cap" stance for members/groups.
- **Building this now, absent a concrete demand signal** — see §1's
  recommendation. This document is ready to execute against if/when that
  signal appears; it is not a recommendation to schedule it next.

## 6. Testing approach

Following `crates/api/src/data/groups.rs`'s existing `db_tests` convention
exactly (colocated `#[cfg(test)] mod db_tests`, `#[ignore]`d
live-database tests with the standard `DATABASE_URL` incantation in each
test's ignore message, `seed_user`/`cleanup` fixture helpers reused
as-is):

- `is_custom_line_id` — plain unit test, no database, mirroring
  `GroupRole`'s own `role_tests` module shape (pure-logic assertions,
  colocated, no `#[ignore]`).
- `add_line_to_group_rejects_a_custom_line_id` — asserts the `400` path
  fires before any `group_lines` row is written, mirroring
  `promote_to_admin_is_a_noop_against_the_owner_row`'s "prove the refusal
  really refused, not just returned an error code" pattern (query
  `group_lines` afterward, assert zero rows).
- `add_line_to_group_is_idempotent_for_a_known_catalogue_line` — mirrors
  `add_train_to_group`'s own `ON CONFLICT DO NOTHING` test shape (call
  twice, assert one row).
- `remove_line_from_group_a_non_manager_can_only_remove_their_own` —
  mirrors `remove_train_from_group`'s sharer-vs-manager split, same two-
  case shape (a plain member removing someone else's shared line fails;
  removing their own succeeds).
- `remove_member_does_not_touch_group_lines` — the deliberate
  divergence from §3.2's departed-member cleanup, proven directly: seed a
  shared line added by a member, remove that member from the group, assert
  the `group_lines` row survives. This is the one test in this suite that
  actively *disproves* a `group_trains`-style expectation, so it earns its
  own explicit assertion rather than being implied by omission.
- `delete_group_cascades_group_lines` — mirrors
  `delete_group_cascades_members_and_trains_and_invite_links` exactly,
  extended to also assert `group_lines` rows are gone.
- Route-level (`crates/api/src/routes/groups.rs`'s own `db_tests`
  convention, real `axum::Router` + real database): one test confirming a
  `POST /groups/{id}/lines` with a `custom-*` id gets `400` end-to-end
  (not just at the data layer), mirroring this file's existing posture
  that permission-dense routes need their own HTTP-level coverage beyond
  what the data layer proves.

## 7. Open questions / risks

1. **Whether the "wait" recommendation in §1 holds once real usage data
   exists for `group_trains`.** This design is ready to build against
   without revisiting §2–§6 if/when a concrete demand signal appears — the
   open question is purely about *when*, not *how*.
2. **Whether excluding custom lines from v1 (§3.1) turns out to be the
   single most-requested gap once this ships.** If so, the extension point
   named there (owner-scoped per-group grants, a separate table) is the
   recommended starting shape — not a reason to revisit the "custom lines
   never enter `group_lines` itself" boundary this design draws.
3. **TfL line validation cost.** §3.3's fallback check against
   `queries::tfl_line_summaries` is one query per add-attempt for any id
   not found in the in-memory catalogue — cheap at this app's stated
   "single trusted personal instance" scale, but worth re-checking if this
   route ever needs to handle bulk/scripted adds.
