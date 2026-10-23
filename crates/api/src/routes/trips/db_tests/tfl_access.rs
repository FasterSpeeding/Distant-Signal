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
    for (crs, _, _) in STATIONS {
        sqlx::query("DELETE FROM stations WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .ok();
    }
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
        .map(|leg| match leg["kind"].as_str() {
            Some("train") => leg["trainUid"].as_str().unwrap().to_string(),
            _ => {
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
            }
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
