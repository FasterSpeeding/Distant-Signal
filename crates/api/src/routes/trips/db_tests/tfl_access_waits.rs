//! DS-234: Tube access rides wait by the `tfl` transit network's
//! timetable, against a Piccadilly-shaped line in the North Sea (the
//! shape of `tfl_access_times`): Uxbridge - Rayners Lane - Acton Town -
//! King's Cross and Heathrow - Hatton Cross - Acton Town - King's Cross,
//! each both ways. The Uxbridge branch runs every 20 minutes, the Heathrow
//! branch every 5 one way and every 10 the other, so the trunk (Acton Town
//! to King's Cross) has both: an Uxbridge-like sparse branch against a
//! frequent trunk.

use chrono::Days;
use common::transit::{
    Direction, Horizon, NetworkId, StopKind, TimetableCommit, TimetableCounts, TimetablePart,
    TimetablePartKind, TimetableRows, TimetableSource, TransitAttribution, TransitLine,
    TransitLineDirection, TransitNetworkRow, TransitPattern, TransitPatternStop,
    TransitServiceDays, TransitStop, TransitTrip,
};
use ds_store::transit::PartIndex;
use serde_json::Value;

use super::*;

const LINE: &str = "testpiccadilly234";
const UXB: &str = "940GZZTWUXB";
const RYL: &str = "940GZZTWRYL";
const ACT: &str = "940GZZTWACT";
const HNX: &str = "940GZZTWHNX";
const HRC: &str = "940GZZTWHRC";
const KSX: &str = "940GZZTWKSX";
const FEED_VERSION: &str = "test-ds234";

/// The date the network has a timetable for.
fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 19).unwrap()
}

/// A date it has none for.
fn other_date() -> NaiveDate {
    date().checked_add_days(Days::new(1)).unwrap()
}

/// `(stop id, name, (latitude, longitude), hub CRS)`.
type SeedStop = (&'static str, &'static str, (f64, f64), Option<&'static str>);

const STOPS: [SeedStop; 6] = [
    (UXB, "Test Uxbridge", (57.5, 3.0), None),
    (RYL, "Test Rayners Lane", (57.5, 3.12), None),
    (ACT, "Test Acton Town", (57.42, 3.25), Some("ZWA")),
    (HNX, "Test Hatton Cross", (57.38, 3.1), None),
    (HRC, "Test Heathrow", (57.37, 3.02), Some("ZWH")),
    (KSX, "Test King's Cross", (57.45, 3.5), None),
];

/// `(crs, tiploc, (latitude, longitude))`.
const STATIONS: [(&str, &str, (f64, f64)); 2] = [
    ("ZWA", "TTWACTN", (57.42, 3.25)),
    ("ZWH", "TTWHTHR", (57.37, 3.02)),
];

/// The two westbound routes, out from the branch ends, with the seconds
/// from each stop to the next (`tfl_access_times`'s).
const ROUTES: [(&[&str], &[i32]); 2] = [
    (&[UXB, RYL, ACT, KSX], &[14 * 60, 23 * 60, 32 * 60]),
    (&[HRC, HNX, ACT, KSX], &[6 * 60, 20 * 60, 32 * 60]),
];

/// A stopping pattern of the timetable: `(id, stops, minutes from the
/// first to each, first departure, every so many minutes)`, until 23:00.
type Pattern = (&'static str, [&'static str; 4], [i32; 4], i32, i32);

/// The timetable's stopping patterns.
const PATTERNS: [Pattern; 4] = [
    // Uxbridge to King's Cross every 20 (at Acton Town :17, :37, :57).
    ("w-uxb", [UXB, RYL, ACT, KSX], [0, 14, 37, 69], 6 * 60, 20),
    // Heathrow to King's Cross every 5 (at Acton Town :00, :05...).
    ("w-hrc", [HRC, HNX, ACT, KSX], [0, 6, 26, 58], 6 * 60 + 9, 5),
    // King's Cross to Uxbridge every 20 (at Acton Town :37, :57, :17).
    (
        "e-uxb",
        [KSX, ACT, RYL, UXB],
        [0, 32, 55, 69],
        6 * 60 + 5,
        20,
    ),
    // King's Cross to Heathrow every 10 (at Acton Town :02, :12...).
    ("e-hrc", [KSX, ACT, HNX, HRC], [0, 32, 52, 58], 6 * 60, 10),
];

/// `940GZZTWUXB` -> `9400ZZTWUXB`, the network's spelling.
fn node(stop: &str) -> String {
    format!("9400{}", &stop[4..])
}

/// The `tfl` network's feed: [`PATTERNS`] on [`date`].
#[expect(clippy::too_many_lines, reason = "one literal feed, part by part")]
fn feed() -> (Vec<(TimetablePart, PartIndex)>, TimetableCommit) {
    let network = NetworkId::parse("tfl").unwrap();
    let one = PartIndex { part: 1, parts: 1 };
    let part = |rows: TimetableRows| {
        (
            TimetablePart {
                network: network.clone(),
                feed_version: FEED_VERSION.to_owned(),
                rows,
            },
            one,
        )
    };
    let mut stops = Vec::new();
    for (id, name, (lat, lon), _) in STOPS {
        for (stop_id, kind, parent) in [
            (node(id), StopKind::Station, None),
            (format!("{}1", node(id)), StopKind::Platform, Some(node(id))),
        ] {
            stops.push(TransitStop {
                stop_id,
                kind,
                parent_stop_id: parent,
                name: name.to_owned(),
                indicator: None,
                lat: Some(lat),
                lon: Some(lon),
                wheelchair: None,
            });
        }
    }
    let mut patterns = Vec::new();
    let mut trips = Vec::new();
    for (id, calls, minutes, first, every) in PATTERNS {
        let last = calls.len() - 1;
        patterns.push(TransitPattern {
            pattern_id: id.to_owned(),
            line_id: Some(LINE.to_owned()),
            direction: if id.starts_with('w') {
                Direction::Outbound
            } else {
                Direction::Inbound
            },
            headsign: None,
            stops: calls
                .iter()
                .zip(minutes)
                .enumerate()
                .map(|(i, (stop, at))| TransitPatternStop {
                    stop_id: format!("{}1", node(stop)),
                    arr_offset_s: at * 60,
                    dep_offset_s: at * 60,
                    pickup: u8::from(i == last),
                    drop_off: u8::from(i == 0),
                })
                .collect(),
        });
        let mut start = first;
        while start <= 23 * 60 {
            trips.push(TransitTrip {
                trip_id: format!("{id}-{start}"),
                line_id: Some(LINE.to_owned()),
                pattern_id: id.to_owned(),
                service_id: "s1".to_owned(),
                start_s: start * 60,
                headsign: None,
                replacement_bus: false,
            });
            start += every;
        }
    }
    let line = TransitLine {
        line_id: LINE.to_owned(),
        name: "Piccadilly".to_owned(),
        colour: None,
        sort_order: 1,
        mode: None,
        directions: vec![TransitLineDirection {
            direction: Direction::Outbound,
            headsign: None,
            stations: vec![node(UXB), node(ACT), node(KSX)],
        }],
    };
    let counts = TimetableCounts {
        lines: 1,
        stops: u32::try_from(stops.len()).unwrap(),
        patterns: u32::try_from(patterns.len()).unwrap(),
        trips: u32::try_from(trips.len()).unwrap(),
        service_days: 1,
    };
    let parts = vec![
        part(TimetableRows::Network(vec![TransitNetworkRow {
            name: "Test TfL".to_owned(),
            mode: "metro".to_owned(),
            timezone: "Europe/London".to_owned(),
            attribution: TransitAttribution {
                licence: "OGL-UK-3.0".to_owned(),
                text: "Test.".to_owned(),
            },
        }])),
        part(TimetableRows::Lines(vec![line])),
        part(TimetableRows::Stops(stops)),
        part(TimetableRows::Patterns(patterns)),
        part(TimetableRows::Trips(trips)),
        part(TimetableRows::ServiceDays(vec![TransitServiceDays {
            service_id: "s1".to_owned(),
            dates: vec![date()],
        }])),
        part(TimetableRows::RailLinks(Vec::new())),
    ];
    let commit = TimetableCommit {
        network,
        feed_version: FEED_VERSION.to_owned(),
        content_sha256: "b".repeat(64),
        parts: TimetablePartKind::ALL.iter().map(|k| (*k, 1)).collect(),
        counts,
        horizon: Horizon {
            from: date(),
            to: date(),
        },
        source: TimetableSource {
            url_kind: "test".to_owned(),
            feed_version: None,
            published_at: None,
            refresh: None,
        },
    };
    (parts, commit)
}

async fn cleanup(pool: &PgPool) {
    ds_store::test_support::transit::cleanup(pool, "tfl").await;
    sqlx::query("DELETE FROM tfl_stops WHERE stop_id LIKE '940GZZTW%'")
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
    sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTTFLWAIT-%'")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TTWEARLY'")
        .execute(pool)
        .await
        .ok();
}

/// The stops, stations, routes (timed) and a schedule for both dates, and
/// the network's timetable for [`date`].
async fn seed(pool: &PgPool) {
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
        .bind(format!("TESTTFLWAIT-{crs}"))
        .bind(crs)
        .bind(tiploc)
        .execute(pool)
        .await
        .expect("seed stanox_crs");
    }
    // A schedule for each date (a plan needs one): one early train
    // between the two stations, long gone by the plans' 10:00.
    for service_date in [date(), other_date()] {
        for (seq, tiploc, kind, arrival, departure) in [
            (0, "TTWACTN", "origin", None, Some("05:00:00")),
            (1, "TTWHTHR", "terminate", Some("05:20:00"), None),
        ] {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, 'TTWEARLY', $2, $3, $4, $5::time, $6::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(service_date)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(pool)
            .await
            .expect("seed the early train");
        }
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
            .bind(seconds)
            .execute(pool)
            .await
            .expect("seed a route");
            index += 1;
        }
    }
    let (parts, commit) = feed();
    ds_store::test_support::transit::seed_feed(pool, &parts, &commit, date()).await;
}

/// The plan's one leg, a ride straight from `origin` to `destination`, on
/// `service_date` at `time` (`departAfter=10:00`, say).
async fn direct_leg(
    router: axum::Router,
    origin: &str,
    destination: &str,
    service_date: NaiveDate,
    time: &str,
) -> Value {
    let uri = format!(
        "/Trips/plan?origin={origin}&destination={destination}&date={service_date}&{time}&live=false"
    );
    let (status, body) = get(router, uri).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    let legs = body["segments"][0]["itineraries"][0]["legs"]
        .as_array()
        .unwrap_or_else(|| panic!("no itinerary: {body:?}"));
    assert_eq!(legs.len(), 1, "{body:?}");
    assert_eq!(legs[0]["access"]["end"], "both", "{body:?}");
    legs[0].clone()
}

/// Uxbridge to Heathrow at 10:00: 2 minutes in, 10 waiting for the
/// Uxbridge branch's train (every 20), 37 to Acton Town, the change there
/// (5, and 5 waiting for a Heathrow train, every 10) and 26 on: 85, where
/// the constant waits (5 in, 5 at the change) make 78. Back from Heathrow
/// the first wait is 3 (every 5, 2.5 rounded) and the change's 10: 83.
/// On the trunk, Acton Town to King's Cross, a train every 5 minutes and
/// every Uxbridge one between them: 2 waiting (2.2), 2 in, 32 riding: 36
/// where the constant made 37. A date the network has no timetable for
/// keeps the constants.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn access_rides_wait_by_the_timetables_headways() {
    let pool = connect().await;
    let router = test_router(test_app(pool.clone()));
    seed(&pool).await;

    let at_ten = "departAfter=10:00";
    for (from, to, minutes) in [(UXB, HRC, 85), (HRC, UXB, 83)] {
        let leg = direct_leg(router.clone(), from, to, date(), at_ten).await;
        assert_eq!(leg["access"]["changeAt"]["id"], ACT, "{leg:?}");
        assert_eq!(leg["minutes"], minutes, "{from} to {to}: {leg:?}");
    }
    let leg = direct_leg(router.clone(), ACT, KSX, date(), at_ten).await;
    assert_eq!(leg["minutes"], 36, "{leg:?}");
    // Arriving by 12:00 averages over the same two hours.
    let leg = direct_leg(router.clone(), UXB, HRC, date(), "arriveBy=12:00").await;
    assert_eq!(leg["minutes"], 85, "{leg:?}");

    for (from, to, minutes) in [(UXB, HRC, 78), (HRC, UXB, 78), (ACT, KSX, 37)] {
        let leg = direct_leg(router.clone(), from, to, other_date(), at_ten).await;
        assert_eq!(leg["minutes"], minutes, "{from} to {to}: {leg:?}");
    }
    cleanup(&pool).await;
}
