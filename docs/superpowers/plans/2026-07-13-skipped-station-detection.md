# Skipped-Station Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect trains that skip a scheduled calling point (a real disruption signal, distinct from a train that was never scheduled to call there) from the Darwin/LDBWS live departure board feed, and feed a skip rate into a line's severity classification as an independent signal alongside cancellation and delay.

**Architecture:** `poller-ldbws` parses Darwin's `subsequentCallingPoints` structure (already present in `GetDepBoardWithDetails` responses — no request-side change needed) into a new `StationDeparture.skipped_stations: Vec<String>` field, unfiltered. The aggregator intersects each relevant departure's `skipped_stations` against the specific line's own station list to decide whether a skip is genuinely relevant to that line, counts it into a new `SampleStats.skipped` field, and `classify()` compares a skip-rate candidate severity against the existing delay-rate candidate, taking whichever is more severe. The API and frontend then surface the aggregate count the same way `cancelled` already is.

**Tech Stack:** Rust (axum, sqlx, serde) backend; Next.js/Mantine v9 frontend; vitest + `@testing-library/react` for frontend tests, `cargo test` for backend tests.

## Global Constraints

- Only genuinely scheduled-but-skipped calls count as a "skip" — a calling point is only relevant if Darwin marks it `isCancelled: true` in `subsequentCallingPoints`. Never infer a skip from a service simply not appearing somewhere.
- Skip detection is an **independent signal** from cancellation — it must never be folded into `cancel_rate`, and cancel-rate-driven severity (`PartSuspended`/`ReducedService`) always takes priority over both delay and skip candidates, exactly as it does today for delay.
- Skip severity maps to the same milder tiers delay already uses (`SevereDelays`/`MinorDelays`), via **dedicated** threshold config keys (`minor_delays_skip_pct` = `0.25`, `severe_delays_skip_pct` = `0.50`, matching the current delay defaults), not the existing `minor_delays_pct`/`severe_delays_pct` keys.
- No new `DataQuality` tier — skip-driven inference stays `DataQuality::LdbwsInferred`.
- No per-service or per-station skip detail in the UI this iteration — aggregate count only, added to `RepresentativeInfo`'s existing stats line the same way `cancelled` was.
- `poller-ldbws` has no line-topology awareness (it only samples a flat CRS list) — it must capture every skipped calling point Darwin reports, unfiltered. Filtering to "does this skip matter to line X" happens only in the aggregator, against that line's own `stations` list (not `sample_stations` or `destination_crs_filter`, which are narrower).
- Full spec: `docs/superpowers/specs/2026-07-13-skipped-station-detection-design.md`.

---

## Task 1: Parse skipped calling points in `poller-ldbws`

**Files:**
- Modify: `crates/common/src/lib.rs` (add `StationDeparture.skipped_stations`)
- Modify: `crates/poller-ldbws/src/schema.rs`

**Interfaces:**
- Produces: `common::StationDeparture.skipped_stations: Vec<String>` — CRS codes of every calling point Darwin marks `isCancelled: true` for a given service, flattened across all `subsequentCallingPoints` entries. Empty when none.

- [ ] **Step 1: Add `skipped_stations` to `StationDeparture`**

In `crates/common/src/lib.rs`, find the `StationDeparture` struct (around line 147):

```rust
pub struct StationDeparture {
    pub service_id: String,
    pub operator: String,
    pub destination_crs: String,
    /// `std` field.
    pub scheduled: String,
    /// `etd` — may be `"On time"`, `"Cancelled"`, or `"HH:MM"`.
    pub estimated: String,
    pub is_cancelled: bool,
    pub delay_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_reason: Option<String>,
    /// e.g. `"1P23"`, from Darwin's `trainid`/`rid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headcode: Option<String>,
}
```

Replace it with:

```rust
pub struct StationDeparture {
    pub service_id: String,
    pub operator: String,
    pub destination_crs: String,
    /// `std` field.
    pub scheduled: String,
    /// `etd` — may be `"On time"`, `"Cancelled"`, or `"HH:MM"`.
    pub estimated: String,
    pub is_cancelled: bool,
    pub delay_minutes: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_reason: Option<String>,
    /// e.g. `"1P23"`, from Darwin's `trainid`/`rid`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headcode: Option<String>,
    /// CRS codes of scheduled calling points this specific service is
    /// skipping today (Darwin's per-calling-point `isCancelled`, not the
    /// same signal as the whole-service `is_cancelled`). Empty when the
    /// service reports no skipped calls.
    #[serde(default)]
    pub skipped_stations: Vec<String>,
}
```

- [ ] **Step 2: Write the failing schema tests**

In `crates/poller-ldbws/src/schema.rs`, replace the `SAMPLE_JSON` constant (the second service, Oxford, gains a `subsequentCallingPoints` block; the other two services are left as-is, exercising the "field absent" default path):

```rust
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
                    "serviceType": "train",
                    "subsequentCallingPoints": [
                        {
                            "callingPoint": [
                                {"locationName": "Didcot Parkway", "crs": "DID", "st": "10:22", "isCancelled": true},
                                {"locationName": "Oxford", "crs": "OXF", "st": "10:40", "isCancelled": false}
                            ]
                        }
                    ]
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
```

Update `parses_sample_board_and_maps_every_field` to assert on `skipped_stations` for all three services:

```rust
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
        assert_eq!(first.skipped_stations, Vec::<String>::new());

        let second = &departures[1];
        assert_eq!(second.estimated, "On time");
        assert_eq!(second.delay_minutes, 0);
        assert!(!second.is_cancelled);
        assert_eq!(second.skipped_stations, vec!["DID".to_string()]);

        let third = &departures[2];
        assert!(third.is_cancelled);
        assert_eq!(third.delay_minutes, 0);
        assert_eq!(
            third.cancel_reason,
            Some("This train has been cancelled because of a fault on this train".to_string())
        );
        assert_eq!(third.skipped_stations, Vec::<String>::new());
    }
```

Add two new tests directly after it, testing the extraction helper (written in Step 4) in isolation:

```rust
    #[test]
    fn skipped_stations_flattens_multiple_calling_point_lists() {
        // A split/joined service reports more than one callingPointList
        // (one per association) — both must be flattened into one result.
        let service = RdmServiceItem {
            service_id: "svc".to_string(),
            operator_code: "GW".to_string(),
            destination: vec![RdmServiceLocation { crs: "BRI".to_string() }],
            std: "10:00".to_string(),
            etd: "On time".to_string(),
            is_cancelled: false,
            cancel_reason: None,
            delay_reason: None,
            subsequent_calling_points: vec![
                RdmCallingPointList {
                    calling_point: vec![
                        RdmCallingPoint { crs: "DID".to_string(), is_cancelled: true },
                        RdmCallingPoint { crs: "SWI".to_string(), is_cancelled: false },
                    ],
                },
                RdmCallingPointList {
                    calling_point: vec![RdmCallingPoint { crs: "BRI".to_string(), is_cancelled: true }],
                },
            ],
        };
        let mut skipped = extract_skipped_stations(&service);
        skipped.sort();
        assert_eq!(skipped, vec!["BRI".to_string(), "DID".to_string()]);
    }

    #[test]
    fn skipped_stations_empty_when_no_calling_points_reported() {
        let service = RdmServiceItem {
            service_id: "svc".to_string(),
            operator_code: "GW".to_string(),
            destination: vec![RdmServiceLocation { crs: "BRI".to_string() }],
            std: "10:00".to_string(),
            etd: "On time".to_string(),
            is_cancelled: false,
            cancel_reason: None,
            delay_reason: None,
            subsequent_calling_points: vec![],
        };
        assert_eq!(extract_skipped_stations(&service), Vec::<String>::new());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p poller-ldbws`
Expected: FAIL to compile — `RdmCallingPointList`, `RdmCallingPoint`, and `extract_skipped_stations` don't exist yet, and `StationDeparture` literals in `parse_departures` are missing the new field.

- [ ] **Step 4: Implement the calling-point parsing**

In `crates/poller-ldbws/src/schema.rs`, replace the `RdmServiceItem` struct and add the two new structs after it:

```rust
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
    #[serde(default, rename = "subsequentCallingPoints")]
    subsequent_calling_points: Vec<RdmCallingPointList>,
}

#[derive(Debug, Deserialize)]
struct RdmCallingPointList {
    #[serde(default, rename = "callingPoint")]
    calling_point: Vec<RdmCallingPoint>,
}

#[derive(Debug, Deserialize)]
struct RdmCallingPoint {
    crs: String,
    #[serde(default, rename = "isCancelled")]
    is_cancelled: bool,
}
```

Add this function after `compute_delay_minutes` (before `parse_departures`):

```rust
/// Flattens every calling point Darwin marks `isCancelled: true` across all
/// of a service's `subsequentCallingPoints` entries (a service can report
/// more than one when it splits/joins) into a single CRS list. A calling
/// point that was never scheduled for this service doesn't appear in
/// `subsequentCallingPoints` at all, so nothing here can mistake a normal
/// fast-service stopping pattern for a genuine skip.
fn extract_skipped_stations(service: &RdmServiceItem) -> Vec<String> {
    service
        .subsequent_calling_points
        .iter()
        .flat_map(|list| list.calling_point.iter())
        .filter(|cp| cp.is_cancelled)
        .map(|cp| cp.crs.clone())
        .collect()
}
```

Update `parse_departures`'s `StationDeparture` literal to add the new field:

```rust
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
                skipped_stations: extract_skipped_stations(service),
            })
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p poller-ldbws`
Expected: PASS, all tests in `schema.rs` green.

- [ ] **Step 6: Run the full workspace build to catch other breakage**

Run: `cargo build --workspace`
Expected: FAIL — `crates/aggregator/src/aggregation.rs`'s `departure()` test helper constructs a `StationDeparture` literal missing the new field. This is expected; Task 2 fixes it. Confirm the *only* compile error is that one struct literal, then proceed.

- [ ] **Step 7: Commit**

```bash
git add crates/common/src/lib.rs crates/poller-ldbws/src/schema.rs
git commit -m "Parse skipped calling points from the LDBWS feed into StationDeparture"
```

---

## Task 2: Skip-rate severity classification in the aggregator

**Files:**
- Modify: `crates/common/src/lib.rs` (add `SampleStats.skipped`, `Defaults.minor_delays_skip_pct`/`severe_delays_skip_pct`, extend `thresholds_for`)
- Modify: `crates/aggregator/src/aggregation.rs`

**Interfaces:**
- Consumes: `StationDeparture.skipped_stations: Vec<String>` (Task 1), `LineDefinition.stations: Vec<Station>` (existing — full line topology, distinct from `sample_stations`).
- Produces: `SampleStats.skipped: usize`; `classify()` now takes and weighs a skip-rate candidate at the same priority tier as delay-rate.

- [ ] **Step 1: Add `skipped` to `SampleStats` and the two new threshold fields to `Defaults`**

In `crates/common/src/lib.rs`, replace the `SampleStats` struct (around line 324):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleStats {
    pub total: usize,
    pub delayed: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub avg_delay_minutes: f64,
}
```

In the `Defaults` struct (around line 387), add the two new fields after `part_suspended_pct`:

```rust
    /// >25% of sampled services skipping a scheduled stop -> Minor Delays.
    /// Independent of `minor_delays_pct` (which only looks at lateness).
    #[serde_inline_default(0.25)]
    pub minor_delays_skip_pct: f64,
    /// >50% of sampled services skipping a scheduled stop -> Severe Delays.
    /// Independent of `severe_delays_pct` (which only looks at lateness).
    #[serde_inline_default(0.50)]
    pub severe_delays_skip_pct: f64,
```

In `thresholds_for`, add the two new match arms alongside the existing ones:

```rust
            "minor_delays_pct" => merged.minor_delays_pct = *value,
            "severe_delays_pct" => merged.severe_delays_pct = *value,
            "minor_delays_skip_pct" => merged.minor_delays_skip_pct = *value,
            "severe_delays_skip_pct" => merged.severe_delays_skip_pct = *value,
            "reduced_service_pct" => merged.reduced_service_pct = *value,
            "part_suspended_pct" => merged.part_suspended_pct = *value,
```

Update the existing `every_field_can_be_overridden` test (in the same file's `defaults_tests` module) to cover the two new fields:

```rust
    #[test]
    fn every_field_can_be_overridden() {
        let defaults = Defaults::default();
        let mut overrides = HashMap::new();
        overrides.insert("delay_threshold_minutes".to_string(), 10.0);
        overrides.insert("minor_delays_pct".to_string(), 0.30);
        overrides.insert("severe_delays_pct".to_string(), 0.60);
        overrides.insert("minor_delays_skip_pct".to_string(), 0.35);
        overrides.insert("severe_delays_skip_pct".to_string(), 0.65);
        overrides.insert("reduced_service_pct".to_string(), 0.40);
        overrides.insert("part_suspended_pct".to_string(), 0.70);
        overrides.insert("knowledgebase_severity_floor".to_string(), 1.0);
        overrides.insert("min_sample_size".to_string(), 5.0);
        let merged = thresholds_for(&defaults, &overrides);
        assert_eq!(merged.delay_threshold_minutes, 10);
        assert_eq!(merged.minor_delays_pct, 0.30);
        assert_eq!(merged.severe_delays_pct, 0.60);
        assert_eq!(merged.minor_delays_skip_pct, 0.35);
        assert_eq!(merged.severe_delays_skip_pct, 0.65);
        assert_eq!(merged.reduced_service_pct, 0.40);
        assert_eq!(merged.part_suspended_pct, 0.70);
        assert_eq!(merged.knowledgebase_severity_floor, 1);
        assert_eq!(merged.min_sample_size, 5);
    }
```

- [ ] **Step 2: Fix the `departure()` test helper in `aggregation.rs`**

In `crates/aggregator/src/aggregation.rs`, add `use std::collections::HashSet;` to the top-level `use std::collections::HashMap;` line (make it `use std::collections::{HashMap, HashSet};`).

Update the `departure()` test helper (around line 519) to include the new field:

```rust
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
            skipped_stations: vec![],
        }
    }
```

Run: `cargo build --workspace`
Expected: PASS — this was the only remaining compile error from Task 1.

- [ ] **Step 3: Write the failing aggregator tests**

Add these tests to `crates/aggregator/src/aggregation.rs`'s `mod tests` block, after `infer_from_samples_classifies_severe_delays`:

```rust
    #[test]
    fn infer_from_samples_classifies_severe_skip_rate() {
        // swr-alton.toml: WOK is on the line's full `stations` list (part
        // of the shared trunk) but is not a sample station or in
        // destination_crs_filter — proves skip-relevance is checked
        // against `line.stations`, not the narrower sample/filter lists.
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let skipping = StationDeparture { skipped_stations: vec!["WOK".to_string()], ..departure("AON", 0, false) };
        let mut samples = HashMap::new();
        // 3 of 4 skip WOK -> 75% skip rate, above the default
        // severe_delays_skip_pct of 0.50, with delay_rate at 0%.
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![skipping.clone(), skipping.clone(), skipping, departure("AON", 0, false)],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults).expect("should classify");
        assert_eq!(status.severity, Severity::SevereDelays);
        assert_eq!(status.data_quality, DataQuality::LdbwsInferred);
        assert_eq!(status.sample_stats.expect("stats").skipped, 3);
    }

    #[test]
    fn infer_from_samples_classifies_minor_skip_rate() {
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let skipping = StationDeparture { skipped_stations: vec!["WOK".to_string()], ..departure("AON", 0, false) };
        let mut samples = HashMap::new();
        // 1 of 4 skips WOK -> 25% skip rate, exactly at the default
        // minor_delays_skip_pct of 0.25.
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![skipping, departure("AON", 0, false), departure("AON", 0, false), departure("AON", 0, false)],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults).expect("should classify");
        assert_eq!(status.severity, Severity::MinorDelays);
    }

    #[test]
    fn infer_from_samples_ignores_skip_of_station_not_on_line() {
        // "ZZZ" isn't anywhere in swr-alton's `stations` list, so this skip
        // must not count towards skip_rate at all.
        let lines = load_all_lines();
        let alton = &lines["swr-alton"];
        let defaults = Defaults::default();
        let skipping_unrelated = StationDeparture { skipped_stations: vec!["ZZZ".to_string()], ..departure("AON", 0, false) };
        let mut samples = HashMap::new();
        samples.insert(
            "AHT".to_string(),
            StationSample {
                crs: "AHT".to_string(),
                polled_at: Utc::now(),
                departures: vec![
                    skipping_unrelated.clone(),
                    skipping_unrelated.clone(),
                    skipping_unrelated.clone(),
                    skipping_unrelated,
                ],
            },
        );
        let status = infer_from_samples(alton, &samples, &defaults).expect("should classify");
        assert_eq!(status.severity, Severity::GoodService);
        assert_eq!(status.sample_stats.expect("stats").skipped, 0);
    }

    #[test]
    fn classify_prefers_more_severe_of_delay_and_skip_candidates() {
        // skip_rate (75%, >= severe_delays_skip_pct 0.50) is more severe
        // than delay_rate (25%, only >= minor_delays_pct 0.25) -> the
        // overall severity must be the skip candidate's SevereDelays, not
        // the delay candidate's MinorDelays.
        let (severity, reason) = classify(0.0, 0.25, 0.75, &Defaults::default(), 4, 0, 1, 3);
        assert_eq!(severity, Severity::SevereDelays);
        assert!(reason.contains("skipping"), "reason was: {reason}");
    }

    #[test]
    fn classify_combines_reason_when_delay_and_skip_tie() {
        // Both candidates land on MinorDelays (delay_rate 30% >= 0.25,
        // skip_rate 30% >= 0.25, neither >= their severe threshold) ->
        // combined message naming both counts.
        let (severity, reason) = classify(0.0, 0.30, 0.30, &Defaults::default(), 10, 0, 3, 3);
        assert_eq!(severity, Severity::MinorDelays);
        assert!(reason.contains("delayed"), "reason was: {reason}");
        assert!(reason.contains("skipping"), "reason was: {reason}");
    }

    #[test]
    fn classify_cancel_rate_still_takes_priority_over_skip_and_delay() {
        // cancel_rate alone (70%, >= part_suspended_pct 0.60) must win
        // even though skip_rate and delay_rate would also qualify for a
        // milder tier on their own.
        let (severity, _) = classify(0.70, 0.75, 0.75, &Defaults::default(), 10, 7, 7, 7);
        assert_eq!(severity, Severity::PartSuspended);
    }
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p aggregator`
Expected: FAIL to compile — `SampleStats { .. }` in `compute_sample_stats` is missing the `skipped` field, and `classify()`'s signature doesn't yet accept a skip rate/count.

- [ ] **Step 5: Implement skip counting and classification**

In `crates/aggregator/src/aggregation.rs`, replace `compute_sample_stats`:

```rust
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
    let line_stations: HashSet<&str> = line.stations.iter().map(|s| s.crs.as_str()).collect();
    let skipped = relevant
        .iter()
        .filter(|d| d.skipped_stations.iter().any(|crs| line_stations.contains(crs.as_str())))
        .count();
    let running: Vec<&&StationDeparture> = relevant.iter().filter(|d| !d.is_cancelled).collect();
    let avg_delay_minutes = if running.is_empty() {
        0.0
    } else {
        running.iter().map(|d| d.delay_minutes as f64).sum::<f64>() / running.len() as f64
    };

    Some(SampleStats { total, delayed, cancelled, skipped, avg_delay_minutes })
}
```

Replace `infer_from_samples`'s rate computation and `classify()` call:

```rust
    let cancel_rate = stats.cancelled as f64 / stats.total as f64;
    let delay_rate = stats.delayed as f64 / stats.total as f64;
    let skip_rate = stats.skipped as f64 / stats.total as f64;

    let (severity, mut reason) = classify(
        cancel_rate,
        delay_rate,
        skip_rate,
        &thresholds,
        stats.total,
        stats.cancelled,
        stats.delayed,
        stats.skipped,
    );
```

Replace `classify`:

```rust
fn classify(
    cancel_rate: f64,
    delay_rate: f64,
    skip_rate: f64,
    thresholds: &Defaults,
    total: usize,
    cancelled: usize,
    delayed: usize,
    skipped: usize,
) -> (Severity, String) {
    if cancel_rate >= thresholds.part_suspended_pct {
        return (Severity::PartSuspended, format!("{cancelled} of {total} sampled services cancelled."));
    }
    if cancel_rate >= thresholds.reduced_service_pct {
        return (Severity::ReducedService, format!("{cancelled} of {total} sampled services cancelled."));
    }

    let delay_severity = if delay_rate >= thresholds.severe_delays_pct {
        Some(Severity::SevereDelays)
    } else if delay_rate >= thresholds.minor_delays_pct {
        Some(Severity::MinorDelays)
    } else {
        None
    };
    let skip_severity = if skip_rate >= thresholds.severe_delays_skip_pct {
        Some(Severity::SevereDelays)
    } else if skip_rate >= thresholds.minor_delays_skip_pct {
        Some(Severity::MinorDelays)
    } else {
        None
    };

    match (delay_severity, skip_severity) {
        (Some(d), Some(s)) if d == s => (
            d,
            format!(
                "{delayed} of {total} sampled services delayed, {skipped} of {total} sampled services skipping a scheduled stop."
            ),
        ),
        (Some(d), Some(s)) if d < s => (d, format!("{delayed} of {total} sampled services delayed.")),
        (Some(_), Some(_)) => (
            skip_severity.expect("skip_severity is Some in this arm"),
            format!("{skipped} of {total} sampled services skipping a scheduled stop."),
        ),
        (Some(d), None) => (d, format!("{delayed} of {total} sampled services delayed.")),
        (None, Some(s)) => (s, format!("{skipped} of {total} sampled services skipping a scheduled stop.")),
        (None, None) => (Severity::GoodService, "Good Service".to_string()),
    }
}
```

(`Severity` derives `Ord` with lower values more severe, so `d < s` means the delay candidate is strictly more severe than the skip candidate.)

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p aggregator`
Expected: PASS, all tests green, including the six new ones from Step 3.

- [ ] **Step 7: Run the full workspace build to catch other breakage**

Run: `cargo build --workspace`
Expected: FAIL — `crates/api/src/render.rs`'s test-only `SampleStats { .. }` literal is missing the `skipped` field. Expected; Task 3 fixes it. Confirm no other compile errors, then proceed.

- [ ] **Step 8: Commit**

```bash
git add crates/common/src/lib.rs crates/aggregator/src/aggregation.rs
git commit -m "Classify skip rate as an independent severity signal"
```

---

## Task 3: Serialize `sampleStats.skipped` in the API response

**Files:**
- Modify: `crates/api/src/render.rs`

**Interfaces:**
- Consumes: `SampleStats.skipped: usize` (Task 2).
- Produces: JSON field `sampleStats.skipped` in `GET /Line/{id}/Status` responses.

- [ ] **Step 1: Write the failing test**

In `crates/api/src/render.rs`, update the `sample_stats_included_when_present` test:

```rust
    #[test]
    fn sample_stats_included_when_present() {
        let mut report = sample_report(None);
        report.statuses[0].sample_stats = Some(SampleStats {
            total: 10,
            delayed: 4,
            cancelled: 1,
            skipped: 2,
            avg_delay_minutes: 6.5,
        });
        let json = to_tfl_shape(&report, false);
        let stats = &json["lineStatuses"][0]["sampleStats"];
        assert_eq!(stats["total"], 10);
        assert_eq!(stats["delayed"], 4);
        assert_eq!(stats["cancelled"], 1);
        assert_eq!(stats["skipped"], 2);
        assert_eq!(stats["avgDelayMinutes"], 6.5);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p api sample_stats_included_when_present`
Expected: FAIL to compile — the `SampleStats` literal is missing `skipped`, and once that's fixed, `stats["skipped"]` would be `Value::Null` since `status_to_json` doesn't serialize it yet.

- [ ] **Step 3: Implement**

In `crates/api/src/render.rs`, update `status_to_json`'s `sampleStats` block:

```rust
    if let Some(stats) = &status.sample_stats {
        out["sampleStats"] = json!({
            "total": stats.total,
            "delayed": stats.delayed,
            "cancelled": stats.cancelled,
            "skipped": stats.skipped,
            "avgDelayMinutes": stats.avg_delay_minutes,
        });
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p api sample_stats_included_when_present`
Expected: PASS.

- [ ] **Step 5: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, all tests green across every crate.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "Serialize sampleStats.skipped in API responses"
```

---

## Task 4: Display skip count in `RepresentativeInfo`

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/components/RepresentativeInfo.tsx`
- Modify: `frontend/components/RepresentativeInfo.test.tsx`

**Interfaces:**
- Consumes: JSON field `sampleStats.skipped` (Task 3), already reaching the frontend via the existing `getLineStatus`/`SampleStats` fetch path — no new fetch needed.
- Produces: an updated aggregate-stats line in `RepresentativeInfo`.

- [ ] **Step 1: Add `skipped` to the `SampleStats` TypeScript type**

In `frontend/lib/types.ts`, update the `SampleStats` interface:

```typescript
export interface SampleStats {
  total: number;
  delayed: number;
  cancelled: number;
  skipped: number;
  avgDelayMinutes: number;
}
```

- [ ] **Step 2: Write the failing test**

In `frontend/components/RepresentativeInfo.test.tsx`, update both fixtures and add an assertion:

```typescript
  it('renders the sample stats summary when present', () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, skipped: 5, avgDelayMinutes: 12.4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withStats]} />);
    expect(screen.getByText(/142 of 160 sampled services delayed/)).toBeInTheDocument();
    expect(screen.getByText(/3 cancelled/)).toBeInTheDocument();
    expect(screen.getByText(/5 skipping stops/)).toBeInTheDocument();
    expect(screen.getByText(/avg 12\.4 min late/)).toBeInTheDocument();
  });

  it('uses the first status carrying sampleStats when multiple statuses exist', () => {
    const withoutStats = baseStatus();
    const withStats = baseStatus({
      reason: 'Different issue',
      sampleStats: { total: 20, delayed: 5, cancelled: 0, skipped: 0, avgDelayMinutes: 4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withoutStats, withStats]} />);
    expect(screen.getByText(/5 of 20 sampled services delayed/)).toBeInTheDocument();
  });
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/RepresentativeInfo.test.tsx`
Expected: FAIL — TypeScript error (fixtures missing `skipped`) and/or the `/5 skipping stops/` assertion not found.

- [ ] **Step 4: Implement**

In `frontend/components/RepresentativeInfo.tsx`, update the destructure and displayed text:

```tsx
  const { total, delayed, cancelled, skipped, avgDelayMinutes } = withStats.sampleStats;

  return (
    <Card withBorder padding="sm">
      <Text size="sm">
        {delayed} of {total} sampled services delayed, {cancelled} cancelled, {skipped} skipping stops, avg{' '}
        {avgDelayMinutes.toFixed(1)} min late.
      </Text>
    </Card>
  );
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/RepresentativeInfo.test.tsx`
Expected: PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run: `cd frontend && npm test -- --run && npm run build`
Expected: PASS — all tests green, production build succeeds.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/types.ts frontend/components/RepresentativeInfo.tsx frontend/components/RepresentativeInfo.test.tsx
git commit -m "Display skip count in RepresentativeInfo"
```

---

## Final verification

After all four tasks:

- [ ] Run `cargo test --workspace` — expect all tests passing.
- [ ] Run `cd frontend && npm test -- --run && npm run build` — expect all tests passing and a clean production build.
- [ ] Follow this session's established pattern: independent code-review subagent dispatch, address findings, merge to `master`, clean up the worktree — matching every prior feature in this session (see `superpowers:finishing-a-development-branch`).
