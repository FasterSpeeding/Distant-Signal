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

/// Normalizes `tiploc` before the lookup -- every other TIPLOC-keyed lookup
/// in this codebase does the same, at its own point of use, because a real
/// production incident (2026-09-16, "Unknown location" -- see
/// `data::trip_planning`'s own doc comments and its
/// `a_padded_tiploc_from_calling_points_full_still_matches_bare_stanox_crs_change_time`
/// test) was caused by exactly this bug: a padded/un-normalized TIPLOC
/// (as flows through this whole system, straight off
/// `schedule_calling_points_full.tiploc`) failing to match a bare-keyed
/// lookup table. `tiploc_to_crs` is built from the UNION of `tiploc_crs`
/// and `stanox_crs` (`trip_planning::fetch_interchange_data`'s two passes,
/// as of Task 3 of
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md), both of
/// which are bare-keyed just like `change_time_by_tiploc` -- see
/// `trip_planner::csa`'s `ready_source_at`/`relax`/`relax_fixed_links`,
/// which normalize for the same reason.
fn crs_for_tiploc(interchange: &InterchangeData, tiploc: &str) -> Option<String> {
    interchange
        .tiploc_to_crs
        .get(schedule_query::normalize_tiploc(tiploc))
        .cloned()
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
    legs.iter()
        .filter(|leg| matches!(leg, JourneyLeg::Train(_)))
        .count() as u32
}

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
        return Err(format!(
            "'{origin_crs}' is not a recognised station CRS code"
        ));
    }
    if to_tiplocs.is_empty() {
        return Err(format!(
            "'{destination_crs}' is not a recognised station CRS code"
        ));
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
        let change_count = train_leg_count(&journey.legs).saturating_sub(1);
        let itinerary = PlannedItinerary {
            legs: journey
                .legs
                .iter()
                .map(|leg| planned_leg(leg, date, interchange))
                .collect(),
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
        let within_cap: Vec<&RaptorJourney> =
            all.iter().filter(|j| j.changes <= MAX_CHANGES).collect();
        // Judgment Call 2: did the headroom round (MAX_ROUNDS, one past
        // what MAX_CHANGES alone needs) find something strictly better
        // than every within-cap entry? If so, the cap genuinely bound the
        // answer -- flagged honestly, not silently swallowed.
        let best_within_cap = within_cap.iter().map(|j| j.arrival_min).min();
        let capped = all.iter().any(|j| {
            j.changes > MAX_CHANGES && best_within_cap.is_none_or(|best| j.arrival_min < best)
        });

        let itineraries = within_cap
            .into_iter()
            .map(|journey| PlannedItinerary {
                legs: journey
                    .legs
                    .iter()
                    .map(|leg| planned_leg(leg, date, interchange))
                    .collect(),
                change_count: journey.changes,
                total_duration_minutes: journey.arrival_min - journey.departure_min,
                exceeds_recommended_changes: None,
            })
            .collect();
        return Ok((itineraries, capped));
    }

    Err(format!(
        "results must be 'fastest' or 'options', not '{results}'"
    ))
}

/// One resolved leg of a multi-waypoint plan -- `origin`/`destination` name
/// which CRS pair this segment was for, so a caller can report exactly
/// which segment failed (this plan's own Review Focus).
#[derive(Debug)]
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
#[allow(clippy::too_many_arguments)]
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
        let segment_departure = if index == 0 {
            departure_after
        } else {
            NaiveTime::MIN
        };
        let (itineraries, capped) = plan_segment(
            connections,
            interchange,
            date,
            from,
            to,
            segment_departure,
            results,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn interchange_with(crs_to_tiplocs: &[(&str, &str)]) -> InterchangeData {
        interchange_with_change_times(crs_to_tiplocs, &[])
    }

    fn interchange_with_change_times(
        crs_to_tiplocs: &[(&str, &str)],
        change_times: &[(&str, i32)],
    ) -> InterchangeData {
        let mut data = InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (crs, tiploc) in crs_to_tiplocs {
            data.tiploc_to_crs
                .insert(tiploc.to_string(), crs.to_string());
            data.crs_to_tiplocs
                .entry(crs.to_string())
                .or_default()
                .push(tiploc.to_string());
        }
        for (tiploc, change_time) in change_times {
            data.change_time_by_tiploc
                .insert(tiploc.to_string(), *change_time);
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
        let result = plan_segment(
            &[],
            &interchange,
            date(),
            "ZZZ",
            "MKC",
            NaiveTime::MIN,
            "fastest",
        );
        assert!(result.unwrap_err().contains("ZZZ"));
    }

    #[test]
    fn fastest_mode_flags_a_result_needing_more_than_the_cap() {
        // Three changes: EUS -> A -> B -> C -> MKC, all changes instant
        // (0-minute change times), so CSA finds this as the only, fastest
        // route -- and it must still be returned, flagged.
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 490),
            conn("U2", "A", "B", 490, 500),
            conn("U3", "B", "C", 500, 510),
            conn("U4", "C", "MKC", 510, 520),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MKC")],
            &[("EUSTON", 0), ("A", 0), ("B", 0), ("C", 0), ("MKC", 0)],
        );
        let (itineraries, _) = plan_segment(
            &connections,
            &interchange,
            date(),
            "EUS",
            "MKC",
            NaiveTime::MIN,
            "fastest",
        )
        .unwrap();
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
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MKC")],
            &[
                ("EUSTON", 0),
                ("A", 0),
                ("B", 0),
                ("P", 0),
                ("Q", 0),
                ("R", 0),
                ("MKC", 0),
            ],
        );
        let (itineraries, capped) = plan_segment(
            &connections,
            &interchange,
            date(),
            "EUS",
            "MKC",
            NaiveTime::MIN,
            "options",
        )
        .unwrap();
        assert!(itineraries.iter().all(|i| i.change_count <= MAX_CHANGES));
        assert!(
            capped,
            "a strictly faster, over-cap itinerary exists and must be flagged"
        );
    }

    #[test]
    fn planned_leg_normalizes_a_padded_tiploc_before_resolving_its_crs() {
        // Regression test for a final-whole-branch-review finding:
        // `TrainLeg::from_tiploc`/`to_tiploc` come straight off
        // `Connection` (`reconstruct_legs`'s own `boarded.from_tiploc.clone()`
        // in `trip_planner::csa`), which itself comes straight off
        // `schedule_calling_points_full.tiploc` -- still padded, per
        // `data::trip_planning`'s own 2026-09-16 "Unknown location"
        // incident doc comments. `tiploc_to_crs` is bare-keyed (built from
        // `stanox_crs`), so `crs_for_tiploc` must normalize the TIPLOC
        // before the lookup or a padded TIPLOC silently resolves to `None`
        // -- exactly the class of bug that incident was.
        let connections = vec![conn("U1", "EUSTON ", "MKC", 480, 530)];
        let interchange = interchange_with_change_times(
            // Bare-keyed, matching how `tiploc_to_crs`/`crs_to_tiplocs` are
            // really built from `stanox_crs`.
            &[("EUS", "EUSTON"), ("MKC", "MKC")],
            &[("EUSTON", 0), ("MKC", 0)],
        );
        let (itineraries, _) = plan_segment(
            &connections,
            &interchange,
            date(),
            "EUS",
            "MKC",
            NaiveTime::MIN,
            "fastest",
        )
        .unwrap();
        assert_eq!(itineraries.len(), 1);
        let PlannedLeg::Train {
            origin_crs,
            destination_crs,
            ..
        } = &itineraries[0].legs[0]
        else {
            panic!("expected a train leg, got {:?}", itineraries[0].legs[0]);
        };
        assert_eq!(
            origin_crs.as_deref(),
            Some("EUS"),
            "a padded TIPLOC ('EUSTON ') must still resolve to its CRS against a \
             bare-keyed tiploc_to_crs map"
        );
        assert_eq!(destination_crs.as_deref(), Some("MKC"));
    }

    #[test]
    fn plan_via_waypoints_returns_one_segment_result_per_hop_in_order() {
        // Every existing waypoint test only exercises the FAILURE path.
        // This proves the happy path: a two-segment (one intermediate
        // waypoint) request returns one `SegmentResult` per hop, in order,
        // each with the right origin/destination CRS pair and at least one
        // real itinerary.
        let connections = vec![
            // EUS -> MKC (first hop).
            conn("U1", "EUSTON", "MILTNKC", 480, 530),
            // MKC -> MAN (second hop) -- departs after the first hop
            // arrives, though `plan_via_waypoints` solves each hop
            // independently and doesn't require this ordering.
            conn("U2", "MILTNKC", "MANCPIC", 600, 660),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MILTNKC"), ("MAN", "MANCPIC")],
            &[("EUSTON", 0), ("MILTNKC", 0), ("MANCPIC", 0)],
        );

        let segments = plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "EUS",
            &["MKC".to_string()],
            "MAN",
            NaiveTime::MIN,
            "fastest",
        )
        .unwrap();

        assert_eq!(segments.len(), 2, "one SegmentResult per hop: {segments:?}");

        assert_eq!(segments[0].origin_crs, "EUS");
        assert_eq!(segments[0].destination_crs, "MKC");
        assert!(
            !segments[0].itineraries.is_empty(),
            "the first hop has a real, findable route and must return at least one itinerary"
        );

        assert_eq!(segments[1].origin_crs, "MKC");
        assert_eq!(segments[1].destination_crs, "MAN");
        assert!(
            !segments[1].itineraries.is_empty(),
            "the second hop has a real, findable route and must return at least one itinerary"
        );
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
        assert!(
            err.contains("EUS -> ZZZ"),
            "error must name the failing segment: {err}"
        );
    }

    #[test]
    fn an_invalid_results_value_is_a_clear_error() {
        let interchange = interchange_with(&[("EUS", "EUSTON"), ("MKC", "MKC")]);
        let err = plan_segment(
            &[],
            &interchange,
            date(),
            "EUS",
            "MKC",
            NaiveTime::MIN,
            "quickest",
        )
        .unwrap_err();
        assert!(err.contains("fastest"));
        assert!(err.contains("options"));
    }
}
