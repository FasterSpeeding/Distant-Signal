# Schedule-Feed-Derived STANOX->CRS Table Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `trust-consumer`'s static, hand-regenerated `reference-data/stanox-crs.csv` (loaded once at startup, never refreshed) with a live-refreshed STANOX->CRS table sourced from the `MCA`/`MSN` files `schedule-ingest` already receives daily — without inventing a new feed, licence, or architectural pattern. A new sibling container, `schedule-reference` (a new `crates/schedule-reference` crate), joins the existing multi-container `schedulefeed` Pod, mounts the same PVC read-only, parses the `TI` records in `RJTTF<n>MCA.txt` and the `A` records in `RJTTF<n>MSN.txt` once `schedule-ingest` has moved a verified-complete delivery into `storage_dir/<n>/`, applies the same STANOX-ambiguity disambiguation policy the CSV was hand-curated under, and `POST`s the resolved rows to a new `/private/stanox-crs` endpoint — the same `poller-* -> /private/X -> Postgres` shape `poller-stations` already uses. `trust-consumer` gains a second periodic HTTP reload (alongside its existing `reference_reload_secs` tracked-trains reload) that swaps a shared cell holding the live table; the CSV stays exactly as it is today — the startup value and the permanent fail-open fallback, never deleted.

**Architecture:**

```
 Darwin Timetable Files (DTD), daily full-refresh push, via SFTP
        |
        v
 schedulefeed Pod (existing 2 containers + 1 NEW)
   sftp (SFTPGo, 3rd-party)  <--PVC "data"-->  ingest (crates/schedule-ingest)
                                                  | verifies + moves to storage_dir/<n>/
                                                  | POST {sequence, files} (unchanged)
                                                  v
                                        /private/schedule-feed-ingests
                                                                    ^
                                                                    | reads storage_dir/<n>/
                                                                    | RJTTF<n>MCA.txt (TI lines only)
                                                                    | RJTTF<n>MSN.txt (A lines only)
                                        <--PVC "data", READ-ONLY-->  reference (NEW,
                                                                      crates/schedule-reference)
                                                                    | parses + disambiguates
                                                                    | POST resolved rows
                                                                    v
                                                          /private/stanox-crs (NEW)
                                                                    |
                                        api (Postgres): stanox_crs table (NEW)
                                        (stanox PK, crs, tiploc, station_name,
                                         source_sequence, updated_at)
                                                                    ^
                                                                    | GET /private/stanox-crs,
                                                                    | on stanox_crs_reload_secs tick
                                                                    |
                                        trust-consumer: --stanox-crs-file CSV at startup
                                        (unchanged fallback) -> Arc<RwLock<StanoxCrsTable>>
                                        swapped by the new reload block, read once per
                                        run_cycle by the existing run_once/process_message
```

**Tech Stack:** Rust (new `crates/schedule-reference` binary crate; `trust-consumer`/`api`/`common` extended, no other crate touched); `reqwest`+`clap`+`tokio` matching every existing poller's dependency set exactly (see Task 1); Postgres via `sqlx`, one new migration; Helm chart (new third container in the existing `schedulefeed` Deployment, new `trustConsumer.stanoxCrs*` values).

**Spec:** `docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md` (749 lines, read in full before starting) — this plan turns that design's five Decisions into concrete tasks and does not re-litigate them. Cross-references below to "Decision N" refer to that document.

**Status note — every citation below re-confirmed directly against this worktree's current source, not trusted from the spec:** `crates/schedule-ingest/src/main.rs`'s module doc (lines 1-11), `run_scan_cycle` (144-296), `post_ingest` (309-325), `ScheduleFeedIngestRequest`/`ScheduleFeedFile` (335-346), `prune_old_sequences` (445-484) all match the spec's own citations. `crates/trust-consumer/src/stanox_crs.rs`'s `StanoxCrsTable` (61-64), `from_file`/`parse` (72-132), `stanox_to_crs` (143-153) match exactly; its existing `REAL_EUSTON`/`REAL_VICTORIA`/`REAL_VICTORIA_CARRIAGE_ROAD` fixture constants (191-200) and their byte-offset `decode` helper (`[44..49]`/`[53..56]`, lines 202-204) are reused directly by this plan's Task 2. `crates/trust-consumer/src/config.rs`'s `reference_reload_secs` (68-72) and `stanox_crs` field (89-108, loaded via a clap `value_parser` at parse time) match. `crates/trust-consumer/src/main.rs`'s reload block (51-78) and `run_cycle` (126-156, taking `stanox_crs: &stanox_crs::StanoxCrsTable`) match. `crates/trust-consumer/src/process.rs`'s `apply_reference_reload` (209-233) and `run_once`/`process_message` (251-282, 284-474, both taking `&crate::stanox_crs::StanoxCrsTable`) match. `crates/poller-stations/src/main.rs` (28-111, flat `tokio::time::interval` + `ingest::post_batch`) and `crates/common/src/ingest.rs`'s `post_batch` (35-57)/`INTERNAL_TOKEN_HEADER` (22) match. `crates/api/src/routes/ingest.rs`'s `router()` (28-53), `post_stations` (111-119), `get_active_tracked_trains`/`post_train_events` (160-183, the `Vec<T>`-direct GET/POST shape this plan's new route mirrors) match; `crates/api/src/routes/mod.rs:58-61` confirms `ingest::router()` is merged into `private_router()`. `crates/api/src/data/queries.rs`'s `upsert_stations` (221-253, `ON CONFLICT (crs) DO UPDATE`) and `last_stations_fetch`/`insert_schedule_feed_ingest` (494-561) match, as does the live-database test convention at `queries.rs:1024-1059` (`#[ignore = "requires a live database; ..."]` + `DATABASE_URL` env var). `charts/distant-signal/templates/schedulefeed-deployment.yaml`'s header comment (1-8), `data`/`host-key`/`sftp-entrypoint` volumes (53-67), and the `ingest` container block (177-231) match; `charts/distant-signal/templates/trust-consumer-deployment.yaml`'s env block (80-118, including `REFERENCE_RELOAD_SECS` at 108-109) matches. `charts/distant-signal/values.yaml`'s `trustConsumer` (596-625), `scheduleFeed` (638-757, `ingest.image`/`checkTimes` etc. at 712-719), and `metrics` (839-855, `port: 9091` at 855) blocks all match. `reference-data/stanox-crs.md`'s full `TI` byte-offset table (65-75: `0..2` type, `2..9` TIPLOC, `18..44` name, `44..49` STANOX, `53..56` CRS) was independently re-verified this session against real bytes (see Task 2). The 749-line spec's own `A`-record byte table (`0..1` type, `5..35` name, `35..36` CATE, `36..43` TIPLOC, `43..46` subsidiary CRS, `49..52` CRS) was also independently re-verified this session directly against `timetable_full.zip`'s real `RJTTF942MSN.txt` (see Task 2's fixtures).

## Global Constraints

- **New crate name is `schedule-reference`** (`crates/schedule-reference`, binary `schedule-reference`), per the spec's own Decision 1 working name — explicitly **not** `poller-corpus` (the spec's Decision 1/Explicitly-out-of-scope rejects CORPUS as a source at all) and not folded into `schedule-ingest` (a deliberate, already-made scope boundary the spec's Decision 1(a) explains at length — do not merge these two crates in any task).
- **Full monthly-timetable ingestion (`BS`/`BX`/`LO`/`LI`/`CR`/`LT` parsing, STP resolution, a schedules/calling-points schema, a journey planner) is explicitly out of scope** (Decision 5). No task below touches anything but `TI` and `A` records.
- **`reference-data/stanox-crs.csv` and its loader (`stanox_crs.rs::from_file`) are kept, not deleted, indefinitely** (Decision 3) — the committed default for local dev / any environment without `scheduleFeed.enabled`, and the permanent fail-open fallback if the live table is ever empty, unreachable, or not-yet-populated. No task deletes this file, its test fixtures, or its `from_file`/`parse` code path.
- **The STANOX-ambiguity disambiguation policy (prefer the sole non-`X`-prefixed CRS candidate; otherwise exclude the STANOX entirely) must be reimplemented as real, tested code in the new crate — it is not inherited from the CSV.** The exact same 14 ambiguous STANOX values documented in `reference-data/stanox-crs.md:96-113` (and independently reconfirmed by this session against the real `timetable_full.zip`, see Task 2) must resolve identically: 9 resolved, 5 excluded (`52215`, `86935`, `87981`, `89428`, `89530`).
- **No invented test data.** Every fixture line used in this plan's tests is either already a real, byte-verified constant in `crates/trust-consumer/src/stanox_crs.rs` (`REAL_EUSTON`, `REAL_VICTORIA`, `REAL_VICTORIA_CARRIAGE_ROAD`) or was extracted directly from `timetable_full.zip` (`/workspaces/github-com-fasterspeeding-network-rail-status/timetable_full.zip`, the repo root's real 2026-08-28 CIF extract) by this planning session via `unzip -p timetable_full.zip RJTTF942MCA.txt | awk ...` / `unzip -p timetable_full.zip RJTTF942MSN.txt | grep ...` and is quoted verbatim in Task 2 below. No task may fabricate a `TI`/`A` line; if a future task needs a fixture this plan didn't extract, pull it from the real zip the same way.
- **`schedule-reference` mounts the shared `data` PVC read-only** (`readOnly: true` on its `volumeMount`, mirroring the pattern already used elsewhere in this chart per the spec's Decision 1(c)) and never writes to it, never renames/deletes anything `schedule-ingest` placed there, and tracks the last-processed sequence **in-memory only** (reset on restart) — the same honestly-scoped limitation `schedule-ingest` itself already carries for its own last-ingested-sequence tracking (`schedule-ingest/src/main.rs:13-32`), reused deliberately per Decision 1's "more benign" analysis (a restart-induced reprocess is a harmless upsert no-op, not a data-loss risk).
- **Every daily delivery is a full refresh, not a delta** (spec's Current relevant state, independently confirmed against `docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md:258-275`) — `POST /private/stanox-crs` is a full-table upsert-by-`stanox` every successful run, never a partial/delta write, and the query layer needs no separate "delete rows missing from today's delivery" step.
- **Metrics-port collision inside the `schedulefeed` Pod is real and must be resolved, not hand-waved.** `ingest` already binds `.Values.metrics.port` (default `9091`) for its own Prometheus listener inside the *same* Pod network namespace `schedule-reference` now joins as a third container — reusing the same port for two Rust binaries in one Pod would fail to bind. Task 7 gives `schedule-reference` its own distinct metrics port value.
- **No frontend or public API surface change** (spec's Explicitly out of scope) — `stanox_crs` is reached only via `/private/stanox-crs`; no task touches `/public/freshness`, `frontend/`, or any `/public/*` route.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file, run via `cargo test -p <crate>`. `crates/api`'s two new queries are DB-touching and follow the existing live-database convention exactly (`crates/api/src/data/queries.rs:1024-1059`): `#[ignore = "requires a live database; run with \`cargo test -p api <test_name> -- --ignored\`"]`, `DATABASE_URL` read from the environment, seed/assert/cleanup in one test body. Every other test in this plan (parsing, disambiguation, sequence-selection, the trust-consumer reload swap) is a plain, non-DB `cargo test -p <crate>`.
- **Migration filename convention:** `crates/api/migrations/<YYYYMMDDHHMMSS>_<name>.sql`, timestamp strictly after the current latest (`20260901140000_standalone_tickets.sql`) — this plan's migration is `20260901150000_stanox_crs.sql`.
- **Parallelizable tasks:** Task 1 (crate scaffold + sequence-selection), Task 2 (pure parsing module), and Task 3 (api endpoint + migration) touch disjoint files and have no dependency on each other — dispatch in parallel. Task 4 depends on Tasks 1-3 (it assembles them). Task 5 (trust-consumer) depends only on Task 3's wire shape (`common::StanoxCrsRecord`), not on Task 4's crate existing or running. Task 6 depends on Task 5. Task 7 (Helm) depends on Task 4 (needs the new binary/image to exist) and Task 5 (needs the new env vars). Task 8 depends on everything.

---

### Task 1: `schedule-reference` crate scaffolding + sequence-selection logic

**Files:**
- Modify: `Cargo.toml` (workspace `members`, currently lines 1-13)
- Create: `crates/schedule-reference/Cargo.toml`
- Create: `crates/schedule-reference/src/config.rs`
- Create: `crates/schedule-reference/src/sequence.rs`
- Create: `crates/schedule-reference/src/main.rs` (skeleton only — `poll_once`'s parsing/POST body is filled in by Task 4, once Task 2's parser exists)

**Interfaces:**
- Produces: `sequence::highest_complete_sequence(storage_dir: &Path) -> anyhow::Result<Option<u32>>`. `config::Config` (clap `Parser`) with fields `storage_dir: PathBuf`, `poll_interval_secs: u64`, `api_ingest_url: String`, `internal_token: String`, `metrics_port: u16`, `metrics_enabled: bool`.
- Consumed by: Task 4 (`main.rs`'s `poll_once` calls `sequence::highest_complete_sequence` and reads `Config`'s fields).
- **Depends on:** nothing.

`crates/poller-stations/src/main.rs` (28-111) is this task's closest precedent — a flat `tokio::time::interval` loop, not `schedule-ingest`'s check-time-list scheduler, per Decision 4's "moderately frequent interval (candidate: every 30-60 minutes)... since this crate reads an already-local, already-verified-complete sequence directory (no network fetch of its own)". `schedule-ingest/src/main.rs:445-484`'s `prune_old_sequences` is the closest precedent for `sequence::highest_complete_sequence`'s numeric-subdirectory scan — independently reimplemented here, not shared code across the crate boundary (Decision 1's own explicit note).

- [ ] **Step 1: Add the crate to the workspace**

In `Cargo.toml`, add `"crates/schedule-reference",` to `members` (after `"crates/schedule-ingest",`).

- [ ] **Step 2: Create `crates/schedule-reference/Cargo.toml`**

Mirrors `crates/poller-stations/Cargo.toml` exactly (same dependency set, same versions — this crate makes one outbound HTTP POST and does local filesystem I/O, nothing `poller-stations` doesn't already need):

```toml
[package]
name = "schedule-reference"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
metrics = "0.24"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write `sequence.rs`'s failing test**

```rust
use std::path::Path;

/// The highest-numbered immediate subdirectory of `storage_dir` that
/// contains both an `RJTTF<n>MCA.txt` and an `RJTTF<n>MSN.txt` file.
/// Mirrors `schedule-ingest`'s own `prune_old_sequences`
/// (crates/schedule-ingest/src/main.rs:445-484) numeric-subdirectory-scan
/// technique -- independently reimplemented here, not shared code across
/// the crate boundary (see this design's Decision 1: "a new, small,
/// independently-written function, not shared code"). `None` if
/// `storage_dir` doesn't exist yet, or no subdirectory has both files --
/// not an error, matching `schedule-ingest::scan::scan_incoming`'s own
/// "not-yet-existing is empty, not an error" posture.
pub fn highest_complete_sequence(storage_dir: &Path) -> anyhow::Result<Option<u32>> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, sequence: &str, name: &str) {
        std::fs::write(dir.join(sequence).join(name), b"x").unwrap();
    }

    #[test]
    fn picks_the_highest_sequence_with_both_files_present() {
        let dir = tempfile::tempdir().unwrap();
        for seq in ["940", "941", "942"] {
            std::fs::create_dir_all(dir.path().join(seq)).unwrap();
        }
        // 942 is missing MSN -- must not be picked.
        touch(dir.path(), "940", "RJTTF940MCA.txt");
        touch(dir.path(), "940", "RJTTF940MSN.txt");
        touch(dir.path(), "941", "RJTTF941MCA.txt");
        touch(dir.path(), "941", "RJTTF941MSN.txt");
        touch(dir.path(), "942", "RJTTF942MCA.txt");

        assert_eq!(highest_complete_sequence(dir.path()).unwrap(), Some(941));
    }
}
```

- [ ] **Step 4: Run the test to verify it fails (panics on `unimplemented!()`)**

Run: `cargo test -p schedule-reference picks_the_highest_sequence -- --nocapture`
Expected: panic with `not implemented`.

- [ ] **Step 5: Implement `highest_complete_sequence`**

```rust
pub fn highest_complete_sequence(storage_dir: &Path) -> anyhow::Result<Option<u32>> {
    let read_dir = match std::fs::read_dir(storage_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut sequences: Vec<u32> = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Ok(sequence) = name.parse::<u32>() else { continue };

        let dir = entry.path();
        let has_mca = dir.join(format!("RJTTF{sequence}MCA.txt")).is_file();
        let has_msn = dir.join(format!("RJTTF{sequence}MSN.txt")).is_file();
        if has_mca && has_msn {
            sequences.push(sequence);
        }
    }

    Ok(sequences.into_iter().max())
}
```

- [ ] **Step 6: Add the two remaining regression tests**

```rust
    #[test]
    fn nonexistent_storage_dir_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist-yet");
        assert_eq!(highest_complete_sequence(&missing).unwrap(), None);
    }

    #[test]
    fn a_sequence_with_only_one_of_the_two_files_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("942")).unwrap();
        touch(dir.path(), "942", "RJTTF942MCA.txt"); // MSN missing
        assert_eq!(highest_complete_sequence(dir.path()).unwrap(), None);
    }
```

- [ ] **Step 7: Run the full test module**

Run: `cargo test -p schedule-reference`
Expected: PASS (3 tests).

- [ ] **Step 8: Write `config.rs`**

```rust
use std::path::PathBuf;

use clap::Parser;

/// CLI/env configuration for the `schedule-reference` service.
///
/// Mounts the same PVC `schedule-ingest` writes to, READ-ONLY -- see
/// docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md's
/// Decision 1(c). Never writes to `storage_dir`.
#[derive(Debug, Parser)]
pub struct Config {
    /// Root of the shared PVC -- same path `schedule-ingest`'s own
    /// `--storage-dir` writes into (`crates/schedule-ingest/src/config.rs`),
    /// mounted read-only in this container.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// How often to check `storage_dir` for a new complete sequence.
    /// Independent of the underlying daily delivery cadence -- see
    /// Decision 4: most checks find nothing new, since a fresh sequence
    /// only lands roughly once a day, but reading an already-local
    /// directory listing is cheap.
    #[arg(long, env, default_value_t = 1800)]
    pub poll_interval_secs: u64,

    /// The `api` crate's ingestion endpoint for resolved STANOX/CRS rows.
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub api_ingest_url: String,

    /// Shared secret sent via `X-Internal-Token` to reach the `api`
    /// endpoint above (see `crates/api/src/auth.rs`).
    #[arg(long, env)]
    pub internal_token: String,

    /// Port for this service's Prometheus `/metrics` endpoint. MUST differ
    /// from the `ingest` sibling container's own metrics port -- both
    /// containers share one Pod network namespace (see this plan's Global
    /// Constraints).
    #[arg(long, env, default_value_t = 9092)]
    pub metrics_port: u16,

    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

- [ ] **Step 9: Write `main.rs`'s skeleton**

`poll_once`'s body is intentionally a stub here — Task 4 fills it in once Task 2's parser exists. This step exists so the crate compiles and the loop/metrics/logging shape is locked in now.

```rust
//! `schedule-reference`: a sibling container in the `schedulefeed` Pod.
//! Once `schedule-ingest` has moved a verified-complete delivery into
//! `storage_dir/<n>/`, reads that sequence's `RJTTF<n>MCA.txt` (`TI`
//! records) and `RJTTF<n>MSN.txt` (`A` records) directly off the
//! already-local, read-only-mounted PVC, resolves a STANOX->CRS table, and
//! POSTs it to `api`'s `/private/stanox-crs`. See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md.

mod config;
mod sequence;

use std::time::Duration;

use clap::Parser;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut last_processed_sequence: Option<u32> = None;

    loop {
        interval.tick().await;
        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &mut last_processed_sequence).await;
        metrics::histogram!(common::metrics::metric_name("schedule_reference_cycle_duration_seconds"))
            .record(cycle_start.elapsed().as_secs_f64());
        if let Err(err) = result {
            tracing::error!(error = ?err, "schedule-reference cycle failed; will retry next interval");
        }
    }
}

/// Filled in by this plan's Task 4, once `crates/schedule-reference/src/parser.rs`
/// exists: scan for the highest complete sequence, skip if unchanged since
/// `last_processed_sequence`, else read+parse+POST and update it only on
/// a successful POST.
async fn poll_once(
    _client: &Client,
    config: &Config,
    last_processed_sequence: &mut Option<u32>,
) -> anyhow::Result<()> {
    let Some(sequence) = sequence::highest_complete_sequence(&config.storage_dir)? else {
        tracing::debug!("no complete MCA+MSN sequence directory found yet");
        return Ok(());
    };
    if Some(sequence) == *last_processed_sequence {
        tracing::debug!(sequence, "no new sequence since last successful parse");
        return Ok(());
    }
    tracing::info!(sequence, "TODO(Task 4): parse and POST this sequence");
    Ok(())
}
```

- [ ] **Step 10: Build the whole workspace**

Run: `cargo build --workspace`
Expected: PASS — confirms the new crate is correctly wired into the workspace and compiles alongside every other crate.

- [ ] **Step 11: Commit**

```bash
git add Cargo.toml crates/schedule-reference
git commit -m "Scaffold crates/schedule-reference: config, sequence-selection, and a stub main loop"
```

---

### Task 2: MSN/MCA parsing + disambiguation (pure module)

**Files:**
- Create: `crates/schedule-reference/src/parser.rs`
- Modify: `crates/schedule-reference/src/main.rs` (add `mod parser;`)

**Interfaces:**
- Produces: `parser::TiRecord { tiploc: String, station_name: String, stanox: Option<String>, crs: Option<String> }`. `parser::parse_ti_lines(text: &str) -> Vec<TiRecord>`. `parser::parse_msn_a_lines(text: &str) -> HashMap<String, String>` (TIPLOC -> CRS). `parser::ParsedRow { stanox: String, crs: String, tiploc: String, station_name: String }`. `parser::resolve(ti: &[TiRecord], msn_crs_by_tiploc: &HashMap<String, String>) -> Vec<ParsedRow>`.
- Consumed by: Task 4 (`main.rs`'s `poll_once` calls all three in sequence).
- **Depends on:** nothing (pure, no I/O — testable in isolation, per this repo's "keep parsing logic pure and testable separately from I/O" convention, matching `schedule-ingest::manifest::parse`'s own shape of taking `&str` rather than a path).

**Real fixture bytes used below** (extracted this planning session directly from `timetable_full.zip`'s `RJTTF942MCA.txt`/`RJTTF942MSN.txt` via `unzip -p timetable_full.zip RJTTF942MCA.txt | awk '...'` / `grep`, per this plan's Global Constraints "no invented test data"):

```text
TI Euston (unambiguous, from stanox_crs.rs's existing REAL_EUSTON):
TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON

TI Victoria / Victoria Carriage Road (ambiguous, resolves to VIC -- from
stanox_crs.rs's existing REAL_VICTORIA / REAL_VICTORIA_CARRIAGE_ROAD):
TIVICTRIA00542600PLONDON VICTORIA           87201   0VICLONDON VICTORIA
TIVICTRCR48542662MVICTORIA CARRIAGE ROAD    87201   0XVR

TI WATRLMN (blank CRS -- the CRS-completion case):
TIWATRLMN16559801RLONDON WATERLOO           87212   0

A WATRLMN (MSN's completion for the above -- resolves to WAT):
A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815

TI pairs for 3 of the 5 genuinely irresolvable STANOX (two non-X candidates,
no principled tiebreaker -- excluded entirely):
TIASHFKI 24546600TASHFORD INT (PLATS 3-4)   89428   0ASIASHFORD INTL
TIASHFKY 08500400QASHFORD INTERNATIONAL     894283025AFKASHFORD KENT
TIEBSFDOM00556600WEBBSFLEET INTL (DOMESTIC) 89530   0EBDEBBSFLEET INT SE
TIEBSFLTI32154400BEBBSFLEET INTERNATIONAL   89530   0EBF
TIPOLEFT 24589100XPOOLE FERRY TERMINAL      86935   0PFT
TIPOOLE  00588300RPOOLE                     86935   0POOPOOLE
```

Byte layout, independently re-verified this session (`reference-data/stanox-crs.md:65-75` for `TI`; the spec's own re-derived table for `A`):

| `TI` bytes | Field | | `A` bytes | Field |
|---|---|---|---|---|
| `0..2` | `"TI"` | | `0..1` | `"A"` |
| `2..9` | TIPLOC (7, space-padded) | | `5..35` | Station name (30, space-padded) |
| `18..44` | Station name (26, space-padded) | | `35..36` | CATE digit |
| `44..49` | STANOX (5 digits; blank/`00000` = none) | | `36..43` | TIPLOC (7, space-padded) |
| `53..56` | CRS (3 letters; blank = none) | | `49..52` | CRS (3 letters, always populated in a real record) |

- [ ] **Step 1: Write `parse_ti_lines`'s failing tests**

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiRecord {
    pub tiploc: String,
    pub station_name: String,
    pub stanox: Option<String>,
    pub crs: Option<String>,
}

/// Extracts every `TI` line from `text` (already filtered to `TI`-prefixed
/// lines by the caller's I/O layer -- see Task 4's `read_prefixed_lines`)
/// into a [`TiRecord`]. A line shorter than the fixed 80-byte real record
/// shape is skipped, not a hard error -- a single malformed line must not
/// abort the whole extraction (see the spec's Error handling section).
pub fn parse_ti_lines(text: &str) -> Vec<TiRecord> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TI_EUSTON: &str = "TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           ";
    const TI_WATRLMN: &str = "TIWATRLMN16559801RLONDON WATERLOO           87212   0                           ";

    #[test]
    fn extracts_stanox_tiploc_crs_and_name_from_a_real_ti_line() {
        let records = parse_ti_lines(TI_EUSTON);
        assert_eq!(
            records,
            vec![TiRecord {
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
                stanox: Some("72410".to_string()),
                crs: Some("EUS".to_string()),
            }]
        );
    }

    #[test]
    fn a_blank_crs_field_parses_as_none_not_an_empty_string() {
        let records = parse_ti_lines(TI_WATRLMN);
        assert_eq!(records[0].tiploc, "WATRLMN");
        assert_eq!(records[0].stanox, Some("87212".to_string()));
        assert_eq!(records[0].crs, None);
    }

    #[test]
    fn a_short_malformed_line_is_skipped_not_an_error() {
        assert_eq!(parse_ti_lines("TIshort"), Vec::new());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-reference parser::`
Expected: panic on `unimplemented!()`.

- [ ] **Step 3: Implement `parse_ti_lines`**

```rust
pub fn parse_ti_lines(text: &str) -> Vec<TiRecord> {
    text.lines()
        .filter_map(|line| {
            if line.len() < 56 {
                return None;
            }
            let tiploc = line[2..9].trim().to_string();
            let station_name = line[18..44].trim().to_string();
            let stanox_raw = line[44..49].trim();
            let stanox = if stanox_raw.is_empty() || stanox_raw == "00000" {
                None
            } else {
                Some(stanox_raw.to_string())
            };
            let crs_raw = line[53..56].trim();
            let crs = if crs_raw.is_empty() { None } else { Some(crs_raw.to_string()) };
            Some(TiRecord { tiploc, station_name, stanox, crs })
        })
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p schedule-reference parser::`
Expected: PASS.

- [ ] **Step 5: Write `parse_msn_a_lines`'s failing test**

The real MSN file's one header pseudo-record (`FILE-SPEC=05`) decodes to a `tiploc` field of `"PEC=05"` at these offsets — not alphanumeric, so filtering on "the decoded TIPLOC is non-empty and entirely alphanumeric" both extracts real records and excludes the header without a special case:

```rust
/// TIPLOC -> CRS, from every real `A` record in `text` (already filtered
/// to `A`-prefixed lines by the caller). The one `FILE-SPEC=...` header
/// pseudo-record present in a real MSN file decodes to a non-alphanumeric
/// "TIPLOC" (`"PEC=05"`) at these byte offsets and is excluded by the same
/// alphanumeric check that guards against any other malformed line -- no
/// special-cased header skip needed.
pub fn parse_msn_a_lines(text: &str) -> HashMap<String, String> {
    unimplemented!()
}

#[cfg(test)]
mod msn_tests {
    use super::*;

    const A_WATRLMN: &str = "A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815";
    const A_HEADER: &str = "A                             FILE-SPEC=05 1.00 28/08/26 18.08.01   944           ";

    #[test]
    fn extracts_tiploc_to_crs_from_a_real_a_record() {
        let map = parse_msn_a_lines(A_WATRLMN);
        assert_eq!(map.get("WATRLMN"), Some(&"WAT".to_string()));
    }

    #[test]
    fn the_file_spec_header_pseudo_record_is_excluded() {
        let map = parse_msn_a_lines(A_HEADER);
        assert!(map.is_empty(), "the header record must not be mistaken for a real TIPLOC");
    }
}
```

- [ ] **Step 6: Implement `parse_msn_a_lines`**

```rust
pub fn parse_msn_a_lines(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if line.len() < 52 {
            continue;
        }
        let tiploc = line[36..43].trim();
        if tiploc.is_empty() || !tiploc.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue; // catches the FILE-SPEC=05 header pseudo-record too
        }
        let crs = line[49..52].trim();
        if crs.is_empty() {
            continue;
        }
        map.insert(tiploc.to_string(), crs.to_string());
    }
    map
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p schedule-reference parser:: msn_tests::`
Expected: PASS.

- [ ] **Step 8: Write `resolve`'s failing tests — the disambiguation policy itself**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
}

/// Resolves the final STANOX->CRS table: completes a blank `TI` CRS from
/// `msn_crs_by_tiploc` (the WATRLMN case), groups by STANOX, and for any
/// STANOX with more than one distinct CRS applies the exact policy
/// `reference-data/stanox-crs.md:104-113` documents by hand for the
/// checked-in CSV -- prefer the sole non-`X`-prefixed candidate; otherwise
/// (2+ non-X, or 2+ X-prefixed, with no principled tiebreaker) exclude the
/// STANOX entirely. See this design's Decision 2.
pub fn resolve(ti: &[TiRecord], msn_crs_by_tiploc: &HashMap<String, String>) -> Vec<ParsedRow> {
    unimplemented!()
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    fn ti(tiploc: &str, name: &str, stanox: &str, crs: &str) -> TiRecord {
        TiRecord {
            tiploc: tiploc.to_string(),
            station_name: name.to_string(),
            stanox: if stanox.is_empty() { None } else { Some(stanox.to_string()) },
            crs: if crs.is_empty() { None } else { Some(crs.to_string()) },
        }
    }

    #[test]
    fn an_unambiguous_stanox_resolves_directly() {
        let rows = resolve(&[ti("EUSTON", "LONDON EUSTON", "72410", "EUS")], &HashMap::new());
        assert_eq!(
            rows,
            vec![ParsedRow {
                stanox: "72410".to_string(),
                crs: "EUS".to_string(),
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
            }]
        );
    }

    #[test]
    fn a_blank_ti_crs_is_completed_from_the_msn_a_record_before_grouping() {
        let ti_records = vec![ti("WATRLMN", "LONDON WATERLOO", "87212", "")];
        let msn = HashMap::from([("WATRLMN".to_string(), "WAT".to_string())]);
        let rows = resolve(&ti_records, &msn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].crs, "WAT");
    }

    #[test]
    fn ambiguous_stanox_with_one_non_x_candidate_resolves_to_it() {
        // The real 87201 case: VICTRIA/VIC (real passenger CRS) vs
        // VICTRCR/XVR (X-prefixed pseudo-code).
        let ti_records = vec![
            ti("VICTRIA", "LONDON VICTORIA", "87201", "VIC"),
            ti("VICTRCR", "VICTORIA CARRIAGE ROAD", "87201", "XVR"),
        ];
        let rows = resolve(&ti_records, &HashMap::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stanox, "87201");
        assert_eq!(rows[0].crs, "VIC", "the non-X-prefixed candidate wins");
    }

    #[test]
    fn ambiguous_stanox_with_two_non_x_candidates_is_excluded_entirely() {
        // The real, genuinely irresolvable 89428 case: ASI and AFK are both
        // real, non-X-prefixed CRS codes -- no principled tiebreaker.
        let ti_records = vec![
            ti("ASHFKI", "ASHFORD INT (PLATS 3-4)", "89428", "ASI"),
            ti("ASHFKY", "ASHFORD INTERNATIONAL", "89428", "AFK"),
        ];
        let rows = resolve(&ti_records, &HashMap::new());
        assert!(rows.is_empty(), "89428 must be excluded, not guessed at");
    }

    #[test]
    fn all_14_real_ambiguous_stanox_values_resolve_exactly_as_the_checked_in_csv_does() {
        // The full real 2026-08-28 ambiguity set (Current relevant state,
        // this plan's spec) -- 9 resolved via the non-X-preference rule, 5
        // excluded. Regression guard: if a future CIF extract's ambiguity
        // set differs, this test's own failure is the signal to update it
        // (Open question 4 in the spec).
        let ti_records = vec![
            ti("A1", "n", "30120", "PRE"), ti("A2", "n", "30120", "XPU"),
            ti("B1", "n", "31510", "MCV"), ti("B2", "n", "31510", "XVS"),
            ti("C1", "n", "40320", "CTR"), ti("C2", "n", "40320", "XCZ"),
            ti("D1", "n", "52215", "SDI"), ti("D2", "n", "52215", "SFA"),
            ti("E1", "n", "86441", "BOG"), ti("E2", "n", "86441", "XBN"),
            ti("F1", "n", "86935", "PFT"), ti("F2", "n", "86935", "POO"),
            ti("G1", "n", "86981", "WEY"), ti("G2", "n", "86981", "XWJ"),
            ti("H1", "n", "87201", "VIC"), ti("H2", "n", "87201", "XVR"),
            ti("I1", "n", "87219", "CLJ"), ti("I2", "n", "87219", "XCP"),
            ti("J1", "n", "87261", "WIM"), ti("J2", "n", "87261", "XWD"),
            ti("K1", "n", "87981", "XBP"), ti("K2", "n", "87981", "XMP"),
            ti("L1", "n", "88486", "SAY"), ti("L2", "n", "88486", "XSQ"),
            ti("M1", "n", "89428", "AFK"), ti("M2", "n", "89428", "ASI"),
            ti("N1", "n", "89530", "EBD"), ti("N2", "n", "89530", "EBF"),
        ];
        let rows = resolve(&ti_records, &HashMap::new());
        let resolved: HashMap<&str, &str> = rows.iter().map(|r| (r.stanox.as_str(), r.crs.as_str())).collect();

        for (stanox, expected_crs) in [
            ("30120", "PRE"), ("31510", "MCV"), ("40320", "CTR"), ("86441", "BOG"),
            ("86981", "WEY"), ("87201", "VIC"), ("87219", "CLJ"), ("87261", "WIM"), ("88486", "SAY"),
        ] {
            assert_eq!(resolved.get(stanox), Some(&expected_crs), "stanox {stanox} should resolve to {expected_crs}");
        }
        for stanox in ["52215", "86935", "87981", "89428", "89530"] {
            assert!(!resolved.contains_key(stanox), "stanox {stanox} should be excluded, not resolved");
        }
        assert_eq!(rows.len(), 9);
    }
}
```

- [ ] **Step 9: Run the tests to verify they fail**

Run: `cargo test -p schedule-reference parser::resolve_tests::`
Expected: panic on `unimplemented!()`.

- [ ] **Step 10: Implement `resolve`**

```rust
pub fn resolve(ti: &[TiRecord], msn_crs_by_tiploc: &HashMap<String, String>) -> Vec<ParsedRow> {
    let mut by_stanox: HashMap<String, Vec<(&TiRecord, String)>> = HashMap::new();

    for record in ti {
        let Some(stanox) = &record.stanox else { continue };
        let crs = record.crs.clone().or_else(|| msn_crs_by_tiploc.get(&record.tiploc).cloned());
        let Some(crs) = crs else { continue };
        by_stanox.entry(stanox.clone()).or_default().push((record, crs));
    }

    let mut rows = Vec::new();
    for (stanox, candidates) in by_stanox {
        let mut distinct: Vec<&str> = candidates.iter().map(|(_, crs)| crs.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();

        let winner = if distinct.len() == 1 {
            Some(distinct[0])
        } else {
            let non_x: Vec<&str> = distinct.iter().copied().filter(|c| !c.starts_with('X')).collect();
            if non_x.len() == 1 { Some(non_x[0]) } else { None }
        };

        if let Some(winner) = winner {
            let (record, crs) = candidates.iter().find(|(_, crs)| crs == winner).expect("winner came from distinct");
            rows.push(ParsedRow {
                stanox,
                crs: crs.clone(),
                tiploc: record.tiploc.clone(),
                station_name: record.station_name.clone(),
            });
        }
        // Otherwise: 2+ non-X candidates, or 2+ X-prefixed with none
        // non-X -- irresolvable, excluded entirely (see this design's
        // Error handling: "treat as irresolvable... never guess").
    }

    rows.sort_by(|a, b| a.stanox.cmp(&b.stanox));
    rows
}
```

- [ ] **Step 11: Run every test in the module**

Run: `cargo test -p schedule-reference parser::`
Expected: PASS (all `parser::`, `msn_tests::`, `resolve_tests::` tests, including the 14-STANOX regression test).

- [ ] **Step 12: Wire the module into `main.rs`**

Add `mod parser;` to `crates/schedule-reference/src/main.rs` (alongside the existing `mod config;`/`mod sequence;`).

- [ ] **Step 13: Commit**

```bash
git add crates/schedule-reference/src/parser.rs crates/schedule-reference/src/main.rs
git commit -m "Add pure MSN/MCA TI+A parsing and the STANOX disambiguation policy to schedule-reference"
```

---

### Task 3: `api`: `/private/stanox-crs` endpoint + migration + `common::StanoxCrsRecord`

**Files:**
- Modify: `crates/common/src/lib.rs` (add `StanoxCrsRecord` near `StationReference`, currently lines 650-658)
- Create: `crates/api/migrations/20260901150000_stanox_crs.sql`
- Modify: `crates/api/src/routes/ingest.rs` (add the route + handlers)
- Modify: `crates/api/src/data/queries.rs` (add `upsert_stanox_crs`/`list_stanox_crs`)

**Interfaces:**
- Produces: `common::StanoxCrsRecord { stanox: String, crs: String, tiploc: String, station_name: String, source_sequence: i32 }` (shared wire type, matching `StationReference`'s existing shared-type pattern rather than `schedule-ingest`'s bespoke single-object request — this is a `Vec<T>`-batch endpoint like every other poller). `POST /private/stanox-crs` (batch upsert by `stanox`). `GET /private/stanox-crs` (returns the full current table as `Vec<common::StanoxCrsRecord>`, mirroring `GET /private/tracked-trains`'s direct-`Vec<T>` shape at `ingest.rs:176-183`, not the `LastFetchedResponse` shape most other routes use — `trust-consumer` needs the actual rows, not a freshness timestamp).
- Consumed by: Task 4 (`schedule-reference`'s `poll_once` POSTs to this route via `common::ingest::post_batch`). Task 5 (`trust-consumer`'s new reload GETs this route).
- **Depends on:** nothing (parallelizable with Tasks 1-2).

- [ ] **Step 1: Add `common::StanoxCrsRecord`**

In `crates/common/src/lib.rs`, immediately after `StationReference` (currently lines 650-658):

```rust
/// One resolved STANOX->CRS row, as `crates/schedule-reference` derives it
/// from a CIF SCHEDULE delivery's `TI`/`A` records and POSTs it to
/// `api`'s `/private/stanox-crs`, and as `trust-consumer` GETs the full
/// current table back. See
/// docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md
/// Decision 2 for the schema and the disambiguation policy that produced
/// each row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StanoxCrsRecord {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
    /// Which `schedule_feed_ingests.sequence` this row was last derived
    /// from -- provenance a live source benefits from that the static CSV
    /// never needed.
    pub source_sequence: i32,
}
```

- [ ] **Step 2: Write the migration**

`crates/api/migrations/20260901150000_stanox_crs.sql`:

```sql
-- -------------------------------------------------------------------------
-- Live STANOX->CRS reference table, replacing (for trust-consumer's
-- purposes -- the CSV stays as a fallback, see this plan's Global
-- Constraints) the static reference-data/stanox-crs.csv. Populated by
-- crates/schedule-reference from the CIF SCHEDULE feed's own TI/A records.
-- Replace-on-write: every daily delivery is a full refresh, so every
-- successful POST /private/stanox-crs upserts the complete current table
-- by `stanox` -- see
-- docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md
-- Decision 2.
-- -------------------------------------------------------------------------

CREATE TABLE stanox_crs (
    stanox TEXT PRIMARY KEY,
    crs TEXT NOT NULL,
    tiploc TEXT NOT NULL,
    station_name TEXT NOT NULL,
    source_sequence INTEGER NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 3: Add `upsert_stanox_crs`/`list_stanox_crs` to `queries.rs`**

Mirroring `upsert_stations` (currently `queries.rs:221-253`, `ON CONFLICT (crs) DO UPDATE`) and `last_stations_fetch`'s tuple-`query_as` shape:

```rust
/// Upserts a batch of resolved STANOX/CRS rows. Every daily delivery is a
/// full refresh (see this table's migration comment), so this is always a
/// complete-table upsert-by-`stanox`, never a delta -- no separate
/// "delete rows missing from today's delivery" step is needed, since every
/// successful run re-asserts every row it still resolves.
pub async fn upsert_stanox_crs(pool: &PgPool, records: &[common::StanoxCrsRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for record in records {
        sqlx::query(
            r#"
            INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, updated_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            ON CONFLICT (stanox) DO UPDATE SET
                crs             = EXCLUDED.crs,
                tiploc          = EXCLUDED.tiploc,
                station_name    = EXCLUDED.station_name,
                source_sequence = EXCLUDED.source_sequence,
                updated_at      = NOW()
            "#,
        )
        .bind(&record.stanox)
        .bind(&record.crs)
        .bind(&record.tiploc)
        .bind(&record.station_name)
        .bind(record.source_sequence)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Row shape for `list_stanox_crs`'s `SELECT` -- a dedicated `FromRow`
/// struct, matching this file's own established convention for any
/// multi-column query result (see `IncidentRow`, `queries.rs:726-...`;
/// `train_tracking.rs`'s `TrackedTrainRow`/`TrackedTrainListItem`), rather
/// than a bare tuple -- this repo reserves raw tuple `query_as` for
/// single-column results only (e.g. `last_stations_fetch`'s `(Option<DateTime<Utc>>,)`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct StanoxCrsRow {
    stanox: String,
    crs: String,
    tiploc: String,
    station_name: String,
    source_sequence: i32,
}

impl From<StanoxCrsRow> for common::StanoxCrsRecord {
    fn from(row: StanoxCrsRow) -> Self {
        common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence: row.source_sequence,
        }
    }
}

/// The full current STANOX/CRS table, ordered by `stanox` for a stable,
/// reviewable response shape -- backs `GET /private/stanox-crs`, which
/// `trust-consumer`'s periodic reload consumes directly (Task 5), unlike
/// every `last_*_fetch` query in this file, which only returns a
/// timestamp.
pub async fn list_stanox_crs(pool: &PgPool) -> Result<Vec<common::StanoxCrsRecord>> {
    let rows = sqlx::query_as::<_, StanoxCrsRow>(
        "SELECT stanox, crs, tiploc, station_name, source_sequence FROM stanox_crs ORDER BY stanox",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(common::StanoxCrsRecord::from).collect())
}
```

- [ ] **Step 4: Add the DB-touching regression test**

Following the exact live-database convention at `queries.rs:1024-1059`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                a_re_post_with_a_changed_crs_overwrites_the_existing_row -- --ignored`"]
    async fn a_re_post_with_a_changed_crs_overwrites_the_existing_row() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        let first = common::StanoxCrsRecord {
            stanox: "99999".to_string(),
            crs: "TST".to_string(),
            tiploc: "TESTLOC".to_string(),
            station_name: "TEST STATION".to_string(),
            source_sequence: 942,
        };
        upsert_stanox_crs(&pool, &[first]).await.expect("first upsert");

        let second = common::StanoxCrsRecord {
            stanox: "99999".to_string(),
            crs: "TS2".to_string(),
            tiploc: "TESTLOC".to_string(),
            station_name: "TEST STATION".to_string(),
            source_sequence: 943,
        };
        upsert_stanox_crs(&pool, &[second]).await.expect("re-upsert with changed crs");

        let rows = list_stanox_crs(&pool).await.expect("list_stanox_crs");
        let row = rows.iter().find(|r| r.stanox == "99999").expect("row present");
        assert_eq!(row.crs, "TS2", "the re-POST must overwrite, not duplicate");
        assert_eq!(row.source_sequence, 943);

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = '99999'")
            .execute(&pool)
            .await
            .expect("cleanup fixture row");
    }
```

- [ ] **Step 5: Add the route + handlers to `ingest.rs`**

In `router()` (currently `ingest.rs:28-53`), add after the `schedule-feed-ingests` route:

```rust
        .route(
            "/stanox-crs",
            axum::routing::get(get_stanox_crs).post(post_stanox_crs),
        )
```

Add the handlers, mirroring `get_active_tracked_trains`/`post_train_events` (currently `ingest.rs:160-183`):

```rust
/// `crates/schedule-reference`'s per-sequence batch of resolved
/// STANOX/CRS rows -- see `queries::upsert_stanox_crs`.
async fn post_stanox_crs(
    State(app): State<App>,
    Json(records): Json<Vec<common::StanoxCrsRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_stanox_crs(&app.database, &records)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}

/// `trust-consumer`'s periodic live-table reload -- returns the full
/// current table, not a freshness timestamp (see `queries::list_stanox_crs`'s
/// own doc comment for why this route differs from every `last_*_fetch`
/// GET elsewhere in this file).
async fn get_stanox_crs(State(app): State<App>) -> Result<Json<Vec<common::StanoxCrsRecord>>, (StatusCode, String)> {
    let rows = queries::list_stanox_crs(&app.database).await.map_err(internal_error)?;
    Ok(Json(rows))
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo build --workspace && cargo test -p common -p api`
Expected: PASS. The new DB-touching test is `#[ignore]`d by default; run it explicitly once a local Postgres is available: `DATABASE_URL=postgres://... cargo test -p api a_re_post_with_a_changed_crs_overwrites_the_existing_row -- --ignored`.

- [ ] **Step 7: Commit**

```bash
git add crates/common/src/lib.rs crates/api/migrations/20260901150000_stanox_crs.sql crates/api/src/routes/ingest.rs crates/api/src/data/queries.rs
git commit -m "Add /private/stanox-crs (GET+POST), the stanox_crs table, and common::StanoxCrsRecord"
```

---

### Task 4: Wire `schedule-reference`'s main loop (assemble Tasks 1-3)

**Files:**
- Modify: `crates/schedule-reference/src/main.rs`

**Interfaces:**
- Produces: `poll_once`'s real body (parses the highest new complete sequence, POSTs it, and only advances `last_processed_sequence` on a successful POST).
- Consumes: `sequence::highest_complete_sequence` (Task 1), `parser::parse_ti_lines`/`parse_msn_a_lines`/`resolve` (Task 2), `common::StanoxCrsRecord` + `POST /private/stanox-crs` (Task 3), `common::ingest::post_batch` (existing, `crates/common/src/ingest.rs:35-57`).
- **Depends on:** Tasks 1, 2, 3.

- [ ] **Step 1: Add the streamed, prefix-filtered file reader**

The real `MCA` file is 707MB — never read whole into memory. `TI`/`A` are a bounded ~15,387-line subset (0.18% of `MCA`), so filtering while streaming line-by-line keeps memory use to that subset only, per the spec's own framing of this as "a streamed, prefix-filtered read".

```rust
/// Streams `path` line-by-line, keeping only lines starting with `prefix`
/// -- so the real 707MB `RJTTF<n>MCA.txt` is never held in memory whole,
/// only its ~12,085 `TI` lines (the `RJTTF<n>MSN.txt` file, at ~340KB
/// total, is small enough that this matters far less for it, but the same
/// function is reused for both for one consistent code path).
fn read_prefixed_lines(path: &std::path::Path, prefix: &str) -> anyhow::Result<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = String::new();
    for line in reader.lines() {
        let line = line?;
        if line.starts_with(prefix) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out)
}
```

- [ ] **Step 2: Replace `poll_once`'s stub body**

```rust
async fn poll_once(
    client: &Client,
    config: &Config,
    last_processed_sequence: &mut Option<u32>,
) -> anyhow::Result<()> {
    let Some(sequence) = sequence::highest_complete_sequence(&config.storage_dir)? else {
        tracing::debug!("no complete MCA+MSN sequence directory found yet");
        return Ok(());
    };
    if Some(sequence) == *last_processed_sequence {
        tracing::debug!(sequence, "no new sequence since last successful parse; nothing to do");
        return Ok(());
    }

    let sequence_dir = config.storage_dir.join(sequence.to_string());
    let mca_path = sequence_dir.join(format!("RJTTF{sequence}MCA.txt"));
    let msn_path = sequence_dir.join(format!("RJTTF{sequence}MSN.txt"));

    let ti_text = read_prefixed_lines(&mca_path, "TI")?;
    let a_text = read_prefixed_lines(&msn_path, "A")?;

    let ti_records = parser::parse_ti_lines(&ti_text);
    let msn_crs = parser::parse_msn_a_lines(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs);

    tracing::info!(sequence, ti_records = ti_records.len(), resolved = rows.len(), "parsed stanox/crs table from sequence");

    let records: Vec<common::StanoxCrsRecord> = rows
        .into_iter()
        .map(|row| common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence: sequence as i32,
        })
        .collect();

    common::ingest::post_batch(client, &config.api_ingest_url, &config.internal_token, &records, "stanox/crs rows").await?;

    // Only advance on a successful POST -- a failed POST just means the
    // already-computed table is discarded and rebuilt from the same
    // still-local, unchanged files next cycle (cheap), matching the
    // spec's Error handling: "a failed POST just means the already-
    // computed in-memory table is discarded and rebuilt... next cycle".
    *last_processed_sequence = Some(sequence);
    Ok(())
}
```

- [ ] **Step 3: Write an integration-style test for `poll_once` against a temp directory**

This exercises the full read -> parse -> resolve path (not the POST itself, which needs a live `api` — matching `schedule-ingest`'s own `post_ingest` being untested at the unit level for the same reason) by asserting on `read_prefixed_lines` + `parser` composed together:

```rust
#[cfg(test)]
mod poll_once_tests {
    use super::*;

    #[test]
    fn read_prefixed_lines_extracts_only_matching_lines_from_a_mixed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.txt");
        std::fs::write(
            &path,
            "HDsomething\nTIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           \nBSsomeschedule\n",
        )
        .unwrap();

        let ti_text = read_prefixed_lines(&path, "TI").unwrap();
        assert_eq!(ti_text.lines().count(), 1);
        assert!(ti_text.starts_with("TIEUSTON"));

        let records = parser::parse_ti_lines(&ti_text);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tiploc, "EUSTON");
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p schedule-reference`
Expected: PASS (every test from Tasks 1, 2, and this task).

- [ ] **Step 5: Manual smoke test against the real `timetable_full.zip` extract**

Not a `cargo test` — a one-time local sanity check that the parser produces the expected `3124`-row count end-to-end against the real file, mirroring `trust-consumer`'s own `the_real_data_file_has_3124_entries` regression test's expectation:

```bash
mkdir -p /tmp/schedule-reference-smoke/942
cd /workspaces/github-com-fasterspeeding-network-rail-status
unzip -p timetable_full.zip RJTTF942MCA.txt > /tmp/schedule-reference-smoke/942/RJTTF942MCA.txt
unzip -p timetable_full.zip RJTTF942MSN.txt > /tmp/schedule-reference-smoke/942/RJTTF942MSN.txt
# Then run a throwaway `cargo run -p schedule-reference` locally with
# STORAGE_DIR=/tmp/schedule-reference-smoke pointed at it, and confirm the
# logged `resolved` count is 3124 -- matching the real CSV's own row count
# (trust-consumer/src/stanox_crs.rs's the_real_data_file_has_3124_entries).
rm -rf /tmp/schedule-reference-smoke
```

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-reference/src/main.rs
git commit -m "Wire schedule-reference's main loop: scan, parse, resolve, and POST /private/stanox-crs"
```

---

### Task 5: `trust-consumer`'s periodic reload wiring

**Files:**
- Modify: `crates/trust-consumer/src/config.rs` (add `stanox_crs_reload_secs`, `stanox_crs_url`)
- Modify: `crates/trust-consumer/src/stanox_crs.rs` (add `StanoxCrsTable::from_records`; update module doc)
- Modify: `crates/trust-consumer/src/queries.rs` (add `fetch_stanox_crs`)
- Modify: `crates/trust-consumer/src/process.rs` (add `apply_stanox_crs_reload`; update module doc's "Known simplification" section)
- Modify: `crates/trust-consumer/src/main.rs` (move `config.stanox_crs` into a shared cell; add the second reload block; `run_cycle` reads a per-cycle snapshot)

**Interfaces:**
- Produces: `StanoxCrsTable::from_records(records: Vec<common::StanoxCrsRecord>) -> Self`. `process::apply_stanox_crs_reload(fetched: anyhow::Result<Vec<common::StanoxCrsRecord>>, cell: &std::sync::RwLock<stanox_crs::StanoxCrsTable>)`. `queries::fetch_stanox_crs(client: &Client, url: &str, internal_token: &str) -> anyhow::Result<Vec<common::StanoxCrsRecord>>`.
- Consumes: `common::StanoxCrsRecord` (Task 3). `GET /private/stanox-crs` (Task 3).
- **Depends on:** Task 3 (wire shape). Independent of Tasks 1/2/4 — `trust-consumer` never talks to `schedule-reference` directly, only to `api`.

**Swap mechanism decision (resolves the spec's Open Question 2):** a bare `std::sync::RwLock<StanoxCrsTable>`, **not** wrapped in `Arc` and **not** `ArcSwap`. No `Arc` is needed because the cell is never moved into a spawned task or shared across threads — it's a single local variable in `main`'s own loop, read (via `run_cycle`'s `&std::sync::RwLock<...>` parameter) and written (via `apply_stanox_crs_reload`) from that same one async task, exactly like `reference`/`state` already are. Not `ArcSwap` either: `arc-swap` is present in `Cargo.lock` only as a transitive dependency (confirmed: no crate in this workspace depends on it directly), and every read here (`stanox_to_crs`, a synchronous `HashMap` lookup) is a brief, non-`await`-holding critical section at a message rate this reload cadence (hourly-ish) makes trivially low-contention — a plain `std::sync::RwLock` needs no new dependency and matches this crate's existing "no `sqlx`/no exotic concurrency primitive" minimalism (spec's Current relevant state: "This crate remains entirely DB-free today"). No `Cargo.toml` change needed for this task.

- [ ] **Step 1: Add `StanoxCrsTable::from_records`**

In `crates/trust-consumer/src/stanox_crs.rs`, after `parse` (currently ending at line 132):

```rust
    /// Builds a table directly from `api`'s `GET /private/stanox-crs`
    /// response rows -- the live-reload counterpart to `from_file`/`parse`.
    /// `tiploc`/`station_name`/`source_sequence` are not needed for
    /// lookup and are dropped here; only `stanox`/`crs` matter to
    /// `stanox_to_crs`.
    pub fn from_records(records: Vec<common::StanoxCrsRecord>) -> Self {
        let by_stanox = records.into_iter().map(|r| (r.stanox, r.crs)).collect();
        Self { by_stanox }
    }
```

Update the module's doc comment (lines 1-35) to describe the new two-tier behavior — see Task 6, which owns this doc update explicitly.

- [ ] **Step 2: Add the failing test**

```rust
    #[test]
    fn from_records_builds_a_table_usable_by_stanox_to_crs() {
        let records = vec![
            common::StanoxCrsRecord {
                stanox: "72410".to_string(),
                crs: "EUS".to_string(),
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
                source_sequence: 942,
            },
        ];
        let table = StanoxCrsTable::from_records(records);
        assert_eq!(table.stanox_to_crs("72410"), Some("EUS".to_string()));
        assert_eq!(table.stanox_to_crs("99999"), None);
    }
```

- [ ] **Step 3: Run the test, confirm PASS**

Run: `cargo test -p trust-consumer from_records_builds_a_table`
Expected: PASS (the implementation in Step 1 is already complete — this is a same-commit verification, not a strict red/green split, since `from_records` is a two-line function with no meaningful failing-first state).

- [ ] **Step 4: Add `fetch_stanox_crs` to `queries.rs`**

Mirroring `fetch_active_tracked_trains` (currently `queries.rs:13-25`):

```rust
pub async fn fetch_stanox_crs(
    client: &Client,
    url: &str,
    internal_token: &str,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    let response = client
        .get(url)
        .header(INTERNAL_TOKEN_HEADER, internal_token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}
```

- [ ] **Step 5: Add `Config::stanox_crs_reload_secs`/`stanox_crs_url`**

In `crates/trust-consumer/src/config.rs`, after the existing `stanox_crs` field (currently ending at line 108):

```rust
    /// How often to reload the live STANOX->CRS table from `api`. Deliberately
    /// coarser than `reference_reload_secs`'s 60s default -- the underlying
    /// data changes roughly daily (Decision 4), so "promptly" only matters
    /// relative to that, not to a human creating a pin. UNRESEARCHED
    /// starting figure, same posture as `MINE_LIST_LIMIT`/`MAX_PIN_AGE`
    /// elsewhere in this codebase (see the spec's Open questions #1).
    #[arg(long, env, default_value_t = 3600)]
    pub stanox_crs_reload_secs: u64,

    /// The `api` crate's endpoint for the live STANOX/CRS table.
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub stanox_crs_url: String,
```

- [ ] **Step 6: Add `apply_stanox_crs_reload` to `process.rs`**

Placed alongside `apply_reference_reload` (currently ending at line 233), matching its shape and its "pure with respect to the decision, not the fetch" split:

```rust
/// Applies one `stanox_crs` reload tick's HTTP result to the shared cell.
/// Pure with respect to the swap-vs-keep *decision* -- given directly what
/// the fetch produced, not performing the fetch itself -- so the fail-open
/// policy below is unit-testable without a live `api`, mirroring
/// `apply_reference_reload`'s own split from `queries::fetch_active_tracked_trains`.
///
/// Fails open in both failure shapes: an `Err` (network/HTTP failure) or an
/// empty `Ok` (fresh environment, or `schedule-reference` has never
/// successfully run) both leave the currently-loaded table (CSV-derived at
/// startup, or a previously-fetched live one) untouched, never swapping in
/// an empty table that would silently stop translating every STANOX. See
/// the spec's Error handling section.
pub fn apply_stanox_crs_reload(
    fetched: anyhow::Result<Vec<common::StanoxCrsRecord>>,
    cell: &std::sync::RwLock<crate::stanox_crs::StanoxCrsTable>,
) {
    match fetched {
        Ok(records) if !records.is_empty() => {
            let count = records.len();
            let table = crate::stanox_crs::StanoxCrsTable::from_records(records);
            *cell.write().expect("stanox_crs lock poisoned") = table;
            tracing::info!(count, "reloaded live stanox/crs table");
        }
        Ok(_) => {
            tracing::warn!("live stanox_crs table is empty; keeping the currently loaded table");
        }
        Err(err) => {
            tracing::error!(error = ?err, "failed to reload stanox_crs table; keeping the currently loaded table");
        }
    }
}
```

- [ ] **Step 7: Add the two regression tests the spec's Testing section calls for**

Alongside `process.rs`'s existing test module (after `apply_reference_reload`'s own test coverage):

```rust
    #[test]
    fn a_successful_reload_replaces_the_table_for_subsequent_lookups() {
        let initial = StanoxCrsTable::from_records(vec![common::StanoxCrsRecord {
            stanox: "72410".to_string(), crs: "EUS".to_string(),
            tiploc: "EUSTON".to_string(), station_name: "LONDON EUSTON".to_string(), source_sequence: 940,
        }]);
        let cell = std::sync::RwLock::new(initial);

        let fresh = vec![common::StanoxCrsRecord {
            stanox: "72410".to_string(), crs: "EU2".to_string(),
            tiploc: "EUSTON".to_string(), station_name: "LONDON EUSTON".to_string(), source_sequence: 942,
        }];
        apply_stanox_crs_reload(Ok(fresh), &cell);

        assert_eq!(cell.read().unwrap().stanox_to_crs("72410"), Some("EU2".to_string()));
    }

    #[test]
    fn a_failed_or_empty_reload_does_not_clear_the_currently_loaded_table() {
        let initial = StanoxCrsTable::from_records(vec![common::StanoxCrsRecord {
            stanox: "72410".to_string(), crs: "EUS".to_string(),
            tiploc: "EUSTON".to_string(), station_name: "LONDON EUSTON".to_string(), source_sequence: 940,
        }]);
        let cell = std::sync::RwLock::new(initial);

        apply_stanox_crs_reload(Err(anyhow::anyhow!("api is down")), &cell);
        assert_eq!(cell.read().unwrap().stanox_to_crs("72410"), Some("EUS".to_string()), "a failed fetch must not clear the table");

        apply_stanox_crs_reload(Ok(Vec::new()), &cell);
        assert_eq!(cell.read().unwrap().stanox_to_crs("72410"), Some("EUS".to_string()), "an empty live table must not clear the table either");
    }
```

(Add `use crate::stanox_crs::StanoxCrsTable;` to this test module's imports if not already present.)

- [ ] **Step 8: Wire the cell and the second reload block into `main.rs`**

Replace the direct `config.stanox_crs` usage. Current shape (lines 34-98): `run_cycle` takes `&config.stanox_crs: &stanox_crs::StanoxCrsTable` directly from the immutable `Config`. New shape:

```rust
    let config = Config::parse();
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();

    let mut feed = KafkaMovementFeed::connect(&config, connection_state)?;

    let mut reference = process::Reference { pending: Vec::new() };
    let reload_interval = Duration::from_secs(config.reference_reload_secs);
    let mut last_reference_reload = tokio::time::Instant::now() - reload_interval;

    // The CSV-derived table `config.stanox_crs` already loaded at parse
    // time becomes the shared cell's initial value -- the startup value
    // and the fail-open fallback stay exactly as they were (Decision 3);
    // only the read path (a per-cycle snapshot instead of a bare
    // reference) and the addition of this reload block are new.
    let stanox_crs = std::sync::RwLock::new(config.stanox_crs.clone());
    let stanox_crs_reload_interval = Duration::from_secs(config.stanox_crs_reload_secs);
    let mut last_stanox_crs_reload = tokio::time::Instant::now() - stanox_crs_reload_interval;

    let mut state = process::ProcessorState::default();

    loop {
        if last_reference_reload.elapsed() >= reload_interval {
            // ...unchanged, see main.rs:52-78...
        }

        if last_stanox_crs_reload.elapsed() >= stanox_crs_reload_interval {
            let fetched = queries::fetch_stanox_crs(&http, &config.stanox_crs_url, &config.internal_token).await;
            process::apply_stanox_crs_reload(fetched, &stanox_crs);
            last_stanox_crs_reload = tokio::time::Instant::now();
        }

        let outcome = run_cycle(&mut feed, &reference, &mut state, &stanox_crs, async |events| {
            queries::post_train_events(&http, &config.api_ingest_url, &config.internal_token, events).await
        })
        .await;

        if outcome == Cycle::Failed {
            tokio::time::sleep(ERROR_BACKOFF).await;
        }
    }
```

`run_cycle`'s signature (currently `main.rs:126-156`) changes to take the cell and read a snapshot once per cycle — matching the spec's own suggested shape ("a cheap cloned snapshot read once per cycle, matching how `reference`/`state` are already handled"):

```rust
async fn run_cycle<F, P>(
    feed: &mut F,
    reference: &process::Reference,
    state: &mut process::ProcessorState,
    stanox_crs: &std::sync::RwLock<stanox_crs::StanoxCrsTable>,
    post: P,
) -> Cycle
where
    F: MovementFeed,
    P: AsyncFnOnce(&[common::TrainMovementEventMessage]) -> anyhow::Result<()>,
{
    let snapshot = stanox_crs.read().expect("stanox_crs lock poisoned").clone();

    let events = match process::run_once(feed, reference, state, &snapshot).await {
        Ok(events) => events,
        Err(err) => {
            tracing::error!(error = ?err, "error processing movement feed batch");
            return Cycle::Failed;
        }
    };

    if let Err(err) = post(&events).await {
        tracing::error!(error = ?err, "failed to post train events; not committing this batch's offsets");
        return Cycle::Failed;
    }

    if let Err(err) = feed.commit().await {
        tracing::error!(error = ?err, "failed to commit Kafka offsets");
        return Cycle::Failed;
    }

    Cycle::Committed
}
```

`process::run_once`/`process_message` (`process.rs:251-282`, `284-474`) need **no signature change** — they already take `&crate::stanox_crs::StanoxCrsTable`, and `run_cycle` now simply passes `&snapshot` (a plain, momentarily-cloned table) instead of `&config.stanox_crs`.

- [ ] **Step 9: Fix `main.rs`'s own existing tests**

Every existing test in `main.rs`'s test module (`a_failed_post_does_not_commit_the_batch`, etc., currently lines 199-278) calls `run_cycle(&mut feed, &reference, &mut state, &TEST_STANOX_CRS, ...)` where `TEST_STANOX_CRS` is a `LazyLock<stanox_crs::StanoxCrsTable>`. Wrap it: change `TEST_STANOX_CRS`'s type from `LazyLock<StanoxCrsTable>` to `LazyLock<std::sync::RwLock<StanoxCrsTable>>`:

```rust
    static TEST_STANOX_CRS: LazyLock<std::sync::RwLock<stanox_crs::StanoxCrsTable>> = LazyLock::new(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference-data/stanox-crs.csv");
        std::sync::RwLock::new(stanox_crs::StanoxCrsTable::from_file(&path).expect("reference-data/stanox-crs.csv should parse"))
    });
```

No other line in any of those tests changes — every call site already passes `&TEST_STANOX_CRS`, which still type-checks against `run_cycle`'s new `&std::sync::RwLock<StanoxCrsTable>` parameter.

- [ ] **Step 10: Run the tests**

Run: `cargo test -p trust-consumer`
Expected: PASS (every existing test, plus the two new `apply_stanox_crs_reload` tests and `from_records_builds_a_table_usable_by_stanox_to_crs`).

- [ ] **Step 11: Commit**

```bash
git add crates/trust-consumer/src/config.rs crates/trust-consumer/src/stanox_crs.rs crates/trust-consumer/src/queries.rs crates/trust-consumer/src/process.rs crates/trust-consumer/src/main.rs
git commit -m "trust-consumer: add a periodic live stanox_crs reload alongside the existing reference reload"
```

---

### Task 6: CSV static-file transition — doc updates only, no functional CSV change

**Files:**
- Modify: `crates/trust-consumer/src/stanox_crs.rs` (module doc, lines 1-35)
- Modify: `reference-data/stanox-crs.md` (append a short section)

**Interfaces:** none (documentation only).
**Depends on:** Task 5 (describes what that task actually built).

**Decision being documented (Decision 3, spec):** `reference-data/stanox-crs.csv` and its loader are **kept, not deleted, indefinitely** — "additive, not a removal of the existing safety net." After Task 5 lands, the CSV's role changes from "the only source, ever" to "the startup value, and the permanent fail-open fallback whenever the live source is empty, unreachable, or a fresh environment has no `schedule-reference` running at all (e.g. local dev without `scheduleFeed.enabled`)." This task exists so that role change is documented where a future reader would look, not left to be inferred from the code alone.

- [ ] **Step 1: Update `stanox_crs.rs`'s module doc**

The current doc comment (lines 10-19) states flatly: "The table itself is `reference-data/stanox-crs.csv`, loaded at runtime via `StanoxCrsTable::from_file` -- not compiled in... read once at process startup." Replace that paragraph with:

```rust
//! # Where the data lives
//!
//! Two tiers, in order:
//!
//! 1. **Startup / fallback**: `reference-data/stanox-crs.csv`, loaded once
//!    at process startup via `StanoxCrsTable::from_file` -- the
//!    `--stanox-crs-file`/`STANOX_CRS_FILE` flag in `config.rs`, unchanged
//!    since before the live table existed. This remains the checked-in
//!    default for local dev and any environment without the schedule-feed
//!    pipeline deployed, and the value this crate falls back to (and keeps
//!    indefinitely) if the live table below is ever empty, unreachable, or
//!    a fresh environment has never had `crates/schedule-reference`
//!    successfully run.
//! 2. **Live reload**: once `crates/schedule-reference` (a sibling
//!    container to `schedule-ingest`, see
//!    docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md)
//!    has successfully parsed a CIF SCHEDULE delivery, this crate's main
//!    loop periodically (`--stanox-crs-reload-secs`) `GET`s
//!    `/private/stanox-crs` and swaps a fresh `StanoxCrsTable::from_records`
//!    into a shared `std::sync::RwLock` cell (see `main.rs`'s second
//!    reload block, alongside its existing tracked-trains reload). A
//!    failed or empty reload never clears the currently-loaded table --
//!    see `process::apply_stanox_crs_reload`.
//!
//! **Full provenance for the CSV specifically** -- exactly how it was
//! extracted, the record format's byte offsets, and the documented
//! exclusion policy for ambiguous STANOX values -- lives in
//! `reference-data/stanox-crs.md`. The live table applies the identical
//! exclusion policy, reimplemented as real code in
//! `crates/schedule-reference/src/parser.rs`.
```

- [ ] **Step 2: Append a short section to `reference-data/stanox-crs.md`**

After the existing "Regenerating this table" section (ending at line 137):

```markdown
## This file's role since the live table (2026-09-01)

As of
docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md,
`trust-consumer` no longer relies on this file exclusively: `crates/schedule-reference`
derives a live-refreshed equivalent from the same `TI`/`A` record types
documented above, straight from the CIF SCHEDULE feed `schedule-ingest`
already receives daily, and `trust-consumer` periodically reloads it (see
`crates/trust-consumer/src/stanox_crs.rs`'s module doc). This file is kept
indefinitely, not deprecated: it remains the startup value and the
permanent fail-open fallback (an environment with no `schedule-reference`
running, e.g. local dev, behaves exactly as it always has). Regenerating it
by hand (the recipe above) is still valid and still occasionally useful for
spot-checking the live table's output, but is no longer this crate's only
path to a working STANOX/CRS table.
```

- [ ] **Step 3: Commit**

```bash
git add crates/trust-consumer/src/stanox_crs.rs reference-data/stanox-crs.md
git commit -m "Document the CSV's new role as startup value + fallback, not the sole source"
```

---

### Task 7: Helm chart — new `reference` container in the `schedulefeed` Pod + `trust-consumer` env

**Files:**
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml` (add the third container, mount the PVC read-only)
- Modify: `charts/distant-signal/templates/trust-consumer-deployment.yaml` (add `STANOX_CRS_RELOAD_SECS`/`STANOX_CRS_URL`)
- Modify: `charts/distant-signal/values.yaml` (`scheduleFeed.reference.*`, `trustConsumer.stanoxCrsReloadSecs`)
- Create: `docker/schedule-reference.Dockerfile`

**Interfaces:** none (deployment only — no Rust code in this task).
**Depends on:** Task 4 (the `schedule-reference` binary/image must exist to reference), Task 5 (the new `trust-consumer` env var names must be settled).

- [ ] **Step 1: Write `docker/schedule-reference.Dockerfile`**

Mirrors `docker/schedule-ingest.Dockerfile` exactly, substituting the binary name (same `rust:1.88-bookworm` builder pin, same BuildKit cache-mount shape, same `debian:bookworm-slim` runtime stage, same non-root `poller` user):

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for the `schedule-reference` service. See
# docker/schedule-ingest.Dockerfile's own header comment for the full
# rationale behind the rust:1.88-bookworm pin and the cache-mount shape --
# unchanged here, this crate shares the same `common`-crate-driven
# reqwest -> url -> idna -> icu_* transitive dependency chain.
#
# Build from the repo root:
#   docker build -f docker/schedule-reference.Dockerfile .
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin schedule-reference; \
    else \
      cargo build --bin schedule-reference; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/schedule-reference /usr/local/bin/schedule-reference

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin poller

COPY --from=builder /usr/local/bin/schedule-reference /usr/local/bin/schedule-reference

USER poller

ENTRYPOINT ["/usr/local/bin/schedule-reference"]
```

- [ ] **Step 2: Add `scheduleFeed.reference.*` and `trustConsumer.stanoxCrsReloadSecs` to `values.yaml`**

In the `scheduleFeed:` block (currently ending at line 757), add a sibling to `ingest:` (currently lines 712-719):

```yaml
  reference:
    image:
      repository: distant-signal/schedule-reference
      tag: ""
      pullPolicy: IfNotPresent
    pollIntervalSecs: 1800
    # -- MUST differ from metrics.port: this container shares the
    # schedulefeed Pod's network namespace with `ingest`, which already
    # binds metrics.port for its own listener -- see this plan's Global
    # Constraints.
    metricsPort: 9092
```

In `trustConsumer:` (currently ending at line 625), add after `referenceReloadSecs: 60` (line 615):

```yaml
  stanoxCrsReloadSecs: 3600
```

- [ ] **Step 3: Add the `reference` container to `schedulefeed-deployment.yaml`**

After the `ingest` container block (currently ending at line 231, before the closing `containers:` list's `{{- with .Values.scheduleFeed.nodeSelector }}` at line 232):

```yaml
        - name: reference
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.scheduleFeed.reference.image) | quote }}
          imagePullPolicy: {{ .Values.scheduleFeed.reference.image.pullPolicy }}
          securityContext:
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          {{- if .Values.metrics.enabled }}
          ports:
            - name: ref-metrics
              containerPort: {{ .Values.scheduleFeed.reference.metricsPort }}
              protocol: TCP
          {{- end }}
          env:
            - name: STORAGE_DIR
              value: /data/schedule-feed
            - name: POLL_INTERVAL_SECS
              value: {{ .Values.scheduleFeed.reference.pollIntervalSecs | quote }}
            - name: API_INGEST_URL
              value: {{ printf "%s/private/stanox-crs" (include "distant-signal.apiBaseUrl" .) | quote }}
            - name: INTERNAL_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.internalTokenSecretName" . }}
                  key: {{ include "distant-signal.internalTokenSecretKey" . }}
            - name: METRICS_ENABLED
              value: {{ .Values.metrics.enabled | quote }}
            {{- if .Values.metrics.enabled }}
            - name: METRICS_PORT
              value: {{ .Values.scheduleFeed.reference.metricsPort | quote }}
            {{- end }}
            - name: RUST_LOG
              value: {{ .Values.scheduleFeed.logLevel | quote }}
          volumeMounts:
            - name: data
              mountPath: /data/schedule-feed
              readOnly: true
          {{- with .Values.scheduleFeed.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
```

Note: the pod-level `prometheus.io/port` annotation (currently `{{ .Values.metrics.port | quote }}`, lines 36-38) still only advertises `ingest`'s port — the annotation-based scrape fallback cannot advertise two ports from one Pod. `reference`'s own `/metrics` (if `podMonitor.enabled`, which supports per-container ports) is scrapeable; under the plain-annotation fallback it is not. Flagged here as a known, accepted limitation (mirrors the spec's own Open Question 6 — "resource/security-context sizing... not sized here; left to implementation" — this is the same class of deferred, not-required-for-correctness detail), not silently unaddressed.

- [ ] **Step 4: Add the two new env vars to `trust-consumer-deployment.yaml`**

After `REFERENCE_RELOAD_SECS` (currently lines 108-109):

```yaml
            - name: STANOX_CRS_RELOAD_SECS
              value: {{ .Values.trustConsumer.stanoxCrsReloadSecs | quote }}
            - name: STANOX_CRS_URL
              value: {{ printf "%s/private/stanox-crs" (include "distant-signal.apiBaseUrl" .) | quote }}
```

- [ ] **Step 5: Render and lint the chart**

Run: `helm lint charts/distant-signal --set scheduleFeed.enabled=true --set scheduleFeed.sftp.authMethod=password --set trustConsumer.kafka.brokers=x --set trustConsumer.kafka.topic=x --set trustConsumer.kafka.saslMechanism=PLAIN`
Expected: `0 chart(s) failed`. Then confirm the new container actually renders:

Run: `helm template charts/distant-signal --set scheduleFeed.enabled=true --set scheduleFeed.sftp.authMethod=password --set trustConsumer.kafka.brokers=x --set trustConsumer.kafka.topic=x --set trustConsumer.kafka.saslMechanism=PLAIN | grep -A2 "name: reference"`
Expected: shows the `reference` container's `image:` line, confirming it rendered inside the `schedulefeed` Deployment.

- [ ] **Step 6: Commit**

```bash
git add docker/schedule-reference.Dockerfile charts/distant-signal/templates/schedulefeed-deployment.yaml charts/distant-signal/templates/trust-consumer-deployment.yaml charts/distant-signal/values.yaml
git commit -m "Add the schedule-reference container to the schedulefeed Pod and wire trust-consumer's new reload env vars"
```

---

### Task 8: End-to-end verification

**Files:** none modified — this task only runs commands and confirms behavior across every prior task's output.
**Depends on:** Tasks 1-7.

- [ ] **Step 1: Full workspace build and test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS. (DB-touching tests added in Task 3 remain `#[ignore]`d in this run, per this repo's convention — see Step 3 below for running them explicitly.)

- [ ] **Step 2: `cargo clippy` across the two touched/new crates**

Run: `cargo clippy -p schedule-reference -p trust-consumer -p api -p common --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 3: Run the DB-touching test against a local Postgres**

Using whatever this repo's existing local Postgres convention is (matching how `queries.rs:1024-1059`'s own `#[ignore]`d tests are run):

Run: `DATABASE_URL=postgres://<local-connection> cargo test -p api a_re_post_with_a_changed_crs_overwrites_the_existing_row -- --ignored`
Expected: PASS.

- [ ] **Step 4: Migration applies cleanly**

Run whatever this repo's existing migration-apply command is (e.g. `sqlx migrate run` against a local/test database pointed at `crates/api/migrations`) and confirm `stanox_crs` exists with the expected columns:

Run: `psql "$DATABASE_URL" -c '\d stanox_crs'`
Expected: shows `stanox` (PK), `crs`, `tiploc`, `station_name`, `source_sequence`, `updated_at`.

- [ ] **Step 5: End-to-end parse against the real `timetable_full.zip` extract, through the real endpoint**

Building on Task 4 Step 5's manual smoke test, but now POSTing for real against a locally running `api`:

```bash
mkdir -p /tmp/schedule-reference-e2e/942
cd /workspaces/github-com-fasterspeeding-network-rail-status
unzip -p timetable_full.zip RJTTF942MCA.txt > /tmp/schedule-reference-e2e/942/RJTTF942MCA.txt
unzip -p timetable_full.zip RJTTF942MSN.txt > /tmp/schedule-reference-e2e/942/RJTTF942MSN.txt
STORAGE_DIR=/tmp/schedule-reference-e2e \
  API_INGEST_URL=http://localhost:8080/private/stanox-crs \
  INTERNAL_TOKEN=<local internal token> \
  POLL_INTERVAL_SECS=5 \
  cargo run -p schedule-reference
# Confirm via: curl -H "x-internal-token: <token>" http://localhost:8080/private/stanox-crs | jq 'length'
# Expected: 3124 (matching stanox_crs.rs's own the_real_data_file_has_3124_entries
# regression test's count for this same 2026-08-28 extract).
rm -rf /tmp/schedule-reference-e2e
```

- [ ] **Step 6: Confirm `trust-consumer`'s live reload actually picks up the posted table**

With `api`'s `stanox_crs` table now populated (Step 5) and a `trust-consumer` process running against the same `api` with a short `STANOX_CRS_RELOAD_SECS` (e.g. `5`), confirm via logs that `"reloaded live stanox/crs table"` appears with `count=3124` within one reload interval, and that a STANOX known to differ between the checked-in CSV and this specific extract (if any — otherwise any real STANOX) resolves identically before and after the reload swap.

- [ ] **Step 7: Confirm the chart still renders with `scheduleFeed.enabled=false` (the default)**

Run: `helm template charts/distant-signal | grep -c "name: reference"`
Expected: `0` — the new container, like the whole `schedulefeed` Deployment, is gated behind `scheduleFeed.enabled` and must not render when it's off (the chart's existing default), confirming this plan didn't accidentally make the new container unconditional.

- [ ] **Step 8: Final review pass**

Confirm: no task deleted or modified `reference-data/stanox-crs.csv`'s content (only its consumers' doc comments changed, per Task 6); no task touched `frontend/` or any `/public/*` route; `cargo build --workspace` and `helm lint` both still pass after every commit in Tasks 1-7 applied in sequence (not just the final state) — spot-check by running `git log --oneline` against this plan's commits and confirming each one is independently buildable if that matters for this repo's bisectability conventions.
