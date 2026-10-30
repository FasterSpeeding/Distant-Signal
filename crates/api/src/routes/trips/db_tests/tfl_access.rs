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
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id LIKE '940GZZTC%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id LIKE '940GZZTP%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query(
        "DELETE FROM tfl_line_routes \
         WHERE line_id IN ('testmet135', 'testpic135', 'testcircle191', 'testhc191', \
                           'testpiccadilly204')",
    )
    .execute(pool)
    .await
    .ok();
    for crs in STATIONS
        .iter()
        .map(|(crs, _, _)| *crs)
        .chain([BRANCH_CRS])
        .chain(CIRCLE_STATIONS.iter().map(|(crs, _, _)| *crs))
        .chain(OLD_STREET_STATIONS.iter().map(|(crs, _, _)| *crs))
        .chain(PICCADILLY_STATIONS.iter().map(|(crs, _, _)| *crs))
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
    assert_eq!(
        after["segments"][0]["itineraries"][0]["accessChangeCount"],
        0
    );
    assert_eq!(after["journeys"][0]["accessChangeCount"], 0);
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
    // The change is counted apart from National Rail's.
    assert_eq!(itinerary["changeCount"], 0);
    assert_eq!(itinerary["accessChangeCount"], 1);
    assert_eq!(body["journeys"][0]["changeCount"], 0);
    assert_eq!(body["journeys"][0]["accessChangeCount"], 1);
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

const OLD_STREET_STOPS: [OldStreetStop<'static>; 17] = [
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
    // The rest of the loop (DS-187): on by Bank to Kennington, and back up
    // by Charing Cross to Euston, so a spanning tree breaks it.
    ("940GZZTSBNK", "Test Bank", (55.5133, 2.4114), None),
    ("940GZZTSLNB", "Test London Bridge", (55.5052, 2.4136), None),
    ("940GZZTSBOR", "Test Borough", (55.5011, 2.4057), None),
    ("940GZZTSEAC", "Test Elephant", (55.4943, 2.3999), None),
    ("940GZZTSKNG", "Test Kennington", (55.4884, 2.3947), None),
    ("940GZZTSWLO", "Test Waterloo", (55.5036, 2.3857), None),
    ("940GZZTSEMB", "Test Embankment", (55.5074, 2.3777), None),
    ("940GZZTSCHX", "Test Charing Cross", (55.5080, 2.3753), None),
    (
        "940GZZTSLSQ",
        "Test Leicester Square",
        (55.5113, 2.3719),
        None,
    ),
    (
        "940GZZTSTCR",
        "Test Tottenham Court Road",
        (55.5165, 2.3690),
        None,
    ),
    ("940GZZTSGDG", "Test Goodge Street", (55.5205, 2.3654), None),
    ("940GZZTSWRR", "Test Warren Street", (55.5247, 2.3616), None),
];

/// The loop's two routes, as stop ids from Euston: by Bank, and by
/// Charing Cross, each to Kennington.
const OLD_STREET_ROUTES: [&[&str]; 2] = [
    &[
        "940GZZLUEUS",
        "940GZZTSKSX",
        "940GZZTSAGL",
        OLD_STREET,
        "940GZZTSMGT",
        "940GZZTSBNK",
        "940GZZTSLNB",
        "940GZZTSBOR",
        "940GZZTSEAC",
        "940GZZTSKNG",
    ],
    &[
        "940GZZLUEUS",
        "940GZZTSWRR",
        "940GZZTSGDG",
        "940GZZTSTCR",
        "940GZZTSLSQ",
        "940GZZTSCHX",
        "940GZZTSEMB",
        "940GZZTSWLO",
        "940GZZTSKNG",
    ],
];

/// `(crs, tiploc, (latitude, longitude))`.
const OLD_STREET_STATIONS: [(&str, &str, (f64, f64)); 3] = [
    ("EUS", "TTEUSTN", (55.5282, 2.3663)),
    ("ZTO", "TTOLDST", (55.5263, 2.4127)),
    ("ZTG", "TTMRGT", (55.5186, 2.4114)),
];

/// The Old Street fixture, with its routes or (as before poller-tfl's
/// routes job first ran) none.
async fn seed_old_street(pool: &PgPool, routes: bool) {
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
    let routes = if routes {
        OLD_STREET_ROUTES.as_slice()
    } else {
        &[]
    };
    for (index, ids) in (0_i16..).zip(routes) {
        let back: Vec<&str> = ids.iter().rev().copied().collect();
        for (direction, stops) in [("outbound", ids.to_vec()), ("inbound", back)] {
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
    euston_to_old_street(true).await;
}

/// DS-187: with no routes loaded (as before poller-tfl's routes job first
/// ran) it is the same short ride by King's Cross and Angel, not round the
/// loop by Charing Cross and Bank: the fallback closes the loop.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn without_routes_euston_to_old_street_is_still_the_short_ride() {
    euston_to_old_street(false).await;
}

async fn euston_to_old_street(with_routes: bool) {
    let pool = connect().await;
    seed(&pool).await;
    seed_old_street(&pool, with_routes).await;
    let router = test_router(test_app(pool.clone()));
    // Euston's candidate into Old Street (the station ride), each way.
    let euston = |body: &Value, end: &str| -> Value {
        body["access"][end]["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .find(|c| c["crs"] == "EUS")
            .cloned()
            .expect("an EUS candidate")
    };
    let check_euston = |candidate: &Value| {
        assert_eq!(candidate["line"], "Northern", "{candidate:?}");
        assert!(candidate.get("changeAt").is_none(), "{candidate:?}");
        assert!(
            candidate["distanceMetres"].as_u64().expect("distance") < 4_000,
            "{candidate:?}"
        );
        if with_routes {
            assert_eq!(candidate["minutes"], 12, "{candidate:?}");
            assert_eq!(candidate["stopCount"], 3, "{candidate:?}");
        } else {
            assert!(
                candidate["minutes"].as_i64().expect("minutes") <= 13,
                "{candidate:?}"
            );
            assert!(candidate.get("stopCount").is_none(), "{candidate:?}");
        }
    };

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
    check_euston(&euston(&body, "destination"));
    assert_eq!(leg["minutes"], euston(&body, "destination")["minutes"]);
    if with_routes {
        // About 3 km: 5 in and 7 riding, no walk out at Euston.
        assert_eq!(leg["minutes"], 12, "{leg:?}");
        assert_eq!(body["journeys"][0]["arrival"]["time"], "08:12:00");
    }
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
    check_euston(&euston(&body, "origin"));
    cleanup(&pool).await;
}

/// DS-191: a hand-written Circle and Hammersmith & City, as `TfL` runs
/// them, moved to the North Sea (each position plus 4 degrees north, 3.5
/// east). The Circle from Edgware Road by Paddington's Circle stop (`PAC`)
/// round by Victoria and Liverpool Street to Edgware Road again, then to
/// Paddington's Hammersmith & City stop (`PAH`); the Hammersmith & City
/// from Liverpool Street by King's Cross to `PAH`; each both ways. The two
/// Paddington stops, 250 m apart, may share a hub (`HUBZTP`, station
/// `ZTP`); Liverpool Street's station is `ZTL`.
const CIRCLE_PAC: &str = "940GZZTCPAC";
const CIRCLE_PAH: &str = "940GZZTCPAH";
const CIRCLE_LVT: &str = "940GZZTCLVT";

/// `(stop id suffix, name, on the Hammersmith & City, (latitude,
/// longitude) before the move, hub CRS)`.
type CircleStop<'a> = (&'a str, &'a str, bool, (f64, f64), Option<&'a str>);

const CIRCLE_STOPS: [CircleStop<'static>; 28] = [
    ("ERC", "Test Edgware Road", true, (51.5199, -0.1679), None),
    (
        "PAC",
        "Test Paddington",
        false,
        (51.5166, -0.1757),
        Some("ZTP"),
    ),
    ("BWT", "Test Bayswater", false, (51.5121, -0.1879), None),
    (
        "NHG",
        "Test Notting Hill Gate",
        false,
        (51.5094, -0.1967),
        None,
    ),
    (
        "HSK",
        "Test High Street Kensington",
        false,
        (51.5009, -0.1925),
        None,
    ),
    (
        "GTR",
        "Test Gloucester Road",
        false,
        (51.4945, -0.1829),
        None,
    ),
    (
        "SKS",
        "Test South Kensington",
        false,
        (51.4941, -0.1738),
        None,
    ),
    ("SSQ", "Test Sloane Square", false, (51.4924, -0.1565), None),
    ("VIC", "Test Victoria", false, (51.4965, -0.1447), None),
    (
        "SJP",
        "Test St. James's Park",
        false,
        (51.4994, -0.1335),
        None,
    ),
    ("WSM", "Test Westminster", false, (51.5010, -0.1254), None),
    ("EMB", "Test Embankment", false, (51.5074, -0.1223), None),
    ("TMP", "Test Temple", false, (51.5111, -0.1141), None),
    ("BKF", "Test Blackfriars", false, (51.5120, -0.1031), None),
    ("MSH", "Test Mansion House", false, (51.5122, -0.0940), None),
    ("CST", "Test Cannon Street", false, (51.5113, -0.0904), None),
    ("MMT", "Test Monument", false, (51.5108, -0.0863), None),
    ("TWH", "Test Tower Hill", false, (51.5098, -0.0766), None),
    ("ALD", "Test Aldgate", false, (51.5143, -0.0755), None),
    (
        "LVT",
        "Test Liverpool Street",
        true,
        (51.5178, -0.0823),
        Some("ZTL"),
    ),
    ("MGT", "Test Moorgate", true, (51.5186, -0.0886), None),
    ("BBN", "Test Barbican", true, (51.5204, -0.0979), None),
    ("FCC", "Test Farringdon", true, (51.5203, -0.1053), None),
    ("KSX", "Test King's Cross", true, (51.5308, -0.1238), None),
    ("ESQ", "Test Euston Square", true, (51.5257, -0.1359), None),
    (
        "GPS",
        "Test Great Portland Street",
        true,
        (51.5238, -0.1439),
        None,
    ),
    ("BST", "Test Baker Street", true, (51.5226, -0.1571), None),
    (
        "PAH",
        "Test Paddington (H&C Line)",
        true,
        (51.5182, -0.1783),
        Some("ZTP"),
    ),
];

/// `(crs, tiploc, (latitude, longitude))`, after the move.
const CIRCLE_STATIONS: [(&str, &str, (f64, f64)); 2] = [
    ("ZTP", "TTPADD", (55.5170, 3.3240)),
    ("ZTL", "TTLIVST", (55.5178, 3.4177)),
];

/// Seeds the Circle fixture; with `hub`, the two Paddington stops share
/// `HUBZTP` (and so its station). Without, `PAH` is no station's: else
/// the plan walks to `ZTP` from `PAC` and rides from `PAH` as a station
/// candidate, which hides the change at Edgware Road.
async fn seed_circle(pool: &PgPool, hub: bool) {
    let both = serde_json::json!([
        { "id": "testcircle191", "name": "Circle" },
        { "id": "testhc191", "name": "Hammersmith & City" },
    ]);
    let circle = serde_json::json!([{ "id": "testcircle191", "name": "Circle" }]);
    for (suffix, name, hc, (lat, lon), crs) in CIRCLE_STOPS {
        let id = format!("940GZZTC{suffix}");
        let hub_code = crs.filter(|_| hub).map(|crs| format!("HUB{crs}"));
        sqlx::query(
            "INSERT INTO tfl_stops \
             (stop_id, name, search_name, modes, lines, hub_code, latitude, longitude, fetched_at) \
             VALUES ($1, $2, lower($2), ARRAY['tube'], $3, $4, $5, $6, now())",
        )
        .bind(&id)
        .bind(name)
        .bind(if hc { &both } else { &circle })
        .bind(hub_code)
        .bind(lat + 4.0)
        .bind(lon + 3.5)
        .execute(pool)
        .await
        .expect("seed tfl_stops");
        if let Some(crs) = crs.filter(|_| hub || suffix != "PAH") {
            sqlx::query("INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ($1, $2, 'hub')")
                .bind(&id)
                .bind(crs)
                .execute(pool)
                .await
                .expect("seed tfl_stop_crs");
        }
    }
    for (crs, tiploc, (lat, lon)) in CIRCLE_STATIONS {
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
    let order = |suffixes: &[&str]| -> Vec<String> {
        suffixes.iter().map(|s| format!("940GZZTC{s}")).collect()
    };
    let north = [
        "LVT", "MGT", "BBN", "FCC", "KSX", "ESQ", "GPS", "BST", "ERC", "PAH",
    ];
    let circle_route: Vec<&str> = CIRCLE_STOPS[..19]
        .iter()
        .map(|(suffix, ..)| *suffix)
        .chain(north)
        .collect();
    for (line, ids) in [
        ("testcircle191", order(&circle_route)),
        ("testhc191", order(&north)),
    ] {
        let back: Vec<String> = ids.iter().rev().cloned().collect();
        for (direction, stops) in [("inbound", ids), ("outbound", back)] {
            sqlx::query(
                "INSERT INTO tfl_line_routes (line_id, direction, route_index, name, stop_ids, fetched_at) \
                 VALUES ($1, $2, 0, 'Test', $3, now())",
            )
            .bind(line)
            .bind(direction)
            .bind(stops)
            .execute(pool)
            .await
            .expect("seed a route");
        }
    }
}

/// DS-191: Paddington to Liverpool Street, stop to stop. One Circle train
/// runs there round by Victoria (31 minutes); the ride to Edgware Road and
/// a change there is quicker (30), and the Hammersmith & City from
/// Paddington's other stop, a walk away, quicker still (25).
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn paddington_to_liverpool_street_takes_the_short_way() {
    let pool = connect().await;
    seed(&pool).await;
    let router = test_router(test_app(pool.clone()));
    let leg = |body: &Value| -> Value {
        let itinerary = &body["segments"][0]["itineraries"][0];
        assert_eq!(
            describe(itinerary),
            [format!("TUBE {CIRCLE_PAC}-{CIRCLE_LVT} both")],
            "{body:?}"
        );
        itinerary["legs"][0].clone()
    };

    // The two Paddington stops apart: the change at Edgware Road.
    seed_circle(&pool, false).await;
    let (status, body) = get(
        router.clone(),
        plan_uri(CIRCLE_PAC, CIRCLE_LVT, "departAfter=10:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let changed = leg(&body);
    assert_eq!(changed["line"], "Circle", "{changed:?}");
    // About 30 (29 here: a degree of longitude is shorter this far north).
    let minutes = changed["minutes"].as_i64().expect("minutes");
    assert!((28..=30).contains(&minutes), "{changed:?}");
    assert_eq!(changed["access"]["changeAt"]["id"], "940GZZTCERC");
    assert!(changed["access"].get("boardStop").is_none(), "{changed:?}");

    // One station: from the Hammersmith & City stop.
    cleanup(&pool).await;
    seed(&pool).await;
    seed_circle(&pool, true).await;
    let (status, body) = get(
        router.clone(),
        plan_uri(CIRCLE_PAC, CIRCLE_LVT, "departAfter=10:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let sibling = leg(&body);
    assert!(
        sibling["minutes"].as_i64().expect("minutes") <= 25,
        "{sibling:?}"
    );
    assert_eq!(sibling["access"]["boardStop"]["id"], CIRCLE_PAH);
    assert!(sibling["access"].get("changeAt").is_none(), "{sibling:?}");
    assert_eq!(sibling["originStop"]["id"], CIRCLE_PAC);
    assert_eq!(sibling["destinationStop"]["id"], CIRCLE_LVT);

    // To Liverpool Street's station: the same ride from the other stop.
    let (status, body) = get(router, plan_uri(CIRCLE_PAC, "ZTL", "departAfter=10:00")).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let ztl = candidate(&body, "ZTL");
    assert_eq!(ztl["boardStop"]["id"], CIRCLE_PAH, "{ztl:?}");
    assert!(ztl["minutes"].as_i64().expect("minutes") <= 25, "{ztl:?}");
    cleanup(&pool).await;
}

/// DS-205: a Piccadilly-shaped line in the North Sea. Its routes, as
/// `TfL`'s: Uxbridge - Rayners Lane - Acton Town - King's Cross, and
/// Heathrow - Hatton Cross - Acton Town - King's Cross, each both ways. No
/// train runs Uxbridge to Heathrow: the branches meet at Acton Town.
const PICCADILLY_LINE: &str = "testpiccadilly204";
const PICCADILLY_UXB: &str = "940GZZTPUXB";
const PICCADILLY_HRC: &str = "940GZZTPHRC";
const PICCADILLY_ACT: &str = "940GZZTPACT";

/// `(stop id, name, (latitude, longitude), hub CRS)`.
const PICCADILLY_STOPS: [OldStreetStop<'static>; 6] = [
    (PICCADILLY_UXB, "Test Uxbridge", (57.0, 3.0), None),
    ("940GZZTPRYL", "Test Rayners Lane", (57.0, 3.08), None),
    (
        PICCADILLY_ACT,
        "Test Acton Town",
        (56.95, 3.15),
        Some("ZPA"),
    ),
    ("940GZZTPHNX", "Test Hatton Cross", (56.9, 3.05), None),
    (PICCADILLY_HRC, "Test Heathrow", (56.88, 2.98), Some("ZPH")),
    ("940GZZTPKSX", "Test King's Cross", (57.0, 3.3), None),
];

/// `(crs, tiploc, (latitude, longitude))`.
const PICCADILLY_STATIONS: [(&str, &str, (f64, f64)); 2] = [
    ("ZPA", "TTACTON", (56.95, 3.15)),
    ("ZPH", "TTHTHRW", (56.88, 2.98)),
];

async fn seed_piccadilly(pool: &PgPool) {
    let lines = serde_json::json!([{ "id": PICCADILLY_LINE, "name": "Piccadilly" }]);
    for (id, name, (lat, lon), hub) in PICCADILLY_STOPS {
        sqlx::query(
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
             VALUES ($1, $2, lower($2), ARRAY['tube'], $3, $4, $5, now())",
        )
        .bind(id)
        .bind(name)
        .bind(&lines)
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
    for (crs, tiploc, (lat, lon)) in PICCADILLY_STATIONS {
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
    let uxbridge = [PICCADILLY_UXB, "940GZZTPRYL", PICCADILLY_ACT, "940GZZTPKSX"];
    let heathrow = [PICCADILLY_HRC, "940GZZTPHNX", PICCADILLY_ACT, "940GZZTPKSX"];
    let rows: [(&str, Vec<&str>); 4] = [
        ("outbound", uxbridge.to_vec()),
        ("inbound", uxbridge.iter().rev().copied().collect()),
        ("outbound", heathrow.to_vec()),
        ("inbound", heathrow.iter().rev().copied().collect()),
    ];
    for (index, (direction, stops)) in (0_i16..).zip(rows) {
        sqlx::query(
            "INSERT INTO tfl_line_routes (line_id, direction, route_index, name, stop_ids, fetched_at) \
             VALUES ($1, $2, $3, 'Test', $4, now())",
        )
        .bind(PICCADILLY_LINE)
        .bind(direction)
        .bind(index)
        .bind(stops)
        .execute(pool)
        .await
        .expect("seed a route");
    }
}

/// DS-205: Uxbridge to Heathrow, stop to stop, is one access leg that
/// changes at Acton Town. `changeCount` stays National Rail's (none);
/// `accessChangeCount` counts the change, on the itinerary and the journey.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn uxbridge_to_heathrow_counts_its_access_change() {
    let pool = connect().await;
    seed(&pool).await;
    seed_piccadilly(&pool).await;
    let router = test_router(test_app(pool.clone()));
    for (from, to) in [
        (PICCADILLY_UXB, PICCADILLY_HRC),
        (PICCADILLY_HRC, PICCADILLY_UXB),
    ] {
        let (status, body) = get(router.clone(), plan_uri(from, to, "departAfter=10:00")).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let itinerary = &body["segments"][0]["itineraries"][0];
        assert_eq!(
            describe(itinerary),
            [format!("TUBE {from}-{to} both")],
            "{body:?}"
        );
        let leg = &itinerary["legs"][0];
        assert_eq!(leg["access"]["changeAt"]["id"], PICCADILLY_ACT, "{leg:?}");
        assert_eq!(itinerary["changeCount"], 0);
        assert_eq!(itinerary["accessChangeCount"], 1);
        assert_eq!(body["journeys"][0]["changeCount"], 0);
        assert_eq!(body["journeys"][0]["accessChangeCount"], 1);
    }

    // To Acton Town's own stop: one train, nothing to count.
    let (status, body) = get(
        router,
        plan_uri(PICCADILLY_UXB, PICCADILLY_ACT, "departAfter=10:00"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert!(
        itinerary["legs"][0]["access"].get("changeAt").is_none(),
        "{body:?}"
    );
    assert_eq!(itinerary["accessChangeCount"], 0);
    assert_eq!(body["journeys"][0]["accessChangeCount"], 0);
    cleanup(&pool).await;
}
