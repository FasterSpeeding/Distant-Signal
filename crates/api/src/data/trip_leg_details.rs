//! Post-planning enrichment of `/Trips/plan` train legs with CIF-derived
//! detail the planner itself never needs: the booked (timetabled) platform
//! at each leg's boarding and alighting calling point, and the schedule's
//! ATOC operator code.
//!
//! Kept OUT of the planner hot path (`trip_planning::fetch_calling_points_for_date`
//! / `build_connections` / the CSA/RAPTOR searches) on purpose: the search
//! reads every calling point of the whole day, while only the handful of
//! schedules that actually appear in a returned itinerary need this, so it
//! is a few small, uid-scoped batch reads after the search instead of widening
//! the whole-day read. Only CIF data is used -- never Darwin/live platform.
//!
//! The schedule's headcode (the CIF `BS` Train Identity -- NOT the TRUST
//! 10-character `train_id`) is read the same way as the operator, from
//! `schedule_destination_departures.headcode`.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{NaiveDate, NaiveTime, Timelike};
use sqlx::PgPool;

use crate::data::trip_planning_itinerary::{PlannedLeg, SegmentResult};

/// One `schedule_calling_points_full` row of a schedule that appears in a
/// planned itinerary.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegCallingPointRow {
    pub uid: String,
    pub booked_arrival: Option<NaiveTime>,
    pub booked_departure: Option<NaiveTime>,
    pub day_offset: i16,
    pub platform: Option<String>,
    /// The call's public (GBTT) times, NULL on a row published before
    /// migration `20261001120000`.
    pub public_arrival: Option<NaiveTime>,
    pub public_departure: Option<NaiveTime>,
}

/// The day offset of a public time given the WTT time and offset of the
/// same call: the same day, unless rounding straddles midnight.
fn public_day_offset(wtt: NaiveTime, wtt_offset: u8, public: NaiveTime) -> u8 {
    let gap = public.signed_duration_since(wtt);
    if gap < -chrono::Duration::hours(12) {
        wtt_offset.saturating_add(1)
    } else if gap > chrono::Duration::hours(12) {
        wtt_offset.saturating_sub(1)
    } else {
        wtt_offset
    }
}

/// Same minutes-since-service-day-midnight arithmetic as
/// `schedule_query::connections`' own (private) `minutes_from_midnight`,
/// which is what `TrainLeg::departure_min`/`arrival_min` -- and therefore
/// `PlannedLeg::Train`'s times -- were computed from. Matching on it (not on
/// TIPLOC alone) keeps a schedule that visits the same TIPLOC twice (a loop)
/// from picking the wrong visit's platform.
fn minutes(time: NaiveTime, day_offset: u8) -> u32 {
    time.num_seconds_from_midnight() / 60 + u32::from(day_offset) * 1440
}

/// `schedule_calling_points_full.day_offset` is `SMALLINT`; same
/// `max(0) as u8` clamp `trip_planning::fetch_calling_points_for_date` uses.
fn row_day_offset(row: &LegCallingPointRow) -> u8 {
    row.day_offset.max(0) as u8
}

/// Pure half of [`attach_leg_details`]: fills every train leg's
/// `booked_departure_platform`/`booked_arrival_platform`/`operator`/
/// `headcode` from the already-fetched rows. A leg whose calling point,
/// operator or headcode has no row is left `None` -- "not known", never
/// guessed (see [`unambiguous_headcodes`] for conflicting headcodes).
///
/// A leg's TIPLOCs are not carried on `PlannedLeg` (only its CRS pair), so
/// the boarding/alighting calling point is identified by `(uid, time)`:
/// the row whose booked departure (boarding) / booked arrival (alighting)
/// lands on exactly the leg's own minute. If more than one row shares that
/// `(uid, minute)` -- two TIPLOCs of one schedule at the same minute --
/// their platforms must agree, otherwise the answer is `None`.
pub fn apply_leg_details(
    segments: &mut [SegmentResult],
    rows: &[LegCallingPointRow],
    operators: &HashMap<String, String>,
    headcodes: &HashMap<String, String>,
) {
    let mut departures: HashMap<(String, u32), Option<String>> = HashMap::new();
    let mut arrivals: HashMap<(String, u32), Option<String>> = HashMap::new();
    let mut public_departures: HashMap<(String, u32), Option<NaiveTime>> = HashMap::new();
    let mut public_arrivals: HashMap<(String, u32), Option<NaiveTime>> = HashMap::new();
    fn record<T: PartialEq + Clone>(
        map: &mut HashMap<(String, u32), Option<T>>,
        uid: &str,
        at: u32,
        value: &Option<T>,
    ) {
        map.entry((uid.to_string(), at))
            .and_modify(|existing| {
                if existing != value {
                    *existing = None;
                }
            })
            .or_insert_with(|| value.clone());
    }
    for row in rows {
        if let Some(departure) = row.booked_departure {
            // The departure's own day (R-043), as the planner computed
            // `departure_min` with.
            let at = minutes(
                departure,
                schedule_query::records::departure_day_offset(
                    row.booked_arrival,
                    Some(departure),
                    row_day_offset(row),
                ),
            );
            record(&mut departures, &row.uid, at, &row.platform);
            record(&mut public_departures, &row.uid, at, &row.public_departure);
        }
        if let Some(arrival) = row.booked_arrival {
            let at = minutes(arrival, row_day_offset(row));
            record(&mut arrivals, &row.uid, at, &row.platform);
            record(&mut public_arrivals, &row.uid, at, &row.public_arrival);
        }
    }
    for segment in segments.iter_mut() {
        for itinerary in &mut segment.itineraries {
            for leg in &mut itinerary.legs {
                let PlannedLeg::Train {
                    train_uid,
                    scheduled_departure,
                    scheduled_arrival,
                    departure_day_offset,
                    arrival_day_offset,
                    booked_departure_platform,
                    booked_arrival_platform,
                    public_departure,
                    public_arrival,
                    public_departure_day_offset,
                    public_arrival_day_offset,
                    operator,
                    headcode,
                    ..
                } = leg
                else {
                    continue;
                };
                // The planner's own departure day offset first; the
                // arrival's offset and the same-day reading remain as
                // fallbacks for legs built without one.
                let departure_minutes = [*departure_day_offset, *arrival_day_offset, 0]
                    .into_iter()
                    .map(|offset| minutes(*scheduled_departure, offset))
                    .find(|at| departures.contains_key(&(train_uid.clone(), *at)));
                *booked_departure_platform = departure_minutes
                    .and_then(|at| departures.get(&(train_uid.clone(), at)).cloned())
                    .flatten();
                let arrival_key = (
                    train_uid.clone(),
                    minutes(*scheduled_arrival, *arrival_day_offset),
                );
                *booked_arrival_platform = arrivals.get(&arrival_key).cloned().flatten();
                *public_departure = departure_minutes
                    .and_then(|at| public_departures.get(&(train_uid.clone(), at)).cloned())
                    .flatten();
                *public_departure_day_offset = public_departure.map(|public| {
                    let offset =
                        departure_minutes.map_or(*departure_day_offset, |at| (at / 1440) as u8);
                    public_day_offset(*scheduled_departure, offset, public)
                });
                *public_arrival = public_arrivals.get(&arrival_key).cloned().flatten();
                *public_arrival_day_offset = public_arrival.map(|public| {
                    public_day_offset(*scheduled_arrival, *arrival_day_offset, public)
                });
                *operator = operators.get(train_uid.as_str()).cloned();
                *headcode = headcodes.get(train_uid.as_str()).cloned();
            }
        }
    }
}

/// Collapses distinct `(train_uid, headcode)` pairs into one headcode per
/// schedule, dropping any schedule whose stored rows disagree (e.g. a
/// half-replaced publish) -- conflicting means "not known", never a guess.
pub fn unambiguous_headcodes(pairs: Vec<(String, String)>) -> HashMap<String, String> {
    let mut by_uid: HashMap<String, Option<String>> = HashMap::new();
    for (uid, headcode) in pairs {
        by_uid
            .entry(uid)
            .and_modify(|existing| {
                if existing.as_deref() != Some(headcode.as_str()) {
                    *existing = None;
                }
            })
            .or_insert(Some(headcode));
    }
    by_uid
        .into_iter()
        .filter_map(|(uid, headcode)| headcode.map(|h| (uid, h)))
        .collect()
}

/// Fetches the calling points, operator and headcode of every schedule appearing in
/// `segments`' train legs on `date` (three batched, uid-scoped queries) and
/// applies them via [`apply_leg_details`]. A no-op with no train legs.
pub async fn attach_leg_details(
    pool: &PgPool,
    date: NaiveDate,
    segments: &mut [SegmentResult],
) -> Result<()> {
    let mut uids: Vec<String> = segments
        .iter()
        .flat_map(|segment| &segment.itineraries)
        .flat_map(|itinerary| &itinerary.legs)
        .filter_map(|leg| match leg {
            PlannedLeg::Train { train_uid, .. } => Some(train_uid.clone()),
            PlannedLeg::Transfer { .. } => None,
        })
        .collect();
    uids.sort_unstable();
    uids.dedup();
    if uids.is_empty() {
        return Ok(());
    }

    let rows: Vec<LegCallingPointRow> = sqlx::query_as(
        "SELECT uid, booked_arrival, booked_departure, day_offset, platform, \
                public_arrival, public_departure \
         FROM schedule_calling_points_full WHERE service_date = $1 AND uid = ANY($2)",
    )
    .bind(date)
    .bind(&uids)
    .fetch_all(pool)
    .await?;

    // One operator per schedule: `operator_atoc` is denormalized identically
    // onto every row a schedule contributes (see its migration), so any
    // non-null row answers it.
    let operators: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT ON (train_uid) train_uid, operator_atoc \
         FROM schedule_destination_departures \
         WHERE service_date = $1 AND train_uid = ANY($2) AND operator_atoc IS NOT NULL",
    )
    .bind(date)
    .bind(&uids)
    .fetch_all(pool)
    .await?;

    // Headcode is likewise per schedule; every distinct non-null value is
    // fetched so disagreeing rows can be detected and reported as unknown.
    let headcodes: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT train_uid, headcode \
         FROM schedule_destination_departures \
         WHERE service_date = $1 AND train_uid = ANY($2) AND headcode IS NOT NULL",
    )
    .bind(date)
    .bind(&uids)
    .fetch_all(pool)
    .await?;

    apply_leg_details(
        segments,
        &rows,
        &operators.into_iter().collect(),
        &unambiguous_headcodes(headcodes),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::trip_planning_itinerary::PlannedItinerary;

    fn train_leg(uid: &str, departs: &str, arrives: &str, arrival_day_offset: u8) -> PlannedLeg {
        PlannedLeg::Train {
            train_uid: uid.to_string(),
            service_date: NaiveDate::from_ymd_opt(2026, 9, 24).unwrap(),
            origin_crs: None,
            destination_crs: None,
            scheduled_departure: departs.parse().unwrap(),
            scheduled_arrival: arrives.parse().unwrap(),
            departure_day_offset: 0,
            arrival_day_offset,
            booked_departure_platform: None,
            booked_arrival_platform: None,
            public_departure: None,
            public_arrival: None,
            public_departure_day_offset: None,
            public_arrival_day_offset: None,
            operator: None,
            headcode: None,
            from_tiploc: String::new(),
            to_tiploc: String::new(),
            departure_min: 0,
            arrival_min: 0,
            live: None,
        }
    }

    fn row(
        uid: &str,
        arrival: Option<&str>,
        departure: Option<&str>,
        day_offset: i16,
        platform: Option<&str>,
    ) -> LegCallingPointRow {
        LegCallingPointRow {
            uid: uid.to_string(),
            booked_arrival: arrival.map(|t| t.parse().unwrap()),
            booked_departure: departure.map(|t| t.parse().unwrap()),
            day_offset,
            platform: platform.map(str::to_string),
            public_arrival: None,
            public_departure: None,
        }
    }

    fn public_times(leg: &PlannedLeg) -> (Option<String>, Option<u8>, Option<String>, Option<u8>) {
        let PlannedLeg::Train {
            public_departure,
            public_departure_day_offset,
            public_arrival,
            public_arrival_day_offset,
            ..
        } = leg
        else {
            panic!("expected a train leg");
        };
        (
            public_departure.map(|t| t.format("%H:%M").to_string()),
            *public_departure_day_offset,
            public_arrival.map(|t| t.format("%H:%M").to_string()),
            *public_arrival_day_offset,
        )
    }

    /// R-043: boarding at a stop that dwells across midnight (arrive 23:55,
    /// depart 00:02, stored with the arrival's day offset 0) is matched on
    /// the departure's own next-day minute, so its platform and public
    /// departure are found and dated the next day.
    #[test]
    fn a_midnight_dwell_boarding_matches_on_the_next_day() {
        let mut leg = train_leg("U1", "00:02:00", "00:05:00", 1);
        if let PlannedLeg::Train {
            departure_day_offset,
            ..
        } = &mut leg
        {
            *departure_day_offset = 1;
        }
        let mut segments = segments(vec![leg]);
        let mut boarding = row("U1", Some("23:55:00"), Some("00:02:00"), 0, Some("3"));
        boarding.public_departure = "00:02:00".parse().ok();
        let mut alighting = row("U1", Some("00:05:00"), None, 1, None);
        alighting.public_arrival = "00:05:00".parse().ok();
        apply_leg_details(
            &mut segments,
            &[boarding, alighting],
            &HashMap::new(),
            &HashMap::new(),
        );
        let leg = &segments[0].itineraries[0].legs[0];
        assert_eq!(details(leg).0, Some("3"));
        assert_eq!(
            public_times(leg),
            (
                Some("00:02".to_string()),
                Some(1),
                Some("00:05".to_string()),
                Some(1)
            )
        );
    }

    /// The public times come from the boarding and alighting calls, and a
    /// public arrival rounded up past midnight lands on the next day.
    #[test]
    fn fills_public_times_and_their_day_offsets() {
        let mut segments = segments(vec![train_leg("U1", "20:52:00", "23:59:00", 0)]);
        let mut boarding = row("U1", Some("20:50:00"), Some("20:52:00"), 0, None);
        boarding.public_arrival = "20:51:00".parse().ok();
        boarding.public_departure = "20:52:00".parse().ok();
        let mut alighting = row("U1", Some("23:59:00"), None, 0, None);
        alighting.public_arrival = "00:00:00".parse().ok();
        apply_leg_details(
            &mut segments,
            &[boarding, alighting],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            public_times(&segments[0].itineraries[0].legs[0]),
            (
                Some("20:52".to_string()),
                Some(0),
                Some("00:00".to_string()),
                Some(1)
            )
        );
    }

    fn segments(legs: Vec<PlannedLeg>) -> Vec<SegmentResult> {
        vec![SegmentResult {
            origin_crs: "AAA".to_string(),
            destination_crs: "BBB".to_string(),
            itineraries: vec![PlannedItinerary {
                legs,
                change_count: 0,
                total_duration_minutes: 0,
                exceeds_recommended_changes: None,
                departure_min: 0,
                arrival_min: 0,
                arrival_tiploc: None,
                departure_tiploc: None,
                live_feasible: None,
                continues_previous_train: false,
            }],
            capped_by_max_changes: false,
            depart_after_min: None,
            arrive_by_min: None,
            no_result_reason: None,
        }]
    }

    fn details(leg: &PlannedLeg) -> (Option<&str>, Option<&str>, Option<&str>) {
        let PlannedLeg::Train {
            booked_departure_platform,
            booked_arrival_platform,
            operator,
            ..
        } = leg
        else {
            panic!("expected a train leg");
        };
        (
            booked_departure_platform.as_deref(),
            booked_arrival_platform.as_deref(),
            operator.as_deref(),
        )
    }

    #[test]
    fn fills_boarding_and_alighting_platforms_by_time_and_operator_by_uid() {
        // Boards at the 08:10 intermediate stop (not the 08:00 origin),
        // alights at an overnight 00:20 (+1 day) terminus.
        let mut segments = segments(vec![train_leg("U1", "08:10:00", "00:20:00", 1)]);
        let rows = vec![
            row("U1", None, Some("08:00:00"), 0, Some("1")),
            row("U1", Some("08:09:00"), Some("08:10:00"), 0, Some("2A")),
            row("U1", Some("00:20:00"), None, 1, Some("7")),
            // Same clock time on a DIFFERENT schedule -- must not leak in.
            row("U2", None, Some("08:10:00"), 0, Some("9")),
        ];
        let operators = HashMap::from([("U1".to_string(), "SW".to_string())]);
        let headcodes = HashMap::from([("U1".to_string(), "1S00".to_string())]);

        apply_leg_details(&mut segments, &rows, &operators, &headcodes);

        assert_eq!(
            details(&segments[0].itineraries[0].legs[0]),
            (Some("2A"), Some("7"), Some("SW"))
        );
        assert_eq!(headcode(&segments[0].itineraries[0].legs[0]), Some("1S00"));
    }

    #[test]
    fn unknown_or_ambiguous_details_stay_none() {
        let mut segments = segments(vec![
            train_leg("U1", "08:00:00", "08:50:00", 0),
            train_leg("U3", "09:00:00", "09:30:00", 0),
        ]);
        let rows = vec![
            // Blank CIF platform at boarding.
            row("U1", None, Some("08:00:00"), 0, None),
            // Two TIPLOCs of one schedule at the alighting minute that
            // disagree -- not guessable.
            row("U1", Some("08:50:00"), None, 0, Some("4")),
            row("U1", Some("08:50:00"), Some("08:51:00"), 0, Some("5")),
        ];

        apply_leg_details(&mut segments, &rows, &HashMap::new(), &HashMap::new());

        let legs = &segments[0].itineraries[0].legs;
        assert_eq!(details(&legs[0]), (None, None, None));
        // No rows at all for U3.
        assert_eq!(details(&legs[1]), (None, None, None));
        assert_eq!(headcode(&legs[0]), None);
        assert_eq!(headcode(&legs[1]), None);
    }

    fn headcode(leg: &PlannedLeg) -> Option<&str> {
        let PlannedLeg::Train { headcode, .. } = leg else {
            panic!("expected a train leg");
        };
        headcode.as_deref()
    }

    #[test]
    fn conflicting_headcodes_for_one_schedule_are_dropped() {
        let pairs = vec![
            ("U1".to_string(), "1S00".to_string()),
            ("U1".to_string(), "1S00".to_string()),
            ("U2".to_string(), "2E88".to_string()),
            ("U2".to_string(), "2E89".to_string()),
        ];
        let headcodes = unambiguous_headcodes(pairs);
        assert_eq!(headcodes.get("U1").map(String::as_str), Some("1S00"));
        assert_eq!(headcodes.get("U2"), None);

        let mut segments = segments(vec![
            train_leg("U1", "08:00:00", "08:50:00", 0),
            train_leg("U2", "09:00:00", "09:30:00", 0),
        ]);
        apply_leg_details(&mut segments, &[], &HashMap::new(), &headcodes);
        let legs = &segments[0].itineraries[0].legs;
        assert_eq!(headcode(&legs[0]), Some("1S00"));
        assert_eq!(headcode(&legs[1]), None);
    }
}
