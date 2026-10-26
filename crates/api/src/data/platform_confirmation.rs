//! When a stop's platform is served as confirmed (DS-180):
//! `platformBasis: "confirmed"`, with `platformChanged`/`plannedPlatform`
//! measured against the timetable's booked platform.
//!
//! A platform is confirmed only when tracking data ties THIS service to
//! it. Never a guess: when the evidence is missing, ambiguous or
//! contradicted, the stop keeps its board or booked platform.
//!
//! **Rule 1, the train's own report** ([`reported_platform`]). This train's
//! own TRUST arrival or departure at the call carries a platform, and was
//! reported AUTOMATICALLY: SMART derives it from the train describer berth
//! the train stepped into, so it is where the train physically is or was
//! (`trust_schema::schema::movement_reported_platform`; a manual report, a
//! pass and a blank platform never count). It covers the origin once the
//! train departs and every call it has arrived at or left. Within one
//! call the latest received report with a platform wins. It overrides a
//! live board: the train is there.
//!
//! **Rule 2, the unit at the origin before departure**
//! ([`previous_working_platform`]). Before the train has its own report
//! at its origin, the platform is confirmed when the train that forms it
//! (its previous working, `schedule_turnarounds`) has arrived there, all
//! of:
//!
//! - the previous working's latest automatic ARRIVAL with a platform is
//!   at this train's origin station, and is its latest report of any kind
//!   (it has not moved on since);
//! - that arrival is no earlier than the booked turnaround plus
//!   [`EARLY_ARRIVAL_SLACK`] before this train's booked departure, and no
//!   later than [`LATE_ARRIVAL_LIMIT`] after it (beyond that the unit was
//!   most likely swapped, as `predicted_start::SHOW_EXCESS_LIMIT` assumes);
//! - this train has not departed, and it is not more than
//!   [`LATE_ARRIVAL_LIMIT`] past its booked departure;
//! - a timetable-inferred link (`link = 'inferred'`, matched on the
//!   platform sequence) only CONFIRMS the booked platform: an arrival at
//!   any other platform confirms nothing, because the inference itself
//!   rested on the booked platform. A CIF association
//!   (`link = 'association'`, DS-152) ties the unit to the service
//!   outright, so its arrival platform is confirmed even when it differs
//!   from the booked one (a platform change);
//! - a live board row for the origin showing another platform, or showing
//!   the call cancelled, vetoes it.
//!
//! **Changed** (`apply_platform_basis`): a confirmed platform that differs
//! from the booked one (`platforms_differ`: case, spaces and a one-sided
//! sub-platform letter ignored) is `platformChanged`, with
//! `plannedPlatform` set to the booked platform. Without a booked
//! platform it compares against the board's first-seen platform.
//!
//! **Later sources.** Darwin's own platform confirmation (DS-152) slots in
//! as another `PlatformEvidenceSource`, set on
//! `JourneyStop::platform_evidence` before `apply_platform_basis` runs.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use ds_store::trains::types::{
    JourneyStop, PlatformEvidence, PlatformEvidenceSource, PlatformStatus,
};
use sqlx::PgPool;

use crate::data::journey::platforms_differ;
use crate::data::queries::MovementEventRow;

/// How much earlier than the booked turnaround the previous working may
/// arrive and still count.
pub const EARLY_ARRIVAL_SLACK: Duration = Duration::minutes(30);
/// How late, after this train's booked departure, the previous working
/// may arrive (and this train may still be waiting) and still count.
pub const LATE_ARRIVAL_LIMIT: Duration = Duration::minutes(30);

/// Rule 1: the platform this train's own reports give one call, from the
/// events of its visit (`members`, indices into `events`): the latest
/// received ARRIVAL or DEPARTURE with a platform. Only automatic reports
/// carry one (the consumers store nothing else).
pub fn reported_platform(
    events: &[MovementEventRow],
    members: &[usize],
) -> Option<PlatformEvidence> {
    members
        .iter()
        .copied()
        .filter(|&i| {
            matches!(
                events[i].event_type.as_deref(),
                Some("ARRIVAL" | "DEPARTURE")
            )
        })
        .filter_map(|i| {
            let platform = events[i].platform.as_deref()?.trim();
            (!platform.is_empty()).then(|| (i, platform.to_ascii_uppercase()))
        })
        .max_by_key(|(i, _)| *i)
        .map(|(_, platform)| PlatformEvidence {
            platform,
            source: PlatformEvidenceSource::TrainReport,
        })
}

/// What [`previous_working_arrivals`] knows about one train's previous
/// working: its link, and its latest platform-reporting arrival.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PreviousWorkingArrival {
    /// `inferred` or `association` (`schedule_turnarounds.link`).
    pub link: String,
    /// This train's booked origin departure.
    pub departs_at: DateTime<Utc>,
    pub booked_turnaround_minutes: i16,
    /// Where the previous working's latest platform-reporting ARRIVAL was.
    pub arrival_crs: String,
    pub arrival_platform: String,
    /// When it arrived (TRUST actual, else planned).
    pub arrived_at: DateTime<Utc>,
    /// The previous working's latest report of any kind.
    pub latest_report_at: Option<DateTime<Utc>>,
}

/// Rule 2 (see the module doc) for one train's origin stop: the platform
/// its previous working's arrival confirms, or `None`.
pub fn previous_working_platform(
    origin: &JourneyStop,
    evidence: &PreviousWorkingArrival,
    now: DateTime<Utc>,
) -> Option<String> {
    if origin.kind != Some(schedule_query::CallingPointKind::Origin)
        || origin.actual_departure.is_some()
        || origin.platform_evidence.is_some()
        || origin.platform_status == Some(PlatformStatus::Cancelled)
        || now > evidence.departs_at + LATE_ARRIVAL_LIMIT
    {
        return None;
    }
    let same_station = origin
        .crs
        .as_deref()
        .is_some_and(|crs| crs.eq_ignore_ascii_case(evidence.arrival_crs.trim()));
    let not_moved_since = evidence
        .latest_report_at
        .is_none_or(|latest| latest <= evidence.arrived_at);
    let earliest = evidence.departs_at
        - Duration::minutes(i64::from(evidence.booked_turnaround_minutes))
        - EARLY_ARRIVAL_SLACK;
    let in_window = evidence.arrived_at >= earliest
        && evidence.arrived_at <= evidence.departs_at + LATE_ARRIVAL_LIMIT;
    if !same_station || !not_moved_since || !in_window {
        return None;
    }
    let platform = evidence.arrival_platform.trim().to_ascii_uppercase();
    if platform.is_empty() {
        return None;
    }
    let booked = origin
        .booked_platform
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let allowed = match evidence.link.as_str() {
        "association" => true,
        // Inferred from the booked platform sequence: confirms it only.
        _ => booked.is_some_and(|booked| !platforms_differ(&platform, booked)),
    };
    let board_agrees = origin
        .platform
        .as_deref()
        .is_none_or(|board| !platforms_differ(&platform, board));
    (allowed && board_agrees).then_some(platform)
}

/// Sets the origin's `platform_evidence` from [`previous_working_platform`].
/// Run after the board overlay and before `apply_platform_basis`.
pub fn apply_previous_working_platform(
    stops: &mut [JourneyStop],
    evidence: Option<&PreviousWorkingArrival>,
    now: DateTime<Utc>,
) {
    let Some(evidence) = evidence else {
        return;
    };
    let Some(origin) = stops
        .iter_mut()
        .find(|stop| stop.kind == Some(schedule_query::CallingPointKind::Origin))
    else {
        return;
    };
    if let Some(platform) = previous_working_platform(origin, evidence, now) {
        origin.platform_evidence = Some(PlatformEvidence {
            platform,
            source: PlatformEvidenceSource::PreviousWorkingArrival,
        });
    }
}

/// For each `(uid, service date)`, its previous working's latest
/// platform-reporting arrival, when the train has a previous working that
/// DS has movement reports for. One query for the batch.
pub async fn previous_working_arrivals(
    pool: &PgPool,
    trains: &[(String, NaiveDate)],
) -> anyhow::Result<HashMap<(String, NaiveDate), PreviousWorkingArrival>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        uid: String,
        service_date: NaiveDate,
        #[sqlx(flatten)]
        arrival: PreviousWorkingArrival,
    }
    if trains.is_empty() {
        return Ok(HashMap::new());
    }
    let uids: Vec<&str> = trains.iter().map(|(uid, _)| uid.as_str()).collect();
    let dates: Vec<NaiveDate> = trains.iter().map(|(_, date)| *date).collect();
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT k.uid, k.service_date, t.link, t.departs_at, t.booked_turnaround_minutes, \
                a.arrival_crs, a.arrival_platform, a.arrived_at, l.latest_report_at \
         FROM (SELECT DISTINCT * FROM UNNEST($1::text[], $2::date[])) AS k(uid, service_date) \
         JOIN schedule_turnarounds t ON t.service_date = k.service_date AND t.uid = k.uid \
         JOIN trains p ON p.train_uid = t.prev_uid AND p.service_date = t.prev_service_date \
         JOIN LATERAL ( \
             SELECT UPPER(e.loc_crs) AS arrival_crs, e.platform AS arrival_platform, \
                    COALESCE(e.actual_timestamp, e.planned_timestamp) AS arrived_at \
             FROM train_movement_events e \
             WHERE e.trains_id = p.id AND e.event_type = 'ARRIVAL' \
               AND e.platform IS NOT NULL AND e.loc_crs IS NOT NULL \
               AND COALESCE(e.actual_timestamp, e.planned_timestamp) IS NOT NULL \
             ORDER BY e.received_at DESC, e.id DESC LIMIT 1 \
         ) a ON true \
         CROSS JOIN LATERAL ( \
             SELECT MAX(COALESCE(e.actual_timestamp, e.planned_timestamp)) AS latest_report_at \
             FROM train_movement_events e WHERE e.trains_id = p.id \
         ) l",
    )
    .bind(&uids)
    .bind(&dates)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ((row.uid, row.service_date), row.arrival))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_store::trains::types::RawCallingPoint;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().expect("valid instant")
    }

    /// A stop from the same JSON shape `trains.calling_points` stores.
    fn stop(crs: &str, kind: &str, platform: Option<&str>) -> JourneyStop {
        let raw: RawCallingPoint = serde_json::from_value(serde_json::json!({
            "tiploc": crs,
            "kind": kind,
            "bookedDeparture": "23:24:00",
            "platform": platform,
        }))
        .expect("raw calling point");
        let mut stop = JourneyStop::from_calling_point(
            &raw,
            Some(crs.to_string()),
            "2099-07-10".parse().expect("date"),
        );
        stop.crs = Some(crs.to_string());
        stop
    }

    fn event(crs: &str, event_type: &str, platform: Option<&str>) -> MovementEventRow {
        MovementEventRow {
            loc_crs: crs.to_string(),
            event_type: Some(event_type.to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            gbtt_timestamp: None,
            platform: platform.map(str::to_string),
        }
    }

    fn arrival(link: &str, platform: &str) -> PreviousWorkingArrival {
        PreviousWorkingArrival {
            link: link.to_string(),
            departs_at: at("2099-07-10T22:24:00Z"),
            booked_turnaround_minutes: 47,
            arrival_crs: "EUS".to_string(),
            arrival_platform: platform.to_string(),
            arrived_at: at("2099-07-10T21:40:00Z"),
            latest_report_at: Some(at("2099-07-10T21:40:00Z")),
        }
    }

    const BEFORE: &str = "2099-07-10T22:00:00Z";

    #[test]
    fn the_latest_platform_of_the_visit_wins_and_a_pass_or_blank_never_counts() {
        let events = vec![
            event("MKC", "ARRIVAL", Some("5")),
            event("MKC", "DEPARTURE", None),
            event("MKC", "PASS", Some("9")),
        ];
        assert_eq!(
            reported_platform(&events, &[1, 0, 2]),
            Some(PlatformEvidence {
                platform: "5".to_string(),
                source: PlatformEvidenceSource::TrainReport,
            }),
            "the departure has none, so the arrival's 5; the pass's 9 is ignored"
        );
        let events = vec![
            event("MKC", "ARRIVAL", Some("5")),
            event("MKC", "DEPARTURE", Some("6a")),
        ];
        assert_eq!(
            reported_platform(&events, &[0, 1]).map(|e| e.platform),
            Some("6A".to_string())
        );
        let events = vec![
            event("MKC", "ARRIVAL", Some(" ")),
            event("MKC", "DEPARTURE", None),
        ];
        assert_eq!(reported_platform(&events, &[0, 1]), None);
    }

    #[test]
    fn an_inferred_previous_working_confirms_the_booked_platform() {
        let origin = stop("EUS", "Origin", Some("5"));
        assert_eq!(
            previous_working_platform(&origin, &arrival("inferred", "5"), at(BEFORE)),
            Some("5".to_string())
        );
    }

    #[test]
    fn an_inferred_previous_working_at_another_platform_confirms_nothing() {
        let origin = stop("EUS", "Origin", Some("5"));
        assert_eq!(
            previous_working_platform(&origin, &arrival("inferred", "6"), at(BEFORE)),
            None
        );
        let unbooked = stop("EUS", "Origin", None);
        assert_eq!(
            previous_working_platform(&unbooked, &arrival("inferred", "6"), at(BEFORE)),
            None,
            "nothing booked to confirm"
        );
    }

    #[test]
    fn an_association_confirms_a_platform_change() {
        let origin = stop("EUS", "Origin", Some("5"));
        assert_eq!(
            previous_working_platform(&origin, &arrival("association", "6"), at(BEFORE)),
            Some("6".to_string())
        );
    }

    #[test]
    fn the_unit_must_still_be_there_at_this_station_in_the_window() {
        let origin = stop("EUS", "Origin", Some("5"));
        let moved = PreviousWorkingArrival {
            latest_report_at: Some(at("2099-07-10T21:45:00Z")),
            ..arrival("inferred", "5")
        };
        assert_eq!(previous_working_platform(&origin, &moved, at(BEFORE)), None);
        let elsewhere = PreviousWorkingArrival {
            arrival_crs: "WFJ".to_string(),
            ..arrival("inferred", "5")
        };
        assert_eq!(
            previous_working_platform(&origin, &elsewhere, at(BEFORE)),
            None
        );
        // 47 minute turnaround + 30 minutes slack before 22:24 is 20:57.
        let too_early = PreviousWorkingArrival {
            arrived_at: at("2099-07-10T20:56:00Z"),
            latest_report_at: Some(at("2099-07-10T20:56:00Z")),
            ..arrival("inferred", "5")
        };
        assert_eq!(
            previous_working_platform(&origin, &too_early, at(BEFORE)),
            None
        );
        let too_late = PreviousWorkingArrival {
            arrived_at: at("2099-07-10T22:55:00Z"),
            latest_report_at: Some(at("2099-07-10T22:55:00Z")),
            ..arrival("inferred", "5")
        };
        assert_eq!(
            previous_working_platform(&origin, &too_late, at("2099-07-10T22:50:00Z")),
            None
        );
        assert_eq!(
            previous_working_platform(
                &origin,
                &arrival("inferred", "5"),
                at("2099-07-10T22:55:00Z")
            ),
            None,
            "half an hour past the booked departure, the evidence has gone stale"
        );
    }

    #[test]
    fn the_trains_own_departure_a_cancellation_or_a_disagreeing_board_wins() {
        let mut departed = stop("EUS", "Origin", Some("5"));
        departed.actual_departure = Some(at("2099-07-10T22:24:00Z"));
        assert_eq!(
            previous_working_platform(&departed, &arrival("inferred", "5"), at(BEFORE)),
            None
        );
        let mut board = stop("EUS", "Origin", Some("5"));
        board.platform = Some("6".to_string());
        assert_eq!(
            previous_working_platform(&board, &arrival("association", "5"), at(BEFORE)),
            None
        );
        board.platform = Some("5".to_string());
        assert_eq!(
            previous_working_platform(&board, &arrival("association", "5"), at(BEFORE)),
            Some("5".to_string())
        );
        board.platform_status = Some(PlatformStatus::Cancelled);
        assert_eq!(
            previous_working_platform(&board, &arrival("association", "5"), at(BEFORE)),
            None
        );
        let intermediate = stop("EUS", "Intermediate", Some("5"));
        assert_eq!(
            previous_working_platform(&intermediate, &arrival("inferred", "5"), at(BEFORE)),
            None
        );
    }

    #[test]
    fn apply_previous_working_platform_marks_only_the_origin() {
        let mut stops = vec![
            stop("EUS", "Origin", Some("5")),
            stop("MKC", "Intermediate", Some("3")),
        ];
        apply_previous_working_platform(&mut stops, Some(&arrival("inferred", "5")), at(BEFORE));
        assert_eq!(
            stops[0].platform_evidence,
            Some(PlatformEvidence {
                platform: "5".to_string(),
                source: PlatformEvidenceSource::PreviousWorkingArrival,
            })
        );
        assert_eq!(stops[1].platform_evidence, None);
        apply_previous_working_platform(&mut stops[1..], None, at(BEFORE));
        assert_eq!(stops[1].platform_evidence, None);
    }
}

#[cfg(test)]
mod db_tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;
    use crate::data::journey::build_journey_stops;
    use ds_store::trains::types::PlatformBasis;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    const TRAIN: &str = "TPLATT";
    const PREVIOUS: &str = "TPLATP";
    const TIPLOC_PREFIX: &str = "TEST-PLATCONF";

    async fn clean(pool: &PgPool, service_date: NaiveDate) {
        for uid in [TRAIN, PREVIOUS] {
            sqlx::query("DELETE FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind(uid)
                .bind(service_date)
                .execute(pool)
                .await
                .expect("clean trains");
        }
        sqlx::query("DELETE FROM schedule_turnarounds WHERE uid = $1")
            .bind(TRAIN)
            .execute(pool)
            .await
            .expect("clean schedule_turnarounds");
        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE $1")
            .bind(format!("{TIPLOC_PREFIX}-%"))
            .execute(pool)
            .await
            .expect("clean stanox_crs");
    }

    async fn insert_event(
        pool: &PgPool,
        trains_id: i64,
        dedup_key: &str,
        (event_type, crs, at, platform): (&str, &str, DateTime<Utc>, Option<&str>),
    ) {
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body, platform) \
             VALUES ($1, $2, '0003', $3, $4, $5, $5, 'ON TIME', '{}'::jsonb, $6)",
        )
        .bind(trains_id)
        .bind(dedup_key)
        .bind(event_type)
        .bind(crs)
        .bind(at)
        .bind(platform)
        .execute(pool)
        .await
        .expect("seed train_movement_events");
    }

    /// DS-180 end to end, hand-written fixtures after the W33284 case: the
    /// train is booked from platform 5 at its origin and platform 3 at
    /// Milton Keynes Central. Its previous working has just arrived at the
    /// origin's platform 5 (automatic report, nothing since), and the
    /// train's own automatic arrival at Milton Keynes reported platform 5.
    /// (The two halves are independent; the origin has no departure
    /// report so that its pre-departure rule is exercised.)
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                platform_confirmation::db_tests -- --ignored --test-threads=1`"]
    async fn tracking_data_confirms_the_origin_and_a_changed_intermediate_platform() {
        let pool = connect().await;
        let now = Utc::now();
        let service_date = crate::routes::london_today();
        clean(&pool, service_date).await;

        let records: Vec<common::StanoxCrsRecord> = ["EUS", "MKC", "WVH"]
            .iter()
            .enumerate()
            .map(|(i, crs)| common::StanoxCrsRecord {
                stanox: format!("{TIPLOC_PREFIX}-{i}"),
                crs: (*crs).to_string(),
                tiploc: format!("{TIPLOC_PREFIX}-{crs}"),
                station_name: (*crs).to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            })
            .collect();
        crate::data::queries::upsert_stanox_crs(&pool, &records)
            .await
            .expect("seed stanox_crs");

        let train_id = crate::data::trains::find_or_create_train(&pool, TRAIN, service_date)
            .await
            .expect("train");
        let previous_id = crate::data::trains::find_or_create_train(&pool, PREVIOUS, service_date)
            .await
            .expect("previous working");

        let departs_at = now + Duration::minutes(10);
        sqlx::query(
            "INSERT INTO schedule_turnarounds \
                (service_date, uid, prev_service_date, prev_uid, booked_turnaround_minutes, \
                 link, origin_tiploc, platform, departs_at) \
             VALUES ($1, $2, $1, $3, 30, 'inferred', $4, '5', $5)",
        )
        .bind(service_date)
        .bind(TRAIN)
        .bind(PREVIOUS)
        .bind(format!("{TIPLOC_PREFIX}-EUS"))
        .bind(departs_at)
        .execute(&pool)
        .await
        .expect("seed schedule_turnarounds");

        let arrived = now - Duration::minutes(15);
        insert_event(
            &pool,
            previous_id,
            "prev-dep",
            (
                "DEPARTURE",
                "WVH",
                arrived - Duration::minutes(90),
                Some("1"),
            ),
        )
        .await;
        insert_event(
            &pool,
            previous_id,
            "prev-arr",
            ("ARRIVAL", "EUS", arrived, Some("5")),
        )
        .await;
        insert_event(
            &pool,
            train_id,
            "mkc-arr",
            ("ARRIVAL", "MKC", now - Duration::minutes(1), Some("5")),
        )
        .await;

        let calling_points = serde_json::json!([
            {"tiploc": format!("{TIPLOC_PREFIX}-EUS"), "kind": "Origin",
             "bookedDeparture": "23:24:00", "platform": "5"},
            {"tiploc": format!("{TIPLOC_PREFIX}-MKC"), "kind": "Intermediate",
             "bookedArrival": "00:13:00", "bookedDeparture": "00:15:00", "dayOffset": 1,
             "platform": "3"},
            {"tiploc": format!("{TIPLOC_PREFIX}-WVH"), "kind": "Terminate",
             "bookedArrival": "01:43:00", "dayOffset": 1, "platform": "1"},
        ]);
        let stops = build_journey_stops(
            &pool,
            train_id,
            TRAIN,
            service_date,
            Some(&calling_points),
            None,
            &[],
            None,
            None,
            None,
        )
        .await
        .expect("build_journey_stops")
        .expect("stops");

        assert_eq!(stops.len(), 3);
        let (origin, mkc, terminus) = (&stops[0], &stops[1], &stops[2]);
        assert_eq!(origin.crs.as_deref(), Some("EUS"));
        assert_eq!(origin.platform_basis, Some(PlatformBasis::Confirmed));
        assert_eq!(origin.platform.as_deref(), Some("5"));
        assert!(!origin.platform_changed);

        assert_eq!(mkc.crs.as_deref(), Some("MKC"));
        assert_eq!(mkc.platform_basis, Some(PlatformBasis::Confirmed));
        assert_eq!(mkc.platform.as_deref(), Some("5"));
        assert_eq!(mkc.planned_platform.as_deref(), Some("3"));
        assert!(mkc.platform_changed, "booked 3, arrived 5");

        assert_eq!(terminus.platform_basis, Some(PlatformBasis::Booked));
        assert_eq!(terminus.platform, None);

        // The previous working moving on (a later report) withdraws the
        // origin's confirmation.
        insert_event(
            &pool,
            previous_id,
            "prev-moved",
            ("DEPARTURE", "EUS", arrived + Duration::minutes(5), None),
        )
        .await;
        let stops = build_journey_stops(
            &pool,
            train_id,
            TRAIN,
            service_date,
            Some(&calling_points),
            None,
            &[],
            None,
            None,
            None,
        )
        .await
        .expect("build_journey_stops")
        .expect("stops");
        assert_eq!(stops[0].platform_basis, Some(PlatformBasis::Booked));
        assert_eq!(stops[1].platform_basis, Some(PlatformBasis::Confirmed));

        clean(&pool, service_date).await;
    }
}
