# Incident NLP Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract resolution status, category, schedule window, and ETA from Knowledgebase incident text via an LLM, and feed resolution status/schedule/ETA into severity classification as a demote-only signal — closing the "SWR leaves a resolved incident active for weeks" gap the stale-incident-handling feature couldn't close.

**Architecture:** A new `crates/enricher` binary crate consumes a Redis Stream of "this incident's text changed" events (published by `crates/api` right after `upsert_incidents` commits), backed by an hourly reconciliation sweep as a delivery backstop. For each changed incident it runs two calls against a configurable OpenAI-compatible Chat Completions endpoint — a primary extraction pass and an adversarial pass that argues the incident is still ongoing — and writes the combined, confidence-gated result to eight new nullable `incidents` columns. `crates/aggregator` reads those columns alongside the existing incident fields and applies a new `apply_extraction` step between the existing `severity_from_incident` and `demote_for_scope`, which can only demote severity or annotate reason text, never suppress a status.

**Tech Stack:** Rust/sqlx/axum/tokio (`crates/api`, `crates/aggregator`, new `crates/enricher`), PostgreSQL, Redis Streams (new `redis` crate dependency), `reqwest` against an OpenAI-compatible REST endpoint, `sha2` for text hashing, `wiremock` (new dev-dependency) for HTTP-mocked extraction tests.

**Spec:** `docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md`

## Global Constraints

- The LLM client targets the generic OpenAI-compatible Chat Completions REST API only (`POST {LLM_BASE_URL}/chat/completions`, forced `response_format: json_schema` structured output). No vendor SDK, no hardcoded assumption about which server sits behind the URL.
- Config is env-only via `clap`'s `env` feature, matching every existing crate: `LLM_BASE_URL`, `LLM_API_KEY` (optional), `LLM_MODEL`, `REDIS_URL`, `DATABASE_URL`, `SWEEP_INTERVAL_SECS` (default `3600`).
- Redis Stream name: `incident-text-changed`. Consumer group name: `enricher`. Both are hardcoded constants, not configurable — same posture this codebase already takes on the rail-day boundary hour.
- Resolution-status extraction may only **demote** severity (to `MinorDelays` at most for `resolved`/schedule-window/ETA-passed, to `Recovering` for `residual`) or annotate `reason` text. It must never remove a `LineStatus` outright. Any missing, low-confidence, or malformed extraction behaves identically to no extraction at all.
- `common::IncidentMessage`'s wire shape (poller ↔ API contract) does not change. All eight new columns live only on `incidents`, read via a new `aggregator`-local type, exactly like `first_seen_at` was kept out of `IncidentMessage`.
- Category extraction is stored but not consumed by severity classification in this plan — the existing regex keyword table stays authoritative. No task here touches it.
- New dependencies introduced by this plan: `redis` (in `api` and `enricher`), `sha2` (in `enricher`), `wiremock` (dev-dependency, in `enricher`).
- The Helm chart (`docs/superpowers/specs/2026-08-18-helm-chart-design.md`) has an established "no subchart dependencies" constraint. Redis is added as a plain in-chart Deployment + Service (no persistence — Redis is a disposable trigger, not a system of record; the hourly sweep is the durability backstop), mirroring how `postgres-statefulset.yaml`/`postgres-service.yaml` are hand-rolled rather than pulled from a subchart, minus the StatefulSet/PVC machinery Postgres needs and Redis here doesn't.
- Migration files are timestamp-prefixed SQL in `crates/api/migrations/`; the next one must sort after the existing `20260716180000_incident_first_seen.sql`.

---

### Task 1: `incidents` schema migration

**Files:**
- Create: `crates/api/migrations/20260820120000_incident_extraction.sql`

**Interfaces:**
- Produces: eight new nullable columns on `incidents` — `source_text_hash`, `extracted_category`, `extracted_resolution_status`, `extracted_schedule_window`, `extracted_eta`, `extraction_confidence`, `extraction_model_version`, `extracted_at`. Consumed by Task 5 (writes), Task 11 (reads).

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260820120000_incident_extraction.sql`:

```sql
-- -------------------------------------------------------------------------
-- Incidents: NLP-extracted structured fields, written only by the
-- `enricher` crate and read only by `aggregator`. All nullable and
-- additive -- a row with every column NULL behaves identically to today's
-- regex-only classifier. See
-- docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md.
-- -------------------------------------------------------------------------

ALTER TABLE incidents
    ADD COLUMN source_text_hash TEXT,
    ADD COLUMN extracted_category TEXT,
    ADD COLUMN extracted_resolution_status TEXT
        CHECK (extracted_resolution_status IN ('ongoing', 'residual', 'resolved')),
    ADD COLUMN extracted_schedule_window JSONB,
    ADD COLUMN extracted_eta TIMESTAMPTZ,
    ADD COLUMN extraction_confidence TEXT
        CHECK (extraction_confidence IN ('high', 'low')),
    ADD COLUMN extraction_model_version TEXT,
    ADD COLUMN extracted_at TIMESTAMPTZ;
```

- [ ] **Step 2: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS — this migration adds nullable columns only; no existing query selects `*` or otherwise breaks.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260820120000_incident_extraction.sql
git commit -m "Add NLP extraction columns to incidents"
```

---

### Task 2: `text_changed` diff + Redis Streams publish in `upsert_incidents`

**Files:**
- Modify: `crates/api/Cargo.toml` (add `redis` dependency)
- Modify: `crates/api/src/data/config.rs` (add `redis_url` to `ServiceArguments`)
- Modify: `crates/api/src/app.rs` (add a `redis::Client` to `AppState`)
- Modify: `crates/api/src/data/queries.rs` (`upsert_incidents`)
- Test: `crates/api/src/data/queries.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `fn text_changed(existing: Option<&ExistingIncident>, summary: &str, description: &str) -> bool` (pure, unit-tested); `upsert_incidents` publishes one Redis Stream entry per text-changed incident to `incident-text-changed` after its transaction commits.
- Consumed by: Task 4's consumer-group loop.

- [ ] **Step 1: Add the `redis` dependency**

```bash
cd crates/api
cargo add redis@0.27 --features tokio-comp,connection-manager
cd ../..
```

Expected: `crates/api/Cargo.toml` gains a `redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }` line.

- [ ] **Step 2: Add `redis_url` to config and wire a `ConnectionManager` into `AppState`**

In `crates/api/src/data/config.rs`, add to `ServiceArguments` (alongside the existing `database_url` field):

```rust
    #[arg(long, env)]
    pub redis_url: String,
```

In `crates/api/src/app.rs`, add the import and field:

```rust
use redis::aio::ConnectionManager;
```

```rust
#[derive(Debug)]
pub struct AppState {
    pub config: ServiceArguments,
    pub database: PgPool,
    pub redis: ConnectionManager,
}
```

And in `AppState::init`, after the existing `db` connection is established:

```rust
        let redis_client = redis::Client::open(config.redis_url.clone())
            .context("Could not parse REDIS_URL")?;
        let redis = redis_client
            .get_connection_manager()
            .await
            .context("Could not connect to redis")?;

        Ok(Arc::new(Self {
            config,
            database: db,
            redis,
        }))
```

(`ConnectionManager` auto-reconnects on transient failures, so it's held long-lived on `AppState` rather than opened per-request — the same reason `PgPool` is held rather than reconnected per query.)

- [ ] **Step 3: Write the failing test for `text_changed`**

Add to `crates/api/src/data/queries.rs`'s `mod tests` block (create one with `use super::*;` if none exists yet — check the bottom of the file first; `incident_changed` next to it should already have coverage to follow the same pattern):

```rust
    fn existing(summary: &str, description: &str) -> ExistingIncident {
        ExistingIncident {
            summary: summary.to_string(),
            description: description.to_string(),
            validity_periods: serde_json::json!([]),
        }
    }

    #[test]
    fn text_changed_true_for_a_new_incident() {
        assert!(text_changed(None, "Signal failure", "Delays expected"));
    }

    #[test]
    fn text_changed_true_when_summary_differs() {
        let row = existing("Signal failure", "Delays expected");
        assert!(text_changed(Some(&row), "Points failure", "Delays expected"));
    }

    #[test]
    fn text_changed_true_when_description_differs() {
        let row = existing("Signal failure", "Delays expected");
        assert!(text_changed(Some(&row), "Signal failure", "Disruption has now ended"));
    }

    #[test]
    fn text_changed_false_when_only_validity_periods_would_differ() {
        // text_changed only compares summary/description -- validity is
        // deliberately excluded, since it doesn't require re-extraction of
        // prose that hasn't moved. This test simulates that by reusing the
        // same summary/description text_changed actually looks at; there's
        // no validity parameter to vary because text_changed never takes one.
        let row = existing("Signal failure", "Delays expected");
        assert!(!text_changed(Some(&row), "Signal failure", "Delays expected"));
    }
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p api text_changed`
Expected: FAIL — compile error, `text_changed` is not defined yet.

- [ ] **Step 5: Implement `text_changed` and wire the publish into `upsert_incidents`**

In `crates/api/src/data/queries.rs`, add next to the existing `incident_changed`:

```rust
/// Narrower than `incident_changed`: true only if summary or description
/// differ from what's stored. Validity-only changes don't need
/// re-extraction -- the prose an LLM would read hasn't moved. Drives
/// whether `upsert_incidents` publishes a `text-changed` event.
fn text_changed(existing: Option<&ExistingIncident>, summary: &str, description: &str) -> bool {
    match existing {
        None => true,
        Some(row) => row.summary != summary || row.description != description,
    }
}
```

Then change `upsert_incidents`'s signature to accept the Redis connection, collect text-changed IDs during the existing loop, and publish after commit:

```rust
pub async fn upsert_incidents(
    pool: &PgPool,
    redis: &redis::aio::ConnectionManager,
    incidents: &[IncidentMessage],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    let mut text_changed_ids = Vec::new();

    for incident in incidents {
        let validity_json = serde_json::to_value(&incident.validity)?;

        let existing: Option<ExistingIncident> = sqlx::query_as(
            "SELECT summary, description, validity_periods FROM incidents WHERE incident_id = $1",
        )
        .bind(&incident.incident_id)
        .fetch_optional(&mut *tx)
        .await?;

        let changed = incident_changed(
            existing.as_ref(),
            &incident.summary,
            &incident.description,
            &validity_json,
        );
        if text_changed(existing.as_ref(), &incident.summary, &incident.description) {
            text_changed_ids.push(incident.incident_id.clone());
        }

        sqlx::query(
            r#"
            INSERT INTO incidents (
                incident_id, summary, description, operators, affected_stations,
                priority, validity_periods, is_planned, is_cleared, fetched_at,
                first_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
            ON CONFLICT (incident_id) DO UPDATE SET
                summary           = EXCLUDED.summary,
                description       = EXCLUDED.description,
                operators         = EXCLUDED.operators,
                affected_stations = EXCLUDED.affected_stations,
                priority          = EXCLUDED.priority,
                validity_periods  = EXCLUDED.validity_periods,
                is_planned        = EXCLUDED.is_planned,
                is_cleared        = EXCLUDED.is_cleared,
                fetched_at        = NOW()
            "#,
        )
        .bind(&incident.incident_id)
        .bind(&incident.summary)
        .bind(&incident.description)
        .bind(&incident.operators)
        .bind(&incident.affected_stations)
        .bind(incident.priority)
        .bind(&validity_json)
        .bind(incident.is_planned)
        .bind(incident.is_cleared)
        .execute(&mut *tx)
        .await?;

        if changed {
            // (unchanged incident_history insert stays exactly as it is today)
        }

        count += 1;
    }

    tx.commit().await?;

    // Publish only after commit: a publish before commit could announce an
    // incident that a later failure in this same batch rolls back. Publish
    // failure is logged, not propagated -- the hourly sweep (Task 5) is the
    // backstop for a missed publish, so ingestion must not fail because
    // Redis is briefly unavailable.
    let mut redis = redis.clone();
    for incident_id in text_changed_ids {
        let result: redis::RedisResult<String> = redis::cmd("XADD")
            .arg("incident-text-changed")
            .arg("*")
            .arg("incident_id")
            .arg(&incident_id)
            .query_async(&mut redis)
            .await;
        if let Err(err) = result {
            tracing::warn!(error = ?err, incident_id, "failed to publish text-changed event; hourly sweep will catch it");
        }
    }

    Ok(count)
}
```

(The `if changed { ... }` block is a placeholder for this plan's snippet only — the real file already has the `incident_history` insert there; do not remove or alter it, just add the `text_changed_ids.push(...)` line above it and the publish loop after `tx.commit().await?`.)

Update the one call site in `crates/api/src/routes/ingest.rs`:

```rust
async fn post_incidents(
    State(app): State<App>,
    Json(incidents): Json<Vec<IncidentMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_incidents(&app.database, &app.redis, &incidents)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p api text_changed`
Expected: PASS.

- [ ] **Step 7: Run the full API crate test suite**

Run: `cargo test -p api`
Expected: PASS, no regressions.

- [ ] **Step 8: Commit**

```bash
git add crates/api/Cargo.toml crates/api/Cargo.lock crates/api/src/data/config.rs crates/api/src/app.rs crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs
git commit -m "Publish incident-text-changed events on summary/description edits"
```

---

### Task 3: `crates/enricher` skeleton

**Files:**
- Create: `crates/enricher/Cargo.toml`
- Create: `crates/enricher/src/main.rs`
- Create: `crates/enricher/src/config.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: a binary that connects to Postgres and Redis at startup and idles on a `SWEEP_INTERVAL_SECS` timer (body added in Task 5), logging a heartbeat. Establishes the crate scaffold every later task builds on.

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add `"crates/enricher"` to `members`, alongside the existing entries.

- [ ] **Step 2: Write `crates/enricher/Cargo.toml`**

```toml
[package]
name = "enricher"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
chrono-tz = "0.10"
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
sha2 = "0.10"
sqlx = { version = "0.8.6", features = ["chrono", "json", "macros", "postgres", "runtime-tokio", "tls-native-tls"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
wiremock = "0.6"
```

- [ ] **Step 3: Write `crates/enricher/src/config.rs`**

```rust
use clap::Parser;

/// CLI/env configuration for the `enricher` service.
#[derive(Debug, Parser)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    #[arg(long, env)]
    pub redis_url: String,

    /// Base URL of an OpenAI-compatible Chat Completions endpoint, e.g.
    /// `http://localhost:8080/v1` for a local server. No vendor is assumed.
    #[arg(long, env)]
    pub llm_base_url: String,

    /// Optional -- many local OpenAI-compatible servers don't require one.
    #[arg(long, env)]
    pub llm_api_key: Option<String>,

    /// Model name/identifier as the endpoint expects it.
    #[arg(long, env)]
    pub llm_model: String,

    /// How often the reconciliation sweep runs, independent of the Redis
    /// Stream consumer loop. Backstop for a missed/lost publish.
    #[arg(long, env, default_value_t = 3600)]
    pub sweep_interval_secs: u64,
}
```

- [ ] **Step 4: Write `crates/enricher/src/main.rs`**

```rust
//! `enricher`: extracts structured resolution status, category, schedule
//! window, and ETA from Knowledgebase incident text via an OpenAI-compatible
//! LLM endpoint. See
//! docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md.

mod config;

use std::time::Duration;

use clap::Parser;
use config::Config;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let _redis = redis_client.get_connection_manager().await?;

    tracing::info!("enricher connected to postgres and redis; consumer loop and sweep land in later tasks");

    // Placeholder idle loop -- Task 4 replaces this with the real Redis
    // Streams consumer-group loop, and Task 5 adds the sweep timer
    // alongside it.
    let mut interval = tokio::time::interval(Duration::from_secs(config.sweep_interval_secs));
    loop {
        interval.tick().await;
        tracing::info!("enricher heartbeat");
        let _ = &pool;
    }
}
```

- [ ] **Step 5: Confirm the workspace builds**

Run: `cargo build -p enricher`
Expected: PASS — compiles cleanly (network access to fetch new crate versions is required for this step).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/enricher
git commit -m "Scaffold enricher crate"
```

---

### Task 4: Redis Streams consumer-group loop

**Files:**
- Create: `crates/enricher/src/stream.rs`
- Modify: `crates/enricher/src/main.rs`

**Interfaces:**
- Consumes: `incident-text-changed` stream (Task 2's publisher).
- Produces: `async fn ensure_group(conn: &mut ConnectionManager) -> anyhow::Result<()>`; `async fn read_one(conn: &mut ConnectionManager) -> anyhow::Result<Option<(String, String)>>` returning `(stream_entry_id, incident_id)` for one pending entry, or `None` if none are ready; `async fn ack(conn: &mut ConnectionManager, entry_id: &str) -> anyhow::Result<()>`. Consumed by Task 8, which replaces the stub processor these call.

- [ ] **Step 1: Write `crates/enricher/src/stream.rs`**

```rust
//! Thin wrapper around the `incident-text-changed` Redis Stream / consumer
//! group. Kept separate from extraction logic (`llm.rs`) and persistence
//! (`queries.rs`) so each can be understood and tested independently.

use redis::AsyncCommands;
use redis::aio::ConnectionManager;

const STREAM: &str = "incident-text-changed";
const GROUP: &str = "enricher";
const CONSUMER: &str = "enricher-1";

/// Creates the consumer group if it doesn't already exist, and the stream
/// itself if this is the very first run (`MKSTREAM`). `BUSYGROUP` (group
/// already exists) is the expected steady-state outcome and is swallowed,
/// not treated as an error.
pub async fn ensure_group(conn: &mut ConnectionManager) -> anyhow::Result<()> {
    let result: redis::RedisResult<()> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(STREAM)
        .arg(GROUP)
        .arg("$")
        .arg("MKSTREAM")
        .query_async(conn)
        .await;

    match result {
        Ok(()) => Ok(()),
        Err(err) if err.to_string().contains("BUSYGROUP") => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Reads at most one new entry for this consumer, blocking up to 5s if
/// none are immediately available. Returns the entry's own stream ID
/// (needed to `ack`) paired with the `incident_id` field it carries.
pub async fn read_one(conn: &mut ConnectionManager) -> anyhow::Result<Option<(String, String)>> {
    let reply: redis::streams::StreamReadReply = conn
        .xread_options(
            &[STREAM],
            &[">"],
            &redis::streams::StreamReadOptions::default()
                .group(GROUP, CONSUMER)
                .count(1)
                .block(5000),
        )
        .await?;

    for stream_key in reply.keys {
        for entry in stream_key.ids {
            let incident_id: String = entry
                .map
                .get("incident_id")
                .and_then(|v| redis::from_redis_value(v).ok())
                .ok_or_else(|| anyhow::anyhow!("stream entry missing incident_id field"))?;
            return Ok(Some((entry.id, incident_id)));
        }
    }
    Ok(None)
}

pub async fn ack(conn: &mut ConnectionManager, entry_id: &str) -> anyhow::Result<()> {
    let _: i64 = conn.xack(STREAM, GROUP, &[entry_id]).await?;
    Ok(())
}
```

- [ ] **Step 2: Wire the loop into `main.rs`, replacing the placeholder idle loop**

Replace the `main.rs` body written in Task 3 Step 4 with:

```rust
mod config;
mod stream;

use clap::Parser;
use config::Config;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let mut redis = redis_client.get_connection_manager().await?;
    stream::ensure_group(&mut redis).await?;

    spawn_sweep_timer(pool.clone(), config.sweep_interval_secs); // implemented in Task 5

    loop {
        match stream::read_one(&mut redis).await {
            Ok(Some((entry_id, incident_id))) => {
                tracing::info!(incident_id, "received text-changed event");
                // Task 8 replaces this stub with the real two-pass
                // extraction + DB write.
                if let Err(err) = stream::ack(&mut redis, &entry_id).await {
                    tracing::error!(error = ?err, entry_id, "failed to ack stream entry");
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = ?err, "error reading from incident-text-changed stream");
            }
        }
    }
}

fn spawn_sweep_timer(_pool: sqlx::PgPool, _interval_secs: u64) {
    // Task 5 fills this in.
}
```

- [ ] **Step 3: Confirm the crate builds**

Run: `cargo build -p enricher`
Expected: PASS.

- [ ] **Step 4: Manually verify against a live Redis**

```bash
docker run --rm -d --name enricher-test-redis -p 6379:6379 redis:7
DATABASE_URL="$DATABASE_URL" REDIS_URL="redis://localhost:6379" \
  LLM_BASE_URL="http://localhost:1" LLM_MODEL="unused" \
  cargo run -p enricher &
redis-cli -h localhost XADD incident-text-changed '*' incident_id MANUAL-TEST-1
```

Expected: the running `enricher` process logs `received text-changed event incident_id="MANUAL-TEST-1"`. Stop both processes and `docker rm -f enricher-test-redis` afterward.

- [ ] **Step 5: Commit**

```bash
git add crates/enricher/src/stream.rs crates/enricher/src/main.rs
git commit -m "Add Redis Streams consumer-group loop to enricher"
```

---

### Task 5: Text hashing + reconciliation sweep

**Files:**
- Create: `crates/enricher/src/hash.rs`
- Create: `crates/enricher/src/sweep.rs`
- Modify: `crates/enricher/src/main.rs` (`spawn_sweep_timer`)

**Interfaces:**
- Produces: `fn text_hash(summary: &str, description: &str) -> String` (pure, unit-tested); `struct SweepRow { incident_id: String, summary: String, description: String, source_text_hash: Option<String>, extraction_model_version: Option<String> }`; `fn incidents_needing_extraction(rows: &[SweepRow], current_model_version: &str) -> Vec<String>` (pure, unit-tested); `async fn fetch_sweep_rows(pool: &PgPool) -> anyhow::Result<Vec<SweepRow>>`. Consumed by Task 8 (both the consumer loop and the sweep enqueue the same processor).

- [ ] **Step 1: Write the failing tests for `text_hash` and `incidents_needing_extraction`**

Create `crates/enricher/src/hash.rs`:

```rust
use sha2::{Digest, Sha256};

/// Deterministic hash of an incident's extractable prose. Used both to
/// stamp `source_text_hash` after a successful extraction and to detect,
/// during the sweep, which incidents' text has moved since their last
/// extraction.
pub fn text_hash(summary: &str, description: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(summary.as_bytes());
    hasher.update(b"\0");
    hasher.update(description.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_hashes_identically() {
        assert_eq!(text_hash("a", "b"), text_hash("a", "b"));
    }

    #[test]
    fn different_summary_hashes_differently() {
        assert_ne!(text_hash("a", "b"), text_hash("a2", "b"));
    }

    #[test]
    fn the_separator_prevents_boundary_collisions() {
        // Without a separator "ab" + "" and "a" + "b" would hash identically.
        assert_ne!(text_hash("ab", ""), text_hash("a", "b"));
    }
}
```

Create `crates/enricher/src/sweep.rs`:

```rust
use sqlx::PgPool;

use crate::hash::text_hash;

pub struct SweepRow {
    pub incident_id: String,
    pub summary: String,
    pub description: String,
    pub source_text_hash: Option<String>,
    pub extraction_model_version: Option<String>,
}

/// Incidents whose current text hash doesn't match what's stored, or whose
/// last extraction ran under a different model/prompt version -- either
/// case means the stored extraction (if any) is stale and needs redoing.
/// Pure so it's testable without a database; `fetch_sweep_rows` below is
/// the thin, untested DB-fetching wrapper, following this codebase's
/// existing pattern of keeping query functions thin and testing the pure
/// logic they feed.
pub fn incidents_needing_extraction(rows: &[SweepRow], current_model_version: &str) -> Vec<String> {
    rows.iter()
        .filter(|row| {
            let current_hash = text_hash(&row.summary, &row.description);
            row.source_text_hash.as_deref() != Some(current_hash.as_str())
                || row.extraction_model_version.as_deref() != Some(current_model_version)
        })
        .map(|row| row.incident_id.clone())
        .collect()
}

pub async fn fetch_sweep_rows(pool: &PgPool) -> anyhow::Result<Vec<SweepRow>> {
    let rows = sqlx::query_as!(
        SweepRowRecord,
        "SELECT incident_id, summary, description, source_text_hash, extraction_model_version \
         FROM incidents WHERE NOT is_cleared"
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SweepRow {
            incident_id: r.incident_id,
            summary: r.summary,
            description: r.description,
            source_text_hash: r.source_text_hash,
            extraction_model_version: r.extraction_model_version,
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct SweepRowRecord {
    incident_id: String,
    summary: String,
    description: String,
    source_text_hash: Option<String>,
    extraction_model_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, summary: &str, hash: Option<&str>, model: Option<&str>) -> SweepRow {
        SweepRow {
            incident_id: id.to_string(),
            summary: summary.to_string(),
            description: "desc".to_string(),
            source_text_hash: hash.map(str::to_string),
            extraction_model_version: model.map(str::to_string),
        }
    }

    #[test]
    fn never_extracted_incident_needs_extraction() {
        let rows = vec![row("A", "text", None, None)];
        assert_eq!(incidents_needing_extraction(&rows, "gpt-oss-20b"), vec!["A"]);
    }

    #[test]
    fn matching_hash_and_model_does_not_need_extraction() {
        let hash = text_hash("text", "desc");
        let rows = vec![row("A", "text", Some(&hash), Some("gpt-oss-20b"))];
        assert!(incidents_needing_extraction(&rows, "gpt-oss-20b").is_empty());
    }

    #[test]
    fn changed_text_needs_re_extraction_even_with_matching_model() {
        let stale_hash = text_hash("old text", "desc");
        let rows = vec![row("A", "new text", Some(&stale_hash), Some("gpt-oss-20b"))];
        assert_eq!(incidents_needing_extraction(&rows, "gpt-oss-20b"), vec!["A"]);
    }

    #[test]
    fn model_version_bump_forces_re_extraction_even_with_matching_hash() {
        let hash = text_hash("text", "desc");
        let rows = vec![row("A", "text", Some(&hash), Some("gpt-oss-20b@prompt-v1"))];
        assert_eq!(incidents_needing_extraction(&rows, "gpt-oss-20b@prompt-v2"), vec!["A"]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p enricher text_hash incidents_needing_extraction`
Expected: PASS — these are pure functions, no red phase needed since implementation and tests were written together (same reasoning the precedent stale-incident plan used for its own atomic-signature-change steps).

- [ ] **Step 3: Wire the sweep into `main.rs`'s timer**

Replace `spawn_sweep_timer` in `crates/enricher/src/main.rs`:

```rust
mod config;
mod hash;
mod stream;
mod sweep;

use std::time::Duration;

use clap::Parser;
use config::Config;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let mut redis = redis_client.get_connection_manager().await?;
    stream::ensure_group(&mut redis).await?;

    tokio::spawn(sweep_loop(pool.clone(), config.llm_model.clone(), config.sweep_interval_secs));

    loop {
        match stream::read_one(&mut redis).await {
            Ok(Some((entry_id, incident_id))) => {
                tracing::info!(incident_id, "received text-changed event");
                if let Err(err) = stream::ack(&mut redis, &entry_id).await {
                    tracing::error!(error = ?err, entry_id, "failed to ack stream entry");
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = ?err, "error reading from incident-text-changed stream");
            }
        }
    }
}

async fn sweep_loop(pool: PgPool, model_version: String, interval_secs: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match sweep::fetch_sweep_rows(&pool).await {
            Ok(rows) => {
                let ids = sweep::incidents_needing_extraction(&rows, &model_version);
                tracing::info!(count = ids.len(), "sweep found incidents needing extraction");
                // Task 8 replaces this log line with actually enqueueing
                // each id through the same processor the stream loop uses.
            }
            Err(err) => tracing::error!(error = ?err, "sweep query failed"),
        }
    }
}
```

- [ ] **Step 4: Confirm the crate builds**

Run: `cargo build -p enricher`
Expected: PASS. (`sqlx::query_as!` in `sweep.rs` needs a reachable `DATABASE_URL` at compile time, or `SQLX_OFFLINE=true` with a prepared query cache -- if this workspace has neither set up, switch `fetch_sweep_rows` to `sqlx::query_as::<_, SweepRowRecord>("...")` runtime-checked instead, matching the pattern the `api` crate's own `queries.rs` module docs describe for exactly this reason.)

- [ ] **Step 5: Commit**

```bash
git add crates/enricher/src/hash.rs crates/enricher/src/sweep.rs crates/enricher/src/main.rs
git commit -m "Add text-hash-based reconciliation sweep to enricher"
```

---

### Task 6: OpenAI-compatible client + primary extraction pass

**Files:**
- Create: `crates/enricher/src/llm.rs`
- Test: `crates/enricher/src/llm.rs` (inline `#[cfg(test)] mod tests`, using `wiremock`)

**Interfaces:**
- Produces: `struct LlmClient { base_url: String, api_key: Option<String>, model: String, http: reqwest::Client }` with `LlmClient::new(base_url, api_key, model) -> Self`; `struct PrimaryExtraction { category: String, resolution_status: String, schedule_window: Option<ScheduleWindow>, eta: Option<DateTime<Utc>> }`; `struct ScheduleWindow { days_of_week: Vec<u8>, start_time: String, end_time: String }`; `async fn LlmClient::extract_primary(&self, summary: &str, description: &str) -> anyhow::Result<PrimaryExtraction>`. Consumed by Task 8. `ScheduleWindow` also consumed by Task 12 (severity classifier).

- [ ] **Step 1: Write the failing tests**

Create `crates/enricher/src/llm.rs` with just the types and a `#[cfg(test)]` block first:

```rust
//! Client for the generic OpenAI-compatible Chat Completions REST API.
//! Deliberately vendor-agnostic: `base_url`/`api_key`/`model` are the only
//! things that vary between a local llama.cpp/vLLM/Ollama server and any
//! hosted provider that speaks the same schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScheduleWindow {
    /// ISO 8601 weekday numbers, 1 (Monday) through 7 (Sunday).
    pub days_of_week: Vec<u8>,
    /// "HH:MM", 24-hour, Europe/London local time.
    pub start_time: String,
    pub end_time: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PrimaryExtraction {
    pub category: String,
    pub resolution_status: String,
    pub schedule_window: Option<ScheduleWindow>,
    pub eta: Option<DateTime<Utc>>,
}

pub struct LlmClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(base_url: String, api_key: Option<String>, model: String) -> Self {
        Self { base_url, api_key, model, http: reqwest::Client::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn extract_primary_parses_a_well_formed_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({
                            "category": "signal_failure",
                            "resolution_status": "resolved",
                            "schedule_window": null,
                            "eta": null
                        }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string());
        let result = client.extract_primary("Signal failure at Reading", "Now resolved").await.unwrap();

        assert_eq!(result.category, "signal_failure");
        assert_eq!(result.resolution_status, "resolved");
        assert_eq!(result.schedule_window, None);
        assert_eq!(result.eta, None);
    }

    #[tokio::test]
    async fn extract_primary_parses_a_schedule_window() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "content": serde_json::json!({
                            "category": "rail_replacement",
                            "resolution_status": "ongoing",
                            "schedule_window": {
                                "days_of_week": [1, 2, 3, 4, 5],
                                "start_time": "22:00",
                                "end_time": "06:00"
                            },
                            "eta": null
                        }).to_string()
                    }
                }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string());
        let result = client
            .extract_primary("Rail replacement buses", "Nightly 22:00-06:00")
            .await
            .unwrap();

        assert_eq!(
            result.schedule_window,
            Some(ScheduleWindow { days_of_week: vec![1, 2, 3, 4, 5], start_time: "22:00".to_string(), end_time: "06:00".to_string() })
        );
    }

    #[tokio::test]
    async fn extract_primary_fails_on_malformed_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "not valid json" } }]
            })))
            .mount(&server)
            .await;

        let client = LlmClient::new(server.uri(), None, "test-model".to_string());
        let result = client.extract_primary("Signal failure", "Delays").await;

        assert!(result.is_err(), "malformed content must be rejected, not silently stored");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p enricher extract_primary`
Expected: FAIL — compile error, `extract_primary` is not defined yet.

- [ ] **Step 3: Implement `extract_primary`**

Add to `crates/enricher/src/llm.rs`, above the `#[cfg(test)]` block:

```rust
#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    response_format: ResponseFormat,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
    json_schema: JsonSchemaSpec,
}

#[derive(Serialize)]
struct JsonSchemaSpec {
    name: &'static str,
    strict: bool,
    schema: serde_json::Value,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

const PRIMARY_SCHEMA_NAME: &str = "incident_extraction";

fn primary_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "category": { "type": "string" },
            "resolution_status": { "type": "string", "enum": ["ongoing", "residual", "resolved"] },
            "schedule_window": {
                "type": ["object", "null"],
                "properties": {
                    "days_of_week": { "type": "array", "items": { "type": "integer", "minimum": 1, "maximum": 7 } },
                    "start_time": { "type": "string" },
                    "end_time": { "type": "string" }
                },
                "required": ["days_of_week", "start_time", "end_time"]
            },
            "eta": { "type": ["string", "null"] }
        },
        "required": ["category", "resolution_status", "schedule_window", "eta"]
    })
}

const PRIMARY_PROMPT: &str = "You extract structured facts from UK National Rail Knowledgebase incident \
    text. Read the summary and description exactly as given -- do not speculate beyond what the text \
    states. `resolution_status` is `resolved` only if the text explicitly says the disruption/root cause \
    has ended; `residual` if it says the cause is fixed but knock-on effects continue; `ongoing` otherwise, \
    including whenever the text doesn't clearly say either way. `schedule_window` and `eta` are null unless \
    the text states them explicitly.";

impl LlmClient {
    pub fn new(base_url: String, api_key: Option<String>, model: String) -> Self {
        Self { base_url, api_key, model, http: reqwest::Client::new() }
    }

    async fn chat_completion(&self, system_prompt: &str, user_content: String, schema_name: &'static str, schema: serde_json::Value) -> anyhow::Result<String> {
        let request = ChatCompletionRequest {
            model: &self.model,
            messages: vec![
                ChatMessage { role: "system", content: system_prompt.to_string() },
                ChatMessage { role: "user", content: user_content },
            ],
            response_format: ResponseFormat {
                kind: "json_schema",
                json_schema: JsonSchemaSpec { name: schema_name, strict: true, schema },
            },
            temperature: 0.0,
        };

        let mut req = self.http.post(format!("{}/chat/completions", self.base_url)).json(&request);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await?.error_for_status()?;
        let body: ChatCompletionResponse = response.json().await?;
        let content = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("chat completion response had no choices"))?
            .message
            .content;
        Ok(content)
    }

    pub async fn extract_primary(&self, summary: &str, description: &str) -> anyhow::Result<PrimaryExtraction> {
        let user_content = format!("Summary: {summary}\nDescription: {description}");
        let content = self
            .chat_completion(PRIMARY_PROMPT, user_content, PRIMARY_SCHEMA_NAME, primary_schema())
            .await?;
        let extraction: PrimaryExtraction = serde_json::from_str(&content)
            .map_err(|err| anyhow::anyhow!("primary extraction returned malformed JSON: {err}"))?;
        Ok(extraction)
    }
}
```

- [ ] **Step 4: Declare the new module so it's actually compiled**

In `crates/enricher/src/main.rs`, add `mod llm;` to the module declarations at the top of the file (alongside the existing `mod config; mod hash; mod stream; mod sweep;` from Task 5, in alphabetical order: `mod config; mod hash; mod llm; mod stream; mod sweep;`). Without this, `llm.rs` is a file on disk but not part of the crate, and its tests silently don't run.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p enricher extract_primary`
Expected: PASS — all three tests pass. (If step 4 was skipped, this instead reports "0 tests ran" with no compile error — `cargo test` filters by name over whatever modules are actually declared, so a missing `mod` doesn't fail the build, it just silently excludes the file. Confirm the count is 3, not 0.)

- [ ] **Step 6: Commit**

```bash
git add crates/enricher/src/llm.rs crates/enricher/src/main.rs
git commit -m "Add OpenAI-compatible client and primary extraction pass"
```

---

### Task 7: Adversarial pass + combination logic

**Files:**
- Modify: `crates/enricher/src/llm.rs` (`extract_adversarial`)
- Create: `crates/enricher/src/combine.rs`

**Interfaces:**
- Produces: `async fn LlmClient::extract_adversarial(&self, summary: &str, description: &str) -> anyhow::Result<String>` (returns just the resolution-status verdict — `"ongoing"`, `"residual"`, or `"resolved"`); `fn combine(primary_status: &str, adversarial_status: &str) -> (String, String)` returning `(resolution_status, confidence)`, pure and unit-tested against the spec's full combination table. Consumed by Task 8.

- [ ] **Step 1: Write the failing tests for `combine`**

Create `crates/enricher/src/combine.rs`:

```rust
/// Combines the primary extraction's resolution-status verdict with the
/// adversarial pass's (which only ever argues for "ongoing" or agrees).
/// Returns `(resolution_status, confidence)` as the raw TEXT values to
/// store. Disagreement is treated as genuine ambiguity in the source text
/// -- low confidence, not an averaged or majority-vote answer -- per the
/// spec's asymmetric-risk reasoning: a false "resolved" is worse than a
/// missed one.
pub fn combine(primary_status: &str, adversarial_status: &str) -> (String, String) {
    if primary_status == "ongoing" {
        // No demotion is possible from "ongoing" either way, so the
        // adversarial pass's answer can't change the outcome.
        return (primary_status.to_string(), "high".to_string());
    }
    if adversarial_status == "ongoing" {
        (primary_status.to_string(), "low".to_string())
    } else {
        (primary_status.to_string(), "high".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_ongoing_is_always_high_confidence() {
        assert_eq!(combine("ongoing", "ongoing"), ("ongoing".to_string(), "high".to_string()));
        assert_eq!(combine("ongoing", "resolved"), ("ongoing".to_string(), "high".to_string()));
    }

    #[test]
    fn primary_resolved_agreeing_adversarial_is_high_confidence() {
        assert_eq!(combine("resolved", "residual"), ("resolved".to_string(), "high".to_string()));
        assert_eq!(combine("resolved", "resolved"), ("resolved".to_string(), "high".to_string()));
    }

    #[test]
    fn primary_resolved_disagreeing_adversarial_is_low_confidence() {
        assert_eq!(combine("resolved", "ongoing"), ("resolved".to_string(), "low".to_string()));
    }

    #[test]
    fn primary_residual_disagreeing_adversarial_is_low_confidence() {
        assert_eq!(combine("residual", "ongoing"), ("residual".to_string(), "low".to_string()));
    }

    #[test]
    fn primary_residual_agreeing_adversarial_is_high_confidence() {
        assert_eq!(combine("residual", "resolved"), ("residual".to_string(), "high".to_string()));
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p enricher combine`
Expected: PASS.

- [ ] **Step 3: Add `extract_adversarial` to `llm.rs`**

```rust
const ADVERSARIAL_SCHEMA_NAME: &str = "adversarial_resolution_check";

fn adversarial_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "resolution_status": { "type": "string", "enum": ["ongoing", "residual", "resolved"] }
        },
        "required": ["resolution_status"]
    })
}

const ADVERSARIAL_PROMPT: &str = "You are reviewing a UK National Rail incident report with a specific \
    job: argue for the most cautious reading. Assume the disruption is still `ongoing` unless the text \
    gives you clear, explicit, unambiguous evidence otherwise. Do not infer resolution from silence, from \
    a lack of new updates, or from an optimistic tone -- only from an explicit statement that the issue is \
    fixed or over.";

#[derive(Deserialize)]
struct AdversarialExtraction {
    resolution_status: String,
}

impl LlmClient {
    pub async fn extract_adversarial(&self, summary: &str, description: &str) -> anyhow::Result<String> {
        let user_content = format!("Summary: {summary}\nDescription: {description}");
        let content = self
            .chat_completion(ADVERSARIAL_PROMPT, user_content, ADVERSARIAL_SCHEMA_NAME, adversarial_schema())
            .await?;
        let extraction: AdversarialExtraction = serde_json::from_str(&content)
            .map_err(|err| anyhow::anyhow!("adversarial extraction returned malformed JSON: {err}"))?;
        Ok(extraction.resolution_status)
    }
}
```

- [ ] **Step 4: Declare the new module so it's actually compiled**

In `crates/enricher/src/main.rs`, add `mod combine;` to the module declarations (alongside the existing `mod config; mod hash; mod llm; mod stream; mod sweep;` from Task 6, in alphabetical order: `mod combine; mod config; mod hash; mod llm; mod stream; mod sweep;`). `combine::combine` isn't called from `main.rs` yet (that's Task 8), so `cargo build` will warn it's unused — expected and harmless until Task 8 wires it in.

- [ ] **Step 5: Run the full enricher crate test suite**

Run: `cargo test -p enricher`
Expected: PASS, no regressions. Confirm `combine`'s 5 tests and `llm`'s 3 tests both actually ran (check the test count in the output), not just the tests from earlier tasks.

- [ ] **Step 6: Commit**

```bash
git add crates/enricher/src/llm.rs crates/enricher/src/combine.rs crates/enricher/src/main.rs
git commit -m "Add adversarial extraction pass and confidence combination logic"
```

---

### Task 8: Wire the full two-pass pipeline into processing + DB writes

**Files:**
- Create: `crates/enricher/src/queries.rs`
- Modify: `crates/enricher/src/main.rs`

**Interfaces:**
- Consumes: `stream::read_one`/`ack` (Task 4), `sweep::incidents_needing_extraction` (Task 5), `LlmClient::extract_primary`/`extract_adversarial` (Task 6/7), `combine::combine` (Task 7).
- Produces: `async fn fetch_incident_text(pool: &PgPool, incident_id: &str) -> anyhow::Result<Option<(String, String)>>`; `async fn write_extraction(pool: &PgPool, incident_id: &str, extraction: &PrimaryExtraction, resolution_status: &str, confidence: &str, model_version: &str, text_hash: &str) -> anyhow::Result<()>`; `async fn process_incident(pool: &PgPool, llm: &LlmClient, model_version: &str, incident_id: &str)` (logs and returns on any failure — never propagates, since one incident's extraction failure must not stop the loop or the sweep). This is the enricher's functionally-complete milestone; consumed by nothing further in this plan.

- [ ] **Step 1: Write `crates/enricher/src/queries.rs`**

```rust
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::llm::PrimaryExtraction;

pub async fn fetch_incident_text(pool: &PgPool, incident_id: &str) -> anyhow::Result<Option<(String, String)>> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT summary, description FROM incidents WHERE incident_id = $1")
            .bind(incident_id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn write_extraction(
    pool: &PgPool,
    incident_id: &str,
    extraction: &PrimaryExtraction,
    resolution_status: &str,
    confidence: &str,
    model_version: &str,
    text_hash: &str,
) -> anyhow::Result<()> {
    let schedule_window_json = extraction
        .schedule_window
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let eta: Option<DateTime<Utc>> = extraction.eta;

    sqlx::query(
        "UPDATE incidents SET \
            source_text_hash = $2, \
            extracted_category = $3, \
            extracted_resolution_status = $4, \
            extracted_schedule_window = $5, \
            extracted_eta = $6, \
            extraction_confidence = $7, \
            extraction_model_version = $8, \
            extracted_at = NOW() \
         WHERE incident_id = $1",
    )
    .bind(incident_id)
    .bind(text_hash)
    .bind(&extraction.category)
    .bind(resolution_status)
    .bind(&schedule_window_json)
    .bind(eta)
    .bind(confidence)
    .bind(model_version)
    .execute(pool)
    .await?;

    Ok(())
}
```

- [ ] **Step 2: Add `process_incident` and wire it into both the stream loop and the sweep**

In `crates/enricher/src/main.rs`, add the module declarations and `process_incident`:

```rust
mod combine;
mod config;
mod hash;
mod llm;
mod queries;
mod stream;
mod sweep;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use config::Config;
use llm::LlmClient;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let mut redis = redis_client.get_connection_manager().await?;
    stream::ensure_group(&mut redis).await?;

    let llm = Arc::new(LlmClient::new(config.llm_base_url.clone(), config.llm_api_key.clone(), config.llm_model.clone()));
    let model_version = config.llm_model.clone();

    tokio::spawn(sweep_loop(pool.clone(), Arc::clone(&llm), model_version.clone(), config.sweep_interval_secs));

    loop {
        match stream::read_one(&mut redis).await {
            Ok(Some((entry_id, incident_id))) => {
                process_incident(&pool, &llm, &model_version, &incident_id).await;
                if let Err(err) = stream::ack(&mut redis, &entry_id).await {
                    tracing::error!(error = ?err, entry_id, "failed to ack stream entry");
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::error!(error = ?err, "error reading from incident-text-changed stream");
            }
        }
    }
}

async fn sweep_loop(pool: PgPool, llm: Arc<LlmClient>, model_version: String, interval_secs: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;
        match sweep::fetch_sweep_rows(&pool).await {
            Ok(rows) => {
                let ids = sweep::incidents_needing_extraction(&rows, &model_version);
                tracing::info!(count = ids.len(), "sweep found incidents needing extraction");
                for id in ids {
                    process_incident(&pool, &llm, &model_version, &id).await;
                }
            }
            Err(err) => tracing::error!(error = ?err, "sweep query failed"),
        }
    }
}

/// Runs both extraction passes for one incident and writes the result.
/// Never propagates an error -- a bad response, a timeout, or a schema
/// mismatch leaves the incident's existing columns untouched (or NULL, if
/// this is the first attempt) and simply logs, so the next sweep pass
/// retries it. This is deliberate per the spec: a broken enrichment step
/// must never be able to take displayed status down with it.
async fn process_incident(pool: &PgPool, llm: &LlmClient, model_version: &str, incident_id: &str) {
    let text = match queries::fetch_incident_text(pool, incident_id).await {
        Ok(Some(text)) => text,
        Ok(None) => {
            tracing::warn!(incident_id, "incident vanished before extraction ran");
            return;
        }
        Err(err) => {
            tracing::error!(error = ?err, incident_id, "failed to fetch incident text");
            return;
        }
    };
    let (summary, description) = text;

    let primary = match llm.extract_primary(&summary, &description).await {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(error = ?err, incident_id, "primary extraction failed");
            return;
        }
    };

    let adversarial_status = match llm.extract_adversarial(&summary, &description).await {
        Ok(s) => s,
        Err(err) => {
            tracing::error!(error = ?err, incident_id, "adversarial extraction failed");
            return;
        }
    };

    let (resolution_status, confidence) = combine::combine(&primary.resolution_status, &adversarial_status);
    let text_hash = hash::text_hash(&summary, &description);

    if let Err(err) = queries::write_extraction(pool, incident_id, &primary, &resolution_status, &confidence, model_version, &text_hash).await {
        tracing::error!(error = ?err, incident_id, "failed to write extraction result");
        return;
    }

    tracing::info!(incident_id, resolution_status, confidence, "extraction written");
}
```

- [ ] **Step 3: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p enricher && cargo test -p enricher`
Expected: PASS.

- [ ] **Step 4: Manually verify end-to-end against a real OpenAI-compatible endpoint**

With a local OpenAI-compatible server running (per this project's stated intent to test against one) and a dev Postgres/Redis stack up:

```bash
DATABASE_URL="$DATABASE_URL" REDIS_URL="redis://localhost:6379" \
  LLM_BASE_URL="http://localhost:8080/v1" LLM_MODEL="<your local model name>" \
  cargo run -p enricher &

psql "$DATABASE_URL" -c "INSERT INTO incidents (incident_id, summary, description, operators, affected_stations, priority, validity_periods, is_planned, is_cleared) VALUES ('MANUAL-EXTRACT-TEST', 'Signal failure at Reading', 'This has now been resolved, residual delays may continue this evening.', '{}', '{}', 0, '[]', false, false) ON CONFLICT (incident_id) DO UPDATE SET description = EXCLUDED.description"
redis-cli XADD incident-text-changed '*' incident_id MANUAL-EXTRACT-TEST

psql "$DATABASE_URL" -c "SELECT extracted_resolution_status, extraction_confidence, extracted_category FROM incidents WHERE incident_id = 'MANUAL-EXTRACT-TEST'"
```

Expected: `extracted_resolution_status` is `residual` or `resolved` (the exact call depends on the configured model's judgment on this deliberately residual-shaped example), `extraction_confidence` is set. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM incidents WHERE incident_id = 'MANUAL-EXTRACT-TEST'"`.

- [ ] **Step 5: Commit**

```bash
git add crates/enricher/src/queries.rs crates/enricher/src/main.rs
git commit -m "Wire two-pass extraction into enricher's processing pipeline"
```

---

### Task 9: Deployment — Dockerfile, docker-compose, env files

**Files:**
- Create: `docker/enricher.Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `local.env.example`
- Modify: `dev.env.example`

**Interfaces:** none (deployment only, no code interfaces).

- [ ] **Step 1: Write `docker/enricher.Dockerfile`**

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for the `enricher` service. Same builder pin as
# `api`/`aggregator` (rust:1.88-bookworm) -- this crate pulls in
# sqlx-postgres, same transitive `home` crate version requirement.
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin enricher; \
    else \
      cargo build --bin enricher; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/enricher /usr/local/bin/enricher

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin enricher

COPY --from=builder /usr/local/bin/enricher /usr/local/bin/enricher

USER enricher

ENTRYPOINT ["/usr/local/bin/enricher"]
```

- [ ] **Step 2: Add `redis` and `enricher` services to `docker-compose.yml`**

Add after the `postgres` service block:

```yaml
  redis:
    image: redis:7
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 10
      start_period: 5s
```

Add after the `aggregator` service block:

```yaml
  enricher:
    build:
      context: .
      dockerfile: docker/enricher.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
      redis:
        condition: service_healthy
    environment:
      # crates/enricher/src/config.rs: Config
      DATABASE_URL: postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB}
      REDIS_URL: redis://redis:6379
      LLM_BASE_URL: ${LLM_BASE_URL}
      LLM_API_KEY: ${LLM_API_KEY:-}
      LLM_MODEL: ${LLM_MODEL}
      SWEEP_INTERVAL_SECS: ${SWEEP_INTERVAL_SECS:-3600}
      RUST_LOG: ${RUST_LOG:-info}
```

Also add `REDIS_URL: redis://redis:6379` to the existing `api` service's `environment` block, and add `redis: { condition: service_healthy }` to `api`'s `depends_on`.

- [ ] **Step 3: Add the new env vars to both example env files**

In `local.env.example`, following the existing `RDM_*_BASE_URL` non-functional-placeholder convention:

```bash
# crates/enricher/src/config.rs -- point at your own OpenAI-compatible
# endpoint (local server or hosted provider). No vendor is assumed.
LLM_BASE_URL=http://llm.example.invalid/v1
LLM_API_KEY=
LLM_MODEL=replace-with-your-model-name
SWEEP_INTERVAL_SECS=3600
```

In `dev.env.example`, point at a real local address since dev mode is meant to actually run:

```bash
# crates/enricher/src/config.rs -- point at a locally running
# OpenAI-compatible server (e.g. llama.cpp server, vLLM, Ollama).
LLM_BASE_URL=http://host.docker.internal:8080/v1
LLM_API_KEY=
LLM_MODEL=replace-with-your-local-model-name
SWEEP_INTERVAL_SECS=300
```

- [ ] **Step 4: Verify the compose stack builds**

Run: `docker compose --env-file dev.env build enricher redis`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add docker/enricher.Dockerfile docker-compose.yml local.env.example dev.env.example
git commit -m "Add enricher and redis to the docker-compose stack"
```

---

### Task 10: Helm chart additions

**Files:**
- Modify: `charts/nr-status/values.yaml`
- Modify: `charts/nr-status/templates/_helpers.tpl`
- Modify: `charts/nr-status/templates/secret.yaml`
- Create: `charts/nr-status/templates/redis-deployment.yaml`
- Create: `charts/nr-status/templates/redis-service.yaml`
- Create: `charts/nr-status/templates/enricher-deployment.yaml`

**Interfaces:** none (deployment only).

- [ ] **Step 1: Add `redis` and `enricher` sections to `values.yaml`**

```yaml
# ---------------------------------------------------------------------------
# redis (crates/enricher's trigger queue; crates/api publishes to it)
# ---------------------------------------------------------------------------
redis:
  enabled: true
  image:
    repository: redis
    tag: "7"
    pullPolicy: IfNotPresent
  service:
    port: 6379
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}

# ---------------------------------------------------------------------------
# enricher (crates/enricher/src/config.rs)
# ---------------------------------------------------------------------------
enricher:
  image:
    repository: nr-status/enricher
    tag: ""
    pullPolicy: IfNotPresent
  llm:
    baseUrl: ""
    model: ""
    # -- API key for the OpenAI-compatible endpoint. Empty is valid for
    # local servers that don't require one. Follows the chart's normal
    # secrets rule: an existingSecret takes priority; otherwise this
    # value (possibly empty) is rendered as-is -- never auto-generated,
    # since a random LLM API key is meaningless.
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: llm-api-key
  sweepIntervalSecs: 3600
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

- [ ] **Step 2: Add `redisUrl` and `enricherFullname` helpers to `_helpers.tpl`**

```
{{- define "nr-status.redisFullname" -}}
{{- printf "%s-redis" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
In-cluster Redis URL, consumed by both api (publisher) and enricher
(consumer). Takes root.
*/}}
{{- define "nr-status.redisUrl" -}}
{{- printf "redis://%s:%d" (include "nr-status.redisFullname" .) (int .Values.redis.service.port) }}
{{- end }}
```

- [ ] **Step 3: Add `llm-api-key` to `secret.yaml`**

Add alongside the existing `internal-token` block (before the final `{{- if $data }}`):

```
{{/* llm-api-key: like the per-poller rdm-*-api-key entries, deliberately
     NOT auto-generated -- a random LLM API key is meaningless. Rendered
     (possibly empty) whenever no existingSecret is configured, so the
     enricher pod's secretKeyRef always resolves. */}}
{{- if not .Values.enricher.llm.existingSecret -}}
{{- $_ := set $data "llm-api-key" (.Values.enricher.llm.apiKey | default "" | b64enc) -}}
{{- end -}}
```

- [ ] **Step 4: Write `charts/nr-status/templates/redis-deployment.yaml`**

```yaml
{{- if .Values.redis.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include "nr-status.redisFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "redis") | nindent 4 }}
spec:
  # Fixed at 1 -- redis here is a disposable trigger queue, not a system of
  # record (the enricher's hourly sweep is the durability backstop), so it
  # doesn't need the StatefulSet/PVC machinery postgres has.
  replicas: 1
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "redis") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "redis") | nindent 8 }}
      {{- with .Values.redis.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.redis.podSecurityContext) | nindent 8 }}
      containers:
        - name: redis
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.redis.image) | quote }}
          imagePullPolicy: {{ .Values.redis.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" false) | nindent 12 }}
          ports:
            - name: redis
              containerPort: {{ .Values.redis.service.port }}
              protocol: TCP
          readinessProbe:
            exec:
              command: ["redis-cli", "ping"]
            initialDelaySeconds: 5
            periodSeconds: 5
          livenessProbe:
            exec:
              command: ["redis-cli", "ping"]
            initialDelaySeconds: 15
            periodSeconds: 10
          {{- with .Values.redis.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.redis.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.redis.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.redis.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

- [ ] **Step 5: Write `charts/nr-status/templates/redis-service.yaml`**

```yaml
{{- if .Values.redis.enabled }}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "nr-status.redisFullname" . }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "redis") | nindent 4 }}
spec:
  selector:
    {{- include "nr-status.selectorLabels" (dict "root" . "component" "redis") | nindent 4 }}
  ports:
    - port: {{ .Values.redis.service.port }}
      targetPort: redis
      protocol: TCP
      name: redis
{{- end }}
```

- [ ] **Step 6: Write `charts/nr-status/templates/enricher-deployment.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-enricher" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "enricher") | nindent 4 }}
spec:
  replicas: 1
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "enricher") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "enricher") | nindent 8 }}
      {{- with .Values.enricher.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.enricher.podSecurityContext) | nindent 8 }}
      containers:
        - name: enricher
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.enricher.image) | quote }}
          imagePullPolicy: {{ .Values.enricher.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          # No probes: like aggregator, this binary exposes no HTTP surface.
          env:
            {{- include "nr-status.databaseEnv" . | nindent 12 }}
            - name: REDIS_URL
              value: {{ include "nr-status.redisUrl" . | quote }}
            - name: LLM_BASE_URL
              value: {{ .Values.enricher.llm.baseUrl | quote }}
            - name: LLM_MODEL
              value: {{ .Values.enricher.llm.model | quote }}
            - name: LLM_API_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ default (include "nr-status.secretName" .) .Values.enricher.llm.existingSecret }}
                  key: {{ .Values.enricher.llm.existingSecretApiKeyKey }}
            - name: SWEEP_INTERVAL_SECS
              value: {{ .Values.enricher.sweepIntervalSecs | quote }}
            - name: RUST_LOG
              value: {{ .Values.enricher.logLevel | quote }}
            {{- with .Values.enricher.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.enricher.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.enricher.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.enricher.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.enricher.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 7: Render the chart to confirm it's syntactically valid**

Run: `helm template nr-status ./charts/nr-status --set enricher.llm.baseUrl=http://example.invalid,enricher.llm.model=test`
Expected: PASS — renders without error, and the output includes a `redis` Deployment/Service and an `enricher` Deployment.

- [ ] **Step 8: Commit**

```bash
git add charts/nr-status/values.yaml charts/nr-status/templates/_helpers.tpl charts/nr-status/templates/secret.yaml charts/nr-status/templates/redis-deployment.yaml charts/nr-status/templates/redis-service.yaml charts/nr-status/templates/enricher-deployment.yaml
git commit -m "Add redis and enricher to the Helm chart"
```

---

### Task 11: Thread extraction columns through `aggregator::queries::LoadedIncident`

**Files:**
- Modify: `crates/aggregator/src/queries.rs`

**Interfaces:**
- Produces: `LoadedIncident` gains `extracted_resolution_status: Option<String>`, `extraction_confidence: Option<String>`, `extracted_schedule_window: Option<serde_json::Value>`, `extracted_eta: Option<DateTime<Utc>>`. Consumed by Task 12's `apply_extraction`.

- [ ] **Step 1: Extend `LoadedIncident` and `load_incidents`'s SELECT/mapping**

In `crates/aggregator/src/queries.rs`, change `LoadedIncident`:

```rust
pub struct LoadedIncident {
    pub message: IncidentMessage,
    pub first_seen_at: DateTime<Utc>,
    pub extracted_resolution_status: Option<String>,
    pub extraction_confidence: Option<String>,
    pub extracted_schedule_window: Option<serde_json::Value>,
    pub extracted_eta: Option<DateTime<Utc>>,
}
```

And `load_incidents`:

```rust
pub async fn load_incidents(pool: &PgPool) -> Result<Vec<LoadedIncident>> {
    let rows = sqlx::query(
        "SELECT incident_id, summary, description, operators, affected_stations, \
                priority, validity_periods, is_planned, is_cleared, first_seen_at, \
                extracted_resolution_status, extraction_confidence, \
                extracted_schedule_window, extracted_eta \
         FROM incidents \
         WHERE NOT is_cleared",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let validity_json: serde_json::Value = row.try_get("validity_periods")?;
            let message = IncidentMessage {
                incident_id: row.try_get("incident_id")?,
                summary: row.try_get("summary")?,
                description: row.try_get("description")?,
                operators: row.try_get("operators")?,
                affected_stations: row.try_get("affected_stations")?,
                priority: row.try_get("priority")?,
                validity: serde_json::from_value(validity_json)?,
                is_planned: row.try_get("is_planned")?,
                is_cleared: row.try_get("is_cleared")?,
            };
            Ok(LoadedIncident {
                message,
                first_seen_at: row.try_get("first_seen_at")?,
                extracted_resolution_status: row.try_get("extracted_resolution_status")?,
                extraction_confidence: row.try_get("extraction_confidence")?,
                extracted_schedule_window: row.try_get("extracted_schedule_window")?,
                extracted_eta: row.try_get("extracted_eta")?,
            })
        })
        .collect()
}
```

- [ ] **Step 2: Update every `LoadedIncident` literal in `aggregation.rs`'s test module**

`crates/aggregator/src/aggregation.rs`'s `mod tests` constructs `LoadedIncident` directly in `aggregate_with_defaults` and one other call site (from the stale-incident-handling plan's Task 5). Add the four new fields, defaulted to `None`, to both:

```rust
    fn aggregate_with_defaults(
        lines: &HashMap<String, LineDefinition>,
        incidents: &[IncidentMessage],
    ) -> HashMap<String, LineStatusReport> {
        let registry = SegmentRegistry::new(lines);
        let defaults = Defaults::default();
        let loaded: Vec<LoadedIncident> = incidents
            .iter()
            .cloned()
            .map(|message| LoadedIncident {
                message,
                first_seen_at: Utc::now(),
                extracted_resolution_status: None,
                extraction_confidence: None,
                extracted_schedule_window: None,
                extracted_eta: None,
            })
            .collect();
        aggregate(lines, &loaded, &HashMap::new(), &registry, &defaults)
    }
```

And the sample-stats-blending test's direct `LoadedIncident { message: inc, first_seen_at: Utc::now() }` literal:

```rust
        let loaded = LoadedIncident {
            message: inc,
            first_seen_at: Utc::now(),
            extracted_resolution_status: None,
            extraction_confidence: None,
            extracted_schedule_window: None,
            extracted_eta: None,
        };
```

- [ ] **Step 3: Confirm the workspace still builds**

Run: `cargo build --workspace`
Expected: PASS.

- [ ] **Step 4: Run the full aggregator crate test suite**

Run: `cargo test -p aggregator`
Expected: PASS, no regressions — every existing test still passes since the new fields default to `None`, which Task 12's `apply_extraction` treats as a no-op.

- [ ] **Step 5: Commit**

```bash
git add crates/aggregator/src/queries.rs crates/aggregator/src/aggregation.rs
git commit -m "Load extraction columns into aggregator's LoadedIncident"
```

---

### Task 12: `apply_extraction` severity classifier

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs`
- Test: `crates/aggregator/src/aggregation.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `LoadedIncident`'s extraction fields (Task 11).
- Produces: `fn apply_extraction(severity: Severity, loaded: &LoadedIncident, now: DateTime<Utc>) -> (Severity, Option<String>)` returning the (possibly demoted) severity and an optional annotation to append to `reason`. Wired into `status_from_incident`, whose signature changes from `(m: &Match, incident: &IncidentMessage)` to `(m: &Match, loaded: &LoadedIncident)`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/aggregator/src/aggregation.rs`'s `mod tests` block:

```rust
    fn loaded_with_extraction(
        resolution_status: Option<&str>,
        confidence: Option<&str>,
        schedule_window: Option<serde_json::Value>,
        eta: Option<DateTime<Utc>>,
    ) -> LoadedIncident {
        LoadedIncident {
            message: incident("EXT1", "Signal failure", "Delays expected", &[], &[]),
            first_seen_at: Utc::now(),
            extracted_resolution_status: resolution_status.map(str::to_string),
            extraction_confidence: confidence.map(str::to_string),
            extracted_schedule_window: schedule_window,
            extracted_eta: eta,
        }
    }

    #[test]
    fn apply_extraction_is_a_no_op_with_no_extraction() {
        let loaded = loaded_with_extraction(None, None, None, None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, Utc::now());
        assert_eq!(severity, Severity::Suspended);
        assert_eq!(annotation, None);
    }

    #[test]
    fn apply_extraction_ignores_low_confidence_resolved() {
        let loaded = loaded_with_extraction(Some("resolved"), Some("low"), None, None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, Utc::now());
        assert_eq!(severity, Severity::Suspended);
        assert_eq!(annotation, None);
    }

    #[test]
    fn apply_extraction_demotes_high_confidence_resolved_to_minor_delays() {
        let loaded = loaded_with_extraction(Some("resolved"), Some("high"), None, None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, Utc::now());
        assert_eq!(severity, Severity::MinorDelays);
        assert!(annotation.unwrap().contains("resolved"));
    }

    #[test]
    fn apply_extraction_never_demotes_resolved_below_minor_delays() {
        // "Demote" means push toward a MILDER (higher-ordinal) severity,
        // never a more severe one. GoodService's ordinal (10) is already
        // higher/milder than MinorDelays' (9), so `.max(MinorDelays)` must
        // leave it unchanged, not pull it back down to 9.
        let loaded = loaded_with_extraction(Some("resolved"), Some("high"), None, None);
        let (severity, _) = apply_extraction(Severity::GoodService, &loaded, Utc::now());
        assert_eq!(severity, Severity::GoodService);
    }

    #[test]
    fn apply_extraction_demotes_high_confidence_residual_to_recovering() {
        let loaded = loaded_with_extraction(Some("residual"), Some("high"), None, None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, Utc::now());
        assert_eq!(severity, Severity::Recovering);
        assert!(annotation.is_some());
    }

    #[test]
    fn apply_extraction_ongoing_is_a_no_op() {
        let loaded = loaded_with_extraction(Some("ongoing"), Some("high"), None, None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, Utc::now());
        assert_eq!(severity, Severity::Suspended);
        assert_eq!(annotation, None);
    }

    #[test]
    fn apply_extraction_demotes_when_now_is_outside_the_schedule_window() {
        // Window is 22:00-06:00 every day; "now" is fixed at a UTC instant
        // that's midday in London.
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap(); // 13:00 BST
        let window = serde_json::json!({ "days_of_week": [1,2,3,4,5,6,7], "start_time": "22:00", "end_time": "06:00" });
        let loaded = loaded_with_extraction(None, None, Some(window), None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, now);
        assert_eq!(severity, Severity::MinorDelays);
        assert!(annotation.is_some());
    }

    #[test]
    fn apply_extraction_no_op_when_now_is_inside_an_overnight_schedule_window() {
        let now: DateTime<Utc> = "2026-06-15T23:00:00Z".parse().unwrap(); // 00:00 BST, inside 22:00-06:00
        let window = serde_json::json!({ "days_of_week": [1,2,3,4,5,6,7], "start_time": "22:00", "end_time": "06:00" });
        let loaded = loaded_with_extraction(None, None, Some(window), None);
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, now);
        assert_eq!(severity, Severity::Suspended);
        assert_eq!(annotation, None);
    }

    #[test]
    fn apply_extraction_demotes_when_eta_has_already_passed() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let eta = now - Duration::hours(1);
        let loaded = loaded_with_extraction(None, None, None, Some(eta));
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, now);
        assert_eq!(severity, Severity::MinorDelays);
        assert!(annotation.unwrap().contains("expected to end"));
    }

    #[test]
    fn apply_extraction_no_op_when_eta_is_in_the_future() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let eta = now + Duration::hours(1);
        let loaded = loaded_with_extraction(None, None, None, Some(eta));
        let (severity, annotation) = apply_extraction(Severity::Suspended, &loaded, now);
        assert_eq!(severity, Severity::Suspended);
        assert_eq!(annotation, None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aggregator apply_extraction`
Expected: FAIL — compile error, `apply_extraction` is not defined yet.

- [ ] **Step 3: Implement `apply_extraction`**

Add to `crates/aggregator/src/aggregation.rs`, after `demote_for_scope`:

```rust
#[derive(Deserialize)]
struct ScheduleWindow {
    days_of_week: Vec<u8>,
    start_time: String,
    end_time: String,
}

/// Whether `now` (converted to Europe/London local time) falls inside
/// `window`. Handles overnight windows (e.g. 22:00-06:00, where
/// `start_time > end_time`) by wraparound: "inside" means at or after
/// `start_time` OR before `end_time`, rather than requiring both.
fn now_within_window(window: &ScheduleWindow, now: DateTime<Utc>) -> bool {
    let local = now.with_timezone(&chrono_tz::Europe::London);
    let weekday = local.weekday().number_from_monday() as u8; // 1=Monday..7=Sunday
    if !window.days_of_week.contains(&weekday) {
        return false;
    }
    let Ok(start) = NaiveTime::parse_from_str(&window.start_time, "%H:%M") else { return true };
    let Ok(end) = NaiveTime::parse_from_str(&window.end_time, "%H:%M") else { return true };
    let now_time = local.time();
    if start <= end {
        now_time >= start && now_time < end
    } else {
        now_time >= start || now_time < end
    }
}

/// Adjusts `severity` based on NLP-extracted signals, and returns an
/// optional annotation to append to the status's `reason` text. Runs
/// between `severity_from_incident` and `demote_for_scope` in
/// `status_from_incident`. Can only demote (raise the numeric severity,
/// since lower is worse) or leave `severity` unchanged -- never make it
/// more severe, and never signals suppression. A missing or
/// low-confidence extraction is always a no-op: the absence of a signal
/// must behave identically to this function not existing at all. See
/// docs/superpowers/specs/2026-08-20-incident-nlp-extraction-design.md §7.
fn apply_extraction(severity: Severity, loaded: &LoadedIncident, now: DateTime<Utc>) -> (Severity, Option<String>) {
    let high_confidence = loaded.extraction_confidence.as_deref() == Some("high");

    if high_confidence {
        match loaded.extracted_resolution_status.as_deref() {
            Some("resolved") => {
                return (
                    severity.max(Severity::MinorDelays),
                    Some("reported resolved -- showing residual impact".to_string()),
                );
            }
            Some("residual") => {
                return (severity.max(Severity::Recovering), Some("reported as residual delays only".to_string()));
            }
            _ => {}
        }
    }

    if let Some(window_json) = &loaded.extracted_schedule_window {
        if let Ok(window) = serde_json::from_value::<ScheduleWindow>(window_json.clone()) {
            if !now_within_window(&window, now) {
                return (
                    severity.max(Severity::MinorDelays),
                    Some(format!("reported active {}-{} only", window.start_time, window.end_time)),
                );
            }
        }
    }

    if let Some(eta) = loaded.extracted_eta {
        if eta < now {
            return (
                severity.max(Severity::MinorDelays),
                Some(format!("expected to end by {}", eta.with_timezone(&chrono_tz::Europe::London).format("%H:%M"))),
            );
        }
    }

    (severity, None)
}
```

Add `use chrono::Weekday;`-independent `.weekday()` is already available via `chrono::Datelike` — add that import and `serde::Deserialize` (for the local `ScheduleWindow` struct) to the top-of-file imports:

```rust
use chrono::Datelike;
use serde::Deserialize;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p aggregator apply_extraction`
Expected: PASS — all eleven tests pass.

- [ ] **Step 5: Wire `apply_extraction` into `status_from_incident`**

Change `status_from_incident`'s signature and body:

```rust
fn status_from_incident(m: &Match, loaded: &LoadedIncident) -> LineStatus {
    let incident = &loaded.message;
    let base_severity = severity_from_incident(incident);
    let (extracted_severity, extraction_annotation) = apply_extraction(base_severity, loaded, Utc::now());
    let severity = demote_for_scope(extracted_severity, m.scope);

    let affected_stations = m.evidence.stations.clone();
    let affected_routes = routes_from_stations(m.line, &affected_stations);

    let mut reason = incident.summary.clone();
    match m.scope {
        MatchScope::SharedSegment => reason.push_str(" (shared trunk — also affects other lines)"),
        MatchScope::OperatorOnly => reason.push_str(" (operator-wide report)"),
        _ => {}
    }
    if let Some(annotation) = extraction_annotation {
        reason.push_str(&format!(" ({annotation})"));
    }

    let disruption = Disruption {
        category: if incident.is_planned { "PlannedWork" } else { "RealTime" }.to_string(),
        description: if incident.description.is_empty() { incident.summary.clone() } else { incident.description.clone() },
        affected_stops: affected_stations,
        affected_routes,
        source: Some(format!("knowledgebase-incident-{}", incident.incident_id)),
    };

    LineStatus {
        severity,
        reason,
        validity: validity_for_output(&incident.validity),
        disruption: Some(disruption),
        data_quality: if incident.is_planned { DataQuality::Planned } else { DataQuality::Knowledgebase },
        sample_stats: None,
    }
}
```

And update its one call site in `aggregate()`:

```rust
    for loaded in incidents.iter().filter(|loaded| is_active(&loaded.message, loaded.first_seen_at, now)) {
        for m in lines_affected_by(&loaded.message, lines, registry) {
            let status = status_from_incident(&m, loaded);
            reports.get_mut(&m.line.id).unwrap().statuses.push(status);
        }
    }
```

- [ ] **Step 6: Run the full aggregator crate test suite**

Run: `cargo test -p aggregator`
Expected: PASS — every existing test still passes (every existing `LoadedIncident` fixture has all four extraction fields `None`, so `apply_extraction` is a no-op for all of them, and `status_from_incident`'s output for those is unchanged) plus the eleven new `apply_extraction` tests.

- [ ] **Step 7: Commit**

```bash
git add crates/aggregator/src/aggregation.rs
git commit -m "Apply NLP-extracted resolution status/schedule/ETA to severity classification"
```

---

### Task 13: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions anywhere in the workspace.

- [ ] **Step 2: Run `cargo clippy` across the workspace**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings introduced by this plan's changes (pre-existing warnings elsewhere in the workspace, if any, are out of scope).

- [ ] **Step 3: Bring up the full dev stack and confirm every new service is healthy**

```bash
docker compose --env-file dev.env up --build -d
docker compose ps
```

Expected: `redis` and `enricher` both show as running (redis: healthy; enricher: no healthcheck defined, so just "running" with no immediate restart loop in `docker compose logs enricher`).

- [ ] **Step 4: Manually verify the full pipeline end-to-end**

Post an incident whose text will change on a follow-up call, confirm it shows full severity, then edit it to say resolved and confirm the displayed severity demotes after enrichment runs:

```bash
source dev.env
curl -s -X POST http://localhost:8080/private/incidents \
  -H "x-internal-token: $INTERNAL_TOKEN" -H "Content-Type: application/json" \
  -d '[{"incident_id":"MANUAL-PIPELINE-TEST","summary":"Signal failure at Woking","description":"Disruption ongoing.","operators":["SW"],"affected_stations":["WOK"],"priority":0,"validity":[],"is_planned":false,"is_cleared":false}]'

# wait one aggregator cycle, then:
curl -s http://localhost:8080/Line/swr-alton/Status | grep -o '"reason":"[^"]*"'
```

Expected: shows the full-severity reason, unchanged from today's behavior (no extraction has run yet for a `null` `extracted_resolution_status`).

```bash
curl -s -X POST http://localhost:8080/private/incidents \
  -H "x-internal-token: $INTERNAL_TOKEN" -H "Content-Type: application/json" \
  -d '[{"incident_id":"MANUAL-PIPELINE-TEST","summary":"Signal failure at Woking","description":"This has now been resolved.","operators":["SW"],"affected_stations":["WOK"],"priority":0,"validity":[],"is_planned":false,"is_cleared":false}]'

# wait for enricher to process the resulting text-changed event (check
# `docker compose logs enricher --follow` for "extraction written"), then
# wait one more aggregator cycle:
curl -s http://localhost:8080/Line/swr-alton/Status | grep -o '"severity":[0-9]*\|"reason":"[^"]*"'
```

Expected: severity is `9` (MinorDelays) or milder, and the reason includes the "reported resolved" annotation — assuming the configured `LLM_MODEL` correctly classifies this deliberately unambiguous example as `resolved` with high confidence; if it doesn't, that's a model-quality finding for the golden-corpus eval this plan's Task 6/7 tests don't replace, not a bug in this plan's wiring.

Clean up: `curl -s -X POST http://localhost:8080/private/incidents ... "isCleared":true` or `psql "$DATABASE_URL" -c "DELETE FROM incidents WHERE incident_id = 'MANUAL-PIPELINE-TEST'"`.

- [ ] **Step 5: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree (everything committed task-by-task above).
