# Plan: Custom Names for Tracked Trains and Tickets

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement
`docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md` (the
approved spec) end to end: a nullable `custom_name` column on `tracked_trains`
and `tracked_train_tickets`, two new write routes, and a Rename
button → modal → fetch UI matching this app's existing
`DeleteTrainButton`/`DeleteTicketButton` convention, so a user's `/track/mine`
list can show something other than a wall of identical route/date rows.

**Architecture:** backend before frontend, since the frontend depends on the
new field/routes existing. One migration adds the column to both tables
(Task 2). One small `crates/common` constant (Task 1) is the single source of
truth for the 100-character cap, imported by the one Rust validator that
enforces it today. `crates/api/src/data/train_tracking.rs` gets a pure
validator plus two `UPDATE ... WHERE id = $1 AND user_id = $2` write functions
(Task 3), mirroring `custom_lines::update_custom_line` and
`delete_tracked_train` exactly. Two new `POST .../name` routes (Task 4) expose
them, following this router's own "narrow action sub-path" convention rather
than introducing `PATCH` (Judgment Call 2, below). `custom_name` is then
threaded through every existing read path's `SELECT`/struct (Task 5), so it's
visible wherever a tracked train or ticket is already read. The frontend gets
the field on its four wire types (Task 6), a client-side default-name helper
that reuses `routeLabel`/`formatDate`/`formatTime` exactly as the two existing
default-label call sites already do (Task 7), two small Rename button
components modeled directly on `DeleteTrainButton`/`DeleteTicketButton`
(Task 8), and finally gets wired into the three real render sites — tracked
trains (Task 9), then tickets (Task 10).

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md`
— authoritative for every architectural decision this plan implements
(schema shape, privacy-audit resolution, default-name-computed-client-side
reasoning, UI pattern). This plan does not re-argue anything that spec already
settled; it only resolves the three things the spec explicitly left for "the
implementation plan."

---

## Judgment calls this plan makes (read before Task 1)

The spec's own "Open questions / risks" section named three things as
genuine judgment calls for this plan, not decided there. Resolved here,
each checked against the real current worktree state rather than re-guessed:

1. **Should the 100-character cap live as a shared `crates/common` constant,
   rather than a bare `100` literal? Yes — `common::CUSTOM_NAME_MAX_LENGTH`
   (Task 1).** Verified there is exactly one existing precedent for a
   cross-cutting `pub const` in `crates/common` doing this same job:
   `TFL_OPERATOR`/`TFL_LINE_ID_PREFIX`
   (`crates/common/src/lib.rs:231,237`), both plain constants used from
   `crates/api` and `crates/poller-tfl`. `crates/api/src/data/train_tracking.rs`
   already has the opposite anti-pattern flagged in its own doc comments —
   `MAX_PIN_AGE`/`MINE_LIST_LIMIT` (`train_tracking.rs:14-38`) are *local*
   constants, fine since nothing else needs them, but the whole point of a
   length cap that a future frontend character-counter would also need to
   know is that duplicating the bare `100` between the Rust validator and a
   TypeScript component is exactly the kind of drift `crates/common`'s
   existing constants exist to prevent. One real limitation, stated plainly
   rather than glossed: **there is no Rust→TypeScript constant bridge
   anywhere in this codebase** (no codegen, no shared JSON schema) — so
   `common::CUSTOM_NAME_MAX_LENGTH` only closes the loop on the Rust side
   today (the validator is the only consumer this plan adds). This plan's
   own Non-goal (no character-counter UI, per the spec's Non-goals) means
   there is no second copy to keep in sync *yet* — when one is eventually
   added, it will have to hardcode `100` in TypeScript with a comment
   pointing back to this constant, the same two-language duplication this
   codebase already accepts for other cross-cutting numbers (e.g. the 8 MiB
   upload cap duplicated as a comment, `crates/api/src/routes/train.rs:90-97`,
   with no frontend-side enforcement at all). Put simply: this call makes
   the Rust side correct now and leaves an honest, documented seam for
   later, rather than inventing a code-generation mechanism this repo has
   never used for a one-off number.

2. **`PATCH` or `POST .../name`? `POST .../name` — no `PATCH` route
   anywhere in this plan.** Verified directly:
   `grep -rn "axum::routing::patch\|\.patch(" crates/api/src/routes/` returns
   nothing — zero `PATCH` routes exist in this codebase today. Verified the
   two real precedents the spec named actually exist as described:
   `crates/api/src/routes/lines.rs:28-36` mounts
   `.route("/lines/{id}", ... .put(update_line) ...)` (a full-resource
   replace — `update_line` takes every editable field of a custom line at
   once), and `crates/api/src/routes/preferences.rs:24,28` mount two more
   `PUT` routes (`put_pinned_lines`/`put_pinned_stations`, also full-resource
   replaces of a preference list). Every *narrow*, single-concern mutation
   in this router, by contrast, is `POST` to a literal sub-path:
   `POST /Train/tickets/{ticket_id}/attach`
   (`crates/api/src/routes/train.rs:58-61`) is the closest structural
   match — one field changes (`tracked_train_id`), ownership-scoped, 404 for
   "doesn't exist or isn't yours," never a resource-replace. A tracked
   train's `custom_name` is exactly this shape: one field, on an otherwise
   write-once-at-creation-then-only-trust-consumer-touches-it row, with no
   "replace everything" concept the way `update_line`'s full `CreateLineRequest`
   body has. Introducing `PATCH` — a verb this router has never used, for
   one feature — adds vocabulary for no benefit `POST .../name` doesn't
   already provide; matching the router's own overwhelming convention (every
   narrow action other than the two full-resource `PUT`s is `POST`) is the
   better fit. Confirmed this needs no proxy change either way:
   `frontend/app/api/[...path]/route.ts:76` forwards `req.method` verbatim
   and only special-cases `GET`/`DELETE` for body handling
   (`route.ts:96-97`) — a `POST` (or a `PATCH`, had that been chosen) with a
   JSON body passes through identically to every existing `POST` mutation
   already routed through this proxy.

3. **Is "submit an empty text input" adequate for clearing a name, given the
   spec's own visible "Clear" button (Decision 5)? Adequate, with one
   refinement: `RenameTrainButton`/`RenameTicketButton`'s Save button is
   disabled whenever the trimmed input is empty.** Reasoning: the backend
   normalizes an empty-after-trim `customName` to `NULL` on *any* successful
   write (Decision 1), so a distinct "Clear" button alone does not, by
   itself, prevent "user backspace-select-alls the field by accident and
   hits Save" — that path was always going to clear the name regardless of
   whether a separate Clear button also exists. Disabling Save on an empty
   input closes that exact gap: clearing a name now requires the visible,
   distinctly-labeled "Clear" action (which needs no typing at all — it
   posts `{ customName: null }` directly, independent of the text field's
   current contents), while Save can never silently do the same thing by
   accident. This is a small, mechanical addition (a `disabled` prop keyed
   off the trimmed input length) to the button component this plan builds
   in Task 8 anyway — not a second confirmation dialog, which would be
   disproportionate for a reversible, non-destructive action a "Clear"
   click already un-does in one more click.

---

## Non-goals

(Restated from the spec's own Non-goals — this plan does not re-litigate
any of these; tasks below have no step touching them.)

- **`ticket_extraction.rs` never populates `custom_name`.** No task in this
  plan touches that file. See Global Constraints.
- **No rename-at-creation-time UI** (`TrackTrainForm.tsx`/
  `TicketEntryForm.tsx` are untouched).
- **No search/filter by custom name** on `/track/mine`.
- **No history of previous custom names** — a rename overwrites in place.
- **No character-counter UI** — see Judgment Call 1 above for why this
  leaves an honest, documented seam rather than a broken promise.
- **No change to `TrackedTrainRef`/`TrackedTrainRow`** (the
  trust-consumer-facing poller shape, `crates/api/src/data/train_tracking.rs:241-264`)
  — it has no user-facing rendering need for a name, and Task 5 explicitly
  does not touch it (see that task's own guardrail).
- **No proxy change** (`frontend/app/api/[...path]/route.ts`) — Judgment
  Call 2 confirms the existing catch-all already forwards a `POST` with a
  JSON body correctly.

## Global Constraints

- **`ticket_extraction.rs` must never be touched by this plan, and no task
  may populate `custom_name` from it.** This is the load-bearing half of the
  spec's Decision 2 privacy-audit resolution: `custom_name` is only ever
  user-typed, never inferred. If a future change ever makes
  `ticket_extraction.rs` write to this column, the "never extracted from the
  document" half of that reasoning stops being true — flagged here so it
  isn't silently violated by later convenience code.
- **Every new route follows the 404-never-403 ownership convention** stated
  in `crates/api/src/routes/train.rs`'s own module doc (lines 1-13):
  "doesn't exist" and "exists but isn't yours" are indistinguishable to the
  caller. Both new routes fold the ownership check directly into the
  `UPDATE ... WHERE id = $1 AND user_id = $2` clause (Task 3), never a
  separate ownership `SELECT` followed by an unscoped write.
- **The migration's privacy-audit-addendum comment (Task 2) must be included
  verbatim** (or word-for-word equivalent) — this is not optional polish;
  it is how a future auditor re-reading this table's audit list doesn't have
  to re-derive Decision 2's reasoning from scratch.
- **100 characters, app-layer only, no DB `CHECK` constraint** (spec
  Decision 1) — enforced once, in `train_tracking::validate_custom_name`
  (Task 3), never duplicated as a second bound anywhere else.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (this repo's actual CI
  invocation — see `.github/workflows/ci.yml`'s `clippy` job, which pins
  `auguwu/clippy-action@9817d076b82df0194935be9db6154c56ac07b317` with
  `--workspace --all-features`; this plan additionally asks for
  `--all-targets` locally, matching the stricter default a contributor
  should run before pushing), `cargo test --workspace` (ignored tests
  skipped, the fast default CI already runs unconditionally —
  `.github/workflows/ci.yml:219-220`), and
  `cargo test -p api -- --ignored --test-threads=1` for every DB-gated test
  this plan adds (the exact invocation CI uses,
  `.github/workflows/ci.yml:229-230`, requiring
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres` against
  a real local Postgres — see that workflow's own `services:` block for how
  CI stands one up; a contributor running this locally needs an equivalent
  running Postgres). Frontend: `npm test -- <file>` (vitest,
  `.github/workflows/ci.yml:268-269`'s `npm test`) per changed test file,
  plus a full `npm test` and `npm run build` before considering the frontend
  tasks done, matching CI's own two frontend steps.
  **UI verification**: per this repo's standing practice for a change with
  no automated end-to-end coverage, start the dev stack and manually verify
  in a real browser — rename a real tracked train and a real ticket, confirm
  the un-renamed rows still show their computed default label, and confirm
  Clear reverts a renamed row back to that default. Folded into Tasks 9/10's
  own Verify steps below, not a separate task.
- **File scope.** Modified/created:
  `crates/common/src/lib.rs`,
  `crates/api/migrations/20260905130000_custom_tracking_names.sql` (new),
  `crates/api/src/data/train_tracking.rs`,
  `crates/api/src/routes/train.rs`,
  `frontend/lib/types.ts`,
  `frontend/lib/trackingName.ts` (new),
  `frontend/lib/trackingName.test.ts` (new),
  `frontend/components/RenameTrainButton.tsx` (new),
  `frontend/components/RenameTrainButton.test.tsx` (new),
  `frontend/components/RenameTicketButton.tsx` (new),
  `frontend/components/RenameTicketButton.test.tsx` (new),
  `frontend/app/track/mine/page.tsx`,
  `frontend/components/TrainJourney.tsx`,
  `frontend/components/TicketSummary.tsx`,
  `frontend/components/TicketPanel.tsx`,
  `frontend/app/train/by-id/[trackingId]/page.tsx`,
  `frontend/app/train/[uid]/[date]/page.tsx`.
  No other file changes.

---

## Task 1: `crates/common` — shared `CUSTOM_NAME_MAX_LENGTH` constant

**Files:** modify `crates/common/src/lib.rs`.

Independent, first task — nothing else depends on this compiling except
Task 3's validator. Resolves Judgment Call 1.

- [ ] **Step 1: Add the constant**, near `TicketEntryRequest`
  (`crates/common/src/lib.rs:611-633`), the request type most directly
  related to this feature's data:

```rust
/// Shared bound for a tracked train's or ticket's `custom_name` (see
/// `docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md`'s
/// Decision 1) -- one source of truth so `crates/api`'s validator
/// (`train_tracking::validate_custom_name`) never drifts from whatever a
/// future frontend character-counter would also need to know, the same
/// reasoning `TFL_OPERATOR`/`TFL_LINE_ID_PREFIX` above already establish
/// for a cross-cutting constant living here rather than as a private
/// literal in one crate. Counts Unicode scalar values (`str::chars().count()`),
/// not bytes -- "100 characters" is the user-facing wording
/// (`validate_custom_name`'s own error message), so the bound should match
/// what a human would count, not UTF-8 byte length. A reasonable-sounding,
/// not researched or load-tested figure, same posture
/// `crates/api/src/data/train_tracking.rs`'s own `MAX_PIN_AGE`/
/// `MINE_LIST_LIMIT` are flagged with -- revisit once real usage exists.
pub const CUSTOM_NAME_MAX_LENGTH: usize = 100;
```

- [ ] **Step 2: Verify**

```bash
cargo build -p common
```

  Expected: builds clean (a bare `pub const` addition cannot break any
  existing caller).

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "common: add CUSTOM_NAME_MAX_LENGTH, the shared bound for a tracked train's/ticket's custom_name"
```

---

## Task 2: Migration — `custom_name` on both tables

**Files:** create `crates/api/migrations/20260905130000_custom_tracking_names.sql`.

Independent of Task 1 (a bare `ALTER TABLE ... ADD COLUMN` needs no Rust
code to apply). `20260905130000` sorts after the latest existing migration
(`20260905120000_island_of_ireland_reference.sql`, confirmed via
`ls crates/api/migrations/ | sort | tail`).

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Custom display names for tracked trains and tickets, per
-- docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md.
--
-- Both nullable, no DEFAULT: NULL means "no custom name set, render the
-- computed default" -- see that spec's Decision 3 for why the default
-- (origin/destination/date derived from data already on the row) is
-- computed client-side at render time and deliberately never stored here.
-- This mirrors the `Option<String>`-typed nullable columns already on both
-- tables (`pin_destination_crs`, `pin_operator`, `train_uid`, every ticket
-- field except `id`/`user_id`/`source`/`created_at`), rather than
-- introducing a new "empty string means unset" convention this schema
-- doesn't otherwise use -- an empty-after-trim value is normalized to NULL
-- server-side on write (`train_tracking::validate_custom_name`), never
-- stored as `''`.
--
-- Capped at common::CUSTOM_NAME_MAX_LENGTH (100) characters, enforced in
-- Rust (train_tracking::validate_custom_name), not a DB CHECK constraint --
-- see the design spec's Decision 1: no precedent in this schema uses a
-- DB-level CHECK for string length on a free-text column
-- (custom_lines.name has none), and a CHECK failure would surface as an
-- opaque 500 rather than this app's usual human-readable 400.
-- -------------------------------------------------------------------------

ALTER TABLE tracked_trains ADD COLUMN custom_name TEXT;

-- LEGAL/PRIVACY AUDIT ADDENDUM (see 20260829090000_journey_ticket_tracking.sql's
-- and 20260901140000_standalone_tickets.sql's own audit comments): this
-- migration adds `custom_name`, a nullable, user-authored display label
-- the tracking user types for their OWN list entry (e.g. "Mum's ticket to
-- Leeds"). This is NOT "passenger name" in the sense that comment bans --
-- that ban targets PII *extracted from the ticket document itself*
-- (barcode/ITSO/pkpass payload), never anything a user types about their
-- own record. `custom_name` is never populated by `ticket_extraction.rs`
-- and carries no connection to a third party's identity. See
-- docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md's
-- Decision 2 for the full reasoning. The audit list itself is unchanged --
-- this table still must never gain payment/price data, any barcode
-- payload, ITSO data, passenger name (as extracted from a document), or
-- the uploaded file itself.
ALTER TABLE tracked_train_tickets ADD COLUMN custom_name TEXT;
```

- [ ] **Step 2: Verify.** `sqlx` migrations in this crate run automatically
  against `DATABASE_URL` on `cargo test`/`cargo run` startup (this crate's
  existing convention — no separate `sqlx migrate run` step exists in CI).
  Confirm the migration applies cleanly against a real local Postgres:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
```

  Expected: no migration error (a broken migration fails loudly, before any
  test runs, with a `sqlx::migrate::MigrateError`). Then confirm the columns
  exist:

```bash
psql "$DATABASE_URL" -c "\d tracked_trains" | grep custom_name
psql "$DATABASE_URL" -c "\d tracked_train_tickets" | grep custom_name
```

  Expected: both show `custom_name | text |`, nullable, no default.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260905130000_custom_tracking_names.sql
git commit -m "api: add nullable custom_name to tracked_trains and tracked_train_tickets"
```

---

## Task 3: Backend validation + data-layer write functions

**Files:** modify `crates/api/src/data/train_tracking.rs`.

Depends on Task 1 (`common::CUSTOM_NAME_MAX_LENGTH`) and Task 2 (the column
must exist for the two `UPDATE` functions to compile against a real schema
at test time, though the pure `validate_custom_name` function itself needs
neither).

- [ ] **Step 1: Add `CUSTOM_NAME_MAX_LENGTH` to this file's existing
  `common::{...}` import** (`train_tracking.rs:8`):

```rust
use common::{
    CUSTOM_NAME_MAX_LENGTH, TicketEntryRequest, TrackPinRequest, TrackedTrainRef,
    TrainMovementEventMessage,
};
```

- [ ] **Step 2: Add `validate_custom_name`**, directly below
  `validate_ticket_entry` (after `train_tracking.rs:148`, before the
  `#[cfg(test)] mod ticket_entry_tests` block):

```rust
/// Normalizes a raw `customName` request field into what should actually be
/// written: `None` if the field was absent/JSON-`null`, or if what's left
/// after trimming is empty (this is "clear the custom name," not an error —
/// see the design spec's Decision 1), or `Some(trimmed)` otherwise, bounded
/// by [`CUSTOM_NAME_MAX_LENGTH`]. Both rename routes (`crates/api/src/routes/train.rs`)
/// call this before writing, so the trim-and-normalize step lives in exactly
/// one place rather than being duplicated between the tracked-train and
/// ticket routes. Same user-facing-copy posture as [`validate_pin`]'s doc
/// comment: this message is rendered verbatim in `RenameTrainButton`/
/// `RenameTicketButton`'s error text, so it carries no internal field names.
pub fn validate_custom_name(name: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > CUSTOM_NAME_MAX_LENGTH {
        return Err(format!(
            "That name is too long — custom names can be at most {CUSTOM_NAME_MAX_LENGTH} \
             characters."
        ));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod custom_name_tests {
    use super::*;

    #[test]
    fn a_well_formed_name_is_trimmed_and_kept() {
        assert_eq!(
            validate_custom_name(Some("  My commute  ")),
            Ok(Some("My commute".to_string()))
        );
    }

    #[test]
    fn none_input_is_kept_as_none() {
        // JSON `customName` omitted or explicitly `null` -- the route's
        // Option<String> deserializes both to None.
        assert_eq!(validate_custom_name(None), Ok(None));
    }

    #[test]
    fn an_empty_string_clears_rather_than_errors() {
        assert_eq!(validate_custom_name(Some("")), Ok(None));
    }

    #[test]
    fn a_whitespace_only_string_clears_rather_than_errors() {
        // Decision 1's own guard: whitespace-only can't masquerade as "a
        // custom name is set" and permanently hide the useful default.
        assert_eq!(validate_custom_name(Some("   ")), Ok(None));
    }

    #[test]
    fn exactly_at_the_cap_is_accepted() {
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH);
        assert_eq!(validate_custom_name(Some(&name)), Ok(Some(name)));
    }

    #[test]
    fn one_over_the_cap_is_rejected() {
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH + 1);
        assert!(validate_custom_name(Some(&name)).is_err());
    }

    #[test]
    fn the_cap_counts_unicode_scalar_values_not_bytes() {
        // "café" x 25 = 100 chars but more than 100 UTF-8 bytes (each 'é' is
        // 2 bytes) -- proves this counts chars(), not len(), matching the
        // "100 characters" wording in the error message.
        let name = "café".repeat(25);
        assert_eq!(name.chars().count(), 100);
        assert!(name.len() > 100);
        assert_eq!(validate_custom_name(Some(&name)), Ok(Some(name)));
    }

    #[test]
    fn validation_messages_carry_no_internal_field_names() {
        // Same guard as validate_pin's/validate_ticket_entry's own tests --
        // this 400 body is rendered verbatim by RenameTrainButton/
        // RenameTicketButton's error text.
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH + 1);
        let message = validate_custom_name(Some(&name)).unwrap_err();
        assert!(!message.is_empty());
        assert!(
            !message.contains('_'),
            "user-facing copy leaked an identifier: {message}"
        );
    }
}
```

- [ ] **Step 3: Run the new unit tests**

```bash
cargo test -p api custom_name_tests
```

  Expected: all 7 tests pass.

- [ ] **Step 4: Add `rename_tracked_train`**, directly below
  `delete_tracked_train` (after its closing brace, `train_tracking.rs:541`):

```rust
/// Renames (or clears, if `custom_name` is `None`) a tracked train's
/// display name, scoped to the caller's ownership -- same
/// `WHERE id = $1 AND user_id = $2` shape as [`delete_tracked_train`]
/// immediately above, folded directly into the `UPDATE` rather than a
/// separate ownership lookup first. Returns `true` if a row was updated,
/// `false` if no tracked train with that id belongs to this caller
/// (doesn't exist, or belongs to someone else -- indistinguishable at this
/// layer, same as every other ownership check in this file; the route
/// handler maps `false` to `404`, never `403`). The caller
/// (`crate::routes::train::post_tracked_train_name`) is responsible for
/// having already run `custom_name` through [`validate_custom_name`] --
/// this function does no validation of its own, matching
/// `attach_ticket_to_tracked_train`'s own "route validates, data layer
/// writes" division of responsibility.
pub async fn rename_tracked_train(
    pool: &PgPool,
    id: i64,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query("UPDATE tracked_trains SET custom_name = $1 WHERE id = $2 AND user_id = $3")
        .bind(custom_name)
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 5: Add `rename_ticket`**, directly below `delete_ticket`
  (after its closing brace, `train_tracking.rs:804`):

```rust
/// Renames (or clears) a ticket's display name, scoped to the caller's
/// ownership -- mirrors [`rename_tracked_train`] exactly, and
/// [`delete_ticket`]'s own `WHERE id = $1 AND user_id = $2` shape
/// immediately above it (no join needed, per this table's own
/// ownership-redundancy design -- see [`delete_ticket`]'s doc comment).
/// Applies identically whether the ticket is attached or standalone, same
/// as `delete_ticket`. Returns `true`/`false` with the same "doesn't
/// exist, or isn't yours -- 404, never 403" contract as
/// [`rename_tracked_train`].
pub async fn rename_ticket(
    pool: &PgPool,
    ticket_id: i64,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_train_tickets SET custom_name = $1 WHERE id = $2 AND user_id = $3",
    )
    .bind(custom_name)
    .bind(ticket_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 6: Add DB-gated `#[ignore]`d tests** for both functions,
  alongside `delete_ticket`'s own `db_tests` module (after
  `train_tracking.rs`'s existing
  `delete_ticket_an_unattached_standalone_ticket_deletes_identically_to_an_attached_one`
  test, inside the same `mod db_tests`):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_tracked_train -- --ignored --test-threads=1`"]
    async fn rename_tracked_train_the_owner_can_set_and_clear_a_custom_name() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TRAIN-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-RENAME-TRAIN-OWNER").await;

        let renamed = rename_tracked_train(
            &pool,
            tracking_id,
            "TEST-RENAME-TRAIN-OWNER",
            Some("My commute"),
        )
        .await
        .expect("rename tracked train");
        assert!(renamed);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, Some("My commute".to_string()));

        let cleared = rename_tracked_train(&pool, tracking_id, "TEST-RENAME-TRAIN-OWNER", None)
            .await
            .expect("clear custom name");
        assert!(cleared);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TRAIN-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_tracked_train -- --ignored --test-threads=1`"]
    async fn rename_tracked_train_a_non_owner_cannot_rename_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;
        seed_user(&pool, "TEST-RENAME-TRAIN-OTHER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;

        let renamed = rename_tracked_train(
            &pool,
            tracking_id,
            "TEST-RENAME-TRAIN-OTHER",
            Some("Hijacked name"),
        )
        .await
        .expect("attempt rename as non-owner");
        assert!(!renamed);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-RENAME-TRAIN-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_ticket -- --ignored --test-threads=1`"]
    async fn rename_ticket_the_owner_can_set_and_clear_a_custom_name() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TICKET-OWNER").await;
        let ticket_id = create_ticket(&pool, None, &fixture_entry(), "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("create fixture ticket");

        let renamed = rename_ticket(
            &pool,
            ticket_id,
            "TEST-RENAME-TICKET-OWNER",
            Some("Mum's ticket to Leeds"),
        )
        .await
        .expect("rename ticket");
        assert!(renamed);

        let ticket = get_ticket_owned(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("read ticket")
            .expect("ticket exists");
        assert_eq!(ticket.custom_name, Some("Mum's ticket to Leeds".to_string()));

        let cleared = rename_ticket(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER", None)
            .await
            .expect("clear custom name");
        assert!(cleared);

        let ticket = get_ticket_owned(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("read ticket")
            .expect("ticket exists");
        assert_eq!(ticket.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TICKET-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_ticket -- --ignored --test-threads=1`"]
    async fn rename_ticket_a_non_owner_cannot_rename_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TICKET-REAL-OWNER").await;
        seed_user(&pool, "TEST-RENAME-TICKET-OTHER").await;
        let ticket_id = create_ticket(
            &pool,
            None,
            &fixture_entry(),
            "TEST-RENAME-TICKET-REAL-OWNER",
        )
        .await
        .expect("create fixture ticket");

        let renamed = rename_ticket(
            &pool,
            ticket_id,
            "TEST-RENAME-TICKET-OTHER",
            Some("Hijacked name"),
        )
        .await
        .expect("attempt rename as non-owner");
        assert!(!renamed);

        cleanup_user(&pool, "TEST-RENAME-TICKET-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-RENAME-TICKET-OTHER").await;
    }
```

  These reference `custom_name` on `TrackedTrainState`/`TrackedTrainTicket`,
  which don't exist until Task 5 — this is deliberate: write the tests now,
  alongside the functions they exercise, and let them fail to compile until
  Task 5 lands (Task 5's own Step 1 note calls this back out explicitly).
  This mirrors how Task 5 itself will need `custom_name` wired before these
  tests can even build.

- [ ] **Step 7: Verify (pure unit tests only — the DB-gated tests above
  cannot pass until Task 5 adds `custom_name` to `TrackedTrainState`/
  `TrackedTrainTicket`, and are re-verified at the end of Task 5)**

```bash
cargo build -p api
```

  Expected: **this will not yet compile** — the four new DB-gated tests
  reference a `custom_name` field that doesn't exist on `TrackedTrainState`/
  `TrackedTrainTicket` until Task 5. This is expected and called out here
  rather than silently glossed over; proceed to Task 5, which fixes this
  compile error as part of its own Step 1.

- [ ] **Step 8: Commit** (once Task 5 makes the crate compile again — see
  that task's own commit step; this task's diff and Task 5's diff land as
  two separate commits regardless, in the order written)

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "api: add validate_custom_name, rename_tracked_train, rename_ticket"
```

---

## Task 4: Routes — two new `POST .../name` routes

**Files:** modify `crates/api/src/routes/train.rs`.

Depends on Task 3's `validate_custom_name`/`rename_tracked_train`/
`rename_ticket`. Resolves Judgment Call 2 (`POST`, not `PATCH`). Like
Task 3, this task's own new HTTP tests will not compile until Task 5 adds
`custom_name` to the response structs — noted in this task's own Step 5.

- [ ] **Step 1: Mount the two new routes** in `router()`
  (`crates/api/src/routes/train.rs:26-98`). Add
  `/Train/{tracking_id}/name` as a sibling of the existing
  `/Train/{tracking_id}` route, and `/Train/tickets/{ticket_id}/name` as a
  sibling of `/Train/tickets/{ticket_id}/attach`:

```rust
        .route(
            "/Train/{tracking_id}",
            axum::routing::get(get_by_tracking_id).delete(delete_tracked_train),
        )
        .route(
            "/Train/{tracking_id}/name",
            axum::routing::post(post_tracked_train_name),
        )
```

  and

```rust
        .route(
            "/Train/tickets/{ticket_id}/attach",
            axum::routing::post(post_attach_ticket),
        )
        .route(
            "/Train/tickets/{ticket_id}/name",
            axum::routing::post(post_ticket_name),
        )
```

- [ ] **Step 2: Add the shared request/response types**, near
  `TicketCreatedResponse` (`crates/api/src/routes/train.rs:107-111`):

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameRequest {
    /// Absent, JSON `null`, or an empty/whitespace-only string all mean
    /// "clear the custom name" -- see `train_tracking::validate_custom_name`.
    #[serde(default)]
    custom_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameResponse {
    /// The normalized value actually stored -- `None` if the name was
    /// cleared, `Some(trimmed)` otherwise. Never echoes back an
    /// un-normalized value the caller sent.
    custom_name: Option<String>,
}
```

- [ ] **Step 3: Add `post_tracked_train_name`**, directly below
  `delete_tracked_train` (after `crates/api/src/routes/train.rs:469`):

```rust
/// `POST /Train/{trackingId}/name` -- sets or clears a tracked train's
/// display name. `POST` to a narrow `/name` sub-path, not `PATCH` or a
/// bare `PUT /Train/{trackingId}`: this router has zero existing `PATCH`
/// routes, and every other narrow single-field mutation here (e.g.
/// `POST /Train/tickets/{ticket_id}/attach`) already follows this exact
/// shape -- see this plan's Judgment Call 2 for the full reasoning.
/// Ownership is folded directly into `train_tracking::rename_tracked_train`'s
/// own `WHERE id = $1 AND user_id = $2` -- same 404-never-403 convention as
/// `delete_tracked_train` immediately above.
async fn post_tracked_train_name(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, (StatusCode, String)> {
    let normalized = train_tracking::validate_custom_name(body.custom_name.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let renamed = train_tracking::rename_tracked_train(
        &app.database,
        tracking_id,
        &user.id,
        normalized.as_deref(),
    )
    .await
    .map_err(internal_error("rename tracked train"))?;
    if !renamed {
        return Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        ));
    }

    Ok(Json(RenameResponse {
        custom_name: normalized,
    }))
}
```

- [ ] **Step 4: Add `post_ticket_name`**, directly below `delete_ticket`
  (after `crates/api/src/routes/train.rs:281`):

```rust
/// `POST /Train/tickets/{ticketId}/name` -- sets or clears a ticket's
/// display name. Same shape as `post_tracked_train_name` above, against
/// `train_tracking::rename_ticket` instead. Deliberately flat
/// (`/Train/tickets/{ticket_id}/name`, not nested under a
/// `{tracking_id}`), matching `delete_ticket`'s own reasoning immediately
/// above it: a ticket may have no owning tracked train at all (a
/// STANDALONE ticket), so a route shape requiring a `tracking_id` in its
/// path cannot express renaming one.
async fn post_ticket_name(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(ticket_id): Path<i64>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, (StatusCode, String)> {
    let normalized = train_tracking::validate_custom_name(body.custom_name.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let renamed = train_tracking::rename_ticket(&app.database, ticket_id, &user.id, normalized.as_deref())
        .await
        .map_err(internal_error("rename ticket"))?;
    if !renamed {
        return Err((StatusCode::NOT_FOUND, "no ticket with that id".to_string()));
    }

    Ok(Json(RenameResponse {
        custom_name: normalized,
    }))
}
```

- [ ] **Step 5: Add HTTP-layer `db_tests`**, in this file's existing
  `mod db_tests` (after `crates/api/src/routes/train.rs`'s last test in that
  module — the tail of the file past line 1313 continues the same module;
  add these alongside the existing `post_attach_ticket_*` tests):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_the_owner_can_rename_and_clear() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-OWNER",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router.clone(),
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "  My commute  " }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "rename response: {body:?}");
        assert_eq!(
            body.get("customName").and_then(Value::as_str),
            Some("My commute")
        );

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": null }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "clear response: {body:?}");
        assert_eq!(body.get("customName"), Some(&Value::Null));

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_a_tracked_train_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-BYSTANDER").await;
        seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&owner_token),
            serde_json::json!({ "customName": "Hijacked" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no tracked train with that id".to_string()));

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-BYSTANDER").await;
        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_tracked_train_name -- --ignored --test-threads=1`"]
    async fn post_tracked_train_name_a_too_long_name_is_400() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TRAIN-TOOLONG").await;
        let router = test_router(test_app(pool.clone()));
        let tracking_id = seed_tracked_train(
            &pool,
            "TEST-ROUTE-RENAME-TRAIN-TOOLONG",
            None,
            "2026-09-05".parse().unwrap(),
        )
        .await;

        let (status, body) = post_json(
            router,
            format!("/Train/{tracking_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "a".repeat(101) }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.as_str().is_some_and(|s| !s.contains('_')),
            "400 body leaked an identifier: {body:?}"
        );

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TRAIN-TOOLONG").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_ticket_name -- --ignored --test-threads=1`"]
    async fn post_ticket_name_the_owner_can_rename_a_standalone_ticket() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-RENAME-TICKET-OWNER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Train/tickets".to_string(),
            Some(&token),
            serde_json::json!({ "source": "manual" }),
        )
        .await;
        let ticket_id = created
            .get("ticketId")
            .and_then(Value::as_i64)
            .expect("ticketId present");

        let (status, body) = post_json(
            router,
            format!("/Train/tickets/{ticket_id}/name"),
            Some(&token),
            serde_json::json!({ "customName": "Mum's ticket to Leeds" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "rename response: {body:?}");
        assert_eq!(
            body.get("customName").and_then(Value::as_str),
            Some("Mum's ticket to Leeds")
        );

        cleanup_user(&pool, "TEST-ROUTE-RENAME-TICKET-OWNER").await;
    }
```

- [ ] **Step 6: Verify.** Not expected to fully compile yet — this task's
  new tests, like Task 3's, reference `body.get("customName")` on a JSON
  response whose backing struct (`TrackedTrainState`/`TrackedTrainTicket`)
  doesn't yet serialize that field, since Task 5 hasn't run. The route
  handlers and mounting themselves (Steps 1-4) compile fine on their own —
  confirm that much now:

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
```

  Expected: builds and lints clean up through this task's own new code (the
  route handlers reference only fields that already exist —
  `RenameRequest`/`RenameResponse` are new self-contained types). The
  `#[ignore]`d tests added in this step and Task 3 do not run in this
  command and so do not block it; they are exercised for real once Task 5
  lands (that task's own Step 5 re-runs `cargo test -p api -- --ignored
  --test-threads=1` and expects all of them, from both this task and
  Task 3, to pass).

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "api: add POST /Train/{trackingId}/name and POST /Train/tickets/{ticketId}/name"
```

---

## Task 5: Wire `custom_name` into every backing `SELECT`/response struct

**Files:** modify `crates/api/src/data/train_tracking.rs`,
`crates/api/src/routes/train.rs`.

This is the task that makes Tasks 3's and 4's new tests compile and pass —
run their tests again at the end of this task's own Verify step. Touches
four read paths; **must not touch `TrackedTrainRow`/`TrackedTrainRef`**
(`train_tracking.rs:241-264`, the trust-consumer-facing poller shape) —
per this plan's Global Constraints and the spec's own Non-goals, that
struct has no user-facing rendering need for a name.

- [ ] **Step 1: `TrackedTrainState`** (backs `GET /Train/{trackingId}` and
  `GET /Train/by-uid/{uid}/{date}`). Add the field
  (`train_tracking.rs:373-395`):

```rust
pub struct TrackedTrainState {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub train_id: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
    pub custom_name: Option<String>,
}
```

  Add `tt.custom_name` to `TRACKED_TRAIN_STATE_SELECT`
  (`train_tracking.rs:408-417`):

```rust
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tt.train_uid, tt.train_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM tracked_trains tt \
    LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs)";
```

  Fix the two existing struct-literal test fixtures that break otherwise —
  `fn state(...)` in `crates/api/src/routes/train.rs:733-752` — add
  `custom_name: None,` (or thread a parameter through if a test needs a
  non-`None` value; none of the existing callers do, so a bare `None` field
  is sufficient):

```rust
    fn state(delay_minutes: Option<i32>) -> train_tracking::TrackedTrainState {
        train_tracking::TrackedTrainState {
            id: 1,
            service_date: "2026-08-29".parse().unwrap(),
            pin_origin_crs: "KGX".to_string(),
            pin_destination_crs: Some("EDB".to_string()),
            pin_origin_name: Some("London Kings Cross".to_string()),
            pin_destination_name: Some("Edinburgh Waverley".to_string()),
            resolution_status: "resolved".to_string(),
            train_uid: Some("A12345".to_string()),
            train_id: Some("1A23".to_string()),
            status: Some("late".to_string()),
            last_reported_location: Some("York".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes,
            next_calling_point: Some("Newcastle".to_string()),
            eta_next: Some(fixed_instant()),
            eta_source: Some("darwin-estimated".to_string()),
            custom_name: None,
        }
    }
```

- [ ] **Step 2: `TrackedTrainListItem`** (backs `GET /Train/mine`). Add the
  field (`train_tracking.rs:430-446`):

```rust
pub struct TrackedTrainListItem {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub tracked_at: DateTime<Utc>,
    pub custom_name: Option<String>,
}
```

  Add `tt.custom_name` to `list_tracked_trains_for_user`'s inline query
  (`train_tracking.rs:468-480`):

```rust
    let rows = sqlx::query_as::<_, TrackedTrainListItem>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                tt.pin_scheduled_departure, tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at, tt.custom_name \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
         WHERE tt.user_id = $1 \
         ORDER BY tt.tracked_at DESC \
         LIMIT $2",
    )
```

  No struct-literal fixture exists for `TrackedTrainListItem` (confirmed via
  `grep -rn "TrackedTrainListItem {" crates/api/src` — only the `sqlx::FromRow`
  derive builds it), so no test fixture needs updating here.

- [ ] **Step 3: `TrackedTrainTicket`** (backs
  `GET /Train/{trackingId}/tickets`, and internally `get_ticket_owned`). Add
  the field (`train_tracking.rs:710-727`):

```rust
pub struct TrackedTrainTicket {
    pub id: i64,
    pub tracked_train_id: Option<i64>,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_name: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub custom_name: Option<String>,
}
```

  Add `t.custom_name` to `TICKET_SELECT` (`train_tracking.rs:733-738`):

```rust
const TICKET_SELECT: &str = "\
    SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
           so.name AS origin_name, sd.name AS destination_name, t.source, t.created_at, \
           t.custom_name \
    FROM tracked_train_tickets t \
    LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs)";
```

  Fix the one existing struct-literal fixture that breaks otherwise —
  `fn ticket(...)` in `crates/api/src/routes/train.rs:718-731` — add
  `custom_name: None,`:

```rust
    fn ticket(operator: Option<&str>) -> train_tracking::TrackedTrainTicket {
        train_tracking::TrackedTrainTicket {
            id: 1,
            tracked_train_id: Some(1),
            operator: operator.map(str::to_string),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            origin_name: Some("London Kings Cross".to_string()),
            destination_name: Some("Edinburgh Waverley".to_string()),
            source: "manual".to_string(),
            created_at: fixed_instant(),
            custom_name: None,
        }
    }
```

- [ ] **Step 4: `TicketListItem`/`TicketListRow`** (backs
  `GET /Train/tickets/mine`). Add `custom_name` to the private row shape
  `TicketListRow` (`train_tracking.rs:828-853`):

```rust
struct TicketListRow {
    id: i64,
    tracked_train_id: Option<i64>,
    operator: Option<String>,
    ticket_type: Option<String>,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
    origin_name: Option<String>,
    destination_name: Option<String>,
    source: String,
    created_at: DateTime<Utc>,
    service_date: Option<chrono::NaiveDate>,
    pin_origin_crs: Option<String>,
    pin_destination_crs: Option<String>,
    pin_scheduled_departure: Option<DateTime<Utc>>,
    resolution_status: Option<String>,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
    custom_name: Option<String>,
}
```

  and the public `TicketListItem` (`train_tracking.rs:882-905`):

```rust
pub struct TicketListItem {
    pub id: i64,
    pub tracked_train_id: Option<i64>,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_name: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub service_date: Option<chrono::NaiveDate>,
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
    pub resolution_status: Option<String>,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub estimate: Option<delay_repay_rules::DelayRepayEstimate>,
    pub claim_url: String,
    pub disclaimer: &'static str,
    pub custom_name: Option<String>,
}
```

  Copy the field through in `build_ticket_list_item`
  (`train_tracking.rs:911-947`) — add `custom_name: row.custom_name,` to the
  returned `TicketListItem` literal (placed anywhere in the literal; field
  order in a struct literal doesn't need to match declaration order).

  Add `t.custom_name` to `list_tickets_for_user`'s inline query
  (`train_tracking.rs:971-985`):

```rust
    let rows = sqlx::query_as::<_, TicketListRow>(
        "SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
                so.name AS origin_name, sd.name AS destination_name, \
                t.source, t.created_at, \
                tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, t.custom_name \
         FROM tracked_train_tickets t \
         LEFT JOIN tracked_trains tt ON tt.id = t.tracked_train_id \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs) \
         WHERE t.user_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT $2",
    )
```

  Fix the two existing struct-literal fixtures that break otherwise —
  `fn row(...)` and `fn standalone_row(...)` in
  `crates/api/src/data/train_tracking.rs:998-1047` (`mod ticket_list_tests`)
  — add `custom_name: None,` to each literal.

- [ ] **Step 5: Verify — everything from this task plus the tests written
  in Tasks 3 and 4 that were deliberately left uncompilable until now**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api -- --ignored --test-threads=1
```

  Expected: clean build, clean clippy, all non-ignored tests pass, and every
  `#[ignore]`d test added in Task 3 (`rename_tracked_train_*`,
  `rename_ticket_*`) and Task 4 (`post_tracked_train_name_*`,
  `post_ticket_name_*`) now passes.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/routes/train.rs
git commit -m "api: expose custom_name on every tracked-train and ticket read response"
```

---

## Task 6: Frontend types — `customName` on the four wire interfaces

**Files:** modify `frontend/lib/types.ts`.

Depends on Task 5 (the backend must actually serialize `customName` for
this to be a faithful type, though nothing enforces that at compile time —
this is a plain interface addition).

- [ ] **Step 1: Add `customName: string | null`** to all four interfaces in
  `frontend/lib/types.ts`:

  `TrackedTrainState` (`types.ts:322-341`, add after `etaSource`):

```typescript
export interface TrackedTrainState {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  trainId: string | null;
  status: JourneyStatus | null;
  lastReportedLocation: string | null;
  lastEventType: string | null; // "ARRIVAL" | "DEPARTURE" | "PASS"
  delayMinutes: number | null;
  nextCallingPoint: string | null;
  etaNext: string | null; // RFC3339
  etaSource: EtaSource | null;
  // User-authored display label, or `null` for the computed default -- see
  // `lib/trackingName.ts`'s `trackedTrainDisplayName`. Never inferred from
  // any parsed document (`crates/api/src/data/ticket_extraction.rs`), only
  // ever set via `RenameTrainButton`.
  customName: string | null;
}
```

  `TrackedTrainListItem` (`types.ts:350-364`, add after `trackedAt`):

```typescript
export interface TrackedTrainListItem {
  id: number;
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string; // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  trackedAt: string; // RFC3339 -- list ordering key
  // See `TrackedTrainState.customName`'s comment -- same contract.
  customName: string | null;
}
```

  `TrackedTrainTicket` (`types.ts:405-418`, add after `createdAt`):

```typescript
export interface TrackedTrainTicket {
  id: number;
  trackedTrainId: number | null;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  originName: string | null;
  destinationName: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339
  // User-authored display label, or `null` for the computed default -- see
  // `TicketSummary.tsx`. Never inferred from an uploaded `.pkpass`/PDF.
  customName: string | null;
}
```

  `TicketListItem` (`types.ts:520-543`, add after `disclaimer`):

```typescript
export interface TicketListItem {
  id: number;
  trackedTrainId: number | null;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  originName: string | null;
  destinationName: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339 -- list ordering key
  serviceDate: string | null; // "YYYY-MM-DD"
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  resolutionStatus: ResolutionStatus | null;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
  // See `TrackedTrainTicket.customName`'s comment -- same contract.
  customName: string | null;
}
```

- [ ] **Step 2: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: no new type errors — this is a purely additive change to four
  interfaces; nothing destructures these types exhaustively in a way that
  would break (TypeScript object types are structurally open by default for
  reads).

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "frontend: add customName to the four tracked-train/ticket wire types"
```

---

## Task 7: Frontend default-name helper (with unit tests)

**Files:** create `frontend/lib/trackingName.ts`, create
`frontend/lib/trackingName.test.ts`.

Depends on Task 6. A new sibling file to `frontend/lib/stationLabel.ts`,
not an addition to it — `stationLabel.ts`'s own module doc frames it as a
single-concern file (station-label formatting); this helper is a distinct
concern (choosing between a stored name and a computed default) that
happens to *use* `routeLabel`/`formatDate`/`formatTime`, not extend them.

- [ ] **Step 1: Write the failing tests**

```typescript
import { describe, it, expect } from 'vitest';
import { trackedTrainDisplayName } from './trackingName';

describe('trackedTrainDisplayName', () => {
  const base = {
    customName: null as string | null,
    pinOriginCrs: 'KGX',
    pinOriginName: 'London Kings Cross' as string | null,
    pinDestinationCrs: 'EDB' as string | null,
    pinDestinationName: 'Edinburgh Waverley' as string | null,
    serviceDate: '2026-05-10',
    pinScheduledDeparture: '2026-05-10T13:32:00Z' as string | undefined,
  };

  it('renders the custom name verbatim when set, ignoring every other field', () => {
    expect(trackedTrainDisplayName({ ...base, customName: 'My commute' })).toBe('My commute');
  });

  it('falls back to route + date + time when there is no custom name and a departure time is present', () => {
    expect(trackedTrainDisplayName(base)).toBe('London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026 · 13:32');
  });

  it('degrades to date-only when pinScheduledDeparture is absent (TrackedTrainState has no such field)', () => {
    expect(
      trackedTrainDisplayName({ ...base, pinScheduledDeparture: undefined }),
    ).toBe('London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026');
  });

  it('falls back to origin-only when there is no destination yet (a pre-match pin)', () => {
    expect(
      trackedTrainDisplayName({ ...base, pinDestinationCrs: null, pinDestinationName: null }),
    ).toBe('London Kings Cross (KGX), 10 May 2026 · 13:32');
  });

  it('falls back to bare CRS codes when no station name resolved', () => {
    expect(
      trackedTrainDisplayName({
        ...base,
        pinOriginName: null,
        pinDestinationName: null,
      }),
    ).toBe('KGX → EDB, 10 May 2026 · 13:32');
  });

  it('an empty-string custom name is treated the same as null (defensive -- the backend never stores one, but this helper does not assume that)', () => {
    expect(trackedTrainDisplayName({ ...base, customName: '' })).toBe(
      'London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026 · 13:32',
    );
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd frontend && npm test -- trackingName.test.ts
```

  Expected: `FAIL` — `trackingName.ts` doesn't exist yet.

- [ ] **Step 3: Write `trackingName.ts`**

```typescript
import { formatDate, formatTime } from './dateFormat';
import { routeLabel } from './stationLabel';

/** The label a tracked train's row/title should show: the user's own
 * `customName` if they set one, verbatim; otherwise the same
 * route + date(/time) default `TrackedTrainListRow`
 * (`app/track/mine/page.tsx`) and `TrainJourney`'s `pinSummary` already
 * compute today -- this function is that computation, extracted so both
 * call sites (and this custom-names feature's own new one) share it rather
 * than each hand-rolling the same `routeLabel(...) + ' · ' + formatDate(...)`
 * shape independently.
 *
 * `pinScheduledDeparture` is optional because `TrackedTrainState`
 * (`lib/types.ts`) has no such field at all -- the backend's own read
 * query for a single tracked train's detail page never selects
 * `pin_scheduled_departure`, only `serviceDate`. When it's absent, this
 * degrades to a date-only default, exactly as `TrainJourney.tsx`'s
 * existing `pinSummary` already does -- not a new gap this feature
 * introduces.
 *
 * Never persisted -- always computed fresh from fields already on the
 * caller's own wire object, at render time. See
 * docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md's
 * Decision 3 for why a stored-at-creation default would go stale the
 * moment a pre-match pin's destination or a slow-to-load station name
 * resolves later. */
export function trackedTrainDisplayName(train: {
  customName: string | null;
  pinOriginCrs: string;
  pinOriginName: string | null;
  pinDestinationCrs: string | null;
  pinDestinationName: string | null;
  serviceDate: string;
  pinScheduledDeparture?: string;
}): string {
  if (train.customName) return train.customName;

  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  return `${route}, ${when}`;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cd frontend && npm test -- trackingName.test.ts
```

  Expected: `PASS`, all 6 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/trackingName.ts frontend/lib/trackingName.test.ts
git commit -m "frontend: add trackedTrainDisplayName, the shared custom-name-or-computed-default helper"
```

---

## Task 8: Frontend Rename components

**Files:** create `frontend/components/RenameTrainButton.tsx`, create
`frontend/components/RenameTrainButton.test.tsx`, create
`frontend/components/RenameTicketButton.tsx`, create
`frontend/components/RenameTicketButton.test.tsx`.

Depends on Task 6 (types) only for the prop shapes used in tests; not on
Task 7. Two separate, near-identical components (not one generic
`RenameButton`) — mirrors this codebase's own existing precedent of two
near-identical `DeleteTrainButton`/`DeleteTicketButton` components rather
than one parameterized one. Implements Judgment Call 3 (Save disabled on an
empty trimmed input; Clear is the only way to actually clear).

- [ ] **Step 1: Write `RenameTrainButton.tsx`**

```typescript
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, TextInput, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Renames or clears a tracked train's `customName`, via the same-origin
 * `/api/*` proxy (see `app/api/[...path]/route.ts`) -- this is a Client
 * Component and cannot reach the `api` service directly.
 * `/api/Train/{trackingId}/name` is passed straight through to the
 * backend's `POST /Train/{trackingId}/name`
 * (`crates/api/src/routes/train.rs::post_tracked_train_name`) with no
 * `/public/` prefix inserted, same as `DeleteTrainButton`.
 *
 * Closely modeled on `DeleteTrainButton.tsx`: same button → confirm-modal →
 * fetch → `router.refresh()` shape, same `useNeedsLogin`/`LoginLink` `401`
 * handling, same generic-error-message fallback for any other non-`ok`
 * status. Unlike `DeleteTrainButton`, this is never destructive in a way
 * that leaves the page's own subject gone, so it always `router.refresh()`s
 * on success rather than navigating away -- same reasoning
 * `DeleteTicketButton` already gives for its own `router.refresh()` choice.
 *
 * `Save` is disabled whenever the trimmed input is empty -- this is the
 * one deliberate divergence from a plain "submit whatever's in the box"
 * pattern (see
 * docs/superpowers/specs/2026-09-05-custom-tracking-names-plan.md's
 * Judgment Call 3): the backend already normalizes an empty-after-trim
 * value to "clear the name" on any successful write, so without this,
 * accidentally emptying the field and hitting Save would silently clear a
 * name the user meant to just edit. Disabling Save on empty input means
 * clearing only ever happens through the explicit `Clear` button (visible
 * only when a custom name is currently set), which needs no typing at all. */
export function RenameTrainButton({
  trackingId,
  customName,
  defaultName,
}: {
  trackingId: number;
  customName: string | null;
  defaultName: string;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [value, setValue] = useState(customName ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setValue(customName ?? '');
    setError(null);
    open();
  }

  async function submit(nextCustomName: string | null) {
    setSaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/${trackingId}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: nextCustomName }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSaving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSaving(false);
    }
  }

  const trimmed = value.trim();

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handleOpen}>
        Rename
      </Button>
      <Modal opened={opened} onClose={close} title="Rename this tracked train">
        <TextInput
          label="Custom name"
          placeholder={defaultName}
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
          maxLength={200}
          data-autofocus
        />
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to rename this tracked train</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={saving}>
            Cancel
          </Button>
          {customName !== null && (
            <Button variant="outline" color="red" onClick={() => submit(null)} loading={saving}>
              Clear
            </Button>
          )}
          <Button onClick={() => submit(trimmed)} loading={saving} disabled={trimmed.length === 0}>
            Save
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

  (`maxLength={200}` on the `TextInput` is a generous client-side typing
  cap, not the enforced 100-character limit — the backend is the source of
  truth per this plan's Global Constraints; this only stops a pathological
  paste from making the input unusable before the user even hits Save. No
  character counter is added, per this plan's Non-goals.)

- [ ] **Step 2: Write `RenameTrainButton.test.tsx`**

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RenameTrainButton } from './RenameTrainButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RenameTrainButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call the API until Save is clicked', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Save is disabled when the input is empty', async () => {
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const save = await screen.findByRole('button', { name: 'Save' });
    expect(save).toBeDisabled();
  });

  it('POSTs the trimmed name and refreshes on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ customName: 'My commute' }), { status: 200 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: '  My commute  ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42/name', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: 'My commute' }),
      });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('Clear is only shown when a custom name is currently set, and posts null', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ customName: null }), { status: 200 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName="My commute" defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Clear' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/42/name', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: null }),
      });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('does not render a Clear button when there is no custom name yet', async () => {
    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    await screen.findByRole('button', { name: 'Save' });
    expect(screen.queryByRole('button', { name: 'Clear' })).not.toBeInTheDocument();
  });

  it('shows the backend error text on a 400', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response('That name is too long — custom names can be at most 100 characters.', { status: 400 }),
    );

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'x'.repeat(101) } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(
        screen.getByText('That name is too long — custom names can be at most 100 characters.'),
      ).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<RenameTrainButton trackingId={42} customName={null} defaultName="KGX → EDB, 10 May" />);
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const input = await screen.findByLabelText('Custom name');
    fireEvent.change(input, { target: { value: 'My commute' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await screen.findByRole('link', { name: 'Log in to rename this tracked train' });
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 3: Run the tests**

```bash
cd frontend && npm test -- RenameTrainButton.test.tsx
```

  Expected: `PASS`, all 7 tests.

- [ ] **Step 4: Write `RenameTicketButton.tsx`** — identical shape, targeting
  the ticket route:

```typescript
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, TextInput, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Renames or clears a ticket's `customName` -- identical shape to
 * `RenameTrainButton.tsx` (see that component's own doc comment for the
 * full rationale, including why Save is disabled on an empty trimmed
 * input), against `POST /Train/tickets/{ticketId}/name`
 * (`crates/api/src/routes/train.rs::post_ticket_name`) instead. Two
 * separate components rather than one generic `RenameButton`, mirroring
 * this codebase's own `DeleteTrainButton`/`DeleteTicketButton` precedent. */
export function RenameTicketButton({
  ticketId,
  customName,
  defaultName,
}: {
  ticketId: number;
  customName: string | null;
  defaultName: string;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [value, setValue] = useState(customName ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setValue(customName ?? '');
    setError(null);
    open();
  }

  async function submit(nextCustomName: string | null) {
    setSaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}/name`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ customName: nextCustomName }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSaving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSaving(false);
    }
  }

  const trimmed = value.trim();

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handleOpen}>
        Rename
      </Button>
      <Modal opened={opened} onClose={close} title="Rename this ticket">
        <TextInput
          label="Custom name"
          placeholder={defaultName}
          value={value}
          onChange={(event) => setValue(event.currentTarget.value)}
          maxLength={200}
          data-autofocus
        />
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to rename this ticket</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={saving}>
            Cancel
          </Button>
          {customName !== null && (
            <Button variant="outline" color="red" onClick={() => submit(null)} loading={saving}>
              Clear
            </Button>
          )}
          <Button onClick={() => submit(trimmed)} loading={saving} disabled={trimmed.length === 0}>
            Save
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

- [ ] **Step 5: Write `RenameTicketButton.test.tsx`** — same seven cases as
  `RenameTrainButton.test.tsx`, with `ticketId={7}` and
  `/api/Train/tickets/7/name` in place of `trackingId={42}`/
  `/api/Train/42/name`, and `"Log in to rename this ticket"` in place of
  `"Log in to rename this tracked train"`. (Full file omitted here — it is
  a mechanical rename of every train-specific identifier in Step 2's file to
  its ticket equivalent; write it by copying that file and substituting: the
  import, the two `screen.getByRole`/`findByRole` login-link name strings,
  the prop names (`ticketId` for `trackingId`), and the two fetch URL
  strings.)

- [ ] **Step 6: Run both test files**

```bash
cd frontend && npm test -- RenameTrainButton.test.tsx RenameTicketButton.test.tsx
```

  Expected: `PASS`, 14 tests total.

- [ ] **Step 7: Commit**

```bash
git add frontend/components/RenameTrainButton.tsx frontend/components/RenameTrainButton.test.tsx \
        frontend/components/RenameTicketButton.tsx frontend/components/RenameTicketButton.test.tsx
git commit -m "frontend: add RenameTrainButton and RenameTicketButton"
```

---

## Task 9: Wire into tracked-train render sites

**Files:** modify `frontend/app/track/mine/page.tsx`,
`frontend/components/TrainJourney.tsx`,
`frontend/app/train/by-id/[trackingId]/page.tsx`,
`frontend/app/train/[uid]/[date]/page.tsx`.

Depends on Tasks 6, 7, 8.

- [ ] **Step 1: `TrackedTrainListRow`** (`frontend/app/track/mine/page.tsx:121-187`).
  Import `trackedTrainDisplayName` and `RenameTrainButton`, compute the
  display name, replace the bare `route` variable's use as the title text,
  and add the button to the header `Group` — outside the `<Link>`, matching
  `DeleteTrainButton`'s own placement rule ("a button inside an anchor is
  invalid HTML"):

```typescript
import { trackedTrainDisplayName } from '@/lib/trackingName';
import { RenameTrainButton } from '@/components/RenameTrainButton';
```

```typescript
function TrackedTrainListRow({ train, tickets }: { train: TrackedTrainListItem; tickets: TicketListItem[] }) {
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );
  const displayName = trackedTrainDisplayName(train);
  const defaultName = `${route}, ${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`;

  return (
    <Card withBorder>
      <Stack gap="sm">
        <Group justify="space-between" wrap="nowrap" align="flex-start">
          <Link href={href} style={{ textDecoration: 'none', color: 'inherit', flex: 1 }}>
            <Stack gap={4}>
              <Group justify="space-between" wrap="nowrap">
                <Text fw={500}>{displayName}</Text>
                <RowStatusBadge train={train} />
              </Group>
              <Text size="sm" c="dimmed">
                {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
              </Text>
            </Stack>
          </Link>
          <RenameTrainButton trackingId={train.id} customName={train.customName} defaultName={defaultName} />
        </Group>
        {tickets.length > 0 && (
          /* ... unchanged ... */
        )}
      </Stack>
    </Card>
  );
}
```

  (The outer `Group` wrapping both the `<Link>` and the new button is a new
  layer around the existing `<Link>` — the `<Link>`'s own internal `Group`
  for the title/badge row is unchanged. `align="flex-start"` keeps the
  Rename button vertically aligned with the title row rather than
  stretching to match the two-line link's full height.)

- [ ] **Step 2: `TrainJourney.tsx`'s `pinSummary`** (`TrainJourney.tsx:19-24`).
  Route it through `trackedTrainDisplayName` instead of a bare
  `routeLabel(...) + formatDate(...)`, so a renamed train's custom name
  shows on the detail page too, not only in the `/track/mine` list —
  `trackedTrainDisplayName` is exactly the helper Decision 3 built for
  "wherever this default label is rendered," and this is the second of the
  two existing call sites the spec named for it:

```typescript
import { trackedTrainDisplayName } from '@/lib/trackingName';
```

```typescript
export function TrainJourney({ state }: { state: TrackedTrainState }) {
  const pinSummary = (
    <Text size="sm" c="dimmed">
      {trackedTrainDisplayName(state)}
    </Text>
  );
```

  (`routeLabel` and `formatDate` may become unused imports in this file if
  nothing else in it calls them — check with
  `grep -n "routeLabel\|formatDate" frontend/components/TrainJourney.tsx`
  after this edit and remove either import if it's now dead, to keep
  `npm run build`'s lint step clean.)

- [ ] **Step 3: Both train detail pages** — add `RenameTrainButton` next to
  `DeleteTrainButton` in the header `Group`.
  `frontend/app/train/by-id/[trackingId]/page.tsx`:

```typescript
import { RenameTrainButton } from '@/components/RenameTrainButton';
```

```typescript
      <Group justify="space-between">
        <Title order={1}>Tracking Train {trackingId}</Title>
        <Group gap="xs">
          <RenameTrainButton
            trackingId={state.id}
            customName={state.customName}
            defaultName={trackedTrainDisplayName(state)}
          />
          <DeleteTrainButton trackingId={state.id} />
        </Group>
      </Group>
```

  (add `import { trackedTrainDisplayName } from '@/lib/trackingName';` too.)
  `frontend/app/train/[uid]/[date]/page.tsx`, identical shape:

```typescript
import { RenameTrainButton } from '@/components/RenameTrainButton';
import { trackedTrainDisplayName } from '@/lib/trackingName';
```

```typescript
      <Group justify="space-between">
        <Title order={1}>Train {uid}</Title>
        <Group gap="xs">
          <RenameTrainButton
            trackingId={state.id}
            customName={state.customName}
            defaultName={trackedTrainDisplayName(state)}
          />
          <DeleteTrainButton trackingId={state.id} />
        </Group>
      </Group>
```

  (Passing `trackedTrainDisplayName(state)` itself as `defaultName` means
  the modal's placeholder shows the *current* rendered label — which is
  either the already-set custom name or the computed default, whichever is
  showing right now. This is correct for the placeholder's purpose either
  way: it always shows "what you'd see if you cleared this," which for an
  already-named train is trivially itself and for an unnamed one is the
  real computed default. `TrackedTrainListRow`'s own `defaultName` in Step 1
  is deliberately the *route-based* string, not `trackedTrainDisplayName(train)`,
  for the same reason applied more carefully: passing
  `trackedTrainDisplayName(train)` there would show the *current custom
  name itself* as the placeholder once one is set, which is confusing —
  a placeholder should show what clearing produces, not what's already
  there. Both detail pages don't have this problem in practice for the
  common case, but Step 1's own explicit `defaultName` construction is the
  more correct pattern; consider it the canonical one if the two ever need
  to be reconciled.)

- [ ] **Step 4: Verify — automated**

```bash
cd frontend && npm test
npx tsc --noEmit
npm run build
```

  Expected: all existing tests still pass (in particular,
  `frontend/lib/stationLabel.test.ts` and any existing `TrainJourney`-
  adjacent snapshot/render test, if one exists, must be checked for any
  literal expected-text assertion against the old bare `routeLabel(...) +
  formatDate(...)` pinSummary shape — grep for one before assuming none
  exists: `grep -rln "TrainJourney" frontend --include=*.test.tsx`), no new
  type errors, and a clean production build.

- [ ] **Step 5: Verify — manual, in a real browser** (this repo's standing
  practice for a UI change with no end-to-end test coverage). Start the dev
  stack (`docker compose up` or this repo's own documented dev-server
  command), log in, and:
  1. Track a new train (or use an existing one) and open `/track/mine`.
     Confirm its row shows the computed default label (route + date/time),
     unchanged from before this plan.
  2. Click **Rename**, type a custom name, click **Save**. Confirm the row
     now shows the custom name, and that `/track/mine`'s Clear button is now
     visible on a second open of the modal.
  3. Open that train's own detail page (`/train/by-id/{id}` or
     `/train/{uid}/{date}`). Confirm the page's pin summary now also shows
     the custom name, and a Rename button sits next to Delete.
  4. Click Rename again, click **Clear**. Confirm both the list row and the
     detail page revert to the computed default label.
  5. Confirm a *different*, never-renamed tracked train still shows its
     plain computed default throughout — this change must be additive, not
     a regression for every row that hasn't been renamed.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/track/mine/page.tsx frontend/components/TrainJourney.tsx \
        "frontend/app/train/by-id/[trackingId]/page.tsx" "frontend/app/train/[uid]/[date]/page.tsx"
git commit -m "frontend: wire custom tracked-train names into the list row and both detail pages"
```

---

## Task 10: Wire into ticket render sites

**Files:** modify `frontend/components/TicketSummary.tsx`,
`frontend/components/TicketPanel.tsx`, `frontend/app/track/mine/page.tsx`.

Depends on Tasks 6 and 8 (not Task 7 — the ticket default label is a
simpler inline extension of `TicketSummary`'s existing rendering, per the
spec's Decision 3, not a new helper function).

- [ ] **Step 1: `TicketSummary.tsx`** (`TicketSummary.tsx:57-63`). When
  `customName` is set, render it as the bold title in place of
  `{operator ?? 'Ticket'}{ticketType...}`; the route sub-line is unchanged
  either way (Decision 3: "a custom name replaces the 'what is this' line,
  not the 'where does it go' line"). Widen the `Pick<...>` prop type to
  include the new field:

```typescript
export function TicketSummary({
  ticket,
}: {
  ticket: Pick<
    TrackedTrainTicket | TicketListItem,
    | 'operator'
    | 'ticketType'
    | 'originCrs'
    | 'destinationCrs'
    | 'originName'
    | 'destinationName'
    | 'source'
    | 'createdAt'
    | 'customName'
  >;
}) {
  const route =
    ticket.originCrs || ticket.destinationCrs
      ? `${ticket.originCrs ? stationLabel(ticket.originCrs, ticket.originName) : '?'} → ${
          ticket.destinationCrs ? stationLabel(ticket.destinationCrs, ticket.destinationName) : '?'
        }`
      : null;
  return (
    <Stack gap={2}>
      <Text fw={500}>
        {ticket.customName ?? (
          <>
            {ticket.operator ?? 'Ticket'}
            {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
          </>
        )}
      </Text>
      {route && <Text size="sm">{route}</Text>}
      <Group gap="xs">
        <Badge variant="outline" size="sm" color="gray">
          {SOURCE_LABELS[ticket.source]}
        </Badge>
        <Text size="xs" c="dimmed">
          Added <LocalDateTime value={ticket.createdAt} />
        </Text>
      </Group>
    </Stack>
  );
}
```

- [ ] **Step 2: `TicketPanel.tsx`** (`TicketPanel.tsx:90-99`) — add
  `RenameTicketButton` next to the existing `DeleteTicketButton`, computing
  the default label inline the same way `TicketSummary` itself does (a
  ticket has no dedicated default-label helper — see this task's own
  header note):

```typescript
import { RenameTicketButton } from './RenameTicketButton';
```

```typescript
      {withEstimates.map(({ ticket, estimate }, index) => (
        <Stack key={ticket.id} gap="xs">
          {index > 0 && <Divider />}
          <TicketSummary ticket={ticket} />
          {estimate && <DelayRepayEstimate response={estimate} />}
          <Group gap="xs">
            <RenameTicketButton
              ticketId={ticket.id}
              customName={ticket.customName}
              defaultName={`${ticket.operator ?? 'Ticket'}${ticket.ticketType ? ` — ${ticket.ticketType}` : ''}`}
            />
            <DeleteTicketButton ticketId={ticket.id} />
          </Group>
        </Stack>
      ))}
```

- [ ] **Step 3: `app/track/mine/page.tsx`'s two `TicketSummary` call sites**
  — `TrackedTrainListRow`'s attached-tickets loop (`page.tsx:165-180`) and
  `UnattachedTicketRow` (`page.tsx:200-218`). Both get the same
  `RenameTicketButton` addition next to their existing
  `DeleteTicketButton`:

```typescript
import { RenameTicketButton } from '@/components/RenameTicketButton';
```

```typescript
            {tickets.map((ticket) => (
              <Stack key={ticket.id} gap={4}>
                <TicketSummary ticket={ticket} />
                <DelayRepayEstimate
                  response={{
                    delayMinutes: ticket.delayMinutes,
                    estimate: ticket.estimate,
                    claimUrl: ticket.claimUrl,
                    disclaimer: ticket.disclaimer,
                  }}
                />
                <Group gap="xs">
                  <RenameTicketButton
                    ticketId={ticket.id}
                    customName={ticket.customName}
                    defaultName={`${ticket.operator ?? 'Ticket'}${ticket.ticketType ? ` — ${ticket.ticketType}` : ''}`}
                  />
                  <DeleteTicketButton ticketId={ticket.id} />
                </Group>
              </Stack>
            ))}
```

  and, in `UnattachedTicketRow`:

```typescript
        <Group gap="lg" wrap="wrap" align="flex-end">
          <AttachTicketAction ticketId={ticket.id} trains={trains} />
          <TextLink href={`/track?${trackParams.toString()}`} underline="always">
            Track a new train for this ticket
          </TextLink>
          <RenameTicketButton
            ticketId={ticket.id}
            customName={ticket.customName}
            defaultName={`${ticket.operator ?? 'Ticket'}${ticket.ticketType ? ` — ${ticket.ticketType}` : ''}`}
          />
          <DeleteTicketButton ticketId={ticket.id} />
        </Group>
```

- [ ] **Step 4: Verify — automated**

```bash
cd frontend && npm test
npx tsc --noEmit
npm run build
```

  Expected: all tests pass (in particular, `TicketSummary`'s own prop-type
  widening must not break any existing caller — check
  `grep -rln "<TicketSummary" frontend` and confirm every call site's
  `ticket` argument already satisfies the widened `Pick<...>`, which it
  does automatically since `customName` was added to both
  `TrackedTrainTicket` and `TicketListItem` in Task 6), no new type errors,
  clean build.

- [ ] **Step 5: Verify — manual, in a real browser.** Same shape as Task 9's
  Step 5, for tickets:
  1. On `/track/mine`, confirm an existing ticket (attached or
     unattached) shows its plain computed title
     (`{operator} — {ticketType}` or route), unchanged.
  2. Click **Rename** on that ticket, set a custom name, **Save**. Confirm
     the ticket's title becomes the custom name, and the route sub-line
     (origin → destination) is unchanged underneath it.
  3. If the ticket is attached to a tracked train, open that train's own
     detail page and confirm `TicketPanel` shows the same renamed title.
  4. Click **Rename**, **Clear**. Confirm the title reverts to the
     computed default (`{operator} — {ticketType}` or route) in both
     places.
  5. Confirm a different, never-renamed ticket is unaffected throughout.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TicketSummary.tsx frontend/components/TicketPanel.tsx frontend/app/track/mine/page.tsx
git commit -m "frontend: wire custom ticket names into TicketSummary, TicketPanel, and /track/mine"
```

---

## Self-review (spec coverage)

- Decision 1 (schema, 100-char cap, app-layer validation) → Tasks 1-3.
- Decision 2 (privacy audit resolution) → Task 2's verbatim migration
  comment; Global Constraints' standing guard against `ticket_extraction.rs`.
- Decision 3 (client-side computed default) → Task 7 (`trackedTrainDisplayName`)
  and Task 10 Step 1 (`TicketSummary`'s inline equivalent).
- Decision 4 (API surface) → Tasks 3-5, with the verb resolved by Judgment
  Call 2 (`POST`, not `PATCH`).
- Decision 5 (UI pattern, placement) → Tasks 8-10.
- Decision 6 (ownership/auth, no new pattern) → Task 3's `WHERE id = $1 AND
  user_id = $2` shape, Global Constraints' 404-never-403 restatement.
- Non-goals → explicitly restated above, with each one's corresponding
  "not touched by any task" confirmed by omission from every task's File
  list.
- All three Open Questions → resolved in "Judgment calls this plan makes."
