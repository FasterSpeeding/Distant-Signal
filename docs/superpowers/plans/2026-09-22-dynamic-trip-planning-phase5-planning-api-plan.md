# Plan: Dynamic Trip Planning — Phase 5: Read-Only Planning API

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 5 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8: a
new, read-only `GET /Trips/plan` endpoint that runs Phase 3's Connection
Scan (`results=fastest`) or Phase 4's RAPTOR (`results=options`) against
Phase 2's per-query connections-array build, over ordered waypoints, and
returns candidate itineraries in a shape the frontend (Phase 6) can render
and then commit via the *existing, unmodified* `POST /Journeys`/
`POST /Journeys/{id}/legs` machinery. **No frontend yet.**

**Architecture:** one new small data-layer module
(`crates/api/src/data/trip_planning_itinerary.rs`, Task 1) turns a raw
`trip_planner::Journey`/`RaptorJourney` (TIPLOC-keyed, minutes-from-midnight)
into a CRS/human-time-keyed wire shape, chains multiple waypoint segments
together, and applies this feature's ≤2-interchange cap (§4) — a
presentation-layer concern neither `scan_connections` nor `raptor_search`
themselves know about, per Phase 3/4's own Judgment Calls. One new route
file (`crates/api/src/routes/trips.rs`, Task 2) exposes it as
`GET /Trips/plan`, mounted directly in `main.rs` exactly like
`routes::journeys::router()` already is.

**Tech stack:** Rust/axum (`crates/api`), depending on `trip-planner`
(Phase 3/4) and `crates/schedule-query` (Phase 2) directly.

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§4, §5, §7 Open Question 2 (naming), §8 Phase 5. Depends on Phases 1-4 of
this same plan series. This plan resolves Open Question 2 with a concrete
answer (Judgment Call 1) and applies Phase 4's own Judgment Call 2 warning
about `max_rounds` concretely (Judgment Call 2 below).

---

## Judgment calls this plan makes (read before Task 1)

1. **Resolves Open Question 2: the route is `GET /Trips/plan`, under a new
   `/Trips` prefix, never `/Journeys/*`.** The design spec's own §0.7
   already names the exact naming collision this avoids — two existing
   specs use "trip search" for a *different*, single-hop feature
   (`TrackTrainForm`'s own picker), and this document's own §0.7
   recommends "Plan a trip"/"Route planner" in user-facing copy specifically
   to not collide with that prior usage. `/Journeys/*` is reserved for the
   *tracking* data model (a real, already-committed `journeys`/
   `journey_legs` row); `/Trips/plan` is a read-only computation over
   *hypothetical* itineraries that have not been committed to anything yet
   — conflating the two prefixes would make "is this a real tracked journey
   or a proposed one" a URL-reading exercise instead of an obvious fact.
2. **`max_rounds` for the `options`-mode RAPTOR call is computed as
   `MAX_CHANGES + 2`, never RAPTOR's own library default, per Phase 4's own
   Judgment Call 2 warning.** `MAX_CHANGES = 2` (design spec §4's hard cap)
   ⇒ `max_rounds = 4` — enough rounds to find every itinerary within the
   cap (round 3 = "at most 2 changes") **plus one further round of
   headroom** to detect whether the cap actually bound the answer (a
   strictly better, more-changes itinerary existing just past the cap),
   mirroring the sibling `Distant-Signal-MCP` project's own hard-won fix
   for exactly this gap (re-confirmed this pass by reading
   `src/tools/plan-journey.ts:60-102` directly — see Phase 4's own Judgment
   Call 2 for the full history). If that headroom round finds a strictly
   earlier arrival than anything within the cap, the response sets
   `cappedByMaxChanges: true` on that segment (Task 1) rather than silently
   hiding the fact that a faster, more-changes itinerary exists.
3. **`results=fastest` (CSA) has no changes cap applied to the search
   itself — matching Phase 3's own Judgment Call 4 (CSA has no
   interchange-count concept at all) — but the RESPONSE flags
   `exceedsRecommendedChanges: true` when the unconstrained-fastest answer
   needs more than 2 changes, rather than silently hiding a real, honestly
   fastest route or silently pretending v1's cap is a hard search
   constraint it structurally isn't for this mode.** This is an honest,
   disclosed exception to §4's "hard cap of at most 2 interchanges,"
   consistent with this feature's own stated posture throughout (§4: "flagged,
   not silently handled"). A future pass could instead re-run CSA
   with a changes-aware variant if real usage shows this exception matters
   in practice; not attempted here since CSA has no such variant today and
   inventing one is out of this phase's scope.
4. **Ordered waypoints are solved as fully independent sub-journeys, each
   over the SAME service date's connections array, chained by concatenating
   legs — never re-optimized across the whole trip.** Directly implements
   §4's own resolved scope ("start→waypoint₁, waypoint₁→waypoint₂, ...,
   waypointₙ→finish, each independently solved... **Not** a
   traveling-salesman-style... problem"). The per-segment ≤2-change cap
   (Judgment Call 2/3) applies to EACH segment independently, not to the
   whole multi-waypoint trip's total change count — an honest reading of
   "per computed itinerary" that keeps each segment's own search bounded,
   matching how `journey_legs` itself already treats one leg's own window
   search as independent of its neighbors (§0.8).
5. **No midnight-rollover special-casing beyond what Phase 2's
   `day_offset`-aware `Connection.arrival_min` already provides.** The
   design spec's §4 "single service day only... out of scope" constraint
   refers to searching across TWO separately-resolved calendar dates'
   connections arrays (never attempted here — one `GET /Trips/plan` call
   builds exactly one date's connections array, per Judgment Call 4's own
   "same service date" framing) — it does not mean a late-evening service
   whose OWN calling points fall after local midnight (a real, correctly
   `day_offset`-tagged shape Phase 2 already handles, see that plan's
   Judgment Call 5) must be rejected. No itinerary-level "arrival is past
   midnight, reject this" check is added; it would incorrectly reject
   perfectly ordinary late services.

---

## Non-goals

- **No frontend** — Phase 6.
- **No route constraints (`via`/`avoid`/`viaStop`/`avoidStop`)** — future
  work per the design spec's §1/§6.
- **No "commit this itinerary" write endpoint** — §5.1 is explicit that a
  picked itinerary drives the *existing*, unmodified `POST /Journeys` +
  `POST /Journeys/{id}/legs` calls directly; this phase adds no new write
  route.
- **No walking-transfer `journey_legs` row of its own** — §5.1 point 5's
  own open question (does a cross-station walk get its own minimal
  `journey_legs`-adjacent record) stays unresolved; this phase's own
  response shape represents a transfer leg in the read-only itinerary
  response only (Task 1's `PlannedLeg::Transfer` variant), with no
  corresponding write path implied or required.
- **No caching of `GET /Trips/plan` responses** — every call rebuilds the
  connections array fresh (Phase 2's own "build fresh per query, discard"
  design); this phase does not add a cache layer on top, consistent with
  not yet having real usage data to size one against (design spec §3's own
  "measure before ruling anything out" posture).

## Global Constraints

- **File scope.** Created:
  `crates/api/src/data/trip_planning_itinerary.rs` (new),
  `crates/api/src/routes/trips.rs` (new).
  Modified: `crates/api/src/data/mod.rs`, `crates/api/src/main.rs`,
  `crates/api/Cargo.toml` (add `trip-planner` as a dependency).
- **Testing.** `cargo fmt --all`, `cargo clippy --workspace --all-features
  --all-targets -- -D warnings`, `cargo test --workspace` (ignored tests
  skipped), `cargo test -p api -- --ignored --test-threads=1` for the
  DB-gated HTTP tests this plan adds — all four exact CI invocations.
- **Response field naming.** `camelCase` on the wire, matching every other
  route in this crate (`#[serde(rename_all = "camelCase")]`); every new
  response type follows the existing `journeys.rs`/`train.rs` convention of
  a private, route-local struct rather than a shared `common::` type
  (nothing outside `crates/api` needs these shapes).
- **404-never-403 / honest-gap conventions.** No leg of this feature is
  user-owned data, so there is no ownership check to make — but the
  "no CIF-derived schedule data has been published for this date yet"
  case (Phase 2's `fetch_calling_points_for_date` returning `None`) must
  produce the same kind of honest, human-readable 404 message
  `search_journey_leg_candidates`'s own `None` case already establishes,
  not a bare empty itinerary list indistinguishable from "genuinely no
  route exists."

## Review Focus

- **A query whose origin or destination CRS resolves to zero TIPLOCs**
  (a typo'd or non-existent CRS) — must be a clear 400, not an empty
  itinerary list that looks like "no route found."
- **A query for a date with no published `schedule_calling_points_full`
  rows at all** (too far in the future, or a delivery gap) — must be a
  distinct, honestly-worded 404, not conflated with "no route exists for
  this real date."
- **`results=options` where every found itinerary needs more than 2
  changes** — the response must say so plainly (empty `itineraries` plus
  `cappedByMaxChanges: true` if the headroom round found something), not
  silently return nothing with no explanation.
- **A multi-waypoint request where one segment fails while an earlier
  segment succeeded** — the response must name WHICH segment failed (its
  own origin/destination), not report a single, undifferentiated "no route
  found" for the whole multi-leg request.
- **`results` parameter with a value that is neither `fastest` nor
  `options`** — a clear 400 naming the two valid values, not a silent
  fallback to one of them.

---

## Task 1: `data/trip_planning_itinerary.rs` — itinerary shape + waypoint chaining

**Files:**
- Create: `crates/api/src/data/trip_planning_itinerary.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/Cargo.toml`

**Interfaces:**
- Produces: `PlannedItinerary`, `PlannedLeg`, `plan_segment`,
  `plan_via_waypoints` — consumed by Task 2's route handler.
- Consumes: `trip_planner::{scan_connections, raptor_search, ScanOptions,
  RaptorOptions, Journey, RaptorJourney, JourneyLeg}` (Phase 3/4),
  `schedule_query::{Connection, InterchangeData}` (Phase 2),
  `crate::data::trip_planning::{build_connections_for_date,
  fetch_interchange_data}` (Phase 2's own `api`-side glue).

- [ ] **Step 1: Add the `trip-planner` dependency**

```toml
# crates/api/Cargo.toml, in [dependencies]
trip-planner = { path = "../trip-planner" }
```

- [ ] **Step 2: Write the wire-shaped itinerary types and the cap constant**

```rust
//! Turns a raw `trip_planner::Journey`/`RaptorJourney` (TIPLOC-keyed,
//! minutes-from-midnight) into the CRS/human-time-keyed shape
//! `routes::trips` serializes, applies this feature's ≤2-interchange cap
//! (design spec §4), and chains ordered waypoints into independently-solved
//! sub-journeys (design spec §4: "Not a traveling-salesman-style... problem").
//! See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Calls for the reasoning behind the cap-enforcement and
//! waypoint-chaining choices below.

use chrono::{NaiveDate, NaiveTime};
use schedule_query::InterchangeData;
use trip_planner::{JourneyLeg, RaptorJourney};

/// Design spec §4's hard cap, at most 2 interchanges (3 legs) per computed
/// itinerary -- applied here, in the presentation layer, never inside
/// `scan_connections`/`raptor_search` themselves (Phase 3/4's own Judgment
/// Calls: neither algorithm has an interchange-count concept built in).
pub const MAX_CHANGES: u32 = 2;

/// Phase 4's own Judgment Call 2: `max_rounds` for RAPTOR is NEVER the
/// library's own default -- `MAX_CHANGES + 1` trips needed for
/// `MAX_CHANGES` changes, plus one further round of headroom to detect
/// whether the cap actually bound the answer.
pub const MAX_ROUNDS: u32 = MAX_CHANGES + 2;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PlannedLeg {
    #[serde(rename_all = "camelCase")]
    Train {
        train_uid: String,
        service_date: NaiveDate,
        origin_crs: Option<String>,
        destination_crs: Option<String>,
        /// Local civil clock time, matching this app's established
        /// "CIF times are Europe/London local, rendered as HH:MM, never
        /// converted to UTC on this wire shape" convention (e.g.
        /// `schedule_query::ScheduleDeparture::scheduled`).
        scheduled_departure: NaiveTime,
        scheduled_arrival: NaiveTime,
        /// How many calendar days past `service_date` `scheduled_arrival`
        /// actually falls on -- same field, same meaning, as
        /// `schedule_query::records::CallingPoint::day_offset`.
        arrival_day_offset: u8,
    },
    #[serde(rename_all = "camelCase")]
    Transfer {
        mode: String,
        origin_crs: Option<String>,
        destination_crs: Option<String>,
        minutes: i32,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItinerary {
    pub legs: Vec<PlannedLeg>,
    pub change_count: u32,
    pub total_duration_minutes: u32,
    /// `results=fastest` only -- see this plan's Judgment Call 3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exceeds_recommended_changes: Option<bool>,
}
```

- [ ] **Step 3: Write the TIPLOC→CRS leg conversion, shared by both
  algorithms' output**

```rust
fn crs_for_tiploc(interchange: &InterchangeData, tiploc: &str) -> Option<String> {
    interchange.tiploc_to_crs.get(tiploc).cloned()
}

fn minutes_to_clock(total_minutes: u32) -> (NaiveTime, u8) {
    let clock = total_minutes % 1440;
    let day_offset = (total_minutes / 1440) as u8;
    (
        NaiveTime::from_num_seconds_from_midnight_opt(clock * 60, 0)
            .expect("minutes-since-midnight modulo 1440 is always a valid clock time"),
        day_offset,
    )
}

/// Converts one `trip_planner::JourneyLeg` into a [`PlannedLeg`]. `date` is
/// the query's own overall service date -- every train leg's `service_date`
/// is that SAME date regardless of any `dayOffset` its own clock time
/// carries (a schedule's identity is `(uid, the date it was resolved
/// against)`, not the calendar date its later calling points' clock times
/// happen to read past midnight -- see this plan's Judgment Call 5, and
/// `find_or_create_train`'s own existing `(train_uid, service_date)`
/// contract, which this must match exactly for Phase 6's "commit this
/// itinerary" flow to create the right `trains` row).
fn planned_leg(leg: &JourneyLeg, date: NaiveDate, interchange: &InterchangeData) -> PlannedLeg {
    match leg {
        JourneyLeg::Train(train) => {
            let (scheduled_departure, _) = minutes_to_clock(train.departure_min);
            let (scheduled_arrival, arrival_day_offset) = minutes_to_clock(train.arrival_min);
            PlannedLeg::Train {
                train_uid: train.uid.clone(),
                service_date: date,
                origin_crs: crs_for_tiploc(interchange, &train.from_tiploc),
                destination_crs: crs_for_tiploc(interchange, &train.to_tiploc),
                scheduled_departure,
                scheduled_arrival,
                arrival_day_offset,
            }
        }
        JourneyLeg::Transfer(transfer) => PlannedLeg::Transfer {
            mode: transfer.mode.clone(),
            origin_crs: crs_for_tiploc(interchange, &transfer.from_tiploc),
            destination_crs: crs_for_tiploc(interchange, &transfer.to_tiploc),
            minutes: transfer.minutes,
        },
    }
}

fn train_leg_count(legs: &[JourneyLeg]) -> u32 {
    legs.iter().filter(|leg| matches!(leg, JourneyLeg::Train(_))).count() as u32
}
```

- [ ] **Step 4: Write `plan_segment`** — one origin→destination sub-journey,
  both `results` modes:

```rust
/// One origin->destination segment (either the whole trip, when there are
/// no waypoints, or one hop of a multi-waypoint chain). `results` is
/// `"fastest"` (CSA, Phase 3) or `"options"` (RAPTOR, Phase 4) -- validated
/// by the caller (`routes::trips`) before this is ever called; this
/// function assumes it is already one of exactly those two strings.
///
/// Returns `Ok(itineraries)` -- possibly empty, meaning "no itinerary
/// within the cap was found" (a real, distinct outcome from an error, see
/// this plan's Review Focus) -- or `Err(message)` for a caller-facing
/// validation problem (an unresolvable CRS).
pub fn plan_segment(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    date: NaiveDate,
    origin_crs: &str,
    destination_crs: &str,
    departure_after: NaiveTime,
    results: &str,
) -> Result<(Vec<PlannedItinerary>, bool), String> {
    let from_tiplocs = interchange
        .crs_to_tiplocs
        .get(&origin_crs.to_ascii_uppercase())
        .cloned()
        .unwrap_or_default();
    let to_tiplocs = interchange
        .crs_to_tiplocs
        .get(&destination_crs.to_ascii_uppercase())
        .cloned()
        .unwrap_or_default();
    if from_tiplocs.is_empty() {
        return Err(format!("'{origin_crs}' is not a recognised station CRS code"));
    }
    if to_tiplocs.is_empty() {
        return Err(format!("'{destination_crs}' is not a recognised station CRS code"));
    }

    let departure_min = {
        use chrono::Timelike;
        departure_after.num_seconds_from_midnight() / 60
    };

    if results == "fastest" {
        let Some(journey) = trip_planner::scan_connections(trip_planner::ScanOptions {
            connections,
            interchange,
            from_tiplocs: &from_tiplocs,
            to_tiplocs: &to_tiplocs,
            departure_min,
            date,
        }) else {
            return Ok((Vec::new(), false));
        };
        let change_count = train_leg_count(&journey.legs).saturating_sub(1).max(0);
        let itinerary = PlannedItinerary {
            legs: journey.legs.iter().map(|leg| planned_leg(leg, date, interchange)).collect(),
            change_count,
            total_duration_minutes: journey.arrival_min - journey.departure_min,
            // See this plan's Judgment Call 3: CSA has no cap of its own,
            // so a genuinely-fastest answer that needs more than 2 changes
            // is still returned, honestly flagged, not hidden.
            exceeds_recommended_changes: Some(change_count > MAX_CHANGES),
        };
        return Ok((vec![itinerary], false));
    }

    if results == "options" {
        let all: Vec<RaptorJourney> = trip_planner::raptor_search(trip_planner::RaptorOptions {
            connections,
            interchange,
            from_tiplocs: &from_tiplocs,
            to_tiplocs: &to_tiplocs,
            departure_min,
            date,
            max_rounds: MAX_ROUNDS,
        });
        let within_cap: Vec<&RaptorJourney> = all.iter().filter(|j| j.changes <= MAX_CHANGES).collect();
        // Judgment Call 2: did the headroom round (MAX_ROUNDS, one past
        // what MAX_CHANGES alone needs) find something strictly better
        // than every within-cap entry? If so, the cap genuinely bound the
        // answer -- flagged honestly, not silently swallowed.
        let best_within_cap = within_cap.iter().map(|j| j.arrival_min).min();
        let capped = all
            .iter()
            .any(|j| j.changes > MAX_CHANGES && best_within_cap.is_none_or(|best| j.arrival_min < best));

        let itineraries = within_cap
            .into_iter()
            .map(|journey| PlannedItinerary {
                legs: journey.legs.iter().map(|leg| planned_leg(leg, date, interchange)).collect(),
                change_count: journey.changes,
                total_duration_minutes: journey.arrival_min - journey.departure_min,
                exceeds_recommended_changes: None,
            })
            .collect();
        return Ok((itineraries, capped));
    }

    Err(format!("results must be 'fastest' or 'options', not '{results}'"))
}
```

- [ ] **Step 5: Write `plan_via_waypoints`** — chains ordered segments:

```rust
/// One resolved leg of a multi-waypoint plan -- `origin`/`destination` name
/// which CRS pair this segment was for, so a caller can report exactly
/// which segment failed (this plan's own Review Focus).
pub struct SegmentResult {
    pub origin_crs: String,
    pub destination_crs: String,
    pub itineraries: Vec<PlannedItinerary>,
    pub capped_by_max_changes: bool,
}

/// Solves `origin -> waypoints[0] -> waypoints[1] -> ... -> destination` as
/// independent segments (design spec §4: ordered, not a traveling-salesman
/// problem -- see this plan's Judgment Call 4), concatenating each
/// segment's own result rather than re-optimising across the whole trip.
/// `departure_after` applies only to the FIRST segment; each subsequent
/// segment searches from `00:00` onward on the same date -- a deliberate
/// simplification consistent with treating each hop as independently
/// solved rather than threading a "must connect after the previous
/// itinerary's own arrival" constraint through (that stronger constraint
/// is real future value, not attempted in this phase -- see this plan's
/// own Non-goals).
pub fn plan_via_waypoints(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    date: NaiveDate,
    origin_crs: &str,
    waypoints: &[String],
    destination_crs: &str,
    departure_after: NaiveTime,
    results: &str,
) -> Result<Vec<SegmentResult>, String> {
    let mut stops: Vec<&str> = vec![origin_crs];
    stops.extend(waypoints.iter().map(String::as_str));
    stops.push(destination_crs);

    let mut segments = Vec::new();
    for (index, pair) in stops.windows(2).enumerate() {
        let (from, to) = (pair[0], pair[1]);
        let segment_departure = if index == 0 { departure_after } else { NaiveTime::MIN };
        let (itineraries, capped) =
            plan_segment(connections, interchange, date, from, to, segment_departure, results)
                .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
        segments.push(SegmentResult {
            origin_crs: from.to_string(),
            destination_crs: to.to_string(),
            itineraries,
            capped_by_max_changes: capped,
        });
    }
    Ok(segments)
}
```

- [ ] **Step 6: Register the module** in `crates/api/src/data/mod.rs`
  (`pub mod trip_planning_itinerary;`).

- [ ] **Step 7: Write tests** for the cap/flagging logic (the parts of this
  file with real decision-making — the TIPLOC/CRS/time conversions are
  straightforward enough to be covered indirectly through Task 2's own
  route-level tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn interchange_with(crs_to_tiplocs: &[(&str, &str)]) -> InterchangeData {
        let mut data = InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (crs, tiploc) in crs_to_tiplocs {
            data.tiploc_to_crs.insert(tiploc.to_string(), crs.to_string());
            data.crs_to_tiplocs.entry(crs.to_string()).or_default().push(tiploc.to_string());
        }
        data
    }

    fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> schedule_query::Connection {
        schedule_query::Connection {
            uid: uid.to_string(),
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            departure_min: dep,
            arrival_min: arr,
        }
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
    }

    #[test]
    fn an_unresolvable_origin_crs_is_a_clear_error() {
        let interchange = interchange_with(&[("MKC", "MILTNKC")]);
        let result = plan_segment(&[], &interchange, date(), "ZZZ", "MKC", NaiveTime::MIN, "fastest");
        assert!(result.unwrap_err().contains("ZZZ"));
    }

    #[test]
    fn fastest_mode_flags_a_result_needing_more_than_the_cap() {
        // Three changes: EUS -> A -> B -> C -> MKC, all changes instant
        // (no change-time data), so CSA finds this as the only, fastest
        // route -- and it must still be returned, flagged.
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 490),
            conn("U2", "A", "B", 490, 500),
            conn("U3", "B", "C", 500, 510),
            conn("U4", "C", "MKC", 510, 520),
        ];
        let interchange = interchange_with(&[("EUS", "EUSTON"), ("MKC", "MKC")]);
        let (itineraries, _) =
            plan_segment(&connections, &interchange, date(), "EUS", "MKC", NaiveTime::MIN, "fastest").unwrap();
        assert_eq!(itineraries.len(), 1);
        assert_eq!(itineraries[0].change_count, 3);
        assert_eq!(itineraries[0].exceeds_recommended_changes, Some(true));
    }

    #[test]
    fn options_mode_excludes_results_over_the_cap_but_flags_when_capped() {
        let connections = vec![
            // Within-cap: 2 changes, arrives 520.
            conn("U1", "EUSTON", "A", 480, 495),
            conn("U2", "A", "B", 495, 510),
            conn("U3", "B", "MKC", 510, 520),
            // Over-cap but strictly faster: 3 changes, arrives 505.
            conn("F1", "EUSTON", "P", 480, 485),
            conn("F2", "P", "Q", 485, 490),
            conn("F3", "Q", "R", 490, 495),
            conn("F4", "R", "MKC", 495, 505),
        ];
        let interchange = interchange_with(&[("EUS", "EUSTON"), ("MKC", "MKC")]);
        let (itineraries, capped) =
            plan_segment(&connections, &interchange, date(), "EUS", "MKC", NaiveTime::MIN, "options").unwrap();
        assert!(itineraries.iter().all(|i| i.change_count <= MAX_CHANGES));
        assert!(capped, "a strictly faster, over-cap itinerary exists and must be flagged");
    }

    #[test]
    fn plan_via_waypoints_names_the_failing_segment() {
        let interchange = interchange_with(&[("EUS", "EUSTON"), ("MKC", "MKC")]);
        let err = plan_via_waypoints(
            &[],
            &interchange,
            date(),
            "EUS",
            &["ZZZ".to_string()],
            "MKC",
            NaiveTime::MIN,
            "fastest",
        )
        .unwrap_err();
        assert!(err.contains("EUS -> ZZZ"), "error must name the failing segment: {err}");
    }

    #[test]
    fn an_invalid_results_value_is_a_clear_error() {
        let interchange = interchange_with(&[("EUS", "EUSTON"), ("MKC", "MKC")]);
        let err = plan_segment(&[], &interchange, date(), "EUS", "MKC", NaiveTime::MIN, "quickest").unwrap_err();
        assert!(err.contains("fastest"));
        assert!(err.contains("options"));
    }
}
```

- [ ] **Step 8: Run the tests**

```bash
cargo test -p api trip_planning_itinerary::
```

  Expected: all 5 pass.

- [ ] **Step 9: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/data/trip_planning_itinerary.rs crates/api/src/data/mod.rs
git commit -m "api: add trip_planning_itinerary, the cap-aware waypoint-chaining presentation layer"
```

---

## Task 2: `routes/trips.rs` — `GET /Trips/plan`

**Files:**
- Create: `crates/api/src/routes/trips.rs`
- Modify: `crates/api/src/main.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Produces: `GET /Trips/plan?origin=&destination=&waypoints=&date=&departAfter=&results=`.

- [ ] **Step 1: Write the route handler**

```rust
//! `GET /Trips/plan` -- the read-only journey-planning endpoint. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §5.2
//! and
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Call 1 for why this lives under a new `/Trips` prefix, not
//! `/Journeys/*`. Unauthenticated, read-only -- computing a hypothetical
//! itinerary commits nothing and belongs to no user, matching
//! `reference::nearest_stations`'s own public/read-only posture, not
//! `routes::journeys`'s authenticated-write one.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;

use crate::app::App;
use crate::data::{trip_planning, trip_planning_itinerary};

pub fn router() -> crate::app::Router {
    crate::app::Router::new().route("/Trips/plan", axum::routing::get(get_trip_plan))
}

#[derive(Debug, Deserialize)]
struct TripPlanParams {
    origin: String,
    destination: String,
    /// Comma-separated ordered CRS codes, e.g. `?waypoints=YRK,NCL`. Absent
    /// or empty means no waypoints -- a direct origin->destination plan.
    #[serde(default)]
    waypoints: Option<String>,
    date: NaiveDate,
    #[serde(default)]
    depart_after: Option<NaiveTime>,
    #[serde(default = "default_results")]
    results: String,
}

fn default_results() -> String {
    "fastest".to_string()
}

async fn get_trip_plan(
    State(app): State<App>,
    Query(params): Query<TripPlanParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if params.results != "fastest" && params.results != "options" {
        return Err((
            StatusCode::BAD_REQUEST,
            "results must be 'fastest' or 'options'".to_string(),
        ));
    }

    let waypoints: Vec<String> = params
        .waypoints
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
        .collect();

    let Some(connections) = trip_planning::build_connections_for_date(&app.database, params.date)
        .await
        .map_err(internal_error("build connections array"))?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "no CIF-derived schedule data has been published for {} yet",
                params.date
            ),
        ));
    };

    let interchange = trip_planning::fetch_interchange_data(&app.database)
        .await
        .map_err(internal_error("fetch interchange data"))?;

    let segments = trip_planning_itinerary::plan_via_waypoints(
        &connections,
        &interchange,
        params.date,
        &params.origin.trim().to_ascii_uppercase(),
        &waypoints,
        &params.destination.trim().to_ascii_uppercase(),
        params.depart_after.unwrap_or(NaiveTime::MIN),
        &params.results,
    )
    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    Ok(Json(serde_json::json!({
        "results": params.results,
        "segments": segments.iter().map(|segment| serde_json::json!({
            "originCrs": segment.origin_crs,
            "destinationCrs": segment.destination_crs,
            "itineraries": segment.itineraries,
            "cappedByMaxChanges": segment.capped_by_max_changes,
        })).collect::<Vec<_>>(),
    })))
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "trip plan request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, format!("failed to {operation}"))
    }
}
```

- [ ] **Step 2: Mount the route** in `crates/api/src/main.rs`, alongside
  `routes::journeys::router()`:

```rust
    let mut router = Router::new()
        .merge(routes::line_status::router())
        .merge(routes::train::router())
        .merge(routes::journeys::router())
        .merge(routes::journey_templates::router())
        .merge(routes::trips::router())
        .nest("/public", routes::public_router())
        .nest("/private", routes::private_router(app.clone()));
```

  and register the module in `crates/api/src/routes/mod.rs`
  (`pub mod trips;`).

- [ ] **Step 3: Write DB-gated HTTP tests**, matching `journeys.rs`'s own
  `db_tests` conventions (`connect`/`test_app`/`test_router`/`request`/
  `post_json` helpers — reuse them via `super::super::journeys::` test
  helpers if `pub(crate)`, or duplicate the small helpers locally if they
  are private to that file's own test module; check with `grep -n "pub(crate) async fn connect\|async fn connect" crates/api/src/routes/journeys.rs`
  before assuming visibility):

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    async fn connect() -> sqlx::PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        sqlx::PgPool::connect(&url).await.expect("connect")
    }

    async fn get(router: axum::Router, uri: String) -> (StatusCode, Value) {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8(bytes.to_vec()).unwrap()));
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_trip_plan -- --ignored --test-threads=1`"]
    async fn a_date_with_no_published_schedule_data_is_a_clear_404() {
        let pool = connect().await;
        let router = crate::app::test_router_for(crate::app::test_app_for(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.as_str().unwrap().contains("2099-01-01"));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_trip_plan -- --ignored --test-threads=1`"]
    async fn an_invalid_results_value_is_a_clear_400() {
        let pool = connect().await;
        let router = crate::app::test_router_for(crate::app::test_app_for(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2026-09-23&results=quickest".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.as_str().unwrap().contains("fastest"));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_trip_plan -- --ignored --test-threads=1`"]
    async fn a_real_seeded_connection_is_found_end_to_end() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLAN1', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLAN1', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLAN-EUS', 'EUS', 'EUSTON', 'LONDON EUSTON', 1), \
                    ('TESTPLAN-MKC', 'MKC', 'MILTNKC', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = crate::app::test_router_for(crate::app::test_app_for(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&destination=MKC&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        let itineraries = segments[0]["itineraries"].as_array().expect("itineraries array");
        assert_eq!(itineraries.len(), 1);
        assert_eq!(itineraries[0]["legs"][0]["trainUid"], "TESTPLAN1");

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLAN1'").execute(&pool).await.ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLAN-%'").execute(&pool).await.ok();
    }
}
```

  **Note on test scaffolding**: `crate::app::test_router_for`/
  `test_app_for` are illustrative names — before writing this step, check
  the ACTUAL shared test-app-construction helper this crate uses (`grep
  -n "fn test_app\b" crates/api/src/routes/*.rs | head -5`; `journeys.rs`'s
  own `test_app`/`test_router`, lines 1130/1202, are private to that file's
  test module today). If no crate-visible shared helper exists, either (a)
  duplicate the minimal `App`-construction fixture locally in `trips.rs`'s
  own test module, matching the `schedule_crs_line_index:
  std::collections::HashMap::new()` pattern every other route file's own
  fixture already repeats (confirmed present in over a dozen files via this
  plan's own research pass), or (b) make `journeys.rs`'s existing
  `test_app`/`test_router` `pub(crate)` and import them — prefer (a) unless
  a third consumer would make (b) clearly worth it, matching this
  codebase's low-shared-test-infrastructure convention observed throughout
  `crates/api/src/routes/*.rs`.

- [ ] **Step 4: Run the tests**

```bash
cargo build -p api
cargo test -p api -- --ignored --test-threads=1
```

  Expected: builds clean; all three new DB-gated tests pass.

- [ ] **Step 5: Run fmt/clippy**

```bash
cargo fmt --all
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/trips.rs crates/api/src/routes/mod.rs crates/api/src/main.rs
git commit -m "api: add GET /Trips/plan, the read-only journey-planning endpoint"
```

---

## Self-review notes

- **Spec coverage**: §5.2's proposed response shape (`legs`, per-leg
  origin/destination/train identity/times, `totalDuration`, `changeCount`)
  is implemented as `PlannedItinerary`/`PlannedLeg`; §7 Open Question 2 is
  resolved (Judgment Call 1); §4's ≤2-interchange cap is enforced
  presentation-side for both `results` modes, with honest disclosure when
  it binds (Judgment Calls 2/3); §4's ordered-waypoints scope is
  `plan_via_waypoints` (Judgment Call 4).
- **Placeholder scan**: none in shipped logic. Task 2 Step 3's own note
  about `test_app_for`/`test_router_for` being illustrative names is an
  explicit, flagged verification step for the implementer (check what
  actually exists before writing), not a silent assumption baked into
  committed code.
- **Type consistency**: `PlannedLeg`/`PlannedItinerary` from Task 1 are
  serialized directly (via `#[derive(Serialize)]`) into Task 2's JSON
  response with no intermediate re-shaping — Phase 6's frontend types
  should mirror these exact field names.
- **Review Focus**: all five items have a directly corresponding test
  (`an_unresolvable_origin_crs_is_a_clear_error`,
  `a_date_with_no_published_schedule_data_is_a_clear_404`,
  `options_mode_excludes_results_over_the_cap_but_flags_when_capped`,
  `plan_via_waypoints_names_the_failing_segment`,
  `an_invalid_results_value_is_a_clear_error`/`an_invalid_results_value_is_a_clear_400`).
