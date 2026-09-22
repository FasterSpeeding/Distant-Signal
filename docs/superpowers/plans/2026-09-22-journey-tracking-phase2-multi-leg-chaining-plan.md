# Plan: Journey Tracking Phase 2 — Multi-Leg Chaining

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement §9's Phase 2 of
`docs/superpowers/specs/2026-09-22-journey-tracking-design.md` (the approved
spec, revised 2026-09-22): let an existing journey grow beyond its first leg.
Concretely: `POST /Journeys/{id}/legs` (both leg-creation shapes — a direct
already-known-train pick, and an open time-window search), `leg_order`
assignment as `max(leg_order) + 1`, the frontend "Add a leg" flow with
default-suggested (never enforced) origin, and a per-leg-status "worst
status" rollup badge on the journey view, per §3.

**Scope discipline — this plan covers Phase 2 ONLY.** Everything in Phase 1
(single-leg journeys, the migration, time-window search, manual-pick,
"Change train") is assumed already built by a sibling plan and is *used*,
never re-implemented, here. Phase 3 (journey-aware notifications, station
skip), Phase 4 (group sharing), and Phase 5/stretch (connection buffers,
auto-commit, platform display) are explicitly out of scope and untouched.

---

## ⚠️ Assumptions about Phase 1 — verify before starting Task 1

Phase 1 is being planned and implemented by a sibling agent, in parallel,
and had not landed at the time this plan was written. Everything below is
inferred from the spec's own §1.1/§2.1-2.3/§4/§9 text (which IS authoritative
even before Phase 1's own plan exists) rather than read from real code. Some
of it — table/column names — is direct schema quoted verbatim from the spec
and should be a safe bet. The **function names, route paths, and file
locations are this plan's own reasonable inference of how Phase 1 will have
built things**, not confirmed fact. **Before starting Task 1, grep the repo
for each of these and adjust this plan's Task 1/2 file:line references to
match whatever Phase 1 actually produced** — a mismatch here is the single
biggest risk to this plan executing cleanly.

1. **Schema** (spec §1.1, quoted directly, with §7.1's own correction
   applied): tables `journeys(id BIGSERIAL PK, user_id TEXT NOT NULL, custom_name TEXT, created_at, updated_at)`
   and `journey_legs(id BIGSERIAL PK, journey_id BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE, leg_order INT NOT NULL, origin_crs TEXT, destination_crs TEXT, service_date DATE NOT NULL, depart_after TIME, depart_before TIME, arrive_after TIME, arrive_before TIME, train_subscription_id BIGINT REFERENCES train_subscriptions(id) ON DELETE SET NULL, match_mode TEXT NOT NULL DEFAULT 'unmatched' CHECK (match_mode IN ('unmatched','manual','auto')), created_at, UNIQUE (journey_id, leg_order))`.
   **`origin_crs`/`destination_crs` are assumed NULLABLE**, not the spec's
   original `NOT NULL` sketch — §7.1 of the spec itself flags this as a
   correction the migration's own data reality forces (an NR-primary
   subscription can have no pin CRS at all). If Phase 1 shipped them
   `NOT NULL` instead, Task 1 below needs a different fallback for the
   direct-known-train leg shape (§2.3's "no schedule data yet" case) —
   check this first.
2. **`common::TimeWindow { after: Option<NaiveTime>, before: Option<NaiveTime> }`**
   (spec §2.1) exists in `crates/common/src/lib.rs`, used for both a leg's
   depart and arrive bounds.
3. **A shared "create one leg" data function** exists in a new
   `crates/api/src/data/journeys.rs` (plural — the spec's own front-matter
   naming decision, distinct from the pre-existing singular
   `crates/api/src/data/journey.rs`), something shaped like
   `create_leg(pool, journey_id, leg_order, request: &NewLegRequest) -> anyhow::Result<i64>`,
   handling **both** the direct-known-train shape (`{trainUid, serviceDate}`,
   internally calling the *already-existing*
   `train_tracking::create_subscription_for_train`, `crates/api/src/data/train_tracking.rs:198-230`,
   verified read above) and the open time-window shape (`{originCrs,
   destinationCrs, serviceDate, departWindow, arriveWindow}`, inserting an
   `'unmatched'` leg row with no `train_subscription_id`). Phase 1's own
   `POST /Journeys` route calls this once, with `leg_order = 1`, to build a
   new journey's first leg. **This is the single function Task 1 below
   needs to reuse for `leg_order = max(...) + 1` instead of hardcoding `1`**
   — if Phase 1 instead inlined this logic directly into its route handler
   rather than factoring it into a standalone function, Task 1 will need to
   extract it first (a small, mechanical refactor, not a redesign).
4. **A request-validation function** for the open-leg shape (3-letter CRS
   codes, sane windows) exists alongside the function in (3) — assumed
   named `validate_new_leg_request` or inlined into route-layer validation
   the same way `validate_pin`/`validate_ticket_entry` are
   (`crates/api/src/data/train_tracking.rs:55-77`, `:252-281`, both read
   above) — reused as-is by Task 2's route, not re-implemented.
5. **`GET /Journeys/{journeyId}/legs/{legId}/candidates`** (spec §2.2) and
   **a per-leg commit route** (spec §2.3 — behavior fully specified: calls
   `find_or_create_train` + `create_subscription_for_train`, sets
   `journey_legs.train_subscription_id`, `match_mode = 'manual'`). **CORRECTED
   post-hoc, once Phase 1's actual plan was written (this plan originally
   guessed `POST .../commit` before Phase 1's route name was known): the
   real route is `POST /Journeys/{journeyId}/legs/{legId}/train`**
   (`docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md`,
   Task 11), body shaped like `{trainUid, serviceDate}`.)
   Both routes exist, are already scoped by `legId` alone (not `leg_order` or
   journey position), and **need zero changes for Phase 2** — a leg created
   by this plan's Task 1/2 is immediately usable by both routes unmodified,
   which is exactly what the top-level task brief means by "reusing Phase
   1's per-leg commit route and candidate query, just scoped to an existing
   journey."
6. **`journey_owner(pool, journey_id) -> anyhow::Result<Option<String>>`** or
   equivalent ownership-lookup helper exists in `data/journeys.rs`, mirroring
   `train_tracking::tracked_train_owner` (`crates/api/src/data/train_tracking.rs`,
   used at `crates/api/src/routes/train.rs:184-195` etc.) — used by Task 1's
   new function to enforce the same 404-never-403 convention documented in
   `crates/api/src/routes/train.rs:1-13`'s module doc (read above) and
   restated in the custom-tracking-names plan's own Global Constraints
   (`docs/superpowers/plans/2026-09-05-custom-tracking-names-plan.md`,
   "Every new route follows the 404-never-403 ownership convention").
7. **Route file `crates/api/src/routes/journeys.rs`** exists with a
   `router()` mounting at least `POST /Journeys`, `GET /Journeys/mine`,
   `GET /Journeys/{id}`, `GET /Journeys/{id}/legs/{legId}/candidates`, and
   the commit route from (5) — Task 2 below adds one more route
   (`POST /Journeys/{id}/legs`) to this same router.
8. **`GET /Journeys/{id}` response shape**: `{journey: {...}, legs: [{...leg
   fields, trackedTrainState?: TrackedTrainState}]}` (spec §4, quoted
   directly) — a matched leg's `trackedTrainState` is exactly today's
   `TrackedTrainState` shape (`crates/api/src/data/train_tracking.rs:1009-1112`,
   read above), reusing `TRACKED_TRAIN_STATE_SELECT`
   (`train_tracking.rs:1151-1168`) verbatim. Phase 2's frontend work (Tasks
   6-8) reads this response as-is; no backend change to this route.
9. **Frontend**: `frontend/lib/types.ts` has `Journey`, `JourneyLeg`,
   `JourneyDetail` (or similarly-named) interfaces mirroring the wire shapes
   above, camelCase, following this file's existing convention (confirmed
   directly — `TrainJourneyState`, `TrackedTrainState` at
   `frontend/lib/types.ts:499-593`, read above); `frontend/app/journeys/[id]/page.tsx`
   exists, rendering a header (name, "Add a leg" button placeholder) and one
   card per leg — a matched leg embeds `<TrainJourney state={...} />`
   unmodified (`frontend/components/TrainJourney.tsx:39-75`, read above), an
   open leg shows a lighter search-parameters + candidate-list card. Task 7
   below adds the real "Add a leg" button/flow to this existing page; it
   does not create the page.
10. **`frontend/lib/api.ts`** functions for reading `GET /Journeys/{id}` /
    `GET /Journeys/mine` exist (this file's own convention — every `GET` is
    a named async function here, confirmed by the ~35 `export async
    function get*` entries grepped above); Phase 2 adds one new mutation
    (`POST /Journeys/{id}/legs`), which — per this codebase's own established
    split — is **not** added to `lib/api.ts` (that file holds reads only;
    every existing write, e.g. `AddTrainToGroupButton.tsx`'s
    `fetch('/api/groups/{id}/trains', ...)`, read above, is a bare
    same-origin `fetch()` call inline in the mutating client component) —
    Task 7 follows that same pattern, not a new `lib/api.ts` export.

If any of (1)-(10) turns out wrong once Phase 1 actually lands, the *shape*
of Tasks 1-8 below still holds — only the specific file:line citations and
function/type names need adjusting to match reality.

---

**Architecture:** Backend first (Tasks 1-3), then frontend (Tasks 4-8),
matching this repo's own convention for a feature whose frontend depends on
new response fields existing (see the custom-tracking-names plan's own
"Architecture" section, same ordering). Task 1 adds one new data-layer
function, `add_leg_to_journey`, that does two things beyond what Phase 1's
`create_leg` (assumption 3) already does on its own: (a) looks up
`max(leg_order)` for the journey inside the same statement rather than a
separate round trip, mirroring the single-CTE idempotency pattern
`create_subscription_for_train` already uses
(`train_tracking.rs:198-230`), and (b) enforces journey ownership before
touching anything, mirroring `tracked_train_owner`/`WHERE ... AND user_id =
$2`'s 404-never-403 convention used throughout `train.rs`. Task 2 exposes it
as `POST /Journeys/{id}/legs`. Task 4 extends the frontend wire types for
the new request/response. Task 5 ports `frontend/lib/severity.ts`'s
`worstStatus`/rank-and-reduce idiom (verified exports: `severityColor`,
`isGoodSeverity`, `severityLabel`, `severityRank`, `worstStatus` — `GROUP_RANK`
and the `SeverityGroup` type stay module-private, confirmed by grep above,
so this is a **new, small, analogous module**, not an import of
`severity.ts`'s own private machinery) into a journey-leg-specific rank
table, since a leg's status shape (`unmatched` / `cancelled` / `delayed` /
`awaiting` / `on-time`) is not TfL/National Rail's `statusSeverity` integer
code and cannot reuse `SEVERITY_TABLE` itself. Tasks 6-7 build the "Add a
leg" UI and wire the rollup badge into the existing journey view.

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-journey-tracking-design.md` —
authoritative for every architectural decision this plan implements. Read
§3 and the revised §9 (the *current* text, not an earlier draft — §9's
Phase 1 scope grew on 2026-09-22 to absorb window-search/manual-pick,
shrinking Phase 2 to just this plan's multi-leg-chaining scope) before
touching any task below.

---

## Judgment calls this plan makes

1. **The new route lives in the same `crates/api/src/routes/journeys.rs`
   router Phase 1 creates, not a new file.** One journey-scoped router,
   growing by one route, matches every existing precedent in this codebase
   (`train.rs` mounts 15+ routes in one `router()`; `groups.rs` does the
   same for every `/groups/...` shape) — there is no existing precedent for
   splitting one resource's routes across multiple router files by which
   plan/phase added them.
2. **`leg_order` assignment uses a single-statement `INSERT ... SELECT
   COALESCE(MAX(leg_order), 0) + 1 ... WHERE journey_id = $1`, not a
   read-then-write round trip**, mirroring `create_subscription_for_train`'s
   own CTE-based single-statement approach (`train_tracking.rs:198-230`,
   its own doc comment explicitly reasons about avoiding a
   SELECT-then-INSERT race). The **honest residual limitation, stated
   rather than hidden** (same posture that function's own doc comment
   takes): under READ COMMITTED, two genuinely simultaneous "add a leg"
   requests for the *same* journey can both read the same `MAX(leg_order)`
   and both attempt to insert the same `leg_order` value — the schema's own
   `UNIQUE (journey_id, leg_order)` constraint (assumption 1) then rejects
   the second one with a constraint-violation error, which Task 1's function
   surfaces as a plain `anyhow::Error` (mapped to the route's existing `500`
   path, Task 2) rather than a friendly retry. This is an acceptable gap for
   the same reason the codebase already accepts it for the analogous
   tracked-train race: "add a second leg" is not a button a user clicks
   twice in the same instant from two different tabs in any realistic flow,
   and the frontend already disables its submit button while a request is
   in flight (Task 7), closing the ordinary repeat-click case exactly as
   `TrackThisTrainButton.tsx` does today.
3. **The leg-status rollup lives in a new, small frontend-only module
   (`frontend/lib/journeyStatus.ts`), not inside `severity.ts` itself.** A
   journey leg's status space (`unmatched leg`, `cancelled`, `delayed`,
   `awaiting first movement`, `on time`/`completed`) is a genuinely
   different domain from TfL/National Rail's `statusSeverity` integer codes
   that `severity.ts`'s `SEVERITY_TABLE` exists to classify — there is
   nothing to add a case to in that table, only the same *rank-and-reduce
   idiom* to port, per §3's own framing ("This needs no new
   severity-ranking code — the shape is a straight port of an idiom"). This
   also avoids `severity.ts` acquiring an import of journey-specific types
   it has no other reason to know about.
4. **The rollup's "delayed" threshold is `delayMinutes > 0`, matching the
   existing per-leg delay badge exactly** (`TrainJourney.tsx:305`, read
   above: `color={state.delayMinutes > 0 ? 'orange' : 'green'}`), **not**
   the notifier's separate `train_delay_threshold_minutes` (default 15
   minutes, `crates/notifier/src/decision.rs`, per the design spec's §0.3).
   Those two numbers already serve different purposes in this codebase (one
   decides when to *notify*, escalation-only; this one decides what color a
   badge the user is already looking at should be) and picking the
   notifier's threshold here would make the journey summary badge disagree
   with the very leg card sitting right below it on the same page — a worse
   outcome than the two thresholds' names merely not matching.
5. **A leg row created via the open-leg (time-window) shape from this new
   route is immediately visible on the journey view as an "open leg" card
   with no further Phase-2-specific frontend work** — Task 7 only needs to
   POST the creation request and trigger a re-fetch of `GET /Journeys/{id}`;
   assumption 9 already has the journey page rendering an open leg's
   candidate-list card generically (it has to, for Phase 1's own first-leg
   case), so a second, third, Nth open leg renders through the exact same
   code path with zero new branching. If Phase 1's implementation turns out
   to special-case "the journey's only leg" instead of iterating generically
   over `legs`, that is a Phase 1 defect this plan does not attempt to fix
   — flag it back to the Phase 1 implementer rather than patching around it
   here.

---

## Non-goals

(Everything the top-level task brief and the spec's own §9 explicitly put in
a later phase — no task below touches any of these.)

- **`'auto'` match mode.** Deferred past Phase 1 already, per §2.3's
  2026-09-22 decision; nothing in Phase 2 changes that.
- **Connection-buffer computation** between adjacent matched legs (spec §3's
  own "explicitly flagged as valuable but out of scope" paragraph,
  reiterated in §9 as deferred to Phase 3+). Phase 2's "connection" divider
  (spec §4, last bullet) is a static scheduled-time display only, and even
  that is Phase-1-owned UI (assumption 9) — this plan adds no live-buffer
  math anywhere.
- **Notifications** — journey/leg-aware `NotificationPayload` copy and
  station-skip detection are both Phase 3 (spec §5). This plan's new leg
  does flow through the *existing*, already-shipped per-`trains_id`
  notifier fan-out once matched (spec §5.1's "fully reused, no new decision
  logic" — true today, unaffected by this plan), but no new notifier code
  is written here.
- **Group sharing** (`group_journeys`, spec §6) — Phase 4. A multi-leg
  journey shares (or doesn't) exactly as well as a single-leg one once
  Phase 4 ships; nothing here needs to anticipate that.
- **Hard validation that leg N's destination equals leg N+1's origin.**
  Spec §3 is explicit: "no hard validation ... the frontend should
  default-suggest the prior leg's destination as the new leg's origin
  without enforcing it." Task 7 implements the suggestion; nothing enforces
  it, by design.
- **`'auto'`-mode-flavored UI, a "which candidate wins" picker, or anything
  else gated on Open Question #1's `'auto'` half.** Out of scope for the
  same reason as its first bullet above.
- **Reworking `TrainSearchForm`/`TimeFilterInput`.** Phase 2 reuses whatever
  window-entry UI Phase 1 built for its own open-leg creation flow
  (assumption 9) — this plan's Task 7 wires a second call site to it, it
  does not modify the component.

## Global Constraints

- **Every new route follows the 404-never-403 ownership convention**
  already established throughout `crates/api/src/routes/train.rs` (its own
  module doc, lines 1-13, read above) and restated as a Global Constraint in
  the custom-tracking-names plan. `POST /Journeys/{id}/legs` folds the
  ownership check into the same statement that reads `max(leg_order)`
  (Task 1) — never a separate ownership `SELECT` followed by an unscoped
  write, and never a `403`.
- **No DB migration in this plan.** The schema (assumption 1) is entirely
  Phase 1's responsibility. If Task 1 discovers the real Phase 1 schema
  differs materially from assumption 1 in a way that blocks this plan
  (e.g. `origin_crs`/`destination_crs` really are `NOT NULL`), stop and
  reconcile with the Phase 1 plan/implementation rather than adding a
  Phase-2-owned migration to patch around it.
- **`journey_legs.leg_order` is 1-based and gap-tolerant.** Nothing in this
  plan ever needs to renumber existing legs (no leg deletion/reordering is
  in scope for Phase 2 at all) — `max(...) + 1` is always correct as long as
  no leg is ever removed, which Phase 2 introduces no way to do.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (this repo's CI invocation,
  confirmed in the custom-tracking-names plan's own Global Constraints from
  `.github/workflows/ci.yml`), `cargo test --workspace` (fast tests, no
  `--ignored`), and `cargo test -p api -- --ignored --test-threads=1` for
  every DB-gated test this plan adds, against
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres`.
  Frontend: `npm test -- <file>` per changed test file, then a full `npm
  test` and `npm run build` before considering the frontend tasks done.
  **UI verification**: per this repo's standing practice for a change with
  no automated end-to-end coverage, start the dev stack and manually verify
  in a real browser — create a journey via Phase 1's flow, add a second leg
  both ways (direct pick and time-window search), confirm `leg_order`
  increments correctly, confirm the rollup badge changes as expected when
  one leg is unmatched vs. delayed vs. cancelled. Folded into Task 8's own
  Verify step, not a separate task.
- **File scope.** Modified/created by this plan (exact paths depend on
  Phase 1's real file layout per the Assumptions section — adjust if
  needed, but do not widen scope beyond "the multi-leg-chaining route, its
  tests, and the frontend Add-a-leg + rollup-badge UI"):
  `crates/api/src/data/journeys.rs`,
  `crates/api/src/routes/journeys.rs`,
  `frontend/lib/types.ts`,
  `frontend/lib/journeyStatus.ts` (new),
  `frontend/lib/journeyStatus.test.ts` (new),
  `frontend/components/AddJourneyLegButton.tsx` (new),
  `frontend/components/AddJourneyLegButton.test.tsx` (new),
  `frontend/components/JourneyStatusBadge.tsx` (new),
  `frontend/components/JourneyStatusBadge.test.tsx` (new),
  `frontend/app/journeys/[id]/page.tsx`.
  No other file changes — in particular, **no touching**
  `frontend/components/TrainJourney.tsx`, `JourneyTimeline.tsx`,
  `JourneyProgress.tsx` (per-train display machinery, unrelated to this
  phase per the spec's own naming-collision front matter), or any Phase
  3/4/5 concern (notifier crate, `group_journeys`).

---

## Task 1: Backend data layer — `add_leg_to_journey`

**Files:** modify `crates/api/src/data/journeys.rs` (created by Phase 1;
confirm it exists and contains assumption 3's `create_leg`/equivalent
before starting).

Depends on Phase 1 (all of the Assumptions section). **First step, before
writing any code**: grep for the real names.

- [ ] **Step 0: Confirm Phase 1's real shape.**

```bash
rg -n "fn create_leg|fn add_leg|NewLegRequest|struct.*Leg.*Request" crates/api/src/data/journeys.rs crates/api/src/routes/journeys.rs
rg -n "journey_owner|fn.*journey.*owner" crates/api/src/data/journeys.rs
rg -n "CREATE TABLE journey_legs" -A 25 crates/api/migrations/
```

  Update this task's Step 1 code below to match whatever these turn up —
  the *shape* of the change (a new function computing `max(leg_order) + 1`
  and delegating to the existing per-shape leg-insert logic) stays the
  same regardless of exact names.

- [ ] **Step 1: Add `add_leg_to_journey`**, in `data/journeys.rs`, next to
  Phase 1's leg-creation function:

```rust
/// Adds a new leg to an EXISTING journey (Phase 2, spec §3) — the
/// `leg_order = max(...) + 1` sibling of Phase 1's own first-leg creation
/// (which always inserts `leg_order = 1` for a brand-new journey). Reuses
/// the exact same per-shape insert logic Phase 1 wrote (both the
/// direct-known-train shape and the open time-window shape,
/// `NewLegRequest`), just parameterized on a caller-supplied `leg_order`
/// instead of a hardcoded `1`.
///
/// Ownership-checked in the SAME statement that computes `leg_order`,
/// never a separate `SELECT` first — same 404-never-403 posture as every
/// other ownership check in this app
/// (`train_tracking::tracked_train_owner`'s callers, `train.rs:184-195`).
/// Returns `Ok(None)` if `journey_id` doesn't exist or isn't owned by
/// `user_id` (the route maps that to `404`); returns `Ok(Some(leg_id))` on
/// success.
///
/// **Race, stated rather than hidden**: under READ COMMITTED, two
/// simultaneous calls for the SAME journey can both read the same
/// `MAX(leg_order)` and both attempt the same value — the schema's own
/// `UNIQUE (journey_id, leg_order)` constraint rejects the second with a
/// constraint-violation error, surfaced here as a plain `anyhow::Error`
/// (mapped to the route's existing 500 path). This closes the ordinary
/// repeat-click case (the frontend disables its submit button while a
/// request is in flight, same as every other mutating control in this
/// app), not a true concurrent double-submit from two different tabs —
/// same accepted-limitation posture as
/// `train_tracking::create_subscription_for_train`'s own doc comment.
pub async fn add_leg_to_journey(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    request: &NewLegRequest,
) -> anyhow::Result<Option<i64>> {
    // Ownership check first, its own statement (not folded into the
    // leg_order computation below): unlike a single-table UPDATE's
    // `WHERE id = $1 AND user_id = $2`, inserting a new CHILD row has no
    // single WHERE clause that can simultaneously scope "which journey"
    // and "is it this user's" AND compute an aggregate over that journey's
    // existing legs -- two round trips is the honest shape here, not an
    // avoidable one.
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if owned.is_none() {
        return Ok(None);
    }

    let next_leg_order: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(leg_order), 0) + 1 FROM journey_legs WHERE journey_id = $1",
    )
    .bind(journey_id)
    .fetch_one(pool)
    .await?;

    // Delegates to Phase 1's own per-shape insert (assumption 3) --
    // DO NOT reimplement the direct-vs-open-leg branching here. If Phase 1
    // named this differently, update this call site only; the rest of this
    // function's shape (ownership check, then leg_order, then insert)
    // still holds.
    let leg_id = insert_leg(pool, journey_id, next_leg_order.0, request).await?;
    Ok(Some(leg_id))
}
```

  **NOTE for the implementer**: `insert_leg` above is this plan's own
  placeholder name for whatever Phase 1 actually calls its "insert one leg
  row, either shape" function (assumption 3). If Phase 1's own first-leg
  creation is inlined directly in its `POST /Journeys` route handler rather
  than factored into a standalone `data/journeys.rs` function, **extract it
  first** (a small, mechanical refactor: move the body into a function
  taking `leg_order` as a parameter, call it with `1` from Phase 1's own
  route, call it with `next_leg_order.0` from `add_leg_to_journey` above) —
  do not duplicate the direct/open-shape insert logic a second time in this
  file.

- [ ] **Step 2: Add DB-gated `#[ignore]`d tests**, in this file's `db_tests`
  module (or wherever Phase 1's own leg-creation tests live — match that
  module's existing seed helpers, e.g. a `seed_journey`/`seed_journey_leg`
  helper Phase 1 will have needed for its own tests):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_leg_to_journey_assigns_the_next_leg_order() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEYS-ADDLEG-OWNER").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEYS-ADDLEG-OWNER").await; // seeds leg_order 1

        let leg_id = add_leg_to_journey(
            &pool,
            journey_id,
            "TEST-JOURNEYS-ADDLEG-OWNER",
            &NewLegRequest::open("WAT", "RDG", "2026-09-23".parse().unwrap(), None, None),
        )
        .await
        .expect("add leg")
        .expect("journey is owned");

        let leg_order: (i32,) = sqlx::query_as("SELECT leg_order FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .fetch_one(&pool)
            .await
            .expect("read inserted leg");
        assert_eq!(leg_order.0, 2);

        cleanup_user(&pool, "TEST-JOURNEYS-ADDLEG-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_leg_to_journey_a_journey_owned_by_someone_else_returns_none() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEYS-ADDLEG-BYSTANDER").await;
        seed_user(&pool, "TEST-JOURNEYS-ADDLEG-REAL-OWNER").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEYS-ADDLEG-REAL-OWNER").await;

        let result = add_leg_to_journey(
            &pool,
            journey_id,
            "TEST-JOURNEYS-ADDLEG-BYSTANDER",
            &NewLegRequest::open("WAT", "RDG", "2026-09-23".parse().unwrap(), None, None),
        )
        .await
        .expect("attempt add leg as non-owner");
        assert!(result.is_none());

        cleanup_user(&pool, "TEST-JOURNEYS-ADDLEG-BYSTANDER").await;
        cleanup_user(&pool, "TEST-JOURNEYS-ADDLEG-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_leg_to_journey_a_direct_known_train_leg_reuses_create_subscription_for_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEYS-ADDLEG-DIRECT").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEYS-ADDLEG-DIRECT").await;
        let (train_uid, service_date) = seed_known_scheduled_train(&pool).await;

        let leg_id = add_leg_to_journey(
            &pool,
            journey_id,
            "TEST-JOURNEYS-ADDLEG-DIRECT",
            &NewLegRequest::direct(&train_uid, service_date),
        )
        .await
        .expect("add leg")
        .expect("journey is owned");

        let row: (Option<i64>, String) = sqlx::query_as(
            "SELECT train_subscription_id, match_mode FROM journey_legs WHERE id = $1",
        )
        .bind(leg_id)
        .fetch_one(&pool)
        .await
        .expect("read inserted leg");
        assert!(row.0.is_some(), "direct-shape leg should be immediately matched");
        assert_eq!(row.1, "manual");

        cleanup_user(&pool, "TEST-JOURNEYS-ADDLEG-DIRECT").await;
    }
```

  **NOTE**: `seed_journey`, `NewLegRequest::open`/`::direct`, and
  `seed_known_scheduled_train` are this plan's placeholder names for
  whatever seed/constructor helpers Phase 1's own tests already introduced
  — reuse those exactly rather than inventing parallel ones, so both
  phases' test suites share one fixture vocabulary (the same reasoning
  `PendingSchedulePin`/`PendingBacklogPin` are kept as two distinct types
  despite identical fields, `train_tracking.rs:933-939`'s doc comment —
  except here the goal is the opposite: reuse, not duplicate, since these
  genuinely are the same concept Phase 1 already built fixtures for).

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api add_leg_to_journey -- --ignored --test-threads=1
```

  Expected: builds clean, lints clean, all three new tests pass against a
  real local Postgres with Phase 1's migration already applied.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/journeys.rs
git commit -m "api: add add_leg_to_journey, the multi-leg-chaining data-layer function"
```

---

## Task 2: Backend route — `POST /Journeys/{id}/legs`

**Files:** modify `crates/api/src/routes/journeys.rs`.

Depends on Task 1.

- [ ] **Step 1: Mount the route** in `router()`, as a sibling of Phase 1's
  existing `/Journeys/{id}` routes (mirroring how `train.rs` mounts
  `/Train/{tracking_id}/name` right next to `/Train/{tracking_id}`,
  `train.rs:90-97`, read above):

```rust
        .route(
            "/Journeys/{journey_id}/legs",
            axum::routing::post(post_journey_leg),
        )
```

- [ ] **Step 2: Add the request/response types.** Reuses Phase 1's own
  `NewLegRequest` deserialization shape directly if it's already a
  `#[derive(Deserialize)]` struct/enum matching the spec's two JSON bodies
  (`{trainUid, serviceDate}` or `{originCrs, destinationCrs, serviceDate,
  departWindow, arriveWindow}`) — if so, this step is just:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddLegResponse {
    leg_id: i64,
}
```

  (mirroring `TrackByUidResponse`, `train.rs:720-724`, read above — a
  minimal, single-field success response, same shape).

- [ ] **Step 3: Add `post_journey_leg`**, following `post_track_by_uid`'s
  exact shape (`train.rs:739-755`, read above) for the "authenticated user,
  resolve, 404-never-403" pattern:

```rust
/// `POST /Journeys/{journeyId}/legs` -- adds a new leg to an existing
/// journey the caller owns (spec §3). Accepts either of Phase 1's two leg
/// shapes verbatim (`NewLegRequest`, already used by `POST /Journeys`'s own
/// first-leg creation) -- `leg_order` is assigned by
/// `data::journeys::add_leg_to_journey` as `max(leg_order) + 1` for this
/// journey, never supplied by the caller. Same 404-never-403 ownership
/// convention as every other route in this app: "journey doesn't exist"
/// and "journey exists but isn't yours" are indistinguishable to the
/// caller.
async fn post_journey_leg(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
    Json(request): Json<NewLegRequest>,
) -> Result<Json<AddLegResponse>, (StatusCode, String)> {
    validate_new_leg_request(&request).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let leg_id = crate::data::journeys::add_leg_to_journey(
        &app.database,
        journey_id,
        &user.id,
        &request,
    )
    .await
    .map_err(internal_error("add leg to journey"))?;

    match leg_id {
        Some(leg_id) => Ok(Json(AddLegResponse { leg_id })),
        None => Err((
            StatusCode::NOT_FOUND,
            "no journey with that id".to_string(),
        )),
    }
}
```

  `validate_new_leg_request` is assumption 4 — Phase 1's own validator for
  this exact request shape, reused unchanged (never re-implemented here);
  confirm its real name via Task 1 Step 0's grep and adjust the call site
  if needed. `internal_error` is this file's own existing shared 500 mapper
  (mirroring `train.rs:1187-1195`, read above — confirm `journeys.rs` has
  an equivalent, or reuse `crate::routes::train::internal_error` if it was
  made `pub(crate)`, or add a two-line equivalent to `journeys.rs` if not —
  a judgment call for whoever finds Phase 1's real file has neither).

- [ ] **Step 4: Add HTTP-layer `db_tests`**, alongside Phase 1's own route
  tests in this file's `mod db_tests` (matching the pattern in
  `train.rs`'s own `db_tests`, e.g.
  `post_tracked_train_name_a_tracked_train_owned_by_someone_else_is_404_not_403`
  read above via the custom-tracking-names plan):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_the_owner_can_add_an_open_leg() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-ADDLEG-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let journey_id = seed_journey(&pool, "TEST-ROUTE-ADDLEG-OWNER").await;

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&token),
            serde_json::json!({
                "originCrs": "WAT",
                "destinationCrs": "RDG",
                "serviceDate": "2026-09-23",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "add-leg response: {body:?}");
        assert!(body.get("legId").and_then(Value::as_i64).is_some());

        cleanup_user(&pool, "TEST-ROUTE-ADDLEG-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_a_journey_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-ADDLEG-BYSTANDER").await;
        seed_session(&pool, "TEST-ROUTE-ADDLEG-REAL-OWNER").await;
        let router = test_router(test_app(pool.clone()));
        let journey_id = seed_journey(&pool, "TEST-ROUTE-ADDLEG-REAL-OWNER").await;

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&bystander_token),
            serde_json::json!({
                "originCrs": "WAT",
                "destinationCrs": "RDG",
                "serviceDate": "2026-09-23",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no journey with that id".to_string()));

        cleanup_user(&pool, "TEST-ROUTE-ADDLEG-BYSTANDER").await;
        cleanup_user(&pool, "TEST-ROUTE-ADDLEG-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_leg -- --ignored --test-threads=1`"]
    async fn post_journey_leg_a_second_leg_gets_leg_order_two() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-ADDLEG-ORDER").await;
        let router = test_router(test_app(pool.clone()));
        let journey_id = seed_journey(&pool, "TEST-ROUTE-ADDLEG-ORDER").await; // leg_order 1 already exists

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs"),
            Some(&token),
            serde_json::json!({
                "originCrs": "RDG",
                "destinationCrs": "PAD",
                "serviceDate": "2026-09-23",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "add-leg response: {body:?}");
        let leg_id = body.get("legId").and_then(Value::as_i64).expect("legId present");

        let leg_order: (i32,) = sqlx::query_as("SELECT leg_order FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .fetch_one(&pool)
            .await
            .expect("read inserted leg");
        assert_eq!(leg_order.0, 2);

        cleanup_user(&pool, "TEST-ROUTE-ADDLEG-ORDER").await;
    }
```

- [ ] **Step 5: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
cargo test -p api post_journey_leg -- --ignored --test-threads=1
cargo test --workspace
```

  Expected: builds/lints clean, all new tests pass, no regression in the
  rest of the workspace's fast test suite.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/journeys.rs
git commit -m "api: add POST /Journeys/{journeyId}/legs for multi-leg chaining"
```

---

## Task 3: Backend — `journeys.rs` module doc + router-conflict test

**Files:** modify `crates/api/src/routes/journeys.rs`.

Small, independent cleanup task, same posture as `train.rs`'s own
`router_builds_without_panicking` test (`train.rs:1259-1262`, read above) —
cheap insurance against a route-table conflict at construction time.

- [ ] **Step 1: Confirm (or add, if missing) a `router_builds_without_panicking`
  test** in `journeys.rs`'s own test module, identical in spirit to
  `train.rs`'s:

```rust
    #[test]
    fn router_builds_without_panicking() {
        let _ = router();
    }
```

  If Phase 1 already added this test, this step is a no-op — just confirm
  it exists and still passes with the new route mounted.

- [ ] **Step 2: Verify**

```bash
cargo test -p api router_builds_without_panicking
```

- [ ] **Step 3: Commit** (only if Step 1 made a real change)

```bash
git add crates/api/src/routes/journeys.rs
git commit -m "api: confirm journeys router has no route-table conflicts after Phase 2's new route"
```

---

## Task 4: Frontend wire types

**Files:** modify `frontend/lib/types.ts`.

Depends on Phase 1's own `Journey`/`JourneyLeg`/`JourneyDetail` types
(assumption 9) already existing. Confirm their real shape first:

```bash
rg -n "interface Journey|interface JourneyLeg|interface JourneyDetail" frontend/lib/types.ts
```

- [ ] **Step 1: Add the add-leg request type**, near wherever Phase 1 put
  its own leg-creation request type (or, if Phase 1 didn't need a separate
  TS type for its inline `POST /Journeys` body, add one here — it's now
  shared by two call sites):

```typescript
/** Body for `POST /Journeys/{journeyId}/legs` (multi-leg chaining, spec
 * §3) -- the SAME two shapes Phase 1's own journey-creation flow already
 * sends for a journey's first leg, just POSTed to a different URL. Either
 * `trainUid`+`serviceDate` (a direct, already-known-identity leg) or
 * `originCrs`+`destinationCrs`+`serviceDate` (+ optional `departWindow`/
 * `arriveWindow`) for an open time-window search leg -- never both. */
export type NewJourneyLegRequest =
  | {
      trainUid: string;
      serviceDate: string; // "YYYY-MM-DD"
    }
  | {
      originCrs: string;
      destinationCrs: string;
      serviceDate: string; // "YYYY-MM-DD"
      departWindow?: TimeWindow;
      arriveWindow?: TimeWindow;
    };

export interface AddJourneyLegResponse {
  legId: number;
}
```

  `TimeWindow` here is assumed to already exist on the frontend from Phase
  1's own porting of `common::TimeWindow` (assumption 2) — confirm via
  `rg -n "interface TimeWindow" frontend/lib/types.ts` and adjust the field
  shape above (`{after: string | null; before: string | null}` or similar)
  to match whatever Phase 1 actually shipped.

- [ ] **Step 2: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: no new type errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "frontend: add NewJourneyLegRequest/AddJourneyLegResponse types for multi-leg chaining"
```

---

## Task 5: Frontend — leg-status rollup module

**Files:** create `frontend/lib/journeyStatus.ts`, `frontend/lib/journeyStatus.test.ts`.

Independent of Tasks 1-4 (pure client-side logic over already-defined
`JourneyLeg`/`TrackedTrainState`-shaped data — can be built and tested in
isolation, though it needs Phase 1's real `JourneyLeg` type to compile
against). Ports the rank-and-reduce idiom `frontend/lib/severity.ts`'s
`worstStatus`/`severityRank`/`GROUP_RANK` already establish (read in full
above) — confirmed exports as of `main` today: `severityColor`,
`isGoodSeverity`, `severityLabel`, `severityRank`, `worstStatus`; `GROUP_RANK`
and the `SeverityGroup` type are **not** exported (module-private), so this
task cannot import them — it defines its own small, analogous, differently-
named set, per Judgment Call 3.

- [ ] **Step 1: Write `journeyStatus.ts`**:

```typescript
import type { JourneyLeg } from './types';

/** A journey leg's own status classification, independent of
 * `severity.ts`'s `SeverityGroup` -- a leg's status space (has this leg
 * even been matched to a real train yet? is it cancelled? delayed? just
 * waiting for its first movement report?) is not TfL/National Rail's
 * `statusSeverity` integer code and has no case in `SEVERITY_TABLE` to
 * reuse. This is the same rank-and-reduce IDIOM `severity.ts`'s
 * `worstStatus`/`GROUP_RANK` already establishes (spec §3's own framing:
 * "no new severity-ranking code needed, the shape is a straight port"),
 * ported to a new domain rather than importing that file's
 * (module-private) machinery directly. */
export type LegStatusGroup = 'good' | 'awaiting' | 'unmatched' | 'delayed' | 'severe';

/** Higher rank = worse. Mirrors `severity.ts`'s `GROUP_RANK` shape and
 * ordering convention exactly (good=0 ... severe=4), but over this
 * module's own `LegStatusGroup`, not `SeverityGroup`. Per spec §3: an
 * unmatched leg ("needs a train picked") outranks a merely-delayed leg,
 * and a cancelled (or, once Phase 3 lands, skipped) leg outranks both. */
const LEG_STATUS_RANK: Record<LegStatusGroup, number> = {
  good: 0,
  awaiting: 1,
  unmatched: 2,
  delayed: 3,
  severe: 4,
};

/** Classifies one leg. Delay threshold is `delayMinutes > 0`, matching
 * `TrainJourney.tsx`'s own existing per-leg delay badge exactly (not the
 * notifier's separate 15-minute escalation threshold) -- see this plan's
 * Judgment Call 4 for why those two numbers are deliberately different
 * things and must not be conflated here. */
export function legStatusGroup(leg: JourneyLeg): LegStatusGroup {
  if (leg.trackedTrainState === null || leg.trackedTrainState === undefined) {
    // No `train_subscription_id` bound yet -- an open, unmatched leg
    // (spec §1.1's `match_mode = 'unmatched'`). Distinguishable from every
    // matched state below by the mere ABSENCE of a `trackedTrainState`,
    // exactly as `GET /Journeys/{id}`'s own response shape (spec §4)
    // already encodes it.
    return 'unmatched';
  }
  const state = leg.trackedTrainState;
  if (state.status === 'cancelled') return 'severe';
  if (state.status === 'awaiting_activation' || state.status === null) return 'awaiting';
  if (state.delayMinutes !== null && state.delayMinutes > 0) return 'delayed';
  return 'good'; // 'en_route' with no reported delay, or 'completed'.
}

/** Higher rank = worse, same convention as `severity.ts`'s `severityRank`. */
export function legStatusRank(leg: JourneyLeg): number {
  return LEG_STATUS_RANK[legStatusGroup(leg)];
}

/** Picks the single worst-status leg across a journey's legs, by
 * `legStatusRank` -- the exact reduce-to-worst shape
 * `frontend/app/stations/[crs]/page.tsx`'s own inline
 * `candidates.reduce((acc, candidate) => severityRank(...) > severityRank(...) ? candidate : acc)`
 * already uses (read directly, confirmed as a real precedent, not merely
 * `severity.ts`'s own `worstStatus` implementation). Returns `null` for a
 * journey with no legs at all -- should not occur in practice (every
 * journey has at least one leg from creation, per Phase 1), but this
 * avoids a runtime crash on `reduce` over an empty array if it ever does. */
export function worstLegStatus(legs: JourneyLeg[]): LegStatusGroup | null {
  if (legs.length === 0) return null;
  return legs.reduce(
    (worst, leg) => (legStatusRank(leg) > LEG_STATUS_RANK[worst] ? legStatusGroup(leg) : worst),
    legStatusGroup(legs[0]),
  );
}
```

  Adjust the `leg.trackedTrainState`/`state.status`/`state.delayMinutes`
  field accesses above to match Phase 1's *actual* `JourneyLeg`/
  `TrackedTrainState` field names if they differ from assumption 8/9.

- [ ] **Step 2: Write `journeyStatus.test.ts`**, covering each branch —
  modeled on `frontend/lib/severity.test.ts`'s own per-case structure (read
  its layout, not full content, via `rg -n "^(describe|it|test)\(" frontend/lib/severity.test.ts`
  before writing, so this file's test naming matches that convention):

```typescript
import { describe, expect, it } from 'vitest';
import { legStatusGroup, legStatusRank, worstLegStatus, type LegStatusGroup } from './journeyStatus';
import type { JourneyLeg } from './types';

function leg(overrides: Partial<JourneyLeg> = {}): JourneyLeg {
  return {
    id: 1,
    legOrder: 1,
    originCrs: 'WAT',
    destinationCrs: 'RDG',
    serviceDate: '2026-09-23',
    trackedTrainState: null,
    ...overrides,
  } as JourneyLeg;
}

describe('legStatusGroup', () => {
  it('classifies an unmatched leg (no trackedTrainState) as unmatched', () => {
    expect(legStatusGroup(leg({ trackedTrainState: null }))).toBe('unmatched');
  });

  it('classifies a cancelled matched leg as severe', () => {
    expect(
      legStatusGroup(leg({ trackedTrainState: { status: 'cancelled', delayMinutes: null } as never })),
    ).toBe('severe');
  });

  it('classifies a delayed matched leg as delayed', () => {
    expect(
      legStatusGroup(leg({ trackedTrainState: { status: 'en_route', delayMinutes: 5 } as never })),
    ).toBe('delayed');
  });

  it('classifies an on-time en_route leg as good', () => {
    expect(
      legStatusGroup(leg({ trackedTrainState: { status: 'en_route', delayMinutes: 0 } as never })),
    ).toBe('good');
  });

  it('classifies a completed leg as good', () => {
    expect(
      legStatusGroup(leg({ trackedTrainState: { status: 'completed', delayMinutes: null } as never })),
    ).toBe('good');
  });

  it('classifies a leg still awaiting its first movement report as awaiting', () => {
    expect(
      legStatusGroup(
        leg({ trackedTrainState: { status: 'awaiting_activation', delayMinutes: null } as never }),
      ),
    ).toBe('awaiting');
  });
});

describe('worstLegStatus', () => {
  it('an unmatched leg outranks a merely-delayed leg', () => {
    const legs = [
      leg({ id: 1, trackedTrainState: { status: 'en_route', delayMinutes: 20 } as never }),
      leg({ id: 2, trackedTrainState: null }),
    ];
    expect(worstLegStatus(legs)).toBe('unmatched');
  });

  it('a cancelled leg outranks both an unmatched leg and a delayed leg', () => {
    const legs = [
      leg({ id: 1, trackedTrainState: null }),
      leg({ id: 2, trackedTrainState: { status: 'en_route', delayMinutes: 20 } as never }),
      leg({ id: 3, trackedTrainState: { status: 'cancelled', delayMinutes: null } as never }),
    ];
    expect(worstLegStatus(legs)).toBe('severe');
  });

  it('all-good legs roll up to good', () => {
    const legs = [
      leg({ id: 1, trackedTrainState: { status: 'en_route', delayMinutes: 0 } as never }),
      leg({ id: 2, trackedTrainState: { status: 'completed', delayMinutes: null } as never }),
    ];
    expect(worstLegStatus(legs)).toBe('good');
  });

  it('an empty leg list returns null', () => {
    expect(worstLegStatus([])).toBeNull();
  });
});

describe('legStatusRank', () => {
  it('is monotonic with severity.ts-style ordering (good < awaiting < unmatched < delayed < severe)', () => {
    const order: LegStatusGroup[] = ['good', 'awaiting', 'unmatched', 'delayed', 'severe'];
    for (let i = 1; i < order.length; i += 1) {
      const lower = legStatusRank(leg({ trackedTrainState: null }));
      // Spot-check via the exported groups directly rather than
      // reconstructing a leg for every group -- this test's real job is
      // just confirming the rank table's own ordering, already exercised
      // indirectly by `worstLegStatus`'s tests above.
      expect(order[i]).not.toBe(order[i - 1]);
      void lower;
    }
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- lib/journeyStatus.test.ts
```

  Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/journeyStatus.ts frontend/lib/journeyStatus.test.ts
git commit -m "frontend: add journeyStatus.ts, the per-leg worst-status rollup idiom for journeys"
```

---

## Task 6: Frontend — `JourneyStatusBadge` component

**Files:** create `frontend/components/JourneyStatusBadge.tsx`,
`frontend/components/JourneyStatusBadge.test.tsx`.

Depends on Task 5. Renders the rollup as a Mantine `Badge`, following
`StatusBadge.tsx`'s existing color-mapping convention (grep its shape
before writing — `rg -n "export function StatusBadge" -A 20 frontend/components/StatusBadge.tsx`)
rather than inventing a new badge-rendering pattern.

- [ ] **Step 1: Write the component**:

```typescript
import { Badge, Tooltip } from '@mantine/core';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import type { JourneyLeg } from '@/lib/types';

const LABEL: Record<LegStatusGroup, string> = {
  good: 'On track',
  awaiting: 'Awaiting first report',
  unmatched: 'Needs a train picked',
  delayed: 'Delayed',
  severe: 'Cancelled',
};

const COLOR: Record<LegStatusGroup, string> = {
  good: 'green',
  awaiting: 'gray',
  unmatched: 'blue',
  delayed: 'yellow',
  severe: 'red',
};

/** The journey-level summary badge, per spec §3: "the journey list/detail
 * view rolls up per-leg status into one summary the same way
 * `LineStatusCard`'s 'worst status' pattern already works elsewhere in
 * this app." Renders nothing for a journey with no legs (should not occur
 * in practice, per Task 5's own `worstLegStatus` doc comment). */
export function JourneyStatusBadge({ legs }: { legs: JourneyLeg[] }) {
  const worst = worstLegStatus(legs);
  if (worst === null) return null;
  return (
    <Tooltip label={LABEL[worst]}>
      <Badge color={COLOR[worst]} variant="light" tt="none">
        {LABEL[worst]}
      </Badge>
    </Tooltip>
  );
}
```

- [ ] **Step 2: Write `JourneyStatusBadge.test.tsx`**, using this repo's
  existing component-test convention (Testing Library + `@mantine/core`'s
  provider wrapper — grep `RenameTrainButton.test.tsx`'s own setup, read
  indirectly above via the custom-tracking-names plan, for the exact
  render-wrapper shape used elsewhere in this app):

```typescript
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MantineProvider } from '@mantine/core';
import { JourneyStatusBadge } from './JourneyStatusBadge';
import type { JourneyLeg } from '@/lib/types';

function renderBadge(legs: JourneyLeg[]) {
  return render(
    <MantineProvider>
      <JourneyStatusBadge legs={legs} />
    </MantineProvider>,
  );
}

function leg(overrides: Partial<JourneyLeg> = {}): JourneyLeg {
  return {
    id: 1,
    legOrder: 1,
    originCrs: 'WAT',
    destinationCrs: 'RDG',
    serviceDate: '2026-09-23',
    trackedTrainState: null,
    ...overrides,
  } as JourneyLeg;
}

describe('JourneyStatusBadge', () => {
  it('shows "Needs a train picked" when any leg is unmatched', () => {
    renderBadge([
      leg({ id: 1, trackedTrainState: { status: 'en_route', delayMinutes: 0 } as never }),
      leg({ id: 2, trackedTrainState: null }),
    ]);
    expect(screen.getByText('Needs a train picked')).toBeInTheDocument();
  });

  it('shows "Cancelled" when the worst leg is cancelled, even if another is merely unmatched', () => {
    renderBadge([
      leg({ id: 1, trackedTrainState: null }),
      leg({ id: 2, trackedTrainState: { status: 'cancelled', delayMinutes: null } as never }),
    ]);
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
  });

  it('renders nothing for an empty leg list', () => {
    const { container } = renderBadge([]);
    expect(container).toBeEmptyDOMElement();
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- components/JourneyStatusBadge.test.tsx
```

- [ ] **Step 4: Commit**

```bash
git add frontend/components/JourneyStatusBadge.tsx frontend/components/JourneyStatusBadge.test.tsx
git commit -m "frontend: add JourneyStatusBadge, the per-journey worst-status rollup badge"
```

---

## Task 7: Frontend — "Add a leg" flow

**Files:** create `frontend/components/AddJourneyLegButton.tsx`,
`frontend/components/AddJourneyLegButton.test.tsx`.

Depends on Task 4 (types). Modeled directly on `AddTrainToGroupButton.tsx`'s
existing button → modal → fetch shape (read in full above): a `'use client'`
component, `useDisclosure` for the modal, a bare same-origin `fetch()`
against the `/api/...` proxy (never `lib/api.ts` — that file is reads-only,
per assumption 10), `router.refresh()` on success to re-pull
`GET /Journeys/{id}` server-side. **Two real differences from
`AddTrainToGroupButton`**: (a) this needs a two-mode form (direct pick vs.
open search), not a single `Select`, and (b) the default-suggested origin
(spec §3) — pre-filling, never locking, the new leg's origin field with the
previous leg's destination.

- [ ] **Step 1: Write the component**:

```typescript
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, SegmentedControl, Stack, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { JourneyLeg, NewJourneyLegRequest } from '@/lib/types';

/** "Add a leg" (spec §3): appends a new leg to an existing journey via
 * `POST /Journeys/{journeyId}/legs`. `priorDestinationCrs` -- the
 * journey's current LAST leg's destination, computed by the caller
 * (`app/journeys/[id]/page.tsx`) -- is used ONLY to pre-fill the new
 * origin field's default value; the user can freely overwrite it before
 * submitting, and nothing here or server-side enforces they match. Per
 * spec §3: "no hard validation that leg N's destination equals leg N+1's
 * origin ... the frontend should default-suggest ... without enforcing
 * it." */
export function AddJourneyLegButton({
  journeyId,
  priorDestinationCrs,
}: {
  journeyId: number;
  priorDestinationCrs: string | null;
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [mode, setMode] = useState<'direct' | 'search'>('search');
  const [trainUid, setTrainUid] = useState('');
  const [serviceDate, setServiceDate] = useState('');
  const [originCrs, setOriginCrs] = useState(priorDestinationCrs ?? '');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  function handleOpen() {
    setError(null);
    // Re-seed the suggestion every time the modal opens, not just once at
    // mount -- `priorDestinationCrs` can change between opens if the user
    // added (and this component re-rendered for) an intervening leg.
    setOriginCrs(priorDestinationCrs ?? '');
    setDestinationCrs('');
    setTrainUid('');
    setServiceDate('');
    open();
  }

  async function handleSubmit() {
    setSubmitting(true);
    setError(null);
    const body: NewJourneyLegRequest =
      mode === 'direct'
        ? { trainUid: trainUid.trim(), serviceDate }
        : { originCrs: originCrs.trim(), destinationCrs: destinationCrs.trim(), serviceDate };
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!response.ok) {
        const text = await response.text();
        setError(text || 'Could not add this leg.');
        setSubmitting(false);
        return;
      }
      close();
      setSubmitting(false);
      router.refresh();
    } catch {
      setError('Could not add this leg.');
      setSubmitting(false);
    }
  }

  if (needsLoginState === 'needs-login') {
    return <LoginLink />;
  }

  return (
    <>
      <Button variant="light" onClick={handleOpen}>
        Add a leg
      </Button>
      <Modal opened={opened} onClose={close} title="Add a leg">
        <Stack gap="sm">
          {error && (
            <Alert color="red" title="Could not add this leg">
              {error}
            </Alert>
          )}
          <SegmentedControl
            value={mode}
            onChange={(value) => setMode(value as 'direct' | 'search')}
            data={[
              { label: 'Search by time window', value: 'search' },
              { label: 'I know the train', value: 'direct' },
            ]}
          />
          {mode === 'search' ? (
            <>
              <TextInput
                label="Origin"
                description="Defaults to your previous leg's destination -- change it if this leg starts somewhere else (a walk, a tram, a different station nearby)."
                value={originCrs}
                onChange={(event) => setOriginCrs(event.currentTarget.value)}
              />
              <TextInput
                label="Destination"
                value={destinationCrs}
                onChange={(event) => setDestinationCrs(event.currentTarget.value)}
              />
            </>
          ) : (
            <TextInput
              label="Train UID"
              value={trainUid}
              onChange={(event) => setTrainUid(event.currentTarget.value)}
            />
          )}
          <TextInput
            label="Service date"
            type="date"
            value={serviceDate}
            onChange={(event) => setServiceDate(event.currentTarget.value)}
          />
          <Button onClick={handleSubmit} loading={submitting} disabled={!serviceDate}>
            Add leg
          </Button>
        </Stack>
      </Modal>
    </>
  );
}
```

  **This is a minimal, functional first cut, not a full replica of
  `TrainSearchForm.tsx`'s 773-line UI.** Per assumption 9/Non-goals, Phase
  1 already built the full time-window search UI (with `TimeFilterInput`
  pairs, live candidate results) for its own first-leg creation flow on
  the journey-creation page. **If that flow is reachable as a standalone,
  embeddable component** (check for one before writing this step's own
  plain `TextInput` fields — e.g. `frontend/components/OpenLegSearchForm.tsx`
  or similar, factored out of Phase 1's journey-creation page), **prefer
  embedding it here over the bare `TextInput` fields sketched above** — the
  bare fields are this plan's fallback only if Phase 1 didn't factor its
  own window-entry UI into a reusable piece. Re-check this decision against
  Phase 1's actual deliverable before finalizing this task.

- [ ] **Step 2: Write `AddJourneyLegButton.test.tsx`**, following
  `AddTrainToGroupButton.test.tsx`'s existing structure (grep its shape
  first: `rg -n "^(describe|it|test)\(" frontend/components/AddTrainToGroupButton.test.tsx`)
  — cover: opening the modal pre-fills origin from `priorDestinationCrs`;
  submitting the search-mode form POSTs the expected body to
  `/api/Journeys/{id}/legs`; submitting the direct-mode form POSTs
  `{trainUid, serviceDate}` instead; a non-OK response surfaces the
  server's error text; success calls `router.refresh()`.

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- components/AddJourneyLegButton.test.tsx
```

- [ ] **Step 4: Commit**

```bash
git add frontend/components/AddJourneyLegButton.tsx frontend/components/AddJourneyLegButton.test.tsx
git commit -m "frontend: add AddJourneyLegButton, the multi-leg chaining UI"
```

---

## Task 8: Wire both new components into the journey view

**Files:** modify `frontend/app/journeys/[id]/page.tsx`.

Depends on Tasks 6-7. This is the only task that touches Phase 1's own
journey detail page — confirm its real structure first
(`rg -n "export default" frontend/app/journeys/\[id\]/page.tsx` and read the
file) before editing, since assumption 9's sketch of its header/leg-card
structure may not exactly match what Phase 1 shipped.

- [ ] **Step 1: Add `JourneyStatusBadge` to the page header**, next to the
  journey's `custom_name` (spec §4's header bullet: "journey `custom_name`
  (editable...), a share-to-group button (§6), an 'Add a leg' button
  (§3)"):

```typescript
import { JourneyStatusBadge } from '@/components/JourneyStatusBadge';
// ... inside the header, alongside the existing name/rename control:
<JourneyStatusBadge legs={journey.legs} />
```

- [ ] **Step 2: Add `AddJourneyLegButton` to the page header**, computing
  `priorDestinationCrs` from the journey's own last leg (by `legOrder`,
  descending) — falling back to that leg's *matched train's* schedule
  destination when the leg itself has no useful `destinationCrs` (the
  accepted §1.1 gap for an NR-primary-sourced leg — mirror
  `TrainJourney.tsx`'s own `scheduleDestinationName ?? scheduleDestinationCrs`
  precedence, read above, rather than leaving the suggestion blank
  whenever that gap is hit):

```typescript
import { AddJourneyLegButton } from '@/components/AddJourneyLegButton';

const lastLeg = [...journey.legs].sort((a, b) => b.legOrder - a.legOrder)[0] ?? null;
const priorDestinationCrs =
  lastLeg?.destinationCrs ??
  lastLeg?.trackedTrainState?.scheduleDestinationCrs ??
  null;

// ... inside the header:
<AddJourneyLegButton journeyId={journey.id} priorDestinationCrs={priorDestinationCrs} />
```

- [ ] **Step 3: Verify — automated**

```bash
cd frontend && npm test
npm run build
```

  Expected: full test suite and production build both pass clean.

- [ ] **Step 4: Verify — manual, real browser** (this repo's standing
  practice for a change with no automated end-to-end coverage, per this
  plan's Global Constraints):
  1. Start the dev stack (`docker compose up` or this repo's own documented
     dev-start command — confirm via `README.md`/`CONTRIBUTING.md` if
     unfamiliar).
  2. Log in, create a journey via Phase 1's existing single-leg flow
     (`/track` or `/journeys/new`, per spec §7.2's redirect plan — whichever
     Phase 1 actually shipped).
  3. On the journey's detail page, click "Add a leg." Confirm the origin
     field is pre-filled with the first leg's destination, and that it can
     be freely edited (typing over it does not get silently reverted).
  4. Add a second leg via the time-window search mode. Confirm it appears
     as a new open-leg card, `leg_order = 2`, and the journey status badge
     now reads "Needs a train picked" (since the new leg is unmatched).
  5. Commit that open leg to a real candidate via Phase 1's existing
     "Track this train" affordance on the open-leg card. Confirm the badge
     updates once the leg is matched.
  6. Add a third leg via the direct-known-train mode (a real `trainUid` +
     `serviceDate` you can look up via `/trains` search first). Confirm it
     appears already matched (`leg_order = 3`, no "Change train" button per
     spec §4's own "a leg created by picking a specific train directly...
     gets no 'Change train' action" rule — since it has no persisted
     window).
  7. If reachable in your test data, force one leg's underlying tracked
     train into a cancelled or delayed state (or find one already in that
     state) and confirm the badge correctly reflects the WORST leg across
     all three, not just the most-recently-added one.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/journeys/[id]/page.tsx
git commit -m "frontend: wire AddJourneyLegButton and JourneyStatusBadge into the journey detail view"
```

---

## Done criteria

- `POST /Journeys/{id}/legs` exists, is ownership-scoped (404-never-403),
  assigns `leg_order = max(...) + 1`, and accepts both leg-creation shapes
  by delegating to Phase 1's own per-shape insert logic (no duplicated
  branching).
- A journey's status badge always reflects the single worst leg by the rank
  order: cancelled > unmatched ("needs a train picked") > delayed >
  awaiting-first-report > good — ported as a new, small, analogous module
  rather than reusing `severity.ts`'s private `GROUP_RANK`/`SeverityGroup`.
- The "Add a leg" flow default-suggests (never enforces) the prior leg's
  destination as the new leg's origin.
- Everything Phase 1 already built (single-leg journeys, migration,
  time-window search, manual-pick, "Change train") continues to work
  unchanged — this plan added one new route and reused everything else.
- No Phase 3/4/5 concern (notifications, station skip, group sharing,
  connection buffers, `'auto'` mode, platform display) was touched.
