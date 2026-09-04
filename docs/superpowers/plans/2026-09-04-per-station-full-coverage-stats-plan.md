# Per-Station Full-Coverage Stats — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.
>
> **Tasks 1–7 are backend, land in order (each depends on the previous
> one's code existing, except where a task explicitly says
> "independent").** **Task 8 is frontend, depends on Task 7's route being
> live** — its mocks and expected JSON shapes come directly from Task 7's
> DB-backed test assertions, matching the prior per-station-stats plan's
> own Task 5 precedent for why frontend work waits on a landed wire shape.

**Goal:** implement
`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`
end to end, per the ownership boundary that document's task brief drew:
this plan **owns** the `station_full_coverage_samples` migration and both
its `POST`/`GET /private/station-full-coverage-samples` routes (the
producer contract Option B's live-consumer design converged on
verbatim — see the design doc's now-resolved Open Question #1), the
`(station, operator)` full-coverage config-gating logic, the wire-shape
extension to `StationOperatorSampleStats`, and the corresponding
station-page rendering. **It does not own, and does not touch, Option B's
own live consumer** (`crates/full-coverage-consumer`, not yet built, a
parallel chain's job) — every layer this plan adds stays exactly as inert
as the already-merged line-level full-coverage scaffolding until (a) a
real line's `full_coverage_enabled` flips `true` (it does not, anywhere,
today) and (b) Option B's consumer exists and writes real rows (it does
not, today — this plan's own tests seed their own fixture rows directly,
per the design doc's binding non-goal, and never assume a live producer).

**Architecture:** one new table + migration (`crates/api`), three new
query functions (`crates/api/src/data/queries.rs`), a new POST/GET pair on
the existing private ingest surface (`crates/api/src/routes/ingest.rs`,
matching this repo's actual established convention for that class of
route — see Task 5's note on where the design doc's own sketch was
adjusted), one new pure gating function plus an extended
`compute_station_operator_stats` (`crates/api/src/data/station_stats.rs`),
an extended `GET /public/stations/{crs}/sample-stats` handler
(`crates/api/src/routes/station_stats.rs`, reusing the render helpers the
line-level work already extracted — no new render.rs code needed this
time), and additive frontend types/precedence-helper widening. No changes
to `aggregator` (per-station stats stay on the read-time Option-C path,
per the design doc's Decision 2 — the per-line full-coverage path through
`aggregator::merge_full_coverage` is untouched) and no changes to
`crates/full-coverage-consumer` (does not exist; not this plan's job to
create).

**Tech Stack:** Rust (axum, sqlx with runtime-checked queries — no
`cargo sqlx prepare` needed, matching this crate's existing convention),
Next.js 16 App Router + TypeScript, Vitest 2 + `@testing-library/react`.

**Design doc:**
`docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md`
— its Decisions section is authoritative for every type/route/wire shape
below; this plan does not repeat the reasoning, only the concrete steps,
and calls out explicitly the few places this plan's concrete step departs
from the design doc's own "sketch, not final" code (all such departures
are toward matching this repo's actual established precedent more
closely, never a substantive design change).

---

## Non-goals

- **No changes to `crates/full-coverage-consumer` or any Option B
  live-consumer file.** That crate does not exist in this worktree and is
  not created by this plan — it is the parallel chain's job, per the
  task's binding file-ownership boundary. This plan only builds the
  `crates/api`-side write/read surface that consumer will eventually call
  as an HTTP client.
- **No flipping `LineDefinition.full_coverage_enabled` for any real line,
  anywhere in `lines/*.toml`.** Every real line's flag stays `false`;
  every `full_coverage_availability` this plan's code can produce for a
  real station therefore stays `NotEnabled` in production, identical to
  the line-level scaffolding's own current shadow state.
- **No real producer writes any real row into `station_full_coverage_samples`
  as part of this plan.** Every DB-backed test this plan adds seeds its
  own fixture rows directly under the reserved `Z…` CRS namespace
  (matching `station_stats.rs`'s and `departures.rs`'s existing
  convention) and deletes them unconditionally at the end — never
  depends on, or waits for, Option B's consumer landing.
- **No per-station history/rollup table.** Design doc Decision 2 — a live,
  wholesale-replaced-per-cycle row per `(crs, operator)`, mirroring
  `station_samples`'s own posture, not an append log.
- **No new render.rs helpers.** `sample_stats_json`/`sample_availability_json`/
  `full_coverage_availability_json` already exist as `pub(crate)`
  (`crates/api/src/render.rs:97-128`), already used by `status_to_json` for
  `LineStatus` — this plan's route handler imports and reuses them
  unchanged.
- **No UI copy/screenshots for the fourth station-page state** (an
  operator row with real `fullCoverageAvailability` but no LDBWS-sampled
  departures at all) beyond what `formatSampleSummary`'s existing,
  source-agnostic precedence chain already renders. Design doc Decision 6 /
  Open Question #5 flags this as a real but small, unscheduled copy
  decision — not blocking, not designed to pixel level here.
- **No aggregator change of any kind.** Per-station full-coverage stays
  read-time in `crates/api`, mirroring per-station *sample* stats' own
  existing architecture (design doc "Architectural constraint" note under
  Decision 2) — `aggregator::merge_full_coverage`'s existing, separate,
  per-*line* path is untouched.

## Global Constraints

- **Testing:** Rust — `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-DB tests), plus
  `DATABASE_URL=<url> cargo test -p api -- --ignored --test-threads=1` for
  the DB-backed tests this plan adds (mirrors
  `.github/workflows/ci.yml:215-216`'s exact CI invocation — no new CI job
  needed). Frontend — `npm test` (`frontend/package.json`'s `"test": "vitest
  run"`) and `npm run build` (`next build`) from `frontend/`.
- **File scope.** Modified: `crates/common/src/lib.rs`,
  `crates/api/src/data/queries.rs`, `crates/api/src/data/config.rs`,
  `crates/api/src/app.rs`, `crates/api/src/routes/ingest.rs`,
  `crates/api/src/data/station_stats.rs`,
  `crates/api/src/routes/station_stats.rs`, `crates/api/src/routes/lines.rs`,
  `crates/api/src/routes/line_status.rs`, `crates/api/src/routes/chatbot.rs`,
  `crates/api/src/routes/departures.rs`, `crates/api/src/routes/train.rs`,
  `crates/api/src/auth.rs` (the last seven only for their colocated
  `test_app`/fixture `ServiceArguments { .. }` literals gaining one new
  field — see Task 5 Step 3), `charts/distant-signal/templates/api-deployment.yaml`,
  `charts/distant-signal/values.yaml`, `frontend/lib/types.ts`,
  `frontend/lib/sampleStats.ts`, `frontend/lib/sampleStats.test.ts`,
  `frontend/app/stations/[crs]/page.test.tsx`. Created: `crates/api/migrations/YYYYMMDDHHMMSS_station_full_coverage_samples.sql`.
- **CRS/operator matching:** exact `==` comparison against
  already-canonical values, no `.to_uppercase()` added anywhere new — same
  convention the per-station-stats plan's own Global Constraints already
  established and this plan's new code (`full_coverage_enabled_for`) must
  not break.
- **Wire field naming for the new private ingest payload uses plain
  snake_case Rust field names, NOT the Option B design doc's illustrative
  camelCase JSON (`resolvedAt`)** — every existing private producer
  payload in this codebase (`StationSample`, `IncidentMessage`,
  `StanoxCrsRecord`, …) derives `Serialize`/`Deserialize` with no
  `#[serde(rename_all = "camelCase")]`, because these are Rust-to-Rust
  payloads between this app's own crates, not a public API. Only the
  hand-built `render.rs` JSON for the *public* route is camelCase (an
  existing, deliberate split — see `render.rs`'s own module doc). This
  plan's `common::StationFullCoverageSample` therefore uses
  `resolved_at`, matching `StationSample.polled_at`'s own precedent, and
  a future `full-coverage-consumer` POSTs `{"crs": ..., "operator": ...,
  "resolved_at": ..., "stats": {...}}` — a small, deliberate correction
  from the Option B design doc's own "sketch, not final" JSON, not a
  disagreement with its schema/endpoint naming (which this plan keeps
  verbatim).

---

### Task 1: `crates/common` — add `StationFullCoverageSample` (backend, independent)

**Files:**
- Modify: `crates/common/src/lib.rs`

Independent of every other task — a plain data type with no logic.

- [ ] **Step 1: Add the type next to `StationSample`**

Place directly after `StationSample` (`lib.rs:564-569`):

```rust
/// One resolved `(crs, operator)` full-coverage row, mirroring
/// `StationSample`'s own per-station shape one level finer. Written
/// directly by a future `full-coverage-consumer` to `POST
/// /private/station-full-coverage-samples` (not built by this plan — see
/// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
/// Decision 2h/3, converged on
/// docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
/// Decision 2's own guess verbatim), read at request time by
/// `queries::latest_station_full_coverage_samples`. `stats` stores
/// serialized JSONB in `station_full_coverage_samples.stats` -- same
/// storage posture as `StationSample.departures`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationFullCoverageSample {
    pub crs: String,
    pub operator: String,
    pub resolved_at: DateTime<Utc>,
    pub stats: SampleStats,
}
```

- [ ] **Step 2: Build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p common
```

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "Add common::StationFullCoverageSample, the per-(crs, operator) full-coverage producer row"
```

---

### Task 2: Migration — `station_full_coverage_samples` (backend, independent)

**Files:**
- Create: `crates/api/migrations/YYYYMMDDHHMMSS_station_full_coverage_samples.sql`
  (use the actual current UTC timestamp at implementation time, after the
  latest existing migration — `20260903200001_line_status_half_hourly_coverage_stats.sql`
  is the most recent as of this plan's writing)

Independent of Task 1 (no code dependency, just needs to land before any
DB-backed test in Task 5/7 runs against a real database).

- [ ] **Step 1: Write the migration, byte-for-byte matching both design
      docs' converged schema**

```sql
-- crates/api/migrations/YYYYMMDDHHMMSS_station_full_coverage_samples.sql
-- -------------------------------------------------------------------------
-- Per-(station, operator) full-coverage producer table. One row per (crs,
-- operator), wholesale-replaced per producer resolution cycle -- same
-- "live snapshot, not history" posture as station_samples (the LDBWS
-- sample-stats sibling this mirrors one level finer) and
-- full_coverage_line_stats (its per-line counterpart, owned by a
-- different chain). No real writer populates this table yet -- see
-- docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
-- Decision 2 and its now-resolved Open Question #1: the schema below is
-- adopted verbatim by
-- docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
-- Decision 2h/3 as the actual producer contract Option B's future
-- consumer writes to.
-- -------------------------------------------------------------------------

CREATE TABLE station_full_coverage_samples (
    crs         CHAR(3)     NOT NULL,
    operator    TEXT        NOT NULL,
    resolved_at TIMESTAMPTZ NOT NULL,
    stats       JSONB       NOT NULL,
    PRIMARY KEY (crs, operator)
);

CREATE INDEX station_full_coverage_samples_crs ON station_full_coverage_samples (crs);
```

The extra `crs`-only index (not in either design doc's sketch, both of
which only declared the composite primary key) mirrors
`station_samples`' own single-column primary key needing no secondary
index for its single-CRS lookup — but this table's primary key is
composite `(crs, operator)`, and `latest_station_full_coverage_samples`
(Task 3) queries by `crs` alone, which the leading column of a composite
PK btree already serves efficiently. On reflection this secondary index is
redundant with the PK's own leading-column index — **do not add it**;
delete this paragraph's index line before landing. (Left in the plan as a
worked-through example of why it was rejected, not as a step to actually
run — the PK on `(crs, operator)` alone already gives `WHERE crs = $1` an
efficient index scan.)

- [ ] **Step 2: Apply and sanity-check against a local database**

```bash
DATABASE_URL=<local url> sqlx migrate run --source crates/api/migrations
```

Confirm the table exists with the expected columns
(`\d station_full_coverage_samples` in `psql`, or equivalent).

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/*_station_full_coverage_samples.sql
git commit -m "Add station_full_coverage_samples table (no writer yet)"
```

---

### Task 3: `crates/api/src/data/queries.rs` — upsert + two reads (backend)

**Files:**
- Modify: `crates/api/src/data/queries.rs`

Depends on Task 1 (`common::StationFullCoverageSample`) and Task 2 (the
table itself, for any DB-backed test — the code compiles without it since
these are runtime-checked `sqlx::query`/`sqlx::query_as` calls, not the
`query!` macro family).

- [ ] **Step 1: Add `upsert_station_full_coverage_samples`**

Directly after `upsert_station_samples` (`queries.rs:258-280`), same
per-row `ON CONFLICT` upsert shape, keyed on the composite primary key:

```rust
/// Upserts a batch of per-(crs, operator) full-coverage rows. No
/// history -- wholesale-replaced per producer resolution cycle, same
/// rationale as `upsert_station_samples`. Written by
/// `post_station_full_coverage_samples` (Task 5), a future
/// full-coverage-consumer's real caller once it exists (not built by this
/// plan).
pub async fn upsert_station_full_coverage_samples(
    pool: &PgPool,
    samples: &[common::StationFullCoverageSample],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let stats_json = serde_json::to_value(&sample.stats)?;

        sqlx::query(
            r#"
            INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (crs, operator) DO UPDATE SET
                resolved_at = EXCLUDED.resolved_at,
                stats       = EXCLUDED.stats
            "#,
        )
        .bind(&sample.crs)
        .bind(&sample.operator)
        .bind(sample.resolved_at)
        .bind(&stats_json)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}
```

- [ ] **Step 2: Add `last_station_full_coverage_samples_fetch`**

Directly after `last_station_samples_fetch` (`queries.rs:533-` — find its
exact current location), same shape (`SELECT MAX(...)`), backing the new
private `GET` freshness-check route (Task 5), mirroring the existing
`last_*_fetch` family exactly:

```rust
pub async fn last_station_full_coverage_samples_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(resolved_at) FROM station_full_coverage_samples")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}
```

(Match the exact return-binding pattern `last_station_samples_fetch`
already uses in this file — copy its exact `query_as` tuple-destructure
idiom rather than re-deriving it, so this stays consistent with every
sibling `last_*_fetch` function.)

- [ ] **Step 3: Add `latest_station_full_coverage_samples`**

Directly after `latest_station_sample` (`queries.rs:664-` region), the
per-station **all-operators** read this plan's route handler (Task 7)
needs — note the return type is `Vec`, not `Option`, since a station can
have full-coverage rows for more than one operator:

```rust
/// Every `station_full_coverage_samples` row for one CRS, one per
/// operator that has resolved this cycle. Full-coverage analog of
/// `latest_station_sample`, one level finer -- design doc Decision 2.
/// Empty `Vec` for every station today: no producer writes this table yet.
pub async fn latest_station_full_coverage_samples(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<common::StationFullCoverageSample>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT crs, operator, resolved_at, stats FROM station_full_coverage_samples WHERE crs = $1",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let stats_json: serde_json::Value = row.try_get("stats")?;
            Ok(common::StationFullCoverageSample {
                crs: row.try_get("crs")?,
                operator: row.try_get("operator")?,
                resolved_at: row.try_get("resolved_at")?,
                stats: serde_json::from_value(stats_json)?,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
```

(No new tests in this file itself — these three functions are exercised
end-to-end by Task 5's and Task 7's `#[ignore]`-gated DB tests, matching
`upsert_station_samples`/`latest_station_sample`'s own precedent of having
no dedicated `queries.rs`-level test module; every existing `last_*_fetch`/
`upsert_*`/`latest_*` function in this file is tested the same indirect
way, through its route.)

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add upsert/read query functions for station_full_coverage_samples"
```

---

### Task 4: `crates/api/src/data/config.rs` + `app.rs` — new internal-OAuth group (backend)

**Files:**
- Modify: `crates/api/src/data/config.rs`
- Modify: `crates/api/src/app.rs`
- Modify (colocated `ServiceArguments { .. }` test/fixture literals — see
  Step 3): `crates/api/src/auth.rs`, `crates/api/src/routes/lines.rs`,
  `crates/api/src/routes/line_status.rs`, `crates/api/src/routes/chatbot.rs`,
  `crates/api/src/routes/departures.rs`, `crates/api/src/routes/train.rs`,
  `crates/api/src/routes/station_stats.rs`

Independent of Tasks 1–3 (pure config plumbing); must land before Task 5
(the route needs a group to gate on) and Task 7 (that file's own
`db_tests::test_app` helper needs the new field too).

Resolves the design doc's Open Question #4 the way Option B's own
Decision 5 already answered it: **the same group name gates both this
plan's `/private/station-full-coverage-samples` and the separate,
different-chain's future `/private/full-coverage-stats`** (one producer
service, one credential, two endpoints), so name the field/group
generically, not station-specific.

- [ ] **Step 1: Add the config field**

`crates/api/src/data/config.rs`, directly after
`internal_oauth_group_schedule_reference` (`:91-92`):

```rust
/// Gates both `POST/GET /private/station-full-coverage-samples`
/// (Task 5, this plan) and a separate, not-yet-built
/// `/private/full-coverage-stats` (the per-line counterpart, a different
/// chain's own future task) -- one producer service
/// (`full-coverage-consumer`, not yet built), one credential, two
/// endpoints it may write to. Resolves
/// docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
/// Open Question #4 per
/// docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
/// Decision 5.
#[arg(long, env, default_value = "svc-full-coverage-consumer")]
pub internal_oauth_group_full_coverage: String,
```

- [ ] **Step 2: Wire it into `app.rs`**

1. `build_internal_oauth_routes` (`app.rs:57-160` region) — add two
   entries, mirroring `/station-samples`'s own GET+POST pair
   (`app.rs:92-100`) exactly:

```rust
(
    "/station-full-coverage-samples",
    Method::GET,
    vec![config.internal_oauth_group_full_coverage.clone()],
),
(
    "/station-full-coverage-samples",
    Method::POST,
    vec![config.internal_oauth_group_full_coverage.clone()],
),
```

2. `AppState::init`'s empty-value startup guard loop (`app.rs:222-268`
   region) — add one more `(name, value)` tuple entry:

```rust
(
    "internal_oauth_group_full_coverage",
    &config.internal_oauth_group_full_coverage,
),
```

- [ ] **Step 3: Update every colocated `ServiceArguments { .. }` test
      fixture with the new required field**

`ServiceArguments` has no `Default` impl (every field is explicit), so
adding a field is a compile break at every hand-built literal until each
gets `internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),`
added (match each file's own existing string style for its sibling
`internal_oauth_group_*` fields — some use the real default, some use a
`"test-..."`-prefixed placeholder; follow whatever that specific file
already does for its neighboring fields, don't introduce a new
convention). Confirmed sites, found via `grep -rn
internal_oauth_group_schedule_reference crates/api/src`:

- `crates/api/src/auth.rs`
- `crates/api/src/routes/lines.rs`
- `crates/api/src/routes/line_status.rs`
- `crates/api/src/routes/chatbot.rs`
- `crates/api/src/routes/departures.rs`
- `crates/api/src/routes/train.rs`
- `crates/api/src/routes/station_stats.rs` (this file's `db_tests::test_app`
  helper — also touched again in Task 7, but the field must exist by
  Task 5/this task's build step regardless)

Re-run the grep at implementation time in case another site was added
between this plan's writing and execution — `cargo build --workspace`
will also just fail loudly at every missed site with a clear "missing
field" error, so this is self-correcting even if the list above is
stale.

- [ ] **Step 4: Wire the Helm chart (this app's own deployment, not
      `full-coverage-consumer`'s — that chart doesn't exist and isn't
      created by this plan)**

`charts/distant-signal/values.yaml`, inside `api.internalOauth.groups`
(`:432-440` region), add:

```yaml
      fullCoverage: svc-full-coverage-consumer
```

`charts/distant-signal/templates/api-deployment.yaml`, directly after the
`INTERNAL_OAUTH_GROUP_SCHEDULE_REFERENCE` env entry (`:123-124`):

```yaml
            - name: INTERNAL_OAUTH_GROUP_FULL_COVERAGE
              value: {{ .Values.api.internalOauth.groups.fullCoverage | quote }}
```

- [ ] **Step 5: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
helm template charts/distant-signal > /dev/null   # sanity-check the chart still renders
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs crates/api/src/auth.rs \
        crates/api/src/routes/lines.rs crates/api/src/routes/line_status.rs \
        crates/api/src/routes/chatbot.rs crates/api/src/routes/departures.rs \
        crates/api/src/routes/train.rs crates/api/src/routes/station_stats.rs \
        charts/distant-signal/templates/api-deployment.yaml charts/distant-signal/values.yaml
git commit -m "Add internal_oauth_group_full_coverage, gating a new producer's future private routes"
```

---

### Task 5: Private ingest routes — `POST`/`GET /private/station-full-coverage-samples` (backend)

**Files:**
- Modify: `crates/api/src/routes/ingest.rs`

Depends on Task 1, Task 3, Task 4.

**Deliberate departure from the design doc's own sketch, noted plainly**:
the design doc's Decision 2 sketched this as `pub mod
station_full_coverage_samples;` — a brand-new route file "mirroring
station-samples' POST/GET pair (`routes/samples.rs`)". That citation is
itself slightly off: `/private/station-samples`'s actual POST/GET pair
lives in `crates/api/src/routes/ingest.rs`
(`post_station_samples`/`get_station_samples_last_fetched`,
`ingest.rs:43-45,97-104,135-143`) — `routes/samples.rs` is the unrelated
public `/sample-stations` GET route. This task follows the *real*
established convention instead: every private producer ingest POST/GET
pair in this codebase lives together in `ingest.rs`, one `router()` with
one `Vec<(path, Method, groups)>` entry per method in `app.rs` — adding a
fourteenth pair there keeps that one-file-per-concern-class convention
intact rather than forking a second file for an otherwise-identical
concern. This is a plan-level correction of the design doc's own
citation, not a disagreement with its schema/endpoint path (both kept
verbatim).

- [ ] **Step 1: Add the route to `ingest.rs`'s `router()`**

`ingest.rs:28-63`, add one more `.route(...)` entry, alongside
`/station-samples`'s own:

```rust
.route(
    "/station-full-coverage-samples",
    axum::routing::get(get_station_full_coverage_samples_last_fetched)
        .post(post_station_full_coverage_samples),
)
```

- [ ] **Step 2: Add the two handlers**

Directly after `post_station_samples`/`get_station_samples_last_fetched`,
matching their exact shape:

```rust
async fn get_station_full_coverage_samples_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_station_full_coverage_samples_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

async fn post_station_full_coverage_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<common::StationFullCoverageSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_full_coverage_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

`common::StationFullCoverageSample` needs adding to this file's existing
`use common::{..., StationSample, ...};` import line (`ingest.rs:21`).

- [ ] **Step 3: Add `#[ignore]`-gated DB tests**

New `#[cfg(test)] mod db_tests` in `ingest.rs` if one doesn't already
exist there (check first — if `ingest.rs` has no existing test module,
this is the first one added to this file; mirror
`crates/api/src/routes/station_stats.rs::db_tests`'s exact
`test_app`/`connect`/fixture-CRS-cleanup pattern, including its own
`test_app` helper — do not import `station_stats.rs`'s copy across files,
this repo's own convention is "colocated per-file rather than shared,
until a third file needs it too", per that file's own doc comment).
Cover, using the reserved `Z…` fixture CRS namespace:

- `POST` with one row → `upserted: 1`; a subsequent direct `SELECT`
  confirms the row landed with the right `stats` JSONB shape.
- `POST` twice with the same `(crs, operator)`, different `stats` the
  second time → the row is updated in place (`ON CONFLICT DO UPDATE`),
  not duplicated — assert exactly one row remains for that `(crs,
  operator)` afterward.
- `GET` (last-fetched) after seeding one row → returns a `fetchedAt`
  close to `resolved_at`, not `null`.
- `GET` against a fixture-only, freshly-cleaned table → `fetchedAt: null`
  (mirrors `last_station_samples_fetch`'s own behavior when the table is
  empty — don't assert this destructively against a real deployment's
  table, only against fixture rows this test itself controls, deleting
  them unconditionally at the end even on assertion failure).

Delete every fixture row unconditionally at the end of each test (both
`(crs, operator)` pairs used).

- [ ] **Step 4: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api station_full_coverage_samples -- --ignored --test-threads=1
```

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/ingest.rs
git commit -m "Add POST/GET /private/station-full-coverage-samples ingest routes"
```

---

### Task 6: Config gating + merge — `crates/api/src/data/station_stats.rs` (backend)

**Files:**
- Modify: `crates/api/src/data/station_stats.rs`

Depends on Task 1 (`StationFullCoverageSample`). Independent of Tasks
2–5's plumbing (pure logic, no I/O) — could in principle land earlier,
but is sequenced here because it's easiest to review right before the
route handler (Task 7) that calls it.

- [ ] **Step 1: Add `full_coverage_enabled_for`, the gating function
      (design doc Decision 1)**

Copy the design doc's Decision 1 sketch verbatim, including its doc
comment (the "route membership, not sample-station membership" reasoning
is load-bearing and should not be paraphrased away):

```rust
fn full_coverage_enabled_for(crs: &str, operator: &str, lines: &[common::LineDefinition]) -> bool {
    lines.iter().any(|line| {
        line.full_coverage_enabled
            && line.operators.iter().any(|op| op == operator)
            && line.stations.iter().any(|s| s.crs == crs)
    })
}
```

- [ ] **Step 2: Extend `OperatorSampleStats` with the two new fields**

```rust
pub struct OperatorSampleStats {
    pub operator: String,
    pub availability: SampleAvailability,
    pub full_coverage_stats: Option<common::SampleStats>,
    pub full_coverage_availability: common::FullCoverageAvailability,
}
```

- [ ] **Step 3: Widen `compute_station_operator_stats`'s signature and
      merge logic**

Per design doc Decision 4: two new parameters
(`full_coverage_rows: &[common::StationFullCoverageSample]`, `lines:
&[common::LineDefinition]`); operator membership becomes a **union** of
LDBWS-observed operators and full-coverage-row operators (not an
intersection — a station can have a real full-coverage row for an
operator with zero current LDBWS departures, the design doc's own flagged
consequence of route-membership gating). Copy the design doc's Decision 4
sketch's merge body, reusing `FullCoverageAvailability::full_coverage_stats()`
(`crates/common/src/lib.rs:858-863`) rather than re-deriving the same
match a second time:

```rust
pub fn compute_station_operator_stats(
    sample: &StationSample,
    defaults: &Defaults,
    full_coverage_rows: &[common::StationFullCoverageSample],
    lines: &[common::LineDefinition],
) -> Vec<OperatorSampleStats> {
    let operators: BTreeSet<&str> = sample
        .departures
        .iter()
        .map(|d| d.operator.as_str())
        .chain(full_coverage_rows.iter().map(|r| r.operator.as_str()))
        .collect();

    operators
        .into_iter()
        .map(|operator| {
            let relevant: Vec<&StationDeparture> = sample
                .departures
                .iter()
                .filter(|d| d.operator == operator)
                .collect();
            let availability = /* UNCHANGED from today's function body */;

            let full_coverage_availability = if !full_coverage_enabled_for(&sample.crs, operator, lines) {
                common::FullCoverageAvailability::NotEnabled
            } else {
                match full_coverage_rows.iter().find(|r| r.operator == operator) {
                    Some(row) => common::FullCoverageAvailability::Available(row.stats.clone()),
                    None => common::FullCoverageAvailability::Pending,
                }
            };
            let full_coverage_stats = full_coverage_availability.full_coverage_stats();

            OperatorSampleStats {
                operator: operator.to_string(),
                availability,
                full_coverage_stats,
                full_coverage_availability,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Update this file's five existing test call sites**

Every existing `#[cfg(test)] mod tests` call to
`compute_station_operator_stats(&sample, &defaults)` (five sites, per
`grep -n compute_station_operator_stats crates/api/src/data/station_stats.rs`)
needs two more arguments. Use `&[]` for `full_coverage_rows` and `&[]`
for `lines` in every existing test — this is deliberate, not a shortcut:
it proves every existing assertion about LDBWS-only behavior still holds
byte-for-byte with full coverage structurally present-but-empty,
mirroring the line-level scaffolding's own "empty map changes nothing"
regression-safety property. Run the existing suite unmodified in
assertions (only the call-site arity changes) — if any existing assertion
needs to change to keep passing, stop and diagnose; that would mean the
refactor changed LDBWS-only behavior, which it must not.

- [ ] **Step 5: Add new unit tests for the full-coverage merge (pure, no
      DB)**

New tests in the same `#[cfg(test)] mod tests`, covering:

- A line with `full_coverage_enabled: true` covering this station/operator,
  no matching `full_coverage_rows` entry → `full_coverage_availability`
  is `Pending`, `full_coverage_stats` is `None`.
- Same enabled line, a matching `full_coverage_rows` entry present →
  `Available(stats)`, and `full_coverage_stats` equals that same `stats`
  clone (proving the accessor round-trips, not just that the enum
  variant is right).
- No line has `full_coverage_enabled` set for this station/operator (the
  real, current state of every catalogued line today) → `NotEnabled`,
  regardless of whether a `full_coverage_rows` entry happens to be
  present — construct this case explicitly (a stray full-coverage row for
  a not-yet-enabled line) to prove the gate, not just the row's presence,
  controls the outcome.
- An operator with a `full_coverage_rows` entry but **zero** LDBWS
  departures at all → still appears in the returned `Vec` (the union
  case, design doc Decision 4's explicit "not an intersection" note) with
  `availability: BelowThreshold { observed: 0, .. }` and a real
  `full_coverage_availability`.
- `full_coverage_enabled_for`'s own gate, tested directly (not only
  through the merge): a line whose `stations` list includes this CRS but
  whose `operators` list does NOT include this operator → `false`; a line
  covering this operator elsewhere but not at this CRS → `false`; two
  lines, only one of which is enabled, both covering this
  (crs, operator) → `true` (the "union over every covering line" case).
  Construct minimal `LineDefinition` fixtures (only `id`, `operators`,
  `stations`, `full_coverage_enabled` need real values — every other
  field can use `Default`-equivalent empty values, matching how
  `crates/common`'s own `LineDefinition` tests construct minimal
  fixtures).

- [ ] **Step 6: Test and build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api station_stats
```

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/station_stats.rs
git commit -m "Add full_coverage_enabled_for gating and merge full-coverage rows into compute_station_operator_stats"
```

---

### Task 7: Route handler — extend `GET /public/stations/{crs}/sample-stats` (backend)

**Files:**
- Modify: `crates/api/src/routes/station_stats.rs`

Depends on Task 3 (new queries), Task 4 (this file's own `test_app` needs
the new config field — already added in Task 4 Step 3, confirm it's still
correct here), Task 6 (new function signature).

- [ ] **Step 1: Widen the 404 gate and assemble `lines` (design doc
      Decision 5)**

Replace the handler body's early-`None`-404 with the wider gate — 404
only when **both** `station_samples` and `station_full_coverage_samples`
are empty for this CRS — and assemble the live line list the same way
`routes/samples.rs:23-25` already does:

```rust
async fn get_station_sample_stats(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let sample = queries::latest_station_sample(&app.database, &crs)
        .await
        .map_err(internal_error)?;
    let full_coverage_rows = queries::latest_station_full_coverage_samples(&app.database, &crs)
        .await
        .map_err(internal_error)?;

    if sample.is_none() && full_coverage_rows.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for station: {crs}"),
        ));
    }

    let custom = crate::data::custom_lines::list_custom_lines(&app.database)
        .await
        .map_err(internal_error)?;
    let mut lines: Vec<common::LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(common::LineDefinition::from));

    let defaults = common::Defaults::default();
    let empty_sample = || common::StationSample {
        crs: crs.clone(),
        polled_at: chrono::Utc::now(),
        departures: vec![],
    };
    let stats = compute_station_operator_stats(
        &sample.unwrap_or_else(empty_sample),
        &defaults,
        &full_coverage_rows,
        &lines,
    );

    Ok(Json(
        stats
            .into_iter()
            .map(|s| {
                let mut out = json!({
                    "operator": s.operator,
                    "sampleAvailability": sample_availability_json(&s.availability),
                });
                if let Some(stats) = s.availability.sample_stats() {
                    out["sampleStats"] = sample_stats_json(&stats);
                }
                if let Some(stats) = &s.full_coverage_stats {
                    out["fullCoverageStats"] = sample_stats_json(stats);
                }
                out["fullCoverageAvailability"] =
                    full_coverage_availability_json(&s.full_coverage_availability);
                out
            })
            .collect(),
    ))
}
```

Add `full_coverage_availability_json` to this file's existing `use
crate::render::{sample_availability_json, sample_stats_json};` import
line — no other render.rs change needed (it already exists, `pub(crate)`,
per this plan's Non-goals).

- [ ] **Step 2: Update the existing `#[ignore]`-gated DB tests' route
      construction**

The three existing `db_tests` (`no_row_for_crs_is_404_naming_the_crs`,
`a_row_present_with_empty_departures_is_200_empty_array`,
`two_operators_render_alphabetically...`) don't call
`compute_station_operator_stats` directly (that's Task 6's job) — they
exercise the route end to end via `oneshot`, so no call-site signature
change is needed here. But **re-verify all three still pass unmodified**
after Step 1's handler rewrite — this is the regression check for the
widened 404 gate and the new `lines`/`full_coverage_rows` plumbing not
having broken the LDBWS-only path. If any assertion needs a change to
keep passing, stop and diagnose; the widened gate must not change
behavior for a station that only ever had `station_samples` data (the
`full_coverage_rows` `Vec` is empty for every fixture CRS these three
tests use, so the widened gate should never actually widen anything for
them).

- [ ] **Step 3: Add new `#[ignore]`-gated DB tests for the full-coverage
      path**

New tests in the same `db_tests` module, using the `Z…` fixture CRS
namespace (fresh codes, not reused from the existing three tests, so
tests can run with `--test-threads=1` without ordering assumptions
between them — matches this file's own existing convention of one fixture
CRS per test):

- **A station with a `station_full_coverage_samples` row but NO
  `station_samples` row at all** → `200`, not `404` (proves the widened
  gate); the response contains one entry for that operator with
  `sampleAvailability: {"state": "below-threshold", "observed": 0, ...}`
  and `fullCoverageAvailability` reflecting whatever the seeded
  `LineDefinition`s produce. Since this test seeds `station_full_coverage_samples`
  directly but this handler reads `app.config.lines` (the static
  catalogue, empty in `test_app`'s placeholder `ServiceArguments`) plus
  `custom_lines` (a live DB table) for gating — seed a **custom line**
  fixture via `custom_lines::create_custom_line` (or a direct `INSERT`,
  matching whatever this crate's existing custom-line DB tests already
  do) with `full_coverage_enabled: true`, this fixture CRS in its
  `stations`, and this fixture operator in its `operators`, so the gate
  actually resolves to `Available`, not `NotEnabled` — a test that only
  seeds the sample row without also enabling gating would trivially pass
  with every `full_coverage_availability` reading `NotEnabled`, which
  proves nothing about the merge path. Delete both fixtures
  (`station_full_coverage_samples` row and the custom line) unconditionally
  at the end.
- **The same setup, but the custom line's `full_coverage_enabled` is
  left `false`** (or the fixture line doesn't cover this CRS/operator at
  all) → `fullCoverageAvailability: {"state": "not-enabled"}` even though
  a real `station_full_coverage_samples` row exists for it — proves the
  gate, not just row presence, controls the wire output. Still `200` (the
  widened 404 gate only checks *row presence*, not gating), assuming a
  `station_samples` row or a `station_full_coverage_samples` row exists.
- **A station with real `station_samples` departures AND a matching,
  gated-enabled `station_full_coverage_samples` row for the same
  operator** → `200`, one entry, both `sampleStats` and
  `fullCoverageStats` present simultaneously on the same JSON object
  (asserting the exact nested shape, including `fullCoverageStats`'s
  camelCase `avgDelayMinutes` — the one place this gets proven end to end
  on the wire, mirroring the existing two-operator test's own "assert
  exact JSON, not just status code" discipline).
- **Neither table has any row for a fresh fixture CRS, and no custom line
  covers it** → still `404`, unchanged from today (the pre-existing
  test already covers this, but add one more assertion — or reuse the
  existing test — confirming the widened gate doesn't accidentally 200
  something it shouldn't).

- [ ] **Step 4: Test, lint, build**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p api
DATABASE_URL=<url> cargo test -p api station_sample_stats -- --ignored --test-threads=1
```

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/station_stats.rs
git commit -m "Extend GET /public/stations/{crs}/sample-stats with full-coverage fields and a widened 404 gate"
```

---

### Task 8: Frontend — wire shape + rendering (frontend only)

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/sampleStats.ts`
- Modify (colocated tests): `frontend/lib/sampleStats.test.ts`,
  `frontend/app/stations/[crs]/page.test.tsx`

Depends on Task 7's route/wire shape being final and merged/landed —
matches the prior per-station-stats plan's own Task 5 precedent for why
frontend work is gated on a landed backend route (no mock/stub layer
exists here for a shape that later changes).

**No `page.tsx` code change is needed** — confirmed by reading it
directly: the existing "Sample stats by operator" section
(`frontend/app/stations/[crs]/page.tsx:198-223`) already calls
`formatSampleSummary(entry)` generically on each
`StationOperatorSampleStats` row, and `formatSampleSummary`'s existing
precedence chain already prefers `fullCoverageStats` over `sampleStats`
when present (`sampleStats.ts`'s own doc comment: "full-coverage available
... -> sample available -> ..."). Once the type carries the new fields,
this section picks them up with zero additional code — exactly the
"reuse the existing precedence helpers, not new components" posture
design doc Decision 6 describes. Confirmed by checking that neither
`coverageProvenanceNote` nor `pendingCoverageNote` is rendered anywhere in
the app today either (`grep` across `frontend/app` — both are unused,
forward-looking scaffolding since the line-level work landed); this plan
does not newly wire either into `page.tsx`, keeping the same
forward-looking-but-unconsumed posture the line-level frontend work
already established, not a scope expansion.

- [ ] **Step 1: Extend `StationOperatorSampleStats` in `types.ts`**

`frontend/lib/types.ts:138-142`. Also correct the type's own doc comment,
which currently only describes the sample-stats fields:

```ts
/** One operator's row from `GET /public/stations/{crs}/sample-stats`
 * (`crates/api/src/routes/station_stats.rs`) --
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9,
 * extended per
 * docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
 * Decision 3. `sampleAvailability.state` is never `'no-coverage'` through
 * this route (a documented invariant of the route handler, not
 * type-enforced). `fullCoverageAvailability` is always present, like
 * `LineStatus`'s own field of the same name -- `'not-enabled'` for every
 * real operator today, since no line has `full_coverage_enabled` set. */
export interface StationOperatorSampleStats {
  operator: string;
  sampleAvailability: SampleAvailability;
  sampleStats?: SampleStats;
  fullCoverageStats?: SampleStats;
  fullCoverageAvailability: FullCoverageAvailability;
}
```

- [ ] **Step 2: Widen `SampleStatsCarrier` in `sampleStats.ts`, and fix
      its now-stale doc comment**

`frontend/lib/sampleStats.ts:17-21`. The current doc comment explicitly
(and, as of this task, incorrectly) states `fullCoverageStats` is
"widened onto this same carrier for Decision 1: only `LineStatus` ever
carries it in practice (`StationOperatorSampleStats` never does, per that
design doc's own 'line-level scope only' statement)" — that claim is now
false and must be corrected, not left to silently mislead a future
reader:

```ts
/** Structural supertype of anything `sampleUnavailableReason`/
 * `formatSampleSummary` can render a reason for -- the existing per-line
 * `LineStatus` callers and the per-(station, operator)
 * `StationOperatorSampleStats` rows both satisfy this without a cast.
 * Widened, not renamed -- see
 * docs/superpowers/specs/2026-09-03-per-station-stats-design.md Decision 9
 * for why the eventual source-agnostic rename flagged by
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * stays a separate, later step. `fullCoverageStats`/`fullCoverageAvailability`
 * are carried by BOTH `LineStatus` and (as of
 * docs/superpowers/specs/2026-09-04-per-station-full-coverage-stats-design.md
 * Decision 3) `StationOperatorSampleStats` -- the earlier "line-level scope
 * only" note here was accurate when written and is now stale; both real
 * callers structurally satisfy this field as of this update.
 * `fullCoverageAvailability` stays optional on this shared type (rather
 * than required) only so a hypothetical future caller with neither field
 * still satisfies it structurally. */
type SampleStatsCarrier = {
  sampleStats?: SampleStats;
  sampleAvailability: SampleAvailability;
  dataQuality?: LineStatus['dataQuality'];
  fullCoverageStats?: SampleStats;
  fullCoverageAvailability?: FullCoverageAvailability;
};
```

No other function in this file needs a signature change:
`sampleUnavailableReason`/`formatSampleSummary`/`coverageProvenanceNote`
already only read `fullCoverageStats` (present on the widened type now,
optionally, exactly like `dataQuality`), and none of them read
`fullCoverageAvailability` directly — only `pendingCoverageNote` does,
and it intentionally keeps its narrower `LineStatus` parameter type,
unchanged (per design doc's own scoping — see this task's note above on
why no new rendering is wired for the station page). Leave
`pendingCoverageNote` exactly as-is.

- [ ] **Step 3: Add/extend tests in `sampleStats.test.ts`**

- A bare `StationOperatorSampleStats`-shaped object (no `dataQuality`)
  carrying both `sampleStats` and `fullCoverageStats` →
  `formatSampleSummary` renders the `fullCoverageStats` numbers, not the
  `sampleStats` ones (proves the existing precedence still holds for the
  new caller shape, not just the old `LineStatus` one).
- The same shape with only `fullCoverageStats` set (no `sampleStats` at
  all — the real "full-coverage-only" case for a station with a
  full-coverage row but zero LDBWS departures) → renders correctly, no
  crash, no `undefined` in the output string.
- `sampleUnavailableReason` on a `StationOperatorSampleStats`-shaped
  object with neither field set → falls through to the existing
  `'no-coverage'`/hedge branches exactly as before (regression check that
  widening the type didn't change behavior for the plain case).

- [ ] **Step 4: Extend `page.test.tsx`'s existing full-response mock case**

The existing "two-operator array" test
(`frontend/app/stations/[crs]/page.test.tsx`, around the `operatorStats`
array literal near line 192) already constructs
`StationOperatorSampleStats[]` objects — extend one entry in that
existing array with `fullCoverageStats`/`fullCoverageAvailability` set
(rather than adding a wholly new test file section), and assert the
rendered row's trailing text reflects the full-coverage numbers, not the
sample ones — proving the wire-shape addition actually reaches the
rendered page, end to end, through the real component tree (not just
through `sampleStats.ts`'s unit tests in isolation).

- [ ] **Step 5: Test and build**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/sampleStats.ts frontend/lib/sampleStats.test.ts \
        "frontend/app/stations/[crs]/page.test.tsx"
git commit -m "Extend StationOperatorSampleStats with full-coverage fields, reusing existing render precedence"
```

---

### Task 9: Final verification

- [ ] **Step 1: Full workspace verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-features
cargo test --workspace
DATABASE_URL=<url> cargo test -p api -- --ignored --test-threads=1
```

- [ ] **Step 2: Full frontend verification**

```bash
cd frontend
npm test
npm run build
```

- [ ] **Step 3: Confirm zero touches outside this plan's ownership
      boundary**

```bash
git diff --stat main...HEAD
```

Two things to confirm from this diff, both binding per the task brief:

1. **No file under `crates/full-coverage-consumer/` appears** — that
   crate doesn't exist in this worktree and this plan never creates it.
2. **Every changed file is either in this plan's Global Constraints "File
   scope" list, or is a `crates/api/migrations/*.sql` addition** (a new
   file, so it won't appear in that list verbatim — confirm it's the one
   new migration this plan added, not an edit to an existing one; this
   repo's migrations are append-only by convention).

- [ ] **Step 4: Confirm the shadow-mode invariant directly, once, by
      inspection**

`grep -rn full_coverage_enabled lines/*.toml` (or equivalent — check
every catalogued line file) — confirm still `false`/absent everywhere,
proving this plan changed no line's real config. This plan's own
`full_coverage_enabled_for` gate reads that value; it must still resolve
to `false` for every real (station, operator) pair after this plan lands,
identically to before it.

- [ ] **Step 5: Manual smoke check against a real deployment (if
      available)**

`GET /public/stations/EDB/sample-stats` — confirm every entry now also
carries `"fullCoverageAvailability": {"state": "not-enabled"}` (every
real line's flag is still `false`), and no entry carries
`fullCoverageStats` (nothing is `Available`). `GET
/public/stations/ZZZ/sample-stats` (an unsampled code with no
full-coverage row either) — confirm still `404`.

## Testing

Summarized (see each task's own steps for the authoritative detail):

- **`crates/common`**: no new tests beyond the type itself compiling and
  (de)serializing — `StationFullCoverageSample` is a plain data carrier,
  proven correct by every DB-backed round-trip test in Tasks 5/7 rather
  than a standalone unit test.
- **`crates/api` (`data/station_stats.rs`)**: new pure unit tests for
  `full_coverage_enabled_for` (the gating logic, tested directly) and the
  extended `compute_station_operator_stats` merge (gated/ungated,
  present/absent-row, union-not-intersection membership) — no DB, mirrors
  this file's existing test style exactly.
- **`crates/api` (`routes/ingest.rs`)**: new `#[ignore]`-gated DB tests
  for the new POST/GET pair — upsert-then-read, upsert-is-idempotent,
  last-fetched freshness — following `station_stats.rs::db_tests`'s exact
  seed/assert/delete convention.
- **`crates/api` (`routes/station_stats.rs`)**: existing DB tests
  re-verified unmodified as a regression check on the widened 404 gate;
  new DB tests for the full-coverage merge reaching the public route,
  including the gated-vs-not-gated distinction and the
  simultaneous-sample-and-full-coverage wire shape, asserted as exact
  JSON.
- **`frontend`**: unit tests for the widened `SampleStatsCarrier`'s
  precedence behavior on a `StationOperatorSampleStats`-shaped input, and
  one extended end-to-end page test proving the new fields render through
  the real component tree via the already-generic
  `formatSampleSummary(entry)` call site.
- **CI**: this plan's DB-backed tests run under the existing
  `.github/workflows/ci.yml:215-216` job pattern (`cargo test -p api --
  --ignored --test-threads=1`) — no new CI job needed.
