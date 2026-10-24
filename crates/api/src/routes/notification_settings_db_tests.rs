//! The notification settings routes against a live database
//! (`DATABASE_URL`): turning push off on one device, the device list, the
//! per-train switch, the per-type preferences and a pinned line's alert
//! switch. Each test owns its `TEST-NOTIFSET-*` users.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::test_support::FixtureCleanup;

async fn connect() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect to postgres")
}

/// Removes everything `users` own (their rows cascade from `users`) now and
/// again when the test ends.
async fn cleanup(pool: &PgPool, users: &[&str]) -> FixtureCleanup {
    let list = users
        .iter()
        .map(|user| format!("'{user}'"))
        .collect::<Vec<_>>()
        .join(", ");
    FixtureCleanup::new(
        pool,
        [
            format!("DELETE FROM journeys WHERE user_id IN ({list})"),
            format!("DELETE FROM train_subscriptions WHERE user_id IN ({list})"),
            format!("DELETE FROM sessions WHERE user_id IN ({list})"),
            format!("DELETE FROM users WHERE id IN ({list})"),
        ],
    )
    .await
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

/// A push subscription row as `POST /notifications/subscribe` saves one
/// (seeded directly: the route resolves the endpoint's host).
async fn seed_push_subscription(pool: &PgPool, user_id: &str, endpoint: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) \
         VALUES ($1, $2, 'p256dh', 'auth') RETURNING id",
    )
    .bind(user_id)
    .bind(endpoint)
    .fetch_one(pool)
    .await
    .expect("seed push subscription")
}

async fn subscription_exists(pool: &PgPool, endpoint: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM push_subscriptions WHERE endpoint = $1)")
        .bind(endpoint)
        .fetch_one(pool)
        .await
        .expect("read push subscription")
}

fn router(pool: PgPool) -> axum::Router {
    crate::app::Router::new()
        .merge(crate::routes::train::router())
        .nest(
            "/public",
            crate::app::Router::new()
                .merge(crate::routes::notifications::router())
                .merge(crate::routes::preferences::router()),
        )
        .with_state(crate::test_support::inert_app(pool))
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
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

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database)"]
async fn turning_off_this_device_deletes_only_the_callers_own_subscription() {
    const OWNER: &str = "TEST-NOTIFSET-UNSUB-OWNER";
    const OTHER: &str = "TEST-NOTIFSET-UNSUB-OTHER";
    const MINE: &str = "https://push.example/notifset-unsub-mine";
    const THEIRS: &str = "https://push.example/notifset-unsub-theirs";
    let pool = connect().await;
    let _cleanup = cleanup(&pool, &[OWNER, OTHER]).await;
    let token = seed_session(&pool, OWNER).await;
    seed_session(&pool, OTHER).await;
    seed_push_subscription(&pool, OWNER, MINE).await;
    seed_push_subscription(&pool, OTHER, THEIRS).await;
    let router = router(pool.clone());

    // Not logged in: 401, nothing deleted.
    let (status, _) = call(
        &router,
        "DELETE",
        "/public/notifications/subscribe",
        None,
        Some(json!({ "endpoint": MINE })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(subscription_exists(&pool, MINE).await);

    // Someone else's endpoint: 404, and theirs stays.
    let (status, body) = call(
        &router,
        "DELETE",
        "/public/notifications/subscribe",
        Some(&token),
        Some(json!({ "endpoint": THEIRS })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(subscription_exists(&pool, THEIRS).await);

    // An unknown field is refused before anything is deleted.
    let (status, body) = call(
        &router,
        "DELETE",
        "/public/notifications/subscribe",
        Some(&token),
        Some(json!({ "endpoint": MINE, "everyDevice": true })),
    )
    .await;
    // axum refuses a body serde refuses with a 422, as every route here does.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(subscription_exists(&pool, MINE).await);

    // The owner's own: 204, gone, and a second try is a 404.
    let (status, body) = call(
        &router,
        "DELETE",
        "/public/notifications/subscribe",
        Some(&token),
        Some(json!({ "endpoint": MINE })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(!subscription_exists(&pool, MINE).await);
    let (status, _) = call(
        &router,
        "DELETE",
        "/public/notifications/subscribe",
        Some(&token),
        Some(json!({ "endpoint": MINE })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(subscription_exists(&pool, THEIRS).await);
}

/// A pending (unresolved) tracked train of `user_id`'s: no `trains` row.
async fn seed_tracked_train(pool: &PgPool, user_id: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO train_subscriptions \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, \
             resolution_status) \
         VALUES ($1, '2099-06-01', 'KGX', '2099-06-01T12:00:00Z', 'EDB', 'pending') \
         RETURNING id",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("seed tracked train")
}

async fn notifications_enabled(pool: &PgPool, tracking_id: i64) -> bool {
    sqlx::query_scalar("SELECT notifications_enabled FROM train_subscriptions WHERE id = $1")
        .bind(tracking_id)
        .fetch_one(pool)
        .await
        .expect("read notifications_enabled")
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database)"]
async fn the_owner_turns_a_tracked_trains_pushes_off_and_on() {
    const OWNER: &str = "TEST-NOTIFSET-TRAIN-OWNER";
    const OTHER: &str = "TEST-NOTIFSET-TRAIN-OTHER";
    let pool = connect().await;
    let _cleanup = cleanup(&pool, &[OWNER, OTHER]).await;
    let token = seed_session(&pool, OWNER).await;
    let other_token = seed_session(&pool, OTHER).await;
    let tracking_id = seed_tracked_train(&pool, OWNER).await;
    let router = router(pool.clone());
    let uri = format!("/Train/{tracking_id}");

    // Someone else, and a train that doesn't exist: 404, nothing changes.
    let (status, body) = call(
        &router,
        "PATCH",
        &uri,
        Some(&other_token),
        Some(json!({ "notificationsEnabled": false })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(notifications_enabled(&pool, tracking_id).await);
    let (status, _) = call(
        &router,
        "PATCH",
        "/Train/999999999999",
        Some(&token),
        Some(json!({ "notificationsEnabled": false })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An unknown field is refused.
    let (status, body) = call(
        &router,
        "PATCH",
        &uri,
        Some(&token),
        Some(json!({ "notificationsEnabled": false, "customName": "x" })),
    )
    .await;
    // axum refuses a body serde refuses with a 422, as every route here does.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(notifications_enabled(&pool, tracking_id).await);

    // The owner: off, then on again; the owner's view says so.
    let (status, body) = call(
        &router,
        "PATCH",
        &uri,
        Some(&token),
        Some(json!({ "notificationsEnabled": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "notificationsEnabled": false }));
    assert!(!notifications_enabled(&pool, tracking_id).await);
    let (status, body) = call(&router, "GET", &uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["notificationsEnabled"], json!(false), "{body}");

    let (status, _) = call(
        &router,
        "PATCH",
        &uri,
        Some(&token),
        Some(json!({ "notificationsEnabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(notifications_enabled(&pool, tracking_id).await);
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database)"]
async fn notification_preferences_read_as_defaults_and_change_only_what_is_sent() {
    const USER: &str = "TEST-NOTIFSET-PREFS";
    const URI: &str = "/public/preferences/notifications";
    let pool = connect().await;
    let _cleanup = cleanup(&pool, &[USER]).await;
    let token = seed_session(&pool, USER).await;
    let router = router(pool.clone());

    let (status, _) = call(&router, "GET", URI, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // No row: every switch on, the default threshold, no quiet hours.
    let defaults = json!({
        "journeySuggestions": true,
        "lineStatus": true,
        "trainDelay": true,
        "trainCancel": true,
        "predictedStart": true,
        "stationSkip": true,
        "templateAttention": true,
        "delayThresholdMinutes": null,
        "quietHours": null,
    });
    let (status, body) = call(&router, "GET", URI, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, defaults);

    // An older client's body still works, and changes only its field.
    let (status, body) = call(
        &router,
        "PUT",
        URI,
        Some(&token),
        Some(json!({ "journeySuggestions": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["journeySuggestions"], json!(false));
    assert_eq!(body["trainDelay"], json!(true));

    let (status, body) = call(
        &router,
        "PUT",
        URI,
        Some(&token),
        Some(json!({
            "trainDelay": false,
            "delayThresholdMinutes": 10,
            "quietHours": { "start": "22:30", "end": "06:45", "timezone": "Europe/London" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, read) = call(&router, "GET", URI, Some(&token), None).await;
    assert_eq!(read, body, "the PUT answers with what a GET then reads");
    assert_eq!(read["journeySuggestions"], json!(false), "kept");
    assert_eq!(read["trainDelay"], json!(false));
    assert_eq!(read["delayThresholdMinutes"], json!(10));
    assert_eq!(
        read["quietHours"],
        json!({ "start": "22:30", "end": "06:45", "timezone": "Europe/London" })
    );

    // Refusals change nothing.
    for (bad, expected) in [
        (
            json!({ "delayThresholdMinutes": 20 }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "quietHours": { "start": "22:00", "end": "22:00", "timezone": "UTC" } }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "quietHours": { "start": "22:00", "end": "07:00", "timezone": "Nowhere/Here" } }),
            StatusCode::BAD_REQUEST,
        ),
        (json!({ "allOff": true }), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let (status, body) = call(&router, "PUT", URI, Some(&token), Some(bad.clone())).await;
        assert_eq!(status, expected, "{bad}: {body}");
    }
    let (_, after) = call(&router, "GET", URI, Some(&token), None).await;
    assert_eq!(after, read);

    // null goes back to the default threshold and no quiet hours.
    let (status, body) = call(
        &router,
        "PUT",
        URI,
        Some(&token),
        Some(json!({ "delayThresholdMinutes": null, "quietHours": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["delayThresholdMinutes"], Value::Null);
    assert_eq!(body["quietHours"], Value::Null);
    assert_eq!(body["trainDelay"], json!(false), "kept");
}
