//! Platforms from the Darwin Push Port (DS-152; spec
//! docs/superpowers/specs/2026-10-21-pre-departure-prediction-design.md
//! section 6.2 item 4): every stop of a train Darwin knows, terminating
//! and set-down stops included, with Darwin's confirmed and suppressed
//! flags. Read from `darwin_calls` ([`ds_store::darwin::calls_for_trains`])
//! and laid over each stop after the board match, before
//! `journey::apply_platform_basis`.
//!
//! **Matching.** A stop matches the Darwin location with its TIPLOC and one
//! of its working times (arrival, departure, pass) to the minute, Europe/
//! London local, which is how Darwin itself tells a circular train's two
//! visits apart. Failing that, a TIPLOC the train visits once on each side
//! matches by itself (a schedule that lost a time, say).
//!
//! **What a match does.**
//!
//! - A platform Darwin suppresses from public display (`platsup`, or the
//!   station CIS's `cisPlatsup`) is not served: `platform_suppressed` is
//!   set, and `platform` keeps whatever a board showed (boards hide it too).
//! - Any other platform becomes the stop's `platform`, unless a board row
//!   polled after Darwin's row last changed shows a different one (the
//!   newer of two Darwin views wins). A confirmed one (`conf`) makes the
//!   basis `confirmed`.
//! - `planned_platform` falls back to Darwin's planned platform, so
//!   `platform_changed` compares against Darwin's plan when the timetable
//!   has no booked platform; with one, `apply_platform_basis` compares
//!   against the timetable's.
//! - A location Darwin cancelled serves its platform as `cancelled`.

use chrono::{DateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Europe::London;
use ds_store::darwin::DarwinCall;

use crate::data::journey::{JourneyStop, platforms_differ};
use ds_store::trains::types::{PlatformEvidence, PlatformEvidenceSource, PlatformStatus};

/// `at` as Europe/London `HH:MM`.
fn local_minute(at: DateTime<Utc>) -> NaiveTime {
    let local = at.with_timezone(&London).time();
    NaiveTime::from_hms_opt(local.hour(), local.minute(), 0).unwrap_or(local)
}

/// A Darwin working time (`HH:MM` or `HH:MM:SS`) to the minute; `None`
/// for `""`.
fn darwin_minute(raw: &str) -> Option<NaiveTime> {
    let time = NaiveTime::parse_from_str(raw, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(raw, "%H:%M"))
        .ok()?;
    NaiveTime::from_hms_opt(time.hour(), time.minute(), 0)
}

/// Whether `call` is `stop`'s visit by a shared working time.
fn same_visit(stop: &JourneyStop, call: &DarwinCall) -> bool {
    let timetable = &stop.timetable;
    let pairs = [
        (
            timetable
                .working_arrival
                .or(timetable.working_minute_arrival),
            call.wta.as_str(),
        ),
        (
            timetable
                .working_departure
                .or(timetable.working_minute_departure),
            call.wtd.as_str(),
        ),
        (timetable.working_pass, call.wtp.as_str()),
    ];
    pairs
        .iter()
        .any(|(ours, theirs)| match (ours, darwin_minute(theirs)) {
            (Some(ours), Some(theirs)) => local_minute(*ours) == theirs,
            _ => false,
        })
}

/// The index into `calls` of `stop`'s location, if one matches (see the
/// module docs). `stops` is the whole train, for the TIPLOC-only rule.
fn find_call(stops: &[JourneyStop], index: usize, calls: &[DarwinCall]) -> Option<usize> {
    let stop = &stops[index];
    let tiploc = stop.tiploc.as_deref()?;
    let at_tiploc: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| call.tiploc == tiploc)
        .map(|(i, _)| i)
        .collect();
    if let Some(&i) = at_tiploc.iter().find(|&&i| same_visit(stop, &calls[i])) {
        return Some(i);
    }
    let visits = stops
        .iter()
        .filter(|s| s.tiploc.as_deref() == Some(tiploc))
        .count();
    (visits == 1 && at_tiploc.len() == 1).then(|| at_tiploc[0])
}

/// Lays `calls` (one train's Push Port locations) over its `stops`. See
/// the module docs.
pub fn apply_pushport_platforms(stops: &mut [JourneyStop], calls: &[DarwinCall]) {
    if calls.is_empty() {
        return;
    }
    let matches: Vec<Option<usize>> = (0..stops.len())
        .map(|i| find_call(stops, i, calls))
        .collect();
    for (stop, call) in stops.iter_mut().zip(matches) {
        let Some(call) = call.map(|i| &calls[i]) else {
            continue;
        };
        if stop.planned_platform.is_none() {
            stop.planned_platform.clone_from(&call.planned_platform);
        }
        let Some(number) = call.platform.as_deref() else {
            continue;
        };
        if call.platform_hidden() {
            stop.platform_suppressed = true;
            // Darwin hides the number from the public, so the forming
            // unit's arrival platform (a prediction) is not shown either.
            // The train's own report at the platform has happened, and
            // stays.
            if stop
                .platform_evidence
                .as_ref()
                .is_some_and(|e| e.source == PlatformEvidenceSource::PreviousWorkingArrival)
            {
                stop.platform_evidence = None;
            }
            continue;
        }
        let board_is_newer = stop.board.as_ref().is_some_and(|board| {
            board.observed_at > call.updated_at
                && stop
                    .platform
                    .as_deref()
                    .is_some_and(|shown| platforms_differ(shown, number))
        });
        if board_is_newer {
            continue;
        }
        stop.platform = Some(number.to_owned());
        // The operator's confirmation is evidence like TRUST's (DS-180):
        // `apply_platform_basis` turns it into `confirmed`. The train's own
        // automatic report outranks it; it outranks the forming unit's
        // arrival. A cancelled call is not confirmed.
        let train_reported = stop
            .platform_evidence
            .as_ref()
            .is_some_and(|e| e.source == PlatformEvidenceSource::TrainReport);
        if call.platform_confirmed && !call.is_cancelled && !train_reported {
            stop.platform_evidence = Some(PlatformEvidence {
                platform: number.trim().to_ascii_uppercase(),
                source: PlatformEvidenceSource::DarwinConfirmed,
            });
        }
        stop.platform_status = Some(if call.is_cancelled {
            PlatformStatus::Cancelled
        } else {
            PlatformStatus::Active
        });
        stop.platform_changed = stop
            .planned_platform
            .as_deref()
            .is_some_and(|planned| platforms_differ(number, planned));
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use ds_store::trains::types::{PlatformBasis, StopBoard};
    use schedule_query::CallingPointKind;

    use super::*;
    use crate::data::journey::apply_platform_basis;
    use crate::data::journey::test_support::stop;

    /// 10:12 Europe/London on a summer date is 09:12 UTC.
    const DEPARTS: &str = "2099-07-10T09:12:00Z";

    fn call(tiploc: &str, wtd: &str, platform: Option<&str>) -> DarwinCall {
        DarwinCall {
            tiploc: tiploc.to_string(),
            wta: String::new(),
            wtd: wtd.to_string(),
            wtp: String::new(),
            planned_platform: Some("3".to_string()),
            is_cancelled: false,
            platform: platform.map(str::to_string),
            platform_confirmed: false,
            platform_suppressed: false,
            platform_cis_suppressed: false,
            platform_basis: None,
            arrival_et: None,
            departure_et: None,
            updated_at: DEPARTS.parse().expect("instant"),
        }
    }

    fn york() -> JourneyStop {
        stop("YRK", "YORK", CallingPointKind::Origin, Some(DEPARTS))
    }

    #[test]
    fn a_confirmed_platform_is_served_as_confirmed() {
        let mut stops = vec![york()];
        let mut confirmed = call("YORK", "10:12", Some("5"));
        confirmed.platform_confirmed = true;
        apply_pushport_platforms(&mut stops, &[confirmed]);
        apply_platform_basis(&mut stops);
        assert_eq!(stops[0].platform.as_deref(), Some("5"));
        assert_eq!(stops[0].platform_basis, Some(PlatformBasis::Confirmed));
        assert_eq!(stops[0].display_platform.as_deref(), Some("5"));
        // Against Darwin's planned 3, with no timetable platform.
        assert!(stops[0].platform_changed);
        let json = serde_json::to_value(&stops[0]).expect("serializes");
        assert_eq!(json["platformBasis"], "confirmed");
        assert_eq!(json["platformSuppressed"], false);
        assert!(json.get("platformConfirmed").is_none());
    }

    #[test]
    fn the_timetable_booked_platform_wins_the_change_comparison() {
        let mut stops = vec![york()];
        stops[0].booked_platform = Some("5".to_string());
        apply_pushport_platforms(&mut stops, &[call("YORK", "10:12:30", Some("5"))]);
        apply_platform_basis(&mut stops);
        assert!(!stops[0].platform_changed, "5 is the booked platform");
        assert_eq!(stops[0].platform_basis, Some(PlatformBasis::Board));
    }

    #[test]
    fn a_suppressed_platform_is_not_served() {
        let mut stops = vec![york()];
        stops[0].booked_platform = Some("3".to_string());
        let mut hidden = call("YORK", "10:12", Some("7"));
        hidden.platform_cis_suppressed = true;
        apply_pushport_platforms(&mut stops, &[hidden]);
        apply_platform_basis(&mut stops);
        assert_eq!(stops[0].platform, None);
        assert!(stops[0].platform_suppressed);
        assert_eq!(stops[0].platform_basis, Some(PlatformBasis::Booked));
        assert_eq!(stops[0].display_platform.as_deref(), Some("3"));
    }

    #[test]
    fn a_terminating_stop_gets_its_platform_by_arrival_time() {
        let mut terminus = stop("SCA", "SCARBRO", CallingPointKind::Terminate, None);
        terminus.timetable.working_arrival = Some("2099-07-10T10:15:00Z".parse().expect("t"));
        let mut arrival = call("SCARBRO", "", Some("1"));
        arrival.wta = "11:15".to_string();
        let mut stops = vec![york(), terminus];
        apply_pushport_platforms(&mut stops, &[call("YORK", "10:12", Some("5")), arrival]);
        assert_eq!(stops[1].platform.as_deref(), Some("1"));
    }

    #[test]
    fn a_circular_trains_two_visits_are_told_apart_by_time() {
        let mut second = stop("YRK", "YORK", CallingPointKind::Terminate, None);
        second.timetable.working_arrival = Some("2099-07-10T11:00:00Z".parse().expect("t"));
        let mut stops = vec![york(), second];
        let mut back = call("YORK", "", Some("9"));
        back.wta = "12:00".to_string();
        apply_pushport_platforms(&mut stops, &[back, call("YORK", "10:12", Some("5"))]);
        assert_eq!(stops[0].platform.as_deref(), Some("5"));
        assert_eq!(stops[1].platform.as_deref(), Some("9"));
    }

    #[test]
    fn a_newer_board_row_that_differs_wins_over_an_older_darwin_row() {
        let mut stops = vec![york()];
        stops[0].platform = Some("6".to_string());
        stops[0].board = Some(StopBoard {
            delay_reason: None,
            cancel_reason: None,
            is_cancelled: false,
            delay_minutes: None,
            estimated: "On time".to_string(),
            observed_at: DEPARTS.parse::<DateTime<Utc>>().expect("t") + Duration::minutes(5),
        });
        apply_pushport_platforms(&mut stops, &[call("YORK", "10:12", Some("5"))]);
        assert_eq!(stops[0].platform.as_deref(), Some("6"));
    }

    #[test]
    fn a_cancelled_location_serves_its_platform_as_cancelled() {
        let mut stops = vec![york()];
        let mut cancelled = call("YORK", "10:12", Some("5"));
        cancelled.is_cancelled = true;
        apply_pushport_platforms(&mut stops, &[cancelled]);
        assert_eq!(stops[0].platform_status, Some(PlatformStatus::Cancelled));
    }

    fn evidence(platform: &str, source: PlatformEvidenceSource) -> Option<PlatformEvidence> {
        Some(PlatformEvidence {
            platform: platform.to_string(),
            source,
        })
    }

    #[test]
    fn the_trains_own_report_outranks_darwin_and_darwin_the_forming_unit() {
        let mut confirmed = call("YORK", "10:12", Some("5"));
        confirmed.platform_confirmed = true;

        let mut reported = vec![york()];
        reported[0].platform_evidence = evidence("6", PlatformEvidenceSource::TrainReport);
        apply_pushport_platforms(&mut reported, std::slice::from_ref(&confirmed));
        apply_platform_basis(&mut reported);
        assert_eq!(reported[0].platform.as_deref(), Some("6"));

        let mut forming = vec![york()];
        forming[0].platform_evidence =
            evidence("6", PlatformEvidenceSource::PreviousWorkingArrival);
        apply_pushport_platforms(&mut forming, std::slice::from_ref(&confirmed));
        apply_platform_basis(&mut forming);
        assert_eq!(forming[0].platform.as_deref(), Some("5"));
        assert_eq!(forming[0].platform_basis, Some(PlatformBasis::Confirmed));
    }

    #[test]
    fn a_suppressed_platform_hides_the_forming_units_but_not_the_trains_own() {
        let mut hidden = call("YORK", "10:12", Some("5"));
        hidden.platform_suppressed = true;

        let mut forming = vec![york()];
        forming[0].platform_evidence =
            evidence("6", PlatformEvidenceSource::PreviousWorkingArrival);
        apply_pushport_platforms(&mut forming, std::slice::from_ref(&hidden));
        apply_platform_basis(&mut forming);
        assert_eq!(forming[0].platform, None);
        assert!(forming[0].platform_suppressed);

        let mut reported = vec![york()];
        reported[0].platform_evidence = evidence("6", PlatformEvidenceSource::TrainReport);
        apply_pushport_platforms(&mut reported, std::slice::from_ref(&hidden));
        apply_platform_basis(&mut reported);
        assert_eq!(reported[0].platform.as_deref(), Some("6"));
    }

    #[test]
    fn no_match_changes_nothing() {
        let mut stops = vec![york()];
        apply_pushport_platforms(&mut stops, &[call("LEEDS", "10:12", Some("5"))]);
        assert_eq!(stops[0].platform, None);
        assert_eq!(stops[0].planned_platform, None);
    }
}

#[cfg(test)]
mod db_tests {
    use chrono::NaiveDate;
    use ds_store::trains::types::PlatformBasis;

    use crate::data::journey::build_journey_stops;

    /// End to end (DS-152): Push Port rows for a train reach its stops'
    /// `platform`/`platformBasis` through the same build every train route
    /// uses: the origin's confirmed platform, and the terminus's platform,
    /// which no departure board has.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                pushport_platform::db_tests -- --ignored --test-threads=1`"]
    async fn push_port_platforms_reach_the_journey_stops() {
        let pool = ds_store::test_support::connect().await;
        let service_date = NaiveDate::from_ymd_opt(2099, 7, 12).expect("date");
        let cleanup = || async {
            for sql in [
                "DELETE FROM darwin_calls WHERE rid = '209907128Z70001'",
                "DELETE FROM darwin_services WHERE rid = '209907128Z70001'",
                "DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-DPP-%'",
                "DELETE FROM trains WHERE train_uid = 'Z70001' AND service_date = '2099-07-12'",
            ] {
                sqlx::query(sql).execute(&pool).await.expect("cleanup");
            }
        };
        cleanup().await;
        let trains_id = crate::data::trains::find_or_create_train(&pool, "Z70001", service_date)
            .await
            .expect("train");
        crate::data::queries::upsert_stanox_crs(
            &pool,
            &[
                ("TEST-DPP-1", "ZQA", "TEST-DPP-A"),
                ("TEST-DPP-2", "ZQB", "TEST-DPP-B"),
            ]
            .map(|(stanox, crs, tiploc)| common::StanoxCrsRecord {
                stanox: stanox.to_string(),
                crs: crs.to_string(),
                tiploc: tiploc.to_string(),
                station_name: crs.to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            }),
        )
        .await
        .expect("stanox_crs");
        sqlx::query(
            "INSERT INTO darwin_services (rid, uid, ssd, message_ts) \
             VALUES ('209907128Z70001', 'Z70001', '2099-07-12', now())",
        )
        .execute(&pool)
        .await
        .expect("service");
        sqlx::query(
            "INSERT INTO darwin_calls (rid, tiploc, wta, wtd, ssd, planned_platform, platform, \
             platform_confirmed) VALUES \
             ('209907128Z70001', 'TEST-DPP-A', '', '09:00', '2099-07-12', '1', '2', TRUE), \
             ('209907128Z70001', 'TEST-DPP-B', '10:30', '', '2099-07-12', '4', '4', FALSE)",
        )
        .execute(&pool)
        .await
        .expect("calls");

        let calling_points = serde_json::json!([
            { "tiploc": "TEST-DPP-A", "kind": "Origin", "bookedArrival": null,
              "bookedDeparture": "09:00:00", "isHalfMinuteArrival": false,
              "isHalfMinuteDeparture": false, "platform": "1" },
            { "tiploc": "TEST-DPP-B", "kind": "Terminate", "bookedArrival": "10:30:00",
              "bookedDeparture": null, "isHalfMinuteArrival": false,
              "isHalfMinuteDeparture": false }
        ]);
        let stops = build_journey_stops(
            &pool,
            trains_id,
            "Z70001",
            service_date,
            Some(&calling_points),
            None,
            &[],
            None,
            None,
            None,
        )
        .await
        .expect("build")
        .expect("stops");

        assert_eq!(stops[0].platform.as_deref(), Some("2"));
        assert_eq!(stops[0].platform_basis, Some(PlatformBasis::Confirmed));
        assert!(stops[0].platform_changed, "booked 1, confirmed 2");
        assert_eq!(stops[1].platform.as_deref(), Some("4"));
        assert_eq!(stops[1].platform_basis, Some(PlatformBasis::Board));
        assert!(!stops[1].platform_changed);
        cleanup().await;
    }
}
