# Custom Lines + Blended Delay Stats Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users define custom lines (arbitrary station sets) that get real incident-matching and LDBWS-delay-inference status computation, and make sample-derived delay stats available on every line's status, not just ones with no active incident.

**Architecture:** A new `custom_lines` Postgres table holds user-defined lines. Both the `aggregator` and `api` crates, which currently load the static `lines/*.toml` catalogue once at startup, are changed to also fetch `custom_lines` (aggregator: every poll cycle; api: on each relevant request) and merge them in as `LineDefinition`s via a `common::CustomLine -> LineDefinition` conversion — the existing matcher, segment registry, and LDBWS-inference pipeline then treat custom lines identically to catalogue lines with no further changes. Sample-derived stats (`SampleStats`) are computed unconditionally each cycle and attached to a line's status as supplementary data, never overriding incident-reported severity.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8, Postgres), existing workspace crates `common`/`aggregator`/`api`.

## Global Constraints

- Unauthenticated: no auth/ownership on custom lines or their endpoints. No `owner_id` column. (Spec: Non-goals.)
- Custom lines get no `segments`, `match_keywords`, `excluded_keywords`, or `severity_overrides` — plain `StationHit`/`OperatorOnly` incident matching and standard-threshold LDBWS inference only. (Spec: Non-goals.)
- Sample stats (`SampleStats`) never change a line's `severity` when an incident is active — informational only, always attached alongside. (Spec: Blended stats.)
- Custom line ids are always prefixed `custom-` (derived from name via `slugify`), so they can never collide with a static `lines/*.toml` id.
- `sampleStats` in the JSON API response is included whenever present, regardless of the `detail` query flag (unlike `disruption`, which is detail-gated) — the frontend's representative-info block (a later plan) needs it visible in the collapsed/summary view.
- No new Cargo dependencies are needed for this plan.

---

### Task 1: Migration — `custom_lines` table

**Files:**
- Create: `crates/api/migrations/20260709100000_custom_lines.sql`

**Interfaces:**
- Produces: table `custom_lines(id TEXT PK, name TEXT, operators TEXT[], stations TEXT[], headcode_prefixes TEXT[], destination_crs_filter TEXT[], created_at TIMESTAMPTZ)`, read/written by Tasks 4–6.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Custom lines: user-defined lines (arbitrary station sets), stored
-- server-side so the aggregator can run the same incident-matching +
-- LDBWS-inference pipeline on them as the static lines/*.toml catalogue.
--
-- Deliberately simpler than the static catalogue's `LineDefinition`: no
-- per-station segment/tiploc/role, no match_keywords/excluded_keywords/
-- severity_overrides/exclusive_segments — those encode official-line route
-- topology and threshold tuning that doesn't apply to an arbitrary
-- user-picked station set. See
-- docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md
-- for the full rationale.
--
-- No owner/user column: unauthenticated for now, by design (see that
-- spec's Non-goals) — add ownership in the migration that actually adds
-- auth, not speculatively here.
--
-- `stations` is a plain `TEXT[]` of ordered CRS codes rather than the
-- spec's suggested jsonb `[{crs}, ...]`: since a custom-line station has no
-- other per-station data (no tiploc/role/segment, unlike catalogue lines),
-- a flat array achieves the same ordering with less structure. Matches how
-- `incidents.operators`/`affected_stations` already use `TEXT[]` elsewhere
-- in this schema.
-- -------------------------------------------------------------------------

CREATE TABLE custom_lines (
    id                      TEXT        PRIMARY KEY,
    name                    TEXT        NOT NULL,
    operators               TEXT[]      NOT NULL DEFAULT '{}',
    stations                TEXT[]      NOT NULL,
    headcode_prefixes       TEXT[]      NOT NULL DEFAULT '{}',
    destination_crs_filter  TEXT[]      NOT NULL DEFAULT '{}',
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 2: Apply and verify the migration**

The `api` service runs `sqlx::migrate!().run(&app.database)` at startup (`crates/api/src/main.rs`), so restarting it against the docker-compose Postgres applies this automatically. From the repo root:

```bash
docker compose build --no-cache api
docker compose up -d --no-build api
docker compose logs --tail 20 api
```

Expected: no migration errors in the logs, and the container reaches `healthy`. Then confirm the table exists:

```bash
docker compose exec postgres psql -U postgres -d nr_status -c '\d custom_lines'
```

(If the database/user names differ from `nr_status`/`postgres`, check `DATABASE_URL` in `.env` first.)

Expected: column list matching Step 1's `CREATE TABLE`.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260709100000_custom_lines.sql
git commit -m "Add custom_lines table migration"
```

---

### Task 2: `common` — `SampleStats`, `CustomLine`, and the `LineStatus.sample_stats` field

**Files:**
- Modify: `crates/common/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct SampleStats { pub total: usize, pub delayed: usize, pub cancelled: usize, pub avg_delay_minutes: f64 }` (`Debug, Clone, PartialEq, Serialize, Deserialize`)
  - `pub struct CustomLine { pub id: String, pub name: String, pub operators: Vec<String>, pub stations: Vec<String>, pub headcode_prefixes: Vec<String>, pub destination_crs_filter: Vec<String> }` (`Debug, Clone, Serialize, Deserialize`)
  - `impl From<CustomLine> for LineDefinition`
  - `LineStatus.sample_stats: Option<SampleStats>` (new field, `#[serde(default, skip_serializing_if = "Option::is_none")]`)
- Consumes: nothing new (uses existing `LineDefinition`, `Station`, `HashMap`).

- [ ] **Step 1: Write the failing test**

Add to the end of `crates/common/src/lib.rs` (new module, after the existing `defaults_tests` module):

```rust
#[cfg(test)]
mod custom_line_tests {
    use super::*;

    #[test]
    fn custom_line_converts_to_line_definition_with_no_segments_or_keywords() {
        let custom = CustomLine {
            id: "custom-my-commute".to_string(),
            name: "My Commute".to_string(),
            operators: vec!["SW".to_string()],
            stations: vec!["WOK".to_string(), "AON".to_string()],
            headcode_prefixes: vec!["1P".to_string()],
            destination_crs_filter: vec!["AON".to_string()],
        };
        let line: LineDefinition = custom.into();
        assert_eq!(line.id, "custom-my-commute");
        assert_eq!(line.name, "My Commute");
        assert_eq!(line.mode, "national-rail");
        assert_eq!(line.category, "custom");
        assert_eq!(line.operators, vec!["SW".to_string()]);
        assert_eq!(line.stations.len(), 2);
        assert_eq!(line.stations[0].crs, "WOK");
        assert!(line.stations[0].segment.is_none());
        assert_eq!(line.sample_stations, vec!["WOK".to_string(), "AON".to_string()]);
        assert!(line.match_keywords.is_empty());
        assert!(line.severity_overrides.is_empty());
        assert_eq!(line.headcode_prefixes, vec!["1P".to_string()]);
        assert_eq!(line.destination_crs_filter, vec!["AON".to_string()]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p common custom_line_converts_to_line_definition_with_no_segments_or_keywords`
Expected: FAIL to compile — `CustomLine` does not exist.

- [ ] **Step 3: Implement `SampleStats`, `CustomLine`, and the conversion**

In `crates/common/src/lib.rs`, immediately after the `TocReference` struct (which ends around line 314), add:

```rust
/// Sample-derived delay/cancellation stats for a line, computed from LDBWS
/// `StationSample`s independently of whether the line also has an
/// incident-derived status. Informational only — never used to change a
/// `LineStatus.severity` that came from an incident. `avg_delay_minutes`
/// is averaged over non-cancelled ("running") sampled departures only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleStats {
    pub total: usize,
    pub delayed: usize,
    pub cancelled: usize,
    pub avg_delay_minutes: f64,
}

/// A user-defined line (see the `custom_lines` table in the `api` crate).
/// Deliberately a much smaller shape than `LineDefinition` — no segments,
/// match keywords, or severity overrides; those encode official-line route
/// topology and threshold tuning that doesn't apply to an arbitrary
/// user-picked station set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomLine {
    pub id: String,
    pub name: String,
    pub operators: Vec<String>,
    /// Ordered CRS codes. Every station here is also used as an LDBWS
    /// sample station — a custom line has no separate concept of "route
    /// station" vs "station to poll for delay data."
    pub stations: Vec<String>,
    #[serde(default)]
    pub headcode_prefixes: Vec<String>,
    #[serde(default)]
    pub destination_crs_filter: Vec<String>,
}

impl From<CustomLine> for LineDefinition {
    fn from(c: CustomLine) -> Self {
        LineDefinition {
            id: c.id,
            name: c.name,
            mode: "national-rail".to_string(),
            category: "custom".to_string(),
            operators: c.operators,
            stations: c
                .stations
                .iter()
                .map(|crs| Station {
                    crs: crs.clone(),
                    tiploc: None,
                    role: Station::default_role(),
                    segment: None,
                })
                .collect(),
            sample_stations: c.stations,
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: c.destination_crs_filter,
            headcode_prefixes: c.headcode_prefixes,
        }
    }
}
```

Then modify the existing `LineStatus` struct (around line 109) to add the new field:

```rust
/// One status entry on a line. A line may have several simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStatus {
    pub severity: Severity,
    pub reason: String,
    pub validity: ValidityPeriod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disruption: Option<Disruption>,
    #[serde(default)]
    pub data_quality: DataQuality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_stats: Option<SampleStats>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p common custom_line_converts_to_line_definition_with_no_segments_or_keywords`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "Add SampleStats and CustomLine types to common"
```

---

### Task 3: `aggregator` — always compute sample stats, blend into incident-active lines

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs`

**Interfaces:**
- Consumes: `common::SampleStats` (Task 2), existing `LineDefinition`, `StationDeparture`, `StationSample`, `Defaults`, `thresholds_for`.
- Produces: `fn compute_sample_stats(line: &LineDefinition, samples: &HashMap<String, StationSample>, defaults: &Defaults) -> Option<SampleStats>` (private, used by Task 4's `merge_custom_lines` test setup indirectly through `aggregate`).

This task first fixes 3 compile breaks left by Task 2's new `LineStatus.sample_stats` field (in `status_from_incident`, `good_service`, `infer_from_samples`), then adds the blending behavior.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `crates/aggregator/src/aggregation.rs` (after `infer_from_samples_returns_good_service_when_below_thresholds`):

```rust
#[test]
fn sample_stats_are_attached_alongside_an_active_incident_without_changing_severity() {
    let lines = load_all_lines();
    let inc = incident(
        "SWR-5",
        "Minor delays on Alton line",
        "A points failure at Alton is causing minor delays.",
        &["SW"],
        &["AON"],
    );
    let registry = SegmentRegistry::new(&lines);
    let defaults = Defaults::default();
    let mut samples = HashMap::new();
    // 4 departures, 3 delayed >= 5 minutes -> would classify as SevereDelays
    // on its own (75% delay rate, above the 50% severe_delays_pct default),
    // but the incident's MinorDelays severity must still win.
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
    let reports = aggregate(&lines, &[inc], &samples, &registry, &defaults);
    let alton = &reports["swr-alton"];
    assert_eq!(
        alton.worst_severity(),
        Severity::MinorDelays,
        "incident severity must stay authoritative"
    );
    let stats = alton.statuses[0]
        .sample_stats
        .as_ref()
        .expect("sample stats should be attached even though an incident is active");
    assert_eq!(stats.total, 4);
    assert_eq!(stats.delayed, 3);
    assert_eq!(stats.cancelled, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aggregator sample_stats_are_attached_alongside_an_active_incident`
Expected: FAIL to compile — the crate doesn't compile yet because `LineStatus` literals in this file are missing the new `sample_stats` field (from Task 2), and `sample_stats` isn't set anywhere.

- [ ] **Step 3: Implement**

First, add `SampleStats` to the `use common::{...}` import list at the top of the file:

```rust
use common::{
    AffectedRoute, DataQuality, Defaults, Disruption, IncidentMessage, LineDefinition, LineStatus,
    LineStatusReport, SampleStats, Severity, StationDeparture, StationSample, ValidityPeriod,
    thresholds_for,
};
```

Fix the `status_from_incident` literal (around line 95) by adding the new field:

```rust
    LineStatus {
        severity,
        reason,
        validity: validity_for_output(&incident.validity),
        disruption: Some(disruption),
        data_quality: if incident.is_planned { DataQuality::Planned } else { DataQuality::Knowledgebase },
        sample_stats: None,
    }
```

Fix `good_service` (around line 273):

```rust
fn good_service() -> LineStatus {
    LineStatus {
        severity: Severity::GoodService,
        reason: "Good Service".to_string(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: None,
        data_quality: DataQuality::LdbwsInferred,
        sample_stats: None,
    }
}
```

Replace `infer_from_samples` entirely, and add `compute_sample_stats` above it:

```rust
/// Raw sample-derived numbers for a line: how many recently-sampled
/// departures were delayed/cancelled, and by how much on average. Computed
/// independently of whether the line also has an incident-derived status —
/// `aggregate()` attaches the result to a line's status either way.
/// `avg_delay_minutes` is averaged over non-cancelled ("running") sampled
/// departures only.
fn compute_sample_stats(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> Option<SampleStats> {
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
    let running: Vec<&&StationDeparture> = relevant.iter().filter(|d| !d.is_cancelled).collect();
    let avg_delay_minutes = if running.is_empty() {
        0.0
    } else {
        running.iter().map(|d| d.delay_minutes as f64).sum::<f64>() / running.len() as f64
    };

    Some(SampleStats { total, delayed, cancelled, avg_delay_minutes })
}

fn infer_from_samples(
    line: &LineDefinition,
    samples: &HashMap<String, StationSample>,
    defaults: &Defaults,
) -> Option<LineStatus> {
    let stats = compute_sample_stats(line, samples, defaults)?;
    let thresholds = thresholds_for(defaults, &line.severity_overrides);

    let cancel_rate = stats.cancelled as f64 / stats.total as f64;
    let delay_rate = stats.delayed as f64 / stats.total as f64;

    let (severity, mut reason) =
        classify(cancel_rate, delay_rate, &thresholds, stats.total, stats.cancelled, stats.delayed);
    if severity == Severity::GoodService {
        let mut status = good_service();
        status.sample_stats = Some(stats);
        return Some(status);
    }

    // `compute_sample_stats` only returns aggregate counts, not the raw
    // departures, so the "most cited reason" text below re-derives its own
    // small filtered view. Cheap: a handful of departures per line per
    // cycle, and keeps `compute_sample_stats` focused on just the numbers.
    let relevant: Vec<&StationDeparture> = line
        .sample_stations
        .iter()
        .filter_map(|crs| samples.get(crs))
        .flat_map(|sample| sample.departures.iter())
        .filter(|dep| belongs_to_line(dep, line))
        .collect();
    let reasons: Vec<&str> = relevant
        .iter()
        .filter_map(|d| d.delay_reason.as_deref().or(d.cancel_reason.as_deref()))
        .collect();
    if let Some(most_common) = most_common(&reasons) {
        reason.push_str(&format!(" (most cited: {most_common})"));
    }

    // `samples` is a fresh `HashMap` every poll cycle with a randomized
    // per-process hash seed, so its iteration order is not stable across
    // cycles even for identical input. Sorting here makes the serialized
    // `affected_stops` array deterministic, which `normalize_for_diff`
    // (queries.rs) relies on to avoid writing spurious `line_status_history`
    // rows when nothing has actually changed.
    let mut affected_stops: Vec<String> = samples.keys().cloned().collect();
    affected_stops.sort();

    Some(LineStatus {
        severity,
        reason: reason.clone(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: Some(Disruption {
            category: "RealTime".to_string(),
            description: reason,
            affected_stops,
            affected_routes: vec![],
            source: Some("ldbws-sampling".to_string()),
        }),
        data_quality: DataQuality::LdbwsInferred,
        sample_stats: Some(stats),
    })
}
```

Finally, replace the `aggregate` function's Layer 2 loop:

```rust
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

    // Layer 2: sample-derived stats. Always computed for every line. Used
    // as the status itself when a line has no incident-derived status
    // (unchanged behavior); attached as supplementary `sample_stats` on top
    // of the incident-derived status(es) otherwise, never overriding their
    // severity — incident-reported severity stays authoritative.
    for line in lines.values() {
        let report = reports.get_mut(&line.id).unwrap();
        if report.statuses.is_empty() {
            let inferred = infer_from_samples(line, samples, defaults);
            report.statuses.push(inferred.unwrap_or_else(good_service));
            continue;
        }
        if let Some(stats) = compute_sample_stats(line, samples, defaults) {
            for status in &mut report.statuses {
                status.sample_stats = Some(stats.clone());
            }
        }
    }

    reports
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aggregator`
Expected: PASS — all existing tests plus the new one (existing tests are unaffected since no-incident behavior is unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/aggregator/src/aggregation.rs
git commit -m "Compute sample stats for every line, blend into incident-active statuses"
```

---

### Task 4: `aggregator` — load and merge custom lines every poll cycle

**Files:**
- Modify: `crates/aggregator/src/aggregation.rs` (add `merge_custom_lines`)
- Modify: `crates/aggregator/src/queries.rs` (add `load_custom_lines`)
- Modify: `crates/aggregator/src/main.rs` (fetch + merge each cycle)

**Interfaces:**
- Consumes: `common::CustomLine` (Task 2), `common::LineDefinition`.
- Produces: `pub fn merge_custom_lines(static_lines: &HashMap<String, LineDefinition>, custom_lines: Vec<CustomLine>) -> HashMap<String, LineDefinition>` (aggregation.rs); `pub async fn load_custom_lines(pool: &PgPool) -> Result<Vec<common::CustomLine>>` and `pub async fn prune_removed_lines(pool: &PgPool, current_line_ids: &[String]) -> Result<u64>` (queries.rs).

- [ ] **Step 1: Write the failing test**

Add to `crates/aggregator/src/aggregation.rs`'s `tests` module:

```rust
#[test]
fn merge_custom_lines_adds_custom_without_touching_static() {
    let lines = load_all_lines();
    let static_count = lines.len();
    let custom = vec![CustomLine {
        id: "custom-my-commute".to_string(),
        name: "My Commute".to_string(),
        operators: vec!["SW".to_string()],
        stations: vec!["WOK".to_string(), "AON".to_string()],
        headcode_prefixes: vec![],
        destination_crs_filter: vec![],
    }];
    let merged = merge_custom_lines(&lines, custom);
    assert_eq!(merged.len(), static_count + 1);
    assert!(merged.contains_key("swr-alton"));
    assert_eq!(merged["custom-my-commute"].name, "My Commute");
    assert_eq!(merged["custom-my-commute"].category, "custom");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aggregator merge_custom_lines_adds_custom_without_touching_static`
Expected: FAIL to compile — `merge_custom_lines` and `CustomLine` (unimported) don't exist yet in this file's scope.

- [ ] **Step 3: Implement**

Add `CustomLine` to the `use common::{...}` import in `crates/aggregator/src/aggregation.rs`:

```rust
use common::{
    AffectedRoute, CustomLine, DataQuality, Defaults, Disruption, IncidentMessage, LineDefinition,
    LineStatus, LineStatusReport, SampleStats, Severity, StationDeparture, StationSample,
    ValidityPeriod, thresholds_for,
};
```

Add this function above `pub fn aggregate`:

```rust
/// Merges DB-stored custom lines into the static catalogue, converting
/// each into a `LineDefinition` (see `common::CustomLine`'s `From` impl) so
/// the rest of the pipeline — matcher, segment registry, LDBWS inference —
/// treats them identically to catalogue lines. Re-run every poll cycle
/// (`main.rs`) since custom lines can be created or deleted at any time,
/// unlike the static catalogue which is fixed at process startup.
pub fn merge_custom_lines(
    static_lines: &HashMap<String, LineDefinition>,
    custom_lines: Vec<CustomLine>,
) -> HashMap<String, LineDefinition> {
    let mut merged = static_lines.clone();
    for custom in custom_lines {
        merged.insert(custom.id.clone(), custom.into());
    }
    merged
}
```

In `crates/aggregator/src/queries.rs`, add (after `load_station_samples`):

```rust
pub async fn load_custom_lines(pool: &PgPool) -> Result<Vec<common::CustomLine>> {
    let rows = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(common::CustomLine {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                operators: row.try_get("operators")?,
                stations: row.try_get("stations")?,
                headcode_prefixes: row.try_get("headcode_prefixes")?,
                destination_crs_filter: row.try_get("destination_crs_filter")?,
            })
        })
        .collect()
}
```

Also add (after `load_custom_lines`), so a deleted custom line's `line_status` row doesn't linger forever — without this, `write_line_status` only ever upserts rows for lines that currently exist, it never removes a row for a line that stopped existing, so `GET /Line/{id}/Status` would keep returning increasingly stale data indefinitely after a custom line is deleted instead of 404ing:

```rust
/// Deletes `line_status` rows for any `line_id` not in `current_line_ids`.
/// Called every cycle with the freshly-merged static+custom line set, so a
/// deleted custom line's last-known status is removed on the next cycle
/// rather than lingering forever (custom lines are the only way a line can
/// disappear between cycles — the static catalogue is fixed for the
/// process's lifetime).
pub async fn prune_removed_lines(pool: &PgPool, current_line_ids: &[String]) -> Result<u64> {
    let result = sqlx::query("DELETE FROM line_status WHERE NOT (line_id = ANY($1))")
        .bind(current_line_ids)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
```

Replace `crates/aggregator/src/main.rs` entirely with:

```rust
//! `aggregator`: periodically recomputes every line's status from
//! incidents + LDBWS samples and writes it to `line_status`/
//! `line_status_history`. See
//! `docs/superpowers/specs/2026-07-06-aggregator-read-api-design.md` for
//! the original design, and
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`
//! for the custom-lines addition.

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

    let static_lines: HashMap<String, LineDefinition> =
        config.lines.iter().map(|l| (l.id.clone(), l.clone())).collect();
    tracing::info!(count = static_lines.len(), "loaded static line catalogue");

    let defaults = Defaults::default();

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        interval.tick().await;

        if let Err(err) = run_cycle(&pool, &static_lines, &defaults, config.history_retention_days).await {
            tracing::error!(error = ?err, "aggregation cycle failed; will retry next interval");
        }
    }
}

async fn run_cycle(
    pool: &sqlx::PgPool,
    static_lines: &HashMap<String, LineDefinition>,
    defaults: &Defaults,
    retention_days: i64,
) -> anyhow::Result<()> {
    let custom_lines = queries::load_custom_lines(pool).await?;
    let lines = aggregation::merge_custom_lines(static_lines, custom_lines);
    let registry = SegmentRegistry::new(&lines);

    let incidents = queries::load_incidents(pool).await?;
    let samples = queries::load_station_samples(pool).await?;

    let reports = aggregation::aggregate(&lines, &incidents, &samples, &registry, defaults);

    for report in reports.values() {
        queries::write_line_status(pool, report).await?;
    }

    let current_line_ids: Vec<String> = lines.keys().cloned().collect();
    let removed = queries::prune_removed_lines(pool, &current_line_ids).await?;

    let pruned = queries::prune_history(pool, retention_days).await?;
    tracing::info!(
        lines = reports.len(),
        incidents = incidents.len(),
        removed_lines = removed,
        pruned_history_rows = pruned,
        "aggregation cycle complete"
    );

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo build -p aggregator && cargo test -p aggregator`
Expected: builds clean, all tests (including the new one) PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/aggregator/src/aggregation.rs crates/aggregator/src/queries.rs crates/aggregator/src/main.rs
git commit -m "Load and merge custom lines into the aggregator's line catalogue each cycle"
```

---

### Task 5: `api` — custom-lines CRUD queries

**Files:**
- Create: `crates/api/src/data/custom_lines.rs`
- Modify: `crates/api/src/data/mod.rs`

**Interfaces:**
- Consumes: `common::CustomLine` (Task 2).
- Produces:
  - `pub fn slugify(name: &str) -> String`
  - `pub struct NewCustomLine { pub name: String, pub operators: Vec<String>, pub stations: Vec<String>, pub headcode_prefixes: Vec<String>, pub destination_crs_filter: Vec<String> }`
  - `pub async fn list_custom_lines(pool: &PgPool) -> Result<Vec<common::CustomLine>>`
  - `pub async fn insert_custom_line(pool: &PgPool, new: NewCustomLine) -> Result<common::CustomLine>`
  - `pub async fn delete_custom_line(pool: &PgPool, id: &str) -> Result<bool>`
  - All consumed by Task 6's route handlers and Task 7's `/sample-stations` handler.

- [ ] **Step 1: Write the failing test**

Create `crates/api/src/data/custom_lines.rs` with just the test module first:

```rust
//! CRUD queries for user-defined custom lines (`custom_lines` table). See
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes_punctuation() {
        assert_eq!(slugify("My Commute"), "custom-my-commute");
    }

    #[test]
    fn slugify_collapses_runs_of_punctuation() {
        assert_eq!(slugify("Woking -> Alton!!"), "custom-woking-alton");
    }

    #[test]
    fn slugify_trims_trailing_punctuation() {
        assert_eq!(slugify("Trailing---"), "custom-trailing");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api slugify`
Expected: FAIL to compile — `slugify` doesn't exist yet.

- [ ] **Step 3: Implement**

Replace the file's contents with:

```rust
//! CRUD queries for user-defined custom lines (`custom_lines` table). See
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`.

use anyhow::Result;
use common::CustomLine;
use sqlx::{PgPool, Row};

pub struct NewCustomLine {
    pub name: String,
    pub operators: Vec<String>,
    pub stations: Vec<String>,
    pub headcode_prefixes: Vec<String>,
    pub destination_crs_filter: Vec<String>,
}

/// Turns a line name into a stable, URL-safe id: lowercase, non-alphanumeric
/// runs collapsed to a single `-`, leading/trailing `-` trimmed, prefixed
/// `custom-` so it can never collide with a static `lines/*.toml` id (none
/// of which start with `custom-`).
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = true; // suppresses a leading dash
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    format!("custom-{slug}")
}

pub async fn list_custom_lines(pool: &PgPool) -> Result<Vec<CustomLine>> {
    let rows = sqlx::query(
        "SELECT id, name, operators, stations, headcode_prefixes, destination_crs_filter \
         FROM custom_lines ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(CustomLine {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                operators: row.try_get("operators")?,
                stations: row.try_get("stations")?,
                headcode_prefixes: row.try_get("headcode_prefixes")?,
                destination_crs_filter: row.try_get("destination_crs_filter")?,
            })
        })
        .collect()
}

/// Inserts a new custom line, deriving its id from `new.name` via
/// [`slugify`]. On a slug collision (another custom line already has that
/// id — e.g. two lines both named "My Commute"), retries with `-2`, `-3`,
/// ... appended until an unused id is found.
pub async fn insert_custom_line(pool: &PgPool, new: NewCustomLine) -> Result<CustomLine> {
    let base_id = slugify(&new.name);
    let mut id = base_id.clone();
    let mut suffix = 2;
    loop {
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM custom_lines WHERE id = $1")
            .bind(&id)
            .fetch_optional(pool)
            .await?;
        if existing.is_none() {
            break;
        }
        id = format!("{base_id}-{suffix}");
        suffix += 1;
    }

    sqlx::query(
        r#"
        INSERT INTO custom_lines (id, name, operators, stations, headcode_prefixes, destination_crs_filter, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        "#,
    )
    .bind(&id)
    .bind(&new.name)
    .bind(&new.operators)
    .bind(&new.stations)
    .bind(&new.headcode_prefixes)
    .bind(&new.destination_crs_filter)
    .execute(pool)
    .await?;

    Ok(CustomLine {
        id,
        name: new.name,
        operators: new.operators,
        stations: new.stations,
        headcode_prefixes: new.headcode_prefixes,
        destination_crs_filter: new.destination_crs_filter,
    })
}

/// Deletes a custom line by id. Returns `true` if a row was deleted,
/// `false` if no custom line had that id.
pub async fn delete_custom_line(pool: &PgPool, id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM custom_lines WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_dashes_punctuation() {
        assert_eq!(slugify("My Commute"), "custom-my-commute");
    }

    #[test]
    fn slugify_collapses_runs_of_punctuation() {
        assert_eq!(slugify("Woking -> Alton!!"), "custom-woking-alton");
    }

    #[test]
    fn slugify_trims_trailing_punctuation() {
        assert_eq!(slugify("Trailing---"), "custom-trailing");
    }
}
```

In `crates/api/src/data/mod.rs`, add the new module:

```rust
pub mod config;
pub mod custom_lines;
pub mod queries;
pub mod samples;

pub use common::{LineDefinition, Station};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api slugify`
Expected: PASS (3 tests).

Then run the full crate's tests to confirm nothing else broke:

Run: `cargo test -p api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/custom_lines.rs crates/api/src/data/mod.rs
git commit -m "Add custom-lines CRUD queries to the api crate"
```

---

### Task 6: `api` — `GET/POST /public/lines`, `DELETE /public/lines/{id}`

**Files:**
- Create: `crates/api/src/routes/lines.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Consumes: `crate::data::custom_lines::{list_custom_lines, insert_custom_line, delete_custom_line, NewCustomLine}` (Task 5).
- Produces: `pub fn router() -> Router` merged into `public_router()`.

There's no existing HTTP-level integration test harness in this codebase (routes are verified manually against the running docker-compose stack — see `line_status.rs`'s module doc, which mentions a throwaway `tower::ServiceExt::oneshot` probe that was deliberately not kept). This task follows that same convention: no automated test, verified with `curl` against the real stack instead.

- [ ] **Step 1: Implement the routes**

Create `crates/api/src/routes/lines.rs`:

```rust
//! `/public/lines`: enumerate official + custom lines, and create/delete
//! custom ones. Unauthenticated — see
//! `docs/superpowers/specs/2026-07-09-custom-lines-and-blended-stats-design.md`'s
//! Non-goals for why.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::data::custom_lines::{self, NewCustomLine};

pub fn router() -> Router {
    Router::new()
        .route("/lines", axum::routing::get(list_lines).post(create_line))
        .route("/lines/{id}", axum::routing::delete(delete_line))
}

#[derive(Debug, Serialize)]
struct LineSummary {
    id: String,
    name: String,
    category: String,
    operators: Vec<String>,
    source: &'static str,
}

async fn list_lines(State(app): State<App>) -> Result<Json<Vec<LineSummary>>, (StatusCode, String)> {
    let mut out: Vec<LineSummary> = app
        .config
        .lines
        .iter()
        .map(|l| LineSummary {
            id: l.id.clone(),
            name: l.name.clone(),
            category: l.category.clone(),
            operators: l.operators.clone(),
            source: "catalogue",
        })
        .collect();

    let custom = custom_lines::list_custom_lines(&app.database).await.map_err(internal_error)?;
    out.extend(custom.into_iter().map(|c| LineSummary {
        id: c.id,
        name: c.name,
        category: "custom".to_string(),
        operators: c.operators,
        source: "custom",
    }));

    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct CreateLineRequest {
    name: String,
    operators: Vec<String>,
    stations: Vec<String>,
    #[serde(default)]
    headcode_prefixes: Vec<String>,
    #[serde(default)]
    destination_crs_filter: Vec<String>,
}

async fn create_line(
    State(app): State<App>,
    Json(req): Json<CreateLineRequest>,
) -> Result<Json<LineSummary>, (StatusCode, String)> {
    if req.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name must not be empty".to_string()));
    }
    if req.stations.len() < 2 {
        return Err((StatusCode::BAD_REQUEST, "a line needs at least 2 stations".to_string()));
    }

    let created = custom_lines::insert_custom_line(
        &app.database,
        NewCustomLine {
            name: req.name,
            operators: req.operators,
            stations: req.stations,
            headcode_prefixes: req.headcode_prefixes,
            destination_crs_filter: req.destination_crs_filter,
        },
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(LineSummary {
        id: created.id,
        name: created.name,
        category: "custom".to_string(),
        operators: created.operators,
        source: "custom",
    }))
}

async fn delete_line(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    if app.config.lines.iter().any(|l| l.id == id) {
        return Err((StatusCode::BAD_REQUEST, "cannot delete a catalogue line".to_string()));
    }

    let deleted = custom_lines::delete_custom_line(&app.database, &id).await.map_err(internal_error)?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "custom line operation failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "operation failed".to_string())
}
```

In `crates/api/src/routes/mod.rs`, add the module declaration and merge it into `public_router()`:

```rust
pub mod health;
pub mod ingest;
pub mod line_status;
pub mod lines;
pub mod samples;
```

```rust
pub fn public_router() -> Router {
    Router::new().merge(health::router()).merge(lines::router())
}
```

(Leave the existing doc comment on `public_router()` as-is — it's still accurate.)

- [ ] **Step 2: Build**

Run: `cargo build -p api`
Expected: builds clean.

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache api
docker compose up -d --no-build api
```

List lines (should show the 5 catalogue lines, each with `"source":"catalogue"`):

```bash
curl -s http://localhost:8080/public/lines | python3 -m json.tool
```

Create a custom line:

```bash
curl -s -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"My Commute","operators":["SW"],"stations":["WOK","AON"]}' \
  | python3 -m json.tool
```

Expected: `{"id":"custom-my-commute","name":"My Commute","category":"custom","operators":["SW"],"source":"custom"}`.

Confirm it now appears in the list:

```bash
curl -s http://localhost:8080/public/lines | python3 -m json.tool
```

Attempt to delete a catalogue line (should be rejected):

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/wcml
```

Expected: `HTTP 400`.

Delete the custom line:

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-my-commute
```

Expected: `HTTP 204`, and it no longer appears in a subsequent `GET /public/lines`.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/lines.rs crates/api/src/routes/mod.rs
git commit -m "Add GET/POST /public/lines and DELETE /public/lines/{id}"
```

---

### Task 7: `api` — `/sample-stations` includes custom lines' stations

**Files:**
- Modify: `crates/api/src/routes/samples.rs`

**Interfaces:**
- Consumes: `crate::data::custom_lines::list_custom_lines` (Task 5), `common::LineDefinition`.

- [ ] **Step 1: Implement**

Replace `crates/api/src/routes/samples.rs` entirely with:

```rust
//! Read-only endpoint exposing which stations `poller-ldbws` should
//! sample, computed from the line catalogue loaded into `AppState` at
//! startup plus any custom lines stored in the database. Custom lines can
//! be created/deleted at any time, so they're queried fresh on every
//! request rather than cached like the static catalogue.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use common::LineDefinition;

use crate::app::{App, Router};
use crate::data::custom_lines;
use crate::data::samples::dedup_sample_stations;

pub fn router() -> Router {
    Router::new().route("/sample-stations", axum::routing::get(get_sample_stations))
}

async fn get_sample_stations(State(app): State<App>) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let custom = custom_lines::list_custom_lines(&app.database).await.map_err(internal_error)?;
    let mut lines: Vec<LineDefinition> = app.config.lines.to_vec();
    lines.extend(custom.into_iter().map(LineDefinition::from));
    Ok(Json(dedup_sample_stations(&lines)))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "sample-stations query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}
```

Note: this route is nested under `/private` (see `crates/api/src/routes/mod.rs`'s `private_router()`), not `/public` — unchanged from before this task, just now DB-backed.

- [ ] **Step 2: Build**

Run: `cargo build -p api`
Expected: builds clean.

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache api
docker compose up -d --no-build api
TOKEN=$(grep -E '^INTERNAL_TOKEN=' .env | cut -d= -f2-)

# Create a custom line with a station not in any catalogue line's
# sample_stations (BSK - Basingstoke - isn't sampled by any of the 5
# catalogue lines' sample_stations lists).
curl -s -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"Test Line","operators":["SW"],"stations":["WOK","BSK"]}'

curl -s -H "X-Internal-Token: $TOKEN" http://localhost:8080/private/sample-stations | python3 -m json.tool
```

Expected: `"BSK"` and `"WOK"` both present in the returned list.

Clean up:

```bash
curl -s -X DELETE http://localhost:8080/public/lines/custom-test-line
```

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/samples.rs
git commit -m "Include custom lines' stations in /sample-stations"
```

---

### Task 8: `api` — `sampleStats` in the JSON response

**Files:**
- Modify: `crates/api/src/render.rs`

**Interfaces:**
- Consumes: `common::SampleStats` (Task 2), `LineStatus.sample_stats` (Task 2).
- Produces: `"sampleStats"` key in `status_to_json`'s output, present whenever `status.sample_stats` is `Some`, regardless of the `detail` flag.

- [ ] **Step 1: Write the failing test**

In `crates/api/src/render.rs`'s test module, first fix the `sample_report` helper's now-incomplete `LineStatus` literal (add `sample_stats: None,`):

```rust
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
                sample_stats: None,
            }],
        }
    }
```

Then add `SampleStats` to the test module's imports (`use common::{DataQuality, Disruption, SampleStats, ValidityPeriod};`) and add two new tests after `no_disruption_present_even_with_detail_flag`:

```rust
    #[test]
    fn sample_stats_included_when_present() {
        let mut report = sample_report(None);
        report.statuses[0].sample_stats = Some(SampleStats {
            total: 10,
            delayed: 4,
            cancelled: 1,
            avg_delay_minutes: 6.5,
        });
        let json = to_tfl_shape(&report, false);
        let stats = &json["lineStatuses"][0]["sampleStats"];
        assert_eq!(stats["total"], 10);
        assert_eq!(stats["delayed"], 4);
        assert_eq!(stats["cancelled"], 1);
        assert_eq!(stats["avgDelayMinutes"], 6.5);
    }

    #[test]
    fn sample_stats_omitted_when_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
        assert!(json["lineStatuses"][0].get("sampleStats").is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api sample_stats`
Expected: FAIL — `sampleStats` isn't produced by `status_to_json` yet (the `sample_stats_included_when_present` assertions fail against `null`).

- [ ] **Step 3: Implement**

In `crates/api/src/render.rs`, modify `status_to_json`:

```rust
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

    if let Some(stats) = &status.sample_stats {
        out["sampleStats"] = json!({
            "total": stats.total,
            "delayed": stats.delayed,
            "cancelled": stats.cancelled,
            "avgDelayMinutes": stats.avg_delay_minutes,
        });
    }

    if detail
        && let Some(disruption) = &status.disruption
    {
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

    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api`
Expected: PASS — all tests, including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "Include sampleStats in the line status JSON response"
```

---

### Task 9: frontend — `sampleStats` type

**Files:**
- Modify: `frontend/lib/types.ts`

**Interfaces:**
- Produces: `export interface SampleStats { total: number; delayed: number; cancelled: number; avgDelayMinutes: number }`; `LineStatus.sampleStats?: SampleStats`.

This is a pure type addition with no runtime behavior yet (a later plan's "representative info" block is what consumes it) — the only verification available at this point is that the frontend still typechecks.

- [ ] **Step 1: Implement**

In `frontend/lib/types.ts`, insert a new `SampleStats` interface (anywhere above `LineStatus`, e.g. right before it), and replace the existing `LineStatus` interface with this version, which adds one new optional field (`sampleStats`) at the end:

```typescript
export interface SampleStats {
  total: number;
  delayed: number;
  cancelled: number;
  avgDelayMinutes: number;
}

export interface LineStatus {
  statusSeverity: number;
  statusSeverityDescription: string;
  reason: string;
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned';
  validityPeriods: ValidityPeriod[];
  disruption?: Disruption;
  sampleStats?: SampleStats;
}
```

- [ ] **Step 2: Verify it typechecks**

```bash
cd frontend && npm run build
```

Expected: build succeeds with no type errors.

- [ ] **Step 3: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "Add SampleStats type to the frontend"
```

---

### Task 10: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Rebuild and restart both backend services**

```bash
docker compose build --no-cache aggregator api
docker compose up -d --no-build aggregator api
docker compose logs --tail 20 aggregator
```

Expected: no errors; `"loaded static line catalogue"` with `count=5` in the logs.

- [ ] **Step 2: Confirm existing (catalogue-only) status computation still works**

```bash
curl -s http://localhost:8080/Line/Mode/national-rail/Status | python3 -m json.tool | head -40
```

Expected: 5 lines returned (the static catalogue), each with at least one `lineStatuses` entry — no regression from before this plan.

- [ ] **Step 3: Create a custom line and confirm it gets a real computed status**

```bash
curl -s -X POST http://localhost:8080/public/lines \
  -H "Content-Type: application/json" \
  -d '{"name":"My Commute","operators":["SW"],"stations":["WOK","AON"]}'
```

Wait for the aggregator's next poll cycle (`POLL_INTERVAL_SECS_AGGREGATOR` in `.env`, default 60s from `crates/aggregator/src/config.rs`'s `poll_interval_secs` default):

```bash
sleep 65
curl -s "http://localhost:8080/Line/custom-my-commute/Status?detail=true" | python3 -m json.tool
```

Expected: HTTP 200 with a `LineStatusReport` for `custom-my-commute` — either "Good Service" (if no incidents/samples currently indicate otherwise) or a real computed status, proving the aggregator picked up the custom line and ran it through the same matching/inference pipeline as the static catalogue.

- [ ] **Step 4: Delete the custom line and confirm it drops out**

```bash
curl -s -o /dev/null -w "HTTP %{http_code}\n" -X DELETE http://localhost:8080/public/lines/custom-my-commute
sleep 65
curl -s -o /dev/null -w "HTTP %{http_code}\n" http://localhost:8080/Line/custom-my-commute/Status
```

Expected: `HTTP 404` — the aggregator's next cycle after the deletion calls `prune_removed_lines` (Task 4), which deletes the `line_status` row for any line no longer in the merged static+custom set, so the stale status doesn't linger.

- [ ] **Step 5: Run the full workspace test suite one more time**

```bash
cargo test --workspace
```

Expected: all tests pass.
