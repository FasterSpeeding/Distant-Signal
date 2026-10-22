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
        "DELETE FROM tiploc_crs WHERE tiploc IN ('RPTSTC', 'RPTSTD')".to_string(),
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
    }
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

    // "I missed this train": a break with suggestions, never the missed
    // train itself.
    let (status, missed) = call(
        &router,
        "POST",
        &format!("/Journeys/{journey_id}/legs/{leg_id}/missed"),
        Some(&token),
        None,
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
    // "I missed this train" already counted as opened, in the app.
    assert_eq!(current["openedVia"], "in_app");

    // Someone else gets 404 everywhere.
    let other = seed_session(&pool, OTHER).await;
    for (method, uri) in [
        ("GET", format!("/Journeys/{journey_id}/breaks/current")),
        ("GET", format!("/Journeys/{journey_id}/revisions")),
        (
            "POST",
            format!("/Journeys/{journey_id}/legs/{leg_id}/missed"),
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
    assert_eq!(prefs, serde_json::json!({ "journeySuggestions": true }));
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
    assert_eq!(prefs, serde_json::json!({ "journeySuggestions": false }));
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
