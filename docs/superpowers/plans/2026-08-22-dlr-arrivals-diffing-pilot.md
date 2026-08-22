# DLR Arrivals-Diffing Pilot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the DLR its own real `sample_stats` (delayed/cancelled/avg-delay counts), inferred from TfL's Arrivals predictions diffed against its published Timetable, instead of the `None` every TfL line carries today.

**Architecture:** A new `dlr` submodule inside `crates/poller-tfl` (not a separate crate or binary — see Task 7's rationale) fetches `GET /Line/dlr/Arrivals` (bulk, one call) and `GET /Line/dlr/Timetable/{stopPointId}` for one fixed pilot station, matches live predictions to scheduled trips, and folds the resulting `SampleStats` onto the DLR line's existing `LineStatus` before the existing `/private/tfl-line-status` POST — reusing the pipeline `poller-tfl` already has, not building a new one. A small amount of in-memory (not persisted) state tracks trips across poll cycles so a trip that never gets a matching prediction can be promoted to "cancelled" after a grace window.

**Tech Stack:** Rust (`crates/poller-tfl`, `crates/api`), existing `common`/`ingest` crates, no new dependencies, no schema/migration changes.

**Spec:** `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`, Area 3 ("DLR arrivals-diffing pilot").

## Global Constraints

- No new `common::DataQuality` variant — DLR statuses stay `DataQuality::Tfl` (spec Non-goals).
- No unification of TfL's and National Rail's severity models — this pilot never changes a `LineStatus.severity` value; `sample_stats` is attached as pure supplementary data, same as `SampleStats`'s own doc comment says ("informational only"). Unlike the aggregator's NR-side `escalate_from_sample_stats`, this pilot does **not** escalate TfL's own severity from the inferred stats — that's a bigger design commitment the spec doesn't ask for, and mixing an unofficial inference into TfL's official feed's severity would blur data provenance.
- `skipped` in the computed `SampleStats` stays `0` always — no calling-point-skip concept exists for a metro service (spec Design sketch).
- Coverage is intentionally narrow: this pilot matches scheduled-vs-actual trips for **Poplar** only (a central DLR interchange served by all branches), not the full ~45-station network. Expanding coverage is explicitly out of scope for this plan — see Open Items in the spec and this plan's own final note.
- Request volume: one `/Line/dlr/Arrivals` call and one `/Line/dlr/Timetable/{stopPointId}` call per poll cycle, on `poller-tfl`'s existing 300s interval (see Task 7 for why the interval is not shortened for this pilot). This is nowhere near TfL's 500 req/min tier.

---

## Task 1: Guard the TfL ingest diff check against `sample_stats` volatility

This lands first, before any code that actually populates `sample_stats` for a TfL line, so the safety net exists before it's needed. Right now `tfl_statuses_changed` (`crates/api/src/data/queries.rs`) does a raw JSON equality check on the whole `statuses` array — safe today only because every TfL-sourced status has `sample_stats: None`. The moment Task 7 starts populating it for DLR, this same field will differ almost every 300s cycle (a live delay count moves constantly), and an unguarded equality check will insert a `line_status_history` row every single cycle for the DLR line — reproducing, on the TfL ingest path, the exact bug class already fixed on the aggregator's NR path via `normalize_for_diff`/`strip_live_sample_annotation` (`crates/aggregator/src/queries.rs:162-193`).

**Files:**
- Modify: `crates/api/src/data/queries.rs:287-308` (the `tfl_statuses_changed` function and its doc comment)
- Test: `crates/api/src/data/queries.rs` (inline `#[cfg(test)] mod tests`, matching this file's existing convention)

**Interfaces:**
- Consumes: nothing new — this task only changes how `upsert_tfl_line_status` (same file, line 322) decides whether to write a `line_status_history` row.
- Produces: `fn tfl_statuses_changed(existing: Option<&serde_json::Value>, incoming: &serde_json::Value) -> bool` — same signature as today, so `upsert_tfl_line_status`'s call site at line 360 does not change.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `crates/api/src/data/queries.rs`:

```rust
#[test]
fn tfl_statuses_changed_ignores_sample_stats_only_differences() {
    let existing = serde_json::json!([{
        "severity": "GoodService",
        "reason": "Good Service",
        "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
        "data_quality": "tfl",
        "sample_stats": { "total": 40, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.2 }
    }]);
    let incoming = serde_json::json!([{
        "severity": "GoodService",
        "reason": "Good Service",
        "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
        "data_quality": "tfl",
        "sample_stats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 2.4 }
    }]);
    assert!(!tfl_statuses_changed(Some(&existing), &incoming));
}

#[test]
fn tfl_statuses_changed_still_true_when_severity_changes_alongside_sample_stats() {
    let existing = serde_json::json!([{
        "severity": "GoodService",
        "reason": "Good Service",
        "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
        "data_quality": "tfl",
        "sample_stats": { "total": 40, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.2 }
    }]);
    let incoming = serde_json::json!([{
        "severity": "MinorDelays",
        "reason": "Minor Delays",
        "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
        "data_quality": "tfl",
        "sample_stats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 2.4 }
    }]);
    assert!(tfl_statuses_changed(Some(&existing), &incoming));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p api tfl_statuses_changed -- --nocapture`
Expected: `tfl_statuses_changed_ignores_sample_stats_only_differences` FAILS (current raw-equality implementation treats the `sample_stats` difference as a change); the second test already passes.

- [ ] **Step 3: Implement a normalizing comparison**

Replace the function body in `crates/api/src/data/queries.rs`:

```rust
/// Pure diff check, factored out of `upsert_tfl_line_status` so it's
/// testable without a database: a TfL line's statuses are "changed" if the
/// line is new to us, or if the incoming `statuses` JSON differs from what
/// is stored, ignoring `sample_stats` — mirroring the aggregator's own
/// `normalize_for_diff` (`crates/aggregator/src/queries.rs`), which strips
/// the same field for the same reason: a live delay/cancellation count
/// rolls over almost every poll cycle even when nothing about the
/// underlying disruption has changed, and must not participate in change
/// detection or `line_status_history` grows a row every poll cycle. This
/// guard exists ahead of any TfL-sourced line actually populating
/// `sample_stats` (see `crates/poller-tfl/src/dlr`), so it's already in
/// place once one does.
fn tfl_statuses_changed(existing: Option<&serde_json::Value>, incoming: &serde_json::Value) -> bool {
    match existing {
        None => true,
        Some(stored) => normalize_for_diff(stored) != normalize_for_diff(incoming),
    }
}

/// Strips `sample_stats` from every status entry before comparison. See
/// `tfl_statuses_changed`.
fn normalize_for_diff(statuses: &serde_json::Value) -> serde_json::Value {
    let mut statuses = statuses.clone();
    if let Some(entries) = statuses.as_array_mut() {
        for entry in entries {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("sample_stats");
            }
        }
    }
    statuses
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p api tfl_statuses_changed -- --nocapture`
Expected: both tests PASS, plus every pre-existing test in `crates/api/src/data/queries.rs` still passes (`cargo test -p api`).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "fix(api): ignore sample_stats when detecting TfL status changes"
```

---

## Task 2: Capture real DLR Arrivals and Timetable API responses

TfL's `Prediction` and `Timetable` response shapes below are transcribed from TfL's public Unified API entity documentation (`Tfl.Api.Presentation.Entities.Prediction`, `.Timetable`, `.Schedule`, `.KnownJourney`, `.StationInterval`), the same confidence tier `crates/poller-ldbws/src/schema.rs`'s XSD cross-reference used for skipped-station calling points — **not yet a live capture in this repo**, unlike `poller-tfl/src/schema.rs`'s `TRAM_STATUS_JSON`, which was. This task produces that live capture so Tasks 3–4 can be corrected against real data before anything ships.

**Files:**
- Create: `crates/poller-tfl/tests/fixtures/README.md` (records what was captured, when, and with what request — this crate has no `tests/fixtures/` directory yet; every other fixture in this codebase lives inline in a `#[cfg(test)]` module, e.g. `TRAM_STATUS_JSON`, so this one small file exists only to document the two saved captures below, not to establish a new fixture-loading convention)
- Create: `crates/poller-tfl/tests/fixtures/dlr_arrivals.json`
- Create: `crates/poller-tfl/tests/fixtures/dlr_timetable_poplar.json`

**Interfaces:**
- Produces: two saved JSON files that Tasks 3 and 4 paste into their own inline test constants (following this crate's existing convention of inline string literals in `#[cfg(test)]` modules, not file-loading at test time) — copy the file contents into the `const` in each task's test module rather than reading the file at runtime.

- [ ] **Step 1: Get a TfL app key**

Register at `https://api-portal.tfl.gov.uk` if the executor doesn't already have one (the same key `TFL_APP_KEY` already uses for `poller-tfl` in this deployment works — check `.env`/deployment secrets first).

- [ ] **Step 2: Capture `/Line/dlr/Arrivals`**

```bash
curl -s -H "Ocp-Apim-Subscription-Key: $TFL_APP_KEY" \
  "https://api.tfl.gov.uk/Line/dlr/Arrivals" \
  | jq '.' > crates/poller-tfl/tests/fixtures/dlr_arrivals.json
```

Confirm the file is non-empty and each element has (at minimum) `id`, `vehicleId`, `naptanId`, `stationName`, `lineId`, `platformName`, `destinationNaptanId`, `destinationName`, `timestamp`, `timeToStation`, `expectedArrival`, `modeName`. If any of these field names differ from what's captured, note the real names — Task 3 must use them, not the ones assumed here.

- [ ] **Step 3: Capture `/Line/dlr/Timetable/{stopPointId}` for Poplar**

Poplar's Naptan id is `940GZZDLPOP` (per TfL's published StopPoint list — confirm this resolves; if not, find Poplar's id via `GET /StopPoint/Search/Poplar` first and use whatever it returns instead):

```bash
curl -s -H "Ocp-Apim-Subscription-Key: $TFL_APP_KEY" \
  "https://api.tfl.gov.uk/Line/dlr/Timetable/940GZZDLPOP" \
  | jq '.' > crates/poller-tfl/tests/fixtures/dlr_timetable_poplar.json
```

Confirm the response has a `timetable.routes[].schedules[].knownJourneys[]` shape (each journey with `hour`/`minute`) and a `timetable.routes[].stationIntervals[].intervals[]` shape (each interval with a `stopId` and `timeToArrival` in minutes from the departure stop). If the real response nests this differently, record the actual shape — Task 4 must match it exactly, not the shape assumed here.

- [ ] **Step 4: Record findings**

Create `crates/poller-tfl/tests/fixtures/README.md`:

```markdown
# poller-tfl test fixtures

Live captures for the DLR arrivals-diffing pilot (see
`docs/superpowers/plans/2026-08-22-dlr-arrivals-diffing-pilot.md`).

- `dlr_arrivals.json` — `GET /Line/dlr/Arrivals`, captured <DATE>.
- `dlr_timetable_poplar.json` — `GET /Line/dlr/Timetable/940GZZDLPOP`,
  captured <DATE>.

<Note here whether the real response shapes matched the plan's assumed
`Prediction`/`Timetable` schema, or record what actually differed and
where it was corrected — Tasks 3 and 4 update this note if they had to
adjust field names.>
```

- [ ] **Step 5: Commit**

```bash
git add crates/poller-tfl/tests/fixtures/
git commit -m "test(poller-tfl): capture live DLR Arrivals and Timetable responses"
```

---

## Task 3: Parse `/Line/dlr/Arrivals` into `Prediction` structs

**Files:**
- Create: `crates/poller-tfl/src/dlr/mod.rs`
- Create: `crates/poller-tfl/src/dlr/arrivals.rs`
- Modify: `crates/poller-tfl/src/main.rs:15-16` (add `mod dlr;`)

**Interfaces:**
- Consumes: the JSON captured in Task 2 (`crates/poller-tfl/tests/fixtures/dlr_arrivals.json`) — paste its contents into this task's test constant, correcting field names below if Task 2 found real ones differ.
- Produces: `pub fn parse_arrivals(json: &str) -> anyhow::Result<Vec<Prediction>>` and `pub struct Prediction { pub vehicle_id: String, pub naptan_id: String, pub station_name: String, pub destination_naptan_id: String, pub destination_name: String, pub expected_arrival: DateTime<Utc>, pub time_to_station: i64 }` — Task 5 matches on `Prediction`'s fields directly.

- [ ] **Step 1: Write the failing test**

Create `crates/poller-tfl/src/dlr/arrivals.rs`:

```rust
//! Parses TfL's `GET /Line/dlr/Arrivals` response — a flat list of live
//! per-train predictions, one entry per (vehicle, next stop) pair, covering
//! the whole DLR network in a single call. Field names are transcribed
//! from TfL's public `Prediction` entity docs; see
//! `crates/poller-tfl/tests/fixtures/README.md` for what the live capture
//! actually confirmed.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    pub vehicle_id: String,
    pub naptan_id: String,
    pub station_name: String,
    #[serde(default)]
    pub destination_naptan_id: String,
    #[serde(default)]
    pub destination_name: String,
    pub expected_arrival: DateTime<Utc>,
    pub time_to_station: i64,
}

pub fn parse_arrivals(json: &str) -> Result<Vec<Prediction>> {
    Ok(serde_json::from_str(json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Replace with the real contents of
    // crates/poller-tfl/tests/fixtures/dlr_arrivals.json once Task 2 has
    // captured it — trimmed to 2-3 representative predictions.
    const DLR_ARRIVALS_JSON: &str = r#"[
      {
        "id": "-1234567890",
        "vehicleId": "301",
        "naptanId": "940GZZDLPOP",
        "stationName": "Poplar",
        "lineId": "dlr",
        "platformName": "3",
        "direction": "outbound",
        "destinationNaptanId": "940GZZDLBKG",
        "destinationName": "Bank",
        "timestamp": "2026-08-22T10:00:00Z",
        "timeToStation": 120,
        "expectedArrival": "2026-08-22T10:02:00Z",
        "modeName": "dlr"
      }
    ]"#;

    #[test]
    fn parses_a_prediction_and_maps_every_field_this_pilot_needs() {
        let predictions = parse_arrivals(DLR_ARRIVALS_JSON).expect("should parse");
        assert_eq!(predictions.len(), 1);
        let p = &predictions[0];
        assert_eq!(p.vehicle_id, "301");
        assert_eq!(p.naptan_id, "940GZZDLPOP");
        assert_eq!(p.station_name, "Poplar");
        assert_eq!(p.destination_name, "Bank");
        assert_eq!(p.expected_arrival, "2026-08-22T10:02:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(p.time_to_station, 120);
    }

    #[test]
    fn an_empty_response_parses_to_an_empty_list() {
        assert!(parse_arrivals("[]").expect("should parse").is_empty());
    }
}
```

Create `crates/poller-tfl/src/dlr/mod.rs`:

```rust
//! DLR-specific arrivals-diffing pilot (see
//! `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`,
//! Area 3, and `docs/superpowers/plans/2026-08-22-dlr-arrivals-diffing-pilot.md`).
//!
//! Unlike the rest of `poller-tfl`, which only relays status TfL has
//! already computed, this module infers `common::SampleStats` itself, by
//! diffing live Arrivals predictions against DLR's published Timetable for
//! one pilot station (Poplar). No other TfL line does this.

pub mod arrivals;
pub mod timetable;
pub mod inference;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p poller-tfl dlr::arrivals -- --nocapture`
Expected: FAILS to compile (`mod dlr;` not yet wired into `main.rs`).

- [ ] **Step 3: Wire the module in**

In `crates/poller-tfl/src/main.rs`, change:

```rust
mod config;
mod schema;
```

to:

```rust
mod config;
mod dlr;
mod schema;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p poller-tfl dlr::arrivals -- --nocapture`
Expected: both tests PASS. (`inference.rs`/`timetable.rs` don't exist yet — Task 3's `mod.rs` above declares them ahead of time; add empty stub files now so the crate compiles: `crates/poller-tfl/src/dlr/timetable.rs` and `crates/poller-tfl/src/dlr/inference.rs`, each containing only their module doc comment for now, filled in by Tasks 4 and 5.)

- [ ] **Step 5: Commit**

```bash
git add crates/poller-tfl/src/dlr/ crates/poller-tfl/src/main.rs
git commit -m "feat(poller-tfl): parse DLR Arrivals predictions"
```

---

## Task 4: Parse `/Line/dlr/Timetable/{stopPointId}` into scheduled trips

**Files:**
- Modify: `crates/poller-tfl/src/dlr/timetable.rs` (fill in the stub from Task 3)

**Interfaces:**
- Consumes: the JSON captured in Task 2 (`dlr_timetable_poplar.json`) — paste into this task's test constant, correcting nesting/field names if Task 2 found the real shape differs.
- Produces: `pub fn parse_timetable(json: &str, service_date: NaiveDate) -> anyhow::Result<Vec<ScheduledTrip>>` and `pub struct ScheduledTrip { pub scheduled_departure: DateTime<Utc>, pub interval_id: Option<String> }` — Task 5 matches trips against `Prediction`s purely on `scheduled_departure` proximity (see Task 5's `MATCH_WINDOW_MINUTES`); `interval_id` is carried through unused, for a future iteration that resolves it to a real destination.

- [ ] **Step 1: Write the failing test**

Replace `crates/poller-tfl/src/dlr/timetable.rs`:

```rust
//! Parses TfL's `GET /Line/dlr/Timetable/{stopPointId}` response for one
//! fixed pilot station (Poplar — see the plan's Global Constraints for why
//! this pilot doesn't cover the whole network). `knownJourneys[]` gives
//! each scheduled departure as an `hour`/`minute` pair with no date; this
//! module combines each with the `service_date` the caller is asking
//! about (`chrono::Utc::now()`'s date, threaded in the same way
//! `poller-tfl/src/schema.rs::parse_line_status` threads `now` — never
//! read directly, so parsing stays deterministic under test).

use anyhow::Result;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TimetableResponse {
    timetable: Timetable,
}

#[derive(Debug, Deserialize)]
struct Timetable {
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Route {
    #[serde(default)]
    schedules: Vec<Schedule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    #[serde(default)]
    known_journeys: Vec<KnownJourney>,
}

#[derive(Debug, Deserialize)]
struct KnownJourney {
    hour: String,
    minute: String,
    #[serde(default)]
    #[serde(rename = "intervalId")]
    interval_id: Option<String>,
}

/// One scheduled DLR departure from the pilot station, resolved to a real
/// timestamp for `service_date`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledTrip {
    pub scheduled_departure: DateTime<Utc>,
    /// TfL's Timetable response does not carry a destination per journey
    /// the way Arrivals does — only a route-level `intervalId` grouping.
    /// Matching (Task 5) does not use this field yet; kept for a future
    /// iteration that resolves `intervalId` to a real destination via
    /// `timetable.routes[].stationIntervals[]`, which this pilot does not
    /// parse.
    pub interval_id: Option<String>,
}

pub fn parse_timetable(json: &str, service_date: NaiveDate) -> Result<Vec<ScheduledTrip>> {
    let response: TimetableResponse = serde_json::from_str(json)?;
    let mut trips = Vec::new();
    for route in &response.timetable.routes {
        for schedule in &route.schedules {
            for journey in &schedule.known_journeys {
                let hour: u32 = journey.hour.parse()?;
                let minute: u32 = journey.minute.parse()?;
                let naive = service_date.and_hms_opt(hour, minute, 0).ok_or_else(|| {
                    anyhow::anyhow!("invalid knownJourney time {}:{}", journey.hour, journey.minute)
                })?;
                trips.push(ScheduledTrip {
                    scheduled_departure: Utc.from_utc_datetime(&naive),
                    interval_id: journey.interval_id.clone(),
                });
            }
        }
    }
    Ok(trips)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Replace with the real contents of
    // crates/poller-tfl/tests/fixtures/dlr_timetable_poplar.json once Task 2
    // has captured it — trimmed to one route/schedule/journey.
    const DLR_TIMETABLE_JSON: &str = r#"{
      "lineId": "dlr",
      "lineName": "DLR",
      "direction": "outbound",
      "timetable": {
        "departureStopId": "940GZZDLPOP",
        "routes": [
          {
            "stationIntervals": [],
            "schedules": [
              {
                "name": "MonFri",
                "knownJourneys": [
                  { "hour": "10", "minute": "02", "intervalId": "1" },
                  { "hour": "10", "minute": "06", "intervalId": "2" }
                ]
              }
            ]
          }
        ]
      }
    }"#;

    fn service_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 22).unwrap()
    }

    #[test]
    fn parses_known_journeys_into_scheduled_trips_on_the_given_date() {
        let trips = parse_timetable(DLR_TIMETABLE_JSON, service_date()).expect("should parse");
        assert_eq!(trips.len(), 2);
        assert_eq!(trips[0].scheduled_departure, "2026-08-22T10:02:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(trips[1].scheduled_departure, "2026-08-22T10:06:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn a_response_with_no_journeys_parses_to_an_empty_list() {
        let json = r#"{"lineId":"dlr","lineName":"DLR","direction":"outbound","timetable":{"departureStopId":"940GZZDLPOP","routes":[]}}"#;
        assert!(parse_timetable(json, service_date()).expect("should parse").is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p poller-tfl dlr::timetable -- --nocapture`
Expected: FAILS (function doesn't exist yet in the stub).

- [ ] **Step 3: Confirm implementation above compiles and passes**

The implementation is written inline above (this task, unlike a from-scratch one, writes test and implementation together since the shape is fixed by TfL's documented schema — run the tests to confirm, adjusting field names per Task 2's findings if the live capture disagreed with the assumed shape).

Run: `cargo test -p poller-tfl dlr::timetable -- --nocapture`
Expected: both tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/poller-tfl/src/dlr/timetable.rs
git commit -m "feat(poller-tfl): parse DLR scheduled timetable"
```

---

## Task 5: Match scheduled trips to live predictions (pure function)

This is the core new capability the spec's risk framing refers to: mapping a live prediction to the scheduled trip it corresponds to. Kept as a pure function — no I/O, no cross-cycle state — so it's fully unit-testable; Task 6 adds the cross-poll bookkeeping around it.

**Files:**
- Modify: `crates/poller-tfl/src/dlr/inference.rs` (fill in the stub from Task 3)

**Interfaces:**
- Consumes: `Vec<timetable::ScheduledTrip>` (Task 4), `&[arrivals::Prediction]` (Task 3).
- Produces: `pub fn match_trips(trips: Vec<ScheduledTrip>, predictions: &[Prediction], now: DateTime<Utc>) -> Vec<MatchedTrip>` and `pub enum MatchedTrip { Matched { delay_minutes: i64 }, Pending }` — Task 6 consumes `Vec<MatchedTrip>` to decide what's still waiting vs. resolved.

- [ ] **Step 1: Write the failing test**

Replace `crates/poller-tfl/src/dlr/inference.rs`:

```rust
//! Matches DLR scheduled trips (`timetable::ScheduledTrip`) against live
//! Arrivals predictions (`arrivals::Prediction`) to infer per-trip delay,
//! and (via `DlrMatchState`, added once this module also owns cross-poll
//! state) cancellation. `sample_stats` computed here is attached to the
//! DLR line's existing `LineStatus` — it never changes that status's
//! `severity`; see the plan's Global Constraints for why.

use chrono::{DateTime, Utc};

use super::arrivals::Prediction;
use super::timetable::ScheduledTrip;

/// A service is "delayed" once its delay exceeds this many minutes —
/// mirrors `common::Defaults::delay_threshold_minutes`'s default (5). Not
/// read from `Defaults` itself: that struct is wired to the NR aggregator's
/// `severity_overrides` TOML mechanism (per-line configuration this pilot
/// has no equivalent of), and the spec's Non-goals rule out unifying the
/// two severity models beyond areas 1-2. This is a local, DLR-only
/// constant instead.
const DLR_DELAY_THRESHOLD_MINUTES: i64 = 5;

/// How close a live prediction's `expected_arrival` must be to a scheduled
/// trip's `scheduled_departure` at the same station to count as a match.
/// Wide enough to tolerate a train running early or a schedule/prediction
/// clock skew, narrow enough that two distinct trips ~4-10 minutes apart
/// (DLR's typical headway) don't both claim the same prediction.
const MATCH_WINDOW_MINUTES: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchedTrip {
    /// Found a prediction within `MATCH_WINDOW_MINUTES` of this trip's
    /// scheduled time. `delay_minutes` can be negative (early) but callers
    /// treat negative as zero — see `Task 6`'s `SampleStats` computation.
    Matched { delay_minutes: i64 },
    /// No matching prediction yet. Not necessarily cancelled — the train
    /// may simply not be visible in predictions yet if its scheduled time
    /// hasn't arrived. Task 6 decides when "pending" becomes "cancelled".
    Pending,
}

/// For each scheduled trip, finds the live prediction (at the same
/// station) whose `expected_arrival` is closest to `scheduled_departure`
/// and within `MATCH_WINDOW_MINUTES`, and computes its delay. Each
/// prediction can match at most one trip — the closest trip claims it,
/// so two trips near the same time don't both consume one late train's
/// prediction as evidence they individually ran on time.
pub fn match_trips(
    trips: Vec<ScheduledTrip>,
    predictions: &[Prediction],
    now: DateTime<Utc>,
) -> Vec<MatchedTrip> {
    let mut claimed = vec![false; predictions.len()];
    trips
        .into_iter()
        .map(|trip| {
            let best = predictions
                .iter()
                .enumerate()
                .filter(|(i, _)| !claimed[*i])
                .map(|(i, p)| (i, p, (p.expected_arrival - trip.scheduled_departure).num_minutes().abs()))
                .filter(|(_, _, diff)| *diff <= MATCH_WINDOW_MINUTES)
                .min_by_key(|(_, _, diff)| *diff);

            match best {
                Some((i, p, _)) => {
                    claimed[i] = true;
                    let delay_minutes = (p.expected_arrival - trip.scheduled_departure).num_minutes();
                    MatchedTrip::Matched { delay_minutes: delay_minutes.max(0) }
                }
                None => {
                    let _ = now; // `now` is unused by matching itself; kept in the signature for Task 6's cancellation check, which needs it and calls this function.
                    MatchedTrip::Pending
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trip(minute: u32) -> ScheduledTrip {
        ScheduledTrip {
            scheduled_departure: "2026-08-22T10:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::minutes(minute as i64),
            interval_id: None,
        }
    }

    fn prediction(expected_offset_minutes: i64) -> Prediction {
        Prediction {
            vehicle_id: "301".to_string(),
            naptan_id: "940GZZDLPOP".to_string(),
            station_name: "Poplar".to_string(),
            destination_naptan_id: String::new(),
            destination_name: String::new(),
            expected_arrival: "2026-08-22T10:00:00Z".parse::<DateTime<Utc>>().unwrap()
                + chrono::Duration::minutes(expected_offset_minutes),
            time_to_station: 0,
        }
    }

    #[test]
    fn an_on_time_prediction_matches_with_zero_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }]);
    }

    #[test]
    fn a_late_prediction_matches_with_the_observed_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(6)], "2026-08-22T10:00:00Z".parse().unwrap());
        // 6 minutes is outside MATCH_WINDOW_MINUTES (3), so this should be
        // Pending, not a 6-minute-late match — window too tight to claim a
        // 6-minute-late train as "this trip" rather than a later one.
        assert_eq!(result, vec![MatchedTrip::Pending]);
    }

    #[test]
    fn a_prediction_within_the_match_window_computes_its_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(2)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 2 }]);
    }

    #[test]
    fn a_trip_with_no_nearby_prediction_is_pending() {
        let result = match_trips(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Pending]);
    }

    #[test]
    fn each_prediction_matches_at_most_one_trip() {
        // Two trips 4 minutes apart (DLR's typical headway), one
        // prediction. The closer trip claims it; the other stays Pending
        // rather than both being marked on-time from the same train.
        let result = match_trips(vec![trip(0), trip(4)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }, MatchedTrip::Pending]);
    }

    #[test]
    fn an_early_prediction_is_clamped_to_zero_delay_not_negative() {
        let result = match_trips(vec![trip(2)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p poller-tfl dlr::inference -- --nocapture`
Expected: FAILS to compile (stub has no content).

- [ ] **Step 3: Confirm the implementation above compiles and passes**

Run: `cargo test -p poller-tfl dlr::inference -- --nocapture`
Expected: all 6 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/poller-tfl/src/dlr/inference.rs
git commit -m "feat(poller-tfl): match DLR scheduled trips to live predictions"
```

---

## Task 6: Cross-poll cancellation tracking and `SampleStats` aggregation

Extends `inference.rs` with the stateful half: a trip that stays `Pending` past a grace window is a cancellation candidate, and resolved trips (matched or cancelled) roll up into a `SampleStats`. State lives only in the running `poller-tfl` process — a restart just drops in-flight trips and the pilot starts fresh on the next window; nothing is persisted, no schema change.

**Files:**
- Modify: `crates/poller-tfl/src/dlr/inference.rs` (add to the same file)

**Interfaces:**
- Consumes: `Vec<MatchedTrip>` and the originating `Vec<ScheduledTrip>` from Task 5's `match_trips`.
- Produces: `pub struct DlrMatchState { .. }` with `pub fn new() -> Self`, `pub fn resolve(&mut self, trips: Vec<ScheduledTrip>, predictions: &[Prediction], now: DateTime<Utc>) -> Option<common::SampleStats>` — Task 7 owns one `DlrMatchState` for the poller's whole lifetime and calls `resolve` once per poll cycle.

- [ ] **Step 1: Write the failing test**

Append to `crates/poller-tfl/src/dlr/inference.rs` (above the existing `#[cfg(test)] mod tests`, add these before the closing brace of that module — or extend the `use` list and add alongside the existing test functions):

```rust
/// A scheduled trip still waiting for a matching prediction, remembered
/// across poll cycles so `resolve` can tell "genuinely never showed up"
/// from "hasn't happened yet".
#[derive(Debug, Clone)]
struct PendingTrip {
    scheduled_departure: DateTime<Utc>,
}

/// A trip that has been resolved one way or the other, kept for
/// `RESOLVED_RETENTION_MINUTES` so `SampleStats` reflects a rolling recent
/// window rather than every trip since the poller started.
#[derive(Debug, Clone)]
struct ResolvedTrip {
    resolved_at: DateTime<Utc>,
    delay_minutes: Option<i64>, // None means cancelled
}

/// A trip pending longer than this past its scheduled time, with no
/// matching prediction ever found, is treated as cancelled. DLR's typical
/// headway is 3-10 minutes; this is roughly two headways' grace so a
/// train that's simply running very late doesn't get misread as
/// cancelled — a pilot-tuned value, not derived from any published TfL
/// number, and worth revisiting once real data is observed.
const CANCELLATION_GRACE_MINUTES: i64 = 15;

/// How long a resolved trip counts toward the reported `SampleStats`
/// before aging out. An hour gives a stable-enough sample size at DLR's
/// headway (roughly 6-20 trips) without the reported numbers describing
/// disruption from hours ago as if it were still happening.
const RESOLVED_RETENTION_MINUTES: i64 = 60;

pub struct DlrMatchState {
    pending: Vec<PendingTrip>,
    resolved: Vec<ResolvedTrip>,
}

impl DlrMatchState {
    pub fn new() -> Self {
        DlrMatchState { pending: Vec::new(), resolved: Vec::new() }
    }

    /// Runs one poll cycle: adds newly-seen scheduled trips to the pending
    /// set (skipping ones already tracked, by `scheduled_departure`),
    /// matches everything pending against this cycle's predictions,
    /// promotes newly-matched or grace-window-expired trips into
    /// `resolved`, evicts resolved trips older than
    /// `RESOLVED_RETENTION_MINUTES`, and returns the resulting
    /// `SampleStats` — `None` if nothing has resolved yet (e.g. right
    /// after startup).
    pub fn resolve(
        &mut self,
        trips: Vec<ScheduledTrip>,
        predictions: &[Prediction],
        now: DateTime<Utc>,
    ) -> Option<common::SampleStats> {
        let known: std::collections::HashSet<DateTime<Utc>> =
            self.pending.iter().map(|t| t.scheduled_departure).collect();
        for trip in trips.iter().filter(|t| !known.contains(&t.scheduled_departure)) {
            self.pending.push(PendingTrip { scheduled_departure: trip.scheduled_departure });
        }

        let pending_as_trips: Vec<ScheduledTrip> = self
            .pending
            .iter()
            .map(|p| ScheduledTrip { scheduled_departure: p.scheduled_departure, interval_id: None })
            .collect();
        let matches = match_trips(pending_as_trips, predictions, now);

        let mut still_pending = Vec::new();
        for (pending_trip, matched) in self.pending.drain(..).zip(matches) {
            match matched {
                MatchedTrip::Matched { delay_minutes } => {
                    self.resolved.push(ResolvedTrip { resolved_at: now, delay_minutes: Some(delay_minutes) });
                }
                MatchedTrip::Pending => {
                    let overdue = (now - pending_trip.scheduled_departure).num_minutes();
                    if overdue >= CANCELLATION_GRACE_MINUTES {
                        self.resolved.push(ResolvedTrip { resolved_at: now, delay_minutes: None });
                    } else {
                        still_pending.push(pending_trip);
                    }
                }
            }
        }
        self.pending = still_pending;

        self.resolved.retain(|r| (now - r.resolved_at).num_minutes() < RESOLVED_RETENTION_MINUTES);

        if self.resolved.is_empty() {
            return None;
        }

        let total = self.resolved.len();
        let cancelled = self.resolved.iter().filter(|r| r.delay_minutes.is_none()).count();
        let running: Vec<i64> = self.resolved.iter().filter_map(|r| r.delay_minutes).collect();
        let delayed = running.iter().filter(|&&d| d >= DLR_DELAY_THRESHOLD_MINUTES).count();
        let avg_delay_minutes = if running.is_empty() {
            0.0
        } else {
            running.iter().sum::<i64>() as f64 / running.len() as f64
        };

        Some(common::SampleStats { total, delayed, cancelled, skipped: 0, avg_delay_minutes })
    }
}

impl Default for DlrMatchState {
    fn default() -> Self {
        Self::new()
    }
}
```

Add these tests inside the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn a_matched_trip_resolves_immediately() {
    let mut state = DlrMatchState::new();
    let stats = state
        .resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap())
        .expect("one resolved trip");
    assert_eq!(stats.total, 1);
    assert_eq!(stats.cancelled, 0);
    assert_eq!(stats.delayed, 0);
}

#[test]
fn a_trip_still_pending_within_the_grace_window_produces_no_stats_yet() {
    let mut state = DlrMatchState::new();
    let stats = state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
    assert_eq!(stats, None);
}

#[test]
fn a_trip_still_unmatched_past_the_grace_window_is_cancelled() {
    let mut state = DlrMatchState::new();
    // First cycle: trip scheduled for 10:00, no prediction yet.
    state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
    // Second cycle, 16 minutes later: still nothing, past the 15-minute
    // grace window.
    let stats = state
        .resolve(vec![], &[], "2026-08-22T10:16:00Z".parse().unwrap())
        .expect("the overdue trip should have resolved as cancelled");
    assert_eq!(stats.total, 1);
    assert_eq!(stats.cancelled, 1);
}

#[test]
fn resolved_trips_age_out_after_the_retention_window() {
    let mut state = DlrMatchState::new();
    state.resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
    // 61 minutes later, with no new trips at all: the earlier resolved
    // trip should have aged out, leaving nothing to report.
    let stats = state.resolve(vec![], &[], "2026-08-22T11:01:00Z".parse().unwrap());
    assert_eq!(stats, None);
}

#[test]
fn a_trip_already_pending_is_not_added_twice_on_the_next_cycle() {
    let mut state = DlrMatchState::new();
    state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
    // Same trip handed in again next cycle (the timetable poll always
    // returns the same day's full schedule) — must not be double-counted.
    let stats = state
        .resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:01:00Z".parse().unwrap())
        .expect("the trip should resolve exactly once");
    assert_eq!(stats.total, 1);
}
```

(`common::SampleStats` needs `PartialEq` derived to support the `assert_eq!(stats, None)` calls above — check `crates/common/src/lib.rs:502`: it already derives `PartialEq`, so no change needed there.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p poller-tfl dlr::inference -- --nocapture`
Expected: FAILS to compile (`DlrMatchState` doesn't exist until Step 1's code is added — this task's Step 1 already contains the implementation, so compile and move to Step 3 directly if using this plan as written; if implementing test-first literally, stub `DlrMatchState` as `todo!()` first, confirm the failure, then paste the real body).

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p poller-tfl dlr::inference -- --nocapture`
Expected: all tests in the module PASS (11 total: 6 from Task 5, 5 new).

- [ ] **Step 4: Commit**

```bash
git add crates/poller-tfl/src/dlr/inference.rs
git commit -m "feat(poller-tfl): track DLR trips across polls and compute sample stats"
```

---

## Task 7: Wire DLR inference into the poll loop

**Files:**
- Modify: `crates/poller-tfl/src/main.rs`
- Modify: `crates/poller-tfl/src/config.rs`

**Interfaces:**
- Consumes: `dlr::arrivals::parse_arrivals`, `dlr::timetable::parse_timetable`, `dlr::inference::DlrMatchState` (Tasks 3-6); `common::TFL_LINE_ID_PREFIX` (already used elsewhere in this file).
- Produces: the DLR entry in the `Vec<LineStatusReport>` posted each cycle now carries `sample_stats: Some(..)` on every one of its `statuses` entries once at least one trip has resolved.

**Design decision — one poller, one interval, not two.** The spec's design sketch floats a 30-60s interval "candidate" for the Arrivals poll; this task keeps it on `poller-tfl`'s existing 300s `poll_interval_secs` instead, running in the *same* loop as the existing `/Line/Mode/{modes}/Status` call rather than adding a second concurrent polling loop with its own interval and shared mutable state. Reasoning: this pilot's cancellation/delay signal already accumulates across *multiple* poll cycles via `DlrMatchState` (Task 6), not from a single tight-interval sample — a 300s cadence sacrifices some resolution in exactly when a delay becomes "visible," but does not change whether a trip eventually resolves as matched/cancelled. Keeping one interval avoids introducing concurrency (two async loops racing to build one `LineStatusReport`) into a crate that is currently a single sequential `loop { }`. If, once real data is observed, resolution turns out to matter, tightening the interval is a config-only follow-up (`--poll-interval-secs`), not a redesign.

**Design decision — same crate, new module, not a new poller binary.** `poller-tfl` already owns the DLR line's `LineStatusReport` (from the existing `/Line/Mode/{modes}/Status` call) and already posts it to `/private/tfl-line-status`. A second, independent poller writing to the same `line_id = "tfl-dlr"` row would race with this one on every cycle — whichever posts last wins, and if their intervals ever drift apart, one write could silently clobber the other's severity/reason with stale values while updating `sample_stats`, or vice versa. Computing both in one process, merging them into one `LineStatusReport` before the single POST, avoids that hazard entirely.

- [ ] **Step 1: Add config**

In `crates/poller-tfl/src/config.rs`, add a field so the pilot can be disabled without a redeploy if it misbehaves in production:

```rust
    /// Enables the DLR arrivals-diffing pilot (see
    /// `docs/superpowers/plans/2026-08-22-dlr-arrivals-diffing-pilot.md`).
    /// Defaults on; set to `false` to fall back to `sample_stats: None`
    /// for DLR, same as every other TfL line, without a redeploy.
    #[arg(long, env, default_value_t = true)]
    pub dlr_pilot_enabled: bool,

    /// Poplar's Naptan id, used as the `stopPointId` for the DLR
    /// Timetable poll. Not derived — this pilot covers one fixed station
    /// only (see the plan's Global Constraints).
    #[arg(long, env, default_value = "940GZZDLPOP")]
    pub dlr_pilot_stop_point_id: String,
```

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/poller-tfl/src/main.rs`:

```rust
#[test]
fn dlr_sample_stats_are_merged_onto_the_matching_line_only() {
    let mut reports = vec![
        common::LineStatusReport {
            id: "tfl-dlr".to_string(),
            name: "DLR".to_string(),
            mode_name: "dlr".to_string(),
            operators: vec!["TfL".to_string()],
            statuses: vec![common::LineStatus {
                severity: common::Severity::GoodService,
                reason: "Good Service".to_string(),
                validity: common::ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
                disruption: None,
                data_quality: common::DataQuality::Tfl,
                sample_stats: None,
            }],
        },
        common::LineStatusReport {
            id: "tfl-victoria".to_string(),
            name: "Victoria".to_string(),
            mode_name: "tube".to_string(),
            operators: vec!["TfL".to_string()],
            statuses: vec![common::LineStatus {
                severity: common::Severity::GoodService,
                reason: "Good Service".to_string(),
                validity: common::ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
                disruption: None,
                data_quality: common::DataQuality::Tfl,
                sample_stats: None,
            }],
        },
    ];
    let stats = common::SampleStats { total: 10, delayed: 2, cancelled: 1, skipped: 0, avg_delay_minutes: 3.5 };

    merge_dlr_sample_stats(&mut reports, stats.clone());

    assert_eq!(reports[0].statuses[0].sample_stats, Some(stats));
    assert_eq!(reports[1].statuses[0].sample_stats, None);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p poller-tfl dlr_sample_stats_are_merged -- --nocapture`
Expected: FAILS to compile (`merge_dlr_sample_stats` doesn't exist).

- [ ] **Step 4: Implement**

Add to `crates/poller-tfl/src/main.rs`, and update `poll_once`:

```rust
/// Attaches `stats` to every status entry on the `tfl-dlr` line only —
/// mirrors the aggregator's own attach-to-every-status-on-the-line
/// pattern (`crates/aggregator/src/aggregation.rs:96-106`), minus its
/// severity escalation, which this pilot deliberately does not adopt (see
/// the plan's Global Constraints).
fn merge_dlr_sample_stats(reports: &mut [common::LineStatusReport], stats: common::SampleStats) {
    for report in reports.iter_mut().filter(|r| r.id == "tfl-dlr") {
        for status in &mut report.statuses {
            status.sample_stats = Some(stats.clone());
        }
    }
}

async fn poll_dlr_sample_stats(
    client: &Client,
    config: &Config,
    state: &mut dlr::inference::DlrMatchState,
) -> anyhow::Result<Option<common::SampleStats>> {
    let arrivals_url = format!("{}/Line/dlr/Arrivals", config.tfl_base_url.trim_end_matches('/'));
    let arrivals_body = client
        .get(&arrivals_url)
        .header(TFL_AUTH_HEADER_NAME, &config.tfl_app_key)
        .send()
        .await?
        .text()
        .await?;
    let predictions = dlr::arrivals::parse_arrivals(&arrivals_body)?;

    let timetable_url = format!(
        "{}/Line/dlr/Timetable/{}",
        config.tfl_base_url.trim_end_matches('/'),
        config.dlr_pilot_stop_point_id
    );
    let timetable_body = client
        .get(&timetable_url)
        .header(TFL_AUTH_HEADER_NAME, &config.tfl_app_key)
        .send()
        .await?
        .text()
        .await?;
    let now = Utc::now();
    let trips = dlr::timetable::parse_timetable(&timetable_body, now.date_naive())?;

    Ok(state.resolve(trips, &predictions, now))
}
```

Change `poll_once`'s signature and body:

```rust
async fn poll_once(client: &Client, config: &Config, dlr_state: &mut dlr::inference::DlrMatchState) -> anyhow::Result<()> {
    let body = fetch_status_json(client, config).await?;
    let mut reports = schema::parse_line_status(&body, Utc::now())?;

    if reports.is_empty() {
        anyhow::bail!("TfL returned no lines for modes {}; refusing to post an empty batch", config.tfl_modes);
    }

    if config.dlr_pilot_enabled {
        match poll_dlr_sample_stats(client, config, dlr_state).await {
            Ok(Some(stats)) => merge_dlr_sample_stats(&mut reports, stats),
            Ok(None) => {}
            Err(err) => {
                // The DLR pilot failing must never take down the rest of
                // the TfL line-status batch — log and post everything
                // else as normal, same as any other line keeps reporting
                // if one call in a multi-call cycle has a bad day.
                tracing::warn!(error = ?err, "DLR arrivals-diffing pilot failed this cycle; continuing without it");
            }
        }
    }

    tracing::info!(count = reports.len(), "parsed line statuses from TfL");

    ingest::post_batch(client, &config.api_ingest_url, &config.internal_token, &reports, "TfL line statuses").await
}
```

And in `main()`, thread a `dlr_state` through the loop:

```rust
    let mut dlr_state = dlr::inference::DlrMatchState::new();
    loop {
        interval.tick().await;

        if let Err(err) = poll_once(&client, &config, &mut dlr_state).await {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p poller-tfl -- --nocapture`
Expected: every test in the crate passes, including the new one from Step 2.

- [ ] **Step 6: Full workspace build**

Run: `cargo build --workspace && cargo clippy --workspace -- -D warnings`
Expected: clean build, no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/poller-tfl/src/main.rs crates/poller-tfl/src/config.rs
git commit -m "feat(poller-tfl): wire DLR arrivals-diffing pilot into the poll loop"
```

---

## Task 8: Manual verification against the real pipeline

This codebase has no Docker/Postgres available in the sandbox this plan was written in (the same constraint the original TfL line-status integration hit — see `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`'s manual-verification note), so end-to-end confirmation is a manual checklist for whoever has a real deployment, not an automated test.

**Files:** none (verification only).

- [ ] **Step 1: Confirm the frontend needs no changes**

`frontend/components/RepresentativeInfo.tsx:9-10` renders the aggregate stats line whenever any status carries `sampleStats`, with no `dataQuality` gating:

```ts
const withStats = statuses.find((status) => status.sampleStats);
if (!withStats?.sampleStats) return null;
```

Confirm this is still true (re-read the file) before relying on it — if it changed since this plan was written, populating `sample_stats` for DLR alone won't be enough and a small frontend task would need to be added here.

- [ ] **Step 2: Deploy to a real environment with `TFL_APP_KEY` set and `dlr_pilot_enabled=true`**

Run `poller-tfl` against the live TfL API for at least `RESOLVED_RETENTION_MINUTES` (60 minutes) of wall-clock time, so at least one `SampleStats` has had a chance to resolve.

- [ ] **Step 3: Check `line_status_history` growth for `tfl-dlr`**

```sql
SELECT count(*) FROM line_status_history WHERE line_id = 'tfl-dlr' AND computed_at > now() - interval '1 hour';
```

Expected: a small number of rows (one per genuine severity/reason change TfL itself reported), **not** one row per poll cycle (every ~300s = up to 12/hour). A row-per-cycle count here means Task 1's guard isn't working as intended — stop the pilot (`dlr_pilot_enabled=false`) and re-check `tfl_statuses_changed`/`normalize_for_diff` before re-enabling.

- [ ] **Step 4: Check the DLR line's public API response**

```bash
curl -s https://<your-deployment>/public/lines/tfl-dlr | jq '.statuses[0].sample_stats'
```

Expected: a non-null object with `total`/`delayed`/`cancelled`/`skipped`/`avg_delay_minutes`, `skipped` always `0`.

- [ ] **Step 5: Visually confirm the frontend renders it**

Load the DLR line's detail page in a browser; confirm `RepresentativeInfo` shows the aggregate line (e.g. "X of Y sampled services delayed, Z cancelled, avg N.N min late") the same way it does for an NR line.

- [ ] **Step 6: Record findings**

Add a short note to this plan file's Task 8 (or a follow-up doc) on whether the numbers looked sane against what TfL's own DLR status page reports for the same period — this is the actual validation the spec's "pilot" framing is about; if the numbers look implausible (e.g. near-100% "delayed" due to `MATCH_WINDOW_MINUTES`/`CANCELLATION_GRACE_MINUTES` being miscalibrated), tune the constants in `crates/poller-tfl/src/dlr/inference.rs` and redeploy before treating this as validated.

---

## Explicitly not in this plan

Matching this plan's Global Constraints and the spec's own scope: no expansion beyond Poplar to other DLR stations or branches, no Tube/Tram work, no Overground work, no Elizabeth line work (a separate plan covers that merge), no severity escalation from the inferred stats, no persistence of `DlrMatchState` across restarts. All of these are legitimate follow-ups once this pilot's numbers have been validated per Task 8 — none of them are needed to answer the question this pilot exists to answer: does Arrivals-vs-Timetable diffing produce a trustworthy signal for DLR at all.
