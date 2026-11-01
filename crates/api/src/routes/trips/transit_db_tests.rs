//! `/Trips/plan` with a transit network (DS-164) against a live database:
//! the golden test (no opt-in, or the switch off, is byte-identical to the
//! switch off), a rail-walk-tram plan to a tram stop, the errors, the
//! horizon notice and the line disruption notice.
//!
//! The network is `ds_store::test_support::transit`'s hand-made one under a
//! test-only id: line `blue` from `9400ZZTTAAA` to `9400ZZTTBBB` (30
//! minutes; trams at 06:00, 07:00, 23:50 and 00:10, back at 07:30, a
//! replacement bus at 08:00), service yesterday and today, and a rail link
//! from `9400ZZTTAAA` to the rail station `ZZT` (`ZZTEST`). Here a train
//! `ZZR` (`ZZRAIL`) 06:30 to `ZZT` 06:50 feeds it, on both service dates.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use ds_store::test_support::transit::{B, apply_situations, cleanup, seed, situation};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::data::transit_planning::set_switch_for_tests;

const NETWORK: &str = "test-trips";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
}

fn tomorrow() -> NaiveDate {
    today().succ_opt().unwrap()
}

async fn get_raw(pool: &PgPool, uri: &str) -> (StatusCode, Vec<u8>) {
    let app = crate::test_support::inert_app(pool.clone());
    let router: axum::Router = crate::app::Router::new()
        .merge(super::router())
        .with_state(app);
    let response = router
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

async fn get(pool: &PgPool, uri: &str) -> (StatusCode, Value) {
    let (status, bytes) = get_raw(pool, uri).await;
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

async fn seed_rail(pool: &PgPool) {
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    cleanup_rail(pool).await;
    for date in [today(), tomorrow()] {
        for (seq, tiploc, kind, arrival, departure) in [
            (0_i16, "ZZRAIL", "origin", None, Some("06:30:00")),
            (1, "ZZTEST", "terminate", Some("06:50:00"), None),
        ] {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, 'TESTTR1', $2, $3, $4, $5::time, $6::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(date)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(&owner)
            .await
            .expect("seed calling point");
        }
    }
    sqlx::query(
        "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, change_time_minutes, \
             source_sequence) \
         VALUES ('TESTTRANSIT-ZZR', 'ZZR', 'ZZRAIL', 'ZED RAIL', NULL, 1), \
                ('TESTTRANSIT-ZZT', 'ZZT', 'ZZTEST', 'ZED TEST', NULL, 1) \
         ON CONFLICT (stanox) DO NOTHING",
    )
    .execute(&owner)
    .await
    .expect("seed stanox_crs");
    // At the tram stop's own coordinates: the 2-minute interchange floor.
    sqlx::query(
        "INSERT INTO stations (crs, name, latitude, longitude) \
         VALUES ('ZZT', 'Zed Test', 53.4, -2.2) ON CONFLICT (crs) DO NOTHING",
    )
    .execute(&owner)
    .await
    .expect("seed stations");
}

async fn cleanup_rail(pool: &PgPool) {
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    for sql in [
        "DELETE FROM schedule_calling_points_full WHERE uid = 'TESTTR1'",
        "DELETE FROM stanox_crs WHERE stanox LIKE 'TESTTRANSIT-%'",
        "DELETE FROM stations WHERE crs = 'ZZT'",
    ] {
        sqlx::query(sql).execute(&owner).await.ok();
    }
}

fn plan_uri(origin: &str, destination: &str, date: NaiveDate, extra: &str) -> String {
    format!(
        "/Trips/plan?origin={origin}&destination={destination}&date={date}&departAfter=06:00&live=false{extra}"
    )
}

/// The golden test (spec §14): with the switch empty, or without the
/// opt-in, every response is byte-identical to today's, for plans, for an
/// unknown `9400` end and for an invalid `transit` value with the switch
/// off.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn without_the_switch_or_the_opt_in_responses_are_byte_identical() {
    let pool = ds_store::test_support::connect().await;
    seed(&pool, NETWORK, today()).await;
    seed_rail(&pool).await;
    let uris = [
        plan_uri("ZZR", "ZZT", today(), ""),
        plan_uri("ZZR", "ZZT", today(), "&results=options&maxChanges=3"),
        plan_uri("ZZR", "ZZT", today(), "&arriveBy=07:00").replace("&departAfter=06:00", ""),
        plan_uri("ZZR", "ZZT", tomorrow(), ""),
        plan_uri("ZZR", "9400ZZTTBBB", today(), ""),
    ];
    let mut off = Vec::new();
    for uri in &uris {
        off.push(get_raw(&pool, uri).await);
    }
    // The parameter is ignored while the switch is off.
    for (uri, before) in uris.iter().zip(&off) {
        assert_eq!(
            &get_raw(&pool, &format!("{uri}&transit=true")).await,
            before,
            "{uri}"
        );
        assert_eq!(
            &get_raw(&pool, &format!("{uri}&transit=maybe")).await,
            before,
            "{uri}"
        );
    }
    {
        let _switch = set_switch_for_tests(&[NETWORK]);
        // Station ends, no opt-in: as with the switch off.
        for (uri, before) in uris.iter().zip(&off).take(4) {
            assert_eq!(&get_raw(&pool, uri).await, before, "{uri}");
            assert_eq!(
                &get_raw(&pool, &format!("{uri}&transit=false")).await,
                before,
                "{uri}"
            );
        }
    }
    assert_eq!(off[0].0, StatusCode::OK);
    cleanup(&pool, NETWORK).await;
    cleanup_rail(&pool).await;
}

/// Rail into ZZT at 06:50, a 2-minute walk plus the 2-minute tram buffer,
/// the 3-minute platform change: the 07:00 tram to the tram stop end.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_tram_stop_end_rides_the_tram_scheduled_only() {
    let pool = ds_store::test_support::connect().await;
    seed(&pool, NETWORK, today()).await;
    seed_rail(&pool).await;
    let _switch = set_switch_for_tests(&[NETWORK]);

    // Lower case, and the stop-area spelling: the station node is echoed.
    for destination in ["9400zzttbbb", "940GZZTTBBB", "9400ZZTTBBB1"] {
        let (status, body) = get(&pool, &plan_uri("ZZR", destination, today(), "")).await;
        assert_eq!(status, StatusCode::OK, "{destination}: {body}");
        assert_eq!(body["segments"][0]["destinationCrs"], B, "{destination}");
    }
    let (status, body) = get(&pool, &plan_uri("ZZR", B, today(), "")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["transitNotices"], serde_json::json!([]));
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_eq!(itinerary["scheduledOnly"], true);
    assert_eq!(itinerary["changeCount"], 1);
    let legs = itinerary["legs"].as_array().unwrap();
    assert_eq!(legs.len(), 3, "{legs:?}");
    assert_eq!(legs[0]["kind"], "train");
    assert_eq!(legs[1]["kind"], "transfer");
    assert_eq!(legs[1]["mode"], "WALK");
    assert_eq!(legs[1]["originCrs"], "ZZT");
    assert_eq!(legs[1]["destinationCrs"], Value::Null);
    assert_eq!(legs[1]["minutes"], 4);
    assert_eq!(
        legs[1]["transitWalk"],
        serde_json::json!({
            "network": NETWORK,
            "stop": {"id": "9400ZZTTAAA", "name": "Aaa"},
            "towardsStop": true,
            "walkMinutes": 2,
            "estimated": true,
        })
    );
    let tram = &legs[2];
    assert_eq!(tram["kind"], "transit");
    assert_eq!(tram["serviceMode"], "tram");
    assert_eq!(tram["realtime"], false);
    assert_eq!(tram["liveTracking"], false);
    assert_eq!(tram["scheduledDeparture"], "07:00:00");
    assert_eq!(tram["scheduledArrival"], "07:30:00");
    assert_eq!(tram["serviceDate"], "2026-10-14");
    assert_eq!(
        tram["line"],
        serde_json::json!({"id": format!("{NETWORK}:blue"), "name": "Blue Line", "colour": "#1f6fb5"})
    );
    assert_eq!(tram["originStop"]["id"], "9400ZZTTAAA");
    assert_eq!(tram["originStop"]["platform"], "To the city");
    assert_eq!(tram["destinationStop"]["id"], B);
    assert_eq!(tram["originCrs"], Value::Null);
    assert_eq!(tram["notices"], serde_json::json!([]));
    // Two trams in the hour around 07:00 from that platform: no hint.
    assert!(tram.get("headwayMinutes").is_none());
    for field in [
        "live",
        "expected",
        "delay",
        "cancelled",
        "status",
        "trainUid",
        "publicDeparture",
        "workingDeparture",
        "operator",
        "headcode",
    ] {
        assert!(tram.get(field).is_none(), "{field} on {tram}");
    }

    // A rail end with `transit=true` rides it too; arrive-by finds it.
    let (status, body) = get(
        &pool,
        &plan_uri("ZZR", B, today(), "&arriveBy=07:45").replace("&departAfter=06:00", ""),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["segments"][0]["itineraries"][0]["legs"][2]["scheduledDeparture"],
        "07:00:00"
    );

    // A disruption on the line becomes the leg's notice.
    let now = chrono::Utc::now();
    apply_situations(
        &pool,
        NETWORK,
        vec![situation("S1", "blue", "delayed", None, now)],
        now,
    )
    .await;
    let (_, body) = get(&pool, &plan_uri("ZZR", B, today(), "")).await;
    let notices = &body["segments"][0]["itineraries"][0]["legs"][2]["notices"];
    assert_eq!(notices[0]["kind"], "disruption", "{notices}");
    assert_eq!(notices[0]["lineId"], format!("{NETWORK}:blue"));
    assert_eq!(
        notices[0]["message"],
        "Disruption reported on the Blue Line"
    );

    cleanup(&pool, NETWORK).await;
    cleanup_rail(&pool).await;
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn tram_stop_errors_and_the_horizon_notice() {
    let pool = ds_store::test_support::connect().await;
    seed(&pool, NETWORK, today()).await;
    seed_rail(&pool).await;
    let _switch = set_switch_for_tests(&[NETWORK]);

    let (status, body) = get(
        &pool,
        &plan_uri("ZZR", "9400ZZXXXXX", today(), "&transit=true"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "'9400ZZXXXXX' is not a stop the planner knows");
    let (status, _) = get(&pool, &plan_uri(B, "9400ZZTTBBB1", today(), "")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = get(
        &pool,
        &plan_uri("ZZR", "ZZT", today(), &format!("&transit=true&via={B}")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = get(&pool, &plan_uri("ZZR", "ZZT", today(), "&transit=maybe")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Tomorrow is past the feed's horizon: rail only, said so.
    let (status, body) = get(&pool, &plan_uri("ZZR", "ZZT", tomorrow(), "&transit=true")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["transitNotices"],
        serde_json::json!([{
            "network": NETWORK,
            "reason": "noTimetable",
            "message": "Tram times aren't available for this date.",
        }])
    );
    assert_eq!(
        body["segments"][0]["itineraries"][0]["scheduledOnly"],
        false
    );
    // A tram stop end on that date cannot be planned.
    let (status, _) = get(&pool, &plan_uri("ZZR", B, tomorrow(), "")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    cleanup(&pool, NETWORK).await;
    cleanup_rail(&pool).await;
}

// --- The `TfL` network (the `TfL` transit design §11) --------------------------

/// The seed's network as `tfl` (a `tfl_stops` planner network): its
/// stations are `TfL` stops `940GZZTTAAA` (the Underground; its rail station
/// `ZZT` by `tfl_stop_crs`) and `940GZZTTBBB` (the DLR), in one hub when
/// `hub` says so, and its line is a Tube line.
async fn seed_tfl(pool: &PgPool, hub: bool) {
    seed(pool, "tfl", today()).await;
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    cleanup_tfl_stops(pool).await;
    let hub_code = hub.then_some("HUBTEST");
    for (stop, name, mode) in [
        ("940GZZTTAAA", "Aaa", "tube"),
        ("940GZZTTBBB", "Bbb", "dlr"),
    ] {
        sqlx::query(
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, hub_code, \
                latitude, longitude, fetched_at) \
             VALUES ($1, $2, lower($2), ARRAY[$3], '[]', $4, 53.4, -2.2, now())",
        )
        .bind(stop)
        .bind(name)
        .bind(mode)
        .bind(hub_code)
        .execute(&owner)
        .await
        .expect("seed tfl_stops");
    }
    sqlx::query(
        "INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ('940GZZTTAAA', 'ZZT', 'hub')",
    )
    .execute(&owner)
    .await
    .expect("seed tfl_stop_crs");
    sqlx::query("UPDATE transit_lines SET mode = 'tube' WHERE network = 'tfl'")
        .execute(&owner)
        .await
        .expect("line mode");
}

async fn cleanup_tfl_stops(pool: &PgPool) {
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    for sql in [
        "DELETE FROM tfl_stops WHERE stop_id IN ('940GZZTTAAA', '940GZZTTBBB')",
        "DELETE FROM line_status WHERE line_id = 'tfl-blue'",
    ] {
        sqlx::query(sql).execute(&owner).await.ok();
    }
}

async fn cleanup_tfl(pool: &PgPool) {
    cleanup_tfl_stops(pool).await;
    cleanup(pool, "tfl").await;
    cleanup_rail(pool).await;
}

/// With `transit=true`, a `TfL` stop end is the network's node: rail into
/// ZZT, its `TfL` station by `tfl_stop_crs`, and a Tube leg, named in `TfL`'s
/// spelling, with `TfL`'s own line status as its notice.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_tfl_stop_end_rides_the_tube_timetable_when_opted_in() {
    let pool = ds_store::test_support::connect().await;
    seed_tfl(&pool, false).await;
    seed_rail(&pool).await;
    let owner = ds_store::test_support::schedule_fixture_pool(&pool).await;
    sqlx::query(
        "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
         VALUES ('tfl-blue', 'Blue', 'tube', '{TfL}', \
           '[{\"severity\": 9, \"reason\": \"Signal failure\", \"validity\": \
             {\"from_date\": \"2026-10-14T00:00:00Z\", \"to_date\": null, \"is_now\": true}}]', 'tfl') \
         ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, source = 'tfl'",
    )
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ingest_freshness (source, fetched_at) VALUES ('tfl', now()) \
         ON CONFLICT (source) DO UPDATE SET fetched_at = now()",
    )
    .execute(&owner)
    .await
    .unwrap();
    let _switch = set_switch_for_tests(&["tfl"]);

    let (status, body) = get(
        &pool,
        &plan_uri("ZZR", "940GZZTTBBB", today(), "&transit=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("access").is_none(), "{body}");
    let legs = body["segments"][0]["itineraries"][0]["legs"]
        .as_array()
        .unwrap();
    assert_eq!(legs.len(), 3, "{legs:?}");
    assert_eq!(legs[1]["kind"], "transfer");
    assert_eq!(legs[1]["originCrs"], "ZZT");
    assert_eq!(legs[1]["transitWalk"]["stop"]["id"], "940GZZTTAAA");
    let tube = &legs[2];
    assert_eq!(tube["kind"], "transit");
    assert_eq!(tube["network"], "tfl");
    assert_eq!(tube["serviceMode"], "tube");
    assert_eq!(tube["scheduledDeparture"], "07:00:00");
    assert_eq!(tube["originStop"]["id"], "940GZZTTAAA");
    assert_eq!(tube["destinationStop"]["id"], "940GZZTTBBB");
    assert_eq!(tube["line"]["id"], "tfl:blue");
    let notice = &tube["notices"][0];
    assert_eq!(notice["kind"], "disruption", "{tube}");
    assert_eq!(notice["description"], "Minor Delays");
    assert_eq!(notice["reason"], "Signal failure");

    // The Tube trips' connections are London Underground's (`LT`) for an
    // operator filter (operator filtering spec §6.2); the seed's
    // replacement bus has no line, so no mode, and stays unknown (it passes
    // every filter).
    let app = crate::test_support::inert_app(pool.clone());
    let graph = super::load_graph(
        &app,
        today(),
        crate::data::trip_planning::GraphMarker::default(),
    )
    .await
    .unwrap()
    .expect("a graph");
    let transit = graph.transit.as_ref().expect("the transit graph");
    let mut tube_connections = 0;
    for connection in graph
        .connections
        .iter()
        .filter(|connection| connection.uid.starts_with("tfl:"))
    {
        let (_, trip) = transit.trip(&connection.uid).expect("a tfl trip");
        let expected = if trip.replacement_bus {
            schedule_query::OperatorId::UNKNOWN
        } else {
            tube_connections += 1;
            schedule_query::OperatorId::LONDON_UNDERGROUND
        };
        assert_eq!(connection.operator, expected, "{}", connection.uid);
    }
    assert!(tube_connections > 0);

    cleanup_tfl(&pool).await;
}

/// Without the opt-in, and on a date the timetable doesn't cover, a `TfL`
/// stop end stays a `TfL` stop: `tfl_access`'s model plans it, as before the
/// network (no 400, no transit legs).
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_tfl_stop_end_falls_back_to_the_access_model() {
    let pool = ds_store::test_support::connect().await;
    seed_tfl(&pool, false).await;
    seed_rail(&pool).await;
    let _switch = set_switch_for_tests(&["tfl"]);

    for uri in [
        plan_uri("ZZR", "940GZZTTBBB", today(), ""),
        plan_uri("ZZR", "940GZZTTBBB", tomorrow(), "&transit=true"),
    ] {
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert!(body.get("access").is_some(), "{uri}: {body}");
        let legs = body["segments"][0]["itineraries"][0]["legs"]
            .as_array()
            .unwrap();
        assert!(
            legs.iter().all(|leg| leg["kind"] != "transit"),
            "{uri}: {legs:?}"
        );
    }

    cleanup_tfl(&pool).await;
}

/// Two stations of one `TfL` hub (Bank's Underground and DLR) are joined by
/// a walk, served with both ends as `TfL` stops.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_walk_between_one_hubs_stations_names_both_tfl_stops() {
    let pool = ds_store::test_support::connect().await;
    seed_tfl(&pool, true).await;
    seed_rail(&pool).await;
    let _switch = set_switch_for_tests(&["tfl"]);

    let (status, body) = get(
        &pool,
        &plan_uri("ZZR", "940GZZTTBBB", today(), "&transit=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let legs = body["segments"][0]["itineraries"][0]["legs"]
        .as_array()
        .unwrap();
    let hub = legs
        .iter()
        .find(|leg| leg["kind"] == "transfer" && leg.get("transitWalk").is_none())
        .unwrap_or_else(|| panic!("no hub walk: {legs:?}"));
    assert_eq!(hub["mode"], "WALK");
    assert_eq!(hub["originCrs"], Value::Null);
    assert_eq!(hub["destinationCrs"], Value::Null);
    assert_eq!(hub["originStop"]["id"], "940GZZTTAAA");
    assert_eq!(hub["originStop"]["modes"], serde_json::json!(["tube"]));
    assert_eq!(hub["destinationStop"]["id"], "940GZZTTBBB");
    assert_eq!(hub["minutes"], 2);

    cleanup_tfl(&pool).await;
}

/// DS-225: the graph cache's key is the schedule publish marker and the
/// transit feeds, so a new publish or a feed activation rebuilds a cached
/// graph, and nothing else before the max age does; the prewarm builds
/// today's and tomorrow's graphs ahead of requests, only with a planning
/// slot free; and both count in `api_trip_plan_graph_builds_total`.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario in order: startup, a publish, a feed, a busy process, the metrics"
)]
async fn the_graph_cache_follows_publishes_and_feeds_and_prewarms() {
    use crate::data::trip_planning::{BuildReason, CacheOutcome, GraphCache};

    let pool = ds_store::test_support::connect().await;
    seed(&pool, NETWORK, today()).await;
    seed_rail(&pool).await;
    let owner = ds_store::test_support::schedule_fixture_pool(&pool).await;
    let cleanup_publishes = || async {
        sqlx::query("DELETE FROM schedule_reference_publishes WHERE delivery LIKE 'TEST-DS225-%'")
            .execute(&owner)
            .await
            .expect("clean up publishes");
    };
    cleanup_publishes().await;
    let publish = |n: u32| {
        let owner = owner.clone();
        async move {
            sqlx::query(
                "INSERT INTO schedule_reference_publishes (delivery, completed_at) \
                 SELECT $1, COALESCE(max(completed_at), now()) + interval '1 second' \
                 FROM schedule_reference_publishes",
            )
            .bind(format!("TEST-DS225-{n}"))
            .execute(&owner)
            .await
            .expect("record a publish");
        }
    };
    let _switch = set_switch_for_tests(&[NETWORK]);
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    let _recorder = metrics::set_default_local_recorder(&recorder);
    super::register_graph_metrics();

    let app = crate::test_support::inert_app(pool.clone());
    let cache = std::sync::Arc::new(GraphCache::new(2, std::time::Duration::from_secs(3600)));
    let slots = tokio::sync::Semaphore::new(1);
    let prewarm = || super::prewarm_once(&cache, &app, today(), &slots);

    // Startup: both dates built, then nothing to do.
    assert_eq!(
        prewarm().await.unwrap(),
        [
            (today(), BuildReason::Absent),
            (tomorrow(), BuildReason::Absent)
        ]
    );
    assert_eq!(prewarm().await.unwrap(), []);
    let marker = super::read_graph_marker(&pool, &[NETWORK.to_string()])
        .await
        .unwrap();
    let feed = &marker.transit.as_ref().expect("the feed read")[0];
    assert_eq!(feed.network, NETWORK);
    assert!(feed.feed_version.is_some(), "{marker:?}");
    let (_, outcome) =
        super::load_graph_in(&cache, &app, today(), marker, super::BuildTrigger::Request)
            .await
            .unwrap()
            .expect("a graph");
    assert_eq!(outcome, CacheOutcome::Hit, "a request after the prewarm");

    // A completed publish rebuilds both.
    publish(1).await;
    assert_eq!(
        prewarm().await.unwrap(),
        [
            (today(), BuildReason::Marker),
            (tomorrow(), BuildReason::Marker)
        ]
    );

    // So does the network's feed being activated again.
    sqlx::query(
        "UPDATE transit_networks SET activated_at = activated_at + interval '1 minute' \
         WHERE network = $1",
    )
    .bind(NETWORK)
    .execute(&owner)
    .await
    .expect("re-activate the feed");
    assert_eq!(
        prewarm().await.unwrap(),
        [
            (today(), BuildReason::Marker),
            (tomorrow(), BuildReason::Marker)
        ]
    );

    // With every planning slot taken, the prewarm leaves the stale graphs
    // for later rather than queue.
    publish(2).await;
    {
        let _busy = slots.try_acquire().unwrap();
        assert_eq!(prewarm().await.unwrap(), []);
    }
    assert_eq!(
        prewarm().await.unwrap(),
        [
            (today(), BuildReason::Marker),
            (tomorrow(), BuildReason::Marker)
        ]
    );

    let rendered = handle.render();
    for (reason, trigger, count) in [
        ("absent", "prewarm", 2),
        ("marker", "prewarm", 6),
        ("expired", "prewarm", 0),
        ("absent", "request", 0),
    ] {
        let series = format!(
            "distant_signal_api_trip_plan_graph_builds_total{{reason=\"{reason}\",trigger=\"{trigger}\"}} {count}"
        );
        assert!(rendered.contains(&series), "{series} in\n{rendered}");
    }
    assert!(
        rendered
            .contains("distant_signal_api_trip_plan_graph_build_seconds_count{phase=\"read\"} 8"),
        "{rendered}"
    );

    cleanup_publishes().await;
    cleanup(&pool, NETWORK).await;
    cleanup_rail(&pool).await;
}

/// DS-235: two graphs for one date, from Postgres and from the timetable
/// snapshot, are the same graph: the same connections in the same order,
/// pass rows, operators, bus and ferry buffer, and transit part.
fn assert_same_graph(
    date: NaiveDate,
    expected: Option<&crate::data::trip_planning::PlanningGraph>,
    actual: Option<&crate::data::trip_planning::PlanningGraph>,
) {
    let (Some(expected), Some(actual)) = (expected, actual) else {
        assert_eq!(expected.is_some(), actual.is_some(), "{date}");
        return;
    };
    assert_eq!(expected.connections, actual.connections, "{date}");
    assert_eq!(expected.passes, actual.passes, "{date}");
    assert_eq!(expected.operators, actual.operators, "{date}");
    assert_eq!(
        expected.interchange.modal_change, actual.interchange.modal_change,
        "{date}"
    );
    assert_eq!(expected.running, actual.running, "{date}");
    let (Some(expected), Some(actual)) = (&expected.transit, &actual.transit) else {
        assert_eq!(
            expected.transit.is_some(),
            actual.transit.is_some(),
            "{date}"
        );
        return;
    };
    assert_eq!(expected.networks, actual.networks, "{date}");
    assert_eq!(expected.skipped, actual.skipped, "{date}");
    assert_eq!(expected.stops, actual.stops, "{date}");
    assert_eq!(
        expected.interchange.modal_change, actual.interchange.modal_change,
        "{date}"
    );
}

async fn seed_services(pool: &PgPool) {
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    for date in [today(), tomorrow()] {
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, operator_atoc, stp) \
             VALUES ($1, 'TESTTR1', 'train', 'VT', 'P'), ($1, 'TESTBUS', 'bus', NULL, 'P') \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&owner)
        .await
        .expect("seed services");
    }
}

async fn cleanup_snapshots(pool: &PgPool) {
    let owner = ds_store::test_support::schedule_fixture_pool(pool).await;
    for sql in [
        "DELETE FROM schedule_services WHERE uid IN ('TESTTR1', 'TESTBUS')",
        "DELETE FROM planner_snapshots",
        "DELETE FROM schedule_reference_publishes WHERE delivery LIKE 'TEST-DS235-%'",
    ] {
        sqlx::query(sql).execute(&owner).await.expect(sql);
    }
    crate::data::planner_snapshot::uninstall_for_tests();
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_graph_from_the_snapshot_is_the_graph_from_postgres() {
    use crate::data::planner_snapshot;

    let pool = ds_store::test_support::connect().await;
    cleanup_snapshots(&pool).await;
    seed(&pool, NETWORK, today()).await;
    seed_rail(&pool).await;
    seed_services(&pool).await;
    let _switch = set_switch_for_tests(&[NETWORK]);
    let networks = vec![NETWORK.to_string()];
    let marker = super::read_graph_marker(&pool, &networks).await.unwrap();
    let dates: Vec<NaiveDate> = (-1..=2)
        .map(|offset| today() + chrono::Duration::days(offset))
        .collect();
    let mut from_postgres = Vec::new();
    for &date in &dates {
        from_postgres.push(
            super::build_planning_graph(&pool, date, &networks, &marker)
                .await
                .unwrap(),
        );
    }
    assert!(from_postgres[1].is_some() && from_postgres[2].is_some());

    // Built, stored, read back and decoded unchanged.
    let snapshot = planner_snapshot::build(&pool, &networks).await.unwrap();
    // It spans the dates with calling points; the others fall back to
    // Postgres, and nothing is published there either way.
    for &date in &dates {
        assert_eq!(
            snapshot.offset(date).is_some(),
            date == today() || date == tomorrow(),
            "{date}"
        );
    }
    let version = planner_snapshot::version_for(&marker, &networks);
    let bytes = planner_snapshot::encode(&snapshot).unwrap();
    planner_snapshot::store(&pool, &version, &bytes)
        .await
        .unwrap();
    let stored = planner_snapshot::load(&pool, &version)
        .await
        .unwrap()
        .expect("the stored row");
    assert_eq!(planner_snapshot::decode(&stored.bytes).unwrap(), snapshot);

    // Installed from the stored row, then every date's graph built from it.
    let hour = std::time::Duration::from_secs(3600);
    assert_eq!(
        planner_snapshot::ensure(&pool, &marker, &networks, hour)
            .await
            .unwrap(),
        planner_snapshot::Outcome::Loaded
    );
    assert!(planner_snapshot::current(&marker, &networks).is_some());
    for (date, expected) in dates.iter().zip(&from_postgres) {
        let actual = super::build_planning_graph(&pool, *date, &networks, &marker)
            .await
            .unwrap();
        assert_same_graph(*date, expected.as_ref(), actual.as_ref());
    }

    cleanup_snapshots(&pool).await;
    cleanup(&pool, NETWORK).await;
    cleanup_rail(&pool).await;
}

/// DS-235: the snapshot follows the graph cache's key: built once, current
/// until a new publish, then built again for the new marker (and the old
/// one no longer used); another process loads the stored row; one older
/// than the max age is rebuilt.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn the_snapshot_is_rebuilt_on_a_new_publish_and_loaded_elsewhere() {
    use crate::data::planner_snapshot::{self, Outcome};

    let pool = ds_store::test_support::connect().await;
    cleanup_snapshots(&pool).await;
    seed_rail(&pool).await;
    let networks: Vec<String> = Vec::new();
    let hour = std::time::Duration::from_secs(3600);
    let ensure = |marker: crate::data::trip_planning::GraphMarker, max_age| {
        let pool = pool.clone();
        let networks = networks.clone();
        async move {
            planner_snapshot::ensure(&pool, &marker, &networks, max_age)
                .await
                .unwrap()
        }
    };
    let marker = super::read_graph_marker(&pool, &networks).await.unwrap();
    assert_eq!(ensure(marker.clone(), hour).await, Outcome::Built);
    assert_eq!(ensure(marker.clone(), hour).await, Outcome::Current);

    let owner = ds_store::test_support::schedule_fixture_pool(&pool).await;
    sqlx::query(
        "INSERT INTO schedule_reference_publishes (delivery, completed_at) \
         SELECT 'TEST-DS235-1', COALESCE(max(completed_at), now()) + interval '1 second' \
         FROM schedule_reference_publishes",
    )
    .execute(&owner)
    .await
    .unwrap();
    let published = super::read_graph_marker(&pool, &networks).await.unwrap();
    assert_ne!(published, marker);
    assert!(planner_snapshot::current(&published, &networks).is_none());
    assert_eq!(ensure(published.clone(), hour).await, Outcome::Built);
    assert!(planner_snapshot::current(&published, &networks).is_some());
    assert!(
        planner_snapshot::current(&marker, &networks).is_none(),
        "graphs for the old publish no longer use it"
    );

    // Another process (or this one restarted) loads the stored row.
    planner_snapshot::uninstall_for_tests();
    assert_eq!(ensure(published.clone(), hour).await, Outcome::Loaded);
    // Past the max age, it is built again.
    assert_eq!(
        ensure(published.clone(), std::time::Duration::ZERO).await,
        Outcome::Built
    );

    cleanup_snapshots(&pool).await;
    cleanup_rail(&pool).await;
}
