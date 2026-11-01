//! Timetabled untracked legs (DS-237) through the journey routes, against
//! a live database: a tram leg saved with its network and line is found in
//! the network's timetable on read (matched, retimed, not in the timetable,
//! no timetable for the date), its line's disruption shows, an edit that
//! keeps its stops keeps it timetabled, the save refuses a bad network or
//! line, and going back to an earlier plan restores its stops and line.
//!
//! The network is `ds_store::test_support::transit`'s hand-made one under a
//! test-only id: line `blue` from `9400ZZTTAAA` to `9400ZZTTBBB` (30
//! minutes; trams at 06:00, 07:00, 23:50 and 00:10, back at 07:30), service
//! on the fixture day and the day before.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::{NaiveDate, TimeZone, Utc};
use ds_store::test_support::transit::{A, B, apply_situations, cleanup, seed, situation};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::data::transit_planning::set_switch_for_tests;

const NETWORK: &str = "test-jtt";
const USER: &str = "TEST-TIMETABLED-LEGS";

/// A Wednesday in winter, so London time is UTC.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2099, 11, 4).unwrap()
}

async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to postgres")
}

async fn cleanup_user(pool: &PgPool) {
    for sql in [
        "DELETE FROM journeys WHERE user_id = $1",
        "DELETE FROM journey_idempotency_keys WHERE user_id = $1",
        "DELETE FROM sessions WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql).bind(USER).execute(pool).await.expect(sql);
    }
}

async fn session(pool: &PgPool) -> String {
    sqlx::query("INSERT INTO users (id, name) VALUES ($1, $1) ON CONFLICT DO NOTHING")
        .bind(USER)
        .execute(pool)
        .await
        .expect("seed user");
    let raw = format!("raw-timetabled-{USER}");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, now() + INTERVAL '1 day') \
         ON CONFLICT (id) DO UPDATE SET user_id = EXCLUDED.user_id, expires_at = EXCLUDED.expires_at",
    )
    .bind(hash_session_token(&raw))
    .bind(USER)
    .execute(pool)
    .await
    .expect("seed session");
    raw
}

fn router(pool: PgPool) -> axum::Router {
    crate::app::Router::new()
        .merge(super::journeys::router())
        .with_state(crate::test_support::inert_app(pool))
}

async fn send(
    router: &axum::Router,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, format!("distant_signal_session={cookie}"));
    let request = match body {
        Some(body) => request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
    )
}

/// `HH:MM` on `date`, London (= UTC in November), RFC 3339.
fn at(date: NaiveDate, time: &str) -> String {
    format!("{date}T{time}:00Z")
}

/// A tram leg from Aaa to Bbb as a plan's transit leg is saved.
fn tram_leg(departure: &str, arrival: &str) -> Value {
    json!({
        "mode": "untracked",
        "transitMode": "tram",
        "lineName": "Blue Line",
        "originStopId": A,
        "destinationStopId": B,
        "plannedDeparture": departure,
        "plannedArrival": arrival,
        "transitNetwork": NETWORK,
        "transitLineId": "blue",
        "headsign": "Bbb",
    })
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
#[expect(
    clippy::too_many_lines,
    reason = "test code: one scenario, read top to bottom"
)]
async fn a_timetabled_leg_is_found_in_its_timetable_on_every_read() {
    let _now = crate::routes::pin_london_now_for_tests(
        Utc.with_ymd_and_hms(2099, 11, 4, 5, 0, 0).unwrap(),
    );
    let _switch = set_switch_for_tests(&[NETWORK]);
    let pool = connect().await;
    cleanup_user(&pool).await;
    seed(&pool, NETWORK, today()).await;
    let cookie = session(&pool).await;
    let router = router(pool.clone());
    let day = today();

    // The 07:00 tram, as planned.
    let (status, created) = send(
        &router,
        "POST",
        "/Journeys",
        &cookie,
        Some(json!({ "leg": tram_leg(&at(day, "07:00"), &at(day, "07:30")) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let journey_id = created["journeyId"].as_i64().unwrap();
    let hint: Value =
        sqlx::query_scalar("SELECT timetable_hint FROM journey_legs WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        hint,
        json!({ "tripId": "T0700", "feedVersion": "test-v1" }),
        "the ride matched at save time is kept as a hint"
    );

    // Moved four minutes, at midday (none near), and the day after the
    // feed's horizon.
    for (departure, arrival) in [
        (at(day, "07:04"), at(day, "07:34")),
        (at(day, "12:00"), at(day, "12:30")),
        (
            at(day.succ_opt().unwrap(), "07:00"),
            at(day.succ_opt().unwrap(), "07:30"),
        ),
    ] {
        let (status, added) = send(
            &router,
            "POST",
            &format!("/Journeys/{journey_id}/legs"),
            &cookie,
            Some(tram_leg(&departure, &arrival)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{added}");
    }

    let (status, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let legs = detail["legs"].as_array().unwrap();
    let matched = &legs[0];
    assert_eq!(matched["kind"], "untracked");
    assert_eq!(matched["transitNetwork"], NETWORK);
    assert_eq!(matched["transitLineId"], "blue");
    assert_eq!(matched["headsign"], "Bbb");
    let timetable = &matched["timetable"];
    assert_eq!(timetable["state"], "matched", "{timetable}");
    assert_eq!(timetable["network"], NETWORK);
    assert_eq!(timetable["line"]["id"], format!("{NETWORK}:blue"));
    assert_eq!(timetable["line"]["name"], "Blue Line");
    assert_eq!(timetable["line"]["colour"], "#1f6fb5");
    assert_eq!(timetable["line"]["mode"], "tram", "the network's mode");
    assert_eq!(timetable["scheduledDeparture"], "2099-11-04T07:00:00Z");
    assert_eq!(timetable["scheduledArrival"], "2099-11-04T07:30:00Z");
    assert_eq!(timetable["headsign"], "Bbb");
    assert_eq!(
        timetable["calls"],
        json!([
            {
                "stopId": A, "name": "Aaa", "platform": "To the city",
                "scheduledArrival": null, "scheduledDeparture": "2099-11-04T07:00:00Z",
            },
            {
                "stopId": B, "name": "Bbb", "platform": "To the city",
                "scheduledArrival": "2099-11-04T07:30:00Z", "scheduledDeparture": null,
            },
        ])
    );
    assert_eq!(timetable["notices"], json!([]));
    assert!(
        timetable.get("rides").is_none(),
        "the rides are for the server"
    );
    assert!(
        timetable.get("live").is_none() && timetable.get("expected").is_none(),
        "scheduled only"
    );

    assert_eq!(legs[1]["timetable"]["state"], "retimed");
    assert_eq!(
        legs[1]["timetable"]["scheduledDeparture"],
        "2099-11-04T07:00:00Z"
    );
    assert_eq!(legs[2]["timetable"]["state"], "notInTimetable");
    assert!(legs[2]["timetable"]["scheduledDeparture"].is_null());
    assert_eq!(legs[2]["timetable"]["calls"], json!([]));
    assert_eq!(legs[3]["timetable"]["state"], "noTimetable");
    assert_eq!(
        legs[3]["timetable"]["line"]["name"], "Blue Line",
        "the line is still known"
    );

    // A disruption on the line shows on every leg of it, with its page.
    apply_situations(
        &pool,
        NETWORK,
        vec![situation("SIT-1", "blue", "disrupted", None, Utc::now())],
        Utc::now(),
    )
    .await;
    let (_, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &cookie,
        None,
    )
    .await;
    let notices = &detail["legs"][0]["timetable"]["notices"];
    assert_eq!(notices[0]["kind"], "disruption", "{notices}");
    assert_eq!(notices[0]["lineId"], format!("{NETWORK}:blue"));
    assert_eq!(notices[0]["situationId"], "SIT-1");
    assert_eq!(
        notices[0]["message"],
        "Disruption reported on the Blue Line"
    );

    // An edit keeping the stops keeps it timetabled; one dropping them
    // doesn't.
    let leg_id = legs[1]["id"].as_i64().unwrap();
    let mut edit = tram_leg(&at(day, "07:00"), &at(day, "07:30"));
    for field in ["mode", "transitNetwork", "transitLineId", "headsign"] {
        edit.as_object_mut().unwrap().remove(field);
    }
    let (status, body) = send(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/legs/{leg_id}"),
        &cookie,
        Some(edit.clone()),
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let row: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT transit_network, transit_line_id FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, (Some(NETWORK.to_owned()), Some("blue".to_owned())));
    for field in ["originStopId", "destinationStopId"] {
        edit.as_object_mut().unwrap().remove(field);
    }
    let (status, body) = send(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/legs/{leg_id}"),
        &cookie,
        Some(edit),
    )
    .await;
    assert!(status.is_success(), "{status} {body}");
    let row: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT transit_network, headsign FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, (None, None), "no stops, no timetable");

    cleanup_user(&pool).await;
    cleanup(&pool, NETWORK).await;
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_bad_network_or_line_is_refused_with_a_sentence() {
    let _now = crate::routes::pin_london_now_for_tests(
        Utc.with_ymd_and_hms(2099, 11, 4, 5, 0, 0).unwrap(),
    );
    let _switch = set_switch_for_tests(&[NETWORK]);
    let pool = connect().await;
    cleanup_user(&pool).await;
    seed(&pool, NETWORK, today()).await;
    let cookie = session(&pool).await;
    let router = router(pool.clone());
    let day = today();
    let leg = || tram_leg(&at(day, "07:00"), &at(day, "07:30"));
    let with = |field: &str, value: Value| {
        let mut leg = leg();
        leg[field] = value;
        leg
    };
    let without = |field: &str| {
        let mut leg = leg();
        leg.as_object_mut().unwrap().remove(field);
        leg
    };
    for (body, expected) in [
        (
            with("transitNetwork", json!("nowhere")),
            "transitNetwork nowhere is not a network we plan with.",
        ),
        (
            with("transitLineId", json!("purple")),
            "transitLineId purple is not a line of test-jtt.",
        ),
        (
            without("transitLineId"),
            "A leg with a transitNetwork needs its transitLineId, such as victoria.",
        ),
        (
            without("transitNetwork"),
            "transitLineId and headsign need a transitNetwork.",
        ),
        (
            with("transitMode", json!("bus")),
            "Only a tube, dlr, tram leg can have a transitNetwork.",
        ),
    ] {
        let (status, answer) = send(
            &router,
            "POST",
            "/Journeys",
            &cookie,
            Some(json!({ "leg": body })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert_eq!(answer, Value::String(expected.to_owned()));
    }
    // A train leg of an itinerary takes none of it.
    let (status, answer) = send(
        &router,
        "POST",
        "/Journeys/itinerary",
        &cookie,
        Some(json!({
            "legs": [{
                "trainUid": "A00001", "serviceDate": day.to_string(),
                "transitNetwork": NETWORK,
            }]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    cleanup_user(&pool).await;
    cleanup(&pool, NETWORK).await;
}

/// Going back to an earlier plan restores an untracked leg's stops and
/// network ride (it dropped them before DS-237).
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn going_back_to_an_earlier_plan_keeps_its_stops_and_line() {
    // Under way (a change before the journey starts only updates the
    // booked plan): on the 07:00 tram.
    let _now = crate::routes::pin_london_now_for_tests(
        Utc.with_ymd_and_hms(2099, 11, 4, 7, 5, 0).unwrap(),
    );
    let _switch = set_switch_for_tests(&[NETWORK]);
    let pool = connect().await;
    cleanup_user(&pool).await;
    seed(&pool, NETWORK, today()).await;
    let cookie = session(&pool).await;
    let router = router(pool.clone());
    let day = today();

    let (status, created) = send(
        &router,
        "POST",
        "/Journeys",
        &cookie,
        Some(json!({ "leg": tram_leg(&at(day, "07:00"), &at(day, "07:30")) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let journey_id = created["journeyId"].as_i64().unwrap();
    let leg_id = created["legId"].as_i64().unwrap();
    // Swapped for a walk, by the traveller's choice.
    let (status, replaced) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/replace"),
        &cookie,
        Some(json!({
            "cause": "choice",
            "legs": [{
                "kind": "untracked", "transitMode": "walk",
                "plannedDeparture": at(day, "07:00"), "plannedArrival": at(day, "07:40"),
            }],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    let (status, history) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let first = history["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["revisionNo"] == 1)
        .unwrap()
        .clone();
    assert_eq!(first["legs"][0]["originStopId"], A, "{first}");
    assert_eq!(first["legs"][0]["transitLineId"], "blue");
    let (status, undone) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/revisions/1/revert"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    let (_, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &cookie,
        None,
    )
    .await;
    let leg = &detail["legs"][0];
    assert_eq!(leg["transitMode"], "tram", "{detail}");
    assert_eq!(leg["originStopId"], A);
    assert_eq!(leg["destinationStopId"], B);
    assert_eq!(leg["transitNetwork"], NETWORK);
    assert_eq!(leg["headsign"], "Bbb");
    assert_eq!(leg["timetable"]["state"], "matched");
    cleanup_user(&pool).await;
    cleanup(&pool, NETWORK).await;
}

/// On a network whose stations are `TfL`'s stops (phase C), a timetabled
/// leg says which of its boarding stop's live predictions are its line's
/// and reach its alighting stop: the line, and the last stations of its
/// rides. Elsewhere it says nothing.
#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database); --test-threads=1"]
async fn a_tfl_leg_names_the_live_arrivals_that_reach_its_stop() {
    const TFL_NETWORK: &str = "test-tfl-arrivals";
    let _now = crate::routes::pin_london_now_for_tests(
        Utc.with_ymd_and_hms(2099, 11, 4, 5, 0, 0).unwrap(),
    );
    let _switch = set_switch_for_tests(&[TFL_NETWORK, NETWORK]);
    let pool = connect().await;
    cleanup_user(&pool).await;
    seed(&pool, TFL_NETWORK, today()).await;
    seed(&pool, NETWORK, today()).await;
    let cookie = session(&pool).await;
    let router = router(pool.clone());
    let day = today();
    let mut tfl_leg = tram_leg(&at(day, "07:00"), &at(day, "07:30"));
    tfl_leg["transitNetwork"] = json!(TFL_NETWORK);
    let (status, created) = send(
        &router,
        "POST",
        "/Journeys",
        &cookie,
        Some(json!({ "leg": tfl_leg })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let journey_id = created["journeyId"].as_i64().unwrap();
    let (status, added) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs"),
        &cookie,
        Some(tram_leg(&at(day, "07:00"), &at(day, "07:30"))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let (_, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(
        detail["legs"][0]["timetable"]["liveArrivals"],
        json!({
            "stopId": "940GZZTTAAA",
            "lineId": "blue",
            "destinations": ["Bbb"],
        }),
        "{detail}"
    );
    assert_eq!(
        detail["legs"][0]["timetable"]["calls"][0]["stopId"], "940GZZTTAAA",
        "TfL's spelling"
    );
    assert!(
        detail["legs"][1]["timetable"].get("liveArrivals").is_none(),
        "no live arrivals off TfL: {detail}"
    );
    cleanup_user(&pool).await;
    cleanup(&pool, TFL_NETWORK).await;
    cleanup(&pool, NETWORK).await;
}
