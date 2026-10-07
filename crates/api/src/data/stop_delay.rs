//! A train's `delayMinutes` as a passenger sees it: measured against the
//! PUBLIC timetable at the passenger's own stop (design doc §9 decision 2).
//!
//! `train_current_state.delay_minutes` is TRUST's running delay, from the
//! train's latest report anywhere (a pass included), against the working
//! timetable. It stays internal: it is the input to the forecasts here and
//! to the per-stop estimates (`journey::apply_delay_estimates`). Every route
//! that serves a train-level `delayMinutes` replaces it through
//! [`apply_public_delays`] with:
//!
//! * the delay at the passenger's own stop (where they get off: a tracked
//!   train's pin destination, a journey leg's destination), measured once
//!   the train has reported there and forecast before then
//!   (`delayProvisional: true`); or
//! * with no stop of their own (the public train page, a line's trains),
//!   the delay at the latest call the train reported at.
//!
//! See `common::public_delay` for the arithmetic and the fallbacks, and
//! `delayBasis` for which baseline was used.

// Moved to ds_store::trains::stop_delay (ingest architecture plan 1A.4)
pub use ds_store::trains::stop_delay::{
    DelayBasis, PublicDelayFields, StopDelay, StopDelayTarget, apply_public_delays, split,
    stop_delays, target,
};

// The DB test stays here until ingest architecture plan unit F: it seeds
// through `queries::upsert_stanox_crs` and
// `queries::upsert_schedule_calling_points_full`, which other 1A tasks move.
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .expect("connect to postgres")
    }

    /// The shared read end to end, on the design doc's Avanti 9G44 shape:
    /// Watford Jn (public departure 20:31) to Milton Keynes (working 20:50H,
    /// public 20:51). 2026-10-02 is BST, so 20:31 local is 19:31Z.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                stop_delays_measure_and_forecast_on_public_times -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "one end-to-end fixture: its seed rows are most of the length"
    )]
    async fn stop_delays_measure_and_forecast_on_public_times() {
        use crate::data::queries;
        let pool = connect().await;
        let date: chrono::NaiveDate = "2026-10-02".parse().unwrap();
        let uid = "TEST-SDLY";
        let trains_id = crate::data::trains::find_or_create_train(&pool, uid, date)
            .await
            .expect("find_or_create_train");
        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .expect("clear events");
        let records: Vec<common::StanoxCrsRecord> = ["EUS", "WFJ", "MKC"]
            .iter()
            .enumerate()
            .map(|(i, crs)| common::StanoxCrsRecord {
                stanox: format!("{uid}-{i}"),
                crs: (*crs).to_string(),
                tiploc: format!("{uid}-{crs}"),
                station_name: (*crs).to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            })
            .collect();
        queries::upsert_stanox_crs(&pool, &records)
            .await
            .expect("seed stanox_crs");
        let t = |s: &str| Some(s.parse::<chrono::NaiveTime>().unwrap());
        let row = |seq: i16, crs: &str, kind: &str| queries::ScheduleCallingPointsFullRow {
            service_date: date,
            uid: uid.to_string(),
            seq,
            tiploc: format!("{uid}-{crs}"),
            kind: kind.to_string(),
            day_offset: 0,
            ..Default::default()
        };
        let rows = vec![
            queries::ScheduleCallingPointsFullRow {
                booked_departure: t("20:16:00"),
                public_departure: t("20:16:00"),
                working_departure: t("20:16:00"),
                ..row(0, "EUS", "origin")
            },
            queries::ScheduleCallingPointsFullRow {
                booked_arrival: t("20:29:00"),
                booked_departure: t("20:31:00"),
                public_departure: t("20:31:00"),
                working_arrival: t("20:29:00"),
                working_departure: t("20:31:00"),
                ..row(1, "WFJ", "intermediate")
            },
            queries::ScheduleCallingPointsFullRow {
                booked_arrival: t("20:50:00"),
                public_arrival: t("20:51:00"),
                working_arrival: t("20:50:30"),
                ..row(2, "MKC", "terminate")
            },
        ];
        queries::upsert_schedule_calling_points_full(&pool, &rows)
            .await
            .expect("seed schedule_calling_points_full");
        // Departed Watford 4 late against TRUST's own public time.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, gbtt_timestamp, variation_status, raw_body) \
             VALUES ($1, 'sdly-1', '0003', 'DEPARTURE', 'WFJ', '2026-10-02T19:31:00Z', \
                     '2026-10-02T19:35:00Z', '2026-10-02T19:31:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed departure");

        let target = |stop: Option<&str>| StopDelayTarget {
            trains_id,
            train_uid: uid.to_string(),
            service_date: date,
            stop_crs: stop.map(str::to_string),
            working_delay_minutes: Some(4),
        };
        let delays = stop_delays(&pool, &[target(Some("MKC")), target(None)])
            .await
            .expect("stop_delays");
        assert_eq!(
            delays,
            vec![
                // Forecast: 20:50:30 + 4 = 20:54:30 against 20:51.
                Some(StopDelay {
                    minutes: 3,
                    basis: DelayBasis::PublicSchedule,
                    provisional: true
                }),
                // No stop of the user's own: the latest call, Watford.
                Some(StopDelay {
                    minutes: 4,
                    basis: DelayBasis::Public,
                    provisional: false
                }),
            ]
        );

        // Arrived at Milton Keynes, matched from the backlog (no gbtt): the
        // public arrival is TRUST's planned time plus the schedule's 30 s.
        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body) \
             VALUES ($1, 'sdly-2', '0003', 'ARRIVAL', 'MKC', '2026-10-02T19:50:30Z', \
                     '2026-10-02T19:56:00Z', 'LATE', '{}'::jsonb)",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed arrival");
        let delays = stop_delays(&pool, &[target(Some("mkc"))])
            .await
            .expect("stop_delays");
        assert_eq!(
            delays,
            vec![Some(StopDelay {
                minutes: 5,
                basis: DelayBasis::PublicSchedule,
                provisional: false
            })]
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
            .bind(uid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-SDLY-%'")
            .execute(&pool)
            .await
            .ok();
    }
}
