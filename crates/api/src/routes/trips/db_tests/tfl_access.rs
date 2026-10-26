//! A `TfL` stop as a `/Trips/plan` end (DS-135, `data::tfl_access`): an
//! Uxbridge-like stop, `Testbridge`, on two synthetic lines, with no
//! National Rail station of its own. Riding the "Test Metropolitan" reaches
//! `Test Harrow` (a hub, `ZTH`); riding the "Test Piccadilly" reaches `Test
//! Airport` (a hub, `ZTX`); `ZTW` is a station a short walk away. Every train runs to or from `ZTM`. Placed in the North Sea,
//! so no real station is near.

use serde_json::Value;

use super::*;

const UXB: &str = "940GZZTSUXB";

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
}

/// `(stop id, name, lines, (latitude, longitude), hub CRS)`.
type SeedStop<'a> = (&'a str, &'a str, &'a [&'a str], (f64, f64), Option<&'a str>);

const STOPS: [SeedStop<'static>; 5] = [
    (
        UXB,
        "Testbridge",
        &["testmet135", "testpic135"],
        (55.0, 2.0),
        None,
    ),
    (
        "940GZZTSMID",
        "Test Middle",
        &["testmet135", "testpic135"],
        (55.0, 2.07),
        None,
    ),
    (
        "940GZZTSHOH",
        "Test Harrow",
        &["testmet135"],
        (55.0, 2.15),
        Some("ZTH"),
    ),
    (
        "940GZZTSACT",
        "Test Acton",
        &["testpic135"],
        (54.85, 2.2),
        None,
    ),
    (
        "940GZZTSHRX",
        "Test Airport",
        &["testpic135"],
        (54.9, 2.1),
        Some("ZTX"),
    ),
];

/// `(crs, tiploc, (latitude, longitude))`.
const STATIONS: [(&str, &str, (f64, f64)); 4] = [
    ("ZTH", "TTHROW", (55.0, 2.15)),
    ("ZTX", "TTHRX", (54.9, 2.1)),
    ("ZTW", "TTWALK", (55.006, 2.0)),
    ("ZTM", "TTMARY", (55.2, 2.4)),
];

/// `(uid, from tiploc, departs, to tiploc, arrives)`.
const TRAINS: [(&str, &str, &str, &str, &str); 6] = [
    ("TTUP1", "TTHROW", "08:40:00", "TTMARY", "08:55:00"),
    ("TTUP2", "TTWALK", "08:30:00", "TTMARY", "09:20:00"),
    ("TTUP3", "TTHRX", "08:50:00", "TTMARY", "09:05:00"),
    ("TTDN1", "TTMARY", "10:00:00", "TTHROW", "10:15:00"),
    ("TTDN2", "TTMARY", "09:50:00", "TTWALK", "10:35:00"),
    ("TTDN3", "TTMARY", "11:00:00", "TTHRX", "11:10:00"),
];

fn line_json(id: &str) -> Value {
    let name = match id {
        "testmet135" => "Test Metropolitan",
        _ => "Test Piccadilly",
    };
    serde_json::json!({ "id": id, "name": name })
}

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id LIKE '940GZZTS%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tfl_line_routes WHERE line_id IN ('testmet135', 'testpic135')")
        .execute(pool)
        .await
        .ok();
    for crs in STATIONS
        .iter()
        .map(|(crs, _, _)| *crs)
        .chain([BRANCH_CRS])
        .chain(OLD_STREET_STATIONS.iter().map(|(crs, _, _)| *crs))
    {
        // Only this file's rows: `EUS` is a real code.
        sqlx::query("DELETE FROM stations WHERE crs = $1 AND name LIKE 'Test station %'")
            .bind(crs)
            .execute(pool)
            .await
            .ok();
    }
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id = ANY($1) AND name LIKE 'Test %'")
        .bind(OLD_STREET_REAL_IDS)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tfl_line_routes WHERE line_id = $1")
        .bind(OLD_STREET_LINE)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TTOS%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTTFLACC-%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query(
        "DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TTUP%' OR uid LIKE 'TTDN%'",
    )
    .execute(pool)
    .await
    .ok();
}

async fn seed(pool: &PgPool) {
    cleanup(pool).await;
    for (id, name, lines, (lat, lon), hub) in STOPS {
        let lines: Vec<Value> = lines.iter().map(|line| line_json(line)).collect();
        sqlx::query(
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
             VALUES ($1, $2, lower($2), ARRAY['tube'], $3, $4, $5, now())",
        )
        .bind(id)
        .bind(name)
        .bind(Value::Array(lines))
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .expect("seed tfl_stops");
        if let Some(crs) = hub {
            sqlx::query("INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ($1, $2, 'hub')")
                .bind(id)
                .bind(crs)
                .execute(pool)
                .await
                .expect("seed tfl_stop_crs");
        }
    }
    for (crs, tiploc, (lat, lon)) in STATIONS {
        sqlx::query(
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, $2, $3, $4)",
        )
        .bind(crs)
        .bind(format!("Test station {crs}"))
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .expect("seed stations");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ($1, $2, $3, $2, 1) ON CONFLICT (stanox) DO NOTHING",
        )
        .bind(format!("TESTTFLACC-{crs}"))
        .bind(crs)
        .bind(tiploc)
        .execute(pool)
        .await
        .expect("seed stanox_crs");
    }
    for (uid, from, departs, to, arrives) in TRAINS {
        for (seq, tiploc, kind, arrival, departure) in [
            (0, from, "origin", None, Some(departs)),
            (1, to, "terminate", Some(arrives), None),
        ] {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(date())
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(pool)
            .await
            .expect("seed calling point");
        }
    }
}

fn plan_uri(origin: &str, destination: &str, time: &str) -> String {
    format!(
        "/Trips/plan?origin={origin}&destination={destination}&date={}&{time}&live=false",
        date()
    )
}

/// `kind`, and for a transfer `mode`, each end's CRS or stop id, and the
/// access end; a train's UID.
fn describe(itinerary: &Value) -> Vec<String> {
    itinerary["legs"]
        .as_array()
        .expect("legs")
        .iter()
        .map(|leg| {
            if leg["kind"] == "train" {
                return leg["trainUid"].as_str().unwrap().to_string();
            }
            let end = |crs: &Value, stop: &Value| {
                crs.as_str()
                    .or_else(|| stop["id"].as_str())
                    .unwrap_or("?")
                    .to_string()
            };
            format!(
                "{} {}-{} {}",
                leg["mode"].as_str().unwrap(),
                end(&leg["originCrs"], &leg["originStop"]),
                end(&leg["destinationCrs"], &leg["destinationStop"]),
                leg["access"]["end"].as_str().unwrap_or("-"),
            )
        })
        .collect()
}

/// From the stop: the Tube to `Test Harrow` and the 08:40 beats the walk to
/// `ZTW` and the slow 08:30, and the Tube leg is dressed as an access leg
/// that leaves just in time for the train.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_tfl_stop_origin_plans_from_its_best_station() {
    let pool = connect().await;
    seed(&pool).await;
    let router = test_router(test_app(pool.clone()));
    let (status, body) = get(router, plan_uri(UXB, "ZTM", "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["segments"][0]["originCrs"], UXB);
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(
        describe(itinerary),
        [format!("TUBE {UXB}-ZTH origin"), "TTUP1".to_string()]
    );
    let access = &itinerary["legs"][0];
    assert_eq!(access["originCrs"], Value::Null);
    assert_eq!(access["originStop"]["name"], "Testbridge");
    assert_eq!(access["destinationStop"]["id"], "940GZZTSHOH");
    assert_eq!(access["line"], "Test Metropolitan");
    assert_eq!(access["access"]["basis"], "line");
    // 9.6 km along the line: 5 + 20 + 3 minutes.
    assert_eq!(access["minutes"], 28);
    // Left at 08:07, not 08:00: 28 minutes and the 5-minute change make
    // the 08:40.
    assert_eq!(body["journeys"][0]["departure"]["time"], "08:07:00");
    assert_eq!(itinerary["totalDurationMinutes"], 48);
    // Every candidate, quickest first, is echoed.
    let candidates: Vec<(&str, &str, &str)> = body["access"]["origin"]["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|c| {
            (
                c["crs"].as_str().unwrap(),
                c["mode"].as_str().unwrap(),
                c["basis"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        candidates,
        [
            ("ZTW", "WALK", "walk"),
            ("ZTH", "TUBE", "line"),
            ("ZTX", "TUBE", "line"),
        ]
    );
    assert_eq!(body["access"]["origin"]["stop"]["id"], UXB);
    assert_eq!(body["access"]["destination"], Value::Null);
    cleanup(&pool).await;
}

/// To the stop, depart-after and arrive-by: the train to `Test Harrow`
/// and the Tube back beats the train to `ZTW` and the walk.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_tfl_stop_destination_ends_with_an_egress_leg() {
    let pool = connect().await;
    seed(&pool).await;
    let router = test_router(test_app(pool.clone()));
    let (status, body) = get(router.clone(), plan_uri("ZTM", UXB, "departAfter=09:30")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(
        describe(itinerary),
        ["TTDN1".to_string(), format!("TUBE ZTH-{UXB} destination")]
    );
    assert_eq!(itinerary["legs"][1]["originStop"]["id"], "940GZZTSHOH");
    assert_eq!(itinerary["legs"][1]["destinationStop"]["id"], UXB);
    // 10:15 at Test Harrow, 28 minutes on: 10:43.
    assert_eq!(body["journeys"][0]["arrival"]["time"], "10:43:00");

    // Arrive by 10:45: the latest that makes it is the same 10:00.
    let (status, body) = get(router.clone(), plan_uri("ZTM", UXB, "arriveBy=10:45")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        describe(&body["segments"][0]["itineraries"][0]),
        ["TTDN1".to_string(), format!("TUBE ZTH-{UXB} destination")]
    );
    // Arrive by 10:42: neither 10:43 nor the walk's 10:47 makes it.
    let (status, body) = get(router, plan_uri("ZTM", UXB, "arriveBy=10:42")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["segments"][0]["itineraries"], serde_json::json!([]));
    cleanup(&pool).await;
}

/// An unknown stop is a 400; two stops on one line ride straight there; a
/// station-only plan has no `access`.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn tfl_stop_ends_are_checked_and_station_plans_are_unchanged() {
    let pool = connect().await;
    seed(&pool).await;
    let router = test_router(test_app(pool.clone()));
    let (status, body) = get(
        router.clone(),
        plan_uri("940GZZTSNOPE", "ZTM", "departAfter=08:00"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
    assert!(
        body.as_str().unwrap().contains("not a known TfL stop"),
        "{body:?}"
    );
    let (status, body) = get(router.clone(), plan_uri(UXB, UXB, "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");

    let (status, body) = get(
        router.clone(),
        plan_uri(UXB, "940GZZTSMID", "departAfter=08:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        describe(&body["segments"][0]["itineraries"][0]),
        [format!("TUBE {UXB}-940GZZTSMID both")]
    );

    let (status, body) = get(router, plan_uri("ZTH", "ZTM", "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert!(body.get("access").is_none(), "{body:?}");
    assert_eq!(describe(&body["segments"][0]["itineraries"][0]), ["TTUP1"]);
    cleanup(&pool).await;
}

/// The branch of [`branch_change_costs_a_change_at_the_junction`]: `Test
/// Ricky`, a hub (`ZTR`) on the Test Metropolitan beyond `Test Harrow`, 3.8
/// km from `Testbridge` as the crow flies but 18 km by the line.
const BRANCH_STOP: &str = "940GZZTSRIC";
const BRANCH_CRS: &str = "ZTR";

/// Seeds `Test Ricky`, its station and an 08:30 from it to `ZTM` (quicker
/// than the 08:40 from `Test Harrow`); with `routes`, the Test
/// Metropolitan's routes as two branches meeting at `Test Harrow`:
/// Testbridge - Test Middle - Test Harrow, and Test Harrow - Test Ricky,
/// each both ways.
async fn seed_branch(pool: &PgPool, routes: bool) {
    sqlx::query(
        "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
         VALUES ($1, 'Test Ricky', 'test ricky', ARRAY['tube'], $2, 55.03, 2.03, now())",
    )
    .bind(BRANCH_STOP)
    .bind(Value::Array(vec![line_json("testmet135")]))
    .execute(pool)
    .await
    .expect("seed the branch stop");
    sqlx::query("INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ($1, $2, 'hub')")
        .bind(BRANCH_STOP)
        .bind(BRANCH_CRS)
        .execute(pool)
        .await
        .expect("seed its hub");
    sqlx::query(
        "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, 'Test station ZTR', 55.03, 2.03)",
    )
    .bind(BRANCH_CRS)
    .execute(pool)
    .await
    .expect("seed its station");
    sqlx::query(
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
         VALUES ('TESTTFLACC-ZTR', $1, 'TTRICK', $1, 1) ON CONFLICT (stanox) DO NOTHING",
    )
    .bind(BRANCH_CRS)
    .execute(pool)
    .await
    .expect("seed its tiploc");
    for (seq, tiploc, kind, arrival, departure) in [
        (0, "TTRICK", "origin", None, Some("08:30:00")),
        (1, "TTMARY", "terminate", Some("08:45:00"), None),
    ] {
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TTUP4', $2, $3, $4, $5::time, $6::time, 0) ON CONFLICT DO NOTHING",
        )
        .bind(date())
        .bind(seq)
        .bind(tiploc)
        .bind(kind)
        .bind(arrival)
        .bind(departure)
        .execute(pool)
        .await
        .expect("seed the 08:30");
    }
    if !routes {
        return;
    }
    let trunk = [UXB, "940GZZTSMID", "940GZZTSHOH"];
    let branch = ["940GZZTSHOH", BRANCH_STOP];
    let rows: [(&str, Vec<&str>); 4] = [
        ("outbound", trunk.to_vec()),
        ("inbound", trunk.iter().rev().copied().collect()),
        ("outbound", branch.to_vec()),
        ("inbound", branch.iter().rev().copied().collect()),
    ];
    for (index, (direction, stops)) in (0_i16..).zip(rows) {
        sqlx::query(
            "INSERT INTO tfl_line_routes (line_id, direction, route_index, name, stop_ids, fetched_at) \
             VALUES ('testmet135', $1, $2, 'Test', $3, now())",
        )
        .bind(direction)
        .bind(index)
        .bind(stops)
        .execute(pool)
        .await
        .expect("seed a route");
    }
}

fn candidate<'a>(body: &'a Value, crs: &str) -> &'a Value {
    body["access"]["origin"]["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|c| c["crs"] == crs)
        .unwrap_or_else(|| panic!("no {crs} candidate: {body:?}"))
}

/// DS-151: without routes, the spanning tree takes `Test Ricky` as a short
/// ride with no change, and the 08:30 from there wins. With the line's
/// routes the ride runs in to `Test Harrow` and out along the other branch,
/// with a change there (5 minutes and 5 more waiting), so it misses the
/// 08:30 and the Tube to `Test Harrow` and the 08:40 win again. Two stops on
/// different branches ride straight there, changing at the junction.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn branch_change_costs_a_change_at_the_junction() {
    let pool = connect().await;
    seed(&pool).await;
    seed_branch(&pool, false).await;
    let router = test_router(test_app(pool.clone()));
    let (status, before) = get(router.clone(), plan_uri(UXB, "ZTM", "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{before:?}");
    assert_eq!(
        describe(&before["segments"][0]["itineraries"][0]),
        [format!("TUBE {UXB}-ZTR origin"), "TTUP4".to_string()]
    );
    let tree = candidate(&before, "ZTR");
    assert!(tree.get("changeAt").is_none(), "{tree:?}");
    assert!(tree.get("stopCount").is_none(), "{tree:?}");
    let tree_minutes = tree["minutes"].as_i64().unwrap();

    cleanup(&pool).await;
    seed(&pool).await;
    seed_branch(&pool, true).await;
    let (status, after) = get(router.clone(), plan_uri(UXB, "ZTM", "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{after:?}");
    assert_eq!(
        describe(&after["segments"][0]["itineraries"][0]),
        [format!("TUBE {UXB}-ZTH origin"), "TTUP1".to_string()]
    );
    let branched = candidate(&after, "ZTR");
    assert_eq!(branched["changeAt"]["id"], "940GZZTSHOH", "{branched:?}");
    assert_eq!(branched["stopCount"], 3);
    // About 18 km: 5 in, 36 riding, 5 to change, 5 waiting, 3 out.
    assert_eq!(branched["minutes"], 54, "{branched:?}");
    assert!(branched["minutes"].as_i64().unwrap() > tree_minutes + 10);
    let harrow = candidate(&after, "ZTH");
    assert!(harrow.get("changeAt").is_none());
    assert_eq!(harrow["stopCount"], 2);

    // Stop to stop across the junction: one ride, its change named.
    let (status, body) = get(router, plan_uri(UXB, BRANCH_STOP, "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(
        describe(itinerary),
        [format!("TUBE {UXB}-{BRANCH_STOP} both")]
    );
    assert_eq!(
        itinerary["legs"][0]["access"]["changeAt"]["name"],
        "Test Harrow"
    );
    cleanup(&pool).await;
}

/// DS-167: a hand-written stretch of a Northern-like line, moved to the
/// North Sea (Euston's position plus 4 degrees north, 2.5 east, and so on):
/// Euston's own stop (`940GZZLUEUS`, Euston's by `CRS_OVERRIDES`), `Test
/// King's Cross`, `Test Angel`, Old Street (`940GZZLUODS`, a hub `ZTO`) and
/// `Test Moorgate` (a hub `ZTG`), with its routes. The two real stop ids
/// and `EUS` are needed: the request names Old Street's, and Euston's stop
/// is Euston's by code.
const OLD_STREET: &str = "940GZZLUODS";
const OLD_STREET_LINE: &str = "testnorthern167";
const OLD_STREET_REAL_IDS: &[&str] = &["940GZZLUEUS", OLD_STREET];

/// `(stop id, name, (latitude, longitude), hub CRS)`.
type OldStreetStop<'a> = (&'a str, &'a str, (f64, f64), Option<&'a str>);

const OLD_STREET_STOPS: [OldStreetStop<'static>; 5] = [
    ("940GZZLUEUS", "Test Euston", (55.5282, 2.3663), None),
    ("940GZZTSKSX", "Test King's Cross", (55.5308, 2.3762), None),
    ("940GZZTSAGL", "Test Angel", (55.5322, 2.3942), None),
    (
        OLD_STREET,
        "Test Old Street",
        (55.5263, 2.4127),
        Some("ZTO"),
    ),
    (
        "940GZZTSMGT",
        "Test Moorgate",
        (55.5186, 2.4114),
        Some("ZTG"),
    ),
];

/// `(crs, tiploc, (latitude, longitude))`.
const OLD_STREET_STATIONS: [(&str, &str, (f64, f64)); 3] = [
    ("EUS", "TTEUSTN", (55.5282, 2.3663)),
    ("ZTO", "TTOLDST", (55.5263, 2.4127)),
    ("ZTG", "TTMRGT", (55.5186, 2.4114)),
];

async fn seed_old_street(pool: &PgPool) {
    let line = serde_json::json!([{ "id": OLD_STREET_LINE, "name": "Northern" }]);
    for (id, name, (lat, lon), hub) in OLD_STREET_STOPS {
        sqlx::query(
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
             VALUES ($1, $2, lower($2), ARRAY['tube'], $3, $4, $5, now())",
        )
        .bind(id)
        .bind(name)
        .bind(&line)
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .expect("seed tfl_stops");
        if let Some(crs) = hub {
            sqlx::query("INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ($1, $2, 'hub')")
                .bind(id)
                .bind(crs)
                .execute(pool)
                .await
                .expect("seed tfl_stop_crs");
        }
    }
    let ids: Vec<&str> = OLD_STREET_STOPS.iter().map(|(id, _, _, _)| *id).collect();
    let back: Vec<&str> = ids.iter().rev().copied().collect();
    for (index, (direction, stops)) in (0_i16..).zip([("outbound", ids), ("inbound", back)]) {
        sqlx::query(
            "INSERT INTO tfl_line_routes (line_id, direction, route_index, name, stop_ids, fetched_at) \
             VALUES ($1, $2, $3, 'Test', $4, now())",
        )
        .bind(OLD_STREET_LINE)
        .bind(direction)
        .bind(index)
        .bind(stops)
        .execute(pool)
        .await
        .expect("seed a route");
    }
    for (crs, tiploc, (lat, lon)) in OLD_STREET_STATIONS {
        sqlx::query(
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, $2, $3, $4)",
        )
        .bind(crs)
        .bind(format!("Test station {crs}"))
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .expect("seed stations");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ($1, $2, $3, $2, 1) ON CONFLICT (stanox) DO NOTHING",
        )
        .bind(format!("TESTTFLACC-{crs}"))
        .bind(crs)
        .bind(tiploc)
        .execute(pool)
        .await
        .expect("seed stanox_crs");
    }
    // A train from Euston to Moorgate, and one on to Old Street: the
    // trains a plan to Old Street's station alone would take.
    for (uid, from, departs, to, arrives) in [
        ("TTOS1", "TTEUSTN", "08:02:00", "TTMRGT", "08:10:00"),
        ("TTOS2", "TTMRGT", "08:31:00", "TTOLDST", "08:33:00"),
    ] {
        for (seq, tiploc, kind, arrival, departure) in [
            (0, from, "origin", None, Some(departs)),
            (1, to, "terminate", Some(arrives), None),
        ] {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(date())
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(pool)
            .await
            .expect("seed calling point");
        }
    }
}

/// Euston to Old Street's Tube stop is one Northern line ride from
/// Euston's own stop, depart-after and arrive-by, and back again; Old
/// Street keeps its hub walk beside its line candidates.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn euston_to_old_street_is_one_northern_line_ride() {
    let pool = connect().await;
    seed(&pool).await;
    seed_old_street(&pool).await;
    let router = test_router(test_app(pool.clone()));

    let (status, body) = get(
        router.clone(),
        plan_uri("EUS", OLD_STREET, "departAfter=08:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(
        describe(itinerary),
        [format!("TUBE EUS-{OLD_STREET} destination")]
    );
    let leg = &itinerary["legs"][0];
    assert_eq!(leg["kind"], "transfer");
    assert_eq!(leg["line"], "Northern");
    assert_eq!(leg["access"]["basis"], "line");
    assert_eq!(leg["originStop"]["id"], "940GZZLUEUS");
    assert_eq!(leg["destinationStop"]["id"], OLD_STREET);
    // About 3 km: 5 in and 7 riding, no walk out at Euston.
    assert_eq!(leg["minutes"], 12, "{leg:?}");
    assert_eq!(body["journeys"][0]["arrival"]["time"], "08:12:00");
    // Old Street's own candidates: the hub walk first, then rides.
    let candidates = body["access"]["destination"]["candidates"]
        .as_array()
        .expect("candidates");
    assert_eq!(candidates[0]["crs"], "ZTO");
    assert_eq!(candidates[0]["basis"], "hub");
    assert!(
        candidates
            .iter()
            .any(|c| c["crs"] == "ZTG" && c["basis"] == "line"),
        "{candidates:?}"
    );

    // Arrive by 09:00: the same ride, left at 08:48.
    let (status, body) = get(
        router.clone(),
        plan_uri("EUS", OLD_STREET, "arriveBy=09:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(
        describe(&body["segments"][0]["itineraries"][0]),
        [format!("TUBE EUS-{OLD_STREET} destination")]
    );
    assert_eq!(
        body["segments"][0]["itineraries"][0]["legs"][0]["line"],
        "Northern"
    );

    // And back: Old Street to Euston, one ride out of the stop.
    let (status, body) = get(router, plan_uri(OLD_STREET, "EUS", "departAfter=08:00")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(
        describe(itinerary),
        [format!("TUBE {OLD_STREET}-EUS origin")]
    );
    assert_eq!(itinerary["legs"][0]["line"], "Northern");
    cleanup(&pool).await;
}
