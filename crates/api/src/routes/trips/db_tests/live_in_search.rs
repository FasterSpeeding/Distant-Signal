//! Later legs planned from the live arrivals of earlier ones (2026-10-22):
//! the in-search live snapshot (`trip_plan_live::LiveSnapshot`) puts every
//! fresh TRUST delay and every cancellation into the very first search, so
//! no plan offers a change that is impossible on live times however small
//! the re-plan budget, and a late train nobody planned on yet can be
//! found. Each test seeds its own synthetic stations and trains for
//! [`live_date`] and pins "now" to [`live_now`] (09:30 BST).

use serde_json::Value;

use super::*;

/// Every train UID of every itinerary of segment `segment`.
fn all_train_uids(body: &Value, segment: usize) -> Vec<Vec<String>> {
    body["segments"][segment]["itineraries"]
        .as_array()
        .expect("itineraries")
        .iter()
        .map(|itinerary| {
            itinerary["legs"]
                .as_array()
                .expect("legs")
                .iter()
                .filter_map(|leg| leg["trainUid"].as_str().map(str::to_string))
                .collect()
        })
        .collect()
}

/// Every itinerary of every segment is feasible on live times, and every
/// change in it is.
fn assert_all_live_feasible(body: &Value) {
    for segment in body["segments"].as_array().expect("segments") {
        for itinerary in segment["itineraries"].as_array().expect("itineraries") {
            assert_eq!(itinerary["liveFeasible"], true, "{itinerary:?}");
            for leg in itinerary["legs"].as_array().expect("legs") {
                let live = &leg["live"];
                if live.is_object() {
                    assert_ne!(live["cancelled"], true, "{leg:?}");
                    assert_ne!(live["interchangeFeasible"], false, "{leg:?}");
                }
            }
        }
    }
}

/// `TLVINS`: ZIA -> ZIB -> ZIC. TLVIN1 runs 15 late into ZIB (10:45), so
/// the 10:40 on is missed; the 11:00 is cancelled; the 11:10 is the answer.
/// Before the snapshot, `results=options` (one re-plan) planned TLVIN1 +
/// TLVIN2, re-planned onto TLVIN1 + TLVIN3, read TLVIN3 only then, and
/// served a cancelled leg.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn options_never_offer_a_change_broken_by_a_train_read_after_the_last_replan() {
    let pool = connect().await;
    let uids = ["TLVIN1", "TLVIN2", "TLVIN3", "TLVIN4"];
    cleanup_live(&pool, &uids, "TLVINS").await;
    seed_stations(
        &pool,
        "TLVINS",
        &[("ZIA", "TLVIA"), ("ZIB", "TLVIB"), ("ZIC", "TLVIC")],
    )
    .await;
    seed_schedule(
        &pool,
        "TLVIN1",
        &[
            (0, "TLVIA", "origin", None, Some("10:00:00")),
            (1, "TLVIB", "terminate", Some("10:30:00"), None),
        ],
    )
    .await;
    for (uid, departs, arrives) in [
        ("TLVIN2", "10:40:00", "11:00:00"),
        ("TLVIN3", "11:00:00", "11:20:00"),
        ("TLVIN4", "11:10:00", "11:30:00"),
    ] {
        seed_schedule(
            &pool,
            uid,
            &[
                (0, "TLVIB", "origin", None, Some(departs)),
                (1, "TLVIC", "terminate", Some(arrives), None),
            ],
        )
        .await;
    }
    seed_trust_state(&pool, "TLVIN1", "en_route", Some(15), live_now()).await;
    seed_trust_state(&pool, "TLVIN3", "cancelled", None, live_now()).await;
    let _now = crate::routes::pin_london_now_for_tests(live_now());

    for results in ["options", "fastest"] {
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            format!(
                "/Trips/plan?origin=ZIA&destination=ZIC&date={}&departAfter=09:45&results={results}",
                live_date()
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["live"]["applied"], true, "{body:?}");
        assert_eq!(
            all_train_uids(&body, 0),
            [["TLVIN1", "TLVIN4"]],
            "{results}: {body:?}"
        );
        assert_all_live_feasible(&body);
        // Known before the first search: nothing to re-plan.
        assert_eq!(body["live"]["replans"], 0, "{results}: {body:?}");
    }

    cleanup_live(&pool, &uids, "TLVINS").await;
}

/// `TLVLAT`: ZKA -> ZKB -> ZKC. The timetable's 10:20 from ZKB leaves
/// before TLVLA1 gets in (10:30), but it is running 20 late (10:40): on
/// live times TLVLA1 + TLVLA2 (11:10) beats TLVLA1 + TLVLA3 (11:30). The
/// re-plan loop only ever read the trains of a plan it had, so it never
/// saw TLVLA2.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_late_onward_train_no_plan_used_yet_is_found_by_the_search() {
    let pool = connect().await;
    let uids = ["TLVLA1", "TLVLA2", "TLVLA3"];
    cleanup_live(&pool, &uids, "TLVLAT").await;
    seed_stations(
        &pool,
        "TLVLAT",
        &[("ZKA", "TLVKA"), ("ZKB", "TLVKB"), ("ZKC", "TLVKC")],
    )
    .await;
    for (uid, from, to, departs, arrives) in [
        ("TLVLA1", "TLVKA", "TLVKB", "10:00:00", "10:30:00"),
        ("TLVLA2", "TLVKB", "TLVKC", "10:20:00", "10:50:00"),
        ("TLVLA3", "TLVKB", "TLVKC", "11:00:00", "11:30:00"),
    ] {
        seed_schedule(
            &pool,
            uid,
            &[
                (0, from, "origin", None, Some(departs)),
                (1, to, "terminate", Some(arrives), None),
            ],
        )
        .await;
    }
    seed_trust_state(&pool, "TLVLA2", "en_route", Some(20), live_now()).await;
    let _now = crate::routes::pin_london_now_for_tests(live_now());

    let (status, body) = get(
        test_router(test_app(pool.clone())),
        format!(
            "/Trips/plan?origin=ZKA&destination=ZKC&date={}&departAfter=09:45",
            live_date()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(train_uids(&body, 0), ["TLVLA1", "TLVLA2"], "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_all_live_feasible(&body);
    let onward = &itinerary["legs"][1];
    // Shown as booked, with the delay alongside.
    assert_eq!(onward["scheduledDeparture"], "10:20:00", "{onward:?}");
    assert_eq!(onward["live"]["delayMinutes"], 20, "{onward:?}");
    assert_eq!(onward["live"]["interchangeFeasible"], true, "{onward:?}");
    // 10:00 to 11:10 on live times.
    assert_eq!(itinerary["totalDurationMinutes"], 70, "{itinerary:?}");

    // The timetable answer is still one parameter away.
    let (status, timetable) = get(
        test_router(test_app(pool.clone())),
        format!(
            "/Trips/plan?origin=ZKA&destination=ZKC&date={}&departAfter=09:45&live=false",
            live_date()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{timetable:?}");
    assert_eq!(train_uids(&timetable, 0), ["TLVLA1", "TLVLA3"]);

    cleanup_live(&pool, &uids, "TLVLAT").await;
}

/// `TLVCHN`: four trains in a row from ZLA are cancelled. Fastest mode's
/// three re-plans used to run out on the fourth and serve it.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_run_of_cancellations_longer_than_the_replan_budget_is_skipped() {
    let pool = connect().await;
    let uids = ["TLVCH1", "TLVCH2", "TLVCH3", "TLVCH4", "TLVCH5"];
    cleanup_live(&pool, &uids, "TLVCHN").await;
    seed_stations(&pool, "TLVCHN", &[("ZLA", "TLVLA"), ("ZLB", "TLVLB")]).await;
    for (index, uid) in uids.iter().enumerate() {
        let departs = format!("10:{index}0:00");
        let arrives = format!("10:{index}5:00");
        seed_schedule(
            &pool,
            uid,
            &[
                (0, "TLVLA", "origin", None, Some(departs.as_str())),
                (1, "TLVLB", "terminate", Some(arrives.as_str()), None),
            ],
        )
        .await;
        if index < 4 {
            seed_trust_state(&pool, uid, "cancelled", None, live_now()).await;
        }
    }
    let _now = crate::routes::pin_london_now_for_tests(live_now());

    let (status, body) = get(
        test_router(test_app(pool.clone())),
        format!(
            "/Trips/plan?origin=ZLA&destination=ZLB&date={}&departAfter=09:45",
            live_date()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(train_uids(&body, 0), ["TLVCH5"], "{body:?}");
    assert_all_live_feasible(&body);

    cleanup_live(&pool, &uids, "TLVCHN").await;
}

/// `TLVWPT`: ZMA -> ZMW (waypoint) -> ZMC. TLVWP1 is 3 late into ZMW
/// (10:33): the 10:40 on still works (10:38 with the five-minute change),
/// so nothing is re-planned, and the onward segment's `departAfter` is the
/// live ready time, not the timetabled 10:35. 8 late (10:38, ready 10:43)
/// it misses the 10:40 and the waypoint chains onto the 10:50.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn a_waypoint_segment_chains_from_the_live_arrival() {
    let pool = connect().await;
    let uids = ["TLVWP1", "TLVWP2", "TLVWP3"];
    cleanup_live(&pool, &uids, "TLVWPT").await;
    seed_stations(
        &pool,
        "TLVWPT",
        &[("ZMA", "TLVMA"), ("ZMW", "TLVMW"), ("ZMC", "TLVMC")],
    )
    .await;
    for (uid, from, to, departs, arrives) in [
        ("TLVWP1", "TLVMA", "TLVMW", "10:00:00", "10:30:00"),
        ("TLVWP2", "TLVMW", "TLVMC", "10:40:00", "11:00:00"),
        ("TLVWP3", "TLVMW", "TLVMC", "10:50:00", "11:10:00"),
    ] {
        seed_schedule(
            &pool,
            uid,
            &[
                (0, from, "origin", None, Some(departs)),
                (1, to, "terminate", Some(arrives), None),
            ],
        )
        .await;
    }
    let trains_id = seed_trust_state(&pool, "TLVWP1", "en_route", Some(3), live_now()).await;
    let _now = crate::routes::pin_london_now_for_tests(live_now());
    let uri = |results: &str| {
        format!(
            "/Trips/plan?origin=ZMA&destination=ZMC&waypoints=ZMW&date={}&departAfter=09:45\
             &results={results}",
            live_date()
        )
    };

    for results in ["fastest", "options"] {
        let (status, body) = get(test_router(test_app(pool.clone())), uri(results)).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(train_uids(&body, 1), ["TLVWP2"], "{results}: {body:?}");
        assert_eq!(
            body["segments"][1]["departAfter"]["time"], "10:38:00",
            "{results}: {body:?}"
        );
        assert_all_live_feasible(&body);
    }

    sqlx::query("UPDATE train_current_state SET delay_minutes = 8 WHERE trains_id = $1")
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("set the delay");
    for results in ["fastest", "options"] {
        let (status, body) = get(test_router(test_app(pool.clone())), uri(results)).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(train_uids(&body, 1), ["TLVWP3"], "{results}: {body:?}");
        assert_eq!(
            body["segments"][1]["departAfter"]["time"], "10:43:00",
            "{results}: {body:?}"
        );
        assert_eq!(body["live"]["replans"], 0, "{results}: {body:?}");
        assert_all_live_feasible(&body);
    }

    cleanup_live(&pool, &uids, "TLVWPT").await;
}

/// Arrive-by (`seed_arrive_by_network`): TAVF1 running 10 late arrives
/// 10:50, after an `arriveBy` of 10:45, so the search itself lands on the
/// 09:40 with no re-plan, in both modes. With an `arriveBy` of 10:55 the
/// late TAVF1 still makes it, and is planned on its live times.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn arrive_by_searches_on_live_times() {
    let pool = connect().await;
    let uids = seed_arrive_by_network(&pool).await;
    seed_trust_state(&pool, "TAVF1", "en_route", Some(10), live_now()).await;
    let _now = crate::routes::pin_london_now_for_tests(live_now());

    for results in ["fastest", "options"] {
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            arrive_by_uri(&format!("&arriveBy=10:45&results={results}")),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(train_uids(&body, 0), ["TAVE1"], "{results}: {body:?}");
        assert_eq!(body["live"]["replans"], 0, "{results}: {body:?}");
        assert_all_live_feasible(&body);
    }

    let (status, body) = get(
        test_router(test_app(pool.clone())),
        arrive_by_uri("&arriveBy=10:55"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(train_uids(&body, 0), ["TAVF1"], "{body:?}");
    let itinerary = &body["segments"][0]["itineraries"][0];
    assert_all_live_feasible(&body);
    assert_eq!(itinerary["legs"][0]["scheduledDeparture"], "10:00:00");
    assert_eq!(itinerary["legs"][0]["live"]["delayMinutes"], 10);
    // 10:10 to 10:50 on live times.
    assert_eq!(itinerary["totalDurationMinutes"], 40, "{itinerary:?}");

    cleanup_live(&pool, &uids, "TAVNET").await;
}

/// No live data at all: the live plan is the timetable plan, leg for leg.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn without_live_data_the_live_plan_is_the_timetable_plan() {
    let pool = connect().await;
    let uids = ["TLVNO1", "TLVNO2", "TLVNO3"];
    cleanup_live(&pool, &uids, "TLVNOD").await;
    seed_stations(
        &pool,
        "TLVNOD",
        &[("ZNA", "TLVNA"), ("ZNB", "TLVNB"), ("ZNC", "TLVNC")],
    )
    .await;
    for (uid, from, to, departs, arrives) in [
        ("TLVNO1", "TLVNA", "TLVNB", "10:00:00", "10:30:00"),
        ("TLVNO2", "TLVNB", "TLVNC", "10:40:00", "11:00:00"),
        ("TLVNO3", "TLVNB", "TLVNC", "11:00:00", "11:20:00"),
    ] {
        seed_schedule(
            &pool,
            uid,
            &[
                (0, from, "origin", None, Some(departs)),
                (1, to, "terminate", Some(arrives), None),
            ],
        )
        .await;
    }
    let _now = crate::routes::pin_london_now_for_tests(live_now());

    for results in ["fastest", "options"] {
        let uri = |live: &str| {
            format!(
                "/Trips/plan?origin=ZNA&destination=ZNC&date={}&departAfter=09:45\
                 &results={results}&live={live}",
                live_date()
            )
        };
        let (status, timetable) = get(test_router(test_app(pool.clone())), uri("false")).await;
        assert_eq!(status, StatusCode::OK, "{timetable:?}");
        let (status, live) = get(test_router(test_app(pool.clone())), uri("true")).await;
        assert_eq!(status, StatusCode::OK, "{live:?}");
        assert_eq!(live["live"]["applied"], true, "{live:?}");
        assert_eq!(live["live"]["replans"], 0, "{live:?}");
        let strip = |body: &Value| {
            let mut segments = body["segments"].clone();
            for segment in segments.as_array_mut().expect("segments") {
                for itinerary in segment["itineraries"].as_array_mut().expect("itineraries") {
                    let itinerary = itinerary.as_object_mut().expect("itinerary");
                    itinerary.remove("liveFeasible");
                    for leg in itinerary["legs"].as_array_mut().expect("legs") {
                        leg.as_object_mut().expect("leg").remove("live");
                    }
                }
            }
            segments
        };
        assert_eq!(strip(&live), strip(&timetable), "{results}");
    }

    cleanup_live(&pool, &uids, "TLVNOD").await;
}

/// `TRIP_PLAN_LIVE_IN_SEARCH=false` is the 2026-09-28 behaviour: the first
/// search is on the timetable and only the re-plans see live data, so with
/// options' one re-plan the cancelled TLVIN3 read after it is still served
/// (flagged). The switch is the way back if the snapshot misbehaves.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api \
            routes::trips -- --ignored --test-threads=1`"]
async fn in_search_off_is_the_timetable_first_replan_loop() {
    let pool = connect().await;
    let uids = ["TLVIN1", "TLVIN2", "TLVIN3", "TLVIN4"];
    cleanup_live(&pool, &uids, "TLVINS").await;
    seed_stations(
        &pool,
        "TLVINS",
        &[("ZIA", "TLVIA"), ("ZIB", "TLVIB"), ("ZIC", "TLVIC")],
    )
    .await;
    seed_schedule(
        &pool,
        "TLVIN1",
        &[
            (0, "TLVIA", "origin", None, Some("10:00:00")),
            (1, "TLVIB", "terminate", Some("10:30:00"), None),
        ],
    )
    .await;
    for (uid, departs, arrives) in [
        ("TLVIN2", "10:40:00", "11:00:00"),
        ("TLVIN3", "11:00:00", "11:20:00"),
        ("TLVIN4", "11:10:00", "11:30:00"),
    ] {
        seed_schedule(
            &pool,
            uid,
            &[
                (0, "TLVIB", "origin", None, Some(departs)),
                (1, "TLVIC", "terminate", Some(arrives), None),
            ],
        )
        .await;
    }
    seed_trust_state(&pool, "TLVIN1", "en_route", Some(15), live_now()).await;
    seed_trust_state(&pool, "TLVIN3", "cancelled", None, live_now()).await;
    let _now = crate::routes::pin_london_now_for_tests(live_now());
    let _off = set_in_search_for_tests(false);

    let (status, body) = get(
        test_router(test_app(pool.clone())),
        format!(
            "/Trips/plan?origin=ZIA&destination=ZIC&date={}&departAfter=09:45&results=options",
            live_date()
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["live"]["inSearch"], false, "{body:?}");
    assert_eq!(body["live"]["replans"], 1, "{body:?}");
    assert_eq!(all_train_uids(&body, 0), [["TLVIN1", "TLVIN3"]], "{body:?}");
    assert_eq!(
        body["segments"][0]["itineraries"][0]["liveFeasible"], false,
        "{body:?}"
    );

    cleanup_live(&pool, &uids, "TLVINS").await;
}

/// Latency of the in-search snapshot end to end, on the synthetic network
/// of `bench_across_midnight` (676 stations, 20,000 ten-stop trains, about
/// 180k connections) with TRUST state seeded for 2,100 of its trains: 1,500
/// running 1-30 minutes late (fresh) and 600 cancelled -- more than a
/// production day carries at any one time. Prints the snapshot read and
/// build, then each mode's median over eight origin/destination pairs:
/// `live=false`, live with the re-plan loop only, and live in-search (warm
/// snapshot). Does nothing unless `TRIP_PLAN_IN_SEARCH_BENCH` is set; seeds
/// and removes `BX%` schedules and trains and `BQ-%` stations; run it on a
/// scratch database, in release mode, with the graph cache on:
///
/// ```text
/// TRIP_PLAN_IN_SEARCH_BENCH=1 TRIP_PLAN_GRAPH_CACHE_DATES=2 cargo test --release \
///   -p api --lib bench_in_search -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "benchmark; see its doc comment"]
#[expect(
    clippy::too_many_lines,
    reason = "one benchmark, read top to bottom: seed, snapshot, the three modes"
)]
async fn bench_in_search() {
    if std::env::var("TRIP_PLAN_IN_SEARCH_BENCH").is_err() {
        return;
    }
    let pool = connect().await;
    let date = live_date();
    let cleanup = || async {
        for statement in [
            "DELETE FROM trains WHERE train_uid LIKE 'BX%'",
            "DELETE FROM schedule_calling_points_full WHERE uid LIKE 'BX%'",
            "DELETE FROM stanox_crs WHERE stanox LIKE 'BQ-%'",
        ] {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("clean the bench rows");
        }
    };
    cleanup().await;
    seed_across_midnight_bench(&pool, date, date.succ_opt().unwrap()).await;
    sqlx::query(
        "WITH picked AS ( \
           SELECT uid, row_number() OVER (ORDER BY md5(uid)) AS n \
           FROM (SELECT DISTINCT uid FROM schedule_calling_points_full \
                 WHERE service_date = $1 AND uid LIKE 'BX%') u), \
         inserted AS ( \
           INSERT INTO trains (train_uid, service_date) \
           SELECT uid, $1 FROM picked WHERE n <= 2100 RETURNING id, train_uid) \
         INSERT INTO train_current_state (trains_id, status, delay_minutes, updated_at) \
         SELECT i.id, CASE WHEN p.n <= 1500 THEN 'en_route' ELSE 'cancelled' END, \
                CASE WHEN p.n <= 1500 THEN 1 + (p.n % 30)::int END, $2 \
         FROM inserted i JOIN picked p ON p.uid = i.train_uid",
    )
    .bind(date)
    .bind(live_now())
    .execute(&pool)
    .await
    .expect("seed train states");

    let _now = crate::routes::pin_london_now_for_tests(live_now());
    let app = test_app(pool.clone());
    let router = test_router(app.clone());
    let warm = std::time::Instant::now();
    let (status, body) = get(
        router.clone(),
        format!("/Trips/plan?origin=QAA&destination=QMZ&date={date}&live=false"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    println!("graph build: {:?}", warm.elapsed());

    let config = trip_plan_live::LiveConfig::default();
    let started = std::time::Instant::now();
    let snapshot_states = trip_plan_live::fetch_snapshot_states(&pool, date, &config, live_now())
        .await
        .expect("read the states");
    let read = started.elapsed();
    let marker = trip_planning::latest_schedule_publish(&pool)
        .await
        .expect("marker");
    let graph = load_graph(&app, date, marker)
        .await
        .expect("graph")
        .expect("a graph");
    let started = std::time::Instant::now();
    let snapshot = trip_plan_live::LiveSnapshot::build(
        &graph.connections,
        date,
        &snapshot_states,
        &config,
        live_now(),
    );
    println!(
        "snapshot: {} states read in {read:.1?}; {} trains, {} replacement connections, \
         built in {:.1?} over {} connections",
        snapshot_states.len(),
        snapshot.trains(),
        snapshot.overlay.replacements().len(),
        started.elapsed(),
        graph.connections.len()
    );

    let started = std::time::Instant::now();
    let _ = trip_plan_live::combined_overlay(
        Some(&snapshot),
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    );
    println!("combined_overlay, nothing read: {:.1?}", started.elapsed());

    let pairs = [
        ("QAA", "QMZ"),
        ("QBC", "QXY"),
        ("QDE", "QPQ"),
        ("QFG", "QRS"),
        ("QHI", "QTU"),
        ("QJK", "QVW"),
        ("QLM", "QAB"),
        ("QNO", "QCD"),
    ];
    for results in ["fastest", "options"] {
        for (label, live, in_search) in [
            ("timetable (live=false)", "false", false),
            ("live, re-plan loop only", "true", false),
            ("live, in-search", "true", true),
        ] {
            let _mode = set_in_search_for_tests(in_search);
            let mut samples = Vec::new();
            let mut replans = 0;
            for (origin, destination) in pairs {
                let mut runs = Vec::new();
                for _ in 0..5 {
                    let started = std::time::Instant::now();
                    let (status, body) = get(
                        router.clone(),
                        format!(
                            "/Trips/plan?origin={origin}&destination={destination}&date={date}\
                             &departAfter=09:30&results={results}&live={live}"
                        ),
                    )
                    .await;
                    runs.push(started.elapsed());
                    assert_eq!(status, StatusCode::OK, "{body:?}");
                    if runs.len() == 1 {
                        replans += body["live"]["replans"].as_u64().unwrap_or(0);
                    }
                }
                runs.sort();
                samples.push(runs[2]);
            }
            samples.sort();
            println!(
                "{results:8} {label:26} median {:>9.1?} max {:>9.1?} replans {replans}",
                samples[samples.len() / 2],
                samples[samples.len() - 1],
            );
        }
    }
    cleanup().await;
}
