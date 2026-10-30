//! The `/trains` boards' advanced stop filters (2026-10-30): `calls_at`,
//! `via`, `avoid` and `avoid_stop` on `GET /public/trains/search` (the
//! departures board and the between-stations view) and
//! `GET /public/trains/arrivals`, the trip planner's `waypoints`, `via`,
//! `avoid` and `avoidStop` applied to one train.
//!
//! A filter looks at the part of the train's run a passenger on that board
//! rides: after the searched station (departures), before it (arrivals),
//! or strictly between the two stations (between). Read from
//! `schedule_calling_points_full`, which, unlike
//! `schedule_destination_departures`, keeps every location of the schedule
//! in order: calls a passenger may use, working-only calls, and the untimed
//! passing points CIF records at timing points.
//!
//! What each filter means, by the planner's own definitions
//! (`trip_planning_itinerary::build_restrictions`, `build_vias`):
//!
//! - `calls_at` (the planner's `waypoints`, "Call at"): the train makes a
//!   passenger call (a public arrival or departure time) at each entry, in
//!   order. An entry may be an OR choice (`KGX|EUS`, `group:LON`).
//! - `via` ("Pass through"): the train calls at or runs through each entry,
//!   in order -- any row of the schedule there, timed or not. Running
//!   through is only visible where CIF records a passing point.
//! - `avoid` ("Avoid completely"): no row of the schedule there at all.
//! - `avoid_stop` ("Don't stop at"): no timed call there (a passenger call
//!   or a working-only one); running through is fine.
//!
//! Every CRS covers all of its TIPLOCs (`tiploc_crs` and `stanox_crs`); a
//! `tiploc:` code names one location of `tiploc_locations`.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use sqlx::PgPool;

/// The filters, as TIPLOC sets (normalized).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopFilters {
    /// In order; each entry is satisfied by a passenger call at ANY of its
    /// TIPLOCs.
    pub calls_at: Vec<HashSet<String>>,
    /// In order; each entry is satisfied by a call at, or a pass of, ANY of
    /// its TIPLOCs.
    pub via: Vec<HashSet<String>>,
    pub avoid: HashSet<String>,
    pub avoid_stop: HashSet<String>,
}

impl StopFilters {
    pub fn is_empty(&self) -> bool {
        self.calls_at.is_empty()
            && self.via.is_empty()
            && self.avoid.is_empty()
            && self.avoid_stop.is_empty()
    }

    /// Whether the part of a train's run in `span` (the rows a passenger
    /// rides past, the boarding and alighting calls themselves excluded)
    /// passes every filter.
    pub fn accepts(&self, span: &[ScheduleCall]) -> bool {
        let tiploc =
            |call: &ScheduleCall| schedule_query::normalize_tiploc(&call.tiploc).to_string();
        if span.iter().any(|call| {
            let at = tiploc(call);
            self.avoid.contains(&at) || (call.is_timed() && self.avoid_stop.contains(&at))
        }) {
            return false;
        }
        in_order(
            &self.calls_at,
            span.iter().filter(|call| call.is_public_call()).map(tiploc),
        ) && in_order(&self.via, span.iter().map(tiploc))
    }
}

/// Whether `locations` meets each of `wanted` in turn (each entry by any of
/// its TIPLOCs, later than the one before).
fn in_order(wanted: &[HashSet<String>], locations: impl Iterator<Item = String>) -> bool {
    let mut next = wanted.iter().peekable();
    for location in locations {
        let Some(entry) = next.peek() else {
            return true;
        };
        if entry.contains(&location) {
            next.next();
        }
    }
    next.peek().is_none()
}

/// One `schedule_calling_points_full` row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ScheduleCall {
    pub uid: String,
    pub seq: i16,
    pub tiploc: String,
    pub booked_arrival: Option<chrono::NaiveTime>,
    pub booked_departure: Option<chrono::NaiveTime>,
    /// The arrival's day (the departure's may be a day later: see
    /// [`schedule_query::records::departure_day_offset`]).
    pub day_offset: i16,
    pub public_arrival: Option<chrono::NaiveTime>,
    pub public_departure: Option<chrono::NaiveTime>,
    pub can_board: Option<bool>,
    pub can_alight: Option<bool>,
}

impl ScheduleCall {
    /// The train calls here (a booked working arrival or departure),
    /// whether passengers may use it or not. A row with neither is a
    /// passing point.
    pub fn is_timed(&self) -> bool {
        self.booked_arrival.is_some() || self.booked_departure.is_some()
    }

    /// A call in the public timetable: a public arrival or departure time.
    /// A row published before public times were (every public column and
    /// direction flag NULL) counts when it is timed.
    pub fn is_public_call(&self) -> bool {
        if self.public_arrival.is_some() || self.public_departure.is_some() {
            return true;
        }
        self.is_timed() && self.can_board.is_none() && self.can_alight.is_none()
    }

    /// Whether this row is the departure a `schedule_destination_departures`
    /// row describes: the same working departure on the same day.
    fn departs_at(&self, scheduled: chrono::NaiveTime, day_offset: i16) -> bool {
        let Some(departure) = self.booked_departure else {
            return false;
        };
        let arrival_day = u8::try_from(self.day_offset).unwrap_or(0);
        let departure_day = schedule_query::records::departure_day_offset(
            self.booked_arrival,
            Some(departure),
            arrival_day,
        );
        departure == scheduled && i16::from(departure_day) == day_offset
    }
}

/// The index in `calls` (one train, in order) of its departure at
/// `scheduled` on day `day_offset` from one of `tiplocs`, searching after
/// index `after` when given. Falls back to the departure at that time from
/// anywhere, for a station whose TIPLOC the CRS lookup and the publish
/// disagree about; `None` when the train has no such departure.
pub(crate) fn find_departure(
    calls: &[ScheduleCall],
    tiplocs: &HashSet<String>,
    scheduled: chrono::NaiveTime,
    day_offset: i16,
    after: Option<usize>,
) -> Option<usize> {
    let start = after.map_or(0, |index| index + 1);
    let candidates = || {
        calls
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, call)| call.departs_at(scheduled, day_offset))
    };
    candidates()
        .find(|(_, call)| tiplocs.contains(schedule_query::normalize_tiploc(&call.tiploc)))
        .or_else(|| candidates().next())
        .map(|(index, _)| index)
}

/// The filters of one calling-point search
/// (`queries::search_schedule_calling_point_departures`), with the
/// stations a row is placed on its train by.
#[derive(Debug, Clone)]
pub struct SearchStopFilter {
    pub filters: StopFilters,
    /// The searched station's TIPLOCs (where the passenger boards).
    pub station_tiplocs: HashSet<String>,
    /// Between two stations, the second's (where they alight); `None` on
    /// the departures board, which rides on to the end of the train.
    pub stops_at_tiplocs: Option<HashSet<String>>,
}

impl SearchStopFilter {
    /// Whether the train boarded at its departure at `scheduled` on day
    /// `day_offset` passes, riding on to the end, or with `stops_at` to
    /// the call the search matched: its departure `(time, day offset)`, or
    /// `None` for the terminus.
    pub fn accepts(
        &self,
        calls: &[ScheduleCall],
        scheduled: chrono::NaiveTime,
        day_offset: i16,
        stop: Option<(chrono::NaiveTime, i16)>,
    ) -> bool {
        let end = match (&self.stops_at_tiplocs, stop) {
            (None, _) => RideEnd::Onward,
            (Some(tiplocs), Some((scheduled, day_offset))) => RideEnd::Call {
                tiplocs,
                scheduled,
                day_offset,
            },
            (Some(_), None) => RideEnd::Terminus,
        };
        departure_span(calls, &self.station_tiplocs, scheduled, day_offset, end)
            .is_some_and(|span| self.filters.accepts(span))
    }
}

/// Where a ride ends, for [`departure_span`].
#[derive(Debug, Clone, Copy)]
pub enum RideEnd<'a> {
    /// Rides on to the end of the train (the departures board).
    Onward,
    /// Alights at the train's departure at `scheduled` on day `day_offset`
    /// from one of `tiplocs` (an intermediate call), after boarding.
    Call {
        tiplocs: &'a HashSet<String>,
        scheduled: chrono::NaiveTime,
        day_offset: i16,
    },
    /// Alights where the train terminates.
    Terminus,
}

/// The calls a passenger boarding `calls` at its departure at `scheduled`
/// on day `day_offset` from one of `tiplocs` rides past before `end`,
/// both ends excluded; `None` when either end cannot be placed on the
/// train (then no filter can be checked, and the train is left out).
pub(crate) fn departure_span<'c>(
    calls: &'c [ScheduleCall],
    tiplocs: &HashSet<String>,
    scheduled: chrono::NaiveTime,
    day_offset: i16,
    end: RideEnd<'_>,
) -> Option<&'c [ScheduleCall]> {
    let from = find_departure(calls, tiplocs, scheduled, day_offset, None)?;
    let to = match end {
        RideEnd::Onward => calls.len(),
        RideEnd::Call {
            tiplocs,
            scheduled,
            day_offset,
        } => find_departure(calls, tiplocs, scheduled, day_offset, Some(from))?,
        RideEnd::Terminus => calls.len().checked_sub(1).filter(|last| *last > from)?,
    };
    calls.get(from + 1..to)
}

/// The calls a train in `calls` runs past before reaching where a
/// passenger alights: its call that departs again at `departure` (`(time,
/// day offset)`) from one of `tiplocs`, or with `None` its terminus. The
/// arrivals board's span, the alighting call excluded.
pub(crate) fn arrival_span<'c>(
    calls: &'c [ScheduleCall],
    tiplocs: &HashSet<String>,
    departure: Option<(chrono::NaiveTime, i16)>,
) -> Option<&'c [ScheduleCall]> {
    let at = match departure {
        Some((scheduled, day_offset)) => {
            find_departure(calls, tiplocs, scheduled, day_offset, None)?
        }
        None => calls.len().checked_sub(1)?,
    };
    calls.get(..at)
}

/// Every `schedule_calling_points_full` row of `uids` on `service_date`,
/// by uid, in order. One read over the primary key's `(service_date, uid)`
/// prefix.
pub async fn fetch_calls(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    uids: &[String],
) -> Result<HashMap<String, Vec<ScheduleCall>>> {
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<ScheduleCall> = sqlx::query_as(
        "SELECT uid, seq, tiploc, booked_arrival, booked_departure, day_offset, \
                public_arrival, public_departure, can_board, can_alight \
         FROM schedule_calling_points_full \
         WHERE service_date = $1 AND uid = ANY($2) \
         ORDER BY uid, seq",
    )
    .bind(service_date)
    .bind(uids)
    .fetch_all(pool)
    .await?;
    let mut by_uid: HashMap<String, Vec<ScheduleCall>> = HashMap::new();
    for row in rows {
        by_uid.entry(row.uid.clone()).or_default().push(row);
    }
    Ok(by_uid)
}

/// The TIPLOCs of each of `codes` (CRS codes, upper case, or `tiploc:`
/// codes as `common::location_naming::normalize_location_code` writes
/// them): a CRS's from `tiploc_crs` and `stanox_crs`, a `tiploc:` code's
/// own TIPLOC when `tiploc_locations` knows it. A code with none is absent
/// from the map. Two small indexed reads.
pub async fn tiplocs_for_codes(
    pool: &PgPool,
    codes: &[String],
) -> Result<HashMap<String, HashSet<String>>> {
    let mut crs_codes: Vec<String> = Vec::new();
    let mut tiploc_codes: Vec<(String, String)> = Vec::new();
    for code in codes {
        match common::location_naming::tiploc_from_code(code) {
            Some(tiploc) => tiploc_codes.push((code.clone(), tiploc)),
            None => crs_codes.push(code.to_ascii_uppercase()),
        }
    }
    let mut found: HashMap<String, HashSet<String>> = HashMap::new();
    if !crs_codes.is_empty() {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT UPPER(crs), tiploc FROM tiploc_crs WHERE UPPER(crs) = ANY($1) \
             UNION \
             SELECT crs, tiploc FROM stanox_crs WHERE crs = ANY($1)",
        )
        .bind(&crs_codes)
        .fetch_all(pool)
        .await?;
        for (crs, tiploc) in rows {
            found
                .entry(crs)
                .or_default()
                .insert(schedule_query::normalize_tiploc(&tiploc).to_string());
        }
    }
    if !tiploc_codes.is_empty() {
        let tiplocs: Vec<String> = tiploc_codes.iter().map(|(_, t)| t.clone()).collect();
        let known: Vec<(String,)> =
            sqlx::query_as("SELECT tiploc FROM tiploc_locations WHERE tiploc = ANY($1)")
                .bind(&tiplocs)
                .fetch_all(pool)
                .await?;
        let known: HashSet<String> = known.into_iter().map(|(t,)| t).collect();
        for (code, tiploc) in tiploc_codes {
            if known.contains(&tiploc) {
                found.entry(code).or_default().insert(tiploc);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    /// A public call, a working-only call, or a passing point.
    #[derive(Clone, Copy)]
    enum Kind {
        Public,
        Working,
        Pass,
    }

    fn call(seq: i16, tiploc: &str, kind: Kind, time: chrono::NaiveTime) -> ScheduleCall {
        let timed = !matches!(kind, Kind::Pass);
        let public = matches!(kind, Kind::Public);
        ScheduleCall {
            uid: "T00001".to_string(),
            seq,
            tiploc: tiploc.to_string(),
            booked_arrival: timed.then_some(time),
            booked_departure: timed.then_some(time),
            day_offset: 0,
            public_arrival: public.then_some(time),
            public_departure: public.then_some(time),
            can_board: Some(public),
            can_alight: Some(public),
        }
    }

    /// AAA (call) - JCT (pass) - BBB (working-only call) - CCC (call).
    fn span() -> Vec<ScheduleCall> {
        vec![
            call(1, "AAA", Kind::Public, hm(9, 0)),
            call(2, "JCT", Kind::Pass, hm(9, 5)),
            call(3, "BBB", Kind::Working, hm(9, 10)),
            call(4, "CCC", Kind::Public, hm(9, 20)),
        ]
    }

    fn set(tiplocs: &[&str]) -> HashSet<String> {
        tiplocs.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn calls_at_needs_a_public_call_in_order() {
        let filters = |calls_at: &[&[&str]]| StopFilters {
            calls_at: calls_at.iter().map(|entry| set(entry)).collect(),
            ..StopFilters::default()
        };
        assert!(filters(&[&["AAA"], &["CCC"]]).accepts(&span()));
        assert!(!filters(&[&["CCC"], &["AAA"]]).accepts(&span()), "order");
        assert!(!filters(&[&["JCT"]]).accepts(&span()), "a pass is no call");
        assert!(
            !filters(&[&["BBB"]]).accepts(&span()),
            "a working-only call is no passenger call"
        );
        assert!(filters(&[&["ZZZ", "CCC"]]).accepts(&span()), "an OR choice");
    }

    #[test]
    fn via_is_satisfied_by_a_call_or_a_pass_in_order() {
        let filters = |via: &[&[&str]]| StopFilters {
            via: via.iter().map(|entry| set(entry)).collect(),
            ..StopFilters::default()
        };
        assert!(filters(&[&["JCT"]]).accepts(&span()));
        assert!(filters(&[&["BBB"], &["CCC"]]).accepts(&span()));
        assert!(!filters(&[&["CCC"], &["JCT"]]).accepts(&span()));
        assert!(!filters(&[&["ZZZ"]]).accepts(&span()));
    }

    #[test]
    fn avoid_rejects_any_row_and_avoid_stop_only_a_timed_one() {
        let avoid = |tiplocs: &[&str]| StopFilters {
            avoid: set(tiplocs),
            ..StopFilters::default()
        };
        let avoid_stop = |tiplocs: &[&str]| StopFilters {
            avoid_stop: set(tiplocs),
            ..StopFilters::default()
        };
        assert!(
            !avoid(&["JCT"]).accepts(&span()),
            "running through is not avoiding"
        );
        assert!(!avoid(&["AAA"]).accepts(&span()));
        assert!(avoid(&["ZZZ"]).accepts(&span()));
        assert!(
            avoid_stop(&["JCT"]).accepts(&span()),
            "running through is fine"
        );
        assert!(
            !avoid_stop(&["BBB"]).accepts(&span()),
            "a working-only call is a stop"
        );
        assert!(!avoid_stop(&["CCC"]).accepts(&span()));
    }

    #[test]
    fn an_empty_span_meets_no_positive_filter() {
        let filters = StopFilters {
            via: vec![set(&["AAA"])],
            ..StopFilters::default()
        };
        assert!(!filters.accepts(&[]));
        assert!(StopFilters::default().accepts(&[]));
    }

    #[test]
    fn a_row_from_before_public_times_counts_as_a_public_call_when_timed() {
        let mut old = call(1, "AAA", Kind::Working, hm(9, 0));
        old.can_board = None;
        old.can_alight = None;
        assert!(old.is_public_call());
        let mut pass = call(1, "AAA", Kind::Pass, hm(9, 0));
        pass.can_board = None;
        pass.can_alight = None;
        assert!(!pass.is_public_call());
    }

    #[test]
    fn find_departure_matches_the_station_time_and_day() {
        let calls = span();
        assert_eq!(
            find_departure(&calls, &set(&["CCC"]), hm(9, 20), 0, None),
            Some(3)
        );
        assert_eq!(
            find_departure(&calls, &set(&["CCC"]), hm(9, 20), 1, None),
            None
        );
        assert_eq!(
            find_departure(&calls, &set(&["AAA"]), hm(9, 0), 0, Some(0)),
            None,
            "only after the given index"
        );
        // A TIPLOC the lookup did not name still matches on the time.
        assert_eq!(
            find_departure(&calls, &set(&["OTHER"]), hm(9, 10), 0, None),
            Some(2)
        );
    }

    /// ORIG - AAA - JCT (pass) - BBB - CCC (terminus).
    fn train() -> Vec<ScheduleCall> {
        let mut calls = vec![call(0, "ORIG", Kind::Public, hm(8, 50))];
        calls.extend(span());
        calls
    }

    fn tiplocs_of(span: Option<&[ScheduleCall]>) -> Option<Vec<&str>> {
        span.map(|calls| calls.iter().map(|c| c.tiploc.as_str()).collect())
    }

    #[test]
    fn a_departure_rides_past_the_calls_after_it() {
        let calls = train();
        let span = departure_span(&calls, &set(&["AAA"]), hm(9, 0), 0, RideEnd::Onward);
        assert_eq!(tiplocs_of(span), Some(vec!["JCT", "BBB", "CCC"]));
    }

    #[test]
    fn a_ride_between_two_calls_excludes_both() {
        let calls = train();
        let ccc = set(&["CCC"]);
        let to_call = RideEnd::Call {
            tiplocs: &set(&["BBB"]),
            scheduled: hm(9, 10),
            day_offset: 0,
        };
        let span = departure_span(&calls, &set(&["ORIG"]), hm(8, 50), 0, to_call);
        assert_eq!(tiplocs_of(span), Some(vec!["AAA", "JCT"]));
        let span = departure_span(&calls, &set(&["AAA"]), hm(9, 0), 0, RideEnd::Terminus);
        assert_eq!(tiplocs_of(span), Some(vec!["JCT", "BBB"]));
        // An end that cannot be placed after the boarding call.
        let before = RideEnd::Call {
            tiplocs: &set(&["ORIG"]),
            scheduled: hm(8, 50),
            day_offset: 0,
        };
        assert_eq!(
            departure_span(&calls, &set(&["AAA"]), hm(9, 0), 0, before),
            None
        );
        assert_eq!(
            departure_span(&calls, &ccc, hm(9, 20), 0, RideEnd::Terminus),
            None
        );
    }

    #[test]
    fn an_arrival_rides_past_the_calls_before_it() {
        let calls = train();
        let span = arrival_span(&calls, &set(&["BBB"]), Some((hm(9, 10), 0)));
        assert_eq!(tiplocs_of(span), Some(vec!["ORIG", "AAA", "JCT"]));
        let span = arrival_span(&calls, &set(&["CCC"]), None);
        assert_eq!(tiplocs_of(span), Some(vec!["ORIG", "AAA", "JCT", "BBB"]));
        assert_eq!(arrival_span(&[], &set(&["CCC"]), None), None);
    }

    #[test]
    fn a_dwell_across_midnight_departs_the_next_day() {
        let mut dwell = call(1, "AAA", Kind::Public, hm(23, 58));
        dwell.booked_departure = Some(hm(0, 2));
        assert!(dwell.departs_at(hm(0, 2), 1));
        assert!(!dwell.departs_at(hm(0, 2), 0));
    }
}
