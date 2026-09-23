# Plan: Reusable & Repeating Journeys — Phase C (Recurrence + Materialization Sweep + `'auto'` Matching)

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Scope discipline:** this plan covers **Phase C only** (spec §8) — recurrence
actually running, the `notifier` materialization sweep, and `'auto'` matching
actually committing a train. Phase A ("Track this journey again") and Phase B
(durable `journey_templates`/`journey_template_legs`, on-demand-only, no
recurrence) are separate plans, planned in parallel by other agents. This plan
does not design Phase B's CRUD routes, its `/journeys/templates` list/detail
pages, or its "Run now" button — it only states, explicitly below, what it
assumes those deliver, so a human can diff this plan against Phase B's actual
plan/implementation once both exist.

---

## ⚠️ Assumptions about Phase B — verify before starting Task 4

Phase B has **not been planned yet** as of this writing (`docs/superpowers/plans/`
has no Phase B file). Everything below is inferred from the design spec's
§2.2/§3.2/§8 and this codebase's own established conventions, not read from a
real Phase B plan. **A human integrating both plans must re-check every
bullet here against what Phase B actually ships**, and fix any drift before
Task 4 lands.

1. **Table shape** (spec §2.2, verbatim): `journey_templates` has
   `id, user_id, custom_name, days_of_week SMALLINT, active BOOLEAN NOT NULL
   DEFAULT TRUE, starts_on DATE, ends_on DATE, default_match_mode TEXT NOT
   NULL DEFAULT 'manual' CHECK IN ('manual','auto'), auto_commit_rule TEXT
   CHECK IN ('earliest','nearest_to_now'), created_at, updated_at`.
   `journey_template_legs` has `id, template_id, leg_order, origin_crs,
   destination_crs, depart_after/before TIME, arrive_after/before TIME` — **no
   `service_date`** (spec §2.2's own point: a template leg is date-less).
   `journeys` has gained a nullable `source_template_id BIGINT REFERENCES
   journey_templates(id) ON DELETE SET NULL`. **All six recurrence-only
   columns (`days_of_week`/`active`/`starts_on`/`ends_on`/
   `default_match_mode`/`auto_commit_rule`) already exist in the DB by the
   time Phase C starts** — per the spec's own migration-avoidance note
   (§8: "or with those columns present but unused/always-NULL, since adding
   them in Phase B avoids a second migration in Phase C"). **This plan adds
   NO migration that touches `journey_templates`/`journey_template_legs`** —
   Task 3's one new migration only touches `journey_leg_notification_state`
   (a Phase 3, already-shipped table). If Phase B's actual migration does
   *not* include these six columns, Task 4 cannot proceed until a small
   follow-up migration adds them — flagged, not designed here.

2. **A per-template "mint today's occurrence" function exists**, called
   synchronously by Phase B's manual `POST /JourneyTemplates/{id}/materialize`
   ("Run now") route. Assumed shape (a plain `&PgPool` function, no
   `AppState`/axum types — this codebase's established convention for
   `crates/api/src/data/*.rs`, e.g. `journeys::create_journey_with_window_leg`):

   ```rust
   // crates/api/src/data/journey_templates.rs (assumed)
   pub async fn materialize_template(
       pool: &PgPool,
       template_id: i64,
       user_id: &str,      // ownership-scoped, same convention as every other write in journeys.rs
       target_date: NaiveDate,
   ) -> anyhow::Result<Option<i64>>;  // Some(journey_id) on real mint, None if already materialized for that date (idempotent) or template not owned by user_id
   ```

   Assumed idempotency guard: **a `journeys` row already exists for
   `(source_template_id, target_date)`**, checked via `journey_legs.service_date`
   (`journeys` itself has no `service_date` column — spec §0.1/§2.2 both
   confirm this), i.e. `EXISTS (SELECT 1 FROM journeys j JOIN journey_legs jl
   ON jl.journey_id = j.id WHERE j.source_template_id = $1 AND jl.service_date
   = $2)`. Assumed: **every minted leg gets `match_mode = 'unmatched'`
   regardless of `default_match_mode`** — Phase B ships "no automatic-matching
   … logic yet" (spec §8), so its own mint function cannot read
   `default_match_mode`/`auto_commit_rule` for anything beyond storing them;
   `'auto'`-vs-`'manual'` behavior is entirely Phase C's addition (Task 4).

3. **Why Phase C does not literally call this function** (rather than
   "duplicating," which the task brief for this plan explicitly warned
   against): `crates/notifier` has a hard, already-established constraint —
   **it must never gain a dependency on `crates/api`** — stated verbatim as a
   binding Global Constraint in the already-merged
   `docs/superpowers/plans/2026-09-22-journey-tracking-phase3-notifications-station-skip-plan.md`
   ("`crates/notifier` must never gain a dependency on `crates/api`… every new
   notifier-side query in this plan is a fresh, independent SQL statement…"),
   confirmed live in the actual `crates/notifier/Cargo.toml` (no `api` path
   dependency) and echoed in `crates/notifier/src/queries.rs`'s own doc
   comments (`station_sample_for_crs`: "necessarily duplicated, not imported,
   since `crates/notifier` does not … depend on `crates/api`"). Phase 3 hit
   this **exact same** problem for `eta_blend::find_darwin_eta` and resolved
   it by extracting only the crate-boundary-safe **pure** decision logic into
   `crates/common` (which both crates depend on), while leaving the SQL fetch
   "necessarily duplicated… the *decision* is written exactly once." This
   plan follows the identical precedent (Task 1/Task 4 below):
   - The one piece of §3.2's mechanic that is genuinely pure, drift-prone
     logic — **picking the nearest-to-now candidate** — is written once,
     unit-tested, and is the only thing this plan treats as "must never
     silently diverge." Since Phase B has no auto-commit logic at all (it
     doesn't call this function yet), this plan places it in
     `crates/notifier/src/decision.rs` rather than promoting it to
     `crates/common` pre-emptively — see Judgment Call 1.
   - The **INSERT shape** (stamping `journeys`/`journey_legs` from a
     template) is SQL, not decision logic, and is mirrored field-for-field in
     `crates/notifier/src/queries.rs::materialize_due_template_occurrence`
     (Task 4) against the *assumed* shape in bullet 2 above — the same kind
     of necessary duplication `list_committed_legs_for_today`/
     `station_sample_for_crs` already are. **A human integrator must diff
     Task 4's SQL against Phase B's real `materialize_template`
     once it exists and correct any drift** (different idempotency guard,
     different default `custom_name` handling, etc.) — called out again in
     Task 4's own Verify step.

4. **Phase B's update route accepts the recurrence fields.** Whatever route
   Phase B builds to edit a template (`PUT /JourneyTemplates/{id}` is assumed,
   mirroring `update_line`'s full-resource-replace convention,
   `crates/api/src/routes/lines.rs`) is assumed to already accept
   `daysOfWeek`/`active`/`startsOn`/`endsOn`/`defaultMatchMode`/
   `autoCommitRule` in its request body and persist them as plain columns,
   even though Phase B's own backend does nothing else with them. **If Phase
   B's real route restricts its accepted fields to only what it acts on,
   Task 6 below extends that route** rather than Phase C inventing a second,
   parallel update route.

If any of the above turns out wrong once Phase B's plan exists, the tasks
that depend on it are Task 4 (backend mint/commit) and Task 6/7 (frontend
edit surface) — Tasks 1, 2, 3, 5 are self-contained and unaffected.

---

## Goal

Make `days_of_week`/`active`/`starts_on`/`ends_on`/`default_match_mode`/
`auto_commit_rule` actually drive behavior: a new hourly `crates/notifier`
sweep mints each due template's occurrence for today (idempotently), and — for
`default_match_mode = 'auto'` templates — a **second**, deferred check commits
the leg to whichever candidate train is nearest to "now" at the moment that
check runs (not the earliest candidate overall), per the product owner's
2026-09-22 resolution of spec Open Question #2. A new, narrowly-scoped
notification fires once if that commit-check ever finds zero candidates.
Frontend gets the controls to configure all of this on a template.

## Architecture

```
                    ┌─────────────────────────────────────────┐
                    │  crates/notifier (new tokio::select! arm) │
                    │  ticks every template_sweep_poll_interval_secs (3600s) │
                    └───────────────┬───────────────────────────┘
                                    │
                 ┌──────────────────┴───────────────────┐
                 │                                       │
   STAGE 1: mint due occurrences          STAGE 2: commit-check due unmatched auto legs
   (mirrors Phase B's assumed             (genuinely NEW logic — Phase B has none of this)
    materialize_template,
    duplicated per Task 4)
                 │                                       │
   for each active template whose         for each 'unmatched' leg whose journey's
   days_of_week bit for today is set,     source_template has default_match_mode='auto',
   today ∈ [starts_on, ends_on], and      service_date = today, and now is within
   no journeys row exists yet for         auto_commit_lead_minutes of the leg's
   (source_template_id, today):           earliest window bound:
     INSERT journeys (source_template_id)   run the (slim, duplicated) candidate
     INSERT journey_legs (match_mode         query against schedule_destination_departures;
       = 'unmatched', ALWAYS,                if >=1 candidate: pick nearest-to-now
       regardless of default_match_mode)     (common decision fn), find_or_create_train +
                                              create_subscription_for_train (duplicated),
                                              UPDATE journey_legs SET match_mode='auto',
                                              train_subscription_id = ...
                                            if 0 candidates: leave 'unmatched', fire the
                                              new "needs attention" notification (once,
                                              ever, per leg — journey_leg_notification_state)
```

**Why one hourly branch, not two.** The spec's updated §3.2 says the
commit-check runs "on the same hourly cadence" as the mint — this plan
therefore adds exactly **one** new `tokio::select!` arm
(`crates/notifier/src/main.rs`) whose cycle function runs stage 1 then stage 2
sequentially each tick, not two separate intervals. This mirrors the existing
crate's precedent of one function per concern but doesn't over-multiply timers
for two things the spec explicitly says share a cadence.

**Why "today" is a plain Europe/London calendar date, not a rail day.**
`journeys`/`journey_legs.service_date` are plain calendar `NaiveDate`
throughout the already-shipped schema and every existing writer (`journeys.rs`'s
`create_journey_with_window_leg` etc. take `NaiveDate` directly from user
input, never a rail-day-shifted value). `common::rail_day` (the existing
02:00-Europe/London-cutover helper) is used elsewhere in this codebase for
incident staleness and full-coverage gating, but never for `journeys`. Using
it here would make a template's "day" concept disagree with every other
`service_date` in this schema for no benefit — so "today" for both stages is
`Utc::now().with_timezone(&chrono_tz::Europe::London).date_naive()`, a plain
calendar date, matching the rest of the journeys feature.

## Tech stack

Rust/axum/sqlx (`crates/api`, `crates/notifier`, both depending on
`crates/common`), Next.js/React/Mantine (`frontend`), Postgres.

## Spec

`docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md` —
§2.2/§2.3 (schema, what `'auto'` means), §3 (materialization sweep, **§3.2
updated 2026-09-22** — the two-stage commit mechanic this plan implements
exactly), §4.2 (new notification class, narrow slice only — see Non-goals),
§6 (UX), §7 Q1/Q2 (resolved), §8 Phase C. This plan does not re-argue
anything the spec already settled; §3.2's 2026-09-22 addendum is the single
most load-bearing paragraph for this plan and is implemented literally.

---

## Judgment calls this plan makes (read before Task 1)

1. **`pick_nearest_to_now_candidate` lives in `crates/notifier/src/decision.rs`,
   not `crates/common`, despite Phase 3's own precedent of extracting
   crate-boundary-shared *decision* logic into `common`.** Phase 3 promoted
   `find_darwin_eta`'s matching step to `common` because **two** real call
   sites needed the identical logic at plan-time (the journey-view skip
   badge in `crates/api`, the skip notification in `crates/notifier`).
   Nothing in Phase B calls a "nearest-to-now" picker — Phase B has no
   auto-commit logic at all. Promoting this function to `common` now, with
   exactly one caller, would be premature abstraction with no drift to
   prevent yet. **If a future phase (e.g. a Phase B revision that wants to
   preview "which train would auto-commit today" in the UI) adds a second
   real call site, promote it to `common` at that time** — mirroring exactly
   when/why Phase 3 did the same move, not before.

2. **`auto_commit_rule` is hard-set to `'nearest_to_now'` by this phase's own
   frontend whenever `default_match_mode = 'auto'`; `'earliest'` is never
   written, and Phase C's backend never reads or branches on
   `auto_commit_rule`'s value at all.** The spec's §6 item 3 (written before
   the 2026-09-22 addendum) describes a "three-way… manual reminder /
   auto-earliest / auto-nearest-to-now" choice, but §7 Q2's later resolution
   supersedes that: "`'nearest_to_now'`… is the rule to ship" (singular).
   Building real `'earliest'` mechanics (immediate-commit-at-mint-time) would
   contradict the resolved decision and double the surface this phase has to
   get right for a rule the product owner explicitly did not choose. This
   plan therefore ships a genuinely **two-way** choice (manual reminder /
   auto-commit), with `auto_commit_rule` fixed to `'nearest_to_now'`
   whenever the latter is picked. `auto_commit_rule = 'earliest'` stays
   **schema-legal but implementation-unreachable** — the exact same "reserved
   value, not currently reachable from any code" posture the spec's own §0.2
   already documents for `journey_legs.match_mode = 'auto'` pre-Phase-C. If a
   later phase wants `'earliest'` for real, it needs its own plan (the
   original, now-superseded §3.2 mechanics for it are still in the spec's
   history for reference).

3. **The commit-check retries every hour, forever within the same
   `service_date`, until it either commits or the calendar date rolls over.**
   The spec doesn't explicitly resolve whether a zero-candidate result is
   retried. Since stage 2's SQL scopes to `service_date = today` (recomputed
   fresh each tick), a leg naturally stops being selected once "today"
   becomes tomorrow — bounding retries to at most ~24 hours without adding a
   second, explicit cutoff. This also means a late-published/updated
   schedule that adds a candidate after an earlier zero-result tick can still
   be picked up on a later tick that same day — free correctness, no extra
   code.

4. **The new "needs attention" notification (§4.2) is narrowly scoped to
   exactly the case Task 4's own stage 2 creates: an `'auto'`-mode leg whose
   commit-check found zero candidates.** The spec's full §4.2 is broader (it
   also covers the `default_match_mode = 'manual'` "remind me, don't guess"
   hybrid from §3.3 approaching its own window, unrelated to any commit-check
   at all) and the spec's own §8 phasing puts §4.2 in **Phase D**, sequenced
   after this one. This plan's task brief explicitly asked for only the
   auto-zero-candidates slice, reasoning that leaving *that* specific case
   fully silent would be a regression Phase C itself introduces (a template
   that silently never matches, forever, with zero signal) — worth closing
   immediately rather than waiting for a separate phase. The broader
   `'manual'`-mode reminder, and §7 Q8's still-open "what cutoff" question for
   *that* case, stay out of scope here — no new generic cutoff constant is
   added; this notification fires exactly at the commit-check tick that finds
   zero candidates, using `auto_commit_lead_minutes` as its only timing input.

5. **`journeys.custom_name` on a minted occurrence is the template's
   `custom_name`, verbatim — no date suffix appended.** The spec named this
   as an open frontend/copy question (§3.2: "or template name + date if the
   product wants…"). Verbatim inheritance is the simplest option and the
   journey list already shows each row's actual date via its legs'
   `service_date` elsewhere in the UI, so appending a date to the name adds
   redundant text rather than new information.

6. **Auto-commit produces a `train_subscriptions` row with exactly the same
   shape/limitations a manual "Change train" pick already produces today —
   `pin_origin_crs`/`pin_scheduled_departure`/`pin_destination_crs` left NULL
   until later backfilled — not a "better," schedule-enriched row.**
   `crates/api/src/routes/journeys.rs::post_leg_train` (the existing manual
   commit route) calls the bare `trains::find_or_create_train` +
   `train_tracking::create_subscription_for_train`, **not**
   `find_or_create_train_with_schedule_match` — so a manually-picked leg's
   `train_subscriptions` row already has NULL pin fields until
   `enrich_shared_train`'s TRUST-backlog-replay/schedule-matching subsystem
   (both `crates/api`-only, non-trivial) backfills them, or live
   `trust-consumer` resolution does. Duplicating that whole subsystem into
   `crates/notifier` is wildly disproportionate to this phase's scope. This
   plan's auto-commit therefore calls only the bare two functions (duplicated
   per the crate-boundary constraint, Task 4), accepting the identical,
   already-existing NULL-pin-fields gap every manual pick has today — **not a
   new gap Phase C introduces.** `skip_check`'s own Darwin-destination
   matching already tolerates a NULL `pin_destination_crs` via its
   `next_calling_point` fallback (`crates/notifier/src/queries.rs`'s
   `CommittedLeg`/`list_committed_legs_for_today`), so this doesn't regress
   the one notifier feature that reads that column.

7. **The day-of-week bitmask (Mon=1..Sun=64) is computed in Rust via
   `chrono::Weekday::num_days_from_monday()`, not stored/recomputed in SQL
   per row beyond one `EXTRACT(ISODOW …)`-based WHERE clause.** Postgres
   `EXTRACT(ISODOW FROM date)` returns 1=Monday..7=Sunday, so the SQL bit
   test is `(days_of_week & (1 << (EXTRACT(ISODOW FROM $1::date)::int - 1)))
   != 0` — this plan's Task 4 writes that literally, and Task 1's
   `weekday_bit` Rust helper exists only for the unit-testable, symmetrical
   Rust-side version used nowhere in a hot query path but useful for
   debugging/tests and any future Rust-side day-of-week logic. If Phase B's
   frontend needs the identical bit convention for its own "Weekdays"
   recurrence-summary rendering (spec §6 item 3), that's a TypeScript-side
   concern outside this plan's file scope — flagged, not built here.

---

## Non-goals

- **`auto_commit_rule = 'earliest'`** — schema-legal, never written by this
  phase's frontend, never read by this phase's backend (Judgment Call 2).
- **The broader §4.2 "manual-mode-by-design reminder" notification class**
  (an unmatched leg from a `default_match_mode='manual'` template approaching
  its window) — Phase D's job per the spec's own phasing (Judgment Call 4).
- **Per-template override of `auto_commit_lead_minutes`** — one global config
  constant, not a per-template column. The spec names this constant at the
  config level (mirroring `train_delay_threshold_minutes`), not as a schema
  field in §2.2's table — not revisited here.
- **Bank holiday exclusion, single-day snooze, retention/archival job,
  occurrence-count end condition** — spec §7 Q4/Q5, explicitly out of scope
  for every phase per §8's own "not phased separately" list.
- **Group template sharing** (`group_journey_templates`) — Phase E.
- **A collapsed/grouped `/journeys/mine` list view** — spec §6's own flagged,
  unresolved list-design question; untouched.
- **Any change to Phase B's `journey_templates`/`journey_template_legs`
  migration, or its CRUD/list/detail routes and pages**, beyond the one
  conditional extension named in Task 6 (only if Phase B's real update route
  doesn't already accept the recurrence fields).
- **No change to `crates/notifier`'s existing three cycles** (`run_cycle`,
  `run_forward_queue_cycle`, `run_skip_check_cycle`) — this plan adds a
  fourth, independent one.

## Global Constraints

- **`crates/notifier` must never gain a dependency on `crates/api`.** Every
  new notifier-side query in this plan (Task 4) is a fresh, independent SQL
  statement, never an import of `crates/api`'s data layer — see the
  Assumptions section above for the full reasoning and precedent citation.
- **Every new migration must sort after whatever timestamp Phase B's own
  `journey_templates` migration lands at, AND after the current latest.**
  Task 3 uses `20260922140000` as a placeholder (sorts after
  `20260922130000_journey_leg_notification_state.sql`, the latest migration
  as of this plan's writing — confirmed via `ls crates/api/migrations | sort
  | tail -3`). **Re-verify at implementation time** with the same command
  (Phase B's migration will very likely have claimed a slot between
  `20260922130000` and whatever Task 3 picks) and bump if needed.
- **`journey_legs.origin_crs`/`.destination_crs` are nullable** (Phase 1's
  own correction, still true) — Task 4's stage-2 query filters these
  `IS NOT NULL` explicitly, never assumes `NOT NULL`.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (this repo's CI invocation),
  `cargo test --workspace` (ignored tests skipped, matching CI's fast
  default), and `cargo test -p notifier -- --ignored --test-threads=1` /
  `cargo test -p api -- --ignored --test-threads=1` (only if Task 6 touches
  `crates/api`) for every DB-gated test this plan adds, against
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres` (CI's
  own Postgres service). Frontend: `npm test -- <file>` (vitest) per changed
  test file, plus a full `npm test` and `npm run build` before considering
  the frontend task done. **UI verification**: start the dev stack and
  manually verify in a real browser, per this repo's standing practice.
- **File scope.** Modified/created:
  `crates/notifier/src/decision.rs` (Task 1),
  `crates/notifier/src/config.rs` (Task 2),
  `crates/api/migrations/20260922140000_journey_leg_notification_state_unmatched.sql`
  (new, Task 3, timestamp subject to the re-verification note above),
  `crates/notifier/src/queries.rs` (Task 4),
  `crates/notifier/src/main.rs` (Task 5),
  `crates/api/src/routes/journey_templates.rs` (Phase B's assumed file, Task
  6 only, conditional — see Task 6's own guardrail),
  `frontend/app/journeys/templates/[id]/page.tsx` and/or
  `frontend/components/JourneyTemplateForm.tsx` (Phase B's assumed files,
  Task 7 only — exact paths to be confirmed against Phase B's real
  implementation before Task 7 starts).
  No other file changes.

---

## Task 1: `crates/notifier/src/decision.rs` — pure decision logic for the sweep

**Files:** modify `crates/notifier/src/decision.rs`.

Independent first task, no DB, no dependency on Tasks 2-5's plumbing — pure
functions with unit tests, matching this file's existing style
(`is_severity_transition`, `decide_train_notification`, etc., all pure, all
tested with no `#[ignore]`).

- [ ] **Step 1: `weekday_bit`** — the Rust-side mirror of the SQL bit test
  (Judgment Call 7), for unit-testability and any future Rust-side use:

  ```rust
  /// Mon=1 (bit 0) .. Sun=64 (bit 6) -- the exact convention
  /// `journey_templates.days_of_week` uses (spec §2.2). Mirrors the SQL
  /// this plan's Task 4 writes as `1 << (EXTRACT(ISODOW FROM $1)::int - 1)`
  /// -- ISODOW is 1=Monday..7=Sunday, matching `num_days_from_monday()`'s
  /// 0=Monday..6=Sunday after the +1/-1 shift.
  pub fn weekday_bit(date: chrono::NaiveDate) -> i16 {
      use chrono::Datelike;
      1i16 << date.weekday().num_days_from_monday()
  }
  ```

- [ ] **Step 2: `is_due_for_commit_check`** — §3.2's lead-time predicate:

  ```rust
  /// True once `now` is within `lead_minutes` of `earliest_bound_utc` --
  /// the leg's own `depart_after` (or `arrive_after` if no `depart_after`
  /// was set), converted to an absolute instant. Stays true for the rest
  /// of that leg's `service_date` (Judgment Call 3 -- the caller's own
  /// `service_date = today` query scoping is what eventually stops this
  /// from being consulted forever, not this function).
  pub fn is_due_for_commit_check(
      now: DateTime<Utc>,
      earliest_bound_utc: DateTime<Utc>,
      lead_minutes: i64,
  ) -> bool {
      now >= earliest_bound_utc - Duration::minutes(lead_minutes)
  }
  ```

- [ ] **Step 3: `pick_nearest_to_now_candidate`** — the crux mechanic (spec
  §3.2, 2026-09-22 addendum: "commit to whichever candidate is nearest to
  'now' at THAT point — not the earliest of all candidates"):

  ```rust
  /// Picks the index of the candidate scheduled time closest to `now_local`
  /// by absolute distance -- ties broken toward the EARLIER candidate (a
  /// deterministic, arbitrary-but-documented choice; the spec does not
  /// resolve exact-tie behavior and an exact tie is vanishingly unlikely
  /// against real CIF data, which never publishes two departures at the
  /// identical minute for the same origin/destination pair in practice).
  /// `None` only for an empty slice -- callers already filter to a leg with
  /// >= 1 candidate before calling this.
  pub fn pick_nearest_to_now_candidate(
      candidate_scheduled_times: &[chrono::NaiveTime],
      now_local: chrono::NaiveTime,
  ) -> Option<usize> {
      candidate_scheduled_times
          .iter()
          .enumerate()
          .min_by_key(|(_, t)| {
              let delta = t.signed_duration_since(now_local).num_seconds().abs();
              // Tie-break: (delta, scheduled_time) ordering makes an earlier
              // candidate win a tie, since NaiveTime: Ord.
              (delta, **t)
          })
          .map(|(i, _)| i)
  }
  ```

- [ ] **Step 4: `decide_unmatched_notification`** — the new notification's
  decision function, structurally the simplest in this file: no cold-start
  guard needed (there is no "prior state" ambiguity — a leg that has never
  had this checked has `already_notified = false` by construction), and no
  reverse transition to model (unlike `decide_skip_notification`, an
  unmatched leg either eventually commits — leaving this check's own WHERE
  clause forever — or stays unmatched for the rest of its `service_date`;
  there is no "un-notify" case):

  ```rust
  /// Fires once, the first time an `'auto'`-mode leg's commit-check finds
  /// zero candidates for today; never re-fires for the same leg (§4.2,
  /// narrowly scoped per this plan's own Judgment Call 4).
  pub fn decide_unmatched_notification(already_notified: bool) -> NotifyDecision {
      if already_notified {
          NotifyDecision::Skip
      } else {
          NotifyDecision::NotifyNow
      }
  }
  ```

- [ ] **Step 5: tests**, in a new `#[cfg(test)] mod sweep_tests` at the
  bottom of the file, alongside the existing `tests`/`skip_notification_tests`
  modules:
  - `weekday_bit`: Monday → 1, Sunday → 64, a known midweek date → the right
    power of two (e.g. Wednesday → 4).
  - `is_due_for_commit_check`: false well before the lead window, false one
    minute before the lead boundary, true exactly at the lead boundary, true
    well after (window already passed).
  - `pick_nearest_to_now_candidate`: empty slice → `None`; a single
    candidate → index 0 regardless of distance; `now` exactly between two
    candidates equidistant → the earlier of the two wins (tie-break test);
    `now` after every candidate (all candidates in the past relative to now,
    a legitimate real case — e.g. materialization ran late) → the *latest*
    (least-far-in-the-past) candidate wins, not the first in the list —
    this specifically exercises the "not `'earliest'`" distinction the whole
    phase exists to implement, so name the test something like
    `nearest_to_now_prefers_the_least_stale_candidate_over_the_earliest_one_when_now_has_already_passed_every_candidate`.
  - `decide_unmatched_notification`: `false` → `NotifyNow`, `true` → `Skip`.

- [ ] **Verify:** `cargo test -p notifier decision -- --nocapture` (no DB
  needed for this task).
- [ ] **Commit:** `git add crates/notifier/src/decision.rs && git commit -m
  "feat(notifier): pure decision logic for the recurring-journey sweep"`.

---

## Task 2: `crates/notifier/src/config.rs` — two new constants

**Files:** modify `crates/notifier/src/config.rs`.

Independent of Task 1; both can land in either order, but Task 2 is listed
second because Task 4/5 need both.

- [ ] **Step 1:** add, mirroring `train_delay_threshold_minutes`'s doc-comment
  style exactly (`crates/notifier/src/config.rs:21-24`) and
  `skip_check_poll_interval_secs`'s "revisit with real usage" framing
  (`config.rs:36-44`):

  ```rust
  /// Cadence for the recurring-journey materialization sweep (spec §3.1) --
  /// one branch does BOTH the daily mint (stage 1) and the auto-commit
  /// lead-time check (stage 2), per the spec's own "checked on the same
  /// hourly cadence" wording (§3.2's 2026-09-22 addendum) -- not two
  /// separate intervals. A reasonable-sounding, not load-tested figure,
  /// same "revisit with real usage" posture as this crate's other interval
  /// constants.
  #[arg(long, env, default_value_t = 3600)]
  pub template_sweep_poll_interval_secs: u64,

  /// Spec §3.2 (2026-09-22 addendum): an `'auto'`-mode leg's commit-check
  /// only runs once "now" is within this many minutes of the leg's earliest
  /// window bound (`depart_after` if set, else `arrive_after`) -- NOT at
  /// materialization time, which is what makes `'nearest_to_now'` mean
  /// something different from `'earliest'` (see that section's own worked
  /// reasoning for why committing immediately would make the two rules
  /// degenerate into the same behavior). 120 (2 hours ahead of the window)
  /// is the spec's own suggested starting default, explicitly flagged there
  /// as "this document's own suggestion, not a second product decision" --
  /// i64, not i32, to pair directly with `chrono::Duration::minutes` the
  /// same way `cooldown_minutes` already does.
  #[arg(long, env, default_value_t = 120)]
  pub auto_commit_lead_minutes: i64,
  ```

- [ ] **Verify:** `cargo build -p notifier`.
- [ ] **Commit:** `git add crates/notifier/src/config.rs && git commit -m
  "feat(notifier): add config constants for the recurring-journey sweep"`.

---

## Task 3: Migration — dedup state for the new notification class

**Files:** new
`crates/api/migrations/20260922140000_journey_leg_notification_state_unmatched.sql`.

Adds two nullable columns to the existing, already-shipped
`journey_leg_notification_state` table (`20260922130000_journey_leg_notification_state.sql`)
rather than a new table — reuses the existing `(user_id, journey_leg_id)`
primary key, which is exactly the right dedup granularity for this new
notification too (§4.2 explicitly leaves "a new boolean column or a sibling
table" as an implementation detail).

- [ ] **Step 1:**

  ```sql
  -- -------------------------------------------------------------------------
  -- Dedup state for the new "today's occurrence needs attention" push
  -- notification (spec §4.2, narrowly scoped to the auto-commit-found-zero-
  -- candidates case only -- see
  -- docs/superpowers/plans/2026-09-22-reusable-journeys-phaseC-recurrence-plan.md
  -- Judgment Call 4). Nullable, unlike journey_leg_notification_state's
  -- existing last_notified_skipped/last_notified_at (NOT NULL): a row
  -- written first by the skip-check path (Phase 3) has no opinion yet on
  -- whether THIS leg was ever unmatched-notified, and vice versa -- NULL
  -- means "never notified for this reason," matching skip_notification_state's
  -- own Option<bool>-from-NULL read convention
  -- (crates/notifier/src/queries.rs::skip_notification_state).
  -- -------------------------------------------------------------------------

  ALTER TABLE journey_leg_notification_state
      ADD COLUMN last_notified_unmatched    BOOLEAN,
      ADD COLUMN last_notified_unmatched_at TIMESTAMPTZ;
  ```

- [ ] **Verify:** apply against a local Postgres (`sqlx migrate run` or
  however this repo's dev stack applies migrations — check
  `crates/api`'s own README/justfile for the exact command used elsewhere in
  this repo), then `\d journey_leg_notification_state` to confirm both
  columns landed nullable.
- [ ] **Commit:** `git add crates/api/migrations/20260922140000_journey_leg_notification_state_unmatched.sql
  && git commit -m "feat(db): add unmatched-notification dedup columns to journey_leg_notification_state"`.

---

## Task 4: `crates/notifier/src/queries.rs` — the sweep's two stages

**Files:** modify `crates/notifier/src/queries.rs`.

Depends on Tasks 1-3. This is the largest, most load-bearing task in this
plan — every SQL statement below is written out in full because the two-stage
commit mechanic is this phase's crux (per the task brief) and because Task 4's
stage-1 mint SQL is a **named assumption about Phase B** (see the Assumptions
section) that a human must be able to diff line-for-line against Phase B's
real implementation.

- [ ] **Step 1: stage-1 mint — `materialize_due_template_occurrence`,
  duplicating the assumed shape of Phase B's
  `journey_templates::materialize_template`.**

  ```rust
  /// One due template's `journeys`/`journey_legs` row(s), minted
  /// idempotently. Mirrors the ASSUMED shape of Phase B's own
  /// `crates/api/src/data/journey_templates.rs::materialize_template`
  /// (see this plan's own "Assumptions about Phase B" section) --
  /// deliberately duplicated, not imported, per `crates/notifier`'s hard
  /// "never depend on crates/api" constraint. A human integrator must diff
  /// this function's SQL against Phase B's real implementation once it
  /// exists.
  ///
  /// Every leg is minted `'unmatched'` regardless of the template's
  /// `default_match_mode` -- `'auto'`-vs-`'manual'` behavior is entirely
  /// this crate's stage-2 commit-check's job (Step 4 below), never decided
  /// at mint time (spec §3.2's own 2026-09-22 addendum: this is the whole
  /// point of the two-stage split).
  ///
  /// Idempotency: the INSERT's own `WHERE NOT EXISTS` guard, single
  /// statement, same idiom as `find_or_create_train`'s `ON CONFLICT DO
  /// UPDATE ... RETURNING` and `create_subscription_for_train`'s CTE --
  /// safe under this crate's normal single-process sequential-tick
  /// execution; a true concurrent double-mint is the same accepted,
  /// documented residual race `create_subscription_for_train`'s own doc
  /// comment already names for this codebase ("closes the ordinary repeat
  /// case, not a true concurrent double-submit").
  ///
  /// Returns `Ok(None)` if this template already has an occurrence for
  /// `today` (no-op, not an error) or if the template has zero legs (should
  /// be unreachable given Phase B's own validation, but defensively a no-op
  /// rather than a partially-minted journey).
  pub async fn materialize_due_template_occurrence(
      pool: &PgPool,
      template_id: i64,
      user_id: &str,
      custom_name: Option<&str>,
      today: chrono::NaiveDate,
  ) -> anyhow::Result<Option<i64>> {
      let legs: Vec<(i32, Option<String>, Option<String>, Option<chrono::NaiveTime>,
                      Option<chrono::NaiveTime>, Option<chrono::NaiveTime>, Option<chrono::NaiveTime>)> =
          sqlx::query_as(
              "SELECT leg_order, origin_crs, destination_crs, depart_after, depart_before, \
                      arrive_after, arrive_before \
               FROM journey_template_legs WHERE template_id = $1 ORDER BY leg_order",
          )
          .bind(template_id)
          .fetch_all(pool)
          .await?;
      if legs.is_empty() {
          return Ok(None);
      }

      let mut tx = pool.begin().await?;
      let journey_id: Option<i64> = sqlx::query_scalar(
          "INSERT INTO journeys (user_id, custom_name, source_template_id) \
           SELECT $1, $2, $3 \
           WHERE NOT EXISTS ( \
               SELECT 1 FROM journeys j JOIN journey_legs jl ON jl.journey_id = j.id \
               WHERE j.source_template_id = $3 AND jl.service_date = $4 \
           ) \
           RETURNING id",
      )
      .bind(user_id)
      .bind(custom_name)
      .bind(template_id)
      .bind(today)
      .fetch_optional(&mut *tx)
      .await?;

      let Some(journey_id) = journey_id else {
          tx.rollback().await?;
          return Ok(None); // already materialized today -- idempotent no-op
      };

      for (leg_order, origin_crs, destination_crs, depart_after, depart_before, arrive_after, arrive_before) in legs {
          sqlx::query(
              "INSERT INTO journey_legs \
                  (journey_id, leg_order, origin_crs, destination_crs, service_date, \
                   depart_after, depart_before, arrive_after, arrive_before, match_mode) \
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'unmatched')",
          )
          .bind(journey_id)
          .bind(leg_order)
          .bind(origin_crs)
          .bind(destination_crs)
          .bind(today)
          .bind(depart_after)
          .bind(depart_before)
          .bind(arrive_after)
          .bind(arrive_before)
          .execute(&mut *tx)
          .await?;
      }
      tx.commit().await?;
      Ok(Some(journey_id))
  }

  /// Every active template due to materialize `today` -- active, today's
  /// weekday bit set, today within [starts_on, ends_on]. Does NOT itself
  /// check the idempotency guard (that's `materialize_due_template_occurrence`'s
  /// own job, per-template) -- this just narrows the sweep's per-tick
  /// candidate set. `days_of_week IS NULL` (a one-shot, non-recurring
  /// template, spec §2.2) is correctly excluded by the AND below (NULL &
  /// anything is NULL, never non-zero).
  #[derive(Debug, sqlx::FromRow)]
  pub struct DueTemplate {
      pub id: i64,
      pub user_id: String,
      pub custom_name: Option<String>,
  }

  pub async fn due_templates_for(
      pool: &PgPool,
      today: chrono::NaiveDate,
  ) -> anyhow::Result<Vec<DueTemplate>> {
      let rows = sqlx::query_as::<_, DueTemplate>(
          "SELECT id, user_id, custom_name FROM journey_templates \
           WHERE active \
             AND days_of_week IS NOT NULL \
             AND (days_of_week & (1 << (EXTRACT(ISODOW FROM $1::date)::int - 1))) != 0 \
             AND $1::date >= COALESCE(starts_on, $1::date) \
             AND $1::date <= COALESCE(ends_on, $1::date)",
      )
      .bind(today)
      .fetch_all(pool)
      .await?;
      Ok(rows)
  }
  ```

- [ ] **Step 2: stage-2 candidate query — a slim, duplicated sibling of
  `crates/api::data::queries::search_journey_leg_candidates`.**

  ```rust
  /// A leg eligible for this cycle's commit-check -- `'unmatched'`,
  /// `service_date = today`, belonging to a journey whose source template
  /// has `default_match_mode = 'auto'`. Does NOT itself apply the
  /// `auto_commit_lead_minutes` lead-time gate (see `decision::is_due_for_commit_check`,
  /// applied per-row by the caller in Task 5) -- keeping that check in Rust,
  /// not SQL, keeps it unit-testable in isolation (Task 1) without a DB.
  #[derive(Debug, sqlx::FromRow)]
  pub struct CommitCheckLeg {
      pub journey_leg_id: i64,
      pub journey_id: i64,
      pub user_id: String,
      pub origin_crs: String,
      pub destination_crs: String,
      pub service_date: chrono::NaiveDate,
      pub depart_after: Option<chrono::NaiveTime>,
      pub depart_before: Option<chrono::NaiveTime>,
      pub arrive_after: Option<chrono::NaiveTime>,
      pub arrive_before: Option<chrono::NaiveTime>,
  }

  pub async fn unmatched_auto_legs_for_commit_check(
      pool: &PgPool,
      today: chrono::NaiveDate,
  ) -> anyhow::Result<Vec<CommitCheckLeg>> {
      let rows = sqlx::query_as::<_, CommitCheckLeg>(
          "SELECT jl.id AS journey_leg_id, jl.journey_id, j.user_id, \
                  jl.origin_crs, jl.destination_crs, jl.service_date, \
                  jl.depart_after, jl.depart_before, jl.arrive_after, jl.arrive_before \
           FROM journey_legs jl \
           JOIN journeys j ON j.id = jl.journey_id \
           JOIN journey_templates jt ON jt.id = j.source_template_id \
           WHERE jl.match_mode = 'unmatched' \
             AND jl.service_date = $1 \
             AND jt.default_match_mode = 'auto' \
             AND jl.origin_crs IS NOT NULL \
             AND jl.destination_crs IS NOT NULL \
             AND (jl.depart_after IS NOT NULL OR jl.arrive_after IS NOT NULL)",
      )
      .bind(today)
      .fetch_all(pool)
      .await?;
      Ok(rows)
  }

  /// One `(train_uid, scheduled departure at the leg's own origin)`
  /// candidate. A deliberately slimmed sibling of
  /// `crates/api::data::queries::search_journey_leg_candidates` -- drops
  /// that function's cursor pagination and leg-destination-arrival
  /// subqueries (this stage only needs enough to pick a train, never
  /// renders a candidate to a human), keeps its WHERE-clause reachability
  /// logic (a candidate's route must actually call at `destination_crs`
  /// after `origin_crs`) and window-bound logic verbatim, duplicated per
  /// this crate's established crate-boundary constraint. `LIMIT 100` is a
  /// safety cap, not true pagination -- this crate never needs a second
  /// page.
  pub async fn schedule_candidates_for_leg(
      pool: &PgPool,
      origin_crs: &str,
      destination_crs: &str,
      service_date: chrono::NaiveDate,
      depart_after: Option<chrono::NaiveTime>,
      depart_before: Option<chrono::NaiveTime>,
      arrive_after: Option<chrono::NaiveTime>,
      arrive_before: Option<chrono::NaiveTime>,
  ) -> anyhow::Result<Vec<(String, chrono::NaiveTime)>> {
      let rows: Vec<(String, chrono::NaiveTime)> = sqlx::query_as(
          "SELECT main.train_uid, main.scheduled \
           FROM schedule_destination_departures main \
           WHERE main.service_date = $1 \
             AND main.origin_crs = $2 \
             AND ($3::time IS NULL OR main.scheduled >= $3) \
             AND ($4::time IS NULL OR main.scheduled <= $4) \
             AND ( \
                   main.destination_crs = $5 \
                   OR EXISTS ( \
                       SELECT 1 FROM schedule_destination_departures stop \
                       WHERE stop.service_date = $1 AND stop.train_uid = main.train_uid \
                         AND stop.origin_crs = $5 \
                         AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled) \
                   ) \
             ) \
             AND ( \
                   ($6::time IS NULL AND $7::time IS NULL) \
                   OR ( \
                       main.destination_crs = $5 \
                       AND ($6::time IS NULL OR main.destination_arrival >= $6) \
                       AND ($7::time IS NULL OR main.destination_arrival <= $7) \
                   ) \
                   OR EXISTS ( \
                       SELECT 1 FROM schedule_destination_departures stop \
                       WHERE stop.service_date = $1 AND stop.train_uid = main.train_uid \
                         AND stop.origin_crs = $5 \
                         AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled) \
                         AND ($6::time IS NULL OR stop.calling_point_arrival >= $6) \
                         AND ($7::time IS NULL OR stop.calling_point_arrival <= $7) \
                   ) \
             ) \
           ORDER BY main.scheduled, main.train_uid \
           LIMIT 100",
      )
      .bind(service_date)
      .bind(origin_crs)
      .bind(depart_after)
      .bind(depart_before)
      .bind(destination_crs)
      .bind(arrive_after)
      .bind(arrive_before)
      .fetch_all(pool)
      .await?;
      Ok(rows)
  }
  ```

  > **Note for the implementer:** `search_journey_leg_candidates`'s real
  > WHERE clause is `crates/api/src/data/queries.rs:1673-1761` at the time
  > this plan was written — re-diff the reachability/window-bound predicates
  > above against the live version before implementing, in case that
  > function has changed since.

- [ ] **Step 3: duplicated train/subscription creation** (Judgment Call 6 —
  bare, not schedule-match-enriched, matching the existing manual-pick path's
  own shape):

  ```rust
  /// Duplicates `crates/api::data::trains::find_or_create_train` --
  /// necessarily, per this crate's crate-boundary constraint. Keep this in
  /// sync with that function's exact ON CONFLICT shape if it ever changes.
  pub async fn find_or_create_train(
      pool: &PgPool,
      train_uid: &str,
      service_date: chrono::NaiveDate,
  ) -> anyhow::Result<i64> {
      let row: (i64,) = sqlx::query_as(
          "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2) \
           ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid \
           RETURNING id",
      )
      .bind(train_uid)
      .bind(service_date)
      .fetch_one(pool)
      .await?;
      Ok(row.0)
  }

  /// Duplicates `crates/api::data::train_tracking::create_subscription_for_train`
  /// -- same CTE idempotency idiom, same accepted "ordinary repeat case
  /// only" concurrency caveat as the original's own doc comment states.
  pub async fn create_subscription_for_train(
      pool: &PgPool,
      trains_id: i64,
      user_id: &str,
  ) -> anyhow::Result<i64> {
      let row: (i64,) = sqlx::query_as(
          "WITH existing AS ( \
               SELECT id FROM train_subscriptions \
               WHERE user_id = $1 AND trains_id = $2 ORDER BY id LIMIT 1 \
           ), inserted AS ( \
               INSERT INTO train_subscriptions \
                   (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs) \
               SELECT $1, tr.id, tr.service_date, tr.origin_crs, tr.scheduled_departure, tr.destination_crs \
               FROM trains tr WHERE tr.id = $2 AND NOT EXISTS (SELECT 1 FROM existing) \
               RETURNING id \
           ) \
           SELECT id FROM existing UNION ALL SELECT id FROM inserted",
      )
      .bind(user_id)
      .bind(trains_id)
      .fetch_one(pool)
      .await?;
      Ok(row.0)
  }

  /// Commits a leg to a train working -- the auto-commit sibling of
  /// `crates/api::data::journeys::set_leg_train_subscription`, but sets
  /// `match_mode = 'auto'` (never `'manual'`) and is guarded by `AND
  /// match_mode = 'unmatched'` so a leg already committed by a concurrent
  /// tick (or since raced-and-lost) is a silent no-op, not a double write.
  pub async fn commit_leg_to_train(
      pool: &PgPool,
      journey_leg_id: i64,
      train_subscription_id: i64,
  ) -> anyhow::Result<bool> {
      let result = sqlx::query(
          "UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'auto' \
           WHERE id = $2 AND match_mode = 'unmatched'",
      )
      .bind(train_subscription_id)
      .bind(journey_leg_id)
      .execute(pool)
      .await?;
      Ok(result.rows_affected() > 0)
  }
  ```

- [ ] **Step 4: unmatched-notification dedup state** (Task 3's new columns):

  ```rust
  pub async fn unmatched_notification_state(
      pool: &PgPool,
      user_id: &str,
      journey_leg_id: i64,
  ) -> anyhow::Result<Option<bool>> {
      let row: Option<(Option<bool>,)> = sqlx::query_as(
          "SELECT last_notified_unmatched FROM journey_leg_notification_state \
           WHERE user_id = $1 AND journey_leg_id = $2",
      )
      .bind(user_id)
      .bind(journey_leg_id)
      .fetch_optional(pool)
      .await?;
      Ok(row.and_then(|(v,)| v))
  }

  /// Only touches the two unmatched-specific columns on conflict -- never
  /// clobbers `last_notified_skipped`/`last_notified_at` if the skip-check
  /// cycle (Phase 3) already has a row here. On a genuine first INSERT for
  /// this `(user_id, journey_leg_id)`, supplies `last_notified_skipped =
  /// FALSE` -- not a placeholder but the literally correct value: a still-
  /// unmatched leg has no bound train to be "skipped" against yet.
  pub async fn upsert_unmatched_notification_state(
      pool: &PgPool,
      user_id: &str,
      journey_leg_id: i64,
      at: chrono::DateTime<Utc>,
  ) -> anyhow::Result<()> {
      sqlx::query(
          "INSERT INTO journey_leg_notification_state \
              (user_id, journey_leg_id, last_notified_skipped, last_notified_at, \
               last_notified_unmatched, last_notified_unmatched_at) \
           VALUES ($1, $2, FALSE, $3, TRUE, $3) \
           ON CONFLICT (user_id, journey_leg_id) DO UPDATE SET \
             last_notified_unmatched = EXCLUDED.last_notified_unmatched, \
             last_notified_unmatched_at = EXCLUDED.last_notified_unmatched_at",
      )
      .bind(user_id)
      .bind(journey_leg_id)
      .bind(at)
      .execute(pool)
      .await?;
      Ok(())
  }
  ```

- [ ] **Step 5: tests** (`#[cfg(test)] mod sweep_tests`, DB-gated
  `#[ignore]`, matching every existing test in this file):
  - `materialize_due_template_occurrence_is_idempotent_on_a_second_call` —
    seed a template + one template leg, call twice for the same `today`;
    first call returns `Some(journey_id)`, second returns `None`; exactly
    one `journeys` row and one `journey_legs` row exist afterward. Mirrors
    `a_second_poll_over_an_unchanged_table_finds_no_new_candidates`'s own
    "run twice, assert no duplicate effect" shape
    (`crates/notifier/src/queries.rs:635`, cited by the spec's §3.1 itself as
    the precedent for this exact idempotency discipline).
  - `due_templates_for_respects_days_of_week_active_and_date_range` — seed
    four templates (active+today's-bit-set — due; inactive — not due;
    active+wrong-bit — not due; active+right-bit+`ends_on` yesterday — not
    due); assert exactly the first is returned.
  - `unmatched_auto_legs_for_commit_check_excludes_manual_mode_templates` —
    seed one `'auto'`-template leg and one `'manual'`-template leg, both
    `'unmatched'`, both `service_date = today`; assert only the `'auto'` one
    is returned.
  - `commit_leg_to_train_is_a_no_op_once_already_committed` — commit once
    (`rows_affected` implies `true`), commit again with a different
    `train_subscription_id` (`false`); assert the leg's
    `train_subscription_id` is still the FIRST value.
  - `unmatched_notification_state_round_trips_without_clobbering_skip_state`
    — seed a row via `upsert_skip_notification_state` first (Phase 3's
    existing function), then call `upsert_unmatched_notification_state`;
    assert `skip_notification_state` still reads the original value AND
    `unmatched_notification_state` reads `Some(true)`.

- [ ] **Verify:** `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
  cargo test -p notifier sweep_tests -- --ignored --test-threads=1`.
- [ ] **Commit:** `git add crates/notifier/src/queries.rs && git commit -m
  "feat(notifier): mint due template occurrences and auto-commit unmatched legs"`.

---

## Task 5: `crates/notifier/src/main.rs` — wire the new sweep branch

**Files:** modify `crates/notifier/src/main.rs`.

Depends on Tasks 1-4.

- [ ] **Step 1:** add a fourth `tokio::time::interval` and `tokio::select!`
  arm, mirroring the existing three exactly (`main.rs:51-95`):

  ```rust
  let mut template_sweep_interval =
      tokio::time::interval(Duration::from_secs(config.template_sweep_poll_interval_secs));
  // ... inside the loop's tokio::select! ...
  _ = template_sweep_interval.tick() => {
      let result = run_template_sweep_cycle(
          &pool,
          config.auto_commit_lead_minutes,
          &config.vapid_private_key,
          &config.vapid_subject,
      )
      .await;
      if let Err(err) = result {
          tracing::error!(error = ?err, "notifier template-sweep cycle failed; will retry next interval");
      }
  }
  ```

- [ ] **Step 2:** the cycle function itself, mirroring `run_skip_check_cycle`'s
  shape (`main.rs:327-378`) but running two stages:

  ```rust
  /// The recurring-journey materialization sweep's own cycle (Task 4/5,
  /// spec §3.1-3.2). "Today" is a plain Europe/London calendar date (see
  /// this plan's Architecture section for why not a rail day) -- computed
  /// once per tick and used for both stages.
  async fn run_template_sweep_cycle(
      pool: &PgPool,
      auto_commit_lead_minutes: i64,
      vapid_private_key: &str,
      vapid_subject: &str,
  ) -> anyhow::Result<()> {
      let now = Utc::now();
      let today = now.with_timezone(&chrono_tz::Europe::London).date_naive();

      // --- Stage 1: mint due occurrences ---
      for template in queries::due_templates_for(pool, today).await? {
          match queries::materialize_due_template_occurrence(
              pool,
              template.id,
              &template.user_id,
              template.custom_name.as_deref(),
              today,
          )
          .await
          {
              Ok(Some(journey_id)) => {
                  tracing::info!(template_id = template.id, journey_id, "materialized today's occurrence");
              }
              Ok(None) => {} // already materialized this cycle or a prior one today
              Err(err) => {
                  tracing::error!(error = ?err, template_id = template.id, "failed to materialize template occurrence; will retry next cycle");
              }
          }
      }

      // --- Stage 2: commit-check due unmatched auto legs ---
      for leg in queries::unmatched_auto_legs_for_commit_check(pool, today).await? {
          let Some(earliest_bound) = leg.depart_after.or(leg.arrive_after) else {
              continue; // guarded by the query's own WHERE, defensive only
          };
          let Some(earliest_bound_utc) =
              london_to_utc(leg.service_date.and_time(earliest_bound))
          else {
              continue; // nonexistent local time (spring-forward gap) -- best-effort, skip this tick
          };
          if !decision::is_due_for_commit_check(now, earliest_bound_utc, auto_commit_lead_minutes) {
              continue;
          }

          let candidates = queries::schedule_candidates_for_leg(
              pool,
              &leg.origin_crs,
              &leg.destination_crs,
              leg.service_date,
              leg.depart_after,
              leg.depart_before,
              leg.arrive_after,
              leg.arrive_before,
          )
          .await?;

          if candidates.is_empty() {
              let already_notified = queries::unmatched_notification_state(pool, &leg.user_id, leg.journey_leg_id)
                  .await?
                  .unwrap_or(false);
              if decision::decide_unmatched_notification(already_notified) != decision::NotifyDecision::NotifyNow {
                  continue;
              }
              let payload = NotificationPayload {
                  title: "Your recurring journey needs attention".to_string(),
                  body: format!(
                      "No {} to {} service was found for today within your usual window.",
                      leg.origin_crs, leg.destination_crs
                  ),
                  url: format!("/journeys/{}", leg.journey_id),
                  tag: format!("journey-leg-unmatched-{}", leg.journey_leg_id),
              };
              if send_to_all_subscriptions(pool, &leg.user_id, &payload, vapid_private_key, vapid_subject).await? {
                  queries::upsert_unmatched_notification_state(pool, &leg.user_id, leg.journey_leg_id, now).await?;
              }
              continue;
          }

          let now_local = now.with_timezone(&chrono_tz::Europe::London).time();
          let scheduled_times: Vec<chrono::NaiveTime> = candidates.iter().map(|(_, t)| *t).collect();
          let Some(winner_idx) = decision::pick_nearest_to_now_candidate(&scheduled_times, now_local) else {
              continue; // unreachable given the is_empty() check above, defensive only
          };
          let (train_uid, _) = &candidates[winner_idx];

          let trains_id = queries::find_or_create_train(pool, train_uid, leg.service_date).await?;
          let tracking_id = queries::create_subscription_for_train(pool, trains_id, &leg.user_id).await?;
          if !queries::commit_leg_to_train(pool, leg.journey_leg_id, tracking_id).await? {
              tracing::warn!(journey_leg_id = leg.journey_leg_id, "leg was committed by a concurrent tick before this one finished; skipping");
          } else {
              tracing::info!(journey_leg_id = leg.journey_leg_id, train_uid, "auto-committed leg to nearest-to-now candidate");
          }
      }

      Ok(())
  }

  /// Resolves a service_date + local wall-clock TIME to the UTC instant it
  /// names -- same `LocalResult` handling as
  /// `crates/api::data::eta_blend::london_to_utc` (duplicated, per this
  /// crate's crate-boundary constraint; that one is `pub(crate)` and
  /// unreachable from here anyway).
  fn london_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
      match chrono_tz::Europe::London.from_local_datetime(&naive) {
          chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
          chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
          chrono::LocalResult::None => None,
      }
  }
  ```

  > `chrono-tz` is not yet a `crates/notifier` dependency — add it to
  > `crates/notifier/Cargo.toml` (`chrono-tz = "0.10"`, matching `crates/common`'s
  > already-pinned version) as part of this task's Step 1.

- [ ] **Step 2: an end-to-end DB-gated test**, mirroring
  `run_cycle_notifies_a_real_transition_and_is_idempotent_on_replay`'s shape
  (`main.rs:588-653`): seed one `'auto'`-mode template + one template leg +
  a published `schedule_destination_departures` row inside the lead window,
  run `run_template_sweep_cycle` once, assert a `journeys` row was minted AND
  its leg was committed (`match_mode = 'auto'`, `train_subscription_id` set)
  in the same tick; run it a second time, assert nothing changes (no second
  `journeys` row, no re-commit). A second test seeds zero
  `schedule_destination_departures` rows for that date and asserts the
  `journey_leg_notification_state.last_notified_unmatched` row is written
  after one run, and NOT re-written (same `last_notified_unmatched_at`) after
  a second run within the same tick's window.

- [ ] **Verify:** `cargo fmt --all && cargo clippy --workspace --all-features
  --all-targets -- -D warnings && cargo test --workspace && DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres
  cargo test -p notifier -- --ignored --test-threads=1`.
- [ ] **Commit:** `git add crates/notifier/src/main.rs crates/notifier/Cargo.toml
  && git commit -m "feat(notifier): wire the recurring-journey sweep into the main loop"`.

---

## Task 6: `crates/api` — confirm/extend the template update route (conditional)

**Files:** `crates/api/src/routes/journey_templates.rs` (Phase B's assumed
file) — **only if** Phase B's real update route does not already accept
`daysOfWeek`/`active`/`startsOn`/`endsOn`/`defaultMatchMode`/`autoCommitRule`.

- [ ] **Step 1:** once Phase B is implemented, `grep` its update route's
  request struct (`PUT /JourneyTemplates/{id}` is assumed — confirm the
  actual method/path) for these six field names. If all six are already
  accepted and persisted as plain columns, **this task is a no-op — check the
  box and move on.**
- [ ] **Step 2 (only if fields are missing):** add the missing fields to that
  route's request DTO and its `UPDATE journey_templates SET ...` statement,
  following whatever validation convention Phase B established for its other
  fields (e.g. a `validate_window_leg`-style pure validator in
  `crates/api/src/data/journey_templates.rs` if Phase B added one). Do not
  restructure Phase B's route — add fields, following its existing shape.
- [ ] **Verify:** whatever Phase B's own test command for this route is
  (`cargo test -p api journey_templates -- --ignored --test-threads=1`,
  adjusted to Phase B's real test names).
- [ ] **Commit:** only if Step 2 was needed —
  `git commit -m "feat(api): accept recurrence fields on the journey template update route"`.

---

## Task 7: Frontend — day-of-week, match-mode, and Pause controls

**Files:** Phase B's assumed
`frontend/app/journeys/templates/[id]/page.tsx`/`frontend/components/JourneyTemplateForm.tsx`
(paths to be confirmed against Phase B's real implementation before this task
starts — see the Global Constraints file-scope note).

Depends on Phase B's frontend existing. Everything below assumes a form
component roughly shaped like `AddJourneyLegButton.tsx`
(`'use client'`, `useState`, a same-origin `fetch()` against the `/api/...`
proxy, `router.refresh()` on success) editing a `journey_templates` row.

- [ ] **Step 1: day-of-week picker.** No existing multi-select precedent in
  this app (spec §0.3 confirmed "no template/preset/recurrence concept
  exists anywhere") — use Mantine's `Chip.Group multiple` (already available,
  `@mantine/core` is already a dependency; no new package), seven `Chip`s
  labeled Mon–Sun, each `value` a string the component maps to/from the
  `days_of_week` bitmask on submit (`selectedDays.reduce((mask, day) => mask
  | dayBit[day], 0)`, mirroring Task 1's `weekday_bit` convention — Mon=1
  .. Sun=64 — in TypeScript). An empty selection means "not recurring" (`null`
  sent for `daysOfWeek`, matching §2.2's own "NULL = a one-shot template"
  semantics) — surfaced as a distinct "Not recurring" state, not a
  disabled-until-picked control.
- [ ] **Step 2: match-mode choice — two-way, not three-way** (Judgment Call
  2 — deviates from the spec's own now-superseded §6 item 3 wording):
  `SegmentedControl` with exactly two options, mirroring
  `AddJourneyLegButton.tsx`'s own `LegMode` `SegmentedControl` pattern
  (`AddJourneyLegButton.tsx:175-183`):
  ```tsx
  <SegmentedControl
    value={matchMode}
    onChange={(v) => setMatchMode(v as 'manual' | 'auto')}
    data={[
      { label: "Remind me, don't guess", value: 'manual' },
      { label: 'Auto-commit for me', value: 'auto' },
    ]}
  />
  ```
  On submit: `defaultMatchMode: matchMode`, and `autoCommitRule:
  matchMode === 'auto' ? 'nearest_to_now' : null` — never user-facing, never
  a third option, never `'earliest'`.
- [ ] **Step 3: Pause toggle.** A Mantine `Switch` bound to `active`
  (inverted label if desired — "Paused" reads more naturally than "Active"
  for a toggle a user reaches for to STOP something), submitting `active:
  !paused` on change. No confirmation dialog (reversible, non-destructive,
  same posture as this app's other toggles).
- [ ] **Step 4:** wire all three to whatever update call Phase B's page
  already makes (or Task 6 extended) — `PUT /JourneyTemplates/{id}` (assumed)
  with the new fields added to its existing body.
- [ ] **Tests:** a vitest file for the new controls in isolation
  (`JourneyTemplateForm.test.tsx` or wherever Phase B's own test lives),
  covering: toggling a day flips the right bit; deselecting every day sends
  `null`; switching to `'auto'` always sends `autoCommitRule: 'nearest_to_now'`;
  switching back to `'manual'` sends `autoCommitRule: null`.
- [ ] **Verify:** `npm test -- <file>`, then a full `npm test && npm run
  build`. Manually verify in a real browser against a dev stack with a
  seeded template: create a Mon–Fri, auto-commit template; confirm the
  detail page round-trips the day selection and match-mode after a save +
  reload.
- [ ] **Commit:** `git add frontend/... && git commit -m "feat(frontend):
  recurrence, match-mode, and pause controls for journey templates"`.

---

## Final verification (all tasks complete)

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-features --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo
  test -p notifier -- --ignored --test-threads=1`
- [ ] `npm test && npm run build` (in `frontend/`)
- [ ] Manual end-to-end smoke test against a real dev stack: seed an
  `'auto'`-mode, Mon–Sun template with a published schedule inside
  `auto_commit_lead_minutes` of "now"; run the notifier binary once (or wait
  one `template_sweep_poll_interval_secs` tick); confirm a `journeys` row
  appears with a committed leg (`match_mode = 'auto'`); separately seed a
  template whose window has no published service that day; confirm exactly
  one push notification fires and the leg stays `'unmatched'`.
