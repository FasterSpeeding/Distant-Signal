//! Turns a raw `trip_planner::Journey`/`RaptorJourney` (TIPLOC-keyed,
//! minutes-from-midnight) into the CRS/human-time-keyed shape
//! `routes::trips` serializes, applies this feature's interchange cap
//! (design spec §4: ≤2 by default, caller-raisable to ≤6 via
//! `?maxChanges=`; a hard filter in `options` mode only, a flag in
//! `fastest` mode -- see [`plan_segment`]), and chains ordered waypoints
//! into independently-solved sub-journeys (design spec §4: "Not a
//! traveling-salesman-style... problem"). See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Calls for the reasoning behind the cap-enforcement and
//! waypoint-chaining choices below.

use chrono::{NaiveDate, NaiveTime};
use schedule_query::InterchangeData;
use trip_planner::{ArriveByOptions, JourneyLeg, RaptorJourney, Restrictions};

/// Design spec §4's default cap, at most 2 interchanges (3 legs) per
/// computed itinerary -- applied here, in the presentation layer, never
/// inside `scan_connections`/`raptor_search` themselves (Phase 3/4's own
/// Judgment Calls: neither algorithm has an interchange-count concept built
/// in). This is what `GET /Trips/plan` uses when the caller omits
/// `?maxChanges=`, so existing callers (this app's own frontend included)
/// see exactly the pre-parameter behaviour.
pub const DEFAULT_MAX_CHANGES: u32 = 2;

/// Upper bound on a caller-requested `?maxChanges=`. It was 4, the
/// sibling `Distant-Signal-MCP` project's `PLAN_MAX_CHANGES` default; that
/// project fell back to its own engine for anything higher. Raised to 6 on
/// 2026-10-06 so DS serves those requests too (an obscure cross-country
/// route can need 5 or 6). Bounded rather than open because every extra
/// change is one more full RAPTOR sweep over the day's connections graph in
/// `options` mode -- see `routes::trips::get_trip_plan`'s own `DoS` notes for
/// the worst-case arithmetic, and `routes::trips::MAX_OPTIONS_SEARCH_SIZE`
/// for the guard that keeps 6 affordable with many waypoints or vias.
pub const MAX_CHANGES_LIMIT: u32 = 6;

/// Phase 4's own Judgment Call 2: `max_rounds` for RAPTOR is NEVER the
/// library's own default -- `max_changes + 1` trips needed for
/// `max_changes` changes, plus one further round of headroom to detect
/// whether the cap actually bound the answer. Derived from the cap actually
/// in effect for THIS request, never from a fixed constant, so a raised cap
/// can't silently lose its headroom round (or its within-cap answers).
pub const fn max_rounds(max_changes: u32) -> u32 {
    max_changes + 2
}

// The train variant is much larger than the transfer one. A response holds a
// few dozen legs at most and nearly all of them are trains, so boxing it
// would add an allocation per leg to save nothing.
#[expect(
    clippy::large_enum_variant,
    reason = "nearly every leg is a train, so boxing would add an allocation per leg; see above"
)]
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
        /// The public (GBTT) departure at the boarding call and arrival at
        /// the alighting call -- what the passenger timetable and station
        /// screens show, and what the planner searched on (falling back to
        /// the working time for a call with none). `scheduled_departure`/
        /// `scheduled_arrival` are the working (WTT) times; they are kept as
        /// WTT for one release (deprecated), then switched to public or
        /// removed. `None` until
        /// `trip_leg_details::attach_leg_details` fills them in, and when the
        /// CIF has no public time there (or the schedule predates the
        /// column). Local clock time, like `scheduled_*`.
        public_departure: Option<NaiveTime>,
        public_arrival: Option<NaiveTime>,
        /// Days past `service_date` of `public_departure`/`public_arrival`.
        /// Usually the WTT time's own offset; differs when rounding crosses
        /// midnight (a 23:59H WTT arrival is a 00:00 public one).
        public_departure_day_offset: Option<u8>,
        public_arrival_day_offset: Option<u8>,
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
        /// `serviceMode` (`train`/`replacementBus`/`bus`/`ferry`) and
        /// `liveTracking` (additive, 2026-10-06). `kind` stays `"train"` for
        /// a bus or ferry leg so existing clients keep working; these say
        /// what the vehicle really is. Filled in by
        /// `trip_leg_details::attach_leg_details`; a train until then.
        #[serde(flatten)]
        service: crate::data::schedule_services::ServiceModeFields,
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
    /// Internal: the TIPLOC the itinerary starts from -- what an arrive-by
    /// chain ([`chain_deadline_min`]) charges the change time at.
    #[serde(skip)]
    pub departure_tiploc: Option<String>,
    /// Live overlay only (absent otherwise): no cancelled leg and no change
    /// that live times make impossible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_feasible: Option<bool>,
    /// The first leg is the same train the previous segment's itinerary rode
    /// into the waypoint: the traveller stays aboard and makes no change
    /// there (additive, 2026-09-29; always `false` for the first segment).
    pub continues_previous_train: bool,
    /// Internal: the vias this itinerary's legs satisfied, collected into
    /// the response's `journeys[j].viaSatisfiedBy` (see [`journey_summaries`]).
    #[serde(skip)]
    pub via_satisfied_by: Vec<ViaSatisfied>,
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
            continues_previous_train: false,
            via_satisfied_by: Vec::new(),
            arrival_tiploc: legs.last().map(|leg| match leg {
                JourneyLeg::Train(train) => train.to_tiploc.clone(),
                JourneyLeg::Transfer(transfer) => transfer.to_tiploc.clone(),
            }),
            departure_tiploc: legs.first().map(|leg| match leg {
                JourneyLeg::Train(train) => train.from_tiploc.clone(),
                JourneyLeg::Transfer(transfer) => transfer.from_tiploc.clone(),
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

#[expect(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    reason = "minute and day-offset values are bounded by a service day or two; a constant or range-checked time is always valid"
)]
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
            // `scheduled*` stay the working-timetable times for one release
            // (design doc §9 decision 1); the search itself, and
            // `departure_min`/`arrival_min` below, are on public times
            // (`schedule_query::Connection::departure_min`).
            let (scheduled_departure, departure_day_offset) =
                minutes_to_clock(train.working_departure_min);
            let (scheduled_arrival, arrival_day_offset) =
                minutes_to_clock(train.working_arrival_min);
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
                public_departure: None,
                public_arrival: None,
                public_departure_day_offset: None,
                public_arrival_day_offset: None,
                operator: None,
                headcode: None,
                service: crate::data::schedule_services::ServiceModeFields::default(),
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

#[expect(
    clippy::cast_possible_truncation,
    reason = "a journey has a handful of legs and rounds"
)]
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
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
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
    let lookup = |code: &str| {
        interchange
            .crs_to_tiplocs
            .get(&common::location_naming::normalize_location_code(code))
            .cloned()
            .unwrap_or_default()
    };
    let from_tiplocs = lookup(origin_crs);
    let to_tiplocs = lookup(destination_crs);
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
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
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
    SegmentSearch {
        connections,
        interchange,
        date,
        results,
        max_changes,
        overlay,
        restrictions: None,
    }
    .search(
        origin_crs,
        destination_crs,
        TimeBound::DepartAfter(departure_min),
    )
}

/// Which end of a segment (or trip) the caller's time pins, in minutes from
/// service-day midnight (may exceed 1440).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBound {
    /// `departAfter`: leave no earlier than this.
    DepartAfter(u32),
    /// `arriveBy`: arrive no later than this; the latest-departing
    /// itineraries that do are wanted.
    ArriveBy(u32),
}

/// Everything one segment search needs besides its endpoints and time.
#[derive(Clone, Copy)]
pub struct SegmentSearch<'a> {
    pub connections: &'a [schedule_query::Connection],
    pub interchange: &'a InterchangeData,
    pub date: NaiveDate,
    /// `"fastest"` or `"options"` -- see [`plan_segment`].
    pub results: &'a str,
    pub max_changes: u32,
    pub overlay: Option<&'a trip_planner::ConnectionOverlay>,
    /// `avoid`/`avoidStop`/`avoidChange`, see [`build_restrictions`].
    pub restrictions: Option<&'a Restrictions>,
}

impl SegmentSearch<'_> {
    /// One segment, `bound` either way. Returns the itineraries (possibly
    /// none) and `cappedByMaxChanges`, or `Err` for an unrecognised CRS.
    ///
    /// Arrive-by: `fastest` is the single latest-departing itinerary that
    /// arrives in time (earliest-arriving among those leaving then);
    /// `options` is, for each number of changes up to `max_changes`, the
    /// latest departure that makes it -- one itinerary per change count,
    /// fewest changes first, each departing strictly later than the one
    /// before (a later departure is the arrive-by analogue of an earlier
    /// arrival). `capped` when a later departure needing more changes exists.
    pub fn search(
        &self,
        origin_crs: &str,
        destination_crs: &str,
        bound: TimeBound,
    ) -> Result<(Vec<PlannedItinerary>, bool), String> {
        let (from_tiplocs, to_tiplocs) =
            resolve_segment_tiplocs(self.interchange, origin_crs, destination_crs)?;
        match (self.results, bound) {
            ("fastest", _) => {
                let Some(journey) =
                    self.fastest(&from_tiplocs, &to_tiplocs, bound, self.restrictions)
                else {
                    return Ok((Vec::new(), false));
                };
                let change_count = train_leg_count(&journey.legs).saturating_sub(1);
                let itinerary = PlannedItinerary::from_legs(
                    &journey.legs,
                    journey.departure_min,
                    journey.arrival_min,
                    change_count,
                    // See this plan's Judgment Call 3: CSA has no cap of its
                    // own, so a genuinely-fastest answer that needs more than
                    // the requested cap is still returned, honestly flagged,
                    // not hidden.
                    Some(change_count > self.max_changes),
                    self.date,
                    self.interchange,
                );
                Ok((vec![itinerary], false))
            }
            ("options", TimeBound::DepartAfter(departure_min)) => {
                let all: Vec<RaptorJourney> = trip_planner::raptor_search_restricted(
                    trip_planner::RaptorOptions {
                        connections: self.connections,
                        interchange: self.interchange,
                        from_tiplocs: &from_tiplocs,
                        to_tiplocs: &to_tiplocs,
                        departure_min,
                        date: self.date,
                        max_rounds: max_rounds(self.max_changes),
                    },
                    self.overlay,
                    self.restrictions,
                );
                let within_cap: Vec<&RaptorJourney> = all
                    .iter()
                    .filter(|j| j.changes <= self.max_changes)
                    .collect();
                // Judgment Call 2: did the headroom round (`max_rounds`, one
                // past what `max_changes` alone needs) find something
                // strictly better than every within-cap entry? If so, the
                // cap genuinely bound the answer -- flagged honestly, not
                // silently swallowed.
                let best_within_cap = within_cap.iter().map(|j| j.arrival_min).min();
                let capped = all.iter().any(|j| {
                    j.changes > self.max_changes
                        && best_within_cap.is_none_or(|best| j.arrival_min < best)
                });
                Ok((self.itineraries(within_cap), capped))
            }
            ("options", TimeBound::ArriveBy(arrive_by_min)) => {
                let options = self.arrive_by_options(&from_tiplocs, &to_tiplocs, arrive_by_min);
                let latest = trip_planner::latest_departures_by_trips(
                    &options,
                    self.overlay,
                    self.restrictions,
                    max_rounds(self.max_changes),
                );
                // `latest[k - 1]` is the latest departure with at most `k`
                // trains; within the cap is `k <= max_changes + 1`.
                let (within, beyond) = latest.split_at((self.max_changes + 1) as usize);
                let best_within_cap = within.iter().flatten().max();
                let capped = beyond
                    .iter()
                    .flatten()
                    .any(|t| best_within_cap.is_none_or(|best| t > best));
                let journeys = trip_planner::raptor_arrive_by_from_latest(
                    &options,
                    self.overlay,
                    self.restrictions,
                    within,
                );
                Ok((
                    self.itineraries(
                        journeys
                            .iter()
                            .filter(|j| j.changes <= self.max_changes)
                            .collect(),
                    ),
                    capped,
                ))
            }
            (results, _) => Err(format!(
                "results must be 'fastest' or 'options', not '{results}'"
            )),
        }
    }

    fn itineraries(&self, journeys: Vec<&RaptorJourney>) -> Vec<PlannedItinerary> {
        journeys
            .into_iter()
            .map(|journey| {
                PlannedItinerary::from_legs(
                    &journey.legs,
                    journey.departure_min,
                    journey.arrival_min,
                    journey.changes,
                    None,
                    self.date,
                    self.interchange,
                )
            })
            .collect()
    }

    fn arrive_by_options<'b>(
        &'b self,
        from_tiplocs: &'b [String],
        to_tiplocs: &'b [String],
        arrive_by_min: u32,
    ) -> ArriveByOptions<'b> {
        ArriveByOptions {
            connections: self.connections,
            interchange: self.interchange,
            from_tiplocs,
            waypoints: &[],
            to_tiplocs,
            vias: None,
            arrive_by_min,
            date: self.date,
        }
    }

    /// The single CSA journey for `bound` under `restrictions`.
    fn fastest(
        &self,
        from_tiplocs: &[String],
        to_tiplocs: &[String],
        bound: TimeBound,
        restrictions: Option<&Restrictions>,
    ) -> Option<trip_planner::Journey> {
        match bound {
            TimeBound::DepartAfter(departure_min) => trip_planner::scan_connections_restricted(
                trip_planner::ScanOptions {
                    connections: self.connections,
                    interchange: self.interchange,
                    from_tiplocs,
                    to_tiplocs,
                    departure_min,
                    date: self.date,
                },
                self.overlay,
                restrictions,
            ),
            TimeBound::ArriveBy(arrive_by_min) => trip_planner::scan_connections_arrive_by(
                self.arrive_by_options(from_tiplocs, to_tiplocs, arrive_by_min),
                self.overlay,
                restrictions,
            ),
        }
    }
}

/// Why a segment has no itineraries -- served as the segment's
/// `noResultReason` (absent when it has some). Adapted from `train-mcp`'s
/// `ConstraintFailure` (`src/timetable/plan/constraints.ts`): "no journey
/// found" is nearly useless once several constraints are in play, so this
/// names the one that made the query infeasible.
///
/// `constraint` is one of:
///
/// - `maxChanges` (`results=options`): itineraries exist, but every one
///   needs more changes than the cap;
/// - `avoid`, `avoidStop`, `avoidChange`: an itinerary exists without that
///   list (`values`), none with it;
/// - `avoidCombined`: one exists without the avoid lists, but dropping any
///   single list is not enough (`values` is every avoided code);
/// - `via` (2026-10-06): one exists without the pass-through vias; `values`
///   is the single via whose removal alone is enough, or every via when no
///   single one is (every segment carries it);
/// - `departAfter` / `arriveBy`: nothing leaves late enough / arrives early
///   enough (`values` is the time, `HH:MM`), though something runs that day;
/// - `noRoute`: nothing reaches the destination that service day at all
///   (under the avoid lists, if any);
/// - `previousSegment` / `nextSegment`: a waypoint segment that was not
///   searched, because the one it chains from found nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoResultReason {
    pub constraint: &'static str,
    pub values: Vec<String>,
    pub message: String,
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
    /// Depart-after requests: minutes from service-day midnight this segment
    /// was searched from -- the caller's `departAfter` for the first
    /// segment, the previous segment's earliest arrival plus the waypoint's
    /// minimum change time for every later one, `None` when the previous
    /// segment found nothing to chain from (so this one was not searched).
    /// May exceed 1440. Always `None` for an arrive-by request.
    pub depart_after_min: Option<u32>,
    /// Arrive-by requests: the latest arrival this segment was searched for
    /// -- the caller's `arriveBy` for the last segment, the next segment's
    /// latest departure less the waypoint's minimum change time for earlier
    /// ones ([`chain_deadline_min`]), `None` when the next segment found
    /// nothing. Always `None` for a depart-after request.
    pub arrive_by_min: Option<u32>,
    /// Set only when `itineraries` is empty -- see [`NoResultReason`].
    pub no_result_reason: Option<NoResultReason>,
}

/// Change time used when chaining a waypoint whose own minimum change time
/// is the `NoInterchange` sentinel: the traveller explicitly asked to stop
/// there, so the station's "never change here" marker (the coach-stand
/// sentinels) says nothing about how long they need -- use the same 5-minute
/// default `schedule_query::minimum_change_time` gives a TIPLOC with no MSN
/// record at all.
const WAYPOINT_FALLBACK_CHANGE_MINUTES: u32 = 5;

/// The minimum change time a waypoint chain charges at `tiploc` -- the same
/// figure the CSA/RAPTOR searches charge for an ordinary change there.
fn waypoint_change_minutes(interchange: &InterchangeData, tiploc: Option<&str>) -> u32 {
    tiploc.map_or(WAYPOINT_FALLBACK_CHANGE_MINUTES, |tiploc| {
        match schedule_query::minimum_change_time(interchange, tiploc) {
            schedule_query::ChangeTime::Finite(minutes) => minutes,
            schedule_query::ChangeTime::NoInterchange => WAYPOINT_FALLBACK_CHANGE_MINUTES,
        }
    })
}

/// When the traveller can leave the waypoint `itinerary` arrives at: its
/// arrival plus the minimum change time at the TIPLOC it arrives at.
fn ready_after(itinerary: &PlannedItinerary, interchange: &InterchangeData) -> u32 {
    itinerary.arrival_min
        + waypoint_change_minutes(interchange, itinerary.arrival_tiploc.as_deref())
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

/// The arrive-by mirror of [`chain_ready_min`]: the latest a segment
/// PRECEDING `itineraries` may arrive at the waypoint -- the latest
/// departure among them, less the minimum change time at the TIPLOC it
/// leaves from (searching to the latest keeps every one of them reachable
/// from at least one earlier option). `None` when that segment found
/// nothing.
pub fn chain_deadline_min(
    itineraries: &[PlannedItinerary],
    interchange: &InterchangeData,
) -> Option<u32> {
    itineraries
        .iter()
        .filter_map(|itinerary| {
            itinerary.departure_min.checked_sub(waypoint_change_minutes(
                interchange,
                itinerary.departure_tiploc.as_deref(),
            ))
        })
        .max()
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
///
/// [`plan_trip`] is the general form (arrive-by, avoid lists).
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
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
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
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
    plan_chained(&TripPlanInput {
        search: SegmentSearch {
            connections,
            interchange,
            date,
            results,
            max_changes,
            overlay,
            restrictions: None,
        },
        passes: None,
        avoid: &AvoidLists::default(),
        origin_crs,
        waypoints,
        destination_crs,
        vias: &[],
        via_search: None,
        time: TimeBound::DepartAfter(clock_minutes(departure_after)),
    })
}

/// The station lists of `?avoid=`, `?avoidStop=` and `?avoidChange=`, as
/// the caller named them (CRS codes). See [`build_restrictions`] for what
/// each means.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvoidLists {
    pub avoid: Vec<String>,
    pub avoid_stop: Vec<String>,
    pub avoid_change: Vec<String>,
}

impl AvoidLists {
    pub fn is_empty(&self) -> bool {
        self.avoid.is_empty() && self.avoid_stop.is_empty() && self.avoid_change.is_empty()
    }

    /// `(wire name, codes)` for every list, in a fixed order.
    pub fn lists(&self) -> [(&'static str, &[String]); 3] {
        [
            ("avoid", &self.avoid),
            ("avoidStop", &self.avoid_stop),
            ("avoidChange", &self.avoid_change),
        ]
    }

    fn without(&self, name: &str) -> Self {
        let mut copy = self.clone();
        match name {
            "avoid" => copy.avoid.clear(),
            "avoidStop" => copy.avoid_stop.clear(),
            _ => copy.avoid_change.clear(),
        }
        copy
    }

    /// Every code in every list, in order, deduplicated.
    fn all_codes(&self) -> Vec<String> {
        let mut codes: Vec<String> = Vec::new();
        for (_, list) in self.lists() {
            for code in list {
                if !codes.contains(code) {
                    codes.push(code.clone());
                }
            }
        }
        codes
    }
}

/// "avoiding BHM, CRE" etc. -- how a message describes one list.
fn describe_list(name: &str, codes: &[String]) -> String {
    let codes = codes.join(", ");
    match name {
        "avoid" => format!("avoiding {codes} (not even passing through)"),
        "avoidStop" => format!("not calling at {codes}"),
        _ => format!("not changing at {codes}"),
    }
}

/// The search restrictions `lists` expand to, or `None` when every list is
/// empty. `Err` names an unrecognised code.
///
/// - `avoid` (as in `train-mcp`): never ride a train that calls at OR runs
///   through the station, and never board, alight or walk there. Needs
///   `passes` (the day's [`schedule_query::PassIndex`]) to see the trains
///   that run through without calling.
/// - `avoidStop` (as in `train-mcp`): never ride a train that calls there
///   (running through without stopping is fine), never board, alight or
///   walk there.
/// - `avoidChange` (Distant Signal's own, the loosest): never board, alight,
///   change or walk there; staying aboard a train that calls there is fine.
///
/// Every CRS code covers all of its TIPLOCs.
pub fn build_restrictions(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    passes: Option<&schedule_query::PassIndex>,
    lists: &AvoidLists,
) -> Result<Option<Restrictions>, String> {
    use std::collections::{HashMap, HashSet};

    if lists.is_empty() {
        return Ok(None);
    }
    let tiplocs_of = |name: &str, codes: &[String]| -> Result<Vec<String>, String> {
        let mut tiplocs = Vec::new();
        for code in codes {
            match interchange.crs_to_tiplocs.get(code) {
                Some(found) if !found.is_empty() => tiplocs.extend(found.iter().cloned()),
                _ => {
                    return Err(format!(
                        "{name}: '{code}' is not a recognised station CRS code"
                    ));
                }
            }
        }
        Ok(tiplocs)
    };
    let avoid = tiplocs_of("avoid", &lists.avoid)?;
    let avoid_stop = tiplocs_of("avoidStop", &lists.avoid_stop)?;
    let avoid_change = tiplocs_of("avoidChange", &lists.avoid_change)?;

    // Trains that run through an `avoid` station without calling: every one
    // of their base connections in order, the spanning ones blocked.
    let mut pass_legs: HashMap<String, Vec<trip_planner::PassLeg>> = HashMap::new();
    if let Some(passes) = passes
        && !avoid.is_empty()
    {
        let blocked: HashSet<usize> = avoid
            .iter()
            .flat_map(|tiploc| passes.connections_passing(tiploc))
            .map(|index| index as usize)
            .collect();
        let uids: HashSet<&str> = blocked
            .iter()
            .filter_map(|&index| connections.get(index))
            .map(|c| c.uid.as_str())
            .collect();
        if !uids.is_empty() {
            for (index, connection) in connections.iter().enumerate() {
                if uids.contains(connection.uid.as_str()) {
                    pass_legs.entry(connection.uid.clone()).or_default().push(
                        trip_planner::PassLeg {
                            from_tiploc: connection.from_tiploc.clone(),
                            to_tiploc: connection.to_tiploc.clone(),
                            blocked: blocked.contains(&index),
                        },
                    );
                }
            }
        }
    }

    let no_call: Vec<String> = avoid.iter().chain(&avoid_stop).cloned().collect();
    Ok(Some(Restrictions::new(avoid_change, no_call, pass_legs)))
}

/// Which leg passed one via: served per journey as
/// `journeys[j].viaSatisfiedBy[k]` for the request's `via[k]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViaSatisfied {
    /// The via, as requested (`KGX|EUS` or `group:LON` for an OR choice).
    pub crs: String,
    /// The station that satisfied it (2026-10-07): for an OR choice, the
    /// member the journey passed; otherwise the via itself. A CRS, or a
    /// `tiploc:` code for a bus stop or ferry terminal; `null` only if the
    /// TIPLOC has no code (not expected).
    pub matched_crs: Option<String>,
    /// Index into `segments` (the part of the journey).
    pub segment: usize,
    /// Index into that segment's `itineraries[j].legs`.
    pub leg: usize,
    /// `call` (the train called there: the traveller stayed aboard, boarded
    /// or alighted), `pass` (it ran through without calling, or past a
    /// call the live overlay cancelled) or `walk` (a transfer leg into it).
    pub how: &'static str,
}

/// The pass-through vias `?via=` asks for, as the search state
/// ([`trip_planner::Vias`]), or `None` when there are none. `Err` names an
/// unrecognised code.
///
/// Every CRS code covers all of its TIPLOCs. A via is passed by a train
/// that calls there or runs through it; running through is only visible
/// where `passes` (the day's [`schedule_query::PassIndex`]) has a row, and
/// CIF records one only at a timing point -- so at a station that is not a
/// timing point, only a call (or a change or walk there) satisfies the via.
/// Every train that calls at or passes a via gets its base connections as
/// [`trip_planner::PassSpan`]s, so a live replacement over a cancelled call
/// there still counts as passing it.
///
/// `vias[v]` is via `v`'s station codes (2026-10-07): one for a plain via,
/// several for an OR choice (`KGX|EUS`, `group:LON`; see
/// [`crate::data::station_groups`]), which is passed by ANY of them. Its
/// targets are the union of their TIPLOCs, so it is still one via (one
/// step of progress) to the search.
pub fn build_vias(
    connections: &[schedule_query::Connection],
    interchange: &InterchangeData,
    passes: Option<&schedule_query::PassIndex>,
    vias: &[Vec<String>],
) -> Result<Option<trip_planner::Vias>, String> {
    use std::collections::{HashMap, HashSet};

    if vias.is_empty() {
        return Ok(None);
    }
    let mut targets: Vec<Vec<String>> = Vec::with_capacity(vias.len());
    for codes in vias {
        let mut tiplocs: Vec<String> = Vec::new();
        for code in codes {
            match interchange.crs_to_tiplocs.get(code) {
                Some(found) if !found.is_empty() => tiplocs.extend(found.iter().cloned()),
                _ => {
                    return Err(format!(
                        "via: '{code}' is not a recognised station CRS code"
                    ));
                }
            }
        }
        targets.push(tiplocs);
    }
    let all: HashSet<&str> = targets
        .iter()
        .flatten()
        .map(|t| schedule_query::normalize_tiploc(t))
        .collect();
    // Per connection index, the via TIPLOCs it runs through, by position.
    let mut passed_by: HashMap<usize, Vec<(u16, &str)>> = HashMap::new();
    if let Some(passes) = passes {
        for &tiploc in &all {
            for row in passes.passes_at(tiploc) {
                passed_by
                    .entry(row.connection as usize)
                    .or_default()
                    .push((row.position, tiploc));
            }
        }
    }
    let mut uids: HashSet<&str> = passed_by
        .keys()
        .filter_map(|&index| connections.get(index))
        .map(|c| c.uid.as_str())
        .collect();
    for connection in connections {
        if all.contains(schedule_query::normalize_tiploc(&connection.from_tiploc))
            || all.contains(schedule_query::normalize_tiploc(&connection.to_tiploc))
        {
            uids.insert(connection.uid.as_str());
        }
    }
    let mut spans: HashMap<String, Vec<trip_planner::PassSpan>> = HashMap::new();
    for (index, connection) in connections.iter().enumerate() {
        if !uids.contains(connection.uid.as_str()) {
            continue;
        }
        let mut ordered = passed_by.get(&index).cloned().unwrap_or_default();
        ordered.sort_unstable();
        spans
            .entry(connection.uid.clone())
            .or_default()
            .push(trip_planner::PassSpan {
                from_tiploc: connection.from_tiploc.clone(),
                to_tiploc: connection.to_tiploc.clone(),
                departure_min: connection.departure_min,
                passed: ordered.into_iter().map(|(_, t)| t.to_string()).collect(),
            });
    }
    Ok(Some(trip_planner::Vias::new(&targets, spans)))
}

/// Everything [`plan_trip`] needs.
pub struct TripPlanInput<'a> {
    /// `search.restrictions` must be [`build_restrictions`] of `avoid`.
    pub search: SegmentSearch<'a>,
    /// The day's pass index, for `avoid` -- and for rebuilding the
    /// restrictions without one list when explaining an empty segment.
    pub passes: Option<&'a schedule_query::PassIndex>,
    pub avoid: &'a AvoidLists,
    pub origin_crs: &'a str,
    pub waypoints: &'a [String],
    pub destination_crs: &'a str,
    /// `?via=`: CRS codes to pass through, in order.
    pub vias: &'a [String],
    /// [`build_vias`] of `vias` (`None` when there are none).
    pub via_search: Option<&'a trip_planner::Vias>,
    /// Applies to the first segment (`DepartAfter`) or the last
    /// (`ArriveBy`); the others are chained from their neighbour.
    pub time: TimeBound,
}

/// The whole `/Trips/plan` computation: `origin -> waypoints... ->
/// destination`, with every empty segment explained (`no_result_reason`).
///
/// With no waypoints this is one [`SegmentSearch`]. With waypoints the trip
/// is searched as ONE journey that calls at every waypoint in order
/// (`trip_planner::staged`, 2026-09-29): a traveller who stays aboard a
/// train calling at a waypoint makes no change there (that part is marked
/// `continues_previous_train`), a fresh boarding at a waypoint costs its
/// minimum change time (5 minutes at a `NoInterchange` sentinel), and
/// `max_changes` caps the changes of the WHOLE journey. The journey is then
/// split at the waypoints into one itinerary per segment, and the segments'
/// itinerary lists are ALIGNED: `segments[s].itineraries[j]` is part `s` of
/// journey `j` (see [`journey_summaries`]). A waypoint is satisfied by a
/// call there (or a walk into it), not by a train passing through without
/// stopping.
///
/// Until 2026-09-29 each segment was searched on its own and chained
/// ([`plan_via_waypoints`]); that per-segment planner is still what explains
/// an infeasible joint plan: it finds the segment that fails and why.
///
/// Pass-through vias (`vias`, 2026-10-06) take the same joint search: the
/// journey must also pass through every via in order, by a train calling
/// there or running through it, or by changing or walking there (see
/// [`build_vias`] and `trip_planner::via`). Vias and waypoints are two
/// independent ordered lists; a via may fall in any segment. Each journey
/// reports which leg passed each via ([`ViaSatisfied`]).
///
/// Every segment's CRS codes are validated before any search, so a bad
/// code is a 400 naming the first segment that has one. A waypoint equal to
/// the origin, the destination or the waypoint before it is a 400 too, and
/// so is a via equal to the origin or the destination.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
pub fn plan_trip(input: &TripPlanInput<'_>) -> Result<Vec<SegmentResult>, String> {
    if input.waypoints.is_empty() && input.vias.is_empty() {
        return plan_chained(input);
    }
    let search = &input.search;
    let interchange = search.interchange;
    let mut stops: Vec<&str> = vec![input.origin_crs];
    stops.extend(input.waypoints.iter().map(String::as_str));
    stops.push(input.destination_crs);
    let pairs: Vec<(&str, &str)> = stops.windows(2).map(|pair| (pair[0], pair[1])).collect();
    let mut tiplocs: Vec<Vec<String>> = Vec::with_capacity(stops.len());
    for &(from, to) in &pairs {
        let (from_tiplocs, to_tiplocs) = resolve_segment_tiplocs(interchange, from, to)
            .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
        if tiplocs.is_empty() {
            tiplocs.push(from_tiplocs);
        }
        tiplocs.push(to_tiplocs);
    }
    for (index, waypoint) in input.waypoints.iter().enumerate() {
        let clash = if waypoint.eq_ignore_ascii_case(input.origin_crs) {
            Some("the origin")
        } else if waypoint.eq_ignore_ascii_case(input.destination_crs) {
            Some("the destination")
        } else if index > 0 && waypoint.eq_ignore_ascii_case(&input.waypoints[index - 1]) {
            Some("the waypoint before it")
        } else {
            None
        };
        if let Some(clash) = clash {
            return Err(format!(
                "waypoint '{waypoint}' is {clash}; remove it or name a different station"
            ));
        }
    }
    for via in input.vias {
        let clash = if via.eq_ignore_ascii_case(input.origin_crs) {
            "the origin"
        } else if via.eq_ignore_ascii_case(input.destination_crs) {
            "the destination"
        } else {
            continue;
        };
        return Err(format!(
            "via: '{via}' is {clash}; every journey passes it already"
        ));
    }
    if !input.vias.is_empty() && input.via_search.is_none() {
        return Err("via: the vias were not resolved".to_string());
    }

    let last = tiplocs.len() - 1;
    let options = ArriveByOptions {
        connections: search.connections,
        interchange,
        from_tiplocs: &tiplocs[0],
        waypoints: &tiplocs[1..last],
        to_tiplocs: &tiplocs[last],
        vias: input.via_search,
        arrive_by_min: match input.time {
            TimeBound::ArriveBy(deadline) => deadline,
            TimeBound::DepartAfter(_) => 0,
        },
        date: search.date,
    };
    let staged = trip_planner::StagedOptions {
        connections: search.connections,
        interchange,
        from_tiplocs: &tiplocs[0],
        waypoints: &tiplocs[1..last],
        to_tiplocs: &tiplocs[last],
        vias: input.via_search,
        date: search.date,
    };
    let (overlay, restrictions) = (search.overlay, search.restrictions);
    let rounds = max_rounds(search.max_changes);
    let (journeys, capped): (Vec<trip_planner::StagedJourney>, bool) =
        match (search.results, input.time) {
            ("fastest", TimeBound::DepartAfter(start)) => (
                trip_planner::scan_staged(&staged, start, overlay, restrictions)
                    .into_iter()
                    .collect(),
                false,
            ),
            ("fastest", TimeBound::ArriveBy(_)) => (
                trip_planner::staged_arrive_by(&options, overlay, restrictions)
                    .into_iter()
                    .collect(),
                false,
            ),
            ("options", TimeBound::DepartAfter(start)) => {
                let all =
                    trip_planner::raptor_staged(&staged, start, rounds, overlay, restrictions);
                let best_within_cap = all
                    .iter()
                    .filter(|j| j.changes <= search.max_changes)
                    .map(|j| j.arrival_min)
                    .min();
                let capped = all.iter().any(|j| {
                    j.changes > search.max_changes
                        && best_within_cap.is_none_or(|best| j.arrival_min < best)
                });
                (
                    all.into_iter()
                        .filter(|j| j.changes <= search.max_changes)
                        .collect(),
                    capped,
                )
            }
            ("options", TimeBound::ArriveBy(_)) => {
                let latest = trip_planner::latest_departures_by_trips(
                    &options,
                    overlay,
                    restrictions,
                    rounds,
                );
                let (within, beyond) = latest.split_at((search.max_changes + 1) as usize);
                let best_within_cap = within.iter().flatten().max();
                let capped = beyond
                    .iter()
                    .flatten()
                    .any(|t| best_within_cap.is_none_or(|best| t > best));
                (
                    trip_planner::staged_raptor_arrive_by_from_latest(
                        &options,
                        overlay,
                        restrictions,
                        within,
                    )
                    .into_iter()
                    .filter(|j| j.changes <= search.max_changes)
                    .collect(),
                    capped,
                )
            }
            (results, _) => {
                return Err(format!(
                    "results must be 'fastest' or 'options', not '{results}'"
                ));
            }
        };

    let fastest = search.results == "fastest";
    let mut segments: Vec<SegmentResult> = pairs
        .iter()
        .map(|&(from, to)| SegmentResult {
            origin_crs: from.to_string(),
            destination_crs: to.to_string(),
            itineraries: Vec::with_capacity(journeys.len()),
            capped_by_max_changes: capped,
            depart_after_min: None,
            arrive_by_min: None,
            no_result_reason: None,
        })
        .collect();
    for journey in &journeys {
        let mut previous_arrival = journey.departure_min;
        for (segment, part) in segments.iter_mut().zip(&journey.parts) {
            let departure = part
                .legs
                .first()
                .map_or(previous_arrival, leg_departure_min);
            let arrival = part.legs.last().map_or(departure, leg_arrival_min);
            previous_arrival = arrival;
            let changes = train_leg_count(&part.legs).saturating_sub(1);
            let mut itinerary = PlannedItinerary::from_legs(
                &part.legs,
                departure,
                arrival,
                changes,
                // The whole journey's changes against the cap.
                fastest.then_some(journey.changes > search.max_changes),
                search.date,
                interchange,
            );
            itinerary.continues_previous_train = part.continues_previous_train;
            segment.itineraries.push(itinerary);
        }
        for (via, hit) in input.vias.iter().zip(&journey.via_legs) {
            let how = match hit.how {
                trip_planner::ViaHow::Call => "call",
                trip_planner::ViaHow::Pass => "pass",
                trip_planner::ViaHow::Walk => "walk",
            };
            if let Some(itinerary) = segments
                .get_mut(hit.part)
                .and_then(|segment| segment.itineraries.last_mut())
            {
                itinerary.via_satisfied_by.push(ViaSatisfied {
                    crs: via.clone(),
                    matched_crs: crs_for_tiploc(interchange, &hit.tiploc),
                    segment: hit.part,
                    leg: hit.leg,
                    how,
                });
            }
        }
    }

    // What each segment was, in effect, searched from / for.
    match input.time {
        TimeBound::DepartAfter(start) => {
            segments[0].depart_after_min = Some(start);
            for index in 1..segments.len() {
                let ready = (0..journeys.len())
                    .map(|j| {
                        let (previous, next) = (
                            &segments[index - 1].itineraries[j],
                            &segments[index].itineraries[j],
                        );
                        waypoint_ready_min(previous, next, interchange)
                    })
                    .min();
                segments[index].depart_after_min = ready;
            }
        }
        TimeBound::ArriveBy(deadline) => {
            segments[last - 1].arrive_by_min = Some(deadline);
            for index in 0..segments.len() - 1 {
                let latest = (0..journeys.len())
                    .map(|j| {
                        let (previous, next) = (
                            &segments[index].itineraries[j],
                            &segments[index + 1].itineraries[j],
                        );
                        waypoint_deadline_min(previous, next, interchange)
                    })
                    .max();
                segments[index].arrive_by_min = latest;
            }
        }
    }

    if journeys.is_empty() {
        // Would the same plan, without (some of) the vias, find a journey?
        // The fastest one is enough to tell.
        let probe = |vias: Option<&trip_planner::Vias>| match input.time {
            TimeBound::DepartAfter(start) => trip_planner::scan_staged(
                &trip_planner::StagedOptions { vias, ..staged },
                start,
                overlay,
                restrictions,
            )
            .is_some(),
            TimeBound::ArriveBy(_) => trip_planner::staged_arrive_by(
                &ArriveByOptions { vias, ..options },
                overlay,
                restrictions,
            )
            .is_some(),
        };
        let via_reason = if capped {
            None
        } else {
            explain_unpassable_via(input, &probe)
        };
        match via_reason {
            Some(reason) => {
                for segment in &mut segments {
                    segment.no_result_reason = Some(reason.clone());
                }
            }
            None => explain_infeasible_joint_plan(input, &mut segments, capped)?,
        }
    }
    Ok(segments)
}

/// With vias, an empty plan that a plan without them would fill: the reason
/// is `via`, naming the single via whose removal alone is enough when there
/// is one, else every via. `None` when even without the vias nothing is
/// found (another constraint is to blame). `probe(vias)` runs the fastest
/// search with that via list. Costs one to three extra CSA searches, and
/// only for an empty plan.
fn explain_unpassable_via(
    input: &TripPlanInput<'_>,
    probe: &dyn Fn(Option<&trip_planner::Vias>) -> bool,
) -> Option<NoResultReason> {
    let vias = input.via_search?;
    if input.vias.is_empty() || !probe(None) {
        return None;
    }
    let single = if input.vias.len() == 1 {
        Some(0)
    } else {
        (0..input.vias.len()).find(|&index| probe(Some(&vias.without(index))))
    };
    // An OR choice (2026-10-07) reads "any of KGX, EUS" / "any of group:LON".
    let describe = |via: &String| {
        if via.contains('|') || via.starts_with(crate::data::station_groups::GROUP_PREFIX) {
            format!("any of {}", via.replace('|', ", "))
        } else {
            via.clone()
        }
    };
    let (values, described, removed) = match single {
        Some(index) => (
            vec![input.vias[index].clone()],
            describe(&input.vias[index]),
            "that via",
        ),
        None => (
            input.vias.to_vec(),
            format!(
                "{} in that order",
                input
                    .vias
                    .iter()
                    .map(describe)
                    .collect::<Vec<_>>()
                    .join(", then ")
            ),
            "the vias",
        ),
    };
    let phrase = match input.time {
        TimeBound::DepartAfter(start) => format!("departing after {}", clock_label(start)),
        TimeBound::ArriveBy(deadline) => format!("arriving by {}", clock_label(deadline)),
    };
    let through = if input.waypoints.is_empty() {
        String::new()
    } else {
        format!(" through {}", input.waypoints.join(", "))
    };
    Some(NoResultReason {
        constraint: "via",
        values,
        message: format!(
            "No itinerary from {}{through} to {} {phrase} on {} passes through {described} \
             (calling there or not); one exists without {removed}.",
            input.origin_crs, input.destination_crs, input.search.date
        ),
    })
}

fn leg_departure_min(leg: &JourneyLeg) -> u32 {
    match leg {
        JourneyLeg::Train(train) => train.departure_min,
        JourneyLeg::Transfer(transfer) => transfer.departure_min,
    }
}

fn leg_arrival_min(leg: &JourneyLeg) -> u32 {
    match leg {
        JourneyLeg::Train(train) => train.arrival_min,
        JourneyLeg::Transfer(transfer) => transfer.arrival_min,
    }
}

/// When the traveller can leave a waypoint on `next`, having arrived on
/// `previous`: at once when `next` continues the same train, otherwise
/// after the change time at the TIPLOC `next` leaves from (the rule the
/// joint search applies).
pub fn waypoint_ready_min(
    previous: &PlannedItinerary,
    next: &PlannedItinerary,
    interchange: &InterchangeData,
) -> u32 {
    if next.continues_previous_train {
        previous.arrival_min
    } else {
        previous.arrival_min
            + waypoint_change_minutes(interchange, next.departure_tiploc.as_deref())
    }
}

/// The arrive-by mirror of [`waypoint_ready_min`]: the latest `previous`
/// may arrive at the waypoint and still make `next`.
fn waypoint_deadline_min(
    previous: &PlannedItinerary,
    next: &PlannedItinerary,
    interchange: &InterchangeData,
) -> u32 {
    let _ = previous;
    if next.continues_previous_train {
        next.departure_min
    } else {
        next.departure_min.saturating_sub(waypoint_change_minutes(
            interchange,
            next.departure_tiploc.as_deref(),
        ))
    }
}

/// No journey calls at every waypoint: find out why with the per-segment
/// chained planner ([`plan_chained`]). The segment it cannot plan keeps its
/// own reason; every other segment names that one. If every segment plans
/// on its own, the whole-journey change cap is what failed (`options`).
#[expect(
    clippy::expect_used,
    reason = "the invariant is established just above; the expect message names it"
)]
fn explain_infeasible_joint_plan(
    input: &TripPlanInput<'_>,
    segments: &mut [SegmentResult],
    capped: bool,
) -> Result<(), String> {
    let chained = plan_chained(input)?;
    let failing = chained.iter().position(|segment| {
        segment.itineraries.is_empty()
            && !matches!(
                segment.no_result_reason.as_ref().map(|r| r.constraint),
                Some("previousSegment" | "nextSegment")
            )
    });
    let Some(failing) = failing else {
        let search = &input.search;
        let reason = if capped {
            NoResultReason {
                constraint: "maxChanges",
                values: vec![search.max_changes.to_string()],
                message: format!(
                    "No itinerary through every waypoint on {} has {} or fewer changes in all; \
                     one with more exists. Raise maxChanges (at most {MAX_CHANGES_LIMIT}) or use \
                     results=fastest.",
                    search.date, search.max_changes
                ),
            }
        } else {
            NoResultReason {
                constraint: "noRoute",
                values: Vec::new(),
                message: format!(
                    "No itinerary calls at every waypoint in order on {}.",
                    search.date
                ),
            }
        };
        for segment in segments.iter_mut() {
            segment.no_result_reason = Some(reason.clone());
        }
        return Ok(());
    };
    let label = format!(
        "{} -> {}",
        chained[failing].origin_crs, chained[failing].destination_crs
    );
    for (index, segment) in segments.iter_mut().enumerate() {
        segment.no_result_reason = Some(if index == failing {
            chained[failing]
                .no_result_reason
                .clone()
                .expect("an empty chained segment is explained")
        } else {
            NoResultReason {
                constraint: if index < failing {
                    "nextSegment"
                } else {
                    "previousSegment"
                },
                values: vec![label.clone()],
                message: format!(
                    "No itinerary for {} -> {}: the {label} segment has none.",
                    segment.origin_crs, segment.destination_crs
                ),
            }
        });
    }
    Ok(())
}

/// One whole journey of a plan: `segments[s].itineraries[j]` for every `s`,
/// summarised. Served as the response's `journeys[j]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneySummary {
    /// Changes over the whole journey, a train continuing through a
    /// waypoint counted once.
    pub change_count: u32,
    pub departure_min: u32,
    pub arrival_min: u32,
    /// `results=fastest` only: `change_count` is over `maxChanges`.
    pub exceeds_recommended_changes: Option<bool>,
    /// Live overlay only: every part live-feasible, and every waypoint
    /// change still made on live times.
    pub live_feasible: Option<bool>,
    /// For each requested via in order, the leg that passed it.
    pub via_satisfied_by: Vec<ViaSatisfied>,
}

/// The journeys of an aligned plan (see [`plan_trip`]). Empty when the
/// segments' itinerary counts differ (a plan from the chained planner).
#[expect(
    clippy::cast_possible_truncation,
    reason = "a journey has a handful of legs and rounds"
)]
pub fn journey_summaries(
    segments: &[SegmentResult],
    interchange: &InterchangeData,
) -> Vec<JourneySummary> {
    let Some(count) = segments.first().map(|s| s.itineraries.len()) else {
        return Vec::new();
    };
    if segments.iter().any(|s| s.itineraries.len() != count) {
        return Vec::new();
    }
    (0..count)
        .map(|j| {
            let parts: Vec<&PlannedItinerary> =
                segments.iter().map(|s| &s.itineraries[j]).collect();
            let rides: u32 = parts
                .iter()
                .map(|part| {
                    part.legs
                        .iter()
                        .filter(|leg| matches!(leg, PlannedLeg::Train { .. }))
                        .count() as u32
                        - u32::from(part.continues_previous_train)
                })
                .sum();
            let first = parts[0];
            let final_part = parts[parts.len() - 1];
            let live_feasible = if parts.iter().any(|p| p.live_feasible.is_some()) {
                Some(
                    parts.iter().all(|p| p.live_feasible != Some(false))
                        && parts.windows(2).all(|pair| {
                            waypoint_ready_min(pair[0], pair[1], interchange)
                                <= pair[1].departure_min
                        }),
                )
            } else {
                None
            };
            JourneySummary {
                change_count: rides.saturating_sub(1),
                departure_min: first.departure_min,
                arrival_min: final_part.arrival_min,
                exceeds_recommended_changes: first.exceeds_recommended_changes,
                live_feasible,
                via_satisfied_by: parts
                    .iter()
                    .flat_map(|part| part.via_satisfied_by.iter().cloned())
                    .collect(),
            }
        })
        .collect()
}

/// The per-segment chained planner (the only planner before 2026-09-29):
/// each segment searched on its own, later ones chained from the previous
/// arrival (depart-after) or earlier ones from the next departure
/// (arrive-by). [`plan_trip`] uses it for trips without waypoints, and to
/// explain a joint plan that finds nothing.
///
/// Depart-after chains forwards as [`plan_via_waypoints`] documents.
/// Arrive-by chains backwards: the LAST segment is searched to arrive by
/// the caller's time, and each earlier one to arrive by
/// [`chain_deadline_min`] of the one after it; when a segment finds nothing,
/// every EARLIER one is validated but not searched. The avoid lists apply
/// to every segment.
fn plan_chained(input: &TripPlanInput<'_>) -> Result<Vec<SegmentResult>, String> {
    let interchange = input.search.interchange;
    let mut stops: Vec<&str> = vec![input.origin_crs];
    stops.extend(input.waypoints.iter().map(String::as_str));
    stops.push(input.destination_crs);
    let pairs: Vec<(&str, &str)> = stops.windows(2).map(|pair| (pair[0], pair[1])).collect();
    for &(from, to) in &pairs {
        resolve_segment_tiplocs(interchange, from, to)
            .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
    }

    let empty = |from: &str, to: &str| SegmentResult {
        origin_crs: from.to_string(),
        destination_crs: to.to_string(),
        itineraries: Vec::new(),
        capped_by_max_changes: false,
        depart_after_min: None,
        arrive_by_min: None,
        no_result_reason: None,
    };

    let mut segments: Vec<SegmentResult> = Vec::with_capacity(pairs.len());
    match input.time {
        TimeBound::DepartAfter(first) => {
            for &(from, to) in &pairs {
                let start = match segments.last() {
                    None => Some(first),
                    Some(previous) => chain_ready_min(&previous.itineraries, interchange),
                };
                let Some(start) = start else {
                    segments.push(empty(from, to));
                    continue;
                };
                let (itineraries, capped) = input
                    .search
                    .search(from, to, TimeBound::DepartAfter(start))
                    .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
                segments.push(SegmentResult {
                    itineraries,
                    capped_by_max_changes: capped,
                    depart_after_min: Some(start),
                    ..empty(from, to)
                });
            }
        }
        TimeBound::ArriveBy(last) => {
            for &(from, to) in pairs.iter().rev() {
                let deadline = match segments.last() {
                    None => Some(last),
                    Some(next) => chain_deadline_min(&next.itineraries, interchange),
                };
                let Some(deadline) = deadline else {
                    segments.push(empty(from, to));
                    continue;
                };
                let (itineraries, capped) = input
                    .search
                    .search(from, to, TimeBound::ArriveBy(deadline))
                    .map_err(|msg| format!("{from} -> {to}: {msg}"))?;
                segments.push(SegmentResult {
                    itineraries,
                    capped_by_max_changes: capped,
                    arrive_by_min: Some(deadline),
                    ..empty(from, to)
                });
            }
            segments.reverse();
        }
    }

    for index in 0..segments.len() {
        if !segments[index].itineraries.is_empty() {
            continue;
        }
        let reason = explain_empty_segment(input, &segments, index);
        segments[index].no_result_reason = Some(reason);
    }
    Ok(segments)
}

/// `HH:MM`, plus ` (+n day)` past midnight of the service date.
fn clock_label(minutes: u32) -> String {
    let (time, day_offset) = minutes_to_clock(minutes);
    let clock = time.format("%H:%M").to_string();
    match day_offset {
        0 => clock,
        1 => format!("{clock} (+1 day)"),
        days => format!("{clock} (+{days} days)"),
    }
}

/// Far enough past any service day's last call that "arrive by then" means
/// "arrive at all".
const END_OF_SERVICE_DAYS_MIN: u32 = 4 * 1440;

/// See [`NoResultReason`]. Adapted from `train-mcp`'s `attributeFailure`:
/// re-run the segment without the avoid lists, and blame them if that
/// finds something -- narrowed here to the single list whose removal is
/// enough, when there is one. Only then is the time (or the route itself)
/// blamed. Costs a few extra CSA searches, and only for an empty segment.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
fn explain_empty_segment(
    input: &TripPlanInput<'_>,
    segments: &[SegmentResult],
    index: usize,
) -> NoResultReason {
    let segment = &segments[index];
    let (from, to) = (
        segment.origin_crs.as_str(),
        segment.destination_crs.as_str(),
    );
    let search = &input.search;
    let bound = match (segment.depart_after_min, segment.arrive_by_min) {
        (Some(start), _) => TimeBound::DepartAfter(start),
        (None, Some(deadline)) => TimeBound::ArriveBy(deadline),
        (None, None) => {
            return match input.time {
                TimeBound::DepartAfter(_) => {
                    let previous = &segments[index - 1];
                    NoResultReason {
                        constraint: "previousSegment",
                        values: vec![format!(
                            "{} -> {}",
                            previous.origin_crs, previous.destination_crs
                        )],
                        message: format!(
                            "{from} -> {to} was not searched: the {} -> {} segment found no \
                             itinerary to continue from.",
                            previous.origin_crs, previous.destination_crs
                        ),
                    }
                }
                TimeBound::ArriveBy(_) => {
                    let next = &segments[index + 1];
                    NoResultReason {
                        constraint: "nextSegment",
                        values: vec![format!("{} -> {}", next.origin_crs, next.destination_crs)],
                        message: format!(
                            "{from} -> {to} was not searched: the {} -> {} segment found no \
                             itinerary to connect into.",
                            next.origin_crs, next.destination_crs
                        ),
                    }
                }
            };
        }
    };
    let phrase = match bound {
        TimeBound::DepartAfter(start) => format!("departing after {}", clock_label(start)),
        TimeBound::ArriveBy(deadline) => format!("arriving by {}", clock_label(deadline)),
    };
    let date = search.date;

    if segment.capped_by_max_changes {
        return NoResultReason {
            constraint: "maxChanges",
            values: vec![search.max_changes.to_string()],
            message: format!(
                "No itinerary from {from} to {to} {phrase} on {date} has {} or fewer changes; \
                 one with more exists. Raise maxChanges (at most {MAX_CHANGES_LIMIT}) or use \
                 results=fastest.",
                search.max_changes
            ),
        };
    }

    let Ok((from_tiplocs, to_tiplocs)) = resolve_segment_tiplocs(search.interchange, from, to)
    else {
        // Validated before any search; unreachable.
        return NoResultReason {
            constraint: "noRoute",
            values: Vec::new(),
            message: format!("No itinerary from {from} to {to}."),
        };
    };
    let found = |bound: TimeBound, restrictions: Option<&Restrictions>| {
        search.fastest(&from_tiplocs, &to_tiplocs, bound, restrictions)
    };

    let restricted = search.restrictions.is_some();
    if restricted && found(bound, None).is_some() {
        let active: Vec<(&'static str, &[String])> = input
            .avoid
            .lists()
            .into_iter()
            .filter(|(_, codes)| !codes.is_empty())
            .collect();
        let single = if active.len() == 1 {
            Some(active[0])
        } else {
            active.iter().copied().find(|(name, _)| {
                let variant = build_restrictions(
                    search.connections,
                    search.interchange,
                    input.passes,
                    &input.avoid.without(name),
                )
                .ok()
                .flatten();
                found(bound, variant.as_ref()).is_some()
            })
        };
        let (constraint, values, described) = match single {
            Some((name, codes)) => (name, codes.to_vec(), describe_list(name, codes)),
            None => (
                "avoidCombined",
                input.avoid.all_codes(),
                active
                    .iter()
                    .map(|(name, codes)| describe_list(name, codes))
                    .collect::<Vec<_>>()
                    .join(" and "),
            ),
        };
        return NoResultReason {
            constraint,
            values,
            message: format!(
                "No itinerary from {from} to {to} {phrase} on {date} while {described}; one \
                 exists without that restriction."
            ),
        };
    }

    let under = if restricted {
        let described: Vec<String> = input
            .avoid
            .lists()
            .into_iter()
            .filter(|(_, codes)| !codes.is_empty())
            .map(|(name, codes)| describe_list(name, codes))
            .collect();
        format!(" while {}", described.join(" and "))
    } else {
        String::new()
    };
    let restrictions = search.restrictions;
    match bound {
        TimeBound::ArriveBy(deadline) => {
            if let Some(earliest) = found(TimeBound::DepartAfter(0), restrictions) {
                return NoResultReason {
                    constraint: "arriveBy",
                    values: vec![clock_label(deadline)],
                    message: format!(
                        "No itinerary from {from} arrives at {to} by {} on {date}{under}; the \
                         earliest arrival is {}, leaving at {}.",
                        clock_label(deadline),
                        clock_label(earliest.arrival_min),
                        clock_label(earliest.departure_min)
                    ),
                };
            }
        }
        TimeBound::DepartAfter(start) => {
            let last = trip_planner::latest_departure(
                &search.arrive_by_options(&from_tiplocs, &to_tiplocs, END_OF_SERVICE_DAYS_MIN),
                search.overlay,
                restrictions,
            );
            if let Some(last) = last {
                return NoResultReason {
                    constraint: "departAfter",
                    values: vec![clock_label(start)],
                    message: format!(
                        "No itinerary from {from} to {to} departs after {} on {date}{under}; \
                         the last one that gets there leaves at {}.",
                        clock_label(start),
                        clock_label(last)
                    ),
                };
            }
        }
    }
    NoResultReason {
        constraint: "noRoute",
        values: Vec::new(),
        message: format!("No itinerary from {from} to {to} runs at all on {date}{under}."),
    }
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
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (crs, tiploc) in crs_to_tiplocs {
            data.tiploc_to_crs
                .insert((*tiploc).to_string(), (*crs).to_string());
            data.crs_to_tiplocs
                .entry((*crs).to_string())
                .or_default()
                .push((*tiploc).to_string());
        }
        for (tiploc, change_time) in change_times {
            data.change_time_by_tiploc
                .insert((*tiploc).to_string(), *change_time);
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
            working_departure_min: dep,
            working_arrival_min: arr,
            can_board: true,
            can_alight: true,
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

    /// A route needing exactly `changes` changes (one train per hop, every
    /// change free).
    fn chain_network(changes: u32) -> (Vec<schedule_query::Connection>, InterchangeData) {
        let stops: Vec<String> = (0..=changes + 1)
            .map(|i| match i {
                0 => "EUSTON".to_string(),
                i if i == changes + 1 => "MKC".to_string(),
                i => format!("H{i}"),
            })
            .collect();
        let connections = stops
            .windows(2)
            .zip(0u32..)
            .map(|(pair, hop)| {
                conn(
                    &format!("U{hop}"),
                    &pair[0],
                    &pair[1],
                    480 + 10 * hop,
                    490 + 10 * hop,
                )
            })
            .collect();
        let change_times: Vec<(&str, i32)> = stops.iter().map(|t| (t.as_str(), 0)).collect();
        let interchange =
            interchange_with_change_times(&[("EUS", "EUSTON"), ("MKC", "MKC")], &change_times);
        (connections, interchange)
    }

    /// The raised ceiling (2026-10-06): 5- and 6-change routes are found
    /// exactly when `maxChanges` allows them, and flagged as capped one below.
    #[test]
    fn options_mode_reaches_five_and_six_changes_under_the_raised_ceiling() {
        assert_eq!(MAX_CHANGES_LIMIT, 6);
        for changes in [5, 6] {
            let (connections, interchange) = chain_network(changes);
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
            let (found, capped) = plan(changes);
            assert_eq!(found.len(), 1, "{changes}: {found:?}");
            assert_eq!(found[0].change_count, changes);
            assert!(!capped);
            let (below, capped_below) = plan(changes - 1);
            assert!(below.is_empty(), "{changes}");
            assert!(capped_below, "{changes}: the headroom round sees it");
            // fastest finds it whatever the cap, flagged against it.
            let (fastest, _) = plan_segment(
                &connections,
                &interchange,
                date(),
                "EUS",
                "MKC",
                NaiveTime::MIN,
                "fastest",
                changes - 1,
            )
            .unwrap();
            assert_eq!(fastest[0].exceeds_recommended_changes, Some(true));
        }
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

    // -----------------------------------------------------------------
    // Arrive-by, avoid lists and noResultReason (2026-09-29).
    // -----------------------------------------------------------------

    fn sorted(mut connections: Vec<schedule_query::Connection>) -> Vec<schedule_query::Connection> {
        connections.sort_by(|a, b| {
            (a.departure_min, &a.uid, &a.from_tiploc).cmp(&(
                b.departure_min,
                &b.uid,
                &b.from_tiploc,
            ))
        });
        connections
    }

    fn plan(
        connections: &[schedule_query::Connection],
        interchange: &InterchangeData,
        passes: Option<&schedule_query::PassIndex>,
        waypoints: &[&str],
        time: TimeBound,
        results: &str,
        avoid: &AvoidLists,
    ) -> Vec<SegmentResult> {
        let restrictions = build_restrictions(connections, interchange, passes, avoid)
            .expect("the avoid lists are valid");
        let waypoints: Vec<String> = waypoints.iter().map(ToString::to_string).collect();
        plan_trip(&TripPlanInput {
            search: SegmentSearch {
                connections,
                interchange,
                date: date(),
                results,
                max_changes: DEFAULT_MAX_CHANGES,
                overlay: None,
                restrictions: restrictions.as_ref(),
            },
            passes,
            avoid,
            origin_crs: "EUS",
            waypoints: &waypoints,
            destination_crs: "MAN",
            vias: &[],
            via_search: None,
            time,
        })
        .expect("valid request")
    }

    fn stations() -> InterchangeData {
        interchange_with_change_times(
            &[
                ("EUS", "EUSTON"),
                ("MKC", "MILTNKC"),
                ("CRE", "CREWE"),
                ("STA", "STAFFRD"),
                ("MAN", "MANCPIC"),
                ("WFJ", "WATFDJ"),
                // A bus stop, under its planner `tiploc:` code.
                ("tiploc:MKCBUS", "MKCBUS"),
            ],
            &[("MILTNKC", 5), ("CREWE", 5), ("STAFFRD", 5)],
        )
    }

    #[test]
    fn arrive_by_takes_the_latest_departure_and_chains_waypoints_backwards() {
        let connections = sorted(vec![
            conn("A1", "EUSTON", "MILTNKC", 480, 530),
            conn("A2", "EUSTON", "MILTNKC", 500, 545),
            conn("A3", "EUSTON", "MILTNKC", 510, 548),
            conn("B1", "MILTNKC", "MANCPIC", 550, 620),
            conn("B2", "MILTNKC", "MANCPIC", 600, 700),
        ]);
        let segments = plan(
            &connections,
            &stations(),
            None,
            &["MKC"],
            TimeBound::ArriveBy(630),
            "fastest",
            &AvoidLists::default(),
        );
        // Last segment: B1 is the latest arriving by 10:30.
        assert_eq!(segments[1].arrive_by_min, Some(630));
        assert_eq!(train_uids(&segments[1].itineraries[0]), vec!["B1"]);
        // First segment: arrive by 09:10 - 5 = 09:05; A2 (arrives 09:05)
        // is the latest that makes it, A3 (09:08) is too late.
        assert_eq!(segments[0].arrive_by_min, Some(545));
        assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["A2"]);
        assert!(segments.iter().all(|s| s.depart_after_min.is_none()));
        assert!(segments.iter().all(|s| s.no_result_reason.is_none()));
    }

    #[test]
    fn arrive_by_options_offers_one_itinerary_per_change_count() {
        // Direct D1 leaves 07:00; changing at MKC (A1 + B1) leaves 07:30.
        let connections = sorted(vec![
            conn("D1", "EUSTON", "MANCPIC", 420, 600),
            conn("A1", "EUSTON", "MILTNKC", 450, 500),
            conn("B1", "MILTNKC", "MANCPIC", 510, 590),
        ]);
        let segments = plan(
            &connections,
            &stations(),
            None,
            &[],
            TimeBound::ArriveBy(600),
            "options",
            &AvoidLists::default(),
        );
        let shape: Vec<(u32, Vec<&str>)> = segments[0]
            .itineraries
            .iter()
            .map(|i| (i.change_count, train_uids(i)))
            .collect();
        assert_eq!(shape, vec![(0, vec!["D1"]), (1, vec!["A1", "B1"])]);
        assert!(!segments[0].capped_by_max_changes);
    }

    /// `T1` runs EUSTON -> CREWE -> MANCPIC; `S1` is a slower alternative
    /// changing at STAFFRD.
    fn crewe_network() -> Vec<schedule_query::Connection> {
        sorted(vec![
            conn("T1", "EUSTON", "CREWE", 480, 570),
            conn("T1", "CREWE", "MANCPIC", 572, 610),
            conn("S1", "EUSTON", "STAFFRD", 470, 560),
            conn("S2", "STAFFRD", "MANCPIC", 570, 650),
        ])
    }

    #[test]
    fn avoid_lists_reroute_and_explain_themselves() {
        let ic = stations();
        let connections = crewe_network();
        let lists = |avoid: &[&str], stop: &[&str], change: &[&str]| AvoidLists {
            avoid: avoid.iter().map(ToString::to_string).collect(),
            avoid_stop: stop.iter().map(ToString::to_string).collect(),
            avoid_change: change.iter().map(ToString::to_string).collect(),
        };
        let first_uids = |segments: &[SegmentResult]| -> Vec<String> {
            train_uids(&segments[0].itineraries[0])
                .into_iter()
                .map(str::to_string)
                .collect()
        };
        for time in [TimeBound::DepartAfter(0), TimeBound::ArriveBy(700)] {
            // Changing at CRE is not needed, so avoidChange keeps T1.
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "fastest",
                &lists(&[], &[], &["CRE"]),
            );
            assert_eq!(first_uids(&segments), vec!["T1"], "{time:?}");
            // T1 calls at CRE: avoidStop (and avoid) reroute via STA.
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "fastest",
                &lists(&[], &["CRE"], &[]),
            );
            assert_eq!(first_uids(&segments), vec!["S1", "S2"], "{time:?}");
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "options",
                &lists(&["CRE"], &[], &[]),
            );
            assert_eq!(first_uids(&segments), vec!["S1", "S2"], "{time:?}");

            // Avoiding both STA and CRE: nothing. Dropping either list
            // alone is enough, so the first (`avoid`) is named.
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "fastest",
                &lists(&["STA"], &["CRE"], &[]),
            );
            assert!(segments[0].itineraries.is_empty());
            let reason = segments[0].no_result_reason.as_ref().expect("explained");
            assert_eq!(reason.constraint, "avoid", "{time:?}");
            assert_eq!(reason.values, vec!["STA"]);

            // STA and CRE both in two lists: dropping either alone is not
            // enough.
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "fastest",
                &lists(&["STA", "CRE"], &["CRE", "STA"], &[]),
            );
            let reason = segments[0].no_result_reason.as_ref().expect("explained");
            assert_eq!(reason.constraint, "avoidCombined", "{time:?}");
            assert_eq!(reason.values, vec!["STA", "CRE"]);

            // Avoiding CRE and changing at STA: only avoidStop is to blame
            // when dropping it alone fixes the query.
            let segments = plan(
                &connections,
                &ic,
                None,
                &[],
                time,
                "fastest",
                &lists(&[], &["CRE"], &["STA"]),
            );
            let reason = segments[0].no_result_reason.as_ref().expect("explained");
            assert_eq!(reason.constraint, "avoidStop", "{time:?}");
            assert_eq!(reason.values, vec!["CRE"]);
            assert!(
                reason.message.contains("not calling at CRE"),
                "{}",
                reason.message
            );
        }
    }

    #[test]
    fn avoid_excludes_a_train_running_through_without_calling() {
        // F1 runs EUSTON -> (passes WATFDJ) -> MANCPIC; S1 is slower via
        // MILTNKC. The pass row only exists in the calling points, so this
        // builds the connections and the pass index from them.
        use schedule_query::CallingPointForConnections as Cp;
        let at = |t: &str| Some(t.parse::<NaiveTime>().unwrap());
        let cp = |tiploc: &str, arr: Option<NaiveTime>, dep: Option<NaiveTime>| Cp {
            tiploc: tiploc.to_string(),
            booked_arrival: arr,
            booked_departure: dep,
            day_offset: 0,
            can_board: true,
            can_alight: true,
            public_arrival: None,
            public_departure: None,
        };
        let f1 = vec![
            cp("EUSTON", None, at("08:00")),
            cp("WATFDJ", None, None),
            cp("MANCPIC", at("10:00"), None),
        ];
        let s1 = vec![
            cp("EUSTON", None, at("07:30")),
            cp("MILTNKC", at("08:10"), at("08:12")),
            cp("MANCPIC", at("10:30"), None),
        ];
        let (connections, passes) = schedule_query::build_connections_with_passes([
            ("F1", f1.as_slice()),
            ("S1", s1.as_slice()),
        ]);
        let ic = stations();
        let avoid = AvoidLists {
            avoid: vec!["WFJ".to_string()],
            ..AvoidLists::default()
        };
        let avoid_stop = AvoidLists {
            avoid_stop: vec!["WFJ".to_string()],
            ..AvoidLists::default()
        };
        for time in [TimeBound::DepartAfter(0), TimeBound::ArriveBy(700)] {
            let segments = plan(
                &connections,
                &ic,
                Some(&passes),
                &[],
                time,
                "fastest",
                &avoid,
            );
            assert_eq!(
                train_uids(&segments[0].itineraries[0]),
                vec!["S1"],
                "{time:?}"
            );
            // avoidStop only cares about calls: F1 never calls at WFJ.
            let segments = plan(
                &connections,
                &ic,
                Some(&passes),
                &[],
                time,
                "fastest",
                &avoid_stop,
            );
            assert_eq!(
                train_uids(&segments[0].itineraries[0]),
                vec!["F1"],
                "{time:?}"
            );
        }
    }

    #[test]
    fn an_unknown_avoided_code_is_an_error_naming_the_list() {
        let err = build_restrictions(
            &[],
            &stations(),
            None,
            &AvoidLists {
                avoid_change: vec!["ZZZ".to_string()],
                ..AvoidLists::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("avoidChange") && err.contains("ZZZ"), "{err}");
    }

    #[test]
    fn an_empty_segment_names_the_time_constraint_or_the_chain() {
        let ic = stations();
        let connections = crewe_network();
        let none = AvoidLists::default();

        // Arrive by 09:00: nothing gets there; the earliest arrival is 10:10.
        let segments = plan(
            &connections,
            &ic,
            None,
            &[],
            TimeBound::ArriveBy(540),
            "fastest",
            &none,
        );
        let reason = segments[0].no_result_reason.as_ref().expect("explained");
        assert_eq!(reason.constraint, "arriveBy");
        assert_eq!(reason.values, vec!["09:00"]);
        assert!(reason.message.contains("10:10"), "{}", reason.message);

        // Depart after 09:00: the last departure that gets there is 08:00.
        let segments = plan(
            &connections,
            &ic,
            None,
            &[],
            TimeBound::DepartAfter(540),
            "options",
            &none,
        );
        let reason = segments[0].no_result_reason.as_ref().expect("explained");
        assert_eq!(reason.constraint, "departAfter");
        assert!(reason.message.contains("08:00"), "{}", reason.message);

        // Via MKC: nothing reaches MKC at all, so EUS -> MKC is `noRoute`
        // and MKC -> MAN was never searched.
        let segments = plan(
            &connections,
            &ic,
            None,
            &["MKC"],
            TimeBound::DepartAfter(0),
            "fastest",
            &none,
        );
        assert_eq!(
            segments[0].no_result_reason.as_ref().unwrap().constraint,
            "noRoute"
        );
        let reason = segments[1].no_result_reason.as_ref().unwrap();
        assert_eq!(reason.constraint, "previousSegment");
        assert_eq!(reason.values, vec!["EUS -> MKC"]);
        // ...and in arrive-by, the chain breaks the other way.
        let segments = plan(
            &connections,
            &ic,
            None,
            &["MKC"],
            TimeBound::ArriveBy(700),
            "fastest",
            &none,
        );
        assert_eq!(
            segments[1].no_result_reason.as_ref().unwrap().constraint,
            "noRoute"
        );
        assert_eq!(
            segments[0].no_result_reason.as_ref().unwrap().constraint,
            "nextSegment"
        );
        assert_eq!(segments[0].arrive_by_min, None);
    }

    #[test]
    fn an_options_segment_empty_only_because_of_the_cap_blames_max_changes() {
        let (connections, interchange) = three_change_only_network();
        let segments = plan_trip(&TripPlanInput {
            search: SegmentSearch {
                connections: &connections,
                interchange: &interchange,
                date: date(),
                results: "options",
                max_changes: 2,
                overlay: None,
                restrictions: None,
            },
            passes: None,
            avoid: &AvoidLists::default(),
            origin_crs: "EUS",
            waypoints: &[],
            destination_crs: "MKC",
            vias: &[],
            via_search: None,
            time: TimeBound::DepartAfter(0),
        })
        .unwrap();
        let reason = segments[0].no_result_reason.as_ref().expect("explained");
        assert_eq!(reason.constraint, "maxChanges");
        assert_eq!(reason.values, vec!["2"]);
    }

    // -----------------------------------------------------------------
    // Joint waypoint planning (2026-09-29): no phantom change on a through
    // train, and maxChanges over the whole journey.
    // -----------------------------------------------------------------

    fn plan_with(
        connections: &[schedule_query::Connection],
        waypoints: &[&str],
        time: TimeBound,
        results: &str,
        max_changes: u32,
    ) -> Result<Vec<SegmentResult>, String> {
        let interchange = stations();
        let waypoints: Vec<String> = waypoints.iter().map(ToString::to_string).collect();
        plan_trip(&TripPlanInput {
            search: SegmentSearch {
                connections,
                interchange: &interchange,
                date: date(),
                results,
                max_changes,
                overlay: None,
                restrictions: None,
            },
            passes: None,
            avoid: &AvoidLists::default(),
            origin_crs: "EUS",
            waypoints: &waypoints,
            destination_crs: "MAN",
            vias: &[],
            via_search: None,
            time,
        })
    }

    #[test]
    fn a_through_train_at_a_waypoint_is_not_a_change() {
        // T1 calls at MKC for 2 minutes (a change there needs 5); T2 is the
        // next train on from MKC. Chaining used to charge 5 minutes at MKC,
        // missing T1's onward half and offering T2.
        let connections = sorted(vec![
            conn("T1", "EUSTON", "MILTNKC", 480, 530),
            conn("T1", "MILTNKC", "MANCPIC", 532, 600),
            conn("T2", "MILTNKC", "MANCPIC", 545, 640),
        ]);
        for time in [TimeBound::DepartAfter(0), TimeBound::ArriveBy(610)] {
            for results in ["fastest", "options"] {
                let segments =
                    plan_with(&connections, &["MKC"], time, results, 0).expect("valid request");
                assert_eq!(segments[0].itineraries.len(), 1, "{time:?} {results}");
                assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["T1"]);
                assert_eq!(train_uids(&segments[1].itineraries[0]), vec!["T1"]);
                assert!(!segments[0].itineraries[0].continues_previous_train);
                assert!(segments[1].itineraries[0].continues_previous_train);
                let journeys = journey_summaries(&segments, &stations());
                assert_eq!(journeys.len(), 1);
                assert_eq!(journeys[0].change_count, 0, "no change at MKC");
                assert_eq!(
                    (journeys[0].departure_min, journeys[0].arrival_min),
                    (480, 600)
                );
                if results == "fastest" {
                    assert_eq!(
                        segments[1].itineraries[0].exceeds_recommended_changes,
                        Some(false)
                    );
                }
            }
        }
        // The old chained planner, for contrast: a phantom change onto T2.
        let chained = plan_via_waypoints(
            &connections,
            &stations(),
            date(),
            "EUS",
            &["MKC".to_string()],
            "MAN",
            NaiveTime::MIN,
            "fastest",
            0,
        )
        .unwrap();
        assert_eq!(train_uids(&chained[1].itineraries[0]), vec!["T2"]);
    }

    /// EUS -> MKC -> CRE -> MAN: the fast way is U1, U2, U3 (two changes, at
    /// the waypoints); D1 calls at both and needs none, but is slower.
    fn two_waypoint_network() -> Vec<schedule_query::Connection> {
        sorted(vec![
            conn("U1", "EUSTON", "MILTNKC", 480, 520),
            conn("U2", "MILTNKC", "CREWE", 530, 580),
            conn("U3", "CREWE", "MANCPIC", 590, 620),
            conn("D1", "EUSTON", "MILTNKC", 470, 525),
            conn("D1", "MILTNKC", "CREWE", 527, 600),
            conn("D1", "CREWE", "MANCPIC", 602, 650),
        ])
    }

    #[test]
    fn max_changes_caps_the_whole_journey_across_waypoints() {
        let connections = two_waypoint_network();
        let shape = |segments: &[SegmentResult]| -> Vec<Vec<Vec<String>>> {
            segments
                .iter()
                .map(|segment| {
                    segment
                        .itineraries
                        .iter()
                        .map(|i| train_uids(i).into_iter().map(str::to_string).collect())
                        .collect()
                })
                .collect()
        };
        let via = ["MKC", "CRE"];
        // Each segment alone is direct (0 changes), but the whole U journey
        // has 2: maxChanges=1 leaves only D1, and flags the cap.
        let segments =
            plan_with(&connections, &via, TimeBound::DepartAfter(0), "options", 1).unwrap();
        assert_eq!(
            shape(&segments),
            vec![vec![vec!["D1"]], vec![vec!["D1"]], vec![vec!["D1"]]]
        );
        assert!(segments.iter().all(|s| s.capped_by_max_changes));
        // maxChanges=2: both, aligned across the segments, fewest changes
        // first.
        let segments =
            plan_with(&connections, &via, TimeBound::DepartAfter(0), "options", 2).unwrap();
        assert_eq!(
            shape(&segments),
            vec![
                vec![vec!["D1"], vec!["U1"]],
                vec![vec!["D1"], vec!["U2"]],
                vec![vec!["D1"], vec!["U3"]],
            ]
        );
        let journeys = journey_summaries(&segments, &stations());
        assert_eq!(
            journeys.iter().map(|j| j.change_count).collect::<Vec<_>>(),
            vec![0, 2]
        );
        // fastest ignores the cap but flags every part against the WHOLE
        // journey's changes.
        let segments =
            plan_with(&connections, &via, TimeBound::DepartAfter(0), "fastest", 1).unwrap();
        assert_eq!(shape(&segments)[2], vec![vec!["U3"]]);
        assert!(
            segments
                .iter()
                .all(|s| s.itineraries[0].exceeds_recommended_changes == Some(true))
        );
        // Nothing within maxChanges=1 once D1 is gone: `maxChanges` is named.
        let without_d1: Vec<_> = connections
            .iter()
            .filter(|c| c.uid != "D1")
            .cloned()
            .collect();
        let segments =
            plan_with(&without_d1, &via, TimeBound::DepartAfter(0), "options", 1).unwrap();
        assert!(segments.iter().all(|s| s.itineraries.is_empty()));
        let reason = segments[1].no_result_reason.as_ref().unwrap();
        assert_eq!(reason.constraint, "maxChanges");
        // Arrive-by, the same cap.
        let segments =
            plan_with(&connections, &via, TimeBound::ArriveBy(700), "options", 1).unwrap();
        // Leaving later is the arrive-by gain: U1 then D1 from MKC (one
        // change), next to D1 throughout.
        assert_eq!(
            shape(&segments),
            vec![
                vec![vec!["D1"], vec!["U1"]],
                vec![vec!["D1"], vec!["D1"]],
                vec![vec!["D1"], vec!["D1"]],
            ]
        );
        assert!(!segments[1].itineraries[1].continues_previous_train);
        assert!(segments[2].itineraries[1].continues_previous_train);
        let journeys = journey_summaries(&segments, &stations());
        assert_eq!(
            journeys
                .iter()
                .map(|j| (j.change_count, j.departure_min))
                .collect::<Vec<_>>(),
            vec![(0, 470), (1, 480)]
        );
    }

    #[test]
    fn a_waypoint_repeating_the_origin_destination_or_itself_is_an_error() {
        let connections = two_waypoint_network();
        for (waypoints, needle) in [
            (vec!["EUS"], "the origin"),
            (vec!["MAN"], "the destination"),
            (vec!["MKC", "MKC"], "the waypoint before it"),
        ] {
            let err = plan_with(
                &connections,
                &waypoints,
                TimeBound::DepartAfter(0),
                "fastest",
                2,
            )
            .unwrap_err();
            assert!(err.contains(needle), "{err}");
        }
    }

    // -----------------------------------------------------------------
    // Pass-through vias (2026-10-06).
    // -----------------------------------------------------------------

    /// Connections and pass index from calling points `(tiploc, arrival,
    /// departure)`; a row with neither time is an untimed passing point
    /// (CIF records one only at a timing point).
    fn day(
        trains: &[(&str, Calls<'_>)],
    ) -> (Vec<schedule_query::Connection>, schedule_query::PassIndex) {
        use schedule_query::CallingPointForConnections as Cp;
        let at = |t: Option<&str>| t.map(|t| t.parse::<NaiveTime>().unwrap());
        let schedules: Vec<(&str, Vec<Cp>)> = trains
            .iter()
            .map(|(uid, calls)| {
                let calls = calls
                    .iter()
                    .map(|(tiploc, arrival, departure)| Cp {
                        tiploc: (*tiploc).to_string(),
                        booked_arrival: at(*arrival),
                        booked_departure: at(*departure),
                        day_offset: 0,
                        can_board: true,
                        can_alight: true,
                        public_arrival: None,
                        public_departure: None,
                    })
                    .collect();
                (*uid, calls)
            })
            .collect();
        schedule_query::build_connections_with_passes(
            schedules
                .iter()
                .map(|(uid, calls)| (*uid, calls.as_slice())),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "test helper mirroring TripPlanInput's independent inputs"
    )]
    fn plan_vias(
        connections: &[schedule_query::Connection],
        passes: &schedule_query::PassIndex,
        waypoints: &[&str],
        vias: &[&str],
        time: TimeBound,
        results: &str,
        max_changes: u32,
        avoid: &AvoidLists,
    ) -> Result<Vec<SegmentResult>, String> {
        let interchange = stations();
        let restrictions = build_restrictions(connections, &interchange, Some(passes), avoid)?;
        let vias: Vec<String> = vias.iter().map(ToString::to_string).collect();
        // `A|B` is one OR via, as `routes::trips` parses it.
        let codes: Vec<Vec<String>> = vias
            .iter()
            .map(|via| via.split('|').map(ToString::to_string).collect())
            .collect();
        let via_search = build_vias(connections, &interchange, Some(passes), &codes)?;
        let waypoints: Vec<String> = waypoints.iter().map(ToString::to_string).collect();
        plan_trip(&TripPlanInput {
            search: SegmentSearch {
                connections,
                interchange: &interchange,
                date: date(),
                results,
                max_changes,
                overlay: None,
                restrictions: restrictions.as_ref(),
            },
            passes: Some(passes),
            avoid,
            origin_crs: "EUS",
            waypoints: &waypoints,
            destination_crs: "MAN",
            vias: &vias,
            via_search: via_search.as_ref(),
            time,
        })
    }

    fn via_hits(segments: &[SegmentResult]) -> Vec<Vec<(String, usize, usize, &'static str)>> {
        journey_summaries(segments, &stations())
            .iter()
            .map(|journey| {
                journey
                    .via_satisfied_by
                    .iter()
                    .map(|v| (v.crs.clone(), v.segment, v.leg, v.how))
                    .collect()
            })
            .collect()
    }

    /// F1 runs EUSTON -> (passes WATFDJ, a timing point) -> MANCPIC; S1 calls
    /// at MILTNKC; N1 is the fastest and goes nowhere near either.
    type Calls<'a> = &'a [(&'a str, Option<&'a str>, Option<&'a str>)];
    const F1: Calls<'static> = &[
        ("EUSTON", None, Some("08:00")),
        ("WATFDJ", None, None),
        ("MANCPIC", Some("10:00"), None),
    ];
    const S1: Calls<'static> = &[
        ("EUSTON", None, Some("07:30")),
        ("MILTNKC", Some("08:10"), Some("08:12")),
        ("MANCPIC", Some("10:30"), None),
    ];
    const N1: Calls<'static> = &[
        ("EUSTON", None, Some("08:05")),
        ("CREWE", Some("09:20"), Some("09:22")),
        ("MANCPIC", Some("09:50"), None),
    ];

    #[test]
    fn a_via_is_passed_without_calling_and_each_journey_names_the_leg() {
        let (connections, passes) = day(&[("F1", F1), ("S1", S1), ("N1", N1)]);
        let none = AvoidLists::default();
        for time in [TimeBound::DepartAfter(0), TimeBound::ArriveBy(700)] {
            for results in ["fastest", "options"] {
                let segments = plan_vias(
                    &connections,
                    &passes,
                    &[],
                    &["WFJ"],
                    time,
                    results,
                    DEFAULT_MAX_CHANGES,
                    &none,
                )
                .unwrap();
                assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["F1"]);
                assert_eq!(
                    via_hits(&segments)[0],
                    vec![("WFJ".to_string(), 0, 0, "pass")],
                    "{time:?} {results}"
                );
                let segments = plan_vias(
                    &connections,
                    &passes,
                    &[],
                    &["MKC"],
                    time,
                    results,
                    DEFAULT_MAX_CHANGES,
                    &none,
                )
                .unwrap();
                assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["S1"]);
                assert_eq!(
                    via_hits(&segments)[0],
                    vec![("MKC".to_string(), 0, 0, "call")]
                );
            }
        }
        // Without a via: the fastest, and no viaSatisfiedBy entries.
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &[],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["N1"]);
        assert_eq!(via_hits(&segments), vec![Vec::new()]);
    }

    /// OR choices (2026-10-07): `A|B` is one via passed by either, and
    /// `matchedCrs` names the member the journey used.
    #[test]
    fn an_or_via_is_passed_by_any_member_and_names_it() {
        let (connections, passes) = day(&[("F1", F1), ("S1", S1), ("N1", N1)]);
        let none = AvoidLists::default();
        let matched = |segments: &[SegmentResult]| -> Vec<(String, Option<String>, &str)> {
            journey_summaries(segments, &stations())[0]
                .via_satisfied_by
                .iter()
                .map(|v| (v.crs.clone(), v.matched_crs.clone(), v.how))
                .collect()
        };
        for time in [TimeBound::DepartAfter(0), TimeBound::ArriveBy(700)] {
            // WFJ (F1 runs through it, 10:00) or MKC (S1 calls, 10:30).
            let segments = plan_vias(
                &connections,
                &passes,
                &[],
                &["WFJ|MKC"],
                time,
                "fastest",
                DEFAULT_MAX_CHANGES,
                &none,
            )
            .unwrap();
            assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["F1"]);
            assert_eq!(
                matched(&segments),
                vec![("WFJ|MKC".to_string(), Some("WFJ".to_string()), "pass")],
                "{time:?}"
            );
            // MKC or CRE: N1 calls at Crewe and is the fastest of all.
            let segments = plan_vias(
                &connections,
                &passes,
                &[],
                &["MKC|CRE"],
                time,
                "fastest",
                DEFAULT_MAX_CHANGES,
                &none,
            )
            .unwrap();
            assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["N1"]);
            assert_eq!(
                matched(&segments),
                vec![("MKC|CRE".to_string(), Some("CRE".to_string()), "call")],
                "{time:?}"
            );
        }
        // Avoiding Crewe leaves Milton Keynes as the member to use.
        let no_crewe = AvoidLists {
            avoid: vec!["CRE".to_string()],
            ..AvoidLists::default()
        };
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["MKC|CRE"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &no_crewe,
        )
        .unwrap();
        assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["S1"]);
        assert_eq!(
            matched(&segments),
            vec![("MKC|CRE".to_string(), Some("MKC".to_string()), "call")]
        );
        // No member reachable: no journey, explained as the via.
        let (connections, passes) = day(&[("N1", N1)]);
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["STA|MKC|WFJ"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        assert!(segments[0].itineraries.is_empty());
        let reason = segments[0].no_result_reason.as_ref().unwrap();
        assert_eq!(
            (reason.constraint, reason.values.clone()),
            ("via", vec!["STA|MKC|WFJ".to_string()])
        );
        assert!(
            reason
                .message
                .contains("passes through any of STA, MKC, WFJ"),
            "{reason:?}"
        );
    }

    /// CIF records a passing point only at a timing point. A train running
    /// through a station that is not one leaves no row there, so it cannot
    /// satisfy a via at that station -- only a call can (documented in
    /// `build_vias`).
    #[test]
    fn a_via_at_a_non_timing_point_is_seen_only_when_a_train_calls_there() {
        // N1 physically runs through Stafford but its schedule has no row
        // there (not a timing point on this route).
        let (connections, passes) = day(&[("N1", N1)]);
        let none = AvoidLists::default();
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["STA"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        assert!(segments[0].itineraries.is_empty());
        let reason = segments[0].no_result_reason.as_ref().unwrap();
        assert_eq!(
            (reason.constraint, reason.values.clone()),
            ("via", vec!["STA".to_string()])
        );
        assert!(reason.message.contains("passes through STA"), "{reason:?}");
        // The same train with Stafford as a timing point (an untimed row) or
        // a call satisfies it.
        for (arrival, departure, how) in
            [(None, None, "pass"), (Some("09:00"), Some("09:01"), "call")]
        {
            let calls = [N1[0], ("STAFFRD", arrival, departure), N1[1], N1[2]];
            let (connections, passes) = day(&[("N1", &calls[..])]);
            let segments = plan_vias(
                &connections,
                &passes,
                &[],
                &["STA"],
                TimeBound::DepartAfter(0),
                "fastest",
                DEFAULT_MAX_CHANGES,
                &none,
            )
            .unwrap();
            assert_eq!(via_hits(&segments)[0], vec![("STA".to_string(), 0, 0, how)]);
        }
    }

    #[test]
    fn vias_and_waypoints_interleave_and_combine_with_avoid_stop() {
        // W1 passes WATFDJ, then calls at MILTNKC and CREWE.
        let w1: Calls<'_> = &[
            ("EUSTON", None, Some("08:00")),
            ("WATFDJ", None, None),
            ("MILTNKC", Some("08:40"), Some("08:42")),
            ("CREWE", Some("09:40"), Some("09:42")),
            ("MANCPIC", Some("10:10"), None),
        ];
        let (connections, passes) = day(&[("W1", w1), ("S1", S1), ("N1", N1)]);
        let none = AvoidLists::default();
        let segments = plan_vias(
            &connections,
            &passes,
            &["MKC"],
            &["WFJ", "CRE"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        assert_eq!(
            via_hits(&segments)[0],
            vec![
                ("WFJ".to_string(), 0, 0, "pass"),
                ("CRE".to_string(), 1, 0, "call")
            ]
        );
        assert!(segments[1].itineraries[0].continues_previous_train);
        // Pass CREWE without stopping: nothing does.
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["CRE"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &AvoidLists {
                avoid_stop: vec!["CRE".to_string()],
                ..AvoidLists::default()
            },
        )
        .unwrap();
        assert_eq!(
            segments[0].no_result_reason.as_ref().map(|r| r.constraint),
            Some("via")
        );
        // Pass WATFDJ without stopping: W1 does.
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["WFJ"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &AvoidLists {
                avoid_stop: vec!["WFJ".to_string()],
                ..AvoidLists::default()
            },
        )
        .unwrap();
        assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["W1"]);
    }

    #[test]
    fn an_unpassable_via_is_named_and_other_constraints_still_come_first() {
        let (connections, passes) = day(&[("F1", F1), ("S1", S1)]);
        let none = AvoidLists::default();
        // No train both calls at MKC and passes WFJ: either alone works, so
        // the first whose removal is enough is named.
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["MKC", "WFJ"],
            TimeBound::DepartAfter(0),
            "options",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        let reason = segments[0].no_result_reason.clone().unwrap();
        assert_eq!(reason.constraint, "via");
        assert_eq!(reason.values, vec!["MKC"]);
        // Too late for anything: the time is to blame, not the via.
        let segments = plan_vias(
            &connections,
            &passes,
            &[],
            &["WFJ"],
            TimeBound::DepartAfter(23 * 60),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap();
        assert_eq!(
            segments[0].no_result_reason.as_ref().map(|r| r.constraint),
            Some("departAfter")
        );
    }

    /// A via may be a bus stop's `tiploc:` code (2026-10-07): only a call
    /// there satisfies it (nothing runs through a bus stop), and
    /// `viaSatisfiedBy` names it by that code.
    #[test]
    fn a_bus_stop_tiploc_code_is_a_via_satisfied_by_a_call() {
        const B1: Calls<'static> = &[
            ("EUSTON", None, Some("07:40")),
            ("MKCBUS", Some("08:40"), Some("08:41")),
            ("MANCPIC", Some("11:30"), None),
        ];
        let (connections, passes) = day(&[("F1", F1), ("N1", N1), ("B1", B1)]);
        let none = AvoidLists::default();
        for results in ["fastest", "options"] {
            let segments = plan_vias(
                &connections,
                &passes,
                &[],
                &["tiploc:MKCBUS"],
                TimeBound::DepartAfter(0),
                results,
                DEFAULT_MAX_CHANGES,
                &none,
            )
            .unwrap();
            assert_eq!(train_uids(&segments[0].itineraries[0]), vec!["B1"]);
            assert_eq!(
                via_hits(&segments)[0],
                vec![("tiploc:MKCBUS".to_string(), 0, 0, "call")],
                "{results}"
            );
        }
        let err = plan_vias(
            &connections,
            &passes,
            &[],
            &["tiploc:NOWHERE"],
            TimeBound::DepartAfter(0),
            "fastest",
            DEFAULT_MAX_CHANGES,
            &none,
        )
        .unwrap_err();
        assert!(err.contains("not a recognised"), "{err}");
    }

    #[test]
    fn a_via_equal_to_the_origin_or_destination_or_unknown_is_an_error() {
        let (connections, passes) = day(&[("F1", F1)]);
        let none = AvoidLists::default();
        for (via, needle) in [
            ("EUS", "the origin"),
            ("MAN", "the destination"),
            ("ZZZ", "not a recognised station CRS code"),
            ("LON", "not a recognised station CRS code"),
        ] {
            let err = plan_vias(
                &connections,
                &passes,
                &[],
                &[via],
                TimeBound::DepartAfter(0),
                "fastest",
                DEFAULT_MAX_CHANGES,
                &none,
            )
            .unwrap_err();
            assert!(err.contains(needle) && err.contains("via"), "{err}");
        }
    }
}
