//! `/Trips/plan?operator=` and `?avoidOperator=` (operator filtering spec
//! 2026-10-31, P1) on a synthetic day: Avanti ZFA (TOFA) to ZFB (TOFB)
//! 10:00-10:30; the Tube ZFB to ZFC (TOFC), 10 minutes; Southern ZFC to
//! ZFD (TOFD) at 11:00 and 11:40. ZFB and ZFC are about 1.5 km apart, a
//! 25-minute estimated walk once both have positions in `stations`.

use super::*;

const DATE: &str = "2099-06-18";

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(sql).execute(pool).await.expect(sql);
}

async fn cleanup(pool: &PgPool) {
    for sql in [
        format!("DELETE FROM schedule_calling_points_full WHERE service_date = '{DATE}'"),
        format!("DELETE FROM schedule_services WHERE service_date = '{DATE}'"),
        "DELETE FROM stanox_crs WHERE stanox LIKE 'TESTOPF-%'".to_string(),
        "DELETE FROM fixed_links WHERE from_crs IN ('ZFB', 'ZFC')".to_string(),
        "DELETE FROM stations WHERE crs IN ('ZFB', 'ZFC') AND name LIKE 'Test operator %'"
            .to_string(),
    ] {
        sqlx::query(&sql).execute(pool).await.ok();
    }
}

/// The day, with ZFB and ZFC positioned (a stand-in walk) when `walkable`.
async fn seed(pool: &PgPool, walkable: bool) {
    cleanup(pool).await;
    crate::test_support::assert_synthetic_date(DATE.parse().unwrap());
    exec(
        pool,
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, \
             change_time_minutes) VALUES \
         ('TESTOPF-A', 'ZFA', 'TOFA', 'Test operator A', 1, 5), \
         ('TESTOPF-B', 'ZFB', 'TOFB', 'Test operator B', 1, 5), \
         ('TESTOPF-C', 'ZFC', 'TOFC', 'Test operator C', 1, 5), \
         ('TESTOPF-D', 'ZFD', 'TOFD', 'Test operator D', 1, 5) \
         ON CONFLICT (stanox) DO NOTHING",
    )
    .await;
    exec(
        pool,
        "INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, \
             days_mask, source_sequence) \
         VALUES ('TUBE', 'ZFB', 'ZFC', 10, '0000', '2359', '1111111', 1)",
    )
    .await;
    if walkable {
        exec(
            pool,
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES \
             ('ZFB', 'Test operator B', 57.0, 2.0), ('ZFC', 'Test operator C', 57.0135, 2.0) \
             ON CONFLICT (crs) DO NOTHING",
        )
        .await;
    }
    for (uid, operator, from, to, dep, arr) in [
        ("TOF001", "VT", "TOFA", "TOFB", "10:00", "10:30"),
        ("TOF002", "SN", "TOFC", "TOFD", "11:00", "11:30"),
        ("TOF003", "SN", "TOFC", "TOFD", "11:40", "12:10"),
    ] {
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_calling_points_full \
                     (service_date, uid, seq, tiploc, kind, booked_departure, public_departure, \
                      booked_arrival, public_arrival, day_offset, can_board, can_alight) VALUES \
                 ('{DATE}', '{uid}', 1, '{from}', 'origin', '{dep}', '{dep}', NULL, NULL, 0, TRUE, TRUE), \
                 ('{DATE}', '{uid}', 2, '{to}', 'terminate', NULL, NULL, '{arr}', '{arr}', 0, TRUE, TRUE)"
            ),
        )
        .await;
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
                 VALUES ('{DATE}', '{uid}', 'train', 'P', '{operator}')"
            ),
        )
        .await;
    }
    // GR and LT are in the timetable (schedules with no calls in the
    // graph), so known codes (the test database has no `tocs`). GR runs
    // nothing the planner can ride; LT runs the Tube link.
    exec(
        pool,
        &format!(
            "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
             VALUES ('{DATE}', 'TOF009', 'train', 'P', 'GR'), \
                    ('{DATE}', 'TOF010', 'train', 'P', 'LT')"
        ),
    )
    .await;
}

async fn plan(pool: &PgPool, query: &str) -> (StatusCode, Value) {
    get(
        test_router(test_app(pool.clone())),
        format!("/Trips/plan?origin=ZFA&destination=ZFD&date={DATE}&departAfter=09:50{query}"),
    )
    .await
}

/// Train UIDs and `MODE minutes` per leg of the first itinerary.
fn legs(body: &Value) -> Vec<String> {
    body["segments"][0]["itineraries"][0]["legs"]
        .as_array()
        .map(|legs| {
            legs.iter()
                .map(|leg| match leg["kind"].as_str() {
                    Some("train") => leg["trainUid"].as_str().unwrap_or_default().to_string(),
                    _ => format!(
                        "{} {}",
                        leg["mode"].as_str().unwrap_or_default(),
                        leg["minutes"]
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn operator_filters_ride_every_way_of_an_operator_or_none() {
    let pool = connect().await;
    seed(&pool, true).await;

    // No filter: Avanti, the Tube (run by LT), Southern at 11:00.
    let (status, body) = plan(&pool, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(legs(&body), ["TOF001", "TUBE 10", "TOF002"], "{body}");
    let tube = &body["segments"][0]["itineraries"][0]["legs"][1];
    assert_eq!(tube["operator"], "LT", "{tube}");
    assert!(tube.get("estimatedWalk").is_none(), "{tube}");
    assert_eq!(body["operator"], serde_json::json!([]));
    assert_eq!(body["avoidOperator"], serde_json::json!([]));
    assert_eq!(body["operatorGroups"], serde_json::json!({}));

    // Only Avanti and Southern: no Tube, so the estimated walk (26
    // minutes) and the 11:40.
    let (status, body) = plan(&pool, "&operator=vt,SN").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(legs(&body), ["TOF001", "WALK 26", "TOF003"], "{body}");
    let walk = &body["segments"][0]["itineraries"][0]["legs"][1];
    assert_eq!(walk["estimatedWalk"], true, "{walk}");
    assert!(walk.get("operator").is_none(), "{walk}");
    assert_eq!(body["operator"], serde_json::json!(["VT", "SN"]));
    assert_eq!(body["operatorsNotRunning"], serde_json::json!([]));

    // With the Underground added, the Tube again.
    let (_, body) = plan(&pool, "&operator=VT,SN,LT").await;
    assert_eq!(legs(&body), ["TOF001", "TUBE 10", "TOF002"], "{body}");

    // Avoiding the Underground: the walk.
    let (_, body) = plan(&pool, "&avoidOperator=LT").await;
    assert_eq!(legs(&body), ["TOF001", "WALK 26", "TOF003"], "{body}");
    assert_eq!(body["avoidOperator"], serde_json::json!(["LT"]));

    // A known operator that runs nothing that day.
    let (status, body) = plan(&pool, "&operator=GR").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["operatorsNotRunning"], serde_json::json!(["GR"]));
    let reason = &body["segments"][0]["noResultReason"];
    assert_eq!(reason["constraint"], "operator", "{reason}");
    assert_eq!(reason["values"], serde_json::json!(["GR"]));

    // Avoiding Avanti: nothing else leaves ZFA.
    let (_, body) = plan(&pool, "&avoidOperator=VT").await;
    let reason = &body["segments"][0]["noResultReason"];
    assert_eq!(reason["constraint"], "avoidOperator", "{reason}");
    assert!(reason.get("linkOperators").is_none(), "{reason}");

    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_filter_that_leaves_out_the_tube_names_it_when_no_walk_is_offered() {
    let pool = connect().await;
    // No positions: no estimated walk.
    seed(&pool, false).await;

    let (status, body) = plan(&pool, "&operator=VT,SN").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reason = &body["segments"][0]["noResultReason"];
    assert_eq!(reason["constraint"], "operator", "{reason}");
    assert_eq!(
        reason["values"],
        serde_json::json!(["SN", "VT"]),
        "in code order"
    );
    assert_eq!(
        reason["linkOperators"],
        serde_json::json!(["LT"]),
        "{reason}"
    );
    assert!(
        reason["message"].as_str().unwrap().contains("needs LT"),
        "{reason}"
    );

    let (_, body) = plan(&pool, "&avoidOperator=LT").await;
    let reason = &body["segments"][0]["noResultReason"];
    assert_eq!(reason["constraint"], "avoidOperator", "{reason}");
    assert_eq!(
        reason["linkOperators"],
        serde_json::json!(["LT"]),
        "{reason}"
    );

    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn operator_filters_reject_bad_lists_before_planning() {
    let pool = connect().await;
    seed(&pool, true).await;
    for (query, needle) in [
        ("&operator=VT&avoidOperator=SN", "can't be used together"),
        ("&operator=VTX", "2-character ATOC code"),
        ("&operator=group:NOPE", "unknown operator group"),
        ("&avoidOperator=ds:NOPE", "unknown operator ds:NOPE"),
        (
            "&operator=Q9",
            "operator Q9 is not a known train operator code",
        ),
        ("&operator=Q8,Q9", "operators Q8, Q9 are not known"),
        ("&operator=A1,A2,A3,A4,A5,A6,A7,A8,A9", "at most 8 entries"),
    ] {
        let (status, body) = plan(&pool, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert!(
            body.as_str().is_some_and(|text| text.contains(needle)),
            "{query}: {body}"
        );
    }

    // A group's members aren't checked against the timetable, and the
    // group is echoed with its members.
    let (status, body) = plan(&pool, "&operator=group:gtr,VT,ds:DLR").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["operator"],
        serde_json::json!(["TL", "GN", "SN", "GX", "VT", "ds:DLR"])
    );
    assert_eq!(
        body["operatorGroups"],
        serde_json::json!({ "GTR": ["TL", "GN", "SN", "GX"] })
    );
    assert_eq!(
        body["operatorsNotRunning"],
        serde_json::json!(["TL", "GN", "GX", "ds:DLR"])
    );

    cleanup(&pool).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn the_operator_groups_catalogue_is_served() {
    let pool = connect().await;
    let (status, body) = get(
        test_router(test_app(pool)),
        "/Trips/operator-groups".to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["groups"][0]["code"], "GTR");
    assert_eq!(body["dsOperators"][0]["code"], "ds:DLR");
    assert_eq!(body["names"]["TW"], "Tyne and Wear Metro");
}
