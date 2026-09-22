# Plan: Journey Tracking Phase 1 — Single-Leg Migration + Manual-Pick Window Search

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement the **revised Phase 1** scope of
`docs/superpowers/specs/2026-09-22-journey-tracking-design.md` (§9, as
amended by the 2026-09-22 addendum to §2.3/§4/§8) end to end: the
`journeys`/`journey_legs` schema, a one-time migration that wraps every
existing `train_subscriptions` row in its own single-leg journey, a new
`POST /Journeys` that supersedes today's two direct tracking entry points
(the legacy CRS+time guess pin AND the already-known-identity path) while
*also* adding a genuinely new third path (an open time-window search with
no train picked yet), the time-window candidate query and its route, the
`'manual'`-mode commit route (reused unchanged for a leg's first pick and
any later "Change train" re-pick), `GET /Journeys/mine`/`GET /Journeys/{id}`,
and the frontend surface for all of it — `TrackTrainForm`/
`TrackThisTrainButton` rewired to create through `POST /Journeys`, and a new
`/journeys/[id]` view.

**Explicitly out of scope** (per the spec's own phasing): multi-leg
chaining (§3, Phase 2), `'auto'` match mode, journey-aware notification
logic and station-skip detection (§5, Phase 3), group sharing (§6, Phase 4).
See this plan's own Non-goals section for the complete, itemized list,
including a few Phase-1-adjacent things the spec's §4 mentions that this
plan deliberately defers (journey rename, delete, platform display).

**Architecture:** backend before frontend, migrations before code, per this
repo's usual ordering. Two migrations (Tasks 1–2): one additive schema
(`journeys`/`journey_legs`, with one correction to the spec's own §1.1
sketch — see Judgment Call 1), one data backfill. A new shared
`common::TimeWindow` type (Task 3) closes a real "no reusable before/after
type" gap the spec names (§0.5/§2.1). A new, deliberately-standalone query
function in `queries.rs` (Task 4) — a sibling of
`search_schedule_calling_point_departures`, not an edit to it — adds the
strict origin-before-destination ordering a journey leg's candidate search
needs, without touching a function other in-flight branches are actively
modifying (Judgment Call 2). A new `crates/api/src/data/journeys.rs` module
(Tasks 5–8) holds every new data-layer function: three leg-creation paths
(wrapping the *existing*, unchanged `create_pin`/
`create_subscription_for_train`, plus a genuinely new open-leg path),
ownership-scoped reads, and the single commit/match function reused for
both a first pick and a "Change train" re-pick. A new
`crates/api/src/routes/journeys.rs` (Tasks 9–12) exposes five routes,
reusing `TRACKED_TRAIN_STATE_SELECT`, `attach_journey_stops`,
`blend_darwin_eta`, and `enrich_shared_train` from `routes/train.rs`
unchanged (visibility widened, not rewritten). The frontend (Tasks 13–17)
rewires the two existing tracking entry points to the new route and adds
one new view, reusing `TrainJourney` unmodified and `TimeFilterInput`'s
existing before/after UI convention for window entry, per the spec's own
explicit direction.

**Tech stack:** Rust/axum/sqlx (`crates/api`), Next.js/React/Mantine
(`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-journey-tracking-design.md` —
authoritative for every architectural decision this plan implements (schema
shape, why `journey_legs` is a separate table, the 2026-09-22 addendum's
manual-pick + persisted-window "Change train" decision, the revised §9
Phase 1 scope). This plan does not re-litigate anything that spec already
settled; it only resolves what the spec explicitly left to "the
implementer" (§2.1's "extend or add a sibling — a judgment call for the
implementer, not this spec" being the clearest example) and a handful of
smaller things the spec's prose left implicit.

**A note on staleness.** This plan was written against `main` at commit
`5fd42b47` (2026-09-22). Two things named explicitly because they may have
shifted by the time this plan is executed:

1. `crates/api/src/data/queries.rs`'s `search_schedule_calling_point_departures`
   (~line 1414) and its `stops_at` ordering logic are named in the research
   prompt as under active, unmerged, sibling development. This plan
   deliberately does **not** touch that function at all (Judgment Call 2) —
   if those branches have since merged, re-check this plan's Task 4 still
   compiles against the *current* signature before proceeding, but the new
   sibling function itself should still apply cleanly regardless of what
   changed inside the original, since it shares no lines with it.
2. `TrainSearchForm.tsx` is named as under active, unmerged, sibling
   development. This plan's frontend tasks only *read* that file for its UI
   convention (Task 15) and do not modify it — if its exact prop/field names
   have since changed, adjust Task 15's copy-derived code to match, but
   nothing in this plan depends on `TrainSearchForm.tsx` compiling in any
   particular shape.

---

## Judgment calls this plan makes (read before Task 1)

1. **`journey_legs.origin_crs`/`.destination_crs` are `TEXT` (nullable),
   not `TEXT NOT NULL`, contradicting the spec's own §1.1 schema sketch.**
   The spec's own §7.1 flags this itself: "an NR-primary subscription whose
   `pin_origin_crs`/`pin_destination_crs` were never populated... Recommend
   `journey_legs.origin_crs`/`.destination_crs` be nullable after all." This
   plan adopts that correction as written — it is the spec's own stated
   intent, not a deviation from it — and it matters beyond the historical
   migration: Task 6's `create_journey_with_known_train_leg` (the
   `knownTrain` mode of `POST /Journeys`, replacing `TrackThisTrainButton`'s
   direct track call) hits the identical gap for a **freshly-created** leg
   whenever the newly-tracked train's shared `trains` row has no schedule
   data yet — exactly the same accepted gap `TrackedTrainState::pin_origin_crs`
   already documents. A `NOT NULL` column would make that leg-creation path
   fail outright on its very first real use, not just on historical rows.

2. **The candidate query is a new, standalone sibling function
   (`queries::search_journey_leg_candidates`), not an edit to
   `search_schedule_calling_point_departures`.** The spec names both as
   options and calls it "a judgment call for the implementer, not this
   spec" (§2.1). Two reasons this plan picks the sibling:
   - **Behavioral scope.** A journey leg's "depart X, arrive Y" is only
     meaningful if Y is reached strictly *after* departing X, for *any*
     origin/destination pair. The general-purpose `/trains` search page's
     `stops_at` filter deliberately does **not** enforce that for two
     *different* stations (only for the same-station loop-service case) —
     see that function's own doc comment, point 2: "`stops_at` has always
     meant 'calls at X somewhere on its route'... it is not, and does not
     become, a 'you can get there from the station you searched' filter."
     Widening that in place would be an unrelated, unreviewed behavior
     change to an already-shipped public endpoint, not something to
     smuggle into this feature.
   - **Merge safety.** Per this plan's own staleness note above, other
     in-flight branches are independently modifying that exact function's
     `stops_at`/ordering logic right now. A sibling function with zero line
     overlap cannot collide with that work; editing the shared function in
     place could.

   The cost: Task 4's new function duplicates roughly 25 lines of
   row-to-JSON mapping logic from the original rather than factoring out a
   shared helper. Worth revisiting once the in-flight `stops_at` work has
   landed and this function's own shape has proven stable — not attempted
   here, and called out in that function's own doc comment so it isn't
   mistaken for an oversight.

3. **`POST /Journeys` supports THREE leg-creation shapes, not two.** The
   task brief driving this plan paraphrases the spec as "a direct-known-train
   leg AND an open time-window-search leg" — but the spec's own §9 Phase 1
   text and §7.2 are more precise: `POST /Journeys` must wrap **both**
   `create_pin` (the legacy CRS+time *guess* — what `TrackTrainForm.tsx`
   submits today, `POST /Train/track`) **and** `create_subscription_for_train`
   (an *already-known* identity — what `TrackThisTrainButton.tsx` submits
   today, `POST /Train/by-uid/{uid}/{date}/track`), because §7.2 is explicit
   that both existing frontend entry points must be rewired onto this one
   new route, and neither can be silently dropped or merged into the other
   (a CRS+time guess and a known `train_uid` are genuinely different pieces
   of information). "Direct-known-train leg" in the task brief is read here
   as shorthand covering both of today's *immediately-bound* paths, as
   opposed to the genuinely new third path (an open leg with no train bound
   at all). This plan implements all three, as three variants of one tagged
   request enum (`mode: "pin" | "knownTrain" | "window"`), each wrapping the
   correspondingly-named existing function unchanged — see Tasks 6–7 and
   Task 9.

4. **An all-blank window search is rejected, not accepted as an "any
   time, any train" search.** The spec's own literal test for whether a
   matched leg offers "Change train" is "depart_after etc. non-null" (§2.3,
   §4). If a window-mode leg could be created with all four bounds left
   blank, it would be schema-indistinguishable, on every later read, from a
   `pin`/`knownTrain`-mode leg that never had a window at all (both would
   have all four columns `NULL`) — silently breaking "Change train" for
   that leg forever. Rather than accept that as a known gap, Task 6's
   `validate_window_leg` requires at least one of the four bounds to be set.
   This is a small, honestly-documented product constraint ("enter at least
   one earliest/latest departure or arrival time"), not a workaround for a
   backend limitation — an unbounded-by-time search is still expressible by
   setting just a wide bound (e.g. depart-after 00:00), so nothing a real
   user wants is actually prevented.

5. **The `journeys` row and its one `journey_legs` row are NOT created
   inside a single database transaction spanning the existing
   `create_pin`/`create_subscription_for_train` call.** Both of those
   existing functions are typed to take `pool: &PgPool`, not a transaction
   handle, and widening their signatures (to accept `impl sqlx::Executor`,
   say) is out of this phase's scope — it would touch two functions with
   many existing callers across `routes/train.rs` for a benefit (atomicity
   of a two-row insert on a brand-new table) that matches this codebase's
   already-relaxed consistency posture elsewhere (e.g. `post_track_by_uid`'s
   own multi-step, non-transactional sequence of `find_or_create_train` →
   `create_subscription_for_train` → best-effort `enrich_shared_train`). The
   accepted residual risk: a process crash between the `journeys` insert and
   the `journey_legs` insert leaves an orphaned, leg-less `journeys` row.
   This is a narrow window (two adjacent `INSERT`s, no `.await` points that
   can suspend for long between them beyond ordinary scheduling), has no
   different blast radius than `train_subscriptions` rows already growing
   unbounded with no retention job (§0.1's own accepted gap), and is
   trivially cleaned up later (`DELETE FROM journeys WHERE id NOT IN (SELECT
   DISTINCT journey_id FROM journey_legs)`) if it is ever observed in
   practice.

6. **Journey rename, delete, and the §4 skip badge/platform column are
   explicitly deferred**, even though the spec's own §4 describes an
   editable header and other polish. See Non-goals below for the complete
   reasoning per item — in short: rename/delete are cheap follow-ups this
   plan intentionally does not bundle in (the task brief's own enumerated
   Phase 1 route list does not include them), the skip badge needs §5's
   detection mechanism (explicitly Phase 3), and the platform column is
   explicitly blocked on unmerged, unrelated sibling work per the spec's
   own §0.6.

7. **The migration's `journeys.id` ↔ `train_subscriptions.id` correlation
   uses a temporary scratch column, not `RETURNING` trickery or an
   order-of-insertion assumption.** The spec's own §7.1 flags this as "an
   implementation detail for whoever writes the real migration" and
   explicitly warns off the unsafe `(user_id, tracked_at)` join. See Task 2
   for the mechanism and why it is correct by construction rather than by
   assumption.

---

## Non-goals

Restated so no task below accidentally reaches for one of these:

- **Multi-leg chaining** (`POST /Journeys/{id}/legs`, adding a second leg to
  an existing journey, the per-leg-status rollup badge) — spec §3, Phase 2.
  Every leg-creation function this plan adds hardcodes `leg_order = 1` and
  says so in its own doc comment.
- **`'auto'` match mode** — spec §2.3, deferred past Phase 1 by the
  2026-09-22 addendum itself. `journey_legs.match_mode` only ever becomes
  `'unmatched'` or `'manual'` from any code this plan adds; `'auto'` stays a
  schema-level possibility with no writer.
- **Journey-aware notification copy and station-skip detection** — spec §5,
  Phase 3. The notifier (`crates/notifier`) is untouched by this plan.
- **Group sharing of journeys** (`group_journeys`, `POST/DELETE
  /groups/{id}/journeys`, the cross-owner read-authorization path) — spec
  §6, Phase 4. `TrackTrainForm`/`TrackThisTrainButton`'s existing
  group-share follow-up (`shareTrackedTrainToGroup`) is **kept exactly as
  it is** — it still shares the underlying `train_subscriptions` row into a
  group via the existing `group_trains` table, unchanged, using the
  `trackingId` this plan's new response still carries. Journey-level
  sharing is a separate, additive Phase 4 capability layered on top later.
- **Journey rename** (`POST /Journeys/{id}/name`, a `RenameJourneyButton`).
  The spec's §4 view sketch shows an editable header; the task brief's own
  precise enumeration of Phase 1 backend routes does not include a rename
  route. This plan's `/journeys/[id]` header renders `customName ??` a
  computed default (see Task 17) but is not editable in Phase 1. Cheap
  follow-up once this scaffolding lands — same shape as
  `post_tracked_train_name`, nothing new to design.
- **Journey delete.** No `DELETE /Journeys/{id}` route. A user who wants to
  stop tracking everything under a journey can still delete the underlying
  tracked train directly via the existing, unchanged `DELETE
  /Train/{trackingId}` (reachable from `/track/mine`, which is untouched by
  this plan) — that orphans the `journey_legs.train_subscription_id`
  reference (`ON DELETE SET NULL`), leaving a leg-with-no-train exactly as
  described in the schema's own comment, and leaves the `journeys` row
  itself behind with nothing pointing at a real train. Accepted as a
  narrow, low-consequence gap for Phase 1 (mirrors §0.1's own "no
  retention/pruning job" acceptance for `train_subscriptions` itself);
  revisit once a journeys list page exists to make it visible at all.
- **The §4 station-skip badge.** Needs §5's Darwin-sample lookup, Phase 3.
- **The §4 platform column.** Blocked on unmerged, unrelated sibling work
  per the spec's own §0.6 — "Phase 1 of this spec should not block on it."
- **A dedicated `/journeys` or `/journeys/mine` list page.** `GET
  /Journeys/mine` (Task 12) is implemented and independently testable, but
  no frontend page consumes it in this plan. `/track/mine` continues to
  serve as the primary "everything I'm tracking" list unchanged — it still
  works correctly for journey-wrapped trains with zero changes, since `GET
  /Train/mine` (which it reads) is completely untouched by this plan (every
  new tracked train still gets an ordinary `train_subscriptions` row,
  journey-wrapping is purely additive). A dedicated journeys list page is a
  reasonable, small follow-up once this scaffolding lands.
- **`TrainSearchForm.tsx`/`/trains` page changes.** This plan reuses that
  component's *UI convention* (Task 15) but does not modify the file or its
  `TrackThisTrainButton` call sites' behavior beyond what Task 14 already
  changes globally (that button is shared by both `/trains` results and
  `/train/[uid]/[date]`'s own CTA — Task 14's change applies to both call
  sites automatically, with no `TrainSearchForm.tsx` edit needed).
- **Changing `/Train/*` backend routes.** Per spec §7.2 and §8's Open
  Question 4 (left unresolved, defaulting to "keep as-is"): every existing
  `/Train/*` route stays exactly as it is, still directly reachable, still
  the machinery every new journey-creation path calls into unchanged.

## Global Constraints

- **Every new route follows the 404-never-403 ownership convention**
  already established throughout `crates/api/src/routes/train.rs`'s own
  module doc: "doesn't exist" and "exists but isn't yours" are
  indistinguishable to the caller. Every new write/read folds the ownership
  check directly into its `WHERE`/`JOIN` clause (never a separate ownership
  `SELECT` followed by an unscoped operation) — see `journeys::get_owned_leg`
  (Task 5) and `journeys::set_leg_train_subscription` (Task 7) for the two
  load-bearing examples.
- **User-facing error copy carries no internal field names or Debug
  dumps** — same posture as `train_tracking::validate_pin`'s own doc
  comment (these 400 bodies are rendered verbatim by the frontend). Every
  new `validate_*` function's tests include the
  `validation_messages_carry_no_internal_field_names` guard this codebase's
  existing validators already use.
- **No new column, index, or route may be added to `train_subscriptions`,
  `trains`, `train_current_state`, `group_trains`, or the notifier.**
  Additive-only, per the spec's own §1.1 framing — every task's file list
  below is scoped accordingly.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (CI's actual invocation is
  `--workspace --all-features` via `auguwu/clippy-action`,
  `.github/workflows/ci.yml`'s `clippy` job — this plan additionally asks
  for `--all-targets` locally, the stricter default a contributor should
  run before pushing), `cargo test --workspace` (ignored tests skipped —
  CI's unconditional fast path, `.github/workflows/ci.yml`'s `rust-test`
  job, "cargo test --workspace (ignored tests skipped)" step), and `cargo
  test -p api -- --ignored --test-threads=1` for every DB-gated test this
  plan adds (CI's exact invocation, same job's "cargo test -p api -p
  aggregator -- --ignored (DB-backed tests)" step — this plan touches only
  `api`, not `aggregator`), requiring `DATABASE_URL=postgres://postgres:
  postgres@localhost:5432/postgres` against a real local Postgres with this
  crate's migrations applied (`sqlx migrate run` from `crates/api`, or let
  `cargo test`'s own runtime migration-on-connect handle it — see CI's own
  "Run api migrations" step for the explicit form). Frontend: `npm test`
  (vitest, CI's `frontend` job), `npx tsc --noEmit` (CI's own "Typecheck"
  step — this repo's `lint` stand-in, no ESLint config exists), and `npm
  run build` (CI's own "npm run build (next build)" step) before
  considering any frontend task done.
  **UI verification**: this repo's standing practice for a change with no
  automated end-to-end coverage — start the dev stack and manually verify
  in a real browser. Folded into Task 17's own Verify step, not a separate
  task.
- **File scope.** Modified/created:
  - `crates/common/src/lib.rs`
  - `crates/api/migrations/20260922090000_journeys.sql` (new)
  - `crates/api/migrations/20260922100000_journeys_historical_migration.sql` (new)
  - `crates/api/src/data/queries.rs`
  - `crates/api/src/data/journeys.rs` (new)
  - `crates/api/src/data/mod.rs`
  - `crates/api/src/routes/journeys.rs` (new)
  - `crates/api/src/routes/mod.rs`
  - `crates/api/src/routes/train.rs` (visibility widening only — `async fn`
    → `pub(crate) async fn` on three existing functions, no logic changes)
  - `crates/api/src/routes/trains.rs` (visibility widening only — same
    shape, four existing items)
  - `crates/api/src/main.rs` (one new `.merge(...)` line)
  - `frontend/lib/types.ts`
  - `frontend/lib/api.ts`
  - `frontend/components/TrackThisTrainButton.tsx`
  - `frontend/components/TrackTrainForm.tsx`
  - `frontend/components/JourneyLegCandidates.tsx` (new)
  - `frontend/components/JourneyLegCandidates.test.tsx` (new)
  - `frontend/components/JourneyLegCard.tsx` (new)
  - `frontend/app/journeys/[id]/page.tsx` (new)

  No other file changes. In particular: `crates/notifier/*`,
  `crates/api/src/routes/groups.rs`, `crates/api/src/data/groups.rs`,
  `frontend/app/track/mine/page.tsx`, `frontend/components/TrainJourney.tsx`,
  and `frontend/components/TrainSearchForm.tsx` are all untouched.

---

## Task 1: Migration — `journeys`/`journey_legs` schema

**Files:** create `crates/api/migrations/20260922090000_journeys.sql`.

Independent — a bare `CREATE TABLE` pair needs no Rust code to apply.
`20260922090000` sorts after the latest existing migration
(`20260917090000_incidents_affected_lines.sql`, confirmed via `ls
crates/api/migrations | sort | tail`).

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Journey tracking, Phase 1 schema.
-- docs/superpowers/specs/2026-09-22-journey-tracking-design.md §1.1, with
-- one correction the spec's own §7.1 makes to its §1.1 sketch: origin_crs/
-- destination_crs are NULLABLE here, not NOT NULL. An NR-primary-created
-- leg (a `knownTrain`-mode POST /Journeys, crates/api/src/routes/journeys.rs)
-- whose underlying `trains` row has no schedule data yet can legitimately
-- have neither -- the same accepted gap `train_subscriptions.pin_origin_crs`/
-- `pin_destination_crs` already model (see
-- 20260906130000_nullable_pin_columns.sql for that same correction, made
-- for the same underlying reason, on the table this one wraps).
--
-- Additive only -- train_subscriptions itself is UNCHANGED by this
-- migration or anything in this plan: every existing route, the notifier,
-- tickets, and group_trains sharing keep working byte-for-byte regardless
-- of this feature.
-- -------------------------------------------------------------------------

CREATE TABLE journeys (
    id          BIGSERIAL PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id),
    custom_name TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX journeys_user_id ON journeys (user_id);

CREATE TABLE journey_legs (
    id                     BIGSERIAL PRIMARY KEY,
    journey_id             BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    -- 1-based sequence within the journey. Phase 1 never writes anything
    -- but 1 -- see crates/api/src/data/journeys.rs's own module doc
    -- comment -- but the column exists now so Phase 2's "add a leg" work
    -- (design doc §3) needs no schema change, only a new writer.
    leg_order              INT NOT NULL,
    -- Nullable: see this migration's own header comment above. This is the
    -- leg's OWN travel intent, kept even once matched -- not merely
    -- derived from whatever train ends up bound to it (design doc §1.1).
    origin_crs              TEXT,
    destination_crs         TEXT,
    service_date             DATE NOT NULL,
    -- Time-window search intent (design doc §1.1, §2.3's 2026-09-22
    -- addendum). NULL for a leg created by picking a specific known train
    -- directly (a `pin` or `knownTrain` mode leg -- no window was ever
    -- searched). Once set, kept FOREVER, even after train_subscription_id
    -- is populated -- see that addendum: a matched leg's window stays live
    -- so "Change train" can re-open the exact same candidate search
    -- without the user re-entering criteria.
    depart_after             TIME,
    depart_before             TIME,
    arrive_after              TIME,
    arrive_before              TIME,
    -- Binding to a real train working, reusing 100% of existing
    -- train_subscriptions/trains/notifier/journey.rs machinery. ON DELETE
    -- SET NULL: deleting the underlying tracked train (existing, unchanged
    -- DELETE /Train/{trackingId}) orphans the leg rather than cascading
    -- into deleting the journey itself.
    train_subscription_id      BIGINT REFERENCES train_subscriptions(id) ON DELETE SET NULL,
    match_mode                 TEXT NOT NULL DEFAULT 'unmatched'
                                CHECK (match_mode IN ('unmatched', 'manual', 'auto')),
    created_at                 TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (journey_id, leg_order)
);
CREATE INDEX journey_legs_journey_id ON journey_legs (journey_id);
CREATE INDEX journey_legs_train_subscription_id ON journey_legs (train_subscription_id);
```

- [ ] **Step 2: Verify.** `sqlx` migrations in this crate run automatically
  against `DATABASE_URL` on `cargo test`/`cargo run` startup — no separate
  `sqlx migrate run` step exists locally (CI runs one explicitly before its
  own `--ignored` DB-gated tests; that is CI's own separate concern, not a
  local requirement). Confirm the migration applies cleanly:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d journeys"
psql "$DATABASE_URL" -c "\d journey_legs"
```

  Expected: no `sqlx::migrate::MigrateError`; both tables show the columns
  above with the stated nullability, and `journey_legs`'s `match_mode`
  column shows its `CHECK` constraint.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260922090000_journeys.sql
git commit -m "api: add journeys/journey_legs schema (journey tracking Phase 1)"
```

---

## Task 2: Migration — historical-data backfill (§7.1)

**Files:** create
`crates/api/migrations/20260922100000_journeys_historical_migration.sql`.

Depends on Task 1 (both tables must exist). This is the migration that
makes requirement #6b's "supersede" framing true for every tracked train
that existed before this feature shipped — after this runs, every
`train_subscriptions` row has exactly one `journeys`/`journey_legs` pair
wrapping it, so `GET /Journeys/mine` (Task 12) is never empty for a user
who already had tracked trains.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- One-time backfill: every EXISTING train_subscriptions row becomes its
-- own single-leg journey (design doc §7.1) -- strictly 1:1, never merged,
-- even for a user with many tracked trains (requirement #6b is explicit:
-- EACH pre-existing tracked train becomes its OWN journey).
--
-- Correlating each new `journeys` row back to the `train_subscriptions`
-- row it came from cannot use RETURNING alone: a RETURNING clause on an
-- INSERT ... SELECT only ever exposes columns of the row just inserted
-- into `journeys` itself -- there is no way to echo an arbitrary source
-- column (train_subscriptions.id) back out of it. Joining back afterwards
-- on (user_id, tracked_at) is unsafe too -- not unique (two subscriptions
-- for the same user created in the same instant are a real, if rare,
-- case) -- and the design doc's own §7.1 explicitly warns this off.
--
-- This uses a temporary scratch column instead: add it, populate it
-- alongside the real columns in the SAME INSERT (so `journeys.id` and
-- `train_subscriptions.id` are correlated by construction -- a real column
-- value copied verbatim, not by insertion-order assumption or a second,
-- fallible lookup), use it to drive the journey_legs INSERT, then drop it
-- -- all inside this one migration, so `journeys`' final, permanent shape
-- carries no trace of this bookkeeping.
-- -------------------------------------------------------------------------

ALTER TABLE journeys ADD COLUMN _migration_source_subscription_id BIGINT;

INSERT INTO journeys (user_id, custom_name, created_at, updated_at, _migration_source_subscription_id)
SELECT user_id, custom_name, tracked_at, tracked_at, id
FROM train_subscriptions;

-- Every migrated leg is already bound to a real train_subscriptions row --
-- there is no window to re-open, so match_mode = 'manual' and every
-- depart_*/arrive_* column stays NULL (design doc §7.1's own explicit
-- statement of this).
INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, train_subscription_id, match_mode)
SELECT j.id, 1, ts.pin_origin_crs, ts.pin_destination_crs, ts.service_date, ts.id, 'manual'
FROM journeys j
JOIN train_subscriptions ts ON ts.id = j._migration_source_subscription_id;

ALTER TABLE journeys DROP COLUMN _migration_source_subscription_id;
```

- [ ] **Step 2: Verify.** Row-count and 1:1-correlation checks, run against
  a database that already has some `train_subscriptions` rows (seed one
  first if testing against an empty local database):

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "SELECT (SELECT COUNT(*) FROM train_subscriptions) AS ts_count, (SELECT COUNT(*) FROM journeys) AS journeys_count, (SELECT COUNT(*) FROM journey_legs) AS legs_count;"
psql "$DATABASE_URL" -c "SELECT COUNT(*) FROM journey_legs jl JOIN journeys j ON j.id = jl.journey_id JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id WHERE ts.user_id != j.user_id;"
psql "$DATABASE_URL" -c "\d journeys"
```

  Expected: `ts_count = journeys_count = legs_count` exactly (1:1:1); the
  second query returns `0` (every migrated leg's train belongs to the same
  user as its journey); `\d journeys` shows no
  `_migration_source_subscription_id` column (dropped).

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260922100000_journeys_historical_migration.sql
git commit -m "api: backfill one single-leg journey per pre-existing tracked train"
```

---

## Task 3: `crates/common` — `TimeWindow` shared type (§2.1)

**Files:** modify `crates/common/src/lib.rs`.

Independent. Closes the "no reusable before/after type" gap the spec names
explicitly (§0.5, §2.1): every existing before/after pair in this codebase
(`search_schedule_calling_point_departures`'s own `scheduled_from`/`to_time`
and `stop_arrival_from`/`stop_arrival_to`, `TrainSearchParams`'s
`from`/`to`/`arrival_from`/`arrival_to`) is two sibling `Option<NaiveTime>`
fields, duplicated at each layer rather than named once.

- [ ] **Step 1: Add the type**, near `TrackPinRequest`
  (`crates/common/src/lib.rs:638`), the request type most directly related:

```rust
/// A reusable, named "after/before" bound pair on a `NaiveTime` --
/// closes a real, named gap
/// (docs/superpowers/specs/2026-09-22-journey-tracking-design.md §0.5,
/// §2.1): every existing before/after pair in this codebase (e.g.
/// `queries::search_schedule_calling_point_departures`'s own
/// `scheduled_from`/`to_time` and `stop_arrival_from`/`stop_arrival_to`
/// parameters, `TrainSearchParams::from`/`to`/`arrival_from`/`arrival_to`)
/// is two sibling `Option<NaiveTime>` fields, duplicated at each layer
/// rather than named once. Used by `CreateJourneyLegRequest::Window`
/// (`crates/api/src/routes/journeys.rs`) for its `departWindow`/
/// `arriveWindow` fields -- the request-body/write side only. The read
/// side (`JourneyLegDetailResponse`, same file) deliberately flattens back
/// to four individual `departAfter`/`departBefore`/`arriveAfter`/
/// `arriveBefore` fields, matching how every other optional time pair in
/// this codebase's existing wire responses is already shaped, rather than
/// introducing a new nested-object convention on the read side too.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TimeWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<chrono::NaiveTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<chrono::NaiveTime>,
}

impl TimeWindow {
    /// `true` if neither bound is set -- an entirely unbounded window.
    /// `journeys::validate_window_leg` (`crates/api/src/data/journeys.rs`)
    /// rejects a leg whose `depart`/`arrive` windows are BOTH empty --
    /// see that function's own doc comment for why.
    pub fn is_empty(&self) -> bool {
        self.after.is_none() && self.before.is_none()
    }
}
```

- [ ] **Step 2: Verify**

```bash
cargo build -p common
```

  Expected: builds clean (a bare additive `pub struct` cannot break any
  existing caller).

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "common: add TimeWindow, the shared before/after bound-pair type"
```

---

## Task 4: `queries.rs` — `search_journey_leg_candidates` (§2.1)

**Files:** modify `crates/api/src/data/queries.rs`.

Depends on nothing else in this plan (a pure addition to an existing file).
See Judgment Call 2 for why this is a new, standalone function rather than
an edit to `search_schedule_calling_point_departures`.

- [ ] **Step 1: Add the function**, directly below
  `search_schedule_calling_point_departures` (after its closing brace,
  `queries.rs` ~line 1562):

```rust
/// The time-window candidate search behind journey-leg matching (`GET
/// /Journeys/{journeyId}/legs/{legId}/candidates`,
/// `crates/api/src/routes/journeys.rs`) --
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §2.1.
///
/// A deliberate SIBLING of `search_schedule_calling_point_departures`
/// above, not an extension of it, even though the design doc names
/// extending that function as one option. Two reasons:
///
/// 1. **Behavioral difference, not just a superset.** A journey leg's
///    "depart X, arrive Y" is meaningless unless Y is reached strictly
///    AFTER departing X (§0.5's own named gap) -- so this function's
///    `EXISTS` branches enforce that ordering UNCONDITIONALLY, for every
///    origin/destination pair. `search_schedule_calling_point_departures`
///    above only enforces it for the SAME-station case (a loop-service
///    check, `stop.origin_crs = main.origin_crs`) and deliberately does
///    NOT for two different stations -- see that function's own doc
///    comment, point 2. That is the right behavior for the general-purpose
///    `/trains` search page (matching a schedule by any calling point, not
///    a directed leg); widening it in place would be an unrelated,
///    unreviewed behavior change to that already-shipped public endpoint.
/// 2. **Merge safety.** Other in-flight, unmerged branches independently
///    modify `search_schedule_calling_point_departures`'s own `stops_at`/
///    ordering logic (see this plan's own staleness note). A sibling
///    function with zero line overlap cannot collide with that work.
///
/// Consequently this duplicates ~25 lines of row-to-JSON mapping logic
/// from `search_schedule_calling_point_departures` rather than factoring
/// out a shared helper -- deliberately, for the same merge-safety reason.
/// Worth doing once the in-flight `stops_at` work above has landed and
/// this function's own shape has proven stable; not attempted here.
///
/// No `true_origin_crs`/`stops_at` params, unlike the function above: a
/// journey leg always names both ends explicitly (`origin_crs`,
/// `destination_crs`, both required), so there is no "optional filter"
/// shape to carry over. `depart_after`/`depart_before` bound
/// `main.scheduled` (the departure at `origin_crs`, both now genuinely
/// optional, unlike `search_schedule_calling_point_departures`'s
/// mandatory `scheduled_from` -- that function's caller always supplies a
/// concrete floor, either an explicit `from` or a `now`-forward default;
/// a journey-leg window search has no such default to fall back on).
/// `arrive_after`/`arrive_before` bound the arrival at `destination_crs`,
/// mirroring `stop_arrival_from`/`stop_arrival_to`'s own two-branch shape
/// above (the `EXISTS` branch for an intermediate call, `main.destination_crs
/// = $5` for the true-terminus case) -- same NULL-never-satisfies-a-bound
/// contract.
///
/// `Ok(None)` means no CIF publish has landed for `service_date` at all
/// (maps to a 404, mirroring the function above). `Ok(Some(page))` with an
/// empty `page.departures` means the day IS published and the window
/// matched nothing.
#[allow(clippy::too_many_arguments)]
pub async fn search_journey_leg_candidates(
    pool: &PgPool,
    origin_crs: &str,
    destination_crs: &str,
    service_date: chrono::NaiveDate,
    depart_after: Option<chrono::NaiveTime>,
    depart_before: Option<chrono::NaiveTime>,
    arrive_after: Option<chrono::NaiveTime>,
    arrive_before: Option<chrono::NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>> {
    let fetch = limit.saturating_add(1);

    let rows: Vec<(
        String,
        String,
        Option<String>,
        chrono::NaiveTime,
        Option<chrono::NaiveTime>,
        i16,
    )> = sqlx::query_as(
        r#"
            SELECT main.train_uid, main.destination_crs, main.true_origin_crs, main.scheduled, main.destination_arrival, main.destination_arrival_day_offset
            FROM schedule_destination_departures main
            WHERE main.service_date = $1
              AND main.origin_crs = $2
              AND ($3::time IS NULL OR main.scheduled >= $3)
              AND ($4::time IS NULL OR main.scheduled <= $4)
              AND (
                    main.destination_crs = $5
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          -- Unconditional, unlike
                          -- search_schedule_calling_point_departures'
                          -- same-station-only ordering check -- a journey
                          -- leg's destination must be reached AFTER its
                          -- origin regardless of which two stations they
                          -- are. See this function's own doc comment.
                          AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                    )
              )
              AND (
                    ($6::time IS NULL AND $7::time IS NULL)
                    OR (
                        main.destination_crs = $5
                        AND ($6::time IS NULL OR main.destination_arrival >= $6)
                        AND ($7::time IS NULL OR main.destination_arrival <= $7)
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                          AND ($6::time IS NULL OR stop.calling_point_arrival >= $6)
                          AND ($7::time IS NULL OR stop.calling_point_arrival <= $7)
                    )
              )
              AND ($8::time IS NULL
                   OR (main.scheduled, main.train_uid) > ($8, $9))
            ORDER BY main.scheduled, main.train_uid
            LIMIT $10
            "#,
    )
    .bind(service_date)
    .bind(origin_crs)
    .bind(depart_after)
    .bind(depart_before)
    .bind(destination_crs)
    .bind(arrive_after)
    .bind(arrive_before)
    .bind(after.map(|c| c.scheduled))
    .bind(after.map(|c| c.train_uid.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        if !schedule_destination_departures_published_for(pool, service_date).await? {
            return Ok(None);
        }
        return Ok(Some(CallingPointDeparturePage {
            departures: Vec::new(),
            next_cursor: None,
        }));
    }

    let has_more = rows.len() as i64 > limit;
    let page_rows = if has_more {
        &rows[..limit as usize]
    } else {
        &rows[..]
    };

    let next_cursor = if has_more {
        page_rows.last().map(
            |(train_uid, _, _, scheduled, _, _)| CallingPointDepartureCursor {
                scheduled: *scheduled,
                train_uid: train_uid.clone(),
            },
        )
    } else {
        None
    };

    let departures = page_rows
        .iter()
        .map(
            |(
                train_uid,
                destination_crs,
                true_origin_crs,
                scheduled,
                destination_arrival,
                destination_arrival_day_offset,
            )| {
                serde_json::json!({
                    "uid": train_uid,
                    "destination_crs": destination_crs,
                    "true_origin_crs": true_origin_crs,
                    "scheduled": scheduled.format("%H:%M:%S").to_string(),
                    "destination_arrival": destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
                    "destination_arrival_day_offset": destination_arrival_day_offset,
                })
            },
        )
        .collect();

    Ok(Some(CallingPointDeparturePage {
        departures,
        next_cursor,
    }))
}
```

- [ ] **Step 2: Add DB-gated tests**, alongside this file's existing
  `mod db_tests` (search for the module in `queries.rs`; add these near any
  existing `search_schedule_calling_point_departures` tests so future
  readers find both search functions' tests together):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                search_journey_leg_candidates -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_enforces_ordering_for_different_stations() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-14".parse().unwrap();
        // A train that calls at RDG BEFORE WAT on this diagram -- the
        // "loops back the wrong way" case this function must exclude for
        // a WAT -> RDG leg search, unlike the general-purpose
        // search_schedule_calling_point_departures (see this function's
        // own doc comment).
        seed_schedule_destination_departure(&pool, service_date, "T00001", "RDG", "08:00:00", "SOU").await;
        seed_schedule_destination_departure(&pool, service_date, "T00001", "WAT", "08:30:00", "SOU").await;
        // A genuinely valid candidate: WAT then RDG, in order.
        seed_schedule_destination_departure(&pool, service_date, "T00002", "WAT", "09:00:00", "RDG").await;
        seed_schedule_destination_departure(&pool, service_date, "T00002", "RDG", "09:30:00", "RDG").await;

        let page = search_journey_leg_candidates(
            &pool, "WAT", "RDG", service_date, None, None, None, None, None, 50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(uids, vec!["T00002"], "T00001 calls at RDG before WAT and must be excluded");

        cleanup_schedule_destination_departures(&pool, service_date, &["T00001", "T00002"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                search_journey_leg_candidates -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_applies_depart_and_arrive_windows() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-14".parse().unwrap();
        seed_schedule_destination_departure(&pool, service_date, "T00003", "WAT", "07:00:00", "RDG").await;
        seed_schedule_destination_departure(&pool, service_date, "T00003", "RDG", "07:30:00", "RDG").await;
        seed_schedule_destination_departure(&pool, service_date, "T00004", "WAT", "09:00:00", "RDG").await;
        seed_schedule_destination_departure(&pool, service_date, "T00004", "RDG", "09:30:00", "RDG").await;

        let page = search_journey_leg_candidates(
            &pool,
            "WAT",
            "RDG",
            service_date,
            Some("08:00:00".parse().unwrap()),
            None,
            None,
            Some("09:35:00".parse().unwrap()),
            None,
            50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(uids, vec!["T00004"]);

        cleanup_schedule_destination_departures(&pool, service_date, &["T00003", "T00004"]).await;
    }
```

  These reuse `db_tests`' existing `connect()` helper and assume
  `seed_schedule_destination_departure`/`cleanup_schedule_destination_departures`
  helpers already exist in this module's test scaffolding (the existing
  `search_schedule_calling_point_departures` tests need the identical
  fixture shape — inspect that test block first; if no such helper exists
  yet under those exact names, add a minimal one following this file's
  existing `INSERT INTO schedule_destination_departures (...) VALUES
  (...)` pattern rather than inventing a differently-shaped one).

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api search_journey_leg_candidates -- --ignored --test-threads=1
```

  Expected: builds and lints clean; both new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "api: add search_journey_leg_candidates, the journey-leg candidate query"
```

---

## Task 5: `data/journeys.rs` — module skeleton, ownership reads

**Files:** create `crates/api/src/data/journeys.rs`; modify
`crates/api/src/data/mod.rs`.

Depends on Task 1 (tables must exist for any test to run) and Task 3
(`common::TimeWindow`, used by later tasks in this same file — declared now
so the module compiles incrementally task by task).

- [ ] **Step 1: Register the module**

```rust
// crates/api/src/data/mod.rs -- add alongside the existing alphabetical list
pub mod journeys;
```

- [ ] **Step 2: Create the file** with its module doc comment and the two
  ownership-scoped read functions:

```rust
//! Journey tracking (Phase 1): a `journeys` row groups one-or-more
//! `journey_legs`, each either bound to a real `train_subscriptions` row
//! (a "matched" leg) or an open time-window search waiting for a manual
//! pick (an "unmatched" leg). See
//! docs/superpowers/specs/2026-09-22-journey-tracking-design.md and
//! docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md.
//!
//! **Phase 1 never creates more than one leg per journey.** Every
//! leg-creation function in this file hardcodes `leg_order = 1` and says so
//! in its own doc comment -- multi-leg chaining (design doc §3, "add a leg
//! to an existing journey") is a later phase's job. The schema itself
//! (`leg_order`, `UNIQUE (journey_id, leg_order)`) is already
//! multi-leg-shaped regardless, per the design doc's own reasoning for not
//! folding leg fields onto `train_subscriptions` (§1.1) -- so that later
//! phase needs no schema change, only a new writer.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::Serialize;
use sqlx::PgPool;

/// Mirrors `train_tracking::tracked_train_owner` exactly -- `None` for "no
/// journey with that id"; the route layer maps both that and a mismatch to
/// `404`, never `403` (this codebase's universal ownership convention).
/// Not currently called anywhere in this plan's own routes (every route
/// below folds its ownership check directly into a `JOIN`/`WHERE` instead,
/// per the same convention) -- kept as a small, independently useful,
/// independently testable primitive, matching `tracked_train_owner`'s own
/// role in `train_tracking.rs`.
pub async fn journey_owner(pool: &PgPool, journey_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_id FROM journeys WHERE id = $1")
        .bind(journey_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id,)| id))
}

/// One `journey_legs` row, scoped to a caller's ownership of its parent
/// journey -- folds the ownership check directly into the `WHERE`/`JOIN`
/// (this codebase's established convention, e.g.
/// `train_tracking::delete_tracked_train`'s `WHERE id = $1 AND user_id =
/// $2`) rather than a separate `journey_owner` lookup followed by an
/// unscoped read. Backs both `GET .../candidates` (reads the window/CRS
/// fields to search with, Task 9) and `POST .../train` (confirms ownership
/// before writing, Task 7).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyLegRow {
    pub id: i64,
    pub journey_id: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub service_date: NaiveDate,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
    pub train_subscription_id: Option<i64>,
    pub match_mode: String,
}

pub async fn get_owned_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneyLegRow>> {
    let row = sqlx::query_as::<_, JourneyLegRow>(
        "SELECT jl.id, jl.journey_id, jl.origin_crs, jl.destination_crs, jl.service_date, \
                jl.depart_after, jl.depart_before, jl.arrive_after, jl.arrive_before, \
                jl.train_subscription_id, jl.match_mode \
         FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.id = $1 AND jl.journey_id = $2 AND j.user_id = $3",
    )
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 3: Verify**

```bash
cargo build -p api
```

  Expected: builds clean (both functions are self-contained additions; the
  module is registered but nothing outside this file calls into it yet).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/journeys.rs crates/api/src/data/mod.rs
git commit -m "api: add data::journeys module skeleton (ownership reads)"
```

---

## Task 6: `data/journeys.rs` — leg-creation functions (§7.2, Judgment Call 3)

**Files:** modify `crates/api/src/data/journeys.rs`.

Depends on Task 5. Implements all three `POST /Journeys` leg shapes (Judgment
Call 3) by wrapping the existing, unchanged `train_tracking::create_pin`/
`create_subscription_for_train` for the two immediately-bound shapes, plus a
genuinely new insert for the open-window shape.

- [ ] **Step 1: Add the shared insert helpers**, private to this module:

```rust
async fn insert_journey(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<i64> {
    let (id,): (i64,) =
        sqlx::query_as("INSERT INTO journeys (user_id, custom_name) VALUES ($1, $2) RETURNING id")
            .bind(user_id)
            .bind(custom_name)
            .fetch_one(pool)
            .await?;
    Ok(id)
}

/// `leg_order` is always `1` -- see this module's own doc comment on why
/// Phase 1 never writes anything else.
#[allow(clippy::too_many_arguments)]
async fn insert_leg(
    pool: &PgPool,
    journey_id: i64,
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    service_date: NaiveDate,
    train_subscription_id: Option<i64>,
    match_mode: &str,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             train_subscription_id, match_mode) \
         VALUES ($1, 1, $2, $3, $4, $5, $6) \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(origin_crs)
    .bind(destination_crs)
    .bind(service_date)
    .bind(train_subscription_id)
    .bind(match_mode)
    .fetch_one(pool)
    .await?;
    Ok(id)
}
```

- [ ] **Step 2: Add `create_journey_with_pin_leg`** (the `pin` mode,
  replacing `TrackTrainForm.tsx`'s current `POST /Train/track` call):

```rust
/// Creates a one-leg journey around a legacy CRS+time GUESS pin
/// (`train_tracking::create_pin`, unchanged) -- the `pin` mode of
/// `POST /Journeys` (`crates/api/src/routes/journeys.rs`), replacing
/// `TrackTrainForm.tsx`'s direct `POST /Train/track` call (design doc
/// §7.2). Two sequential inserts, not one transaction -- see this plan's
/// own Judgment Call 5 for why (`create_pin` is typed to take `&PgPool`,
/// not a transaction handle; widening its signature is out of this
/// phase's scope).
///
/// The new leg's `origin_crs`/`destination_crs` are the pin's OWN
/// `origin_crs`/`destination_crs` (the latter may be `None` -- optional on
/// `TrackPinRequest`), never re-derived from anywhere else.
/// `match_mode = 'manual'` immediately: the leg is bound to a real
/// `train_subscriptions` row from birth (even though that row's own
/// `resolution_status` may still be `'pending'`), exactly mirroring how
/// Task 2's own historical backfill treats every pre-existing row --
/// "already bound to a real train, no window to re-open."
/// `depart_*`/`arrive_*` stay `NULL`: no window was ever searched.
///
/// Returns `(journey_id, leg_id, tracking_id)` -- the route layer
/// (`crates/api/src/routes/journeys.rs::post_journey`) still needs
/// `tracking_id` to run the same best-effort schedule/backlog match
/// attempts `post_track` already makes for a bare pin.
pub async fn create_journey_with_pin_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    pin: &common::TrackPinRequest,
) -> anyhow::Result<(i64, i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let tracking_id = crate::data::train_tracking::create_pin(pool, pin, user_id).await?;
    let leg_id = insert_leg(
        pool,
        journey_id,
        Some(pin.origin_crs.as_str()),
        pin.destination_crs.as_deref(),
        pin.service_date,
        Some(tracking_id),
        "manual",
    )
    .await?;
    Ok((journey_id, leg_id, tracking_id))
}
```

- [ ] **Step 3: Add `create_journey_with_known_train_leg`** (the
  `knownTrain` mode, replacing `TrackThisTrainButton.tsx`'s current `POST
  /Train/by-uid/{uid}/{date}/track` call):

```rust
/// Creates a one-leg journey around an ALREADY-known train identity
/// (`train_tracking::create_subscription_for_train`, unchanged) -- the
/// `knownTrain` mode of `POST /Journeys`, replacing
/// `TrackThisTrainButton.tsx`'s direct
/// `POST /Train/by-uid/{uid}/{date}/track` call.
///
/// `origin_crs`/`destination_crs` are read back off the RESULTING
/// `train_subscriptions` row's own `pin_origin_crs`/`pin_destination_crs`
/// -- populated live from the `trains` row by
/// `create_subscription_for_train` itself, `NULL` if that row has no
/// schedule data yet (the same accepted gap named on
/// `TrackedTrainState::pin_origin_crs`'s own doc comment,
/// `crates/api/src/data/train_tracking.rs`, and the reason Task 1's
/// migration made these two columns nullable -- see this plan's own
/// Judgment Call 1). Never independently supplied by the caller: the
/// caller only ever has a bare `(trainUid, serviceDate)` for this mode.
/// `match_mode = 'manual'`, `depart_*`/`arrive_*` `NULL` -- same reasoning
/// as [`create_journey_with_pin_leg`].
pub async fn create_journey_with_known_train_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    trains_id: i64,
    service_date: NaiveDate,
) -> anyhow::Result<(i64, i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(pool, trains_id, user_id)
            .await?;
    let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
    )
    .bind(tracking_id)
    .fetch_optional(pool)
    .await?;
    let (origin_crs, destination_crs) = pins.unwrap_or((None, None));
    let leg_id = insert_leg(
        pool,
        journey_id,
        origin_crs.as_deref(),
        destination_crs.as_deref(),
        service_date,
        Some(tracking_id),
        "manual",
    )
    .await?;
    Ok((journey_id, leg_id, tracking_id))
}
```

- [ ] **Step 4: Add `validate_window_leg` and
  `create_journey_with_window_leg`** (the `window` mode, genuinely new):

```rust
/// User-facing validation for a window-search leg's request fields --
/// same posture as `train_tracking::validate_pin`'s own doc comment (this
/// message is rendered verbatim by the frontend). Requires real 3-letter
/// CRS codes for both ends (mirroring `routes::trains::normalize_crs`'s own
/// check, duplicated here rather than reached across the
/// `data`/`routes` module boundary -- that helper is private to
/// `routes/trains.rs`) and AT LEAST ONE of the four window bounds set.
///
/// The "at least one bound" rule closes a real, otherwise-silent
/// ambiguity: `journey_legs.depart_after`/`.../arrive_before` being
/// non-null is literally the signal `GET /Journeys/{id}` uses to decide
/// whether a matched leg offers "Change train" (design doc §2.3/§4). A
/// window search with all four bounds left blank would be a legitimate
/// "any train, any time" search, but would leave every one of those four
/// columns NULL -- indistinguishable, on every later read, from a leg that
/// was never window-searched at all (a `pin`/`knownTrain`-mode leg).
/// Requiring one bound closes that ambiguity outright. See this plan's own
/// Judgment Call 4 for the full reasoning.
pub fn validate_window_leg(
    origin_crs: &str,
    destination_crs: &str,
    depart_window: &TimeWindow,
    arrive_window: &TimeWindow,
) -> Result<(), String> {
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
    if depart_window.is_empty() && arrive_window.is_empty() {
        return Err(
            "Enter at least one earliest/latest departure or arrival time to search a window \
             — or pick a specific known departure instead."
                .to_string(),
        );
    }
    Ok(())
}

/// Creates a one-leg journey around an OPEN time-window search -- the
/// `window` mode of `POST /Journeys` (design doc §2.1, §9's revised Phase
/// 1 scope). No `train_subscriptions` row at all yet: `train_subscription_id`
/// is `NULL`, `match_mode = 'unmatched'`, and the caller is expected to
/// browse `GET /Journeys/{journeyId}/legs/{legId}/candidates` and commit
/// one via `POST /Journeys/{journeyId}/legs/{legId}/train` next (Tasks 9,
/// 11).
///
/// Caller must have already run `origin_crs`/`destination_crs`/
/// `depart_window`/`arrive_window` through [`validate_window_leg`] -- this
/// function does no validation of its own, matching this codebase's
/// established "route validates, data layer writes" split (e.g.
/// `train_tracking::rename_tracked_train`'s own doc comment).
#[allow(clippy::too_many_arguments)]
pub async fn create_journey_with_window_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    origin_crs: &str,
    destination_crs: &str,
    service_date: NaiveDate,
    depart_window: TimeWindow,
    arrive_window: TimeWindow,
) -> anyhow::Result<(i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             depart_after, depart_before, arrive_after, arrive_before, match_mode) \
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, 'unmatched') \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(origin_crs)
    .bind(destination_crs)
    .bind(service_date)
    .bind(depart_window.after)
    .bind(depart_window.before)
    .bind(arrive_window.after)
    .bind(arrive_window.before)
    .fetch_one(pool)
    .await?;
    Ok((journey_id, id))
}
```

- [ ] **Step 5: Add DB-gated tests**, in a new `#[cfg(test)] mod db_tests`
  at the bottom of the file, mirroring `train_tracking.rs`'s own `db_tests`
  scaffolding (`connect`/`seed_user`/`cleanup_user` — copy those three
  helpers verbatim from `crates/api/src/data/train_tracking.rs`'s own
  `db_tests` module rather than sharing them cross-file, matching this
  codebase's established "test scaffolding is duplicated per file, not
  shared" convention, per `routes/train.rs`'s own `test_app` doc comment):

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "DELETE FROM journey_legs WHERE journey_id IN (SELECT id FROM journeys WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_legs");
        sqlx::query("DELETE FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journeys");
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    fn fixture_pin(origin_crs: &str) -> common::TrackPinRequest {
        common::TrackPinRequest {
            service_date: "2026-09-22".parse().unwrap(),
            origin_crs: origin_crs.to_string(),
            scheduled_departure: "2026-09-22T09:00:00Z".parse().unwrap(),
            destination_crs: Some("EDB".to_string()),
            operator: None,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_pin_leg -- --ignored --test-threads=1`"]
    async fn create_journey_with_pin_leg_wraps_a_pin_in_a_one_row_journey() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-PIN").await;

        let (journey_id, leg_id, tracking_id) =
            create_journey_with_pin_leg(&pool, "TEST-JOURNEY-PIN", None, &fixture_pin("KGX"))
                .await
                .expect("create journey with pin leg");

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-PIN")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.origin_crs.as_deref(), Some("KGX"));
        assert_eq!(leg.destination_crs.as_deref(), Some("EDB"));
        assert_eq!(leg.train_subscription_id, Some(tracking_id));
        assert_eq!(leg.match_mode, "manual");
        assert_eq!(leg.depart_after, None);

        cleanup_user(&pool, "TEST-JOURNEY-PIN").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_window_leg -- --ignored --test-threads=1`"]
    async fn create_journey_with_window_leg_creates_an_unmatched_leg() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-WINDOW").await;

        let depart_window = common::TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: Some("10:00:00".parse().unwrap()),
        };
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-WINDOW",
            Some("Commute"),
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            depart_window,
            common::TimeWindow::default(),
        )
        .await
        .expect("create journey with window leg");

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-WINDOW")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, None);
        assert_eq!(leg.match_mode, "unmatched");
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));
        assert_eq!(leg.depart_before, Some("10:00:00".parse().unwrap()));

        cleanup_user(&pool, "TEST-JOURNEY-WINDOW").await;
    }

    #[test]
    fn validate_window_leg_rejects_an_all_blank_window() {
        let err = validate_window_leg(
            "WAT",
            "RDG",
            &common::TimeWindow::default(),
            &common::TimeWindow::default(),
        )
        .unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_window_leg_accepts_one_bound_set() {
        let depart_window = common::TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: None,
        };
        assert!(validate_window_leg("WAT", "RDG", &depart_window, &common::TimeWindow::default()).is_ok());
    }

    #[test]
    fn validate_window_leg_messages_carry_no_internal_field_names() {
        let messages = [
            validate_window_leg("W", "RDG", &common::TimeWindow::default(), &common::TimeWindow::default())
                .unwrap_err(),
            validate_window_leg(
                "WAT",
                "RDG",
                &common::TimeWindow::default(),
                &common::TimeWindow::default(),
            )
            .unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty());
            assert!(!message.contains('_'), "user-facing copy leaked an identifier: {message}");
        }
    }
}
```

- [ ] **Step 6: Verify**

```bash
cargo build -p api
cargo test -p api validate_window_leg
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api create_journey_with -- --ignored --test-threads=1
```

  Expected: pure unit tests (3) pass without a database; both DB-gated
  tests pass against a live Postgres.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/journeys.rs
git commit -m "api: add the three POST /Journeys leg-creation paths (pin, knownTrain, window)"
```

---

## Task 7: `data/journeys.rs` — commit/match function (§2.3)

**Files:** modify `crates/api/src/data/journeys.rs`.

Depends on Task 5. This is the ONE function behind the `'manual'`-mode
commit route, reused unchanged for both a leg's first pick and any later
"Change train" re-pick, per the 2026-09-22 addendum.

- [ ] **Step 1: Add `set_leg_train_subscription`**:

```rust
/// Binds (or re-binds) a leg to a real train working -- the `manual`-mode
/// commit route's data function (design doc §2.3), reused UNCHANGED for
/// both a leg's first pick and any later "Change train" re-pick: this is
/// always a plain `UPDATE`, never a new `journey_legs` row, per the
/// 2026-09-22 addendum's explicit decision that the leg's OLD
/// `train_subscription_id` is simply orphaned from the leg once
/// overwritten -- left exactly as today's `delete_tracked_train`/re-pin
/// flows already leave an unreferenced row, no extra cleanup here. The
/// leg's `depart_*`/`arrive_*` window is deliberately left untouched by
/// this `UPDATE` -- it is what makes a later "Change train" possible at
/// all (design doc §1.1/§2.3).
///
/// Ownership-scoped via the same `journeys j` join `get_owned_leg` uses,
/// folded directly into the `UPDATE` (not re-derived from a prior read
/// alone) -- same paranoia as every other ownership-scoped write in this
/// codebase. Returns `true` if a row was updated, `false` for "no such
/// leg, or not this caller's" (the route maps this to `404`, never `403`).
pub async fn set_leg_train_subscription(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    train_subscription_id: i64,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'manual' \
         FROM journeys j \
         WHERE journey_legs.id = $2 AND journey_legs.journey_id = $3 \
           AND j.id = journey_legs.journey_id AND j.user_id = $4",
    )
    .bind(train_subscription_id)
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 2: Add DB-gated tests**, in the same `mod db_tests` Task 6
  created:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_binds_an_unmatched_leg_and_is_reusable_for_change_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        // First pick.
        let first_tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT")
                .await
                .expect("seed first candidate subscription");
        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT",
            first_tracking_id,
        )
        .await
        .expect("commit first pick");
        assert!(updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(first_tracking_id));
        assert_eq!(leg.match_mode, "manual");
        // The window survives the first commit -- this is what makes
        // "Change train" possible at all.
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));

        // "Change train" re-pick -- same route, same function, an UPDATE
        // not a new leg.
        let second_tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT")
                .await
                .expect("seed second candidate subscription");
        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT",
            second_tracking_id,
        )
        .await
        .expect("commit re-pick");
        assert!(updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(second_tracking_id));
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_a_non_owner_cannot_bind_someone_elses_leg() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OWNER").await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OTHER").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT-OWNER",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        let tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT-OTHER")
                .await
                .expect("seed candidate subscription");

        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT-OTHER",
            tracking_id,
        )
        .await
        .expect("attempt bind as non-owner");
        assert!(!updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT-OWNER")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, None);

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT-OWNER").await;
        cleanup_user(&pool, "TEST-JOURNEY-COMMIT-OTHER").await;
    }
```

- [ ] **Step 3: Verify**

```bash
cargo build -p api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api set_leg_train_subscription -- --ignored --test-threads=1
```

  Expected: both tests pass; the first proves the exact "reused unchanged
  for first pick AND re-pick" contract the 2026-09-22 addendum requires.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/journeys.rs
git commit -m "api: add set_leg_train_subscription, the manual-mode commit/re-pick function"
```

---

## Task 8: `data/journeys.rs` — list/detail read models + widen `routes/train.rs` visibility (§4)

**Files:** modify `crates/api/src/data/journeys.rs`,
`crates/api/src/routes/train.rs`.

Depends on Task 5. `GET /Journeys/{id}` needs to reuse
`TRACKED_TRAIN_STATE_SELECT` (via `train_tracking::get_by_tracking_id`, no
change needed there — already `pub`), plus `attach_journey_stops`/
`blend_darwin_eta` (currently private `async fn`s in `routes/train.rs`,
overlaying `journey_stops`/`eta_next` the same way `GET /Train/{trackingId}`
already does) and `enrich_shared_train` (same file, the best-effort
schedule-enrichment follow-up `post_track_by_uid` already makes) — all three
need `pub(crate)` visibility so `routes/journeys.rs` (Task 9) can call them.

- [ ] **Step 1: Widen three functions' visibility in `routes/train.rs`** —
  visibility only, no logic change:

```rust
// was: async fn enrich_shared_train(
pub(crate) async fn enrich_shared_train(
```

```rust
// was: async fn blend_darwin_eta(
pub(crate) async fn blend_darwin_eta(
```

```rust
// was: async fn attach_journey_stops(
pub(crate) async fn attach_journey_stops(
```

  (Leave `attach_journey_stops_public`/`get_delay_repay_estimate` and
  everything else in this file untouched — only these three signatures
  change, at their existing locations in the file.)

- [ ] **Step 2: Add the list read model** to `data/journeys.rs`:

```rust
/// One row of `GET /Journeys/mine` -- deliberately lighter than the full
/// `GET /Journeys/{id}` detail (`routes::journeys::JourneyDetailResponse`),
/// mirroring `TrackedTrainListItem`'s own "list is lighter than detail"
/// split. Phase 1 never creates more than one leg per journey (this
/// module's own doc comment), so this surfaces `leg_order = 1`'s own
/// fields directly rather than a nested array -- a genuine multi-leg
/// rollup (design doc §3's "worst status across legs" idiom) is a later
/// phase's job, once a journey can actually have more than one leg.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyListItem {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub leg_id: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub match_mode: String,
    pub train_subscription_id: Option<i64>,
    pub resolution_status: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
}

/// Most-recently-created journey first, capped at the same
/// `train_tracking::MINE_LIST_LIMIT` `GET /Train/mine` already uses --
/// `pub(crate)` on that constant already permits this cross-module read
/// (see its own doc comment, which anticipates exactly this: "any list
/// this list's own cap should agree with").
pub async fn list_journeys_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyListItem>> {
    let rows = sqlx::query_as::<_, JourneyListItem>(
        "SELECT j.id, j.custom_name, j.created_at, \
                jl.id AS leg_id, jl.origin_crs, jl.destination_crs, jl.match_mode, \
                jl.train_subscription_id, \
                ts.resolution_status, cs.status, cs.delay_minutes \
         FROM journeys j \
         JOIN journey_legs jl ON jl.journey_id = j.id AND jl.leg_order = 1 \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         WHERE j.user_id = $1 \
         ORDER BY j.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 3: Add the detail read model's row-level pieces** (the route
  layer, Task 12, assembles the final response — this data layer returns
  plain rows, matching `train_tracking::get_by_tracking_id`'s own "data
  layer returns a row, route layer overlays computed fields" split):

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneySummaryRow {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn get_owned_journey_summary(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneySummaryRow>> {
    let row = sqlx::query_as::<_, JourneySummaryRow>(
        "SELECT id, custom_name, created_at FROM journeys WHERE id = $1 AND user_id = $2",
    )
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every leg of a journey, `leg_order` ascending -- Phase 1 callers only
/// ever see one row (this module's own doc comment), but this is already
/// shaped for a later phase's longer result. Deliberately NOT
/// ownership-scoped on its own (unlike [`get_owned_leg`]) -- every real
/// caller (`routes::journeys::get_journey`, Task 12) already confirmed the
/// journey's ownership via [`get_owned_journey_summary`] one call earlier
/// in the same request, so re-checking here would be a redundant query,
/// not a real safety gain.
pub async fn list_legs_for_journey(pool: &PgPool, journey_id: i64) -> anyhow::Result<Vec<JourneyLegRow>> {
    let rows = sqlx::query_as::<_, JourneyLegRow>(
        "SELECT id, journey_id, origin_crs, destination_crs, service_date, \
                depart_after, depart_before, arrive_after, arrive_before, \
                train_subscription_id, match_mode \
         FROM journey_legs WHERE journey_id = $1 ORDER BY leg_order",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
```

  Expected: builds and lints clean (the three visibility widenings cannot
  break any existing caller — `pub(crate)` is strictly wider than the
  private default, and every existing call site is inside the same crate).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/journeys.rs crates/api/src/routes/train.rs
git commit -m "api: add journeys list/detail read models; widen train.rs helper visibility for reuse"
```

---

## Task 9: `routes/journeys.rs` — router + `POST /Journeys`

**Files:** create `crates/api/src/routes/journeys.rs`; modify
`crates/api/src/routes/mod.rs`, `crates/api/src/main.rs`,
`crates/api/src/routes/trains.rs` (visibility widening).

Depends on Task 6 (leg-creation functions) and Task 8 (visibility
widening). Also needs four items in `routes/trains.rs` widened from
private to `pub(crate)` for reuse by Task 10's candidates route:
`DEFAULT_SEARCH_LIMIT`, `MAX_SEARCH_LIMIT`, `normalize_limit`,
`encode_cursor`, `decode_cursor` — done here since this task is what first
needs the router file to exist at all.

- [ ] **Step 1: Widen five items in `routes/trains.rs`** — visibility only:

```rust
// was: const DEFAULT_SEARCH_LIMIT: i64 = 50;
pub(crate) const DEFAULT_SEARCH_LIMIT: i64 = 50;
```

```rust
// was: const MAX_SEARCH_LIMIT: i64 = 200;
pub(crate) const MAX_SEARCH_LIMIT: i64 = 200;
```

```rust
// was: fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
pub(crate) fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
```

```rust
// was: fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
pub(crate) fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
```

```rust
// was: fn decode_cursor(raw: &str) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
pub(crate) fn decode_cursor(raw: &str) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
```

- [ ] **Step 2: Register the module and mount the router**

```rust
// crates/api/src/routes/mod.rs -- add alongside the existing alphabetical list
pub mod journeys;
```

```rust
// crates/api/src/main.rs -- merged unprefixed, next to routes::train::router(),
// matching that route's own capitalized `/Train` -> `/Journeys` convention
// (both require an authenticated session; neither is nested under /public
// or /private).
let mut router = Router::new()
    .merge(routes::line_status::router())
    .merge(routes::train::router())
    .merge(routes::journeys::router())
    .nest("/public", routes::public_router())
    .nest("/private", routes::private_router(app.clone()));
```

- [ ] **Step 3: Create `crates/api/src/routes/journeys.rs`** with its
  module doc comment, router, and `POST /Journeys`:

```rust
//! `/Journeys/...`: journey tracking, Phase 1 (single-leg journeys +
//! manual-pick window search). See
//! docs/superpowers/specs/2026-09-22-journey-tracking-design.md and
//! docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md.
//! Every route here requires an authenticated session
//! (`AuthenticatedUser`) -- journeys have no anonymous/service-token path,
//! matching `routes::train`'s own posture for its write routes. Unlike
//! `routes::train::get_by_uid_and_date`, there is no public/unscoped
//! journey read in Phase 1 at all -- group sharing (design doc §6) is what
//! eventually opens a journey to anyone other than its own owner, and that
//! is Phase 4's job, not this file's.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::{journeys, schedule_matching, train_tracking};

pub fn router() -> Router {
    Router::new()
        .route("/Journeys", axum::routing::post(post_journey))
        .route("/Journeys/mine", axum::routing::get(get_my_journeys))
        .route("/Journeys/{journey_id}", axum::routing::get(get_journey))
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/candidates",
            axum::routing::get(get_leg_candidates),
        )
        .route(
            "/Journeys/{journey_id}/legs/{leg_id}/train",
            axum::routing::post(post_leg_train),
        )
}

/// Three mutually-exclusive leg-creation shapes, discriminated by an
/// explicit `mode` field on the wire (`"pin" | "knownTrain" | "window"`,
/// `#[serde(tag = "mode", rename_all = "camelCase")]`) rather than an
/// untagged enum -- an untagged enum's default serde error
/// ("data did not match any variant of untagged enum...") is exactly the
/// kind of internal, non-actionable message this codebase's
/// `validate_pin`/`validate_custom_name` family of user-facing errors
/// deliberately avoids; a missing/invalid `mode` instead gets serde's own
/// "unknown variant" message naming the three real, meaningful wire
/// values. See this plan's own Judgment Call 3 for why THREE shapes, not
/// two.
#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
enum CreateJourneyLegRequest {
    /// The legacy CRS+time GUESS pin, field-for-field identical to
    /// `common::TrackPinRequest` -- what `TrackTrainForm.tsx`'s existing
    /// "Pick a departure" / manual-entry flow submits (Task 15).
    Pin {
        origin_crs: String,
        scheduled_departure: DateTime<Utc>,
        service_date: NaiveDate,
        #[serde(default)]
        destination_crs: Option<String>,
        #[serde(default)]
        operator: Option<String>,
    },
    /// An already-known identity -- what `TrackThisTrainButton.tsx` submits
    /// (Task 14).
    KnownTrain {
        train_uid: String,
        service_date: NaiveDate,
    },
    /// An open time-window search -- no train chosen yet. Design doc §2.1;
    /// creates an `'unmatched'` leg the caller browses via
    /// `GET .../candidates` and commits via `POST .../train` (Task 16).
    Window {
        origin_crs: String,
        destination_crs: String,
        service_date: NaiveDate,
        #[serde(default)]
        depart_window: TimeWindow,
        #[serde(default)]
        arrive_window: TimeWindow,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyRequest {
    #[serde(default)]
    custom_name: Option<String>,
    leg: CreateJourneyLegRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyResponse {
    journey_id: i64,
    leg_id: i64,
    /// `None` only for a `window`-mode leg -- no train is bound yet.
    tracking_id: Option<i64>,
    /// Mirrors `train::TrackPinResponse::resolution_status` for a
    /// `pin`-mode leg; `None` for `knownTrain`/`window` modes, neither of
    /// which has an equivalent synchronous-match-attempt outcome to report
    /// (a `knownTrain` leg is resolved eagerly, immediately, below; a
    /// `window` leg has no train at all yet).
    resolution_status: Option<&'static str>,
}

async fn post_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<CreateJourneyRequest>,
) -> Result<Json<CreateJourneyResponse>, (StatusCode, String)> {
    match body.leg {
        CreateJourneyLegRequest::Pin {
            origin_crs,
            scheduled_departure,
            service_date,
            destination_crs,
            operator,
        } => {
            let pin = common::TrackPinRequest {
                service_date,
                origin_crs,
                scheduled_departure,
                destination_crs,
                operator,
            };
            train_tracking::validate_pin(&pin, Utc::now())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

            let (journey_id, leg_id, tracking_id) = journeys::create_journey_with_pin_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                &pin,
            )
            .await
            .map_err(internal_error("create journey (pin leg)"))?;

            // Same best-effort synchronous match attempts
            // `routes::train::post_track` already makes for the exact same
            // `create_pin` call -- kept in lockstep so a pin-mode journey
            // resolves exactly as fast as a legacy bare pin would have
            // (design doc §7.2: "the literal same code path from this
            // point on").
            let resolution_status = match schedule_matching::attempt_schedule_match(
                &app.database,
                tracking_id,
                &pin.origin_crs,
                pin.scheduled_departure,
                pin.service_date,
                &app.schedule_crs_line_index,
            )
            .await
            {
                Ok(true) => "schedule_matched",
                Ok(false) => "pending",
                Err(err) => {
                    tracing::warn!(
                        error = ?err,
                        tracking_id,
                        "schedule match attempt failed at journey creation; leg stays pending"
                    );
                    "pending"
                }
            };
            if let Err(err) = crate::data::trust_event_backlog_match::attempt_backlog_match(
                &app.database,
                tracking_id,
                &pin.origin_crs,
                pin.scheduled_departure,
                pin.service_date,
            )
            .await
            {
                tracing::warn!(error = ?err, tracking_id, "backlog match attempt failed; leg remains pending");
            }

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: Some(tracking_id),
                resolution_status: Some(resolution_status),
            }))
        }
        CreateJourneyLegRequest::KnownTrain {
            train_uid,
            service_date,
        } => {
            let trains_id =
                crate::data::trains::find_or_create_train(&app.database, &train_uid, service_date)
                    .await
                    .map_err(internal_error("find or create train"))?;
            let (journey_id, leg_id, tracking_id) = journeys::create_journey_with_known_train_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                trains_id,
                service_date,
            )
            .await
            .map_err(internal_error("create journey (known-train leg)"))?;

            // Same best-effort enrichment `routes::train::post_track_by_uid`
            // already makes for the exact same `create_subscription_for_train`
            // call.
            crate::routes::train::enrich_shared_train(
                &app,
                tracking_id,
                trains_id,
                &train_uid,
                service_date,
            )
            .await;

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: Some(tracking_id),
                resolution_status: None,
            }))
        }
        CreateJourneyLegRequest::Window {
            origin_crs,
            destination_crs,
            service_date,
            depart_window,
            arrive_window,
        } => {
            journeys::validate_window_leg(&origin_crs, &destination_crs, &depart_window, &arrive_window)
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

            let (journey_id, leg_id) = journeys::create_journey_with_window_leg(
                &app.database,
                &user.id,
                body.custom_name.as_deref(),
                &origin_crs.trim().to_ascii_uppercase(),
                &destination_crs.trim().to_ascii_uppercase(),
                service_date,
                depart_window,
                arrive_window,
            )
            .await
            .map_err(internal_error("create journey (window leg)"))?;

            Ok(Json(CreateJourneyResponse {
                journey_id,
                leg_id,
                tracking_id: None,
                resolution_status: None,
            }))
        }
    }
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "journey request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to {operation}"))
    }
}
```

  (`get_my_journeys`, `get_journey`, `get_leg_candidates`, `post_leg_train`
  are added by Tasks 10–12 below — this task's own `router()` already
  routes to all four names so the file compiles once every task in this
  group lands; if executing tasks strictly in order, the crate will not
  compile again until Task 12 finishes, mirroring how the custom-tracking-
  names reference plan's own Tasks 3–4 accept a temporarily-broken build
  between dependent steps.)

- [ ] **Step 4: Verify**

```bash
cargo build -p api 2>&1 | tail -30
```

  Expected: **does not compile yet** — `router()` references
  `get_my_journeys`/`get_journey`/`get_leg_candidates`/`post_leg_train`,
  none of which exist until Tasks 10–12. This is expected; proceed.

- [ ] **Step 5: Commit** (once Tasks 10–12 make the crate compile again —
  this task's diff lands as its own commit regardless, in the order
  written, same convention the custom-tracking-names reference plan uses)

```bash
git add crates/api/src/routes/journeys.rs crates/api/src/routes/mod.rs crates/api/src/main.rs crates/api/src/routes/trains.rs
git commit -m "api: add routes::journeys router + POST /Journeys (pin/knownTrain/window)"
```

---

## Task 10: `routes/journeys.rs` — `GET .../candidates`

**Files:** modify `crates/api/src/routes/journeys.rs`.

Depends on Task 4 (`search_journey_leg_candidates`), Task 5
(`get_owned_leg`), and Task 9's visibility widening
(`normalize_limit`/`decode_cursor`/`encode_cursor` in `routes/trains.rs`).

- [ ] **Step 1: Add `get_leg_candidates`**:

```rust
#[derive(Debug, Deserialize)]
struct CandidatesParams {
    limit: Option<String>,
    after: Option<String>,
}

/// `GET /Journeys/{journeyId}/legs/{legId}/candidates` -- design doc §2.2.
/// Runs [`crate::data::queries::search_journey_leg_candidates`] against the
/// leg's own persisted `origin_crs`/`destination_crs`/`depart_*`/
/// `arrive_*`, returns the exact same envelope shape
/// `GET /public/trains/search` already returns (`{results, nextCursor}`,
/// reusing `render::calling_point_departure_json` verbatim) -- no new DTO,
/// per the design doc's own explicit direction.
async fn get_leg_candidates(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    Query(params): Query<CandidatesParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let leg = journeys::get_owned_leg(&app.database, journey_id, leg_id, &user.id)
        .await
        .map_err(internal_error("read journey leg"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey leg with that id".to_string()))?;

    let (Some(origin_crs), Some(destination_crs)) =
        (leg.origin_crs.as_deref(), leg.destination_crs.as_deref())
    else {
        // Only reachable for a `pin`/`knownTrain`-mode leg whose underlying
        // train has no schedule data yet -- see this plan's own Judgment
        // Call 1. Such a leg was never window-searched and has nothing to
        // browse candidates for.
        return Err((
            StatusCode::BAD_REQUEST,
            "this leg has no origin/destination to search candidates for".to_string(),
        ));
    };

    let limit = crate::routes::trains::normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(crate::routes::trains::decode_cursor)
        .transpose()?;

    let Some(page) = crate::data::queries::search_journey_leg_candidates(
        &app.database,
        origin_crs,
        destination_crs,
        leg.service_date,
        leg.depart_after,
        leg.depart_before,
        leg.arrive_after,
        leg.arrive_before,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error("search journey leg candidates"))?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for this leg's service date"
                .to_string(),
        ));
    };

    Ok(Json(serde_json::json!({
        "results": page
            .departures
            .iter()
            .map(|row| crate::render::calling_point_departure_json(row, origin_crs))
            .collect::<Vec<serde_json::Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(crate::routes::trains::encode_cursor),
    })))
}
```

  Add `NaiveTime`/`Path`/`Query` imports if not already present from Task
  9's Step 3 (`chrono::{DateTime, NaiveDate, NaiveTime, Utc}`,
  `axum::extract::{Path, Query, State}` — both already listed in this
  file's `use` block from Task 9).

- [ ] **Step 2: Add HTTP-layer `db_tests`**, in a new `#[cfg(test)] mod
  db_tests` at the bottom of `routes/journeys.rs`, copying
  `routes/train.rs`'s own `test_app`/`test_router`/`seed_session`/
  `cleanup_user`/`connect`/`request`/`post_json` helpers verbatim (same
  cross-file duplication convention Task 6 already follows for the data
  layer):

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_leg_candidates -- --ignored --test-threads=1`"]
    async fn get_leg_candidates_a_non_owner_gets_404() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-CANDIDATES-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-CANDIDATES-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, _) = request(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/candidates"),
            Some(&bystander_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, "TEST-ROUTE-CANDIDATES-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-CANDIDATES-BYSTANDER").await;
    }
```

  (A `request` helper doesn't currently exist for a GET-with-query-params
  case in `routes/train.rs`'s own scaffolding — its own `request` helper
  already takes a plain `uri: String`, sufficient here since the test above
  passes no query params; reuse it verbatim.)

- [ ] **Step 3: Verify**

```bash
cargo build -p api 2>&1 | tail -30
```

  Expected: **still does not compile** — `router()` (Task 9) still
  references `get_my_journeys`/`get_journey`/`post_leg_train`, added by
  Tasks 11–12. Confirm this task's own new code has no syntax/type errors
  by checking the compiler output names only those two missing functions,
  nothing from this task.

- [ ] **Step 4: Commit** (once Tasks 11–12 land, same "diff lands now,
  crate compiles once the group is done" convention as Task 9)

```bash
git add crates/api/src/routes/journeys.rs
git commit -m "api: add GET /Journeys/{id}/legs/{id}/candidates"
```

---

## Task 11: `routes/journeys.rs` — `POST .../train` (commit/re-pick, §2.3)

**Files:** modify `crates/api/src/routes/journeys.rs`.

Depends on Task 7 (`set_leg_train_subscription`) and Task 5
(`get_owned_leg`).

- [ ] **Step 1: Add `post_leg_train`**:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchLegRequest {
    train_uid: String,
    service_date: NaiveDate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchLegResponse {
    tracking_id: i64,
}

/// `POST /Journeys/{journeyId}/legs/{legId}/train` -- binds (or re-binds)
/// a leg to a real train, `'manual'` mode only (design doc §2.3). The SAME
/// route handles a leg's first pick (from `GET .../candidates`, Task 10)
/// and any later "Change train" re-pick -- it is always an `UPDATE`, never
/// a new leg (see `journeys::set_leg_train_subscription`'s own doc
/// comment).
async fn post_leg_train(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((journey_id, leg_id)): Path<(i64, i64)>,
    Json(body): Json<MatchLegRequest>,
) -> Result<Json<MatchLegResponse>, (StatusCode, String)> {
    // Ownership-checked read first -- same 404-never-403 posture as every
    // other route in this crate. Also confirms the leg exists before this
    // conjures a `trains`/`train_subscriptions` row for it.
    journeys::get_owned_leg(&app.database, journey_id, leg_id, &user.id)
        .await
        .map_err(internal_error("read journey leg"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey leg with that id".to_string()))?;

    let trains_id =
        crate::data::trains::find_or_create_train(&app.database, &body.train_uid, body.service_date)
            .await
            .map_err(internal_error("find or create train"))?;
    let tracking_id =
        train_tracking::create_subscription_for_train(&app.database, trains_id, &user.id)
            .await
            .map_err(internal_error("create subscription"))?;

    crate::routes::train::enrich_shared_train(
        &app,
        tracking_id,
        trains_id,
        &body.train_uid,
        body.service_date,
    )
    .await;

    let updated =
        journeys::set_leg_train_subscription(&app.database, journey_id, leg_id, &user.id, tracking_id)
            .await
            .map_err(internal_error("set journey leg train"))?;
    if !updated {
        // Lost a race against a concurrent deletion of the underlying leg
        // between the read above and this write -- vanishingly unlikely
        // (no delete-journey route exists in Phase 1 at all, per this
        // plan's own Non-goals), but handled rather than silently ignored,
        // matching `post_attach_ticket`'s own analogous race-handling
        // posture in `routes/train.rs`.
        return Err((
            StatusCode::NOT_FOUND,
            "no journey leg with that id".to_string(),
        ));
    }

    Ok(Json(MatchLegResponse { tracking_id }))
}
```

- [ ] **Step 2: Add HTTP-layer `db_tests`**, in the same `mod db_tests`
  Task 10 created:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_leg_train -- --ignored --test-threads=1`"]
    async fn post_leg_train_commits_a_first_pick_then_a_change_train_repick() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, body) = post_json(
            router.clone(),
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "A11111", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "first pick: {body:?}");
        let first_tracking_id = body["trackingId"].as_i64().expect("trackingId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&token),
            serde_json::json!({ "trainUid": "A22222", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "change train: {body:?}");
        let second_tracking_id = body["trackingId"].as_i64().expect("trackingId present");
        assert_ne!(first_tracking_id, second_tracking_id);

        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_leg_train -- --ignored --test-threads=1`"]
    async fn post_leg_train_a_leg_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-MATCH-LEG-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": {
                    "mode": "window",
                    "originCrs": "WAT",
                    "destinationCrs": "RDG",
                    "serviceDate": "2026-09-22",
                    "departWindow": { "after": "08:00:00" }
                }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");
        let leg_id = created["legId"].as_i64().expect("legId present");

        let (status, body) = post_json(
            router,
            format!("/Journeys/{journey_id}/legs/{leg_id}/train"),
            Some(&bystander_token),
            serde_json::json!({ "trainUid": "A33333", "serviceDate": "2026-09-22" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, serde_json::Value::String("no journey leg with that id".to_string()));

        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-MATCH-LEG-BYSTANDER").await;
    }
```

- [ ] **Step 3: Verify**

```bash
cargo build -p api 2>&1 | tail -30
```

  Expected: still not fully linking — `router()` still references
  `get_my_journeys`/`get_journey`, added by Task 12. Confirm the compiler
  output names only those two, nothing from this task or Task 10.

- [ ] **Step 4: Commit** (once Task 12 lands)

```bash
git add crates/api/src/routes/journeys.rs
git commit -m "api: add POST /Journeys/{id}/legs/{id}/train (manual commit, reused for Change train)"
```

---

## Task 12: `routes/journeys.rs` — `GET /Journeys/mine`, `GET /Journeys/{id}`

**Files:** modify `crates/api/src/routes/journeys.rs`.

Depends on Task 8 (list/detail read models, widened `attach_journey_stops`/
`blend_darwin_eta`). This is the task that finally makes the whole crate
compile again — run the full test suite for Tasks 9–12 together at the end
of this task's own Verify step.

- [ ] **Step 1: Add `get_my_journeys`**:

```rust
async fn get_my_journeys(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<journeys::JourneyListItem>>, (StatusCode, String)> {
    let rows = journeys::list_journeys_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list journeys"))?;
    Ok(Json(rows))
}
```

- [ ] **Step 2: Add `get_journey`**:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyDetailResponse {
    id: i64,
    custom_name: Option<String>,
    created_at: DateTime<Utc>,
    legs: Vec<JourneyLegDetailResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyLegDetailResponse {
    id: i64,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
    service_date: NaiveDate,
    depart_after: Option<NaiveTime>,
    depart_before: Option<NaiveTime>,
    arrive_after: Option<NaiveTime>,
    arrive_before: Option<NaiveTime>,
    match_mode: String,
    /// `Some` once a train is bound -- the EXACT same shape
    /// `GET /Train/{trackingId}` returns
    /// (`train_tracking::TRACKED_TRAIN_STATE_SELECT`, `attach_journey_stops`,
    /// `blend_darwin_eta`, all reused unchanged; design doc §4). `None`
    /// for an unmatched (`train_subscription_id IS NULL`) leg.
    tracked_train_state: Option<train_tracking::TrackedTrainState>,
}

/// `GET /Journeys/{journeyId}` -- design doc §4. No new backend read-model
/// query beyond joining straight into the existing
/// `TRACKED_TRAIN_STATE_SELECT` per matched leg, exactly as the design doc
/// itself specifies: "the wire payload for a matched leg is exactly
/// today's `TrackedTrainState` shape, unchanged."
async fn get_journey(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(journey_id): Path<i64>,
) -> Result<Json<JourneyDetailResponse>, (StatusCode, String)> {
    let summary = journeys::get_owned_journey_summary(&app.database, journey_id, &user.id)
        .await
        .map_err(internal_error("read journey"))?
        .ok_or((StatusCode::NOT_FOUND, "no journey with that id".to_string()))?;

    let leg_rows = journeys::list_legs_for_journey(&app.database, journey_id)
        .await
        .map_err(internal_error("list journey legs"))?;

    let mut legs = Vec::with_capacity(leg_rows.len());
    for leg in leg_rows {
        let tracked_train_state = match leg.train_subscription_id {
            Some(tracking_id) => {
                match train_tracking::get_by_tracking_id(&app.database, tracking_id)
                    .await
                    .map_err(internal_error("read tracked train state"))?
                {
                    Some(state) => Some(
                        crate::routes::train::attach_journey_stops(
                            &app,
                            crate::routes::train::blend_darwin_eta(&app, state).await,
                        )
                        .await,
                    ),
                    None => None,
                }
            }
            None => None,
        };
        legs.push(JourneyLegDetailResponse {
            id: leg.id,
            origin_crs: leg.origin_crs,
            destination_crs: leg.destination_crs,
            service_date: leg.service_date,
            depart_after: leg.depart_after,
            depart_before: leg.depart_before,
            arrive_after: leg.arrive_after,
            arrive_before: leg.arrive_before,
            match_mode: leg.match_mode,
            tracked_train_state,
        });
    }

    Ok(Json(JourneyDetailResponse {
        id: summary.id,
        custom_name: summary.custom_name,
        created_at: summary.created_at,
        legs,
    }))
}
```

- [ ] **Step 3: Add HTTP-layer `db_tests`**, in the same `mod db_tests`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey -- --ignored --test-threads=1`"]
    async fn get_journey_returns_the_matched_legs_tracked_train_state() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A44444", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, body) = request(router, format!("/Journeys/{journey_id}"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK, "get journey: {body:?}");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["matchMode"], "manual");
        assert!(legs[0]["trackedTrainState"].is_object());
        assert_eq!(legs[0]["trackedTrainState"]["trainUid"], "A44444");

        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey -- --ignored --test-threads=1`"]
    async fn get_journey_a_journey_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-OWNER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-GET-JOURNEY-BYSTANDER").await;
        let router = test_router(test_app(pool.clone()));

        let (_, created) = post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "leg": { "mode": "knownTrain", "trainUid": "A55555", "serviceDate": "2026-09-22" }
            }),
        )
        .await;
        let journey_id = created["journeyId"].as_i64().expect("journeyId present");

        let (status, _) = request(router, format!("/Journeys/{journey_id}"), Some(&bystander_token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-OWNER").await;
        cleanup_user(&pool, "TEST-ROUTE-GET-JOURNEY-BYSTANDER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_my_journeys -- --ignored --test-threads=1`"]
    async fn get_my_journeys_lists_every_owned_journey_most_recent_first() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-ROUTE-MY-JOURNEYS").await;
        let router = test_router(test_app(pool.clone()));

        post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({ "leg": { "mode": "knownTrain", "trainUid": "A66666", "serviceDate": "2026-09-22" } }),
        )
        .await;
        post_json(
            router.clone(),
            "/Journeys".to_string(),
            Some(&token),
            serde_json::json!({ "leg": { "mode": "knownTrain", "trainUid": "A77777", "serviceDate": "2026-09-22" } }),
        )
        .await;

        let (status, body) = request(router, "/Journeys/mine".to_string(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("array response");
        assert_eq!(rows.len(), 2);

        cleanup_user(&pool, "TEST-ROUTE-MY-JOURNEYS").await;
    }
```

- [ ] **Step 4: Verify — the whole `routes::journeys` module, now that the
  crate compiles again**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres sqlx migrate run --source crates/api/migrations
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api -- --ignored --test-threads=1
```

  Expected: builds and lints clean; every `#[ignore]`d test added across
  Tasks 4, 6, 7, 10, 11, and this task passes (run the full `-p api
  --ignored` sweep here, not just this task's own tests, since this is the
  first point the whole `routes::journeys` module compiles and links).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/journeys.rs
git commit -m "api: add GET /Journeys/mine and GET /Journeys/{id}"
```

---

## Task 13: Frontend — wire types + server fetchers

**Files:** modify `frontend/lib/types.ts`, `frontend/lib/api.ts`.

Depends on nothing else in this plan's frontend tasks (pure type/fetcher
additions); depends on Task 12 being deployed/available for real manual
verification later, but compiles and typechecks against nothing but its own
shape.

- [ ] **Step 1: Add wire types** to `frontend/lib/types.ts`, near the
  existing `TrackedTrainState`/`TrackedTrainListItem` block:

```typescript
/** `common::TimeWindow` on the wire -- `crates/common/src/lib.rs`. Both
 * fields `"HH:MM:SS" | null`. */
export interface TimeWindow {
  after: string | null;
  before: string | null;
}

/** One row of `GET /Journeys/mine`
 * (`crates/api/src/data/journeys.rs::JourneyListItem`, camelCase).
 * Deliberately lighter than `JourneyDetail` below -- see that Rust
 * struct's own doc comment for why. */
export interface JourneyListItem {
  id: number;
  customName: string | null;
  createdAt: string;
  legId: number;
  originCrs: string | null;
  destinationCrs: string | null;
  matchMode: 'unmatched' | 'manual' | 'auto';
  trainSubscriptionId: number | null;
  resolutionStatus: string | null;
  status: string | null;
  delayMinutes: number | null;
}

/** One leg of `GET /Journeys/{id}`'s response
 * (`crates/api/src/routes/journeys.rs::JourneyLegDetailResponse`).
 * `trackedTrainState` is `null` for an unmatched leg, and otherwise the
 * EXACT SAME shape `GET /Train/{trackingId}` returns -- `TrackedTrainState`
 * is reused verbatim, not a narrower/different type. */
export interface JourneyLegDetail {
  id: number;
  originCrs: string | null;
  destinationCrs: string | null;
  serviceDate: string;
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
  matchMode: 'unmatched' | 'manual' | 'auto';
  trackedTrainState: TrackedTrainState | null;
}

/** `GET /Journeys/{id}`'s full response. */
export interface JourneyDetail {
  id: number;
  customName: string | null;
  createdAt: string;
  legs: JourneyLegDetail[];
}

/** `POST /Journeys`'s response
 * (`crates/api/src/routes/journeys.rs::CreateJourneyResponse`). */
export interface CreateJourneyResponse {
  journeyId: number;
  legId: number;
  trackingId: number | null;
  resolutionStatus: string | null;
}
```

- [ ] **Step 2: Add server fetchers** to `frontend/lib/api.ts`, near
  `getTrackedTrainById` (mirroring its exact `ApiNotFoundError`/
  `ApiUnauthorizedError` convention — see that function's own doc comment
  for why a dedicated "log in, this might be yours" prompt is warranted
  here too, same reasoning: a journey detail page "has no public sibling
  content to fall back to"):

```typescript
/** `GET /Journeys/{id}` -- same error-mapping contract as
 * `getTrackedTrainById` immediately above: throws `ApiNotFoundError` on a
 * 404 (doesn't exist, or isn't this caller's -- indistinguishable, per
 * this app's 404-never-403 convention) and `ApiUnauthorizedError` on a
 * 401 (not logged in at all) via `errorForResponse`, so
 * `app/journeys/[id]/page.tsx` can render the same two distinct page
 * states `app/train/by-id/[trackingId]/page.tsx` already does. */
export async function getJourney(id: number): Promise<JourneyDetail> {
  const url = `${baseUrl()}/Journeys/${id}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyDetail>;
}

/** `GET /Journeys/mine` -- `null` on a `401`, same "not logged in" signal
 * `getMyTrackedTrains` already uses (no id in this route's path to
 * disambiguate a second way). Not consumed by any page in this plan (see
 * this plan's own Non-goals: no `/journeys/mine` list page yet) --
 * implemented now, independently testable, for a follow-up list page to
 * consume later without a backend change. */
export async function getMyJourneys(): Promise<JourneyListItem[] | null> {
  const url = `${baseUrl()}/Journeys/mine`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyListItem[]>;
}
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: no new type errors (both functions are additive; `getJourney`
  follows `getTrackedTrainById`'s exact existing signature shape).

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts
git commit -m "frontend: add Journey wire types and getJourney/getMyJourneys fetchers"
```

---

## Task 14: Frontend — `TrackThisTrainButton.tsx` rewired to `POST /Journeys`

**Files:** modify `frontend/components/TrackThisTrainButton.tsx`.

Depends on Task 9 (the `POST /Journeys` route must exist for real manual
verification, though this task's own automated tests can run against a
mocked `fetch`). This is the smaller of the two rewiring tasks — the
button's request has no body today and gets one; its two call sites
(`/trains` search results via `TrainSearchForm.tsx`, and
`/train/[uid]/[date]`'s own CTA) both pick up the change automatically with
no edit to either of those files, per this plan's own Non-goals.

- [ ] **Step 1: Change the `track()` function's request** (currently
  `fetch(\`/api/Train/by-uid/${uid}/${date}/track\`, { method: 'POST' })`,
  no body):

```typescript
  async function track(groupId: string | null) {
    setBusy(true);
    needsLoginState.reset();
    setError(null);
    try {
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: uid, serviceDate: date },
        }),
      });

      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort, exactly as before -- tracking has already
          // succeeded above. Still keyed on `trackingId`, not the new
          // `journeyId`: ticket attachment is scoped to the underlying
          // `train_subscriptions` row, unchanged by this feature.
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed.
          }
        }
        if (groupId !== null && result.trackingId !== null) {
          // Best-effort, same posture as the ticket-attach block above.
          // `result.trackingId` is always non-null here -- `knownTrain`
          // mode always binds a train immediately -- but the check keeps
          // this call site honest against `CreateJourneyResponse`'s wider
          // (window-mode-inclusive) type. Still shares into the group via
          // the EXISTING group_trains table, unchanged -- journey-level
          // sharing is Phase 4 (see this plan's own Non-goals).
          await shareTrackedTrainToGroup(groupId, result.trackingId);
        }
        router.push(`/journeys/${result.journeyId}`);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      setError("Couldn't track this train. Try again.");
    } catch {
      setError("Couldn't track this train. Try again.");
    } finally {
      setBusy(false);
    }
  }
```

- [ ] **Step 2: Add the import**

```typescript
import type { CreateJourneyResponse } from '@/lib/types';
```

- [ ] **Step 3: Update this component's existing test file**
  (`TrackThisTrainButton.test.tsx`, if one exists — check via `ls
  frontend/components/TrackThisTrainButton.test.tsx`; if present, update
  its mocked-`fetch` assertions to expect `POST /api/Journeys` with the new
  body shape and a `{journeyId, trackingId}`-shaped mock response instead
  of `POST /api/Train/by-uid/.../track` with a bare `{trackingId}`
  response, and its `router.push` assertion to expect
  `/journeys/{journeyId}` instead of `/train/by-id/{trackingId}`).

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- TrackThisTrainButton
npx tsc --noEmit
```

  Expected: updated tests pass; no new type errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrackThisTrainButton.tsx frontend/components/TrackThisTrainButton.test.tsx
git commit -m "frontend: rewire TrackThisTrainButton onto POST /Journeys (knownTrain mode)"
```

---

## Task 15: Frontend — `TrackTrainForm.tsx` rewired + window-search mode

**Files:** modify `frontend/components/TrackTrainForm.tsx`.

Depends on Task 9. The larger of the two rewiring tasks: the existing
"pick a departure / enter by hand" flow moves onto `POST /Journeys` in
`pin` mode, and a new `SegmentedControl` adds a second mode — an open
window search — reusing `TimeFilterInput`'s existing before/after
convention verbatim, per the spec's own explicit direction (§0.5: "the UI
pattern §2/§3 should reuse verbatim for leg-window entry, not invent a new
one").

- [ ] **Step 1: Change `submitTrack`'s request body and success handling**
  (currently posts `TrackPinRequest` to `/api/Train/track` and expects a
  bare `TrackPinResponse`):

```typescript
      const body = {
        customName: null,
        leg: {
          mode: 'pin' as const,
          originCrs: originCrs.trim().toUpperCase(),
          scheduledDeparture: departure.toISOString(),
          serviceDate,
          ...(destinationCrs.trim() ? { destinationCrs: destinationCrs.trim().toUpperCase() } : {}),
          ...(operator.trim() ? { operator: operator.trim() } : {}),
        },
      };

      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        if (attachTicketId !== undefined && result.trackingId !== null) {
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed -- see this block's own comment above.
          }
        }
        if (groupId !== null && result.trackingId !== null) {
          // Still shares the underlying train_subscriptions row via the
          // EXISTING group_trains table, unchanged -- journey-level
          // sharing is Phase 4 (see this plan's own Non-goals).
          await shareTrackedTrainToGroup(groupId, result.trackingId);
        }
        router.push(`/journeys/${result.journeyId}`);
        return;
      }
```

  Remove the now-unused `import type { TrackPinRequest, TrackPinResponse }
  from '@/lib/types';` and add `import type { CreateJourneyResponse } from
  '@/lib/types';`.

- [ ] **Step 2: Add window-search mode state and a `submitWindow`
  function**, alongside the existing `submitTrack`:

```typescript
  const [mode, setMode] = useState<'pick' | 'window'>('pick');
  const [windowDestinationCrs, setWindowDestinationCrs] = useState('');
  const [windowServiceDate, setWindowServiceDate] = useState<string | null>(null);
  const [departFrom, setDepartFrom] = useState('');
  const [departTo, setDepartTo] = useState('');
  const [arriveFrom, setArriveFrom] = useState('');
  const [arriveTo, setArriveTo] = useState('');
  // Same half-entered-time bookkeeping TrainSearchForm.tsx's own four
  // TimeFilterInput fields already need -- see that component's own
  // `incompleteTimes` doc comment for the full reasoning (a native
  // `<input type="time">` reports a half-entered value as `''`,
  // indistinguishable from untouched).
  const [windowIncompleteTimes, setWindowIncompleteTimes] = useState({
    departFrom: false,
    departTo: false,
    arriveFrom: false,
    arriveTo: false,
  });
  const windowDestinationValid = CRS_PATTERN.test(windowDestinationCrs.trim());
  const windowTimesComplete = !Object.values(windowIncompleteTimes).some(Boolean);
  const windowHasABound =
    departFrom.trim() !== '' || departTo.trim() !== '' || arriveFrom.trim() !== '' || arriveTo.trim() !== '';
  const canSubmitWindow =
    originValid && windowDestinationValid && windowTimesComplete && windowHasABound && !submitting;

  async function submitWindow(groupId: string | null) {
    if (!canSubmitWindow) return;
    setSubmitting(true);
    needsLoginState.reset();
    setFieldError(null);
    try {
      const body = {
        customName: null,
        leg: {
          mode: 'window' as const,
          originCrs: originCrs.trim().toUpperCase(),
          destinationCrs: windowDestinationCrs.trim().toUpperCase(),
          serviceDate: windowServiceDate ?? dayjs().format('YYYY-MM-DD'),
          departWindow: { after: departFrom || null, before: departTo || null },
          arriveWindow: { after: arriveFrom || null, before: arriveTo || null },
        },
      };
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        // No trackingId yet (an open leg has no train bound) -- so no
        // ticket-attach or group-share follow-up is possible here, unlike
        // submitTrack/pin mode. The journey view itself (Task 17) is where
        // a candidate gets picked next.
        void groupId; // reserved for a future group-share-on-window-search follow-up
        router.push(`/journeys/${result.journeyId}`);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        const text = await response.text();
        setFieldError(text || "Couldn't search for a train. Try again.");
        return;
      }
      setFieldError("Couldn't search for a train. Try again.");
    } catch {
      setFieldError("Couldn't search for a train. Try again.");
    } finally {
      setSubmitting(false);
    }
  }
```

  (`void groupId;` above is a deliberate placeholder — group-sharing an
  open/unmatched leg at creation time has no real precedent yet, since
  there is no train to share, and this plan's Non-goals explicitly defer
  journey-level group sharing to Phase 4; the parameter is kept, unused,
  purely so `handleSubmit`'s dispatch to either `submitTrack`/`submitWindow`
  can share one call shape without a branch on arity.)

- [ ] **Step 3: Branch `handleSubmit` on `mode`**, and add the
  `SegmentedControl` + conditional window fields to the JSX, importing
  `SegmentedControl` and `TimeFilterInput`:

```typescript
import { SegmentedControl } from '@mantine/core';
import { TimeFilterInput } from './TimeFilterInput';
```

```typescript
  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (submitting) return;
    if (mode === 'window') {
      if (!originValid || !windowDestinationValid || !windowHasABound) {
        setFieldError(
          !originValid || !windowDestinationValid
            ? 'Enter a valid origin and destination station before searching.'
            : 'Enter at least one earliest/latest departure or arrival time to search a window.',
        );
        return;
      }
      setFieldError(null);
      if (groups.length > 0) {
        setDestinationPromptOpened(true);
        return;
      }
      void submitWindow(null);
      return;
    }
    if (!originValid || scheduledDeparture === null) {
      setFieldError(
        !originValid
          ? 'Enter a valid origin station before tracking — pick one from the suggestions, or a 3-letter CRS code.'
          : 'Pick a scheduled departure before tracking.',
      );
      return;
    }
    setFieldError(null);
    if (groups.length > 0) {
      setDestinationPromptOpened(true);
      return;
    }
    void submitTrack(null);
  }
```

```tsx
      <SegmentedControl
        value={mode}
        onChange={(value) => setMode(value as 'pick' | 'window')}
        data={[
          { label: 'Pick a departure', value: 'pick' },
          { label: 'Search a time window', value: 'window' },
        ]}
      />
      {mode === 'window' ? (
        <>
          <Autocomplete
            label="Destination station"
            placeholder="e.g. Reading or RDG"
            value={windowDestinationCrs}
            onChange={setWindowDestinationCrs}
            data={withNoMatchPlaceholder(
              destinationSuggestions.map((s) => ({ value: s.code, label: s.code })),
              'No matching stations',
              { active: windowDestinationCrs.trim().length > 0 && !destinationSuggestionsLoading },
            )}
            filter={({ options }) => options}
            error={
              windowDestinationCrs.length > 0 && !windowDestinationValid
                ? 'Must be a 3-letter CRS code'
                : null
            }
            required
          />
          <DatePickerInput
            label="Date"
            placeholder="Today"
            value={windowServiceDate}
            onChange={setWindowServiceDate}
            clearable
          />
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest departure (optional)"
              name="earliest departure"
              description={`Only trains at ${originValid ? originCrs.trim().toUpperCase() : 'the origin above'} at or after this time.`}
              value={departFrom}
              onChange={setDepartFrom}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departFrom: v }))}
              error={null}
            />
            <TimeFilterInput
              label="Latest departure (optional)"
              name="latest departure"
              description="Only trains at or before this time."
              value={departTo}
              onChange={setDepartTo}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departTo: v }))}
              error={null}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name="earliest arrival"
              description="Only trains reaching the destination at or after this time."
              value={arriveFrom}
              onChange={setArriveFrom}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveFrom: v }))}
              error={null}
            />
            <TimeFilterInput
              label="Latest arrival (optional)"
              name="latest arrival"
              description="Only trains reaching the destination at or before this time."
              value={arriveTo}
              onChange={setArriveTo}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveTo: v }))}
              error={null}
            />
          </Group>
        </>
      ) : (
        // ... the existing DateTimePicker/"Now" button/Destination/Operator/
        // picker JSX, unchanged, exactly as it renders today.
      )}
```

  Also update `TrackDestinationModal`'s `onConfirm` to dispatch to whichever
  of `submitTrack`/`submitWindow` matches the current `mode`:

```tsx
      <TrackDestinationModal
        opened={destinationPromptOpened}
        groups={groups}
        onClose={() => setDestinationPromptOpened(false)}
        onConfirm={(groupId) => void (mode === 'window' ? submitWindow(groupId) : submitTrack(groupId))}
      />
```

- [ ] **Step 4: Update this component's existing test file**
  (`TrackTrainForm.test.tsx`, if one exists — check via `ls
  frontend/components/TrackTrainForm.test.tsx`). Update every existing
  `pin`-mode test's mocked-`fetch` assertions to expect `POST /api/Journeys`
  with a `{leg: {mode: 'pin', ...}}` body and a `{journeyId, ...}`-shaped
  mock response, and `router.push` assertions to expect
  `/journeys/{journeyId}`. Add at least one new test exercising the
  `SegmentedControl` → window-mode path: select "Search a time window",
  fill Origin + Destination + one time bound, submit, assert the POST body
  has `leg.mode === 'window'` and the correct nested `departWindow`/
  `arriveWindow` shape, and assert an all-blank window submission is
  blocked client-side (the Search button's disabled state, or
  `fieldError`, per `canSubmitWindow`'s own gating) without a network call.

- [ ] **Step 5: Verify**

```bash
cd frontend && npm test -- TrackTrainForm
npx tsc --noEmit
npm run build
```

  Expected: updated + new tests pass; clean typecheck and build.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "frontend: rewire TrackTrainForm onto POST /Journeys; add window-search mode"
```

---

## Task 16: Frontend — `JourneyLegCandidates.tsx` (§2.2/§2.3/§4)

**Files:** create `frontend/components/JourneyLegCandidates.tsx`,
`frontend/components/JourneyLegCandidates.test.tsx`.

Depends on Task 10 (`GET .../candidates`) and Task 11 (`POST .../train`)
for real manual verification; compiles and unit-tests standalone. Built
ONCE and used TWICE by Task 17's journey view — unconditionally for a
genuinely open leg, and behind a "Change train" toggle for an already-matched
leg with a persisted window — so the candidate-browsing UI is reviewed once,
not duplicated between the two call sites (this plan's own design choice,
resolving the spec's §4 description of both surfaces as "the same open-leg
candidate card").

- [ ] **Step 1: Create the component**

```tsx
'use client';

import { useEffect, useState } from 'react';
import { Alert, Button, Group, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';

/** Wire shape of `GET /Journeys/{id}/legs/{id}/candidates` -- the same
 * envelope `GET /public/trains/search` returns
 * (`crates/api/src/render.rs::calling_point_departure_json`), reused
 * verbatim per design doc §2.2. */
interface CandidateRow {
  uid: string;
  scheduled: string;
  destinationCrs: string | null;
  trueOriginCrs: string | null;
  destinationArrival: string | null;
}

interface CandidatesResponse {
  results: CandidateRow[];
  nextCursor: string | null;
}

/** The open-leg candidate list + pick action -- design doc §2.2/§2.3/§4.
 * `onPicked` is called after a successful commit; the caller (a
 * `JourneyLegCard`, `frontend/components/JourneyLegCard.tsx`) decides what
 * to do next (typically `router.refresh()`). Pagination (`nextCursor`) is
 * deliberately not implemented in this first pass -- the backend route
 * supports it (same shape `TrainSearchForm.tsx`'s own "Load more" already
 * consumes), but a journey leg's candidate list is expected to be short
 * (a bounded time window, not a whole day's unfiltered search); add a
 * `LoadMoreControl` here, mirroring `TrainSearchForm.tsx`'s own, if that
 * assumption proves wrong in practice. */
export function JourneyLegCandidates({
  journeyId,
  legId,
  serviceDate,
  onPicked,
}: {
  journeyId: number;
  legId: number;
  serviceDate: string;
  onPicked: () => void;
}) {
  const [results, setResults] = useState<CandidateRow[] | 'loading' | 'error' | null>(null);
  const [picking, setPicking] = useState<string | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setResults('loading');
    fetch(`/api/Journeys/${journeyId}/legs/${legId}/candidates`)
      .then((res) => (res.ok ? res.json() : Promise.reject(res)))
      .then((body: CandidatesResponse) => {
        if (!cancelled) setResults(body.results);
      })
      .catch(() => {
        if (!cancelled) setResults('error');
      });
    return () => {
      cancelled = true;
    };
  }, [journeyId, legId]);

  async function pick(uid: string) {
    setPicking(uid);
    setPickError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${legId}/train`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainUid: uid, serviceDate }),
      });
      if (!response.ok) {
        setPickError("Couldn't track that train. Try again.");
        return;
      }
      onPicked();
    } catch {
      setPickError("Couldn't track that train. Try again.");
    } finally {
      setPicking(null);
    }
  }

  if (results === null || results === 'loading') {
    return (
      <Text size="sm" c="dimmed">
        Searching for candidate trains…
      </Text>
    );
  }
  if (results === 'error') {
    return (
      <Alert color="red" title="Search failed">
        Couldn&apos;t load candidate trains right now. Try again.
      </Alert>
    );
  }
  if (results.length === 0) {
    return (
      <Text size="sm" c="dimmed">
        No scheduled trains match this window.{' '}
        <TextLink href="/track" inline underline="always">
          Search manually
        </TextLink>{' '}
        instead.
      </Text>
    );
  }
  return (
    <Stack gap="xs">
      {pickError && <Alert color="red">{pickError}</Alert>}
      {results.map((row) => (
        <Group key={row.uid} justify="space-between" wrap="wrap">
          <Text size="sm">
            {row.scheduled} · {row.trueOriginCrs ?? '?'} → {row.destinationCrs ?? '?'}
          </Text>
          <Button size="xs" loading={picking === row.uid} disabled={picking !== null} onClick={() => pick(row.uid)}>
            Track this train
          </Button>
        </Group>
      ))}
    </Stack>
  );
}
```

- [ ] **Step 2: Add a test file**, mocking `fetch` for both the candidates
  GET and the train-pick POST, following this codebase's existing
  vitest/`@testing-library/react` conventions (see any existing
  `TrackThisTrainButton.test.tsx`-shaped file for the exact mocking
  pattern used elsewhere in this repo). Cover: loading state renders,
  results render with a "Track this train" button per row, a click POSTs
  the right body and calls `onPicked` on success, an empty result set
  renders the "Search manually" fallback link, and a fetch rejection
  renders the error `Alert`.

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- JourneyLegCandidates
npx tsc --noEmit
```

  Expected: new tests pass; no new type errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/components/JourneyLegCandidates.tsx frontend/components/JourneyLegCandidates.test.tsx
git commit -m "frontend: add JourneyLegCandidates (open-leg candidate list + pick action)"
```

---

## Task 17: Frontend — `/journeys/[id]` view (§4)

**Files:** create `frontend/components/JourneyLegCard.tsx`,
`frontend/app/journeys/[id]/page.tsx`.

Depends on Task 13 (`getJourney`), Task 16 (`JourneyLegCandidates`), and
reuses `TrainJourney.tsx` completely unmodified (design doc §4: "already
fully built... needs no work beyond embedding" — see this plan's own
Non-goals for what is deliberately NOT added to this view: rename, delete,
the skip badge, the platform column, group-share).

- [ ] **Step 1: Create `JourneyLegCard.tsx`** — the client component
  deciding, per leg, whether to render the matched-leg embed (`TrainJourney`
  + a conditional "Change train" toggle) or the open-leg candidate card:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
import type { JourneyLegDetail } from '@/lib/types';

/** One leg's card on `/journeys/[id]` (design doc §4). Two branches:
 *
 * - **Open** (`trackedTrainState === null`): the search parameters plus an
 *   unconditional `JourneyLegCandidates`.
 * - **Matched**: `TrainJourney` (reused unmodified, per the design doc's
 *   own explicit direction) plus, ONLY when the leg has a persisted window
 *   (`departAfter`/`departBefore`/`arriveAfter`/`arriveBefore` -- any
 *   non-null), a "Change train" toggle that reveals the SAME
 *   `JourneyLegCandidates` component, re-scoped to this leg (its own
 *   persisted window drives what the backend searches -- see
 *   `crates/api/src/routes/journeys.rs::get_leg_candidates`). A leg with
 *   no window (a `pin`/`knownTrain`-mode leg) has no window to re-search
 *   and so gets no "Change train" action -- swapping it means
 *   delete-and-recreate, same as today's single-train tracking, per the
 *   design doc's own 2026-09-22 addendum. */
export function JourneyLegCard({ journeyId, leg }: { journeyId: number; leg: JourneyLegDetail }) {
  const router = useRouter();
  const [changingTrain, setChangingTrain] = useState(false);
  const hasWindow =
    leg.departAfter !== null ||
    leg.departBefore !== null ||
    leg.arriveAfter !== null ||
    leg.arriveBefore !== null;

  if (leg.trackedTrainState === null) {
    return (
      <Card withBorder>
        <Stack gap="sm">
          <Text fw={500}>
            {leg.originCrs ?? '?'} → {leg.destinationCrs ?? '?'}, {leg.serviceDate}
          </Text>
          <Text size="sm" c="dimmed">
            Searching for a train to track — pick one below.
          </Text>
          <JourneyLegCandidates
            journeyId={journeyId}
            legId={leg.id}
            serviceDate={leg.serviceDate}
            onPicked={() => router.refresh()}
          />
        </Stack>
      </Card>
    );
  }

  return (
    <Card withBorder>
      <Stack gap="sm">
        <TrainJourney state={leg.trackedTrainState} />
        {hasWindow && (
          <>
            <Button size="xs" variant="default" onClick={() => setChangingTrain((c) => !c)}>
              {changingTrain ? 'Cancel' : 'Change train'}
            </Button>
            {changingTrain && (
              <JourneyLegCandidates
                journeyId={journeyId}
                legId={leg.id}
                serviceDate={leg.serviceDate}
                onPicked={() => {
                  setChangingTrain(false);
                  router.refresh();
                }}
              />
            )}
          </>
        )}
      </Stack>
    </Card>
  );
}
```

- [ ] **Step 2: Create `app/journeys/[id]/page.tsx`**:

```tsx
import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { LoginLink } from '@/components/LoginLink';

export const revalidate = 0;

/** `/journeys/[id]` -- design doc §4. One card per leg (Phase 1: always
 * exactly one, see `crates/api/src/data/journeys.rs`'s own module doc
 * comment). No editable header, no delete, no share-to-group button, no
 * skip badge, no platform column -- all explicitly deferred, see this
 * plan's own Non-goals for the reasoning behind each. */
export default async function JourneyDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!/^\d+$/.test(id)) {
    notFound();
  }

  let journey;
  try {
    journey = await getJourney(Number(id));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    // Same distinct "log in, this might be yours" posture
    // app/train/by-id/[trackingId]/page.tsx already takes for the
    // identical 401-vs-404 split, for the same reason: this page has no
    // public sibling content to fall back to.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Someone&apos;s tracked journey — log in to see it</Title>
          <LoginLink underline="always">Log in to view this journey</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>{journey.customName ?? 'Tracked journey'}</Title>
      {journey.legs.map((leg) => (
        <JourneyLegCard key={leg.id} journeyId={journey.id} leg={leg} />
      ))}
    </Stack>
  );
}
```

- [ ] **Step 3: Verify — automated**

```bash
cd frontend && npm test
npx tsc --noEmit
npm run build
```

  Expected: all tests pass, clean typecheck, clean build (in particular,
  confirm `next build` doesn't try to statically prerender
  `/journeys/[id]` — it shouldn't, given `revalidate = 0` and a dynamic
  route param, matching every other per-id detail page in this app).

- [ ] **Step 4: Verify — manual, in a real browser.** Per this plan's own
  Global Constraints (no automated end-to-end coverage exists for this
  flow):
  1. On `/track`, leave mode at "Pick a departure", track a train the
     usual way. Confirm you land on `/journeys/{id}` (not
     `/train/by-id/{id}`), and that the page renders the same journey
     content `TrainJourney` always has.
  2. On `/trains`, click "Track this train" on a search result. Confirm
     you land on `/journeys/{id}` for a NEW journey wrapping that train.
  3. Back on `/track`, switch to "Search a time window". Enter an origin,
     destination, and at least one departure/arrival time bound (for a
     service date you know has published CIF data — today or the near
     future). Submit. Confirm you land on `/journeys/{id}` for a new,
     UNMATCHED journey, and that the page shows a candidate list.
  4. Click "Track this train" on one of the candidates. Confirm the page
     updates (via `router.refresh()`) to show the matched leg's full
     `TrainJourney` content, and a "Change train" button.
  5. Click "Change train". Confirm the SAME candidate list reappears
     (re-searched against the leg's own persisted window). Pick a
     DIFFERENT candidate. Confirm the page updates to the new train's
     journey content.
  6. Confirm a `pin`-mode or `knownTrain`-mode journey's matched leg shows
     NO "Change train" button (no window was ever searched).
  7. Log out (or open a private window) and visit another user's
     `/journeys/{id}` URL directly. Confirm the distinct "log in, this
     might be yours" page renders, not a generic 404.
  8. Visit `/journeys/999999999` (a made-up id) while logged in. Confirm a
     genuine Next.js 404 page renders.
  9. Confirm `/track/mine` still lists every train tracked in steps 1–5
     above, unchanged, exactly as it did before this feature shipped.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/JourneyLegCard.tsx frontend/app/journeys/[id]/page.tsx
git commit -m "frontend: add /journeys/[id] view (matched-leg embed + open-leg candidates)"
```

---

## Self-review (spec coverage)

- §1.1 schema (with §7.1's own nullability correction) → Task 1, Judgment
  Call 1.
- §7.1 historical migration → Task 2, Judgment Call 7.
- §0.5/§2.1 `TimeWindow` gap → Task 3.
- §2.1 candidate query + ordering fix → Task 4, Judgment Call 2.
- §7.2 "`POST /Journeys` wraps `create_pin`/`create_subscription_for_train`
  unchanged" → Tasks 6, 9, Judgment Call 3.
- §2.1 new open-leg path → Task 6, Judgment Call 4.
- §2.2 candidates route → Task 10.
- §2.3 (+ 2026-09-22 addendum) manual-mode commit, reused for first pick
  and "Change train" → Tasks 7, 11.
- §4 `GET /Journeys/{id}` reusing `TRACKED_TRAIN_STATE_SELECT` verbatim →
  Task 8, 12.
- §4 journey view (matched-leg embed, open-leg candidate card, "Change
  train" gated on a persisted window) → Tasks 16, 17.
- §9's revised Phase 1 scope, itemized → every task above; the explicit
  "still out of Phase 1" list (multi-leg chaining, `'auto'` mode,
  notifications, group sharing) → this plan's own Non-goals.
- Every open judgment the task brief itself left implicit (three leg shapes
  vs. two, the all-blank-window ambiguity, the sibling-vs-edit query
  decision, non-transactional leg creation, and what to defer from §4's
  fuller view sketch) → "Judgment calls this plan makes," above.

---

## Handoff to Phase 2 (multi-leg chaining)

A Phase 2 plan implementing `POST /Journeys/{id}/legs` (design doc §3)
should reuse the following exact surface from this plan without
modification:

- **`crates/api/src/data/journeys.rs`'s `insert_leg`** (private to that
  file as of this plan) — becomes the second leg's writer too, just with
  `leg_order = (SELECT COALESCE(MAX(leg_order), 0) + 1 FROM journey_legs
  WHERE journey_id = $1)` computed by the caller instead of this plan's
  hardcoded `1`. Widen its visibility from private to `pub(crate)` if
  Phase 2 needs to call it directly, or add a thin
  `insert_leg_at_next_order(pool, journey_id, ...)` wrapper — either way,
  the underlying `INSERT` shape does not need to change.
- **`journeys::create_journey_with_pin_leg`/
  `create_journey_with_known_train_leg`/`create_journey_with_window_leg`**
  — Phase 2's `POST /Journeys/{id}/legs` needs the SAME three leg-creation
  shapes (a leg can be added via a pin guess, a known identity, or an open
  window, exactly like a journey's first leg), just targeting an EXISTING
  `journey_id` instead of creating a new `journeys` row first. Expect to
  refactor each of these three functions into "create the journey, then
  call a shared `add_leg_*` helper" so Phase 2 can call the shared helper
  directly against an existing journey — this plan's current shape (one
  function per creation path, `journeys` row creation inlined) makes that
  refactor mechanical, not a redesign.
- **`journeys::set_leg_train_subscription`, `journeys::get_owned_leg`,
  `routes/journeys.rs`'s `get_leg_candidates`/`post_leg_train`** — used
  UNCHANGED per additional leg; nothing about the commit/candidates flow is
  leg-count-aware.
- **`JourneyDetail`/`JourneyLegDetailResponse`'s existing `Vec<...>` shape**
  (Task 8/12) — already multi-leg-correct; Phase 2 needs no response-shape
  change, only a longer `Vec`.
- **`JourneyLegCandidates`/`JourneyLegCard`** (Tasks 16–17) — already
  parameterized per-leg (`legId`, not journey-wide); Phase 2's "Add a leg"
  UI can render an additional `JourneyLegCard` per leg with zero change to
  either component.
- **What Phase 2 must add that this plan deliberately does not**: the
  per-leg-status "worst status" rollup badge (design doc §3, mirroring
  `frontend/lib/severity.ts`'s existing `worstStatus`/`GROUP_RANK` idiom —
  named in the spec as "no new severity-ranking code... a straight port"),
  the "Add a leg" frontend flow and its default-suggest-prior-destination
  behavior, and (per the spec's own explicit sequencing) the connection-
  buffer feature is Phase 3+, not Phase 2's job either.
