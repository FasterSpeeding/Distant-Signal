//! DS-206: Tube access rides timed by `TfL`'s timetable
//! (`tfl_line_routes.step_seconds`), against a Piccadilly-shaped line in
//! the North Sea: Uxbridge - Rayners Lane - Acton Town - King's Cross and
//! Heathrow - Hatton Cross - Acton Town - King's Cross, each both ways,
//! with the timetable's minutes between stops (Uxbridge to Acton Town 37,
//! Acton Town to Heathrow 26, as `TfL`'s Piccadilly timetable has them).
//! No train runs Uxbridge to Heathrow: the branches meet at Acton Town.

use serde_json::Value;

use super::*;

const LINE: &str = "testpiccadilly206";
const UXB: &str = "940GZZTQUXB";
const RYL: &str = "940GZZTQRYL";
const ACT: &str = "940GZZTQACT";
const HNX: &str = "940GZZTQHNX";
const HRC: &str = "940GZZTQHRC";
const KSX: &str = "940GZZTQKSX";

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
}

/// `(stop id, name, (latitude, longitude), hub CRS)`.
type SeedStop = (&'static str, &'static str, (f64, f64), Option<&'static str>);

const STOPS: [SeedStop; 6] = [
    (UXB, "Test Uxbridge", (57.5, 3.0), None),
    (RYL, "Test Rayners Lane", (57.5, 3.12), None),
    (ACT, "Test Acton Town", (57.42, 3.25), Some("ZQA")),
    (HNX, "Test Hatton Cross", (57.38, 3.1), None),
    (HRC, "Test Heathrow", (57.37, 3.02), Some("ZQH")),
    (KSX, "Test King's Cross", (57.45, 3.5), None),
];

/// `(crs, tiploc, (latitude, longitude))`.
const STATIONS: [(&str, &str, (f64, f64)); 2] = [
    ("ZQA", "TTQACTN", (57.42, 3.25)),
    ("ZQH", "TTQHTHR", (57.37, 3.02)),
];

/// The two westbound routes, out from the branch ends, with the seconds
/// from each stop to the next.
const ROUTES: [(&[&str], &[i32]); 2] = [
    (&[UXB, RYL, ACT, KSX], &[14 * 60, 23 * 60, 32 * 60]),
    (&[HRC, HNX, ACT, KSX], &[6 * 60, 20 * 60, 32 * 60]),
];

async fn cleanup(pool: &PgPool) {
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id LIKE '940GZZTQ%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tfl_line_routes WHERE line_id = $1")
        .bind(LINE)
        .execute(pool)
        .await
        .ok();
    for (crs, _, _) in STATIONS {
        sqlx::query("DELETE FROM stations WHERE crs = $1 AND name LIKE 'Test station %'")
            .bind(crs)
            .execute(pool)
            .await
            .ok();
    }
    sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTTFLTIME-%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TTQEARLY'")
        .execute(pool)
        .await
        .ok();
}

/// The fixture, its routes timed when `timed`.
async fn seed(pool: &PgPool, timed: bool) {
    cleanup(pool).await;
    let lines = serde_json::json!([{ "id": LINE, "name": "Piccadilly" }]);
    for (id, name, (lat, lon), hub) in STOPS {
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
        .bind(format!("TESTTFLTIME-{crs}"))
        .bind(crs)
        .bind(tiploc)
        .execute(pool)
        .await
        .expect("seed stanox_crs");
    }
    // A schedule for the date (a plan needs one): one early train between
    // the two stations, long gone by the plans' 10:00.
    for (seq, tiploc, kind, arrival, departure) in [
        (0, "TTQACTN", "origin", None, Some("05:00:00")),
        (1, "TTQHTHR", "terminate", Some("05:20:00"), None),
    ] {
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TTQEARLY', $2, $3, $4, $5::time, $6::time, 0) ON CONFLICT DO NOTHING",
        )
        .bind(date())
        .bind(seq)
        .bind(tiploc)
        .bind(kind)
        .bind(arrival)
        .bind(departure)
        .execute(pool)
        .await
        .expect("seed the early train");
    }
    let mut index: i16 = 0;
    for (stops, seconds) in ROUTES {
        let back_stops: Vec<&str> = stops.iter().rev().copied().collect();
        let back_seconds: Vec<i32> = seconds.iter().rev().copied().collect();
        for (direction, stops, seconds) in [
            ("outbound", stops.to_vec(), seconds.to_vec()),
            ("inbound", back_stops, back_seconds),
        ] {
            sqlx::query(
                "INSERT INTO tfl_line_routes \
                 (line_id, direction, route_index, name, stop_ids, step_seconds, fetched_at) \
                 VALUES ($1, $2, $3, 'Test', $4, $5, now())",
            )
            .bind(LINE)
            .bind(direction)
            .bind(index)
            .bind(stops)
            .bind(timed.then_some(seconds))
            .execute(pool)
            .await
            .expect("seed a route");
            index += 1;
        }
    }
}

fn plan_uri(origin: &str, destination: &str) -> String {
    format!(
        "/Trips/plan?origin={origin}&destination={destination}&date={}&departAfter=10:00&live=false",
        date()
    )
}

/// The plan's one leg, a ride straight from `origin` to `destination`.
async fn direct_leg(router: axum::Router, origin: &str, destination: &str) -> Value {
    let (status, body) = get(router, plan_uri(origin, destination)).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let legs = body["segments"][0]["itineraries"][0]["legs"]
        .as_array()
        .unwrap_or_else(|| panic!("no itinerary: {body:?}"));
    assert_eq!(legs.len(), 1, "{body:?}");
    assert_eq!(legs[0]["access"]["end"], "both", "{body:?}");
    legs[0].clone()
}

/// Uxbridge to Heathrow by the timetable: 5 minutes in, 37 to Acton Town,
/// the change there (5, and 5 more waiting) and 26 on to Heathrow: 78,
/// either way round. Without the times the same ride is its distance at
/// 30 km/h.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn uxbridge_to_heathrow_takes_the_timetables_minutes() {
    let pool = connect().await;
    let router = test_router(test_app(pool.clone()));

    seed(&pool, true).await;
    for (from, to) in [(UXB, HRC), (HRC, UXB)] {
        let leg = direct_leg(router.clone(), from, to).await;
        assert_eq!(leg["access"]["changeAt"]["id"], ACT, "{leg:?}");
        assert_eq!(leg["minutes"], 78, "{from} to {to}: {leg:?}");
    }
    // To the junction itself: one train, 5 in and 37 riding.
    let leg = direct_leg(router.clone(), UXB, ACT).await;
    assert!(leg["access"].get("changeAt").is_none(), "{leg:?}");
    assert_eq!(leg["minutes"], 42, "{leg:?}");

    seed(&pool, false).await;
    let leg = direct_leg(router, UXB, HRC).await;
    assert_eq!(leg["access"]["changeAt"]["id"], ACT, "{leg:?}");
    let metres = leg["access"]["distanceMetres"].as_f64().unwrap();
    let estimate = 5 + (metres / 1000.0 / 30.0 * 60.0).ceil() as i64 + 10;
    assert_eq!(leg["minutes"].as_i64(), Some(estimate), "{leg:?}");
    cleanup(&pool).await;
}
