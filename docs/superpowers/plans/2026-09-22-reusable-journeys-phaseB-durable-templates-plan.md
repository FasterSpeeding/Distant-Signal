# Plan: Reusable & Repeating Journeys — Phase B, Durable Journey Templates

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase B of
`docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`
(§8's "Phase B — Durable journey templates, on-demand only") end to end: a
new `journey_templates`/`journey_template_legs` table pair, a
`journeys.source_template_id` lineage column, four core routes under
`/JourneyTemplates` (create/promote, list, detail, manual materialize —
"Run now"), plus two small CRUD routes this plan adds to make the durable
entity actually manageable (edit, delete — see Judgment Call 1), and a
`/journeys/templates` frontend section (list, detail/edit, "save as
template," "run now"). **On-demand only** — no scheduler, no automated
sweep, `'auto'` match-mode does nothing, and `days_of_week`/`active`/
`starts_on`/`ends_on`/`default_match_mode`/`auto_commit_rule` are all
columns that exist (per the design doc §8's explicit migration-avoidance
note) but that no code in this plan reads or writes beyond their `DEFAULT`.
Recurrence, the sweep, and real `'auto'` matching are Phase C, planned
separately — see this plan's closing section for exactly what Phase C
needs to reuse from Phase B's work.

**Architecture:** backend before frontend, same ordering the parent
journey-tracking plans and the custom-tracking-names plan both use — the
frontend depends on the new routes/types existing. One migration
(Task 1) adds both new tables plus the one additive `journeys` column,
using the design doc's own §2.2 schema verbatim (every column, including
the Phase-C-only ones). A new, self-contained data module,
`crates/api/src/data/journey_templates.rs` (Task 2), owns every read/write
against the two new tables — it does **not** reach into
`crates/api/src/data/journeys.rs`'s private `insert_journey`/`insert_leg`
helpers (those stay private and unchanged); instead it does its own
`journeys`/`journey_legs` inserts for materialization, the same
"each data module owns the tables its own routes need to touch, in its own
queries" convention `journeys.rs` itself established relative to
`train_tracking.rs`. It does, however, **reuse two existing `journeys.rs`
functions unchanged**: `journey_owner` (confirmed present,
`crates/api/src/data/journeys.rs:33-39`, and — per its own doc comment —
not yet called by any route; this plan is its first real caller) to gate
the "promote an existing journey" path on true ownership, and
`list_legs_for_journey` (`journeys.rs:795-813`) to read the source
journey's legs back. A new routes module,
`crates/api/src/routes/journey_templates.rs` (Task 3), exposes six HTTP
handlers behind `AuthenticatedUser`, mounted in `main.rs` exactly the way
`routes::journeys::router()` already is. The frontend (Tasks 4-9) adds the
new wire types, two `GET` fetchers in `lib/api.ts` (mirroring
`getJourney`/`getMyJourneys`'s own shape), and a `/journeys/templates`
list + `/journeys/templates/[id]` detail/edit page pair modeled directly on
`/journeys/[id]/page.tsx` and `AddJourneyLegButton.tsx`/
`RemoveJourneyLegButton.tsx`/`ShareJourneyButton.tsx`'s established
button → modal → same-origin-`fetch()` → `router.refresh()` shape.

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`
— authoritative for every architectural decision this plan implements
(§2.2's schema, §3.2's materialization mechanics minus the `'auto'`/deferred
-commit machinery, §6's UX entry points). This plan implements **only**
Phase B (§8) of that document. It does not re-argue anything the design doc
already settled, including the two 2026-09-22 product decisions in §7 (a
full durable template entity is in scope; `'nearest_to_now'` is the
eventual auto-commit rule) — the second of those is Phase C's problem
entirely; nothing in this plan reads `auto_commit_rule`.

**Verified against the current codebase, not just the design doc's own
citations** (the parent journey-tracking feature has been through a UX
review fix cycle since the design doc was written): `journeys.rs`'s three
creation functions are confirmed still named `create_journey_with_pin_leg`,
`create_journey_with_known_train_leg`, `create_journey_with_window_leg`
exactly as the design doc assumes (`crates/api/src/data/journeys.rs:232,
276,424`); the `journeys`/`journey_legs` schema in
`crates/api/migrations/20260922090000_journeys.sql` matches the design
doc's §0.1 citation field-for-field; `journey_readable_by`
(`journeys.rs:762-780`) carries an explicit doc comment forbidding its use
to gate any write route — honored below by using `journey_owner` instead
for the promote-from-journey path, which needs true ownership, not
group-shared readability.

---

## Judgment calls this plan makes (read before Task 1)

The task brief names four routes explicitly (`POST /JourneyTemplates`,
`GET /JourneyTemplates/mine`, `GET /JourneyTemplates/{id}`,
`POST /JourneyTemplates/{id}/materialize`) but also asks for a frontend
"detail/edit view" and calls journey templates a "durable entity" the
product wants to "manage." Four things needed resolving to make that
buildable:

1. **This plan adds two routes beyond the four named explicitly: `PUT
   /JourneyTemplates/{id}` (full-resource edit) and `DELETE
   /JourneyTemplates/{id}`.** A "detail/edit view" cannot edit anything
   without a write route to call, and a "durable entity" a user "manages"
   that can never be deleted or corrected once created is not actually
   durable-and-manageable, it's create-only. `PUT` for a full-resource
   replace (not a narrow `POST .../name`-style single-field route) mirrors
   this codebase's own established precedent for a full-resource edit —
   `crates/api/src/routes/lines.rs:28-36`'s `PUT /lines/{id}` →
   `update_line`, which takes every editable field of a custom line at once
   and replaces them, exactly the shape a template's "customName + legs"
   editable surface needs (see the custom-tracking-names plan's own
   Judgment Call 2 for why this codebase reserves `PUT` for a full replace
   and `POST .../narrow-subpath` for a single-field mutation — a
   template's edit is the former, not the latter: there is no single field,
   the whole leg list can change shape). `DELETE` mirrors
   `routes::train::delete_tracked_train`'s and
   `routes::journeys::delete_journey_leg`'s own shape (`AuthenticatedUser`
   + ownership-scoped `WHERE ... AND user_id = $N` + `204 No Content`).
   Both stay minimal: `PUT` replaces the whole leg list at once (delete all,
   re-insert), not a per-leg patch route — see Judgment Call 4.

2. **`validate_template_leg` does NOT require "at least one window bound
   set," unlike `journeys::validate_window_leg`
   (`crates/api/src/data/journeys.rs:381-408`).** That existing rule exists
   to resolve a real ambiguity for an ordinary journey leg: an all-`NULL`
   window is indistinguishable from a `pin`/`knownTrain`-mode leg that was
   never window-searched at all, and `journey_legs.depart_after` etc. being
   non-`NULL` is literally the signal `GET /Journeys/{id}` uses to decide
   whether a *matched* leg offers "Change train" (`validate_window_leg`'s
   own doc comment). **That ambiguity does not exist for a template leg**:
   a template leg is never itself "matched," so there is no "does this leg
   support Change train" question to answer from its window fields. This
   matters concretely for the promote-from-journey path (below): promoting
   a `pin`/`knownTrain`-mode leg (whose `depart_after`/`depart_before`/
   `arrive_after`/`arrive_before` are `NULL` by construction — `journeys.rs`'s
   own doc comments on `create_journey_with_pin_leg`/
   `create_journey_with_known_train_leg`) must still succeed and produce a
   usable template leg, not a 400. The materialized leg it eventually
   produces (Task 2's `materialize_template`) will have an all-`NULL`
   window — functionally an unbounded "any train, any time" candidate
   search, identical in effect to a deliberately wide window search a user
   could already construct today via `AddJourneyLegButton`'s window mode
   with all four fields blank... except `validate_window_leg` itself
   blocks that exact case for an ordinary leg. This is a real, named,
   accepted gap for Phase B: promoting a pin/knownTrain-mode leg yields a
   template leg with no window, and materializing it yields an unmatched
   leg whose candidate list is unfiltered. Not a blocker — the candidate
   picker still works, it's just wide — but worth surfacing to the
   product owner if it proves confusing in practice (a natural Phase-C-or-
   later follow-up, not fixed here).

3. **Promoting an existing journey into a template requires the caller to
   *own* the source journey — `journeys::journey_owner`, not
   `journeys::journey_readable_by`.** `journey_readable_by`'s own doc
   comment (`journeys.rs:750-761`) is explicit: "READ-ONLY AUTHORIZATION
   ONLY... must NEVER be used to gate a write route." Promoting a journey
   derives a new, durable, independently-owned resource from it — squarely
   a write, not a read — so a fellow group member who can merely *view* a
   journey shared into their group must not be able to mint a template
   from someone else's journey. `journey_owner` (confirmed present,
   `journeys.rs:33-39`, currently uncalled by any route) is exactly the
   right primitive: `Some(user_id) if user_id == caller.id` or reject with
   404 (never 403, same convention as everything else in this file).

4. **`PUT /JourneyTemplates/{id}` replaces the whole leg list atomically
   (delete-then-reinsert in one transaction), not a per-leg patch API.**
   A per-leg `PATCH`/reorder API would need `leg_order` renumbering logic
   with no existing precedent to model (the closest analogue,
   `journeys::owned_next_leg_order`, only ever *appends* — Phase 1/2 never
   needed to reorder or remove a specific leg from the middle). A template
   is a much smaller, editor-owned resource than a live journey (no bound
   trains, no notification state, nothing external references an
   individual `journey_template_legs` row by id once
   `journeys.source_template_id` only ever points at the template, never
   at one of its legs) — so "send the whole new leg list, we delete the
   old ones and insert the new ones in the same transaction" is safe,
   simple, and exactly what a "detail/edit view" form naturally produces
   (the user is looking at the whole leg list in one form and hits Save).

5. **Frontend Phase B ships only the "promote an existing journey" creation
   entry point — no "start a blank template from scratch" UI.** The
   design doc's own §6 UX section lists exactly one creation entry point
   for a durable template: "Make this a template" / "Save as reusable" on
   an existing journey's detail page (§6 item 2). It does not describe a
   "create a template with no starting journey" flow anywhere. The backend
   still supports both shapes (`POST /JourneyTemplates`'s `mode: "manual"`
   variant, Task 2/3 — needed anyway since `PUT`'s replace body reuses the
   identical `TemplateLegRequest` shape, so building it costs nothing
   extra), matching the task brief's own "create, and 'promote an existing
   journey'" wording literally. But the frontend (Tasks 6-8) wires up only
   the promote flow and the edit-after-creation flow (which, via Judgment
   Call 4's whole-list replace, already lets a user add further legs to a
   promoted single-leg template) — not a second "blank template" creation
   form the spec never asked for. If a future need for "start from
   nothing" emerges, the backend route already exists; only a frontend form
   would need adding.

6. **No shared-constant length cap on `journey_templates.custom_name`.**
   Confirmed by reading `post_journey`
   (`crates/api/src/routes/journeys.rs:363-532`): `journeys.custom_name`
   itself has **no** length validation anywhere in this codebase today —
   `body.custom_name.as_deref()` is passed straight through uncapped. A
   template's `custom_name` follows the same, already-established
   precedent (unlike a tracked train's/ticket's `custom_name`, which does
   have `common::CUSTOM_NAME_MAX_LENGTH` — a different feature, a
   different decision, not one this plan should retroactively impose on
   journeys or templates).

7. **`materialize_template`'s idempotency guard is Phase C's problem, not
   built here.** The design doc's §3.1 "no `journeys` row already exists
   for `(source_template_id, today)`" guard exists to stop an *automated
   sweep* from double-minting an occurrence if it runs twice before
   midnight. Phase B has no sweep — the trigger is always a deliberate
   human click on "Run now," and the design doc's own §6 item 3 explicitly
   names a legitimate reason to click it more than once for the same date
   range: "or for topping up a recurring one on an extra day." Building a
   uniqueness constraint or a guard clause here would make that legitimate
   repeat-click case an error for no product reason. Phase C's sweep adds
   its own idempotency check as an extra `WHERE NOT EXISTS (...)` clause in
   its *own* due-template query, entirely outside
   `materialize_template`'s signature — see this plan's closing section.

---

## Non-goals

(Restated and scoped from the design doc's own §8 Phase B boundary and
this plan's task brief — no task below touches any of these.)

- **No automated daily sweep, no `notifier`/scheduler branch.** Phase C.
  `materialize_template` is only ever called synchronously from the
  `POST /JourneyTemplates/{id}/materialize` route's own request handler.
- **`'auto'` match-mode does nothing.** `default_match_mode`/
  `auto_commit_rule` exist as columns (migration, Task 1) with their
  `DEFAULT`s, and are round-tripped by `GET`/`PUT` (so a future Phase C UI
  has somewhere to write them without a second migration), but **no task
  in this plan reads either column to change materialization behavior** —
  `materialize_template` unconditionally mints every leg `match_mode =
  'unmatched'`, regardless of what `default_match_mode` says.
- **`days_of_week`/`active`/`starts_on`/`ends_on` are inert.** Same
  treatment as `default_match_mode` above: columns exist, are
  round-tripped by the CRUD routes this plan adds (see Task 3's exact
  field list), but nothing anywhere checks them. A template with
  `active = false` still materializes fine via the manual "Run now" button
  — pausing only matters once a sweep exists to pause.
- **No template sharing (`group_journey_templates`).** Design doc §5,
  Phase E. A template is visible only to its owner in this plan; every
  route in Task 3 is ownership-scoped, none reads `group_members`/
  `group_journeys`.
- **No new notification class.** Design doc §4.2, Phase D. Materializing a
  template's occurrence is silent, exactly like every other leg-creation
  path today.
- **No retention/archival job, no `/journeys/mine` list redesign.** Design
  doc §7 Q3/Q4, explicitly deferred there too.
- **No bank-holiday calendar, no single-day snooze.** Design doc §7 Q5.
- **No day-of-week picker, three-way match-mode chooser, or Pause toggle
  UI.** These are real fields on the `PUT`/`GET` wire shapes (round-tripped
  so Phase C's frontend work has a place to plug in without another
  backend change) but Task 3/8's UI does **not** render controls for them
  — see Task 3's own field-list note and Task 8's own scope note.

## Global Constraints

- **Every new/changed route follows the 404-never-403 ownership
  convention** already stated in `crates/api/src/routes/train.rs`'s module
  doc and used verbatim by every route in `routes/journeys.rs`: "doesn't
  exist" and "exists but isn't yours" are indistinguishable to the caller.
  Every write in Task 2 folds the ownership check directly into its own
  `WHERE ... AND user_id = $N` (or, for materialize's multi-statement
  transaction, an explicit ownership `SELECT` first) — never a separate
  ownership lookup followed by an unscoped write, matching
  `journeys::set_leg_train_subscription`'s own established shape.
- **Data layer validates nothing beyond pure, callable-without-a-pool
  checks (`validate_template_leg`); routes validate, data layer writes.**
  Same split `journeys.rs`'s own module doc and
  `validate_window_leg`/`create_journey_with_window_leg`'s doc comments
  establish.
- **`journey_templates.rs` never touches `journeys.rs`'s private
  `insert_journey`/`insert_leg` helpers.** They stay `private` to that
  file, unchanged. Materialization does its own `INSERT INTO
  journeys (...)`/`INSERT INTO journey_legs (...)` inside
  `data::journey_templates::materialize_template`, in the same module that
  owns the ownership-checked read of the template being materialized —
  seeing the whole multi-statement transaction in one function, one file,
  matters more here than deduplicating a two-line `INSERT` with a
  same-shaped one three files away.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (this repo's actual CI
  invocation for local use — CI's own `clippy` job runs
  `auguwu/clippy-action@9817d076b82df0194935be9db6154c56ac07b317` with
  `--workspace --all-features` only, per `.github/workflows/ci.yml`'s own
  comment explaining why `--all-targets` can't be passed to that action;
  running the fuller local invocation before pushing is still the right
  habit), `cargo test --workspace` (ignored tests skipped — CI's
  unconditional fast job, `.github/workflows/ci.yml:231-232`), and
  `cargo test -p api -- --ignored --test-threads=1` for every DB-gated
  test this plan adds (CI's own exact invocation,
  `.github/workflows/ci.yml:259-260`, against
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres` —
  needs an equivalent local Postgres). Frontend: `npm test -- <file>`
  (vitest) per changed test file, plus a full `npm test` and `npm run
  build` before considering the frontend tasks done
  (`.github/workflows/ci.yml:303-307`).
  **UI verification**: per this repo's standing practice for a change with
  no automated end-to-end coverage, start the dev stack and manually
  verify in a real browser — promote a real journey into a template,
  confirm it appears on `/journeys/templates`, edit its legs and confirm
  the change round-trips, click "Run now" and confirm a new journey
  appears at `/journeys/{newId}` with unmatched legs ready to pick a
  candidate, then delete the template and confirm the previously-created
  journey (`journeys.source_template_id`) survives untouched. Folded into
  Tasks 8/9's own Verify steps, not a separate task.
- **File scope.** Modified/created:
  - `crates/api/migrations/20260922140000_journey_templates.sql` (new)
  - `crates/api/src/data/journey_templates.rs` (new)
  - `crates/api/src/data/mod.rs`
  - `crates/api/src/routes/journey_templates.rs` (new)
  - `crates/api/src/routes/mod.rs`
  - `crates/api/src/main.rs`
  - `frontend/lib/types.ts`
  - `frontend/lib/api.ts`
  - `frontend/components/SaveAsTemplateButton.tsx` (new)
  - `frontend/components/SaveAsTemplateButton.test.tsx` (new)
  - `frontend/components/RunTemplateNowButton.tsx` (new)
  - `frontend/components/RunTemplateNowButton.test.tsx` (new)
  - `frontend/components/DeleteJourneyTemplateButton.tsx` (new)
  - `frontend/components/DeleteJourneyTemplateButton.test.tsx` (new)
  - `frontend/components/EditJourneyTemplateForm.tsx` (new)
  - `frontend/components/EditJourneyTemplateForm.test.tsx` (new)
  - `frontend/app/journeys/[id]/page.tsx`
  - `frontend/app/journeys/templates/page.tsx` (new)
  - `frontend/app/journeys/templates/page.test.tsx` (new)
  - `frontend/app/journeys/templates/[id]/page.tsx` (new)
  - `frontend/app/journeys/templates/[id]/page.test.tsx` (new)
  - `frontend/app/track/mine/page.tsx`
  No other file changes.

---

## Task 1: Migration — `journey_templates`, `journey_template_legs`, `journeys.source_template_id`

**Files:** create `crates/api/migrations/20260922140000_journey_templates.sql`.

`20260922140000` sorts immediately after the latest existing migration
(`20260922130000_journey_leg_notification_state.sql`, confirmed via `ls
crates/api/migrations | sort | tail -3`). Independent of every other task —
a bare DDL migration needs no Rust code to apply.

- [ ] **Step 1: Write the migration**, using the design doc's §2.2 schema
  verbatim (every column, including the Phase-C-only ones, per this plan's
  own header/§8's migration-avoidance note):

```sql
-- -------------------------------------------------------------------------
-- Journey templates, Phase B (durable, on-demand only) --
-- docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
-- §2.2, §8.
--
-- A template is a saved journey SHAPE a user can stamp a new `journeys` row
-- from, either on demand (this migration -- Phase B) or, eventually, on a
-- schedule (Phase C). Deliberately its OWN table pair, not folded onto
-- `journeys`/`journey_legs`: a template leg has no `service_date`
-- (`journey_legs.service_date` is NOT NULL and every existing reader
-- assumes a concrete date), no `train_subscription_id` (a template is
-- never itself bound to a real train), and needs fields that would be
-- meaningless NULLs on every ordinary journey.
--
-- Phase-C-only columns (days_of_week, active, starts_on, ends_on,
-- default_match_mode, auto_commit_rule) are created NOW, in this Phase-B
-- migration, deliberately unused by any Phase-B code path -- adding them
-- later would be a second migration for no reason; every Phase-B route
-- round-trips them (a client can set/read default_match_mode etc. today)
-- but nothing acts on them until Phase C's sweep exists. See the design
-- doc's §8 note making this explicit.
-- -------------------------------------------------------------------------

CREATE TABLE journey_templates (
    id                  BIGSERIAL PRIMARY KEY,
    user_id             TEXT NOT NULL REFERENCES users(id),
    custom_name         TEXT,
    -- Phase C: NULL = a one-shot template (Phase B's only shape). Non-NULL
    -- = which days this template auto-materializes on, Mon=1..Sun=64.
    -- Written/read by this plan's routes (round-tripped, always NULL in
    -- practice until Phase C's UI sets it) but never checked by
    -- materialize_template -- Phase B ignores this column entirely.
    days_of_week        SMALLINT,
    -- Phase C: pause without deleting. Always TRUE in practice through
    -- Phase B (no UI writes anything but the DEFAULT), but round-tripped
    -- by GET/PUT so a future Phase C toggle has somewhere to write.
    active              BOOLEAN NOT NULL DEFAULT TRUE,
    -- Phase C: recurrence window bounds. Unused by Phase B.
    starts_on           DATE,
    ends_on             DATE,
    -- Phase C: how a materialized leg's match_mode is seeded. Phase B's
    -- own materialize_template ignores this column completely -- every
    -- leg it mints is 'unmatched', regardless of what this says.
    default_match_mode  TEXT NOT NULL DEFAULT 'manual'
                         CHECK (default_match_mode IN ('manual', 'auto')),
    -- Phase C: NULL unless default_match_mode='auto'. See the design doc's
    -- §2.3/§3.2 for the 2026-09-22-resolved 'nearest_to_now' mechanics --
    -- none of that is implemented anywhere in this migration or plan.
    auto_commit_rule    TEXT
                         CHECK (auto_commit_rule IN ('earliest', 'nearest_to_now')),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX journey_templates_user_id ON journey_templates (user_id);

CREATE TABLE journey_template_legs (
    id                  BIGSERIAL PRIMARY KEY,
    template_id         BIGINT NOT NULL REFERENCES journey_templates(id) ON DELETE CASCADE,
    leg_order           INT NOT NULL,
    -- Both nullable: a leg promoted from a pin/knownTrain-mode journey leg
    -- may legitimately have neither yet (the "no schedule data yet" gap
    -- journey_legs.origin_crs/destination_crs already accept -- see
    -- 20260922090000_journeys.sql's own header comment for the same
    -- reasoning one layer down). This plan's own route-level validation
    -- (Task 3) still requires BOTH to be present for a MANUALLY-created
    -- template leg -- only a promoted leg can legitimately arrive here
    -- with either NULL, and even then only transiently (see Task 2's
    -- promote-path handling).
    origin_crs          TEXT,
    destination_crs     TEXT,
    -- No service_date -- a template leg is date-less by definition; the
    -- materialized journey_legs row gets the target date at stamping time
    -- (Task 2's materialize_template).
    depart_after        TIME,
    depart_before       TIME,
    arrive_after        TIME,
    arrive_before       TIME,
    UNIQUE (template_id, leg_order)
);
CREATE INDEX journey_template_legs_template_id ON journey_template_legs (template_id);

-- Lineage: which template (if any) produced a given journey. Nullable and
-- ON DELETE SET NULL so deleting a template never cascades into deleting
-- journeys it already produced -- matches journey_legs.train_subscription_id's
-- own ON DELETE SET NULL precedent (20260922090000_journeys.sql): a
-- deleted parent orphans its children's foreign key, never deletes the
-- children themselves.
ALTER TABLE journeys ADD COLUMN source_template_id BIGINT
    REFERENCES journey_templates(id) ON DELETE SET NULL;
CREATE INDEX journeys_source_template_id ON journeys (source_template_id);
```

- [ ] **Step 2: Verify.** `sqlx` migrations in this crate run automatically
  against `DATABASE_URL` on `cargo test`/`cargo run` startup (no separate
  `sqlx migrate run` step in CI). Confirm the migration applies cleanly:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
```

  Expected: no `sqlx::migrate::MigrateError`. Then confirm the shape:

```bash
psql "$DATABASE_URL" -c "\d journey_templates"
psql "$DATABASE_URL" -c "\d journey_template_legs"
psql "$DATABASE_URL" -c "\d journeys" | grep source_template_id
```

  Expected: both new tables show every column listed above with the right
  nullability/`CHECK`/`DEFAULT`; `journeys` shows the new nullable
  `source_template_id bigint` column with its FK.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260922140000_journey_templates.sql
git commit -m "api: add journey_templates/journey_template_legs and journeys.source_template_id"
```

---

## Task 2: Data layer — `crates/api/src/data/journey_templates.rs`

**Files:** create `crates/api/src/data/journey_templates.rs`; modify
`crates/api/src/data/mod.rs` (add `pub mod journey_templates;`, alongside
the existing `pub mod journeys;` line, alphabetically after
`island_of_ireland`/before `legacy_backfill` per that file's existing
alphabetical ordering).

Depends on Task 1 (the tables/column must exist for every query here to
compile against a real schema at test time).

- [ ] **Step 1: Row/DTO structs.** Mirrors `journeys.rs`'s own
  `JourneyLegRow`/`JourneyLegWithNamesRow` split — a plain row for
  ownership-scoped internal reads, a "with names" row for the two places
  that need resolved station names (list + detail).

```rust
//! Durable journey templates (Phase B, on-demand only) -- see
//! docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
//! §2.2, §8 and
//! docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md.
//! This module owns `journey_templates`/`journey_template_legs` entirely --
//! it never reaches into `crate::data::journeys`'s private `insert_journey`/
//! `insert_leg` helpers (see this plan's own Global Constraints); it does
//! its own `journeys`/`journey_legs` writes for [`materialize_template`],
//! and reuses exactly two existing `journeys.rs` functions unchanged:
//! [`crate::data::journeys::journey_owner`] (ownership check for the
//! promote-from-journey path) and
//! [`crate::data::journeys::list_legs_for_journey`] (reads the source
//! journey's own legs back for that same path).
//!
//! **Phase C columns, present but inert**: `days_of_week`/`active`/
//! `starts_on`/`ends_on`/`default_match_mode`/`auto_commit_rule` are all
//! read and written by this module's CRUD functions (so a future Phase C
//! UI/sweep has somewhere to read/write without a second migration), but
//! [`materialize_template`] never inspects any of them -- every leg it
//! mints is unconditionally `match_mode = 'unmatched'`.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::Serialize;
use sqlx::PgPool;

/// One `journey_templates` row, unresolved -- backs the ownership check
/// every write route folds a read through, and (via
/// [`get_owned_template`]) the detail route's own header fields.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyTemplateRow {
    pub id: i64,
    pub user_id: String,
    pub custom_name: Option<String>,
    pub days_of_week: Option<i16>,
    pub active: bool,
    pub starts_on: Option<NaiveDate>,
    pub ends_on: Option<NaiveDate>,
    pub default_match_mode: String,
    pub auto_commit_rule: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One `journey_template_legs` row plus resolved station names -- same
/// `LEFT JOIN stations ... ON s.crs = UPPER(...)` mechanism
/// `journeys::JourneyLegWithNamesRow` already uses, same "`None` means no
/// reference row for that code, not no leg" contract.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyTemplateLegWithNamesRow {
    pub id: i64,
    pub template_id: i64,
    pub leg_order: i32,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
}

/// One row of `GET /JourneyTemplates/mine` -- deliberately lighter than the
/// full detail response, mirroring `journeys::JourneyListItem`'s own
/// "list is lighter than detail" split. Summarizes a multi-leg template as
/// "first leg's origin -> last leg's destination," the same rollup
/// `app/journeys/[id]/page.tsx`'s `defaultJourneyTitle` computes client-side
/// for a journey with no `customName` -- computed server-side here instead
/// since a list row has no per-leg detail to compute it from client-side.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyTemplateListItem {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub leg_count: i64,
    pub first_origin_crs: Option<String>,
    pub first_origin_name: Option<String>,
    pub last_destination_crs: Option<String>,
    pub last_destination_name: Option<String>,
    pub active: bool,
    pub days_of_week: Option<i16>,
}
```

- [ ] **Step 2: `validate_template_leg`** — pure, no pool, the one piece of
  validation this module owns (route calls it before every write; see
  Judgment Call 2 for why this does *not* require a window bound the way
  `journeys::validate_window_leg` does):

```rust
/// User-facing validation for a manually-entered template leg's
/// origin/destination -- same 3-letter CRS check as
/// `journeys::validate_window_leg`, deliberately WITHOUT that function's
/// "at least one window bound set" requirement. See this plan's Judgment
/// Call 2 for the full reasoning: a template leg is never itself
/// "matched," so the ambiguity that rule exists to prevent for an ordinary
/// journey leg doesn't apply here. Not called at all for a leg produced by
/// the promote-from-journey path (Task 3's route handles that leg
/// separately -- see [`create_template`]'s own doc comment).
pub fn validate_template_leg(origin_crs: &str, destination_crs: &str) -> Result<(), String> {
    if origin_crs.trim().len() != 3 {
        return Err(
            "Enter a valid origin station — CRS codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    if destination_crs.trim().len() != 3 {
        return Err(
            "Enter a valid destination station — CRS codes are three letters, like WOK or \
             EUS."
                .to_string(),
        );
    }
    Ok(())
}
```

- [ ] **Step 3: `TemplateLegInput` + `create_template`** — the one write
  path both `POST /JourneyTemplates`'s two modes (`manual` and
  `fromJourney`) and `PUT /JourneyTemplates/{id}` funnel through:

```rust
/// A template leg's writable fields, already validated/CRS-normalized by
/// the caller (route layer for `manual` mode via [`validate_template_leg`];
/// the promote-from-journey route handler for `fromJourney` mode, which
/// copies a source journey leg's own fields verbatim with no re-validation
/// -- see Task 3's `post_journey_template`). Shared by [`create_template`]
/// and [`replace_template`].
pub struct TemplateLegInput {
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
}

async fn insert_template(pool: &PgPool, user_id: &str, custom_name: Option<&str>) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_templates (user_id, custom_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(custom_name)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

async fn insert_template_leg(
    pool: &mut sqlx::PgConnection,
    template_id: i64,
    leg_order: i32,
    leg: &TemplateLegInput,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_template_legs \
            (template_id, leg_order, origin_crs, destination_crs, \
             depart_after, depart_before, arrive_after, arrive_before) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id",
    )
    .bind(template_id)
    .bind(leg_order)
    .bind(&leg.origin_crs)
    .bind(&leg.destination_crs)
    .bind(leg.depart_after)
    .bind(leg.depart_before)
    .bind(leg.arrive_after)
    .bind(leg.arrive_before)
    .fetch_one(&mut *pool)
    .await?;
    Ok(id)
}

/// Creates a new template with `legs.len()` legs (`leg_order` 1-based,
/// assignment order), in one transaction -- all legs land or none do. The
/// ROUTE layer is responsible for having already validated every leg
/// (`validate_template_leg` for a `manual`-mode request; the
/// promote-from-journey path validates nothing here, since a source
/// journey's own legs were already validated when THAT journey was
/// created -- see Task 3). `legs` must be non-empty -- the route rejects
/// an empty array with 400 before this is ever called (see Task 3's own
/// validation step), so [`materialize_template`] can safely assume every
/// stored template has at least one leg.
pub async fn create_template(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    legs: &[TemplateLegInput],
) -> anyhow::Result<i64> {
    let mut tx = pool.begin().await?;
    let (template_id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_templates (user_id, custom_name) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(custom_name)
    .fetch_one(&mut *tx)
    .await?;
    for (index, leg) in legs.iter().enumerate() {
        insert_template_leg(&mut tx, template_id, (index + 1) as i32, leg).await?;
    }
    tx.commit().await?;
    Ok(template_id)
}
```

  (Note: `insert_template` above is dead code once `create_template` inlines
  its own insert to share one transaction handle — delete the standalone
  `insert_template` helper before committing this step; it was drafted
  first as a template for the transactional version and is superseded by
  it, not meant to coexist. `cargo clippy` will flag it as unused if left
  in.)

- [ ] **Step 4: `get_owned_template` + `list_template_legs`** —
  ownership-scoped single-row read, and the (unscoped, caller-must-have-
  already-checked-ownership) leg list, mirroring `journeys::get_owned_leg`/
  `list_legs_for_journey`'s own split exactly:

```rust
/// Ownership-scoped, folds `user_id` directly into the `WHERE` — same
/// convention as every other ownership check in this codebase.
/// `Ok(None)` for "no such template, or not this caller's" (route maps to
/// 404, never 403).
pub async fn get_owned_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneyTemplateRow>> {
    let row = sqlx::query_as::<_, JourneyTemplateRow>(
        "SELECT id, user_id, custom_name, days_of_week, active, starts_on, ends_on, \
                default_match_mode, auto_commit_rule, created_at, updated_at \
         FROM journey_templates WHERE id = $1 AND user_id = $2",
    )
    .bind(template_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every leg of a template, `leg_order` ascending, with resolved station
/// names. Deliberately NOT ownership-scoped on its own -- same reasoning
/// as `journeys::list_legs_for_journey`'s own doc comment: every real
/// caller (the detail route, `materialize_template` below) already
/// confirmed ownership one call earlier via [`get_owned_template`].
pub async fn list_template_legs(
    pool: &PgPool,
    template_id: i64,
) -> anyhow::Result<Vec<JourneyTemplateLegWithNamesRow>> {
    let rows = sqlx::query_as::<_, JourneyTemplateLegWithNamesRow>(
        "SELECT jtl.id, jtl.template_id, jtl.leg_order, jtl.origin_crs, so.name AS origin_name, \
                jtl.destination_crs, sd.name AS destination_name, \
                jtl.depart_after, jtl.depart_before, jtl.arrive_after, jtl.arrive_before \
         FROM journey_template_legs jtl \
         LEFT JOIN stations so ON so.crs = UPPER(jtl.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(jtl.destination_crs) \
         WHERE jtl.template_id = $1 ORDER BY jtl.leg_order",
    )
    .bind(template_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 5: `list_templates_for_user`** — the `/mine` list query,
  reusing `train_tracking::MINE_LIST_LIMIT` exactly as
  `journeys::list_journeys_for_user` already does (that constant's own doc
  comment anticipates this: "any list this list's own cap should agree
  with"):

```rust
/// Most-recently-created template first, capped at
/// `train_tracking::MINE_LIST_LIMIT` -- same cap `journeys::JourneyListItem`
/// already shares. Each row is summarized by its first leg's origin and
/// last leg's destination (`MIN`/`MAX` on `leg_order`, joined back to the
/// leg rows that own those extremes) -- a template with zero legs (should
/// never happen given `create_template`'s own non-empty-legs invariant,
/// but defensively possible if a row is ever hand-edited) renders with
/// `leg_count = 0` and every origin/destination field `NULL`, not an error.
pub async fn list_templates_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyTemplateListItem>> {
    let rows = sqlx::query_as::<_, JourneyTemplateListItem>(
        "SELECT jt.id, jt.custom_name, jt.created_at, jt.active, jt.days_of_week, \
                COALESCE(leg_counts.leg_count, 0) AS leg_count, \
                first_leg.origin_crs AS first_origin_crs, so.name AS first_origin_name, \
                last_leg.destination_crs AS last_destination_crs, sd.name AS last_destination_name \
         FROM journey_templates jt \
         LEFT JOIN ( \
             SELECT template_id, COUNT(*) AS leg_count, \
                    MIN(leg_order) AS min_order, MAX(leg_order) AS max_order \
             FROM journey_template_legs GROUP BY template_id \
         ) leg_counts ON leg_counts.template_id = jt.id \
         LEFT JOIN journey_template_legs first_leg \
             ON first_leg.template_id = jt.id AND first_leg.leg_order = leg_counts.min_order \
         LEFT JOIN journey_template_legs last_leg \
             ON last_leg.template_id = jt.id AND last_leg.leg_order = leg_counts.max_order \
         LEFT JOIN stations so ON so.crs = UPPER(first_leg.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(last_leg.destination_crs) \
         WHERE jt.user_id = $1 \
         ORDER BY jt.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 6: `replace_template`** — the `PUT` full-replace (Judgment
  Calls 1 and 4): ownership-scoped update of the header row, then a
  delete-then-reinsert of every leg, all in one transaction.

```rust
/// Full-resource replace: updates `custom_name` and wholesale replaces
/// every leg, ownership-scoped, one transaction. `Ok(false)` for "no such
/// template, or not this caller's" (route maps to 404) -- checked via the
/// `UPDATE ... WHERE id = $1 AND user_id = $2` itself, same
/// fold-ownership-into-the-write convention as
/// `journeys::set_leg_train_subscription`. `legs` must be non-empty --
/// same route-level guard as [`create_template`]'s own contract.
pub async fn replace_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
    custom_name: Option<&str>,
    legs: &[TemplateLegInput],
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE journey_templates SET custom_name = $1, updated_at = NOW() \
         WHERE id = $2 AND user_id = $3",
    )
    .bind(custom_name)
    .bind(template_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        // Not owned/doesn't exist -- roll back (no leg mutation happened
        // yet) and report "not found" to the route.
        tx.rollback().await?;
        return Ok(false);
    }

    sqlx::query("DELETE FROM journey_template_legs WHERE template_id = $1")
        .bind(template_id)
        .execute(&mut *tx)
        .await?;
    for (index, leg) in legs.iter().enumerate() {
        insert_template_leg(&mut tx, template_id, (index + 1) as i32, leg).await?;
    }
    tx.commit().await?;
    Ok(true)
}
```

- [ ] **Step 7: `delete_template`** — mirrors
  `journeys::delete_leg`'s/`train_tracking::delete_tracked_train`'s own
  ownership-scoped delete shape. `ON DELETE CASCADE` on
  `journey_template_legs.template_id` handles the legs; the `journeys.
  source_template_id ... ON DELETE SET NULL` FK (Task 1) means every
  journey this template ever produced survives, merely losing its lineage
  pointer.

```rust
/// Deletes a template the caller owns. Cascades into
/// `journey_template_legs` (`ON DELETE CASCADE`, Task 1); every
/// `journeys` row this template ever produced survives untouched, losing
/// only its `source_template_id` (`ON DELETE SET NULL`, same table). `true`
/// if a row was deleted, `false` for "no such template, or not this
/// caller's" (404, never 403).
pub async fn delete_template(pool: &PgPool, template_id: i64, user_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM journey_templates WHERE id = $1 AND user_id = $2")
        .bind(template_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 8: `MaterializedJourney` + `materialize_template`** — the
  core logic Phase C's sweep will eventually call too (see this plan's
  closing section for the exact reuse contract).

```rust
/// The result of one successful materialization -- the new `journeys.id`
/// plus every `journey_legs.id` it produced, `leg_order` ascending (same
/// order the template's own legs were read in).
pub struct MaterializedJourney {
    pub journey_id: i64,
    pub leg_ids: Vec<i64>,
}

/// Stamps a new `journeys` row (plus one `journey_legs` row per template
/// leg) from `template_id`'s current shape, dated `service_date`. This is
/// the design doc's §3.2 per-template materialization logic, called
/// synchronously by Task 3's `POST /JourneyTemplates/{id}/materialize`
/// route with no scheduler involved -- Phase C's automated sweep (not this
/// plan) will call this SAME function once it exists, passing the due
/// template's own `user_id` instead of an authenticated caller's (see this
/// plan's closing section).
///
/// Every leg is minted `match_mode = 'unmatched'`, `train_subscription_id
/// = NULL`, `service_date = service_date` (the caller's target date, not
/// "today" -- there is no implicit "today" default anywhere in this
/// function, matching this codebase's established "every leg-creation
/// wire type requires an explicit service_date" convention). The
/// template's own `default_match_mode`/`auto_commit_rule` are READ (via
/// [`get_owned_template`]) but never inspected for this decision -- Phase
/// B's materialization is unconditionally manual-pick, regardless of what
/// those columns say (see this plan's Non-goals).
///
/// Deliberately carries NO idempotency guard against calling this twice
/// for the same `(template_id, service_date)` -- see this plan's Judgment
/// Call 7: that guard belongs to Phase C's sweep query, not to this
/// function, since a Phase-B human deliberately re-clicking "Run now" for
/// the same date (e.g. to top up a second occurrence) is a legitimate,
/// supported case here.
///
/// `Ok(None)` for "no such template, or not this caller's" (route maps to
/// 404). All-or-nothing inside one transaction: a multi-leg template
/// either fully materializes or the whole attempt is rolled back -- an
/// incomplete journey missing some of its legs would violate every other
/// reader's assumption that a journey's legs are exactly what its owner
/// asked for.
pub async fn materialize_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<MaterializedJourney>> {
    let Some(template) = get_owned_template(pool, template_id, user_id).await? else {
        return Ok(None);
    };
    let legs = list_template_legs(pool, template_id).await?;
    // Invariant from create_template/replace_template: a stored template
    // always has >=1 leg. Defensive rather than an unwrap/panic if that's
    // ever violated by a hand-edited row -- report "nothing to
    // materialize" the same way "template not found" reads to the route,
    // rather than minting a zero-leg journeys row nothing else in this
    // codebase expects to see (journeys::delete_leg's own doc comment).
    if legs.is_empty() {
        return Ok(None);
    }

    let mut tx = pool.begin().await?;
    let (journey_id,): (i64,) = sqlx::query_as(
        "INSERT INTO journeys (user_id, custom_name, source_template_id) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(&template.custom_name)
    .bind(template_id)
    .fetch_one(&mut *tx)
    .await?;

    let mut leg_ids = Vec::with_capacity(legs.len());
    for leg in &legs {
        let (leg_id,): (i64,) = sqlx::query_as(
            "INSERT INTO journey_legs \
                (journey_id, leg_order, origin_crs, destination_crs, service_date, \
                 train_subscription_id, match_mode, \
                 depart_after, depart_before, arrive_after, arrive_before) \
             VALUES ($1, $2, $3, $4, $5, NULL, 'unmatched', $6, $7, $8, $9) \
             RETURNING id",
        )
        .bind(journey_id)
        .bind(leg.leg_order)
        .bind(&leg.origin_crs)
        .bind(&leg.destination_crs)
        .bind(service_date)
        .bind(leg.depart_after)
        .bind(leg.depart_before)
        .bind(leg.arrive_after)
        .bind(leg.arrive_before)
        .fetch_one(&mut *tx)
        .await?;
        leg_ids.push(leg_id);
    }
    tx.commit().await?;

    Ok(Some(MaterializedJourney { journey_id, leg_ids }))
}
```

- [ ] **Step 9: Unit tests** for `validate_template_leg` (pure, no DB) —
  mirror `journeys.rs`'s own `validate_window_leg` test shapes:

```rust
#[cfg(test)]
mod validate_template_leg_tests {
    use super::*;

    #[test]
    fn a_well_formed_leg_is_accepted() {
        assert!(validate_template_leg("WAT", "RDG").is_ok());
    }

    #[test]
    fn a_short_origin_code_is_rejected() {
        assert!(validate_template_leg("W", "RDG").is_err());
    }

    #[test]
    fn a_short_destination_code_is_rejected() {
        assert!(validate_template_leg("WAT", "R").is_err());
    }

    #[test]
    fn no_window_bound_is_required_unlike_validate_window_leg() {
        // Judgment Call 2 -- deliberately no window-bound check at all
        // here; this test exists to keep that decision from silently
        // regressing if someone copies validate_window_leg's body in
        // later.
        assert!(validate_template_leg("WAT", "RDG").is_ok());
    }

    #[test]
    fn validation_messages_carry_no_internal_field_names() {
        let message = validate_template_leg("W", "RDG").unwrap_err();
        assert!(!message.is_empty());
        assert!(!message.contains('_'), "user-facing copy leaked an identifier: {message}");
    }
}
```

- [ ] **Step 10: DB-gated `#[ignore]`d tests**, in a `mod db_tests` at the
  end of the file — mirror `journeys.rs`'s own `db_tests` fixture helpers
  (`connect`/`seed_user`/`cleanup_user`) verbatim (copy them; this module
  needs its own `cleanup_user` that also clears `journey_templates`/
  `journey_template_legs`, matching that file's own "each module needing
  fixture cleanup keeps its own copy" precedent). At minimum:

  - `create_template_creates_a_template_with_ordered_legs` — creates a
    2-leg template via `create_template`, reads it back via
    `list_template_legs`, asserts `leg_order` 1 and 2 in the right order
    with the right fields.
  - `get_owned_template_a_non_owner_gets_none` — same shape as
    `journeys.rs`'s own `set_leg_train_subscription_a_non_owner_cannot_bind_someone_elses_leg`:
    seed two users, template belongs to one, `get_owned_template` as the
    other returns `Ok(None)`.
  - `replace_template_swaps_the_whole_leg_list` — create a 2-leg template,
    `replace_template` with a *different* 1-leg list, assert
    `list_template_legs` now returns exactly the new leg (old legs gone,
    not merely appended).
  - `replace_template_a_non_owner_cannot_replace_it_and_it_survives` —
    same non-owner-write-rejected-and-row-survives shape as
    `journeys.rs`'s own `rename_tracked_train_a_non_owner_cannot_rename_it_and_the_row_survives`
    pattern (see the custom-tracking-names plan's Task 3 Step 6 for the
    closest literal precedent).
  - `delete_template_the_owner_can_delete_it_and_produced_journeys_survive_orphaned` —
    create a template, `materialize_template` once to produce a real
    `journeys` row, `delete_template`, then assert (a) the template is
    gone (`get_owned_template` returns `None`), (b) the previously
    materialized `journeys` row still exists with `source_template_id`
    now `NULL` (direct `SELECT source_template_id FROM journeys WHERE id =
    $1`).
  - `materialize_template_mints_a_journey_with_unmatched_legs_dated_the_target_date` —
    create a 2-leg template, `materialize_template` for a given
    `service_date`, then read the resulting `journey_legs` rows directly
    (`sqlx::query_as` on `journey_id`) and assert: `match_mode =
    'unmatched'` on both, `train_subscription_id IS NULL` on both,
    `service_date` equals the requested date (not `today`/`Utc::now()`),
    `leg_order` 1 and 2 preserved, and (this is the load-bearing assertion
    for this plan's Non-goals) that this holds **even when the template's
    own `default_match_mode` is `'auto'`** — seed the template with
    `default_match_mode = 'auto'` via a raw `UPDATE` after creation, then
    materialize and assert the legs are still `'unmatched'`, proving Phase
    B genuinely ignores that column.
  - `materialize_template_a_non_owner_gets_none` — same non-owner shape.
  - `materialize_template_can_be_called_twice_for_the_same_date_and_mints_two_journeys` —
    calls it twice with the same `service_date`, asserts two distinct
    `journey_id`s came back, both with `source_template_id` pointing at
    the same template — the concrete proof of Judgment Call 7 (no
    idempotency guard here).
  - `materialize_template_copies_a_nullable_window_verbatim` — create a
    template leg with all four window bounds `None` (the promote-from-a-
    pin-mode-leg shape, Judgment Call 2), materialize it, assert the
    resulting `journey_legs` row's four window columns are all `NULL` too
    (not defaulted to anything), and that `origin_crs`/`destination_crs`
    still made it through — proving the "wide open candidate search"
    outcome that Judgment Call names is real, observable behavior, not
    just a paragraph of reasoning.

  Every test in this group needs the same
  `#[ignore = "requires a live database; see this plan's Global Constraints \
   for the DATABASE_URL incantation, then run with `cargo test -p api \
   <fn_name> -- --ignored --test-threads=1`"]` annotation this codebase
  uses everywhere else.

- [ ] **Step 11: Verify**

```bash
cargo fmt --all
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api validate_template_leg_tests
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api -- --ignored --test-threads=1 journey_templates
```

  Expected: builds and lints clean, all unit tests pass, every DB-gated
  test added in Step 10 passes.

- [ ] **Step 12: Commit**

```bash
git add crates/api/src/data/journey_templates.rs crates/api/src/data/mod.rs
git commit -m "api: add journey_templates data layer (create/list/get/replace/delete/materialize)"
```

---

## Task 3: Routes — `crates/api/src/routes/journey_templates.rs`

**Files:** create `crates/api/src/routes/journey_templates.rs`; modify
`crates/api/src/routes/mod.rs` (add `pub mod journey_templates;`,
alphabetically after `journeys`); modify `crates/api/src/main.rs` (merge
the new router, immediately after the existing
`.merge(routes::journeys::router())` line, `main.rs:60`).

Depends on Task 2.

- [ ] **Step 1: Wire request/response types.** The `POST` create route
  uses a tagged enum exactly like `routes::journeys::CreateJourneyLegRequest`
  (`journeys.rs:72-134`) — same `#[serde(tag = "mode", rename_all =
  "camelCase", rename_all_fields = "camelCase")]` gotcha, same reasoning
  (an untagged enum's default error is unactionable; `rename_all_fields`
  is required in ADDITION to the container-level `rename_all` or every
  field silently fails to deserialize off camelCase JSON — see that type's
  own doc comment for the full explanation, and this task's own Step 5 for
  the regression test that would catch it if omitted).

```rust
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::journey_templates::{self, TemplateLegInput};
use crate::data::journeys;

pub fn router() -> Router {
    Router::new()
        .route("/JourneyTemplates", axum::routing::post(post_journey_template))
        .route("/JourneyTemplates/mine", axum::routing::get(get_my_journey_templates))
        .route(
            "/JourneyTemplates/{template_id}",
            axum::routing::get(get_journey_template)
                .put(put_journey_template)
                .delete(delete_journey_template),
        )
        .route(
            "/JourneyTemplates/{template_id}/materialize",
            axum::routing::post(post_materialize_journey_template),
        )
}

/// One manually-entered template leg on the wire -- shared by
/// `CreateJourneyTemplateRequest::Manual` and `PutJourneyTemplateRequest`.
/// Field-for-field the `window`-mode shape of `journeys::CreateJourneyLegRequest`
/// minus `service_date` (a template leg is date-less) and minus `mode`
/// itself (there's only one leg shape here, no pin/knownTrain equivalent
/// for a template -- see this plan's Judgment Call 2 for why that's fine).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateLegRequest {
    origin_crs: String,
    destination_crs: String,
    #[serde(default)]
    depart_window: TimeWindow,
    #[serde(default)]
    arrive_window: TimeWindow,
}

/// `POST /JourneyTemplates`'s two mutually-exclusive creation shapes --
/// mirrors `journeys::CreateJourneyLegRequest`'s own tagged-enum shape and
/// serde attributes exactly (same `rename_all_fields` requirement, same
/// reasoning).
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum CreateJourneyTemplateRequest {
    /// Build a new template directly from caller-supplied legs. See this
    /// plan's Judgment Call 5: the frontend built in this plan never
    /// actually sends this variant (no "start from nothing" UI ships in
    /// Phase B) -- it exists because the task brief asks for both "create"
    /// and "promote," and because `PutJourneyTemplateRequest` needs this
    /// exact leg-list shape anyway.
    Manual {
        #[serde(default)]
        custom_name: Option<String>,
        legs: Vec<TemplateLegRequest>,
    },
    /// Promotes an existing journey the caller OWNS (not merely
    /// group-shared-with -- see this plan's Judgment Call 3) into a
    /// durable template. Reads the journey's own legs
    /// (`journeys::list_legs_for_journey`) and copies each one's
    /// origin/destination/window verbatim, no `service_date`, no train
    /// binding. This is §6 item 2's "Make this a template" button.
    FromJourney {
        #[serde(default)]
        custom_name: Option<String>,
        journey_id: i64,
    },
}

/// `PUT /JourneyTemplates/{id}`'s body -- full-resource replace (Judgment
/// Calls 1 and 4). Same `TemplateLegRequest` shape as the `Manual` create
/// variant; no `mode` tag needed since there's only one shape for an edit.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutJourneyTemplateRequest {
    #[serde(default)]
    custom_name: Option<String>,
    legs: Vec<TemplateLegRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeTemplateRequest {
    /// Required, no server-side default -- matches
    /// `journeys::CreateJourneyLegRequest`'s own convention that every
    /// leg-creation wire type takes an explicit `serviceDate`, never an
    /// implicit "today". The frontend's "Run now" button (Task 9) defaults
    /// its own date field to today client-side and lets the user change
    /// it before submitting, the same "user is present and picks a date"
    /// posture the design doc's §1 describes for the whole "reusable"
    /// trigger mechanism.
    service_date: NaiveDate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyTemplateResponse {
    template_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeTemplateResponse {
    journey_id: i64,
    leg_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyTemplateLegDetailResponse {
    id: i64,
    origin_crs: Option<String>,
    origin_name: Option<String>,
    destination_crs: Option<String>,
    destination_name: Option<String>,
    depart_after: Option<NaiveTime>,
    depart_before: Option<NaiveTime>,
    arrive_after: Option<NaiveTime>,
    arrive_before: Option<NaiveTime>,
}

/// `GET /JourneyTemplates/{id}`'s response. `daysOfWeek`/`active`/
/// `startsOn`/`endsOn`/`defaultMatchMode`/`autoCommitRule` are all real,
/// round-tripped fields (so a future Phase C edit UI has somewhere to
/// read/write) -- this plan's own frontend (Task 8) renders NONE of them
/// as editable controls; see that task's own scope note.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyTemplateDetailResponse {
    id: i64,
    custom_name: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    days_of_week: Option<i16>,
    active: bool,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
    default_match_mode: String,
    auto_commit_rule: Option<String>,
    legs: Vec<JourneyTemplateLegDetailResponse>,
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "journey template request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to {operation}"))
    }
}

fn to_template_leg_input(leg: TemplateLegRequest) -> Result<TemplateLegInput, (StatusCode, String)> {
    journey_templates::validate_template_leg(&leg.origin_crs, &leg.destination_crs)
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    Ok(TemplateLegInput {
        origin_crs: Some(leg.origin_crs.trim().to_ascii_uppercase()),
        destination_crs: Some(leg.destination_crs.trim().to_ascii_uppercase()),
        depart_after: leg.depart_window.after,
        depart_before: leg.depart_window.before,
        arrive_after: leg.arrive_window.after,
        arrive_before: leg.arrive_window.before,
    })
}
```

- [ ] **Step 2: `post_journey_template`** — both creation modes:

```rust
/// `POST /JourneyTemplates` -- create (`Manual`) or promote-from-journey
/// (`FromJourney`). Rejects an empty `legs` array with 400 for `Manual`
/// mode (see `data::journey_templates::create_template`'s own
/// non-empty-invariant doc comment); rejects a source journey with any
/// leg missing an `origin_crs`/`destination_crs` with 400 for `FromJourney`
/// mode (the "no schedule data yet" gap named in Task 1's migration
/// comment -- rather than silently dropping that leg or minting a
/// half-blank template leg, this fails loudly with an actionable message).
async fn post_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<CreateJourneyTemplateRequest>,
) -> Result<Json<CreateJourneyTemplateResponse>, (StatusCode, String)> {
    match body {
        CreateJourneyTemplateRequest::Manual { custom_name, legs } => {
            if legs.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "A template needs at least one leg.".to_string(),
                ));
            }
            let leg_inputs = legs
                .into_iter()
                .map(to_template_leg_input)
                .collect::<Result<Vec<_>, _>>()?;
            let template_id = journey_templates::create_template(
                &app.database,
                &user.id,
                custom_name.as_deref(),
                &leg_inputs,
            )
            .await
            .map_err(internal_error("create journey template"))?;
            Ok(Json(CreateJourneyTemplateResponse { template_id }))
        }
        CreateJourneyTemplateRequest::FromJourney { custom_name, journey_id } => {
            // Ownership, not mere readability -- Judgment Call 3.
            let owner = journeys::journey_owner(&app.database, journey_id)
                .await
                .map_err(internal_error("check journey ownership"))?;
            if owner.as_deref() != Some(user.id.as_str()) {
                return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string()));
            }

            let source_legs = journeys::list_legs_for_journey(&app.database, journey_id)
                .await
                .map_err(internal_error("read journey legs"))?;
            if source_legs.iter().any(|leg| leg.origin_crs.is_none() || leg.destination_crs.is_none()) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "At least one leg of this journey has no known origin/destination yet — \
                     try again once its train has schedule data."
                        .to_string(),
                ));
            }
            let leg_inputs: Vec<TemplateLegInput> = source_legs
                .into_iter()
                .map(|leg| TemplateLegInput {
                    origin_crs: leg.origin_crs,
                    destination_crs: leg.destination_crs,
                    depart_after: leg.depart_after,
                    depart_before: leg.depart_before,
                    arrive_after: leg.arrive_after,
                    arrive_before: leg.arrive_before,
                })
                .collect();

            let template_id = journey_templates::create_template(
                &app.database,
                &user.id,
                custom_name.as_deref(),
                &leg_inputs,
            )
            .await
            .map_err(internal_error("create journey template from journey"))?;
            Ok(Json(CreateJourneyTemplateResponse { template_id }))
        }
    }
}
```

- [ ] **Step 3: `get_my_journey_templates`, `get_journey_template`,
  `put_journey_template`, `delete_journey_template`,
  `post_materialize_journey_template`**:

```rust
async fn get_my_journey_templates(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<journey_templates::JourneyTemplateListItem>>, (StatusCode, String)> {
    let rows = journey_templates::list_templates_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list journey templates"))?;
    Ok(Json(rows))
}

async fn get_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
) -> Result<Json<JourneyTemplateDetailResponse>, (StatusCode, String)> {
    let template = journey_templates::get_owned_template(&app.database, template_id, &user.id)
        .await
        .map_err(internal_error("read journey template"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey template with that id".to_string()))?;
    let legs = journey_templates::list_template_legs(&app.database, template_id)
        .await
        .map_err(internal_error("list journey template legs"))?;

    Ok(Json(JourneyTemplateDetailResponse {
        id: template.id,
        custom_name: template.custom_name,
        created_at: template.created_at,
        updated_at: template.updated_at,
        days_of_week: template.days_of_week,
        active: template.active,
        starts_on: template.starts_on,
        ends_on: template.ends_on,
        default_match_mode: template.default_match_mode,
        auto_commit_rule: template.auto_commit_rule,
        legs: legs
            .into_iter()
            .map(|leg| JourneyTemplateLegDetailResponse {
                id: leg.id,
                origin_crs: leg.origin_crs,
                origin_name: leg.origin_name,
                destination_crs: leg.destination_crs,
                destination_name: leg.destination_name,
                depart_after: leg.depart_after,
                depart_before: leg.depart_before,
                arrive_after: leg.arrive_after,
                arrive_before: leg.arrive_before,
            })
            .collect(),
    }))
}

async fn put_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
    Json(body): Json<PutJourneyTemplateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if body.legs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "A template needs at least one leg.".to_string(),
        ));
    }
    let leg_inputs = body
        .legs
        .into_iter()
        .map(to_template_leg_input)
        .collect::<Result<Vec<_>, _>>()?;
    let replaced = journey_templates::replace_template(
        &app.database,
        template_id,
        &user.id,
        body.custom_name.as_deref(),
        &leg_inputs,
    )
    .await
    .map_err(internal_error("replace journey template"))?;
    if !replaced {
        return Err((StatusCode::NOT_FOUND, "no journey template with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = journey_templates::delete_template(&app.database, template_id, &user.id)
        .await
        .map_err(internal_error("delete journey template"))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "no journey template with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /JourneyTemplates/{id}/materialize` -- the manual "Run now"
/// trigger. Literally `data::journey_templates::materialize_template`
/// called synchronously from this handler, no scheduler involved -- see
/// that function's own doc comment, and this plan's closing section for
/// exactly this signature's reuse contract for Phase C.
async fn post_materialize_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
    Json(body): Json<MaterializeTemplateRequest>,
) -> Result<Json<MaterializeTemplateResponse>, (StatusCode, String)> {
    let result = journey_templates::materialize_template(
        &app.database,
        template_id,
        &user.id,
        body.service_date,
    )
    .await
    .map_err(internal_error("materialize journey template"))?;
    let Some(materialized) = result else {
        return Err((StatusCode::NOT_FOUND, "no journey template with that id".to_string()));
    };
    Ok(Json(MaterializeTemplateResponse {
        journey_id: materialized.journey_id,
        leg_ids: materialized.leg_ids,
    }))
}
```

- [ ] **Step 4: Mount the router and register the module.**

  In `crates/api/src/routes/mod.rs`, add alphabetically after `journeys`:

```rust
pub mod journey_templates;
```

  In `crates/api/src/main.rs`, immediately after `main.rs:60`'s
  `.merge(routes::journeys::router())`:

```rust
        .merge(routes::journey_templates::router())
```

- [ ] **Step 5: Wire-format regression tests** (no live database — mirrors
  `routes::journeys::wire_format_tests` exactly, same reasoning: prove the
  `rename_all_fields` attribute is actually present and doing its job, not
  merely assumed):

```rust
#[cfg(test)]
mod wire_format_tests {
    use super::{CreateJourneyTemplateRequest, PutJourneyTemplateRequest};

    #[test]
    fn manual_mode_deserializes_its_camel_case_fields() {
        let request: CreateJourneyTemplateRequest = serde_json::from_str(
            r#"{
                "mode": "manual",
                "customName": "Weekday commute",
                "legs": [
                    {"originCrs": "WAT", "destinationCrs": "RDG",
                     "departWindow": {"after": "08:00:00"}}
                ]
            }"#,
        )
        .expect("valid manual-mode request should deserialize");
        assert!(matches!(request, CreateJourneyTemplateRequest::Manual { .. }));
    }

    #[test]
    fn from_journey_mode_deserializes_its_camel_case_fields() {
        let request: CreateJourneyTemplateRequest = serde_json::from_str(
            r#"{"mode": "fromJourney", "journeyId": 42}"#,
        )
        .expect("valid fromJourney-mode request should deserialize");
        assert!(matches!(request, CreateJourneyTemplateRequest::FromJourney { .. }));
    }

    #[test]
    fn put_request_deserializes_its_camel_case_fields() {
        let request: PutJourneyTemplateRequest = serde_json::from_str(
            r#"{"customName": "Renamed", "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]}"#,
        )
        .expect("valid PUT request should deserialize");
        assert_eq!(request.legs.len(), 1);
    }
}
```

- [ ] **Step 6: HTTP-layer `db_tests`**, following
  `routes::journeys::db_tests`'s own scaffolding (`test_app`/`test_router`/
  `seed_session`/`connect`/`request`/`post_json`/`delete_request` — copy
  verbatim, plus a `put_json` helper mirroring `post_json` with `method("PUT")`)
  and its own `cleanup_user` adapted to also clear `journey_templates`.
  Minimum coverage:

  - `post_journey_template_manual_mode_creates_a_template` — 200/201,
    response has a `templateId`; a follow-up `GET
    /JourneyTemplates/{templateId}` returns the leg just sent.
  - `post_journey_template_manual_mode_an_empty_legs_array_is_400`.
  - `post_journey_template_from_journey_promotes_an_owned_journey` — seed a
    journey via `journeys::create_journey_with_window_leg` (imported from
    `crate::data::journeys`, same fixture pattern
    `routes::journeys::db_tests` itself already uses for its own DB
    fixtures), promote it, assert the resulting template's one leg matches
    the source journey's origin/destination/window.
  - `post_journey_template_from_journey_a_journey_owned_by_someone_else_is_404_not_403` —
    the direct analogue of
    `routes::journeys::db_tests::get_leg_candidates_a_non_owner_gets_404`.
  - `get_journey_template_a_non_owner_gets_404`.
  - `put_journey_template_the_owner_can_replace_its_legs`.
  - `delete_journey_template_the_owner_can_delete_it`.
  - `post_materialize_journey_template_mints_a_journey_with_unmatched_legs` —
    HTTP-layer restatement of Task 2's own DB-gated test, this time through
    the route: assert `200`, `journeyId` present, then a follow-up `GET
    /Journeys/{journeyId}` (reusing `routes::journeys`'s own router merged
    into the same `test_router` — or a second `.oneshot` against a router
    built from both `super::router()` and
    `crate::routes::journeys::router()` merged, so this test can assert
    against the real downstream `GET /Journeys/{id}` shape) shows the
    expected leg count, `matchMode: "unmatched"`, and the requested
    `serviceDate`.
  - `post_materialize_journey_template_a_non_owner_gets_404`.

  Every DB-gated test gets the same `#[ignore = "requires a live database; \
  ..."]` annotation, invocation string updated to name this file's own
  functions (`cargo test -p api post_journey_template -- --ignored \
  --test-threads=1`, etc.).

- [ ] **Step 7: Verify**

```bash
cargo fmt --all
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api wire_format_tests
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api -- --ignored --test-threads=1 journey_template
cargo test --workspace
```

  Expected: everything builds and lints clean; every new unit and
  DB-gated test passes; the full non-ignored workspace test suite still
  passes (confirms nothing else broke from the `main.rs`/`mod.rs`
  changes).

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/routes/journey_templates.rs crates/api/src/routes/mod.rs crates/api/src/main.rs
git commit -m "api: add /JourneyTemplates routes (create/mine/get/put/delete/materialize)"
```

---

## Task 4: Frontend types — `frontend/lib/types.ts`

**Files:** modify `frontend/lib/types.ts`.

Depends on Task 3 (must match the real wire shapes it defines). Add these
near the existing `JourneyListItem`/`JourneyDetail`/`NewJourneyLegRequest`
block (`types.ts:765-885`), same section, same conventions (JSDoc citing
the backing Rust type, camelCase field names).

- [ ] **Step 1: Add the types**

```typescript
/** One row of `GET /JourneyTemplates/mine`
 * (`crates/api/src/data/journey_templates.rs::JourneyTemplateListItem`,
 * camelCase). Summarized the same way `app/journeys/[id]/page.tsx`'s
 * `defaultJourneyTitle` computes a journey's own fallback title —
 * first leg's origin to last leg's destination — but computed
 * server-side, since a list row has no per-leg detail to derive it from
 * client-side. `leg_count` can be `0` only for a hand-edited row; every
 * template this app's own UI creates has at least one leg. `active`/
 * `daysOfWeek` are round-tripped for Phase C but unused by anything in
 * this app today — Phase B never sets `daysOfWeek` and every template's
 * `active` is always `true`. */
export interface JourneyTemplateListItem {
  id: number;
  customName: string | null;
  createdAt: string;
  legCount: number;
  firstOriginCrs: string | null;
  firstOriginName: string | null;
  lastDestinationCrs: string | null;
  lastDestinationName: string | null;
  active: boolean;
  daysOfWeek: number | null;
}

/** One leg of `GET /JourneyTemplates/{id}`'s response
 * (`crates/api/src/routes/journey_templates.rs::JourneyTemplateLegDetailResponse`).
 * No `serviceDate`/`matchMode`/`trackedTrainState` — a template leg is
 * date-less and never itself bound to a train; contrast with
 * `JourneyLegDetail`. */
export interface JourneyTemplateLegDetail {
  id: number;
  originCrs: string | null;
  originName: string | null;
  destinationCrs: string | null;
  destinationName: string | null;
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
}

/** `GET /JourneyTemplates/{id}`'s full response. `daysOfWeek`/`active`/
 * `startsOn`/`endsOn`/`defaultMatchMode`/`autoCommitRule` are real,
 * round-tripped fields (Phase C scaffolding, per
 * docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md) —
 * this app's own Phase B UI reads none of them for anything beyond
 * display; see `app/journeys/templates/[id]/page.tsx`'s own scope note. */
export interface JourneyTemplateDetail {
  id: number;
  customName: string | null;
  createdAt: string;
  updatedAt: string;
  daysOfWeek: number | null;
  active: boolean;
  startsOn: string | null;
  endsOn: string | null;
  defaultMatchMode: 'manual' | 'auto';
  autoCommitRule: 'earliest' | 'nearestToNow' | null;
  legs: JourneyTemplateLegDetail[];
}

/** One leg in a `POST /JourneyTemplates` (`mode: 'manual'`) or
 * `PUT /JourneyTemplates/{id}` request body
 * (`crates/api/src/routes/journey_templates.rs::TemplateLegRequest`). No
 * `serviceDate` — see `JourneyTemplateLegDetail`'s own comment. */
export interface TemplateLegRequest {
  originCrs: string;
  destinationCrs: string;
  departWindow?: TimeWindow;
  arriveWindow?: TimeWindow;
}

/** `POST /JourneyTemplates`'s two mutually-exclusive request shapes
 * (`crates/api/src/routes/journey_templates.rs::CreateJourneyTemplateRequest`).
 * This app's own frontend only ever sends `fromJourney` (see
 * `components/SaveAsTemplateButton.tsx`) — `manual` exists on the wire for
 * a future "start from nothing" UI this plan does not build (see that
 * plan's Judgment Call 5), and because `PUT`'s body needs the identical
 * `TemplateLegRequest` shape regardless. */
export type CreateJourneyTemplateRequest =
  | {
      mode: 'manual';
      customName?: string;
      legs: TemplateLegRequest[];
    }
  | {
      mode: 'fromJourney';
      customName?: string;
      journeyId: number;
    };

/** `PUT /JourneyTemplates/{id}`'s request body — full-resource replace,
 * not a per-field patch (see the Phase B plan's Judgment Calls 1/4). */
export interface PutJourneyTemplateRequest {
  customName?: string;
  legs: TemplateLegRequest[];
}

/** `POST /JourneyTemplates`'s response
 * (`crates/api/src/routes/journey_templates.rs::CreateJourneyTemplateResponse`). */
export interface CreateJourneyTemplateResponse {
  templateId: number;
}

/** `POST /JourneyTemplates/{id}/materialize`'s request body — always
 * explicit, never defaulted server-side; the "Run now" button's own date
 * field (`components/RunTemplateNowButton.tsx`) defaults to today
 * client-side and lets the caller change it first. */
export interface MaterializeTemplateRequest {
  serviceDate: string; // "YYYY-MM-DD"
}

/** `POST /JourneyTemplates/{id}/materialize`'s response
 * (`crates/api/src/routes/journey_templates.rs::MaterializeTemplateResponse`).
 * `journeyId` is where `RunTemplateNowButton` navigates on success — the
 * same `/journeys/{id}` detail page any other freshly-created journey
 * lands on. */
export interface MaterializeTemplateResponse {
  journeyId: number;
  legIds: number[];
}
```

- [ ] **Step 2: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: no new type errors (pure additive types — nothing existing
  references them yet).

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "frontend: add journey template wire types"
```

---

## Task 5: Frontend API client — `frontend/lib/api.ts`

**Files:** modify `frontend/lib/api.ts`.

Depends on Task 4. Adds exactly two `GET` fetchers, mirroring
`getJourney`/`getMyJourneys`'s own shape (`api.ts:657-683`) —
**deliberately no `POST`/`PUT`/`DELETE`/materialize function added here**:
every write in this app's journeys feature is a client-side same-origin
`fetch('/api/...')` call made directly inside the button/form component
(`AddJourneyLegButton.tsx`, `ShareJourneyButton.tsx`,
`RemoveJourneyLegButton.tsx` — none of them route their `POST`/`DELETE`
through `lib/api.ts`), and Tasks 6-9's components follow the same
established pattern, not a new one.

- [ ] **Step 1: Add the two imports** to `api.ts`'s existing `import type
  { ... } from './types'` block (`api.ts:2-49`), alongside
  `JourneyDetail`/`JourneyListItem`:

```typescript
  JourneyTemplateListItem,
  JourneyTemplateDetail,
```

- [ ] **Step 2: Add the fetchers**, directly below `getMyJourneys`
  (`api.ts:673-683`):

```typescript
/** `GET /JourneyTemplates/mine` -- same `null`-on-401 "not logged in"
 * contract as `getMyJourneys` (no id in this route's path to disambiguate
 * a second way). */
export async function getMyJourneyTemplates(): Promise<JourneyTemplateListItem[] | null> {
  const url = `${baseUrl()}/JourneyTemplates/mine`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyTemplateListItem[]>;
}

/** `GET /JourneyTemplates/{id}` -- same error-mapping contract as
 * `getJourney`: throws `ApiNotFoundError` on a 404 (doesn't exist, or
 * isn't this caller's — templates have no group-shared read path in
 * Phase B, unlike a journey) and `ApiUnauthorizedError` on a 401, so
 * `app/journeys/templates/[id]/page.tsx` can render the same two distinct
 * page states `app/journeys/[id]/page.tsx` already does. */
export async function getJourneyTemplate(id: number): Promise<JourneyTemplateDetail> {
  const url = `${baseUrl()}/JourneyTemplates/${id}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyTemplateDetail>;
}
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/api.ts
git commit -m "frontend: add getMyJourneyTemplates/getJourneyTemplate fetchers"
```

---

## Task 6: `SaveAsTemplateButton` + wire into the journey detail page

**Files:** create `frontend/components/SaveAsTemplateButton.tsx`, create
`frontend/components/SaveAsTemplateButton.test.tsx`; modify
`frontend/app/journeys/[id]/page.tsx`.

Depends on Task 3 (the `POST /JourneyTemplates` route). This is §6 item
2's "Make this a template" entry point — modeled directly on
`ShareJourneyButton.tsx`'s shape (button → small modal → same-origin
`fetch()`), since both are single-purpose owner-only actions in the same
journey-detail-page header button group, not `AddJourneyLegButton.tsx`'s
heavier multi-field form.

- [ ] **Step 1: `SaveAsTemplateButton.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Stack, Text, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { CreateJourneyTemplateRequest, CreateJourneyTemplateResponse } from '@/lib/types';

/** "Make this a template" — journey-detail-page header button
 * (docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 2). Promotes THIS journey's shape into a durable template via
 * `POST /JourneyTemplates` (`mode: 'fromJourney'`) — the backend re-reads
 * this journey's own legs server-side, so this component needs nothing
 * but the journey's id, not its already-fetched leg data (contrast with
 * a hypothetical Phase A "Track this journey again" button, which pre-fills
 * a NEW journey's creation form client-side from data it already has — a
 * different operation entirely, targeting `/journeys/new`, not this
 * route). On success, navigates to the new template's own detail page —
 * the natural next stop, matching `AddJourneyLegButton`'s
 * `router.refresh()`-on-success posture but going further since a whole
 * new resource, not just an update to the current page, was created. */
export function SaveAsTemplateButton({ journeyId }: { journeyId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [customName, setCustomName] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setCustomName('');
    setError(null);
    needsLoginState.reset();
    open();
  }

  async function handleSubmit() {
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const body: CreateJourneyTemplateRequest = {
        mode: 'fromJourney',
        journeyId,
        ...(customName.trim() ? { customName: customName.trim() } : {}),
      };
      const response = await fetch('/api/JourneyTemplates', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
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
      const result: CreateJourneyTemplateResponse = await response.json();
      setSubmitting(false);
      close();
      router.push(`/journeys/templates/${result.templateId}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Make this a template
      </Button>
      <Modal opened={opened} onClose={close} title="Save this journey as a reusable template">
        <Stack>
          <Text size="sm">
            Creates a reusable template from this journey&apos;s route — you can run it again
            for a new date any time from your templates list.
          </Text>
          <TextInput
            label="Template name (optional)"
            placeholder="e.g. Weekday commute"
            value={customName}
            onChange={(event) => setCustomName(event.currentTarget.value)}
          />
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && (
            <LoginLink underline="always">Log in to save a template</LoginLink>
          )}
          <Button onClick={handleSubmit} loading={submitting}>
            Save as template
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
```

- [ ] **Step 2: Test.** Mirror `ShareJourneyButton.test.tsx`'s own shape
  (mock `fetch`, open the modal, submit, assert the request body and the
  success navigation). Cover: opens/closes; submits `{mode: 'fromJourney',
  journeyId}` with no `customName` when left blank; includes a trimmed
  `customName` when provided; a 401 shows `LoginLink`; a non-401 error
  shows the server's message text; success calls
  `router.push('/journeys/templates/{id}')` with the id from the response.

- [ ] **Step 3: Wire it into the journey detail page.** In
  `frontend/app/journeys/[id]/page.tsx`, import it alongside
  `ShareJourneyButton` (`page.tsx:9`) and render it in the same owner-only
  button group as `ShareJourneyButton`/`AddJourneyLegButton`
  (`page.tsx:184-187`):

```tsx
import { SaveAsTemplateButton } from '@/components/SaveAsTemplateButton';
```

```tsx
          {journey.isOwner && canAddLeg && (
            <AddJourneyLegButton journeyId={journey.id} priorDestinationCrs={priorDestinationCrs} />
          )}
          {journey.isOwner && <ShareJourneyButton journeyId={journey.id} />}
          {journey.isOwner && <SaveAsTemplateButton journeyId={journey.id} />}
```

  Owner-only, unconditional on `canAddLeg` (unlike `AddJourneyLegButton`)
  — promoting a journey into a template is meaningful at any point in its
  lifecycle, matched or not, single-leg or multi-leg, not gated on "the
  last leg is already matched" the way chaining a NEW leg onto the SAME
  journey is.

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- SaveAsTemplateButton
cd frontend && npm test -- journeys/\[id\]/page
cd frontend && npx tsc --noEmit
```

- [ ] **Step 5: Commit**

```bash
git add frontend/components/SaveAsTemplateButton.tsx frontend/components/SaveAsTemplateButton.test.tsx frontend/app/journeys/\[id\]/page.tsx
git commit -m "frontend: add 'Make this a template' button to the journey detail page"
```

---

## Task 7: Templates list page — `frontend/app/journeys/templates/page.tsx`

**Files:** create `frontend/app/journeys/templates/page.tsx`, create
`frontend/app/journeys/templates/page.test.tsx`, create
`frontend/components/DeleteJourneyTemplateButton.tsx`, create
`frontend/components/DeleteJourneyTemplateButton.test.tsx`.

Depends on Task 5 (`getMyJourneyTemplates`). Modeled directly on
`app/journeys/[id]/page.tsx`'s own Server Component + 401/404 handling
shape, and on `app/track/mine/page.tsx`'s `JourneyListRow`-style summary
row rendering (`track/mine/page.tsx:275-` onward) — a card per template,
linking to `/journeys/templates/{id}`.

- [ ] **Step 1: `page.tsx`**

```tsx
import { Anchor, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyJourneyTemplates } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyTemplateListItem } from '@/lib/types';

export const revalidate = 0;

/** `/journeys/templates` -- the templates list, per
 * docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 3. `getMyJourneyTemplates()` returns `null` on a 401 (not "no
 * templates"), same "not logged in at all" signal `getMyJourneys`/
 * `getMyTrackedTrains` already use -- this page shows a login prompt for
 * that case, an empty state for a real logged-in-but-templateless caller. */
export default async function JourneyTemplatesPage() {
  const templates = await getMyJourneyTemplates();

  if (templates === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Your journey templates</Title>
        <LoginLink underline="always">Log in to see your journey templates</LoginLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/track/mine" underline="always">
        Back to my trains &amp; journeys
      </TextLink>
      <Title order={1}>Your journey templates</Title>
      {templates.length === 0 && (
        <Text c="dimmed">
          No templates yet. Open a journey and choose &quot;Make this a template&quot; to save
          its shape for reuse.
        </Text>
      )}
      {templates.map((template) => (
        <TemplateCard key={template.id} template={template} />
      ))}
    </Stack>
  );
}

function TemplateCard({ template }: { template: JourneyTemplateListItem }) {
  const route = routeLabel(
    template.firstOriginCrs,
    template.firstOriginName,
    template.lastDestinationCrs,
    template.lastDestinationName,
  );
  const title = template.customName ?? route;
  return (
    <Card withBorder>
      <Group justify="space-between">
        <Stack gap={2}>
          <Anchor component={Link} href={`/journeys/templates/${template.id}`} fw={600}>
            {title}
          </Anchor>
          {template.customName && (
            <Text size="sm" c="dimmed">
              {route}
            </Text>
          )}
          <Text size="xs" c="dimmed">
            {template.legCount} {template.legCount === 1 ? 'leg' : 'legs'} · saved{' '}
            {formatDate(template.createdAt)}
          </Text>
        </Stack>
      </Group>
    </Card>
  );
}
```

  (`routeLabel`/`formatDate` are the same existing helpers
  `app/journeys/[id]/page.tsx`/`app/track/mine/page.tsx` already import —
  no new date/route-label formatting logic needed.)

- [ ] **Step 2: `DeleteJourneyTemplateButton.tsx`** — mirrors
  `RemoveJourneyLegButton.tsx`'s confirm-modal shape exactly (same
  `Modal` + `useDisclosure` + `useNeedsLogin`, same
  `aria-label="Confirm ..."` convention), used on both the list page (a
  quick per-card delete, if the product wants it — see Step 3's note) and
  the detail page (Task 8):

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Deletes a journey template the caller owns
 * (`DELETE /JourneyTemplates/{id}`). Every journey this template ever
 * produced (`journeys.source_template_id`) survives untouched — see
 * `crates/api/src/data/journey_templates.rs::delete_template`'s own doc
 * comment (`ON DELETE SET NULL`, not a cascade) — so the confirm copy
 * below says so explicitly, unlike `RemoveJourneyLegButton`'s "this
 * cannot be undone" alone, since a caller might otherwise reasonably fear
 * losing journeys they've already run from this template. Always
 * redirects to `/journeys/templates` on success (mirrors
 * `RemoveJourneyLegButton`'s `afterDelete`-style "closest surviving list"
 * target for a delete that removes the CURRENT page's own object) rather
 * than a bare `router.refresh()`, since this component is used both on
 * the list (Task 7) and the detail page (Task 8) and the detail page
 * itself is gone after a successful delete. */
export function DeleteJourneyTemplateButton({ templateId }: { templateId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/JourneyTemplates/${templateId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.push('/journeys/templates');
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete template
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this template?">
        <Text>
          This cannot be undone. Any journeys you&apos;ve already run from this template are
          NOT deleted — only the reusable template itself.
        </Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to delete this template</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button
            color="red"
            onClick={handleDelete}
            loading={deleting}
            aria-label="Confirm delete template"
          >
            Delete template
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

  **Scope note**: this plan wires `DeleteJourneyTemplateButton` onto the
  detail page (Task 8) only, not onto each list-page card — deleting from
  a card the user hasn't opened yet risks a misclick against the wrong
  similarly-named template far more than deleting from a page that's
  already showing that template's full leg list. Built here (Task 7)
  because it's the natural place to introduce the component file
  alongside the list page it's most related to; wired in Task 8.

- [ ] **Step 3: Tests.** `page.test.tsx` mirrors
  `app/journeys/[id]/page.test.tsx`'s own mocking shape (mock
  `getMyJourneyTemplates`): renders a login prompt on `null`; renders the
  empty-state copy on `[]`; renders one card per template with the right
  title (custom name vs. computed route fallback) and leg count.
  `DeleteJourneyTemplateButton.test.tsx` mirrors
  `RemoveJourneyLegButton.test.tsx`'s shape: confirm flow, 401 handling,
  non-401 error text, success redirect to `/journeys/templates`.

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- journeys/templates/page
cd frontend && npm test -- DeleteJourneyTemplateButton
cd frontend && npx tsc --noEmit
```

- [ ] **Step 5: Commit**

```bash
git add frontend/app/journeys/templates/page.tsx frontend/app/journeys/templates/page.test.tsx frontend/components/DeleteJourneyTemplateButton.tsx frontend/components/DeleteJourneyTemplateButton.test.tsx
git commit -m "frontend: add /journeys/templates list page and delete button"
```

---

## Task 8: Detail/edit page — `frontend/app/journeys/templates/[id]/page.tsx`

**Files:** create `frontend/app/journeys/templates/[id]/page.tsx`, create
`frontend/app/journeys/templates/[id]/page.test.tsx`, create
`frontend/components/EditJourneyTemplateForm.tsx`, create
`frontend/components/EditJourneyTemplateForm.test.tsx`, create
`frontend/components/RunTemplateNowButton.tsx`, create
`frontend/components/RunTemplateNowButton.test.tsx`.

Depends on Task 5 (`getJourneyTemplate`), Task 7
(`DeleteJourneyTemplateButton`). This is §6 item 3's detail page — "edit
origin/destination/windows per leg... same `TimeFilterInput`/
`TrainSearchForm` before/after UI... reused verbatim, not reinvented" —
plus the "Run now" button. **Scope note, restating this plan's Non-goals**:
this page does NOT render a day-of-week picker, the three-way match-mode
chooser, or a Pause toggle — those are real response fields
(`JourneyTemplateDetail.daysOfWeek`/`active`/`defaultMatchMode`/
`autoCommitRule`) this page simply doesn't surface any control for yet,
matching Phase B's "on-demand only" boundary. `startsOn`/`endsOn` are
likewise not rendered.

- [ ] **Step 1: `RunTemplateNowButton.tsx`** — the "Run now" trigger, §6
  item 3:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Stack, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { MaterializeTemplateRequest, MaterializeTemplateResponse } from '@/lib/types';

/** "Run now" — the manual, on-demand materialization trigger
 * (`POST /JourneyTemplates/{id}/materialize`), §6 item 3. Defaults its
 * date field to today (`dayjs().format('YYYY-MM-DD')`, same convention
 * `AddJourneyLegButton`'s own `handleOpen` already uses) but always sends
 * it explicitly — there is no implicit "today" on the wire
 * (`MaterializeTemplateRequest.serviceDate` is required, matching
 * `crates/api/src/routes/journey_templates.rs::MaterializeTemplateRequest`'s
 * own doc comment). Clicking this MORE THAN ONCE for the same date is
 * explicitly supported, not blocked — see the Phase B plan's Judgment
 * Call 7 — so this component adds no "already ran today" guard of its
 * own. On success, navigates straight to the freshly-minted journey's own
 * detail page (`/journeys/{journeyId}`) — the natural next step is
 * picking a candidate for each newly-unmatched leg there. */
export function RunTemplateNowButton({ templateId }: { templateId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [serviceDate, setServiceDate] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setServiceDate(dayjs().format('YYYY-MM-DD'));
    setError(null);
    needsLoginState.reset();
    open();
  }

  async function handleSubmit() {
    if (!serviceDate) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const body: MaterializeTemplateRequest = { serviceDate };
      const response = await fetch(`/api/JourneyTemplates/${templateId}/materialize`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
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
      const result: MaterializeTemplateResponse = await response.json();
      setSubmitting(false);
      close();
      router.push(`/journeys/${result.journeyId}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button onClick={handleOpen}>Run now</Button>
      <Modal opened={opened} onClose={close} title="Run this template">
        <Stack>
          <TextInput
            label="Service date"
            placeholder="YYYY-MM-DD"
            value={serviceDate}
            onChange={(event) => setServiceDate(event.currentTarget.value)}
          />
          {error && <Alert color="red">{error}</Alert>}
          {needsLoginState.needsLogin && (
            <LoginLink underline="always">Log in to run this template</LoginLink>
          )}
          <Button onClick={handleSubmit} disabled={!serviceDate} loading={submitting}>
            Create journey
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
```

- [ ] **Step 2: `EditJourneyTemplateForm.tsx`** — the leg editor,
  submitting a full `PUT` replace. Reuses `TimeFilterInput` exactly as
  `AddJourneyLegButton.tsx` does for its own window fields (design doc
  §6 item 3's explicit instruction: "same `TimeFilterInput`... before/after
  UI... reused verbatim, not reinvented"); supports add/remove-leg
  client-side before a single Save submits the whole array:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Alert, Button, Group, Stack, Text, TextInput } from '@mantine/core';
import { IconTrash } from '@tabler/icons-react';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { TimeFilterInput } from './TimeFilterInput';
import type { JourneyTemplateDetail, PutJourneyTemplateRequest, TemplateLegRequest } from '@/lib/types';

interface EditableLeg {
  originCrs: string;
  destinationCrs: string;
  departFrom: string;
  departTo: string;
  arriveFrom: string;
  arriveTo: string;
}

function toEditableLeg(leg: JourneyTemplateDetail['legs'][number]): EditableLeg {
  return {
    originCrs: leg.originCrs ?? '',
    destinationCrs: leg.destinationCrs ?? '',
    departFrom: leg.departAfter ?? '',
    departTo: leg.departBefore ?? '',
    arriveFrom: leg.arriveAfter ?? '',
    arriveTo: leg.arriveBefore ?? '',
  };
}

/** The template detail page's leg editor -- §6 item 3: "edit
 * origin/destination/windows per leg." Submits the WHOLE leg list on
 * every Save (`PUT /JourneyTemplates/{id}`, a full-resource replace, not
 * a per-leg patch — see the Phase B plan's Judgment Calls 1/4), so
 * add/remove/edit are all plain client-side array operations until Save
 * is pressed; nothing is persisted mid-edit. Deliberately does NOT render
 * a day-of-week picker, match-mode chooser, or Pause toggle — see this
 * page's own scope note in the Phase B plan (Task 8). */
export function EditJourneyTemplateForm({ template }: { template: JourneyTemplateDetail }) {
  const router = useRouter();
  const [customName, setCustomName] = useState(template.customName ?? '');
  const [legs, setLegs] = useState<EditableLeg[]>(template.legs.map(toEditableLeg));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const needsLoginState = useNeedsLogin();

  function updateLeg(index: number, patch: Partial<EditableLeg>) {
    setSaved(false);
    setLegs((current) => current.map((leg, i) => (i === index ? { ...leg, ...patch } : leg)));
  }

  function addLeg() {
    setSaved(false);
    setLegs((current) => [
      ...current,
      { originCrs: '', destinationCrs: '', departFrom: '', departTo: '', arriveFrom: '', arriveTo: '' },
    ]);
  }

  function removeLeg(index: number) {
    setSaved(false);
    setLegs((current) => current.filter((_, i) => i !== index));
  }

  const isValid =
    legs.length > 0 && legs.every((leg) => leg.originCrs.trim() !== '' && leg.destinationCrs.trim() !== '');

  async function handleSave() {
    if (!isValid) return;
    setSubmitting(true);
    setError(null);
    setSaved(false);
    needsLoginState.reset();
    try {
      const body: PutJourneyTemplateRequest = {
        ...(customName.trim() ? { customName: customName.trim() } : {}),
        legs: legs.map(
          (leg): TemplateLegRequest => ({
            originCrs: leg.originCrs.trim(),
            destinationCrs: leg.destinationCrs.trim(),
            departWindow: { after: leg.departFrom || null, before: leg.departTo || null },
            arriveWindow: { after: leg.arriveFrom || null, before: leg.arriveTo || null },
          }),
        ),
      };
      const response = await fetch(`/api/JourneyTemplates/${template.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
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
      setSubmitting(false);
      setSaved(true);
      router.refresh();
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack>
      <TextInput
        label="Template name"
        placeholder="e.g. Weekday commute"
        value={customName}
        onChange={(event) => setCustomName(event.currentTarget.value)}
      />
      {legs.map((leg, index) => (
        <Stack key={index} gap="xs" p="sm" style={{ border: '1px solid var(--mantine-color-gray-3)' }}>
          <Group justify="space-between">
            <Text size="sm" fw={600}>
              Leg {index + 1}
            </Text>
            {legs.length > 1 && (
              <ActionIcon
                variant="subtle"
                color="red"
                aria-label={`Remove leg ${index + 1}`}
                onClick={() => removeLeg(index)}
              >
                <IconTrash size={16} />
              </ActionIcon>
            )}
          </Group>
          <Group grow>
            <TextInput
              label="Origin CRS"
              value={leg.originCrs}
              onChange={(event) => updateLeg(index, { originCrs: event.currentTarget.value })}
            />
            <TextInput
              label="Destination CRS"
              value={leg.destinationCrs}
              onChange={(event) => updateLeg(index, { destinationCrs: event.currentTarget.value })}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest departure (optional)"
              name={`leg-${index}-depart-from`}
              description="Only trains leaving at or after this time."
              value={leg.departFrom}
              onChange={(v) => updateLeg(index, { departFrom: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
            <TimeFilterInput
              label="Latest departure (optional)"
              name={`leg-${index}-depart-to`}
              description="Only trains leaving at or before this time."
              value={leg.departTo}
              onChange={(v) => updateLeg(index, { departTo: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name={`leg-${index}-arrive-from`}
              description="Only trains reaching the destination at or after this time."
              value={leg.arriveFrom}
              onChange={(v) => updateLeg(index, { arriveFrom: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
            <TimeFilterInput
              label="Latest arrival (optional)"
              name={`leg-${index}-arrive-to`}
              description="Only trains reaching the destination at or before this time."
              value={leg.arriveTo}
              onChange={(v) => updateLeg(index, { arriveTo: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
          </Group>
        </Stack>
      ))}
      <Button variant="default" onClick={addLeg}>
        Add another leg
      </Button>
      {error && <Alert color="red">{error}</Alert>}
      {saved && <Alert color="green">Saved.</Alert>}
      {needsLoginState.needsLogin && <LoginLink underline="always">Log in to save changes</LoginLink>}
      <Button onClick={handleSave} disabled={!isValid} loading={submitting}>
        Save changes
      </Button>
    </Stack>
  );
}
```

  (The plan deliberately drops the half-typed-time `incompleteTimes`
  tracking `AddJourneyLegButton` uses to block submission on an
  incomplete time — acceptable here because `validate_template_leg`
  (Task 2) has no "at least one bound" requirement to violate, so a
  half-typed time simply reports as `''`/untouched, same as
  `TimeFilterInput`'s own documented behavior, and is sent as `null`,
  which is always valid for a template leg. Note this explicitly as a
  minor, deliberate simplification versus `AddJourneyLegButton`'s fuller
  validation, not an oversight.)

- [ ] **Step 3: `page.tsx`**

```tsx
import { Group, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourneyTemplate, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { DeleteJourneyTemplateButton } from '@/components/DeleteJourneyTemplateButton';
import { EditJourneyTemplateForm } from '@/components/EditJourneyTemplateForm';
import { LoginLink } from '@/components/LoginLink';
import { RunTemplateNowButton } from '@/components/RunTemplateNowButton';
import { TextLink } from '@/components/TextLink';

export const revalidate = 0;

/** `/journeys/templates/[id]` -- detail/edit view, §6 item 3. Templates
 * have no group-shared read path in Phase B (unlike `/journeys/[id]`,
 * which does) -- every field and control here is owner-only by
 * construction, since `GET /JourneyTemplates/{id}` itself 404s for
 * anyone but the owner. */
export default async function JourneyTemplateDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!/^\d+$/.test(id)) {
    notFound();
  }

  let template;
  try {
    template = await getJourneyTemplate(Number(id));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Someone&apos;s journey template — log in to see it</Title>
          <LoginLink underline="always">Log in to view this template</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/journeys/templates" underline="always">
        Back to your templates
      </TextLink>
      <Group justify="space-between" align="baseline">
        <Title order={1}>{template.customName ?? 'Journey template'}</Title>
        <Group gap="xs">
          <RunTemplateNowButton templateId={template.id} />
          <DeleteJourneyTemplateButton templateId={template.id} />
        </Group>
      </Group>
      <EditJourneyTemplateForm template={template} />
    </Stack>
  );
}
```

- [ ] **Step 4: Tests.** `page.test.tsx` mirrors
  `app/journeys/[id]/page.test.tsx`'s own mocking shape (mock
  `getJourneyTemplate`): 401 renders the login prompt; a thrown
  `ApiNotFoundError` calls `notFound()`; a successful fetch renders the
  title, the leg editor pre-filled with the template's legs, and both
  header buttons. `EditJourneyTemplateForm.test.tsx` covers: pre-fills
  from `template`; add/remove leg client-side; Save disabled when any leg
  is missing origin/destination; successful `PUT` shows the "Saved." alert
  and calls `router.refresh()`; 401 shows `LoginLink`; a non-401 error
  shows the server's message. `RunTemplateNowButton.test.tsx` covers:
  defaults the date field to today on open; submits `{serviceDate}`;
  success navigates to `/journeys/{journeyId}` using the response's
  `journeyId`; 401 shows `LoginLink`; error text surfaced.

- [ ] **Step 5: Verify**

```bash
cd frontend && npm test -- journeys/templates/\[id\]/page
cd frontend && npm test -- EditJourneyTemplateForm
cd frontend && npm test -- RunTemplateNowButton
cd frontend && npx tsc --noEmit
cd frontend && npm test
cd frontend && npm run build
```

  Expected: everything passes; `npm run build` succeeds (confirms the two
  new dynamic routes — `/journeys/templates`, `/journeys/templates/[id]`
  — compile cleanly alongside everything else).

  **Manual UI verification** (per this repo's standing practice, no
  end-to-end coverage exists for this flow): start the dev stack, log in,
  open a real journey, click "Make this a template," follow the redirect
  to the new template's detail page, edit a leg's origin and save, confirm
  the change persists on refresh, click "Run now" with today's date,
  confirm the redirect lands on a real new `/journeys/{id}` page showing
  unmatched legs ready to pick a candidate, then go back and delete the
  template and confirm the previously-created journey is still reachable
  and unaffected.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/journeys/templates/\[id\]/page.tsx frontend/app/journeys/templates/\[id\]/page.test.tsx frontend/components/EditJourneyTemplateForm.tsx frontend/components/EditJourneyTemplateForm.test.tsx frontend/components/RunTemplateNowButton.tsx frontend/components/RunTemplateNowButton.test.tsx
git commit -m "frontend: add journey template detail/edit page with Run now"
```

---

## Task 9: Nav wiring — link from `/track/mine` to `/journeys/templates`

**Files:** modify `frontend/app/track/mine/page.tsx`.

Depends on Task 7. `/track/mine` is where `getMyJourneys()` already
renders a "Your journeys" section (`track/mine/page.tsx:198-` onward,
per the file read for this plan) — the natural, lowest-friction place to
surface the new templates list, matching how that same page already
became the single hub linking out to `/journeys/{id}` for each journey
row. This plan deliberately does **not** add a top-level `navLinks.ts`
destination (`frontend/lib/navLinks.ts`) — templates are a sub-feature of
the already-nested journeys/tracking area, not a first-class nav
destination the way "Find a Train" or "Groups" are; the design doc's own
§6 never proposes a nav-bar entry either.

- [ ] **Step 1: Add a link.** In `frontend/app/track/mine/page.tsx`,
  immediately after the existing "Your journeys" section header (the
  `{journeyRows.length > 0 && (...)}` block, `page.tsx:198-` per this
  plan's own earlier read), add a small text link to
  `/journeys/templates` — visible whenever the caller is logged in at all
  (not gated on `journeyRows.length > 0`, since a user with zero current
  journeys may still have durable templates worth reusing):

```tsx
import { TextLink } from '@/components/TextLink';
```

```tsx
      <TextLink href="/journeys/templates" underline="always">
        Manage your journey templates
      </TextLink>
```

  Place this near the top of the authenticated content (alongside the
  page's other cross-links), not buried inside the `journeyRows.length >
  0` conditional — the whole point is discoverability for a user who
  doesn't currently have any active journeys but does have a saved
  template.

- [ ] **Step 2: Verify**

```bash
cd frontend && npm test -- track/mine/page
cd frontend && npx tsc --noEmit
```

- [ ] **Step 3: Commit**

```bash
git add frontend/app/track/mine/page.tsx
git commit -m "frontend: link /track/mine to the journey templates list"
```

---

## What Phase C needs to reuse from this plan

Phase C (recurrence + the automated sweep + real `'auto'` matching,
planned separately, not built here) needs exactly one thing from this
plan's backend work, and should NOT duplicate it:

**`crate::data::journey_templates::materialize_template`**
(`crates/api/src/data/journey_templates.rs`, Task 2 Step 8):

```rust
pub async fn materialize_template(
    pool: &PgPool,
    template_id: i64,
    user_id: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<MaterializedJourney>>
```

This is the exact function Task 3's `POST /JourneyTemplates/{id}/materialize`
route calls synchronously. Phase C's sweep (most naturally a new branch in
`crates/notifier`'s existing interval-driven loop, per the design doc's
§3.1/§0.4 precedent — not this plan's concern to build) should call this
**same function**, once per due template, rather than reimplementing the
journeys/journey_legs insert logic a second time. Three things Phase C's
own planning pass needs to know about this signature, all already true
today and none requiring a change to this function:

1. **`user_id` is always the template row's own `user_id` column** for a
   sweep caller — there is no "authenticated caller" in a background
   sweep, but the parameter exists (and the ownership check inside
   `get_owned_template` runs) regardless, so the sweep can pass the same
   `user_id` it already read off the due-templates query with zero special
   -casing. This keeps exactly one code path for "materialize this
   template," used identically by a human's click and the sweep's own
   loop.
2. **No idempotency guard lives inside this function — Phase C must add
   its own** (Judgment Call 7). The sweep's own due-template query needs an
   extra clause equivalent to "`AND NOT EXISTS (SELECT 1 FROM journeys
   WHERE source_template_id = jt.id AND EXISTS (SELECT 1 FROM journey_legs
   WHERE journey_id = journeys.id AND service_date = <today>))`" (or a
   simpler variant keyed off any one leg's `service_date`, since every leg
   materialize_template mints for one call shares the same `service_date`)
   — this is new code Phase C must write, not something to retrofit into
   `materialize_template` itself, since Phase B's manual "Run now" path
   correctly has no such restriction.
3. **`default_match_mode`/`auto_commit_rule` are already round-tripped by
   every Phase B route (`GET`/`PUT`) but never read by
   `materialize_template`.** Phase C's own version of this concern —
   seeding a leg's `match_mode` from the template's `default_match_mode`,
   and (for `'auto'` + `'nearest_to_now'`) deferring the actual candidate
   commit to a second, closer-to-window check per the design doc's
   now-resolved §3.2 mechanics — is new logic that belongs in a **new**
   function (or a parameterized variant of this one), not a silent change
   to `materialize_template`'s existing behavior: Phase B's own manual
   "Run now" trigger must keep minting `'unmatched'` legs unconditionally
   forever, regardless of what Phase C eventually builds for the automated
   path, per this plan's own DB-gated test
   (`materialize_template_mints_a_journey_with_unmatched_legs_dated_the_target_date`,
   Task 2 Step 10) asserting exactly that even when `default_match_mode =
   'auto'`.

Nothing else in this plan's frontend or route layer is expected to be
reused by Phase C directly — Phase C adds its own scheduler-side code path
entirely, and (per the design doc's own phasing) its own frontend surface
for the day-of-week picker, match-mode chooser, and Pause toggle this
plan's `EditJourneyTemplateForm` deliberately leaves unrendered.
