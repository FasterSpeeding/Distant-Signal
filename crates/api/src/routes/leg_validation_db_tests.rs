//! Hand-entered train legs checked against the timetable
//! (`data::leg_validation`), through the web app's journey routes, against a
//! live database. The delegated itinerary path is in `delegated_db_tests`,
//! which reuses [`seed_schedules`].
//!
//! Fixtures: synthetic UIDs (`ZLV00n`), TIPLOCs (`ZLV1..`, `ZLV2..`) and
//! three-letter CRS codes (`ZQ.`, `ZS.`) that no real station has, on a date
//! inside the tracking horizon; every cleanup is keyed on them.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::test_support::FixtureCleanup;

/// The ordinary train: `ZQA` origin, `ZQB` stop, `ZQC` passing point, `ZQD`
/// set-down only, `ZQE` request stop, `ZQF` pick-up only, `ZQG` terminus.
pub(super) const ORDINARY: &str = "ZLV001";
/// A real-shaped Caledonian Sleeper (1S25, Euston to Inverness): Euston
/// (`ZSE`) origin, Watford Junction (`ZSW`), Crewe (`ZSC`) and Preston
/// (`ZSP`) pick-up only (`U`), then Stirling (`ZSS`) and Perth (`ZSH`)
/// set-down only (`D`) after midnight, Inverness (`ZSI`) terminus.
pub(super) const SLEEPER: &str = "ZLV002";
/// No schedule rows at all: not matched yet.
pub(super) const UNMATCHED: &str = "ZLV003";
const UIDS: [&str; 3] = [ORDINARY, SLEEPER, UNMATCHED];

/// Inside the 28-day tracking horizon.
pub(super) fn service_date() -> chrono::NaiveDate {
    crate::routes::london_today() + chrono::Duration::days(2)
}

/// `(tiploc, crs, kind, times, day_offset, flags)`: `times` is
/// `"arrival/departure"` (`-` for none) or `"pass HH:MM"` for a passing
/// point; `flags` holds `B` (can board), `A` (can alight), `R` (request stop).
type Row = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    i16,
    &'static str,
);

const ORDINARY_ROWS: [Row; 7] = [
    ("ZLV1A", "ZQA", "origin", "-/08:00", 0, "B"),
    ("ZLV1B", "ZQB", "intermediate", "08:10/08:11", 0, "BA"),
    ("ZLV1C", "ZQC", "intermediate", "pass 08:15", 0, ""),
    ("ZLV1D", "ZQD", "intermediate", "08:20/08:21", 0, "A"),
    ("ZLV1E", "ZQE", "intermediate", "08:30/08:31", 0, "BAR"),
    ("ZLV1F", "ZQF", "intermediate", "08:40/08:41", 0, "B"),
    ("ZLV1G", "ZQG", "terminate", "08:50/-", 0, "A"),
];

const SLEEPER_ROWS: [Row; 7] = [
    ("ZLV2E", "ZSE", "origin", "-/21:15", 0, "B"),
    ("ZLV2W", "ZSW", "intermediate", "21:33/21:35", 0, "B"),
    ("ZLV2C", "ZSC", "intermediate", "23:27/23:35", 0, "B"),
    ("ZLV2P", "ZSP", "intermediate", "00:15/00:23", 1, "B"),
    ("ZLV2S", "ZSS", "intermediate", "05:59/06:01", 1, "A"),
    ("ZLV2H", "ZSH", "intermediate", "06:35/06:40", 1, "A"),
    ("ZLV2I", "ZSI", "terminate", "08:41/-", 1, "A"),
];

/// `(arrival, departure, pass)` from a [`Row`]'s `times`.
fn row_times(
    times: &str,
) -> (
    Option<chrono::NaiveTime>,
    Option<chrono::NaiveTime>,
    Option<chrono::NaiveTime>,
) {
    let time = |t: &str| (t != "-").then(|| chrono::NaiveTime::parse_from_str(t, "%H:%M").unwrap());
    if let Some(pass) = times.strip_prefix("pass ") {
        return (None, None, time(pass));
    }
    let (arrival, departure) = times.split_once('/').unwrap();
    (time(arrival), time(departure), None)
}

fn cleanup_statements() -> Vec<String> {
    let uids = UIDS.map(|uid| format!("'{uid}'")).join(", ");
    vec![
        format!("DELETE FROM schedule_calling_points_full WHERE uid IN ({uids})"),
        "DELETE FROM tiploc_crs WHERE tiploc LIKE 'ZLV1%' OR tiploc LIKE 'ZLV2%'".to_string(),
        format!("DELETE FROM trains WHERE train_uid IN ({uids})"),
    ]
}

/// Seeds both schedules on [`service_date`] (as the schema owner when the
/// suite runs as a narrow role, `schedule_fixture_pool`) and returns the
/// guard that removes them, and any `trains` row a test made, on drop.
pub(super) async fn seed_schedules(pool: &PgPool) -> FixtureCleanup {
    let seed = ds_store::test_support::schedule_fixture_pool(pool).await;
    let guard = FixtureCleanup::new(&seed, cleanup_statements()).await;
    for (uid, rows) in [(ORDINARY, &ORDINARY_ROWS), (SLEEPER, &SLEEPER_ROWS)] {
        for (seq, row) in rows.iter().enumerate() {
            let (tiploc, crs, kind, times, day_offset, flags) = *row;
            let (arrival, departure, pass) = row_times(times);
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, \
                  working_pass, day_offset, can_board, can_alight, request_stop) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(service_date())
            .bind(uid)
            .bind(i16::try_from(seq).unwrap())
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .bind(pass)
            .bind(day_offset)
            .bind(flags.contains('B'))
            .bind(flags.contains('A'))
            .bind(flags.contains('R'))
            .execute(&seed)
            .await
            .expect("seed calling point");
            sqlx::query(
                "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) \
                 VALUES ($1, $2, $1, '99999', 1)",
            )
            .bind(tiploc)
            .bind(crs)
            .execute(&seed)
            .await
            .expect("seed tiploc_crs");
        }
    }
    guard
}

async fn connect() -> PgPool {
    ds_store::test_support::connect().await
}

async fn seed_user(pool: &PgPool, user: &str) -> String {
    sqlx::query("INSERT INTO users (id, name) VALUES ($1, $1) ON CONFLICT (id) DO NOTHING")
        .bind(user)
        .execute(pool)
        .await
        .expect("seed user");
    let raw = format!("raw-session-{user}");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, now() + INTERVAL '1 day') \
         ON CONFLICT (id) DO UPDATE SET expires_at = EXCLUDED.expires_at",
    )
    .bind(hash_session_token(&raw))
    .bind(user)
    .execute(pool)
    .await
    .expect("seed session");
    raw
}

async fn cleanup_user(pool: &PgPool, user: &str) {
    for statement in [
        "DELETE FROM journey_legs WHERE journey_id IN (SELECT id FROM journeys WHERE user_id = $1)",
        "DELETE FROM journeys WHERE user_id = $1",
        "DELETE FROM train_subscriptions WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(statement)
            .bind(user)
            .execute(pool)
            .await
            .expect("clean up fixture user");
    }
}

/// `(status, body)`: JSON when the body parses as JSON, else the text.
async fn post(pool: &PgPool, uri: &str, cookie: &str, body: Value) -> (StatusCode, Value) {
    let router = crate::app::Router::new()
        .merge(super::journeys::router())
        .with_state(crate::test_support::inert_app(pool.clone()));
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, format!("distant_signal_session={cookie}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn known_train(uid: &str, origin: Option<&str>, destination: Option<&str>) -> Value {
    let mut leg = json!({
        "mode": "knownTrain",
        "trainUid": uid,
        "serviceDate": service_date().to_string(),
    });
    if let Some(origin) = origin {
        leg["originCrs"] = json!(origin);
    }
    if let Some(destination) = destination {
        leg["destinationCrs"] = json!(destination);
    }
    leg
}

async fn trains_rows(pool: &PgPool, uid: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM trains WHERE train_uid = $1 AND service_date = $2")
        .bind(uid)
        .bind(service_date())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The text of a plain-text refusal.
fn text(body: &Value) -> &str {
    body.as_str().unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api leg_validation_db_tests \
            -- --ignored --test-threads=1`"]
async fn a_new_journey_s_known_train_leg_must_call_at_its_overrides() {
    let pool = connect().await;
    let _schedules = seed_schedules(&pool).await;
    let user = "TEST-LEGVAL-NEW";
    let cookie = seed_user(&pool, user).await;
    let create = |leg: Value| {
        let (pool, cookie) = (&pool, &cookie);
        async move { post(pool, "/Journeys", cookie, json!({ "leg": leg })).await }
    };

    // Each refusal: 422, a sentence the web app shows as is, and no
    // `trains` row minted for it.
    let cases = [
        (Some("ZQX"), Some("ZQG"), "doesn't stop at ZQX"),
        // A passing point is not a call.
        (Some("ZQC"), Some("ZQG"), "doesn't stop at ZQC"),
        (Some("ZQA"), Some("ZQX"), "doesn't stop at ZQX"),
        (Some("ZQE"), Some("ZQB"), "reaches ZQB before ZQE"),
        (Some("ZQD"), Some("ZQG"), "can't board train ZLV001 at ZQD"),
        (
            Some("ZQA"),
            Some("ZQF"),
            "can't get off train ZLV001 at ZQF",
        ),
        // Lowercase, untrimmed overrides are normalised first.
        (Some(" zqg "), None, "can't board train ZLV001 at ZQG"),
    ];
    for (origin, destination, expected) in cases {
        let (status, body) = create(known_train(ORDINARY, origin, destination)).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{origin:?} -> {destination:?}: {body}"
        );
        assert!(text(&body).contains(expected), "{body}");
        let saved = crate::data::journeys::count_manual_journeys_for_user(&pool, user)
            .await
            .unwrap();
        assert_eq!(saved, 0);
    }
    assert_eq!(trains_rows(&pool, ORDINARY).await, 0);

    // The happy path, and a request stop at either end.
    for (origin, destination) in [
        (Some("ZQB"), Some("ZQD")),
        (Some("ZQE"), Some("ZQG")),
        (Some("ZQA"), Some("ZQE")),
        (None, Some("ZQD")),
        (Some("ZQF"), None),
    ] {
        let (status, body) = create(known_train(ORDINARY, origin, destination)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{origin:?} -> {destination:?}: {body}"
        );
    }

    // The sleeper: Preston is pick-up only northbound, so a leg ending
    // there is refused; Euston to Stirling is fine, and boarding at a
    // set-down-only Scottish stop is not.
    let (status, body) = create(known_train(SLEEPER, Some("ZSE"), Some("ZSP"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        text(&body).contains("can't get off train ZLV002 at ZSP"),
        "{body}"
    );
    let (status, body) = create(known_train(SLEEPER, Some("ZSS"), Some("ZSI"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(text(&body).contains("can't board"), "{body}");
    let (status, body) = create(known_train(SLEEPER, Some("ZSW"), Some("ZSS"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Not matched yet: no schedule to check against, accepted as before.
    let (status, body) = create(known_train(UNMATCHED, Some("ZQX"), Some("ZQY"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    cleanup_user(&pool, user).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api leg_validation_db_tests \
            -- --ignored --test-threads=1`"]
async fn added_and_rebound_legs_are_checked_too() {
    let pool = connect().await;
    let _schedules = seed_schedules(&pool).await;
    let user = "TEST-LEGVAL-ADD";
    let cookie = seed_user(&pool, user).await;

    // A journey to add to: a window leg ZQB -> ZQG.
    let (status, created) = post(
        &pool,
        "/Journeys",
        &cookie,
        json!({ "leg": {
            "mode": "window",
            "originCrs": "ZQB",
            "destinationCrs": "ZQG",
            "serviceDate": service_date().to_string(),
            "departWindow": { "after": "08:00:00" },
        }}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let journey_id = created["journeyId"].as_i64().unwrap();
    let window_leg = created["legId"].as_i64().unwrap();

    // `POST /Journeys/{id}/legs`, known-train mode.
    let legs = format!("/Journeys/{journey_id}/legs");
    let (status, body) = post(
        &pool,
        &legs,
        &cookie,
        known_train(SLEEPER, Some("ZSE"), Some("ZSP")),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(text(&body).contains("only picks passengers up"), "{body}");
    let (status, body) = post(
        &pool,
        &legs,
        &cookie,
        known_train(ORDINARY, Some("ZQB"), Some("ZQE")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // `POST .../legs/{legId}/train`: binding a UID to the window leg checks
    // the leg's own ends. The sleeper never calls at ZQB.
    let bind = format!("/Journeys/{journey_id}/legs/{window_leg}/train");
    let (status, body) = post(
        &pool,
        &bind,
        &cookie,
        json!({ "trainUid": SLEEPER, "serviceDate": service_date().to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(text(&body).contains("doesn't stop at ZQB"), "{body}");
    assert_eq!(trains_rows(&pool, SLEEPER).await, 0);
    let (status, body) = post(
        &pool,
        &bind,
        &cookie,
        json!({ "trainUid": ORDINARY, "serviceDate": service_date().to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Unmatched: accepted, left to the resolution status.
    let (status, body) = post(
        &pool,
        &bind,
        &cookie,
        json!({ "trainUid": UNMATCHED, "serviceDate": service_date().to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The web planner's itinerary save: plain text, "Leg 2: ".
    let (status, body) = post(
        &pool,
        "/Journeys/itinerary",
        &cookie,
        json!({ "legs": [
            { "trainUid": ORDINARY, "serviceDate": service_date().to_string(),
              "originCrs": "ZQA", "destinationCrs": "ZQB" },
            { "trainUid": ORDINARY, "serviceDate": service_date().to_string(),
              "originCrs": "ZQD", "destinationCrs": "ZQG" },
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(text(&body).starts_with("Leg 2: You can't board"), "{body}");

    cleanup_user(&pool, user).await;
}
