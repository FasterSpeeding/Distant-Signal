# Plan: Option B's Live Consumer, in Shadow Mode — Implementation

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1–3 are independent, small, foundational extractions/additions and
> should land first, in any order, each as its own commit.** **Tasks 4–6
> (the `crates/api` surface) must land before Tasks 7–13 that call the
> routes they add.** **Task 7 (`schedule-reference`) and Tasks 8–13
> (`crates/full-coverage-consumer`) can proceed in parallel once Tasks 1–6
> are in, but Task 13 (main-loop wiring) depends on every one of Tasks
> 8–12's modules existing.** **Task 14 (`aggregator`) only depends on Task
> 6.** **Task 15 (deployment) depends on the crate existing (Task 8) and
> compiling.**

**Goal:** implement
`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`
("the design doc") end to end, per the task brief's binding file-ownership
boundary: this plan owns everything in the design doc **except** the
`station_full_coverage_samples` table and its `POST`/`GET
/private/station-full-coverage-samples` routes, which belong to the
parallel per-station-full-coverage-stats chain
(`docs/superpowers/plans/2026-09-04-per-station-full-coverage-stats-plan.md`,
worktree `worktree-per-station-full-coverage`). This plan's own
`full-coverage-consumer` crate calls that endpoint as an HTTP client only —
no route-handler or migration code for it is created here (see Non-goals).

**Architecture:** one new lib crate (`crates/trust-schema`, extracted from
`crates/trust-consumer`, Task 1), one small shared utility added to
`crates/common` (Task 2), one small pure-data addition to `crates/schedule-query`
(Task 3), two new `crates/api` tables + their private ingest routes (Tasks
5–6), a second CIF-reading responsibility added to `crates/schedule-reference`
(Task 7), one new binary crate `crates/full-coverage-consumer` (Tasks 8–13),
`crates/aggregator`'s placeholder call site replaced with a real direct-SQL
query (Task 14, **not** an HTTP call — see this task's own correction of the
design doc's sketch), and a new Helm Deployment + Dockerfile + CI/compose
wiring (Task 15).

**Tech stack:** Rust (axum + sqlx runtime-checked queries for `crates/api`,
matching its existing convention; `rdkafka` for the new consumer, matching
`trust-consumer`'s existing convention; `reqwest` + `common::oauth_client`
for every HTTP producer/consumer role, matching every existing poller).

**Design doc:**
`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md` — its
Decisions section is authoritative for every shape below; this plan does not
repeat the reasoning, only the concrete steps, and calls out explicitly (see
"Corrections to the design doc's own sketches," immediately below) the few
places a concrete step departs from the design doc's own "sketch, not final"
code, all toward matching this repo's actual established precedent more
closely, verified by reading the real files, never a substantive redesign.

---

## Corrections to the design doc's own sketches (read this before Task 4)

The design doc's own sketches were written without re-reading every file
they touch line-by-line. Three real mismatches were found while grounding
this plan against the actual code, each resolved below the same way the
per-station-full-coverage-stats plan resolved its own `ingest.rs` mismatch —
toward the repo's real convention, never toward a new one:

1. **`aggregator` does not call `api` over HTTP at all — it holds its own
   direct `sqlx::PgPool` against the same Postgres database.** Confirmed by
   reading `crates/aggregator/src/main.rs` (connects via
   `PgPoolOptions::new().connect(&config.database_url)`) and
   `crates/aggregator/src/queries.rs` (`load_incidents`, `load_station_samples`,
   `load_custom_lines` are all plain `sqlx::query`/`query_as` calls against
   that pool, not `reqwest` calls) — `aggregator`'s `Cargo.toml` has no
   `reqwest` or `oauth_client` dependency at all, structurally confirming
   this isn't an oversight. The design doc's Decision 3 sketch
   (`queries::fetch_full_coverage_stats` as a `reqwest`/`OAuthTokenCache`
   call against `GET /private/full-coverage-stats`) is corrected here: Task
   14 adds a **direct SQL query function** to `crates/aggregator/src/queries.rs`,
   mirroring `load_station_samples`'s own shape exactly. No `GET
   /private/full-coverage-stats` route is needed for `aggregator`'s own
   consumption — see point 2.
2. **Every existing `/private/*` ingest route falls into one of two
   distinct GET shapes, and Decision 3's `full_coverage_line_stats` table
   needs the second one, not the first.** `GET /private/stanox-crs`
   (`queries::list_stanox_crs`) returns the **actual current table**, read
   by a real cross-service caller (`trust-consumer`). `GET /private/incidents`,
   `/private/station-samples`, `/private/tocs`, `/private/tfl-line-status`,
   `/private/schedule-feed`-adjacent routes instead return only a
   `LastFetchedResponse { fetched_at }` freshness timestamp
   (`crates/api/src/routes/ingest.rs:70-112,228-234`) — every one of those
   tables' *real* consumer is `aggregator`'s own direct SQL, exactly like
   point 1 above; the GET-last-fetched route exists purely so a poller can
   skip an already-fresh first fetch at startup
   (`common::ingest::time_until_next_poll`'s own doc comment), not because
   anything reads the table over HTTP. `full_coverage_line_stats` fits this
   **second** shape (its real reader is `aggregator`'s new direct SQL, Task
   14) — Task 6 gives it a `POST`/`GET-last-fetched` pair, matching
   `/incidents`'s shape, not `/stanox-crs`'s. `schedule_line_population`
   (Task 5) is the opposite case: its real reader
   (`full-coverage-consumer`, a *different service* than its writer,
   `schedule-reference`) genuinely needs the actual rows over HTTP, so it
   gets the `/stanox-crs`-shaped pair instead (Task 5's own note).
3. **`common::next_rail_day_boundary`-equivalent logic does not exist as a
   reusable function today — it's `fn` (private), not `pub fn`, inside
   `crates/aggregator/src/aggregation.rs:213-227`.** The design doc's
   Decision 2e says "reusing this repo's own existing rail-day boundary
   convention... `aggregation.rs:214-227`'s `next_rail_day_boundary`" as if
   this were already a shared, importable utility — it is not, and
   `full-coverage-consumer` (a different crate) cannot reach a private `fn`
   in `aggregator`. Task 2 extracts it into `common::rail_day::next_rail_day_boundary`
   (a small, behavior-preserving move, the same low-risk shape as the
   `schedule-ingest`/`schedule-reference` split and this plan's own Task 1),
   used by both `aggregator` (updated call site, Task 2's own regression
   test) and `full-coverage-consumer` (new use, Task 11).

---

## Non-goals — binding

- **No migration, query function, or route handler for
  `station_full_coverage_samples` / `POST`/`GET /private/station-full-coverage-samples`.**
  Owned entirely by
  `docs/superpowers/plans/2026-09-04-per-station-full-coverage-stats-plan.md`
  (its Tasks 1, 2, 5). This plan's `full-coverage-consumer` (Task 12) is
  only ever an HTTP **client** of that endpoint, via
  `common::StationFullCoverageSample` (a type that plan adds to
  `crates/common`, not this one).
- **Real merge-order dependency, stated plainly, not silently assumed**:
  Task 12 of this plan cannot be exercised end-to-end (nor can its
  `#[ignore]`-gated tests that POST against a live `api` pass) until (a)
  `common::StationFullCoverageSample` exists (per-station plan's Task 1)
  and (b) `POST /private/station-full-coverage-samples` exists and is
  gated by a real internal-OAuth group (per-station plan's Tasks 2, 4, 5).
  If this plan's Task 12 is implemented first, its own unit tests (pure
  grouping/asymmetry logic, no HTTP) still pass; only its
  `queries::post_station_full_coverage_samples` HTTP call and any
  DB-backed integration test that exercises it against a real `api` must
  wait for that merge. Task 4 below has the same kind of dependency for
  the **config field name** `internal_oauth_group_full_coverage` — see
  that task's own note.
- **No flipping `LineDefinition.full_coverage_enabled` for any real
  catalogued line.** Every `lines/*.toml` entry stays untouched; this
  plan's own Task 14 verification step greps to confirm this held after
  every other task landed.
- **No RDM Kafka access/licensing work.** Treated as already available per
  the design doc's own scope (its Open Question 1 is a real but
  operational risk, not something this plan resolves).
- **No comparison/dashboard UI for shadow-mode data.** A human queries
  `full_coverage_line_stats`/`station_full_coverage_samples` directly, or a
  future, separate task builds one.
- **No wiring `schedule-query` into `trust-consumer`'s own live
  pin-matching (`matching.rs`).** Unrelated to this plan's scope.

## Global Constraints

- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests) after every
  task; `DATABASE_URL=<url> cargo test -p api -p aggregator --
  --ignored --test-threads=1` for every DB-backed test this plan adds
  (mirrors `.github/workflows/ci.yml:215-216`'s exact invocation — no new
  CI job needed; `aggregator` currently has **zero** `#[ignore]`-gated
  tests, confirmed by `grep -rln '#\[ignore\]' crates/aggregator/` — Task
  14's test is the first, following `crates/api/src/routes/station_stats.rs::db_tests`'s
  established convention as its structural precedent, since that's the
  only real precedent in this repo, not a same-crate one).
- **`trust-schema` extraction (Task 1) is behavior-preserving for
  `trust-consumer` — non-negotiable.** `trust-consumer`'s existing test
  suite must pass **unmodified in assertions** (only import paths change)
  after the move. This is the same bar this session already applied to
  `stats_from_departures`/`common::compute_sample_stats` — pure code
  motion, zero logic changes, verified by running the full suite before
  and after and diffing pass/fail, not just "it compiles."
  `trust-consumer`'s own Cargo.toml dependency list shrinks by whatever
  becomes exclusively `trust-schema`'s (none of `schema.rs`/`dedup.rs`/
  `journey.rs`'s own dependencies — `serde`, `sha2` — are used elsewhere in
  `trust-consumer`, confirmed by grep in Task 1 itself before removing
  them).
- **Wire field naming.** Every new private ingest payload
  (`common`-defined or ad hoc per-crate) uses snake_case Rust field names,
  matching `StationSample`/`StanoxCrsRecord`'s own established convention
  (no `#[serde(rename_all = "camelCase")]` on any `/private/*` payload —
  that's reserved for `render.rs`'s hand-built *public* JSON only, per its
  own module doc). The design doc's illustrative camelCase JSON sketches
  (e.g. `lineId`, `avgDelayMinutes` in its POST bodies) are corrected here
  the same way the per-station plan corrected its own sketch's `resolvedAt`.
- **File scope, backend.** New: `crates/trust-schema/`,
  `crates/full-coverage-consumer/`, two `crates/api/migrations/*.sql` files,
  `docker/full-coverage-consumer.Dockerfile`,
  `charts/distant-signal/templates/full-coverage-consumer-deployment.yaml`.
  Modified: `Cargo.toml` (workspace members),
  `crates/trust-consumer/{Cargo.toml,src/main.rs}` (drop the three moved
  files, add `trust-schema` dependency, fix imports),
  `crates/common/src/lib.rs` (new `rail_day` module),
  `crates/schedule-query/src/records.rs` (new `LinePopulationEntry`),
  `crates/schedule-reference/{Cargo.toml,src/main.rs,src/config.rs}`,
  `crates/api/src/data/{config.rs,queries.rs}`, `crates/api/src/app.rs`,
  `crates/api/src/routes/ingest.rs`, `crates/aggregator/{src/main.rs,src/queries.rs,src/config.rs}`,
  `charts/distant-signal/values.yaml`, `.github/workflows/containers.yml`,
  `docker-compose.yml`. Colocated `ServiceArguments { .. }` test-fixture
  literals gain the new config field the same way the per-station plan's
  Task 4 Step 3 documents (see Task 4 below).
- **No CI system-package change needed for `rdkafka` in the new crate.**
  `.github/workflows/ci.yml:66-68` already installs the `libsasl2`/build
  packages `docker/trust-consumer.Dockerfile` needs, workspace-wide,
  because `--all-features`/`--workspace` builds already compile
  `trust-consumer`. Adding a second `rdkafka`-dependent crate to the same
  workspace needs no new CI step.

---

## Task 1: Extract `crates/trust-schema` from `crates/trust-consumer` (behavior-preserving refactor)

**Files:** create `crates/trust-schema/{Cargo.toml,src/lib.rs,src/schema.rs,src/dedup.rs,src/journey.rs}`;
modify `Cargo.toml` (workspace members), `crates/trust-consumer/Cargo.toml`,
`crates/trust-consumer/src/main.rs` and every file inside
`crates/trust-consumer/src/` that references `schema::`/`dedup::`/`journey::`.

Independent of every other task. Lands first, isolated, its own commit(s),
so nothing downstream is built against code that later moves out from under
it.

**Scope, per the design doc's own flagged Open Question 6 ("hasn't been
scoped as its own task")** — resolved here concretely, having now read all
three files in full (this plan's grounding pass, above):

- `schema.rs` (parsing, `TrustMessage`/`Activation`/`Movement`/
  `Cancellation`/`ChangeOfOrigin`/`ChangeOfIdentity`, `parse_batch`) moves
  **verbatim**, including its full test module. No caller-specific
  assumption exists in this file at all — it's pure wire-format parsing.
- `dedup.rs` (`dedup_key`) moves **verbatim**, including its full test
  module. Pure, stateless, no caller assumption.
- `journey.rs` (`DerivedState`, `apply_movement`, `apply_cancellation`,
  `variation_to_minutes`) moves **verbatim, with no generalization
  needed** — confirmed by reading it closely: `DerivedState` is already
  caller-keyed (it carries no `train_id`/UID of its own; the caller's own
  map, whatever it's keyed by, supplies "the previous state" and receives
  "the new state" back). `trust-consumer` keys its copy
  `train_id -> DerivedState` (`process::ProcessorState.last_derived`);
  `full-coverage-consumer` will key its own copy `(line_id, uid) ->
  DerivedState` (Task 10) — both are just "a map from this caller's own
  identity concept to the last `DerivedState`," which `apply_movement`/
  `apply_cancellation`'s existing `&DerivedState -> DerivedState` shape
  already supports with zero change. This closes the design doc's Open
  Question 6 concretely: **no generalization of `journey.rs` is needed**,
  only its move.
- **What does NOT move**: `matching.rs` (`resolve_origin_departure`,
  `PendingPin`) and `process.rs` (`ProcessorState`, `Reference`,
  `apply_reference_reload`, `run_once`) — these carry `trust-consumer`'s
  own pin-matching semantics (the design doc's Decision 1 rationale for a
  separate crate) and stay put, now calling into `trust_schema::` instead
  of local `schema::`/`dedup::`/`journey::` modules.

- [ ] **Step 1: Create the crate**

```toml
# crates/trust-schema/Cargo.toml
[package]
name = "trust-schema"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
sha2 = "0.11.0"
tracing = "0.1.44"
```

(Version pins copied verbatim from `crates/trust-consumer/Cargo.toml`'s
current values for these same four dependencies — confirm they're still
current at implementation time; don't silently downgrade.)

Add `"crates/trust-schema"` to the workspace `Cargo.toml`'s `members`
list, alongside `"crates/trust-consumer"`.

- [ ] **Step 2: Move the three files verbatim**

```bash
git mv crates/trust-consumer/src/schema.rs crates/trust-schema/src/schema.rs
git mv crates/trust-consumer/src/dedup.rs crates/trust-schema/src/dedup.rs
git mv crates/trust-consumer/src/journey.rs crates/trust-schema/src/journey.rs
```

`journey.rs`'s one internal reference, `use crate::schema::Movement;`,
becomes `use crate::schema::Movement;` unchanged (both files now live in
the same crate, `trust-schema`) — no edit needed there.

`src/lib.rs`:

```rust
//! Pure TRUST movement-feed message parsing, dedup-key derivation, and
//! journey-state derivation -- extracted from `crates/trust-consumer` so
//! `crates/full-coverage-consumer` (a second, independent Kafka consumer
//! against the same feed) doesn't duplicate ~300 lines of already-tested
//! envelope parsing. Pure code motion, no behavior change -- see
//! docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md Task 1
//! and docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
//! Decision 1 for the extraction rationale. No I/O, no `tokio`, no
//! `rdkafka` dependency -- both real callers own their own Kafka
//! plumbing; this crate only understands the message bytes once they're
//! already `&str`.

pub mod dedup;
pub mod journey;
pub mod schema;
```

- [ ] **Step 3: Update `trust-consumer` to depend on `trust-schema`**

`crates/trust-consumer/Cargo.toml`: add `trust-schema = { path =
"../trust-schema" }`; remove `sha2` (only `dedup.rs` used it — confirm via
`grep -rn sha2 crates/trust-consumer/src/` returns nothing else first);
keep `serde`/`serde_json` (used elsewhere in `trust-consumer`, e.g.
`stanox_crs.rs`, `queries.rs`).

`crates/trust-consumer/src/main.rs`: remove `mod schema; mod dedup; mod
journey;`; every other file's `use crate::schema::...` / `use
crate::dedup::...` / `use crate::journey::...` becomes `use
trust_schema::schema::...` / `use trust_schema::dedup::...` / `use
trust_schema::journey::...` (confirmed call sites via `grep -rln
'crate::\(schema\|dedup\|journey\)::' crates/trust-consumer/src/` at
implementation time — expected in `process.rs`, `matching.rs`, `main.rs`
itself, `eta.rs` per the design doc's own file list).

- [ ] **Step 4: Behavior-preserving verification**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p trust-schema
cargo test -p trust-consumer
```

Every test that existed in `schema.rs`/`dedup.rs`/`journey.rs` before the
move now runs (and passes) under `cargo test -p trust-schema`, with
**identical assertions** — diff the test module bodies against `git show
HEAD~1:crates/trust-consumer/src/schema.rs` (etc.) to confirm zero logic
edits crept in during the move, only `use` path fixes. Every test that
already existed in `process.rs`/`matching.rs`/`main.rs` (which exercise
`journey`/`dedup`/`schema` indirectly) still passes under `cargo test -p
trust-consumer`, with **identical assertions** — if any of them needs an
assertion changed to keep passing, stop and diagnose; that means the move
altered behavior, which is the one thing this task must not do.

- [ ] **Step 5: Commit**

```bash
git add -A crates/trust-schema crates/trust-consumer Cargo.toml
git commit -m "Extract crates/trust-schema from trust-consumer (pure code motion, no behavior change)"
```

---

## Task 2: `common::rail_day` — extract the shared rail-day boundary utility

**Files:** modify `crates/common/{Cargo.toml,src/lib.rs}` (or a new
`crates/common/src/rail_day.rs` module, either is fine — match whatever
`common`'s existing module-file-vs-inline convention favors for a
same-sized addition, check first), `crates/aggregator/{Cargo.toml,src/aggregation.rs}`.

Independent. Small, low-risk, same "pure code motion" bar as Task 1 — see
Correction 3 above for why this is needed at all (the design doc assumed a
`pub fn` that doesn't exist).

- [ ] **Step 1: Add `chrono-tz` to `common`**

`crates/common/Cargo.toml`: add `chrono-tz = "0.10"` (same version
`crates/aggregator/Cargo.toml` already pins).

- [ ] **Step 2: Move the function, verbatim logic, into `common`**

```rust
// crates/common/src/rail_day.rs -- new file
//! The rail-day boundary this app already uses for incident staleness
//! (`crates/aggregator/src/aggregation.rs`'s original site) and, as of
//! docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
//! Decision 2e, for full-coverage Resolved/Pending gating too. Extracted
//! here (rather than left `aggregator`-private) specifically so a second
//! crate can share it without duplicating the Europe/London 02:00-cutoff
//! DST-transition-safe logic -- pure code motion, no behavior change; see
//! docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md Task 2.

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};

/// The next rail-day boundary (02:00 Europe/London) strictly after
/// `at`. If `at`'s local time is already past 02:00, that's the current
/// day's 02:00; otherwise it's the next calendar day's 02:00.
///
/// UK clocks change exactly at the 01:00/02:00 boundary in both directions,
/// so local 02:00 itself is never ambiguous or missing on a transition day.
pub fn next_rail_day_boundary(at: DateTime<Utc>) -> DateTime<Utc> {
    let local = at.with_timezone(&chrono_tz::Europe::London);
    let boundary_time = NaiveTime::from_hms_opt(2, 0, 0).expect("2:00:00 is a valid time");

    let boundary_date = if local.time() < boundary_time {
        local.date_naive()
    } else {
        local.date_naive() + Duration::days(1)
    };
    let boundary_naive = boundary_date.and_time(boundary_time);

    match chrono_tz::Europe::London.from_local_datetime(&boundary_naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        other => panic!(
            "unexpected {other:?} resolving rail-day boundary {boundary_naive} in Europe/London; \
             02:00 local should never be ambiguous or missing"
        ),
    }
}
```

Copy `aggregation.rs`'s existing test module for this function verbatim
into `rail_day.rs`'s own `#[cfg(test)] mod tests` (the plain-midweek,
just-before-0200, just-after-0200, spring-forward test cases already
listed at `aggregation.rs:2140-2192` region) — same "identical assertions"
bar as Task 1.

- [ ] **Step 3: Repoint `aggregator`'s call site**

`crates/aggregator/src/aggregation.rs`: delete the private `fn
next_rail_day_boundary` and its own test module; replace its one call site
with `common::rail_day::next_rail_day_boundary(first_seen_at)`. Remove
`chrono_tz` from `crates/aggregator/Cargo.toml` only if nothing else in
that crate uses it directly (`grep -rn chrono_tz crates/aggregator/src/`
first — if this was its only use, drop the now-unused direct dependency;
`common` re-exporting it transitively is fine either way).

- [ ] **Step 4: Verify and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p common
cargo test -p aggregator
```

```bash
git add crates/common crates/aggregator Cargo.toml
git commit -m "Extract common::rail_day::next_rail_day_boundary, shared by aggregator and (soon) full-coverage-consumer"
```

---

## Task 3: `schedule-query` — add `LinePopulationEntry` (small, pure, no I/O)

**Files:** modify `crates/schedule-query/src/records.rs`.

Independent. A tiny addition to the existing pure library (no `tokio`, no
`reqwest`, no database — Task 3 of the first-slice plan's own Non-goals
still holds: this stays a pure data/parsing crate). Needed because both
`schedule-reference` (writer, Task 7) and `full-coverage-consumer` (reader,
Task 9) need one shared shape for "one UID's calling points, as published
over the wire" — adding it here (next to `CallingPoint`, which already
derives `Serialize`/`Deserialize`) avoids either (a) making `common`
depend on `schedule-query` or vice versa, or (b) two independent, silently
divergent copies of the same two-field struct in two different crates.

- [ ] **Step 1: Add the type**

Directly after `CallingPoint` (`records.rs:137-152` region):

```rust
/// One UID's resolved calling points, as published over the wire between
/// `crates/schedule-reference` (writer, via `POST
/// /private/schedule-line-population`) and `crates/full-coverage-consumer`
/// (reader, via `GET /private/schedule-line-population`) -- see
/// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
/// Decision 2a/2b. Deliberately NOT `ResolvedSchedule` itself (which
/// carries `stp_indicator`/`cancelled`, neither of which either producer
/// or consumer needs on the wire -- `schedules_touching` already filters
/// to non-cancelled results before this type is ever constructed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinePopulationEntry {
    pub uid: String,
    pub calling_points: Vec<CallingPoint>,
}

impl From<ResolvedSchedule> for LinePopulationEntry {
    fn from(resolved: crate::resolve::ResolvedSchedule) -> Self {
        Self {
            uid: resolved.uid,
            calling_points: resolved.calling_points,
        }
    }
}
```

(Adjust the `impl From` block's exact placement/import if `ResolvedSchedule`
being in `resolve.rs` rather than `records.rs` makes a direct `impl`
awkward from this file — an inherent `impl` needs to live in the crate
that owns at least one of the two types, either file works since both are
in this crate; pick whichever avoids a circular `use`.)

- [ ] **Step 2: Unit test**

```rust
#[test]
fn line_population_entry_from_resolved_schedule_drops_stp_fields() {
    let resolved = ResolvedSchedule {
        uid: "C11052".to_string(),
        stp_indicator: StpIndicator::Permanent,
        cancelled: false,
        calling_points: vec![/* one synthetic CallingPoint */],
    };
    let entry: LinePopulationEntry = resolved.clone().into();
    assert_eq!(entry.uid, "C11052");
    assert_eq!(entry.calling_points, resolved.calling_points);
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p schedule-query --all-features
cargo test -p schedule-query
```

```bash
git add crates/schedule-query/src/records.rs
git commit -m "Add schedule_query::LinePopulationEntry, the schedule-reference<->full-coverage-consumer wire shape"
```

---

## Task 4: `crates/api` — `internal_oauth_group_full_coverage` config field

**Files:** modify `crates/api/src/data/config.rs`, `crates/api/src/app.rs`,
`charts/distant-signal/{values.yaml,templates/api-deployment.yaml}`, and
every colocated `ServiceArguments { .. }` test-fixture literal (same list
the per-station plan's own Task 4 Step 3 already enumerates: `auth.rs`,
`routes/{lines,line_status,chatbot,departures,train,station_stats}.rs`).

**Real merge-order note, stated plainly**: the per-station-full-coverage-stats
plan's own Task 4 adds a config field with this **exact same name**
(`internal_oauth_group_full_coverage`, default `svc-full-coverage-consumer`)
for its own two routes. Both plans independently arrived at the same name
because both cite the design doc's Decision 5 ("the same group/credential
that gates `/private/full-coverage-stats`, since both endpoints are written
by the same service"). **If that plan's Task 4 has already landed by the
time this task is implemented** (check first: `grep -n
internal_oauth_group_full_coverage crates/api/src/data/config.rs`), this
task becomes "add three more `(prefix, method)` entries in
`build_internal_oauth_routes` reusing the already-existing field," not "add
the field" — do the field-existence check before writing any code, and
skip Step 1 entirely if it's already there. If this task lands first, the
per-station plan's own Task 4 becomes the same kind of no-op on the field
(its Step 1 becomes "confirm it's already there") when it merges. Either
merge order is safe **because the field name and semantics are identical**,
not just similar — this was verified by reading both plans' own reasoning,
not assumed.

- [ ] **Step 1 (skip if already present): add the config field**

Directly after `internal_oauth_group_schedule_reference`
(`config.rs:91-92`):

```rust
/// Gates every route this plan's `full-coverage-consumer` and the
/// separate per-station-full-coverage-stats chain's own producer surface
/// use -- one service, one credential, several endpoints it may read or
/// write. See
/// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
/// Decision 5.
#[arg(long, env, default_value = "svc-full-coverage-consumer")]
pub internal_oauth_group_full_coverage: String,
```

- [ ] **Step 2: Wire the startup guard (skip if already present)**

`AppState::init`'s empty-value guard loop (`app.rs:222-268` region): add
`("internal_oauth_group_full_coverage", &config.internal_oauth_group_full_coverage)`
if not already there.

- [ ] **Step 3: Update every colocated `ServiceArguments { .. }` fixture
      (skip any file where it's already been added)**

Same file list and same "follow whatever that file already does for its
sibling `internal_oauth_group_*` fields" rule as the per-station plan's
Task 4 Step 3.

- [ ] **Step 4: Helm wiring (skip if already present)**

`charts/distant-signal/values.yaml`, inside `api.internalOauth.groups`
(`:432-440` region — confirm exact current line numbers, they may have
shifted): add `fullCoverage: svc-full-coverage-consumer` if not already
there. `charts/distant-signal/templates/api-deployment.yaml`: add
`INTERNAL_OAUTH_GROUP_FULL_COVERAGE` env entry mirroring
`INTERNAL_OAUTH_GROUP_SCHEDULE_REFERENCE`'s if not already there.

- [ ] **Step 5: Test, build, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
helm template charts/distant-signal > /dev/null
```

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs crates/api/src/auth.rs \
        crates/api/src/routes/{lines,line_status,chatbot,departures,train,station_stats}.rs \
        charts/distant-signal/templates/api-deployment.yaml charts/distant-signal/values.yaml
git commit -m "Add internal_oauth_group_full_coverage (or confirm/extend it if the per-station chain already added it)"
```

---

## Task 5: `crates/api` — `schedule_line_population` migration + queries + routes

**Files:** create `crates/api/migrations/YYYYMMDDHHMMSS_schedule_line_population.sql`;
modify `crates/api/src/data/queries.rs`, `crates/api/src/routes/ingest.rs`,
`crates/api/src/app.rs`.

Depends on Task 3 (not directly — `api` never depends on `schedule-query`;
it stores the population as opaque `JSONB`, deserialized by
`full-coverage-consumer` on read, per Task 3's `LinePopulationEntry` — see
Step 1's schema comment for why `api` doesn't need to understand the
shape). Depends on Task 4 (needs the OAuth group to exist for the GET
route). This is the **`GET`-returns-actual-rows shape** (Correction 2
above) — two different callers, two different groups, mirroring
`/stanox-crs`'s established split exactly.

- [ ] **Step 1: Migration**

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_schedule_line_population.sql
-- ---------------------------------------------------------------------
-- One row per (line_id, service_date): a shadow-computed line's full CIF
-- SCHEDULE population for one rail day, published by schedule-reference
-- (POST, its own existing writer credential) and read by
-- full-coverage-consumer (GET, a new credential) to build its in-memory
-- correlation index. `population` is opaque JSONB here -- a
-- Vec<schedule_query::LinePopulationEntry> -- `api` never deserializes
-- it, only stores/relays it, the same "opaque blob" posture
-- station_full_coverage_samples.stats already established for a
-- different table. See
-- docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
-- Decision 2a/2b and
-- docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md Task 5.
-- ---------------------------------------------------------------------

CREATE TABLE schedule_line_population (
    line_id      TEXT        NOT NULL,
    service_date DATE        NOT NULL,
    population   JSONB       NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (line_id, service_date)
);
```

Apply locally, confirm the table exists, commit
(`git commit -m "Add schedule_line_population table (no writer/reader yet)"`)
— same isolated-migration-commit convention every prior plan in this
lineage uses.

- [ ] **Step 2: `queries.rs` — upsert + list**

Directly after `upsert_stanox_crs`/`list_stanox_crs` (`queries.rs:596-670`
region), same shape, but storing/returning an **opaque `serde_json::Value`**
for `population` (never a concrete Rust type — `api` doesn't depend on
`schedule-query`, deliberately, since it's a leaf pure-parsing crate with
no reason to become a dependency of the storage layer):

```rust
/// Upserts one line's population for one service date -- wholesale
/// replaces any existing row for that `(line_id, service_date)` (a fresh
/// CIF read supersedes the prior one entirely, never merged). `population`
/// is stored opaquely; `api` never deserializes it into
/// `schedule_query::LinePopulationEntry` -- only `schedule-reference`
/// (writer) and `full-coverage-consumer` (reader) need that shape.
pub async fn upsert_schedule_line_population(
    pool: &PgPool,
    line_id: &str,
    service_date: chrono::NaiveDate,
    population: &serde_json::Value,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_line_population (line_id, service_date, population, updated_at)
        VALUES ($1, $2, $3, now())
        ON CONFLICT (line_id, service_date) DO UPDATE SET
            population = EXCLUDED.population,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(line_id)
    .bind(service_date)
    .bind(population)
    .execute(pool)
    .await?;
    Ok(())
}

/// Reads one line's population for one service date, if published.
/// `None` when `full-coverage-consumer` reloads before `schedule-reference`
/// has ever published that day's population yet (a real, expected startup
/// race, not an error -- the caller treats it the same as "empty
/// population," per Decision 2e's own Pending semantics).
pub async fn get_schedule_line_population(
    pool: &PgPool,
    line_id: &str,
    service_date: chrono::NaiveDate,
) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT population FROM schedule_line_population WHERE line_id = $1 AND service_date = $2",
    )
    .bind(line_id)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get("population")).transpose().map_err(Into::into)
}
```

- [ ] **Step 3: Routes**

`ingest.rs`'s `router()`, one more `.route(...)`:

```rust
.route(
    "/schedule-line-population",
    axum::routing::get(get_schedule_line_population).post(post_schedule_line_population),
)
```

Handlers, query-string-parameterized (this is the one private route in
this file that isn't a bare batch POST — mirrors any existing
`Query<...>`-extracting handler in `crates/api/src/routes/` for the
pattern, e.g. check `routes/departures.rs` or `routes/train.rs` for this
crate's own `axum::extract::Query` idiom before writing this from scratch):

```rust
#[derive(Deserialize)]
struct SchedulePopulationParams {
    line_id: String,
    service_date: chrono::NaiveDate,
}

#[derive(Deserialize)]
struct SchedulePopulationBody {
    line_id: String,
    service_date: chrono::NaiveDate,
    population: serde_json::Value,
}

async fn post_schedule_line_population(
    State(app): State<App>,
    Json(body): Json<SchedulePopulationBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    queries::upsert_schedule_line_population(&app.database, &body.line_id, body.service_date, &body.population)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

async fn get_schedule_line_population(
    State(app): State<App>,
    axum::extract::Query(params): axum::extract::Query<SchedulePopulationParams>,
) -> Result<Json<Option<serde_json::Value>>, (StatusCode, String)> {
    let population = queries::get_schedule_line_population(&app.database, &params.line_id, params.service_date)
        .await
        .map_err(internal_error)?;
    Ok(Json(population))
}
```

`app.rs`'s `build_internal_oauth_routes`, two entries — **different
groups per method**, the `/stanox-crs` split:

```rust
(
    "/schedule-line-population",
    Method::POST,
    vec![config.internal_oauth_group_schedule_reference.clone()],
),
(
    "/schedule-line-population",
    Method::GET,
    vec![config.internal_oauth_group_full_coverage.clone()],
),
```

- [ ] **Step 4: `#[ignore]`-gated DB tests**

New tests in `ingest.rs`'s (or a new) `db_tests` module, mirroring
`station_stats.rs::db_tests`'s seed/assert/delete convention, using a
reserved fixture `line_id` (e.g. `"ZTEST"`, matching the `Z…` reserved-CRS
convention's spirit for a non-CRS key):

- `POST` a population, then `GET` the same `(line_id, service_date)` →
  round-trips the exact JSON.
- `POST` twice for the same `(line_id, service_date)`, different
  `population` the second time → wholesale replaced, not merged (assert
  via a direct `SELECT`).
- `GET` for a `(line_id, service_date)` never posted → `Json(None)`, not a
  404 or an error.

Delete the fixture row unconditionally at the end.

- [ ] **Step 5: Test, lint, build, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api schedule_line_population -- --ignored --test-threads=1
```

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs crates/api/src/app.rs
git commit -m "Add POST/GET /private/schedule-line-population"
```

---

## Task 6: `crates/api` — `full_coverage_line_stats` migration + queries + routes

**Files:** create `crates/api/migrations/YYYYMMDDHHMMSS_full_coverage_line_stats.sql`;
modify `crates/api/src/data/queries.rs`, `crates/api/src/routes/ingest.rs`,
`crates/api/src/app.rs`.

Depends on Task 4. Independent of Task 5. This is the
**`GET`-returns-last-fetched shape** (Correction 2 above) — the real reader
is `aggregator`'s direct SQL (Task 14), not this route.

- [ ] **Step 1: Migration**

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_full_coverage_line_stats.sql
-- ---------------------------------------------------------------------
-- One row per line -- a live snapshot (mirrors LineStatus's own "current
-- state, not history" shape), not an append log; the existing
-- line_status_daily_coverage_stats/line_status_half_hourly_coverage_stats
-- tables remain the historical rollups this table is NOT a substitute
-- for. `service_date` is a real freshness guard: a stale row (a producer
-- outage spanning a rail-day rollover) is detected and treated as Pending
-- on read by Task 14's own service_date == today filter, never served as
-- a silently-aging Available snapshot. See
-- docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
-- Decision 3.
-- ---------------------------------------------------------------------

CREATE TABLE full_coverage_line_stats (
    line_id           TEXT             PRIMARY KEY,
    service_date      DATE             NOT NULL,
    availability      TEXT             NOT NULL, -- 'pending' | 'available'
    total             INT              NOT NULL DEFAULT 0,
    delayed           INT              NOT NULL DEFAULT 0,
    cancelled         INT              NOT NULL DEFAULT 0,
    skipped           INT              NOT NULL DEFAULT 0,
    avg_delay_minutes DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at        TIMESTAMPTZ      NOT NULL DEFAULT now()
);
```

Apply locally, confirm, commit in isolation.

- [ ] **Step 2: `common::FullCoverageLineStatsRow`**

Add to `crates/common/src/lib.rs`, next to `StationFullCoverageSample`'s
future location (or wherever the per-station plan's own Task 1 lands it —
check first, place this one nearby for discoverability, not required to be
adjacent):

```rust
/// One `full_coverage_line_stats` row -- the per-line counterpart of
/// `common::StationFullCoverageSample` (owned by a different plan). Posted
/// by `full-coverage-consumer` to `POST /private/full-coverage-stats`;
/// read by `aggregator` via a direct SQL query
/// (`crates/aggregator/src/queries.rs::fetch_full_coverage_line_stats`,
/// NOT over HTTP -- see this plan's Correction 1). snake_case field
/// names -- a private producer payload between this app's own crates, not
/// a public wire type (see this plan's Global Constraints).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullCoverageLineStatsRow {
    pub line_id: String,
    pub service_date: chrono::NaiveDate,
    pub availability: String, // "pending" | "available"
    pub stats: SampleStats,
}
```

(`stats: SampleStats` rather than four separate flat fields —
`SampleStats` already has `total`/`delayed`/`cancelled`/`skipped`/
`avg_delay_minutes` with the exact right shape and already derives
`Serialize`/`Deserialize`; reusing it here avoids a parallel, field-for-field
duplicate struct. The SQL table stays flat columns, per Step 1 — the
mapping from `SampleStats` to/from four columns happens in `queries.rs`,
Step 3, the same "typed in Rust, flat in SQL" split `StationSample`/
`StationDeparture` already uses elsewhere in this codebase.)

- [ ] **Step 3: `queries.rs` — upsert + last-fetched**

```rust
/// Upserts one line's full-coverage stats row -- wholesale replaces any
/// existing row for that `line_id` (a live snapshot, never merged/append).
pub async fn upsert_full_coverage_line_stats(
    pool: &PgPool,
    rows: &[common::FullCoverageLineStatsRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO full_coverage_line_stats
                (line_id, service_date, availability, total, delayed, cancelled, skipped, avg_delay_minutes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
            ON CONFLICT (line_id) DO UPDATE SET
                service_date      = EXCLUDED.service_date,
                availability      = EXCLUDED.availability,
                total             = EXCLUDED.total,
                delayed           = EXCLUDED.delayed,
                cancelled         = EXCLUDED.cancelled,
                skipped           = EXCLUDED.skipped,
                avg_delay_minutes = EXCLUDED.avg_delay_minutes,
                updated_at        = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.line_id)
        .bind(row.service_date)
        .bind(&row.availability)
        .bind(row.stats.total as i32)
        .bind(row.stats.delayed as i32)
        .bind(row.stats.cancelled as i32)
        .bind(row.stats.skipped as i32)
        .bind(row.stats.avg_delay_minutes)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}

pub async fn last_full_coverage_line_stats_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM full_coverage_line_stats")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}
```

- [ ] **Step 4: Routes**

`ingest.rs`'s `router()`:

```rust
.route(
    "/full-coverage-stats",
    axum::routing::get(get_full_coverage_stats_last_fetched).post(post_full_coverage_stats),
)
```

Handlers mirror `post_station_samples`/`get_station_samples_last_fetched`
exactly (both gated by the **same** group, `internal_oauth_group_full_coverage`,
matching `/incidents`'s "same producer, same group, both methods" shape —
not `/stanox-crs`'s split, since both methods here are really "this one
producer's own read-back of its own writes," not two different services).

`app.rs`: one `Method::POST` and one `Method::GET` entry, both
`vec![config.internal_oauth_group_full_coverage.clone()]`.

- [ ] **Step 5: `#[ignore]`-gated DB tests**

Same shape as Task 5 Step 4 (seed/assert/delete against a reserved
`line_id`): POST → GET-last-fetched returns non-null and close to now();
POST twice, different stats → row updated in place (assert via direct
`SELECT`, exactly one row); GET-last-fetched against an empty table →
`null`.

- [ ] **Step 6: Test, lint, build, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api -p common
DATABASE_URL=<url> cargo test -p api full_coverage_line_stats -- --ignored --test-threads=1
```

```bash
git add crates/common/src/lib.rs crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs crates/api/src/app.rs
git commit -m "Add POST/GET(last-fetched) /private/full-coverage-stats and common::FullCoverageLineStatsRow"
```

---

## Task 7: `crates/schedule-reference` — second responsibility (CIF SCHEDULE population publish)

**Files:** modify `crates/schedule-reference/{Cargo.toml,src/main.rs,src/config.rs}`.

Depends on Task 3 (`LinePopulationEntry`), Task 5 (the route it POSTs to).

Per the design doc's Decision 2a (the `ReadWriteOnce`-PVC-driven placement
choice, not re-litigated here) and this plan's own simplification of that
decision's sketch:

**Simplification, stated plainly**: the design doc's own sketch shows
`schedule-reference` itself filtering to `catalogue.lines_configured_for_shadow(&config)`
— implying a *second* line-scoping flag on this crate, duplicating the
design doc's own Decision 4 `shadow_lines` flag that already lives on
`full-coverage-consumer`'s config. This plan does **not** add a second
scoping knob: `schedule-reference` publishes a population for **every**
catalogued line with at least one `tiploc`-bearing station (the same
natural exclusion Decision 4 already names — a line with zero TIPLOCs
trivially produces an empty `schedules_touching` result, harmless to
publish), unconditionally. `full-coverage-consumer`'s own `shadow_lines`
flag (Task 8) is the **only** place "which lines does this deployment
actually care about" is decided — a read-time/reload-time filter on the
consumer side, not a publish-time filter on the producer side. This avoids
two independently-configured scoping flags across two different
Deployments needing to be kept in sync by an operator.

- [ ] **Step 1: New dependencies and config**

`Cargo.toml`: add `schedule-query = { path = "../schedule-query" }`.

`config.rs`: add a `LineCatalogue`-shaped `--lines-dir`/`LINES_DIR` field,
mirroring `crates/aggregator/src/config.rs`'s own existing
`value_parser`-based directory-loading pattern for `lines/*.toml` (read
that file's exact `clap` shape before writing this — don't re-derive the
parser from scratch); add `schedule_line_population_url` (default
`http://api:8080/private/schedule-line-population`).

- [ ] **Step 2: Read the `BS`/`BX`/`LO`/`LI`/`CR`/`LT` record family**

`main.rs`'s `poll_once`, after the existing `TI`/`A` read+POST block —
same delivery, same already-open local files, one more prefixed read:

```rust
let mca_schedule_text = read_prefixed_lines_multi(&delivery.mca_path, &["BS", "BX", "LO", "LI", "CR", "LT"])?;
```

(`read_prefixed_lines` (Step-adjacent, existing) only keeps lines matching
ONE prefix — extend it to accept a slice of prefixes, or add a sibling
`read_prefixed_lines_multi`, whichever keeps `read_prefixed_lines`'s
existing single-prefix callers — the `TI`/`A` reads — untouched. Confirm
by checking every existing call site first.)

```rust
let index = schedule_query::ScheduleIndex::from_text(&mca_schedule_text);
let today = chrono::Utc::now().date_naive(); // schedule-reference has no
    // rail-day concept of its own yet -- publishing against the plain
    // calendar date is deliberate and sufficient here: `schedules_touching`
    // resolves STP overlays per calendar date already, and
    // full-coverage-consumer's OWN rail-day gating (Decision 2e) is what
    // decides Pending/Available, not this publish step.
for line in config.lines.values().filter(|l| l.stations.iter().any(|s| s.tiploc.is_some())) {
    let tiplocs: Vec<&str> = line.stations.iter().filter_map(|s| s.tiploc.as_deref()).collect();
    let resolved = schedule_query::schedules_touching(&index, &tiplocs, today);
    let population: Vec<schedule_query::LinePopulationEntry> =
        resolved.into_iter().map(Into::into).collect();
    let body = serde_json::json!({
        "line_id": line.id,
        "service_date": today,
        "population": population,
    });
    if let Err(err) = post_schedule_line_population(client, &config.schedule_line_population_url, internal_oauth, &body).await {
        tracing::error!(error = ?err, line_id = %line.id, "failed to publish schedule line population; will retry next cycle");
        // deliberately NOT a hard failure for the whole poll_once -- one
        // line's publish failing must not block every other line's, or
        // the existing stanox/crs publish above it in the same function.
    }
}
```

A small local `post_schedule_line_population` helper (`reqwest::Client::post`
+ `common::oauth_client` bearer token, mirroring `common::ingest::post_batch`'s
own shape but for a single-object body rather than a `Vec<T>` batch — reuse
`post_batch`'s internals if it's easily generalized to a non-slice body,
otherwise a small bespoke function is fine, this is a low-risk, low-reuse
corner).

- [ ] **Step 3: Tests**

Unit test (no I/O) for the line-filtering predicate (`.stations.iter().any(...)`)
against a minimal `LineDefinition` fixture with and without any
`tiploc`-bearing station. The CIF-reading path itself is exercised the
same way this crate's existing `read_prefixed_lines` is (a small
`tempfile`-backed fixture asserting the right lines get extracted) — reuse
`schedule-query`'s own already-proven parsing/resolution correctness
(Task 3 of the first-slice plan) rather than re-testing STP resolution
here; this crate's own test only needs to prove it wires the pieces
together, not that `schedules_touching` itself is correct.

- [ ] **Step 4: Test, lint, build, commit**

```bash
cargo fmt --all
cargo clippy -p schedule-reference --all-features
cargo test -p schedule-reference
```

```bash
git add crates/schedule-reference
git commit -m "schedule-reference: publish per-line CIF SCHEDULE population to /private/schedule-line-population"
```

---

## Task 8: `crates/full-coverage-consumer` — crate scaffolding

**Files:** create `crates/full-coverage-consumer/{Cargo.toml,src/{main.rs,config.rs,health.rs}}`;
modify workspace `Cargo.toml`.

Depends on Task 1 (`trust-schema`), Task 4 (config field name, for
reference — this crate's own config just needs the credential values, not
`api`'s config code). Produces a crate that **compiles and runs** (an empty
consume loop is fine at this point) but does no real correlation yet —
Tasks 9–13 fill that in. Matches this plan's own "scaffold first, prove it
boots, then layer logic in" structure, the same shape the design doc's own
directory sketch lays out (Decision 1's tree).

- [ ] **Step 1: `Cargo.toml`**

```toml
[package]
name = "full-coverage-consumer"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.104"
axum = { version = "0.8.9", features = ["http2"] }
chrono = { version = "0.4.45", features = ["serde"] }
clap = { version = "4.6.6", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
metrics = "0.24"
rdkafka = { version = "0.39.0", features = ["cmake-build", "ssl", "sasl"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
schedule-query = { path = "../schedule-query" }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "time"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
trust-schema = { path = "../trust-schema" }

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "test-util"] }
```

(Version pins copied from `trust-consumer`'s and `schedule-reference`'s own
`Cargo.toml`s for the shared dependencies — confirm current at
implementation time.) Add `"crates/full-coverage-consumer"` to the
workspace `members` list.

- [ ] **Step 2: `health.rs` — verbatim copy**

`git show HEAD:crates/trust-consumer/src/health.rs` content, copied
unchanged (module doc's "unlike every existing poller... a persistent
Kafka consumer needs a real connected/disconnected signal" reasoning
applies identically here — this is a second persistent Kafka consumer).

- [ ] **Step 3: `config.rs`**

`clap::Parser` struct, mirroring `trust-consumer/src/config.rs`'s shape:

```rust
#[derive(Debug, Parser)]
pub struct Config {
    // Kafka: brokers/topic/consumer-group/sasl -- REUSES trustConsumer's
    // broker/topic/mechanism values at the Helm layer (Task 15), but
    // still has its own consumer_group default and its own env var
    // names at the crate/binary layer, per Decision 1's "connection vs.
    // group membership" reasoning.
    #[arg(long, env)]
    pub kafka_brokers: String,
    #[arg(long, env)]
    pub kafka_topic: String,
    #[arg(long, env, default_value = "distant-signal-full-coverage-consumer")]
    pub kafka_consumer_group: String,
    #[arg(long, env)]
    pub kafka_sasl_username: String,
    #[arg(long, env)]
    pub kafka_sasl_password: String,
    #[arg(long, env)]
    pub kafka_sasl_mechanism: String,

    // api endpoints
    #[arg(long, env, default_value = "http://api:8080/private/schedule-line-population")]
    pub schedule_line_population_url: String,
    #[arg(long, env, default_value = "http://api:8080/private/full-coverage-stats")]
    pub full_coverage_stats_url: String,
    /// The OTHER chain's own endpoint -- see this plan's Non-goals for the
    /// merge-order dependency this URL implies.
    #[arg(long, env, default_value = "http://api:8080/private/station-full-coverage-samples")]
    pub station_full_coverage_stats_url: String,
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub stanox_crs_url: String,

    // Shared+distinct OAuth2 (Decision 5 -- same shape as every other caller)
    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    // Reload cadences
    #[arg(long, env, default_value_t = 300)]
    pub population_reload_secs: u64,
    #[arg(long, env, default_value_t = 3600)]
    pub stanox_crs_reload_secs: u64,
    #[arg(long, env, default_value_t = 60)]
    pub stats_write_interval_secs: u64,

    /// Decision 4 -- comma-separated line ids to shadow-compute, or "*"
    /// (default) for every catalogued line with at least one tiploc.
    /// Does NOT gate whether a line's stats are ever shown/escalated --
    /// that's LineDefinition.full_coverage_enabled, unchanged, in
    /// aggregator.
    #[arg(long, env, default_value = "*")]
    pub shadow_lines: String,

    // Static line catalogue, same value_parser pattern as aggregator's
    // own --lines-dir (needed to build the reverse tiploc->line index,
    // Task 9)
    #[arg(long = "lines-dir", env = "LINES_DIR", value_parser = parse_lines_dir)]
    pub lines: std::collections::HashMap<String, common::LineDefinition>,

    #[arg(long, env, default_value = "0.0.0.0:8082")]
    pub health_bind_url: String,
    #[arg(long, env, default_value_t = 9093)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

(`parse_lines_dir` — copy `aggregator/src/config.rs`'s exact equivalent
function, don't re-derive; confirm its real name/signature first.)

- [ ] **Step 4: `main.rs` — compiles, boots, does nothing real yet**

```rust
//! `full-coverage-consumer`: a second, independent Kafka consumer against
//! the same RDM Train Movements feed `trust-consumer` reads, correlating
//! every event against the FULL scheduled population of every
//! shadow-computed line (not a small pinned-train set) -- see
//! docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md and
//! docs/superpowers/plans/2026-09-04-option-b-live-consumer-plan.md.
//! SHADOW MODE ONLY: writes real per-line/per-station stats, but nothing
//! reads them into a real line's severity/DataQuality while
//! LineDefinition.full_coverage_enabled stays false everywhere (see the
//! design doc's binding condition).

mod config;
mod health;
// mod correlate;       -- Task 10
// mod population;      -- Task 9
// mod queries;          -- Tasks 9/11/12
// mod stanox_tiploc;    -- Task 9
// mod station_correlate; -- Task 12
// mod stats;            -- Task 11

use clap::Parser;
use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let _connection_state = health::spawn(config.health_bind_url.clone());
    tracing::info!("full-coverage-consumer scaffolding booted; no correlation logic yet (Task 8 of the implementation plan)");
    // Task 13 replaces this with the real loop.
    std::future::pending::<()>().await;
    Ok(())
}
```

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo build -p full-coverage-consumer
```

```bash
git add crates/full-coverage-consumer Cargo.toml
git commit -m "Scaffold crates/full-coverage-consumer (boots, no correlation logic yet)"
```

---

## Task 9: `full-coverage-consumer` — `stanox_tiploc.rs` + `population.rs`

**Files:** create `crates/full-coverage-consumer/src/{stanox_tiploc.rs,population.rs,queries.rs}`;
modify `src/main.rs` (add the two `mod` lines).

Depends on Task 8 (scaffolding), Task 5 (the population GET route),
Task 6 is not needed yet.

- [ ] **Step 1: `stanox_tiploc.rs` — Decision 2c/2h's table**

```rust
//! STANOX -> TIPLOC and STANOX -> CRS, from the same live
//! `/private/stanox-crs` feed `trust-consumer` already reloads --
//! extended (unlike `trust-consumer::stanox_crs::StanoxCrsTable`, which
//! drops `tiploc`) to keep BOTH fields, since this consumer needs both:
//! TIPLOC for line-population membership (Decision 2c), CRS for
//! station-level grouping (Decision 2h). See
//! docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md's
//! "Current relevant state" section, the `common::StanoxCrsRecord`
//! finding.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct StanoxTable {
    stanox_to_tiploc: HashMap<String, String>,
    stanox_to_crs: HashMap<String, String>,
}

impl StanoxTable {
    pub fn from_records(records: &[common::StanoxCrsRecord]) -> Self {
        let mut stanox_to_tiploc = HashMap::new();
        let mut stanox_to_crs = HashMap::new();
        for r in records {
            stanox_to_tiploc.insert(r.stanox.clone(), r.tiploc.clone());
            stanox_to_crs.insert(r.stanox.clone(), r.crs.clone());
        }
        Self { stanox_to_tiploc, stanox_to_crs }
    }

    pub fn tiploc(&self, stanox: &str) -> Option<&str> {
        self.stanox_to_tiploc.get(stanox).map(String::as_str)
    }

    pub fn crs(&self, stanox: &str) -> Option<&str> {
        self.stanox_to_crs.get(stanox).map(String::as_str)
    }
}
```

Unit tests: a two-record fixture, assert both lookups succeed and an
unknown STANOX returns `None` for both.

- [ ] **Step 2: `population.rs` — in-memory population + reverse index**

```rust
//! Decision 2b's in-memory population map and Decision 2c's reverse
//! tiploc->line index, built from `schedule_query::LinePopulationEntry`
//! rows fetched via `GET /private/schedule-line-population`.

use std::collections::HashMap;

use schedule_query::{CallingPoint, LinePopulationEntry};

#[derive(Debug, Clone, Default)]
pub struct Population {
    /// line_id -> service_date -> uid -> calling points
    by_line: HashMap<String, HashMap<chrono::NaiveDate, HashMap<String, Vec<CallingPoint>>>>,
}

impl Population {
    pub fn insert(&mut self, line_id: &str, service_date: chrono::NaiveDate, entries: Vec<LinePopulationEntry>) {
        let by_uid: HashMap<String, Vec<CallingPoint>> =
            entries.into_iter().map(|e| (e.uid, e.calling_points)).collect();
        self.by_line
            .entry(line_id.to_string())
            .or_default()
            .insert(service_date, by_uid);
    }

    /// Every UID this line's population contains for `service_date`,
    /// empty if nothing has been published yet (Decision 2e's Pending
    /// case, upstream of the rail-day gate).
    pub fn uids_for(&self, line_id: &str, service_date: chrono::NaiveDate) -> Vec<&str> {
        self.by_line
            .get(line_id)
            .and_then(|by_date| by_date.get(&service_date))
            .map(|by_uid| by_uid.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub fn calling_points(&self, line_id: &str, service_date: chrono::NaiveDate, uid: &str) -> Option<&[CallingPoint]> {
        self.by_line.get(line_id)?.get(&service_date)?.get(uid).map(Vec::as_slice)
    }
}

/// Decision 2c's reverse index: tiploc -> every shadow-computed line whose
/// `lines/*.toml` `Station.tiploc` includes it. Built once per population
/// reload from the static catalogue (not from the population data itself
/// -- a line's STATION list, not its scheduled services, defines which
/// TIPLOCs are "its own").
pub fn build_tiploc_index(lines: &HashMap<String, common::LineDefinition>) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines.values() {
        for station in &line.stations {
            if let Some(tiploc) = &station.tiploc {
                index.entry(tiploc.clone()).or_default().push(line.id.clone());
            }
        }
    }
    index
}
```

- [ ] **Step 3: `queries.rs` — HTTP client functions**

```rust
//! Thin HTTP client wrappers -- deliberately separate from correlation
//! logic, same reasoning as trust-consumer's own queries.rs module doc:
//! keeps correlation logic unit-testable without a live api.

pub async fn fetch_line_population(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    line_id: &str,
    service_date: chrono::NaiveDate,
) -> anyhow::Result<Option<serde_json::Value>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .query(&[("line_id", line_id), ("service_date", &service_date.to_string())])
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn fetch_stanox_crs(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    // Identical to trust_consumer::queries::fetch_stanox_crs -- not
    // extracted into trust-schema (Task 1's scope was parsing/dedup/
    // journey only, deliberately not HTTP client code, which has no
    // shared logic beyond "GET + bearer + deserialize," already trivial).
    let token = tokens.get_token(client).await?;
    let response = client.get(url).bearer_auth(&token).send().await?.error_for_status()?;
    Ok(response.json().await?)
}
```

- [ ] **Step 4: Tests, verify, commit**

Pure unit tests for `StanoxTable`, `Population::{insert,uids_for,calling_points}`,
`build_tiploc_index` (a two-line fixture sharing one TIPLOC, asserting the
reverse index maps it to both line ids). `queries.rs` untested directly,
same posture as `trust-consumer::queries`.

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer
```

```bash
git add crates/full-coverage-consumer
git commit -m "full-coverage-consumer: STANOX/TIPLOC/CRS table, in-memory population, reverse tiploc index"
```

---

## Task 10: `full-coverage-consumer` — `correlate.rs` (Decision 2d matching)

**Files:** create `crates/full-coverage-consumer/src/correlate.rs`; modify
`src/main.rs`.

Depends on Task 1 (`trust_schema::{schema,journey}`), Task 9
(`population.rs`, `stanox_tiploc.rs`).

- [ ] **Step 1: State shape**

```rust
//! Decision 2d's matching algorithm: per-`(line_id, uid)` running record,
//! reusing trust_schema::journey's derivation logic exactly as
//! trust-consumer does, keyed differently (per-(line_id, uid) here vs.
//! per-train_id there) -- confirmed compatible with zero generalization
//! by Task 1's own grounding pass.

use std::collections::HashMap;

use trust_schema::journey::DerivedState;
use trust_schema::schema::{Activation, Cancellation, Movement};

use crate::population::Population;
use crate::stanox_tiploc::StanoxTable;

#[derive(Debug, Clone, Default)]
pub struct CorrelationState {
    /// train_id -> train_uid, parked by Activation (mirrors
    /// trust-consumer's ProcessorState.pending_activations, but this
    /// consumer has no expiry-pruning need yet since it's rebuilt per
    /// rail day -- see Task 13's own cycle-reset note).
    pub pending_activations: HashMap<String, String>,
    /// (line_id, uid) -> DerivedState, one entry per line a UID has been
    /// matched against.
    pub derived: HashMap<(String, String), DerivedState>,
    /// train_id -> train_uid, learned once an Activation OR a matched
    /// Movement confirms it (mirrors ProcessorState.resolved).
    pub resolved: HashMap<String, String>,
}

pub fn apply_activation(state: &mut CorrelationState, activation: &Activation) {
    state.pending_activations.insert(activation.train_id.clone(), activation.train_uid.clone());
}

/// Returns every `(line_id, uid)` this Movement was matched against, for
/// the caller (Task 12) to also feed into station-level grouping.
pub fn apply_movement(
    state: &mut CorrelationState,
    movement: &Movement,
    stanox: &StanoxTable,
    tiploc_index: &HashMap<String, Vec<String>>,
    population: &Population,
    service_date: chrono::NaiveDate,
) -> Vec<(String, String)> {
    let Some(train_uid) = state
        .resolved
        .get(&movement.train_id)
        .cloned()
        .or_else(|| state.pending_activations.get(&movement.train_id).cloned())
    else {
        return vec![]; // no Activation seen for this train_id yet -- nothing to attribute
    };

    let loc_tiploc = movement.loc_stanox.as_deref().and_then(|s| stanox.tiploc(s));
    let candidate_lines = loc_tiploc
        .and_then(|t| tiploc_index.get(t))
        .cloned()
        .unwrap_or_default();

    let mut matched = vec![];
    for line_id in candidate_lines {
        if population.uids_for(&line_id, service_date).contains(&train_uid.as_str()) {
            state.resolved.insert(movement.train_id.clone(), train_uid.clone());
            let key = (line_id.clone(), train_uid.clone());
            let previous = state.derived.entry(key.clone()).or_insert_with(DerivedState::awaiting_activation);
            let loc_crs = None; // Decision 2h's CRS need is handled by the caller (Task 12), which
                                 // has its own stanox->crs lookup; correlate.rs stays line/tiploc-scoped.
            *previous = trust_schema::journey::apply_movement(previous, movement, loc_crs);
            matched.push(key);
        }
    }
    matched
}

pub fn apply_cancellation(state: &mut CorrelationState, cancellation: &Cancellation) -> Vec<(String, String)> {
    let Some(train_uid) = state.resolved.get(&cancellation.train_id).cloned() else {
        return vec![];
    };
    let mut cancelled = vec![];
    for (key, derived) in state.derived.iter_mut() {
        if key.1 == train_uid {
            *derived = trust_schema::journey::apply_cancellation(derived);
            cancelled.push(key.clone());
        }
    }
    cancelled
}
```

- [ ] **Step 2: Unit tests, no I/O, no Kafka**

Construct a `CorrelationState`, a two-line `tiploc_index` sharing one
TIPLOC, a `Population` with one UID in one line's population, and assert:

- An Activation followed by a Movement at that TIPLOC matches exactly the
  line whose population contains the UID (not the other candidate line
  sharing the TIPLOC, if its population doesn't contain the UID).
- A Movement for a `train_id` with no prior Activation returns `vec![]`
  and mutates nothing (the "zero events" case starts here, not as a
  special branch — an unmatched UID just never gets a `derived` entry).
- Two Movements for the same `(line_id, uid)` update the same
  `DerivedState` in place (not two separate entries) — the per-UID-per-line
  dedup Decision 2d's own doc names.
- A Cancellation after a Movement flips every matched `(line_id, uid)`
  pair's status to `"cancelled"` while preserving `last_reported_location`
  (reusing `trust_schema::journey::apply_cancellation`'s own already-tested
  behavior — this test proves the fan-out across multiple `(line_id, uid)`
  keys for the same `train_uid`, not `apply_cancellation`'s own logic
  again).

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer correlate
```

```bash
git add crates/full-coverage-consumer/src/correlate.rs crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: Decision 2d matching (Activation/Movement/Cancellation correlation)"
```

---

## Task 11: `full-coverage-consumer` — `stats.rs` (synthesis, rail-day gating, line-level write)

**Files:** create `crates/full-coverage-consumer/src/stats.rs`; modify
`src/main.rs`, `src/queries.rs`.

Depends on Task 2 (`common::rail_day`), Task 6 (the write route), Task 10
(`CorrelationState.derived`).

- [ ] **Step 1: Synthesis (2f/2g) + per-line resolve (2e)**

```rust
//! Decision 2f/2g's SampleStats synthesis and Decision 2e's
//! Resolved-vs-Pending rail-day gating, per line.

use std::collections::HashMap;

use common::{FullCoverageLineStatsRow, SampleStats, StationDeparture};
use trust_schema::journey::DerivedState;

fn synthesize_departure(uid: &str, derived: &DerivedState) -> StationDeparture {
    StationDeparture {
        service_id: uid.to_string(),
        operator: String::new(),
        destination_crs: String::new(),
        scheduled: String::new(),
        estimated: String::new(),
        is_cancelled: derived.status == "cancelled",
        delay_minutes: derived.delay_minutes.unwrap_or(0),
        cancel_reason: None,
        delay_reason: None,
        headcode: None,
        skipped_stations: vec![], // Decision 2g -- PASS-to-skipped mapping unresolved; left empty, not guessed
    }
}

/// Builds one line's stats row for `service_date`. `unconfirmed` is every
/// population UID for this line/date with NO `derived` entry at all
/// (Decision 2d's "zero observed events" case) -- treated as cancelled,
/// per that decision's own flagged accuracy-risk caveat.
pub fn build_line_row(
    line_id: &str,
    service_date: chrono::NaiveDate,
    population_uids: &[&str],
    derived: &HashMap<(String, String), DerivedState>,
    rail_day_closed: bool,
    defaults: &common::Defaults,
) -> FullCoverageLineStatsRow {
    let departures: Vec<StationDeparture> = population_uids
        .iter()
        .map(|uid| match derived.get(&(line_id.to_string(), uid.to_string())) {
            Some(state) => synthesize_departure(uid, state),
            None => StationDeparture {
                // Decision 2d: unconfirmed-by-window-close = cancelled.
                // Applied here regardless of rail_day_closed's value for
                // the STATS COMPUTATION -- but `availability` below still
                // reads Pending until the window genuinely closes, per
                // Decision 2e's literal reading. A Pending row's stats
                // are therefore a preview, not yet the line's real
                // determination -- consistent with Available meaning
                // "every scheduled service... has been matched," not
                // "matched so far."
                service_id: uid.to_string(),
                operator: String::new(),
                destination_crs: String::new(),
                scheduled: String::new(),
                estimated: String::new(),
                is_cancelled: true,
                delay_minutes: 0,
                cancel_reason: None,
                delay_reason: None,
                headcode: None,
                skipped_stations: vec![],
            },
        })
        .collect();

    let refs: Vec<&StationDeparture> = departures.iter().collect();
    let stats: SampleStats = common::compute_sample_stats(&refs, defaults.delay_threshold_minutes, |d| {
        !d.skipped_stations.is_empty()
    });

    FullCoverageLineStatsRow {
        line_id: line_id.to_string(),
        service_date,
        availability: if rail_day_closed { "available" } else { "pending" }.to_string(),
        stats,
    }
}
```

- [ ] **Step 2: Rail-day gate + write path**

```rust
/// Decision 2e: a line's rail day is "closed" once
/// common::rail_day::next_rail_day_boundary for that service_date's start
/// has passed `now`.
pub fn rail_day_closed(service_date: chrono::NaiveDate, now: chrono::DateTime<chrono::Utc>) -> bool {
    let day_start = service_date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc();
    common::rail_day::next_rail_day_boundary(day_start) <= now
}
```

`queries.rs`, add:

```rust
pub async fn post_full_coverage_stats(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    rows: &[common::FullCoverageLineStatsRow],
) -> anyhow::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    common::ingest::post_batch(client, url, tokens, rows, "full-coverage line stats").await
}
```

- [ ] **Step 3: Tests**

Pure unit tests for `build_line_row`: a fully-matched population (every
UID has a `derived` entry, none cancelled) → `total == len`, `cancelled ==
0`; a population with one UID that has NO `derived` entry → that UID
counts toward `cancelled`, not silently dropped from `total` (this is the
single most important behavioral assertion in this file — it's the
concrete expression of Decision 2d's own named risk); `availability`
reads `"pending"` before `rail_day_closed` and `"available"` after, using
two fixed `DateTime<Utc>` fixtures straddling a known 02:00 Europe/London
boundary (reuse Task 2's own test dates for the exact boundary instant, so
this doesn't re-derive DST-transition correctness, only that this file
calls the shared function correctly).

- [ ] **Step 4: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer stats
```

```bash
git add crates/full-coverage-consumer/src/{stats.rs,queries.rs} crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: SampleStats synthesis, rail-day gating, line-level write path"
```

---

## Task 12: `full-coverage-consumer` — `station_correlate.rs` (Decision 2h)

**Files:** create `crates/full-coverage-consumer/src/station_correlate.rs`;
modify `src/correlate.rs` (Movement/Cancellation handlers gain a CRS/toc_id
side output), `src/main.rs`, `src/queries.rs`.

Depends on Task 10, Task 9 (`StanoxTable::crs`). **Its write path depends
on the per-station chain's own `common::StationFullCoverageSample` and
`POST /private/station-full-coverage-samples` existing — see this plan's
Non-goals for the merge-order note.** This task's own **logic and unit
tests are independent of that dependency**; only `queries::post_station_full_coverage_samples`'s
compilation needs `common::StationFullCoverageSample` to exist.

- [ ] **Step 1: Station-level state + the asymmetric-population rule**

```rust
//! Decision 2h: a second, parallel running record keyed (crs, toc_id),
//! fed off the same event stream correlate.rs already processes. The
//! asymmetric-population rule (a population UID only contributes once
//! its toc_id is learned from a real Activation) is this module's one
//! genuinely new correctness property -- see this module's own doc on
//! `StationCorrelationState::activations_by_uid`.

use std::collections::HashMap;

use trust_schema::journey::DerivedState;

#[derive(Debug, Clone, Default)]
pub struct StationCorrelationState {
    /// train_uid -> toc_id, learned only from a real Activation
    /// (`schema::Activation::toc_id`). A UID absent here has NOT been
    /// confirmed by TRUST this rail day -- Decision 2h's own "excluded
    /// entirely, not guessed" rule for the station-level output,
    /// asymmetric with the line-level output's treatment of the same
    /// case (Decision 2d still counts it as a line-level cancellation).
    pub activations_by_uid: HashMap<String, String>,
    /// (crs, toc_id) -> uid -> DerivedState
    pub derived: HashMap<(String, String), HashMap<String, DerivedState>>,
}

pub fn apply_activation(state: &mut StationCorrelationState, train_uid: &str, toc_id: &str) {
    state.activations_by_uid.insert(train_uid.to_string(), toc_id.to_string());
}

/// Called by correlate.rs's own apply_movement, once per matched
/// `(line_id, uid)`, with the movement's translated CRS (Decision 2c's
/// STANOX->CRS half) -- a UID with no learned toc_id yet is silently
/// skipped here (returns false), per Decision 2h's own rule; the caller
/// (Task 13's metrics wiring) increments
/// `full_coverage_consumer_station_buckets_dropped_total` when this
/// returns false, so the drop is observable, not silent in the
/// operational sense even though it's silent in the stats themselves.
pub fn apply_movement_station(
    state: &mut StationCorrelationState,
    train_uid: &str,
    crs: &str,
    derived_line_level: &DerivedState,
) -> bool {
    let Some(toc_id) = state.activations_by_uid.get(train_uid).cloned() else {
        return false;
    };
    state
        .derived
        .entry((crs.to_string(), toc_id))
        .or_default()
        .insert(train_uid.to_string(), derived_line_level.clone());
    true
}
```

(`derived_line_level.clone()` — reuses the exact `DerivedState` `correlate::apply_movement`
already computed for the line-level record, per the design doc's own "one
pass over the feed producing two outputs, not two passes" framing — this
function does not re-derive delay/status, only re-files the same already-computed
state under a second key.)

- [ ] **Step 2: Wire into `correlate.rs`**

`correlate::apply_movement`'s return type grows to also report the
translated CRS (via `stanox.crs(...)`, alongside the existing
`stanox.tiploc(...)` lookup) so `main.rs` (Task 13) can call
`station_correlate::apply_movement_station` once per matched line, without
`correlate.rs` itself depending on `station_correlate.rs` (keeps the two
modules' test suites independent — `correlate.rs`'s own tests, Task 10,
don't need to know station-level grouping exists at all).

- [ ] **Step 3: Synthesis + write path**

Mirrors Task 11's `build_line_row`/`post_full_coverage_stats` shape
exactly, but producing `common::StationFullCoverageSample` rows (one per
`(crs, toc_id)` bucket with at least one `derived` entry — Decision 2h's
own "only pairs that actually resolved this cycle are included" rule, no
`Pending`-sentinel row):

```rust
pub fn build_station_rows(
    state: &StationCorrelationState,
    resolved_at: chrono::DateTime<chrono::Utc>,
    defaults: &common::Defaults,
) -> Vec<common::StationFullCoverageSample> {
    state
        .derived
        .iter()
        .map(|((crs, operator), by_uid)| {
            let departures: Vec<common::StationDeparture> = by_uid
                .iter()
                .map(|(uid, derived)| /* same synthesize_departure as Task 11's stats.rs -- reused, not duplicated */ )
                .collect();
            let refs: Vec<&common::StationDeparture> = departures.iter().collect();
            let stats = common::compute_sample_stats(&refs, defaults.delay_threshold_minutes, |d| !d.skipped_stations.is_empty());
            common::StationFullCoverageSample {
                crs: crs.clone(),
                operator: operator.clone(),
                resolved_at,
                stats,
            }
        })
        .collect()
}
```

`queries.rs`:

```rust
pub async fn post_station_full_coverage_samples(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    samples: &[common::StationFullCoverageSample],
) -> anyhow::Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    common::ingest::post_batch(client, url, tokens, samples, "station full-coverage samples").await
}
```

(This function's own body doesn't compile until `common::StationFullCoverageSample`
exists — the merge-order dependency named in Non-goals, concretely
located.)

- [ ] **Step 4: Tests**

Pure unit tests for `StationCorrelationState`: a Movement for a UID with
NO prior Activation → `apply_movement_station` returns `false`, `derived`
gains no entry (the asymmetric-population case, tested directly); the same
Movement AFTER an Activation for that UID → returns `true`, files under
the right `(crs, toc_id)`; two UIDs for the same `(crs, toc_id)` both
contribute to the same bucket's `SampleStats` (not two separate buckets);
`build_station_rows` on an empty `derived` map → empty `Vec` (Decision
2h's "no sentinel Pending row" rule, tested directly).

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer station_correlate
```

```bash
git add crates/full-coverage-consumer/src/{station_correlate.rs,correlate.rs,queries.rs} crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: Decision 2h station-level grouping and its write path (depends on the per-station chain's endpoint at runtime)"
```

---

## Task 13: `full-coverage-consumer` — `main.rs` full loop wiring

**Files:** modify `crates/full-coverage-consumer/src/main.rs`; create
`crates/full-coverage-consumer/src/feed.rs` (or reuse a `feed` module —
see Step 1).

Depends on Tasks 8–12 all existing.

- [ ] **Step 1: Kafka feed**

This crate needs its own `MovementFeed`/`KafkaMovementFeed` — **not**
extracted into `trust-schema` (Task 1's scope was explicitly
parsing/dedup/journey; `feed/kafka.rs` is `rdkafka`-specific plumbing with
real `trust-consumer`-only concerns — offset management tied to that
crate's own single-replica constraint). Copy `trust-consumer/src/feed/`'s
structure (the `MovementFeed` trait + `KafkaMovementFeed` real impl +
`FakeMovementFeed` test double), adjusted only for this crate's own
`Config` shape — same file-for-file precedent Task 8's `health.rs` copy
already established for "genuinely per-consumer plumbing, not shared
logic."

- [ ] **Step 2: The loop**

Mirrors `trust-consumer/src/main.rs`'s own multi-cadence-in-one-loop shape
(population reload / stanox-crs reload / consume-and-correlate, each on
its own timer, all checked once per iteration) plus one more cadence this
crate alone needs (the periodic stats-write, Decision 3's "upserts... at
the end of every correlation cycle"):

```rust
let mut correlation_state = correlate::CorrelationState::default();
let mut station_state = station_correlate::StationCorrelationState::default();
let mut population = population::Population::default();
let mut tiploc_index: HashMap<String, Vec<String>> = HashMap::new();
let stanox = std::sync::RwLock::new(StanoxTable::default());

loop {
    // 1. population reload (population_reload_secs) -- Task 9's fetch,
    //    for every line in config.shadow_lines (parsed against
    //    config.lines' own keys -- "*" means every key).
    // 2. stanox_crs reload (stanox_crs_reload_secs) -- Task 9's fetch,
    //    rebuilds `stanox` AND `tiploc_index` (the latter is purely
    //    static-catalogue-derived, so it only needs rebuilding when
    //    config.lines changes, which doesn't happen at runtime -- build
    //    it ONCE before the loop, not on this cadence; only `stanox`
    //    itself reloads periodically).
    // 3. consume + correlate: feed.poll() -> for each TrustMessage,
    //    dispatch to correlate::apply_activation/apply_movement/
    //    apply_cancellation, feeding apply_movement's per-(line_id, uid)
    //    matches into station_correlate::apply_movement_station too.
    //    Commit offsets after a successful poll+dispatch cycle --
    //    correlation state derivation is naturally idempotent on Kafka
    //    redelivery (DerivedState fields are last-write-wins per event,
    //    not additive -- confirmed by Task 1's own reading of
    //    journey::apply_movement/apply_cancellation), so unlike
    //    trust-consumer's commit-only-after-successful-POST discipline,
    //    THIS crate's offset commit does not need to wait on the
    //    periodic stats POST succeeding -- a real, deliberate difference
    //    from trust-consumer's shape, worth stating plainly: this
    //    consumer's Kafka commit cadence and its stats-write cadence are
    //    genuinely decoupled, because persistence here is a periodic
    //    snapshot of accumulated state, not a per-event append.
    // 4. stats write (stats_write_interval_secs): for every
    //    shadow-computed line, stats::build_line_row + POST; for every
    //    populated (crs, toc_id) bucket, station_correlate::build_station_rows
    //    + POST (best-effort, independent failures -- Decision 3's "no
    //    shared transaction" rule).
    // 5. rail-day rollover: when `chrono::Utc::now()`'s rail day has
    //    advanced past every currently-tracked service_date, reset
    //    correlation_state/station_state for the new day (population for
    //    the new day is picked up by step 1's own reload, which already
    //    fetches "today's and tomorrow's" per Decision 2b) -- a fresh
    //    correlation_state per rail day is deliberate: yesterday's
    //    (line_id, uid) keys must not silently accumulate forever in a
    //    long-lived process.
}
```

(The `//` comments above are the loop's real shape, to be filled in as
real code following `trust-consumer/src/main.rs`'s own structure line for
line — this step is deliberately left at this level of detail because the
loop's exact control flow is genuinely implementation-level plumbing best
written against the real, compiling Tasks 8–12 modules rather than
hand-typed here against a guess of their final signatures.)

- [ ] **Step 3: Metrics**

`common::metrics::install(config.metrics_port)` (already wired in Task 8's
scaffolding). New counters, per the design doc's own Metrics list: 

`full_coverage_consumer_events_matched_total{line_id}`,
`full_coverage_consumer_lines_available_total`/`_pending_total`,
`full_coverage_consumer_stations_available_total`/`_pending_total`,
`full_coverage_consumer_station_buckets_dropped_total` (incremented every
time `station_correlate::apply_movement_station` returns `false`, per Task
12 Step 1's own note), `full_coverage_consumer_cycle_duration_seconds`.

- [ ] **Step 4: Integration-shaped tests against `FakeMovementFeed`**

Mirrors `trust-consumer/src/main.rs`'s own `#[cfg(test)] mod tests`
structure exactly (a `FakeMovementFeed` fixture, real
`reference-data/stanox-crs.csv`, asserting end-to-end: an Activation +
Movement batch against a fixture population produces the expected
`derived` state and the expected `stats::build_line_row` output) — this is
the one place this crate's tests exercise the full wiring together, not
just each module in isolation.

- [ ] **Step 5: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p full-coverage-consumer --all-features
cargo test -p full-coverage-consumer
```

```bash
git add crates/full-coverage-consumer
git commit -m "full-coverage-consumer: wire the full consume/correlate/write loop"
```

---

## Task 14: `crates/aggregator` — real full-coverage query, replacing the placeholder

**Files:** modify `crates/aggregator/src/{queries.rs,main.rs}`.

Depends on Task 6. This is the "Correction 1" fix — a **direct SQL query**,
not an HTTP call.

- [ ] **Step 1: `queries.rs`**

```rust
/// Every full_coverage_line_stats row with availability = 'available' AND
/// service_date = today (Europe/London-naive "today," matching this
/// crate's own existing convention for date-scoped reads elsewhere in
/// this file -- confirm the exact convention this file already uses
/// before picking one). A stale (yesterday's) or still-pending row is
/// simply absent from the returned map -- merge_full_coverage already
/// treats a missing key identically to "no signal yet" (Pending), so no
/// new branch is needed in aggregation.rs for this. See
/// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
/// Decision 3 and this plan's own Correction 1 (this replaces that
/// design doc's HTTP-based sketch with a direct query, since this crate
/// already holds its own `PgPool`).
pub async fn load_full_coverage_line_stats(
    pool: &PgPool,
    today: chrono::NaiveDate,
) -> Result<HashMap<String, SampleStats>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT line_id, total, delayed, cancelled, skipped, avg_delay_minutes \
         FROM full_coverage_line_stats WHERE availability = 'available' AND service_date = $1",
    )
    .bind(today)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let line_id: String = row.try_get("line_id")?;
            let stats = SampleStats {
                total: row.try_get::<i32, _>("total")? as usize,
                delayed: row.try_get::<i32, _>("delayed")? as usize,
                cancelled: row.try_get::<i32, _>("cancelled")? as usize,
                skipped: row.try_get::<i32, _>("skipped")? as usize,
                avg_delay_minutes: row.try_get("avg_delay_minutes")?,
            };
            Ok((line_id, stats))
        })
        .collect()
}
```

- [ ] **Step 2: Wire into `main.rs::run_cycle`**

```rust
// crates/aggregator/src/main.rs -- replaces line 192's literal.
let today = chrono::Utc::now().date_naive(); // or this file's own existing "today" convention, if one already exists -- check first
let full_coverage = queries::load_full_coverage_line_stats(pool, today)
    .await
    .unwrap_or_else(|err| {
        tracing::error!(error = ?err, "failed to load full_coverage_line_stats; treating every line as Pending this cycle");
        HashMap::new()
    });
aggregation::merge_full_coverage(&mut reports, &lines, &full_coverage, defaults);
```

(Fail-open to an empty map, matching `stanox_crs`'s own established
fail-open reload posture the design doc's own Decision 3 already cited —
now implemented for real, against a direct SQL failure rather than an HTTP
failure.)

- [ ] **Step 3: Regression tests — behavior-preserving for every real
      line today**

Every existing `aggregation.rs` test that calls `aggregate(...)` (not
`merge_full_coverage` directly) is untouched — `merge_full_coverage` isn't
called from inside `aggregate()` at all (confirmed: it's a separate
post-pass in `main.rs::run_cycle`, per `aggregation.rs`'s own doc comment
on the function). The only thing this task can regress is `main.rs`'s own
`run_cycle` behavior when `load_full_coverage_line_stats` returns a
genuinely empty map — which it always will in production today, since no
real line has `full_coverage_enabled: true` (Non-goals) and Task 15's
deployment doesn't change that. Add one new `#[ignore]`-gated DB test
(the first in this crate — see Global Constraints) proving this
end-to-end:

- Seed one `full_coverage_line_stats` row for a fixture `line_id` with
  `availability = 'available'`, run `load_full_coverage_line_stats` against
  today's date → the row comes back in the map.
- The same row with `service_date` set to yesterday → absent from the map
  (the staleness guard, tested directly).
- The same row with `availability = 'pending'` → absent from the map.
- Delete the fixture row unconditionally at the end.

- [ ] **Step 4: Test, lint, build, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p aggregator
DATABASE_URL=<url> cargo test -p aggregator full_coverage -- --ignored --test-threads=1
```

```bash
git add crates/aggregator/src/queries.rs crates/aggregator/src/main.rs
git commit -m "aggregator: replace the full-coverage placeholder with a real direct-SQL query against full_coverage_line_stats"
```

---

## Task 15: Deployment — Helm chart, Dockerfile, CI/compose wiring

**Files:** create `docker/full-coverage-consumer.Dockerfile`,
`charts/distant-signal/templates/full-coverage-consumer-deployment.yaml`;
modify `charts/distant-signal/values.yaml`, `.github/workflows/containers.yml`,
`docker-compose.yml`.

Depends on Task 8 (the crate must exist and build).

- [ ] **Step 1: Dockerfile**

`docker/full-coverage-consumer.Dockerfile`, structurally identical to
`docker/trust-consumer.Dockerfile` (same builder-stage `cmake`/
`libssl-dev`/`libsasl2-dev`/`libcurl4-openssl-dev` packages, same
`rust:1.88-bookworm` base, same runtime-stage `libssl3`/`libsasl2-2` +
non-root user, `cargo build --bin full-coverage-consumer` instead of
`--bin trust-consumer`). No reference-data COPY step needed (unlike
`trust-consumer`, this crate has no baked-in CSV — its STANOX/CRS table is
always live-reloaded via `queries::fetch_stanox_crs`, never a
`--stanox-crs-file` startup default — confirm this is actually true once
Task 9's `config.rs` is final; if a startup default proves necessary for
the same reason `trust-consumer` has one, add the identical COPY step).

- [ ] **Step 2: Helm Deployment**

`charts/distant-signal/templates/full-coverage-consumer-deployment.yaml`,
copied structurally from `trust-consumer-deployment.yaml` line for line:

- Same fail-fast Helm guard block for empty
  `fullCoverageConsumer.kafka.{brokers,topic,saslMechanism}` — but note
  these values are **not new** at the values.yaml level (see Step 3): the
  guard checks `trustConsumer.kafka.*` (reused), so the guard block itself
  can be a comment noting it's covered by `trust-consumer-deployment.yaml`'s
  own guard already firing first if those values are empty, OR (simpler,
  less coupled) this Deployment repeats the same guard against the same
  `trustConsumer.kafka.*` values independently, so it fails on its own if
  ever rendered without `trust-consumer-deployment.yaml` also present —
  pick the independent-guard version; two harmless duplicate `fail`
  messages if both are empty is safer than one Deployment silently
  depending on the other's template rendering first.
- `replicas: 1` (same single-consumer-group-per-deployment reasoning).
- `readinessProbe`/`livenessProbe` against `/healthz`, same shape.
- `automountServiceAccountToken: false`, same `securityContext` posture.
- Env vars: `KAFKA_BROKERS`/`KAFKA_TOPIC`/`KAFKA_SASL_MECHANISM` sourced
  from `.Values.trustConsumer.kafka.*` (**reused**, per Decision 1's
  "connection vs. group membership" reasoning — no new values.yaml keys
  for these three); `KAFKA_CONSUMER_GROUP` from a **new**
  `.Values.fullCoverageConsumer.kafka.consumerGroup`
  (`distant-signal-full-coverage-consumer` default);
  `KAFKA_SASL_USERNAME`/`KAFKA_SASL_PASSWORD` from the **same** secret
  `trust-consumer-deployment.yaml` already references (same RDM
  credential, same connection, per Decision 1); `SCHEDULE_LINE_POPULATION_URL`/
  `FULL_COVERAGE_STATS_URL`/`STATION_FULL_COVERAGE_STATS_URL`/`STANOX_CRS_URL`
  built from `distant-signal.apiBaseUrl` + the fixed private paths;
  `INTERNAL_OAUTH_*` from a **new**, distinct
  `fullCoverageConsumerSecretName`/OAuth-username/password secret-key-ref
  pair (own Authentik service-account credential, per Decision 5 —
  mirror `trustConsumerSecretName`/`trustConsumerOauthUsernameSecretKey`'s
  own `_helpers.tpl` pattern, don't hand-roll a new one); `LINES_DIR`
  mirroring `aggregator`'s own env var for the same mounted
  `lines/*.toml` ConfigMap volume (this Deployment needs that volume
  mounted too — copy `aggregator-deployment.yaml`'s volume/volumeMount
  block, not `trust-consumer-deployment.yaml`'s, since `trust-consumer`
  doesn't mount it); `SHADOW_LINES` from
  `.Values.fullCoverageConsumer.shadowLines` (default `"*"`);
  `POPULATION_RELOAD_SECS`/`STANOX_CRS_RELOAD_SECS`/`STATS_WRITE_INTERVAL_SECS`
  from their own new values; `HEALTH_BIND_URL`, `RUST_LOG`.

- [ ] **Step 3: `values.yaml`**

New `fullCoverageConsumer:` top-level block, mirroring `trustConsumer:`'s
own shape minus the `kafka.brokers`/`kafka.topic`/`kafka.saslMechanism`/
`kafka.saslUsername`/`kafka.saslPassword`/`kafka.existingSecret*` keys
(reused from `trustConsumer.kafka.*`, per Step 2's own note) — this
block only needs its own `image`, `kafka.consumerGroup`, `shadowLines`,
`populationReloadSecs`, `stanoxCrsReloadSecs`, `statsWriteIntervalSecs`,
`healthPort`, `metricsPort`, `logLevel`, and the usual
`resources`/`nodeSelector`/`tolerations`/`affinity`/`podAnnotations`/
`podSecurityContext`/`extraEnv` tail every other service block already
has. `api.internalOauth.groups.fullCoverage` (Task 4) is already covered.

- [ ] **Step 4: CI + compose**

`.github/workflows/containers.yml`'s matrix, one more entry:

```yaml
- service: full-coverage-consumer
  dockerfile: docker/full-coverage-consumer.Dockerfile
  target: ""
```

`docker-compose.yml`: a `full-coverage-consumer` service block, structurally
copied from the existing `trust-consumer` block (same build context/cache
ids per that file's own header comment, same env var list minus the ones
reused from `trust-consumer`'s own env block — actually distinct env vars
here, not shared at the compose layer, since compose has no Helm-style
value reuse; copy `trust-consumer`'s block and adjust every URL/credential
name).

- [ ] **Step 5: Verify and commit**

```bash
helm template charts/distant-signal > /dev/null
docker compose config > /dev/null   # or `docker-compose config`, whichever this repo's README documents
```

```bash
git add docker/full-coverage-consumer.Dockerfile \
        charts/distant-signal/templates/full-coverage-consumer-deployment.yaml \
        charts/distant-signal/values.yaml .github/workflows/containers.yml docker-compose.yml
git commit -m "Deploy full-coverage-consumer: Helm Deployment, Dockerfile, CI image matrix, compose service"
```

---

## Task 16: Final verification

- [ ] **Step 1: Full workspace verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -p aggregator -- --ignored --test-threads=1
```

- [ ] **Step 2: Confirm the shadow-mode invariant, by inspection**

```bash
grep -rn full_coverage_enabled lines/*.toml
```

Confirm still `false`/absent everywhere — this plan touches zero
`lines/*.toml` files (Non-goals).

- [ ] **Step 3: Confirm the file-ownership boundary held**

```bash
git diff --stat main...HEAD | grep -i station_full_coverage_samples
```

Expected: **zero matches for a migration or route file** — the only
legitimate appearances of the string `station_full_coverage_samples`
anywhere in this plan's diff are (a) `common::StationFullCoverageSample`
usages inside `crates/full-coverage-consumer/src/station_correlate.rs`/
`queries.rs` (an HTTP client's own type usage, not a route/migration) and
(b) doc-comment citations. If a `crates/api/migrations/*station_full_coverage_samples*.sql`
or a route-handler diff to `crates/api/src/routes/ingest.rs` touching that
specific table appears, stop — that's this plan overstepping the binding
boundary.

- [ ] **Step 4: Confirm `merge_full_coverage`'s call site is real, not a
      literal**

```bash
grep -n "merge_full_coverage(&mut reports" crates/aggregator/src/main.rs
```

Confirm it no longer reads `&HashMap::new()` — Task 14's own change.

- [ ] **Step 5: Manual smoke check against a real deployment (if
      available)**

Confirm `full-coverage-consumer`'s `/healthz` reports `connected` once
Kafka group membership is established; confirm at least one
`full_coverage_line_stats` row appears with `availability = 'pending'`
within one `population_reload_secs` window of a fresh deploy; confirm no
real line's public `/public/lines` or `/public/stations/*` response ever
shows anything but `"fullCoverageAvailability": {"state": "not-enabled"}`
(the shadow-mode binding condition, verified end-to-end one more time,
the same check the per-station plan's own Task 9 Step 5 already performs
for its own surface).

---

## Testing

Summarized (see each task's own steps for the authoritative detail):

- **`trust-schema`** (Task 1): the moved test suites, unmodified in
  assertions — proof of behavior-preservation.
- **`common::rail_day`** (Task 2): the moved DST/boundary test suite,
  unmodified in assertions.
- **`schedule-query`** (Task 3): one new unit test for `LinePopulationEntry`'s
  `From` conversion.
- **`crates/api`** (Tasks 5, 6, 14's aggregator side): `#[ignore]`-gated
  DB tests for both new tables' upsert/read paths, following
  `station_stats.rs::db_tests`'s seed/assert/delete convention exactly, per
  the task brief's own required density.
- **`schedule-reference`** (Task 7): a unit test for the new line-filter
  predicate; reuses `schedule-query`'s own already-proven parsing
  correctness rather than re-testing it.
- **`full-coverage-consumer`** (Tasks 9–13): pure unit tests for every
  correlation/matching/synthesis function (no I/O, no Kafka, no database —
  matching the task brief's explicit ask), plus one integration-shaped
  test suite against `FakeMovementFeed` mirroring `trust-consumer`'s own
  end-to-end test structure (Task 13).
- **`aggregator`** (Task 14): the first `#[ignore]`-gated DB test in this
  crate, proving the real query's staleness/availability filtering; a
  by-inspection confirmation that no existing `aggregate()`-based test
  needed a single assertion changed (the placeholder replacement is
  provably behavior-preserving for every real line, since the map stays
  empty in every scenario those tests construct).
- **CI**: every DB-backed test in this plan runs under the existing
  `.github/workflows/ci.yml:215-216` invocation once `aggregator` is added
  to its `-p` list alongside `api` (it may already be listed — confirm;
  the workflow's comment already anticipates both crates having ignored
  tests even though `aggregator` has none as of this plan's writing).
