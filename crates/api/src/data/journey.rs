//! Merges a train's scheduled timetable (from `trains.calling_points` when
//! schedule-matching has populated it, else reconstructed from
//! `schedule_destination_departures`) with the latest reported movement
//! event per location, into one ordered `JourneyStop[]` -- the primary
//! data source for the train detail page's timeline. See
//! docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::eta_blend::london_to_utc;
use crate::data::queries;

/// Mirrors `schedule_matching::ScheduleCallingPointDto`'s exact camelCase
/// wire shape (the format `trains.calling_points` is stored in) -- a
/// separate, `Deserialize`-only type rather than importing that module's
/// private struct, matching this codebase's "each layer owns its own wire
/// shape" posture (the same relationship `frontend/lib/types.ts`'s
/// `ScheduleCallingPoint` already has to it, just on the Rust side).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCallingPoint {
    tiploc: String,
    kind: schedule_query::CallingPointKind,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    /// Mirrors `schedule_matching::ScheduleCallingPointDto::day_offset` /
    /// `schedule_query::CallingPoint::day_offset` -- how many calendar days
    /// past `service_date` this calling point's booked times actually fall
    /// on (a real overnight service crosses midnight mid-schedule; see that
    /// field's own doc comment). `#[serde(default)]` so a `trains.calling_points`
    /// row written before this field existed still deserializes, as `0`
    /// (the previous, buggy "always same day" behavior) rather than
    /// failing outright.
    #[serde(default)]
    day_offset: u8,
}

/// One calling point of a train's journey, booked schedule merged with the
/// latest reported live data for that location -- see this module's own
/// doc comment and the design doc §2/§3.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyStop {
    pub crs: Option<String>,
    pub name: Option<String>,
    pub tiploc: Option<String>,
    pub kind: Option<schedule_query::CallingPointKind>,
    pub scheduled_arrival: Option<DateTime<Utc>>,
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    pub last_event_type: Option<String>,
    pub variation_status: Option<String>,
    pub delay_minutes: Option<i32>,
}

impl JourneyStop {
    fn from_calling_point(
        cp: &RawCallingPoint,
        crs: Option<String>,
        service_date: NaiveDate,
    ) -> Self {
        // `day_offset` calendar days past `service_date` -- see
        // `RawCallingPoint::day_offset`'s own doc comment for why this
        // can't just be `service_date` unconditionally: a real overnight
        // service's post-midnight calling points are really the NEXT
        // calendar day.
        let calling_point_date = service_date + Duration::days(cp.day_offset as i64);
        Self {
            crs,
            name: None, // filled in by a batch station-name pass in `build_journey_stops`
            tiploc: Some(cp.tiploc.clone()),
            kind: Some(cp.kind),
            scheduled_arrival: cp
                .booked_arrival
                .and_then(|t| london_to_utc(calling_point_date.and_time(t))),
            scheduled_departure: cp
                .booked_departure
                .and_then(|t| london_to_utc(calling_point_date.and_time(t))),
            actual_arrival: None,
            actual_departure: None,
            last_event_type: None,
            variation_status: None,
            delay_minutes: None,
        }
    }
}

/// Builds the ordered stop list for `(train_uid, service_date)`, or `None`
/// if neither the primary (`calling_points_json`) nor fallback
/// (`schedule_destination_departures`) source has anything -- see the
/// design doc §1 for when this is/isn't called, and §0.2/§3.2 for the
/// fallback's synthetic-terminus construction.
pub async fn build_journey_stops(
    pool: &PgPool,
    trains_id: i64,
    train_uid: &str,
    service_date: NaiveDate,
    calling_points_json: Option<&serde_json::Value>,
) -> anyhow::Result<Option<Vec<JourneyStop>>> {
    let mut stops: Vec<JourneyStop> = match calling_points_json {
        Some(json) => {
            let raw: Vec<RawCallingPoint> = serde_json::from_value(json.clone())?;
            let tiplocs: Vec<String> = raw.iter().map(|cp| cp.tiploc.clone()).collect();
            let tiploc_to_crs = queries::crs_for_tiplocs_batch(pool, &tiplocs).await?;
            raw.iter()
                .map(|cp| {
                    let crs = tiploc_to_crs.get(&cp.tiploc.to_uppercase()).cloned();
                    JourneyStop::from_calling_point(cp, crs, service_date)
                })
                .collect()
        }
        None => {
            let rows =
                queries::list_calling_point_departures_for_train(pool, train_uid, service_date)
                    .await?;
            if rows.is_empty() {
                return Ok(None);
            }
            let mut built: Vec<JourneyStop> = rows
                .iter()
                .map(|row| JourneyStop {
                    crs: Some(row.origin_crs.clone()),
                    name: None,
                    tiploc: None,
                    kind: Some(
                        if row.true_origin_crs.as_deref().is_some_and(|true_origin| {
                            true_origin.eq_ignore_ascii_case(&row.origin_crs)
                        }) {
                            schedule_query::CallingPointKind::Origin
                        } else {
                            schedule_query::CallingPointKind::Intermediate
                        },
                    ),
                    scheduled_arrival: None,
                    // `row.day_offset` -- see `queries::CallingPointDepartureRow::day_offset`'s
                    // own doc comment -- shifts the base date forward for a
                    // calling point that falls on a calendar day AFTER
                    // `service_date` (a real overnight service). Same fix as
                    // the `calling_points_json` branch above, for this
                    // fallback source.
                    scheduled_departure: london_to_utc(
                        (service_date + Duration::days(row.day_offset as i64))
                            .and_time(row.scheduled),
                    ),
                    actual_arrival: None,
                    actual_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                })
                .collect();

            if let Some(destination_crs) = rows.last().and_then(|r| r.destination_crs.clone())
                && built
                    .last()
                    .and_then(|s| s.crs.as_deref())
                    .is_none_or(|last_crs| !last_crs.eq_ignore_ascii_case(&destination_crs))
            {
                built.push(JourneyStop {
                    crs: Some(destination_crs),
                    name: None,
                    tiploc: None,
                    kind: Some(schedule_query::CallingPointKind::Terminate),
                    scheduled_arrival: None,
                    scheduled_departure: None,
                    actual_arrival: None,
                    actual_departure: None,
                    last_event_type: None,
                    variation_status: None,
                    delay_minutes: None,
                });
            }
            built
        }
    };

    if stops.is_empty() {
        return Ok(None);
    }

    // Station names, batched over every distinct CRS this stop list has.
    let stop_crs: Vec<String> = stops.iter().filter_map(|s| s.crs.clone()).collect();
    let names = queries::station_names_for_crs_batch(pool, &stop_crs).await?;
    for stop in &mut stops {
        if let Some(crs) = &stop.crs {
            stop.name = names.get(&crs.to_uppercase()).cloned();
        }
    }

    // Live overlay.
    let events = queries::latest_movement_event_per_location(pool, trains_id).await?;
    let events_by_crs: HashMap<String, queries::MovementEventRow> =
        events.into_iter().map(|e| (e.loc_crs.clone(), e)).collect();

    for stop in &mut stops {
        let Some(crs) = &stop.crs else { continue };
        let Some(event) = events_by_crs.get(&crs.to_uppercase()) else {
            continue;
        };
        stop.last_event_type = event.event_type.clone();
        stop.variation_status = event.variation_status.clone();

        match event.event_type.as_deref() {
            Some("ARRIVAL") => {
                stop.actual_arrival = event.actual_timestamp;
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
            }
            Some("DEPARTURE") => {
                stop.actual_departure = event.actual_timestamp;
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            Some("PASS") => {
                stop.actual_arrival = event.actual_timestamp;
                stop.actual_departure = event.actual_timestamp;
                stop.scheduled_arrival = stop.scheduled_arrival.or(event.planned_timestamp);
                stop.scheduled_departure = stop.scheduled_departure.or(event.planned_timestamp);
            }
            _ => {}
        }

        // Delay is diffed from THIS movement event's own two fields --
        // `actual_timestamp` and `planned_timestamp`, both off the SAME
        // `train_movement_events` row -- rather than against
        // `stop.scheduled_arrival`/`scheduled_departure`, which (once a CIF
        // schedule source has populated them, via `from_calling_point` or
        // the fallback branch above) come from a completely different
        // pipeline: the CIF timetable, correctly BST-converted via
        // `chrono_tz`/`london_to_utc`. The real TRUST `TRAIN_MVT_ALL_TOC`
        // feed has been observed delivering `planned_timestamp` AND
        // `actual_timestamp` both skewed by the same amount vs true UTC
        // (an upstream feed issue, outside this codebase -- this repo's own
        // epoch-millis parsing in `trust-consumer` is unaffected). Diffing
        // TRUST's own two fields against EACH OTHER cancels that skew out,
        // exactly as `trust-consumer`'s own top-level `delay_minutes`
        // already does (`crates/trust-consumer/src/process.rs:708`:
        // `derived.delay_minutes = Some((a - p).num_minutes() as i32)`) --
        // positive means late. Diffing TRUST's `actual` against the
        // CIF-derived scheduled time instead mixes two independent
        // timestamp bases and, under that skew, manufactures a bogus ~1
        // hour "late" even when the train is genuinely on time per TRUST's
        // own self-consistent numbers (see this fix's own regression
        // tests). Because both fields come off the one event row, there's
        // no ARRIVAL-vs-DEPARTURE pairing ambiguity to resolve here (unlike
        // the DISPLAYED `actual_arrival`/`actual_departure` /
        // `scheduled_arrival`/`scheduled_departure` above, which do need
        // that pairing).
        //
        // If this event has no `planned_timestamp` (some TRUST messages
        // omit it), `delay_minutes` is `None` -- "delay unknown" -- rather
        // than falling back to the CIF-derived scheduled time, which would
        // silently reintroduce the exact cross-basis bug this is fixing.
        // This matches this function's established "don't guess when data
        // is incomplete" convention (e.g. the event-type `_ => {}` arm
        // just above, and the no-match-found early `continue` at the top
        // of this loop).
        stop.delay_minutes = match (event.actual_timestamp, event.planned_timestamp) {
            (Some(a), Some(p)) => Some((a - p).num_minutes() as i32),
            _ => None,
        };
    }

    Ok(Some(stops))
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_from_calling_points_json_resolves_tiploc_to_crs_and_kind() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-CP", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-1".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JRN-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-2".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-JRN-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-EUSTON",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-CREWE",
                "kind": "Terminate",
                "bookedArrival": "10:30:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-CP",
            service_date,
            Some(&calling_points),
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].crs.as_deref(), Some("EUS"));
        assert_eq!(
            stops[0].kind,
            Some(schedule_query::CallingPointKind::Origin)
        );
        assert!(stops[0].scheduled_departure.is_some());
        assert_eq!(stops[1].crs.as_deref(), Some("CRE"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );
        assert!(stops[1].scheduled_arrival.is_some());

        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_falls_back_to_schedule_destination_departures_and_appends_terminus()
     {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-FB", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:00:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
                crate::data::queries::ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "08:20:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-JRN-FB".to_string(),
                    origin_crs: "SLO".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                },
            ],
        )
        .await
        .expect("seed schedule_destination_departures");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-FB", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops from the fallback source");

        assert_eq!(stops.len(), 3, "RDG + SLO + synthetic WAT terminus");
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(
            stops[0].kind,
            Some(schedule_query::CallingPointKind::Origin)
        );
        assert_eq!(stops[1].crs.as_deref(), Some("SLO"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Intermediate)
        );
        assert_eq!(stops[2].crs.as_deref(), Some("WAT"));
        assert_eq!(
            stops[2].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );
        assert!(
            stops[2].scheduled_arrival.is_none(),
            "no arrival time known from this source yet"
        );

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-FB'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_returns_none_when_neither_source_has_anything \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_returns_none_when_neither_source_has_anything() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NONE", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NONE'",
        )
        .execute(&pool)
        .await
        .ok();

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-NONE", service_date, None)
            .await
            .expect("build_journey_stops");

        assert!(stops.is_none());

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_a_departure_event_with_correct_delay_sign \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_a_departure_event_with_correct_delay_sign() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-OV", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-OV".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k1', '0003', 'DEPARTURE', 'RDG', '2026-09-08T07:00:00Z', \
                     '2026-09-08T07:04:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-OV", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops");

        assert_eq!(
            stops.len(),
            2,
            "RDG + synthetic WAT terminus (destination_crs differs from last row)"
        );
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(
            stops[0].actual_departure,
            "2026-09-08T07:04:00Z".parse().ok()
        );
        assert_eq!(stops[0].last_event_type.as_deref(), Some("DEPARTURE"));
        assert_eq!(
            stops[0].delay_minutes,
            Some(4),
            "actual 4 minutes after this event's own planned time"
        );
        assert_eq!(stops[1].crs.as_deref(), Some("WAT"));
        assert_eq!(
            stops[1].kind,
            Some(schedule_query::CallingPointKind::Terminate)
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-OV'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the final whole-branch review's Finding 2: a
    /// stop with BOTH a booked arrival (10:00 London) and a booked
    /// departure (10:02 London) -- a real Intermediate calling point with
    /// a dwell time -- gets an `ARRIVAL`-only movement event one minute
    /// LATE. Under the old, buggy code (`scheduled_reference =
    /// stop.scheduled_departure.or(stop.scheduled_arrival)`, unconditional
    /// "prefer departure"), this would have paired the actual ARRIVAL
    /// (09:01 UTC) against the booked DEPARTURE (09:02 UTC), computing
    /// `09:01 - 09:02 = -1 minute` (`Some(-1)`, rendering as "1m early"
    /// for a train that arrived late). The fix pairs by
    /// `last_event_type` instead, so this asserts `Some(1)` (correctly
    /// late), NOT `Some(-1)`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_an_arrival_event_pairing_arrival_with_arrival_not_departure \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_an_arrival_event_pairing_arrival_with_arrival_not_departure()
     {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-ARR", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-ARR-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-ARR-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-ARR-2".to_string(),
                    crs: "TSA".to_string(),
                    tiploc: "TEST-JRN-ARR-MID".to_string(),
                    station_name: "TEST STATION A".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // Booked arrival 10:00 London, booked departure 10:02 London --
        // 2026-09-08 is within BST (UTC+1), so 09:00Z/09:02Z respectively.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-ARR-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-ARR-MID",
                "kind": "Intermediate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": "10:02:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-arr-1', '0001', 'ARRIVAL', 'TSA', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:01:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-ARR",
            service_date,
            Some(&calling_points),
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSA"));
        assert_eq!(stops[1].last_event_type.as_deref(), Some("ARRIVAL"));
        assert_eq!(stops[1].actual_arrival, "2026-09-08T09:01:00Z".parse().ok());
        assert_eq!(
            stops[1].delay_minutes,
            Some(1),
            "must pair actual ARRIVAL (09:01Z) against booked ARRIVAL (09:00Z) -> 1 minute late; \
             the old buggy code paired it against booked DEPARTURE (09:02Z) -> Some(-1), \
             falsely rendering as \"1m early\""
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-ARR-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// §6(d): a `PASS` event sets both `actual_arrival` and
    /// `actual_departure` to the same instant (a passing train's arrival
    /// and departure are the same instant for display purposes), and
    /// `delay_minutes` is computed correctly against whichever scheduled
    /// time is available (the fallback source here only ever has
    /// `scheduled_departure`).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_overlays_a_pass_event_setting_both_actual_times \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_overlays_a_pass_event_setting_both_actual_times() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-PASS", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-PASS'",
        )
        .execute(&pool)
        .await
        .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-PASS".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-pass-1', '0002', 'PASS', 'RDG', '2026-09-08T07:00:00Z', \
                     '2026-09-08T07:02:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-PASS", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops");

        assert_eq!(stops.len(), 2, "RDG + synthetic WAT terminus");
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(stops[0].last_event_type.as_deref(), Some("PASS"));
        let expected_instant: Option<DateTime<Utc>> = "2026-09-08T07:02:00Z".parse().ok();
        assert_eq!(stops[0].actual_arrival, expected_instant);
        assert_eq!(stops[0].actual_departure, expected_instant);
        assert_eq!(
            stops[0].delay_minutes,
            Some(2),
            "actual 2 minutes after this event's own planned time, via scheduled_departure \
             (the only scheduled time this fallback source has)"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-PASS'",
        )
        .execute(&pool)
        .await
        .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// §6(e): a movement event whose `loc_crs` matches NO stop in the base
    /// list is silently dropped -- no panic, no stray extra stop, and none
    /// of the real stops' `actual_*`/`delay_minutes` fields get corrupted
    /// by it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_silently_drops_an_event_whose_loc_crs_matches_no_stop \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_silently_drops_an_event_whose_loc_crs_matches_no_stop() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOMATCH", service_date)
                .await
                .expect("find_or_create_train");
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NOMATCH'",
        )
        .execute(&pool)
        .await
        .ok();

        crate::data::queries::upsert_schedule_destination_departures(
            &pool,
            &[crate::data::queries::ScheduleDestinationDeparturesRow {
                service_date,
                destination_crs: "WAT".to_string(),
                scheduled: "08:00:00".parse().unwrap(),
                day_offset: 0,
                train_uid: "TEST-JRN-NOMATCH".to_string(),
                origin_crs: "RDG".to_string(),
                true_origin_crs: Some("RDG".to_string()),
                destination_arrival: None,
                destination_arrival_day_offset: 0,
            }],
        )
        .await
        .expect("seed schedule_destination_departures");

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // 'ZZZ' is not RDG (the base stop) nor WAT (the synthetic
        // terminus) -- an unscheduled diversion location, or a
        // STANOX->CRS translation that doesn't line up with either
        // source's own CRS.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-nomatch-1', '0001', 'ARRIVAL', 'ZZZ', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:05:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(&pool, trains_id, "TEST-JRN-NOMATCH", service_date, None)
            .await
            .expect("build_journey_stops")
            .expect("Some stops");

        assert_eq!(
            stops.len(),
            2,
            "RDG + synthetic WAT terminus, no stray 'ZZZ' stop appended"
        );
        assert_eq!(stops[0].crs.as_deref(), Some("RDG"));
        assert_eq!(stops[0].actual_arrival, None);
        assert_eq!(stops[0].actual_departure, None);
        assert_eq!(stops[0].last_event_type, None);
        assert_eq!(stops[0].delay_minutes, None);
        assert_eq!(stops[1].crs.as_deref(), Some("WAT"));
        assert_eq!(stops[1].actual_arrival, None);
        assert_eq!(stops[1].actual_departure, None);
        assert_eq!(stops[1].last_event_type, None);
        assert_eq!(stops[1].delay_minutes, None);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JRN-NOMATCH'",
        )
        .execute(&pool)
        .await
        .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the live journey-page bug (2026-09-10, trains
    /// `L78923`/`C34231`): the real TRUST `TRAIN_MVT_ALL_TOC` feed was
    /// delivering `planned_timestamp`/`actual_timestamp` epoch millis that
    /// were BOTH consistently ~1 hour ahead of true UTC (an upstream feed
    /// issue -- confirmed against `received_at` -- outside this codebase;
    /// nothing in `trust-consumer::process::parse_epoch_millis` needed to
    /// change). Because the skew hits both of TRUST's own fields equally,
    /// diffing them against EACH OTHER (this test) cancels it out and
    /// yields the true delay, whereas diffing TRUST's `actual` against the
    /// CIF-schedule-derived `scheduled_arrival` (a completely separate,
    /// correctly-BST-converted pipeline that the skew never touched) mixes
    /// two independent bases and manufactures a bogus ~59 minute "late".
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_uses_events_own_planned_timestamp_not_cif_schedule \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_uses_events_own_planned_timestamp_not_cif_schedule() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-SKEW", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-SKEW-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-SKEW-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-SKEW-2".to_string(),
                    crs: "TSB".to_string(),
                    tiploc: "TEST-JRN-SKEW-MID".to_string(),
                    station_name: "TEST STATION B".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // CIF schedule (correctly BST-converted): booked arrival 11:23
        // London on 2026-09-08 (BST, UTC+1) -> 10:23:00Z.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-SKEW-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "09:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-SKEW-MID",
                "kind": "Terminate",
                "bookedArrival": "11:23:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // The live-investigated scenario: TRUST's own two fields on this
        // event (`planned_timestamp` and `actual_timestamp`) are BOTH ~1
        // hour ahead of true UTC, but only 1 minute apart from EACH OTHER
        // -- this train is genuinely running 1 minute early per TRUST's
        // own self-consistent numbers, even though neither TRUST
        // timestamp lines up at all with the correctly-converted CIF
        // schedule time (10:23:00Z).
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-skew-1', '0001', 'ARRIVAL', 'TSB', '2026-09-08T11:23:00Z', \
                     '2026-09-08T11:22:00Z', 'EARLY', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-SKEW",
            service_date,
            Some(&calling_points),
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSB"));
        assert_eq!(
            stops[1].scheduled_arrival,
            "2026-09-08T10:23:00Z".parse().ok(),
            "the DISPLAYED scheduled time is still the correctly-BST-converted CIF value \
             -- this fix only changes the delay-minutes arithmetic, not what's shown as \
             \"scheduled\""
        );
        assert_eq!(stops[1].actual_arrival, "2026-09-08T11:22:00Z".parse().ok());
        assert_eq!(
            stops[1].delay_minutes,
            Some(-1),
            "must diff TRUST's own actual_timestamp (11:22Z) against TRUST's own \
             planned_timestamp (11:23Z) on the SAME movement event row -> 1 minute early. \
             The old buggy code diffed TRUST's actual (11:22Z) against the CIF-derived \
             scheduled_arrival (10:23Z) instead -> Some(59), a bogus ~1 hour \"late\" caused \
             entirely by the upstream TRUST feed's timestamp skew against true UTC."
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-SKEW-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// The non-skewed, legitimate case: TRUST's own `planned_timestamp` for
    /// this event genuinely agrees with the correctly-converted CIF
    /// schedule time. This fix must not change the computed delay for this,
    /// the normal case -- asserts the same value old and new code both
    /// produce.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_unaffected_when_trust_and_cif_timestamps_agree \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_unaffected_when_trust_and_cif_timestamps_agree() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOSKEW", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOSKEW-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-NOSKEW-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOSKEW-2".to_string(),
                    crs: "TSC".to_string(),
                    tiploc: "TEST-JRN-NOSKEW-MID".to_string(),
                    station_name: "TEST STATION C".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        // Booked arrival 10:00 London on 2026-09-08 (BST) -> 09:00:00Z,
        // and TRUST's own planned_timestamp for the same event agrees
        // exactly -- no upstream skew present.
        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-NOSKEW-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "08:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-NOSKEW-MID",
                "kind": "Terminate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-noskew-1', '0001', 'ARRIVAL', 'TSC', '2026-09-08T09:00:00Z', \
                     '2026-09-08T09:06:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-NOSKEW",
            service_date,
            Some(&calling_points),
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSC"));
        assert_eq!(
            stops[1].delay_minutes,
            Some(6),
            "TRUST's own planned_timestamp (09:00Z) matches the CIF schedule (09:00Z) here, \
             so diffing against either basis gives the same, correct 6-minutes-late result"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-NOSKEW-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// A movement event that's missing its own `planned_timestamp` (some
    /// TRUST messages omit it) must NOT fall back to diffing against the
    /// CIF-derived `scheduled_arrival`/`scheduled_departure` -- that
    /// fallback is exactly the cross-basis bug this fix removes. Instead
    /// `delay_minutes` is `None` ("delay unknown"), matching this
    /// function's established "don't guess when data is incomplete"
    /// convention (the same convention behind the `_ => None` arm and the
    /// "silently drops" test above).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                build_journey_stops_delay_is_none_when_events_own_planned_timestamp_is_missing \
                -- --ignored --test-threads=1`"]
    async fn build_journey_stops_delay_is_none_when_events_own_planned_timestamp_is_missing() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-JRN-NOPLAN", service_date)
                .await
                .expect("find_or_create_train");

        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOPLAN-1".to_string(),
                    crs: "PAD".to_string(),
                    tiploc: "TEST-JRN-NOPLAN-ORIGIN".to_string(),
                    station_name: "PADDINGTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JRN-NOPLAN-2".to_string(),
                    crs: "TSD".to_string(),
                    tiploc: "TEST-JRN-NOPLAN-MID".to_string(),
                    station_name: "TEST STATION D".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let calling_points = serde_json::json!([
            {
                "tiploc": "TEST-JRN-NOPLAN-ORIGIN",
                "kind": "Origin",
                "bookedArrival": null,
                "bookedDeparture": "08:00:00",
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            },
            {
                "tiploc": "TEST-JRN-NOPLAN-MID",
                "kind": "Terminate",
                "bookedArrival": "10:00:00",
                "bookedDeparture": null,
                "isHalfMinuteArrival": false,
                "isHalfMinuteDeparture": false
            }
        ]);

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();

        // `planned_timestamp` explicitly NULL, `actual_timestamp` present.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'k-noplan-1', '0001', 'ARRIVAL', 'TSD', NULL, \
                     '2026-09-08T09:06:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let stops = build_journey_stops(
            &pool,
            trains_id,
            "TEST-JRN-NOPLAN",
            service_date,
            Some(&calling_points),
        )
        .await
        .expect("build_journey_stops")
        .expect("Some stops from calling_points_json");

        assert_eq!(stops.len(), 2);
        assert_eq!(stops[1].crs.as_deref(), Some("TSD"));
        assert_eq!(
            stops[1].actual_arrival,
            "2026-09-08T09:06:00Z".parse().ok(),
            "the actual time itself is still shown even without a planned_timestamp"
        );
        assert_eq!(
            stops[1].delay_minutes, None,
            "no planned_timestamp on this event -> delay unknown, NOT a fallback diff \
             against the CIF-derived scheduled_arrival (which would reintroduce the \
             cross-basis bug this fix removes)"
        );

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JRN-NOPLAN-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }
}
