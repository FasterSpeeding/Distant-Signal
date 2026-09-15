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

/// A claim that is PRESENT but blank (`""`, or whitespace only) carries
/// exactly as much information as an absent one, and every consumer of
/// `users.name`/`users.email` in this app treats "absent" as "fall back to
/// something else" -- so the two have to be made indistinguishable here,
/// at the boundary, rather than at each of those consumers.
///
/// This is not hypothetical: an identity provider with no name on file for
/// a user generally sends `"name": ""` rather than omitting the claim
/// (Authentik's own `profile` scope mapping returns the user's `name`
/// attribute verbatim, and that attribute is optional and defaults to the
/// empty string). Stored raw, that empty string answers every
/// `Option`-shaped "does this user have a name?" downstream with a
/// confident yes -- `Some("")` is `Some`, and `"" ?? placeholder` is `""`
/// -- and so gets rendered as the label itself: a group member row that is
/// just a role badge, a "Shared by " with nothing after it, an empty
/// nav-bar label.
///
/// Also trims: a name of `"  Ada  "` is `"Ada"`, never rendered with its
/// padding intact.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// The one label this app is willing to show OTHER people for a user:
/// their `users.name`, else their `users.username`, else nothing at all.
///
/// Deliberately never the email, which is what the group member list and
/// shared-train attribution used to fall back to. A group is a set of
/// people who may have joined via nothing but a link, and shipping one
/// member's email address to the rest of them is a privacy leak, not a
/// display-name fallback -- so the fallback is the OTHER non-email
/// identifier this app stores: `username`, from the standard
/// `preferred_username` claim (`auth::oidc`). `id` is not a candidate at
/// any point: it is the opaque OIDC subject, not a name.
///
/// `None` -- nothing usable on file -- is the signal for the frontend to
/// render its own generic placeholder ("A member" / "a member") instead.
pub fn display_label(name: Option<String>, username: Option<String>) -> Option<String> {
    shareable(name.as_deref())
        .or_else(|| shareable(username.as_deref()))
        .map(str::to_string)
}

/// `non_blank`, plus: not an email address.
///
/// Not selecting `users.email` is only half of "never show one member's
/// email to the others" -- the other half is that an IdP is perfectly
/// entitled to put an email address in the claims we DO show. Authentik
/// can be configured with a user's email as their username, and plenty of
/// directories set the `name` attribute to the email for accounts created
/// in bulk. A value containing `@` is treated as an email and declined,
/// falling through to the next candidate and ultimately to the generic
/// placeholder: "we can't say who this is" is the correct outcome there,
/// not "here is their email address".
///
/// `@` is a deliberately blunt test: the cost of a false negative -- a
/// leaked email address -- is much higher than the cost of a false
/// positive, which is a member shown as the generic placeholder.
///
/// Note what that means on some deployments. On Entra ID / Azure AD,
/// `preferred_username` IS the UPN and is therefore email-shaped for
/// essentially every user, so this fallback is inert there by
/// construction, not just occasionally -- everyone without a `name` shows
/// as "A member". That is the intended trade, not a bug to go debugging:
/// the alternative is showing the whole group an address.
fn shareable(value: Option<&str>) -> Option<&str> {
    non_blank(value).filter(|candidate| !candidate.contains('@'))
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
            preferred_username: Some("ada".to_string()),
            groups: Vec::new(),
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

    #[test]
    fn non_blank_keeps_a_real_value_but_trims_it() {
        assert_eq!(non_blank(Some("Ada Rider")), Some("Ada Rider"));
        assert_eq!(non_blank(Some("  Ada Rider  ")), Some("Ada Rider"));
    }

    #[test]
    fn non_blank_maps_every_shape_of_blank_onto_none() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(Some("")), None);
        assert_eq!(non_blank(Some("   ")), None);
        assert_eq!(non_blank(Some("\t\n")), None);
    }

    fn label(name: Option<&str>, username: Option<&str>) -> Option<String> {
        display_label(name.map(str::to_string), username.map(str::to_string))
    }

    #[test]
    fn display_label_prefers_the_name_and_trims_it() {
        assert_eq!(
            label(Some("Ada Rider"), Some("ada")),
            Some("Ada Rider".to_string())
        );
        assert_eq!(
            label(Some("  Ada Rider  "), Some("ada")),
            Some("Ada Rider".to_string())
        );
    }

    /// Half the reported bug: a present-but-blank `name` is not a label.
    /// It used to be treated as one (`Some("")` is `Some`), which is what
    /// rendered an empty member row and a "Shared by " with nothing after
    /// it. It now falls through to the username, exactly as an absent name
    /// does.
    #[test]
    fn display_label_falls_through_a_blank_name_to_the_username() {
        assert_eq!(label(Some(""), Some("ada")), Some("ada".to_string()));
        assert_eq!(label(Some("   "), Some("ada")), Some("ada".to_string()));
        assert_eq!(label(None, Some("  ada  ")), Some("ada".to_string()));
    }

    /// Never an email, and never the opaque `users.id`: with neither a
    /// name nor a username on file there is simply nothing this app is
    /// willing to show other members, and `None` is how it says so.
    #[test]
    fn display_label_is_none_when_neither_is_usable() {
        assert_eq!(label(Some(""), Some("")), None);
        assert_eq!(label(Some("   "), None), None);
        assert_eq!(label(None, None), None);
    }

    /// The other half of "never show one member's email to the rest of the
    /// group": not selecting `users.email` doesn't help if the IdP put an
    /// email address in the `name` or `preferred_username` claim instead,
    /// which plenty of directories do.
    #[test]
    fn display_label_declines_an_email_address_in_either_field() {
        assert_eq!(
            label(Some("rider@example.com"), Some("ada")),
            Some("ada".to_string())
        );
        assert_eq!(
            label(Some("Ada Rider"), Some("rider@example.com")),
            Some("Ada Rider".to_string())
        );
        assert_eq!(
            label(Some("rider@example.com"), Some("rider@example.com")),
            None
        );
        assert_eq!(label(None, Some("rider@example.com")), None);
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub username: Option<String>,
}

/// Creates the user on first login, or updates `email`/`name`/`groups`/
/// `last_login_at` on every return visit -- design doc: "upserted, not
/// just inserted once." `groups` is overwritten wholesale, never merged
/// with what was already stored -- see
/// docs/superpowers/specs/2026-09-02-mcp-server-oauth-access-groups-design.md's
/// Global Constraints: a group removed in Authentik is reflected on the
/// user's very next login.
///
/// `name`/`username`/`email` are normalized through `non_blank` on the way
/// in, so a blank claim is stored as SQL `NULL` rather than as an empty
/// string -- the read side (`display_label`, and the session shape's own
/// name-else-email fallback) then needs no special case for a value this
/// app never writes. The read side normalizes too, for rows written before
/// this normalization existed.
///
/// `username` (the `preferred_username` claim) is overwritten on every
/// login for the same reason `name` and `email` are: this table mirrors
/// what the IdP currently asserts, it is not an independent record.
pub async fn upsert_user(pool: &PgPool, identity: &OidcIdentity) -> Result<User> {
    let email = non_blank(verified_email(identity));
    let name = non_blank(identity.name.as_deref());
    let username = non_blank(identity.preferred_username.as_deref());
    let row = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, email, name, username, groups, created_at, last_login_at) \
         VALUES ($1, $2, $3, $4, $5, NOW(), NOW()) \
         ON CONFLICT (id) DO UPDATE SET \
            email = EXCLUDED.email, name = EXCLUDED.name, \
            username = EXCLUDED.username, groups = EXCLUDED.groups, \
            last_login_at = NOW() \
         RETURNING id, email, name, username",
    )
    .bind(&identity.sub)
    .bind(email)
    .bind(name)
    .bind(username)
    .bind(&identity.groups)
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
    pub groups: Vec<String>,
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
        "SELECT u.id, u.email, u.name, u.groups \
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

        let mut identity = OidcIdentity {
            sub: "TEST-USER-ROUND-TRIP".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            preferred_username: Some("test-rider".to_string()),
            groups: Vec::new(),
        };
        let user = upsert_user(&pool, &identity).await.expect("upsert user");
        assert_eq!(user.id, "TEST-USER-ROUND-TRIP");
        assert_eq!(user.name.as_deref(), Some("Test Rider"));
        assert_eq!(user.username.as_deref(), Some("test-rider"));

        // A blank `name` claim -- what an IdP with no name on file for the
        // user actually sends -- must be stored as NULL, not as `''`.
        // Storing `''` is what made `display_label`'s fallback (and the
        // frontend's own placeholder) fail to fire, rendering a member row
        // with an empty label. Same for a blank `preferred_username`, and
        // for the padding around an otherwise-real value.
        identity.name = Some("   ".to_string());
        identity.preferred_username = Some("  test-rider  ".to_string());
        let user = upsert_user(&pool, &identity).await.expect("re-upsert user");
        assert_eq!(user.name, None);
        assert_eq!(user.username.as_deref(), Some("test-rider"));

        identity.preferred_username = Some(String::new());
        let user = upsert_user(&pool, &identity).await.expect("re-upsert user");
        assert_eq!(user.username, None);

        identity.name = Some("Test Rider".to_string());

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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                groups_are_overwritten_not_merged_on_repeat_login -- --ignored`"]
    async fn groups_are_overwritten_not_merged_on_repeat_login() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let mut identity = OidcIdentity {
            sub: "TEST-USER-GROUPS-OVERWRITE".to_string(),
            email: Some("test@example.com".to_string()),
            email_verified: true,
            name: Some("Test Rider".to_string()),
            preferred_username: Some("test-rider".to_string()),
            groups: vec!["mcp-users".to_string(), "mcp-live-boards".to_string()],
        };
        let user = upsert_user(&pool, &identity)
            .await
            .expect("first login upsert");

        insert_session(&pool, "test-hashed-token-groups", &user.id, 14)
            .await
            .expect("insert session");
        let found = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should exist");
        assert_eq!(
            found.groups,
            vec!["mcp-users".to_string(), "mcp-live-boards".to_string()]
        );

        // Second login, with mcp-live-boards removed in Authentik -- must
        // be reflected exactly, not unioned with the first login's set.
        identity.groups = vec!["mcp-users".to_string()];
        upsert_user(&pool, &identity)
            .await
            .expect("second login upsert");
        let found_again = get_session_with_user(&pool, "test-hashed-token-groups")
            .await
            .expect("lookup session")
            .expect("session should still exist");
        assert_eq!(found_again.groups, vec!["mcp-users".to_string()]);

        delete_session(&pool, "test-hashed-token-groups")
            .await
            .expect("delete session");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-USER-GROUPS-OVERWRITE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
