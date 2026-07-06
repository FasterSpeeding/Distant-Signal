# Aggregator + Read API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port the Python matcher/segments/aggregation prototype to a new
Rust `aggregator` service that periodically computes each line's status
into the already-existing `line_status`/`line_status_history` tables, and
add the four TfL-shaped read endpoints DESIGN.md describes to the existing
`api` crate.

**Architecture:** A new standalone binary crate (`crates/aggregator`) reads
`incidents` + `station_samples` from Postgres every 60s, runs the ported
matcher/aggregation logic against the loaded line catalogue, and
upserts/prunes the two status tables. The `api` crate gets four new
unauthenticated read endpoints under `public_router()` that query those
same tables and render TfL-shaped JSON.

**Tech Stack:** Rust (existing Cargo workspace), sqlx/Postgres, axum,
chrono, serde/serde_json. No new external dependencies.

## Global Constraints

- **No `severity_hint`/`priority`-based severity classification.** The real
  `common::IncidentMessage.priority: i32` has no confirmed meaning (a
  documented, unresolved RDM gap). Severity comes *only* from keyword
  matching on `summary`+`description` text.
- **`IncidentMessage.validity: Vec<ValidityPeriod>` can hold 0, 1, or many
  entries.** A `LineStatus`'s single `validity: ValidityPeriod` field is
  chosen by: if the vec is empty, synthesize `{from_date: now(), to_date:
  None, is_now: true}`; otherwise pick the first entry where `from_date <=
  now && (to_date.is_none() || to_date > now)`; otherwise fall back to the
  first entry in the vec.
- **The matcher/segments port is otherwise faithful** — every existing
  Python test in `tests/test_matcher.py` gets an equivalent Rust test with
  the same assertions, adapted only where the `IncidentMessage`
  construction needs to drop the nonexistent `severity_hint` field (see
  Task 3's notes on why this doesn't change any test's expected outcome).
- **`aggregator` is a separate binary crate**, not a background task in
  `api`. Default poll cadence 60s, configurable via `POLL_INTERVAL_SECS`
  env var (matching every poller's existing pattern).
- **The four read endpoints are unauthenticated**, mounted on
  `public_router()` — this is a deliberate contrast with `/private/*`,
  matching TfL's own public API.
- **`/StopPoint/{crs}/Disruption` resolves station-to-line membership
  in-memory** from the already-loaded `LineDefinition`s — no denormalized
  column, no new source of truth beyond `lines/*.toml`.
- **`line_status_history` pruning is in scope**: the aggregator deletes
  rows older than a configurable retention window (default 7 days) once
  per cycle.
- **Keep line-catalogue loading behind a narrow interface** (a function
  returning `Vec<LineDefinition>`) in both `api` and `aggregator` — this is
  already true today (`LineDefinition::from_dir`) and must not regress, to
  keep a future move to a Postgres-backed catalogue a localized change.

---

## Current relevant code (read before starting; verified against the real files, not reconstructed from memory)

**`crates/common/src/lib.rs`** already has (unchanged by this plan except
Task 1's `Defaults` move):
```rust
pub struct ValidityPeriod {
    pub from_date: DateTime<Utc>,
    pub to_date: Option<DateTime<Utc>>,
    pub is_now: bool,
}
pub struct AffectedRoute { pub from_crs: String, pub to_crs: String }
pub struct Disruption {
    pub category: String,
    pub description: String,
    pub affected_stops: Vec<String>,
    pub affected_routes: Vec<AffectedRoute>,
    pub source: Option<String>,
}
pub struct LineStatus {
    pub severity: Severity,
    pub reason: String,
    pub validity: ValidityPeriod,
    pub disruption: Option<Disruption>,
    pub data_quality: DataQuality,
}
pub struct LineStatusReport {
    pub id: String, pub name: String, pub mode_name: String,
    pub operators: Vec<String>, pub statuses: Vec<LineStatus>,
}
impl LineStatusReport {
    pub fn worst_severity(&self) -> Severity { /* min of statuses, or GoodService if empty */ }
}
pub struct IncidentMessage {
    pub incident_id: String, pub summary: String, pub description: String,
    pub operators: Vec<String>, pub affected_stations: Vec<String>,
    pub priority: i32, pub validity: Vec<ValidityPeriod>,
    pub is_planned: bool, pub is_cleared: bool,
}
pub struct StationDeparture {
    pub service_id: String, pub operator: String, pub destination_crs: String,
    pub scheduled: String, pub estimated: String, pub is_cancelled: bool,
    pub delay_minutes: i32, pub cancel_reason: Option<String>,
    pub delay_reason: Option<String>, pub headcode: Option<String>,
}
pub struct StationSample {
    pub crs: String, pub polled_at: DateTime<Utc>, pub departures: Vec<StationDeparture>,
}
pub struct Station {
    pub crs: String, pub tiploc: Option<String>, pub role: String,
    pub segment: Option<String>,
}
pub struct LineDefinition {
    pub id: String, pub name: String, pub mode: String, pub category: String,
    pub operators: Vec<String>, pub stations: Vec<Station>,
    pub sample_stations: Vec<String>, pub match_keywords: Vec<String>,
    pub excluded_keywords: Vec<String>, pub severity_overrides: HashMap<String, f64>,
    pub exclusive_segments: Vec<String>, pub destination_crs_filter: Vec<String>,
    pub headcode_prefixes: Vec<String>,
}
impl LineDefinition {
    pub fn from_file(path: &Path) -> Result<Self>;
    pub fn from_dir(dir_path: &Path) -> Result<Vec<Self>>;
    pub fn has_station(&self, crs: &str) -> bool;
    pub fn segment_for(&self, crs: &str) -> Option<&str>;
    pub fn segments(&self) -> HashSet<&str>;
    pub fn stations_between(&self, from_crs: &str, to_crs: &str) -> Vec<&str>;
}
```
`DataQuality` already has `#[serde(rename_all = "kebab-case")]` with
variants `Knowledgebase` (default), `LdbwsInferred`, `TrustInferred`,
`Planned`.

**`crates/api/src/data/config.rs`**'s `Defaults` (to be moved, see Task 1):
```rust
#[serde_inline_default]
#[derive(Clone, Deserialize, Debug)]
pub struct Defaults {
    #[serde_inline_default(5)] delay_threshold_minutes: i64,
    #[serde_inline_default(0.25)] minor_delays_pct: f64,
    #[serde_inline_default(0.50)] severe_delays_pct: f64,
    #[serde_inline_default(0.25)] reduced_service_pct: f64,
    #[serde_inline_default(0.60)] part_suspended_pct: f64,
    #[serde_inline_default(0)] knowledgebase_severity_floor: i8,
    #[serde_inline_default(3)] min_sample_size: i64,
}
```
Fields are currently **not** `pub` — Task 1 makes them `pub` since
`aggregator` needs to read them.

**`crates/api/migrations/20260510023522_initial.sql`** already has (no
migration changes needed in this plan):
```sql
CREATE TABLE line_status (
    line_id TEXT PRIMARY KEY, name TEXT NOT NULL, mode_name TEXT NOT NULL,
    operators TEXT[] NOT NULL, statuses JSONB NOT NULL DEFAULT '[]',
    computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE TABLE line_status_history (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, line_id TEXT NOT NULL,
    statuses JSONB NOT NULL, computed_at TIMESTAMPTZ NOT NULL
);
```
Also already exist and unrelated to this plan: `incidents`, `stations`,
`tocs`, `station_samples` (all populated by the four existing pollers).

**`crates/api/src/data/config.rs`**'s `ServiceArguments` currently has
`bind_url`, `database_url`, `internal_token`, `defaults_file: Option<Defaults>`
(unused today), `lines: LineCatalogue` (a `Vec<LineDefinition>` newtype,
`Deref`-coercible, loaded via `--lines-dir`/`LINES_DIR`, defaulting to
`/app/lines`).

**The Python reference implementation** (`src/segments.py`, `matcher.py`,
`aggregator.py` — read in full during planning) is the algorithm being
ported. Exact logic is reproduced inline in each task below; do not
re-derive it from a paraphrase.

**`lines/*.toml`** (5 files: `west-coast-main-line.toml`,
`thameslink-core.toml`, `swr-south-west-main.toml`,
`swr-portsmouth-direct.toml`, `swr-alton.toml`) are the real fixtures the
ported tests load directly via `LineDefinition::from_dir`, exactly as
`tests/test_matcher.py` does via `LINES_DIR = Path(__file__).parent.parent
/ "lines"`. Relevant facts verified from the files themselves: WCML has
`match_keywords = ["West Coast Main Line", "WCML"]`,
`excluded_keywords = ["Cross Country"]`, and station `RUG` (role `major`,
segment `wcml-midlands`). The three SWR lines share segment
`swr-trunk-waterloo` (stations `WAT`/`CLJ`/`WIM`/`SUR`/`WOK`) and each have
their own exclusive branch segment (e.g. `swr-alton`'s `swr-alton-branch`
containing `AHT`/`FRM`/`AON` among others).

---

## Task 1: Move `Defaults` + threshold merging into `common`

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/common/Cargo.toml`
- Modify: `crates/api/src/data/config.rs`

**Interfaces:**
- Produces: `common::Defaults` (all fields `pub`), `common::thresholds_for(defaults: &Defaults, overrides: &HashMap<String, f64>) -> Defaults`.
- Consumes (by later tasks): both of the above, from `crates/aggregator`.

- [ ] **Step 1: Add `serde_inline_default` and `serde-inline-default` to `common`'s dependencies**

`crates/common/Cargo.toml` currently has no `serde-inline-default`
dependency (only `crates/api` does). Add it:
```toml
[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
glob = "0.3.3"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls"] }
serde = { version = "1.0.228", features = ["derive"] }
serde-inline-default = "1.0.1"
serde_json = "1.0.149"
serde_repr = "0.1.20"
toml = { version = "1.1.2", features = ["serde"] }
tracing = "0.1.44"
```
(only the `serde-inline-default = "1.0.1"` line is new; keep every other
existing dependency line unchanged.)

- [ ] **Step 2: Add `Defaults` and `thresholds_for` to `crates/common/src/lib.rs`**

Add near the top of the file, after the existing `use` statements (add
`use serde_inline_default::serde_inline_default;` to the imports):
```rust
use serde_inline_default::serde_inline_default;
```

Add this after the `TocReference` struct definition, at the end of the
file:
```rust
// --- Aggregator thresholds (ported from Python config.py DEFAULTS) ---

/// Default thresholds for status derivation. Lines override any subset via
/// `LineDefinition.severity_overrides`. Field names match the keys used in
/// `severity_overrides` TOML tables (e.g. `minor_delays_pct = 0.20`).
#[serde_inline_default]
#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Defaults {
    /// A service is "delayed" once its delay exceeds this many minutes.
    #[serde_inline_default(5)]
    pub delay_threshold_minutes: i64,
    /// >25% of sampled services delayed -> Minor Delays.
    #[serde_inline_default(0.25)]
    pub minor_delays_pct: f64,
    /// >50% of sampled services delayed -> Severe Delays.
    #[serde_inline_default(0.50)]
    pub severe_delays_pct: f64,
    /// >25% of sampled services cancelled -> Reduced Service.
    #[serde_inline_default(0.25)]
    pub reduced_service_pct: f64,
    /// >60% of sampled services cancelled -> Part Suspended.
    #[serde_inline_default(0.60)]
    pub part_suspended_pct: f64,
    /// Unused by the current keyword-only severity classifier; kept for
    /// parity with the Python prototype's `DEFAULTS` dict and any future
    /// use once `IncidentMessage.priority`'s meaning is confirmed.
    #[serde_inline_default(0)]
    pub knowledgebase_severity_floor: i8,
    /// Below this many sampled services, don't infer a status from LDBWS
    /// samples alone.
    #[serde_inline_default(3)]
    pub min_sample_size: i64,
}

impl Default for Defaults {
    fn default() -> Self {
        toml::from_str("").expect("Defaults must deserialize from an empty TOML table via serde_inline_default")
    }
}

/// Merges a line's `severity_overrides` on top of shared `Defaults`,
/// returning a new `Defaults` with any recognized keys overridden. Unknown
/// keys are ignored (there's no field for them to override). Ported from
/// Python's `config.thresholds_for`.
pub fn thresholds_for(defaults: &Defaults, overrides: &HashMap<String, f64>) -> Defaults {
    let mut merged = defaults.clone();
    for (key, value) in overrides {
        match key.as_str() {
            "delay_threshold_minutes" => merged.delay_threshold_minutes = *value as i64,
            "minor_delays_pct" => merged.minor_delays_pct = *value,
            "severe_delays_pct" => merged.severe_delays_pct = *value,
            "reduced_service_pct" => merged.reduced_service_pct = *value,
            "part_suspended_pct" => merged.part_suspended_pct = *value,
            "knowledgebase_severity_floor" => merged.knowledgebase_severity_floor = *value as i8,
            "min_sample_size" => merged.min_sample_size = *value as i64,
            _ => {}
        }
    }
    merged
}

#[cfg(test)]
mod defaults_tests {
    use super::*;

    #[test]
    fn no_overrides_returns_defaults_unchanged() {
        let defaults = Defaults::default();
        let merged = thresholds_for(&defaults, &HashMap::new());
        assert_eq!(merged, defaults);
    }

    #[test]
    fn partial_override_changes_only_named_fields() {
        let defaults = Defaults::default();
        let mut overrides = HashMap::new();
        overrides.insert("minor_delays_pct".to_string(), 0.20);
        overrides.insert("delay_threshold_minutes".to_string(), 4.0);
        let merged = thresholds_for(&defaults, &overrides);
        assert_eq!(merged.minor_delays_pct, 0.20);
        assert_eq!(merged.delay_threshold_minutes, 4);
        assert_eq!(merged.severe_delays_pct, defaults.severe_delays_pct);
        assert_eq!(merged.min_sample_size, defaults.min_sample_size);
    }

    #[test]
    fn every_field_can_be_overridden() {
        let defaults = Defaults::default();
        let mut overrides = HashMap::new();
        overrides.insert("delay_threshold_minutes".to_string(), 10.0);
        overrides.insert("minor_delays_pct".to_string(), 0.30);
        overrides.insert("severe_delays_pct".to_string(), 0.60);
        overrides.insert("reduced_service_pct".to_string(), 0.40);
        overrides.insert("part_suspended_pct".to_string(), 0.70);
        overrides.insert("knowledgebase_severity_floor".to_string(), 1.0);
        overrides.insert("min_sample_size".to_string(), 5.0);
        let merged = thresholds_for(&defaults, &overrides);
        assert_eq!(merged.delay_threshold_minutes, 10);
        assert_eq!(merged.minor_delays_pct, 0.30);
        assert_eq!(merged.severe_delays_pct, 0.60);
        assert_eq!(merged.reduced_service_pct, 0.40);
        assert_eq!(merged.part_suspended_pct, 0.70);
        assert_eq!(merged.knowledgebase_severity_floor, 1);
        assert_eq!(merged.min_sample_size, 5);
    }

    #[test]
    fn unknown_key_is_ignored() {
        let defaults = Defaults::default();
        let mut overrides = HashMap::new();
        overrides.insert("not_a_real_field".to_string(), 42.0);
        let merged = thresholds_for(&defaults, &overrides);
        assert_eq!(merged, defaults);
    }
}
```

- [ ] **Step 2b: Run the new tests**

Run: `cargo test -p common defaults_tests`
Expected: `test result: ok. 4 passed; 0 failed`.

- [ ] **Step 3: Remove the old `Defaults` from `crates/api/src/data/config.rs`, re-export from `common`**

Delete the entire `#[serde_inline_default] #[derive(...)] pub struct
Defaults { ... }` block from `crates/api/src/data/config.rs`. Add this
import near the top of the file instead:
```rust
pub use common::Defaults;
```

Confirm `parse_toml_path::<Defaults>` (used by the `--defaults-file`
`ServiceArguments` field) still compiles — it should, since `Defaults`
still implements `Deserialize` and is now just re-exported rather than
locally defined.

Also remove the now-unused `use serde_inline_default::serde_inline_default;`
import from `crates/api/src/data/config.rs` if nothing else in that file
uses the attribute macro directly (check first — `ServiceArguments` itself
doesn't use `#[serde_inline_default]`, only the old `Defaults` did).

- [ ] **Step 4: Verify the workspace builds and all existing tests still pass**

Run: `cargo build --workspace && cargo test --workspace`
Expected: builds cleanly; test counts match pre-Task-1 counts plus the 4
new `defaults_tests` (so `common`'s test count goes from 0 to 4, `api`'s
stays at 14).

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/lib.rs crates/common/Cargo.toml crates/api/src/data/config.rs Cargo.lock
git commit -m "Move Defaults threshold struct into common, shared with the future aggregator crate"
```

---

## Task 2: `crates/aggregator` — segments + matcher port

**Files:**
- Create: `crates/aggregator/Cargo.toml`
- Create: `crates/aggregator/src/segments.rs`
- Create: `crates/aggregator/src/matcher.rs`
- Create: `crates/aggregator/src/main.rs` (placeholder only — real one in Task 4)
- Modify: `Cargo.toml` (workspace root)

**Interfaces:**
- Consumes: `common::LineDefinition`, `common::Station`, `common::IncidentMessage`.
- Produces: `segments::SegmentRegistry` (with `new`, `lines_for_segment`,
  `is_shared`, `is_exclusive_to`, `segment_at`, `segments_touched_by`);
  `matcher::{MatchScope, Match, lines_affected_by}`.

- [ ] **Step 1: Add the crate to the workspace**

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
    "crates/aggregator",
]
```

- [ ] **Step 2: Create the crate manifest**

```toml
[package]
name = "aggregator"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
sqlx = { version = "0.8.6", features = ["chrono", "json", "macros", "postgres", "runtime-tokio", "tls-native-tls"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

- [ ] **Step 3: Write `segments.rs`**

Port of `src/segments.py`, verbatim in logic (`HashMap`/`HashSet` in place
of Python's `dict`/`set`, `Vec` preserving insertion order in place of
Python's list-based dedup):
```rust
//! Segment registry: cross-line view of which segments are shared and
//! which are exclusive, derived from the full set of loaded line
//! definitions. Ported from `src/segments.py`.

use std::collections::{HashMap, HashSet};

use common::LineDefinition;

/// Indexes segment usage across all known lines.
pub struct SegmentRegistry {
    /// segment -> ordered list of unique line IDs that include it.
    segment_lines: HashMap<String, Vec<String>>,
    /// (line_id, crs) -> segment.
    station_segments: HashMap<(String, String), String>,
}

impl SegmentRegistry {
    pub fn new(lines: &HashMap<String, LineDefinition>) -> Self {
        let mut segment_lines: HashMap<String, Vec<String>> = HashMap::new();
        let mut station_segments: HashMap<(String, String), String> = HashMap::new();

        for line in lines.values() {
            for station in &line.stations {
                if let Some(segment) = &station.segment {
                    let entry = segment_lines.entry(segment.clone()).or_default();
                    if !entry.contains(&line.id) {
                        entry.push(line.id.clone());
                    }
                    station_segments.insert((line.id.clone(), station.crs.clone()), segment.clone());
                }
            }
        }

        Self { segment_lines, station_segments }
    }

    /// Every line ID that includes this segment, in load order.
    pub fn lines_for_segment(&self, segment: &str) -> Vec<String> {
        self.segment_lines.get(segment).cloned().unwrap_or_default()
    }

    /// A segment is shared if more than one line uses it.
    pub fn is_shared(&self, segment: &str) -> bool {
        self.segment_lines.get(segment).map(|v| v.len() > 1).unwrap_or(false)
    }

    /// True if `line_id` is the only line using this segment.
    pub fn is_exclusive_to(&self, segment: &str, line_id: &str) -> bool {
        matches!(self.segment_lines.get(segment), Some(users) if users == &[line_id.to_string()])
    }

    pub fn segment_at(&self, line_id: &str, crs: &str) -> Option<&str> {
        self.station_segments
            .get(&(line_id.to_string(), crs.to_string()))
            .map(|s| s.as_str())
    }

    /// Which of this line's segments are touched by these stations.
    pub fn segments_touched_by(&self, line: &LineDefinition, affected_stations: &[String]) -> HashSet<String> {
        affected_stations
            .iter()
            .filter_map(|crs| self.station_segments.get(&(line.id.clone(), crs.clone())).cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load_all_lines() -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        LineDefinition::from_dir(&dir)
            .expect("lines/ directory should parse")
            .into_iter()
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    #[test]
    fn shared_trunk_segment_is_shared_across_three_swr_lines() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        assert!(registry.is_shared("swr-trunk-waterloo"));
        let mut users = registry.lines_for_segment("swr-trunk-waterloo");
        users.sort();
        assert_eq!(
            users,
            vec!["swr-alton", "swr-portsmouth-direct", "swr-south-west-main"]
        );
    }

    #[test]
    fn exclusive_branch_segment_is_not_shared() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        assert!(!registry.is_shared("swr-alton-branch"));
        assert!(registry.is_exclusive_to("swr-alton-branch", "swr-alton"));
        assert!(!registry.is_exclusive_to("swr-alton-branch", "swr-south-west-main"));
    }

    #[test]
    fn segment_at_returns_the_right_segment_for_a_station() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        assert_eq!(registry.segment_at("swr-alton", "WOK"), Some("swr-trunk-waterloo"));
        assert_eq!(registry.segment_at("swr-alton", "AON"), Some("swr-alton-branch"));
        assert_eq!(registry.segment_at("swr-alton", "NOTASTATION"), None);
    }

    #[test]
    fn segments_touched_by_finds_shared_and_exclusive_together() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let alton = &lines["swr-alton"];
        let touched = registry.segments_touched_by(alton, &["WOK".to_string(), "AON".to_string()]);
        assert_eq!(touched.len(), 2);
        assert!(touched.contains("swr-trunk-waterloo"));
        assert!(touched.contains("swr-alton-branch"));
    }
}
```

- [ ] **Step 4: Write `matcher.rs`**

Port of `src/matcher.py`, unchanged in logic — this module never touches
`severity_hint`/`priority`, so no adaptation is needed here at all (only
`aggregator.rs`, Task 3, needs one):
```rust
//! Decide which lines a Knowledgebase incident affects, and classify the
//! scope of each match. Ported from `src/matcher.py`.

use std::collections::{HashMap, HashSet};

use common::{IncidentMessage, LineDefinition};

use crate::segments::SegmentRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchScope {
    ExclusiveSegment,
    SharedSegment,
    StationHit,
    KeywordOnly,
    OperatorOnly,
}

#[derive(Debug, Clone)]
pub struct Evidence {
    pub stations: Vec<String>,
    pub segments: Vec<String>,
    pub operators: Vec<String>,
    pub keywords: Vec<String>,
}

pub struct Match<'a> {
    pub line: &'a LineDefinition,
    pub scope: MatchScope,
    pub evidence: Evidence,
}

/// Return all lines the incident could plausibly affect, classified.
pub fn lines_affected_by<'a>(
    incident: &IncidentMessage,
    lines: &'a HashMap<String, LineDefinition>,
    registry: &SegmentRegistry,
) -> Vec<Match<'a>> {
    let haystack = format!("{} {}", incident.summary, incident.description).to_lowercase();
    let mut out: Vec<Match<'a>> = Vec::new();

    for line in lines.values() {
        if is_excluded(line, &haystack) {
            continue;
        }
        if let Some(m) = match_one(line, incident, registry, &haystack) {
            out.push(m);
        }
    }

    // If any precise match exists, drop operator-only matches — they're
    // almost certainly false positives where another line on the same
    // operator is the actual target.
    let has_precise = out.iter().any(|m| m.scope != MatchScope::OperatorOnly);
    if has_precise {
        out.retain(|m| m.scope != MatchScope::OperatorOnly);
    }

    out
}

fn match_one<'a>(
    line: &'a LineDefinition,
    incident: &IncidentMessage,
    registry: &SegmentRegistry,
    haystack: &str,
) -> Option<Match<'a>> {
    let operator_overlap: Vec<String> = line
        .operators
        .iter()
        .filter(|op| incident.operators.contains(op))
        .cloned()
        .collect();
    let station_hits: Vec<String> = incident
        .affected_stations
        .iter()
        .filter(|crs| line.has_station(crs))
        .cloned()
        .collect();
    let keyword_hits: Vec<String> = line
        .match_keywords
        .iter()
        .filter(|kw| haystack.contains(&kw.to_lowercase()))
        .cloned()
        .collect();

    // Tier 1: station hits — try to classify by segment.
    if !station_hits.is_empty() {
        let segments: HashSet<String> = registry.segments_touched_by(line, &station_hits);
        let evidence = Evidence {
            stations: station_hits,
            segments: segments.iter().cloned().collect(),
            operators: operator_overlap,
            keywords: keyword_hits,
        };

        if !segments.is_empty() && segments.iter().all(|s| registry.is_exclusive_to(s, &line.id)) {
            return Some(Match { line, scope: MatchScope::ExclusiveSegment, evidence });
        }
        if !segments.is_empty() && segments.iter().any(|s| registry.is_shared(s)) {
            return Some(Match { line, scope: MatchScope::SharedSegment, evidence });
        }
        return Some(Match { line, scope: MatchScope::StationHit, evidence });
    }

    // Tier 2: keyword match.
    if !keyword_hits.is_empty() {
        return Some(Match {
            line,
            scope: MatchScope::KeywordOnly,
            evidence: Evidence { stations: vec![], segments: vec![], operators: operator_overlap, keywords: keyword_hits },
        });
    }

    // Tier 3: operator only.
    if !operator_overlap.is_empty() {
        return Some(Match {
            line,
            scope: MatchScope::OperatorOnly,
            evidence: Evidence { stations: vec![], segments: vec![], operators: operator_overlap, keywords: vec![] },
        });
    }

    None
}

fn is_excluded(line: &LineDefinition, haystack: &str) -> bool {
    line.excluded_keywords.iter().any(|kw| haystack.contains(&kw.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load_line(id: &str) -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        let all = LineDefinition::from_dir(&dir).expect("lines/ directory should parse");
        all.into_iter()
            .filter(|l| l.id == id)
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    fn load_all_lines() -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        LineDefinition::from_dir(&dir)
            .expect("lines/ directory should parse")
            .into_iter()
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    fn incident(id: &str, summary: &str, description: &str, operators: &[&str], affected_stations: &[&str]) -> IncidentMessage {
        IncidentMessage {
            incident_id: id.to_string(),
            summary: summary.to_string(),
            description: description.to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            affected_stations: affected_stations.iter().map(|s| s.to_string()).collect(),
            priority: 0,
            validity: vec![],
            is_planned: false,
            is_cleared: false,
        }
    }

    #[test]
    fn excluded_keyword_vetoes_match() {
        let lines = load_line("wcml");
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("T1", "Cross Country delays", "Cross Country services are delayed at Rugby.", &[], &["RUG"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        assert!(matches.is_empty(), "excluded keyword should veto match");
    }

    #[test]
    fn keyword_only_match() {
        let lines = load_line("wcml");
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("T2", "WCML engineering", "Overnight engineering work on the West Coast Main Line.", &[], &[]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].scope, MatchScope::KeywordOnly);
    }

    #[test]
    fn swr_shared_trunk_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("SWR-1", "Signal failure at Woking", "Signal failure causing delays to SWR services.", &["SW"], &["WOK"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("swr-south-west-main"));
        assert!(matched_ids.contains("swr-portsmouth-direct"));
        assert!(matched_ids.contains("swr-alton"));
        for m in &matches {
            if m.line.id.starts_with("swr-") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn swr_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("SWR-2", "Power supply issue at Alton", "Power supply problem causing delays at Alton.", &["SW"], &["AON"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["swr-alton".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }
}
```

Note: the two SWR-specific matcher tests above **drop the Python
original's `severity_hint` field** — these tests only assert `MatchScope`,
never severity, so the field was inert to their outcome and its removal
changes nothing about what's being verified.

- [ ] **Step 5: Write a placeholder `main.rs`** (replaced for real in Task 4)

```rust
mod matcher;
mod segments;

fn main() {
    println!("stub");
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p aggregator`
Expected: `test result: ok. 8 passed; 0 failed` (4 segments tests + 4
matcher tests, including the two SWR scenarios — the remaining 3 Python
tests, which exercise `aggregate()` rather than the matcher alone, are
ported in Task 3).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/aggregator
git commit -m "Add aggregator crate: port SegmentRegistry and the incident matcher"
```

---

## Task 3: `aggregator` crate — aggregation logic

**Files:**
- Create: `crates/aggregator/src/aggregation.rs`
- Modify: `crates/aggregator/src/main.rs`

**Interfaces:**
- Consumes: `matcher::{MatchScope, Match, lines_affected_by}`,
  `segments::SegmentRegistry`, `common::{Defaults, thresholds_for,
  IncidentMessage, StationSample, StationDeparture, LineDefinition,
  LineStatus, LineStatusReport, ValidityPeriod, Disruption, AffectedRoute,
  Severity, DataQuality}`.
- Produces: `aggregation::aggregate(lines: &HashMap<String, LineDefinition>,
  incidents: &[IncidentMessage], samples: &HashMap<String, StationSample>,
  registry: &SegmentRegistry, defaults: &Defaults) -> HashMap<String, LineStatusReport>`.

- [ ] **Step 1: Write `aggregation.rs`**

Port of `src/aggregator.py`, with the two adaptations from the Global
Constraints (no `severity_hint`; validity-period selection from a `Vec`):
```rust
//! Combine Knowledgebase incidents and LDBWS samples into one status
//! report per line. Ported from `src/aggregator.py`, adapted for the real
//! `common::IncidentMessage` shape (see module-level notes below).
//!
//! Two adaptations from the Python prototype, both because the real
//! `IncidentMessage` (built against confirmed RDM facts) differs from
//! what the prototype assumed:
//! 1. No `severity_hint` field exists — `priority: i32`'s meaning is an
//!    unresolved RDM gap, so severity classification uses keyword text
//!    only, dropping the Python version's `severity_hint == "major"`/
//!    `"minor"` branches.
//! 2. `IncidentMessage.validity` is a `Vec<ValidityPeriod>`, not a single
//!    optional pair — `validity_for_output` below picks one period for the
//!    (still-singular) `LineStatus.validity` field.

use std::collections::HashMap;

use chrono::Utc;
use common::{
    AffectedRoute, DataQuality, Defaults, Disruption, IncidentMessage, LineDefinition, LineStatus,
    LineStatusReport, Severity, StationDeparture, StationSample, ValidityPeriod, thresholds_for,
};

use crate::matcher::{Match, MatchScope, lines_affected_by};
use crate::segments::SegmentRegistry;

pub fn aggregate(
    lines: &HashMap<String, LineDefinition>,
    incidents: &[IncidentMessage],
    samples: &HashMap<String, StationSample>,
    registry: &SegmentRegistry,
    defaults: &Defaults,
) -> HashMap<String, LineStatusReport> {
    let mut reports: HashMap<String, LineStatusReport> = lines
        .values()
        .map(|line| {
            (
                line.id.clone(),
                LineStatusReport {
                    id: line.id.clone(),
                    name: line.name.clone(),
                    mode_name: line.mode.clone(),
                    operators: line.operators.clone(),
                    statuses: vec![],
                },
            )
        })
        .collect();

    // Layer 1: incidents.
    for incident in incidents {
        for m in lines_affected_by(incident, lines, registry) {
            let status = status_from_incident(&m, incident);
            reports.get_mut(&m.line.id).unwrap().statuses.push(status);
        }
    }

    // Layer 2: inference for lines with no incidents.
    for line in lines.values() {
        let report = reports.get_mut(&line.id).unwrap();
        if !report.statuses.is_empty() {
            continue;
        }
        let inferred = infer_from_samples(line, samples, defaults);
        report.statuses.push(inferred.unwrap_or_else(good_service));
    }

    reports
}

// --- Incident path ---

fn status_from_incident(m: &Match, incident: &IncidentMessage) -> LineStatus {
    let base_severity = severity_from_incident(incident);
    let severity = demote_for_scope(base_severity, m.scope);

    let affected_stations = m.evidence.stations.clone();
    let affected_routes = routes_from_stations(m.line, &affected_stations);

    let mut reason = incident.summary.clone();
    match m.scope {
        MatchScope::SharedSegment => reason.push_str(" (shared trunk — also affects other lines)"),
        MatchScope::OperatorOnly => reason.push_str(" (operator-wide report)"),
        _ => {}
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
    }
}

/// Picks one `ValidityPeriod` for `LineStatus.validity` from an incident's
/// (possibly empty, possibly multi-entry) `validity` vec. See module docs
/// for why this exists — the real schema allows repeated validity periods,
/// the output type doesn't.
fn validity_for_output(periods: &[ValidityPeriod]) -> ValidityPeriod {
    if periods.is_empty() {
        return ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true };
    }
    let now = Utc::now();
    periods
        .iter()
        .find(|p| p.from_date <= now && p.to_date.map(|to| to > now).unwrap_or(true))
        .cloned()
        .unwrap_or_else(|| periods[0].clone())
}

fn severity_from_incident(incident: &IncidentMessage) -> Severity {
    if incident.is_planned {
        return Severity::PlannedClosure;
    }

    let text = format!("{} {}", incident.summary, incident.description).to_lowercase();
    if text.contains("suspended") || text.contains("no service") {
        return Severity::Suspended;
    }
    if text.contains("rail replacement") || text.contains("replacement bus") {
        return Severity::BusService;
    }
    if text.contains("lines blocked") || text.contains("all lines blocked") {
        return Severity::PartSuspended;
    }
    if text.contains("severe delays") || text.contains("major disruption") {
        return Severity::SevereDelays;
    }
    if text.contains("diverted") {
        return Severity::Diverted;
    }
    Severity::MinorDelays
}

/// Weaker evidence -> milder reported status. Lower severity numbers are
/// more disruptive, so capping "at Minor Delays or milder" means picking
/// whichever of (severity, floor) sorts later (higher number = milder).
fn demote_for_scope(severity: Severity, scope: MatchScope) -> Severity {
    match scope {
        MatchScope::ExclusiveSegment | MatchScope::StationHit | MatchScope::SharedSegment => severity,
        MatchScope::KeywordOnly => severity.max(Severity::SevereDelays),
        MatchScope::OperatorOnly => severity.max(Severity::MinorDelays),
    }
}

fn routes_from_stations(line: &LineDefinition, stations: &[String]) -> Vec<AffectedRoute> {
    if stations.len() < 2 {
        return vec![];
    }
    let line_order: Vec<&str> = line.stations.iter().map(|s| s.crs.as_str()).collect();
    let mut in_order: Vec<&String> = stations.iter().collect();
    in_order.sort_by_key(|c| line_order.iter().position(|o| *o == c.as_str()).unwrap_or(999));
    vec![AffectedRoute { from_crs: in_order[0].clone(), to_crs: in_order[in_order.len() - 1].clone() }]
}

// --- Inference path ---

fn infer_from_samples(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> Option<LineStatus> {
    let thresholds = thresholds_for(defaults, &line.severity_overrides);

    let relevant: Vec<&StationDeparture> = line
        .sample_stations
        .iter()
        .filter_map(|crs| samples.get(crs))
        .flat_map(|sample| sample.departures.iter())
        .filter(|dep| belongs_to_line(dep, line))
        .collect();

    if (relevant.len() as i64) < thresholds.min_sample_size {
        return None;
    }

    let total = relevant.len();
    let cancelled = relevant.iter().filter(|d| d.is_cancelled).count();
    let delayed = relevant
        .iter()
        .filter(|d| !d.is_cancelled && d.delay_minutes as i64 >= thresholds.delay_threshold_minutes)
        .count();
    let cancel_rate = cancelled as f64 / total as f64;
    let delay_rate = delayed as f64 / total as f64;

    let (severity, mut reason) = classify(cancel_rate, delay_rate, &thresholds, total, cancelled, delayed);
    if severity == Severity::GoodService {
        return Some(good_service());
    }

    let reasons: Vec<&str> = relevant
        .iter()
        .filter_map(|d| d.delay_reason.as_deref().or(d.cancel_reason.as_deref()))
        .collect();
    if let Some(most_common) = most_common(&reasons) {
        reason.push_str(&format!(" (most cited: {most_common})"));
    }

    Some(LineStatus {
        severity,
        reason: reason.clone(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: Some(Disruption {
            category: "RealTime".to_string(),
            description: reason,
            affected_stops: samples.keys().cloned().collect(),
            affected_routes: vec![],
            source: Some("ldbws-sampling".to_string()),
        }),
        data_quality: DataQuality::LdbwsInferred,
    })
}

/// Operator filter is mandatory; destination-CRS/headcode-prefix filters
/// are optional narrowing, used at shared-trunk sample stations.
fn belongs_to_line(dep: &StationDeparture, line: &LineDefinition) -> bool {
    if !line.operators.contains(&dep.operator) {
        return false;
    }
    if !line.destination_crs_filter.is_empty() && !line.destination_crs_filter.contains(&dep.destination_crs) {
        return false;
    }
    if !line.headcode_prefixes.is_empty() {
        let Some(headcode) = &dep.headcode else { return false };
        if !line.headcode_prefixes.iter().any(|p| headcode.starts_with(p.as_str())) {
            return false;
        }
    }
    true
}

fn classify(
    cancel_rate: f64,
    delay_rate: f64,
    thresholds: &Defaults,
    total: usize,
    cancelled: usize,
    delayed: usize,
) -> (Severity, String) {
    if cancel_rate >= thresholds.part_suspended_pct {
        return (Severity::PartSuspended, format!("{cancelled} of {total} sampled services cancelled."));
    }
    if cancel_rate >= thresholds.reduced_service_pct {
        return (Severity::ReducedService, format!("{cancelled} of {total} sampled services cancelled."));
    }
    if delay_rate >= thresholds.severe_delays_pct {
        return (Severity::SevereDelays, format!("{delayed} of {total} sampled services delayed."));
    }
    if delay_rate >= thresholds.minor_delays_pct {
        return (Severity::MinorDelays, format!("{delayed} of {total} sampled services delayed."));
    }
    (Severity::GoodService, "Good Service".to_string())
}

fn good_service() -> LineStatus {
    LineStatus {
        severity: Severity::GoodService,
        reason: "Good Service".to_string(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: None,
        data_quality: DataQuality::LdbwsInferred,
    }
}

fn most_common<'a>(items: &[&'a str]) -> Option<&'a str> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for item in items {
        *counts.entry(item).or_insert(0) += 1;
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map(|(item, _)| item)
}
```

`Severity` needs `PartialOrd`/`Ord` for `.max(...)` above — it already
derives `PartialOrd, Ord` (confirmed in `crates/common/src/lib.rs`, and
its discriminant values are declared in ascending "more disruptive to
milder" order, so `.max()` correctly picks the milder one, matching
Python's `max(int(severity), int(floor))` on the same repr values).

Add `mod aggregation;` to `crates/aggregator/src/main.rs`'s existing
`mod matcher; mod segments;` lines.

- [ ] **Step 2: Add the ported `aggregate()`-level tests plus the two new adaptation tests**

Append to `crates/aggregator/src/aggregation.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load_all_lines() -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        LineDefinition::from_dir(&dir)
            .expect("lines/ directory should parse")
            .into_iter()
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    fn incident(id: &str, summary: &str, description: &str, operators: &[&str], affected_stations: &[&str]) -> IncidentMessage {
        IncidentMessage {
            incident_id: id.to_string(),
            summary: summary.to_string(),
            description: description.to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            affected_stations: affected_stations.iter().map(|s| s.to_string()).collect(),
            priority: 0,
            validity: vec![],
            is_planned: false,
            is_cleared: false,
        }
    }

    fn aggregate_with_defaults(
        lines: &HashMap<String, LineDefinition>,
        incidents: &[IncidentMessage],
    ) -> HashMap<String, LineStatusReport> {
        let registry = SegmentRegistry::new(lines);
        let defaults = Defaults::default();
        aggregate(lines, incidents, &HashMap::new(), &registry, &defaults)
    }

    #[test]
    fn aggregator_propagates_severity_through_shared_trunk() {
        // Description text already contains "severe delays", which the
        // keyword classifier alone resolves to SevereDelays — the Python
        // original's `severity_hint="major"` was redundant with this text,
        // not load-bearing, so dropping it changes nothing about the result.
        let lines = load_all_lines();
        let inc = incident(
            "SWR-3",
            "Signal failure at Woking",
            "Severe delays expected on SWR services.",
            &["SW"],
            &["WOK"],
        );
        let reports = aggregate_with_defaults(&lines, &[inc]);
        for line_id in ["swr-south-west-main", "swr-portsmouth-direct", "swr-alton"] {
            let worst = reports[line_id].worst_severity();
            assert!(
                (worst as i32) <= (Severity::SevereDelays as i32),
                "{line_id} should have severe-or-worse severity, got {worst:?}"
            );
        }
    }

    #[test]
    fn aggregator_isolates_exclusive_incident() {
        // "minor delays" appears twice in the summary+description text, so
        // the keyword classifier alone reaches MinorDelays — the Python
        // original's `severity_hint="minor"` was likewise redundant here.
        let lines = load_all_lines();
        let inc = incident(
            "SWR-4",
            "Minor delays on Alton line",
            "A power supply problem at Alton is causing minor delays.",
            &["SW"],
            &["AON"],
        );
        let reports = aggregate_with_defaults(&lines, &[inc]);
        assert_eq!(reports["swr-alton"].worst_severity(), Severity::MinorDelays);
        assert_eq!(reports["swr-south-west-main"].worst_severity(), Severity::GoodService);
        assert_eq!(reports["swr-portsmouth-direct"].worst_severity(), Severity::GoodService);
    }

    #[test]
    fn operator_only_match_is_demoted_to_minor() {
        let lines = load_all_lines();
        let inc = incident(
            "OP-1",
            "SWR services suspended",
            "No service on SWR following an earlier incident.",
            &["SW"],
            &[], // no stations -> operator-only
        );
        let reports = aggregate_with_defaults(&lines, &[inc]);
        for line_id in ["swr-south-west-main", "swr-portsmouth-direct", "swr-alton"] {
            assert_eq!(
                reports[line_id].worst_severity(),
                Severity::MinorDelays,
                "{line_id} should be capped at Minor Delays"
            );
        }
    }

    #[test]
    fn no_incident_no_samples_yields_good_service() {
        let lines = load_all_lines();
        let reports = aggregate_with_defaults(&lines, &[]);
        for report in reports.values() {
            assert_eq!(report.worst_severity(), Severity::GoodService);
        }
    }

    #[test]
    fn validity_for_output_uses_now_when_no_periods_given() {
        let period = validity_for_output(&[]);
        assert!(period.is_now);
        assert!(period.to_date.is_none());
    }

    #[test]
    fn validity_for_output_picks_the_currently_active_period() {
        let now = Utc::now();
        let expired = ValidityPeriod {
            from_date: now - chrono::Duration::days(2),
            to_date: Some(now - chrono::Duration::days(1)),
            is_now: false,
        };
        let active = ValidityPeriod {
            from_date: now - chrono::Duration::hours(1),
            to_date: None,
            is_now: true,
        };
        let chosen = validity_for_output(&[expired.clone(), active.clone()]);
        assert_eq!(chosen.from_date, active.from_date);
    }

    #[test]
    fn validity_for_output_falls_back_to_first_when_none_are_active() {
        let now = Utc::now();
        let future = ValidityPeriod {
            from_date: now + chrono::Duration::days(1),
            to_date: None,
            is_now: false,
        };
        let chosen = validity_for_output(&[future.clone()]);
        assert_eq!(chosen.from_date, future.from_date);
    }

    // --- Inference-path tests ---
    //
    // The Python test suite (tests/test_matcher.py) never exercises
    // `_infer_from_samples`/`_belongs_to_line`/`_classify` at all — every
    // existing Python test is incident-path or matcher-only. There is no
    // Python original to port here, so these are new tests covering
    // logic that was faithfully ported but never had test coverage
    // upstream. Found and added during this plan's self-review.

    fn departure(destination_crs: &str, delay_minutes: i32, is_cancelled: bool) -> StationDeparture {
        StationDeparture {
            service_id: "svc".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "10:00".to_string(),
            estimated: "10:00".to_string(),
            is_cancelled,
            delay_minutes,
            cancel_reason: if is_cancelled { Some("fault".to_string()) } else { None },
            delay_reason: if !is_cancelled && delay_minutes > 0 { Some("signal failure".to_string()) } else { None },
            headcode: None,
        }
    }

    #[test]
    fn belongs_to_line_filters_by_operator() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let matching = departure("AON", 0, false);
        let wrong_operator = StationDeparture { operator: "XX".to_string(), ..matching.clone() };
        assert!(belongs_to_line(&matching, alton));
        assert!(!belongs_to_line(&wrong_operator, alton));
    }

    #[test]
    fn belongs_to_line_filters_by_destination_crs() {
        // swr-alton.toml: destination_crs_filter = ["AON", "BTL", "FRM", "AHT"]
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let matching = departure("AON", 0, false);
        let wrong_destination = StationDeparture { destination_crs: "WOK".to_string(), ..matching.clone() };
        assert!(belongs_to_line(&matching, alton));
        assert!(!belongs_to_line(&wrong_destination, alton));
    }

    #[test]
    fn infer_from_samples_returns_none_below_min_sample_size() {
        // swr-alton.toml: sample_stations = ["AHT", "FRM", "AON"]
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let mut samples = HashMap::new();
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                // Only 2 relevant departures, below the default min_sample_size of 3.
                departures: vec![departure("AON", 0, false), departure("AON", 0, false)],
            },
        );
        assert!(infer_from_samples(alton, &samples, &defaults).is_none());
    }

    #[test]
    fn infer_from_samples_classifies_severe_delays() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let mut samples = HashMap::new();
        // 4 departures, 3 delayed >= 5 minutes -> 75% delay rate, above the
        // default severe_delays_pct of 0.50.
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![
                    departure("AON", 10, false),
                    departure("AON", 12, false),
                    departure("AON", 8, false),
                    departure("AON", 0, false),
                ],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults).expect("should classify");
        assert_eq!(status.severity, Severity::SevereDelays);
        assert_eq!(status.data_quality, DataQuality::LdbwsInferred);
    }

    #[test]
    fn infer_from_samples_returns_good_service_when_below_thresholds() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let mut samples = HashMap::new();
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![departure("AON", 0, false), departure("AON", 0, false), departure("AON", 0, false)],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults).expect("should still classify (Good Service)");
        assert_eq!(status.severity, Severity::GoodService);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail first (TDD RED for the new adaptation tests), then pass**

Since `aggregation.rs` is being written whole in Step 1 rather than
incrementally, do this instead: after Step 1's implementation is in place,
run the tests directly to confirm GREEN (there's no separate RED phase for
a from-scratch module — the RED/GREEN cycle already happened conceptually
by hand-tracing each test against the Python source during planning, as
recorded in each test's comment above).

Run: `cargo test -p aggregator`
Expected: `test result: ok. 20 passed; 0 failed` (8 from Task 2 + 12 new
here: 3 ported `aggregate()`-level tests, 1 new no-incident test, 3 new
`validity_for_output` tests, 2 `belongs_to_line` tests, 3
`infer_from_samples` tests).

- [ ] **Step 4: Commit**

```bash
git add crates/aggregator/src/aggregation.rs crates/aggregator/src/main.rs
git commit -m "Port the aggregation logic: incident-driven statuses, LDBWS inference, validity-period selection"
```

---

## Task 4: `aggregator` crate — DB wiring, main loop, Dockerfile

**Files:**
- Create: `crates/aggregator/src/config.rs`
- Create: `crates/aggregator/src/queries.rs`
- Modify: `crates/aggregator/src/main.rs` (replace the placeholder)
- Create: `docker/aggregator.Dockerfile`

**Interfaces:**
- Consumes: `aggregation::aggregate`, `segments::SegmentRegistry`,
  `common::{Defaults, LineDefinition, IncidentMessage, StationSample, LineStatusReport}`.
- Produces: the `aggregator` binary; no other crate depends on it.

- [ ] **Step 1: Write `config.rs`**

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use common::LineDefinition;

fn parse_lines(path: &str) -> Result<Vec<LineDefinition>> {
    LineDefinition::from_dir(&PathBuf::from(path))
}

/// CLI/env configuration for the `aggregator` service.
#[derive(Debug, Parser)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    /// Directory of line-catalogue TOML files, loaded once at startup.
    /// Same default as the `api` crate's `--lines-dir`, since both load
    /// the same catalogue independently (see the plan's Global
    /// Constraints on keeping this behind a narrow, swappable interface).
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: Vec<LineDefinition>,

    /// DESIGN.md §4 target cadence is "every 30-60s"; 60 is the
    /// conservative end.
    #[arg(long, env, default_value_t = 60)]
    pub poll_interval_secs: u64,

    /// How long to keep `line_status_history` rows before pruning them.
    #[arg(long, env, default_value_t = 7)]
    pub history_retention_days: i64,
}
```

- [ ] **Step 2: Write `queries.rs`**

```rust
//! Read/write query functions the aggregator's own poll loop uses. Reads
//! `incidents`/`station_samples` (written by the four existing pollers);
//! writes `line_status`/`line_status_history` (read by the api crate's
//! new endpoints, Task 5).

use std::collections::HashMap;

use anyhow::Result;
use common::{IncidentMessage, LineStatusReport, StationSample};
use sqlx::{PgPool, Row};

pub async fn load_incidents(pool: &PgPool) -> Result<Vec<IncidentMessage>> {
    let rows = sqlx::query(
        "SELECT incident_id, summary, description, operators, affected_stations, \
                priority, validity_periods, is_planned, is_cleared \
         FROM incidents",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let validity_json: serde_json::Value = row.try_get("validity_periods")?;
            Ok(IncidentMessage {
                incident_id: row.try_get("incident_id")?,
                summary: row.try_get("summary")?,
                description: row.try_get("description")?,
                operators: row.try_get("operators")?,
                affected_stations: row.try_get("affected_stations")?,
                priority: row.try_get("priority")?,
                validity: serde_json::from_value(validity_json)?,
                is_planned: row.try_get("is_planned")?,
                is_cleared: row.try_get("is_cleared")?,
            })
        })
        .collect()
}

pub async fn load_station_samples(pool: &PgPool) -> Result<HashMap<String, StationSample>> {
    let rows = sqlx::query("SELECT crs, polled_at, departures FROM station_samples")
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|row| {
            let crs: String = row.try_get("crs")?;
            let departures_json: serde_json::Value = row.try_get("departures")?;
            let sample = StationSample {
                crs: crs.clone(),
                polled_at: row.try_get("polled_at")?,
                departures: serde_json::from_value(departures_json)?,
            };
            Ok((crs, sample))
        })
        .collect()
}

/// Fetches the currently-stored `statuses` JSON for one line, if any row
/// exists yet.
async fn existing_statuses(pool: &PgPool, line_id: &str) -> Result<Option<serde_json::Value>> {
    let row = sqlx::query("SELECT statuses FROM line_status WHERE line_id = $1")
        .bind(line_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.try_get("statuses")).transpose()?)
}

/// Upserts one line's computed report into `line_status` (always), and
/// inserts a `line_status_history` snapshot only if the statuses actually
/// changed since the last cycle.
pub async fn write_line_status(pool: &PgPool, report: &LineStatusReport) -> Result<()> {
    let statuses_json = serde_json::to_value(&report.statuses)?;

    let changed = match existing_statuses(pool, &report.id).await? {
        None => true,
        Some(existing) => existing != statuses_json,
    };

    sqlx::query(
        r#"
        INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        ON CONFLICT (line_id) DO UPDATE SET
            name        = EXCLUDED.name,
            mode_name   = EXCLUDED.mode_name,
            operators   = EXCLUDED.operators,
            statuses    = EXCLUDED.statuses,
            computed_at = NOW()
        "#,
    )
    .bind(&report.id)
    .bind(&report.name)
    .bind(&report.mode_name)
    .bind(&report.operators)
    .bind(&statuses_json)
    .execute(pool)
    .await?;

    if changed {
        sqlx::query(
            "INSERT INTO line_status_history (line_id, statuses, computed_at) VALUES ($1, $2, NOW())",
        )
        .bind(&report.id)
        .bind(&statuses_json)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Deletes `line_status_history` rows older than `retention_days`.
pub async fn prune_history(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM line_status_history WHERE computed_at < NOW() - ($1 || ' days')::interval",
    )
    .bind(retention_days.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 3: Write the real `main.rs`**

Replace the Task 2 placeholder entirely:
```rust
//! `aggregator`: periodically recomputes every line's status from
//! incidents + LDBWS samples and writes it to `line_status`/
//! `line_status_history`. See
//! `docs/superpowers/specs/2026-07-06-aggregator-read-api-design.md` for
//! the full design.

mod aggregation;
mod config;
mod matcher;
mod queries;
mod segments;

use std::collections::HashMap;
use std::time::Duration;

use clap::Parser;
use common::{Defaults, LineDefinition};
use config::Config;
use segments::SegmentRegistry;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    let lines: HashMap<String, LineDefinition> =
        config.lines.iter().map(|l| (l.id.clone(), l.clone())).collect();
    tracing::info!(count = lines.len(), "loaded line catalogue");

    let registry = SegmentRegistry::new(&lines);
    let defaults = Defaults::default();

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        interval.tick().await;

        if let Err(err) = run_cycle(&pool, &lines, &registry, &defaults, config.history_retention_days).await {
            tracing::error!(error = ?err, "aggregation cycle failed; will retry next interval");
        }
    }
}

async fn run_cycle(
    pool: &sqlx::PgPool,
    lines: &HashMap<String, LineDefinition>,
    registry: &SegmentRegistry,
    defaults: &Defaults,
    retention_days: i64,
) -> anyhow::Result<()> {
    let incidents = queries::load_incidents(pool).await?;
    let samples = queries::load_station_samples(pool).await?;

    let reports = aggregation::aggregate(lines, &incidents, &samples, registry, defaults);

    for report in reports.values() {
        queries::write_line_status(pool, report).await?;
    }

    let pruned = queries::prune_history(pool, retention_days).await?;
    tracing::info!(
        lines = reports.len(),
        incidents = incidents.len(),
        pruned_history_rows = pruned,
        "aggregation cycle complete"
    );

    Ok(())
}
```

- [ ] **Step 4: Verify build, tests, clippy**

Run: `cargo build -p aggregator && cargo test -p aggregator && cargo clippy -p aggregator --all-targets -- -D warnings`
Expected: clean build, `test result: ok. 20 passed; 0 failed` (unchanged
from Task 3 — this task adds no new pure-logic tests, only DB/main
wiring), no clippy warnings.

- [ ] **Step 5: Manual end-to-end verification against a real Postgres**

```bash
docker run -d --rm --name nrstatus-agg-verify -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=nr_status -p 55434:5432 postgres:16
sleep 3
DATABASE_URL=postgres://postgres:postgres@localhost:55434/nr_status cargo run -p api &
sleep 3
kill %1  # api only needed to run migrations; stop it once they've applied

DATABASE_URL=postgres://postgres:postgres@localhost:55434/nr_status \
LINES_DIR=lines \
POLL_INTERVAL_SECS=5 \
cargo run -p aggregator &
sleep 8

docker exec nrstatus-agg-verify psql -U postgres -d nr_status -c \
  "SELECT line_id, mode_name, statuses->0->>'severity' FROM line_status ORDER BY line_id;"
```
Expected: one row per line in `lines/*.toml` (7 lines total: `wcml`,
`thameslink-core`, `swr-south-west-main`, `swr-portsmouth-direct`,
`swr-alton`, plus any others present), each with `mode_name =
national-rail` and a `severity` value of `10` (Good Service — no incidents
or samples exist in this throwaway DB, so every line falls through to
`good_service()`).

Clean up:
```bash
kill %1
docker stop nrstatus-agg-verify
```

- [ ] **Step 6: Write the Dockerfile**

```dockerfile
# Multi-stage build for the `aggregator` service.
#
# Builder pin: matches `api` at rust:1.88-bookworm (this crate pulls in
# sqlx-postgres, whose transitive `home` crate requires 1.88+, same as
# `api` — confirmed by `api`'s own Dockerfile comment).
#
# Build from the repo root so the workspace's Cargo.toml/Cargo.lock and
# crates/common path dependency are all in the build context:
#   docker build -f docker/aggregator.Dockerfile .
FROM rust:1.88-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin aggregator

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin aggregator

COPY --from=builder /app/target/release/aggregator /usr/local/bin/aggregator
COPY --chown=aggregator:aggregator lines/ /app/lines/

USER aggregator

ENTRYPOINT ["/usr/local/bin/aggregator"]
```

- [ ] **Step 7: Verify the image builds and runs as non-root**

```bash
docker build -f docker/aggregator.Dockerfile -t aggregator-verify .
docker run --rm --entrypoint id aggregator-verify
```
Expected: build succeeds; output contains `uid=999(aggregator)` (or
similar non-zero uid), not `uid=0(root)`.

- [ ] **Step 8: Commit**

```bash
git add crates/aggregator/src/config.rs crates/aggregator/src/queries.rs \
        crates/aggregator/src/main.rs docker/aggregator.Dockerfile Cargo.lock
git commit -m "Wire the aggregator's DB access, main poll loop, and Dockerfile"
```

---

## Task 5: Read endpoints on `api`

**Files:**
- Create: `crates/api/src/render.rs`
- Create: `crates/api/src/routes/line_status.rs`
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/mod.rs`
- Modify: `crates/api/src/main.rs`

**Interfaces:**
- Consumes: `common::{LineStatusReport, LineStatus, Severity, DataQuality}`, `app::{App, Router}`.
- Produces: 4 new routes on `public_router()`.

- [ ] **Step 1: Write `render.rs`** (port of `src/render.py`, TDD)

```rust
//! Render `common::LineStatusReport`/`LineStatus` as TfL-shaped JSON.
//! Ported from `src/render.py`. Deliberately independent of any
//! `#[serde(rename)]` on the stored types — the internal storage
//! representation (however `LineStatus` happens to serialize by default)
//! and the public TfL response shape are different concerns; this module
//! is the only place that knows the public shape, exactly like the
//! Python original builds its response dict by hand rather than relying
//! on dataclass field names.

use common::{LineStatus, LineStatusReport, Severity};
use serde_json::{Value, json};

pub fn to_tfl_shape(report: &LineStatusReport, detail: bool) -> Value {
    json!({
        "$type": "NRStatus.LineStatusReport",
        "id": report.id,
        "name": report.name,
        "modeName": report.mode_name,
        "operators": report.operators,
        "lineStatuses": report.statuses.iter().map(|s| status_to_json(s, detail)).collect::<Vec<_>>(),
    })
}

fn status_to_json(status: &LineStatus, detail: bool) -> Value {
    let mut out = json!({
        "statusSeverity": status.severity as i32,
        "statusSeverityDescription": severity_description(status.severity),
        "reason": status.reason,
        "dataQuality": status.data_quality,
        "validityPeriods": [
            {
                "fromDate": status.validity.from_date.to_rfc3339(),
                "toDate": status.validity.to_date.map(|d| d.to_rfc3339()),
                "isNow": status.validity.is_now,
            }
        ],
    });

    if detail {
        if let Some(disruption) = &status.disruption {
            out["disruption"] = json!({
                "category": disruption.category,
                "description": disruption.description,
                "affectedStops": disruption.affected_stops,
                "affectedRoutes": disruption.affected_routes.iter().map(|r| json!({
                    "from": r.from_crs,
                    "to": r.to_crs,
                })).collect::<Vec<_>>(),
                "source": disruption.source,
            });
        }
    }

    out
}

fn severity_description(severity: Severity) -> &'static str {
    severity.description()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::{DataQuality, Disruption, ValidityPeriod};

    fn sample_report(disruption: Option<Disruption>) -> LineStatusReport {
        LineStatusReport {
            id: "wcml".to_string(),
            name: "West Coast Main Line".to_string(),
            mode_name: "national-rail".to_string(),
            operators: vec!["AW".to_string()],
            statuses: vec![LineStatus {
                severity: Severity::MinorDelays,
                reason: "Signal failure".to_string(),
                validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
                disruption,
                data_quality: DataQuality::Knowledgebase,
            }],
        }
    }

    #[test]
    fn renders_top_level_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
        assert_eq!(json["$type"], "NRStatus.LineStatusReport");
        assert_eq!(json["id"], "wcml");
        assert_eq!(json["name"], "West Coast Main Line");
        assert_eq!(json["modeName"], "national-rail");
        assert_eq!(json["operators"][0], "AW");
    }

    #[test]
    fn renders_status_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
        let status = &json["lineStatuses"][0];
        assert_eq!(status["statusSeverity"], 9);
        assert_eq!(status["statusSeverityDescription"], "Minor Delays");
        assert_eq!(status["reason"], "Signal failure");
        assert_eq!(status["dataQuality"], "knowledgebase");
        assert_eq!(status["validityPeriods"][0]["isNow"], true);
        assert!(status["validityPeriods"][0]["toDate"].is_null());
    }

    #[test]
    fn disruption_omitted_without_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![],
            source: Some("knowledgebase-incident-1".to_string()),
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, false);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }

    #[test]
    fn disruption_included_with_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![common::AffectedRoute { from_crs: "WAT".to_string(), to_crs: "WOK".to_string() }],
            source: Some("knowledgebase-incident-1".to_string()),
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, true);
        let d = &json["lineStatuses"][0]["disruption"];
        assert_eq!(d["category"], "RealTime");
        assert_eq!(d["description"], "Signal failure at Woking");
        assert_eq!(d["affectedStops"][0], "WOK");
        assert_eq!(d["affectedRoutes"][0]["from"], "WAT");
        assert_eq!(d["affectedRoutes"][0]["to"], "WOK");
        assert_eq!(d["source"], "knowledgebase-incident-1");
    }

    #[test]
    fn no_disruption_present_even_with_detail_flag() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, true);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }
}
```

This requires `Severity::description(self) -> &'static str` to exist on
`common::Severity` — it already does (`crates/common/src/lib.rs`, added in
the very first workspace-conversion plan). No change needed there.

- [ ] **Step 2: Run the render tests**

Run: `cargo test -p api render`
Expected: `test result: ok. 5 passed; 0 failed`.

- [ ] **Step 3: Add query functions to `crates/api/src/data/queries.rs`**

Append (the file already imports `IncidentMessage, StationReference,
StationSample, TocReference` from `common` — no new `use` line is needed
here since none of the code below references a `common` type by bare
name outside what's already in scope via `common::LineStatus` /
`serde_json::Value`'s full paths):
```rust
/// One row from `line_status`, deserialized into the shape `render.rs`
/// consumes.
pub struct LineStatusRow {
    pub id: String,
    pub name: String,
    pub mode_name: String,
    pub operators: Vec<String>,
    pub statuses: Vec<common::LineStatus>,
}

fn row_to_report(row: sqlx::postgres::PgRow) -> Result<LineStatusRow> {
    use sqlx::Row;
    let statuses_json: serde_json::Value = row.try_get("statuses")?;
    Ok(LineStatusRow {
        id: row.try_get("line_id")?,
        name: row.try_get("name")?,
        mode_name: row.try_get("mode_name")?,
        operators: row.try_get("operators")?,
        statuses: serde_json::from_value(statuses_json)?,
    })
}

pub async fn line_status_for_mode(pool: &PgPool, mode: &str) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query("SELECT line_id, name, mode_name, operators, statuses FROM line_status WHERE mode_name = $1")
        .bind(mode)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_report).collect()
}

pub async fn line_status_for_ids(pool: &PgPool, ids: &[String]) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query("SELECT line_id, name, mode_name, operators, statuses FROM line_status WHERE line_id = ANY($1)")
        .bind(ids)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_to_report).collect()
}

pub async fn line_status_history_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<(chrono::DateTime<chrono::Utc>, Vec<common::LineStatus>)>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT statuses, computed_at FROM line_status_history \
         WHERE line_id = $1 AND computed_at BETWEEN $2 AND $3 ORDER BY computed_at",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let statuses_json: serde_json::Value = row.try_get("statuses")?;
            let computed_at: chrono::DateTime<chrono::Utc> = row.try_get("computed_at")?;
            Ok((computed_at, serde_json::from_value(statuses_json)?))
        })
        .collect()
}
```

- [ ] **Step 4: Write `crates/api/src/routes/line_status.rs`**

```rust
//! The four TfL-shaped read endpoints: `/Line/Mode/{mode}/Status`,
//! `/Line/{ids}/Status`, `/StopPoint/{crs}/Disruption`,
//! `/Line/{id}/Status/{from}/to/{to}`. Mounted on `public_router()` —
//! unauthenticated, matching TfL's own public API.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, Router as AxumRouter};
use chrono::{DateTime, Utc};
use common::{LineStatus, LineStatusReport, Severity};
use serde::Deserialize;
use serde_json::Value;

use crate::app::{App, Router};
use crate::data::queries;
use crate::render::to_tfl_shape;

pub fn router() -> Router {
    AxumRouter::new()
        .route("/Line/Mode/{mode}/Status", axum::routing::get(get_mode_status))
        .route("/Line/{ids}/Status", axum::routing::get(get_line_status))
        .route("/StopPoint/{crs}/Disruption", axum::routing::get(get_stop_point_disruption))
        .route("/Line/{id}/Status/{from}/to/{to}", axum::routing::get(get_line_status_history))
}

#[derive(Debug, Deserialize)]
pub struct DetailQuery {
    #[serde(default)]
    pub detail: bool,
}

fn to_report(row: queries::LineStatusRow) -> LineStatusReport {
    LineStatusReport {
        id: row.id,
        name: row.name,
        mode_name: row.mode_name,
        operators: row.operators,
        statuses: row.statuses,
    }
}

async fn get_mode_status(
    State(app): State<App>,
    Path(mode): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if mode != "national-rail" {
        return Err((StatusCode::BAD_REQUEST, format!("unsupported mode: {mode}")));
    }

    let rows = queries::line_status_for_mode(&app.database, &mode)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter().map(to_report).map(|r| to_tfl_shape(&r, query.detail)).collect(),
    ))
}

async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();

    let rows = queries::line_status_for_ids(&app.database, &ids)
        .await
        .map_err(internal_error)?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no matching line(s): {}", ids.join(","))));
    }

    Ok(Json(
        rows.into_iter().map(to_report).map(|r| to_tfl_shape(&r, query.detail)).collect(),
    ))
}

async fn get_stop_point_disruption(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let matching_line_ids: Vec<String> = app
        .config
        .lines
        .iter()
        .filter(|line| line.has_station(&crs))
        .map(|line| line.id.clone())
        .collect();

    if matching_line_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let rows = queries::line_status_for_ids(&app.database, &matching_line_ids)
        .await
        .map_err(internal_error)?;

    let disruptions: Vec<Value> = rows
        .into_iter()
        .flat_map(|row| {
            let statuses: Vec<LineStatus> = row
                .statuses
                .into_iter()
                .filter(|s| s.severity != Severity::GoodService)
                .collect();
            let report = LineStatusReport {
                id: row.id,
                name: row.name,
                mode_name: row.mode_name,
                operators: row.operators,
                statuses,
            };
            if report.statuses.is_empty() {
                None
            } else {
                Some(to_tfl_shape(&report, true))
            }
        })
        .collect();

    Ok(Json(disruptions))
}

async fn get_line_status_history(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let history = queries::line_status_history_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        history
            .into_iter()
            .map(|(computed_at, statuses)| {
                let report = LineStatusReport {
                    id: id.clone(),
                    name: String::new(),
                    mode_name: String::new(),
                    operators: vec![],
                    statuses,
                };
                let mut json = to_tfl_shape(&report, true);
                json["computedAt"] = Value::String(computed_at.to_rfc3339());
                json
            })
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "line status query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}
```

Note `get_line_status_history` renders with empty `name`/`mode_name`/
`operators` since `line_status_history` doesn't store them (only
`line_id`/`statuses`/`computed_at`) — this matches what's actually
queryable from that table; a client wanting the line's name/mode alongside
history can pair this with `/Line/{id}/Status`.

- [ ] **Step 5: Wire the new router into `public_router()`**

`crates/api/src/routes/mod.rs` currently is:
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
Change to:
```rust
use axum::middleware;
use crate::app::{App, Router};
use crate::auth::require_internal_token;

pub mod health;
pub mod ingest;
pub mod line_status;
pub mod samples;

pub fn public_router() -> Router {
    Router::new().merge(health::router()).merge(line_status::router())
}

pub fn private_router(app: App) -> Router {
    Router::new()
        .merge(ingest::router())
        .merge(samples::router())
        .layer(middleware::from_fn_with_state(app, require_internal_token))
}
```

Add `pub mod render;` to `crates/api/src/main.rs`'s existing `pub mod app;
pub mod auth; pub mod data; pub mod routes;` line, making it `pub mod app;
pub mod auth; pub mod data; pub mod render; pub mod routes;`.

- [ ] **Step 6: Verify the workspace builds and every existing test still passes**

Run: `cargo build --workspace && cargo test --workspace`
Expected: clean build; `api`'s test count goes from 14 to 19 (5 new
`render` tests); every other crate's count unchanged.

- [ ] **Step 7: Manual verification against a real Postgres**

Reuse Task 4's running `aggregator`-populated database (or bring
`postgres` + migrated `api` + one `aggregate()` cycle up again per Task
4's Step 5), then:
```bash
DATABASE_URL=postgres://postgres:postgres@localhost:55434/nr_status \
LINES_DIR=lines \
BIND_URL=127.0.0.1:18082 \
INTERNAL_TOKEN=test-secret-token \
cargo run -p api &
sleep 3

curl -s http://127.0.0.1:18082/Line/Mode/national-rail/Status | head -c 500
echo
curl -s "http://127.0.0.1:18082/Line/wcml,swr-alton/Status?detail=true" | head -c 500
echo
curl -s http://127.0.0.1:18082/StopPoint/WOK/Disruption
echo
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18082/Line/not-a-real-line/Status

kill %1
```
Expected: first two calls return JSON arrays of `NRStatus.LineStatusReport`
objects with `statusSeverity: 10` (Good Service, since no incidents exist
in this throwaway run — confirmed by Task 4's own verification); the
`/StopPoint/WOK/Disruption` call returns `[]` (no disruptions, all
Good Service); the last call returns `404`.

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/render.rs crates/api/src/routes/line_status.rs \
        crates/api/src/data/queries.rs crates/api/src/routes/mod.rs crates/api/src/main.rs
git commit -m "Add the four TfL-shaped read endpoints to the api crate"
```

---

## Task 6: docker-compose wiring + end-to-end verification

**Files:**
- Modify: `docker-compose.yml`
- Modify: `.env.example`

**Interfaces:** none (integration-only task).

- [ ] **Step 1: Add the `aggregator` service to `docker-compose.yml`**

Add this block after the `poller-ldbws:` service and before `volumes:`:
```yaml
  aggregator:
    build:
      context: .
      dockerfile: docker/aggregator.Dockerfile
    restart: unless-stopped
    depends_on:
      postgres:
        condition: service_healthy
    environment:
      # crates/aggregator/src/config.rs: Config
      DATABASE_URL: postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB}
      LINES_DIR: /app/lines
      POLL_INTERVAL_SECS: ${POLL_INTERVAL_SECS_AGGREGATOR:-60}
      HISTORY_RETENTION_DAYS: ${HISTORY_RETENTION_DAYS:-7}
      RUST_LOG: ${RUST_LOG:-info}
```

- [ ] **Step 2: Update `.env.example`**

Add before the final "Shared across all pollers + api" section:
```bash
# ---------------------------------------------------------------------------
# aggregator (crates/aggregator/src/config.rs: Config)
# ---------------------------------------------------------------------------
# Target cadence per DESIGN.md is 30-60s; 60 is the default. Shorten
# temporarily (e.g. to 5) only to verify the aggregation loop locally.
POLL_INTERVAL_SECS_AGGREGATOR=60
# How long line_status_history rows are kept before being pruned.
HISTORY_RETENTION_DAYS=7
```

- [ ] **Step 3: Bring up the full stack**

```bash
docker compose up --build -d
sleep 15
docker compose ps
```
Expected: all 7 services (`postgres`, `api`, `poller-incidents`,
`poller-stations`, `poller-tocs`, `poller-ldbws`, `aggregator`) show as
running/healthy.

- [ ] **Step 4: Confirm the read endpoints work against the live stack**

```bash
source .env
curl -s "http://localhost:${API_HOST_PORT:-8080}/Line/Mode/national-rail/Status" | head -c 300
echo
curl -s -o /dev/null -w '%{http_code}\n' "http://localhost:${API_HOST_PORT:-8080}/Line/wcml/Status"
```
Expected: a JSON array with one entry per line (all `statusSeverity: 10`,
since no real incidents exist without live RDM credentials — expected, not
a defect, same as every prior poller's verification in this project);
second call returns `200`.

- [ ] **Step 5: Confirm `aggregator`'s logs show real cycles running**

```bash
docker compose logs aggregator --tail=20
```
Expected: log lines showing `"loaded line catalogue"` (count matching
`lines/*.toml`'s file count) and repeated `"aggregation cycle complete"`
entries with `lines=<N> incidents=0 pruned_history_rows=0`.

- [ ] **Step 6: Confirm `GET /public/health` still works**

```bash
curl -s "http://localhost:${API_HOST_PORT:-8080}/public/health"
```
Expected: `{"message":"Alive"}`.

- [ ] **Step 7: Tear down**

```bash
docker compose down -v
```

- [ ] **Step 8: Commit**

```bash
git add docker-compose.yml .env.example
git commit -m "Wire the aggregator into docker-compose and add the four read endpoints' local verification"
```

---

## Self-Review Notes (completed during writing, recorded here per skill instructions)

**Spec coverage:** every section of `docs/superpowers/specs/2026-07-06-aggregator-read-api-design.md`
is covered: `Defaults` relocation (Task 1), segments+matcher port (Task 2),
aggregation logic + both adaptations (Task 3), DB wiring/main loop/pruning/
Dockerfile (Task 4), the four read endpoints + render.rs (Task 5),
docker-compose + end-to-end verification (Task 6). The "future direction"
note (moving the line catalogue to Postgres) is explicitly not built,
matching the spec's own "not built now" framing — no task attempts it.

**Placeholder scan:** no TBD/TODO markers. The two "Open questions for the
planning phase" items from the spec (module layout, f64 casting semantics)
are resolved concretely in Task 2 (one file per Python module, `matcher.rs`/
`segments.rs`/`aggregation.rs`) and Task 1 (`as i64`/`as i8` casts, tested
directly in `defaults_tests`).

**Coverage gap found and fixed during self-review:** the spec's testing
section said to port "one test per existing Python test," but
`tests/test_matcher.py` never exercises the inference path
(`_infer_from_samples`/`_belongs_to_line`/`_classify`) at all — every
Python test is incident-path or matcher-only. A literal "port existing
tests" reading would have shipped `infer_from_samples`/`belongs_to_line`
with zero test coverage despite being real, non-trivial ported logic
(threshold classification, three different filter conditions). Fixed by
adding 5 new tests to Task 3 (`belongs_to_line_filters_by_operator`,
`belongs_to_line_filters_by_destination_crs`,
`infer_from_samples_returns_none_below_min_sample_size`,
`infer_from_samples_classifies_severe_delays`,
`infer_from_samples_returns_good_service_when_below_thresholds`), and
corrected the resulting test-count arithmetic error this also surfaced
(Task 2 was mislabeled 6 instead of 8; Task 3/4's running totals updated
to 20).

**Type consistency:** `SegmentRegistry::new`, `lines_affected_by`,
`aggregate`, `thresholds_for`, and `to_tfl_shape` are used with identical
signatures everywhere they appear across Tasks 2-5, cross-checked against
each task's own "Produces"/"Consumes" interface block.
