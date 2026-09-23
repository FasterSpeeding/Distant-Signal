# Plan: Dynamic Trip Planning — Phase 2: Connections Array + Shared Interchange Layer

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 2 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8:
produce the two things Connection Scan (Phase 3) and RAPTOR (Phase 4)
both need and must consume **identically**, so their later agreement is
meaningful — a whole-day, sorted-by-departure `Connection` array, and a
shared, pure interchange-lookup layer (minimum same-station change time,
CRS-sibling TIPLOCs, and valid cross-CRS fixed links at a given time). **No
search algorithm yet** — this phase proves both pieces correct in isolation,
against constructed fixtures, before either scan algorithm is built on top.
This phase also resolves the design spec's own §7 Open Question 1 (where the
whole-network build actually runs) with a concrete answer — see Judgment
Call 1.

**Architecture:** this phase corrects one detail of the design spec's own
§8 Phase 2 phrasing after re-reading the real, working sibling
`Distant-Signal-MCP` implementation directly (re-cloned for this plan's own
research pass — see Judgment Call 2): interchange is **not** materialized as
synthetic "transfer connections" inside the connections array itself. It is
a separate, shared lookup layer (`minimum_change_time`/`sibling_tiplocs`/
`fixed_links_from`) that Phase 3/4's own search algorithms each call
*directly* while walking the plain train-connections array — exactly
mirroring the sibling's own `interchange.ts`, which both `csa.ts` and
`raptor.ts` import unchanged, while each independently reimplements its own
`relax`/`readySourceAt` search logic on top of it (deliberately, so a later
differential test compares two genuinely independent implementations, not
one calling the other).

Three pieces:

1. **A new Postgres table, `schedule_calling_points_full`** (Task 1),
   populated by `crates/schedule-reference` every ~30-minute cycle exactly
   like the existing `schedule_destination_departures` forward-window
   publish (`main.rs`'s `forward_publish_dates`) — the literal, un-bucketed
   GTFS-shaped "trip + ordered stop_times" data §0.2 of the design spec
   already describes as sitting in memory every cycle, persisted for the
   first time so `api` can read it back without ever touching CIF text or
   holding a resident whole-network index of its own. This directly answers
   Open Question 1 (Judgment Call 1).
2. **`crates/schedule-query` gains two new, pure, I/O-free modules** (Task
   2): `connections.rs` (`Connection`, `CallingPointForConnections`,
   `build_connections`) and `interchange.rs` (`InterchangeData`,
   `ChangeTime`, `minimum_change_time`, `sibling_tiplocs`, `fixed_links_from`)
   — both operate on already-fetched, already-shaped data, matching this
   crate's own established "no I/O, take plain data in" convention
   (`schedule-query/src/lib.rs`'s own module doc).
3. **`crates/api` gains a new data-layer module, `data/trip_planning.rs`**
   (Task 3), the I/O glue: fetches one date's rows from
   `schedule_calling_points_full` and builds `InterchangeData` from the
   already-existing `stanox_crs`/`fixed_links` tables (Phase 1), then calls
   Task 2's pure functions. This is the exact "build fresh per query,
   discard after" pattern the sibling project's own measured, shipped
   design uses (26,848 schedules / ~289,514 connections built once per
   `plan_journey` call and thrown away) — reused here against Postgres
   instead of that project's SQLite store.

**Tech stack:** Rust (`schedule-reference`, `schedule-query`, `api`),
sqlx/Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§0.2, §1, §2, §3, §7 Open Question 1, §8 Phase 2 — authoritative for scope
and the CSA/RAPTOR requirement to share input. This plan's own Phase 1
(`2026-09-22-dynamic-trip-planning-phase1-cif-interchange-ingestion-plan.md`)
is a hard prerequisite: `stanox_crs.change_time_minutes` and `fixed_links`
must already exist and be populated.

---

## Judgment calls this plan makes (read before Task 1)

1. **Resolves the design spec's own §7 Open Question 1 (resident index vs.
   bounded prefilter vs. build-and-discard) with a concrete answer: neither
   of the spec's own two named options — build a new whole-network Postgres
   table of already-resolved calling points, and build the connections array
   fresh, in `api`, per query, from an indexed read of that table.** The
   spec's own §3 frames this as unresolved and recommends starting with
   "bound the search scope per query" (a geographic/line-catalogue
   prefilter, option 2) specifically to avoid `api`'s first resident
   whole-network index. Re-reading the actual code this pass surfaces a
   structural fact the spec's own §3 discussion doesn't have available to
   it: **`api` has no access to raw CIF text at all — only
   `schedule-reference`'s container does** (it alone reads the
   PVC-mounted `RJTTFnnnMCA.txt`). A prefilter bounded by
   `lines/*.toml`/`schedule_crs_line_index` (§0.1, already resident in
   `AppState` — `crates/api/src/app.rs:57`) narrows *which stations* a
   query considers, but CSA and RAPTOR both fundamentally need ONE
   globally-sorted connections array to do their single linear sweep /
   per-round sweep over — there is no way to "fetch as the search explores
   outward" against a per-CRS Postgres row without first materializing
   something connections-array-shaped, at which point the prefilter's own
   promised savings mostly evaporate for any query that legitimately spans
   more than a couple of catalogued lines (exactly the kind of unusual,
   real cross-country journey §3's own "Real risk this option accepts"
   paragraph already worries a prefilter would miss).

   The sibling project's own "third possibility" (§3) — build the whole
   day fresh per query and discard it — turns out to be more directly
   portable than the spec's own text expected, once one structural fact is
   named plainly: that project's cheap build is cheap because it re-reads
   from an **already-ingested, indexed store** (a real SQL query,
   `resolvedCallingPointsForDate`, bounded to one date), not because it
   re-parses raw CIF flat files per query. This app has no equivalent
   store *for the full network* today — but it already has the closest
   possible precedent for building one: `schedule_destination_departures`
   is already a full, uncapped, whole-network-for-one-day product,
   published fresh every cycle (`main.rs`'s own doc: "~377,000 objects,
   ~30MB" per day, "~1.7-2 million rows torn down and rebuilt" across its
   own forward window) and read back by `api` via ordinary indexed SQL at
   request time. **Persisting the full calling-point set the same way** —
   this app's own §0.2 already calls it "a GTFS-style trips + ordered
   stop_times table," so this is a literal, un-bucketed persistence of
   data this codebase already resolves every cycle, not a new kind of
   product — gives `api` exactly the same kind of already-ingested,
   indexed store the sibling's own cheap build relies on. Building the
   connections array from it, per query, inside a normal `api` request
   handler (Task 3), then discarding it, needs no new service, no HTTP
   surface on `schedule-reference`, and no resident whole-network index
   anywhere — sidestepping both of the spec's own named options' real
   costs (a lossy geographic prefilter that can silently miss a real
   route; a brand-new resident-index operational commitment) rather than
   picking one. **This is a plan-level architecture decision that overrides
   the design spec's own "start with option 2" recommendation** — flagged
   here explicitly, with the concrete reasoning above, for the product
   owner to review before Phase 5 ships against it. It should still be
   measured against real usage (per capacity numbers below) before being
   treated as permanently settled, exactly as the spec's own §3 already
   asks of any hosting choice.

   **Capacity check, done honestly rather than assumed**: the sibling's own
   real measurement is 316,362 public calling points/day. This app's own
   already-accepted `schedule_destination_departures` product is a
   comparable order of magnitude (~377,000 rows/day) at ~30MB, with
   ~3.3x headroom under `DefaultBodyLimit::max(100 * 1024 * 1024)`
   (`crates/api/src/routes/mod.rs:86`, cited directly in that product's own
   doc comment). A calling-points-full row is narrower per row (tiploc,
   seq, kind, two nullable times, day_offset — no `true_origin_crs`/
   `destination_arrival` bucket-duplication fields), so this new product,
   at a comparable row count, is not expected to exceed that same envelope.
   Task 1's own Step 5 measures the real POST body size against a real
   delivery before this is treated as settled rather than assumed.

2. **The design spec's own §8 Phase 2 wording — "the logic that turns Phase
   1's interchange data into synthetic 'transfer connections' a search
   algorithm can traverse the same way as a real train connection" — is
   corrected here, not implemented literally, after independently re-reading
   the real sibling implementation this pass** (re-cloned from
   `ssh://git@git-bringer-ssh.fox-prometheus.ts.net/lucy/Distant-Signal-MCP.git`,
   since the design spec's own provenance note says this exact detail was
   carried forward unverified). The real, working, tested implementation —
   `src/timetable/plan/interchange.ts` (`minimumChangeTime`, `siblingTiplocs`,
   `fixedLinks`) plus `src/timetable/plan/csa.ts`'s own `relax`/
   `relaxFixedLinks`/`readySourceAt` — does **not** pre-materialize transfer
   connections into the connections array at all. Interchange is checked
   **live, during the scan**, at the moment a stop's arrival improves: same
   train continuation is free and needs no interchange check at all; a
   *fresh* boarding at a stop charges that stop's own `minimumChangeTime`
   (which can be infinite at the nine real "not a rail interchange"
   sentinel stations, `interchange.ts:5-54`); and every improvement to a
   stop's earliest arrival also tries "relaxing" that stop's own fixed
   links, recursively, exactly the textbook "Connection Scan with
   footpaths" formulation. Baking this into fabricated `Connection` entries
   instead would mean inventing fake `(dep_stop, dep_time, arr_stop,
   arr_time)` rows for a search that already has a clean way to check
   validity live against the actual clock time of the specific arrival
   being relaxed — the ALF validity-window check (§0.4: "Euston → King's
   Cross is 5–10 min by tube but 15 by transfer depending on time of day")
   cannot be correctly pre-baked into a single static synthetic connection
   at all, since which of several ALF rows for the same physical link
   applies depends on the time the passenger actually arrives there, not a
   value knowable when the connections array is first built. This phase
   therefore builds the shared *lookup* layer (`interchange.rs`) Phase 3/4
   call live, not a synthetic-connection generator.

3. **`crates/schedule-query` becomes the home for both new modules**, per
   the design spec's own §8 Phase 2 suggestion, and gains a genuinely new
   dependency on `common` (for `common::StanoxCrsRecord`/`FixedLinkRecord`)
   to build `InterchangeData` from Phase 1's published rows — a small,
   reasonable addition given `common` is this workspace's cross-cutting
   types crate, already depended on by every other production crate. This
   crate's own module doc (`lib.rs:17-26`) currently states "nothing in
   `crates/api`... depends on this crate" as a deliberate, load-bearing
   fact; Phase 5 (not this phase) is what first makes that no longer true,
   and this plan flags the doc comment for Phase 5 to update rather than
   updating it prematurely here, before any real consumer exists.

4. **`Connection.uid` doubles as the "which physical working is this" key
   RAPTOR/CSA both need (the sibling's own `scheduleId`), because this
   app's data is UID-keyed at the source, not synthetic-integer-keyed.**
   The sibling's own `Connection.scheduleId`/`sourceScheduleId` split exists
   because its `constraints.ts` (route `via`/`avoid` constraints — out of
   this app's v1 scope per the design spec's §1 "Not re-litigated here")
   relabels `scheduleId` into a synthetic per-run id for a feature this app
   is not building. Without that feature, one field (`uid`, the real train
   UID `resolve_for_date` already resolves) does the whole job here —
   simpler than the sibling's split by construction, not a missing feature.

5. **Real overnight day-offset handling improves on the sibling's own
   per-pair heuristic, because this app's data already carries something
   the sibling's does not.** The sibling's `connectionBetween`
   (`connections.ts:114-120`) infers a midnight rollover per adjacent pair
   (`arrivalMin < departureMin` ⇒ add 1440) because its own store has no
   day-offset field on a calling point. This app's `ScheduleIndex` already
   computes `CallingPoint::day_offset` for every calling point via
   `assign_day_offsets` (`schedule-query/src/resolve.rs:61-78`), correctly
   handling a schedule that crosses **two** midnights (a case the sibling's
   own single-pair heuristic cannot distinguish from a same-day
   regression). `build_connections` (Task 2) computes
   `departure_min`/`arrival_min` directly from each calling point's own
   `day_offset * 1440 + minutes_since_midnight`, never re-deriving a
   rollover from adjacent-pair comparison — a strict correctness
   improvement this app's existing infrastructure makes free, not a
   simplification.

---

## Non-goals

- **No CSA, no RAPTOR, no route-pattern grouping.** Phase 3/4's job.
- **No live HTTP route** — `data/trip_planning.rs`'s functions are called
  directly from Rust tests in this phase; Phase 5 wires them behind
  `GET /Trips/plan`.
- **No route constraints (`via`/`avoid`/`viaStop`/`avoidStop`)** — explicitly
  named as "not re-litigated" future work in the design spec's §1; this
  phase's `Connection`/`InterchangeData` shapes carry nothing constraint-related.
- **No resolution of "walking from Paddington mainline to the Elizabeth
  line's own Paddington platforms"-style gaps in the underlying ALF data
  itself** — if a real ALF extract genuinely has no fixed link between two
  stations a human would consider "the same place," this phase's lookup
  layer honestly reports no link; it does not invent one (matching the
  sibling's own disclosed, not-papered-over gap, `csa.ts:336-341`).

## Global Constraints

- **File scope.** Created/modified:
  `crates/api/migrations/20260923100000_schedule_calling_points_full.sql`
  (new; sorts after Phase 1's `20260923090000_fixed_links_and_change_time.sql`),
  `crates/schedule-reference/src/main.rs`,
  `crates/schedule-query/Cargo.toml`,
  `crates/schedule-query/src/lib.rs`,
  `crates/schedule-query/src/connections.rs` (new),
  `crates/schedule-query/src/interchange.rs` (new),
  `crates/api/Cargo.toml` (add `schedule-query` as a dependency if not
  already present — check with `grep schedule-query crates/api/Cargo.toml`
  first; per Judgment Call 3's own citation of `schedule-query`'s "nothing
  in `crates/api` depends on this crate" doc comment, it is very likely
  absent today),
  `crates/api/src/data/trip_planning.rs` (new),
  `crates/api/src/data/mod.rs` (register the new module),
  `crates/api/src/routes/ingest.rs` (new
  `POST /private/schedule-calling-points-full` route, mirroring
  `post_schedule_destination_departures`'s existing shape — confirm its
  exact name via `grep -n "schedule_destination_departures" crates/api/src/routes/ingest.rs`
  before writing this task's own route, and match its body-handling
  convention exactly).
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings`, `cargo test --workspace`
  (ignored tests skipped), `cargo test -p api -- --ignored --test-threads=1`
  for every DB-gated test this plan adds — all four exact invocations
  `.github/workflows/ci.yml`'s `rust-test` job runs. `schedule-query`'s own
  new modules are pure and need no `DATABASE_URL` at all — every test in
  Task 2 runs under the plain `cargo test --workspace` step.
- **No invented interchange semantics.** `minimum_change_time`'s
  98/99-sentinel and 5-minute-default policy (Task 2) must match the real,
  independently-confirmed values this plan's own research pass re-verified
  against the sibling project's `interchange.ts` (re-cloned and re-read
  directly, not carried forward from an earlier, unverified pass) — not
  re-derived from memory or invented afresh.

## Review Focus

- **A schedule with only one calling point** (a real, if rare, CIF shape —
  e.g. a cancelled-and-replaced short working) — `build_connections` must
  emit zero connections for it, not panic on an empty `windows(2)`.
- **A calling point pair where the earlier point has no `booked_departure`
  or the later has no `booked_arrival`** (a real shape per this app's own
  `CallingPointKind::Terminate`/`Origin` split — a `Terminate` point is
  arrival-only) — must be silently skipped as a non-connection, not
  produce a connection with a fabricated time.
- **A same-CRS "sibling" TIPLOC lookup for a TIPLOC with no CRS at all**
  (a junction-only TIPLOC with no MSN record, a real, confirmed-existing
  gap per Phase 1's own `parse_msn_change_time_by_tiploc` doc) — must
  return an empty sibling list, not panic or fabricate a CRS.
- **A fixed link whose day-of-week mask excludes the query's date, or
  whose HHMM validity window excludes the query's time** — `fixed_links_from`
  must exclude it even though the row exists in the table; a query one
  minute outside a link's window is a real, correctness-relevant case, not
  an edge case to skip testing.
- **Two ALF rows for the same `(from_crs, to_crs)` both valid at the query
  time, with different durations** (the real Euston↔King's Cross
  tube-vs-transfer case, §0.4) — `fixed_links_from` must return only the
  shorter one; returning both, or the wrong one, would let a search offer a
  transfer slower than what is actually fastest available.

---

## Task 1: `schedule_calling_points_full` — the persisted whole-network product

**Files:**
- Create: `crates/api/migrations/20260923100000_schedule_calling_points_full.sql`
- Modify: `crates/schedule-reference/src/main.rs`
- Modify: `crates/api/src/routes/ingest.rs`

**Interfaces:**
- Produces: a `schedule_calling_points_full` table, one row per calling
  point per schedule per published date, readable by Task 3's
  `trip_planning::fetch_calling_points_for_date`.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Dynamic Trip Planning Phase 2: the persisted, whole-network, STP-resolved
-- calling-point product this app's own §0.2 investigation already
-- identified as sitting in memory every ~30-minute cycle
-- (schedule_query::ScheduleIndex) but never previously written to
-- Postgres. See
-- docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §0.2
-- and this plan's own Judgment Call 1 for why this is the chosen answer
-- to that design spec's Open Question 1.
--
-- One row per calling point of one resolved (non-cancelled) schedule on one
-- service date -- the literal GTFS "ordered stop_times per trip" shape,
-- persisted un-bucketed for the first time. `api` reads this back, per
-- trip-planning query, to build a Connection array fresh (see
-- schedule_query::connections::build_connections) -- never held resident
-- between requests.
--
-- Wholesale-replaced per service_date on every publish cycle, same
-- "DELETE ... WHERE service_date = $1" convention
-- schedule_destination_departures already established -- see
-- crates/api/src/routes/ingest.rs's existing handler for that table.
-- -------------------------------------------------------------------------

CREATE TABLE schedule_calling_points_full (
    service_date      DATE NOT NULL,
    uid               TEXT NOT NULL,
    -- 1-based position within this schedule's own calling-point sequence --
    -- the ORDER BY key that reconstructs stopping order; NOT a real CIF
    -- field, assigned at publish time.
    seq               SMALLINT NOT NULL,
    tiploc            TEXT NOT NULL,
    kind              TEXT NOT NULL CHECK (kind IN ('origin', 'intermediate', 'terminate')),
    booked_arrival    TIME,
    booked_departure  TIME,
    day_offset        SMALLINT NOT NULL,
    PRIMARY KEY (service_date, uid, seq)
);
```

- [ ] **Step 2: Verify the migration applies**

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d schedule_calling_points_full"
```

- [ ] **Step 3: Add `POST /private/schedule-calling-points-full`** in
  `crates/api/src/routes/ingest.rs`. First read the existing
  `post_schedule_destination_departures` handler (`grep -n
  "post_schedule_destination_departures" -A 25 crates/api/src/routes/ingest.rs`)
  to copy its exact `DELETE ... WHERE service_date = $1` + batch-`INSERT`
  transaction shape and its request body type (a bare
  `Vec<serde_json::Value>` or a typed row struct — match whichever that
  route actually uses). Add a matching handler and `queries::upsert_schedule_calling_points_full`
  function with the same per-`service_date` full-replace semantics, and
  mount it alongside the existing route with the same internal-OAuth group
  entry `schedule-reference`'s other publishes already use.

- [ ] **Step 4: Add the `schedule-reference` publish.** In `main.rs`,
  add a new constant next to `DESTINATION_DEPARTURES_FORWARD_DAYS`:

```rust
/// Forward publish window for `schedule_calling_points_full` -- same value
/// as DESTINATION_DEPARTURES_FORWARD_DAYS (both are whole-network,
/// full-day products published on the same cycle for the same reason: a
/// trip-planning query needs the query date, which may be up to a week
/// ahead, immediately queryable without waiting for a same-day publish).
const TRIP_PLANNING_FORWARD_DAYS: i64 = 7;
```

  and a new function, called from `publish_cif_derived_products` alongside
  the three existing per-date publishes:

```rust
/// Publishes this cycle's whole-network resolved calling points for `date`
/// -- the literal, un-bucketed persistence Phase 2 of the dynamic
/// trip-planning plan adds (see that plan's Task 1). Unlike
/// `publish_schedule_destination_departures`'s own bucketed/flattened
/// shape, this emits every calling point of every non-cancelled schedule,
/// in order, with no `now`-forward filter at all (a trip-planning query
/// needs the WHOLE day, including departures already in the past relative
/// to publish time, since the traveller picks their own date/time at query
/// time, not at publish time -- same reasoning as
/// `publish_schedule_destination_departures`'s own `NaiveTime::MIN`, see
/// that function's doc comment, point 1).
async fn publish_schedule_calling_points_full(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    date: chrono::NaiveDate,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        for (seq, cp) in resolved.calling_points.iter().enumerate() {
            let kind = match cp.kind {
                schedule_query::CallingPointKind::Origin => "origin",
                schedule_query::CallingPointKind::Intermediate => "intermediate",
                schedule_query::CallingPointKind::Terminate => "terminate",
            };
            rows.push(serde_json::json!({
                "service_date": date,
                "uid": resolved.uid,
                "seq": seq as i32,
                "tiploc": cp.tiploc,
                "kind": kind,
                "booked_arrival": cp.booked_arrival,
                "booked_departure": cp.booked_departure,
                "day_offset": cp.day_offset,
            }));
        }
    }

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_calling_points_full_url,
        internal_oauth,
        &rows,
        "schedule-derived full calling-point rows",
    )
    .await
    {
        tracing::error!(error = ?err, %date, "failed to publish schedule calling points; will retry next cycle");
    }
}
```

  and call it from `publish_cif_derived_products`'s existing
  `for date in forward_publish_dates(today, ...)` loop (add it as a
  sibling call next to `publish_schedule_destination_departures`, reusing
  the SAME loop rather than a second one, per this file's own "one pass,
  multiple outputs" precedent), plus add `schedule_calling_points_full_url`
  to `config.rs` the same way `FIXED_LINKS_URL` was added in Phase 1.

- [ ] **Step 5: Measure the real body size** against a real delivery once
  Phase 1's own live-delivery access is available (Judgment Call 1's own
  capacity check) — log `rows.len()` and the serialized body's byte length
  once in a real environment and confirm it stays comfortably under
  `DefaultBodyLimit::max(100 * 1024 * 1024)`; if it does not, apply the
  same `for chunk in rows.chunks(50_000)` fallback
  `schedule_destination_departures`'s own doc comment already documents for
  this exact situation, teaching the ingest handler "the first chunk clears
  the date."

- [ ] **Step 6: Commit**

```bash
git add crates/api/migrations/20260923100000_schedule_calling_points_full.sql \
        crates/schedule-reference/src/main.rs crates/schedule-reference/src/config.rs \
        crates/api/src/routes/ingest.rs
git commit -m "schedule-reference,api: persist the whole-network resolved calling-point set per service date"
```

---

## Task 2: `crates/schedule-query` — `connections.rs` + `interchange.rs`

**Files:**
- Modify: `crates/schedule-query/Cargo.toml`
- Modify: `crates/schedule-query/src/lib.rs`
- Create: `crates/schedule-query/src/connections.rs`
- Create: `crates/schedule-query/src/interchange.rs`

**Interfaces:**
- Produces: `connections::{Connection, CallingPointForConnections,
  build_connections}`; `interchange::{InterchangeData, ChangeTime,
  minimum_change_time, sibling_tiplocs, fixed_links_from}`. Both consumed
  by Task 3 and, later, Phase 3/4.

- [ ] **Step 1: Add the `common` dependency**

```toml
# crates/schedule-query/Cargo.toml, in [dependencies]
common = { path = "../common" }
```

- [ ] **Step 2: Write `connections.rs`**

```rust
//! A whole-day connections array: one entry per consecutive pair of a
//! resolved, non-cancelled schedule's calling points, sorted by departure.
//! The shared input both Connection Scan (Phase 3) and RAPTOR (Phase 4)
//! sweep -- see this codebase's
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md
//! for why the two algorithms must consume the SAME array, unmodified by
//! either, for their later agreement to be meaningful.
//!
//! No I/O, no dependency on [`crate::resolve::ScheduleIndex`] -- takes
//! [`CallingPointForConnections`] slices grouped by schedule identity,
//! deliberately decoupled from HOW those calling points were produced (an
//! in-memory `ScheduleIndex` resolve, in this crate's own tests, or a
//! Postgres row set re-hydrated by `crates/api`'s `data::trip_planning`,
//! in production) -- same "pure function over already-shaped data"
//! convention every other function in this crate already follows.

use chrono::NaiveTime;

/// One calling point, reduced to exactly what `build_connections` needs --
/// deliberately NOT [`crate::records::CallingPoint`] itself, so this module
/// has no dependency on how the caller obtained the data (a resolved
/// `ScheduleIndex` schedule, or a Postgres row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallingPointForConnections {
    pub tiploc: String,
    pub booked_arrival: Option<NaiveTime>,
    pub booked_departure: Option<NaiveTime>,
    /// Calendar days past this schedule's own service date -- see
    /// [`crate::records::CallingPoint::day_offset`]'s own doc comment for
    /// the real overnight case this exists to handle. `build_connections`
    /// folds this directly into `departure_min`/`arrival_min` rather than
    /// re-deriving a rollover from adjacent-pair comparison (an
    /// improvement over a same-pair-only heuristic -- see this plan's
    /// Judgment Call 5).
    pub day_offset: u8,
}

impl From<&crate::records::CallingPoint> for CallingPointForConnections {
    fn from(cp: &crate::records::CallingPoint) -> Self {
        Self {
            tiploc: cp.tiploc.clone(),
            booked_arrival: cp.booked_arrival,
            booked_departure: cp.booked_departure,
            day_offset: cp.day_offset,
        }
    }
}

/// One consecutive pair of public calling points on one schedule -- the
/// edge type both search algorithms walk. `uid` is this app's own train
/// UID, doubling as "which physical working is this" (see this plan's
/// Judgment Call 4 for why this app needs no separate synthetic id, unlike
/// the sibling `Distant-Signal-MCP` project's own `scheduleId`/
/// `sourceScheduleId` split).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub uid: String,
    pub from_tiploc: String,
    pub to_tiploc: String,
    /// Minutes from midnight of the schedule's OWN service date --
    /// `day_offset * 1440 + minutes-since-midnight`, so a connection whose
    /// calling points fall on a later calendar day than the schedule's own
    /// service date is already correctly ordered against same-day
    /// connections, with no adjacent-pair rollover heuristic needed.
    pub departure_min: u32,
    pub arrival_min: u32,
}

fn minutes_from_midnight(time: NaiveTime, day_offset: u8) -> u32 {
    use chrono::Timelike;
    time.num_seconds_from_midnight() / 60 + u32::from(day_offset) * 1440
}

/// Builds and sorts the whole connections array. `schedules` is every
/// resolved, non-cancelled schedule to include, as `(uid, calling points in
/// stopping order)` pairs -- the caller (a `ScheduleIndex`-driven test
/// fixture, or `crates/api`'s Postgres-row hydration) is responsible for
/// having already excluded cancelled schedules and having each schedule's
/// own calling points already in seq order; this function does no
/// resolution or reordering of its own.
///
/// A pair contributes a connection only when the earlier point has a
/// `booked_departure` and the later has a `booked_arrival` -- both real,
/// non-`Err` gaps (an `Origin` point has no arrival; a `Terminate` point
/// has no departure; see [`crate::records::CallingPointKind`]) that simply
/// produce no connection across that specific gap, never a fabricated one.
/// A schedule with fewer than two calling points contributes nothing.
pub fn build_connections<'a>(
    schedules: impl IntoIterator<Item = (&'a str, &'a [CallingPointForConnections])>,
) -> Vec<Connection> {
    let mut connections = Vec::new();
    for (uid, calling_points) in schedules {
        for pair in calling_points.windows(2) {
            let (from, to) = (&pair[0], &pair[1]);
            let (Some(departure), Some(arrival)) = (from.booked_departure, to.booked_arrival)
            else {
                continue;
            };
            connections.push(Connection {
                uid: uid.to_string(),
                from_tiploc: from.tiploc.clone(),
                to_tiploc: to.tiploc.clone(),
                departure_min: minutes_from_midnight(departure, from.day_offset),
                arrival_min: minutes_from_midnight(arrival, to.day_offset),
            });
        }
    }
    connections.sort_by_key(|c| c.departure_min);
    connections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(tiploc: &str, arrival: Option<&str>, departure: Option<&str>, day_offset: u8) -> CallingPointForConnections {
        CallingPointForConnections {
            tiploc: tiploc.to_string(),
            booked_arrival: arrival.map(|t| t.parse().unwrap()),
            booked_departure: departure.map(|t| t.parse().unwrap()),
            day_offset,
        }
    }

    #[test]
    fn a_three_stop_schedule_produces_two_connections_in_departure_order() {
        let points = vec![
            cp("EUSTON", None, Some("08:00:00"), 0),
            cp("WATFDJ", Some("08:20:00"), Some("08:21:00"), 0),
            cp("MKC", Some("08:50:00"), None, 0),
        ];
        let connections = build_connections([("C11052", points.as_slice())]);
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].from_tiploc, "EUSTON");
        assert_eq!(connections[0].to_tiploc, "WATFDJ");
        assert_eq!(connections[0].departure_min, 480);
        assert_eq!(connections[0].arrival_min, 500);
        assert_eq!(connections[1].from_tiploc, "WATFDJ");
        assert_eq!(connections[1].to_tiploc, "MKC");
    }

    #[test]
    fn a_single_calling_point_schedule_produces_no_connections() {
        let points = vec![cp("EUSTON", None, Some("08:00:00"), 0)];
        assert!(build_connections([("C11052", points.as_slice())]).is_empty());
    }

    #[test]
    fn an_overnight_calling_point_sorts_correctly_via_day_offset_not_a_rollover_heuristic() {
        // Real live-confirmed shape (schedule_query::resolve's own
        // day_offset tests): Liverpool St 23:48 -> Barking 00:06 next day.
        let points = vec![
            cp("LIVST", None, Some("23:48:00"), 0),
            cp("BARKING", Some("00:06:00"), None, 1),
        ];
        let connections = build_connections([("F49687", points.as_slice())]);
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].departure_min, 23 * 60 + 48);
        assert_eq!(connections[0].arrival_min, 1440 + 6);
        assert!(
            connections[0].arrival_min > connections[0].departure_min,
            "day_offset must make this a positive-duration connection, not a negative one"
        );
    }

    #[test]
    fn a_terminate_point_followed_by_nothing_boardable_contributes_no_connection() {
        // An Origin-kind point with no booked_departure at all (a real,
        // if rare, malformed-looking but non-panicking shape) must not
        // fabricate a connection.
        let points = vec![
            cp("EUSTON", None, None, 0),
            cp("MKC", Some("08:50:00"), None, 0),
        ];
        assert!(build_connections([("C1", points.as_slice())]).is_empty());
    }

    #[test]
    fn results_are_sorted_by_departure_across_multiple_schedules() {
        let early = vec![cp("A", None, Some("06:00:00"), 0), cp("B", Some("06:10:00"), None, 0)];
        let late = vec![cp("A", None, Some("09:00:00"), 0), cp("B", Some("09:10:00"), None, 0)];
        let connections = build_connections([("LATE", late.as_slice()), ("EARLY", early.as_slice())]);
        assert_eq!(connections[0].uid, "EARLY");
        assert_eq!(connections[1].uid, "LATE");
    }
}
```

- [ ] **Step 3: Write `interchange.rs`**

```rust
//! Shared, pure interchange lookups: minimum same-station change time,
//! CRS-sibling TIPLOCs (different platform groups of one physical
//! station), and valid cross-CRS fixed links at a given date/time. Both
//! Connection Scan (Phase 3) and RAPTOR (Phase 4) call these functions
//! directly while independently implementing their own scan/relaxation
//! logic on top -- see this plan's Judgment Call 2 for why this is a live
//! lookup layer, not synthetic connections baked into
//! [`crate::connections::Connection`].
//!
//! Real interchange rules and sentinel values, independently re-verified
//! this pass against the sibling `Distant-Signal-MCP` project's own
//! `src/timetable/plan/interchange.ts` (re-cloned directly for this plan's
//! research pass, not carried forward from an earlier, unverified pass):
//! the MSN member's minimum-change-time field defaults to 5 minutes when
//! absent (the modal real value), and two specific values (98, 99) are
//! sentinels meaning "not a real rail interchange" (a bus/coach stand),
//! never a literal duration -- see [`minimum_change_time`]'s own doc
//! comment for the full reasoning that project's own Task 1 investigation
//! established and this app inherits unchanged.

use std::collections::HashMap;

use chrono::NaiveDate;

/// Every other TIPLOC sharing this one's CRS code -- a different platform
/// group of the same physical station (the sibling's own real example:
/// Wimbledon's `WDON`/`WIMBLDN`/`WDNLUL` all share CRS `WIM`). Deliberately
/// does not collapse siblings into one node: a caller changing between them
/// still owes the real minimum-change-time cost, charged at the TIPLOC
/// actually being boarded at -- Phase 3/4's own scan logic is where that
/// charge is applied, not here.
#[derive(Debug, Clone)]
pub struct InterchangeData {
    /// TIPLOC -> raw minimum-change-time minutes, straight from
    /// `stanox_crs.change_time_minutes` (Phase 1) -- `None`/absent means
    /// "no MSN record matched this TIPLOC at all," genuinely different
    /// from a present sentinel or the modal 5 (see Phase 1's own Judgment
    /// Call 3).
    pub change_time_by_tiploc: HashMap<String, i32>,
    pub tiploc_to_crs: HashMap<String, String>,
    pub crs_to_tiplocs: HashMap<String, Vec<String>>,
    /// CRS -> every fixed link departing FROM it (ALF's `O` field),
    /// unfiltered by date/time -- [`fixed_links_from`] applies the
    /// date/time filter at lookup time.
    pub fixed_links_from_crs: HashMap<String, Vec<FixedLink>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedLink {
    pub mode: String,
    pub to_crs: String,
    pub minutes: i32,
    /// Raw "HHMM", 4 ASCII digits.
    pub valid_from: String,
    pub valid_to: String,
    /// Raw 7-char '0'/'1' bitmask, Monday-first.
    pub days_mask: String,
}

/// The result of looking up a station's minimum same-train-to-different-train
/// change time. `Finite` is real minutes; `NoInterchange` is the two real
/// sentinel values (98, 99) -- no candidate change duration can ever meet or
/// beat "no interchange possible here."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeTime {
    Finite(u32),
    NoInterchange,
}

impl ChangeTime {
    /// `true` if `available_minutes` is enough to make this change --
    /// callers compare with `>=`, not `>`: CIF publishes its minimum as the
    /// shortest connection that IS timetabled to work, not the shortest one
    /// that fails.
    pub fn allows(self, available_minutes: u32) -> bool {
        match self {
            ChangeTime::Finite(minimum) => available_minutes >= minimum,
            ChangeTime::NoInterchange => false,
        }
    }
}

/// Used whenever a TIPLOC's MSN record carries no change-time value at all
/// -- the modal value among stations that do carry one.
const DEFAULT_CHANGE_TIME: u32 = 5;

/// Raw MSN change-time values that appear against a bus/coach stand, never
/// a genuine rail interchange -- independently re-confirmed this pass
/// against the sibling project's own investigation (`interchange.ts:9-29`):
/// every station carrying either value in a real extract was traced to a
/// bus/coach stand (airport transfer stops, a market place), never a
/// station where a passenger changes between two trains.
const NO_INTERCHANGE_SENTINELS: [i32; 2] = [98, 99];

/// The shortest same-station change this app's own interchange data
/// considers valid at `tiploc`. Defaults to [`DEFAULT_CHANGE_TIME`] when
/// `tiploc` has no MSN record at all (a genuine gap, not a sentinel).
pub fn minimum_change_time(data: &InterchangeData, tiploc: &str) -> ChangeTime {
    match data.change_time_by_tiploc.get(tiploc) {
        None => ChangeTime::Finite(DEFAULT_CHANGE_TIME),
        Some(raw) if NO_INTERCHANGE_SENTINELS.contains(raw) => ChangeTime::NoInterchange,
        Some(raw) => ChangeTime::Finite((*raw).max(0) as u32),
    }
}

/// Every other TIPLOC sharing `tiploc`'s own CRS code, excluding `tiploc`
/// itself. Empty when `tiploc` has no CRS at all (a junction-only TIPLOC
/// with no MSN record -- the same population [`minimum_change_time`]
/// defaults for) or is the only TIPLOC recorded against its CRS.
pub fn sibling_tiplocs<'a>(data: &'a InterchangeData, tiploc: &str) -> Vec<&'a str> {
    let Some(crs) = data.tiploc_to_crs.get(tiploc) else {
        return Vec::new();
    };
    data.crs_to_tiplocs
        .get(crs)
        .map(|tiplocs| {
            tiplocs
                .iter()
                .filter(|candidate| candidate.as_str() != tiploc)
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default()
}

/// Monday-first day-of-week index (0=Monday..6=Sunday) for `date` -- same
/// convention `schedule_query::records::BasicSchedule::days_of_week` uses
/// for the CIF `SCHEDULE` member's own bitmask, applied here to ALF's
/// `days_mask` for consistency across this crate.
fn day_index(date: NaiveDate) -> usize {
    use chrono::Datelike;
    date.weekday().num_days_from_monday() as usize
}

fn clock_minutes(time_min: u32) -> u32 {
    time_min % 1440
}

fn to_hhmm(minutes: u32) -> String {
    format!("{:02}{:02}", minutes / 60, minutes % 60)
}

/// The fixed links (tube, walk, transfer, bus, ferry) actually usable from
/// `from_crs` at `date`/`time_min` -- ALF's raw rows, resolved down to the
/// ones that genuinely apply. `time_min` may exceed 1440 (an overnight
/// connection's own `arrival_min` convention, see [`crate::connections::Connection`]);
/// it is reduced to a same-day clock position before comparison, since a
/// fixed link's validity window is defined against the clock, not a
/// running total.
///
/// A row applies only when its day mask matches `date`'s day of week AND
/// `time_min`'s clock time falls inside its `valid_from`/`valid_to` window
/// (both bounds inclusive). Where several rows exist for the same
/// destination CRS and both are valid, only the shortest is returned --
/// callers must never be handed a slower option when a faster one is also
/// timetabled at the same moment (the real Euston↔King's Cross
/// tube-vs-transfer case, §0.4 of the design spec).
pub fn fixed_links_from(data: &InterchangeData, from_crs: &str, date: NaiveDate, time_min: u32) -> Vec<&FixedLink> {
    let clock = to_hhmm(clock_minutes(time_min));
    let day = day_index(date);

    let Some(candidates) = data.fixed_links_from_crs.get(from_crs) else {
        return Vec::new();
    };

    let mut shortest_by_destination: HashMap<&str, &FixedLink> = HashMap::new();
    for link in candidates {
        let Some(day_flag) = link.days_mask.as_bytes().get(day) else {
            continue;
        };
        if *day_flag != b'1' {
            continue;
        }
        if !(link.valid_from.as_str() <= clock.as_str() && clock.as_str() <= link.valid_to.as_str()) {
            continue;
        }
        match shortest_by_destination.get(link.to_crs.as_str()) {
            Some(existing) if existing.minutes <= link.minutes => {}
            _ => {
                shortest_by_destination.insert(&link.to_crs, link);
            }
        }
    }
    shortest_by_destination.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_data() -> InterchangeData {
        InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        }
    }

    #[test]
    fn a_tiploc_with_no_msn_record_defaults_to_five_minutes() {
        assert_eq!(minimum_change_time(&empty_data(), "ANYTPL"), ChangeTime::Finite(5));
    }

    #[test]
    fn a_recorded_finite_value_is_used_directly() {
        let mut data = empty_data();
        data.change_time_by_tiploc.insert("EUSTON".to_string(), 8);
        assert_eq!(minimum_change_time(&data, "EUSTON"), ChangeTime::Finite(8));
    }

    #[test]
    fn a_98_or_99_sentinel_means_no_interchange_possible() {
        let mut data = empty_data();
        data.change_time_by_tiploc.insert("BUSSTOP".to_string(), 99);
        assert_eq!(minimum_change_time(&data, "BUSSTOP"), ChangeTime::NoInterchange);
        assert!(!ChangeTime::NoInterchange.allows(10_000));
    }

    #[test]
    fn allows_uses_greater_than_or_equal_not_strictly_greater() {
        assert!(ChangeTime::Finite(5).allows(5));
        assert!(!ChangeTime::Finite(5).allows(4));
    }

    #[test]
    fn sibling_tiplocs_excludes_itself_and_returns_other_crs_members() {
        let mut data = empty_data();
        data.tiploc_to_crs.insert("WDON".to_string(), "WIM".to_string());
        data.tiploc_to_crs.insert("WIMBLDN".to_string(), "WIM".to_string());
        data.crs_to_tiplocs.insert(
            "WIM".to_string(),
            vec!["WDON".to_string(), "WIMBLDN".to_string(), "WDNLUL".to_string()],
        );
        let mut siblings = sibling_tiplocs(&data, "WDON");
        siblings.sort_unstable();
        assert_eq!(siblings, vec!["WDNLUL", "WIMBLDN"]);
    }

    #[test]
    fn sibling_tiplocs_is_empty_for_a_tiploc_with_no_crs() {
        assert!(sibling_tiplocs(&empty_data(), "JUNCTION").is_empty());
    }

    fn link(to_crs: &str, minutes: i32, valid_from: &str, valid_to: &str, days_mask: &str) -> FixedLink {
        FixedLink {
            mode: "TUBE".to_string(),
            to_crs: to_crs.to_string(),
            minutes,
            valid_from: valid_from.to_string(),
            valid_to: valid_to.to_string(),
            days_mask: days_mask.to_string(),
        }
    }

    fn monday() -> NaiveDate {
        // 2026-08-31 is independently confirmed a Monday elsewhere in this
        // codebase's own tests (schedule_query::records's own BasicSchedule
        // doc comment).
        NaiveDate::from_ymd_opt(2026, 8, 31).unwrap()
    }

    #[test]
    fn a_link_valid_on_this_day_and_time_is_returned() {
        let mut data = empty_data();
        data.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![link("KGX", 5, "0500", "2359", "1111111")],
        );
        let links = fixed_links_from(&data, "EUS", monday(), 8 * 60);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].to_crs, "KGX");
    }

    #[test]
    fn a_link_excluded_by_day_mask_is_not_returned() {
        let mut data = empty_data();
        // Sunday-only (index 6).
        data.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![link("KGX", 5, "0000", "2359", "0000001")],
        );
        assert!(fixed_links_from(&data, "EUS", monday(), 8 * 60).is_empty());
    }

    #[test]
    fn a_link_excluded_by_time_window_is_not_returned() {
        let mut data = empty_data();
        data.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![link("KGX", 5, "0500", "0800", "1111111")],
        );
        assert!(fixed_links_from(&data, "EUS", monday(), 9 * 60).is_empty());
    }

    #[test]
    fn an_overnight_time_min_past_1440_is_reduced_to_clock_time_first() {
        let mut data = empty_data();
        data.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![link("KGX", 5, "0500", "2359", "1111111")],
        );
        // 1440 + 8*60 = day-2 08:00 -- must still match a same-clock-time window.
        let links = fixed_links_from(&data, "EUS", monday(), 1440 + 8 * 60);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn only_the_shortest_of_several_valid_rows_to_the_same_destination_is_returned() {
        // The real Euston<->King's Cross case: tube (5 min) and transfer
        // (15 min) both valid at the same moment -- only the faster wins.
        let mut data = empty_data();
        data.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![
                link("KGX", 15, "0000", "2359", "1111111"),
                link("KGX", 5, "0500", "2359", "1111111"),
            ],
        );
        let links = fixed_links_from(&data, "EUS", monday(), 8 * 60);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].minutes, 5);
    }
}
```

- [ ] **Step 4: Register both modules** in `crates/schedule-query/src/lib.rs`:

```rust
pub mod connections;
pub mod interchange;
pub mod parse;
pub mod records;
pub mod resolve;
pub mod tiploc;

pub use connections::{build_connections, CallingPointForConnections, Connection};
pub use interchange::{fixed_links_from, minimum_change_time, sibling_tiplocs, ChangeTime, FixedLink, InterchangeData};
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p schedule-query connections:: interchange::
```

  Expected: all pass (14 tests across both modules).

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-query/Cargo.toml crates/schedule-query/src/lib.rs \
        crates/schedule-query/src/connections.rs crates/schedule-query/src/interchange.rs
git commit -m "schedule-query: add build_connections and the shared interchange lookup layer"
```

---

## Task 3: `crates/api` — `data/trip_planning.rs`, the per-query build

**Files:**
- Modify: `crates/api/Cargo.toml`
- Create: `crates/api/src/data/trip_planning.rs`
- Modify: `crates/api/src/data/mod.rs`

**Interfaces:**
- Produces: `trip_planning::{fetch_calling_points_for_date,
  fetch_interchange_data, build_connections_for_date}`, consumed directly
  by Phase 3/4's own tests and, in Phase 5, by the new `GET /Trips/plan`
  route.
- Consumes: Task 1's `schedule_calling_points_full` table, Phase 1's
  `stanox_crs`/`fixed_links` tables.

- [ ] **Step 1: Add the dependency** (`grep schedule-query
  crates/api/Cargo.toml` first — per Judgment Call 3, expected absent):

```toml
schedule-query = { path = "../schedule-query" }
```

- [ ] **Step 2: Write `trip_planning.rs`**

```rust
//! Per-query build of a service date's [`schedule_query::Connection`] array
//! and [`schedule_query::InterchangeData`] -- built fresh from Postgres on
//! every trip-planning query and discarded after, never held resident.
//! See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md's
//! Judgment Call 1 for why this shape (an already-ingested, indexed
//! Postgres read, not a raw CIF re-parse) was chosen over both of the
//! design spec's own named hosting options.

use std::collections::HashMap;

use anyhow::Result;
use chrono::NaiveDate;
use schedule_query::{
    build_connections, CallingPointForConnections, Connection, FixedLink, InterchangeData,
};
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
struct CallingPointRow {
    uid: String,
    seq: i16,
    tiploc: String,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    day_offset: i16,
}

/// Reads every `schedule_calling_points_full` row for `date`, grouped by
/// `uid` and already ordered by `seq` (the `ORDER BY` below, not an
/// in-memory re-sort) -- exactly the shape [`schedule_query::build_connections`]
/// needs. `None` if no rows exist for `date` at all (no CIF delivery has
/// published this far ahead yet) -- the caller (Phase 5's route handler)
/// maps this to a 404, same "no CIF-derived schedule data has been
/// published for this leg's service date" convention
/// `search_journey_leg_candidates` already establishes.
pub async fn fetch_calling_points_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<HashMap<String, Vec<CallingPointForConnections>>>> {
    let rows: Vec<CallingPointRow> = sqlx::query_as(
        "SELECT uid, seq, tiploc, booked_arrival, booked_departure, day_offset \
         FROM schedule_calling_points_full WHERE service_date = $1 ORDER BY uid, seq",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut by_uid: HashMap<String, Vec<CallingPointForConnections>> = HashMap::new();
    for row in rows {
        by_uid.entry(row.uid).or_default().push(CallingPointForConnections {
            tiploc: row.tiploc,
            booked_arrival: row.booked_arrival,
            booked_departure: row.booked_departure,
            day_offset: row.day_offset.max(0) as u8,
        });
    }
    Ok(Some(by_uid))
}

/// [`fetch_calling_points_for_date`] plus [`schedule_query::build_connections`]
/// in one call -- the single function Phase 5's route handler calls.
pub async fn build_connections_for_date(pool: &PgPool, date: NaiveDate) -> Result<Option<Vec<Connection>>> {
    let Some(by_uid) = fetch_calling_points_for_date(pool, date).await? else {
        return Ok(None);
    };
    let schedules: Vec<(&str, &[CallingPointForConnections])> =
        by_uid.iter().map(|(uid, points)| (uid.as_str(), points.as_slice())).collect();
    Ok(Some(build_connections(schedules)))
}

#[derive(Debug, sqlx::FromRow)]
struct FixedLinkRow {
    from_crs: String,
    to_crs: String,
    mode: String,
    minutes: i32,
    valid_from: String,
    valid_to: String,
    days_mask: String,
}

/// Builds [`InterchangeData`] from the whole current `stanox_crs` and
/// `fixed_links` tables (Phase 1) -- both are small (~3,100 and ~4,222 rows
/// respectively, per this plan's own header), so a full-table read on every
/// trip-planning query is the same cost class as the existing per-request
/// reference-data reads this app already does elsewhere (e.g.
/// `queries::list_stanox_crs`), not a new performance concern.
pub async fn fetch_interchange_data(pool: &PgPool) -> Result<InterchangeData> {
    let stanox_rows = crate::data::queries::list_stanox_crs(pool).await?;
    let mut change_time_by_tiploc = HashMap::new();
    let mut tiploc_to_crs = HashMap::new();
    let mut crs_to_tiplocs: HashMap<String, Vec<String>> = HashMap::new();
    for row in &stanox_rows {
        if let Some(minutes) = row.change_time_minutes {
            change_time_by_tiploc.insert(row.tiploc.clone(), minutes);
        }
        tiploc_to_crs.insert(row.tiploc.clone(), row.crs.clone());
        crs_to_tiplocs.entry(row.crs.clone()).or_default().push(row.tiploc.clone());
    }

    let fixed_link_rows: Vec<FixedLinkRow> = sqlx::query_as(
        "SELECT from_crs, to_crs, mode, minutes, valid_from, valid_to, days_mask FROM fixed_links",
    )
    .fetch_all(pool)
    .await?;
    let mut fixed_links_from_crs: HashMap<String, Vec<FixedLink>> = HashMap::new();
    for row in fixed_link_rows {
        fixed_links_from_crs.entry(row.from_crs).or_default().push(FixedLink {
            mode: row.mode,
            to_crs: row.to_crs,
            minutes: row.minutes,
            valid_from: row.valid_from,
            valid_to: row.valid_to,
            days_mask: row.days_mask,
        });
    }

    Ok(InterchangeData {
        change_time_by_tiploc,
        tiploc_to_crs,
        crs_to_tiplocs,
        fixed_links_from_crs,
    })
}

#[cfg(test)]
mod db_tests {
    use super::*;

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for db_tests");
        PgPool::connect(&url).await.expect("connect to test database")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_returns_none_when_nothing_is_published() {
        let pool = connect().await;
        let far_future = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let result = build_connections_for_date(&pool, far_future).await.expect("query succeeds");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_builds_a_real_connection_from_seeded_rows() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTUID1', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTUID1', 1, 'MKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");

        let connections = build_connections_for_date(&pool, date)
            .await
            .expect("query succeeds")
            .expect("rows exist for this date");
        assert!(connections.iter().any(|c| c.uid == "TESTUID1" && c.from_tiploc == "EUSTON"));

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTUID1'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                fetch_interchange_data -- --ignored --test-threads=1`"]
    async fn fetch_interchange_data_reads_real_stanox_crs_and_fixed_links_rows() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes) \
             VALUES ('TEST-IC-STANOX', 'ZZZ', 'ZZZTPL', 'TEST STATION', 1, 7) \
             ON CONFLICT (stanox) DO UPDATE SET change_time_minutes = EXCLUDED.change_time_minutes",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence) \
             VALUES ('WALK', 'ZZZ', 'YYY', 8, '0000', '2359', '1111111', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed fixed_links");

        let data = fetch_interchange_data(&pool).await.expect("query succeeds");
        assert_eq!(data.change_time_by_tiploc.get("ZZZTPL"), Some(&7));
        assert!(data.fixed_links_from_crs.get("ZZZ").is_some_and(|links| links.iter().any(|l| l.to_crs == "YYY")));

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-IC-STANOX'").execute(&pool).await.ok();
        sqlx::query("DELETE FROM fixed_links WHERE from_crs = 'ZZZ'").execute(&pool).await.ok();
    }
}
```

- [ ] **Step 3: Register the module** in `crates/api/src/data/mod.rs`
  (`pub mod trip_planning;`, alongside the existing `pub mod journeys;`
  etc.).

- [ ] **Step 4: Verify**

```bash
cargo build -p api
cargo test -p api -- --ignored --test-threads=1
```

  Expected: builds clean, all three new DB-gated tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/data/trip_planning.rs crates/api/src/data/mod.rs
git commit -m "api: add data::trip_planning, the per-query connections-array and interchange-data build"
```

---

## Self-review notes

- **Spec coverage**: §8 Phase 2's "connections-array extraction" is Task 2's
  `connections.rs`; "interchange-aware connection building" is delivered as
  a shared lookup layer instead of synthetic connections, with the
  correction fully justified in Judgment Call 2 against the real,
  independently re-verified sibling implementation. §7 Open Question 1 is
  answered concretely (Judgment Call 1), not left open, since an
  implementation plan cannot ship a route (Phase 5) against an
  unresolved architecture question.
- **Placeholder scan**: none — every function above has a real
  implementation and real tests; Task 1 Step 5's capacity measurement is
  the one step this plan cannot execute itself (no live delivery available
  to this research pass), and it is written as a concrete, boundable
  follow-up with a named fallback, not an open-ended TBD.
- **Type consistency**: `CallingPointForConnections`/`Connection` in
  `connections.rs` match exactly what `trip_planning.rs`'s
  `CallingPointRow`/`build_connections_for_date` produce and consume;
  `InterchangeData`/`FixedLink` in `interchange.rs` match exactly what
  `fetch_interchange_data` builds.
- **Review Focus**: all five items above have a directly corresponding test
  in Task 2 (`a_single_calling_point_schedule_produces_no_connections`,
  `a_terminate_point_followed_by_nothing_boardable_contributes_no_connection`,
  `sibling_tiplocs_is_empty_for_a_tiploc_with_no_crs`,
  `a_link_excluded_by_day_mask_is_not_returned`/`a_link_excluded_by_time_window_is_not_returned`,
  `only_the_shortest_of_several_valid_rows_to_the_same_destination_is_returned`).
