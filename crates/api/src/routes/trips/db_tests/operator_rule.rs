//! Operator rules in the planner (operator filtering spec 2026-10-31, P0):
//! connections take their operator from `schedule_services`, buses
//! included, and a journey replan's "same operator" search
//! (`plan_for_journey` with `OperatorRule::only(..).cif_trains_only()`)
//! rides that operator's trains and buses and nothing else.
//!
//! Three direct services ZOA (TOPRA) to ZOB (TOPRB) on a synthetic day:
//! TOP001 Avanti 10:00-10:30 (the fastest), TOP002 an LNER bus
//! 10:05-10:50, TOP003 an LNER train 10:10-11:00. TOP002 and TOP003 have
//! no `schedule_destination_departures` row at all, as for the buses and
//! ferries that table misses.

use super::*;

const DATE: &str = "2099-06-17";

fn date() -> NaiveDate {
    let date: NaiveDate = DATE.parse().expect("date");
    crate::test_support::assert_synthetic_date(date);
    date
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(sql).execute(pool).await.expect(sql);
}

async fn cleanup(pool: &PgPool) {
    for sql in [
        format!("DELETE FROM schedule_calling_points_full WHERE service_date = '{DATE}'"),
        format!("DELETE FROM schedule_services WHERE service_date = '{DATE}'"),
        "DELETE FROM stanox_crs WHERE stanox LIKE 'TESTOPRULE-%'".to_string(),
    ] {
        sqlx::query(&sql).execute(pool).await.ok();
    }
}

async fn seed(pool: &PgPool) {
    cleanup(pool).await;
    exec(
        pool,
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) VALUES \
         ('TESTOPRULE-A', 'ZOA', 'TOPRA', 'Test operator A', 1), \
         ('TESTOPRULE-B', 'ZOB', 'TOPRB', 'Test operator B', 1) \
         ON CONFLICT (stanox) DO NOTHING",
    )
    .await;
    for (uid, dep, arr, mode, operator) in [
        ("TOP001", "10:00", "10:30", "train", "VT"),
        ("TOP002", "10:05", "10:50", "bus", "GR"),
        ("TOP003", "10:10", "11:00", "train", "GR"),
    ] {
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_calling_points_full \
                     (service_date, uid, seq, tiploc, kind, booked_departure, public_departure, \
                      booked_arrival, public_arrival, day_offset, can_board, can_alight) VALUES \
                 ('{DATE}', '{uid}', 1, 'TOPRA', 'origin', '{dep}', '{dep}', NULL, NULL, 0, TRUE, TRUE), \
                 ('{DATE}', '{uid}', 2, 'TOPRB', 'terminate', NULL, NULL, '{arr}', '{arr}', 0, TRUE, TRUE)"
            ),
        )
        .await;
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
                 VALUES ('{DATE}', '{uid}', '{mode}', 'P', '{operator}')"
            ),
        )
        .await;
    }
}

async fn first_uids(app: &App, rule: Option<trip_planner::OperatorRule>) -> Vec<String> {
    let itineraries = plan_for_journey(
        app,
        &ReplanQuery {
            origin: "ZOA".to_string(),
            destination: "ZOB".to_string(),
            date: date(),
            depart_after_min: 9 * 60 + 50,
            results: "options",
            max_changes: 2,
            operator_rule: rule,
        },
        ReplanSlot::Request,
    )
    .await
    .expect("plan")
    .expect("a timetable for the date");
    itineraries
        .iter()
        .filter_map(|itinerary| match itinerary.legs.first() {
            Some(trip_planning_itinerary::PlannedLeg::Train { train_uid, .. }) => {
                Some(train_uid.clone())
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn the_same_operator_search_rides_that_operators_trains_and_buses_only() {
    let pool = connect().await;
    seed(&pool).await;
    let app = test_app(pool.clone());

    // Unrestricted: Avanti's train is fastest.
    assert_eq!(first_uids(&app, None).await, ["TOP001"]);

    // LNER only: its bus (no `schedule_destination_departures` row) is the
    // fastest of its services; Avanti's train is never offered.
    let lner = schedule_query::OperatorMask::of(schedule_query::OperatorId::from_atoc("GR"));
    let uids = first_uids(
        &app,
        Some(trip_planner::OperatorRule::only(lner.clone()).cif_trains_only()),
    )
    .await;
    assert_eq!(uids, ["TOP002"]);

    // Avoiding LNER leaves Avanti.
    let uids = first_uids(&app, Some(trip_planner::OperatorRule::avoid(lner))).await;
    assert_eq!(uids, ["TOP001"]);

    cleanup(&pool).await;
}
