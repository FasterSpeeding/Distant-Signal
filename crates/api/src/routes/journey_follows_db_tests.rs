//! Following a shared journey (DS-146) through the web app's routes,
//! against a live database: the follow and unfollow routes, followed
//! journeys in `GET /Journeys/mine?include=followed` (in travel order, with
//! `followedFrom`), the follow state on the journey page and group rows,
//! and that a follower gets no write access.
//!
//! Fixtures come from `data::journey_follows::db_tests`: users, a group and
//! journeys under one id prefix per test, every cleanup keyed on it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::data::journey_follows::db_tests::{cleanup, exec, seed_journey, seed_shared_journey};

async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to postgres")
}

fn router(pool: PgPool) -> axum::Router {
    crate::app::Router::new()
        .merge(super::journeys::router())
        .merge(super::groups::router())
        .with_state(crate::test_support::inert_app(pool))
}

/// A session for an existing fixture user; returns the raw cookie value.
async fn session(pool: &PgPool, user: &str) -> String {
    let raw = format!("raw-follow-{user}");
    sqlx::query(
        "INSERT INTO sessions (id, user_id, expires_at) VALUES ($1, $2, now() + INTERVAL '1 day') \
         ON CONFLICT (id) DO UPDATE SET user_id = EXCLUDED.user_id, expires_at = EXCLUDED.expires_at",
    )
    .bind(hash_session_token(&raw))
    .bind(user)
    .execute(pool)
    .await
    .expect("seed session");
    raw
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn ids(list: &Value) -> Vec<i64> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

async fn leg_count(pool: &PgPool, journey: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
        .bind(journey)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
async fn a_followed_journey_lists_with_the_followers_own_in_travel_order() {
    let pool = connect().await;
    let prefix = "jfr-list";
    // The owner's journey, shared into the group, two hours from now; the
    // owner's other journey, not shared; the follower's own, in five hours
    // and in thirty, and one in the past.
    let shared = seed_shared_journey(&pool, prefix, 2).await;
    let owner = format!("{prefix}-owner");
    let friend = format!("{prefix}-friend");
    let private = seed_journey(&pool, &owner, 3).await;
    let soon = seed_journey(&pool, &friend, 5).await;
    let later = seed_journey(&pool, &friend, 30).await;
    let past = seed_journey(&pool, &friend, -30).await;
    let router = router(pool.clone());
    let cookie = session(&pool, &friend).await;

    // Not followed yet: the group row offers it, the list leaves it out.
    let (status, rows) = send(
        &router,
        "GET",
        &format!("/groups/{prefix}-g/journeys"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rows[0]["journeyId"], shared);
    assert_eq!(rows[0]["following"], false);
    let (_, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{shared}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(detail["canFollow"], true);
    assert!(detail.get("followedFrom").is_none());

    let (status, follow) = send(
        &router,
        "POST",
        &format!("/Journeys/{shared}/follow"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{follow}");
    assert_eq!(follow["via"], "group");
    assert_eq!(follow["ownerName"], "Olive Owner");
    assert_eq!(follow["groupName"], "Family");

    // Without `include`, the list is the caller's own, as before.
    let (_, own) = send(&router, "GET", "/Journeys/mine", &cookie, None).await;
    assert_eq!(ids(&own), vec![soon, later, past]);

    // With it, the followed journey takes its place in travel order, and
    // the owner's unshared journey stays out.
    let (status, all) = send(
        &router,
        "GET",
        "/Journeys/mine?include=followed",
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&all), vec![shared, soon, later, past]);
    assert!(!ids(&all).contains(&private));
    let row = &all[0];
    assert_eq!(row["followedFrom"]["via"], "group");
    assert_eq!(row["followedFrom"]["ownerName"], "Olive Owner");
    assert_eq!(row["travelled"], false);
    // The owner's bookkeeping is not the follower's.
    assert!(row.get("createdVia").is_none());
    assert_eq!(row["trainSubscriptionId"], Value::Null);
    assert!(all[1].get("followedFrom").is_none());
    assert_eq!(all[1]["createdVia"], "web");

    // The journey page and the group row say so.
    let (_, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{shared}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(detail["isOwner"], false);
    assert_eq!(detail["followedFrom"]["via"], "group");
    assert!(detail.get("canFollow").is_none());
    assert_eq!(detail["shareLink"], Value::Null);
    let (_, rows) = send(
        &router,
        "GET",
        &format!("/groups/{prefix}-g/journeys"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(rows[0]["following"], true);
    let (_, rows) = send(&router, "GET", "/groups/shared-journeys", &cookie, None).await;
    assert_eq!(rows[0]["following"], true);

    let (status, _) = send(
        &router,
        "GET",
        "/Journeys/mine?include=everything",
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Unfollowed: gone from the list, offered again.
    let (status, _) = send(
        &router,
        "DELETE",
        &format!("/Journeys/{shared}/follow"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &router,
        "DELETE",
        &format!("/Journeys/{shared}/follow"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, all) = send(
        &router,
        "GET",
        "/Journeys/mine?include=followed",
        &cookie,
        None,
    )
    .await;
    assert_eq!(ids(&all), vec![soon, later, past]);

    // Unshared after following: gone from the list and the page.
    send(
        &router,
        "POST",
        &format!("/Journeys/{shared}/follow"),
        &cookie,
        None,
    )
    .await;
    exec(
        &pool,
        "DELETE FROM group_journeys WHERE group_id = $1",
        &[&format!("{prefix}-g")],
    )
    .await;
    let (_, all) = send(
        &router,
        "GET",
        "/Journeys/mine?include=followed",
        &cookie,
        None,
    )
    .await;
    assert_eq!(ids(&all), vec![soon, later, past]);
    let (status, _) = send(
        &router,
        "GET",
        &format!("/Journeys/{shared}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
async fn a_share_link_visitor_follows_with_the_token_until_it_is_revoked() {
    let pool = connect().await;
    let prefix = "jfr-link";
    let journey = seed_shared_journey(&pool, prefix, 2).await;
    let owner = format!("{prefix}-owner");
    let reader = format!("{prefix}-reader");
    exec(
        &pool,
        "INSERT INTO users (id, name) VALUES ($1, 'Rita')",
        &[&reader],
    )
    .await;
    let router = router(pool.clone());
    let owner_cookie = session(&pool, &owner).await;
    let cookie = session(&pool, &reader).await;

    let (status, link) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/share-link"),
        &owner_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = link["token"].as_str().unwrap().to_string();

    // Not in the group: no group route in, and the page is not theirs.
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &cookie,
        Some(json!({"shareToken": "not-the-token"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &cookie,
        Some(json!({"shareToken": token, "groupId": format!("{prefix}-g")})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, follow) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &cookie,
        Some(json!({"shareToken": token})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{follow}");
    // A link shows nothing about its owner.
    assert_eq!(follow["via"], "link");
    assert_eq!(follow["ownerName"], Value::Null);
    let (status, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["followedFrom"]["via"], "link");
    let (_, all) = send(
        &router,
        "GET",
        "/Journeys/mine?include=followed",
        &cookie,
        None,
    )
    .await;
    assert_eq!(ids(&all), vec![journey]);

    // The owner can't follow their own journey.
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &owner_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Regenerating revokes the old link: the follow through it ends.
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/share-link"),
        &owner_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, all) = send(
        &router,
        "GET",
        "/Journeys/mine?include=followed",
        &cookie,
        None,
    )
    .await;
    assert_eq!(ids(&all), Vec::<i64>::new());
    let follows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM journey_follows WHERE journey_id = $1")
            .bind(journey)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(follows, 0);
    let (status, _) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    cleanup(&pool, prefix).await;
}

/// A follower reads the journey and nothing more: every write the owner
/// has is refused, and the journey is unchanged.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_follows -- --ignored --test-threads=1`"]
async fn a_follower_cannot_edit_replace_or_change_trains() {
    let pool = connect().await;
    let prefix = "jfr-readonly";
    let journey = seed_shared_journey(&pool, prefix, 2).await;
    let friend = format!("{prefix}-friend");
    let router = router(pool.clone());
    let cookie = session(&pool, &friend).await;
    let (status, _) = send(
        &router,
        "POST",
        &format!("/Journeys/{journey}/follow"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let leg: i64 = sqlx::query_scalar("SELECT id FROM journey_legs WHERE journey_id = $1")
        .bind(journey)
        .fetch_one(&pool)
        .await
        .unwrap();
    let date = (crate::routes::london_today() + chrono::Duration::days(1)).to_string();
    let untracked = json!({
        "mode": "untracked",
        "transitMode": "bus",
        "plannedDeparture": format!("{date}T10:00:00Z"),
        "plannedArrival": format!("{date}T11:00:00Z"),
    });

    for (method, uri, body) in [
        ("DELETE", format!("/Journeys/{journey}"), None),
        (
            "POST",
            format!("/Journeys/{journey}/legs"),
            Some(untracked.clone()),
        ),
        (
            "PUT",
            format!("/Journeys/{journey}/legs/{leg}"),
            Some(untracked.clone()),
        ),
        ("DELETE", format!("/Journeys/{journey}/legs/{leg}"), None),
        (
            "POST",
            format!("/Journeys/{journey}/legs/{leg}/train"),
            Some(json!({"trainUid": "Z00001", "serviceDate": date})),
        ),
        (
            "POST",
            format!("/Journeys/{journey}/legs/{leg}/replace"),
            Some(json!({"cause": "choice", "legs": [{
                "kind": "untracked",
                "transitMode": "bus",
                "plannedDeparture": format!("{date}T10:00:00Z"),
                "plannedArrival": format!("{date}T11:00:00Z"),
            }]})),
        ),
        (
            "GET",
            format!("/Journeys/{journey}/legs/{leg}/alternatives"),
            None,
        ),
        ("POST", format!("/Journeys/{journey}/share-link"), None),
        ("DELETE", format!("/Journeys/{journey}/share-link"), None),
        ("GET", format!("/Journeys/{journey}/revisions"), None),
    ] {
        let (status, body) = send(&router, method, &uri, &cookie, body).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST,
            "{method} {uri} answered {status}: {body}"
        );
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journeys WHERE id = $1)")
        .bind(journey)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(exists);
    assert_eq!(leg_count(&pool, journey).await, 1);
    let links: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM unlisted_links WHERE resource_type = 'journey' AND resource_id = $1",
    )
    .bind(journey.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(links, 0);

    // The page still reads, as the owner's view without owner extras.
    let (status, detail) = send(
        &router,
        "GET",
        &format!("/Journeys/{journey}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["isOwner"], false);
    assert!(detail.get("createdVia").is_none());
    assert!(detail.get("suggestionsMuted").is_none());
    cleanup(&pool, prefix).await;
}
