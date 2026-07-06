# Plan: Poller Microservices for Knowledgebase Incidents, Stations, TOCs

## Goal

Build three independently-deployable poller services (Knowledgebase Incidents,
Stations JSON, TOCs) that fetch reference/incident data from the Rail Data
Marketplace (RDM) and push it into the existing Rust `axum` API's storage via
new internal ingestion endpoints on that same API — no separate ingestion
service. Restructure the repo into a Cargo workspace so the API and the three
pollers share one `common` crate for wire types.

This plan does **not** port the Python matcher/aggregator/segments prototype
(`src/*.py`) to Rust, and does not build the public `/Line/...` read endpoints.
Both are separate, already-flagged future work (DESIGN.md §8 Stage 1/§10).
This plan's scope ends at: raw data lands correctly in Postgres via the API.

## Global Constraints

1. **Deployment:** one `Dockerfile` per service now. Kubernetes manifests are
   explicitly out of scope for this plan — do not write them. Keep each
   Dockerfile a plain single-binary image so a future k8s Deployment is a
   drop-in.
2. **Ingestion path:** pollers POST to the *same* Rust API's private router
   (`private_router()` in `src/routes/mod.rs`). There is no separate
   ingestion-only service.
3. **RDM credentials:** already held by the user. No signup/registration
   steps in this plan — only config plumbing (env vars) for the keys.
4. **Poller stack:** Rust, in a Cargo **workspace** with separate member
   crates: `api`, `poller-incidents`, `poller-stations`, `poller-tocs`,
   `common`.
5. **No invented API details.** Every RDM endpoint URL, field name, and enum
   value used in code must trace to a source quoted in this plan. Where a
   fact is marked "gap" below, the task must use an env-configurable
   placeholder and a TODO comment, not a guess.
6. **No history table for reference data.** `stations` and `tocs` are
   wholesale-replaced per poll (same pattern as the existing
   `station_samples` table). Only `incidents` gets a history table, per the
   existing migration's own documented convention — do not add history
   tracking for stations/tocs.
7. **JSONB passthrough for accessibility data.** Don't hand-model every
   Stations-JSON accessibility sub-field as a typed Rust struct field; store
   the sub-object as `serde_json::Value` / Postgres `JSONB`, matching the
   existing `station_samples.departures JSONB` precedent.

## Current repo state (verified by reading the files directly, 2026-07-05)

- **Not yet a workspace.** Root `Cargo.toml` is a single `[package]`
  (`nr-status-v2`) using `axum`, `sqlx` (postgres, migrate), `tokio`, `serde`,
  `clap`, `chrono`, `glob`, `toml`, `anyhow`, `dotenv`, `tower`,
  `tower-http`, `tracing`. **No git commits exist yet** (repo has untracked
  files only) — Task 1 should make the first commit.
- `src/main.rs` — boots `AppState`, nests `/public` and `/private` routers,
  runs `sqlx::migrate!()`, serves.
- `src/app.rs` — `AppState { config: ServiceArguments, database: PgPool }`,
  `type App = Arc<AppState>`, `type Router = axum::Router<App>`.
- `src/types.rs` — `Severity` enum (0-14 TfL scale + 20/21 NR extensions).
- `src/dataclasses.rs` (private `mod` at crate root) — `DataQuality`,
  `ValidityPeriod`, `AffectedRoute`, `Disruption`, `LineStatus`,
  `LineStatusReport`, `StationDeparture`, `HealthStatus`.
- `src/data/mod.rs` — **only** declares `pub mod config; pub mod lines;`.
- `src/data/config.rs` — `Defaults` (per-line thresholds) + `ServiceArguments`
  (clap CLI: `bind_url`, `database_url`, `defaults_file`, `lines` dir).
- `src/data/lines.rs` — `Station`, `LineDefinition` (TOML line catalogue).
- `src/data/database.rs` — **exists but is dead code**: defines
  `IncidentMessage` and `StationSample` but is never `mod`-declared in
  `data/mod.rs`, so it does not currently compile into the crate. Task 1
  folds its types into `common` with fixes (see Task 1).
- `src/routes/mod.rs` — `public_router()` nests `health`; `private_router()`
  is **empty** — this is where ingestion endpoints go (Task 2).
- `migrations/20260510023522_initial.sql` — tables: `incidents`,
  `station_samples`, `line_status`, `line_status_history`,
  `incident_history`. **No `stations` or `tocs` reference-data tables yet** —
  Task 2 adds them.
- `lines/*.toml` + `lines/SCHEMA.md` — curatorial line catalogue, untouched
  by this plan.
- `src/*.py`, `tests/test_matcher.py`, `demo.py` — the original Python
  prototype (matcher/aggregator/segments). Reference only; out of scope.

## Background: RDM API research findings (context for Tasks 3-5, already inlined into those tasks below — read this section only if you need the full sourcing)

Full research was done against **RSPS5050 "National Rail Enquiries
Knowledgebase Data Feeds Specification", P-03-00 Rev A (18-Nov-2025)**,
downloaded from
`https://www.rspaccreditation.org/downloadPublic.php?did=1LY5sWcQiy6zrUqIgjYxYSu4V1RkEy0hMLkNhTAe9DnRjL7egR`,
cross-referenced against the exact RDM product IDs in this task:
Incidents = `P-cf16832d-d971-46e7-8883-4fca2101d3fa` (XML v5.0), Stations =
`P-9c97bd03-e2f2-462d-860a-5bec92700c2d` (JSON), TOCs =
`P-49f7a182-c71b-45a2-b0f0-3b52c9a2968c` (XML v4.0). The confirmed/gap facts
below are copied verbatim into Tasks 3-5; this section exists only so the
sourcing is auditable in one place. **The user may supply account-gated
docs (OpenAPI specs, the two missing base URLs, IncidentPriority meaning)
during execution — if they do, that supersedes the "gap" placeholders below
in whichever task is in flight when it arrives.**

Full details of the research (endpoint tables, exact field lists, mismatch
analysis against the original struct) are preserved in this plan's git
history / the conversation that produced it. The load-bearing facts are
repeated in each of Task 3/4/5 below so each task brief is self-contained.

---

## Task 1: Convert to a Cargo workspace + `common` crate

**What to implement:**

1. If not already a git repo with an initial commit, make one first: `git
   add` the existing untracked files (`.gitignore`, `Cargo.lock`,
   `Cargo.toml`, `DESIGN.md`, `README.md`, `demo.py`, `lines/`,
   `migrations/`, `mise.local.toml`, `plans/`, `src/`, `tests/`) and commit
   as the baseline before restructuring, so the workspace conversion is a
   reviewable diff rather than indistinguishable from "no history."
2. Rewrite root `Cargo.toml` as a workspace manifest:
   ```toml
   [workspace]
   resolver = "2"
   members = [
       "crates/common",
       "crates/api",
       "crates/poller-incidents",
       "crates/poller-stations",
       "crates/poller-tocs",
   ]
   ```
3. Move existing `src/` into `crates/api/src/` using `git mv` (preserve
   history — do not delete+recreate). `crates/api/Cargo.toml` keeps `axum`,
   `sqlx`, `tokio`, `clap`, `dotenv`, `tower`, `tower-http`, `tracing`,
   `tracing-subscriber`, `anyhow`, `toml`, `glob`, plus a path dependency on
   `common`.
4. Create `crates/common/src/lib.rs` and move into it, unchanged in
   substance except where noted:
   - `types.rs` → `Severity` (as-is).
   - `dataclasses.rs` → `DataQuality`, `ValidityPeriod`, `AffectedRoute`,
     `Disruption`, `LineStatus`, `LineStatusReport`, `StationDeparture`,
     `HealthStatus` (as-is).
   - `data/lines.rs` → `Station`, `LineDefinition` (as-is — `api` still owns
     loading it from the `lines/` TOML dir at startup; the type is shared in
     case a poller ever needs it later).
   - `data/database.rs` → `IncidentMessage`, `StationSample`, **with these
     fixes** (the old shape was never wired in — this is a clean rename,
     not a breaking migration):
     ```rust
     pub struct IncidentMessage {
         pub incident_id: String,            // maps IncidentNumber
         pub summary: String,
         pub description: String,
         pub operators: Vec<String>,         // ATOC codes, flattened from Affects.Operators.AffectedOperator[].OperatorRef
         pub affected_stations: Vec<String>, // left empty by pollers — no CRS field exists in the Incidents schema, only free-text RoutesAffected
         pub priority: i32,                  // raw IncidentPriority integer — no documented enum, do not re-invent "major"/"minor"
         pub validity: Vec<ValidityPeriod>,  // schema allows repeated ValidityPeriod, not a single from/to pair
         pub is_planned: bool,               // maps Planned
         pub is_cleared: bool,               // maps ClearedIncident (spec: feed retains cleared incidents for a time)
     }
     ```
   - **New** `StationReference` struct: `crs: String`, `name: String`,
     `latitude: Option<f64>`, `longitude: Option<f64>`,
     `station_operator: Option<String>`, `accessibility: serde_json::Value`
     (JSONB passthrough — see Global Constraint 7).
   - **New** `TocReference` struct: `atoc_code: String`, `name: String`,
     `legal_name: String`, `atoc_member: Option<bool>`,
     `station_operator: Option<bool>`.
5. Update `crates/api`'s `use` paths from `crate::types::...` /
   `crate::dataclasses::...` to `common::...`.
6. `data/mod.rs` in `crates/api` keeps `pub mod config;` (ServiceArguments
   stays API-specific) and re-exports the line-catalogue types via
   `pub use common::{Station, LineDefinition};` so `main.rs`/`data/config.rs`
   keep working with the smallest diff.
7. Create `crates/poller-incidents`, `crates/poller-stations`,
   `crates/poller-tocs` now as minimal stub binary crates: a `Cargo.toml`
   depending on `common` (path dep) plus `tokio` (rt-multi-thread, macros),
   and a `src/main.rs` with a `#[tokio::main] async fn main() { println!("stub"); }`.
   This keeps `cargo build --workspace` meaningful from this task onward;
   Tasks 3-5 replace the stub bodies with real implementations — don't add
   `reqwest`/XML parsing dependencies yet, that belongs to those tasks.

**Anti-pattern guards:**
- Don't invent new dependencies beyond what's listed — this task is a move
  + type-fix + stub scaffold, not a feature add.
- Don't wire the ingestion endpoints yet — that's Task 2. This task ends
  with the API serving exactly what it serves today (`/public/health`),
  just from a workspace layout.
- Don't implement any real poller logic in the three stub crates.

**Verification checklist:**
- `cargo build --workspace` succeeds (all 5 crates, including the 3 stubs).
- `cargo run --bin api` still serves `GET /public/health` → `{"message":
  "Alive"}`, unchanged from before.
- `git log --follow crates/api/src/main.rs` shows the pre-move history
  (confirms `git mv` was used, not delete+recreate).

---

## Task 2: Reference-data migrations + ingestion endpoints on the API

**Depends on:** Task 1 (needs the `common` crate's `IncidentMessage`,
`StationReference`, `TocReference` types and the `crates/api` layout).

**Note from Task 1's review:** the migrations directory now lives at
`crates/api/migrations/` (moved there in Task 1 because `sqlx::migrate!()`
resolves relative to `CARGO_MANIFEST_DIR`, which is now `crates/api/`, not
the repo root). All migration paths below are relative to that directory.

**Also from Task 1's review — a pre-existing bug you must fix as part of
this task:** `crates/api/migrations/20260510023522_initial.sql`'s
`incidents_active` index is `CREATE INDEX incidents_active ON incidents
(valid_from) WHERE valid_to IS NULL OR valid_to > NOW();`. Postgres rejects
`NOW()` in a partial index predicate ("functions in index predicate must be
marked IMMUTABLE") — confirmed by directly running this DDL against a real
Postgres 16 instance. Since this task already alters the `incidents` table's
columns (dropping `valid_to` entirely in favor of `validity_periods
JSONB`), fold the fix into that same migration: `DROP INDEX
incidents_active;` before the column changes (you cannot drop a column an
index depends on without dropping the index first), and do not recreate an
equivalent partial index on `validity_periods` — querying "active"
incidents against a JSONB array is a query-time concern, not an index
predicate; leave that to whatever later work adds the read endpoints.

**What to implement:**

1. New migration `crates/api/migrations/<timestamp>_reference_data.sql`:
   ```sql
   CREATE TABLE stations (
       crs               CHAR(3)     PRIMARY KEY,
       name              TEXT        NOT NULL,
       latitude          DOUBLE PRECISION,
       longitude         DOUBLE PRECISION,
       station_operator  TEXT,
       accessibility     JSONB       NOT NULL DEFAULT '{}',
       fetched_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );

   CREATE TABLE tocs (
       atoc_code        CHAR(2)     PRIMARY KEY,
       name             TEXT        NOT NULL,
       legal_name       TEXT        NOT NULL,
       atoc_member      BOOLEAN,
       station_operator BOOLEAN,
       fetched_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   ```
   No history tables for these two (Global Constraint 6).
2. Alter the existing `incidents` table to match the Task-1
   `IncidentMessage` shape: drop the old `severity_hint TEXT` and
   `valid_from`/`valid_to TIMESTAMPTZ` columns, add `priority INTEGER NOT
   NULL`, `validity_periods JSONB NOT NULL DEFAULT '[]'` (array of `{start,
   end}` objects mirroring `Vec<ValidityPeriod>`), and `is_cleared BOOLEAN
   NOT NULL DEFAULT FALSE`. This is schema-breaking but the table holds no
   real data yet, so do it now rather than carrying the mismatch forward.
3. In `crates/api`, add real query functions (this is where
   `data/database.rs`'s previously-dead code becomes live) —
   `upsert_incidents(&PgPool, &[IncidentMessage])`, `upsert_stations(&PgPool,
   &[StationReference])`, `upsert_tocs(&PgPool, &[TocReference])`, each
   doing a batch `INSERT ... ON CONFLICT (pk) DO UPDATE`. Incidents
   additionally insert into `incident_history` only when summary,
   description, or validity_periods differ from the currently-stored row
   (compare before overwriting; matches the migration file's own documented
   intent for that table).
4. Add a `tower::Layer` that checks a shared-secret header (`X-Internal-Token`)
   against a new `internal_token: String` field on `ServiceArguments`, using
   a constant-time compare (e.g. via the `subtle` crate, or hand-rolled
   fixed-time byte comparison — do not use `==` on the raw strings). Apply
   this layer only to `private_router()` in `src/routes/mod.rs` —
   `public_router()` stays unauthenticated.
5. Add ingestion handlers under `crates/api/src/routes/ingest.rs`:
   - `POST /private/incidents` — body `Vec<IncidentMessage>`, calls
     `upsert_incidents`, returns `200` with `{"upserted": N}`.
   - `POST /private/stations` — body `Vec<StationReference>`, calls
     `upsert_stations`, same response shape.
   - `POST /private/tocs` — body `Vec<TocReference>`, calls `upsert_tocs`,
     same response shape.
   - Rely on axum's `Json<T>` extractor for body validation/deserialization
     errors (don't hand-roll that).

**Anti-pattern guards:**
- Don't build a generic auth framework — one header, one constant-time
  compare, one layer, applied at the router-nesting level.
- Don't add pagination/streaming to the ingestion endpoints — a single
  `Vec<T>` JSON body per poll cycle is fine at this data scale.

**Verification checklist:**
- `sqlx migrate run` applies cleanly against a local Postgres (spin one up
  via `docker run postgres` or equivalent if none is running).
- `curl -X POST localhost:8080/private/incidents -H "X-Internal-Token: $TOKEN" -H 'Content-Type: application/json' -d '[...]'`
  round-trips: row appears in `incidents`; re-posting the same
  `incident_id` with a changed `summary` produces exactly one new
  `incident_history` row, not a duplicate `incidents` row.
- Same curl check for `/private/stations` and `/private/tocs`.
- A request with a missing/wrong `X-Internal-Token` gets `401`; `GET
  /public/health` still works with no token required.

---

## Task 3: `poller-incidents` crate

**Depends on:** Task 1 (stub crate + `common` types), Task 2 (the
`/private/incidents` endpoint it POSTs to).

**RDM facts for this task (from RSPS5050 P-03-00 Rev A, §10, verbatim):**

- **Poll frequency (confirmed):** "Recommend every 5 minutes."
- **RDM endpoint path: GAP — not published in the current spec** (only the
  legacy NRE display page and the XSD filename `nre-incident-v5-0.xsd` are
  given). Make the base URL a required env var with no default — do not
  guess a path.
- **Auth header: `x-apikey: <key>`** — corroborated for the RDM platform
  generally (via a different product's confirmed example), not proven
  specifically for this product. Isolate the header name behind one
  constant so it's a one-line fix if wrong.
- **XML schema, root `Incidents` → `PtIncident[]`:**
  - `IncidentNumber` (String(32), mandatory) — unique ID, e.g.
    `'8B68D83E08C1415A906022178722BDCB'`.
  - `Summary` (String, mandatory), `Description` (String, mandatory).
  - `Planned` (Boolean, mandatory).
  - `ClearedIncident` (Boolean, optional, default treat-as-false-if-absent).
  - `ValidityPeriod` (**mandatory AND repeatable** — can occur more than
    once) → `StartTime` (mandatory dateTime), `EndTime` (optional dateTime
    — absent means "until further notice").
  - `Affects.Operators.AffectedOperator[]` → `OperatorRef` (String(2) ATOC
    code, mandatory), `OperatorName` (optional).
  - `Affects.RoutesAffected` — free-text String. **No structured CRS/station
    code field exists anywhere in this schema.**
  - `IncidentPriority` (Integer, mandatory) — no documented value table.
    Store the raw integer; do not invent a "major"/"minor" mapping.

**What to implement:**

1. `crates/poller-incidents/Cargo.toml`: add `reqwest` (json + native-tls
   features), `quick-xml` (with `serde` feature), `chrono`, `anyhow`,
   `tracing`, `tracing-subscriber`, `dotenv`, `clap` (derive, env) to the
   existing stub crate's deps.
2. Config (clap, `#[arg(env)]`): `rdm_incidents_base_url: String` (env
   `RDM_INCIDENTS_BASE_URL`, **no default — this is the documented gap
   above**), `rdm_api_key: String` (env `RDM_API_KEY`), `api_ingest_url:
   String` (default `http://api:8080/private/incidents`), `internal_token:
   String` (env `INTERNAL_TOKEN`), `poll_interval_secs: u64` (default `300`
   — the confirmed 5-min recommendation).
3. Poll loop (`tokio::time::interval`): `GET {base_url}` with header
   `x-apikey: {key}`, deserialize the `Incidents` → `PtIncident[]` XML body
   via `quick-xml`'s serde support, map each `PtIncident` to
   `common::IncidentMessage`:
   - `incident_id` ← `IncidentNumber`
   - `summary`/`description` ← `Summary`/`Description`
   - `operators` ← flatten `Affects.Operators.AffectedOperator[].OperatorRef`
   - `affected_stations` ← `vec![]` (no schema backing — do not parse
     `RoutesAffected` free text; that CRS-extraction gap is a separate,
     already-flagged DESIGN.md item, not this task's job)
   - `priority` ← `IncidentPriority` (raw integer, no reinterpretation)
   - `validity` ← map every `ValidityPeriod` entry present
   - `is_planned` ← `Planned`
   - `is_cleared` ← `ClearedIncident` (default `false` if absent)
4. POST the resulting `Vec<IncidentMessage>` as JSON to `api_ingest_url`
   with header `X-Internal-Token: {internal_token}`. Log success/failure
   counts via `tracing`. On HTTP or parse failure: log the error and keep
   the loop alive — don't crash the process over one bad poll.
5. `docker/poller-incidents.Dockerfile`: multi-stage — builder `FROM
   rust:<pin the version matching this workspace's edition 2024
   requirement> AS builder`, `COPY . .`, `cargo build --release --bin
   poller-incidents`; runtime `FROM debian:bookworm-slim`, copy just the
   compiled binary, set `ENTRYPOINT`.

**Anti-pattern guards:**
- Do not give `RDM_INCIDENTS_BASE_URL` a default value — a misconfiguration
  should fail loudly at startup, not silently poll the wrong thing.
- Do not implement any severity/major-minor interpretation of `priority` —
  store the raw integer only.
- Do not attempt CRS/station extraction from `RoutesAffected` text.

**Verification checklist:**
- Unit test: feed a hand-written sample XML string (using the field names
  and the `IncidentNumber` example value above) through the parser, assert
  the mapped `IncidentMessage` fields match exactly.
- If real RDM credentials are available (the user has them — ask if the
  base URL has been supplied yet; if not, this step waits): one live run
  against the real endpoint, confirm a successful POST to a locally-running
  `api` and a visible row in `incidents`.
- `docker build -f docker/poller-incidents.Dockerfile .` succeeds and
  `docker run` (with required env vars set) starts polling without
  panicking.

---

## Task 4: `poller-stations` crate

**Depends on:** Task 1, Task 2 (the `/private/stations` endpoint).

**RDM facts for this task (from RSPS5050 P-03-00 Rev A, §6, verbatim —
this is the best-documented of the three products):**

- **Poll frequency (confirmed):** "updated overnight; Poll frequency should
  only be once every 24 hours."
- **RDM endpoint (confirmed):** URL convention `/json/1.0/stations/{crs}`.
  Endpoints:
  - `GET /stations` — all stations (spec warns: "large payload").
  - `GET /stations/{crs}` — one station by 3-char CRS.
  - `GET /stations/tocs/{toc}` — stations run by a 2-char TOC code (not
    needed for this task — use `/stations`).
- **Auth:** "An API Key will be required to access the JSON feed via RDM" —
  header name not stated for this specific product; use the `x-apikey`
  working assumption (same as Task 3), isolated behind one constant.
- **Fields (medium confidence — documented via the sibling XML schema; JSON
  casing itself is unconfirmed):** `CrsCode`, `Name`, `Longitude`/
  `Latitude`, `StationOperator` (2-char TOC), `Accessibility` (an object
  containing ~14 sub-fields like `Helpline`, `InductionLoop: bool`,
  `AccessibleTicketMachines`, `RampForTrainAccess`,
  `StepFreeAccess.Coverage` — do not model these individually, see below).

**What to implement:**

1. `crates/poller-stations/Cargo.toml`: add `reqwest`, `serde_json`,
   `anyhow`, `tracing`, `tracing-subscriber`, `dotenv`, `clap` to the stub
   crate.
2. Config: `rdm_stations_base_url: String` (env `RDM_STATIONS_BASE_URL`,
   no hardcoded default — the host portion is account-specific even though
   the path suffix `/stations` is confirmed), `rdm_api_key`,
   `api_ingest_url` (default `http://api:8080/private/stations`),
   `internal_token`, `poll_interval_secs` (default `86400`).
3. Poll loop: `GET {base}/stations` with `x-apikey` header. **On the first
   real run, log the raw response body once at `debug` level before
   parsing** — this is how you resolve the JSON-casing gap; inspect the
   logged body and set `#[serde(rename_all = "...")]` (or per-field
   `rename`) on the intermediate deserialize struct to match observed
   reality. Do not guess camelCase vs PascalCase upfront and ship it
   unverified.
4. Map to `common::StationReference`: `crs` ← `CrsCode`, `name` ← `Name`,
   `latitude`/`longitude` ← `Latitude`/`Longitude`, `station_operator` ←
   `StationOperator`, `accessibility` ← re-serialize the `Accessibility`
   sub-object verbatim as `serde_json::Value` (JSONB passthrough — Global
   Constraint 7, do not hand-model the ~14 sub-fields).
5. POST the batch to `api_ingest_url` with `X-Internal-Token`. Log the row
   count (the spec's own "large payload" warning applies — a station list
   is a few thousand rows, which is fine as one POST body).
6. `docker/poller-stations.Dockerfile`, same pattern as Task 3.

**Anti-pattern guards:**
- Don't hardcode a guessed JSON casing scheme without the "log raw body
  once, then set rename" verification step.
- Don't model the accessibility sub-fields individually.

**Verification checklist:**
- One real poll against the account's actual endpoint (ask the user if the
  RDM key/host is ready), confirm the raw logged body parses without
  silent field loss — spot-check a known station, e.g. `EUS`, against the
  spec's own worked example path `/stations/eus`.
- Confirm the `stations` table populates with plausible lat/long and a
  non-empty `accessibility` JSONB for at least one station.
- `docker build -f docker/poller-stations.Dockerfile .` succeeds.

---

## Task 5: `poller-tocs` crate

**Depends on:** Task 1, Task 2 (the `/private/tocs` endpoint).

**RDM facts for this task (from RSPS5050 P-03-00 Rev A, §3, verbatim):**

- **Poll frequency (confirmed):** "At least once every 24 hours."
- **RDM endpoint path: GAP — not published in the current spec edition**
  (even the legacy internal-only URL from the 2017 edition was removed).
  Make the base URL a required env var with no default.
- **Auth:** same `x-apikey` working assumption as Tasks 3-4, isolated
  behind one constant.
- **XML schema, root `TrainOperatingCompanyList` →
  `TrainOperatingCompany[]`:** `AtocCode` (String(2), mandatory, e.g.
  `'LE'`), `Name` (String, mandatory — brand name, e.g. `'Greater
  Anglia'`), `LegalName` (String, mandatory, e.g. `'London Eastern
  Railways'`), `AtocMember` (Boolean), `StationOperator` (Boolean). Fields
  present in the schema but **not needed downstream** — do not model them:
  `ManagingDirector`, `Logo`, `NetworkMap`, `CompanyWebsite`, contact-detail
  structures.

**What to implement:** structurally identical to Task 3 (XML poller crate),
but:

1. `crates/poller-tocs/Cargo.toml`: same additions as Task 3
   (`reqwest`, `quick-xml` with serde, `anyhow`, `tracing`,
   `tracing-subscriber`, `dotenv`, `clap`).
2. Config: `rdm_tocs_base_url: String` (env `RDM_TOCS_BASE_URL`, no
   default), `rdm_api_key`, `api_ingest_url` (default
   `http://api:8080/private/tocs`), `internal_token`, `poll_interval_secs`
   (default `86400`).
3. Poll loop: `GET {base_url}` with `x-apikey` header, parse
   `TrainOperatingCompanyList` → `TrainOperatingCompany[]`, map
   `AtocCode`/`Name`/`LegalName`/`AtocMember`/`StationOperator` to
   `common::TocReference`.
4. POST batch to `api_ingest_url` with `X-Internal-Token`.
5. `docker/poller-tocs.Dockerfile`, same pattern as Task 3.

**Anti-pattern guards:** same as Task 3 — no invented base URL default, no
extra fields beyond what's actually consumed by `common::TocReference`.

**Verification checklist:**
- Unit test: hand-written sample XML using the spec's own example values
  (`AtocCode: 'LE'`, `Name: 'Greater Anglia'`, `LegalName: 'London Eastern
  Railways'`), assert correct mapping.
- Live run once the base URL is known (ask the user).
- `docker build -f docker/poller-tocs.Dockerfile .` succeeds.

---

## Task 6: Local integration (docker-compose) + end-to-end verification

**Depends on:** Tasks 1-5 (needs all four Dockerfiles and the API's
ingestion endpoints).

**What to implement:**

1. `docker-compose.yml` at repo root: `postgres` (official image, volume for
   data), `api` (build from `docker/api.Dockerfile` — Task 1/2 need to add
   this Dockerfile too, following the same multi-stage pattern as the
   pollers; depends_on postgres; env: `DATABASE_URL`, `BIND_URL`,
   `INTERNAL_TOKEN`), `poller-incidents`, `poller-stations`, `poller-tocs`
   (each depends_on `api`; share `INTERNAL_TOKEN` plus their own `RDM_*` env
   vars).
2. `.env.example` at repo root documenting every required var across all 5
   services (do not commit real API keys — they go in a git-ignored
   `.env`; confirm `.env` is in `.gitignore`).

**Anti-pattern guards:**
- Don't add a k8s manifest here — out of scope per Global Constraint 1.
- Don't bake secrets into the compose file or any Dockerfile.

**Verification checklist (final gate for this plan):**
- `docker compose up` brings up all 5 containers healthy.
- Using a temporarily-shortened `POLL_INTERVAL_SECS` for the two 24h-cadence
  pollers (stations, tocs), confirm all three tables (`incidents`,
  `stations`, `tocs`) get rows within a couple of poll cycles.
- `GET /public/health` still responds `200`.
- Re-run `docker compose up` a second time (simulating a restart) and
  confirm upserts don't duplicate rows (primary-key conflict path exercised
  for real, not just asserted).
- Report which of the plan's flagged "gaps" (Incidents/TOCs base URL,
  exact auth header name, `IncidentPriority` meaning, Stations JSON casing)
  were resolved with real account data during execution vs. which remain
  open placeholders — do not report the plan as fully verified if any gap
  is still a placeholder.
