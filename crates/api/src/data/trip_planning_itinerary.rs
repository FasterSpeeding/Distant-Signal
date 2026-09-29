//! Turns a raw `trip_planner::Journey`/`RaptorJourney` (TIPLOC-keyed,
//! minutes-from-midnight) into the CRS/human-time-keyed shape
//! `routes::trips` serializes, applies this feature's interchange cap
//! (design spec §4: ≤2 by default, caller-raisable to ≤4 via
//! `?maxChanges=`; a hard filter in `options` mode only, a flag in
//! `fastest` mode -- see [`plan_segment`]), and chains ordered waypoints
//! into independently-solved sub-journeys (design spec §4: "Not a
//! traveling-salesman-style... problem"). See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Calls for the reasoning behind the cap-enforcement and
//! waypoint-chaining choices below.

use chrono::{NaiveDate, NaiveTime};
use schedule_query::InterchangeData;
use trip_planner::{JourneyLeg, RaptorJourney};

/// Design spec §4's default cap, at most 2 interchanges (3 legs) per
/// computed itinerary -- applied here, in the presentation layer, never
/// inside `scan_connections`/`raptor_search` themselves (Phase 3/4's own
/// Judgment Calls: neither algorithm has an interchange-count concept built
/// in). This is what `GET /Trips/plan` uses when the caller omits
/// `?maxChanges=`, so existing callers (this app's own frontend included)
/// see exactly the pre-parameter behaviour.
pub const DEFAULT_MAX_CHANGES: u32 = 2;

/// Upper bound on a caller-requested `?maxChanges=` -- matches the sibling
/// `Distant-Signal-MCP` project's own `PLAN_MAX_CHANGES` default of 4 (the
/// precedent design spec §4 already cites), so that project can route every
/// `plan_journey` query through `/Trips/plan`. Bounded rather than open
/// because every extra change is one more full RAPTOR sweep over the day's
/// connections graph per segment in `options` mode -- see
/// `routes::trips::get_trip_plan`'s own DoS notes for the worst-case
/// arithmetic.
pub const MAX_CHANGES_LIMIT: u32 = 4;

/// Phase 4's own Judgment Call 2: `max_rounds` for RAPTOR is NEVER the
/// library's own default -- `max_changes + 1` trips needed for
/// `max_changes` changes, plus one further round of headroom to detect
/// whether the cap actually bound the answer. Derived from the cap actually
/// in effect for THIS request, never from a fixed constant, so a raised cap
/// can't silently lose its headroom round (or its within-cap answers).
pub const fn max_rounds(max_changes: u32) -> u32 {
    max_changes + 2
}

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
        /// How many calendar days past `service_date` `scheduled_departure`
        /// actually falls on (additive, 2026-09-28): a later waypoint
        /// segment now searches from the previous segment's arrival, so an
        /// onward train can board after midnight on a service that started
        /// the evening before -- this says so rather than leaving a bare
        /// `00:30` that reads as the morning of `service_date`.
        departure_day_offset: u8,
        /// How many calendar days past `service_date` `scheduled_arrival`
        /// actually falls on -- same field, same meaning, as
        /// `schedule_query::records::CallingPoint::day_offset`.
        arrival_day_offset: u8,
        /// CIF booked (timetabled) platform at the boarding calling point,
        /// and at the alighting one -- `schedule_calling_points_full.platform`.
        /// Never live/Darwin. `None` until `trip_leg_details::attach_leg_details`
        /// fills them in; stays `None` when the CIF field is blank.
        booked_departure_platform: Option<String>,
        booked_arrival_platform: Option<String>,
        /// The schedule's `BX` ATOC operator code (e.g. `"SW"`), via
        /// `schedule_destination_departures.operator_atoc`. Filled in by
        /// `trip_leg_details::attach_leg_details`; `None` when unknown.
        operator: Option<String>,
        /// The schedule's CIF `BS` Train Identity -- the 4-character
        /// signalling headcode (e.g. `"1S00"`), via
        /// `schedule_destination_departures.headcode`. Filled in by
        /// `trip_leg_details::attach_leg_details`; `None` when unknown or
        /// when the stored rows disagree. NOT the TRUST 10-char train id.
        headcode: Option<String>,
        /// Internal (never serialized): the boarding/alighting TIPLOCs and
        /// raw minutes-from-service-day-midnight the planner produced, so
        /// waypoint chaining and the live overlay can work on exact values
        /// rather than re-deriving them from the rendered clock times.
        #[serde(skip)]
        from_tiploc: String,
        #[serde(skip)]
        to_tiploc: String,
        #[serde(skip)]
        departure_min: u32,
        #[serde(skip)]
        arrival_min: u32,
        /// The live overlay's view of this leg (`data::trip_plan_live`):
        /// absent unless the overlay was applied (so `live=false` is the
        /// pre-overlay shape exactly), then `null` (nothing known, or
        /// outside the live window) or a [`crate::data::trip_plan_live::LegLive`].
        #[serde(skip_serializing_if = "Option::is_none")]
        live: Option<Option<Box<crate::data::trip_plan_live::LegLive>>>,
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
    /// Internal (never serialized): minutes from service-day midnight of the
    /// itinerary's first departure and final arrival, and the TIPLOC it
    /// finally arrives at -- what [`plan_via_waypoints`] chains the next
    /// segment from.
    #[serde(skip)]
    pub departure_min: u32,
    #[serde(skip)]
    pub arrival_min: u32,
    #[serde(skip)]
    pub arrival_tiploc: Option<String>,
    /// Live overlay only (absent otherwise): no cancelled leg and no change
    /// that live times make impossible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_feasible: Option<bool>,
}

impl PlannedItinerary {
    fn from_legs(
        legs: &[JourneyLeg],
        departure_min: u32,
        arrival_min: u32,
        change_count: u32,
        exceeds_recommended_changes: Option<bool>,
        date: NaiveDate,
        interchange: &InterchangeData,
    ) -> Self {
        PlannedItinerary {
            legs: legs
                .iter()
                .map(|leg| planned_leg(leg, date, interchange))
                .collect(),
            change_count,
            total_duration_minutes: arrival_min - departure_min,
            exceeds_recommended_changes,
            departure_min,
            arrival_min,
            live_feasible: None,
            arrival_tiploc: legs.last().map(|leg| match leg {
                JourneyLeg::Train(train) => train.to_tiploc.clone(),
                JourneyLeg::Transfer(transfer) => transfer.to_tiploc.clone(),
            }),
        }
    }
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
            let (scheduled_departure, departure_day_offset) = minutes_to_clock(train.departure_min);
            let (scheduled_arrival, arrival_day_offset) = minutes_to_clock(train.arrival_min);
            PlannedLeg::Train {
                train_uid: train.uid.clone(),
                service_date: date,
                origin_crs: crs_for_tiploc(interchange, &train.from_tiploc),
                destination_crs: crs_for_tiploc(interchange, &train.to_tiploc),
                scheduled_departure,
                scheduled_arrival,
                departure_day_offset,
                arrival_day_offset,
                // Filled in after planning -- see `trip_leg_details`.
                booked_departure_platform: None,
                booked_arrival_platform: None,
                operator: None,
                headcode: None,
                from_tiploc: train.from_tiploc.clone(),
                to_tiploc: train.to_tiploc.clone(),
                departure_min: train.departure_min,
                arrival_min: train.arrival_min,
                live: None,
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
/// `max_changes` is the interchange cap in effect for this request
/// ([`DEFAULT_MAX_CHANGES`] unless the caller asked otherwise; the route
/// bounds it to at most [`MAX_CHANGES_LIMIT`]). Only `options` enforces it
/// as a hard limit: it drops every itinerary over the cap (and sizes
/// RAPTOR's rounds from it, see [`max_rounds`]), reporting via the returned
/// `bool` (`cappedByMaxChanges`) when a strictly faster over-cap itinerary
/// was dropped. `fastest` does NOT enforce it: CSA always returns the
/// earliest-arrival itinerary, even one needing more changes than the cap,
/// and only flags that via `exceeds_recommended_changes`
/// (`exceedsRecommendedChanges`); its returned `bool` is always `false`.
///
/// Returns `Ok(itineraries)` -- possibly empty, meaning "no itinerary was
/// found" (in `options` mode, "none within the cap"; a real, distinct
/// outcome from an error, see this plan's Review Focus) -- or
/// `Err(message)` for a caller-facing validation problem (an unresolvable
/// CRS).
#[allow(clippy::too_many_arguments)]
pub fn plan_segment(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    date: NaiveDate,
    origin_crs: &str,
    destination_crs: &str,
    departure_after: NaiveTime,
    results: &str,
    max_changes: u32,
) -> Result<(Vec<PlannedItinerary>, bool), String> {
    plan_segment_from_min(
        connections,
        interchange,
        date,
        origin_crs,
        destination_crs,
        clock_minutes(departure_after),
        results,
        max_changes,
        None,
    )
}

fn clock_minutes(time: NaiveTime) -> u32 {
    use chrono::Timelike;
    time.num_seconds_from_midnight() / 60
}

/// The CRS -> TIPLOC resolution [`plan_segment`] starts with, factored out
/// so [`plan_via_waypoints`] can still validate a segment it has no reason
/// to search (see there).
fn resolve_segment_tiplocs(
    interchange: &InterchangeData,
    origin_crs: &str,
    destination_crs: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
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
    Ok((from_tiplocs, to_tiplocs))
}

/// [`plan_segment`] with the departure bound as raw minutes from
/// service-day midnight, which may exceed 1440: a chained waypoint segment
/// can become ready after midnight, and the day's connections graph already
/// carries a late service's post-midnight calls at `1440 + ...` (see
/// `schedule_query::Connection::departure_min`).
#[allow(clippy::too_many_arguments)]
pub fn plan_segment_from_min(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    date: NaiveDate,
    origin_crs: &str,
    destination_crs: &str,
    departure_min: u32,
    results: &str,
    max_changes: u32,
    overlay: Option<&trip_planner::ConnectionOverlay>,
) -> Result<(Vec<PlannedItinerary>, bool), String> {
    let (from_tiplocs, to_tiplocs) =
        resolve_segment_tiplocs(interchange, origin_crs, destination_crs)?;

    if results == "fastest" {
        let Some(journey) = trip_planner::scan_connections_with_overlay(
            trip_planner::ScanOptions {
                connections,
                interchange,
                from_tiplocs: &from_tiplocs,
                to_tiplocs: &to_tiplocs,
                departure_min,
                date,
            },
            overlay,
        ) else {
            return Ok((Vec::new(), false));
        };
        let change_count = train_leg_count(&journey.legs).saturating_sub(1);
        let itinerary = PlannedItinerary::from_legs(
            &journey.legs,
            journey.departure_min,
            journey.arrival_min,
            change_count,
            // See this plan's Judgment Call 3: CSA has no cap of its own,
            // so a genuinely-fastest answer that needs more than the
            // requested cap is still returned, honestly flagged, not hidden.
            Some(change_count > max_changes),
            date,
            interchange,
        );
        return Ok((vec![itinerary], false));
    }

    if results == "options" {
        let all: Vec<RaptorJourney> = trip_planner::raptor_search_with_overlay(
            trip_planner::RaptorOptions {
                connections,
                interchange,
                from_tiplocs: &from_tiplocs,
                to_tiplocs: &to_tiplocs,
                departure_min,
                date,
                max_rounds: max_rounds(max_changes),
            },
            overlay,
        );
        let within_cap: Vec<&RaptorJourney> =
            all.iter().filter(|j| j.changes <= max_changes).collect();
        // Judgment Call 2: did the headroom round (`max_rounds`, one past
        // what `max_changes` alone needs) find something strictly better
        // than every within-cap entry? If so, the cap genuinely bound the
        // answer -- flagged honestly, not silently swallowed.
        let best_within_cap = within_cap.iter().map(|j| j.arrival_min).min();
        let capped = all.iter().any(|j| {
            j.changes > max_changes && best_within_cap.is_none_or(|best| j.arrival_min < best)
        });

        let itineraries = within_cap
            .into_iter()
            .map(|journey| {
                PlannedItinerary::from_legs(
                    &journey.legs,
                    journey.departure_min,
                    journey.arrival_min,
                    journey.changes,
                    None,
                    date,
                    interchange,
                )
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
#[derive(Debug, Clone)]
pub struct SegmentResult {
    pub origin_crs: String,
    pub destination_crs: String,
    pub itineraries: Vec<PlannedItinerary>,
    pub capped_by_max_changes: bool,
    /// Minutes from service-day midnight this segment was searched from:
    /// the caller's `departAfter` for the first segment, the previous
    /// segment's earliest arrival plus the waypoint's minimum change time
    /// for every later one, `None` when the previous segment found nothing
    /// to chain from (so this one was not searched). May exceed 1440.
    pub depart_after_min: Option<u32>,
}

/// Change time used when chaining a waypoint whose own minimum change time
/// is the `NoInterchange` sentinel: the traveller explicitly asked to stop
/// there, so the station's "never change here" marker (the coach-stand
/// sentinels) says nothing about how long they need -- use the same 5-minute
/// default `schedule_query::minimum_change_time` gives a TIPLOC with no MSN
/// record at all.
const WAYPOINT_FALLBACK_CHANGE_MINUTES: u32 = 5;

/// When the traveller can leave the waypoint `itinerary` arrives at: its
/// arrival plus the minimum change time at the TIPLOC it arrives at -- the
/// same figure the CSA/RAPTOR searches charge for an ordinary change there.
fn ready_after(itinerary: &PlannedItinerary, interchange: &InterchangeData) -> u32 {
    let change = itinerary
        .arrival_tiploc
        .as_deref()
        .map(
            |tiploc| match schedule_query::minimum_change_time(interchange, tiploc) {
                schedule_query::ChangeTime::Finite(minutes) => minutes,
                schedule_query::ChangeTime::NoInterchange => WAYPOINT_FALLBACK_CHANGE_MINUTES,
            },
        )
        .unwrap_or(WAYPOINT_FALLBACK_CHANGE_MINUTES);
    itinerary.arrival_min + change
}

/// The earliest a segment following `itineraries` can start: the earliest
/// ready time over every itinerary offered for the previous segment (in
/// `options` mode several are; any of them is a valid choice, and searching
/// from the earliest keeps every onward option reachable from at least one
/// of them). `None` when the previous segment found nothing.
pub fn chain_ready_min(
    itineraries: &[PlannedItinerary],
    interchange: &InterchangeData,
) -> Option<u32> {
    itineraries
        .iter()
        .map(|itinerary| ready_after(itinerary, interchange))
        .min()
}

/// Solves `origin -> waypoints[0] -> waypoints[1] -> ... -> destination` as
/// independent segments (design spec §4: ordered, not a traveling-salesman
/// problem -- see this plan's Judgment Call 4), concatenating each
/// segment's own result rather than re-optimising across the whole trip.
/// `departure_after` applies to the FIRST segment; each later segment
/// searches from the previous segment's earliest arrival plus the minimum
/// change time at the TIPLOC it arrives at ([`chain_ready_min`]). Until
/// 2026-09-28 later segments searched from `00:00`, so an onward train that
/// left before the traveller even reached the waypoint could be offered.
///
/// Day rollover: the ready time is carried as raw minutes and may pass
/// 1440, and the search then only sees this service date's own
/// post-midnight calls (the only ones in the date's graph). A chain that
/// runs past the last of those finds nothing for the later segment -- an
/// honest empty result, not a silent jump to the next service date.
///
/// When a segment finds nothing, every later segment is still CRS-validated
/// (so a bad code is still a 400 naming its segment) but not searched: there
/// is no arrival to chain from, and searching from `00:00` would reintroduce
/// the bug above. Those segments come back with no itineraries and
/// `depart_after_min: None`.
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
    max_changes: u32,
) -> Result<Vec<SegmentResult>, String> {
    plan_via_waypoints_with_overlay(
        connections,
        interchange,
        date,
        origin_crs,
        waypoints,
        destination_crs,
        departure_after,
        results,
        max_changes,
        None,
    )
}

/// [`plan_via_waypoints`] over the day graph with `overlay`'s trains
/// replaced (the `/Trips/plan` live overlay, `data::trip_plan_live`).
#[allow(clippy::too_many_arguments)]
pub fn plan_via_waypoints_with_overlay(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    date: NaiveDate,
    origin_crs: &str,
    waypoints: &[String],
    destination_crs: &str,
    departure_after: NaiveTime,
    results: &str,
    max_changes: u32,
    overlay: Option<&trip_planner::ConnectionOverlay>,
) -> Result<Vec<SegmentResult>, String> {
    let mut stops: Vec<&str> = vec![origin_crs];
    stops.extend(waypoints.iter().map(String::as_str));
    stops.push(destination_crs);

    let mut segments: Vec<SegmentResult> = Vec::new();
    for pair in stops.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let depart_after_min = match segments.last() {
            None => Some(clock_minutes(departure_after)),
            Some(previous) => chain_ready_min(&previous.itineraries, interchange),
        };
        let Some(depart_after_min) = depart_after_min else {
            resolve_segment_tiplocs(interchange, from, to)
                .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
            segments.push(SegmentResult {
                origin_crs: from.to_string(),
                destination_crs: to.to_string(),
                itineraries: Vec::new(),
                capped_by_max_changes: false,
                depart_after_min: None,
            });
            continue;
        };
        let (itineraries, capped) = plan_segment_from_min(
            connections,
            interchange,
            date,
            from,
            to,
            depart_after_min,
            results,
            max_changes,
            overlay,
        )
        .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
        segments.push(SegmentResult {
            origin_crs: from.to_string(),
            destination_crs: to.to_string(),
            itineraries,
            capped_by_max_changes: capped,
            depart_after_min: Some(depart_after_min),
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
            DEFAULT_MAX_CHANGES,
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
            DEFAULT_MAX_CHANGES,
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
            DEFAULT_MAX_CHANGES,
        )
        .unwrap();
        assert!(
            itineraries
                .iter()
                .all(|i| i.change_count <= DEFAULT_MAX_CHANGES)
        );
        assert!(
            capped,
            "a strictly faster, over-cap itinerary exists and must be flagged"
        );
    }

    /// Four instant-change hops `EUS -> A -> B -> C -> MKC`: the ONLY route,
    /// needing exactly 3 changes. Shared by the `max_changes` tests below.
    fn three_change_only_network() -> (Vec<schedule_query::Connection>, InterchangeData) {
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
        (connections, interchange)
    }

    #[test]
    fn options_mode_finds_a_three_change_route_only_when_the_cap_allows_it() {
        let (connections, interchange) = three_change_only_network();
        let plan = |max_changes| {
            plan_segment(
                &connections,
                &interchange,
                date(),
                "EUS",
                "MKC",
                NaiveTime::MIN,
                "options",
                max_changes,
            )
            .unwrap()
        };

        let (at_default, capped_at_default) = plan(DEFAULT_MAX_CHANGES);
        assert!(at_default.is_empty(), "{at_default:?}");
        assert!(
            capped_at_default,
            "the only route needs 3 changes, so the default cap of 2 genuinely bound it"
        );

        let (at_three, capped_at_three) = plan(3);
        assert_eq!(at_three.len(), 1, "{at_three:?}");
        assert_eq!(at_three[0].change_count, 3);
        assert!(!capped_at_three);
    }

    /// Proves the headroom round scales with the requested cap rather than
    /// staying pinned to the default: a 4-change-only route must be reported
    /// as `capped` at `max_changes = 3` (which needs RAPTOR round 5, i.e.
    /// `max_rounds(3)`), and found at `max_changes = 4` (the upper limit).
    #[test]
    fn the_headroom_round_scales_with_the_requested_cap() {
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 490),
            conn("U2", "A", "B", 490, 500),
            conn("U3", "B", "C", 500, 510),
            conn("U4", "C", "D", 510, 520),
            conn("U5", "D", "MKC", 520, 530),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MKC")],
            &[
                ("EUSTON", 0),
                ("A", 0),
                ("B", 0),
                ("C", 0),
                ("D", 0),
                ("MKC", 0),
            ],
        );
        let plan = |max_changes| {
            plan_segment(
                &connections,
                &interchange,
                date(),
                "EUS",
                "MKC",
                NaiveTime::MIN,
                "options",
                max_changes,
            )
            .unwrap()
        };

        let (at_three, capped_at_three) = plan(3);
        assert!(at_three.is_empty(), "{at_three:?}");
        assert!(
            capped_at_three,
            "max_rounds(3) must include a headroom round deep enough to see the 4-change route"
        );

        let (at_limit, capped_at_limit) = plan(MAX_CHANGES_LIMIT);
        assert_eq!(at_limit.len(), 1, "{at_limit:?}");
        assert_eq!(at_limit[0].change_count, 4);
        assert!(!capped_at_limit);
    }

    #[test]
    fn fastest_mode_flags_against_the_requested_cap_not_the_default() {
        let (connections, interchange) = three_change_only_network();
        let flag = |max_changes| {
            let (itineraries, capped) = plan_segment(
                &connections,
                &interchange,
                date(),
                "EUS",
                "MKC",
                NaiveTime::MIN,
                "fastest",
                max_changes,
            )
            .unwrap();
            assert!(!capped, "fastest mode never reports cappedByMaxChanges");
            assert_eq!(itineraries.len(), 1);
            itineraries[0].exceeds_recommended_changes
        };
        assert_eq!(flag(DEFAULT_MAX_CHANGES), Some(true));
        assert_eq!(flag(3), Some(false));
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
            DEFAULT_MAX_CHANGES,
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
            DEFAULT_MAX_CHANGES,
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

    fn train_uids(itinerary: &PlannedItinerary) -> Vec<&str> {
        itinerary
            .legs
            .iter()
            .filter_map(|leg| match leg {
                PlannedLeg::Train { train_uid, .. } => Some(train_uid.as_str()),
                PlannedLeg::Transfer { .. } => None,
            })
            .collect()
    }

    /// Regression test for the waypoint-chaining bug: every segment after
    /// the first used to search from 00:00, so a `MKC -> MAN` train that
    /// leaves hours BEFORE the `EUS -> MKC` leg even arrives was offered as
    /// the onward connection. Each later segment must search from the
    /// previous segment's arrival plus the waypoint's minimum change time
    /// (10 minutes at MILTNKC here), so both `EARLY` (before the arrival)
    /// and `TIGHT` (5 minutes after it, inside the change time) are out.
    #[test]
    fn a_later_waypoint_segment_departs_after_the_previous_arrival_plus_change_time() {
        let connections = vec![
            conn("EARLY", "MILTNKC", "MANCPIC", 300, 360),
            conn("U1", "EUSTON", "MILTNKC", 480, 530),
            conn("TIGHT", "MILTNKC", "MANCPIC", 535, 595),
            conn("OK", "MILTNKC", "MANCPIC", 545, 605),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MILTNKC"), ("MAN", "MANCPIC")],
            &[("EUSTON", 0), ("MILTNKC", 10), ("MANCPIC", 0)],
        );
        for results in ["fastest", "options"] {
            let segments = plan_via_waypoints(
                &connections,
                &interchange,
                date(),
                "EUS",
                &["MKC".to_string()],
                "MAN",
                NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
                results,
                DEFAULT_MAX_CHANGES,
            )
            .unwrap();
            assert_eq!(train_uids(&segments[0].itineraries[0]), ["U1"], "{results}");
            assert!(
                !segments[1].itineraries.is_empty(),
                "{results}: {segments:?}"
            );
            for itinerary in &segments[1].itineraries {
                assert_eq!(
                    train_uids(itinerary),
                    ["OK"],
                    "{results}: the onward segment must depart after the first \
                     segment's 08:50 arrival plus MILTNKC's 10-minute change time"
                );
            }
        }
    }

    /// Day rollover within the service date's window: the first segment
    /// arrives at 23:50, so the onward search starts at 23:55 and must be
    /// able to board a same-service-date train whose call is at 00:15 the
    /// next calendar day (minute 1455), reported with `departureDayOffset 1`.
    #[test]
    fn a_chained_segment_can_board_after_midnight_within_the_service_date() {
        let connections = vec![
            conn("MORNING", "MILTNKC", "MANCPIC", 360, 420),
            conn("LATE", "EUSTON", "MILTNKC", 1380, 1430),
            conn("NIGHT", "MILTNKC", "MANCPIC", 1455, 1520),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MILTNKC"), ("MAN", "MANCPIC")],
            &[("EUSTON", 0), ("MILTNKC", 5), ("MANCPIC", 0)],
        );
        let segments = plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "EUS",
            &["MKC".to_string()],
            "MAN",
            NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            "fastest",
            DEFAULT_MAX_CHANGES,
        )
        .unwrap();
        assert_eq!(segments[1].depart_after_min, Some(1435));
        let itinerary = &segments[1].itineraries[0];
        assert_eq!(train_uids(itinerary), ["NIGHT"]);
        let PlannedLeg::Train {
            scheduled_departure,
            departure_day_offset,
            service_date,
            ..
        } = &itinerary.legs[0]
        else {
            panic!("expected a train leg");
        };
        assert_eq!(
            *scheduled_departure,
            NaiveTime::from_hms_opt(0, 15, 0).unwrap()
        );
        assert_eq!(*departure_day_offset, 1);
        assert_eq!(
            *service_date,
            date(),
            "still the schedule's own service date"
        );
    }

    /// A segment with nothing to chain from is not searched from 00:00 (which
    /// would reintroduce the bug): it comes back empty with no search time,
    /// while a later bad CRS is still reported against its own segment.
    #[test]
    fn a_segment_after_an_unreachable_one_is_empty_but_still_validated() {
        let connections = vec![
            conn("EARLY", "MILTNKC", "MANCPIC", 300, 360),
            conn("U1", "EUSTON", "MILTNKC", 480, 530),
        ];
        let interchange = interchange_with_change_times(
            &[
                ("EUS", "EUSTON"),
                ("MKC", "MILTNKC"),
                ("MAN", "MANCPIC"),
                ("YRK", "YORK"),
            ],
            &[],
        );
        // MKC -> MAN has no train after 08:55, so MAN -> YRK has no start.
        let segments = plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "EUS",
            &["MKC".to_string(), "MAN".to_string()],
            "YRK",
            NaiveTime::MIN,
            "fastest",
            DEFAULT_MAX_CHANGES,
        )
        .unwrap();
        assert_eq!(segments.len(), 3);
        assert!(segments[1].itineraries.is_empty(), "{segments:?}");
        assert_eq!(segments[1].depart_after_min, Some(535));
        assert!(segments[2].itineraries.is_empty());
        assert_eq!(segments[2].depart_after_min, None);

        let err = plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "EUS",
            &["MKC".to_string(), "MAN".to_string()],
            "ZZZ",
            NaiveTime::MIN,
            "fastest",
            DEFAULT_MAX_CHANGES,
        )
        .unwrap_err();
        assert!(err.contains("MAN -> ZZZ"), "{err}");
    }

    /// In `options` mode the next segment chains from the EARLIEST-arriving
    /// option, so every onward option is reachable from at least one choice.
    #[test]
    fn options_mode_chains_from_the_earliest_arriving_option() {
        let connections = vec![
            // Direct but slow (arrives 560) vs one change, fast (arrives 520).
            conn("SLOW", "EUSTON", "MILTNKC", 480, 560),
            conn("F1", "EUSTON", "WATFDJ", 485, 495),
            conn("F2", "WATFDJ", "MILTNKC", 500, 520),
            conn("ONWARD1", "MILTNKC", "MANCPIC", 527, 590),
            conn("ONWARD2", "MILTNKC", "MANCPIC", 570, 630),
        ];
        let interchange = interchange_with_change_times(
            &[("EUS", "EUSTON"), ("MKC", "MILTNKC"), ("MAN", "MANCPIC")],
            &[("EUSTON", 0), ("WATFDJ", 5), ("MILTNKC", 5), ("MANCPIC", 0)],
        );
        let segments = plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "EUS",
            &["MKC".to_string()],
            "MAN",
            NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            "options",
            DEFAULT_MAX_CHANGES,
        )
        .unwrap();
        assert_eq!(segments[0].itineraries.len(), 2, "{segments:?}");
        assert_eq!(segments[1].depart_after_min, Some(525));
        assert_eq!(train_uids(&segments[1].itineraries[0]), ["ONWARD1"]);
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
            DEFAULT_MAX_CHANGES,
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
            DEFAULT_MAX_CHANGES,
        )
        .unwrap_err();
        assert!(err.contains("fastest"));
        assert!(err.contains("options"));
    }
}
