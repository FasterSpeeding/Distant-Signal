//! `push_subscriptions`: one row per browser/device a user has granted
//! push permission on. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decision 5 and Open question 5 (endpoint re-ownership).

use anyhow::Result;
use sqlx::PgPool;

/// `ON CONFLICT (endpoint)`, not `(user_id, endpoint)`: the Push API's
/// `endpoint` is already a globally unique per-device-registration URL, so
/// this is also how a shared device correctly re-points its one endpoint
/// row at whichever user is currently logged in and re-subscribes (Open
/// question 5).
pub async fn upsert_push_subscription(
    pool: &PgPool,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, NOW(), NOW()) \
         ON CONFLICT (endpoint) DO UPDATE SET \
           user_id = EXCLUDED.user_id, p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth, last_seen_at = NOW()",
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query("INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .bind(format!("{user_id}@example.com"))
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed fixture user");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM push_subscriptions WHERE user_id = $1").bind(user_id).execute(pool).await.expect("cleanup fixture subscriptions");
        sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(pool).await.expect("cleanup fixture user");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_creates_then_reassigns_ownership_on_conflict -- --ignored`"]
    async fn upsert_creates_then_reassigns_ownership_on_conflict() {
        let pool = connect().await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-A").await;
        seed_user(&pool, "TEST-NOTIF-SUB-USER-B").await;

        upsert_push_subscription(&pool, "TEST-NOTIF-SUB-USER-A", "https://push.example/ep1", "p256dh-a", "auth-a")
            .await
            .expect("first insert");
        let owner: String = sqlx::query_scalar("SELECT user_id FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(owner, "TEST-NOTIF-SUB-USER-A");

        // Same endpoint, different user -- re-subscription on a shared device.
        upsert_push_subscription(&pool, "TEST-NOTIF-SUB-USER-B", "https://push.example/ep1", "p256dh-b", "auth-b")
            .await
            .expect("second insert (conflict path)");
        let owner: String = sqlx::query_scalar("SELECT user_id FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("read back after conflict");
        assert_eq!(owner, "TEST-NOTIF-SUB-USER-B", "re-subscribing must re-own the row, not create a duplicate");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "ON CONFLICT must update in place, not insert a second row");

        cleanup_user(&pool, "TEST-NOTIF-SUB-USER-A").await;
        cleanup_user(&pool, "TEST-NOTIF-SUB-USER-B").await;
    }
}
