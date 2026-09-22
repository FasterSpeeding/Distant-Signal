# Plan: Journey Tracking Phase 3 — Journey-Aware Notifications + Station-Skip Detection

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 3 of
`docs/superpowers/specs/2026-09-22-journey-tracking-design.md`'s §9 phased
plan — and *only* Phase 3 — end to end: journey/leg-aware
`NotificationPayload` copy for the notifier's existing, fully-reused
delay/cancellation decision logic (§5.1), and the one genuinely new piece in
the whole spec, per its own complexity assessment: station-skip detection
for a specific tracked train's own leg (§5.2), wired into both the journey
view's skip badge (§4) and a new notifier decision path with its own dedup
state table and `main.rs` interval branch. Phases 1 (single-leg journeys +
migration + time-window search), 2 (multi-leg chaining) and 4 (group
sharing) are **out of scope** — they are being planned by sibling agents in
parallel and are not touched here beyond the one explicit dependency named
below.

## ⚠️ External dependency on Phase 1 — read before Task 1

This plan assumes Phase 1 has already landed the following, exactly as
specified in §1.1 of the design spec (authoritative even though Phase 1's
own implementation plan doesn't exist yet at the time this plan was
written):

- A `journeys` table: `id BIGSERIAL PK, user_id TEXT NOT NULL REFERENCES
  users(id), custom_name TEXT, created_at, updated_at`.
- A `journey_legs` table: `id BIGSERIAL PK, journey_id BIGINT NOT NULL
  REFERENCES journeys(id) ON DELETE CASCADE, leg_order INT, origin_crs TEXT,
  destination_crs TEXT` (per §7.1's own correction to §1.1, these two are
  **nullable** — empty for an NR-primary subscription whose pin CRS was
  never populated — so every query this plan adds against them must handle
  `NULL`, never assume `NOT NULL`), `service_date DATE, depart_after TIME,
  depart_before TIME, arrive_after TIME, arrive_before TIME,
  train_subscription_id BIGINT REFERENCES train_subscriptions(id) ON DELETE
  SET NULL, match_mode TEXT, created_at`, unique `(journey_id, leg_order)`.
- The historical-data migration (§7.1) has wrapped every pre-existing
  `train_subscriptions` row in its own one-row `journeys`/`journey_legs`
  pair, so **by the time Phase 3 ships, every resolvable tracked train has
  exactly one `journey_legs` row pointing at it** (mod the one edge case
  named in Task 6 below: two legs idempotently sharing one
  `train_subscription_id`).
- A `GET /Journeys/{id}` route (likely `crates/api/src/routes/journeys.rs`,
  data layer likely `crates/api/src/data/journeys.rs` per the spec's own
  front-matter naming convention) returning `{journey, legs: [{...,
  trackedTrainState?: TrackedTrainState}]}`, joining each matched leg
  straight into the **existing** `TRACKED_TRAIN_STATE_SELECT`
  (`crates/api/src/data/train_tracking.rs:1151-1168`) per §4.
- A frontend route `frontend/app/journeys/[id]/page.tsx` rendering one card
  per leg, ascending `leg_order`, embedding `<TrainJourney state={...} />`
  for each matched leg.

**Because Phase 1's actual plan/code doesn't exist yet, Task 4 and Task 5
below (the two tasks that touch this surface) are written as precisely as
the spec allows but cannot cite exact file:line locations the way every
other task in this plan does.** Whoever executes this plan after Phase 1
has landed for real must locate the actual function/route/component names
Phase 1 produced and adjust the two tasks' insertion points accordingly —
the *shape* of what to add (a `legSkip` field on the leg response, a
`skippedCrs` prop threaded into `TrainJourney`/`JourneyTimeline`) does not
change; only *where exactly* it plugs in might.

**Explicitly NOT assumed from Phase 1:** multi-leg chaining UI/routes
(Phase 2), `'auto'` match mode, group sharing (Phase 4), platform display.
None of this plan's tasks touch any of those.

---

## Architecture

Two independent halves, matching the spec's own §5.1/§5.2 split:

**§5.1 (copy only, Task 6).** The notifier's existing per-`trains_id`
fan-out (`candidates_for_trains_id`, `crates/notifier/src/queries.rs:145-204`)
and escalation-only decision function (`decide_train_notification`,
`crates/notifier/src/decision.rs:83-89`) are untouched — this plan adds
exactly one new read (`journey_leg_for_train_subscription`) consulted by
`notify_train_candidates` (`crates/notifier/src/main.rs:169-221`) when
building the `NotificationPayload`, with a hard fallback to today's exact
copy/URL whenever no `journey_legs` row is found, or the leg's journey has
only one leg (see Task 6's own Judgment Call).

**§5.2 (new detection, Tasks 1-3 and 7-9).** `crates/notifier` does **not**
depend on `crates/api`
(confirmed: `crates/notifier/Cargo.toml`'s `[dependencies]` lists only
`anyhow, chrono, clap, common, dotenv, serde, serde_json, sqlx, tokio,
tracing, tracing-subscriber, web-push` — no `api` path dependency) — so the
spec's own citation of `eta_blend::find_darwin_eta`
(`crates/api/src/data/eta_blend.rs:22-`) as the model to build the skip
check on **cannot mean literal code reuse across the crate boundary**. This
plan resolves that by extracting the one piece of `find_darwin_eta` that
*is* crate-boundary-safe — the pure "match a sample to this service by its
reported destination" step, and a new one-line "does the matched entry skip
this CRS" predicate — into `crates/common` (Task 1), which both `crates/api`
and `crates/notifier` already depend on. Each crate then writes its own,
independent async "fetch the row(s), apply the shared pure logic" wrapper
(`crates/api/src/data/station_skip.rs`, Task 3; `crates/notifier/src/skip_check.rs`,
Task 9) against its own crate's own query layer — the SQL fetch is
necessarily duplicated (no shared data-access crate exists in this
workspace for either side to share), but the *decision* of "is this CRS
among today's skipped calling points" is written exactly once and can never
quietly drift between the journey-view badge and the push notification.

**The symmetric origin-skip check's anchor point** (§5.2 point 1's "a sample
taken further back down the line, if one exists"): resolved as `trains.origin_crs`
(a plain, already-existing column, `crates/api/migrations/20260906100000_trains.sql:19`,
nullable) — the bound train's own true, full-route point of origin — rather
than any dynamically-derived "some upstream station" (which would need the
train's full calling-point list and pick-the-station-before-X logic neither
crate has cheap, crate-boundary-safe access to). This anchor is only
consulted when it differs from the leg's own `origin_crs` (i.e. the
traveller boarded partway through a through-service) — see Task 3's doc
comment.

---

## Tech stack

Rust/axum/sqlx (`crates/api`, `crates/notifier`, both depending on
`crates/common`), Next.js/React/Mantine (`frontend`), Postgres.

## Spec

`docs/superpowers/specs/2026-09-22-journey-tracking-design.md` — §4 (skip
badge), §5 (notifications), §9 (Phase 3 scope) are the load-bearing
sections; §0.3 and §1.1 establish the baseline this plan builds on. This
plan does not re-argue anything the spec already settled.

---

## Judgment calls this plan makes (read before Task 1)

1. **`common::match_darwin_departure`/`common::departure_skips_station` are
   new shared pure functions, and `eta_blend::find_darwin_eta` is refactored
   to call the first one, rather than notifier duplicating that matching
   logic from scratch.** Forced by the crate-boundary fact above — without
   this, the API's skip badge and the notifier's skip notification could
   define "matches this service" two subtly different ways with no test
   ever catching the drift. The refactor is behavior-preserving (verified by
   `find_darwin_eta`'s own existing test suite, which this plan re-runs
   unchanged in Task 1) — it does not change what any existing caller sees.

2. **Origin-skip check's anchor is `trains.origin_crs`, not a dynamically
   walked "previous station on the route."** See Architecture above. Named
   explicitly because the spec's own wording ("a sample taken further back
   down the line, if one exists") is deliberately vague about *which*
   upstream station — this plan picks the one upstream station both crates
   can already resolve with a single extra column read, no route-walking.

3. **Skip polling is bounded to `journey_legs.service_date = <today>`, not
   every committed leg ever.** Unlike the existing delay/cancellation cycles
   (which are event-driven off `train_movement_events`/the forward queue and
   naturally stop mattering once a train finishes), `station_samples` is a
   wholesale-replaced *current snapshot*, not an append log
   (`crates/api/src/data/queries.rs:1620-1644`'s own doc comment: "one row
   per station, no history"). There is nothing to diff against a watermark,
   so this plan's third cycle does a fresh full poll every interval instead
   — bounding it to today's committed legs keeps that poll's row count
   proportional to "journeys actually happening today," not every journey
   ever tracked.

4. **Notification audience is journey-owner-only**, matching the spec's own
   §5.3 recommendation and its explicit "Phase 1" framing there (read here
   as "the first phase that ships any journey-notification concept at all,"
   which is this Phase 3, not literally journey-tracking's own Phase 1) —
   `journeys.user_id` is the sole recipient; no group-member fan-out exists
   yet because Phase 4 (group sharing) hasn't landed.

5. **§5.1's copy change only fires for a journey with `total_legs > 1`.**
   The spec's own example ("Leg 2 of 'Weekend in Edinburgh'...") is
   unambiguously about a genuine multi-leg journey. Every existing tracked
   train, after Phase 1's migration, sits inside a *one*-leg journey — for
   that overwhelming common case, "Leg 1 of 1" adds no information a user
   didn't already have, so this plan treats a one-leg journey exactly like
   the spec's own documented "no `journey_legs` row" fallback: today's exact
   copy, byte-for-byte. This is a deliberately conservative reading that
   keeps single-train tracking's notification UX completely unchanged,
   correctly matching §1.2's own table ("Delay/cancellation detection +
   notification: Reused entirely").

6. **Notification copy for a real multi-leg leg uses raw CRS codes
   (`WAV → KGX`), not resolved station names.** `crates/notifier` has no
   existing precedent for joining `stations` for a display name (today's
   train notification copy never names a station at all — "Your train is
   delayed", no origin/destination mentioned). Adding a `stations` join to a
   background service crate whose whole existing copy style is deliberately
   terse is disproportionate to what this phase needs; CRS codes are
   already meaningful to anyone who set up the leg in the first place.

7. **The skip badge is a new optional prop threaded through
   `TrainJourney`/`JourneyTimeline`, not a new field baked into
   `TrackedTrainState`/`GET /Train/{trackingId}`.** Verified directly
   (Task 5's own research) that `JourneyTimeline.tsx` has **no existing
   precedent at all** for a cancelled/skipped-stop badge — the spec's own
   §4 wording ("same visual family as the existing cancelled-stop styling
   that component presumably already has for a `PASS`-kind stop") turned
   out to be an unverified assumption; no such styling exists anywhere in
   that file today (confirmed: `grep -n "cancelled\|Cancelled\|PASS"
   frontend/components/JourneyTimeline.tsx` returns nothing). The closest
   real precedent in this app for a disruption-flavoured badge is
   `TrackedTrainStatusBadge.tsx:50-52`'s `<Badge color={train.status ===
   'cancelled' ? 'red' : 'gray'} variant="light">` and `TrainJourney.tsx:171`'s
   `<Alert color="red" title="Cancelled">` — this plan's new badge follows
   that exact `color="red" variant="light"` convention. Threading it as an
   **optional** prop (default `undefined`, following `JourneyTimeline`'s own
   existing `endpointNames?: JourneyEndpointNames` precedent exactly) keeps
   every non-journey caller of `TrainJourney`/`JourneyTimeline`
   (`app/train/by-id/[trackingId]/page.tsx`, `app/train/[uid]/[date]/page.tsx`)
   byte-for-byte unchanged — this plan does not touch either of those pages.
   Station-skip detection stays scoped to the leg's own `origin_crs`/
   `destination_crs` (only defined once `journey_legs` exists), matching
   this plan's own scope discipline of touching only the `/journeys/*`
   surface, not `/Train/*`/`/train/*`.

8. **CRS comparisons throughout this plan's new code are
   `eq_ignore_ascii_case`**, matching `eta_blend::find_darwin_eta`'s own
   existing convention (`eta_blend.rs:32`) rather than assuming every CRS
   string arriving from a user-typed leg or a Darwin sample is
   already-normalized uppercase.

---

## Non-goals

(Explicitly out of scope for this plan — no task below touches any of
these.)

- **Phases 1, 2, 4 of the design spec** — single-leg journey creation,
  multi-leg chaining, `'auto'` match mode, group sharing. This plan only
  ever *reads* `journeys`/`journey_legs` rows Phase 1 is assumed to have
  already created; it creates no journey/leg itself.
- **Connection buffers** (§3) — explicitly deferred past Phase 1 by the
  spec itself; nothing here touches it.
- **Platform display** (§0.6) — blocked on unrelated in-flight work per the
  spec; not touched.
- **No change to `decide_train_notification`'s escalation logic itself**,
  or to `train_notification_state`'s dedup shape — §5.1 is copy-only, per
  the spec's own framing ("fully reused, no new decision logic").
- **No `frontend/public/sw.js` change.** Verified directly: the push
  handler (`sw.js:138-158`) is fully generic — it renders whatever
  `{title, body, url, tag}` it's handed with no train/journey-specific
  logic at all. A journey-aware notification is purely a differently-worded
  backend payload; nothing client-side needs to change for it to render
  correctly.
- **No retry/backoff/delivery changes to `crates/notifier/src/send.rs`.**
  The skip notification reuses `send_to_all_subscriptions`
  (`crates/notifier/src/main.rs:268-292`) exactly as-is.
- **No cross-owner/group-shared audience for a journey's notifications**
  (§5.3's own open question) — owner-only, per Judgment Call 4.
- **No frontend UI for `/journeys/{id}` beyond the skip badge itself** —
  the page's overall layout, leg cards, "Change train" action, etc. are all
  Phase 1/2 deliverables this plan does not touch.

## Global Constraints

- **Every new migration must sort after whatever timestamp Phase 1's own
  `journeys`/`journey_legs` migration lands at.** This plan's Task 2 uses
  `20260922130000` as a placeholder, chosen because it sorts after the
  latest migration that exists as of this plan's writing
  (`20260917090000_incidents_affected_lines.sql`, confirmed via `ls
  crates/api/migrations | sort | tail -5`) — **re-verify this at
  implementation time** with the same command and bump the timestamp if
  Phase 1's migrations have since claimed a later slot.
- **`crates/notifier` must never gain a dependency on `crates/api`.** Every
  new notifier-side query in this plan is a fresh, independent SQL
  statement against tables Phase 1/this plan create, never an import of
  `crates/api`'s data-layer functions — see Architecture above for why, and
  Task 1 for the one piece of logic this plan does share (via `crates/common`,
  which both crates already depend on).
- **`journey_legs.origin_crs`/`.destination_crs` are nullable** (§7.1's
  correction) — every query/function this plan adds that reads them must
  filter out or otherwise handle `NULL`, never assume `NOT NULL`.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (this repo's CI invocation,
  `.github/workflows/ci.yml`'s `clippy` job), `cargo test --workspace`
  (ignored tests skipped, matching CI's fast default), and `cargo test -p
  api -- --ignored --test-threads=1` / `cargo test -p notifier --
  --ignored --test-threads=1` for every DB-gated test this plan adds
  (CI's own invocation, requiring `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres`
  against a real local Postgres — see `.github/workflows/ci.yml`'s
  `services:` block). Frontend: `npm test -- <file>` (vitest) per changed
  test file, plus a full `npm test` and `npm run build` before considering
  the frontend tasks done. **UI verification**: per this repo's standing
  practice, start the dev stack and manually verify in a real browser —
  Task 10 is dedicated to this.
- **File scope.** Modified/created:
  `crates/common/src/lib.rs`,
  `crates/api/migrations/20260922130000_journey_leg_notification_state.sql` (new,
  timestamp subject to the re-verification note above),
  `crates/api/src/data/eta_blend.rs`,
  `crates/api/src/data/station_skip.rs` (new),
  `crates/api/src/data/journeys.rs` (Phase 1's file, Task 4 only — adds one
  field/call, does not restructure it),
  `crates/api/src/routes/journeys.rs` (Phase 1's file, Task 4 only),
  `crates/notifier/src/config.rs`,
  `crates/notifier/src/decision.rs`,
  `crates/notifier/src/queries.rs`,
  `crates/notifier/src/main.rs`,
  `crates/notifier/src/skip_check.rs` (new),
  `frontend/lib/types.ts`,
  `frontend/components/JourneyTimeline.tsx`,
  `frontend/components/TrainJourney.tsx`,
  `frontend/app/journeys/[id]/page.tsx` (Phase 1's file, Task 5 only).
  No other file changes.

---

## Task 1: `crates/common` — shared Darwin-departure matching + skip predicate

**Files:** modify `crates/common/src/lib.rs`, `crates/api/src/data/eta_blend.rs`.

Independent first task — nothing else in this plan compiles without it.
Extracts the crate-boundary-safe core of `find_darwin_eta`'s matching logic
(see Architecture) so `crates/notifier` (Task 9) can share it without
depending on `crates/api`.

- [ ] **Step 1: Add the two shared functions**, directly below the
  `StationDeparture` struct (`crates/common/src/lib.rs:426-449`):

```rust
/// Finds the live Darwin/LDBWS departure-board sample matching a specific
/// service among several sampled at one station, by its reported
/// destination CRS. This is the shared matching step behind both
/// `eta_blend::find_darwin_eta`'s ETA overlay (`crates/api`) and this
/// crate's own `departure_skips_station` (used by both `crates/api`'s
/// journey-view skip badge and `crates/notifier`'s skip notification) --
/// it lives here, not in `eta_blend.rs`, because `crates/notifier` cannot
/// depend on `crates/api` (see that crate's own `skip_check.rs` doc
/// comment) but both crates already depend on this one. Never matches an
/// already-cancelled departure -- whole-service cancellation is a distinct,
/// already-handled signal (`train_current_state.status = 'cancelled'`, via
/// TRUST, not Darwin), not this function's job. Case-insensitive
/// (`eq_ignore_ascii_case`), matching every other CRS comparison in this
/// codebase that isn't guaranteed pre-normalized.
pub fn match_darwin_departure<'a>(
    samples: &'a [StationDeparture],
    target_destination: Option<&str>,
) -> Option<&'a StationDeparture> {
    let target = target_destination?;
    samples
        .iter()
        .find(|d| !d.is_cancelled && d.destination_crs.eq_ignore_ascii_case(target))
}

/// Whether a matched Darwin departure reports `crs` as one of today's
/// skipped calling points (Darwin's per-calling-point `isCancelled`,
/// `StationDeparture::skipped_stations`) -- the one-line predicate shared
/// by every caller of [`match_darwin_departure`] that cares about skips, so
/// "how do we test skipped_stations membership" has exactly one
/// implementation, not several independently-written
/// `eq_ignore_ascii_case` loops that could quietly drift apart.
pub fn departure_skips_station(matched: &StationDeparture, crs: &str) -> bool {
    matched
        .skipped_stations
        .iter()
        .any(|skipped| skipped.eq_ignore_ascii_case(crs))
}

#[cfg(test)]
mod darwin_departure_matching_tests {
    use super::*;

    fn departure(destination_crs: &str, is_cancelled: bool, skipped: Vec<&str>) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "18:32".to_string(),
            estimated: "18:41".to_string(),
            is_cancelled,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: skipped.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn matches_by_destination_case_insensitively() {
        let samples = vec![departure("wok", false, vec![])];
        assert!(match_darwin_departure(&samples, Some("WOK")).is_some());
    }

    #[test]
    fn never_matches_a_cancelled_departure() {
        let samples = vec![departure("WOK", true, vec![])];
        assert!(match_darwin_departure(&samples, Some("WOK")).is_none());
    }

    #[test]
    fn no_target_destination_means_no_match() {
        let samples = vec![departure("WOK", false, vec![])];
        assert!(match_darwin_departure(&samples, None).is_none());
    }

    #[test]
    fn departure_skips_station_is_case_insensitive() {
        let matched = departure("WOK", false, vec!["rdg"]);
        assert!(departure_skips_station(&matched, "RDG"));
        assert!(!departure_skips_station(&matched, "WAT"));
    }

    #[test]
    fn empty_skipped_stations_never_matches_anything() {
        let matched = departure("WOK", false, vec![]);
        assert!(!departure_skips_station(&matched, "RDG"));
    }
}
```

- [ ] **Step 2: Refactor `find_darwin_eta` to use `match_darwin_departure`**
  (`crates/api/src/data/eta_blend.rs:22-36`), behavior-preserving:

```rust
pub fn find_darwin_eta(
    samples: &[StationDeparture],
    pin_destination_crs: Option<&str>,
    next_calling_point: Option<&str>,
    service_date: NaiveDate,
) -> Option<DateTime<Utc>> {
    let target_destination = pin_destination_crs.or(next_calling_point)?;
    let matched = common::match_darwin_departure(samples, Some(target_destination))?;
    let time = NaiveTime::parse_from_str(&matched.estimated, "%H:%M").ok()?;
    london_to_utc(service_date.and_time(time))
}
```

  This changes zero observable behavior (the `!d.is_cancelled &&
  d.destination_crs.eq_ignore_ascii_case(target)` predicate is identical to
  what `match_darwin_departure` now implements) — `eta_blend.rs`'s own
  existing test suite (`eta_blend.rs:61-156`) must all still pass unchanged
  and is the proof of that.

- [ ] **Step 3: Verify**

```bash
cargo test -p common darwin_departure_matching_tests
cargo test -p api --lib eta_blend
cargo build --workspace
```

  Expected: all new `common` tests pass; every existing `eta_blend` test
  (`no_target_destination_means_no_darwin_eta`,
  `matches_by_pinned_destination_and_parses_hhmm`,
  `falls_back_to_next_calling_point_when_no_pinned_destination`,
  `a_winter_estimate_is_utc_because_gmt_is_utc`,
  `a_nonexistent_local_time_yields_no_eta_rather_than_a_guess`,
  `an_ambiguous_local_time_takes_the_first_occurrence`,
  `a_cancelled_departure_never_matches`,
  `on_time_yields_no_concrete_eta_to_prefer_over_trust`) still passes
  unchanged; the workspace builds clean.

- [ ] **Step 4: Commit**

```bash
git add crates/common/src/lib.rs crates/api/src/data/eta_blend.rs
git commit -m "common: extract match_darwin_departure/departure_skips_station, shared by api and notifier for station-skip detection"
```

---

## Task 2: Migration — `journey_leg_notification_state`

**Files:** create `crates/api/migrations/20260922130000_journey_leg_notification_state.sql`.

The dedup table for §5.2's skip-notification decision path (Task 8/9),
mirroring `train_notification_state`'s exact shape
(`crates/api/migrations/20260902100000_notifications.sql:26-32`) one level
finer, keyed on `journey_leg_id` rather than `tracked_train_id`. **Cannot be
applied/tested until Phase 1's `journeys`/`journey_legs` tables exist** — if
executing this plan before Phase 1 has landed, this migration will fail at
`sqlx` migrate time with an undefined-table error against the
`REFERENCES journey_legs(id)` clause; that is expected, not a bug in this
migration.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Per-(user, journey leg) dedup state for station-skip push notifications
-- (§5.2, docs/superpowers/specs/2026-09-22-journey-tracking-design.md).
-- Same escalation-only "written only after a successful send" discipline
-- as train_notification_state (20260902100000_notifications.sql) and
-- line_notification_state -- crates/notifier's own decide_skip_notification
-- fires only on last_notified_skipped false -> true, never the reverse, and
-- this row is only ever upserted after send_to_all_subscriptions actually
-- succeeded (or the user has zero push_subscriptions -- "still counts as
-- handled", crates/notifier/src/main.rs's own send_to_all_subscriptions doc
-- comment), never before -- an unresolved send failure retries at the next
-- poll cycle rather than being queued.
-- -------------------------------------------------------------------------

CREATE TABLE journey_leg_notification_state (
    user_id                TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    journey_leg_id         BIGINT      NOT NULL REFERENCES journey_legs(id) ON DELETE CASCADE,
    last_notified_skipped  BOOLEAN     NOT NULL,
    last_notified_at       TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, journey_leg_id)
);
```

- [ ] **Step 2: Verify** (only once Phase 1's `journeys`/`journey_legs`
  tables exist in the target database):

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d journey_leg_notification_state"
```

  Expected: no `sqlx::migrate::MigrateError`; the table exists with the
  shape above.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260922130000_journey_leg_notification_state.sql
git commit -m "api: add journey_leg_notification_state, the dedup table for station-skip push notifications"
```

---

## Task 3: `crates/api` — station-skip detection (`station_skip.rs`)

**Files:** create `crates/api/src/data/station_skip.rs`, modify
`crates/api/src/data/mod.rs` (or wherever `eta_blend` is declared as a
module — confirm via `grep -n "mod eta_blend" crates/api/src/data/mod.rs`
and add `pub mod station_skip;` alongside it).

Depends on Task 1 (`common::match_darwin_departure`/`departure_skips_station`).
The API-side half of §5.2's detection mechanism, structurally mirroring
`eta_blend.rs`'s own pure-function-plus-async-wrapper split
(`find_darwin_eta` / `blend_darwin_eta`).

- [ ] **Step 1: Write the module**

```rust
//! Station-skip detection for a journey leg's own origin/destination --
//! §5.2 of docs/superpowers/specs/2026-09-22-journey-tracking-design.md.
//! Structurally mirrors eta_blend.rs's find_darwin_eta/blend_darwin_eta
//! split (a pure matching function, plus an async "fetch samples, apply
//! it" wrapper), reusing common::match_darwin_departure/
//! departure_skips_station so this exact matching/skip semantics is shared
//! with crates/notifier's own, independently-written implementation of the
//! same check (crates/notifier/src/skip_check.rs) -- that crate cannot
//! depend on this one, see this module's own Cargo dependency note in the
//! plan that added it.
//!
//! "The leg's own origin/destination", not the matched train's full-route
//! origin/destination -- see the design spec's §1.1 for why journey_legs
//! keeps its own origin_crs/destination_crs even once matched to a real
//! train. A skip 50 miles from either end of this traveller's leg is not
//! this traveller's problem (§5.2's own framing).

use common::{StationDeparture, departure_skips_station, match_darwin_departure};
use sqlx::PgPool;

use crate::data::queries;

/// Whether either end of one journey leg's own travel intent is among
/// today's Darwin-reported skipped calling points. `false`/`false` (never
/// an error) whenever there's simply no live sample to check against --
/// same best-effort posture as `blend_darwin_eta`'s own overlay; this is a
/// nice-to-have enhancement layered on a read, never something a read
/// route should fail over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegSkipStatus {
    pub origin_skipped: bool,
    pub destination_skipped: bool,
}

impl LegSkipStatus {
    pub fn any(self) -> bool {
        self.origin_skipped || self.destination_skipped
    }
}

/// Pure core, no I/O: given the departure-board sample already fetched at
/// the leg's own `origin_crs` (`origin_board`), and -- only when the bound
/// train's own true point of origin differs from the leg's own origin (a
/// traveller boarding partway through a through-service) -- a second
/// sample fetched at that true origin (`train_origin_board`), decide
/// whether either end of this leg is among today's skipped calling points.
/// `match_target` is the same "which entry on this departure board is
/// actually the service we're tracking" key `find_darwin_eta` already
/// uses: the bound train's own pinned destination, or failing that its
/// current next calling point -- NOT `leg_destination_crs`, which may be
/// only an intermediate stop on the train's full route and so would never
/// match any departure board entry's own reported destination.
pub fn find_leg_skip(
    origin_board: &[StationDeparture],
    train_origin_board: Option<&[StationDeparture]>,
    match_target: Option<&str>,
    leg_origin_crs: &str,
    leg_destination_crs: &str,
) -> LegSkipStatus {
    let destination_skipped = match_darwin_departure(origin_board, match_target)
        .is_some_and(|matched| departure_skips_station(matched, leg_destination_crs));

    let origin_skipped = train_origin_board
        .and_then(|board| match_darwin_departure(board, match_target))
        .is_some_and(|matched| departure_skips_station(matched, leg_origin_crs));

    LegSkipStatus {
        origin_skipped,
        destination_skipped,
    }
}

/// Async wrapper: fetches the live sample(s) `find_leg_skip` needs and
/// applies it. `trains_id` is the leg's bound `train_subscriptions.trains_id`
/// -- used only to look up that train's own true `origin_crs`
/// (`trains.origin_crs`, nullable,
/// `crates/api/migrations/20260906100000_trains.sql:19`) for the symmetric
/// origin-skip check; never re-derives the leg's own origin/destination,
/// which the caller already has from its own `journey_legs` row. Any
/// failure to fetch a sample degrades to `LegSkipStatus::default()`
/// (both `false`) -- same best-effort posture as `blend_darwin_eta`.
pub async fn leg_skip_status(
    pool: &PgPool,
    trains_id: i64,
    leg_origin_crs: &str,
    leg_destination_crs: &str,
    match_target: Option<&str>,
) -> LegSkipStatus {
    let Ok(Some(origin_sample)) = queries::latest_station_sample(pool, leg_origin_crs).await else {
        return LegSkipStatus::default();
    };

    let train_true_origin_crs: Option<String> =
        sqlx::query_scalar("SELECT origin_crs FROM trains WHERE id = $1")
            .bind(trains_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .flatten();

    let train_origin_sample = match train_true_origin_crs.as_deref() {
        Some(crs) if !crs.eq_ignore_ascii_case(leg_origin_crs) => {
            queries::latest_station_sample(pool, crs).await.ok().flatten()
        }
        _ => None,
    };

    find_leg_skip(
        &origin_sample.departures,
        train_origin_sample.as_ref().map(|s| s.departures.as_slice()),
        match_target,
        leg_origin_crs,
        leg_destination_crs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn departure(destination_crs: &str, skipped: Vec<&str>) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "18:32".to_string(),
            estimated: "18:41".to_string(),
            is_cancelled: false,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: skipped.into_iter().map(str::to_string).collect(),
        }
    }

    #[test]
    fn no_skip_when_the_matched_entry_reports_nothing_skipped() {
        let origin_board = vec![departure("WAT", vec![])];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }

    #[test]
    fn destination_skip_is_detected_from_the_origin_board() {
        let origin_board = vec![departure("WAT", vec!["WOK"])];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert!(status.destination_skipped);
        assert!(!status.origin_skipped);
        assert!(status.any());
    }

    #[test]
    fn origin_skip_is_only_checked_when_a_train_origin_board_is_supplied() {
        let origin_board = vec![departure("WAT", vec![])];
        let train_origin_board = vec![departure("WAT", vec!["RDG"])];
        let status = find_leg_skip(
            &origin_board,
            Some(&train_origin_board),
            Some("WAT"),
            "RDG",
            "WOK",
        );
        assert!(status.origin_skipped);
        assert!(!status.destination_skipped);
    }

    #[test]
    fn no_match_target_means_nothing_can_ever_be_flagged() {
        let origin_board = vec![departure("WAT", vec!["WOK"])];
        let status = find_leg_skip(&origin_board, None, None, "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }

    #[test]
    fn a_cancelled_service_never_flags_a_skip_here_either() {
        // Whole-service cancellation is a separate, already-handled signal
        // (train_current_state.status) -- see match_darwin_departure's own
        // "never matches a cancelled departure" test in crates/common.
        let mut cancelled = departure("WAT", vec!["WOK"]);
        cancelled.is_cancelled = true;
        let origin_board = vec![cancelled];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }
}
```

- [ ] **Step 2: Declare the module** — add `pub mod station_skip;` next to
  the existing `eta_blend` module declaration (locate via `grep -n "mod
  eta_blend" crates/api/src/data/mod.rs` or equivalent — this repo's module
  tree may declare it differently; match whatever convention `eta_blend` is
  already declared with).

- [ ] **Step 3: Verify**

```bash
cargo test -p api --lib station_skip
cargo clippy -p api --all-features --all-targets -- -D warnings
```

  Expected: all 5 new unit tests pass; no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/station_skip.rs crates/api/src/data/mod.rs
git commit -m "api: add station_skip::find_leg_skip/leg_skip_status, the API-side half of §5.2's station-skip detection"
```

---

## Task 4: `crates/api` — wire skip status into `GET /Journeys/{id}`

**Files:** modify Phase 1's journey-detail data function (assumed
`crates/api/src/data/journeys.rs`) and/or route handler (assumed
`crates/api/src/routes/journeys.rs`).

**Depends on Phase 1 having landed** — see this plan's header. Adds one new
field to the per-leg response shape and one call into Task 3's
`station_skip::leg_skip_status` for each matched leg.

- [ ] **Step 1: Locate Phase 1's journey-detail read path.** Find the
  function backing `GET /Journeys/{id}` (search `grep -rn "GET /Journeys"
  crates/api/src/routes/` and `grep -rn "struct.*Journey.*Leg.*Response\|JourneyDetail"
  crates/api/src/data/` to locate the actual names Phase 1 used — they are
  not cited here with file:line because Phase 1's code doesn't exist yet at
  the time this plan was written).

- [ ] **Step 2: Add a `legSkip` field to the per-leg response DTO.** For
  each leg in the response whose `train_subscription_id` and
  `origin_crs`/`destination_crs` are all non-`NULL` (per this plan's Global
  Constraints note on nullability), call:

```rust
let match_target = tracked_train_state
    .pin_destination_crs
    .as_deref()
    .or(tracked_train_state.next_calling_point.as_deref());
let skip_status = match tracked_train_state.trains_id {
    Some(trains_id) => {
        crate::data::station_skip::leg_skip_status(
            &app.database,
            trains_id,
            &leg.origin_crs, // already known non-null at this point
            &leg.destination_crs,
            match_target,
        )
        .await
    }
    None => crate::data::station_skip::LegSkipStatus::default(),
};
```

  and serialize it onto the wire response as (camelCase, matching this
  API's existing convention throughout `crates/api/src/render.rs` and every
  other response DTO):

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegSkipResponse {
    origin_skipped: bool,
    destination_skipped: bool,
}
```

  A leg with no matched train, or with a `NULL` `origin_crs`/`destination_crs`,
  gets `legSkip: null` on the wire (no skip concept applies) — do not
  synthesize a `false`/`false` value for those cases, since that would read
  as "checked, not skipped" rather than "not applicable," a real,
  user-visible difference this plan should not blur.

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
```

  Expected: builds and lints clean against whatever Phase 1 actually
  shipped, once this task's insertion points are adjusted to match Phase
  1's real names.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/journeys.rs crates/api/src/routes/journeys.rs
git commit -m "api: surface per-leg station-skip status on GET /Journeys/{id}"
```

---

## Task 5: Frontend — skip badge on the journey view

**Files:** modify `frontend/lib/types.ts`, `frontend/components/JourneyTimeline.tsx`,
`frontend/components/TrainJourney.tsx`, and Phase 1's
`frontend/app/journeys/[id]/page.tsx`.

**Depends on Task 4's `legSkip` field existing on the wire.** Threads an
optional `skippedCrs` prop through `TrainJourney` → `JourneyTimeline` →
`JourneyStopRow`, following `endpointNames?: JourneyEndpointNames`'s own
existing optional-prop precedent exactly (default `undefined`, every
non-journey caller unaffected) — see Judgment Call 7 for why this is a new
prop, not a new field baked into `TrackedTrainState`.

- [ ] **Step 1: Add the wire type**, near `JourneyStop`
  (`frontend/lib/types.ts:468-487`):

```typescript
/** `GET /Journeys/{id}`'s per-leg `legSkip` field
 * (`crates/api/src/data/station_skip.rs`'s `LegSkipStatus`, camelCase on
 * the wire) -- `null` when the leg has no matched train yet, or no known
 * origin/destination to check (nothing to report, not "checked and
 * clean"). See docs/superpowers/specs/2026-09-22-journey-tracking-design.md §5.2. */
export interface LegSkipStatus {
  originSkipped: boolean;
  destinationSkipped: boolean;
}
```

- [ ] **Step 2: Thread `skippedCrs` through `JourneyTimeline`**
  (`frontend/components/JourneyTimeline.tsx:55-106`):

```typescript
export function JourneyTimeline({
  stops,
  endpointNames,
  skippedCrs,
}: {
  stops: JourneyStop[];
  endpointNames?: JourneyEndpointNames;
  /** CRS codes of stops on THIS leg that a live Darwin sample reports as
   * no longer being called at today (§5.2) -- optional, `undefined` for
   * every caller outside the journey view (single-train tracking has no
   * leg-scoped skip concept, per Judgment Call 7). At most two entries in
   * practice (a leg's own origin and/or destination), but this accepts a
   * plain list rather than two named booleans so `JourneyStopRow` doesn't
   * need to know which end it's rendering. */
  skippedCrs?: string[];
}) {
  // ... (body unchanged except the two call sites below)
          {stops.map((stop, index) => (
            <JourneyStopRow
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              total={total}
              endpointNames={endpointNames}
              skippedCrs={skippedCrs}
            />
          ))}
  // ...
}
```

  and in `JourneyStopRow` (`frontend/components/JourneyTimeline.tsx:172-223`),
  add the prop and render a badge in the Station cell — same
  `color="red" variant="light"` convention as
  `TrackedTrainStatusBadge.tsx:50-52`/`TrainJourney.tsx:171` (Judgment
  Call 7):

```typescript
function JourneyStopRow({
  stop,
  index,
  total,
  endpointNames,
  skippedCrs,
}: {
  stop: JourneyStop;
  index: number;
  total: number;
  endpointNames?: JourneyEndpointNames;
  skippedCrs?: string[];
}) {
  const label = journeyStopLabel(stop, index, total, endpointNames);
  const isSkipped =
    stop.crs !== null && (skippedCrs ?? []).some((crs) => crs.toUpperCase() === stop.crs?.toUpperCase());
  // ... existing scheduled/actual/estimated/reached logic unchanged ...

  return (
    <TableTr>
      <TableTd>
        <Group gap={6} wrap="nowrap">
          <Text
            fw={stop.kind === 'Origin' || stop.kind === 'Terminate' ? 700 : 400}
            c={reached ? undefined : 'dimmed'}
          >
            {label}
          </Text>
          {isSkipped && (
            <Badge color="red" variant="light" size="sm">
              Skipped
            </Badge>
          )}
        </Group>
      </TableTd>
      {/* ... remaining cells unchanged ... */}
    </TableTr>
  );
}
```

  `Group` is already importable from `@mantine/core` (this file already
  imports `Badge`/`Table`/`Text` from there — add `Group` to that existing
  import statement).

- [ ] **Step 3: Thread `skippedCrs` through `TrainJourney`**
  (`frontend/components/TrainJourney.tsx:39-74`):

```typescript
export function TrainJourney({
  state,
  suppressTrainUidHeading = false,
  skippedCrs,
}: {
  state: TrainJourneyState;
  suppressTrainUidHeading?: boolean;
  skippedCrs?: string[];
}) {
  // ... endpointNames computation unchanged ...
  return (
    <Stack gap="sm">
      <StatusMessage state={state} suppressTrainUidHeading={suppressTrainUidHeading} />
      {state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />}
      {state.journeyStops && (
        <JourneyProgress
          stops={state.journeyStops}
          resolutionStatus={state.resolutionStatus}
          status={state.status}
          trainUid={state.trainUid}
          mayHaveArrived={state.mayHaveArrived}
          lastReportedLocation={state.lastReportedLocation}
          endpointNames={endpointNames}
        />
      )}
      {state.journeyStops && (
        <JourneyTimeline stops={state.journeyStops} endpointNames={endpointNames} skippedCrs={skippedCrs} />
      )}
    </Stack>
  );
}
```

- [ ] **Step 4: Compute `skippedCrs` per leg in the journey page.** In
  Phase 1's `frontend/app/journeys/[id]/page.tsx`, wherever it renders
  `<TrainJourney state={leg.trackedTrainState} />` for a matched leg, pass:

```typescript
const skippedCrs = [
  leg.legSkip?.originSkipped ? leg.originCrs : null,
  leg.legSkip?.destinationSkipped ? leg.destinationCrs : null,
].filter((crs): crs is string => crs !== null);

<TrainJourney state={leg.trackedTrainState} skippedCrs={skippedCrs} />
```

  (adjust field names to whatever Phase 1's actual per-leg response type
  calls them — `legSkip`/`originCrs`/`destinationCrs` are this plan's own
  Task 4 naming, assumed carried through unchanged).

- [ ] **Step 5: Add/update component tests.** `frontend/components/JourneyTimeline.tsx`
  has no existing `.test.tsx` file (confirmed: no `JourneyTimeline.test.tsx`
  in `frontend/components/`) — this plan does not introduce one (out of
  scope: adding a first test suite to a previously-untested component is a
  larger undertaking than this one prop deserves). Instead, verify manually
  in Task 10. If `TrainJourney.test.tsx` or similar already exists and
  snapshot-tests its rendered output, confirm it still passes with
  `skippedCrs` omitted (the new prop is optional, so no existing test
  should need updating) via:

```bash
npm test -- TrainJourney
```

- [ ] **Step 6: Verify**

```bash
npm run build
```

  Expected: clean build — the new prop is additive/optional everywhere, so
  no existing call site should fail to typecheck.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/types.ts frontend/components/JourneyTimeline.tsx frontend/components/TrainJourney.tsx frontend/app/journeys/\[id\]/page.tsx
git commit -m "frontend: render a station-skip badge on the journey view's per-leg timeline"
```

---

## Task 6: `crates/notifier` — journey/leg-aware notification copy (§5.1)

**Files:** modify `crates/notifier/src/queries.rs`, `crates/notifier/src/main.rs`.

Independent of Tasks 1-5 and 7-9 (this is the "fully reused, no new
decision logic" half of the spec — see Architecture). Implements Judgment
Calls 5 and 6.

- [ ] **Step 1: Add `JourneyLegContext` and its query**, in
  `crates/notifier/src/queries.rs`, alongside `upsert_train_notification_state`:

```rust
#[derive(Debug, sqlx::FromRow)]
pub struct JourneyLegContext {
    pub journey_id: i64,
    pub journey_name: Option<String>,
    pub leg_order: i32,
    pub total_legs: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
}

/// Finds the journey/leg context for a `train_subscriptions.id`, if one
/// exists -- absent for every legacy tracked train until Phase 1's own
/// migration wraps it in a one-row journey (§7.1 of the design spec), and
/// for any train tracked outside the journeys flow, if that path stays
/// open at all. `notify_train_candidates` falls back to today's exact
/// copy/URL when this returns `None`, OR when it returns `Some` with
/// `total_legs == 1` (Judgment Call 5 -- a one-leg journey's notification
/// copy is indistinguishable in value from today's plain tracked-train
/// copy, so this plan doesn't change it).
///
/// `ORDER BY jl.id LIMIT 1`: a known, accepted edge case -- if the SAME
/// physical train (`trains_id`) is tracked via two different legs (legal:
/// `create_subscription_for_train` is idempotent by `(user_id, trains_id)`,
/// so a second leg pointing at the same trains_id reuses the same
/// `train_subscriptions` row -- design spec §0.1), this query returns only
/// the first-created leg's context, so the payload describes only one of
/// the two legs even though both legs' owners (if different users) are
/// notified via the existing per-trains_id fan-out. Rare, and no worse
/// than the ambiguity already inherent in "one physical train, several
/// subscribers" today.
pub async fn journey_leg_for_train_subscription(
    pool: &PgPool,
    tracked_train_id: i64,
) -> anyhow::Result<Option<JourneyLegContext>> {
    let row = sqlx::query_as::<_, JourneyLegContext>(
        "SELECT j.id AS journey_id, j.custom_name AS journey_name, jl.leg_order, \
                (SELECT COUNT(*) FROM journey_legs jl2 WHERE jl2.journey_id = jl.journey_id) AS total_legs, \
                jl.origin_crs, jl.destination_crs \
         FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.train_subscription_id = $1 \
         ORDER BY jl.id LIMIT 1",
    )
    .bind(tracked_train_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 2: Build journey-aware copy in `notify_train_candidates`**
  (`crates/notifier/src/main.rs:169-221`), replacing the existing
  `NotificationPayload` construction:

```rust
async fn notify_train_candidates(
    pool: &PgPool,
    candidates: &[queries::TrainCandidate],
    vapid_private_key: &str,
    vapid_subject: &str,
    now: chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    for candidate in candidates {
        tracing::info!(
            tracked_train_id = candidate.tracked_train_id,
            trains_id = candidate.trains_id,
            previous_rank = candidate.previous_rank,
            new_rank = candidate.new_rank,
            "train notification candidate"
        );
        let (status, delay_minutes) = current_train_state(pool, candidate.trains_id).await?;
        let journey_context =
            queries::journey_leg_for_train_subscription(pool, candidate.tracked_train_id).await?;
        let payload = build_train_notification_payload(
            candidate.tracked_train_id,
            &status,
            delay_minutes,
            journey_context.as_ref(),
        );
        if send_to_all_subscriptions(
            pool,
            &candidate.user_id,
            &payload,
            vapid_private_key,
            vapid_subject,
        )
        .await?
        {
            queries::upsert_train_notification_state(
                pool,
                &candidate.user_id,
                candidate.tracked_train_id,
                &status,
                delay_minutes,
                now,
            )
            .await?;
        }
    }
    Ok(())
}

/// Builds the delay/cancellation `NotificationPayload` -- journey/leg-aware
/// when `journey_context` names a genuine multi-leg journey (`total_legs >
/// 1`), otherwise byte-for-byte today's plain copy (Judgment Call 5). CRS
/// codes, not resolved station names, in the multi-leg body (Judgment
/// Call 6). Extracted out of `notify_train_candidates` as its own pure
/// function so this copy-building logic (the ONLY thing §5.1 changes,
/// design spec's own framing) is independently testable without a database.
fn build_train_notification_payload(
    tracked_train_id: i64,
    status: &str,
    delay_minutes: Option<i32>,
    journey_context: Option<&queries::JourneyLegContext>,
) -> NotificationPayload {
    let is_cancelled = status == "cancelled";

    let multi_leg = journey_context.filter(|ctx| ctx.total_legs > 1);
    let Some(ctx) = multi_leg else {
        // Fallback: no journey_legs row, or a trivial one-leg journey --
        // today's exact copy/URL, unchanged.
        return NotificationPayload {
            title: if is_cancelled {
                "Your train was cancelled".to_string()
            } else {
                "Your train is delayed".to_string()
            },
            body: match delay_minutes {
                Some(minutes) if !is_cancelled => format!("Now running about {minutes} minutes late."),
                _ => "Check the latest status.".to_string(),
            },
            url: format!("/track/{tracked_train_id}"),
            tag: format!("train-{tracked_train_id}"),
        };
    };

    let journey_label = match &ctx.journey_name {
        Some(name) => format!("'{name}'"),
        None => "your journey".to_string(),
    };
    let route = match (&ctx.origin_crs, &ctx.destination_crs) {
        (Some(origin), Some(destination)) => Some(format!("{origin} to {destination}")),
        _ => None,
    };

    let title = if is_cancelled {
        format!("Leg {} of {journey_label} was cancelled", ctx.leg_order)
    } else {
        format!("Leg {} of {journey_label} is delayed", ctx.leg_order)
    };
    let body = match (is_cancelled, delay_minutes, &route) {
        (true, _, Some(route)) => format!("The {route} service was cancelled."),
        (true, _, None) => "This service was cancelled.".to_string(),
        (false, Some(minutes), Some(route)) => {
            format!("{route}, now running about {minutes} minutes late.")
        }
        (false, Some(minutes), None) => format!("Now running about {minutes} minutes late."),
        (false, None, _) => "Check the latest status.".to_string(),
    };

    NotificationPayload {
        title,
        body,
        url: format!("/journeys/{}", ctx.journey_id),
        tag: format!("train-{tracked_train_id}"), // unchanged -- still the same underlying tracked-train row
    }
}
```

- [ ] **Step 2b: Add unit tests for `build_train_notification_payload`**,
  directly below `notify_train_candidates` in `main.rs`, next to the
  existing `#[cfg(test)] mod db_tests` block (a NEW, separate, non-`#[ignore]`d
  `#[cfg(test)] mod copy_tests` — no database needed, this function is
  pure):

```rust
#[cfg(test)]
mod copy_tests {
    use super::*;

    fn ctx(total_legs: i64, journey_name: Option<&str>) -> queries::JourneyLegContext {
        queries::JourneyLegContext {
            journey_id: 42,
            journey_name: journey_name.map(str::to_string),
            leg_order: 2,
            total_legs,
            origin_crs: Some("WAV".to_string()),
            destination_crs: Some("KGX".to_string()),
        }
    }

    #[test]
    fn no_journey_context_falls_back_to_todays_exact_copy() {
        let payload = build_train_notification_payload(7, "en_route", Some(18), None);
        assert_eq!(payload.title, "Your train is delayed");
        assert_eq!(payload.body, "Now running about 18 minutes late.");
        assert_eq!(payload.url, "/track/7");
        assert_eq!(payload.tag, "train-7");
    }

    #[test]
    fn a_one_leg_journey_also_falls_back_to_todays_exact_copy() {
        let context = ctx(1, Some("Weekend in Edinburgh"));
        let payload = build_train_notification_payload(7, "cancelled", None, Some(&context));
        assert_eq!(payload.title, "Your train was cancelled");
        assert_eq!(payload.url, "/track/7");
    }

    #[test]
    fn a_multi_leg_journey_names_the_leg_and_journey() {
        let context = ctx(3, Some("Weekend in Edinburgh"));
        let payload = build_train_notification_payload(7, "en_route", Some(18), Some(&context));
        assert_eq!(payload.title, "Leg 2 of 'Weekend in Edinburgh' is delayed");
        assert_eq!(payload.body, "WAV to KGX, now running about 18 minutes late.");
        assert_eq!(payload.url, "/journeys/42");
        assert_eq!(payload.tag, "train-7");
    }

    #[test]
    fn an_unnamed_multi_leg_journey_falls_back_to_a_generic_journey_label() {
        let context = ctx(2, None);
        let payload = build_train_notification_payload(7, "en_route", Some(5), Some(&context));
        assert_eq!(payload.title, "Leg 2 of your journey is delayed");
    }

    #[test]
    fn a_multi_leg_cancellation_names_the_route_when_known() {
        let context = ctx(2, Some("Weekend in Edinburgh"));
        let payload = build_train_notification_payload(7, "cancelled", None, Some(&context));
        assert_eq!(payload.title, "Leg 2 of 'Weekend in Edinburgh' was cancelled");
        assert_eq!(payload.body, "The WAV to KGX service was cancelled.");
    }

    #[test]
    fn a_multi_leg_journey_with_no_leg_origin_destination_omits_the_route() {
        let mut context = ctx(2, Some("Weekend in Edinburgh"));
        context.origin_crs = None;
        context.destination_crs = None;
        let payload = build_train_notification_payload(7, "en_route", Some(5), Some(&context));
        assert_eq!(payload.body, "Now running about 5 minutes late.");
    }
}
```

- [ ] **Step 3: Verify**

```bash
cargo test -p notifier copy_tests
cargo build -p notifier
cargo clippy -p notifier --all-features --all-targets -- -D warnings
```

  Expected: all 6 new tests pass; clean build/lint.

- [ ] **Step 4: Commit**

```bash
git add crates/notifier/src/queries.rs crates/notifier/src/main.rs
git commit -m "notifier: journey/leg-aware NotificationPayload copy for delay/cancellation (§5.1)"
```

---

## Task 7: `crates/notifier` — `decide_skip_notification` (pure decision fn)

**Files:** modify `crates/notifier/src/decision.rs`.

Independent, pure, no I/O — mirrors `decide_train_notification`'s
escalation-only shape exactly (§5.2 point 3's own instruction).

- [ ] **Step 1: Add the function**, directly below `decide_train_notification`
  (`crates/notifier/src/decision.rs:83-89`):

```rust
/// Escalation-only, same shape as [`decide_train_notification`]: fires the
/// instant a leg transitions from not-skipped to skipped, never on the
/// reverse (a skip that later resolves itself does not warrant a second
/// push -- matching every other de-escalation-is-silent decision in this
/// module). No cold-start guard, same as [`decide_train_notification`]'s
/// own documented posture: a leg that's ALREADY skipped the very first
/// time this notifier ever checks it (no prior
/// `journey_leg_notification_state` row, so `was_skipped` is `false` by
/// convention -- see `crates/notifier/src/queries.rs`'s
/// `skip_notification_state`) still notifies once immediately, the same
/// way "a newly tracked already-delayed train does notify once."
pub fn decide_skip_notification(was_skipped: bool, is_skipped: bool) -> NotifyDecision {
    if is_skipped && !was_skipped {
        NotifyDecision::NotifyNow
    } else {
        NotifyDecision::Skip
    }
}

#[cfg(test)]
mod skip_notification_tests {
    use super::*;

    #[test]
    fn not_skipped_to_skipped_notifies() {
        assert_eq!(decide_skip_notification(false, true), NotifyDecision::NotifyNow);
    }

    #[test]
    fn skipped_to_not_skipped_does_not_notify() {
        assert_eq!(decide_skip_notification(true, false), NotifyDecision::Skip);
    }

    #[test]
    fn staying_skipped_does_not_re_notify() {
        assert_eq!(decide_skip_notification(true, true), NotifyDecision::Skip);
    }

    #[test]
    fn staying_not_skipped_does_not_notify() {
        assert_eq!(decide_skip_notification(false, false), NotifyDecision::Skip);
    }

    #[test]
    fn a_leg_already_skipped_on_first_ever_check_notifies_once() {
        // Status note mirroring decide_train_notification's own equivalent
        // test: no cold-start guard -- was_skipped=false (no prior state
        // row) is the correct baseline, not a skip.
        assert_eq!(decide_skip_notification(false, true), NotifyDecision::NotifyNow);
    }
}
```

- [ ] **Step 2: Verify**

```bash
cargo test -p notifier skip_notification_tests
```

  Expected: all 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/notifier/src/decision.rs
git commit -m "notifier: add decide_skip_notification, escalation-only station-skip decision logic"
```

---

## Task 8: `crates/notifier` — queries for committed legs + skip state

**Files:** modify `crates/notifier/src/queries.rs`.

Depends on Task 2's migration (`journey_leg_notification_state`) and Phase
1's `journeys`/`journey_legs` tables. Adds the notifier-local station-sample
fetch (necessarily duplicated from `crates/api`'s own
`queries::latest_station_sample` — see Architecture) and the
listing/state-read/state-write functions Task 9's cycle needs.

- [ ] **Step 1: Add `station_sample_for_crs`** (the notifier-local
  duplicate of `crates/api/src/data/queries.rs:1628-1644`'s
  `latest_station_sample` — same table, same shape, independently written
  because this crate cannot import that one):

```rust
/// This crate's own copy of `crates/api/src/data/queries.rs`'s
/// `latest_station_sample` -- necessarily duplicated, not imported, since
/// `crates/notifier` does not (and per this plan's Global Constraints,
/// must not) depend on `crates/api`. Same table (`station_samples`,
/// wholesale-replaced per poll, one row per station, no history), same
/// "None means no sample for this CRS yet" contract.
pub async fn station_sample_for_crs(
    pool: &PgPool,
    crs: &str,
) -> anyhow::Result<Option<common::StationSample>> {
    let row = sqlx::query("SELECT crs, polled_at, departures FROM station_samples WHERE crs = $1")
        .bind(crs)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let departures_json: serde_json::Value = row.try_get("departures")?;
    Ok(Some(common::StationSample {
        crs: row.try_get("crs")?,
        polled_at: row.try_get("polled_at")?,
        departures: serde_json::from_value(departures_json)?,
    }))
}
```

- [ ] **Step 2: Add `CommittedLeg` and `list_committed_legs_for_today`**:

```rust
pub struct CommittedLeg {
    pub journey_leg_id: i64,
    pub journey_id: i64,
    pub user_id: String,
    pub origin_crs: String,
    pub destination_crs: String,
    pub trains_id: i64,
    pub pin_destination_crs: Option<String>,
    pub next_calling_point: Option<String>,
    pub train_origin_crs: Option<String>,
}

/// Every leg worth station-skip-checking today: bound to a real train
/// (`train_subscription_id IS NOT NULL`), resolved to a shared `trains`
/// row (`ts.trains_id IS NOT NULL` -- an unresolved pin has no departure
/// board to check against), with a known own origin/destination (§7.1's
/// nullability correction), on `today` (Judgment Call 3 -- bounds this
/// full poll to journeys actually happening today, since `station_samples`
/// is a current-snapshot table with no watermark to diff against). One row
/// per leg, already carrying everything `skip_check::leg_is_skipped`
/// (Task 9) needs -- no further per-leg query required.
pub async fn list_committed_legs_for_today(
    pool: &PgPool,
    today: chrono::NaiveDate,
) -> anyhow::Result<Vec<CommittedLeg>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT jl.id AS journey_leg_id, jl.journey_id, j.user_id, \
                jl.origin_crs, jl.destination_crs, ts.trains_id, \
                ts.pin_destination_crs, cs.next_calling_point, tr.origin_crs AS train_origin_crs \
         FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         WHERE jl.train_subscription_id IS NOT NULL \
           AND ts.trains_id IS NOT NULL \
           AND jl.service_date = $1 \
           AND jl.origin_crs IS NOT NULL \
           AND jl.destination_crs IS NOT NULL",
    )
    .bind(today)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CommittedLeg {
                journey_leg_id: row.try_get("journey_leg_id")?,
                journey_id: row.try_get("journey_id")?,
                user_id: row.try_get("user_id")?,
                origin_crs: row.try_get("origin_crs")?,
                destination_crs: row.try_get("destination_crs")?,
                trains_id: row.try_get("trains_id")?,
                pin_destination_crs: row.try_get("pin_destination_crs")?,
                next_calling_point: row.try_get("next_calling_point")?,
                train_origin_crs: row.try_get("train_origin_crs")?,
            })
        })
        .collect()
}
```

- [ ] **Step 3: Add skip-notification state read/write**, mirroring
  `line_notification_state`/`upsert_line_notification_state`
  (`queries.rs:295-331`) one level finer:

```rust
pub async fn skip_notification_state(
    pool: &PgPool,
    user_id: &str,
    journey_leg_id: i64,
) -> anyhow::Result<Option<bool>> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT last_notified_skipped FROM journey_leg_notification_state \
         WHERE user_id = $1 AND journey_leg_id = $2",
    )
    .bind(user_id)
    .bind(journey_leg_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(skipped,)| skipped))
}

pub async fn upsert_skip_notification_state(
    pool: &PgPool,
    user_id: &str,
    journey_leg_id: i64,
    skipped: bool,
    at: chrono::DateTime<Utc>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journey_leg_notification_state \
           (user_id, journey_leg_id, last_notified_skipped, last_notified_at) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, journey_leg_id) DO UPDATE SET \
           last_notified_skipped = EXCLUDED.last_notified_skipped, \
           last_notified_at = EXCLUDED.last_notified_at",
    )
    .bind(user_id)
    .bind(journey_leg_id)
    .bind(skipped)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 4: Add DB-gated `#[ignore]`d tests**, in this file's existing
  `#[cfg(test)] mod tests` block, alongside `push_subscriptions_round_trip_and_self_cleanup_on_delete`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database with Phase 1's journeys/journey_legs tables \
                already migrated; run with `DATABASE_URL=... cargo test -p notifier \
                list_committed_legs_for_today_finds_a_bound_leg_with_a_live_sample \
                -- --ignored --test-threads=1`"]
    async fn list_committed_legs_for_today_finds_a_bound_leg_with_a_live_sample() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-22".parse().unwrap();
        seed_user(&pool, "TEST-SKIP-LEG-USER").await;

        let trains_id: i64 = sqlx::query_scalar(
            "INSERT INTO trains (train_uid, service_date, origin_crs) \
             VALUES ('TEST-SKIP-LEG-UID', $1, 'PAD') RETURNING id",
        )
        .bind(service_date)
        .fetch_one(&pool)
        .await
        .expect("seed trains row");

        let tracked_train_id: i64 = sqlx::query_scalar(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, resolution_status) \
             VALUES ($1, $2, 'RDG', $3, $4, 'resolved') RETURNING id",
        )
        .bind("TEST-SKIP-LEG-USER")
        .bind(service_date)
        .bind(service_date.and_hms_opt(9, 0, 0).unwrap().and_utc())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed train_subscriptions row");

        let journey_id: i64 = sqlx::query_scalar(
            "INSERT INTO journeys (user_id, custom_name) VALUES ($1, NULL) RETURNING id",
        )
        .bind("TEST-SKIP-LEG-USER")
        .fetch_one(&pool)
        .await
        .expect("seed journeys row");

        let journey_leg_id: i64 = sqlx::query_scalar(
            "INSERT INTO journey_legs \
                (journey_id, leg_order, origin_crs, destination_crs, service_date, train_subscription_id, match_mode) \
             VALUES ($1, 1, 'RDG', 'WOK', $2, $3, 'manual') RETURNING id",
        )
        .bind(journey_id)
        .bind(service_date)
        .bind(tracked_train_id)
        .fetch_one(&pool)
        .await
        .expect("seed journey_legs row");

        let legs = list_committed_legs_for_today(&pool, service_date)
            .await
            .expect("list_committed_legs_for_today");
        let found = legs
            .iter()
            .find(|leg| leg.journey_leg_id == journey_leg_id)
            .expect("the seeded leg must be returned");
        assert_eq!(found.origin_crs, "RDG");
        assert_eq!(found.destination_crs, "WOK");
        assert_eq!(found.trains_id, trains_id);
        assert_eq!(found.train_origin_crs.as_deref(), Some("PAD"));

        sqlx::query("DELETE FROM journey_legs WHERE id = $1").bind(journey_leg_id).execute(&pool).await.ok();
        sqlx::query("DELETE FROM journeys WHERE id = $1").bind(journey_id).execute(&pool).await.ok();
        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1").bind(tracked_train_id).execute(&pool).await.ok();
        sqlx::query("DELETE FROM trains WHERE id = $1").bind(trains_id).execute(&pool).await.ok();
        cleanup_user_skip(&pool, "TEST-SKIP-LEG-USER").await;
    }

    async fn cleanup_user_skip(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM journey_leg_notification_state WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database with Phase 1's journeys/journey_legs tables \
                already migrated; run with `DATABASE_URL=... cargo test -p notifier \
                skip_notification_state_round_trips -- --ignored --test-threads=1`"]
    async fn skip_notification_state_round_trips() {
        let pool = connect().await;
        seed_user(&pool, "TEST-SKIP-STATE-USER").await;
        let journey_id: i64 = sqlx::query_scalar(
            "INSERT INTO journeys (user_id, custom_name) VALUES ($1, NULL) RETURNING id",
        )
        .bind("TEST-SKIP-STATE-USER")
        .fetch_one(&pool)
        .await
        .expect("seed journeys row");
        let journey_leg_id: i64 = sqlx::query_scalar(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             VALUES ($1, 1, 'RDG', 'WOK', '2026-09-22', 'unmatched') RETURNING id",
        )
        .bind(journey_id)
        .fetch_one(&pool)
        .await
        .expect("seed journey_legs row");

        assert_eq!(
            skip_notification_state(&pool, "TEST-SKIP-STATE-USER", journey_leg_id)
                .await
                .expect("read before any write"),
            None
        );

        let now = Utc::now();
        upsert_skip_notification_state(&pool, "TEST-SKIP-STATE-USER", journey_leg_id, true, now)
            .await
            .expect("first upsert");
        assert_eq!(
            skip_notification_state(&pool, "TEST-SKIP-STATE-USER", journey_leg_id)
                .await
                .expect("read after first upsert"),
            Some(true)
        );

        upsert_skip_notification_state(&pool, "TEST-SKIP-STATE-USER", journey_leg_id, false, now)
            .await
            .expect("second upsert (resolved)");
        assert_eq!(
            skip_notification_state(&pool, "TEST-SKIP-STATE-USER", journey_leg_id)
                .await
                .expect("read after second upsert"),
            Some(false)
        );

        sqlx::query("DELETE FROM journey_legs WHERE id = $1").bind(journey_leg_id).execute(&pool).await.ok();
        sqlx::query("DELETE FROM journeys WHERE id = $1").bind(journey_id).execute(&pool).await.ok();
        cleanup_user_skip(&pool, "TEST-SKIP-STATE-USER").await;
    }
```

- [ ] **Step 5: Verify**

```bash
cargo build -p notifier
cargo clippy -p notifier --all-features --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p notifier \
  list_committed_legs_for_today skip_notification_state -- --ignored --test-threads=1
```

  Expected: clean build/lint; both DB-gated tests pass (once Phase 1's
  tables and Task 2's migration are present).

- [ ] **Step 6: Commit**

```bash
git add crates/notifier/src/queries.rs
git commit -m "notifier: add station_sample_for_crs, list_committed_legs_for_today, skip-notification state read/write"
```

---

## Task 9: `crates/notifier` — skip-check orchestration + third interval branch

**Files:** create `crates/notifier/src/skip_check.rs`, modify
`crates/notifier/src/main.rs`, `crates/notifier/src/config.rs`.

Depends on Tasks 1, 7, 8. Assembles everything into the new `main.rs`
interval branch §5.2 point 4 asks for.

- [ ] **Step 1: Write `skip_check.rs`**:

```rust
//! Station-skip detection for a journey leg's own origin/destination,
//! notifier-side -- §5.2 of docs/superpowers/specs/2026-09-22-journey-tracking-design.md.
//! Cannot import `crates/api/src/data/station_skip.rs` (this crate does
//! not depend on `crates/api` -- see this plan's Architecture section for
//! why), so this async wrapper is written fresh against this crate's own
//! `queries.rs`; the actual matching/skip-membership logic is still shared
//! via `common::match_darwin_departure`/`departure_skips_station` so the
//! two independent implementations (this one, and
//! `crates/api/src/data/station_skip.rs::find_leg_skip`) can never define
//! "skipped" two different ways.

use common::{departure_skips_station, match_darwin_departure};
use sqlx::PgPool;

use crate::queries::{self, CommittedLeg};

/// Whether either end of `leg`'s own travel intent is among today's
/// skipped calling points, per the live Darwin sample(s) currently on
/// file. `Ok(false)` (never an error) when there's simply no sample yet to
/// check against -- a genuine "don't know" degrades to "assume not
/// skipped," matching `station_skip::leg_skip_status`'s own best-effort
/// posture on the API side. A real DB connectivity failure still
/// propagates via `?`, same as every other query in this crate -- the
/// caller (`run_skip_check_cycle`) lets that fail the whole cycle, retried
/// next interval, same as `run_cycle`/`run_forward_queue_cycle` already do
/// for their own DB errors.
pub async fn leg_is_skipped(pool: &PgPool, leg: &CommittedLeg) -> anyhow::Result<bool> {
    let match_target = leg
        .pin_destination_crs
        .as_deref()
        .or(leg.next_calling_point.as_deref());

    let Some(origin_sample) = queries::station_sample_for_crs(pool, &leg.origin_crs).await? else {
        return Ok(false);
    };
    let destination_skipped = match_darwin_departure(&origin_sample.departures, match_target)
        .is_some_and(|matched| departure_skips_station(matched, &leg.destination_crs));

    let origin_skipped = match leg.train_origin_crs.as_deref() {
        Some(train_origin) if !train_origin.eq_ignore_ascii_case(&leg.origin_crs) => {
            match queries::station_sample_for_crs(pool, train_origin).await? {
                Some(sample) => match_darwin_departure(&sample.departures, match_target)
                    .is_some_and(|matched| departure_skips_station(matched, &leg.origin_crs)),
                None => false,
            }
        }
        _ => false,
    };

    Ok(destination_skipped || origin_skipped)
}
```

- [ ] **Step 2: Add the config field** (`crates/notifier/src/config.rs`,
  alongside `forward_queue_poll_interval_secs`):

```rust
    /// Cadence for the station-skip check (Task 9, §5.2) -- an independent
    /// full poll every interval, NOT cursor/watermark-based like the other
    /// two cycles, because `station_samples` is a wholesale-replaced
    /// current snapshot with no append log to diff against (see this
    /// plan's Architecture section). A reasonable-sounding, not
    /// load-tested figure -- same "revisit with real usage" posture this
    /// crate's other interval constants are already flagged with.
    #[arg(long, env, default_value_t = 90)]
    pub skip_check_poll_interval_secs: u64,
```

- [ ] **Step 3: Add `mod skip_check;` to `main.rs`'s module list**
  (`crates/notifier/src/main.rs:6-9`):

```rust
mod config;
mod decision;
mod queries;
mod send;
mod skip_check;
```

- [ ] **Step 4: Add the third interval and its cycle function**
  (`crates/notifier/src/main.rs`). In `main()`, add a third
  `tokio::time::interval` and `select!` arm alongside `interval`/`forward_interval`
  (`main.rs:50-81`):

```rust
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut forward_interval =
        tokio::time::interval(Duration::from_secs(config.forward_queue_poll_interval_secs));
    let mut skip_check_interval =
        tokio::time::interval(Duration::from_secs(config.skip_check_poll_interval_secs));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // ... unchanged ...
            }
            _ = forward_interval.tick() => {
                // ... unchanged ...
            }
            _ = skip_check_interval.tick() => {
                let result = run_skip_check_cycle(
                    &pool,
                    &config.vapid_private_key,
                    &config.vapid_subject,
                )
                .await;
                if let Err(err) = result {
                    tracing::error!(error = ?err, "notifier skip-check cycle failed; will retry next interval");
                }
            }
        }
    }
```

  and add the cycle function itself, near `run_forward_queue_cycle`
  (`main.rs:223-245`):

```rust
/// The station-skip check's own cycle (Task 9, §5.2) -- a full poll of
/// today's committed journey legs every `skip_check_poll_interval_secs`,
/// not cursor/watermark-based (see `config.rs`'s own doc comment on why).
/// Each leg is judged independently against its own
/// `journey_leg_notification_state` row -- `decide_skip_notification`'s
/// escalation-only shape, same discipline as every other notification path
/// in this crate: state is written only after a successful send.
async fn run_skip_check_cycle(
    pool: &PgPool,
    vapid_private_key: &str,
    vapid_subject: &str,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let today = now.date_naive();
    let legs = queries::list_committed_legs_for_today(pool, today).await?;

    for leg in &legs {
        let is_skipped = skip_check::leg_is_skipped(pool, leg).await?;
        let was_skipped = queries::skip_notification_state(pool, &leg.user_id, leg.journey_leg_id)
            .await?
            .unwrap_or(false);

        if decision::decide_skip_notification(was_skipped, is_skipped) != decision::NotifyDecision::NotifyNow
        {
            continue;
        }

        let payload = NotificationPayload {
            title: "A stop on your journey is being skipped".to_string(),
            body: format!(
                "Your service between {} and {} is no longer calling at one of those stops today.",
                leg.origin_crs, leg.destination_crs
            ),
            url: format!("/journeys/{}", leg.journey_id),
            tag: format!("journey-leg-skip-{}", leg.journey_leg_id),
        };

        if send_to_all_subscriptions(pool, &leg.user_id, &payload, vapid_private_key, vapid_subject).await? {
            queries::upsert_skip_notification_state(pool, &leg.user_id, leg.journey_leg_id, true, now)
                .await?;
        }
    }

    Ok(())
}
```

  **Note on the payload's body copy**: it deliberately does not say
  *which* end (origin or destination) is skipped — `leg_is_skipped`
  collapses both into one boolean by design (Task 9, Step 1's own doc
  comment), matching §5.2's framing of "is this leg affected at all" as
  the notify-worthy signal; distinguishing which end in the push copy
  itself would require `leg_is_skipped` to return the richer
  `LegSkipStatus`-shaped result instead of a bare `bool`, which is a
  reasonable enhancement but not required by the spec's own wording and
  adds a second place (besides Task 3's `LegSkipStatus`) that would need to
  stay in sync — left as a stated, deliberate simplification rather than
  silently narrowed scope.

- [ ] **Step 5: Verify**

```bash
cargo build -p notifier
cargo clippy -p notifier --all-features --all-targets -- -D warnings
cargo test -p notifier
```

  Expected: clean build/lint; every existing and newly-added non-`#[ignore]`d
  test still passes.

- [ ] **Step 6: Commit**

```bash
git add crates/notifier/src/skip_check.rs crates/notifier/src/main.rs crates/notifier/src/config.rs
git commit -m "notifier: add the station-skip check's own interval branch (§5.2)"
```

---

## Task 10: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Full workspace check**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Every DB-gated test this plan added**, against a real local
  Postgres with Phase 1's migrations already applied:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p notifier -- --ignored --test-threads=1
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api -- --ignored --test-threads=1
```

- [ ] **Step 3: Frontend**

```bash
npm test
npm run build
```

- [ ] **Step 4: Manual end-to-end walkthrough**, against a running dev
  stack with a real journey tracked (matched leg, real `trains_id`):
  1. Seed a `station_samples` row for the leg's own `origin_crs` whose
     matching entry's `skipped_stations` includes the leg's `destination_crs`
     (a direct `UPDATE station_samples SET departures = ...` against the
     dev DB is the fastest way to force this without waiting for a real
     Darwin sample to actually show a skip).
  2. Confirm the journey view (`/journeys/{id}`) renders the red "Skipped"
     badge next to the correct stop in that leg's timeline.
  3. Run the notifier locally with `--skip-check-poll-interval-secs 5`
     against the same database and a subscribed test push endpoint;
     confirm a "A stop on your journey is being skipped" push notification
     arrives once, and does **not** repeat on the next cycle (dedup via
     `journey_leg_notification_state`).
  4. Revert the seeded sample (or wait for a real poll to overwrite it) and
     confirm no further push fires for the same leg (§5.2/decision.rs's
     escalation-only, no-de-escalation-notify posture).
  5. Force a real delay/cancellation on a multi-leg journey's second leg
     and confirm the resulting push says "Leg 2 of '<name>' is
     delayed"/"...was cancelled" with a `/journeys/{id}` URL, while a
     single-leg journey's equivalent push still reads exactly as it does
     today ("Your train is delayed", `/track/{id}`).

- [ ] **Step 5: No commit** — this task is verification only; if any step
  above surfaces a real bug, fix it as part of the task that introduced it
  and re-run this task's steps before considering Phase 3 done.
