# LDBWS Sampler Poller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a fourth poller (`poller-ldbws`) that samples live departure-board
data per station and stores it in the already-existing `station_samples`
table, giving DESIGN.md's aggregator inference layer (§6.2) real data to run
on.

**Architecture:** Two new endpoints on the existing `api` crate
(`GET /private/sample-stations`, computed from the already-loaded line
catalogue; `POST /private/station-samples`, upserting into the existing
`station_samples` table) plus a new poller crate that calls the first
endpoint to learn which stations to sample, makes one RDM
`GetDepBoardWithDetails` call per station per cycle, and POSTs the batch to
the second endpoint.

**Tech Stack:** Rust (existing Cargo workspace), axum, sqlx/Postgres,
reqwest, serde/serde_json, chrono, clap. No new external dependencies
beyond what the other three pollers already use.

## Global Constraints

- **No invented API details.** Every RDM field name below is transcribed
  from a Swagger 2.0 spec fetched and parsed directly during planning
  (`GetDepBoardWithDetails` operation, RDM's Live Departure Board product).
  Two facts are genuinely unconfirmed and MUST stay env-configurable, not
  guessed: the exact RDM product-slug segment of the base URL (two variants
  seen in research: `1010-live-departure-board-dep` vs `...-dep1_2`), and
  this feed's real rate limit (one low-trust source claims "5M requests /
  4-week period" — do not hardcode this as a real limit anywhere).
- **`headcode` is always `None`.** Confirmed absent from this API's entire
  schema (not merely undocumented — searched every response definition).
  Do not invent a substitute from `rsid` or `serviceID` (both are
  confirmed to be something else: `rsid` is a distinct Retail Service ID,
  `serviceID` is an opaque token for chaining into `GetServiceDetails`, not
  a Darwin headcode/trainid).
- **Only `trainServices` are sampled.** `busServices`/`ferryServices`
  (rail-replacement) are out of scope for this plan.
- **`operator` on `StationDeparture` must be the ATOC code
  (`operatorCode`, e.g. `"GW"`), not the display name (`operator`, e.g.
  `"Great Western Railway"`).** The aggregator's future inference logic
  (DESIGN.md §6.2) filters departures by `line.operators`, which are ATOC
  codes — populating the display name here would silently break that
  matching later. This is a real design decision, not a style choice; get
  it right now.
- **`delay_minutes` must be computed, not read from a field** — no such
  field exists in the schema. `std`/`etd` are both time-of-day strings;
  compute the difference, handling midnight wraparound, and return `0`
  whenever `etd` isn't itself a valid time (e.g. `"On time"`, `"Delayed"`,
  `"Cancelled"` — status words, not guessable as an exhaustive enum since
  the schema types `etd` as a bare string with no `enum`).
- **No history table for `station_samples`** — it's wholesale-replaced per
  poll (`ON CONFLICT (crs) DO UPDATE`), matching the existing table's
  design and the same convention already used for `stations`/`tocs`.
- **Deployment: Dockerfile only, no Kubernetes manifests** (out of scope,
  matches the rest of this project).
- **Ingestion path: through the existing `api` crate's `private_router()`**,
  gated by the same `X-Internal-Token` check as `/private/{incidents,
  stations,tocs}` — no separate service.
- A single station's LDBWS call failing must not abort the whole poll
  cycle — log it, skip that station, and POST whatever samples succeeded
  from the other stations. This is routine (per-station transient errors
  are expected), not exceptional.

---

## Current relevant code (read before starting; exact signatures below are
verified against the real files, not reconstructed from memory)

**`crates/common/src/lib.rs`** already has (unchanged by this plan):
```rust
pub struct StationDeparture {
    pub service_id: String,
    pub operator: String,
    pub destination_crs: String,
    pub scheduled: String,
    pub estimated: String,
    pub is_cancelled: bool,
    pub delay_minutes: i32,
    pub cancel_reason: Option<String>,
    pub delay_reason: Option<String>,
    pub headcode: Option<String>,
}

pub struct StationSample {
    pub crs: String,
    pub polled_at: DateTime<Utc>,
    pub departures: Vec<StationDeparture>,
}

pub struct LineDefinition {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub category: String,
    pub operators: Vec<String>,
    pub stations: Vec<Station>,
    pub sample_stations: Vec<String>,
    pub match_keywords: Vec<String>,
    pub excluded_keywords: Vec<String>,
    pub severity_overrides: HashMap<String, Severity>,
    pub exclusive_segments: Vec<String>,
    pub destination_crs_filter: Vec<String>,
    pub headcode_prefixes: Vec<String>,
}
```

**`crates/common/src/ingest.rs`** already has:
```rust
pub const INTERNAL_TOKEN_HEADER: &str = "x-internal-token";
pub const RDM_AUTH_HEADER_NAME: &str = "x-apikey";
pub async fn post_batch<T: Serialize>(
    client: &reqwest::Client, url: &str, internal_token: &str, items: &[T], noun: &str,
) -> anyhow::Result<()>
```

**`crates/api/migrations/20260510023522_initial.sql`** already has:
```sql
CREATE TABLE station_samples (
    crs        CHAR(3)     PRIMARY KEY,
    polled_at  TIMESTAMPTZ NOT NULL,
    departures JSONB       NOT NULL DEFAULT '[]'
);
```
No migration changes needed — this table already matches `StationSample`.

**`crates/api/src/data/config.rs`**'s `ServiceArguments.lines: Vec<LineDefinition>`
is populated at startup from `--lines-dir` (no `env` attribute currently,
and no `default_value` — clap treats untagged `Vec<T>` fields as 0-or-more,
defaulting to an empty `Vec` when absent, which is why the deployed stack
currently runs with an empty line catalogue).

**`crates/api/src/routes/ingest.rs`** currently has three handlers
(`post_incidents`, `post_stations`, `post_tocs`), each following this exact
shape:
```rust
async fn post_stations(
    State(app): State<App>,
    Json(stations): Json<Vec<StationReference>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stations(&app.database, &stations)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```
with `UpsertResponse { upserted: u64 }` and a shared `internal_error` helper
already defined in that file.

**`crates/api/src/data/queries.rs`**'s `upsert_stations` (the closest
existing precedent — reference-data table, no history) is:
```rust
pub async fn upsert_stations(pool: &PgPool, stations: &[StationReference]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    for station in stations {
        sqlx::query(
            r#"
            INSERT INTO stations (crs, name, latitude, longitude, station_operator, accessibility, fetched_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (crs) DO UPDATE SET
                name             = EXCLUDED.name,
                latitude         = EXCLUDED.latitude,
                longitude        = EXCLUDED.longitude,
                station_operator = EXCLUDED.station_operator,
                accessibility    = EXCLUDED.accessibility,
                fetched_at       = NOW()
            "#,
        )
        .bind(&station.crs)
        .bind(&station.name)
        .bind(station.latitude)
        .bind(station.longitude)
        .bind(&station.station_operator)
        .bind(&station.accessibility)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}
```
No automated test exists for this function (or `upsert_tocs`) — the
established convention in this codebase is manual `curl` verification
against a real Postgres instance for DB-touching upsert functions (only the
pure `incident_changed` diff-check has a unit test, since it needs no DB).
This plan follows that same convention for `upsert_station_samples`.

**`crates/api/src/routes/mod.rs`** currently is:
```rust
use axum::middleware;
use crate::app::{App, Router};
use crate::auth::require_internal_token;

pub mod health;
pub mod ingest;

pub fn public_router() -> Router {
    Router::new().merge(health::router())
}

pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .layer(middleware::from_fn_with_state(app, require_internal_token))
}
```

**`crates/api/src/data/mod.rs`** currently is:
```rust
pub mod config;
pub mod queries;

pub use common::{LineDefinition, Station};
```

**`docker/poller-stations.Dockerfile`** (the closest existing template —
JSON-based like this new poller, not XML) is the exact multi-stage/non-root
pattern this plan's new Dockerfile copies, substituting the binary name.

**`docker-compose.yml`**'s `poller-stations` service block is the template
for this plan's new `poller-ldbws` service block.

**`lines/*.toml`**'s current `sample_stations` values (verified by reading
every file — used as the exact expected output in Task 2's verification
step):
```
swr-portsmouth-direct: GLD, HSL, PMS
swr-south-west-main:   WIN, SOU, BMH
thameslink-core:       STP, ZFD, LBG
swr-alton:             AHT, FRM, AON
west-coast-main-line:  EUS, MKC, CRE, PRE, CAR
```
Deduplicated + sorted union (17 stations, no overlaps today):
`AHT, AON, BMH, CAR, CRE, EUS, FRM, GLD, HSL, LBG, MKC, PMS, PRE, SOU, STP, WIN, ZFD`

**RDM Live Departure Board facts** (from a Swagger 2.0 spec fetched and
parsed directly during planning — `GetDepBoardWithDetails` operation),
exact field names verbatim from the `definitions` block:
```
StationBoardWithDetails: { trainServices: [ServiceItemWithCallingPoints], ... }
ServiceItemWithCallingPoints: {
    serviceID: string        (opaque token, NOT a headcode)
    operator: string         (display name — do not use)
    operatorCode: string     (ATOC code, e.g. "GW" — use this)
    destination: [ServiceLocation]
    std: string              (scheduled departure, "HH:MM")
    etd: string              ("HH:MM" | "On time" | "Delayed" | "Cancelled" | ...; untyped string, no enum)
    isCancelled: boolean
    cancelReason: string | null
    delayReason: string | null
}
ServiceLocation: { crs: string, locationName: string, ... }
```
Response for `GET /api/20220120/GetDepBoardWithDetails/{crs}?numRows=N` is
`StationBoardWithDetails` directly — no envelope wrapper. No bulk/multi-CRS
endpoint exists; one call per station is required.

---

## Task 1: `POST /private/station-samples` ingestion endpoint

**Files:**
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/ingest.rs`

**Interfaces:**
- Consumes: `common::StationSample` (existing, unchanged)
- Produces: `pub async fn upsert_station_samples(pool: &PgPool, samples: &[StationSample]) -> Result<u64>`
  (in `queries.rs`); route `POST /station-samples` added to
  `ingest::router()`, reachable as `/private/station-samples` once merged
  under `private_router()`.

- [ ] **Step 1: Add `upsert_station_samples` to `crates/api/src/data/queries.rs`**

Add this import at the top of the file, alongside the existing
`common::{IncidentMessage, StationReference, TocReference}` import — change
that line to:
```rust
use common::{IncidentMessage, StationReference, StationSample, TocReference};
```

Add this function after `upsert_stations` (before `upsert_tocs`):
```rust
/// Upserts a batch of station samples (LDBWS departure-board snapshots).
/// No history — this is a point-in-time sample, wholesale-replaced per
/// poll, same rationale as `upsert_stations`/`upsert_tocs`.
pub async fn upsert_station_samples(pool: &PgPool, samples: &[StationSample]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let departures_json = serde_json::to_value(&sample.departures)?;

        sqlx::query(
            r#"
            INSERT INTO station_samples (crs, polled_at, departures)
            VALUES ($1, $2, $3)
            ON CONFLICT (crs) DO UPDATE SET
                polled_at  = EXCLUDED.polled_at,
                departures = EXCLUDED.departures
            "#,
        )
        .bind(&sample.crs)
        .bind(sample.polled_at)
        .bind(&departures_json)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p api`
Expected: builds with no errors (existing `#[cfg(test)] mod tests` at the
bottom of the file is untouched and unaffected by this addition).

- [ ] **Step 3: Add the route handler to `crates/api/src/routes/ingest.rs`**

Change the `use common::{...}` import line to:
```rust
use common::{IncidentMessage, StationReference, StationSample, TocReference};
```

Add `"/station-samples"` to the router in `router()`:
```rust
pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::post(post_incidents))
        .route("/stations", axum::routing::post(post_stations))
        .route("/tocs", axum::routing::post(post_tocs))
        .route("/station-samples", axum::routing::post(post_station_samples))
}
```

Add the handler after `post_stations`:
```rust
async fn post_station_samples(
    State(app): State<App>,
    Json(samples): Json<Vec<StationSample>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_station_samples(&app.database, &samples)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p api`
Expected: builds with no errors.

- [ ] **Step 5: Manual verification against a real Postgres**

Start a throwaway Postgres and run migrations (adjust host port if 55432 is
taken):
```bash
docker run -d --rm --name nrstatus-verify -e POSTGRES_PASSWORD=postgres -p 55432:5432 postgres:16
sleep 3
DATABASE_URL=postgres://postgres:postgres@localhost:55432/postgres \
BIND_URL=127.0.0.1:18080 \
INTERNAL_TOKEN=test-secret-token \
cargo run -p api &
sleep 3
```

Round-trip a sample:
```bash
curl -s -w '\nstatus=%{http_code}\n' -X POST http://127.0.0.1:18080/private/station-samples \
  -H 'X-Internal-Token: test-secret-token' -H 'Content-Type: application/json' \
  -d '[{
    "crs": "PAD",
    "polled_at": "2026-07-06T10:00:00Z",
    "departures": [{
      "service_id": "abc==",
      "operator": "GW",
      "destination_crs": "RDG",
      "scheduled": "10:00",
      "estimated": "10:05",
      "is_cancelled": false,
      "delay_minutes": 5,
      "cancel_reason": null,
      "delay_reason": null,
      "headcode": null
    }]
  }]'
```
Expected: `{"upserted":1}` and `status=200`.

Confirm the row landed correctly:
```bash
docker exec nrstatus-verify psql -U postgres -c \
  "SELECT crs, polled_at, jsonb_array_length(departures) FROM station_samples;"
```
Expected: one row, `crs=PAD`, `jsonb_array_length=1`.

Clean up:
```bash
kill %1
docker stop nrstatus-verify
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs
git commit -m "Add POST /private/station-samples ingestion endpoint"
```

---

## Task 2: `GET /private/sample-stations` endpoint + line-catalogue Docker wiring

**Files:**
- Create: `crates/api/src/data/samples.rs`
- Create: `crates/api/src/routes/samples.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/routes/mod.rs`
- Modify: `crates/api/src/data/config.rs`
- Modify: `docker/api.Dockerfile`

**Interfaces:**
- Consumes: `common::LineDefinition` (existing, unchanged)
- Produces: `pub fn dedup_sample_stations(lines: &[LineDefinition]) -> Vec<String>`
  (in `data::samples`); route `GET /sample-stations` under
  `private_router()`, returning a JSON array of CRS strings.

- [ ] **Step 1: Write the failing test for the dedup function**

Create `crates/api/src/data/samples.rs`:
```rust
//! Pure logic for computing which stations `poller-ldbws` should sample,
//! independent of any HTTP/DB concern so it's testable without either.

use common::LineDefinition;
use std::collections::BTreeSet;

/// Deduplicated, sorted union of every line's `sample_stations` CRS codes.
/// Sorted so the returned list (and therefore `poller-ldbws`'s poll order)
/// is deterministic across runs, not dependent on `Vec<LineDefinition>`
/// iteration order.
pub fn dedup_sample_stations(lines: &[LineDefinition]) -> Vec<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_with_samples(id: &str, sample_stations: &[&str]) -> LineDefinition {
        LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: vec![],
            stations: vec![],
            sample_stations: sample_stations.iter().map(|s| s.to_string()).collect(),
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
        }
    }

    #[test]
    fn empty_lines_produce_empty_list() {
        assert_eq!(dedup_sample_stations(&[]), Vec::<String>::new());
    }

    #[test]
    fn single_line_returns_its_stations_sorted() {
        let lines = vec![line_with_samples("wcml", &["EUS", "MKC", "BHM"])];
        assert_eq!(dedup_sample_stations(&lines), vec!["BHM", "EUS", "MKC"]);
    }

    #[test]
    fn overlapping_stations_across_lines_are_deduplicated() {
        let lines = vec![
            line_with_samples("swr-main", &["WAT", "WOK", "BSK"]),
            line_with_samples("swr-portsmouth", &["WAT", "WOK", "PMH"]),
        ];
        assert_eq!(
            dedup_sample_stations(&lines),
            vec!["BSK", "PMH", "WAT", "WOK"]
        );
    }
}
```

Add `pub mod samples;` to `crates/api/src/data/mod.rs`:
```rust
pub mod config;
pub mod queries;
pub mod samples;

pub use common::{LineDefinition, Station};
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p api dedup_sample_stations`
Expected: compile error or panic from the `todo!()` (e.g. `not yet
implemented`) on all three new tests.

- [ ] **Step 3: Implement `dedup_sample_stations`**

Replace the `todo!()` body:
```rust
pub fn dedup_sample_stations(lines: &[LineDefinition]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for line in lines {
        for crs in &line.sample_stations {
            set.insert(crs.clone());
        }
    }
    set.into_iter().collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p api dedup_sample_stations`
Expected: `test result: ok. 3 passed; 0 failed`.

- [ ] **Step 5: Add the route handler**

Create `crates/api/src/routes/samples.rs`:
```rust
//! Read-only endpoint exposing which stations `poller-ldbws` should
//! sample, computed from the line catalogue already loaded into
//! `AppState` at startup (see `crates/api/src/data/config.rs`).

use axum::Json;
use axum::extract::State;

use crate::app::{App, Router};
use crate::data::samples::dedup_sample_stations;

pub fn router() -> Router {
    Router::new().route("/sample-stations", axum::routing::get(get_sample_stations))
}

async fn get_sample_stations(State(app): State<App>) -> Json<Vec<String>> {
    Json(dedup_sample_stations(&app.config.lines))
}
```

Wire it into `private_router()` in `crates/api/src/routes/mod.rs`:
```rust
use axum::middleware;
use crate::app::{App, Router};
use crate::auth::require_internal_token;

pub mod health;
pub mod ingest;
pub mod samples;

pub fn public_router() -> Router {
    Router::new().merge(health::router())
}

pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .merge(samples::router())
        .layer(middleware::from_fn_with_state(app, require_internal_token))
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build -p api`
Expected: builds with no errors.

- [ ] **Step 7: Wire the line catalogue into the Docker image**

The `GET /sample-stations` endpoint is only meaningful if `api` actually
loads `lines/*.toml` at startup, which today it doesn't (no `--lines-dir`
is ever passed in `docker-compose.yml`, and the arg has no default, so
`app.config.lines` is currently always empty in the deployed stack).

In `crates/api/src/data/config.rs`, change the `lines` field's attribute
from:
```rust
    #[arg(long = "lines-dir", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: Vec<LineDefinition>,
```
to:
```rust
    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Defaults to `/app/lines` (baked into the Docker image — see
    /// `docker/api.Dockerfile`), overridable via `LINES_DIR` for local
    /// (non-Docker) runs.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: Vec<LineDefinition>,
```

In `docker/api.Dockerfile`, add a line copying `lines/` into the runtime
image. The file currently reads (relevant excerpt):
```dockerfile
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin api

COPY --from=builder /app/target/release/api /usr/local/bin/api

USER api

ENTRYPOINT ["/usr/local/bin/api"]
```
Add one `COPY` line for the line catalogue, after the `useradd` line and
before `USER api` (order relative to the binary `COPY` doesn't matter —
placed here to keep both `COPY` lines adjacent):
```dockerfile
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin api

COPY --from=builder /app/target/release/api /usr/local/bin/api
COPY --chown=api:api lines/ /app/lines/

USER api

ENTRYPOINT ["/usr/local/bin/api"]
```

- [ ] **Step 8: Verify the whole stack end-to-end**

Build and run against a throwaway Postgres (adjust ports if taken):
```bash
docker run -d --rm --name nrstatus-verify2 -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=nr_status -p 55433:5432 postgres:16
sleep 3
docker build -f docker/api.Dockerfile -t nrstatus-api-verify .
docker run -d --rm --name nrstatus-api-verify \
  -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:55433/nr_status \
  -e BIND_URL=0.0.0.0:8080 \
  -e INTERNAL_TOKEN=test-secret-token \
  -p 18081:8080 \
  nrstatus-api-verify
sleep 3
curl -s -H 'X-Internal-Token: test-secret-token' http://127.0.0.1:18081/private/sample-stations
```
Expected (order matches the sorted/deduplicated union computed in the
"Current relevant code" section above — 17 stations):
```json
["AHT","AON","BMH","CAR","CRE","EUS","FRM","GLD","HSL","LBG","MKC","PMS","PRE","SOU","STP","WIN","ZFD"]
```

If `host.docker.internal` isn't resolvable in your Docker setup, run
`docker network create nrstatus-verify-net`, attach both containers to it
with `--network nrstatus-verify-net`, and use the Postgres container's name
(`nrstatus-verify2`) as the host in `DATABASE_URL` instead.

Clean up:
```bash
docker stop nrstatus-api-verify nrstatus-verify2
```

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/data/samples.rs crates/api/src/routes/samples.rs \
        crates/api/src/data/mod.rs crates/api/src/routes/mod.rs \
        crates/api/src/data/config.rs docker/api.Dockerfile
git commit -m "Add GET /private/sample-stations endpoint and wire lines/ into the api image"
```

---

## Task 3: `poller-ldbws` crate

**Files:**
- Create: `crates/poller-ldbws/Cargo.toml`
- Create: `crates/poller-ldbws/src/config.rs`
- Create: `crates/poller-ldbws/src/schema.rs`
- Create: `crates/poller-ldbws/src/main.rs`
- Create: `docker/poller-ldbws.Dockerfile`
- Modify: `Cargo.toml` (workspace root)

**Interfaces:**
- Consumes: `common::{StationSample, StationDeparture}`, `common::ingest::{post_batch, INTERNAL_TOKEN_HEADER, RDM_AUTH_HEADER_NAME}`
- Produces: binary `poller-ldbws`; no other crate depends on this one.

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add `"crates/poller-ldbws"` to `members`:
```toml
[workspace]
resolver = "2"
members = [
    "crates/common",
    "crates/api",
    "crates/poller-incidents",
    "crates/poller-stations",
    "crates/poller-tocs",
    "crates/poller-ldbws",
]
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/poller-ldbws/Cargo.toml`:
```toml
[package]
name = "poller-ldbws"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

- [ ] **Step 3: Write config**

Create `crates/poller-ldbws/src/config.rs`:
```rust
use clap::Parser;

/// CLI/env configuration for the `poller-ldbws` service.
///
/// `ldbws_base_url` deliberately has no default: research found two
/// different RDM product-slug segments in use across sources
/// (`1010-live-departure-board-dep` vs `...-dep1_2`) with no way to
/// reconcile which is currently correct without a live RDM subscription —
/// this must be supplied out of band once confirmed, not guessed.
#[derive(Debug, Parser)]
pub struct Config {
    /// RDM Live Departure Board base URL, up to and including the
    /// `/LDBWS/api/20220120` segment. The poller appends
    /// `/GetDepBoardWithDetails/{crs}` itself (see `main.rs`).
    #[arg(long, env)]
    pub ldbws_base_url: String,

    /// RDM API key, sent via the `x-apikey` header (see
    /// `RDM_AUTH_HEADER_NAME` in `main.rs`). Community sources describe
    /// this as the "consumer key" specifically (as opposed to a paired
    /// "consumer secret") — unconfirmed against RDM's own docs, but
    /// consistent with how the other three pollers authenticate.
    #[arg(long, env)]
    pub rdm_api_key: String,

    /// Number of services requested per station per cycle (LDBWS's own
    /// `numRows` query parameter). Kept at the upstream API's own default
    /// (10) rather than inventing a "better" number without evidence of
    /// what the aggregator's inference logic actually needs.
    #[arg(long, env, default_value_t = 10)]
    pub num_rows: u32,

    /// The `api` crate's endpoint for the deduplicated list of stations to
    /// sample (`GET /private/sample-stations`) — not an RDM endpoint.
    #[arg(long, env, default_value = "http://api:8080/private/sample-stations")]
    pub api_sample_stations_url: String,

    /// The `api` crate's ingestion endpoint for station samples.
    #[arg(long, env, default_value = "http://api:8080/private/station-samples")]
    pub api_ingest_url: String,

    /// Shared secret sent via `X-Internal-Token` to reach both `api`
    /// endpoints above (see `crates/api/src/auth.rs`).
    #[arg(long, env)]
    pub internal_token: String,

    /// DESIGN.md §4's aggregator polling cadence target is "30-60s"; 60 is
    /// the conservative end, given this feed's real rate limit is
    /// unconfirmed (see module docs in `main.rs`).
    #[arg(long, env, default_value_t = 60)]
    pub poll_interval_secs: u64,
}
```

- [ ] **Step 4: Write the failing test for delay computation**

Create `crates/poller-ldbws/src/schema.rs` with the test module first and a
`todo!()` body:
```rust
//! RDM Live Departure Board (`GetDepBoardWithDetails`) JSON schema and its
//! mapping to `common::StationDeparture`.
//!
//! Field names below are transcribed verbatim from a Swagger 2.0 spec
//! fetched and parsed directly during planning (see the implementation
//! plan's "Current relevant code" section for the source and exact
//! `definitions` block). High confidence on field names/types; the base
//! URL's exact product-slug segment and this feed's rate limit are the
//! genuinely unconfirmed facts, both handled in `config.rs`, not here.

use anyhow::Result;
use common::StationDeparture;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RdmStationBoard {
    #[serde(default, rename = "trainServices")]
    train_services: Vec<RdmServiceItem>,
}

#[derive(Debug, Deserialize)]
struct RdmServiceItem {
    #[serde(rename = "serviceID")]
    service_id: String,
    #[serde(rename = "operatorCode")]
    operator_code: String,
    destination: Vec<RdmServiceLocation>,
    std: String,
    etd: String,
    #[serde(rename = "isCancelled")]
    is_cancelled: bool,
    #[serde(default, rename = "cancelReason")]
    cancel_reason: Option<String>,
    #[serde(default, rename = "delayReason")]
    delay_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RdmServiceLocation {
    crs: String,
}

fn parse_hhmm(s: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(s, "%H:%M").ok()
}

/// Computes minutes of delay between a scheduled ("std") and estimated
/// ("etd") departure time-of-day string. LDBWS's `etd` field is not always
/// a time — it may be a status word like `"On time"`, `"Delayed"`, or
/// `"Cancelled"` — so this returns `0` whenever `etd` isn't itself a valid
/// "HH:MM" time (including `"On time"`: no delay to report, and any other
/// status word: `is_cancelled`/`delay_reason` already carry the more
/// precise signal, and there's no time to diff against).
///
/// Handles the midnight wraparound case (e.g. std="23:55", etd="00:05" is
/// a 10-minute delay, not -1430).
pub fn compute_delay_minutes(std: &str, etd: &str) -> i32 {
    todo!()
}

/// Maps one RDM `GetDepBoardWithDetails` JSON response body into the
/// `StationDeparture`s for that station. Only `trainServices` are sampled
/// (see the implementation plan's Global Constraints). A service missing a
/// destination is skipped (logged, not fabricated) rather than guessing a
/// CRS. `headcode` is always `None`: confirmed absent from this API's
/// schema entirely.
pub fn parse_departures(json: &str) -> Result<Vec<StationDeparture>> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_time_etd_has_zero_delay() {
        assert_eq!(compute_delay_minutes("10:00", "On time"), 0);
    }

    #[test]
    fn normal_delay_is_computed() {
        assert_eq!(compute_delay_minutes("10:00", "10:05"), 5);
    }

    #[test]
    fn midnight_wraparound_is_handled() {
        assert_eq!(compute_delay_minutes("23:55", "00:05"), 10);
    }

    #[test]
    fn non_time_status_word_has_zero_delay() {
        assert_eq!(compute_delay_minutes("10:00", "Cancelled"), 0);
        assert_eq!(compute_delay_minutes("10:00", "Delayed"), 0);
    }

    #[test]
    fn identical_times_have_zero_delay() {
        assert_eq!(compute_delay_minutes("10:00", "10:00"), 0);
    }

    const SAMPLE_JSON: &str = r#"
        {
            "generatedAt": "2026-07-06T10:00:00Z",
            "locationName": "London Paddington",
            "crs": "PAD",
            "trainServices": [
                {
                    "serviceID": "yjnJDu6rXAM6MhtwfOUZZg==",
                    "operator": "Great Western Railway",
                    "operatorCode": "GW",
                    "destination": [{"locationName": "Reading", "crs": "RDG"}],
                    "origin": [{"locationName": "London Paddington", "crs": "PAD"}],
                    "std": "10:00",
                    "etd": "10:05",
                    "platform": "6",
                    "isCancelled": false,
                    "cancelReason": null,
                    "delayReason": "This train has been delayed by a signalling problem",
                    "rsid": "GW123400",
                    "serviceType": "train"
                },
                {
                    "serviceID": "abc123==",
                    "operator": "Great Western Railway",
                    "operatorCode": "GW",
                    "destination": [{"locationName": "Oxford", "crs": "OXF"}],
                    "origin": [{"locationName": "London Paddington", "crs": "PAD"}],
                    "std": "10:15",
                    "etd": "On time",
                    "platform": "9",
                    "isCancelled": false,
                    "cancelReason": null,
                    "delayReason": null,
                    "rsid": "GW123500",
                    "serviceType": "train"
                },
                {
                    "serviceID": "def456==",
                    "operator": "Great Western Railway",
                    "operatorCode": "GW",
                    "destination": [{"locationName": "Bristol Temple Meads", "crs": "BRI"}],
                    "origin": [{"locationName": "London Paddington", "crs": "PAD"}],
                    "std": "10:30",
                    "etd": "Cancelled",
                    "platform": null,
                    "isCancelled": true,
                    "cancelReason": "This train has been cancelled because of a fault on this train",
                    "delayReason": null,
                    "rsid": "GW123600",
                    "serviceType": "train"
                }
            ]
        }
    "#;

    #[test]
    fn parses_sample_board_and_maps_every_field() {
        let departures = parse_departures(SAMPLE_JSON).expect("sample JSON should parse");
        assert_eq!(departures.len(), 3);

        let first = &departures[0];
        assert_eq!(first.service_id, "yjnJDu6rXAM6MhtwfOUZZg==");
        assert_eq!(first.operator, "GW");
        assert_eq!(first.destination_crs, "RDG");
        assert_eq!(first.scheduled, "10:00");
        assert_eq!(first.estimated, "10:05");
        assert!(!first.is_cancelled);
        assert_eq!(first.delay_minutes, 5);
        assert_eq!(first.cancel_reason, None);
        assert_eq!(
            first.delay_reason,
            Some("This train has been delayed by a signalling problem".to_string())
        );
        assert_eq!(first.headcode, None);

        let second = &departures[1];
        assert_eq!(second.estimated, "On time");
        assert_eq!(second.delay_minutes, 0);
        assert!(!second.is_cancelled);

        let third = &departures[2];
        assert!(third.is_cancelled);
        assert_eq!(third.delay_minutes, 0);
        assert_eq!(
            third.cancel_reason,
            Some("This train has been cancelled because of a fault on this train".to_string())
        );
    }

    #[test]
    fn service_with_no_destination_is_skipped() {
        let json = r#"
            {
                "trainServices": [
                    {
                        "serviceID": "x==",
                        "operator": "Test",
                        "operatorCode": "TT",
                        "destination": [],
                        "std": "10:00",
                        "etd": "On time",
                        "isCancelled": false
                    }
                ]
            }
        "#;
        let departures = parse_departures(json).expect("should parse despite empty destination");
        assert_eq!(departures.len(), 0);
    }
}
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test -p poller-ldbws`
Expected: compile succeeds (the crate isn't in the workspace's `Cargo.lock`
yet, so this will also resolve dependencies — that's expected), but tests
panic with `not yet implemented` from the two `todo!()`s.

- [ ] **Step 6: Implement `compute_delay_minutes` and `parse_departures`**

Replace both `todo!()` bodies:
```rust
pub fn compute_delay_minutes(std: &str, etd: &str) -> i32 {
    let (Some(scheduled), Some(estimated)) = (parse_hhmm(std), parse_hhmm(etd)) else {
        return 0;
    };

    let diff = (estimated - scheduled).num_minutes();
    if diff < 0 { (diff + 1440) as i32 } else { diff as i32 }
}

pub fn parse_departures(json: &str) -> Result<Vec<StationDeparture>> {
    let board: RdmStationBoard = serde_json::from_str(json)?;

    Ok(board
        .train_services
        .iter()
        .filter_map(|service| {
            let Some(destination) = service.destination.first() else {
                tracing::warn!(service_id = %service.service_id, "service has no destination, skipping");
                return None;
            };

            Some(StationDeparture {
                service_id: service.service_id.clone(),
                operator: service.operator_code.clone(),
                destination_crs: destination.crs.clone(),
                scheduled: service.std.clone(),
                estimated: service.etd.clone(),
                is_cancelled: service.is_cancelled,
                delay_minutes: compute_delay_minutes(&service.std, &service.etd),
                cancel_reason: service.cancel_reason.clone(),
                delay_reason: service.delay_reason.clone(),
                headcode: None,
            })
        })
        .collect())
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p poller-ldbws`
Expected: `test result: ok. 8 passed; 0 failed`.

- [ ] **Step 8: Write `main.rs`**

Create `crates/poller-ldbws/src/main.rs`:
```rust
//! `poller-ldbws`: samples live departure-board data for every station any
//! line's inference logic depends on, and forwards parsed `StationSample`s
//! to the `api` crate's `/private/station-samples` ingestion endpoint.
//!
//! See `docs/superpowers/specs/2026-07-06-ldbws-sampler-poller-design.md`
//! for the full design and `docs/superpowers/plans/2026-07-06-ldbws-sampler-poller.md`
//! for the RDM facts this is built against (a documentation-discovery pass
//! against a fetched Swagger spec for RDM's Live Departure Board REST
//! product, `GetDepBoardWithDetails`). Two documented gaps carried into
//! `config.rs`: the exact RDM product-slug segment of the base URL, and
//! this feed's real rate limit — both are env-configurable rather than
//! guessed.
//!
//! Unlike the other three pollers, this one calls a second `api` endpoint
//! first (`GET /private/sample-stations`) to learn which CRS codes to
//! sample, then makes one LDBWS call *per station* each cycle — there is
//! no bulk/multi-station LDBWS operation.

mod config;
mod schema;

use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use common::ingest::{self, INTERNAL_TOKEN_HEADER, RDM_AUTH_HEADER_NAME};
use common::{StationDeparture, StationSample};
use config::Config;
use reqwest::Client;

/// Per-request timeout — see the other three pollers' identical rationale.
/// 30s is comfortably short relative to the 60s default poll interval.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        interval.tick().await;

        if let Err(err) = poll_once(&client, &config).await {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(client: &Client, config: &Config) -> anyhow::Result<()> {
    let stations = fetch_sample_stations(client, config).await?;
    tracing::info!(count = stations.len(), "fetched station list to sample");

    let mut samples = Vec::with_capacity(stations.len());

    for crs in &stations {
        match fetch_departures(client, config, crs).await {
            Ok(departures) => samples.push(StationSample {
                crs: crs.clone(),
                polled_at: Utc::now(),
                departures,
            }),
            Err(err) => {
                tracing::error!(crs = %crs, error = ?err, "failed to sample station; skipping");
            }
        }
    }

    if samples.is_empty() {
        tracing::warn!("no station samples collected this cycle; nothing to post");
        return Ok(());
    }

    ingest::post_batch(
        client,
        &config.api_ingest_url,
        &config.internal_token,
        &samples,
        "station samples",
    )
    .await
}

/// Calls the `api` crate's own `/private/sample-stations` endpoint — not an
/// RDM endpoint — to get the deduplicated CRS list computed from the
/// loaded line catalogue. Sent with the internal token, not the RDM API
/// key.
async fn fetch_sample_stations(client: &Client, config: &Config) -> anyhow::Result<Vec<String>> {
    let response = client
        .get(&config.api_sample_stations_url)
        .header(INTERNAL_TOKEN_HEADER, &config.internal_token)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<Vec<String>>().await?)
}

/// One `GetDepBoardWithDetails` call for a single station.
async fn fetch_departures(
    client: &Client,
    config: &Config,
    crs: &str,
) -> anyhow::Result<Vec<StationDeparture>> {
    let url = format!(
        "{}/GetDepBoardWithDetails/{crs}?numRows={}",
        config.ldbws_base_url, config.num_rows
    );

    let response = client
        .get(&url)
        .header(RDM_AUTH_HEADER_NAME, &config.rdm_api_key)
        .send()
        .await?
        .error_for_status()?;

    let body = response.text().await?;
    schema::parse_departures(&body)
}
```

- [ ] **Step 9: Verify the whole crate builds and tests still pass**

Run: `cargo build -p poller-ldbws && cargo test -p poller-ldbws`
Expected: builds cleanly, `test result: ok. 8 passed; 0 failed`.

Run: `cargo clippy -p poller-ldbws --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 10: Write the Dockerfile**

Create `docker/poller-ldbws.Dockerfile`:
```dockerfile
# Multi-stage build for the `poller-ldbws` service.
#
# Builder pin: matches poller-incidents/poller-stations/poller-tocs at
# rust:1.86-bookworm — this crate pulls in the same reqwest -> idna/icu_*
# transitive chain requiring rustc 1.86+.
#
# Build from the repo root so the workspace's Cargo.toml/Cargo.lock and
# crates/common path dependency are all in the build context:
#   docker build -f docker/poller-ldbws.Dockerfile .
FROM rust:1.86-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin poller-ldbws

FROM debian:bookworm-slim

# reqwest's native-tls backend verifies certs against the system store, so
# the runtime image needs a CA bundle even though it otherwise only carries
# the one binary.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin poller

COPY --from=builder /app/target/release/poller-ldbws /usr/local/bin/poller-ldbws

USER poller

ENTRYPOINT ["/usr/local/bin/poller-ldbws"]
```

- [ ] **Step 11: Verify the image builds**

Run: `docker build -f docker/poller-ldbws.Dockerfile -t poller-ldbws-verify .`
Expected: builds successfully.

Verify the non-root user:
```bash
docker run --rm --entrypoint id poller-ldbws-verify
```
Expected output contains `uid=999(poller)` (or similar non-zero uid) —
not `uid=0(root)`.

- [ ] **Step 12: Commit**

```bash
git add Cargo.toml Cargo.lock crates/poller-ldbws docker/poller-ldbws.Dockerfile
git commit -m "Add poller-ldbws: samples LDBWS departure boards per station"
```

---

## Task 4: docker-compose wiring, `.env.example`, and end-to-end verification

**Files:**
- Modify: `docker-compose.yml`
- Modify: `.env.example`

**Interfaces:** none (integration-only task).

- [ ] **Step 1: Add the `poller-ldbws` service to `docker-compose.yml`**

Add this block after the `poller-tocs:` service and before the `volumes:`
section at the end of the file:
```yaml
  poller-ldbws:
    build:
      context: .
      dockerfile: docker/poller-ldbws.Dockerfile
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
    environment:
      # crates/poller-ldbws/src/config.rs: Config. GAP: RDM product-slug
      # segment is unconfirmed (two variants seen in research); GAP: this
      # feed's real rate limit is unconfirmed (see the implementation
      # plan's Global Constraints).
      LDBWS_BASE_URL: ${LDBWS_BASE_URL}
      RDM_API_KEY: ${LDBWS_API_KEY}
      NUM_ROWS: ${LDBWS_NUM_ROWS:-10}
      API_SAMPLE_STATIONS_URL: http://api:8080/private/sample-stations
      API_INGEST_URL: http://api:8080/private/station-samples
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      # DESIGN.md §4 target cadence is 30-60s; 60 is the conservative end
      # given this feed's rate limit is unconfirmed.
      POLL_INTERVAL_SECS: ${POLL_INTERVAL_SECS_LDBWS:-60}
      RUST_LOG: ${RUST_LOG:-info}
```

- [ ] **Step 2: Update `.env.example`**

Add a new section at the end, before the final "Shared across all pollers"
section (match the existing file's per-poller section style):
```bash
# ---------------------------------------------------------------------------
# poller-ldbws (crates/poller-ldbws/src/config.rs: Config)
# ---------------------------------------------------------------------------
# GAP: the exact RDM product-slug segment for the Live Departure Board
# product is unconfirmed — two variants were found during research
# (e.g. "1010-live-departure-board-dep" vs "...-dep1_2"). Placeholder only;
# confirm the real value once you have RDM account access.
LDBWS_BASE_URL=http://rdm-ldbws.example.invalid
LDBWS_API_KEY=changeme-rdm-ldbws-api-key
# Services requested per station per poll (LDBWS's own default is 10).
LDBWS_NUM_ROWS=10
# Target cadence per DESIGN.md is 30-60s; this feed's real rate limit is
# unconfirmed, so 60 (the conservative end) is the default. Shorten
# temporarily (e.g. to 30) only to verify the poll loop locally.
POLL_INTERVAL_SECS_LDBWS=60
```

Also update the file's opening gap-disclosure comment block (the one
listing "As of this plan's completion, none of the following gaps have
been resolved...") to add two more bullet points:
```
#   - LDBWS (Live Departure Board) feed: no confirmed base URL/product-slug
#     segment; two variants seen in research, not reconciled.
#   - LDBWS feed rate limit: one low-confidence source claims a number,
#     not corroborated against RDM's own plan/quota page.
```

- [ ] **Step 3: Bring up the full stack**

```bash
cp .env.example .env
# edit .env: set INTERNAL_TOKEN, POSTGRES_* to whatever local values you like
docker compose up --build -d
sleep 10
docker compose ps
```
Expected: all 6 services (`postgres`, `api`, `poller-incidents`,
`poller-stations`, `poller-tocs`, `poller-ldbws`) show as running/healthy.

- [ ] **Step 4: Confirm the sample-stations endpoint is populated**

```bash
source .env
curl -s -H "X-Internal-Token: $INTERNAL_TOKEN" http://localhost:${API_HOST_PORT:-8080}/private/sample-stations
```
Expected: the same 17-station sorted array as Task 2's verification step
(`["AHT","AON","BMH","CAR","CRE","EUS","FRM","GLD","HSL","LBG","MKC","PMS","PRE","SOU","STP","WIN","ZFD"]`).

- [ ] **Step 5: Confirm `poller-ldbws` runs its resilience contract against the placeholder RDM host**

```bash
docker compose logs poller-ldbws --tail=50
```
Expected: log lines showing it fetched the station list (`count=17`) each
cycle, then a `poll cycle failed` or per-station `failed to sample station`
error for the unreachable `*.example.invalid` placeholder host — and the
container still shows as running (not crash-looped) in `docker compose ps`.
This confirms the same "log and keep the loop alive" contract already
proven for the other three pollers, without needing real RDM credentials
(none are available in this environment — this is expected, not a defect;
see the design doc and this plan's Global Constraints).

- [ ] **Step 6: Confirm `GET /public/health` still works**

```bash
curl -s http://localhost:${API_HOST_PORT:-8080}/public/health
```
Expected: `{"message":"Alive"}`.

- [ ] **Step 7: Tear down**

```bash
docker compose down -v
```

- [ ] **Step 8: Commit**

```bash
git add docker-compose.yml .env.example
git commit -m "Wire poller-ldbws into docker-compose and document its config in .env.example"
```

---

## Self-Review Notes (completed during writing, recorded here per skill instructions)

**Spec coverage:** Every element of `docs/superpowers/specs/2026-07-06-ldbws-sampler-poller-design.md`
is covered: the ingestion endpoint (Task 1), the sample-stations endpoint +
line-catalogue Docker wiring (Task 2), the poller crate including the
delay-computation and parsing logic with TDD (Task 3), and the
docker-compose/`.env.example`/end-to-end verification (Task 4). The design
doc's explicitly-out-of-scope items (aggregator, read endpoints, history)
are not touched by any task here.

**Placeholder scan:** No TBD/TODO markers remain outside the two
intentional `todo!()` Rust placeholders that are themselves TDD RED-phase
steps (replaced by Step 6/Step 3 of their respective tasks, per the
skill's own bite-sized-step pattern — these are not spec gaps, they're the
standard "write failing test, then implement" shape).

**Type consistency:** `StationSample`/`StationDeparture`/`LineDefinition`
field names and types are used identically across Tasks 1-3 (verified
against the actual `crates/common/src/lib.rs` file during planning, not
reconstructed from memory). `dedup_sample_stations` (Task 2) and
`compute_delay_minutes`/`parse_departures` (Task 3) signatures are
consistent between their introduction and every later use.
