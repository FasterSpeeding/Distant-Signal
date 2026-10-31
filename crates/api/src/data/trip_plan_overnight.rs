//! `/Trips/plan?acrossMidnight=true` (2026-10-20): merging two service
//! dates' plans into one.
//!
//! Between midnight and the early morning, trains of two service dates run
//! at once: date D's late trains (their calls stored past 1440) and date
//! D+1's first ones. A request for "the morning after D" (`dayOffset=1`)
//! searches only D's trains; with `acrossMidnight` the route also plans
//! D+1 from the same clock time and this module merges the two.
//!
//! It works on finished [`SegmentResult`]s (searched, live-checked and with
//! leg details attached, per date), so the searches themselves are
//! untouched. Spec:
//! docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md §3.3.

use std::collections::HashSet;

use trip_planner::options::{Direction, MAX_ALTERNATIVES, within_bound};

use super::trip_planning_itinerary::{PlannedItinerary, PlannedLeg, SegmentResult, TimeBound};

const DAY_MIN: u32 = 1440;

/// One whole journey: part `s` is its itinerary for segment `s`.
type Journey = Vec<PlannedItinerary>;

/// `later`'s times moved onto `earlier`'s clock: a day on. Its legs keep
/// their own `serviceDate` and day offsets, which already describe them.
fn shift_a_day(mut segments: Vec<SegmentResult>) -> Vec<SegmentResult> {
    for segment in &mut segments {
        segment.depart_after_min = segment.depart_after_min.map(|m| m + DAY_MIN);
        segment.arrive_by_min = segment.arrive_by_min.map(|m| m + DAY_MIN);
        for itinerary in &mut segment.itineraries {
            itinerary.departure_min += DAY_MIN;
            itinerary.arrival_min += DAY_MIN;
            for leg in &mut itinerary.legs {
                if let PlannedLeg::Train {
                    departure_min,
                    arrival_min,
                    ..
                } = leg
                {
                    *departure_min += DAY_MIN;
                    *arrival_min += DAY_MIN;
                }
            }
        }
    }
    segments
}

/// The aligned journeys of a plan (`segments[s].itineraries[j]` is part `s`
/// of journey `j`); none when the segments disagree on the count.
fn journeys(segments: &[SegmentResult]) -> Vec<Journey> {
    let Some(count) = segments.first().map(|s| s.itineraries.len()) else {
        return Vec::new();
    };
    if segments.iter().any(|s| s.itineraries.len() != count) {
        return Vec::new();
    }
    (0..count)
        .map(|j| segments.iter().map(|s| s.itineraries[j].clone()).collect())
        .collect()
}

/// Trains ridden end to end, less one; a ride continuing through a
/// waypoint counts once (as `journey_summaries` counts).
fn changes(journey: &Journey) -> u32 {
    let rides: usize = journey
        .iter()
        .map(|part| {
            part.legs
                .iter()
                .filter(|leg| matches!(leg, PlannedLeg::Train { .. }))
                .count()
                - usize::from(part.continues_previous_train)
        })
        .sum();
    u32::try_from(rides.saturating_sub(1)).unwrap_or(u32::MAX)
}

fn departure(journey: &Journey) -> u32 {
    journey.first().map_or(0, |part| part.departure_min)
}

fn arrival(journey: &Journey) -> u32 {
    journey.last().map_or(0, |part| part.arrival_min)
}

/// What makes two journeys the same: every leg as served, at the same times.
fn identity(journey: &Journey) -> String {
    let parts: Vec<(String, u32, u32)> = journey
        .iter()
        .map(|part| {
            (
                serde_json::to_string(&part.legs).unwrap_or_default(),
                part.departure_min,
                part.arrival_min,
            )
        })
        .collect();
    serde_json::to_string(&parts).unwrap_or_default()
}

/// The ordering rule each search mode already uses, as a sort key (smaller
/// is better). Arrive-by: the latest departure, then the earliest arrival.
/// Depart-after: the earliest arrival, then the latest departure.
fn rank(journey: &Journey, arrive_by: bool) -> (u32, u32) {
    if arrive_by {
        (u32::MAX - departure(journey), arrival(journey))
    } else {
        (arrival(journey), u32::MAX - departure(journey))
    }
}

/// Merges `earlier`'s plan (service date D) with `later`'s (D+1, searched
/// from the same clock time a day on). Identical journeys are kept once,
/// and the result follows the ordering rules a single date's plan does:
///
/// - `fastest`: the one best journey;
/// - `options`: per number of changes, fewest first, the best journey,
///   kept only when it beats every one with fewer changes; then the
///   alternative routes (DS-196) of either date still within the detour
///   bound of the merged best journey, best first, at most
///   `trip_planner::options::MAX_ALTERNATIVES`.
///
/// Segment metadata comes from `earlier` (the request's own date), except
/// `cappedByMaxChanges` (either), the waypoint deadlines (the widest of
/// the two) and `noResultReason` (cleared when the merge has a journey).
pub fn merge_across_midnight(
    earlier: Vec<SegmentResult>,
    later: Vec<SegmentResult>,
    time: TimeBound,
    results: &str,
) -> Vec<SegmentResult> {
    let later = shift_a_day(later);
    if later.len() != earlier.len() {
        return earlier;
    }
    let arrive_by = matches!(time, TimeBound::ArriveBy(_));

    let mut seen = HashSet::new();
    let (mut alternatives, mut pool): (Vec<Journey>, Vec<Journey>) = journeys(&earlier)
        .into_iter()
        .chain(journeys(&later))
        .filter(|journey| seen.insert(identity(journey)))
        .partition(|journey| {
            journey
                .first()
                .is_some_and(|part| part.alternative.is_some())
        });
    pool.sort_by_key(|journey| (changes(journey), rank(journey, arrive_by)));

    let selected: Vec<Journey> = if results == "fastest" {
        pool.into_iter()
            .min_by_key(|journey| rank(journey, arrive_by))
            .into_iter()
            .collect()
    } else {
        let mut kept: Vec<Journey> = Vec::new();
        for journey in pool {
            let beats_kept = kept
                .iter()
                .all(|best| rank(&journey, arrive_by).0 < rank(best, arrive_by).0);
            // `pool` is sorted by changes then rank, so the first of each
            // change count is its best.
            let new_count = kept
                .last()
                .is_none_or(|last| changes(last) < changes(&journey));
            if beats_kept && new_count {
                kept.push(journey);
            }
        }
        let direction = if arrive_by {
            Direction::ArriveBy
        } else {
            Direction::DepartAfter
        };
        if let Some(best) = kept.iter().min_by_key(|journey| rank(journey, arrive_by)) {
            let best = (departure(best), arrival(best));
            alternatives.retain(|journey| {
                within_bound(direction, best, (departure(journey), arrival(journey)))
            });
            alternatives.sort_by_key(|journey| rank(journey, arrive_by));
            alternatives.truncate(MAX_ALTERNATIVES);
            kept.extend(alternatives);
        }
        kept
    };

    earlier
        .into_iter()
        .zip(later)
        .enumerate()
        .map(|(s, (first, second))| SegmentResult {
            itineraries: selected.iter().map(|journey| journey[s].clone()).collect(),
            capped_by_max_changes: first.capped_by_max_changes || second.capped_by_max_changes,
            depart_after_min: min_some(first.depart_after_min, second.depart_after_min),
            arrive_by_min: first.arrive_by_min.max(second.arrive_by_min),
            no_result_reason: if selected.is_empty() {
                first.no_result_reason.or(second.no_result_reason)
            } else {
                None
            },
            origin_crs: first.origin_crs,
            destination_crs: first.destination_crs,
        })
        .collect()
}

fn min_some(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    fn leg(uid: &str, date: NaiveDate, dep: u32, arr: u32) -> PlannedLeg {
        let clock = |m: u32| NaiveTime::from_hms_opt((m % 1440) / 60, m % 60, 0).unwrap();
        PlannedLeg::Train {
            train_uid: uid.to_string(),
            service_date: date,
            origin_crs: None,
            destination_crs: None,
            scheduled_departure: clock(dep),
            scheduled_arrival: clock(arr),
            departure_day_offset: u8::from(dep >= 1440),
            arrival_day_offset: u8::from(arr >= 1440),
            working_departure: clock(dep),
            working_arrival: clock(arr),
            working_departure_day_offset: u8::from(dep >= 1440),
            working_arrival_day_offset: u8::from(arr >= 1440),
            booked_departure_platform: None,
            booked_arrival_platform: None,
            public_departure: None,
            public_arrival: None,
            public_departure_day_offset: None,
            public_arrival_day_offset: None,
            operator: None,
            headcode: None,
            service: crate::data::schedule_services::ServiceModeFields::default(),
            from_tiploc: String::new(),
            to_tiploc: String::new(),
            departure_min: dep,
            arrival_min: arr,
            live: None,
            transit: None,
        }
    }

    /// One itinerary riding `trains` (`(uid, dep, arr)`, minutes on its
    /// own service date `date`).
    fn itinerary(date: NaiveDate, trains: &[(&str, u32, u32)]) -> PlannedItinerary {
        let legs: Vec<PlannedLeg> = trains
            .iter()
            .map(|&(uid, dep, arr)| leg(uid, date, dep, arr))
            .collect();
        let (departure_min, arrival_min) = (trains[0].1, trains[trains.len() - 1].2);
        PlannedItinerary {
            change_count: u32::try_from(legs.len()).unwrap() - 1,
            access_change_count: 0,
            legs,
            total_duration_minutes: arrival_min - departure_min,
            exceeds_recommended_changes: None,
            departure_min,
            arrival_min,
            arrival_tiploc: None,
            departure_tiploc: None,
            live_feasible: None,
            continues_previous_train: false,
            via_satisfied_by: Vec::new(),
            scheduled_only: None,
            alternative: None,
            labels: Vec::new(),
        }
    }

    fn plan(itineraries: Vec<PlannedItinerary>, arrive_by: Option<u32>) -> Vec<SegmentResult> {
        vec![SegmentResult {
            origin_crs: "ZOA".to_string(),
            destination_crs: "ZOB".to_string(),
            itineraries,
            capped_by_max_changes: false,
            depart_after_min: None,
            arrive_by_min: arrive_by,
            no_result_reason: None,
        }]
    }

    fn d() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap()
    }

    fn d1() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 24).unwrap()
    }

    fn shape(segments: &[SegmentResult]) -> Vec<(u32, u32, u32)> {
        segments[0]
            .itineraries
            .iter()
            .map(|it| (it.change_count, it.departure_min, it.arrival_min))
            .collect()
    }

    #[test]
    fn arrive_by_takes_the_next_dates_train_when_it_leaves_later() {
        // By 00:40 on the 24th (1480 on the 23rd's clock): the 23rd's 23:30
        // (arriving 00:20) or the 24th's own 00:05 (arriving 00:35). The
        // 00:05 leaves later.
        let earlier = plan(vec![itinerary(d(), &[("LATE1", 1410, 1460)])], Some(1480));
        let later = plan(vec![itinerary(d1(), &[("EARLY1", 5, 35)])], Some(40));
        let merged = merge_across_midnight(earlier, later, TimeBound::ArriveBy(1480), "fastest");
        assert_eq!(shape(&merged), [(0, 1445, 1475)]);
        let PlannedLeg::Train {
            train_uid,
            service_date,
            departure_min,
            ..
        } = &merged[0].itineraries[0].legs[0]
        else {
            panic!("a train leg")
        };
        assert_eq!((train_uid.as_str(), *service_date), ("EARLY1", d1()));
        assert_eq!(*departure_min, 1445, "shifted onto the 23rd's clock");
        assert_eq!(merged[0].arrive_by_min, Some(1480));
    }

    #[test]
    fn depart_after_takes_the_earliest_arrival_from_either_date() {
        // After 00:10 on the 24th: the 23rd's 00:20 (+1) arrives 01:00;
        // the 24th's 00:15 arrives 00:50.
        let earlier = plan(vec![itinerary(d(), &[("LATE1", 1460, 1500)])], None);
        let later = plan(vec![itinerary(d1(), &[("EARLY1", 15, 50)])], None);
        let merged = merge_across_midnight(earlier, later, TimeBound::DepartAfter(1450), "fastest");
        assert_eq!(shape(&merged), [(0, 1455, 1490)]);
    }

    #[test]
    fn one_side_empty_keeps_the_other_and_clears_the_reason() {
        let mut earlier = plan(Vec::new(), Some(1470));
        earlier[0].no_result_reason = Some(super::super::trip_planning_itinerary::NoResultReason {
            link_operators: Vec::new(),
            constraint: "arriveBy",
            values: vec!["00:30".to_string()],
            message: "none".to_string(),
        });
        let later = plan(vec![itinerary(d1(), &[("EARLY1", 5, 25)])], Some(30));
        let merged = merge_across_midnight(earlier, later, TimeBound::ArriveBy(1470), "fastest");
        assert_eq!(shape(&merged), [(0, 1445, 1465)]);
        assert!(merged[0].no_result_reason.is_none());
        // Both empty: the request date's own reason stays.
        let mut earlier = plan(Vec::new(), Some(1470));
        earlier[0].no_result_reason = Some(super::super::trip_planning_itinerary::NoResultReason {
            link_operators: Vec::new(),
            constraint: "arriveBy",
            values: Vec::new(),
            message: "none".to_string(),
        });
        let merged = merge_across_midnight(
            earlier,
            plan(Vec::new(), Some(30)),
            TimeBound::ArriveBy(1470),
            "fastest",
        );
        assert!(merged[0].itineraries.is_empty());
        assert_eq!(
            merged[0].no_result_reason.as_ref().map(|r| r.constraint),
            Some("arriveBy")
        );
    }

    #[test]
    fn options_keep_one_per_change_count_each_leaving_later() {
        // Arrive by 01:00 (+1): direct 23:00 (23rd); one change leaving
        // 23:40 (23rd); direct 00:10 (24th); one change 00:05 (24th) is
        // beaten by the direct 00:10, so it goes.
        let earlier = plan(
            vec![
                itinerary(d(), &[("D1", 1380, 1430)]),
                itinerary(d(), &[("C1", 1420, 1440), ("C2", 1445, 1490)]),
            ],
            Some(1500),
        );
        let later = plan(
            vec![
                itinerary(d1(), &[("D2", 10, 50)]),
                itinerary(d1(), &[("C3", 5, 20), ("C4", 25, 55)]),
            ],
            Some(60),
        );
        let merged = merge_across_midnight(earlier, later, TimeBound::ArriveBy(1500), "options");
        // The direct 00:10 (1450) is the latest with no change; nothing
        // with one change leaves later than it.
        assert_eq!(shape(&merged), [(0, 1450, 1490)]);
    }

    #[test]
    fn options_add_a_change_count_only_when_it_improves() {
        // Depart after 00:00 (+1): direct arrives 02:00 (24th); one change
        // arrives 01:00 (23rd) -- an improvement, so both.
        let earlier = plan(
            vec![itinerary(d(), &[("C1", 1445, 1460), ("C2", 1465, 1500)])],
            None,
        );
        let later = plan(vec![itinerary(d1(), &[("D1", 30, 120)])], None);
        let merged = merge_across_midnight(earlier, later, TimeBound::DepartAfter(1440), "options");
        assert_eq!(shape(&merged), [(0, 1470, 1560), (1, 1445, 1500)]);
    }

    #[test]
    fn identical_journeys_are_kept_once() {
        // A walk-free identical itinerary from both sides (same legs, same
        // times once shifted) appears once.
        let same = itinerary(d1(), &[("X1", 1450, 1470)]);
        let mut shifted_back = same.clone();
        shifted_back.departure_min -= DAY_MIN;
        shifted_back.arrival_min -= DAY_MIN;
        if let PlannedLeg::Train {
            departure_min,
            arrival_min,
            ..
        } = &mut shifted_back.legs[0]
        {
            *departure_min -= DAY_MIN;
            *arrival_min -= DAY_MIN;
        }
        let merged = merge_across_midnight(
            plan(vec![same], None),
            plan(vec![shifted_back], None),
            TimeBound::DepartAfter(1440),
            "options",
        );
        assert_eq!(merged[0].itineraries.len(), 1);
    }
}
