//! Queries for `users`/`sessions`/`oidc_login_state` -- the tables Task
//! 1's migration creates. See
//! docs/superpowers/specs/2026-08-28-user-accounts-sso-design.md's Data
//! model section.

use anyhow::Result;
use sqlx::PgPool;

use crate::auth::oidc::OidcIdentity;

/// Only ever `Some` when the ID token asserted `email_verified: true` --
/// the actual enforcement point for design doc Open Question 2 (see
/// `crates/api/src/auth/oidc.rs`'s `identity_from_claims` doc comment,
/// which maps the claim through unfiltered; this is where it's filtered).
fn verified_email(identity: &OidcIdentity) -> Option<&str> {
    identity
        .email_verified
        .then_some(identity.email.as_deref())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(email_verified: bool) -> OidcIdentity {
        OidcIdentity {
            sub: "user-123".to_string(),
            email: Some("rider@example.com".to_string()),
            email_verified,
            name: Some("Ada Rider".to_string()),
        }
    }

    #[test]
    fn verified_email_is_kept() {
        assert_eq!(verified_email(&identity(true)), Some("rider@example.com"));
    }

    #[test]
    fn unverified_email_is_dropped() {
        assert_eq!(verified_email(&identity(false)), None);
    }

    #[test]
    fn no_email_claim_at_all_is_none_regardless_of_verified_flag() {
        let mut i = identity(true);
        i.email = None;
        assert_eq!(verified_email(&i), None);
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Creates the user on first login, or updates `email`/`name`/
/// `last_login_at` on every return visit -- design doc: "upserted, not
/// just inserted once."
pub async fn upsert_user(pool: &PgPool, identity: &OidcIdentity) -> Result<User> {
    let email = verified_email(identity);
    let row = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, name, created_at, last_login_at) \
         VALUES ($1, $2, $3, NOW(), NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
            email = EXCLUDED.email, name = EXCLUDED.name, last_login_at = NOW() \
         RETURNING id, email, name",
    )
    .bind(&identity.sub)
    .bind(email)
    .bind(&identity.name)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// `sessions.refresh_token` is deliberately left NULL. The design doc's
/// only use for it is silent ID-token renewal before local session expiry
/// ("Expiry and refresh"), and nothing in this plan implements that -- the
/// column is written by no other path and read by none at all. Storing a
/// live IdP credential server-side with zero present consumer is pure
/// added blast radius on a database leak, and the design doc's own Open
/// Question 5 already flags that this schema would hold it in plaintext
/// with no column-encryption precedent anywhere in the repo.
///
/// The column itself stays in the schema, unused, for whoever implements
/// refresh: it is nullable, so nothing needs migrating when that lands,
/// and this is the only INSERT that would have to start binding it.
pub async fn insert_session(
    pool: &PgPool,
    hashed_token: &str,
    user_id: &str,
    ttl_days: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, refresh_token, created_at, expires_at) \
         VALUES ($1, $2, NULL, NOW(), NOW() + make_interval(days => $3))",
    )
    .bind(hashed_token)
    .bind(user_id)
    .bind(ttl_days as i32)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Looks up a session by its *hashed* token and joins the owning user, but
/// only if it hasn't expired -- an expired row reads back identically to
/// no row at all. Expired rows are never explicitly pruned by a
/// background job in this plan (a small table; left as a documented
/// follow-up, same posture as not implementing RP-initiated logout).
pub async fn get_session_with_user(
    pool: &PgPool,
    hashed_token: &str,
) -> Result<Option<SessionUser>> {
    let row = sqlx::query_as::<_, SessionUser>(
        "SELECT u.id, u.email, u.name \
         FROM sessions s JOIN users u ON u.id = s.user_id \
         WHERE s.id = $1 AND s.expires_at > NOW()",
    )
    .bind(hashed_token)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_session(pool: &PgPool, hashed_token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(hashed_token)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deliberately derives NEITHER `Debug` nor `Clone`, unlike the other row
/// types in this module. All three fields are plaintext single-use
/// secrets, and a derived `Debug` is exactly how they end up in a log line
/// or a panic message -- the same reasoning that made `AppState`'s and
/// `OidcConfig`'s `Debug` impls hand-rolled and secret-redacting (see
/// `crate::app` and `crate::auth::oidc`); there is nothing worth printing
/// here at all, so this one simply goes without. Nothing debug-formats or
/// clones one today, and leaving the derives in place is just an open
/// invitation for a future `tracing::debug!(?stored)` to change that.
#[derive(sqlx::FromRow)]
pub struct LoginState {
    pub pkce_verifier: String,
    pub nonce: String,
    pub csrf_state: String,
    pub return_to: Option<String>,
}

pub async fn insert_login_state(
    pool: &PgPool,
    id: &str,
    pkce_verifier: &str,
    nonce: &str,
    csrf_state: &str,
    return_to: Option<&str>,
) -> Result<()> {
    // Opportunistic cleanup -- no cron needed for a table this small and
    // self-limiting; every login attempt takes out its own trash.
    sqlx::query("DELETE FROM oidc_login_state WHERE created_at < NOW() - INTERVAL '15 minutes'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO oidc_login_state (id, pkce_verifier, nonce, csrf_state, return_to, created_at) \
         VALUES ($1, $2, $3, $4, $5, NOW())",
    )
    .bind(id)
    .bind(pkce_verifier)
    .bind(nonce)
    .bind(csrf_state)
    .bind(return_to)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetches and deletes in one step -- login state is single-use by
/// construction (a replayed callback with the same state must not
/// succeed twice). `None` if the id is unknown, already consumed, or
/// older than the 15-minute window `insert_login_state` also sweeps on.
pub async fn consume_login_state(pool: &PgPool, id: &str) -> Result<Option<LoginState>> {
    let row = sqlx::query_as::<_, LoginState>(
        "DELETE FROM oidc_login_state \
         WHERE id = $1 AND created_at > NOW() - INTERVAL '15 minutes' \
         RETURNING pkce_verifier, nonce, csrf_state, return_to",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod db_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                session_round_trip_creates_looks_up_and_deletes -- --ignored`"]
    async fn session_round_trip_creates_looks_up_and_deletes() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let identity = OidcIdentity {
            sub: "TEST-USER-ROUND-TRIP".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
        };
        let user = upsert_user(&pool, &identity).await.expect("upsert user");
        assert_eq!(user.id, "TEST-USER-ROUND-TRIP");

        insert_session(&pool, "test-hashed-token", &user.id, 14)
            .await
            .expect("insert session");

        let found = get_session_with_user(&pool, "test-hashed-token")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(found.id, "TEST-USER-ROUND-TRIP");

        delete_session(&pool, "test-hashed-token")
            .await
            .expect("delete session");
        let gone = get_session_with_user(&pool, "test-hashed-token")
            .await
            .expect("lookup after delete");
        assert!(gone.is_none());

        // Cleanup -- cascades to sessions via ON DELETE CASCADE, though
        // the session row above was already explicitly deleted.
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-ROUND-TRIP'")
            .execute(&pool)
            .await
            .expect("cleanup test user");
    }
}
