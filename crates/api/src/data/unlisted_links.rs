//! Unlisted links: a generic, reusable share-link primitive backing the
//! `unlisted_links` table. One opaque, high-entropy token resolves to one
//! `(resource_type, resource_id)` pair -- polymorphic by a bare string
//! discriminator rather than a typed foreign key, so a future resource
//! type can mint its own links by calling the functions below with its own
//! `resource_type` string and a fresh thin wrapper, with no schema change
//! and no change to this module. See
//! docs/superpowers/specs/2026-09-23-unlisted-links-design.md (§3) for the
//! full reasoning, including why this is a new table rather than a
//! generalization of `group_invite_links` (`crate::data::groups`): that
//! table carries a group-specific side effect (`consume_invite_link`'s
//! membership insert) with no generic equivalent, and a hard, non-optional
//! 7-day TTL that this table's callers must be free to opt out of.
//!
//! This module intentionally knows nothing about journeys, groups, or any
//! other concrete resource -- `resource_type`/`resource_id` are plain
//! `&str`s the caller supplies. It also has no `consume`-shaped function:
//! `resolve_link` never mutates, matching `resolve_invite_link`'s (not
//! `consume_invite_link`'s) semantics. A resource type that needs
//! consume-once behaviour builds it as its own wrapper around
//! `resolve_link`.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// A freshly minted link, as returned by [`rotate_link`] -- the ONLY point
/// at which the plaintext `token` exists server-side. See
/// [`hash_link_token`]: the table stores only its SHA-256, so an
/// already-existing link's token can never be read back again (see
/// [`ActiveLink`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlistedLink {
    pub token: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// What [`get_active_link`] can say about an already-existing link: that it
/// exists, and when it expires -- never its token, which isn't stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLink {
    pub expires_at: Option<DateTime<Utc>>,
}

/// `unlisted_links.token_hash` (and `group_invite_links.token_hash`, see
/// `groups::rotate_invite_link`) store THIS, never the raw token
/// (2026-09-26 review, L14) -- exactly the scheme `sessions.id` already
/// uses, by calling the very same function (`auth::hash_session_token`,
/// SHA-256, lowercase hex): the token is 32 random bytes, so an unsalted
/// fast hash is sufficient (nothing to brute-force), and a database
/// read -- a backup, a replica, a leaked dump -- no longer hands out
/// working share/invite links. Every lookup hashes the incoming token and
/// compares hashes; nothing compares or stores plaintext.
pub fn hash_link_token(token: &str) -> String {
    crate::auth::hash_session_token(token)
}

/// Revokes any currently-active link for `(resource_type, resource_id)` and
/// inserts a fresh one, in one transaction -- exactly
/// `groups::rotate_invite_link`'s own shape, generalized. `ttl` is the
/// caller's own choice per resource type: `Some(d)` sets `expires_at =
/// NOW() + d`, `None` leaves it NULL (no forced expiry). Used for both
/// "create the first link" and "regenerate" -- there is no separate
/// create-only function, matching `rotate_invite_link`'s own dual role.
///
/// **Concurrent rotations of the same resource are serialized by the
/// schema, not by this function.** Two simultaneous calls for the same
/// `(resource_type, resource_id)` can both run their own `UPDATE ...
/// revoked_at IS NULL` (READ COMMITTED doesn't block that on its own) and
/// both attempt to INSERT a fresh row -- `unlisted_links_one_active_per_resource`
/// (`20260925091000_unlisted_links_one_active_per_resource.sql`), a partial
/// unique index on `(resource_type, resource_id) WHERE revoked_at IS
/// NULL`, means at most one of those two inserts can commit; the loser's
/// `?` on the `INSERT` propagates the constraint-violation as a plain
/// `anyhow::Error` (the route maps it to the same 500 path an ordinary
/// repeat-click race already gets elsewhere in this codebase -- see
/// `journeys::owned_next_leg_order`'s own doc comment for the same
/// accepted posture). Without that index, both inserts used to succeed,
/// leaving two simultaneously active tokens for the same resource until
/// the next revoke.
pub async fn rotate_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    created_by: &str,
    ttl: Option<Duration>,
) -> anyhow::Result<UnlistedLink> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE unlisted_links SET revoked_at = NOW() \
         WHERE resource_type = $1 AND resource_id = $2 AND revoked_at IS NULL",
    )
    .bind(resource_type)
    .bind(resource_id)
    .execute(&mut *tx)
    .await?;

    let token = crate::auth::generate_session_token();
    let expires_at = ttl.map(|d| Utc::now() + d);
    sqlx::query(
        "INSERT INTO unlisted_links \
            (token_hash, resource_type, resource_id, created_by, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, NOW(), $5)",
    )
    .bind(hash_link_token(&token))
    .bind(resource_type)
    .bind(resource_id)
    .bind(created_by)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(UnlistedLink { token, expires_at })
}

/// Revokes the active link for `(resource_type, resource_id)`, no
/// replacement. Idempotent: returns `false` if there was nothing active
/// to revoke (not an error) -- same contract as `revoke_invite_link`.
pub async fn revoke_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE unlisted_links SET revoked_at = NOW() \
         WHERE resource_type = $1 AND resource_id = $2 AND revoked_at IS NULL",
    )
    .bind(resource_type)
    .bind(resource_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The active link for `(resource_type, resource_id)`, if any -- `None`
/// once revoked or past `expires_at` (when set). Same validity predicate
/// as `get_active_invite_link`: `revoked_at IS NULL AND (expires_at IS
/// NULL OR expires_at > NOW())`.
pub async fn get_active_link(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
) -> anyhow::Result<Option<ActiveLink>> {
    let row: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
        "SELECT expires_at FROM unlisted_links \
         WHERE resource_type = $1 AND resource_id = $2 AND revoked_at IS NULL \
           AND (expires_at IS NULL OR expires_at > NOW()) \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(resource_type)
    .bind(resource_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(expires_at,)| ActiveLink { expires_at }))
}

/// One resolved `(resource_type, resource_id)` pair for a token -- `None`
/// if the token doesn't exist or fails the same validity predicate above.
/// Never mutates (this module has no `consume`-shaped function at all --
/// see the design doc's Non-goals: a future resource type that needs
/// consume-once semantics builds it as its own wrapper around this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    pub resource_type: String,
    pub resource_id: String,
}

pub async fn resolve_link(pool: &PgPool, token: &str) -> anyhow::Result<Option<ResolvedLink>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT resource_type, resource_id FROM unlisted_links \
         WHERE token_hash = $1 AND revoked_at IS NULL \
           AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(hash_link_token(token))
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(resource_type, resource_id)| ResolvedLink {
        resource_type,
        resource_id,
    }))
}

/// How long a revoked or expired link row is kept before
/// [`prune_dead_links`] deletes it. Long enough that a "why did my link stop
/// working" support question can still be answered from the table for a
/// while; short enough that rows from every regenerate click (each one
/// revokes the previous row, and nothing ever deleted them) don't pile up
/// forever.
pub const DEAD_LINK_RETENTION: Duration = Duration::days(30);

/// Deletes `unlisted_links` AND `group_invite_links` rows that have been
/// revoked, or have expired, for longer than [`DEAD_LINK_RETENTION`]
/// (2026-09-26 review, L10: revoked rows were never pruned, so every
/// Regenerate/rotate left a permanent dead row behind). Neither table's
/// dead rows can ever resolve again -- both validity predicates require
/// `revoked_at IS NULL` and an unexpired `expires_at` -- so deleting them
/// changes no behavior. Run from `main.rs`'s hourly
/// `session_cleanup_sweep_loop`, the same periodic-cleanup home
/// `users::prune_expired_sessions` already uses. Returns the number of
/// rows deleted across both tables, for logging only.
pub async fn prune_dead_links(pool: &PgPool) -> anyhow::Result<u64> {
    let cutoff = Utc::now() - DEAD_LINK_RETENTION;
    let unlisted =
        sqlx::query("DELETE FROM unlisted_links WHERE revoked_at < $1 OR expires_at < $1")
            .bind(cutoff)
            .execute(pool)
            .await?;
    let invites =
        sqlx::query("DELETE FROM group_invite_links WHERE revoked_at < $1 OR expires_at < $1")
            .bind(cutoff)
            .execute(pool)
            .await?;
    Ok(unlisted.rows_affected() + invites.rows_affected())
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(format!("{id}@example.com"))
        .bind(id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    async fn cleanup(pool: &PgPool, resource_type: &str, user_ids: &[&str]) {
        sqlx::query("DELETE FROM unlisted_links WHERE resource_type = $1")
            .bind(resource_type)
            .execute(pool)
            .await
            .ok();
        for id in user_ids {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .ok();
        }
    }

    /// L14: only SHA-256(token) is stored, and it's the exact same value
    /// the migration (`20260926150000_hash_share_and_invite_tokens.sql`)
    /// computes in SQL for pre-existing rows -- so a link minted before
    /// that migration keeps resolving after it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                link_tokens_are_stored_hashed_matching_the_migrations_sql_hash -- --ignored --test-threads=1`"]
    async fn link_tokens_are_stored_hashed_matching_the_migrations_sql_hash() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-L14").await;
        let link = rotate_link(&pool, "widget-l14", "1", "TEST-UNLISTED-LINKS-L14", None)
            .await
            .expect("rotate");

        let stored: String = sqlx::query_scalar(
            "SELECT token_hash FROM unlisted_links WHERE resource_type = 'widget-l14'",
        )
        .fetch_one(&pool)
        .await
        .expect("read stored hash");
        assert_ne!(stored, link.token, "plaintext token must never be stored");
        assert_eq!(stored, hash_link_token(&link.token));

        let sql_hash: String =
            sqlx::query_scalar("SELECT encode(sha256(convert_to($1, 'UTF8')), 'hex')")
                .bind(&link.token)
                .fetch_one(&pool)
                .await
                .expect("sql hash");
        assert_eq!(
            sql_hash, stored,
            "migration's SQL hash must match the Rust hash"
        );

        cleanup(&pool, "widget-l14", &["TEST-UNLISTED-LINKS-L14"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                rotate_link_creates_a_link_and_revokes_any_previous_active_one -- --ignored --test-threads=1`"]
    async fn rotate_link_creates_a_link_and_revokes_any_previous_active_one() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-1").await;

        let first = rotate_link(&pool, "widget", "42", "TEST-UNLISTED-LINKS-OWNER-1", None)
            .await
            .expect("first rotate");
        let second = rotate_link(&pool, "widget", "42", "TEST-UNLISTED-LINKS-OWNER-1", None)
            .await
            .expect("second rotate");
        assert_ne!(first.token, second.token);

        get_active_link(&pool, "widget", "42")
            .await
            .expect("query")
            .expect("should have an active link");
        assert!(
            resolve_link(&pool, &second.token)
                .await
                .expect("query")
                .is_some(),
            "the newest link should be active"
        );
        assert_eq!(
            resolve_link(&pool, &first.token).await.expect("query"),
            None,
            "only the newest link should be active"
        );

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                concurrent_rotations_never_leave_two_active_tokens -- --ignored --test-threads=1`"]
    async fn concurrent_rotations_never_leave_two_active_tokens() {
        // Regression test for the 19-pass security/bug review's journeys-
        // area Medium finding 2: two concurrent `rotate_link` calls for the
        // SAME resource used to both commit, leaving two simultaneously
        // active tokens (`resolve_link` honoring both) until the next
        // revoke -- see `20260925091000_unlisted_links_one_active_per_resource.sql`
        // and this function's own updated doc comment for the fix (a
        // partial unique index on `(resource_type, resource_id) WHERE
        // revoked_at IS NULL`).
        //
        // This is deliberately NOT `tokio::join!(rotate_link(...),
        // rotate_link(...))` against a shared pool -- that was this test's
        // first shape, and it turned out not to be reliable: against this
        // environment's local Postgres, the two calls' network round trips
        // consistently resolved fully sequentially (one `rotate_link` ran
        // to completion, including its own commit, before the other's
        // first statement was even sent), so the actual race this finding
        // describes never occurred, and the test could pass for the wrong
        // reason. Instead, this issues the SAME bare `INSERT` `rotate_link`
        // itself runs, directly, AFTER a first rotation has already
        // committed and is active -- deterministically reproducing the
        // exact moment a second, concurrently-racing `rotate_link` call
        // would reach its own INSERT "without having seen" the first's
        // already-active row (the finding's own phrasing). Before
        // `unlisted_links_one_active_per_resource` existed, this INSERT
        // would have silently succeeded, leaving two simultaneously active
        // tokens; the schema itself must now reject it.
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-CONCURRENT").await;

        let first = rotate_link(
            &pool,
            "widget",
            "99",
            "TEST-UNLISTED-LINKS-CONCURRENT",
            None,
        )
        .await
        .expect("first rotation");

        let racing_insert = sqlx::query(
            "INSERT INTO unlisted_links \
                (token_hash, resource_type, resource_id, created_by, created_at, expires_at) \
             VALUES ($1, 'widget', '99', $2, NOW(), NULL)",
        )
        .bind("TEST-CONCURRENT-RACE-TOKEN")
        .bind("TEST-UNLISTED-LINKS-CONCURRENT")
        .execute(&pool)
        .await;
        assert!(
            racing_insert.is_err(),
            "a second active row for the same resource must be rejected by the schema \
             itself, not silently coexist with the first"
        );

        let (active_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM unlisted_links \
             WHERE resource_type = $1 AND resource_id = $2 AND revoked_at IS NULL",
        )
        .bind("widget")
        .bind("99")
        .fetch_one(&pool)
        .await
        .expect("count active links");
        assert_eq!(
            active_count, 1,
            "exactly one active token must survive a concurrent rotation, never two"
        );

        assert!(
            resolve_link(&pool, &first.token)
                .await
                .expect("query")
                .is_some(),
            "the first rotation's own token must remain the sole active one"
        );

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-CONCURRENT"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                revoke_link_is_idempotent_and_clears_the_active_link -- --ignored --test-threads=1`"]
    async fn revoke_link_is_idempotent_and_clears_the_active_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-2").await;
        rotate_link(&pool, "widget", "42", "TEST-UNLISTED-LINKS-OWNER-2", None)
            .await
            .expect("rotate");

        assert!(
            revoke_link(&pool, "widget", "42")
                .await
                .expect("first revoke")
        );
        assert!(
            !revoke_link(&pool, "widget", "42")
                .await
                .expect("second revoke is a no-op")
        );
        assert_eq!(
            get_active_link(&pool, "widget", "42").await.expect("query"),
            None
        );

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                resolve_link_returns_none_for_an_expired_link -- --ignored --test-threads=1`"]
    async fn resolve_link_returns_none_for_an_expired_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-3").await;
        let token = "test-expired-unlisted-token";
        sqlx::query(
            "INSERT INTO unlisted_links \
                (token_hash, resource_type, resource_id, created_by, expires_at) \
             VALUES ($1, $2, $3, $4, NOW() - INTERVAL '1 hour')",
        )
        .bind(hash_link_token(token))
        .bind("widget")
        .bind("42")
        .bind("TEST-UNLISTED-LINKS-OWNER-3")
        .execute(&pool)
        .await
        .expect("seed an expired link");

        assert_eq!(resolve_link(&pool, token).await.expect("query"), None);

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-3"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                resolve_link_returns_none_for_a_revoked_link -- --ignored --test-threads=1`"]
    async fn resolve_link_returns_none_for_a_revoked_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-4").await;
        let link = rotate_link(&pool, "widget", "42", "TEST-UNLISTED-LINKS-OWNER-4", None)
            .await
            .expect("rotate");
        revoke_link(&pool, "widget", "42").await.expect("revoke");

        assert_eq!(resolve_link(&pool, &link.token).await.expect("query"), None);

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-4"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                resolve_link_returns_the_resource_for_a_link_with_no_expiry -- --ignored --test-threads=1`"]
    async fn resolve_link_returns_the_resource_for_a_link_with_no_expiry() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-5").await;
        let link = rotate_link(&pool, "widget", "42", "TEST-UNLISTED-LINKS-OWNER-5", None)
            .await
            .expect("rotate with no ttl");
        assert_eq!(link.expires_at, None);

        let resolved = resolve_link(&pool, &link.token)
            .await
            .expect("query")
            .expect("should resolve");
        assert_eq!(
            resolved,
            ResolvedLink {
                resource_type: "widget".into(),
                resource_id: "42".into(),
            }
        );

        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-5"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                resolve_link_never_confuses_two_different_resources_sharing_an_id -- --ignored --test-threads=1`"]
    async fn resolve_link_never_confuses_two_different_resources_sharing_an_id() {
        let pool = connect().await;
        seed_user(&pool, "TEST-UNLISTED-LINKS-OWNER-6").await;

        let widget_link = rotate_link(&pool, "widget", "1", "TEST-UNLISTED-LINKS-OWNER-6", None)
            .await
            .expect("rotate widget link");
        let gadget_link = rotate_link(&pool, "gadget", "1", "TEST-UNLISTED-LINKS-OWNER-6", None)
            .await
            .expect("rotate gadget link");

        let widget_resolved = resolve_link(&pool, &widget_link.token)
            .await
            .expect("query")
            .expect("widget link should resolve");
        let gadget_resolved = resolve_link(&pool, &gadget_link.token)
            .await
            .expect("query")
            .expect("gadget link should resolve");

        assert_eq!(
            widget_resolved,
            ResolvedLink {
                resource_type: "widget".into(),
                resource_id: "1".into(),
            }
        );
        assert_eq!(
            gadget_resolved,
            ResolvedLink {
                resource_type: "gadget".into(),
                resource_id: "1".into(),
            }
        );

        sqlx::query("DELETE FROM unlisted_links WHERE resource_type = $1")
            .bind("gadget")
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, "widget", &["TEST-UNLISTED-LINKS-OWNER-6"]).await;
    }
}
