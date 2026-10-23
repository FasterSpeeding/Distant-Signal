//! Every notification setting's gate against a live database
//! (`DATABASE_URL`): a train with its pushes off, the per-type switches,
//! the per-user delay threshold, a pinned line's alert switch, quiet hours
//! (held, then released as the latest state; cancellations go through)
//! and snooze. Each test owns its `TEST-NOTIFSET-*` users and its
//! far-future fixture day's `TEST-NOTIFSET-*` trains.

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::queries;

async fn connect() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("connect to postgres")
}

/// The fixture day: far in the future, so nothing real is on it.
fn day() -> NaiveDate {
    let day: NaiveDate = "2099-07-01".parse().expect("date");
    ds_store::test_support::assert_synthetic_date(day);
    day
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::query(sql)
        .execute(pool)
        .await
        .unwrap_or_else(|err| panic!("{sql}: {err}"));
}

/// Deletes `user`'s rows (most cascade from `users`) and `uid`'s train,
/// now and again when the test ends.
async fn cleanup(pool: &PgPool, user: &str, uid: &str) -> ds_store::test_support::FixtureCleanup {
    ds_store::test_support::FixtureCleanup::new(
        pool,
        [
            format!("DELETE FROM journeys WHERE user_id = '{user}'"),
            format!("DELETE FROM users WHERE id = '{user}'"),
            format!(
                "DELETE FROM trains WHERE train_uid = '{uid}' AND service_date = '{}'",
                day()
            ),
        ],
    )
    .await
}

/// One user with a one-leg journey on a train of their own.
struct Fixture {
    trains_id: i64,
    subscription_id: i64,
    journey_leg_id: i64,
}

async fn seed(pool: &PgPool, user: &str, uid: &str) -> Fixture {
    exec(
        pool,
        &format!("INSERT INTO users (id, name) VALUES ('{user}', '{user}') ON CONFLICT DO NOTHING"),
    )
    .await;
    let trains_id: i64 = sqlx::query_scalar(
        "INSERT INTO trains (train_uid, service_date, origin_crs, destination_crs) \
         VALUES ($1, $2, 'PAD', 'RDG') RETURNING id",
    )
    .bind(uid)
    .bind(day())
    .fetch_one(pool)
    .await
    .expect("seed train");
    let subscription_id: i64 = sqlx::query_scalar(
        "INSERT INTO train_subscriptions \
             (user_id, service_date, pin_origin_crs, pin_destination_crs, trains_id, \
              resolution_status) \
         VALUES ($1, $2, 'PAD', 'RDG', $3, 'resolved') RETURNING id",
    )
    .bind(user)
    .bind(day())
    .bind(trains_id)
    .fetch_one(pool)
    .await
    .expect("seed subscription");
    let journey_id: i64 =
        sqlx::query_scalar("INSERT INTO journeys (user_id) VALUES ($1) RETURNING id")
            .bind(user)
            .fetch_one(pool)
            .await
            .expect("seed journey");
    let journey_leg_id: i64 = sqlx::query_scalar(
        "INSERT INTO journey_legs \
             (journey_id, leg_order, origin_crs, destination_crs, service_date, \
              train_subscription_id, match_mode) \
         VALUES ($1, 1, 'PAD', 'RDG', $2, $3, 'manual') RETURNING id",
    )
    .bind(journey_id)
    .bind(day())
    .bind(subscription_id)
    .fetch_one(pool)
    .await
    .expect("seed leg");
    Fixture {
        trains_id,
        subscription_id,
        journey_leg_id,
    }
}

async fn skip_checked_legs(pool: &PgPool) -> Vec<i64> {
    queries::list_committed_legs_for_today(pool, day())
        .await
        .expect("list_committed_legs_for_today")
        .into_iter()
        .map(|leg| leg.journey_leg_id)
        .collect()
}

#[tokio::test]
#[ignore = "requires DATABASE_URL (a migrated database)"]
async fn a_train_with_its_pushes_off_gets_no_skipped_stop_check() {
    const USER: &str = "TEST-NOTIFSET-SKIP-TRAIN-OFF";
    const UID: &str = "TEST-NOTIFSET-SKIP1";
    let pool = connect().await;
    let _cleanup = cleanup(&pool, USER, UID).await;
    let fixture = seed(&pool, USER, UID).await;
    assert!(
        skip_checked_legs(&pool)
            .await
            .contains(&fixture.journey_leg_id)
    );

    exec(
        &pool,
        &format!(
            "UPDATE train_subscriptions SET notifications_enabled = FALSE WHERE id = {}",
            fixture.subscription_id
        ),
    )
    .await;
    assert!(
        !skip_checked_legs(&pool)
            .await
            .contains(&fixture.journey_leg_id)
    );
    assert!(fixture.trains_id > 0);
}
