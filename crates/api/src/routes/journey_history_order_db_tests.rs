//! Journey history order (DS-178), against a live database: `GET
//! /Journeys/mine?include=followed,shared` lists the caller's own journeys
//! and the ones shared with them in ONE list, still to come soonest first
//! then travelled newest first (by the first leg's departure, newest id on
//! a tie), under one cap; a group's journeys and the journeys shared with
//! the caller come in the same order; and merging lets no journey in that
//! the caller could not already read.
//!
//! Fixtures come from `data::journey_follows::db_tests`: users, a group and
//! journeys under one id prefix per test, every cleanup keyed on it.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::hash_session_token;
use crate::data::journey_follows::db_tests::{cleanup, exec, seed_journey, seed_shared_journey};
use crate::data::journeys::{ListInclude, journey_readable_by, list_journeys_for_user_capped};

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
    let raw = format!("raw-history-{user}");
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

async fn get(router: &axum::Router, uri: &str, cookie: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, format!("distant_signal_session={cookie}"))
        .body(Body::empty())
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

fn ids(list: &Value, key: &str) -> Vec<i64> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|row| row[key].as_i64().unwrap())
        .collect()
}

/// Shares `journey` into `group`, as `added_by`.
async fn share(pool: &PgPool, group: &str, journey: i64, added_by: &str) {
    exec(
        pool,
        &format!(
            "INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, {journey}, $2)"
        ),
        &[group, added_by],
    )
    .await;
}

/// A journey of `user_id`'s with one untracked leg leaving at exactly
/// `departure` (an RFC 3339 instant), for ties.
async fn seed_journey_at(pool: &PgPool, user_id: &str, departure: &str) -> i64 {
    let journey_id: i64 =
        sqlx::query_scalar("INSERT INTO journeys (user_id) VALUES ($1) RETURNING id")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("seed journey");
    sqlx::query(
        "INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, \
                                   transit_mode, planned_departure, planned_arrival) \
         VALUES ($1, 1, ($2::timestamptz AT TIME ZONE 'Europe/London')::date, 'manual', \
                 'untracked', 'bus', $2::timestamptz, $2::timestamptz + INTERVAL '1 hour')",
    )
    .bind(journey_id)
    .bind(departure)
    .execute(pool)
    .await
    .expect("seed leg");
    journey_id
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_history -- --ignored --test-threads=1`"]
async fn shared_journeys_sit_inline_with_the_callers_own_newest_first() {
    let pool = connect().await;
    let prefix = "jho-merge";
    // `{prefix}-owner` shares two journeys into "Family" (with the friend):
    // one two hours ahead, one twenty hours ago. Their third journey is
    // not shared. The friend's own: five hours ahead, ten and thirty hours
    // ago.
    let shared_next = seed_shared_journey(&pool, prefix, 2).await;
    let owner = format!("{prefix}-owner");
    let friend = format!("{prefix}-friend");
    let group = format!("{prefix}-g");
    let shared_past = seed_journey(&pool, &owner, -20).await;
    share(&pool, &group, shared_past, &owner).await;
    let private = seed_journey(&pool, &owner, -5).await;
    let own_next = seed_journey(&pool, &friend, 5).await;
    let own_recent = seed_journey(&pool, &friend, -10).await;
    let own_old = seed_journey(&pool, &friend, -30).await;
    // A stranger's group the friend is not in, with a journey shared into
    // it that would sort first among the past ones.
    let stranger = format!("{prefix}-stranger");
    let strangers_group = format!("{prefix}-g2");
    exec(&pool, "INSERT INTO users (id) VALUES ($1)", &[&stranger]).await;
    exec(
        &pool,
        "INSERT INTO groups (id, name, created_by) VALUES ($1, 'Strangers', $2)",
        &[&strangers_group, &stranger],
    )
    .await;
    exec(
        &pool,
        "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner'), ($1, $3, 'member')",
        &[&strangers_group, &stranger, &owner],
    )
    .await;
    let strangers = seed_journey(&pool, &stranger, -1).await;
    share(&pool, &strangers_group, strangers, &stranger).await;
    // The owner shares the private one there too: the friend still can't
    // see it, they aren't in that group.
    share(&pool, &strangers_group, private, &owner).await;
    // The friend shares one of their own into Family: still their own row.
    share(&pool, &group, own_recent, &friend).await;

    let router = router(pool.clone());
    let cookie = session(&pool, &friend).await;

    let (status, merged) = get(&router, "/Journeys/mine?include=followed,shared", &cookie).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    // One list: upcoming soonest first, then past newest first, own and
    // shared interleaved by the same key.
    assert_eq!(
        ids(&merged, "id"),
        vec![shared_next, own_next, own_recent, shared_past, own_old]
    );
    let travelled: Vec<bool> = merged
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["travelled"].as_bool().unwrap())
        .collect();
    assert_eq!(travelled, vec![false, false, true, true, true]);
    // `departsAt` is the sort key: ascending while upcoming, descending once
    // travelled.
    let departs: Vec<String> = merged
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["departsAt"].as_str().expect("departsAt").to_string())
        .collect();
    assert!(departs[0] < departs[1]);
    assert!(departs[2] > departs[3] && departs[3] > departs[4]);

    // The shared rows say where they came from and carry none of the
    // owner's bookkeeping; the friend's own (even one they shared) don't.
    let row = |id: i64| {
        merged
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
            .clone()
    };
    for shared in [shared_next, shared_past] {
        let row = row(shared);
        let via = row["sharedVia"].as_array().expect("sharedVia");
        assert_eq!(via.len(), 1);
        assert_eq!(via[0]["groupId"], group.as_str());
        assert_eq!(via[0]["groupName"], "Family");
        assert_eq!(via[0]["addedByName"], "Olive Owner");
        assert!(row.get("createdVia").is_none());
        assert_eq!(row["trainSubscriptionId"], Value::Null);
        assert!(row.get("followedFrom").is_none());
    }
    for own in [own_next, own_recent, own_old] {
        let row = row(own);
        assert!(row.get("sharedVia").is_none(), "{row}");
        assert_eq!(row["createdVia"], "web");
    }

    // No journey the friend could not already read: every row passes the
    // journey page's own gate, the shared ones are exactly the group
    // endpoint's, and the stranger's and the private journey stay out.
    for id in ids(&merged, "id") {
        assert!(
            journey_readable_by(&pool, id, &friend).await.unwrap(),
            "{id}"
        );
    }
    let (_, shared_list) = get(&router, "/groups/shared-journeys", &cookie).await;
    let mut from_groups = ids(&shared_list, "journeyId");
    from_groups.sort_unstable();
    from_groups.dedup();
    let mut merged_shared: Vec<i64> = merged
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row.get("sharedVia").is_some())
        .map(|row| row["id"].as_i64().unwrap())
        .collect();
    merged_shared.sort_unstable();
    assert_eq!(merged_shared, from_groups);
    assert!(!ids(&merged, "id").contains(&private));
    assert!(!ids(&merged, "id").contains(&strangers));
    // The group endpoint lists them in the same order too.
    assert_eq!(
        ids(&shared_list, "journeyId"),
        vec![shared_next, shared_past]
    );

    // `shared` alone: the same without followed ones (there are none);
    // without `include`, own only, as before.
    let (_, shared_only) = get(&router, "/Journeys/mine?include=shared", &cookie).await;
    assert_eq!(ids(&shared_only, "id"), ids(&merged, "id"));
    let (_, own) = get(&router, "/Journeys/mine", &cookie).await;
    assert_eq!(ids(&own, "id"), vec![own_next, own_recent, own_old]);
    let (_, followed) = get(&router, "/Journeys/mine?include=followed", &cookie).await;
    assert_eq!(ids(&followed, "id"), vec![own_next, own_recent, own_old]);
    for bad in ["shared,everything", "everything", ",", "shared,"] {
        let (status, _) = get(&router, &format!("/Journeys/mine?include={bad}"), &cookie).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // The owner sees their own journeys only once, never as shared.
    let owner_cookie = session(&pool, &owner).await;
    let (_, owners) = get(
        &router,
        "/Journeys/mine?include=followed,shared",
        &owner_cookie,
    )
    .await;
    let owners_ids = ids(&owners, "id");
    assert!(owners_ids.contains(&private));
    // The stranger's journey reaches the owner through the Strangers group.
    assert!(owners_ids.contains(&strangers));
    // And the friend's journey shared into Family.
    assert!(owners_ids.contains(&own_recent));
    let mut deduped = owners_ids.clone();
    deduped.sort_unstable();
    deduped.dedup();
    assert_eq!(deduped.len(), owners_ids.len());
    for row in owners.as_array().unwrap() {
        let id = row["id"].as_i64().unwrap();
        let is_own = [shared_next, shared_past, private].contains(&id);
        assert_eq!(row.get("sharedVia").is_none(), is_own, "{row}");
    }

    // Leaving the group takes its journeys out of the list.
    exec(
        &pool,
        "DELETE FROM group_members WHERE group_id = $1 AND user_id = $2",
        &[&group, &friend],
    )
    .await;
    let (_, after) = get(&router, "/Journeys/mine?include=followed,shared", &cookie).await;
    assert_eq!(ids(&after, "id"), vec![own_next, own_recent, own_old]);
    cleanup(&pool, prefix).await;
}

#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_history -- --ignored --test-threads=1`"]
async fn a_followed_and_shared_journey_lists_once_with_both_labels() {
    let pool = connect().await;
    let prefix = "jho-both";
    let journey = seed_shared_journey(&pool, prefix, -3).await;
    let friend = format!("{prefix}-friend");
    let router = router(pool.clone());
    let cookie = session(&pool, &friend).await;
    crate::data::journey_follows::follow_journey(
        &pool,
        &friend,
        journey,
        crate::data::journey_follows::FollowThrough::Group(None),
    )
    .await
    .expect("follow");

    let (_, merged) = get(&router, "/Journeys/mine?include=followed,shared", &cookie).await;
    assert_eq!(ids(&merged, "id"), vec![journey]);
    assert_eq!(merged[0]["followedFrom"]["via"], "group");
    assert_eq!(merged[0]["sharedVia"][0]["groupName"], "Family");
    let (_, followed) = get(&router, "/Journeys/mine?include=followed", &cookie).await;
    assert_eq!(ids(&followed, "id"), vec![journey]);
    assert!(followed[0].get("sharedVia").is_none());
    cleanup(&pool, prefix).await;
}

/// Ties on the departure go to the newest journey id, whoever owns it, and
/// the cap applies to the merged list: the journeys kept are the newest
/// across own and shared alike, not the newest of each. (Two lists capped
/// apart and merged by the client would keep the wrong ones and break the
/// order at the cap's edge, as a client paging two lists would.)
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_history -- --ignored --test-threads=1`"]
async fn ties_break_by_id_and_one_cap_covers_own_and_shared() {
    let pool = connect().await;
    let prefix = "jho-cap";
    let first_shared = seed_shared_journey(&pool, prefix, -48).await;
    let owner = format!("{prefix}-owner");
    let friend = format!("{prefix}-friend");
    let group = format!("{prefix}-g");
    // Past journeys, alternating own and shared, an hour apart; then two
    // pairs on exactly the same departure, own and shared each way round.
    let own_1 = seed_journey_at(&pool, &friend, "2020-03-01T09:00:00Z").await;
    let shared_2 = seed_journey_at(&pool, &owner, "2020-03-01T08:00:00Z").await;
    let own_3 = seed_journey_at(&pool, &friend, "2020-03-01T07:00:00Z").await;
    let shared_4 = seed_journey_at(&pool, &owner, "2020-03-01T06:00:00Z").await;
    let tie_own_older = seed_journey_at(&pool, &friend, "2020-02-01T10:00:00Z").await;
    let tie_shared_newer = seed_journey_at(&pool, &owner, "2020-02-01T10:00:00Z").await;
    let tie_shared_older = seed_journey_at(&pool, &owner, "2020-01-01T10:00:00Z").await;
    let tie_own_newer = seed_journey_at(&pool, &friend, "2020-01-01T10:00:00Z").await;
    for journey in [shared_2, shared_4, tie_shared_newer, tie_shared_older] {
        share(&pool, &group, journey, &owner).await;
    }

    let list = |limit: i64| {
        let pool = pool.clone();
        let friend = friend.clone();
        async move {
            list_journeys_for_user_capped(&pool, &friend, ListInclude::ALL, limit)
                .await
                .expect("list")
                .iter()
                .map(|row| row.id)
                .collect::<Vec<i64>>()
        }
    };
    let everything = list(100).await;
    assert_eq!(
        everything,
        vec![
            first_shared,
            own_1,
            shared_2,
            own_3,
            shared_4,
            tie_shared_newer,
            tie_own_older,
            tie_own_newer,
            tie_shared_older,
        ]
    );
    // Each cap keeps a prefix of the full order: like successive pages,
    // never a reshuffle at the edge.
    for limit in 1..=everything.len() {
        let capped = list(i64::try_from(limit).unwrap()).await;
        assert_eq!(capped, everything[..limit].to_vec(), "cap {limit}");
    }
    // Stable from one read to the next.
    assert_eq!(list(100).await, everything);
    cleanup(&pool, prefix).await;
}

/// A group's own journey list and the journeys shared with the caller come
/// in the same travel order (they were newest share first, the group page
/// oldest share first).
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_history -- --ignored --test-threads=1`"]
async fn group_journey_lists_come_in_travel_order() {
    let pool = connect().await;
    let prefix = "jho-group";
    // Shared in this order: an old past one, a future one, a recent past
    // one, the friend's own.
    let old = seed_shared_journey(&pool, prefix, -40).await;
    let owner = format!("{prefix}-owner");
    let friend = format!("{prefix}-friend");
    let group = format!("{prefix}-g");
    let future = seed_journey(&pool, &owner, 6).await;
    let recent = seed_journey(&pool, &owner, -4).await;
    let friends = seed_journey(&pool, &friend, -8).await;
    for (journey, by) in [(future, &owner), (recent, &owner), (friends, &friend)] {
        share(&pool, &group, journey, by).await;
    }
    let router = router(pool.clone());
    let cookie = session(&pool, &friend).await;

    let (status, rows) = get(&router, &format!("/groups/{group}/journeys"), &cookie).await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    assert_eq!(ids(&rows, "journeyId"), vec![future, recent, friends, old]);
    let (_, shared) = get(&router, "/groups/shared-journeys", &cookie).await;
    assert_eq!(ids(&shared, "journeyId"), vec![future, recent, old]);
    cleanup(&pool, prefix).await;
}

/// A shared row carries the train its owner picked (DS-199): the owner's
/// subscription id is blanked, but the train's resolution and the
/// journey's counts are the journey's own, so the reader sees the train,
/// not a leg still needing one. The frontend reads "has a train" from
/// `resolutionStatus`, never from the blanked id.
#[tokio::test]
#[ignore = "requires a live database; run with `cargo test -p api journey_history -- --ignored --test-threads=1`"]
async fn a_shared_row_keeps_the_owners_picked_train() {
    let pool = connect().await;
    let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
    let prefix = "jho-train";
    seed_shared_journey(&pool, prefix, 2).await;
    let owner = format!("{prefix}-owner");
    let friend = format!("{prefix}-friend");
    let group = format!("{prefix}-g");
    exec(
        &pool,
        "INSERT INTO trains (train_uid, service_date) VALUES ('TEST-TRAIN-JHO1', '2026-09-22')",
        &[],
    )
    .await;
    let journey: i64 = sqlx::query_scalar(
        "WITH j AS (INSERT INTO journeys (user_id) VALUES ($1) RETURNING id), \
              sub AS ( \
                  INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, \
                                                   pin_destination_crs, resolution_status, trains_id) \
                  SELECT $1, '2026-09-22', 'KGX', 'YRK', 'resolved', t.id FROM trains t \
                  WHERE t.train_uid = 'TEST-TRAIN-JHO1' AND t.service_date = '2026-09-22' \
                  RETURNING id \
              ), \
              leg AS ( \
                  INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, \
                                            service_date, train_subscription_id, match_mode) \
                  SELECT j.id, 1, 'KGX', 'YRK', '2026-09-22', sub.id, 'manual' FROM j, sub \
              ) \
         SELECT id FROM j",
    )
    .bind(&owner)
    .fetch_one(&pool)
    .await
    .expect("seed the train journey");
    share(&pool, &group, journey, &owner).await;

    let router = router(pool.clone());
    let (status, rows) = get(
        &router,
        "/Journeys/mine?include=shared",
        &session(&pool, &friend).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rows}");
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == journey)
        .expect("the shared journey")
        .clone();
    assert!(row.get("sharedVia").is_some(), "{row}");
    assert_eq!(row["trainSubscriptionId"], Value::Null);
    assert_eq!(row["resolutionStatus"], "resolved", "{row}");
    assert_eq!(row["trainLegCount"], 1, "{row}");
    assert_eq!(row["unmatchedLegCount"], 0, "{row}");
    cleanup(&pool, prefix).await;
}
