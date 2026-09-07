# Train Listing Page (`/trains`, Approach B v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `/trains` — a destination-first, whole-network, CIF-derived
train search page — alongside (never replacing) `/track`, plus a "Track this
train" CTA on the existing public `/train/[uid]/[date]` page, so a user can
find an arbitrary train by where it's going rather than only by a station
they already know, click through to its public status page, and start
tracking it in one action.

**Architecture:** A second grouping pass in `crates/schedule-query`
(`departures_by_destination_crs`, sibling to the already-shipped
`departures_by_crs`, run against the *same* transient, per-cycle
`ScheduleIndex`) buckets every non-cancelled schedule's `now`-forward,
departure-bearing calling points by their schedule's **destination** CRS
instead of by their own origin CRS. `crates/schedule-reference` publishes
that grouping on its existing 30-minute cycle to a new
`POST /private/schedule-destination-departures` ingest route, which upserts
a new `schedule_destination_departures` table — structurally a copy of
`schedule_network_departures`'s own migration/ingest/read triple. A new
public read route, `GET /public/trains/search?destination=&origin=&from=&to=`,
filters that table's JSONB bucket server-side (a `LEFT JOIN LATERAL
jsonb_array_elements` so "no published row" and "row published, filters
matched nothing" stay distinguishable) and returns camelCase rows. On the
frontend, a new client component `TrainSearchForm` drives that route through
the existing same-origin `/api/*` proxy and renders each result with a link
to `/train/{uid}/{today}` and a shared `TrackThisTrainButton` that calls
`POST /Train/by-uid/{uid}/{date}/track` — with `TrackTrainForm`'s exact
best-effort ticket-attach follow-up on `/trains`, and without it on
`/train/[uid]/[date]` (per the spec's own §5 exclusion).

**Tech Stack:** Rust (`axum`, `sqlx` runtime-checked `query`/`query_as` — no
`query!` macro, `anyhow`, `chrono`, `serde_json`, `reqwest`), PostgreSQL 16,
Next.js/React (Server Component pages + `'use client'` interactive
components, Mantine UI), Vitest + `@testing-library/react` +
`renderWithMantine`, `cargo test` with `#[tokio::test] #[ignore]`-gated
live-database tests.

**Spec:** `docs/superpowers/specs/2026-09-07-train-listing-page-design.md`
(read in full; every reference to "the design doc" below means this
document). Its §3 **Recommendation — Approach B** is a settled decision this
plan does not re-litigate: Approaches A and C are not built, not partially
built, and not kept as fallbacks anywhere in this plan.

## Decisions this plan resolves (the design doc's own §7 Open Questions)

The design doc closed with six open questions. This plan resolves the two
that actually block implementation, and records why the other four are
deliberately left alone.

1. **Open Question 1 — the destination-keyed bucket's cap/cardinality is
   unmeasured.** Resolved by **Task 1**, a controller-run diagnostic that
   measures the real distribution against a real CIF extract *before* Task 4
   hard-codes `MAX_DEPARTURES_PER_DESTINATION`. This mirrors the
   shared-train-identity plan's own Task 6 precedent ("Backfill diagnostic —
   count the un-repointable edge case before Step B runs"): a decision only a
   human with the real data can make, turned into a real, executable,
   explicitly-gated step rather than an assumption baked into a constant.

2. **Open Question 2 — is `train_tracking::create_subscription_for_train`
   idempotent for a second click by the same user on the same train?**
   **Resolved definitively: NO, it is not, and this was already known and
   proven in-repo.** Evidence, read directly:
   - `crates/api/src/data/train_tracking.rs:162-169` — the function's own doc
     comment states it verbatim: *"NOT idempotent by `(trains_id, user_id)`
     — every call inserts a new row, same as `create_pin`'s own
     long-established behavior for the legacy path… Calling this twice for
     the same train therefore creates two separate subscriptions, each
     independently rename/delete-able."*
   - `crates/api/src/data/train_tracking.rs:170-187` — the implementation is
     a bare `INSERT … SELECT … RETURNING id` with **no** `ON CONFLICT`, no
     existence check, no `WHERE NOT EXISTS`.
   - `crates/api/src/data/train_tracking.rs:3506-3556` — a live-database test,
     `create_subscription_for_train_called_twice_creates_two_separate_subscriptions`,
     asserts `assert_ne!(first_tracking_id, second_tracking_id)` and
     `assert_eq!(row_count, 2)`.
   - There is **no** uniqueness constraint backing it: the only index that
     ever guarded per-user train identity,
     `tracked_trains_resolved_identity`
     (`crates/api/migrations/20260828120000_train_tracking.sql:85`), was
     dropped outright by
     `crates/api/migrations/20260906140000_drop_legacy_columns.sql`, and
     `crates/api/migrations/20260907100000_rename_tracked_trains.sql` renamed
     the table to `train_subscriptions` with only three
     non-unique indexes carried over.

   **Task 8 therefore exists and is ordered before every task that wires a
   CTA** (Tasks 9-12). Its fix is **application-level, inside
   `create_subscription_for_train` only** — a single atomic
   existing-row-wins CTE — and deliberately **not** a `(user_id, trains_id)`
   unique index. That was considered and rejected on direct evidence: four
   separate, unrelated code paths already do a bare
   `UPDATE train_subscriptions SET trains_id = $2 WHERE id = $1`
   (`crates/api/src/data/schedule_matching.rs:131`,
   `crates/api/src/data/trust_event_backlog_match.rs:394` and `:504`,
   `crates/api/src/data/train_tracking.rs:684`), and `create_pin`
   (`train_tracking.rs:79-100`) has never deduplicated legacy pins at all —
   so a user with two legacy CRS+time pins for the same physical train is
   reachable *today*, and a global unique index would turn each of those four
   `UPDATE`s into a hard failure inside schedule matching, backlog matching,
   and live TRUST resolution. Task 8 states this tradeoff in the code, and
   Task 9 closes the remaining concurrent-double-submit window the
   application-level fix cannot close, the same way every other mutating
   control in this app already does: an in-flight `busy` state that disables
   the button.

3. **Open Question 3 (mixed-source rows), 4 (retention/pruning parity),
   5 (shareable filter state in the URL), 6 (same-day VSTP amendments).**
   - 3 needs no resolution: this plan's `/trains` is **CIF-derived only**
     (the design doc's own §3 Approach B v1 scope and §6's "no true
     whole-network live-board destination search"), so no result row has two
     possible sources and there is nothing to merge or disambiguate.
   - 4 is resolved by inspection rather than a task:
     `schedule_network_departures` has **no** retention/pruning job anywhere
     (confirmed — no `prune_schedule_network_departures` exists in
     `crates/aggregator`, and `upsert_schedule_network_departures`
     wholesale-replaces per `(crs, service_date)` each cycle). The new
     sibling table gets exactly the same posture — per-key wholesale replace,
     no pruning job — so parity is achieved by doing nothing, and Task 2's
     migration says so in its own header comment rather than leaving a reader
     to wonder.
   - 5 **is** implemented, because it costs nothing here: Task 11's `/trains`
     page reads `?destination=`, `?origin=` and `?ticketId=` out of
     `searchParams` exactly the way `frontend/app/track/page.tsx:7,10-21`
     already reads `?origin=`/`?ticketId=`. This is a direct consequence of
     the §4 requirement that `/trains` be reachable via a `?ticketId=`
     deep link; extending the same mechanism to the two filter fields is one
     extra prop each.
   - 6 is genuinely out of this plan's scope — it is inherited, unchanged,
     from two prior shipped features, and nothing here makes it better or
     worse.

## Global Constraints

These are the design doc's §6 exclusions, binding on **every** task below.
If a task seems to need one of these, stop and re-read the design doc — it
doesn't.

- **No change to `/Train/track`, `TrackPinRequest`, or `post_track`'s
  matching logic.** This plan only adds a new discovery surface and a new
  CTA; the legacy fallback path is untouched.
- **No CIF operator/`BX`/headcode decoding.** `schedule_query::records`
  carries no operator field and this plan does not add one. There is **no
  operator filter** on `/trains` — CIF rows have no operator field at all.
- **No browsing a date other than today.** Both the LDBWS and CIF sources
  stay "always now/today, server-side" (`get_station_schedule_departures`'s
  own posture, `crates/api/src/routes/departures.rs:71-88`). The new route
  computes `today` server-side and accepts no date parameter.
- **No resident, permanently-in-memory whole-network index.** The new
  grouping pass is transient and per-cycle, run against the same
  stack-local `ScheduleIndex` `publish_cif_derived_products` already builds
  once per cycle (`crates/schedule-reference/src/main.rs:194`) — never a
  second parse, never a cached index.
- **No synchronous request-time call from `api` into `schedule-reference`**
  (or any other batch poller). Publish-then-poll only, same as every
  existing cross-service link in this app.
- **No broadening of `poller-ldbws`'s ~286-station sampled set.**
- **No whole-network LDBWS (live-board) destination search.** CIF-derived
  only for this v1.
- **No merging of LDBWS and CIF-derived rows for the same train/station.**
- **No `ticketId` deep-link convention for `/train/[uid]/[date]`.** The
  `/trains` page's own "Track this train" action gets full ticket-attach
  parity (design doc §4); the `/train/[uid]/[date]` CTA gets **no**
  `attachTicketId` at all (design doc §5, explicit).
- **No pixel-level UI design.** Use plain, functional Mantine markup
  consistent with `frontend/components/TrackTrainForm.tsx`'s own structure
  (`Stack`/`Group`/`Autocomplete`/`ScrollArea`/`Button`/`Alert`). Every
  component in this plan ships complete, real markup — "left to design
  review" is not a valid step in any task.
- **`crates/api` uses runtime-checked `sqlx::query`/`sqlx::query_as`
  exclusively** — no `query!`/`query_as!`, no `.sqlx` query cache. Every new
  query in this plan follows that.
- **New JSON responses stay camelCase**, produced the same way every other
  hand-built response in this crate is — a `serde_json::json!()` field-by-field
  render function in `crates/api/src/render.rs`, not a
  `#[derive(Serialize)]` struct (see `schedule_departure_json`,
  `crates/api/src/render.rs:171-181`, the exact sibling to copy).
- **Backend live-database tests** follow this crate's existing convention
  exactly: a `#[cfg(test)] mod db_tests` block, a private `connect()` reading
  `DATABASE_URL` via `PgPoolOptions`, `#[tokio::test]` +
  `#[ignore = "requires a live database; …"]`, seed via raw `sqlx::query`
  INSERT, and an explicit `DELETE` cleanup at the end of the test body. Do
  **not** use `#[sqlx::test]` — it is not used anywhere in this crate.
  Fixture CRS codes come from the reserved `Z…` namespace and must not
  collide with ones already claimed (`ZQQ`/`ZQR`/`ZQS` by
  `routes/station_stats.rs`, `ZQT`/`ZQU`/`ZQV`/`ZQW`/`ZQX`/`ZQY`/`ZQZ`/`ZRA`
  by `routes/departures.rs` and `routes/ingest.rs`). This plan claims
  `ZRB`-`ZRF`.
- **Frontend tests** use Vitest + `@testing-library/react` +
  `renderWithMantine` (`frontend/test/render`), run via `npm test -- <file>`
  from the `frontend/` directory.

---

## Task 1: Cap/cardinality diagnostic — measure the destination bucket before any constant is hard-coded

**This task deliberately does not follow the TDD template.** There is no
code to write and no commit to make: this is the design doc's own Open
Question 1 ("Approach B's per-bucket cap and cardinality are unmeasured…
This needs a real check against `timetable_full.zip` before an
implementation plan commits to a specific cap") turned into a real,
executable, gating step — the same shape as the shared-train-identity plan's
Task 6.

> **CONTROLLER-RUN. Do not delegate this task to an implementer subagent.**
> It reads a real CIF extract (and, in its fallback form, production
> database state). This session's standing rule is that production
> database/SSH access is performed by the primary controlling agent
> directly, never by a subagent working unsupervised. Task 4 must not be
> dispatched until this task's number has been recorded.

**Files:** none created, none changed, nothing committed.

**Interfaces:**
- Produces: one number — the observed per-destination bucket size
  distribution (max, and roughly the 99th percentile) — plus a recorded
  decision on the value of `MAX_DEPARTURES_PER_DESTINATION`, consumed by
  Task 4 Step 3, which hard-codes it.

- [ ] **Step 1: Get a real CIF extract, locally, without touching production**

Preferred and sufficient. This diagnostic is **fully locally reproducible**
— it needs only a CIF `MCA` file, not the database and not SSH — so prefer
this form over the production fallback in Step 4:

```bash
# Either: a fresh timetable_full.zip pulled from the Network Rail SCHEDULE
# feed into the repo root (untracked; this is the same file
# crates/schedule-query/examples/inspect.rs's own module doc already
# documents as the manual re-check input).
ls -la timetable_full.zip

# Or: copy the current delivery's MCA file off the running schedulefeed
# PVC, which is exactly what schedule-reference itself reads
# (crates/schedule-reference/src/discovery.rs).
kubectl cp <namespace>/<schedulefeed-pod>:/data/schedule-feed/<timestamp>/RJTTF<n>MCA.txt ./RJTTFMCA.txt -c schedule-reference
```

- [ ] **Step 2: Run the destination-bucket histogram**

This needs no new Rust and no new binary — the two facts the cap decision
turns on (how many distinct trains terminate at the busiest destination, and
how many departure-bearing calling points they contribute) are both readable
straight off the CIF text. A `BS` record opens a schedule block, `LO`/`LI`
records are its departure-bearing calling points, and the `LT` record's
TIPLOC (bytes 3-9, 1-indexed, per `crates/schedule-query/src/records.rs`'s
own offset documentation) is its destination:

```bash
unzip -p timetable_full.zip 'RJTTF*MCA.txt' | awk '
  /^BS/ { dep = 0; next }
  /^LO/ { dep++; next }
  /^LI/ { dep++; next }
  /^LT/ { dest = substr($0, 3, 7); sub(/ +$/, "", dest);
          trains[dest]++; points[dest] += dep; next }
  END   { for (d in trains) printf "%s\t%d\t%d\n", d, trains[d], points[d] }
' | sort -k3 -rn | head -30
```

(If Step 1 produced a plain `RJTTFMCA.txt` instead of the zip, replace the
`unzip -p …` prefix with `cat ./RJTTFMCA.txt`.)

Columns are `TIPLOC`, `distinct schedule records terminating there`,
`total departure-bearing calling points bucketed under it`. The third column
is the one that matters — it is the direct analogue of what
`departures_by_destination_crs` (Task 3) will put in one bucket.

**Read the number correctly — it is an over-count, deliberately.** This awk
pass counts *every* schedule record in the file, across every date range and
days-of-week bitmask, with no STP-overlay resolution and no `now`-forward
filter. The real per-cycle bucket for one date, after
`resolve_for_date` picks one record per UID and the `now`-forward filter
drops everything already departed, is **strictly smaller**. Erring high is
the correct direction for a cap decision.

- [ ] **Step 3: Record the number and decide the cap**

Write the top-30 output into this plan's execution record (a comment on the
tracking issue/PR, or appended to this file's checklist) before Task 4 is
dispatched. Then decide:

- **If the largest third-column value is ≤ ~200:** set
  `MAX_DEPARTURES_PER_DESTINATION = 200` in Task 4 Step 3, exactly as
  written there. 200 is this plan's default because it is comfortably above
  a "busiest terminus on the busiest day" over-count while still bounding
  the publish payload — see the size check below.
- **If it is materially larger (say, > 500):** raise the constant to the
  next round number above the observed maximum *and* re-run the size check
  below before dispatching Task 4. If the size check fails at that value,
  stop and flag it back to the repo owner: the honest fix would be
  pagination or a narrower `now`-forward window, both of which are their own
  design pass and are not this plan's to invent unprompted.

**The size check** (run it with the chosen constant, not skipped): the
publish is one batch-array POST of roughly
`(distinct destination CRS codes) × (cap) × (~55 bytes per JSON entry)`.
With ~2,500 CRS codes and a cap of 200 that is ~27MB — under the private
router's real limit, `DefaultBodyLimit::max(100 * 1024 * 1024)`
(`crates/api/src/routes/mod.rs:86`), with ~3.7x headroom. Redo this
arithmetic against whatever cap Step 3 actually picks; if the product
exceeds ~60MB, the cap is too high for a single-POST publish.

- [ ] **Step 4: Fallback ONLY if no CIF extract can be obtained**

If Step 1 genuinely cannot produce an extract, the weaker proxy is a
controller-run read of the already-published origin-keyed table, which at
least bounds total per-cycle volume:

```bash
psql "$DATABASE_URL" -c "
  SELECT count(*) AS crs_rows,
         sum(jsonb_array_length(departures)) AS total_departures
  FROM schedule_network_departures
  WHERE service_date = CURRENT_DATE;"
```

`total_departures` here is capped at 10 per origin station, so it **cannot**
answer the destination-cardinality question directly — it only tells you the
network-wide per-cycle order of magnitude. Treat a result from this fallback
as grounds for choosing the conservative end of the range (200), not as a
substitute for Step 2. Run this yourself; do not hand it to a subagent.

- [ ] **Step 5: No commit for this task**

Nothing in the repository changed. Proceed to Task 2 immediately (it does
not depend on this number); Task 4 is the one gated on it.

---

## Task 2: Migration — `schedule_destination_departures`

**Files:**
- Create: `crates/api/migrations/20260907130000_schedule_destination_departures.sql`
- Test: none of its own — this repo has no migration-testing framework; a
  migration is proven by the next Rust code that queries the new shape
  (Task 5's `db_tests`). Same convention as
  `20260904110000_schedule_network_departures.sql`, which has no test file
  either.

**Interfaces:**
- Consumes: nothing.
- Produces: table `schedule_destination_departures (destination_crs TEXT NOT
  NULL, service_date DATE NOT NULL, departures JSONB NOT NULL, updated_at
  TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (destination_crs,
  service_date))`. Tasks 5, 6 and 7 all depend on this migration having
  applied.

- [ ] **Step 1: Verify the table does not exist yet**

Run: `psql "$DATABASE_URL" -c "SELECT to_regclass('public.schedule_destination_departures');"`
Expected: prints an empty result (`NULL`) — there is something real to build.

- [ ] **Step 2: Write the migration**

```sql
-- ---------------------------------------------------------------------
-- One row per (destination_crs, service_date): every CIF-SCHEDULE-derived,
-- `now`-forward departure-bearing calling point of every non-cancelled
-- schedule TERMINATING at `destination_crs`, for one rail day, capped per
-- destination and published by schedule-reference on its existing cycle.
--
-- The destination-keyed sibling of schedule_network_departures
-- (20260904110000_schedule_network_departures.sql), which is keyed by the
-- departure's OWN origin CRS. Both are produced from the SAME transient,
-- per-cycle ScheduleIndex in the same pass over the same delivery -- see
-- crates/schedule-reference/src/main.rs's publish_cif_derived_products --
-- so this table costs one extra grouping pass and one extra POST, not a
-- second parse and not a resident index. See
-- docs/superpowers/specs/2026-09-07-train-listing-page-design.md, Approach
-- B (the recommended one), and §0 point 5 for why an origin-keyed bucket
-- could not answer "which trains go to X".
--
-- `departures` is opaque JSONB here -- a Vec<schedule_query::DestinationDeparture>
-- ({uid, origin_crs, scheduled}) -- and `api` never deserializes it into
-- that Rust type. Unlike its origin-keyed sibling, `api` does not merely
-- store and relay the blob: GET /public/trains/search filters INTO it with
-- jsonb_array_elements (see queries::search_schedule_destination_departures),
-- because a destination bucket is far larger than a station's next-10 and
-- shipping the whole thing to the client to filter would defeat the point.
-- The blob stays opaque to serde all the same -- only its `origin_crs` and
-- `scheduled` keys are ever named, in SQL.
--
-- `destination_crs` is deliberately NOT stored on each array element: it is
-- the bucket key and identical for every element, exactly as `crs` is for
-- schedule_network_departures. The read route re-attaches it to each
-- rendered row from the caller's own query parameter.
--
-- RETENTION: none, deliberately, and this matches its sibling exactly.
-- schedule_network_departures has no pruning job anywhere in this repo
-- (there is no prune_schedule_network_departures in crates/aggregator) --
-- each cycle wholesale-replaces the row for its own (key, service_date),
-- so the table's steady-state size is bounded by distinct CRS codes times
-- distinct service dates seen, and stale past-date rows are simply never
-- read (every read is scoped to CURRENT_DATE server-side). This table
-- adopts the same posture rather than inventing a second one. Design doc
-- §7 Open Question 4, resolved by inspection.
-- ---------------------------------------------------------------------

CREATE TABLE schedule_destination_departures (
    destination_crs TEXT        NOT NULL,
    service_date    DATE        NOT NULL,
    departures      JSONB       NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (destination_crs, service_date)
);
```

- [ ] **Step 3: Apply the migration and verify it took**

The migration is applied automatically on `api`'s next startup via
`sqlx::migrate!`, or manually:

```bash
sqlx migrate run --source crates/api/migrations
psql "$DATABASE_URL" -c "SELECT to_regclass('public.schedule_destination_departures');"
```

Expected: prints `schedule_destination_departures`.

- [ ] **Step 4: Confirm nothing else broke**

Run: `cargo test -p api`
Expected: PASS — no existing test touches this table, so this is a pure
regression check that the migration file parses and the crate still builds.

- [ ] **Step 5: Commit**

```bash
git add crates/api/migrations/20260907130000_schedule_destination_departures.sql
git commit -m "Add schedule_destination_departures table for destination-first train search"
```

---

## Task 3: `schedule-query` — `departures_by_destination_crs`

**Files:**
- Modify: `crates/schedule-query/src/records.rs` (add `DestinationDeparture`
  immediately after `ScheduleDeparture`, which ends at line 188)
- Modify: `crates/schedule-query/src/resolve.rs` (add
  `departures_by_destination_crs` immediately after `departures_by_crs`,
  which ends at line 216; add tests to the existing `#[cfg(test)] mod tests`)
- Modify: `crates/schedule-query/src/lib.rs` (add both names to the two
  existing `pub use` lists)

**Interfaces:**
- Consumes: existing `ScheduleIndex`, `resolve_for_date`, `normalize_tiploc`
  — all unchanged.
- Produces:
  `pub struct DestinationDeparture { pub uid: String, pub origin_crs: String,
  pub scheduled: chrono::NaiveTime }` (Serialize + Deserialize + Debug +
  Clone + PartialEq + Eq), and
  `pub fn departures_by_destination_crs(index: &ScheduleIndex, date:
  NaiveDate, now: NaiveTime, tiploc_to_crs: &HashMap<String, String>) ->
  HashMap<String, Vec<DestinationDeparture>>`. Both re-exported from
  `schedule_query`'s crate root. Consumed by Task 4.

- [ ] **Step 1: Write the failing tests**

Add these to the existing `#[cfg(test)] mod tests` block in
`crates/schedule-query/src/resolve.rs`, immediately after
`departures_by_crs_buckets_an_intermediate_calling_point_departure_under_its_own_crs`
(which ends at line 559). They reuse that module's existing `basic`,
`calling_point`, `calling_point_with_departure`, `tiploc_map`, `WEEKDAYS`
and `c11052_with_departures` helpers verbatim — nothing new to define.

```rust
    #[test]
    fn departures_by_destination_crs_buckets_an_origin_departure_under_the_schedules_destination() {
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(
            by_destination.len(),
            1,
            "the ONLY bucket key is the schedule's destination (CRE), never its origin"
        );
        assert!(
            !by_destination.contains_key("EUS"),
            "this function must not also bucket by origin -- that is departures_by_crs's job"
        );
        let crewe = &by_destination["CRE"];
        assert_eq!(crewe.len(), 1);
        assert_eq!(crewe[0].uid, "C11052");
        assert_eq!(crewe[0].origin_crs, "EUS");
        assert_eq!(
            crewe[0].scheduled,
            NaiveTime::from_hms_opt(8, 22, 0).unwrap()
        );
    }

    #[test]
    fn departures_by_destination_crs_buckets_every_departure_bearing_calling_point_under_one_destination() {
        // The load-bearing difference from departures_by_crs: a train from
        // EUSTON to MNCRPIC calling at CREWE contributes TWO entries to the
        // SAME (MAN) bucket -- "next train to Manchester from anywhere"
        // must find it whether the searcher is at Euston or at Crewe. This
        // is also exactly why this bucket's cardinality needed its own
        // sizing pass (Task 1) rather than reusing the origin-keyed cap.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination.len(), 1, "one destination, one bucket");
        let manchester = &by_destination["MAN"];
        assert_eq!(manchester.len(), 2);
        let mut origins: Vec<&str> = manchester.iter().map(|d| d.origin_crs.as_str()).collect();
        origins.sort();
        assert_eq!(origins, vec!["CRE", "EUS"]);
    }

    #[test]
    fn departures_by_destination_crs_excludes_a_departure_already_before_now() {
        // Same `now`-forward posture as departures_by_crs (resolve.rs:193):
        // the 08:22 EUSTON departure is gone by 10:00, but the 10:05 CREWE
        // one is still ahead -- the bucket keeps only the latter.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination["MAN"].len(), 1);
        assert_eq!(by_destination["MAN"][0].origin_crs, "CRE");
    }

    #[test]
    fn departures_by_destination_crs_excludes_a_cancelled_schedule_even_though_its_time_has_not_passed() {
        // Real UID/STP/date-range/days values (a base P pattern plus a real
        // STP=C override on 2026-08-31), reusing this module's own
        // c11052_with_departures fixture and its Bank Holiday cross-check.
        let index = ScheduleIndex::build(c11052_with_departures());
        let date = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(); // the cancelled date
        let now = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert!(
            by_destination.is_empty(),
            "the STP=C override must suppress this date's bucket entirely"
        );
    }

    #[test]
    fn departures_by_destination_crs_drops_a_schedule_whose_destination_tiploc_is_unresolved() {
        // The asymmetry with departures_by_crs, and it is deliberate: THERE,
        // an unresolved destination degrades to `destination_crs: None` and
        // the row is still returned under its own origin. HERE the
        // destination IS the bucket key, so there is no honest bucket to
        // file this schedule under -- it is dropped entirely rather than
        // guessed at or filed under a fabricated key.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]); // CREWE deliberately absent

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert!(
            by_destination.is_empty(),
            "an unresolved DESTINATION tiploc drops the whole schedule -- there is no bucket key"
        );
    }

    #[test]
    fn departures_by_destination_crs_drops_only_the_calling_point_whose_own_tiploc_is_unresolved() {
        // Complementary to the test above: an unresolved INTERMEDIATE
        // tiploc drops just that one entry, not the schedule -- the
        // destination bucket still exists and still holds the resolvable
        // calling points. Same "drop, never fabricate" rule as
        // departures_by_crs (resolve.rs:196-198), applied per entry.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        // CREWE deliberately absent; EUSTON and MNCRPIC both resolve.
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination["MAN"].len(), 1);
        assert_eq!(by_destination["MAN"][0].origin_crs, "EUS");
    }

    #[test]
    fn departures_by_destination_crs_never_buckets_the_terminating_calling_point_itself() {
        // A Terminate calling point has no booked_departure by
        // construction (CallingPointKind::Terminate's own doc), so a train
        // must never appear as "departing from X" in X's own arrivals
        // bucket. Guards against a future refactor that starts reading
        // booked_arrival here.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert_eq!(by_destination["CRE"].len(), 1);
        assert_eq!(
            by_destination["CRE"][0].origin_crs, "EUS",
            "CRE must not appear as its own bucket's origin"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-query departures_by_destination_crs`
Expected: FAIL with a compile error — `departures_by_destination_crs` is not
defined and `DestinationDeparture` does not exist.

- [ ] **Step 3: Add the `DestinationDeparture` record**

In `crates/schedule-query/src/records.rs`, immediately after the
`ScheduleDeparture` struct (which ends at line 188):

```rust
/// One departure-bearing calling point of a schedule that TERMINATES at
/// some destination CRS, as bucketed by
/// [`crate::resolve::departures_by_destination_crs`]. The destination CRS
/// itself is deliberately absent from this struct: it is the bucket key
/// (identical for every entry in a bucket), exactly as the origin CRS is
/// the bucket key for [`ScheduleDeparture`]/`departures_by_crs`.
///
/// `origin_crs` means "the station this train departs FROM", which is the
/// calling point's own CRS -- an `Origin` calling point for the first
/// entry, an `Intermediate` one for every later entry of the same
/// schedule. It is NOT necessarily the schedule's own first station, and
/// is deliberately not the same concept as `trains.origin_crs` in
/// `crates/api`, which always is. A caller filtering by "origin" on the
/// train-search route (`GET /public/trains/search?origin=`) is asking
/// "departing from here", which is exactly this field.
///
/// `scheduled` is Europe/London LOCAL civil time, straight off the CIF
/// body, same as [`ScheduleDeparture::scheduled`] -- never UTC. See
/// `crates/schedule-reference/src/main.rs`'s `london_local_time_at` for
/// the one place that distinction is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationDeparture {
    pub uid: String,
    pub origin_crs: String,
    pub scheduled: NaiveTime,
}
```

- [ ] **Step 4: Add the grouping function**

In `crates/schedule-query/src/resolve.rs`, immediately after
`departures_by_crs` (which ends at line 216):

```rust
/// The destination-keyed sibling of [`departures_by_crs`]: every
/// non-cancelled, resolved schedule's `now`-forward, departure-bearing
/// calling points, bucketed by the CRS of that schedule's own TERMINATING
/// calling point rather than by each calling point's own CRS. Backs the
/// destination-first whole-network train search
/// (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B).
///
/// Runs against the SAME already-built, transient, per-cycle
/// [`ScheduleIndex`] as [`departures_by_crs`] -- one extra O(all UIDs)
/// resolve pass plus O(total calling points) bucketing per cycle, no second
/// parse and no resident index (that constraint is restated verbatim in the
/// design doc's §6).
///
/// Two deliberate asymmetries with [`departures_by_crs`], both about the
/// "drop, never fabricate" rule applied to a value that is now a bucket
/// KEY rather than a field:
///
/// * A schedule whose terminating TIPLOC has no `tiploc_to_crs` entry is
///   dropped **entirely** -- there is no honest bucket to file it under.
///   `departures_by_crs` can degrade the same case to
///   `destination_crs: None` because there the destination is only a
///   field; here it is the key.
/// * A calling point whose OWN TIPLOC has no `tiploc_to_crs` entry drops
///   just that entry, leaving the schedule's other entries in the bucket --
///   identical to `departures_by_crs`'s own per-calling-point drop.
///
/// The caller caps each bucket (see
/// `crates/schedule-reference/src/main.rs`'s
/// `MAX_DEPARTURES_PER_DESTINATION`); this function itself is uncapped and
/// unsorted, exactly like `departures_by_crs`.
pub fn departures_by_destination_crs(
    index: &ScheduleIndex,
    date: NaiveDate,
    now: NaiveTime,
    tiploc_to_crs: &HashMap<String, String>,
) -> HashMap<String, Vec<crate::records::DestinationDeparture>> {
    let mut by_destination: HashMap<String, Vec<crate::records::DestinationDeparture>> =
        HashMap::new();

    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        let Some(destination_crs) = resolved
            .calling_points
            .last()
            .and_then(|last| tiploc_to_crs.get(normalize_tiploc(&last.tiploc)))
        else {
            continue;
        };
        for cp in &resolved.calling_points {
            let Some(departure) = cp.booked_departure else {
                continue;
            };
            if departure < now {
                continue;
            }
            let Some(origin_crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else {
                continue;
            };
            by_destination
                .entry(destination_crs.clone())
                .or_default()
                .push(crate::records::DestinationDeparture {
                    uid: resolved.uid.clone(),
                    origin_crs: origin_crs.clone(),
                    scheduled: departure,
                });
        }
    }

    by_destination
}
```

- [ ] **Step 5: Export both names from the crate root**

In `crates/schedule-query/src/lib.rs`, extend the two existing `pub use`
lists (alphabetically, matching how they are already ordered):

```rust
pub use records::{
    BasicSchedule, CallingPoint, CallingPointKind, DestinationDeparture, LinePopulationEntry,
    RawSchedule, ScheduleDeparture, StpIndicator,
};
pub use resolve::{
    ResolvedSchedule, ScheduleIndex, departures_by_crs, departures_by_destination_crs, match_pin,
    resolve_for_date, schedules_touching,
};
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p schedule-query`
Expected: PASS — all seven new tests plus every pre-existing one (in
particular the six `departures_by_crs_*` tests must still pass **unchanged**;
this task adds a sibling function and must not have altered that one's
behaviour).

- [ ] **Step 7: Commit**

```bash
git add crates/schedule-query/src/records.rs crates/schedule-query/src/resolve.rs crates/schedule-query/src/lib.rs
git commit -m "Add departures_by_destination_crs, the destination-keyed grouping pass"
```

---

## Task 4: `schedule-reference` — publish the destination-keyed buckets

> **GATED ON TASK 1.** Do not start this task until Task 1's number has been
> recorded and the cap decided. Step 3 hard-codes a constant whose value is
> Task 1's output.

**Files:**
- Modify: `crates/schedule-reference/src/config.rs` (add
  `schedule_destination_departures_url` after
  `schedule_network_departures_url`, which ends at line 55)
- Modify: `crates/schedule-reference/src/main.rs` (add the constant, the
  publish function and the pure row-shaping function after
  `schedule_network_departures_rows`, which ends at line 329; add one call
  in `publish_cif_derived_products`, currently ending at line 214; add tests
  to the existing `#[cfg(test)] mod poll_once_tests`)
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml:288-289`
  (add the new env var alongside `SCHEDULE_NETWORK_DEPARTURES_URL`)

**Interfaces:**
- Consumes: `schedule_query::departures_by_destination_crs(index, date, now,
  &tiploc_to_crs) -> HashMap<String, Vec<schedule_query::DestinationDeparture>>`
  (Task 3); existing `common::ingest::post_batch`,
  `london_local_time_now()`, `Config`.
- Produces: a batch-array POST to `config.schedule_destination_departures_url`
  whose elements are `{"destination_crs": String, "service_date": "YYYY-MM-DD",
  "departures": [{"uid", "origin_crs", "scheduled"}]}` — the exact body shape
  Task 6's ingest route deserializes into
  `queries::ScheduleDestinationDeparturesRow`.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod poll_once_tests` in
`crates/schedule-reference/src/main.rs`, immediately after
`schedule_network_departures_rows_produces_one_row_per_crs_key` (which ends
at line 566):

```rust
    #[test]
    fn schedule_destination_departures_rows_sorts_earliest_first_and_caps_at_the_destination_limit() {
        let mut by_destination = std::collections::HashMap::new();
        let departures: Vec<schedule_query::DestinationDeparture> = (0..24)
            .rev() // deliberately out of order
            .map(|hour| schedule_query::DestinationDeparture {
                uid: format!("U{hour:05}"),
                origin_crs: "EUS".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(hour, 0, 0).unwrap(),
            })
            .collect();
        by_destination.insert("MAN".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["destination_crs"], "MAN");
        assert_eq!(rows[0]["service_date"], "2026-09-07");
        let row_departures = rows[0]["departures"].as_array().unwrap();
        assert_eq!(
            row_departures.len(),
            std::cmp::min(24, MAX_DEPARTURES_PER_DESTINATION),
            "capped at MAX_DEPARTURES_PER_DESTINATION"
        );
        assert_eq!(
            row_departures[0]["uid"], "U00000",
            "earliest-first after sort"
        );
        assert_eq!(row_departures[0]["origin_crs"], "EUS");
    }

    #[test]
    fn schedule_destination_departures_rows_produces_one_row_per_destination_key() {
        let mut by_destination = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![schedule_query::DestinationDeparture {
                uid: "U1".to_string(),
                origin_crs: "EUS".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            }],
        );
        by_destination.insert(
            "EDB".to_string(),
            vec![schedule_query::DestinationDeparture {
                uid: "U2".to_string(),
                origin_crs: "KGX".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            }],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(rows.len(), 2);
        let keys: Vec<&str> = rows
            .iter()
            .map(|r| r["destination_crs"].as_str().unwrap())
            .collect();
        assert!(keys.contains(&"MAN"));
        assert!(keys.contains(&"EDB"));
        for row in &rows {
            assert_eq!(row["service_date"], "2026-09-07");
        }
    }

    #[test]
    fn schedule_destination_departures_rows_keeps_the_earliest_entries_when_capping_a_mixed_origin_bucket() {
        // Discriminating check on WHICH entries survive the cap, not just
        // how many: a destination bucket mixes origins freely (that is the
        // whole point of this grouping), so a naive truncate-before-sort
        // would silently keep an arbitrary subset. Two origins, interleaved
        // times, deliberately inserted latest-first.
        let mut by_destination = std::collections::HashMap::new();
        let mut departures = Vec::new();
        for hour in (0..MAX_DEPARTURES_PER_DESTINATION + 4).rev() {
            departures.push(schedule_query::DestinationDeparture {
                uid: format!("U{hour:05}"),
                origin_crs: if hour % 2 == 0 { "EUS" } else { "CRE" }.to_string(),
                // Minutes, so the count can exceed 24 without overflowing an hour.
                scheduled: chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                    (hour as u32) * 60,
                    0,
                )
                .unwrap(),
            });
        }
        by_destination.insert("MAN".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);
        let row_departures = rows[0]["departures"].as_array().unwrap();

        assert_eq!(row_departures.len(), MAX_DEPARTURES_PER_DESTINATION);
        assert_eq!(row_departures[0]["uid"], "U00000");
        assert_eq!(
            row_departures[MAX_DEPARTURES_PER_DESTINATION - 1]["uid"],
            format!("U{:05}", MAX_DEPARTURES_PER_DESTINATION - 1),
            "the cap must drop the LATEST entries, never an arbitrary subset"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-reference schedule_destination_departures_rows`
Expected: FAIL with a compile error — neither
`schedule_destination_departures_rows` nor `MAX_DEPARTURES_PER_DESTINATION`
exists yet.

- [ ] **Step 3: Add the constant and the pure row-shaping function**

In `crates/schedule-reference/src/main.rs`, immediately after
`schedule_network_departures_rows` (which ends at line 329):

```rust
/// Per-destination bucket cap for the destination-keyed publish. **NOT a
/// copy of `MAX_DEPARTURES_PER_STATION`, and deliberately much larger.**
/// That constant is 10 because it mirrors `poller-ldbws`'s own `num_rows`
/// default for a single station's departure board; a destination-keyed
/// bucket aggregates every `now`-forward departure of every train
/// terminating at one CRS, from anywhere on the network, which is a
/// materially different and (before this feature) wholly unmeasured
/// distribution -- the design doc's own §7 Open Question 1.
///
/// This value was set from a real measurement, not by precedent-matching:
/// see Task 1 of
/// docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md,
/// which runs a destination histogram over a real CIF `MCA` extract and
/// records the result before this constant is written. If you are changing
/// it, re-run that diagnostic AND its payload-size check (the publish is
/// one batch POST, bounded by `DefaultBodyLimit::max(100 * 1024 * 1024)` on
/// `crates/api/src/routes/mod.rs`'s private router) rather than guessing.
const MAX_DEPARTURES_PER_DESTINATION: usize = 200;

/// Pure sort/cap/JSON-shaping logic, split out of
/// `publish_schedule_destination_departures` purely so it is unit-testable
/// without a mock HTTP server -- same convention as
/// `schedule_network_departures_rows` directly above.
///
/// Sort BEFORE truncate is load-bearing, not stylistic: a destination
/// bucket mixes origins and times freely, so truncating first would keep an
/// arbitrary `HashMap`-order subset rather than the next
/// `MAX_DEPARTURES_PER_DESTINATION` trains.
fn schedule_destination_departures_rows(
    mut by_destination: std::collections::HashMap<
        String,
        Vec<schedule_query::DestinationDeparture>,
    >,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_destination
        .drain()
        .map(|(destination_crs, mut departures)| {
            departures.sort_by_key(|d| d.scheduled);
            departures.truncate(MAX_DEPARTURES_PER_DESTINATION);
            serde_json::json!({
                "destination_crs": destination_crs,
                "service_date": today,
                "departures": departures,
            })
        })
        .collect()
}

/// The destination-keyed sibling of `publish_schedule_network_departures`
/// directly above: same one-batch-array POST shape, same
/// `london_local_time_now()` `now`-forward boundary, same
/// `tiploc_to_crs` map built from this cycle's already-resolved
/// `stanox_crs_records`, same log-and-continue error posture (a failed POST
/// just means this cycle's grouping is discarded and rebuilt next cycle).
/// Only the grouping key differs. See
/// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B.
async fn publish_schedule_destination_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = stanox_crs_records
        .iter()
        .map(|r| {
            (
                schedule_query::normalize_tiploc(&r.tiploc).to_string(),
                r.crs.clone(),
            )
        })
        .collect();
    let now = london_local_time_now();

    let by_destination =
        schedule_query::departures_by_destination_crs(index, today, now, &tiploc_to_crs);
    let rows = schedule_destination_departures_rows(by_destination, today);

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_destination_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived destination departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish schedule-derived destination departures; will retry next cycle");
    }
}
```

- [ ] **Step 4: Wire the publish into the existing shared cycle**

In `publish_cif_derived_products` (`crates/schedule-reference/src/main.rs`,
currently ending at line 214), add the third publish call at the end of the
function body, after the existing `publish_schedule_network_departures` call:

```rust
    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    // Third CIF-derived product off the SAME one-per-cycle ScheduleIndex
    // and the SAME `today` -- the design doc's Approach B is explicit that
    // this must not trigger a second parse or a resident index.
    publish_schedule_destination_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
```

- [ ] **Step 5: Add the config field**

In `crates/schedule-reference/src/config.rs`, immediately after
`schedule_network_departures_url` (which ends at line 55):

```rust
    /// The `api` crate's ingestion endpoint for this service's fourth
    /// responsibility: the destination-keyed, CIF-derived whole-network
    /// train-search publish. See
    /// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
    /// Approach B. POST-only, no GET pair -- same shape as
    /// `schedule_network_departures_url` directly above, and reusing the
    /// same `internal_oauth_group_schedule_reference` writer credential.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-destination-departures"
    )]
    pub schedule_destination_departures_url: String,
```

- [ ] **Step 6: Add the Helm env var**

In `charts/distant-signal/templates/schedulefeed-deployment.yaml`,
immediately after the existing `SCHEDULE_NETWORK_DEPARTURES_URL` block
(lines 288-289):

```yaml
            # Train-listing-page plan: this crate's fourth responsibility,
            # the destination-keyed sibling of the publish directly above.
            - name: SCHEDULE_DESTINATION_DEPARTURES_URL
              value: {{ printf "%s/private/schedule-destination-departures" (include "distant-signal.apiBaseUrl" .) | quote }}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p schedule-reference`
Expected: PASS — all three new tests plus every pre-existing one (the two
`schedule_network_departures_rows_*` tests in particular must still pass
unchanged).

Then run the chart lint so the template edit is proven syntactically valid:

```bash
helm lint charts/distant-signal
```

Expected: `1 chart(s) linted, 0 chart(s) failed`.

- [ ] **Step 8: Commit**

```bash
git add crates/schedule-reference/src/main.rs crates/schedule-reference/src/config.rs charts/distant-signal/templates/schedulefeed-deployment.yaml
git commit -m "Publish CIF-derived destination-keyed departures from schedule-reference"
```

---

## Task 5: `api` data layer — upsert and filtered search

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add the row struct, the upsert
  and the search function immediately after
  `latest_schedule_network_departures`, which ends at line 895; add a new
  test module at the end of the file)

**Interfaces:**
- Consumes: table `schedule_destination_departures` (Task 2).
- Produces:
  - `pub struct ScheduleDestinationDeparturesRow { pub destination_crs:
    String, pub service_date: chrono::NaiveDate, pub departures:
    serde_json::Value }` (`Debug + Clone + Deserialize`) — consumed by
    Task 6's ingest handler as a `Json<Vec<…>>` body.
  - `pub async fn upsert_schedule_destination_departures(pool: &PgPool, rows:
    &[ScheduleDestinationDeparturesRow]) -> Result<u64>` — consumed by
    Task 6.
  - `pub async fn search_schedule_destination_departures(pool: &PgPool,
    destination_crs: &str, service_date: chrono::NaiveDate, origin_crs:
    Option<&str>, from_time: Option<&str>, to_time: Option<&str>, limit: i64)
    -> Result<Option<Vec<serde_json::Value>>>` — consumed by Task 7.
    `None` means "no row published for this `(destination_crs,
    service_date)` at all" (the 404 case); `Some(vec![])` means "published,
    but nothing matched the filters" (the `200 []` case). `from_time`/
    `to_time` are `"HH:MM:SS"` strings, compared lexicographically against
    the stored `scheduled` values (which serde writes as `"HH:MM:SS"`) —
    that ordering is identical to chronological ordering for a fixed-width
    24-hour clock string, which is why no cast is needed.

- [ ] **Step 1: Write the failing tests**

Add this new module at the very end of `crates/api/src/data/queries.rs`:

```rust
/// Tested at the query level rather than through a route harness -- same
/// posture as this file's other `*_query_tests` modules. The real SQL here
/// (the `LEFT JOIN LATERAL jsonb_array_elements` that keeps "no published
/// row" and "published but nothing matched" distinguishable, plus the three
/// optional filters) is exactly what needs live-database coverage;
/// `routes/trains.rs`'s handler is a thin parse/render wrapper over it.
#[cfg(test)]
mod schedule_destination_departures_query_tests {
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

    async fn delete_fixture(pool: &PgPool, destination_crs: &str) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE destination_crs = $1")
            .bind(destination_crs)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    fn fixture_date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()
    }

    /// Three trains to one destination from two origins, at three times --
    /// enough to discriminate all three filters independently.
    async fn seed_fixture(pool: &PgPool, destination_crs: &str) {
        delete_fixture(pool, destination_crs).await;
        let departures = serde_json::json!([
            {"uid": "C10001", "origin_crs": "EUS", "scheduled": "08:22:00"},
            {"uid": "C10002", "origin_crs": "CRE", "scheduled": "10:05:00"},
            {"uid": "C10003", "origin_crs": "EUS", "scheduled": "18:40:00"},
        ]);
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (destination_crs, service_date, departures) VALUES ($1, $2, $3)",
        )
        .bind(destination_crs)
        .bind(fixture_date())
        .bind(departures)
        .execute(pool)
        .await
        .expect("seed fixture row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_wholesale_replaces_an_existing_row_for_the_same_key() {
        let pool = connect().await;
        delete_fixture(&pool, "ZRB").await;

        let first = vec![ScheduleDestinationDeparturesRow {
            destination_crs: "ZRB".to_string(),
            service_date: fixture_date(),
            departures: serde_json::json!([{"uid": "OLD", "origin_crs": "EUS", "scheduled": "08:00:00"}]),
        }];
        let upserted = upsert_schedule_destination_departures(&pool, &first)
            .await
            .expect("first upsert");
        assert_eq!(upserted, 1);

        let second = vec![ScheduleDestinationDeparturesRow {
            destination_crs: "ZRB".to_string(),
            service_date: fixture_date(),
            departures: serde_json::json!([{"uid": "NEW", "origin_crs": "CRE", "scheduled": "09:00:00"}]),
        }];
        upsert_schedule_destination_departures(&pool, &second)
            .await
            .expect("second upsert");

        let (stored,): (serde_json::Value,) = sqlx::query_as(
            "SELECT departures FROM schedule_destination_departures \
             WHERE destination_crs = 'ZRB' AND service_date = $1",
        )
        .bind(fixture_date())
        .fetch_one(&pool)
        .await
        .expect("read back");
        assert_eq!(
            stored.as_array().unwrap().len(),
            1,
            "a fresh cycle wholesale-replaces the bucket, never merges into it"
        );
        assert_eq!(stored[0]["uid"], "NEW");

        delete_fixture(&pool, "ZRB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_no_published_row_is_none_not_an_empty_vec() {
        // The whole reason for the Option: `None` becomes a 404 ("nothing
        // published for this destination today"), `Some(vec![])` becomes a
        // `200 []` ("published, but your filters matched nothing"). These
        // are different facts and must never collapse.
        let pool = connect().await;
        delete_fixture(&pool, "ZRC").await;

        let result =
            search_schedule_destination_departures(&pool, "ZRC", fixture_date(), None, None, None, 100)
                .await
                .expect("search");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_a_published_row_but_no_matches_is_some_empty_vec() {
        let pool = connect().await;
        seed_fixture(&pool, "ZRC").await;

        let result = search_schedule_destination_departures(
            &pool,
            "ZRC",
            fixture_date(),
            Some("ZZZ"), // no fixture row has this origin
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert_eq!(
            result,
            Some(Vec::new()),
            "a published-but-unmatched bucket is Some(empty), never None"
        );

        delete_fixture(&pool, "ZRC").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_no_filters_returns_every_entry_earliest_first() {
        let pool = connect().await;
        seed_fixture(&pool, "ZRD").await;

        let rows =
            search_schedule_destination_departures(&pool, "ZRD", fixture_date(), None, None, None, 100)
                .await
                .expect("search")
                .expect("row is published");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(rows[1]["uid"], "C10002");
        assert_eq!(rows[2]["uid"], "C10003");

        delete_fixture(&pool, "ZRD").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_filters_by_origin_crs() {
        let pool = connect().await;
        seed_fixture(&pool, "ZRD").await;

        let rows = search_schedule_destination_departures(
            &pool,
            "ZRD",
            fixture_date(),
            Some("CRE"),
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("row is published");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C10002");
        assert_eq!(rows[0]["origin_crs"], "CRE");

        delete_fixture(&pool, "ZRD").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_filters_by_an_inclusive_time_range() {
        // Inclusive at BOTH ends, and discriminating about it: 10:05:00 is
        // the exact lower bound here and must be returned, while 08:22:00
        // (below it) and 18:40:00 (above the upper bound) must not.
        let pool = connect().await;
        seed_fixture(&pool, "ZRE").await;

        let rows = search_schedule_destination_departures(
            &pool,
            "ZRE",
            fixture_date(),
            None,
            Some("10:05:00"),
            Some("12:00:00"),
            100,
        )
        .await
        .expect("search")
        .expect("row is published");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C10002");

        delete_fixture(&pool, "ZRE").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_is_scoped_to_the_requested_service_date_only() {
        // Proves the "always today, server-side" scoping: a stale bucket
        // from a different service_date must never leak through.
        let pool = connect().await;
        delete_fixture(&pool, "ZRF").await;

        let yesterday = fixture_date() - chrono::Duration::days(1);
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (destination_crs, service_date, departures) VALUES ('ZRF', $1, $2)",
        )
        .bind(yesterday)
        .bind(serde_json::json!([{"uid": "STALE", "origin_crs": "EUS", "scheduled": "08:00:00"}]))
        .execute(&pool)
        .await
        .expect("seed a stale fixture row");

        let result =
            search_schedule_destination_departures(&pool, "ZRF", fixture_date(), None, None, None, 100)
                .await
                .expect("search");
        assert!(result.is_none(), "yesterday's bucket must not answer today's query");

        delete_fixture(&pool, "ZRF").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_applies_the_limit() {
        let pool = connect().await;
        seed_fixture(&pool, "ZRE").await;

        let rows =
            search_schedule_destination_departures(&pool, "ZRE", fixture_date(), None, None, None, 2)
                .await
                .expect("search")
                .expect("row is published");
        assert_eq!(rows.len(), 2, "limit caps the result");
        assert_eq!(
            rows[0]["uid"], "C10001",
            "the limit keeps the EARLIEST entries -- ORDER BY runs before LIMIT"
        );

        delete_fixture(&pool, "ZRE").await;
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api schedule_destination_departures -- --ignored`
Expected: FAIL with a compile error — neither
`ScheduleDestinationDeparturesRow`,
`upsert_schedule_destination_departures`, nor
`search_schedule_destination_departures` is defined.

- [ ] **Step 3: Implement all three items**

In `crates/api/src/data/queries.rs`, immediately after
`latest_schedule_network_departures` (which ends at line 895):

```rust
/// One `POST /private/schedule-destination-departures` batch element --
/// query-scoped, deserialized straight off the request body by
/// `routes::ingest::post_schedule_destination_departures`. Defined here
/// (the data layer), not in `routes/ingest.rs`, so the data layer never
/// depends on a route-layer type -- same direction as every other
/// dependency between these two files, and the exact shape of
/// `ScheduleNetworkDeparturesRow` above.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDestinationDeparturesRow {
    pub destination_crs: String,
    pub service_date: chrono::NaiveDate,
    pub departures: serde_json::Value,
}

/// Upserts one cycle's batch of per-destination CIF-derived departures --
/// wholesale replaces any existing row for each `(destination_crs,
/// service_date)`, never merges (a fresh cycle's grouping pass supersedes
/// the prior one entirely). Same one-transaction, one
/// `INSERT ... ON CONFLICT` per row shape as
/// `upsert_schedule_network_departures` directly above.
pub async fn upsert_schedule_destination_departures(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO schedule_destination_departures
                (destination_crs, service_date, departures, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (destination_crs, service_date) DO UPDATE SET
                departures = EXCLUDED.departures,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.destination_crs)
        .bind(row.service_date)
        .bind(&row.departures)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// The destination-first train search's one read. Filters INSIDE the
/// published JSONB bucket rather than returning the whole blob for the
/// caller to filter: a destination bucket holds up to
/// `MAX_DEPARTURES_PER_DESTINATION` entries (see
/// `crates/schedule-reference/src/main.rs`), far more than
/// `schedule_network_departures`' next-10, so shipping it whole would
/// defeat the point of a server-side search.
///
/// `Ok(None)` means no row is published for `(destination_crs,
/// service_date)` at all -- the caller maps that to a `404`, the same
/// honesty split `get_station_schedule_departures` already draws.
/// `Ok(Some(vec![]))` means a row IS published but nothing in it matched
/// the filters -- a `200 []`. The `LEFT JOIN LATERAL` (rather than a plain
/// comma join) is exactly what keeps those two distinguishable in ONE
/// round trip: an unmatched-but-published row still yields one result row,
/// carrying a `NULL` element.
///
/// `from_time`/`to_time` are `"HH:MM:SS"` strings and are compared as text
/// against the stored `scheduled` values, which serde writes in the same
/// fixed-width form. For a 24-hour clock in that format, lexicographic and
/// chronological order are identical, so no `::time` cast is needed -- and
/// avoiding one keeps the filter usable against the raw JSONB text without
/// per-row parsing. Both bounds are INCLUSIVE.
pub async fn search_schedule_destination_departures(
    pool: &PgPool,
    destination_crs: &str,
    service_date: chrono::NaiveDate,
    origin_crs: Option<&str>,
    from_time: Option<&str>,
    to_time: Option<&str>,
    limit: i64,
) -> Result<Option<Vec<serde_json::Value>>> {
    use sqlx::Row;
    let rows = sqlx::query(
        r#"
        SELECT elem
        FROM schedule_destination_departures d
        LEFT JOIN LATERAL jsonb_array_elements(d.departures) AS elem
            ON ($3::text IS NULL OR elem->>'origin_crs' = $3)
           AND ($4::text IS NULL OR elem->>'scheduled' >= $4)
           AND ($5::text IS NULL OR elem->>'scheduled' <= $5)
        WHERE d.destination_crs = $1 AND d.service_date = $2
        ORDER BY elem->>'scheduled'
        LIMIT $6
        "#,
    )
    .bind(destination_crs)
    .bind(service_date)
    .bind(origin_crs)
    .bind(from_time)
    .bind(to_time)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut matched = Vec::with_capacity(rows.len());
    for row in rows {
        // NULL only for the single placeholder row a published-but-unmatched
        // bucket yields from the LEFT JOIN -- skipped, leaving Some(vec![]).
        let elem: Option<serde_json::Value> = row.try_get("elem")?;
        if let Some(elem) = elem {
            matched.push(elem);
        }
    }
    Ok(Some(matched))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api schedule_destination_departures -- --ignored --test-threads=1 --nocapture`
Expected: PASS — all eight new tests.

- [ ] **Step 5: Run the full crate suite to confirm no regression**

Run: `cargo test -p api`
Expected: PASS — the new tests are `#[ignore]`d so they do not run here,
matching every other live-database test in this file.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add upsert/search queries for schedule_destination_departures"
```

---

## Task 6: `api` ingest route — `POST /private/schedule-destination-departures`

**Files:**
- Modify: `crates/api/src/routes/ingest.rs` (add the route to `router()`,
  currently at lines 32-95, next to the existing
  `/schedule-network-departures` entry at lines 88-91; add the handler after
  `post_schedule_network_departures`, which ends at line 447; add a
  `db_tests` test next to the existing
  `post_schedule_network_departures_upserts_the_row` test at line 1114)
- Modify: `crates/api/src/app.rs` (add the route/method/group triple to
  `build_internal_oauth_routes`, immediately after the
  `/schedule-network-departures` entry at lines 231-235)

**Interfaces:**
- Consumes: `queries::upsert_schedule_destination_departures(pool, rows) ->
  Result<u64>` and `queries::ScheduleDestinationDeparturesRow` (Task 5);
  the existing `UpsertResponse` struct in this file.
- Produces: `POST /private/schedule-destination-departures`, accepting
  `Json<Vec<ScheduleDestinationDeparturesRow>>` and returning
  `Json(UpsertResponse { upserted })`. Authorized by the **existing**
  `internal_oauth_group_schedule_reference` credential — no new group, no
  new secret, no chart change beyond Task 4's env var. Called by Task 4's
  `publish_schedule_destination_departures`.

- [ ] **Step 1: Write the failing test**

Add to the existing `db_tests` module in `crates/api/src/routes/ingest.rs`,
immediately after `post_schedule_network_departures_upserts_the_row` and its
sibling (the module's `delete_fixture`-style helper for that table is at
lines 1094-1101 — this test brings its own, for the new table):

```rust
    async fn delete_destination_departures_fixture(pool: &PgPool, destination_crs: &str) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE destination_crs = $1")
            .bind(destination_crs)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn post_schedule_destination_departures_upserts_the_row() {
        let pool = connect().await;
        delete_destination_departures_fixture(&pool, "ZRB").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let body = serde_json::json!([{
            "destination_crs": "ZRB",
            "service_date": "2026-09-07",
            "departures": [{"uid": "C10001", "origin_crs": "EUS", "scheduled": "08:22:00"}]
        }]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-destination-departures")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(json["upserted"], 1);

        let (stored,): (serde_json::Value,) = sqlx::query_as(
            "SELECT departures FROM schedule_destination_departures \
             WHERE destination_crs = 'ZRB' AND service_date = '2026-09-07'",
        )
        .fetch_one(&pool)
        .await
        .expect("read back the upserted row");
        assert_eq!(stored[0]["uid"], "C10001");
        assert_eq!(stored[0]["origin_crs"], "EUS");

        delete_destination_departures_fixture(&pool, "ZRB").await;
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api post_schedule_destination_departures -- --ignored`
Expected: FAIL — the route is not registered, so the request 404s (the
assertion on `StatusCode::OK` fails), or the crate fails to compile because
`upsert_schedule_destination_departures` is referenced by a handler that
does not exist yet.

- [ ] **Step 3: Register the route**

In `crates/api/src/routes/ingest.rs`'s `router()`, immediately after the
existing `/schedule-network-departures` entry (lines 88-91):

```rust
        .route(
            "/schedule-destination-departures",
            axum::routing::post(post_schedule_destination_departures),
        )
```

Extend that file's existing import of query-layer types to include the new
row struct (the same `use crate::data::queries::{…}` line that already
brings in `ScheduleNetworkDeparturesRow`):

```rust
use crate::data::queries::{ScheduleDestinationDeparturesRow, ScheduleNetworkDeparturesRow};
```

- [ ] **Step 4: Write the handler**

Immediately after `post_schedule_network_departures` (which ends at line
447):

```rust
/// `crates/schedule-reference`'s per-cycle batch of CIF-derived
/// per-DESTINATION departures -- the destination-keyed sibling of
/// `post_schedule_network_departures` directly above, and the write side of
/// the destination-first train search
/// (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B). POST only: no service reads this table back over HTTP --
/// `api` serves it straight off Postgres via
/// `routes::trains::get_trains_search`.
async fn post_schedule_destination_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleDestinationDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_destination_departures(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 5: Authorize the route for `schedule-reference`'s existing credential**

In `crates/api/src/app.rs`'s `build_internal_oauth_routes`, immediately after
the `/schedule-network-departures` entry (lines 231-235):

```rust
        // POST-only, same as /schedule-network-departures directly above,
        // and reusing schedule-reference's EXISTING writer credential --
        // the same one /stanox-crs, /schedule-line-population and
        // /schedule-network-departures already use. A fourth product from
        // the same producer is not a fourth identity.
        (
            "/schedule-destination-departures",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
```

- [ ] **Step 6: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api post_schedule_destination_departures -- --ignored --test-threads=1`
Expected: PASS.

Then: `cargo test -p api`
Expected: PASS, no regressions — in particular
`build_internal_oauth_routes`' own existing tests (if any assert the table's
length or contents) must be updated to account for exactly one new entry,
never rewritten to assert something weaker.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/ingest.rs crates/api/src/app.rs
git commit -m "Add POST /private/schedule-destination-departures ingest route"
```

---

## Task 7: `api` public route — `GET /public/trains/search`

**Files:**
- Create: `crates/api/src/routes/trains.rs`
- Modify: `crates/api/src/routes/mod.rs:7-24` (add `pub mod trains;`),
  `crates/api/src/routes/mod.rs:48-63` (add `.merge(trains::router())` to
  `public_router()`)
- Modify: `crates/api/src/render.rs` (add `destination_departure_json`
  immediately after `schedule_departure_json`, which ends at line 181; add a
  test to the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `queries::search_schedule_destination_departures(pool,
  destination_crs, service_date, origin_crs, from_time, to_time, limit) ->
  Result<Option<Vec<serde_json::Value>>>` (Task 5).
- Produces:
  - `pub(crate) fn destination_departure_json(d: &serde_json::Value,
    destination_crs: &str) -> serde_json::Value`, rendering
    `{uid, scheduled: "HH:MM", originCrs, destinationCrs}`.
  - `GET /public/trains/search?destination={CRS}&origin={CRS}&from=HH:MM&to=HH:MM`
    — `destination` required, the other three optional. `200` with a JSON
    array of the above shape; `400` on a malformed parameter; `404` when no
    bucket is published for `(destination, today)`. Unauthenticated, like
    every other read in `public_router()`. Consumed by Task 10's
    `TrainSearchForm` via the same-origin `/api/*` proxy.

- [ ] **Step 1: Write the failing render test**

Add to the existing `#[cfg(test)] mod tests` in
`crates/api/src/render.rs`, immediately after
`schedule_departure_json_maps_snake_case_to_camel_case_and_trims_seconds`:

```rust
    #[test]
    fn destination_departure_json_maps_snake_case_to_camel_case_and_reattaches_the_destination() {
        let raw = serde_json::json!({
            "uid": "C11052",
            "origin_crs": "EUS",
            "scheduled": "08:22:00",
        });
        let json = destination_departure_json(&raw, "MAN");
        assert_eq!(
            json,
            serde_json::json!({
                "uid": "C11052",
                "scheduled": "08:22",
                "originCrs": "EUS",
                "destinationCrs": "MAN",
            })
        );
        assert!(
            json.get("origin_crs").is_none(),
            "no stray snake_case field"
        );
    }

    #[test]
    fn destination_departure_json_renders_a_missing_field_as_null_rather_than_omitting_it() {
        // Same defensive posture as schedule_departure_json: the stored
        // blob is opaque JSONB written by another service, so a missing key
        // must produce an explicit null rather than a differently-shaped
        // object the frontend's own row type would silently mis-parse.
        let json = destination_departure_json(&serde_json::json!({}), "MAN");
        assert!(json["uid"].is_null());
        assert!(json["originCrs"].is_null());
        assert!(json["scheduled"].is_null());
        assert_eq!(json["destinationCrs"], "MAN");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p api destination_departure_json`
Expected: FAIL with a compile error — `destination_departure_json` is not
defined.

- [ ] **Step 3: Implement the render function**

In `crates/api/src/render.rs`, immediately after `schedule_departure_json`
(which ends at line 181):

```rust
/// One `GET /public/trains/search` result row. Sibling of
/// `schedule_departure_json` directly above, same "hand-built camelCase
/// over an opaque JSONB element" convention, with two differences:
///
/// * it carries `originCrs` (the calling point this train departs FROM),
///   which the origin-keyed sibling doesn't need because there the origin
///   is the URL path segment; and
/// * `destinationCrs` is supplied by the CALLER, not read out of `d` --
///   it is the storage bucket's key, identical for every element, and is
///   deliberately not duplicated into each stored element (see
///   `20260907130000_schedule_destination_departures.sql`).
///
/// `scheduled` is trimmed from the stored `"HH:MM:SS"` to `"HH:MM"`,
/// identical to `schedule_departure_json`, so both sources hand the
/// frontend the same time shape.
pub(crate) fn destination_departure_json(d: &Value, destination_crs: &str) -> Value {
    let scheduled = d
        .get("scheduled")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "originCrs": d.get("origin_crs").cloned().unwrap_or(Value::Null),
        "destinationCrs": destination_crs,
    })
}
```

- [ ] **Step 4: Write the route module**

Create `crates/api/src/routes/trains.rs`:

```rust
//! `GET /public/trains/search` -- destination-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
//! Approach B).
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly, filtering inside the
//! published JSONB bucket (`queries::search_schedule_destination_departures`).
//! This is a publish-then-poll read of a table `schedule-reference` writes
//! on its own cycle -- never a synchronous call into that service, per the
//! design doc's §6.
//!
//! **v1 filter set, and why it stops here.** `destination` is required;
//! `origin` and the `from`/`to` time range are optional. There is
//! deliberately NO operator filter: the CIF SCHEDULE feed's operator field
//! is parsed-but-undecoded everywhere in this codebase, so a CIF-derived row
//! has no operator to filter on at all (design doc §1.3/§6). There is
//! deliberately NO date parameter: like `get_station_schedule_departures`,
//! this is "always today, server-side" (design doc §6).

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::queries;
use crate::render::destination_departure_json;

/// Hard ceiling on one response's row count, independent of the
/// publish-side per-bucket cap (`MAX_DEPARTURES_PER_DESTINATION` in
/// `crates/schedule-reference`). Two separate concerns: that one bounds
/// what is STORED per cycle, this one bounds what a single unauthenticated
/// request can pull. There is no pagination in v1 (design doc §3's
/// Approach B scope), so this is a flat cap, not a page size.
const MAX_SEARCH_RESULTS: i64 = 100;

#[derive(Debug, Deserialize)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on it.
    destination: String,
    /// Optional. Matches the calling point a train departs FROM, which for
    /// a mid-route result is an intermediate station, not the schedule's
    /// own first station -- see `schedule_query::DestinationDeparture`'s
    /// own doc comment.
    origin: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Normalizes a caller-supplied `"HH:MM"` into the `"HH:MM:SS"` form the
/// stored `scheduled` values use, so the query's text comparison lines up.
/// `Err` (a 400) rather than silently ignoring an unparseable value: a
/// dropped filter would return MORE trains than asked for, which reads as a
/// broken search rather than a rejected input.
fn normalize_time(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M")
        .map(|t| t.format("%H:%M:%S").to_string())
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("{label} must be a time of day in HH:MM form"),
            )
        })
}

/// Validates and uppercases a CRS code. Rejecting rather than passing a
/// malformed value through matters here because a non-CRS `destination`
/// would otherwise 404 with "nothing published for this destination", which
/// misreports a caller error as a data gap.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter CRS code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let destination = normalize_crs("destination", &params.destination)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    let from_time = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("to", s))
        .transpose()?;

    // "Always today, server-side" -- no date parameter exists on this route
    // by design (design doc §6). Same posture and same expression as
    // `routes::departures::get_station_schedule_departures`.
    let today = chrono::Utc::now().date_naive();

    let Some(rows) = queries::search_schedule_destination_departures(
        &app.database,
        &destination,
        today,
        origin.as_deref(),
        from_time.as_deref(),
        to_time.as_deref(),
        MAX_SEARCH_RESULTS,
    )
    .await
    .map_err(internal_error)?
    else {
        // 404 vs `200 []` is a real distinction here, not pedantry: the
        // former means this destination has no published timetable data at
        // all today (it may not be in `stanox_crs`, or the cycle may not
        // have run yet), the latter means it does and the caller's filters
        // simply excluded everything. The frontend renders different copy
        // for each.
        return Err((
            StatusCode::NOT_FOUND,
            format!("no CIF-derived schedule data for destination: {destination}"),
        ));
    };

    Ok(Json(
        rows.iter()
            .map(|row| destination_departure_json(row, &destination))
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}
```

- [ ] **Step 5: Wire the module into `routes/mod.rs`**

Add the module declaration alphabetically among the existing ones (after
`pub mod train;`, line 24):

```rust
pub mod train;
pub mod trains;
```

And add it to `public_router()`'s merge chain:

```rust
        .merge(departures::router())
        .merge(stanox_crs::router())
        .merge(trains::router())
}
```

- [ ] **Step 6: Write the route-level failing tests**

Add a `db_tests` module at the end of `crates/api/src/routes/trains.rs`. The
`test_app` helper is copied verbatim from
`crates/api/src/routes/departures.rs:162-219` (that module's own doc comment
establishes the colocated-per-file convention; copy it exactly, including
every inert placeholder field, rather than importing it):

```rust
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Copied from `routes::departures::db_tests::test_app`, per that
    /// module's own doc comment ("colocated per-file rather than shared,
    /// until a third file needs it too"). Every field is an inert
    /// placeholder except `database`, which the caller supplies -- this
    /// route touches nothing else on `App`.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixture(pool: &PgPool, destination_crs: &str) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE destination_crs = $1")
            .bind(destination_crs)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    async fn seed_today(pool: &PgPool, destination_crs: &str) {
        delete_fixture(pool, destination_crs).await;
        let today = chrono::Utc::now().date_naive();
        let departures = serde_json::json!([
            {"uid": "C10001", "origin_crs": "EUS", "scheduled": "08:22:00"},
            {"uid": "C10002", "origin_crs": "CRE", "scheduled": "10:05:00"},
        ]);
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (destination_crs, service_date, departures) VALUES ($1, $2, $3)",
        )
        .bind(destination_crs)
        .bind(today)
        .bind(departures)
        .execute(pool)
        .await
        .expect("seed fixture row");
    }

    async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_missing_destination_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "destination is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_destination_is_a_400_not_a_404() {
        // The discriminating case: a caller error must not be reported as
        // "no data for that destination".
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("destination"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_no_published_row_for_today_is_404_naming_the_destination() {
        let pool = connect().await;
        delete_fixture(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("ZRB"), "404 body should name the CRS: {body}");
        delete_fixture(&pool, "ZRB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_published_row_with_no_matches_is_200_empty_array() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&origin=ZZZ").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "published-but-unmatched is a 200 [], never a 404"
        );
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json, serde_json::json!([]));
        delete_fixture(&pool, "ZRB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_the_destination_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=zrb").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();

        assert_eq!(json.as_array().unwrap().len(), 2);
        assert_eq!(json[0]["uid"], "C10001");
        assert_eq!(json[0]["scheduled"], "08:22", "seconds trimmed");
        assert_eq!(json[0]["originCrs"], "EUS");
        assert_eq!(
            json[0]["destinationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert!(json[0].get("origin_crs").is_none(), "no stray snake_case field");

        delete_fixture(&pool, "ZRB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_origin_and_time_filters_together() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) =
            get(&pool, "/trains/search?destination=ZRB&origin=CRE&from=09:00&to=11:00").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["uid"], "C10002");
        delete_fixture(&pool, "ZRB").await;
    }
}
```

> **Step 6 note on `test_app`:** it is copied, not shared, on purpose —
> `routes::departures::db_tests` is a private `#[cfg(test)]` module, so it
> cannot be imported, and that module's own doc comment already establishes
> the convention ("colocated per-file rather than shared, until a third file
> needs it too"). The version written above is verbatim from
> `crates/api/src/routes/departures.rs:162-219`. If `ServiceArguments` has
> gained a field since, the compiler will say so; add it with the same inert
> placeholder value `departures.rs`'s copy uses, and do not delete a field to
> make it build.

- [ ] **Step 7: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api trains_search destination_departure_json -- --ignored --test-threads=1`
Expected: PASS — seven route tests plus the two render tests. (The two render
tests are not `#[ignore]`d; run them separately with
`cargo test -p api destination_departure_json` if the combined invocation's
`--ignored` flag skips them.)

- [ ] **Step 8: Manually verify against a running local stack**

```bash
docker compose up -d --build api
curl -s "http://localhost:8080/public/trains/search?destination=MAN" | jq .
curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8080/public/trains/search?destination=ZZZ"
curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8080/public/trains/search"
```

Expected: the first returns a JSON array (or a 404 body if
`schedule-reference` has not published a cycle yet — either confirms the
route is reachable); the second `404`; the third `400`.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/trains.rs crates/api/src/routes/mod.rs crates/api/src/render.rs
git commit -m "Add GET /public/trains/search, the destination-first train search route"
```

---

## Task 8: Make `create_subscription_for_train` idempotent per `(user_id, trains_id)`

> **This task must land before Tasks 9-12.** Every CTA in this plan calls
> `POST /Train/by-uid/{uid}/{date}/track`, which calls this function. Today,
> a user who clicks "Track this train" twice — a repeat click, a back-navigate
> and re-click, a second tab — silently gets two independent
> `train_subscriptions` rows, two entries in `/track/mine`, and two
> notification streams for one train. See this plan's "Decisions this plan
> resolves", item 2, for the file:line evidence that this is the current,
> documented, test-asserted behaviour.

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs:162-187`
  (`create_subscription_for_train`'s doc comment and body)
- Test: `crates/api/src/data/train_tracking.rs:3497-3556` (invert the
  existing `create_subscription_for_train_called_twice_creates_two_separate_subscriptions`
  test; add two more to the same `db_tests` module)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub async fn create_subscription_for_train(pool: &PgPool,
  trains_id: i64, user_id: &str) -> anyhow::Result<i64>` — **unchanged
  signature**, changed semantics: it now returns the caller's *existing*
  subscription id for that `trains_id` when one exists, instead of creating a
  second row. No caller's code changes; `routes::train::post_track_by_uid`
  (`crates/api/src/routes/train.rs:691-707`) keeps working verbatim and now
  returns the same `trackingId` on a repeat call.

**Design note this task must not silently drop:** the fix is
**application-level, in this one function**, and deliberately **not** a
`UNIQUE (user_id, trains_id)` index. Four unrelated code paths already do a
bare `UPDATE train_subscriptions SET trains_id = $2 WHERE id = $1`
(`crates/api/src/data/schedule_matching.rs:131`,
`crates/api/src/data/trust_event_backlog_match.rs:394` and `:504`,
`crates/api/src/data/train_tracking.rs:684`), and `create_pin`
(`train_tracking.rs:79-100`) has never deduplicated legacy CRS+time pins —
so a user with two legacy pins that later resolve to the same physical train
is reachable today, and a global unique index would turn each of those four
`UPDATE`s into a hard runtime failure inside schedule matching, backlog
matching, and live TRUST resolution. That is a much larger, separately
reviewable change than this plan's scope. Do not add the index.

- [ ] **Step 1: Invert the existing test and add two more**

In `crates/api/src/data/train_tracking.rs`'s `db_tests` module, replace the
whole of
`create_subscription_for_train_called_twice_creates_two_separate_subscriptions`
(lines 3497-3556, doc comment included) with:

```rust
    /// Explicit idempotency check. This test previously asserted the
    /// OPPOSITE -- that two calls produced two rows -- which was an honest
    /// record of the behaviour at the time, not a requirement. The
    /// train-listing-page plan turned that behaviour into a real user-facing
    /// bug (a "Track this train" button a user can click twice), so the
    /// function was fixed and this test inverted alongside it. See
    /// docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md
    /// Task 8, which also records why a `UNIQUE (user_id, trains_id)` index
    /// was rejected in favour of this in-function fix.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_called_twice_returns_the_same_subscription \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_called_twice_returns_the_same_subscription() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK-TWICE";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-TWICE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let first_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first create_subscription_for_train call");
        let second_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second create_subscription_for_train call, same trains_id and user_id");

        assert_eq!(
            first_tracking_id, second_tracking_id,
            "a repeat call for the same (trains_id, user_id) must return the EXISTING \
             subscription, not create a second one"
        );

        let (row_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM train_subscriptions WHERE trains_id = $1 AND user_id = $2",
        )
        .bind(trains_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("count subscriptions for this (trains_id, user_id) pair");
        assert_eq!(row_count, 1, "exactly one row must exist after two calls");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(first_tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Discriminating counterpart: idempotency is scoped to ONE user. Two
    /// different users tracking the same physical train is this endpoint's
    /// own headline scenario (the whole point of the shared `trains` table)
    /// and must still produce two independent subscriptions.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_is_not_shared_between_users \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_is_not_shared_between_users() {
        let pool = connect().await;
        let user_a = "TEST-NR-PRIMARY-SHARED-A";
        let user_b = "TEST-NR-PRIMARY-SHARED-B";
        seed_user(&pool, user_a).await;
        seed_user(&pool, user_b).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-SHARED-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let a = create_subscription_for_train(&pool, trains_id, user_a)
            .await
            .expect("user A subscribes");
        let b = create_subscription_for_train(&pool, trains_id, user_b)
            .await
            .expect("user B subscribes to the same train");

        assert_ne!(
            a, b,
            "two users tracking one train must still get two independent subscriptions"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id IN ($1, $2)")
            .bind(a)
            .bind(b)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_a).await;
        cleanup_user(&pool, user_b).await;
    }

    /// Guards the one thing the existing-row-wins CTE could plausibly get
    /// wrong: the returned id must be the row that actually exists, usable
    /// as a real tracking id, not a stale/duplicated value. Reads the row
    /// back through the same ownership query every `/Train/{trackingId}`
    /// route uses.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_repeat_call_returns_a_usable_tracking_id \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_repeat_call_returns_a_usable_tracking_id() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-USABLE-ID";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-USABLE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first call");
        let returned = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second call");

        let owner = tracked_train_owner(&pool, returned)
            .await
            .expect("ownership lookup");
        assert_eq!(
            owner,
            Some(user_id.to_string()),
            "the returned id must resolve to a real, caller-owned subscription"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(returned)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:
`DATABASE_URL=... cargo test -p api create_subscription_for_train -- --ignored --test-threads=1`
Expected: FAIL —
`create_subscription_for_train_called_twice_returns_the_same_subscription`
fails on `assert_eq!(first_tracking_id, second_tracking_id)` (they differ
today) and on `assert_eq!(row_count, 1)` (it is 2 today).
`create_subscription_for_train_is_not_shared_between_users` passes already —
that is expected and correct; it is a regression guard for the fix, not a
new requirement.

- [ ] **Step 3: Replace the function body and its doc comment**

In `crates/api/src/data/train_tracking.rs`, replace lines 162-187 (the
"NOT idempotent by `(trains_id, user_id)`" paragraph through the end of the
function) with:

```rust
/// **Idempotent by `(user_id, trains_id)`.** A second call for the same
/// user and the same shared train returns that user's EXISTING subscription
/// id rather than inserting another row -- so a user who clicks "Track this
/// train" twice (a repeat click, a back-navigate, a second tab) ends up
/// with one subscription, one `/track/mine` entry and one notification
/// stream, and is navigated to the same `/train/by-id/{trackingId}` both
/// times.
///
/// Scoped to ONE user: two different users tracking the same physical train
/// still get two independent subscriptions, which is this endpoint's own
/// headline scenario and the entire reason the shared `trains` table
/// exists.
///
/// Deliberately NOT backed by a `UNIQUE (user_id, trains_id)` index, and
/// this is a considered rejection rather than an oversight. Four unrelated
/// paths already do a bare `UPDATE train_subscriptions SET trains_id = $2
/// WHERE id = $1` (`data/schedule_matching.rs`'s `attempt_schedule_match`,
/// `data/trust_event_backlog_match.rs` in two places, and this file's own
/// live-resolution write), and `create_pin` above has never deduplicated
/// legacy CRS+time pins -- so a user holding two legacy pins that later
/// resolve to the same physical train is reachable today, and a global
/// unique index would turn each of those `UPDATE`s into a hard failure
/// inside schedule matching, backlog matching and live TRUST resolution.
///
/// The honest residual limitation, stated rather than papered over: under
/// READ COMMITTED, two genuinely simultaneous in-flight calls can both
/// observe no existing row and both insert. This closes the ordinary
/// repeat-click case, not a true concurrent double-submit; the frontend
/// closes that one the way every other mutating control in this app does,
/// by disabling the button while its request is in flight
/// (`frontend/components/TrackThisTrainButton.tsx`).
///
/// See docs/superpowers/specs/2026-09-07-train-listing-page-design.md §7
/// Open Question 2, which flagged this function's behaviour as unverified,
/// and Task 8 of that spec's implementation plan, which verified it (it was
/// not idempotent) and fixed it.
pub async fn create_subscription_for_train(
    pool: &PgPool,
    trains_id: i64,
    user_id: &str,
) -> anyhow::Result<i64> {
    // One statement, not a SELECT-then-INSERT round trip: the `inserted`
    // CTE's `NOT EXISTS (SELECT 1 FROM existing)` guard means the INSERT
    // never fires when a subscription is already there, and the final
    // UNION ALL yields exactly one row either way -- so `fetch_one` still
    // errors (RowNotFound) for a `trains_id` that names no `trains` row,
    // exactly as the previous plain `INSERT ... SELECT` did.
    let row: (i64,) = sqlx::query_as(
        "WITH existing AS ( \
             SELECT id FROM train_subscriptions \
             WHERE user_id = $1 AND trains_id = $2 \
             ORDER BY id LIMIT 1 \
         ), \
         inserted AS ( \
             INSERT INTO train_subscriptions \
                 (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs) \
             SELECT $1, tr.id, tr.service_date, tr.origin_crs, tr.scheduled_departure, tr.destination_crs \
             FROM trains tr \
             WHERE tr.id = $2 AND NOT EXISTS (SELECT 1 FROM existing) \
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
```

`ORDER BY id LIMIT 1` rather than a bare `SELECT id`: if pre-existing
duplicate rows are already in the database (they are reachable today, and
this task deliberately does not delete them), the oldest one wins,
deterministically, on every call.

- [ ] **Step 4: Run the tests to verify they pass**

Run:
`DATABASE_URL=... cargo test -p api create_subscription_for_train -- --ignored --test-threads=1`
Expected: PASS — all three tests, including the two pre-existing ones in
this module (`create_subscription_for_train_inherits_known_schedule_data`,
`create_subscription_for_train_allows_null_pins_for_a_bare_uid_with_no_schedule_data`,
`train_tracking.rs:3385` and `:3448`), which exercise a *first* call and
must be unaffected.

- [ ] **Step 5: Run the route-level tests that depend on this function**

Run:
`DATABASE_URL=... cargo test -p api post_track_by_uid nr_primary -- --ignored --test-threads=1`
Expected: PASS. If any test in `crates/api/src/routes/train.rs` asserts that
two `POST /Train/by-uid/…/track` calls produce two distinct `trackingId`s,
that assertion is now wrong for the same reason the `db_tests` one was —
invert it (same `assert_eq!`, same reasoning) rather than reverting this
task.

- [ ] **Step 6: Run the full crate suite**

Run: `cargo test -p api`
Expected: PASS, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Make create_subscription_for_train idempotent per user and train"
```

---

## Task 9: `TrackThisTrainButton` — the shared "Track this train" CTA

**Files:**
- Create: `frontend/components/TrackThisTrainButton.tsx`
- Test: `frontend/components/TrackThisTrainButton.test.tsx`

**Interfaces:**
- Consumes: `POST /Train/by-uid/{uid}/{date}/track` (existing,
  `crates/api/src/routes/train.rs:691-707`, made idempotent by Task 8) and
  `POST /Train/tickets/{ticketId}/attach` (existing,
  `crates/api/src/routes/train.rs:247-276`), both through the same-origin
  `/api/*` proxy (`frontend/app/api/[...path]/route.ts`, whose
  `resolveTargetPath` already passes a `Train/...` path straight through
  with no prefix inserted); `useNeedsLogin` (existing,
  `frontend/components/useNeedsLogin.ts`); `LoginPromptModal` (existing).
- Produces: `export function TrackThisTrainButton({ uid, date,
  attachTicketId, size }: { uid: string; date: string; attachTicketId?:
  number; size?: 'xs' | 'sm' | 'md' }): JSX.Element` — consumed by Task 10
  (`/trains` rows, **with** `attachTicketId`) and Task 12
  (`/train/[uid]/[date]`, **without** it).

- [ ] **Step 1: Write the failing tests**

```typescript
// frontend/components/TrackThisTrainButton.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackThisTrainButton } from './TrackThisTrainButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Routes a mocked `fetch` by URL, the same shape
 * `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * by-uid track call and the ticket-attach follow-up are configured
 * independently so a test can make one fail without the other. */
function mockFetchByUrl(
  options: { track?: () => Response; attach?: () => Response | Promise<Response> } = {},
) {
  const {
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    attach = () => new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url)) return Promise.resolve(attach());
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

describe('TrackThisTrainButton', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('POSTs to the by-uid track route with the uid and date from its props', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('percent-encodes a path-like uid', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052/../mine" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052%2F..%2Fmine/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('navigates to the new tracking id on success', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('makes no ticket-attach call when attachTicketId is absent', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('attaches the ticket after a successful track when attachTicketId is given', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ trackingId: 42 }),
        }),
      );
    });
  });

  // The parity requirement's real substance: a failed attach must not block
  // navigation, exactly as TrackTrainForm.tsx:321-335 already behaves.
  it('still navigates when the ticket-attach follow-up rejects', async () => {
    const fetchMock = mockFetchByUrl({ attach: () => Promise.reject(new Error('network blip')) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('still navigates when the ticket-attach follow-up returns a 409', async () => {
    const fetchMock = mockFetchByUrl({
      attach: () => new Response('ticket is already attached to a tracked train', { status: 409 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('opens the login prompt and does not navigate on a 401', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('unauthorized', { status: 401 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText("Couldn't track this train. Try again.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  // The concurrent-double-submit guard Task 8's in-function fix cannot
  // close on its own -- asserted, not assumed.
  it('disables itself while a request is in flight', async () => {
    let resolveTrack: (value: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      resolveTrack = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn((input: RequestInfo | URL) => {
        if (/\/track$/.test(String(input))) return pending;
        throw new Error(`unexpected fetch for ${String(input)}`);
      }),
    );
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    const button = screen.getByRole('button', { name: 'Track this train' });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Tracking…' })).toBeDisabled());

    resolveTrack(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
    await waitFor(() => expect(pushMock).toHaveBeenCalled());
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- TrackThisTrainButton.test.tsx`
Expected: FAIL — `./TrackThisTrainButton` does not exist.

- [ ] **Step 3: Write the component**

```tsx
// frontend/components/TrackThisTrainButton.tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** The "Track this train" action for a train whose real CIF identity is
 * already known -- a `(train_uid, service_date)` pair. Calls
 * `POST /Train/by-uid/{uid}/{date}/track`
 * (`crates/api/src/routes/train.rs`'s `post_track_by_uid`, the NR-primary
 * tracking entry point), which takes NO request body at all: identity is
 * entirely in the path, so there is no form to fill in and nothing to
 * validate client-side. That is the whole difference from
 * `TrackTrainForm`'s legacy `POST /Train/track` flow, which has to guess an
 * identity from a CRS + time pin.
 *
 * Two call sites, deliberately different:
 * * `/trains` result rows pass `attachTicketId`, giving the listing page's
 *   action full parity with `/track?ticketId=...`'s existing flow
 *   (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 * * `/train/[uid]/[date]`'s page-level CTA does NOT -- that page has no
 *   `ticketId` query-param convention and inventing one is explicitly out
 *   of scope (§5 of the same doc).
 *
 * The ticket-attach follow-up mirrors `TrackTrainForm.tsx`'s own
 * (`handleSubmit`, the `attachTicketId !== undefined` block) exactly,
 * including swallowing every failure: tracking the train has ALREADY
 * succeeded by then, so a network blip or a `409 Conflict` (the ticket
 * having since been attached elsewhere) must not block navigation. The
 * ticket simply stays standalone and reattachable from the merged
 * trains/tickets list.
 *
 * Calls the same-origin `/api/*` proxy rather than `lib/api.ts` -- this is
 * a Client Component and cannot read the server-only `API_BASE_URL` env var
 * (same reasoning as `PinToggle` and `TrackTrainForm`).
 *
 * 401 handling is the shared `useNeedsLogin`/`LoginPromptModal` pattern
 * (`useNeedsLogin.ts`'s own doc comment names it). Unlike `TrackTrainForm`,
 * there is no typed input to protect on a 401 -- this control is a single
 * click, so it behaves like `PinToggle`: show the prompt, change nothing
 * else.
 *
 * `disabled={busy}` is load-bearing, not cosmetic:
 * `train_tracking::create_subscription_for_train` is idempotent per
 * `(user_id, trains_id)` as of this feature, but that in-function fix
 * cannot close a genuinely concurrent double-submit under READ COMMITTED.
 * Disabling the control while its request is in flight is what does. */
export function TrackThisTrainButton({
  uid,
  date,
  attachTicketId,
  size = 'sm',
}: {
  uid: string;
  date: string;
  attachTicketId?: number;
  size?: 'xs' | 'sm' | 'md';
}) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function track() {
    setBusy(true);
    needsLoginState.reset();
    setError(null);
    try {
      const response = await fetch(
        `/api/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}/track`,
        { method: 'POST' },
      );

      if (response.ok) {
        const result: { trackingId: number } = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort, exactly as TrackTrainForm does it -- see this
          // component's own doc comment.
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
        router.push(`/train/by-id/${result.trackingId}`);
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

  return (
    <Stack gap="xs">
      <Button size={size} onClick={track} disabled={busy}>
        {busy ? 'Tracking…' : 'Track this train'}
      </Button>
      {error && (
        <Alert color="red" title="Couldn't track this train">
          {error}
        </Alert>
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- TrackThisTrainButton.test.tsx`
Expected: PASS, all 11 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrackThisTrainButton.tsx frontend/components/TrackThisTrainButton.test.tsx
git commit -m "Add TrackThisTrainButton, the shared NR-primary track CTA"
```

---

## Task 10: `TrainSearchForm` — the filter form and result list

**Files:**
- Create: `frontend/components/TrainSearchForm.tsx`
- Test: `frontend/components/TrainSearchForm.test.tsx`

**Interfaces:**
- Consumes: `GET /public/trains/search` (Task 7) via the same-origin proxy
  path `/api/trains/search?…` (`resolveTargetPath` prefixes anything that
  isn't `Train/...` with `/public/`, and `proxy` forwards
  `req.nextUrl.search` verbatim, so the query string reaches the backend
  unchanged); `TrackThisTrainButton` (Task 9); `searchStations`
  (`frontend/lib/suggestions.ts:9-14`) and `useSuggestions`
  (`frontend/lib/useSuggestions.ts`), the same pair every station field in
  this app already uses; `TextLink`.
- Produces: `export function TrainSearchForm({ initialDestination,
  initialOrigin, attachTicketId }: { initialDestination?: string;
  initialOrigin?: string; attachTicketId?: number }): JSX.Element` —
  consumed by Task 11's `/trains` page.

- [ ] **Step 1: Write the failing tests**

```typescript
// frontend/components/TrainSearchForm.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Routes a mocked `fetch` by URL: the search call, the station-suggestion
 * calls both Autocompletes fire, and the track/attach calls
 * `TrackThisTrainButton` makes. `search` defaults to two rows so most tests
 * only override the branch they care about. */
function mockFetchByUrl(
  options: { search?: () => Response; track?: () => Response } = {},
) {
  const {
    search = () =>
      new Response(
        JSON.stringify([
          { uid: 'C10001', scheduled: '08:22', originCrs: 'EUS', destinationCrs: 'MAN' },
          { uid: 'C10002', scheduled: '10:05', originCrs: 'CRE', destinationCrs: 'MAN' },
        ]),
        { status: 200 },
      ),
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/search')) return Promise.resolve(search());
    if (url.startsWith('/api/stations?')) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url))
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }));
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

function searchCallUrl(fetchMock: ReturnType<typeof vi.fn>): string {
  const call = fetchMock.mock.calls.find((args: unknown[]) =>
    String(args[0]).startsWith('/api/trains/search'),
  );
  if (!call) throw new Error('no /api/trains/search call recorded');
  return String(call[0]);
}

describe('TrainSearchForm', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('does not search until a valid destination CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(
      screen.getByText('Enter a destination station above to search for trains.'),
    ).toBeInTheDocument();
  });

  it('sends only the destination when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?destination=MAN'));
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?destination=MAN&origin=EUS&from=09%3A00&to=12%3A00',
      ),
    );
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN')).toBeInTheDocument();
  });

  it('links each row to the public train page for today', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const links = await screen.findAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a Track this train action on every row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    fireEvent.click(buttons[0]);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      ),
    );
  });

  // The 404-vs-200-[] split the backend route draws deliberately (Task 7)
  // has to survive into the UI, or it was pointless.
  it('distinguishes "nothing published for this destination" from "no matches"', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="ZZZ" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(
        /No scheduled timetable data is available for that destination today/,
      ),
    ).toBeInTheDocument();
  });

  it('says so when the search succeeds but matches nothing', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('[]', { status: 200 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText('No scheduled trains match those filters right now.'),
    ).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("Couldn't search for trains right now. Try again."),
    ).toBeInTheDocument();
  });

  // The honesty requirement carried over from TrackTrainForm's own CIF
  // branch: these rows are timetable data, not live running information,
  // and the UI must never imply otherwise.
  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(/scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers the manual /track fallback, carrying any ticketId through', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm attachTicketId={7} />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  it('offers the manual /track fallback with no query string when there is no ticketId', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
  });

  it('renders no operator filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/Operator/i)).not.toBeInTheDocument();
  });

  it('renders no date filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/^Date/i)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- TrainSearchForm.test.tsx`
Expected: FAIL — `./TrainSearchForm` does not exist.

- [ ] **Step 3: Write the component**

```tsx
// frontend/components/TrainSearchForm.tsx
'use client';

import { useState, type FormEvent } from 'react';
import { Alert, Autocomplete, Button, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::destination_departure_json`). Deliberately
 * NOT `ScheduleDepartureRow` from `TrackTrainForm.tsx`, even though the two
 * overlap: this one carries `originCrs` (the station a train departs FROM,
 * which for a mid-route result is an intermediate stop, not the schedule's
 * first station) and a non-nullable `destinationCrs` (it is the search key,
 * so a row can only exist if it resolved). Like every CIF-derived row in
 * this app it carries NO operator and NO live running status -- the CIF
 * SCHEDULE feed has neither (see
 * docs/superpowers/specs/2026-09-07-train-listing-page-design.md §1.3), and
 * this list never fabricates them. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  originCrs: string;
  destinationCrs: string;
}

/** Exactly one of five mutually-exclusive states, checked top to bottom by
 * `resultsContent` below. `'unpublished'` and an empty `rows` array are
 * genuinely different facts and get different copy -- the backend route
 * draws that 404-vs-`200 []` distinction on purpose (Task 7) and collapsing
 * it here would waste it. */
type Results = { rows: TrainSearchRow[] } | 'unpublished' | 'error' | null;

/** Destination-first, whole-network train search -- the `/trains` page's
 * one interactive component
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
 * Approach B).
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by destination and
 * cannot see a train that isn't in it (a capped bucket, a station missing
 * from `stanox_crs`, a same-day amendment). `/track`'s manual-entry form
 * remains the honest fallback for exactly those gaps, and this component
 * links to it explicitly rather than pretending they don't exist -- §4 of
 * the design doc is a direct "no" on full replacement.
 *
 * Filter set, and why it stops here: Destination is required (it is the
 * server-side bucket key); Origin and a From/To time range are optional.
 * There is no Operator filter -- CIF rows carry no operator field at all,
 * so the filter could only ever match nothing. There is no Date filter --
 * both of this app's schedule sources are "today only, server-side". Both
 * are explicit §6 non-goals, not omissions to fill in later.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  initialDestination = '',
  initialOrigin = '',
  attachTicketId,
}: {
  initialDestination?: string;
  initialOrigin?: string;
  attachTicketId?: number;
}) {
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [fromTime, setFromTime] = useState('');
  const [toTime, setToTime] = useState('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);

  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);
  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);

  const destinationValid = CRS_PATTERN.test(destinationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const canSearch = destinationValid && originValid && fromValid && toValid && !searching;

  // Computed once per render rather than once per row: every result links
  // to the same calendar date, because this search is always "today"
  // (server-side) -- same construction and same browser-local-date
  // assumption `TrackTrainForm`'s own CIF branch already makes.
  const today = dayjs().format('YYYY-MM-DD');

  // The manual fallback keeps a standalone ticket's id attached to the
  // journey, so a user who bounces from /trains to /track doesn't silently
  // lose the ticket they were trying to link -- same `?ticketId=` param
  // `app/track/page.tsx` already reads.
  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSearch) return;
    setSearching(true);
    try {
      const params = new URLSearchParams({ destination: destinationCrs.trim().toUpperCase() });
      if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
      if (fromTime.trim()) params.set('from', fromTime.trim());
      if (toTime.trim()) params.set('to', toTime.trim());

      const response = await fetch(`/api/trains/search?${params.toString()}`);
      if (response.status === 404) {
        // A real, distinct fact, not an error: nothing is published for
        // this destination today. Never collapsed into "no matches".
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const rows: TrainSearchRow[] = await response.json();
      setResults({ rows });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  function resultsContent() {
    if (!destinationValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter a destination station above to search for trains.
        </Text>
      );
    }
    if (searching) {
      return (
        <Text size="sm" c="dimmed">
          Searching…
        </Text>
      );
    }
    if (results === null) {
      return (
        <Text size="sm" c="dimmed">
          Press Search to find trains to this destination.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search for trains right now. Try again.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          No scheduled timetable data is available for that destination today — it may not be a
          station this feed covers, or today&apos;s timetable may not have been published yet.
        </Text>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled trains match those filters right now.
        </Text>
      );
    }
    return (
      <>
        <Text size="sm" c="dimmed">
          These are from the scheduled timetable, not live running information, and may be up to 30
          minutes out of date. Open a train to see its live status.
        </Text>
        <ScrollArea mah={420} offsetScrollbars>
          <Stack gap="xs">
            {results.rows.map((row) => (
              <Group key={`${row.uid}-${row.originCrs}-${row.scheduled}`} justify="space-between" wrap="nowrap">
                <Text size="sm">
                  {row.scheduled} · {row.originCrs} → {row.destinationCrs}
                </Text>
                <Group gap="sm" wrap="nowrap">
                  {/* Always safe to render, unlike TrackTrainForm's LDBWS
                      branch: every row here carries a real CIF schedule UID
                      (it is the grouping pass's own key), which is exactly
                      what /train/[uid]/[date] is keyed on. */}
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${today}`}>
                    View live status
                  </TextLink>
                  <TrackThisTrainButton
                    uid={row.uid}
                    date={today}
                    attachTicketId={attachTicketId}
                    size="xs"
                  />
                </Group>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Destination station"
        placeholder="e.g. Manchester or MAN"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Euston or EUS"
        description="Any station on the train's route, not just where it started."
        value={originCrs}
        onChange={setOriginCrs}
        data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Group grow align="flex-start">
        {/* Plain TextInputs, not @mantine/dates pickers: these are a
            time-of-day range on a fixed (today) date, which DateTimePicker
            cannot express without also asking for a date this feature
            deliberately does not accept. */}
        <TextInput
          label="From (optional)"
          placeholder="09:00"
          value={fromTime}
          onChange={(event) => setFromTime(event.currentTarget.value)}
          error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
        />
        <TextInput
          label="To (optional)"
          placeholder="12:00"
          value={toTime}
          onChange={(event) => setToTime(event.currentTarget.value)}
          error={toTime.length > 0 && !toValid ? 'Must be a time like 12:00' : null}
        />
      </Group>
      <Group>
        <Button type="submit" disabled={!canSearch}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
      <Text size="sm" c="dimmed">
        Can&apos;t find your train? <TextLink href={manualHref}>Track it manually</TextLink> by
        entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- TrainSearchForm.test.tsx`
Expected: PASS, all 15 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrainSearchForm.tsx frontend/components/TrainSearchForm.test.tsx
git commit -m "Add TrainSearchForm, the destination-first train search UI"
```

---

## Task 11: The `/trains` page and its nav entry

**Files:**
- Create: `frontend/app/trains/page.tsx`
- Test: `frontend/app/trains/page.test.tsx`
- Modify: `frontend/app/layout.tsx:225-227` (add the nav link)
- Test: `frontend/app/layout.test.tsx` (assert the new nav link)

**Interfaces:**
- Consumes: `TrainSearchForm` (Task 10).
- Produces: the route `/trains`, accepting `?destination=`, `?origin=` and
  `?ticketId=` query parameters. Linked from the global nav and from
  Task 12's page copy.

- [ ] **Step 1: Write the failing page tests**

```typescript
// frontend/app/trains/page.test.tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrainsPage from './page';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

// The page mounts TrainSearchForm, whose suggestion hooks fire real
// fetches on mount for any pre-filled, valid CRS -- give every test an
// inert 200 so none of them depend on network behaviour.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('TrainsPage', () => {
  it('renders the title and the search form', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({}) }));
    expect(screen.getByRole('heading', { name: 'Find a Train' })).toBeInTheDocument();
    expect(screen.getByLabelText('Destination station')).toBeInTheDocument();
  });

  it('pre-fills the destination and origin from the query string, uppercased', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: 'man', origin: 'eus' }) }),
    );
    expect(screen.getByLabelText('Destination station')).toHaveValue('MAN');
    expect(screen.getByLabelText('Departing from (optional)')).toHaveValue('EUS');
  });

  it('uses the first value when a query param is repeated', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: ['MAN', 'EDB'] }) }),
    );
    expect(screen.getByLabelText('Destination station')).toHaveValue('MAN');
  });

  it('shows the ticket-attach explainer copy when arriving with a ticketId', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(
      screen.getByText(
        "Find the train your saved ticket is for — it'll be attached automatically once you track it.",
      ),
    ).toBeInTheDocument();
  });

  it('carries a valid ticketId through to the manual fallback link', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  // Same posture as app/track/page.tsx:16-21: a malformed value is treated
  // as absent rather than passed through as NaN.
  it('treats a non-numeric ticketId as absent', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: 'nope' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
    expect(
      screen.queryByText(/it'll be attached automatically once you track it/),
    ).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- app/trains/page.test.tsx`
Expected: FAIL — `./page` does not exist.

- [ ] **Step 3: Write the page**

```tsx
// frontend/app/trains/page.tsx
import { Stack, Title, Text } from '@mantine/core';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** `/trains` -- the primary train-discovery surface
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close (a
 * station outside the CIF-derived data, a train past the per-destination
 * cap, a same-day amendment), and both `/stations/[crs]`'s "Track a train
 * from here" link and `TicketEntryForm`'s standalone-ticket flow still
 * point at it unchanged. See §4's explicit "do not delete or hide /track".
 *
 * Query params mirror `/track`'s own convention (`app/track/page.tsx`):
 * `?destination=`/`?origin=` pre-fill the filters (so a filtered search is
 * a shareable link -- the design doc's §7 Open Question 5, resolved
 * affirmatively because it costs one prop each), and `?ticketId=` carries a
 * standalone ticket through so the row-level "Track this train" action can
 * attach it, giving this page full parity with `/track?ticketId=...`. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    destination?: string | string[];
    origin?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { destination, origin, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?destination=a&destination=b`) -- fall back to the first value rather
  // than letting `.toUpperCase()` throw on an array. Same handling as
  // `app/track/page.tsx:10-13`.
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Find a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for — it'll be attached automatically once you track it."
          : 'Search the whole network by where a train is going. Open any result for its live status, or track it to get updates.'}
      </Text>
      <TrainSearchForm
        initialDestination={destinationParam?.toUpperCase()}
        initialOrigin={originParam?.toUpperCase()}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
```

- [ ] **Step 4: Run the page tests to verify they pass**

Run (from `frontend/`): `npm test -- app/trains/page.test.tsx`
Expected: PASS, all 6 tests.

- [ ] **Step 5: Write the failing nav test**

The nav's two static links (`All Lines`, `Station Lookup`) live directly
inside `RootLayout`, which is not exported and cannot be rendered in a unit
test — it awaits `getDataFreshness()` and returns a whole `<html>` tree.
`frontend/app/layout.test.tsx` already has an established pattern for
asserting things in that unrenderable part of the file: a source assertion
via the `readFileSync` it already imports on line 1 (see the existing
`expect(source).toMatch(/<Container\s+component="main"/)` case at line
75-77). Follow it exactly, as a new top-level `describe` appended to the
file:

```typescript
describe('the primary nav', () => {
  // Source assertion, not a render: these links live inside RootLayout
  // itself, which is unexported and awaits getDataFreshness() -- the same
  // reason the `<Container component="main">` case above is written this
  // way rather than rendered.
  it('links to the new train-search page', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<TextLink href="\/trains">Find a Train<\/TextLink>/);
  });

  // Regression guard: /trains is an ADDITION, not a replacement. The
  // design doc's §4 is an explicit "no" on removing or hiding /track, and
  // the two station/line entry points either side of the new link must
  // survive it.
  it('still links to the existing lines and stations pages', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<TextLink href="\/lines">All Lines<\/TextLink>/);
    expect(source).toMatch(/<TextLink href="\/stations">Station Lookup<\/TextLink>/);
  });
});
```

- [ ] **Step 6: Run it to verify it fails**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: FAIL on the first new case — `app/layout.tsx`'s source contains no
`<TextLink href="/trains">Find a Train</TextLink>`. The second new case
(`still links to the existing lines and stations pages`) passes already;
that is correct and intended — it is a regression guard for Step 7, not a
new requirement.

- [ ] **Step 7: Add the nav link**

In `frontend/app/layout.tsx`, in the nav `<Group gap="lg">` (lines 225-227),
between the two existing static links and the tracked-trains item:

```tsx
                  <Group gap="lg">
                    <TextLink href="/lines">All Lines</TextLink>
                    <TextLink href="/stations">Station Lookup</TextLink>
                    {/* The primary train-discovery surface. `/track` is
                        still reachable (from here via /trains' own manual
                        fallback link, from /stations/[crs], and from
                        TicketEntryForm) but is no longer the first thing a
                        visitor is pointed at -- see
                        docs/superpowers/specs/2026-09-07-train-listing-page-design.md
                        §4. */}
                    <TextLink href="/trains">Find a Train</TextLink>
                    <TrackedTrainsNavItem />
```

- [ ] **Step 8: Run the tests and the build**

Run (from `frontend/`): `npm test -- layout.test.tsx` — Expected: PASS.
Run (from `frontend/`): `npm run build` — Expected: a clean build with no
TypeScript errors, and `/trains` listed in the route table.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/trains/page.tsx frontend/app/trains/page.test.tsx frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Add the /trains discovery page and its nav entry"
```

---

## Task 12: The `/train/[uid]/[date]` "Track this train" CTA

**Files:**
- Modify: `frontend/app/train/[uid]/[date]/page.tsx:99-111` (the returned
  JSX only — `toJourneyState` and the fetch/404 logic above it are
  untouched)
- Test: `frontend/app/train/[uid]/[date]/page.test.tsx`

**Interfaces:**
- Consumes: `TrackThisTrainButton` (Task 9), rendered **without**
  `attachTicketId`.
- Produces: nothing other tasks depend on. This is the last task.

**The one thing this task must not do:** pass an `attachTicketId`. The
design doc's §5 is explicit — `/train/[uid]/[date]` has no existing
`ticketId` query-param convention, and inventing one here is out of scope
(§6 restates it as a named non-goal). Only `/trains`' own row-level action
gets ticket-attach parity.

- [ ] **Step 1: Write the failing tests**

Append to the existing `describe` block in
`frontend/app/train/[uid]/[date]/page.test.tsx`. The file's existing
`next/navigation` mock already supplies `useRouter`, `usePathname` and
`useSearchParams`, which is everything `TrackThisTrainButton` and
`LoginPromptModal` need — no mock changes are required.

```typescript
  it('renders a Track this train button for every visitor', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  it('tracks by the uid and date from the URL, not from the response body', async () => {
    // Discriminating: the fixture's own trainUid deliberately differs from
    // the URL segment, so a component wired to the wrong source fails here.
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(
      publicTrainState({ trainUid: 'DIFFERENT' }),
    );
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/W12345/2026-08-31/track',
        expect.objectContaining({ method: 'POST' }),
      ),
    );
  });

  // The spec's own §5 exclusion, asserted rather than assumed: this page's
  // CTA must never make a ticket-attach call, because this page has no
  // ticketId convention to source one from.
  it('makes no ticket-attach call after tracking', async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('still renders no owner actions alongside the new CTA', async () => {
    // Regression guard on this page's whole reason for being read-only:
    // Rename/Delete/tickets all key on a train_subscriptions.id this
    // response does not carry (see the page's own doc comment). Adding a
    // track CTA must not have opened that door.
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Delete/i })).not.toBeInTheDocument();
  });

  it('points at the new /trains page for finding other trains', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
  });
```

Add `fireEvent` and `waitFor` to this file's existing
`@testing-library/react` import.

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- app/train`
Expected: FAIL — no "Track this train" button is rendered and no link named
"Find a train" exists.

- [ ] **Step 3: Wire the CTA into the page**

In `frontend/app/train/[uid]/[date]/page.tsx`, add the import alongside the
existing component imports:

```tsx
import { TrackThisTrainButton } from '@/components/TrackThisTrainButton';
import { TextLink } from '@/components/TextLink';
```

Then replace the returned JSX (lines 99-111) with:

```tsx
  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Train {uid}</Title>
        <Group gap="sm">
          {/* Shown to EVERY visitor, logged in or not -- the shared
              "show the control to everyone, prompt on the real 401"
              posture `PinToggle`/`TrackTrainForm` already establish via
              useNeedsLogin/LoginPromptModal. No `attachTicketId`: this page
              has no `ticketId` query-param convention and inventing one is
              explicitly out of scope
              (docs/superpowers/specs/2026-09-07-train-listing-page-design.md
              §5/§6). Only /trains' own row action attaches tickets.

              A logged-in visitor who ALREADY tracks this train gets no
              special treatment, deliberately: `PublicTrainState` carries no
              "you already have a subscription" hint, by design (it is the
              shared, public train, with nothing per-subscriber on it).
              Clicking again is harmless -- `create_subscription_for_train`
              is idempotent per (user, train) and returns the existing
              subscription, so both clicks land on the same
              /train/by-id/{trackingId}. */}
          <TrackThisTrainButton uid={uid} date={date} />
          <ShareButton />
        </Group>
      </Group>
      <TrainJourney state={toJourneyState(train)} />
      <Text size="sm" c="dimmed">
        This is the public view of this service. Track it above to get updates, or{' '}
        <TextLink href="/trains">Find a train</TextLink> going somewhere else.
      </Text>
    </Stack>
  );
```

The closing sentence changed because it had to: it previously told the
visitor to go to `/track` "to get updates", which is now stale advice with a
Track button directly above it. It points at `/trains` (the new primary
discovery surface) for the *other* thing that sentence was doing — finding a
different train — and `/track` remains reachable from there via that page's
own manual-fallback link, unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- app/train`
Expected: PASS — the 5 new tests plus every pre-existing one in the file (in
particular the `notFound()` and journey-rendering tests, which this task does
not touch).

- [ ] **Step 5: Run the full frontend suite and build**

Run (from `frontend/`): `npm test` — Expected: PASS, no regressions in any
suite.
Run (from `frontend/`): `npm run build` — Expected: clean.

- [ ] **Step 6: Manually verify the whole feature end to end**

```bash
docker compose up -d --build
```

Then, in a browser:
1. Open `/trains`, type a destination (e.g. `MAN`), press Search. Expect a
   list of scheduled trains, each with "View live status" and "Track this
   train". If the list says no timetable data is available, check that
   `schedule-reference` has completed a cycle since Task 4 shipped
   (`docker compose logs schedule-reference | grep destination`).
2. Click "View live status" on a row → lands on `/train/{uid}/{today}`,
   which now shows a "Track this train" button.
3. Click it while logged out → the login prompt modal opens, nothing else
   changes.
4. Log in, click it → lands on `/train/by-id/{trackingId}`.
5. Go back to that train's public page and click it again → lands on the
   **same** `/train/by-id/{trackingId}`, and `/track/mine` shows **one**
   entry for it, not two. (This is Task 8's fix, observed end to end.)
6. Open `/trains?ticketId={a real standalone ticket id}` and track a row →
   the ticket appears attached to the new tracked train in `/track/mine`.

- [ ] **Step 7: Commit**

```bash
git add "frontend/app/train/[uid]/[date]/page.tsx" "frontend/app/train/[uid]/[date]/page.test.tsx"
git commit -m "Add the Track this train CTA to the public train page"
```

---

## Self-review notes (performed on this plan before handoff)

**1. Spec coverage**, section by section against
`docs/superpowers/specs/2026-09-07-train-listing-page-design.md`:

- **§2 Goal and scope.** The realistic-filter table's verdicts are all
  honoured: Origin — yes, and optional (Task 7's `origin` param, Task 10's
  "Departing from (optional)" field); Date — out of scope for v1, and no
  date parameter exists anywhere in Tasks 7/10/11; Time-of-day range —
  implemented as the inclusive `from`/`to` pair (Tasks 5/7/10); Destination
  — the new backend work, Tasks 2-7; Operator — **not built**, per the
  table's "CIF rows have no operator field at all" and §6, with Task 10
  carrying an explicit `renders no operator filter at all` test so a future
  reader can't mistake the omission for an oversight. The result-row content
  bullets are covered by Task 10's row markup (time, origin → destination,
  a `/train/{uid}/{date}` link, a Track action) minus the "live-status
  summary" bullet, which is deliberately absent: §3's Approach B v1 is
  CIF-derived only, and fabricating a delay badge for a timetable row is the
  exact dishonesty §6's no-merging rule exists to prevent — Task 10's copy
  says so on the page.
- **§3 Approach B.** Grouping function — Task 3; publish on the existing
  cycle — Task 4; new table copy-adjacent to
  `schedule_network_departures`'s migration — Task 2; new ingest route —
  Task 6; new public `GET /public/trains/search` filtering server-side —
  Tasks 5+7. The section's own "needs its own sizing pass, not an assumed
  reuse of `MAX_DEPARTURES_PER_STATION = 10`" is Task 1, gating Task 4. The
  "operator stays unavailable" and "LDBWS coverage is not extended" caveats
  are both in Global Constraints and neither is contradicted by any task.
- **§4 The `/track` replacement question.** `/track` is not deleted, hidden,
  redirected, or modified by any task in this plan — `TrackTrainForm.tsx`
  and `app/track/page.tsx` appear in no task's Files list. `/trains` becomes
  the primary discovery surface (Task 11's nav entry, and Task 12's page
  copy now pointing there). `TrackTrainForm` is kept as the explicit
  fallback, reachable from `/trains` itself via Task 10's "Can't find your
  train? Track it manually" link, which carries `?ticketId=` through. The
  ticket-attach parity fix §4 calls for is Task 9's `attachTicketId` branch,
  which mirrors `TrackTrainForm.tsx:319-335` including its swallow-on-failure
  behaviour, proven by two tests (reject, and 409).
- **§5 The `/train/[uid]/[date]` CTA.** Every bullet: visible to every
  visitor with the `useNeedsLogin`/`LoginPromptModal` posture (Task 9's
  component, Task 12's placement); calls
  `POST /Train/by-uid/{uid}/{date}/track` with no body; navigates to
  `/train/by-id/{trackingId}` on success; `markNeedsLogin()` +
  `LoginPromptModal` on 401; **no** ticket-attach parameter (Task 12 asserts
  its absence); no special treatment for a visitor who already tracks the
  train — with the difference that this plan no longer has to *assume* the
  repeat click is safe, because Task 8 made it so.
- **§6 Explicitly out of scope.** All ten bullets are reproduced verbatim in
  Global Constraints. Checked against every task: no task touches
  `/Train/track`/`TrackPinRequest`/`post_track`; no task decodes `BX`,
  operator or headcode; no task adds a date parameter; Task 3's function is
  a plain stack-local pass with no cache or `static`; Task 7 reads Postgres
  only, never `schedule-reference`; `poller-ldbws` appears in no task; no
  task queries `station_samples`; no task merges two sources; no task adds a
  `ticketId` convention to `/train/[uid]/[date]`; every component ships
  complete Mantine markup.
- **§7 Open questions.** 1 and 2 are resolved by Tasks 1 and 8 respectively
  and written up in "Decisions this plan resolves"; 3 is void under a
  CIF-only v1; 4 is resolved by inspection and recorded in Task 2's
  migration header; 5 is implemented in Task 11; 6 is inherited and
  untouched.

**2. Placeholder scan.** No "TBD", "TODO", "implement later", "add
appropriate error handling", "similar to Task N", or "write tests for the
above" appears anywhere. Every code step contains literal code. Two things
this pass found and fixed inline rather than reported:

- Task 7 Step 6 originally referred readers to
  `crates/api/src/routes/departures.rs:162-219` to paste `test_app` from,
  via a fake `use … as _;` line. That is exactly the "steps that describe
  what to do without showing how" failure mode, so the complete
  `fn test_app(pool: PgPool) -> App` body is now written out in the task.
- Task 11 Step 5 originally said "use whichever helper the file's existing
  nav assertions already use", a deferred decision. Reading
  `frontend/app/layout.test.tsx` settled it: the nav's static links live
  inside the unexported, un-renderable `RootLayout`, and that file already
  has a `readFileSync('app/layout.tsx', 'utf8')` source-assertion precedent
  (line 75-77) for exactly this. The step now contains the real test.

The one remaining value that is intentionally decided outside the plan text
is Task 4's `MAX_DEPARTURES_PER_DESTINATION = 200`, and that is not a
placeholder: it is a real, working default with an explicit
confirm-or-replace gate (Task 1) and a stated arithmetic criterion, which is
the writing-plans-sanctioned way to represent a decision only the repo owner
can make with real data.

**3. Type and signature consistency across tasks.**
- `DestinationDeparture { uid: String, origin_crs: String, scheduled:
  NaiveTime }` (Task 3) is what Task 4's `schedule_destination_departures_rows`
  serializes, giving JSON keys `uid`/`origin_crs`/`scheduled` — exactly the
  keys Task 5's SQL names (`elem->>'origin_crs'`, `elem->>'scheduled'`),
  Task 5's test fixtures use, and Task 7's `destination_departure_json`
  reads.
- Task 4's POST body element `{destination_crs, service_date, departures}`
  matches Task 5's `ScheduleDestinationDeparturesRow` field-for-field, which
  is what Task 6's handler deserializes.
- `search_schedule_destination_departures(pool, destination_crs,
  service_date, origin_crs, from_time, to_time, limit) ->
  Result<Option<Vec<Value>>>` (Task 5) is called with exactly that arity and
  those types by Task 7, whose `let … else` destructures the `Option` into
  the 404 branch.
- `destination_departure_json(d, destination_crs)` (Task 7) emits
  `{uid, scheduled, originCrs, destinationCrs}`, which is exactly Task 10's
  `TrainSearchRow` interface and exactly what Task 10's test fixtures
  produce.
- `TrackThisTrainButton({ uid, date, attachTicketId?, size? })` (Task 9) is
  called by Task 10 with all four props (`size="xs"`) and by Task 12 with
  two — never with a prop it doesn't declare.
- `TrainSearchForm({ initialDestination?, initialOrigin?, attachTicketId? })`
  (Task 10) is called by Task 11 with exactly those three.
- `create_subscription_for_train(pool, trains_id, user_id) ->
  anyhow::Result<i64>` (Task 8) keeps its exact existing signature, so
  `post_track_by_uid` (`crates/api/src/routes/train.rs:699-702`) needs no
  change and none is planned.
- Route path segments are consistent between Rust and the frontend:
  `/private/schedule-destination-departures` (Task 6 route, Task 4 config
  default, Task 6 app.rs auth entry, Task 4 Helm env all agree);
  `/public/trains/search` (Task 7) reaches the frontend as
  `/api/trains/search` through the proxy's `/public/` prefixing rule, which
  Task 10 uses verbatim; `/Train/by-uid/{uid}/{date}/track` reaches it as
  `/api/Train/by-uid/…` through the proxy's `Train`-passthrough rule, which
  Task 9 uses verbatim.

**4. Ordering check.** Task 1 gates Task 4 (stated in both). Tasks 2→5→6 and
2→5→7 are strict data dependencies. Task 3 gates Task 4. Task 8 gates Tasks
9-12 (stated in Task 8's own header and in "Decisions this plan resolves").
Task 9 gates Tasks 10 and 12. Task 10 gates Task 11. Tasks 2 and 3 are
independent of each other and of Task 1, so work can begin immediately while
Task 1's diagnostic is being run.

**5. Global Constraints self-check.** Every new query uses runtime-checked
`sqlx::query`/`query_as` (no `query!`); every new JSON response goes through
a hand-built `json!()` render function; every live-DB test uses the
`#[tokio::test] #[ignore]` + manual `connect()` pattern with explicit
`DELETE` cleanup and no `#[sqlx::test]`; the fixture CRS codes claimed
(`ZRB`-`ZRF`) collide with nothing already in use.
