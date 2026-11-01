//! The mid-journey routes against a live database (`DATABASE_URL`): a
//! synthetic day with three trains from ZRC to ZRD, a journey on the
//! first, and the whole flow (miss, alternatives, accept, history, Undo).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use chrono::NaiveDate;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::test_support::{FixtureCleanup, assert_synthetic_date};

const USER: &str = "TEST-REPLAN-USER";
const OTHER: &str = "TEST-REPLAN-OTHER";
const DATE: &str = "2099-05-05";

fn date() -> NaiveDate {
    let date: NaiveDate = DATE.parse().expect("date");
    assert_synthetic_date(date);
    date
}

async fn connect() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .connect(&database_url)
        .await
        .expect("connect to postgres")
}

/// 08:00 London on the fixture day, before the first train: the tracking
/// horizon and "now" are judged from here.
fn pin_now() -> crate::routes::PinnedLondonNow {
    use chrono::TimeZone;
    crate::routes::pin_london_now_for_tests(
        chrono::Utc.with_ymd_and_hms(2099, 5, 5, 7, 0, 0).unwrap(),
    )
}

async fn cleanup(pool: &PgPool) -> FixtureCleanup {
    let mut statements = Vec::new();
    for user in [USER, OTHER] {
        statements.extend([
            format!("DELETE FROM journeys WHERE user_id = '{user}'"),
            format!("DELETE FROM tracked_train_tickets WHERE user_id = '{user}'"),
            format!("DELETE FROM train_subscriptions WHERE user_id = '{user}'"),
            format!("DELETE FROM journey_idempotency_keys WHERE user_id = '{user}'"),
            format!("DELETE FROM notification_preferences WHERE user_id = '{user}'"),
            format!("DELETE FROM sessions WHERE user_id = '{user}'"),
            format!("DELETE FROM users WHERE id = '{user}'"),
        ]);
    }
    statements.extend([
        format!("DELETE FROM trains WHERE service_date = '{DATE}'"),
        format!("DELETE FROM schedule_calling_points_full WHERE service_date = '{DATE}'"),
        format!("DELETE FROM schedule_destination_departures WHERE service_date = '{DATE}'"),
        format!("DELETE FROM schedule_services WHERE service_date = '{DATE}'"),
        "DELETE FROM tiploc_crs WHERE tiploc IN ('RPTSTC', 'RPTSTD', 'RPTSTE')".to_string(),
    ]);
    FixtureCleanup::new(pool, statements).await
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(sql).execute(pool).await.expect(sql);
}

async fn seed_session(pool: &PgPool, user_id: &str) -> String {
    exec(
        pool,
        &format!(
            "INSERT INTO users (id, name) VALUES ('{user_id}', '{user_id}') ON CONFLICT DO NOTHING"
        ),
    )
    .await;
    let raw = format!("test-raw-session-token-for-{user_id}");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
         VALUES ($1, $2, NULL, NOW(), NOW() + make_interval(days => 14)) \
         ON CONFLICT (id) DO UPDATE SET user_id = EXCLUDED.user_id, \
             expires_at = EXCLUDED.expires_at",
    )
    .bind(hash_session_token(&raw))
    .bind(user_id)
    .execute(pool)
    .await
    .expect("seed session");
    raw
}

/// Three trains ZRC (RPTSTC) to ZRD (RPTSTD), all one operator: RP0001
/// 10:00-11:00, RP0002 10:30-11:30, RP0003 11:00-12:00.
async fn seed_timetable(pool: &PgPool) {
    exec(
        pool,
        "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) VALUES \
         ('RPTSTC', 'ZRC', 'Replan C', 'RPC', 1), ('RPTSTD', 'ZRD', 'Replan D', 'RPD', 1)",
    )
    .await;
    for (uid, dep, arr) in [
        ("RP0001", "10:00", "11:00"),
        ("RP0002", "10:30", "11:30"),
        ("RP0003", "11:00", "12:00"),
    ] {
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_calling_points_full \
                     (service_date, uid, seq, tiploc, kind, booked_departure, public_departure, \
                      booked_arrival, public_arrival, day_offset, can_board, can_alight) VALUES \
                 ('{DATE}', '{uid}', 1, 'RPTSTC', 'origin', '{dep}', '{dep}', NULL, NULL, 0, TRUE, TRUE), \
                 ('{DATE}', '{uid}', 2, 'RPTSTD', 'terminate', NULL, NULL, '{arr}', '{arr}', 0, TRUE, TRUE)"
            ),
        )
        .await;
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_destination_departures \
                     (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                      destination_arrival, operator_atoc, public_departure, \
                      public_destination_arrival) \
                 VALUES ('{DATE}', 'ZRD', '{dep}', '{uid}', 'ZRC', '{arr}', 'GR', '{dep}', '{arr}')"
            ),
        )
        .await;
        // Their operator, as the planner reads it (operator filtering P0).
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
                 VALUES ('{DATE}', '{uid}', 'train', 'P', 'GR')"
            ),
        )
        .await;
    }
}

/// Onward from ZRD (RPTSTD) to ZRE (RPTSTE): RP0004 11:10-12:00 and
/// RP0005 11:40-12:30, for a two-leg journey changing at ZRD.
async fn seed_onward(pool: &PgPool) {
    exec(
        pool,
        "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) VALUES \
         ('RPTSTE', 'ZRE', 'Replan E', 'RPE', 1)",
    )
    .await;
    for (uid, dep, arr) in [("RP0004", "11:10", "12:00"), ("RP0005", "11:40", "12:30")] {
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_calling_points_full \
                     (service_date, uid, seq, tiploc, kind, booked_departure, public_departure, \
                      booked_arrival, public_arrival, day_offset, can_board, can_alight) VALUES \
                 ('{DATE}', '{uid}', 1, 'RPTSTD', 'origin', '{dep}', '{dep}', NULL, NULL, 0, TRUE, TRUE), \
                 ('{DATE}', '{uid}', 2, 'RPTSTE', 'terminate', NULL, NULL, '{arr}', '{arr}', 0, TRUE, TRUE)"
            ),
        )
        .await;
        exec(
            pool,
            &format!(
                "INSERT INTO schedule_destination_departures \
                     (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                      destination_arrival, operator_atoc, public_departure, \
                      public_destination_arrival) \
                 VALUES ('{DATE}', 'ZRE', '{dep}', '{uid}', 'ZRD', '{arr}', 'GR', '{dep}', '{arr}')"
            ),
        )
        .await;
    }
}

/// Live state for `uid`'s train: `status`, `delay` minutes late, reported
/// at `updated_at` (an RFC 3339 instant).
async fn seed_live(pool: &PgPool, uid: &str, status: &str, delay: i32, updated_at: &str) {
    let train =
        format!("SELECT id FROM trains WHERE service_date = '{DATE}' AND train_uid = '{uid}'");
    exec(
        pool,
        &format!("DELETE FROM train_current_state WHERE trains_id IN ({train})"),
    )
    .await;
    exec(
        pool,
        &format!(
            "INSERT INTO train_current_state (trains_id, status, delay_minutes, updated_at) \
             SELECT id, '{status}', {delay}, '{updated_at}' FROM ({train}) t"
        ),
    )
    .await;
}

async fn break_count(pool: &PgPool, journey_id: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM journey_breaks WHERE journey_id = $1")
        .bind(journey_id)
        .fetch_one(pool)
        .await
        .expect("count breaks")
}

fn router(pool: PgPool) -> axum::Router {
    crate::app::Router::new()
        .merge(crate::routes::journeys::router())
        .nest("/public", crate::routes::preferences::router())
        .with_state(crate::test_support::inert_app(pool))
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
    idempotency_key: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
    }
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key);
    }
    let request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("request");
    let response = router.clone().oneshot(request).await.expect("oneshot");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, value)
}

/// A one-leg journey on RP0001, saved as the planner saves one.
async fn create_journey(router: &axum::Router, token: &str) -> (i64, i64) {
    let (status, body) = call(
        router,
        "POST",
        "/Journeys/itinerary",
        Some(token),
        Some(serde_json::json!({
            "legs": [{
                "trainUid": "RP0001", "serviceDate": DATE,
                "originCrs": "ZRC", "destinationCrs": "ZRD"
            }]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (
        body["journeyId"].as_i64().expect("journeyId"),
        body["legId"].as_i64().expect("legId"),
    )
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
#[expect(
    clippy::too_many_lines,
    reason = "test code: one scenario, read top to bottom"
)]
async fn a_missed_train_is_replaced_kept_in_history_and_undone() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;

    // Nothing broken yet: no banner, one plan.
    let (status, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert!(detail["openBreak"].is_null(), "{detail}");
    assert_eq!(detail["revisionCount"], 1, "{detail}");
    assert_eq!(detail["suggestionsMuted"], false, "{detail}");
    assert_eq!(detail["legs"][0]["replacesLegIds"], serde_json::json!([]));

    // A traveller-marked miss left open from before "I missed this train"
    // stopped opening breaks (an Undo can reopen one too), planned: its
    // suggestions, never the missed train itself.
    let seeded: i64 = sqlx::query_scalar(
        "INSERT INTO journey_breaks (journey_id, user_id, cause, fault, severity, \
             trigger_leg_id, break_position, fingerprint) \
         VALUES ($1, $2, 'user_missed', 'user', 1, $3, 1, '') RETURNING id",
    )
    .bind(journey_id)
    .bind(USER)
    .bind(leg_id)
    .fetch_one(&pool)
    .await
    .expect("seed break");
    let (status, missed) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/replan/preview"),
        Some(&token),
        Some(serde_json::json!({ "breakId": seeded })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{missed}");
    assert_eq!(missed["cause"], "user_missed");
    assert_eq!(missed["fault"], "user");
    assert_eq!(missed["triggerLegId"], leg_id);
    let suggestions = missed["suggestions"]["suggestions"]
        .as_array()
        .expect("suggestions");
    let uids: Vec<&str> = suggestions
        .iter()
        .filter_map(|s| s["legs"][0]["trainUid"].as_str())
        .collect();
    assert!(uids.contains(&"RP0002"), "{missed}");
    assert!(!uids.contains(&"RP0001"), "{missed}");
    let first = &suggestions[0];
    assert!(first["card"].as_bool().unwrap_or_default());
    assert!(
        first["pickLabels"]
            .as_array()
            .is_some_and(|labels| labels.contains(&Value::from("Recommended"))),
        "{first}"
    );
    // With no ticket, the sheet asks for one; a passenger-fault miss warns.
    assert!(missed["suggestions"]["hint"].is_string(), "{missed}");
    let break_id = missed["id"].as_i64().expect("break id");

    // The banner on the journey page, and the open break with `stale`.
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(detail["openBreak"]["id"], break_id, "{detail}");
    assert_eq!(detail["openBreak"]["suggestionsReady"], true, "{detail}");
    let (status, current) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/breaks/current?src=push"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["id"], break_id);
    // Stamped as first opened from the push.
    let opened_via: Option<String> =
        sqlx::query_scalar("SELECT opened_via FROM journey_breaks WHERE id = $1")
            .bind(break_id)
            .fetch_one(&pool)
            .await
            .expect("break");
    assert_eq!(opened_via.as_deref(), Some("push"));

    // Someone else gets 404 everywhere.
    let other = seed_session(&pool, OTHER).await;
    for (method, uri) in [
        ("GET", format!("/Journeys/{journey_id}/breaks/current")),
        ("GET", format!("/Journeys/{journey_id}/revisions")),
        (
            "GET",
            format!("/Journeys/{journey_id}/legs/{leg_id}/alternatives?cause=missed"),
        ),
    ] {
        let (status, body) = call(&router, method, &uri, Some(&other), None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {body}");
    }
    // And no session at all is a 401.
    let (status, _) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A ticket on the booked train: it must follow the change, never be
    // cascaded away.
    let (ticket_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tracked_train_tickets (tracked_train_id, user_id, ticket_type, source) \
         SELECT train_subscription_id, $2, 'Advance Single', 'manual' FROM journey_legs \
         WHERE id = $1 RETURNING id",
    )
    .bind(leg_id)
    .bind(USER)
    .fetch_one(&pool)
    .await
    .expect("seed ticket");

    // Take RP0002, twice with one Idempotency-Key: one change.
    let pick = suggestions
        .iter()
        .find(|s| s["legs"][0]["trainUid"] == "RP0002")
        .expect("RP0002 offered");
    let accept_body = serde_json::json!({
        "breakId": break_id,
        "suggestionId": pick["id"],
        "fingerprint": missed["suggestions"]["fingerprint"],
    });
    let (status, accepted) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/replan/accept"),
        Some(&token),
        Some(accept_body.clone()),
        Some("accept-once"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["revisionNo"], 2, "{accepted}");
    assert_eq!(accepted["undoRevisionNo"], 1, "{accepted}");
    let (status, again) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/replan/accept"),
        Some(&token),
        Some(accept_body),
        Some("accept-once"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, accepted);

    // The journey now rides RP0002; the booked plan is kept, the missed
    // leg marked missed, and this passenger-fault change is the new
    // compensation baseline.
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(
        detail["legs"][0]["trackedTrainState"]["trainUid"], "RP0002",
        "{detail}"
    );
    assert_eq!(detail["revisionCount"], 2);
    assert_eq!(
        detail["legs"][0]["replacesLegIds"],
        serde_json::json!([leg_id])
    );
    assert!(detail["openBreak"].is_null(), "{detail}");
    let (moved,): (bool,) = sqlx::query_as(
        "SELECT t.tracked_train_id = jl.train_subscription_id FROM tracked_train_tickets t, \
         journey_legs jl WHERE t.id = $1 AND jl.journey_id = $2",
    )
    .bind(ticket_id)
    .bind(journey_id)
    .fetch_one(&pool)
    .await
    .expect("ticket");
    assert!(moved, "the ticket moved to the same operator's new train");
    let (status, history) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let revisions = history["revisions"].as_array().expect("revisions");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0]["revisionNo"], 2);
    assert_eq!(revisions[0]["cause"], "user_missed");
    assert_eq!(revisions[0]["isCompensationBaseline"], true);
    assert_eq!(revisions[1]["reason"], "booked");
    assert_eq!(revisions[1]["legs"][0]["trainUid"], "RP0001");
    assert_eq!(revisions[1]["legs"][0]["fate"], "missed");
    assert_eq!(revisions[1]["canRevert"], true, "{history}");

    // Undo: back to the booked train, as revision 3, and the break is open
    // again.
    let (status, undone) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/revisions/1/revert"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["revisionNo"], 3);
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(
        detail["legs"][0]["trackedTrainState"]["trainUid"], "RP0001",
        "{detail}"
    );
    assert_eq!(detail["openBreak"]["id"], break_id, "{detail}");

    // The ticket is back on the booked train's subscription.
    let (on_leg,): (bool,) = sqlx::query_as(
        "SELECT t.tracked_train_id = jl.train_subscription_id FROM tracked_train_tickets t, \
         journey_legs jl WHERE t.id = $1 AND jl.journey_id = $2",
    )
    .bind(ticket_id)
    .bind(journey_id)
    .fetch_one(&pool)
    .await
    .expect("ticket");
    assert!(on_leg, "the ticket follows the plan");

    // Dismissed, it is gone.
    let (status, body) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/breaks/{break_id}/dismiss"),
        Some(&token),
        Some(serde_json::json!({ "reason": "willMakeIt" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/breaks/current"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn choosing_another_train_before_travel_updates_the_booked_plan() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;

    let (status, alternatives) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/alternatives?cause=choice"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{alternatives}");
    assert!(
        alternatives["suggestions"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "{alternatives}"
    );

    let (status, replaced) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/replace"),
        Some(&token),
        Some(serde_json::json!({
            "cause": "choice",
            "legs": [{
                "trainUid": "RP0003", "serviceDate": DATE,
                "originCrs": "ZRC", "destinationCrs": "ZRD"
            }]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    // Before travel and with no break, the booked plan itself changes.
    assert_eq!(replaced["revisionNo"], 1, "{replaced}");
    assert!(replaced["undoRevisionNo"].is_null());
    let (_, history) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        Some(&token),
        None,
        None,
    )
    .await;
    let revisions = history["revisions"].as_array().expect("revisions");
    assert_eq!(revisions.len(), 1, "{history}");
    assert_eq!(revisions[0]["legs"][0]["trainUid"], "RP0003");
    assert_eq!(date().to_string(), DATE);
}

/// Taking another train after "I missed this train" before the journey
/// starts is still a missed train, not a planning edit: its own revision,
/// with the booked train kept as missed and an Undo, and no break.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_train_missed_before_departure_is_its_own_plan_with_undo() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;

    let (status, replaced) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/replace"),
        Some(&token),
        Some(serde_json::json!({
            "cause": "missed",
            "legs": [{
                "trainUid": "RP0002", "serviceDate": DATE,
                "originCrs": "ZRC", "destinationCrs": "ZRD"
            }]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(replaced["revisionNo"], 2, "{replaced}");
    assert_eq!(replaced["undoRevisionNo"], 1, "{replaced}");
    let (_, history) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        Some(&token),
        None,
        None,
    )
    .await;
    let revisions = history["revisions"].as_array().expect("revisions");
    assert_eq!(revisions[0]["cause"], "user_missed", "{history}");
    assert_eq!(revisions[0]["fault"], "user", "{history}");
    assert_eq!(revisions[1]["legs"][0]["trainUid"], "RP0001", "{history}");
    assert_eq!(revisions[1]["legs"][0]["fate"], "missed", "{history}");
    assert_eq!(break_count(&pool, journey_id).await, 0);
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn suggestions_can_be_muted_per_journey_and_turned_off() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, _) = create_journey(&router, &token).await;

    let (status, _) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/suggestions-muted"),
        Some(&token),
        Some(serde_json::json!({ "muted": true })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(detail["suggestionsMuted"], true);

    let (status, prefs) = call(
        &router,
        "GET",
        "/public/preferences/notifications",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prefs}");
    assert_eq!(prefs["journeySuggestions"], true, "{prefs}");
    let (status, _) = call(
        &router,
        "PUT",
        "/public/preferences/notifications",
        Some(&token),
        Some(serde_json::json!({ "journeySuggestions": false })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, prefs) = call(
        &router,
        "GET",
        "/public/preferences/notifications",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(prefs["journeySuggestions"], false, "{prefs}");
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_cancelled_train_opens_a_break_on_read_and_it_recovers() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;

    exec(
        &pool,
        &format!(
            "INSERT INTO train_current_state (trains_id, status) \
             SELECT id, 'cancelled' FROM trains WHERE service_date = '{DATE}' AND train_uid = 'RP0001'"
        ),
    )
    .await;
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    let open = &detail["openBreak"];
    assert_eq!(open["cause"], "cancelled", "{detail}");
    assert_eq!(open["triggerLegId"], leg_id);
    assert_eq!(open["suggestionsReady"], false);
    let break_id = open["id"].as_i64().expect("break id");

    // The sheet re-plans the stale (unplanned) break.
    let (status, preview) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/replan/preview"),
        Some(&token),
        Some(serde_json::json!({ "breakId": break_id })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["stale"], false, "{preview}");
    assert!(
        preview["suggestions"]["suggestions"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "{preview}"
    );
    // The journey list says so too.
    let (_, mine) = call(&router, "GET", "/Journeys/mine", Some(&token), None, None).await;
    let row = mine
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == journey_id))
        .expect("journey listed");
    assert_eq!(row["openBreakId"], break_id, "{row}");

    // Reinstated: the next read ends the break.
    exec(
        &pool,
        &format!(
            "UPDATE train_current_state SET status = 'awaiting_activation' WHERE trains_id IN \
             (SELECT id FROM trains WHERE service_date = '{DATE}' AND train_uid = 'RP0001')"
        ),
    )
    .await;
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert!(detail["openBreak"].is_null(), "{detail}");
    let (status,): (String,) = sqlx::query_as("SELECT status FROM journey_breaks WHERE id = $1")
        .bind(break_id)
        .fetch_one(&pool)
        .await
        .expect("break");
    assert_eq!(status, "recovered");
}

#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn the_sweep_opens_a_break_plans_it_and_ends_it_once_recovered() {
    use chrono::TimeZone;
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;
    let app = crate::test_support::inert_app(pool.clone());
    let now = chrono::Utc.with_ymd_and_hms(2099, 5, 5, 8, 30, 0).unwrap();

    // Nothing broken: nothing written.
    let swept = crate::journey_break_sweep::run_sweep(&app, now)
        .await
        .expect("sweep");
    assert!(swept.journeys >= 1, "{swept:?}");
    assert_eq!(swept.breaks, 0);

    exec(
        &pool,
        &format!(
            "INSERT INTO train_current_state (trains_id, status) \
             SELECT id, 'cancelled' FROM trains WHERE service_date = '{DATE}' AND train_uid = 'RP0001'"
        ),
    )
    .await;
    let swept = crate::journey_break_sweep::run_sweep(&app, now)
        .await
        .expect("sweep");
    assert_eq!((swept.breaks, swept.planned), (1, 1), "{swept:?}");
    let row = crate::data::journey_breaks::open_break(&pool, journey_id)
        .await
        .expect("read")
        .expect("open break");
    assert_eq!(row.cause, "cancelled");
    assert_eq!(row.trigger_leg_id, Some(leg_id));
    let suggestions = row.suggestions.expect("planned");
    assert!(
        suggestions["suggestions"]
            .as_array()
            .is_some_and(|s| !s.is_empty()),
        "{suggestions}"
    );
    assert_eq!(suggestions["trigger"]["legId"], leg_id);

    // Seen again unchanged: confirmed, not planned again.
    let swept = crate::journey_break_sweep::run_sweep(&app, now)
        .await
        .expect("sweep");
    assert_eq!((swept.breaks, swept.planned), (1, 0), "{swept:?}");
    let row = crate::data::journey_breaks::open_break(&pool, journey_id)
        .await
        .expect("read")
        .expect("open break");
    assert_eq!(row.confirmations, 2);

    exec(
        &pool,
        &format!(
            "UPDATE train_current_state SET status = 'awaiting_activation' WHERE trains_id IN \
             (SELECT id FROM trains WHERE service_date = '{DATE}' AND train_uid = 'RP0001')"
        ),
    )
    .await;
    let swept = crate::journey_break_sweep::run_sweep(&app, now)
        .await
        .expect("sweep");
    assert_eq!(swept.recovered, 1, "{swept:?}");
    assert!(
        crate::data::journey_breaks::open_break(&pool, journey_id)
            .await
            .expect("read")
            .is_none()
    );
}

/// The prod report (2026-10-22): "You missed the ..." on the first leg of a
/// journey whose train left on time. Departure time passing is no evidence
/// of a miss, and looking at the options "I missed this train" offers is
/// not saying so: neither opens a break. Only taking another train records
/// the miss, and only then as the traveller's.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn an_on_time_first_leg_after_departure_is_never_missed_until_the_traveller_says_so() {
    use chrono::TimeZone;
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;

    // 10:15 London: RP0001 left on time at 10:00 and is on its way.
    let _later = crate::routes::pin_london_now_for_tests(
        chrono::Utc.with_ymd_and_hms(2099, 5, 5, 9, 15, 0).unwrap(),
    );
    seed_live(&pool, "RP0001", "en_route", 0, "2099-05-05T09:14:00Z").await;
    let journey = format!("/Journeys/{journey_id}");
    let (status, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert!(detail["openBreak"].is_null(), "{detail}");
    assert!(detail["legs"][0]["connection"].is_null(), "{detail}");

    // "I missed this train" shows the options, and that is all it does.
    let (status, alternatives) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/alternatives?cause=missed"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{alternatives}");
    let offered: Vec<&str> = alternatives["suggestions"]
        .as_array()
        .expect("suggestions")
        .iter()
        .filter_map(|s| s["legs"][0]["trainUid"].as_str())
        .collect();
    assert!(offered.contains(&"RP0002"), "{alternatives}");
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert!(detail["openBreak"].is_null(), "{detail}");
    assert_eq!(break_count(&pool, journey_id).await, 0);

    // The old "mark it missed" route is gone: a stale tab calling it gets
    // a plain 404, and nothing is written.
    let (status, _) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/missed"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(break_count(&pool, journey_id).await, 0);

    // Taking RP0002 is the traveller saying they missed RP0001: their own
    // fault (no change onto a first leg), kept in the history, no banner.
    let (status, replaced) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/replace"),
        Some(&token),
        Some(serde_json::json!({
            "cause": "missed",
            "legs": [{
                "trainUid": "RP0002", "serviceDate": DATE,
                "originCrs": "ZRC", "destinationCrs": "ZRD"
            }]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    let (_, history) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/revisions"),
        Some(&token),
        None,
        None,
    )
    .await;
    let revisions = history["revisions"].as_array().expect("revisions");
    assert_eq!(revisions[0]["cause"], "user_missed", "{history}");
    assert_eq!(revisions[0]["fault"], "user", "{history}");
    assert_eq!(revisions[1]["legs"][0]["fate"], "missed", "{history}");
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert!(detail["openBreak"].is_null(), "{detail}");
    assert_eq!(break_count(&pool, journey_id).await, 0);
}

/// A later leg is missed only when the inbound train's live arrival leaves
/// too little time to change: on time, nothing; 30 minutes late into a
/// 10-minute change, the connection is missed and the page opens the
/// break on the later leg, never on the first.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_late_inbound_breaks_the_connection_onto_the_later_leg_only() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    seed_onward(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (status, body) = call(
        &router,
        "POST",
        "/Journeys/itinerary",
        Some(&token),
        Some(serde_json::json!({
            "legs": [
                { "trainUid": "RP0001", "serviceDate": DATE, "originCrs": "ZRC", "destinationCrs": "ZRD" },
                { "trainUid": "RP0004", "serviceDate": DATE, "originCrs": "ZRD", "destinationCrs": "ZRE" }
            ]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let journey_id = body["journeyId"].as_i64().expect("journeyId");
    let journey = format!("/Journeys/{journey_id}");

    seed_live(&pool, "RP0001", "en_route", 0, "2099-05-05T07:00:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert!(detail["openBreak"].is_null(), "{detail}");
    assert!(detail["legs"][0]["connection"].is_null(), "{detail}");
    assert_eq!(detail["legs"][1]["connection"]["state"], "ok", "{detail}");

    seed_live(&pool, "RP0001", "en_route", 30, "2099-05-05T07:00:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert!(detail["legs"][0]["connection"].is_null(), "{detail}");
    assert_eq!(
        detail["legs"][1]["connection"]["state"], "missed",
        "{detail}"
    );
    let open = &detail["openBreak"];
    assert_eq!(open["cause"], "connection_missed", "{detail}");
    assert_eq!(open["triggerLegId"], detail["legs"][1]["id"], "{detail}");
}

/// The cleanup migration: an open `user_missed` break (left by the old
/// sheet, which opened one just by loading) is dismissed, a settled one is
/// left alone, and a second run changes nothing.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn the_cleanup_dismisses_unconfirmed_missed_breaks_once() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (journey_id, leg_id) = create_journey(&router, &token).await;
    let insert = |status: &str| {
        format!(
            "INSERT INTO journey_breaks (journey_id, user_id, cause, fault, severity, \
                 trigger_leg_id, break_position, fingerprint, status) \
             VALUES ({journey_id}, '{USER}', 'user_missed', 'user', 1, {leg_id}, 1, 'f', \
                 '{status}') RETURNING id"
        )
    };
    let settled: i64 = sqlx::query_scalar(&insert("accepted"))
        .fetch_one(&pool)
        .await
        .expect("settled break");
    let open: i64 = sqlx::query_scalar(&insert("open"))
        .fetch_one(&pool)
        .await
        .expect("open break");
    let journey = format!("/Journeys/{journey_id}");
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert_eq!(detail["openBreak"]["id"], open, "{detail}");

    let migration = include_str!(
        "../../../../ds-store/migrations/20261023091000_journey_breaks_close_unconfirmed_missed.sql"
    );
    for _ in 0..2 {
        let mut tx = pool.begin().await.expect("begin");
        sqlx::raw_sql(migration)
            .execute(&mut *tx)
            .await
            .expect("cleanup");
        tx.commit().await.expect("commit");
    }
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, status, dismiss_reason FROM journey_breaks WHERE journey_id = $1 ORDER BY id",
    )
    .bind(journey_id)
    .fetch_all(&pool)
    .await
    .expect("breaks");
    assert_eq!(
        rows,
        vec![
            (settled, "accepted".to_string(), None),
            (open, "dismissed".to_string(), Some("other".to_string())),
        ]
    );
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert!(detail["openBreak"].is_null(), "{detail}");
}

/// DS-141: a change that works only because the next train is running
/// late. RP0001 (ZRC 10:00 to ZRD 11:00) then RP0004 (ZRD 11:10), five
/// minutes to change at ZRD; "now" is 10:30. RP0001 30 late is in at 11:30
/// and RP0004 30 late leaves 11:40: the change is `ok`, and flagged, since
/// on time RP0004 would leave 25 minutes before the traveller could reach
/// it. With RP0001 on time the change works on time too: not flagged. The
/// alternatives from RP0001 carry the same flag on a late RP0004 boarded
/// from RP0002 (in 11:30).
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_change_that_relies_on_a_late_next_train_is_flagged() {
    use chrono::TimeZone;
    let _now = crate::routes::pin_london_now_for_tests(
        chrono::Utc.with_ymd_and_hms(2099, 5, 5, 9, 30, 0).unwrap(),
    );
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    seed_onward(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (status, body) = call(
        &router,
        "POST",
        "/Journeys/itinerary",
        Some(&token),
        Some(serde_json::json!({
            "legs": [
                { "trainUid": "RP0001", "serviceDate": DATE, "originCrs": "ZRC", "destinationCrs": "ZRD" },
                { "trainUid": "RP0004", "serviceDate": DATE, "originCrs": "ZRD", "destinationCrs": "ZRE" }
            ]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let journey_id = body["journeyId"].as_i64().expect("journeyId");
    let journey = format!("/Journeys/{journey_id}");

    seed_live(&pool, "RP0001", "en_route", 30, "2099-05-05T09:29:00Z").await;
    seed_live(&pool, "RP0004", "en_route", 30, "2099-05-05T09:29:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    let connection = &detail["legs"][1]["connection"];
    assert_eq!(connection["state"], "ok", "{detail}");
    assert_eq!(connection["gapMinutes"], 10, "{detail}");
    assert_eq!(
        connection["reliesOnLateDeparture"],
        serde_json::json!({
            "scheduledDeparture": "2099-05-05T10:10:00Z",
            "expectedDeparture": "2099-05-05T10:40:00Z",
            "delayMinutes": 30,
            "slackIfOnTimeMinutes": -25,
        }),
        "{detail}"
    );
    assert!(detail["openBreak"].is_null(), "{detail}");

    seed_live(&pool, "RP0001", "en_route", 0, "2099-05-05T09:29:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    let connection = &detail["legs"][1]["connection"];
    assert_eq!(connection["state"], "ok", "{detail}");
    assert!(
        connection.get("reliesOnLateDeparture").is_none(),
        "{detail}"
    );

    let leg_id = detail["legs"][0]["id"].as_i64().expect("leg id");
    let (status, alternatives) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/alternatives?cause=choice"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{alternatives}");
    let suggestions = alternatives["suggestions"].as_array().expect("suggestions");
    let mut flagged = 0;
    for suggestion in suggestions {
        let legs = suggestion["legs"].as_array().expect("legs");
        for pair in legs.windows(2) {
            let reliance = pair[1].get("reliesOnLateDeparture");
            if pair[0]["trainUid"] == "RP0002" && pair[1]["trainUid"] == "RP0004" {
                let reliance = reliance.expect("RP0002 into RP0004 relies on it running late");
                assert_eq!(reliance["delayMinutes"], 30, "{suggestion}");
                flagged += 1;
            } else {
                assert!(reliance.is_none(), "{suggestion}");
            }
        }
    }
    assert!(flagged > 0, "{alternatives}");
}

/// Operator filtering P5 (spec 2026-10-31 §5.6, §6.9): a journey keeps the
/// operator filter its plan was saved with, its page echoes it, `PUT
/// .../operator-filter` changes or clears it, and the replans honour it,
/// falling back to the best unfiltered route, flagged, when it leaves none.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
#[expect(
    clippy::too_many_lines,
    reason = "test code: one scenario, read top to bottom"
)]
async fn a_journeys_operator_filter_is_kept_echoed_and_honoured_by_replans() {
    let _now = pin_now();
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());

    // Saved with "only LNER" (every fixture train is LNER).
    let (status, body) = call(
        &router,
        "POST",
        "/Journeys/itinerary",
        Some(&token),
        Some(serde_json::json!({
            "legs": [{
                "trainUid": "RP0001", "serviceDate": DATE,
                "originCrs": "ZRC", "destinationCrs": "ZRD"
            }],
            "operatorFilter": {"mode": "only", "entries": ["gr"]}
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let journey_id = body["journeyId"].as_i64().unwrap();
    let leg_id = body["legId"].as_i64().unwrap();
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(
        detail["operatorFilter"],
        serde_json::json!({"mode": "only", "codes": ["GR"], "entries": ["GR"]}),
        "{detail}"
    );

    // A missed train: the replan rides LNER, no flag.
    let seeded: i64 = sqlx::query_scalar(
        "INSERT INTO journey_breaks (journey_id, user_id, cause, fault, severity, \
             trigger_leg_id, break_position, fingerprint) \
         VALUES ($1, $2, 'user_missed', 'user', 1, $3, 1, '') RETURNING id",
    )
    .bind(journey_id)
    .bind(USER)
    .bind(leg_id)
    .fetch_one(&pool)
    .await
    .expect("seed break");
    let preview = |body: Value| {
        let router = router.clone();
        let token = token.clone();
        async move {
            call(
                &router,
                "POST",
                &format!("/Journeys/{journey_id}/replan/preview"),
                Some(&token),
                Some(body),
                None,
            )
            .await
        }
    };
    let (status, missed) = preview(serde_json::json!({ "breakId": seeded })).await;
    assert_eq!(status, StatusCode::OK, "{missed}");
    let suggestions = missed["suggestions"]["suggestions"].as_array().unwrap();
    assert!(!suggestions.is_empty(), "{missed}");
    assert!(
        suggestions
            .iter()
            .all(|s| s.get("outsideOperatorFilter").is_none()),
        "{missed}"
    );

    // Avoiding LNER leaves nothing: the best route anyway, flagged.
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&token),
        Some(serde_json::json!({ "operatorFilter": {"mode": "avoid", "entries": ["GR"]} })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, avoided) = preview(serde_json::json!({ "breakId": seeded })).await;
    assert_eq!(status, StatusCode::OK, "{avoided}");
    let suggestions = avoided["suggestions"]["suggestions"].as_array().unwrap();
    assert_eq!(suggestions.len(), 1, "{avoided}");
    assert_eq!(suggestions[0]["outsideOperatorFilter"], true, "{avoided}");

    // Preferring LNER labels its route.
    call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&token),
        Some(serde_json::json!({ "operatorFilter": {"mode": "prefer", "entries": ["GR"]} })),
        None,
    )
    .await;
    let (_, preferred) = preview(serde_json::json!({ "breakId": seeded })).await;
    let suggestions = preferred["suggestions"]["suggestions"].as_array().unwrap();
    assert!(
        suggestions
            .iter()
            .any(|s| s["operatorOnly"] == serde_json::json!(["GR"])),
        "{preferred}"
    );

    // A bad filter is a 400; someone else's journey a 404; null clears it.
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&token),
        Some(serde_json::json!({ "operatorFilter": {"mode": "sometimes", "entries": ["GR"]} })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&token),
        Some(serde_json::json!({ "operatorFilter": {"mode": "only", "entries": ["Q9"]} })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let other = seed_session(&pool, OTHER).await;
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&other),
        Some(serde_json::json!({ "operatorFilter": null })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/Journeys/{journey_id}/operator-filter"),
        Some(&token),
        Some(serde_json::json!({ "operatorFilter": null })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = call(
        &router,
        "GET",
        &format!("/Journeys/{journey_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert!(detail.get("operatorFilter").is_none(), "{detail}");

    // Saving with a bad filter is refused, and saves nothing.
    let (status, body) = call(
        &router,
        "POST",
        "/Journeys/itinerary",
        Some(&token),
        Some(serde_json::json!({
            "legs": [{"trainUid": "RP0002", "serviceDate": DATE}],
            "operatorFilter": {"mode": "only", "entries": []}
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// A timetabled tram leg (DS-237) between RP0001 and RP0005, on the test
/// transit network: the tram line every ten minutes from 11:05 London
/// (20 minutes to the far stop), the leg planned on the 11:05. Six minutes
/// late into ZRD, the traveller misses it and takes the 11:15, in at
/// 11:35, leaving five minutes for the 11:40: at risk. Shifting the
/// planned ride instead would have ended it at 11:26 and called the
/// change fine.
#[tokio::test]
#[ignore = "requires a live database: DATABASE_URL"]
async fn a_late_train_re_times_a_timetabled_leg_and_judges_the_next_change_from_it() {
    use common::transit::{TimetableRows, TransitTrip};
    use ds_store::test_support::transit::{A, B, cleanup as transit_cleanup, feed, seed_feed};
    const NETWORK: &str = "test-replan-tt";
    let _now = pin_now();
    let _switch = crate::data::transit_planning::set_switch_for_tests(&[NETWORK]);
    let pool = connect().await;
    let _cleanup = cleanup(&pool).await;
    seed_timetable(&pool).await;
    seed_onward(&pool).await;
    let (mut parts, mut manifest) = feed(NETWORK, date());
    let starts = [11 * 3600 + 300, 11 * 3600 + 900, 11 * 3600 + 1500];
    for (part, _) in &mut parts {
        match &mut part.rows {
            TimetableRows::Patterns(patterns) => {
                for pattern in patterns.iter_mut().filter(|p| p.pattern_id == "out") {
                    pattern.stops[1].arr_offset_s = 1200;
                    pattern.stops[1].dep_offset_s = 1200;
                }
            }
            TimetableRows::Trips(trips) => {
                *trips = starts
                    .iter()
                    .map(|start| TransitTrip {
                        trip_id: format!("T{start}"),
                        line_id: Some("blue".to_owned()),
                        pattern_id: "out".to_owned(),
                        service_id: "s1".to_owned(),
                        start_s: *start,
                        headsign: Some("Bbb".to_owned()),
                        replacement_bus: false,
                    })
                    .collect();
            }
            _ => {}
        }
    }
    manifest.counts.trips = 3;
    seed_feed(&pool, &parts, &manifest, date()).await;

    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());
    let (status, body) = call(
        &router,
        "POST",
        "/Journeys/itinerary",
        Some(&token),
        Some(serde_json::json!({
            "legs": [
                { "trainUid": "RP0001", "serviceDate": DATE, "originCrs": "ZRC", "destinationCrs": "ZRD" },
                {
                    "kind": "untracked", "transitMode": "tram",
                    "originStopId": A, "destinationStopId": B,
                    "plannedDeparture": "2099-05-05T10:05:00Z", "plannedArrival": "2099-05-05T10:25:00Z",
                    "transitNetwork": NETWORK, "transitLineId": "blue", "headsign": "Bbb"
                },
                { "trainUid": "RP0005", "serviceDate": DATE, "originCrs": "ZRD", "destinationCrs": "ZRE" }
            ]
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let journey = format!(
        "/Journeys/{}",
        body["journeyId"].as_i64().expect("journeyId")
    );

    // On time: the planned tram, and the 11:40 with 15 minutes to spare.
    seed_live(&pool, "RP0001", "en_route", 0, "2099-05-05T07:00:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    assert_eq!(
        detail["legs"][1]["timetable"]["state"], "matched",
        "{detail}"
    );
    assert!(
        detail["legs"][1]["connection"]
            .get("nextTimetabled")
            .is_none(),
        "{detail}"
    );
    assert_eq!(detail["legs"][2]["connection"]["state"], "ok", "{detail}");

    seed_live(&pool, "RP0001", "en_route", 6, "2099-05-05T07:00:00Z").await;
    let (_, detail) = call(&router, "GET", &journey, Some(&token), None, None).await;
    let onto_tram = &detail["legs"][1]["connection"];
    assert_eq!(onto_tram["state"], "ok", "{detail}");
    assert_eq!(onto_tram["gapMinutes"], 9, "{detail}");
    assert_eq!(
        onto_tram["nextTimetabled"],
        serde_json::json!({
            "departure": "2099-05-05T10:15:00Z",
            "arrival": "2099-05-05T10:35:00Z",
        })
    );
    assert_eq!(
        detail["legs"][2]["connection"]["state"], "atRisk",
        "{detail}"
    );
    assert_eq!(detail["legs"][2]["connection"]["gapMinutes"], 5, "{detail}");
    transit_cleanup(&pool, NETWORK).await;
}
