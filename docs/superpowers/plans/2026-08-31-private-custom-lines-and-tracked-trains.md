# Private Custom Lines and Tracked Trains Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reverse two deliberate, previously-shipped "reads stay public" decisions, per the repo owner's own explicit instruction: custom lines and tracked trains must become private to the user who created them. Today `custom_lines.user_id` is nullable (a permanently-supported "orphaned, but still publicly readable" state) and every read route for both custom lines and tracked trains is either fully unauthenticated or merely *reports* ownership (`isOwner`) without enforcing it. This plan retrofits access control onto six backend read routes across two files, adds one bulk ownership-lookup helper, migrates `custom_lines.user_id` to `NOT NULL` (behind an explicit, gated go/no-go decision — see Task 1), and fixes six frontend read functions that today forward no cookies at all, which would otherwise 401 even the real owner the moment the backend is gated.

This is **not** a new feature — it is an access-control retrofit across more existing routes than any prior plan in this repo has touched at once, with no new database tables and only one new (already-fully-specified) migration.

**Architecture (net summary):**

```
frontend/lib/api.ts
  + ApiUnauthorizedError (NEW, mirrors ApiNotFoundError)
  getCustomLine            + cookie fwd, collapse 401 -> ApiNotFoundError
  getAllLines               + cookie fwd (custom-line entries now caller-scoped)
  getLineDefinition         + cookie fwd
  getLineStatus              + cookie fwd (private custom lines filtered server-side)
  getLineStatusForMode       + cookie fwd (private custom lines filtered server-side)
  getTrackedTrainById         + cookie fwd, 401 -> NEW ApiUnauthorizedError
  getTrackedTrainByUidAndDate + cookie fwd, 401 -> NEW ApiUnauthorizedError

frontend/app/lines/[id]/page.tsx           -- drop isOwner, isCustom && gate only
frontend/app/train/by-id/[trackingId]/page.tsx   -- NEW: ApiUnauthorizedError branch (login prompt)
frontend/app/train/[uid]/[date]/page.tsx         -- NEW: ApiUnauthorizedError branch (login prompt)
frontend/lib/types.ts                       -- drop CustomLineDetail.isOwner
        │ server-side fetch (cookie-forwarded, no-store)
        ▼
crates/api
 ├─ routes/lines.rs
 │   get_line               OptionalAuthenticatedUser -> AuthenticatedUser; 401/404; drop is_owner
 │   list_lines               + OptionalAuthenticatedUser; caller-scoped custom-line set
 │   get_line_definition      + OptionalAuthenticatedUser; 404 for a custom id the caller doesn't own
 ├─ routes/line_status.rs
 │   get_line_status          + OptionalAuthenticatedUser; filter private custom rows
 │   get_mode_status          + OptionalAuthenticatedUser; filter private custom rows
 │   get_line_status_history  + OptionalAuthenticatedUser; empty array for an unowned custom id
 │   get_stop_point_disruption   UNCHANGED (never serves custom lines -- confirmed by spec)
 ├─ routes/train.rs
 │   get_by_tracking_id       (none) -> AuthenticatedUser + tracked_train_owner check
 │   get_by_uid_and_date      (none) -> AuthenticatedUser + tracked_train_owner check
 │   ticket routes             UNCHANGED (already private)
 ├─ data/custom_lines.rs
 │   + owners_for_ids (NEW, bulk ownership lookup for line_status.rs)
 │   + list_custom_lines_for_user (NEW, caller-scoped variant of list_custom_lines)
 └─ data/train_tracking.rs        UNCHANGED (tracked_train_owner already exists)

crates/api/migrations/  NEW: custom_lines.user_id -> NOT NULL, with a placeholder-owner
                         reassignment step for any surviving legacy NULL rows
                         -- gated behind Task 1's explicit go/no-go decision
```

**Tech Stack:** Rust (axum, sqlx, `PgPool`) for the backend tasks; Next.js App Router + TypeScript, Vitest + `@testing-library/react` for the frontend tasks — same stack as every other plan in this repo, no new dependency anywhere.

**Spec:** `docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md` — read in full before starting; this plan does not restate its research or reasoning, only carries its decisions into concrete tasks. Cross-references below to "Decision N" refer to that document.

**Status note:** verified against the real current file structure while writing this plan (2026-09-01). `crates/api/src/routes/lines.rs`'s `get_line` currently takes `OptionalAuthenticatedUser` and returns a `CustomLineDetail` with an `is_owner: bool` field computed by the pure, already-unit-tested `is_owner()` function (5 existing tests in that file's `#[cfg(test)] mod tests`). `crates/api/src/data/custom_lines.rs`'s `get_custom_line` already returns `(CustomLine, Option<String>)` — the owner lookup this plan's single-id routes (`get_line`, `get_line_definition`) need already exists; only the **bulk** variant (`owners_for_ids`, for `line_status.rs`'s multi-row routes) and the caller-scoped list variant (`list_custom_lines_for_user`, for `list_lines`) are genuinely new. `crates/api/src/data/train_tracking.rs`'s `tracked_train_owner(pool, tracking_id) -> Result<Option<String>>` already exists and is already used by `post_ticket`/`get_tickets` in `train.rs` — Task 8 reuses it verbatim, no new query. `crates/api/src/auth.rs`'s `AuthenticatedUser`/`OptionalAuthenticatedUser` extractors are both already defined and already used elsewhere in this codebase. `frontend/lib/api.ts`'s cookie-forwarding pattern (`getPreferences`, `getSession`, `getTicketsForTrackedTrain`) already exists as a precedent to copy for every Task 10 function.

## Global Constraints

- **401/404 convention, everywhere in this plan, no exceptions:** no session at all → `401`, `"no session"` (from the `AuthenticatedUser` extractor itself — never write a bespoke 401 anywhere). Session present but the resource doesn't exist, isn't the caller's, or is a legacy NULL-owner row → `404`, reusing each route's own existing not-found message. **Never `403`, anywhere in this plan.** "Doesn't exist" and "exists but not yours" must stay indistinguishable to an external observer in every task below — do not add a distinguishing error message, field, or status code as a side effect of any task.
- **The destructive-vs-non-destructive migration fork is a real go/no-go decision, not a foregone conclusion.** Task 1 is a dedicated, non-code checkpoint that must be completed — and its outcome recorded — before Task 2 (writing the actual migration file) starts. Do not default silently to either path.
- **Catalogue and TfL lines stay fully, unconditionally publicly readable.** Every task touching `list_lines`, `get_line_definition`, or any `line_status.rs` route must leave the catalogue/TfL code paths completely untouched — only the custom-line branch/rows gain a filter. This is Tier 1 per `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` and must never be gated.
- **`get_stop_point_disruption` (`crates/api/src/routes/line_status.rs`) is out of scope — do not touch it.** Confirmed by the spec (and independently confirmed while writing this plan, by reading the handler): it builds its candidate line list from `app.config.lines` only, never `custom_lines`, so no custom line can ever appear in its response.
- **No change to `crates/aggregator`.** The privacy boundary is entirely a read-time concern in `crates/api`; the aggregator keeps computing one shared status per line, owner-blind.
- **No change to ticket routes** (`post_ticket`, `get_tickets`, `get_delay_repay_estimate`, `post_pkpass_upload`, `post_pdf_upload`) or to `data/train_tracking.rs`. Already fully private; not touched by any task here.
- **No change to `pinned_lines`/`pinned_stations` privacy.** Already fully user-scoped since the 2026-08-28 ownership retrofit — not implicated by anything in this plan.
- **Do not build the "shareable tracking link" opt-in.** Named in Decision 6 as a possible future, separate feature. No task in this plan may add a per-pin visibility flag, a UI for it, or any unauthenticated bypass route.
- **Do not build a distinct "log in again" prompt for `/lines/[id]`'s session-expiry edge case.** Decision 8 explicitly accepts a bare 404 there as a deliberate, minor tradeoff versus the train pages' distinct prompt (Decision 6) — do not "fix" this asymmetry as a side effect of any task.
- **Do not touch the tracked-trains-list feature** (`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`, plan at `docs/superpowers/plans/2026-08-31-tracked-trains-list.md`). Decision 7 confirms no reconciliation is needed; this plan does not implement, extend, or modify that feature.
- **Custom-line creation gating needs no task.** Confirmed already fully gated (Decision 4: `create_line` already requires `AuthenticatedUser`, `insert_custom_line` always attributes a real owner, `CustomLineForm.tsx` already has a working `needsLogin` 401 branch). Recorded here so it isn't mistaken for an oversight — no task implements it, because there is nothing to implement.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file for pure logic (this repo's existing convention — see `lines.rs`'s `is_owner` tests, `line_status.rs`'s `parse_modes`/`tfl_ids_to_overlay` tests); `#[ignore]`d live-database tests colocated in `mod db_tests` for anything that needs a real `PgPool`, following `custom_lines.rs`'s existing `get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row` test's exact fixture/cleanup shape. Frontend: colocated `*.test.ts`/`*.test.tsx`, Vitest (`npm test` from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures. Every backend task's verification step runs `cargo test -p api` (plus, where noted, the relevant `#[ignore]`d test run by hand against a real database) and requires it to pass with no new failures.
- **No task may modify `crates/trust-consumer`, `crates/common`, `crates/poller-tfl`, or `crates/aggregator`.** Every code change in this plan lives in `crates/api` or `frontend/`.

---

### Task 1: Migration go/no-go decision checkpoint — NOT a code task, a required prerequisite

**This task produces a recorded decision, not a diff.** Do not write Task 2's migration file until this task is complete and its outcome is written down (e.g. as a note in the PR description, commit message, or a line added to this plan file itself).

Per the spec's Decision 2 and its Open Question 2, there are two real mechanics for eliminating NULL-owner `custom_lines` rows, and the spec is explicit that choosing between them needs a fact about the live target database this design doc cannot supply:

- **Recommended default — reassign, don't delete:** insert a placeholder, unreachable `users` row (`'legacy-unclaimed'`) and `UPDATE custom_lines SET user_id = 'legacy-unclaimed' WHERE user_id IS NULL` before adding the `NOT NULL` constraint. Non-destructive — a NULL-owned row becomes exactly as unreachable as it is today (nobody can authenticate as the placeholder), while the authored content (name, stations, operators, headcode filters) survives and remains recoverable via an operator runbook.
- **Alternative, rejected as the default — destructive:** `DELETE FROM custom_lines WHERE user_id IS NULL` before adding the constraint. Simpler, "cleaner," and **irreversible** — real user-authored content is destroyed with no way back, contrary to this codebase's own established distinction (in `20260828100000_add_ownership.sql`'s own header comment) between `custom_lines` ("authored content, which IS carried forward") and `pinned_lines`/`pinned_stations` ("pure UI convenience state," already safely `TRUNCATE`d).

**This plan defaults to the non-destructive reassignment path (Task 2 is written for it) but does not treat that as decided.** Per this codebase's own "stop for destructive/irreversible operations" posture, an implementer must not silently pick either path — the delete alternative is only "equally valid" (the spec's own words) if the repo owner has verified, out-of-band, that no live deployment has NULL-owner rows worth keeping.

- [ ] **Step 1: Run the fact-finding query against the real target database**

```sql
SELECT count(*) FROM custom_lines WHERE user_id IS NULL;
```

Record the result. Per the spec's own reasoning about this app's "single trusted personal instance"-sized deployment posture, the realistic expectation is a small number (very plausibly zero) — but the number itself, not an assumption, is what this step exists to establish.

- [ ] **Step 2: Get an explicit go/no-go from the repo owner before Task 2 runs**

Present both mechanics above (reassign-and-migrate vs. delete-and-migrate) and the Step 1 count, and get an explicit choice — do not infer one from silence, and do not proceed past this task on the plan-writer's or implementer's own judgment alone. If the count is genuinely `0`, both mechanics are behaviorally identical (the `UPDATE` becomes a no-op either way) — even so, get the explicit choice recorded, since a `0` today doesn't retroactively justify skipping the check on a different target database later.

- [ ] **Step 3: Record the decision**

Write down which path was chosen and why (e.g. "reassign: default per spec, no reason to deviate" or "delete: repo owner confirmed count=0 and prefers no placeholder account"). This record is what Task 2 implements — Task 2's steps below assume the reassignment path was chosen; if delete was chosen instead, adapt Task 2's SQL accordingly and note the deviation from this plan explicitly in the migration file's own header comment.

---

### Task 2: Migration — `custom_lines.user_id` becomes `NOT NULL`

**Depends on Task 1 being complete and recorded.**

**Files:**
- Create: `crates/api/migrations/<timestamp>_custom_lines_owner_not_null.sql` (timestamp per this directory's existing `YYYYMMDDHHMMSS_description.sql` convention — see `crates/api/migrations/20260828100000_add_ownership.sql` for the precedent this migration continues)

**Interfaces:**
- Produces: a schema migration, picked up automatically by this crate's existing migration-runner startup path (no code change needed to run it — confirmed by how every prior `.sql` file in this directory is already picked up).

- [ ] **Step 1: Write the migration (assuming Task 1 chose reassignment — the default)**

```sql
-- -------------------------------------------------------------------------
-- Closes the NULL-owner state `20260828100000_add_ownership.sql` left open
-- on purpose. Per the repo owner's explicit instruction (see
-- docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md
-- Decision 2), a custom line with no real owner must become genuinely
-- impossible, not just application-layer-inaccessible.
--
-- Any surviving NULL-owner row is reassigned to an unreachable placeholder
-- account first, never deleted -- this repo's own ownership-retrofit
-- migration already drew a hard line between custom_lines ("authored
-- content, which IS carried forward") and pinned_lines/pinned_stations
-- ("pure UI convenience state", safely truncated). No sessions row can
-- ever reference 'legacy-unclaimed' except by an operator manually
-- crafting one, so a line "owned" by it is exactly as unreachable through
-- the app as a NULL-owned row is today.
--
-- UPDATED OPERATOR RUNBOOK (same shape as 20260828100000's own runbook,
-- re-pointed at a concrete id instead of NULL):
--   UPDATE custom_lines SET user_id = '<admin sub>' WHERE user_id = 'legacy-unclaimed';
-- -------------------------------------------------------------------------

INSERT INTO users (id, email, name)
VALUES ('legacy-unclaimed', NULL, 'Unclaimed legacy custom lines')
ON CONFLICT (id) DO NOTHING;

UPDATE custom_lines SET user_id = 'legacy-unclaimed' WHERE user_id IS NULL;

ALTER TABLE custom_lines ALTER COLUMN user_id SET NOT NULL;
```

If Task 1 instead recorded the destructive path, replace the middle statement with `DELETE FROM custom_lines WHERE user_id IS NULL;` and drop the `INSERT INTO users` statement — and say so explicitly in this migration file's own header comment, quoting Task 1's recorded rationale, so a future reader isn't left wondering why this migration diverged from the spec's own recommended default.

- [ ] **Step 2: Apply the migration against a local/dev database and verify**

Run this crate's normal migration path (however `crates/api` already runs its own migrations at startup/test time — check `crates/api`'s existing test setup or `main.rs` for the exact invocation before assuming a command). Confirm:
- `ALTER TABLE ... SET NOT NULL` succeeds (no leftover NULL rows).
- If Task 1's Step 1 count was nonzero, confirm those specific rows now show `user_id = 'legacy-unclaimed'` (reassignment path) — not silently dropped.
- A fresh `INSERT INTO custom_lines (...)` with no explicit `user_id` now fails at the database level (proves the constraint is real, not just assumed).

- [ ] **Step 3: Add a live-database migration test**

Per the spec's Testing section: a real, applied-and-verified test against a fixture database containing at least one legacy NULL-owner row, confirming it survives as `'legacy-unclaimed'`-owned (or is genuinely absent, if the delete path was chosen) rather than silently disappearing, and that the `NOT NULL` constraint then holds. Follow `custom_lines.rs`'s existing `#[ignore]`d `db_tests` pattern (seed a NULL-owner row via a raw `sqlx::query` INSERT exactly as that file's existing `get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row` test already does, run migrations, assert the row's `user_id` is now `'legacy-unclaimed'`, clean up). Place it in `crates/api/src/data/custom_lines.rs`'s `mod db_tests`, or a new colocated test module if this repo's migration-testing convention lives elsewhere — check for an existing migration-test pattern in `crates/api` before assuming one doesn't exist.

- [ ] **Step 4: Run the backend test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, including the new ignored test run explicitly (`cargo test -p api <test name> -- --ignored`, per this repo's existing convention for DB-dependent tests) against a real database.

- [ ] **Step 5: Commit**

```bash
git add crates/api/migrations/<timestamp>_custom_lines_owner_not_null.sql
git commit -m "Migrate custom_lines.user_id to NOT NULL, reassigning legacy NULL rows to a placeholder owner"
```

---

### Task 3: Backend — `owners_for_ids` bulk ownership lookup and `list_custom_lines_for_user`

**Files:**
- Modify: `crates/api/src/data/custom_lines.rs`

**Interfaces:**
- Produces: `pub async fn owners_for_ids(pool: &PgPool, ids: &[String]) -> Result<HashMap<String, Option<String>>>`, `pub async fn list_custom_lines_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<CustomLine>>`.
- Consumed by: Task 7 (`owners_for_ids`, `line_status.rs`'s three affected routes), Task 5 (`list_custom_lines_for_user`, `lines.rs`'s `list_lines`).

This task does not itself depend on Task 1/2 — both new functions operate correctly whether or not the `NOT NULL` constraint has landed yet (a NULL `user_id` simply never matches any real caller's id, exactly like today).

- [ ] **Step 1: Add `owners_for_ids`**

```rust
/// Owners for every custom-prefixed id in `ids`, for filtering a bulk
/// status response by ownership without an N+1 query per row (see
/// `crate::routes::line_status`'s three affected handlers). Catalogue/TfL
/// ids in `ids` simply won't match anything here -- callers should look
/// them up unconditionally in the returned map and treat "no entry" as
/// "not a custom line, leave it alone," never as "unowned."
pub async fn owners_for_ids(pool: &PgPool, ids: &[String]) -> Result<std::collections::HashMap<String, Option<String>>> {
    let rows = sqlx::query("SELECT id, user_id FROM custom_lines WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| Ok((row.try_get::<String, _>("id")?, row.try_get::<Option<String>, _>("user_id")?)))
        .collect()
}
```

- [ ] **Step 2: Add `list_custom_lines_for_user`**

```rust
/// Caller-scoped variant of [`list_custom_lines`] -- used by `list_lines`
/// (`GET /public/lines`) once custom lines become private, so an
/// authenticated caller sees only their own custom lines in the bulk list,
/// never anyone else's. Deliberately a separate function rather than an
/// `Option<&str>` parameter on `list_custom_lines` itself: the anonymous
/// case (Decision 8) skips the custom-line query entirely rather than
/// calling this with some sentinel, so the two call shapes never need to
/// share a signature.
pub async fn list_custom_lines_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<CustomLine>> {
    let rows = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CustomLine {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                operators: row.try_get("operators")?,
                stations: row.try_get("stations")?,
                headcode_prefixes: row.try_get("headcode_prefixes")?,
                destination_crs_filter: row.try_get("destination_crs_filter")?,
            })
        })
        .collect()
}
```

- [ ] **Step 3: Add tests**

Pure-logic unit test for `owners_for_ids`' *shape* is not meaningful without a database (it's a plain bulk SELECT), so per this file's existing convention, add an `#[ignore]`d `db_tests` test mirroring `get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row`'s exact fixture/cleanup pattern:
- `owners_for_ids` returns the real owner for an owned row, `None` for a legacy NULL-owner row (pre-Task-2-migration behavior, or `Some("legacy-unclaimed")` post-migration if Task 2 already landed — write the assertion to match whichever state Task 2 is in when this test runs, and note the dependency in the test's own comment), and simply omits any id in `ids` that has no `custom_lines` row at all (a catalogue/TfL id) — assert the returned map has no entry for that id, not a `None` entry.
- `list_custom_lines_for_user` returns only the calling user's own rows, not another user's, when both have custom lines.

- [ ] **Step 4: Compile-check and run the crate's test suite**

Run (from repo root): `cargo check -p api` then `cargo test -p api`.
Expected: PASS. Both new functions will show `dead_code` warnings until Tasks 5/7 consume them — acceptable if this task is done immediately before those, otherwise expect a transient warning.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/custom_lines.rs
git commit -m "Add owners_for_ids bulk lookup and list_custom_lines_for_user"
```

---

### Task 4: Backend — `GET /public/lines/{id}` (`get_line`): require ownership, drop `isOwner`

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Changes: `get_line`'s extractor from `OptionalAuthenticatedUser` to `AuthenticatedUser`; `CustomLineDetail` drops its `is_owner` field; the `is_owner()` pure function and its 5 existing tests are removed (no longer meaningful — any `200` from this endpoint is now by construction from the real owner).

Depends on nothing else in this plan (uses the existing `get_custom_line`, which already returns the owner alongside the row).

- [ ] **Step 1: Switch the extractor and collapse the three "not visible to this caller" cases into one 404**

```rust
async fn get_line(
    State(app): State<App>,
    Path(id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<CustomLineDetail>, (StatusCode, String)> {
    let line = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;

    // Doesn't exist, exists with no owner at all (legacy NULL row), and
    // exists but owned by someone else are all treated identically -- the
    // same 404, same message update_line/delete_line already use for
    // "exists but not yours" -- so an external observer gets no signal
    // distinguishing any of the three cases. No session at all never
    // reaches this line: AuthenticatedUser's own extractor already
    // rejected with 401 before this handler runs.
    let Some((line, owner)) = line else {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    };
    if owner.as_deref() != Some(user.id.as_str()) {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    Ok(Json(CustomLineDetail {
        id: line.id,
        name: line.name,
        operators: line.operators,
        stations: line.stations,
        headcode_prefixes: line.headcode_prefixes,
        destination_crs_filter: line.destination_crs_filter,
    }))
}
```

- [ ] **Step 2: Remove `CustomLineDetail.is_owner`, the `is_owner()` function, and its 5 tests**

Delete the `is_owner: bool` field from the `CustomLineDetail` struct (and its doc comment's explanation of the field, which no longer applies), delete the `is_owner()` function, and delete its 5 tests (`the_real_owner_is_reported_as_owner`, `a_logged_in_non_owner_is_not_reported_as_owner`, `an_anonymous_visitor_is_never_reported_as_owner`, `a_legacy_ownerless_line_is_never_reported_as_owner_even_when_logged_in`, `an_anonymous_visitor_against_a_legacy_ownerless_line_is_not_owner`). Update the module doc comment at the top of the file (currently states `GET /lines/{id}` is unauthenticated and describes `isOwner`'s purpose) to reflect the new behavior.

- [ ] **Step 3: Add route-level tests covering the full 401/404/200 matrix**

Per the spec's own flagged Open Question 3: there is no existing precedent in this codebase for an `AuthenticatedUser`-gated *read* route with test coverage at the HTTP layer — check `crates/api`'s existing integration-test setup (if any; grep for `tower::ServiceExt::oneshot` or similar test-request patterns across `crates/api/src`) before assuming a shape. If none exists, this establishes the pattern — a `#[ignore]`d live-database test that builds the router, seeds a real session + user + custom line, and issues real requests via `tower::ServiceExt::oneshot`, is a reasonable default matching this file's existing `db_tests` fixture/cleanup conventions. Cover:
  - No session cookie at all → `401`.
  - Session for a user who isn't the owner → `404`.
  - A legacy NULL-owner row (or `'legacy-unclaimed'`-owned, if Task 2 already landed) with a real, logged-in caller → `404`.
  - A nonexistent id with a real, logged-in caller → `404`.
  - The real owner's own session → `200`, full `CustomLineDetail`, no `isOwner` field in the JSON body.
  - A catalogue-id request (any `id` in `app.config.lines`) still 404s the same way it always has — confirm this path is untouched.

- [ ] **Step 4: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, no leftover references to the removed `is_owner` tests/function.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "Require ownership on GET /public/lines/{id}, drop the now-vestigial isOwner field"
```

---

### Task 5: Backend — `list_lines` (`GET /public/lines`): scope the custom-line section to the caller

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Changes: `list_lines` gains an `OptionalAuthenticatedUser` parameter; its unconditional `custom_lines::list_custom_lines(&app.database)` call is replaced by a caller-scoped branch.

Depends on Task 3 (`list_custom_lines_for_user`).

- [ ] **Step 1: Branch the custom-line section on the caller's session**

```rust
async fn list_lines(
    State(app): State<App>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<LineSummary>>, (StatusCode, String)> {
    let mut out: Vec<LineSummary> = app.config.lines.iter().map(/* unchanged */).collect();

    // Custom lines are now private (see get_line, Task 4) -- an
    // authenticated caller sees only their own; an anonymous visitor sees
    // none at all, same shape as today's default but now also true for a
    // logged-in non-owner. Catalogue and TfL entries below are completely
    // unaffected -- no filtering, no auth requirement change.
    if let Some(user) = &user {
        let custom = custom_lines::list_custom_lines_for_user(&app.database, &user.id)
            .await
            .map_err(internal_error)?;
        out.extend(custom.into_iter().map(|c| LineSummary { /* unchanged mapping */ }));
    }

    // TfL section: entirely unchanged.
    ...
    Ok(Json(out))
}
```

(Keep the existing catalogue and TfL blocks byte-for-byte identical — only the custom-line block changes.)

- [ ] **Step 2: Add tests**

Extend the route-level test suite established in Task 4 Step 3 (or start it here if Task 4 hasn't landed yet in the actual implementation order — check):
  - Anonymous request: response contains catalogue and TfL entries, zero custom-line entries, even if custom lines exist in the database.
  - Logged-in caller with one owned custom line and one other user's custom line in the database: response contains their own custom line, not the other user's.
  - Catalogue/TfL entries present and identical regardless of session state.

- [ ] **Step 3: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "Scope list_lines' custom-line section to the authenticated caller"
```

---

### Task 6: Backend — `get_line_definition` (`GET /public/lines/{id}/definition`): gate the custom-line branch

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Changes: `get_line_definition` gains an `OptionalAuthenticatedUser` parameter; its existing custom-line branch (unchanged catalogue-first branch returns before this) adds an ownership check.

Depends on nothing else in this plan (reuses `get_custom_line`'s existing owner return, same as Task 4).

- [ ] **Step 1: Add the ownership check to the custom-line branch only**

```rust
async fn get_line_definition(
    State(app): State<App>,
    Path(id): Path<String>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<LineDefinitionSummary>, (StatusCode, String)> {
    if let Some(catalogue_line) = app.config.lines.iter().find(|l| l.id == id) {
        return Ok(Json(LineDefinitionSummary { /* unchanged */ }));
    }

    let custom = custom_lines::get_custom_line(&app.database, &id)
        .await
        .map_err(internal_error)?;
    let Some((custom, owner)) = custom else {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    };
    let caller_owns_it = matches!((&user, &owner), (Some(u), Some(o)) if &u.id == o);
    if !caller_owns_it {
        return Err((StatusCode::NOT_FOUND, "line not found".to_string()));
    }

    Ok(Json(LineDefinitionSummary { stations: custom.stations, operators: custom.operators }))
}
```

- [ ] **Step 2: Add tests**

Extend the same route-level suite: anonymous/non-owner/legacy-NULL/nonexistent all 404 for a custom id; real owner gets `200`; any catalogue id returns its definition regardless of session, unchanged.

- [ ] **Step 3: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "Gate GET /public/lines/{id}/definition's custom-line branch by ownership"
```

---

### Task 7: Backend — `line_status.rs`: filter custom-line rows by ownership in three routes

**Files:**
- Modify: `crates/api/src/routes/line_status.rs`

**Interfaces:**
- Changes: `get_line_status`, `get_mode_status`, `get_line_status_history` each gain an `OptionalAuthenticatedUser` parameter and a filtering step. `get_stop_point_disruption` is explicitly unchanged (Global Constraints).

Depends on Task 3 (`owners_for_ids`). This is the one place this plan introduces genuinely new filtering logic beyond swapping an extractor — per the spec, worth extra care.

All three routes share the same shape of filter: any row whose `id` starts with `custom-` (guaranteed prefix — `custom_lines::slugify` always produces `format!("custom-{slug}")`, no other id shape reaches `line_status`) is dropped unless its `owners_for_ids` entry equals `Some(caller.id)`. A non-`custom-`-prefixed id (catalogue or TfL) is always kept, untouched.

- [ ] **Step 1: Add a shared filter helper**

```rust
/// Drops any row whose id is a private custom line the caller doesn't own.
/// Catalogue/TfL rows (no `custom-` prefix) are always kept untouched.
/// `user` is `None` for an anonymous caller -- every custom-line row is
/// dropped for them, since an anonymous caller can never be the owner of
/// anything.
async fn filter_private_custom_rows(
    pool: &sqlx::PgPool,
    rows: Vec<queries::LineStatusRow>,
    user: &Option<crate::auth::AuthenticatedUser>,
) -> anyhow::Result<Vec<queries::LineStatusRow>> {
    let custom_ids: Vec<String> = rows.iter().filter(|r| r.id.starts_with("custom-")).map(|r| r.id.clone()).collect();
    if custom_ids.is_empty() {
        return Ok(rows);
    }
    let owners = custom_lines::owners_for_ids(pool, &custom_ids).await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let Some(owner) = owners.get(&row.id) else {
                return true; // not a custom-line row after all (shouldn't happen given the prefix check, but never drop on a lookup miss)
            };
            match (user, owner) {
                (Some(caller), Some(owner_id)) => &caller.id == owner_id,
                _ => false,
            }
        })
        .collect())
}
```

Add `use crate::data::custom_lines;` and `use crate::auth::OptionalAuthenticatedUser;` to this file's imports.

- [ ] **Step 2: `get_mode_status` — silently drop, never error the whole request**

```rust
async fn get_mode_status(
    State(app): State<App>,
    Path(modes): Path<String>,
    Query(query): Query<DetailQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let modes = parse_modes(&modes).map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let rows = queries::line_status_for_modes(&app.database, &modes).await.map_err(internal_error)?;
    let rows = filter_private_custom_rows(&app.database, rows, &user).await.map_err(internal_error)?;
    Ok(Json(rows_to_json(rows, query.detail)))
}
```

An anonymous visitor sees zero custom lines in the bulk feed; a logged-in visitor sees only their own. This is list filtering, matching `list_lines`' (Task 5) and every other "browse everything you're allowed to see" surface in this codebase.

- [ ] **Step 3: `get_line_status` — filter before the existing empty-check**

```rust
async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();
    let rows = queries::line_status_for_ids(&app.database, &ids).await.map_err(internal_error)?;
    let rows = filter_private_custom_rows(&app.database, rows, &user).await.map_err(internal_error)?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no matching line(s): {}", ids.join(","))));
    }
    // ... rest (TfL overlay fetch, response assembly) unchanged, operating on the now-filtered `rows`.
}
```

A request whose only requested id is a custom line the caller doesn't own falls straight into the existing `"no matching line(s): {ids}"` 404 an unknown id already produces — no new branch, no new status code.

- [ ] **Step 4: `get_line_status_history` — same non-distinguishing "empty" treatment this route already gives any unknown id**

```rust
async fn get_line_status_history(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if id.starts_with("custom-") {
        let owners = custom_lines::owners_for_ids(&app.database, std::slice::from_ref(&id)).await.map_err(internal_error)?;
        let owned_by_caller = match (&user, owners.get(&id)) {
            (Some(caller), Some(Some(owner_id))) => &caller.id == owner_id,
            _ => false,
        };
        if !owned_by_caller {
            return Ok(Json(vec![])); // identical shape to a genuinely unknown id -- this route has never distinguished the two.
        }
    }

    let history = queries::line_status_history_for_range(&app.database, &id, from, to).await.map_err(internal_error)?;
    // ... rest unchanged.
}
```

This route has no existence check at all today for *any* id — an unknown id already just returns an empty array. This closes the leak without adding a response shape the route has never had. Per the spec's own flagged Open Question 4, this is worth a second look at implementation time (it falls out of a pre-existing accident, not a deliberate prior design choice) — implement as specified here, but if it turns out to feel wrong in practice, that is a legitimate follow-up, not something to silently "fix" mid-task by inventing a 404 this route has never had for anything else.

- [ ] **Step 5: Add tests**

Route-level tests (same DB-test convention as Task 4):
  - `get_mode_status`: a request spanning `national-rail` plus a private custom line returns catalogue/TfL rows always, the custom-line row only when the caller owns it.
  - `get_line_status`: requesting `[catalogue_id, private_custom_id_not_owned]` returns just the catalogue row; requesting `[private_custom_id_not_owned]` alone 404s with the existing "no matching line(s)" message; requesting `[private_custom_id_owned]` returns it.
  - `get_line_status_history`: an owned custom id returns real history; a not-owned custom id returns `[]`; an unknown id still returns `[]` (regression check that this pre-existing behavior is unchanged); a catalogue id is completely unaffected.
  - `get_stop_point_disruption`: unchanged, add a regression test only if none already covers "never returns a custom line" — otherwise no new test needed here.

- [ ] **Step 6: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/line_status.rs
git commit -m "Filter private custom-line rows out of the three line_status.rs bulk/history routes"
```

---

### Task 8: Backend — tracked-train reads (`get_by_tracking_id`, `get_by_uid_and_date`): require ownership

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Changes: both handlers gain an `AuthenticatedUser` parameter and a `tracked_train_owner` check, reusing the exact pattern `post_ticket`/`get_tickets` already use in this same file.

Depends on nothing else in this plan — `tracked_train_owner` already exists (`crates/api/src/data/train_tracking.rs`), no schema change needed (`tracked_trains.user_id` has been `NOT NULL` since birth).

- [ ] **Step 1: Gate `get_by_tracking_id`**

```rust
async fn get_by_tracking_id(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?;
    match state {
        Some(state) => Ok(Json(blend_darwin_eta(&app, state).await)),
        None => Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }
}
```

- [ ] **Step 2: Gate `get_by_uid_and_date`**

This route resolves by `(train_uid, date)`, not `id` — the ownership check needs the row's `id` first, so fetch state, then check ownership against `state.id`, before returning it:

```rust
async fn get_by_uid_and_date(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_uid_and_date(&app.database, &train_uid, date)
        .await
        .map_err(internal_error("read tracked train state"))?;
    let Some(state) = state else {
        return Err((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string()));
    };

    match train_tracking::tracked_train_owner(&app.database, state.id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string())),
    }

    Ok(Json(blend_darwin_eta(&app, state).await))
}
```

Each route keeps its own existing not-found message for both "doesn't exist" and "exists but isn't the caller's" — no new message, no way to distinguish the two from the response alone.

- [ ] **Step 3: Add tests**

Mirror the ticket routes' own `tracked_train_owner`-based test coverage if it exists (check `crates/api`'s test setup first, per the spec's own Open Question 3 — this may be the first route-level test for either surface). Cover:
  - No session → `401`.
  - Session for a user who didn't create this pin → `404`, same message as "doesn't exist."
  - Nonexistent `tracking_id` / unresolved `(uid, date)` pair → `404`, unchanged message.
  - The real owner's own session → `200`, full state, `blend_darwin_eta` overlay still applied.

- [ ] **Step 4: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, including `router_builds_without_panicking` (unaffected by an extractor change, but still the regression check for the route table itself).

- [ ] **Step 5: Run the full backend build**

Run (from repo root): `cargo build --workspace`
Expected: PASS, no new warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Require ownership on GET /Train/{trackingId} and GET /Train/by-uid/{uid}/{date}"
```

---

### Task 9: Frontend — `ApiUnauthorizedError` type and `errorForResponse` mapping

**Files:**
- Modify: `frontend/lib/api.ts`

**Interfaces:**
- Produces: `export class ApiUnauthorizedError extends Error {}` (mirrors `ApiNotFoundError`'s existing shape), `errorForResponse` maps `401` to it.
- Consumed by: Task 10 (`getTrackedTrainById`/`getTrackedTrainByUidAndDate`), Task 11 (both train page components).

This is purely additive to the shared error-mapping path — every other existing caller of `errorForResponse`/`fetchJson` that currently treats a 401 as a bare `Error` will, after this change, get an `ApiUnauthorizedError` instead (still an `Error` subclass, still thrown, still uncaught unless a call site explicitly checks for it) — confirm no existing call site was relying on 401 falling into the generic `Error` branch by name before landing this (grep `frontend/` for `instanceof Error` checks that aren't `instanceof ApiNotFoundError`/narrower, as a sanity check).

- [ ] **Step 1: Add the class and extend `errorForResponse`**

```ts
/** Thrown when the API responds 401 -- lets callers distinguish "not logged
 * in at all" from `ApiNotFoundError`'s "doesn't exist / isn't yours"
 * (which stays deliberately indistinguishable from each other, per this
 * app's 401-vs-404 convention -- see
 * docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md). */
export class ApiUnauthorizedError extends Error {}

function errorForResponse(url: string, response: Response): Error {
  const message = `API request to ${url} failed: ${response.status} ${response.statusText}`;
  if (response.status === 404) return new ApiNotFoundError(message);
  if (response.status === 401) return new ApiUnauthorizedError(message);
  return new Error(message);
}
```

- [ ] **Step 2: Add tests**

Extend `frontend/lib/api.test.ts`: a call through the shared `fetchJson` path against a mocked `401` response throws `ApiUnauthorizedError`, not a bare `Error`; a `404` still throws `ApiNotFoundError`, unchanged; a `500` still throws a bare `Error`, unchanged.

- [ ] **Step 3: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add ApiUnauthorizedError, mapped from a 401 response"
```

---

### Task 10: Frontend — cookie-forwarding fixes across six read functions

**Files:**
- Modify: `frontend/lib/api.ts`

**Interfaces:**
- Changes: `getTrackedTrainById`, `getTrackedTrainByUidAndDate`, `getCustomLine`, `getAllLines`, `getLineStatus`, `getLineStatusForMode`, `getLineDefinition` all gain cookie-forwarding (**seven** functions total — the spec's own count; `getLineStatus` and `getLineStatusForMode` are the two halves of what the spec's table calls "getLineStatus(ForMode)").

Depends on Task 9 (`ApiUnauthorizedError`) for `getTrackedTrainById`/`getTrackedTrainByUidAndDate`'s new error branch. This is a **required precondition**, not an optional cleanup — every backend task above (4–8) gates a route these functions call; without this fix, gating the backend 401s even the legitimate owner, because none of these six functions forward the incoming request's `Cookie` header today, and a Server Component's own `fetch` never inherits it automatically.

- [ ] **Step 1: `getTrackedTrainById` / `getTrackedTrainByUidAndDate` — cookie-forward, map 401 to `ApiUnauthorizedError`**

```ts
export async function getTrackedTrainById(id: number): Promise<TrackedTrainState> {
  const url = `${baseUrl()}/Train/${id}`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<TrackedTrainState>;
}

export async function getTrackedTrainByUidAndDate(uid: string, date: string): Promise<TrackedTrainState> {
  const url = `${baseUrl()}/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<TrackedTrainState>;
}
```

Both now throw `ApiUnauthorizedError` for a 401 (via `errorForResponse`, Task 9) and `ApiNotFoundError` for a 404 — two distinct exceptions, consumed by Task 11's two page components to show two different things.

- [ ] **Step 2: `getCustomLine` — cookie-forward, collapse 401 into `ApiNotFoundError`**

```ts
export async function getCustomLine(id: string): Promise<CustomLineDetail> {
  const url = `${baseUrl()}/public/lines/${id}`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401 || response.status === 404) {
    throw new ApiNotFoundError(`API request to ${url} failed: ${response.status}`);
  }
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<CustomLineDetail>;
}
```

**Deliberately does not use the new `ApiUnauthorizedError` here**, unlike Step 1 — this is not an oversight, it's the spec's own explicit reasoning (Decision 8), and Task 12's page-level code depends on this exact collapse: on `/lines/[id]`, "not logged in" and "logged in but not the owner" already render identically both before and after this change (both just see a 404), and there's no scenario on this page where "please log in, this might be yours" is worth a distinct prompt the way it is on the single-purpose tracked-train page — the default, common case for this page is a public catalogue line most visitors have no reason to think they own.

- [ ] **Step 3: `getAllLines`, `getLineStatus`, `getLineStatusForMode`, `getLineDefinition` — cookie-forward only, no error-shape change**

None of these four need a new error branch — they keep throwing whatever `errorForResponse` already produces (now including the new `ApiUnauthorizedError` for a bare 401, which none of these four should actually ever produce for these particular routes given the `OptionalAuthenticatedUser` gating Tasks 5–7 use, but the mapping is correct and harmless either way). Only the cookie-forwarding is new:

```ts
export async function getAllLines(): Promise<LineSummary[]> {
  const url = `${baseUrl()}/public/lines`;
  const cookieHeader = (await cookies()).toString();
  return fetchJson<LineSummary[]>(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
}

export async function getLineStatusForMode(mode: string): Promise<LineStatusReport[]> {
  const url = `${baseUrl()}/Line/Mode/${mode}/Status`;
  const cookieHeader = (await cookies()).toString();
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
}

export async function getLineStatus(ids: string[], detail: boolean): Promise<LineStatusReport[]> {
  const idsParam = ids.join(',');
  const query = detail ? '?detail=true' : '';
  const url = `${baseUrl()}/Line/${idsParam}/Status${query}`;
  const cookieHeader = (await cookies()).toString();
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
}

export async function getLineDefinition(id: string): Promise<LineDefinitionSummary> {
  const url = `${baseUrl()}/public/lines/${id}/definition`;
  const cookieHeader = (await cookies()).toString();
  return fetchJson<LineDefinitionSummary>(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
}
```

(`fetchJson` itself is unchanged — it already just forwards whatever `init` it's given, so passing a `headers` object through it works with no change to that helper.)

- [ ] **Step 4: Add/extend tests**

For each of the seven functions touched in Steps 1–3, following `getTicketsForTrackedTrain`'s existing test shape:
  - Cookie forwarding: mock an incoming `Cookie` header, assert the outgoing `fetch` call includes it.
  - No incoming cookie: assert no `Cookie` header is sent (the existing `...(cookieHeader ? {...} : {})` pattern already handles this — verify, don't newly implement).
  - `getTrackedTrainById`/`getTrackedTrainByUidAndDate`: a `401` mock throws `ApiUnauthorizedError` specifically (not just "throws"); a `404` mock still throws `ApiNotFoundError`.
  - `getCustomLine`: both a `401` mock and a `404` mock throw `ApiNotFoundError` — same exception type, not distinguishable from the outside, confirming the collapse.

- [ ] **Step 5: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: PASS, all new/extended cases included.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Forward cookies on six previously-anonymous reads; collapse getCustomLine's 401 into ApiNotFoundError"
```

---

### Task 11: Frontend — train pages: distinct login-prompt branch for `ApiUnauthorizedError`

**Files:**
- Modify: `frontend/app/train/by-id/[trackingId]/page.tsx`
- Modify: `frontend/app/train/[uid]/[date]/page.tsx`
- Modify (or create): matching `*.test.tsx` files for both pages.

**Interfaces:**
- Changes: both pages' existing `try { ... } catch (err) { if (err instanceof ApiNotFoundError) notFound(); throw err; }` block gains a second branch for `ApiUnauthorizedError`.

Depends on Task 9 (`ApiUnauthorizedError`) and Task 10 Step 1 (both `getTrackedTrain*` functions now actually throw it).

This is a deliberate departure from Task 12's custom-line page (which collapses 401 into 404) — worth restating in the code comment, not just doing silently: a tracked-train page has no public sibling content one route below it the way a custom line's detail page does (catalogue lines at other ids), so a bare 404 here would be a materially worse experience for a visitor who genuinely owns the train but whose session lapsed — "log in, this might be yours" is the more honest message this single-purpose page can afford.

- [ ] **Step 1: `frontend/app/train/by-id/[trackingId]/page.tsx`**

```tsx
import { ApiNotFoundError, ApiUnauthorizedError, getTrackedTrainById } from '@/lib/api';
// ... other imports unchanged

  let state;
  try {
    state = await getTrackedTrainById(Number(trackingId));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    // Distinct from the custom-line detail page's 401-collapses-into-404
    // choice (see frontend/app/lines/[id]/page.tsx and its own comment) --
    // this page has no public sibling content to fall back to, so a
    // dedicated "log in, this might be yours" prompt is more honest than a
    // bare 404 for a real owner whose session lapsed.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Tracking Train {trackingId}</Title>
          <TextLink href="/api/auth/login" underline="always">
            Log in to view this tracked train
          </TextLink>
        </Stack>
      );
    }
    throw err;
  }
```

- [ ] **Step 2: `frontend/app/train/[uid]/[date]/page.tsx`**

Same shape, adapted to this page's existing `<Title>` text (`Train {uid}`) and imports — this file does not currently import `TextLink`, so add that import alongside `ApiUnauthorizedError`.

- [ ] **Step 3: Add/extend tests**

For both pages: mock `getTrackedTrainById`/`getTrackedTrainByUidAndDate` to reject with `ApiUnauthorizedError`, assert the login-prompt renders with a link to `/api/auth/login`; mock a rejection with `ApiNotFoundError`, assert `notFound()` still fires (i.e. the existing behavior is unchanged, not silently swallowed by the new branch); mock a rejection with a bare `Error`, assert it still propagates uncaught (i.e. `throw err` at the bottom of the catch block is still reachable and not accidentally shadowed by the new `if`).

- [ ] **Step 4: Run the test suite**

Run (from `frontend/`): `npm test`
Expected: PASS, including both pages' extended/new test files.

- [ ] **Step 5: Run the full build**

Run (from `frontend/`): `npm run build`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/app/train/by-id/[trackingId]/page.tsx frontend/app/train/[uid]/[date]/page.tsx
git commit -m "Show a distinct login prompt for a 401 on either tracked-train detail page"
```

(Include the test file paths in the `git add` list too.)

---

### Task 12: Frontend — `/lines/[id]/page.tsx`: drop `isOwner`, simplify the Edit/Delete gate

**Files:**
- Modify: `frontend/app/lines/[id]/page.tsx`
- Modify: `frontend/lib/types.ts`

**Interfaces:**
- Changes: `CustomLineDetail` (types.ts) drops its `isOwner: boolean` field, matching Task 4's backend removal. `LineDetailPage` drops its `isOwner` local variable and the `custom.isOwner` read; the Edit/Delete gate simplifies from `isCustom && isOwner &&` to `isCustom &&`.

Depends on Task 4 (backend no longer sends `isOwner` in the JSON body — this task is the frontend catching up) and Task 10 Step 2 (`getCustomLine` now collapses any non-owner/anonymous caller's response into `ApiNotFoundError`, which is *why* the gate simplification below is safe: by the time this page's Edit/Delete gate is reached, `getCustomLine` has already thrown and the whole page has already 404d for any caller who isn't the true owner — there is no remaining path where `isCustom` is `true` and the viewer isn't the owner).

- [ ] **Step 1: Drop `isOwner` from `CustomLineDetail` (`frontend/lib/types.ts`)**

Remove the `isOwner: boolean;` field and its doc-comment sentence explaining how it's computed.

- [ ] **Step 2: Simplify the page**

```tsx
  let isCustom = true;
  try {
    await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      isCustom = false;
    } else {
      throw err;
    }
  }
```

(Drop the `isOwner` local entirely — the call's return value is no longer even needed for its data, only for whether it throws, though keep assigning it to a variable if any other part of the page reads fields off it elsewhere; confirm by re-reading the full file before deleting the binding outright.)

```tsx
          {isCustom && (
            <>
              <Link href={`/lines/${id}/edit`} style={{ textDecoration: 'none' }}>
                <Button variant="outline" size="xs">Edit</Button>
              </Link>
              <DeleteLineButton id={id} />
            </>
          )}
```

Update the surrounding comment block (currently explains why `isOwner` is the real gate) to instead explain the new invariant: `getCustomLine` already 404s the whole page for any non-owner before this line is ever reached, so `isCustom` alone is now sufficient — reference Task 10 Step 2 / Decision 8 in the comment rather than re-deriving the reasoning from scratch.

- [ ] **Step 3: Update/add tests**

If `frontend/app/lines/[id]/page.test.tsx` (or equivalent) exists, remove any test asserting `isOwner`-driven visibility differences (e.g. "owner sees Edit, non-owner logged in doesn't") and replace with: Edit/Delete render whenever `isCustom` is true (i.e. `getCustomLine` resolved), and the whole page 404s (via `getLineStatus`'s own filtering from Task 7, or `getCustomLine`'s 404 collapse from Task 10) for a non-owner before Edit/Delete visibility is even a question. If no such test file exists yet, this is optional — do not create new test surface purely for this simplification unless an existing test needs updating to keep passing.

- [ ] **Step 4: Run the test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, no leftover references to `CustomLineDetail.isOwner` anywhere in `frontend/`.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/[id]/page.tsx frontend/lib/types.ts
git commit -m "Drop the now-vestigial isOwner field; getCustomLine's 404 is the real gate"
```

(Include any updated test file in the `git add` list.)

---

## Sequencing notes

- **Task 1 must complete, with its decision recorded, before Task 2.** This is a hard gate, not a soft recommendation — Task 2 as written assumes Task 1's default (reassignment) outcome; if the destructive path is chosen instead, Task 2's Step 1 must be adapted accordingly before that task's SQL is written, not after.
- **Task 2 (migration) has no hard dependency on Tasks 4–8** — the route-level 404-for-a-legacy/NULL-owner behavior (Task 4, Task 6, Task 7's history route) is written to be correct whether or not the `NOT NULL` constraint has landed yet, since a NULL `user_id` never equals a real caller's id either way. Task 2 can run any time after Task 1, in parallel with the backend route tasks if convenient — it does not block them, and they do not block it.
- **Task 3 (`owners_for_ids`, `list_custom_lines_for_user`) blocks Task 5 and Task 7** (both consume one of its two new functions) but nothing else.
- **Tasks 4, 6 have no dependency on Task 3** — both reuse the existing single-id `get_custom_line`, which already returns the owner.
- **Recommended backend order:** 1 → 2 (or 2 run later in parallel, per the note above) → 3 → 4 → 5 → 6 → 7 → 8. 4/5/6 all touch `lines.rs` and are easiest to land as one contiguous run to avoid repeated merge friction in the same file, but are written as independently completable tasks (each has its own commit) in case they need to land separately.
- **Frontend Task 9 has no backend dependency** and could be done any time — it's pure additive infrastructure (a new exception class) with no caller yet.
- **Task 10 depends on Task 9** (for the `ApiUnauthorizedError` mapping) and is the **hard precondition** for Tasks 4–8's backend changes to work end-to-end for a real logged-in owner — per the spec, this is not an optional cleanup, and ideally lands in the same work session as (or immediately after) the backend tasks it unblocks, so there's no window where the backend is gated but the frontend still can't authenticate its own requests.
- **Task 11 depends on Task 9 and Task 10 Step 1.**
- **Task 12 depends on Task 4 (backend field removal) and Task 10 Step 2 (`getCustomLine`'s 401-to-404 collapse), and should land no earlier than both.**
- **Recommended overall order:** 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12 — backend-then-frontend, matching this repo's own established precedent (`docs/superpowers/plans/2026-08-31-tracked-trains-list.md`), adapted here for a wider backend surface (five routes across two files, plus a migration) before any frontend work begins, since — unlike a from-scratch feature — every frontend task in this plan is a *fix* that only becomes verifiable end-to-end once its corresponding backend gate actually exists.

## Open questions carried forward from the spec (not resolved by this plan)

1. **Whether a "shareable tracking link" opt-in is wanted as a later, separate feature** (spec Open Question 1 / Decision 6). Not designed or built here — this plan implements the literal, unconditional instruction. If wanted, it is new, separate feature surface (a per-pin flag, a UI to set it, a distinct unauthenticated read path gated by that flag), out of scope for every task above.
2. **The exact destructive-vs-non-destructive migration mechanics** (spec Open Question 2) are the subject of this plan's own Task 1 — carried forward as a live decision point rather than resolved by this plan itself, per the instruction to make this impossible to miss rather than silently pick a default.
3. **No existing precedent in this codebase for an `AuthenticatedUser`-gated *read* route with integration-test coverage** (spec Open Question 3). Tasks 4, 5, 6, 7, and 8 all flag this at their own "Add tests" step rather than assuming a shape — whichever task in the actual implementation order hits this first establishes the pattern for the rest to follow.
4. **`get_line_status_history`'s "no distinction, just empty" treatment for a private custom line** (spec Open Question 4, implemented in Task 7 Step 4) matches that route's pre-existing behavior for any unknown id, but was never a deliberate design choice before now. Implemented as specified; flagged as worth a second look once it's load-bearing for privacy specifically, not something this plan resolves differently.
5. **No distinct "log in again" prompt for `/lines/[id]`'s session-expiry edge case** (Decision 8's own named tradeoff, restated in this plan's Global Constraints). Not built here, per the spec's own explicit acceptance of this as a reasonable minor cost — a real, deliberate asymmetry versus Task 11's train-page treatment, not an inconsistency to quietly fix.
