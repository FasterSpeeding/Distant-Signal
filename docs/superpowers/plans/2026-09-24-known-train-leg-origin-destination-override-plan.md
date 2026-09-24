# `knownTrain`-mode leg origin/destination override

**Status: approved for implementation.** Written directly from a
same-session investigation of the real codebase (file/line citations
below), not from a separate design doc. No spec doc exists elsewhere for
this fix; this plan is its own authority, subordinate only to the real,
cited evidence in it.

## Why

`crates/api/src/routes/journeys.rs`'s `CreateJourneyLegRequest::KnownTrain`
(`POST /Journeys`) and `AddJourneyLegRequest::KnownTrain` (`POST
/Journeys/{journeyId}/legs`) carry only `train_uid`/`service_date` — no
origin/destination CRS of any kind. `crates/api/src/data/journeys.rs`'s
`create_journey_with_known_train_leg` and `add_known_train_leg_to_journey`
read the new leg's `origin_crs`/`destination_crs` back off
`train_subscriptions.pin_origin_crs`/`pin_destination_crs`, which
`train_tracking::create_subscription_for_train` copies from the matched
`trains` row's own `origin_crs`/`destination_crs` — i.e. the train's own
full working, start to end, not wherever this particular traveller actually
boards or alights.

This is silent, not just incomplete: `frontend/components/PlanTripFlow.tsx`
(the dynamic trip planner's commit step) already has the real per-leg
`originCrs`/`destinationCrs` locally, on every `kind: 'train'` entry of a
`GET /Trips/plan` itinerary (`TripPlanLeg`, `frontend/lib/types.ts`) — CSA/
RAPTOR routinely returns an itinerary where a leg's own origin/destination
differ from the underlying train's real full route (boarding a
Birmingham→Glasgow service at Crewe, alighting at Preston) — but there is
nowhere on either request shape to send them, so `PlanTripFlow.tsx`
discards them today and lets the backend derive the wrong values from the
pin. `frontend/components/AddJourneyLegButton.tsx`'s manual "I know the
train" flow has the identical gap in its request shape, but no origin/
destination *input* of its own to lose in the first place — its `knownTrain`
mode has always relied on the pin-derived value being what the traveller
wants (see Judgment Call 2 below for why that stays true after this fix).

This plan adds real, optional origin/destination overrides to both
`KnownTrain` request shapes, threads them through the data layer, and wires
`PlanTripFlow.tsx`'s commit step to send the itinerary's real per-leg
values instead of silently dropping them.

## Judgment calls (ruled on up front)

1. **Omit-both-fields behavior is unchanged, by construction.** Both new
   fields are `Option<String>` with `#[serde(default)]`. When both are
   `None` (every existing caller: `AddJourneyLegButton.tsx`'s `knownTrain`
   submission, `TrackThisTrainButton.tsx`, an older frontend build), the
   data layer's existing pin-read-back path runs completely unchanged. When
   either is `Some`, that field specifically uses the override instead of
   the pin-derived value — the other field (if `None`) still falls back to
   the pin, so a caller can override just one side.
2. **`AddJourneyLegButton.tsx` stays behavior-unchanged, deliberately.**
   Its `knownTrain` mode has no origin/destination *input* at all today (it
   only asks for a train UID + service date, see its own `mode ===
   'knownTrain'` render branch) — there is no real per-leg value in that
   component to send that isn't already the pin's own value. Wiring it to
   send explicit overrides would mean inventing new form fields for a
   manual-entry flow nobody asked for; out of scope for this fix, which
   exists to unblock the trip planner, which *does* already have the real
   value. `frontend/lib/types.ts`'s `NewJourneyLegRequest` type documents
   this decision at the point future readers will look for it.
3. **No validation of an override against the train's real calling
   points, for now — documented, not silently skipped.** Both routes
   resolve `trains_id` via `trains::find_or_create_train`
   (`crates/api/src/data/trains.rs`), a bare `(train_uid, service_date)`
   identity upsert that does **not** populate `calling_points` (or even
   `origin_crs`/`destination_crs`) for a brand-new `trains` row — that data
   arrives later, best-effort, via `routes::train::enrich_shared_train`,
   which both routes call **after** they've already built and returned
   their response. There is no real schedule data to validate an override
   against synchronously, for a train identity seen for the first time, at
   the point either route would need to run that check. Fetching the
   train's real calling points synchronously just for this validation would
   mean adding a new schedule lookup neither route makes today, changing
   the route's latency/failure semantics for every caller, not just the
   (today, zero) callers who supply an override — a materially bigger
   change than this fix's scope. This plan therefore validates only that a
   *supplied* override is a well-formed 3-letter CRS code (the same
   posture `validate_window_leg`/`journey_templates::validate_template_leg`
   already take for a manually-typed CRS field), and defers real
   calling-point validation to a future plan if it turns out to matter in
   practice — `PlanTripFlow.tsx`'s own commit flow already only ever sends
   a structurally valid CRS pair by construction (it's copied straight off
   a resolved `GET /Trips/plan` itinerary leg), so the gap this leaves open
   is scoped to `AddJourneyLegButton.tsx`'s manual flow ever adopting these
   fields later, which Judgment Call 2 above rules is not this plan's job.
4. **Normalization matches `window`-mode's convention, not `pin`-mode's.**
   `create_journey_with_window_leg`'s route caller trims + uppercases
   before storing (`origin_crs.trim().to_ascii_uppercase()`,
   `post_journey`'s `Window` arm). `pin`-mode's `TrackPinRequest.origin_crs`
   is stored as-is (a long-standing, separate inconsistency, out of this
   plan's scope to fix). A `knownTrain` override is structurally the same
   "caller-typed CRS string" as a `window`-mode field (as opposed to
   `pin`-mode's legacy path), so it gets the same trim+uppercase treatment
   at the route layer before it ever reaches the data layer or `insert_leg`.

## Global Constraints (bind every task)

- Every new wire field is additive and optional (`Option<String>` +
  `#[serde(default)]` on the Rust side, `originCrs?`/`destinationCrs?` on
  the TypeScript side) — an older frontend build, or any caller that omits
  them, keeps today's exact behavior.
- `create_journey_with_known_train_leg`/`add_known_train_leg_to_journey`
  each gain exactly two new trailing parameters:
  `origin_crs_override: Option<&str>`, `destination_crs_override: Option<&str>`
  — update every call site (2 routes in `crates/api/src/routes/journeys.rs`,
  2 existing test call sites in `crates/api/src/data/journeys.rs`'s
  `db_tests` module, at the time of writing:
  `add_known_train_leg_to_journey_sets_train_subscription_and_manual_mode`
  and
  `add_leg_to_journey_a_non_owner_cannot_add_a_leg_to_someone_elses_journey`)
  to pass `None, None` unless the test is specifically about the new
  behavior.
- Match this repo's own established doc-comment rigor: every doc comment
  that currently states or implies "the caller only ever has a bare
  `(trainUid, serviceDate)` for this mode" (both data-layer functions named
  above) must be corrected, not just left stale beside new code that
  contradicts it.
- DB-gated tests in `crates/api` follow the file's own established
  convention exactly: `#[tokio::test]` + `#[ignore = "requires a live
  database; see this plan's Global Constraints for the DATABASE_URL
  incantation, then run with `cargo test -p api <test_name> -- --ignored
  --test-threads=1`"]`, the existing `connect()`/`seed_user()`/
  `cleanup_user()` helpers, `crate::data::trains::find_or_create_train` to
  seed a fixture train. Run against a live Postgres with migrations applied
  via `sqlx migrate run --source crates/api/migrations`; incantation:
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/<db> cargo test
  -p api -- --ignored --test-threads=1 --skip incident_search_query_tests`.
- `cargo fmt --all`, then `cargo clippy --workspace --all-features
  --all-targets -- -D warnings`, then `cargo test --workspace -- --skip
  incident_search_query_tests` must be clean after every backend task (the
  `incident_search_query_tests` module is a known pre-existing unrelated
  sandbox hang — skip it, do not investigate it).
- Frontend: `npx tsc --noEmit` and `npm run lint` clean after the frontend
  task. Vitest via `mise exec node@22 -- npx vitest run` (the ambient
  sandbox Node has a known unrelated `localStorage`/jsdom incompatibility —
  if a FRESH `origin/main` checkout shows the same failures, they are not
  this plan's regression).
- Do not touch `TrackThisTrainButton.tsx` — it tracks a whole train from a
  train-detail/listing page with no planned-itinerary leg context at all,
  so it has no real per-leg override value to send either, for the exact
  same reason `AddJourneyLegButton.tsx` doesn't (Judgment Call 2). Not one
  of the two call sites this plan is scoped to fix.

## Task 1 — Backend data layer: validator + threading the overrides

Files: `crates/api/src/data/journeys.rs`.

1. Add a new pure validator, placed near `validate_window_leg` (same
   module, same "route validates, data layer writes" split its own doc
   comment already documents):

   ```rust
   pub fn validate_known_train_overrides(
       origin_crs: Option<&str>,
       destination_crs: Option<&str>,
   ) -> Result<(), String> {
       if let Some(origin_crs) = origin_crs {
           if origin_crs.trim().len() != 3 {
               return Err(
                   "Enter a valid origin station — CRS codes are three letters, like WOK or EUS."
                       .to_string(),
               );
           }
       }
       if let Some(destination_crs) = destination_crs {
           if destination_crs.trim().len() != 3 {
               return Err(
                   "Enter a valid destination station — CRS codes are three letters, like WOK or \
                    EUS."
                       .to_string(),
               );
           }
       }
       Ok(())
   }
   ```

   Doc comment: cite `validate_window_leg`/`journey_templates::validate_template_leg`
   as the precedent for the exact check and message wording, and state this
   plan's Judgment Call 3 verbatim (why no real-calling-points check here) —
   don't just say "not implemented," say why, citing
   `trains::find_or_create_train` by name and the fact that
   `enrich_shared_train` runs only after the response is already built.
2. Change `create_journey_with_known_train_leg`'s signature to:
   ```rust
   pub async fn create_journey_with_known_train_leg(
       pool: &PgPool,
       user_id: &str,
       custom_name: Option<&str>,
       trains_id: i64,
       service_date: NaiveDate,
       origin_crs_override: Option<&str>,
       destination_crs_override: Option<&str>,
   ) -> anyhow::Result<(i64, i64, i64)> {
   ```
   Body: after reading `pins` back (rename the tuple to `(pin_origin_crs,
   pin_destination_crs)` for clarity), compute:
   ```rust
   let origin_crs = origin_crs_override.map(str::to_string).or(pin_origin_crs);
   let destination_crs = destination_crs_override.map(str::to_string).or(pin_destination_crs);
   ```
   and pass `origin_crs.as_deref()`/`destination_crs.as_deref()` into
   `insert_leg` exactly where the old `origin_crs.as_deref()`/
   `destination_crs.as_deref()` were. Rewrite the doc comment's "Never
   independently supplied by the caller ... " sentence to describe the new
   override behavior precisely (both fields optional, either one alone
   overridable, `None`+`None` reproduces today's exact behavior) and add a
   line pointing at this plan's Judgment Call 3 for why no calling-points
   check runs here.
3. Make the exact same two changes (signature + body + doc comment) to
   `add_known_train_leg_to_journey`.
4. Update the two existing test call sites named in Global Constraints to
   pass `None, None` as the two new trailing arguments — no behavior change
   to either test.
5. Add a `#[test]` (no DB, alongside `validate_window_leg`'s own plain
   `#[test]`s near line 1048) covering `validate_known_train_overrides`:
   both `None` -> `Ok`; a valid 3-letter origin with `None` destination ->
   `Ok`; a 2-letter destination with `None` origin -> `Err` naming the
   destination (not the origin) in its message; both invalid -> `Err`
   (origin's message takes priority, matching the sequential-check order in
   the function body — assert on the origin's message text specifically so
   this ordering is pinned down, not incidental).
6. Add three new `#[tokio::test]` DB-gated tests (same file, same
   `db_tests` module, same `#[ignore]` convention, same `cleanup_user`
   posture as `add_known_train_leg_to_journey_sets_train_subscription_and_manual_mode`):
   - `create_journey_with_known_train_leg_overrides_both_ends_when_given`:
     seed a fixture train via `find_or_create_train` (no schedule data, so
     `pin_origin_crs`/`pin_destination_crs` are both `None` off the bare
     upsert — proves the override is what lands, not a coincidental pin
     value), call `create_journey_with_known_train_leg` with
     `Some("CRE")`/`Some("PRE")`, read the leg back via `get_owned_leg`,
     assert `origin_crs == Some("CRE")` and `destination_crs ==
     Some("PRE")`.
   - `create_journey_with_known_train_leg_overrides_one_end_and_falls_back_to_the_pin_for_the_other`:
     seed a `trains` row WITH real `origin_crs`/`destination_crs` set
     directly via `INSERT INTO trains (train_uid, service_date, origin_crs,
     destination_crs) VALUES (...)` (same raw-SQL fixture pattern
     `list_journeys_for_user_picks_the_earliest_non_completed_leg` already
     uses to seed a `trains` row with real columns), so
     `create_subscription_for_train`'s pin read-back is non-`None` for
     both ends; call `create_journey_with_known_train_leg` with
     `Some(override_origin)` and `None` for destination; assert the leg's
     `origin_crs` is the override and `destination_crs` is the pin-derived
     value (proves partial override + partial fallback both work in the
     same call).
   - `create_journey_with_known_train_leg_omitting_both_overrides_reproduces_the_pin_derived_behavior`:
     same seeded-`trains`-row fixture as the previous test, call with
     `None, None`, assert both fields equal the pin-derived values —
     this is the explicit regression test for Judgment Call 1's
     backward-compatibility claim.
   Use a distinct `TEST-JOURNEY-KNOWN-OVERRIDE-*` user id per test (this
   file's own per-test-user-id convention) and `cleanup_user` at the end of
   each. `add_known_train_leg_to_journey`'s own override path does not need
   a fourth duplicate DB test — Task 1's job is proving the shared
   override-selection logic works once; that logic is byte-identical in
   both functions per this brief's step 2/3, and
   `add_known_train_leg_to_journey_sets_train_subscription_and_manual_mode`
   already exercises the `None, None` path for that sibling function
   end-to-end via Task 1 step 4's update.
7. Run `cargo fmt --all --check`, `cargo clippy --workspace --all-features
   --all-targets -- -D warnings`, `cargo test -p api -- --skip
   incident_search_query_tests` (non-DB tests only is fine for this task;
   Task 2 or the final verification pass is where DB-gated tests actually
   run against a live database — report which you managed).

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary (including whether DB-gated tests in this
task's own scope were run against a live Postgres or not, and why),
concerns.

## Task 2 — Backend route layer: wire shape + validation call + doc comments

Files: `crates/api/src/routes/journeys.rs`. Depends on Task 1's new
`create_journey_with_known_train_leg`/`add_known_train_leg_to_journey`
signatures already existing on this branch.

1. `CreateJourneyLegRequest::KnownTrain` (currently `train_uid: String,
   service_date: NaiveDate`): add
   ```rust
   #[serde(default)]
   origin_crs: Option<String>,
   #[serde(default)]
   destination_crs: Option<String>,
   ```
   Doc comment: state plainly that these are the leg's OWN
   boarding/alighting points, distinct from the train's real full route,
   and that omitting both reproduces the historical pin-derived behavior —
   cite this plan's Judgment Calls 1 and 3.
2. Make the identical field addition (fields + doc comment) to
   `AddJourneyLegRequest::KnownTrain`.
3. In `post_journey`'s `CreateJourneyLegRequest::KnownTrain` match arm:
   destructure the two new fields, call
   `journeys::validate_known_train_overrides(origin_crs.as_deref(),
   destination_crs.as_deref())` (map its `Err` to `(StatusCode::BAD_REQUEST,
   msg)`, same as the `Window` arm's `validate_window_leg` call) BEFORE the
   existing `find_or_create_train` call, then normalize per Judgment Call 4:
   ```rust
   let origin_override = origin_crs.as_deref().map(|s| s.trim().to_ascii_uppercase());
   let destination_override = destination_crs.as_deref().map(|s| s.trim().to_ascii_uppercase());
   ```
   and pass `origin_override.as_deref()`, `destination_override.as_deref()`
   as the two new trailing arguments to `create_journey_with_known_train_leg`.
4. Make the identical change (destructure, validate, normalize, pass
   through) to `post_journey_leg`'s `AddJourneyLegRequest::KnownTrain` arm,
   calling `add_known_train_leg_to_journey`.
5. Extend the two existing wire-format tests
   (`known_train_and_window_mode_legs_deserialize_their_camel_case_fields`
   and
   `add_journey_leg_known_train_and_window_mode_legs_deserialize_their_camel_case_fields`,
   this file's `leg_candidate_json_tests`... no — check the actual enclosing
   `#[cfg(test)]` module name at the point you edit, these two tests live in
   whichever module currently contains them, do not assume a name not
   confirmed by reading the file) to also assert: (a) a `knownTrain` JSON
   body omitting `originCrs`/`destinationCrs` still deserializes with both
   `None` (the pre-existing JSON literal already omits them — just add the
   `origin_crs`/`destination_crs` `None` assertions to the existing
   `matches!`/destructure); (b) a NEW small test,
   `known_train_mode_leg_deserializes_its_optional_origin_and_destination_overrides`,
   asserting a JSON body WITH `originCrs`/`destinationCrs` set deserializes
   both as `Some(...)` for both `CreateJourneyLegRequest::KnownTrain` and
   `AddJourneyLegRequest::KnownTrain`.
6. Run `cargo fmt --all --check`, `cargo clippy --workspace --all-features
   --all-targets -- -D warnings`, `cargo test --workspace -- --skip
   incident_search_query_tests`. Then, if a live Postgres is reachable in
   this sandbox (see this plan's own environment notes — try `psql` /
   `pg_ctl` first; if genuinely unreachable after reasonable troubleshooting,
   say so explicitly and move on): apply migrations
   (`sqlx migrate run --source crates/api/migrations`) and run
   `DATABASE_URL=postgres://postgres:postgres@localhost:5432/<a-db-you-create>
   cargo test -p api -- --ignored --test-threads=1 --skip
   incident_search_query_tests`, confirming Task 1's three new DB-gated
   tests (and every pre-existing DB-gated test) pass for real.

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary (state explicitly whether DB-gated tests ran
against a live database), concerns.

## Task 3 — Frontend: types, `PlanTripFlow.tsx` wiring, stale-comment cleanup, tests

Files: `frontend/lib/types.ts`, `frontend/components/PlanTripFlow.tsx`,
`frontend/components/PlanTripFlow.test.tsx`. Depends on Task 2 (wire shape
must already exist so the manual test-body assertions below match the real
backend contract) — read Tasks 1 and 2 above for the exact field names
(`originCrs`/`destinationCrs`, both optional).

1. `frontend/lib/types.ts`: add `originCrs?: string; destinationCrs?:
   string;` to `NewJourneyLegRequest`'s `knownTrain` variant (around line
   911-916). Rewrite that type's own doc comment (currently describing only
   the two existing wire shapes) to document: these two new fields are
   optional overrides for the leg's own boarding/alighting point, omitting
   both reproduces the historical pin-derived backend behavior, and
   `AddJourneyLegButton.tsx` deliberately never sets them (cite this plan's
   Judgment Call 2 by name/summary) because its `knownTrain` mode has no
   per-leg origin/destination input to send in the first place.
2. `frontend/components/PlanTripFlow.tsx`:
   - First-leg `POST /Journeys` call (currently `body: JSON.stringify({
     leg: { mode: 'knownTrain', trainUid: firstLeg.trainUid, serviceDate:
     firstLeg.serviceDate } })`): add `firstLeg.originCrs`/
     `firstLeg.destinationCrs` to the `leg` object, but ONLY when
     non-`null` — `TripPlanLeg`'s train variant types both as `string |
     null`, and the backend fields are optional, so omit the key entirely
     for a `null` value rather than sending an explicit `null` (mirrors
     `TrackTrainForm.tsx`'s own `...(destinationCrs.trim() ? {
     destinationCrs: ... } : {})` conditional-spread convention for an
     optional field). Concretely:
     ```ts
     leg: {
       mode: 'knownTrain',
       trainUid: firstLeg.trainUid,
       serviceDate: firstLeg.serviceDate,
       ...(firstLeg.originCrs ? { originCrs: firstLeg.originCrs } : {}),
       ...(firstLeg.destinationCrs ? { destinationCrs: firstLeg.destinationCrs } : {}),
     },
     ```
   - Subsequent-leg `POST /Journeys/{id}/legs` call inside the `for` loop
     (currently `body: JSON.stringify({ mode: 'knownTrain', trainUid:
     leg.trainUid, serviceDate: leg.serviceDate })`): identical treatment
     using `leg.originCrs`/`leg.destinationCrs`.
   - Rewrite BOTH doc comments that currently describe this as a "Known,
     pre-existing backend limitation ... out of scope for this task to
     fix" (the block above the first `fetch('/api/Journeys', ...)` call,
     and the shorter one above the subsequent-leg `fetch` inside the loop)
     to instead state plainly that the leg's real origin/destination is now
     sent, name the two backend fields (`originCrs`/`destinationCrs`) and
     the two request shapes they were added to
     (`CreateJourneyLegRequest::KnownTrain`/`AddJourneyLegRequest::KnownTrain`,
     `crates/api/src/routes/journeys.rs`), and note the `null`-means-omit
     convention above. Do not simply delete the comments — a future reader
     needs to know this was deliberately fixed, not that it never existed.
3. `frontend/components/PlanTripFlow.test.tsx`: every existing
   `expect(fetchMock).toHaveBeenCalledWith(...)`/`toHaveBeenNthCalledWith(...)`
   assertion whose `body` is an exact `JSON.stringify({...})` of a
   `knownTrain`-mode request (as of this writing: the first test, "creates
   the journey via POST /api/Journeys...", one assertion; the "creates a
   leg per train across multiple segments..." test, two assertions, one per
   leg) must be updated to include `originCrs`/`destinationCrs` matching
   that test's own fixture (`EUS`/`MKC` and `MKC`/`EDB` respectively, per
   the fixtures already in this file) — these will otherwise fail once
   step 2 ships, since the real request body now differs from what these
   assertions currently expect. Grep for `mode: 'knownTrain'` in this file
   to find every one before you finish; do not assume the count above is
   exhaustive if the file has drifted since this brief was written.
4. Add ONE new test to this file proving the fix's actual point — that the
   sent origin/destination is the LEG's own, not the overall segment's (a
   segment's advertised origin/destination and a train leg's real
   boarding/alighting point can legitimately differ when the itinerary has
   a transfer leg before/after the train leg). Model:
   `it('sends the train leg's own origin/destination, not the segment's, for a leg that boards/alights mid-route', async () => { ... })`
   with a fixture itinerary for ONE segment (`originCrs: 'BHM', destinationCrs:
   'GLC'`) whose itinerary has legs `[{ kind: 'train', trainUid: 'X12345',
   serviceDate: '2026-09-23', originCrs: 'CRE', destinationCrs: 'PRE',
   scheduledDeparture: '10:00:00', scheduledArrival: '11:15:00',
   arrivalDayOffset: 0 }]` (a train leg whose own origin/destination,
   Crewe→Preston, differ from the segment's Birmingham→Glasgow — the
   plan's own Birmingham→Glasgow/Crewe→Preston example, made concrete).
   Assert the `POST /Journeys` call's body is exactly
   `JSON.stringify({ leg: { mode: 'knownTrain', trainUid: 'X12345', serviceDate: '2026-09-23', originCrs: 'CRE', destinationCrs: 'PRE' } })`
   — i.e. `CRE`/`PRE` (the leg's own), never `BHM`/`GLC` (the segment's).
5. Run `npx tsc --noEmit`, `npm run lint`, and `mise exec node@22 -- npx
   vitest run PlanTripFlow` (scope to this file's suite; the full suite has
   the known unrelated sandbox jsdom/localStorage failures noted in Global
   Constraints — if this file's own tests are clean, that's this task's
   bar). Confirm against a fresh `origin/main` checkout first if anything
   fails, to separate a real regression from the known pre-existing issue.

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary, concerns.

## Final Verification (after all tasks, before final review)

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-features --all-targets -- -D warnings`
- `cargo test --workspace -- --skip incident_search_query_tests`
- DB-gated: apply migrations (`sqlx migrate run --source
  crates/api/migrations`) against a live Postgres if reachable, then
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/<a-db-you-create>
  cargo test -p api -- --ignored --test-threads=1 --skip
  incident_search_query_tests`. If Postgres is genuinely unreachable in
  this sandbox after reasonable troubleshooting (see this plan's own
  environment notes on `dnf`/`initdb`), say so explicitly rather than
  silently skipping the claim.
- `npx tsc --noEmit`
- `npm run lint`
- `mise exec node@22 -- npx vitest run` (compare against a fresh
  `origin/main` checkout for any failure outside `PlanTripFlow.test.tsx`
  before treating it as this plan's regression)
