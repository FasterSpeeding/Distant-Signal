//! Post-planning enrichment of `/Trips/plan` train legs with CIF-derived
//! detail the planner itself never needs: the booked (timetabled) platform
//! at each leg's boarding and alighting calling point, and the schedule's
//! ATOC operator code.
//!
//! Kept OUT of the planner hot path (`trip_planning::fetch_calling_points_for_date`
//! / `build_connections` / the CSA/RAPTOR searches) on purpose: the search
//! reads every calling point of the whole day, while only the handful of
//! schedules that actually appear in a returned itinerary need this, so it
//! is two small, uid-scoped batch reads after the search instead of widening
//! the whole-day read. Only CIF data is used -- never Darwin/live platform.
//!
//! Headcode is deliberately NOT here: the CIF `BS` Train Identity field is
//! not decoded by `schedule_query` nor stored in any table, so surfacing it
//! would need new parse + publish + storage plumbing.

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
/// `booked_departure_platform`/`booked_arrival_platform`/`operator` from the
/// already-fetched rows. A leg whose calling point or operator has no row
/// is left `None` -- "not known", never guessed.
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
) {
    let mut departures: HashMap<(String, u32), Option<String>> = HashMap::new();
    let mut arrivals: HashMap<(String, u32), Option<String>> = HashMap::new();
    let record = |map: &mut HashMap<(String, u32), Option<String>>,
                  uid: &str,
                  at: u32,
                  platform: &Option<String>| {
        map.entry((uid.to_string(), at))
            .and_modify(|existing| {
                if existing != platform {
                    *existing = None;
                }
            })
            .or_insert_with(|| platform.clone());
    };
    for row in rows {
        if let Some(departure) = row.booked_departure {
            record(
                &mut departures,
                &row.uid,
                minutes(departure, row_day_offset(row)),
                &row.platform,
            );
        }
        if let Some(arrival) = row.booked_arrival {
            record(
                &mut arrivals,
                &row.uid,
                minutes(arrival, row_day_offset(row)),
                &row.platform,
            );
        }
    }
    for segment in segments.iter_mut() {
        for itinerary in &mut segment.itineraries {
            for leg in &mut itinerary.legs {
                let PlannedLeg::Train {
                    train_uid,
                    scheduled_departure,
                    scheduled_arrival,
                    arrival_day_offset,
                    booked_departure_platform,
                    booked_arrival_platform,
                    operator,
                    ..
                } = leg
                else {
                    continue;
                };
                // A leg's own departure can itself be past midnight only if
                // the arrival is too; the planner never reports a departure
                // day offset separately, so try the arrival's offset first
                // and then the same-day reading.
                let departure_minutes = [*arrival_day_offset, 0]
                    .into_iter()
                    .map(|offset| minutes(*scheduled_departure, offset))
                    .find(|at| departures.contains_key(&(train_uid.clone(), *at)));
                *booked_departure_platform = departure_minutes
                    .and_then(|at| departures.get(&(train_uid.clone(), at)).cloned())
                    .flatten();
                *booked_arrival_platform = arrivals
                    .get(&(
                        train_uid.clone(),
                        minutes(*scheduled_arrival, *arrival_day_offset),
                    ))
                    .cloned()
                    .flatten();
                *operator = operators.get(train_uid.as_str()).cloned();
            }
        }
    }
}

/// Fetches the calling points and operator of every schedule appearing in
/// `segments`' train legs on `date` (two batched, uid-scoped queries) and
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
        "SELECT uid, booked_arrival, booked_departure, day_offset, platform \
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

    apply_leg_details(segments, &rows, &operators.into_iter().collect());
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
            arrival_day_offset,
            booked_departure_platform: None,
            booked_arrival_platform: None,
            operator: None,
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
        }
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
            }],
            capped_by_max_changes: false,
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

        apply_leg_details(&mut segments, &rows, &operators);

        assert_eq!(
            details(&segments[0].itineraries[0].legs[0]),
            (Some("2A"), Some("7"), Some("SW"))
        );
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

        apply_leg_details(&mut segments, &rows, &HashMap::new());

        let legs = &segments[0].itineraries[0].legs;
        assert_eq!(details(&legs[0]), (None, None, None));
        // No rows at all for U3.
        assert_eq!(details(&legs[1]), (None, None, None));
    }
}
