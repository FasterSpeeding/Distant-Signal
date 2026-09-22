# Plan: Journey Tracking — Phase 4 (Group Sharing of Journeys)

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Scope: Phase 4 ONLY**, per
`docs/superpowers/specs/2026-09-22-journey-tracking-design.md` §9's phased
plan: *"Backend: `group_journeys` table + routes (§6, a near-verbatim copy of
`group_trains`'s pattern) + the one real new piece, the journey-scoped
cross-owner read authorization (§6's final paragraph). Frontend: journey share
button, `SharedJourneyRow` in the group detail page (mirroring
`SharedTrainRow`)."* This plan does not implement Phase 1 (single-leg
journeys + migration), Phase 2 (multi-leg chaining), or Phase 3
(journey-aware notifications + station skip) — those are separate,
in-parallel efforts. **Phase 4 has a hard, non-negotiable dependency on
Phase 1 having landed and merged first** — see "Assumptions about Phase 1"
below; every task in this plan that touches `journeys`/`journey_legs` or
`crates/api/src/routes/journeys.rs`/`crates/api/src/data/journeys.rs` is
blocked until that's true, not merely "nice to have first."

**Goal:** let a journey's owner share it into a group they belong to — the
same "add my personal resource to a shared group, view-only for everyone
else, sharer-or-manager can unshare" model `group_trains` already
established for tracked trains (`crates/api/migrations/20260911090000_shared_groups.sql`,
`crates/api/src/data/groups.rs`, `crates/api/src/routes/groups.rs`) — plus
the one piece that model doesn't already solve: letting a group member who
does not own a journey's underlying `train_subscriptions` rows nonetheless
read that journey's full live detail, because journey-level sharing is now
the authority for that one read path, not per-leg ownership.

**Architecture:** a straight backend-then-frontend build, in two backend
halves that are independent of each other's code (though both depend on
Phase 1's schema):

1. **The `group_trains` copy** (Tasks 1-3): one migration
   (`group_journeys`, identical shape to `group_trains`), the CRUD data
   functions in `crates/api/src/data/groups.rs` (`add_journey_to_group`,
   `remove_journey_from_group`, `list_group_journeys`,
   `list_shared_journeys_for_user`), and the routes in
   `crates/api/src/routes/groups.rs` (`GET`/`POST /groups/{id}/journeys`,
   `DELETE /groups/{id}/journeys/{journeyId}`, `GET
   /groups/shared-journeys`) — every one of these mirrors an existing
   `group_trains` (or, for the departed-member cascade, `remove_member`)
   function or route so closely that the diff against its `group_trains`
   counterpart is close to a search-and-replace of `train`→`journey` /
   `train_subscription`→`journey`, with the one substantive difference that
   `add_journey_to_group`'s ownership check targets `journeys` instead of
   `train_subscriptions`.
2. **The cross-owner read authorization** (Task 4): a new
   `journeys::journey_readable_by` predicate (modeled on
   `custom_lines::readable_custom_line_ids`'s "owned OR granted-into-a-group-
   I'm-in" OR-clause, per the closest existing precedent for widening a
   private resource's read gate for group sharing — see
   `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`),
   wired into Phase 1's `GET /Journeys/{id}` handler in place of its
   ownership-only gate. This is the one place this phase's diff is not a
   copy of an existing pattern — see Task 4's own design discussion.

Frontend (Tasks 5-8) then threads the new wire types and routes through:
`lib/types.ts`/`lib/api.ts` get the new shapes (Task 5, mirroring
`GroupTrain`/`SharedGroupTrain`), three new components mirror
`AddTrainToGroupButton`/`AddToGroupButton`/`RemoveGroupTrainButton` exactly
(Task 6), the group detail page gets a "Shared journeys" section with a
`SharedJourneyRow` mirroring `SharedTrainRow` (Task 7), and the journey
detail page gets the share button mounted in its header (Task 8).

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-journey-tracking-design.md` —
authoritative for every architectural decision this plan implements,
specifically §6 ("Groups sharing (#6a)") and the Phase 4 paragraph of §9.
This plan does not re-argue anything that spec already settled. Also
read for precedent (not re-implemented, only mirrored):
`docs/superpowers/specs/2026-09-11-shared-groups-design.md` (the original
`group_trains` design) and
`docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md` (the
closest existing precedent for widening a private resource's read gate to a
group, which Task 4 draws its shape from).

---

## Assumptions about Phase 1 (verify before starting Task 4; ideally before
## starting Task 1)

Phase 1 is being planned/implemented by a separate, parallel effort. As of
this plan's writing, **none of the following exists on `main`** (confirmed:
`find crates/api/src -iname "*journey*"` returns only the pre-existing,
narrower `crates/api/src/data/journey.rs` — calling-point overlay machinery,
not this feature's `journeys`/`journey_legs` — and no `frontend/app/journeys/`
directory exists). This plan assumes Phase 1 delivers, per the spec's §1.1
(cited as authoritative "even before Phase 1's own plan exists"):

1. **A `journeys` table**:
   ```sql
   CREATE TABLE journeys (
       id          BIGSERIAL PRIMARY KEY,
       user_id     TEXT NOT NULL REFERENCES users(id),
       custom_name TEXT,
       created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
       updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   ```
   The load-bearing part for this phase: **`journeys.user_id`** is the
   ownership column `add_journey_to_group`'s ownership check (Task 2) and
   `journey_readable_by`'s "OR owned by caller" branch (Task 4) both key
   off — the exact same shape `add_train_to_group` already uses against
   `train_subscriptions.user_id`.
2. **A `journey_legs` table** with (at minimum) `id`, `journey_id BIGINT
   NOT NULL REFERENCES journeys(id) ON DELETE CASCADE`, and
   `train_subscription_id BIGINT REFERENCES train_subscriptions(id) ON
   DELETE SET NULL`. Task 4 does not modify this table, but its read-path
   change (widening who may read a journey's legs) only matters because
   this table is how a journey's detail response reaches into
   `train_subscriptions` at all.
3. **A `GET /Journeys/{journeyId}` route** (mounted in a new
   `crates/api/src/routes/journeys.rs`, per the spec's front-matter naming
   convention) returning the journey plus its legs, each matched leg's
   detail read through the **existing** `TRACKED_TRAIN_STATE_SELECT`
   machinery (`train_tracking.rs:1151-`) joined via
   `journey_legs.train_subscription_id` — per spec §4: *"No new backend
   read-model is strictly required beyond a `GET /Journeys/{id}` that
   returns `{journey, legs: [{..., trackedTrainState?: TrackedTrainState}]}`"*.
   This plan assumes that handler's authorization gate follows this
   codebase's own dominant, explicitly-documented pattern — the exact
   shape `train_tracking::tracked_train_owner` +
   `train_tracking::get_by_tracking_id` already use for
   `GET /Train/{trackingId}` (`routes/train.rs:552-582`): a **separate**
   ownership-lookup function (assumed name `journeys::journey_owner`,
   mirroring `tracked_train_owner`'s exact
   `SELECT user_id FROM journeys WHERE id = $1` shape), called by the route
   handler *before*, and independently of, a detail-fetch function that
   itself does **not** re-filter by `user_id` (mirroring
   `get_by_tracking_id(pool, id)`, which takes no `user_id` parameter at
   all — the gate and the fetch are two separate calls). Task 4 is written
   against this assumption, with an explicit fallback (Task 4, Step 0) for
   the case where Phase 1 instead combined ownership-check-and-fetch into
   one query (the shape `groups::get_group_detail` uses, joining `WHERE
   g.id = $1 AND ... caller.user_id = $2` in a single `SELECT`) — **the
   first concrete action of Task 4 is to read Phase 1's actual landed code
   and confirm or correct this assumption before writing any of that
   task's own code.**
4. **`GET /Journeys/mine`**, a list of the caller's own journeys — assumed
   to exist for Task 6's `AddJourneyToGroupButton` (the direct analogue of
   `AddTrainToGroupButton`'s `GET /Train/mine` fetch), and for Task 8's
   assumption that the journey detail page (`frontend/app/journeys/[id]/page.tsx`)
   exists with a header a share button can be mounted into.

**If any of these assumptions is wrong once Phase 1 has actually landed**,
the fix is local to whichever task depends on the wrong part (most likely
Task 4, given how many of the above bullets converge on the shape of one
route handler) — re-read the real Phase 1 diff, adjust names/shapes in that
task only, and continue. Nothing in Tasks 1-3 (the `group_journeys`
table/routes themselves) depends on anything beyond bullet 1
(`journeys.id`/`journeys.user_id` existing) and bullet 2
(`journey_legs`/`train_subscription_id` existing, for `list_group_journeys`'s
join), so those three tasks are far more robust to Phase 1 naming
differences than Task 4 is.

---

## Judgment calls this plan makes

1. **Where does `journey_readable_by` live: `data/journeys.rs`, or
   `data/groups.rs`? `data/journeys.rs` — beside the resource it protects,
   not beside the sharing mechanism.** This mirrors the one existing
   precedent for exactly this shape of function:
   `custom_lines::readable_custom_line_ids` lives in `custom_lines.rs`, not
   `groups.rs`, even though it joins through `custom_line_group_grants` and
   `group_members` (both conceptually "groups" tables) — see
   `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`
   §3.2's own call site list. The reasoning transfers directly: every
   *other* reader of a journey's detail (once Phase 2/3 add more of them)
   will want to ask "can this caller read journey N," and that question
   belongs with the thing being protected, not scattered into the sharing
   module every future caller would otherwise have to know to import from.

2. **`GroupJourney`/`SharedJourney`'s display shape: mirror `GroupTrain`
   via the journey's FIRST leg only, plus a `legCount` field — not a full
   multi-leg status rollup.** `GroupTrain` embeds one train's live identity
   and status (origin/destination/departure/status/delay) directly on the
   row. A journey can have more than one leg once Phase 2 ships, and a
   worst-status-across-legs rollup is explicitly described in spec §3 as
   reusing the `worstStatus`/`GROUP_RANK` idiom — but that rollup is Phase
   1/3's own concern for the journey's *own* detail page, not something
   this phase should invent a second copy of just to decorate a group-list
   row. This plan's `GroupJourney` instead joins `journey_legs` filtered to
   `leg_order = 1` (guaranteed to exist for every journey, since Phase 1's
   own migration always creates at least one leg) for the same
   identity/status fields `GroupTrain` already shows, plus a plain
   `legCount: i64` (`COUNT(*)` over that journey's legs) so a multi-leg
   journey's row can at least say "+2 more legs" rather than silently
   implying it's a single-train journey. If Phase 3 later lands a real
   worst-status rollup as a reusable query, upgrading this row to call it
   instead is a small, isolated follow-up to this table's query — not
   blocking Phase 4's own ship.

3. **No `sharedGroupCount`-style field is added to `Journey`/
   `JourneyListItem` in this phase.** `TrackedTrainListItem.sharedGroupCount`
   (used by `DeleteTrainButton`'s "this train is shared into N groups"
   warning) lives on a Phase-1-owned type, not one this phase's file scope
   touches. Adding the journey equivalent is real, follow-up value (a
   future `DeleteJourneyButton` will want it) but is out of scope here —
   flagged as a note for whichever phase (1, 2, or a small follow-up) owns
   `Journey`/`JourneyListItem`'s shape, not attempted by this plan.

4. **`/journeys/mine` is NOT extended to merge in group-shared journeys in
   this phase**, even though `/track/mine` does exactly that for
   `group_trains` (merging `getMyTrackedTrains`/`getSharedGroupTrains`).
   The task brief for this phase scopes the frontend deliverable
   explicitly to "a journey share button, and a `SharedJourneyRow`
   component in the group detail page" — not a `/journeys/mine` merge.
   `list_shared_journeys_for_user`/`GET /groups/shared-journeys` are still
   built in this phase (§6's routes list, and needed for backend
   completeness/parity with `group_trains`'s own route set), just not yet
   consumed by any frontend page. This is the same "backend route exists,
   one frontend consumer doesn't yet" gap
   `list_shared_trains_for_user`'s own doc comment already accepts for the
   *outgoing* direction of `group_trains` sharing ("a train shared WITH the
   caller is tagged, while one the caller shared OUT carries no group tag
   ... a deliberate follow-up, not something this query is hiding") — same
   posture, applied here to the whole `/journeys/mine` merge rather than
   half of one row's tagging.

5. **`journey_readable_by` returns a single `bool`, not a set of readable
   ids**, unlike `readable_custom_line_ids` (which takes a batch of ids for
   the bulk `get_mode_status` call site). There is exactly one call site
   for this phase (`GET /Journeys/{journeyId}`, a single-id read) and no
   bulk-list analogue to `get_mode_status` exists for journeys in Phase
   1-3's scope — a `HashSet`-returning, id-batched version would be
   premature generality for a function with one caller. If a future bulk
   journey-status list needs the same check, it can be added as a sibling
   function at that point, matching how `owners_for_ids` and
   `tracked_train_owner` coexist today as separate single-id/batch
   functions rather than one over-general one.

---

## Non-goals

(Restated from the spec's own Phase 4 scoping and this plan's own file
scope — no task below touches any of these.)

- **Phase 1 (single-leg journeys + migration), Phase 2 (multi-leg
  chaining), Phase 3 (journey-aware notifications + station skip).**
  Entirely out of scope; this plan assumes Phase 1 has already landed (see
  above) and touches none of Phase 2/3's work.
- **§5.3's open question ("does a group a journey is shared into also get
  push notifications for it?").** Spec's own recommendation is owner-only
  for Phase 1, and this phase adds no notification logic of any kind —
  sharing a journey into a group has exactly the same "view-only, no side
  effects for anyone else" posture `group_trains`/`custom_line_group_grants`
  already established for their own resources.
- **Any change to `journeys`/`journey_legs`' own write paths** (rename,
  add-leg, commit-leg, delete). Every one of those stays gated purely on
  `journeys.user_id = caller.id`, via whatever `journey_owner`-style
  function Phase 1 already wrote — this phase's new `journey_readable_by`
  is used **only** by the `GET /Journeys/{journeyId}` read path (Task 4);
  it must never be substituted into a write-path gate. See Task 4's own
  guardrail.
- **§8 Open Question 6 ("does sharing a journey into a group also grant
  `group_trains`-level access to its individual legs")** — the spec
  explicitly assumes "no" (journey-scoped access is sufficient, legs never
  independently visible outside the journey view), and this plan builds
  nothing that would let a group member reach a shared journey's
  individual leg through `GET /Train/{trackingId}` or any `group_trains`
  route. `journey_readable_by` only ever gates the journey-level read; it
  is not consulted by `tracked_train_owner`, `get_by_tracking_id`, or any
  existing `group_trains` route, and no task in this plan adds a route
  that would let a caller pass a leg's raw `train_subscription_id` to any
  of those existing per-train endpoints.
- **Extending `/journeys/mine`** to merge in group-shared journeys — see
  Judgment Call 4.
- **A `sharedGroupCount`-style field on `Journey`/`JourneyListItem`** — see
  Judgment Call 3.
- **A worst-status-across-legs rollup for `GroupJourney`/`SharedJourney`**
  — see Judgment Call 2.

## Global Constraints

- **Hard external dependency: Phase 1 must be merged to `main` before Task
  1 of this plan is started** (Task 1's migration references
  `journeys(id)`; Tasks 2-4's Rust code references `journeys.user_id` and,
  for Task 4, Phase 1's own route/handler). If Phase 1 has not yet merged
  when this plan is picked up, work may still proceed as far as writing
  Tasks 1-3's code against the schema this plan documents above, but no
  task's Verify step (which requires a real `journeys` table to exist) can
  actually run, and Task 4 cannot be started at all until Phase 1's real
  `GET /Journeys/{journeyId}` handler exists to read and adapt to.
- **Every new route follows the 404-never-403 ownership convention**
  documented in `crates/api/src/routes/groups.rs`'s own module doc for
  *membership* (a non-member gets `404`, "no group with that id," never a
  signal the group exists) — this phase's new routes are mounted in that
  same file and inherit that file's existing `require_member`/`require_role`
  gates unchanged. Where `add_journey_to_group` needs a stronger check
  (does the caller own *this specific journey*, not just "are they a group
  member"), that check is **404, never 403**, for the same reason
  `add_train_to_group`'s "no train with that id" is — see Task 2.
- **`journey_readable_by` is a READ-ONLY authorization widening.** It must
  never be used to gate any write route (`POST`/`PUT`/`DELETE` on
  `/Journeys/*` or `/groups/*/journeys*`) — every write in this phase and
  every write Phase 1/2/3 already wrote stays owner-only. See Task 4's own
  guardrail comment, which must be included verbatim (or word-for-word
  equivalent) in the code, the same way the custom-tracking-names plan's
  own privacy-audit-addendum comment was required verbatim in its
  migration.
- **Departed-member cleanup for `group_journeys` mirrors `group_trains`,
  NOT `custom_line_group_grants`** — per spec §6's explicit instruction
  ("§6 recommends journeys follow the `group_trains` precedent (cascade on
  departure) ... a journey is an active, live-tracked personal thing, not
  a static definition"). `remove_member`'s existing departed-member cleanup
  gains one more `DELETE ... WHERE group_id = $1 AND added_by = $2`
  statement, in the same transaction, for `group_journeys` — see Task 2.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings`, `cargo test --workspace`
  (ignored tests skipped, matching CI's fast default), and `cargo test -p
  api -- --ignored --test-threads=1` for every DB-gated test this plan adds
  (requires `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres`
  against a real local Postgres with Phase 1's migrations already applied).
  Frontend: `npm test -- <file>` (vitest) per changed test file, plus a
  full `npm test` and `npm run build` before considering the frontend tasks
  done. **UI verification**: per this repo's standing practice for a
  change with no automated end-to-end coverage, start the dev stack and
  manually verify in a real browser once Phase 1's UI exists to click
  through: create two accounts, one journey, share it from one account
  into a group the other account is a member of, confirm the second
  account can open `/groups/{id}` and see the `SharedJourneyRow`, click
  through to `/journeys/{id}`, and see the SAME full live detail the owner
  sees (not a 404) — this is the concrete, end-to-end proof Task 4's
  authorization widening actually works. Folded into Task 7/8's own Verify
  steps, not a separate task.
- **File scope.** Modified/created:
  `crates/api/migrations/<TIMESTAMP>_group_journeys.sql` (new — timestamp
  chosen at Task 1 execution time, after Phase 1's own migration; see Task
  1, Step 0),
  `crates/api/src/data/groups.rs`,
  `crates/api/src/routes/groups.rs`,
  `crates/api/src/data/journeys.rs` (Phase 1-created; modified here),
  `crates/api/src/routes/journeys.rs` (Phase 1-created; modified here),
  `frontend/lib/types.ts`,
  `frontend/lib/api.ts`,
  `frontend/components/ShareJourneyButton.tsx` (new),
  `frontend/components/ShareJourneyButton.test.tsx` (new),
  `frontend/components/AddJourneyToGroupButton.tsx` (new),
  `frontend/components/AddJourneyToGroupButton.test.tsx` (new),
  `frontend/components/RemoveGroupJourneyButton.tsx` (new),
  `frontend/app/groups/[id]/page.tsx`,
  `frontend/app/journeys/[id]/page.tsx` (Phase 1-created; modified here, or
  a header-controls component it delegates to — see Task 8).
  No other file changes. In particular, **no task in this plan touches
  `crates/api/src/routes/train.rs`, `train_tracking.rs`,
  `crates/notifier`, or anything under `frontend/app/track/`** — those are
  the existing single-train surface, untouched by group-sharing a journey.

---

## Task 1: Migration — `group_journeys` table

**Files:** create `crates/api/migrations/<TIMESTAMP>_group_journeys.sql`.

Independent of Tasks 2-4's Rust code (a bare `CREATE TABLE` needs no Rust
to apply), but hard-blocked on Phase 1's own `journeys` migration having
already landed, since the FK below references `journeys(id)`.

- [ ] **Step 0: Pick the real timestamp.** Once Phase 1 has merged, run
  `ls crates/api/migrations/ | sort | tail -5` and choose a timestamp that
  sorts after both the latest migration on `main` at that point AND
  Phase 1's own `journeys`/`journey_legs` migration specifically (the two
  may not be the same file if Phase 1 shipped other migrations after its
  own journeys one — check `journeys`/`journey_legs` are visible via
  `psql "$DATABASE_URL" -c "\d journeys"` before proceeding). As of this
  plan's writing the latest migration on `main` is
  `20260917090000_incidents_affected_lines.sql` — this is a floor, not the
  real answer; do not use a timestamp from before Phase 1's migration.

- [ ] **Step 1: Write the migration**, following `group_trains`'s own
  migration comment style
  (`crates/api/migrations/20260911090000_shared_groups.sql`) and schema
  exactly, per spec §6:

```sql
-- -------------------------------------------------------------------------
-- Sharing a journey into a group: a near-verbatim copy of group_trains's
-- own shape (20260911090000_shared_groups.sql), for the same reason --
-- one journey can be shared into more than one group at once, so this is
-- a JOIN TABLE, not a `group_id` column on `journeys` itself. See
-- docs/superpowers/specs/2026-09-22-journey-tracking-design.md §6.
--
-- Departed-member cleanup: mirrors group_trains (cascade -- see
-- groups::remove_member's own extended cleanup), NOT
-- custom_line_group_grants (which deliberately persists after the
-- granter leaves). Spec §6's own reasoning: "a journey is an active,
-- live-tracked personal thing, not a static definition" -- same category
-- as a tracked train, not a custom line.
--
-- View access to a shared journey's full live detail (every leg's bound
-- train_subscriptions row) is NOT gated by this table alone -- see
-- journeys::journey_readable_by (crates/api/src/data/journeys.rs), the
-- new authorization path this table's existence makes possible but which
-- lives in its own function, not a view or a second table.
-- -------------------------------------------------------------------------

CREATE TABLE group_journeys (
    group_id   TEXT   NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    journey_id BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    added_by   TEXT   NOT NULL REFERENCES users(id),
    added_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, journey_id)
);

-- "Which groups is this journey shared into" -- needed by
-- journey_readable_by's membership check and by a future owner-facing
-- "shared with: Family, Commute Club" indicator (not built in this
-- phase -- see this plan's Non-goals). The PK's leading column (group_id)
-- doesn't cover this, the same reason group_trains_train_subscription_id
-- and custom_line_group_grants_line_id exist.
CREATE INDEX group_journeys_journey_id ON group_journeys (journey_id);
```

- [ ] **Step 2: Verify.** `sqlx` migrations in this crate run automatically
  against `DATABASE_URL` on `cargo test`/`cargo run` startup. Confirm the
  migration applies cleanly against a real local Postgres that already has
  Phase 1's `journeys` table:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d group_journeys"
```

  Expected: no `sqlx::migrate::MigrateError`, and `\d group_journeys` shows
  the four columns, the composite PK, and both FKs (`group_id ...
  ON DELETE CASCADE`, `journey_id ... ON DELETE CASCADE`).

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/<TIMESTAMP>_group_journeys.sql
git commit -m "api: add group_journeys table, mirroring group_trains's sharing model"
```

---

## Task 2: Data layer — `crates/api/src/data/groups.rs`

**Files:** modify `crates/api/src/data/groups.rs`.

Depends on Task 1 (the table must exist for these functions to compile
against a real schema at test time). Every function here is written by
direct analogy to its `group_trains`/`remove_member` counterpart, already
read in full during this plan's own research — cited by name below so the
diff is checkable line-for-line against the original.

- [ ] **Step 1: Add `GroupJourney`**, directly below `GroupTrain`
  (`data/groups.rs:785-838`), mirroring its shape per Judgment Call 2
  (first-leg identity + `legCount`, not a full rollup):

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupJourneyRow {
    journey_id: i64,
    custom_name: Option<String>,
    leg_count: i64,
    // First-leg (leg_order = 1) identity + live status -- Judgment Call 2:
    // a journey always has at least one leg (Phase 1's migration
    // guarantees this), so this join can never come back NULL for a
    // well-formed journey.
    pin_origin_crs: Option<String>,
    pin_destination_crs: Option<String>,
    pin_origin_name: Option<String>,
    pin_destination_name: Option<String>,
    pin_scheduled_departure: Option<DateTime<Utc>>,
    service_date: chrono::NaiveDate,
    resolution_status: Option<String>,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
    added_by: String,
    added_by_name: Option<String>,
    added_by_username: Option<String>,
}

/// A shared journey's display shape for `GET /groups/{id}/journeys` --
/// `GroupTrain`'s direct analogue, one level up. Carries the FIRST leg's
/// identity/live-status fields (Judgment Call 2 in this feature's plan) so
/// the group page can render something useful without duplicating a
/// worst-status-across-legs rollup that belongs to the journey's own
/// detail page instead. `legCount` lets the row at least signal "there's
/// more" for a multi-leg journey once Phase 2 ships.
///
/// Same hard privacy constraint `GroupTrain` documents: no ticket field,
/// no `notificationsEnabled`/exact `addedAt` -- a shared journey's group
/// row must never widen past what `group_trains` already decided was safe
/// to show a fellow member for the underlying tracked-train resource.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupJourney {
    pub journey_id: i64,
    pub custom_name: Option<String>,
    pub leg_count: i64,
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
    pub service_date: chrono::NaiveDate,
    pub resolution_status: Option<String>,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub added_by: String,
    pub added_by_name: Option<String>,
    /// Same contract as `GroupTrain.added_by_tag`.
    pub added_by_tag: Option<String>,
}

impl From<GroupJourneyRow> for GroupJourney {
    fn from(row: GroupJourneyRow) -> Self {
        let added_by =
            users::MemberDisplay::of(row.added_by_name, row.added_by_username, &row.added_by);
        GroupJourney {
            journey_id: row.journey_id,
            custom_name: row.custom_name,
            leg_count: row.leg_count,
            pin_origin_crs: row.pin_origin_crs,
            pin_destination_crs: row.pin_destination_crs,
            pin_origin_name: row.pin_origin_name,
            pin_destination_name: row.pin_destination_name,
            pin_scheduled_departure: row.pin_scheduled_departure,
            service_date: row.service_date,
            resolution_status: row.resolution_status,
            train_uid: row.train_uid,
            status: row.status,
            delay_minutes: row.delay_minutes,
            added_by: row.added_by,
            added_by_name: added_by.label,
            added_by_tag: added_by.tag,
        }
    }
}
```

  **Note on the `leg_count`/`resolution_status: Option<String>` shift from
  `GroupTrain`'s bare `String`**: `GroupTrain.resolution_status` is
  non-optional because `group_trains.train_subscription_id` is `NOT NULL`
  and always joins to a real row. A journey's first leg's
  `train_subscription_id` may itself be `NULL` (per spec §1.1, an
  unmatched open-window leg — though Phase 1 ships manual-pick only, so in
  practice every leg created via Phase 1's UI has one; Phase 2+ may not),
  so this row's leg-detail fields must all tolerate a `NULL` join. Confirm
  the actual nullability of `journey_legs.train_subscription_id` against
  Phase 1's real migration before finalizing this struct — it should
  already be nullable per the spec's own schema (§1.1: `train_subscription_id
  BIGINT REFERENCES train_subscriptions(id) ON DELETE SET NULL`).

- [ ] **Step 2: Add `add_journey_to_group`**, directly below
  `add_train_to_group` (`data/groups.rs:692-719`), mirroring its ownership
  check and idempotent insert exactly:

```rust
/// Adds one of the caller's own journeys to a group. Ownership is enforced
/// at the APPLICATION layer, not the DB -- the exact `WHERE id = $1 AND
/// user_id = $2` shape [`add_train_to_group`] already uses, now against
/// `journeys` instead of `train_subscriptions` (spec §6: "share requires
/// the caller to own the journey ... same as `add_train_to_group`'s
/// ownership check"). Idempotent: re-adding an already-shared journey is a
/// silent no-op (`ON CONFLICT DO NOTHING`), matching
/// [`add_train_to_group`].
///
/// Returns `false` if `journey_id` doesn't exist or isn't owned by
/// `user_id` -- the route maps this to `404`, never `403`, same as every
/// other ownership check in this file.
pub async fn add_journey_to_group(
    pool: &PgPool,
    group_id: &str,
    journey_id: i64,
    user_id: &str,
) -> Result<bool> {
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if owned.is_none() {
        return Ok(false);
    }

    sqlx::query(
        "INSERT INTO group_journeys (group_id, journey_id, added_by, added_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (group_id, journey_id) DO NOTHING",
    )
    .bind(group_id)
    .bind(journey_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(true)
}
```

- [ ] **Step 3: Add `remove_journey_from_group`**, directly below
  `remove_train_from_group` (`data/groups.rs:728-753`), mirroring its
  sharer-or-manager two-branch shape exactly:

```rust
/// Removes a shared journey from a group. `caller_can_manage` should be
/// the route's own already-resolved `GroupRole::can_manage()` -- an
/// `admin`/`owner` may remove ANY shared journey; anyone else may only
/// remove one THEY added (spec §6: "unshare is sharer-or-manager", same
/// rule as [`remove_train_from_group`]). Returns `false` if no matching
/// row was deleted -- the route maps that to `404`.
pub async fn remove_journey_from_group(
    pool: &PgPool,
    group_id: &str,
    journey_id: i64,
    user_id: &str,
    caller_can_manage: bool,
) -> Result<bool> {
    let result = if caller_can_manage {
        sqlx::query("DELETE FROM group_journeys WHERE group_id = $1 AND journey_id = $2")
            .bind(group_id)
            .bind(journey_id)
            .execute(pool)
            .await?
    } else {
        sqlx::query(
            "DELETE FROM group_journeys \
             WHERE group_id = $1 AND journey_id = $2 AND added_by = $3",
        )
        .bind(group_id)
        .bind(journey_id)
        .bind(user_id)
        .execute(pool)
        .await?
    };
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 4: Add `list_group_journeys`**, directly below
  `list_group_trains` (`data/groups.rs:845-867`):

```rust
/// Every journey shared into `group_id`, oldest-shared first. No
/// permission check here -- the route's own `require_member` call gates
/// "is the caller even a member," the same split [`list_group_trains`]
/// already uses.
///
/// The first-leg join (`jl.leg_order = 1`) and the `leg_count` subquery
/// are this function's one real divergence from `list_group_trains` --
/// see `GroupJourney`'s own doc comment (Judgment Call 2 in this
/// feature's plan) for why.
pub async fn list_group_journeys(pool: &PgPool, group_id: &str) -> Result<Vec<GroupJourney>> {
    let rows: Vec<GroupJourneyRow> = sqlx::query_as(
        "SELECT gj.journey_id, j.custom_name, \
                (SELECT COUNT(*) FROM journey_legs jl2 WHERE jl2.journey_id = j.id) AS leg_count, \
                ts.pin_origin_crs, ts.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                ts.pin_scheduled_departure, jl.service_date, ts.resolution_status, \
                tr.train_uid, cs.status, cs.delay_minutes, \
                gj.added_by, u.name AS added_by_name, u.username AS added_by_username \
         FROM group_journeys gj \
         JOIN journeys j ON j.id = gj.journey_id \
         JOIN journey_legs jl ON jl.journey_id = j.id AND jl.leg_order = 1 \
         JOIN users u ON u.id = gj.added_by \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(ts.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(ts.pin_destination_crs) \
         WHERE gj.group_id = $1 \
         ORDER BY gj.added_at",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(GroupJourney::from).collect())
}
```

  **Confirm `journey_legs.service_date` really exists** (per spec §1.1 it
  does, one column per leg, distinct from `journeys` itself having no date
  of its own) before landing this query — adjust the `jl.service_date`
  reference if Phase 1's real column differs.

- [ ] **Step 5: Add `SharedJourney`/`SharedJourneyRow` and
  `list_shared_journeys_for_user`**, directly below the equivalent
  `SharedTrain`/`SharedTrainRow`/`list_shared_trains_for_user`
  (`data/groups.rs:869-1033`) — same `group_id`/`group_name`-tagged
  extension of `GroupJourney`, same `LIMIT
  crate::data::train_tracking::MINE_LIST_LIMIT` cap, same "one row per
  (group, journey) pair" shape, same `ORDER BY gj.added_at DESC,
  gj.journey_id DESC` tie-break:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct SharedJourneyRow {
    group_id: String,
    group_name: String,
    journey_id: i64,
    custom_name: Option<String>,
    leg_count: i64,
    pin_origin_crs: Option<String>,
    pin_destination_crs: Option<String>,
    pin_origin_name: Option<String>,
    pin_destination_name: Option<String>,
    pin_scheduled_departure: Option<DateTime<Utc>>,
    service_date: chrono::NaiveDate,
    resolution_status: Option<String>,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
    added_by: String,
    added_by_name: Option<String>,
    added_by_username: Option<String>,
}

/// One journey shared into one group the CALLER is a member of --
/// [`GroupJourney`] plus the two fields that only make sense once rows
/// from several groups land in one list, mirroring [`SharedTrain`]
/// exactly. Not consumed by any frontend page in this phase -- see this
/// feature's plan, Judgment Call 4 -- but built now for parity with
/// `group_trains`'s own route/function set, per spec §6.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedJourney {
    pub group_id: String,
    pub group_name: String,
    pub journey_id: i64,
    pub custom_name: Option<String>,
    pub leg_count: i64,
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
    pub service_date: chrono::NaiveDate,
    pub resolution_status: Option<String>,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub added_by: String,
    pub added_by_name: Option<String>,
    pub added_by_tag: Option<String>,
}

impl From<SharedJourneyRow> for SharedJourney {
    fn from(row: SharedJourneyRow) -> Self {
        let added_by =
            users::MemberDisplay::of(row.added_by_name, row.added_by_username, &row.added_by);
        SharedJourney {
            group_id: row.group_id,
            group_name: row.group_name,
            journey_id: row.journey_id,
            custom_name: row.custom_name,
            leg_count: row.leg_count,
            pin_origin_crs: row.pin_origin_crs,
            pin_destination_crs: row.pin_destination_crs,
            pin_origin_name: row.pin_origin_name,
            pin_destination_name: row.pin_destination_name,
            pin_scheduled_departure: row.pin_scheduled_departure,
            service_date: row.service_date,
            resolution_status: row.resolution_status,
            train_uid: row.train_uid,
            status: row.status,
            delay_minutes: row.delay_minutes,
            added_by: row.added_by,
            added_by_name: added_by.label,
            added_by_tag: added_by.tag,
        }
    }
}

/// Every journey shared into ANY group `user_id` belongs to, EXCLUDING the
/// ones they own themselves -- mirrors [`list_shared_trains_for_user`]
/// exactly, including its `LIMIT`/ordering/one-row-per-pair reasoning; see
/// that function's own doc comment for the full justification, which
/// applies here unchanged with `journeys`/`group_journeys` in place of
/// `train_subscriptions`/`group_trains`.
pub async fn list_shared_journeys_for_user(
    pool: &PgPool,
    user_id: &str,
) -> Result<Vec<SharedJourney>> {
    let rows: Vec<SharedJourneyRow> = sqlx::query_as(
        "SELECT g.id AS group_id, g.name AS group_name, \
                gj.journey_id, j.custom_name, \
                (SELECT COUNT(*) FROM journey_legs jl2 WHERE jl2.journey_id = j.id) AS leg_count, \
                ts.pin_origin_crs, ts.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                ts.pin_scheduled_departure, jl.service_date, ts.resolution_status, \
                tr.train_uid, cs.status, cs.delay_minutes, \
                gj.added_by, u.name AS added_by_name, u.username AS added_by_username \
         FROM group_members me \
         JOIN groups g ON g.id = me.group_id \
         JOIN group_journeys gj ON gj.group_id = g.id \
         JOIN journeys j ON j.id = gj.journey_id \
         JOIN journey_legs jl ON jl.journey_id = j.id AND jl.leg_order = 1 \
         JOIN users u ON u.id = gj.added_by \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(ts.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(ts.pin_destination_crs) \
         WHERE me.user_id = $1 AND j.user_id <> $1 \
         ORDER BY gj.added_at DESC, gj.journey_id DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(SharedJourney::from).collect())
}
```

- [ ] **Step 6: Extend `remove_member`'s departed-member cleanup**
  (`data/groups.rs:481-485`, the `DELETE FROM group_trains WHERE group_id =
  $1 AND added_by = $2` statement) to also clean up `group_journeys`, in
  the same transaction:

```rust
    // Departed-member cleanup (§2.2 of the shared-groups design, decided) --
    // same transaction as the removal below. group_journeys follows
    // group_trains's own cleanup precedent, not custom_line_group_grants's
    // persist-after-departure exception -- spec §6: "a journey is an
    // active, live-tracked personal thing, not a static definition."
    sqlx::query("DELETE FROM group_trains WHERE group_id = $1 AND added_by = $2")
        .bind(group_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM group_journeys WHERE group_id = $1 AND added_by = $2")
        .bind(group_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;
```

- [ ] **Step 7: Add DB-gated `db_tests`**, alongside the existing
  `group_trains` tests in the same `mod db_tests` block
  (`data/groups.rs:1404-`), mirroring each cited test's exact fixture/assert
  shape:

```rust
    async fn seed_journey(pool: &PgPool, user_id: &str) -> i64 {
        let journey_id: (i64,) = sqlx::query_as(
            "INSERT INTO journeys (user_id, custom_name) VALUES ($1, NULL) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("seed a journey");
        sqlx::query(
            "INSERT INTO journey_legs \
                (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             VALUES ($1, 1, 'WOK', 'WAT', CURRENT_DATE, 'manual')",
        )
        .bind(journey_id.0)
        .execute(pool)
        .await
        .expect("seed a journey leg");
        journey_id.0
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_journey_to_group_rejects_a_journey_the_caller_does_not_own -- --ignored`"]
    async fn add_journey_to_group_rejects_a_journey_the_caller_does_not_own() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-ADDJOURNEY-OWNER-1").await;
        seed_user(&pool, "TEST-GROUPS-ADDJOURNEY-STRANGER-1").await;
        let group_id = create_group(&pool, "Add Journey Test 1", "TEST-GROUPS-ADDJOURNEY-OWNER-1")
            .await
            .expect("create group");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-ADDJOURNEY-STRANGER-1").await;

        let added = add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-GROUPS-ADDJOURNEY-OWNER-1", // owns the GROUP, not the journey
        )
        .await
        .expect("add attempt");
        assert!(!added);

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-ADDJOURNEY-OWNER-1",
                "TEST-GROUPS-ADDJOURNEY-STRANGER-1",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_journey_to_group_is_idempotent -- --ignored`"]
    async fn add_journey_to_group_is_idempotent() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-ADDJOURNEY-OWNER-2").await;
        let group_id = create_group(&pool, "Add Journey Test 2", "TEST-GROUPS-ADDJOURNEY-OWNER-2")
            .await
            .expect("create group");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-ADDJOURNEY-OWNER-2").await;

        assert!(
            add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-ADDJOURNEY-OWNER-2")
                .await
                .expect("first add")
        );
        assert!(
            add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-ADDJOURNEY-OWNER-2")
                .await
                .expect("second add is a no-op, not an error")
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_journeys WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count");
        assert_eq!(count.0, 1);

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-ADDJOURNEY-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_journey_from_group_allows_the_sharer_to_remove_their_own_journey \
                -- --ignored`"]
    async fn remove_journey_from_group_allows_the_sharer_to_remove_their_own_journey() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVEJOURNEY-1").await;
        let group_id =
            create_group(&pool, "Remove Journey Test 1", "TEST-GROUPS-REMOVEJOURNEY-1")
                .await
                .expect("create group");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-REMOVEJOURNEY-1").await;
        add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-REMOVEJOURNEY-1")
            .await
            .expect("add");

        let removed = remove_journey_from_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-GROUPS-REMOVEJOURNEY-1",
            false, // plain member, but they ARE the sharer
        )
        .await
        .expect("remove");
        assert!(removed);

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-REMOVEJOURNEY-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_journey_from_group_denies_a_plain_member_removing_someone_elses_journey \
                -- --ignored`"]
    async fn remove_journey_from_group_denies_a_plain_member_removing_someone_elses_journey() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVEJOURNEY-OWNER-2").await;
        seed_user(&pool, "TEST-GROUPS-REMOVEJOURNEY-OTHER-2").await;
        let group_id =
            create_group(&pool, "Remove Journey Test 2", "TEST-GROUPS-REMOVEJOURNEY-OWNER-2")
                .await
                .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-REMOVEJOURNEY-OTHER-2")
        .execute(&pool)
        .await
        .expect("seed member");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-REMOVEJOURNEY-OWNER-2").await;
        add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-REMOVEJOURNEY-OWNER-2")
            .await
            .expect("add");

        let removed = remove_journey_from_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-GROUPS-REMOVEJOURNEY-OTHER-2", // did not share it, not a manager
            false,
        )
        .await
        .expect("attempt remove");
        assert!(!removed);

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-REMOVEJOURNEY-OWNER-2",
                "TEST-GROUPS-REMOVEJOURNEY-OTHER-2",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_group_journeys_returns_shared_journeys_with_attribution -- --ignored`"]
    async fn list_group_journeys_returns_shared_journeys_with_attribution() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-LISTJOURNEY-1").await;
        let group_id = create_group(&pool, "List Journey Test 1", "TEST-GROUPS-LISTJOURNEY-1")
            .await
            .expect("create group");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-LISTJOURNEY-1").await;
        add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-LISTJOURNEY-1")
            .await
            .expect("add");

        let journeys = list_group_journeys(&pool, &group_id)
            .await
            .expect("list group journeys");
        assert_eq!(journeys.len(), 1);
        assert_eq!(journeys[0].journey_id, journey_id);
        assert_eq!(journeys[0].leg_count, 1);
        assert_eq!(journeys[0].added_by, "TEST-GROUPS-LISTJOURNEY-1");

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-LISTJOURNEY-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_deletes_a_departed_members_shared_journeys_in_the_same_transaction \
                -- --ignored`"]
    async fn remove_member_deletes_a_departed_members_shared_journeys_in_the_same_transaction() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVE-OWNER-J1").await;
        seed_user(&pool, "TEST-GROUPS-REMOVE-MEMBER-J1").await;
        let group_id =
            create_group(&pool, "Remove Journey Cleanup Test", "TEST-GROUPS-REMOVE-OWNER-J1")
                .await
                .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-REMOVE-MEMBER-J1")
        .execute(&pool)
        .await
        .expect("seed member");
        let journey_id = seed_journey(&pool, "TEST-GROUPS-REMOVE-MEMBER-J1").await;
        add_journey_to_group(&pool, &group_id, journey_id, "TEST-GROUPS-REMOVE-MEMBER-J1")
            .await
            .expect("share the journey into the group");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-REMOVE-MEMBER-J1")
            .await
            .expect("remove member");
        assert_eq!(outcome, RemoveMemberOutcome::Removed { new_owner: None });

        let remaining_shared: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_journeys WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count group_journeys");
        assert_eq!(
            remaining_shared.0, 0,
            "the departed member's shared journey should be pulled"
        );

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &["TEST-GROUPS-REMOVE-OWNER-J1", "TEST-GROUPS-REMOVE-MEMBER-J1"],
        )
        .await;
    }
```

- [ ] **Step 8: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api -- --ignored --test-threads=1 2>&1 | grep -E "journey|FAILED"
```

  Expected: builds clean; every new `#[ignore]`d test above passes against
  a real Postgres that already has Phase 1's `journeys`/`journey_legs`
  tables.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/data/groups.rs
git commit -m "api: add group_journeys data-layer CRUD, mirroring group_trains"
```

---

## Task 3: Routes — `crates/api/src/routes/groups.rs`

**Files:** modify `crates/api/src/routes/groups.rs`.

Depends on Task 2. Mirrors the `group_trains` route block
(`routes/groups.rs:94-101`, `583-681`) line-for-line, plus the
`/groups/shared-trains` literal-route-precedence pattern
(`routes/groups.rs:80-93`, `598-618`, and its own dedicated precedence test
at `routes/groups.rs:894-921`).

- [ ] **Step 1: Mount the three new routes** in `router()`
  (`routes/groups.rs:31-118`), as siblings of the existing train routes —
  the literal `/groups/shared-journeys` segment must be registered ahead of
  `/groups/{id}` for the same matchit-precedence reason `/groups/
  shared-trains` and `/groups/shared-custom-lines` already are (see the
  existing route comments immediately above each):

```rust
        // A fourth literal segment at `/groups/{id}`'s dynamic position,
        // resolved ahead of it by the same matchit precedence
        // `/groups/shared-trains` and `/groups/shared-custom-lines` above
        // already rely on (and which those files' own precedence tests
        // pin). Group ids are 32 random base64url bytes, so no real group
        // can ever be shadowed by this path.
        .route(
            "/groups/shared-journeys",
            axum::routing::get(list_shared_journeys_route),
        )
        .route(
            "/groups/{id}/trains",
            axum::routing::get(list_group_trains_route).post(add_group_train),
        )
        .route(
            "/groups/{id}/trains/{train_subscription_id}",
            axum::routing::delete(remove_group_train),
        )
        .route(
            "/groups/{id}/journeys",
            axum::routing::get(list_group_journeys_route).post(add_group_journey),
        )
        .route(
            "/groups/{id}/journeys/{journey_id}",
            axum::routing::delete(remove_group_journey),
        )
```

- [ ] **Step 2: Add the four handlers**, directly below their
  `group_trains` counterparts:

```rust
/// `GET /groups/{id}/journeys` -- every journey shared into this group.
/// Mirrors `list_group_trains_route` exactly.
async fn list_group_journeys_route(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<groups::GroupJourney>>, (StatusCode, String)> {
    require_member(&app, &group_id, &user.id).await?;

    let journeys = groups::list_group_journeys(&app.database, &group_id)
        .await
        .map_err(internal_error("list group journeys"))?;
    Ok(Json(journeys))
}

/// `GET /groups/shared-journeys` -- every journey shared into ANY group
/// the caller belongs to, minus the ones they own themselves. Mirrors
/// `list_shared_trains_route` exactly, including its "no group id in the
/// path, no `require_member` gate" reasoning: the caller's own membership
/// rows ARE the scope of `groups::list_shared_journeys_for_user`'s query.
/// Not consumed by any frontend page in this phase -- see this feature's
/// plan, Judgment Call 4 -- built for parity with `group_trains`'s own
/// route set per spec §6.
async fn list_shared_journeys_route(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<groups::SharedJourney>>, (StatusCode, String)> {
    let journeys = groups::list_shared_journeys_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list shared journeys"))?;
    Ok(Json(journeys))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddGroupJourneyRequest {
    journey_id: i64,
}

/// Any current member may add one of their OWN journeys (spec §6);
/// `groups::add_journey_to_group`'s own ownership check is what actually
/// enforces "their own" -- this handler only checks group membership.
/// Mirrors `add_group_train` exactly.
async fn add_group_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
    Json(req): Json<AddGroupJourneyRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_member(&app, &group_id, &user.id).await?;

    let added = groups::add_journey_to_group(&app.database, &group_id, req.journey_id, &user.id)
        .await
        .map_err(internal_error("add journey to group"))?;
    if !added {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /groups/{id}/journeys/{journeyId}` -- the sharer, or any
/// `admin`/`owner`, may remove a shared journey. Mirrors
/// `remove_group_train` exactly.
async fn remove_group_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((group_id, journey_id)): Path<(String, i64)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let role = require_member(&app, &group_id, &user.id).await?;

    let removed = groups::remove_journey_from_group(
        &app.database,
        &group_id,
        journey_id,
        &user.id,
        role.can_manage(),
    )
    .await
    .map_err(internal_error("remove journey from group"))?;
    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            "no shared journey with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 3: Add the literal-route-precedence test**, in the existing
  `#[cfg(test)] mod tests` block (`routes/groups.rs:813-956`), mirroring
  `shared_custom_lines_literal_route_wins_over_same_position_dynamic_id_route`
  exactly:

```rust
    /// The same precedence check for `/groups/shared-journeys`, the fourth
    /// literal segment this file registers at `/groups/{id}`'s dynamic
    /// position. Same hand-rolled two-route shape as its three siblings
    /// above, for the same reason.
    #[tokio::test]
    async fn shared_journeys_literal_route_wins_over_same_position_dynamic_id_route() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route(
                "/groups/shared-journeys",
                axum::routing::get(|| async { "shared-journeys" }),
            )
            .route("/groups/{id}", axum::routing::get(|| async { "dynamic" }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/groups/shared-journeys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"shared-journeys");
    }
```

- [ ] **Step 4: Add HTTP-layer `db_tests`**, in the existing `mod db_tests`
  block (`routes/groups.rs:978-`), reusing that module's own
  `seed_membership`/`seed_train_subscription`-style helpers — add a
  `seed_journey` helper of the same shape as Task 2's, and one route-level
  test confirming the ownership 404 end to end:

```rust
    async fn seed_journey(pool: &PgPool, user_id: &str) -> i64 {
        let journey_id: (i64,) = sqlx::query_as(
            "INSERT INTO journeys (user_id, custom_name) VALUES ($1, NULL) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("seed a journey");
        sqlx::query(
            "INSERT INTO journey_legs \
                (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             VALUES ($1, 1, 'WOK', 'WAT', CURRENT_DATE, 'manual')",
        )
        .bind(journey_id.0)
        .execute(pool)
        .await
        .expect("seed a journey leg");
        journey_id.0
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_group_journey_a_caller_who_does_not_own_the_journey_gets_404 -- --ignored \
                --test-threads=1`"]
    async fn add_group_journey_a_caller_who_does_not_own_the_journey_gets_404() {
        let pool = connect().await;
        let member_token = seed_session(&pool, "TEST-ROUTE-GROUPS-ADDJOURNEY-MEMBER").await;
        seed_session(&pool, "TEST-ROUTE-GROUPS-ADDJOURNEY-STRANGER").await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Test Add Journey",
            "TEST-ROUTE-GROUPS-ADDJOURNEY-MEMBER",
        )
        .await
        .expect("create fixture group");
        let journey_id = seed_journey(&pool, "TEST-ROUTE-GROUPS-ADDJOURNEY-STRANGER").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = post_json(
            router,
            format!("/groups/{group_id}/journeys"),
            Some(&member_token),
            Some(serde_json::json!({ "journeyId": journey_id })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no journey with that id".to_string()));

        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GROUPS-ADDJOURNEY-MEMBER",
                "TEST-ROUTE-GROUPS-ADDJOURNEY-STRANGER",
            ],
        )
        .await;
    }
```

- [ ] **Step 5: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api router_builds_without_panicking shared_journeys_literal_route_wins
cargo test -p api -- --ignored --test-threads=1 2>&1 | grep -E "journey|FAILED"
```

  Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/groups.rs
git commit -m "api: add /groups/{id}/journeys and /groups/shared-journeys routes"
```

---

## Task 4: Cross-owner read authorization (the genuinely new piece)

**Files:** modify `crates/api/src/data/journeys.rs`,
`crates/api/src/routes/journeys.rs` (both Phase-1-created).

Depends on Task 1 (`group_journeys`/`group_members` must exist for the new
query) AND on Phase 1 having actually landed with a real
`GET /Journeys/{journeyId}` handler to adapt. **This is the one task in
this plan that is not a mirror of an existing route/function — it is new
authorization logic, and per spec §6's own instruction ("call this out
explicitly during implementation review since it's the one place this
feature's sharing model isn't a pure copy-paste"), treat it with
correspondingly more scrutiny than Tasks 1-3.**

- [ ] **Step 0: Read Phase 1's actual `GET /Journeys/{journeyId}` handler
  and confirm which of the two shapes described in "Assumptions about
  Phase 1" (above) it actually uses**, before writing any code below:

  - **Shape A (assumed default — separate gate + unscoped fetch, mirroring
    `tracked_train_owner` + `get_by_tracking_id`)**: the handler calls
    something like `journeys::journey_owner(pool, journey_id).await? ==
    Some(user.id)` as its entire authorization check, then separately calls
    a detail-fetch function that takes only `journey_id` (no `user_id`
    parameter). If this is what Phase 1 actually did, proceed with Steps
    1-3 below unchanged.
  - **Shape B (combined ownership+fetch, mirroring
    `groups::get_group_detail`)**: the handler runs one query that both
    authorizes and fetches in the same `SELECT ... WHERE j.id = $1 AND
    j.user_id = $2 ...`. If this is what Phase 1 actually did, Steps 1-2
    below (adding `journey_readable_by`) are unchanged, but Step 3 instead
    widens that single query's `WHERE` clause directly: replace `j.user_id
    = $2` with `(j.user_id = $2 OR EXISTS (SELECT 1 FROM group_journeys gj
    JOIN group_members gm ON gm.group_id = gj.group_id AND gm.user_id = $2
    WHERE gj.journey_id = j.id))`, and audit every join inside that same
    query for any OTHER place it filters on `user_id` (a per-leg
    `train_subscriptions.user_id = $2` clause, if one exists, must be
    removed — see this task's own guardrail below) rather than adding a
    second query.

  Whichever shape is real, the two hard invariants below must hold; if
  Phase 1's actual code makes either one impossible to satisfy cleanly,
  stop and flag it rather than working around it, since violating either
  is exactly the mistake spec §6 is warning about:
  1. The widened check must be **read-only**, reachable only from `GET
     /Journeys/{journeyId}`, never from any write route.
  2. Once the journey-level check passes, the response-building code must
     **not** additionally filter any leg's `train_subscriptions` row by
     `user_id` — the journey-level check is the sole authority for this
     read, per spec §6's explicit instruction ("reading every leg's
     `train_subscriptions` row **without** re-checking that row's own
     ownership").

- [ ] **Step 1: Add `journey_readable_by`** to
  `crates/api/src/data/journeys.rs`, modeled on
  `custom_lines::readable_custom_line_ids`'s OR-clause shape (the closest
  existing precedent for widening a private resource's read gate to group
  membership — see
  `docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md`
  §3.2), but single-id rather than batched (Judgment Call 5 — no bulk
  journey-status list exists yet to justify a `HashSet`-returning version):

```rust
/// Whether `user_id` may read journey `journey_id`'s full detail: either
/// they own it outright, or it's been shared (`group_journeys`, see
/// `crates/api/src/data/groups.rs`) into at least one group they're
/// currently a member of. Mirrors `custom_lines::readable_custom_line_ids`'s
/// "owned OR granted-into-a-group-I'm-in" shape (see
/// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
/// §3.2) -- the closest existing precedent for widening a private
/// resource's read gate to group sharing -- but returns a single `bool`
/// rather than a batched id set: this function has exactly one call site
/// (`GET /Journeys/{journeyId}`, a single-id read), and no bulk
/// journey-status list exists in this codebase to justify the extra
/// complexity a `HashSet<i64>`-returning, `ANY($1)`-parameterized version
/// would add for zero current callers.
///
/// READ-ONLY AUTHORIZATION ONLY. This function must NEVER be used to gate
/// a write route (rename/add-leg/commit-leg/delete a journey, or anything
/// under `/Journeys/*` that mutates state) -- every write stays scoped to
/// `journeys.user_id = caller.id` alone, via `journey_owner` (this file's
/// existing ownership-only check, unchanged by this feature). Sharing a
/// journey into a group conveys READ access ONLY, per
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §6 -- the
/// same hard boundary `custom_line_group_grants`/`group_trains` already
/// enforce for their own resources ("no group role can edit or delete
/// someone else's shared resource, only unshare it").
pub async fn journey_readable_by(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    let (readable,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM journeys j WHERE j.id = $1 AND j.user_id = $2) \
         OR EXISTS ( \
             SELECT 1 FROM group_journeys gj \
             JOIN group_members gm ON gm.group_id = gj.group_id AND gm.user_id = $2 \
             WHERE gj.journey_id = $1 \
         )",
    )
    .bind(journey_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(readable)
}
```

- [ ] **Step 2: Add DB-gated tests for `journey_readable_by`**, in
  `journeys.rs`'s own `db_tests` module (creating one, mirroring
  `groups.rs`'s `seed_user`/`connect`/`cleanup` helpers verbatim, if Phase
  1 hasn't already established one — reuse Phase 1's fixtures/helpers if
  they already exist rather than duplicating):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_the_owner_can_always_read_their_own_journey_with_no_grant() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-1").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-1").await;

        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-OWNER-1")
                .await
                .expect("check readability")
        );

        cleanup_journey(&pool, journey_id, &["TEST-JOURNEY-READABLE-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_a_fellow_group_member_can_read_a_shared_journey() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-2").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-MEMBER-2").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-2").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Journey Readable Test",
            "TEST-JOURNEY-READABLE-OWNER-2",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-JOURNEY-READABLE-MEMBER-2")
        .execute(&pool)
        .await
        .expect("seed member");
        crate::data::groups::add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-JOURNEY-READABLE-OWNER-2",
        )
        .await
        .expect("share journey");

        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-2")
                .await
                .expect("check readability")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &["TEST-JOURNEY-READABLE-OWNER-2", "TEST-JOURNEY-READABLE-MEMBER-2"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_excludes_a_stranger_in_no_shared_group() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-3").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-STRANGER-3").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-3").await;

        // The stranger is a member of SOME group, just not one this
        // journey was shared into -- the core negative case, protecting
        // against a query that accidentally checks "is a member of any
        // group" instead of "is a member of a group THIS journey was
        // shared into".
        let unrelated_group_id = crate::data::groups::create_group(
            &pool,
            "Unrelated Group",
            "TEST-JOURNEY-READABLE-STRANGER-3",
        )
        .await
        .expect("create unrelated group");

        assert!(
            !journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-STRANGER-3")
                .await
                .expect("check readability")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&unrelated_group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &["TEST-JOURNEY-READABLE-OWNER-3", "TEST-JOURNEY-READABLE-STRANGER-3"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_a_former_member_loses_access_once_removed() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-4").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-MEMBER-4").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-4").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Journey Readable Departure Test",
            "TEST-JOURNEY-READABLE-OWNER-4",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-JOURNEY-READABLE-MEMBER-4")
        .execute(&pool)
        .await
        .expect("seed member");
        crate::data::groups::add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-JOURNEY-READABLE-OWNER-4",
        )
        .await
        .expect("share journey");
        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-4")
                .await
                .expect("readable while a member")
        );

        crate::data::groups::remove_member(&pool, &group_id, "TEST-JOURNEY-READABLE-MEMBER-4")
            .await
            .expect("remove member");

        // Task 2's remove_member cascade should have deleted the
        // group_journeys row too, so this is doubly protected -- even a
        // query that only checked group_members (and not group_journeys)
        // would already deny this, but the point of this test is
        // end-to-end: departure really does revoke read access.
        assert!(
            !journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-4")
                .await
                .expect("no longer readable after leaving")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &["TEST-JOURNEY-READABLE-OWNER-4", "TEST-JOURNEY-READABLE-MEMBER-4"],
        )
        .await;
    }
```

  (`seed_journey`/`cleanup_journey` here are this file's own fixture
  helpers — reuse Phase 1's if they already exist under different names,
  matching whatever convention Phase 1 established rather than introducing
  a second one.)

- [ ] **Step 3: Wire `journey_readable_by` into the `GET
  /Journeys/{journeyId}` handler** in `crates/api/src/routes/journeys.rs`,
  per whichever shape Step 0 confirmed. For the assumed default (Shape A):

```rust
    // BEFORE (Phase 1, ownership-only):
    // match journeys::journey_owner(&app.database, journey_id).await... {
    //     Some(owner) if owner == user.id => {}
    //     _ => return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string())),
    // }

    // AFTER: widened to ownership-OR-group-membership. journey_owner
    // itself is UNCHANGED and still used, unmodified, by every write route
    // under /Journeys/* -- only this one GET handler's check changes.
    let readable = journeys::journey_readable_by(&app.database, journey_id, &user.id)
        .await
        .map_err(internal_error("check journey readability"))?;
    if !readable {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey with that id".to_string(),
        ));
    }
```

  The detail-fetch call immediately below this (Phase 1's existing
  `journeys::get_journey_detail(&app.database, journey_id)` or equivalent)
  is otherwise **unchanged** — per this task's own guardrail, it must not
  itself filter by `user_id` anywhere in its leg-joining query, which is
  exactly what makes "read every leg's `train_subscriptions` row without
  re-checking that row's own ownership" true by construction rather than
  by a second, easy-to-forget exemption.

  Add a doc comment on the handler itself recording why this gate differs
  from every other `/Journeys/*` route:

```rust
/// `GET /Journeys/{journeyId}` -- the ONE read route in this file gated on
/// `journeys::journey_readable_by` (owner OR group-shared-with) rather
/// than `journeys::journey_owner` (owner only, used by every write route
/// in this file). See
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §6's final
/// paragraph and
/// docs/superpowers/plans/2026-09-22-journey-tracking-phase4-group-sharing-plan.md's
/// Task 4: this is the one place in the whole /Journeys/* surface where a
/// caller who does not own the resource can still read it, and it must
/// stay that way -- deliberately -- while every other handler in this
/// file keeps the ownership-only gate.
```

- [ ] **Step 4: Add a route-level `db_tests` case** proving the widening
  end to end through the real HTTP handler (in `routes/journeys.rs`'s own
  `db_tests`, following whatever `test_router`/`seed_session`/`request`
  helper shape Phase 1 already established there, mirroring
  `crates/api/src/routes/groups.rs`'s own `db_tests` conventions if
  Phase 1 built its own from scratch):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized \
                -- --ignored --test-threads=1`"]
    async fn get_journey_a_group_member_can_read_a_shared_journey_the_owner_never_authorized() {
        // Seed an owner + journey, a second user who is a fellow group
        // member (never given any train_subscriptions-level ownership of
        // the leg's underlying row), share the journey into a group both
        // are in, and confirm GET /Journeys/{id} as the second user
        // returns 200 with the same leg detail the owner sees -- not 404.
        // This is the concrete, end-to-end proof of Task 4's whole point:
        // the second user was never checked against
        // train_subscriptions.user_id at all, and correctly doesn't need
        // to be.
        // ... (fixture/assert bodies follow this file's own established
        // db_tests helper conventions once Phase 1's real shape is known)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_journey_a_stranger_in_no_shared_group_still_gets_404 -- --ignored \
                --test-threads=1`"]
    async fn get_journey_a_stranger_in_no_shared_group_still_gets_404() {
        // The negative case: a caller with no ownership and no group-share
        // relationship to this journey at all gets the ordinary 404,
        // identical to today's (pre-Phase-4) behavior.
    }
```

  (Left as a sketch, not fully filled in, because it depends on the real
  `db_tests` scaffolding Phase 1 wrote for this file, which does not exist
  yet at plan-writing time — fill in the fixture/assert bodies against
  Phase 1's actual `test_router`/`seed_session`/`request` helpers once
  they exist, following the same shape as this task's Step 2 tests and
  `routes/groups.rs`'s own `db_tests` precedent.)

- [ ] **Step 5: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api -- --ignored --test-threads=1 2>&1 | grep -E "journey_readable|get_journey.*group|FAILED"
```

  Expected: all pass, and — this is the important negative check —
  re-running every existing write-route test under `/Journeys/*` that
  Phase 1/2/3 already wrote (`cargo test -p api -- --ignored
  --test-threads=1 2>&1 | grep -E "journeys::|rename_journey|delete_journey|commit_leg"`,
  adjusting the grep to Phase 1's real test names) still passes unchanged —
  proof this task did not accidentally widen any write path.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/journeys.rs crates/api/src/routes/journeys.rs
git commit -m "api: allow a group member to read a journey shared into their group"
```

---

## Task 5: Frontend types + `lib/api.ts`

**Files:** modify `frontend/lib/types.ts`, `frontend/lib/api.ts`.

Depends on Task 3's routes existing (for the response shapes to mirror
accurately) — can be written in parallel with Task 4 once Task 3 is done.

- [ ] **Step 1: Add `GroupJourney`/`SharedGroupJourney`** to
  `frontend/lib/types.ts`, directly below `SharedGroupCustomLine`
  (`lib/types.ts:1001-1004`), mirroring `GroupTrain`/`SharedGroupTrain`
  (`lib/types.ts:928-967`) field-for-field against Task 2's Rust structs:

```typescript
/** A journey shared into a group -- `crates/api/src/data/groups.rs`'s
 * `GroupJourney`. Carries the journey's own identity plus its FIRST leg's
 * identity/live-status fields (not a full multi-leg rollup -- see this
 * feature's plan, Judgment Call 2) and a `legCount` so a multi-leg journey
 * at least signals "there's more". Same "never shown" privacy constraint
 * `GroupTrain` documents: no ticket field, no notification state, no
 * exact `addedAt`. */
export interface GroupJourney {
  journeyId: number;
  customName: string | null;
  legCount: number;
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  serviceDate: string; // "YYYY-MM-DD"
  resolutionStatus: string | null;
  trainUid: string | null;
  status: string | null;
  delayMinutes: number | null;
  addedBy: string;
  /** Same contract as `GroupMember.displayName`: the sharer's own name, or
   * `null` -- never their email address. */
  addedByName: string | null;
  /** Same contract as `GroupMember.displayTag`, for the sharer. */
  addedByTag: string | null;
}

/** `GET /public/groups/shared-journeys`'s per-item shape
 * (`crates/api/src/data/groups.rs`'s `SharedJourney`): a `GroupJourney`
 * plus the group it was shared into. Not consumed by any page in this
 * phase (see this feature's plan, Judgment Call 4) -- kept for parity with
 * `SharedGroupTrain`. */
export interface SharedGroupJourney extends GroupJourney {
  groupId: string;
  groupName: string;
}
```

- [ ] **Step 2: Add `getGroupJourneys`/`getSharedGroupJourneys`** to
  `frontend/lib/api.ts`, directly below `getSharedGroupCustomLines`
  (`lib/api.ts:679-687`), mirroring `getGroupTrains`/`getSharedGroupTrains`
  exactly:

```typescript
import type {
  // ...existing imports...
  GroupJourney,
  SharedGroupJourney,
} from './types';

/** `GET /public/groups/{id}/journeys` -- the journeys shared into this
 * group. Any current member may read it; a non-member gets the group's
 * usual `404`. Throws on a `401`, like `getGroupTrains`/`getGroupCustomLines`
 * and for the same reason (there is an id in the path). */
export async function getGroupJourneys(id: string): Promise<GroupJourney[]> {
  const url = `${baseUrl()}/public/groups/${id}/journeys`;
  return fetchJson<GroupJourney[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

/** `GET /public/groups/shared-journeys` -- every journey OTHER members
 * have shared into any group the caller belongs to (never the caller's
 * own). Not called by any page yet -- see this feature's plan, Judgment
 * Call 4 -- built now for parity with `getSharedGroupTrains`/
 * `getSharedGroupCustomLines`. `null` on a `401`, same reasoning as those
 * two: no id in the path, so a `401` can only ever mean "not logged in". */
export async function getSharedGroupJourneys(): Promise<SharedGroupJourney[] | null> {
  const url = `${baseUrl()}/public/groups/shared-journeys`;
  const response = await fetch(url, { cache: 'no-store', ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<SharedGroupJourney[]>;
}
```

- [ ] **Step 3: Verify**

```bash
npm run build --prefix /home/coder/Distant-Signal/frontend
```

  Expected: type-checks clean (both new functions are unused by anything
  yet — Tasks 6-7 wire in the consumer — so this step is purely a
  compile/type check).

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts
git commit -m "frontend: add GroupJourney/SharedGroupJourney types and fetchers"
```

---

## Task 6: Frontend components

**Files:** create `frontend/components/ShareJourneyButton.tsx`,
`frontend/components/ShareJourneyButton.test.tsx`,
`frontend/components/AddJourneyToGroupButton.tsx`,
`frontend/components/AddJourneyToGroupButton.test.tsx`,
`frontend/components/RemoveGroupJourneyButton.tsx`.

Depends on Task 5. Three components, each a close mirror of an existing
one — cited by name so the diff is checkable.

- [ ] **Step 1: `AddJourneyToGroupButton.tsx`** — mirrors
  `AddTrainToGroupButton.tsx` exactly: fixed `groupId`, fetches the
  caller's own journeys, excludes ones already shared, POSTs the picked one.

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Select, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
// Adjust this import to whatever Phase 1 actually names its "my journeys"
// list item type and display-name helper -- mirrors
// AddTrainToGroupButton.tsx's own `TrackedTrainListItem`/
// `trackedTrainDisplayName` import exactly, one level up.
import { journeyDisplayName } from '@/lib/journeyName';
import type { JourneyListItem } from '@/lib/types';

/** Picker sourced from the user's own `/Journeys/mine` list -- the direct
 * analogue of `AddTrainToGroupButton.tsx`, one level up (journeys instead
 * of tracked trains). Fetched lazily on open via the same-origin
 * `/api/Journeys/mine` proxy, mirroring that component's own
 * `/api/Train/mine` fetch. `excludeJourneyIds` hides journeys already
 * shared into this group -- re-adding one is harmless
 * (`groups::add_journey_to_group` is idempotent) but offering it again in
 * the picker would be confusing. */
export function AddJourneyToGroupButton({
  groupId,
  excludeJourneyIds,
}: {
  groupId: string;
  excludeJourneyIds: number[];
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [loading, setLoading] = useState(false);
  const [journeys, setJourneys] = useState<JourneyListItem[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleOpen() {
    setError(null);
    setSelected(null);
    open();
    setLoading(true);
    try {
      const response = await fetch('/api/Journeys/mine');
      if (!response.ok) {
        setError('Could not load your journeys.');
        setLoading(false);
        return;
      }
      const all: JourneyListItem[] = await response.json();
      setJourneys(all.filter((j) => !excludeJourneyIds.includes(j.id)));
      setLoading(false);
    } catch {
      setError('Could not load your journeys.');
      setLoading(false);
    }
  }

  async function handleAdd() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/journeys`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ journeyId: Number(selected) }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Add one of my journeys
      </Button>
      <Modal opened={opened} onClose={close} title="Share a journey with this group">
        {loading && <Text c="dimmed">Loading your journeys…</Text>}
        {!loading && journeys !== null && journeys.length === 0 && (
          <Text c="dimmed">Every journey you have is already shared into this group.</Text>
        )}
        {!loading && journeys !== null && journeys.length > 0 && (
          <Select
            label="Journey"
            placeholder="Pick one"
            data={journeys.map((j) => ({ value: String(j.id), label: journeyDisplayName(j) }))}
            value={selected}
            onChange={setSelected}
          />
        )}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share a journey</LoginLink>}
        <Button mt="md" onClick={handleAdd} disabled={!selected} loading={submitting}>
          Add to group
        </Button>
      </Modal>
    </>
  );
}
```

  **Adapt the two imports flagged in comments** (`journeyDisplayName`,
  `JourneyListItem`) once Phase 1's real names are known — this plan does
  not invent Phase 1's own display-name helper or list-item type, only
  assumes something analogous to `trackedTrainDisplayName`/
  `TrackedTrainListItem` exists per the spec's §0.2/§4 description of the
  journeys list/detail views.

- [ ] **Step 2: `RemoveGroupJourneyButton.tsx`** — mirrors
  `RemoveGroupTrainButton.tsx` verbatim, swapping the endpoint path and
  prop name:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Removes a shared journey from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which via
 * `groups::remove_journey_from_group`'s sharer-or-manager check). Mirrors
 * `RemoveGroupTrainButton.tsx` verbatim -- see that component's own doc
 * comment for the full reasoning, which applies here unchanged. */
export function RemoveGroupJourneyButton({ groupId, journeyId }: { groupId: string; journeyId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleRemove() {
    setRemoving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/journeys/${journeyId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setRemoving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setRemoving(false);
    }
  }

  return (
    <>
      <Button variant="subtle" color="red" size="xs" onClick={open}>
        Remove from group
      </Button>
      <Modal opened={opened} onClose={close} title="Remove this journey from the group?">
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this journey</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove journey from group">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

- [ ] **Step 3: `ShareJourneyButton.tsx`** — the "journey share button" the
  task brief calls for, mirroring `AddToGroupButton.tsx` exactly: fixed
  `journeyId`, picks a group, sourced from `useGroupSummaries()` (never
  renders when the caller is in zero groups):

```tsx
'use client';

import { useState } from 'react';
import { Alert, Button, Modal, Select } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { useGroupSummaries } from '@/lib/useGroupSummaries';

/** The journey-detail-page share control -- the direct analogue of
 * `AddToGroupButton.tsx` (tracked trains), one level up: starts from a
 * fixed `journeyId` and picks a `groupId` from the viewer's own groups,
 * rather than the other direction `AddJourneyToGroupButton.tsx` takes.
 * Lets an already-created journey be shared into a group after the fact
 * (including a second one) from the journey's own detail page, per spec
 * §4's "a share-to-group button" in the journey view header.
 *
 * Sourced from `useGroupSummaries()`, same as `AddToGroupButton.tsx` --
 * renders nothing at all for a viewer in zero groups (an anonymous
 * visitor, a genuinely group-less user, or a failed fetch, all treated
 * identically), so it adds no visible clutter for the common case.
 *
 * The POST here is NOT best-effort (unlike whatever journey-creation-time
 * group-share flow Phase 1's `TrackTrainForm`-equivalent may have) --
 * sharing IS the entire point of this click, so a failure must be shown,
 * matching `AddToGroupButton.tsx`'s own not-best-effort posture for its
 * own (also after-the-fact) share action. */
export function ShareJourneyButton({ journeyId }: { journeyId: number }) {
  const { groups } = useGroupSummaries();
  const [opened, { open, close }] = useDisclosure(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addedGroupName, setAddedGroupName] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  if (groups.length === 0) {
    return null;
  }

  function handleOpen() {
    setSelected(null);
    setError(null);
    setAddedGroupName(null);
    open();
  }

  async function handleShare() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    setAddedGroupName(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${selected}/journeys`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ journeyId }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      const group = groups.find((g) => g.id === selected);
      setAddedGroupName(group?.name ?? 'the group');
      setSelected(null);
      setSubmitting(false);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Share with a group
      </Button>
      <Modal opened={opened} onClose={close} title="Share this journey with a group">
        <Select
          label="Group"
          placeholder="Pick one"
          data={groups.map((group) => ({ value: group.id, label: group.name }))}
          value={selected}
          onChange={setSelected}
        />
        {addedGroupName && <Alert color="green">Added to {addedGroupName}.</Alert>}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share this journey</LoginLink>}
        <Button mt="md" onClick={handleShare} disabled={!selected} loading={submitting}>
          Share
        </Button>
      </Modal>
    </>
  );
}
```

  **Naming note**: `AddToGroupButton.tsx`'s button label is "Add to
  group"; this plan gives `ShareJourneyButton` the label "Share with a
  group" instead, matching the modal title's own "Share this journey..."
  wording and the task brief's own phrase "a journey share button" — a
  small, deliberate copy divergence from its mirrored precedent, not an
  oversight, since a journey (unlike a single tracked train, which is
  already reached via "Add to group" from `TrackedTrainOwnerControls`'
  three-button row) has this as its own standalone header action rather
  than one of a same-styled trio, so slightly more explicit wording earns
  its keep here.

- [ ] **Step 4: Component tests**, mirroring
  `AddTrainToGroupButton.test.tsx` (fetch-mock + `renderWithMantine` +
  `vi.mock('next/navigation', ...)` shape) for
  `AddJourneyToGroupButton.test.tsx`, and the equivalent shape from
  `AddToGroupButton.test.tsx` (not read in full during this plan's
  research, but confirmed to exist at `frontend/components/AddToGroupButton.test.tsx`
  — read it before writing `ShareJourneyButton.test.tsx` to mirror its
  `useGroupSummaries` mocking approach exactly) for `ShareJourneyButton.test.tsx`.
  At minimum, cover:
  - `AddJourneyToGroupButton`: fetches `/api/Journeys/mine` on open and
    excludes already-shared journeys (mirroring
    `AddTrainToGroupButton.test.tsx`'s first test); POSTs the chosen
    `journeyId` and refreshes (mirroring its second test).
  - `ShareJourneyButton`: renders nothing when `useGroupSummaries()`
    returns zero groups; POSTs the chosen `groupId` with the fixed
    `journeyId` and shows the "Added to {group}." confirmation without
    closing the modal (mirroring `AddToGroupButton`'s own stay-open
    behavior).

- [ ] **Step 5: Verify**

```bash
npm test -- ShareJourneyButton --prefix /home/coder/Distant-Signal/frontend
npm test -- AddJourneyToGroupButton --prefix /home/coder/Distant-Signal/frontend
npm run build --prefix /home/coder/Distant-Signal/frontend
```

  Expected: all pass; `RemoveGroupJourneyButton` has no dedicated test file
  in this task (matching `RemoveGroupTrainButton.tsx`, which also has
  none — its behavior is exercised indirectly through Task 7's group page
  wiring).

- [ ] **Step 6: Commit**

```bash
git add frontend/components/ShareJourneyButton.tsx frontend/components/ShareJourneyButton.test.tsx \
        frontend/components/AddJourneyToGroupButton.tsx frontend/components/AddJourneyToGroupButton.test.tsx \
        frontend/components/RemoveGroupJourneyButton.tsx
git commit -m "frontend: add ShareJourneyButton, AddJourneyToGroupButton, RemoveGroupJourneyButton"
```

---

## Task 7: Group detail page — "Shared journeys" section + `SharedJourneyRow`

**Files:** modify `frontend/app/groups/[id]/page.tsx`.

Depends on Task 5 (types/fetchers) and Task 6 (`AddJourneyToGroupButton`,
`RemoveGroupJourneyButton`). This is the task the brief names explicitly:
*"a `SharedJourneyRow` component in the group detail page mirroring the
existing `SharedTrainRow`."*

- [ ] **Step 1: Fetch journeys alongside the existing trains/custom
  lines**, in `GroupDetailPage`'s `Promise.all` (`page.tsx:82-87`):

```tsx
  const [members, trains, customLines, journeys, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getGroupCustomLines(id),
    getGroupJourneys(id),
    getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null })),
  ]);
```

  Add the import (`page.tsx:1-34`):

```tsx
import { AddJourneyToGroupButton } from '@/components/AddJourneyToGroupButton';
import { RemoveGroupJourneyButton } from '@/components/RemoveGroupJourneyButton';
```

  and add `getGroupJourneys` to the existing `@/lib/api` import list, and
  `GroupJourney` to the existing `@/lib/types` import list.

- [ ] **Step 2: Add a "Shared journeys" section**, directly below "Shared
  custom lines" (`page.tsx:230-263`, after that `Stack`'s closing tag,
  before the "Danger zone" block):

```tsx
      <Divider />

      {/* A fifth, separate section, mirroring "Shared trains"'s own shape
          exactly (unlike "Shared custom lines", which has a genuinely
          different add-permission story) -- sharing a journey has the
          identical "any member may share one of THEIR OWN" model
          group_trains already established. See the journey-tracking
          spec's §6. */}
      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2} size="h4">
            Shared journeys
          </Title>
          <AddJourneyToGroupButton
            groupId={id}
            excludeJourneyIds={journeys.map((j) => j.journeyId)}
          />
        </Group>
        {journeys.length === 0 ? (
          <Text c="dimmed">No journeys have been shared into this group yet.</Text>
        ) : (
          journeys.map((journey) => (
            <SharedJourneyRow
              key={journey.journeyId}
              groupId={id}
              journey={journey}
              canManage={canManage}
              currentUserId={currentUserId}
            />
          ))
        )}
      </Stack>
```

- [ ] **Step 3: Add `SharedJourneyRow`**, directly below `SharedTrainRow`
  (`page.tsx:348-404`), mirroring it field-for-field:

```tsx
/** One journey shared into this group -- mirrors `SharedTrainRow` exactly,
 * one level up (a journey's identity/status instead of a single train's).
 * `canRemove` mirrors `groups::remove_journey_from_group`'s own
 * sharer-or-manager check exactly, the same way `SharedTrainRow`'s does.
 * Links out to `/journeys/{journeyId}` for full detail -- reachable by a
 * non-owning group member because of Task 4's `journey_readable_by`
 * widening, the one piece of this feature that isn't a pure
 * `SharedTrainRow` copy (that link resolving to a real page, rather than a
 * 404, for a member who isn't the journey's owner, IS the whole point of
 * this feature's one new authorization path). */
function SharedJourneyRow({
  groupId,
  journey,
  canManage,
  currentUserId,
}: {
  groupId: string;
  journey: GroupJourney;
  canManage: boolean;
  currentUserId: string | null;
}) {
  const canRemove = canManage || (currentUserId !== null && journey.addedBy === currentUserId);
  // Falls back to a plain leg-count label when no custom name was set --
  // mirrors trackedTrainDisplayName's own "compute a sensible default from
  // whatever's on the row" posture, kept inline here since it's a single
  // conditional rather than a reusable multi-field default-name
  // computation like that helper's.
  const displayName =
    journey.customName ?? (journey.legCount === 1 ? 'Untitled journey' : `Untitled journey (${journey.legCount} legs)`);
  return (
    <Card withBorder>
      <StatusRow
        title={
          <Link href={`/journeys/${journey.journeyId}`} style={{ textDecoration: 'none', color: 'inherit' }}>
            <Text fw={500}>{displayName}</Text>
          </Link>
        }
        subtitle={
          <Text size="sm" c="dimmed">
            Shared by {memberLabel(journey.addedByName, journey.addedByTag, MEMBER_PLACEHOLDER_INLINE)}
          </Text>
        }
        trailing={
          <Group gap="xs" wrap="nowrap">
            <TrackedTrainStatusBadge train={journey} />
            {canRemove && (
              <RemoveGroupJourneyButton groupId={groupId} journeyId={journey.journeyId} />
            )}
          </Group>
        }
      />
    </Card>
  );
}
```

  **`<TrackedTrainStatusBadge train={journey} />` note**: check that
  component's own prop type (`frontend/components/TrackedTrainStatusBadge.tsx`,
  not read during this plan's research) — it's called with `train={train}`
  for a `GroupTrain` in `SharedTrainRow` today, and `GroupJourney` carries
  the same `status`/`delayMinutes`/`resolutionStatus` fields by
  construction (Task 2, Step 1), so it should accept a `GroupJourney` value
  structurally if its prop type is duck-typed on those fields already; if
  it's instead typed narrowly to `GroupTrain`, either widen its prop type
  to a shared structural interface both satisfy, or pass the three fields
  individually — a small adaptation to make at implementation time, not
  designed further here since it depends on that component's exact
  current signature.

- [ ] **Step 4: Verify**

```bash
npm run build --prefix /home/coder/Distant-Signal/frontend
```

  Expected: type-checks clean. Then, once Phase 1's `/journeys/{id}` page
  exists and a real backend is running with Task 4 merged, **manually
  verify in a browser** (per Global Constraints): share a journey from its
  owner's account into a group, open that group as a *different* member
  account, confirm the "Shared journeys" section shows the row, click its
  title, confirm `/journeys/{id}` renders the full detail (not a 404) for
  that non-owning member.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/groups/[id]/page.tsx
git commit -m "frontend: add Shared journeys section and SharedJourneyRow to the group detail page"
```

---

## Task 8: Journey detail page — mount `ShareJourneyButton`

**Files:** modify `frontend/app/journeys/[id]/page.tsx` (Phase-1-created) —
or, if Phase 1 factored its header controls into a dedicated component the
way `TrackedTrainOwnerControls.tsx` bundles the single-train equivalent,
modify that component instead.

Depends on Task 6 (`ShareJourneyButton`) and on Phase 1's journey detail
page/header-controls component existing. Per spec §4: *"Header: journey
`custom_name` (editable, same rename pattern as `RenameTrainButton`), a
share-to-group button (§6), an 'Add a leg' button (§3)."* — this task adds
only the "share-to-group button" piece; the rest of that header is Phase
1/2's own work.

- [ ] **Step 1: Locate Phase 1's journey detail page header** (or its
  owner-controls component, if factored out the way
  `TrackedTrainOwnerControls.tsx` was for the single-train case — check for
  a `JourneyOwnerControls.tsx` or equivalent first) and mount
  `<ShareJourneyButton journeyId={journey.id} />` alongside whatever
  rename/add-a-leg controls Phase 1 already placed there, following that
  file's own existing control-ordering convention (e.g., if it mirrors
  `TrackedTrainOwnerControls`'s Rename → Add-to-group → Delete ordering,
  place Share in the equivalent middle position).

- [ ] **Step 2: Verify**

```bash
npm run build --prefix /home/coder/Distant-Signal/frontend
```

  Then, in a real browser once the backend is running: open a journey you
  own, confirm the "Share with a group" button appears (and renders
  nothing if you're in zero groups — same posture as `AddToGroupButton`),
  click it, share into a group, confirm the "Added to {group}." success
  message, then repeat Task 7's cross-account verification from the other
  side (open the SAME journey's page as the sharer, confirmed working; the
  reverse direction — a fellow member reading it — was already verified in
  Task 7's own Step 4).

- [ ] **Step 3: Commit**

```bash
git add frontend/app/journeys/[id]/page.tsx
git commit -m "frontend: mount ShareJourneyButton on the journey detail page header"
```

---

## Summary of what this plan does NOT decide

Everything above is scoped to Phase 4. Two things worth restating plainly,
since they're easy to lose track of across eight tasks:

1. **The only genuinely new authorization logic in this entire plan is
   `journey_readable_by` (Task 4)** — a single function, one new call site
   (`GET /Journeys/{journeyId}`), read-only, never touching any write path.
   Everything else — the `group_journeys` table, its CRUD functions, its
   routes, and every frontend component — is a mechanical, field-for-field
   mirror of `group_trains`'s existing, already-shipped pattern.
2. **Nothing in this plan changes who may edit, delete, rename, add a leg
   to, or commit a candidate for a journey.** Every one of those stays
   gated on `journeys.user_id = caller.id` alone, exactly as Phase 1 built
   it — group sharing, per spec §6, conveys read access only, the same
   hard boundary `group_trains` and `custom_line_group_grants` already
   enforce for their own resources.
